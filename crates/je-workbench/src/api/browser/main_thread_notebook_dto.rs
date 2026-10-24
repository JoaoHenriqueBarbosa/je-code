//! Porte parcial de `src/vs/workbench/api/browser/mainThreadNotebookDto.ts`: só as conversões do DTO do host de extensões para o
//! modelo (`fromNotebookOutputItemDto`, `fromNotebookOutputDto`, `fromCellExecuteUpdateDto` e `fromCellExecuteCompleteDto`) que o
//! `MainThreadNotebookKernels` usa. As demais (`toNotebook*Dto`, `fromNotebookCellDataDto`, `fromNotebookDataDto`,
//! `fromCellEditOperationDto`) entram junto de quem as pedir.
//!
//! Divergências: `fromCellExecuteUpdateDto` devolve o enum [`ICellExecuteUpdate`]; o ramo `else { return data; }` (a atualização de
//! estado de execução) é a variante `ExecutionState` com o mesmo valor. `fromCellExecuteCompleteDto` é a identidade
//! (`ICellExecutionCompleteDto` é o próprio `ICellExecutionComplete`).

use crate::api::common::ext_host_protocol::{ICellExecuteUpdateDto, ICellExecutionCompleteDto, NotebookOutputDto, NotebookOutputItemDto};
use crate::contrib::notebook::common::notebook_common::{IOutputDto, IOutputItemDto};
use crate::contrib::notebook::common::notebook_execution_service::{ICellExecuteOutputEdit, ICellExecuteOutputItemEdit};
use crate::contrib::notebook::common::notebook_execution_state_service::{ICellExecuteUpdate, ICellExecutionComplete};

/// `NotebookDto.fromNotebookOutputItemDto`.
pub fn from_notebook_output_item_dto(item: &NotebookOutputItemDto) -> IOutputItemDto {
    IOutputItemDto { mime: item.mime.clone(), data: item.value_bytes.clone() }
}

/// `NotebookDto.fromNotebookOutputDto`.
pub fn from_notebook_output_dto(output: &NotebookOutputDto) -> IOutputDto {
    IOutputDto { output_id: output.output_id.clone(), metadata: output.metadata.clone(), outputs: output.items.iter().map(from_notebook_output_item_dto).collect() }
}

/// `NotebookDto.fromCellExecuteUpdateDto`.
pub fn from_cell_execute_update_dto(data: &ICellExecuteUpdateDto) -> ICellExecuteUpdate {
    match data {
        ICellExecuteUpdateDto::Output(data) => {
            ICellExecuteUpdate::Output(ICellExecuteOutputEdit { cell_handle: data.cell_handle, append: data.append, outputs: data.outputs.iter().map(from_notebook_output_dto).collect() })
        }
        ICellExecuteUpdateDto::OutputItems(data) => ICellExecuteUpdate::OutputItems(ICellExecuteOutputItemEdit {
            append: data.append,
            output_id: data.output_id.clone(),
            items: data.items.iter().map(from_notebook_output_item_dto).collect(),
        }),
        ICellExecuteUpdateDto::ExecutionState(data) => ICellExecuteUpdate::ExecutionState(data.clone()),
    }
}

/// `NotebookDto.fromCellExecuteCompleteDto`.
pub fn from_cell_execute_complete_dto(data: &ICellExecutionCompleteDto) -> ICellExecutionComplete {
    data.clone()
}
