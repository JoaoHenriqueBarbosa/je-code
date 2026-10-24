//! Porte de `src/vs/workbench/contrib/notebook/common/notebookEditorModelResolverService.ts`: o contrato do serviço que
//! resolve os modelos de editor de notebook, com contagem de referências (`INotebookEditorModelResolverService`). A
//! implementação (`notebookEditorModelResolverServiceImpl.ts`) fica para a fatia dos serviços.
//!
//! Divergências:
//! - `IReference<IResolvedNotebookEditorModel>` é o [`Reference`] de `je_base::lifecycle` ([`NotebookModelReference`]).
//! - `INotebookConflictEvent extends IWaitUntil` é o [`WaitUntilEvent`] com os dados [`INotebookConflictEventData`].
//! - As duas sobrecargas de `resolve` (`URI` e `IUntitledNotebookResource`) são [`resolve`](INotebookEditorModelResolverService::resolve)
//!   e [`resolve_untitled`](INotebookEditorModelResolverService::resolve_untitled).
//! - `NotebookTextModel` é o `Rc<NotebookTextModel>` concreto.

use std::rc::Rc;

use je_base::event::{Event, WaitUntilEvent};
use je_base::lifecycle::Reference;
use je_base::promise::Promise;
use je_base::uri::URI;
use je_instantiation::service_id;

use super::model::notebook_text_model::NotebookTextModel;
use super::notebook_common::{IResolvedNotebookEditorModel, NotebookEditorModelCreationOptions};

/// `IReference<IResolvedNotebookEditorModel>`.
pub type NotebookModelReference = Reference<Rc<dyn IResolvedNotebookEditorModel>>;

/// Os dados de `INotebookConflictEvent` (o evento sem o `waitUntil` e o `token`).
#[derive(Clone, Debug)]
pub struct INotebookConflictEventData {
    pub resource: URI,
    pub view_type: String,
}

/// `INotebookConflictEvent`.
pub type INotebookConflictEvent = WaitUntilEvent<INotebookConflictEventData>;

/// `IUntitledNotebookResource`.
///
/// Depending on the value of `untitledResource` will resolve a untitled notebook that:
/// - gets a unique name if `None` (e.g. `Untitled-1`)
/// - uses the resource directly if the scheme is `untitled:`
/// - converts any other resource scheme to `untitled:` and will assume an associated file path
///
/// Untitled notebook editors with associated path behave slightly different from other untitled editors:
/// - they are dirty right when opening
/// - they will not ask for a file path when saving but use the associated path
#[derive(Clone, Debug, Default)]
pub struct IUntitledNotebookResource {
    pub untitled_resource: Option<URI>,
}

/// `INotebookEditorModelResolverService`.
pub trait INotebookEditorModelResolverService {
    fn on_did_save_notebook(&self) -> Event<URI>;
    fn on_did_change_dirty(&self) -> Event<Rc<dyn IResolvedNotebookEditorModel>>;
    fn on_will_fail_with_conflict(&self) -> Event<INotebookConflictEvent>;

    fn is_dirty(&self, resource: &URI) -> bool;

    fn create_untitled_notebook_text_model(&self, view_type: &str) -> Promise<Rc<NotebookTextModel>>;

    fn resolve(&self, resource: &URI, view_type: Option<&str>, creation_options: Option<NotebookEditorModelCreationOptions>) -> Promise<NotebookModelReference>;

    fn resolve_untitled(&self, resource: &IUntitledNotebookResource, view_type: &str, creation_options: Option<NotebookEditorModelCreationOptions>) -> Promise<NotebookModelReference>;
}

service_id!(pub I_NOTEBOOK_EDITOR_MODEL_RESOLVER_SERVICE: dyn INotebookEditorModelResolverService = "INotebookModelResolverService");
