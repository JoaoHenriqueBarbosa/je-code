//! `Intl.DurationFormat` sem ICU (`IntlDurationFormat.cpp`, `IntlDurationFormatPrototype.cpp`,
//! `IntlDurationFormatConstructor.cpp`): a leitura das opções por unidade, `format`, `formatToParts` e
//! `resolvedOptions`, e `Temporal.Duration.prototype.toLocaleString` (`to_locale_string`).
//!
//! O número de cada unidade sai do mesmo formatador do `Intl.NumberFormat` (`default_number_format::format_parts`
//! sobre `icu_number.rs`), com o locale resolvido e o sistema numérico de `resolve_numbering_system` (a opção
//! `numberingSystem` ou a chave `-u-nu-`); as unidades entre elas, da lista `unit` do `intl_list_format.rs` (o
//! `ULISTFMT_TYPE_UNITS`).
//!
//! LOCALES cobertas: as 38 de `intl_duration_format_data` (unidades, separador digital e lista `unit` pela primeira
//! tag pedida); o número usa o separador decimal, o agrupamento e os dígitos do locale.
//!
//! DIVERGÊNCIAS e LACUNAS, e por quê:
//!
//! - Os padrões das unidades e o separador de horas, minutos e segundos (`retrieveSeparator`) vêm dos dados
//!   medidos no bun (`scripts/gen-duration-format-data.js`). Nada disso foi compilado nem testado ainda.
//! - O separador de horas, minutos e segundos do estilo digital vem dos dados medidos e não troca com o sistema
//!   numérico pedido (o ICU também lê o do locale).
//! - Replica o C++ no sinal da fração: `buildDecimalFormat` escreve o sinal pela parte inteira, então
//!   `{ milliseconds: -500 }` em `digital` perde o sinal (`0.5`), porque a parte inteira em segundos é 0.

use crate::host_function;
use crate::runtime::lookup::{native_entry};
use crate::runtime::default_number_format::{format_parts, NumberSettings, NumericInput, Part, Rounding, RoundingMode, UseGrouping};
use crate::runtime::host_call::{HostCall, HostResult, Thrown};
use crate::runtime::icu_plural;
use crate::runtime::intl_duration_format_data::{DIGITAL_SEPARATORS, LOCALES, PATTERNS, POOL};
use crate::runtime::intl_list_format::{list_parts, ListStyle, ListType};
use crate::runtime::intl_locale_data::best_available_by;
use crate::runtime::intl_number_format::{is_unicode_locale_identifier_type, resolve_numbering_system};
use crate::runtime::intl_plural_rules::PluralOperands;
use crate::runtime::intl_support::{
    array_of, canonicalize_locale_list, construct_instance, get_options_object, new_object, number_option, number_value, option_enum, option_string, part_object, put,
    read_locale_matcher, resolve_locale_from, str_value, with_instance, IntlClass, IntlEnum,
};
use crate::runtime::iso8601::Duration;
use crate::runtime::js_global_object::JSGlobalObject;
use crate::runtime::js_object::JSObject;
use crate::runtime::js_value::JSValue;
use crate::runtime::temporal_core_duration::duration_sign;
use crate::runtime::temporal_duration::TemporalDuration;
use crate::runtime::temporal_object::{TemporalUnit, NUMBER_OF_TEMPORAL_UNITS};

crate::intl_enum!(DurationStyle { Long => "long", Short => "short", Narrow => "narrow", Digital => "digital" });
crate::intl_enum!(UnitStyle {
    Long => "long", Short => "short", Narrow => "narrow", Numeric => "numeric", TwoDigit => "2-digit"
});
crate::intl_enum!(Display { Auto => "auto", Always => "always" });

/// `IntlDurationFormat::UnitData`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct UnitData {
    style: UnitStyle,
    display: Display,
}

/// O estado de um `IntlDurationFormat`.
struct DurationFormatState {
    locale: String,
    /// A primeira tag pedida (ou a resolvida): o locale dos padrões das unidades e da lista.
    tag: String,
    /// `m_numberingSystem`: o sistema numérico resolvido (`latn`, `arab`, `deva`...).
    numbering_system: String,
    /// A base do formatador de número de cada unidade: o locale resolvido e o sistema numérico, como no
    /// `Intl.NumberFormat` (`resolve_numbering_system`).
    number: NumberSettings,
    style: DurationStyle,
    units: [UnitData; NUMBER_OF_TEMPORAL_UNITS],
    /// `m_fractionalDigits`; `None` é o `fractionalDigitsUndefinedValue`.
    fractional_digits: Option<u32>,
}

const PLURAL_NAMES: [&str; NUMBER_OF_TEMPORAL_UNITS] =
    ["years", "months", "weeks", "days", "hours", "minutes", "seconds", "milliseconds", "microseconds", "nanoseconds"];
const SINGULAR_NAMES: [&str; NUMBER_OF_TEMPORAL_UNITS] =
    ["year", "month", "week", "day", "hour", "minute", "second", "millisecond", "microsecond", "nanosecond"];
const DISPLAY_NAMES: [&str; NUMBER_OF_TEMPORAL_UNITS] = [
    "yearsDisplay",
    "monthsDisplay",
    "weeksDisplay",
    "daysDisplay",
    "hoursDisplay",
    "minutesDisplay",
    "secondsDisplay",
    "millisecondsDisplay",
    "microsecondsDisplay",
    "nanosecondsDisplay",
];

/// O locale dos dados de `tag`: a tag pedida cortada até uma das 38 locales de `intl_duration_format_data`
/// (`en-US` vira `en`, `pt-BR` vira `pt`, `es-MX` fica); sem dados, `en`.
fn data_locale(tag: &str) -> &'static str {
    let found = best_available_by(tag, |candidate| LOCALES.contains(&candidate));
    LOCALES.iter().copied().find(|locale| Some(*locale) == found.as_deref()).unwrap_or("en")
}

/// As partes `(tipo, valor)` do padrão da unidade (`n` número, `l` literal, `u` unidade) para a categoria de
/// plural; categoria sem padrão usa `other`. O padrão decide se o número aparece e se há espaço.
fn unit_pattern(tag: &str, unit: TemporalUnit, style: UnitStyle, category: &str) -> Vec<(&'static str, &'static str)> {
    let lang = data_locale(tag);
    let style_index = match style {
        UnitStyle::Long => 0,
        UnitStyle::Short => 1,
        _ => 2,
    };
    let find = |wanted: &str| {
        PATTERNS
            .iter()
            .find(|pattern| pattern.lang == lang && pattern.style == style_index && pattern.unit == unit as u8 && pattern.category == wanted)
    };
    let Some(pattern) = find(category).or_else(|| find("other")) else { return Vec::new() };
    POOL[pattern.text as usize].split('\u{1f}').filter_map(|part| part.split_once(':')).collect()
}

/// O separador de horas, minutos e segundos do estilo digital (`retrieveSeparator`).
fn digital_separator(tag: &str) -> &'static str {
    let lang = data_locale(tag);
    DIGITAL_SEPARATORS.iter().find(|separator| separator.lang == lang).map_or(":", |separator| separator.text)
}

// ---------------------------------------------------------------------------------------------
// Opções
// ---------------------------------------------------------------------------------------------

/// O estilo e o `display` de uma unidade quando as opções não dizem (`intlDurationUnitOptions`, o que vem
/// depois da leitura).
fn unit_defaults(unit: TemporalUnit, base: DurationStyle, style_given: bool, prev: Option<UnitStyle>) -> (UnitStyle, Display) {
    if style_given {
        return (UnitStyle::Short, Display::Always);
    }
    let prev_numeric = matches!(prev, Some(UnitStyle::Numeric | UnitStyle::TwoDigit));
    if base == DurationStyle::Digital {
        let display = if matches!(unit, TemporalUnit::Hour | TemporalUnit::Minute | TemporalUnit::Second) {
            Display::Always
        } else {
            Display::Auto
        };
        let style = if (unit as usize) < TemporalUnit::Hour as usize { UnitStyle::Short } else { UnitStyle::Numeric };
        return (style, display);
    }
    let style = if prev_numeric {
        UnitStyle::Numeric
    } else {
        match base {
            DurationStyle::Long => UnitStyle::Long,
            DurationStyle::Narrow => UnitStyle::Narrow,
            _ => UnitStyle::Short,
        }
    };
    (style, Display::Auto)
}

/// A conferência final de `intlDurationUnitOptions`: depois de uma unidade numérica só cabem numéricas, e
/// minutos e segundos nesse caso viram `2-digit`.
fn finish_unit(unit: TemporalUnit, mut data: UnitData, prev: Option<UnitStyle>) -> Result<UnitData, Thrown> {
    if matches!(prev, Some(UnitStyle::Numeric | UnitStyle::TwoDigit)) {
        if !matches!(data.style, UnitStyle::Numeric | UnitStyle::TwoDigit) {
            return Err(Thrown::range_error("style option is inconsistent"));
        }
        if matches!(unit, TemporalUnit::Minute | TemporalUnit::Second) {
            data.style = UnitStyle::TwoDigit;
        }
    }
    Ok(data)
}

/// `intlDurationUnitOptions(globalObject, options, unit, ...)`.
fn read_unit_options(
    global_object: &JSGlobalObject,
    options: Option<JSValue>,
    unit: TemporalUnit,
    base: DurationStyle,
    prev: Option<UnitStyle>,
) -> Result<UnitData, Thrown> {
    let index = unit as usize;
    let (allowed, message): (&[&str], &str) = match index {
        0..=3 => (&["long", "short", "narrow"], "style must be either \"long\", \"short\", or \"narrow\""),
        4..=6 => (
            &["long", "short", "narrow", "numeric", "2-digit"],
            "style must be either \"long\", \"short\", \"narrow\" or \"numeric\", or \"2-digit\"",
        ),
        _ => (&["long", "short", "narrow", "numeric"], "style must be either \"long\", \"short\", \"narrow\", or \"numeric\""),
    };
    let given = option_string(global_object, options, PLURAL_NAMES[index], allowed, message)?.and_then(|text| UnitStyle::parse(&text));
    let (default_style, default_display) = unit_defaults(unit, base, given.is_some(), prev);
    let display = option_enum::<Display>(
        global_object,
        options,
        DISPLAY_NAMES[index],
        "display name must be either \"auto\" or \"always\"",
    )?
    .unwrap_or(default_display);
    finish_unit(unit, UnitData { style: given.unwrap_or(default_style), display }, prev)
}

/// O `prevStyle` seguinte: só horas até microssegundos o atualizam.
fn next_prev(unit: TemporalUnit, data: UnitData, prev: Option<UnitStyle>) -> Option<UnitStyle> {
    if (TemporalUnit::Hour as usize..=TemporalUnit::Microsecond as usize).contains(&(unit as usize)) {
        Some(data.style)
    } else {
        prev
    }
}

/// `IntlDurationFormat::initializeDurationFormat`.
fn initialize(global_object: &JSGlobalObject, locales: JSValue, options_value: JSValue) -> Result<DurationFormatState, Thrown> {
    let resolved = resolve_locale_from(global_object, locales, &["nu"])?;
    // Os padrões e a lista saem da primeira tag pedida (o ICU do bun formata pelo locale pedido).
    let tag = canonicalize_locale_list(global_object, locales)?.into_iter().next().unwrap_or_else(|| resolved.locale.clone());
    let options = get_options_object(options_value)?;
    read_locale_matcher(global_object, options)?;

    let numbering_system = option_string(global_object, options, "numberingSystem", &[], "")?;
    if let Some(numbering_system) = &numbering_system {
        if !is_unicode_locale_identifier_type(numbering_system) {
            return Err(Thrown::range_error("numberingSystem is not a well-formed numbering system value"));
        }
    }
    let (numbering_system, explicit_numbering, locale) = resolve_numbering_system(&resolved, numbering_system.map(|name| name.to_ascii_lowercase()).as_deref());
    let mut number = NumberSettings::defaults(resolved.language);
    number.locale = resolved.locale.clone();
    if explicit_numbering {
        number.numbering_system = numbering_system.clone();
    }

    let style = option_enum::<DurationStyle>(
        global_object,
        options,
        "style",
        "style must be either \"long\", \"short\", \"narrow\", or \"digital\"",
    )?
    .unwrap_or(DurationStyle::Short);

    let mut units = [UnitData { style: UnitStyle::Short, display: Display::Auto }; NUMBER_OF_TEMPORAL_UNITS];
    let mut prev: Option<UnitStyle> = None;
    for unit in TemporalUnit::ALL {
        let data = read_unit_options(global_object, options, unit, style, prev)?;
        units[unit as usize] = data;
        prev = next_prev(unit, data, prev);
    }

    let fractional_digits = number_option(global_object, options, "fractionalDigits", 0, 9)?;
    Ok(DurationFormatState { locale, numbering_system, tag, number, style, units, fractional_digits })
}

// ---------------------------------------------------------------------------------------------
// Formatação
// ---------------------------------------------------------------------------------------------

/// Um elemento do `PartitionDurationFormatPattern`: um número com a unidade, ou o separador entre dois.
struct Element {
    unit: TemporalUnit,
    /// `ElementType::Literal`.
    literal: bool,
    parts: Vec<Part>,
}

impl Element {
    fn text(&self) -> String {
        self.parts.iter().map(|(_, text)| text.as_str()).collect()
    }
}

/// `buildDecimalFormat`: os nanossegundos totais como decimal da unidade (`seconds`, `milliseconds` ou
/// `microseconds`), com o sinal escrito pela parte inteira.
fn build_decimal_format(unit: TemporalUnit, ns: i128) -> NumericInput {
    let (digits, exponent): (usize, i128) = match unit {
        TemporalUnit::Second => (9, 1_000_000_000),
        TemporalUnit::Millisecond => (6, 1_000_000),
        _ => (3, 1_000),
    };
    let integer = ns / exponent;
    let fraction = (ns % exponent).unsigned_abs();
    NumericInput::Decimal { negative: integer < 0, digits: format!("{}.{fraction:0digits$}", integer.unsigned_abs()) }
}

/// A categoria de plural cardinal do número já formatado, pelas regras do locale `tag`.
fn plural_category(tag: &str, number: &[Part]) -> &'static str {
    let join = |kind: &str| -> String { number.iter().filter(|(part, _)| part == kind).map(|(_, text)| text.as_str()).collect() };
    let operands = PluralOperands::from_decimal(&join("integer"), &join("fraction"));
    icu_plural::select(tag, false, &operands.integer, &operands.fraction, 0).unwrap_or("other")
}

/// Quanto do número cada unidade pede: os dígitos fracionários (`rounding-mode-down` com `.###`).
fn number_settings(base: &NumberSettings, style: UnitStyle, fraction: Option<(u32, u32)>) -> NumberSettings {
    let mut settings = base.clone();
    if style == UnitStyle::TwoDigit {
        settings.minimum_integer_digits = 2;
    }
    if matches!(style, UnitStyle::TwoDigit | UnitStyle::Numeric) {
        settings.use_grouping = UseGrouping::False;
    }
    if let Some((min, max)) = fraction {
        settings.rounding = Rounding::FractionDigits { min, max };
        settings.rounding_mode = RoundingMode::Trunc;
    }
    settings
}

/// `collectElements` (`PartitionDurationFormatPattern`).
fn collect_elements(state: &DurationFormatState, duration: &Duration) -> Vec<Element> {
    let mut elements: Vec<Element> = Vec::new();
    let mut done = false;
    let mut needs_sign_display = false;
    let mut duration_sign_value: Option<i32> = None;
    let unit_data = |unit: TemporalUnit| state.units[unit as usize];

    for unit in TemporalUnit::ALL {
        if done {
            break;
        }
        let data = unit_data(unit);
        let mut value = duration.field(unit);
        let mut total_ns: Option<i128> = None;
        let mut fraction: Option<(u32, u32)> = None;

        if matches!(unit, TemporalUnit::Second | TemporalUnit::Millisecond | TemporalUnit::Microsecond) {
            let next = TemporalUnit::ALL[unit as usize + 1];
            if unit_data(next).style == UnitStyle::Numeric {
                total_ns = Some(duration.total_nanoseconds_from(unit).expect("Duration válida não estoura Int128"));
                fraction = Some(match state.fractional_digits {
                    None => (0, 9),
                    Some(digits) => (digits, digits),
                });
                done = true;
            }
        }

        let style = data.style;
        let numeric = matches!(style, UnitStyle::TwoDigit | UnitStyle::Numeric);
        if value == 0.0 && data.display == Display::Auto && !numeric {
            continue;
        }

        let suppress_sign = needs_sign_display;
        let mut adjust_sign_display = |value: &mut f64, needs_sign_display: &mut bool| {
            if !*needs_sign_display && *value == 0.0 {
                let sign = *duration_sign_value.get_or_insert_with(|| duration_sign(duration));
                if sign < 0 {
                    *value = -0.0;
                    *needs_sign_display = true;
                }
            }
        };
        let format_number = |value: f64| -> Vec<Part> {
            let input = match total_ns {
                Some(ns) => build_decimal_format(unit, if suppress_sign { ns.abs() } else { ns }),
                None => NumericInput::Double(if suppress_sign { value.abs() } else { value }),
            };
            format_parts(&number_settings(&state.number, style, fraction), &input)
        };

        if numeric {
            // `FormatNumericUnits`.
            let mut seconds = duration.field(TemporalUnit::Second);
            if unit_data(TemporalUnit::Millisecond).style == UnitStyle::Numeric {
                seconds = seconds
                    + duration.field(TemporalUnit::Millisecond) / 1000.0
                    + duration.field(TemporalUnit::Microsecond) / 1_000_000.0
                    + duration.field(TemporalUnit::Nanosecond) / 1_000_000_000.0;
            }
            let needs_hours = duration.field(TemporalUnit::Hour) != 0.0 || unit_data(TemporalUnit::Hour).display != Display::Auto;
            let needs_seconds = seconds != 0.0 || unit_data(TemporalUnit::Second).display != Display::Auto;
            let needs_minutes = (needs_hours && needs_seconds)
                || duration.field(TemporalUnit::Minute) != 0.0
                || unit_data(TemporalUnit::Minute).display != Display::Auto;
            let needs_format = (unit == TemporalUnit::Hour && needs_hours)
                || (unit == TemporalUnit::Minute && needs_minutes)
                || (unit == TemporalUnit::Second && needs_seconds);
            let needs_separator = (unit == TemporalUnit::Hour && needs_hours && needs_minutes)
                || (unit == TemporalUnit::Minute && needs_minutes && needs_seconds);
            if needs_format {
                adjust_sign_display(&mut value, &mut needs_sign_display);
                elements.push(Element { unit, literal: false, parts: format_number(value) });
            }
            if needs_separator {
                elements.push(Element { unit, literal: true, parts: vec![("literal".to_string(), digital_separator(&state.tag).to_string())] });
            }
        } else {
            adjust_sign_display(&mut value, &mut needs_sign_display);
            let number = format_number(value);
            let category = plural_category(&state.tag, &number);
            let mut parts: Vec<Part> = Vec::new();
            for (kind, text) in unit_pattern(&state.tag, unit, style, category) {
                match kind {
                    "n" => parts.extend(number.iter().cloned()),
                    "l" => parts.push(("literal".to_string(), text.to_string())),
                    _ => parts.push(("unit".to_string(), text.to_string())),
                }
            }
            elements.push(Element { unit, literal: false, parts });
        }
        if value != 0.0 {
            needs_sign_display = true;
        }
    }
    elements
}

/// Junta cada elemento com o separador e o elemento seguinte (`1:02:03` é um item só da lista).
fn group_elements(elements: Vec<Element>) -> Vec<Vec<Element>> {
    let mut groups: Vec<Vec<Element>> = Vec::new();
    let mut iterator = elements.into_iter().peekable();
    while let Some(first) = iterator.next() {
        let mut group = vec![first];
        while iterator.peek().is_some_and(|next| next.literal) {
            group.push(iterator.next().expect("peek"));
            if iterator.peek().is_some_and(|next| !next.literal) {
                group.push(iterator.next().expect("peek"));
            } else {
                break;
            }
        }
        groups.push(group);
    }
    groups
}

/// A lista `unit` do formatador: o estilo da lista é o do formatador (`digital` vira `short`).
fn list_style(style: DurationStyle) -> ListStyle {
    match style {
        DurationStyle::Long => ListStyle::Long,
        DurationStyle::Short | DurationStyle::Digital => ListStyle::Short,
        DurationStyle::Narrow => ListStyle::Narrow,
    }
}

/// As partes do resultado: tipo, texto e a unidade (`None` nos textos da lista e no separador).
type DurationPart = (String, String, Option<TemporalUnit>);

fn duration_parts(state: &DurationFormatState, duration: &Duration) -> Vec<DurationPart> {
    let groups = group_elements(collect_elements(state, duration));
    let strings: Vec<String> = groups.iter().map(|group| group.iter().map(Element::text).collect()).collect();
    let mut groups = groups.into_iter();
    let mut result: Vec<DurationPart> = Vec::new();
    for (kind, text) in list_parts(&state.tag, ListType::Unit, list_style(state.style), &strings) {
        if kind == "literal" {
            result.push((kind, text, None));
            continue;
        }
        for element in groups.next().expect("um grupo por elemento da lista") {
            for (part_kind, part_text) in element.parts {
                let unit = if element.literal { None } else { Some(element.unit) };
                result.push((part_kind, part_text, unit));
            }
        }
    }
    result
}

/// `IntlDurationFormat::format`.
fn format_text(state: &DurationFormatState, duration: &Duration) -> String {
    duration_parts(state, duration).into_iter().map(|(_, text, _)| text).collect()
}

/// `IntlDurationFormat::formatToParts`: `{ type, value }` mais `unit` nas partes de um número.
fn format_to_parts(global_object: &JSGlobalObject, state: &DurationFormatState, duration: &Duration) -> JSValue {
    let vm = global_object.vm();
    let values: Vec<JSValue> = duration_parts(state, duration)
        .into_iter()
        .map(|(kind, text, unit)| match unit {
            None => part_object(global_object, &kind, &text),
            Some(unit) => {
                let object = new_object(global_object);
                put(global_object, &object, "type", str_value(vm, &kind));
                put(global_object, &object, "value", str_value(vm, &text));
                put(global_object, &object, "unit", str_value(vm, SINGULAR_NAMES[unit as usize]));
                object.as_value()
            }
        })
        .collect();
    array_of(global_object, &values)
}

/// O argumento de `format` e `formatToParts`: objeto ou string, vira o registro da `Duration`.
fn duration_argument(global_object: &JSGlobalObject, call: &HostCall, member: &str) -> Result<Duration, Thrown> {
    let argument = call.argument(0);
    if !argument.is_object() && !argument.is_string() {
        return Err(Thrown::TypeError(format!("Intl.DurationFormat.prototype.{member} argument needs to be an object or a string")));
    }
    TemporalDuration::to_temporal_duration_record(global_object, argument)
}

/// `Temporal.Duration.prototype.toLocaleString` (ECMA-402): um `IntlDurationFormat` descartável com `locales` e
/// `options`, depois `format`.
pub fn to_locale_string(global_object: &JSGlobalObject, locales: JSValue, options: JSValue, duration: &Duration) -> Result<String, Thrown> {
    let state = initialize(global_object, locales, options)?;
    Ok(format_text(&state, duration))
}

fn construct_duration_format_body(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    construct_instance(global_object, call, |global_object| {
        Ok(Box::new(initialize(global_object, call.argument(0), call.argument(1))?))
    })
}

fn call_duration_format_body(_global_object: &JSGlobalObject, _call: &HostCall) -> HostResult {
    crate::runtime::collection_support::constructor_cannot_be_called_as_function("DurationFormat")
}

fn format_body(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    with_instance::<DurationFormatState, _>(
        call.this_value(),
        "Intl.DurationFormat.prototype.format called on value that's not a DurationFormat",
        |state, _| {
            let duration = duration_argument(global_object, call, "format")?;
            Ok(str_value(global_object.vm(), &format_text(state, &duration)))
        },
    )
}

fn format_to_parts_body(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    with_instance::<DurationFormatState, _>(
        call.this_value(),
        "Intl.DurationFormat.prototype.formatToParts called on value that's not a DurationFormat",
        |state, _| {
            let duration = duration_argument(global_object, call, "formatToParts")?;
            Ok(format_to_parts(global_object, state, &duration))
        },
    )
}

fn resolved_options_body(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    with_instance::<DurationFormatState, _>(
        call.this_value(),
        "Intl.DurationFormat.prototype.resolvedOptions called on value that's not a DurationFormat",
        |state, _| {
            let vm = global_object.vm();
            let options = new_object(global_object);
            put(global_object, &options, "locale", str_value(vm, &state.locale));
            put(global_object, &options, "numberingSystem", str_value(vm, &state.numbering_system));
            put(global_object, &options, "style", str_value(vm, state.style.as_str()));
            for index in 0..NUMBER_OF_TEMPORAL_UNITS {
                put(global_object, &options, PLURAL_NAMES[index], str_value(vm, state.units[index].style.as_str()));
                put(global_object, &options, DISPLAY_NAMES[index], str_value(vm, state.units[index].display.as_str()));
            }
            if let Some(digits) = state.fractional_digits {
                put(global_object, &options, "fractionalDigits", number_value(digits));
            }
            Ok(options.as_value())
        },
    )
}

host_function!(call_duration_format, call_duration_format_body);
host_function!(construct_duration_format, construct_duration_format_body);
host_function!(duration_format_proto_format, format_body);
host_function!(duration_format_proto_format_to_parts, format_to_parts_body);
host_function!(duration_format_proto_resolved_options, resolved_options_body);

crate::intl_prototype_s_info!(
    DURATION_FORMAT_PROTOTYPE_S_INFO,
    "Intl.DurationFormat",
    [
        native_entry("format", duration_format_proto_format, 1),
        native_entry("formatToParts", duration_format_proto_format_to_parts, 1),
        native_entry("resolvedOptions", duration_format_proto_resolved_options, 0),
    ]
);

/// `IntlDurationFormatConstructor` e `IntlDurationFormatPrototype`.
pub fn install_duration_format(global_object: &JSGlobalObject, intl: &JSObject) {
    let class = IntlClass {
        name: "DurationFormat",
        length: 0,
        has_supported_locales_of: true,
        call: call_duration_format,
        construct: construct_duration_format,
    };
    class.install_with_table(global_object, intl, &DURATION_FORMAT_PROTOTYPE_S_INFO);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::runtime::intl_locale_data::Language;

    /// O estado que as opções vazias produzem (`style` e o resto no padrão).
    fn state(language: Language, style: DurationStyle, fractional_digits: Option<u32>) -> DurationFormatState {
        let mut units = [UnitData { style: UnitStyle::Short, display: Display::Auto }; NUMBER_OF_TEMPORAL_UNITS];
        let mut prev = None;
        for unit in TemporalUnit::ALL {
            let (unit_style, display) = unit_defaults(unit, style, false, prev);
            let data = finish_unit(unit, UnitData { style: unit_style, display }, prev).expect("padrão consistente");
            units[unit as usize] = data;
            prev = next_prev(unit, data, prev);
        }
        let tag = if language == Language::Portuguese { "pt" } else { "en" }.to_string();
        DurationFormatState {
            locale: String::new(),
            numbering_system: "latn".to_string(),
            tag,
            number: NumberSettings::defaults(language),
            style,
            units,
            fractional_digits,
        }
    }

    fn duration(hours: i64, minutes: i64, seconds: i64, milliseconds: i64) -> Duration {
        Duration::new(0, 0, 0, 0, hours, minutes, seconds, milliseconds, 0, 0)
    }

    fn en(style: DurationStyle, duration: &Duration) -> String {
        format_text(&state(Language::English, style, None), duration)
    }

    #[test]
    fn english_short_long_narrow() {
        let d = duration(1, 2, 3, 0);
        assert_eq!(en(DurationStyle::Short, &d), "1 hr, 2 min, 3 sec");
        assert_eq!(en(DurationStyle::Long, &d), "1 hour, 2 minutes, 3 seconds");
        assert_eq!(en(DurationStyle::Narrow, &d), "1h 2m 3s");
        assert_eq!(en(DurationStyle::Short, &duration(0, 0, 0, 0)), "");
    }

    #[test]
    fn digital_style() {
        assert_eq!(en(DurationStyle::Digital, &duration(1, 2, 3, 0)), "1:02:03");
        assert_eq!(en(DurationStyle::Digital, &duration(0, 5, 7, 0)), "0:05:07");
        assert_eq!(en(DurationStyle::Digital, &duration(1, 2, 3, 456)), "1:02:03.456");
        assert_eq!(en(DurationStyle::Digital, &duration(0, 0, 0, 0)), "0:00:00");
    }

    #[test]
    fn fractional_digits_pad_and_truncate() {
        let state = state(Language::English, DurationStyle::Digital, Some(2));
        assert_eq!(format_text(&state, &duration(0, 0, 3, 456)), "0:00:03.45");
        assert_eq!(format_text(&state, &duration(0, 0, 3, 0)), "0:00:03.00");
    }

    #[test]
    fn sign_only_on_the_first_unit() {
        assert_eq!(en(DurationStyle::Short, &duration(-1, -2, 0, 0)), "-1 hr, 2 min");
        assert_eq!(en(DurationStyle::Short, &duration(0, -2, -3, 0)), "-2 min, 3 sec");
    }

    #[test]
    fn plural_follows_the_formatted_number() {
        let d = Duration::new(0, 0, 0, 1, 0, 0, 0, 0, 0, 0);
        assert_eq!(en(DurationStyle::Long, &d), "1 day");
        assert_eq!(en(DurationStyle::Long, &Duration::new(0, 0, 0, 2, 0, 0, 0, 0, 0, 0)), "2 days");
    }

    #[test]
    fn portuguese_lists() {
        let state = state(Language::Portuguese, DurationStyle::Long, None);
        assert_eq!(format_text(&state, &duration(2, 30, 0, 0)), "2 horas e 30 minutos");
        assert_eq!(format_text(&state, &duration(1, 0, 0, 0)), "1 hora");
    }

    #[test]
    fn parts_carry_the_unit() {
        let parts = duration_parts(&state(Language::English, DurationStyle::Short, None), &duration(1, 2, 0, 0));
        let flat: Vec<(&str, &str, Option<TemporalUnit>)> =
            parts.iter().map(|(kind, text, unit)| (kind.as_str(), text.as_str(), *unit)).collect();
        assert_eq!(
            flat,
            [
                ("integer", "1", Some(TemporalUnit::Hour)),
                ("literal", " ", Some(TemporalUnit::Hour)),
                ("unit", "hr", Some(TemporalUnit::Hour)),
                ("literal", ", ", None),
                ("integer", "2", Some(TemporalUnit::Minute)),
                ("literal", " ", Some(TemporalUnit::Minute)),
                ("unit", "min", Some(TemporalUnit::Minute)),
            ]
        );
    }

    #[test]
    fn digital_separator_has_no_unit() {
        let parts = duration_parts(&state(Language::English, DurationStyle::Digital, None), &duration(1, 2, 3, 0));
        let separators: Vec<Option<TemporalUnit>> = parts.iter().filter(|(_, text, _)| text == ":").map(|(_, _, unit)| *unit).collect();
        assert_eq!(separators, [None, None]);
    }

    #[test]
    fn inconsistent_style_after_numeric_is_an_error() {
        let prev = Some(UnitStyle::Numeric);
        let data = UnitData { style: UnitStyle::Long, display: Display::Auto };
        assert!(finish_unit(TemporalUnit::Minute, data, prev).is_err());
        let numeric = UnitData { style: UnitStyle::Numeric, display: Display::Auto };
        assert_eq!(finish_unit(TemporalUnit::Minute, numeric, prev).unwrap().style, UnitStyle::TwoDigit);
        assert_eq!(finish_unit(TemporalUnit::Hour, numeric, prev).unwrap().style, UnitStyle::Numeric);
    }

    #[test]
    fn defaults_follow_the_base_style() {
        let narrow = state(Language::English, DurationStyle::Narrow, None);
        assert_eq!(narrow.units[TemporalUnit::Hour as usize], UnitData { style: UnitStyle::Narrow, display: Display::Auto });
        let digital = state(Language::English, DurationStyle::Digital, None);
        assert_eq!(digital.units[TemporalUnit::Minute as usize], UnitData { style: UnitStyle::TwoDigit, display: Display::Always });
        assert_eq!(digital.units[TemporalUnit::Day as usize], UnitData { style: UnitStyle::Short, display: Display::Auto });
        assert_eq!(digital.units[TemporalUnit::Millisecond as usize], UnitData { style: UnitStyle::Numeric, display: Display::Auto });
    }

    #[test]
    fn decimal_text_keeps_the_sign_on_the_integer_part() {
        let NumericInput::Decimal { negative, digits } = build_decimal_format(TemporalUnit::Second, -5_000_000_007) else {
            panic!("esperava Decimal")
        };
        assert!(negative);
        assert_eq!(digits, "5.000000007");
        let NumericInput::Decimal { negative, digits } = build_decimal_format(TemporalUnit::Millisecond, -500_000) else {
            panic!("esperava Decimal")
        };
        assert!(!negative);
        assert_eq!(digits, "0.500000");
    }
}
