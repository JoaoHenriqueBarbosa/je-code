//! Porte de `runtime/DisposableStackPrototype.{h,cpp}` e `DisposableStackPrototypeInlines.h`: o
//! `DisposableStack.prototype` (um `JSNonFinalObject`) com `adopt`, `defer`, `dispose` (também sob
//! `@@dispose`), `use` e `move` (builtins JS, `DisposableStackPrototype.js`), o getter `disposed` e
//! `@@toStringTag`.

use crate::host_function;
use crate::runtime::builtins_source::BuiltinCodeIndex;
use crate::runtime::class_info::ClassInfo;
use crate::runtime::collection_support::put_to_string_tag;
use crate::runtime::host_call::{HostCall, HostResult, Thrown};
use crate::runtime::identifier::Identifier;
use crate::runtime::intrinsic::Intrinsic;
use crate::runtime::array_buffer_prototype::put_native_getter;
use crate::runtime::js_disposable_stack::JSDisposableStack;
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

/// `const ClassInfo DisposableStackPrototype::s_info`.
pub static DISPOSABLE_STACK_PROTOTYPE_S_INFO: ClassInfo =
    ClassInfo { class_name: "DisposableStack", parent_class: Some(&JS_NON_FINAL_OBJECT_S_INFO), static_prop_hash_table: None, inherits_js_type_range: None };

/// `disposableStackProtoDisposedGetter`.
fn disposable_stack_proto_disposed_getter_body(_global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    match JSDisposableStack::from_value(&call.this_value()) {
        Some(stack) => Ok(js_boolean(stack.disposed())),
        None => Err(Thrown::type_error("DisposableStack.prototype.disposed getter requires that |this| be a DisposableStack object")),
    }
}

host_function!(disposable_stack_proto_disposed_getter, disposable_stack_proto_disposed_getter_body);

/// `class DisposableStackPrototype final : public JSNonFinalObject`: sem campos próprios.
pub struct DisposableStackPrototype;

impl DisposableStackPrototype {
    /// `StructureFlags = Base::StructureFlags`.
    pub const STRUCTURE_FLAGS: u32 = JSNonFinalObject::STRUCTURE_FLAGS;

    /// `createStructure(vm, globalObject, prototype)`.
    pub fn create_structure(vm: &VM, global_object: &JSGlobalObject, prototype: JSValue) -> StructureRef {
        Structure::create(
            vm,
            Some(global_object),
            prototype,
            TypeInfo::new(JSType::ObjectType, DisposableStackPrototype::STRUCTURE_FLAGS),
            &DISPOSABLE_STACK_PROTOTYPE_S_INFO,
        )
    }

    /// `create(vm, globalObject, structure)`: `DisposableStackPrototype(vm, structure)` e `finishCreation`.
    pub fn create(vm: &VM, global_object: &JSGlobalObject, structure: &StructureRef) -> JSObjectRef {
        let prototype = JSObject::allocate(vm, structure);
        DisposableStackPrototype::finish_creation(&prototype, vm, global_object);
        prototype
    }

    /// `finishCreation(vm, globalObject)`: os builtins `DontEnum`, o getter `disposed` e `@@dispose` igual a
    /// `dispose`, na ordem do C++.
    fn finish_creation(prototype: &JSObject, vm: &VM, global_object: &JSGlobalObject) {
        prototype.finish_creation(vm);
        let put_builtin = |name: &Identifier, index: BuiltinCodeIndex| {
            put_direct_builtin_function_without_transition(vm, global_object, prototype, name, index, DONT_ENUM)
        };
        put_builtin(&vm.property_names.adopt, BuiltinCodeIndex::DisposableStackPrototypeAdoptCode);
        put_builtin(&vm.property_names.defer_keyword, BuiltinCodeIndex::DisposableStackPrototypeDeferMethodCode);
        let dispose_function = put_builtin(&vm.property_names.dispose, BuiltinCodeIndex::DisposableStackPrototypeDisposeCode);
        put_native_getter(vm, global_object, prototype, "disposed", disposable_stack_proto_disposed_getter, Intrinsic::NoIntrinsic, DONT_ENUM);
        prototype.put_direct_without_transition(
            vm,
            &PropertyName::from_identifier(&vm.property_names.dispose_symbol),
            dispose_function.as_value(),
            DONT_ENUM,
        );
        put_builtin(&vm.property_names.r#use, BuiltinCodeIndex::DisposableStackPrototypeUseCode);
        put_builtin(&vm.property_names.r#move, BuiltinCodeIndex::DisposableStackPrototypeMoveCode);
        // `JSC_TO_STRING_TAG_WITHOUT_TRANSITION()`.
        put_to_string_tag(vm, prototype, DISPOSABLE_STACK_PROTOTYPE_S_INFO.class_name);
    }
}
