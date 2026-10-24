//! Porte de `runtime/BooleanConstructor.h`, `BooleanConstructorInlines.h` e `BooleanConstructor.cpp`: o
//! construtor `Boolean` (no C++ um `JSFunction` sobre um `NativeExecutable` com `call` e `construct`
//! próprios) e `constructBooleanFromImmediateBoolean` (em `boolean_object.rs`).
//!
//! A estrutura é própria (`JSFunction::create_native_with_own_structure`), como a do
//! `BooleanConstructor::createStructure`: o `put_direct` do `prototype` não transita a `hostFunctionStructure`
//! compartilhada, que no bun segue folha.

use crate::runtime::host_call::{HostCall, HostResult};
use crate::runtime::implementation_visibility::ImplementationVisibility;
use crate::runtime::intrinsic::Intrinsic;
use crate::runtime::internal_function::get_derived_structure_in_realm;
use crate::runtime::class_info::ClassInfo;
use crate::runtime::js_function::{function_structure, JSFunction, JSFunctionRef, JS_FUNCTION_S_INFO};
use crate::runtime::js_global_object::JSGlobalObject;
use crate::runtime::js_value::js_boolean;
use crate::runtime::js_wrapper_object::JSWrapperObject;
use crate::runtime::property_attribute::{DONT_DELETE, DONT_ENUM, READ_ONLY};
use crate::runtime::property_name::PropertyName;
use crate::runtime::vm::VM;

// ECMA 15.6.1
fn call_boolean_constructor_body(_global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    Ok(js_boolean(call.argument(0).to_boolean()))
}

// ECMA 15.6.2
fn construct_with_boolean_constructor_body(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    let vm = global_object.vm();
    let boolean = js_boolean(call.argument(0).to_boolean());

    let boolean_structure = get_derived_structure_in_realm(global_object, call.new_target(), call.callee(), |realm| {
        realm.boolean_object_structure()
    })?;

    let object = JSWrapperObject::create(vm, boolean_structure);
    object.set_internal_value(boolean);
    Ok(object.as_value())
}

crate::host_function!(call_boolean_constructor, call_boolean_constructor_body);
crate::host_function!(construct_with_boolean_constructor, construct_with_boolean_constructor_body);

/// `const ClassInfo BooleanConstructor::s_info` (`"Function"`, base `JSFunction`).
pub static BOOLEAN_CONSTRUCTOR_S_INFO: ClassInfo =
    ClassInfo { class_name: "Function", parent_class: Some(&JS_FUNCTION_S_INFO), static_prop_hash_table: None, inherits_js_type_range: None };

/// `class BooleanConstructor final : public JSFunction`: espaço de nomes de `create`.
pub struct BooleanConstructor;

impl BooleanConstructor {
    /// `create(vm, structure, booleanPrototype)`: o `NativeExecutable` com `callBooleanConstructor` e
    /// `constructWithBooleanConstructor` (comprimento 1, nome `Boolean`) e o `finishCreation`.
    pub fn create(vm: &VM, global_object: &JSGlobalObject, boolean_prototype: &JSWrapperObject) -> JSFunctionRef {
        let prototype = global_object.host_function_structure().stored_prototype();
        let structure = function_structure(vm, global_object, prototype, &BOOLEAN_CONSTRUCTOR_S_INFO);
        let constructor = JSFunction::create_native_with_structure(
            vm,
            global_object,
            structure,
            1,
            vm.property_names.boolean.string().string(),
            call_boolean_constructor,
            ImplementationVisibility::Public,
            Intrinsic::BooleanConstructorIntrinsic,
            construct_with_boolean_constructor,
        );
        constructor.put_direct(
            vm,
            &PropertyName::from_identifier(&vm.property_names.prototype),
            boolean_prototype.as_value(),
            READ_ONLY | DONT_ENUM | DONT_DELETE,
        );
        constructor
    }
}
