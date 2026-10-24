//! Porte parcial de `src/vs/workbench/contrib/notebook/common/notebookExecutionService.ts`: só o tipo de atualização
//! de execução de célula (`CellExecutionUpdateType`) e as duas edições de saída (`ICellExecuteOutputEdit` e
//! `ICellExecuteOutputItemEdit`) de que `notebookExecutionStateService.ts` depende. O resto do arquivo
//! (`INotebookExecutionService`, que executa células com um `IContextKeyService`) fica para a fatia do serviço de
//! execução.

use super::notebook_common::{IOutputDto, IOutputItemDto};

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
