//! Porte de `src/vs/base/node/zip.ts`: extração e leitura de ZIP em disco (`extract`, `buffer`), a validação de
//! limites (`validateZip`) e a escrita com deflate (`zip`).
//!
//! O original usa `yauzl` (leitura) e `yazl` (escrita); aqui é a crate `zip` em Rust puro (deflate por `flate2` com
//! o backend `zlib-rs`), a mesma que o `je-extensions` usa para o VSIX.
//!
//! Notas de tradução:
//! - O original é assíncrono (streams e promessas); aqui tudo é bloqueante, e quem precisa de assíncrono executa a
//!   chamada fora do laço de eventos. O cancelamento (`CancellationToken`) é conferido a cada entrada e a cada bloco
//!   de 64 KiB lido, o que equivale ao `destroy` dos streams do original.
//! - Os erros são o [`Error`] do `je-base`. O `ExtractError` do original vira [`ExtractError`], que se converte em
//!   [`Error`] (o `type` fica na propriedade `type`, ver [`Error::prop`]).
//! - `yauzl` recusa, ao ler cada entrada, nome com `\`, absoluto ou com segmento `..` ([`validate_file_name`]); é essa
//!   a defesa contra um ZIP que escreve fora do destino. Soma-se a conferência do `extractEntry` (o diretório da
//!   entrada precisa começar com o destino).
//! - Entrada que é link simbólico não é tratada à parte pelo original: o conteúdo (o alvo do link) é gravado como arquivo
//!   comum, e o modo (`modeFromEntry`) é aplicado na criação do arquivo, só em Unix.
//! - `sourcePath` do original é um padrão de `RegExp` (`new RegExp('^' + sourcePath)`), então aqui também é
//!   compilado pelo `zyarr`, com a mesma semântica.
//! - Um ZIP cortado no meio dos dados de uma entrada (fim de arquivo inesperado) vira o erro `Incomplete` do original
//!   (`Incomplete. Found N of M entries`); o `CorruptZip` é o ZIP em que a crate não acha ou não entende o diretório
//!   central (o `end of central directory record signature not found` do `yauzl`).

use std::fmt;
use std::fs::{self, File, OpenOptions};
use std::io::{self, BufReader, BufWriter, Read, Write};
use std::path::{Path, PathBuf};

use zip::result::ZipError;
use zip::write::SimpleFileOptions;
use zip::{CompressionMethod, ZipArchive, ZipWriter};

use crate::cancellation::CancellationToken;
use crate::errors::Error;
use crate::js_regexp::JsRegExp;
use crate::path::{PathModule, ProcessEnvironment};

/// `CorruptZipMessage`: a mensagem do `yauzl` para um arquivo que não é ZIP.
pub const CORRUPT_ZIP_MESSAGE: &str = "end of central directory record signature not found";

/// O tamanho do bloco de leitura (o `highWaterMark` padrão dos streams de arquivo do Node).
const CHUNK_SIZE: usize = 64 * 1024;

/// `IExtractOptions`.
#[derive(Clone, Debug, Default)]
pub struct ExtractOptions {
    /// Apaga o destino antes de extrair.
    pub overwrite: bool,
    /// Caminho de origem dentro do ZIP (um padrão de `RegExp`): só os arquivos dentro dele são extraídos, sem o prefixo.
    pub source_path: Option<String>,
}

/// `ExtractErrorType`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ExtractErrorType {
    CorruptZip,
    Incomplete,
}

impl ExtractErrorType {
    /// O texto do original (`'CorruptZip'`, `'Incomplete'`).
    pub fn as_str(self) -> &'static str {
        match self {
            Self::CorruptZip => "CorruptZip",
            Self::Incomplete => "Incomplete",
        }
    }
}

/// `ExtractError`.
#[derive(Clone, Debug, PartialEq)]
pub struct ExtractError {
    pub error_type: Option<ExtractErrorType>,
    pub message: String,
    pub cause: Error,
}

impl ExtractError {
    pub fn new(error_type: Option<ExtractErrorType>, cause: Error) -> Self {
        let message = match error_type {
            Some(ExtractErrorType::CorruptZip) => format!("Corrupt ZIP: {}", cause.message),
            _ => cause.message.clone(),
        };
        Self { error_type, message, cause }
    }
}

impl fmt::Display for ExtractError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.message)
    }
}

impl std::error::Error for ExtractError {}

impl From<ExtractError> for Error {
    fn from(error: ExtractError) -> Error {
        let converted = Error::new(error.message).with_cause(error.cause);
        match error.error_type {
            Some(error_type) => converted.with_prop("type", error_type.as_str()),
            None => converted,
        }
    }
}

fn io_error(error: io::Error) -> Error {
    Error::new(error.to_string())
}

fn zip_error(error: ZipError) -> Error {
    Error::new(error.to_string())
}

/// Um erro da leitura de uma entrada, já no formato do `toExtractError` (sem tipo).
fn untyped_extract_error(cause: Error) -> Error {
    ExtractError::new(None, cause).into()
}

/// `modeFromEntry`: o modo Unix do atributo externo (o padrão `0o100644` quando não há), com o tipo de arquivo
/// (`S_IFMT`) e as permissões (`S_IRWXU`, `S_IRWXG`, `S_IRWXO`).
fn mode_from_attributes(attributes: u32) -> u32 {
    let attr = if attributes == 0 { 33188 } else { attributes };
    [448, 56, 7].iter().map(|mask| attr & mask).sum::<u32>() + (attr & 61440)
}

/// A validação de nome do `yauzl` (`validateFileName`), feita em cada entrada lida.
pub fn validate_file_name(file_name: &str) -> Option<String> {
    if file_name.contains('\\') {
        return Some(format!("invalid characters in fileName: {file_name}"));
    }
    let bytes = file_name.as_bytes();
    let has_drive = bytes.len() >= 2 && bytes[0].is_ascii_alphabetic() && bytes[1] == b':';
    if has_drive || file_name.starts_with('/') {
        return Some(format!("absolute path: {file_name}"));
    }
    if file_name.split('/').any(|segment| segment == "..") {
        return Some(format!("invalid relative path: {file_name}"));
    }
    None
}

/// `openZip`: abre o ZIP. Um arquivo que a crate não reconhece como ZIP é o `CorruptZip`.
fn open_zip(zip_path: &Path) -> Result<ZipArchive<BufReader<File>>, Error> {
    let file = File::open(zip_path).map_err(|e| untyped_extract_error(io_error(e)))?;
    ZipArchive::new(BufReader::new(file)).map_err(|e| {
        let error_type = matches!(e, ZipError::InvalidArchive(_) | ZipError::UnsupportedArchive(_)).then_some(ExtractErrorType::CorruptZip);
        ExtractError::new(error_type, zip_error(e)).into()
    })
}

/// Lê `reader` em blocos de [`CHUNK_SIZE`] e entrega cada um a `visit`, que devolve `false` para parar.
/// `read_error` traduz a falha de leitura (a de `visit` já vem pronta).
fn for_each_chunk(
    reader: &mut impl Read,
    read_error: impl Fn(io::Error) -> Error,
    mut visit: impl FnMut(&[u8]) -> Result<bool, Error>,
) -> Result<(), Error> {
    let mut chunk = vec![0u8; CHUNK_SIZE];
    loop {
        let read = match reader.read(&mut chunk) {
            Ok(0) => return Ok(()),
            Ok(read) => read,
            Err(e) if e.kind() == io::ErrorKind::Interrupted => continue,
            Err(e) => return Err(read_error(e)),
        };
        if !visit(&chunk[..read])? {
            return Ok(());
        }
    }
}

/// `Promises.rm`: apaga arquivo ou pasta, e não é erro o caminho não existir.
fn remove_all(path: &Path) -> io::Result<()> {
    match fs::symlink_metadata(path) {
        Ok(metadata) if metadata.is_dir() => fs::remove_dir_all(path),
        Ok(_) => fs::remove_file(path),
        Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(e) => Err(e),
    }
}

/// `createWriteStream(path, { mode })`: cria ou trunca o arquivo, com o modo dado (só em Unix).
fn open_for_write(path: &str, mode: u32) -> io::Result<File> {
    let mut options = OpenOptions::new();
    options.write(true).create(true).truncate(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(mode & 0o777);
    }
    #[cfg(not(unix))]
    let _ = mode;
    options.open(path)
}

/// `entry.fileName.replace(sourcePathRegex, '')`: `None` quando o padrão não casa com o nome (a entrada é
/// pulada); senão o nome sem o trecho casado.
fn strip_source_path(source_path_regex: &JsRegExp, file_name: &str) -> Option<String> {
    let units: Vec<u16> = file_name.encode_utf16().collect();
    let found = source_path_regex.exec(&units)?;
    let mut rest = units[..found.start()].to_vec();
    rest.extend_from_slice(&units[found.end()..]);
    Some(String::from_utf16_lossy(&rest))
}

/// `extractEntry`: grava o conteúdo da entrada em `target/file_name`. `completed` e `total` montam o erro
/// `Incomplete` quando os dados acabam antes do tempo.
fn extract_entry(
    entry: &mut impl Read,
    file_name: &str,
    mode: u32,
    target_path: &str,
    token: &CancellationToken,
    (completed, total): (usize, usize),
) -> Result<(), Error> {
    let env = ProcessEnvironment;
    let path = PathModule::native(&env);
    let dir_name = path.dirname(file_name);
    let target_dir_name = path.join(&[target_path, &dir_name]);
    if !target_dir_name.starts_with(target_path) {
        return Err(Error::new(format!("Error extracting {file_name}. Invalid file.")));
    }
    let target_file_name = path.join(&[target_path, file_name]);

    fs::create_dir_all(&target_dir_name).map_err(io_error)?;
    if token.is_cancellation_requested() {
        return Ok(());
    }

    let mut output = open_for_write(&target_file_name, mode).map_err(io_error)?;
    let read_error = |e: io::Error| {
        if e.kind() == io::ErrorKind::UnexpectedEof {
            let cause = Error::new(format!("Incomplete. Found {completed} of {total} entries"));
            ExtractError::new(Some(ExtractErrorType::Incomplete), cause).into()
        } else {
            io_error(e)
        }
    };
    for_each_chunk(entry, read_error, |chunk| {
        if token.is_cancellation_requested() {
            return Ok(false);
        }
        output.write_all(chunk).map_err(io_error)?;
        Ok(true)
    })
}

/// `extractZip`: percorre as entradas na ordem do diretório central.
fn extract_zip(archive: &mut ZipArchive<BufReader<File>>, target_path: &str, source_path_regex: &JsRegExp, token: &CancellationToken) -> Result<(), Error> {
    let env = ProcessEnvironment;
    let path = PathModule::native(&env);
    let total = archive.len();
    for index in 0..total {
        if token.is_cancellation_requested() {
            return Ok(());
        }
        let mut entry = archive.by_index(index).map_err(zip_error)?;
        if let Some(message) = validate_file_name(entry.name()) {
            return Err(Error::new(message));
        }
        let Some(file_name) = strip_source_path(source_path_regex, entry.name()) else { continue };

        // Nomes de diretório terminam em '/'.
        if file_name.ends_with('/') {
            fs::create_dir_all(path.join(&[target_path, &file_name])).map_err(io_error)?;
            continue;
        }

        let mode = mode_from_attributes(entry.unix_mode().unwrap_or(0) & 0xffff);
        extract_entry(&mut entry, &file_name, mode, target_path, token, (index, total))?;
    }
    Ok(())
}

/// `extract`: extrai o ZIP para `target_path`. Cancelar o `token` interrompe sem erro, como no original.
pub fn extract(zip_path: &Path, target_path: &Path, options: &ExtractOptions, token: &CancellationToken) -> Result<(), Error> {
    let pattern = match options.source_path.as_deref() {
        Some(source_path) if !source_path.is_empty() => format!("^{source_path}"),
        _ => String::new(),
    };
    let source_path_regex = JsRegExp::from_str(&pattern, "").map_err(|e| Error::named("SyntaxError", format!("Invalid regular expression: /{pattern}/: {e}")))?;

    let mut archive = open_zip(zip_path)?;
    if options.overwrite {
        remove_all(target_path).map_err(io_error)?;
    }
    extract_zip(&mut archive, &target_path.to_string_lossy(), &source_path_regex, token)
}

/// `buffer`: o conteúdo da entrada `file_path` do ZIP.
pub fn buffer(zip_path: &Path, file_path: &str) -> Result<Vec<u8>, Error> {
    let mut archive = open_zip(zip_path)?;
    for index in 0..archive.len() {
        let mut entry = archive.by_index(index).map_err(|e| untyped_extract_error(zip_error(e)))?;
        if let Some(message) = validate_file_name(entry.name()) {
            return Err(untyped_extract_error(Error::new(message)));
        }
        if entry.name() == file_path {
            let mut contents = Vec::new();
            entry.read_to_end(&mut contents).map_err(|e| untyped_extract_error(io_error(e)))?;
            return Ok(contents);
        }
    }
    Err(Error::new(format!("{file_path} not found inside zip.")))
}

/// `IZipValidationOptions`.
#[derive(Clone, Copy, Debug)]
pub struct ZipValidationOptions {
    pub max_entries: u64,
    pub max_uncompressed_size: Option<u64>,
}

/// `validateZip`: confere o número de entradas e o tamanho descomprimido declarado, sem ler os dados.
pub fn validate_zip(zip_path: &Path, options: &ZipValidationOptions) -> Result<(), Error> {
    let mut archive = open_zip(zip_path)?;
    let mut entries: u64 = 0;
    let mut uncompressed_size: u64 = 0;
    for index in 0..archive.len() {
        let entry = archive.by_index_raw(index).map_err(|e| untyped_extract_error(zip_error(e)))?;
        if let Some(message) = validate_file_name(entry.name()) {
            return Err(untyped_extract_error(Error::new(message)));
        }
        entries += 1;
        uncompressed_size += entry.size();
        if entries > options.max_entries {
            return Err(Error::new(format!("ZIP contains too many entries ({entries}; limit {})", options.max_entries)));
        }
        if let Some(limit) = options.max_uncompressed_size
            && uncompressed_size > limit
        {
            return Err(Error::new(format!("ZIP expands beyond the allowed size ({uncompressed_size} bytes; limit {limit} bytes)")));
        }
    }
    Ok(())
}

/// `IFile`: uma entrada para o [`zip`]. Com `contents` ela vem da memória; senão, de `local_path`. Sem nenhum dos
/// dois a entrada é ignorada (como no original).
#[derive(Clone, Debug, Default)]
pub struct ZipInput {
    pub compress: Option<bool>,
    pub path: String,
    pub contents: Option<Vec<u8>>,
    pub local_path: Option<PathBuf>,
    /// Com `local_path` (e sem `contents`), grava no máximo estes bytes do início do arquivo. O prefixo é limitado ao
    /// tamanho atual, então um arquivo rotacionado ou truncado ainda dá uma entrada válida.
    pub local_path_size: Option<u64>,
    /// Pula a entrada quando a origem local não pode ser aberta ou inspecionada.
    pub skip_source_errors: bool,
}

impl ZipInput {
    /// Uma entrada com o conteúdo em memória.
    pub fn from_contents(path: &str, contents: impl Into<Vec<u8>>) -> Self {
        Self { path: path.to_string(), contents: Some(contents.into()), ..Self::default() }
    }

    /// Uma entrada lida de um arquivo local.
    pub fn from_local(path: &str, local_path: impl Into<PathBuf>) -> Self {
        Self { path: path.to_string(), local_path: Some(local_path.into()), ..Self::default() }
    }
}

/// `IZipOptions`.
#[derive(Default)]
pub struct ZipOptions<'a> {
    pub token: Option<CancellationToken>,
    /// Chamado com o total de bytes de origem gravados até agora e o caminho da entrada.
    pub on_progress: Option<&'a mut dyn FnMut(u64, &str)>,
}

/// A validação de caminho do `yazl` (`validateMetadataPath`): `\` vira `/`, e caminho absoluto ou com `..` é erro.
fn validate_metadata_path(metadata_path: &str) -> Result<String, Error> {
    if metadata_path.is_empty() {
        return Err(Error::new("empty metadataPath"));
    }
    let normalized = metadata_path.replace('\\', "/");
    let bytes = normalized.as_bytes();
    let has_drive = bytes.len() >= 2 && bytes[0].is_ascii_alphabetic() && bytes[1] == b':';
    if has_drive || normalized.starts_with('/') {
        return Err(Error::new(format!("absolute path: {normalized}")));
    }
    if normalized.split('/').any(|segment| segment == "..") {
        return Err(Error::new(format!("invalid relative path: {normalized}")));
    }
    Ok(normalized)
}

/// Abre uma entrada no ZIP: deflate, a menos que `compress` seja `false`.
fn start_entry(writer: &mut ZipWriter<BufWriter<File>>, path: &str, compress: Option<bool>, size: u64) -> Result<(), Error> {
    let name = validate_metadata_path(path)?;
    let method = if compress == Some(false) { CompressionMethod::Stored } else { CompressionMethod::Deflated };
    let options = SimpleFileOptions::default().compression_method(method).large_file(size >= 0xffff_ffff);
    writer.start_file(name, options).map_err(zip_error)
}

/// `zip`: escreve o ZIP em `zip_path` e devolve o caminho. Cancelar o `token` falha com o erro de cancelamento
/// (e, como no original, deixa o arquivo parcial).
pub fn zip(zip_path: &Path, files: &[ZipInput], mut options: ZipOptions<'_>) -> Result<PathBuf, Error> {
    let token = options.token.clone().unwrap_or(CancellationToken::None);
    let check_cancelled = || if token.is_cancellation_requested() { Err(Error::cancellation()) } else { Ok(()) };
    check_cancelled()?;

    let mut writer = ZipWriter::new(BufWriter::new(File::create(zip_path).map_err(io_error)?));
    let mut bytes: u64 = 0;
    let mut report = |bytes: u64, path: &str| {
        if let Some(on_progress) = options.on_progress.as_mut() {
            on_progress(bytes, path);
        }
    };

    for file in files {
        check_cancelled()?;
        if let Some(contents) = &file.contents {
            start_entry(&mut writer, &file.path, file.compress, contents.len() as u64)?;
            writer.write_all(contents).map_err(io_error)?;
            bytes += contents.len() as u64;
            report(bytes, &file.path);
            continue;
        }
        let Some(local_path) = file.local_path.as_ref().filter(|p| !p.as_os_str().is_empty()) else { continue };

        // Tamanho e leitura vêm de um único handle, para os números baterem mesmo que o arquivo seja rotacionado ou
        // truncado ao mesmo tempo; uma origem que sumiu é pulada em vez de falhar o ZIP inteiro.
        let opened = File::open(local_path).and_then(|handle| handle.metadata().map(|metadata| (handle, metadata.len())));
        let (handle, current_size) = match opened {
            Ok(opened) => opened,
            Err(_) if file.skip_source_errors => continue,
            Err(e) => return Err(io_error(e)),
        };
        let size = file.local_path_size.map_or(current_size, |limit| limit.min(current_size));
        if size == 0 {
            start_entry(&mut writer, &file.path, None, 0)?;
            continue;
        }
        start_entry(&mut writer, &file.path, file.compress, size)?;
        for_each_chunk(&mut handle.take(size), io_error, |chunk| {
            bytes += chunk.len() as u64;
            report(bytes, &file.path);
            check_cancelled()?;
            writer.write_all(chunk).map_err(io_error)?;
            Ok(true)
        })?;
    }

    writer.finish().map_err(zip_error)?.flush().map_err(io_error)?;
    Ok(zip_path.to_path_buf())
}
