//! Porte de `src/vs/editor/test/common/modes/supports/tokenization.test.ts` (suítes `Token theme matching`, `Token theme parsing` e `Token theme resolving`), caso a caso e na mesma ordem.

#[path = "../../je-base/tests/common/mod.rs"]
mod common;

use common::ensure_no_disposables_are_leaked_in_test_suite;
use je_editor::encoded_token_attributes::font_style;
use je_editor::token_theme::{
    ColorMap, ExternalThemeTrieElement, ITokenThemeRule, ParsedTokenThemeRule,
    ThemeTrieElementRule, TokenTheme, parse_token_theme, strcmp,
};

/// `new ThemeTrieElementRule(fontStyle, foreground, background)` com o `FontStyle` em `u32`.
fn rule(font: u32, foreground: u32, background: u32) -> ThemeTrieElementRule {
    ThemeTrieElementRule::new(font as i32, foreground, background)
}

/// `colorMap.getId(color)` para uma cor válida.
fn id(color_map: &mut ColorMap, color: &str) -> u32 {
    color_map.get_id(Some(color)).unwrap()
}

// Suíte `Token theme matching`.

#[test]
fn gives_higher_priority_to_deeper_matches() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    let theme = TokenTheme::create_from_raw_token_theme(
        &[
            ITokenThemeRule::new("")
                .with_foreground("100000")
                .with_background("200000"),
            ITokenThemeRule::new("punctuation.definition.string.begin.html").with_foreground("300000"),
            ITokenThemeRule::new("punctuation.definition.string").with_foreground("400000"),
        ],
        &[],
    )
    .unwrap();

    let mut color_map = ColorMap::new();
    id(&mut color_map, "100000");
    let b = id(&mut color_map, "200000");
    id(&mut color_map, "400000");
    let d = id(&mut color_map, "300000");

    let actual = theme.match_rule("punctuation.definition.string.begin.html");

    assert_eq!(actual, &rule(font_style::NONE, d, b));
}

#[test]
fn can_match() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    let theme = TokenTheme::create_from_raw_token_theme(
        &[
            ITokenThemeRule::new("")
                .with_foreground("F8F8F2")
                .with_background("272822"),
            ITokenThemeRule::new("source").with_background("100000"),
            ITokenThemeRule::new("something").with_background("100000"),
            ITokenThemeRule::new("bar").with_background("200000"),
            ITokenThemeRule::new("baz").with_background("200000"),
            ITokenThemeRule::new("bar").with_font_style("bold"),
            ITokenThemeRule::new("constant")
                .with_font_style("italic")
                .with_foreground("300000"),
            ITokenThemeRule::new("constant.numeric").with_foreground("400000"),
            ITokenThemeRule::new("constant.numeric.hex").with_font_style("bold"),
            ITokenThemeRule::new("constant.numeric.oct").with_font_style("bold italic underline"),
            ITokenThemeRule::new("constant.numeric.bin").with_font_style("bold strikethrough"),
            ITokenThemeRule::new("constant.numeric.dec")
                .with_font_style("")
                .with_foreground("500000"),
            ITokenThemeRule::new("storage.object.bar")
                .with_font_style("")
                .with_foreground("600000"),
        ],
        &[],
    )
    .unwrap();

    let mut color_map = ColorMap::new();
    let a = id(&mut color_map, "F8F8F2");
    let b = id(&mut color_map, "272822");
    let c = id(&mut color_map, "200000");
    let d = id(&mut color_map, "300000");
    let e = id(&mut color_map, "400000");
    let f = id(&mut color_map, "500000");
    let g = id(&mut color_map, "100000");
    let h = id(&mut color_map, "600000");

    let assert_match = |scope_name: &str, expected: ThemeTrieElementRule| {
        let actual = theme.match_rule(scope_name);
        assert_eq!(actual, &expected, "when matching <<{scope_name}>>");
    };

    let assert_simple_match = |scope_name: &str, font: u32, foreground: u32, background: u32| {
        assert_match(scope_name, rule(font, foreground, background));
    };

    let assert_no_match = |scope_name: &str| {
        assert_match(scope_name, rule(font_style::NONE, a, b));
    };

    // matches defaults
    assert_no_match("");
    assert_no_match("bazz");
    assert_no_match("asdfg");

    // matches source
    assert_simple_match("source", font_style::NONE, a, g);
    assert_simple_match("source.ts", font_style::NONE, a, g);
    assert_simple_match("source.tss", font_style::NONE, a, g);

    // matches something
    assert_simple_match("something", font_style::NONE, a, g);
    assert_simple_match("something.ts", font_style::NONE, a, g);
    assert_simple_match("something.tss", font_style::NONE, a, g);

    // matches baz
    assert_simple_match("baz", font_style::NONE, a, c);
    assert_simple_match("baz.ts", font_style::NONE, a, c);
    assert_simple_match("baz.tss", font_style::NONE, a, c);

    // matches constant
    assert_simple_match("constant", font_style::ITALIC, d, b);
    assert_simple_match("constant.string", font_style::ITALIC, d, b);
    assert_simple_match("constant.hex", font_style::ITALIC, d, b);

    // matches constant.numeric
    assert_simple_match("constant.numeric", font_style::ITALIC, e, b);
    assert_simple_match("constant.numeric.baz", font_style::ITALIC, e, b);

    // matches constant.numeric.hex
    assert_simple_match("constant.numeric.hex", font_style::BOLD, e, b);
    assert_simple_match("constant.numeric.hex.baz", font_style::BOLD, e, b);

    // matches constant.numeric.oct
    let bold_italic_underline = font_style::BOLD | font_style::ITALIC | font_style::UNDERLINE;
    assert_simple_match("constant.numeric.oct", bold_italic_underline, e, b);
    assert_simple_match("constant.numeric.oct.baz", bold_italic_underline, e, b);

    // matches constant.numeric.bin
    let bold_strikethrough = font_style::BOLD | font_style::STRIKETHROUGH;
    assert_simple_match("constant.numeric.bin", bold_strikethrough, e, b);
    assert_simple_match("constant.numeric.bin.baz", bold_strikethrough, e, b);

    // matches constant.numeric.dec
    assert_simple_match("constant.numeric.dec", font_style::NONE, f, b);
    assert_simple_match("constant.numeric.dec.baz", font_style::NONE, f, b);

    // matches storage.object.bar
    assert_simple_match("storage.object.bar", font_style::NONE, h, b);
    assert_simple_match("storage.object.bar.baz", font_style::NONE, h, b);

    // does not match storage.object.bar
    assert_simple_match("storage.object.bart", font_style::NONE, a, b);
    assert_simple_match("storage.object", font_style::NONE, a, b);
    assert_simple_match("storage", font_style::NONE, a, b);

    assert_simple_match("bar", font_style::BOLD, a, c);
}

// Suíte `Token theme parsing`.

#[test]
fn can_parse() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();

    let actual = parse_token_theme(&[
        ITokenThemeRule::new("")
            .with_foreground("F8F8F2")
            .with_background("272822"),
        ITokenThemeRule::new("source").with_background("100000"),
        ITokenThemeRule::new("something").with_background("100000"),
        ITokenThemeRule::new("bar").with_background("010000"),
        ITokenThemeRule::new("baz").with_background("010000"),
        ITokenThemeRule::new("bar").with_font_style("bold"),
        ITokenThemeRule::new("constant")
            .with_font_style("italic")
            .with_foreground("ff0000"),
        ITokenThemeRule::new("constant.numeric").with_foreground("00ff00"),
        ITokenThemeRule::new("constant.numeric.hex").with_font_style("bold"),
        ITokenThemeRule::new("constant.numeric.oct").with_font_style("bold italic underline"),
        ITokenThemeRule::new("constant.numeric.dec")
            .with_font_style("")
            .with_foreground("0000ff"),
    ]);

    let not_set = font_style::NOT_SET;
    let expected = vec![
        ParsedTokenThemeRule::new("", 0, not_set, Some("F8F8F2"), Some("272822")),
        ParsedTokenThemeRule::new("source", 1, not_set, None, Some("100000")),
        ParsedTokenThemeRule::new("something", 2, not_set, None, Some("100000")),
        ParsedTokenThemeRule::new("bar", 3, not_set, None, Some("010000")),
        ParsedTokenThemeRule::new("baz", 4, not_set, None, Some("010000")),
        ParsedTokenThemeRule::new("bar", 5, font_style::BOLD as i32, None, None),
        ParsedTokenThemeRule::new("constant", 6, font_style::ITALIC as i32, Some("ff0000"), None),
        ParsedTokenThemeRule::new("constant.numeric", 7, not_set, Some("00ff00"), None),
        ParsedTokenThemeRule::new("constant.numeric.hex", 8, font_style::BOLD as i32, None, None),
        ParsedTokenThemeRule::new(
            "constant.numeric.oct",
            9,
            (font_style::BOLD | font_style::ITALIC | font_style::UNDERLINE) as i32,
            None,
            None,
        ),
        ParsedTokenThemeRule::new(
            "constant.numeric.dec",
            10,
            font_style::NONE as i32,
            Some("0000ff"),
            None,
        ),
    ];

    assert_eq!(actual, expected);
}

// Suíte `Token theme resolving`.

#[test]
fn strcmp_works() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    let mut actual = vec!["bar", "z", "zu", "a", "ab", ""];
    actual.sort_by(|a, b| strcmp(a, b).cmp(&0));

    let expected = vec!["", "a", "ab", "bar", "z", "zu"];
    assert_eq!(actual, expected);
}

#[test]
fn always_has_defaults() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    let actual = TokenTheme::create_from_parsed_token_theme(vec![], &[]).unwrap();
    let mut color_map = ColorMap::new();
    let a = id(&mut color_map, "000000");
    let b = id(&mut color_map, "ffffff");
    assert_eq!(actual.get_color_map(), color_map.get_color_map());
    assert_eq!(
        actual.get_theme_trie_element(),
        ExternalThemeTrieElement::new(rule(font_style::NONE, a, b))
    );
}

#[test]
fn respects_incoming_defaults_1() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    let actual = TokenTheme::create_from_parsed_token_theme(
        vec![ParsedTokenThemeRule::new("", -1, font_style::NOT_SET, None, None)],
        &[],
    )
    .unwrap();
    let mut color_map = ColorMap::new();
    let a = id(&mut color_map, "000000");
    let b = id(&mut color_map, "ffffff");
    assert_eq!(actual.get_color_map(), color_map.get_color_map());
    assert_eq!(
        actual.get_theme_trie_element(),
        ExternalThemeTrieElement::new(rule(font_style::NONE, a, b))
    );
}

#[test]
fn respects_incoming_defaults_2() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    let actual = TokenTheme::create_from_parsed_token_theme(
        vec![ParsedTokenThemeRule::new(
            "",
            -1,
            font_style::NONE as i32,
            None,
            None,
        )],
        &[],
    )
    .unwrap();
    let mut color_map = ColorMap::new();
    let a = id(&mut color_map, "000000");
    let b = id(&mut color_map, "ffffff");
    assert_eq!(actual.get_color_map(), color_map.get_color_map());
    assert_eq!(
        actual.get_theme_trie_element(),
        ExternalThemeTrieElement::new(rule(font_style::NONE, a, b))
    );
}

#[test]
fn respects_incoming_defaults_3() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    let actual = TokenTheme::create_from_parsed_token_theme(
        vec![ParsedTokenThemeRule::new(
            "",
            -1,
            font_style::BOLD as i32,
            None,
            None,
        )],
        &[],
    )
    .unwrap();
    let mut color_map = ColorMap::new();
    let a = id(&mut color_map, "000000");
    let b = id(&mut color_map, "ffffff");
    assert_eq!(actual.get_color_map(), color_map.get_color_map());
    assert_eq!(
        actual.get_theme_trie_element(),
        ExternalThemeTrieElement::new(rule(font_style::BOLD, a, b))
    );
}

#[test]
fn respects_incoming_defaults_4() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    let actual = TokenTheme::create_from_parsed_token_theme(
        vec![ParsedTokenThemeRule::new(
            "",
            -1,
            font_style::NOT_SET,
            Some("ff0000"),
            None,
        )],
        &[],
    )
    .unwrap();
    let mut color_map = ColorMap::new();
    let a = id(&mut color_map, "ff0000");
    let b = id(&mut color_map, "ffffff");
    assert_eq!(actual.get_color_map(), color_map.get_color_map());
    assert_eq!(
        actual.get_theme_trie_element(),
        ExternalThemeTrieElement::new(rule(font_style::NONE, a, b))
    );
}

#[test]
fn respects_incoming_defaults_5() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    let actual = TokenTheme::create_from_parsed_token_theme(
        vec![ParsedTokenThemeRule::new(
            "",
            -1,
            font_style::NOT_SET,
            None,
            Some("ff0000"),
        )],
        &[],
    )
    .unwrap();
    let mut color_map = ColorMap::new();
    let a = id(&mut color_map, "000000");
    let b = id(&mut color_map, "ff0000");
    assert_eq!(actual.get_color_map(), color_map.get_color_map());
    assert_eq!(
        actual.get_theme_trie_element(),
        ExternalThemeTrieElement::new(rule(font_style::NONE, a, b))
    );
}

#[test]
fn can_merge_incoming_defaults() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    let actual = TokenTheme::create_from_parsed_token_theme(
        vec![
            ParsedTokenThemeRule::new("", -1, font_style::NOT_SET, None, Some("ff0000")),
            ParsedTokenThemeRule::new("", -1, font_style::NOT_SET, Some("00ff00"), None),
            ParsedTokenThemeRule::new("", -1, font_style::BOLD as i32, None, None),
        ],
        &[],
    )
    .unwrap();
    let mut color_map = ColorMap::new();
    let a = id(&mut color_map, "00ff00");
    let b = id(&mut color_map, "ff0000");
    assert_eq!(actual.get_color_map(), color_map.get_color_map());
    assert_eq!(
        actual.get_theme_trie_element(),
        ExternalThemeTrieElement::new(rule(font_style::BOLD, a, b))
    );
}

#[test]
fn defaults_are_inherited() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    let actual = TokenTheme::create_from_parsed_token_theme(
        vec![
            ParsedTokenThemeRule::new("", -1, font_style::NOT_SET, Some("F8F8F2"), Some("272822")),
            ParsedTokenThemeRule::new("var", -1, font_style::NOT_SET, Some("ff0000"), None),
        ],
        &[],
    )
    .unwrap();
    let mut color_map = ColorMap::new();
    let a = id(&mut color_map, "F8F8F2");
    let b = id(&mut color_map, "272822");
    let c = id(&mut color_map, "ff0000");
    assert_eq!(actual.get_color_map(), color_map.get_color_map());
    let root = ExternalThemeTrieElement::with_children(
        rule(font_style::NONE, a, b),
        [(
            "var",
            ExternalThemeTrieElement::new(rule(font_style::NONE, c, b)),
        )],
    );
    assert_eq!(actual.get_theme_trie_element(), root);
}

#[test]
fn same_rules_get_merged() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    let actual = TokenTheme::create_from_parsed_token_theme(
        vec![
            ParsedTokenThemeRule::new("", -1, font_style::NOT_SET, Some("F8F8F2"), Some("272822")),
            ParsedTokenThemeRule::new("var", 1, font_style::BOLD as i32, None, None),
            ParsedTokenThemeRule::new("var", 0, font_style::NOT_SET, Some("ff0000"), None),
        ],
        &[],
    )
    .unwrap();
    let mut color_map = ColorMap::new();
    let a = id(&mut color_map, "F8F8F2");
    let b = id(&mut color_map, "272822");
    let c = id(&mut color_map, "ff0000");
    assert_eq!(actual.get_color_map(), color_map.get_color_map());
    let root = ExternalThemeTrieElement::with_children(
        rule(font_style::NONE, a, b),
        [(
            "var",
            ExternalThemeTrieElement::new(rule(font_style::BOLD, c, b)),
        )],
    );
    assert_eq!(actual.get_theme_trie_element(), root);
}

#[test]
fn rules_are_inherited_1() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    let actual = TokenTheme::create_from_parsed_token_theme(
        vec![
            ParsedTokenThemeRule::new("", -1, font_style::NOT_SET, Some("F8F8F2"), Some("272822")),
            ParsedTokenThemeRule::new("var", -1, font_style::BOLD as i32, Some("ff0000"), None),
            ParsedTokenThemeRule::new("var.identifier", -1, font_style::NOT_SET, Some("00ff00"), None),
        ],
        &[],
    )
    .unwrap();
    let mut color_map = ColorMap::new();
    let a = id(&mut color_map, "F8F8F2");
    let b = id(&mut color_map, "272822");
    let c = id(&mut color_map, "ff0000");
    let d = id(&mut color_map, "00ff00");
    assert_eq!(actual.get_color_map(), color_map.get_color_map());
    let root = ExternalThemeTrieElement::with_children(
        rule(font_style::NONE, a, b),
        [(
            "var",
            ExternalThemeTrieElement::with_children(
                rule(font_style::BOLD, c, b),
                [(
                    "identifier",
                    ExternalThemeTrieElement::new(rule(font_style::BOLD, d, b)),
                )],
            ),
        )],
    );
    assert_eq!(actual.get_theme_trie_element(), root);
}

#[test]
fn rules_are_inherited_2() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    let bold_italic_underline = font_style::BOLD | font_style::ITALIC | font_style::UNDERLINE;
    let actual = TokenTheme::create_from_parsed_token_theme(
        vec![
            ParsedTokenThemeRule::new("", -1, font_style::NOT_SET, Some("F8F8F2"), Some("272822")),
            ParsedTokenThemeRule::new("var", -1, font_style::BOLD as i32, Some("ff0000"), None),
            ParsedTokenThemeRule::new("var.identifier", -1, font_style::NOT_SET, Some("00ff00"), None),
            ParsedTokenThemeRule::new("constant", 4, font_style::ITALIC as i32, Some("100000"), None),
            ParsedTokenThemeRule::new("constant.numeric", 5, font_style::NOT_SET, Some("200000"), None),
            ParsedTokenThemeRule::new("constant.numeric.hex", 6, font_style::BOLD as i32, None, None),
            ParsedTokenThemeRule::new(
                "constant.numeric.oct",
                7,
                bold_italic_underline as i32,
                None,
                None,
            ),
            ParsedTokenThemeRule::new(
                "constant.numeric.dec",
                8,
                font_style::NONE as i32,
                Some("300000"),
                None,
            ),
        ],
        &[],
    )
    .unwrap();
    let mut color_map = ColorMap::new();
    let a = id(&mut color_map, "F8F8F2");
    let b = id(&mut color_map, "272822");
    let c = id(&mut color_map, "100000");
    let d = id(&mut color_map, "200000");
    let e = id(&mut color_map, "300000");
    let f = id(&mut color_map, "ff0000");
    let g = id(&mut color_map, "00ff00");
    assert_eq!(actual.get_color_map(), color_map.get_color_map());
    let root = ExternalThemeTrieElement::with_children(
        rule(font_style::NONE, a, b),
        [
            (
                "var",
                ExternalThemeTrieElement::with_children(
                    rule(font_style::BOLD, f, b),
                    [(
                        "identifier",
                        ExternalThemeTrieElement::new(rule(font_style::BOLD, g, b)),
                    )],
                ),
            ),
            (
                "constant",
                ExternalThemeTrieElement::with_children(
                    rule(font_style::ITALIC, c, b),
                    [(
                        "numeric",
                        ExternalThemeTrieElement::with_children(
                            rule(font_style::ITALIC, d, b),
                            [
                                (
                                    "hex",
                                    ExternalThemeTrieElement::new(rule(font_style::BOLD, d, b)),
                                ),
                                (
                                    "oct",
                                    ExternalThemeTrieElement::new(rule(bold_italic_underline, d, b)),
                                ),
                                (
                                    "dec",
                                    ExternalThemeTrieElement::new(rule(font_style::NONE, e, b)),
                                ),
                            ],
                        ),
                    )],
                ),
            ),
        ],
    );
    assert_eq!(actual.get_theme_trie_element(), root);
}

#[test]
fn custom_colors_are_first_in_color_map() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    let custom_colors: Vec<String> = ["000000", "FFFFFF", "0F0F0F"]
        .into_iter()
        .map(String::from)
        .collect();
    let actual = TokenTheme::create_from_parsed_token_theme(
        vec![ParsedTokenThemeRule::new(
            "var",
            -1,
            font_style::NOT_SET,
            Some("F8F8F2"),
            None,
        )],
        &custom_colors,
    )
    .unwrap();
    let mut color_map = ColorMap::new();
    id(&mut color_map, "000000");
    id(&mut color_map, "FFFFFF");
    id(&mut color_map, "0F0F0F");
    id(&mut color_map, "F8F8F2");
    assert_eq!(actual.get_color_map(), color_map.get_color_map());
}
