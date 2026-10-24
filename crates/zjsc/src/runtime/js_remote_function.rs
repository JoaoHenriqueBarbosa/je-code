//! Porte de `runtime/JSRemoteFunction.h`, `JSRemoteFunctionInlines.h` e `JSRemoteFunction.cpp`: a função que
//! liga um reino ao outro no `ShadowRealm` (`remoteFunctionCallForJSFunction`, `remoteFunctionCallGeneric`,
//! `tryCreate`, `copyNameAndLength`, `createRemoteFunction`, `isRemoteFunction` e os `wrapArgument` e
//! `wrapReturnValue`, que são o mesmo corpo).
//!
//! DIVERGÊNCIAS, e por quê:
//! - `JSRemoteFunction` é subclasse de `JSFunction`; aqui é o campo `remote` do `JSFunction`
//!   ([`JSFunction::as_remote_function`]), pelo mesmo motivo do `JSBoundFunction` (o registro de células e o
//!   `getCallData` alcançam funções só por `CellEntry::Function`), então não há entrada nova em
//!   `cell_registry.rs`. A célula é a mesma `JSFunction`, com o `ClassInfo` e a `Structure` do
//!   `JSRemoteFunction`.
//! - `vm.getRemoteFunction(isJSFunction)` (os dois `NativeExecutable` guardados em `Weak`) é o
//!   `getHostFunctionWithIntrinsic` direto. O `executable->entrypointFor(...)` do caminho rápido só
//!   aquece o cache do JIT e não existe.
//! - O `MarkedArgumentBuffer` é um `Vec`; o `hasOverflowed` não existe. `visitChildren` some.
//! - `copyNameAndLength` roda antes da célula existir (só lê o alvo, e a célula não é observável antes de
//!   `tryCreate` devolvê-la), e `m_nameMayBeNull`/`m_length` ficam como campos sem mutabilidade interior.
//! - O `toIntegerOrInfinity` do `length` em `copyNameAndLength` chama o `valueOf` (`to_integer_or_infinity_checked`,
//!   no `CurrentRealmScope` do `globalObject`) e a exceção sai pendente, como no C++.
//! - O `wrapReturnValue` do caminho genérico cria a função no reino do alvo e o do caminho rápido no
//!   reino de quem chama: é o que o C++ do upstream faz, mantido.

use std::rc::Rc;

use crate::host_function;
use crate::runtime::call_data::{call, get_call_data};
use crate::runtime::cell_registry::{self, CellEntry};
use crate::runtime::class_info::ClassInfo;
use crate::runtime::current_realm::CurrentRealmScope;
use crate::runtime::host_call::{pending_or, throw_thrown, HostCall, HostResult, Thrown};
use crate::runtime::error_instance::ErrorInstance;
use crate::runtime::implementation_visibility::ImplementationVisibility;
use crate::runtime::intrinsic::Intrinsic;
use crate::runtime::iterator_operations::thrown_from_llint;
use crate::runtime::js_function::{call_host_function_as_constructor, function_structure, JSFunction, JSFunctionRef, JS_FUNCTION_S_INFO};
use crate::runtime::js_global_object::{JSGlobalObject, JSGlobalObjectRef};
use crate::runtime::js_scope::JSScopeRef;
use crate::runtime::js_string::JSStringRef;
use crate::runtime::js_value::{js_boolean, js_undefined, JSValue};
use crate::runtime::native_function::NativeFunction;
use crate::runtime::property_name::PropertyName;
use crate::runtime::property_slot::{InternalMethodType, PropertySlot};
use crate::runtime::proxy_object::ProxyObject;
use crate::runtime::shadow_realm_object::ShadowRealmObject;
use crate::runtime::structure::StructureRef;
use crate::runtime::vm::VM;
use crate::wtf::text::conversion_mode::ConversionMode;
use crate::wtf::text::wtf_string::{String as WtfString};

/// `const ClassInfo JSRemoteFunction::s_info`.
pub static JS_REMOTE_FUNCTION_S_INFO: ClassInfo =
    ClassInfo { class_name: "Function", parent_class: Some(&JS_FUNCTION_S_INFO), static_prop_hash_table: None, inherits_js_type_range: None };

/// A mensagem do `TypeError` de `wrapArgument` e `wrapReturnValue`.
const NOT_CALLABLE_OR_PRIMITIVE_MESSAGE: &str = "value passing between realms must be callable or primitive";

/// Os campos de `class JSRemoteFunction final : public JSFunction`.
pub struct JSRemoteFunction {
    /// `m_targetFunction`: o `JSObject*` (sempre célula chamável, e nunca outra `JSRemoteFunction`).
    target_function: JSValue,
    /// `m_nameMayBeNull`: nunca é rope.
    name_may_be_null: Option<JSStringRef>,
    /// `m_length`.
    length: f64,
}

impl JSRemoteFunction {
    /// `visitChildren`: a função alvo.
    pub(crate) fn visit_children(&self, marker: &mut crate::runtime::gc::Marker) {
        marker.value(self.target_function);
    }
    /// `StructureFlags = Base::StructureFlags`.
    pub const STRUCTURE_FLAGS: u32 = JSFunction::STRUCTURE_FLAGS;

    /// `createStructure(vm, globalObject, prototype)`.
    pub fn create_structure(vm: &VM, global_object: &JSGlobalObject, prototype: JSValue) -> StructureRef {
        function_structure(vm, global_object, prototype, &JS_REMOTE_FUNCTION_S_INFO)
    }

    /// `tryCreate(globalObject, vm, targetCallable)`: a função sempre é devolvida; se `copyNameAndLength`
    /// lançou, o `TypeError` fica pendente no `VM` (quem chama confere).
    pub fn try_create(global_object: &JSGlobalObject, vm: &VM, target_callable: JSValue) -> JSFunctionRef {
        debug_assert!(target_callable.is_callable());
        let mut target = target_callable;
        if let Some(remote) = target.as_js_function().and_then(|function| function.as_remote_function().map(|remote| remote.target_function)) {
            target = remote;
            debug_assert!(!target.as_js_function().is_some_and(|function| function.is_remote_function()));
        }

        let is_js_function = target.as_js_function().is_some();
        let (native_function, intrinsic): (NativeFunction, Intrinsic) = if is_js_function {
            (remote_function_call_for_js_function, Intrinsic::RemoteFunctionCallIntrinsic)
        } else {
            (remote_function_call_generic, Intrinsic::NoIntrinsic)
        };
        let executable = vm.get_host_function_with_intrinsic(
            native_function,
            ImplementationVisibility::Public,
            intrinsic,
            call_host_function_as_constructor,
            0,
            &WtfString::default(),
        );
        let structure = global_object.remote_function_structure();

        let (length, name_may_be_null) = copy_name_and_length(global_object, target);
        let function = JSFunction::create_remote(vm, executable, structure, JSRemoteFunction { target_function: target, name_may_be_null, length });

        // `finishCreation`: qualquer exceção do `copyNameAndLength` vira um `TypeError`.
        if vm.exception().is_some() && !vm.has_pending_termination_exception() {
            vm.clear_exception();
            throw_thrown(global_object, Thrown::type_error("wrapping returned function throws an error"));
        }
        function
    }

    /// `targetFunction()`.
    pub fn target_function(&self) -> JSValue {
        self.target_function
    }

    /// `targetGlobalObject()`: o `realm()` do alvo.
    pub fn target_global_object(&self) -> JSGlobalObjectRef {
        realm_of(self.target_function)
    }

    /// `nameMayBeNull()`.
    pub fn name_may_be_null(&self) -> Option<JSStringRef> {
        self.name_may_be_null.clone()
    }

    /// `nameString()`.
    pub fn name_string(&self) -> WtfString {
        match &self.name_may_be_null {
            Some(name) => name.value(),
            None => WtfString::default(),
        }
    }

    /// `length(vm)`.
    pub fn length(&self) -> f64 {
        self.length
    }
}

/// `realm()` de um objeto chamável: o `scope` da função, ou o `globalObject` da `Structure` dos demais.
pub(crate) fn realm_of(target: JSValue) -> JSGlobalObjectRef {
    if let Some(function) = target.as_js_function() {
        return function.realm();
    }
    target.as_object().structure().realm().expect("objeto chamável sem realm na Structure")
}

/// `copyNameAndLength(globalObject)` (https://tc39.es/proposal-shadowrealm/#sec-copynameandlength): o
/// `m_length` e o `m_nameMayBeNull`. Se algo lança, a exceção fica pendente no `VM` e o resultado é o que
/// já tinha sido lido.
fn copy_name_and_length(global_object: &JSGlobalObject, target_function: JSValue) -> (f64, Option<JSStringRef>) {
    let vm = global_object.vm();
    // As conversões de `JSValue` (o `valueOf` do `length`) rodam no `globalObject` recebido.
    let _realm = CurrentRealmScope::enter(global_object);
    let target = target_function.as_object();
    let mut length = 0.0;

    let length_name = PropertyName::from_identifier(&vm.property_names.length);
    let mut slot = PropertySlot::new(target_function, InternalMethodType::GetOwnProperty);
    let target_has_length = match ProxyObject::from_cell_id(target.cell_id()) {
        // `getOwnPropertySlotInline` é virtual no C++: o `Proxy` responde pelo trap `getOwnPropertyDescriptor`.
        Some(proxy) => match proxy.get_own_property_slot(global_object, &length_name, &mut slot) {
            Ok(found) => found,
            Err(thrown) => {
                throw_thrown(global_object, thrown);
                false
            }
        },
        None => target.get_own_property_slot(global_object, &length_name, &mut slot),
    };
    if vm.exception().is_some() {
        return (length, None);
    }

    if target_has_length {
        let target_length = if !slot.is_tainted_by_opaque_object() {
            slot.get_value_for(&length_name)
        } else {
            target.get(global_object, &length_name)
        };
        if vm.exception().is_some() {
            return (length, None);
        }
        let Ok(target_length_as_int) = target_length.to_integer_or_infinity_checked() else {
            return (length, None);
        };
        length = target_length_as_int.max(0.0);
    }

    let target_name = target.get(global_object, &PropertyName::from_identifier(&vm.property_names.name));
    if vm.exception().is_some() {
        return (length, None);
    }
    if target_name.is_string() {
        let target_string = target_name.as_js_string();
        // Resolving rope.
        let _ = target_string.value();
        if vm.exception().is_some() {
            return (length, None);
        }
        return (length, Some(target_string));
    }
    (length, None)
}

/// `wrapValue`, `wrapArgument` e `wrapReturnValue`: o primitivo passa, o chamável vira uma
/// `JSRemoteFunction` do reino `target_global_object`, e o resto lança. `Err(Pending)` é a exceção que a
/// criação deixou pendente.
fn wrap_value(global_object: &JSGlobalObject, target_global_object: &JSGlobalObject, value: JSValue) -> Result<JSValue, Thrown> {
    let vm = global_object.vm();
    let wrapped = if !value.is_object() {
        Some(value)
    } else if value.is_callable() {
        Some(JSRemoteFunction::try_create(target_global_object, vm, value).as_value())
    } else {
        None
    };
    pending_or(global_object, ())?;
    wrapped.ok_or_else(|| Thrown::type_error(NOT_CALLABLE_OR_PRIMITIVE_MESSAGE))
}

/// A exceção que escapa do alvo atravessa a fronteira como um `TypeError` do reino de quem chama (o
/// `GetWrappedValue`/`CrossRealmThrow` da proposta do `ShadowRealm`, medido no bun): primitivo vira a própria
/// conversão em string (`Symbol` inclusive cai na mensagem padrão), instância de `Error` leva a mensagem
/// dela, e qualquer outro objeto (`Proxy` e literal com `message` também) leva "Type error".
fn cross_realm_throw(global_object: &JSGlobalObject, failure: crate::llint::LLIntFailure) -> Thrown {
    let vm = global_object.vm();
    if !matches!(failure, crate::llint::LLIntFailure::Thrown) || vm.has_pending_termination_exception() {
        return thrown_from_llint(failure);
    }
    let Some(exception) = vm.exception() else { return thrown_from_llint(failure) };
    let error = exception.value();
    vm.clear_exception();

    let message = if let Some(instance) = error.is_object().then(|| error.as_object()).and_then(|object| ErrorInstance::from_cell_id(object.cell_id())) {
        String::from_utf8_lossy(&instance.message().utf8(ConversionMode::LenientConversion)).into_owned()
    } else if !error.is_object() && !error.is_symbol() {
        String::from_utf8_lossy(&error.to_wtf_string().utf8(ConversionMode::LenientConversion)).into_owned()
    } else {
        "Type error".to_string()
    };
    Thrown::TypeError(if message.is_empty() { "Type error".to_string() } else { message })
}

/// O corpo de `remoteFunctionCallForJSFunction` (`wrap_return_in_target_realm` falso) e de
/// `remoteFunctionCallGeneric` (verdadeiro): é a única diferença que o C++ tem entre os dois.
fn call_remote_function(global_object: &JSGlobalObject, call: &HostCall, wrap_return_in_target_realm: bool) -> HostResult {
    let callee = JSValue::from_cell(call.callee()).as_js_function().expect("callee de remoteFunctionCall que não é JSFunction");
    let remote = callee.as_remote_function().expect("remoteFunctionCall com callee que não é JSRemoteFunction");
    let target = remote.target_function();
    let target_global_object = remote.target_global_object();

    let mut args = Vec::with_capacity(call.argument_count());
    for &argument in call.arguments() {
        args.push(wrap_value(global_object, &target_global_object, argument)?);
    }

    let call_data = get_call_data(target);
    debug_assert!(!call_data.is_none());
    let result = match crate::runtime::call_data::call(&target_global_object, target, &call_data, js_undefined(), &args) {
        Ok(result) => result,
        Err(failure) => return Err(cross_realm_throw(global_object, failure)),
    };

    let return_realm: &JSGlobalObject = if wrap_return_in_target_realm { &target_global_object } else { global_object };
    wrap_value(global_object, return_realm, result)
}

fn remote_function_call_for_js_function_body(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    call_remote_function(global_object, call, false)
}
host_function!(pub remote_function_call_for_js_function, remote_function_call_for_js_function_body);

fn remote_function_call_generic_body(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    call_remote_function(global_object, call, true)
}
host_function!(pub remote_function_call_generic, remote_function_call_generic_body);

/// `isRemoteFunction(value)`.
fn is_remote_function_body(_global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    debug_assert!(call.argument_count() == 1);
    Ok(js_boolean(call.argument(0).as_js_function().is_some_and(|function| function.is_remote_function())))
}
host_function!(pub is_remote_function, is_remote_function_body);

/// `createRemoteFunction(targetFunction, shadowRealmOrNull)`: o destino é o `JSGlobalObject` do
/// `ShadowRealm`, ou o de quem chama quando o segundo argumento é `undefined` ou `null`.
fn create_remote_function_body(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    debug_assert!(call.argument_count() == 2);
    let target_function = call.argument(0);
    debug_assert!(target_function.is_callable());

    let destination_argument = call.argument(1);
    let destination_holder: JSGlobalObjectRef;
    let destination: &JSGlobalObject = if destination_argument.is_undefined_or_null() {
        global_object
    } else {
        destination_holder = match ShadowRealmObject::from_value(&destination_argument) {
            Some(shadow_realm) => shadow_realm.global_object(),
            None => global_object_from_value(destination_argument),
        };
        &destination_holder
    };

    let function = JSRemoteFunction::try_create(destination, global_object.vm(), target_function);
    pending_or(global_object, function.as_value())
}
host_function!(pub create_remote_function, create_remote_function_body);

/// `uncheckedDowncast<JSGlobalObject>(value)`.
fn global_object_from_value(value: JSValue) -> JSGlobalObjectRef {
    let JSValue::Cell(cell_id) = value else {
        panic!("createRemoteFunction: o argumento 1 não é uma célula");
    };
    match cell_registry::get(cell_id) {
        Some(CellEntry::Scope(JSScopeRef::GlobalObject(global_object))) => Rc::clone(&global_object),
        _ => panic!("createRemoteFunction: o argumento 1 não é um JSGlobalObject"),
    }
}
