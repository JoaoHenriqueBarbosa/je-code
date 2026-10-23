//! Porte de `src/vs/editor/test/common/model/bracketPairColorizer/length.test.ts` (suíte `Bracket Pair Colorizer - Length`), caso a caso e na mesma ordem.

#[path = "../../je-base/tests/common/mod.rs"]
mod common;

use common::ensure_no_disposables_are_leaked_in_test_suite;
use je_editor::bracket_pairs_text_model_part::bracket_pairs_tree::length::{Length, length_add, length_diff_non_negative, length_to_obj, to_length};

fn to_str(length: Length) -> String {
    length_to_obj(length).to_string()
}

#[test]
fn basic() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();

    let l1 = to_length(100, 10);
    assert_eq!(length_to_obj(l1).line_count, 100);
    assert_eq!(length_to_obj(l1).column_count, 10);

    assert_eq!(to_str(length_add(l1, to_length(100, 10))), "200,10");
    assert_eq!(to_str(length_add(l1, to_length(0, 10))), "100,20");
}

#[test]
fn length_diff_non_neg() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();

    assert_eq!(to_str(length_diff_non_negative(to_length(100, 10), to_length(100, 20))), "0,10");

    assert_eq!(to_str(length_diff_non_negative(to_length(100, 10), to_length(101, 20))), "1,20");

    assert_eq!(to_str(length_diff_non_negative(to_length(101, 30), to_length(101, 20))), "0,0");

    assert_eq!(to_str(length_diff_non_negative(to_length(102, 10), to_length(101, 20))), "0,0");
}
