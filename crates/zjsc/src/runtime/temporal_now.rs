//! Porte de `runtime/TemporalNow.{h,cpp}`: o namespace `Temporal.Now` (com `@@toStringTag`), `instant`,
//! `timeZoneId`, `plainDateISO`, `plainDateTimeISO`, `plainTimeISO` e `zonedDateTimeISO`.
//!
//! DIVERGÊNCIAS:
//!
//! - `systemUTCEpochNanoseconds` lê o `overridenDateNow` do `bun test`, que o global do porte não tem (mesmo
//!   caso de `date_constructor_natives.rs`): sem substituição, é o relógio inteiro, `ExactTime::now()`, o mesmo
//!   de `Date.now` com a resolução de nanossegundos.
//! - `timeZoneId` vem do fuso do processo (`ProcessTimeZone`, o `TZ` como o Debian resolve), que o
//!   `DateCache` do `VM` também usa; o C++ lê o `defaultTimeZone()` do `DateCache`, que não expõe o nome.
//! - `plainDateISO`, `plainDateTimeISO` e `plainTimeISO` sem argumento leem o deslocamento do fuso do processo
//!   (`ProcessTimeZone`, como o `msToGregorianDateTime` do `DateCache` no C++, em milissegundos); com
//!   `timeZoneLike` resolvem o fuso por `to_temporal_time_zone_identifier`.
//! - `zonedDateTimeISO` sem argumento usa o fuso do processo pelo nome (`system_time_zone`): o `TZ` que não tem
//!   nome IANA (regra POSIX, caminho de arquivo) vira `UTC`, como o `timeZoneId`.

use crate::host_function;
use crate::runtime::class_info::ClassInfo;
use crate::runtime::collection_support::put_to_string_tag;
use crate::runtime::host_call::{HostCall, HostResult, Thrown};
use crate::runtime::identifier::Identifier;
use crate::runtime::intl_support::str_value;
use crate::runtime::intrinsic::Intrinsic;
use crate::runtime::iso8601::{ExactTime, PlainDateTime};
use crate::runtime::js_date_math::TimeZoneSource;
use crate::runtime::js_global_object::JSGlobalObject;
use crate::runtime::js_object::{JSNonFinalObject, JSObject, JSObjectRef, JS_NON_FINAL_OBJECT_S_INFO};
use crate::runtime::js_type::JSType;
use crate::runtime::js_type_info::{TypeInfo, HAS_STATIC_PROPERTY_TABLE};
use crate::runtime::js_value::JSValue;
use crate::runtime::lookup::{HashTable, HashTableValue, Kind};
use crate::runtime::lookup::{native_entry};
use crate::runtime::native_function::NativeFunction;
use crate::runtime::process_time_zone::ProcessTimeZone;
use crate::runtime::property_attribute::{DONT_ENUM, FUNCTION};
use crate::runtime::property_name::PropertyName;
use crate::runtime::structure::{Structure, StructureRef};
use crate::runtime::temporal_calendar::ISO8601_CALENDAR_ID;
use crate::runtime::temporal_instant::TemporalInstant;
use crate::runtime::temporal_plain_date::TemporalPlainDate;
use crate::runtime::temporal_plain_date_time::TemporalPlainDateTime;
use crate::runtime::temporal_plain_time::TemporalPlainTime;
use crate::runtime::temporal_time_zone::{exact_time_to_local_date_and_time, get_iso_date_time_for, intl_resolve_time_zone_id, TimeZone};
use crate::runtime::temporal_zoned_date_time::{create_temporal_zoned_date_time, to_temporal_time_zone_identifier};
use crate::runtime::vm::VM;

/// `const ClassInfo TemporalNow::s_info` (`"Temporal.Now"`, `&temporalNowTable`).
pub static TEMPORAL_NOW_S_INFO: ClassInfo = ClassInfo {
    class_name: "Temporal.Now",
    parent_class: Some(&JS_NON_FINAL_OBJECT_S_INFO),
    static_prop_hash_table: Some(&TEMPORAL_NOW_TABLE),
    inherits_js_type_range: None,
};

/// `class TemporalNow final : public JSNonFinalObject`: sem campos próprios.
pub struct TemporalNow;

impl TemporalNow {
    /// `StructureFlags = Base::StructureFlags | HasStaticPropertyTable`.
    pub const STRUCTURE_FLAGS: u32 = JSNonFinalObject::STRUCTURE_FLAGS | HAS_STATIC_PROPERTY_TABLE;

    /// `createStructure(vm, globalObject)`: o protótipo é `globalObject->objectPrototype()`.
    pub fn create_structure(vm: &VM, global_object: &JSGlobalObject, object_prototype: crate::runtime::js_value::JSValue) -> StructureRef {
        Structure::create(
            vm,
            Some(global_object),
            object_prototype,
            TypeInfo::new(JSType::ObjectType, TemporalNow::STRUCTURE_FLAGS),
            &TEMPORAL_NOW_S_INFO,
        )
    }

    /// `create(vm, structure)`: `TemporalNow(vm, structure)` e `finishCreation`.
    pub fn create(vm: &VM, structure: &StructureRef) -> JSObjectRef {
        let object = JSObject::allocate(vm, structure);
        TemporalNow::finish_creation(&object, vm);
        object
    }

    /// `finishCreation(vm)`: `JSC_TO_STRING_TAG_WITHOUT_TRANSITION()`; as seis funções ficam na tabela.
    fn finish_creation(object: &JSObject, vm: &VM) {
        object.finish_creation(vm);
        put_to_string_tag(vm, object, TEMPORAL_NOW_S_INFO.class_name);
    }
}

/// `temporalNowFuncInstant`: https://tc39.es/proposal-temporal/#sec-temporal.now.instant
/// (`systemUTCEpochNanoseconds()`, https://tc39.es/proposal-temporal/#sec-temporal-systemutcepochnanoseconds)
fn now_instant_body(global_object: &JSGlobalObject, _call: &HostCall) -> HostResult {
    let instant = TemporalInstant::create(global_object.vm(), global_object.instant_structure(), ExactTime::now());
    Ok(instant.as_value())
}

/// `temporalNowFuncTimeZoneId`: https://tc39.es/proposal-temporal/#sec-temporal.now.timezoneid
fn now_time_zone_id_body(global_object: &JSGlobalObject, _call: &HostCall) -> HostResult {
    Ok(str_value(global_object.vm(), &ProcessTimeZone::new().time_zone_id()))
}

host_function!(temporal_now_func_instant, now_instant_body);
host_function!(temporal_now_func_time_zone_id, now_time_zone_id_body);

/// `resolveNowTimeZone(globalObject, arg)`: `undefined` é `None` (o chamador usa o fuso do sistema); senão
/// `ToTemporalTimeZoneIdentifier(temporalTimeZoneLike)`.
fn resolve_now_time_zone(global_object: &JSGlobalObject, time_zone_like: JSValue) -> Result<Option<TimeZone>, Thrown> {
    if time_zone_like.is_undefined() {
        return Ok(None);
    }
    Ok(Some(to_temporal_time_zone_identifier(global_object, time_zone_like)?))
}

/// `vm.dateCache.defaultTimeZone()`: o fuso do processo pelo nome IANA (`UTC` se o `TZ` não tem nome).
fn system_time_zone() -> TimeZone {
    intl_resolve_time_zone_id(ProcessTimeZone::new().time_zone_id().as_bytes()).map_or_else(TimeZone::utc, TimeZone::Id)
}

/// O corpo de `systemDateTime(globalObject, timeZoneLike)` depois de resolvido o fuso: o instante agora, e
/// `GetISODateTimeFor` no fuso; `None` é o fuso do sistema, com o deslocamento do `DateCache` no instante (o
/// `ProcessTimeZone`, em milissegundos).
fn system_date_time_in(time_zone: Option<&TimeZone>) -> Result<PlainDateTime, Thrown> {
    // Passo 3: `SystemUTCEpochNanoseconds()`.
    let exact_time = ExactTime::now();
    // Passo 4: `GetISODateTimeFor`.
    match time_zone {
        Some(time_zone) => Ok(get_iso_date_time_for(time_zone, exact_time)?),
        None => {
            let offset_ms = ProcessTimeZone::new().utc_offset(exact_time.floor_epoch_milliseconds() as f64).map_or(0, |offset| offset.offset);
            Ok(exact_time_to_local_date_and_time(exact_time, i64::from(offset_ms) * 1_000_000))
        }
    }
}

/// `systemDateTime(globalObject, timeZoneLike)`: https://tc39.es/proposal-temporal/#sec-temporal-systemdatetime
fn system_date_time(global_object: &JSGlobalObject, time_zone_like: JSValue) -> Result<PlainDateTime, Thrown> {
    // Passos 1 e 2: `undefined` é o fuso do sistema; senão `ToTemporalTimeZoneIdentifier`.
    let time_zone = resolve_now_time_zone(global_object, time_zone_like)?;
    system_date_time_in(time_zone.as_ref())
}

/// `temporalNowFuncPlainDateISO`: https://tc39.es/proposal-temporal/#sec-temporal.now.plaindateiso
fn now_plain_date_iso_body(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    // Passo 1: `SystemDateTime(temporalTimeZoneLike)`. Passo 2: `CreateTemporalDate(isoDateTime.[[ISODate]], "iso8601")`.
    let iso_date_time = system_date_time(global_object, call.argument(0))?;
    let plain_date = TemporalPlainDate::create(global_object.vm(), &global_object.plain_date_structure(), iso_date_time.date, ISO8601_CALENDAR_ID);
    Ok(plain_date.as_value())
}

/// `temporalNowFuncPlainDateTimeISO`: https://tc39.es/proposal-temporal/#sec-temporal.now.plaindatetimeiso
fn now_plain_date_time_iso_body(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    // Passo 1: `SystemDateTime(temporalTimeZoneLike)`. Passo 2: `CreateTemporalDateTime(isoDateTime, "iso8601")`.
    let iso_date_time = system_date_time(global_object, call.argument(0))?;
    let plain_date_time = TemporalPlainDateTime::create(
        global_object.vm(),
        &global_object.plain_date_time_structure(),
        iso_date_time.date,
        iso_date_time.time,
        ISO8601_CALENDAR_ID,
    );
    Ok(plain_date_time.as_value())
}

/// `temporalNowFuncPlainTimeISO`: https://tc39.es/proposal-temporal/#sec-temporal.now.plaintimeiso
fn now_plain_time_iso_body(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    // Passo 1: `SystemDateTime(temporalTimeZoneLike)`. Passo 2: `CreateTemporalTime(isoDateTime.[[Time]])`.
    let iso_date_time = system_date_time(global_object, call.argument(0))?;
    let plain_time = TemporalPlainTime::create(global_object.vm(), &global_object.plain_time_structure(), iso_date_time.time);
    Ok(plain_time.as_value())
}

/// `temporalNowFuncZonedDateTimeISO`: https://tc39.es/proposal-temporal/#sec-temporal.now.zoneddatetimeiso
fn now_zoned_date_time_iso_body(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    // Passo 1: `undefined` é `SystemTimeZoneIdentifier()`; senão `ToTemporalTimeZoneIdentifier(temporalTimeZoneLike)`.
    let time_zone = resolve_now_time_zone(global_object, call.argument(0))?.unwrap_or_else(system_time_zone);
    // Passo 2: `SystemUTCEpochNanoseconds()`.
    let exact_time = ExactTime::now();
    // Passo 3: `CreateTemporalZonedDateTime(epochNs, tz, "iso8601")`.
    Ok(create_temporal_zoned_date_time(global_object, exact_time, time_zone, ISO8601_CALENDAR_ID, None)?.as_value())
}

host_function!(temporal_now_func_plain_date_iso, now_plain_date_iso_body);
host_function!(temporal_now_func_plain_date_time_iso, now_plain_date_time_iso_body);
host_function!(temporal_now_func_plain_time_iso, now_plain_time_iso_body);
host_function!(temporal_now_func_zoned_date_time_iso, now_zoned_date_time_iso_body);

/// `temporalNowTable`, na ordem do `@begin`.
static TEMPORAL_NOW_TABLE_VALUES: [HashTableValue; 6] = [
    native_entry("instant", temporal_now_func_instant, 0),
    native_entry("timeZoneId", temporal_now_func_time_zone_id, 0),
    native_entry("plainDateISO", temporal_now_func_plain_date_iso, 0),
    native_entry("plainDateTimeISO", temporal_now_func_plain_date_time_iso, 0),
    native_entry("plainTimeISO", temporal_now_func_plain_time_iso, 0),
    native_entry("zonedDateTimeISO", temporal_now_func_zoned_date_time_iso, 0),
];

static TEMPORAL_NOW_TABLE: HashTable = HashTable { class_for_this: None, values: &TEMPORAL_NOW_TABLE_VALUES };

/// A propriedade `Now` do `Temporal` (`DontEnum`, a entrada de `temporalObjectTable`).
pub fn install_temporal_now(global_object: &JSGlobalObject, temporal: &JSObject, object_prototype: &JSObjectRef) {
    let vm = global_object.vm();
    let structure = TemporalNow::create_structure(vm, global_object, object_prototype.as_value());
    let now = TemporalNow::create(vm, &structure);
    temporal.put_direct(vm, &PropertyName::from_identifier(&Identifier::from_span(vm, b"Now".as_slice())), now.as_value(), DONT_ENUM);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::runtime::process_time_zone::set_time_zone_spec_override;

    fn minutes_of_day(date_time: &PlainDateTime) -> i64 {
        i64::from(date_time.time.hour()) * 60 + i64::from(date_time.time.minute())
    }

    #[test]
    fn system_date_time_follows_the_process_time_zone() {
        set_time_zone_spec_override(Some("UTC"));
        let utc = system_date_time_in(None).unwrap();
        set_time_zone_spec_override(Some("America/Sao_Paulo"));
        let sao_paulo = system_date_time_in(None).unwrap();
        set_time_zone_spec_override(None);
        // São Paulo é UTC-3 o ano todo desde 2019; os dois instantes diferem de microssegundos.
        let difference = (minutes_of_day(&utc) - minutes_of_day(&sao_paulo)).rem_euclid(24 * 60);
        assert!(difference == 180 || difference == 179, "diferença de {difference} minutos");
    }

    #[test]
    fn explicit_time_zone_goes_through_the_core() {
        let utc = system_date_time_in(Some(&TimeZone::utc())).unwrap();
        let minus_three = system_date_time_in(Some(&TimeZone::UtcOffset(-3 * 3_600_000_000_000))).unwrap();
        let difference = (minutes_of_day(&utc) - minutes_of_day(&minus_three)).rem_euclid(24 * 60);
        assert!(difference == 180 || difference == 179, "diferença de {difference} minutos");
    }

    #[test]
    fn the_system_time_zone_has_the_process_name() {
        set_time_zone_spec_override(Some("America/Sao_Paulo"));
        let time_zone = system_time_zone();
        set_time_zone_spec_override(Some("EST5EDT,M3.2.0,M11.1.0"));
        let posix = system_time_zone();
        set_time_zone_spec_override(None);
        assert_eq!(time_zone.to_string(), "America/Sao_Paulo");
        // `TZ` sem nome IANA vira `UTC`, como o `timeZoneId`.
        assert_eq!(posix, TimeZone::utc());
    }
}

