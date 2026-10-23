//! Porte de `src/vs/platform/browserView/common/cssHelpers.ts`: formata os estilos casados do CDP
//! (`CSS.getMatchedStylesForNode`) num texto compacto para o prompt do agente e recompõe os valores
//! computados em abreviações (`margin`, `border`, `transition`...).
//!
//! Divergências: `Set<string>` é [`IndexSet`] e `Map<string, string>` é [`IndexMap`] (a ordem de inserção
//! do original); os campos opcionais do CDP viram `Option`, e as listas opcionais (`matchedCSSRules?`,
//! `inherited?`, `pseudoElements?`), cujo único uso é `?? []`, viram `Vec` vazio. `String.prototype.trim`
//! usa o conjunto de espaços do JavaScript ([`js_trim`]), `matchAll` do `var(--nome)` é a varredura
//! manual de [`collect_var_references`] e o `localeCompare` do final é o colador do `je-base`.

use indexmap::{IndexMap, IndexSet};
use je_base::collator::locale_compare;
use je_base::js_string::is_js_whitespace;

// -- Tipos do CDP (o subconjunto que `formatMatchedStyles` usa) --

/// Uma declaração de `cssProperties`.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct CssProperty {
    pub name: String,
    pub value: String,
    pub disabled: bool,
}

impl CssProperty {
    /// Uma declaração habilitada (`{ name, value }`).
    pub fn new(name: &str, value: &str) -> Self {
        Self { name: name.to_owned(), value: value.to_owned(), disabled: false }
    }
}

/// `ICSSStyle`.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct CssStyle {
    pub css_text: Option<String>,
    pub css_properties: Vec<CssProperty>,
}

/// `{ text }` de um seletor.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Selector {
    pub text: String,
}

/// `ISelectorList`.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct SelectorList {
    pub selectors: Vec<Selector>,
}

/// `ICSSRule`.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct CssRule {
    pub selector_list: SelectorList,
    pub origin: String,
    pub style: CssStyle,
}

/// `IRuleMatch`.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct RuleMatch {
    pub rule: CssRule,
}

/// `IInheritedStyleEntry`.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct InheritedStyleEntry {
    pub inline_style: Option<CssStyle>,
    pub matched_css_rules: Vec<RuleMatch>,
}

/// `IPseudoElementMatches`.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct PseudoElementMatches {
    pub pseudo_type: String,
    pub matches: Vec<RuleMatch>,
}

/// `IMatchedStyles`.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct MatchedStyles {
    pub inline_style: Option<CssStyle>,
    pub matched_css_rules: Vec<RuleMatch>,
    pub inherited: Vec<InheritedStyleEntry>,
    pub pseudo_elements: Vec<PseudoElementMatches>,
}

/// `IFormattedStyles`.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct FormattedStyles {
    /// Compact CSS text for the agent prompt (rules only, without resolved values).
    pub rules_text: String,
    /// Set of CSS variable names referenced by the element's rules.
    pub referenced_vars: IndexSet<String>,
    /// Set of CSS property names that were explicitly set by author rules.
    pub author_property_names: IndexSet<String>,
    /// Set of CSS property names that were set by user-agent rules.
    pub user_agent_property_names: IndexSet<String>,
}

// -- Constantes --

/// CSS properties that are inherited by child elements.
const INHERITABLE_CSS_PROPERTIES: &[&str] = &[
    "color", "cursor", "direction", "font", "font-family", "font-feature-settings",
    "font-kerning", "font-size", "font-size-adjust", "font-stretch", "font-style",
    "font-variant", "font-weight", "letter-spacing", "line-height", "list-style",
    "list-style-image", "list-style-position", "list-style-type", "orphans",
    "overflow-wrap", "quotes", "tab-size", "text-align", "text-align-last",
    "text-indent", "text-transform", "visibility", "white-space", "widows",
    "word-break", "word-spacing", "writing-mode",
];

/// Key computed properties included for hover display in the UI.
pub const KEY_COMPUTED_PROPERTIES: &[&str] = &[
    "display", "position", "margin", "margin-top", "margin-right", "margin-bottom", "margin-left",
    "padding", "padding-top", "padding-right", "padding-bottom", "padding-left",
    "font-size", "font-family", "color", "background-color",
];

/// Properties always included in resolved values even if only set by user-agent rules,
/// matching Chrome DevTools' `alwaysShownComputedProperties`.
const ALWAYS_RESOLVED_PROPERTIES: &[&str] = &["display", "height", "width"];

// -- Funções auxiliares --

/// `String.prototype.trim`: o conjunto de espaços do JavaScript (inclui U+FEFF, não inclui U+0085).
fn js_trim(text: &str) -> &str {
    text.trim_matches(is_js_space)
}

fn is_js_space(c: char) -> bool {
    u16::try_from(u32::from(c)).is_ok_and(is_js_whitespace)
}

/// Collects var(--name) references from a CSS value string: o `matchAll` de
/// `/var\(\s*(--[a-zA-Z0-9_-]+)/g`. Uma tentativa que falha recomeça um caractere depois; uma que casa
/// recomeça no fim do casamento.
fn collect_var_references(value: &str, into: &mut IndexSet<String>) {
    let is_name_char = |c: char| c.is_ascii_alphanumeric() || c == '_' || c == '-';
    let mut from = 0;
    while let Some(found) = value[from..].find("var(") {
        let start = from + found;
        let after_paren = &value[start + 4..];
        let name = after_paren.trim_start_matches(is_js_space);
        let matched = name.strip_prefix("--").map(|rest| rest.len() - rest.trim_start_matches(is_name_char).len()).filter(|len| *len > 0);
        match matched {
            Some(len) => {
                into.insert(name[..2 + len].to_owned());
                from = value.len() - name.len() + 2 + len;
            }
            None => from = start + 1,
        }
    }
}

/// Collects longhand property names from the `cssProperties` array of a matched rule.
/// Skips variable definitions and disabled properties.
fn collect_property_names(css_properties: &[CssProperty], into: &mut IndexSet<String>, inheritable_only: bool) {
    for prop in css_properties {
        if prop.name.is_empty() || prop.value.is_empty() || prop.disabled || prop.name.starts_with("--") {
            continue;
        }
        if inheritable_only && !INHERITABLE_CSS_PROPERTIES.contains(&prop.name.as_str()) {
            continue;
        }
        into.insert(prop.name.clone());
    }
}

/// Filters CSS declarations to only inheritable properties (not variable definitions).
pub fn filter_inheritable_declarations(css_text: &str) -> Option<String> {
    let filtered: Vec<&str> = css_text
        .split(';')
        .map(js_trim)
        .filter(|d| !d.is_empty())
        .filter(|decl| decl.find(':').is_some_and(|colon_idx| INHERITABLE_CSS_PROPERTIES.contains(&js_trim(&decl[..colon_idx]))))
        .collect();
    if filtered.is_empty() { None } else { Some(filtered.join("; ")) }
}

fn selectors_text(rule: &CssRule) -> String {
    rule.selector_list.selectors.iter().map(|s| s.text.as_str()).collect::<Vec<_>>().join(", ")
}

/// O estado que `formatMatchedStyles` acumula: os conjuntos do resultado e os `cssText` já vistos.
#[derive(Default)]
struct Tracker {
    referenced_vars: IndexSet<String>,
    author_property_names: IndexSet<String>,
    user_agent_property_names: IndexSet<String>,
    seen_css_texts: IndexSet<String>,
}

impl Tracker {
    /// Uma regra direta ou de pseudo-elemento: usa o `cssText` para exibir e `cssProperties` para
    /// rastrear as propriedades; regra de user-agent só rastreia.
    fn add_rule(&mut self, rule: &CssRule, lines: &mut Vec<String>) {
        if rule.origin == "user-agent" {
            collect_property_names(&rule.style.css_properties, &mut self.user_agent_property_names, false);
            return;
        }
        let Some(css_text) = rule.style.css_text.as_deref().map(js_trim).filter(|t| !t.is_empty()) else {
            return;
        };
        if self.seen_css_texts.contains(css_text) {
            return;
        }
        self.seen_css_texts.insert(css_text.to_owned());
        collect_var_references(css_text, &mut self.referenced_vars);
        collect_property_names(&rule.style.css_properties, &mut self.author_property_names, false);
        lines.push(format!("{} {{ {} }}", selectors_text(rule), css_text));
    }

    /// Uma regra herdada: só as propriedades herdáveis, tanto no texto quanto no rastreio.
    fn add_inherited_rule(&mut self, rule: &CssRule, lines: &mut Vec<String>) {
        if rule.origin == "user-agent" {
            collect_property_names(&rule.style.css_properties, &mut self.user_agent_property_names, true);
            return;
        }
        let Some(css_text) = rule.style.css_text.as_deref().map(js_trim).filter(|t| !t.is_empty()) else {
            return;
        };
        // Display: keep only inheritable properties from cssText
        let Some(filtered) = filter_inheritable_declarations(css_text) else {
            return;
        };
        if self.seen_css_texts.contains(&filtered) {
            return;
        }
        // Track: use cssProperties longhands, inheritable only
        collect_var_references(&filtered, &mut self.referenced_vars);
        collect_property_names(&rule.style.css_properties, &mut self.author_property_names, true);
        lines.push(format!("{} {{ {} }}", selectors_text(rule), filtered));
        self.seen_css_texts.insert(filtered);
    }
}

/// Formats matched styles into a compact representation for agent prompts.
///
/// Only includes author-origin rules (not browser defaults), uses the raw
/// `cssText` instead of expanded longhand properties, and for inherited
/// rules only keeps inheritable CSS properties.
///
/// Also includes pseudo-element styles (::before, ::after, etc.) when present.
///
/// Uses `cssProperties` (the longhand array) from matched rules to determine
/// which computed properties are author-affected, matching Chrome DevTools'
/// `computePropertyTraces` approach.
pub fn format_matched_styles(matched: &MatchedStyles) -> FormattedStyles {
    let mut tracker = Tracker::default();
    let mut lines: Vec<String> = Vec::new();

    // Inline styles on the element itself
    if let Some(inline_style) = &matched.inline_style
        && let Some(css_text) = inline_style.css_text.as_deref().map(js_trim).filter(|t| !t.is_empty())
    {
        collect_var_references(css_text, &mut tracker.referenced_vars);
        collect_property_names(&inline_style.css_properties, &mut tracker.author_property_names, false);
        lines.push(format!("element {{ {css_text} }}"));
    }

    // Direct author rules: use cssText for display, cssProperties for property tracking
    for rule_entry in &matched.matched_css_rules {
        tracker.add_rule(&rule_entry.rule, &mut lines);
    }

    // Pseudo-element styles (::before, ::after, etc.)
    if !matched.pseudo_elements.is_empty() {
        let mut pseudo_lines: Vec<String> = Vec::new();
        for pseudo in &matched.pseudo_elements {
            for rule_entry in &pseudo.matches {
                tracker.add_rule(&rule_entry.rule, &mut pseudo_lines);
            }
        }
        if !pseudo_lines.is_empty() {
            lines.push(String::new());
            lines.push("/* Pseudo-elements */".to_owned());
            lines.extend(pseudo_lines);
        }
    }

    // Inherited author rules, only inheritable properties
    let mut inherited_lines: Vec<String> = Vec::new();
    for entry in &matched.inherited {
        for rule_entry in &entry.matched_css_rules {
            tracker.add_inherited_rule(&rule_entry.rule, &mut inherited_lines);
        }
    }

    if !inherited_lines.is_empty() {
        lines.push(String::new());
        lines.push("/* Inherited */".to_owned());
        lines.extend(inherited_lines);
    }

    // Always include DevTools' alwaysShownComputedProperties
    for prop in ALWAYS_RESOLVED_PROPERTIES {
        tracker.author_property_names.insert((*prop).to_owned());
    }

    FormattedStyles {
        rules_text: lines.join("\n"),
        referenced_vars: tracker.referenced_vars,
        author_property_names: tracker.author_property_names,
        user_agent_property_names: tracker.user_agent_property_names,
    }
}

// -- Shorthand collapsing configuration --
//
// Each constant below describes one kind of CSS shorthand that can be reconstituted from computed
// longhand values. `collapse_to_shorthands` walks these lists in declaration order and produces compact
// output for the agent prompt.
//
// Sources:
//  * MDN "Formal definition -> Initial value" tables
//  * CSS Backgrounds & Borders 3, CSS Transitions 1, CSS Animations 1,
//    CSS Text Decoration 4, CSS Text 4

/// Um shorthand de caixa: `(shorthand, [top/TL, right/TR, bottom/BR, left/BL])`.
type BoxShorthand = (&'static str, [&'static str; 4]);

/// Um longhand com o seu valor inicial: `(nome, inicial)`.
type Longhand = (&'static str, &'static str);

// -- Box model (T R B L) shorthands --
// Collapsed with 1-4-value syntax per CSS spec section 8.3.

const BOX_SHORTHANDS: [BoxShorthand; 3] = [
    // margin: <margin-top> <margin-right> <margin-bottom> <margin-left>
    ("margin", ["margin-top", "margin-right", "margin-bottom", "margin-left"]),
    // padding: <padding-top> <padding-right> <padding-bottom> <padding-left>
    ("padding", ["padding-top", "padding-right", "padding-bottom", "padding-left"]),
    // border-radius: <TL> <TR> <BR> <BL>   (clockwise from top-left)
    ("border-radius", ["border-top-left-radius", "border-top-right-radius", "border-bottom-right-radius", "border-bottom-left-radius"]),
];

// -- Border per-side groups (collapse to border: W S C when uniform) --

const BORDER_SIDE_GROUPS: [BoxShorthand; 3] = [
    // border-width: initial medium per MDN (but computed is always an absolute length)
    ("border-width", ["border-top-width", "border-right-width", "border-bottom-width", "border-left-width"]),
    // border-style: initial none per MDN
    ("border-style", ["border-top-style", "border-right-style", "border-bottom-style", "border-left-style"]),
    // border-color: initial currentcolor per MDN
    ("border-color", ["border-top-color", "border-right-color", "border-bottom-color", "border-left-color"]),
];

// -- Longhands that are dropped entirely when all at their initial values --

const DROP_WHEN_ALL_DEFAULT: [&[Longhand]; 2] = [
    // border-image  (CSS Backgrounds & Borders 3 section 6.8)
    &[
        ("border-image-source", "none"),
        ("border-image-slice", "100%"),
        ("border-image-width", "1"),
        ("border-image-outset", "0"),
        ("border-image-repeat", "stretch"),
    ],
    // animation-range  (CSS Scroll-driven Animations section 5.2)  initial: normal
    &[("animation-range-start", "normal"), ("animation-range-end", "normal")],
];

// -- Background collapse (color-only shorthand when images/position/etc. default) --

/// background-color longhand.
const BACKGROUND_COLOR_LONGHAND: &str = "background-color";

/// Other background longhands that must all be at their initial value.
const BACKGROUND_OTHER_LONGHANDS: &[Longhand] = &[
    // MDN background formal definition initial values:
    ("background-image", "none"),         // initial: none
    ("background-position-x", "0px"),     // initial: 0% (computed as 0px)
    ("background-position-y", "0px"),     // initial: 0%
    ("background-size", "auto"),          // initial: auto auto
    ("background-repeat", "repeat"),      // initial: repeat
    ("background-attachment", "scroll"),  // initial: scroll
    ("background-origin", "padding-box"), // initial: padding-box
    ("background-clip", "border-box"),    // initial: border-box
];

// -- Simple shorthand collapse (longhands -> single shorthand, omit defaults) --

const SIMPLE_SHORTHANDS: [(&str, &[Longhand]); 1] = [
    // text-decoration (CSS Text Decoration 4 section 3)
    // Constituents: text-decoration-line || text-decoration-style || text-decoration-color || text-decoration-thickness
    (
        "text-decoration",
        &[
            ("text-decoration-line", "none"),
            ("text-decoration-style", "solid"),
            ("text-decoration-color", "currentcolor"),
            ("text-decoration-thickness", "auto"),
        ],
    ),
];

// -- white-space (CSS Text 4 section 3) --
// Shorthand for white-space-collapse || text-wrap-mode.
// Named keyword mappings for the well-known combinations: `(collapse, wrap, keyword)`.

const WHITE_SPACE_KEYWORDS: [(&str, &str, &str); 6] = [
    ("collapse", "wrap", "normal"),
    ("collapse", "nowrap", "nowrap"),
    ("preserve", "nowrap", "pre"),
    ("preserve", "wrap", "pre-wrap"),
    ("preserve-breaks", "wrap", "pre-line"),
    ("break-spaces", "wrap", "break-spaces"),
];

// -- Comma-separated list shorthands (transition, animation) --

const LIST_SHORTHANDS: [(&str, &[Longhand]); 2] = [
    // transition (CSS Transitions 1 section 2.1)
    // Constituents: transition-property || transition-duration || transition-timing-function || transition-delay || transition-behavior
    (
        "transition",
        &[
            ("transition-property", "all"),
            ("transition-duration", "0s"),
            ("transition-timing-function", "ease"),
            ("transition-delay", "0s"),
            ("transition-behavior", "normal"),
        ],
    ),
    // animation (CSS Animations 1 section 3 + Scroll-driven Animations section 5)
    // Constituents: animation-name || animation-duration || animation-timing-function || animation-delay
    //             || animation-iteration-count || animation-direction || animation-fill-mode
    //             || animation-play-state || animation-timeline
    (
        "animation",
        &[
            ("animation-name", "none"),
            ("animation-duration", "0s"),
            ("animation-timing-function", "ease"),
            ("animation-delay", "0s"),
            ("animation-iteration-count", "1"),
            ("animation-direction", "normal"),
            ("animation-fill-mode", "none"),
            ("animation-play-state", "running"),
            ("animation-timeline", "auto"),
        ],
    ),
];

// -- Funções auxiliares --

/// Tries to collapse a box shorthand (4 sides -> 1-4 value shorthand).
/// Returns the collapsed value or None if not all sides are present.
fn collapse_box_values(entries: &mut IndexMap<String, String>, sides: &[&str; 4]) -> Option<String> {
    let [top_key, right_key, bottom_key, left_key] = sides;
    let top = entries.get(*top_key)?.clone();
    let right = entries.get(*right_key)?.clone();
    let bottom = entries.get(*bottom_key)?.clone();
    let left = entries.get(*left_key)?.clone();

    for key in sides {
        entries.shift_remove(*key);
    }

    if top == right && right == bottom && bottom == left {
        return Some(top);
    }
    if top == bottom && right == left {
        return Some(format!("{top} {right}"));
    }
    if right == left {
        return Some(format!("{top} {right} {bottom}"));
    }
    Some(format!("{top} {right} {bottom} {left}"))
}

/// Splits a CSS value by top-level commas, respecting parenthesized groups
/// like `cubic-bezier(0.16, 1, 0.3, 1)`.
fn split_css_list(value: &str) -> Vec<String> {
    let mut items = Vec::new();
    let mut depth: i32 = 0;
    let mut start = 0;
    for (i, ch) in value.char_indices() {
        match ch {
            '(' => depth += 1,
            ')' => depth -= 1,
            ',' if depth == 0 => {
                items.push(js_trim(&value[start..i]).to_owned());
                start = i + 1;
            }
            _ => {}
        }
    }
    items.push(js_trim(&value[start..]).to_owned());
    items
}

/// Collapses comma-separated list longhands into a single shorthand declaration.
fn collapse_list_shorthand(entries: &mut IndexMap<String, String>, output: &mut Vec<String>, shorthand: &str, longhands: &[Longhand]) {
    let Some(values) = longhands.iter().map(|(name, _)| entries.get(*name)).collect::<Option<Vec<_>>>() else {
        return;
    };

    let lists: Vec<Vec<String>> = values.into_iter().map(|v| split_css_list(v)).collect();
    let item_count = lists[0].len();
    if !lists.iter().all(|l| l.len() == item_count) {
        return;
    }

    for (name, _) in longhands {
        entries.shift_remove(*name);
    }

    let items: Vec<String> = (0..item_count)
        .map(|i| {
            let parts: Vec<&str> = longhands.iter().zip(&lists).map(|((_, initial), list)| (initial, &list[i])).filter(|(initial, val)| val.as_str() != **initial).map(|(_, val)| val.as_str()).collect();
            if parts.is_empty() { longhands[0].1.to_owned() } else { parts.join(" ") }
        })
        .collect();

    output.push(format!("{shorthand}: {};", items.join(", ")));
}

// -- Main entry point --

/// Collapses resolved computed properties into shorthands where possible,
/// then returns sorted CSS declaration lines. Driven entirely by the
/// constant shorthand configuration tables above.
pub fn collapse_to_shorthands(entries: &mut IndexMap<String, String>) -> Vec<String> {
    let mut shorthand_lines: Vec<String> = Vec::new();

    // 1. Box shorthands (margin, padding, border-radius)
    for (shorthand, sides) in &BOX_SHORTHANDS {
        if let Some(collapsed) = collapse_box_values(entries, sides) {
            shorthand_lines.push(format!("{shorthand}: {collapsed};"));
        }
    }

    // 2. Border: try full `border: W S C` when all four sides are uniform,
    //    otherwise collapse each group (border-width, border-style, border-color).
    let border_vals: Vec<Vec<Option<String>>> = BORDER_SIDE_GROUPS.iter().map(|(_, sides)| sides.iter().map(|s| entries.get(*s).cloned()).collect()).collect();
    let has_all_border_props = border_vals.iter().all(|group| group.iter().all(Option::is_some));
    if has_all_border_props {
        let all_uniform = border_vals.iter().all(|group| group.iter().all(|v| *v == group[0]));
        if all_uniform {
            for (_, sides) in &BORDER_SIDE_GROUPS {
                for side in sides {
                    entries.shift_remove(*side);
                }
            }
            let [width, style, color] = [0, 1, 2].map(|g| border_vals[g][0].clone().unwrap_or_default());
            shorthand_lines.push(format!("border: {width} {style} {color};"));
        } else {
            for (shorthand, sides) in &BORDER_SIDE_GROUPS {
                if let Some(collapsed) = collapse_box_values(entries, sides) {
                    shorthand_lines.push(format!("{shorthand}: {collapsed};"));
                }
            }
        }
    }

    // 3. Drop-when-all-default groups (border-image, etc.)
    for longhands in &DROP_WHEN_ALL_DEFAULT {
        let all_default = longhands.iter().all(|(k, v)| entries.get(*k).is_some_and(|e| e == *v));
        if all_default && longhands.iter().any(|(k, _)| entries.contains_key(*k)) {
            for (key, _) in *longhands {
                entries.shift_remove(*key);
            }
        }
    }

    // 4. Background collapse (-> `background: <color>` when other props at default)
    {
        let bg_color = entries.get(BACKGROUND_COLOR_LONGHAND).cloned();
        let all_others_default = BACKGROUND_OTHER_LONGHANDS.iter().all(|(k, v)| entries.get(*k).is_some_and(|e| e == *v));
        if let Some(bg_color) = bg_color
            && all_others_default
        {
            entries.shift_remove(BACKGROUND_COLOR_LONGHAND);
            for (key, _) in BACKGROUND_OTHER_LONGHANDS {
                entries.shift_remove(*key);
            }
            shorthand_lines.push(format!("background: {bg_color};"));
        }
    }

    // 5. Simple shorthands (text-decoration, etc.): combine longhands, omit defaults
    for (shorthand, longhands) in &SIMPLE_SHORTHANDS {
        if !entries.contains_key(longhands[0].0) {
            continue;
        }
        // Snapshot values before deleting
        let values: Vec<Option<String>> = longhands.iter().map(|(name, _)| entries.get(*name).cloned()).collect();
        for (name, _) in *longhands {
            entries.shift_remove(*name);
        }
        // Build shorthand value, omitting longhands at their initial value
        let parts: Vec<String> = longhands
            .iter()
            .zip(values)
            .map(|((_, initial), value)| value.unwrap_or_else(|| (*initial).to_owned()))
            .zip(longhands.iter())
            .filter(|(value, (_, initial))| value.as_str() != *initial)
            .map(|(value, _)| value)
            .collect();
        let value = if parts.is_empty() { longhands[0].1.to_owned() } else { parts.join(" ") };
        shorthand_lines.push(format!("{shorthand}: {value};"));
    }

    // 6. white-space (CSS Text 4): map longhand pair to named keyword
    {
        let ws_collapse = entries.get("white-space-collapse").cloned();
        let text_wrap = entries.get("text-wrap-mode").cloned();
        if let (Some(ws_collapse), Some(text_wrap)) = (ws_collapse, text_wrap) {
            entries.shift_remove("white-space-collapse");
            entries.shift_remove("text-wrap-mode");
            let keyword = WHITE_SPACE_KEYWORDS
                .iter()
                .find(|(collapse, wrap, _)| *collapse == ws_collapse && *wrap == text_wrap)
                .map_or_else(|| format!("{ws_collapse} {text_wrap}"), |(_, _, keyword)| (*keyword).to_owned());
            shorthand_lines.push(format!("white-space: {keyword};"));
        }
    }

    // 7. Comma-separated list shorthands (transition, animation)
    for (shorthand, longhands) in &LIST_SHORTHANDS {
        collapse_list_shorthand(entries, &mut shorthand_lines, shorthand, longhands);
    }

    // 8. Remaining properties as individual lines, sorted
    let mut remaining: Vec<(&String, &String)> = entries.iter().collect();
    remaining.sort_by(|(a, _), (b, _)| locale_compare(a, b));
    shorthand_lines.extend(remaining.into_iter().map(|(name, value)| format!("{name}: {value};")));
    shorthand_lines
}
