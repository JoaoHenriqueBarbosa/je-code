//! `PluralRules` do CLDR pelo icu4x (`icu_plurals` com dados compilados), para qualquer locale.
//!
//! É a camada de dados que substitui as regras escritas à mão de `intl_plural_rules.rs` (só `en` e `pt`):
//! o `uplrules` do ICU no C++ faz o mesmo papel. Aqui só há a seleção; quem monta o objeto JS continua
//! em `intl_plural_rules.rs`.

use fixed_decimal::{CompactDecimal, Decimal};
use icu_locale_core::Locale;
use icu_plurals::provider::{Baked, PluralsRangesV1, UnvalidatedPluralRange};
use icu_plurals::{PluralCategory, PluralRuleType, PluralRules, PluralRulesOptions, PluralRulesWithRanges};
use icu_provider::prelude::{DataIdentifierBorrowed, DataMarkerExt, DataProvider, DataRequest};

use super::icu_plural_range_data;

/// O nome CLDR da categoria, como o `select` do JS devolve.
pub fn category_name(category: PluralCategory) -> &'static str {
    match category {
        PluralCategory::Zero => "zero",
        PluralCategory::One => "one",
        PluralCategory::Two => "two",
        PluralCategory::Few => "few",
        PluralCategory::Many => "many",
        PluralCategory::Other => "other",
    }
}

fn parse_locale(tag: &str) -> Option<Locale> {
    Locale::try_from_str(tag).ok()
}

/// O tipo do `PluralRules`: `ordinal` ou `cardinal`.
fn rule_type(ordinal: bool) -> PluralRuleType {
    if ordinal { PluralRuleType::Ordinal } else { PluralRuleType::Cardinal }
}

/// O decimal com os dígitos visíveis do número já arredondado (`1.0` difere de `1` em inglês). A fração
/// guarda os zeros à direita, que contam no operando `v`.
/// O `exponent` é o expoente de dez da notação compacta (`1.5M` tem `exponent` 6, `integer` `1` e `fraction`
/// `5`): ele vira o operando `c` (e `e`) das regras, como no `uplrules` do ICU. Zero fora do compacto.
fn decimal_of(integer: &str, fraction: &str, exponent: u8) -> Option<CompactDecimal> {
    let text = if fraction.is_empty() { integer.to_string() } else { format!("{integer}.{fraction}") };
    Some(CompactDecimal::from_significand_and_exponent(Decimal::try_from_str(&text).ok()?, exponent))
}

/// As categorias que o locale e o tipo usam, na ordem `zero`, `one`, `two`, `few`, `many`, `other`
/// (o `resolvedOptions().pluralCategories`). `None` se a tag de locale não é válida.
pub fn categories(locale: &str, ordinal: bool) -> Option<Vec<&'static str>> {
    let locale = parse_locale(locale)?;
    let rules = PluralRules::try_new((&locale).into(), PluralRulesOptions::from(rule_type(ordinal))).ok()?;
    let mut found: Vec<PluralCategory> = rules.categories().collect();
    found.sort();
    Some(found.into_iter().map(category_name).collect())
}

/// A categoria do número dado pelos dígitos da parte inteira e da fração (sem sinal) e pelo expoente
/// compacto (zero fora do compacto).
pub fn select(locale: &str, ordinal: bool, integer: &str, fraction: &str, exponent: u8) -> Option<&'static str> {
    let locale = parse_locale(locale)?;
    let rules = PluralRules::try_new((&locale).into(), PluralRulesOptions::from(rule_type(ordinal))).ok()?;
    let decimal = decimal_of(integer, fraction, exponent)?;
    Some(category_name(rules.category_for(&decimal)))
}

/// A categoria do intervalo (`selectRange`): as categorias das pontas, pelas regras do `type` pedido
/// (cardinais ou ordinais), e a tabela de intervalos do CLDR. Cada ponta é `(inteiro, fração, expoente)`; `None` é o
/// infinito, que o ICU classifica como `other` (medido no bun 1.4.2: `ru` `selectRange(Infinity, 1)` é `one`).
pub fn select_range(
    locale: &str,
    ordinal: bool,
    start: Option<(&str, &str, u8)>,
    end: Option<(&str, &str, u8)>,
) -> Option<&'static str> {
    let locale = parse_locale(locale)?;
    let rules = if ordinal {
        PluralRulesWithRanges::try_new_ordinal((&locale).into()).ok()?
    } else {
        PluralRulesWithRanges::try_new_cardinal((&locale).into()).ok()?
    };
    let category_of = |bound: Option<(&str, &str, u8)>| -> Option<PluralCategory> {
        match bound {
            Some((integer, fraction, exponent)) => Some(rules.rules().category_for(&decimal_of(integer, fraction, exponent)?)),
            None => Some(PluralCategory::Other),
        }
    };
    let start_category = category_of(start)?;
    let end_category = category_of(end)?;
    // O `StandardPluralRanges` do ICU consulta a tabela do locale (a mesma para cardinais e ordinais) e,
    // sem entrada para o par de categorias, devolve `other`. O `resolve_range` do icu4x devolveria a
    // categoria da ponta final, por isso a tabela é lida direto.
    let ranges_locale = PluralsRangesV1::make_locale((&locale).into());
    let table = DataProvider::<PluralsRangesV1>::load(
        &Baked,
        DataRequest { id: DataIdentifierBorrowed::for_locale(&ranges_locale), ..Default::default() },
    )
    .ok()?
    .payload;
    let key = UnvalidatedPluralRange::from_range(start_category.into(), end_category.into());
    let raw: Option<icu_plurals::provider::RawPluralCategory> = table.get().ranges.get_copied(&key);
    let result = match raw {
        Some(raw) => PluralCategory::from(raw),
        // O gerador de dados do icu4x descarta as linhas do CLDR cujo resultado é a categoria final (o
        // `resolve_range` dele devolve a final quando a linha falta). O ICU devolve `other` para o par ausente,
        // então essas linhas voltam daqui.
        None if end_result_pairs(&locale, ordinal)
            .is_some_and(|pairs| pairs.contains(&pair_name(start_category, end_category).as_str())) =>
        {
            end_category
        }
        None => PluralCategory::Other,
    };
    Some(category_name(result))
}

fn pair_name(start: PluralCategory, end: PluralCategory) -> String {
    format!("{}-{}", category_name(start), category_name(end))
}

/// Os pares de categorias cujo resultado é a final, do locale mais específico que tem entrada na tabela
/// gerada (escrita e região, região, escrita, língua), como o ICU resolve o locale de dados.
fn end_result_pairs(locale: &Locale, ordinal: bool) -> Option<&'static [&'static str]> {
    let table = if ordinal { icu_plural_range_data::ORDINAL } else { icu_plural_range_data::CARDINAL };
    let language = locale.id.language.as_str();
    let script = locale.id.script.as_ref().map(|script| script.as_str());
    let region = locale.id.region.as_ref().map(|region| region.as_str());
    let mut candidates = Vec::with_capacity(4);
    if let (Some(script), Some(region)) = (script, region) {
        candidates.push(format!("{language}-{script}-{region}"));
    }
    if let Some(region) = region {
        candidates.push(format!("{language}-{region}"));
    }
    if let Some(script) = script {
        candidates.push(format!("{language}-{script}"));
    }
    candidates.push(language.to_string());
    candidates.iter().find_map(|tag| {
        table.binary_search_by(|(key, _)| (*key).cmp(tag.as_str())).ok().map(|index| table[index].1)
    })
}
