//! Porte de `src/vs/workbench/contrib/notebook/browser/services/notebookExecutionServiceImpl.ts`: o `NotebookExecutionService`,
//! que cria as execuções de célula, resolve o kernel, filtra as células pelas linguagens do kernel, roda os participantes de
//! execução e pede a execução ao kernel; e cancela execuções.
//!
//! Divergências:
//! - Os serviços do construtor (`@ICommandService`, `@INotebookKernelService`, `@INotebookKernelHistoryService`,
//!   `@IWorkspaceTrustRequestService`, `@INotebookLoggingService`, `@INotebookExecutionStateService`) são recebidos por
//!   [`NotebookExecutionService::new`], que devolve o serviço em um `Rc`.
//! - Os métodos `async` são [`async_fn`]: o corpo roda já, de forma síncrona, até o primeiro `await`.
//! - `_activeProxyKernelExecutionToken` (nunca atribuído no original, só descartado no `dispose`) não existe no porte.
//! - `Iterable<NotebookCellTextModel>` e `Iterable<number>` são `Vec`; o `Set<ICellExecutionParticipant>` é uma lista sem
//!   repetição por identidade do `Rc`.
//! - `JSON.stringify(handles)` é o `format!("{:?}", handles)` (a lista de inteiros, `[1, 2]`) sem espaços, escrito por
//!   [`json_handles`].

use std::cell::RefCell;
use std::rc::{Rc, Weak};

use je_base::async_::async_fn;
use je_base::lifecycle::{DisposableRef, IDisposable, to_disposable};
use je_base::nls::localize;
use je_base::promise::Promise;
use je_platform::commands::commands::ICommandService;
use je_platform::contextkey::contextkey::IContextKeyService;
use je_platform::workspace_trust::{IWorkspaceTrustRequestService, WorkspaceTrustRequestOptions};

use crate::contrib::notebook::browser::view_parts::notebook_kernel_quick_pick_strategy::KernelPickerMRUStrategy;
use crate::contrib::notebook::common::model::notebook_cell_text_model::NotebookCellTextModel;
use crate::contrib::notebook::common::notebook_common::{CellKind, ICell, INotebookTextModel, NotebookCellExecutionState};
use crate::contrib::notebook::common::notebook_execution_service::{ICellExecutionParticipant, INotebookExecutionService};
use crate::contrib::notebook::common::notebook_execution_state_service::{INotebookCellExecution, INotebookExecutionStateService};
use crate::contrib::notebook::common::notebook_kernel_service::{INotebookKernelHistoryService, INotebookKernelService, INotebookTextModelLike, NotebookTextModelLike};
use crate::contrib::notebook::common::notebook_logging_service::INotebookLoggingService;

/// `JSON.stringify(handles)`.
fn json_handles(handles: &[i64]) -> String {
    format!("[{}]", handles.iter().map(i64::to_string).collect::<Vec<_>>().join(","))
}

/// `NotebookExecutionService`.
pub struct NotebookExecutionService {
    this: Weak<NotebookExecutionService>,
    command_service: Rc<dyn ICommandService>,
    notebook_kernel_service: Rc<dyn INotebookKernelService>,
    notebook_kernel_history_service: Rc<dyn INotebookKernelHistoryService>,
    workspace_trust_request_service: Rc<dyn IWorkspaceTrustRequestService>,
    log_service: Rc<dyn INotebookLoggingService>,
    notebook_execution_state_service: Rc<dyn INotebookExecutionStateService>,
    cell_execution_participants: RefCell<Vec<Rc<dyn ICellExecutionParticipant>>>,
}

impl NotebookExecutionService {
    pub fn new(
        command_service: Rc<dyn ICommandService>,
        notebook_kernel_service: Rc<dyn INotebookKernelService>,
        notebook_kernel_history_service: Rc<dyn INotebookKernelHistoryService>,
        workspace_trust_request_service: Rc<dyn IWorkspaceTrustRequestService>,
        log_service: Rc<dyn INotebookLoggingService>,
        notebook_execution_state_service: Rc<dyn INotebookExecutionStateService>,
    ) -> Rc<Self> {
        Rc::new_cyclic(|weak| Self {
            this: weak.clone(),
            command_service,
            notebook_kernel_service,
            notebook_kernel_history_service,
            workspace_trust_request_service,
            log_service,
            notebook_execution_state_service,
            cell_execution_participants: RefCell::new(Vec::new()),
        })
    }

    /// `runExecutionParticipants(executions)`.
    fn run_execution_participants(&self, executions: Vec<Rc<dyn INotebookCellExecution>>) -> Promise<()> {
        let participants: Vec<Rc<dyn ICellExecutionParticipant>> = self.cell_execution_participants.borrow().clone();
        async_fn(async move {
            for participant in participants {
                participant.on_will_execute_cell(executions.clone()).await?;
            }
            Ok(())
        })
    }
}

impl INotebookExecutionService for NotebookExecutionService {
    fn execute_notebook_cells(&self, notebook: &dyn INotebookTextModel, cells: Vec<Rc<NotebookCellTextModel>>, _context_key_service: Rc<dyn IContextKeyService>) -> Promise<()> {
        let cells_arr: Vec<Rc<NotebookCellTextModel>> = cells.into_iter().filter(|cell| cell.cell_kind() == CellKind::Code).collect();
        let notebook_like = NotebookTextModelLike { uri: notebook.uri(), notebook_type: notebook.notebook_type() };
        let this = self.this.upgrade().expect("o serviço de execução está vivo");

        async_fn(async move {
            if cells_arr.is_empty() {
                return Ok(());
            }

            this.log_service.debug("Execution", &json_handles(&cells_arr.iter().map(|cell| cell.handle()).collect::<Vec<_>>()));
            let message = localize("notebookRunTrust", "Executing a notebook cell will run code from this workspace.", &[]);
            let trust = this.workspace_trust_request_service.request_workspace_trust(Some(WorkspaceTrustRequestOptions { buttons: None, message: Some(message) })).await?;
            if trust != Some(true) {
                return Ok(());
            }

            // create cell executions
            let mut cell_executions: Vec<(Rc<NotebookCellTextModel>, Rc<dyn INotebookCellExecution>)> = Vec::new();
            for cell in &cells_arr {
                if this.notebook_execution_state_service.get_cell_execution(&cell.uri()).is_some() {
                    continue;
                }
                cell_executions.push((cell.clone(), this.notebook_execution_state_service.create_cell_execution(&notebook_like.uri, cell.handle())));
            }

            let kernel =
                KernelPickerMRUStrategy::resolve_kernel(&notebook_like, this.notebook_kernel_history_service.clone(), this.command_service.clone()).await?;

            let Some(kernel) = kernel else {
                // clear all pending cell executions
                for (_, cell_exe) in &cell_executions {
                    cell_exe.complete(Default::default());
                }
                return Ok(());
            };

            this.notebook_kernel_history_service.add_most_recent_kernel(&kernel);

            // filter cell executions based on selected kernel
            let mut valid_cell_executions: Vec<Rc<dyn INotebookCellExecution>> = Vec::new();
            for (cell, cell_execution) in &cell_executions {
                if !kernel.supported_languages().contains(&cell.language()) {
                    cell_execution.complete(Default::default());
                } else {
                    valid_cell_executions.push(cell_execution.clone());
                }
            }

            // request execution
            if !valid_cell_executions.is_empty() {
                this.run_execution_participants(valid_cell_executions.clone()).await?;

                this.notebook_kernel_service.select_kernel_for_notebook(Some(&kernel), &notebook_like);
                kernel.execute_notebook_cells_request(&notebook_like.uri, valid_cell_executions.iter().map(|cell| cell.cell_handle()).collect()).await?;
                // the connecting state can change before the kernel resolves executeNotebookCellsRequest
                let unconfirmed: Vec<&Rc<dyn INotebookCellExecution>> = valid_cell_executions.iter().filter(|exe| exe.state() == NotebookCellExecutionState::Unconfirmed).collect();
                if !unconfirmed.is_empty() {
                    this.log_service
                        .debug("Execution", &format!("Completing unconfirmed executions {}", json_handles(&unconfirmed.iter().map(|exe| exe.cell_handle()).collect::<Vec<_>>())));
                    for exe in unconfirmed {
                        exe.complete(Default::default());
                    }
                }
                this.log_service.debug("Execution", &format!("Completed executions {}", json_handles(&valid_cell_executions.iter().map(|exe| exe.cell_handle()).collect::<Vec<_>>())));
            }
            Ok(())
        })
    }

    fn cancel_notebook_cell_handles(&self, notebook: &dyn INotebookTextModel, cells: Vec<i64>) -> Promise<()> {
        self.log_service.debug("Execution", &format!("CancelNotebookCellHandles {}", json_handles(&cells)));
        match self.notebook_kernel_service.get_selected_or_suggested_kernel(notebook) {
            Some(kernel) => kernel.cancel_notebook_cell_execution(&notebook.uri(), cells),
            None => Promise::resolved(()),
        }
    }

    fn cancel_notebook_cells(&self, notebook: &dyn INotebookTextModel, cells: Vec<Rc<NotebookCellTextModel>>) -> Promise<()> {
        // `this.cancelNotebookCellHandles(...)` sem `await`: a promessa não é devolvida
        self.cancel_notebook_cell_handles(notebook, cells.iter().map(|cell| cell.handle()).collect());
        Promise::resolved(())
    }

    fn register_execution_participant(&self, participant: Rc<dyn ICellExecutionParticipant>) -> DisposableRef {
        {
            let mut participants = self.cell_execution_participants.borrow_mut();
            if !participants.iter().any(|candidate| std::ptr::addr_eq(Rc::as_ptr(candidate), Rc::as_ptr(&participant))) {
                participants.push(participant.clone());
            }
        }
        let weak = self.this.clone();
        to_disposable(move || {
            if let Some(this) = weak.upgrade() {
                this.cell_execution_participants.borrow_mut().retain(|candidate| !std::ptr::addr_eq(Rc::as_ptr(candidate), Rc::as_ptr(&participant)));
            }
        })
    }
}

impl IDisposable for NotebookExecutionService {
    /// O `dispose()` do original só descarta o `_activeProxyKernelExecutionToken`, que nunca é atribuído.
    fn dispose(&self) {}

    fn constructor_name(&self) -> &'static str {
        "NotebookExecutionService"
    }
}
