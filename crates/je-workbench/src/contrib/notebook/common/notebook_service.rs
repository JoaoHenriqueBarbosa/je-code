//! Porte parcial de `src/vs/workbench/contrib/notebook/common/notebookService.ts`: o contrato do serviço de notebooks
//! (`INotebookService`), o serializador (`INotebookSerializer`) e a `SimpleNotebookProviderInfo`. A implementação do serviço
//! (`notebookServiceImpl.ts`) fica para a fatia dos serviços.
//!
//! Divergências:
//! - `INotebookService` só tem os membros que o porte usa (`MainThreadNotebooks`, `MainThreadNotebookDocumentsAndEditors` e os
//!   comandos `_executeDataToNotebook` e `_executeNotebookToData`): `canResolve`, os eventos de tipo de visão e de documento,
//!   `registerNotebookSerializer`, `withNotebookDataProvider`, `tryGetDataProviderSync`, `getViewTypeProvider`,
//!   `getRendererInfo`, `getRenderers`, `getStaticPreloads`, `createNotebookTextModel`, `getNotebookTextModel`,
//!   `getNotebookTextModels`, `listNotebookDocuments`, `registerContributedNotebookType`, `hasSupportedNotebooks`,
//!   `getNotebookProviderResourceRoots` e `clearEditorCache`. Ficam de fora `getOutputMimeTypeInfo`, `updateMimePreferredRenderer`,
//!   `saveMimeDisplayOrder`, `createNotebookTextDocumentSnapshot`, `restoreNotebookTextModelFromSnapshot`,
//!   `getContributedNotebookType(s)` (dependem do `NotebookProviderInfo` de `notebookProvider.ts`, que não tem porte),
//!   `setToCopy` e `getToCopy`.
//! - `INotebookSerializer.searchInNotebooks` não existe: o `$searchInNotebooks` não está em `ExtHostNotebookShape` (depende do
//!   `ExtHostSearch`), então o serializador do porte não busca em notebooks fechados. `INotebookContentProvider` e
//!   `INotebookRawData` (sem uso no porte) não são portados.
//! - `Promise<IFileStatWithMetadata>` de `save` é a [`FileStatWithMetadata`]; o `throw` do `FileOperationError` é a promessa
//!   rejeitada com [`FileOperationError::into_error`].
//! - `NotebookTextModel` é o `Rc<NotebookTextModel>` concreto. `SimpleNotebookProviderInfo` é devolvido como `Rc`.

use std::rc::Rc;

use je_base::buffer::{VSBuffer, VSBufferReadableStream};
use je_base::cancellation::CancellationToken;
use je_base::event::Event;
use je_base::lifecycle::DisposableRef;
use je_base::promise::Promise;
use je_base::uri::URI;
use je_instantiation::service_id;
use je_platform::files::files::{FileStatWithMetadata, WriteFileOptions};

use super::model::notebook_text_model::NotebookTextModel;
use super::notebook_common::{
    INotebookContributionData, INotebookRendererInfo, INotebookStaticPreloadInfo, NotebookData, NotebookExtensionDescription, TransientOptions,
};

/// `INotebookSerializer`.
pub trait INotebookSerializer {
    fn options(&self) -> TransientOptions;
    fn data_to_notebook(&self, data: VSBuffer) -> Promise<NotebookData>;
    fn notebook_to_data(&self, data: NotebookData) -> Promise<VSBuffer>;
    fn save(&self, uri: &URI, version_id: i64, options: WriteFileOptions, token: CancellationToken) -> Promise<FileStatWithMetadata>;
}

/// `SimpleNotebookProviderInfo`.
pub struct SimpleNotebookProviderInfo {
    pub view_type: String,
    pub serializer: Rc<dyn INotebookSerializer>,
    pub extension_data: NotebookExtensionDescription,
}

impl SimpleNotebookProviderInfo {
    pub fn new(view_type: &str, serializer: Rc<dyn INotebookSerializer>, extension_data: NotebookExtensionDescription) -> Self {
        Self { view_type: view_type.to_string(), serializer, extension_data }
    }
}

/// `INotebookService` (ver as divergências do módulo).
pub trait INotebookService {
    fn can_resolve(&self, view_type: &str) -> Promise<bool>;

    fn on_add_view_type(&self) -> Event<String>;
    fn on_will_remove_view_type(&self) -> Event<String>;
    fn on_did_change_output_renderers(&self) -> Event<()>;
    fn on_will_add_notebook_document(&self) -> Event<Rc<NotebookTextModel>>;
    fn on_did_add_notebook_document(&self) -> Event<Rc<NotebookTextModel>>;
    fn on_will_remove_notebook_document(&self) -> Event<Rc<NotebookTextModel>>;
    fn on_did_remove_notebook_document(&self) -> Event<Rc<NotebookTextModel>>;

    fn register_notebook_serializer(&self, view_type: &str, extension_data: NotebookExtensionDescription, serializer: Rc<dyn INotebookSerializer>) -> DisposableRef;
    fn with_notebook_data_provider(&self, view_type: &str) -> Promise<Rc<SimpleNotebookProviderInfo>>;
    fn try_get_data_provider_sync(&self, view_type: &str) -> Option<Rc<SimpleNotebookProviderInfo>>;

    fn get_view_type_provider(&self, view_type: &str) -> Option<String>;
    fn get_renderer_info(&self, id: &str) -> Option<Rc<dyn INotebookRendererInfo>>;
    fn get_renderers(&self) -> Vec<Rc<dyn INotebookRendererInfo>>;
    fn get_static_preloads(&self, view_type: &str) -> Vec<INotebookStaticPreloadInfo>;

    fn create_notebook_text_model(&self, view_type: &str, uri: &URI, stream: Option<VSBufferReadableStream>) -> Promise<Rc<NotebookTextModel>>;
    fn get_notebook_text_model(&self, uri: &URI) -> Option<Rc<NotebookTextModel>>;
    fn get_notebook_text_models(&self) -> Vec<Rc<NotebookTextModel>>;
    fn list_notebook_documents(&self) -> Vec<Rc<NotebookTextModel>>;

    /// Register a notebook type that we will handle. The notebook editor will be registered for notebook types contributed by
    /// extensions.
    fn register_contributed_notebook_type(&self, view_type: &str, data: INotebookContributionData) -> DisposableRef;
    fn has_supported_notebooks(&self, resource: &URI) -> bool;
    fn get_notebook_provider_resource_roots(&self) -> Vec<URI>;
    fn clear_editor_cache(&self);
}

service_id!(pub I_NOTEBOOK_SERVICE: dyn INotebookService = "notebookService");
