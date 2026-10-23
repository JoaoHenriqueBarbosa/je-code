//! Porte de `src/vs/editor/test/browser/commands/shiftCommand.test.ts` (suíte `Editor Commands - ShiftCommand`),
//! caso a caso e na mesma ordem.
//!
//! Notas de tradução:
//! - `testCommand` recebe a seleção já ajustada e a `commandFactory` sem o `accessor` (ver
//!   `tests/support/test_command.rs`). O `ILanguageConfigurationService` que o original pega do `accessor` é
//!   criado em cada caso (`TestLanguageConfigurationService`) e registrado no `LeakGuard`.
//! - O `prepare` do original (`prepareDocBlockCommentLanguage`, que registra a linguagem `commentMode` nos
//!   serviços do editor de teste) vira o registro da linguagem nos serviços de um modelo com linguagens de
//!   verdade (`create_language_model_services`), e o comando recebe o `ILanguageConfigurationService` desses
//!   mesmos serviços, como no original. O caminho é `test_command_on_model`.
//! - `ensureNoDisposablesAreLeakedInTestSuite()` vira o `LeakGuard` criado no começo de cada caso.
//! - `createSingleEditOp(text, positionLineNumber, positionColumn, selectionLineNumber, selectionColumn)` vira
//!   [`create_single_edit_op`], com a posição e a seleção como no original.
//! - `DocBlockCommentMode` (um `Disposable` que registra a linguagem e a configuração) vira
//!   [`register_doc_block_comment_mode`], cujos registros são descartados pelo `LeakGuard`.

#[path = "../../je-base/tests/common/mod.rs"]
mod common;
mod support;

use std::rc::Rc;

use common::{LeakGuard, ensure_no_disposables_are_leaked_in_test_suite};
use je_base::JsString;
use je_base::lifecycle::IDisposable;
use je_editor::config::editor_options::EditorAutoIndentStrategy;
use je_editor::edit_operation::SingleEditOperation;
use je_editor::language_configuration::LanguageConfiguration;
use je_editor::language_configuration_registry::ILanguageConfigurationService;
use je_editor::language_service::ILanguageService;
use je_editor::modes_registry::LanguageExtensionPoint;
use je_editor::range::Range;
use je_editor::selection::Selection;
use je_editor::shift_command::{ShiftCommand, ShiftCommandOpts};
use support::on_enter_rules::javascript_on_enter_rules;
use support::test_command::{get_edit_operation, test_command, test_command_on_model};
use support::test_language_configuration_service::TestLanguageConfigurationService;
use support::test_text_model::{create_language_model_services, instantiate_text_model_with_languages, with_editor_model, LanguageModelServices};

/// `DocBlockCommentMode.languageId`.
const DOC_BLOCK_COMMENT_LANGUAGE_ID: &str = "commentMode";

/// As linhas que a maioria dos casos usa.
const BASE_LINES: &[&str] = &["My First Line", "\t\tMy Second Line", "    Third Line", "", "123"];

/// Create single edit operation
fn create_single_edit_op(
    text: &str,
    position_line_number: i32,
    position_column: i32,
    selection_line_number: i32,
    selection_column: i32,
) -> SingleEditOperation {
    SingleEditOperation::new(
        Range::new(selection_line_number, selection_column, position_line_number, position_column),
        Some(JsString::from(text)),
        false,
    )
}

/// `new DocBlockCommentMode(languageService, languageConfigurationService)`.
fn register_doc_block_comment_mode(ds: &LeakGuard, services: &LanguageModelServices) {
    ds.add(services.language_service.register_language(LanguageExtensionPoint::new(DOC_BLOCK_COMMENT_LANGUAGE_ID)));
    ds.add(services.language_configuration_service.register(
        DOC_BLOCK_COMMENT_LANGUAGE_ID,
        LanguageConfiguration {
            brackets: Some(vec![
                ("(".to_owned(), ")".to_owned()),
                ("{".to_owned(), "}".to_owned()),
                ("[".to_owned(), "]".to_owned()),
            ]),
            on_enter_rules: Some(javascript_on_enter_rules()),
            ..LanguageConfiguration::default()
        },
        None,
    ));
}

fn shift_opts(is_unshift: bool, use_tab_stops: bool) -> ShiftCommandOpts {
    ShiftCommandOpts {
        is_unshift,
        tab_size: 4,
        indent_size: 4,
        insert_spaces: false,
        use_tab_stops,
        auto_indent: EditorAutoIndentStrategy::Full,
    }
}

/// O corpo comum de `testShiftCommand`, `testUnshiftCommand` e dos casos que montam o `ShiftCommand` à mão.
fn test_shift_command_with_opts(
    ds: &LeakGuard,
    lines: &[&str],
    language_id: Option<&str>,
    opts: ShiftCommandOpts,
    selection: Selection,
    expected_lines: &[&str],
    expected_selection: Selection,
) {
    if language_id == Some(DOC_BLOCK_COMMENT_LANGUAGE_ID) {
        // `prepareDocBlockCommentLanguage`
        let services = create_language_model_services(ds);
        register_doc_block_comment_mode(ds, &services);
        let model = instantiate_text_model_with_languages(&services, lines.join("\n").as_str(), DOC_BLOCK_COMMENT_LANGUAGE_ID);
        let language_configuration_service: Rc<dyn ILanguageConfigurationService> = services.language_configuration_service.clone();
        test_command_on_model(
            &model,
            selection,
            move |sel| Box::new(ShiftCommand::new(sel, opts, language_configuration_service)),
            expected_lines,
            expected_selection,
            false,
        );
    } else {
        let test_service = ds.add_rc(TestLanguageConfigurationService::new());
        let language_configuration_service: Rc<dyn ILanguageConfigurationService> = test_service;
        test_command(
            lines,
            language_id,
            selection,
            move |sel| Box::new(ShiftCommand::new(sel, opts, language_configuration_service)),
            expected_lines,
            expected_selection,
            false,
        );
    }
}

fn test_shift_command(
    ds: &LeakGuard,
    lines: &[&str],
    language_id: Option<&str>,
    use_tab_stops: bool,
    selection: Selection,
    expected_lines: &[&str],
    expected_selection: Selection,
) {
    test_shift_command_with_opts(ds, lines, language_id, shift_opts(false, use_tab_stops), selection, expected_lines, expected_selection);
}

fn test_unshift_command(
    ds: &LeakGuard,
    lines: &[&str],
    language_id: Option<&str>,
    use_tab_stops: bool,
    selection: Selection,
    expected_lines: &[&str],
    expected_selection: Selection,
) {
    test_shift_command_with_opts(ds, lines, language_id, shift_opts(true, use_tab_stops), selection, expected_lines, expected_selection);
}

// --------- shift

#[test]
fn bug_9503_shifting_without_any_selection() {
    let ds = ensure_no_disposables_are_leaked_in_test_suite();
    test_shift_command(
        &ds,
        BASE_LINES,
        None,
        true,
        Selection::new(1, 1, 1, 1),
        &["\tMy First Line", "\t\tMy Second Line", "    Third Line", "", "123"],
        Selection::new(1, 2, 1, 2),
    );
}

#[test]
fn shift_on_single_line_selection_1() {
    let ds = ensure_no_disposables_are_leaked_in_test_suite();
    test_shift_command(
        &ds,
        BASE_LINES,
        None,
        true,
        Selection::new(1, 3, 1, 1),
        &["\tMy First Line", "\t\tMy Second Line", "    Third Line", "", "123"],
        Selection::new(1, 4, 1, 1),
    );
}

#[test]
fn shift_on_single_line_selection_2() {
    let ds = ensure_no_disposables_are_leaked_in_test_suite();
    test_shift_command(
        &ds,
        BASE_LINES,
        None,
        true,
        Selection::new(1, 1, 1, 3),
        &["\tMy First Line", "\t\tMy Second Line", "    Third Line", "", "123"],
        Selection::new(1, 1, 1, 4),
    );
}

#[test]
fn simple_shift() {
    let ds = ensure_no_disposables_are_leaked_in_test_suite();
    test_shift_command(
        &ds,
        BASE_LINES,
        None,
        true,
        Selection::new(1, 1, 2, 1),
        &["\tMy First Line", "\t\tMy Second Line", "    Third Line", "", "123"],
        Selection::new(1, 1, 2, 1),
    );
}

#[test]
fn shifting_on_two_separate_lines() {
    let ds = ensure_no_disposables_are_leaked_in_test_suite();
    test_shift_command(
        &ds,
        BASE_LINES,
        None,
        true,
        Selection::new(1, 1, 2, 1),
        &["\tMy First Line", "\t\tMy Second Line", "    Third Line", "", "123"],
        Selection::new(1, 1, 2, 1),
    );

    test_shift_command(
        &ds,
        &["\tMy First Line", "\t\tMy Second Line", "    Third Line", "", "123"],
        None,
        true,
        Selection::new(2, 1, 3, 1),
        &["\tMy First Line", "\t\t\tMy Second Line", "    Third Line", "", "123"],
        Selection::new(2, 1, 3, 1),
    );
}

#[test]
fn shifting_on_two_lines() {
    let ds = ensure_no_disposables_are_leaked_in_test_suite();
    test_shift_command(
        &ds,
        BASE_LINES,
        None,
        true,
        Selection::new(1, 2, 2, 2),
        &["\tMy First Line", "\t\t\tMy Second Line", "    Third Line", "", "123"],
        Selection::new(1, 3, 2, 2),
    );
}

#[test]
fn shifting_on_two_lines_again() {
    let ds = ensure_no_disposables_are_leaked_in_test_suite();
    test_shift_command(
        &ds,
        BASE_LINES,
        None,
        true,
        Selection::new(2, 2, 1, 2),
        &["\tMy First Line", "\t\t\tMy Second Line", "    Third Line", "", "123"],
        Selection::new(2, 2, 1, 3),
    );
}

#[test]
fn shifting_at_end_of_file() {
    let ds = ensure_no_disposables_are_leaked_in_test_suite();
    test_shift_command(
        &ds,
        BASE_LINES,
        None,
        true,
        Selection::new(4, 1, 5, 2),
        &["My First Line", "\t\tMy Second Line", "    Third Line", "", "\t123"],
        Selection::new(4, 1, 5, 3),
    );
}

#[test]
fn issue_1120_tab_should_not_indent_empty_lines_in_a_multi_line_selection() {
    let ds = ensure_no_disposables_are_leaked_in_test_suite();
    test_shift_command(
        &ds,
        BASE_LINES,
        None,
        true,
        Selection::new(1, 1, 5, 2),
        &["\tMy First Line", "\t\t\tMy Second Line", "\t\tThird Line", "", "\t123"],
        Selection::new(1, 1, 5, 3),
    );

    test_shift_command(
        &ds,
        BASE_LINES,
        None,
        true,
        Selection::new(4, 1, 5, 1),
        &["My First Line", "\t\tMy Second Line", "    Third Line", "\t", "123"],
        Selection::new(4, 1, 5, 1),
    );
}

// --------- unshift

#[test]
fn unshift_on_single_line_selection_1() {
    let ds = ensure_no_disposables_are_leaked_in_test_suite();
    test_shift_command(
        &ds,
        BASE_LINES,
        None,
        true,
        Selection::new(2, 3, 2, 1),
        &["My First Line", "\t\t\tMy Second Line", "    Third Line", "", "123"],
        Selection::new(2, 3, 2, 1),
    );
}

#[test]
fn unshift_on_single_line_selection_2() {
    let ds = ensure_no_disposables_are_leaked_in_test_suite();
    test_shift_command(
        &ds,
        BASE_LINES,
        None,
        true,
        Selection::new(2, 1, 2, 3),
        &["My First Line", "\t\t\tMy Second Line", "    Third Line", "", "123"],
        Selection::new(2, 1, 2, 3),
    );
}

#[test]
fn simple_unshift() {
    let ds = ensure_no_disposables_are_leaked_in_test_suite();
    test_unshift_command(
        &ds,
        BASE_LINES,
        None,
        true,
        Selection::new(1, 1, 2, 1),
        &["My First Line", "\t\tMy Second Line", "    Third Line", "", "123"],
        Selection::new(1, 1, 2, 1),
    );
}

#[test]
fn unshifting_on_two_lines_1() {
    let ds = ensure_no_disposables_are_leaked_in_test_suite();
    test_unshift_command(
        &ds,
        BASE_LINES,
        None,
        true,
        Selection::new(1, 2, 2, 2),
        &["My First Line", "\tMy Second Line", "    Third Line", "", "123"],
        Selection::new(1, 2, 2, 2),
    );
}

#[test]
fn unshifting_on_two_lines_2() {
    let ds = ensure_no_disposables_are_leaked_in_test_suite();
    test_unshift_command(
        &ds,
        BASE_LINES,
        None,
        true,
        Selection::new(2, 3, 2, 1),
        &["My First Line", "\tMy Second Line", "    Third Line", "", "123"],
        Selection::new(2, 2, 2, 1),
    );
}

#[test]
fn unshifting_at_the_end_of_the_file() {
    let ds = ensure_no_disposables_are_leaked_in_test_suite();
    test_unshift_command(
        &ds,
        BASE_LINES,
        None,
        true,
        Selection::new(4, 1, 5, 2),
        &["My First Line", "\t\tMy Second Line", "    Third Line", "", "123"],
        Selection::new(4, 1, 5, 2),
    );
}

#[test]
fn unshift_many_times_plus_shift() {
    let ds = ensure_no_disposables_are_leaked_in_test_suite();
    test_unshift_command(
        &ds,
        BASE_LINES,
        None,
        true,
        Selection::new(1, 1, 5, 4),
        &["My First Line", "\tMy Second Line", "Third Line", "", "123"],
        Selection::new(1, 1, 5, 4),
    );

    test_unshift_command(
        &ds,
        &["My First Line", "\tMy Second Line", "Third Line", "", "123"],
        None,
        true,
        Selection::new(1, 1, 5, 4),
        &["My First Line", "My Second Line", "Third Line", "", "123"],
        Selection::new(1, 1, 5, 4),
    );

    test_shift_command(
        &ds,
        &["My First Line", "My Second Line", "Third Line", "", "123"],
        None,
        true,
        Selection::new(1, 1, 5, 4),
        &["\tMy First Line", "\tMy Second Line", "\tThird Line", "", "\t123"],
        Selection::new(1, 1, 5, 5),
    );
}

#[test]
fn bug_9119_unshift_from_first_column_doesnt_work() {
    let ds = ensure_no_disposables_are_leaked_in_test_suite();
    test_unshift_command(
        &ds,
        BASE_LINES,
        None,
        true,
        Selection::new(2, 1, 2, 1),
        &["My First Line", "\tMy Second Line", "    Third Line", "", "123"],
        Selection::new(2, 1, 2, 1),
    );
}

#[test]
fn issue_348_indenting_around_doc_block_comments() {
    let ds = ensure_no_disposables_are_leaked_in_test_suite();
    test_shift_command(
        &ds,
        &["", "/**", " * a doc comment", " */", "function hello() {}"],
        Some(DOC_BLOCK_COMMENT_LANGUAGE_ID),
        true,
        Selection::new(1, 1, 5, 20),
        &["", "\t/**", "\t * a doc comment", "\t */", "\tfunction hello() {}"],
        Selection::new(1, 1, 5, 21),
    );

    test_unshift_command(
        &ds,
        &["", "/**", " * a doc comment", " */", "function hello() {}"],
        Some(DOC_BLOCK_COMMENT_LANGUAGE_ID),
        true,
        Selection::new(1, 1, 5, 20),
        &["", "/**", " * a doc comment", " */", "function hello() {}"],
        Selection::new(1, 1, 5, 20),
    );

    test_unshift_command(
        &ds,
        &["\t", "\t/**", "\t * a doc comment", "\t */", "\tfunction hello() {}"],
        Some(DOC_BLOCK_COMMENT_LANGUAGE_ID),
        true,
        Selection::new(1, 1, 5, 21),
        &["", "/**", " * a doc comment", " */", "function hello() {}"],
        Selection::new(1, 1, 5, 20),
    );
}

#[test]
fn issue_1609_wrong_indentation_of_block_comments() {
    let ds = ensure_no_disposables_are_leaked_in_test_suite();
    test_shift_command(
        &ds,
        &["", "/**", " * test", " *", " * @type {number}", " */", "var foo = 0;"],
        Some(DOC_BLOCK_COMMENT_LANGUAGE_ID),
        true,
        Selection::new(1, 1, 7, 13),
        &["", "\t/**", "\t * test", "\t *", "\t * @type {number}", "\t */", "\tvar foo = 0;"],
        Selection::new(1, 1, 7, 14),
    );
}

#[test]
fn issue_1620_a_line_indent_doesnt_handle_leading_whitespace_properly() {
    let ds = ensure_no_disposables_are_leaked_in_test_suite();
    test_shift_command_with_opts(
        &ds,
        &[
            "   Written | Numeric",
            "       one | 1",
            "       two | 2",
            "     three | 3",
            "      four | 4",
            "      five | 5",
            "       six | 6",
            "     seven | 7",
            "     eight | 8",
            "      nine | 9",
            "       ten | 10",
            "    eleven | 11",
            "",
        ],
        None,
        ShiftCommandOpts {
            is_unshift: false,
            tab_size: 4,
            indent_size: 4,
            insert_spaces: true,
            use_tab_stops: false,
            auto_indent: EditorAutoIndentStrategy::Full,
        },
        Selection::new(1, 1, 13, 1),
        &[
            "       Written | Numeric",
            "           one | 1",
            "           two | 2",
            "         three | 3",
            "          four | 4",
            "          five | 5",
            "           six | 6",
            "         seven | 7",
            "         eight | 8",
            "          nine | 9",
            "           ten | 10",
            "        eleven | 11",
            "",
        ],
        Selection::new(1, 1, 13, 1),
    );
}

#[test]
fn issue_1620_b_line_indent_doesnt_handle_leading_whitespace_properly() {
    let ds = ensure_no_disposables_are_leaked_in_test_suite();
    test_shift_command_with_opts(
        &ds,
        &[
            "       Written | Numeric",
            "           one | 1",
            "           two | 2",
            "         three | 3",
            "          four | 4",
            "          five | 5",
            "           six | 6",
            "         seven | 7",
            "         eight | 8",
            "          nine | 9",
            "           ten | 10",
            "        eleven | 11",
            "",
        ],
        None,
        ShiftCommandOpts {
            is_unshift: true,
            tab_size: 4,
            indent_size: 4,
            insert_spaces: true,
            use_tab_stops: false,
            auto_indent: EditorAutoIndentStrategy::Full,
        },
        Selection::new(1, 1, 13, 1),
        &[
            "   Written | Numeric",
            "       one | 1",
            "       two | 2",
            "     three | 3",
            "      four | 4",
            "      five | 5",
            "       six | 6",
            "     seven | 7",
            "     eight | 8",
            "      nine | 9",
            "       ten | 10",
            "    eleven | 11",
            "",
        ],
        Selection::new(1, 1, 13, 1),
    );
}

#[test]
fn issue_1620_c_line_indent_doesnt_handle_leading_whitespace_properly() {
    let ds = ensure_no_disposables_are_leaked_in_test_suite();
    test_shift_command_with_opts(
        &ds,
        &[
            "       Written | Numeric",
            "           one | 1",
            "           two | 2",
            "         three | 3",
            "          four | 4",
            "          five | 5",
            "           six | 6",
            "         seven | 7",
            "         eight | 8",
            "          nine | 9",
            "           ten | 10",
            "        eleven | 11",
            "",
        ],
        None,
        ShiftCommandOpts {
            is_unshift: true,
            tab_size: 4,
            indent_size: 4,
            insert_spaces: false,
            use_tab_stops: false,
            auto_indent: EditorAutoIndentStrategy::Full,
        },
        Selection::new(1, 1, 13, 1),
        &[
            "   Written | Numeric",
            "       one | 1",
            "       two | 2",
            "     three | 3",
            "      four | 4",
            "      five | 5",
            "       six | 6",
            "     seven | 7",
            "     eight | 8",
            "      nine | 9",
            "       ten | 10",
            "    eleven | 11",
            "",
        ],
        Selection::new(1, 1, 13, 1),
    );
}

#[test]
fn issue_1620_d_line_indent_doesnt_handle_leading_whitespace_properly() {
    let ds = ensure_no_disposables_are_leaked_in_test_suite();
    test_shift_command_with_opts(
        &ds,
        &[
            "\t   Written | Numeric",
            "\t       one | 1",
            "\t       two | 2",
            "\t     three | 3",
            "\t      four | 4",
            "\t      five | 5",
            "\t       six | 6",
            "\t     seven | 7",
            "\t     eight | 8",
            "\t      nine | 9",
            "\t       ten | 10",
            "\t    eleven | 11",
            "",
        ],
        None,
        ShiftCommandOpts {
            is_unshift: true,
            tab_size: 4,
            indent_size: 4,
            insert_spaces: true,
            use_tab_stops: false,
            auto_indent: EditorAutoIndentStrategy::Full,
        },
        Selection::new(1, 1, 13, 1),
        &[
            "   Written | Numeric",
            "       one | 1",
            "       two | 2",
            "     three | 3",
            "      four | 4",
            "      five | 5",
            "       six | 6",
            "     seven | 7",
            "     eight | 8",
            "      nine | 9",
            "       ten | 10",
            "    eleven | 11",
            "",
        ],
        Selection::new(1, 1, 13, 1),
    );
}

#[test]
fn issue_monaco_editor_443_indentation_of_a_single_row_deletes_selected_text_in_some_cases() {
    let ds = ensure_no_disposables_are_leaked_in_test_suite();
    test_shift_command_with_opts(
        &ds,
        &["Hello world!", "another line"],
        None,
        ShiftCommandOpts {
            is_unshift: false,
            tab_size: 4,
            indent_size: 4,
            insert_spaces: false,
            use_tab_stops: true,
            auto_indent: EditorAutoIndentStrategy::Full,
        },
        Selection::new(1, 1, 1, 13),
        &["\tHello world!", "another line"],
        Selection::new(1, 1, 1, 14),
    );
}

#[test]
fn bug_16815_shift_tab_doesnt_go_back_to_tabstop() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();

    fn repeat_str(str: &str, cnt: usize) -> String {
        let mut r = String::new();
        for _ in 0..cnt {
            r.push_str(str);
        }
        r
    }

    fn assert_shift_command_with(
        is_unshift: bool,
        tab_size: i32,
        indent_size: i32,
        insert_spaces: bool,
        text: &[&str],
        expected: Vec<SingleEditOperation>,
    ) {
        with_editor_model(text, |model| {
            let test_language_configuration_service = TestLanguageConfigurationService::new();
            let language_configuration_service: Rc<dyn ILanguageConfigurationService> = test_language_configuration_service.clone();
            let mut op = ShiftCommand::new(
                Selection::new(1, 1, text.len() as i32 + 1, 1),
                ShiftCommandOpts {
                    is_unshift,
                    tab_size,
                    indent_size,
                    insert_spaces,
                    use_tab_stops: true,
                    auto_indent: EditorAutoIndentStrategy::Full,
                },
                language_configuration_service,
            );
            let actual = get_edit_operation(model, &mut op);
            assert_eq!(actual, expected);
            IDisposable::dispose(&*test_language_configuration_service);
        });
    }

    fn assert_unshift_command(tab_size: i32, indent_size: i32, insert_spaces: bool, text: &[&str], expected: Vec<SingleEditOperation>) {
        assert_shift_command_with(true, tab_size, indent_size, insert_spaces, text, expected);
    }

    fn assert_shift_command(tab_size: i32, indent_size: i32, insert_spaces: bool, text: &[&str], expected: Vec<SingleEditOperation>) {
        assert_shift_command_with(false, tab_size, indent_size, insert_spaces, text, expected);
    }

    let test_outdent = |tab_size: i32, indent_size: i32, insert_spaces: bool, line_text: &str, expected_indents: usize| {
        let one_indent = if insert_spaces { repeat_str(" ", indent_size as usize) } else { "\t".to_owned() };
        let expected_indent = repeat_str(&one_indent, expected_indents);
        let text = format!("{line_text}aaa");
        if !line_text.is_empty() {
            assert_unshift_command(
                tab_size,
                indent_size,
                insert_spaces,
                &[text.as_str()],
                vec![create_single_edit_op(&expected_indent, 1, 1, 1, line_text.len() as i32 + 1)],
            );
        } else {
            assert_unshift_command(tab_size, indent_size, insert_spaces, &[text.as_str()], vec![]);
        }
    };

    let test_indent = |tab_size: i32, indent_size: i32, insert_spaces: bool, line_text: &str, expected_indents: usize| {
        let one_indent = if insert_spaces { repeat_str(" ", indent_size as usize) } else { "\t".to_owned() };
        let expected_indent = repeat_str(&one_indent, expected_indents);
        let text = format!("{line_text}aaa");
        assert_shift_command(
            tab_size,
            indent_size,
            insert_spaces,
            &[text.as_str()],
            vec![create_single_edit_op(&expected_indent, 1, 1, 1, line_text.len() as i32 + 1)],
        );
    };

    let test_indentation = |tab_size: i32, indent_size: i32, line_text: &str, expected_on_outdent: usize, expected_on_indent: usize| {
        test_outdent(tab_size, indent_size, true, line_text, expected_on_outdent);
        test_outdent(tab_size, indent_size, false, line_text, expected_on_outdent);

        test_indent(tab_size, indent_size, true, line_text, expected_on_indent);
        test_indent(tab_size, indent_size, false, line_text, expected_on_indent);
    };

    // insertSpaces: true
    // 0 => 0
    test_indentation(4, 4, "", 0, 1);

    // 1 => 0
    test_indentation(4, 4, "\t", 0, 2);
    test_indentation(4, 4, " ", 0, 1);
    test_indentation(4, 4, " \t", 0, 2);
    test_indentation(4, 4, "  ", 0, 1);
    test_indentation(4, 4, "  \t", 0, 2);
    test_indentation(4, 4, "   ", 0, 1);
    test_indentation(4, 4, "   \t", 0, 2);
    test_indentation(4, 4, "    ", 0, 2);

    // 2 => 1
    test_indentation(4, 4, "\t\t", 1, 3);
    test_indentation(4, 4, "\t ", 1, 2);
    test_indentation(4, 4, "\t \t", 1, 3);
    test_indentation(4, 4, "\t  ", 1, 2);
    test_indentation(4, 4, "\t  \t", 1, 3);
    test_indentation(4, 4, "\t   ", 1, 2);
    test_indentation(4, 4, "\t   \t", 1, 3);
    test_indentation(4, 4, "\t    ", 1, 3);
    test_indentation(4, 4, " \t\t", 1, 3);
    test_indentation(4, 4, " \t ", 1, 2);
    test_indentation(4, 4, " \t \t", 1, 3);
    test_indentation(4, 4, " \t  ", 1, 2);
    test_indentation(4, 4, " \t  \t", 1, 3);
    test_indentation(4, 4, " \t   ", 1, 2);
    test_indentation(4, 4, " \t   \t", 1, 3);
    test_indentation(4, 4, " \t    ", 1, 3);
    test_indentation(4, 4, "  \t\t", 1, 3);
    test_indentation(4, 4, "  \t ", 1, 2);
    test_indentation(4, 4, "  \t \t", 1, 3);
    test_indentation(4, 4, "  \t  ", 1, 2);
    test_indentation(4, 4, "  \t  \t", 1, 3);
    test_indentation(4, 4, "  \t   ", 1, 2);
    test_indentation(4, 4, "  \t   \t", 1, 3);
    test_indentation(4, 4, "  \t    ", 1, 3);
    test_indentation(4, 4, "   \t\t", 1, 3);
    test_indentation(4, 4, "   \t ", 1, 2);
    test_indentation(4, 4, "   \t \t", 1, 3);
    test_indentation(4, 4, "   \t  ", 1, 2);
    test_indentation(4, 4, "   \t  \t", 1, 3);
    test_indentation(4, 4, "   \t   ", 1, 2);
    test_indentation(4, 4, "   \t   \t", 1, 3);
    test_indentation(4, 4, "   \t    ", 1, 3);
    test_indentation(4, 4, "    \t", 1, 3);
    test_indentation(4, 4, "     ", 1, 2);
    test_indentation(4, 4, "     \t", 1, 3);
    test_indentation(4, 4, "      ", 1, 2);
    test_indentation(4, 4, "      \t", 1, 3);
    test_indentation(4, 4, "       ", 1, 2);
    test_indentation(4, 4, "       \t", 1, 3);
    test_indentation(4, 4, "        ", 1, 3);

    // 3 => 2
    test_indentation(4, 4, "         ", 2, 3);
}
