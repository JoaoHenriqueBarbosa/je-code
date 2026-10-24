//! Porte de `runtime/WeakObjectRefPrototype.{h,cpp}` e `WeakObjectRefPrototypeInlines.h`: o
//! `WeakRef.prototype` (um `JSNonFinalObject` com o `ClassInfo` `"WeakRef"`), `deref` e `@@toStringTag`.

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
use crate::runtime::js_weak_map::get_weak_receiver;
use crate::runtime::js_weak_object_ref::JSWeakObjectRef;
use crate::runtime::structure::{Structure, StructureRef};
use crate::runtime::vm::VM;

/// `const ClassInfo WeakObjectRefPrototype::s_info`.
pub static WEAK_OBJECT_REF_PROTOTYPE_S_INFO: ClassInfo =
    ClassInfo { class_name: "WeakRef", parent_class: Some(&JS_NON_FINAL_OBJECT_S_INFO), static_prop_hash_table: None, inherits_js_type_range: None };

const WEAK_REF_NOT_AN_OBJECT_MESSAGE: &str = "Called WeakRef function on non-object";
const WEAK_REF_WRONG_RECEIVER_MESSAGE: &str = "Called WeakRef function on a non-WeakRef object";

/// `protoFuncWeakRefDeref`: `getWeakRef(globalObject, thisValue)` e o alvo (ou `undefined`).
fn weak_ref_deref_body(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    run_collection(global_object, || {
        let reference = get_weak_receiver(
            call.this_value(),
            JSWeakObjectRef::from_value,
            WEAK_REF_NOT_AN_OBJECT_MESSAGE,
            WEAK_REF_WRONG_RECEIVER_MESSAGE,
        )?;
        Ok(reference.deref_target(global_object.vm()))
    })
}

host_function!(weak_ref_proto_func_deref, weak_ref_deref_body);

/// `class WeakObjectRefPrototype final : public JSNonFinalObject`: sem campos próprios.
pub struct WeakObjectRefPrototype;

impl WeakObjectRefPrototype {
    /// `StructureFlags = Base::StructureFlags`.
    pub const STRUCTURE_FLAGS: u32 = JSNonFinalObject::STRUCTURE_FLAGS;

    /// `createStructure(vm, globalObject, prototype)`.
    pub fn create_structure(vm: &VM, global_object: &JSGlobalObject, prototype: JSValue) -> StructureRef {
        Structure::create(
            vm,
            Some(global_object),
            prototype,
            TypeInfo::new(JSType::ObjectType, WeakObjectRefPrototype::STRUCTURE_FLAGS),
            &WEAK_OBJECT_REF_PROTOTYPE_S_INFO,
        )
    }

    /// `create(vm, globalObject, structure)`: `WeakObjectRefPrototype(vm, structure)` e `finishCreation`.
    pub fn create(vm: &VM, global_object: &JSGlobalObject, structure: &StructureRef) -> JSObjectRef {
        let prototype = JSObject::allocate(vm, structure);
        WeakObjectRefPrototype::finish_creation(&prototype, vm, global_object);
        prototype
    }

    /// `finishCreation(vm, globalObject)`: `deref` (`DontEnum`, comprimento 0) e `@@toStringTag`.
    fn finish_creation(prototype: &JSObject, vm: &VM, global_object: &JSGlobalObject) {
        prototype.finish_creation(vm);
        put_native_function(vm, global_object, prototype, &vm.property_names.deref, 0, weak_ref_proto_func_deref, Intrinsic::NoIntrinsic);
        put_to_string_tag(vm, prototype, WEAK_OBJECT_REF_PROTOTYPE_S_INFO.class_name);
    }
}
