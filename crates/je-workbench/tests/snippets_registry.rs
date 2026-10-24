//! Porte de `src/vs/workbench/contrib/snippets/test/browser/snippetsRegistry.test.ts` (suíte `getNonWhitespacePrefix`),
//! caso a caso e na mesma ordem.
//!
//! Notas de tradução:
//! - `ensureNoDisposablesAreLeakedInTestSuite()` vira o `LeakGuard` criado no começo de cada caso.
//! - O modelo `{ getLineContent: (lineNumber) => line }` é a struct [`LineModel`], que implementa o `ISimpleModel` do
//!   porte; o resultado é um `JsString` (unidades UTF-16), comparado com o texto esperado.
//! - Os escapes `\f`, `\v` e `\uXXXX` do original são `\u{000c}`, `\u{000b}` e `\u{XXXX}`.

#[path = "../../je-base/tests/common/mod.rs"]
mod common;

use common::ensure_no_disposables_are_leaked_in_test_suite;
use je_base::js_string::JsString;
use je_editor::position::Position;
use je_workbench::contrib::snippets::snippets_service::{ISimpleModel, get_non_whitespace_prefix};

struct LineModel {
    line: String,
}

impl ISimpleModel for LineModel {
    fn get_line_content(&self, _line_number: i32) -> JsString {
        JsString::from(self.line.as_str())
    }
}

fn assert_get_non_whitespace_prefix(line: &str, column: i32, expected: &str) {
    let model = LineModel { line: line.to_owned() };
    let actual = get_non_whitespace_prefix(&model, Position::new(1, column));
    assert_eq!(actual, expected);
}

#[test]
fn empty_line() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    assert_get_non_whitespace_prefix("", 1, "");
}

#[test]
fn single_word_line() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    assert_get_non_whitespace_prefix("something", 1, "");
    assert_get_non_whitespace_prefix("something", 2, "s");
    assert_get_non_whitespace_prefix("something", 3, "so");
    assert_get_non_whitespace_prefix("something", 4, "som");
    assert_get_non_whitespace_prefix("something", 5, "some");
    assert_get_non_whitespace_prefix("something", 6, "somet");
    assert_get_non_whitespace_prefix("something", 7, "someth");
    assert_get_non_whitespace_prefix("something", 8, "somethi");
    assert_get_non_whitespace_prefix("something", 9, "somethin");
    assert_get_non_whitespace_prefix("something", 10, "something");
}

#[test]
fn two_word_line() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    assert_get_non_whitespace_prefix("something interesting", 1, "");
    assert_get_non_whitespace_prefix("something interesting", 2, "s");
    assert_get_non_whitespace_prefix("something interesting", 3, "so");
    assert_get_non_whitespace_prefix("something interesting", 4, "som");
    assert_get_non_whitespace_prefix("something interesting", 5, "some");
    assert_get_non_whitespace_prefix("something interesting", 6, "somet");
    assert_get_non_whitespace_prefix("something interesting", 7, "someth");
    assert_get_non_whitespace_prefix("something interesting", 8, "somethi");
    assert_get_non_whitespace_prefix("something interesting", 9, "somethin");
    assert_get_non_whitespace_prefix("something interesting", 10, "something");
    assert_get_non_whitespace_prefix("something interesting", 11, "");
    assert_get_non_whitespace_prefix("something interesting", 12, "i");
    assert_get_non_whitespace_prefix("something interesting", 13, "in");
    assert_get_non_whitespace_prefix("something interesting", 14, "int");
    assert_get_non_whitespace_prefix("something interesting", 15, "inte");
    assert_get_non_whitespace_prefix("something interesting", 16, "inter");
    assert_get_non_whitespace_prefix("something interesting", 17, "intere");
    assert_get_non_whitespace_prefix("something interesting", 18, "interes");
    assert_get_non_whitespace_prefix("something interesting", 19, "interest");
    assert_get_non_whitespace_prefix("something interesting", 20, "interesti");
    assert_get_non_whitespace_prefix("something interesting", 21, "interestin");
    assert_get_non_whitespace_prefix("something interesting", 22, "interesting");
}

#[test]
fn many_separators() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    // https://developer.mozilla.org/en-US/docs/Web/JavaScript/Guide/Regular_Expressions?redirectlocale=en-US&redirectslug=JavaScript%2FGuide%2FRegular_Expressions#special-white-space
    // \s matches a single white space character, including space, tab, form feed, line feed.
    // Equivalent to [ \f\n\r\t\v  ᠎ -     　﻿].

    assert_get_non_whitespace_prefix("something interesting", 22, "interesting");
    assert_get_non_whitespace_prefix("something\tinteresting", 22, "interesting");
    assert_get_non_whitespace_prefix("something\u{000c}interesting", 22, "interesting");
    assert_get_non_whitespace_prefix("something\u{000b}interesting", 22, "interesting");
    assert_get_non_whitespace_prefix("something\u{00a0}interesting", 22, "interesting");
    assert_get_non_whitespace_prefix("something\u{2000}interesting", 22, "interesting");
    assert_get_non_whitespace_prefix("something\u{2028}interesting", 22, "interesting");
    assert_get_non_whitespace_prefix("something\u{3000}interesting", 22, "interesting");
    assert_get_non_whitespace_prefix("something\u{feff}interesting", 22, "interesting");
}
