//! Porte de `src/vs/base/test/common/markdownString.test.ts` (suíte `MarkdownString`), caso a caso e na
//! mesma ordem.
//!
//! O `ensureNoDisposablesAreLeakedInTestSuite` não entra, porque `MarkdownString` não cria `IDisposable`.
//! Texto é `&[u16]`; `u` converte um `&str`. As suítes aninhadas do original são módulos.

use je_base::html_content::{
    IMarkdownString, IsTrusted, MarkdownString, MarkdownStringOptions, MarkdownStringTextNewlineStyle, TrustedOrOptions,
    UriOrOwnedString,
};
use je_base::js_string::JsString;
use je_base::map::OrderedMap;
use je_base::path::Platform;
use je_base::uri::URI;

fn u(text: &str) -> Vec<u16> {
    text.encode_utf16().collect()
}

fn new_mds() -> MarkdownString {
    MarkdownString::new(None, TrustedOrOptions::default())
}

fn support_theme_icons(enabled: bool) -> MarkdownString {
    MarkdownString::new(
        None,
        TrustedOrOptions::Options(MarkdownStringOptions { support_theme_icons: Some(enabled), ..MarkdownStringOptions::default() }),
    )
}

fn paragraph(mds: &mut MarkdownString, text: &str) {
    mds.append_text(&u(text), MarkdownStringTextNewlineStyle::Paragraph);
}

#[test]
fn escape_leading_whitespace() {
    let mut mds = new_mds();
    paragraph(&mut mds, "Hello\n    Not a code block");
    assert_eq!(mds.value, "Hello\n\n&nbsp;&nbsp;&nbsp;&nbsp;Not&nbsp;a&nbsp;code&nbsp;block");
}

#[test]
fn markdown_string_append_text_doesnt_escape_quote_109040() {
    let mut mds = new_mds();
    paragraph(&mut mds, "> Text\n>More");
    assert_eq!(mds.value, "\\>&nbsp;Text\n\n\\>More");
}

#[test]
fn append_text() {
    let mut mds = new_mds();
    paragraph(&mut mds, "# foo\n*bar*");

    assert_eq!(mds.value, "\\#&nbsp;foo\n\n\\*bar\\*");
}

#[test]
fn append_link() {
    fn assert_link(target: &str, label: &str, title: Option<&str>, expected: &str) {
        let mut mds = new_mds();
        let title = title.map(u);
        mds.append_link(UriOrOwnedString::Str(&u(target)), &u(label), title.as_deref());
        assert_eq!(mds.value, expected);
    }

    assert_link(
        "https://example.com\\()![](file:///Users/jrieken/Code/_samples/devfest/foo/img.png)",
        "hello",
        None,
        "[hello](https://example.com\\(\\)![](file:///Users/jrieken/Code/_samples/devfest/foo/img.png\\))",
    );
    assert_link("https://example.com", "hello", Some("title"), "[hello](https://example.com \"title\")");
    assert_link("foo)", "hello]", None, "[hello\\]](foo\\))");
    assert_link("foo\\)", "hello]", None, "[hello\\]](foo\\))");
    assert_link("fo)o", "hell]o", None, "[hell\\]o](fo\\)o)");
    assert_link("foo)", "hello]", Some("title\""), "[hello\\]](foo\\) \"title\\\"\")");
}

#[test]
fn lift() {
    let file = |path: &str| URI::file(path, Platform::Posix);
    let mut uris = OrderedMap::new();
    uris.insert(file("/foo/bar2").to_string(false), file("/foo/bar2"));
    uris.insert(file("/foo/bar3").to_string(false), file("/foo/bar3"));
    let dto = IMarkdownString {
        value: JsString::from("hello"),
        base_uri: Some(file("/foo/bar")),
        support_theme_icons: Some(true),
        is_trusted: Some(IsTrusted::Bool(true)),
        support_html: Some(true),
        uris: Some(uris),
        ..IMarkdownString::default()
    };
    let mds = MarkdownString::lift(&dto);
    assert_eq!(mds.value, dto.value);
    assert_eq!(mds.base_uri.as_ref().map(|u| u.to_string(false)), dto.base_uri.as_ref().map(|u| u.to_string(false)));
    assert_eq!(mds.support_theme_icons, dto.support_theme_icons);
    assert_eq!(mds.is_trusted, dto.is_trusted);
    assert_eq!(mds.support_html, dto.support_html);
    // `deepStrictEqual(mds.uris, dto.uris)`
    let (actual, expected) = (mds.uris.unwrap(), dto.uris.unwrap());
    assert_eq!(actual.len(), expected.len());
    for ((ak, av), (ek, ev)) in actual.iter().zip(expected.iter()) {
        assert_eq!(ak, ek);
        assert_eq!(av, ev);
    }
}

#[test]
fn lift_returns_new_instance() {
    let instance = MarkdownString::new(Some(JsString::from("hello")), TrustedOrOptions::default());
    let mut mds2 = MarkdownString::lift(&IMarkdownString { value: instance.value.clone(), ..IMarkdownString::default() });
    paragraph(&mut mds2, "world");
    assert_eq!(mds2.value, "helloworld");
    assert_eq!(instance.value, "hello");
}

mod append_code_block {
    use super::*;

    fn assert_code_block(lang: &str, code: &str, result: &str) {
        let mut mds = new_mds();
        mds.append_codeblock(&u(lang), &u(code));
        assert_eq!(mds.value, result);
    }

    #[test]
    fn common_cases() {
        // no backticks
        assert_code_block("ts", "const a = 1;", &format!("\n{}\n", ["```ts", "const a = 1;", "```"].join("\n")));
        // backticks
        assert_code_block("ts", "const a = `1`;", &format!("\n{}\n", ["```ts", "const a = `1`;", "```"].join("\n")));
    }

    // @see https://github.com/microsoft/vscode/issues/193746
    #[test]
    fn escape_fence() {
        // fence in the first line
        assert_code_block("md", "```\n```", &format!("\n{}\n", ["````md", "```\n```", "````"].join("\n")));
        // fence in the middle of code
        assert_code_block("md", "\n\n```\n```", &format!("\n{}\n", ["````md", "\n\n```\n```", "````"].join("\n")));
        // longer fence at the end of code
        assert_code_block(
            "md",
            "```\n```\n````\n````",
            &format!("\n{}\n", ["`````md", "```\n```\n````\n````", "`````"].join("\n")),
        );
    }
}

mod theme_icons {
    use super::*;

    mod support_on {
        use super::*;

        #[test]
        fn append_text() {
            let mut mds = support_theme_icons(true);
            paragraph(&mut mds, "$(zap) $(not a theme icon) $(add)");

            assert_eq!(mds.value, "\\\\$\\(zap\\)&nbsp;$\\(not&nbsp;a&nbsp;theme&nbsp;icon\\)&nbsp;\\\\$\\(add\\)");
        }

        #[test]
        fn append_markdown() {
            let mut mds = support_theme_icons(true);
            mds.append_markdown(&u("$(zap) $(not a theme icon) $(add)"));

            assert_eq!(mds.value, "$(zap) $(not a theme icon) $(add)");
        }

        #[test]
        fn append_markdown_with_escaped_icon() {
            let mut mds = support_theme_icons(true);
            mds.append_markdown(&u("\\$(zap) $(not a theme icon) $(add)"));

            assert_eq!(mds.value, "\\$(zap) $(not a theme icon) $(add)");
        }
    }

    mod support_off {
        use super::*;

        #[test]
        fn append_text() {
            let mut mds = support_theme_icons(false);
            paragraph(&mut mds, "$(zap) $(not a theme icon) $(add)");

            assert_eq!(mds.value, "$\\(zap\\)&nbsp;$\\(not&nbsp;a&nbsp;theme&nbsp;icon\\)&nbsp;$\\(add\\)");
        }

        #[test]
        fn append_markdown() {
            let mut mds = support_theme_icons(false);
            mds.append_markdown(&u("$(zap) $(not a theme icon) $(add)"));

            assert_eq!(mds.value, "$(zap) $(not a theme icon) $(add)");
        }

        #[test]
        fn append_markdown_with_escaped_icon() {
            let mut mds = support_theme_icons(true);
            mds.append_markdown(&u("\\$(zap) $(not a theme icon) $(add)"));

            assert_eq!(mds.value, "\\$(zap) $(not a theme icon) $(add)");
        }
    }
}
