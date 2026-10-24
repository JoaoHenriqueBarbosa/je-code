//! Porte de `runtime/WeakSetPrototype.{h,cpp}`: o `WeakSet.prototype` (um `JSNonFinalObject` com o
//! `ClassInfo` `"WeakSet"`) e as funções nativas, sobre os algoritmos puros de `js_weak_set.rs`.

use crate::host_function;
use crate::runtime::class_info::ClassInfo;
use crate::runtime::collection_support::{put_native_function, put_to_string_tag, run_collection};
use crate::runtime::host_call::{HostCall, HostResult};
use crate::runtime::intrinsic::Intrinsic;
use crate::runtime::js_global_object::JSGlobalObject;
use crate::runtime::js_object::{JSNonFinalObject, JSObject, JSObjectRef, JS_NON_FINAL_OBJECT_S_INFO};
use crate::runtime::js_type::JSType;
use crate::runtime::js_type_info::TypeInfo;
use crate::runtime::js_value::JSValue;
use crate::runtime::js_weak_set::{weak_set_proto_add, weak_set_proto_delete, weak_set_proto_has};
use crate::runtime::structure::{Structure, StructureRef};
use crate::runtime::vm::VM;

/// `const ClassInfo WeakSetPrototype::s_info`.
pub static WEAK_SET_PROTOTYPE_S_INFO: ClassInfo =
    ClassInfo { class_name: "WeakSet", parent_class: Some(&JS_NON_FINAL_OBJECT_S_INFO), static_prop_hash_table: None, inherits_js_type_range: None };

fn weak_set_delete_body(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    run_collection(global_object, || Ok(weak_set_proto_delete(call.this_value(), call.argument(0))?))
}

fn weak_set_has_body(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    run_collection(global_object, || Ok(weak_set_proto_has(call.this_value(), call.argument(0))?))
}

fn weak_set_add_body(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    run_collection(global_object, || Ok(weak_set_proto_add(call.this_value(), call.argument(0))?))
}

host_function!(weak_set_proto_func_delete, weak_set_delete_body);
host_function!(weak_set_proto_func_has, weak_set_has_body);
host_function!(weak_set_proto_func_add, weak_set_add_body);

/// `class WeakSetPrototype final : public JSNonFinalObject`: sem campos próprios.
pub struct WeakSetPrototype;

impl WeakSetPrototype {
    /// `StructureFlags = Base::StructureFlags`.
    pub const STRUCTURE_FLAGS: u32 = JSNonFinalObject::STRUCTURE_FLAGS;

    /// `createStructure(vm, globalObject, prototype)`.
    pub fn create_structure(vm: &VM, global_object: &JSGlobalObject, prototype: JSValue) -> StructureRef {
        Structure::create(
            vm,
            Some(global_object),
            prototype,
            TypeInfo::new(JSType::ObjectType, WeakSetPrototype::STRUCTURE_FLAGS),
            &WEAK_SET_PROTOTYPE_S_INFO,
        )
    }

    /// `create(vm, globalObject, structure)`: `WeakSetPrototype(vm, structure)` e `finishCreation`.
    pub fn create(vm: &VM, global_object: &JSGlobalObject, structure: &StructureRef) -> JSObjectRef {
        let prototype = JSObject::allocate(vm, structure);
        WeakSetPrototype::finish_creation(&prototype, vm, global_object);
        prototype
    }

    /// `finishCreation(vm, globalObject)`.
    fn finish_creation(prototype: &JSObject, vm: &VM, global_object: &JSGlobalObject) {
        prototype.finish_creation(vm);

        let names = &vm.property_names;
        put_native_function(vm, global_object, prototype, &names.delete_keyword, 1, weak_set_proto_func_delete, Intrinsic::NoIntrinsic);
        put_native_function(vm, global_object, prototype, &names.has, 1, weak_set_proto_func_has, Intrinsic::JSWeakSetHasIntrinsic);
        put_native_function(vm, global_object, prototype, &names.add, 1, weak_set_proto_func_add, Intrinsic::JSWeakSetAddIntrinsic);

        put_to_string_tag(vm, prototype, WEAK_SET_PROTOTYPE_S_INFO.class_name);
    }
}
