//! Porte de `src/vs/editor/contrib/find/test/browser/replacePattern.test.ts` (suíte `Replace Pattern test`), caso a caso e na mesma ordem.

#[path = "../../je-base/tests/common/mod.rs"]
mod common;

use common::ensure_no_disposables_are_leaked_in_test_suite;
use je_base::JsString;
use je_base::search::build_replace_string_with_case_preserved;
use je_editor::find::replace_pattern::{ReplacePattern, ReplacePiece, parse_replace_string as parse};
use je_editor::js_reg_exp::JsRegExp;

fn js(text: &str) -> JsString {
    JsString::from(text)
}

fn static_value(value: &str) -> ReplacePiece {
    ReplacePiece::static_value(js(value))
}

/// `search.exec(target)` convertido para a lista de capturas (`string[] | null`).
fn exec(search: &str, target: &str) -> Option<Vec<Option<JsString>>> {
    let regex = JsRegExp::from_str(search, "").expect("regex válida");
    let text = js(target);
    regex.exec(text.units()).map(|found| {
        found
            .captures
            .iter()
            .map(|capture| capture.as_ref().map(|span| JsString::from(&text.units()[span.clone()])))
            .collect()
    })
}

fn test_parse(input: &str, expected_pieces: Vec<ReplacePiece>) {
    let actual = parse(js(input).units());
    let expected = ReplacePattern::new(Some(expected_pieces));
    assert_eq!(actual, expected, "Parsing {input}");
}

fn assert_replace(target: &str, search: &str, replace_string: &str, expected: &str) {
    let replace_pattern = parse(js(replace_string).units());
    let m = exec(search, target);
    let actual = replace_pattern.build_replace_string(m.as_deref(), false);

    assert_eq!(actual, js(expected), "{target}.replace({search}, {replace_string}) === {expected}");
}

#[test]
fn parse_replace_string() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();

    // no backslash => no treatment
    test_parse("hello", vec![static_value("hello")]);

    // \t => TAB
    test_parse("\\thello", vec![static_value("\thello")]);
    test_parse("h\\tello", vec![static_value("h\tello")]);
    test_parse("hello\\t", vec![static_value("hello\t")]);

    // \n => LF
    test_parse("\\nhello", vec![static_value("\nhello")]);

    // \\t => \t
    test_parse("\\\\thello", vec![static_value("\\thello")]);
    test_parse("h\\\\tello", vec![static_value("h\\tello")]);
    test_parse("hello\\\\t", vec![static_value("hello\\t")]);

    // \\\t => \TAB
    test_parse("\\\\\\thello", vec![static_value("\\\thello")]);

    // \\\\t => \\t
    test_parse("\\\\\\\\thello", vec![static_value("\\\\thello")]);

    // \ at the end => no treatment
    test_parse("hello\\", vec![static_value("hello\\")]);

    // \ with unknown char => no treatment
    test_parse("hello\\x", vec![static_value("hello\\x")]);

    // \ with back reference => no treatment
    test_parse("hello\\0", vec![static_value("hello\\0")]);

    test_parse("hello$&", vec![static_value("hello"), ReplacePiece::match_index(0)]);
    test_parse("hello$0", vec![static_value("hello"), ReplacePiece::match_index(0)]);
    test_parse("hello$02", vec![static_value("hello"), ReplacePiece::match_index(0), static_value("2")]);
    test_parse("hello$1", vec![static_value("hello"), ReplacePiece::match_index(1)]);
    test_parse("hello$2", vec![static_value("hello"), ReplacePiece::match_index(2)]);
    test_parse("hello$9", vec![static_value("hello"), ReplacePiece::match_index(9)]);
    test_parse("$9hello", vec![ReplacePiece::match_index(9), static_value("hello")]);

    test_parse("hello$12", vec![static_value("hello"), ReplacePiece::match_index(12)]);
    test_parse("hello$99", vec![static_value("hello"), ReplacePiece::match_index(99)]);
    test_parse("hello$99a", vec![static_value("hello"), ReplacePiece::match_index(99), static_value("a")]);
    test_parse("hello$1a", vec![static_value("hello"), ReplacePiece::match_index(1), static_value("a")]);
    test_parse("hello$100", vec![static_value("hello"), ReplacePiece::match_index(10), static_value("0")]);
    test_parse("hello$100a", vec![static_value("hello"), ReplacePiece::match_index(10), static_value("0a")]);
    test_parse("hello$10a0", vec![static_value("hello"), ReplacePiece::match_index(10), static_value("a0")]);
    test_parse("hello$$", vec![static_value("hello$")]);
    test_parse("hello$$0", vec![static_value("hello$0")]);

    test_parse("hello$`", vec![static_value("hello$`")]);
    test_parse("hello$'", vec![static_value("hello$'")]);
}

#[test]
fn parse_replace_string_with_case_modifiers() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();

    // \U, \u => uppercase  \L, \l => lowercase  \E => cancel

    test_parse("hello\\U$1", vec![static_value("hello"), ReplacePiece::case_ops(1, &['U'])]);
    assert_replace("func privateFunc(", r"func (\w+)\(", "func \\U$1(", "func PRIVATEFUNC(");

    test_parse("hello\\u$1", vec![static_value("hello"), ReplacePiece::case_ops(1, &['u'])]);
    assert_replace("func privateFunc(", r"func (\w+)\(", "func \\u$1(", "func PrivateFunc(");

    test_parse("hello\\L$1", vec![static_value("hello"), ReplacePiece::case_ops(1, &['L'])]);
    assert_replace("func privateFunc(", r"func (\w+)\(", "func \\L$1(", "func privatefunc(");

    test_parse("hello\\l$1", vec![static_value("hello"), ReplacePiece::case_ops(1, &['l'])]);
    assert_replace("func PrivateFunc(", r"func (\w+)\(", "func \\l$1(", "func privateFunc(");

    test_parse(
        "hello$1\\u\\u\\U$4goodbye",
        vec![
            static_value("hello"),
            ReplacePiece::match_index(1),
            ReplacePiece::case_ops(4, &['u', 'u', 'U']),
            static_value("goodbye"),
        ],
    );
    assert_replace("hellogooDbye", r"hello(\w+)", "hello\\u\\u\\l\\l\\U$1", "helloGOodBYE");
}

#[test]
fn replace_has_java_script_semantics() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();

    // Cada esperado é o que `'hi'.replace(search, replaceString)` devolve no JavaScript.
    let test_js_replace_semantics = |target: &str, search: &str, replace_string: &str, expected: &str| {
        let replace_pattern = parse(js(replace_string).units());
        let m = exec(search, target);
        let actual = replace_pattern.build_replace_string(m.as_deref(), false);

        assert_eq!(actual, js(expected), "{target}.replace({search}, {replace_string})");
    };

    test_js_replace_semantics("hi", "hi", "hello", "hello");
    test_js_replace_semantics("hi", "hi", "\\t", "\t");
    test_js_replace_semantics("hi", "hi", "\\n", "\n");
    test_js_replace_semantics("hi", "hi", "\\\\t", "\\t");
    test_js_replace_semantics("hi", "hi", "\\\\n", "\\n");

    // implicit capture group 0
    test_js_replace_semantics("hi", "hi", "hello$&", "hellohi");
    test_js_replace_semantics("hi", "hi", "hello$0", "hellohi");
    test_js_replace_semantics("hi", "hi", "hello$&1", "hellohi1");
    test_js_replace_semantics("hi", "hi", "hello$01", "hellohi1");

    // capture groups have funny semantics in replace strings
    // the replace string interprets $nn as a captured group only if it exists in the search regex
    test_js_replace_semantics("hi", "(hi)", "hello$10", "hellohi0");
    test_js_replace_semantics("hi", "(hi)()()()()()()()()()", "hello$10", "hello");
    test_js_replace_semantics("hi", "(hi)", "hello$100", "hellohi00");
    test_js_replace_semantics("hi", "(hi)", "hello$20", "hello$20");
}

#[test]
fn get_replace_string_if_given_text_is_a_complete_match() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();

    assert_replace("bla", "bla", "hello", "hello");
    assert_replace("bla", "(bla)", "hello", "hello");
    assert_replace("bla", "(bla)", "hello$0", "hellobla");

    let search_regex = r#"let\s+(\w+)\s*=\s*require\s*\(\s*['"]([\w\.\-/]+)\s*['"]\s*\)\s*"#;
    assert_replace("let fs = require('fs')", search_regex, "import * as $1 from '$2';", "import * as fs from 'fs';");
    assert_replace("let something = require('fs')", search_regex, "import * as $1 from '$2';", "import * as something from 'fs';");
    assert_replace("let something = require('fs')", search_regex, "import * as $1 from '$1';", "import * as something from 'something';");
    assert_replace("let something = require('fs')", search_regex, "import * as $2 from '$1';", "import * as fs from 'something';");
    assert_replace(
        "let something = require('fs')",
        search_regex,
        "import * as $0 from '$0';",
        "import * as let something = require('fs') from 'let something = require('fs')';",
    );
    assert_replace("let fs = require('fs')", search_regex, "import * as $1 from '$2';", "import * as fs from 'fs';");
    assert_replace("for ()", "for(.*)", "cat$1", "cat ()");

    // issue #18111
    assert_replace("HRESULT OnAmbientPropertyChange(DISPID   dispid);", r"\b\s{3}\b", " ", " ");
}

#[test]
fn get_replace_string_if_match_is_sub_string_of_the_text() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();

    assert_replace("this is a bla text", "bla", "hello", "hello");
    assert_replace("this is a bla text", "this(?=.*bla)", "that", "that");
    assert_replace("this is a bla text", "(th)is(?=.*bla)", "$1at", "that");
    assert_replace("this is a bla text", "(th)is(?=.*bla)", "$1e", "the");
    assert_replace("this is a bla text", "(th)is(?=.*bla)", "$1ere", "there");
    assert_replace("this is a bla text", "(th)is(?=.*bla)", "$1", "th");
    assert_replace("this is a bla text", "(th)is(?=.*bla)", "ma$1", "math");
    assert_replace("this is a bla text", "(th)is(?=.*bla)", "ma$1s", "maths");
    assert_replace("this is a bla text", "(th)is(?=.*bla)", "$0", "this");
    assert_replace("this is a bla text", "(th)is(?=.*bla)", "$0$1", "thisth");
    assert_replace("this is a bla text", r"bla(?=\stext$)", "foo", "foo");
    assert_replace("this is a bla text", r"b(la)(?=\stext$)", "f$1", "fla");
    assert_replace("this is a bla text", r"b(la)(?=\stext$)", "f$0", "fbla");
    assert_replace("this is a bla text", r"b(la)(?=\stext$)", "$0ah", "blaah");
}

#[test]
fn issue_19740_find_and_replace_capture_group_backreference_inserts_undefined_instead_of_empty_string() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();

    let replace_pattern = parse(js("a{$1}").units());
    let matches = exec("a(z)?", "abcd");
    let actual = replace_pattern.build_replace_string(matches.as_deref(), false);
    assert_eq!(actual, js("a{}"));
}

/// Os casos de preservação de caixa, compartilhados pelos dois últimos testes (o original os repete
/// literalmente, com a diferença de uma linha final).
fn case_preserved_cases() -> Vec<(Vec<&'static str>, &'static str, &'static str)> {
    vec![
        (vec!["abc"], "Def", "def"),
        (vec!["Abc"], "Def", "Def"),
        (vec!["ABC"], "Def", "DEF"),
        (vec!["abc", "Abc"], "Def", "def"),
        (vec!["Abc", "abc"], "Def", "Def"),
        (vec!["ABC", "abc"], "Def", "DEF"),
        (vec!["aBc", "abc"], "Def", "def"),
        (vec!["AbC"], "Def", "Def"),
        (vec!["aBC"], "Def", "def"),
        (vec!["aBc"], "DeF", "deF"),
        (vec!["Foo-Bar"], "newfoo-newbar", "Newfoo-Newbar"),
        (vec!["Foo-Bar-Abc"], "newfoo-newbar-newabc", "Newfoo-Newbar-Newabc"),
        (vec!["Foo-Bar-abc"], "newfoo-newbar", "Newfoo-newbar"),
        (vec!["foo-Bar"], "newfoo-newbar", "newfoo-Newbar"),
        (vec!["foo-BAR"], "newfoo-newbar", "newfoo-NEWBAR"),
        (vec!["foO-BAR"], "NewFoo-NewBar", "newFoo-NEWBAR"),
        (vec!["Foo_Bar"], "newfoo_newbar", "Newfoo_Newbar"),
        (vec!["Foo_Bar_Abc"], "newfoo_newbar_newabc", "Newfoo_Newbar_Newabc"),
        (vec!["Foo_Bar_abc"], "newfoo_newbar", "Newfoo_newbar"),
    ]
}

#[test]
fn build_replace_string_with_case_preserved_test() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();

    let assert_replace = |target: &[&str], replace_string: &str, expected: &str| {
        let target: Vec<JsString> = target.iter().map(|t| js(t)).collect();
        let actual = build_replace_string_with_case_preserved(Some(target.as_slice()), &js(replace_string));
        assert_eq!(actual, js(expected));
    };

    for (target, replace_string, expected) in case_preserved_cases() {
        assert_replace(&target, replace_string, expected);
    }
    assert_replace(&["Foo_Bar-abc"], "newfoo_newbar-abc", "Newfoo_newbar-abc");
    assert_replace(&["foo_Bar"], "newfoo_newbar", "newfoo_Newbar");
    assert_replace(&["Foo_BAR"], "newfoo_newbar", "Newfoo_NEWBAR");
}

#[test]
fn preserve_case() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();

    let assert_replace = |target: &[&str], replace_string: &str, expected: &str| {
        let replace_pattern = parse(js(replace_string).units());
        let target: Vec<Option<JsString>> = target.iter().map(|t| Some(js(t))).collect();
        let actual = replace_pattern.build_replace_string(Some(target.as_slice()), true);
        assert_eq!(actual, js(expected));
    };

    for (target, replace_string, expected) in case_preserved_cases() {
        assert_replace(&target, replace_string, expected);
    }
    assert_replace(&["Foo_Bar-abc"], "newfoo_newbar-abc", "Newfoo_newbar-abc");
    assert_replace(&["foo_Bar"], "newfoo_newbar", "newfoo_Newbar");
    assert_replace(&["foo_BAR"], "newfoo_newbar", "newfoo_NEWBAR");
}
