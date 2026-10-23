//! Porte de `src/vs/editor/test/common/modes/supports/onEnter.test.ts` (suíte `OnEnter`), caso a caso e na mesma ordem.
//!
//! Notas de tradução:
//! - `assert.strictEqual(actual, null)` vira `actual.is_none()`; `actual!.indentAction` e os demais
//!   campos vêm do `Option<EnterAction>` desempacotado.
//! - `ensureNoDisposablesAreLeakedInTestSuite` vem do `common` do `je-base`, incluído por `#[path]`.

#[path = "../../je-base/tests/common/mod.rs"]
mod common;
mod support;

use common::ensure_no_disposables_are_leaked_in_test_suite;
use je_editor::config::editor_options::EditorAutoIndentStrategy;
use je_editor::language_configuration::{EnterAction, IndentAction, OnEnterRule};
use je_editor::on_enter::{OnEnterSupport, OnEnterSupportOptions};
use support::on_enter_rules::javascript_on_enter_rules;
use support::{re, u};

fn pairs(list: &[(&str, &str)]) -> Vec<(String, String)> {
    list.iter().map(|(open, close)| ((*open).to_owned(), (*close).to_owned())).collect()
}

/// O `testIndentAction` das regras de `RegExp`, com a linha anterior.
fn test_indent_action_with_rules(
    support: &OnEnterSupport,
    previous_line_text: &str,
    before_text: &str,
    after_text: &str,
    expected_indent_action: Option<IndentAction>,
    expected_append_text: Option<&str>,
    remove_text: i32,
) {
    let actual = support.on_enter(
        EditorAutoIndentStrategy::Advanced,
        &u(previous_line_text),
        &u(before_text),
        &u(after_text),
    );
    match expected_indent_action {
        None => assert!(actual.is_none(), "isNull:{before_text}"),
        Some(expected_indent_action) => {
            assert!(actual.is_some(), "isNotNull:{before_text}");
            let actual = actual.unwrap();
            assert_eq!(actual.indent_action, expected_indent_action, "indentAction:{before_text}");
            if let Some(expected_append_text) = expected_append_text {
                assert_eq!(actual.append_text.as_deref(), Some(expected_append_text), "appendText:{before_text}");
            }
            if remove_text != 0 {
                assert_eq!(actual.remove_text, Some(remove_text), "removeText:{before_text}");
            }
        }
    }
}

#[test]
fn uses_brackets() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    let brackets = pairs(&[("(", ")"), ("begin", "end")]);
    let support = OnEnterSupport::new(OnEnterSupportOptions { brackets: Some(brackets), on_enter_rules: None });
    let test_indent_action = |before_text: &str, after_text: &str, expected: IndentAction| {
        let actual = support.on_enter(EditorAutoIndentStrategy::Advanced, &[], &u(before_text), &u(after_text));
        if expected == IndentAction::None {
            assert_eq!(actual, None);
        } else {
            assert_eq!(actual.unwrap().indent_action, expected);
        }
    };

    test_indent_action("a", "", IndentAction::None);
    test_indent_action("", "b", IndentAction::None);
    test_indent_action("(", "b", IndentAction::Indent);
    test_indent_action("a", ")", IndentAction::None);
    test_indent_action("begin", "ending", IndentAction::Indent);
    test_indent_action("abegin", "end", IndentAction::None);
    test_indent_action("begin", ")", IndentAction::Indent);
    test_indent_action("begin", "end", IndentAction::IndentOutdent);
    test_indent_action("begin ", " end", IndentAction::IndentOutdent);
    test_indent_action(" begin", "end//as", IndentAction::IndentOutdent);
    test_indent_action("(", ")", IndentAction::IndentOutdent);
    test_indent_action("( ", ")", IndentAction::IndentOutdent);
    test_indent_action("a(", ")b", IndentAction::IndentOutdent);

    test_indent_action("(", "", IndentAction::Indent);
    test_indent_action("(", "foo", IndentAction::Indent);
    test_indent_action("begin", "foo", IndentAction::Indent);
    test_indent_action("begin", "", IndentAction::Indent);
}

#[test]
fn issue_121125_on_enter_rules_with_global_modifier() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    let support = OnEnterSupport::new(OnEnterSupportOptions {
        brackets: None,
        on_enter_rules: Some(vec![OnEnterRule {
            action: EnterAction {
                indent_action: IndentAction::Outdent,
                append_text: Some("/// ".to_owned()),
                remove_text: None,
            },
            before_text: re(r"^\s*\/{3}.*$", "gm"),
            after_text: None,
            previous_line_text: None,
        }]),
    });

    let test_indent_action = |previous_line_text: &str,
                              before_text: &str,
                              after_text: &str,
                              expected_indent_action: Option<IndentAction>,
                              expected_append_text: Option<&str>| {
        test_indent_action_with_rules(
            &support,
            previous_line_text,
            before_text,
            after_text,
            expected_indent_action,
            expected_append_text,
            0,
        );
    };

    test_indent_action("/// line", "/// line", "", Some(IndentAction::Outdent), Some("/// "));
    test_indent_action("/// line", "/// line", "", Some(IndentAction::Outdent), Some("/// "));
}

#[test]
fn uses_reg_exp_rules() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    let support = OnEnterSupport::new(OnEnterSupportOptions {
        brackets: None,
        on_enter_rules: Some(javascript_on_enter_rules()),
    });
    let test_indent_action = |previous_line_text: &str,
                              before_text: &str,
                              after_text: &str,
                              expected_indent_action: Option<IndentAction>,
                              expected_append_text: Option<&str>,
                              remove_text: i32| {
        test_indent_action_with_rules(
            &support,
            previous_line_text,
            before_text,
            after_text,
            expected_indent_action,
            expected_append_text,
            remove_text,
        );
    };
    let none = Some(IndentAction::None);

    test_indent_action("", "\t/**", " */", Some(IndentAction::IndentOutdent), Some(" * "), 0);
    test_indent_action("", "\t/**", "", none, Some(" * "), 0);
    test_indent_action("", "\t/** * / * / * /", "", none, Some(" * "), 0);
    test_indent_action("", "\t/** /*", "", none, Some(" * "), 0);
    test_indent_action("", "/**", "", none, Some(" * "), 0);
    test_indent_action("", "\t/**/", "", None, None, 0);
    test_indent_action("", "\t/***/", "", None, None, 0);
    test_indent_action("", "\t/*******/", "", None, None, 0);
    test_indent_action("", "\t/** * * * * */", "", None, None, 0);
    test_indent_action("", "\t/** */", "", None, None, 0);
    test_indent_action("", "\t/** asdfg */", "", None, None, 0);
    test_indent_action("", "\t/* asdfg */", "", None, None, 0);
    test_indent_action("", "\t/* asdfg */", "", None, None, 0);
    test_indent_action("", "\t/** asdfg */", "", None, None, 0);
    test_indent_action("", "*/", "", None, None, 0);
    test_indent_action("", "\t/*", "", None, None, 0);
    test_indent_action("", "\t*", "", None, None, 0);

    test_indent_action("\t/**", "\t *", "", none, Some("* "), 0);
    test_indent_action("\t * something", "\t *", "", none, Some("* "), 0);
    test_indent_action("\t *", "\t *", "", none, Some("* "), 0);

    test_indent_action("", "\t */", "", none, None, 1);
    test_indent_action("", "\t * */", "", none, None, 1);
    test_indent_action("", "\t * * / * / * / */", "", None, None, 0);

    test_indent_action("\t/**", "\t * ", "", none, Some("* "), 0);
    test_indent_action("\t * something", "\t * ", "", none, Some("* "), 0);
    test_indent_action("\t *", "\t * ", "", none, Some("* "), 0);

    test_indent_action("/**", " * ", "", none, Some("* "), 0);
    test_indent_action(" * something", " * ", "", none, Some("* "), 0);
    test_indent_action(" *", " * asdfsfagadfg", "", none, Some("* "), 0);

    test_indent_action("/**", " * asdfsfagadfg * * * ", "", none, Some("* "), 0);
    test_indent_action(" * something", " * asdfsfagadfg * * * ", "", none, Some("* "), 0);
    test_indent_action(" *", " * asdfsfagadfg * * * ", "", none, Some("* "), 0);

    test_indent_action("/**", " * /*", "", none, Some("* "), 0);
    test_indent_action(" * something", " * /*", "", none, Some("* "), 0);
    test_indent_action(" *", " * /*", "", none, Some("* "), 0);

    test_indent_action("/**", " * asdfsfagadfg * / * / * /", "", none, Some("* "), 0);
    test_indent_action(" * something", " * asdfsfagadfg * / * / * /", "", none, Some("* "), 0);
    test_indent_action(" *", " * asdfsfagadfg * / * / * /", "", none, Some("* "), 0);

    test_indent_action("/**", " * asdfsfagadfg * / * / * /*", "", none, Some("* "), 0);
    test_indent_action(" * something", " * asdfsfagadfg * / * / * /*", "", none, Some("* "), 0);
    test_indent_action(" *", " * asdfsfagadfg * / * / * /*", "", none, Some("* "), 0);

    test_indent_action("", " */", "", none, None, 1);
    test_indent_action(" */", " * test() {", "", Some(IndentAction::Indent), None, 0);
    test_indent_action("", "\t */", "", none, None, 1);
    test_indent_action("", "\t\t */", "", none, None, 1);
    test_indent_action("", "   */", "", none, None, 1);
    test_indent_action("", "     */", "", none, None, 1);
    test_indent_action("", "\t     */", "", none, None, 1);
    test_indent_action(
        "",
        " *--------------------------------------------------------------------------------------------*/",
        "",
        none,
        None,
        1,
    );

    // issue #43469
    test_indent_action("class A {", "    * test() {", "", Some(IndentAction::Indent), None, 0);
    test_indent_action("", "    * test() {", "", Some(IndentAction::Indent), None, 0);
    test_indent_action("    ", "    * test() {", "", Some(IndentAction::Indent), None, 0);
    test_indent_action("class A {", "  * test() {", "", Some(IndentAction::Indent), None, 0);
    test_indent_action("", "  * test() {", "", Some(IndentAction::Indent), None, 0);
    test_indent_action("  ", "  * test() {", "", Some(IndentAction::Indent), None, 0);
}

#[test]
fn issue_141816() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    let support = OnEnterSupport::new(OnEnterSupportOptions {
        brackets: None,
        on_enter_rules: Some(javascript_on_enter_rules()),
    });
    let test_indent_action = |before_text: &str, after_text: &str, expected: IndentAction| {
        let actual = support.on_enter(EditorAutoIndentStrategy::Advanced, &[], &u(before_text), &u(after_text));
        if expected == IndentAction::None {
            assert_eq!(actual, None);
        } else {
            assert_eq!(actual.unwrap().indent_action, expected);
        }
    };

    test_indent_action("const r = /{/;", "", IndentAction::None);
    test_indent_action("const r = /{[0-9]/;", "", IndentAction::None);
    test_indent_action("const r = /[a-zA-Z]{/;", "", IndentAction::None);
}
