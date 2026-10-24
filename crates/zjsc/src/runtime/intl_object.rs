//! `Intl` (`IntlObject.cpp`): o objeto global com `getCanonicalLocales`, `supportedValuesOf`, o
//! `@@toStringTag` e os construtores das classes (`Collator`, `DateTimeFormat`, `DisplayNames`,
//! `DurationFormat`, `ListFormat`, `Locale`, `NumberFormat`, `PluralRules`, `RelativeTimeFormat` e `Segmenter`).
//!
//! LACUNAS, e por quê (ausentes em vez de falsas): `Intl.DisplayNames`
//! tem só as tabelas de en e pt-BR (`intl_display_names_data.rs`), sem o CLDR inteiro. As locales
//! cobertas são en e pt-BR (`intl_locale_data.rs`).
//!
//! `intlObjectTable` é a tabela estática de `IntlObject::s_info`: as duas funções e os dez construtores
//! (`PropertyCallback`) nascem no primeiro acesso, via `reify_static_property`. O callback reaproveita o
//! `install_*` de cada classe, que grava o construtor em `Intl` e o devolve.
//!
//! `supportedValuesOf`: as seis chaves devolvem as listas completas do ICU do bun 1.4.2 (calendar 16,
//! collation 12, currency 307, numberingSystem 78, timeZone 445, unit 45), como o JSC; o que as classes
//! honram (só `gregory`, `latn`, as moedas e unidades das tabelas) é outra coisa. Conferido por
//! `tests/intl_object_bun_golden.rs`.

use crate::host_function;
use crate::runtime::collection_support::put_to_string_tag;
use crate::runtime::host_call::{HostCall, HostResult, Thrown};
use crate::runtime::intl_collator::install_collator;
use crate::runtime::intl_date_time_format::install_date_time_format;
use crate::runtime::intl_display_names::install_display_names;
use crate::runtime::intl_duration_format::install_duration_format;
use crate::runtime::intl_list_format::install_list_format;
use crate::runtime::intl_locale::install_locale;
use crate::runtime::intl_number_format::install_number_format;
use crate::runtime::intl_plural_rules::install_plural_rules;
use crate::runtime::intl_relative_time_format::install_relative_time_format;
use crate::runtime::intl_segmenter::install_segmenter;
use crate::runtime::intl_supported_values_data::{CURRENCIES, NUMBERING_SYSTEMS, TIME_ZONES, UNITS};
use crate::runtime::class_info::ClassInfo;
use crate::runtime::intl_support::{canonicalize_locale_list, prop, string_array, to_rust_string};
use crate::runtime::intrinsic::Intrinsic;
use crate::runtime::js_global_object::JSGlobalObject;
use crate::runtime::js_object::{JSNonFinalObject, JSObject, JS_NON_FINAL_OBJECT_S_INFO};
use crate::runtime::js_type::JSType;
use crate::runtime::js_type_info::{TypeInfo, HAS_STATIC_PROPERTY_TABLE};
use crate::runtime::js_value::JSValue;
use crate::runtime::lookup::{HashTable, HashTableValue, Kind, LazyPropertyCallback};
use crate::runtime::lookup::{lazy_entry, native_entry};
use crate::runtime::native_function::NativeFunction;
use crate::runtime::property_attribute::{DONT_ENUM, FUNCTION, PROPERTY_CALLBACK};
use crate::runtime::structure::{Structure, StructureRef};
use crate::runtime::vm::VM;

/// `intlObjectFuncGetCanonicalLocales`.
fn get_canonical_locales_body(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    let locales = canonicalize_locale_list(global_object, call.argument(0))?;
    Ok(string_array(global_object, &locales))
}

/// `intlObjectFuncSupportedValuesOf`.
fn supported_values_of_body(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    let key = to_rust_string(global_object, call.argument(0))?;
    let values: Vec<String> = match key.as_str() {
        // `intlAvailableCalendars()`: `FOR_EACH_CACHED_CALENDAR_ID` mais `iso8601`, em ordem de ponto de
        // código (16 valores no bun 1.4.2). O que `DateTimeFormat` honra continua só `gregory`.
        "calendar" => [
            "buddhist", "chinese", "coptic", "dangi", "ethioaa", "ethiopic", "gregory", "hebrew", "indian", "islamic-civil",
            "islamic-tbla", "islamic-umalqura", "iso8601", "japanese", "persian", "roc",
        ]
        .iter()
        .map(|name| (*name).to_string())
        .collect(),
        // `availableCollations`: `emoji` e `eor` fixos mais os tipos de `ucol_getKeywordValues` sem `standard` e
        // `search`, com os nomes BCP 47, em ordem de ponto de código (a lista medida no bun 1.4.2).
        "collation" => [
            "compat", "dict", "emoji", "eor", "phonebk", "phonetic", "pinyin", "searchjl", "stroke", "trad", "unihan", "zhuyin",
        ]
        .iter()
        .map(|name| (*name).to_string())
        .collect(),
        // As quatro listas abaixo são as do ICU do bun 1.4.2 (`intl_supported_values_data.rs`, gerado), não as
        // tabelas das classes: o que `supportedValuesOf` reporta independe do que cada classe honra.
        "currency" => CURRENCIES.iter().map(|code| (*code).to_string()).collect(),
        "numberingSystem" => NUMBERING_SYSTEMS.iter().map(|name| (*name).to_string()).collect(),
        "timeZone" => TIME_ZONES.iter().map(|name| (*name).to_string()).collect(),
        "unit" => UNITS.iter().map(|name| (*name).to_string()).collect(),
        _ => return Err(Thrown::range_error("Unknown key for Intl.supportedValuesOf")),
    };
    Ok(string_array(global_object, &values))
}

host_function!(intl_object_func_get_canonical_locales, get_canonical_locales_body);
host_function!(intl_object_func_supported_values_of, supported_values_of_body);

/// `createXConstructor(vm, object)` do C++ (`PropertyCallback`): o `install_*` da classe cria protótipo e
/// construtor e grava o construtor em `Intl` (`DontEnum`); o callback devolve o valor gravado, e a reificação
/// da tabela o grava de novo com os mesmos atributos (substituição no lugar, sem transição).
macro_rules! lazy_constructor {
    ($callback:ident, $install:ident, $name:literal) => {
        fn $callback(vm: &VM, intl: &JSObject) -> JSValue {
            let global_object = intl.structure().realm().expect("Intl sem realm");
            $install(&global_object, intl);
            intl.get_direct_by_name(vm, &prop(vm, $name))
        }
    };
}

lazy_constructor!(create_collator_constructor, install_collator, "Collator");
lazy_constructor!(create_date_time_format_constructor, install_date_time_format, "DateTimeFormat");
lazy_constructor!(create_display_names_constructor, install_display_names, "DisplayNames");
lazy_constructor!(create_duration_format_constructor, install_duration_format, "DurationFormat");
lazy_constructor!(create_list_format_constructor, install_list_format, "ListFormat");
lazy_constructor!(create_locale_constructor, install_locale, "Locale");
lazy_constructor!(create_number_format_constructor, install_number_format, "NumberFormat");
lazy_constructor!(create_plural_rules_constructor, install_plural_rules, "PluralRules");
lazy_constructor!(create_relative_time_format_constructor, install_relative_time_format, "RelativeTimeFormat");
lazy_constructor!(create_segmenter_constructor, install_segmenter, "Segmenter");

/// `intlObjectTable`, na ordem do `@begin`.
static INTL_OBJECT_TABLE_VALUES: [HashTableValue; 12] = [
    native_entry("getCanonicalLocales", intl_object_func_get_canonical_locales, 1),
    native_entry("supportedValuesOf", intl_object_func_supported_values_of, 1),
    lazy_entry("Collator", create_collator_constructor),
    lazy_entry("DateTimeFormat", create_date_time_format_constructor),
    lazy_entry("DisplayNames", create_display_names_constructor),
    lazy_entry("DurationFormat", create_duration_format_constructor),
    lazy_entry("ListFormat", create_list_format_constructor),
    lazy_entry("Locale", create_locale_constructor),
    lazy_entry("NumberFormat", create_number_format_constructor),
    lazy_entry("PluralRules", create_plural_rules_constructor),
    lazy_entry("RelativeTimeFormat", create_relative_time_format_constructor),
    lazy_entry("Segmenter", create_segmenter_constructor),
];

static INTL_OBJECT_TABLE: HashTable = HashTable { class_for_this: None, values: &INTL_OBJECT_TABLE_VALUES };

/// `const ClassInfo IntlObject::s_info` (`"Intl"`, `&intlObjectTable`).
pub static INTL_OBJECT_S_INFO: ClassInfo = ClassInfo {
    class_name: "Intl",
    parent_class: Some(&JS_NON_FINAL_OBJECT_S_INFO),
    static_prop_hash_table: Some(&INTL_OBJECT_TABLE),
    inherits_js_type_range: None,
};

/// `IntlObject::createStructure`: `ObjectType` com `StructureFlags` (`HasStaticPropertyTable`).
fn create_intl_structure(vm: &VM, global_object: &JSGlobalObject) -> StructureRef {
    Structure::create(
        vm,
        Some(global_object),
        global_object.object_prototype().as_value(),
        TypeInfo::new(JSType::ObjectType, JSNonFinalObject::STRUCTURE_FLAGS | HAS_STATIC_PROPERTY_TABLE),
        &INTL_OBJECT_S_INFO,
    )
}

/// Cria `Intl` e o põe no global (`DontEnum`). Como no `finishCreation` do C++, só o `@@toStringTag` entra
/// na criação; as doze entradas da tabela nascem no primeiro acesso.
pub fn install_intl(global_object: &JSGlobalObject) {
    let vm = global_object.vm();
    let structure = create_intl_structure(vm, global_object);
    let intl = JSObject::allocate(vm, &structure);
    intl.finish_creation(vm);
    put_to_string_tag(vm, &intl, "Intl");

    global_object.put_direct(vm, &prop(vm, "Intl"), intl.as_value(), DONT_ENUM);
}
