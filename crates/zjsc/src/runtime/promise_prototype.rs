//! Porte de `runtime/JSPromisePrototype.{h,cpp}`: o `Promise.prototype` (`then`, `catch`, `finally`,
//! `@@toStringTag` e o `@then` privado), os corpos nativos de `then`, `catch` e `finally`, as funções
//! internas de `finally` (`promiseFinallyThenFinallyFunc` e companhia, `JSFunctionWithFields`) e o
//! `promiseSpeciesWatchpointIsValid`, que é o `PromiseHost::promise_species_watchpoint_is_valid` do
//! `JSGlobalObject` (`promise_constructor.rs`).
//!
//! DIVERGÊNCIAS:
//! - `promisePrototypeTable` (só `finally`) fica no `ClassInfo` e a `Structure` leva
//!   `HasStaticPropertyTable`: `finally` reifica no primeiro acesso. `then` é a função que o global guarda
//!   (`promiseProtoThenFunction()`, o `defaultPromiseThen`) e `catch` segue eager, como no C++.
//! - `thisValue.get(globalObject, then)` de primitivo que não é `undefined`/`null` responde
//!   `Unported` (`get_value_property`).

use crate::host_function;
use crate::runtime::class_info::ClassInfo;
use crate::runtime::host_call::{HostCall, HostResult, Thrown};
use crate::runtime::implementation_visibility::ImplementationVisibility;
use crate::runtime::intrinsic::Intrinsic;
use crate::runtime::iterator_operations::{call_checked, get_value_property};
use crate::runtime::proxy_object::to_this_strict;
use crate::runtime::js_function::{
    call_host_function_as_constructor, put_direct_native_function_without_transition, JSFunction, JSFunctionRef,
};
use crate::runtime::js_function_with_fields::JSFunctionWithFields;
use crate::runtime::js_global_object::JSGlobalObject;
use crate::runtime::js_object::{JSNonFinalObject, JSObject, JSObjectRef, JS_NON_FINAL_OBJECT_S_INFO};
use crate::runtime::js_promise::JSPromise;
use crate::runtime::js_promise_capability::promise_species_constructor;
use crate::runtime::js_promise_host::{host_result, rethrow, FunctionField, PromiseHost, Thrown as PromiseThrown};
use crate::runtime::js_promise_reaction::JSSlimPromiseReaction;
use crate::runtime::js_string::js_string;
use crate::runtime::js_type::JSType;
use crate::runtime::js_type_info::{TypeInfo, HAS_STATIC_PROPERTY_TABLE};
use crate::runtime::lookup::{HashTable, HashTableValue, Kind};
use crate::runtime::js_value::JSValue;
use crate::runtime::microtask::InternalMicrotask;
use crate::runtime::native_function::NativeFunction;
use crate::runtime::property_attribute::{DONT_DELETE, DONT_ENUM, FUNCTION, READ_ONLY};
use crate::runtime::property_name::PropertyName;
use crate::runtime::structure::{Structure, StructureRef};
use crate::runtime::vm::VM;
use crate::wtf::text::wtf_string::String as WtfString;

/// `const ClassInfo JSPromisePrototype::s_info`.
pub static JS_PROMISE_PROTOTYPE_S_INFO: ClassInfo = ClassInfo {
    class_name: "Promise",
    parent_class: Some(&JS_NON_FINAL_OBJECT_S_INFO),
    static_prop_hash_table: Some(&PROMISE_PROTOTYPE_TABLE),
    inherits_js_type_range: None,
};

/// `promisePrototypeTableValues` de `JSPromisePrototype.lut.h`: `finally` (`DontEnum|Function`, comprimento 1).
static PROMISE_PROTOTYPE_TABLE_VALUES: [HashTableValue; 1] = [HashTableValue {
    key: "finally",
    attributes: DONT_ENUM | FUNCTION,
    intrinsic: Intrinsic::NoIntrinsic,
    kind: Kind::NativeFunction { function: promise_proto_func_finally_host, length: 1 },
}];

/// `promisePrototypeTable`.
static PROMISE_PROTOTYPE_TABLE: HashTable = HashTable { class_for_this: None, values: &PROMISE_PROTOTYPE_TABLE_VALUES };

const THEN_NOT_A_FUNCTION_ERROR: &str = "|this|.then is not a function";

/// `promiseProtoFuncThen`.
fn promise_proto_func_then(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    let this_value = to_this_strict(call.this_value());
    let Some(promise) = JSPromise::from_value(&this_value) else {
        return Err(Thrown::type_error("|this| is not a Promise"));
    };
    host_result(global_object, promise.then(global_object, call.argument(0), call.argument(1)))
}

/// `promiseProtoFuncCatch`.
fn promise_proto_func_catch(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    let vm = global_object.vm();
    let this_value = to_this_strict(call.this_value());
    let on_rejected = call.argument(0);

    if let Some(promise) = JSPromise::from_value(&this_value) {
        if promise.is_then_fast_and_non_observable(global_object) {
            return host_result(global_object, promise.then(global_object, JSValue::undefined(), on_rejected));
        }
    }

    let then = get_value_property(global_object, this_value, &PropertyName::from_identifier(&vm.property_names.then))?;
    call_checked(global_object, then, this_value, &[JSValue::undefined(), on_rejected], THEN_NOT_A_FUNCTION_ERROR)
}

/// `promiseFinallyValueThunkFunc`: devolve o valor guardado.
fn promise_finally_value_thunk_func(_global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    Ok(JSFunctionWithFields::get_field(&JSFunctionWithFields::callee(call), FunctionField::RESOLVING_PROMISE))
}

/// `promiseFinallyThrowerFunc`: lança o motivo guardado.
fn promise_finally_thrower_func(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    let reason = JSFunctionWithFields::get_field(&JSFunctionWithFields::callee(call), FunctionField::RESOLVING_PROMISE);
    Err(rethrow(global_object, PromiseThrown::Value(reason)))
}

/// O corpo comum de `promiseFinallyThenFinallyFunc` e `promiseFinallyCatchFinallyFunc`: chama
/// `onFinally()`, resolve o resultado pelo construtor e devolve `resolvedPromise.then(thunk)`, onde
/// `thunk` é a função (devolvedora de valor ou lançadora de motivo) de `thunk_function` com o `argument(0)`.
fn promise_finally_continuation(
    global_object: &JSGlobalObject,
    call: &HostCall,
    thunk_function: NativeFunction,
) -> HostResult {
    let vm = global_object.vm();
    let callee = JSFunctionWithFields::callee(call);
    let on_finally = JSFunctionWithFields::get_field(&callee, FunctionField::RESOLVING_PROMISE);
    let constructor = JSFunctionWithFields::get_field(&callee, FunctionField::RESOLVING_OTHER);
    let value_or_reason = call.argument(0);

    let result = call_checked(global_object, on_finally, JSValue::undefined(), &[], "onFinally is not a function")?;

    let resolved_promise = host_result(global_object, JSPromise::promise_resolve(global_object, constructor, result))?;

    let thunk = JSFunctionWithFields::create(vm, global_object, 0, thunk_function);
    JSFunctionWithFields::set_field(&thunk, FunctionField::RESOLVING_PROMISE, value_or_reason);

    let then = get_value_property(global_object, resolved_promise, &PropertyName::from_identifier(&vm.property_names.then))?;
    call_checked(global_object, then, resolved_promise, &[thunk.as_value(), JSValue::undefined()], THEN_NOT_A_FUNCTION_ERROR)
}

/// `promiseFinallyThenFinallyFunc`.
fn promise_finally_then_finally_func(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    promise_finally_continuation(global_object, call, promise_finally_value_thunk_func_host)
}

/// `promiseFinallyCatchFinallyFunc`.
fn promise_finally_catch_finally_func(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    promise_finally_continuation(global_object, call, promise_finally_thrower_func_host)
}

host_function!(promise_proto_func_then_host, promise_proto_func_then);
host_function!(promise_proto_func_catch_host, promise_proto_func_catch);
host_function!(promise_finally_value_thunk_func_host, promise_finally_value_thunk_func);
host_function!(promise_finally_thrower_func_host, promise_finally_thrower_func);
host_function!(promise_finally_then_finally_func_host, promise_finally_then_finally_func);
host_function!(promise_finally_catch_finally_func_host, promise_finally_catch_finally_func);

/// `promiseProtoFuncFinallySlow`.
fn promise_proto_func_finally_slow(global_object: &JSGlobalObject, this_value: JSValue, on_finally: JSValue) -> HostResult {
    let vm = global_object.vm();

    let constructor = host_result(global_object, promise_species_constructor(global_object, this_value))?;

    let then = get_value_property(global_object, this_value, &PropertyName::from_identifier(&vm.property_names.then))?;

    if !on_finally.is_callable() {
        return call_checked(global_object, then, this_value, &[on_finally, on_finally], THEN_NOT_A_FUNCTION_ERROR);
    }

    let then_finally = JSFunctionWithFields::create(vm, global_object, 1, promise_finally_then_finally_func_host);
    JSFunctionWithFields::set_field(&then_finally, FunctionField::RESOLVING_PROMISE, on_finally);
    JSFunctionWithFields::set_field(&then_finally, FunctionField::RESOLVING_OTHER, constructor);

    let catch_finally = JSFunctionWithFields::create(vm, global_object, 1, promise_finally_catch_finally_func_host);
    JSFunctionWithFields::set_field(&catch_finally, FunctionField::RESOLVING_PROMISE, on_finally);
    JSFunctionWithFields::set_field(&catch_finally, FunctionField::RESOLVING_OTHER, constructor);

    call_checked(global_object, then, this_value, &[then_finally.as_value(), catch_finally.as_value()], THEN_NOT_A_FUNCTION_ERROR)
}

/// `promiseProtoFuncFinally`.
fn promise_proto_func_finally(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    let vm = global_object.vm();
    let this_value = to_this_strict(call.this_value());
    if !this_value.is_object() {
        return Err(Thrown::type_error("|this| is not an object"));
    }

    let on_finally = call.argument(0);
    if let Some(promise) = JSPromise::from_value(&this_value) {
        if promise.is_then_fast_and_non_observable(global_object) {
            if !on_finally.is_callable() {
                return host_result(global_object, promise.then(global_object, on_finally, on_finally));
            }

            if global_object.promise_species_watchpoint_is_valid(&promise) {
                let result_promise = JSPromise::create(vm, &global_object.promise_structure());
                let context = JSSlimPromiseReaction::create(result_promise.as_value(), on_finally, false, None);
                promise.perform_promise_then_with_internal_microtask(
                    global_object,
                    InternalMicrotask::PromiseFinallyReactionJob,
                    None,
                    context.as_value(),
                    global_object.async_context(),
                );
                return Ok(result_promise.as_value());
            }
        }
    }

    promise_proto_func_finally_slow(global_object, this_value, on_finally)
}

host_function!(promise_proto_func_finally_host, promise_proto_func_finally);

/// `defaultPromiseThen` do `JSGlobalObject::init`: `JSFunction::create(vm, this, 2, "then",
/// promiseProtoFuncThen, Public, PromisePrototypeThenIntrinsic)`, que o global guarda como
/// `promiseProtoThenFunction()` e como a `LinkTimeConstant::DefaultPromiseThen`.
pub fn create_default_promise_then(vm: &VM, global_object: &JSGlobalObject) -> JSFunctionRef {
    JSFunction::create_native(
        vm,
        global_object,
        2,
        vm.property_names.then.string().string(),
        promise_proto_func_then_host,
        ImplementationVisibility::Public,
        Intrinsic::PromisePrototypeThenIntrinsic,
        call_host_function_as_constructor,
    )
}

/// `class JSPromisePrototype : public JSNonFinalObject`.
pub struct JSPromisePrototype;

impl JSPromisePrototype {
    /// `StructureFlags = Base::StructureFlags | HasStaticPropertyTable`.
    pub const STRUCTURE_FLAGS: u32 = JSNonFinalObject::STRUCTURE_FLAGS | HAS_STATIC_PROPERTY_TABLE;

    /// `createStructure(vm, globalObject, prototype)`.
    pub fn create_structure(vm: &VM, global_object: &JSGlobalObject, prototype: JSValue) -> StructureRef {
        Structure::create(
            vm,
            Some(global_object),
            prototype,
            TypeInfo::new(JSType::ObjectType, JSPromisePrototype::STRUCTURE_FLAGS),
            &JS_PROMISE_PROTOTYPE_S_INFO,
        )
    }

    /// `create(vm, globalObject, structure)`: `JSPromisePrototype(vm, structure)`, `finishCreation` e
    /// `addOwnInternalSlots`. O `then` é o `promiseProtoThenFunction()` que o global já guarda.
    pub fn create(
        vm: &VM,
        global_object: &JSGlobalObject,
        structure: &StructureRef,
        then_function: JSFunctionRef,
    ) -> JSObjectRef {
        let prototype = JSObject::allocate(vm, structure);
        JSPromisePrototype::finish_creation(&prototype, vm, global_object, then_function.as_value());
        JSPromisePrototype::add_own_internal_slots(&prototype, vm, then_function.as_value());
        prototype
    }

    /// `finishCreation(vm, globalObject)`.
    fn finish_creation(prototype: &JSObject, vm: &VM, global_object: &JSGlobalObject, then_function: JSValue) {
        // `finally` vem de `promisePrototypeTable` e reifica no primeiro acesso.
        prototype.finish_creation(vm);
        let builtin_names = vm.property_names.builtin_names();
        prototype.put_direct_without_transition(
            vm,
            &PropertyName::from_identifier(builtin_names.then_public_name()),
            then_function,
            DONT_ENUM,
        );
        put_direct_native_function_without_transition(
            vm,
            global_object,
            prototype,
            &vm.property_names.catch_keyword,
            1,
            promise_proto_func_catch_host,
            ImplementationVisibility::Public,
            Intrinsic::PromisePrototypeCatchIntrinsic,
            DONT_ENUM,
        );
        // `JSC_TO_STRING_TAG_WITHOUT_TRANSITION()`.
        prototype.put_direct_without_transition(
            vm,
            &PropertyName::from_identifier(&vm.property_names.to_string_tag_symbol),
            JSValue::from_js_string(js_string(vm, &WtfString::from_latin1(JS_PROMISE_PROTOTYPE_S_INFO.class_name.as_bytes()))),
            DONT_ENUM | READ_ONLY,
        );
        prototype.structure().set_may_be_prototype(true);
    }

    /// `addOwnInternalSlots(vm, globalObject)`: o `@then` privado.
    fn add_own_internal_slots(prototype: &JSObject, vm: &VM, then_function: JSValue) {
        prototype.put_direct_without_transition(
            vm,
            &PropertyName::from_identifier(&vm.property_names.builtin_names().then_private_name()),
            then_function,
            DONT_ENUM | DONT_DELETE | READ_ONLY,
        );
    }
}
