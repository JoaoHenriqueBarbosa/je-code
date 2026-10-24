//! Porte de `runtime/TemporalPlainDateConstructor.{h,cpp}`: o construtor `Temporal.PlainDate` (um
//! `InternalFunction`, comprimento 3) com `from` e `compare`, e `install_plain_date`, que cria o protótipo, a
//! estrutura intrínseca (`m_plainDateStructure`) e põe o construtor no `Temporal` (a entrada `PlainDate` de
//! `temporalObjectTable`).
//!
//! DIVERGÊNCIAS:
//! - O `LazyClassStructure` do C++ cria tudo na primeira leitura de `Temporal.PlainDate`; aqui é eager, junto
//!   com o `Temporal` (ver a DIVERGÊNCIA de `temporal_object.rs`).
//! - `temporalPlainDateConstructorFuncFrom` trata `PlainDate` antes de chamar `TemporalPlainDate::from`, que faz a
//!   mesma conferência de opções e a mesma cópia: aqui só `from` existe (sem o desvio repetido).

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
use crate::runtime::js_global_object::JSGlobalObject;
use crate::runtime::js_object::{JSObject, JSObjectRef};
use crate::runtime::js_value::{js_number, JSValue};
use crate::runtime::property_attribute::DONT_ENUM;
use crate::runtime::property_name::PropertyName;
use crate::runtime::structure::StructureRef;
use crate::runtime::temporal_calendar::{is_builtin_calendar, CalendarID, ISO8601_CALENDAR_ID};
use crate::runtime::temporal_core_iso_date::iso_date_compare;
use crate::runtime::temporal_object::{string_units, to_integer_with_truncation};
use crate::runtime::temporal_plain_date::{create_temporal_date, validate_and_create_iso_date_record, TemporalPlainDate};
use crate::runtime::temporal_plain_date_prototype::TemporalPlainDatePrototype;
use crate::runtime::vm::VM;

/// `const ClassInfo TemporalPlainDateConstructor::s_info` (`"Function"`).
pub static TEMPORAL_PLAIN_DATE_CONSTRUCTOR_S_INFO: ClassInfo =
    ClassInfo { class_name: "Function", parent_class: Some(&INTERNAL_FUNCTION_S_INFO), static_prop_hash_table: Some(&CONSTRUCTOR_TABLE), inherits_js_type_range: None };

/// `callTemporalPlainDate`: `throwConstructorCannotBeCalledAsFunctionTypeError(globalObject, scope, "PlainDate")`.
fn call_temporal_plain_date_body(_global_object: &JSGlobalObject, _call: &HostCall) -> HostResult {
    constructor_cannot_be_called_as_function("PlainDate")
}

/// O argumento `index` de `new Temporal.PlainDate(...)` como inteiro finito (`ToIntegerWithTruncation`). Argumento
/// ausente fica em zero, sem conversão: o `IsValidISODate` seguinte recusa o mês zero, como pede a spec.
fn integer_argument(global_object: &JSGlobalObject, call: &HostCall, index: usize, name: &str) -> Result<f64, Thrown> {
    if call.argument_count() <= index {
        return Ok(0.0);
    }
    let value = to_integer_with_truncation(global_object, call.argument(index))?;
    if !value.is_finite() {
        return Err(Thrown::RangeError(format!("Temporal.PlainDate {name} property must be finite")));
    }
    Ok(value)
}

/// `constructTemporalPlainDate`: https://tc39.es/proposal-temporal/#sec-temporal.plaindate
fn construct_temporal_plain_date_body(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    // Passo 1: `NewTarget` `undefined` é tratado pela divisão chamada/construção (ver `callTemporalPlainDate`).

    // Passos 2 a 4: `isoYear`, `isoMonth` e `isoDay` por `ToIntegerWithTruncation` (`NaN` e infinito lançam
    // `RangeError`, na ordem year, month, day).
    let year = integer_argument(global_object, call, 0, "year")?;
    let month = integer_argument(global_object, call, 1, "month")?;
    let day = integer_argument(global_object, call, 2, "day")?;

    // Passos 5 a 7: `calendar` `undefined` é `"iso8601"`; não `String` é `TypeError`; `CanonicalizeCalendar`.
    let mut calendar_id: CalendarID = ISO8601_CALENDAR_ID;
    if call.argument_count() > 3 {
        let calendar_argument = call.argument(3);
        if !calendar_argument.is_undefined() {
            if !calendar_argument.is_string() {
                return Err(Thrown::type_error("calendarId must be a string"));
            }
            let raw_calendar_id = string_units(global_object, calendar_argument)?;
            calendar_id = is_builtin_calendar(&raw_calendar_id).ok_or_else(|| Thrown::range_error("invalid calendar ID"))?;
        }
    }

    // Passos 8 e 9: `IsValidISODate` e `CreateISODateRecord`.
    let plain_date = validate_and_create_iso_date_record(year, month, day)?;
    // Passo 10: `CreateTemporalDate(isoDate, calendar, NewTarget)`.
    Ok(create_temporal_date(global_object, plain_date, calendar_id, Some((call.new_target(), call.callee())))?.as_value())
}

/// `temporalPlainDateConstructorFuncFrom`: https://tc39.es/proposal-temporal/#sec-temporal.plaindate.from
fn temporal_plain_date_constructor_func_from_body(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    // Passo 1: `ToTemporalDate(item, options)`.
    Ok(TemporalPlainDate::from(global_object, call.argument(0), call.argument(1))?.as_value())
}

/// `temporalPlainDateConstructorFuncCompare`: https://tc39.es/proposal-temporal/#sec-temporal.plaindate.compare
fn temporal_plain_date_constructor_func_compare_body(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    // Passos 1 e 2: `ToTemporalDate(one)` e `ToTemporalDate(two)`.
    let one = TemporalPlainDate::from(global_object, call.argument(0), JSValue::Undefined)?;
    let two = TemporalPlainDate::from(global_object, call.argument(1), JSValue::Undefined)?;
    // Passo 3: `CompareISODate(one.[[ISODate]], two.[[ISODate]])`.
    Ok(js_number(iso_date_compare(one.plain_date(), two.plain_date())))
}

host_function!(call_temporal_plain_date, call_temporal_plain_date_body);
host_function!(construct_temporal_plain_date, construct_temporal_plain_date_body);
host_function!(temporal_plain_date_constructor_func_from, temporal_plain_date_constructor_func_from_body);
host_function!(temporal_plain_date_constructor_func_compare, temporal_plain_date_constructor_func_compare_body);

/// `temporalPlainDateConstructorTableValues`, na ordem do `@begin`.
static CONSTRUCTOR_TABLE_VALUES: [HashTableValue; 2] = [
    native_entry("from", temporal_plain_date_constructor_func_from, 1),
    native_entry("compare", temporal_plain_date_constructor_func_compare, 2),
];

/// `temporalPlainDateConstructorTable`.
static CONSTRUCTOR_TABLE: HashTable = HashTable { class_for_this: None, values: &CONSTRUCTOR_TABLE_VALUES };

/// `class TemporalPlainDateConstructor final : public InternalFunction`: sem campos próprios.
pub struct TemporalPlainDateConstructor;

impl TemporalPlainDateConstructor {
    /// `createStructure(vm, globalObject, prototype)`: `InternalFunctionType`.
    pub fn create_structure(vm: &VM, global_object: &JSGlobalObject, prototype: JSValue) -> StructureRef {
        collection_constructor_structure(vm, global_object, prototype, &TEMPORAL_PLAIN_DATE_CONSTRUCTOR_S_INFO)
    }

    /// `create(vm, structure, plainDatePrototype)`: `finishCreation` com comprimento 3, nome `"PlainDate"`,
    /// `prototype` `DontEnum|DontDelete|ReadOnly` e a tabela (`from` com 1, `compare` com 2, `DontEnum`).
    pub fn create(
        vm: &VM,
        global_object: &JSGlobalObject,
        structure: StructureRef,
        plain_date_prototype: &JSObject,
    ) -> InternalFunctionRef {
        create_collection_constructor(
            vm,
            global_object,
            structure,
            plain_date_prototype,
            "PlainDate",
            3,
            call_temporal_plain_date,
            construct_temporal_plain_date,
            false,
        )
    }
}

/// `createPlainDateConstructor` e o `LazyClassStructure` de `Temporal.PlainDate`: cria o protótipo
/// (`didBecomePrototype`), a estrutura intrínseca (`globalObject->plainDateStructure()`), o construtor, o
/// `constructor` do protótipo (`DontEnum`) e a propriedade `PlainDate` do `Temporal` (`DontEnum`).
pub fn install_plain_date(global_object: &JSGlobalObject, temporal: &JSObject, object_prototype: &JSObjectRef) {
    let vm = global_object.vm();
    let function_prototype = global_object.function_prototype();

    let prototype_structure = TemporalPlainDatePrototype::create_structure(vm, global_object, object_prototype.as_value());
    let prototype = TemporalPlainDatePrototype::create(vm, global_object, &prototype_structure);
    prototype.did_become_prototype(vm);

    let plain_date_structure = TemporalPlainDate::create_structure(vm, global_object, prototype.as_value());
    global_object.temporal_data.borrow_mut().plain_date_structure = Some(plain_date_structure);

    let constructor_structure = TemporalPlainDateConstructor::create_structure(vm, global_object, function_prototype.as_value());
    let constructor = TemporalPlainDateConstructor::create(vm, global_object, constructor_structure, &prototype);

    prototype.put_direct(vm, &PropertyName::from_identifier(&vm.property_names.constructor), constructor.as_value(), DONT_ENUM);
    temporal.put_direct(vm, &PropertyName::from_identifier(&Identifier::from_span(vm, b"PlainDate".as_slice())), constructor.as_value(), DONT_ENUM);
}
