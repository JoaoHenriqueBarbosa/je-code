//! Porte de `runtime/AggregateError.{h,cpp}`, `AggregateErrorConstructor.{h,cpp}` e
//! `AggregateErrorPrototype.{h,cpp}` (com os `Inlines.h`), mais o `initializeAggregateErrorConstructor` de
//! `JSGlobalObject.cpp`: o `AggregateError` (`new AggregateError(errors, message, options)`), o protótipo
//! (`ErrorPrototypeBase`, só `name` e `message`) e a propriedade global.
//!
//! O que o `AggregateError` e o `SuppressedError` repetem (a criação do protótipo, da estrutura de
//! `ErrorInstance` por tipo e do construtor, ligados ao `Error` e ao `Error.prototype`) mora em
//! `install_error_subclass`, que `suppressed_error.rs` também usa: o C++ tem duas classes de cada, mas com o
//! mesmo corpo.
//!
//! DIVERGÊNCIAS:
//! - O `LazyClassStructure` cria tudo na primeira leitura de `AggregateError`/`SuppressedError`; aqui é
//!   eager (como `error_natives::init_error_classes`) e a estrutura de `ErrorInstance` do tipo entra em
//!   `error_structures` (`errorStructure(ErrorType::AggregateError)`).
//! - `ErrorInstance::create` com `sourceAppender`, `runtimeType` e `useCurrentFrame` `nullptr`/`TypeNothing`/
//!   `false` em todos os chamadores deste módulo. A pilha dos construtores é capturada pelo quadro nativo
//!   (`HostCall::capture_stack_frames`); o `Promise.any` captura a dele com o frame nativo `any` no topo
//!   (`capture_stack_frames_with_native`, `at any (unknown)`), e os rejeitadores de elemento, que rodam numa
//!   microtask, não capturam nada (o erro fica sem `stack`, como no bun). A propriedade `message` é gravada por
//!   `put_message_property`.
//! - O `ClassInfo` do construtor é o `ERROR_CONSTRUCTOR_S_INFO` (`"Function"`, base `JSFunction`), o
//!   mesmo que `error_natives.rs` usa para o `Error` e os nativos; o C++ tem um `s_info` por classe, iguais.
//! - `MarkedArgumentBuffer` é um `Vec`: não há `hasOverflowed`.

use crate::runtime::error_instance::{ErrorInstance, ErrorInstanceRef};
use crate::runtime::error_natives::{create_error_constructor_function, error_constructor_structure, put_message_property, ERROR_PROTOTYPE_S_INFO};
use crate::runtime::error_prototype::initial_name_and_message;
use crate::runtime::error_type::{error_type_name, ErrorType};
use crate::runtime::host_call::{HostCall, HostResult, Thrown};
use crate::runtime::identifier::Identifier;
use crate::runtime::internal_function::get_derived_structure_in_realm;
use crate::runtime::js_function::JSFunctionRef;
use crate::runtime::iterator_operations::for_each_in_iterable;
use crate::runtime::js_array::construct_array;
use crate::runtime::js_global_object::JSGlobalObject;
use crate::runtime::js_object::{JSNonFinalObject, JSObject, JSObjectRef};
use crate::runtime::js_string::{js_empty_string, js_string};
use crate::runtime::js_type::JSType;
use crate::runtime::js_type_info::TypeInfo;
use crate::runtime::js_value::JSValue;
use crate::runtime::native_function::NativeFunction;
use crate::runtime::property_attribute::DONT_ENUM;
use crate::runtime::property_name::PropertyName;
use crate::runtime::proxy_object::{object_get, object_has_property};
use crate::runtime::string_regexp_support::to_wtf_string_value;
use crate::runtime::structure::{Structure, StructureRef};
use crate::runtime::vm::VM;
use crate::wtf::text::wtf_string::String as WtfString;
use crate::host_function;

/// `AggregateErrorConstructor`: `finishCreation(vm, 2, ...)`.
const AGGREGATE_ERROR_CONSTRUCTOR_LENGTH: u32 = 2;

/// `getIfPropertyExists(globalObject, propertyName)`: `None` quando a propriedade não existe.
pub(crate) fn get_if_property_exists(
    global_object: &JSGlobalObject,
    object: &JSObject,
    property_name: &PropertyName,
) -> Result<Option<JSValue>, Thrown> {
    if !object_has_property(global_object, object, property_name)? {
        return Ok(None);
    }
    object_get(global_object, object, property_name, object.as_value()).map(Some)
}

/// `message.isUndefined() ? String() : message.toWTFString(globalObject)`: `None` é o `String()` nulo. Um
/// `Symbol` ou um `toString` que lança é o `RETURN_IF_EXCEPTION`: o `Err` carrega a exceção pendente.
pub(crate) fn message_to_string(global_object: &JSGlobalObject, message: JSValue) -> Result<Option<WtfString>, Thrown> {
    if message.is_undefined() {
        return Ok(None);
    }
    to_wtf_string_value(global_object, message).map(Some)
}

/// `createAggregateError(vm, structure, errors, message, cause, ...)`: o `ErrorInstance` com `cause` (se
/// houver) e `errors` (`DontEnum`). `pub(crate)` porque `Promise.any` também o usa.
pub(crate) fn create_aggregate_error(
    vm: &VM,
    structure: StructureRef,
    errors: JSValue,
    message: Option<WtfString>,
    cause: Option<JSValue>,
) -> ErrorInstanceRef {
    let error = ErrorInstance::create(vm, structure, message.clone().unwrap_or_default(), ErrorType::AggregateError);
    if let Some(message) = &message {
        put_message_property(vm, &error, message);
    }
    if let Some(cause) = cause {
        error.put_direct(vm, &PropertyName::from_identifier(&vm.property_names.cause), cause, DONT_ENUM);
    }
    error.put_direct(vm, &PropertyName::from_identifier(&vm.property_names.errors), errors, DONT_ENUM);
    error
}

/// `constructAggregateError(globalObject, vm, structure, errors, message, options, ...)`.
fn construct_aggregate_error(global_object: &JSGlobalObject, structure: StructureRef, call: &HostCall) -> HostResult {
    let vm = global_object.vm();
    let (errors, message, options) = (call.argument(0), call.argument(1), call.argument(2));

    let message_string = message_to_string(global_object, message)?;

    // "Since `throw undefined;` is valid, we need to distinguish the case where `cause` is an explicit undefined."
    let mut cause = None;
    if let Some(options) = JSObject::from_value(&options) {
        cause = get_if_property_exists(global_object, &options, &PropertyName::from_identifier(&vm.property_names.cause))?;
    }

    let mut errors_list = Vec::new();
    for_each_in_iterable(global_object, errors, |next_value| {
        errors_list.push(next_value);
        Ok(())
    })?;

    // `constructArray(globalObject, nullptr, errorsList)`.
    let array = construct_array(vm, &global_object.array_structure(), &errors_list);
    let error = create_aggregate_error(vm, structure, array.as_value(), message_string, cause);
    // `ErrorInstance::finishCreation` captura a pilha de quem chamou o construtor.
    if let Some(frames) = call.capture_stack_frames(global_object, None) {
        error.set_pending_stack(frames);
    }
    Ok(error.as_value())
}

/// `callAggregateErrorConstructor`.
fn call_aggregate_error_body(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    let structure = global_object.error_structure_for(ErrorType::AggregateError);
    construct_aggregate_error(global_object, structure, call)
}

/// `constructAggregateErrorConstructor`: a estrutura deriva de `newTarget`.
fn construct_aggregate_error_body(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    let structure = get_derived_structure_in_realm(global_object, call.new_target(), call.callee(), |realm| {
        realm.error_structure_for(ErrorType::AggregateError)
    })?;
    construct_aggregate_error(global_object, structure, call)
}

host_function!(call_aggregate_error_constructor, call_aggregate_error_body);
host_function!(construct_aggregate_error_constructor, construct_aggregate_error_body);

/// `createStructure(vm, globalObject, prototype)` de `AggregateErrorPrototype` e `SuppressedErrorPrototype`:
/// `ObjectType`, `JSNonFinalObject::StructureFlags`.
fn error_subclass_prototype_structure(vm: &VM, global_object: &JSGlobalObject, prototype: JSValue) -> StructureRef {
    Structure::create(
        vm,
        Some(global_object),
        prototype,
        TypeInfo::new(JSType::ObjectType, JSNonFinalObject::STRUCTURE_FLAGS),
        &ERROR_PROTOTYPE_S_INFO,
    )
}

/// `ErrorPrototypeBase::finishCreation(vm, errorTypeName(errorType))`: `name` e `message`, ambos `DontEnum`.
fn create_error_subclass_prototype(vm: &VM, structure: &StructureRef, type_name: &str) -> JSObjectRef {
    let prototype = JSObject::allocate(vm, structure);
    prototype.finish_creation(vm);
    let (name, _) = initial_name_and_message(type_name);
    prototype.put_direct_without_transition(
        vm,
        &PropertyName::from_identifier(&vm.property_names.name),
        JSValue::from_js_string(js_string(vm, &name)),
        DONT_ENUM,
    );
    prototype.put_direct_without_transition(
        vm,
        &PropertyName::from_identifier(&vm.property_names.message),
        JSValue::from_js_string(js_empty_string(vm)),
        DONT_ENUM,
    );
    prototype
}

/// `initializeAggregateErrorConstructor` e `initializeSuppressedErrorConstructor` (JSGlobalObject.cpp:1063 e
/// 1070) mais a propriedade global: protótipo sobre o `Error.prototype`, a estrutura de `ErrorInstance` do
/// tipo, o construtor (comprimento `length`) sobre o `Error`, o `constructor` do protótipo (`DontEnum`) e
/// `AggregateError`/`SuppressedError` no global (`DontEnum`). O `Error` e o `Error.prototype` são os que
/// `init_error_classes` já pôs no global.
pub(crate) fn install_error_subclass(
    global_object: &JSGlobalObject,
    error_type: ErrorType,
    length: u32,
    call: NativeFunction,
    construct: NativeFunction,
) -> JSFunctionRef {
    let name = error_type_name(error_type);
    install_named_error_subclass(
        global_object,
        name,
        length,
        call,
        construct,
        |structure| global_object.error_structures.borrow_mut().push((error_type, structure)),
        |vm, constructor| {
            global_object.put_direct(
                vm,
                &PropertyName::from_identifier(&Identifier::from_span(vm, name.as_bytes())),
                constructor,
                DONT_ENUM,
            );
        },
    )
}

/// O corpo de `install_error_subclass` para um erro que não é um `ErrorType` (os do WebAssembly, que o C++
/// também cria sobre `Error`/`Error.prototype`): `register` recebe a estrutura de `ErrorInstance` e `define`
/// grava o construtor onde o chamador quiser (o global, ou o objeto `WebAssembly`).
pub(crate) fn install_named_error_subclass(
    global_object: &JSGlobalObject,
    type_name: &str,
    length: u32,
    call: NativeFunction,
    construct: NativeFunction,
    register: impl FnOnce(StructureRef),
    define: impl FnOnce(&VM, JSValue),
) -> JSFunctionRef {
    let vm = global_object.vm();
    let error_constructor_name = PropertyName::from_identifier(&Identifier::from_span(vm, error_type_name(ErrorType::Error).as_bytes()));
    let error_constructor = JSObject::from_value(&global_object.get_direct_by_name(vm, &error_constructor_name))
        .expect("init_error_classes cria o Error antes dos erros derivados");
    let error_prototype = error_constructor.get_direct_by_name(vm, &PropertyName::from_identifier(&vm.property_names.prototype));

    let prototype_structure = error_subclass_prototype_structure(vm, global_object, error_prototype);
    let prototype = create_error_subclass_prototype(vm, &prototype_structure, type_name);
    prototype.did_become_prototype(vm);

    let instance_structure = ErrorInstance::create_structure(vm, Some(global_object), prototype.as_value());
    register(instance_structure);

    let constructor_structure = error_constructor_structure(vm, global_object, error_constructor.as_value(), 0);
    let constructor = create_error_constructor_function(vm, global_object, constructor_structure, &prototype, type_name, length, call, construct);
    define(vm, constructor.as_value());
    constructor
}

/// Cria o `AggregateError` (`m_aggregateErrorStructure`).
pub fn install_aggregate_error(global_object: &JSGlobalObject) -> JSFunctionRef {
    install_error_subclass(
        global_object,
        ErrorType::AggregateError,
        AGGREGATE_ERROR_CONSTRUCTOR_LENGTH,
        call_aggregate_error_constructor,
        construct_aggregate_error_constructor,
    )
}
