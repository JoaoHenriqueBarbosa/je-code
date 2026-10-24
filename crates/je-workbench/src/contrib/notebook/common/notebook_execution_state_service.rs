//! Porte de `src/vs/workbench/contrib/notebook/common/notebookExecutionStateService.ts`: os contratos do serviço de
//! estado de execução dos notebooks (`INotebookExecutionStateService`, `INotebookCellExecution`,
//! `INotebookExecution`), as atualizações e eventos de execução, e o erro de execução de célula
//! (`ICellErrorStackFrame` e `ICellExecutionError`). O serviço em si (`notebookExecutionStateServiceImpl.ts`) fica
//! para a fatia dos serviços.
//!
//! Divergências:
//! - `IRange` e `Range` são o mesmo tipo no porte do editor ([`Range`]).
//! - `stack: string | ICellErrorStackFrame[] | undefined` é `Option<CellExecutionErrorStack>`.
//! - `ICellExecuteUpdate` (`ICellExecuteOutputEdit | ICellExecuteOutputItemEdit | ICellExecutionStateUpdate`, uma
//!   união discriminada por `editType`) é o enum [`ICellExecuteUpdate`]; o `editType` de cada variante é o
//!   [`CellExecutionUpdateType`] correspondente e os structs não o repetem.
//! - `ICellExecutionStateChangedEvent` e `IExecutionStateChangedEvent` têm métodos (`affectsCell`,
//!   `affectsNotebook`), então são traits; a união dos dois (`Event<ICellExecutionStateChangedEvent |
//!   IExecutionStateChangedEvent>`) é o enum [`ExecutionStateChange`], e o `type` que discrimina a união é a variante.
//! - `cellHandle` é `i64`; `executionOrder` e os tempos são `f64`; `changed?: X` (indefinido quando a execução
//!   terminou) é `Option<Rc<dyn X>>`.
//! - `getCellExecutionsByHandleForNotebook` devolve um `HashMap<i64, ...>` no lugar do `Map<number, ...>`.

use std::collections::HashMap;
use std::rc::Rc;

use je_base::event::Event;
use je_base::lifecycle::DisposableRef;
use je_base::uri::{URI, UriComponents};
use je_editor::range::Range;
use je_instantiation::service_id;

use super::notebook_common::{NotebookCellExecutionState, NotebookExecutionState};
use super::notebook_execution_service::{ICellExecuteOutputEdit, ICellExecuteOutputItemEdit};

/// `ICellExecutionStateUpdate`: o `editType` é `CellExecutionUpdateType.ExecutionState`.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct ICellExecutionStateUpdate {
    pub execution_order: Option<f64>,
    pub run_start_time: Option<f64>,
    pub did_pause: Option<bool>,
    pub is_paused: Option<bool>,
}

/// `ICellExecuteUpdate`.
#[derive(Clone, Debug)]
pub enum ICellExecuteUpdate {
    Output(ICellExecuteOutputEdit),
    OutputItems(ICellExecuteOutputItemEdit),
    ExecutionState(ICellExecutionStateUpdate),
}

/// `ICellErrorStackFrame`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ICellErrorStackFrame {
    /// The location of this stack frame. This should be provided as a URI if the location of the call frame can be
    /// accessed by the editor.
    pub uri: Option<UriComponents>,

    pub location: Option<Range>,

    /// The name of the stack frame, typically a method or function name.
    pub label: String,
}

/// `string | ICellErrorStackFrame[]`: o valor definido de `ICellExecutionError.stack`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CellExecutionErrorStack {
    Text(String),
    Frames(Vec<ICellErrorStackFrame>),
}

/// `ICellExecutionError`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ICellExecutionError {
    pub name: String,
    pub message: String,
    pub stack: Option<CellExecutionErrorStack>,
    pub uri: UriComponents,
    pub location: Option<Range>,
}

/// `ICellExecutionComplete`.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ICellExecutionComplete {
    pub run_end_time: Option<f64>,
    pub last_run_success: Option<bool>,
    pub error: Option<ICellExecutionError>,
}

/// `NotebookExecutionType`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NotebookExecutionType {
    Cell = 0,
    Notebook = 1,
}

/// `ICellExecutionStateChangedEvent`: o `type` é [`NotebookExecutionType::Cell`].
pub trait ICellExecutionStateChangedEvent {
    fn notebook(&self) -> URI;
    fn cell_handle(&self) -> i64;
    /// `changed?`: indefinido quando a execução terminou.
    fn changed(&self) -> Option<Rc<dyn INotebookCellExecution>>;
    fn affects_cell(&self, cell: &URI) -> bool;
    fn affects_notebook(&self, notebook: &URI) -> bool;
}

/// `IExecutionStateChangedEvent`: o `type` é [`NotebookExecutionType::Notebook`].
pub trait IExecutionStateChangedEvent {
    fn notebook(&self) -> URI;
    /// `changed?`: indefinido quando a execução terminou.
    fn changed(&self) -> Option<Rc<dyn INotebookExecution>>;
    fn affects_notebook(&self, notebook: &URI) -> bool;
}

/// `ICellExecutionStateChangedEvent | IExecutionStateChangedEvent`.
#[derive(Clone)]
pub enum ExecutionStateChange {
    Cell(Rc<dyn ICellExecutionStateChangedEvent>),
    Notebook(Rc<dyn IExecutionStateChangedEvent>),
}

impl ExecutionStateChange {
    /// O campo `type` do original.
    pub fn type_(&self) -> NotebookExecutionType {
        match self {
            ExecutionStateChange::Cell(_) => NotebookExecutionType::Cell,
            ExecutionStateChange::Notebook(_) => NotebookExecutionType::Notebook,
        }
    }
}

/// `INotebookFailStateChangedEvent`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct INotebookFailStateChangedEvent {
    pub visible: bool,
    pub notebook: URI,
}

/// `IFailedCellInfo`.
#[derive(Clone)]
pub struct IFailedCellInfo {
    pub cell_handle: i64,
    pub disposable: DisposableRef,
    pub visible: bool,
}

/// `INotebookExecutionStateService`.
pub trait INotebookExecutionStateService {
    fn on_did_change_execution(&self) -> Event<ExecutionStateChange>;
    fn on_did_change_last_run_fail_state(&self) -> Event<INotebookFailStateChangedEvent>;

    fn force_cancel_notebook_executions(&self, notebook_uri: &URI);
    fn get_cell_executions_for_notebook(&self, notebook: &URI) -> Vec<Rc<dyn INotebookCellExecution>>;
    fn get_cell_executions_by_handle_for_notebook(&self, notebook: &URI) -> Option<HashMap<i64, Rc<dyn INotebookCellExecution>>>;
    fn get_cell_execution(&self, cell_uri: &URI) -> Option<Rc<dyn INotebookCellExecution>>;
    fn create_cell_execution(&self, notebook: &URI, cell_handle: i64) -> Rc<dyn INotebookCellExecution>;
    fn get_execution(&self, notebook: &URI) -> Option<Rc<dyn INotebookExecution>>;
    fn create_execution(&self, notebook: &URI) -> Rc<dyn INotebookExecution>;
    fn get_last_failed_cell_for_notebook(&self, notebook: &URI) -> Option<i64>;
    fn get_last_completed_cell_for_notebook(&self, notebook: &URI) -> Option<i64>;
}

service_id!(pub I_NOTEBOOK_EXECUTION_STATE_SERVICE: dyn INotebookExecutionStateService = "INotebookExecutionStateService");

/// `INotebookCellExecution`.
pub trait INotebookCellExecution {
    fn notebook(&self) -> URI;
    fn cell_handle(&self) -> i64;
    fn state(&self) -> NotebookCellExecutionState;
    fn did_pause(&self) -> bool;
    fn is_paused(&self) -> bool;

    fn confirm(&self);
    fn update(&self, updates: Vec<ICellExecuteUpdate>);
    fn complete(&self, complete: ICellExecutionComplete);
}

/// `INotebookExecution`.
pub trait INotebookExecution {
    fn notebook(&self) -> URI;
    fn state(&self) -> NotebookExecutionState;

    fn confirm(&self);
    fn begin(&self);
    fn complete(&self);
}
