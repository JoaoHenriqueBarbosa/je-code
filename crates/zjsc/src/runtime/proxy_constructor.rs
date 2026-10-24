//! Porte de `runtime/ProxyConstructor.{h,cpp}` e `ProxyConstructorInlines.h`: o construtor `Proxy` (no bun um
//! `JSFunction` sobre `NativeExecutable`, não `InternalFunction`) com `new Proxy(target, handler)` e `Proxy.revocable(target, handler)`.
//!
//! DIVERGÊNCIAS: `HasStaticPropertyTable` não existe; `revocable` entra por `put_direct` (como no
//! `SymbolConstructor`). `getGetter` (declarada sem corpo no `.h`) não existe.

use crate::host_function;
use crate::runtime::class_info::ClassInfo;
use crate::runtime::host_call::{HostCall, HostResult, Thrown};
use crate::runtime::identifier::Identifier;
use crate::runtime::implementation_visibility::ImplementationVisibility;
use crate::runtime::intrinsic::Intrinsic;
use crate::runtime::js_function::{put_direct_native_function_without_transition, JSFunction, JSFunctionRef, JS_FUNCTION_S_INFO};
use crate::runtime::js_global_object::JSGlobalObject;
use crate::runtime::js_object::JSFinalObject;
use crate::runtime::js_type::JSType;
use crate::runtime::js_type_info::TypeInfo;
use crate::runtime::js_value::JSValue;
use crate::runtime::property_attribute::DONT_ENUM;
use crate::runtime::property_name::PropertyName;
use crate::runtime::proxy_object::ProxyObject;
use crate::runtime::proxy_revoke::ProxyRevoke;
use crate::runtime::structure::{Structure, StructureRef};
use crate::runtime::vm::VM;
use crate::wtf::text::wtf_string::String as WtfString;

/// `const ClassInfo ProxyConstructor::s_info`.
pub static PROXY_CONSTRUCTOR_S_INFO: ClassInfo =
    ClassInfo { class_name: "Proxy", parent_class: Some(&JS_FUNCTION_S_INFO), static_prop_hash_table: None, inherits_js_type_range: None };

/// `makeRevocableProxy`: `Proxy.revocable(target, handler)` devolve `{ proxy, revoke }`.
fn make_revocable_proxy_body(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    let vm = global_object.vm();
    if call.argument_count() < 2 {
        return Err(Thrown::type_error("Proxy.revocable needs to be called with two arguments: the target and the handler"));
    }

    let proxy = ProxyObject::create(global_object, call.argument(0), call.argument(1))?;
    let revoke = ProxyRevoke::create(vm, &global_object.proxy_revoke_structure(), &proxy);

    let result = JSFinalObject::create(vm, &global_object.object_structure_for_object_constructor());
    result.put_direct(vm, &PropertyName::from_identifier(&Identifier::from_span(vm, b"proxy")), proxy.as_value(), 0);
    result.put_direct(vm, &PropertyName::from_identifier(&Identifier::from_span(vm, b"revoke")), revoke.as_value(), 0);
    Ok(result.as_value())
}

/// `constructProxyObject`: `new Proxy(target, handler)`.
fn construct_proxy_object_body(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    Ok(ProxyObject::create(global_object, call.argument(0), call.argument(1))?.as_value())
}

/// `callProxy`: `throwConstructorCannotBeCalledAsFunctionTypeError(globalObject, scope, "Proxy")`.
fn call_proxy_body(_global_object: &JSGlobalObject, _call: &HostCall) -> HostResult {
    Err(Thrown::type_error("calling Proxy constructor without new is invalid"))
}

host_function!(make_revocable_proxy, make_revocable_proxy_body);
host_function!(construct_proxy_object, construct_proxy_object_body);
host_function!(call_proxy, call_proxy_body);

/// `class ProxyConstructor final : public InternalFunction`: sem campos próprios.
pub struct ProxyConstructor;

impl ProxyConstructor {
    /// `StructureFlags = Base::StructureFlags`.
    pub const STRUCTURE_FLAGS: u32 = JSFunction::STRUCTURE_FLAGS;

    /// `createStructure(vm, globalObject, prototype)` (`ProxyConstructorInlines.h`).
    pub fn create_structure(vm: &VM, global_object: Option<&JSGlobalObject>, prototype: JSValue) -> StructureRef {
        Structure::create(
            vm,
            global_object,
            prototype,
            TypeInfo::new(JSType::JSFunctionType, ProxyConstructor::STRUCTURE_FLAGS),
            &PROXY_CONSTRUCTOR_S_INFO,
        )
    }

    /// `create(vm, structure)`: o construtor e o `finishCreation(vm, structure->realm())`.
    pub fn create(vm: &VM, structure: StructureRef) -> JSFunctionRef {
        let global_object = structure.realm().expect("ProxyConstructor sem realm na Structure");
        // No bun `Proxy` é um `JSFunction` sobre `NativeExecutable`: `length` e `name` preguiçosos, antes de
        // `revocable` no `Reflect.ownKeys`, e sem `prototype`.
        let constructor = JSFunction::create_native_with_structure(
            vm,
            &global_object,
            structure,
            2,
            &WtfString::from_latin1(b"Proxy"),
            call_proxy,
            ImplementationVisibility::Public,
            Intrinsic::NoIntrinsic,
            construct_proxy_object,
        );
        put_direct_native_function_without_transition(
            vm,
            &global_object,
            &constructor,
            &Identifier::from_span(vm, b"revocable"),
            2,
            make_revocable_proxy,
            ImplementationVisibility::Public,
            Intrinsic::NoIntrinsic,
            DONT_ENUM,
        );
        constructor
    }
}
