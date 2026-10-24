//! Porte de `runtime/TemporalPlainYearMonthConstructor.{h,cpp}`: o construtor `Temporal.PlainYearMonth` (um
//! `InternalFunction`, comprimento 2) com `from` e `compare`, e `install_plain_year_month`, que cria o protótipo, a
//! estrutura intrínseca (`m_plainYearMonthStructure`) e põe o construtor no `Temporal` (a entrada `PlainYearMonth`
//! de `temporalObjectTable`).
//!
//! DIVERGÊNCIA: o `LazyClassStructure` do C++ cria tudo na primeira leitura de `Temporal.PlainYearMonth`; aqui é
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
use crate::runtime::iso8601::{is_valid_iso_date, PlainDate, MAX_YEAR, MIN_YEAR};
use crate::runtime::js_global_object::JSGlobalObject;
use crate::runtime::js_object::{JSObject, JSObjectRef};
use crate::runtime::js_value::{js_number, JSValue};
use crate::runtime::property_attribute::DONT_ENUM;
use crate::runtime::property_name::PropertyName;
use crate::runtime::structure::StructureRef;
use crate::runtime::temporal_calendar::{is_builtin_calendar, CalendarID, ISO8601_CALENDAR_ID};
use crate::runtime::temporal_core_iso_date::iso_date_compare;
use crate::runtime::temporal_object::{string_units, to_finite_integer_with_truncation};
use crate::runtime::temporal_plain_year_month::{create_temporal_year_month, TemporalPlainYearMonth};
use crate::runtime::temporal_plain_year_month_prototype::TemporalPlainYearMonthPrototype;
use crate::runtime::vm::VM;

/// `const ClassInfo TemporalPlainYearMonthConstructor::s_info` (`"Function"`).
pub static TEMPORAL_PLAIN_YEAR_MONTH_CONSTRUCTOR_S_INFO: ClassInfo =
    ClassInfo { class_name: "Function", parent_class: Some(&INTERNAL_FUNCTION_S_INFO), static_prop_hash_table: Some(&CONSTRUCTOR_TABLE), inherits_js_type_range: None };

/// `callTemporalPlainYearMonth`: `throwConstructorCannotBeCalledAsFunctionTypeError(globalObject, scope, "PlainYearMonth")`.
fn call_temporal_plain_year_month_body(_global_object: &JSGlobalObject, _call: &HostCall) -> HostResult {
    constructor_cannot_be_called_as_function("PlainYearMonth")
}

/// `constructTemporalPlainYearMonth`: https://tc39.es/proposal-temporal/#sec-temporal.plainyearmonth
fn construct_temporal_plain_year_month_body(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    // Passo 1: `NewTarget` `undefined` é tratado pela divisão chamada/construção (ver `callTemporalPlainYearMonth`).

    // Passos 3 e 4: `ToIntegerWithTruncation(isoYear)` e `(isoMonth)`. O do spec lança `RangeError` com valor não
    // finito e o do JSC não, então o teste de finitude é explícito; argumento ausente é `undefined`, ou seja `NaN`, e
    // lança aqui.
    let iso_year = to_finite_integer_with_truncation(global_object, call.argument(0), "Temporal.PlainYearMonth year property must be finite")?;
    let iso_month = to_finite_integer_with_truncation(global_object, call.argument(1), "Temporal.PlainYearMonth month property must be finite")?;

    // Passos 5 a 7: `calendar` `undefined` é `"iso8601"`; não `String` é `TypeError`; `CanonicalizeCalendar`.
    let mut calendar_id: CalendarID = ISO8601_CALENDAR_ID;
    let calendar_argument = call.argument(2);
    if !calendar_argument.is_undefined() {
        if !calendar_argument.is_string() {
            return Err(Thrown::type_error("calendarId must be a string"));
        }
        let raw_calendar_id = string_units(global_object, calendar_argument)?;
        calendar_id = is_builtin_calendar(&raw_calendar_id).ok_or_else(|| Thrown::range_error("invalid calendar ID"))?;
    }

    // Passos 2 e 8: `referenceISODay` `undefined` é 1; senão `ToIntegerWithTruncation`.
    let mut reference_day = 1.0;
    let reference_day_argument = call.argument(3);
    if !reference_day_argument.is_undefined() {
        reference_day =
            to_finite_integer_with_truncation(global_object, reference_day_argument, "Temporal.PlainYearMonth reference day must be finite")?;
    }

    // Passo 9: `IsValidISODate(y, m, ref)` falso é `RangeError`.
    if !is_valid_iso_date(iso_year, iso_month, reference_day) {
        return Err(Thrown::range_error("Temporal.PlainYearMonth: not a valid ISO date"));
    }
    if !(iso_year >= f64::from(MIN_YEAR) && iso_year <= f64::from(MAX_YEAR)) {
        return Err(Thrown::range_error("year is out of range"));
    }

    // Passos 10 e 11: `CreateISODateRecord(y, m, ref)` e `CreateTemporalYearMonth(isoDate, calendar, NewTarget)`
    // (o mês, em `[1, 12]`, e o dia, em `[1, 31]`, já foram conferidos por `IsValidISODate`).
    let plain_date = PlainDate::new(iso_year as i64, iso_month as u32, reference_day as u32);
    Ok(create_temporal_year_month(global_object, plain_date, calendar_id, Some((call.new_target(), call.callee())))?.as_value())
}

/// `temporalPlainYearMonthConstructorFuncFrom`: https://tc39.es/proposal-temporal/#sec-temporal.plainyearmonth.from
fn temporal_plain_year_month_constructor_func_from_body(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    // Passo 1: `ToTemporalYearMonth(item, options)`.
    Ok(TemporalPlainYearMonth::from(global_object, call.argument(0), call.argument(1))?.as_value())
}

/// `temporalPlainYearMonthConstructorFuncCompare`: https://tc39.es/proposal-temporal/#sec-temporal.plainyearmonth.compare
fn temporal_plain_year_month_constructor_func_compare_body(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    // Passos 1 e 2: `ToTemporalYearMonth(one)` e `ToTemporalYearMonth(two)`.
    let one = TemporalPlainYearMonth::from(global_object, call.argument(0), JSValue::Undefined)?;
    let two = TemporalPlainYearMonth::from(global_object, call.argument(1), JSValue::Undefined)?;
    // Passo 3: `CompareISODate(one.[[ISODate]], two.[[ISODate]])`.
    Ok(js_number(iso_date_compare(*one.plain_year_month().iso_plain_date(), *two.plain_year_month().iso_plain_date())))
}

host_function!(call_temporal_plain_year_month, call_temporal_plain_year_month_body);
host_function!(construct_temporal_plain_year_month, construct_temporal_plain_year_month_body);
host_function!(temporal_plain_year_month_constructor_func_from, temporal_plain_year_month_constructor_func_from_body);
host_function!(temporal_plain_year_month_constructor_func_compare, temporal_plain_year_month_constructor_func_compare_body);

/// `temporalPlainYearMonthConstructorTableValues`, na ordem do `@begin`.
static CONSTRUCTOR_TABLE_VALUES: [HashTableValue; 2] = [
    native_entry("from", temporal_plain_year_month_constructor_func_from, 1),
    native_entry("compare", temporal_plain_year_month_constructor_func_compare, 2),
];

/// `temporalPlainYearMonthConstructorTable`.
static CONSTRUCTOR_TABLE: HashTable = HashTable { class_for_this: None, values: &CONSTRUCTOR_TABLE_VALUES };

/// `class TemporalPlainYearMonthConstructor final : public InternalFunction`: sem campos próprios.
pub struct TemporalPlainYearMonthConstructor;

impl TemporalPlainYearMonthConstructor {
    /// `createStructure(vm, globalObject, prototype)`: `InternalFunctionType`.
    pub fn create_structure(vm: &VM, global_object: &JSGlobalObject, prototype: JSValue) -> StructureRef {
        collection_constructor_structure(vm, global_object, prototype, &TEMPORAL_PLAIN_YEAR_MONTH_CONSTRUCTOR_S_INFO)
    }

    /// `create(vm, structure, plainYearMonthPrototype)`: `finishCreation` com comprimento 2, nome `"PlainYearMonth"`,
    /// `prototype` `DontEnum|DontDelete|ReadOnly` e a tabela (`from` com 1, `compare` com 2, `DontEnum`).
    pub fn create(
        vm: &VM,
        global_object: &JSGlobalObject,
        structure: StructureRef,
        plain_year_month_prototype: &JSObject,
    ) -> InternalFunctionRef {
        create_collection_constructor(
            vm,
            global_object,
            structure,
            plain_year_month_prototype,
            "PlainYearMonth",
            2,
            call_temporal_plain_year_month,
            construct_temporal_plain_year_month,
            false,
        )
    }
}

/// `createPlainYearMonthConstructor` e o `LazyClassStructure` de `Temporal.PlainYearMonth`: cria o protótipo
/// (`didBecomePrototype`), a estrutura intrínseca (`globalObject->plainYearMonthStructure()`), o construtor, o
/// `constructor` do protótipo (`DontEnum`) e a propriedade `PlainYearMonth` do `Temporal` (`DontEnum`).
pub fn install_plain_year_month(global_object: &JSGlobalObject, temporal: &JSObject, object_prototype: &JSObjectRef) {
    let vm = global_object.vm();
    let function_prototype = global_object.function_prototype();

    let prototype_structure = TemporalPlainYearMonthPrototype::create_structure(vm, global_object, object_prototype.as_value());
    let prototype = TemporalPlainYearMonthPrototype::create(vm, global_object, &prototype_structure);
    prototype.did_become_prototype(vm);

    let plain_year_month_structure = TemporalPlainYearMonth::create_structure(vm, global_object, prototype.as_value());
    global_object.temporal_data.borrow_mut().plain_year_month_structure = Some(plain_year_month_structure);

    let constructor_structure = TemporalPlainYearMonthConstructor::create_structure(vm, global_object, function_prototype.as_value());
    let constructor = TemporalPlainYearMonthConstructor::create(vm, global_object, constructor_structure, &prototype);

    prototype.put_direct(vm, &PropertyName::from_identifier(&vm.property_names.constructor), constructor.as_value(), DONT_ENUM);
    temporal.put_direct(
        vm,
        &PropertyName::from_identifier(&Identifier::from_span(vm, b"PlainYearMonth".as_slice())),
        constructor.as_value(),
        DONT_ENUM,
    );
}
