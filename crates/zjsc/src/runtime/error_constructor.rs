//! Porte de `runtime/ErrorConstructor.cpp` e `ErrorConstructor.h`: o construtor `Error`, o
//! `stackTraceLimit` e `Error.isError`.
//!
//! DIVERGÊNCIAS (ligação pendente, a `NativeFunction` está sendo redesenhada):
//!
//! - `callErrorConstructor` e `constructErrorConstructor` são cascas sobre `ErrorInstance::create`:
//!   a chamada usa `globalObject->errorStructure()`; o `construct` deriva a estrutura de `newTarget`
//!   (`JSC_GET_DERIVED_STRUCTURE`) e passa `newTarget` ao `ErrorInstance` quando `newTarget != callee`
//!   (`is_subclass`, abaixo). Os argumentos são `argument(0)` (mensagem) e `argument(1)` (opções).
//! - `errorConstructorCaptureStackTrace` precisa de `Interpreter::getStackTrace`, `stackTraceAsString` e
//!   `putDirect` no objeto; só a validação do primeiro argumento (mensagem em `CAPTURE_STACK_TRACE_NOT_OBJECT`)
//!   e o tamanho a capturar (`capture_limit`) são puros.
//! - `ErrorConstructor::put` e `deleteProperty` (que mexem em `globalObject->setStackTraceLimit`) usam
//!   `stack_trace_limit_from_value` e o `None` do delete; a casca faz o `Base::put`/`Base::deleteProperty`.
//! - `finishCreation` (`length` 1, `name` "Error", `prototype` com `DontEnum|DontDelete|ReadOnly`,
//!   `stackTraceLimit` com `None`, `captureStackTrace` com `DontEnum` e comprimento 0, `isError` com
//!   `DontEnum` e comprimento 1 e o intrínseco `ErrorIsErrorIntrinsic`) entra junto com a ligação; as
//!   constantes abaixo são os dados dessa criação.

use crate::runtime::property_attribute::PropertyAttribute;

/// O `length` do construtor `Error`.
pub const LENGTH: u32 = 1;

/// Os atributos de `Error.prototype`.
pub const PROTOTYPE_ATTRIBUTES: u32 =
    PropertyAttribute::DontEnum as u32 | PropertyAttribute::DontDelete as u32 | PropertyAttribute::ReadOnly as u32;

/// Os atributos de `Error.stackTraceLimit`.
pub const STACK_TRACE_LIMIT_ATTRIBUTES: u32 = PropertyAttribute::None as u32;

/// Os atributos de `captureStackTrace` e `isError`.
pub const FUNCTION_ATTRIBUTES: u32 = PropertyAttribute::DontEnum as u32;

/// O `length` de `Error.captureStackTrace`.
pub const CAPTURE_STACK_TRACE_LENGTH: u32 = 2;

/// O `length` de `Error.isError`.
pub const IS_ERROR_LENGTH: u32 = 1;

/// A mensagem do `TypeError` de `captureStackTrace` com primeiro argumento que não é objeto. Medido no `bun`
/// 1.4.2: o host troca a mensagem do JSC pelo código `invalid_argument`.
pub const CAPTURE_STACK_TRACE_NOT_OBJECT: &str = "invalid_argument";

/// `isErrorSubclass = newTarget != callee`.
pub fn is_subclass(new_target_cell: usize, callee_cell: usize) -> bool {
    new_target_cell != callee_cell
}

/// O novo `stackTraceLimit` guardado no global em `ErrorConstructor::put`: número vira
/// `clamp(valor, 0, UINT_MAX)` truncado; qualquer outro valor limpa (`std::nullopt`). O argumento é o
/// número quando `value.isNumber()`, `None` caso contrário.
pub fn stack_trace_limit_from_value(number: Option<f64>) -> Option<u32> {
    let value = number?;
    // `std::max(0., NaN)` devolve 0 e `std::min(0, max)` mantém 0; o `static_cast` do C++ de NaN não
    // chega aqui, então NaN fica 0 como o `max` o deixaria.
    let clamped = if value.is_nan() { 0.0 } else { value.max(0.0).min(f64::from(u32::MAX)) };
    Some(clamped as u32)
}

/// O limite efetivo de frames de `captureStackTrace`: `stackTraceLimit().value_or(0)`.
pub fn capture_limit(stack_trace_limit: Option<u32>) -> u32 {
    stack_trace_limit.unwrap_or(0)
}

/// O limite inicial de `stackTraceLimit` (`value_or(Options::defaultErrorStackTraceLimit())`).
pub fn initial_stack_trace_limit(global_limit: Option<u32>, default_limit: u32) -> u32 {
    global_limit.unwrap_or(default_limit)
}

/// `errorConstructorIsError` depois do `dynamicDowncast<JSObject>`: `object && object->isErrorInstance()`.
pub fn is_error(is_object: bool, is_error_instance: bool) -> bool {
    is_object && is_error_instance
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stack_trace_limit_put() {
        assert_eq!(stack_trace_limit_from_value(Some(10.0)), Some(10));
        assert_eq!(stack_trace_limit_from_value(Some(-5.0)), Some(0));
        assert_eq!(stack_trace_limit_from_value(Some(1e20)), Some(u32::MAX));
        assert_eq!(stack_trace_limit_from_value(Some(f64::INFINITY)), Some(u32::MAX));
        assert_eq!(stack_trace_limit_from_value(Some(3.9)), Some(3));
        // `std::max(0., NaN)` é 0.
        assert_eq!(stack_trace_limit_from_value(Some(f64::NAN)), Some(0));
        assert_eq!(stack_trace_limit_from_value(Some(f64::NEG_INFINITY)), Some(0));
        assert_eq!(stack_trace_limit_from_value(None), None);
    }

    #[test]
    fn helpers() {
        assert!(is_subclass(1, 2));
        assert!(!is_subclass(2, 2));
        assert_eq!(capture_limit(None), 0);
        assert_eq!(initial_stack_trace_limit(None, 100), 100);
        assert_eq!(initial_stack_trace_limit(Some(5), 100), 5);
        assert!(is_error(true, true));
        assert!(!is_error(true, false));
        assert!(!is_error(false, true));
    }
}
