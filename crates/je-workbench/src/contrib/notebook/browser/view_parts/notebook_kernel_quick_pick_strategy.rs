//! Porte parcial de `src/vs/workbench/contrib/notebook/browser/viewParts/notebookKernelQuickPickStrategy.ts`: só o
//! `KernelPickerMRUStrategy.resolveKernel`, que o `NotebookExecutionService` usa para obter o kernel da execução. O seletor de
//! kernel em si (o quick pick) é widget e fica para a Fase 6.
//!
//! Divergências:
//! - `SELECT_KERNEL_ID` é de `controller/coreActions.ts`; vive aqui porque este é o único consumidor portado.
//! - `static async resolveKernel(...)` é uma função que devolve [`Promise`].

use std::rc::Rc;

use je_base::errors::Error;
use je_base::promise::Promise;
use je_platform::commands::commands::ICommandService;

use crate::contrib::notebook::common::notebook_kernel_service::{INotebookKernel, INotebookKernelHistoryService, INotebookTextModelLike};

/// `SELECT_KERNEL_ID`.
pub const SELECT_KERNEL_ID: &str = "_notebook.selectKernel";

/// `KernelPickerMRUStrategy`.
pub struct KernelPickerMRUStrategy;

impl KernelPickerMRUStrategy {
    pub fn resolve_kernel(
        notebook: &dyn INotebookTextModelLike,
        notebook_kernel_history_service: Rc<dyn INotebookKernelHistoryService>,
        command_service: Rc<dyn ICommandService>,
    ) -> Promise<Option<Rc<dyn INotebookKernel>>> {
        let already_selected = notebook_kernel_history_service.get_kernels(notebook);

        if already_selected.selected.is_some() {
            return Promise::resolved(already_selected.selected);
        }

        let notebook_like = crate::contrib::notebook::common::notebook_kernel_service::NotebookTextModelLike { uri: notebook.uri(), notebook_type: notebook.notebook_type() };
        command_service.execute_command(SELECT_KERNEL_ID, Vec::new()).then_result(move |result: Result<_, Error>| {
            result?;
            Ok(notebook_kernel_history_service.get_kernels(&notebook_like).selected)
        })
    }
}
