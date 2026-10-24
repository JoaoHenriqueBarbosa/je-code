//! Tradução de `runtime/Error.h` e `Error.cpp`: `createError` e as variantes por tipo, que devolvem o
//! `JSObject*` (`JSObjectHandle`) de um `ErrorInstance` (`runtime/error_instance.rs`).
//!
//! DIVERGÊNCIAS:
//!
//! - `ErrorInstance::create(..., SourceAppender, RuntimeType, ...)` roda o appender e captura a
//!   pilha; `error_instance.rs` não os tem (veja o cabeçalho dele), então `createTypeError` não repassa
//!   `RuntimeType` e nenhuma função aqui recebe `SourceAppender`.
//! - `JSGlobalObject::errorStructure(ErrorType)` é uma só `Structure` (`JSGlobalObject::error_structure`)
//!   enquanto os protótipos por tipo não existem; o tipo vive no `ErrorData` da célula.
//! - Fora desta fatia: `createInvalidFunctionApplyParameterError`, `createTypeErrorCopy`, `throwTypeError`
//!   e `throwSyntaxError` (dependem de `JSValue::toWTFString`/`PropertySlot` e dos `ErrorMessage`s).

use crate::runtime::error_instance::ErrorInstance;
use crate::runtime::error_type::{ErrorType, ErrorTypeWithExtension};
use crate::runtime::js_global_object::JSGlobalObject;
use crate::runtime::js_object::JSObjectHandle;
use crate::wtf::text::string_concatenate::make_string_dyn;
use crate::wtf::text::wtf_string::String as WtfString;

/// `ErrorInstance::create(globalObject->vm(), globalObject->errorStructure(type), message, ..., type)`; o
/// `ErrorInstance::finishCreation` grava `message` (`DontEnum`).
fn create_error_instance(global_object: &JSGlobalObject, error_type: ErrorType, message: &WtfString) -> JSObjectHandle {
    // `ErrorInstance::finishCreation`: só grava `message` quando há mensagem (o `TypeError` da fronteira do
    // `ShadowRealm` pode nascer com ela vazia; o `message` do protótipo, "", responde).
    let vm = global_object.vm();
    let message = &v8_or_jsc_message(error_type, message);
    let instance = ErrorInstance::create(vm, global_object.error_structure_for(error_type), message.clone(), error_type);
    if !message.is_empty() {
        crate::runtime::error_natives::put_message_property(vm, &instance, message);
    }
    instance.as_object()
}

/// No formato de pilha do V8 o erro que o motor cria leva a mensagem do V8 ([`crate::runtime::v8_stack::v8_message`]).
pub(crate) fn v8_or_jsc_message(error_type: ErrorType, message: &WtfString) -> WtfString {
    if !crate::runtime::stack_frame::v8_stack_format() {
        return message.clone();
    }
    let text = String::from_utf8_lossy(&message.utf8(crate::wtf::text::conversion_mode::ConversionMode::LenientConversion)).into_owned();
    crate::runtime::v8_stack::v8_message(error_type, &text).map_or_else(|| message.clone(), |v8| WtfString::from_utf8(v8.as_bytes()))
}

/// `createError(JSGlobalObject*, const String&)`.
pub fn create_error(global_object: &JSGlobalObject, message: &WtfString) -> JSObjectHandle {
    create_error_instance(global_object, ErrorType::Error, message)
}

/// `createEvalError(JSGlobalObject*, const String&)`.
pub fn create_eval_error(global_object: &JSGlobalObject, message: &WtfString) -> JSObjectHandle {
    create_error_instance(global_object, ErrorType::EvalError, message)
}

/// `createRangeError(JSGlobalObject*, const String&)`.
pub fn create_range_error(global_object: &JSGlobalObject, message: &WtfString) -> JSObjectHandle {
    create_error_instance(global_object, ErrorType::RangeError, message)
}

/// `createReferenceError(JSGlobalObject*, const String&)`.
pub fn create_reference_error(global_object: &JSGlobalObject, message: &WtfString) -> JSObjectHandle {
    create_error_instance(global_object, ErrorType::ReferenceError, message)
}

/// `createSyntaxError(JSGlobalObject*, const String&)`.
pub fn create_syntax_error(global_object: &JSGlobalObject, message: &WtfString) -> JSObjectHandle {
    create_error_instance(global_object, ErrorType::SyntaxError, message)
}

/// `createTypeError(JSGlobalObject*, const String&)`.
pub fn create_type_error(global_object: &JSGlobalObject, message: &WtfString) -> JSObjectHandle {
    create_error_instance(global_object, ErrorType::TypeError, message)
}

/// `createURIError(JSGlobalObject*, const String&)`.
pub fn create_uri_error(global_object: &JSGlobalObject, message: &WtfString) -> JSObjectHandle {
    create_error_instance(global_object, ErrorType::URIError, message)
}

/// `createNotEnoughArgumentsError(JSGlobalObject*)`.
pub fn create_not_enough_arguments_error(global_object: &JSGlobalObject) -> JSObjectHandle {
    create_type_error(global_object, &WtfString::from_latin1(b"Not enough arguments"))
}

/// `createOutOfMemoryError(JSGlobalObject*)`.
pub fn create_out_of_memory_error(global_object: &JSGlobalObject) -> JSObjectHandle {
    let vm = global_object.vm();
    let message = WtfString::from_latin1(b"Out of memory");
    let error = ErrorInstance::create(vm, global_object.error_structure_for(ErrorType::RangeError), message.clone(), ErrorType::RangeError);
    crate::runtime::error_natives::put_message_property(vm, &error, &message);
    error.set_out_of_memory_error();
    error.as_object()
}

/// `createOutOfMemoryError(JSGlobalObject*, const String&)`.
pub fn create_out_of_memory_error_with_message(global_object: &JSGlobalObject, message: &WtfString) -> JSObjectHandle {
    if message.is_empty() {
        return create_out_of_memory_error(global_object);
    }
    let vm = global_object.vm();
    let full_message = make_string_dyn(&[&"Out of memory: ", message]);
    let error = ErrorInstance::create(vm, global_object.error_structure_for(ErrorType::RangeError), full_message.clone(), ErrorType::RangeError);
    crate::runtime::error_natives::put_message_property(vm, &error, &full_message);
    error.set_out_of_memory_error();
    error.as_object()
}

/// `createStackOverflowError(JSGlobalObject*)`.
pub fn create_stack_overflow_error(global_object: &JSGlobalObject) -> JSObjectHandle {
    let vm = global_object.vm();
    let message = v8_or_jsc_message(ErrorType::RangeError, &WtfString::from_latin1(b"Maximum call stack size exceeded."));
    let error = ErrorInstance::create(vm, global_object.error_structure_for(ErrorType::RangeError), message.clone(), ErrorType::RangeError);
    crate::runtime::error_natives::put_message_property(vm, &error, &message);
    error.set_stack_overflow_error();
    error.as_object()
}

/// `createError(JSGlobalObject*, ErrorType, const String&)` (aqui `create_error_of_type`, porque o Rust não sobrecarrega; `create_error` é a sobrecarga sem tipo). `AggregateError` e `SuppressedError` caem
/// no `createError` comum, como no C++ (o construtor deles é que acrescenta `errors`/`error`).
pub fn create_error_of_type(global_object: &JSGlobalObject, error_type: ErrorType, message: &WtfString) -> JSObjectHandle {
    match error_type {
        ErrorType::Error | ErrorType::AggregateError | ErrorType::SuppressedError => create_error(global_object, message),
        ErrorType::EvalError => create_eval_error(global_object, message),
        ErrorType::RangeError => create_range_error(global_object, message),
        ErrorType::ReferenceError => create_reference_error(global_object, message),
        ErrorType::SyntaxError => create_syntax_error(global_object, message),
        ErrorType::TypeError => create_type_error(global_object, message),
        ErrorType::URIError => create_uri_error(global_object, message),
    }
}

/// `createError(JSGlobalObject*, ErrorTypeWithExtension, const String&)`.
pub fn create_error_with_extension(
    global_object: &JSGlobalObject,
    error_type: ErrorTypeWithExtension,
    message: &WtfString,
) -> JSObjectHandle {
    let plain = match error_type {
        ErrorTypeWithExtension::Error => ErrorType::Error,
        ErrorTypeWithExtension::EvalError => ErrorType::EvalError,
        ErrorTypeWithExtension::RangeError => ErrorType::RangeError,
        ErrorTypeWithExtension::ReferenceError => ErrorType::ReferenceError,
        ErrorTypeWithExtension::SyntaxError => ErrorType::SyntaxError,
        ErrorTypeWithExtension::TypeError => ErrorType::TypeError,
        ErrorTypeWithExtension::URIError => ErrorType::URIError,
        ErrorTypeWithExtension::AggregateError => ErrorType::AggregateError,
        ErrorTypeWithExtension::SuppressedError => ErrorType::SuppressedError,
        ErrorTypeWithExtension::OutOfMemoryError => return create_out_of_memory_error_with_message(global_object, message),
    };
    create_error_of_type(global_object, plain, message)
}
