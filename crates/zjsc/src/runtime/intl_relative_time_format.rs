//! `Intl.RelativeTimeFormat` sem ICU (`IntlRelativeTimeFormat.cpp`,
//! `IntlRelativeTimeFormatPrototype.cpp`, `IntlRelativeTimeFormatConstructor.cpp`): `format` e
//! `formatToParts` com `numeric: "always"` e `"auto"` e os estilos `long`, `short` e `narrow`.
//!
//! LOCALES cobertas: en, pt, es, fr, de, it, ja, ru, ar e hi, todas pela tabela gerada do bun em
//! `intl_relative_time_data.rs` (padrão por categoria de plural do `icu_plural`, passado e futuro, três
//! estilos, e os textos de `numeric: "auto"`). O número sai do `icu_number` (icu4x), no locale resolvido e
//! com o `numberingSystem` honrado. Não há tabela escrita à mão: o golden `reltime_bun.tsv` confere tudo.

use crate::host_function;
use crate::runtime::lookup::{native_entry};
use crate::runtime::default_number_format::{format_parts, NumberSettings, NumericInput};
use crate::runtime::icu_plural;
use crate::runtime::intl_relative_time_data as relative_time_data;
use crate::runtime::host_call::{HostCall, HostResult, Thrown};
use crate::runtime::intl_locale_data::Language;
use crate::runtime::intl_support::{
    array_of, coerce_options_to_object, construct_instance, new_object, option_enum, option_string, put, read_locale_matcher, resolve_locale_from, str_value, to_number_checked, to_rust_string, with_instance, IntlClass,
    IntlEnum,
};
use crate::runtime::icu_number;
use crate::runtime::intl_number_format::is_unicode_locale_identifier_type;
use crate::runtime::js_global_object::JSGlobalObject;
use crate::runtime::js_object::JSObject;
use crate::runtime::js_value::JSValue;

crate::intl_enum!(RelativeStyle { Long => "long", Short => "short", Narrow => "narrow" });
crate::intl_enum!(Numeric { Always => "always", Auto => "auto" });

/// A unidade de tempo.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum TimeUnit {
    Second,
    Minute,
    Hour,
    Day,
    Week,
    Month,
    Quarter,
    Year,
}

impl TimeUnit {
    /// `second`/`seconds`, `minute`/`minutes`...
    fn parse(text: &str) -> Option<TimeUnit> {
        Some(match text.strip_suffix('s').unwrap_or(text) {
            "second" => TimeUnit::Second,
            "minute" => TimeUnit::Minute,
            "hour" => TimeUnit::Hour,
            "day" => TimeUnit::Day,
            "week" => TimeUnit::Week,
            "month" => TimeUnit::Month,
            "quarter" => TimeUnit::Quarter,
            "year" => TimeUnit::Year,
            _ => return None,
        })
    }

    /// O nome singular do campo `unit` das partes.
    fn name(self) -> &'static str {
        match self {
            TimeUnit::Second => "second",
            TimeUnit::Minute => "minute",
            TimeUnit::Hour => "hour",
            TimeUnit::Day => "day",
            TimeUnit::Week => "week",
            TimeUnit::Month => "month",
            TimeUnit::Quarter => "quarter",
            TimeUnit::Year => "year",
        }
    }
}

/// O estado de um `IntlRelativeTimeFormat`.
struct RelativeTimeFormatState {
    locale: String,
    /// A tag base do locale resolvido (sem `-u-`): de onde o `icu_number` lê os símbolos do número.
    base_locale: String,
    /// O sistema numérico resolvido (`latn`, `arab`, `deva`...).
    numbering_system: String,
    language: Language,
    style: RelativeStyle,
    numeric: Numeric,
}

/// A frase em partes: o tipo, o texto e a unidade (nas partes do número).
type RelativePart = (String, String, Option<&'static str>);

/// A língua de `intl_relative_time_data` do locale resolvido; o que a tabela não tem cai em `en`, a
/// língua padrão do resolvedor de locales.
fn data_language(locale: &str) -> &'static str {
    let mut subtags = locale.split('-');
    let primary = subtags.next().unwrap_or("");
    // As variantes regionais medidas (en-GB, pt-PT, es-MX, fr-CA, zh-TW) vêm antes da língua genérica.
    let with_region = subtags.next().map(|region| format!("{primary}-{region}"));
    let table = relative_time_data::LANGUAGES;
    let regional = table.iter().copied().find(|language| with_region.as_deref() == Some(*language));
    regional.or_else(|| table.iter().copied().find(|language| *language == primary)).unwrap_or("en")
}

/// O índice do estilo nas tabelas geradas.
fn style_index(style: RelativeStyle) -> u8 {
    match style {
        RelativeStyle::Long => 0,
        RelativeStyle::Short => 1,
        RelativeStyle::Narrow => 2,
    }
}

/// A posição da categoria de plural nos padrões gerados (`zero`, `one`, `two`, `few`, `many`, `other`).
fn category_index(category: &str) -> usize {
    ["zero", "one", "two", "few", "many"].iter().position(|name| *name == category).unwrap_or(5)
}

/// O texto de `numeric: "auto"` das línguas da tabela gerada.
fn data_auto_text(language: &str, style: RelativeStyle, unit: TimeUnit, value: i64) -> Option<&'static str> {
    relative_time_data::AUTO
        .iter()
        .find(|entry| {
            entry.lang == language
                && entry.style == style_index(style)
                && entry.unit == unit as u8
                && i64::from(entry.value) == value
        })
        .map(|entry| entry.text)
}

/// As partes da frase de uma língua da tabela gerada: o padrão do plural, com o número no lugar de `{0}`.
fn data_parts(
    state: &RelativeTimeFormatState,
    language: &str,
    unit: TimeUnit,
    past: bool,
    number: Vec<(String, String)>,
    category: &str,
) -> Vec<RelativePart> {
    let entry = relative_time_data::PATTERNS
        .iter()
        .find(|entry| entry.lang == language && entry.style == style_index(state.style) && entry.unit == unit as u8);
    let patterns = entry.map(|entry| if past { &entry.past } else { &entry.future });
    let pattern = patterns
        .map(|list| if list[category_index(category)].is_empty() { list[5] } else { list[category_index(category)] })
        .unwrap_or("{0}");
    let mut parts: Vec<RelativePart> = Vec::new();
    let literal = |text: &str, parts: &mut Vec<RelativePart>| {
        if !text.is_empty() {
            parts.push(("literal".to_string(), text.to_string(), None));
        }
    };
    match pattern.split_once("{0}") {
        Some((before, after)) => {
            literal(before, &mut parts);
            parts.extend(number.into_iter().map(|(kind, text)| (kind, text, Some(unit.name()))));
            literal(after, &mut parts);
        }
        None => literal(pattern, &mut parts),
    }
    parts
}

fn relative_parts(state: &RelativeTimeFormatState, value: f64, unit: TimeUnit) -> Vec<RelativePart> {
    let data_language = data_language(&state.locale);
    if state.numeric == Numeric::Auto && value.fract() == 0.0 && value.abs() <= 2.0 {
        if let Some(text) = data_auto_text(data_language, state.style, unit, value as i64) {
            return vec![("literal".to_string(), text.to_string(), None)];
        }
    }
    // O sinal de `-0` conta como passado.
    let past = value.is_sign_negative();
    // O `unumf` do C++: padrão do `Intl.NumberFormat` (até 3 fração, agrupamento do locale), no locale
    // resolvido e com o sistema numérico honrado.
    let mut settings = NumberSettings::defaults(state.language);
    settings.locale = state.base_locale.clone();
    settings.numbering_system = state.numbering_system.clone();
    let number = format_parts(&settings, &NumericInput::Double(value.abs()));
    let (integer, fraction): (String, String) = (
        number.iter().filter(|part| part.0 == "integer").map(|part| part.1.as_str()).collect(),
        number.iter().filter(|part| part.0 == "fraction").map(|part| part.1.as_str()).collect(),
    );
    let category = icu_plural::select(&state.locale, false, &integer, &fraction, 0).unwrap_or("other");
    data_parts(state, data_language, unit, past, number, category)
}

/// `IntlRelativeTimeFormat::initializeRelativeTimeFormat`.
fn initialize(
    global_object: &JSGlobalObject,
    locales: JSValue,
    options_value: JSValue,
) -> Result<RelativeTimeFormatState, Thrown> {
    let resolved = resolve_locale_from(global_object, locales, &["nu"])?;
    let options = coerce_options_to_object(global_object, options_value)?;
    read_locale_matcher(global_object, options)?;
    let numbering_option = option_string(global_object, options, "numberingSystem", &[], "")?;
    if let Some(numbering_system) = &numbering_option {
        if !is_unicode_locale_identifier_type(numbering_system) {
            return Err(Thrown::range_error("numberingSystem is not a well-formed numbering system value"));
        }
    }
    // Como o `NumberFormat`: a opção vence o `-u-nu-` da tag, e o que o icu4x não tem em dígitos é ignorado.
    let numbering_option = numbering_option.map(|text| text.to_ascii_lowercase());
    let base_locale = resolved.locale.clone();
    let option_honored = numbering_option.as_deref().filter(|name| icu_number::numbering_system_honored(&base_locale, name));
    let extension_honored = resolved.keyword("nu").filter(|name| icu_number::numbering_system_honored(&base_locale, name));
    let (numbering_system, keep_extension) = match (option_honored, extension_honored) {
        (Some(option), extension) => (option.to_string(), extension == Some(option)),
        (None, Some(extension)) => (extension.to_string(), true),
        (None, None) => (icu_number::default_numbering_system(&base_locale).to_string(), false),
    };
    let honored: Vec<(&str, &str)> = if keep_extension { vec![("nu", numbering_system.as_str())] } else { Vec::new() };
    let locale = resolved.tag_with(&honored);
    let style = option_enum::<RelativeStyle>(
        global_object,
        options,
        "style",
        "style must be either \"long\", \"short\", or \"narrow\"",
    )?
    .unwrap_or(RelativeStyle::Long);
    let numeric =
        option_enum::<Numeric>(global_object, options, "numeric", "numeric must be either \"always\" or \"auto\"")?
            .unwrap_or(Numeric::Always);
    Ok(RelativeTimeFormatState { locale, base_locale, numbering_system, language: resolved.language, style, numeric })
}

/// A leitura de `value` e `unit` de `format` e `formatToParts`.
fn read_arguments(global_object: &JSGlobalObject, call: &HostCall) -> Result<(f64, TimeUnit), Thrown> {
    let value = to_number_checked(global_object, call.argument(0))?;
    let unit_text = to_rust_string(global_object, call.argument(1))?;
    if !value.is_finite() {
        return Err(Thrown::range_error("number argument must be finite"));
    }
    let unit = TimeUnit::parse(&unit_text).ok_or_else(|| Thrown::range_error("unit argument is not a recognized unit type"))?;
    Ok((value, unit))
}

fn construct_relative_time_format_body(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    construct_instance(global_object, call, |global_object| {
        Ok(Box::new(initialize(global_object, call.argument(0), call.argument(1))?))
    })
}

fn call_relative_time_format_body(_global_object: &JSGlobalObject, _call: &HostCall) -> HostResult {
    crate::runtime::collection_support::constructor_cannot_be_called_as_function("RelativeTimeFormat")
}

fn format_body(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    with_instance::<RelativeTimeFormatState, _>(
        call.this_value(),
        "Intl.RelativeTimeFormat.prototype.format called on value that's not a RelativeTimeFormat",
        |state, _| {
            let (value, unit) = read_arguments(global_object, call)?;
            let text: String = relative_parts(state, value, unit).into_iter().map(|(_, text, _)| text).collect();
            Ok(str_value(global_object.vm(), &text))
        },
    )
}

fn format_to_parts_body(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    with_instance::<RelativeTimeFormatState, _>(
        call.this_value(),
        "Intl.RelativeTimeFormat.prototype.formatToParts called on value that's not a RelativeTimeFormat",
        |state, _| {
            let (value, unit) = read_arguments(global_object, call)?;
            let vm = global_object.vm();
            let objects: Vec<JSValue> = relative_parts(state, value, unit)
                .into_iter()
                .map(|(kind, text, unit)| {
                    let part = new_object(global_object);
                    put(global_object, &part, "type", str_value(vm, &kind));
                    put(global_object, &part, "value", str_value(vm, &text));
                    if let Some(unit) = unit {
                        put(global_object, &part, "unit", str_value(vm, unit));
                    }
                    part.as_value()
                })
                .collect();
            Ok(array_of(global_object, &objects))
        },
    )
}

fn resolved_options_body(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    with_instance::<RelativeTimeFormatState, _>(
        call.this_value(),
        "Intl.RelativeTimeFormat.prototype.resolvedOptions called on value that's not a RelativeTimeFormat",
        |state, _| {
            let vm = global_object.vm();
            let options = new_object(global_object);
            put(global_object, &options, "locale", str_value(vm, &state.locale));
            put(global_object, &options, "style", str_value(vm, state.style.as_str()));
            put(global_object, &options, "numeric", str_value(vm, state.numeric.as_str()));
            put(global_object, &options, "numberingSystem", str_value(vm, &state.numbering_system));
            Ok(options.as_value())
        },
    )
}

host_function!(call_relative_time_format, call_relative_time_format_body);
host_function!(construct_relative_time_format, construct_relative_time_format_body);
host_function!(relative_time_format_proto_format, format_body);
host_function!(relative_time_format_proto_format_to_parts, format_to_parts_body);
host_function!(relative_time_format_proto_resolved_options, resolved_options_body);

crate::intl_prototype_s_info!(
    RELATIVE_TIME_FORMAT_PROTOTYPE_S_INFO,
    "Intl.RelativeTimeFormat",
    [
        native_entry("format", relative_time_format_proto_format, 2),
        native_entry("formatToParts", relative_time_format_proto_format_to_parts, 2),
        native_entry("resolvedOptions", relative_time_format_proto_resolved_options, 0),
    ]
);

/// `IntlRelativeTimeFormatConstructor` e `IntlRelativeTimeFormatPrototype`.
pub fn install_relative_time_format(global_object: &JSGlobalObject, intl: &JSObject) {
    let class = IntlClass {
        name: "RelativeTimeFormat",
        length: 0,
        has_supported_locales_of: true,
        call: call_relative_time_format,
        construct: construct_relative_time_format,
    };
    class.install_with_table(global_object, intl, &RELATIVE_TIME_FORMAT_PROTOTYPE_S_INFO);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn text(language: Language, style: RelativeStyle, numeric: Numeric, value: f64, unit: TimeUnit) -> String {
        let base_locale = match language {
            Language::English => "en",
            Language::Portuguese => "pt",
        };
        let state = RelativeTimeFormatState {
            locale: base_locale.to_string(),
            base_locale: base_locale.to_string(),
            numbering_system: String::new(),
            language,
            style,
            numeric,
        };
        relative_parts(&state, value, unit).into_iter().map(|(_, text, _)| text).collect()
    }

    #[test]
    fn english_always() {
        let english = |value, unit| text(Language::English, RelativeStyle::Long, Numeric::Always, value, unit);
        assert_eq!(english(3.0, TimeUnit::Day), "in 3 days");
        assert_eq!(english(1.0, TimeUnit::Day), "in 1 day");
        assert_eq!(english(-3.0, TimeUnit::Day), "3 days ago");
        assert_eq!(english(-0.0, TimeUnit::Day), "0 days ago");
        assert_eq!(english(1500.0, TimeUnit::Second), "in 1,500 seconds");
    }

    #[test]
    fn english_auto() {
        let english = |value, unit| text(Language::English, RelativeStyle::Long, Numeric::Auto, value, unit);
        assert_eq!(english(-1.0, TimeUnit::Day), "yesterday");
        assert_eq!(english(0.0, TimeUnit::Year), "this year");
        assert_eq!(english(2.0, TimeUnit::Day), "in 2 days");
    }

    #[test]
    fn portuguese() {
        let portuguese = |numeric, value, unit| text(Language::Portuguese, RelativeStyle::Long, numeric, value, unit);
        assert_eq!(portuguese(Numeric::Always, 3.0, TimeUnit::Day), "em 3 dias");
        assert_eq!(portuguese(Numeric::Always, -1.0, TimeUnit::Month), "h\u{e1} 1 m\u{ea}s");
        assert_eq!(portuguese(Numeric::Auto, 1.0, TimeUnit::Day), "amanh\u{e3}");
    }
}
