//! Porte de `runtime/WeakMapPrototype.{h,cpp}`: o `WeakMap.prototype` (um `JSNonFinalObject` com o
//! `ClassInfo` `"WeakMap"`) e as funções nativas, sobre os algoritmos puros de `js_weak_map.rs`.
//!
//! DIVERGÊNCIA: o `JSC_NATIVE_FUNCTION_WITHOUT_TRANSITION` do C++ não tem nome privado; as seis funções
//! entram na ordem do C++ (`delete`, `get`, `has`, `set`, `getOrInsert`, `getOrInsertComputed`).

use crate::host_function;
use crate::runtime::class_info::ClassInfo;
use crate::runtime::collection_support::{put_native_function, put_to_string_tag, run_collection, CollectionFailure};
use crate::runtime::host_call::{HostCall, HostResult, Thrown};
use crate::runtime::identifier::Identifier;
use crate::runtime::intrinsic::Intrinsic;
use crate::runtime::iterator_operations::call_checked;
use crate::runtime::js_global_object::JSGlobalObject;
use crate::runtime::js_object::{JSNonFinalObject, JSObject, JSObjectRef, JS_NON_FINAL_OBJECT_S_INFO};
use crate::runtime::js_type::JSType;
use crate::runtime::js_type_info::TypeInfo;
use crate::runtime::js_value::JSValue;
use crate::runtime::js_weak_map::{
    get_weak_map, can_be_held_weakly, weak_map_proto_delete, weak_map_proto_get, weak_map_proto_get_or_insert,
    weak_map_proto_get_or_insert_computed, weak_map_proto_has, weak_map_proto_set, WEAK_MAP_GET_OR_INSERT_COMPUTED_NOT_CALLABLE_MESSAGE,
    WEAK_MAP_INVALID_KEY_ERROR,
};
use crate::runtime::structure::{Structure, StructureRef};
use crate::runtime::vm::VM;

/// `const ClassInfo WeakMapPrototype::s_info`.
pub static WEAK_MAP_PROTOTYPE_S_INFO: ClassInfo =
    ClassInfo { class_name: "WeakMap", parent_class: Some(&JS_NON_FINAL_OBJECT_S_INFO), static_prop_hash_table: None, inherits_js_type_range: None };

fn weak_map_delete_body(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    run_collection(global_object, || Ok(weak_map_proto_delete(call.this_value(), call.argument(0))?))
}

fn weak_map_get_body(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    run_collection(global_object, || Ok(weak_map_proto_get(call.this_value(), call.argument(0))?))
}

fn weak_map_has_body(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    run_collection(global_object, || Ok(weak_map_proto_has(call.this_value(), call.argument(0))?))
}

fn weak_map_set_body(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    run_collection(global_object, || Ok(weak_map_proto_set(call.this_value(), call.argument(0), call.argument(1))?))
}

fn weak_map_get_or_insert_body(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    run_collection(global_object, || Ok(weak_map_proto_get_or_insert(call.this_value(), call.argument(0), call.argument(1))?))
}

/// `protoFuncWeakMapGetOrInsertComputed`: receptor, chave e só depois o `callback` (a ordem do C++).
fn weak_map_get_or_insert_computed_body(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    run_collection(global_object, || {
        get_weak_map(call.this_value())?;
        let key = call.argument(0);
        if !can_be_held_weakly(key) {
            return Err(Thrown::type_error(WEAK_MAP_INVALID_KEY_ERROR).into());
        }
        let callback = call.argument(1);
        if !callback.is_callable() {
            return Err(Thrown::type_error(WEAK_MAP_GET_OR_INSERT_COMPUTED_NOT_CALLABLE_MESSAGE).into());
        }
        weak_map_proto_get_or_insert_computed(call.this_value(), key, |key| -> Result<JSValue, CollectionFailure> {
            Ok(call_checked(
                global_object,
                callback,
                JSValue::undefined(),
                &[key],
                WEAK_MAP_GET_OR_INSERT_COMPUTED_NOT_CALLABLE_MESSAGE,
            )?)
        })
    })
}

host_function!(weak_map_proto_func_delete, weak_map_delete_body);
host_function!(weak_map_proto_func_get, weak_map_get_body);
host_function!(weak_map_proto_func_has, weak_map_has_body);
host_function!(weak_map_proto_func_set, weak_map_set_body);
host_function!(weak_map_proto_func_get_or_insert, weak_map_get_or_insert_body);
host_function!(weak_map_proto_func_get_or_insert_computed, weak_map_get_or_insert_computed_body);

/// `class WeakMapPrototype final : public JSNonFinalObject`: sem campos próprios.
pub struct WeakMapPrototype;

impl WeakMapPrototype {
    /// `StructureFlags = Base::StructureFlags`.
    pub const STRUCTURE_FLAGS: u32 = JSNonFinalObject::STRUCTURE_FLAGS;

    /// `createStructure(vm, globalObject, prototype)`.
    pub fn create_structure(vm: &VM, global_object: &JSGlobalObject, prototype: JSValue) -> StructureRef {
        Structure::create(
            vm,
            Some(global_object),
            prototype,
            TypeInfo::new(JSType::ObjectType, WeakMapPrototype::STRUCTURE_FLAGS),
            &WEAK_MAP_PROTOTYPE_S_INFO,
        )
    }

    /// `create(vm, globalObject, structure)`: `WeakMapPrototype(vm, structure)` e `finishCreation`.
    pub fn create(vm: &VM, global_object: &JSGlobalObject, structure: &StructureRef) -> JSObjectRef {
        let prototype = JSObject::allocate(vm, structure);
        WeakMapPrototype::finish_creation(&prototype, vm, global_object);
        prototype
    }

    /// `finishCreation(vm, globalObject)`.
    fn finish_creation(prototype: &JSObject, vm: &VM, global_object: &JSGlobalObject) {
        prototype.finish_creation(vm);

        let names = &vm.property_names;
        let get_or_insert = Identifier::from_span(vm, b"getOrInsert");
        let get_or_insert_computed = Identifier::from_span(vm, b"getOrInsertComputed");
        put_native_function(vm, global_object, prototype, &names.delete_keyword, 1, weak_map_proto_func_delete, Intrinsic::NoIntrinsic);
        put_native_function(vm, global_object, prototype, &names.get, 1, weak_map_proto_func_get, Intrinsic::JSWeakMapGetIntrinsic);
        put_native_function(vm, global_object, prototype, &names.has, 1, weak_map_proto_func_has, Intrinsic::JSWeakMapHasIntrinsic);
        put_native_function(vm, global_object, prototype, &names.set, 2, weak_map_proto_func_set, Intrinsic::JSWeakMapSetIntrinsic);
        put_native_function(vm, global_object, prototype, &get_or_insert, 2, weak_map_proto_func_get_or_insert, Intrinsic::NoIntrinsic);
        put_native_function(vm, global_object, prototype, &get_or_insert_computed, 2, weak_map_proto_func_get_or_insert_computed, Intrinsic::NoIntrinsic);

        put_to_string_tag(vm, prototype, WEAK_MAP_PROTOTYPE_S_INFO.class_name);
    }
}
