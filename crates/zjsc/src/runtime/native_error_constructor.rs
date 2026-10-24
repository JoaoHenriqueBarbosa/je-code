//! Porte de `runtime/NativeErrorConstructor.cpp` e `NativeErrorConstructor.h`: os seis construtores
//! `EvalError`, `RangeError`, `ReferenceError`, `SyntaxError`, `TypeError` e `URIError`.
//!
//! DIVERGÊNCIAS (ligação pendente, a `NativeFunction` está sendo redesenhada):
//!
//! - O `template<ErrorType>` do C++ e as seis duplas `callXError`/`constructXError` viram dados: a
//!   casca escolhe o `ErrorType` e chama `ErrorInstance::create(globalObject, structure, message, options,
//!   nullptr, TypeNothing, errorType, false)`. A chamada usa `globalObject->errorStructure(errorType)`; o
//!   `construct` deriva a estrutura de `newTarget` (`JSC_GET_DERIVED_STRUCTURE`), sem passar `newTarget`
//!   ao `ErrorInstance` (diferente do `Error`).
//! - `finishCreation`: `length` 1, `name` = `error_type_name(tipo)` (sem transição) e `prototype` com
//!   `DontDelete|ReadOnly|DontEnum`; as constantes e `constructor_name` são os dados dessa criação.

use crate::runtime::error_type::{error_type_name, ErrorType};
use crate::runtime::property_attribute::PropertyAttribute;

/// Os tipos que têm construtor nativo, na ordem do `JSC_NATIVE_ERROR_TYPES`.
pub const NATIVE_ERROR_TYPES: [ErrorType; 6] = [
    ErrorType::EvalError,
    ErrorType::RangeError,
    ErrorType::ReferenceError,
    ErrorType::SyntaxError,
    ErrorType::TypeError,
    ErrorType::URIError,
];

/// O `length` de todo construtor nativo.
pub const LENGTH: u32 = 1;

/// Os atributos de `prototype`.
pub const PROTOTYPE_ATTRIBUTES: u32 =
    PropertyAttribute::DontDelete as u32 | PropertyAttribute::ReadOnly as u32 | PropertyAttribute::DontEnum as u32;

/// `callFunction`/`constructFunction` devolvem `nullptr` para o que não é nativo (`Error`,
/// `AggregateError`, `SuppressedError`): aqui, `false`.
pub fn is_native_error_type(error_type: ErrorType) -> bool {
    NATIVE_ERROR_TYPES.contains(&error_type)
}

/// O `name` do construtor (`errorTypeName(errorType)`), só para os tipos nativos.
pub fn constructor_name(error_type: ErrorType) -> Option<&'static str> {
    is_native_error_type(error_type).then(|| error_type_name(error_type))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_and_membership() {
        assert_eq!(constructor_name(ErrorType::TypeError), Some("TypeError"));
        assert_eq!(constructor_name(ErrorType::URIError), Some("URIError"));
        assert_eq!(constructor_name(ErrorType::Error), None);
        assert_eq!(constructor_name(ErrorType::AggregateError), None);
        assert_eq!(NATIVE_ERROR_TYPES.len(), 6);
        assert!(NATIVE_ERROR_TYPES.iter().all(|&t| is_native_error_type(t)));
    }
}
