//! O contrato `IFileService` (de `src/vs/platform/files/common/files.ts`) que os serviços do workbench recebem por
//! injeção.
//!
//! O porte do serviço de arquivos é a struct concreta [`FileService`], sem trait. Este módulo define o `IFileService`
//! reduzido ao que os serviços portados até aqui leem dele (`getProvider`, `resolve` com `resolveMetadata`,
//! `onDidFilesChange`, `exists`, `hasProvider`, `hasCapability`, `readFile`, `resolve`, `createFolder`, `watch` e os eventos de registro e de capacidades dos
//! providers), com a assinatura de cada método igual à do original, e cresce junto com os
//! consumidores. O `FileService` o implementa por [`Rc<FileService>`], porque os métodos dele recebem `self: &Rc<Self>`.
//!
//! Divergências:
//! - `resolve(resource, { resolveMetadata: true })` é [`IFileService::resolve_with_metadata`], que devolve o
//!   [`FileStatWithMetadata`] (o tipo que o `resolveMetadata: true` garante).

use std::rc::Rc;

use je_base::cancellation::CancellationToken;
use je_base::event::Event;
use je_base::lifecycle::DisposableRef;
use je_base::promise::Promise;
use je_base::uri::URI;
use je_instantiation::service_id;

use super::file_service::{FileContentSource, FileService};
use super::files::{
    FileChangesEvent, FileContent, FileStat, FileStatWithMetadata, FileSystemProviderCapabilities, FileSystemProviderCapabilitiesChangeEvent,
    FileStreamContent, FileSystemProviderRef, FileSystemProviderRegistrationEvent, PartialFileDeleteOptions, ReadFileOptions, ResolveFileOptions,
    WatchOptions, WriteFileOptions,
};

/// `IFileService`.
pub trait IFileService {
    /// An event that is fired when file system changes are provided.
    fn on_did_files_change(&self) -> Event<Rc<FileChangesEvent>>;

    /// Returns the provider registered for the given scheme, if any.
    fn get_provider(&self, scheme: &str) -> Option<FileSystemProviderRef>;

    /// `resolve(resource, { ...options, resolveMetadata: true })`: resolves the properties of a file or folder with
    /// the metadata filled in.
    fn resolve_with_metadata(&self, resource: &URI, options: Option<ResolveFileOptions>) -> Promise<FileStatWithMetadata>;

    /// `stat(resource)`: `resolve(resource, { resolveMetadata: true })` sem os filhos.
    fn stat(&self, resource: &URI) -> Promise<FileStatWithMetadata> {
        self.resolve_with_metadata(resource, None)
    }

    /// Checks if a resource exists.
    fn exists(&self, resource: &URI) -> Promise<bool>;

    /// An event that is fired when a file system provider is added or removed.
    fn on_did_change_file_system_provider_registrations(&self) -> Event<FileSystemProviderRegistrationEvent>;

    /// An event that is fired when the capabilities of a file system provider change.
    fn on_did_change_file_system_provider_capabilities(&self) -> Event<FileSystemProviderCapabilitiesChangeEvent>;

    /// Checks if this file service has a registered provider for the provided resource.
    ///
    /// Note: this does NOT account for contributed providers from extensions that have not been activated yet.
    fn has_provider(&self, resource: &URI) -> bool;

    /// Checks if the provider for the provided resource has the provided file system capability.
    fn has_capability(&self, resource: &URI, capability: FileSystemProviderCapabilities) -> bool;

    /// `readFile(resource, options?, token?)`: reads the file at the given resource. A missing file rejects with a
    /// [`super::files::FileOperationError`] whose result is `FileNotFound`.
    fn read_file(&self, resource: &URI, options: Option<ReadFileOptions>, token: Option<CancellationToken>) -> Promise<FileContent>;

    /// `resolve(resource, options?)`: resolves the properties of a file or folder (sem os metadados, a menos que
    /// `options.resolve_metadata` seja pedido; o caso garantido é [`IFileService::resolve_with_metadata`]).
    fn resolve(&self, resource: &URI, options: Option<ResolveFileOptions>) -> Promise<FileStat>;

    /// Creates a new folder with the given path. The returned promise will have the stat model object as a result.
    ///
    /// Emits a `FileOperation.CREATE` file operation event when successful.
    fn create_folder(&self, resource: &URI) -> Promise<FileStatWithMetadata>;

    /// Allows to start a watcher that reports file/folder change events on the provided resource.
    ///
    /// Note: watching a folder does not report events recursively. Dispose the returned value to stop watching.
    fn watch(&self, resource: &URI, options: Option<WatchOptions>) -> DisposableRef;

    /// `readFileStream(resource, options?, token?)`: read the contents of the provided resource buffered as stream.
    fn read_file_stream(&self, resource: &URI, options: Option<ReadFileOptions>, token: Option<CancellationToken>) -> Promise<FileStreamContent>;

    /// `writeFile(resource, bufferOrReadableOrStream, options?)`: updates the content replacing its previous value.
    ///
    /// Emits a `FileOperation.WRITE` file operation event when successful.
    fn write_file(&self, resource: &URI, content: FileContentSource, options: Option<WriteFileOptions>) -> Promise<FileStatWithMetadata>;

    /// `del(resource, options?)`: deletes the provided file. The optional useTrash parameter allows to move the file to
    /// trash. The optional recursive parameter allows to delete non-empty folders recursively.
    ///
    /// Emits a `FileOperation.DELETE` file operation event when successful.
    fn del(&self, resource: &URI, options: Option<PartialFileDeleteOptions>) -> Promise<()>;
}

service_id!(pub I_FILE_SERVICE: dyn IFileService = "fileService");

impl IFileService for Rc<FileService> {
    fn on_did_files_change(&self) -> Event<Rc<FileChangesEvent>> {
        FileService::on_did_files_change(self)
    }

    fn get_provider(&self, scheme: &str) -> Option<FileSystemProviderRef> {
        FileService::get_provider(self, scheme)
    }

    fn resolve_with_metadata(&self, resource: &URI, options: Option<ResolveFileOptions>) -> Promise<FileStatWithMetadata> {
        FileService::resolve_with_metadata(self, resource, options)
    }

    fn exists(&self, resource: &URI) -> Promise<bool> {
        FileService::exists(self, resource)
    }

    fn on_did_change_file_system_provider_registrations(&self) -> Event<FileSystemProviderRegistrationEvent> {
        FileService::on_did_change_file_system_provider_registrations(self)
    }

    fn on_did_change_file_system_provider_capabilities(&self) -> Event<FileSystemProviderCapabilitiesChangeEvent> {
        FileService::on_did_change_file_system_provider_capabilities(self)
    }

    fn has_provider(&self, resource: &URI) -> bool {
        FileService::has_provider(self, resource)
    }

    fn has_capability(&self, resource: &URI, capability: FileSystemProviderCapabilities) -> bool {
        FileService::has_capability(self, resource, capability)
    }

    fn read_file(&self, resource: &URI, options: Option<ReadFileOptions>, token: Option<CancellationToken>) -> Promise<FileContent> {
        FileService::read_file(self, resource, options, token)
    }

    fn resolve(&self, resource: &URI, options: Option<ResolveFileOptions>) -> Promise<FileStat> {
        FileService::resolve(self, resource, options)
    }

    fn create_folder(&self, resource: &URI) -> Promise<FileStatWithMetadata> {
        FileService::create_folder(self, resource)
    }

    fn watch(&self, resource: &URI, options: Option<WatchOptions>) -> DisposableRef {
        FileService::watch(self, resource, options)
    }

    fn read_file_stream(&self, resource: &URI, options: Option<ReadFileOptions>, token: Option<CancellationToken>) -> Promise<FileStreamContent> {
        FileService::read_file_stream(self, resource, options, token)
    }

    fn write_file(&self, resource: &URI, content: FileContentSource, options: Option<WriteFileOptions>) -> Promise<FileStatWithMetadata> {
        FileService::write_file(self, resource, content, options)
    }

    fn del(&self, resource: &URI, options: Option<PartialFileDeleteOptions>) -> Promise<()> {
        FileService::del(self, resource, options)
    }
}
