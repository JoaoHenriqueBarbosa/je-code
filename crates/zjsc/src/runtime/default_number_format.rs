//! A formatação de número do `Intl.NumberFormat` sem ICU (o `UNumberFormatter` do `unumf_*`): a
//! conversão do número nos dígitos decimais mais curtos que o identificam, o arredondamento (os nove
//! `roundingMode`, dígitos fracionários e significativos, `roundingPriority`), o agrupamento, as
//! notações (padrão, científica, de engenharia e compacta), o estilo (decimal, porcentagem, moeda e
//! unidade) e a lista de partes que o `formatToParts` devolve.
//!
//! O que o ICU faz com um `double`, e este módulo reproduz: pega os dígitos decimais mais curtos que
//! identificam o `double` (o mesmo texto de `String(x)`, e não o valor binário exato: `1.0005` vira
//! `1.001`, enquanto `toFixed(3)` dá `1.000`) e arredonda esses dígitos (`roundingMode: "halfExpand"`,
//! `maximumFractionDigits: 3` no padrão). Um BigInt entra pelos dígitos exatos.
//!
//! LOCALES: os símbolos (decimal, grupo, sinais), o agrupamento (lakh e crore em `en-IN`,
//! `minimumGroupingDigits` do `es` e do `pt-PT`), os dígitos do sistema numérico (`-u-nu-`/`numberingSystem`)
//! e a notação compacta curta e longa vêm do CLDR pelo `icu_decimal` (`icu_number.rs`), para qualquer
//! locale. O percentual tem a tabela de padrões de [`percent_pattern`].
//!
//! DIVERGÊNCIAS e LACUNAS, e por quê (o `icu_decimal` só traz decimal e compacto):
//!
//! - `roundingIncrement` (só com dígitos fracionários fixos, como o ECMA-402 exige) arredonda ao múltiplo
//!   `incremento * 10^-casas` em `round_to_increment`, com os nove `roundingMode`.
//! - Moedas: o símbolo, o nome e os dígitos fracionários existem para as moedas mais comuns (tabelas
//!   abaixo), em inglês e português do Brasil; as demais saem pelo código ISO, com os dois dígitos
//!   fracionários do padrão. O lado do símbolo (`1.234,50 €` em `de`) e os nomes por locale dependem do
//!   `icu_experimental` (ainda não baixado): em outro locale a moeda sai no padrão do inglês.
//! - Unidades: as simples saem da tabela gerada `UNITS` de `icu_number_data` (a de `en` inclusa, que também
//!   serve de reserva aos locales sem entrada); as compostas somam o texto do numerador ao sufixo "por unidade".
//! - O texto de `NaN` por locale e o sinal de percentual por sistema numérico vêm de tabelas medidas no
//!   bun/ICU (`icu_number::nan_symbol` e `percent_sign`), só para os locales medidos; o `ckb` (percentual
//!   com espaço) e os demais padrões de percentual fora de `percent_pattern` seguem o do inglês.
//! - `useGrouping: "always"` força o grupo dos quatro dígitos onde o locale pede mínimo de 2 (`es`,
//!   `pt-PT`), em `DecimalFormat::number` e em `CompactFormat::format` (o significando de quatro dígitos).
//! - Se o locale não tem dados de compacto no icu4x, o número sai sem compactar.

use crate::runtime::icu_number::{default_numbering_system, nan_symbol, percent_sign, plain_parts, CompactFormat, DecimalFormat, GroupingStrategy};
use crate::runtime::icu_number_patterns::{self as patterns, Fill};
use crate::runtime::icu_plural;
use crate::runtime::intl_locale_data::Language;
use crate::runtime::intl_plural_rules::{cardinal_category, PluralOperands};
use crate::runtime::intl_supported_values_data::UNITS as SIMPLE_UNITS;
use crate::runtime::intl_support::IntlEnum;
use crate::wtf::dtoa::{number_to_string_and_size, NumberToStringBuffer};

crate::intl_enum!(Style { Decimal => "decimal", Percent => "percent", Currency => "currency", Unit => "unit" });
crate::intl_enum!(CurrencyDisplay {
    Code => "code", Symbol => "symbol", NarrowSymbol => "narrowSymbol", Name => "name"
});
crate::intl_enum!(CurrencySign { Standard => "standard", Accounting => "accounting" });
crate::intl_enum!(UnitDisplay { Short => "short", Narrow => "narrow", Long => "long" });
crate::intl_enum!(Notation {
    Standard => "standard", Scientific => "scientific", Engineering => "engineering", Compact => "compact"
});
crate::intl_enum!(CompactDisplay { Short => "short", Long => "long" });
crate::intl_enum!(SignDisplay {
    Auto => "auto", Never => "never", Always => "always", ExceptZero => "exceptZero", Negative => "negative"
});
crate::intl_enum!(RoundingMode {
    Ceil => "ceil", Floor => "floor", Expand => "expand", Trunc => "trunc", HalfCeil => "halfCeil",
    HalfFloor => "halfFloor", HalfExpand => "halfExpand", HalfTrunc => "halfTrunc", HalfEven => "halfEven"
});
crate::intl_enum!(RoundingPriority { Auto => "auto", MorePrecision => "morePrecision", LessPrecision => "lessPrecision" });
crate::intl_enum!(TrailingZeroDisplay { Auto => "auto", StripIfInteger => "stripIfInteger" });

/// `UseGrouping`: o valor de `useGrouping` (`false`, `"min2"`, `"auto"` ou `"always"`).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum UseGrouping {
    False,
    Min2,
    Auto,
    Always,
}

/// O que o arredondamento mantém (`IntlRoundingType`).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Rounding {
    FractionDigits { min: u32, max: u32 },
    SignificantDigits { min: u32, max: u32 },
    MorePrecision { min_fraction: u32, max_fraction: u32, min_significant: u32, max_significant: u32 },
    LessPrecision { min_fraction: u32, max_fraction: u32, min_significant: u32, max_significant: u32 },
}

/// Tudo que o formatador lê (os campos `m_*` de `IntlNumberFormat`).
#[derive(Clone, Debug)]
pub struct NumberSettings {
    pub language: Language,
    /// A tag base do locale resolvido (`de`, `en-GB`, sem `-u-`): de onde o icu4x lê os símbolos.
    pub locale: String,
    /// O sistema numérico honrado (`arab`, `deva`); vazio usa o padrão do locale.
    pub numbering_system: String,
    pub style: Style,
    pub currency: String,
    pub currency_display: CurrencyDisplay,
    pub currency_sign: CurrencySign,
    pub unit: String,
    pub unit_display: UnitDisplay,
    pub notation: Notation,
    pub compact_display: CompactDisplay,
    pub minimum_integer_digits: u32,
    pub rounding: Rounding,
    pub rounding_mode: RoundingMode,
    pub rounding_increment: u32,
    pub trailing_zero_display: TrailingZeroDisplay,
    pub use_grouping: UseGrouping,
    pub sign_display: SignDisplay,
}

impl NumberSettings {
    /// O `defaultNumberFormat()` do `JSGlobalObject`: `Intl.NumberFormat` sem `locales` nem `options`.
    pub fn defaults(language: Language) -> NumberSettings {
        NumberSettings {
            language,
            locale: match language {
                Language::English => "en",
                Language::Portuguese => "pt",
            }
            .to_string(),
            numbering_system: String::new(),
            style: Style::Decimal,
            currency: String::new(),
            currency_display: CurrencyDisplay::Symbol,
            currency_sign: CurrencySign::Standard,
            unit: String::new(),
            unit_display: UnitDisplay::Short,
            notation: Notation::Standard,
            compact_display: CompactDisplay::Short,
            minimum_integer_digits: 1,
            rounding: Rounding::FractionDigits { min: 0, max: 3 },
            rounding_mode: RoundingMode::HalfExpand,
            rounding_increment: 1,
            trailing_zero_display: TrailingZeroDisplay::Auto,
            use_grouping: UseGrouping::Auto,
            sign_display: SignDisplay::Auto,
        }
    }
}

/// O número a formatar.
pub enum NumericInput {
    Double(f64),
    /// O BigInt: o sinal e os dígitos decimais exatos (sem sinal).
    Decimal { negative: bool, digits: String },
}

/// Uma parte do `formatToParts`: o tipo e o texto.
pub type Part = (String, String);

// ---------------------------------------------------------------------------------------------
// Dígitos
// ---------------------------------------------------------------------------------------------

/// Os dígitos decimais de um número finito e não negativo: o valor é `0.d0d1d2... * 10^point`, sem zeros
/// à esquerda nem à direita. Zero é a lista vazia com `point` 0.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Digits {
    pub digits: Vec<u8>,
    pub point: i32,
}

impl Digits {
    /// Os dígitos mais curtos de `value` (o texto de `String(x)`).
    pub fn shortest(value: f64) -> Digits {
        let mut buffer: NumberToStringBuffer = [0; 124];
        let text = number_to_string_and_size(value, &mut buffer);

        let (mantissa, exponent) = match text.iter().position(|&c| c == b'e') {
            Some(index) => {
                let exponent: i32 = std::str::from_utf8(&text[index + 1..])
                    .ok()
                    .and_then(|digits| digits.parse().ok())
                    .expect("o expoente de String(x) é um inteiro com sinal");
                (&text[..index], exponent)
            }
            None => (text, 0),
        };

        let integer_length = mantissa.iter().position(|&c| c == b'.').unwrap_or(mantissa.len());
        let digits: Vec<u8> = mantissa.iter().copied().filter(|c| c.is_ascii_digit()).collect();
        Digits::normalized(digits, integer_length as i32 + exponent)
    }

    /// Os dígitos de um decimal em texto (o BigInt, ou o `unumf_formatDecimal` do `Intl.DurationFormat`, que
    /// passa `"12.000345"`): o ponto, se houver, separa a parte inteira da fração.
    pub fn from_integer(text: &str) -> Digits {
        let integer_length = text.bytes().position(|c| c == b'.').unwrap_or(text.len());
        let point = text.bytes().take(integer_length).filter(u8::is_ascii_digit).count() as i32;
        let digits: Vec<u8> = text.bytes().filter(u8::is_ascii_digit).collect();
        Digits::normalized(digits, point)
    }

    /// Tira os zeros da frente (cada um tira uma casa do expoente) e os de trás.
    fn normalized(mut digits: Vec<u8>, mut point: i32) -> Digits {
        let leading_zeros = digits.iter().take_while(|&&c| c == b'0').count();
        digits.drain(..leading_zeros);
        point -= leading_zeros as i32;
        while digits.last() == Some(&b'0') {
            digits.pop();
        }
        if digits.is_empty() {
            point = 0;
        }
        Digits { digits, point }
    }

    pub fn is_zero(&self) -> bool {
        self.digits.is_empty()
    }

    /// O expoente de `floor(log10(x))` (0 para zero).
    fn magnitude(&self) -> i32 {
        if self.is_zero() { 0 } else { self.point - 1 }
    }

    /// Arredonda para manter `keep` dígitos a partir do primeiro (`keep` pode ser zero ou negativo: a
    /// unidade de arredondamento fica à esquerda do primeiro dígito).
    fn round_keeping(&mut self, keep: i32, mode: RoundingMode, negative: bool) {
        if keep >= self.digits.len() as i32 {
            return;
        }
        // Compara o resto com a metade da unidade.
        let (half, nonzero) = if keep < 0 {
            (std::cmp::Ordering::Less, true)
        } else {
            let remainder = &self.digits[keep as usize..];
            let nonzero = remainder.iter().any(|&digit| digit != b'0');
            let half = match remainder[0].cmp(&b'5') {
                std::cmp::Ordering::Equal if remainder[1..].iter().any(|&digit| digit != b'0') => std::cmp::Ordering::Greater,
                other => other,
            };
            (half, nonzero)
        };
        let last_kept_is_odd = keep > 0 && (self.digits[keep as usize - 1] - b'0') % 2 == 1;
        let round_up = rounds_up(mode, negative, nonzero, half, last_kept_is_odd);

        if keep <= 0 {
            if round_up {
                self.point = self.point - keep + 1;
                self.digits = vec![b'1'];
            } else {
                *self = Digits { digits: Vec::new(), point: 0 };
            }
            return;
        }
        self.digits.truncate(keep as usize);
        if round_up {
            let mut carry = true;
            for digit in self.digits.iter_mut().rev() {
                if *digit == b'9' {
                    *digit = b'0';
                } else {
                    *digit += 1;
                    carry = false;
                    break;
                }
            }
            if carry {
                self.digits.insert(0, b'1');
                self.point += 1;
            }
        }
        *self = Digits::normalized(std::mem::take(&mut self.digits), self.point);
    }
}

/// A decisão dos nove `roundingMode`: `nonzero` (há resto), `half` (o resto contra a metade da unidade) e
/// se o último dígito (ou múltiplo) mantido é ímpar.
fn rounds_up(mode: RoundingMode, negative: bool, nonzero: bool, half: std::cmp::Ordering, kept_is_odd: bool) -> bool {
    use std::cmp::Ordering::{Equal, Greater, Less};
    match mode {
        RoundingMode::Ceil => nonzero && !negative,
        RoundingMode::Floor => nonzero && negative,
        RoundingMode::Expand => nonzero,
        RoundingMode::Trunc => false,
        RoundingMode::HalfCeil => half == Greater || (half == Equal && !negative),
        RoundingMode::HalfFloor => half == Greater || (half == Equal && negative),
        RoundingMode::HalfExpand => half != Less,
        RoundingMode::HalfTrunc => half == Greater,
        RoundingMode::HalfEven => half == Greater || (half == Equal && kept_is_odd),
    }
}

/// Divide o inteiro decimal `integer` (dígitos ASCII) por `divisor`: o quociente (dígitos) e o resto.
fn divide_small(integer: &[u8], divisor: u32) -> (Vec<u8>, u32) {
    let mut remainder = 0u32;
    let mut quotient = Vec::with_capacity(integer.len());
    for &digit in integer {
        let current = remainder * 10 + u32::from(digit - b'0');
        quotient.push(b'0' + (current / divisor) as u8);
        remainder = current % divisor;
    }
    (quotient, remainder)
}

/// Arredonda `digits` ao múltiplo de `increment * 10^-fraction` (o `roundingIncrement` do ECMA-402 com
/// dígitos fracionários fixos): `I` é o valor em unidades de `10^-fraction`, e o resto de `I` por
/// `increment`, somado à cauda fracionária, decide para que lado vai.
fn round_to_increment(digits: &Digits, fraction: u32, increment: u32, mode: RoundingMode, negative: bool) -> Digits {
    use std::cmp::Ordering::{Equal, Greater, Less};
    let boundary = digits.point + fraction as i32;
    let mut full: Vec<u8> = Vec::new();
    if boundary < 0 {
        full.resize((-boundary) as usize, b'0');
    }
    full.extend_from_slice(&digits.digits);
    let split = boundary.max(0) as usize;
    if full.len() < split {
        full.resize(split, b'0');
    }
    let (integer, tail) = full.split_at(split);
    let tail_nonzero = tail.iter().any(|&digit| digit != b'0');

    let (quotient, remainder) = divide_small(integer, increment);
    let nonzero = remainder > 0 || tail_nonzero;
    // Compara `remainder + tail` com `increment / 2`, em `diff = increment - 2 * remainder`.
    let diff = i64::from(increment) - 2 * i64::from(remainder);
    let half = if diff <= 0 {
        if diff == 0 && !tail_nonzero { Equal } else { Greater }
    } else if diff >= 2 {
        Less
    } else {
        // diff == 1: a cauda decide contra 0.5.
        match tail.first().copied().unwrap_or(b'0').cmp(&b'5') {
            Equal if tail[1..].iter().any(|&digit| digit != b'0') => Greater,
            other => other,
        }
    };
    let kept_is_odd = quotient.last().is_some_and(|&digit| (digit - b'0') % 2 == 1);
    let mut steps = quotient;
    if rounds_up(mode, negative, nonzero, half, kept_is_odd) {
        let mut carry = true;
        for digit in steps.iter_mut().rev() {
            if *digit == b'9' {
                *digit = b'0';
            } else {
                *digit += 1;
                carry = false;
                break;
            }
        }
        if carry {
            steps.insert(0, b'1');
        }
    }
    // steps * increment, dígito a dígito da direita.
    let mut product: Vec<u8> = Vec::with_capacity(steps.len() + 4);
    let mut carry = 0u32;
    for &digit in steps.iter().rev() {
        let current = u32::from(digit - b'0') * increment + carry;
        product.push(b'0' + (current % 10) as u8);
        carry = current / 10;
    }
    while carry > 0 {
        product.push(b'0' + (carry % 10) as u8);
        carry /= 10;
    }
    product.reverse();
    let point = product.len() as i32 - fraction as i32;
    Digits::normalized(product, point)
}

/// O resultado do arredondamento: os dígitos e quantas casas fracionárias no mínimo.
struct RoundedNumber {
    digits: Digits,
    minimum_fraction: u32,
}

/// `unumf` com `Rounding`: os dígitos arredondados e o mínimo de casas a mostrar.
fn round_number(digits: &Digits, rounding: Rounding, mode: RoundingMode, negative: bool, increment: u32) -> RoundedNumber {
    if let (Rounding::FractionDigits { min, max }, true) = (rounding, increment != 1) {
        return RoundedNumber { digits: round_to_increment(digits, max, increment, mode, negative), minimum_fraction: min };
    }
    let by_fraction = |min: u32, max: u32| {
        let mut rounded = digits.clone();
        rounded.round_keeping(digits.point + max as i32, mode, negative);
        RoundedNumber { digits: rounded, minimum_fraction: min }
    };
    let by_significant = |min: u32, max: u32| {
        let mut rounded = digits.clone();
        rounded.round_keeping(max as i32, mode, negative);
        // `min` dígitos significativos viram casas fracionárias: o primeiro dígito está em `point`.
        let point = if rounded.is_zero() { 1 } else { rounded.point };
        RoundedNumber { digits: rounded, minimum_fraction: (min as i32 - point).max(0) as u32 }
    };
    match rounding {
        Rounding::FractionDigits { min, max } => by_fraction(min, max),
        Rounding::SignificantDigits { min, max } => by_significant(min, max),
        Rounding::MorePrecision { min_fraction, max_fraction, min_significant, max_significant }
        | Rounding::LessPrecision { min_fraction, max_fraction, min_significant, max_significant } => {
            let significant_magnitude = (if digits.is_zero() { 1 } else { digits.point }) - max_significant as i32;
            let fraction_magnitude = -(max_fraction as i32);
            let more = matches!(rounding, Rounding::MorePrecision { .. });
            let use_significant = (significant_magnitude <= fraction_magnitude) == more;
            if use_significant {
                by_significant(min_significant, max_significant)
            } else {
                by_fraction(min_fraction, max_fraction)
            }
        }
    }
}

// ---------------------------------------------------------------------------------------------
// Tabelas de moeda e de unidade
// ---------------------------------------------------------------------------------------------

/// Dígitos fracionários da moeda (`computeCurrencyDigits`, ISO 4217).
pub fn currency_digits(code: &str) -> u32 {
    match code {
        "JPY" | "KRW" | "VND" | "CLP" | "ISK" | "UGX" | "XAF" | "XOF" | "XPF" | "PYG" | "RWF" | "KMF" | "GNF" | "DJF" | "VUV" => 0,
        "BHD" | "KWD" | "OMR" | "JOD" | "TND" | "IQD" | "LYD" => 3,
        _ => 2,
    }
}

/// Os códigos das moedas que têm símbolo ou nome nas tabelas (`Intl.supportedValuesOf("currency")`).
pub const KNOWN_CURRENCIES: [&str; 20] = [
    "AUD", "BRL", "CAD", "CHF", "CNY", "EUR", "GBP", "HKD", "ILS", "INR", "JPY", "KRW", "MXN", "NZD", "PHP", "TWD", "USD", "VND",
    "XAF", "XOF",
];

/// O símbolo da moeda no inglês e no português do Brasil.
pub fn currency_symbol(code: &str, language: Language) -> Option<&'static str> {
    let english = match code {
        "USD" => "$",
        "EUR" => "\u{20ac}",
        "BRL" => "R$",
        "GBP" => "\u{a3}",
        "JPY" => "\u{a5}",
        "CNY" => "CN\u{a5}",
        "INR" => "\u{20b9}",
        "KRW" => "\u{20a9}",
        "MXN" => "MX$",
        "CAD" => "CA$",
        "AUD" => "A$",
        "NZD" => "NZ$",
        "HKD" => "HK$",
        "ILS" => "\u{20aa}",
        "VND" => "\u{20ab}",
        "TWD" => "NT$",
        "PHP" => "\u{20b1}",
        "XAF" => "FCFA",
        "XOF" => "F\u{202f}CFA",
        "XPF" => "CFPF",
        _ => {
            // As moedas fora das tabelas acima: o símbolo de `en` medido no bun (vazio quando é o código).
            let symbol = patterns::extra_currency("en", code).map(|entry| entry.symbol()).filter(|text| !text.is_empty());
            return if language == Language::English { symbol } else { None };
        }
    };
    if language == Language::Portuguese {
        return Some(match code {
            "USD" => "US$",
            "JPY" => "JP\u{a5}",
            "CAD" => "CA$",
            _ => english,
        });
    }
    Some(english)
}

/// O símbolo estreito (`narrowSymbol`): o símbolo sem o prefixo de país.
pub fn currency_narrow_symbol(code: &str, language: Language) -> Option<&'static str> {
    if language == Language::English && !KNOWN_CURRENCIES.contains(&code) {
        let narrow = patterns::extra_currency("en", code).map(|entry| entry.narrow()).filter(|text| !text.is_empty());
        if narrow.is_some() {
            return narrow;
        }
    }
    let symbol = currency_symbol(code, language)?;
    Some(match symbol {
        "CA$" | "A$" | "NZ$" | "HK$" | "MX$" | "NT$" | "US$" => "$",
        "CN\u{a5}" | "JP\u{a5}" => "\u{a5}",
        other => other,
    })
}

/// O nome da moeda (`currencyDisplay: "name"`), no singular e no plural do inglês.
fn currency_name(code: &str, plural: bool) -> Option<&'static str> {
    let (one, other) = match code {
        "USD" => ("US dollar", "US dollars"),
        "EUR" => ("euro", "euros"),
        "BRL" => ("Brazilian real", "Brazilian reals"),
        "GBP" => ("British pound", "British pounds"),
        "JPY" => ("Japanese yen", "Japanese yen"),
        "CNY" => ("Chinese yuan", "Chinese yuan"),
        "INR" => ("Indian rupee", "Indian rupees"),
        "CAD" => ("Canadian dollar", "Canadian dollars"),
        "AUD" => ("Australian dollar", "Australian dollars"),
        "MXN" => ("Mexican peso", "Mexican pesos"),
        "CHF" => ("Swiss franc", "Swiss francs"),
        _ => {
            let category = if plural { "other" } else { "one" };
            return patterns::extra_currency("en", code).and_then(|entry| entry.name_for(category));
        }
    };
    Some(if plural { other } else { one })
}

/// A unidade pelo identificador simples (`kilometer`) ou composto (`kilometer-per-hour`).
pub fn is_known_unit(id: &str) -> bool {
    let simple = |id: &str| SIMPLE_UNITS.contains(&id);
    match id.split_once("-per-") {
        Some((numerator, denominator)) => simple(numerator) && simple(denominator),
        None => simple(id),
    }
}

/// O texto de uma unidade simples em `en` (a parte `unit` do padrão gerado) e se ele cola no número
/// (o padrão não tem `literal` entre os dois), no `unitDisplay` (0 long, 1 short, 2 narrow) e na forma pedidos.
fn simple_unit_text(id: &str, display_index: u8, plural: bool) -> Option<(String, bool)> {
    let entry = patterns::unit("en", id, display_index, if plural { "other" } else { "one" })?;
    let fill = Fill { number: &[], sign_prefix: &[], sign_suffix: &[], currency: None, percent: None };
    let parts = patterns::render(entry.template, &fill);
    let attached = !parts.iter().any(|(kind, _)| kind == "literal");
    let text = parts.into_iter().find(|(kind, _)| kind == "unit")?.1;
    Some((text, attached))
}

/// O texto da unidade (simples ou `a-per-b`) no `unitDisplay` e na forma (singular ou plural) pedidos.
/// A unidade composta usa o `per` do CLDR (`km/h`, `kilometers per hour`).
fn unit_text(id: &str, display: UnitDisplay, plural: bool) -> (String, bool) {
    let display_index = match display {
        UnitDisplay::Long => 0,
        UnitDisplay::Short => 1,
        UnitDisplay::Narrow => 2,
    };
    match id.split_once("-per-") {
        Some((numerator, denominator)) => {
            let (numerator_text, _) = simple_unit_text(numerator, display_index, plural).expect("unidade validada");
            let text = match patterns::compound_unit_text(numerator, denominator, display_index, plural) {
                Some(measured) => measured.to_string(),
                None => {
                    let suffix = patterns::per_unit_suffix("en", denominator, display_index).expect("denominador medido no bun");
                    format!("{numerator_text}{suffix}")
                }
            };
            (text, display == UnitDisplay::Narrow)
        }
        None => simple_unit_text(id, display_index, plural).expect("unidade validada"),
    }
}

// ---------------------------------------------------------------------------------------------
// Formatação
// ---------------------------------------------------------------------------------------------

const NO_BREAK_SPACE: &str = "\u{a0}";

/// O `useGrouping` do JS na estratégia de agrupamento do icu4x.
fn grouping_strategy(grouping: UseGrouping) -> GroupingStrategy {
    match grouping {
        UseGrouping::False => GroupingStrategy::Never,
        UseGrouping::Min2 => GroupingStrategy::Min2,
        UseGrouping::Auto => GroupingStrategy::Auto,
        UseGrouping::Always => GroupingStrategy::Always,
    }
}

/// A parte inteira e a fração com o agrupamento do locale: as partes `integer`, `group`, `decimal` e
/// `fraction`. Sem dados do locale, os dígitos saem sem grupo e com o ponto decimal.
fn number_parts(format: Option<&DecimalFormat>, integer: &str, fraction: &str) -> Vec<Part> {
    format.and_then(|format| format.number(integer, fraction)).unwrap_or_else(|| plain_parts(integer, fraction))
}

/// O sinal do locale (`-`, `+` e as marcas bidirecionais do `ar`) antes e depois do número.
fn sign_parts(format: Option<&DecimalFormat>, negative: bool) -> (Vec<Part>, Vec<Part>) {
    match format {
        Some(format) => format.sign(negative),
        None if negative => (vec![("minusSign".to_string(), "-".to_string())], Vec::new()),
        None => (vec![("plusSign".to_string(), "+".to_string())], Vec::new()),
    }
}

/// O padrão do percentual do locale (`percentFormat` do CLDR): o sinal depois do número e o que fica
/// entre os dois e depois do sinal; `sign_first` põe o `%` antes do número (`tr`: `%12`).
struct PercentPattern {
    sign_first: bool,
    gap: &'static str,
    trailing: &'static str,
}

/// O padrão de percentual do locale. O `icu_decimal` não traz padrões com afixos, então só as línguas
/// que diferem do `#,##0%` do inglês estão aqui: espaço antes do `%` (`fr`, `de`, `es`, `ru`, `sv`,
/// `nb`, `da`, `fi`, `cs`, `sk`, `uk`, `bg`), `%` na frente (`tr`) e as marcas bidirecionais do `ar`.
fn percent_pattern(locale: &str) -> PercentPattern {
    let language = locale.split('-').next().unwrap_or(locale);
    match language {
        "fr" | "de" | "es" | "ru" | "sv" | "nb" | "no" | "da" | "fi" | "cs" | "sk" | "uk" | "bg" => {
            PercentPattern { sign_first: false, gap: NO_BREAK_SPACE, trailing: "" }
        }
        "tr" => PercentPattern { sign_first: true, gap: "", trailing: "" },
        "ar" => PercentPattern { sign_first: false, gap: "\u{200e}", trailing: "\u{200e}" },
        _ => PercentPattern { sign_first: false, gap: "", trailing: "" },
    }
}

/// O número com o sinal de percentual no padrão do locale.
fn percent_parts(locale: &str, numbering_system: &str, compact: bool, number: Vec<Part>) -> Vec<Part> {
    let symbol = percent_sign(locale, numbering_system);
    // O padrão compacto (o da unidade `percent`) só foi medido no sistema numérico do locale.
    let compact = compact && (numbering_system.is_empty() || numbering_system == default_numbering_system(locale));
    // `ar-EG` com dígitos latinos usa o padrão de `ar` (`50‎%‎`, com U+200E), medido no bun.
    let entry = if symbol == "%" && locale.starts_with("ar-") { patterns::percent("ar", compact) } else { patterns::percent(locale, compact) };
    if let Some(entry) = entry {
        let fill = Fill { number: &number, sign_prefix: &[], sign_suffix: &[], currency: None, percent: Some(symbol) };
        let mut parts = patterns::render(entry.template, &fill);
        // Com o sinal de outro sistema numérico (árabe), as marcas do padrão latino saem; e a marca de
        // letra árabe do padrão `arab` só fica quando o sinal é o `٪` com ela (`ar-SA` latn: `50٪`).
        if symbol != "%" {
            parts.retain(|(kind, text)| !(kind == "literal" && text == "\u{200e}"));
        }
        // A marca de letra do padrão sai sempre: quando o sinal é o `٪` com ela, ela já vem no próprio
        // sinal (medido no bun: `١٢٣٬٤٥٠٪؜`, uma só marca), e senão não há marca nenhuma.
        parts.retain(|(kind, text)| !(kind == "literal" && text == "\u{61c}"));
        return parts;
    }
    let mut pattern = percent_pattern(locale);
    // O árabe com dígitos árabes já leva a marca de letra no próprio sinal, sem as marcas do latino.
    if symbol != "%" && pattern.trailing == "\u{200e}" {
        pattern = PercentPattern { sign_first: false, gap: "", trailing: "" };
    }
    let literal = |text: &str| ("literal".to_string(), text.to_string());
    let sign = ("percentSign".to_string(), symbol.to_string());
    let mut parts: Vec<Part> = Vec::new();
    if pattern.sign_first {
        parts.push(sign);
        parts.extend(number);
        return parts;
    }
    parts.extend(number);
    if !pattern.gap.is_empty() {
        parts.push(literal(pattern.gap));
    }
    parts.push(sign);
    if !pattern.trailing.is_empty() {
        parts.push(literal(pattern.trailing));
    }
    parts
}

/// A parte inteira e a fração do número arredondado (com `minimumIntegerDigits` e `minimumFraction`).
fn integer_and_fraction(rounded: &RoundedNumber, settings: &NumberSettings) -> (String, String) {
    let digits = &rounded.digits;
    let mut integer = String::new();
    if digits.point > 0 {
        for index in 0..digits.point as usize {
            integer.push(digits.digits.get(index).copied().unwrap_or(b'0') as char);
        }
    }
    while integer.len() < settings.minimum_integer_digits as usize {
        integer.insert(0, '0');
    }
    let mut fraction = String::new();
    if digits.point < 0 {
        fraction.extend(std::iter::repeat_n('0', (-digits.point) as usize));
    }
    let integer_length = digits.point.max(0) as usize;
    if digits.digits.len() > integer_length {
        fraction.extend(digits.digits[integer_length..].iter().map(|&digit| digit as char));
    }
    while fraction.len() < rounded.minimum_fraction as usize {
        fraction.push('0');
    }
    if settings.trailing_zero_display == TrailingZeroDisplay::StripIfInteger && fraction.bytes().all(|digit| digit == b'0') {
        fraction.clear();
    }
    (integer, fraction)
}

/// Os dígitos do significando de volta na ordem original (`1`, `.5` e expoente 3 viram `1500`): os operandos
/// de plural do compacto.
fn unscale_digits(integer: &str, fraction: &str, exponent: u8) -> (String, String) {
    let exponent = exponent as usize;
    if exponent == 0 {
        return (integer.to_string(), fraction.to_string());
    }
    let mut digits = format!("{integer}{fraction}");
    let point = integer.len() + exponent;
    while digits.len() < point {
        digits.push('0');
    }
    let (whole, rest) = digits.split_at(point);
    (whole.to_string(), rest.to_string())
}

/// `unumf_formatDouble`/`unumf_formatDecimal` com os campos: as partes do número formatado.
pub fn format_parts(settings: &NumberSettings, input: &NumericInput) -> Vec<Part> {
    format_parts_with_exponent(settings, input).0
}

/// Como [`format_parts`], devolvendo também o expoente de dez que a notação mostra (o da científica ou o do compacto,
/// já com a subida de ordem do arredondamento, `999999` em `1M`): o operando `e` das regras de plural do ICU4C.
pub fn format_parts_with_exponent(settings: &NumberSettings, input: &NumericInput) -> (Vec<Part>, i32) {
    let (mut negative, mut digits, special) = match input {
        NumericInput::Double(value) => {
            if value.is_nan() {
                (false, Digits { digits: Vec::new(), point: 0 }, Some("nan"))
            } else if value.is_infinite() {
                (value.is_sign_negative(), Digits { digits: Vec::new(), point: 0 }, Some("infinity"))
            } else {
                (value.is_sign_negative(), Digits::shortest(value.abs()), None)
            }
        }
        NumericInput::Decimal { negative, digits } => (*negative, Digits::from_integer(digits), None),
    };
    if settings.style == Style::Percent && !digits.is_zero() {
        digits.point += 2;
    }

    let strategy = grouping_strategy(settings.use_grouping);
    let decimal_format = DecimalFormat::new(&settings.locale, &settings.numbering_system, strategy);
    let compact = if settings.notation == Notation::Compact && special.is_none() {
        // Moeda por símbolo ou código usa sempre o formato curto; só o nome da moeda acompanha o
        // `compactDisplay: "long"` (medido no bun, en/pt/fr).
        let long_compact = settings.compact_display == CompactDisplay::Long
            && (settings.style != Style::Currency || settings.currency_display == CurrencyDisplay::Name);
        CompactFormat::new(&settings.locale, &settings.numbering_system, strategy, long_compact)
    } else {
        None
    };

    // Notação: o expoente (científica e de engenharia) ou o expoente de dez do compacto, que vem dos
    // dados do locale (3 para mil, 4 para a miríade do `ja`, 5 para o lakh do `hi`).
    let mut exponent = 0i32;
    let mut compact_exponent = 0u8;
    let mut compact_magnitude = digits.magnitude();
    let mut rounded;
    // O passo a mais que o arredondamento pode exigir (9.99 vira 10 na científica; 999999 vira 1000K).
    let mut bump = 0;
    loop {
        let mut scaled = digits.clone();
        match settings.notation {
            Notation::Standard => {}
            Notation::Scientific | Notation::Engineering => {
                if !scaled.is_zero() {
                    let magnitude = scaled.magnitude();
                    exponent =
                        if settings.notation == Notation::Scientific { magnitude + bump } else { magnitude.div_euclid(3) * 3 + 3 * bump };
                    scaled.point -= exponent;
                }
            }
            Notation::Compact => {
                if let (Some(compact), false) = (&compact, scaled.is_zero()) {
                    compact_exponent = compact.exponent_for_magnitude(compact_magnitude);
                    scaled.point -= compact_exponent as i32;
                }
            }
        }
        rounded = round_number(&scaled, settings.rounding, settings.rounding_mode, negative, settings.rounding_increment);
        let overflow = !scaled.is_zero()
            && match settings.notation {
                Notation::Standard | Notation::Compact => false,
                Notation::Scientific => rounded.digits.point > 1,
                Notation::Engineering => rounded.digits.point > 3,
            };
        if overflow && bump == 0 {
            bump = 1;
            continue;
        }
        // O arredondamento subiu de ordem (999999 vira 1000K): refaz na ordem seguinte se o locale
        // compacta ali de outro jeito (1M).
        if let (Some(compact), false, 0) = (&compact, scaled.is_zero(), bump) {
            let total = rounded.digits.magnitude() + compact_exponent as i32;
            if total > compact_magnitude && compact.exponent_for_magnitude(total) != compact_exponent {
                compact_magnitude = total;
                bump = 1;
                continue;
            }
        }
        break;
    }

    let (integer, fraction) = integer_and_fraction(&rounded, settings);
    let is_zero = rounded.digits.is_zero() && special.is_none();
    if special == Some("nan") {
        negative = false;
    }

    // O sinal.
    let sign_part: Option<&str> = match settings.sign_display {
        SignDisplay::Auto => negative.then_some("minusSign"),
        SignDisplay::Never => None,
        SignDisplay::Always => Some(if negative { "minusSign" } else { "plusSign" }),
        SignDisplay::ExceptZero => {
            if is_zero || special == Some("nan") { None } else { Some(if negative { "minusSign" } else { "plusSign" }) }
        }
        SignDisplay::Negative => (negative && !is_zero).then_some("minusSign"),
    };
    let sign_part = if special == Some("nan") && settings.sign_display != SignDisplay::Always { None } else { sign_part };
    let accounting = settings.style == Style::Currency
        && settings.currency_sign == CurrencySign::Accounting
        && sign_part == Some("minusSign")
        && accounting_uses_parentheses(settings);

    // O número (com o expoente e o sufixo compacto).
    let mut number: Vec<Part> = match special {
        Some("nan") => vec![("nan".to_string(), nan_symbol(&settings.locale).to_string())],
        Some(_) => vec![("infinity".to_string(), "\u{221e}".to_string())],
        None => compact
            .as_ref()
            .and_then(|compact| compact.format(&integer, &fraction, compact_exponent))
            .unwrap_or_else(|| number_parts(decimal_format.as_ref(), &integer, &fraction)),
    };
    if special.is_none() && matches!(settings.notation, Notation::Scientific | Notation::Engineering) {
        number.push(("exponentSeparator".to_string(), "E".to_string()));
        // Medido no bun: o expoente usa os dígitos e o sinal de menos do locale (`ne`: `E५`, `eu`: `E−4`).
        let local_exponent = decimal_format.as_ref().and_then(|format| format.number(&exponent.unsigned_abs().to_string(), ""));
        let local_minus = decimal_format.as_ref().and_then(|format| format.sign(true).0.into_iter().find(|(kind, _)| kind == "minusSign"));
        if exponent < 0 {
            number.push(("exponentMinusSign".to_string(), local_minus.map_or_else(|| "-".to_string(), |(_, text)| text)));
        }
        let digits = local_exponent.map(|parts| parts.into_iter().map(|(_, text)| text).collect::<String>());
        number.push(("exponentInteger".to_string(), digits.unwrap_or_else(|| exponent.unsigned_abs().to_string())));
    }
    // Os operandos de plural do número formatado (as casas visíveis contam). No compacto o ICU escolhe a forma
    // pelo número na ordem original, não pelo significando: `1 trillion euros` e `1 M euros`, no plural
    // (medido no bun, `number_compact_bun.tsv`).
    let (plural_integer, plural_fraction) = unscale_digits(&integer, &fraction, compact_exponent);
    let category = icu_plural::select(&settings.locale, false, &plural_integer, &plural_fraction, 0);
    let plural = match category {
        Some(category) => category != "one",
        None => cardinal_category(settings.language, &PluralOperands::from_decimal(&plural_integer, &plural_fraction)) != "one",
    };

    // Moeda e unidade das línguas medidas no bun (`icu_number_data`): o padrão inteiro vem da tabela.
    if let Some(parts) = localized_parts(settings, decimal_format.as_ref(), sign_part, accounting, category.unwrap_or("other"), &number) {
        return (with_currency_spacing(settings, parts), exponent + i32::from(compact_exponent));
    }

    // O sinal do locale (o `ar` põe a marca bidirecional junto do menos); o contábil troca o menos pelo parêntese.
    let (sign_prefix, sign_suffix) = match sign_part {
        Some(sign) if !accounting => sign_parts(decimal_format.as_ref(), sign == "minusSign"),
        _ => (Vec::new(), Vec::new()),
    };
    let mut parts: Vec<Part> = Vec::new();
    if accounting {
        parts.push(("literal".to_string(), "(".to_string()));
    } else {
        parts.extend(sign_prefix);
    }
    match settings.style {
        Style::Decimal => parts.extend(number),
        Style::Percent => {
            let mut percent =
                percent_parts(&settings.locale, &settings.numbering_system, settings.notation == Notation::Compact, number);
            rename_compact_percent_sign(settings, &mut percent);
            parts.extend(percent);
        }
        Style::Currency => {
            let code = settings.currency.as_str();
            let symbol_prefix = |text: &str, parts: &mut Vec<Part>, number: Vec<Part>, spaced: bool| {
                parts.push(("currency".to_string(), text.to_string()));
                if spaced {
                    parts.push(("literal".to_string(), NO_BREAK_SPACE.to_string()));
                }
                parts.extend(number);
            };
            let spaced_language = settings.language == Language::Portuguese;
            match settings.currency_display {
                CurrencyDisplay::Symbol | CurrencyDisplay::NarrowSymbol => {
                    let symbol = if settings.currency_display == CurrencyDisplay::NarrowSymbol {
                        currency_narrow_symbol(code, settings.language)
                    } else {
                        currency_symbol(code, settings.language)
                    };
                    match symbol {
                        Some(symbol) => symbol_prefix(symbol, &mut parts, number, spaced_language),
                        None => symbol_prefix(code, &mut parts, number, true),
                    }
                }
                CurrencyDisplay::Code => symbol_prefix(code, &mut parts, number, true),
                CurrencyDisplay::Name => {
                    parts.extend(number);
                    parts.push(("literal".to_string(), " ".to_string()));
                    parts.push(("currency".to_string(), currency_name(code, plural).unwrap_or(code).to_string()));
                }
            }
        }
        Style::Unit => {
            let (text, attached) = unit_text(&settings.unit, settings.unit_display, plural);
            parts.extend(number);
            if !attached {
                parts.push(("literal".to_string(), " ".to_string()));
            }
            parts.push(("unit".to_string(), text));
        }
    }
    if accounting {
        parts.push(("literal".to_string(), ")".to_string()));
    } else {
        parts.extend(sign_suffix);
    }
    (with_currency_spacing(settings, parts), exponent + i32::from(compact_exponent))
}

/// Com `∞` e `NaN` o ICU não insere o espaço entre a moeda (símbolo ou código) e o número, porque o
/// `currencySpacing` só age ao lado de dígito: `EUR∞` em `en`, `EUR ∞` em `pt` (onde o espaço é do padrão).
/// O nome da moeda não passa por isso.
fn with_currency_spacing(settings: &NumberSettings, mut parts: Vec<Part>) -> Vec<Part> {
    if settings.style == Style::Currency && settings.currency_display != CurrencyDisplay::Name {
        let accounting = settings.currency_sign == CurrencySign::Accounting;
        patterns::drop_inserted_currency_spacing(&settings.locale, accounting, &mut parts);
    }
    parts
}

/// Moeda e unidade pelos padrões medidos no bun (`icu_number_data`), para as línguas que a tabela cobre.
/// `None` quando o locale, a moeda ou a unidade não tem entrada: quem chama segue nas tabelas à mão.
/// O sinal vem do padrão (`-` no negativo, parênteses no contábil); o positivo com sinal explícito
/// usa o padrão do negativo, onde o sinal fica no mesmo lugar.
fn localized_parts(
    settings: &NumberSettings,
    decimal_format: Option<&DecimalFormat>,
    sign_part: Option<&str>,
    accounting: bool,
    category: &str,
    number: &[Part],
) -> Option<Vec<Part>> {
    let (sign_prefix, sign_suffix) = match sign_part {
        Some(sign) => sign_parts(decimal_format, sign == "minusSign"),
        None => (Vec::new(), Vec::new()),
    };
    let fill = Fill { number, sign_prefix: &sign_prefix, sign_suffix: &sign_suffix, currency: None, percent: None };
    let signed = sign_part.is_some();
    match settings.style {
        Style::Currency => {
            let code = settings.currency.as_str();
            let extra = patterns::extra_currency(&settings.locale, code);
            if settings.currency_display == CurrencyDisplay::Name {
                if let Some(entry) = patterns::currency_name(&settings.locale, code, category) {
                    let template = if signed { entry.negative } else { entry.positive };
                    return Some(patterns::render(template, &fill));
                }
                // Moeda fora do conjunto principal: o padrão do dólar com o nome da moeda no lugar.
                let entry = patterns::currency_name(&settings.locale, "USD", category)?;
                let template = if signed { entry.negative } else { entry.positive };
                let name = extra.and_then(|extra| extra.name_for(category)).unwrap_or(code);
                return Some(patterns::render(template, &Fill { currency: Some(name), ..fill }));
            }
            let display = match settings.currency_display {
                CurrencyDisplay::Symbol => 0,
                CurrencyDisplay::NarrowSymbol => 1,
                _ => 2,
            };
            // Símbolo próprio de uma moeda fora do conjunto principal, com o padrão medido para ele.
            if let (Some(extra), 0 | 1) = (extra, display) {
                let (text, base) = if display == 1 && !extra.narrow().is_empty() {
                    (extra.narrow(), extra.narrow_base)
                } else {
                    (extra.symbol(), extra.symbol_base)
                };
                if !text.is_empty() {
                    if let Some(base) = patterns::currency_base(&settings.locale, base) {
                        let template = if accounting {
                            base.accounting
                        } else if signed {
                            base.negative
                        } else {
                            base.positive
                        };
                        return Some(patterns::render(template, &Fill { currency: Some(text), ..fill }));
                    }
                }
            }
            let (entry, swap_code) = match patterns::currency(&settings.locale, code, display) {
                Some(entry) => (entry, false),
                None => (patterns::currency_code_fallback(&settings.locale)?, true),
            };
            let positive_accounting = (!signed && settings.currency_sign == CurrencySign::Accounting)
                .then(|| patterns::accounting_positive(&settings.locale, code, display))
                .flatten();
            let template = if accounting {
                entry.accounting
            } else if let Some(template) = positive_accounting {
                template
            } else if signed {
                entry.negative
            } else {
                entry.positive
            };
            let mut parts = patterns::render(template, &fill);
            if swap_code {
                for (kind, text) in parts.iter_mut() {
                    if kind == "currency" {
                        *text = code.to_string();
                    }
                }
            }
            Some(parts)
        }
        Style::Unit => {
            let display = match settings.unit_display {
                UnitDisplay::Long => 0,
                UnitDisplay::Short => 1,
                UnitDisplay::Narrow => 2,
            };
            let entry = patterns::unit(&settings.locale, &settings.unit, display, category)?;
            let mut parts = patterns::render(entry.template, &fill);
            parts.splice(0..0, sign_prefix.iter().cloned());
            parts.extend(sign_suffix.iter().cloned());
            Some(parts)
        }
        _ => None,
    }
}

/// Se o contábil usa parênteses: consulta a tabela medida no bun (locale x currencyDisplay x notação).
fn accounting_uses_parentheses(settings: &NumberSettings) -> bool {
    let display = match settings.currency_display {
        CurrencyDisplay::Symbol => 0,
        CurrencyDisplay::NarrowSymbol => 1,
        CurrencyDisplay::Name => 3,
        _ => 2,
    };
    patterns::accounting_parentheses(&settings.locale, display, settings.notation == Notation::Compact)
}

/// No compacto o ICU chama o sinal de percentual de `unit` em vez de `percentSign` (medido no bun, todos os locales).
fn rename_compact_percent_sign(settings: &NumberSettings, parts: &mut [Part]) {
    if settings.notation == Notation::Compact {
        for (kind, _) in parts.iter_mut().filter(|(kind, _)| kind == "percentSign") {
            *kind = "unit".to_string();
        }
    }
}

/// O texto do número formatado: as partes juntas.
pub fn format_to_string(settings: &NumberSettings, input: &NumericInput) -> String {
    format_parts(settings, input).into_iter().map(|(_, text)| text).collect()
}

impl IntlEnum for UseGrouping {
    const NAMES: &'static [&'static str] = &["min2", "auto", "always"];

    fn parse(text: &str) -> Option<UseGrouping> {
        match text {
            "min2" => Some(UseGrouping::Min2),
            "auto" => Some(UseGrouping::Auto),
            "always" => Some(UseGrouping::Always),
            _ => None,
        }
    }

    fn as_str(self) -> &'static str {
        match self {
            UseGrouping::False => "false",
            UseGrouping::Min2 => "min2",
            UseGrouping::Auto => "auto",
            UseGrouping::Always => "always",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn format(x: f64) -> String {
        format_to_string(&NumberSettings::defaults(Language::English), &NumericInput::Double(x))
    }

    #[test]
    fn groups_and_limits_fraction_digits() {
        assert_eq!(format(0.0), "0");
        assert_eq!(format(999.0), "999");
        assert_eq!(format(1000.0), "1,000");
        assert_eq!(format(1234567.891), "1,234,567.891");
        assert_eq!(format(1234.5678), "1,234.568");
        assert_eq!(format(-1234.5678), "-1,234.568");
        assert_eq!(format(0.1 + 0.2), "0.3");
        assert_eq!(format(12345.0), "12,345");
    }

    #[test]
    fn rounds_the_shortest_digits_half_expand() {
        assert_eq!(format(1.0005), "1.001");
        assert_eq!(format(0.0005), "0.001");
        assert_eq!(format(0.0004), "0");
        assert_eq!(format(999.9996), "1,000");
        assert_eq!(format(0.9995), "1");
        assert_eq!(format(1e-10), "0");
    }

    #[test]
    fn keeps_the_sign_of_zero() {
        assert_eq!(format(-0.0), "-0");
        assert_eq!(format(-0.0001), "-0");
    }

    #[test]
    fn special_and_huge_values() {
        assert_eq!(format(f64::NAN), "NaN");
        assert_eq!(format(f64::INFINITY), "\u{221e}");
        assert_eq!(format(f64::NEG_INFINITY), "-\u{221e}");
        assert_eq!(format(1e21), "1,000,000,000,000,000,000,000");
    }

    #[test]
    fn small_fractions_keep_their_leading_zeros() {
        assert_eq!(format(0.001), "0.001");
        assert_eq!(format(0.0123), "0.012");
        assert_eq!(format(0.05), "0.05");
    }

    #[test]
    fn brazilian_separators() {
        let settings = NumberSettings::defaults(Language::Portuguese);
        assert_eq!(format_to_string(&settings, &NumericInput::Double(1234567.891)), "1.234.567,891");
    }

    #[test]
    fn currency() {
        let mut settings = NumberSettings::defaults(Language::English);
        settings.style = Style::Currency;
        settings.currency = "USD".to_string();
        settings.rounding = Rounding::FractionDigits { min: 2, max: 2 };
        assert_eq!(format_to_string(&settings, &NumericInput::Double(1234.5)), "$1,234.50");
        assert_eq!(format_to_string(&settings, &NumericInput::Double(-0.5)), "-$0.50");
        settings.currency = "BRL".to_string();
        settings.language = Language::Portuguese;
        settings.locale = "pt".to_string();
        assert_eq!(format_to_string(&settings, &NumericInput::Double(1234.5)), "R$\u{a0}1.234,50");
    }

    #[test]
    fn percent_and_compact() {
        let mut settings = NumberSettings::defaults(Language::English);
        settings.style = Style::Percent;
        settings.rounding = Rounding::FractionDigits { min: 0, max: 0 };
        assert_eq!(format_to_string(&settings, &NumericInput::Double(0.256)), "26%");

        let mut compact = NumberSettings::defaults(Language::English);
        compact.notation = Notation::Compact;
        compact.use_grouping = UseGrouping::Min2;
        compact.rounding =
            Rounding::MorePrecision { min_fraction: 0, max_fraction: 0, min_significant: 1, max_significant: 2 };
        assert_eq!(format_to_string(&compact, &NumericInput::Double(1234.0)), "1.2K");
        assert_eq!(format_to_string(&compact, &NumericInput::Double(123456.0)), "123K");
        assert_eq!(format_to_string(&compact, &NumericInput::Double(999999.0)), "1M");
    }

    #[test]
    fn scientific() {
        let mut settings = NumberSettings::defaults(Language::English);
        settings.notation = Notation::Scientific;
        assert_eq!(format_to_string(&settings, &NumericInput::Double(12345.0)), "1.235E4");
        assert_eq!(format_to_string(&settings, &NumericInput::Double(0.00012)), "1.2E-4");
    }

    #[test]
    fn rounding_modes() {
        let mut settings = NumberSettings::defaults(Language::English);
        settings.rounding = Rounding::FractionDigits { min: 0, max: 0 };
        settings.rounding_mode = RoundingMode::HalfEven;
        assert_eq!(format_to_string(&settings, &NumericInput::Double(2.5)), "2");
        settings.rounding_mode = RoundingMode::Ceil;
        assert_eq!(format_to_string(&settings, &NumericInput::Double(2.1)), "3");
        settings.rounding_mode = RoundingMode::Floor;
        assert_eq!(format_to_string(&settings, &NumericInput::Double(-2.1)), "-3");
    }

    #[test]
    fn big_integers() {
        let input = NumericInput::Decimal { negative: true, digits: "123456789012345678901234567890".to_string() };
        assert_eq!(
            format_to_string(&NumberSettings::defaults(Language::English), &input),
            "-123,456,789,012,345,678,901,234,567,890"
        );
    }

    fn locale_settings(locale: &str) -> NumberSettings {
        let mut settings = NumberSettings::defaults(Language::English);
        settings.locale = locale.to_string();
        settings
    }

    fn locale_format(settings: &NumberSettings, x: f64) -> String {
        format_to_string(settings, &NumericInput::Double(x))
    }

    #[test]
    fn decimal_symbols_follow_the_locale() {
        assert_eq!(locale_format(&locale_settings("fr"), 1234567.891), "1\u{202f}234\u{202f}567,891");
        assert_eq!(locale_format(&locale_settings("de"), 1234567.891), "1.234.567,891");
        assert_eq!(locale_format(&locale_settings("ja"), 1234567.891), "1,234,567.891");
        assert_eq!(locale_format(&locale_settings("en-GB"), 1234567.891), "1,234,567.891");
        assert_eq!(locale_format(&locale_settings("pt-PT"), 1234567.891), "1\u{a0}234\u{a0}567,891");
        assert_eq!(locale_format(&locale_settings("de"), -0.0005), "-0,001");
    }

    #[test]
    fn indian_grouping_and_minimum_grouping_digits() {
        assert_eq!(locale_format(&locale_settings("en-IN"), 12345678.0), "1,23,45,678");
        assert_eq!(locale_format(&locale_settings("hi"), 1234567.891), "12,34,567.891");
        assert_eq!(locale_format(&locale_settings("hi"), 1e21), "1,00,00,00,00,00,00,00,00,00,000");
        assert_eq!(locale_format(&locale_settings("es"), 1234.0), "1234");
        assert_eq!(locale_format(&locale_settings("es"), 12345.0), "12.345");
        assert_eq!(locale_format(&locale_settings("pt-PT"), 1234.0), "1234");
    }

    #[test]
    fn use_grouping_modes() {
        let mut settings = locale_settings("de");
        settings.use_grouping = UseGrouping::False;
        assert_eq!(locale_format(&settings, 1234567.5), "1234567,5");
        settings.use_grouping = UseGrouping::Min2;
        assert_eq!(locale_format(&settings, 1234.0), "1234");
        assert_eq!(locale_format(&settings, 12345.0), "12.345");
    }

    #[test]
    fn numbering_systems() {
        let mut settings = locale_settings("de");
        settings.numbering_system = "arab".to_string();
        assert_eq!(locale_format(&settings, 1234567.891), "\u{661}\u{66c}\u{662}\u{663}\u{664}\u{66c}\u{665}\u{666}\u{667}\u{66b}\u{668}\u{669}\u{661}");
        settings.numbering_system = "deva".to_string();
        assert_eq!(locale_format(&settings, 1234567.891), "\u{967}.\u{968}\u{969}\u{96a}.\u{96b}\u{96c}\u{96d},\u{96e}\u{96f}\u{967}");
        let mut arabic = locale_settings("ar");
        arabic.numbering_system = "latn".to_string();
        assert_eq!(locale_format(&arabic, 1234567.891), "1,234,567.891");
    }

    #[test]
    fn percent_patterns_per_locale() {
        let percent = |locale: &str, x: f64| {
            let mut settings = locale_settings(locale);
            settings.style = Style::Percent;
            settings.rounding = Rounding::FractionDigits { min: 0, max: 0 };
            locale_format(&settings, x)
        };
        assert_eq!(percent("en", 0.256), "26%");
        assert_eq!(percent("fr", 1234.5), "123\u{202f}450\u{a0}%");
        assert_eq!(percent("de", 1234.5), "123.450\u{a0}%");
        assert_eq!(percent("de", -0.0005), "-0\u{a0}%");
        assert_eq!(percent("ja", 1234.5), "123,450%");
        assert_eq!(percent("hi", 1234.5), "1,23,450%");
        assert_eq!(percent("ar", 1.0), "100\u{200e}%\u{200e}");
        assert_eq!(percent("ar", -0.0005), "\u{200e}-0\u{200e}%\u{200e}");
        assert_eq!(percent("ar-EG", 0.25), "\u{662}\u{665}\u{66a}\u{61c}");
        assert_eq!(percent("fa", 0.25), "\u{6f2}\u{6f5}\u{66a}");
    }

    #[test]
    fn nan_and_always_per_locale() {
        let nan = |locale: &str| locale_format(&locale_settings(locale), f64::NAN);
        assert_eq!(nan("en"), "NaN");
        assert_eq!(nan("he"), "NaN");
        assert_eq!(nan("ar"), "\u{644}\u{64a}\u{633}\u{a0}\u{631}\u{642}\u{645}\u{64b}\u{627}");
        assert_eq!(nan("fi"), "ep\u{e4}luku");
        let mut settings = locale_settings("es");
        settings.use_grouping = UseGrouping::Always;
        assert_eq!(locale_format(&settings, 1234.5), "1.234,5");
        settings.use_grouping = UseGrouping::Auto;
        assert_eq!(locale_format(&settings, 1234.5), "1234,5");
    }

    #[test]
    fn sign_display_uses_the_locale_marks() {
        let mut settings = locale_settings("ar");
        settings.sign_display = SignDisplay::Always;
        assert_eq!(locale_format(&settings, 5.0), "\u{200e}+5");
        settings.sign_display = SignDisplay::Auto;
        assert_eq!(locale_format(&settings, -5.0), "\u{200e}-5");
        let mut german = locale_settings("de");
        german.sign_display = SignDisplay::ExceptZero;
        assert_eq!(locale_format(&german, 5.0), "+5");
        assert_eq!(locale_format(&german, 0.0), "0");
    }

    #[test]
    fn compact_from_the_locale_data() {
        let compact = |locale: &str, display: CompactDisplay, x: f64| {
            let mut settings = locale_settings(locale);
            settings.notation = Notation::Compact;
            settings.compact_display = display;
            settings.use_grouping = UseGrouping::Min2;
            settings.rounding =
                Rounding::MorePrecision { min_fraction: 0, max_fraction: 0, min_significant: 1, max_significant: 2 };
            locale_format(&settings, x)
        };
        assert_eq!(compact("en", CompactDisplay::Short, 1234567.0), "1.2M");
        assert_eq!(compact("en", CompactDisplay::Long, 1234567.0), "1.2 million");
        assert_eq!(compact("pt", CompactDisplay::Short, 1234.0), "1,2\u{a0}mil");
        assert_eq!(compact("fr", CompactDisplay::Short, 1000.0), "1\u{a0}k");
        assert_eq!(compact("fr", CompactDisplay::Long, 1000.0), "mille");
        assert_eq!(compact("de", CompactDisplay::Short, 1000.0), "1000");
        assert_eq!(compact("de", CompactDisplay::Short, 12345.0), "12.345");
        assert_eq!(compact("de", CompactDisplay::Long, 1000.0), "1 Tausend");
        assert_eq!(compact("de", CompactDisplay::Short, 1234567.0), "1,2\u{a0}Mio.");
        assert_eq!(compact("ja", CompactDisplay::Short, 123456789.0), "1.2\u{5104}");
        assert_eq!(compact("hi", CompactDisplay::Long, 1234567.0), "12 \u{932}\u{93e}\u{916}");
        assert_eq!(compact("en", CompactDisplay::Short, 999999.0), "1M");
        assert_eq!(compact("en", CompactDisplay::Short, 999.0), "999");
    }

    #[test]
    fn compact_parts_name_the_suffix() {
        let mut settings = locale_settings("en");
        settings.notation = Notation::Compact;
        settings.compact_display = CompactDisplay::Long;
        settings.rounding =
            Rounding::MorePrecision { min_fraction: 0, max_fraction: 0, min_significant: 1, max_significant: 2 };
        let parts = format_parts(&settings, &NumericInput::Double(1234567.0));
        let kinds: Vec<&str> = parts.iter().map(|(kind, _)| kind.as_str()).collect();
        assert_eq!(kinds, ["integer", "decimal", "fraction", "literal", "compact"]);
    }

    #[test]
    fn compact_percent_uses_the_unit_pattern() {
        // Medido no bun: no compacto o espaço antes do `%` é o comum em fr e de (no padrão é U+00A0), e o
        // sinal é do tipo `unit`.
        let compact_percent = |locale: &str| {
            let mut settings = locale_settings(locale);
            settings.notation = Notation::Compact;
            settings.style = Style::Percent;
            settings.use_grouping = UseGrouping::Min2;
            settings.rounding =
                Rounding::MorePrecision { min_fraction: 0, max_fraction: 0, min_significant: 1, max_significant: 2 };
            format_parts(&settings, &NumericInput::Double(12345.0))
        };
        let text = |parts: &[Part]| parts.iter().map(|(_, text)| text.as_str()).collect::<String>();
        assert_eq!(text(&compact_percent("fr")), "1,2\u{a0}M %");
        assert_eq!(text(&compact_percent("de")), "1,2\u{a0}Mio. %");
        assert_eq!(text(&compact_percent("es")), "1,2\u{a0}M\u{a0}%");
        assert_eq!(compact_percent("fr").last().map(|(kind, _)| kind.as_str()), Some("unit"));
    }

    fn currency_text(locale: &str, display: CurrencyDisplay, sign: CurrencySign, x: f64) -> String {
        let mut settings = locale_settings(locale);
        settings.style = Style::Currency;
        settings.currency = "EUR".to_string();
        settings.currency_display = display;
        settings.currency_sign = sign;
        let digits = currency_digits("EUR");
        settings.rounding = Rounding::FractionDigits { min: digits, max: digits };
        locale_format(&settings, x)
    }

    /// Medido no bun 1.4.2: com `∞` e `NaN` some o espaço que o ICU insere entre a moeda e o número; o
    /// espaço que é do padrão do locale (pt, de, fr) fica.
    #[test]
    fn infinity_drops_the_inserted_currency_spacing() {
        let standard = CurrencySign::Standard;
        let accounting = CurrencySign::Accounting;
        let code = CurrencyDisplay::Code;
        assert_eq!(currency_text("en", code, standard, f64::INFINITY), "EUR\u{221e}");
        assert_eq!(currency_text("en", code, standard, f64::NEG_INFINITY), "-EUR\u{221e}");
        assert_eq!(currency_text("en", code, standard, f64::NAN), "EURNaN");
        assert_eq!(currency_text("en", code, accounting, f64::NEG_INFINITY), "(EUR\u{221e})");
        assert_eq!(currency_text("en", code, standard, 5.0), "EUR\u{a0}5.00");
        assert_eq!(currency_text("ja", code, standard, f64::INFINITY), "EUR\u{221e}");
        assert_eq!(currency_text("pt", code, standard, f64::INFINITY), "EUR\u{a0}\u{221e}");
        assert_eq!(currency_text("de", code, standard, f64::INFINITY), "\u{221e}\u{a0}EUR");
        assert_eq!(currency_text("fr", code, accounting, f64::NEG_INFINITY), "(\u{221e}\u{a0}EUR)");
        // O contábil do ar insere o espaço (some), o padrão comum dele tem o espaço no padrão (fica).
        assert_eq!(currency_text("ar", code, accounting, f64::INFINITY), "\u{61c}\u{221e}EUR");
        assert_eq!(currency_text("ar", code, standard, f64::INFINITY), "\u{200f}\u{221e}\u{a0}EUR");
        assert_eq!(currency_text("en", CurrencyDisplay::Symbol, standard, f64::INFINITY), "\u{20ac}\u{221e}");
    }

    /// Medido no bun 1.4.2: o nome da moeda nunca usa parênteses, nem no `currencySign: "accounting"`.
    #[test]
    fn currency_name_ignores_accounting_parentheses() {
        let name = CurrencyDisplay::Name;
        let accounting = CurrencySign::Accounting;
        assert_eq!(currency_text("en", name, accounting, -5.0), "-5.00 euros");
        assert_eq!(currency_text("de", name, accounting, -5.0), "-5,00 Euro");
        assert_eq!(currency_text("pt", name, accounting, -5.0), "-5,00 Euros");
        assert_eq!(currency_text("ja", name, accounting, -5.0), "-5.00\u{30e6}\u{30fc}\u{30ed}");
    }

    /// Medido no bun 1.4.2: as unidades por (`per`) do inglês usam o sufixo do CLDR (`/h`), não o `hr` da unidade
    /// simples `hour`; o composto `hour-per-day` começa pelo `hr`.
    #[test]
    fn per_units_use_the_cldr_suffix() {
        let unit = |id: &str, display: UnitDisplay| {
            let mut settings = locale_settings("en");
            settings.style = Style::Unit;
            settings.unit = id.to_string();
            settings.unit_display = display;
            locale_format(&settings, 5.0)
        };
        assert_eq!(unit("kilometer-per-hour", UnitDisplay::Short), "5 km/h");
        assert_eq!(unit("kilometer-per-hour", UnitDisplay::Narrow), "5km/h");
        assert_eq!(unit("kilometer-per-hour", UnitDisplay::Long), "5 kilometers per hour");
        assert_eq!(unit("meter-per-second", UnitDisplay::Short), "5 m/s");
        assert_eq!(unit("mile-per-hour", UnitDisplay::Short), "5 mph");
        assert_eq!(unit("hour-per-day", UnitDisplay::Short), "5 hr/d");
        assert_eq!(unit("hour", UnitDisplay::Short), "5 hr");
    }
}
