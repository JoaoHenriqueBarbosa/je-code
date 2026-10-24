//! Tradução de `jsTypeStringForValueWithConcurrency`, `jsTypeofIsObjectWithConcurrency` e
//! `jsTypeofIsFunctionWithConcurrency` (`runtime/Operations.cpp`, `OperationsInlines.h`): o
//! `typeof` e os testes `is_object`/`is_function` de `op_typeof`, `op_is_object` e
//! `op_is_callable`.
//!
//! DIVERGÊNCIAS:
//!
//! - O resultado é o texto (`&'static str`), não o `JSString*` de `vm.smallStrings`: o chamador que
//!   precisa da célula faz `js_string(vm, ...)`, e a identidade de string não é observável.
//! - Sem `Concurrency::ConcurrentThread` nem `TriState::Indeterminate`: o porte roda em uma thread
//!   e a `Structure` está sempre estável, então só existe o ramo `MainThread`.
//! - `masqueradesAsUndefined(globalObject)` é o `cell_masquerades_as_undefined` das conversões (o bit do
//!   `TypeInfo` e o reino da `Structure`).
//! - `isCallable()` de `JSObject` é decidido pelo `JSType` do cabeçalho: `JSFunctionType`,
//!   `InternalFunctionType` e `NullSetterFunctionType` são os que `JSCell::getCallData` aceita. O
//!   `ProxyObject` com alvo chamável também é (`is_callable_cell`, `m_isCallable`).

use crate::runtime::cell_registry::{self, CellEntry};
use crate::runtime::js_type::{is_object_type, JSType};
use crate::runtime::js_value::JSValue;
use crate::runtime::js_value_conversions::cell_masquerades_as_undefined;

/// `JSObject::isCallable()` pelo `JSType` do cabeçalho.
pub(crate) fn is_callable_type(type_: JSType) -> bool {
    matches!(type_, JSType::JSFunctionType | JSType::InternalFunctionType | JSType::NullSetterFunctionType)
}

/// `JSCell::isCallable()` da célula: o `JSType` do cabeçalho, e o `ProxyObject` cujo alvo é chamável
/// (`ProxyObject::getCallData`, `m_isCallable`).
pub(crate) fn is_callable_cell(cell: &CellEntry) -> bool {
    match cell {
        CellEntry::Proxy(proxy) => proxy.is_callable(),
        _ => is_callable_type(cell.js_type()),
    }
}

/// `jsTypeStringForValue(globalObject, value)`.
pub fn js_type_string_for_value(value: JSValue) -> &'static str {
    if value.is_undefined() {
        return "undefined";
    }
    if value.is_boolean() {
        return "boolean";
    }
    if value.is_number() {
        return "number";
    }
    if !value.is_cell() || value.is_null() {
        // `null` é `typeof` "object"; `Empty` e `Deleted` não chegam aqui no C++ (`isObject()` falso
        // cai no "object" do fim da função).
        return "object";
    }
    let Some(cell) = cell_registry::get(value.as_cell()) else {
        return "object";
    };
    match cell.js_type() {
        JSType::StringType => "string",
        JSType::SymbolType => "symbol",
        JSType::HeapBigIntType => "bigint",
        type_ => {
            if !cell.as_js_object().is_some() && !is_callable_type(type_) && !is_object_type(type_) {
                // célula interna exposta (GetterSetter etc.): "o resultado não importa", o C++ devolve
                // `objectString`.
                return "object";
            }
            if cell_masquerades_as_undefined(&value) {
                "undefined"
            } else if is_callable_cell(&cell) {
                "function"
            } else {
                "object"
            }
        }
    }
}

/// `jsTypeofIsObject(globalObject, value)`: `typeof value === "object"`.
pub fn js_typeof_is_object(value: JSValue) -> bool {
    js_type_string_for_value(value) == "object"
}

/// `jsTypeofIsFunction(globalObject, value)`: `typeof value === "function"`.
pub fn js_typeof_is_function(value: JSValue) -> bool {
    js_type_string_for_value(value) == "function"
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::runtime::js_value::js_number;

    #[test]
    fn primitives() {
        assert_eq!(js_type_string_for_value(JSValue::undefined()), "undefined");
        assert_eq!(js_type_string_for_value(JSValue::null()), "object");
        assert_eq!(js_type_string_for_value(js_number(1.5)), "number");
        assert!(js_typeof_is_object(JSValue::null()));
        assert!(!js_typeof_is_function(js_number(0.0)));
    }
}
