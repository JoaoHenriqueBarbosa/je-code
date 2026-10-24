//! Porte de `src/vs/workbench/api/common/extHostNotebookEditors.ts`: os eventos de seleção e de intervalos visíveis dos editores
//! de notebook (`vscode.window.onDidChangeNotebookEditorSelection` e `onDidChangeNotebookEditorVisibleRanges`) e a coluna de cada
//! editor.
//!
//! Divergências:
//! - `ExtHostNotebookEditors` é um `Rc` ([`ExtHostNotebookEditors::new`]) que guarda o controlador por `Rc`. O `URI.revive` do
//!   original já está feito no protocolo.
//! - `vscode.NotebookEditorSelectionChangeEvent` e `vscode.NotebookEditorVisibleRangesChangeEvent` são as structs deste arquivo.
//!   O `Object.freeze` do evento de seleção não existe (a struct é só dados).
//! - O `data` do log (`debug(..., id, data)`) é o objeto [`Dynamic`] com `visibleRanges` e `selections` (só os presentes).
//! - Um `NotebookRange.to` que lança (intervalo inválido) é o pânico com o [`Error`](je_base::errors::Error), como o `throw`.
//! - `getEditorById` lança para um editor desconhecido: é o pânico do [`ExtHostNotebookController::get_editor_by_id`].

use std::rc::Rc;

use je_base::dynamic::Dynamic;
use je_base::errors;
use je_base::event::{Emitter, Event};
use je_platform::log::{LogArg, LogServiceRef, Logger};

use super::ext_host_notebook::ExtHostNotebookController;
use super::ext_host_notebook_editor::NotebookEditor;
use super::ext_host_protocol::{ExtHostNotebookEditorsShape, INotebookEditorPropertiesChangeData, INotebookEditorViewColumnInfo};
use super::ext_host_type_converters as type_converters;
use super::ext_host_type_converters::CellRange;
use super::ext_host_types as types;

/// `vscode.NotebookEditorSelectionChangeEvent`.
#[derive(Clone)]
pub struct NotebookEditorSelectionChangeEvent {
    pub notebook_editor: NotebookEditor,
    pub selections: Vec<types::NotebookRange>,
}

/// `vscode.NotebookEditorVisibleRangesChangeEvent`.
#[derive(Clone)]
pub struct NotebookEditorVisibleRangesChangeEvent {
    pub notebook_editor: NotebookEditor,
    pub visible_ranges: Vec<types::NotebookRange>,
}

/// `data.visibleRanges.ranges.map(typeConverters.NotebookRange.to)` e o análogo de `selections`.
fn to_notebook_ranges(ranges: &[CellRange]) -> Vec<types::NotebookRange> {
    ranges.iter().map(|range| type_converters::NotebookRange::to(range).unwrap_or_else(|error| errors::throw(error))).collect()
}

/// O `data` do log do `$acceptEditorPropertiesChanged`.
fn properties_to_dynamic(data: &INotebookEditorPropertiesChangeData) -> Dynamic {
    let ranges = |ranges: &[CellRange]| Dynamic::array(ranges.iter().map(type_converters::NotebookRange::to_dto));
    let mut entries: Vec<(&str, Dynamic)> = Vec::new();
    if let Some(visible_ranges) = &data.visible_ranges {
        entries.push(("visibleRanges", Dynamic::object([("ranges", ranges(&visible_ranges.ranges))])));
    }
    if let Some(selections) = &data.selections {
        entries.push(("selections", Dynamic::object([("selections", ranges(&selections.selections))])));
    }
    Dynamic::object(entries)
}

/// `ExtHostNotebookEditors`.
pub struct ExtHostNotebookEditors {
    on_did_change_notebook_editor_selection: Rc<Emitter<NotebookEditorSelectionChangeEvent>>,
    on_did_change_notebook_editor_visible_ranges: Rc<Emitter<NotebookEditorVisibleRangesChangeEvent>>,
    log_service: LogServiceRef,
    notebooks_and_editors: Rc<ExtHostNotebookController>,
}

impl ExtHostNotebookEditors {
    pub fn new(log_service: LogServiceRef, notebooks_and_editors: Rc<ExtHostNotebookController>) -> Rc<ExtHostNotebookEditors> {
        Rc::new(ExtHostNotebookEditors {
            on_did_change_notebook_editor_selection: Emitter::new(),
            on_did_change_notebook_editor_visible_ranges: Emitter::new(),
            log_service,
            notebooks_and_editors,
        })
    }

    /// `onDidChangeNotebookEditorSelection`.
    pub fn on_did_change_notebook_editor_selection(&self) -> Event<NotebookEditorSelectionChangeEvent> {
        self.on_did_change_notebook_editor_selection.event()
    }

    /// `onDidChangeNotebookEditorVisibleRanges`.
    pub fn on_did_change_notebook_editor_visible_ranges(&self) -> Event<NotebookEditorVisibleRangesChangeEvent> {
        self.on_did_change_notebook_editor_visible_ranges.event()
    }
}

impl ExtHostNotebookEditorsShape for ExtHostNotebookEditors {
    fn rpc_accept_editor_properties_changed(&self, id: &str, data: INotebookEditorPropertiesChangeData) {
        self.log_service.debug_args("ExtHostNotebook#$acceptEditorPropertiesChanged", &[LogArg::from(id), LogArg::Value(properties_to_dynamic(&data))]);
        let editor = self.notebooks_and_editors.get_editor_by_id(id);
        // ONE: make all state updates
        if let Some(visible_ranges) = &data.visible_ranges {
            editor.accept_visible_ranges(to_notebook_ranges(&visible_ranges.ranges));
        }
        if let Some(selections) = &data.selections {
            editor.accept_selections(to_notebook_ranges(&selections.selections));
        }

        // TWO: send all events after states have been updated
        if data.visible_ranges.is_some() {
            let api_editor = editor.api_editor();
            self.on_did_change_notebook_editor_visible_ranges.fire(NotebookEditorVisibleRangesChangeEvent { visible_ranges: api_editor.visible_ranges(), notebook_editor: api_editor });
        }
        if data.selections.is_some() {
            let api_editor = editor.api_editor();
            self.on_did_change_notebook_editor_selection.fire(NotebookEditorSelectionChangeEvent { selections: api_editor.selections(), notebook_editor: api_editor });
        }
    }

    fn rpc_accept_editor_view_columns(&self, data: INotebookEditorViewColumnInfo) {
        for (id, column) in &data {
            let editor = self.notebooks_and_editors.get_editor_by_id(id);
            editor.accept_view_column(Some(type_converters::ViewColumn::to(*column)));
        }
    }
}
