//! Porte de `runtime/SuppressedError.{h,cpp}`, `SuppressedErrorConstructor.{h,cpp}` e
//! `SuppressedErrorPrototype.{h,cpp}` (com os `Inlines.h`), mais o `initializeSuppressedErrorConstructor` de
//! `JSGlobalObject.cpp`. O protótipo, a estrutura de `ErrorInstance` por tipo e a criação do construtor são
//! os de `aggregate_error.rs` (`install_error_subclass`); aqui ficam `createSuppressedError` e as funções
//! `call`/`construct`. As DIVERGÊNCIAS são as de `aggregate_error.rs`.

use crate::host_function;
use crate::runtime::aggregate_error::{install_error_subclass, message_to_string};
use crate::runtime::error_instance::{ErrorInstance, ErrorInstanceRef};
use crate::runtime::error_natives::put_message_property;
use crate::runtime::error_type::ErrorType;
use crate::runtime::host_call::{HostCall, HostResult, Thrown};
use crate::runtime::internal_function::get_derived_structure_in_realm;
use crate::runtime::js_function::JSFunctionRef;
use crate::runtime::js_global_object::JSGlobalObject;
use crate::runtime::property_attribute::DONT_ENUM;
use crate::runtime::property_name::PropertyName;
use crate::runtime::structure::StructureRef;

/// `SuppressedErrorConstructor`: `finishCreation(vm, 3, ...)`.
const SUPPRESSED_ERROR_CONSTRUCTOR_LENGTH: u32 = 3;

/// `createSuppressedError(globalObject, vm, structure, error, suppressed, message, ...)`: o `ErrorInstance`
/// sem `cause`, com `error` e `suppressed` (`DontEnum`).
fn create_suppressed_error(
    global_object: &JSGlobalObject,
    structure: StructureRef,
    call: &HostCall,
) -> Result<ErrorInstanceRef, Thrown> {
    let vm = global_object.vm();
    let (error, suppressed, message) = (call.argument(0), call.argument(1), call.argument(2));

    let message_string = message_to_string(global_object, message)?;

    let suppressed_error =
        ErrorInstance::create(vm, structure, message_string.clone().unwrap_or_default(), ErrorType::SuppressedError);
    // `ErrorInstance::finishCreation` captura a pilha de quem chamou o construtor.
    if let Some(frames) = call.capture_stack_frames(global_object, None) {
        suppressed_error.set_pending_stack(frames);
    }
    if let Some(message_string) = &message_string {
        put_message_property(vm, &suppressed_error, message_string);
    }

    suppressed_error.put_direct(vm, &PropertyName::from_identifier(&vm.property_names.error_dup), error, DONT_ENUM);
    suppressed_error.put_direct(vm, &PropertyName::from_identifier(&vm.property_names.suppressed), suppressed, DONT_ENUM);

    Ok(suppressed_error)
}

/// `callSuppressedErrorConstructor`.
fn call_suppressed_error_body(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    let structure = global_object.error_structure_for(ErrorType::SuppressedError);
    create_suppressed_error(global_object, structure, call).map(|error| error.as_value())
}

/// `constructSuppressedErrorConstructor`: a estrutura deriva de `newTarget`.
fn construct_suppressed_error_body(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    let structure = get_derived_structure_in_realm(global_object, call.new_target(), call.callee(), |realm| {
        realm.error_structure_for(ErrorType::SuppressedError)
    })?;
    create_suppressed_error(global_object, structure, call).map(|error| error.as_value())
}

host_function!(call_suppressed_error_constructor, call_suppressed_error_body);
host_function!(construct_suppressed_error_constructor, construct_suppressed_error_body);

/// Cria o `SuppressedError` (`m_suppressedErrorStructure`).
pub fn install_suppressed_error(global_object: &JSGlobalObject) -> JSFunctionRef {
    install_error_subclass(
        global_object,
        ErrorType::SuppressedError,
        SUPPRESSED_ERROR_CONSTRUCTOR_LENGTH,
        call_suppressed_error_constructor,
        construct_suppressed_error_constructor,
    )
}
