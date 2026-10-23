//! Porte de `src/vs/base/common/search.ts`: substituição que preserva a caixa do texto casado.

use crate::js_string::{JsString, substring};
use crate::strings::contains_uppercase_character;

const HYPHEN: u16 = b'-' as u16;
const UNDERSCORE: u16 = b'_' as u16;

fn split(text: &[u16], separator: u16) -> Vec<&[u16]> {
    text.split(|&u| u == separator).collect()
}

/// `str[0].toUpperCase() + str.substr(1)` (ou a versão em minúsculas).
fn map_first(text: &[u16], upper: bool) -> JsString {
    let first = JsString::from_units(vec![text[0]]);
    let mut out = if upper { first.to_upper_case() } else { first.to_lower_case() };
    out.push_units(substring(text, 1, text.len() as i64));
    out
}

/// `buildReplaceStringWithCasePreserved`: `matches` é o resultado de um `RegExp.exec` (o texto casado em
/// `matches[0]`), ou `None` quando nada casou.
pub fn build_replace_string_with_case_preserved(matches: Option<&[JsString]>, pattern: &JsString) -> JsString {
    let Some(matches) = matches.filter(|m| !m[0].is_empty()) else {
        return pattern.clone();
    };
    let matched = &matches[0];
    let contains_hyphens = validate_specific_special_character(matches, pattern, HYPHEN);
    let contains_underscores = validate_specific_special_character(matches, pattern, UNDERSCORE);
    if contains_hyphens && !contains_underscores {
        return build_replace_string_for_specific_special_character(matches, pattern, HYPHEN);
    } else if !contains_hyphens && contains_underscores {
        return build_replace_string_for_specific_special_character(matches, pattern, UNDERSCORE);
    }
    if matched.to_upper_case() == *matched {
        pattern.to_upper_case()
    } else if matched.to_lower_case() == *matched {
        pattern.to_lower_case()
    } else if contains_uppercase_character(&matched.units()[..1], false) && !pattern.is_empty() {
        map_first(pattern.units(), true)
    } else if JsString::from_units(vec![matched.units()[0]]).to_upper_case().units() != &matched.units()[..1] && !pattern.is_empty() {
        map_first(pattern.units(), false)
    } else {
        // we don't understand its pattern yet.
        pattern.clone()
    }
}

fn validate_specific_special_character(matches: &[JsString], pattern: &JsString, special_character: u16) -> bool {
    let matched = matches[0].units();
    let does_contain_special_character = matched.contains(&special_character) && pattern.units().contains(&special_character);
    does_contain_special_character && split(matched, special_character).len() == split(pattern.units(), special_character).len()
}

fn build_replace_string_for_specific_special_character(matches: &[JsString], pattern: &JsString, special_character: u16) -> JsString {
    let split_pattern = split(pattern.units(), special_character);
    let split_match = split(matches[0].units(), special_character);
    let mut replace_string = JsString::new();
    for (index, split_value) in split_pattern.iter().enumerate() {
        let part = [JsString::from_units(split_match[index].to_vec())];
        replace_string.push_units(build_replace_string_with_case_preserved(Some(&part), &JsString::from(*split_value)).units());
        replace_string.push_unit(special_character);
    }
    let units = replace_string.units();
    JsString::from_units(units[..units.len() - 1].to_vec())
}
