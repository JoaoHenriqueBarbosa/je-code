//! `Intl.NumberFormat` sem ICU (`IntlNumberFormat.cpp`, `IntlNumberFormatInlines.h`,
//! `IntlNumberFormatPrototype.cpp`, `IntlNumberFormatConstructor.cpp`): a leitura das opções, a
//! instância, `format`, `formatToParts` e `resolvedOptions`, sobre o formatador de
//! `default_number_format.rs`.
//!
//! LOCALES: os símbolos, o agrupamento e a notação compacta vêm do CLDR pelo `icu_decimal`
//! (`icu_number.rs`); moeda, unidade e nomes seguem as tabelas de `default_number_format.rs`.
//!
//! DIVERGÊNCIAS e LACUNAS, e por quê:
//!
//! - `formatRange` e `formatRangeToParts` vivem em `intl_number_range.rs` (o colapso de afixos do
//!   `UNumberRangeFormatter` com as regras do ICU, para `en` e `pt`).
//! - `numberingSystem` e `-u-nu-` valem para os sistemas de dígitos decimais que o `icu_decimal` tem
//!   (`arab`, `deva`, `thai`, `beng`...); um sistema algorítmico (`roman`, `hans`) ou desconhecido cai no
//!   padrão do locale.
//! - Ver `default_number_format.rs` para `roundingIncrement`, moedas e unidades.

use crate::custom_getter;
use crate::host_function;
use crate::runtime::class_info::ClassInfo;
use crate::runtime::js_object::JS_NON_FINAL_OBJECT_S_INFO;
use crate::runtime::lookup::{HashTable, HashTableValue};
use crate::runtime::property_name::PropertyName;
use crate::runtime::default_number_format::{
    currency_digits, format_parts, format_to_string, is_known_unit, CompactDisplay, CurrencyDisplay, CurrencySign, Notation,
    NumberSettings, NumericInput, Rounding, RoundingMode, RoundingPriority, SignDisplay, Style, TrailingZeroDisplay,
    UnitDisplay, UseGrouping,
};
use crate::runtime::host_call::{pending_or, HostCall, HostResult, Thrown};
use crate::runtime::icu_number;
use crate::runtime::intl_locale_data::{Language, ResolvedLocale};
use crate::runtime::intl_number_range::format_range_parts;
use crate::runtime::intl_support::{
    bound_function, call_instance, coerce_options_to_object, construct_instance, default_number_option, get_property, new_object,
    number_option, number_value, option_enum, option_string, parts_array, put, intl_format_prototype_values,
    range_parts_array, read_locale_matcher, resolve_locale_from, str_value, to_number_checked, to_rust_string,
    with_instance, wtf_to_rust, IntlClass, IntlEnum, RangePart,
};
use crate::runtime::object_to_primitive::PreferredPrimitiveType;
use crate::runtime::js_big_int_ops::big_int_of;
use crate::runtime::js_global_object::JSGlobalObject;
use crate::runtime::js_object::JSObject;
use crate::runtime::js_value::{js_boolean, JSValue};

/// O estado de um `IntlNumberFormat`.
pub struct NumberFormatState {
    pub locale: String,
    /// O `numberingSystem` resolvido (`latn`, `arab`...).
    pub numbering_system: String,
    pub settings: NumberSettings,
}

/// O resultado de `setNumberFormatDigitOptions`.
pub struct DigitOptions {
    pub minimum_integer_digits: u32,
    pub rounding: Rounding,
    pub rounding_mode: RoundingMode,
    pub rounding_increment: u32,
    pub trailing_zero_display: TrailingZeroDisplay,
}

impl DigitOptions {
    /// Grava os campos no formatador.
    pub fn apply(&self, settings: &mut NumberSettings) {
        settings.minimum_integer_digits = self.minimum_integer_digits;
        settings.rounding = self.rounding;
        settings.rounding_mode = self.rounding_mode;
        settings.rounding_increment = self.rounding_increment;
        settings.trailing_zero_display = self.trailing_zero_display;
    }
}

/// `setNumberFormatDigitOptions(globalObject, intlInstance, options, minimumFractionDigitsDefault,
/// maximumFractionDigitsDefault, notation)`.
pub fn read_digit_options(
    global_object: &JSGlobalObject,
    options: Option<JSValue>,
    minimum_fraction_digits_default: u32,
    mut maximum_fraction_digits_default: u32,
    notation: Notation,
) -> Result<DigitOptions, Thrown> {
    let minimum_integer_digits = number_option(global_object, options, "minimumIntegerDigits", 1, 21)?.unwrap_or(1);

    let read = |name: &str| -> Result<JSValue, Thrown> {
        match options {
            Some(options) => get_property(global_object, options, name),
            None => Ok(JSValue::undefined()),
        }
    };
    let minimum_fraction_value = read("minimumFractionDigits")?;
    let maximum_fraction_value = read("maximumFractionDigits")?;
    let minimum_significant_value = read("minimumSignificantDigits")?;
    let maximum_significant_value = read("maximumSignificantDigits")?;

    let rounding_increment = number_option(global_object, options, "roundingIncrement", 1, 5000)?.unwrap_or(1);
    const CANDIDATES: [u32; 15] = [1, 2, 5, 10, 20, 25, 50, 100, 200, 250, 500, 1000, 2000, 2500, 5000];
    if !CANDIDATES.contains(&rounding_increment) {
        return Err(Thrown::range_error(
            "roundingIncrement must be one of 1, 2, 5, 10, 20, 25, 50, 100, 200, 250, 500, 1000, 2000, 2500, 5000",
        ));
    }

    let rounding_mode = option_enum::<RoundingMode>(
        global_object,
        options,
        "roundingMode",
        "roundingMode must be either \"ceil\", \"floor\", \"expand\", \"trunc\", \"halfCeil\", \"halfFloor\", \"halfExpand\", \"halfTrunc\", or \"halfEven\"",
    )?
    .unwrap_or(RoundingMode::HalfExpand);
    let rounding_priority = option_enum::<RoundingPriority>(
        global_object,
        options,
        "roundingPriority",
        "roundingPriority must be either \"auto\", \"morePrecision\", or \"lessPrecision\"",
    )?
    .unwrap_or(RoundingPriority::Auto);
    let trailing_zero_display = option_enum::<TrailingZeroDisplay>(
        global_object,
        options,
        "trailingZeroDisplay",
        "trailingZeroDisplay must be either \"auto\" or \"stripIfInteger\"",
    )?
    .unwrap_or(TrailingZeroDisplay::Auto);

    if rounding_increment != 1 {
        maximum_fraction_digits_default = minimum_fraction_digits_default;
    }

    let has_significant = !minimum_significant_value.is_undefined() || !maximum_significant_value.is_undefined();
    let has_fraction = !minimum_fraction_value.is_undefined() || !maximum_fraction_value.is_undefined();

    let mut need_significant = true;
    let mut need_fraction = true;
    if rounding_priority == RoundingPriority::Auto {
        need_significant = has_significant;
        if has_significant || (!has_fraction && notation == Notation::Compact) {
            need_fraction = false;
        }
    }

    let mut significant = (1, 21);
    if need_significant && has_significant {
        let minimum =
            default_number_option(global_object, minimum_significant_value, "minimumSignificantDigits", 1, 21)?.unwrap_or(1);
        let maximum = default_number_option(global_object, maximum_significant_value, "maximumSignificantDigits", minimum, 21)?
            .unwrap_or(21);
        significant = (minimum, maximum);
    }

    let mut fraction = (minimum_fraction_digits_default, maximum_fraction_digits_default);
    if need_fraction && has_fraction {
        let minimum = default_number_option(global_object, minimum_fraction_value, "minimumFractionDigits", 0, 100)?;
        let maximum = default_number_option(global_object, maximum_fraction_value, "maximumFractionDigits", 0, 100)?;
        fraction = match (minimum, maximum) {
            (None, Some(maximum)) => (minimum_fraction_digits_default.min(maximum), maximum),
            (Some(minimum), None) => (minimum, maximum_fraction_digits_default.max(minimum)),
            (Some(minimum), Some(maximum)) => {
                if minimum > maximum {
                    return Err(Thrown::range_error("Computed minimumFractionDigits is larger than maximumFractionDigits"));
                }
                (minimum, maximum)
            }
            (None, None) => fraction,
        };
    }

    let rounding = if need_significant || need_fraction {
        match rounding_priority {
            RoundingPriority::MorePrecision => Rounding::MorePrecision {
                min_fraction: fraction.0,
                max_fraction: fraction.1,
                min_significant: significant.0,
                max_significant: significant.1,
            },
            RoundingPriority::LessPrecision => Rounding::LessPrecision {
                min_fraction: fraction.0,
                max_fraction: fraction.1,
                min_significant: significant.0,
                max_significant: significant.1,
            },
            RoundingPriority::Auto if has_significant => Rounding::SignificantDigits { min: significant.0, max: significant.1 },
            RoundingPriority::Auto => Rounding::FractionDigits { min: fraction.0, max: fraction.1 },
        }
    } else {
        Rounding::MorePrecision { min_fraction: 0, max_fraction: 0, min_significant: 1, max_significant: 2 }
    };

    if rounding_increment != 1 {
        let Rounding::FractionDigits { min, max } = rounding else {
            return Err(Thrown::type_error("rounding type is not fraction-digits while roundingIncrement is specified"));
        };
        if min != max {
            return Err(Thrown::range_error(
                "maximumFractionDigits and minimumFractionDigits are different while roundingIncrement is specified",
            ));
        }
    }

    Ok(DigitOptions { minimum_integer_digits, rounding, rounding_mode, rounding_increment, trailing_zero_display })
}

/// Os campos de dígitos do `resolvedOptions`: `minimumIntegerDigits` e os fracionários e significativos.
pub fn put_digit_fields(global_object: &JSGlobalObject, options: &JSObject, settings: &NumberSettings) {
    put(global_object, options, "minimumIntegerDigits", number_value(settings.minimum_integer_digits));
    let fraction = |min: u32, max: u32| {
        put(global_object, options, "minimumFractionDigits", number_value(min));
        put(global_object, options, "maximumFractionDigits", number_value(max));
    };
    match settings.rounding {
        Rounding::FractionDigits { min, max } => fraction(min, max),
        Rounding::SignificantDigits { min, max } => {
            put(global_object, options, "minimumSignificantDigits", number_value(min));
            put(global_object, options, "maximumSignificantDigits", number_value(max));
        }
        Rounding::MorePrecision { min_fraction, max_fraction, min_significant, max_significant }
        | Rounding::LessPrecision { min_fraction, max_fraction, min_significant, max_significant } => {
            fraction(min_fraction, max_fraction);
            put(global_object, options, "minimumSignificantDigits", number_value(min_significant));
            put(global_object, options, "maximumSignificantDigits", number_value(max_significant));
        }
    }
}

/// Os campos de arredondamento do `resolvedOptions`: incremento, modo, prioridade e zeros finais.
pub fn put_rounding_fields(global_object: &JSGlobalObject, options: &JSObject, settings: &NumberSettings) {
    let vm = global_object.vm();
    put(global_object, options, "roundingIncrement", number_value(settings.rounding_increment));
    put(global_object, options, "roundingMode", str_value(vm, settings.rounding_mode.as_str()));
    let priority = match settings.rounding {
        Rounding::FractionDigits { .. } | Rounding::SignificantDigits { .. } => "auto",
        Rounding::MorePrecision { .. } => "morePrecision",
        Rounding::LessPrecision { .. } => "lessPrecision",
    };
    put(global_object, options, "roundingPriority", str_value(vm, priority));
    put(global_object, options, "trailingZeroDisplay", str_value(vm, settings.trailing_zero_display.as_str()));
}

/// `toIntlMathematicalValue`: o número (ou o BigInt exato) a formatar.
pub fn numeric_input(global_object: &JSGlobalObject, value: JSValue) -> Result<NumericInput, Thrown> {
    let primitive = value.to_primitive_preferred(PreferredPrimitiveType::PreferNumber);
    let primitive = pending_or(global_object, primitive)?;
    if primitive.is_string() {
        if let Some(exact) = exact_decimal_literal(&wtf_to_rust(&primitive.as_js_string().value())) {
            return Ok(exact);
        }
    }
    let numeric = primitive.to_numeric();
    let numeric = pending_or(global_object, numeric)?;
    if numeric.is_big_int() {
        let big_int = big_int_of(numeric).expect("isBigInt sem JSBigInt");
        let text = big_int.to_string(global_object, 10);
        let text = pending_or(global_object, text)?;
        let text = String::from_utf8_lossy(&text.latin1()).into_owned();
        let negative = text.starts_with('-');
        return Ok(NumericInput::Decimal { negative, digits: text.trim_start_matches('-').to_string() });
    }
    Ok(NumericInput::Double(to_number_checked(global_object, numeric)?))
}

/// O `StrDecimalLiteral` finito de um texto (`"99999999999999999999.99"`, `"-1.5e3"`) como decimal exato, sem
/// passar pelo `f64`: o `toIntlMathematicalValue` do spec. `None` para o que não é decimal finito (vazio,
/// `Infinity`, hexadecimal, lixo), que segue o caminho do `Number`.
fn exact_decimal_literal(text: &str) -> Option<NumericInput> {
    let trimmed = text.trim_matches(|c: char| c.is_whitespace() || c == '\u{feff}');
    let (negative, unsigned) = match trimmed.as_bytes().first()? {
        b'-' => (true, &trimmed[1..]),
        b'+' => (false, &trimmed[1..]),
        _ => (false, trimmed),
    };
    let (mantissa, exponent) = match unsigned.find(['e', 'E']) {
        Some(index) => (&unsigned[..index], unsigned[index + 1..].parse::<i32>().ok()?),
        None => (unsigned, 0),
    };
    let (whole, fraction) = mantissa.split_once('.').unwrap_or((mantissa, ""));
    let all_digits = |part: &str| part.bytes().all(|c| c.is_ascii_digit());
    if (whole.is_empty() && fraction.is_empty()) || !all_digits(whole) || !all_digits(fraction) || exponent.abs() > 10_000 {
        return None;
    }
    let digits = format!("{whole}{fraction}");
    let point = whole.len() as i64 + i64::from(exponent);
    let length = digits.len() as i64;
    let placed = if point <= 0 {
        format!("0.{}{digits}", "0".repeat((-point) as usize))
    } else if point >= length {
        format!("{digits}{}", "0".repeat((point - length) as usize))
    } else {
        format!("{}.{}", &digits[..point as usize], &digits[point as usize..])
    };
    Some(NumericInput::Decimal { negative, digits: placed })
}

/// `isUnicodeLocaleIdentifierType`: um ou mais trechos de 3 a 8 letras ou dígitos separados por `-`.
pub fn is_unicode_locale_identifier_type(text: &str) -> bool {
    !text.is_empty() && text.split('-').all(|part| (3..=8).contains(&part.len()) && part.bytes().all(|byte| byte.is_ascii_alphanumeric()))
}

/// O sistema numérico: a opção vence a chave `-u-nu-` da tag; o que o CLDR não tem em dígitos é ignorado (cai no
/// padrão do locale), e a chave só fica na tag resolvida se foi honrada e a opção não a trocou. Devolve o sistema, se
/// ele foi pedido e honrado (`explicit`), e a tag resolvida. `NumberFormat` e `DurationFormat` usam esta.
pub fn resolve_numbering_system(resolved: &ResolvedLocale, option: Option<&str>) -> (String, bool, String) {
    let base_locale = &resolved.locale;
    let option_honored = option.filter(|name| icu_number::numbering_system_honored(base_locale, name));
    let extension_honored = resolved.keyword("nu").filter(|name| icu_number::numbering_system_honored(base_locale, name));
    let (numbering_system, keep_extension) = match (option_honored, extension_honored) {
        (Some(option), extension) => (option.to_string(), extension == Some(option)),
        (None, Some(extension)) => (extension.to_string(), true),
        (None, None) => (icu_number::default_numbering_system(base_locale).to_string(), false),
    };
    let explicit = option_honored.is_some() || extension_honored.is_some();
    let honored: Vec<(&str, &str)> = if keep_extension { vec![("nu", numbering_system.as_str())] } else { Vec::new() };
    let locale = resolved.tag_with(&honored);
    (numbering_system, explicit, locale)
}

/// `IntlNumberFormat::initializeNumberFormat`.
fn initialize(global_object: &JSGlobalObject, locales: JSValue, options_value: JSValue) -> Result<NumberFormatState, Thrown> {
    let resolved = resolve_locale_from(global_object, locales, &["nu"])?;
    let options = coerce_options_to_object(global_object, options_value)?;
    read_locale_matcher(global_object, options)?;

    let numbering_option = option_string(global_object, options, "numberingSystem", &[], "")?.map(|text| text.to_ascii_lowercase());
    if let Some(numbering_system) = &numbering_option {
        if !is_unicode_locale_identifier_type(numbering_system) {
            return Err(Thrown::range_error("numberingSystem is not a well-formed numbering system value"));
        }
    }
    // O sistema numérico: a opção vence a chave `-u-nu-` da tag; o que o CLDR não tem em dígitos é ignorado
    // (cai no padrão do locale), e a chave só fica na tag resolvida se foi honrada e a opção não a trocou.
    let base_locale = resolved.locale.clone();
    let (numbering_system, explicit_numbering, locale) = resolve_numbering_system(&resolved, numbering_option.as_deref());

    let style = option_enum::<Style>(
        global_object,
        options,
        "style",
        "style must be either \"decimal\", \"percent\", \"currency\", or \"unit\"",
    )?
    .unwrap_or(Style::Decimal);

    let currency = option_string(global_object, options, "currency", &[], "")?;
    if let Some(currency) = &currency {
        if currency.len() != 3 || !currency.bytes().all(|byte| byte.is_ascii_alphabetic()) {
            return Err(Thrown::range_error("currency is not a well-formed currency code"));
        }
    }
    let mut currency_digits_default = 0;
    let mut currency_code = String::new();
    if style == Style::Currency {
        let Some(currency) = currency else { return Err(Thrown::type_error("currency must be a string")) };
        currency_code = currency.to_ascii_uppercase();
        currency_digits_default = currency_digits(&currency_code);
    }

    let currency_display = option_enum::<CurrencyDisplay>(
        global_object,
        options,
        "currencyDisplay",
        "currencyDisplay must be either \"code\", \"symbol\", or \"name\"",
    )?
    .unwrap_or(CurrencyDisplay::Symbol);
    let currency_sign = option_enum::<CurrencySign>(
        global_object,
        options,
        "currencySign",
        "currencySign must be either \"standard\" or \"accounting\"",
    )?
    .unwrap_or(CurrencySign::Standard);

    let unit = option_string(global_object, options, "unit", &[], "")?;
    let mut unit_id = String::new();
    match unit {
        Some(unit) => {
            if !is_known_unit(&unit) {
                return Err(Thrown::range_error("unit is not a well-formed unit identifier"));
            }
            unit_id = unit;
        }
        None if style == Style::Unit => return Err(Thrown::type_error("unit must be a string")),
        None => {}
    }
    let unit_display = option_enum::<UnitDisplay>(
        global_object,
        options,
        "unitDisplay",
        "unitDisplay must be either \"short\", \"narrow\", or \"long\"",
    )?
    .unwrap_or(UnitDisplay::Short);

    let notation = option_enum::<Notation>(
        global_object,
        options,
        "notation",
        "notation must be either \"standard\", \"scientific\", \"engineering\", or \"compact\"",
    )?
    .unwrap_or(Notation::Standard);

    let (minimum_fraction_default, maximum_fraction_default) = if style == Style::Currency && notation == Notation::Standard {
        (currency_digits_default, currency_digits_default)
    } else {
        (0, if style == Style::Percent { 0 } else { 3 })
    };
    let digits = read_digit_options(global_object, options, minimum_fraction_default, maximum_fraction_default, notation)?;

    let compact_display =
        option_enum::<CompactDisplay>(global_object, options, "compactDisplay", "compactDisplay must be either \"short\" or \"long\"")?
            .unwrap_or(CompactDisplay::Short);

    let default_grouping = if notation == Notation::Compact { UseGrouping::Min2 } else { UseGrouping::Auto };
    let use_grouping = read_use_grouping(global_object, options, default_grouping)?;

    let sign_display = option_enum::<SignDisplay>(
        global_object,
        options,
        "signDisplay",
        "signDisplay must be either \"auto\", \"never\", \"always\", \"exceptZero\", or \"negative\"",
    )?
    .unwrap_or(SignDisplay::Auto);

    let mut settings = NumberSettings::defaults(resolved.language);
    settings.locale = base_locale;
    if explicit_numbering {
        settings.numbering_system = numbering_system.clone();
    }
    settings.style = style;
    settings.currency = currency_code;
    settings.currency_display = currency_display;
    settings.currency_sign = currency_sign;
    settings.unit = unit_id;
    settings.unit_display = unit_display;
    settings.notation = notation;
    settings.compact_display = compact_display;
    settings.use_grouping = use_grouping;
    settings.sign_display = sign_display;
    digits.apply(&mut settings);
    Ok(NumberFormatState { locale, numbering_system, settings })
}

/// `intlStringOrBooleanOption` de `useGrouping`.
fn read_use_grouping(
    global_object: &JSGlobalObject,
    options: Option<JSValue>,
    fallback: UseGrouping,
) -> Result<UseGrouping, Thrown> {
    let Some(options) = options else { return Ok(fallback) };
    let value = get_property(global_object, options, "useGrouping")?;
    if value.is_undefined() {
        return Ok(fallback);
    }
    if value.is_boolean() && value.as_boolean() {
        return Ok(UseGrouping::Always);
    }
    let truthy = value.to_boolean();
    let truthy = pending_or(global_object, truthy)?;
    if !truthy {
        return Ok(UseGrouping::False);
    }
    let text = to_rust_string(global_object, value)?;
    if text == "true" || text == "false" {
        return Ok(fallback);
    }
    <UseGrouping as IntlEnum>::parse(&text)
        .ok_or_else(|| Thrown::range_error("useGrouping must be either true, false, \"min2\", \"auto\", or \"always\""))
}

/// `Number.prototype.toLocaleString`, `BigInt.prototype.toLocaleString` e o `format`: o texto de `input`
/// com `locales` e `options` (sem eles, o `defaultNumberFormat()` do locale padrão en-US).
pub fn to_locale_string(
    global_object: &JSGlobalObject,
    locales: JSValue,
    options: JSValue,
    input: &NumericInput,
) -> Result<String, Thrown> {
    if locales.is_undefined() && options.is_undefined() && !crate::runtime::intl_locale_data::default_locale_is_overridden() {
        return Ok(format_to_string(&NumberSettings::defaults(Language::English), input));
    }
    let state = initialize(global_object, locales, options)?;
    Ok(format_to_string(&state.settings, input))
}

fn construct_number_format_body(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    construct_instance(global_object, call, |global_object| {
        Ok(Box::new(initialize(global_object, call.argument(0), call.argument(1))?))
    })
}

/// `callNumberFormat`: sem `new`, sem ler `newTarget()`.
fn call_number_format_body(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    call_instance(global_object, call, |global_object| {
        Ok(Box::new(initialize(global_object, call.argument(0), call.argument(1))?))
    })
}

/// `intlNumberFormatFuncFormat`: o `format` ligado à instância.
fn format_function_body(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    with_instance::<NumberFormatState, _>(
        call.this_value(),
        "Intl.NumberFormat.prototype.format called on value that's not a NumberFormat",
        |state, _| {
            let input = numeric_input(global_object, call.argument(0))?;
            Ok(str_value(global_object.vm(), &format_to_string(&state.settings, &input)))
        },
    )
}

/// `intlNumberFormatPrototypeGetterFormat` (`CustomAccessor`).
fn format_getter_body(global_object: &JSGlobalObject, this_value: JSValue, _name: &PropertyName) -> HostResult {
    with_instance::<NumberFormatState, _>(
        this_value,
        "Intl.NumberFormat.prototype.format called on value that's not a NumberFormat",
        |_, instance| bound_function(global_object, instance, number_format_format, "format", 1),
    )
}

fn format_to_parts_body(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    with_instance::<NumberFormatState, _>(
        call.this_value(),
        "Intl.NumberFormat.prototype.formatToParts called on value that's not a NumberFormat",
        |state, _| {
            let input = numeric_input(global_object, call.argument(0))?;
            Ok(parts_array(global_object, &format_parts(&state.settings, &input)))
        },
    )
}

fn resolved_options_body(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    with_instance::<NumberFormatState, _>(
        call.this_value(),
        "Intl.NumberFormat.prototype.resolvedOptions called on value that's not a NumberFormat",
        |state, _| {
            let vm = global_object.vm();
            let settings = &state.settings;
            let options = new_object(global_object);
            put(global_object, &options, "locale", str_value(vm, &state.locale));
            put(global_object, &options, "numberingSystem", str_value(vm, &state.numbering_system));
            put(global_object, &options, "style", str_value(vm, settings.style.as_str()));
            match settings.style {
                Style::Decimal | Style::Percent => {}
                Style::Currency => {
                    put(global_object, &options, "currency", str_value(vm, &settings.currency));
                    put(global_object, &options, "currencyDisplay", str_value(vm, settings.currency_display.as_str()));
                    put(global_object, &options, "currencySign", str_value(vm, settings.currency_sign.as_str()));
                }
                Style::Unit => {
                    put(global_object, &options, "unit", str_value(vm, &settings.unit));
                    put(global_object, &options, "unitDisplay", str_value(vm, settings.unit_display.as_str()));
                }
            }
            put_digit_fields(global_object, &options, settings);
            let use_grouping = match settings.use_grouping {
                UseGrouping::False => js_boolean(false),
                other => str_value(vm, other.as_str()),
            };
            put(global_object, &options, "useGrouping", use_grouping);
            put(global_object, &options, "notation", str_value(vm, settings.notation.as_str()));
            if settings.notation == Notation::Compact {
                put(global_object, &options, "compactDisplay", str_value(vm, settings.compact_display.as_str()));
            }
            put(global_object, &options, "signDisplay", str_value(vm, settings.sign_display.as_str()));
            put_rounding_fields(global_object, &options, settings);
            Ok(options.as_value())
        },
    )
}

/// `intlNumberFormatPrototypeFuncFormatRange` e `...FormatRangeToParts`: os dois extremos pelo
/// `toIntlMathematicalValue`, `undefined` e `NaN` rejeitados.
fn range_parts_of(
    global_object: &JSGlobalObject,
    call: &HostCall,
    state: &NumberFormatState,
) -> Result<Vec<RangePart>, Thrown> {
    let (start_value, end_value) = (call.argument(0), call.argument(1));
    if start_value.is_undefined() || end_value.is_undefined() {
        return Err(Thrown::type_error("start or end is undefined"));
    }
    let start = numeric_input(global_object, start_value)?;
    let end = numeric_input(global_object, end_value)?;
    let is_nan = |input: &NumericInput| matches!(input, NumericInput::Double(value) if value.is_nan());
    if is_nan(&start) || is_nan(&end) {
        return Err(Thrown::range_error("Passed numbers are out of range"));
    }
    Ok(format_range_parts(&state.settings, &start, &end))
}

fn format_range_body(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    with_instance::<NumberFormatState, _>(
        call.this_value(),
        "Intl.NumberFormat.prototype.formatRange called on value that's not a NumberFormat",
        |state, _| {
            let parts = range_parts_of(global_object, call, state)?;
            let text: String = parts.into_iter().map(|(_, text, _)| text).collect();
            Ok(str_value(global_object.vm(), &text))
        },
    )
}

fn format_range_to_parts_body(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    with_instance::<NumberFormatState, _>(
        call.this_value(),
        "Intl.NumberFormat.prototype.formatRangeToParts called on value that's not a NumberFormat",
        |state, _| Ok(range_parts_array(global_object, &range_parts_of(global_object, call, state)?)),
    )
}

host_function!(call_number_format, call_number_format_body);
host_function!(construct_number_format, construct_number_format_body);
host_function!(number_format_format, format_function_body);
custom_getter!(number_format_proto_format_getter, format_getter_body);
host_function!(number_format_proto_format_to_parts, format_to_parts_body);
host_function!(number_format_proto_format_range, format_range_body);
host_function!(number_format_proto_format_range_to_parts, format_range_to_parts_body);
host_function!(number_format_proto_resolved_options, resolved_options_body);

/// `numberFormatPrototypeTableValues` de `IntlNumberFormatPrototype.lut.h`, na ordem do `@begin`.
static NUMBER_FORMAT_PROTOTYPE_TABLE_VALUES: [HashTableValue; 5] = intl_format_prototype_values(
    number_format_proto_format_getter,
    number_format_proto_format_range,
    number_format_proto_format_range_to_parts,
    number_format_proto_format_to_parts,
    number_format_proto_resolved_options,
);

/// `numberFormatPrototypeTable`.
static NUMBER_FORMAT_PROTOTYPE_TABLE: HashTable =
    HashTable { class_for_this: None, values: &NUMBER_FORMAT_PROTOTYPE_TABLE_VALUES };

/// `IntlNumberFormatPrototype::s_info` (`"Intl.NumberFormat"`).
static NUMBER_FORMAT_PROTOTYPE_S_INFO: ClassInfo = ClassInfo {
    class_name: "Intl.NumberFormat",
    parent_class: Some(&JS_NON_FINAL_OBJECT_S_INFO),
    static_prop_hash_table: Some(&NUMBER_FORMAT_PROTOTYPE_TABLE),
    inherits_js_type_range: None,
};

/// `IntlNumberFormatConstructor` e `IntlNumberFormatPrototype` (`format`, `formatToParts`,
/// `formatRange`, `formatRangeToParts`, `resolvedOptions`, da tabela estática, reificadas no primeiro acesso).
/// O construtor funciona com e sem `new`.
pub fn install_number_format(global_object: &JSGlobalObject, intl: &JSObject) {
    let class = IntlClass {
        name: "NumberFormat",
        length: 0,
        has_supported_locales_of: true,
        call: call_number_format,
        construct: construct_number_format,
    };
    class.install_with_table(global_object, intl, &NUMBER_FORMAT_PROTOTYPE_S_INFO);
}
