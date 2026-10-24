//! Porte de `runtime/DisposableStackConstructor.{h,cpp}` e `DisposableStackConstructorInlines.h`: o
//! construtor `DisposableStack` (um `InternalFunction`, comprimento 0).
//!
//! A casca (`ClassInfo`, `createStructure`, `finishCreation`) é a de `collection_support.rs`; a estrutura
//! base vem do `prototype` do próprio construtor (ver a DIVERGÊNCIA de `collection_support.rs`).

use crate::host_function;
use crate::runtime::class_info::ClassInfo;
use crate::runtime::collection_support::{
    collection_constructor_structure, constructor_cannot_be_called_as_function, create_collection_constructor, derived_structure,
};
use crate::runtime::host_call::{HostCall, HostResult};
use crate::runtime::internal_function::{InternalFunctionRef, INTERNAL_FUNCTION_S_INFO};
use crate::runtime::js_array::construct_array;
use crate::runtime::js_disposable_stack::{Field, JSDisposableStack};
use crate::runtime::js_global_object::JSGlobalObject;
use crate::runtime::js_object::JSObject;
use crate::runtime::js_value::JSValue;
use crate::runtime::structure::StructureRef;
use crate::runtime::vm::VM;

/// `const ClassInfo DisposableStackConstructor::s_info` (`"Function"`).
pub static DISPOSABLE_STACK_CONSTRUCTOR_S_INFO: ClassInfo =
    ClassInfo { class_name: "Function", parent_class: Some(&INTERNAL_FUNCTION_S_INFO), static_prop_hash_table: None, inherits_js_type_range: None };

/// `callDisposableStack`: `throwConstructorCannotBeCalledAsFunctionTypeError`.
fn call_disposable_stack_body(_global_object: &JSGlobalObject, _call: &HostCall) -> HostResult {
    constructor_cannot_be_called_as_function("DisposableStack")
}

/// `constructDisposableStack`: a célula sobre a estrutura derivada e o array vazio de recursos no campo
/// `Capability`.
fn construct_disposable_stack_body(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    let vm = global_object.vm();
    let structure = derived_structure(global_object, call, JSDisposableStack::create_structure)?;
    let stack = JSDisposableStack::create(vm, &structure);
    let capability_array = construct_array(vm, &global_object.array_structure(), &[]);
    stack.set_internal_field(Field::Capability as u32, capability_array.as_value());
    Ok(stack.as_value())
}

host_function!(call_disposable_stack, call_disposable_stack_body);
host_function!(construct_disposable_stack, construct_disposable_stack_body);

/// `class DisposableStackConstructor final : public InternalFunction`: sem campos próprios.
pub struct DisposableStackConstructor;

impl DisposableStackConstructor {
    /// `createStructure(vm, globalObject, prototype)`.
    pub fn create_structure(vm: &VM, global_object: &JSGlobalObject, prototype: JSValue) -> StructureRef {
        collection_constructor_structure(vm, global_object, prototype, &DISPOSABLE_STACK_CONSTRUCTOR_S_INFO)
    }

    /// `create(vm, globalObject, structure, prototype)`: `finishCreation` com comprimento 0, nome
    /// `"DisposableStack"` e `prototype`.
    pub fn create(vm: &VM, global_object: &JSGlobalObject, structure: StructureRef, prototype: &JSObject) -> InternalFunctionRef {
        create_collection_constructor(
            vm,
            global_object,
            structure,
            prototype,
            "DisposableStack",
            0,
            call_disposable_stack,
            construct_disposable_stack,
            false,
        )
    }
}
