//! Porte de `runtime/FinalizationRegistryPrototype.{h,cpp}` e `FinalizationRegistryPrototypeInlines.h`: o
//! `FinalizationRegistry.prototype` (um `JSNonFinalObject` com o `ClassInfo` `"FinalizationRegistry"`),
//! `register`, `unregister` e `@@toStringTag`. O JavaScriptCore deste commit não define `cleanupSome`,
//! então ele não existe aqui.

use crate::host_function;
use crate::runtime::class_info::ClassInfo;
use crate::runtime::collection_support::{put_native_function, put_to_string_tag, thrown_from_failure, CollectionFailure};
use crate::runtime::host_call::{HostCall, HostResult, Thrown};
use crate::runtime::identifier::Identifier;
use crate::runtime::intrinsic::Intrinsic;
use crate::runtime::js_finalization_registry::{JSFinalizationRegistry, JSFinalizationRegistryRef};
use crate::runtime::js_global_object::JSGlobalObject;
use crate::runtime::js_object::{JSNonFinalObject, JSObject, JSObjectRef, JS_NON_FINAL_OBJECT_S_INFO};
use crate::runtime::js_type::JSType;
use crate::runtime::js_type_info::TypeInfo;
use crate::runtime::js_value::{js_boolean, JSValue};
use crate::runtime::js_weak_map::{can_be_held_weakly, get_weak_receiver};
use crate::runtime::structure::{Structure, StructureRef};
use crate::runtime::vm::VM;

/// `const ClassInfo FinalizationRegistryPrototype::s_info`.
pub static FINALIZATION_REGISTRY_PROTOTYPE_S_INFO: ClassInfo = ClassInfo {
    class_name: "FinalizationRegistry",
    parent_class: Some(&JS_NON_FINAL_OBJECT_S_INFO),
    static_prop_hash_table: None, inherits_js_type_range: None,
};

const NOT_AN_OBJECT_MESSAGE: &str = "Called FinalizationRegistry function on non-object";
const WRONG_RECEIVER_MESSAGE: &str = "Called FinalizationRegistry function on a non-FinalizationRegistry object";

/// `getFinalizationRegistry(vm, globalObject, value)`.
fn get_finalization_registry(global_object: &JSGlobalObject, this_value: JSValue) -> Result<JSFinalizationRegistryRef, Thrown> {
    get_weak_receiver(this_value, JSFinalizationRegistry::from_value, NOT_AN_OBJECT_MESSAGE, WRONG_RECEIVER_MESSAGE)
        .map_err(|error| thrown_from_failure(global_object, CollectionFailure::from(error)))
}

/// `protoFuncFinalizationRegistryRegister`.
fn finalization_registry_register_body(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    let registry = get_finalization_registry(global_object, call.this_value())?;

    let target = call.argument(0);
    if !can_be_held_weakly(target) {
        return Err(Thrown::type_error("register requires an object or a non-registered symbol as the target"));
    }

    let holdings = call.argument(1);
    if target == holdings {
        return Err(Thrown::type_error(
            "register expects the target object and the holdings parameter are not the same. Otherwise, the target can never be collected",
        ));
    }

    let unregister_token = call.argument(2);
    if !unregister_token.is_undefined() && !can_be_held_weakly(unregister_token) {
        return Err(Thrown::type_error("register requires an object or a non-registered symbol as the unregistration token"));
    }

    registry.register_target(target, holdings, unregister_token);
    Ok(JSValue::Undefined)
}

/// `protoFuncFinalizationRegistryUnregister`.
fn finalization_registry_unregister_body(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    let registry = get_finalization_registry(global_object, call.this_value())?;

    let token = call.argument(0);
    if !can_be_held_weakly(token) {
        return Err(Thrown::type_error("unregister requires an object or a non-registered symbol as the unregistration token"));
    }

    Ok(js_boolean(registry.unregister(token)))
}

host_function!(proto_func_finalization_registry_register, finalization_registry_register_body);
host_function!(proto_func_finalization_registry_unregister, finalization_registry_unregister_body);

/// `class FinalizationRegistryPrototype final : public JSNonFinalObject`: sem campos próprios.
pub struct FinalizationRegistryPrototype;

impl FinalizationRegistryPrototype {
    /// `StructureFlags = Base::StructureFlags`.
    pub const STRUCTURE_FLAGS: u32 = JSNonFinalObject::STRUCTURE_FLAGS;

    /// `createStructure(vm, globalObject, prototype)`.
    pub fn create_structure(vm: &VM, global_object: &JSGlobalObject, prototype: JSValue) -> StructureRef {
        Structure::create(
            vm,
            Some(global_object),
            prototype,
            TypeInfo::new(JSType::ObjectType, FinalizationRegistryPrototype::STRUCTURE_FLAGS),
            &FINALIZATION_REGISTRY_PROTOTYPE_S_INFO,
        )
    }

    /// `create(vm, globalObject, structure)`: `FinalizationRegistryPrototype(vm, structure)` e `finishCreation`.
    pub fn create(vm: &VM, global_object: &JSGlobalObject, structure: &StructureRef) -> JSObjectRef {
        let prototype = JSObject::allocate(vm, structure);
        FinalizationRegistryPrototype::finish_creation(&prototype, vm, global_object);
        prototype
    }

    /// `finishCreation(vm, globalObject)`: `register` (comprimento 2), `unregister` (1) e `@@toStringTag`.
    fn finish_creation(prototype: &JSObject, vm: &VM, global_object: &JSGlobalObject) {
        prototype.finish_creation(vm);
        let register = Identifier::from_span(vm, b"register".as_slice());
        let unregister = Identifier::from_span(vm, b"unregister".as_slice());
        put_native_function(vm, global_object, prototype, &register, 2, proto_func_finalization_registry_register, Intrinsic::NoIntrinsic);
        put_native_function(vm, global_object, prototype, &unregister, 1, proto_func_finalization_registry_unregister, Intrinsic::NoIntrinsic);
        put_to_string_tag(vm, prototype, FINALIZATION_REGISTRY_PROTOTYPE_S_INFO.class_name);
    }
}
