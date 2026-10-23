//! Porte de `src/vs/editor/contrib/snippet/test/browser/snippetParser.test.ts` (suíte `SnippetParser`), caso a caso e na mesma ordem.

#[path = "../../je-base/tests/common/mod.rs"]
mod common;

use std::collections::HashSet;
use std::rc::Rc;

use common::ensure_no_disposables_are_leaked_in_test_suite;
use je_base::JsString;
use je_base::js_regexp::JsRegExp;
use je_editor::snippet::snippet_parser::MarkerKind::*;
use je_editor::snippet::snippet_parser::{
    FormatStringData, Marker, MarkerKind, MarkerRef, Scanner, SnippetParser, TokenType, VariableResolver,
};
use zyarr::Flags;

fn u(text: &str) -> Vec<u16> {
    text.encode_utf16().collect()
}

fn js(text: &str) -> JsString {
    JsString::from(text)
}

fn parse(text: &str) -> MarkerRef {
    SnippetParser::new().parse(&u(text), false, false)
}

/// `parse(template, true)`: com `insertFinalTabstop`.
fn parse_with_final_tabstop(text: &str) -> MarkerRef {
    SnippetParser::new().parse(&u(text), true, false)
}

fn assert_next(scanner: &mut Scanner, kinds: &[TokenType]) {
    for kind in kinds {
        assert_eq!(scanner.next().kind, *kind);
    }
}

#[test]
fn scanner() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();

    let mut scanner = Scanner::default();
    assert_eq!(scanner.next().kind, TokenType::EOF);

    scanner.text(&u("abc"));
    assert_next(&mut scanner, &[TokenType::VariableName, TokenType::EOF]);

    scanner.text(&u("{{abc}}"));
    assert_next(
        &mut scanner,
        &[
            TokenType::CurlyOpen,
            TokenType::CurlyOpen,
            TokenType::VariableName,
            TokenType::CurlyClose,
            TokenType::CurlyClose,
            TokenType::EOF,
        ],
    );

    scanner.text(&u("abc() "));
    assert_next(&mut scanner, &[TokenType::VariableName, TokenType::Format, TokenType::EOF]);

    scanner.text(&u("abc 123"));
    assert_next(&mut scanner, &[TokenType::VariableName, TokenType::Format, TokenType::Int, TokenType::EOF]);

    scanner.text(&u("$foo"));
    assert_next(&mut scanner, &[TokenType::Dollar, TokenType::VariableName, TokenType::EOF]);

    scanner.text(&u("$foo_bar"));
    assert_next(&mut scanner, &[TokenType::Dollar, TokenType::VariableName, TokenType::EOF]);

    scanner.text(&u("$foo-bar"));
    assert_next(
        &mut scanner,
        &[TokenType::Dollar, TokenType::VariableName, TokenType::Dash, TokenType::VariableName, TokenType::EOF],
    );

    scanner.text(&u("${foo}"));
    assert_next(
        &mut scanner,
        &[TokenType::Dollar, TokenType::CurlyOpen, TokenType::VariableName, TokenType::CurlyClose, TokenType::EOF],
    );

    scanner.text(&u("${1223:foo}"));
    assert_next(
        &mut scanner,
        &[
            TokenType::Dollar,
            TokenType::CurlyOpen,
            TokenType::Int,
            TokenType::Colon,
            TokenType::VariableName,
            TokenType::CurlyClose,
            TokenType::EOF,
        ],
    );

    scanner.text(&u("\\${}"));
    assert_next(&mut scanner, &[TokenType::Backslash, TokenType::Dollar, TokenType::CurlyOpen, TokenType::CurlyClose]);

    scanner.text(&u("${foo/regex/format/option}"));
    assert_next(
        &mut scanner,
        &[
            TokenType::Dollar,
            TokenType::CurlyOpen,
            TokenType::VariableName,
            TokenType::Forwardslash,
            TokenType::VariableName,
            TokenType::Forwardslash,
            TokenType::VariableName,
            TokenType::Forwardslash,
            TokenType::VariableName,
            TokenType::CurlyClose,
            TokenType::EOF,
        ],
    );
}

fn assert_text(value: &str, expected: &str) {
    let actual = SnippetParser::as_insert_text(&u(value));
    assert_eq!(actual, expected);
}

/// `assertMarker` sobre uma lista de marcadores: confere o tipo de cada um, do último para o primeiro.
fn assert_markers(mut markers: Vec<MarkerRef>, ctors: &[MarkerKind]) {
    let mut ctors = ctors.to_vec();
    while let Some(marker) = markers.pop() {
        let ctor = ctors.pop().expect("mais marcadores do que construtores");
        assert_eq!(marker.kind(), ctor);
    }
    assert_eq!(markers.len(), ctors.len());
    assert_eq!(markers.len(), 0);
}

/// `assertMarker` sobre um texto: confere os filhos do snippet analisado.
fn assert_marker(input: &str, ctors: &[MarkerKind]) {
    assert_markers(parse(input).children(), ctors);
}

fn assert_text_and_marker(value: &str, escaped: &str, ctors: &[MarkerKind]) {
    assert_text(value, escaped);
    assert_marker(value, ctors);
}

fn assert_escaped(value: &str, expected: &str) {
    let actual = SnippetParser::escape(&u(value));
    assert_eq!(actual, expected);
}

#[test]
fn parser_escaped() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    assert_escaped("foo$0", "foo\\$0");
    assert_escaped("foo\\$0", "foo\\\\\\$0");
    assert_escaped("f$1oo$0", "f\\$1oo\\$0");
    assert_escaped("${1:foo}$0", "\\${1:foo\\}\\$0");
    assert_escaped("$", "\\$");
}

#[test]
fn parser_text() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    assert_text("$", "$");
    assert_text("\\\\$", "\\$");
    assert_text("{", "{");
    assert_text("\\}", "}");
    assert_text("\\abc", "\\abc");
    assert_text("foo${f:\\}}bar", "foo}bar");
    assert_text("\\{", "\\{");
    assert_text("I need \\\\\\$", "I need \\$");
    assert_text("\\", "\\");
    assert_text("\\{{", "\\{{");
    assert_text("{{", "{{");
    assert_text("{{dd", "{{dd");
    assert_text("}}", "}}");
    assert_text("ff}}", "ff}}");

    assert_text("farboo", "farboo");
    assert_text("far{{}}boo", "far{{}}boo");
    assert_text("far{{123}}boo", "far{{123}}boo");
    assert_text("far\\{{123}}boo", "far\\{{123}}boo");
    assert_text("far{{id:bern}}boo", "far{{id:bern}}boo");
    assert_text("far{{id:bern {{basel}}}}boo", "far{{id:bern {{basel}}}}boo");
    assert_text("far{{id:bern {{id:basel}}}}boo", "far{{id:bern {{id:basel}}}}boo");
    assert_text("far{{id:bern {{id2:basel}}}}boo", "far{{id:bern {{id2:basel}}}}boo");
}

#[test]
fn parser_tm_text() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    assert_text_and_marker("foo${1:bar}}", "foobar}", &[Text, Placeholder, Text]);
    assert_text_and_marker("foo${1:bar}${2:foo}}", "foobarfoo}", &[Text, Placeholder, Placeholder, Text]);

    assert_text_and_marker("foo${1:bar\\}${2:foo}}", "foobar}foo", &[Text, Placeholder]);

    let snippet = parse("foo${1:bar\\}${2:foo}}");
    let placeholder = snippet.children()[1].clone();
    let children = placeholder.children();

    assert_eq!(placeholder.as_placeholder().unwrap().index, 1);
    assert!(children[0].as_text().is_some());
    assert_eq!(children[0].to_js_string(), "bar}");
    assert!(children[1].as_placeholder().is_some());
    assert_eq!(children[1].to_js_string(), "foo");
}

#[test]
fn parser_placeholder() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    assert_text_and_marker("farboo", "farboo", &[Text]);
    assert_text_and_marker("far{{}}boo", "far{{}}boo", &[Text]);
    assert_text_and_marker("far{{123}}boo", "far{{123}}boo", &[Text]);
    assert_text_and_marker("far\\{{123}}boo", "far\\{{123}}boo", &[Text]);
}

#[test]
fn parser_literal_code() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    assert_text_and_marker("far`123`boo", "far`123`boo", &[Text]);
    assert_text_and_marker("far\\`123\\`boo", "far\\`123\\`boo", &[Text]);
}

#[test]
fn parser_variables_tabstop() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    assert_text_and_marker("$far-boo", "-boo", &[Variable, Text]);
    assert_text_and_marker("\\$far-boo", "$far-boo", &[Text]);
    assert_text_and_marker("far$farboo", "far", &[Text, Variable]);
    assert_text_and_marker("far${farboo}", "far", &[Text, Variable]);
    assert_text_and_marker("$123", "", &[Placeholder]);
    assert_text_and_marker("$farboo", "", &[Variable]);
    assert_text_and_marker("$far12boo", "", &[Variable]);
    assert_text_and_marker("000_${far}_000", "000__000", &[Text, Variable, Text]);
    assert_text_and_marker("FFF_${TM_SELECTED_TEXT}_FFF$0", "FFF__FFF", &[Text, Variable, Text, Placeholder]);
}

#[test]
fn parser_variables_placeholder_with_defaults() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    assert_text_and_marker("${name:value}", "value", &[Variable]);
    assert_text_and_marker("${1:value}", "value", &[Placeholder]);
    assert_text_and_marker("${1:bar${2:foo}bar}", "barfoobar", &[Placeholder]);

    assert_text_and_marker("${name:value", "${name:value", &[Text]);
    assert_text_and_marker("${1:bar${2:foobar}", "${1:barfoobar", &[Text, Placeholder]);
}

#[test]
fn parser_variable_transforms() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    assert_text_and_marker("${foo///}", "", &[Variable]);
    assert_text_and_marker("${foo/regex/format/gmi}", "", &[Variable]);
    assert_text_and_marker("${foo/([A-Z][a-z])/format/}", "", &[Variable]);

    // invalid regex
    assert_text_and_marker("${foo/([A-Z][a-z])/format/GMI}", "${foo/([A-Z][a-z])/format/GMI}", &[Text]);
    assert_text_and_marker("${foo/([A-Z][a-z])/format/funky}", "${foo/([A-Z][a-z])/format/funky}", &[Text]);
    assert_text_and_marker("${foo/([A-Z][a-z]/format/}", "${foo/([A-Z][a-z]/format/}", &[Text]);

    // tricky regex
    assert_text_and_marker("${foo/m\\/atch/$1/i}", "", &[Variable]);
    assert_marker("${foo/regex/format/options}", &[Text]);

    // incomplete
    assert_text_and_marker("${foo///", "${foo///", &[Text]);
    assert_text_and_marker("${foo/regex/format/options", "${foo/regex/format/options", &[Text]);

    // format string
    assert_marker("${foo/.*/${0:fooo}/i}", &[Variable]);
    assert_marker("${foo/.*/${1}/i}", &[Variable]);
    assert_marker("${foo/.*/$1/i}", &[Variable]);
    assert_marker("${foo/.*/This-$1-encloses/i}", &[Variable]);
    assert_marker("${foo/.*/complex${1:else}/i}", &[Variable]);
    assert_marker("${foo/.*/complex${1:-else}/i}", &[Variable]);
    assert_marker("${foo/.*/complex${1:+if}/i}", &[Variable]);
    assert_marker("${foo/.*/complex${1:?if:else}/i}", &[Variable]);
    assert_marker("${foo/.*/complex${1:/upcase}/i}", &[Variable]);
}

#[test]
fn parser_placeholder_transforms() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    assert_text_and_marker("${1///}", "", &[Placeholder]);
    assert_text_and_marker("${1/regex/format/gmi}", "", &[Placeholder]);
    assert_text_and_marker("${1/([A-Z][a-z])/format/}", "", &[Placeholder]);

    // tricky regex
    assert_text_and_marker("${1/m\\/atch/$1/i}", "", &[Placeholder]);
    assert_marker("${1/regex/format/options}", &[Text]);

    // incomplete
    assert_text_and_marker("${1///", "${1///", &[Text]);
    assert_text_and_marker("${1/regex/format/options", "${1/regex/format/options", &[Text]);
}

#[test]
fn no_way_to_escape_forward_slash_in_snippet_regex_36715() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    assert_marker("${TM_DIRECTORY/src\\//$1/}", &[Variable]);
}

#[test]
fn no_way_to_escape_forward_slash_in_snippet_format_section_37562() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    assert_marker("${TM_SELECTED_TEXT/a/\\/$1/g}", &[Variable]);
    assert_marker("${TM_SELECTED_TEXT/a/in\\/$1ner/g}", &[Variable]);
    assert_marker("${TM_SELECTED_TEXT/a/end\\//g}", &[Variable]);
}

#[test]
fn parser_placeholder_with_choice() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();

    assert_text_and_marker("${1|one,two,three|}", "one", &[Placeholder]);
    assert_text_and_marker("${1|one|}", "one", &[Placeholder]);
    assert_text_and_marker("${1|one1,two2|}", "one1", &[Placeholder]);
    assert_text_and_marker("${1|one1\\,two2|}", "one1,two2", &[Placeholder]);
    assert_text_and_marker("${1|one1\\|two2|}", "one1|two2", &[Placeholder]);
    assert_text_and_marker("${1|one1\\atwo2|}", "one1\\atwo2", &[Placeholder]);
    assert_text_and_marker("${1|one,two,three,|}", "${1|one,two,three,|}", &[Text]);
    assert_text_and_marker("${1|one,", "${1|one,", &[Text]);

    let snippet = parse("${1|one,two,three|}");
    // Os dois marcadores esperados, na ordem da visita: o Placeholder e o Choice com 3 opções, todas Text.
    let mut step = 0;
    snippet.walk(|marker| {
        match step {
            0 => assert_eq!(marker.kind(), Placeholder),
            1 => {
                assert_eq!(marker.kind(), Choice);
                let options = marker.as_choice().unwrap().options.borrow();
                assert_eq!(options.len(), 3);
                assert!(options.iter().all(|option| option.kind() == Text));
            }
            _ => panic!("visita inesperada"),
        }
        step += 1;
        true
    });
}

#[test]
fn snippet_choices_unable_to_escape_comma_and_pipe_31521() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    assert_text_and_marker(
        "console.log(${1|not\\, not, five, 5, 1   23|});",
        "console.log(not, not);",
        &[Text, Placeholder, Text],
    );
}

#[test]
fn marker_to_textmate_string() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();

    fn assert_textsnippet_string(input: &str, expected: &str) {
        let snippet = parse(input);
        let actual = snippet.to_textmate_string();
        assert_eq!(actual, expected);
    }

    assert_textsnippet_string("$1", "$1");
    assert_textsnippet_string("\\$1", "\\$1");
    assert_textsnippet_string(
        "console.log(${1|not\\, not, five, 5, 1   23|});",
        "console.log(${1|not\\, not, five, 5, 1   23|});",
    );
    assert_textsnippet_string(
        "console.log(${1|not\\, not, \\| five, 5, 1   23|});",
        "console.log(${1|not\\, not, \\| five, 5, 1   23|});",
    );
    assert_textsnippet_string(
        "${1|cho\\,ices,wi\\|th,esc\\\\aping,chall\\\\\\,enges|}",
        "${1|cho\\,ices,wi\\|th,esc\\\\aping,chall\\\\\\,enges|}",
    );
    assert_textsnippet_string("this is text", "this is text");
    assert_textsnippet_string("this ${1:is ${2:nested with $var}}", "this ${1:is ${2:nested with ${var}}}");
    assert_textsnippet_string("this ${1:is ${2:nested with $var}}}", "this ${1:is ${2:nested with ${var}}}\\}");
}

#[test]
fn marker_to_textmate_string_identity() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();

    fn assert_ident(input: &str) {
        // full loop: (1) parse input, (2) generate textmate string, (3) parse, (4) ensure both trees are equal
        let snippet = parse(input);
        let input2 = snippet.to_textmate_string();
        let snippet2 = SnippetParser::new().parse(input2.units(), false, false);

        fn check_check_children(marker1: &MarkerRef, marker2: &MarkerRef) {
            assert_eq!(marker1.kind(), marker2.kind());

            let (children1, children2) = (marker1.children(), marker2.children());
            assert_eq!(children1.len(), children2.len());
            assert_eq!(marker1.to_js_string(), marker2.to_js_string());

            for (child1, child2) in children1.iter().zip(children2.iter()) {
                check_check_children(child1, child2);
            }
        }

        check_check_children(&snippet, &snippet2);
    }

    assert_ident("$1");
    assert_ident("\\$1");
    assert_ident("console.log(${1|not\\, not, five, 5, 1   23|});");
    assert_ident("console.log(${1|not\\, not, \\| five, 5, 1   23|});");
    assert_ident("this is text");
    assert_ident("this ${1:is ${2:nested with $var}}");
    assert_ident("this ${1:is ${2:nested with $var}}}");
    assert_ident("this ${1:is ${2:nested with $var}} and repeating $1");
}

#[test]
fn parser_choise_marker() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    let snippet = parse("${1|one,two,three|}");
    let placeholders = snippet.placeholders();

    assert_eq!(placeholders.len(), 1);
    assert!(placeholders[0].choice().is_some());
    assert_eq!(placeholders[0].children()[0].kind(), Choice);
    assert_eq!(placeholders[0].children()[0].as_choice().unwrap().options.borrow().len(), 3);

    assert_text("${1|one,two,three|}", "one");
    assert_text("\\${1|one,two,three|}", "${1|one,two,three|}");
    assert_text("${1\\|one,two,three|}", "${1\\|one,two,three|}");
    assert_text("${1||}", "${1||}");
}

#[test]
fn backslash_character_escape_in_choice_tabstop_doesnt_work_58494() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();

    let snippet = parse("${1|\\,,},$,\\|,\\\\|}");
    let placeholders = snippet.placeholders();
    assert_eq!(placeholders.len(), 1);
    assert!(placeholders[0].choice().is_some());
}

#[test]
fn parser_only_textmate() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    let mut p = SnippetParser::new();
    assert_markers(p.parse(&u("far{{}}boo"), false, false).children(), &[Text]);
    assert_markers(p.parse(&u("far{{123}}boo"), false, false).children(), &[Text]);
    assert_markers(p.parse(&u("far\\{{123}}boo"), false, false).children(), &[Text]);

    assert_markers(p.parse(&u("far$0boo"), false, false).children(), &[Text, Placeholder, Text]);
    assert_markers(p.parse(&u("far${123}boo"), false, false).children(), &[Text, Placeholder, Text]);
    assert_markers(p.parse(&u("far\\${123}boo"), false, false).children(), &[Text]);
}

#[test]
fn parser_real_world() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    let snippet = parse("console.warn(${1: $TM_SELECTED_TEXT })");
    let marker = snippet.children();

    assert_eq!(marker[0].to_js_string(), "console.warn(");
    assert!(marker[1].as_placeholder().is_some());
    assert_eq!(marker[2].to_js_string(), ")");

    let placeholder = marker[1].clone();
    assert_eq!(placeholder.as_placeholder().unwrap().index, 1);
    let placeholder_children = placeholder.children();
    assert_eq!(placeholder_children.len(), 3);
    assert!(placeholder_children[0].as_text().is_some());
    assert!(placeholder_children[1].as_variable().is_some());
    assert!(placeholder_children[2].as_text().is_some());
    assert_eq!(placeholder_children[0].to_js_string(), " ");
    assert_eq!(placeholder_children[1].to_js_string(), "");
    assert_eq!(placeholder_children[2].to_js_string(), " ");

    let nested_variable = placeholder_children[1].clone();
    assert_eq!(nested_variable.as_variable().unwrap().name, "TM_SELECTED_TEXT");
    assert_eq!(nested_variable.children().len(), 0);

    let snippet = parse("$TM_SELECTED_TEXT");
    let marker = snippet.children();
    assert_eq!(marker.len(), 1);
    assert!(marker[0].as_variable().is_some());
}

#[test]
fn parser_transform_example() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    let snippet = parse("${1:name} : ${2:type}${3/\\s:=(.*)/${1:+ :=}${1}/};\n$0");
    let children = snippet.children();

    //${1:name}
    assert!(children[0].as_placeholder().is_some());
    assert_eq!(children[0].children().len(), 1);
    assert_eq!(children[0].children()[0].to_js_string(), "name");
    assert!(children[0].transform().is_none());

    // :
    assert!(children[1].as_text().is_some());
    assert_eq!(children[1].to_js_string(), " : ");

    //${2:type}
    assert!(children[2].as_placeholder().is_some());
    assert_eq!(children[2].children().len(), 1);
    assert_eq!(children[2].children()[0].to_js_string(), "type");

    //${3/\\s:=(.*)/${1:+ :=}${1}/}
    assert!(children[3].as_placeholder().is_some());
    assert_eq!(children[3].children().len(), 0);
    assert!(children[3].transform().is_some());
    let transform = children[3].transform().unwrap();
    // `deepStrictEqual(transform.regexp, /\s:=(.*)/)`: mesmo padrão e mesmas flags.
    assert_eq!(transform.as_transform().unwrap().source(), "\\s:=(.*)");
    assert_eq!(transform.as_transform().unwrap().regexp().flags(), Flags::default());
    let transform_children = transform.children();
    assert_eq!(transform_children.len(), 2);
    assert!(transform_children[0].as_format_string().is_some());
    assert_eq!(transform_children[0].as_format_string().unwrap().index, 1);
    assert_eq!(transform_children[0].as_format_string().unwrap().if_value, Some(js(" :=")));
    assert!(transform_children[1].as_format_string().is_some());
    assert_eq!(transform_children[1].as_format_string().unwrap().index, 1);
    assert!(children[4].as_text().is_some());
    assert_eq!(children[4].to_js_string(), ";\n");
}

#[test]
fn parser_default_placeholder_values() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();

    assert_marker("errorContext: `${1:err}`, error: $1", &[Text, Placeholder, Text, Placeholder]);

    let snippet = parse("errorContext: `${1:err}`, error:$1");
    let children = snippet.children();
    let (p1, p2) = (&children[1], &children[3]);

    assert_eq!(p1.as_placeholder().unwrap().index, 1);
    assert_eq!(p1.children().len(), 1);
    assert_eq!(p1.children()[0].to_js_string(), "err");

    assert_eq!(p2.as_placeholder().unwrap().index, 1);
    assert_eq!(p2.children().len(), 1);
    assert_eq!(p2.children()[0].to_js_string(), "err");
}

#[test]
fn parser_default_placeholder_values_and_one_transform() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();

    assert_marker("errorContext: `${1:err}`, error: ${1/err/ok/}", &[Text, Placeholder, Text, Placeholder]);

    let snippet = parse("errorContext: `${1:err}`, error:${1/err/ok/}");
    let children = snippet.children();
    let (p3, p4) = (&children[1], &children[3]);

    assert_eq!(p3.as_placeholder().unwrap().index, 1);
    assert_eq!(p3.children().len(), 1);
    assert_eq!(p3.children()[0].to_js_string(), "err");
    assert!(p3.transform().is_none());

    assert_eq!(p4.as_placeholder().unwrap().index, 1);
    assert_eq!(p4.children().len(), 1);
    assert_eq!(p4.children()[0].to_js_string(), "err");
    assert!(p4.transform().is_some());
}

#[test]
fn repeated_snippet_placeholder_should_always_inherit_31040() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    assert_text("${1:foo}-abc-$1", "foo-abc-foo");
    assert_text("${1:foo}-abc-${1}", "foo-abc-foo");
    assert_text("${1:foo}-abc-${1:bar}", "foo-abc-foo");
    assert_text("${1}-abc-${1:foo}", "foo-abc-foo");
}

#[test]
fn backspace_esapce_in_tm_only_16212() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    let actual = SnippetParser::as_insert_text(&u("Foo \\\\${abc}bar"));
    assert_eq!(actual, "Foo \\bar");
}

#[test]
fn colon_as_variable_placeholder_value_16717() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    let mut actual = SnippetParser::as_insert_text(&u("${TM_SELECTED_TEXT:foo:bar}"));
    assert_eq!(actual, "foo:bar");

    actual = SnippetParser::as_insert_text(&u("${1:foo:bar}"));
    assert_eq!(actual, "foo:bar");
}

#[test]
fn incomplete_placeholder() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    assert_text_and_marker("${1:}", "", &[Placeholder]);
}

#[test]
fn marker_len() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();

    fn assert_len(template: &str, lengths: &[usize]) {
        let mut lengths: std::collections::VecDeque<usize> = lengths.iter().copied().collect();
        let snippet = parse_with_final_tabstop(template);
        snippet.walk(|m| {
            let expected = lengths.pop_front();
            assert_eq!(Some(m.len()), expected);
            true
        });
        assert_eq!(lengths.len(), 0);
    }

    assert_len("text$0", &[4, 0]);
    assert_len("$1text$0", &[0, 4, 0]);
    assert_len("te$1xt$0", &[2, 0, 2, 0]);
    assert_len("errorContext: `${1:err}`, error: $0", &[15, 0, 3, 10, 0]);
    assert_len("errorContext: `${1:err}`, error: $1$0", &[15, 0, 3, 10, 0, 3, 0]);
    assert_len("$TM_SELECTED_TEXT$0", &[0, 0]);
    assert_len("${TM_SELECTED_TEXT:def}$0", &[0, 3, 0]);
}

#[test]
fn parser_parent_node() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    let snippet = parse_with_final_tabstop("This ${1:is ${2:nested}}$0");

    let placeholders = snippet.placeholders();
    assert_eq!(placeholders.len(), 3);
    let (first, second) = (&placeholders[0], &placeholders[1]);
    assert_eq!(first.as_placeholder().unwrap().index, 1);
    assert_eq!(second.as_placeholder().unwrap().index, 2);
    assert!(Rc::ptr_eq(&second.parent().unwrap(), first));
    assert!(Rc::ptr_eq(&first.parent().unwrap(), &snippet));

    let snippet = parse_with_final_tabstop("${VAR:default${1:value}}$0");
    let placeholders = snippet.placeholders();
    assert_eq!(placeholders.len(), 2);
    let first = &placeholders[0];
    assert_eq!(first.as_placeholder().unwrap().index, 1);

    assert!(snippet.children()[0].as_variable().is_some());
    assert!(Rc::ptr_eq(&first.parent().unwrap(), &snippet.children()[0]));
}

#[test]
fn textmate_snippet_enclosing_placeholders() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    let snippet = parse_with_final_tabstop("This ${1:is ${2:nested}}$0");
    let placeholders = snippet.placeholders();
    let (first, second) = (&placeholders[0], &placeholders[1]);

    assert!(snippet.enclosing_placeholders(first).is_empty());
    let enclosing = snippet.enclosing_placeholders(second);
    assert_eq!(enclosing.len(), 1);
    assert!(Rc::ptr_eq(&enclosing[0], first));
}

#[test]
fn textmate_snippet_offset() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    let snippet = parse_with_final_tabstop("te$1xt");
    let children = snippet.children();
    assert_eq!(snippet.offset(&children[0]), Some(0));
    assert_eq!(snippet.offset(&children[1]), Some(2));
    assert_eq!(snippet.offset(&children[2]), Some(2));

    let snippet = parse_with_final_tabstop("${TM_SELECTED_TEXT:def}");
    let children = snippet.children();
    assert_eq!(snippet.offset(&children[0]), Some(0));
    assert_eq!(snippet.offset(&children[0].children()[0]), Some(0));

    // forgein marker
    assert_eq!(snippet.offset(&Marker::new_text("foo")), None);
}

#[test]
fn textmate_snippet_placeholder() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    let snippet = parse_with_final_tabstop("te$1xt$0");
    let mut placeholders = snippet.placeholders();
    assert_eq!(placeholders.len(), 2);

    let snippet = parse_with_final_tabstop("te$1xt$1$0");
    placeholders = snippet.placeholders();
    assert_eq!(placeholders.len(), 3);

    let snippet = parse_with_final_tabstop("te$1xt$2$0");
    placeholders = snippet.placeholders();
    assert_eq!(placeholders.len(), 3);

    let snippet = parse_with_final_tabstop("${1:bar${2:foo}bar}$0");
    placeholders = snippet.placeholders();
    assert_eq!(placeholders.len(), 3);
}

#[test]
fn textmate_snippet_replace_1_2() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    let snippet = parse_with_final_tabstop("aaa${1:bbb${2:ccc}}$0");

    assert_eq!(snippet.placeholders().len(), 3);
    let second = snippet.placeholders()[1].clone();
    assert_eq!(second.as_placeholder().unwrap().index, 2);

    let enclosing = snippet.enclosing_placeholders(&second);
    assert_eq!(enclosing.len(), 1);
    assert_eq!(enclosing[0].as_placeholder().unwrap().index, 1);

    let nested = parse_with_final_tabstop("ddd$1eee$0");
    snippet.replace(&second, nested.children());

    assert_eq!(snippet.to_js_string(), "aaabbbdddeee");
    let placeholders = snippet.placeholders();
    assert_eq!(placeholders.len(), 4);
    assert_eq!(placeholders[0].as_placeholder().unwrap().index, 1);
    assert_eq!(placeholders[1].as_placeholder().unwrap().index, 1);
    assert_eq!(placeholders[2].as_placeholder().unwrap().index, 0);
    assert_eq!(placeholders[3].as_placeholder().unwrap().index, 0);

    let new_enclosing = snippet.enclosing_placeholders(&placeholders[1]);
    assert!(Rc::ptr_eq(&new_enclosing[0], &placeholders[0]));
    assert_eq!(new_enclosing.len(), 1);
    assert_eq!(new_enclosing[0].as_placeholder().unwrap().index, 1);
}

#[test]
fn textmate_snippet_replace_2_2() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    let snippet = parse_with_final_tabstop("aaa${1:bbb${2:ccc}}$0");

    assert_eq!(snippet.placeholders().len(), 3);
    let second = snippet.placeholders()[1].clone();
    assert_eq!(second.as_placeholder().unwrap().index, 2);

    let nested = parse_with_final_tabstop("dddeee$0");
    snippet.replace(&second, nested.children());

    assert_eq!(snippet.to_js_string(), "aaabbbdddeee");
    assert_eq!(snippet.placeholders().len(), 3);
}

#[test]
fn snippet_order_for_placeholders_28185() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();

    let _10 = Marker::new_placeholder(10);
    let _2 = Marker::new_placeholder(2);

    assert_eq!(Marker::compare_by_index(&_10, &_2), std::cmp::Ordering::Greater);
}

#[test]
fn maximum_call_stack_size_exceeded_28983() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    parse("${1:${foo:${1}}}");
}

#[test]
fn snippet_can_freeze_the_editor_30407() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();

    let mut seen: HashSet<*const Marker> = HashSet::new();

    seen.clear();
    parse("class ${1:${TM_FILENAME/(?:\\A|_)([A-Za-z0-9]+)(?:\\.rb)?/(?2::\\u$1)/g}} < ${2:Application}Controller\n  $3\nend")
        .walk(|marker| {
            assert!(!seen.contains(&Rc::as_ptr(marker)));
            seen.insert(Rc::as_ptr(marker));
            true
        });

    seen.clear();
    parse("${1:${FOO:abc$1def}}").walk(|marker| {
        assert!(!seen.contains(&Rc::as_ptr(marker)));
        seen.insert(Rc::as_ptr(marker));
        true
    });
}

#[test]
fn snippets_make_parser_ignore_0_choice_31599() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    assert_text_and_marker("${0|foo,bar|}", "${0|foo,bar|}", &[Text]);
    assert_text_and_marker("${1|foo,bar|}", "foo", &[Placeholder]);
}

#[test]
fn transform_format_string_resolve() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();

    fn shorthand(name: &str, value: &str) -> JsString {
        FormatStringData::new(1, Some(name), None, None).resolve(Some(&u(value)))
    }

    fn conditional(if_value: Option<&str>, else_value: Option<&str>, value: Option<&str>) -> JsString {
        let value = value.map(u);
        FormatStringData::new(1, None, if_value.map(js), else_value.map(js)).resolve(value.as_deref())
    }

    // shorthand functions
    assert_eq!(shorthand("upcase", "foo"), "FOO");
    assert_eq!(shorthand("downcase", "FOO"), "foo");
    assert_eq!(shorthand("capitalize", "bar"), "Bar");
    assert_eq!(shorthand("capitalize", "bar no repeat"), "Bar no repeat");
    assert_eq!(shorthand("pascalcase", "bar-foo"), "BarFoo");
    assert_eq!(shorthand("pascalcase", "bar-42-foo"), "Bar42Foo");
    assert_eq!(shorthand("pascalcase", "snake_AndPascalCase"), "SnakeAndPascalCase");
    assert_eq!(shorthand("pascalcase", "kebab-AndPascalCase"), "KebabAndPascalCase");
    assert_eq!(shorthand("pascalcase", "_justPascalCase"), "JustPascalCase");
    assert_eq!(shorthand("camelcase", "bar-foo"), "barFoo");
    assert_eq!(shorthand("camelcase", "bar-42-foo"), "bar42Foo");
    assert_eq!(shorthand("camelcase", "snake_AndCamelCase"), "snakeAndCamelCase");
    assert_eq!(shorthand("camelcase", "kebab-AndCamelCase"), "kebabAndCamelCase");
    assert_eq!(shorthand("camelcase", "_JustCamelCase"), "justCamelCase");
    assert_eq!(shorthand("kebabcase", "barFoo"), "bar-foo");
    assert_eq!(shorthand("kebabcase", "BarFoo"), "bar-foo");
    assert_eq!(shorthand("kebabcase", "ABarFoo"), "a-bar-foo");
    assert_eq!(shorthand("kebabcase", "bar42Foo"), "bar42-foo");
    assert_eq!(shorthand("kebabcase", "snake_AndPascalCase"), "snake-and-pascal-case");
    assert_eq!(shorthand("kebabcase", "kebab-AndCamelCase"), "kebab-and-camel-case");
    assert_eq!(shorthand("kebabcase", "_justPascalCase"), "just-pascal-case");
    assert_eq!(shorthand("kebabcase", "__UPCASE__"), "upcase");
    assert_eq!(shorthand("kebabcase", "__BAR_FOO__"), "bar-foo");
    assert_eq!(shorthand("snakecase", "bar-foo"), "bar_foo");
    assert_eq!(shorthand("snakecase", "bar-42-foo"), "bar_42_foo");
    assert_eq!(shorthand("snakecase", "snake_AndPascalCase"), "snake_and_pascal_case");
    assert_eq!(shorthand("snakecase", "kebab-AndPascalCase"), "kebab_and_pascal_case");
    assert_eq!(shorthand("snakecase", "_justPascalCase"), "_just_pascal_case");
    assert_eq!(shorthand("notKnown", "input"), "input");

    // if
    assert_eq!(conditional(Some("foo"), None, None), "");
    assert_eq!(conditional(Some("foo"), None, Some("")), "");
    assert_eq!(conditional(Some("foo"), None, Some("bar")), "foo");

    // else
    assert_eq!(conditional(None, Some("foo"), None), "foo");
    assert_eq!(conditional(None, Some("foo"), Some("")), "foo");
    assert_eq!(conditional(None, Some("foo"), Some("bar")), "bar");

    // if-else
    assert_eq!(conditional(Some("bar"), Some("foo"), None), "foo");
    assert_eq!(conditional(Some("bar"), Some("foo"), Some("")), "foo");
    assert_eq!(conditional(Some("bar"), Some("foo"), Some("baz")), "bar");
}

#[test]
fn unicode_variable_transformations() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();

    struct Resolver;

    impl VariableResolver for Resolver {
        fn resolve(&self, variable: &MarkerRef) -> Option<JsString> {
            let value = match variable.as_variable().unwrap().name.as_str() {
                "RUSSIAN" => "одинДва",
                "GREEK" => "έναςΔύο",
                "TURKISH" => "istanbulLı",
                "JAPANESE" => "こんにちは",
                _ => return None,
            };
            Some(js(value))
        }
    }

    fn assert_transform(transform_name: &str, var_name: &str, expected: &str) {
        let mut p = SnippetParser::new();
        let snippet = p.parse(&u(&format!("${{{var_name}/(.*)/${{1:/{transform_name}}}/}}")), false, false);
        let variable = snippet.children()[0].clone();
        variable.resolve_variable(&Resolver);
        let resolved = variable.to_js_string();
        assert_eq!(resolved, expected, "{transform_name} failed for {var_name}");
    }

    assert_transform("kebabcase", "RUSSIAN", "один-два");
    assert_transform("kebabcase", "GREEK", "ένας-δύο");
    assert_transform("snakecase", "RUSSIAN", "один_два");
    assert_transform("snakecase", "GREEK", "ένας_δύο");
    assert_transform("camelcase", "RUSSIAN", "одинДва");
    assert_transform("camelcase", "GREEK", "έναςΔύο");
    assert_transform("pascalcase", "RUSSIAN", "ОдинДва");
    assert_transform("pascalcase", "GREEK", "ΈναςΔύο");
    assert_transform("upcase", "RUSSIAN", "ОДИНДВА");
    assert_transform("downcase", "RUSSIAN", "одиндва");
    assert_transform("kebabcase", "TURKISH", "istanbul-lı");
    assert_transform("pascalcase", "TURKISH", "IstanbulLı");
    assert_transform("upcase", "JAPANESE", "こんにちは");
    assert_transform("kebabcase", "JAPANESE", "こんにちは");
}

#[test]
fn snippet_variable_transformation_doesnt_work_if_regex_is_complicated_and_snippet_body_contains_55627() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    let snippet = parse("const fileName = \"${TM_FILENAME/(.*)\\..+$/$1/}\"");
    assert_eq!(snippet.to_textmate_string(), "const fileName = \"${TM_FILENAME/(.*)\\..+$/${1}/}\"");
}

#[test]
fn bug_html_attribute_suggestions_snippet_session_does_not_have_end_position_set_33147() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();

    let snippet = parse_with_final_tabstop("src=\"$1\"");
    let placeholders = snippet.placeholders();
    let (first, second) = (&placeholders[0], &placeholders[1]);

    assert_eq!(placeholders.len(), 2);
    assert_eq!(first.as_placeholder().unwrap().index, 1);
    assert_eq!(second.as_placeholder().unwrap().index, 0);
}

#[test]
fn snippet_optional_transforms_are_not_applied_correctly_when_reusing_the_same_variable_37702() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();

    let transform = Marker::new_transform();
    transform.append_child(Marker::new_format_string(1, Some("upcase"), None, None));
    transform.append_child(Marker::new_format_string(2, Some("upcase"), None, None));
    *transform.as_transform().unwrap().regexp.borrow_mut() = JsRegExp::from_str("^(.)|-(.)", "g").unwrap();

    assert_eq!(transform.resolve_transform(&u("my-file-name")), "MyFileName");

    let clone = transform.deep_clone();
    assert_eq!(clone.resolve_transform(&u("my-file-name")), "MyFileName");
}

#[test]
fn problem_with_snippets_regex_40570() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();

    let snippet = parse("${TM_DIRECTORY/.*src[\\/](.*)/$1/}");
    assert_markers(snippet.children(), &[Variable]);
}

#[test]
fn variable_transformation_doesnt_work_if_undefined_variables_are_used_in_the_same_snippet_51769() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    let transform = Marker::new_transform();
    transform.append_child(Marker::new_text("bar"));
    *transform.as_transform().unwrap().regexp.borrow_mut() = JsRegExp::from_str("foo", "gi").unwrap();
    assert_eq!(transform.to_textmate_string(), "/foo/bar/ig");
}

#[test]
fn transform_serialization_joins_children_without_comma() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    let transform_with_format_string = Marker::new_transform();
    transform_with_format_string.append_child(Marker::new_format_string(1, Some("upcase"), None, None));
    transform_with_format_string.append_child(Marker::new_text("_"));
    *transform_with_format_string.as_transform().unwrap().regexp.borrow_mut() = JsRegExp::from_str("foo", "g").unwrap();
    let serialized = transform_with_format_string.to_textmate_string();
    assert_eq!(serialized, "/foo/${1:/upcase}_/g");

    let template = format!("${{TM_FILENAME{serialized}}}");
    let snippet = parse(&template);
    assert_eq!(snippet.to_textmate_string(), template.as_str());
}

#[test]
fn snippet_parser_freeze_53144() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    let snippet = parse("${1/(void$)|(.+)/${1:?-\treturn nil;}/}");
    assert_markers(snippet.children(), &[Placeholder]);
}

#[test]
fn snippets_variable_not_resolved_in_json_proposal_52931() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    assert_text_and_marker("FOO${1:/bin/bash}", "FOO/bin/bash", &[Text, Placeholder]);
}

#[test]
fn mirroring_sequence_of_nested_placeholders_not_selected_properly_on_backjumping_58736() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    let snippet = parse("${3:nest1 ${1:nest2 ${2:nest3}}} $3");
    let children = snippet.children();
    assert_eq!(children.len(), 3);
    assert!(children[0].as_placeholder().is_some());
    assert!(children[1].as_text().is_some());
    assert!(children[2].as_placeholder().is_some());

    fn assert_parent(marker: &MarkerRef, snippet: &MarkerRef) {
        for child in marker.children() {
            assert_parent(&child, snippet);
        }
        if marker.as_placeholder().is_none() {
            return;
        }
        let mut found = false;
        let mut m = Some(marker.clone());
        while let Some(current) = m.take() {
            if current.parent().is_some_and(|parent| Rc::ptr_eq(&parent, snippet)) {
                found = true;
                break;
            }
            m = current.parent();
        }
        assert!(found);
    }
    let clone = &children[2];
    assert_parent(clone, &snippet);
}

#[test]
fn backspace_cant_be_escaped_in_snippet_variable_transforms_65412() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();

    let snippet = parse("namespace ${TM_DIRECTORY/[\\/]/\\\\/g};");
    assert_markers(snippet.children(), &[Text, Variable, Text]);
}

#[test]
fn snippet_cannot_escape_closing_bracket_inside_conditional_insertion_variable_replacement_78883() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();

    let snippet = parse("${TM_DIRECTORY/(.+)/${1:+import { hello \\} from world}/}");
    let children = snippet.children();
    let variable = &children[0];
    assert_eq!(children.len(), 1);
    assert!(variable.as_variable().is_some());
    let transform = variable.transform().expect("a variável tem transform");
    let transform_children = transform.children();
    assert_eq!(transform_children.len(), 1);
    assert!(transform_children[0].as_format_string().is_some());
    assert_eq!(transform_children[0].as_format_string().unwrap().if_value, Some(js("import { hello } from world")));
    assert_eq!(transform_children[0].as_format_string().unwrap().else_value, None);
}

#[test]
fn snippet_escape_backslashes_inside_conditional_insertion_variable_replacement_80394() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();

    let snippet = parse("${CURRENT_YEAR/(.+)/${1:+\\\\}/}");
    let children = snippet.children();
    let variable = &children[0];
    assert_eq!(children.len(), 1);
    assert!(variable.as_variable().is_some());
    let transform = variable.transform().expect("a variável tem transform");
    let transform_children = transform.children();
    assert_eq!(transform_children.len(), 1);
    assert!(transform_children[0].as_format_string().is_some());
    assert_eq!(transform_children[0].as_format_string().unwrap().if_value, Some(js("\\")));
    assert_eq!(transform_children[0].as_format_string().unwrap().else_value, None);
}

#[test]
fn snippet_placeholder_empty_right_after_expansion_152553() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();

    let snippet = parse("${1:prog}: ${2:$1.cc} - $2");
    let actual = snippet.to_js_string();
    assert_eq!(actual, "prog: prog.cc - prog.cc");

    let snippet2 = parse("${1:prog}: ${3:${2:$1.cc}.33} - $2 $3");
    let actual2 = snippet2.to_js_string();
    assert_eq!(actual2, "prog: prog.cc.33 - prog.cc prog.cc.33");

    // cyclic references of placeholders
    let snippet3 = parse("${1:$2.one} <> ${2:$1.two}");
    let actual3 = snippet3.to_js_string();
    assert_eq!(actual3, ".two.one.two.one <> .one.two.one.two");
}

#[test]
fn snippet_choices_are_incorrectly_escaped_applied_180132() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    assert_text_and_marker("${1|aaa$aaa|}bbb\\$bbb", "aaa$aaabbb$bbb", &[Placeholder, Text]);
}
