//! Porte de `src/vs/base/test/common/jsonEdit.test.ts` (suíte `JSON - edits`), caso a caso e na mesma ordem.

use je_base::JsString;
use je_base::json::{Object, Value};
use je_base::json_edit::{EditSegment, GetInsertionIndex, remove_property as remove_edits, set_property as set_edits};
use je_base::json_formatter::{Edit, FormattingOptions};

fn utf16(text: &str) -> Vec<u16> {
    text.encode_utf16().collect()
}

fn object(entries: &[(&str, Value)]) -> Value {
    let mut object = Object::new();
    for (key, value) in entries {
        object.insert(JsString::from(*key), value.clone());
    }
    Value::Object(object)
}

fn formatter_options() -> FormattingOptions {
    FormattingOptions { insert_spaces: Some(true), tab_size: Some(2), eol: Some(JsString::from("\n")) }
}

fn assert_edit(content: &str, edits: &[Edit], expected: &str) {
    let mut content = utf16(content);
    let mut last_edit_offset = content.len();
    for edit in edits.iter().rev() {
        assert!(edit.offset + edit.length <= content.len());
        assert!(last_edit_offset >= edit.offset + edit.length, "as edições devem estar ordenadas");
        last_edit_offset = edit.offset;
        content.splice(edit.offset..edit.offset + edit.length, edit.content.units().iter().copied());
    }
    assert_eq!(String::from_utf16(&content).unwrap(), expected);
}

fn set(content: &str, path: &[EditSegment], value: Option<Value>, get_insertion_index: Option<GetInsertionIndex<'_>>) -> Vec<Edit> {
    set_edits(&utf16(content), path, value, &formatter_options(), get_insertion_index).unwrap()
}

fn remove(content: &str, path: &[EditSegment]) -> Vec<Edit> {
    remove_edits(&utf16(content), path, &formatter_options()).unwrap()
}

#[test]
fn set_property() {
    assert_edit(
        "{\n  \"x\": \"y\"\n}",
        &set(
        "{\n  \"x\": \"y\"\n}",
        &[EditSegment::from("x")],
        Some(Value::String(JsString::from("bar"))),
        None,
    ),
        "{\n  \"x\": \"bar\"\n}",
    );
    assert_edit(
        "true",
        &set(
        "true",
        &[],
        Some(Value::String(JsString::from("bar"))),
        None,
    ),
        "\"bar\"",
    );
    assert_edit(
        "{\n  \"x\": \"y\"\n}",
        &set(
        "{\n  \"x\": \"y\"\n}",
        &[EditSegment::from("x")],
        Some(object(&[("key", Value::Bool(true))])),
        None,
    ),
        "{\n  \"x\": {\n    \"key\": true\n  }\n}",
    );
    assert_edit(
        "{\n  \"a\": \"b\",  \"x\": \"y\"\n}",
        &set(
        "{\n  \"a\": \"b\",  \"x\": \"y\"\n}",
        &[EditSegment::from("a")],
        Some(Value::Null),
        None,
    ),
        "{\n  \"a\": null,  \"x\": \"y\"\n}",
    );
}

#[test]
fn insert_property() {
    assert_edit(
        "{}",
        &set(
        "{}",
        &[EditSegment::from("foo")],
        Some(Value::String(JsString::from("bar"))),
        None,
    ),
        "{\n  \"foo\": \"bar\"\n}",
    );
    assert_edit(
        "{}",
        &set(
        "{}",
        &[EditSegment::from("foo"), EditSegment::from("foo2")],
        Some(Value::String(JsString::from("bar"))),
        None,
    ),
        "{\n  \"foo\": {\n    \"foo2\": \"bar\"\n  }\n}",
    );
    assert_edit(
        "{\n}",
        &set(
        "{\n}",
        &[EditSegment::from("foo")],
        Some(Value::String(JsString::from("bar"))),
        None,
    ),
        "{\n  \"foo\": \"bar\"\n}",
    );
    assert_edit(
        "  {\n  }",
        &set(
        "  {\n  }",
        &[EditSegment::from("foo")],
        Some(Value::String(JsString::from("bar"))),
        None,
    ),
        "  {\n    \"foo\": \"bar\"\n  }",
    );
    assert_edit(
        "{\n  \"x\": \"y\"\n}",
        &set(
        "{\n  \"x\": \"y\"\n}",
        &[EditSegment::from("foo")],
        Some(Value::String(JsString::from("bar"))),
        None,
    ),
        "{\n  \"x\": \"y\",\n  \"foo\": \"bar\"\n}",
    );
    assert_edit(
        "{\n  \"x\": \"y\"\n}",
        &set(
        "{\n  \"x\": \"y\"\n}",
        &[EditSegment::from("e")],
        Some(Value::String(JsString::from("null"))),
        None,
    ),
        "{\n  \"x\": \"y\",\n  \"e\": \"null\"\n}",
    );
    assert_edit(
        "{\n  \"x\": \"y\"\n}",
        &set(
        "{\n  \"x\": \"y\"\n}",
        &[EditSegment::from("x")],
        Some(Value::String(JsString::from("bar"))),
        None,
    ),
        "{\n  \"x\": \"bar\"\n}",
    );
    assert_edit(
        "{\n  \"x\": {\n    \"a\": 1,\n    \"b\": true\n  }\n}\n",
        &set(
        "{\n  \"x\": {\n    \"a\": 1,\n    \"b\": true\n  }\n}\n",
        &[EditSegment::from("x")],
        Some(Value::String(JsString::from("bar"))),
        None,
    ),
        "{\n  \"x\": \"bar\"\n}\n",
    );
    assert_edit(
        "{\n  \"x\": {\n    \"a\": 1,\n    \"b\": true\n  }\n}\n",
        &set(
        "{\n  \"x\": {\n    \"a\": 1,\n    \"b\": true\n  }\n}\n",
        &[EditSegment::from("x"), EditSegment::from("b")],
        Some(Value::String(JsString::from("bar"))),
        None,
    ),
        "{\n  \"x\": {\n    \"a\": 1,\n    \"b\": \"bar\"\n  }\n}\n",
    );
    assert_edit(
        "{\n  \"x\": {\n    \"a\": 1,\n    \"b\": true\n  }\n}\n",
        &set(
        "{\n  \"x\": {\n    \"a\": 1,\n    \"b\": true\n  }\n}\n",
        &[EditSegment::from("x"), EditSegment::from("c")],
        Some(Value::String(JsString::from("bar"))),
        Some(&|_| 0),
    ),
        "{\n  \"x\": {\n    \"c\": \"bar\",\n    \"a\": 1,\n    \"b\": true\n  }\n}\n",
    );
    assert_edit(
        "{\n  \"x\": {\n    \"a\": 1,\n    \"b\": true\n  }\n}\n",
        &set(
        "{\n  \"x\": {\n    \"a\": 1,\n    \"b\": true\n  }\n}\n",
        &[EditSegment::from("x"), EditSegment::from("c")],
        Some(Value::String(JsString::from("bar"))),
        Some(&|_| 1),
    ),
        "{\n  \"x\": {\n    \"a\": 1,\n    \"c\": \"bar\",\n    \"b\": true\n  }\n}\n",
    );
    assert_edit(
        "{\n  \"x\": {\n    \"a\": 1,\n    \"b\": true\n  }\n}\n",
        &set(
        "{\n  \"x\": {\n    \"a\": 1,\n    \"b\": true\n  }\n}\n",
        &[EditSegment::from("x"), EditSegment::from("c")],
        Some(Value::String(JsString::from("bar"))),
        Some(&|_| 2),
    ),
        "{\n  \"x\": {\n    \"a\": 1,\n    \"b\": true,\n    \"c\": \"bar\"\n  }\n}\n",
    );
    assert_edit(
        "{\n  \"x\": {\n    \"a\": 1,\n    \"b\": true\n  }\n}\n",
        &set(
        "{\n  \"x\": {\n    \"a\": 1,\n    \"b\": true\n  }\n}\n",
        &[EditSegment::from("c")],
        Some(Value::String(JsString::from("bar"))),
        None,
    ),
        "{\n  \"x\": {\n    \"a\": 1,\n    \"b\": true\n  },\n  \"c\": \"bar\"\n}\n",
    );
    assert_edit(
        "{\n  \"a\": [\n    {\n    } \n  ]  \n}",
        &set(
        "{\n  \"a\": [\n    {\n    } \n  ]  \n}",
        &[EditSegment::from("foo")],
        Some(Value::String(JsString::from("bar"))),
        None,
    ),
        "{\n  \"a\": [\n    {\n    } \n  ],\n  \"foo\": \"bar\"\n}",
    );
    assert_edit(
        "",
        &set(
        "",
        &[EditSegment::from("foo"), EditSegment::from(0_i64)],
        Some(Value::String(JsString::from("bar"))),
        None,
    ),
        "{\n  \"foo\": [\n    \"bar\"\n  ]\n}",
    );
    assert_edit(
        "//comment",
        &set(
        "//comment",
        &[EditSegment::from("foo"), EditSegment::from(0_i64)],
        Some(Value::String(JsString::from("bar"))),
        None,
    ),
        "{\n  \"foo\": [\n    \"bar\"\n  ]\n} //comment",
    );
}

#[test]
fn remove_property() {
    assert_edit(
        "{\n  \"x\": \"y\"\n}",
        &remove("{\n  \"x\": \"y\"\n}", &[EditSegment::from("x")]),
        "{\n}",
    );
    assert_edit(
        "{\n  \"x\": \"y\", \"a\": []\n}",
        &remove("{\n  \"x\": \"y\", \"a\": []\n}", &[EditSegment::from("x")]),
        "{\n  \"a\": []\n}",
    );
    assert_edit(
        "{\n  \"x\": \"y\", \"a\": []\n}",
        &remove("{\n  \"x\": \"y\", \"a\": []\n}", &[EditSegment::from("a")]),
        "{\n  \"x\": \"y\"\n}",
    );
}

#[test]
fn remove_only_property_if_ends_with_comma() {
    assert_edit(
        "{\n  \"x\": \"y\",\n}",
        &remove("{\n  \"x\": \"y\",\n}", &[EditSegment::from("x")]),
        "{\n}",
    );
    assert_edit(
        "{\n  \"x\": \"y\" /* x */, // trailing\n}",
        &remove("{\n  \"x\": \"y\" /* x */, // trailing\n}", &[EditSegment::from("x")]),
        "{ // trailing\n}",
    );
    assert_edit(
        "{\n  \"s\": {\n    \"x\": \"y\",\n  }\n}",
        &remove("{\n  \"s\": {\n    \"x\": \"y\",\n  }\n}", &[EditSegment::from("s"), EditSegment::from("x")]),
        "{\n  \"s\": {\n  }\n}",
    );
}

#[test]
fn insert_item_at_0() {
    assert_edit(
        "[\n  2,\n  3\n]",
        &set(
        "[\n  2,\n  3\n]",
        &[EditSegment::from(0_i64)],
        Some(Value::Number(1.0)),
        None,
    ),
        "[\n  1,\n  2,\n  3\n]",
    );
}

#[test]
fn insert_item_at_0_in_empty_array() {
    assert_edit(
        "[\n]",
        &set(
        "[\n]",
        &[EditSegment::from(0_i64)],
        Some(Value::Number(1.0)),
        None,
    ),
        "[\n  1\n]",
    );
}

#[test]
fn insert_item_at_an_index() {
    assert_edit(
        "[\n  1,\n  3\n]",
        &set(
        "[\n  1,\n  3\n]",
        &[EditSegment::from(1_i64)],
        Some(Value::Number(2.0)),
        None,
    ),
        "[\n  1,\n  2,\n  3\n]",
    );
}

#[test]
fn insert_item_at_an_index_im_empty_array() {
    assert_edit(
        "[\n]",
        &set(
        "[\n]",
        &[EditSegment::from(1_i64)],
        Some(Value::Number(1.0)),
        None,
    ),
        "[\n  1\n]",
    );
}

#[test]
fn insert_item_at_end_index() {
    assert_edit(
        "[\n  1,\n  2\n]",
        &set(
        "[\n  1,\n  2\n]",
        &[EditSegment::from(2_i64)],
        Some(Value::Number(3.0)),
        None,
    ),
        "[\n  1,\n  2,\n  3\n]",
    );
}

#[test]
fn insert_item_at_end_to_empty_array() {
    assert_edit(
        "[\n]",
        &set(
        "[\n]",
        &[EditSegment::from(-1_i64)],
        Some(Value::String(JsString::from("bar"))),
        None,
    ),
        "[\n  \"bar\"\n]",
    );
}

#[test]
fn insert_item_at_end() {
    assert_edit(
        "[\n  1,\n  2\n]",
        &set(
        "[\n  1,\n  2\n]",
        &[EditSegment::from(-1_i64)],
        Some(Value::String(JsString::from("bar"))),
        None,
    ),
        "[\n  1,\n  2,\n  \"bar\"\n]",
    );
}

#[test]
fn remove_item_in_array_with_one_item() {
    assert_edit(
        "[\n  1\n]",
        &set(
        "[\n  1\n]",
        &[EditSegment::from(0_i64)],
        None,
        None,
    ),
        "[]",
    );
}

#[test]
fn remove_item_in_the_middle_of_the_array() {
    assert_edit(
        "[\n  1,\n  2,\n  3\n]",
        &set(
        "[\n  1,\n  2,\n  3\n]",
        &[EditSegment::from(1_i64)],
        None,
        None,
    ),
        "[\n  1,\n  3\n]",
    );
}

#[test]
fn remove_last_item_in_the_array() {
    assert_edit(
        "[\n  1,\n  2,\n  \"bar\"\n]",
        &set(
        "[\n  1,\n  2,\n  \"bar\"\n]",
        &[EditSegment::from(2_i64)],
        None,
        None,
    ),
        "[\n  1,\n  2\n]",
    );
}

#[test]
fn remove_last_item_in_the_array_if_ends_with_comma() {
    assert_edit(
        "[\n  1,\n  \"foo\",\n  \"bar\",\n]",
        &set(
        "[\n  1,\n  \"foo\",\n  \"bar\",\n]",
        &[EditSegment::from(2_i64)],
        None,
        None,
    ),
        "[\n  1,\n  \"foo\"\n]",
    );
}

#[test]
fn remove_last_item_in_the_array_if_there_is_a_comment_in_the_beginning() {
    assert_edit(
        "// This is a comment\n[\n  1,\n  \"foo\",\n  \"bar\"\n]",
        &set(
        "// This is a comment\n[\n  1,\n  \"foo\",\n  \"bar\"\n]",
        &[EditSegment::from(2_i64)],
        None,
        None,
    ),
        "// This is a comment\n[\n  1,\n  \"foo\"\n]",
    );
}

#[test]
fn remove_last_item_in_compact_array_without_whitespace() {
    assert_edit(
        "[1,2]",
        &set(
        "[1,2]",
        &[EditSegment::from(1_i64)],
        None,
        None,
    ),
        "[\n  1\n]",
    );
}
