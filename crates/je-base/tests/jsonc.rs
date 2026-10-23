//! Testes próprios de `src/vs/base/common/jsonc.ts` (o VS Code não tem `jsonc.test.ts`): comentários,
//! vírgulas finais e o `JSON.parse` estrito que o módulo usa por baixo.

use je_base::JsString;
use je_base::json::Value;
use je_base::jsonc::{json_parse, parse, strip_comments};

fn utf16(text: &str) -> Vec<u16> {
    text.encode_utf16().collect()
}

fn strip(text: &str) -> String {
    strip_comments(&utf16(text)).to_string_lossy()
}

#[test]
fn strip_comments_removes_line_and_block_comments() {
    assert_eq!(strip("{ /* a */ \"x\": 1 // fim\n}"), "{  \"x\": 1 \n}");
    assert_eq!(strip("// só comentário"), "");
    assert_eq!(strip("1 //x\r\n2"), "1 \r\n2");
    assert_eq!(strip("/* a\nb */1"), "1");
}

#[test]
fn strip_comments_keeps_strings_untouched() {
    assert_eq!(strip(r#"{"url": "http://x/*y*/"}"#), r#"{"url": "http://x/*y*/"}"#);
    assert_eq!(strip(r#"'// nao'"#), r#"'// nao'"#);
}

#[test]
fn strip_comments_removes_trailing_commas() {
    assert_eq!(strip("[1, 2, ]"), "[1, 2 ]");
    assert_eq!(strip("{\"a\": 1,\n}"), "{\"a\": 1\n}");
}

#[test]
fn parse_accepts_comments_and_trailing_commas() {
    let value = parse(&utf16("{\n // c\n \"a\": [1, 2,], /* x */\n \"b\": null,\n}")).unwrap();
    let Value::Object(object) = value else { panic!("objeto esperado") };
    assert_eq!(object.get("a"), Some(&Value::Array(vec![Value::Number(1.0), Value::Number(2.0)])));
    assert_eq!(object.get("b"), Some(&Value::Null));
}

#[test]
fn parse_fails_on_invalid_json() {
    assert!(parse(&utf16("{")).is_err());
    assert!(parse(&utf16("{'a': 1}")).is_err());
}

#[test]
fn json_parse_is_strict() {
    for bad in ["01", "1.", ".5", "+1", "[1,]", "{\"a\":1,}", "\"\t\"", "\"\\x\"", "tru", "[] x", "", "'a'", "{a:1}"] {
        assert!(json_parse(&utf16(bad)).is_err(), "deveria rejeitar {bad:?}");
    }
    for (good, expected) in [
        ("-0", Value::Number(-0.0)),
        ("1e3", Value::Number(1000.0)),
        ("  true ", Value::Bool(true)),
        ("\"a\\u0041\\n\"", Value::String(JsString::from("aA\n"))),
        ("-1.5E-2", Value::Number(-0.015)),
    ] {
        assert_eq!(json_parse(&utf16(good)).unwrap(), expected, "texto {good:?}");
    }
}
