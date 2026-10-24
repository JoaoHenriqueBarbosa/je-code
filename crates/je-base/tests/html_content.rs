//! Porte de `src/vs/base/test/common/htmlContent.test.ts` (suíte `htmlContent`), caso a caso e na mesma
//! ordem. As três suítes aninhadas do original (`appendEscapedMarkdownInlineCode`,
//! `escapeMarkdownLinkLabel` e `escapeMarkdownSyntaxTokens`) viram prefixos dos nomes dos testes.
//!
//! Os casos próprios do porte (que o original não tem) ficam em `html_content_extras.rs`.
//!
//! Divergência de forma: o texto é `&[u16]`/`JsString`, então cada entrada passa por `u(...)`.

mod common;

use common::ensure_no_disposables_are_leaked_in_test_suite;
use je_base::html_content::{append_escaped_markdown_inline_code, escape_markdown_link_label, escape_markdown_syntax_tokens};

fn u(text: &str) -> Vec<u16> {
    text.encode_utf16().collect()
}

// suite('appendEscapedMarkdownInlineCode')

#[test]
fn append_escaped_markdown_inline_code_wraps_plain_text_in_single_backticks() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();

    assert_eq!(append_escaped_markdown_inline_code(&u("hello")), "`hello`");
    assert_eq!(append_escaped_markdown_inline_code(&u("")), "``");
    assert_eq!(append_escaped_markdown_inline_code(&u("foo bar")), "`foo bar`");
}

#[test]
fn append_escaped_markdown_inline_code_chooses_a_fence_longer_than_any_backtick_run_in_the_content() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();

    assert_eq!(append_escaped_markdown_inline_code(&u("a`b")), "``a`b``");
    assert_eq!(append_escaped_markdown_inline_code(&u("a``b")), "```a``b```");
    assert_eq!(append_escaped_markdown_inline_code(&u("a```b```c")), "````a```b```c````");
}

#[test]
fn append_escaped_markdown_inline_code_pads_with_spaces_when_the_content_begins_or_ends_with_a_backtick() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();

    assert_eq!(append_escaped_markdown_inline_code(&u("`")), "`` ` ``");
    assert_eq!(append_escaped_markdown_inline_code(&u("`hello")), "`` `hello ``");
    assert_eq!(append_escaped_markdown_inline_code(&u("hello`")), "`` hello` ``");
    assert_eq!(append_escaped_markdown_inline_code(&u("`a`b`")), "`` `a`b` ``");
}

#[test]
fn append_escaped_markdown_inline_code_does_not_pad_when_backticks_are_only_in_the_interior() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();

    assert_eq!(append_escaped_markdown_inline_code(&u("a`b")), "``a`b``");
}

#[test]
fn append_escaped_markdown_inline_code_handles_content_composed_entirely_of_backticks() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();

    assert_eq!(append_escaped_markdown_inline_code(&u("``")), "``` `` ```");
}

// suite('escapeMarkdownLinkLabel')

#[test]
fn escape_markdown_link_label_passes_plain_text_through_unchanged() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();

    assert_eq!(escape_markdown_link_label(&u("hello")), "hello");
    assert_eq!(escape_markdown_link_label(&u("")), "");
    assert_eq!(escape_markdown_link_label(&u("heap-snapshot-analysis")), "heap-snapshot-analysis");
    assert_eq!(escape_markdown_link_label(&u("foo.bar_baz")), "foo.bar_baz");
}

#[test]
fn escape_markdown_link_label_escapes_only_backslash_and_closing_bracket() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();

    assert_eq!(escape_markdown_link_label(&u("a]b")), "a\\]b");
    assert_eq!(escape_markdown_link_label(&u("a\\b")), "a\\\\b");
    assert_eq!(escape_markdown_link_label(&u("]]")), "\\]\\]");
}

#[test]
fn escape_markdown_link_label_does_not_escape_characters_that_are_safe_in_link_text() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();

    // these would be escaped by escapeMarkdownSyntaxTokens but must
    // pass through here since they render literally inside `[...]`.
    assert_eq!(escape_markdown_link_label(&u("a*b_c#d-e.f!g~h+i(j)k{l}m")), "a*b_c#d-e.f!g~h+i(j)k{l}m");
}

// suite('escapeMarkdownSyntaxTokens')

#[test]
fn escape_markdown_syntax_tokens_escapes_inline_syntax_tokens_anywhere() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();

    assert_eq!(
        escape_markdown_syntax_tokens(&u("a*b_c`d[e]f(g)h#i+j!k~l{m}")),
        "a\\*b\\_c\\`d\\[e\\]f\\(g\\)h\\#i\\+j\\!k\\~l\\{m\\}"
    );
}

#[test]
fn escape_markdown_syntax_tokens_does_not_escape_mid_line_dashes() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();

    assert_eq!(escape_markdown_syntax_tokens(&u("heap-snapshot-analysis")), "heap-snapshot-analysis");
    assert_eq!(escape_markdown_syntax_tokens(&u("npm run foo-bar")), "npm run foo-bar");
}

#[test]
fn escape_markdown_syntax_tokens_escapes_dashes_that_start_a_line() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();

    assert_eq!(escape_markdown_syntax_tokens(&u("- item")), "\\- item");
    assert_eq!(escape_markdown_syntax_tokens(&u("  - indented")), "  \\- indented");
    assert_eq!(escape_markdown_syntax_tokens(&u("---")), "\\---");
    assert_eq!(escape_markdown_syntax_tokens(&u("line one\n- item")), "line one\n\\- item");
}
