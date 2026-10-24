//! Porte de `src/vs/workbench/api/browser/mainThreadNotebookDto.ts`: o `NotebookDto`, as conversões entre os DTOs do protocolo
//! com o host de extensões (`NotebookDataDto`, `NotebookCellDataDto`, `NotebookOutputDto`...) e o modelo do notebook
//! (`NotebookData`, `ICellDto2`, `IOutputDto`, `ICell`).
//!
//! Divergências:
//! - O `namespace NotebookDto` é o módulo; as funções perdem o prefixo do namespace e viram `snake_case`.
//! - `source: string` do DTO é o `String`; o `source` do modelo é o `JsString` (com unidades UTF-16 soltas), então a ida passa
//!   por `to_string_lossy` e a volta por `JsString::from`. `NotebookCellDto.source` (`getLinesContent()`) é a lista de linhas
//!   como `String`, e o `eol` é o texto do `getEOL()` do buffer.
//! - `ICellDto2.collapseState` não existe no DTO do protocolo: a volta deixa `None`.
//! - `NotebookCellDto.mime` não é preenchido por `toNotebookCellDto` (o original não o lê da célula): é `None`.
//! - `fromCellExecuteUpdateDto` devolve o enum [`ICellExecuteUpdate`]; o ramo `else { return data; }` (a atualização de estado de
//!   execução) é a variante `ExecutionState` com o mesmo valor. `fromCellExecuteCompleteDto` é a identidade
//!   (`ICellExecutionCompleteDto` é o próprio `ICellExecutionComplete`).
//! - Fica de fora `fromCellEditOperationDto`: só o `NotebookTextModel.applyEdits` do host o lê, e o protocolo do porte já entrega a
//!   edição de célula como o `IWorkspaceNotebookCellEdit` do modelo (ver `ext_host_protocol`).

use je_base::js_string::JsString;

use crate::api::common::ext_host_protocol::{
    ICellExecuteUpdateDto, ICellExecutionCompleteDto, NotebookCellDataDto, NotebookCellDto, NotebookDataDto, NotebookOutputDto, NotebookOutputItemDto,
};
use crate::contrib::notebook::common::notebook_common::{ICell, ICellDto2, IOutputDto, IOutputItemDto, NotebookData};
use crate::contrib::notebook::common::notebook_execution_service::{ICellExecuteOutputEdit, ICellExecuteOutputItemEdit};
use crate::contrib::notebook::common::notebook_execution_state_service::{ICellExecuteUpdate, ICellExecutionComplete};

/// `toNotebookOutputItemDto`.
pub fn to_notebook_output_item_dto(item: &IOutputItemDto) -> NotebookOutputItemDto {
    NotebookOutputItemDto { mime: item.mime.clone(), value_bytes: item.data.clone() }
}

/// `toNotebookOutputDto`.
pub fn to_notebook_output_dto(output: &IOutputDto) -> NotebookOutputDto {
    NotebookOutputDto { output_id: output.output_id.clone(), metadata: output.metadata.clone(), items: output.outputs.iter().map(to_notebook_output_item_dto).collect() }
}

/// `toNotebookCellDataDto`.
pub fn to_notebook_cell_data_dto(cell: &ICellDto2) -> NotebookCellDataDto {
    NotebookCellDataDto {
        cell_kind: cell.cell_kind,
        language: cell.language.clone(),
        mime: cell.mime.clone(),
        source: cell.source.to_string_lossy(),
        internal_metadata: cell.internal_metadata.clone(),
        metadata: cell.metadata.clone(),
        outputs: cell.outputs.iter().map(to_notebook_output_dto).collect(),
    }
}

/// `toNotebookDataDto`.
pub fn to_notebook_data_dto(data: &NotebookData) -> NotebookDataDto {
    NotebookDataDto { metadata: data.metadata.clone(), cells: data.cells.iter().map(to_notebook_cell_data_dto).collect() }
}

/// `fromNotebookOutputItemDto`.
pub fn from_notebook_output_item_dto(item: &NotebookOutputItemDto) -> IOutputItemDto {
    IOutputItemDto { mime: item.mime.clone(), data: item.value_bytes.clone() }
}

/// `fromNotebookOutputDto`.
pub fn from_notebook_output_dto(output: &NotebookOutputDto) -> IOutputDto {
    IOutputDto { output_id: output.output_id.clone(), metadata: output.metadata.clone(), outputs: output.items.iter().map(from_notebook_output_item_dto).collect() }
}

/// `fromNotebookCellDataDto`.
pub fn from_notebook_cell_data_dto(cell: &NotebookCellDataDto) -> ICellDto2 {
    ICellDto2 {
        cell_kind: cell.cell_kind,
        language: cell.language.clone(),
        mime: cell.mime.clone(),
        source: JsString::from(cell.source.as_str()),
        outputs: cell.outputs.iter().map(from_notebook_output_dto).collect(),
        metadata: cell.metadata.clone(),
        internal_metadata: cell.internal_metadata.clone(),
        collapse_state: None,
    }
}

/// `fromNotebookDataDto`.
pub fn from_notebook_data_dto(data: &NotebookDataDto) -> NotebookData {
    NotebookData { metadata: data.metadata.clone(), cells: data.cells.iter().map(from_notebook_cell_data_dto).collect() }
}

/// `toNotebookCellDto`.
pub fn to_notebook_cell_dto(cell: &dyn ICell) -> NotebookCellDto {
    let text_buffer = cell.text_buffer();
    let text_buffer = text_buffer.borrow();
    NotebookCellDto {
        handle: cell.handle(),
        uri: cell.uri(),
        source: text_buffer.get_lines_content().iter().map(JsString::to_string_lossy).collect(),
        eol: String::from_utf16_lossy(text_buffer.get_eol().as_units()),
        language: cell.language(),
        cell_kind: cell.cell_kind(),
        mime: None,
        outputs: cell.outputs().iter().map(|output| to_notebook_output_dto(&output.as_dto())).collect(),
        metadata: Some(cell.metadata()),
        internal_metadata: Some(cell.internal_metadata()),
    }
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
