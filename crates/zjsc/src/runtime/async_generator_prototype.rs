//! Porte de `runtime/AsyncGeneratorPrototype.{h,cpp}` e `AsyncGeneratorPrototypeInlines.h`: o
//! `%AsyncGeneratorPrototype%` (`return`, `throw`, `next` do `LinkTimeConstant::asyncGeneratorPrototypeNext`),
//! os corpos nativos de `next`, `return` e `throw`
//! (https://tc39.es/ecma262/#sec-properties-of-asyncgenerator-prototype) e `asyncGeneratorNext`, que o
//! `asyncIteratorNextWithDriver` (`js_microtask_async.rs`) também usa.
//!
//! `asyncGeneratorPrototypeTable` (`return`, `throw`, `DontEnum|Function 1`) fica no `ClassInfo` e a `Structure`
//! leva `HasStaticPropertyTable`: as duas reificam no primeiro acesso (`Reflect.ownKeys` no bun 1.4.2:
//! `return,throw,next,constructor,@@toStringTag`, igual antes e depois). `next` e o `@@toStringTag` são do
//! `finishCreation`; o `constructor` vem de `link_generator_prototype` (`function_kind_intrinsics.rs`).
//!
//! DIVERGÊNCIA:
//! - `MicrotaskCallCache* microtaskCallCache` (`&vm.syncResumeCallCache()`) não existe (ver `js_microtask.rs`).

use crate::bytecode::bytecode_intrinsics_table::LinkTimeConstant;
use crate::host_function;
use crate::runtime::class_info::ClassInfo;
use crate::runtime::host_call::{HostCall, HostResult};
use crate::runtime::implementation_visibility::ImplementationVisibility;
use crate::runtime::intrinsic::Intrinsic;
use crate::runtime::js_async_generator::{
    is_executing_state, is_suspended_yield_state, AsyncGeneratorResumeMode, AsyncGeneratorState, JSAsyncGenerator, JSAsyncGeneratorRef,
};
use crate::runtime::collection_support::put_to_string_tag;
use crate::runtime::js_function::{call_host_function_as_constructor, JSFunction, JSFunctionRef};
use crate::runtime::lookup::{HashTable, HashTableValue, Kind};
use crate::runtime::lookup::{native_entry};
use crate::runtime::native_function::NativeFunction;
use crate::runtime::js_global_object::JSGlobalObject;
use crate::runtime::js_microtask_async::{async_generator_await_return, async_generator_resume, realm_of};
use crate::runtime::js_object::{JSNonFinalObject, JSObject, JSObjectRef, JS_NON_FINAL_OBJECT_S_INFO};
use crate::runtime::js_promise::{JSPromise, JSPromiseRef};
use crate::runtime::js_promise_host::PromiseHost;
use crate::runtime::js_type::JSType;
use crate::runtime::js_type_info::{TypeInfo, HAS_STATIC_PROPERTY_TABLE};
use crate::runtime::js_value::JSValue;
use crate::runtime::property_attribute::{DONT_ENUM, FUNCTION};
use crate::runtime::property_name::PropertyName;
use crate::runtime::string_regexp_support::create_iterator_result_object;
use crate::runtime::structure::{Structure, StructureRef};
use crate::runtime::vm::VM;

/// `const ClassInfo AsyncGeneratorPrototype::s_info`.
pub static ASYNC_GENERATOR_PROTOTYPE_S_INFO: ClassInfo = ClassInfo {
    class_name: "AsyncGenerator",
    parent_class: Some(&JS_NON_FINAL_OBJECT_S_INFO),
    static_prop_hash_table: Some(&ASYNC_GENERATOR_PROTOTYPE_TABLE),
    inherits_js_type_range: None,
};

/// `asyncGeneratorPrototypeTableValues` de `AsyncGeneratorPrototype.lut.h`, na ordem do `@begin`.
static ASYNC_GENERATOR_PROTOTYPE_TABLE_VALUES: [HashTableValue; 2] = [
    native_entry("return", async_generator_prototype_return_host, 1),
    native_entry("throw", async_generator_prototype_throw_host, 1),
];

/// `asyncGeneratorPrototypeTable`.
static ASYNC_GENERATOR_PROTOTYPE_TABLE: HashTable = HashTable { class_for_this: None, values: &ASYNC_GENERATOR_PROTOTYPE_TABLE_VALUES };

/// A mensagem do `TypeError` de `AsyncGeneratorValidate`.
const NOT_AN_ASYNC_GENERATOR_MESSAGE: &str = "|this| should be an async generator";

/// `asyncGeneratorNext(globalObject, generator, argument, cache)`
/// (https://tc39.es/ecma262/#sec-asyncgenerator-prototype-next): a promessa do pedido.
pub fn async_generator_next(global_object: &JSGlobalObject, generator: &JSAsyncGenerator, argument: JSValue) -> JSValue {
    let vm = global_object.vm();
    let promise = JSPromise::create(vm, &global_object.promise_structure());

    // 5. Let state be gen.[[AsyncGeneratorState]].
    let state = generator.state();
    // 6. If state is completed, then
    if state == AsyncGeneratorState::Completed as i32 {
        // 6.a. Let iteratorResult be CreateIteratorResultObject(undefined, true).
        // 6.b. Perform ! Call(promiseCapability.[[Resolve]], undefined, « iteratorResult »).
        // The iterator result object belongs to the generator's realm.
        let iterator_result = create_iterator_result_object(&realm_of(generator), JSValue::undefined(), /* done */ true);
        promise.resolve(global_object, iterator_result);
        return promise.as_value();
    }

    // 7. Let completion be NormalCompletion(value).
    // 8. Perform AsyncGeneratorEnqueue(gen, completion, promiseCapability).
    generator.enqueue(argument, AsyncGeneratorResumeMode::Normal as i32, promise.as_value());

    // 9. If state is either suspended-start or suspended-yield, then
    if state == AsyncGeneratorState::Init as i32 || is_suspended_yield_state(state) {
        // 9.a. Perform AsyncGeneratorResume(gen, completion).
        async_generator_resume(global_object, generator);
    } else {
        // 10. Else,
        // 10.a. Assert: state is either executing or draining-queue.
        debug_assert!(is_executing_state(state) || state == AsyncGeneratorState::DrainingQueue as i32);
    }

    // 11. Return promiseCapability.[[Promise]].
    promise.as_value()
}

/// `asyncGeneratorPrototypeNext`.
fn async_generator_prototype_next(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    // 3. Let result be Completion(AsyncGeneratorValidate(gen, empty)).
    // 4. IfAbruptRejectPromise(result, promiseCapability).
    let Some(generator) = JSAsyncGenerator::from_value(&call.this_value()) else {
        let error = global_object.create_type_error(NOT_AN_ASYNC_GENERATOR_MESSAGE);
        return Ok(JSPromise::rejected_promise(global_object, error).as_value());
    };

    Ok(async_generator_next(global_object, &generator, call.argument(0)))
}

host_function!(async_generator_prototype_next_host, async_generator_prototype_next);

/// O início comum de `return` e `throw`: a promessa do pedido e o gerador validado; sem gerador, a promessa
/// já rejeitada, que é o resultado da função.
fn promise_and_validated_generator(
    global_object: &JSGlobalObject,
    call: &HostCall,
) -> Result<(JSPromiseRef, JSAsyncGeneratorRef), JSValue> {
    let promise = JSPromise::create(global_object.vm(), &global_object.promise_structure());

    // 3. Let result be Completion(AsyncGeneratorValidate(gen, empty)).
    // 4. IfAbruptRejectPromise(result, promiseCapability).
    match JSAsyncGenerator::from_value(&call.this_value()) {
        Some(generator) => Ok((promise, generator)),
        None => {
            promise.reject(global_object, global_object.create_type_error(NOT_AN_ASYNC_GENERATOR_MESSAGE));
            Err(promise.as_value())
        }
    }
}

/// `asyncGeneratorPrototypeReturn` (https://tc39.es/ecma262/#sec-asyncgenerator-prototype-return).
fn async_generator_prototype_return(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    let (promise, generator) = match promise_and_validated_generator(global_object, call) {
        Ok(validated) => validated,
        Err(rejected) => return Ok(rejected),
    };

    // 5. Let completion be ReturnCompletion(value).
    // 6. Perform AsyncGeneratorEnqueue(gen, completion, promiseCapability).
    generator.enqueue(call.argument(0), AsyncGeneratorResumeMode::Return as i32, promise.as_value());

    // 7. Let state be gen.[[AsyncGeneratorState]].
    let state = generator.state();
    // 8. If state is either suspended-start or completed, then
    if state == AsyncGeneratorState::Init as i32 || state == AsyncGeneratorState::Completed as i32 {
        // 8.a. Set gen.[[AsyncGeneratorState]] to draining-queue.
        // 8.b. Perform AsyncGeneratorAwaitReturn(gen).
        generator.set_state(AsyncGeneratorState::DrainingQueue as i32);
        async_generator_await_return(global_object, &generator);
    } else if is_suspended_yield_state(state) {
        // 9. Else if state is suspended-yield, then
        // 9.a. Perform AsyncGeneratorResume(gen, completion).
        async_generator_resume(global_object, &generator);
    } else {
        // 10. Else,
        // 10.a. Assert: state is either executing or draining-queue.
        debug_assert!(is_executing_state(state) || state == AsyncGeneratorState::DrainingQueue as i32);
    }

    // 11. Return promiseCapability.[[Promise]].
    Ok(promise.as_value())
}

/// `asyncGeneratorPrototypeThrow` (https://tc39.es/ecma262/#sec-asyncgenerator-prototype-throw).
fn async_generator_prototype_throw(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    let (promise, generator) = match promise_and_validated_generator(global_object, call) {
        Ok(validated) => validated,
        Err(rejected) => return Ok(rejected),
    };

    let exception = call.argument(0);
    let mut state = generator.state();
    // 5. Let state be gen.[[AsyncGeneratorState]].
    // 6. If state is suspended-start, then
    if state == AsyncGeneratorState::Init as i32 {
        // 6.a. Set gen.[[AsyncGeneratorState]] to completed.
        // 6.b. Set state to completed.
        generator.set_state(AsyncGeneratorState::Completed as i32);
        state = AsyncGeneratorState::Completed as i32;
    }

    // 7. If state is completed, then
    if state == AsyncGeneratorState::Completed as i32 {
        // 7.a. Perform ! Call(promiseCapability.[[Reject]], undefined, « exception »).
        // 7.b. Return promiseCapability.[[Promise]].
        promise.reject(global_object, exception);
        return Ok(promise.as_value());
    }

    // 8. Let completion be ThrowCompletion(exception).
    // 9. Perform AsyncGeneratorEnqueue(gen, completion, promiseCapability).
    generator.enqueue(exception, AsyncGeneratorResumeMode::Throw as i32, promise.as_value());

    // 10. If state is suspended-yield, then
    // 10.a. Perform AsyncGeneratorResume(gen, completion).
    if is_suspended_yield_state(state) {
        async_generator_resume(global_object, &generator);
    } else {
        // 11. Else,
        // 11.a. Assert: state is either executing or draining-queue.
        debug_assert!(is_executing_state(state) || state == AsyncGeneratorState::DrainingQueue as i32);
    }

    // 12. Return promiseCapability.[[Promise]].
    Ok(promise.as_value())
}

host_function!(async_generator_prototype_return_host, async_generator_prototype_return);
host_function!(async_generator_prototype_throw_host, async_generator_prototype_throw);

/// `JSFunction::create(vm, owner, 1, vm.propertyNames->next.impl(), asyncGeneratorPrototypeNext,
/// ImplementationVisibility::Public)`, o `LinkTimeConstant::asyncGeneratorPrototypeNext` do `JSGlobalObject`.
pub fn create_async_generator_prototype_next_function(vm: &VM, global_object: &JSGlobalObject) -> JSFunctionRef {
    JSFunction::create_native(
        vm,
        global_object,
        1,
        vm.property_names.next.string().string(),
        async_generator_prototype_next_host,
        ImplementationVisibility::Public,
        Intrinsic::NoIntrinsic,
        call_host_function_as_constructor,
    )
}

/// `class AsyncGeneratorPrototype final : public JSNonFinalObject`.
pub struct AsyncGeneratorPrototype;

impl AsyncGeneratorPrototype {
    /// `StructureFlags = Base::StructureFlags | HasStaticPropertyTable`.
    pub const STRUCTURE_FLAGS: u32 = JSNonFinalObject::STRUCTURE_FLAGS | HAS_STATIC_PROPERTY_TABLE;

    /// `createStructure(vm, globalObject, prototype)` (`AsyncGeneratorPrototypeInlines.h`).
    pub fn create_structure(vm: &VM, global_object: &JSGlobalObject, prototype: JSValue) -> StructureRef {
        Structure::create(
            vm,
            Some(global_object),
            prototype,
            TypeInfo::new(JSType::ObjectType, AsyncGeneratorPrototype::STRUCTURE_FLAGS),
            &ASYNC_GENERATOR_PROTOTYPE_S_INFO,
        )
    }

    /// `create(vm, globalObject, structure)`: `AsyncGeneratorPrototype(vm, structure)` e
    /// `finishCreation(vm, globalObject)`. Lê o `LinkTimeConstant::asyncGeneratorPrototypeNext`, que o
    /// global tem de ter criado antes.
    pub fn create(vm: &VM, global_object: &JSGlobalObject, structure: &StructureRef) -> JSObjectRef {
        let prototype = JSObject::allocate(vm, structure);
        prototype.finish_creation(vm);
        // Só `next` e o `@@toStringTag` nascem aqui; `return` e `throw` (`asyncGeneratorPrototypeTable`) reificam
        // no primeiro acesso, e o `constructor` entra depois, em `link_generator_prototype`
        // (`function_kind_intrinsics.rs`).
        prototype.put_direct_without_transition(
            vm,
            &PropertyName::from_identifier(&vm.property_names.next),
            global_object.link_time_constant(LinkTimeConstant::AsyncGeneratorPrototypeNext),
            DONT_ENUM,
        );
        // `JSC_TO_STRING_TAG_WITHOUT_TRANSITION()`.
        put_to_string_tag(vm, &prototype, ASYNC_GENERATOR_PROTOTYPE_S_INFO.class_name);
        prototype.structure().set_may_be_prototype(true);
        prototype
    }
}
