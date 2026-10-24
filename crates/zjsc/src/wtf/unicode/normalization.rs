//! As quatro formas de normalização Unicode (`unorm2_getNFCInstance` e companhia do ICU), sobre a crate
//! `icu_normalizer` (ICU4X, dados compilados do Unicode 16, o mesmo do ICU 76 do bun).
//!
//! DIVERGÊNCIA tratada: o ICU deixa surrogate solto passar intacto (ele é um iniciador sem composição),
//! enquanto o `icu_normalizer` troca surrogate solto por U+FFFD. Por isso o texto é partido em cada
//! surrogate solto, cada trecho bem formado é normalizado separado e o surrogate é copiado como está.
use std::borrow::Cow;

use icu_normalizer::{ComposingNormalizerBorrowed, DecomposingNormalizerBorrowed};

/// `NormalizationForm` de `StringPrototype.cpp`.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum NormalizationForm {
    NFC,
    NFD,
    NFKC,
    NFKD,
}

/// Normaliza um trecho UTF-16 bem formado.
fn normalize_well_formed(form: NormalizationForm, units: &[u16]) -> Cow<'_, [u16]> {
    match form {
        NormalizationForm::NFC => ComposingNormalizerBorrowed::new_nfc().normalize_utf16(units),
        NormalizationForm::NFKC => ComposingNormalizerBorrowed::new_nfkc().normalize_utf16(units),
        NormalizationForm::NFD => DecomposingNormalizerBorrowed::new_nfd().normalize_utf16(units),
        NormalizationForm::NFKD => DecomposingNormalizerBorrowed::new_nfkd().normalize_utf16(units),
    }
}

/// `unorm2_normalize` sobre unidades UTF-16 quaisquer: `None` quando o texto já está normalizado
/// (o `unorm2_isNormalized` do C++, que devolve a própria string).
pub fn normalize_utf16(form: NormalizationForm, units: &[u16]) -> Option<Vec<u16>> {
    let mut output: Vec<u16> = Vec::with_capacity(units.len());
    let mut changed = false;
    let mut run_start = 0;
    let mut index = 0;
    while index < units.len() {
        let unit = units[index];
        let is_lone_surrogate = match unit {
            0xD800..=0xDBFF => !matches!(units.get(index + 1), Some(0xDC00..=0xDFFF)),
            0xDC00..=0xDFFF => true,
            _ => false,
        };
        if is_lone_surrogate {
            changed |= flush_run(form, &units[run_start..index], &mut output);
            output.push(unit);
            run_start = index + 1;
        } else if (0xD800..=0xDBFF).contains(&unit) {
            index += 1;
        }
        index += 1;
    }
    changed |= flush_run(form, &units[run_start..], &mut output);
    if changed { Some(output) } else { None }
}

/// Normaliza um trecho bem formado para o fim de `output`; devolve se o trecho mudou.
fn flush_run(form: NormalizationForm, run: &[u16], output: &mut Vec<u16>) -> bool {
    match normalize_well_formed(form, run) {
        Cow::Borrowed(same) => {
            output.extend_from_slice(same);
            false
        }
        Cow::Owned(normalized) => {
            output.extend_from_slice(&normalized);
            true
        }
    }
}
