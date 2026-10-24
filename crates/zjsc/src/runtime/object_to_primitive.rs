//! Tradução de `JSObject::toPrimitive` e `JSObject::ordinaryToPrimitive` (`runtime/JSObject.cpp`), com o
//! `callToPrimitiveFunction` que as dois usam, mais o `call(...)` de função JS no formato "valor ou
//! exceção pendente" que o porte usa nas operações do `JSValue` (`ObjectRef::get`, `toObject`).
//!
//! O `JSValue::to_primitive` de `js_value_conversions.rs` só cobre primitivos; o despacho para o objeto
//! entra aqui.
//!
//! DIVERGÊNCIAS:
//!
//! - `isToPrimitiveFastAndNonObservable` do `JSArray` (`fastToString`) está portado, conferindo direto as
//!   propriedades que os watchpoints do C++ vigiam: o atalho é observável na pilha (o `toString` e o `join` do
//!   ciclo de arrays aninhados não entram no VM no bun, só recursam em C++). Os outros atalhos de desempenho sem
//!   efeito observável não foram portados: `defaultToPrimitiveFastAndNonObservable` da `Structure` e o
//!   `cachedSpecialProperty` (o cache de `@@toPrimitive`, `toString` e `valueOf`). O caminho geral
//!   (`getPropertySlot` e `call`) dá o mesmo resultado. O atalho `function == objectProtoValueOfFunction`
//!   também some: o `valueOf` primordial devolve o próprio objeto, que o teste `isObject` descarta do
//!   mesmo jeito.
//! - O resultado é `Option<JSValue>`: `None` é o `JSValue()` vazio com a exceção pendente no `VM`.

use crate::llint::LLIntFailure;
use crate::runtime::array_prototype::{fast_to_string, is_to_primitive_fast_and_non_observable};
use crate::runtime::call_data::{call, get_call_data};
use crate::runtime::host_function_support::{throw_vm_type_error, ObjectRef};
use crate::runtime::identifier::Identifier;
use crate::runtime::js_global_object::JSGlobalObject;
use crate::runtime::js_string::js_string;
use crate::runtime::js_value::JSValue;
use crate::runtime::property_name::PropertyName;
use crate::wtf::text::wtf_string::String as WtfString;

/// `enum PreferredPrimitiveType`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PreferredPrimitiveType {
    NoPreference,
    PreferNumber,
    PreferString,
}

impl PreferredPrimitiveType {
    /// O argumento que o `@@toPrimitive` recebe (`defaultString`, `numberString`, `stringString`).
    fn hint_text(self) -> &'static [u8] {
        match self {
            PreferredPrimitiveType::NoPreference => b"default",
            PreferredPrimitiveType::PreferNumber => b"number",
            PreferredPrimitiveType::PreferString => b"string",
        }
    }
}

/// `call(globalObject, function, callData, thisValue, args)` de uma função chamável: `None` com a
/// exceção pendente no `VM`. `LLIntFailure::Unported*` é a lacuna do porte (o mesmo `panic!` das
/// demais operações sem implementação, nunca um valor inventado).
pub fn call_function(global_object: &JSGlobalObject, function: JSValue, this_value: JSValue, args: &[JSValue]) -> Option<JSValue> {
    let call_data = get_call_data(function);
    match call(global_object, function, &call_data, this_value, args) {
        Ok(value) => Some(value),
        Err(LLIntFailure::Thrown) => None,
        Err(failure) => panic!("chamada de função JS ainda não portada: {failure:?}"),
    }
}

/// O desfecho de um `callToPrimitiveFunction`.
enum Step {
    /// Exceção pendente no `VM` (`return scope.exception()`).
    Exception,
    /// `JSValue()`: sem função ou sem valor primitivo, tenta o próximo método.
    Skip,
    /// O valor primitivo.
    Value(JSValue),
}

/// `callToPrimitiveFunction<key>(globalObject, object, propertyName, hint)`. `hint` só vale para o
/// `@@toPrimitive` (`key == ToPrimitive`), que recebe o argumento de dica e lança nos dois erros.
fn call_to_primitive_function(
    global_object: &JSGlobalObject,
    object: &ObjectRef,
    name: &Identifier,
    to_primitive_hint: Option<PreferredPrimitiveType>,
) -> Step {
    let function = object.get(global_object, &PropertyName::from_identifier(name));
    if global_object.vm().exception().is_some() {
        return Step::Exception;
    }
    if function.is_undefined_or_null() {
        return Step::Skip;
    }

    if get_call_data(function).is_none() {
        if to_primitive_hint.is_some() {
            throw_vm_type_error(global_object, Some("Symbol.toPrimitive is not a function, undefined, or null"));
            return Step::Exception;
        }
        return Step::Skip;
    }

    let hint_argument;
    let arguments: &[JSValue] = match to_primitive_hint {
        Some(hint) => {
            let text = WtfString::from_latin1(hint.hint_text());
            hint_argument = [JSValue::from_js_string(js_string(global_object.vm(), &text))];
            &hint_argument
        }
        None => &[],
    };

    let Some(result) = call_function(global_object, function, object.as_value(), arguments) else {
        return Step::Exception;
    };
    if result.is_object() {
        if to_primitive_hint.is_some() {
            throw_vm_type_error(global_object, Some("Symbol.toPrimitive returned an object"));
            return Step::Exception;
        }
        return Step::Skip;
    }
    Step::Value(result)
}

/// `JSObject::ordinaryToPrimitive(globalObject, hint)`: `None` com a exceção pendente.
pub fn ordinary_to_primitive(global_object: &JSGlobalObject, object: &ObjectRef, hint: PreferredPrimitiveType) -> Option<JSValue> {
    // O frame de `ordinaryToPrimitive` e `callToPrimitiveFunction` que fica na pilha enquanto o método roda.
    let Some(_stack_frame) = global_object.vm().enter_logical_frame(crate::runtime::vm::stack_cost::TO_PRIMITIVE) else {
        let mut scope = crate::runtime::throw_scope::ThrowScope::new(global_object.vm());
        crate::runtime::exception_helpers::throw_stack_overflow_error(global_object, &mut scope);
        return None;
    };
    let names = &global_object.vm().property_names;
    let order = if hint == PreferredPrimitiveType::PreferString {
        [&names.to_string, &names.value_of]
    } else {
        [&names.value_of, &names.to_string]
    };
    for name in order {
        match call_to_primitive_function(global_object, object, name, None) {
            Step::Exception => return None,
            Step::Value(value) => return Some(value),
            Step::Skip => {}
        }
    }
    throw_vm_type_error(global_object, Some("No default value"));
    None
}

/// `JSObject::toPrimitive(globalObject, preferredType)`: `None` com a exceção pendente.
pub fn object_to_primitive(global_object: &JSGlobalObject, object: &ObjectRef, hint: PreferredPrimitiveType) -> Option<JSValue> {
    // `isJSArray(this) && array->isToPrimitiveFastAndNonObservable()`: `fastToString`, sem `call()`.
    if is_to_primitive_fast_and_non_observable(global_object, object.as_value()) {
        return fast_to_string(global_object, object.as_value());
    }
    let to_primitive_symbol = &global_object.vm().property_names.to_primitive_symbol;
    match call_to_primitive_function(global_object, object, to_primitive_symbol, Some(hint)) {
        Step::Exception => return None,
        Step::Value(value) => return Some(value),
        Step::Skip => {}
    }
    ordinary_to_primitive(global_object, object, hint)
}
