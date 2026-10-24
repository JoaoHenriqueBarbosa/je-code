//! Tradução de `runtime/ErrorType.h` e `runtime/ErrorType.cpp`.
//!
//! `printInternal` (saída para `PrintStream`) não se porta: só serve a depuração.

/// `JSC_ERROR_TYPES`: o número de elementos de `ErrorType`.
pub const NUMBER_OF_ERROR_TYPE: u32 = 9;

/// `enum class ErrorType : uint8_t`.
#[repr(u8)]
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ErrorType {
    Error = 0,
    EvalError = 1,
    RangeError = 2,
    ReferenceError = 3,
    SyntaxError = 4,
    TypeError = 5,
    URIError = 6,
    AggregateError = 7,
    SuppressedError = 8,
}

/// `enum class ErrorTypeWithExtension : uint8_t` (`JSC_ERROR_TYPES_WITH_EXTENSION`).
#[repr(u8)]
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ErrorTypeWithExtension {
    Error = 0,
    EvalError = 1,
    RangeError = 2,
    ReferenceError = 3,
    SyntaxError = 4,
    TypeError = 5,
    URIError = 6,
    AggregateError = 7,
    SuppressedError = 8,
    OutOfMemoryError = 9,
}

const ERROR_TYPE_NAMES: [&str; 10] = [
    "Error",
    "EvalError",
    "RangeError",
    "ReferenceError",
    "SyntaxError",
    "TypeError",
    "URIError",
    "AggregateError",
    "SuppressedError",
    "OutOfMemoryError",
];

/// `errorTypeName(ErrorTypeWithExtension)`.
pub fn error_type_name_with_extension(error_type: ErrorTypeWithExtension) -> &'static str {
    ERROR_TYPE_NAMES[error_type as usize]
}

/// `errorTypeName(ErrorType)`: converte para `ErrorTypeWithExtension` pelo valor, como o C++.
pub fn error_type_name(error_type: ErrorType) -> &'static str {
    ERROR_TYPE_NAMES[error_type as usize]
}

impl From<ErrorType> for ErrorTypeWithExtension {
    fn from(error_type: ErrorType) -> Self {
        match error_type {
            ErrorType::Error => ErrorTypeWithExtension::Error,
            ErrorType::EvalError => ErrorTypeWithExtension::EvalError,
            ErrorType::RangeError => ErrorTypeWithExtension::RangeError,
            ErrorType::ReferenceError => ErrorTypeWithExtension::ReferenceError,
            ErrorType::SyntaxError => ErrorTypeWithExtension::SyntaxError,
            ErrorType::TypeError => ErrorTypeWithExtension::TypeError,
            ErrorType::URIError => ErrorTypeWithExtension::URIError,
            ErrorType::AggregateError => ErrorTypeWithExtension::AggregateError,
            ErrorType::SuppressedError => ErrorTypeWithExtension::SuppressedError,
        }
    }
}
