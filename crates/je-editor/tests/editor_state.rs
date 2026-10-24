//! Porte de `src/vs/editor/contrib/editorState/test/browser/editorState.test.ts` (suíte `Editor Core - Editor State`), caso
//! a caso e na mesma ordem.
//!
//! Notas de tradução:
//! - `createEditor` devolve um objeto literal com `getModel`, `getPosition`, `getSelection`, `getScrollLeft` e
//!   `getScrollTop` (`as ICodeEditor`); aqui é [`StubEditor`], que implementa só o [`EditorStateSource`] que o
//!   `EditorState` consulta. Os campos que o literal deixa `undefined` são `Option`.
//! - `getVersionId: () => model.version` devolve `undefined` quando a versão falta, e o `EditorState` imprime
//!   `undefined` no identificador, o que [`EditorStateModel::version_id`] `None` representa.

#[path = "../../je-base/tests/common/mod.rs"]
mod common;

use common::ensure_no_disposables_are_leaked_in_test_suite;
use je_base::uri::URI;
use je_editor::editor_state::{EditorState, EditorStateModel, EditorStateSource, code_editor_state_flag};
use je_editor::position::Position;
use je_editor::range::Range;
use je_editor::selection::Selection;

const ALL_FLAGS: u32 = code_editor_state_flag::VALUE
    | code_editor_state_flag::SELECTION
    | code_editor_state_flag::POSITION
    | code_editor_state_flag::SCROLL;

/// `IStubEditorState.model`.
#[derive(Default)]
struct StubModel {
    uri: Option<URI>,
    version: Option<i64>,
}

/// `IStubEditorState.scroll`.
#[derive(Default)]
struct StubScroll {
    left: Option<f64>,
    top: Option<f64>,
}

/// `IStubEditorState`.
#[derive(Default)]
struct StubEditorState {
    model: Option<StubModel>,
    position: Option<Position>,
    selection: Option<Selection>,
    scroll: Option<StubScroll>,
}

/// O objeto literal que `createEditor` devolve.
struct StubEditor {
    model: Option<EditorStateModel>,
    position: Option<Position>,
    selection: Option<Selection>,
    scroll: Option<StubScroll>,
}

impl EditorStateSource for StubEditor {
    fn get_model(&self) -> Option<EditorStateModel> {
        self.model.clone()
    }

    fn get_position(&self) -> Option<Position> {
        self.position
    }

    fn get_selection(&self) -> Option<Range> {
        self.selection.map(|selection| *selection)
    }

    fn get_scroll_left(&self) -> Option<f64> {
        self.scroll.as_ref().and_then(|scroll| scroll.left)
    }

    fn get_scroll_top(&self) -> Option<f64> {
        self.scroll.as_ref().and_then(|scroll| scroll.top)
    }
}

fn create_editor(state: StubEditorState) -> StubEditor {
    let mapped_model = state.model.map(|model| EditorStateModel {
        uri: model.uri.unwrap_or_else(|| URI::parse("http://dummy.org", false)),
        version_id: model.version,
    });
    StubEditor { model: mapped_model, position: state.position, selection: state.selection, scroll: state.scroll }
}

fn validate(source: StubEditorState, target: StubEditorState) -> bool {
    let source_editor = create_editor(source);
    let target_editor = create_editor(target);

    EditorState::new(&source_editor, ALL_FLAGS).validate(&target_editor)
}

#[test]
fn empty_editor_state_should_be_valid() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    let result = validate(StubEditorState::default(), StubEditorState::default());
    assert!(result);
}

#[test]
fn different_model_uris_should_be_invalid() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    let result = validate(
        StubEditorState { model: Some(StubModel { uri: Some(URI::parse("http://test1", false)), version: None }), ..Default::default() },
        StubEditorState { model: Some(StubModel { uri: Some(URI::parse("http://test2", false)), version: None }), ..Default::default() },
    );

    assert!(!result);
}

#[test]
fn different_model_versions_should_be_invalid() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    let result = validate(
        StubEditorState { model: Some(StubModel { uri: None, version: Some(1) }), ..Default::default() },
        StubEditorState { model: Some(StubModel { uri: None, version: Some(2) }), ..Default::default() },
    );

    assert!(!result);
}

#[test]
fn different_positions_should_be_invalid() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    let result = validate(
        StubEditorState { position: Some(Position::new(1, 2)), ..Default::default() },
        StubEditorState { position: Some(Position::new(2, 3)), ..Default::default() },
    );

    assert!(!result);
}

#[test]
fn different_selections_should_be_invalid() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    let result = validate(
        StubEditorState { selection: Some(Selection::new(1, 2, 3, 4)), ..Default::default() },
        StubEditorState { selection: Some(Selection::new(5, 2, 3, 4)), ..Default::default() },
    );

    assert!(!result);
}

#[test]
fn different_scroll_positions_should_be_invalid() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    let result = validate(
        StubEditorState { scroll: Some(StubScroll { left: Some(1.0), top: Some(2.0) }), ..Default::default() },
        StubEditorState { scroll: Some(StubScroll { left: Some(3.0), top: Some(2.0) }), ..Default::default() },
    );

    assert!(!result);
}
