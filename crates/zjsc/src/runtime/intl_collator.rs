//! `Intl.Collator` (`IntlCollator.cpp`, `IntlCollatorPrototype.cpp`, `IntlCollatorConstructor.cpp`) e
//! `String.prototype.localeCompare`: a comparação é do `icu_collator` (CLDR compilado, o mesmo UCA do
//! ICU do bun) para todo locale, sem tabela de pesos própria.
//!
//! LOCALES: a resolução usa o conjunto do colador (`en-GB` resolve `en`, `fr-CA` e `de-AT` ficam como
//! pedidos; sem cobertura, `en-US`) e `intl_collator_tailoring.rs`. O ajuste de cada idioma (`sv`, `tr`,
//! `pl`...) e as colações extras (`es-u-co-trad`, `de-u-co-phonebk`, `zh` pinyin, `stroke`, `zhuyin`,
//! `unihan`) vêm do CLDR pela extensão `-u-co-` da tag passada ao `icu_collator`; a colação resolvida
//! entra nela mesmo quando veio da opção `collation`.
//!
//! OPÇÕES: `sensitivity` vira `Strength` e `CaseLevel`, `numeric` e `caseFirst` vão pelas preferências
//! (`-u-kn`, `-u-kf`) e `ignorePunctuation` é `AlternateHandling::Shifted` (o `UCOL_SHIFTED` do ICU).
//!
//! DIVERGÊNCIAS e LACUNAS, e por quê:
//!
//! - Os valores de `collation` (`emoji` e `eor` são os únicos que `en` e `pt` aceitam, como no ICU; os outros
//!   caem em `default`) e `usage: "search"` não mudam a ordem. `ResolveLocale` segue o C++: a opção só
//!   desloca a extensão `-u-co`, `-u-kf` ou `-u-kn` da tag quando difere dela.

use std::sync::Arc;

use icu_collator::options::{AlternateHandling, CaseLevel, CollatorOptions, Strength};
use icu_collator::preferences::{CollationCaseFirst, CollationNumericOrdering};
use icu_collator::{Collator, CollatorBorrowed, CollatorPreferences};
use icu_locale_core::Locale;

use crate::custom_getter;
use crate::host_function;
use crate::runtime::class_info::ClassInfo;
use crate::runtime::host_call::{HostCall, HostResult, Thrown};
use crate::runtime::js_object::JS_NON_FINAL_OBJECT_S_INFO;
use crate::runtime::lookup::{HashTable, HashTableValue};
use crate::runtime::lookup::{custom_getter_entry, native_entry};
use crate::runtime::property_name::PropertyName;
use crate::runtime::intl_collator_tailoring::{collator_locale, extra_collations};
use crate::runtime::intl_locale_data::{parse_language_tag, resolve_key, resolve_locale, Language};
use crate::runtime::intl_number_format::is_unicode_locale_identifier_type;
use crate::runtime::intl_support::{
    bound_function, call_instance, canonicalize_locale_list, coerce_options_to_object, construct_instance, new_object, option_bool,
    option_enum, option_string, put, read_locale_matcher, str_value,
    to_wtf_checked, with_instance, IntlClass, IntlEnum,
};
use crate::runtime::js_global_object::JSGlobalObject;
use crate::runtime::js_object::JSObject;
use crate::runtime::js_value::{js_boolean, js_number, JSValue};
use crate::wtf::text::wtf_string::String as WtfString;

crate::intl_enum!(Usage { Sort => "sort", Search => "search" });
crate::intl_enum!(Sensitivity { Base => "base", Accent => "accent", Case => "case", Variant => "variant" });
crate::intl_enum!(CaseFirst { Upper => "upper", Lower => "lower", False => "false" });

/// O que a comparação lê.
#[derive(Clone)]
pub struct CollatorSettings {
    pub sensitivity: Sensitivity,
    pub case_first: CaseFirst,
    pub numeric: bool,
    pub ignore_punctuation: bool,
    /// O colador CLDR do `icu_collator` (raiz mais o ajuste do locale, inclusive `-u-co-`): decide a
    /// comparação inteira.
    pub icu: Arc<CollatorBorrowed<'static>>,
}

impl CollatorSettings {
    /// As configurações com o colador do `icu_collator` para a tag resolvida (com `-u-co-` honrada). Se os
    /// dados não têm a colação pedida, cai para o locale sem extensões e, por fim, para a colação raiz.
    fn new(tag: &str, sensitivity: Sensitivity, case_first: CaseFirst, numeric: bool, ignore_punctuation: bool) -> CollatorSettings {
        let build = |tag: &str| {
            let locale: Locale = tag.parse().ok()?;
            let mut preferences = CollatorPreferences::from(&locale);
            preferences.case_first = Some(match case_first {
                CaseFirst::Upper => CollationCaseFirst::Upper,
                CaseFirst::Lower => CollationCaseFirst::Lower,
                CaseFirst::False => CollationCaseFirst::False,
            });
            preferences.numeric_ordering =
                Some(if numeric { CollationNumericOrdering::True } else { CollationNumericOrdering::False });
            let (strength, case_level) = match sensitivity {
                Sensitivity::Base => (Strength::Primary, CaseLevel::Off),
                Sensitivity::Accent => (Strength::Secondary, CaseLevel::Off),
                Sensitivity::Case => (Strength::Primary, CaseLevel::On),
                Sensitivity::Variant => (Strength::Tertiary, CaseLevel::Off),
            };
            let mut options = CollatorOptions::default();
            options.strength = Some(strength);
            options.case_level = Some(case_level);
            options.alternate_handling = ignore_punctuation.then_some(AlternateHandling::Shifted);
            Collator::try_new(preferences, options).ok().map(Arc::new)
        };
        let icu = build(tag)
            .or_else(|| build(tag.split("-u-").next().unwrap_or(tag)))
            .or_else(|| build("und"))
            .expect("a colação raiz está nos dados compilados");
        CollatorSettings { sensitivity, case_first, numeric, ignore_punctuation, icu }
    }
}

/// O locale do colador para um pedido sem cobertura: o padrão do hospedeiro se o colador o conhece, senão `en-US`.
fn collator_default_locale() -> String {
    collator_locale(&crate::runtime::intl_locale_data::default_locale()).unwrap_or_else(|| "en-US".to_string())
}

impl Default for CollatorSettings {
    /// `defaultCollator()`: `Intl.Collator` sem `locales` nem `options`.
    fn default() -> CollatorSettings {
        CollatorSettings::new(&collator_default_locale(), Sensitivity::Variant, CaseFirst::False, false, false)
    }
}

thread_local! {
    /// O colador padrão do `localeCompare` sem argumentos, construído uma vez por thread.
    static DEFAULT_SETTINGS: CollatorSettings = CollatorSettings::default();
}

/// `ucol_strcoll`: `-1`, `0` ou `1`.
pub fn compare_strings(settings: &CollatorSettings, x: &WtfString, y: &WtfString) -> i32 {
    let icu = &settings.icu;
    let ordering = match (x.is_8bit(), y.is_8bit()) {
        (true, true) => icu.compare_latin1(x.span8(), y.span8()),
        (true, false) => icu.compare_latin1_utf16(x.span8(), y.span16()),
        (false, true) => icu.compare_latin1_utf16(y.span8(), x.span16()).reverse(),
        (false, false) => icu.compare_utf16(x.span16(), y.span16()),
    };
    ordering as i32
}

// ---------------------------------------------------------------------------------------------
// A classe
// ---------------------------------------------------------------------------------------------

/// O estado de um `IntlCollator`.
struct CollatorState {
    locale: String,
    usage: Usage,
    collation: String,
    settings: CollatorSettings,
}

/// Os tipos de colação que o ICU lista para `en` e `pt` (`ucol_getKeywordValuesForLocale`, sem `standard` e
/// `search`): `sortLocaleData[locale].co`.
const SORT_COLLATIONS: [&str; 2] = ["emoji", "eor"];

/// `IntlCollator::initializeCollator`.
fn initialize(global_object: &JSGlobalObject, locales: JSValue, options_value: JSValue) -> Result<CollatorState, Thrown> {
    let requested = canonicalize_locale_list(global_object, locales)?;
    let mut resolved = resolve_locale(&requested, &["co", "kf", "kn"]);
    // O colador do bun usa um conjunto de locales próprio: `en-GB` resolve `en`, `fr-CA` fica como
    // pedido. O primeiro pedido que o conjunto cobre vence; sem nenhum, `en-US`.
    resolved.locale = requested
        .iter()
        .find_map(|tag| parse_language_tag(tag).and_then(|parsed| collator_locale(&parsed.base_name())))
        .unwrap_or_else(|| collator_default_locale());
    resolved.language = Language::of_locale(&resolved.locale);
    let language = resolved.locale.split('-').next().unwrap_or("en").to_string();
    let options = coerce_options_to_object(global_object, options_value)?;

    let usage = option_enum::<Usage>(global_object, options, "usage", "usage must be either \"sort\" or \"search\"")?
        .unwrap_or(Usage::Sort);
    read_locale_matcher(global_object, options)?;
    let collation_option = option_string(global_object, options, "collation", &[], "")?;
    if let Some(collation) = &collation_option {
        if !is_unicode_locale_identifier_type(collation) {
            return Err(Thrown::range_error("collation is not a well-formed collation value"));
        }
    }
    let numeric_option = option_bool(global_object, options, "numeric")?;
    let case_first_option = option_enum::<CaseFirst>(
        global_object,
        options,
        "caseFirst",
        "caseFirst must be either \"upper\", \"lower\", or \"false\"",
    )?;

    // `ResolveLocale` para `co`, `kf` e `kn`; com `usage: "search"` o `localeData` de `co` só tem `null`.
    let collation_values: Vec<&str> =
        if usage == Usage::Sort { SORT_COLLATIONS.iter().chain(extra_collations(&language)).copied().collect() } else { Vec::new() };
    let collation = resolve_key(resolved.keyword("co"), collation_option.as_deref(), &collation_values, None);
    let numeric = resolve_key(
        resolved.keyword("kn"),
        numeric_option.map(|numeric| if numeric { "true" } else { "false" }),
        &["false", "true"],
        Some("false"),
    );
    let case_first = resolve_key(
        resolved.keyword("kf"),
        case_first_option.map(CaseFirst::as_str),
        &["false", "lower", "upper"],
        Some("false"),
    );

    let sensitivity = option_enum::<Sensitivity>(
        global_object,
        options,
        "sensitivity",
        "sensitivity must be either \"base\", \"accent\", \"case\", or \"variant\"",
    )?
    .unwrap_or(Sensitivity::Variant);
    let ignore_punctuation = option_bool(global_object, options, "ignorePunctuation")?.unwrap_or(false);

    let mut honored: Vec<(&str, &str)> = Vec::new();
    for (key, resolved_key) in [("co", &collation), ("kf", &case_first), ("kn", &numeric)] {
        if let (true, Some(value)) = (resolved_key.keep_extension, resolved_key.value.as_deref()) {
            honored.push((key, value));
        }
    }
    let locale = resolved.tag_with(&honored);
    // A colação resolvida vai ao `icu_collator` pela extensão `-u-co-`, mesmo quando veio da opção
    // `collation` e a extensão da tag foi descartada de `locale`.
    let icu_tag = match collation.value.as_deref() {
        Some(value) if value != "default" => resolved.tag_with(&[("co", value)]),
        _ => resolved.tag_with(&[]),
    };
    let settings = CollatorSettings::new(
        &icu_tag,
        sensitivity,
        case_first.value.as_deref().and_then(CaseFirst::parse).unwrap_or(CaseFirst::False),
        numeric.value.as_deref() == Some("true"),
        ignore_punctuation,
    );
    Ok(CollatorState {
        locale,
        usage,
        collation: collation.value.clone().unwrap_or_else(|| "default".to_string()),
        settings,
    })
}

/// `String.prototype.localeCompare`: o colador padrão sem `locales` nem `options`.
pub fn locale_compare(
    global_object: &JSGlobalObject,
    this: &WtfString,
    that: &WtfString,
    locales: JSValue,
    options: JSValue,
) -> Result<i32, Thrown> {
    if locales.is_undefined() && options.is_undefined() {
        if crate::runtime::intl_locale_data::default_locale_is_overridden() {
            return Ok(compare_strings(&CollatorSettings::default(), this, that));
        }
        return Ok(DEFAULT_SETTINGS.with(|settings| compare_strings(settings, this, that)));
    }
    let state = initialize(global_object, locales, options)?;
    Ok(compare_strings(&state.settings, this, that))
}

fn construct_collator_body(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    construct_instance(global_object, call, |global_object| {
        Ok(Box::new(initialize(global_object, call.argument(0), call.argument(1))?))
    })
}

/// `callCollator`: sem `new`, sem ler `newTarget()`.
fn call_collator_body(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    call_instance(global_object, call, |global_object| {
        Ok(Box::new(initialize(global_object, call.argument(0), call.argument(1))?))
    })
}

/// `intlCollatorFuncCompare`: o `compare` ligado à instância.
fn compare_function_body(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    with_instance::<CollatorState, _>(
        call.this_value(),
        "Intl.Collator.prototype.compare called on value that's not a Collator",
        |state, _| {
            let x = to_wtf_checked(global_object, call.argument(0))?;
            let y = to_wtf_checked(global_object, call.argument(1))?;
            Ok(js_number(f64::from(compare_strings(&state.settings, &x, &y))))
        },
    )
}

/// `intlCollatorPrototypeGetterCompare` (`CustomAccessor`).
fn compare_getter_body(global_object: &JSGlobalObject, this_value: JSValue, _name: &PropertyName) -> HostResult {
    with_instance::<CollatorState, _>(
        this_value,
        "Intl.Collator.prototype.compare called on value that's not a Collator",
        |_, instance| bound_function(global_object, instance, collator_compare, "compare", 2),
    )
}

fn resolved_options_body(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    with_instance::<CollatorState, _>(
        call.this_value(),
        "Intl.Collator.prototype.resolvedOptions called on value that's not a Collator",
        |state, _| {
            let vm = global_object.vm();
            let options = new_object(global_object);
            put(global_object, &options, "locale", str_value(vm, &state.locale));
            put(global_object, &options, "usage", str_value(vm, state.usage.as_str()));
            put(global_object, &options, "sensitivity", str_value(vm, state.settings.sensitivity.as_str()));
            put(global_object, &options, "ignorePunctuation", js_boolean(state.settings.ignore_punctuation));
            put(global_object, &options, "collation", str_value(vm, &state.collation));
            put(global_object, &options, "numeric", js_boolean(state.settings.numeric));
            put(global_object, &options, "caseFirst", str_value(vm, state.settings.case_first.as_str()));
            Ok(options.as_value())
        },
    )
}

host_function!(call_collator, call_collator_body);
host_function!(construct_collator, construct_collator_body);
host_function!(collator_compare, compare_function_body);
custom_getter!(collator_proto_compare_getter, compare_getter_body);
host_function!(collator_proto_resolved_options, resolved_options_body);

/// `collatorPrototypeTableValues` de `IntlCollatorPrototype.lut.h`, na ordem do `@begin`.
static COLLATOR_PROTOTYPE_TABLE_VALUES: [HashTableValue; 2] = [
    custom_getter_entry("compare", collator_proto_compare_getter),
    native_entry("resolvedOptions", collator_proto_resolved_options, 0),
];

/// `collatorPrototypeTable`.
static COLLATOR_PROTOTYPE_TABLE: HashTable = HashTable { class_for_this: None, values: &COLLATOR_PROTOTYPE_TABLE_VALUES };

/// `IntlCollatorPrototype::s_info` (`"Intl.Collator"`).
static COLLATOR_PROTOTYPE_S_INFO: ClassInfo = ClassInfo {
    class_name: "Intl.Collator",
    parent_class: Some(&JS_NON_FINAL_OBJECT_S_INFO),
    static_prop_hash_table: Some(&COLLATOR_PROTOTYPE_TABLE),
    inherits_js_type_range: None,
};

/// `IntlCollatorConstructor` e `IntlCollatorPrototype` (`compare`, `resolvedOptions`, da tabela estática,
/// reificadas no primeiro acesso). O construtor funciona com e sem `new`.
pub fn install_collator(global_object: &JSGlobalObject, intl: &JSObject) {
    let class = IntlClass {
        name: "Collator",
        length: 0,
        has_supported_locales_of: true,
        call: call_collator,
        construct: construct_collator,
    };
    class.install_with_table(global_object, intl, &COLLATOR_PROTOTYPE_S_INFO);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn compare_with(settings: &CollatorSettings, x: &str, y: &str) -> i32 {
        let units = |text: &str| crate::runtime::string_prototype::string_from_units(&text.encode_utf16().collect::<Vec<u16>>());
        compare_strings(settings, &units(x), &units(y))
    }

    fn compare(x: &str, y: &str) -> i32 {
        compare_with(&CollatorSettings::default(), x, y)
    }

    #[test]
    fn letters_are_alphabetical_and_lowercase_comes_first() {
        assert_eq!(compare("a", "B"), -1);
        assert_eq!(compare("a", "A"), -1);
        assert_eq!(compare("A", "a"), 1);
        assert_eq!(compare("x", "x"), 0);
        assert_eq!(compare("b", "a"), 1);
        assert_eq!(compare("abc", "abd"), -1);
        assert_eq!(compare("ab", "abc"), -1);
    }

    #[test]
    fn accents_are_secondary() {
        assert_eq!(compare("resume", "r\u{e9}sum\u{e9}"), -1);
        assert_eq!(compare("r\u{e9}sum\u{e9}", "resumf"), -1);
        assert_eq!(compare("e\u{301}", "\u{e9}"), 0);
        let base = CollatorSettings::new("en-US", Sensitivity::Base, CaseFirst::False, false, false);
        assert_eq!(compare_with(&base, "a", "\u{e1}"), 0);
        assert_eq!(compare_with(&base, "a", "A"), 0);
    }

    #[test]
    fn digits_come_before_letters_and_numeric_orders_by_value() {
        assert_eq!(compare("1", "a"), -1);
        assert_eq!(compare("10", "9"), -1);
        let numeric = CollatorSettings::new("en-US", Sensitivity::Variant, CaseFirst::False, true, false);
        assert_eq!(compare_with(&numeric, "10", "9"), 1);
        assert_eq!(compare_with(&numeric, "a10", "a9"), 1);
    }

    #[test]
    fn punctuation_comes_first_and_can_be_ignored() {
        assert_eq!(compare("-a", "a"), -1);
        let ignoring = CollatorSettings::new("en-US", Sensitivity::Variant, CaseFirst::False, false, true);
        assert_eq!(compare_with(&ignoring, "a-b", "ab"), 0);
    }

    fn compare_units(settings: &CollatorSettings, x: &[u16], y: &[u16]) -> i32 {
        let units = |text: &[u16]| crate::runtime::string_prototype::string_from_units(text);
        compare_strings(settings, &units(x), &units(y))
    }

    /// Surrogate solitário tem peso implícito do próprio code unit, como no ICU4C (medido no bun):
    /// não vira U+FFFD nem fica igual a outro surrogate.
    #[test]
    fn lone_surrogates_sort_by_code_unit() {
        let d = CollatorSettings::default();
        assert_eq!(compare_units(&d, &[0xD83D], &[0xD83E]), -1);
        assert_eq!(compare_units(&d, &[0xDC00], &[0xD83D]), 1);
        assert_eq!(compare_units(&d, &[0xD83D], &[0xD83D]), 0);
        assert_eq!(compare_units(&d, &[0xD83D], &[u16::from(b'a')]), 1);
        assert_eq!(compare_units(&d, &[0xD83D], &[0xFFFF]), -1);
        assert_eq!(compare_units(&d, &[0xD83D], &[0xFFFD]), -1);
        // Prefixo comum antes do surrogate e dado latin1 contra UTF-16.
        assert_eq!(compare_units(&d, &[0x61, 0xD83D], &[0x61, 0xD83E]), -1);
        assert_eq!(compare_units(&d, &[0xD83D, 0x61], &[0xD83D, 0x62]), -1);
    }

    /// Marca combinante depois do surrogate solitário: a marca vale no nível secundário.
    #[test]
    fn lone_surrogate_with_combining_mark() {
        let d = CollatorSettings::default();
        assert_eq!(compare_units(&d, &[0xD83D, 0x301], &[0xD83D]), 1);
        assert_eq!(compare_units(&d, &[0xD83D, 0x301], &[0xD83D, 0x300]), -1);
    }

    #[test]
    fn lone_surrogates_respect_sensitivity() {
        let base = CollatorSettings::new("en-US", Sensitivity::Base, CaseFirst::False, false, false);
        assert_eq!(compare_units(&base, &[0xD83D], &[0xD83E]), -1);
        assert_eq!(compare_units(&base, &[0xD83D, 0x301], &[0xD83D]), 0);
        let accent = CollatorSettings::new("en-US", Sensitivity::Accent, CaseFirst::False, false, false);
        assert_eq!(compare_units(&accent, &[0xD83D, 0x301], &[0xD83D]), 1);
    }

    #[test]
    fn case_first_upper_reverses_case() {
        let upper = CollatorSettings::new("en-US", Sensitivity::Variant, CaseFirst::Upper, false, false);
        assert_eq!(compare_with(&upper, "a", "A"), 1);
    }
}
