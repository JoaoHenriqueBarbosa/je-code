//! Porte de `runtime/AsyncIteratorPrototype.{h,cpp}` e `AsyncIteratorPrototypeInlines.h`: o
//! `%AsyncIteratorPrototype%` (`@@asyncIterator` do `LinkTimeConstant::asyncIteratorPrototypeSymbolAsyncIterator`
//! e, com `useExplicitResourceManagement`, o `@@asyncDispose` do builtin
//! `AsyncIteratorPrototype.js`) e a função nativa `asyncIteratorProtoFuncAsyncIterator`.

use crate::bytecode::bytecode_intrinsics_table::LinkTimeConstant;
use crate::host_function;
use crate::runtime::builtins_source::BuiltinCodeIndex;
use crate::runtime::class_info::ClassInfo;
use crate::runtime::host_call::{HostCall, HostResult};
use crate::runtime::implementation_visibility::ImplementationVisibility;
use crate::runtime::intrinsic::Intrinsic;
use crate::runtime::js_function::{
    call_host_function_as_constructor, put_direct_builtin_function_without_transition, JSFunction, JSFunctionRef,
};
use crate::runtime::js_global_object::JSGlobalObject;
use crate::runtime::js_object::{JSNonFinalObject, JSObject, JSObjectRef, JS_NON_FINAL_OBJECT_S_INFO};
use crate::runtime::js_type::JSType;
use crate::runtime::js_type_info::TypeInfo;
use crate::runtime::js_value::JSValue;
use crate::runtime::options_list::Options;
use crate::runtime::property_attribute::DONT_ENUM;
use crate::runtime::property_name::PropertyName;
use crate::runtime::proxy_object::to_this_strict;
use crate::runtime::structure::{Structure, StructureRef};
use crate::runtime::vm::VM;
use crate::wtf::text::wtf_string::String as WtfString;

/// `const ClassInfo AsyncIteratorPrototype::s_info`.
pub static ASYNC_ITERATOR_PROTOTYPE_S_INFO: ClassInfo =
    ClassInfo { class_name: "AsyncIterator", parent_class: Some(&JS_NON_FINAL_OBJECT_S_INFO), static_prop_hash_table: None, inherits_js_type_range: None };

/// `asyncIteratorProtoFuncAsyncIterator`.
fn async_iterator_proto_func_async_iterator(_global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    Ok(to_this_strict(call.this_value()))
}

host_function!(async_iterator_proto_func_async_iterator_host, async_iterator_proto_func_async_iterator);

/// `JSFunction::create(vm, owner, 0, "[Symbol.asyncIterator]"_s, asyncIteratorProtoFuncAsyncIterator,
/// ImplementationVisibility::Public, AsyncIteratorIntrinsic)`, o
/// `LinkTimeConstant::asyncIteratorPrototypeSymbolAsyncIterator` do `JSGlobalObject`.
pub fn create_async_iterator_proto_func_async_iterator(vm: &VM, global_object: &JSGlobalObject) -> JSFunctionRef {
    JSFunction::create_native(
        vm,
        global_object,
        0,
        &WtfString::from_latin1(b"[Symbol.asyncIterator]"),
        async_iterator_proto_func_async_iterator_host,
        ImplementationVisibility::Public,
        Intrinsic::AsyncIteratorIntrinsic,
        call_host_function_as_constructor,
    )
}

/// `class AsyncIteratorPrototype final : public JSNonFinalObject`.
pub struct AsyncIteratorPrototype;

impl AsyncIteratorPrototype {
    /// `StructureFlags = Base::StructureFlags`.
    pub const STRUCTURE_FLAGS: u32 = JSNonFinalObject::STRUCTURE_FLAGS;

    /// `createStructure(vm, globalObject, prototype)` (`AsyncIteratorPrototypeInlines.h`).
    pub fn create_structure(vm: &VM, global_object: &JSGlobalObject, prototype: JSValue) -> StructureRef {
        Structure::create(
            vm,
            Some(global_object),
            prototype,
            TypeInfo::new(JSType::ObjectType, AsyncIteratorPrototype::STRUCTURE_FLAGS),
            &ASYNC_ITERATOR_PROTOTYPE_S_INFO,
        )
    }

    /// `create(vm, globalObject, structure)`: `AsyncIteratorPrototype(vm, structure)` e
    /// `finishCreation(vm, globalObject)`. Lê o `LinkTimeConstant::asyncIteratorPrototypeSymbolAsyncIterator`,
    /// que o global tem de ter criado antes.
    pub fn create(vm: &VM, global_object: &JSGlobalObject, structure: &StructureRef) -> JSObjectRef {
        let prototype = JSObject::allocate(vm, structure);
        prototype.finish_creation(vm);
        prototype.put_direct_without_transition(
            vm,
            &PropertyName::from_identifier(&vm.property_names.async_iterator_symbol),
            global_object.link_time_constant(LinkTimeConstant::AsyncIteratorPrototypeSymbolAsyncIterator),
            DONT_ENUM,
        );

        if Options::use_explicit_resource_management() {
            put_direct_builtin_function_without_transition(
                vm,
                global_object,
                &prototype,
                &vm.property_names.async_dispose_symbol,
                BuiltinCodeIndex::AsyncIteratorPrototypeAsyncDisposeCode,
                DONT_ENUM,
            );
        }
        prototype
    }
}
