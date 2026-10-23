//! Porte de `src/vs/platform/files/node/diskFileSystemProvider.ts`: o provider do sistema de arquivos
//! real (`file:`), com leitura e escrita inteiras, `open/read/write/close` com handles, leitura em stream,
//! escrita e exclusão atômicas, copiar, mover, clonar e `realpath`.
//!
//! Divergências do original (resultados e ordem de microtarefas dos casos portados não mudam):
//!
//! - **E/S bloqueante.** No Node a E/S roda no pool de threads do libuv e a `Promise` resolve no event loop;
//!   aqui o corpo usa `std::fs` e bloqueia o thread do loop. Só o que espera de verdade (o lock por recurso,
//!   a nova tentativa de `write`, o `rename` do Windows, o `rm` com nova tentativa) é `async`.
//! - **Handles sintéticos.** O original devolve o descritor do sistema operacional; sem `unsafe` não há fd
//!   cru, então cada handle é um `i64` sintético (contador, nunca reaproveitado) para um `std::fs::File`
//!   guardado num mapa, e a leitura e a escrita são posicionais (`pread`/`pwrite`, ou `seek_read`/`seek_write`
//!   no Windows). Por isso não existem `mapHandleToPos`, `normalizePos` e `updatePos` (que só evitam um
//!   `seek` extra do Node), e o aviso do #142462 (o sistema reaproveita o fd de um handle recém fechado)
//!   não se aplica. O erro de `close(2)` não é observável num `Drop`: o `fdatasync` antes dele é.
//! - **Subclasses.** O `DiskFileSystemProvider` de teste do VS Code é uma subclasse que sobrescreve
//!   `capabilities`, `stat`, `read` e `readFile`. As chamadas virtuais que o original faz sobre `this` e que
//!   um teste observa (`readFileIntoStream(this, ...)` e as capacidades) passam pelo provider registrado com
//!   [`DiskFileSystemProvider::set_outer`]; as demais chamam as próprias funções.
//! - `ERR_UNC_HOST_NOT_ALLOWED` (a lista de hosts UNC do Node, só no Windows) não existe aqui.
//! - `doWriteFile` repete `write` até escrever tudo (o original confia numa única chamada, que o sistema
//!   pode encurtar acima de 2 GB).

use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::fs::{File, OpenOptions};
use std::rc::{Rc, Weak};
use std::sync::atomic::{AtomicBool, Ordering};

use je_base::async_::{Barrier, async_fn, retry};
use je_base::buffer::{VSBuffer, VSBufferReadableStream, new_writeable_buffer_stream};
use je_base::cancellation::CancellationToken;
use je_base::errors::Error;
use je_base::event::Event;
use je_base::extpath::is_equal;
use je_base::lifecycle::{DisposableRef, DisposableStore, IDisposable, to_disposable};
use je_base::map::ResourceMap;
use je_base::nls::localize;
use je_base::path::{Environment, PathModule, ProcessEnvironment};
use je_base::promise::Promise;
use je_base::resources::ExtUri;
use je_base::stream::WriteableStream;
use je_base::uri::URI;

use super::disk_provider_base::{AbstractDiskFileSystemProvider, CreateWatcherClient, DiskFileSystemProviderOptions};
use super::files::*;
use super::io::{CreateReadStreamOptions, read_file_into_stream};
use super::node_watcher::{nodejs_watcher_client, universal_watcher_client};
use super::pfs::{self, EntryKind, FsError, RenameRetry, RimRafMode, SymbolicLink, SymlinkStats};
use crate::log::LogServiceRef;

/// Not enabled by default because very spammy.
const TRACE_LOG_RESOURCE_LOCKS: bool = false;

/// Renames are only retried on Windows, by default for up to a minute, because antivirus software briefly
/// locks files that were just written (for example during atomic saves). A folder however is typically
/// locked because something inside it is in use by another application, which rarely resolves by itself, so
/// we only retry briefly to report the error quickly. This does not apply when overwriting, because an
/// existing target may already have been deleted and giving up early would leave it deleted without the
/// folder being moved.
const FOLDER_RENAME_RETRY_TIMEOUT: u64 = 1000;

/// O tamanho dos pedaços de `readFileStream`: 256 KB cada, para reduzir o custo de IPC.
const READ_STREAM_BUFFER_SIZE: usize = 256 * 1024;

/// O bit de escrita do dono (`stat.mode & 0o200`).
const OWNER_WRITE: u32 = 0o200;

/// `private static canFlush = true`.
static CAN_FLUSH: AtomicBool = AtomicBool::new(true);

/// O `error` de um `catch` do original: o erro de E/S do Node (com `code`) ou um erro que o provider já
/// montou (`FileSystemProviderError`).
enum Failure {
    Fs(FsError),
    Provider(Error),
}

impl From<FsError> for Failure {
    fn from(error: FsError) -> Self {
        Failure::Fs(error)
    }
}

impl From<Error> for Failure {
    fn from(error: Error) -> Self {
        Failure::Provider(error)
    }
}

impl Failure {
    /// O `error.code` do Node; um erro do provider não tem.
    fn code(&self) -> Option<&'static str> {
        match self {
            Failure::Fs(error) => error.code,
            Failure::Provider(_) => None,
        }
    }

    /// `error.toString()`.
    fn describe(&self) -> String {
        match self {
            Failure::Fs(error) => error.to_string(),
            Failure::Provider(error) => error.to_string(),
        }
    }

    fn provider(message: &str, code: FileSystemProviderErrorCode) -> Self {
        Failure::Provider(create_file_system_provider_error_from_text(message, code))
    }
}

/// `toFileSystemProviderError(error)`.
fn to_file_system_provider_error(failure: Failure) -> Error {
    let error = match failure {
        Failure::Provider(error) => return error, // avoid double conversion
        Failure::Fs(error) => error,
    };
    let code = match error.code {
        Some("ENOENT") => FileSystemProviderErrorCode::FileNotFound,
        Some("EISDIR") => FileSystemProviderErrorCode::FileIsADirectory,
        Some("ENOTDIR") => FileSystemProviderErrorCode::FileNotADirectory,
        Some("EEXIST") => FileSystemProviderErrorCode::FileExists,
        Some("EPERM" | "EACCES") => FileSystemProviderErrorCode::NoPermissions,
        _ => FileSystemProviderErrorCode::Unknown,
    };
    create_file_system_provider_error_from_text(&error.to_string(), code)
}

/// `await promise` de uma função que já terminou: a promessa já assentada.
fn settled<T: Clone + 'static>(result: Result<T, Error>) -> Promise<T> {
    match result {
        Ok(value) => Promise::resolved(value),
        Err(error) => Promise::rejected(error),
    }
}

fn trace_lock(log: &LogServiceRef, message: impl FnOnce() -> String) {
    if TRACE_LOG_RESOURCE_LOCKS {
        log.trace(&message());
    }
}

/// Um handle aberto: o arquivo, o recurso (só se aberto para escrita) e o lock do recurso.
struct Handle {
    file: Rc<File>,
    /// `writeHandles`.
    write_resource: Option<URI>,
    /// `mapHandleToLock`.
    lock: Option<DisposableRef>,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum MoveCopy {
    Move,
    Copy,
}

/// `DiskFileSystemProvider`.
pub struct DiskFileSystemProvider {
    this: Weak<DiskFileSystemProvider>,
    base: Rc<AbstractDiskFileSystemProvider>,
    capabilities: FileSystemProviderCapabilities,
    /// `extUri` do `resources.ts`, sem viés (`basename`, `dirname` e `joinPath` do original).
    ext_uri: ExtUri,
    /// `resource_locks`: o mapa fica num `Rc` porque o `dispose` do lock o usa depois que o provider pode ter saído de cena.
    resource_locks: Rc<RefCell<ResourceMap<Barrier>>>,
    next_handle: Cell<i64>,
    handles: RefCell<HashMap<i64, Handle>>,
    /// O provider que o mundo enxerga, quando é uma subclasse (ver [`set_outer`](Self::set_outer)).
    outer: RefCell<Option<Weak<dyn FileSystemProvider>>>,
}

impl DiskFileSystemProvider {
    /// `new DiskFileSystemProvider(logService, options?)`.
    pub fn new(log: LogServiceRef, options: Option<DiskFileSystemProviderOptions>) -> Rc<Self> {
        Self::with_watcher_clients(log, options, Rc::new(universal_watcher_client), Rc::new(nodejs_watcher_client))
    }

    /// O mesmo, com as fábricas de cliente de watcher injetadas (`createUniversalWatcher` e
    /// `createNonRecursiveWatcher` do original).
    pub fn with_watcher_clients(
        log: LogServiceRef,
        options: Option<DiskFileSystemProviderOptions>,
        create_universal_watcher: CreateWatcherClient,
        create_non_recursive_watcher: CreateWatcherClient,
    ) -> Rc<Self> {
        let env: Rc<dyn Environment> = Rc::new(ProcessEnvironment);
        let ext_uri_biased = ExtUri::biased_ignore_path_case(env.clone(), cfg!(target_os = "linux"));
        let mut capabilities = FileSystemProviderCapabilities::FILE_READ_WRITE
            | FileSystemProviderCapabilities::FILE_OPEN_READ_WRITE_CLOSE
            | FileSystemProviderCapabilities::FILE_READ_STREAM
            | FileSystemProviderCapabilities::FILE_FOLDER_COPY
            | FileSystemProviderCapabilities::FILE_WRITE_UNLOCK
            | FileSystemProviderCapabilities::FILE_APPEND
            | FileSystemProviderCapabilities::FILE_ATOMIC_READ
            | FileSystemProviderCapabilities::FILE_ATOMIC_WRITE
            | FileSystemProviderCapabilities::FILE_ATOMIC_DELETE
            | FileSystemProviderCapabilities::FILE_CLONE
            | FileSystemProviderCapabilities::FILE_REALPATH;
        if cfg!(target_os = "linux") {
            capabilities |= FileSystemProviderCapabilities::PATH_CASE_SENSITIVE;
        }

        Rc::new_cyclic(|this| Self {
            this: this.clone(),
            base: AbstractDiskFileSystemProvider::new(log, options, create_universal_watcher, create_non_recursive_watcher),
            capabilities,
            ext_uri: ExtUri::unbiased(env),
            resource_locks: Rc::new(RefCell::new(ResourceMap::with_key_fn(Rc::new(move |resource| ext_uri_biased.get_comparison_key(resource, false))))),
            next_handle: Cell::new(1),
            handles: RefCell::new(HashMap::new()),
            outer: RefCell::new(None),
        })
    }

    /// `static configureFlushOnWrite(enabled)`.
    pub fn configure_flush_on_write(enabled: bool) {
        CAN_FLUSH.store(enabled, Ordering::Relaxed);
    }

    /// Registra o provider que envolve este (a "subclasse" de um teste): `readFileStream` lê pelos
    /// `open/read/close` dele e as capacidades que a validação de mover e copiar consulta são as dele.
    pub fn set_outer(&self, outer: &Rc<dyn FileSystemProvider>) {
        *self.outer.borrow_mut() = Some(Rc::downgrade(outer));
    }

    fn rc(&self) -> Rc<Self> {
        self.this.upgrade().expect("o provider está vivo enquanto um método dele roda")
    }

    /// `this`, como o código do original o enxerga: a subclasse, se houver.
    fn virtual_self(&self) -> Rc<dyn FileSystemProvider> {
        let outer = self.outer.borrow().as_ref().and_then(Weak::upgrade);
        outer.unwrap_or_else(|| self.rc() as Rc<dyn FileSystemProvider>)
    }

    fn log(&self) -> &LogServiceRef {
        self.base.log_service()
    }

    fn to_file_path(&self, resource: &URI) -> String {
        self.base.to_file_path(resource)
    }

    fn native_path(&self) -> PathModule<'static> {
        PathModule::native(&ProcessEnvironment)
    }

    /// Roda o corpo `async` do método como a função `async` do original: já, até o primeiro `await`.
    fn run<T: Clone + 'static, F: std::future::Future<Output = Result<T, Error>> + 'static>(&self, body: impl FnOnce(Rc<Self>) -> F) -> Promise<T> {
        async_fn(body(self.rc()))
    }

    // region: metadados

    fn stat_sync(&self, resource: &URI) -> Result<Stat, Failure> {
        // cannot use fs.stat() here to support links properly
        let SymlinkStats { stat, symbolic_link } = pfs::symlink_stat(&self.to_file_path(resource))?;

        let mode = stat.mode();
        let mut permissions = None;
        if mode & OWNER_WRITE == 0 {
            permissions = Some(FilePermission::LOCKED);
        }
        if mode & 0o111 != 0 {
            permissions = Some(permissions.unwrap_or(FilePermission::NONE) | FilePermission::EXECUTABLE);
        }

        Ok(Stat {
            file_type: to_type(stat.kind(), symbolic_link),
            ctime: stat.birthtime_ms(), // intentionally not using ctime here, we want the creation time
            mtime: stat.mtime_ms(),
            size: stat.size(),
            permissions,
        })
    }

    /// `statIgnoreError(resource)`.
    fn stat_ignore_error(&self, resource: &URI) -> Option<Stat> {
        self.stat_sync(resource).ok()
    }

    fn readdir_sync(&self, resource: &URI) -> Result<Vec<(String, FileType)>, Error> {
        let children = pfs::readdir(&self.to_file_path(resource)).map_err(|error| to_file_system_provider_error(error.into()))?;

        // No original cada entrada é uma tarefa: as que não são link simbólico entram no resultado na hora e
        // as que são só depois do `await` do `stat` do alvo.
        let (links, others): (Vec<_>, Vec<_>) = children.into_iter().partition(|child| child.kind.is_symbolic_link);
        let mut result = Vec::with_capacity(links.len() + others.len());
        for child in others {
            result.push((child.name, to_type(child.kind, None)));
        }
        for child in links {
            // always resolve target the link points to if any
            match self.stat_sync(&self.ext_uri.join_path(resource, &[&child.name])) {
                Ok(stat) => result.push((child.name, stat.file_type)),
                // ignore errors for individual entries that can arise from permission denied
                Err(failure) => self.log().trace(&failure.describe()),
            }
        }
        Ok(result)
    }

    // endregion

    // region: lock por recurso

    /// `createResourceLock(resource)`.
    async fn create_resource_lock(&self, resource: &URI) -> DisposableRef {
        let file_path = self.to_file_path(resource);
        trace_lock(self.log(), || format!("[Disk FileSystemProvider]: createResourceLock() - request to acquire resource lock ({file_path})"));

        // Await pending locks for resource. It is possible for a new lock being added right after opening,
        // so we have to loop over locks until no lock remains.
        loop {
            let existing_lock = self.resource_locks.borrow().get(resource).cloned();
            let Some(existing_lock) = existing_lock else { break };
            trace_lock(self.log(), || format!("[Disk FileSystemProvider]: createResourceLock() - waiting for resource lock to be released ({file_path})"));
            let _ = existing_lock.wait().await;
        }

        // Store new
        let new_lock = Barrier::new();
        self.resource_locks.borrow_mut().set(resource.clone(), new_lock.clone());

        trace_lock(self.log(), || format!("[Disk FileSystemProvider]: createResourceLock() - new resource lock created ({file_path})"));

        let (locks, resource, log) = (self.resource_locks.clone(), resource.clone(), self.log().clone());
        to_disposable(move || {
            trace_lock(&log, || format!("[Disk FileSystemProvider]: createResourceLock() - resource lock dispose() ({file_path})"));

            // Delete lock if it is still ours
            let is_ours = locks.borrow().get(&resource).is_some_and(|current| current.wait().ptr_eq(&new_lock.wait()));
            if is_ours {
                trace_lock(&log, || format!("[Disk FileSystemProvider]: createResourceLock() - resource lock removed from resource-lock map ({file_path})"));
                locks.borrow_mut().delete(&resource);
            }

            // Open lock
            trace_lock(&log, || format!("[Disk FileSystemProvider]: createResourceLock() - resource lock barrier open() ({file_path})"));
            new_lock.open();
        })
    }

    // endregion

    // region: leitura e escrita

    async fn read_file_impl(self: &Rc<Self>, resource: &URI, atomic: bool) -> Result<VSBuffer, Error> {
        let mut lock = None;
        if atomic {
            trace_lock(self.log(), || format!("[Disk FileSystemProvider]: atomic read operation started ({})", self.to_file_path(resource)));

            // When the read should be atomic, make sure to await any pending locks for the resource and
            // lock for the duration of the read.
            lock = Some(self.create_resource_lock(resource).await);
        }

        let file_path = self.to_file_path(resource);
        let result = self.open_file_for_read(&file_path).and_then(|file| Ok(pfs::read_to_end(&file)?));
        if let Some(lock) = lock {
            lock.dispose();
        }
        result.map(VSBuffer::wrap).map_err(to_file_system_provider_error)
    }

    /// `openFileForRead(filePath)`: abre sem bloquear e recusa o que não é arquivo regular.
    fn open_file_for_read(&self, file_path: &str) -> Result<File, Failure> {
        let file = pfs::open_for_read(file_path)?;

        // O handle se fecha ao sair do escopo, também no erro (o `finally` do original).
        let stat = pfs::fstat(&file)?;
        if stat.is_directory() {
            return Err(Failure::provider(&localize("fileIsDirectory", "File is a directory", &[]), FileSystemProviderErrorCode::FileIsADirectory));
        }
        if !stat.is_file() {
            return Err(Failure::provider(&localize("fileNotRegular", "File is not a regular file", &[]), FileSystemProviderErrorCode::Unavailable));
        }

        Ok(file)
    }

    async fn write_file_impl(self: &Rc<Self>, resource: &URI, content: VSBuffer, opts: FileWriteOptions) -> Result<(), Error> {
        if let Some(atomic) = &opts.atomic
            && !atomic.postfix.is_empty()
            && self.can_write_file_atomic(resource)
        {
            let temp_resource = self.ext_uri.join_path(&self.ext_uri.dirname(resource), &[&format!("{}{}", self.ext_uri.basename(resource, None), atomic.postfix)]);
            self.do_write_file_atomic(resource, &temp_resource, &content, &opts).await
        } else {
            self.do_write_file(resource, &content, &opts, false).await
        }
    }

    fn can_write_file_atomic(&self, resource: &URI) -> bool {
        // ignore stat errors here and just proceed trying to write
        if let Ok(SymlinkStats { symbolic_link: Some(_), .. }) = pfs::symlink_stat(&self.to_file_path(resource)) {
            // atomic writes are unsupported for symbolic links because we need to ensure that the `rename`
            // operation is atomic and that only works if the link is on the same disk. Since we do not know
            // where the symbolic link points to we refuse to write atomically.
            return false;
        }

        true // atomic writing supported
    }

    async fn do_write_file_atomic(self: &Rc<Self>, resource: &URI, temp_resource: &URI, content: &VSBuffer, opts: &FileWriteOptions) -> Result<(), Error> {
        // Ensure to create locks for all resources involved since atomic write involves mutiple disk
        // operations and resources.
        let locks = DisposableStore::new();
        let result = self.write_atomic_with_locks(&locks, resource, temp_resource, content, opts).await;
        locks.dispose();
        result
    }

    async fn write_atomic_with_locks(
        self: &Rc<Self>,
        locks: &Rc<DisposableStore>,
        resource: &URI,
        temp_resource: &URI,
        content: &VSBuffer,
        opts: &FileWriteOptions,
    ) -> Result<(), Error> {
        locks.add(self.create_resource_lock(resource).await);
        locks.add(self.create_resource_lock(temp_resource).await);

        // Write to temp resource first
        self.do_write_file(temp_resource, content, &FileWriteOptions { create: true, overwrite: true, ..opts.clone() }, true /* disable write lock */).await?;

        // Rename over existing to ensure atomic replace
        if let Err(error) = self.rename_impl(temp_resource, resource, true).await {
            // Cleanup in case of rename error: ignore - we want the outer error to bubble up
            let _ = self.delete_impl(temp_resource, &FileDeleteOptions { recursive: false, use_trash: false, atomic: None }).await;

            return Err(error);
        }
        Ok(())
    }

    async fn do_write_file(self: &Rc<Self>, resource: &URI, content: &VSBuffer, opts: &FileWriteOptions, disable_write_lock: bool) -> Result<(), Error> {
        let mut handle: Option<i64> = None;
        let result: Result<(), Failure> = async {
            let file_path = self.to_file_path(resource);

            // Validate target unless { create: true, overwrite: true }
            if !opts.create || !opts.overwrite {
                if pfs::exists(&file_path) {
                    if !opts.overwrite {
                        return Err(Failure::provider(&localize("fileExists", "File already exists", &[]), FileSystemProviderErrorCode::FileExists));
                    }
                } else if !opts.create {
                    return Err(Failure::provider(&localize("fileNotExists", "File does not exist", &[]), FileSystemProviderErrorCode::FileNotFound));
                }
            }

            // Open
            let fd = self.open_impl(resource, FileOpenOptions { create: true, unlock: opts.unlock, append: opts.append }, disable_write_lock).await?;
            handle = Some(fd);

            // Write content at once
            let total = content.byte_length();
            let mut written = 0;
            loop {
                let bytes = self.write_impl(fd, written as u64, content, written, total - written).await?;
                written += bytes;
                if written >= total || bytes == 0 {
                    break;
                }
            }
            Ok(())
        }
        .await;

        let result = result.map_err(|failure| self.to_write_error(Some(resource), failure));
        if let Some(fd) = handle {
            self.close_sync(fd)?; // o `finally { await this.close(handle) }`: o erro do close substitui o do corpo
        }
        result
    }

    /// `open(resource, opts, disableWriteLock?)`.
    async fn open_impl(self: &Rc<Self>, resource: &URI, opts: FileOpenOptions, disable_write_lock: bool) -> Result<i64, Error> {
        let file_path = self.to_file_path(resource);
        let is_write = opts.is_for_write();

        // Writes: guard multiple writes to the same resource behind a single lock to prevent races when
        // writing from multiple places at the same time to the same file
        let mut lock = None;
        if is_write && !disable_write_lock {
            lock = Some(self.create_resource_lock(resource).await);
        }

        let file = match self.open_file(&file_path, &opts) {
            Ok(file) => file,
            Err(failure) => {
                // Release lock because we have no valid handle if we did open a lock during this operation
                if let Some(lock) = lock {
                    lock.dispose();
                }

                // Rethrow as file system provider error
                return Err(if is_write { self.to_write_error(Some(resource), failure) } else { to_file_system_provider_error(failure) });
            }
        };

        let handle = self.next_handle.get();
        self.next_handle.set(handle + 1);
        if lock.is_some() {
            trace_lock(self.log(), || format!("[Disk FileSystemProvider]: open() - storing lock for handle {handle} ({file_path})"));
        }
        self.handles.borrow_mut().insert(handle, Handle { file: Rc::new(file), write_resource: is_write.then(|| resource.clone()), lock });
        Ok(handle)
    }

    fn open_file(&self, file_path: &str, opts: &FileOpenOptions) -> Result<File, Failure> {
        let is_write = opts.is_for_write();

        // Determine whether to unlock the file (write only)
        if is_write && opts.unlock {
            match pfs::symlink_stat(file_path) {
                Ok(SymlinkStats { stat, .. }) => {
                    let mode = stat.mode();
                    if mode & OWNER_WRITE == 0 {
                        // File mode indicating writable by owner
                        if let Err(error) = pfs::chmod(file_path, mode | OWNER_WRITE) {
                            self.log_unless_not_found(&error);
                        }
                    }
                }
                Err(error) => self.log_unless_not_found(&error),
            }
        }

        // Windows gets special treatment (write only, but not for append)
        if cfg!(windows) && is_write && !opts.append {
            // We try to use 'r+' for opening (which will fail if the file does not exist) to prevent issues
            // when saving hidden files or preserving alternate data streams.
            // Related issues:
            // - https://github.com/microsoft/vscode/issues/931
            // - https://github.com/microsoft/vscode/issues/6363
            match OpenOptions::new().read(true).write(true).open(file_path) {
                Ok(file) => {
                    // The flag 'r+' will not truncate the file, so we have to do this manually
                    match file.set_len(0) {
                        Ok(()) => return Ok(file),
                        Err(error) => self.log_unless_not_found(&FsError::from_io(&error, "ftruncate", None, None)), // log errors but do not give up writing
                    }
                }
                Err(error) => self.log_unless_not_found(&FsError::from_io(&error, "open", Some(file_path), None)),
            }
        }

        if !is_write {
            return self.open_file_for_read(file_path);
        }

        // We take `opts.create` as a hint that the file is opened for writing as such we use 'w' to
        // truncate an existing or create the file otherwise. we do not allow reading.
        // If `opts.append` is true, use 'a' to append to the file.
        let mut options = OpenOptions::new();
        if opts.append {
            options.append(true).create(true);
        } else {
            options.write(true).create(true).truncate(true);
        }
        Ok(options.open(file_path).map_err(pfs::fs_err("open", file_path))?)
    }

    fn log_unless_not_found(&self, error: &FsError) {
        if error.code != Some("ENOENT") {
            self.log().trace(&error.to_string()); // log errors but do not give up writing
        }
    }

    /// `close(fd)`.
    fn close_sync(&self, fd: i64) -> Result<(), Error> {
        // It is very important that we keep any associated lock for the file handle before releasing the
        // handle: the lock só é liberado depois do fechamento.
        let Some(handle) = self.handles.borrow_mut().remove(&fd) else {
            return Err(to_file_system_provider_error(pfs::bad_file_descriptor("close").into()));
        };

        // If a handle is closed that was used for writing, ensure to flush the contents to disk if possible.
        if handle.write_resource.is_some()
            && CAN_FLUSH.load(Ordering::Relaxed)
            && let Err(error) = handle.file.sync_data()
        {
            // In some exotic setups it is well possible that node fails to sync. In that case we disable
            // flushing and log the error to our logger
            Self::configure_flush_on_write(false);
            self.log().error(&FsError::from_io(&error, "fdatasync", None, None).to_string());
        }

        drop(handle.file);
        if let Some(lock) = handle.lock {
            trace_lock(self.log(), || format!("[Disk FileSystemProvider]: close() - disposing lock for handle {fd}"));
            lock.dispose();
        }
        Ok(())
    }

    /// `read(fd, pos, data, offset, length)`.
    fn read_sync(&self, fd: i64, pos: u64, data: &VSBuffer, offset: usize, length: usize) -> Result<usize, Error> {
        let file = self.handles.borrow().get(&fd).map(|handle| handle.file.clone());
        let result = match file {
            Some(file) if offset.checked_add(length).is_some_and(|end| end <= data.byte_length()) => {
                let mut buffer = vec![0; length];
                pfs::read_at(&file, &mut buffer, pos).inspect(|&bytes_read| data.set_bytes(&buffer[..bytes_read], offset))
            }
            Some(_) => Err(FsError::other(format!("The value of \"length\" is out of range: {length} (offset {offset}, buffer {})", data.byte_length()))),
            None => Err(pfs::bad_file_descriptor("read")),
        };
        result.map_err(|error| to_file_system_provider_error(error.into()))
    }

    /// `write(fd, pos, data, offset, length)`: We know at this point that the file to write to is truncated
    /// and thus empty if the write now fails, the file remains empty. as such we really try hard to ensure
    /// the write succeeds by retrying up to three times.
    async fn write_impl(self: &Rc<Self>, fd: i64, pos: u64, data: &VSBuffer, offset: usize, length: usize) -> Result<usize, Error> {
        let (this, data) = (self.clone(), data.clone());
        retry(move || settled(this.do_write(fd, pos, &data, offset, length)), 100 /* ms delay */, 3 /* retries */).await
    }

    fn do_write(&self, fd: i64, pos: u64, data: &VSBuffer, offset: usize, length: usize) -> Result<usize, Error> {
        let handle = self.handles.borrow().get(&fd).map(|handle| (handle.file.clone(), handle.write_resource.clone()));
        let Some((file, write_resource)) = handle else {
            return Err(self.to_write_error(None, pfs::bad_file_descriptor("write").into()));
        };
        let result = match offset.checked_add(length).filter(|end| *end <= data.byte_length()) {
            Some(end) => data.with_bytes(|bytes| pfs::write_at(&file, &bytes[offset..end], pos)),
            None => Err(FsError::other(format!("The value of \"length\" is out of range: {length} (offset {offset}, buffer {})", data.byte_length()))),
        };
        result.map_err(|error| self.to_write_error(write_resource.as_ref(), error.into()))
    }

    // endregion

    // region: mover, copiar, apagar, criar pasta

    async fn delete_impl(&self, resource: &URI, opts: &FileDeleteOptions) -> Result<(), Error> {
        self.delete_raw(resource, opts).await.map_err(to_file_system_provider_error)
    }

    async fn delete_raw(&self, resource: &URI, opts: &FileDeleteOptions) -> Result<(), Failure> {
        let file_path = self.to_file_path(resource);
        if opts.recursive {
            let mut rm_move_to_path = None;
            if let Some(atomic) = &opts.atomic
                && !atomic.postfix.is_empty()
            {
                let module = self.native_path();
                rm_move_to_path = Some(module.join(&[&module.dirname(&file_path), &format!("{}{}", module.basename(&file_path, None), atomic.postfix)]));
            }

            pfs::rm(&file_path, RimRafMode::Move, rm_move_to_path.as_deref()).await?;
            return Ok(());
        }

        let unlink_error = match std::fs::remove_file(&file_path) {
            Ok(()) => return Ok(()),
            Err(error) => FsError::from_io(&error, "unlink", Some(&file_path), None),
        };

        // `fs.unlink` will throw when used on directories we try to detect this error and then see if the
        // provided resource is actually a directory. in that case we use `fs.rmdir` to delete the directory.
        if matches!(unlink_error.code, Some("EPERM" | "EISDIR")) {
            let is_directory = pfs::symlink_stat(&file_path).is_ok_and(|result| result.stat.is_directory() && result.symbolic_link.is_none());
            if is_directory {
                std::fs::remove_dir(&file_path).map_err(pfs::fs_err("rmdir", &file_path))?;
                return Ok(());
            }
        }
        Err(unlink_error.into())
    }

    async fn rename_impl(&self, from: &URI, to: &URI, overwrite: bool) -> Result<(), Error> {
        let (from_file_path, to_file_path) = (self.to_file_path(from), self.to_file_path(to));

        if from_file_path == to_file_path {
            return Ok(()); // simulate node.js behaviour here and do a no-op if paths match
        }

        let renamed: Result<(), Failure> = async {
            // Validate the move operation can perform
            let from_stat = self.validate_move_copy(from, to, MoveCopy::Move, overwrite).await?;

            // Rename
            let is_folder = from_stat.file_type.intersects(FileType::DIRECTORY);
            let retry = if is_folder && !overwrite { RenameRetry::Millis(FOLDER_RENAME_RETRY_TIMEOUT) } else { RenameRetry::Default };
            pfs::rename(&from_file_path, &to_file_path, retry).await?;
            Ok(())
        }
        .await;

        let Err(failure) = renamed else { return Ok(()) };
        let module = self.native_path();
        let (from_name, to_dir) = (module.basename(&from_file_path, None), module.dirname(&to_file_path));
        let code = failure.code();

        // On Windows, a rename fails when the file or folder (or anything inside the folder) is in use by
        // another application. node.js reports this with the same error codes as a permission problem, so
        // we mention both causes.
        let failure = if cfg!(windows) && matches!(code, Some("EPERM" | "EACCES" | "EBUSY")) {
            let error = failure.describe();
            let message = if is_equal(&module.dirname(&from_file_path), &to_dir, true /* ignore case */) {
                localize(
                    "renameInUseError",
                    "Unable to rename '{0}' to '{1}'. It may be in use by another application, or you may not have permission ({2}).",
                    &[&from_name, &module.basename(&to_file_path, None), &error],
                )
            } else {
                localize(
                    "moveInUseError",
                    "Unable to move '{0}' into '{1}'. It may be in use by another application, or you may not have permission ({2}).",
                    &[&from_name, &module.basename(&to_dir, None), &error],
                )
            };
            Failure::provider(&message, if code == Some("EBUSY") { FileSystemProviderErrorCode::Unknown } else { FileSystemProviderErrorCode::NoPermissions })
        }
        // Rewrite some typical errors that can happen especially around symlinks to something the user can
        // better understand
        else if matches!(code, Some("EINVAL" | "EBUSY" | "ENAMETOOLONG")) {
            let message = localize("moveError", "Unable to move '{0}' into '{1}' ({2}).", &[&from_name, &module.basename(&to_dir, None), &failure.describe()]);
            Failure::Fs(FsError::other(message))
        } else {
            failure
        };
        Err(to_file_system_provider_error(failure))
    }

    async fn copy_impl(&self, from: &URI, to: &URI, overwrite: bool) -> Result<(), Error> {
        let (from_file_path, to_file_path) = (self.to_file_path(from), self.to_file_path(to));

        if from_file_path == to_file_path {
            return Ok(()); // simulate node.js behaviour here and do a no-op if paths match
        }

        let copied: Result<(), Failure> = async {
            // Validate the copy operation can perform
            self.validate_move_copy(from, to, MoveCopy::Copy, overwrite).await?;

            // Copy
            pfs::copy(&from_file_path, &to_file_path, true)?;
            Ok(())
        }
        .await;

        let Err(failure) = copied else { return Ok(()) };

        // Rewrite some typical errors that can happen especially around symlinks to something the user can
        // better understand
        let failure = if matches!(failure.code(), Some("EINVAL" | "EBUSY" | "ENAMETOOLONG")) {
            let module = self.native_path();
            let message = localize("copyError", "Unable to copy '{0}' into '{1}' ({2}).", &[&module.basename(&from_file_path, None), &module.basename(&module.dirname(&to_file_path), None), &failure.describe()]);
            Failure::Fs(FsError::other(message))
        } else {
            failure
        };
        Err(to_file_system_provider_error(failure))
    }

    async fn validate_move_copy(&self, from: &URI, to: &URI, mode: MoveCopy, overwrite: bool) -> Result<Stat, Failure> {
        let (from_file_path, to_file_path) = (self.to_file_path(from), self.to_file_path(to));

        let Some(from_stat) = self.stat_ignore_error(from) else {
            return Err(Failure::provider(&localize("fileMoveCopyErrorNotFound", "File to move/copy does not exist", &[]), FileSystemProviderErrorCode::FileNotFound));
        };

        let is_path_case_sensitive = self.virtual_self().capabilities().intersects(FileSystemProviderCapabilities::PATH_CASE_SENSITIVE);
        let is_same_resource_with_different_path_case = !is_path_case_sensitive && is_equal(&from_file_path, &to_file_path, true /* ignore case */);

        if is_same_resource_with_different_path_case {
            // You cannot copy the same file to the same location with different path case unless you are on
            // a case sensitive file system
            if mode == MoveCopy::Copy {
                return Err(Failure::provider(
                    &localize("fileCopyErrorPathCase", "File cannot be copied to same path with different path case", &[]),
                    FileSystemProviderErrorCode::FileExists,
                ));
            }

            // You can move the same file to the same location with different path case on case insensitive
            // file systems
            return Ok(from_stat);
        }

        // Here we have to see if the target to move/copy to exists or not. We need to respect the
        // `overwrite` option to throw in case the target exists.

        let Some(to_stat) = self.stat_ignore_error(to) else {
            return Ok(from_stat); // target does not exist so we are good
        };

        if !overwrite {
            return Err(Failure::provider(
                &localize("fileMoveCopyErrorExists", "File at target already exists and thus will not be moved/copied to unless overwrite is specified", &[]),
                FileSystemProviderErrorCode::FileExists,
            ));
        }

        // Handle existing target for move/copy
        if from_stat.file_type.intersects(FileType::FILE) && to_stat.file_type.intersects(FileType::FILE) {
            return Ok(from_stat); // node.js can move/copy a file over an existing file without having to delete it first
        }
        self.delete_impl(to, &FileDeleteOptions { recursive: true, use_trash: false, atomic: None }).await?;

        Ok(from_stat)
    }

    // endregion

    // region: clonar

    /// `cloneFile(from, to)`: `doCloneFile(from, to, false)` e, se a pasta de destino não existe, de novo
    /// com `mkdir` (o original chama a si mesmo; um laço dá o mesmo resultado).
    async fn clone_file_impl(&self, from: &URI, to: &URI) -> Result<(), Error> {
        let mut mkdir = false; // optimistically assume parent folders exist
        loop {
            match self.do_clone_file(from, to, mkdir).await {
                Err(failure) if failure.code() == Some("ENOENT") && !mkdir => mkdir = true,
                result => return result.map_err(to_file_system_provider_error),
            }
        }
    }

    async fn do_clone_file(&self, from: &URI, to: &URI, mkdir: bool) -> Result<(), Failure> {
        let (from_file_path, to_file_path) = (self.to_file_path(from), self.to_file_path(to));

        let is_path_case_sensitive = self.virtual_self().capabilities().intersects(FileSystemProviderCapabilities::PATH_CASE_SENSITIVE);
        if is_equal(&from_file_path, &to_file_path, !is_path_case_sensitive) {
            return Ok(()); // cloning is only supported `from` and `to` are different files
        }

        // Implement clone by using `fs.copyFile`, however setup locks for both `from` and `to` because
        // node.js does not ensure this to be an atomic operation
        let locks = DisposableStore::new();
        locks.add(self.create_resource_lock(from).await);
        locks.add(self.create_resource_lock(to).await);

        let result = (|| -> Result<(), FsError> {
            if mkdir {
                pfs::mkdir_recursive(&self.native_path().dirname(&to_file_path), None)?;
            }
            std::fs::copy(&from_file_path, &to_file_path).map_err(pfs::fs_err2("copyfile", &from_file_path, &to_file_path))?;
            Ok(())
        })();
        locks.dispose();
        Ok(result?)
    }

    // endregion

    /// `toFileSystemProviderWriteError(resource, error)`.
    fn to_write_error(&self, resource: Option<&URI>, failure: Failure) -> Error {
        let original = failure.describe();
        let error = to_file_system_provider_error(failure);

        // If the write error signals permission issues, we try to read the file's mode to see if the file is
        // write locked.
        if let Some(resource) = resource
            && to_file_system_provider_error_code(Some(&error)) == FileSystemProviderErrorCode::NoPermissions
        {
            match pfs::symlink_stat(&self.to_file_path(resource)) {
                Ok(SymlinkStats { stat, .. }) if stat.mode() & OWNER_WRITE == 0 => {
                    return create_file_system_provider_error_from_text(&original, FileSystemProviderErrorCode::FileWriteLocked);
                }
                Ok(_) => {}
                Err(failure) => self.log().trace(&failure.to_string()), // ignore - return original error
            }
        }

        error
    }
}

/// `toType(entry, symbolicLink?)`.
fn to_type(entry: EntryKind, symbolic_link: Option<SymbolicLink>) -> FileType {
    // Signal file type by checking for file / directory, except:
    // - symbolic links pointing to nonexistent files are FileType.Unknown
    // - files that are neither file nor directory are FileType.Unknown
    let mut file_type = if symbolic_link.is_some_and(|link| link.dangling) {
        FileType::UNKNOWN
    } else if entry.is_file {
        FileType::FILE
    } else if entry.is_directory {
        FileType::DIRECTORY
    } else {
        FileType::UNKNOWN
    };

    // Always signal symbolic link as file type additionally
    if symbolic_link.is_some() {
        file_type |= FileType::SYMBOLIC_LINK;
    }

    file_type
}

impl IDisposable for DiskFileSystemProvider {
    fn dispose(&self) {
        self.base.dispose();
    }
}

impl FileSystemProvider for DiskFileSystemProvider {
    fn capabilities(&self) -> FileSystemProviderCapabilities {
        self.capabilities
    }

    fn on_did_change_capabilities(&self) -> Event<()> {
        Event::none()
    }

    fn on_did_change_file(&self) -> Event<Vec<FileChange>> {
        self.base.on_did_change_file()
    }

    fn on_did_watch_error(&self) -> Option<Event<String>> {
        Some(self.base.on_did_watch_error())
    }

    fn watch(&self, resource: &URI, opts: &WatchOptions) -> DisposableRef {
        self.base.watch(resource, opts)
    }

    fn stat(&self, resource: &URI) -> Promise<Stat> {
        settled(self.stat_sync(resource).map_err(to_file_system_provider_error))
    }

    fn mkdir(&self, resource: &URI) -> Promise<()> {
        let file_path = self.to_file_path(resource);
        settled(std::fs::create_dir(&file_path).map_err(|error| to_file_system_provider_error(FsError::from_io(&error, "mkdir", Some(&file_path), None).into())))
    }

    fn readdir(&self, resource: &URI) -> Promise<Vec<(String, FileType)>> {
        settled(self.readdir_sync(resource))
    }

    fn delete(&self, resource: &URI, opts: &FileDeleteOptions) -> Promise<()> {
        let (resource, opts) = (resource.clone(), opts.clone());
        self.run(move |this| async move { this.delete_impl(&resource, &opts).await })
    }

    fn rename(&self, from: &URI, to: &URI, opts: &FileOverwriteOptions) -> Promise<()> {
        let (from, to, overwrite) = (from.clone(), to.clone(), opts.overwrite);
        self.run(move |this| async move { this.rename_impl(&from, &to, overwrite).await })
    }

    fn copy(&self, from: &URI, to: &URI, opts: &FileOverwriteOptions) -> Promise<()> {
        let (from, to, overwrite) = (from.clone(), to.clone(), opts.overwrite);
        self.run(move |this| async move { this.copy_impl(&from, &to, overwrite).await })
    }

    fn read_file(&self, resource: &URI, atomic: bool) -> Promise<VSBuffer> {
        let resource = resource.clone();
        self.run(move |this| async move { this.read_file_impl(&resource, atomic).await })
    }

    fn write_file(&self, resource: &URI, content: VSBuffer, opts: &FileWriteOptions) -> Promise<()> {
        let (resource, opts) = (resource.clone(), opts.clone());
        self.run(move |this| async move { this.write_file_impl(&resource, content, opts).await })
    }

    fn read_file_stream(&self, resource: &URI, opts: &FileReadStreamOptions, token: &CancellationToken) -> VSBufferReadableStream {
        let stream = new_writeable_buffer_stream(None);

        // A promessa não é esperada, como no original: `read_file_into_stream` entrega o erro no stream.
        let options = CreateReadStreamOptions { stream: *opts, buffer_size: READ_STREAM_BUFFER_SIZE, error_transformer: None };
        let _ = read_file_into_stream(self.virtual_self(), resource.clone(), stream.clone() as Rc<dyn WriteableStream<VSBuffer>>, options, token.clone());

        stream
    }

    fn open(&self, resource: &URI, opts: &FileOpenOptions) -> Promise<i64> {
        let (resource, opts) = (resource.clone(), *opts);
        self.run(move |this| async move { this.open_impl(&resource, opts, false).await })
    }

    fn close(&self, fd: i64) -> Promise<()> {
        settled(self.close_sync(fd))
    }

    fn read(&self, fd: i64, pos: u64, data: &VSBuffer, offset: usize, length: usize) -> Promise<usize> {
        settled(self.read_sync(fd, pos, data, offset, length))
    }

    fn write(&self, fd: i64, pos: u64, data: &VSBuffer, offset: usize, length: usize) -> Promise<usize> {
        let data = data.clone();
        self.run(move |this| async move { this.write_impl(fd, pos, &data, offset, length).await })
    }

    fn clone_file(&self, from: &URI, to: &URI) -> Promise<()> {
        let (from, to) = (from.clone(), to.clone());
        self.run(move |this| async move { this.clone_file_impl(&from, &to).await })
    }

    fn realpath(&self, resource: &URI) -> Promise<String> {
        settled(pfs::realpath(&self.to_file_path(resource)).map_err(|error| to_file_system_provider_error(error.into())))
    }
}
