//! `String.prototype.toLocaleLowerCase` e `toLocaleUpperCase` (`toLocaleCase` de `StringPrototype.cpp`):
//! a escolha do locale com sensibilidade de caixa (`az`, `el`, `lt` e `tr`, as quatro que o banco
//! Unicode do ICU tem) e as regras de mapeamento do turco, azeri e grego.
//!
//! DIVERGÊNCIA: as regras do lituano (`lt`) usam uma tabela própria das classes de combinação do bloco
//! U+0300..U+036F (o porte não tem as propriedades do Unicode além do normalizador), então marcas
//! combinantes fora desse bloco não entram na regra dos pontos acima do `i`. O grego em maiúsculas tira
//! os tonos, os espíritos e o perispomeni das letras gregas e mantém o dialytika
//! (`ΐ` vira `Ϊ`, `ᾀ` vira `ΑΙ`); `greek_upper` é nova e ainda não foi compilada nem medida pelo golden.

use icu_normalizer::{ComposingNormalizerBorrowed, DecomposingNormalizerBorrowed};

use crate::runtime::host_call::Thrown;
use crate::runtime::intl_locale_data::{best_available_by, default_locale, parse_language_tag};
use crate::runtime::intl_support::{canonicalize_locale_list, map_utf16_segments};
use crate::runtime::js_global_object::JSGlobalObject;
use crate::runtime::js_value::JSValue;
use crate::wtf::text::wtf_string::String as WtfString;

/// Os locales com mapeamento de caixa próprio.
const CASING_LOCALES: [&str; 4] = ["az", "el", "lt", "tr"];

/// O locale de `toLocaleCase` (`None` é a raiz, o `"und"` do C++).
fn casing_locale(global_object: &JSGlobalObject, locales: JSValue) -> Result<Option<String>, Thrown> {
    let requested = canonicalize_locale_list(global_object, locales)?;
    let first = requested.first().map_or_else(default_locale, Clone::clone);
    // `removeUnicodeLocaleExtension`: a tag sem extensões.
    let base = parse_language_tag(&first).map_or(first, |tag| tag.base_name());
    Ok(best_available_by(&base, |candidate| candidate.len() == 2 && CASING_LOCALES.contains(&candidate)))
}

/// A classe de combinação 230 (marca acima) dentro do bloco Combining Diacritical Marks.
fn is_above_mark(c: char) -> bool {
    matches!(c as u32,
        0x300..=0x314 | 0x33d..=0x344 | 0x346 | 0x34a..=0x34c | 0x350..=0x352 | 0x357 | 0x35b | 0x363..=0x36f)
}

/// Marca combinante de classe diferente de 0 e de 230 (o `OTHER_ACCENT` do ICU: não interrompe a regra).
fn is_other_accent(c: char) -> bool {
    matches!(c as u32, 0x315..=0x32f | 0x330..=0x33c | 0x345 | 0x347..=0x349 | 0x34d | 0x34e | 0x353..=0x356 | 0x358..=0x35a | 0x35c..=0x362)
}

/// `Soft_Dotted` do Unicode (os caracteres que perdem o ponto ao receber uma marca acima).
fn is_soft_dotted(c: char) -> bool {
    matches!(c as u32,
        0x69 | 0x6a | 0x12f | 0x249 | 0x268 | 0x29d | 0x2b2 | 0x3f3 | 0x456 | 0x458 | 0x1d62 | 0x1d96 | 0x1da4
        | 0x1da8 | 0x1e2d | 0x1ecb | 0x2071 | 0x2148 | 0x2149 | 0x2c7c)
}

/// `isFollowedByMoreAbove` do ICU: depois do caractere há uma marca de classe 230 antes de qualquer outra de classe 0.
fn followed_by_more_above(rest: &[char]) -> bool {
    for &c in rest {
        if is_above_mark(c) {
            return true;
        }
        if !is_other_accent(c) {
            return false;
        }
    }
    false
}

/// A caixa baixa do lituano: `I`, `J` e `Į` seguidos de marca acima ganham o ponto (`i̇`), e `Ì`, `Í` e `Ĩ`
/// viram `i` + ponto + acento, sempre (as regras de `SpecialCasing.txt` para `lt`).
fn lithuanian_pre_lower(text: &str) -> String {
    let chars: Vec<char> = text.chars().collect();
    let mut out = String::with_capacity(text.len());
    for (index, &c) in chars.iter().enumerate() {
        match c {
            'I' | 'J' | '\u{12e}' if followed_by_more_above(&chars[index + 1..]) => {
                out.push(c.to_lowercase().next().unwrap_or(c));
                out.push('\u{307}');
            }
            '\u{cc}' => out.push_str("i\u{307}\u{300}"),
            '\u{cd}' => out.push_str("i\u{307}\u{301}"),
            '\u{128}' => out.push_str("i\u{307}\u{303}"),
            other => out.push(other),
        }
    }
    out
}

/// A caixa alta do lituano: o ponto acima (`U+0307`) depois de um `Soft_Dotted` some.
fn lithuanian_pre_upper(text: &str) -> String {
    let mut out: Vec<char> = Vec::with_capacity(text.len());
    for c in text.chars() {
        if c == '\u{307}' {
            let preceded = out.iter().rev().find(|&&p| !is_other_accent(p)).is_some_and(|&p| is_soft_dotted(p));
            if preceded {
                continue;
            }
        }
        out.push(c);
    }
    out.into_iter().collect()
}

fn lower(text: &str, locale: &str) -> String {
    if matches!(locale, "tr" | "az") {
        // `I` + ponto acima vira `i`, `I` vira `ı` e `İ` vira `i`.
        let text = text.replace("I\u{307}", "i").replace('I', "\u{131}").replace('\u{130}', "i");
        return text.to_lowercase();
    }
    if locale == "lt" {
        return lithuanian_pre_lower(text).to_lowercase();
    }
    text.to_lowercase()
}

fn upper(text: &str, locale: &str) -> String {
    match locale {
        "lt" => lithuanian_pre_upper(text).to_uppercase(),
        "tr" | "az" => text.replace('i', "\u{130}").to_uppercase(),
        "el" => greek_upper(&text.to_uppercase()),
        _ => text.to_uppercase(),
    }
}

/// O `el` do ICU em maiúsculas: depois do mapeamento da raiz, tira de cada letra grega todas as marcas
/// (tonos, espíritos, perispomeni) menos o dialytika, como `ΐ` que vira `Ϊ` e `ᾀ` que vira `ΑΙ`.
fn greek_upper(upper: &str) -> String {
    let is_greek = |c: char| matches!(c as u32, 0x370..=0x3ff | 0x1f00..=0x1fff);
    let mut stripped = String::with_capacity(upper.len());
    let mut after_greek = false;
    for c in DecomposingNormalizerBorrowed::new_nfd().normalize(upper).chars() {
        let is_mark = matches!(c as u32, 0x300..=0x345) && c != '\u{308}';
        if is_mark && after_greek {
            continue;
        }
        if !matches!(c as u32, 0x300..=0x36f) {
            after_greek = is_greek(c);
        }
        stripped.push(c);
    }
    ComposingNormalizerBorrowed::new_nfc().normalize(&stripped).into_owned()
}

/// O texto convertido (`None`: nada a converter, devolve a mesma string).
pub fn to_locale_case(
    global_object: &JSGlobalObject,
    text: &WtfString,
    locales: JSValue,
    to_upper: bool,
) -> Result<WtfString, Thrown> {
    // A otimização do C++ para a string vazia sem locale.
    if text.is_empty() && locales.is_undefined() {
        return Ok(text.clone());
    }
    let locale = casing_locale(global_object, locales)?;
    let converted = match locale {
        None if to_upper => text.convert_to_uppercase_without_locale(),
        None => text.convert_to_lowercase_without_locale(),
        Some(locale) => {
            map_utf16_segments(text, |source| if to_upper { upper(source, &locale) } else { lower(source, &locale) })
        }
    };
    Ok(converted)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn turkish_dotted_and_dotless_i() {
        assert_eq!(lower("I\u{130}i", "tr"), "\u{131}ii");
        assert_eq!(upper("iI\u{131}", "tr"), "\u{130}II");
    }

    #[test]
    fn lithuanian_dot_above_rules() {
        // Medido no bun: "iIİıi̇".toLocaleUpperCase("lt") = "IIİII".
        assert_eq!(upper("iI\u{130}\u{131}i\u{307}", "lt"), "II\u{130}II");
        assert_eq!(lower("I\u{301}", "lt"), "i\u{307}\u{301}");
        assert_eq!(lower("\u{cc}", "lt"), "i\u{307}\u{300}");
        assert_eq!(lower("Ij", "lt"), "ij");
    }

    #[test]
    fn root_mapping_is_untouched() {
        assert_eq!(lower("I", "en"), "i");
        assert_eq!(upper("i", "en"), "I");
    }

    #[test]
    fn greek_uppercase_drops_the_tonos() {
        assert_eq!(upper("\u{3ac}", "el"), "\u{391}");
    }
}
