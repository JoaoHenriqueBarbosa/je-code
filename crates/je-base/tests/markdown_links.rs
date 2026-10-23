//! Porte de `src/vs/base/test/common/markdownLinks.test.ts` (suíte `rewriteMarkdownLinks`), caso a caso e na mesma ordem.

use je_base::JsString;
use je_base::markdown_links::rewrite_markdown_links;
use je_base::marked::Token;

fn rewrite_to_marker(token: &Token) -> Option<JsString> {
    let _ = token;
    Some(JsString::from("REWRITTEN"))
}

fn rewrite(markdown: &str, mut rewriter: impl FnMut(&Token) -> Option<JsString>) -> String {
    let units: Vec<u16> = markdown.encode_utf16().collect();
    rewrite_markdown_links(&units, &mut rewriter).to_string_lossy()
}

#[test]
fn edits_the_token_that_produced_the_source_not_the_first_lookalike() {
    // Procurar o texto do token acharia o exemplo dentro do trecho de código, corrompendo-o e deixando
    // o link de verdade no lugar.
    assert_eq!(
        [
            rewrite("Use `[a](./x.txt)` then a real [a](./x.txt) link.", rewrite_to_marker),
            rewrite("```\n[a](./x.txt)\n```\n\n[a](./x.txt)", rewrite_to_marker),
        ],
        [
            "Use `[a](./x.txt)` then a real REWRITTEN link.",
            "```\n[a](./x.txt)\n```\n\nREWRITTEN"
        ],
    );
}

#[test]
fn keeps_tokens_the_rewriter_declines() {
    let markdown = "See [a](./x.txt) and ![b](./y.png).";
    assert_eq!(rewrite(markdown, |_| None), markdown);
}

#[test]
fn returns_the_source_unchanged_when_it_cannot_be_parsed_as_markdown() {
    assert_eq!(rewrite("", rewrite_to_marker), "");
}
