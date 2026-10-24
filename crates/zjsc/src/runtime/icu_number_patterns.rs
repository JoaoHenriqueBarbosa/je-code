//! Leitura dos padrões de moeda, de percentual e de unidade de `icu_number_data` (gerado do bun por
//! `scripts/gen-number-format-data.js`). O `icu_decimal` não traz afixos, então o que fica em volta do
//! número (posição e espaço do símbolo, sinal, parênteses do contábil, nome da moeda e da unidade por
//! categoria de plural) vem desta tabela, só para as línguas medidas. `en` e `pt` seguem nas tabelas
//! escritas à mão de `default_number_format.rs`; o que não tem entrada aqui cai nelas.

use crate::runtime::icu_number::Part;
use crate::runtime::icu_number_data::{
    ACCOUNTING, ACCOUNTING_POSITIVES, COMPOUND_UNITS, CURRENCIES, CURRENCY_BASES, CURRENCY_NAMES, CURRENCY_SPACING, EXTRA_CURRENCIES, PER_UNITS, PERCENTS, PERCENTS_COMPACT, RANGES, STRINGS, UNITS,
};

const SEPARATOR: char = '\u{1f}';

/// Se o negativo contábil usa parênteses no `currencyDisplay` (0 symbol, 1 narrowSymbol, 2 code, 3 name) e na
/// notação (padrão ou compacta), medido no bun.
pub struct AccountingEntry {
    pub locale: &'static str,
    pub display: u8,
    pub compact: bool,
    pub parentheses: bool,
}

/// Se o espaço entre a moeda e o número é o que o ICU insere (`currencySpacing`) e some quando o número é
/// `∞` ou `NaN`, por locale e `currencySign`, medido no bun. O espaço que está no padrão (pt, de, fr) fica.
pub struct CurrencySpacingEntry {
    pub locale: &'static str,
    pub accounting: bool,
    pub inserted: bool,
}

/// Moeda por símbolo (`display` 0), símbolo estreito (1) ou código (2): o padrão do positivo, do
/// negativo e do negativo contábil.
pub struct CurrencyEntry {
    pub locale: &'static str,
    pub currency: &'static str,
    pub display: u8,
    pub positive: &'static str,
    pub negative: &'static str,
    pub accounting: &'static str,
}

/// O padrão do positivo contábil de uma moeda, quando difere do positivo comum (ar, fa, nb).
pub struct AccountingPositiveEntry {
    pub locale: &'static str,
    pub currency: &'static str,
    pub display: u8,
    pub template: &'static str,
}

/// Moeda por nome (`currencyDisplay: "name"`) na categoria de plural dada.
pub struct CurrencyNameEntry {
    pub locale: &'static str,
    pub currency: &'static str,
    pub category: &'static str,
    pub positive: &'static str,
    pub negative: &'static str,
}

/// Unidade simples no `unitDisplay` (0 long, 1 short, 2 narrow) e na categoria de plural dada.
pub struct UnitEntry {
    pub locale: &'static str,
    pub unit: &'static str,
    pub display: u8,
    pub category: &'static str,
    pub template: &'static str,
}

/// O padrão "por unidade" (`perUnitPattern` do CLDR) de um denominador no `unitDisplay` (0 long, 1 short,
/// 2 narrow): o sufixo que vai depois do texto do numerador (`/h`, ` per hour`).
pub struct PerUnitEntry {
    pub locale: &'static str,
    pub denominator: &'static str,
    pub display: u8,
    pub suffix: &'static str,
}

/// Unidade composta de `en` cujo texto não é numerador + sufixo: o texto inteiro no `unitDisplay` (0 long,
/// 1 short, 2 narrow), para o valor 1 (`one`) e o 2 (`other`).
pub struct CompoundUnitEntry {
    pub numerator: &'static str,
    pub denominator: &'static str,
    pub display: u8,
    pub one: &'static str,
    pub other: &'static str,
}

/// O padrão de percentual do locale.
pub struct PercentEntry {
    pub locale: &'static str,
    pub template: &'static str,
}

/// Moeda fora de `CurrencyEntry` (as de `Intl.supportedValuesOf("currency")`), só com o que difere do código:
/// `symbol` e `narrow` vazios são o código e o símbolo; os padrões vêm de `CurrencyBase` (`symbol_base`,
/// `narrow_base`); `name` vazio é o código; `plural_names` é `categoria=texto` separado por U+001F.
///
/// Os textos moram na tabela `STRINGS` de `icu_number_data` (únicos, compartilhados entre locales) e a
/// entrada guarda índices; as entradas de um locale ficam juntas em `ExtraLocale`.
pub struct ExtraCurrencyEntry {
    pub currency: &'static str,
    symbol: u16,
    narrow: u16,
    pub symbol_base: u8,
    pub narrow_base: u8,
    name: u16,
    plural_names: u16,
}

/// As moedas extras de um locale, ordenadas por código.
pub struct ExtraLocale {
    pub locale: &'static str,
    pub entries: &'static [ExtraCurrencyEntry],
}

/// Linha compacta da tabela gerada: índices em `STRINGS` para símbolo, símbolo estreito, nome e nomes por plural.
pub const fn extra(currency: &'static str, symbol: u16, narrow: u16, symbol_base: u8, narrow_base: u8, name: u16, plural_names: u16) -> ExtraCurrencyEntry {
    ExtraCurrencyEntry { currency, symbol, narrow, symbol_base, narrow_base, name, plural_names }
}

fn text(index: u16) -> &'static str {
    STRINGS[usize::from(index)]
}

/// Uma forma de padrão de moeda por símbolo do locale (posição e espaço do símbolo), com o texto da
/// moeda de uma moeda qualquer que o locale mostra assim; quem renderiza troca o texto.
pub struct CurrencyBase {
    pub locale: &'static str,
    pub id: u8,
    pub positive: &'static str,
    pub negative: &'static str,
    pub accounting: &'static str,
}

/// Padrão de intervalo do locale: o separador de `{0}{separator}{1}` e o texto do sinal de aproximado.
pub struct RangeEntry {
    pub locale: &'static str,
    pub separator: &'static str,
    pub approximately: &'static str,
}

/// O que preenche os símbolos de um padrão.
pub struct Fill<'a> {
    pub number: &'a [Part],
    pub sign_prefix: &'a [Part],
    pub sign_suffix: &'a [Part],
    /// Troca o texto da moeda do padrão (o código de uma moeda sem entrada).
    pub currency: Option<&'a str>,
    /// Troca o texto do sinal de percentual (dígitos árabes têm o seu).
    pub percent: Option<&'a str>,
}

/// O locale pedido e depois só a língua: `fr-CA` acha `fr-CA`, `fr-FR` acha `fr`.
fn keys(locale: &str) -> [&str; 2] {
    [locale, locale.split('-').next().unwrap_or(locale)]
}

fn find<T>(table: &'static [T], locale: &str, matches: impl Fn(&'static T) -> (&'static str, bool)) -> Option<&'static T> {
    keys(locale).into_iter().find_map(|key| {
        table.iter().find(|entry| {
            let (entry_locale, hit) = matches(*entry);
            hit && entry_locale == key
        })
    })
}

/// A entrada de moeda; o símbolo estreito sem entrada própria é o símbolo.
pub fn currency(locale: &str, code: &str, display: u8) -> Option<&'static CurrencyEntry> {
    let by = |wanted: u8| find(CURRENCIES, locale, |entry| (entry.locale, entry.currency == code && entry.display == wanted));
    by(display).or_else(|| if display == 1 { by(0) } else { None })
}

/// O positivo contábil que difere do positivo comum; `None` quando os dois são o mesmo padrão.
pub fn accounting_positive(locale: &str, code: &str, display: u8) -> Option<&'static str> {
    // Um locale medido por inteiro (ar-EG) sem entrada aqui tem o positivo contábil igual ao comum, não o da língua.
    let measured = CURRENCIES.iter().any(|entry| entry.locale == locale);
    let by = |wanted: u8| {
        ACCOUNTING_POSITIVES
            .iter()
            .find(|entry| entry.currency == code && entry.display == wanted && (entry.locale == locale || (!measured && keys(locale)[1] == entry.locale)))
    };
    by(display).or_else(|| if display == 1 { by(0) } else { None }).map(|entry| entry.template)
}

/// Se o negativo contábil usa parênteses; o locale sem entrada cai em `en`.
pub fn accounting_parentheses(locale: &str, display: u8, compact: bool) -> bool {
    let by = |key: &str| ACCOUNTING.iter().find(|entry| entry.locale == key && entry.display == display && entry.compact == compact);
    keys(locale).into_iter().find_map(by).or_else(|| by("en")).is_some_and(|entry| entry.parentheses)
}

/// O padrão em código de uma moeda sem entrada (o texto da moeda é trocado por quem renderiza).
pub fn currency_code_fallback(locale: &str) -> Option<&'static CurrencyEntry> {
    find(CURRENCIES, locale, |entry| (entry.locale, entry.currency == "USD" && entry.display == 2))
}

/// O nome da moeda na categoria dada, ou em `other` quando o locale não distingue a categoria.
pub fn currency_name(locale: &str, code: &str, category: &str) -> Option<&'static CurrencyNameEntry> {
    let by = |wanted: &str| find(CURRENCY_NAMES, locale, |entry| (entry.locale, entry.currency == code && entry.category == wanted));
    by(category).or_else(|| by("other"))
}

/// O padrão da unidade (`unit` inteiro, `kilometer-per-hour` incluso) na categoria dada, ou em `other`.
pub fn unit(locale: &str, unit: &str, display: u8, category: &str) -> Option<&'static UnitEntry> {
    let by = |wanted: &str| find(UNITS, locale, |entry| (entry.locale, entry.unit == unit && entry.display == display && entry.category == wanted));
    by(category).or_else(|| by("other"))
}

/// O sufixo "por unidade" do denominador no locale e no `unitDisplay`, medido no bun.
pub fn per_unit_suffix(locale: &str, denominator: &str, display: u8) -> Option<&'static str> {
    find(PER_UNITS, locale, |entry| (entry.locale, entry.denominator == denominator && entry.display == display)).map(|entry| entry.suffix)
}

/// O texto inteiro da unidade composta de `en` que tem padrão próprio no CLDR, ou `None` quando é
/// numerador + sufixo.
pub fn compound_unit_text(numerator: &str, denominator: &str, display: u8, plural: bool) -> Option<&'static str> {
    COMPOUND_UNITS
        .iter()
        .find(|entry| entry.numerator == numerator && entry.denominator == denominator && entry.display == display)
        .map(|entry| if plural { entry.other } else { entry.one })
}

/// A moeda fora do conjunto principal, ou `None` quando o locale não a distingue do código.
pub fn extra_currency(locale: &str, code: &str) -> Option<&'static ExtraCurrencyEntry> {
    // O locale medido por inteiro (en-GB) sem a moeda no seu grupo mostra o código; não cai na língua.
    keys(locale).into_iter().find_map(|key| {
        let group = EXTRA_CURRENCIES.iter().find(|group| group.locale == key)?;
        Some(group.entries.binary_search_by(|entry| entry.currency.cmp(code)).ok().map(|at| &group.entries[at]))
    })?
}

/// O padrão de símbolo `id` do locale (o de `locale` ou, sem ele, o da língua).
pub fn currency_base(locale: &str, id: u8) -> Option<&'static CurrencyBase> {
    find(CURRENCY_BASES, locale, |entry| (entry.locale, entry.id == id))
}

/// O padrão de intervalo do locale (`en` e `pt` inclusos).
pub fn range(locale: &str) -> Option<&'static RangeEntry> {
    find(RANGES, locale, |entry| (entry.locale, true))
}

impl ExtraCurrencyEntry {
    /// O símbolo do locale (vazio quando é o código).
    pub fn symbol(&self) -> &'static str {
        text(self.symbol)
    }

    /// O símbolo estreito (vazio quando é o símbolo).
    pub fn narrow(&self) -> &'static str {
        text(self.narrow)
    }

    /// O nome por extenso na categoria de plural dada (`other` quando o locale não a distingue).
    pub fn name_for(&self, category: &str) -> Option<&'static str> {
        let plural = text(self.plural_names).split(SEPARATOR).find_map(|item| item.strip_prefix(category)?.strip_prefix('='));
        let name = text(self.name);
        plural.or(if name.is_empty() { None } else { Some(name) })
    }
}

/// Tira o espaço que o ICU insere entre a moeda e o número quando o número é `∞` ou `NaN` (não são dígitos,
/// então o `currencySpacing` não age): só o literal de um caractere de espaço entre `currency` e
/// `infinity`/`nan`, e só nos locales em que a tabela medida diz que o espaço é inserido e não do padrão.
pub fn drop_inserted_currency_spacing(locale: &str, accounting: bool, parts: &mut Vec<Part>) {
    let by = |key: &str| CURRENCY_SPACING.iter().find(|entry| entry.locale == key && entry.accounting == accounting);
    if !keys(locale).into_iter().find_map(by).or_else(|| by("en")).is_some_and(|entry| entry.inserted) {
        return;
    }
    let is_number = |kind: &str| kind == "infinity" || kind == "nan";
    let is_space = |text: &str| matches!(text, "\u{a0}" | "\u{202f}" | " ");
    let mut at = 1;
    while at + 1 < parts.len() {
        let (before, middle, after) = (&parts[at - 1], &parts[at], &parts[at + 1]);
        let between = (before.0 == "currency" && is_number(&after.0)) || (is_number(&before.0) && after.0 == "currency");
        if between && middle.0 == "literal" && is_space(&middle.1) {
            parts.remove(at);
        } else {
            at += 1;
        }
    }
}

/// O padrão de percentual do locale; `compact` pede o da notação compacta (o da unidade `percent`).
pub fn percent(locale: &str, compact: bool) -> Option<&'static PercentEntry> {
    find(if compact { PERCENTS_COMPACT } else { PERCENTS }, locale, |entry| (entry.locale, true))
}

/// Monta as partes de um padrão: `n` recebe o número, `-` o sinal, e os literais, a moeda, a unidade e o
/// percentual saem com os tipos `literal`, `currency`, `unit` e `percentSign`.
pub fn render(template: &str, fill: &Fill) -> Vec<Part> {
    let mut parts: Vec<Part> = Vec::new();
    for token in template.split(SEPARATOR) {
        let mut chars = token.chars();
        let Some(kind) = chars.next() else { continue };
        let text = chars.as_str();
        match kind {
            'n' => parts.extend(fill.number.iter().cloned()),
            '-' => {
                parts.extend(fill.sign_prefix.iter().cloned());
                parts.extend(fill.sign_suffix.iter().cloned());
            }
            'l' => parts.push(("literal".to_string(), text.to_string())),
            'c' => parts.push(("currency".to_string(), fill.currency.unwrap_or(text).to_string())),
            'u' => parts.push(("unit".to_string(), text.to_string())),
            '%' => parts.push(("percentSign".to_string(), fill.percent.unwrap_or(text).to_string())),
            _ => {}
        }
    }
    parts
}

#[cfg(test)]
mod tests {
    use super::*;

    fn text(parts: &[Part]) -> String {
        parts.iter().map(|(_, text)| text.as_str()).collect()
    }

    fn number() -> Vec<Part> {
        vec![("integer".to_string(), "5".to_string())]
    }

    #[test]
    fn locale_falls_back_to_language() {
        assert!(currency("fr-FR", "EUR", 0).is_some());
        assert!(currency("fr-CA", "EUR", 0).is_some());
        assert!(currency("en", "EUR", 0).is_none());
    }

    #[test]
    fn french_euro_follows_the_number() {
        let entry = currency("fr", "EUR", 0).expect("entrada de fr");
        let fill = Fill { number: &number(), sign_prefix: &[], sign_suffix: &[], currency: None, percent: None };
        assert_eq!(text(&render(entry.positive, &fill)), "5\u{a0}\u{20ac}");
    }

    #[test]
    fn code_fallback_swaps_the_currency_text() {
        let entry = currency_code_fallback("de").expect("entrada de de");
        let fill = Fill { number: &number(), sign_prefix: &[], sign_suffix: &[], currency: Some("AUD"), percent: None };
        assert!(text(&render(entry.positive, &fill)).contains("AUD"));
    }

    #[test]
    fn plural_category_falls_back_to_other() {
        assert!(unit("ja", "kilometer", 1, "many").is_some());
    }
}
