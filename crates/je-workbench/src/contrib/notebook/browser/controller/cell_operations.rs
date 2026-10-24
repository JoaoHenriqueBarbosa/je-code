//! Porte de `src/vs/workbench/contrib/notebook/browser/controller/cellOperations.ts`: as operações de célula (mudar o tipo,
//! apagar, mover, copiar, juntar, dividir e inserir).
//!
//! Divergências:
//! - As funções `async` do original são síncronas (o `NotebookTextModel.applyEdits` e o `UndoRedoService` do porte são
//!   síncronos; os `await` só esperavam `focusNotebookCell`).
//! - `joinSelectedCells` e `joinCellsWithSurrounds` aplicam as edições pelo `IBulkEditService` (o `ResourceNotebookCellEdit` do
//!   `bulkCellEdits.ts`, frente do `MainThreadNotebook`/`BulkCellEdits`) e `joinSelectedCells` ainda usa o
//!   `INotificationService`; não são portadas aqui. [`join_notebook_cells`], que só calcula as edições, é portada, devolvendo
//!   [`JoinEdit`] (o espelho local de `ResourceTextEdit | ResourceNotebookCellEdit`: `Text` com `resource`, `range` e `text`;
//!   `NotebookCell` com o `resource` do notebook e a `ICellReplaceEdit`).
//! - `insertCell` não recebe o `INotebookKernelHistoryService` (`kernelHistoryService?`, opcional); o ramo
//!   `!cell && viewModel.length === 0` com histórico de kernels cai no `defaultLanguage`, o resultado do original sem histórico.
//! - `ICellViewModel` é o [`CellViewModel`]; `editor.cellAt(i)` com índice fora do intervalo é `None` (o `undefined`).
//! - `computeCellLinesContents` e `_splitPointsToBoundaries` usam [`Position`] no lugar de `IPosition`.

use je_base::uri::URI;
use je_editor::model::EndOfLinePreference;
use je_editor::position::Position;
use je_editor::range::Range;
use je_editor::language_service::ILanguageService;

use super::{IActiveNotebookEditor, INotebookActionContext, expand_cell_ranges_with_hidden_cells};
use crate::contrib::notebook::browser::notebook_browser::CellEditState;
use crate::contrib::notebook::browser::view_model::cell_view_model_types::CellFocusMode;
use crate::contrib::notebook::browser::view_model::notebook_view_model_impl::{CellViewModel, NotebookViewModel};
use crate::contrib::notebook::common::model::notebook_cell_text_model::clone_notebook_cell_text_model;
use crate::contrib::notebook::common::notebook_common::{
    CellKind, ICell, ICellDto2, ICellEditOperation, ICellMoveEdit, ICellReplaceEdit, IOutputDto, ISelectionIndexState, ISelectionState, NotebookCellMetadata,
};
use crate::contrib::notebook::common::notebook_range::{ICellRange, cell_range_contains, cell_ranges_to_indexes};

const PLAINTEXT_LANGUAGE_ID: &str = "plaintext";

fn index_state(focus: ICellRange, selections: Vec<ICellRange>) -> ISelectionState {
    ISelectionState::Index(ISelectionIndexState { focus, selections })
}

/// `{ kind: Index, focus: editor.getFocus(), selections: editor.getSelections() }`.
fn current_state(editor: &dyn IActiveNotebookEditor) -> ISelectionState {
    index_state(editor.get_focus(), editor.get_selections())
}

/// A célula como `ICellDto2` com o tipo, o texto e o idioma dados (o literal de `cells: [{ ... }]` do original).
fn cell_dto(cell: &CellViewModel, kind: CellKind, source: String, language: String, mime: Option<String>) -> ICellDto2 {
    ICellDto2 {
        source: source.as_str().into(),
        language,
        mime,
        cell_kind: kind,
        outputs: cell.model().outputs().iter().map(|output| output.as_dto()).collect(),
        metadata: Some(cell.base().metadata()),
        internal_metadata: None,
        collapse_state: None,
    }
}

pub fn change_cell_to_kind(kind: CellKind, context: &INotebookActionContext, language: Option<&str>, mime: Option<&str>) {
    let notebook_editor = &context.notebook_editor;

    if notebook_editor.is_read_only() {
        return;
    }

    let mut language = language.map(str::to_string);

    if context.ui.unwrap_or(false)
        && let Some(cell) = &context.cell
    {
        if cell.cell_kind() == kind {
            return;
        }

        let text = cell.get_text();
        let idx = notebook_editor.get_cell_index(cell);

        if language.is_none() {
            let available_languages = notebook_editor.active_kernel_supported_languages().unwrap_or_default();
            language = Some(available_languages.first().cloned().unwrap_or_else(|| PLAINTEXT_LANGUAGE_ID.to_string()));
        }

        notebook_editor.text_model().apply_edits(
            &[ICellEditOperation::Replace(ICellReplaceEdit {
                index: idx as usize,
                count: 1,
                cells: vec![cell_dto(cell, kind, text, language.clone().expect("language"), Some(mime.map(str::to_string).unwrap_or_else(|| cell.base().mime())))],
            })],
            true,
            Some(current_state(notebook_editor.as_ref())),
            &|| Some(current_state(notebook_editor.as_ref())),
            None,
            true,
        );
        let new_cell = notebook_editor.cell_at(idx).expect("cell");
        notebook_editor.focus_notebook_cell(&new_cell, if cell.base().get_edit_state() == CellEditState::Editing { "editor" } else { "container" });
    } else if let Some(selected_cells) = &context.selected_cells {
        let mut raw_edits: Vec<ICellEditOperation> = Vec::new();

        for cell in selected_cells {
            if cell.cell_kind() == kind {
                continue;
            }
            let text = cell.get_text();
            let idx = notebook_editor.get_cell_index(cell);

            if language.is_none() {
                let available_languages = notebook_editor.active_kernel_supported_languages().unwrap_or_default();
                language = Some(available_languages.first().cloned().unwrap_or_else(|| PLAINTEXT_LANGUAGE_ID.to_string()));
            }

            raw_edits.push(ICellEditOperation::Replace(ICellReplaceEdit {
                index: idx as usize,
                count: 1,
                cells: vec![cell_dto(cell, kind, text, language.clone().expect("language"), Some(mime.map(str::to_string).unwrap_or_else(|| cell.base().mime())))],
            }));
        }

        notebook_editor.text_model().apply_edits(&raw_edits, true, Some(current_state(notebook_editor.as_ref())), &|| Some(current_state(notebook_editor.as_ref())), None, true);
    }
}

pub fn run_delete_action(editor: &dyn IActiveNotebookEditor, cell: &CellViewModel) {
    let text_model = editor.text_model();
    let mut selections = editor.get_selections();
    let target_cell_index = editor.get_cell_index(cell);
    let containing_selection = selections.iter().find(|selection| selection.start <= target_cell_index && target_cell_index < selection.end).copied();

    let compute_undo_redo = !editor.is_read_only() || text_model.view_type() == "interactive";
    if let Some(containing_selection) = containing_selection {
        selections.reverse();
        let edits: Vec<ICellEditOperation> = selections
            .iter()
            .map(|selection| ICellEditOperation::Replace(ICellReplaceEdit { index: selection.start as usize, count: (selection.end - selection.start) as usize, cells: Vec::new() }))
            .collect();

        let next_cell_after_containing_selection = if containing_selection.end >= editor.get_length() as i64 { None } else { editor.cell_at(containing_selection.end) };

        text_model.apply_edits(
            &edits,
            true,
            Some(current_state(editor)),
            &|| {
                if let Some(next_cell) = &next_cell_after_containing_selection {
                    let cell_index = text_model.cells().iter().position(|cell| cell.handle() == next_cell.handle()).map(|index| index as i64).unwrap_or(-1);
                    Some(index_state(ICellRange { start: cell_index, end: cell_index + 1 }, vec![ICellRange { start: cell_index, end: cell_index + 1 }]))
                } else if text_model.length() > 0 {
                    let last_cell_index = text_model.length() as i64 - 1;
                    Some(index_state(ICellRange { start: last_cell_index, end: last_cell_index + 1 }, vec![ICellRange { start: last_cell_index, end: last_cell_index + 1 }]))
                } else {
                    Some(index_state(ICellRange { start: 0, end: 0 }, vec![ICellRange { start: 0, end: 0 }]))
                }
            },
            None,
            compute_undo_redo,
        );
    } else {
        let focus = editor.get_focus();
        let edits = vec![ICellEditOperation::Replace(ICellReplaceEdit { index: target_cell_index as usize, count: 1, cells: Vec::new() })];

        let mut final_selections: Vec<ICellRange> = Vec::new();
        for selection in &selections {
            if selection.end <= target_cell_index {
                final_selections.push(*selection);
            } else if selection.start > target_cell_index {
                final_selections.push(ICellRange { start: selection.start - 1, end: selection.end - 1 });
            } else {
                final_selections.push(ICellRange { start: target_cell_index, end: target_cell_index + 1 });
            }
        }

        let focus_is_target = editor.cell_at(focus.start).is_some_and(|focus_cell| &focus_cell == cell);
        let new_focus = if focus_is_target {
            if focus.end == text_model.length() as i64 { ICellRange { start: focus.start - 1, end: focus.end - 1 } } else { focus }
        } else if focus.start > target_cell_index {
            ICellRange { start: focus.start - 1, end: focus.end - 1 }
        } else {
            focus
        };

        text_model.apply_edits(&edits, true, Some(current_state(editor)), &|| Some(index_state(new_focus, final_selections.clone())), None, compute_undo_redo);
    }
}

pub fn move_cell_range(context: &INotebookActionContext, direction: &str) {
    let editor = &context.notebook_editor;
    let text_model = editor.text_model();

    if editor.is_read_only() {
        return;
    }

    let range: Option<ICellRange> = if let Some(cell) = &context.cell {
        let idx = editor.get_cell_index(cell);
        Some(ICellRange { start: idx, end: idx + 1 })
    } else {
        let selections = editor.get_selections();
        let model_ranges = expand_cell_ranges_with_hidden_cells(editor.as_ref(), &selections);
        model_ranges.first().copied()
    };

    let Some(range) = range else { return };
    if range.start == range.end {
        return;
    }

    if direction == "up" {
        if range.start == 0 {
            return;
        }

        let index_above = range.start - 1;
        let final_selection = ICellRange { start: range.start - 1, end: range.end - 1 };
        let focus = editor.get_focus();
        let new_focus = if cell_range_contains(&range, &focus) { ICellRange { start: focus.start - 1, end: focus.end - 1 } } else { ICellRange { start: range.start - 1, end: range.start } };
        text_model.apply_edits(
            &[ICellEditOperation::Move(ICellMoveEdit { index: index_above as usize, length: 1, new_idx: (range.end - 1) as usize })],
            true,
            Some(current_state(editor.as_ref())),
            &|| Some(index_state(new_focus, vec![final_selection])),
            None,
            true,
        );
        let focus_range = editor.get_selections().first().copied().unwrap_or_else(|| editor.get_focus());
        editor.reveal_cell_range_in_view(focus_range);
    } else {
        if range.end >= text_model.length() as i64 {
            return;
        }

        let index_below = range.end;
        let final_selection = ICellRange { start: range.start + 1, end: range.end + 1 };
        let focus = editor.get_focus();
        let new_focus = if cell_range_contains(&range, &focus) { ICellRange { start: focus.start + 1, end: focus.end + 1 } } else { ICellRange { start: range.start + 1, end: range.start + 2 } };

        text_model.apply_edits(
            &[ICellEditOperation::Move(ICellMoveEdit { index: index_below as usize, length: 1, new_idx: range.start as usize })],
            true,
            Some(current_state(editor.as_ref())),
            &|| Some(index_state(new_focus, vec![final_selection])),
            None,
            true,
        );

        let focus_range = editor.get_selections().first().copied().unwrap_or_else(|| editor.get_focus());
        editor.reveal_cell_range_in_view(focus_range);
    }
}

pub fn copy_cell_range(context: &INotebookActionContext, direction: &str) {
    let editor = &context.notebook_editor;
    let text_model = editor.text_model();

    if editor.is_read_only() {
        return;
    }

    let range: Option<ICellRange> = if context.ui.unwrap_or(false) {
        let target_cell = context.cell.as_ref().expect("cell");
        let target_cell_index = editor.get_cell_index(target_cell);
        Some(ICellRange { start: target_cell_index, end: target_cell_index + 1 })
    } else {
        let selections = editor.get_selections();
        let model_ranges = expand_cell_ranges_with_hidden_cells(editor.as_ref(), &selections);
        model_ranges.first().copied()
    };

    let Some(range) = range else { return };
    if range.start == range.end {
        return;
    }

    let cloned_cells = || -> Vec<ICellDto2> {
        cell_ranges_to_indexes(&[range]).into_iter().map(|index| clone_notebook_cell_text_model(&editor.cell_at(index).expect("cell").model())).collect()
    };

    if direction == "up" {
        let focus = editor.get_focus();
        let selections = editor.get_selections();
        text_model.apply_edits(
            &[ICellEditOperation::Replace(ICellReplaceEdit { index: range.end as usize, count: 0, cells: cloned_cells() })],
            true,
            Some(index_state(focus, selections.clone())),
            &|| Some(index_state(focus, selections.clone())),
            None,
            true,
        );
    } else {
        let focus = editor.get_focus();
        let selections = editor.get_selections();
        let new_cells = cloned_cells();
        let count_delta = new_cells.len() as i64;
        let ui = context.ui.unwrap_or(false);
        let new_focus = if ui { focus } else { ICellRange { start: focus.start + count_delta, end: focus.end + count_delta } };
        let new_selections = if ui { selections.clone() } else { vec![ICellRange { start: range.start + count_delta, end: range.end + count_delta }] };
        text_model.apply_edits(
            &[ICellEditOperation::Replace(ICellReplaceEdit { index: range.end as usize, count: 0, cells: cloned_cells() })],
            true,
            Some(index_state(focus, selections.clone())),
            &|| Some(index_state(new_focus, new_selections.clone())),
            None,
            true,
        );

        let focus_range = editor.get_selections().first().copied().unwrap_or_else(|| editor.get_focus());
        editor.reveal_cell_range_in_view(focus_range);
    }
}

/// O espelho local de `ResourceTextEdit | ResourceNotebookCellEdit` (ver as divergências).
#[derive(Clone, Debug)]
pub enum JoinEdit {
    /// `ResourceTextEdit(resource, { range, text })`.
    Text { resource: URI, range: Range, text: String },
    /// `ResourceNotebookCellEdit(resource, cellEdit)`.
    NotebookCell { resource: URI, cell_edit: ICellReplaceEdit },
}

/// O resultado de `joinNotebookCells`.
#[derive(Clone)]
pub struct JoinResult {
    pub edits: Vec<JoinEdit>,
    pub cell: CellViewModel,
    pub end_focus: ICellRange,
    pub end_selections: Vec<ICellRange>,
}

pub fn join_notebook_cells(editor: &dyn IActiveNotebookEditor, range: ICellRange, direction: &str, constraint: Option<CellKind>) -> Option<JoinResult> {
    if editor.is_read_only() {
        return None;
    }

    let text_model = editor.text_model();
    let cells = editor.get_cells_in_range(Some(range));

    if cells.is_empty() {
        return None;
    }

    if range.start == 0 && direction == "above" {
        return None;
    }

    if range.end == text_model.length() as i64 && direction == "below" {
        return None;
    }

    for cell in &cells {
        if constraint.is_some_and(|constraint| cell.cell_kind() != constraint) {
            return None;
        }
    }

    let eol_of = |cell: &CellViewModel| -> &'static str {
        match cell.base().text_buffer().borrow().get_eol() {
            je_editor::model::Eol::Lf => "\n",
            je_editor::model::Eol::CrLf => "\r\n",
        }
    };

    if direction == "above" {
        let above = editor.cell_at(range.start - 1).expect("cell above");
        if constraint.is_some_and(|constraint| above.cell_kind() != constraint) {
            return None;
        }

        let insert_content: String = cells.iter().map(|cell| format!("{}{}", eol_of(cell), cell.get_text())).collect();
        let above_cell_line_count = above.base().text_buffer().borrow().get_line_count() as i32;
        let above_cell_last_line_end_column = above.base().text_buffer().borrow().get_line_length(above_cell_line_count) as i32;

        Some(JoinResult {
            edits: vec![
                JoinEdit::Text {
                    resource: above.uri(),
                    range: Range::new(above_cell_line_count, above_cell_last_line_end_column + 1, above_cell_line_count, above_cell_last_line_end_column + 1),
                    text: insert_content,
                },
                JoinEdit::NotebookCell {
                    resource: text_model.uri(),
                    cell_edit: ICellReplaceEdit { index: range.start as usize, count: (range.end - range.start) as usize, cells: Vec::new() },
                },
            ],
            cell: above,
            end_focus: ICellRange { start: range.start - 1, end: range.start },
            end_selections: vec![ICellRange { start: range.start - 1, end: range.start }],
        })
    } else {
        let below = editor.cell_at(range.end).expect("cell below");
        if constraint.is_some_and(|constraint| below.cell_kind() != constraint) {
            return None;
        }

        let cell = cells[0].clone();
        let mut rest_cells: Vec<CellViewModel> = cells[1..].to_vec();
        rest_cells.push(below);
        let insert_content: String = rest_cells.iter().map(|cl| format!("{}{}", eol_of(cl), cl.get_text())).collect();

        let cell_line_count = cell.base().text_buffer().borrow().get_line_count() as i32;
        let cell_last_line_end_column = cell.base().text_buffer().borrow().get_line_length(cell_line_count) as i32;

        Some(JoinResult {
            edits: vec![
                JoinEdit::Text {
                    resource: cell.uri(),
                    range: Range::new(cell_line_count, cell_last_line_end_column + 1, cell_line_count, cell_last_line_end_column + 1),
                    text: insert_content,
                },
                JoinEdit::NotebookCell {
                    resource: text_model.uri(),
                    cell_edit: ICellReplaceEdit { index: (range.start + 1) as usize, count: (range.end - range.start) as usize, cells: Vec::new() },
                },
            ],
            cell,
            end_focus: ICellRange { start: range.start, end: range.start + 1 },
            end_selections: vec![ICellRange { start: range.start, end: range.start + 1 }],
        })
    }
}

fn split_points_to_boundaries(split_points: &[Position], text_buffer: &je_editor::piece_tree_text_buffer::PieceTreeTextBuffer) -> Option<Vec<Position>> {
    let mut boundaries: Vec<Position> = Vec::new();
    let line_cnt = text_buffer.get_line_count() as i32;
    let get_line_len = |line_number: i32| text_buffer.get_line_length(line_number) as i32;

    let mut split_points = split_points.to_vec();
    split_points.sort_by(|l, r| {
        let line_diff = l.line_number - r.line_number;
        let column_diff = l.column - r.column;
        (if line_diff != 0 { line_diff } else { column_diff }).cmp(&0)
    });

    for sp in split_points {
        let mut sp = sp;
        if get_line_len(sp.line_number) + 1 == sp.column && sp.column != 1 /* empty line */ && sp.line_number < line_cnt {
            sp = Position::new(sp.line_number + 1, 1);
        }
        push_if_absent(&mut boundaries, sp);
    }

    if boundaries.is_empty() {
        return None;
    }

    let model_start = Position::new(1, 1);
    let model_end = Position::new(line_cnt, get_line_len(line_cnt) + 1);
    let mut result = vec![model_start];
    result.extend(boundaries);
    result.push(model_end);
    Some(result)
}

fn push_if_absent(positions: &mut Vec<Position>, p: Position) {
    let absent = match positions.last() {
        None => true,
        Some(last) => last.line_number != p.line_number || last.column != p.column,
    };
    if absent {
        positions.push(p);
    }
}

pub fn compute_cell_lines_contents(cell: &CellViewModel, split_points: &[Position]) -> Option<Vec<String>> {
    let text_buffer = cell.base().text_buffer();
    let text_buffer = text_buffer.borrow();
    let range_boundaries = split_points_to_boundaries(split_points, &text_buffer)?;
    let mut new_line_models = Vec::new();
    for i in 1..range_boundaries.len() {
        let start = &range_boundaries[i - 1];
        let end = &range_boundaries[i];

        new_line_models.push(text_buffer.get_value_in_range(&Range::new(start.line_number, start.column, end.line_number, end.column), EndOfLinePreference::TextDefined).to_string_lossy());
    }

    Some(new_line_models)
}

#[allow(clippy::too_many_arguments)]
pub fn insert_cell(
    language_service: &dyn ILanguageService,
    editor: &dyn IActiveNotebookEditor,
    index: i64,
    type_: CellKind,
    direction: &str,
    initial_text: &str,
    ui: bool,
) -> Option<CellViewModel> {
    let view_model = editor.get_view_model();
    let active_kernel_languages = editor.active_kernel_supported_languages();
    if view_model.options().is_read_only {
        return None;
    }

    let cell = editor.cell_at(index);
    let next_index = if ui { view_model.get_next_visible_cell_index(index) } else { index + 1 };
    let language: String;
    if type_ == CellKind::Code {
        let supported_languages = active_kernel_languages.unwrap_or_else(|| language_service.get_registered_language_ids());
        let default_language = supported_languages.first().filter(|language| !language.is_empty()).cloned().unwrap_or_else(|| PLAINTEXT_LANGUAGE_ID.to_string());

        let mut chosen = match &cell {
            Some(cell) if cell.cell_kind() == CellKind::Code => cell.language(),
            Some(cell) if cell.cell_kind() == CellKind::Markup => {
                let nearest_code_cell_index = view_model.nearest_code_cell_index(index);
                if nearest_code_cell_index > -1 { view_model.cell_at(nearest_code_cell_index).expect("cell").language() } else { default_language.clone() }
            }
            None if view_model.length() == 0 => default_language.clone(),
            _ => {
                if cell.is_none() && direction == "above" {
                    view_model
                        .view_cells()
                        .iter()
                        .find(|cell| cell.cell_kind() == CellKind::Code)
                        .map(|cell| cell.language())
                        .filter(|language| !language.is_empty())
                        .unwrap_or_else(|| default_language.clone())
                } else {
                    default_language.clone()
                }
            }
        };

        if !supported_languages.contains(&chosen) {
            chosen = default_language;
        }
        language = chosen;
    } else {
        language = "markdown".to_string();
    }

    let insert_index = if cell.is_some() { if direction == "above" { index } else { next_index } } else { index };
    Some(insert_cell_at_index(&view_model, insert_index, initial_text, &language, type_, None, Vec::new(), true, true))
}

#[allow(clippy::too_many_arguments)]
pub fn insert_cell_at_index(
    view_model: &NotebookViewModel,
    index: i64,
    source: &str,
    language: &str,
    type_: CellKind,
    metadata: Option<NotebookCellMetadata>,
    outputs: Vec<IOutputDto>,
    synchronous: bool,
    push_undo_stop: bool,
) -> CellViewModel {
    let end_selections = index_state(ICellRange { start: index, end: index + 1 }, vec![ICellRange { start: index, end: index + 1 }]);
    view_model.notebook_document().apply_edits(
        &[ICellEditOperation::Replace(ICellReplaceEdit {
            index: index as usize,
            count: 0,
            cells: vec![ICellDto2 { cell_kind: type_, language: language.to_string(), mime: None, outputs, metadata, source: source.into(), internal_metadata: None, collapse_state: None }],
        })],
        synchronous,
        Some(index_state(view_model.get_focus(), view_model.get_selections())),
        &|| Some(end_selections.clone()),
        None,
        push_undo_stop && !view_model.options().is_read_only,
    );
    view_model.cell_at(index).expect("cell")
}

/// Mantém `CellFocusMode` importável pelos chamadores de `focus_notebook_cell`.
pub fn focus_mode_for(focus_item: &str) -> CellFocusMode {
    match focus_item {
        "editor" => CellFocusMode::Editor,
        "output" => CellFocusMode::Output,
        _ => CellFocusMode::Container,
    }
}
