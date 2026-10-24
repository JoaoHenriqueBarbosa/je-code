//! Porte de `runtime/TemporalPlainDateTimeConstructor.{h,cpp}`: o construtor `Temporal.PlainDateTime` (um
//! `InternalFunction`, comprimento 3) com `from` e `compare`, e `install_plain_date_time`, que cria o protótipo,
//! a estrutura intrínseca (`m_plainDateTimeStructure`) e põe o construtor no `Temporal` (a entrada
//! `PlainDateTime` de `temporalObjectTable`).
//!
//! DIVERGÊNCIA: o `LazyClassStructure` do C++ cria tudo na primeira leitura de `Temporal.PlainDateTime`; aqui é
//! eager, junto com o `Temporal` (ver a DIVERGÊNCIA de `temporal_object.rs`).

use crate::host_function;
use crate::runtime::class_info::ClassInfo;
use crate::runtime::collection_support::{
    collection_constructor_structure, constructor_cannot_be_called_as_function, create_collection_constructor,
};
use crate::runtime::lookup::{HashTable, HashTableValue};
use crate::runtime::lookup::{native_entry};
use crate::runtime::host_call::{HostCall, HostResult, Thrown};
use crate::runtime::identifier::Identifier;
use crate::runtime::internal_function::{InternalFunctionRef, INTERNAL_FUNCTION_S_INFO};
use crate::runtime::iso8601::Duration;
use crate::runtime::js_global_object::JSGlobalObject;
use crate::runtime::js_object::{JSObject, JSObjectRef};
use crate::runtime::js_value::{js_number, JSValue};
use crate::runtime::property_attribute::DONT_ENUM;
use crate::runtime::property_name::PropertyName;
use crate::runtime::structure::StructureRef;
use crate::runtime::temporal_calendar::{is_builtin_calendar, CalendarID, ISO8601_CALENDAR_ID};
use crate::runtime::temporal_core_plain_date_time::compare_iso_date_time;
use crate::runtime::temporal_object::{string_units, to_integer_with_truncation, TemporalUnit};
use crate::runtime::temporal_plain_date::validate_and_create_iso_date_record;
use crate::runtime::temporal_plain_date_time::{create_temporal_date_time, TemporalPlainDateTime};
use crate::runtime::temporal_plain_date_time_prototype::TemporalPlainDateTimePrototype;
use crate::runtime::temporal_plain_time::{validate_and_create_time_record, NUMBER_OF_TEMPORAL_PLAIN_TIME_UNITS as NUMBER_OF_TIME_UNITS};
use crate::runtime::vm::VM;

/// `const ClassInfo TemporalPlainDateTimeConstructor::s_info` (`"Function"`).
pub static TEMPORAL_PLAIN_DATE_TIME_CONSTRUCTOR_S_INFO: ClassInfo =
    ClassInfo { class_name: "Function", parent_class: Some(&INTERNAL_FUNCTION_S_INFO), static_prop_hash_table: Some(&CONSTRUCTOR_TABLE), inherits_js_type_range: None };

/// `numberOfTemporalPlainDateUnits`: ano, mês e dia.
const NUMBER_OF_DATE_UNITS: usize = 3;

/// `callTemporalPlainDateTime`: `throwConstructorCannotBeCalledAsFunctionTypeError(globalObject, scope, "PlainDateTime")`.
fn call_temporal_plain_date_time_body(_global_object: &JSGlobalObject, _call: &HostCall) -> HostResult {
    constructor_cannot_be_called_as_function("PlainDateTime")
}

/// `constructTemporalPlainDateTime`: https://tc39.es/proposal-temporal/#sec-temporal.plaindatetime
fn construct_temporal_plain_date_time_body(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    // Passo 1: `NewTarget` `undefined` é tratado pela divisão chamada/construção (ver `callTemporalPlainDateTime`).

    // Passos 2 a 10: ano, mês e dia por `ToIntegerWithTruncation` (o `undefined` é `NaN`, `RangeError`); os seis
    // campos de hora `undefined` valem 0. Cada valor não finito é `RangeError`. O índice da `Duration` é o da
    // tabela de unidades sem `Week` (a semana ocupa o índice 2 da `Duration`, que o laço do C++ pula).
    let mut fields = Duration::default();
    for index in 0..NUMBER_OF_DATE_UNITS + NUMBER_OF_TIME_UNITS {
        let argument = call.argument(index);
        if index >= NUMBER_OF_DATE_UNITS && argument.is_undefined() {
            continue;
        }
        let value = to_integer_with_truncation(global_object, argument)?;
        if !value.is_finite() {
            return Err(Thrown::range_error("Temporal.PlainDateTime properties must be finite"));
        }
        let duration_index = if index >= TemporalUnit::Week as usize { index + 1 } else { index };
        fields.set_field(TemporalUnit::ALL[duration_index], value);
    }

    // Passos 11 a 13: `calendar` `undefined` é `"iso8601"`; não `String` é `TypeError`; `CanonicalizeCalendar`.
    let mut calendar_id: CalendarID = ISO8601_CALENDAR_ID;
    if call.argument_count() > NUMBER_OF_DATE_UNITS + NUMBER_OF_TIME_UNITS {
        let calendar_argument = call.argument(NUMBER_OF_DATE_UNITS + NUMBER_OF_TIME_UNITS);
        if !calendar_argument.is_undefined() {
            if !calendar_argument.is_string() {
                return Err(Thrown::type_error("calendarId must be a string"));
            }
            let raw_calendar_id = string_units(global_object, calendar_argument)?;
            calendar_id = is_builtin_calendar(&raw_calendar_id).ok_or_else(|| Thrown::range_error("invalid calendar ID"))?;
        }
    }

    // Passos 14 e 15: `IsValidISODate` e `CreateISODateRecord`.
    let plain_date = validate_and_create_iso_date_record(fields.years() as f64, fields.months() as f64, fields.days() as f64)?;
    // Passos 16 e 17: `IsValidTime` e `CreateTimeRecord`.
    let plain_time = validate_and_create_time_record(&fields)?;

    // Passos 18 e 19: `CombineISODateAndTimeRecord` e `CreateTemporalDateTime(isoDateTime, calendar, NewTarget)`.
    Ok(create_temporal_date_time(global_object, plain_date, plain_time, calendar_id, Some((call.new_target(), call.callee())))?.as_value())
}

/// `temporalPlainDateTimeConstructorFuncFrom`: https://tc39.es/proposal-temporal/#sec-temporal.plaindatetime.from
fn temporal_plain_date_time_constructor_func_from_body(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    // Passo 1: `ToTemporalDateTime(item, options)`.
    Ok(TemporalPlainDateTime::from(global_object, call.argument(0), call.argument(1))?.as_value())
}

/// `temporalPlainDateTimeConstructorFuncCompare`: https://tc39.es/proposal-temporal/#sec-temporal.plaindatetime.compare
fn temporal_plain_date_time_constructor_func_compare_body(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    // Passos 1 e 2: `ToTemporalDateTime(one)` e `ToTemporalDateTime(two)`.
    let one = TemporalPlainDateTime::from(global_object, call.argument(0), JSValue::Undefined)?;
    let two = TemporalPlainDateTime::from(global_object, call.argument(1), JSValue::Undefined)?;
    // Passo 3: `CompareISODateTime(one.[[ISODateTime]], two.[[ISODateTime]])`.
    Ok(js_number(compare_iso_date_time(one.plain_date(), one.plain_time(), two.plain_date(), two.plain_time())))
}

host_function!(call_temporal_plain_date_time, call_temporal_plain_date_time_body);
host_function!(construct_temporal_plain_date_time, construct_temporal_plain_date_time_body);
host_function!(temporal_plain_date_time_constructor_func_from, temporal_plain_date_time_constructor_func_from_body);
host_function!(temporal_plain_date_time_constructor_func_compare, temporal_plain_date_time_constructor_func_compare_body);

/// `temporalPlainDateTimeConstructorTableValues`, na ordem do `@begin`.
static CONSTRUCTOR_TABLE_VALUES: [HashTableValue; 2] = [
    native_entry("from", temporal_plain_date_time_constructor_func_from, 1),
    native_entry("compare", temporal_plain_date_time_constructor_func_compare, 2),
];

/// `temporalPlainDateTimeConstructorTable`.
static CONSTRUCTOR_TABLE: HashTable = HashTable { class_for_this: None, values: &CONSTRUCTOR_TABLE_VALUES };

/// `class TemporalPlainDateTimeConstructor final : public InternalFunction`: sem campos próprios.
pub struct TemporalPlainDateTimeConstructor;

impl TemporalPlainDateTimeConstructor {
    /// `createStructure(vm, globalObject, prototype)`: `InternalFunctionType`.
    pub fn create_structure(vm: &VM, global_object: &JSGlobalObject, prototype: JSValue) -> StructureRef {
        collection_constructor_structure(vm, global_object, prototype, &TEMPORAL_PLAIN_DATE_TIME_CONSTRUCTOR_S_INFO)
    }

    /// `create(vm, structure, plainDateTimePrototype)`: `finishCreation` com comprimento 3, nome `"PlainDateTime"`,
    /// `prototype` `DontEnum|DontDelete|ReadOnly` e a tabela (`from` com 1, `compare` com 2, `DontEnum`).
    pub fn create(
        vm: &VM,
        global_object: &JSGlobalObject,
        structure: StructureRef,
        plain_date_time_prototype: &JSObject,
    ) -> InternalFunctionRef {
        create_collection_constructor(
            vm,
            global_object,
            structure,
            plain_date_time_prototype,
            "PlainDateTime",
            3,
            call_temporal_plain_date_time,
            construct_temporal_plain_date_time,
            false,
        )
    }
}

/// O `LazyClassStructure` de `Temporal.PlainDateTime` (`JSGlobalObject.cpp`): cria o protótipo
/// (`didBecomePrototype`), a estrutura intrínseca (`globalObject->plainDateTimeStructure()`), o construtor, o
/// `constructor` do protótipo (`DontEnum`) e a propriedade `PlainDateTime` do `Temporal` (`DontEnum`).
pub fn install_plain_date_time(global_object: &JSGlobalObject, temporal: &JSObject, object_prototype: &JSObjectRef) {
    let vm = global_object.vm();
    let function_prototype = global_object.function_prototype();

    let prototype_structure = TemporalPlainDateTimePrototype::create_structure(vm, global_object, object_prototype.as_value());
    let prototype = TemporalPlainDateTimePrototype::create(vm, global_object, &prototype_structure);
    prototype.did_become_prototype(vm);

    let plain_date_time_structure = TemporalPlainDateTime::create_structure(vm, global_object, prototype.as_value());
    global_object.temporal_data.borrow_mut().plain_date_time_structure = Some(plain_date_time_structure);

    let constructor_structure = TemporalPlainDateTimeConstructor::create_structure(vm, global_object, function_prototype.as_value());
    let constructor = TemporalPlainDateTimeConstructor::create(vm, global_object, constructor_structure, &prototype);

    prototype.put_direct(vm, &PropertyName::from_identifier(&vm.property_names.constructor), constructor.as_value(), DONT_ENUM);
    temporal.put_direct(
        vm,
        &PropertyName::from_identifier(&Identifier::from_span(vm, b"PlainDateTime".as_slice())),
        constructor.as_value(),
        DONT_ENUM,
    );
}
