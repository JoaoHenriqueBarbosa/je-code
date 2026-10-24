//! `Bun.file`, `Bun.write` e os métodos de arquivo do `Blob` (bun 1.4.2). Um Blob de arquivo é um `Blob` de verdade
//! (mesmo protótipo, `constructor.name` "Blob", `instanceof Blob`, sem chaves próprias): o estado de bytes de `blob.rs`
//! fica vazio e o que o liga ao disco mora numa tabela lateral daqui, indexada pela instância. Medido no oráculo:
//!
//! - `size` e `lastModified` são lidos uma vez, no primeiro acesso, e ficam (um arquivo que cresce depois não muda o
//!   `size` do mesmo Blob); arquivo ausente dá `size` 0 e `lastModified` 2^52-1; diretório dá o `size` do `stat`;
//! - `type`: o do `options.type`, senão o da extensão (tabela `mime_table`), senão `application/octet-stream`;
//! - `name` é o caminho como veio (relativo continua relativo), e um descritor não tem nome;
//! - o conteúdo só entra por `text`, `bytes`, `arrayBuffer`, `json`, `formData`, `stream`, `Response` e `Bun.write`: um
//!   Blob de arquivo dentro de `new Blob([...])` contribui com nada;
//! - `slice` continua sendo do arquivo (deslocamento e tamanho), com o `type` da extensão se o argumento não der um;
//! - `exists` é falso para diretório; `delete` e `unlink` removem (`ENOENT ..., unlink '<caminho>'`); `stat` é o
//!   `fs.promises.stat`; `write` e `writer` seguem `bun_file.js`.
//!
//! A leitura é síncrona e a promessa já nasce resolvida (o bun resolve depois da E/S); declarado no PLAN.

use std::cell::RefCell;
use std::collections::HashMap;

use crate::api::eval::js_text;
use crate::host_function;
use crate::runtime::blob::{make_blob, normalized_type, resolved_promise, state_of, BlobState, MEMORY_LAST_MODIFIED};
use crate::runtime::body::pending_reason;
use crate::runtime::host::{host, FsStat, Host};
use crate::runtime::host_call::{pending_or, HostCall, HostResult, Thrown};
use crate::runtime::identifier::Identifier;
use crate::runtime::js_global_object::JSGlobalObject;
use crate::runtime::js_module_loader::rust_string;
use crate::runtime::js_promise::JSPromise;
use crate::runtime::js_value::{js_number, EncodedJSValue, JSValue};
use crate::runtime::node_error::{throw_coded_range_error, throw_coded_type_error, throw_error_with_properties, uv_error_info, NetworkProperty};
use crate::runtime::string_regexp_support::get_object_property;
use crate::wtf::text::wtf_string::String as WtfString;

const SOURCE: &str = include_str!("bun_file.js");
const S_IFMT: u32 = 0o170000;
const S_IFDIR: u32 = 0o040000;
const S_IFREG: u32 = 0o100000;
const CHUNK: usize = 65536;

#[derive(Clone)]
pub(crate) enum Source {
    Path(String),
    Fd(i32),
}

#[derive(Clone, Copy)]
struct Snapshot {
    size: u64,
    last_modified: f64,
}

#[derive(Clone)]
pub(crate) struct FileBacking {
    pub(crate) source: Source,
    start: u64,
    end: Option<u64>,
    snapshot: Option<Snapshot>,
}

/// Uma falha de E/S pronta para virar o erro do `node:fs` (`code`, `path`, `syscall`, `errno` negativo).
pub(crate) struct FileError {
    code: &'static str,
    message: String,
    syscall: &'static str,
    path: Option<String>,
    errno: i32,
}

thread_local! {
    /// O que liga cada Blob de arquivo ao disco (as instâncias já são raízes do coletor em `blob.rs`).
    static FILES: RefCell<HashMap<EncodedJSValue, FileBacking>> = RefCell::new(HashMap::new());
}

pub(crate) fn reset_for_program() {
    let _ = FILES.try_with(|files| files.borrow_mut().clear());
}

pub(crate) fn backing_of(value: JSValue) -> Option<FileBacking> {
    FILES.with(|files| files.borrow().get(&value.encode()).cloned())
}

fn path_text(source: &Source) -> Option<&str> {
    match source {
        Source::Path(path) => Some(path),
        Source::Fd(_) => None,
    }
}

fn stat_of(source: &Source) -> Result<FsStat, i32> {
    match source {
        Source::Path(path) => host().fs_stat(path, true),
        Source::Fd(fd) => host().fs_fstat(*fd),
    }
}

fn is_directory(stat: &FsStat) -> bool {
    stat.mode & S_IFMT == S_IFDIR
}

fn io_error(errno: i32, syscall: &'static str, path: Option<&str>) -> FileError {
    let (code, description) = uv_error_info(errno);
    let message = match path {
        Some(path) => format!("{code}: {description}, {syscall} '{path}'"),
        None => format!("{code}: {description}, {syscall}"),
    };
    FileError { code, message, syscall, path: path.map(str::to_owned), errno: -errno }
}

/// O erro como valor (as propriedades na ordem do `node:fs`: `code`, `path`, `syscall`, `errno`).
pub(crate) fn error_value(global_object: &JSGlobalObject, error: &FileError) -> JSValue {
    let mut properties = vec![("code", NetworkProperty::Text(error.code))];
    if let Some(path) = &error.path {
        properties.push(("path", NetworkProperty::Text(path)));
    }
    properties.push(("syscall", NetworkProperty::Text(error.syscall)));
    properties.push(("errno", NetworkProperty::Number(error.errno)));
    let _ = throw_error_with_properties(global_object, &error.message, &properties);
    pending_reason(global_object)
}

pub(crate) fn rejected(global_object: &JSGlobalObject, error: &FileError) -> JSValue {
    JSPromise::rejected_promise(global_object, error_value(global_object, error)).as_value()
}

/// `size` e `lastModified` do Blob de arquivo, lidos no primeiro acesso e guardados.
fn snapshot(value: JSValue) -> Option<Snapshot> {
    let backing = backing_of(value)?;
    if let Some(snapshot) = backing.snapshot {
        return Some(snapshot);
    }
    let snapshot = match stat_of(&backing.source) {
        Ok(stat) => {
            let end = backing.end.map_or(stat.size, |end| end.min(stat.size));
            let modified = stat.mtime.0 as f64 * 1000.0 + (stat.mtime.1 / 1_000_000) as f64;
            Snapshot { size: end.saturating_sub(backing.start), last_modified: modified }
        }
        Err(_) => Snapshot { size: 0, last_modified: MEMORY_LAST_MODIFIED },
    };
    FILES.with(|files| files.borrow_mut().get_mut(&value.encode()).map(|entry| entry.snapshot = Some(snapshot)));
    Some(snapshot)
}

/// `size`: um descritor que não é arquivo regular (pipe, terminal, dispositivo) não tem tamanho conhecido, e o bun
/// responde `Infinity` (medido no bun 1.4.2 para `Bun.stdin`, `Bun.stdout` e `Bun.file(fd)`).
pub(crate) fn size(value: JSValue) -> Option<f64> {
    if let Source::Fd(fd) = backing_of(value)?.source {
        if host().fs_fstat(fd).is_ok_and(|stat| stat.mode & S_IFMT != S_IFREG) {
            return Some(f64::INFINITY);
        }
    }
    snapshot(value).map(|snapshot| snapshot.size as f64)
}

/// `Bun.stdin`, `Bun.stdout` e `Bun.stderr`: o Blob de arquivo do descritor `fd`.
pub(crate) fn std_file(global_object: &JSGlobalObject, fd: i32) -> JSValue {
    register(global_object, FileBacking { source: Source::Fd(fd), start: 0, end: None, snapshot: None }, b"application/octet-stream".to_vec(), None)
}

pub(crate) fn last_modified(value: JSValue) -> Option<f64> {
    snapshot(value).map(|snapshot| snapshot.last_modified)
}

/// O conteúdo (do deslocamento `start` até `end`). Um diretório não se lê como arquivo.
pub(crate) fn read(backing: &FileBacking) -> Result<Vec<u8>, FileError> {
    let files = host();
    let path = path_text(&backing.source);
    let (fd, owned) = match &backing.source {
        Source::Path(path) => (files.fs_open(path, 0, 0).map_err(|errno| io_error(errno, "open", Some(path)))?, true),
        Source::Fd(fd) => (*fd, false),
    };
    let result = read_open(&*files, fd, backing, path);
    if owned {
        let _ = files.fs_close(fd);
    }
    result
}

fn read_open(files: &dyn Host, fd: i32, backing: &FileBacking, path: Option<&str>) -> Result<Vec<u8>, FileError> {
    let stat = files.fs_fstat(fd).map_err(|errno| io_error(errno, "fstat", path))?;
    if is_directory(&stat) {
        let message = "Directories cannot be read like files".to_owned();
        return Err(FileError { code: "EISDIR", message, syscall: "read", path: path.map(str::to_owned), errno: 0 });
    }
    let mut offset = backing.start;
    let mut bytes = Vec::new();
    loop {
        let wanted = match backing.end {
            Some(end) if offset >= end => break,
            Some(end) => (end - offset).min(CHUNK as u64) as usize,
            None => CHUNK,
        };
        // Pipe, terminal e dispositivo não têm posição: lê em sequência, como o bun lê `Bun.stdin`.
        let position = (stat.mode & S_IFMT == S_IFREG).then_some(offset);
        let chunk = files.fs_read(fd, wanted, position).map_err(|errno| io_error(errno, "read", path))?;
        if chunk.is_empty() {
            break;
        }
        offset += chunk.len() as u64;
        bytes.extend(chunk);
    }
    Ok(bytes)
}

/// O `type` da extensão (`mime_table`), ou `application/octet-stream`.
fn derived_type(source: &Source) -> Vec<u8> {
    let Some(path) = path_text(source) else { return b"application/octet-stream".to_vec() };
    let name = path.rsplit('/').next().unwrap_or(path);
    name.rsplit_once('.')
        .and_then(|(_, extension)| crate::runtime::mime_table::by_extension(extension.as_bytes()))
        .unwrap_or(b"application/octet-stream")
        .to_vec()
}

fn units_of(text: &str) -> Vec<u16> {
    text.encode_utf16().collect()
}

fn register(global_object: &JSGlobalObject, backing: FileBacking, content_type: Vec<u8>, name: Option<Vec<u16>>) -> JSValue {
    let blob = make_blob(global_object, BlobState { content_type, name, ..BlobState::default() });
    FILES.with(|files| files.borrow_mut().insert(blob.encode(), backing));
    blob
}

/// `Bun.file(caminho | URL | descritor, { type })`.
fn file_body(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    let argument = call.argument(0);
    let (source, name) = if argument.is_string() {
        let given = rust_string(&pending_or(global_object, argument.to_wtf_string())?);
        (Source::Path(crate::runtime::process_system::resolve_in_current_directory(&given)), Some(units_of(&given)))
    } else if let Some(url) = crate::runtime::url::url_of(argument) {
        let path = rust_string(&url.file_system_path());
        (Source::Path(path.clone()), Some(units_of(&path)))
    } else if argument.is_number() {
        let number = argument.as_number();
        if !number.is_finite() || number.fract() != 0.0 {
            let message = format!("The value of \"fd\" is out of range. It must be an integer. Received {number}");
            return Err(throw_coded_range_error(global_object, &message, "ERR_OUT_OF_RANGE"));
        }
        (Source::Fd(number as i32), None)
    } else {
        return Err(throw_coded_type_error(global_object, "Expected file path string or file descriptor", "ERR_INVALID_ARG_TYPE"));
    };
    let options = call.argument(1);
    let explicit = if options.is_object() {
        normalized_type(get_object_property(global_object, options, &Identifier::from_span(global_object.vm(), b"type"))?)
    } else {
        Vec::new()
    };
    let content_type = if explicit.is_empty() { derived_type(&source) } else { explicit };
    Ok(register(global_object, FileBacking { source, start: 0, end: None, snapshot: None }, content_type, name))
}
host_function!(pub bun_file, file_body);

/// O fatiamento de um Blob de arquivo: continua do arquivo, com `start` e `end` relativos ao `size` já lido.
pub(crate) fn slice(global_object: &JSGlobalObject, value: JSValue, start: i64, end: i64, explicit_type: Vec<u8>) -> Option<JSValue> {
    let backing = backing_of(value)?;
    let (start, end) = (start.max(0) as u64, end.max(start) as u64);
    let sliced = FileBacking { source: backing.source.clone(), start: backing.start + start, end: Some(backing.start + end), snapshot: None };
    let content_type = if explicit_type.is_empty() { derived_type(&backing.source) } else { explicit_type };
    let name = state_of(value).and_then(|state| state.name);
    Some(register(global_object, sliced, content_type, name))
}

/// `exists`: verdadeiro para quem existe e não é diretório.
pub(crate) fn exists(global_object: &JSGlobalObject, value: JSValue) -> Option<JSValue> {
    let backing = backing_of(value)?;
    let found = stat_of(&backing.source).is_ok_and(|stat| !is_directory(&stat));
    Some(resolved_promise(global_object, JSValue::Bool(found)))
}

/// `delete` e `unlink`.
pub(crate) fn delete(global_object: &JSGlobalObject, value: JSValue) -> Option<JSValue> {
    let backing = backing_of(value)?;
    let outcome = match &backing.source {
        Source::Path(path) => host().fs_unlink(path).map_err(|errno| io_error(errno, "unlink", Some(path))),
        Source::Fd(_) => Ok(()),
    };
    Some(match outcome {
        Ok(()) => resolved_promise(global_object, JSValue::undefined()),
        Err(error) => rejected(global_object, &error),
    })
}

/// `stat`: o `fs.promises.stat` do caminho.
pub(crate) fn stat(global_object: &JSGlobalObject, value: JSValue) -> Option<HostResult> {
    let backing = backing_of(value)?;
    Some(match &backing.source {
        Source::Path(path) => call_fs_promise(global_object, "stat", &[js_text(global_object.vm(), path)]),
        Source::Fd(fd) => call_module(global_object, "fstat", &[js_number(f64::from(*fd))]),
    })
}

fn call_fs_promise(global_object: &JSGlobalObject, name: &str, arguments: &[JSValue]) -> HostResult {
    let crate::api::builtin_modules::Resolved::Builtin(entry) = crate::api::builtin_modules::resolve("fs") else { return Ok(JSValue::undefined()) };
    let fs = crate::api::builtin_modules::load(global_object, entry)?;
    let promises = get_object_property(global_object, fs, &Identifier::from_span(global_object.vm(), b"promises"))?;
    let function = get_object_property(global_object, promises, &Identifier::from_span(global_object.vm(), name.as_bytes()))?;
    crate::api::node_js_host::call_value(global_object, function, promises, arguments, "bun:file")
}

/// `fileInfo(blob)` para o JavaScript: o caminho, o descritor, ou `undefined` se não é Blob de arquivo.
fn file_info_body(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    Ok(match backing_of(call.argument(0)) {
        Some(FileBacking { source: Source::Path(path), .. }) => js_text(global_object.vm(), &path),
        Some(FileBacking { source: Source::Fd(fd), .. }) => js_number(f64::from(fd)),
        None => JSValue::undefined(),
    })
}
host_function!(file_info_function, file_info_body);

fn module(global_object: &JSGlobalObject) -> HostResult {
    crate::api::builtin_modules::cached("internal:bun-file", || {
        let host = crate::api::node_js_host::module_host(global_object, &[("fileInfo", 1, file_info_function)])?;
        Ok(crate::api::node_js_host::run_native_factory(global_object, SOURCE, "bun-file", &host))
    })
}

/// Chama `write` ou `writer` do `bun_file.js`.
pub(crate) fn call_module(global_object: &JSGlobalObject, name: &str, arguments: &[JSValue]) -> HostResult {
    let exports = module(global_object)?;
    let function = get_object_property(global_object, exports, &Identifier::from_span(global_object.vm(), name.as_bytes()))?;
    crate::api::node_js_host::call_value(global_object, function, JSValue::undefined(), arguments, "bun:file")
}

/// `Bun.write(destino, dado, opções)`.
fn write_body(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    call_module(global_object, "write", call.arguments())
}
host_function!(pub bun_write, write_body);

/// O que o `inspect` mostra entre parênteses depois de `FileRef`: `"<caminho>"` ou `fd: N`; `None` se não é Blob de
/// arquivo (ou é de caminho sem nome).
pub(crate) fn display_path(value: JSValue) -> Option<String> {
    match backing_of(value)?.source {
        Source::Path(_) => state_of(value)
            .and_then(|state| state.name)
            .map(|units| WtfString::from_utf16(&units))
            .map(|text| format!("\"{}\"", rust_string(&text))),
        Source::Fd(fd) => Some(format!("fd: {fd}")),
    }
}
