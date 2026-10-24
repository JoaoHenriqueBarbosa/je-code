//! Porte do que o provider de disco usa de `src/vs/base/node/pfs.ts`: `SymlinkSupport`, `Promises.readdir`
//! (com e sem tipos), `readDirsInDir`, `rm` (`RimRafMode`), `rename` com a nova tentativa do Windows, `copy`,
//! `realpath` (e `realpathSync`), `realcase`, `exists`, `Promises.writeFile` e `writeFileSync` (com `fdatasync`,
//! `configureFlushOnWrite` e a fila por caminho) e a abertura de arquivo para leitura.
//!
//! TODO(integração): mover para um módulo `base::node` quando existir.
//!
//! Fora do porte (nada no provider nem nos testes usa): `whenDeleted` e `Promises.read/write/fstat/ftruncate/
//! fdatasync` crus (o provider faz E/S posicional sobre `std::fs::File`, ver [`read_at`] e [`write_at`]).
//! O `configureFlushOnWrite` do provider de disco é outro, o dele; este é o do `pfs`.
//!
//! Divergência de execução: no original a E/S roda no pool de threads do libuv e a `Promise` resolve no
//! event loop. Aqui as funções síncronas bloqueiam o thread do loop; só as que precisam esperar de verdade
//! (`rm` com nova tentativa, `rename` com nova tentativa no Windows) são `async`. Os resultados são os mesmos.
//! `realcase` é síncrono pelo mesmo motivo, e `realpathSync` é a própria [`realpath`] (a lógica do original
//! é a mesma nas duas, só muda ser síncrona ou não). `writeFile` devolve a [`Promise`] da fila por caminho
//! (a escrita em si roda quando a fila a chama).
//!
//! Erros: [`FsError`] imita o erro do Node (o `code` do errno e a mensagem `ENOENT: no such file or
//! directory, stat '/x'`), porque o provider decide o código do erro dele pelo `code`.

use std::collections::HashSet;
use std::fmt;
use std::fs::{self, File, Metadata, OpenOptions};
use std::io;
use std::rc::Rc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

use je_base::async_::{ResourceQueue, timeout};
use je_base::cancellation::CancellationToken;
use je_base::errors::Error;
use je_base::extpath::{is_equal_or_parent, is_root_or_drive_letter, random_path};
use je_base::normalization::normalize_nfc;
use je_base::path::{Environment, PathModule, ProcessEnvironment};
use je_base::promise::Promise;
use je_base::resources::ExtUri;
use je_base::uri::URI;

// region: erro

/// O erro de E/S do Node: `code` é o `error.code` (`ENOENT`, `EACCES`...) e a mensagem já vem no formato
/// do Node. `code` é `None` para o erro sem errno (o `new Error('...')` do original).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FsError {
    pub code: Option<&'static str>,
    message: String,
}

impl FsError {
    /// `new Error(message)`: sem `code`.
    pub fn other(message: impl Into<String>) -> Self {
        Self { code: None, message: message.into() }
    }

    /// O erro de uma chamada de sistema, como o Node o monta: `<code>: <descrição>, <syscall> '<caminho>'`
    /// (com ` -> '<destino>'` nas chamadas de dois caminhos, como `rename` e `copyfile`).
    pub fn from_io(error: &io::Error, syscall: &str, path: Option<&str>, dest: Option<&str>) -> Self {
        let code = errno_code(error);
        let mut message = match code {
            Some(code) => format!("{code}: {}, {syscall}", errno_description(code)),
            None => format!("{error}, {syscall}"),
        };
        if let Some(path) = path {
            message.push_str(&format!(" '{path}'"));
        }
        if let Some(dest) = dest {
            message.push_str(&format!(" -> '{dest}'"));
        }
        Self { code, message }
    }

    /// O texto sem o prefixo `Error: ` do `toString()`.
    pub fn message(&self) -> &str {
        &self.message
    }
}

impl fmt::Display for FsError {
    /// `error.toString()`.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Error: {}", self.message)
    }
}

impl std::error::Error for FsError {}

impl From<FsError> for Error {
    /// O erro de uma `Promise` rejeitada pelo `fs`: a mensagem do Node e o `code` como propriedade.
    fn from(error: FsError) -> Self {
        let rejected = Error::new(error.message);
        match error.code {
            Some(code) => rejected.with_prop("code", code),
            None => rejected,
        }
    }
}

/// O adaptador do `map_err`: `fs::metadata(path).map_err(fs_err("stat", path))`.
pub fn fs_err<'a>(syscall: &'a str, path: &'a str) -> impl FnOnce(io::Error) -> FsError + 'a {
    move |error| FsError::from_io(&error, syscall, Some(path), None)
}

/// O mesmo para as chamadas de dois caminhos (`rename`, `copyfile`, `symlink`).
pub fn fs_err2<'a>(syscall: &'a str, path: &'a str, dest: &'a str) -> impl FnOnce(io::Error) -> FsError + 'a {
    move |error| FsError::from_io(&error, syscall, Some(path), Some(dest))
}

/// O `error.code` do Node a partir do erro do `std`. O `std` não distingue `EPERM` de `EACCES` (os dois são
/// `PermissionDenied`): no Unix o errno 1 é `EPERM`; no Windows o libuv traduz `ERROR_ACCESS_DENIED` para `EPERM`.
fn errno_code(error: &io::Error) -> Option<&'static str> {
    use io::ErrorKind::*;
    #[cfg(windows)]
    if matches!(error.raw_os_error(), Some(32 | 33)) {
        return Some("EBUSY"); // ERROR_SHARING_VIOLATION e ERROR_LOCK_VIOLATION
    }
    Some(match error.kind() {
        NotFound => "ENOENT",
        PermissionDenied if cfg!(windows) || error.raw_os_error() == Some(1) => "EPERM",
        PermissionDenied => "EACCES",
        AlreadyExists => "EEXIST",
        NotADirectory => "ENOTDIR",
        IsADirectory => "EISDIR",
        DirectoryNotEmpty => "ENOTEMPTY",
        InvalidInput => "EINVAL",
        CrossesDevices => "EXDEV",
        ResourceBusy => "EBUSY",
        InvalidFilename => "ENAMETOOLONG",
        _ => return None,
    })
}

/// `uv_strerror`: a descrição que o libuv dá a cada código.
fn errno_description(code: &str) -> &'static str {
    match code {
        "ENOENT" => "no such file or directory",
        "EACCES" => "permission denied",
        "EPERM" => "operation not permitted",
        "EEXIST" => "file already exists",
        "ENOTDIR" => "not a directory",
        "EISDIR" => "illegal operation on a directory",
        "ENOTEMPTY" => "directory not empty",
        "EINVAL" => "invalid argument",
        "EXDEV" => "cross-device link not permitted",
        "EBUSY" => "resource busy or locked",
        "ENAMETOOLONG" => "name too long",
        "EBADF" => "bad file descriptor",
        _ => "unknown error",
    }
}

/// O `EBADF` de uma operação sobre um handle que não existe (o `fs.read(fdInvalido)` do Node).
pub fn bad_file_descriptor(syscall: &str) -> FsError {
    FsError { code: Some("EBADF"), message: format!("EBADF: {}, {syscall}", errno_description("EBADF")) }
}

// endregion

// region: stat e leitura de pasta

/// O que `isFile()`, `isDirectory()` e `isSymbolicLink()` respondem, no `fs.Stats` e no `fs.Dirent`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct EntryKind {
    pub is_file: bool,
    pub is_directory: bool,
    pub is_symbolic_link: bool,
}

impl EntryKind {
    fn of(file_type: fs::FileType) -> Self {
        Self { is_file: file_type.is_file(), is_directory: file_type.is_dir(), is_symbolic_link: file_type.is_symlink() }
    }
}

/// `fs.Stats`.
#[derive(Clone, Debug)]
pub struct Stats(Metadata);

fn system_time_ms(time: SystemTime) -> i64 {
    match time.duration_since(UNIX_EPOCH) {
        Ok(duration) => i64::try_from(duration.as_millis()).unwrap_or(i64::MAX),
        Err(before) => -i64::try_from(before.duration().as_millis()).unwrap_or(i64::MAX),
    }
}

impl Stats {
    pub fn kind(&self) -> EntryKind {
        EntryKind::of(self.0.file_type())
    }

    pub fn is_file(&self) -> bool {
        self.0.is_file()
    }

    pub fn is_directory(&self) -> bool {
        self.0.is_dir()
    }

    pub fn is_symbolic_link(&self) -> bool {
        self.0.is_symlink()
    }

    pub fn size(&self) -> u64 {
        self.0.len()
    }

    /// `stat.mode`, só os bits de permissão. No Windows o Node sintetiza `0o666`, ou `0o444` com o atributo
    /// somente leitura (e nunca reporta os bits de execução).
    pub fn mode(&self) -> u32 {
        #[cfg(unix)]
        {
            use std::os::unix::fs::MetadataExt;
            self.0.mode() & 0o7777
        }
        #[cfg(not(unix))]
        {
            if self.0.permissions().readonly() { 0o444 } else { 0o666 }
        }
    }

    /// `stat.mtime.getTime()`.
    pub fn mtime_ms(&self) -> i64 {
        self.0.modified().map(system_time_ms).unwrap_or(0)
    }

    /// `stat.birthtime.getTime()`. Sem data de criação no sistema de arquivos, cai para o `ctime` (Unix) e
    /// depois para o `mtime`, que é o que o libuv faz nesses casos.
    pub fn birthtime_ms(&self) -> i64 {
        if let Ok(created) = self.0.created() {
            return system_time_ms(created);
        }
        #[cfg(unix)]
        {
            use std::os::unix::fs::MetadataExt;
            self.0.ctime() * 1000 + self.0.ctime_nsec() / 1_000_000
        }
        #[cfg(not(unix))]
        {
            self.mtime_ms()
        }
    }
}

/// `SymlinkSupport.IStats`.
#[derive(Clone, Debug)]
pub struct SymlinkStats {
    /// Os stats do alvo se o caminho é um link simbólico; do link, se ele aponta para o que não existe.
    pub stat: Stats,
    /// Presente se o caminho é um link simbólico; `dangling` diz que o alvo não existe.
    pub symbolic_link: Option<SymbolicLink>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SymbolicLink {
    pub dangling: bool,
}

/// `SymlinkSupport.stat(path)`.
pub fn symlink_stat(path: &str) -> Result<SymlinkStats, FsError> {
    // First stat the link
    let lstats = fs::symlink_metadata(path).ok();
    if let Some(lstats) = &lstats
        && !lstats.is_symlink()
    {
        return Ok(SymlinkStats { stat: Stats(lstats.clone()), symbolic_link: None });
    }

    // If the stat is a symbolic link or failed to stat, use fs.stat() which for symbolic links will stat
    // the target they point to
    let link_was_symlink = lstats.as_ref().is_some_and(Metadata::is_symlink);
    let dangling = |lstats: Option<Metadata>| {
        lstats.map(|lstats| SymlinkStats { stat: Stats(lstats), symbolic_link: Some(SymbolicLink { dangling: true }) })
    };
    match fs::metadata(path) {
        Ok(stats) => Ok(SymlinkStats { stat: Stats(stats), symbolic_link: link_was_symlink.then_some(SymbolicLink { dangling: false }) }),
        Err(error) => {
            // If the link points to a nonexistent file we still want to return it as result while setting
            // dangling: true flag
            if error.kind() == io::ErrorKind::NotFound
                && let Some(result) = dangling(lstats.clone())
            {
                return Ok(result);
            }

            // Windows: workaround a node.js bug where reparse points are not supported
            // (https://github.com/nodejs/node/issues/36790)
            if cfg!(windows) && error.kind() == io::ErrorKind::PermissionDenied {
                match fs::read_link(path).and_then(fs::metadata) {
                    Ok(stats) => return Ok(SymlinkStats { stat: Stats(stats), symbolic_link: Some(SymbolicLink { dangling: false }) }),
                    Err(inner) => {
                        if inner.kind() == io::ErrorKind::NotFound
                            && let Some(result) = dangling(lstats)
                        {
                            return Ok(result);
                        }
                        return Err(FsError::from_io(&inner, "stat", Some(path), None));
                    }
                }
            }

            Err(FsError::from_io(&error, "stat", Some(path), None))
        }
    }
}

/// `SymlinkSupport.existsFile(path)`: `false` também para o link simbólico que não aponta para nada.
pub fn exists_file(path: &str) -> bool {
    symlink_stat(path).is_ok_and(|result| result.stat.is_file() && result.symbolic_link.is_none_or(|link| !link.dangling))
}

/// `SymlinkSupport.existsDirectory(path)`.
pub fn exists_directory(path: &str) -> bool {
    symlink_stat(path).is_ok_and(|result| result.stat.is_directory() && result.symbolic_link.is_none_or(|link| !link.dangling))
}

/// `Promises.exists(path)`: o `fs.access(path)` com `F_OK`, que segue links simbólicos.
pub fn exists(path: &str) -> bool {
    fs::metadata(path).is_ok()
}

/// `IDirent`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Dirent {
    pub name: String,
    pub kind: EntryKind,
}

/// `Promises.readdir(path, { withFileTypes: true })`, na ordem em que o sistema entrega.
///
/// Uma entrada cujo tipo não se consegue ler (o que o original trata com o `lstat` de reserva, #115645) vem
/// com os três `is*` falsos. Na macOS os nomes saem em NFC (https://github.com/nodejs/node/issues/2165).
pub fn readdir(path: &str) -> Result<Vec<Dirent>, FsError> {
    match do_readdir(path) {
        Err(error) if error.code == Some("ENOENT") && cfg!(windows) && is_root_or_drive_letter(path, &ProcessEnvironment) => {
            // Workaround for #252361 that should be removed once the upstream issue in node.js is resolved.
            // Adds a trailing dot to a root drive letter path (G:\ => G:\.) as a workaround.
            do_readdir(&format!("{path}.")).map_err(|_| error)
        }
        result => result,
    }
}

fn do_readdir(path: &str) -> Result<Vec<Dirent>, FsError> {
    let entries = fs::read_dir(path).map_err(fs_err("scandir", path))?;
    let mut children = Vec::new();
    for entry in entries {
        let entry = entry.map_err(fs_err("scandir", path))?;
        let mut name = entry.file_name().to_string_lossy().into_owned();
        if cfg!(target_os = "macos") {
            // Mac: uses NFD unicode form on disk, but we want NFC
            let units: Vec<u16> = name.encode_utf16().collect();
            name = normalize_nfc(&units).to_string_lossy();
        }
        let kind = entry.file_type().map(EntryKind::of).unwrap_or_default();
        children.push(Dirent { name, kind });
    }
    Ok(children)
}

/// `Promises.readdir(path)` sem tipos: só os nomes, com o mesmo tratamento de [`readdir`].
pub fn readdir_names(path: &str) -> Result<Vec<String>, FsError> {
    readdir(path).map(|children| children.into_iter().map(|child| child.name).collect())
}

/// `Promises.readDirsInDir(dirPath)`: os nomes dos filhos que são pastas (seguindo links simbólicos).
pub fn read_dirs_in_dir(dir_path: &str) -> Result<Vec<String>, FsError> {
    let module = PathModule::native(&ProcessEnvironment);
    Ok(readdir_names(dir_path)?.into_iter().filter(|child| exists_directory(&module.join(&[dir_path, child]))).collect())
}

// endregion

// region: rimraf

/// `RimRafMode`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RimRafMode {
    /// Slow version that unlinks each file and folder.
    Unlink,
    /// Fast version that first moves the file/folder into a temp directory and then deletes that without
    /// waiting for it.
    Move,
}

/// `Promises.rm(path, mode, moveToPath?)`: apaga o arquivo ou a pasta (recursivo) com o modo dado. O
/// `moveToPath` troca o lugar para onde o `Move` renomeia antes de apagar.
pub async fn rm(path: &str, mode: RimRafMode, move_to_path: Option<&str>) -> Result<(), FsError> {
    if is_root_or_drive_letter(path, &ProcessEnvironment) {
        return Err(FsError::other("rimraf - will refuse to recursively delete root"));
    }

    // delete: via rm
    if mode == RimRafMode::Unlink {
        return rimraf_unlink(path).await;
    }

    // delete: via move
    rimraf_move(path, move_to_path).await
}

async fn rimraf_move(path: &str, move_to_path: Option<&str>) -> Result<(), FsError> {
    let move_to_path = move_to_path.map_or_else(|| random_path(Some(&temp_dir()), None, 8, &ProcessEnvironment), str::to_string);
    if let Err(error) = fs::rename(path, &move_to_path) {
        if error.kind() == io::ErrorKind::NotFound {
            return Ok(()); // ignore - path to delete did not exist
        }
        return rimraf_unlink(path).await; // otherwise fallback to unlink
    }

    // Delete but do not wait for it: no original a E/S roda no pool de threads, aqui roda num thread
    // solto. Só toca no caminho que acabou de ser renomeado, nunca no estado do loop.
    let background = move_to_path.clone();
    if std::thread::Builder::new().spawn(move || drop(remove_force(&background))).is_err() {
        drop(remove_force(&move_to_path)); // sem threads (wasm): apaga aqui mesmo
    }
    Ok(())
}

/// `fs.promises.rm(path, { recursive: true, force: true, maxRetries: 3 })`: as tentativas esperam
/// `100 ms * tentativa` entre si, e só para os erros que o Node considera passageiros.
async fn rimraf_unlink(path: &str) -> Result<(), FsError> {
    let mut attempt = 0;
    loop {
        match remove_force(path) {
            Err(error) if attempt < 3 && matches!(error.code, Some("EBUSY" | "ENOTEMPTY" | "EPERM")) => {
                attempt += 1;
                let _ = timeout(100 * attempt).await;
            }
            result => return result,
        }
    }
}

/// O `rm` recursivo com `force`: não segue link simbólico e ignora o caminho que não existe.
fn remove_force(path: &str) -> Result<(), FsError> {
    let metadata = match fs::symlink_metadata(path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(()),
        Err(error) => return Err(FsError::from_io(&error, "lstat", Some(path), None)),
    };
    let (result, syscall) = if metadata.is_dir() { (fs::remove_dir_all(path), "rmdir") } else { (fs::remove_file(path), "unlink") };
    match result {
        Err(error) if error.kind() != io::ErrorKind::NotFound => Err(FsError::from_io(&error, syscall, Some(path), None)),
        _ => Ok(()),
    }
}

fn temp_dir() -> String {
    std::env::temp_dir().to_string_lossy().into_owned()
}

// endregion

// region: write file

/// `IWriteFileOptions`: `mode` (padrão `0o666`, o do Node para arquivos) e `flag` (padrão `'w'`).
#[derive(Clone, Debug, Default)]
pub struct WriteFileOptions {
    pub mode: Option<u32>,
    pub flag: Option<String>,
}

static CAN_FLUSH: AtomicBool = AtomicBool::new(true);

/// `configureFlushOnWrite(enabled)`.
pub fn configure_flush_on_write(enabled: bool) {
    CAN_FLUSH.store(enabled, Ordering::SeqCst);
}

thread_local! {
    // According to node.js docs (https://nodejs.org/docs/v14.16.0/api/fs.html#fs_fs_writefile_file_data_options_callback)
    // it is not safe to call writeFile() on the same path multiple times without waiting for the callback to
    // return. Therefor we use a Queue on the path that is given to us to sequentialize calls to the same path properly.
    static WRITE_QUEUES: ResourceQueue = ResourceQueue::new(biased_ignore_path_case());
}

/// `extUriBiasedIgnorePathCase`.
fn biased_ignore_path_case() -> ExtUri {
    ExtUri::biased_ignore_path_case(Rc::new(ProcessEnvironment), cfg!(target_os = "linux"))
}

/// `Promises.writeFile(path, data, options)`: o `fs.writeFile` com um `fdatasync` depois de escrever, para
/// garantir que as mudanças chegaram ao disco. Várias escritas no mesmo caminho entram numa fila.
pub fn write_file(path: &str, data: impl Into<Vec<u8>>, options: Option<WriteFileOptions>) -> Promise<()> {
    let (path, data) = (path.to_string(), data.into());
    let resource = URI::file(&path, ProcessEnvironment.platform());
    WRITE_QUEUES.with(|queues| {
        queues.queue_for(&resource, move || match write_file_and_flush(&path, &data, options.as_ref()) {
            Ok(()) => Promise::resolved(()),
            Err(error) => Promise::rejected(error.into()),
        })
    })
}

/// `writeFileSync(path, data, options)`: o mesmo sem a fila e sem a `Promise`.
pub fn write_file_sync(path: &str, data: impl AsRef<[u8]>, options: Option<&WriteFileOptions>) -> Result<(), FsError> {
    write_file_and_flush(path, data.as_ref(), options)
}

/// `doWriteFileAndFlush` e `writeFileSync`: abre com o mesmo `flag` e `mode` do `fs.writeFile`, escreve e,
/// se o flush está ligado, sincroniza o conteúdo (não os metadados) com o disco
/// (https://github.com/microsoft/vscode/issues/9589).
fn write_file_and_flush(path: &str, data: &[u8], options: Option<&WriteFileOptions>) -> Result<(), FsError> {
    use io::Write;

    let mode = options.and_then(|options| options.mode).unwrap_or(0o666 /* default node.js mode for files */);
    let flag = options.and_then(|options| options.flag.as_deref()).unwrap_or("w");

    let mut file = open_for_write(path, flag, mode)?;
    file.write_all(data).map_err(|error| FsError::from_io(&error, "write", None, None))?;

    if CAN_FLUSH.load(Ordering::SeqCst)
        && let Err(error) = file.sync_data()
    {
        // In some exotic setups it is well possible that node fails to sync. In that case we disable
        // flushing and warn to the console
        eprintln!("[node.js fs] fdatasync is now disabled for this session because it failed: {}", FsError::from_io(&error, "fdatasync", None, None));
        configure_flush_on_write(false);
    }
    Ok(())
}

/// `fs.open(path, flag, mode)` para escrita: as letras do `flag` do Node (`r`, `w`, `a`, com `+`, `x` e `s`).
fn open_for_write(path: &str, flag: &str, mode: u32) -> Result<File, FsError> {
    let mut options = OpenOptions::new();
    let plus = flag.contains('+');
    let exclusive = flag.contains('x');
    match flag.chars().next() {
        Some('r') => options.read(true).write(plus),
        Some('w') => options.write(true).read(plus).create(!exclusive).create_new(exclusive).truncate(!exclusive),
        Some('a') => options.append(true).read(plus).create(!exclusive).create_new(exclusive),
        _ => return Err(FsError::other(format!("The argument 'flags' is invalid. Received '{flag}'"))),
    };
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(mode);
    }
    #[cfg(not(unix))]
    let _ = mode;
    options.open(path).map_err(fs_err("open", path))
}

// endregion

// region: rename e copy

/// O `windowsRetryTimeout: number | false = 60000` do `rename`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RenameRetry {
    /// O padrão do original: um minuto.
    Default,
    Millis(u64),
    Disabled,
}

/// `Promises.rename(source, target, windowsRetryTimeout)`: um `fs.rename` que atravessa discos (cai para
/// copiar e apagar no `EXDEV`) e que, no Windows, tenta de novo nos erros que o antivírus causa.
pub async fn rename(source: &str, target: &str, windows_retry_timeout: RenameRetry) -> Result<(), FsError> {
    if source == target {
        return Ok(()); // simulate node.js behaviour here and do a no-op if paths match
    }

    let renamed = match windows_retry_timeout {
        // On Windows, a rename can fail when either source or target is locked by AV software.
        RenameRetry::Default | RenameRetry::Millis(_) if cfg!(windows) => {
            let retry_timeout = match windows_retry_timeout {
                RenameRetry::Millis(millis) => millis,
                _ => 60000,
            };
            rename_with_retry(source, target, je_base::runtime::date_now(), retry_timeout).await
        }
        _ => fs::rename(source, target).map_err(fs_err2("rename", source, target)),
    };

    match renamed {
        Ok(()) => Ok(()),
        // In two cases we fallback to classic copy and delete:
        //
        // 1.) The EXDEV error indicates that source and target are on different devices
        // In this case, fallback to using a copy() operation as there is no way to
        // rename() between different devices.
        //
        // 2.) The user tries to rename a file/folder that ends with a dot. This is not
        // really possible to move then, at least on UNC devices.
        Err(error) => {
            if (source.to_lowercase() != target.to_lowercase() && error.code == Some("EXDEV")) || source.ends_with('.') {
                copy(source, target, false /* copying to another device */)?;
                rm(source, RimRafMode::Move, None).await
            } else {
                Err(error)
            }
        }
    }
}

async fn rename_with_retry(source: &str, target: &str, start_time: u64, retry_timeout: u64) -> Result<(), FsError> {
    let mut attempt: u64 = 0;
    loop {
        let error = match fs::rename(source, target) {
            Ok(()) => return Ok(()),
            Err(error) => FsError::from_io(&error, "rename", Some(source), Some(target)),
        };
        if !matches!(error.code, Some("EACCES" | "EPERM" | "EBUSY")) {
            return Err(error); // only for errors we think are temporary
        }

        if je_base::runtime::date_now().saturating_sub(start_time) >= retry_timeout {
            eprintln!("[node.js fs] rename failed after {attempt} retries with error: {error}");
            return Err(error); // give up after configurable timeout
        }

        if attempt == 0 {
            // if target is not a file, EPERM error may be raised and we should not attempt to retry
            if symlink_stat(target).is_ok_and(|result| !result.stat.is_file()) {
                return Err(error);
            }
        }

        // Delay with incremental backoff up to 100ms
        let _ = timeout((attempt * 10).min(100)).await;
        attempt += 1;
    }
}

/// O que sobra das permissões ao copiar: `stat.mode & 0o777` (https://github.com/nodejs/node-v0.x-archive/issues/3045).
const COPY_MODE_MASK: u32 = 0o777;

/// `Promises.copy(source, target, { preserveSymlinks })`: copia `source` inteiro para `target`. Com
/// `preserve_symlinks: false` os links simbólicos são seguidos em vez de recriados.
pub fn copy(source: &str, target: &str, preserve_symlinks: bool) -> Result<(), FsError> {
    let mut handled_source_paths = HashSet::new();
    do_copy(source, target, &CopyRoot { source, target, preserve_symlinks }, &mut handled_source_paths)
}

struct CopyRoot<'a> {
    source: &'a str,
    target: &'a str,
    preserve_symlinks: bool,
}

fn do_copy(source: &str, target: &str, root: &CopyRoot, handled_source_paths: &mut HashSet<String>) -> Result<(), FsError> {
    // Keep track of paths already copied to prevent cycles from symbolic links to cause issues
    if !handled_source_paths.insert(source.to_string()) {
        return Ok(());
    }

    let SymlinkStats { stat, symbolic_link } = symlink_stat(source)?;

    // Symlink
    if let Some(link) = symbolic_link {
        // Try to re-create the symlink unless `preserveSymlinks: false`
        if root.preserve_symlinks && do_copy_symlink(source, target, root).is_ok() {
            return Ok(());
            // in any case of an error fallback to normal copy via dereferencing
        }

        if link.dangling {
            return Ok(()); // skip dangling symbolic links from here on (https://github.com/microsoft/vscode/issues/111621)
        }
    }

    let mode = stat.mode() & COPY_MODE_MASK;
    if stat.is_directory() {
        do_copy_directory(source, target, mode, root, handled_source_paths)
    } else {
        do_copy_file(source, target, mode)
    }
}

fn do_copy_directory(source: &str, target: &str, mode: u32, root: &CopyRoot, handled_source_paths: &mut HashSet<String>) -> Result<(), FsError> {
    // Create folder
    mkdir_recursive(target, Some(mode))?;

    // Copy each file recursively
    let module = PathModule::native(&ProcessEnvironment);
    for file in readdir(source)? {
        do_copy(&module.join(&[source, &file.name]), &module.join(&[target, &file.name]), root, handled_source_paths)?;
    }
    Ok(())
}

fn do_copy_file(source: &str, target: &str, mode: u32) -> Result<(), FsError> {
    // Copy file
    fs::copy(source, target).map_err(fs_err2("copyfile", source, target))?;

    // restore mode (https://github.com/nodejs/node/issues/1104)
    chmod(target, mode)
}

fn do_copy_symlink(source: &str, target: &str, root: &CopyRoot) -> Result<(), FsError> {
    // Figure out link target
    let mut link_target = fs::read_link(source).map_err(fs_err("readlink", source))?.to_string_lossy().into_owned();

    // Special case: the symlink points to a target that is actually within the path that is being copied.
    // In that case we want the symlink to point to the target and not the source
    if is_equal_or_parent(&link_target, root.source, !cfg!(target_os = "linux"), false, &ProcessEnvironment) {
        let rest: Vec<u16> = link_target.encode_utf16().skip(root.source.encode_utf16().count() + 1).collect();
        link_target = PathModule::native(&ProcessEnvironment).join(&[root.target, &String::from_utf16_lossy(&rest)]);
    }

    // Create symlink
    symlink(&link_target, target)
}

/// `fs.promises.symlink(target, path)`: no Windows o Node descobre sozinho se o alvo é pasta.
fn symlink(link_target: &str, path: &str) -> Result<(), FsError> {
    #[cfg(unix)]
    let result = std::os::unix::fs::symlink(link_target, path);
    #[cfg(windows)]
    let result = {
        let resolved = std::path::Path::new(path).parent().map_or_else(|| link_target.into(), |parent| parent.join(link_target));
        if resolved.is_dir() { std::os::windows::fs::symlink_dir(link_target, path) } else { std::os::windows::fs::symlink_file(link_target, path) }
    };
    #[cfg(not(any(unix, windows)))]
    let result: io::Result<()> = Err(io::ErrorKind::Unsupported.into());
    result.map_err(fs_err2("symlink", link_target, path))
}

/// `fs.promises.mkdir(path, { recursive: true, mode })`.
pub fn mkdir_recursive(path: &str, mode: Option<u32>) -> Result<(), FsError> {
    let mut builder = fs::DirBuilder::new();
    builder.recursive(true);
    #[cfg(unix)]
    if let Some(mode) = mode {
        use std::os::unix::fs::DirBuilderExt;
        builder.mode(mode);
    }
    #[cfg(not(unix))]
    let _ = mode;
    builder.create(path).map_err(fs_err("mkdir", path))
}

/// `fs.promises.chmod(path, mode)`. No Windows só o bit de escrita do dono vira o atributo somente leitura.
pub fn chmod(path: &str, mode: u32) -> Result<(), FsError> {
    #[cfg(unix)]
    let permissions = {
        use std::os::unix::fs::PermissionsExt;
        fs::Permissions::from_mode(mode)
    };
    #[cfg(not(unix))]
    let permissions = {
        let mut permissions = fs::metadata(path).map_err(fs_err("chmod", path))?.permissions();
        permissions.set_readonly(mode & 0o200 == 0);
        permissions
    };
    fs::set_permissions(path, permissions).map_err(fs_err("chmod", path))
}

// endregion

// region: realpath

/// `Promises.realpath(path)`. Se o sistema não resolve (link quebrado no meio, por exemplo), cai para o
/// caminho normalizado, desde que seja legível.
pub fn realpath(path: &str) -> Result<String, FsError> {
    match fs::canonicalize(path) {
        Ok(real) => Ok(strip_verbatim_prefix(real.to_string_lossy().into_owned())),
        Err(_) => {
            // We hit an error calling fs.realpath(). Since fs.realpath() is doing some path normalization
            // we now do a similar normalization and then try again if we can access the path with read
            // permissions at least. If that succeeds, we return that path.
            let module = PathModule::native(&ProcessEnvironment);
            let normalized = module.normalize(path);
            let normalized = normalized.trim_end_matches(module.sep()).to_string();
            access_readable(&normalized)?;
            Ok(normalized)
        }
    }
}

/// O `fs::canonicalize` do Windows devolve `\\?\C:\x`; o `fs.realpath` do Node, `C:\x`.
fn strip_verbatim_prefix(path: String) -> String {
    if !cfg!(windows) {
        return path;
    }
    match path.strip_prefix(r"\\?\") {
        Some(rest) => match rest.strip_prefix(r"UNC\") {
            Some(unc) => format!(r"\\{unc}"),
            None => rest.to_string(),
        },
        None => path,
    }
}

/// `fs.promises.access(path, R_OK)`.
fn access_readable(path: &str) -> Result<(), FsError> {
    let stats = fs::metadata(path).map_err(fs_err("access", path))?;
    let opened = if stats.is_dir() { fs::read_dir(path).map(drop) } else { open_options_for_read().open(path).map(drop) };
    opened.map_err(fs_err("access", path))
}

/// `realcase(path, token)`: dado um caminho absoluto, normalizado e existente, o caminho exato que o
/// arquivo tem no disco. Num sistema de arquivos que não diferencia maiúsculas o resultado pode diferir do
/// original só no uso de maiúsculas; num que diferencia é sempre idêntico. Devolve `None` em caso de erro
/// (e por isso não serve para verificar que um caminho existe).
///
/// Não trata os segmentos `..` e `.` nem leva o idioma em conta.
pub fn realcase(path: &str, token: Option<&CancellationToken>) -> Option<String> {
    if cfg!(target_os = "linux") {
        // This method is unsupported on OS that have case sensitive file system where the same path can exist in
        // different forms (see also https://github.com/microsoft/vscode/issues/139709)
        return Some(path.to_string());
    }

    let module = PathModule::native(&ProcessEnvironment);
    let dir = module.dirname(path);
    if path == dir {
        return Some(path.to_string()); // end recursion
    }

    let base = module.basename(path, None);
    let name = (if base.is_empty() { path } else { base.as_str() /* can be '' for windows drive letters */ }).to_lowercase();

    if token.is_some_and(CancellationToken::is_cancellation_requested) {
        return None;
    }

    let entries = readdir_names(&dir).ok()?; // silently ignore error
    let found: Vec<&String> = entries.iter().filter(|entry| entry.to_lowercase() == name).collect(); // use a case insensitive search
    let index = match found.len() {
        // on a case sensitive filesystem we cannot determine here, whether the file exists or not, hence we need the 'file exists' precondition
        1 => 0,
        // must be a case sensitive $filesystem
        n if n > 1 => found.iter().position(|entry| **entry == name)?,
        _ => return None,
    };
    let prefix = realcase(&dir, token)?; // recurse
    Some(module.join(&[&prefix, found[index]]))
}

// endregion

// region: E/S de arquivo

/// As opções da abertura de leitura do original: `O_RDONLY | O_NONBLOCK` (no Windows, `'r'`). O
/// `O_NONBLOCK` faz o `open` de um named pipe sem escritor voltar na hora em vez de travar o thread.
pub fn open_options_for_read() -> OpenOptions {
    let mut options = OpenOptions::new();
    options.read(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.custom_flags(libc::O_NONBLOCK);
    }
    #[cfg(windows)]
    {
        use std::os::windows::fs::OpenOptionsExt;
        const FILE_FLAG_BACKUP_SEMANTICS: u32 = 0x0200_0000; // o libuv abre pastas assim, o `fstat` diz o que é
        options.custom_flags(FILE_FLAG_BACKUP_SEMANTICS);
    }
    options
}

/// `fs.open(path, 'r')` do `Promises.open`, com as opções de [`open_options_for_read`].
pub fn open_for_read(path: &str) -> Result<File, FsError> {
    open_options_for_read().open(path).map_err(fs_err("open", path))
}

/// `fs.fstat(fd)`.
pub fn fstat(file: &File) -> Result<Stats, FsError> {
    file.metadata().map(Stats).map_err(|error| FsError::from_io(&error, "fstat", None, None))
}

/// `fs.readFile(fd)`: lê até o fim a partir da posição atual do handle.
pub fn read_to_end(mut file: &File) -> Result<Vec<u8>, FsError> {
    use io::Read;
    let mut content = Vec::new();
    file.read_to_end(&mut content).map_err(|error| FsError::from_io(&error, "read", None, None))?;
    Ok(content)
}

/// `fs.read(fd, buffer, 0, length, position)`: lê a partir de `position`, sem mexer na posição do handle.
/// Devolve os bytes lidos, que podem ser menos que o pedido.
pub fn read_at(file: &File, buffer: &mut [u8], position: u64) -> Result<usize, FsError> {
    loop {
        #[cfg(unix)]
        let result = std::os::unix::fs::FileExt::read_at(file, buffer, position);
        #[cfg(windows)]
        let result = std::os::windows::fs::FileExt::seek_read(file, buffer, position);
        #[cfg(not(any(unix, windows)))]
        let result: io::Result<usize> = {
            let _ = (file, &buffer, position);
            Err(io::ErrorKind::Unsupported.into())
        };
        match result {
            Err(error) if error.kind() == io::ErrorKind::Interrupted => continue,
            result => return result.map_err(|error| FsError::from_io(&error, "read", None, None)),
        }
    }
}

/// `fs.write(fd, buffer, 0, length, position)`: escreve a partir de `position`, sem mexer na posição do
/// handle. Devolve os bytes escritos. Num handle aberto para anexar, o sistema anexa no fim e ignora a posição.
pub fn write_at(file: &File, buffer: &[u8], position: u64) -> Result<usize, FsError> {
    loop {
        #[cfg(unix)]
        let result = std::os::unix::fs::FileExt::write_at(file, buffer, position);
        #[cfg(windows)]
        let result = std::os::windows::fs::FileExt::seek_write(file, buffer, position);
        #[cfg(not(any(unix, windows)))]
        let result: io::Result<usize> = {
            let _ = (file, &buffer, position);
            Err(io::ErrorKind::Unsupported.into())
        };
        match result {
            Err(error) if error.kind() == io::ErrorKind::Interrupted => continue,
            result => return result.map_err(|error| FsError::from_io(&error, "write", None, None)),
        }
    }
}

// endregion
