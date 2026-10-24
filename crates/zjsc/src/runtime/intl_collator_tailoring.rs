//! A resolução de locale do `Intl.Collator` e as colações que cada idioma lista, medidas no bun
//! (`tests/golden/collator_bun.tsv`, gerado por `scripts/gen-collator-golden.js`).
//!
//! A ordem em si (tailoring de `sv`, `tr`, `de-u-co-phonebk`, `es-u-co-trad`, han por pinyin ou traços...)
//! vem toda do CLDR compilado do `icu_collator`, em `intl_collator.rs`; aqui ficam só a escolha do
//! locale do colador e a disponibilidade das colações.

use crate::runtime::intl_locale_data::best_available_by;

/// O locale do colador para a tag pedida (sem extensões): `en-GB` vira `en`, `fr-CA` fica `fr-CA`, o que
/// o colador não conhece não tem resposta. As listas vêm de `intl_available_locales_data` (medidas no bun).
pub fn collator_locale(base_name: &str) -> Option<String> {
    use crate::runtime::intl_available_locales_data::{COLLATOR_LANGUAGES, COLLATOR_LOCALES};
    use crate::runtime::intl_table_lookup::contains_sorted;
    best_available_by(base_name, |candidate| {
        contains_sorted(&COLLATOR_LOCALES, candidate)
            || (!candidate.contains('-') && contains_sorted(&COLLATOR_LANGUAGES, candidate))
    })
}

/// As colações que o colador lista para o idioma, além de `emoji` e `eor`.
pub fn extra_collations(language: &str) -> &'static [&'static str] {
    match language {
        "de" => &["phonebk"],
        "es" | "sv" | "fi" => &["trad"],
        "zh" => &["pinyin", "stroke", "zhuyin", "unihan"],
        "ja" | "ko" => &["unihan"],
        "ar" => &["compat"],
        _ => &[],
    }
}
