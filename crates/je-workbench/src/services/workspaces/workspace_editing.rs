//! Porte de `src/vs/workbench/services/workspaces/common/workspaceEditing.ts`: o contrato `IWorkspaceEditingService`,
//! que adiciona, remove e atualiza as pastas do workspace e entra em outro workspace.
//!
//! Divergências:
//! - `_serviceBrand` é o identificador [`I_WORKSPACE_EDITING_SERVICE`].
//! - `IDidEnterWorkspaceEvent.join(promise)` é o método [`DidEnterWorkspaceEvent::join`], que guarda as promessas como o
//!   [`WorkspaceFoldersWillChangeEvent`](je_platform::workspace::WorkspaceFoldersWillChangeEvent) da plataforma.
//! - Os parâmetros opcionais (`deleteCount?`, `foldersToAdd?`, `donotNotifyError?`) são `Option`; `number` é `f64`, como
//!   no resto do protocolo com o extension host.

use std::cell::RefCell;
use std::rc::Rc;

use je_base::event::Event;
use je_base::promise::Promise;
use je_base::uri::URI;
use je_instantiation::service_id;
use je_platform::workspace::{AnyWorkspaceIdentifier, WorkspaceIdentifier};
use je_platform::workspaces::workspaces::WorkspaceFolderCreationData;

/// `IDidEnterWorkspaceEvent`.
#[derive(Clone)]
pub struct DidEnterWorkspaceEvent {
    pub old_workspace: AnyWorkspaceIdentifier,
    pub new_workspace: AnyWorkspaceIdentifier,
    joiners: Rc<RefCell<Vec<Promise<()>>>>,
}

impl DidEnterWorkspaceEvent {
    pub fn new(old_workspace: AnyWorkspaceIdentifier, new_workspace: AnyWorkspaceIdentifier) -> Self {
        Self { old_workspace, new_workspace, joiners: Rc::new(RefCell::new(Vec::new())) }
    }

    /// `join(promise)`.
    pub fn join(&self, promise: Promise<()>) {
        self.joiners.borrow_mut().push(promise);
    }

    /// As promessas entregues a `join` até agora.
    pub fn joined(&self) -> Vec<Promise<()>> {
        self.joiners.borrow().clone()
    }
}

/// `IWorkspaceEditingService`.
pub trait IWorkspaceEditingService {
    fn on_did_enter_workspace(&self) -> Event<DidEnterWorkspaceEvent>;

    fn add_folders(&self, folders: Vec<WorkspaceFolderCreationData>, donot_notify_error: Option<bool>) -> Promise<()>;

    fn remove_folders(&self, folders: Vec<URI>, donot_notify_error: Option<bool>) -> Promise<()>;

    fn update_folders(&self, index: f64, delete_count: Option<f64>, folders_to_add: Option<Vec<WorkspaceFolderCreationData>>, donot_notify_error: Option<bool>) -> Promise<()>;

    fn enter_workspace(&self, path: &URI) -> Promise<()>;

    fn create_and_enter_workspace(&self, folders: Vec<WorkspaceFolderCreationData>, path: Option<URI>) -> Promise<()>;

    fn save_and_enter_workspace(&self, path: &URI) -> Promise<()>;

    fn copy_workspace_settings(&self, to_workspace: &WorkspaceIdentifier) -> Promise<()>;

    fn pick_new_workspace_path(&self) -> Promise<Option<URI>>;
}

service_id!(pub I_WORKSPACE_EDITING_SERVICE: dyn IWorkspaceEditingService = "workspaceEditingService");
