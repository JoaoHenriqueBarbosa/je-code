//! Porte de `runtime/TemporalDurationConstructor.{h,cpp}`: o construtor `Temporal.Duration` (um
//! `InternalFunction`, comprimento 0) com `from` e `compare`, e `install_duration`, que cria o protótipo, a
//! estrutura intrínseca (`m_durationStructure`) e põe o construtor no `Temporal` (a entrada `Duration` de
//! `temporalObjectTable`).
//!
//! DIVERGÊNCIA: o `LazyClassStructure` do C++ cria tudo na primeira leitura de `Temporal.Duration` (ou de
//! `globalObject->durationStructure()`); aqui é eager, junto com o `Temporal` (ver a DIVERGÊNCIA de
//! `temporal_object.rs`).

use crate::host_function;
use crate::runtime::class_info::ClassInfo;
use crate::runtime::collection_support::{
    collection_constructor_structure, constructor_cannot_be_called_as_function, create_collection_constructor,
};
use crate::runtime::host_call::{HostCall, HostResult, Thrown};
use crate::runtime::identifier::Identifier;
use crate::runtime::intl_support::to_number_checked;
use crate::runtime::intrinsic::Intrinsic;
use crate::runtime::internal_function::{InternalFunctionRef, INTERNAL_FUNCTION_S_INFO};
use crate::runtime::iso8601::Duration;
use crate::runtime::js_global_object::JSGlobalObject;
use crate::runtime::js_object::{JSObject, JSObjectRef};
use crate::runtime::js_value::JSValue;
use crate::runtime::math_common::is_integer;
use crate::runtime::lookup::{HashTable, HashTableValue, Kind};
use crate::runtime::property_attribute::{DONT_ENUM, FUNCTION};
use crate::runtime::property_name::PropertyName;
use crate::runtime::structure::StructureRef;
use crate::runtime::temporal_duration::{create_temporal_duration, TemporalDuration};
use crate::runtime::temporal_duration_prototype::TemporalDurationPrototype;
use crate::runtime::temporal_object::{TemporalUnit, NUMBER_OF_TEMPORAL_UNITS};
use crate::runtime::vm::VM;

/// `const ClassInfo TemporalDurationConstructor::s_info` (`"Function"`).
pub static TEMPORAL_DURATION_CONSTRUCTOR_S_INFO: ClassInfo =
    ClassInfo {
        class_name: "Function",
        parent_class: Some(&INTERNAL_FUNCTION_S_INFO),
        static_prop_hash_table: Some(&TEMPORAL_DURATION_CONSTRUCTOR_TABLE),
        inherits_js_type_range: None,
    };

/// `callTemporalDuration`: `throwConstructorCannotBeCalledAsFunctionTypeError(globalObject, scope, "Duration")`.
fn call_temporal_duration_body(_global_object: &JSGlobalObject, _call: &HostCall) -> HostResult {
    constructor_cannot_be_called_as_function("Duration")
}

/// `constructTemporalDuration`: https://tc39.es/proposal-temporal/#sec-temporal.duration
fn construct_temporal_duration_body(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    // Passo 1: `NewTarget` `undefined` é tratado pela divisão chamada/construção (ver `callTemporalDuration`).

    // Passos 2 a 11: para cada unidade (`years` a `nanoseconds`), `undefined` vira 0 e o resto passa por
    // `ToIntegerIfIntegral`; o `+ 0.0` normaliza -0 para +0 (passo 4).
    let mut result = Duration::default();
    let count = call.argument_count().min(NUMBER_OF_TEMPORAL_UNITS);
    for (index, unit) in TemporalUnit::ALL.into_iter().enumerate().take(count) {
        let value = call.argument(index);
        if value.is_undefined() {
            continue;
        }

        let v = to_number_checked(global_object, value)? + 0.0;
        if !is_integer(v) {
            return Err(Thrown::range_error("Temporal.Duration properties must be integers"));
        }
        result.set_field(unit, v);
    }

    // Passo 12: `CreateTemporalDuration(..., NewTarget)`.
    Ok(create_temporal_duration(global_object, result, Some((call.new_target(), call.callee())))?.as_value())
}

/// `temporalDurationConstructorFuncFrom`: https://tc39.es/proposal-temporal/#sec-temporal.duration.from
fn temporal_duration_constructor_func_from_body(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    // Passo 1: `ToTemporalDuration(item)`.
    Ok(TemporalDuration::from(global_object, call.argument(0))?.as_value())
}

/// `temporalDurationConstructorFuncCompare`: https://tc39.es/proposal-temporal/#sec-temporal.duration.compare
fn temporal_duration_constructor_func_compare_body(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    // Passos 1 e 2: `ToTemporalDuration(one)` e `(two)`. Passo 3: `CompareTemporalDuration`.
    TemporalDuration::compare(global_object, call.argument(0), call.argument(1), call.argument(2))
}

host_function!(call_temporal_duration, call_temporal_duration_body);
host_function!(construct_temporal_duration, construct_temporal_duration_body);
host_function!(temporal_duration_constructor_func_from, temporal_duration_constructor_func_from_body);
host_function!(temporal_duration_constructor_func_compare, temporal_duration_constructor_func_compare_body);

/// `temporalDurationConstructorTableValues`, na ordem do `@begin`: `from` (1) e `compare` (2), `DontEnum|Function`.
static TEMPORAL_DURATION_CONSTRUCTOR_TABLE_VALUES: [HashTableValue; 2] = [
    HashTableValue {
        key: "from",
        attributes: DONT_ENUM | FUNCTION,
        intrinsic: Intrinsic::NoIntrinsic,
        kind: Kind::NativeFunction { function: temporal_duration_constructor_func_from, length: 1 },
    },
    HashTableValue {
        key: "compare",
        attributes: DONT_ENUM | FUNCTION,
        intrinsic: Intrinsic::NoIntrinsic,
        kind: Kind::NativeFunction { function: temporal_duration_constructor_func_compare, length: 2 },
    },
];

/// `temporalDurationConstructorTable`.
static TEMPORAL_DURATION_CONSTRUCTOR_TABLE: HashTable =
    HashTable { class_for_this: None, values: &TEMPORAL_DURATION_CONSTRUCTOR_TABLE_VALUES };

/// `class TemporalDurationConstructor final : public InternalFunction`: sem campos próprios.
pub struct TemporalDurationConstructor;

impl TemporalDurationConstructor {
    /// `createStructure(vm, globalObject, prototype)`: `InternalFunctionType`.
    pub fn create_structure(vm: &VM, global_object: &JSGlobalObject, prototype: JSValue) -> StructureRef {
        collection_constructor_structure(vm, global_object, prototype, &TEMPORAL_DURATION_CONSTRUCTOR_S_INFO)
    }

    /// `create(vm, structure, durationPrototype)`: `finishCreation` com comprimento 0, nome `"Duration"` e
    /// `prototype` `DontEnum|DontDelete|ReadOnly`; `from` e `compare` reificam no primeiro acesso.
    pub fn create(
        vm: &VM,
        global_object: &JSGlobalObject,
        structure: StructureRef,
        duration_prototype: &JSObject,
    ) -> InternalFunctionRef {
        create_collection_constructor(
            vm,
            global_object,
            structure,
            duration_prototype,
            "Duration",
            0,
            call_temporal_duration,
            construct_temporal_duration,
            false,
        )
    }
}

/// `createDurationConstructor` e o `LazyClassStructure` de `Temporal.Duration`: cria o protótipo
/// (`didBecomePrototype`), a estrutura intrínseca (`globalObject->durationStructure()`), o construtor, o
/// `constructor` do protótipo (`DontEnum`) e a propriedade `Duration` do `Temporal` (`DontEnum`).
pub fn install_duration(global_object: &JSGlobalObject, temporal: &JSObject, object_prototype: &JSObjectRef) {
    let vm = global_object.vm();
    let function_prototype = global_object.function_prototype();

    let prototype_structure = TemporalDurationPrototype::create_structure(vm, global_object, object_prototype.as_value());
    let prototype = TemporalDurationPrototype::create(vm, global_object, &prototype_structure);
    prototype.did_become_prototype(vm);

    let duration_structure = TemporalDuration::create_structure(vm, Some(global_object), prototype.as_value());
    global_object.temporal_data.borrow_mut().duration_structure = Some(duration_structure);

    let constructor_structure = TemporalDurationConstructor::create_structure(vm, global_object, function_prototype.as_value());
    let constructor = TemporalDurationConstructor::create(vm, global_object, constructor_structure, &prototype);

    prototype.put_direct(vm, &PropertyName::from_identifier(&vm.property_names.constructor), constructor.as_value(), DONT_ENUM);
    temporal.put_direct(vm, &PropertyName::from_identifier(&Identifier::from_span(vm, b"Duration".as_slice())), constructor.as_value(), DONT_ENUM);
}
