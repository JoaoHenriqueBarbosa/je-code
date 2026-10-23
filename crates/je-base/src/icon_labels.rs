//! Porte de `src/vs/base/common/iconLabels.ts`: rótulos com ícones (`$(iconId)xyz`), os escapes
//! (`escapeIcons`, `markdownEscapeEscapedIcons`), `stripIcons`, o rótulo de acessibilidade e o casamento
//! difuso que ignora os ícones.
//!
//! Texto é `&[u16]` (as posições de `IMatch` são unidades UTF-16, como em `filters`).

use std::cell::LazyCell;

use zyarr::RegExp;

use crate::filters::{Match, matches_fuzzy};
use crate::js_regexp::{match_all, replace_all};
use crate::js_string::{self, JsString};
use crate::strings::ltrim;
use crate::themables::{ICON_MODIFIER_EXPRESSION, ICON_NAME_CHARACTER, ICON_NAME_EXPRESSION};

const ICON_START_MARKER: &str = "$(";

/// `iconsRegex.source`: sem grupos de captura.
fn icons_source() -> String {
    format!("\\$\\({ICON_NAME_EXPRESSION}(?:{ICON_MODIFIER_EXPRESSION})?\\)")
}

thread_local! {
    static ESCAPE_ICONS_REGEX: LazyCell<RegExp> = LazyCell::new(|| RegExp::from_str(&format!("(\\\\)?{}", icons_source()), "").expect("regex válida"));
    static MARKDOWN_ESCAPED_ICONS_REGEX: LazyCell<RegExp> = LazyCell::new(|| RegExp::from_str(&format!("\\\\{}", icons_source()), "").expect("regex válida"));
    static STRIP_ICONS_REGEX: LazyCell<RegExp> = LazyCell::new(|| RegExp::from_str(&format!("(\\s)?(\\\\)?{}(\\s)?", icons_source()), "").expect("regex válida"));
    static ARIA_LABEL_REGEX: LazyCell<RegExp> = LazyCell::new(|| RegExp::from_str("\\$\\((.*?)\\)", "").expect("regex válida"));
    static PARSE_ICONS_REGEX: LazyCell<RegExp> = LazyCell::new(|| RegExp::from_str(&format!("\\$\\({ICON_NAME_CHARACTER}+\\)"), "").expect("regex válida"));
}

fn backslash_then(text: &[u16]) -> JsString {
    let mut out = JsString::from("\\");
    out.push_units(text);
    out
}

pub fn escape_icons(text: &[u16]) -> JsString {
    ESCAPE_ICONS_REGEX.with(|regex| {
        replace_all(regex, text, |captures| {
            let escaped = captures.group(1);
            // `escaped ? match : `\${match}``: o grupo opcional que casou é a barra, nunca vazio.
            if escaped.is_some() { JsString::from(captures.whole()) } else { backslash_then(captures.whole()) }
        })
    })
}

pub fn markdown_escape_escaped_icons(text: &[u16]) -> JsString {
    // Need to add an extra \ for escaping in markdown
    MARKDOWN_ESCAPED_ICONS_REGEX.with(|regex| replace_all(regex, text, |captures| backslash_then(captures.whole())))
}

/// Takes a label with icons (`$(iconId)xyz`)  and strips the icons out (`xyz`)
pub fn strip_icons(text: &[u16]) -> JsString {
    let marker: Vec<u16> = ICON_START_MARKER.encode_utf16().collect();
    if js_string::index_of(text, &marker, 0).is_none() {
        return JsString::from(text);
    }

    STRIP_ICONS_REGEX.with(|regex| {
        replace_all(regex, text, |captures| {
            if captures.group(2).is_some() {
                return JsString::from(captures.whole());
            }
            // `preWhitespace || postWhitespace || ''`
            let pre = captures.group(1).filter(|g| !g.is_empty());
            let post = captures.group(3).filter(|g| !g.is_empty());
            JsString::from(pre.or(post).unwrap_or_default())
        })
    })
}

/// Takes a label with icons (`$(iconId)xyz`), removes the icon syntax adds whitespace so that screen
/// readers can read the text better.
pub fn get_codicon_aria_label(text: Option<&[u16]>) -> JsString {
    let Some(text) = text.filter(|t| !t.is_empty()) else {
        return JsString::new();
    };

    let replaced = ARIA_LABEL_REGEX.with(|regex| {
        replace_all(regex, text, |captures| {
            let mut out = JsString::from(" ");
            out.push_units(captures.group(1).unwrap_or_default());
            out.push_units(&[u16::from(b' ')]);
            out
        })
    });
    JsString::from(js_string::trim(replaced.units()))
}

/// `IParsedLabelWithIcons`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParsedLabelWithIcons {
    pub text: JsString,
    pub icon_offsets: Option<Vec<usize>>,
}

/// Takes a label with icons (`abc $(iconId)xyz`) and returns the text (`abc xyz`) and the offsets of the
/// icons (`[3]`)
pub fn parse_label_with_icons(input: &[u16]) -> ParsedLabelWithIcons {
    let mut text = JsString::new();
    let mut icon_offsets = Vec::new();
    let mut icons_offset = 0;

    let matches = PARSE_ICONS_REGEX.with(|regex| match_all(regex, input));
    let mut pos = 0;
    for found in matches.iter().map(Some).chain(std::iter::once(None)) {
        let chars = &input[pos..found.map_or(input.len(), |m| m.start())];
        if !chars.is_empty() {
            text.push_units(chars);
            icon_offsets.extend(std::iter::repeat_n(icons_offset, chars.len()));
        }
        let Some(found) = found else { break };
        icons_offset += found.end() - found.start();
        pos = found.end();
    }

    ParsedLabelWithIcons { text, icon_offsets: Some(icon_offsets) }
}

pub fn matches_fuzzy_icon_aware(query: &[u16], target: &ParsedLabelWithIcons, enable_separate_substring_matching: bool) -> Option<Vec<Match>> {
    let ParsedLabelWithIcons { text, icon_offsets } = target;

    // Return early if there are no icon markers in the word to match against
    let Some(icon_offsets) = icon_offsets.as_ref().filter(|offsets| !offsets.is_empty()) else {
        return matches_fuzzy(query, text.units(), enable_separate_substring_matching);
    };

    // Trim the word to match against because it could have leading
    // whitespace now if the word started with an icon
    let word_to_match_against_without_icons_trimmed = ltrim(text.units(), &[u16::from(b' ')]);
    let leading_whitespace_offset = text.len() - word_to_match_against_without_icons_trimmed.len();

    // match on value without icon
    let mut matches = matches_fuzzy(query, word_to_match_against_without_icons_trimmed, enable_separate_substring_matching);

    // Map matches back to offsets with icon and trimming
    if let Some(matches) = &mut matches {
        for found in matches {
            let icon_offset = icon_offsets[found.start + leading_whitespace_offset] /* icon offsets at index */ + leading_whitespace_offset /* overall leading whitespace offset */;
            found.start += icon_offset;
            found.end += icon_offset;
        }
    }

    matches
}
