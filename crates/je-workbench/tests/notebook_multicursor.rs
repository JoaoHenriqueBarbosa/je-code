//! Porte de `src/vs/workbench/contrib/notebook/test/browser/contrib/notebookMulticursor.test.ts` (suíte `NotebookMultiCursorController`), caso a caso e na mesma ordem. O caso está portado.
//!
//! Notas de tradução:
//! - `ensureNoDisposablesAreLeakedInTestSuite()` é o `LeakGuard` do caso.
//! - `new class extends mock<ICellViewModel>() { override readonly handle; override readonly uri }` é o [`MockCell`], que implementa o
//!   [`IMultiCursorCell`] (o que o relé lê da célula).
//! - `Object.create(CursorsController.prototype)` com `paste` trocado por uma função do caso é o [`MirroredController`], que implementa o
//!   [`ICursorsOperationTarget`] guardando o `isBlock` que recebeu; os outros comandos lançam, como lançaria o protótipo sem estado.
//! - `Object.assign(Object.create(NotebookMultiCursorController.prototype), { anchorCell, trackedCells, cursorsControllers })` é o
//!   `NotebookMultiCursorController::from_parts`, e o editor `Object.create(null)` de `anchorCell` é o editor ausente do [`AnchorCell`].
//! - `satisfies PastePayload` é o `PastePayload` do porte, e `payload` é o `to_dynamic()` dele.

#[path = "../../je-base/tests/common/mod.rs"]
mod common;

use std::cell::Cell;
use std::rc::Rc;

use common::ensure_no_disposables_are_leaked_in_test_suite;
use je_base::errors::{self, Error};
use je_base::js_string::JsString;
use je_base::map::ResourceMap;
use je_base::uri::URI;
use je_editor::code_editor_widget::{Handler, ITriggerEditorOperationEvent, PastePayload};
use je_editor::view_model_event_dispatcher::ViewModelEventsCollector;
use je_workbench::contrib::notebook::browser::contrib::multicursor::notebook_multicursor::{
    AnchorCell, ICursorsOperationTarget, IMultiCursorCell, NotebookMultiCursorController, TrackedCell,
};

/// O `new class extends mock<ICellViewModel>()` do caso.
struct MockCell {
    handle: i64,
    uri: URI,
}

impl IMultiCursorCell for MockCell {
    fn handle(&self) -> i64 {
        self.handle
    }

    fn uri(&self) -> URI {
        self.uri.clone()
    }
}

/// `mirroredController`: um `CursorsController` sem estado com o `paste` trocado.
struct MirroredController {
    mirrored_is_block: Cell<Option<bool>>,
}

impl ICursorsOperationTarget for MirroredController {
    fn start_composition(&self, _events_collector: &ViewModelEventsCollector) {
        errors::throw(Error::new("Cannot read properties of undefined (startComposition)"))
    }

    fn end_composition(&self, _events_collector: &ViewModelEventsCollector, _source: Option<&str>) {
        errors::throw(Error::new("Cannot read properties of undefined (endComposition)"))
    }

    fn composition_type(
        &self,
        _events_collector: &ViewModelEventsCollector,
        _text: &[u16],
        _replace_prev_char_cnt: i32,
        _replace_next_char_cnt: i32,
        _position_delta: i32,
        _source: Option<&str>,
    ) {
        errors::throw(Error::new("Cannot read properties of undefined (compositionType)"))
    }

    fn paste(
        &self,
        _events_collector: &ViewModelEventsCollector,
        _text: &[u16],
        _paste_on_new_line: bool,
        _multicursor_text: Option<&[JsString]>,
        _source: Option<&str>,
        is_block: bool,
    ) {
        self.mirrored_is_block.set(Some(is_block));
    }

    fn cut(&self, _events_collector: &ViewModelEventsCollector, _source: Option<&str>) {
        errors::throw(Error::new("Cannot read properties of undefined (cut)"))
    }
}

#[test]
fn forwards_block_paste_to_mirrored_cells() {
    let _store = ensure_no_disposables_are_leaked_in_test_suite();
    let anchor_cell: Rc<dyn IMultiCursorCell> = Rc::new(MockCell { handle: 1, uri: URI::parse("test:///notebook/cell1", false) });
    let mirrored_cell: Rc<dyn IMultiCursorCell> = Rc::new(MockCell { handle: 2, uri: URI::parse("test:///notebook/cell2", false) });
    let mirrored_controller = Rc::new(MirroredController { mirrored_is_block: Cell::new(None) });

    let mut cursors_controllers: ResourceMap<Rc<dyn ICursorsOperationTarget>> = ResourceMap::new();
    cursors_controllers.set(mirrored_cell.uri(), mirrored_controller.clone());
    let controller = NotebookMultiCursorController::from_parts(
        Some(AnchorCell { cell: anchor_cell.clone(), editor: None }),
        vec![TrackedCell { cell_view_model: anchor_cell }, TrackedCell { cell_view_model: mirrored_cell }],
        cursors_controllers,
    );

    let event = ITriggerEditorOperationEvent {
        handler_id: Handler::PASTE.to_string(),
        payload: PastePayload {
            text: JsString::from("A\nBC\nD"),
            paste_on_new_line: false,
            multicursor_text: Some(vec![JsString::from("A"), JsString::from("BC"), JsString::from("D")]),
            mode: None,
            is_block: true,
        }
        .to_dynamic(),
        source: Some("keyboard".to_string()),
    };
    controller.handle_editor_operation_event(&event);

    assert_eq!(mirrored_controller.mirrored_is_block.get(), Some(true));
}
