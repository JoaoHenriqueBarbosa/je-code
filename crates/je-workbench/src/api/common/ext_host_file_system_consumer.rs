//! Porte de `src/vs/workbench/api/common/extHostFileSystemConsumer.ts`: o `vscode.workspace.fs` do lado das extensões. As
//! operações vão pelo atalho (direto no provedor registrado pela própria extensão host, depois de garantir a ativação do
//! esquema) quando há provedor para o esquema, e pela thread principal quando não há (ou quando a operação pode cruzar
//! esquemas, como `rename` e `copy`). Os erros saem como `FileSystemError`.
//!
//! Este módulo também guarda o contrato `vscode.FileSystemProvider` ([`ApiFileSystemProvider`]) e os tipos que ele usa
//! ([`ApiFileStat`], [`ApiFileChangeEvent`], [`FileSystemProviderOptions`]), porque o consumidor e `ext_host_file_system`
//! compartilham o contrato e este é o módulo de que o outro depende.
//!
//! Divergências:
//! - `IExtHostRpcService` é o [`IMainContext`] (só serve para pedir o proxy `MainThreadFileSystem`), e
//!   `IExtHostConsumerFileSystem` (o decorator e a interface) é o próprio [`ExtHostConsumerFileSystem`], que o `Rc` do
//!   construtor compartilha. `IExtHostFileSystemInfo` é o trait de `ext_host_workspace` (só o `getCapabilities`).
//! - `readonly value: vscode.FileSystem` é [`ExtHostConsumerFileSystem::value`], que entrega um [`ApiFileSystem`] novo (um
//!   punho barato sobre o mesmo consumidor) em vez de um objeto congelado único: a identidade do objeto não é observável.
//! - `vscode.FileType` é o `FileType` de `files` (mesmos valores: `File = 1`, `Directory = 2`, `SymbolicLink = 64`) e
//!   `vscode.FilePermission` é [`ApiFilePermission`] (só `Readonly = 1`). `vscode.FileStat` é [`ApiFileStat`].
//! - As funções de `vscode.FileSystem` devolvem [`Promise`] e o `try { ... } catch (err) { _handleError(err) }` de cada
//!   uma é [`guarded`], que roda o corpo como função `async` e passa o erro por [`ExtHostConsumerFileSystem::handle_error`].
//! - `_handleError` devolve o [`Error`] que o original lançaria (o `throw` vira a rejeição da promessa). Como `Error`
//!   é a única forma de erro, `FileSystemError` e `FileSystemProviderError` têm a mesma representação (um erro marcado
//!   com o código): os dois primeiros ramos do original, que ou relançam o `FileSystemError` ou o reconstroem pela fábrica
//!   com a mesma mensagem, produzem o mesmo erro e viram um só (o erro é devolvido como está). O ramo "não é um `Error`"
//!   (`String(err)`) não existe, porque toda rejeição é um [`Error`]. No ramo final, o `err.name` arbitrário vira o código
//!   do erro marcado, como em `new FileSystemError(err.message, err.name)`.
//! - `isReadonly?: boolean | IMarkdownString` é [`FileSystemReadonly`], com o `vscode.MarkdownString` (que o
//!   `IMarkdownString` aceita) na variante de mensagem. `new ResourceQueue()` usa o `extUri` padrão do processo.
//! - `Uint8Array` é `Vec<u8>` (o `.slice()` de cópia segura é a posse do vetor) e `ProviderResult<T>` é
//!   [`MaybePromise`]. O `Promise.resolve(x)` de `x: T | Thenable<T>` é [`into_promise`].
//! - `FileSystemProvider#watch(uri, options)` recebe as [`WatchOptions`] completas, como o original repassa o
//!   `IWatchOptions`; a operação opcional `copy` e o quarteto `open`/`close`/`read`/`write` (a proposta `fsChunks`) são
//!   métodos com um `has_*` que faz o papel do `typeof provider.x === 'function'`. `read` e `write` recebem o
//!   [`VSBuffer`], porque o provedor pode escrever nele depois de ceder (o `Uint8Array` compartilhado do original).

use std::cell::RefCell;
use std::collections::HashMap;
use std::future::Future;
use std::rc::Rc;

use je_base::async_::{ResourceQueue, async_fn};
use je_base::buffer::VSBuffer;
use je_base::errors::Error;
use je_base::event::Event;
use je_base::lifecycle::{DisposableRef, to_disposable};
use je_base::network::schemas;
use je_base::promise::{MaybePromise, Promise};
use je_base::resources::ExtUri;
use je_base::uri::URI;
use je_platform::files::files::{
    FileDeleteOptions, FileOpenOptions, FileOverwriteOptions, FilePermission, FileSystemProviderCapabilities, FileSystemProviderErrorCode, FileType, FileWriteOptions, Stat,
    WatchOptions, to_file_system_provider_error_code,
};
use je_platform::resources::{ext_uri as new_ext_uri, fs_path, host_environment};

use super::ext_host_protocol::{IMainContext, MainContext, MainThreadFileSystemShape};
use super::ext_host_types::{FileChangeType, FileSystemError, MarkdownString, MessageOrUri};
use super::ext_host_workspace::IExtHostFileSystemInfo;

/// O sufixo do nome de um erro marcado como erro do provedor de arquivos (`createFileSystemProviderError`).
pub const FILE_SYSTEM_ERROR_SUFFIX: &str = " (FileSystemError)";

/// Os códigos de erro do provedor de arquivos (`FileSystemProviderErrorCode`), para reconhecer o erro marcado.
const ERROR_CODES: [FileSystemProviderErrorCode; 10] = [
    FileSystemProviderErrorCode::FileExists,
    FileSystemProviderErrorCode::FileNotFound,
    FileSystemProviderErrorCode::FileNotADirectory,
    FileSystemProviderErrorCode::FileIsADirectory,
    FileSystemProviderErrorCode::FileExceedsStorageQuota,
    FileSystemProviderErrorCode::FileTooLarge,
    FileSystemProviderErrorCode::FileWriteLocked,
    FileSystemProviderErrorCode::NoPermissions,
    FileSystemProviderErrorCode::Unavailable,
    FileSystemProviderErrorCode::Unknown,
];

/// Os códigos que `_handleError` converte pela fábrica de `FileSystemError`, com a fábrica de cada um.
const MAPPED_ERRORS: [(FileSystemProviderErrorCode, fn(Option<MessageOrUri>) -> FileSystemError); 6] = [
    (FileSystemProviderErrorCode::FileExists, FileSystemError::file_exists),
    (FileSystemProviderErrorCode::FileNotFound, FileSystemError::file_not_found),
    (FileSystemProviderErrorCode::FileNotADirectory, FileSystemError::file_not_a_directory),
    (FileSystemProviderErrorCode::FileIsADirectory, FileSystemError::file_is_a_directory),
    (FileSystemProviderErrorCode::NoPermissions, FileSystemError::no_permissions),
    (FileSystemProviderErrorCode::Unavailable, FileSystemError::unavailable),
];

/// `vscode.FilePermission`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ApiFilePermission {
    Readonly = 1,
}

/// `vscode.FileStat`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ApiFileStat {
    pub file_type: FileType,
    pub ctime: i64,
    pub mtime: i64,
    pub size: u64,
    pub permissions: Option<ApiFilePermission>,
}

impl ApiFileStat {
    /// O `vscode.FileStat` de um `files.IStat`: só o `Readonly` (comparado por igualdade, como `===`) sobrevive.
    pub fn from_stat(stat: &Stat) -> ApiFileStat {
        ApiFileStat {
            file_type: stat.file_type,
            ctime: stat.ctime,
            mtime: stat.mtime,
            size: stat.size,
            permissions: (stat.permissions == Some(FilePermission::READONLY)).then_some(ApiFilePermission::Readonly),
        }
    }

    /// O `files.IStat` de um `vscode.FileStat` (`_asIStat` de `extHostFileSystem.ts`).
    pub fn to_stat(&self) -> Stat {
        Stat {
            file_type: self.file_type,
            mtime: self.mtime,
            ctime: self.ctime,
            size: self.size,
            permissions: self.permissions.map(|_| FilePermission::READONLY),
        }
    }
}

/// `vscode.FileChangeEvent`: o `type` é o `FileChangeType` da API (`Changed`, `Created`, `Deleted`).
#[derive(Clone, Debug)]
pub struct ApiFileChangeEvent {
    pub change_type: FileChangeType,
    pub uri: URI,
}

/// `vscode.FileSystemProvider`.
pub trait ApiFileSystemProvider {
    /// `onDidChangeFile`.
    fn on_did_change_file(&self) -> Event<Vec<ApiFileChangeEvent>>;

    /// `watch(uri, options)`.
    fn watch(&self, uri: &URI, options: &WatchOptions) -> DisposableRef;

    fn stat(&self, uri: &URI) -> MaybePromise<ApiFileStat>;

    fn read_directory(&self, uri: &URI) -> MaybePromise<Vec<(String, FileType)>>;

    fn create_directory(&self, uri: &URI) -> MaybePromise<()>;

    fn read_file(&self, uri: &URI) -> MaybePromise<Vec<u8>>;

    fn write_file(&self, uri: &URI, content: &[u8], options: &FileWriteOptions) -> MaybePromise<()>;

    fn delete(&self, uri: &URI, options: &FileDeleteOptions) -> MaybePromise<()>;

    fn rename(&self, old_uri: &URI, new_uri: &URI, options: &FileOverwriteOptions) -> MaybePromise<()>;

    /// `typeof provider.copy === 'function'`.
    fn has_copy(&self) -> bool {
        false
    }

    /// `copy(source, destination, options)`; só é chamado quando [`Self::has_copy`] é verdadeiro.
    fn copy(&self, _source: &URI, _destination: &URI, _options: &FileOverwriteOptions) -> MaybePromise<()> {
        unsupported("copy")
    }

    /// `typeof provider.open === 'function' && typeof provider.close === 'function' && typeof provider.read === 'function'
    /// && typeof provider.write === 'function'`.
    fn has_open_close_read_write(&self) -> bool {
        false
    }

    /// `open(uri, options)`: o descritor de arquivo; só é chamado quando [`Self::has_open_close_read_write`] é verdadeiro.
    fn open(&self, _uri: &URI, _options: &FileOpenOptions) -> MaybePromise<u32> {
        unsupported("open")
    }

    fn close(&self, _fd: u32) -> MaybePromise<()> {
        unsupported("close")
    }

    /// `read(fd, pos, data, offset, length)`: o número de bytes lidos.
    fn read(&self, _fd: u32, _pos: u64, _data: &VSBuffer, _offset: usize, _length: usize) -> MaybePromise<usize> {
        unsupported("read")
    }

    /// `write(fd, pos, data, offset, length)`: o número de bytes escritos.
    fn write(&self, _fd: u32, _pos: u64, _data: &VSBuffer, _offset: usize, _length: usize) -> MaybePromise<usize> {
        unsupported("write")
    }
}

/// A promessa rejeitada de uma operação opcional que o provedor não declarou.
fn unsupported<T: Clone + 'static>(operation: &str) -> MaybePromise<T> {
    MaybePromise::Promise(Promise::rejected(Error::new(format!("FileSystemProvider does not implement \"{operation}\""))))
}

/// `isReadonly?: boolean | IMarkdownString`.
#[derive(Clone, Debug)]
pub enum FileSystemReadonly {
    Flag(bool),
    Message(MarkdownString),
}

impl FileSystemReadonly {
    /// `!!isReadonly`: um objeto (a mensagem) é sempre verdadeiro.
    pub fn is_truthy(&self) -> bool {
        match self {
            FileSystemReadonly::Flag(flag) => *flag,
            FileSystemReadonly::Message(_) => true,
        }
    }
}

/// `{ isCaseSensitive?: boolean; isReadonly?: boolean | IMarkdownString }`.
#[derive(Clone, Debug, Default)]
pub struct FileSystemProviderOptions {
    pub is_case_sensitive: bool,
    pub is_readonly: Option<FileSystemReadonly>,
}

/// `{ recursive?: boolean; useTrash?: boolean }`: as opções de `vscode.FileSystem#delete`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ApiDeleteOptions {
    pub recursive: Option<bool>,
    pub use_trash: Option<bool>,
}

impl ApiDeleteOptions {
    /// `{ recursive: false, ...options }` e `{ recursive: false, useTrash: false, atomic: false, ...options }`.
    fn resolve(options: Option<ApiDeleteOptions>) -> FileDeleteOptions {
        let options = options.unwrap_or_default();
        FileDeleteOptions { recursive: options.recursive.unwrap_or(false), use_trash: options.use_trash.unwrap_or(false), atomic: None }
    }
}

/// `Promise.resolve(value)` para `T | Thenable<T>`.
pub fn into_promise<T: Clone + 'static>(value: MaybePromise<T>) -> Promise<T> {
    match value {
        MaybePromise::Value(value) => Promise::resolved(value),
        MaybePromise::Promise(promise) => promise,
    }
}

/// O `try { ... } catch (err) { return ExtHostConsumerFileSystem._handleError(err); }` de cada função de
/// `vscode.FileSystem`: o corpo roda como função `async` e a rejeição passa por `_handleError`.
fn guarded<T: Clone + 'static>(body: impl Future<Output = Result<T, Error>> + 'static) -> Promise<T> {
    async_fn(async move { body.await.map_err(ExtHostConsumerFileSystem::handle_error) })
}

/// O valor do mapa `_fileSystemProvider`: `{ impl, extUri, isReadonly }`.
#[derive(Clone)]
struct RegisteredProvider {
    provider: Rc<dyn ApiFileSystemProvider>,
    ext_uri: Rc<ExtUri>,
    is_readonly: bool,
}

/// `ExtHostConsumerFileSystem`.
pub struct ExtHostConsumerFileSystem {
    proxy: Rc<dyn MainThreadFileSystemShape>,
    file_system_provider: RefCell<HashMap<String, RegisteredProvider>>,
    write_queue: ResourceQueue,
    file_system_info: Rc<dyn IExtHostFileSystemInfo>,
    default_ext_uri: Rc<ExtUri>,
}

impl ExtHostConsumerFileSystem {
    pub fn new(main_context: &dyn IMainContext, file_system_info: Rc<dyn IExtHostFileSystemInfo>) -> Rc<Self> {
        Rc::new(Self {
            proxy: main_context.get_proxy(&MainContext::MAIN_THREAD_FILE_SYSTEM),
            file_system_provider: RefCell::new(HashMap::new()),
            write_queue: ResourceQueue::new(new_ext_uri()),
            file_system_info,
            default_ext_uri: Rc::new(new_ext_uri()),
        })
    }

    /// `value`: o `vscode.FileSystem`.
    pub fn value(self: &Rc<Self>) -> ApiFileSystem {
        ApiFileSystem { consumer: self.clone() }
    }

    /// O provedor registrado para o esquema do recurso, se houver (`this._fileSystemProvider.get(uri.scheme)`).
    fn provider_for(&self, uri: &URI) -> Option<RegisteredProvider> {
        self.file_system_provider.borrow().get(&uri.scheme().to_string_lossy()).cloned()
    }

    async fn mkdirp(provider: Rc<dyn ApiFileSystemProvider>, provider_ext_uri: Rc<ExtUri>, mut directory: URI) -> Result<(), Error> {
        let mut directories_to_create: Vec<String> = Vec::new();

        while !provider_ext_uri.is_equal(Some(&directory), Some(&provider_ext_uri.dirname(&directory)), false) {
            match into_promise(provider.stat(&directory)).await {
                Ok(stat) => {
                    if !stat.file_type.intersects(FileType::DIRECTORY) {
                        let shown = if directory.scheme() == schemas::FILE { fs_path(&directory) } else { directory.to_string(true).to_string_lossy() };
                        return Err(FileSystemError::file_exists(Some(MessageOrUri::Message(format!(
                            "Unable to create folder '{shown}' that already exists but is not a directory"
                        ))))
                        .error()
                        .clone());
                    }

                    break; // we have hit a directory that exists -> good
                }
                Err(error) => {
                    if to_file_system_provider_error_code(Some(&error)) != FileSystemProviderErrorCode::FileNotFound {
                        return Err(error);
                    }

                    // further go up and remember to create this directory
                    directories_to_create.push(provider_ext_uri.basename(&directory, None));
                    directory = provider_ext_uri.dirname(&directory);
                }
            }
        }

        for name in directories_to_create.iter().rev() {
            directory = provider_ext_uri.join_path(&directory, &[name.as_str()]);

            if let Err(error) = into_promise(provider.create_directory(&directory)).await
                && to_file_system_provider_error_code(Some(&error)) != FileSystemProviderErrorCode::FileExists
            {
                // For mkdirp() we tolerate that the mkdir() call fails
                // in case the folder already exists. This follows node.js
                // own implementation of fs.mkdir({ recursive: true }) and
                // reduces the chances of race conditions leading to errors
                // if multiple calls try to create the same folders
                // As such, we only throw an error here if it is other than
                // the fact that the file already exists.
                // (see also https://github.com/microsoft/vscode/issues/89834)
                return Err(error);
            }
        }

        Ok(())
    }

    /// `_handleError(err)`: o erro, no tipo que a API devolve para a extensão.
    pub fn handle_error(err: Error) -> Error {
        // desired error type, or file system provider error (ver as divergências do cabeçalho)
        let is_marked = err.name.ends_with(FILE_SYSTEM_ERROR_SUFFIX) || err.prop("code").is_some_and(|code| ERROR_CODES.iter().any(|known| known.as_str() == code));
        if is_marked {
            return err;
        }

        // no provider (unknown scheme) error
        if err.name == "ENOPRO" || err.message.contains("ENOPRO") {
            return FileSystemError::unavailable(Some(MessageOrUri::Message(err.message))).error().clone();
        }

        // file system error
        if let Some((_, factory)) = MAPPED_ERRORS.iter().find(|(code, _)| code.as_str() == err.name) {
            return factory(Some(MessageOrUri::Message(err.message))).error().clone();
        }
        Error::named(format!("{}{FILE_SYSTEM_ERROR_SUFFIX}", err.name), err.message).with_prop("code", err.name)
    }

    // ---

    /// `addFileSystemProvider(scheme, provider, options)`.
    pub fn add_file_system_provider(self: &Rc<Self>, scheme: &str, provider: Rc<dyn ApiFileSystemProvider>, options: Option<&FileSystemProviderOptions>) -> DisposableRef {
        let ext_uri = match options {
            Some(options) if options.is_case_sensitive => Rc::new(new_ext_uri()),
            _ => Rc::new(ExtUri::ignore_path_case(host_environment())),
        };
        let is_readonly = options.and_then(|options| options.is_readonly.as_ref()).is_some_and(FileSystemReadonly::is_truthy);
        self.file_system_provider.borrow_mut().insert(scheme.to_owned(), RegisteredProvider { provider, ext_uri, is_readonly });

        let (this, scheme) = (self.clone(), scheme.to_owned());
        to_disposable(move || {
            this.file_system_provider.borrow_mut().remove(&scheme);
        })
    }

    pub fn get_file_system_provider_ext_uri(&self, scheme: &str) -> Rc<ExtUri> {
        self.file_system_provider.borrow().get(scheme).map_or_else(|| self.default_ext_uri.clone(), |registered| registered.ext_uri.clone())
    }
}

/// `vscode.FileSystem`.
#[derive(Clone)]
pub struct ApiFileSystem {
    consumer: Rc<ExtHostConsumerFileSystem>,
}

impl ApiFileSystem {
    pub fn stat(&self, uri: &URI) -> Promise<ApiFileStat> {
        let (this, uri) = (self.consumer.clone(), uri.clone());
        guarded(async move {
            let stat = match this.provider_for(&uri) {
                Some(registered) => {
                    // use shortcut
                    this.proxy.rpc_ensure_activation(&uri.scheme().to_string_lossy()).await?;
                    into_promise(registered.provider.stat(&uri)).await?.to_stat()
                }
                None => this.proxy.rpc_stat(&uri).await?,
            };

            Ok(ApiFileStat::from_stat(&stat))
        })
    }

    pub fn read_directory(&self, uri: &URI) -> Promise<Vec<(String, FileType)>> {
        let (this, uri) = (self.consumer.clone(), uri.clone());
        guarded(async move {
            match this.provider_for(&uri) {
                Some(registered) => {
                    // use shortcut
                    this.proxy.rpc_ensure_activation(&uri.scheme().to_string_lossy()).await?;
                    into_promise(registered.provider.read_directory(&uri)).await // safe-copy
                }
                None => this.proxy.rpc_readdir(&uri).await,
            }
        })
    }

    pub fn create_directory(&self, uri: &URI) -> Promise<()> {
        let (this, uri) = (self.consumer.clone(), uri.clone());
        guarded(async move {
            match this.provider_for(&uri) {
                Some(registered) if !registered.is_readonly => {
                    // use shortcut
                    this.proxy.rpc_ensure_activation(&uri.scheme().to_string_lossy()).await?;
                    ExtHostConsumerFileSystem::mkdirp(registered.provider, registered.ext_uri, uri).await
                }
                _ => this.proxy.rpc_mkdir(&uri).await,
            }
        })
    }

    pub fn read_file(&self, uri: &URI) -> Promise<Vec<u8>> {
        let (this, uri) = (self.consumer.clone(), uri.clone());
        guarded(async move {
            match this.provider_for(&uri) {
                Some(registered) => {
                    // use shortcut
                    this.proxy.rpc_ensure_activation(&uri.scheme().to_string_lossy()).await?;
                    into_promise(registered.provider.read_file(&uri)).await // safe-copy
                }
                None => Ok(this.proxy.rpc_read_file(&uri).await?.to_vec()),
            }
        })
    }

    pub fn write_file(&self, uri: &URI, content: &[u8]) -> Promise<()> {
        let (this, uri, content) = (self.consumer.clone(), uri.clone(), content.to_vec());
        guarded(async move {
            match this.provider_for(&uri) {
                Some(registered) if !registered.is_readonly => {
                    // use shortcut
                    this.proxy.rpc_ensure_activation(&uri.scheme().to_string_lossy()).await?;
                    ExtHostConsumerFileSystem::mkdirp(registered.provider.clone(), registered.ext_uri.clone(), registered.ext_uri.dirname(&uri)).await?;
                    let target = uri.clone();
                    this.write_queue
                        .queue_for(&uri, move || into_promise(registered.provider.write_file(&target, &content, &FileWriteOptions { create: true, overwrite: true, ..FileWriteOptions::default() })))
                        .await
                }
                _ => this.proxy.rpc_write_file(&uri, VSBuffer::wrap(content)).await,
            }
        })
    }

    pub fn delete(&self, uri: &URI, options: Option<ApiDeleteOptions>) -> Promise<()> {
        let (this, uri) = (self.consumer.clone(), uri.clone());
        guarded(async move {
            let use_trash = options.and_then(|options| options.use_trash).unwrap_or(false);
            match this.provider_for(&uri) {
                Some(registered) if !registered.is_readonly && !use_trash /* no shortcut: use trash */ => {
                    // use shortcut
                    this.proxy.rpc_ensure_activation(&uri.scheme().to_string_lossy()).await?;
                    into_promise(registered.provider.delete(&uri, &ApiDeleteOptions::resolve(options))).await
                }
                _ => this.proxy.rpc_delete(&uri, ApiDeleteOptions::resolve(options)).await,
            }
        })
    }

    pub fn rename(&self, old_uri: &URI, new_uri: &URI, options: Option<FileOverwriteOptions>) -> Promise<()> {
        let (this, old_uri, new_uri) = (self.consumer.clone(), old_uri.clone(), new_uri.clone());
        guarded(async move {
            // no shortcut: potentially involves different schemes, does mkdirp
            this.proxy.rpc_rename(&old_uri, &new_uri, options.unwrap_or_default()).await
        })
    }

    pub fn copy(&self, source: &URI, destination: &URI, options: Option<FileOverwriteOptions>) -> Promise<()> {
        let (this, source, destination) = (self.consumer.clone(), source.clone(), destination.clone());
        guarded(async move {
            // no shortcut: potentially involves different schemes, does mkdirp
            this.proxy.rpc_copy(&source, &destination, options.unwrap_or_default()).await
        })
    }

    pub fn is_writable_file_system(&self, scheme: &str) -> Option<bool> {
        let capabilities = self.consumer.file_system_info.get_capabilities(scheme);
        capabilities.map(|capabilities: FileSystemProviderCapabilities| !capabilities.intersects(FileSystemProviderCapabilities::READONLY))
    }
}
