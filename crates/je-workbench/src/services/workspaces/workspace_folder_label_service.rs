//! Porte de `src/vs/workbench/services/workspaces/common/workspaceFolderLabelService.ts`: o contrato
//! `IWorkspaceFolderLabelService` (o rótulo que uma pasta do workspace mostra, por exemplo nas migalhas) e a
//! implementação padrão, que não rotula nada.
//!
//! Notas de tradução:
//! - `getWorkspaceFolderLabel(folder, verbose?)` devolve `string | undefined`: é `Option<String>`, e o `verbose?` é
//!   `Option<bool>`. A implementação padrão devolve sempre `None` (o `undefined` do original).
//! - `registerSingleton(IWorkspaceFolderLabelService, WorkspaceFolderLabelService, InstantiationType.Delayed)` é
//!   [`register`]; o proxy da instanciação atrasada é o [`WorkspaceFolderLabelServiceProxy`].

use std::rc::Rc;

use je_instantiation::{Injectable, InstantiationType, delayed_proxy, register_singleton, service_id, service_impl};
use je_platform::workspace::WorkspaceFolder;

/// `IWorkspaceFolderLabelService`.
pub trait IWorkspaceFolderLabelService {
    fn get_workspace_folder_label(&self, folder: &WorkspaceFolder, verbose: Option<bool>) -> Option<String>;
}

delayed_proxy!(pub WorkspaceFolderLabelServiceProxy for dyn IWorkspaceFolderLabelService {
    methods {
        fn get_workspace_folder_label(&self, folder: &WorkspaceFolder, verbose: Option<bool>) -> Option<String>;
    }
    events {}
});

service_id!(pub I_WORKSPACE_FOLDER_LABEL_SERVICE: dyn IWorkspaceFolderLabelService = "workspaceFolderLabelService", proxy WorkspaceFolderLabelServiceProxy);

/// `WorkspaceFolderLabelService`.
pub struct WorkspaceFolderLabelService;

impl Injectable for WorkspaceFolderLabelService {
    type Args = ();
    type Deps = ();

    fn create(_: (), _: ()) -> Rc<Self> {
        Rc::new(Self)
    }
}

service_impl!(WorkspaceFolderLabelService => dyn IWorkspaceFolderLabelService);

/// `registerSingleton(IWorkspaceFolderLabelService, WorkspaceFolderLabelService, InstantiationType.Delayed)`.
pub fn register() {
    register_singleton::<WorkspaceFolderLabelService>(I_WORKSPACE_FOLDER_LABEL_SERVICE, InstantiationType::Delayed);
}

impl IWorkspaceFolderLabelService for WorkspaceFolderLabelService {
    fn get_workspace_folder_label(&self, _folder: &WorkspaceFolder, _verbose: Option<bool>) -> Option<String> {
        None
    }
}
