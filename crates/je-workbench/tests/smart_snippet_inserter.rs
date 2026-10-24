//! Porte de `src/vs/workbench/contrib/preferences/test/common/smartSnippetInserter.test.ts` (suíte
//! `SmartSnippetInserter`), caso a caso e na mesma ordem.
//!
//! Notas de tradução:
//! - `ensureNoDisposablesAreLeakedInTestSuite()` vira o guarda criado no começo de cada caso.
//! - `createTextModel` é o de `je-editor/tests/support/test_text_model.rs`.
//! - O `runner` recebe a função de asserção (`assert(desiredPos, pos, prepend, append)`) e `model.dispose()` roda
//!   no fim, como no original; o `deepStrictEqual` do resultado é o `assert_eq!` do [`InsertSnippetResult`].

#[path = "../../je-base/tests/common/mod.rs"]
mod common;
#[allow(dead_code)]
#[path = "../../je-editor/tests/support/test_text_model.rs"]
mod test_text_model;

use common::ensure_no_disposables_are_leaked_in_test_suite;
use je_editor::position::Position;
use je_workbench::contrib::preferences::smart_snippet_inserter::{InsertSnippetResult, SmartSnippetInserter};
use test_text_model::create_text_model;

fn test_smart_snippet_inserter(text: &[&str], runner: impl FnOnce(&dyn Fn(Position, Position, &'static str, &'static str))) {
    let model = create_text_model(&text.join("\n"));
    runner(&|desired_pos, pos, prepend, append| {
        let actual = SmartSnippetInserter::insert_snippet(&model, desired_pos);
        let expected = InsertSnippetResult { position: pos, prepend, append };
        assert_eq!(actual, expected);
    });
    model.dispose();
}

#[test]
fn empty_text() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    test_smart_snippet_inserter(&[], |assert| {
        assert(Position::new(1, 1), Position::new(1, 1), "\n[", "]");
    });

    test_smart_snippet_inserter(&[" "], |assert| {
        assert(Position::new(1, 1), Position::new(1, 2), "\n[", "]");
        assert(Position::new(1, 2), Position::new(1, 2), "\n[", "]");
    });

    test_smart_snippet_inserter(&["// just some text"], |assert| {
        assert(Position::new(1, 1), Position::new(1, 18), "\n[", "]");
        assert(Position::new(1, 18), Position::new(1, 18), "\n[", "]");
    });

    test_smart_snippet_inserter(&["// just some text", ""], |assert| {
        assert(Position::new(1, 1), Position::new(2, 1), "\n[", "]");
        assert(Position::new(1, 18), Position::new(2, 1), "\n[", "]");
        assert(Position::new(2, 1), Position::new(2, 1), "\n[", "]");
    });
}

#[test]
fn empty_array_1() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    test_smart_snippet_inserter(&["// just some text", "[]"], |assert| {
        assert(Position::new(1, 1), Position::new(2, 2), "", "");
        assert(Position::new(2, 1), Position::new(2, 2), "", "");
        assert(Position::new(2, 2), Position::new(2, 2), "", "");
        assert(Position::new(2, 3), Position::new(2, 2), "", "");
    });
}

#[test]
fn empty_array_2() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    test_smart_snippet_inserter(&["// just some text", "[", "]"], |assert| {
        assert(Position::new(1, 1), Position::new(2, 2), "", "");
        assert(Position::new(2, 1), Position::new(2, 2), "", "");
        assert(Position::new(2, 2), Position::new(2, 2), "", "");
        assert(Position::new(3, 1), Position::new(3, 1), "", "");
        assert(Position::new(3, 2), Position::new(3, 1), "", "");
    });
}

#[test]
fn empty_array_3() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    test_smart_snippet_inserter(&["// just some text", "[", "// just some text", "]"], |assert| {
        assert(Position::new(1, 1), Position::new(2, 2), "", "");
        assert(Position::new(2, 1), Position::new(2, 2), "", "");
        assert(Position::new(2, 2), Position::new(2, 2), "", "");
        assert(Position::new(3, 1), Position::new(3, 1), "", "");
        assert(Position::new(3, 2), Position::new(3, 1), "", "");
        assert(Position::new(4, 1), Position::new(4, 1), "", "");
        assert(Position::new(4, 2), Position::new(4, 1), "", "");
    });
}

#[test]
fn one_element_array_1() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    test_smart_snippet_inserter(&["// just some text", "[", "{}", "]"], |assert| {
        assert(Position::new(1, 1), Position::new(2, 2), "", ",");
        assert(Position::new(2, 1), Position::new(2, 2), "", ",");
        assert(Position::new(2, 2), Position::new(2, 2), "", ",");
        assert(Position::new(3, 1), Position::new(3, 1), "", ",");
        assert(Position::new(3, 2), Position::new(3, 1), "", ",");
        assert(Position::new(3, 3), Position::new(3, 3), ",", "");
        assert(Position::new(4, 1), Position::new(4, 1), ",", "");
        assert(Position::new(4, 2), Position::new(4, 1), ",", "");
    });
}

#[test]
fn two_elements_array_1() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    test_smart_snippet_inserter(&["// just some text", "[", "{},", "{}", "]"], |assert| {
        assert(Position::new(1, 1), Position::new(2, 2), "", ",");
        assert(Position::new(2, 1), Position::new(2, 2), "", ",");
        assert(Position::new(2, 2), Position::new(2, 2), "", ",");
        assert(Position::new(3, 1), Position::new(3, 1), "", ",");
        assert(Position::new(3, 2), Position::new(3, 1), "", ",");
        assert(Position::new(3, 3), Position::new(3, 3), ",", "");
        assert(Position::new(3, 4), Position::new(3, 4), "", ",");
        assert(Position::new(4, 1), Position::new(4, 1), "", ",");
        assert(Position::new(4, 2), Position::new(4, 1), "", ",");
        assert(Position::new(4, 3), Position::new(4, 3), ",", "");
        assert(Position::new(5, 1), Position::new(5, 1), ",", "");
        assert(Position::new(5, 2), Position::new(5, 1), ",", "");
    });
}

#[test]
fn two_elements_array_2() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    test_smart_snippet_inserter(&["// just some text", "[", "{},{}", "]"], |assert| {
        assert(Position::new(1, 1), Position::new(2, 2), "", ",");
        assert(Position::new(2, 1), Position::new(2, 2), "", ",");
        assert(Position::new(2, 2), Position::new(2, 2), "", ",");
        assert(Position::new(3, 1), Position::new(3, 1), "", ",");
        assert(Position::new(3, 2), Position::new(3, 1), "", ",");
        assert(Position::new(3, 3), Position::new(3, 3), ",", "");
        assert(Position::new(3, 4), Position::new(3, 4), "", ",");
        assert(Position::new(3, 5), Position::new(3, 4), "", ",");
        assert(Position::new(3, 6), Position::new(3, 6), ",", "");
        assert(Position::new(4, 1), Position::new(4, 1), ",", "");
        assert(Position::new(4, 2), Position::new(4, 1), ",", "");
    });
}
