//! Porte de `runtime/JSAsyncFunction.h` e `.cpp`. Ver as DIVERGÊNCIAS de `js_generator_function.rs`: a
//! instância é um `JSFunction` com a `Structure` de `ClassInfo` `AsyncFunction`.

use crate::runtime::class_info::ClassInfo;
use crate::runtime::js_function::{function_structure, FunctionExecutableRef, JSFunction, JSFunctionRef, JS_FUNCTION_S_INFO};
use crate::runtime::js_global_object::JSGlobalObject;
use crate::runtime::js_scope::JSScopeRef;
use crate::runtime::js_value::JSValue;
use crate::runtime::structure::StructureRef;
use crate::runtime::vm::VM;

/// `const ClassInfo JSAsyncFunction::s_info`.
pub static JS_ASYNC_FUNCTION_S_INFO: ClassInfo =
    ClassInfo { class_name: "AsyncFunction", parent_class: Some(&JS_FUNCTION_S_INFO), static_prop_hash_table: None, inherits_js_type_range: None };

/// `class JSAsyncFunction final : public JSFunction`.
pub struct JSAsyncFunction;

impl JSAsyncFunction {
    /// `StructureFlags = Base::StructureFlags`.
    pub const STRUCTURE_FLAGS: u32 = JSFunction::STRUCTURE_FLAGS;

    /// `create(vm, globalObject, executable, scope)`.
    pub fn create(vm: &VM, global_object: &JSGlobalObject, executable: &FunctionExecutableRef, scope: JSScopeRef) -> JSFunctionRef {
        JSAsyncFunction::create_with_structure(vm, global_object, executable, scope, global_object.async_function_structure())
    }

    /// `create(vm, globalObject, executable, scope, structure)`.
    pub fn create_with_structure(
        vm: &VM,
        _global_object: &JSGlobalObject,
        executable: &FunctionExecutableRef,
        scope: JSScopeRef,
        structure: StructureRef,
    ) -> JSFunctionRef {
        let function = JSFunction::create_impl(vm, executable, scope, structure);
        executable.borrow_mut().notify_creation(vm, &function, "Allocating an async function");
        function
    }

    /// `createWithInvalidatedReallocationWatchpoint(vm, globalObject, executable, scope)`.
    pub fn create_with_invalidated_reallocation_watchpoint(
        vm: &VM,
        global_object: &JSGlobalObject,
        executable: &FunctionExecutableRef,
        scope: JSScopeRef,
    ) -> JSFunctionRef {
        JSAsyncFunction::create_with_invalidated_reallocation_watchpoint_and_structure(
            vm,
            global_object,
            executable,
            scope,
            global_object.async_function_structure(),
        )
    }

    /// `createWithInvalidatedReallocationWatchpoint(vm, globalObject, executable, scope, structure)`.
    pub fn create_with_invalidated_reallocation_watchpoint_and_structure(
        vm: &VM,
        _global_object: &JSGlobalObject,
        executable: &FunctionExecutableRef,
        scope: JSScopeRef,
        structure: StructureRef,
    ) -> JSFunctionRef {
        JSFunction::create_impl(vm, executable, scope, structure)
    }

    /// `createStructure(vm, globalObject, prototype)`.
    pub fn create_structure(vm: &VM, global_object: &JSGlobalObject, prototype: JSValue) -> StructureRef {
        function_structure(vm, global_object, prototype, &JS_ASYNC_FUNCTION_S_INFO)
    }
}
