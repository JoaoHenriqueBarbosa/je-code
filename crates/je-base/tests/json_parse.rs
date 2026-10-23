//! Porte de `src/vs/base/test/common/jsonParse.test.ts` (suíte `JSON Parse`), caso a caso e na mesma ordem.
//!
//! Notas de tradução:
//! - `JSON.parse(expected)` é `jsonc::json_parse` (o parser estrito do módulo) e `assert.deepEqual` é a
//!   igualdade de `json::Value`.
//! - O `ensureNoDisposablesAreLeakedInTestSuite` entra em cada teste pela guarda de `common`.

mod common;

use common::ensure_no_disposables_are_leaked_in_test_suite;
use je_base::json::{Object, Value};
use je_base::jsonc::{json_parse, parse, strip_comments};

fn utf16(text: &str) -> Vec<u16> {
    text.encode_utf16().collect()
}

fn parse_jsonc(content: &str) -> Value {
    parse(&utf16(content)).expect("o jsonc deveria ser válido")
}

fn json_parse_text(content: &str) -> Value {
    json_parse(&utf16(content)).expect("o json deveria ser válido")
}

fn strip(content: &str) -> String {
    strip_comments(&utf16(content)).to_string_lossy()
}

#[test]
fn line_comment() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    let content = ["{", "  \"prop\": 10 // a comment", "}"].join("\n");
    let expected = ["{", "  \"prop\": 10 ", "}"].join("\n");
    assert_eq!(parse_jsonc(&content), json_parse_text(&expected));
}

#[test]
fn line_comment_eof() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    let content = ["{", "}", "// a comment"].join("\n");
    let expected = ["{", "}", ""].join("\n");
    assert_eq!(parse_jsonc(&content), json_parse_text(&expected));
}

#[test]
fn line_comment_crlf() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    let content = ["{", "  \"prop\": 10 // a comment", "}"].join("\r\n");
    let expected = ["{", "  \"prop\": 10 ", "}"].join("\r\n");
    assert_eq!(parse_jsonc(&content), json_parse_text(&expected));
}

#[test]
fn line_comment_eof_crlf() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    let content = ["{", "}", "// a comment"].join("\r\n");
    let expected = ["{", "}", ""].join("\r\n");
    assert_eq!(parse_jsonc(&content), json_parse_text(&expected));
}

#[test]
fn block_comment_single_line() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    let content = ["{", "  /* before */\"prop\": 10/* after */", "}"].join("\n");
    let expected = ["{", "  \"prop\": 10", "}"].join("\n");
    assert_eq!(parse_jsonc(&content), json_parse_text(&expected));
}

#[test]
fn block_comment_multi_line() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    let content = ["{", "  /**", "   * Some comment", "   */", "  \"prop\": 10", "}"].join("\n");
    let expected = ["{", "  ", "  \"prop\": 10", "}"].join("\n");
    assert_eq!(parse_jsonc(&content), json_parse_text(&expected));
}

#[test]
fn block_comment_shortest_match() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    let content = "/* abc */ */";
    let expected = " */";
    assert_eq!(strip(content), expected);
}

#[test]
fn no_strings_double_quote() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    let content = ["{", "  \"/* */\": 10", "}"].join("\n");
    let expected = ["{", "  \"/* */\": 10", "}"].join("\n");
    assert_eq!(parse_jsonc(&content), json_parse_text(&expected));
}

#[test]
fn no_strings_single_quote() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    let content = ["{", "  '/* */': 10", "}"].join("\n");
    let expected = ["{", "  '/* */': 10", "}"].join("\n");
    assert_eq!(strip(&content), expected);
}

#[test]
fn trailing_comma_in_object() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    let content = ["{", "  \"a\": 10,", "}"].join("\n");
    let expected = ["{", "  \"a\": 10", "}"].join("\n");
    assert_eq!(parse_jsonc(&content), json_parse_text(&expected));
}

#[test]
fn trailing_comma_in_array() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    let content = ["[ \"a\", \"b\", \"c\", ]"].join("\n");
    let expected = ["[ \"a\", \"b\", \"c\" ]"].join("\n");
    assert_eq!(parse_jsonc(&content), json_parse_text(&expected));
}

#[test]
fn trailing_comma() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    let content = ["{", "  \"propA\": 10, // a comment", "  \"propB\": false, // a trailing comma", "}"].join("\n");
    let expected = ["{", "  \"propA\": 10,", "  \"propB\": false", "}"].join("\n");
    assert_eq!(parse_jsonc(&content), json_parse_text(&expected));
}

#[test]
fn trailing_comma_eof() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    let content = "
// This configuration file allows you to pass permanent command line arguments to VS Code.
// Only a subset of arguments is currently supported to reduce the likelihood of breaking
// the installation.
//
// PLEASE DO NOT CHANGE WITHOUT UNDERSTANDING THE IMPACT
//
// NOTE: Changing this file requires a restart of VS Code.
{
\t// Use software rendering instead of hardware accelerated rendering.
\t// This can help in cases where you see rendering issues in VS Code.
\t// \"disable-hardware-acceleration\": true,
\t// Allows to disable crash reporting.
\t// Should restart the app if the value is changed.
\t\"enable-crash-reporter\": true,
\t// Unique id used for correlating crash reports sent from this instance.
\t// Do not edit this value.
\t\"crash-reporter-id\": \"aaaaab31-7453-4506-97d0-93411b2c21c7\",
\t\"locale\": \"en\",
\t// \"log-level\": \"trace\"
}
";
    let mut expected = Object::new();
    expected.insert("enable-crash-reporter".into(), Value::Bool(true));
    expected.insert("crash-reporter-id".into(), Value::String("aaaaab31-7453-4506-97d0-93411b2c21c7".into()));
    expected.insert("locale".into(), Value::String("en".into()));
    assert_eq!(parse_jsonc(content), Value::Object(expected));
}
