//! Porte de `src/vs/editor/contrib/indentation/test/browser/indentation.test.ts`, caso a caso e na mesma ordem.
//! Cada `suite` do original é um prefixo do nome dos testes; um `#[test]` por `test(...)`, e os `test.skip` do
//! original são `#[ignore]` com o corpo mantido.
//!
//! Notas de tradução:
//! - `Language`, `registerLanguage`, `registerLanguageConfiguration`, `registerTokenizationSupport` e
//!   `StandardTokenTypeData`, que o original exporta daqui, são o módulo `tests/support/indentation_test_utils.rs`
//!   (o `indentationLineProcessor.test.ts` também os importa); o `setup`/`teardown` das suítes de linguagem são a
//!   `IndentationFixture` e `run_test` (ver o topo do módulo).
//! - `testIndentationToSpacesCommand` e `testIndentationToTabsCommand` só diferem no comando que criam: são
//!   [`test_indentation_command`], com o [`IndentationConversion`] como argumento (o `IndentationCommand` funde os dois
//!   comandos do original). Cada caso chama o `testCommand` com `null` de linguagem e `ensureNoDisposablesAreLeaked`.
//! - `editor.registerAndInstantiateContribution(AutoIndentOnPaste.ID, AutoIndentOnPaste)` é
//!   [`register_auto_indent_on_paste`]: o serviço de configuração de linguagem que o construtor recebe por injeção é o
//!   do editor.
//! - `editor.executeCommands('editor.action.indentLines', TypeOperations.indent(viewModel.cursorConfig, model,
//!   selections))` dos casos ignorados é [`indent_lines`].
//! - `viewModel.model.setValue('')` é `model.set_value(...)` (o modelo da `viewModel` é o mesmo `model`).

#[path = "../../je-base/tests/common/mod.rs"]
mod common;
#[path = "support/indentation_test_utils.rs"]
mod indentation_test_utils;
mod support;

use std::rc::Rc;

use common::ensure_no_disposables_are_leaked_in_test_suite;
use je_base::JsString;
use je_editor::code_editor_widget::{EditorViewModel, ICodeEditor};
use je_editor::cursor_type_operations::TypeOperations;
use je_editor::encoded_token_attributes::standard_token_type::{COMMENT, OTHER, STRING};
use je_editor::indentation_contrib::{AutoIndentOnPaste, IndentationCommand, IndentationConversion};
use je_editor::range::Range;
use je_editor::selection::Selection;
use je_editor::text_model::{TextModel, TextSource};
use je_editor::view_model_types::IViewModel as _;
use support::test_code_editor::TestCodeEditor;
use support::test_command::test_command;

use indentation_test_utils::{Language, paste_from_keyboard, run_test, set_selection, token, type_keyboard, type_text, value_of};

fn test_indentation_command(
    conversion: IndentationConversion,
    lines: &[&str],
    selection: Selection,
    tab_size: i32,
    expected_lines: &[&str],
    expected_selection: Selection,
) {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    test_command(
        lines,
        None,
        selection,
        move |sel| Box::new(IndentationCommand::new(sel, tab_size, conversion)),
        expected_lines,
        expected_selection,
        false,
    );
}

/// `editor.registerAndInstantiateContribution(AutoIndentOnPaste.ID, AutoIndentOnPaste)`.
fn register_auto_indent_on_paste(editor: &TestCodeEditor) -> Rc<AutoIndentOnPaste> {
    editor.register_and_instantiate_contribution(AutoIndentOnPaste::ID, |editor, _instantiation_service| {
        AutoIndentOnPaste::new(editor, editor.language_configuration_service())
    })
}

/// `editor.executeCommands('editor.action.indentLines', TypeOperations.indent(viewModel.cursorConfig, editor.getModel(),
/// editor.getSelections()))`.
fn indent_lines(editor: &TestCodeEditor, view_model: &EditorViewModel, model: &Rc<TextModel>) {
    let commands = TypeOperations::indent(&view_model.cursor_config(), Some(&**model), editor.get_selections().as_deref())
        .into_iter()
        .map(Some)
        .collect();
    editor.execute_commands(Some("editor.action.indentLines"), commands);
}

// suite('Change Indentation to Spaces - TypeScript/Javascript')

#[test]
fn change_indentation_to_spaces_typescript_javascript_single_tabs_only_at_start_of_line() {
    test_indentation_command(
        IndentationConversion::ToSpaces,
        &["first", "second line", "third line", "\tfourth line", "\tfifth"],
        Selection::new(2, 3, 2, 3),
        4,
        &["first", "second line", "third line", "    fourth line", "    fifth"],
        Selection::new(2, 3, 2, 3),
    );
}

#[test]
fn change_indentation_to_spaces_typescript_javascript_multiple_tabs_at_start_of_line() {
    test_indentation_command(
        IndentationConversion::ToSpaces,
        &["\t\tfirst", "\tsecond line", "\t\t\t third line", "fourth line", "fifth"],
        Selection::new(1, 5, 1, 5),
        3,
        &["      first", "   second line", "          third line", "fourth line", "fifth"],
        Selection::new(1, 9, 1, 9),
    );
}

#[test]
fn change_indentation_to_spaces_typescript_javascript_multiple_tabs() {
    test_indentation_command(
        IndentationConversion::ToSpaces,
        &["\t\tfirst\t", "\tsecond  \t line \t", "\t\t\t third line", " \tfourth line", "fifth"],
        Selection::new(1, 5, 1, 5),
        2,
        &["    first\t", "  second  \t line \t", "       third line", "   fourth line", "fifth"],
        Selection::new(1, 7, 1, 7),
    );
}

#[test]
fn change_indentation_to_spaces_typescript_javascript_empty_lines() {
    test_indentation_command(
        IndentationConversion::ToSpaces,
        &["\t\t\t", "\t", "\t\t"],
        Selection::new(1, 4, 1, 4),
        2,
        &["      ", "  ", "    "],
        Selection::new(1, 4, 1, 4),
    );
}

// suite('Change Indentation to Tabs -  TypeScript/Javascript')

#[test]
fn change_indentation_to_tabs_typescript_javascript_spaces_only_at_start_of_line() {
    test_indentation_command(
        IndentationConversion::ToTabs,
        &["    first", "second line", "    third line", "fourth line", "fifth"],
        Selection::new(2, 3, 2, 3),
        4,
        &["\tfirst", "second line", "\tthird line", "fourth line", "fifth"],
        Selection::new(2, 3, 2, 3),
    );
}

#[test]
fn change_indentation_to_tabs_typescript_javascript_multiple_spaces_at_start_of_line() {
    test_indentation_command(
        IndentationConversion::ToTabs,
        &["first", "   second line", "          third line", "fourth line", "     fifth"],
        Selection::new(1, 5, 1, 5),
        3,
        &["first", "\tsecond line", "\t\t\t third line", "fourth line", "\t  fifth"],
        Selection::new(1, 5, 1, 5),
    );
}

#[test]
fn change_indentation_to_tabs_typescript_javascript_multiple_spaces() {
    test_indentation_command(
        IndentationConversion::ToTabs,
        &["      first   ", "  second     line \t", "       third line", "   fourth line", "fifth"],
        Selection::new(1, 8, 1, 8),
        2,
        &["\t\t\tfirst   ", "\tsecond     line \t", "\t\t\t third line", "\t fourth line", "fifth"],
        Selection::new(1, 5, 1, 5),
    );
}

#[test]
fn change_indentation_to_tabs_typescript_javascript_issue_45996() {
    test_indentation_command(
        IndentationConversion::ToSpaces,
        &["\tabc"],
        Selection::new(1, 3, 1, 3),
        4,
        &["    abc"],
        Selection::new(1, 6, 1, 6),
    );
}

// suite('Indent With Tab - TypeScript/JavaScript')

#[test]
fn indent_with_tab_typescript_javascript_temp_issue_because_there_should_be_at_least_one_passing_test_in_a_suite() {
    run_test(Language::TypeScript, |_f| {});
}

#[test]
#[ignore = "test.skip no original"]
fn indent_with_tab_typescript_javascript_issue_63388_perserve_correct_indentation_on_tab_1() {
    run_test(Language::TypeScript, |f| {
        // https://github.com/microsoft/vscode/issues/63388

        let model = f.create_text_model(&["/*", " * Comment", " * /"].join("\n"));

        f.with_test_code_editor(&model, "full", |editor, view_model, _instantiation_service| {
            set_selection(editor, 1, 1, 3, 5);
            indent_lines(editor, view_model, &model);
            assert_eq!(value_of(&model), ["    /*", "     * Comment", "     * /"].join("\n"));
        });
    });
}

#[test]
#[ignore = "test.skip no original"]
fn indent_with_tab_typescript_javascript_issue_63388_perserve_correct_indentation_on_tab_2() {
    run_test(Language::TypeScript, |f| {
        // https://github.com/microsoft/vscode/issues/63388

        let model = f.create_text_model(&["switch (something) {", "  case 1:", "    whatever();", "    break;", "}"].join("\n"));

        f.with_test_code_editor(&model, "full", |editor, view_model, _instantiation_service| {
            set_selection(editor, 1, 1, 5, 2);
            indent_lines(editor, view_model, &model);
            assert_eq!(
                value_of(&model),
                ["    switch (something) {", "        case 1:", "            whatever();", "            break;", "    }"].join("\n")
            );
        });
    });
}

// suite('Auto Indent On Paste - TypeScript/JavaScript')

#[test]
fn auto_indent_on_paste_typescript_javascript_issue_119225_do_not_add_extra_leading_space_when_pasting_jsdoc() {
    run_test(Language::TypeScript, |f| {
        let model = f.create_text_model("");

        f.with_test_code_editor(&model, "full", |editor, view_model, _instantiation_service| {
            let paste_text = ["/**", " * JSDoc", " */", "function a() {}"].join("\n");
            let tokens = vec![
                vec![token(0, COMMENT), token(3, COMMENT)],
                vec![token(0, COMMENT), token(2, COMMENT), token(8, COMMENT)],
                vec![token(0, COMMENT), token(1, COMMENT), token(3, OTHER)],
                vec![
                    token(0, OTHER),
                    token(8, OTHER),
                    token(9, OTHER),
                    token(10, OTHER),
                    token(11, OTHER),
                    token(12, OTHER),
                    token(13, OTHER),
                    token(14, OTHER),
                    token(15, OTHER),
                ],
            ];
            f.register_tokenization_support(tokens);
            let auto_indent_on_paste_controller = register_auto_indent_on_paste(editor);
            paste_from_keyboard(view_model, &paste_text);
            auto_indent_on_paste_controller.trigger(Range::new(1, 1, 4, 16));
            assert_eq!(value_of(&model), paste_text);
        });
    });
}

#[test]
fn auto_indent_on_paste_typescript_javascript_issue_167299_blank_line_removes_indent() {
    run_test(Language::TypeScript, |f| {
        let model = f.create_text_model("");

        f.with_test_code_editor(&model, "full", |editor, view_model, _instantiation_service| {
            // no need for tokenization because there are no comments
            let paste_text = [
                "",
                "export type IncludeReference =",
                "\t| BaseReference",
                "\t| SelfReference",
                "\t| RelativeReference;",
                "",
                "export const enum IncludeReferenceKind {",
                "\tBase,",
                "\tSelf,",
                "\tRelativeReference,",
                "}",
            ]
            .join("\n");

            let auto_indent_on_paste_controller = register_auto_indent_on_paste(editor);
            paste_from_keyboard(view_model, &paste_text);
            auto_indent_on_paste_controller.trigger(Range::new(1, 1, 11, 2));
            assert_eq!(value_of(&model), paste_text);
        });
    });
}

#[test]
fn auto_indent_on_paste_typescript_javascript_issue_29803_do_not_indent_when_pasting_text_with_only_one_line() {
    run_test(Language::TypeScript, |f| {
        // https://github.com/microsoft/vscode/issues/29803

        let model = f.create_text_model(&["const linkHandler = new Class(a, b, c,", "    d)"].join("\n"));

        f.with_test_code_editor(&model, "full", |editor, view_model, _instantiation_service| {
            set_selection(editor, 2, 6, 2, 6);
            let text = ", null";
            paste_from_keyboard(view_model, text);
            let auto_indent_on_paste_controller = register_auto_indent_on_paste(editor);
            auto_indent_on_paste_controller.trigger(Range::new(2, 6, 2, 11));
            assert_eq!(value_of(&model), ["const linkHandler = new Class(a, b, c,", "    d, null)"].join("\n"));
        });
    });
}

#[test]
fn auto_indent_on_paste_typescript_javascript_issue_29753_incorrect_indentation_after_comment() {
    run_test(Language::TypeScript, |f| {
        // https://github.com/microsoft/vscode/issues/29753

        let model = f.create_text_model(
            &["class A {", "    /**", "     * used only for debug purposes.", "     */", "    private _codeInfo: KeyMapping[];", "}"].join("\n"),
        );

        f.with_test_code_editor(&model, "full", |editor, view_model, _instantiation_service| {
            set_selection(editor, 5, 24, 5, 34);
            let text = "IMacLinuxKeyMapping";
            paste_from_keyboard(view_model, text);
            let auto_indent_on_paste_controller = register_auto_indent_on_paste(editor);
            auto_indent_on_paste_controller.trigger(Range::new(5, 24, 5, 43));
            assert_eq!(
                value_of(&model),
                [
                    "class A {",
                    "    /**",
                    "     * used only for debug purposes.",
                    "     */",
                    "    private _codeInfo: IMacLinuxKeyMapping[];",
                    "}",
                ]
                .join("\n")
            );
        });
    });
}

#[test]
fn auto_indent_on_paste_typescript_javascript_issue_29753_incorrect_indentation_of_header_comment() {
    run_test(Language::TypeScript, |f| {
        // https://github.com/microsoft/vscode/issues/29753

        let model = f.create_text_model("");

        f.with_test_code_editor(&model, "full", |editor, view_model, _instantiation_service| {
            let text = ["/*----------------", " *  Copyright (c) ", " *  Licensed under ...", " *-----------------*/"].join("\n");
            paste_from_keyboard(view_model, &text);
            let auto_indent_on_paste_controller = register_auto_indent_on_paste(editor);
            auto_indent_on_paste_controller.trigger(Range::new(1, 1, 4, 22));
            assert_eq!(value_of(&model), text);
        });
    });
}

#[test]
fn auto_indent_on_paste_typescript_javascript_issue_209859_do_not_do_change_indentation_when_pasted_inside_of_a_string() {
    run_test(Language::TypeScript, |f| {
        // issue: https://github.com/microsoft/vscode/issues/209859
        // issue: https://github.com/microsoft/vscode/issues/209418

        let initial_text = ["const foo = \"some text", "         which is strangely", "    indented\""].join("\n");
        let model = f.create_text_model(&initial_text);

        f.with_test_code_editor(&model, "full", |editor, view_model, _instantiation_service| {
            let tokens = vec![
                vec![token(0, OTHER), token(12, STRING)],
                vec![token(0, STRING)],
                vec![token(0, STRING)],
            ];
            f.register_tokenization_support(tokens);

            set_selection(editor, 2, 10, 2, 15);
            paste_from_keyboard(view_model, "which");
            let auto_indent_on_paste_controller = register_auto_indent_on_paste(editor);
            auto_indent_on_paste_controller.trigger(Range::new(2, 1, 2, 28));
            assert_eq!(value_of(&model), initial_text);
        });
    });
}

// Failing tests found in issues...

#[test]
#[ignore = "test.skip no original"]
fn auto_indent_on_paste_typescript_javascript_issue_181065_incorrect_paste_of_object_within_comment() {
    run_test(Language::TypeScript, |f| {
        // https://github.com/microsoft/vscode/issues/181065

        let model = f.create_text_model("");

        f.with_test_code_editor(&model, "full", |editor, view_model, _instantiation_service| {
            let text = ["/**", " * @typedef {", " * }", " */"].join("\n");
            let tokens = vec![
                vec![token(0, COMMENT), token(3, COMMENT)],
                vec![
                    token(0, COMMENT),
                    token(2, COMMENT),
                    token(3, COMMENT),
                    token(11, COMMENT),
                    token(12, OTHER),
                    token(13, OTHER),
                ],
                vec![token(0, COMMENT), token(2, OTHER), token(3, OTHER), token(4, OTHER)],
                vec![token(0, COMMENT), token(1, COMMENT), token(3, OTHER)],
            ];
            f.register_tokenization_support(tokens);
            let auto_indent_on_paste_controller = register_auto_indent_on_paste(editor);
            paste_from_keyboard(view_model, &text);
            auto_indent_on_paste_controller.trigger(Range::new(1, 1, 4, 4));
            assert_eq!(value_of(&model), text);
        });
    });
}

#[test]
#[ignore = "test.skip no original"]
fn auto_indent_on_paste_typescript_javascript_issue_86301_preserve_cursor_at_inserted_indentation_level() {
    run_test(Language::TypeScript, |f| {
        // https://github.com/microsoft/vscode/issues/86301

        let model = f.create_text_model(&["() => {", "", "}"].join("\n"));

        f.with_test_code_editor(&model, "full", |editor, view_model, _instantiation_service| {
            set_selection(editor, 2, 1, 2, 1);
            let text = ["() => {", "", "}", ""].join("\n");
            let auto_indent_on_paste_controller = register_auto_indent_on_paste(editor);
            paste_from_keyboard(view_model, &text);
            auto_indent_on_paste_controller.trigger(Range::new(2, 1, 5, 1));

            // notes:
            // why is line 3 not indented to the same level as line 2?
            // looks like the indentation is inserted correctly at line 5, but the cursor does not appear at the maximum indentation level?
            assert_eq!(
                value_of(&model),
                [
                    "() => {",
                    "    () => {",
                    "    ", // <- should also be indented
                    "    }",
                    "    ", // <- cursor should be at the end of the indentation
                    "}",
                ]
                .join("\n")
            );

            let selection = view_model.get_selection();
            assert_eq!(selection, Selection::new(5, 5, 5, 5));
        });
    });
}

#[test]
#[ignore = "test.skip no original"]
fn auto_indent_on_paste_typescript_javascript_issue_85781_indent_line_with_extra_white_space() {
    run_test(Language::TypeScript, |f| {
        // https://github.com/microsoft/vscode/issues/85781
        // note: still to determine whether this is a bug or not

        let model = f.create_text_model(&["() => {", "    console.log(\"a\");", "}"].join("\n"));

        f.with_test_code_editor(&model, "full", |editor, view_model, _instantiation_service| {
            set_selection(editor, 2, 5, 2, 5);
            let text = ["() => {", "    console.log(\"b\")", "}", " "].join("\n");
            let auto_indent_on_paste_controller = register_auto_indent_on_paste(editor);
            paste_from_keyboard(view_model, &text);
            // todo@aiday-mar, make sure range is correct, and make test work as in real life
            auto_indent_on_paste_controller.trigger(Range::new(2, 5, 5, 6));
            assert_eq!(
                value_of(&model),
                ["() => {", "    () => {", "        console.log(\"b\")", "    }", "    console.log(\"a\");", "}"].join("\n")
            );
        });
    });
}

#[test]
#[ignore = "test.skip no original"]
fn auto_indent_on_paste_typescript_javascript_issue_29589_incorrect_indentation_of_closing_brace_on_paste() {
    run_test(Language::TypeScript, |f| {
        // https://github.com/microsoft/vscode/issues/29589

        let model = f.create_text_model("");

        f.with_test_code_editor(&model, "full", |editor, view_model, _instantiation_service| {
            set_selection(editor, 2, 5, 2, 5);
            let text = ["function makeSub(a,b) {", "subsent = sent.substring(a,b);", "return subsent;", "}"].join("\n");
            let auto_indent_on_paste_controller = register_auto_indent_on_paste(editor);
            paste_from_keyboard(view_model, &text);
            // todo@aiday-mar, make sure range is correct, and make test work as in real life
            auto_indent_on_paste_controller.trigger(Range::new(1, 1, 4, 2));
            assert_eq!(
                value_of(&model),
                ["function makeSub(a,b) {", "subsent = sent.substring(a,b);", "return subsent;", "}"].join("\n")
            );
        });
    });
}

#[test]
#[ignore = "test.skip no original"]
fn auto_indent_on_paste_typescript_javascript_issue_201420_incorrect_indentation_when_first_line_is_comment() {
    run_test(Language::TypeScript, |f| {
        // https://github.com/microsoft/vscode/issues/201420

        let model = f.create_text_model(&["function bar() {", "", "}"].join("\n"));

        f.with_test_code_editor(&model, "full", |editor, view_model, _instantiation_service| {
            let tokens = vec![
                vec![
                    token(0, OTHER),
                    token(8, OTHER),
                    token(9, OTHER),
                    token(12, OTHER),
                    token(13, OTHER),
                    token(14, OTHER),
                    token(15, OTHER),
                    token(16, OTHER),
                ],
                vec![token(0, COMMENT), token(2, COMMENT), token(3, COMMENT), token(10, COMMENT)],
                vec![
                    token(0, OTHER),
                    token(5, OTHER),
                    token(6, OTHER),
                    token(9, OTHER),
                    token(10, OTHER),
                    token(11, OTHER),
                    token(12, OTHER),
                    token(14, OTHER),
                ],
                vec![token(0, OTHER), token(1, OTHER)],
            ];
            f.register_tokenization_support(tokens);

            set_selection(editor, 2, 1, 2, 1);
            let text = ["// comment", "const foo = 42"].join("\n");
            let auto_indent_on_paste_controller = register_auto_indent_on_paste(editor);
            paste_from_keyboard(view_model, &text);
            auto_indent_on_paste_controller.trigger(Range::new(2, 1, 3, 15));
            assert_eq!(
                value_of(&model),
                ["function bar() {", "    // comment", "    const foo = 42", "}"].join("\n")
            );
        });
    });
}

// suite('Auto Indent On Type - TypeScript/JavaScript')

// Failing tests from issues...

#[test]
fn auto_indent_on_type_typescript_javascript_issue_208215_indent_after_arrow_function() {
    run_test(Language::TypeScript, |f| {
        // https://github.com/microsoft/vscode/issues/208215

        let model = f.create_text_model("");

        f.with_test_code_editor(&model, "full", |_editor, view_model, _instantiation_service| {
            type_text(view_model, "const add1 = (n) =>");
            type_keyboard(view_model, "\n");
            assert_eq!(value_of(&model), ["const add1 = (n) =>", "    "].join("\n"));
        });
    });
}

#[test]
fn auto_indent_on_type_typescript_javascript_issue_208215_indent_after_arrow_function_2() {
    run_test(Language::TypeScript, |f| {
        // https://github.com/microsoft/vscode/issues/208215

        let model = f.create_text_model(&["const array = [1, 2, 3, 4, 5];", "array.map(", "    v =>"].join("\n"));

        f.with_test_code_editor(&model, "full", |editor, view_model, _instantiation_service| {
            set_selection(editor, 3, 9, 3, 9);
            type_keyboard(view_model, "\n");
            assert_eq!(
                value_of(&model),
                ["const array = [1, 2, 3, 4, 5];", "array.map(", "    v =>", "        "].join("\n")
            );
        });
    });
}

#[test]
fn auto_indent_on_type_typescript_javascript_issue_116843_indent_after_arrow_function() {
    run_test(Language::TypeScript, |f| {
        // https://github.com/microsoft/vscode/issues/116843

        let model = f.create_text_model("");

        f.with_test_code_editor(&model, "full", |_editor, view_model, _instantiation_service| {
            type_text(view_model, &["const add1 = (n) =>", "    n + 1;"].join("\n"));
            type_keyboard(view_model, "\n");
            assert_eq!(value_of(&model), ["const add1 = (n) =>", "    n + 1;", ""].join("\n"));
        });
    });
}

#[test]
fn auto_indent_on_type_typescript_javascript_issue_29755_do_not_add_indentation_on_enter_if_indentation_is_already_valid() {
    run_test(Language::TypeScript, |f| {
        //https://github.com/microsoft/vscode/issues/29755

        let model = f.create_text_model(&["function f() {", "    const one = 1;", "    const two = 2;", "}"].join("\n"));

        f.with_test_code_editor(&model, "full", |editor, view_model, _instantiation_service| {
            set_selection(editor, 3, 1, 3, 1);
            type_keyboard(view_model, "\n");
            assert_eq!(
                value_of(&model),
                ["function f() {", "    const one = 1;", "", "    const two = 2;", "}"].join("\n")
            );
        });
    });
}

#[test]
fn auto_indent_on_type_typescript_javascript_issue_36090() {
    run_test(Language::TypeScript, |f| {
        // https://github.com/microsoft/vscode/issues/36090

        let model = f.create_text_model(
            &[
                "class ItemCtrl {",
                "    getPropertiesByItemId(id) {",
                "        return this.fetchItem(id)",
                "            .then(item => {",
                "                return this.getPropertiesOfItem(item);",
                "            });",
                "    }",
                "}",
            ]
            .join("\n"),
        );

        f.with_test_code_editor(&model, "advanced", |editor, view_model, _instantiation_service| {
            set_selection(editor, 7, 6, 7, 6);
            type_keyboard(view_model, "\n");
            assert_eq!(
                value_of(&model),
                [
                    "class ItemCtrl {",
                    "    getPropertiesByItemId(id) {",
                    "        return this.fetchItem(id)",
                    "            .then(item => {",
                    "                return this.getPropertiesOfItem(item);",
                    "            });",
                    "    }",
                    "    ",
                    "}",
                ]
                .join("\n")
            );
            assert_eq!(editor.get_selection(), Some(Selection::new(8, 5, 8, 5)));
        });
    });
}

#[test]
fn auto_indent_on_type_typescript_javascript_issue_115304_indent_block_comment_on_enter() {
    run_test(Language::TypeScript, |f| {
        // https://github.com/microsoft/vscode/issues/115304

        let model = f.create_text_model(&["/** */", "function f() {}"].join("\n"));

        f.with_test_code_editor(&model, "advanced", |editor, view_model, _instantiation_service| {
            set_selection(editor, 1, 4, 1, 4);
            type_keyboard(view_model, "\n");
            assert_eq!(value_of(&model), ["/**", " * ", " */", "function f() {}"].join("\n"));
            assert_eq!(editor.get_selection(), Some(Selection::new(2, 4, 2, 4)));
        });
    });
}

#[test]
fn auto_indent_on_type_typescript_javascript_issue_43244_indent_when_lambda_arrow_function_is_detected_outdent_when_end_is_reached() {
    run_test(Language::TypeScript, |f| {
        // https://github.com/microsoft/vscode/issues/43244

        let model = f.create_text_model(&["const array = [1, 2, 3, 4, 5];", "array.map(_)"].join("\n"));

        f.with_test_code_editor(&model, "full", |editor, view_model, _instantiation_service| {
            set_selection(editor, 2, 12, 2, 12);
            type_keyboard(view_model, "\n");
            assert_eq!(
                value_of(&model),
                ["const array = [1, 2, 3, 4, 5];", "array.map(_", "    ", ")"].join("\n")
            );
        });
    });
}

#[test]
fn auto_indent_on_type_typescript_javascript_issue_43244_incorrect_indentation_after_if_for_while_without_braces() {
    run_test(Language::TypeScript, |f| {
        // https://github.com/microsoft/vscode/issues/43244

        let model = f.create_text_model(&["function f() {", "    if (condition)", "}"].join("\n"));

        f.with_test_code_editor(&model, "full", |editor, view_model, _instantiation_service| {
            set_selection(editor, 2, 19, 2, 19);
            type_keyboard(view_model, "\n");
            assert_eq!(
                value_of(&model),
                ["function f() {", "    if (condition)", "        ", "}"].join("\n")
            );

            type_text(view_model, "return;");
            type_keyboard(view_model, "\n");
            assert_eq!(
                value_of(&model),
                ["function f() {", "    if (condition)", "        return;", "    ", "}"].join("\n")
            );
        });
    });
}

#[test]
fn auto_indent_on_type_typescript_javascript_issue_208232_incorrect_indentation_inside_of_comments() {
    run_test(Language::TypeScript, |f| {
        // https://github.com/microsoft/vscode/issues/208232

        let model = f.create_text_model(&["/**", "indentation done for {", "*/"].join("\n"));

        f.with_test_code_editor(&model, "full", |editor, view_model, _instantiation_service| {
            let tokens = vec![vec![token(0, COMMENT)], vec![token(0, COMMENT)], vec![token(0, COMMENT)]];
            f.register_tokenization_support(tokens);
            set_selection(editor, 2, 23, 2, 23);
            type_keyboard(view_model, "\n");
            assert_eq!(value_of(&model), ["/**", "indentation done for {", "", "*/"].join("\n"));
        });
    });
}

#[test]
fn auto_indent_on_type_typescript_javascript_issue_209802_allman_style_braces_in_javascript() {
    run_test(Language::TypeScript, |f| {
        // https://github.com/microsoft/vscode/issues/209802

        let model = f.create_text_model(&["if (/*condition*/)"].join("\n"));

        f.with_test_code_editor(&model, "full", |editor, view_model, _instantiation_service| {
            set_selection(editor, 1, 19, 1, 19);
            type_keyboard(view_model, "\n");
            assert_eq!(value_of(&model), ["if (/*condition*/)", "    "].join("\n"));
            type_keyboard(view_model, "{");
            assert_eq!(value_of(&model), ["if (/*condition*/)", "{}"].join("\n"));
            set_selection(editor, 2, 2, 2, 2);
            type_keyboard(view_model, "\n");
            assert_eq!(value_of(&model), ["if (/*condition*/)", "{", "    ", "}"].join("\n"));
        });
    });
}

#[test]
fn auto_indent_on_type_typescript_javascript_issue_225916_surrounding_with_brackets_works_after_arrow_function() {
    run_test(Language::TypeScript, |f| {
        // https://github.com/microsoft/vscode/issues/225916

        let bracket_pairs = [("{", "}"), ("[", "]"), ("(", ")")];

        for (open, close) in bracket_pairs {
            let model = f.create_text_model(&["const fn = () =>", "    doSomething()"].join("\n"));

            f.with_test_code_editor(&model, "full", |editor, view_model, _instantiation_service| {
                set_selection(editor, 2, 5, 2, 18);
                type_keyboard(view_model, open);
                assert_eq!(
                    value_of(&model),
                    ["const fn = () =>".to_owned(), format!("    {open}doSomething(){close}")].join("\n"),
                    "failed to surround selection with {open}{close}"
                );
            });
        }
    });
}

// Failing tests...

#[test]
#[ignore = "test.skip no original"]
fn auto_indent_on_type_typescript_javascript_issue_43244_indent_after_equal_sign_is_detected() {
    run_test(Language::TypeScript, |f| {
        // https://github.com/microsoft/vscode/issues/43244
        // issue: Should indent after an equal sign is detected followed by whitespace characters.
        // This should be outdented when a semi-colon is detected indicating the end of the assignment.

        // TODO: requires exploring indent/outdent pairs instead

        let model = f.create_text_model(&["const array ="].join("\n"));

        f.with_test_code_editor(&model, "full", |editor, view_model, _instantiation_service| {
            set_selection(editor, 1, 14, 1, 14);
            type_keyboard(view_model, "\n");
            assert_eq!(value_of(&model), ["const array =", "    "].join("\n"));
        });
    });
}

#[test]
#[ignore = "test.skip no original"]
fn auto_indent_on_type_typescript_javascript_issue_43244_indent_after_dot_detected_after_object_array_signifying_a_method_call() {
    run_test(Language::TypeScript, |f| {
        // https://github.com/microsoft/vscode/issues/43244
        // issue: When a dot is written, we should detect that this is a method call and indent accordingly

        // TODO: requires exploring indent/outdent pairs instead

        let model = f.create_text_model(&["const array = [1, 2, 3];", "array."].join("\n"));

        f.with_test_code_editor(&model, "full", |editor, view_model, _instantiation_service| {
            set_selection(editor, 2, 7, 2, 7);
            type_keyboard(view_model, "\n");
            assert_eq!(value_of(&model), ["const array = [1, 2, 3];", "array.", "    "].join("\n"));
        });
    });
}

#[test]
#[ignore = "test.skip no original"]
fn auto_indent_on_type_typescript_javascript_issue_43244_indent_after_dot_detected_on_a_subsequent_line_after_object_array_signifying_a_method_call() {
    run_test(Language::TypeScript, |f| {
        // https://github.com/microsoft/vscode/issues/43244
        // issue: When a dot is written, we should detect that this is a method call and indent accordingly

        // TODO: requires exploring indent/outdent pairs instead

        let model = f.create_text_model(&["const array = [1, 2, 3]"].join("\n"));

        f.with_test_code_editor(&model, "full", |editor, view_model, _instantiation_service| {
            set_selection(editor, 2, 7, 2, 7);
            type_keyboard(view_model, "\n");
            type_text(view_model, ".");
            assert_eq!(value_of(&model), ["const array = [1, 2, 3]", "    ."].join("\n"));
        });
    });
}

#[test]
#[ignore = "test.skip no original"]
fn auto_indent_on_type_typescript_javascript_issue_43244_keep_indentation_when_methods_called_on_object_array() {
    run_test(Language::TypeScript, |f| {
        // https://github.com/microsoft/vscode/issues/43244
        // Currently passes, but should pass with all the tests above too

        // TODO: requires exploring indent/outdent pairs instead

        let model = f.create_text_model(&["const array = [1, 2, 3]", "    .filter(() => true)"].join("\n"));

        f.with_test_code_editor(&model, "full", |editor, view_model, _instantiation_service| {
            set_selection(editor, 2, 24, 2, 24);
            type_keyboard(view_model, "\n");
            assert_eq!(
                value_of(&model),
                ["const array = [1, 2, 3]", "    .filter(() => true)", "    "].join("\n")
            );
        });
    });
}

#[test]
#[ignore = "test.skip no original"]
fn auto_indent_on_type_typescript_javascript_issue_43244_keep_indentation_when_chained_methods_called_on_object_array() {
    run_test(Language::TypeScript, |f| {
        // https://github.com/microsoft/vscode/issues/43244
        // When the call chain is not finished yet, and we type a dot, we do not want to change the indentation

        // TODO: requires exploring indent/outdent pairs instead

        let model = f.create_text_model(&["const array = [1, 2, 3]", "    .filter(() => true)", "    "].join("\n"));

        f.with_test_code_editor(&model, "full", |editor, view_model, _instantiation_service| {
            set_selection(editor, 3, 5, 3, 5);
            type_text(view_model, ".");
            assert_eq!(
                value_of(&model),
                [
                    "const array = [1, 2, 3]",
                    "    .filter(() => true)",
                    "    ." // here we don't want to increase the indentation because we have chained methods
                ]
                .join("\n")
            );
        });
    });
}

#[test]
#[ignore = "test.skip no original"]
fn auto_indent_on_type_typescript_javascript_issue_43244_outdent_when_a_semi_color_is_detected_indicating_the_end_of_the_assignment() {
    run_test(Language::TypeScript, |f| {
        // https://github.com/microsoft/vscode/issues/43244

        // TODO: requires exploring indent/outdent pairs instead

        let model = f.create_text_model(&["const array = [1, 2, 3]", "    .filter(() => true);"].join("\n"));

        f.with_test_code_editor(&model, "full", |editor, view_model, _instantiation_service| {
            set_selection(editor, 2, 25, 2, 25);
            type_keyboard(view_model, "\n");
            assert_eq!(
                value_of(&model),
                ["const array = [1, 2, 3]", "    .filter(() => true);", ""].join("\n")
            );
        });
    });
}

#[test]
#[ignore = "test.skip no original"]
fn auto_indent_on_type_typescript_javascript_issue_40115_keep_indentation_when_added() {
    run_test(Language::TypeScript, |f| {
        // https://github.com/microsoft/vscode/issues/40115

        let model = f.create_text_model("function foo() {}");

        f.with_test_code_editor(&model, "full", |editor, view_model, _instantiation_service| {
            set_selection(editor, 1, 17, 1, 17);
            type_keyboard(view_model, "\n");
            assert_eq!(value_of(&model), ["function foo() {", "    ", "}"].join("\n"));
            set_selection(editor, 2, 5, 2, 5);
            type_keyboard(view_model, "\n");
            assert_eq!(value_of(&model), ["function foo() {", "    ", "    ", "}"].join("\n"));
        });
    });
}

#[test]
#[ignore = "test.skip no original"]
fn auto_indent_on_type_typescript_javascript_issue_193875_incorrect_indentation_on_enter() {
    run_test(Language::TypeScript, |f| {
        // https://github.com/microsoft/vscode/issues/193875

        let model = f.create_text_model(&["{", "    for(;;)", "    for(;;) {}", "}"].join("\n"));

        f.with_test_code_editor(&model, "full", |editor, view_model, _instantiation_service| {
            set_selection(editor, 3, 14, 3, 14);
            type_keyboard(view_model, "\n");
            assert_eq!(
                value_of(&model),
                ["{", "    for(;;)", "    for(;;) {", "        ", "    }", "}"].join("\n")
            );
        });
    });
}

#[test]
#[ignore = "test.skip no original"]
fn auto_indent_on_type_typescript_javascript_issue_67678_indent_on_typing_curly_brace() {
    run_test(Language::TypeScript, |f| {
        // https://github.com/microsoft/vscode/issues/67678

        let model = f.create_text_model(&["if (true) {", "console.log(\"a\")", "console.log(\"b\")", ""].join("\n"));

        f.with_test_code_editor(&model, "full", |editor, view_model, _instantiation_service| {
            set_selection(editor, 4, 1, 4, 1);
            type_keyboard(view_model, "}");
            assert_eq!(
                value_of(&model),
                ["if (true) {", "    console.log(\"a\")", "    console.log(\"b\")", "}"].join("\n")
            );
        });
    });
}

#[test]
#[ignore = "test.skip no original"]
fn auto_indent_on_type_typescript_javascript_issue_46401_outdent_when_encountering_bracket_on_line_allman_style_indentation() {
    run_test(Language::TypeScript, |f| {
        // https://github.com/microsoft/vscode/issues/46401

        let model = f.create_text_model(&["if (true)", "    "].join("\n"));

        f.with_test_code_editor(&model, "full", |editor, view_model, _instantiation_service| {
            set_selection(editor, 2, 5, 2, 5);
            type_keyboard(view_model, "{}");
            assert_eq!(value_of(&model), ["if (true)", "{}"].join("\n"));
            set_selection(editor, 2, 2, 2, 2);
            type_keyboard(view_model, "\n");
            assert_eq!(value_of(&model), ["if (true)", "{", "    ", "}"].join("\n"));
        });
    });
}

#[test]
#[ignore = "test.skip no original"]
fn auto_indent_on_type_typescript_javascript_issue_125261_typing_closing_brace_does_not_keep_the_current_indentation() {
    run_test(Language::TypeScript, |f| {
        // https://github.com/microsoft/vscode/issues/125261

        let model = f.create_text_model(&["foo {", "    "].join("\n"));

        f.with_test_code_editor(&model, "keep", |editor, view_model, _instantiation_service| {
            set_selection(editor, 2, 5, 2, 5);
            type_keyboard(view_model, "}");
            assert_eq!(value_of(&model), ["foo {", "}"].join("\n"));
        });
    });
}

// suite('Auto Indent On Type - Ruby')

#[test]
fn auto_indent_on_type_ruby_issue_198350_in_or_when_incorrectly_match_non_keywords_for_ruby() {
    run_test(Language::Ruby, |f| {
        // https://github.com/microsoft/vscode/issues/198350

        let model = f.create_text_model("");

        f.with_test_code_editor(&model, "full", |_editor, view_model, _instantiation_service| {
            type_text(view_model, "def foo\n        i");
            type_keyboard(view_model, "n");
            assert_eq!(value_of(&model), "def foo\n        in");
            type_keyboard(view_model, " ");
            assert_eq!(value_of(&model), "def foo\nin ");

            model.set_value(TextSource::Text(JsString::new()));
            type_text(view_model, "  # in");
            assert_eq!(value_of(&model), "  # in");
            type_keyboard(view_model, " ");
            assert_eq!(value_of(&model), "  # in ");
        });
    });
}

// Failing tests...

#[test]
#[ignore = "test.skip no original"]
fn auto_indent_on_type_ruby_issue_199846_in_or_when_incorrectly_match_non_keywords_for_ruby() {
    run_test(Language::Ruby, |f| {
        // https://github.com/microsoft/vscode/issues/199846
        // explanation: happening because the # is detected probably as a comment

        let model = f.create_text_model("");

        f.with_test_code_editor(&model, "full", |_editor, view_model, _instantiation_service| {
            type_text(view_model, "method('#foo') do");
            type_keyboard(view_model, "\n");
            assert_eq!(value_of(&model), ["method('#foo') do", "    "].join("\n"));
        });
    });
}

// suite('Auto Indent On Type - PHP')

#[test]
fn auto_indent_on_type_php_issue_199050_should_not_indent_after_detected_in_a_string() {
    run_test(Language::Php, |f| {
        // https://github.com/microsoft/vscode/issues/199050

        let model = f.create_text_model("preg_replace('{');");

        f.with_test_code_editor(&model, "full", |editor, view_model, _instantiation_service| {
            let tokens = vec![vec![token(0, OTHER), token(13, STRING), token(16, OTHER)]];
            f.register_tokenization_support(tokens);
            set_selection(editor, 1, 54, 1, 54);
            type_keyboard(view_model, "\n");
            assert_eq!(value_of(&model), ["preg_replace('{');", ""].join("\n"));
        });
    });
}

// suite('Auto Indent On Paste - Go')

#[test]
fn auto_indent_on_paste_go_temp_issue_because_there_should_be_at_least_one_passing_test_in_a_suite() {
    run_test(Language::Go, |_f| {});
}

#[test]
#[ignore = "test.skip no original"]
fn auto_indent_on_paste_go_issue_199050_should_not_indent_after_detected_in_a_string() {
    run_test(Language::Go, |f| {
        // https://github.com/microsoft/vscode/issues/199050

        let model = f.create_text_model(&["var s = `", "quick  brown", "fox", "`"].join("\n"));

        f.with_test_code_editor(&model, "full", |editor, view_model, _instantiation_service| {
            set_selection(editor, 3, 1, 3, 1);
            let text = "  ";
            let auto_indent_on_paste_controller = register_auto_indent_on_paste(editor);
            paste_from_keyboard(view_model, text);
            auto_indent_on_paste_controller.trigger(Range::new(3, 1, 3, 3));
            assert_eq!(value_of(&model), ["var s = `", "quick  brown", "  fox", "`"].join("\n"));
        });
    });
}

// suite('Auto Indent On Type - CPP')

#[test]
fn auto_indent_on_type_cpp_temp_issue_because_there_should_be_at_least_one_passing_test_in_a_suite() {
    run_test(Language::Cpp, |_f| {});
}

#[test]
#[ignore = "test.skip no original"]
fn auto_indent_on_type_cpp_issue_178334_incorrect_outdent_of_when_signature_spans_multiple_lines() {
    run_test(Language::Cpp, |f| {
        // https://github.com/microsoft/vscode/issues/178334

        let model = f.create_text_model(&["int WINAPI WinMain(bool instance,", "    int nshowcmd) {}"].join("\n"));

        f.with_test_code_editor(&model, "full", |editor, view_model, _instantiation_service| {
            set_selection(editor, 2, 20, 2, 20);
            type_keyboard(view_model, "\n");
            assert_eq!(
                value_of(&model),
                ["int WINAPI WinMain(bool instance,", "    int nshowcmd) {", "    ", "}"].join("\n")
            );
        });
    });
}

#[test]
#[ignore = "test.skip no original"]
fn auto_indent_on_type_cpp_issue_118929_incorrect_indent_when_follows_curly_brace() {
    run_test(Language::Cpp, |f| {
        // https://github.com/microsoft/vscode/issues/118929

        let model = f.create_text_model(&["if (true) { // jaja", "}"].join("\n"));

        f.with_test_code_editor(&model, "full", |editor, view_model, _instantiation_service| {
            set_selection(editor, 1, 20, 1, 20);
            type_keyboard(view_model, "\n");
            assert_eq!(value_of(&model), ["if (true) { // jaja", "    ", "}"].join("\n"));
        });
    });
}

#[test]
#[ignore = "test.skip no original"]
fn auto_indent_on_type_cpp_issue_111265_auto_indentation_set_to_none_still_changes_the_indentation() {
    run_test(Language::Cpp, |f| {
        // https://github.com/microsoft/vscode/issues/111265

        let model = f.create_text_model(&["int func() {", "\t\t"].join("\n"));

        f.with_test_code_editor(&model, "none", |editor, view_model, _instantiation_service| {
            set_selection(editor, 2, 3, 2, 3);
            type_keyboard(view_model, "}");
            assert_eq!(value_of(&model), ["int func() {", "\t\t}"].join("\n"));
        });
    });
}

// suite('Auto Indent On Type - HTML')

#[test]
fn auto_indent_on_type_html_temp_issue_because_there_should_be_at_least_one_passing_test_in_a_suite() {
    run_test(Language::Html, |_f| {});
}

#[test]
#[ignore = "test.skip no original"]
fn auto_indent_on_type_html_issue_61510_incorrect_indentation_after_in_html_file() {
    run_test(Language::Html, |f| {
        // https://github.com/microsoft/vscode/issues/178334

        let model = f.create_text_model(&["<pre>", "  foo //I press <Enter> at the end of this line", "</pre>"].join("\n"));

        f.with_test_code_editor(&model, "full", |editor, view_model, _instantiation_service| {
            set_selection(editor, 2, 48, 2, 48);
            type_keyboard(view_model, "\n");
            assert_eq!(
                value_of(&model),
                ["<pre>", "  foo //I press <Enter> at the end of this line", "  ", "</pre>"].join("\n")
            );
        });
    });
}

// suite('Auto Indent On Type - Visual Basic')

#[test]
fn auto_indent_on_type_visual_basic_temp_issue_because_there_should_be_at_least_one_passing_test_in_a_suite() {
    run_test(Language::Vb, |_f| {});
}

#[test]
fn auto_indent_on_type_visual_basic_issue_118932_no_indentation_in_visual_basic_files() {
    run_test(Language::Vb, |f| {
        // https://github.com/microsoft/vscode/issues/118932

        let model = f.create_text_model(&["If True Then", "    Some code", "    End I"].join("\n"));

        f.with_test_code_editor(&model, "full", |editor, view_model, _instantiation_service| {
            set_selection(editor, 3, 10, 3, 10);
            type_keyboard(view_model, "f");
            assert_eq!(value_of(&model), ["If True Then", "    Some code", "End If"].join("\n"));
        });
    });
}

#[test]
fn auto_indent_on_type_visual_basic_issue_118932_indent_after_module_declaration() {
    run_test(Language::Vb, |f| {
        // https://github.com/microsoft/vscode/issues/118932

        let model = f.create_text_model("");

        f.with_test_code_editor(&model, "full", |_editor, view_model, _instantiation_service| {
            type_text(view_model, "Module Test");
            type_keyboard(view_model, "\n");
            assert_eq!(value_of(&model), ["Module Test", "    "].join("\n"));
        });
    });
}

#[test]
fn auto_indent_on_type_visual_basic_issue_118932_indent_after_sub_declaration() {
    run_test(Language::Vb, |f| {
        // https://github.com/microsoft/vscode/issues/118932

        let model = f.create_text_model(&["Module Test", "    "].join("\n"));

        f.with_test_code_editor(&model, "full", |editor, view_model, _instantiation_service| {
            set_selection(editor, 2, 5, 2, 5);
            type_text(view_model, "Sub Main()");
            type_keyboard(view_model, "\n");
            assert_eq!(value_of(&model), ["Module Test", "    Sub Main()", "        "].join("\n"));
        });
    });
}

#[test]
fn auto_indent_on_type_visual_basic_issue_118932_dedent_on_end_sub() {
    run_test(Language::Vb, |f| {
        // https://github.com/microsoft/vscode/issues/118932

        let model = f.create_text_model(&["Module Test", "    Sub Main()", "        Console.WriteLine(\"Hello\")", "        End Su"].join("\n"));

        f.with_test_code_editor(&model, "full", |editor, view_model, _instantiation_service| {
            set_selection(editor, 4, 15, 4, 15);
            type_keyboard(view_model, "b");
            assert_eq!(
                value_of(&model),
                ["Module Test", "    Sub Main()", "        Console.WriteLine(\"Hello\")", "    End Sub"].join("\n")
            );
        });
    });
}

#[test]
fn auto_indent_on_type_visual_basic_issue_118932_dedent_on_end_module() {
    run_test(Language::Vb, |f| {
        // https://github.com/microsoft/vscode/issues/118932
        // When End Module is typed right after Module (no nested blocks), it dedents correctly

        let model = f.create_text_model(&["Module Test", "    Private x As Integer", "    End Modul"].join("\n"));

        f.with_test_code_editor(&model, "full", |editor, view_model, _instantiation_service| {
            set_selection(editor, 3, 14, 3, 14);
            type_keyboard(view_model, "e");
            assert_eq!(value_of(&model), ["Module Test", "    Private x As Integer", "End Module"].join("\n"));
        });
    });
}

#[test]
fn auto_indent_on_type_visual_basic_issue_118932_indent_after_function_declaration() {
    run_test(Language::Vb, |f| {
        // https://github.com/microsoft/vscode/issues/118932

        let model = f.create_text_model(&["Module Test", "    "].join("\n"));

        f.with_test_code_editor(&model, "full", |editor, view_model, _instantiation_service| {
            set_selection(editor, 2, 5, 2, 5);
            type_text(view_model, "Function Add(a As Integer, b As Integer) As Integer");
            type_keyboard(view_model, "\n");
            assert_eq!(
                value_of(&model),
                ["Module Test", "    Function Add(a As Integer, b As Integer) As Integer", "        "].join("\n")
            );
        });
    });
}

#[test]
fn auto_indent_on_type_visual_basic_issue_118932_dedent_on_end_function() {
    run_test(Language::Vb, |f| {
        // https://github.com/microsoft/vscode/issues/118932

        let model = f.create_text_model(&["Module Test", "    Function Add(a, b)", "        Return a + b", "        End Functio"].join("\n"));

        f.with_test_code_editor(&model, "full", |editor, view_model, _instantiation_service| {
            set_selection(editor, 4, 20, 4, 20);
            type_keyboard(view_model, "n");
            assert_eq!(
                value_of(&model),
                ["Module Test", "    Function Add(a, b)", "        Return a + b", "    End Function"].join("\n")
            );
        });
    });
}

#[test]
fn auto_indent_on_type_visual_basic_issue_118932_indent_after_if_then() {
    run_test(Language::Vb, |f| {
        // https://github.com/microsoft/vscode/issues/118932

        let model = f.create_text_model(&["Sub Test()", "    "].join("\n"));

        f.with_test_code_editor(&model, "full", |editor, view_model, _instantiation_service| {
            set_selection(editor, 2, 5, 2, 5);
            type_text(view_model, "If x > 0 Then");
            type_keyboard(view_model, "\n");
            assert_eq!(value_of(&model), ["Sub Test()", "    If x > 0 Then", "        "].join("\n"));
        });
    });
}

#[test]
fn auto_indent_on_type_visual_basic_issue_118932_indent_after_elseif_then() {
    run_test(Language::Vb, |f| {
        // https://github.com/microsoft/vscode/issues/118932

        let model = f.create_text_model(&["Sub Test()", "    If x > 0 Then", "        DoSomething()", "    ElseIf x < 0 Then"].join("\n"));

        f.with_test_code_editor(&model, "full", |editor, view_model, _instantiation_service| {
            set_selection(editor, 4, 22, 4, 22);
            type_keyboard(view_model, "\n");
            assert_eq!(
                value_of(&model),
                ["Sub Test()", "    If x > 0 Then", "        DoSomething()", "    ElseIf x < 0 Then", "        "].join("\n")
            );
        });
    });
}

#[test]
fn auto_indent_on_type_visual_basic_issue_118932_dedent_and_indent_on_else() {
    run_test(Language::Vb, |f| {
        // https://github.com/microsoft/vscode/issues/118932

        let model = f.create_text_model(&["Sub Test()", "    If x > 0 Then", "        DoSomething()", "        Els"].join("\n"));

        f.with_test_code_editor(&model, "full", |editor, view_model, _instantiation_service| {
            set_selection(editor, 4, 12, 4, 12);
            type_keyboard(view_model, "e");
            assert_eq!(
                value_of(&model),
                ["Sub Test()", "    If x > 0 Then", "        DoSomething()", "    Else"].join("\n")
            );
        });
    });
}

#[test]
fn auto_indent_on_type_visual_basic_issue_118932_indent_after_while() {
    run_test(Language::Vb, |f| {
        // https://github.com/microsoft/vscode/issues/118932

        let model = f.create_text_model(&["Sub Test()", "    "].join("\n"));

        f.with_test_code_editor(&model, "full", |editor, view_model, _instantiation_service| {
            set_selection(editor, 2, 5, 2, 5);
            type_text(view_model, "While x > 0");
            type_keyboard(view_model, "\n");
            assert_eq!(value_of(&model), ["Sub Test()", "    While x > 0", "        "].join("\n"));
        });
    });
}

#[test]
fn auto_indent_on_type_visual_basic_issue_118932_dedent_on_end_while() {
    run_test(Language::Vb, |f| {
        // https://github.com/microsoft/vscode/issues/118932

        let model = f.create_text_model(&["Sub Test()", "    While x > 0", "        x = x - 1", "        End Whil"].join("\n"));

        f.with_test_code_editor(&model, "full", |editor, view_model, _instantiation_service| {
            set_selection(editor, 4, 17, 4, 17);
            type_keyboard(view_model, "e");
            assert_eq!(
                value_of(&model),
                ["Sub Test()", "    While x > 0", "        x = x - 1", "    End While"].join("\n")
            );
        });
    });
}

#[test]
fn auto_indent_on_type_visual_basic_issue_118932_indent_after_for() {
    run_test(Language::Vb, |f| {
        // https://github.com/microsoft/vscode/issues/118932

        let model = f.create_text_model(&["Sub Test()", "    "].join("\n"));

        f.with_test_code_editor(&model, "full", |editor, view_model, _instantiation_service| {
            set_selection(editor, 2, 5, 2, 5);
            type_text(view_model, "For i = 1 To 10");
            type_keyboard(view_model, "\n");
            assert_eq!(value_of(&model), ["Sub Test()", "    For i = 1 To 10", "        "].join("\n"));
        });
    });
}

#[test]
fn auto_indent_on_type_visual_basic_issue_118932_dedent_on_next() {
    run_test(Language::Vb, |f| {
        // https://github.com/microsoft/vscode/issues/118932

        let model = f.create_text_model(&["Sub Test()", "    For i = 1 To 10", "        DoSomething(i)", "        Nex"].join("\n"));

        f.with_test_code_editor(&model, "full", |editor, view_model, _instantiation_service| {
            set_selection(editor, 4, 12, 4, 12);
            type_keyboard(view_model, "t");
            assert_eq!(
                value_of(&model),
                ["Sub Test()", "    For i = 1 To 10", "        DoSomething(i)", "    Next"].join("\n")
            );
        });
    });
}

#[test]
fn auto_indent_on_type_visual_basic_issue_118932_indent_after_do() {
    run_test(Language::Vb, |f| {
        // https://github.com/microsoft/vscode/issues/118932

        let model = f.create_text_model(&["Sub Test()", "    "].join("\n"));

        f.with_test_code_editor(&model, "full", |editor, view_model, _instantiation_service| {
            set_selection(editor, 2, 5, 2, 5);
            type_text(view_model, "Do");
            type_keyboard(view_model, "\n");
            assert_eq!(value_of(&model), ["Sub Test()", "    Do", "        "].join("\n"));
        });
    });
}

#[test]
fn auto_indent_on_type_visual_basic_issue_118932_dedent_on_loop() {
    run_test(Language::Vb, |f| {
        // https://github.com/microsoft/vscode/issues/118932

        let model = f.create_text_model(&["Sub Test()", "    Do", "        x = x + 1", "        Loo"].join("\n"));

        f.with_test_code_editor(&model, "full", |editor, view_model, _instantiation_service| {
            set_selection(editor, 4, 12, 4, 12);
            type_keyboard(view_model, "p");
            assert_eq!(value_of(&model), ["Sub Test()", "    Do", "        x = x + 1", "    Loop"].join("\n"));
        });
    });
}

#[test]
fn auto_indent_on_type_visual_basic_issue_118932_indent_after_select_case() {
    run_test(Language::Vb, |f| {
        // https://github.com/microsoft/vscode/issues/118932

        let model = f.create_text_model(&["Sub Test()", "    "].join("\n"));

        f.with_test_code_editor(&model, "full", |editor, view_model, _instantiation_service| {
            set_selection(editor, 2, 5, 2, 5);
            type_text(view_model, "Select Case x");
            type_keyboard(view_model, "\n");
            assert_eq!(value_of(&model), ["Sub Test()", "    Select Case x", "        "].join("\n"));
        });
    });
}

#[test]
fn auto_indent_on_type_visual_basic_issue_118932_dedent_on_end_select() {
    run_test(Language::Vb, |f| {
        // https://github.com/microsoft/vscode/issues/118932
        // When End Select is typed, it dedents to match Select Case level

        let model = f.create_text_model(&["Sub Test()", "    Select Case x", "        End Selec"].join("\n"));

        f.with_test_code_editor(&model, "full", |editor, view_model, _instantiation_service| {
            set_selection(editor, 3, 18, 3, 18);
            type_keyboard(view_model, "t");
            assert_eq!(value_of(&model), ["Sub Test()", "    Select Case x", "    End Select"].join("\n"));
        });
    });
}

#[test]
fn auto_indent_on_type_visual_basic_issue_118932_indent_after_try() {
    run_test(Language::Vb, |f| {
        // https://github.com/microsoft/vscode/issues/118932

        let model = f.create_text_model(&["Sub Test()", "    "].join("\n"));

        f.with_test_code_editor(&model, "full", |editor, view_model, _instantiation_service| {
            set_selection(editor, 2, 5, 2, 5);
            type_text(view_model, "Try");
            type_keyboard(view_model, "\n");
            assert_eq!(value_of(&model), ["Sub Test()", "    Try", "        "].join("\n"));
        });
    });
}

#[test]
fn auto_indent_on_type_visual_basic_issue_118932_dedent_and_indent_on_catch() {
    run_test(Language::Vb, |f| {
        // https://github.com/microsoft/vscode/issues/118932

        let model = f.create_text_model(&["Sub Test()", "    Try", "        DoSomething()", "        Catc"].join("\n"));

        f.with_test_code_editor(&model, "full", |editor, view_model, _instantiation_service| {
            set_selection(editor, 4, 13, 4, 13);
            type_keyboard(view_model, "h");
            assert_eq!(
                value_of(&model),
                ["Sub Test()", "    Try", "        DoSomething()", "    Catch"].join("\n")
            );
        });
    });
}

#[test]
fn auto_indent_on_type_visual_basic_issue_118932_dedent_and_indent_on_finally() {
    run_test(Language::Vb, |f| {
        // https://github.com/microsoft/vscode/issues/118932

        let model = f.create_text_model(
            &["Sub Test()", "    Try", "        DoSomething()", "    Catch", "        HandleError()", "        Finall"].join("\n"),
        );

        f.with_test_code_editor(&model, "full", |editor, view_model, _instantiation_service| {
            set_selection(editor, 6, 15, 6, 15);
            type_keyboard(view_model, "y");
            assert_eq!(
                value_of(&model),
                ["Sub Test()", "    Try", "        DoSomething()", "    Catch", "        HandleError()", "    Finally"].join("\n")
            );
        });
    });
}

#[test]
fn auto_indent_on_type_visual_basic_issue_118932_dedent_on_end_try() {
    run_test(Language::Vb, |f| {
        // https://github.com/microsoft/vscode/issues/118932

        let model = f.create_text_model(
            &[
                "Sub Test()",
                "    Try",
                "        DoSomething()",
                "    Catch",
                "        HandleError()",
                "    Finally",
                "        Cleanup()",
                "        End Tr",
            ]
            .join("\n"),
        );

        f.with_test_code_editor(&model, "full", |editor, view_model, _instantiation_service| {
            set_selection(editor, 8, 15, 8, 15);
            type_keyboard(view_model, "y");
            assert_eq!(
                value_of(&model),
                [
                    "Sub Test()",
                    "    Try",
                    "        DoSomething()",
                    "    Catch",
                    "        HandleError()",
                    "    Finally",
                    "        Cleanup()",
                    "    End Try",
                ]
                .join("\n")
            );
        });
    });
}

#[test]
fn auto_indent_on_type_visual_basic_issue_118932_indent_after_class() {
    run_test(Language::Vb, |f| {
        // https://github.com/microsoft/vscode/issues/118932

        let model = f.create_text_model("");

        f.with_test_code_editor(&model, "full", |_editor, view_model, _instantiation_service| {
            type_text(view_model, "Class MyClass");
            type_keyboard(view_model, "\n");
            assert_eq!(value_of(&model), ["Class MyClass", "    "].join("\n"));
        });
    });
}

#[test]
fn auto_indent_on_type_visual_basic_issue_118932_dedent_on_end_class() {
    run_test(Language::Vb, |f| {
        // https://github.com/microsoft/vscode/issues/118932

        let model = f.create_text_model(&["Class MyClass", "    Private x As Integer", "    End Clas"].join("\n"));

        f.with_test_code_editor(&model, "full", |editor, view_model, _instantiation_service| {
            set_selection(editor, 3, 14, 3, 14);
            type_keyboard(view_model, "s");
            assert_eq!(value_of(&model), ["Class MyClass", "    Private x As Integer", "End Class"].join("\n"));
        });
    });
}

#[test]
fn auto_indent_on_type_visual_basic_issue_118932_full_program_indentation_flow() {
    run_test(Language::Vb, |f| {
        // https://github.com/microsoft/vscode/issues/118932
        // Verify the complete flow as described in the verification comment
        // Note: Auto-indent only triggers on typing the last character that completes a keyword
        // and only decreases by one indentation level per keyword completion

        let model = f.create_text_model("");

        f.with_test_code_editor(&model, "full", |_editor, view_model, _instantiation_service| {
            // Type Module Test and press Enter
            type_text(view_model, "Module Test");
            type_keyboard(view_model, "\n");
            assert_eq!(value_of(&model), ["Module Test", "    "].join("\n"), "After Module Test");

            // Type Sub Main() and press Enter
            type_text(view_model, "Sub Main()");
            type_keyboard(view_model, "\n");
            assert_eq!(value_of(&model), ["Module Test", "    Sub Main()", "        "].join("\n"), "After Sub Main()");

            // Type Console.WriteLine and press Enter
            type_text(view_model, "Console.WriteLine(\"Hello, World!\")");
            type_keyboard(view_model, "\n");
            assert_eq!(
                value_of(&model),
                ["Module Test", "    Sub Main()", "        Console.WriteLine(\"Hello, World!\")", "        "].join("\n"),
                "After Console.WriteLine"
            );

            // Type End Su then 'b' to complete End Sub (auto-indent triggers on last char)
            type_text(view_model, "End Su");
            type_keyboard(view_model, "b");
            assert_eq!(
                value_of(&model),
                ["Module Test", "    Sub Main()", "        Console.WriteLine(\"Hello, World!\")", "    End Sub"].join("\n"),
                "After End Sub"
            );

            // Press Enter - should maintain same indent level after End Sub
            type_keyboard(view_model, "\n");
            assert_eq!(
                value_of(&model),
                ["Module Test", "    Sub Main()", "        Console.WriteLine(\"Hello, World!\")", "    End Sub", "    "].join("\n"),
                "After Enter after End Sub"
            );
        });
    });
}

// suite('Auto Indent On Type - Latex')

#[test]
fn auto_indent_on_type_latex_temp_issue_because_there_should_be_at_least_one_passing_test_in_a_suite() {
    run_test(Language::Latex, |_f| {});
}

#[test]
#[ignore = "test.skip no original"]
fn auto_indent_on_type_latex_issue_178075_no_auto_closing_pair_when_indentation_done() {
    run_test(Language::Latex, |f| {
        // https://github.com/microsoft/vscode/issues/178075

        let model = f.create_text_model(&["\\begin{theorem}", "    \\end"].join("\n"));

        f.with_test_code_editor(&model, "full", |editor, view_model, _instantiation_service| {
            set_selection(editor, 2, 9, 2, 9);
            type_keyboard(view_model, "{");
            assert_eq!(value_of(&model), ["\\begin{theorem}", "\\end{}"].join("\n"));
        });
    });
}

// suite('Auto Indent On Type - Lua')

#[test]
fn auto_indent_on_type_lua_temp_issue_because_there_should_be_at_least_one_passing_test_in_a_suite() {
    run_test(Language::Lua, |_f| {});
}

#[test]
#[ignore = "test.skip no original"]
fn auto_indent_on_type_lua_issue_178075_no_auto_closing_pair_when_indentation_done() {
    run_test(Language::Lua, |f| {
        // https://github.com/microsoft/vscode/issues/178075

        let model = f.create_text_model(&["print(\"asdf function asdf\")"].join("\n"));

        f.with_test_code_editor(&model, "full", |editor, view_model, _instantiation_service| {
            set_selection(editor, 1, 28, 1, 28);
            type_keyboard(view_model, "\n");
            assert_eq!(value_of(&model), ["print(\"asdf function asdf\")", ""].join("\n"));
        });
    });
}
