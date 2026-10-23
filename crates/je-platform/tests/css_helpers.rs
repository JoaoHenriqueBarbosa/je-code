//! Porte de `src/vs/platform/browserView/test/common/cssHelpers.test.ts` (suítes `collapseToShorthands` e
//! `formatAuthorStyles`), caso a caso e na mesma ordem.
//!
//! Divergências: o `Map<string, string>` de entrada é um `IndexMap` montado de uma lista de pares (a ordem
//! dos literais do original); os literais CDP viram os tipos de `css_helpers`, e `rule(..., origin)` com
//! `origin = 'regular'` por padrão é `rule(..., None)`. `ensureNoDisposablesAreLeakedInTestSuite` roda no
//! começo de cada teste, como o `setup` e o `teardown` da suíte (nada aqui é descartável).

mod common;

use common::ensure_no_disposables_are_leaked_in_test_suite;
use indexmap::IndexMap;
use je_platform::browser_view::css_helpers::{
    CssProperty, CssRule, CssStyle, InheritedStyleEntry, MatchedStyles, PseudoElementMatches, RuleMatch, Selector, SelectorList,
    collapse_to_shorthands, format_matched_styles,
};

/// Helper: build a Map from an object literal and run collapseToShorthands.
fn collapse(props: &[(&str, &str)]) -> Vec<String> {
    let mut entries: IndexMap<String, String> = props.iter().map(|(k, v)| ((*k).to_owned(), (*v).to_owned())).collect();
    collapse_to_shorthands(&mut entries)
}

fn lines(items: &[&str]) -> Vec<String> {
    items.iter().map(|i| (*i).to_owned()).collect()
}

// ── Box shorthands ──

#[test]
fn margin_all_sides_equal_1_value() {
    let _leaks = ensure_no_disposables_are_leaked_in_test_suite();
    assert_eq!(
        collapse(&[("margin-top", "10px"), ("margin-right", "10px"), ("margin-bottom", "10px"), ("margin-left", "10px")]),
        lines(&["margin: 10px;"])
    );
}

#[test]
fn padding_vertical_horizontal_2_value() {
    let _leaks = ensure_no_disposables_are_leaked_in_test_suite();
    assert_eq!(
        collapse(&[("padding-top", "4px"), ("padding-right", "12px"), ("padding-bottom", "4px"), ("padding-left", "12px")]),
        lines(&["padding: 4px 12px;"])
    );
}

#[test]
fn margin_3_value_when_left_equals_right() {
    let _leaks = ensure_no_disposables_are_leaked_in_test_suite();
    assert_eq!(
        collapse(&[("margin-top", "10px"), ("margin-right", "5px"), ("margin-bottom", "20px"), ("margin-left", "5px")]),
        lines(&["margin: 10px 5px 20px;"])
    );
}

#[test]
fn margin_4_value_when_all_differ() {
    let _leaks = ensure_no_disposables_are_leaked_in_test_suite();
    assert_eq!(
        collapse(&[("margin-top", "1px"), ("margin-right", "2px"), ("margin-bottom", "3px"), ("margin-left", "4px")]),
        lines(&["margin: 1px 2px 3px 4px;"])
    );
}

#[test]
fn border_radius_uniform() {
    let _leaks = ensure_no_disposables_are_leaked_in_test_suite();
    assert_eq!(
        collapse(&[
            ("border-top-left-radius", "6px"),
            ("border-top-right-radius", "6px"),
            ("border-bottom-right-radius", "6px"),
            ("border-bottom-left-radius", "6px"),
        ]),
        lines(&["border-radius: 6px;"])
    );
}

// ── Border ──

#[test]
fn border_uniform_sides_single_shorthand() {
    let _leaks = ensure_no_disposables_are_leaked_in_test_suite();
    assert_eq!(
        collapse(&[
            ("border-top-width", "1px"),
            ("border-right-width", "1px"),
            ("border-bottom-width", "1px"),
            ("border-left-width", "1px"),
            ("border-top-style", "solid"),
            ("border-right-style", "solid"),
            ("border-bottom-style", "solid"),
            ("border-left-style", "solid"),
            ("border-top-color", "red"),
            ("border-right-color", "red"),
            ("border-bottom-color", "red"),
            ("border-left-color", "red"),
        ]),
        lines(&["border: 1px solid red;"])
    );
}

#[test]
fn border_non_uniform_per_group_shorthands() {
    let _leaks = ensure_no_disposables_are_leaked_in_test_suite();
    let result = collapse(&[
        ("border-top-width", "1px"),
        ("border-right-width", "2px"),
        ("border-bottom-width", "1px"),
        ("border-left-width", "2px"),
        ("border-top-style", "solid"),
        ("border-right-style", "solid"),
        ("border-bottom-style", "solid"),
        ("border-left-style", "solid"),
        ("border-top-color", "red"),
        ("border-right-color", "red"),
        ("border-bottom-color", "red"),
        ("border-left-color", "red"),
    ]);
    assert_eq!(result, lines(&["border-width: 1px 2px;", "border-style: solid;", "border-color: red;"]));
}

// ── Drop-when-all-default ──

#[test]
fn border_image_at_defaults_dropped_entirely() {
    let _leaks = ensure_no_disposables_are_leaked_in_test_suite();
    assert_eq!(
        collapse(&[
            ("border-image-source", "none"),
            ("border-image-slice", "100%"),
            ("border-image-width", "1"),
            ("border-image-outset", "0"),
            ("border-image-repeat", "stretch"),
            ("color", "red"),
        ]),
        lines(&["color: red;"])
    );
}

#[test]
fn animation_range_at_defaults_dropped() {
    let _leaks = ensure_no_disposables_are_leaked_in_test_suite();
    assert_eq!(
        collapse(&[("animation-range-start", "normal"), ("animation-range-end", "normal"), ("display", "block")]),
        lines(&["display: block;"])
    );
}

// ── Background ──

#[test]
fn background_color_only_when_others_at_default() {
    let _leaks = ensure_no_disposables_are_leaked_in_test_suite();
    assert_eq!(
        collapse(&[
            ("background-color", "rgb(255, 0, 0)"),
            ("background-image", "none"),
            ("background-position-x", "0px"),
            ("background-position-y", "0px"),
            ("background-size", "auto"),
            ("background-repeat", "repeat"),
            ("background-attachment", "scroll"),
            ("background-origin", "padding-box"),
            ("background-clip", "border-box"),
        ]),
        lines(&["background: rgb(255, 0, 0);"])
    );
}

// ── Text-decoration ──

#[test]
fn text_decoration_none() {
    let _leaks = ensure_no_disposables_are_leaked_in_test_suite();
    assert_eq!(
        collapse(&[
            ("text-decoration-line", "none"),
            ("text-decoration-style", "solid"),
            ("text-decoration-color", "currentcolor"),
            ("text-decoration-thickness", "auto"),
        ]),
        lines(&["text-decoration: none;"])
    );
}

#[test]
fn text_decoration_underline_with_non_default_style() {
    let _leaks = ensure_no_disposables_are_leaked_in_test_suite();
    assert_eq!(
        collapse(&[
            ("text-decoration-line", "underline"),
            ("text-decoration-style", "wavy"),
            ("text-decoration-color", "currentcolor"),
            ("text-decoration-thickness", "auto"),
        ]),
        lines(&["text-decoration: underline wavy;"])
    );
}

// ── White-space ──

#[test]
fn white_space_nowrap() {
    let _leaks = ensure_no_disposables_are_leaked_in_test_suite();
    assert_eq!(collapse(&[("white-space-collapse", "collapse"), ("text-wrap-mode", "nowrap")]), lines(&["white-space: nowrap;"]));
}

#[test]
fn white_space_pre_wrap() {
    let _leaks = ensure_no_disposables_are_leaked_in_test_suite();
    assert_eq!(collapse(&[("white-space-collapse", "preserve"), ("text-wrap-mode", "wrap")]), lines(&["white-space: pre-wrap;"]));
}

// ── Transition ──

#[test]
fn transition_single_property_with_cubic_bezier() {
    let _leaks = ensure_no_disposables_are_leaked_in_test_suite();
    assert_eq!(
        collapse(&[
            ("transition-property", "opacity"),
            ("transition-duration", "0.5s"),
            ("transition-timing-function", "cubic-bezier(0.16, 1, 0.3, 1)"),
            ("transition-delay", "0s"),
            ("transition-behavior", "normal"),
        ]),
        lines(&["transition: opacity 0.5s cubic-bezier(0.16, 1, 0.3, 1);"])
    );
}

#[test]
fn transition_multi_property_comma_separated() {
    let _leaks = ensure_no_disposables_are_leaked_in_test_suite();
    assert_eq!(
        collapse(&[
            ("transition-property", "opacity, transform"),
            ("transition-duration", "0.5s, 0.3s"),
            ("transition-timing-function", "ease, ease"),
            ("transition-delay", "0s, 0s"),
            ("transition-behavior", "normal, normal"),
        ]),
        lines(&["transition: opacity 0.5s, transform 0.3s;"])
    );
}

// ── Animation ──

#[test]
fn animation_name_and_duration_only() {
    let _leaks = ensure_no_disposables_are_leaked_in_test_suite();
    assert_eq!(
        collapse(&[
            ("animation-name", "fadeIn"),
            ("animation-duration", "0.3s"),
            ("animation-timing-function", "ease"),
            ("animation-delay", "0s"),
            ("animation-iteration-count", "1"),
            ("animation-direction", "normal"),
            ("animation-fill-mode", "none"),
            ("animation-play-state", "running"),
            ("animation-timeline", "auto"),
        ]),
        lines(&["animation: fadeIn 0.3s;"])
    );
}

#[test]
fn animation_with_fill_mode_and_custom_easing() {
    let _leaks = ensure_no_disposables_are_leaked_in_test_suite();
    assert_eq!(
        collapse(&[
            ("animation-name", "slideIn"),
            ("animation-duration", "0.5s"),
            ("animation-timing-function", "ease-in-out"),
            ("animation-delay", "0s"),
            ("animation-iteration-count", "1"),
            ("animation-direction", "normal"),
            ("animation-fill-mode", "forwards"),
            ("animation-play-state", "running"),
            ("animation-timeline", "auto"),
        ]),
        lines(&["animation: slideIn 0.5s ease-in-out forwards;"])
    );
}

// ── Remaining properties pass through sorted ──

#[test]
fn unknown_properties_pass_through_alphabetically() {
    let _leaks = ensure_no_disposables_are_leaked_in_test_suite();
    assert_eq!(collapse(&[("z-index", "1"), ("color", "red"), ("display", "flex")]), lines(&["color: red;", "display: flex;", "z-index: 1;"]));
}

// ── Mixed: realistic GitHub-like element ──

#[test]
fn realistic_element_with_multiple_shorthand_groups() {
    let _leaks = ensure_no_disposables_are_leaked_in_test_suite();
    let result = collapse(&[
        ("padding-top", "4px"),
        ("padding-right", "12px"),
        ("padding-bottom", "4px"),
        ("padding-left", "12px"),
        ("border-top-left-radius", "6px"),
        ("border-top-right-radius", "6px"),
        ("border-bottom-right-radius", "6px"),
        ("border-bottom-left-radius", "6px"),
        ("border-top-width", "1px"),
        ("border-right-width", "1px"),
        ("border-bottom-width", "1px"),
        ("border-left-width", "1px"),
        ("border-top-style", "solid"),
        ("border-right-style", "solid"),
        ("border-bottom-style", "solid"),
        ("border-left-style", "solid"),
        ("border-top-color", "rgb(209, 217, 224)"),
        ("border-right-color", "rgb(209, 217, 224)"),
        ("border-bottom-color", "rgb(209, 217, 224)"),
        ("border-left-color", "rgb(209, 217, 224)"),
        ("border-image-source", "none"),
        ("border-image-slice", "100%"),
        ("border-image-width", "1"),
        ("border-image-outset", "0"),
        ("border-image-repeat", "stretch"),
        ("background-color", "rgba(0, 0, 0, 0)"),
        ("background-image", "none"),
        ("background-position-x", "0px"),
        ("background-position-y", "0px"),
        ("background-size", "auto"),
        ("background-repeat", "repeat"),
        ("background-attachment", "scroll"),
        ("background-origin", "padding-box"),
        ("background-clip", "border-box"),
        ("text-decoration-line", "none"),
        ("text-decoration-style", "solid"),
        ("text-decoration-color", "currentcolor"),
        ("text-decoration-thickness", "auto"),
        ("white-space-collapse", "collapse"),
        ("text-wrap-mode", "nowrap"),
        ("transition-property", "opacity, transform"),
        ("transition-duration", "0.5s, 0.5s"),
        ("transition-timing-function", "cubic-bezier(0.16, 1, 0.3, 1), cubic-bezier(0.16, 1, 0.3, 1)"),
        ("transition-delay", "0s, 0s"),
        ("transition-behavior", "normal, normal"),
        ("color", "rgb(255, 255, 255)"),
        ("display", "inline-flex"),
        ("font-size", "14px"),
    ]);
    assert_eq!(
        result,
        lines(&[
            "padding: 4px 12px;",
            "border-radius: 6px;",
            "border: 1px solid rgb(209, 217, 224);",
            "background: rgba(0, 0, 0, 0);",
            "text-decoration: none;",
            "white-space: nowrap;",
            "transition: opacity 0.5s cubic-bezier(0.16, 1, 0.3, 1), transform 0.5s cubic-bezier(0.16, 1, 0.3, 1);",
            "color: rgb(255, 255, 255);",
            "display: inline-flex;",
            "font-size: 14px;",
        ])
    );
}

// ── Helper to build CDP-like rule matches ──

fn rule(selector: &str, css_text: &str, origin: Option<&str>) -> RuleMatch {
    let props = css_text
        .split(';')
        .map(str::trim)
        .filter(|d| !d.is_empty())
        .map(|d| {
            let (name, rest) = d.split_once(':').unwrap_or((d, ""));
            CssProperty::new(name.trim(), rest.trim())
        })
        .collect();
    RuleMatch {
        rule: CssRule {
            selector_list: SelectorList { selectors: vec![Selector { text: selector.to_owned() }] },
            origin: origin.unwrap_or("regular").to_owned(),
            style: CssStyle { css_text: Some(css_text.to_owned()), css_properties: props },
        },
    }
}

fn pseudo(pseudo_type: &str, matches: Vec<RuleMatch>) -> PseudoElementMatches {
    PseudoElementMatches { pseudo_type: pseudo_type.to_owned(), matches }
}

#[test]
fn format_author_styles_includes_direct_author_rules_and_skips_user_agent() {
    let _leaks = ensure_no_disposables_are_leaked_in_test_suite();
    let matched = MatchedStyles {
        matched_css_rules: vec![rule(".btn", "padding: 8px; color: white;", None), rule("button", "display: inline-block;", Some("user-agent"))],
        ..Default::default()
    };
    let rules_text = format_matched_styles(&matched).rules_text;
    assert!(rules_text.contains(".btn"));
    assert!(rules_text.contains("padding: 8px"));
    assert!(!rules_text.contains("display: inline-block"));
}

#[test]
fn format_author_styles_includes_pseudo_element_styles() {
    let _leaks = ensure_no_disposables_are_leaked_in_test_suite();
    let matched = MatchedStyles {
        matched_css_rules: vec![rule(".btn", "color: white;", None)],
        pseudo_elements: vec![
            pseudo("before", vec![rule(".btn::before", "content: \"→\"; color: red;", None)]),
            pseudo("after", vec![rule(".btn::after", "content: \"✓\"; color: green;", None)]),
        ],
        ..Default::default()
    };
    let rules_text = format_matched_styles(&matched).rules_text;
    assert!(rules_text.contains("/* Pseudo-elements */"));
    assert!(rules_text.contains(".btn::before"));
    assert!(rules_text.contains(".btn::after"));
    assert!(rules_text.contains("content: \"→\""));
}

#[test]
fn format_author_styles_skips_user_agent_pseudo_element_rules() {
    let _leaks = ensure_no_disposables_are_leaked_in_test_suite();
    let matched = MatchedStyles {
        matched_css_rules: vec![rule(".x", "color: red;", None)],
        pseudo_elements: vec![pseudo("before", vec![rule("input::before", "content: \"\";", Some("user-agent"))])],
        ..Default::default()
    };
    let rules_text = format_matched_styles(&matched).rules_text;
    assert!(!rules_text.contains("Pseudo-elements"));
}

#[test]
fn format_author_styles_filters_inherited_rules_to_inheritable_properties_only() {
    let _leaks = ensure_no_disposables_are_leaked_in_test_suite();
    let matched = MatchedStyles {
        matched_css_rules: vec![rule(".child", "display: flex;", None)],
        inherited: vec![InheritedStyleEntry {
            matched_css_rules: vec![rule("body", "font-family: sans-serif; background: red; margin: 0;", None)],
            ..Default::default()
        }],
        ..Default::default()
    };
    let rules_text = format_matched_styles(&matched).rules_text;
    assert!(rules_text.contains("font-family: sans-serif"));
    assert!(!rules_text.contains("background"));
    assert!(!rules_text.contains("margin"));
}

#[test]
fn format_author_styles_collects_var_references_from_rules() {
    let _leaks = ensure_no_disposables_are_leaked_in_test_suite();
    let matched = MatchedStyles {
        matched_css_rules: vec![rule(".x", "color: var(--fg-color); border: var(--border-width) solid;", None)],
        ..Default::default()
    };
    let referenced_vars = format_matched_styles(&matched).referenced_vars;
    assert!(referenced_vars.contains("--fg-color"));
    assert!(referenced_vars.contains("--border-width"));
}

#[test]
fn format_author_styles_tracks_author_property_names_from_css_properties_longhands() {
    let _leaks = ensure_no_disposables_are_leaked_in_test_suite();
    let matched = MatchedStyles {
        matched_css_rules: vec![RuleMatch {
            rule: CssRule {
                selector_list: SelectorList { selectors: vec![Selector { text: ".x".to_owned() }] },
                origin: "regular".to_owned(),
                style: CssStyle {
                    css_text: Some("border: 1px solid red;".to_owned()),
                    css_properties: vec![
                        CssProperty::new("border-top-width", "1px"),
                        CssProperty::new("border-top-style", "solid"),
                        CssProperty::new("border-top-color", "red"),
                    ],
                },
            },
        }],
        ..Default::default()
    };
    let author_property_names = format_matched_styles(&matched).author_property_names;
    assert!(author_property_names.contains("border-top-width"));
    assert!(author_property_names.contains("border-top-style"));
    // Always-shown properties
    assert!(author_property_names.contains("display"));
    assert!(author_property_names.contains("width"));
}

#[test]
fn format_author_styles_tracks_user_agent_property_names_from_direct_rules() {
    let _leaks = ensure_no_disposables_are_leaked_in_test_suite();
    let matched = MatchedStyles {
        matched_css_rules: vec![rule(".btn", "color: white;", None), rule("button", "display: inline-block; padding: 2px;", Some("user-agent"))],
        ..Default::default()
    };
    let user_agent_property_names = format_matched_styles(&matched).user_agent_property_names;
    assert!(user_agent_property_names.contains("display"));
    assert!(user_agent_property_names.contains("padding"));
    assert!(!user_agent_property_names.contains("color"));
}

#[test]
fn format_author_styles_tracks_user_agent_property_names_from_pseudo_element_rules() {
    let _leaks = ensure_no_disposables_are_leaked_in_test_suite();
    let matched = MatchedStyles {
        matched_css_rules: vec![rule(".x", "color: red;", None)],
        pseudo_elements: vec![pseudo("before", vec![rule("input::before", "content: \"\"; display: block;", Some("user-agent"))])],
        ..Default::default()
    };
    let user_agent_property_names = format_matched_styles(&matched).user_agent_property_names;
    assert!(user_agent_property_names.contains("content"));
    assert!(user_agent_property_names.contains("display"));
}

#[test]
fn format_author_styles_tracks_user_agent_property_names_from_inherited_rules_inheritable_only() {
    let _leaks = ensure_no_disposables_are_leaked_in_test_suite();
    let matched = MatchedStyles {
        matched_css_rules: vec![rule(".child", "display: flex;", None)],
        inherited: vec![InheritedStyleEntry {
            matched_css_rules: vec![rule("body", "font-family: sans-serif; margin: 0;", Some("user-agent"))],
            ..Default::default()
        }],
        ..Default::default()
    };
    let user_agent_property_names = format_matched_styles(&matched).user_agent_property_names;
    assert!(user_agent_property_names.contains("font-family"));
    assert!(!user_agent_property_names.contains("margin"));
}
