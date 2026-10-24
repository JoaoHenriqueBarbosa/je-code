//! O contrato `IFileService` (de `src/vs/platform/files/common/files.ts`) que os serviços do workbench recebem por
//! injeção.
//!
//! O porte do serviço de arquivos é a struct concreta [`FileService`], sem trait. Este módulo define o `IFileService`
//! reduzido ao que os serviços portados até aqui leem dele (`getProvider`, `resolve` com `resolveMetadata`,
//! `onDidFilesChange` e `exists`), com a assinatura de cada método igual à do original, e cresce junto com os
//! consumidores. O `FileService` o implementa por [`Rc<FileService>`], porque os métodos dele recebem `self: &Rc<Self>`.
//!
//! Divergências:
//! - `resolve(resource, { resolveMetadata: true })` é [`IFileService::resolve_with_metadata`], que devolve o
//!   [`FileStatWithMetadata`] (o tipo que o `resolveMetadata: true` garante).

use std::rc::Rc;

use je_base::event::Event;
use je_base::promise::Promise;
use je_base::uri::URI;
use je_instantiation::service_id;

use super::file_service::FileService;
use super::files::{FileChangesEvent, FileStatWithMetadata, FileSystemProviderRef, ResolveFileOptions};

/// `IFileService`.
pub trait IFileService {
    /// An event that is fired when file system changes are provided.
    fn on_did_files_change(&self) -> Event<Rc<FileChangesEvent>>;

    /// Returns the provider registered for the given scheme, if any.
    fn get_provider(&self, scheme: &str) -> Option<FileSystemProviderRef>;

    /// `resolve(resource, { ...options, resolveMetadata: true })`: resolves the properties of a file or folder with
    /// the metadata filled in.
    fn resolve_with_metadata(&self, resource: &URI, options: Option<ResolveFileOptions>) -> Promise<FileStatWithMetadata>;

    /// Checks if a resource exists.
    fn exists(&self, resource: &URI) -> Promise<bool>;
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
}
