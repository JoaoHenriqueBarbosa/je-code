//! Porte de `runtime/TemporalPlainMonthDayConstructor.{h,cpp}`: o construtor `Temporal.PlainMonthDay` (um
//! `InternalFunction`, comprimento 2) com `from` (não há `compare`), e `install_plain_month_day`, que cria o
//! protótipo, a estrutura intrínseca (`m_plainMonthDayStructure`) e põe o construtor no `Temporal` (a entrada
//! `PlainMonthDay` de `temporalObjectTable`).
//!
//! DIVERGÊNCIA: o `LazyClassStructure` do C++ cria tudo na primeira leitura de `Temporal.PlainMonthDay`; aqui é
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
use crate::runtime::iso8601::{is_date_time_within_limits, is_valid_iso_date, PlainDate, MAX_YEAR, MIN_YEAR};
use crate::runtime::js_global_object::JSGlobalObject;
use crate::runtime::js_object::{JSObject, JSObjectRef};
use crate::runtime::js_value::JSValue;
use crate::runtime::property_attribute::DONT_ENUM;
use crate::runtime::property_name::PropertyName;
use crate::runtime::structure::StructureRef;
use crate::runtime::temporal_calendar::{is_builtin_calendar, CalendarID, ISO8601_CALENDAR_ID};
use crate::runtime::temporal_core_calendar_fields::ISO_MONTH_DAY_REFERENCE_LEAP_YEAR;
use crate::runtime::temporal_object::{string_units, to_finite_integer_with_truncation};
use crate::runtime::temporal_plain_month_day::{create_temporal_month_day, TemporalPlainMonthDay};
use crate::runtime::temporal_plain_month_day_prototype::TemporalPlainMonthDayPrototype;
use crate::runtime::vm::VM;

/// `const ClassInfo TemporalPlainMonthDayConstructor::s_info` (`"Function"`).
pub static TEMPORAL_PLAIN_MONTH_DAY_CONSTRUCTOR_S_INFO: ClassInfo =
    ClassInfo { class_name: "Function", parent_class: Some(&INTERNAL_FUNCTION_S_INFO), static_prop_hash_table: Some(&CONSTRUCTOR_TABLE), inherits_js_type_range: None };

/// `callTemporalPlainMonthDay`: `throwConstructorCannotBeCalledAsFunctionTypeError(globalObject, scope, "PlainMonthDay")`.
fn call_temporal_plain_month_day_body(_global_object: &JSGlobalObject, _call: &HostCall) -> HostResult {
    constructor_cannot_be_called_as_function("PlainMonthDay")
}

/// `constructTemporalPlainMonthDay`: https://tc39.es/proposal-temporal/#sec-temporal.plainmonthday
fn construct_temporal_plain_month_day_body(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    // Passo 1: `NewTarget` `undefined` é tratado pela divisão chamada/construção (ver `callTemporalPlainMonthDay`).

    // Passos 3 e 4: `ToIntegerWithTruncation(isoMonth)` e `(isoDay)`, com o teste de finitude explícito.
    let iso_month = to_finite_integer_with_truncation(global_object, call.argument(0), "Temporal.PlainMonthDay month property must be finite")?;
    let iso_day = to_finite_integer_with_truncation(global_object, call.argument(1), "Temporal.PlainMonthDay day property must be finite")?;

    // Passos 5 a 7: `calendar` `undefined` é `"iso8601"`; não `String` é `TypeError`; `CanonicalizeCalendar`.
    let mut calendar_id: CalendarID = ISO8601_CALENDAR_ID;
    let calendar_argument = call.argument(2);
    if !calendar_argument.is_undefined() {
        if !calendar_argument.is_string() {
            return Err(Thrown::type_error("calendar must be a string"));
        }
        let raw_calendar_id = string_units(global_object, calendar_argument)?;
        calendar_id = is_builtin_calendar(&raw_calendar_id).ok_or_else(|| Thrown::range_error("invalid calendar ID"))?;
    }

    // Passos 2 e 8: `referenceISOYear` `undefined` é 1972; senão `ToIntegerWithTruncation`.
    let mut reference_year = f64::from(ISO_MONTH_DAY_REFERENCE_LEAP_YEAR);
    let reference_year_argument = call.argument(3);
    if !reference_year_argument.is_undefined() {
        reference_year =
            to_finite_integer_with_truncation(global_object, reference_year_argument, "Temporal.PlainMonthDay reference year must be finite")?;
    }

    // Passo 9: `IsValidISODate(y, m, d)` falso é `RangeError` (e o ano e a data fora da faixa do ECMAScript).
    const OUT_OF_RANGE: &str = "PlainMonthDay: date out of range of ECMAScript representation";
    if !(reference_year >= f64::from(MIN_YEAR) && reference_year <= f64::from(MAX_YEAR)) || !is_valid_iso_date(reference_year, iso_month, iso_day) {
        return Err(Thrown::range_error(OUT_OF_RANGE));
    }
    if !is_date_time_within_limits(reference_year as i32, iso_month as u8, iso_day as u8, 12, 0, 0, 0, 0, 0) {
        return Err(Thrown::range_error(OUT_OF_RANGE));
    }

    // Passos 10 e 11: `CreateTemporalMonthDay(isoDate, calendar, NewTarget)` (o `ISODateWithinLimits` do passo 10 é
    // refeito lá).
    let plain_date = PlainDate::new(reference_year as i64, iso_month as u32, iso_day as u32);
    Ok(create_temporal_month_day(global_object, plain_date, calendar_id, Some((call.new_target(), call.callee())))?.as_value())
}

/// `temporalPlainMonthDayConstructorFuncFrom`: https://tc39.es/proposal-temporal/#sec-temporal.plainmonthday.from
fn temporal_plain_month_day_constructor_func_from_body(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    // Passo 1: `ToTemporalMonthDay(item, options)`.
    Ok(TemporalPlainMonthDay::from(global_object, call.argument(0), call.argument(1))?.as_value())
}

host_function!(call_temporal_plain_month_day, call_temporal_plain_month_day_body);
host_function!(construct_temporal_plain_month_day, construct_temporal_plain_month_day_body);
host_function!(temporal_plain_month_day_constructor_func_from, temporal_plain_month_day_constructor_func_from_body);

/// `temporalPlainMonthDayConstructorTableValues`, na ordem do `@begin`.
static CONSTRUCTOR_TABLE_VALUES: [HashTableValue; 1] = [
    native_entry("from", temporal_plain_month_day_constructor_func_from, 1),
];

/// `temporalPlainMonthDayConstructorTable`.
static CONSTRUCTOR_TABLE: HashTable = HashTable { class_for_this: None, values: &CONSTRUCTOR_TABLE_VALUES };

/// `class TemporalPlainMonthDayConstructor final : public InternalFunction`: sem campos próprios.
pub struct TemporalPlainMonthDayConstructor;

impl TemporalPlainMonthDayConstructor {
    /// `createStructure(vm, globalObject, prototype)`: `InternalFunctionType`.
    pub fn create_structure(vm: &VM, global_object: &JSGlobalObject, prototype: JSValue) -> StructureRef {
        collection_constructor_structure(vm, global_object, prototype, &TEMPORAL_PLAIN_MONTH_DAY_CONSTRUCTOR_S_INFO)
    }

    /// `create(vm, structure, plainMonthDayPrototype)`: `finishCreation` com comprimento 2, nome `"PlainMonthDay"`,
    /// `prototype` `DontEnum|DontDelete|ReadOnly` e a tabela (`from` com 1, `DontEnum`).
    pub fn create(
        vm: &VM,
        global_object: &JSGlobalObject,
        structure: StructureRef,
        plain_month_day_prototype: &JSObject,
    ) -> InternalFunctionRef {
        create_collection_constructor(
            vm,
            global_object,
            structure,
            plain_month_day_prototype,
            "PlainMonthDay",
            2,
            call_temporal_plain_month_day,
            construct_temporal_plain_month_day,
            false,
        )
    }
}

/// `createPlainMonthDayConstructor` e o `LazyClassStructure` de `Temporal.PlainMonthDay`: cria o protótipo
/// (`didBecomePrototype`), a estrutura intrínseca (`globalObject->plainMonthDayStructure()`), o construtor, o
/// `constructor` do protótipo (`DontEnum`) e a propriedade `PlainMonthDay` do `Temporal` (`DontEnum`).
pub fn install_plain_month_day(global_object: &JSGlobalObject, temporal: &JSObject, object_prototype: &JSObjectRef) {
    let vm = global_object.vm();
    let function_prototype = global_object.function_prototype();

    let prototype_structure = TemporalPlainMonthDayPrototype::create_structure(vm, global_object, object_prototype.as_value());
    let prototype = TemporalPlainMonthDayPrototype::create(vm, global_object, &prototype_structure);
    prototype.did_become_prototype(vm);

    let plain_month_day_structure = TemporalPlainMonthDay::create_structure(vm, global_object, prototype.as_value());
    global_object.temporal_data.borrow_mut().plain_month_day_structure = Some(plain_month_day_structure);

    let constructor_structure = TemporalPlainMonthDayConstructor::create_structure(vm, global_object, function_prototype.as_value());
    let constructor = TemporalPlainMonthDayConstructor::create(vm, global_object, constructor_structure, &prototype);

    prototype.put_direct(vm, &PropertyName::from_identifier(&vm.property_names.constructor), constructor.as_value(), DONT_ENUM);
    temporal.put_direct(
        vm,
        &PropertyName::from_identifier(&Identifier::from_span(vm, b"PlainMonthDay".as_slice())),
        constructor.as_value(),
        DONT_ENUM,
    );
}
