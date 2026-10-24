//! `require("util/types")` do bun 1.4.2: o `NodeUtilTypesModule.cpp`, os 44 predicados na ordem do bun, cada um função
//! nativa de `length` 1. Sem argumento, todos dão `false`; quase todos olham só o `JSType` da célula.

use crate::runtime::bigint_object::BigIntObject;
use crate::runtime::cell_registry::cell_type;
use crate::runtime::host_call::{HostCall, HostResult, Thrown};
use crate::runtime::host_function_support::ObjectRef;
use crate::runtime::js_array_buffer::JSArrayBuffer;
use crate::runtime::js_global_object::JSGlobalObject;
use crate::runtime::js_type::JSType;
use crate::runtime::js_value::{js_boolean, JSValue};
use crate::runtime::native_function::NativeFunction;
use crate::runtime::object_constructor::construct_empty_object;
use crate::runtime::symbol_object::SymbolObject;

/// O `JSType` da célula de `value`; `None` para valor que não é célula.
fn js_type(value: JSValue) -> Option<JSType> {
    match value {
        JSValue::Cell(cell_id) => cell_type(cell_id),
        _ => None,
    }
}

fn type_is(value: JSValue, types: &[JSType]) -> bool {
    js_type(value).is_some_and(|found| types.contains(&found))
}

/// `jsDynamicCast<JSArrayBuffer*>`: `Some(compartilhado)` para um `ArrayBuffer` ou `SharedArrayBuffer`.
fn array_buffer_shared(value: JSValue) -> Option<bool> {
    let JSValue::Cell(cell_id) = value else { return None };
    JSArrayBuffer::from_cell_id(cell_id).map(|buffer| buffer.impl_().is_shared())
}

fn typed_array_range(value: JSValue, last: JSType) -> bool {
    js_type(value).is_some_and(|found| (JSType::Int8ArrayType as u32..=last as u32).contains(&(found as u32)))
}

/// `isAsyncFunction`: função de JavaScript (não de host) que é gerador assíncrono, ou cujo protótipo é o
/// `AsyncFunctionPrototype` (medido no bun: trocar o protótipo de uma função comum para ele dá `true`).
fn is_async_function(global_object: &JSGlobalObject, value: JSValue) -> Result<bool, Thrown> {
    let Some(function) = value.as_js_function().filter(|function| !function.is_host_function()) else { return Ok(false) };
    if function.js_executable().borrow().is_async_generator() {
        return Ok(true);
    }
    let Some(object) = ObjectRef::from_value(&value) else { return Ok(false) };
    let prototype = object.get_prototype(global_object)?;
    Ok(prototype == global_object.async_function_prototype().as_value())
}

fn is_generator_function(value: JSValue) -> bool {
    value.as_js_function().filter(|function| !function.is_host_function()).is_some_and(|function| {
        let executable = function.js_executable();
        let executable = executable.borrow();
        executable.is_generator() || executable.is_async_generator()
    })
}

/// O predicado de `name` sobre `value`.
fn predicate(global_object: &JSGlobalObject, name: &str, value: JSValue) -> Result<bool, Thrown> {
    use JSType::*;
    Ok(match name {
        // `NapiExternal`: o porte não tem N-API.
        "isExternal" => false,
        "isDate" => type_is(value, &[JSDateType]),
        "isArgumentsObject" => type_is(value, &[DirectArgumentsType, ScopedArgumentsType, ClonedArgumentsType]),
        "isBigIntObject" => BigIntObject::from_value(&value).is_some(),
        "isBooleanObject" => type_is(value, &[BooleanObjectType]),
        "isNumberObject" => type_is(value, &[NumberObjectType]),
        "isStringObject" => type_is(value, &[StringObjectType, DerivedStringObjectType]),
        "isSymbolObject" => SymbolObject::from_value(&value).is_some(),
        "isNativeError" => type_is(value, &[ErrorInstanceType]),
        "isRegExp" => type_is(value, &[RegExpObjectType]),
        "isAsyncFunction" => is_async_function(global_object, value)?,
        "isGeneratorFunction" => is_generator_function(value),
        "isGeneratorObject" => type_is(value, &[JSGeneratorType, JSAsyncGeneratorType]),
        "isPromise" => type_is(value, &[JSPromiseType]),
        "isMap" => type_is(value, &[JSMapType]),
        "isSet" => type_is(value, &[JSSetType]),
        "isMapIterator" => type_is(value, &[JSMapIteratorType]),
        "isSetIterator" => type_is(value, &[JSSetIteratorType]),
        "isWeakMap" => type_is(value, &[JSWeakMapType]),
        "isWeakSet" => type_is(value, &[JSWeakSetType]),
        "isArrayBuffer" => array_buffer_shared(value) == Some(false),
        "isDataView" => type_is(value, &[DataViewType]),
        "isSharedArrayBuffer" => array_buffer_shared(value) == Some(true),
        "isProxy" => type_is(value, &[GlobalProxyType, ProxyObjectType]),
        "isModuleNamespaceObject" => type_is(value, &[ModuleNamespaceObjectType]),
        "isAnyArrayBuffer" => array_buffer_shared(value).is_some(),
        "isBoxedPrimitive" => {
            type_is(value, &[BooleanObjectType, NumberObjectType, StringObjectType, DerivedStringObjectType])
                || SymbolObject::from_value(&value).is_some()
                || BigIntObject::from_value(&value).is_some()
        }
        "isArrayBufferView" => typed_array_range(value, DataViewType),
        "isTypedArray" => typed_array_range(value, BigUint64ArrayType),
        "isUint8Array" => type_is(value, &[Uint8ArrayType]),
        "isUint8ClampedArray" => type_is(value, &[Uint8ClampedArrayType]),
        "isUint16Array" => type_is(value, &[Uint16ArrayType]),
        "isUint32Array" => type_is(value, &[Uint32ArrayType]),
        "isInt8Array" => type_is(value, &[Int8ArrayType]),
        "isInt16Array" => type_is(value, &[Int16ArrayType]),
        "isInt32Array" => type_is(value, &[Int32ArrayType]),
        "isFloat16Array" => type_is(value, &[Float16ArrayType]),
        "isFloat32Array" => type_is(value, &[Float32ArrayType]),
        "isFloat64Array" => type_is(value, &[Float64ArrayType]),
        "isBigInt64Array" => type_is(value, &[BigInt64ArrayType]),
        "isBigUint64Array" => type_is(value, &[BigUint64ArrayType]),
        // `JSKeyObject` é do `node:crypto`, que o porte ainda não instala: nenhum valor é um.
        "isKeyObject" => false,
        "isCryptoKey" => crate::runtime::crypto::is_crypto_key(value),
        // O global do bun não herda de `JSEventTarget` (medido: `isEventTarget(globalThis)` é `false`), embora aceite
        // `addEventListener`.
        "isEventTarget" => crate::runtime::event_target::is_target(value) && !type_is(value, &[GlobalObjectType, GlobalProxyType]),
        _ => unreachable!("predicado de util/types fora da tabela: {name}"),
    })
}

/// Uma função nativa por predicado, com o nome dele.
macro_rules! type_predicates {
    ($($function:ident => $name:literal),* $(,)?) => {
        $(
            fn $function(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
                if call.argument_count() < 1 {
                    return Ok(js_boolean(false));
                }
                Ok(js_boolean(predicate(global_object, $name, call.argument(0))?))
            }
        )*
        mod natives {
            use super::*;
            $(crate::host_function!(pub $function, super::$function);)*
        }
        /// Os predicados na ordem em que o bun os põe no módulo.
        const PREDICATES: &[(&str, NativeFunction)] = &[$(($name, natives::$function)),*];
    };
}

type_predicates! {
    is_external => "isExternal",
    is_date => "isDate",
    is_arguments_object => "isArgumentsObject",
    is_big_int_object => "isBigIntObject",
    is_boolean_object => "isBooleanObject",
    is_number_object => "isNumberObject",
    is_string_object => "isStringObject",
    is_symbol_object => "isSymbolObject",
    is_native_error => "isNativeError",
    is_reg_exp => "isRegExp",
    is_async_function_native => "isAsyncFunction",
    is_generator_function_native => "isGeneratorFunction",
    is_generator_object => "isGeneratorObject",
    is_promise => "isPromise",
    is_map => "isMap",
    is_set => "isSet",
    is_map_iterator => "isMapIterator",
    is_set_iterator => "isSetIterator",
    is_weak_map => "isWeakMap",
    is_weak_set => "isWeakSet",
    is_array_buffer => "isArrayBuffer",
    is_data_view => "isDataView",
    is_shared_array_buffer => "isSharedArrayBuffer",
    is_proxy => "isProxy",
    is_module_namespace_object => "isModuleNamespaceObject",
    is_any_array_buffer => "isAnyArrayBuffer",
    is_boxed_primitive => "isBoxedPrimitive",
    is_array_buffer_view => "isArrayBufferView",
    is_typed_array => "isTypedArray",
    is_uint8_array => "isUint8Array",
    is_uint8_clamped_array => "isUint8ClampedArray",
    is_uint16_array => "isUint16Array",
    is_uint32_array => "isUint32Array",
    is_int8_array => "isInt8Array",
    is_int16_array => "isInt16Array",
    is_int32_array => "isInt32Array",
    is_float16_array => "isFloat16Array",
    is_float32_array => "isFloat32Array",
    is_float64_array => "isFloat64Array",
    is_big_int64_array => "isBigInt64Array",
    is_big_uint64_array => "isBigUint64Array",
    is_key_object => "isKeyObject",
    is_crypto_key => "isCryptoKey",
    is_event_target => "isEventTarget",
}

/// Instalador de `node:util/types`: um objeto comum com os predicados como propriedades graváveis, enumeráveis e
/// configuráveis.
pub(crate) fn install_util_types_module(global_object: &JSGlobalObject) -> HostResult {
    let module = construct_empty_object(global_object);
    let members: Vec<(&str, u32, NativeFunction)> = PREDICATES.iter().map(|(name, function)| (*name, 1, *function)).collect();
    crate::api::node_js_host::put_functions(global_object, &module, &members);
    Ok(module.as_value())
}
