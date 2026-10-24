//! Porte de `runtime/TemporalZonedDateTimeConstructor.{h,cpp}`: o construtor `Temporal.ZonedDateTime` (um
//! `InternalFunction`, comprimento 2) com `from` e `compare`, e `install_zoned_date_time`, que cria o protótipo,
//! a estrutura intrínseca (`m_zonedDateTimeStructure`) e põe o construtor no `Temporal` (a entrada `ZonedDateTime`
//! de `temporalObjectTable`).
//!
//! DIVERGÊNCIA: o `LazyClassStructure` do C++ cria tudo na primeira leitura de `Temporal.ZonedDateTime`; aqui é
//! eager, junto com o `Temporal` (ver a DIVERGÊNCIA de `temporal_object.rs`).

use crate::host_function;
use crate::runtime::class_info::ClassInfo;
use crate::runtime::collection_support::{
    collection_constructor_structure, constructor_cannot_be_called_as_function, create_collection_constructor,
};
use crate::runtime::lookup::{HashTable, HashTableValue};
use crate::runtime::lookup::{native_entry};
use crate::runtime::host_call::{pending_or, HostCall, HostResult, Thrown};
use crate::runtime::identifier::Identifier;
use crate::runtime::internal_function::{InternalFunctionRef, INTERNAL_FUNCTION_S_INFO};
use crate::runtime::iso8601::parse_time_zone_identifier_string;
use crate::runtime::js_big_int_ops::to_big_int;
use crate::runtime::js_global_object::JSGlobalObject;
use crate::runtime::js_object::{JSObject, JSObjectRef};
use crate::runtime::js_value::{js_number, JSValue};
use crate::runtime::property_attribute::DONT_ENUM;
use crate::runtime::property_name::PropertyName;
use crate::runtime::structure::StructureRef;
use crate::runtime::temporal_calendar::{is_builtin_calendar, CalendarID, ISO8601_CALENDAR_ID};
use crate::runtime::temporal_instant::big_int_value_to_exact_time;
use crate::runtime::temporal_object::{ellipsize_at, string_units, throw_range_error_with_units};
use crate::runtime::temporal_zoned_date_time::{
    create_temporal_zoned_date_time, time_zone_from_identifier_parse_record, TemporalZonedDateTime,
};
use crate::runtime::temporal_zoned_date_time_prototype::TemporalZonedDateTimePrototype;
use crate::runtime::vm::VM;

/// `const ClassInfo TemporalZonedDateTimeConstructor::s_info` (`"Function"`).
pub static TEMPORAL_ZONED_DATE_TIME_CONSTRUCTOR_S_INFO: ClassInfo =
    ClassInfo { class_name: "Function", parent_class: Some(&INTERNAL_FUNCTION_S_INFO), static_prop_hash_table: Some(&CONSTRUCTOR_TABLE), inherits_js_type_range: None };

/// `callTemporalZonedDateTime`: `throwConstructorCannotBeCalledAsFunctionTypeError(globalObject, scope, "ZonedDateTime")`.
fn call_temporal_zoned_date_time_body(_global_object: &JSGlobalObject, _call: &HostCall) -> HostResult {
    constructor_cannot_be_called_as_function("ZonedDateTime")
}

/// `constructTemporalZonedDateTime`: https://tc39.es/proposal-temporal/#sec-temporal.zoneddatetime
fn construct_temporal_zoned_date_time_body(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    // Passo 1: `NewTarget` `undefined` é tratado pela divisão chamada/construção (ver `callTemporalZonedDateTime`).

    // Passo 2: `epochNanoseconds = ? ToBigInt(epochNanoseconds)`.
    let big_int_value = pending_or(global_object, to_big_int(call.argument(0)))?;
    // Passo 3: `IsValidEpochNanoseconds` falso é `RangeError`.
    let exact_time = big_int_value_to_exact_time(global_object, big_int_value, "Temporal.ZonedDateTime")?;

    // Passo 4: `timeZone` que não é `String` é `TypeError`.
    let time_zone_value = call.argument(1);
    if !time_zone_value.is_string() {
        return Err(Thrown::type_error("Temporal.ZonedDateTime timeZoneIdentifier must be a string"));
    }
    let time_zone_string = string_units(global_object, time_zone_value)?;
    let invalid_time_zone = || {
        throw_range_error_with_units(global_object, "'", &ellipsize_at(100, &time_zone_string), "' is not a valid time zone identifier")
    };

    // Passo 5: `timeZoneParse = ? ParseTimeZoneIdentifier(timeZone)`.
    let Some(time_zone_parse) = parse_time_zone_identifier_string(&time_zone_string) else {
        return Err(invalid_time_zone());
    };

    // Passos 6 e 7: `[[OffsetMinutes]]` vazio resolve `[[Name]]` (o `RangeError` do passo 6.b se indisponível); senão
    // `FormatOffsetTimeZoneIdentifier`.
    let time_zone = time_zone_from_identifier_parse_record(&time_zone_parse).ok_or_else(invalid_time_zone)?;

    // Passos 8 a 10: `calendar` `undefined` é `"iso8601"`; não `String` é `TypeError`; `CanonicalizeCalendar`.
    let mut calendar_id: CalendarID = ISO8601_CALENDAR_ID;
    let calendar_argument = call.argument(2);
    if !calendar_argument.is_undefined() {
        if !calendar_argument.is_string() {
            return Err(Thrown::type_error("Temporal.ZonedDateTime calendar must be a string"));
        }
        let calendar_string = string_units(global_object, calendar_argument)?;
        calendar_id = is_builtin_calendar(&calendar_string).ok_or_else(|| {
            throw_range_error_with_units(global_object, "'", &ellipsize_at(100, &calendar_string), "' is not a valid calendar identifier")
        })?;
    }

    // Passo 11: `CreateTemporalZonedDateTime(epochNanoseconds, timeZone, calendar, NewTarget)`.
    Ok(create_temporal_zoned_date_time(global_object, exact_time, time_zone, calendar_id, Some((call.new_target(), call.callee())))?.as_value())
}

/// `temporalZonedDateTimeConstructorFuncFrom`: https://tc39.es/proposal-temporal/#sec-temporal.zoneddatetime.from
fn temporal_zoned_date_time_constructor_func_from_body(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    // Passo 1: `ToTemporalZonedDateTime(item, options)`.
    Ok(TemporalZonedDateTime::from(global_object, call.argument(0), call.argument(1))?.as_value())
}

/// `temporalZonedDateTimeConstructorFuncCompare`: https://tc39.es/proposal-temporal/#sec-temporal.zoneddatetime.compare
fn temporal_zoned_date_time_constructor_func_compare_body(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    // Passos 1 e 2: `ToTemporalZonedDateTime(one)` e `ToTemporalZonedDateTime(two)`.
    let one = TemporalZonedDateTime::from(global_object, call.argument(0), JSValue::Undefined)?;
    let two = TemporalZonedDateTime::from(global_object, call.argument(1), JSValue::Undefined)?;
    // Passo 3: `CompareEpochNanoseconds`.
    Ok(js_number(match one.exact_time().cmp(&two.exact_time()) {
        std::cmp::Ordering::Less => -1,
        std::cmp::Ordering::Equal => 0,
        std::cmp::Ordering::Greater => 1,
    }))
}

host_function!(call_temporal_zoned_date_time, call_temporal_zoned_date_time_body);
host_function!(construct_temporal_zoned_date_time, construct_temporal_zoned_date_time_body);
host_function!(temporal_zoned_date_time_constructor_func_from, temporal_zoned_date_time_constructor_func_from_body);
host_function!(temporal_zoned_date_time_constructor_func_compare, temporal_zoned_date_time_constructor_func_compare_body);

/// `temporalZonedDateTimeConstructorTableValues`, na ordem do `@begin`.
static CONSTRUCTOR_TABLE_VALUES: [HashTableValue; 2] = [
    native_entry("from", temporal_zoned_date_time_constructor_func_from, 1),
    native_entry("compare", temporal_zoned_date_time_constructor_func_compare, 2),
];

/// `temporalZonedDateTimeConstructorTable`.
static CONSTRUCTOR_TABLE: HashTable = HashTable { class_for_this: None, values: &CONSTRUCTOR_TABLE_VALUES };

/// `class TemporalZonedDateTimeConstructor final : public InternalFunction`: sem campos próprios.
pub struct TemporalZonedDateTimeConstructor;

impl TemporalZonedDateTimeConstructor {
    /// `createStructure(vm, globalObject, prototype)`: `InternalFunctionType`.
    pub fn create_structure(vm: &VM, global_object: &JSGlobalObject, prototype: JSValue) -> StructureRef {
        collection_constructor_structure(vm, global_object, prototype, &TEMPORAL_ZONED_DATE_TIME_CONSTRUCTOR_S_INFO)
    }

    /// `create(vm, structure, zonedDateTimePrototype)`: `finishCreation` com comprimento 2, nome `"ZonedDateTime"`,
    /// `prototype` `DontEnum|DontDelete|ReadOnly` e a tabela (`from` com 1, `compare` com 2, `DontEnum`).
    pub fn create(
        vm: &VM,
        global_object: &JSGlobalObject,
        structure: StructureRef,
        zoned_date_time_prototype: &JSObject,
    ) -> InternalFunctionRef {
        create_collection_constructor(
            vm,
            global_object,
            structure,
            zoned_date_time_prototype,
            "ZonedDateTime",
            2,
            call_temporal_zoned_date_time,
            construct_temporal_zoned_date_time,
            false,
        )
    }
}

/// `createZonedDateTimeConstructor` e o `LazyClassStructure` de `Temporal.ZonedDateTime`: cria o protótipo
/// (`didBecomePrototype`), a estrutura intrínseca (`globalObject->zonedDateTimeStructure()`), o construtor, o
/// `constructor` do protótipo (`DontEnum`) e a propriedade `ZonedDateTime` do `Temporal` (`DontEnum`).
pub fn install_zoned_date_time(global_object: &JSGlobalObject, temporal: &JSObject, object_prototype: &JSObjectRef) {
    let vm = global_object.vm();
    let function_prototype = global_object.function_prototype();

    let prototype_structure = TemporalZonedDateTimePrototype::create_structure(vm, global_object, object_prototype.as_value());
    let prototype = TemporalZonedDateTimePrototype::create(vm, global_object, &prototype_structure);
    prototype.did_become_prototype(vm);

    let zoned_date_time_structure = TemporalZonedDateTime::create_structure(vm, global_object, prototype.as_value());
    global_object.temporal_data.borrow_mut().zoned_date_time_structure = Some(zoned_date_time_structure);

    let constructor_structure = TemporalZonedDateTimeConstructor::create_structure(vm, global_object, function_prototype.as_value());
    let constructor = TemporalZonedDateTimeConstructor::create(vm, global_object, constructor_structure, &prototype);

    prototype.put_direct(vm, &PropertyName::from_identifier(&vm.property_names.constructor), constructor.as_value(), DONT_ENUM);
    temporal.put_direct(
        vm,
        &PropertyName::from_identifier(&Identifier::from_span(vm, b"ZonedDateTime".as_slice())),
        constructor.as_value(),
        DONT_ENUM,
    );
}
