//! Porte de `runtime/TemporalPlainTimeConstructor.{h,cpp}`: o construtor `Temporal.PlainTime` (um
//! `InternalFunction`, comprimento 0) com `from` e `compare`, e `install_plain_time`, que cria o protótipo, a
//! estrutura intrínseca (`m_plainTimeStructure`) e põe o construtor no `Temporal` (a entrada `PlainTime` de
//! `temporalObjectTable`).
//!
//! DIVERGÊNCIA: o `LazyClassStructure` do C++ cria tudo na primeira leitura de `Temporal.PlainTime` (ou de
//! `globalObject->plainTimeStructure()`); aqui é eager, junto com o `Temporal` (ver a DIVERGÊNCIA de
//! `temporal_object.rs`).

use crate::host_function;
use crate::runtime::class_info::ClassInfo;
use crate::runtime::collection_support::{
    collection_constructor_structure, constructor_cannot_be_called_as_function, create_collection_constructor,
};
use crate::runtime::lookup::{HashTable, HashTableValue};
use crate::runtime::lookup::{native_entry};
use crate::runtime::host_call::{HostCall, HostResult, Thrown};
use crate::runtime::identifier::Identifier;
use crate::runtime::intl_support::to_number_checked;
use crate::runtime::internal_function::{InternalFunctionRef, INTERNAL_FUNCTION_S_INFO};
use crate::runtime::iso8601::Duration;
use crate::runtime::js_global_object::JSGlobalObject;
use crate::runtime::js_object::{JSObject, JSObjectRef};
use crate::runtime::js_value::{js_number, js_undefined, JSValue};
use crate::runtime::property_attribute::DONT_ENUM;
use crate::runtime::property_name::PropertyName;
use crate::runtime::structure::StructureRef;
use crate::runtime::temporal_object::TemporalUnit;
use crate::runtime::temporal_plain_time::{
    create_temporal_time, validate_and_create_time_record, TemporalPlainTime, NUMBER_OF_TEMPORAL_PLAIN_TIME_UNITS,
};
use crate::runtime::temporal_plain_time_prototype::TemporalPlainTimePrototype;
use crate::runtime::vm::VM;

/// `const ClassInfo TemporalPlainTimeConstructor::s_info` (`"Function"`).
pub static TEMPORAL_PLAIN_TIME_CONSTRUCTOR_S_INFO: ClassInfo =
    ClassInfo { class_name: "Function", parent_class: Some(&INTERNAL_FUNCTION_S_INFO), static_prop_hash_table: Some(&CONSTRUCTOR_TABLE), inherits_js_type_range: None };

/// `callTemporalPlainTime`: `throwConstructorCannotBeCalledAsFunctionTypeError(globalObject, scope, "PlainTime")`.
fn call_temporal_plain_time_body(_global_object: &JSGlobalObject, _call: &HostCall) -> HostResult {
    constructor_cannot_be_called_as_function("PlainTime")
}

/// `constructTemporalPlainTime`: https://tc39.es/proposal-temporal/#sec-temporal.plaintime
/// `Temporal.PlainTime ( [ hour [ , minute [ , second [ , millisecond [ , microsecond [ , nanosecond ] ] ] ] ] ] )`
fn construct_temporal_plain_time_body(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    // Passo 1: `NewTarget` `undefined` é tratado pela divisão chamada/construção (ver `callTemporalPlainTime`).

    // Passos 2 a 7: cada argumento `undefined` vale 0; o resto passa por `ToIntegerWithTruncation`.
    let mut duration = Duration::default();
    let count = call.argument_count().min(NUMBER_OF_TEMPORAL_PLAIN_TIME_UNITS);
    for index in 0..count {
        let unit = TemporalUnit::ALL[index + TemporalUnit::Hour as usize];
        let argument = call.argument(index);
        if argument.is_undefined() {
            duration.set_field(unit, 0.0);
            continue;
        }
        let value = to_number_checked(global_object, argument)?;
        if value.is_nan() {
            return Err(Thrown::range_error("Temporal.PlainTime argument must not be NaN"));
        }
        if !value.is_finite() {
            return Err(Thrown::range_error("Temporal.PlainTime properties must be finite"));
        }
        duration.set_field(unit, value.trunc());
    }

    // Passos 8 e 9: `IsValidTime` e `CreateTimeRecord`, antes do passo 10 para o `Get(NewTarget, "prototype")`
    // observável de `CreateTemporalTime` não rodar antes da validação.
    let plain_time = validate_and_create_time_record(&duration)?;

    // Passo 10: `CreateTemporalTime(time, NewTarget)`.
    Ok(create_temporal_time(global_object, plain_time, Some((call.new_target(), call.callee())))?.as_value())
}

/// `temporalPlainTimeConstructorFuncFrom`: https://tc39.es/proposal-temporal/#sec-temporal.plaintime.from
fn temporal_plain_time_constructor_func_from_body(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    // Passo 1: `ToTemporalTime(item, options)`.
    Ok(TemporalPlainTime::from(global_object, call.argument(0), call.argument(1))?.as_value())
}

/// `temporalPlainTimeConstructorFuncCompare`: https://tc39.es/proposal-temporal/#sec-temporal.plaintime.compare
fn temporal_plain_time_constructor_func_compare_body(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    // Passos 1 e 2: `ToTemporalTime(one)` e `ToTemporalTime(two)`.
    let one = TemporalPlainTime::from(global_object, call.argument(0), js_undefined())?;
    let two = TemporalPlainTime::from(global_object, call.argument(1), js_undefined())?;

    // Passo 3: `CompareTimeRecord(one.[[Time]], two.[[Time]])`.
    Ok(js_number(TemporalPlainTime::compare(one.plain_time(), two.plain_time())))
}

host_function!(call_temporal_plain_time, call_temporal_plain_time_body);
host_function!(construct_temporal_plain_time, construct_temporal_plain_time_body);
host_function!(temporal_plain_time_constructor_func_from, temporal_plain_time_constructor_func_from_body);
host_function!(temporal_plain_time_constructor_func_compare, temporal_plain_time_constructor_func_compare_body);

/// `temporalPlainTimeConstructorTableValues`, na ordem do `@begin`.
static CONSTRUCTOR_TABLE_VALUES: [HashTableValue; 2] = [
    native_entry("from", temporal_plain_time_constructor_func_from, 1),
    native_entry("compare", temporal_plain_time_constructor_func_compare, 2),
];

/// `temporalPlainTimeConstructorTable`.
static CONSTRUCTOR_TABLE: HashTable = HashTable { class_for_this: None, values: &CONSTRUCTOR_TABLE_VALUES };

/// `class TemporalPlainTimeConstructor final : public InternalFunction`: sem campos próprios.
pub struct TemporalPlainTimeConstructor;

impl TemporalPlainTimeConstructor {
    /// `createStructure(vm, globalObject, prototype)`: `InternalFunctionType`.
    pub fn create_structure(vm: &VM, global_object: &JSGlobalObject, prototype: JSValue) -> StructureRef {
        collection_constructor_structure(vm, global_object, prototype, &TEMPORAL_PLAIN_TIME_CONSTRUCTOR_S_INFO)
    }

    /// `create(vm, structure, plainTimePrototype)`: `finishCreation` com comprimento 0, nome `"PlainTime"`,
    /// `prototype` `DontEnum|DontDelete|ReadOnly` e a tabela (`from` com 1, `compare` com 2, `DontEnum`).
    pub fn create(
        vm: &VM,
        global_object: &JSGlobalObject,
        structure: StructureRef,
        plain_time_prototype: &JSObject,
    ) -> InternalFunctionRef {
        create_collection_constructor(
            vm,
            global_object,
            structure,
            plain_time_prototype,
            "PlainTime",
            0,
            call_temporal_plain_time,
            construct_temporal_plain_time,
            false,
        )
    }
}

/// `createPlainTimeConstructor` e o `LazyClassStructure` de `Temporal.PlainTime`: cria o protótipo
/// (`didBecomePrototype`), a estrutura intrínseca (`globalObject->plainTimeStructure()`), o construtor, o
/// `constructor` do protótipo (`DontEnum`) e a propriedade `PlainTime` do `Temporal` (`DontEnum`).
pub fn install_plain_time(global_object: &JSGlobalObject, temporal: &JSObject, object_prototype: &JSObjectRef) {
    let vm = global_object.vm();
    let function_prototype = global_object.function_prototype();

    let prototype_structure = TemporalPlainTimePrototype::create_structure(vm, global_object, object_prototype.as_value());
    let prototype = TemporalPlainTimePrototype::create(vm, global_object, &prototype_structure);
    prototype.did_become_prototype(vm);

    let plain_time_structure = TemporalPlainTime::create_structure(vm, Some(global_object), prototype.as_value());
    global_object.temporal_data.borrow_mut().plain_time_structure = Some(plain_time_structure);

    let constructor_structure = TemporalPlainTimeConstructor::create_structure(vm, global_object, function_prototype.as_value());
    let constructor = TemporalPlainTimeConstructor::create(vm, global_object, constructor_structure, &prototype);

    prototype.put_direct(vm, &PropertyName::from_identifier(&vm.property_names.constructor), constructor.as_value(), DONT_ENUM);
    temporal.put_direct(vm, &PropertyName::from_identifier(&Identifier::from_span(vm, b"PlainTime".as_slice())), constructor.as_value(), DONT_ENUM);
}
