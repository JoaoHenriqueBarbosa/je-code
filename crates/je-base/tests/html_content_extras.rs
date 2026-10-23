//! Casos próprios do porte de `htmlContent.ts`: o original só testa o `MarkdownString` (ver
//! `markdown_string.rs`); aqui entram as funções de escape e de link, com os valores esperados tirados
//! da execução das mesmas expressões no V8 (Node).

use je_base::dynamic::Dynamic;
use je_base::html_content::{
    HrefAndDimensions, IMarkdownString, IsTrusted, MarkdownCommand, OneOrManyMarkdown, append_escaped_markdown_inline_code, create_command_uri,
    create_markdown_command_link, create_markdown_link, escape_double_quotes, escape_markdown_link_label, escape_markdown_syntax_tokens,
    is_empty_markdown_string, is_markdown_string, is_portable_link_target, is_portable_markdown_target, markdown_string_equal, parse_href_and_dimensions,
    remove_markdown_escapes,
};
use je_base::js_string::JsString;
use je_base::path::{FixedEnvironment, Platform};
use je_base::resources::ExtUri;
use je_base::uri::URI;
use std::rc::Rc;

fn u(text: &str) -> Vec<u16> {
    text.encode_utf16().collect()
}

#[test]
fn escape_markdown_syntax_tokens_case() {
    assert_eq!(escape_markdown_syntax_tokens(&u("a*b_c\n- d")), "a\\*b\\_c\n\\- d");
    assert_eq!(escape_markdown_syntax_tokens(&u("  - x\n\t-y")), "  \\- x\n\t\\-y");
    assert_eq!(escape_markdown_syntax_tokens(&u("p+q!~#(){}[]\\`")), "p\\+q\\!\\~\\#\\(\\)\\{\\}\\[\\]\\\\\\`");
}

#[test]
fn remove_markdown_escapes_case() {
    assert_eq!(remove_markdown_escapes(&u("\\*a\\\\b\\x")), "*a\\b\\x");
    assert_eq!(remove_markdown_escapes(&u("a\\.b\\-c\\ d")), "a.b-c\\ d");
    assert_eq!(remove_markdown_escapes(&u("")), "");
}

#[test]
fn inline_code_picks_a_fence_longer_than_any_backtick_run() {
    assert_eq!(append_escaped_markdown_inline_code(&u("a`b")), "``a`b``");
    assert_eq!(append_escaped_markdown_inline_code(&u("`a")), "`` `a ``");
    assert_eq!(append_escaped_markdown_inline_code(&u("a``b`c")), "```a``b`c```");
    assert_eq!(append_escaped_markdown_inline_code(&u("plain")), "`plain`");
    assert_eq!(append_escaped_markdown_inline_code(&u("")), "``");
}

#[test]
fn small_escapes() {
    assert_eq!(escape_double_quotes(&u("say \"hi\"")), "say &quot;hi&quot;");
    assert_eq!(escape_markdown_link_label(&u("a]b\\c-d")), "a\\]b\\\\c-d");
}

#[test]
fn parse_href_and_dimensions_reads_only_the_second_part() {
    let dims = |href: &str| parse_href_and_dimensions(&u(href));
    let result = |href: &str, dimensions: &[&str]| HrefAndDimensions { href: JsString::from(href), dimensions: dimensions.iter().map(ToString::to_string).collect() };
    assert_eq!(dims("img.png|width=10|height=5"), result("img.png", &["width=\"10\""]));
    assert_eq!(dims("img.png|height=5"), result("img.png", &["height=\"5\""]));
    assert_eq!(dims("img.png | width=3 height=4"), result("img.png", &["width=\"3\"", "height=\"4\""]));
    assert_eq!(dims("img.png"), result("img.png", &[]));
    assert_eq!(dims("a|b|c"), result("a", &[]));
}

#[test]
fn portable_targets() {
    for (target, expected) in [
        ("https://example.com", true),
        (" HTTPS://x ", true),
        ("http://_vscodecontentref_/0", false),
        ("file:///x", false),
        ("mailto:a@b", true),
        ("command:foo", false),
        ("ftp://x", false),
    ] {
        assert_eq!(is_portable_link_target(&u(target)), expected, "{target}");
    }
    for (target, expected) in [("#section", true), ("C:\\x", false), ("/abs", false), ("relative/path.md", true), (" ", false), ("x:y", false), ("\\\\server", false)] {
        assert_eq!(is_portable_markdown_target(&u(target)), expected, "{target}");
    }
}

#[test]
fn links_and_command_uris() {
    assert_eq!(create_markdown_link(&u("a_b"), &u("http://x"), Some(&u("t(1)")), true), "[a\\_b](http://x \"t\\(1\\)\")");
    assert_eq!(create_markdown_link(&u("a_b"), &u("http://x"), None, false), "[a_b](http://x)");

    let args = [Dynamic::Number(1.0), Dynamic::string("x"), Dynamic::object([("a", Dynamic::array([Dynamic::Bool(true), Dynamic::Null]))])];
    let uri = create_command_uri(&u("my.command"), &args);
    assert_eq!(uri.to_string(false), "command:my.command?%255B1%252C%2522x%2522%252C%257B%2522a%2522%253A%255Btrue%252Cnull%255D%257D%255D");
    assert_eq!(create_command_uri(&u("my.command"), &[]).to_string(false), "command:my.command");

    let command = MarkdownCommand { text: &u("Run"), id: &u("my.command"), arguments: None, tooltip: &u("Go") };
    assert_eq!(create_markdown_command_link(&command, true), "[Run](command:my.command \"Go\")");
}

#[test]
fn emptiness_equality_and_shape() {
    let markdown = |value: &str| IMarkdownString { value: JsString::from(value), ..IMarkdownString::default() };
    assert!(is_empty_markdown_string(None));
    assert!(is_empty_markdown_string(Some(OneOrManyMarkdown::One(&markdown("")))));
    assert!(!is_empty_markdown_string(Some(OneOrManyMarkdown::One(&markdown("x")))));
    assert!(is_empty_markdown_string(Some(OneOrManyMarkdown::Many(&[]))));
    assert!(!is_empty_markdown_string(Some(OneOrManyMarkdown::Many(&[markdown(""), markdown("x")]))));

    let ext_uri = ExtUri::unbiased(Rc::new(FixedEnvironment::new(Platform::Posix, "/")));
    let (mut a, mut b) = (markdown("x"), markdown("x"));
    assert!(markdown_string_equal(&a, &b, &ext_uri));
    a.base_uri = Some(URI::parse("file:///a", false));
    assert!(!markdown_string_equal(&a, &b, &ext_uri));
    b.base_uri = Some(URI::parse("file:///a", false));
    assert!(markdown_string_equal(&a, &b, &ext_uri));
    b.is_trusted = Some(IsTrusted::Bool(true));
    assert!(!markdown_string_equal(&a, &b, &ext_uri));

    let object = |entries: Vec<(&str, Dynamic)>| Dynamic::object(entries);
    assert!(is_markdown_string(&object(vec![("value", Dynamic::string("x"))])));
    assert!(is_markdown_string(&object(vec![("value", Dynamic::string("x")), ("isTrusted", object(vec![])), ("supportThemeIcons", Dynamic::Bool(true))])));
    assert!(!is_markdown_string(&object(vec![("value", Dynamic::Number(1.0))])));
    assert!(!is_markdown_string(&object(vec![("value", Dynamic::string("x")), ("supportThemeIcons", Dynamic::string("yes"))])));
    assert!(!is_markdown_string(&Dynamic::Null));
}
