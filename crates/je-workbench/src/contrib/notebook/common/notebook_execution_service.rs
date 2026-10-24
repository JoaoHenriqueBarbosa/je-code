//! Porte de `src/vs/workbench/contrib/notebook/common/notebookExecutionService.ts`: o tipo de atualização de execução de
//! célula (`CellExecutionUpdateType`), as duas edições de saída (`ICellExecuteOutputEdit` e `ICellExecuteOutputItemEdit`), o
//! contrato `INotebookExecutionService` e o `ICellExecutionParticipant`.
//!
//! Divergências:
//! - `Iterable<NotebookCellTextModel>` e `Iterable<number>` são `Vec`, e o `INotebookTextModel` é recebido como
//!   `&dyn INotebookTextModel`.
//! - Os métodos `async` devolvem [`Promise`].

use std::rc::Rc;

use je_base::lifecycle::DisposableRef;
use je_base::promise::Promise;
use je_instantiation::service_id;
use je_platform::contextkey::contextkey::IContextKeyService;

use super::model::notebook_cell_text_model::NotebookCellTextModel;
use super::notebook_common::{INotebookTextModel, IOutputDto, IOutputItemDto};
use super::notebook_execution_state_service::INotebookCellExecution;

/// `CellExecutionUpdateType`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CellExecutionUpdateType {
    Output = 1,
    OutputItems = 2,
    ExecutionState = 3,
}

/// `ICellExecuteOutputEdit`: o `editType` é [`CellExecutionUpdateType::Output`].
#[derive(Clone, Debug)]
pub struct ICellExecuteOutputEdit {
    pub cell_handle: i64,
    pub append: Option<bool>,
    pub outputs: Vec<IOutputDto>,
}

/// `ICellExecuteOutputItemEdit`: o `editType` é [`CellExecutionUpdateType::OutputItems`].
#[derive(Clone, Debug)]
pub struct ICellExecuteOutputItemEdit {
    pub append: Option<bool>,
    pub output_id: String,
    pub items: Vec<IOutputItemDto>,
}

/// `INotebookExecutionService`.
pub trait INotebookExecutionService {
    fn execute_notebook_cells(&self, notebook: &dyn INotebookTextModel, cells: Vec<Rc<NotebookCellTextModel>>, context_key_service: Rc<dyn IContextKeyService>) -> Promise<()>;
    fn cancel_notebook_cells(&self, notebook: &dyn INotebookTextModel, cells: Vec<Rc<NotebookCellTextModel>>) -> Promise<()>;
    fn cancel_notebook_cell_handles(&self, notebook: &dyn INotebookTextModel, cells: Vec<i64>) -> Promise<()>;
    fn register_execution_participant(&self, participant: Rc<dyn ICellExecutionParticipant>) -> DisposableRef;
}

service_id!(pub I_NOTEBOOK_EXECUTION_SERVICE: dyn INotebookExecutionService = "INotebookExecutionService");

/// `ICellExecutionParticipant`.
pub trait ICellExecutionParticipant {
    fn on_will_execute_cell(&self, executions: Vec<Rc<dyn INotebookCellExecution>>) -> Promise<()>;
}
