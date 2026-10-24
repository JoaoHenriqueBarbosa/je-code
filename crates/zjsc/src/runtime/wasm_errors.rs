//! Porte de `wasm/js/JSWebAssemblyCompileError.{h,cpp}`, `JSWebAssemblyLinkError`,
//! `JSWebAssemblyRuntimeError` (e os `WebAssembly*ErrorConstructor`/`Prototype`): `WebAssembly.CompileError`,
//! `WebAssembly.LinkError` e `WebAssembly.RuntimeError`.
//!
//! No C++ os três são subclasses de `ErrorInstance` com estrutura própria, mas com `ErrorType::Error`; por
//! isso aqui também não são `ErrorType` novos. A estrutura de cada um mora em
//! `JSGlobalObject::wasm_error_structures`, o construtor e o protótipo são instalados no objeto `WebAssembly`
//! (não no global) e `Thrown::WebAssembly` lança o erro com a mensagem do C++.
//!
//! DIVERGÊNCIA: `ErrorInstance::create` com `sourceAppender`/`runtimeType` nulos, como `AggregateError`; o
//! protótipo é um `ErrorPrototypeBase` (`name` e `message`), como o `AggregateErrorPrototype`.

use crate::host_function;
use crate::runtime::aggregate_error::{get_if_property_exists, install_named_error_subclass, message_to_string};
use crate::runtime::error_instance::{ErrorInstance, ErrorInstanceRef};
use crate::runtime::error_natives::put_message_property;
use crate::runtime::error_type::ErrorType;
use crate::runtime::host_call::{HostCall, HostResult};
use crate::runtime::internal_function::get_derived_structure_in_realm;
use crate::runtime::js_global_object::JSGlobalObject;
use crate::runtime::js_object::JSObject;
use crate::runtime::js_value::JSValue;
use crate::runtime::property_attribute::DONT_ENUM;
use crate::runtime::property_name::PropertyName;
use crate::runtime::structure::StructureRef;
use crate::runtime::vm::VM;
use crate::runtime::identifier::Identifier;
use crate::wtf::text::wtf_string::String as WtfString;

/// Os três erros do WebAssembly.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WasmErrorKind {
    Compile,
    Link,
    Runtime,
    /// `WebAssembly.SuspendError` (JSPI).
    Suspend,
}

impl WasmErrorKind {
    /// O `name` do protótipo e do construtor (`"CompileError"`...).
    pub fn type_name(self) -> &'static str {
        match self {
            WasmErrorKind::Compile => "CompileError",
            WasmErrorKind::Link => "LinkError",
            WasmErrorKind::Runtime => "RuntimeError",
            WasmErrorKind::Suspend => "SuspendError",
        }
    }
}

/// O `ClassStructure::get` do tipo (`globalObject->webAssemblyCompileErrorStructure()` etc.).
fn wasm_error_structure(global_object: &JSGlobalObject, kind: WasmErrorKind) -> StructureRef {
    global_object
        .wasm_error_structures
        .borrow()
        .iter()
        .find(|(known, _)| *known == kind)
        .map(|(_, structure)| structure.clone())
        .expect("install_web_assembly instala os erros do WebAssembly antes de usá-los")
}

/// `JSWebAssembly{Compile,Link,Runtime}Error::create(globalObject, vm, structure, message)`.
pub fn create_wasm_error(global_object: &JSGlobalObject, kind: WasmErrorKind, message: &WtfString) -> ErrorInstanceRef {
    let vm = global_object.vm();
    let error = ErrorInstance::create(vm, wasm_error_structure(global_object, kind), message.clone(), ErrorType::Error);
    put_message_property(vm, &error, message);
    error
}

/// `constructJSWebAssembly*Error(globalObject, callFrame, structure)`: `message` (argumento 0) e `cause` de
/// `options` (argumento 1).
fn construct_wasm_error(global_object: &JSGlobalObject, structure: StructureRef, call: &HostCall) -> HostResult {
    let vm = global_object.vm();
    let message = message_to_string(global_object, call.argument(0))?;
    let mut cause = None;
    if let Some(options) = JSObject::from_value(&call.argument(1)) {
        cause = get_if_property_exists(global_object, &options, &PropertyName::from_identifier(&vm.property_names.cause))?;
    }
    let error = ErrorInstance::create(vm, structure, message.clone().unwrap_or_default(), ErrorType::Error);
    if let Some(message) = &message {
        put_message_property(vm, &error, message);
    }
    if let Some(cause) = cause {
        error.put_direct(vm, &PropertyName::from_identifier(&vm.property_names.cause), cause, DONT_ENUM);
    }
    if let Some(frames) = call.capture_stack_frames(global_object, None) {
        error.set_pending_stack(frames);
    }
    Ok(error.as_value())
}

/// `call...ErrorConstructor`: chamar sem `new` usa a estrutura do realm.
fn call_wasm_error(global_object: &JSGlobalObject, call: &HostCall, kind: WasmErrorKind) -> HostResult {
    construct_wasm_error(global_object, wasm_error_structure(global_object, kind), call)
}

/// `construct...ErrorConstructor`: a estrutura deriva de `newTarget`.
fn construct_wasm_error_with_new_target(global_object: &JSGlobalObject, call: &HostCall, kind: WasmErrorKind) -> HostResult {
    let structure = get_derived_structure_in_realm(global_object, call.new_target(), call.callee(), |realm| wasm_error_structure(realm, kind))?;
    construct_wasm_error(global_object, structure, call)
}

fn call_compile_error_body(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    call_wasm_error(global_object, call, WasmErrorKind::Compile)
}
fn construct_compile_error_body(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    construct_wasm_error_with_new_target(global_object, call, WasmErrorKind::Compile)
}
fn call_link_error_body(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    call_wasm_error(global_object, call, WasmErrorKind::Link)
}
fn construct_link_error_body(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    construct_wasm_error_with_new_target(global_object, call, WasmErrorKind::Link)
}
fn call_runtime_error_body(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    call_wasm_error(global_object, call, WasmErrorKind::Runtime)
}
fn construct_runtime_error_body(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    construct_wasm_error_with_new_target(global_object, call, WasmErrorKind::Runtime)
}

fn call_suspend_error_body(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    call_wasm_error(global_object, call, WasmErrorKind::Suspend)
}
fn construct_suspend_error_body(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    construct_wasm_error_with_new_target(global_object, call, WasmErrorKind::Suspend)
}

host_function!(call_suspend_error, call_suspend_error_body);
host_function!(construct_suspend_error, construct_suspend_error_body);
host_function!(call_compile_error, call_compile_error_body);
host_function!(construct_compile_error, construct_compile_error_body);
host_function!(call_link_error, call_link_error_body);
host_function!(construct_link_error, construct_link_error_body);
host_function!(call_runtime_error, call_runtime_error_body);
host_function!(construct_runtime_error, construct_runtime_error_body);

/// `JSWebAssembly::finishCreation`: instala os três construtores (`length` 1) em `namespace`
/// (`WebAssembly.CompileError` etc., `DontEnum`) com o protótipo sobre `Error.prototype`.
pub fn install_wasm_errors(global_object: &JSGlobalObject, namespace: &JSObject, only: WasmErrorKind) {
    let entries = [
        (WasmErrorKind::Compile, call_compile_error as crate::runtime::native_function::NativeFunction, construct_compile_error as crate::runtime::native_function::NativeFunction),
        (WasmErrorKind::Link, call_link_error, construct_link_error),
        (WasmErrorKind::Runtime, call_runtime_error, construct_runtime_error),
        (WasmErrorKind::Suspend, call_suspend_error, construct_suspend_error),
    ];
    for (kind, call, construct) in entries {
        if kind != only {
            continue;
        }
        install_named_error_subclass(
            global_object,
            kind.type_name(),
            1,
            call,
            construct,
            |structure| global_object.wasm_error_structures.borrow_mut().push((kind, structure)),
            |vm: &VM, constructor: JSValue| {
                namespace.put_direct(vm, &PropertyName::from_identifier(&Identifier::from_span(vm, kind.type_name().as_bytes())), constructor, DONT_ENUM);
            },
        );
    }
}
