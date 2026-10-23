//! Porte de `src/vs/editor/test/common/core/positionOffsetTransformer.test.ts` (suíte `PositionOffsetTransformer`), caso a caso e na mesma ordem.

#[path = "../../je-base/tests/common/mod.rs"]
mod common;

use common::ensure_no_disposables_are_leaked_in_test_suite;
use je_editor::position_to_offset::{PositionOffsetTransformer, PositionOffsetTransformerBase};

const STR: &str = "123456\nabcdef\nghijkl\nmnopqr";

#[test]
fn get_position() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    let t = PositionOffsetTransformer::new(STR);
    // `new OffsetRange(0, str.length + 2).map(...)`
    let actual: Vec<String> = (0..STR.len() as i32 + 2).map(|i| t.get_position(i).to_string()).collect();
    assert_eq!(
        actual,
        vec![
            "(1,1)", "(1,2)", "(1,3)", "(1,4)", "(1,5)", "(1,6)", "(1,7)", "(2,1)", "(2,2)", "(2,3)", "(2,4)",
            "(2,5)", "(2,6)", "(2,7)", "(3,1)", "(3,2)", "(3,3)", "(3,4)", "(3,5)", "(3,6)", "(3,7)", "(4,1)",
            "(4,2)", "(4,3)", "(4,4)", "(4,5)", "(4,6)", "(4,7)", "(4,8)"
        ]
    );
}

#[test]
fn get_offset() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    let t = PositionOffsetTransformer::new(STR);
    for i in 0..STR.len() as i32 + 1 {
        assert_eq!(t.get_offset(t.get_position(i)), i);
    }
}
