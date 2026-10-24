//! `Intl.NumberFormat.prototype.formatRange` e `formatRangeToParts` sem ICU
//! (`IntlNumberFormat::formatRange`, `formatRangeToParts`, `formatRangeToPartsInternal`).
//!
//! O JSC cria o `UNumberRangeFormatter` com `UNUM_RANGE_COLLAPSE_AUTO` e
//! `UNUM_IDENTITY_FALLBACK_APPROXIMATELY`; aqui as duas regras do ICU 75 são reproduzidas sobre as
//! partes do formatador de `default_number_format.rs`:
//!
//! - Identidade: se os dois lados saem no mesmo texto (depois do arredondamento), o resultado é o
//!   número com o `approximatelySign` (`~5`), todo `shared`.
//! - Colapso (`NumberRangeFormatterImpl::formatRange`): o modificador externo (os nomes de unidade e de
//!   moeda por extenso, que o ICU monta no `LongNameHandler`) sai uma vez, na forma plural do fim do
//!   intervalo (`StandardPluralRanges` de `en` e `pt`). O modificador do meio (sinal, símbolo de moeda,
//!   `%`, parênteses contábeis) só colapsa se os dois lados o têm igual e ele tem mais de um ponto de
//!   código (a heurística do ICU 63). O modificador interno do expoente nunca colapsa; o sufixo compacto
//!   colapsa pela mesma heurística quando é igual dos dois lados (`1\u{2013}2 mil`, mas `1K \u{2013} 2K`), e o espaço em
//!   volta do separador só entra se o lado inicial tem modificador que se repete (`999\u{2013}1B`).
//!   Se algum modificador se repete, o separador ganha um espaço de cada lado (quando ainda
//!   não há espaço junto dele).
//!
//! LACUNAS: o separador e o sinal de aproximado vêm de `icu_number_data::RANGES` (medidos no bun para os 38
//! locales do gerador mais `en` e `pt`); o `StandardPluralRanges` de outros locales não existe, então o
//! plural do nome por extenso é sempre o do fim do intervalo (confere nos casos medidos, 3 a 5).

use crate::runtime::default_number_format::{format_parts, NumberSettings, NumericInput, SignDisplay, Style};
use crate::runtime::icu_number_patterns as patterns;
use crate::runtime::intl_support::{RangePart, RangeSource};

/// O separador do `rangePattern` (`{0}\u{2013}{1}`).
const RANGE_SEPARATOR: &str = "\u{2013}";

fn is_core(kind: &str) -> bool {
    matches!(
        kind,
        "integer"
            | "group"
            | "decimal"
            | "fraction"
            | "nan"
            | "infinity"
            | "exponentSeparator"
            | "exponentMinusSign"
            | "exponentInteger"
            | "compact"
    )
}

fn is_inner(kind: &str) -> bool {
    matches!(kind, "exponentSeparator" | "exponentMinusSign" | "exponentInteger" | "compact")
}

/// Onde começa o modificador interno do núcleo: o sufixo compacto (com o espaço que o precede) ou o expoente;
/// `core.len()` se não há.
fn inner_start(core: &[(String, String)]) -> usize {
    match core.iter().position(|(kind, _)| is_inner(kind)) {
        Some(index) if core[index].0 == "compact" && index > 0 && core[index - 1].0 == "literal" => index - 1,
        Some(index) => index,
        None => core.len(),
    }
}

/// Um lado do intervalo repartido nos afixos e no número.
struct Side {
    prefix: Vec<(String, String)>,
    core: Vec<(String, String)>,
    suffix: Vec<(String, String)>,
}

fn split(parts: Vec<(String, String)>) -> Side {
    let first = parts.iter().position(|(kind, _)| is_core(kind)).unwrap_or(0);
    let last = parts.iter().rposition(|(kind, _)| is_core(kind)).unwrap_or(parts.len().saturating_sub(1));
    let mut prefix = parts;
    let mut core = prefix.split_off(first);
    let suffix = core.split_off(last + 1 - first);
    Side { prefix, core, suffix }
}

fn text_of(parts: &[(String, String)]) -> String {
    parts.iter().map(|(_, text)| text.as_str()).collect()
}

fn tagged(parts: &[(String, String)], source: RangeSource) -> impl Iterator<Item = RangePart> + '_ {
    parts.iter().map(move |(kind, text)| (kind.clone(), text.clone(), source))
}

/// Nome de unidade e de moeda por extenso: o `LongNameHandler` do ICU, o modificador externo.
fn suffix_is_outer(settings: &NumberSettings) -> bool {
    use crate::runtime::default_number_format::CurrencyDisplay;
    match settings.style {
        Style::Unit => true,
        Style::Currency => settings.currency_display == CurrencyDisplay::Name,
        Style::Decimal | Style::Percent => false,
    }
}

/// As partes do intervalo de `start` a `end`, cada uma com a origem (`source`).
pub fn format_range_parts(settings: &NumberSettings, start: &NumericInput, end: &NumericInput) -> Vec<RangePart> {
    let start_parts = format_parts(settings, start);
    let end_parts = format_parts(settings, end);
    let range_entry = patterns::range(&settings.locale);
    // Identidade: o mesmo texto nos dois lados (`UNUM_IDENTITY_FALLBACK_APPROXIMATELY`). O sinal de aproximado
    // ocupa o lugar do sinal do número (`nl`: `€ ~5,00`, `de-AT`: `≈€ 5,00`), que se acha formatando o
    // número com `signDisplay: "always"`; sem sinal para trocar, ele vai na frente.
    if text_of(&start_parts) == text_of(&end_parts) {
        let approx = range_entry.map_or("~", |entry| entry.approximately);
        let mut signed_settings = settings.clone();
        signed_settings.sign_display = SignDisplay::Always;
        let signed_parts = format_parts(&signed_settings, start);
        if signed_parts.iter().filter(|(kind, _)| kind == "plusSign").count() == 1 {
            return signed_parts
                .into_iter()
                .map(|(kind, text)| {
                    if kind == "plusSign" {
                        ("approximatelySign".to_string(), approx.to_string(), RangeSource::Shared)
                    } else {
                        (kind, text, RangeSource::Shared)
                    }
                })
                .collect();
        }
        let mut parts: Vec<RangePart> = vec![("approximatelySign".to_string(), approx.to_string(), RangeSource::Shared)];
        parts.extend(tagged(&start_parts, RangeSource::Shared));
        return parts;
    }

    let left = split(start_parts);
    let right = split(end_parts);
    let outer = suffix_is_outer(settings);

    // O modificador do meio: o prefixo e, fora dos nomes por extenso, o sufixo.
    let none: &[(String, String)] = &[];
    let (left_tail, right_tail) = if outer { (none, none) } else { (left.suffix.as_slice(), right.suffix.as_slice()) };
    let left_middle = format!("{}{}", text_of(&left.prefix), text_of(left_tail));
    let right_middle = format!("{}{}", text_of(&right.prefix), text_of(right_tail));
    let middle_len = left_middle.chars().count();
    let collapse_middle = left_middle == right_middle && middle_len > 1;
    // O modificador interno (o sufixo compacto com o espaço que o precede, ou o expoente). Só o compacto
    // colapsa, e pela mesma heurística do ICU 63: texto igual dos dois lados e mais de um ponto de código
    // (`1\u{2013}2 mil`, `1\u{2013}2 k`; `1K \u{2013} 2K` e `1천 ~ 2천` não colapsam). Sem o colapso, o separador ganha espaços
    // quando é o modificador do começo que existe (`999\u{2013}1B` sai sem eles, `1.5K \u{2013} 1M` com).
    let left_inner_at = inner_start(&left.core);
    let right_inner_at = inner_start(&right.core);
    let (left_inner, right_inner) = (&left.core[left_inner_at..], &right.core[right_inner_at..]);
    let inner_text = text_of(left_inner);
    let collapse_inner = left_inner.iter().any(|(kind, _)| kind == "compact")
        && inner_text == text_of(right_inner)
        && inner_text.chars().count() > 1
        && (collapse_middle || (left_middle.is_empty() && right_middle.is_empty()));
    let repeat = (!collapse_middle && middle_len > 0) || (!collapse_inner && !left_inner.is_empty());

    // Os pedaços de cada lado, na ordem em que saem.
    let (left_core, right_core) =
        if collapse_inner { (&left.core[..left_inner_at], &right.core[..right_inner_at]) } else { (&left.core[..], &right.core[..]) };
    let mut left_out: Vec<RangePart> = Vec::new();
    let mut right_out: Vec<RangePart> = Vec::new();
    let mut tail_out: Vec<RangePart> = Vec::new();
    let mut head_out: Vec<RangePart> = Vec::new();
    if collapse_inner {
        tail_out.extend(tagged(left_inner, RangeSource::Shared));
    }
    if collapse_middle {
        head_out.extend(tagged(&left.prefix, RangeSource::Shared));
        left_out.extend(tagged(left_core, RangeSource::StartRange));
        right_out.extend(tagged(right_core, RangeSource::EndRange));
        if !outer {
            tail_out.extend(tagged(&left.suffix, RangeSource::Shared));
        }
    } else {
        left_out.extend(tagged(&left.prefix, RangeSource::StartRange));
        left_out.extend(tagged(left_core, RangeSource::StartRange));
        right_out.extend(tagged(&right.prefix, RangeSource::EndRange));
        right_out.extend(tagged(right_core, RangeSource::EndRange));
        if !outer {
            left_out.extend(tagged(&left.suffix, RangeSource::StartRange));
            right_out.extend(tagged(&right.suffix, RangeSource::EndRange));
        }
    }
    if outer {
        // A forma plural do intervalo é a do fim (`StandardPluralRanges` de `en` e `pt`).
        tail_out.extend(tagged(&right.suffix, RangeSource::Shared));
    }

    let left_ends_in_space = left_out.last().is_some_and(|(_, text, _)| text.chars().last().is_some_and(char::is_whitespace));
    let right_starts_with_space =
        right_out.first().is_some_and(|(_, text, _)| text.chars().next().is_some_and(char::is_whitespace));
    let mut separator = String::new();
    if repeat && !left_ends_in_space {
        separator.push(' ');
    }
    separator.push_str(range_entry.map_or(RANGE_SEPARATOR, |entry| entry.separator));
    if repeat && !right_starts_with_space {
        separator.push(' ');
    }

    let mut parts = head_out;
    parts.extend(left_out);
    parts.push(("literal".to_string(), separator, RangeSource::Shared));
    parts.extend(right_out);
    parts.extend(tail_out);
    parts
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::runtime::default_number_format::{CurrencyDisplay, Notation, UnitDisplay};
    use crate::runtime::intl_locale_data::Language;

    fn range(settings: &NumberSettings, start: f64, end: f64) -> String {
        format_range_parts(settings, &NumericInput::Double(start), &NumericInput::Double(end))
            .into_iter()
            .map(|(_, text, _)| text)
            .collect()
    }

    fn usd(language: Language) -> NumberSettings {
        let mut settings = NumberSettings::defaults(language);
        settings.style = Style::Currency;
        settings.currency = if language == Language::English { "USD" } else { "BRL" }.to_string();
        settings.rounding = crate::runtime::default_number_format::Rounding::FractionDigits { min: 0, max: 0 };
        settings
    }

    #[test]
    fn plain_numbers_use_the_bare_separator() {
        let settings = NumberSettings::defaults(Language::English);
        assert_eq!(range(&settings, 3.0, 5.0), "3\u{2013}5");
        assert_eq!(range(&settings, 1000.0, 2500.5), "1,000\u{2013}2,500.5");
    }

    #[test]
    fn one_code_point_affix_repeats_with_spaces() {
        assert_eq!(range(&usd(Language::English), 3.0, 5.0), "$3 \u{2013} $5");
        let mut percent = NumberSettings::defaults(Language::English);
        percent.style = Style::Percent;
        assert_eq!(range(&percent, 0.03, 0.05), "3% \u{2013} 5%");
        assert_eq!(range(&NumberSettings::defaults(Language::English), -3.0, 5.0), "-3 \u{2013} 5");
    }

    #[test]
    fn longer_affix_collapses() {
        // `R$` mais o espaço sem quebra: três pontos de código.
        assert_eq!(range(&usd(Language::Portuguese), 3.0, 5.0), "R$\u{a0}3\u{2013}5");
        let mut code = usd(Language::English);
        code.currency_display = CurrencyDisplay::Code;
        assert_eq!(range(&code, 3.0, 5.0), "USD\u{a0}3\u{2013}5");
    }

    #[test]
    fn unit_and_currency_names_collapse_with_the_range_plural() {
        let mut unit = NumberSettings::defaults(Language::English);
        unit.style = Style::Unit;
        unit.unit = "kilometer".to_string();
        unit.unit_display = UnitDisplay::Long;
        assert_eq!(range(&unit, 1.0, 5.0), "1\u{2013}5 kilometers");
        unit.unit_display = UnitDisplay::Short;
        assert_eq!(range(&unit, 3.0, 5.0), "3\u{2013}5 km");
    }

    #[test]
    fn compact_suffix_repeats() {
        let mut compact = NumberSettings::defaults(Language::English);
        compact.notation = Notation::Compact;
        assert_eq!(range(&compact, 3000.0, 5000.0), "3K \u{2013} 5K");
        // O modificador do começo vazio: sem espaços em volta do separador (medido no bun).
        assert_eq!(range(&compact, 999.0, 1_000_000_000.0), "999\u{2013}1B");
        let mut long = compact.clone();
        long.compact_display = crate::runtime::default_number_format::CompactDisplay::Long;
        assert_eq!(range(&long, 1000.0, 2000.0), "1\u{2013}2 thousand");
        assert_eq!(range(&long, 1500.0, 1_000_000.0), "1.5 thousand \u{2013} 1 million");
    }

    #[test]
    fn equal_endpoints_are_approximate_and_shared() {
        let settings = NumberSettings::defaults(Language::English);
        let parts = format_range_parts(&settings, &NumericInput::Double(5.0), &NumericInput::Double(5.0));
        assert_eq!(
            parts,
            vec![
                ("approximatelySign".to_string(), "~".to_string(), RangeSource::Shared),
                ("integer".to_string(), "5".to_string(), RangeSource::Shared),
            ]
        );
        // Iguais depois do arredondamento.
        assert_eq!(range(&settings, 1.0001, 1.0002), "~1");
        assert_eq!(range(&usd(Language::English), 5.0, 5.0), "~$5");
    }

    #[test]
    fn sources_follow_the_endpoints() {
        let parts = format_range_parts(&usd(Language::English), &NumericInput::Double(3.0), &NumericInput::Double(5.0));
        let tags: Vec<(&str, &str, RangeSource)> = parts.iter().map(|(a, b, c)| (a.as_str(), b.as_str(), *c)).collect();
        assert_eq!(
            tags,
            vec![
                ("currency", "$", RangeSource::StartRange),
                ("integer", "3", RangeSource::StartRange),
                ("literal", " \u{2013} ", RangeSource::Shared),
                ("currency", "$", RangeSource::EndRange),
                ("integer", "5", RangeSource::EndRange),
            ]
        );
        let parts = format_range_parts(&usd(Language::Portuguese), &NumericInput::Double(3.0), &NumericInput::Double(5.0));
        let sources: Vec<RangeSource> = parts.iter().map(|part| part.2).collect();
        assert_eq!(
            sources,
            vec![
                RangeSource::Shared,
                RangeSource::Shared,
                RangeSource::StartRange,
                RangeSource::Shared,
                RangeSource::EndRange
            ]
        );
    }
}
