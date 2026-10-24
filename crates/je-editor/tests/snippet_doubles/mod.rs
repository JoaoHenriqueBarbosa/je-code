//! Dublês dos testes de snippet: o `ILabelService` e o `IWorkspaceContextService` que o original cria com
//! `mock<>()` (e, no `snippetVariables.test.ts`, com uma classe anônima cujos métodos lançam).
//!
//! Um método que o `mock<>()` deixa indefinido (ou que `_throw` lança) é um pânico aqui: o teste que o chama sem
//! querer reprova.
#![allow(dead_code)]

use std::cell::RefCell;
use std::rc::Rc;

use je_base::event::Event;
use je_base::promise::Promise;
use je_base::uri::URI;
use je_editor::snippet::snippet_variables::{ILabelService, UriLabelOptions};
use je_platform::workspace::{
    CurrentWorkspaceQuery, IWorkspaceContextService, WorkbenchState, Workspace, WorkspaceFolder,
    WorkspaceFoldersChangeEvent, WorkspaceFoldersWillChangeEvent,
};

/// O `ILabelService` cujo `getUriLabel` é o fecho dado (o `override getUriLabel` da classe anônima).
pub struct ClosureLabelService {
    get_uri_label: Box<dyn Fn(&URI, &UriLabelOptions) -> String>,
}

impl ClosureLabelService {
    pub fn new(get_uri_label: impl Fn(&URI, &UriLabelOptions) -> String + 'static) -> Rc<Self> {
        Rc::new(Self { get_uri_label: Box::new(get_uri_label) })
    }

    /// O `new class extends mock<ILabelService>() { }` sem método nenhum: chamar `getUriLabel` reprova.
    pub fn without_methods() -> Rc<Self> {
        Self::new(|_, _| panic!("ILabelService.getUriLabel não está definido neste mock"))
    }
}

impl ILabelService for ClosureLabelService {
    fn get_uri_label(&self, resource: &URI, options: &UriLabelOptions) -> String {
        (self.get_uri_label)(resource, options)
    }
}

/// O `IWorkspaceContextService` que só define `getWorkspace()` (o resto lança, como o `_throw` do original).
pub struct WorkspaceOnlyService {
    workspace: RefCell<Rc<Workspace>>,
}

impl WorkspaceOnlyService {
    pub fn new(workspace: Rc<Workspace>) -> Rc<Self> {
        Rc::new(Self { workspace: RefCell::new(workspace) })
    }

    /// Troca o workspace devolvido (o `workspace = new Workspace(...)` do teste).
    pub fn set_workspace(&self, workspace: Rc<Workspace>) {
        *self.workspace.borrow_mut() = workspace;
    }
}

impl IWorkspaceContextService for WorkspaceOnlyService {
    fn on_did_change_workbench_state(&self) -> Event<WorkbenchState> {
        panic!("onDidChangeWorkbenchState não está definido neste mock")
    }

    fn on_did_change_workspace_name(&self) -> Event<()> {
        panic!("onDidChangeWorkspaceName não está definido neste mock")
    }

    fn on_will_change_workspace_folders(&self) -> Event<WorkspaceFoldersWillChangeEvent> {
        panic!("onWillChangeWorkspaceFolders não está definido neste mock")
    }

    fn on_did_change_workspace_folders(&self) -> Event<WorkspaceFoldersChangeEvent> {
        panic!("onDidChangeWorkspaceFolders não está definido neste mock")
    }

    fn get_complete_workspace(&self) -> Promise<Rc<Workspace>> {
        panic!("getCompleteWorkspace não está definido neste mock")
    }

    fn get_workspace(&self) -> Rc<Workspace> {
        self.workspace.borrow().clone()
    }

    fn get_workbench_state(&self) -> WorkbenchState {
        panic!("getWorkbenchState não está definido neste mock")
    }

    fn get_workspace_folder(&self, _resource: &URI) -> Option<WorkspaceFolder> {
        panic!("getWorkspaceFolder não está definido neste mock")
    }

    fn is_current_workspace(&self, _workspace_id_or_folder: &CurrentWorkspaceQuery) -> bool {
        panic!("isCurrentWorkspace não está definido neste mock")
    }

    fn is_inside_workspace(&self, _resource: &URI) -> bool {
        panic!("isInsideWorkspace não está definido neste mock")
    }

    fn has_workspace_data(&self) -> bool {
        panic!("hasWorkspaceData não está definido neste mock")
    }
}
