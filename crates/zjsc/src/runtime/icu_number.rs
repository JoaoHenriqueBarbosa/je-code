//! Formatação decimal do CLDR pelo icu4x (`icu_decimal` com dados compilados), para qualquer locale.
//!
//! É a camada de dados que substitui as tabelas de símbolo e agrupamento de `default_number_format.rs`
//! (só `en` e `pt`): o `unumf` do ICU no C++ faz o mesmo papel. Aqui vivem o separador decimal e o de
//! grupo, o tamanho dos grupos (`en-IN` agrupa em 3 e 2), o `minimumGroupingDigits` (`es`, `pt-PT`), os
//! dígitos do sistema numérico (`-u-nu-`), o sinal de mais e o de menos (com as marcas bidirecionais do
//! árabe) e a notação compacta curta e longa. O arredondamento continua em `default_number_format.rs`,
//! no estilo do JSC: este módulo recebe os dígitos já arredondados.
//!
//! O que o `icu_decimal` NÃO traz, e vem daqui à mão (medido no bun/ICU): o texto de `NaN` por locale
//! ([`nan_symbol`]), o sinal de percentual por sistema numérico ([`percent_sign`]) e o `useGrouping:
//! "always"` (`GroupingStrategy::Always` age como `Auto` no `DecimalFormatter`; `DecimalFormat::number`
//! força o grupo dos quatro dígitos). Fora daqui, por falta de dados: moeda e unidade (padrões com afixos).

use std::fmt;

use fixed_decimal::{Decimal, Sign};
use icu_decimal::options::{CompactDecimalFormatterOptions, DecimalFormatterOptions};
use icu_decimal::preferences::CompactDecimalFormatterPreferences;
use icu_decimal::{parts, CompactDecimalFormatter, DecimalFormatter};
use icu_locale_core::Locale;
use writeable::{Part as IcuPart, PartsWrite, Writeable};

pub use icu_decimal::options::GroupingStrategy;

/// Uma parte do `formatToParts`: o tipo e o texto.
pub type Part = (String, String);

/// Os sistemas numéricos de dígitos decimais do Unicode e o dígito zero de cada um. O `icu_decimal` cai
/// em silêncio no `latn` quando não tem dados do sistema pedido; o zero revela o que foi usado de fato.
const ZERO_DIGITS: [(&str, char); 21] = [
    ("arab", '\u{660}'),
    ("arabext", '\u{6f0}'),
    ("beng", '\u{9e6}'),
    ("deva", '\u{966}'),
    ("fullwide", '\u{ff10}'),
    ("gujr", '\u{ae6}'),
    ("guru", '\u{a66}'),
    ("hanidec", '\u{3007}'),
    ("khmr", '\u{17e0}'),
    ("knda", '\u{ce6}'),
    ("laoo", '\u{ed0}'),
    ("latn", '0'),
    ("limb", '\u{1946}'),
    ("mlym", '\u{d66}'),
    ("mong", '\u{1810}'),
    ("mymr", '\u{1040}'),
    ("orya", '\u{b66}'),
    ("tamldec", '\u{be6}'),
    ("telu", '\u{c66}'),
    ("thai", '\u{e50}'),
    ("tibt", '\u{f20}'),
];

/// O texto escrito, em trechos com o tipo da parte numérica do icu4x (`None`: texto sem parte, como o
/// `k` do compacto).
type Segments = Vec<(Option<&'static str>, String)>;

/// O nome ECMA-402 da parte do `icu_decimal`.
fn part_name(part: IcuPart) -> Option<&'static str> {
    if part == parts::INTEGER {
        Some("integer")
    } else if part == parts::GROUP {
        Some("group")
    } else if part == parts::DECIMAL {
        Some("decimal")
    } else if part == parts::FRACTION {
        Some("fraction")
    } else if part == parts::MINUS_SIGN {
        Some("minusSign")
    } else if part == parts::PLUS_SIGN {
        Some("plusSign")
    } else {
        None
    }
}

/// O destino de `Writeable::write_to_parts` que junta o texto por parte (a mais interna que o `Intl` nomeia).
#[derive(Default)]
struct Collector {
    stack: Vec<IcuPart>,
    segments: Segments,
}

impl fmt::Write for Collector {
    fn write_str(&mut self, text: &str) -> fmt::Result {
        if text.is_empty() {
            return Ok(());
        }
        let name = self.stack.iter().rev().find_map(|part| part_name(*part));
        match self.segments.last_mut() {
            Some((last, buffer)) if *last == name => buffer.push_str(text),
            _ => self.segments.push((name, text.to_string())),
        }
        Ok(())
    }
}

impl PartsWrite for Collector {
    type SubPartsWrite = Collector;

    fn with_part(&mut self, part: IcuPart, mut body: impl FnMut(&mut Collector) -> fmt::Result) -> fmt::Result {
        self.stack.push(part);
        let result = body(self);
        self.stack.pop();
        result
    }
}

fn collect(writeable: &impl Writeable) -> Segments {
    let mut collector = Collector::default();
    // O destino não falha: o `fmt::Result` só existe porque a interface o exige.
    let _ = writeable.write_to_parts(&mut collector);
    collector.segments
}

/// Os trechos como partes do `Intl`; o que não tem parte vira `fallback` (`literal`).
fn named(segments: Segments, fallback: &str) -> Vec<Part> {
    segments.into_iter().map(|(name, text)| (name.unwrap_or(fallback).to_string(), text)).collect()
}

/// O locale do icu4x: a tag base (`de`) com o sistema numérico pedido (`-u-nu-arab`), se houver.
fn locale_with_numbering(tag: &str, numbering_system: &str) -> Option<Locale> {
    let text = if numbering_system.is_empty() { tag.to_string() } else { format!("{tag}-u-nu-{numbering_system}") };
    Locale::try_from_str(&text).ok()
}

/// O decimal com os dígitos visíveis (os zeros à esquerda e à direita contam: `minimumIntegerDigits` e
/// `minimumFractionDigits`).
fn decimal_of(integer: &str, fraction: &str) -> Option<Decimal> {
    let text = if fraction.is_empty() { integer.to_string() } else { format!("{integer}.{fraction}") };
    Decimal::try_from_str(&text).ok()
}

/// Sem dados do locale: os dígitos em ASCII, sem grupo, com o ponto decimal.
pub fn plain_parts(integer: &str, fraction: &str) -> Vec<Part> {
    let mut parts = vec![("integer".to_string(), integer.to_string())];
    if !fraction.is_empty() {
        parts.push(("decimal".to_string(), ".".to_string()));
        parts.push(("fraction".to_string(), fraction.to_string()));
    }
    parts
}

/// Formata `integer.fraction` com `render` e, em `useGrouping: "always"`, força o grupo dos quatro dígitos
/// que o locale suprime (`minimumGroupingDigits` 2): com cinco dígitos o locale agrupa, e o dígito a mais sai da
/// primeira parte inteira. Serve ao decimal e ao compacto (`render` devolve `None` se recusa o decimal, e então
/// vale o texto sem a força).
fn with_forced_group(
    always: bool,
    integer: &str,
    fraction: &str,
    render: impl Fn(&Decimal) -> Option<Vec<Part>>,
) -> Option<Vec<Part>> {
    let parts = render(&decimal_of(integer, fraction)?)?;
    let has_group = |parts: &[Part]| parts.iter().any(|(kind, _)| kind == "group");
    if always && integer.len() == 4 && !has_group(&parts) {
        let padded = decimal_of(&format!("1{integer}"), fraction)?;
        if let Some(mut forced) = render(&padded).filter(|forced| has_group(forced)) {
            if let Some((_, text)) = forced.iter_mut().find(|(kind, _)| kind == "integer") {
                *text = text.chars().skip(1).collect();
                return Some(forced);
            }
        }
    }
    Some(parts)
}

/// O `DecimalFormatter` de um locale, de um sistema numérico e de uma estratégia de agrupamento.
pub struct DecimalFormat {
    formatter: DecimalFormatter,
    /// `useGrouping: "always"`: o `DecimalFormatter` o trata como `auto`, então o grupo dos quatro dígitos
    /// que o locale suprime (`minimumGroupingDigits` 2: `es`, `pt-PT`) é forçado em `number`.
    always: bool,
    /// `numberingSystem: "arab"` num locale sem símbolos próprios desse sistema (`en`, `de`, `pt`, `hi`...):
    /// o ICU cai nos símbolos do `root` (`٬`, `٫` e o menos com a marca de letra), medido no bun.
    root_arab_symbols: bool,
}

/// Os símbolos do sistema `arab` no `root` do CLDR: separador de grupo e decimal.
const ROOT_ARAB_GROUP: &str = "\u{66c}";
const ROOT_ARAB_DECIMAL: &str = "\u{66b}";

/// Se o locale define os próprios símbolos do sistema `arab` (o árabe; o icu4x já os traz certos).
fn has_own_arab_symbols(tag: &str) -> bool {
    matches!(tag.split('-').next(), Some("ar" | "fa" | "ur" | "ps" | "ckb" | "ks" | "sd" | "ug" | "uz" | "pa"))
}

impl DecimalFormat {
    /// `tag` é a tag base do locale resolvido (sem `-u-`); `numbering_system` vazio usa o do locale.
    pub fn new(tag: &str, numbering_system: &str, grouping: GroupingStrategy) -> Option<DecimalFormat> {
        let locale = locale_with_numbering(tag, numbering_system)?;
        let always = matches!(grouping, GroupingStrategy::Always);
        let formatter = DecimalFormatter::try_new((&locale).into(), DecimalFormatterOptions::from(grouping)).ok()?;
        let root_arab_symbols = numbering_system == "arab" && !has_own_arab_symbols(tag);
        Some(DecimalFormat { formatter, always, root_arab_symbols })
    }

    /// As partes `integer`, `group`, `decimal` e `fraction` do número sem sinal.
    pub fn number(&self, integer: &str, fraction: &str) -> Option<Vec<Part>> {
        with_forced_group(self.always, integer, fraction, |decimal| {
            let mut parts = named(collect(&self.formatter.format(decimal)), "literal");
            if self.root_arab_symbols {
                for (kind, text) in &mut parts {
                    match kind.as_str() {
                        "group" => *text = ROOT_ARAB_GROUP.to_string(),
                        "decimal" => *text = ROOT_ARAB_DECIMAL.to_string(),
                        _ => {}
                    }
                }
            }
            Some(parts)
        })
    }

    /// O que o locale escreve antes e depois do número para o sinal: o sinal de menos (ou de mais)
    /// com as marcas bidirecionais do locale.
    pub fn sign(&self, negative: bool) -> (Vec<Part>, Vec<Part>) {
        let zero = Decimal::from(0).with_sign(if negative { Sign::Negative } else { Sign::Positive });
        let segments = collect(&self.formatter.format(&zero));
        let Some(digit) = segments.iter().position(|(name, _)| *name == Some("integer")) else {
            return (Vec::new(), Vec::new());
        };
        let suffix = named(segments[digit + 1..].to_vec(), "literal");
        let mut prefix = named(segments[..digit].to_vec(), "literal");
        if self.root_arab_symbols {
            for (_, text) in &mut prefix {
                if !text.starts_with('\u{61c}') {
                    text.insert(0, '\u{61c}');
                }
            }
        }
        (prefix, suffix)
    }
}

/// O texto de `NaN` do locale (o símbolo `nan` do CLDR, que o `icu_decimal` não traz). A maioria dos
/// locales escreve `NaN`; o resto vem da medição contra o bun/ICU.
pub fn nan_symbol(tag: &str) -> &'static str {
    let mut subtags = tag.split('-');
    let language = subtags.next().unwrap_or(tag);
    let rest: Vec<&str> = subtags.collect();
    let traditional = rest.iter().any(|subtag| matches!(*subtag, "Hant" | "TW" | "HK" | "MO"));
    match language {
        "ar" => "\u{644}\u{64a}\u{633}\u{a0}\u{631}\u{642}\u{645}\u{64b}\u{627}",
        "fa" => "\u{646}\u{627}\u{639}\u{62f}\u{62f}",
        "ru" => "\u{43d}\u{435}\u{a0}\u{447}\u{438}\u{441}\u{43b}\u{43e}",
        "my" => "\u{1002}\u{100f}\u{1014}\u{103a}\u{1038}\u{1019}\u{101f}\u{102f}\u{1010}\u{103a}\u{101e}\u{1031}\u{102c}",
        "ka" => "\u{10d0}\u{10e0}\u{a0}\u{10d0}\u{10e0}\u{10d8}\u{10e1}\u{a0}\u{10e0}\u{10d8}\u{10ea}\u{10ee}\u{10d5}\u{10d8}",
        "hy" => "\u{548}\u{579}\u{539}",
        "kk" => "\u{441}\u{430}\u{43d}\u{a0}\u{435}\u{43c}\u{435}\u{441}",
        "ky" => "\u{441}\u{430}\u{43d}\u{a0}\u{44d}\u{43c}\u{435}\u{441}",
        "uz" => "son\u{a0}emas",
        "am" => "\u{1260}\u{1241}\u{1325}\u{122d}\u{a0}\u{120a}\u{1308}\u{1208}\u{133d}\u{a0}\u{12e8}\u{121b}\u{12ed}\u{127d}\u{120d}",
        "yue" if rest.contains(&"Hans") => "\u{975e}\u{6570}\u{503c}",
        "yue" => "\u{975e}\u{6578}\u{503c}",
        "zh" if traditional => "\u{975e}\u{6578}\u{503c}",
        "fi" => "ep\u{e4}luku",
        "lv" => "NS",
        "lo" => "\u{e9a}\u{ecd}\u{ec8}\u{200b}\u{ec1}\u{ea1}\u{ec8}\u{e99}\u{200b}\u{ec2}\u{e95}\u{200b}\u{ec0}\u{ea5}\u{e81}",
        _ => "NaN",
    }
}

/// O sinal de percentual do locale no sistema numérico (vazio: o do locale): `٪` mais a marca de letra
/// árabe em `arab`, `٪` em `arabext`, `%` nos demais.
pub fn percent_sign(tag: &str, numbering_system: &str) -> &'static str {
    let system = if numbering_system.is_empty() { default_numbering_system(tag) } else { numbering_system };
    // Medido no bun: `ar-SA` escreve `٪` (sem marcas) até com dígitos latinos; `ar` e `ar-EG` usam `%`.
    let saudi = tag == "ar-SA" || tag.starts_with("ar-SA-");
    match system {
        "arab" => "\u{66a}\u{61c}",
        "arabext" => "\u{66a}",
        _ if saudi => "\u{66a}",
        _ => "%",
    }
}

/// O dígito zero do locale no sistema numérico dado (vazio: o do locale).
fn zero_digit(tag: &str, numbering_system: &str) -> Option<char> {
    let format = DecimalFormat::new(tag, numbering_system, GroupingStrategy::Never)?;
    let parts = format.number("0", "")?;
    parts.first().and_then(|(_, text)| text.chars().next())
}

/// Se o icu4x tem dígitos para `requested` (o `numberingSystem` ou o `-u-nu-` honrados na resolução).
pub fn numbering_system_honored(tag: &str, requested: &str) -> bool {
    ZERO_DIGITS
        .iter()
        .find(|(name, _)| *name == requested)
        .is_some_and(|&(_, zero)| zero_digit(tag, requested) == Some(zero))
}

/// Os dez dígitos de 0 a 9 do sistema numérico `numbering_system` (vazio: o do locale), um por um, porque
/// `hanidec` não ocupa pontos de código contíguos.
pub fn digits_of(tag: &str, numbering_system: &str) -> Option<[char; 10]> {
    let format = DecimalFormat::new(tag, numbering_system, GroupingStrategy::Never)?;
    let mut digits = ['0'; 10];
    for (value, slot) in digits.iter_mut().enumerate() {
        let parts = format.number(&value.to_string(), "")?;
        *slot = parts.first()?.1.chars().next()?;
    }
    Some(digits)
}

/// O sistema numérico padrão do locale (`latn` em `en`, `fr`, `ar`; `arab` em `ar-EG`; `beng` em `bn`).
pub fn default_numbering_system(tag: &str) -> &'static str {
    let Some(zero) = zero_digit(tag, "") else { return "latn" };
    ZERO_DIGITS.iter().find(|&&(_, digit)| digit == zero).map_or("latn", |&(name, _)| name)
}

/// O `CompactDecimalFormatter` (curto ou longo) de um locale.
pub struct CompactFormat {
    formatter: CompactDecimalFormatter,
    /// `useGrouping: "always"`, como em [`DecimalFormat`]: o significando de quatro dígitos recebe o grupo.
    always: bool,
}

impl CompactFormat {
    pub fn new(tag: &str, numbering_system: &str, grouping: GroupingStrategy, long: bool) -> Option<CompactFormat> {
        let locale = locale_with_numbering(tag, numbering_system)?;
        let options = CompactDecimalFormatterOptions::from(DecimalFormatterOptions::from(grouping));
        let prefs = CompactDecimalFormatterPreferences::from(&locale);
        let formatter = if long {
            CompactDecimalFormatter::try_new_long(prefs, options)
        } else {
            CompactDecimalFormatter::try_new_short(prefs, options)
        }
        .ok()?;
        Some(CompactFormat { formatter, always: matches!(grouping, GroupingStrategy::Always) })
    }

    /// O expoente de dez em que o locale compacta um número da ordem `magnitude` (3 para mil, 6 para
    /// milhão; 4 e 8 em `ja`, 5 e 7 em `hi` para lakh e crore; 0 se o locale não compacta ali).
    pub fn exponent_for_magnitude(&self, magnitude: i32) -> u8 {
        self.formatter.compact_exponent_for_magnitude(magnitude.clamp(i16::MIN as i32, i16::MAX as i32) as i16)
    }

    /// O significando (já dividido por `10^exponent` e arredondado) com o afixo compacto do locale. O texto
    /// fora dos dígitos vira `compact`, e o espaço em volta dele, `literal`. `None` se o expoente não é o
    /// que os dados do locale pedem para essa ordem.
    pub fn format(&self, integer: &str, fraction: &str, exponent: u8) -> Option<Vec<Part>> {
        with_forced_group(self.always, integer, fraction, |significand| {
            let formatted = self.formatter.format_with_exponent(significand, exponent).ok()?;
            Some(split_compact(collect(&formatted)))
        })
    }
}

fn is_space(character: char) -> bool {
    matches!(character, ' ' | '\u{a0}' | '\u{202f}')
}

/// Separa o texto sem parte em `literal` (espaços nas pontas) e `compact` (o resto).
fn split_compact(segments: Segments) -> Vec<Part> {
    let mut parts: Vec<Part> = Vec::new();
    for (name, text) in segments {
        if let Some(name) = name {
            parts.push((name.to_string(), text));
            continue;
        }
        let rest = text.trim_start_matches(is_space);
        let lead = &text[..text.len() - rest.len()];
        let body = rest.trim_end_matches(is_space);
        let trail = &rest[body.len()..];
        for (kind, piece) in [("literal", lead), ("compact", body), ("literal", trail)] {
            if !piece.is_empty() {
                parts.push((kind.to_string(), piece.to_string()));
            }
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

    fn number(tag: &str, numbering_system: &str, grouping: GroupingStrategy, integer: &str, fraction: &str) -> String {
        let format = DecimalFormat::new(tag, numbering_system, grouping).expect("o locale tem dados");
        text(&format.number(integer, fraction).expect("dígitos válidos"))
    }

    #[test]
    fn separators_per_locale() {
        let auto = GroupingStrategy::Auto;
        assert_eq!(number("en", "", auto, "1234567", "891"), "1,234,567.891");
        assert_eq!(number("de", "", auto, "1234567", "891"), "1.234.567,891");
        assert_eq!(number("fr", "", auto, "1234567", "891"), "1\u{202f}234\u{202f}567,891");
        assert_eq!(number("ja", "", auto, "1234567", "891"), "1,234,567.891");
        assert_eq!(number("pt", "", auto, "1234567", "891"), "1.234.567,891");
    }

    #[test]
    fn indian_grouping() {
        let auto = GroupingStrategy::Auto;
        assert_eq!(number("en-IN", "", auto, "12345678", ""), "1,23,45,678");
        assert_eq!(number("hi", "", auto, "1234567", "891"), "12,34,567.891");
    }

    #[test]
    fn minimum_grouping_digits() {
        // O `es` e o `pt-PT` pedem ao menos dois dígitos antes do primeiro grupo.
        assert_eq!(number("es", "", GroupingStrategy::Auto, "1234", ""), "1234");
        assert_eq!(number("es", "", GroupingStrategy::Auto, "12345", ""), "12.345");
        assert_eq!(number("en", "", GroupingStrategy::Min2, "1234", ""), "1234");
        assert_eq!(number("en", "", GroupingStrategy::Min2, "12345", ""), "12,345");
        assert_eq!(number("en", "", GroupingStrategy::Never, "1234567", ""), "1234567");
    }

    #[test]
    fn numbering_systems() {
        let auto = GroupingStrategy::Auto;
        assert_eq!(number("de", "arab", auto, "1234567", "891"), "\u{661}\u{66c}\u{662}\u{663}\u{664}\u{66c}\u{665}\u{666}\u{667}\u{66b}\u{668}\u{669}\u{661}");
        assert_eq!(number("de", "deva", auto, "1234567", "891"), "\u{967}.\u{968}\u{969}\u{96a}.\u{96b}\u{96c}\u{96d},\u{96e}\u{96f}\u{967}");
        assert!(numbering_system_honored("de", "arab"));
        assert!(numbering_system_honored("ar", "latn"));
        assert!(!numbering_system_honored("en", "roman"));
        assert_eq!(default_numbering_system("en"), "latn");
        assert_eq!(default_numbering_system("ar"), "latn");
        assert_eq!(default_numbering_system("ar-EG"), "arab");
    }

    #[test]
    fn signs_carry_the_locale_marks() {
        let format = DecimalFormat::new("en", "", GroupingStrategy::Auto).expect("o locale tem dados");
        assert_eq!(format.sign(true), (vec![("minusSign".to_string(), "-".to_string())], Vec::new()));
        assert_eq!(format.sign(false), (vec![("plusSign".to_string(), "+".to_string())], Vec::new()));
        let arabic = DecimalFormat::new("ar", "", GroupingStrategy::Auto).expect("o locale tem dados");
        let (prefix, _) = arabic.sign(true);
        assert_eq!(text(&prefix), "\u{200e}-");
    }

    #[test]
    fn parts_name_the_pieces() {
        let format = DecimalFormat::new("de", "", GroupingStrategy::Auto).expect("o locale tem dados");
        let parts = format.number("1234", "5").expect("dígitos válidos");
        let kinds: Vec<&str> = parts.iter().map(|(kind, _)| kind.as_str()).collect();
        assert_eq!(kinds, ["integer", "group", "integer", "decimal", "fraction"]);
    }

    #[test]
    fn always_forces_the_four_digit_group() {
        let always = GroupingStrategy::Always;
        assert_eq!(number("es", "", always, "1234", ""), "1.234");
        assert_eq!(number("es", "", always, "12345", ""), "12.345");
        assert_eq!(number("es", "", always, "1234567", ""), "1.234.567");
        assert_eq!(number("pt-PT", "", always, "1234", ""), "1\u{a0}234");
        assert_eq!(number("pt-PT", "", always, "1234", "5"), "1\u{a0}234,5");
        assert_eq!(number("fr", "", always, "1234", ""), "1\u{202f}234");
        assert_eq!(number("en-IN", "", always, "1234", ""), "1,234");
        assert_eq!(number("en-IN", "", always, "1234567", ""), "12,34,567");
        assert_eq!(number("ar-EG", "", always, "1234", ""), "\u{661}\u{66c}\u{662}\u{663}\u{664}");
        assert_eq!(number("de", "", always, "123", ""), "123");
        assert_eq!(number("es", "", GroupingStrategy::Auto, "1234", ""), "1234");
        assert_eq!(number("es", "", GroupingStrategy::Never, "12345", ""), "12345");
    }

    #[test]
    fn always_forces_the_group_in_the_compact_significand() {
        // `es` pede ao menos dois dígitos antes do grupo; `always` o força (1e15 compacta em `1.000 B`).
        let always = CompactFormat::new("es", "", GroupingStrategy::Always, false).expect("o locale tem dados");
        assert_eq!(text(&always.format("1000", "", 12).expect("expoente do locale")), "1.000\u{a0}B");
        let min2 = CompactFormat::new("es", "", GroupingStrategy::Min2, false).expect("o locale tem dados");
        assert_eq!(text(&min2.format("1000", "", 12).expect("expoente do locale")), "1000\u{a0}B");
    }

    #[test]
    fn nan_symbol_per_locale() {
        for tag in ["en", "he", "hi", "de", "ja", "zh", "zh-Hans", "th", "bn", "tr", "pl", "ko", "uk", "sr-Cyrl"] {
            assert_eq!(nan_symbol(tag), "NaN", "{tag}");
        }
        let arabic = "\u{644}\u{64a}\u{633}\u{a0}\u{631}\u{642}\u{645}\u{64b}\u{627}";
        for tag in ["ar", "ar-EG", "ar-MA", "ar-SA"] {
            assert_eq!(nan_symbol(tag), arabic, "{tag}");
        }
        assert_eq!(nan_symbol("fa"), "\u{646}\u{627}\u{639}\u{62f}\u{62f}");
        assert_eq!(nan_symbol("fa-AF"), "\u{646}\u{627}\u{639}\u{62f}\u{62f}");
        assert_eq!(nan_symbol("ru"), "\u{43d}\u{435}\u{a0}\u{447}\u{438}\u{441}\u{43b}\u{43e}");
        assert_eq!(nan_symbol("ru-UA"), "\u{43d}\u{435}\u{a0}\u{447}\u{438}\u{441}\u{43b}\u{43e}");
        assert_eq!(nan_symbol("hy"), "\u{548}\u{579}\u{539}");
        assert_eq!(nan_symbol("uz"), "son\u{a0}emas");
        assert_eq!(nan_symbol("fi"), "ep\u{e4}luku");
        assert_eq!(nan_symbol("lv"), "NS");
        assert_eq!(nan_symbol("zh-TW"), "\u{975e}\u{6578}\u{503c}");
        assert_eq!(nan_symbol("zh-Hant-TW"), "\u{975e}\u{6578}\u{503c}");
        assert_eq!(nan_symbol("yue"), "\u{975e}\u{6578}\u{503c}");
        assert_eq!(nan_symbol("yue-Hans"), "\u{975e}\u{6570}\u{503c}");
    }

    #[test]
    fn percent_sign_per_numbering_system() {
        assert_eq!(percent_sign("ar-EG", ""), "\u{66a}\u{61c}");
        assert_eq!(percent_sign("ar", "arab"), "\u{66a}\u{61c}");
        assert_eq!(percent_sign("ar", "latn"), "%");
        assert_eq!(percent_sign("ar", ""), "%");
        assert_eq!(percent_sign("en", "arab"), "\u{66a}\u{61c}");
        assert_eq!(percent_sign("en", "arabext"), "\u{66a}");
        assert_eq!(percent_sign("fa", ""), "\u{66a}");
        assert_eq!(percent_sign("fa", "latn"), "%");
        assert_eq!(percent_sign("hi", "deva"), "%");
        assert_eq!(percent_sign("de", "thai"), "%");
    }

    #[test]
    fn compact_short_and_long() {
        let short = CompactFormat::new("en", "", GroupingStrategy::Min2, false).expect("o locale tem dados");
        assert_eq!(short.exponent_for_magnitude(3), 3);
        assert_eq!(text(&short.format("1", "2", 3).expect("expoente do locale")), "1.2K");
        let long = CompactFormat::new("fr", "", GroupingStrategy::Min2, true).expect("o locale tem dados");
        assert_eq!(text(&long.format("1", "2", 6).expect("expoente do locale")), "1,2 million");
        let japanese = CompactFormat::new("ja", "", GroupingStrategy::Min2, false).expect("o locale tem dados");
        assert_eq!(japanese.exponent_for_magnitude(8), 8);
        assert_eq!(text(&japanese.format("1", "2", 8).expect("expoente do locale")), "1.2\u{5104}");
    }

    #[test]
    fn compact_parts_split_the_space_from_the_suffix() {
        let short = CompactFormat::new("fr", "", GroupingStrategy::Min2, false).expect("o locale tem dados");
        let parts = short.format("12", "", 3).expect("expoente do locale");
        let kinds: Vec<&str> = parts.iter().map(|(kind, _)| kind.as_str()).collect();
        assert_eq!(kinds, ["integer", "literal", "compact"]);
    }

    #[test]
    fn wrong_compact_exponent_is_refused() {
        let short = CompactFormat::new("en", "", GroupingStrategy::Min2, false).expect("o locale tem dados");
        // Um significando de uma casa inteira só compacta em 3 (mil) ou em 6 (milhão), não em 4.
        assert!(short.format("1", "2", 4).is_none());
    }
}
