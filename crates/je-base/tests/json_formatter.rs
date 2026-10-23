//! Porte de `src/vs/base/test/common/jsonFormatter.test.ts` (suíte `JSON - formatter`), caso a caso e na mesma ordem.

use je_base::JsString;
use je_base::json::Value;
use je_base::json_formatter::{self as formatter, FormattingOptions, Range};
use je_base::json_edit::apply_edits;

fn utf16(text: &str) -> Vec<u16> {
    text.encode_utf16().collect()
}

fn format(content: &str, expected: &str, insert_spaces: bool) {
    let (content, range) = match (content.find('|'), content.rfind('|')) {
        (Some(start), Some(end)) => {
            let before = &content[..start];
            let middle = &content[start + 1..end];
            let after = &content[end + 1..];
            let range = Range { offset: before.encode_utf16().count(), length: middle.encode_utf16().count() };
            (format!("{before}{middle}{after}"), Some(range))
        }
        _ => (content.to_string(), None),
    };
    let units = utf16(&content);
    let options = FormattingOptions {
        tab_size: Some(2),
        insert_spaces: Some(insert_spaces),
        eol: Some(JsString::from("\n")),
    };
    let edits = formatter::format(&units, range, &options);
    let mut last_edit_offset = units.len();
    for edit in edits.iter().rev() {
        assert!(edit.offset + edit.length <= units.len());
        assert!(last_edit_offset >= edit.offset + edit.length, "as edições devem estar ordenadas");
        last_edit_offset = edit.offset;
    }
    let actual = apply_edits(&units, &edits).expect("sem sobreposição");
    assert_eq!(String::from_utf16(&actual).unwrap(), expected);
}

#[test]
fn object_single_property() {
    format(
        "{\"x\" : 1}",
        "{\n  \"x\": 1\n}",
        true,
    );
}

#[test]
fn object_multiple_properties() {
    format(
        "{\"x\" : 1,  \"y\" : \"foo\", \"z\"  : true}",
        "{\n  \"x\": 1,\n  \"y\": \"foo\",\n  \"z\": true\n}",
        true,
    );
}

#[test]
fn object_no_properties() {
    format(
        "{\"x\" : {    },  \"y\" : {}}",
        "{\n  \"x\": {},\n  \"y\": {}\n}",
        true,
    );
}

#[test]
fn object_nesting() {
    format(
        "{\"x\" : {  \"y\" : { \"z\"  : { }}, \"a\": true}}",
        "{\n  \"x\": {\n    \"y\": {\n      \"z\": {}\n    },\n    \"a\": true\n  }\n}",
        true,
    );
}

#[test]
fn array_single_items() {
    format(
        "[\"[]\"]",
        "[\n  \"[]\"\n]",
        true,
    );
}

#[test]
fn array_multiple_items() {
    format(
        "[true,null,1.2]",
        "[\n  true,\n  null,\n  1.2\n]",
        true,
    );
}

#[test]
fn array_no_items() {
    format(
        "[      ]",
        "[]",
        true,
    );
}

#[test]
fn array_nesting() {
    format(
        "[ [], [ [ {} ], \"a\" ]  ]",
        "[\n  [],\n  [\n    [\n      {}\n    ],\n    \"a\"\n  ]\n]",
        true,
    );
}

#[test]
fn syntax_errors() {
    format(
        "[ null 1.2 ]",
        "[\n  null 1.2\n]",
        true,
    );
}

#[test]
fn empty_lines() {
    format(
        "{\n\"a\": true,\n\n\"b\": true\n}",
        "{\n\t\"a\": true,\n\t\"b\": true\n}",
        false,
    );
}

#[test]
fn single_line_comment() {
    format(
        "[ \n//comment\n\"foo\", \"bar\"\n] ",
        "[\n  //comment\n  \"foo\",\n  \"bar\"\n]",
        true,
    );
}

#[test]
fn block_line_comment() {
    format(
        "[{\n        /*comment*/     \n\"foo\" : true\n}] ",
        "[\n  {\n    /*comment*/\n    \"foo\": true\n  }\n]",
        true,
    );
}

#[test]
fn single_line_comment_on_same_line() {
    format(
        " {  \n        \"a\": {}// comment    \n } ",
        "{\n  \"a\": {} // comment    \n}",
        true,
    );
}

#[test]
fn single_line_comment_on_same_line_2() {
    format(
        "{ //comment\n}",
        "{ //comment\n}",
        true,
    );
}

#[test]
fn block_comment_on_same_line() {
    format(
        "{      \"a\": {}, /*comment*/    \n        /*comment*/ \"b\": {},    \n        \"c\": {/*comment*/}    } ",
        "{\n  \"a\": {}, /*comment*/\n  /*comment*/ \"b\": {},\n  \"c\": { /*comment*/}\n}",
        true,
    );
}

#[test]
fn block_comment_on_same_line_advanced() {
    format(
        " {       \"d\": [\n             null\n        ] /*comment*/\n        ,\"e\": /*comment*/ [null] }",
        "{\n  \"d\": [\n    null\n  ] /*comment*/,\n  \"e\": /*comment*/ [\n    null\n  ]\n}",
        true,
    );
}

#[test]
fn multiple_block_comments_on_same_line() {
    format(
        "{      \"a\": {} /*comment*/, /*comment*/   \n        /*comment*/ \"b\": {}  /*comment*/  } ",
        "{\n  \"a\": {} /*comment*/, /*comment*/\n  /*comment*/ \"b\": {} /*comment*/\n}",
        true,
    );
}

#[test]
fn multiple_mixed_comments_on_same_line() {
    format(
        "[ /*comment*/  /*comment*/   // comment \n]",
        "[ /*comment*/ /*comment*/ // comment \n]",
        true,
    );
}

#[test]
fn range() {
    format(
        "{ \"a\": {},\n|\"b\": [null, null]|\n} ",
        "{ \"a\": {},\n\"b\": [\n  null,\n  null\n]\n} ",
        true,
    );
}

#[test]
fn range_with_existing_indent() {
    format(
        "{ \"a\": {},\n   |\"b\": [null],\n\"c\": {}\n}|",
        "{ \"a\": {},\n   \"b\": [\n    null\n  ],\n  \"c\": {}\n}",
        true,
    );
}

#[test]
fn range_with_existing_indent_tabs() {
    format(
        "{ \"a\": {},\n|  \"b\": [null],   \n\"c\": {}\n} |    ",
        "{ \"a\": {},\n\t\"b\": [\n\t\tnull\n\t],\n\t\"c\": {}\n}",
        false,
    );
}

#[test]
fn block_comment_none_line_breaking_symbols() {
    format(
        "{ \"a\": [ 1\n/* comment */\n, 2\n/* comment */\n]\n/* comment */\n,\n \"b\": true\n/* comment */\n}",
        "{\n  \"a\": [\n    1\n    /* comment */\n    ,\n    2\n    /* comment */\n  ]\n  /* comment */\n  ,\n  \"b\": true\n  /* comment */\n}",
        true,
    );
}

#[test]
fn line_comment_after_none_line_breaking_symbols() {
    format(
        "{ \"a\":\n// comment\nnull,\n \"b\"\n// comment\n: null\n// comment\n}",
        "{\n  \"a\":\n  // comment\n  null,\n  \"b\"\n  // comment\n  : null\n  // comment\n}",
        true,
    );
}

#[test]
fn to_formatted_string() {
    let mut inner = je_base::json::Object::new();
    inner.insert(JsString::from("b"), Value::Number(1.0));
    inner.insert(JsString::from("d"), Value::Array(vec![Value::String(JsString::from("hello"))]));
    let mut obj = je_base::json::Object::new();
    obj.insert(JsString::from("a"), Value::Object(inner));
    let obj = Value::Object(obj);

    let get_expected = |tab: &str, eol: &str| {
        [
            "{".to_string(),
            format!("{tab}\"a\": {{"),
            format!("{tab}{tab}\"b\": 1,"),
            format!("{tab}{tab}\"d\": ["),
            format!("{tab}{tab}{tab}\"hello\""),
            format!("{tab}{tab}]"),
            format!("{tab}}}"),
            "}".to_string(),
        ]
        .join(eol)
    };

    let options = |insert_spaces: bool, tab_size: Option<usize>, eol: &str| FormattingOptions {
        tab_size,
        insert_spaces: Some(insert_spaces),
        eol: Some(JsString::from(eol)),
    };
    let actual = formatter::to_formatted_string(&obj, &options(true, Some(2), "\n"));
    assert_eq!(actual.to_string_lossy(), get_expected("  ", "\n"));

    let actual = formatter::to_formatted_string(&obj, &options(true, Some(2), "\r\n"));
    assert_eq!(actual.to_string_lossy(), get_expected("  ", "\r\n"));

    let actual = formatter::to_formatted_string(&obj, &options(false, None, "\r\n"));
    assert_eq!(actual.to_string_lossy(), get_expected("\t", "\r\n"));
}
