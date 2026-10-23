//! Porte de `src/vs/editor/test/common/model/editableTextModel.test.ts` (suítes `EditorModel - EditableTextModel.applyEdits updates mightContainRTL`, `EditorModel - EditableTextModel.applyEdits updates mightContainNonBasicASCII`, `EditorModel - EditableTextModel.applyEdits` e `CRLF edit normalization`), caso a caso e na mesma ordem.
//!
//! Cada `suite` do original vira um `mod`.
//!
//! Notas de tradução:
//! - `model.applyEdits(edits, true)` é [`TextModel::apply_edits_computing_undo`]; as operações inversas
//!   ([`je_editor::model::ValidEditOperation`]) voltam a ser entrada de `applyEdits` por
//!   `to_single_edit`, do módulo de suporte (no TypeScript isso é tipagem estrutural).
//! - O `try { ... } catch` de `testApplyEditsFails` é `je_base::errors::catch`.
//! - `model.getEOL()` devolve [`je_editor::model::Eol`], não a string do fim de linha.
//!
//! [`TextModel::apply_edits_computing_undo`]: je_editor::text_model::TextModel::apply_edits_computing_undo

#[path = "../../je-base/tests/common/mod.rs"]
mod common;
mod support;

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use common::ensure_no_disposables_are_leaked_in_test_suite;
use je_base::JsString;
use je_base::errors;
use je_base::lifecycle::{DisposableRef, IDisposable};
use je_editor::edit_operation::SingleEditOperation;
use je_editor::mirror_text_model::MirrorTextModel;
use je_editor::model::{EndOfLinePreference, EndOfLineSequence, Eol};
use je_editor::range::Range;
use support::editable_text_model_test_utils::*;
use support::test_text_model::*;

/// `editOp(startLineNumber, startColumn, endLineNumber, endColumn, text)`: as três suítes que o
/// declaram usam o mesmo corpo (a terceira põe `forceMoveMarkers: false` explícito, que é o mesmo que
/// omitir).
fn edit_op(start_line_number: i32, start_column: i32, end_line_number: i32, end_column: i32, text: &[&str]) -> SingleEditOperation {
    SingleEditOperation::new(
        Range::new(start_line_number, start_column, end_line_number, end_column),
        Some(JsString::from(text.join("\n").as_str())),
        false,
    )
}

/// `{ range, text }`: a operação literal dos casos que não usam `editOp`.
fn op(range: Range, text: Option<&str>) -> SingleEditOperation {
    SingleEditOperation::new(range, text.map(JsString::from), false)
}

mod apply_edits_updates_might_contain_rtl {
    use super::*;

    /// `testApplyEdits(original, edits, before, after)`.
    fn test_apply_edits(original: &[&str], edits: &[SingleEditOperation], before: bool, after: bool) {
        let model = create_text_model(&original.join("\n"));
        model.set_eol(EndOfLineSequence::Lf);

        assert_eq!(model.might_contain_rtl(), before);

        model.apply_edits(edits);
        assert_eq!(model.might_contain_rtl(), after);
        model.dispose();
    }

    #[test]
    fn start_with_rtl_insert_ltr() {
        let _guard = ensure_no_disposables_are_leaked_in_test_suite();
        test_apply_edits(&["Hello,\nזוהי עובדה מבוססת שדעתו"], &[edit_op(1, 1, 1, 1, &["hello"])], true, true);
    }

    #[test]
    fn start_with_rtl_delete_rtl() {
        let _guard = ensure_no_disposables_are_leaked_in_test_suite();
        test_apply_edits(&["Hello,\nזוהי עובדה מבוססת שדעתו"], &[edit_op(1, 1, 10, 10, &[""])], true, true);
    }

    #[test]
    fn start_with_rtl_insert_rtl() {
        let _guard = ensure_no_disposables_are_leaked_in_test_suite();
        test_apply_edits(
            &["Hello,\nזוהי עובדה מבוססת שדעתו"],
            &[edit_op(1, 1, 1, 1, &["هناك حقيقة مثبتة منذ زمن طويل"])],
            true,
            true,
        );
    }

    #[test]
    fn start_with_ltr_insert_ltr() {
        let _guard = ensure_no_disposables_are_leaked_in_test_suite();
        test_apply_edits(&["Hello,\nworld!"], &[edit_op(1, 1, 1, 1, &["hello"])], false, false);
    }

    #[test]
    fn start_with_ltr_insert_rtl_1() {
        let _guard = ensure_no_disposables_are_leaked_in_test_suite();
        test_apply_edits(&["Hello,\nworld!"], &[edit_op(1, 1, 1, 1, &["هناك حقيقة مثبتة منذ زمن طويل"])], false, true);
    }

    #[test]
    fn start_with_ltr_insert_rtl_2() {
        let _guard = ensure_no_disposables_are_leaked_in_test_suite();
        test_apply_edits(&["Hello,\nworld!"], &[edit_op(1, 1, 1, 1, &["זוהי עובדה מבוססת שדעתו"])], false, true);
    }
}

mod apply_edits_updates_might_contain_non_basic_ascii {
    use super::*;

    /// `testApplyEdits(original, edits, before, after)`.
    fn test_apply_edits(original: &[&str], edits: &[SingleEditOperation], before: bool, after: bool) {
        let model = create_text_model(&original.join("\n"));
        model.set_eol(EndOfLineSequence::Lf);

        assert_eq!(model.might_contain_non_basic_ascii(), before);

        model.apply_edits(edits);
        assert_eq!(model.might_contain_non_basic_ascii(), after);
        model.dispose();
    }

    #[test]
    fn start_with_non_ascii_insert_ascii() {
        let _guard = ensure_no_disposables_are_leaked_in_test_suite();
        test_apply_edits(&["Hello,\nZürich"], &[edit_op(1, 1, 1, 1, &["hello", "second line"])], true, true);
    }

    #[test]
    fn start_with_non_ascii_delete_non_ascii() {
        let _guard = ensure_no_disposables_are_leaked_in_test_suite();
        test_apply_edits(&["Hello,\nZürich"], &[edit_op(1, 1, 10, 10, &[""])], true, true);
    }

    #[test]
    fn start_with_non_ascii_insert_non_ascii() {
        let _guard = ensure_no_disposables_are_leaked_in_test_suite();
        test_apply_edits(&["Hello,\nZürich"], &[edit_op(1, 1, 1, 1, &["Zürich"])], true, true);
    }

    #[test]
    fn start_with_ascii_insert_ascii() {
        let _guard = ensure_no_disposables_are_leaked_in_test_suite();
        test_apply_edits(&["Hello,\nworld!"], &[edit_op(1, 1, 1, 1, &["hello", "second line"])], false, false);
    }

    #[test]
    fn start_with_ascii_insert_non_ascii() {
        let _guard = ensure_no_disposables_are_leaked_in_test_suite();
        test_apply_edits(&["Hello,\nworld!"], &[edit_op(1, 1, 1, 1, &["Zürich", "Zürich"])], false, true);
    }
}

mod apply_edits {
    use super::*;

    /// `high-low surrogates 1`
    #[test]
    fn high_low_surrogates_1() {
        let _guard = ensure_no_disposables_are_leaked_in_test_suite();
        test_apply_edits_with_synced_models(
            &["📚some", "very nice", "text"],
            &[
                edit_op(1, 2, 1, 2, &["a"]),
            ],
            &["a📚some", "very nice", "text"],
            true,
        );
    }

    /// `high-low surrogates 2`
    #[test]
    fn high_low_surrogates_2() {
        let _guard = ensure_no_disposables_are_leaked_in_test_suite();
        test_apply_edits_with_synced_models(
            &["📚some", "very nice", "text"],
            &[
                edit_op(1, 2, 1, 3, &["a"]),
            ],
            &["asome", "very nice", "text"],
            true,
        );
    }

    /// `high-low surrogates 3`
    #[test]
    fn high_low_surrogates_3() {
        let _guard = ensure_no_disposables_are_leaked_in_test_suite();
        test_apply_edits_with_synced_models(
            &["📚some", "very nice", "text"],
            &[
                edit_op(1, 1, 1, 2, &["a"]),
            ],
            &["asome", "very nice", "text"],
            true,
        );
    }

    /// `high-low surrogates 4`
    #[test]
    fn high_low_surrogates_4() {
        let _guard = ensure_no_disposables_are_leaked_in_test_suite();
        test_apply_edits_with_synced_models(
            &["📚some", "very nice", "text"],
            &[
                edit_op(1, 1, 1, 3, &["a"]),
            ],
            &["asome", "very nice", "text"],
            true,
        );
    }

    /// `Bug 19872: Undo is funky`
    #[test]
    fn bug_19872_undo_is_funky() {
        let _guard = ensure_no_disposables_are_leaked_in_test_suite();
        test_apply_edits_with_synced_models(
            &["something", " A", "", " B", "something else"],
            &[
                edit_op(2, 1, 2, 2, &[""]),
                edit_op(3, 1, 4, 2, &[""]),
            ],
            &["something", "A", "B", "something else"],
            false,
        );
    }

    /// `Bug 19872: Undo is funky (2)`
    #[test]
    fn bug_19872_undo_is_funky_2() {
        let _guard = ensure_no_disposables_are_leaked_in_test_suite();
        test_apply_edits_with_synced_models(
            &["something", "A", "B", "something else"],
            &[
                edit_op(2, 1, 2, 1, &[" "]),
                edit_op(3, 1, 3, 1, &["", " "]),
            ],
            &["something", " A", "", " B", "something else"],
            false,
        );
    }

    /// `insert empty text`
    #[test]
    fn insert_empty_text() {
        let _guard = ensure_no_disposables_are_leaked_in_test_suite();
        test_apply_edits_with_synced_models(
            &["My First Line", "\t\tMy Second Line", "    Third Line", "", "1"],
            &[
                edit_op(1, 1, 1, 1, &[""]),
            ],
            &["My First Line", "\t\tMy Second Line", "    Third Line", "", "1"],
            false,
        );
    }

    /// `last op is no-op`
    #[test]
    fn last_op_is_no_op() {
        let _guard = ensure_no_disposables_are_leaked_in_test_suite();
        test_apply_edits_with_synced_models(
            &["My First Line", "\t\tMy Second Line", "    Third Line", "", "1"],
            &[
                edit_op(1, 1, 1, 2, &[""]),
                edit_op(4, 1, 4, 1, &[""]),
            ],
            &["y First Line", "\t\tMy Second Line", "    Third Line", "", "1"],
            false,
        );
    }

    /// `insert text without newline 1`
    #[test]
    fn insert_text_without_newline_1() {
        let _guard = ensure_no_disposables_are_leaked_in_test_suite();
        test_apply_edits_with_synced_models(
            &["My First Line", "\t\tMy Second Line", "    Third Line", "", "1"],
            &[
                edit_op(1, 1, 1, 1, &["foo "]),
            ],
            &["foo My First Line", "\t\tMy Second Line", "    Third Line", "", "1"],
            false,
        );
    }

    /// `insert text without newline 2`
    #[test]
    fn insert_text_without_newline_2() {
        let _guard = ensure_no_disposables_are_leaked_in_test_suite();
        test_apply_edits_with_synced_models(
            &["My First Line", "\t\tMy Second Line", "    Third Line", "", "1"],
            &[
                edit_op(1, 3, 1, 3, &[" foo"]),
            ],
            &["My foo First Line", "\t\tMy Second Line", "    Third Line", "", "1"],
            false,
        );
    }

    /// `insert one newline`
    #[test]
    fn insert_one_newline() {
        let _guard = ensure_no_disposables_are_leaked_in_test_suite();
        test_apply_edits_with_synced_models(
            &["My First Line", "\t\tMy Second Line", "    Third Line", "", "1"],
            &[
                edit_op(1, 4, 1, 4, &["", ""]),
            ],
            &["My ", "First Line", "\t\tMy Second Line", "    Third Line", "", "1"],
            false,
        );
    }

    /// `insert text with one newline`
    #[test]
    fn insert_text_with_one_newline() {
        let _guard = ensure_no_disposables_are_leaked_in_test_suite();
        test_apply_edits_with_synced_models(
            &["My First Line", "\t\tMy Second Line", "    Third Line", "", "1"],
            &[
                edit_op(1, 3, 1, 3, &[" new line", "No longer"]),
            ],
            &["My new line", "No longer First Line", "\t\tMy Second Line", "    Third Line", "", "1"],
            false,
        );
    }

    /// `insert text with two newlines`
    #[test]
    fn insert_text_with_two_newlines() {
        let _guard = ensure_no_disposables_are_leaked_in_test_suite();
        test_apply_edits_with_synced_models(
            &["My First Line", "\t\tMy Second Line", "    Third Line", "", "1"],
            &[
                edit_op(1, 3, 1, 3, &[" new line", "One more line in the middle", "No longer"]),
            ],
            &["My new line", "One more line in the middle", "No longer First Line", "\t\tMy Second Line", "    Third Line", "", "1"],
            false,
        );
    }

    /// `insert text with many newlines`
    #[test]
    fn insert_text_with_many_newlines() {
        let _guard = ensure_no_disposables_are_leaked_in_test_suite();
        test_apply_edits_with_synced_models(
            &["My First Line", "\t\tMy Second Line", "    Third Line", "", "1"],
            &[
                edit_op(1, 3, 1, 3, &["", "", "", "", ""]),
            ],
            &["My", "", "", "", " First Line", "\t\tMy Second Line", "    Third Line", "", "1"],
            false,
        );
    }

    /// `insert multiple newlines`
    #[test]
    fn insert_multiple_newlines() {
        let _guard = ensure_no_disposables_are_leaked_in_test_suite();
        test_apply_edits_with_synced_models(
            &["My First Line", "\t\tMy Second Line", "    Third Line", "", "1"],
            &[
                edit_op(1, 3, 1, 3, &["", "", "", "", ""]),
                edit_op(3, 15, 3, 15, &["a", "b"]),
            ],
            &["My", "", "", "", " First Line", "\t\tMy Second Line", "    Third Linea", "b", "", "1"],
            false,
        );
    }

    /// `delete empty text`
    #[test]
    fn delete_empty_text() {
        let _guard = ensure_no_disposables_are_leaked_in_test_suite();
        test_apply_edits_with_synced_models(
            &["My First Line", "\t\tMy Second Line", "    Third Line", "", "1"],
            &[
                edit_op(1, 1, 1, 1, &[""]),
            ],
            &["My First Line", "\t\tMy Second Line", "    Third Line", "", "1"],
            false,
        );
    }

    /// `delete text from one line`
    #[test]
    fn delete_text_from_one_line() {
        let _guard = ensure_no_disposables_are_leaked_in_test_suite();
        test_apply_edits_with_synced_models(
            &["My First Line", "\t\tMy Second Line", "    Third Line", "", "1"],
            &[
                edit_op(1, 1, 1, 2, &[""]),
            ],
            &["y First Line", "\t\tMy Second Line", "    Third Line", "", "1"],
            false,
        );
    }

    /// `delete text from one line 2`
    #[test]
    fn delete_text_from_one_line_2() {
        let _guard = ensure_no_disposables_are_leaked_in_test_suite();
        test_apply_edits_with_synced_models(
            &["My First Line", "\t\tMy Second Line", "    Third Line", "", "1"],
            &[
                edit_op(1, 1, 1, 3, &["a"]),
            ],
            &["a First Line", "\t\tMy Second Line", "    Third Line", "", "1"],
            false,
        );
    }

    /// `delete all text from a line`
    #[test]
    fn delete_all_text_from_a_line() {
        let _guard = ensure_no_disposables_are_leaked_in_test_suite();
        test_apply_edits_with_synced_models(
            &["My First Line", "\t\tMy Second Line", "    Third Line", "", "1"],
            &[
                edit_op(1, 1, 1, 14, &[""]),
            ],
            &["", "\t\tMy Second Line", "    Third Line", "", "1"],
            false,
        );
    }

    /// `delete text from two lines`
    #[test]
    fn delete_text_from_two_lines() {
        let _guard = ensure_no_disposables_are_leaked_in_test_suite();
        test_apply_edits_with_synced_models(
            &["My First Line", "\t\tMy Second Line", "    Third Line", "", "1"],
            &[
                edit_op(1, 4, 2, 6, &[""]),
            ],
            &["My Second Line", "    Third Line", "", "1"],
            false,
        );
    }

    /// `delete text from many lines`
    #[test]
    fn delete_text_from_many_lines() {
        let _guard = ensure_no_disposables_are_leaked_in_test_suite();
        test_apply_edits_with_synced_models(
            &["My First Line", "\t\tMy Second Line", "    Third Line", "", "1"],
            &[
                edit_op(1, 4, 3, 5, &[""]),
            ],
            &["My Third Line", "", "1"],
            false,
        );
    }

    /// `delete everything`
    #[test]
    fn delete_everything() {
        let _guard = ensure_no_disposables_are_leaked_in_test_suite();
        test_apply_edits_with_synced_models(
            &["My First Line", "\t\tMy Second Line", "    Third Line", "", "1"],
            &[
                edit_op(1, 1, 5, 2, &[""]),
            ],
            &[""],
            false,
        );
    }

    /// `two unrelated edits`
    #[test]
    fn two_unrelated_edits() {
        let _guard = ensure_no_disposables_are_leaked_in_test_suite();
        test_apply_edits_with_synced_models(
            &["My First Line", "\t\tMy Second Line", "    Third Line", "", "123"],
            &[
                edit_op(2, 1, 2, 3, &["\t"]),
                edit_op(3, 1, 3, 5, &[""]),
            ],
            &["My First Line", "\tMy Second Line", "Third Line", "", "123"],
            false,
        );
    }

    /// `two edits on one line`
    #[test]
    fn two_edits_on_one_line() {
        let _guard = ensure_no_disposables_are_leaked_in_test_suite();
        test_apply_edits_with_synced_models(
            &["\t\tfirst\t    ", "\t\tsecond line", "\tthird line", "fourth line", "\t\t<!@#fifth#@!>\t\t"],
            &[
                edit_op(5, 3, 5, 7, &[""]),
                edit_op(5, 12, 5, 16, &[""]),
            ],
            &["\t\tfirst\t    ", "\t\tsecond line", "\tthird line", "fourth line", "\t\tfifth\t\t"],
            false,
        );
    }

    /// `many edits`
    #[test]
    fn many_edits() {
        let _guard = ensure_no_disposables_are_leaked_in_test_suite();
        test_apply_edits_with_synced_models(
            &["{\"x\" : 1}"],
            &[
                edit_op(1, 2, 1, 2, &["\n  "]),
                edit_op(1, 5, 1, 6, &[""]),
                edit_op(1, 9, 1, 9, &["\n"]),
            ],
            &["{", "  \"x\": 1", "}"],
            false,
        );
    }

    /// `many edits reversed`
    #[test]
    fn many_edits_reversed() {
        let _guard = ensure_no_disposables_are_leaked_in_test_suite();
        test_apply_edits_with_synced_models(
            &["{", "  \"x\": 1", "}"],
            &[
                edit_op(1, 2, 2, 3, &[""]),
                edit_op(2, 6, 2, 6, &[" "]),
                edit_op(2, 9, 3, 1, &[""]),
            ],
            &["{\"x\" : 1}"],
            false,
        );
    }

    /// `replacing newlines 1`
    #[test]
    fn replacing_newlines_1() {
        let _guard = ensure_no_disposables_are_leaked_in_test_suite();
        test_apply_edits_with_synced_models(
            &["{", "\"a\": true,", "", "\"b\": true", "}"],
            &[
                edit_op(1, 2, 2, 1, &["", "\t"]),
                edit_op(2, 11, 4, 1, &["", "\t"]),
            ],
            &["{", "\t\"a\": true,", "\t\"b\": true", "}"],
            false,
        );
    }

    /// `replacing newlines 2`
    #[test]
    fn replacing_newlines_2() {
        let _guard = ensure_no_disposables_are_leaked_in_test_suite();
        test_apply_edits_with_synced_models(
            &["some text", "some more text", "now comes an empty line", "", "after empty line", "and the last line"],
            &[
                edit_op(1, 5, 3, 1, &[" text", "some more text", "some more text"]),
                edit_op(3, 2, 4, 1, &["o more lines", "asd", "asd", "asd"]),
                edit_op(5, 1, 5, 6, &["zzzzzzzz"]),
                edit_op(5, 11, 6, 16, &["1", "2", "3", "4"]),
            ],
            &["some text", "some more text", "some more textno more lines", "asd", "asd", "asd", "zzzzzzzz empt1", "2", "3", "4ne"],
            false,
        );
    }

    /// `advanced 1`
    #[test]
    fn advanced_1() {
        let _guard = ensure_no_disposables_are_leaked_in_test_suite();
        test_apply_edits_with_synced_models(
            &[" {       \"d\": [", "             null", "        ] /*comment*/", "        ,\"e\": /*comment*/ [null] }"],
            &[
                edit_op(1, 1, 1, 2, &[""]),
                edit_op(1, 3, 1, 10, &["", "  "]),
                edit_op(1, 16, 2, 14, &["", "    "]),
                edit_op(2, 18, 3, 9, &["", "  "]),
                edit_op(3, 22, 4, 9, &[""]),
                edit_op(4, 10, 4, 10, &["", "  "]),
                edit_op(4, 28, 4, 28, &["", "    "]),
                edit_op(4, 32, 4, 32, &["", "  "]),
                edit_op(4, 33, 4, 34, &["", ""]),
            ],
            &["{", "  \"d\": [", "    null", "  ] /*comment*/,", "  \"e\": /*comment*/ [", "    null", "  ]", "}"],
            false,
        );
    }

    /// `advanced simplified`
    #[test]
    fn advanced_simplified() {
        let _guard = ensure_no_disposables_are_leaked_in_test_suite();
        test_apply_edits_with_synced_models(
            &["   abc", " ,def"],
            &[
                edit_op(1, 1, 1, 4, &[""]),
                edit_op(1, 7, 2, 2, &[""]),
                edit_op(2, 3, 2, 3, &["", ""]),
            ],
            &["abc,", "def"],
            false,
        );
    }

    /// `issue #144`
    #[test]
    fn issue_144() {
        let _guard = ensure_no_disposables_are_leaked_in_test_suite();
        test_apply_edits_with_synced_models(
            &["package caddy", "", "func main() {", "\tfmt.Println(\"Hello World! :)\")", "}", ""],
            &[
                edit_op(1, 1, 6, 1, &["package caddy", "", "import \"fmt\"", "", "func main() {", "\tfmt.Println(\"Hello World! :)\")", "}", ""]),
            ],
            &["package caddy", "", "import \"fmt\"", "", "func main() {", "\tfmt.Println(\"Hello World! :)\")", "}", ""],
            false,
        );
    }

    /// `issue #2586 Replacing selected end-of-line with newline locks up the document`
    #[test]
    fn issue_2586_replacing_selected_end_of_line_with_newline_locks_up_the_document() {
        let _guard = ensure_no_disposables_are_leaked_in_test_suite();
        test_apply_edits_with_synced_models(
            &["something", "interesting"],
            &[
                edit_op(1, 10, 2, 1, &["", ""]),
            ],
            &["something", "interesting"],
            false,
        );
    }

    /// `issue #3980`
    #[test]
    fn issue_3980() {
        let _guard = ensure_no_disposables_are_leaked_in_test_suite();
        test_apply_edits_with_synced_models(
            &["class A {", "    someProperty = false;", "    someMethod() {", "    this.someMethod();", "    }", "}"],
            &[
                edit_op(1, 8, 1, 9, &["", ""]),
                edit_op(3, 17, 3, 18, &["", ""]),
                edit_op(3, 18, 3, 18, &["    "]),
                edit_op(4, 5, 4, 5, &["    "]),
            ],
            &["class A", "{", "    someProperty = false;", "    someMethod()", "    {", "        this.someMethod();", "    }", "}"],
            false,
        );
    }

    /// `testApplyEditsFails(original, edits)`.
    fn test_apply_edits_fails(original: &[&str], edits: &[SingleEditOperation]) {
        let model = create_text_model(&original.join("\n"));

        let has_thrown = errors::catch(|| model.apply_edits(edits)).is_err();
        assert!(has_thrown, "expected model.applyEdits to fail.");

        model.dispose();
    }

    /// `touching edits: two inserts at the same position`
    #[test]
    fn touching_edits_two_inserts_at_the_same_position() {
        let _guard = ensure_no_disposables_are_leaked_in_test_suite();
        test_apply_edits_with_synced_models(
            &["hello world"],
            &[
                edit_op(1, 1, 1, 1, &["a"]),
                edit_op(1, 1, 1, 1, &["b"]),
            ],
            &["abhello world"],
            false,
        );
    }

    /// `touching edits: insert and replace touching`
    #[test]
    fn touching_edits_insert_and_replace_touching() {
        let _guard = ensure_no_disposables_are_leaked_in_test_suite();
        test_apply_edits_with_synced_models(
            &["hello world"],
            &[
                edit_op(1, 1, 1, 1, &["b"]),
                edit_op(1, 1, 1, 3, &["ab"]),
            ],
            &["babllo world"],
            false,
        );
    }

    /// `overlapping edits: two overlapping replaces`
    #[test]
    fn overlapping_edits_two_overlapping_replaces() {
        let _guard = ensure_no_disposables_are_leaked_in_test_suite();
        test_apply_edits_fails(&["hello world"], &[edit_op(1, 1, 1, 2, &["b"]), edit_op(1, 1, 1, 3, &["ab"])]);
    }

    /// `overlapping edits: two overlapping deletes`
    #[test]
    fn overlapping_edits_two_overlapping_deletes() {
        let _guard = ensure_no_disposables_are_leaked_in_test_suite();
        test_apply_edits_fails(&["hello world"], &[edit_op(1, 1, 1, 2, &[""]), edit_op(1, 1, 1, 3, &[""])]);
    }

    /// `touching edits: two touching replaces`
    #[test]
    fn touching_edits_two_touching_replaces() {
        let _guard = ensure_no_disposables_are_leaked_in_test_suite();
        test_apply_edits_with_synced_models(
            &["hello world"],
            &[
                edit_op(1, 1, 1, 2, &["H"]),
                edit_op(1, 2, 1, 3, &["E"]),
            ],
            &["HEllo world"],
            false,
        );
    }

    /// `touching edits: two touching deletes`
    #[test]
    fn touching_edits_two_touching_deletes() {
        let _guard = ensure_no_disposables_are_leaked_in_test_suite();
        test_apply_edits_with_synced_models(
            &["hello world"],
            &[
                edit_op(1, 1, 1, 2, &[""]),
                edit_op(1, 2, 1, 3, &[""]),
            ],
            &["llo world"],
            false,
        );
    }

    /// `touching edits: insert and replace`
    #[test]
    fn touching_edits_insert_and_replace() {
        let _guard = ensure_no_disposables_are_leaked_in_test_suite();
        test_apply_edits_with_synced_models(
            &["hello world"],
            &[
                edit_op(1, 1, 1, 1, &["H"]),
                edit_op(1, 1, 1, 3, &["e"]),
            ],
            &["Hello world"],
            false,
        );
    }

    /// `touching edits: replace and insert`
    #[test]
    fn touching_edits_replace_and_insert() {
        let _guard = ensure_no_disposables_are_leaked_in_test_suite();
        test_apply_edits_with_synced_models(
            &["hello world"],
            &[
                edit_op(1, 1, 1, 3, &["H"]),
                edit_op(1, 3, 1, 3, &["e"]),
            ],
            &["Hello world"],
            false,
        );
    }

    /// `change while emitting events 1`
    #[test]
    fn change_while_emitting_events_1() {
        let _guard = ensure_no_disposables_are_leaked_in_test_suite();
        let disposable: RefCell<Option<DisposableRef>> = RefCell::new(None);
        assert_synced_models(
            "Hello",
            |model, assert_mirror_models| {
                model.apply_edits(&[op(Range::new(1, 6, 1, 6), Some(" world!"))]);

                assert_mirror_models();
            },
            Some(&|model| {
                let is_first_time = Cell::new(true);
                let model_in_listener = model.clone();
                *disposable.borrow_mut() = Some(model.on_did_change_content().listen(move |_| {
                    if !is_first_time.get() {
                        return;
                    }
                    is_first_time.set(false);

                    model_in_listener.apply_edits(&[op(Range::new(1, 13, 1, 13), Some(" How are you?"))]);
                }));
            }),
        );
        disposable.borrow().as_ref().expect("o setup registra o ouvinte").dispose();
    }

    /// `change while emitting events 2`
    #[test]
    fn change_while_emitting_events_2() {
        let _guard = ensure_no_disposables_are_leaked_in_test_suite();
        let disposable: RefCell<Option<DisposableRef>> = RefCell::new(None);
        assert_synced_models(
            "Hello",
            |model, assert_mirror_models| {
                model.apply_edits(&[op(Range::new(1, 6, 1, 6), Some(" world!"))]);

                assert_mirror_models();
            },
            Some(&|model| {
                let is_first_time = Cell::new(true);
                let model_in_listener = model.clone();
                *disposable.borrow_mut() = Some(model.on_did_change_content().listen(move |_e| {
                    if !is_first_time.get() {
                        return;
                    }
                    is_first_time.set(false);

                    model_in_listener.apply_edits(&[op(Range::new(1, 13, 1, 13), Some(" How are you?"))]);
                }));
            }),
        );
        disposable.borrow().as_ref().expect("o setup registra o ouvinte").dispose();
    }

    /// `issue #1580: Changes in line endings are not correctly reflected in the extension host, leading
    /// to invalid offsets sent to external refactoring tools`
    #[test]
    fn issue_1580_changes_in_line_endings_are_not_correctly_reflected_in_the_extension_host() {
        let _guard = ensure_no_disposables_are_leaked_in_test_suite();
        let model = create_text_model("Hello\nWorld!");
        assert_eq!(model.get_eol(), Eol::Lf);

        let mirror_model2 = Rc::new(RefCell::new(MirrorTextModel::new(
            None,
            model.get_lines_content(),
            model.get_eol(),
            model.get_version_id(),
        )));
        let mirror_model2_prev_version_id = Rc::new(Cell::new(model.get_version_id()));

        let disposable = {
            let mirror_model2 = mirror_model2.clone();
            let mirror_model2_prev_version_id = mirror_model2_prev_version_id.clone();
            model.on_did_change_content().listen(move |e| {
                let version_id = e.version_id;
                if version_id < mirror_model2_prev_version_id.get() {
                    eprintln!("Model version id did not advance between edits (2)");
                }
                mirror_model2_prev_version_id.set(version_id);
                mirror_model2.borrow_mut().on_events(e);
            })
        };

        let assert_mirror_models = || {
            assert_eq!(mirror_model2.borrow().get_text(), model.get_value(), "mirror model 2 text OK");
            assert_eq!(mirror_model2.borrow().version(), model.get_version_id(), "mirror model 2 version OK");
        };

        model.set_eol(EndOfLineSequence::CrLf);
        assert_mirror_models();

        disposable.dispose();
        model.dispose();
        mirror_model2.borrow_mut().dispose();
    }

    /// `issue #47733: Undo mangles unicode characters`
    #[test]
    fn issue_47733_undo_mangles_unicode_characters() {
        let _guard = ensure_no_disposables_are_leaked_in_test_suite();
        let model = create_text_model("'👁'");

        model.apply_edits(&[op(Range::new(1, 1, 1, 1), Some("\"")), op(Range::new(1, 2, 1, 2), Some("\""))]);

        assert_eq!(model.get_value_with(EndOfLinePreference::Lf, false), "\"'\"👁'");

        assert_eq!(model.validate_range(&Range::new(1, 3, 1, 4)), Range::new(1, 3, 1, 4));

        model.apply_edits(&[op(Range::new(1, 1, 1, 2), None), op(Range::new(1, 3, 1, 4), None)]);

        assert_eq!(model.get_value_with(EndOfLinePreference::Lf, false), "'👁'");

        model.dispose();
    }

    /// `issue #48741: Broken undo stack with move lines up with multiple cursors`
    #[test]
    fn issue_48741_broken_undo_stack_with_move_lines_up_with_multiple_cursors() {
        let _guard = ensure_no_disposables_are_leaked_in_test_suite();
        let model = create_text_model(&["line1", "line2", "line3", ""].join("\n"));

        let undo_edits = model.apply_edits_computing_undo(&[
            op(Range::new(4, 1, 4, 1), Some("line3")),
            op(Range::new(3, 1, 3, 6), None),
            op(Range::new(2, 1, 3, 1), None),
            op(Range::new(3, 6, 3, 6), Some("\nline2")),
        ]);

        model.apply_edits(&undo_edits.iter().map(to_single_edit).collect::<Vec<_>>());

        assert_eq!(model.get_value(), "line1\nline2\nline3\n");

        model.dispose();
    }
}

mod crlf_edit_normalization {
    use super::*;

    /// `edit ending with \r followed by \n in buffer should strip trailing \r`
    #[test]
    fn edit_ending_with_cr_followed_by_lf_in_buffer_should_strip_trailing_cr() {
        let _guard = ensure_no_disposables_are_leaked_in_test_suite();
        // Documento: "abc\r\ndef\r\n"
        // Edição: troca o intervalo (1,1)-(1,4) "abc" por "xyz\r"
        // O \r no fim do texto novo deve ser removido, porque o caractere seguinte é \n
        let model = create_text_model("abc\r\ndef\r\n");
        model.set_eol(EndOfLineSequence::CrLf);

        assert_eq!(model.get_eol(), Eol::CrLf);
        assert_eq!(model.get_line_count(), 3);
        assert_eq!(model.get_line_content(1), "abc");
        assert_eq!(model.get_line_content(2), "def");

        model.apply_edits(&[op(Range::new(1, 1, 1, 4), Some("xyz\r"))]);

        // O \r final deve ser removido: fica "xyz", não "xyz\r"
        assert_eq!(model.get_line_content(1), "xyz");
        assert_eq!(model.get_line_content(2), "def");
        assert_eq!(model.get_line_count(), 3);

        model.dispose();
    }

    /// `edit ending with \r\n should NOT be modified`
    #[test]
    fn edit_ending_with_crlf_should_not_be_modified() {
        let _guard = ensure_no_disposables_are_leaked_in_test_suite();
        // Documento: "abc\r\ndef\r\n"
        // Edição: troca o intervalo (1,1)-(1,4) "abc" por "xyz\r\n"
        // É um CRLF de verdade, então não deve ser alterado
        let model = create_text_model("abc\r\ndef\r\n");
        model.set_eol(EndOfLineSequence::CrLf);

        model.apply_edits(&[op(Range::new(1, 1, 1, 4), Some("xyz\r\n"))]);

        // Deve acrescentar uma linha
        assert_eq!(model.get_line_content(1), "xyz");
        assert_eq!(model.get_line_content(2), "");
        assert_eq!(model.get_line_content(3), "def");
        assert_eq!(model.get_line_count(), 4);

        model.dispose();
    }

    /// `edit ending with \r NOT followed by \n should NOT be modified`
    #[test]
    fn edit_ending_with_cr_not_followed_by_lf_should_not_be_modified() {
        let _guard = ensure_no_disposables_are_leaked_in_test_suite();
        // Documento: "abcdef" (sem quebra de linha depois)
        // Edição: troca o intervalo (1,1)-(1,4) "abc" por "xyz\r"
        // Como não há \n depois do intervalo, o \r fica
        let model = create_text_model("abcdef");
        model.set_eol(EndOfLineSequence::CrLf);

        model.apply_edits(&[op(Range::new(1, 1, 1, 4), Some("xyz\r"))]);

        // O \r deve criar uma linha nova, porque o buffer normaliza o fim de linha
        // Na verdade, como o buffer usa CRLF, o \r sozinho vira \r\n
        assert_eq!(model.get_line_count(), 2);

        model.dispose();
    }

    /// `edit in LF buffer should NOT strip trailing \r`
    #[test]
    fn edit_in_lf_buffer_should_not_strip_trailing_cr() {
        let _guard = ensure_no_disposables_are_leaked_in_test_suite();
        // Documento com LF: "abc\ndef\n"
        // Edição: troca o intervalo (1,1)-(1,4) "abc" por "xyz\r"
        // Como o buffer é LF, não há tratamento especial
        let model = create_text_model("abc\ndef\n");
        model.set_eol(EndOfLineSequence::Lf);

        assert_eq!(model.get_eol(), Eol::Lf);
        assert_eq!(model.get_line_count(), 3);

        model.apply_edits(&[op(Range::new(1, 1, 1, 4), Some("xyz\r"))]);

        // O \r vira \n (o fim de linha do buffer)
        assert_eq!(model.get_line_count(), 4);

        model.dispose();
    }

    /// `LSP include sorting scenario - edit ending with \r should be normalized`
    #[test]
    fn lsp_include_sorting_scenario_edit_ending_with_cr_should_be_normalized() {
        let _guard = ensure_no_disposables_are_leaked_in_test_suite();
        // O cenário real da issue
        // Documento: "#include \"a.h\"\r\n#include \"c.h\"\r\n#include \"b.h\"\r\n"
        // Edição: troca as linhas 1 a 3 pelos includes reordenados, terminando em \r
        let model = create_text_model("#include \"a.h\"\r\n#include \"c.h\"\r\n#include \"b.h\"\r\n");
        model.set_eol(EndOfLineSequence::CrLf);

        assert_eq!(model.get_eol(), Eol::CrLf);
        assert_eq!(model.get_line_count(), 4);
        assert_eq!(model.get_line_content(1), "#include \"a.h\"");
        assert_eq!(model.get_line_content(2), "#include \"c.h\"");
        assert_eq!(model.get_line_content(3), "#include \"b.h\"");

        // Edição: troca o intervalo (1,1)-(3,16) por um texto que termina em \r
        // O intervalo cobre: #include "a.h"\r\n#include "c.h"\r\n#include "b.h"
        // Nota: a coluna 16 da linha 3 fica depois do último caractere "h", antes do \r\n
        model.apply_edits(&[op(
            Range::new(1, 1, 3, 16),
            Some("#include \"a.h\"\r\n#include \"b.h\"\r\n#include \"c.h\"\r"),
        )]);

        // O \r final deve ser removido, porque o caractere depois do intervalo é \n
        assert_eq!(model.get_line_count(), 4);
        assert_eq!(model.get_line_content(1), "#include \"a.h\"");
        assert_eq!(model.get_line_content(2), "#include \"b.h\"");
        assert_eq!(model.get_line_content(3), "#include \"c.h\"");
        assert_eq!(model.get_line_content(4), "");

        model.dispose();
    }
}
