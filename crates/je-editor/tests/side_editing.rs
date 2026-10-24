//! Porte de `src/vs/editor/test/browser/commands/sideEditing.test.ts` (suítes `Editor Side Editing - collapsed
//! selection` e `SideEditing`), caso a caso e na mesma ordem.
//!
//! Notas de tradução:
//! - `ensureNoDisposablesAreLeakedInTestSuite()` vira o `LeakGuard` criado no começo de `test_command` e de
//!   `run_single_test`, os dois únicos caminhos que cada caso percorre.
//! - `withTestCodeEditor(lines, {}, (editor, viewModel) => ...)` é `with_test_code_editor` de
//!   `tests/support/test_code_editor.rs` (as opções `{}` são o `Default`).
//! - `viewModel.setSelections('tests', selections)` é `set_selections(Some("tests"), ..., CursorChangeReason::NotSet)`
//!   (o `reason` omitido do original vale `NotSet`).
//! - `new Selection(...)` dentro das tabelas de expectativa é a abreviação `sel`.
//! - `EditOperation.replace(new Selection(...), 'x')` passa o `Range` da seleção (`*sel(...)`), o que o `Selection`
//!   do original, subclasse de `Range`, faz sem conversão.

#[path = "../../je-base/tests/common/mod.rs"]
mod common;
mod support;

use common::ensure_no_disposables_are_leaked_in_test_suite;
use je_base::JsString;
use je_editor::code_editor_widget::ICodeEditor;
use je_editor::cursor_events::CursorChangeReason;
use je_editor::edit_operation::{EditOperation, SingleEditOperation};
use je_editor::position::Position;
use je_editor::range::Range;
use je_editor::selection::Selection;
use support::test_code_editor::{TestCodeEditorInstantiationOptions, with_test_code_editor};

fn sel(start_line_number: i32, start_column: i32, end_line_number: i32, end_column: i32) -> Selection {
    Selection::new(start_line_number, start_column, end_line_number, end_column)
}

fn text(value: &str) -> Option<JsString> {
    Some(JsString::from(value))
}

fn test_command(
    lines: &[&str],
    selections: &[Selection],
    edits: Vec<SingleEditOperation>,
    expected_lines: &[&str],
    expected_selections: &[Selection],
) {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    with_test_code_editor(lines, TestCodeEditorInstantiationOptions::default(), |editor, view_model, _| {
        let model = editor.get_model().expect("o editor tem o modelo");

        view_model.set_selections(Some("tests"), selections, CursorChangeReason::NotSet);

        model.apply_edits(&edits);

        let expected: Vec<JsString> = expected_lines.iter().map(|line| JsString::from(*line)).collect();
        assert_eq!(model.get_lines_content(), expected);

        let actual_selections: Vec<String> = view_model.get_selections().iter().map(ToString::to_string).collect();
        let expected_selections: Vec<String> = expected_selections.iter().map(ToString::to_string).collect();
        assert_eq!(actual_selections, expected_selections);
    });
}

mod editor_side_editing_collapsed_selection {
    use super::*;

    #[test]
    fn replace_at_selection() {
        test_command(
            &["first", "second line", "third line", "fourth"],
            &[sel(1, 1, 1, 1)],
            vec![EditOperation::replace(*sel(1, 1, 1, 1), text("something "))],
            &["something first", "second line", "third line", "fourth"],
            &[sel(1, 11, 1, 11)],
        );
    }

    #[test]
    fn replace_at_selection_2() {
        test_command(
            &["first", "second line", "third line", "fourth"],
            &[sel(1, 1, 1, 6)],
            vec![EditOperation::replace(*sel(1, 1, 1, 6), text("something"))],
            &["something", "second line", "third line", "fourth"],
            &[sel(1, 1, 1, 10)],
        );
    }

    #[test]
    fn insert_at_selection() {
        test_command(
            &["first", "second line", "third line", "fourth"],
            &[sel(1, 1, 1, 1)],
            vec![EditOperation::insert(Position::new(1, 1), "something ")],
            &["something first", "second line", "third line", "fourth"],
            &[sel(1, 11, 1, 11)],
        );
    }

    #[test]
    fn insert_at_selection_sitting_on_max_column() {
        test_command(
            &["first", "second line", "third line", "fourth"],
            &[sel(1, 6, 1, 6)],
            vec![EditOperation::insert(Position::new(1, 6), " something\nnew ")],
            &["first something", "new ", "second line", "third line", "fourth"],
            &[sel(2, 5, 2, 5)],
        );
    }

    #[test]
    fn issue_3994_replace_on_top_of_selection() {
        test_command(
            &["$obj = New-Object \"system.col\""],
            &[sel(1, 30, 1, 30)],
            vec![EditOperation::replace_move(Range::new(1, 19, 1, 31), text("\"System.Collections\""))],
            &["$obj = New-Object \"System.Collections\""],
            &[sel(1, 39, 1, 39)],
        );
    }

    #[test]
    fn issue_15267_suggestion_that_adds_a_line_cursor_goes_to_the_wrong_line() {
        test_command(
            &[
                "package main",
                "",
                "import (",
                "\t\"fmt\"",
                ")",
                "",
                "func main(",
                "\tfmt.Println(strings.Con)",
                "}",
            ],
            &[sel(8, 25, 8, 25)],
            vec![EditOperation::replace_move(Range::new(5, 1, 5, 1), text("\t\"strings\"\n"))],
            &[
                "package main",
                "",
                "import (",
                "\t\"fmt\"",
                "\t\"strings\"",
                ")",
                "",
                "func main(",
                "\tfmt.Println(strings.Con)",
                "}",
            ],
            &[sel(9, 25, 9, 25)],
        );
    }

    #[test]
    fn issue_15236_selections_broke_after_deleting_text_using_vscode_text_editor_edit() {
        test_command(
            &["foofoofoo, foofoofoo, bar"],
            &[sel(1, 1, 1, 10), sel(1, 12, 1, 21)],
            vec![
                EditOperation::replace(Range::new(1, 1, 1, 10), text("")),
                EditOperation::replace(Range::new(1, 12, 1, 21), text("")),
            ],
            &[", , bar"],
            &[sel(1, 1, 1, 1), sel(1, 3, 1, 3)],
        );
    }
}

mod side_editing {
    use super::*;

    const LINES: [&str; 3] = ["My First Line", "My Second Line", "Third Line"];

    fn run_single_test(
        selection: Selection,
        edit_range: Range,
        edit_text: &str,
        edit_force_move_markers: bool,
        expected: Selection,
        msg: &str,
    ) {
        let _ds = ensure_no_disposables_are_leaked_in_test_suite();
        with_test_code_editor(LINES.join("\n"), TestCodeEditorInstantiationOptions::default(), |editor, view_model, _| {
            view_model.set_selections(Some("tests"), &[selection], CursorChangeReason::NotSet);
            editor
                .get_model()
                .expect("o editor tem o modelo")
                .apply_edits(&[SingleEditOperation::new(edit_range, text(edit_text), edit_force_move_markers)]);
            let actual = view_model.get_selection();
            assert_eq!(actual.to_string(), expected.to_string(), "{msg}");
        });
    }

    fn run_test(selection: Range, edit_range: Range, edit_text: &str, expected: [[Selection; 2]; 2]) {
        let sel1 = sel(selection.start_line_number, selection.start_column, selection.end_line_number, selection.end_column);
        run_single_test(sel1, edit_range, edit_text, false, expected[0][0], "0-0-regular-no-force");
        run_single_test(sel1, edit_range, edit_text, true, expected[1][0], "1-0-regular-force");

        // RTL selection
        let sel2 = sel(selection.end_line_number, selection.end_column, selection.start_line_number, selection.start_column);
        run_single_test(sel2, edit_range, edit_text, false, expected[0][1], "0-1-inverse-no-force");
        run_single_test(sel2, edit_range, edit_text, true, expected[1][1], "1-1-inverse-force");
    }

    mod insert {
        use super::*;

        mod collapsed_sel {
            use super::*;

            #[test]
            fn before() {
                run_test(
                    Range::new(1, 4, 1, 4),
                    Range::new(1, 3, 1, 3),
                    "xx",
                    [[sel(1, 6, 1, 6), sel(1, 6, 1, 6)], [sel(1, 6, 1, 6), sel(1, 6, 1, 6)]],
                );
            }

            #[test]
            fn equal() {
                run_test(
                    Range::new(1, 4, 1, 4),
                    Range::new(1, 4, 1, 4),
                    "xx",
                    [[sel(1, 6, 1, 6), sel(1, 6, 1, 6)], [sel(1, 6, 1, 6), sel(1, 6, 1, 6)]],
                );
            }

            #[test]
            fn after() {
                run_test(
                    Range::new(1, 4, 1, 4),
                    Range::new(1, 5, 1, 5),
                    "xx",
                    [[sel(1, 4, 1, 4), sel(1, 4, 1, 4)], [sel(1, 4, 1, 4), sel(1, 4, 1, 4)]],
                );
            }
        }

        mod non_collapsed_dec {
            use super::*;

            #[test]
            fn before() {
                run_test(
                    Range::new(1, 4, 1, 9),
                    Range::new(1, 3, 1, 3),
                    "xx",
                    [[sel(1, 6, 1, 11), sel(1, 11, 1, 6)], [sel(1, 6, 1, 11), sel(1, 11, 1, 6)]],
                );
            }

            #[test]
            fn start() {
                run_test(
                    Range::new(1, 4, 1, 9),
                    Range::new(1, 4, 1, 4),
                    "xx",
                    [[sel(1, 4, 1, 11), sel(1, 11, 1, 4)], [sel(1, 6, 1, 11), sel(1, 11, 1, 6)]],
                );
            }

            #[test]
            fn inside() {
                run_test(
                    Range::new(1, 4, 1, 9),
                    Range::new(1, 5, 1, 5),
                    "xx",
                    [[sel(1, 4, 1, 11), sel(1, 11, 1, 4)], [sel(1, 4, 1, 11), sel(1, 11, 1, 4)]],
                );
            }

            #[test]
            fn end() {
                run_test(
                    Range::new(1, 4, 1, 9),
                    Range::new(1, 9, 1, 9),
                    "xx",
                    [[sel(1, 4, 1, 11), sel(1, 11, 1, 4)], [sel(1, 4, 1, 11), sel(1, 11, 1, 4)]],
                );
            }

            #[test]
            fn after() {
                run_test(
                    Range::new(1, 4, 1, 9),
                    Range::new(1, 10, 1, 10),
                    "xx",
                    [[sel(1, 4, 1, 9), sel(1, 9, 1, 4)], [sel(1, 4, 1, 9), sel(1, 9, 1, 4)]],
                );
            }
        }
    }

    mod delete {
        use super::*;

        mod collapsed_dec {
            use super::*;

            #[test]
            fn edit_end_lt_range_start() {
                run_test(
                    Range::new(1, 4, 1, 4),
                    Range::new(1, 1, 1, 3),
                    "",
                    [[sel(1, 2, 1, 2), sel(1, 2, 1, 2)], [sel(1, 2, 1, 2), sel(1, 2, 1, 2)]],
                );
            }

            #[test]
            fn edit_end_le_range_start() {
                run_test(
                    Range::new(1, 4, 1, 4),
                    Range::new(1, 2, 1, 4),
                    "",
                    [[sel(1, 2, 1, 2), sel(1, 2, 1, 2)], [sel(1, 2, 1, 2), sel(1, 2, 1, 2)]],
                );
            }

            #[test]
            fn edit_start_lt_range_start_and_edit_end_gt_range_end() {
                run_test(
                    Range::new(1, 4, 1, 4),
                    Range::new(1, 3, 1, 5),
                    "",
                    [[sel(1, 3, 1, 3), sel(1, 3, 1, 3)], [sel(1, 3, 1, 3), sel(1, 3, 1, 3)]],
                );
            }

            #[test]
            fn edit_start_ge_range_end() {
                run_test(
                    Range::new(1, 4, 1, 4),
                    Range::new(1, 4, 1, 6),
                    "",
                    [[sel(1, 4, 1, 4), sel(1, 4, 1, 4)], [sel(1, 4, 1, 4), sel(1, 4, 1, 4)]],
                );
            }

            #[test]
            fn edit_start_gt_range_end() {
                run_test(
                    Range::new(1, 4, 1, 4),
                    Range::new(1, 5, 1, 7),
                    "",
                    [[sel(1, 4, 1, 4), sel(1, 4, 1, 4)], [sel(1, 4, 1, 4), sel(1, 4, 1, 4)]],
                );
            }
        }

        mod non_collapsed_dec {
            use super::*;

            #[test]
            fn edit_end_lt_range_start() {
                run_test(
                    Range::new(1, 4, 1, 9),
                    Range::new(1, 1, 1, 3),
                    "",
                    [[sel(1, 2, 1, 7), sel(1, 7, 1, 2)], [sel(1, 2, 1, 7), sel(1, 7, 1, 2)]],
                );
            }

            #[test]
            fn edit_end_le_range_start() {
                run_test(
                    Range::new(1, 4, 1, 9),
                    Range::new(1, 2, 1, 4),
                    "",
                    [[sel(1, 2, 1, 7), sel(1, 7, 1, 2)], [sel(1, 2, 1, 7), sel(1, 7, 1, 2)]],
                );
            }

            #[test]
            fn edit_start_lt_range_start_and_edit_end_lt_range_end() {
                run_test(
                    Range::new(1, 4, 1, 9),
                    Range::new(1, 3, 1, 5),
                    "",
                    [[sel(1, 3, 1, 7), sel(1, 7, 1, 3)], [sel(1, 3, 1, 7), sel(1, 7, 1, 3)]],
                );
            }

            #[test]
            fn edit_start_lt_range_start_and_edit_end_eq_range_end() {
                run_test(
                    Range::new(1, 4, 1, 9),
                    Range::new(1, 3, 1, 9),
                    "",
                    [[sel(1, 3, 1, 3), sel(1, 3, 1, 3)], [sel(1, 3, 1, 3), sel(1, 3, 1, 3)]],
                );
            }

            #[test]
            fn edit_start_lt_range_start_and_edit_end_gt_range_end() {
                run_test(
                    Range::new(1, 4, 1, 9),
                    Range::new(1, 3, 1, 10),
                    "",
                    [[sel(1, 3, 1, 3), sel(1, 3, 1, 3)], [sel(1, 3, 1, 3), sel(1, 3, 1, 3)]],
                );
            }

            #[test]
            fn edit_start_eq_range_start_and_edit_end_lt_range_end() {
                run_test(
                    Range::new(1, 4, 1, 9),
                    Range::new(1, 4, 1, 6),
                    "",
                    [[sel(1, 4, 1, 7), sel(1, 7, 1, 4)], [sel(1, 4, 1, 7), sel(1, 7, 1, 4)]],
                );
            }

            #[test]
            fn edit_start_eq_range_start_and_edit_end_eq_range_end() {
                run_test(
                    Range::new(1, 4, 1, 9),
                    Range::new(1, 4, 1, 9),
                    "",
                    [[sel(1, 4, 1, 4), sel(1, 4, 1, 4)], [sel(1, 4, 1, 4), sel(1, 4, 1, 4)]],
                );
            }

            #[test]
            fn edit_start_eq_range_start_and_edit_end_gt_range_end() {
                run_test(
                    Range::new(1, 4, 1, 9),
                    Range::new(1, 4, 1, 10),
                    "",
                    [[sel(1, 4, 1, 4), sel(1, 4, 1, 4)], [sel(1, 4, 1, 4), sel(1, 4, 1, 4)]],
                );
            }

            #[test]
            fn edit_start_gt_range_start_and_edit_start_lt_range_end_and_edit_end_lt_range_end() {
                run_test(
                    Range::new(1, 4, 1, 9),
                    Range::new(1, 5, 1, 7),
                    "",
                    [[sel(1, 4, 1, 7), sel(1, 7, 1, 4)], [sel(1, 4, 1, 7), sel(1, 7, 1, 4)]],
                );
            }

            #[test]
            fn edit_start_gt_range_start_and_edit_start_lt_range_end_and_edit_end_eq_range_end() {
                run_test(
                    Range::new(1, 4, 1, 9),
                    Range::new(1, 5, 1, 9),
                    "",
                    [[sel(1, 4, 1, 5), sel(1, 5, 1, 4)], [sel(1, 4, 1, 5), sel(1, 5, 1, 4)]],
                );
            }

            #[test]
            fn edit_start_gt_range_start_and_edit_start_lt_range_end_and_edit_end_gt_range_end() {
                run_test(
                    Range::new(1, 4, 1, 9),
                    Range::new(1, 5, 1, 10),
                    "",
                    [[sel(1, 4, 1, 5), sel(1, 5, 1, 4)], [sel(1, 4, 1, 5), sel(1, 5, 1, 4)]],
                );
            }

            #[test]
            fn edit_start_eq_range_end() {
                run_test(
                    Range::new(1, 4, 1, 9),
                    Range::new(1, 9, 1, 11),
                    "",
                    [[sel(1, 4, 1, 9), sel(1, 9, 1, 4)], [sel(1, 4, 1, 9), sel(1, 9, 1, 4)]],
                );
            }

            #[test]
            fn edit_start_gt_range_end() {
                run_test(
                    Range::new(1, 4, 1, 9),
                    Range::new(1, 10, 1, 11),
                    "",
                    [[sel(1, 4, 1, 9), sel(1, 9, 1, 4)], [sel(1, 4, 1, 9), sel(1, 9, 1, 4)]],
                );
            }
        }
    }

    mod replace_short {
        use super::*;

        mod collapsed_dec {
            use super::*;

            #[test]
            fn edit_end_lt_range_start() {
                run_test(
                    Range::new(1, 4, 1, 4),
                    Range::new(1, 1, 1, 3),
                    "c",
                    [[sel(1, 3, 1, 3), sel(1, 3, 1, 3)], [sel(1, 3, 1, 3), sel(1, 3, 1, 3)]],
                );
            }

            #[test]
            fn edit_end_le_range_start() {
                run_test(
                    Range::new(1, 4, 1, 4),
                    Range::new(1, 2, 1, 4),
                    "c",
                    [[sel(1, 3, 1, 3), sel(1, 3, 1, 3)], [sel(1, 3, 1, 3), sel(1, 3, 1, 3)]],
                );
            }

            #[test]
            fn edit_start_lt_range_start_and_edit_end_gt_range_end() {
                run_test(
                    Range::new(1, 4, 1, 4),
                    Range::new(1, 3, 1, 5),
                    "c",
                    [[sel(1, 4, 1, 4), sel(1, 4, 1, 4)], [sel(1, 4, 1, 4), sel(1, 4, 1, 4)]],
                );
            }

            #[test]
            fn edit_start_ge_range_end() {
                run_test(
                    Range::new(1, 4, 1, 4),
                    Range::new(1, 4, 1, 6),
                    "c",
                    [[sel(1, 4, 1, 4), sel(1, 4, 1, 4)], [sel(1, 5, 1, 5), sel(1, 5, 1, 5)]],
                );
            }

            #[test]
            fn edit_start_gt_range_end() {
                run_test(
                    Range::new(1, 4, 1, 4),
                    Range::new(1, 5, 1, 7),
                    "c",
                    [[sel(1, 4, 1, 4), sel(1, 4, 1, 4)], [sel(1, 4, 1, 4), sel(1, 4, 1, 4)]],
                );
            }
        }

        mod non_collapsed_dec {
            use super::*;

            #[test]
            fn edit_end_lt_range_start() {
                run_test(
                    Range::new(1, 4, 1, 9),
                    Range::new(1, 1, 1, 3),
                    "c",
                    [[sel(1, 3, 1, 8), sel(1, 8, 1, 3)], [sel(1, 3, 1, 8), sel(1, 8, 1, 3)]],
                );
            }

            #[test]
            fn edit_end_le_range_start() {
                run_test(
                    Range::new(1, 4, 1, 9),
                    Range::new(1, 2, 1, 4),
                    "c",
                    [[sel(1, 3, 1, 8), sel(1, 8, 1, 3)], [sel(1, 3, 1, 8), sel(1, 8, 1, 3)]],
                );
            }

            #[test]
            fn edit_start_lt_range_start_and_edit_end_lt_range_end() {
                run_test(
                    Range::new(1, 4, 1, 9),
                    Range::new(1, 3, 1, 5),
                    "c",
                    [[sel(1, 4, 1, 8), sel(1, 8, 1, 4)], [sel(1, 4, 1, 8), sel(1, 8, 1, 4)]],
                );
            }

            #[test]
            fn edit_start_lt_range_start_and_edit_end_eq_range_end() {
                run_test(
                    Range::new(1, 4, 1, 9),
                    Range::new(1, 3, 1, 9),
                    "c",
                    [[sel(1, 4, 1, 4), sel(1, 4, 1, 4)], [sel(1, 4, 1, 4), sel(1, 4, 1, 4)]],
                );
            }

            #[test]
            fn edit_start_lt_range_start_and_edit_end_gt_range_end() {
                run_test(
                    Range::new(1, 4, 1, 9),
                    Range::new(1, 3, 1, 10),
                    "c",
                    [[sel(1, 4, 1, 4), sel(1, 4, 1, 4)], [sel(1, 4, 1, 4), sel(1, 4, 1, 4)]],
                );
            }

            #[test]
            fn edit_start_eq_range_start_and_edit_end_lt_range_end() {
                run_test(
                    Range::new(1, 4, 1, 9),
                    Range::new(1, 4, 1, 6),
                    "c",
                    [[sel(1, 4, 1, 8), sel(1, 8, 1, 4)], [sel(1, 5, 1, 8), sel(1, 8, 1, 5)]],
                );
            }

            #[test]
            fn edit_start_eq_range_start_and_edit_end_eq_range_end() {
                run_test(
                    Range::new(1, 4, 1, 9),
                    Range::new(1, 4, 1, 9),
                    "c",
                    [[sel(1, 4, 1, 5), sel(1, 5, 1, 4)], [sel(1, 5, 1, 5), sel(1, 5, 1, 5)]],
                );
            }

            #[test]
            fn edit_start_eq_range_start_and_edit_end_gt_range_end() {
                run_test(
                    Range::new(1, 4, 1, 9),
                    Range::new(1, 4, 1, 10),
                    "c",
                    [[sel(1, 4, 1, 5), sel(1, 5, 1, 4)], [sel(1, 5, 1, 5), sel(1, 5, 1, 5)]],
                );
            }

            #[test]
            fn edit_start_gt_range_start_and_edit_start_lt_range_end_and_edit_end_lt_range_end() {
                run_test(
                    Range::new(1, 4, 1, 9),
                    Range::new(1, 5, 1, 7),
                    "c",
                    [[sel(1, 4, 1, 8), sel(1, 8, 1, 4)], [sel(1, 4, 1, 8), sel(1, 8, 1, 4)]],
                );
            }

            #[test]
            fn edit_start_gt_range_start_and_edit_start_lt_range_end_and_edit_end_eq_range_end() {
                run_test(
                    Range::new(1, 4, 1, 9),
                    Range::new(1, 5, 1, 9),
                    "c",
                    [[sel(1, 4, 1, 6), sel(1, 6, 1, 4)], [sel(1, 4, 1, 6), sel(1, 6, 1, 4)]],
                );
            }

            #[test]
            fn edit_start_gt_range_start_and_edit_start_lt_range_end_and_edit_end_gt_range_end() {
                run_test(
                    Range::new(1, 4, 1, 9),
                    Range::new(1, 5, 1, 10),
                    "c",
                    [[sel(1, 4, 1, 6), sel(1, 6, 1, 4)], [sel(1, 4, 1, 6), sel(1, 6, 1, 4)]],
                );
            }

            #[test]
            fn edit_start_eq_range_end() {
                run_test(
                    Range::new(1, 4, 1, 9),
                    Range::new(1, 9, 1, 11),
                    "c",
                    [[sel(1, 4, 1, 9), sel(1, 9, 1, 4)], [sel(1, 4, 1, 10), sel(1, 10, 1, 4)]],
                );
            }

            #[test]
            fn edit_start_gt_range_end() {
                run_test(
                    Range::new(1, 4, 1, 9),
                    Range::new(1, 10, 1, 11),
                    "c",
                    [[sel(1, 4, 1, 9), sel(1, 9, 1, 4)], [sel(1, 4, 1, 9), sel(1, 9, 1, 4)]],
                );
            }
        }
    }

    mod replace_long {
        use super::*;

        mod collapsed_dec {
            use super::*;

            #[test]
            fn edit_end_lt_range_start() {
                run_test(
                    Range::new(1, 4, 1, 4),
                    Range::new(1, 1, 1, 3),
                    "cccc",
                    [[sel(1, 6, 1, 6), sel(1, 6, 1, 6)], [sel(1, 6, 1, 6), sel(1, 6, 1, 6)]],
                );
            }

            #[test]
            fn edit_end_le_range_start() {
                run_test(
                    Range::new(1, 4, 1, 4),
                    Range::new(1, 2, 1, 4),
                    "cccc",
                    [[sel(1, 6, 1, 6), sel(1, 6, 1, 6)], [sel(1, 6, 1, 6), sel(1, 6, 1, 6)]],
                );
            }

            #[test]
            fn edit_start_lt_range_start_and_edit_end_gt_range_end() {
                run_test(
                    Range::new(1, 4, 1, 4),
                    Range::new(1, 3, 1, 5),
                    "cccc",
                    [[sel(1, 4, 1, 4), sel(1, 4, 1, 4)], [sel(1, 7, 1, 7), sel(1, 7, 1, 7)]],
                );
            }

            #[test]
            fn edit_start_ge_range_end() {
                run_test(
                    Range::new(1, 4, 1, 4),
                    Range::new(1, 4, 1, 6),
                    "cccc",
                    [[sel(1, 4, 1, 4), sel(1, 4, 1, 4)], [sel(1, 8, 1, 8), sel(1, 8, 1, 8)]],
                );
            }

            #[test]
            fn edit_start_gt_range_end() {
                run_test(
                    Range::new(1, 4, 1, 4),
                    Range::new(1, 5, 1, 7),
                    "cccc",
                    [[sel(1, 4, 1, 4), sel(1, 4, 1, 4)], [sel(1, 4, 1, 4), sel(1, 4, 1, 4)]],
                );
            }
        }

        mod non_collapsed_dec {
            use super::*;

            #[test]
            fn edit_end_lt_range_start() {
                run_test(
                    Range::new(1, 4, 1, 9),
                    Range::new(1, 1, 1, 3),
                    "cccc",
                    [[sel(1, 6, 1, 11), sel(1, 11, 1, 6)], [sel(1, 6, 1, 11), sel(1, 11, 1, 6)]],
                );
            }

            #[test]
            fn edit_end_le_range_start() {
                run_test(
                    Range::new(1, 4, 1, 9),
                    Range::new(1, 2, 1, 4),
                    "cccc",
                    [[sel(1, 4, 1, 11), sel(1, 11, 1, 4)], [sel(1, 6, 1, 11), sel(1, 11, 1, 6)]],
                );
            }

            #[test]
            fn edit_start_lt_range_start_and_edit_end_lt_range_end() {
                run_test(
                    Range::new(1, 4, 1, 9),
                    Range::new(1, 3, 1, 5),
                    "cccc",
                    [[sel(1, 4, 1, 11), sel(1, 11, 1, 4)], [sel(1, 7, 1, 11), sel(1, 11, 1, 7)]],
                );
            }

            #[test]
            fn edit_start_lt_range_start_and_edit_end_eq_range_end() {
                run_test(
                    Range::new(1, 4, 1, 9),
                    Range::new(1, 3, 1, 9),
                    "cccc",
                    [[sel(1, 4, 1, 7), sel(1, 7, 1, 4)], [sel(1, 7, 1, 7), sel(1, 7, 1, 7)]],
                );
            }

            #[test]
            fn edit_start_lt_range_start_and_edit_end_gt_range_end() {
                run_test(
                    Range::new(1, 4, 1, 9),
                    Range::new(1, 3, 1, 10),
                    "cccc",
                    [[sel(1, 4, 1, 7), sel(1, 7, 1, 4)], [sel(1, 7, 1, 7), sel(1, 7, 1, 7)]],
                );
            }

            #[test]
            fn edit_start_eq_range_start_and_edit_end_lt_range_end() {
                run_test(
                    Range::new(1, 4, 1, 9),
                    Range::new(1, 4, 1, 6),
                    "cccc",
                    [[sel(1, 4, 1, 11), sel(1, 11, 1, 4)], [sel(1, 8, 1, 11), sel(1, 11, 1, 8)]],
                );
            }

            #[test]
            fn edit_start_eq_range_start_and_edit_end_eq_range_end() {
                run_test(
                    Range::new(1, 4, 1, 9),
                    Range::new(1, 4, 1, 9),
                    "cccc",
                    [[sel(1, 4, 1, 8), sel(1, 8, 1, 4)], [sel(1, 8, 1, 8), sel(1, 8, 1, 8)]],
                );
            }

            #[test]
            fn edit_start_eq_range_start_and_edit_end_gt_range_end() {
                run_test(
                    Range::new(1, 4, 1, 9),
                    Range::new(1, 4, 1, 10),
                    "cccc",
                    [[sel(1, 4, 1, 8), sel(1, 8, 1, 4)], [sel(1, 8, 1, 8), sel(1, 8, 1, 8)]],
                );
            }

            #[test]
            fn edit_start_gt_range_start_and_edit_start_lt_range_end_and_edit_end_lt_range_end() {
                run_test(
                    Range::new(1, 4, 1, 9),
                    Range::new(1, 5, 1, 7),
                    "cccc",
                    [[sel(1, 4, 1, 11), sel(1, 11, 1, 4)], [sel(1, 4, 1, 11), sel(1, 11, 1, 4)]],
                );
            }

            #[test]
            fn edit_start_gt_range_start_and_edit_start_lt_range_end_and_edit_end_eq_range_end() {
                run_test(
                    Range::new(1, 4, 1, 9),
                    Range::new(1, 5, 1, 9),
                    "cccc",
                    [[sel(1, 4, 1, 9), sel(1, 9, 1, 4)], [sel(1, 4, 1, 9), sel(1, 9, 1, 4)]],
                );
            }

            #[test]
            fn edit_start_gt_range_start_and_edit_start_lt_range_end_and_edit_end_gt_range_end() {
                run_test(
                    Range::new(1, 4, 1, 9),
                    Range::new(1, 5, 1, 10),
                    "cccc",
                    [[sel(1, 4, 1, 9), sel(1, 9, 1, 4)], [sel(1, 4, 1, 9), sel(1, 9, 1, 4)]],
                );
            }

            #[test]
            fn edit_start_eq_range_end() {
                run_test(
                    Range::new(1, 4, 1, 9),
                    Range::new(1, 9, 1, 11),
                    "cccc",
                    [[sel(1, 4, 1, 9), sel(1, 9, 1, 4)], [sel(1, 4, 1, 13), sel(1, 13, 1, 4)]],
                );
            }

            #[test]
            fn edit_start_gt_range_end() {
                run_test(
                    Range::new(1, 4, 1, 9),
                    Range::new(1, 10, 1, 11),
                    "cccc",
                    [[sel(1, 4, 1, 9), sel(1, 9, 1, 4)], [sel(1, 4, 1, 9), sel(1, 9, 1, 4)]],
                );
            }
        }
    }
}
