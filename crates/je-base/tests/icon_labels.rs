//! Porte de `src/vs/base/test/common/iconLabels.test.ts` (suíte `Icon Labels`), caso a caso e na mesma ordem.
//!
//! O `ensureNoDisposablesAreLeakedInTestSuite` não entra, porque `iconLabels` não cria `IDisposable`.
//! Texto é `&[u16]`; `u` converte um `&str`.

use je_base::filters::Match;
use je_base::icon_labels::{
    ParsedLabelWithIcons, escape_icons, get_codicon_aria_label, markdown_escape_escaped_icons, matches_fuzzy_icon_aware,
    parse_label_with_icons, strip_icons,
};

fn u(text: &str) -> Vec<u16> {
    text.encode_utf16().collect()
}

fn m(start: usize, end: usize) -> Match {
    Match { start, end }
}

type IconFilter = fn(&[u16], &ParsedLabelWithIcons, bool) -> Option<Vec<Match>>;

fn filter_ok(filter: IconFilter, word: &str, target: &ParsedLabelWithIcons, highlights: Option<Vec<Match>>) {
    let r = filter(&u(word), target, false);
    let r = r.expect("o filtro devia casar");
    if let Some(highlights) = highlights {
        assert_eq!(r, highlights);
    }
}

#[test]
fn can_get_proper_aria_labels() {
    // note, the spaces in the results are important
    let test_cases = [
        ("", ""),
        ("asdf", "asdf"),
        ("asdf$(squirrel)asdf", "asdf squirrel asdf"),
        ("asdf $(squirrel) asdf", "asdf  squirrel  asdf"),
        ("$(rocket)asdf", "rocket asdf"),
        ("$(rocket) asdf", "rocket  asdf"),
        ("$(rocket)$(rocket)$(rocket)asdf", "rocket  rocket  rocket asdf"),
        ("$(rocket) asdf $(rocket)", "rocket  asdf  rocket"),
        ("$(rocket)asdf$(rocket)", "rocket asdf rocket"),
    ];

    for (input, expected) in test_cases {
        assert_eq!(get_codicon_aria_label(Some(&u(input))), expected);
    }
}

#[test]
fn matches_fuzzy_icon_aware_case() {
    // Camel Case

    filter_ok(
        matches_fuzzy_icon_aware,
        "ccr",
        &parse_label_with_icons(&u("$(codicon)CamelCaseRocks$(codicon)")),
        Some(vec![m(10, 11), m(15, 16), m(19, 20)]),
    );

    filter_ok(
        matches_fuzzy_icon_aware,
        "ccr",
        &parse_label_with_icons(&u("$(codicon) CamelCaseRocks $(codicon)")),
        Some(vec![m(11, 12), m(16, 17), m(20, 21)]),
    );

    filter_ok(
        matches_fuzzy_icon_aware,
        "iut",
        &parse_label_with_icons(&u("$(codicon) Indent $(octico) Using $(octic) Tpaces")),
        Some(vec![m(11, 12), m(28, 29), m(43, 44)]),
    );

    // Prefix

    filter_ok(
        matches_fuzzy_icon_aware,
        "using",
        &parse_label_with_icons(&u("$(codicon) Indent Using Spaces")),
        Some(vec![m(18, 23)]),
    );

    // Broken Codicon

    filter_ok(
        matches_fuzzy_icon_aware,
        "codicon",
        &parse_label_with_icons(&u("This $(codicon Indent Using Spaces")),
        Some(vec![m(7, 14)]),
    );

    filter_ok(
        matches_fuzzy_icon_aware,
        "indent",
        &parse_label_with_icons(&u("This $codicon Indent Using Spaces")),
        Some(vec![m(14, 20)]),
    );

    // Testing #59343
    filter_ok(
        matches_fuzzy_icon_aware,
        "unt",
        &parse_label_with_icons(&u("$(primitive-dot) $(file-text) Untitled-1")),
        Some(vec![m(30, 33)]),
    );

    // Testing #136172
    filter_ok(
        matches_fuzzy_icon_aware,
        "s",
        &parse_label_with_icons(&u("$(loading~spin) start")),
        Some(vec![m(16, 17)]),
    );
}

#[test]
fn strip_icons_case() {
    assert_eq!(strip_icons(&u("Hello World")), "Hello World");
    assert_eq!(strip_icons(&u("$(Hello World")), "$(Hello World");
    assert_eq!(strip_icons(&u("$(Hello) World")), " World");
    assert_eq!(strip_icons(&u("$(Hello) W$(oi)rld")), " Wrld");
}

#[test]
fn escape_icons_case() {
    assert_eq!(escape_icons(&u("Hello World")), "Hello World");
    assert_eq!(escape_icons(&u("$(Hello World")), "$(Hello World");
    assert_eq!(escape_icons(&u("$(Hello) World")), "\\$(Hello) World");
    assert_eq!(escape_icons(&u("\\$(Hello) W$(oi)rld")), "\\$(Hello) W\\$(oi)rld");
}

#[test]
fn markdown_escape_escaped_icons_case() {
    assert_eq!(markdown_escape_escaped_icons(&u("Hello World")), "Hello World");
    assert_eq!(markdown_escape_escaped_icons(&u("$(Hello) World")), "$(Hello) World");
    assert_eq!(markdown_escape_escaped_icons(&u("\\$(Hello) World")), "\\\\$(Hello) World");
}
