//! Porte de `src/vs/editor/test/common/model/editStack.test.ts` (suíte `EditStack`), caso a caso e na mesma ordem.

#[path = "../../je-base/tests/common/mod.rs"]
mod common;

use common::ensure_no_disposables_are_leaked_in_test_suite;
use je_base::JsString;
use je_editor::edit_stack::SingleModelEditStackData;
use je_editor::model::EndOfLineSequence;
use je_editor::selection::Selection;
use je_editor::text_change::TextChange;

#[test]
fn issue_118041_unicode_character_undo_bug() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();

    let stack_data = SingleModelEditStackData::new(
        1,
        2,
        EndOfLineSequence::Lf,
        EndOfLineSequence::Lf,
        Some(vec![Selection::new(10, 2, 10, 2)]),
        Some(vec![Selection::new(10, 1, 10, 1)]),
        vec![TextChange::new(428, JsString::from("\u{feff}"), 428, JsString::from(""))],
    );

    let buff = stack_data.serialize();
    let actual = SingleModelEditStackData::deserialize(&buff);

    assert_eq!(actual, stack_data);
}
