//! `Intl.Locale` sem ICU (`IntlLocale.cpp`, `IntlLocalePrototype.cpp`, `IntlLocaleConstructor.cpp`):
//! a análise e a canonicalização da tag BCP 47 (`intl_locale_data.rs`), as opções
//! (`language`, `script`, `region`, `variants`, `calendar`, `collation`, `firstDayOfWeek`, `hourCycle`,
//! `caseFirst`, `numeric`, `numberingSystem`), `maximize`, `minimize`, `toString` e todos os acessores
//! e métodos de informação do locale do `IntlLocalePrototype` (`getCalendars`, `getCollations`,
//! `getHourCycles`, `getNumberingSystems`, `getTimeZones`, `getTextInfo`, `getWeekInfo`).
//!
//! LOCALES cobertas: qualquer tag bem formada é aceita e canonicalizada; `maximize` e `minimize` usam o
//! `likelySubtags` inteiro medido no bun (`intl_likely_subtags_data.rs`, `scripts/gen-likely-subtags.js`).
//!
//! DIVERGÊNCIAS e LACUNAS, e por quê (sem o CLDR): os sete getters de informação leem as tabelas de
//! `intl_locale_getters_data.rs`, medidas no bun (`scripts/gen-locale-data.js`): calendários, ciclos de
//! hora, fusos e semana por região, para todas as regiões que o bun aceita, usando a região da tag ou a do
//! `maximize`; collations, numeração e direção por par e língua (122 tags medidas); o que está fora da
//! medição cai no padrão (`gregory`, `h23`,
//! `latn`, `ltr`, segunda a sexta, `[]`).

use crate::custom_getter;
use crate::runtime::lookup::{custom_getter_entry, native_entry};
use crate::host_function;
use crate::runtime::host_call::{HostCall, HostResult, Thrown};
use crate::runtime::intl_locale_data::{self, LanguageTag};
use crate::runtime::intl_locale_getters_data;
use crate::runtime::intl_number_format::is_unicode_locale_identifier_type;
use crate::runtime::intl_support::{
    array_of, coerce_options_to_object, construct_instance, new_object, option_bool, option_enum, option_string, put,
    string_array, str_value, to_rust_string, with_instance, IntlClass, IntlEnum, IntlInstance,
};
use crate::runtime::js_global_object::JSGlobalObject;
use crate::runtime::js_object::JSObject;
use crate::runtime::js_value::{js_boolean, js_number, JSValue};
use crate::runtime::property_name::PropertyName;

crate::intl_enum!(HourCycle { H11 => "h11", H12 => "h12", H23 => "h23", H24 => "h24" });
crate::intl_enum!(CaseFirst { Upper => "upper", Lower => "lower", False => "false" });

/// O estado de um `IntlLocale`: a tag canônica já com as opções aplicadas.
pub struct LocaleState {
    tag: LanguageTag,
}

/// A tag (`toString()`) de um `Intl.Locale`; `None` para qualquer outro valor.
pub fn locale_tag_of(value: &JSValue) -> Option<String> {
    let instance = IntlInstance::from_value(value)?;
    Some(instance.state::<LocaleState>()?.tag.canonical())
}

impl LocaleState {
    /// O valor de uma chave `-u-` (`None` se ausente).
    fn keyword(&self, key: &str) -> Option<String> {
        self.tag.unicode_keyword(key)
    }

    /// Troca o valor de uma chave `-u-`; as outras e os atributos ficam.
    fn set_keyword(&mut self, key: &str, value: &str) {
        let mut subtags: Vec<String> = match self.tag.extensions.iter().find(|(singleton, _)| *singleton == 'u') {
            Some((_, subtags)) => subtags.clone(),
            None => Vec::new(),
        };
        // Remove a chave existente e o valor dela.
        if let Some(position) = subtags.iter().position(|subtag| subtag == key) {
            let mut end = position + 1;
            while end < subtags.len() && subtags[end].len() > 2 {
                end += 1;
            }
            subtags.drain(position..end);
        }
        subtags.push(key.to_string());
        if !value.is_empty() {
            subtags.extend(value.split('-').map(str::to_string));
        }
        self.tag.extensions.retain(|(singleton, _)| *singleton != 'u');
        self.tag.extensions.push(('u', subtags));
    }
}

/// `weekdayToString`: o número do dia (`0` e `7` são domingo) vira o nome; o resto passa.
fn weekday_to_string(value: &str) -> &str {
    match value {
        "0" | "7" => "sun",
        "1" => "mon",
        "2" => "tue",
        "3" => "wed",
        "4" => "thu",
        "5" => "fri",
        "6" => "sat",
        other => other,
    }
}

/// `isUnicodeVariantSubtag`: cinco a oito alfanuméricos, ou quatro começando por dígito.
fn is_variant_subtag(subtag: &str) -> bool {
    let alphanumeric = subtag.bytes().all(|byte| byte.is_ascii_alphanumeric());
    alphanumeric && ((5..=8).contains(&subtag.len()) || (subtag.len() == 4 && subtag.as_bytes()[0].is_ascii_digit()))
}

/// A validação da opção `variants`: a lista de variantes em minúsculas, ou `None` se mal formada (vazia,
/// com hífen sobrando, com subtag inválido ou repetida).
fn parse_variants_option(variants: &str) -> Option<Vec<String>> {
    if variants.is_empty() || variants.starts_with('-') || variants.ends_with('-') || variants.contains("--") {
        return None;
    }
    let mut seen: Vec<String> = Vec::new();
    for variant in variants.split('-') {
        if !is_variant_subtag(variant) {
            return None;
        }
        let lower = variant.to_ascii_lowercase();
        if seen.contains(&lower) {
            return None;
        }
        seen.push(lower);
    }
    Some(seen)
}

/// `IntlLocale::initializeLocale`.
fn initialize(global_object: &JSGlobalObject, tag: JSValue, options_value: JSValue) -> Result<LocaleState, Thrown> {
    let tag_text = match locale_tag_of(&tag) {
        Some(text) => text,
        None => to_rust_string(global_object, tag)?,
    };
    let options = coerce_options_to_object(global_object, options_value)?;
    let Some(parsed) = intl_locale_data::parse_language_tag(&tag_text) else {
        return Err(Thrown::range_error("invalid language tag"));
    };
    let mut state = LocaleState { tag: parsed.with_aliases_replaced() };

    if let Some(language) = option_string(global_object, options, "language", &[], "")? {
        let well_formed = language.bytes().all(|byte| byte.is_ascii_alphabetic())
            && ((2..=3).contains(&language.len()) || (5..=8).contains(&language.len()));
        if !well_formed {
            return Err(Thrown::range_error("language is not a well-formed language value"));
        }
        state.tag.language = language.to_ascii_lowercase();
    }
    if let Some(script) = option_string(global_object, options, "script", &[], "")? {
        if script.len() != 4 || !script.bytes().all(|byte| byte.is_ascii_alphabetic()) {
            return Err(Thrown::range_error("script is not a well-formed script value"));
        }
        let lower = script.to_ascii_lowercase();
        state.tag.script = Some(lower[..1].to_ascii_uppercase() + &lower[1..]);
    }
    if let Some(region) = option_string(global_object, options, "region", &[], "")? {
        let well_formed = (region.len() == 2 && region.bytes().all(|byte| byte.is_ascii_alphabetic()))
            || (region.len() == 3 && region.bytes().all(|byte| byte.is_ascii_digit()));
        if !well_formed {
            return Err(Thrown::range_error("region is not a well-formed region value"));
        }
        state.tag.region = Some(region.to_ascii_uppercase());
    }
    if let Some(variants) = option_string(global_object, options, "variants", &[], "")? {
        let Some(list) = parse_variants_option(&variants) else {
            return Err(Thrown::range_error("variants is not a well-formed variants value"));
        };
        state.tag.variants = list;
    }
    state.tag = state.tag.with_aliases_replaced();

    for (key, option, message) in [
        ("ca", "calendar", "calendar is not a well-formed calendar value"),
        ("co", "collation", "collation is not a well-formed collation value"),
    ] {
        if let Some(value) = option_string(global_object, options, option, &[], "")? {
            if !is_unicode_locale_identifier_type(&value) {
                return Err(Thrown::range_error(message));
            }
            state.set_keyword(key, &value.to_ascii_lowercase());
        }
    }
    if let Some(first_day) = option_string(global_object, options, "firstDayOfWeek", &[], "")? {
        let weekday = weekday_to_string(&first_day);
        if !is_unicode_locale_identifier_type(weekday) {
            return Err(Thrown::range_error("firstDayOfWeek is not a well-formed firstDayOfWeek value"));
        }
        state.set_keyword("fw", &weekday.to_ascii_lowercase());
    }
    if let Some(cycle) = option_enum::<HourCycle>(
        global_object,
        options,
        "hourCycle",
        "hourCycle must be \"h11\", \"h12\", \"h23\", or \"h24\"",
    )? {
        state.set_keyword("hc", cycle.as_str());
    }
    if let Some(case_first) = option_enum::<CaseFirst>(
        global_object,
        options,
        "caseFirst",
        "caseFirst must be either \"upper\", \"lower\", or \"false\"",
    )? {
        state.set_keyword("kf", case_first.as_str());
    }
    if let Some(numeric) = option_bool(global_object, options, "numeric")? {
        state.set_keyword("kn", if numeric { "true" } else { "false" });
    }
    if let Some(numbering_system) = option_string(global_object, options, "numberingSystem", &[], "")? {
        if !is_unicode_locale_identifier_type(&numbering_system) {
            return Err(Thrown::range_error("numberingSystem is not a well-formed numbering system value"));
        }
        state.set_keyword("nu", &numbering_system.to_ascii_lowercase());
    }
    Ok(state)
}

fn construct_locale_body(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    let tag = call.argument(0);
    if !tag.is_string() && !tag.is_object() {
        return Err(Thrown::type_error("First argument to Intl.Locale must be a string or an object"));
    }
    construct_instance(global_object, call, |global_object| Ok(Box::new(initialize(global_object, tag, call.argument(1))?)))
}

fn call_locale_body(_global_object: &JSGlobalObject, _call: &HostCall) -> HostResult {
    crate::runtime::collection_support::constructor_cannot_be_called_as_function("Locale")
}

/// `Intl.Locale.prototype.X called on value that's not a Locale`.
fn not_a_locale(member: &str) -> String {
    format!("Intl.Locale.prototype.{member} called on value that's not a Locale")
}

fn to_string_body(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    with_instance::<LocaleState, _>(call.this_value(), &not_a_locale("toString"), |state, _| {
        Ok(str_value(global_object.vm(), &state.tag.canonical()))
    })
}

/// `maximize` e `minimize`: um `Intl.Locale` novo com o mesmo protótipo.
fn derived_locale(
    global_object: &JSGlobalObject,
    call: &HostCall,
    member: &str,
    transform: fn(&LanguageTag) -> LanguageTag,
) -> HostResult {
    with_instance::<LocaleState, _>(call.this_value(), &not_a_locale(member), |state, instance| {
        let tag = transform(&state.tag);
        Ok(IntlInstance::create(global_object.vm(), &instance.structure(), Box::new(LocaleState { tag })).as_value())
    })
}

fn maximize_body(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    derived_locale(global_object, call, "maximize", intl_locale_data::maximize)
}

fn minimize_body(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    derived_locale(global_object, call, "minimize", intl_locale_data::minimize)
}

/// Um acessor de texto opcional (`undefined` quando ausente).
fn optional_text(
    global_object: &JSGlobalObject,
    this_value: JSValue,
    member: &str,
    read: fn(&LocaleState) -> Option<String>,
) -> HostResult {
    with_instance::<LocaleState, _>(this_value, &not_a_locale(member), |state, _| {
        Ok(read(state).map_or_else(JSValue::undefined, |text| str_value(global_object.vm(), &text)))
    })
}

fn base_name_body(global_object: &JSGlobalObject, this_value: JSValue, _name: &PropertyName) -> HostResult {
    optional_text(global_object, this_value, "baseName", |state| Some(state.tag.base_name()))
}

fn language_body(global_object: &JSGlobalObject, this_value: JSValue, _name: &PropertyName) -> HostResult {
    optional_text(global_object, this_value, "language", |state| Some(state.tag.language.clone()))
}

fn script_body(global_object: &JSGlobalObject, this_value: JSValue, _name: &PropertyName) -> HostResult {
    optional_text(global_object, this_value, "script", |state| state.tag.script.clone())
}

fn region_body(global_object: &JSGlobalObject, this_value: JSValue, _name: &PropertyName) -> HostResult {
    optional_text(global_object, this_value, "region", |state| state.tag.region.clone())
}

/// `variants`: os subtags de variante em minúsculas e em ordem, `undefined` sem nenhum.
fn variants_body(global_object: &JSGlobalObject, this_value: JSValue, _name: &PropertyName) -> HostResult {
    optional_text(global_object, this_value, "variants", |state| {
        let mut variants = state.tag.variants.clone();
        variants.sort();
        (!variants.is_empty()).then(|| variants.join("-"))
    })
}

fn calendar_body(global_object: &JSGlobalObject, this_value: JSValue, _name: &PropertyName) -> HostResult {
    optional_text(global_object, this_value, "calendar", |state| state.keyword("ca"))
}

fn collation_body(global_object: &JSGlobalObject, this_value: JSValue, _name: &PropertyName) -> HostResult {
    optional_text(global_object, this_value, "collation", |state| state.keyword("co"))
}

fn first_day_of_week_body(global_object: &JSGlobalObject, this_value: JSValue, _name: &PropertyName) -> HostResult {
    optional_text(global_object, this_value, "firstDayOfWeek", |state| state.keyword("fw"))
}

fn hour_cycle_body(global_object: &JSGlobalObject, this_value: JSValue, _name: &PropertyName) -> HostResult {
    optional_text(global_object, this_value, "hourCycle", |state| state.keyword("hc"))
}

fn case_first_body(global_object: &JSGlobalObject, this_value: JSValue, _name: &PropertyName) -> HostResult {
    optional_text(global_object, this_value, "caseFirst", |state| state.keyword("kf"))
}

fn numbering_system_body(global_object: &JSGlobalObject, this_value: JSValue, _name: &PropertyName) -> HostResult {
    optional_text(global_object, this_value, "numberingSystem", |state| state.keyword("nu"))
}

/// `numeric`: `true` quando `kn` está presente e vale `true` ou vazio.
fn numeric_body(_global_object: &JSGlobalObject, this_value: JSValue, _name: &PropertyName) -> HostResult {
    with_instance::<LocaleState, _>(this_value, &not_a_locale("numeric"), |state, _| {
        Ok(js_boolean(state.keyword("kn").is_some_and(|value| value.is_empty() || value == "true")))
    })
}

// ---------------------------------------------------------------------------------------------
// Informação do locale (`IntlLocale::calendars`, `collations`, `hourCycles`...)
// ---------------------------------------------------------------------------------------------


/// A região dos dados por região (`getCalendars`, `getHourCycles`, `getWeekInfo`): a da tag ou a que o
/// `maximize` acrescentaria (`zh-Hant` é `TW`, `es-419` é `419`). Sem região conhecida, a tabela usa o padrão.
fn data_region(tag: &LanguageTag) -> String {
    intl_locale_data::region_or_likely(tag).unwrap_or_default()
}

/// `getCalendars`: a tabela por região medida no bun (`intl_locale_getters_data`).
fn get_calendars_body(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    with_instance::<LocaleState, _>(call.this_value(), &not_a_locale("getCalendars"), |state, _| {
        let calendars: Vec<String> = match state.keyword("ca") {
            Some(calendar) if !calendar.is_empty() => vec![calendar],
            _ => string_list(intl_locale_getters_data::calendars(&data_region(&state.tag))),
        };
        Ok(string_array(global_object, &calendars))
    })
}

fn string_list(list: &[&str]) -> Vec<String> {
    list.iter().map(|item| (*item).to_string()).collect()
}

fn get_collations_body(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    with_instance::<LocaleState, _>(call.this_value(), &not_a_locale("getCollations"), |state, _| {
        let collations: Vec<String> = match state.keyword("co") {
            Some(collation) if !collation.is_empty() => vec![collation],
            _ => string_list(intl_locale_getters_data::collations(&state.tag.language, state.tag.region.as_deref())),
        };
        Ok(string_array(global_object, &collations))
    })
}

fn get_hour_cycles_body(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    with_instance::<LocaleState, _>(call.this_value(), &not_a_locale("getHourCycles"), |state, _| {
        let cycles = match state.keyword("hc") {
            Some(cycle) if !cycle.is_empty() => vec![cycle],
            _ => string_list(intl_locale_getters_data::hour_cycles(&state.tag.language, &data_region(&state.tag))),
        };
        Ok(string_array(global_object, &cycles))
    })
}

fn get_numbering_systems_body(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    with_instance::<LocaleState, _>(call.this_value(), &not_a_locale("getNumberingSystems"), |state, _| {
        let numbering_systems = match state.keyword("nu") {
            Some(numbering_system) if !numbering_system.is_empty() => vec![numbering_system],
            _ => string_list(intl_locale_getters_data::numbering_systems(&state.tag.language, state.tag.region.as_deref())),
        };
        Ok(string_array(global_object, &numbering_systems))
    })
}

/// `getTimeZones`: `undefined` sem região na tag; `[]` para uma região fora da tabela medida.
fn get_time_zones_body(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    with_instance::<LocaleState, _>(call.this_value(), &not_a_locale("getTimeZones"), |state, _| {
        let Some(region) = &state.tag.region else {
            return Ok(JSValue::undefined());
        };
        Ok(string_array(global_object, &string_list(intl_locale_getters_data::time_zones(region))))
    })
}

/// A direção do texto (`uloc_getCharacterOrientation`): a tabela medida quando a tag não traz escrita; com
/// escrita, pela escrita.
fn text_direction(tag: &LanguageTag) -> &'static str {
    const RIGHT_TO_LEFT_SCRIPTS: [&str; 9] = ["Arab", "Hebr", "Thaa", "Syrc", "Nkoo", "Adlm", "Rohg", "Mand", "Samr"];
    match &tag.script {
        None => intl_locale_getters_data::text_direction(&tag.language),
        Some(script) if RIGHT_TO_LEFT_SCRIPTS.contains(&script.as_str()) => "rtl",
        Some(_) => "ltr",
    }
}

fn get_text_info_body(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    with_instance::<LocaleState, _>(call.this_value(), &not_a_locale("getTextInfo"), |state, _| {
        let info = new_object(global_object);
        put(global_object, &info, "direction", str_value(global_object.vm(), text_direction(&state.tag)));
        Ok(info.as_value())
    })
}

/// O número do dia (1 segunda, 7 domingo) do nome BCP 47 de `fw`.
fn weekday_number(name: &str) -> Option<u32> {
    Some(match name {
        "mon" => 1,
        "tue" => 2,
        "wed" => 3,
        "thu" => 4,
        "fri" => 5,
        "sat" => 6,
        "sun" => 7,
        _ => return None,
    })
}

/// `getWeekInfo`: `{ firstDay, weekend }`, sem `minimalDays` (o `IntlLocale::weekInfo` do JSC não o tem).
fn get_week_info_body(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    with_instance::<LocaleState, _>(call.this_value(), &not_a_locale("getWeekInfo"), |state, _| {
        let (region_first_day, region_weekend) = intl_locale_getters_data::week_info(&data_region(&state.tag));
        let first_day = state.keyword("fw").and_then(|name| weekday_number(&name)).unwrap_or(u32::from(region_first_day));
        let weekend: Vec<JSValue> = region_weekend.iter().map(|day| js_number(f64::from(*day))).collect();
        let info = new_object(global_object);
        put(global_object, &info, "firstDay", js_number(f64::from(first_day)));
        put(global_object, &info, "weekend", array_of(global_object, &weekend));
        Ok(info.as_value())
    })
}

host_function!(call_locale, call_locale_body);
host_function!(construct_locale, construct_locale_body);
host_function!(locale_proto_to_string, to_string_body);
host_function!(locale_proto_maximize, maximize_body);
host_function!(locale_proto_minimize, minimize_body);
host_function!(locale_proto_get_calendars, get_calendars_body);
host_function!(locale_proto_get_collations, get_collations_body);
host_function!(locale_proto_get_hour_cycles, get_hour_cycles_body);
host_function!(locale_proto_get_numbering_systems, get_numbering_systems_body);
host_function!(locale_proto_get_time_zones, get_time_zones_body);
host_function!(locale_proto_get_text_info, get_text_info_body);
host_function!(locale_proto_get_week_info, get_week_info_body);
custom_getter!(locale_proto_base_name, base_name_body);
custom_getter!(locale_proto_calendar, calendar_body);
custom_getter!(locale_proto_case_first, case_first_body);
custom_getter!(locale_proto_collation, collation_body);
custom_getter!(locale_proto_first_day_of_week, first_day_of_week_body);
custom_getter!(locale_proto_hour_cycle, hour_cycle_body);
custom_getter!(locale_proto_numeric, numeric_body);
custom_getter!(locale_proto_numbering_system, numbering_system_body);
custom_getter!(locale_proto_language, language_body);
custom_getter!(locale_proto_script, script_body);
custom_getter!(locale_proto_region, region_body);
custom_getter!(locale_proto_variants, variants_body);

crate::intl_prototype_s_info!(
    LOCALE_PROTOTYPE_S_INFO,
    "Intl.Locale",
    [
        native_entry("maximize", locale_proto_maximize, 0),
        native_entry("minimize", locale_proto_minimize, 0),
        native_entry("toString", locale_proto_to_string, 0),
        native_entry("getCalendars", locale_proto_get_calendars, 0),
        native_entry("getCollations", locale_proto_get_collations, 0),
        native_entry("getHourCycles", locale_proto_get_hour_cycles, 0),
        native_entry("getNumberingSystems", locale_proto_get_numbering_systems, 0),
        native_entry("getTimeZones", locale_proto_get_time_zones, 0),
        native_entry("getTextInfo", locale_proto_get_text_info, 0),
        native_entry("getWeekInfo", locale_proto_get_week_info, 0),
        custom_getter_entry("baseName", locale_proto_base_name),
        custom_getter_entry("calendar", locale_proto_calendar),
        custom_getter_entry("caseFirst", locale_proto_case_first),
        custom_getter_entry("collation", locale_proto_collation),
        custom_getter_entry("firstDayOfWeek", locale_proto_first_day_of_week),
        custom_getter_entry("hourCycle", locale_proto_hour_cycle),
        custom_getter_entry("numeric", locale_proto_numeric),
        custom_getter_entry("numberingSystem", locale_proto_numbering_system),
        custom_getter_entry("language", locale_proto_language),
        custom_getter_entry("script", locale_proto_script),
        custom_getter_entry("region", locale_proto_region),
        custom_getter_entry("variants", locale_proto_variants),
    ]
);

/// `IntlLocaleConstructor` (`length` 1) e `IntlLocalePrototype`, na ordem da tabela do `.lut`.
pub fn install_locale(global_object: &JSGlobalObject, intl: &JSObject) {
    let class = IntlClass {
        name: "Locale",
        length: 1,
        has_supported_locales_of: false,
        call: call_locale,
        construct: construct_locale,
    };
    class.install_with_table(global_object, intl, &LOCALE_PROTOTYPE_S_INFO);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn state(tag: &str) -> LocaleState {
        LocaleState { tag: intl_locale_data::parse_language_tag(tag).unwrap().with_aliases_replaced() }
    }

    #[test]
    fn keywords_are_set_and_replaced() {
        let mut locale = state("en-US");
        locale.set_keyword("hc", "h23");
        locale.set_keyword("kn", "true");
        assert_eq!(locale.tag.canonical(), "en-US-u-hc-h23-kn");
        locale.set_keyword("hc", "h12");
        assert_eq!(locale.tag.canonical(), "en-US-u-hc-h12-kn");
        assert_eq!(locale.keyword("kn").as_deref(), Some(""));
    }

    #[test]
    fn existing_extension_keywords_survive() {
        let mut locale = state("de-u-co-phonebk");
        locale.set_keyword("nu", "latn");
        assert_eq!(locale.tag.canonical(), "de-u-co-phonebk-nu-latn");
    }

    #[test]
    fn weekday_option_maps_numbers_to_names() {
        assert_eq!(weekday_to_string("0"), "sun");
        assert_eq!(weekday_to_string("1"), "mon");
        assert_eq!(weekday_to_string("7"), "sun");
        assert_eq!(weekday_to_string("mon"), "mon");
        assert_eq!(weekday_to_string("8"), "8");
        assert_eq!(weekday_number("sun"), Some(7));
        assert_eq!(weekday_number("xyz"), None);
    }

    #[test]
    fn variants_option_is_validated_like_the_c_plus_plus() {
        assert_eq!(parse_variants_option("posix"), Some(vec!["posix".to_string()]));
        assert_eq!(parse_variants_option("1996-Rozaj"), Some(vec!["1996".to_string(), "rozaj".to_string()]));
        for malformed in ["", "-posix", "posix-", "posix--rozaj", "abc", "posix-POSIX", "toolongsub"] {
            assert_eq!(parse_variants_option(malformed), None, "{malformed}");
        }
    }

    #[test]
    fn variants_replace_and_sort() {
        let mut locale = state("sl-rozaj-biske");
        assert_eq!(locale.tag.canonical(), "sl-biske-rozaj");
        locale.tag.variants = parse_variants_option("nedis").unwrap();
        assert_eq!(locale.tag.canonical(), "sl-nedis");
    }

    #[test]
    fn week_data_follows_the_region() {
        assert_eq!(intl_locale_getters_data::week_info("US").0, 7);
        assert_eq!(intl_locale_getters_data::week_info("BR").0, 7);
        assert_eq!(intl_locale_getters_data::week_info("GB").0, 1);
        assert_eq!(intl_locale_getters_data::week_info("EG").1, [5, 6]);
        // Sem região na tag, vale a do `maximize` (`pt` é `pt-BR`, `zh-Hant` é `zh-Hant-TW`).
        assert_eq!(intl_locale_getters_data::week_info(&data_region(&state("pt").tag)).0, 7);
        assert_eq!(intl_locale_getters_data::week_info(&data_region(&state("zh-Hant").tag)).0, 7);
    }

    #[test]
    fn calendars_follow_the_region() {
        assert_eq!(intl_locale_getters_data::calendars("US"), ["gregory"]);
        assert_eq!(intl_locale_getters_data::calendars("TH")[0], "buddhist");
        assert_eq!(intl_locale_getters_data::calendars("SA"), ["gregory", "islamic-umalqura", "islamic", "islamic-rgsa"]);
        assert_eq!(
            intl_locale_getters_data::calendars(&data_region(&state("zh-Hant").tag)),
            ["gregory", "roc", "chinese"]
        );
        // Região fora da tabela cai no padrão.
        assert_eq!(intl_locale_getters_data::calendars("ZZ"), ["gregory"]);
    }

    #[test]
    fn hour_cycles_follow_the_maximized_region() {
        assert_eq!(intl_locale_getters_data::hour_cycles("es", &data_region(&state("es-419").tag)), ["h12"]);
        assert_eq!(intl_locale_getters_data::hour_cycles("en", "CA"), ["h12"]);
        assert_eq!(intl_locale_getters_data::hour_cycles("fr", "CA"), ["h23"]);
    }

    #[test]
    fn aliases_are_replaced_in_the_constructor() {
        assert_eq!(state("iw").tag.language, "he");
        assert_eq!(state("in").tag.canonical(), "id");
    }

    #[test]
    fn time_zones_follow_the_region() {
        assert_eq!(intl_locale_getters_data::time_zones("ME"), ["Europe/Podgorica"]);
        assert_eq!(intl_locale_getters_data::time_zones("419"), [] as [&str; 0]);
    }

    #[test]
    fn text_direction_follows_the_script() {
        assert_eq!(text_direction(&state("ar").tag), "rtl");
        assert_eq!(text_direction(&state("he-IL").tag), "rtl");
        assert_eq!(text_direction(&state("en").tag), "ltr");
        assert_eq!(text_direction(&state("pa-Arab").tag), "rtl");
        assert_eq!(text_direction(&state("az-Cyrl").tag), "ltr");
        assert_eq!(text_direction(&state("und-Arab").tag), "rtl");
    }

    #[test]
    fn time_zone_table_is_sorted_by_code_point() {
        for region in ["BR", "PT", "AR", "US"] {
            let zones = intl_locale_getters_data::time_zones(region);
            let mut sorted = zones.to_vec();
            sorted.sort();
            assert_eq!(zones.to_vec(), sorted, "{region}");
        }
    }
}
