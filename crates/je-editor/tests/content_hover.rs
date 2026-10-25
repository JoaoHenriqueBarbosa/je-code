//! Porte de `src/vs/editor/contrib/hover/test/browser/contentHover.test.ts` (suíte `Content Hover`), caso a caso e
//! na mesma ordem.
//!
//! Notas de tradução:
//! - `ensureNoDisposablesAreLeakedInTestSuite()` é o `LeakGuard` criado no começo de cada caso.
//! - `<IHoverPart>{ range }` (um objeto parcial com `as`) é o `TestHoverPart`, com o `owner` no participante
//!   nulo `NullHoverParticipant` (o original deixa o `owner` indefinido, que a conta de posições não lê).
//! - `{}` e `{ wordWrap: 'wordWrapColumn', wordWrapColumn: 6 }` são os objetos `Dynamic` das opções.
//! - `assert.deepStrictEqual(actual, { showAtPosition, showAtSecondaryPosition })` é `assert_eq!` com o
//!   `HoverPositions`.

#[path = "../../je-base/tests/common/mod.rs"]
mod common;
mod support;

use std::rc::Rc;

use common::ensure_no_disposables_are_leaked_in_test_suite;
use je_base::dynamic::Dynamic;
use je_editor::hover::content_hover_rendered::{HoverPositions, RenderedContentHover};
use je_editor::hover::hover_operation::HoverStartSource;
use je_editor::hover::hover_types::{HoverAnchor, IEditorHoverParticipant, IHoverPart};
use je_editor::model::ModelDecoration;
use je_editor::position::Position;
use je_editor::range::Range;
use support::test_code_editor::{TestCodeEditorInstantiationOptions, with_test_code_editor};

/// O participante que o `owner` dos objetos parciais do original deixa indefinido.
struct NullHoverParticipant;

impl IEditorHoverParticipant for NullHoverParticipant {
    fn hover_ordinal(&self) -> i32 {
        0
    }

    fn compute_sync(&self, _anchor: &HoverAnchor, _line_decorations: &[ModelDecoration], _source: HoverStartSource) -> Vec<Rc<dyn IHoverPart>> {
        Vec::new()
    }

    fn get_accessible_content(&self, _hover_part: &dyn IHoverPart) -> String {
        String::new()
    }
}

/// `<IHoverPart>{ range }`.
struct TestHoverPart {
    range: Range,
}

impl IHoverPart for TestHoverPart {
    fn owner(&self) -> Rc<dyn IEditorHoverParticipant> {
        Rc::new(NullHoverParticipant)
    }

    fn range(&self) -> Range {
        self.range
    }

    fn is_valid_for_hover_anchor(&self, _anchor: &HoverAnchor) -> bool {
        true
    }
}

#[test]
fn issue_151235_gitlens_hover_shows_up_in_the_wrong_place() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    let text = "just some text";
    with_test_code_editor(text, TestCodeEditorInstantiationOptions::default(), |editor, _view_model, _services| {
        let part = TestHoverPart { range: Range::new(4, 1, 5, 6) };
        let actual = RenderedContentHover::compute_hover_positions(editor, Range::new(5, 5, 5, 5), &[&part]);
        assert_eq!(
            actual,
            HoverPositions { show_at_position: Position::new(5, 5), show_at_secondary_position: Position::new(5, 5) }
        );
    });
}

#[test]
fn issue_95328_hover_placement_with_word_wrap() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    let text = "just some text";
    let opts = TestCodeEditorInstantiationOptions::new(Dynamic::object([
        ("wordWrap", Dynamic::string("wordWrapColumn")),
        ("wordWrapColumn", Dynamic::Number(6.0)),
    ]));
    with_test_code_editor(text, opts, |editor, _view_model, _services| {
        let part = TestHoverPart { range: Range::new(1, 1, 1, 15) };
        let actual = RenderedContentHover::compute_hover_positions(editor, Range::new(1, 8, 1, 8), &[&part]);
        assert_eq!(
            actual,
            HoverPositions { show_at_position: Position::new(1, 8), show_at_secondary_position: Position::new(1, 6) }
        );
    });
}
