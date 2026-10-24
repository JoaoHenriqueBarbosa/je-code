//! Porte de `src/vs/base/test/browser/htmlToMarkdown.test.ts` (suíte `htmlToMarkdown`), caso a caso e na
//! mesma ordem. A suíte aninhada `internal links` do original vira o prefixo `internal_links_` dos nomes.
//!
//! O HTML é parseado pelo parser em Rust puro de `html_parser.rs`, no lugar do `DOMParser` do navegador
//! (ver `html_to_markdown.rs`). O travessão que o original usa nos casos de entidade numérica
//! (`&#8212;`) é escrito como `\u{2014}`.

mod common;

use common::ensure_no_disposables_are_leaked_in_test_suite;
use je_base::browser::html_to_markdown::convert_html_to_markdown;

fn convert(html: &str) -> String {
    let units: Vec<u16> = html.encode_utf16().collect();
    convert_html_to_markdown(&units).to_string_lossy()
}

#[test]
fn converts_headings() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();

    assert_eq!(convert("<h1>Title</h1>"), "# Title");
    assert_eq!(convert("<h2>Subtitle</h2>"), "## Subtitle");
    assert_eq!(convert("<h3>Section</h3>"), "### Section");
    assert_eq!(convert("<h4>Sub-section</h4>"), "#### Sub-section");
    assert_eq!(convert("<h5>Minor</h5>"), "##### Minor");
    assert_eq!(convert("<h6>Smallest</h6>"), "###### Smallest");
}

#[test]
fn converts_links() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();

    assert_eq!(convert("<a href=\"https://example.com\">Example</a>"), "[Example](https://example.com)");
}

#[test]
fn strips_dangerous_schemes_from_links() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();

    assert_eq!(convert("<a href=\"javascript:alert(1)\">click</a>"), "click");
    assert_eq!(convert("<a href=\"vbscript:run\">run</a>"), "run");
    assert_eq!(convert("<a href=\"data:text/html,<h1>hi</h1>\">data</a>"), "data");
}

#[test]
fn internal_links_converts_targets_that_only_resolve_in_this_window_to_inline_code() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();

    assert_eq!(
        [
            convert("<a href=\"vscode-file://vscode-app/c:/Users/user/AppData/Local/Programs/Microsoft%20VS%20Code/resources/app/out/vs/code/electron-browser/workbench/workbench.html\">foo-bar.md</a>"),
            convert("<a href=\"file:///home/user/project/DefaultMeshInterpolator.java#46,62\">mx:text</a>"),
            convert("<a href=\"code-oss://file/c:/repo/src/config.ts\">src/config.ts</a>"),
            convert("<a href=\"http://_vscodecontentref_/0\">index.ts</a>"),
            // The agent host prompt asks models for bare absolute paths.
            convert("<a href=\"/Users/me/repo/src/a.ts\">a.ts</a>"),
            convert("<a href=\"command:workbench.action.files.save\">Save</a>"),
            convert("<img src=\"/Users/me/shot.png\" alt=\"screenshot\">"),
        ],
        ["`foo-bar.md`", "`mx:text`", "`src/config.ts`", "`index.ts`", "`a.ts`", "`Save`", "`screenshot`"]
    );
}

#[test]
fn internal_links_prefers_data_href_only_when_href_cannot_be_shared() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();

    assert_eq!(
        [
            convert("<a href=\"\" data-href=\"file:///home/user/project/a.ts\">a.ts</a>"),
            convert("<a href=\"vscode-file://vscode-app/resources/app/out/workbench.html\" data-href=\"https://example.com\">Example</a>"),
            // A page of unknown origin must not redirect a link it displayed as safe.
            convert("<a href=\"https://real.example\" data-href=\"https://evil.example\">docs</a>"),
        ],
        ["`a.ts`", "[Example](https://example.com)", "[docs](https://real.example)"]
    );
}

#[test]
fn internal_links_uses_the_plain_label_when_the_target_cannot_be_shared() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();

    // Emphasis markers would be read literally inside a code span.
    assert_eq!(
        [convert("<a href=\"file:///x\"><strong>Foo</strong></a>"), convert("<a href=\"https://example.com\"><strong>Kept</strong></a>")],
        ["`Foo`", "[**Kept**](https://example.com)"]
    );
}

#[test]
fn internal_links_keeps_internal_links_inside_surrounding_markdown_formatting() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();

    assert_eq!(
        convert("<p>This is <strong>inherited</strong> from the <a href=\"vscode-file://vscode-app/resources/app/out/workbench.html\">FooBar</a> class.</p>"),
        "This is **inherited** from the `FooBar` class."
    );
}

#[test]
fn internal_links_keeps_document_relative_links() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();

    assert_eq!(
        [convert("<a href=\"#details\">details</a>"), convert("<a href=\"../CONTRIBUTING.md\">guide</a>")],
        ["[details](#details)", "[guide](../CONTRIBUTING.md)"]
    );
}

#[test]
fn converts_bold_and_italic() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();

    assert_eq!(convert("<strong>bold</strong>"), "**bold**");
    assert_eq!(convert("<b>bold</b>"), "**bold**");
    assert_eq!(convert("<em>italic</em>"), "*italic*");
    assert_eq!(convert("<i>italic</i>"), "*italic*");
}

#[test]
fn converts_inline_code() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();

    assert_eq!(convert("<code>foo()</code>"), "`foo()`");
}

#[test]
fn preserves_html_tag_names_inside_inline_code() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();

    assert_eq!(convert("<code>&lt;aside&gt;</code>"), "`<aside>`");
    assert_eq!(convert("<code>&lt;details&gt;</code>"), "`<details>`");
}

#[test]
fn preserves_html_tag_names_inside_inline_code_with_nested_tags() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();

    assert_eq!(convert("<code><span class=\"hl\">&lt;aside&gt;</span></code>"), "`<aside>`");
}

#[test]
fn preserves_html_tag_names_inside_code_blocks() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();

    assert_eq!(convert("<pre><code>&lt;aside&gt;</code></pre>"), "```\n<aside>\n```");
}

#[test]
fn converts_code_blocks() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();

    assert_eq!(convert("<pre><code>const x = 1;</code></pre>"), "```\nconst x = 1;\n```");
}

#[test]
fn converts_syntax_highlighted_code_blocks_by_stripping_inner_tags() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();

    assert_eq!(
        convert("<pre><code><span class=\"kw\">const</span> x = <span class=\"num\">1</span>;</code></pre>"),
        "```\nconst x = 1;\n```"
    );
}

#[test]
fn preserves_indentation_in_code_blocks() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();

    assert_eq!(convert("<pre><code>function foo() {\n  return 1;\n}</code></pre>"), "```\nfunction foo() {\n  return 1;\n}\n```");
}

#[test]
fn converts_unordered_lists() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();

    let html = "<ul><li>one</li><li>two</li><li>three</li></ul>";
    assert_eq!(convert(html), "- one\n- two\n- three");
}

#[test]
fn converts_ordered_lists_to_numbered_items() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();

    let html = "<ol><li>first</li><li>second</li></ol>";
    assert_eq!(convert(html), "1. first\n2. second");
}

#[test]
fn converts_line_breaks() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();

    assert_eq!(convert("hello<br>world"), "hello\nworld");
    assert_eq!(convert("hello<br/>world"), "hello\nworld");
}

#[test]
fn converts_horizontal_rules() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();

    assert_eq!(convert("above<hr>below"), "above\n---\nbelow");
}

#[test]
fn converts_strikethrough() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();

    assert_eq!(convert("<del>removed</del>"), "~~removed~~");
    assert_eq!(convert("<s>struck</s>"), "~~struck~~");
}

#[test]
fn converts_blockquotes() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();

    assert_eq!(convert("<blockquote>quoted text</blockquote>"), "> quoted text");
}

#[test]
fn converts_images() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();

    assert_eq!(convert("<img src=\"https://example.com/img.png\" alt=\"photo\">"), "![photo](https://example.com/img.png)");
}

#[test]
fn decodes_html_entities() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();

    assert_eq!(convert("&amp; &lt; &gt; &quot; &#39;"), "& < > \" '");
}

#[test]
fn strips_unknown_tags() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();

    assert_eq!(convert("<span class=\"x\">hello</span>"), "hello");
}

#[test]
fn handles_nested_inline_elements() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();

    assert_eq!(convert("<strong><em>bold italic</em></strong>"), "***bold italic***");
}

#[test]
fn handles_link_with_bold_text_inside() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();

    assert_eq!(convert("<a href=\"https://example.com\"><strong>click here</strong></a>"), "[**click here**](https://example.com)");
}

#[test]
fn handles_heading_with_link_inside() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();

    assert_eq!(convert("<h2><a href=\"https://example.com\">Title</a></h2>"), "## [Title](https://example.com)");
}

#[test]
fn collapses_excessive_newlines() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();

    let html = "<p>one</p><p></p><p></p><p>two</p>";
    let result = convert(html);
    assert!(!result.contains("\n\n\n"), "should not have 3+ consecutive newlines");
    assert!(result.contains("one"));
    assert!(result.contains("two"));
}

#[test]
fn handles_a_realistic_web_page_snippet() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();

    let html = "
				<h1>Getting Started</h1>
				<p>Welcome to <strong>VS Code</strong>. Visit <a href=\"https://code.visualstudio.com\">the website</a> for more info.</p>
				<ul>
					<li>Fast</li>
					<li>Extensible</li>
				</ul>
			";
    let md = convert(html);
    assert!(md.contains("# Getting Started"));
    assert!(md.contains("**VS Code**"));
    assert!(md.contains("[the website](https://code.visualstudio.com)"));
    assert!(md.contains("- Fast"));
    assert!(md.contains("- Extensible"));
}

#[test]
fn decodes_numeric_html_entities() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();

    assert_eq!(convert("&#60;tag&#62;"), "<tag>");
    assert_eq!(convert("&#x3C;tag&#x3E;"), "<tag>");
    assert_eq!(convert("&#8212;"), "\u{2014}");
    assert_eq!(convert("&#x2014;"), "\u{2014}");
}

#[test]
fn falls_back_to_tag_stripping_for_very_large_input() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();

    let large = format!("<b>{}</b>", "x".repeat(200_001));
    let result = convert(&large);
    // Should strip tags but NOT apply markdown bold formatting
    assert!(!result.contains("**"));
    assert!(!result.contains("<b>"));
}
