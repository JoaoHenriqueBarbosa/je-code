//! Porte de `runtime/AsyncDisposableStackPrototype.{h,cpp}` e `AsyncDisposableStackPrototypeInlines.h`: o
//! `AsyncDisposableStack.prototype` (um `JSNonFinalObject`) com `adopt`, `defer`, `disposeAsync` (também
//! sob `@@asyncDispose`), `move` e `use` (builtins JS, `AsyncDisposableStackPrototype.js`), o getter
//! `disposed` e `@@toStringTag`.

use crate::host_function;
use crate::runtime::array_buffer_prototype::put_native_getter;
use crate::runtime::builtins_source::BuiltinCodeIndex;
use crate::runtime::class_info::ClassInfo;
use crate::runtime::collection_support::put_to_string_tag;
use crate::runtime::host_call::{HostCall, HostResult, Thrown};
use crate::runtime::identifier::Identifier;
use crate::runtime::intrinsic::Intrinsic;
use crate::runtime::js_async_disposable_stack::JSAsyncDisposableStack;
use crate::runtime::js_function::put_direct_builtin_function_without_transition;
use crate::runtime::js_global_object::JSGlobalObject;
use crate::runtime::js_object::{JSNonFinalObject, JSObject, JSObjectRef, JS_NON_FINAL_OBJECT_S_INFO};
use crate::runtime::js_type::JSType;
use crate::runtime::js_type_info::TypeInfo;
use crate::runtime::js_value::{js_boolean, JSValue};
use crate::runtime::property_attribute::DONT_ENUM;
use crate::runtime::property_name::PropertyName;
use crate::runtime::structure::{Structure, StructureRef};
use crate::runtime::vm::VM;

/// `const ClassInfo AsyncDisposableStackPrototype::s_info`.
pub static ASYNC_DISPOSABLE_STACK_PROTOTYPE_S_INFO: ClassInfo = ClassInfo {
    class_name: "AsyncDisposableStack",
    parent_class: Some(&JS_NON_FINAL_OBJECT_S_INFO),
    static_prop_hash_table: None, inherits_js_type_range: None,
};

/// `asyncDisposableStackProtoDisposedGetter`. A mensagem diz `DisposableStack` mesmo aqui, como no C++.
fn async_disposable_stack_proto_disposed_getter_body(_global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    match JSAsyncDisposableStack::from_value(&call.this_value()) {
        Some(stack) => Ok(js_boolean(stack.disposed())),
        None => Err(Thrown::type_error(
            "AsyncDisposableStack.prototype.disposed getter requires that |this| be a DisposableStack object",
        )),
    }
}

host_function!(async_disposable_stack_proto_disposed_getter, async_disposable_stack_proto_disposed_getter_body);

/// `class AsyncDisposableStackPrototype final : public JSNonFinalObject`: sem campos próprios.
pub struct AsyncDisposableStackPrototype;

impl AsyncDisposableStackPrototype {
    /// `StructureFlags = Base::StructureFlags`.
    pub const STRUCTURE_FLAGS: u32 = JSNonFinalObject::STRUCTURE_FLAGS;

    /// `createStructure(vm, globalObject, prototype)`.
    pub fn create_structure(vm: &VM, global_object: &JSGlobalObject, prototype: JSValue) -> StructureRef {
        Structure::create(
            vm,
            Some(global_object),
            prototype,
            TypeInfo::new(JSType::ObjectType, AsyncDisposableStackPrototype::STRUCTURE_FLAGS),
            &ASYNC_DISPOSABLE_STACK_PROTOTYPE_S_INFO,
        )
    }

    /// `create(vm, globalObject, structure)`: `AsyncDisposableStackPrototype(vm, structure)` e `finishCreation`.
    pub fn create(vm: &VM, global_object: &JSGlobalObject, structure: &StructureRef) -> JSObjectRef {
        let prototype = JSObject::allocate(vm, structure);
        AsyncDisposableStackPrototype::finish_creation(&prototype, vm, global_object);
        prototype
    }

    /// `finishCreation(vm, globalObject)`: os builtins `DontEnum`, o getter `disposed` e `@@asyncDispose`
    /// igual a `disposeAsync`, na ordem do C++.
    fn finish_creation(prototype: &JSObject, vm: &VM, global_object: &JSGlobalObject) {
        prototype.finish_creation(vm);
        let put_builtin = |name: &Identifier, index: BuiltinCodeIndex| {
            put_direct_builtin_function_without_transition(vm, global_object, prototype, name, index, DONT_ENUM)
        };
        put_builtin(&vm.property_names.adopt, BuiltinCodeIndex::AsyncDisposableStackPrototypeAdoptCode);
        put_builtin(&vm.property_names.defer_keyword, BuiltinCodeIndex::AsyncDisposableStackPrototypeDeferMethodCode);
        let dispose_async_function =
            put_builtin(&vm.property_names.dispose_async, BuiltinCodeIndex::AsyncDisposableStackPrototypeDisposeAsyncCode);
        put_native_getter(
            vm,
            global_object,
            prototype,
            "disposed",
            async_disposable_stack_proto_disposed_getter,
            Intrinsic::NoIntrinsic,
            DONT_ENUM,
        );
        prototype.put_direct_without_transition(
            vm,
            &PropertyName::from_identifier(&vm.property_names.async_dispose_symbol),
            dispose_async_function.as_value(),
            DONT_ENUM,
        );
        put_builtin(&vm.property_names.r#move, BuiltinCodeIndex::AsyncDisposableStackPrototypeMoveCode);
        put_builtin(&vm.property_names.r#use, BuiltinCodeIndex::AsyncDisposableStackPrototypeUseCode);
        // `JSC_TO_STRING_TAG_WITHOUT_TRANSITION()`.
        put_to_string_tag(vm, prototype, ASYNC_DISPOSABLE_STACK_PROTOTYPE_S_INFO.class_name);
    }
}
