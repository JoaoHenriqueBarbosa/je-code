//! Porte de `runtime/AsyncFromSyncIteratorPrototype.{h,cpp}` e `AsyncFromSyncIteratorPrototypeInlines.h`: o
//! `%AsyncFromSyncIteratorPrototype%` (`next` do `m_asyncFromSyncIteratorProtoNextFunction`, `return` e
//! `throw`), seus corpos nativos e os drivers `asyncFromSyncIteratorNext` e
//! `driveAsyncFromSyncIteratorWithDriver`
//! (https://tc39.es/ecma262/#sec-%asyncfromsynciteratorprototype%-object). A continuação
//! (`asyncFromSyncIteratorContinueOrDone`) é a tarefa de microtask de `js_microtask_async.rs`.
//!
//! DIVERGÊNCIAS:
//! - `callFrame->argumentCount() > 0 ? uncheckedArgument(0) : JSValue()` é `optional_argument`: o
//!   `JSValue::empty()` pede a chamada sem argumentos, como o `ArgList { data, 0 }` do C++.
//! - `JSArrayIterator::next(globalObject, value)` de `FastArray*` constrói o par de `entries` na
//!   `array_structure()` do realm (o C++ usa a estrutura do array original do global).
//! - `downcast<JSAsyncFromSyncIterator>(thisValue)` é `expect`: o `%AsyncFromSyncIteratorPrototype%` só é
//!   alcançável pelo invólucro, nunca pelo usuário.
//! - `HasStaticPropertyTable` não existe nesta classe (`StructureFlags = Base::StructureFlags`).

use crate::bytecode::op_metadata::IterationMode;
use crate::host_function;
use crate::runtime::class_info::ClassInfo;
use crate::runtime::collection_support::iterator_step_value;
use crate::runtime::host_call::{HostCall, HostResult};
use crate::runtime::implementation_visibility::ImplementationVisibility;
use crate::runtime::intrinsic::Intrinsic;
use crate::runtime::js_array_iterator::JSArrayIterator;
use crate::runtime::js_async_from_sync_iterator::JSAsyncFromSyncIterator;
use crate::runtime::js_function::{call_host_function_as_constructor, put_direct_native_function_without_transition, JSFunction, JSFunctionRef};
use crate::runtime::js_global_object::JSGlobalObject;
use crate::runtime::js_map::JSMapIterator;
use crate::runtime::js_microtask::call_microtask;
use crate::runtime::js_object::{JSNonFinalObject, JSObject, JSObjectRef, JS_NON_FINAL_OBJECT_S_INFO};
use crate::runtime::js_promise::{JSPromise, JSPromiseRef};
use crate::runtime::js_promise_host::{caught_exception, get_property_named, PromiseHost, Thrown};
use crate::runtime::js_set::JSSetIterator;
use crate::runtime::js_type::JSType;
use crate::runtime::js_type_info::TypeInfo;
use crate::runtime::js_value::JSValue;
use crate::runtime::microtask::InternalMicrotask;
use crate::runtime::property_attribute::DONT_ENUM;
use crate::runtime::property_name::PropertyName;
use crate::runtime::string_regexp_support::create_iterator_result_object;
use crate::runtime::structure::{Structure, StructureRef};
use crate::runtime::vm::VM;

/// `const ClassInfo AsyncFromSyncIteratorPrototype::s_info`.
pub static ASYNC_FROM_SYNC_ITERATOR_PROTOTYPE_S_INFO: ClassInfo = ClassInfo {
    class_name: "AsyncFromSyncIterator",
    parent_class: Some(&JS_NON_FINAL_OBJECT_S_INFO),
    static_prop_hash_table: None, inherits_js_type_range: None,
};

/// O `TypeError` que o C++ lança com `throwTypeError(globalObject, scope, message)`, como valor.
fn type_error(global_object: &JSGlobalObject, message: &str) -> Thrown {
    Thrown::Value(global_object.create_type_error(message))
}

/// `awaitAndContinue(globalObject, vm, iterator, target, closeSyncIteratorOnRejection, value, done)`: guarda
/// o alvo no iterador e espera `value`; a tarefa (`AsyncFromSyncIteratorDone` ou `...Continue`) o liquida.
fn await_and_continue(
    global_object: &JSGlobalObject,
    iterator: &JSAsyncFromSyncIterator,
    target: JSValue,
    close_sync_iterator_on_rejection: bool,
    value: JSValue,
    done: bool,
) {
    let task = if done { InternalMicrotask::AsyncFromSyncIteratorDone } else { InternalMicrotask::AsyncFromSyncIteratorContinue };
    debug_assert!(target.is_cell(), "o alvo de AsyncFromSyncIterator é uma promessa ou um driver");
    iterator.set_target(target, close_sync_iterator_on_rejection);
    JSPromise::resolve_with_internal_microtask_for_async_await(global_object, value, task, iterator.as_value());
}

/// `callSyncIteratorMethodAndExtract(globalObject, method, syncIterator, argument, value)`: chama o método do
/// iterador síncrono e devolve `(value, done)` do resultado. `argument` vazio chama sem argumentos.
fn call_sync_iterator_method_and_extract(
    global_object: &JSGlobalObject,
    method: JSValue,
    sync_iterator: JSValue,
    argument: JSValue,
) -> Result<(JSValue, bool), Thrown> {
    let vm = global_object.vm();
    let arguments: &[JSValue] = if argument.is_empty() { &[] } else { std::slice::from_ref(&argument) };
    let result = call_microtask(global_object, method, sync_iterator, arguments, "Iterator method is not callable.")?;

    if !result.is_object() {
        return Err(type_error(global_object, "Iterator result interface is not an object."));
    }

    let done = get_property_named(global_object, result, &vm.property_names.done)?.to_boolean();
    let value = get_property_named(global_object, result, &vm.property_names.value)?;
    Ok((value, done))
}

/// `driveFastSyncIterator(globalObject, vm, syncIterator, mode, value)`: um passo dos iteradores de `Array`,
/// `Map` e `Set` sem passar pelo `next` observável; `(value, done)`.
fn drive_fast_sync_iterator(global_object: &JSGlobalObject, sync_iterator: JSValue, mode: IterationMode) -> Result<(JSValue, bool), Thrown> {
    let step = match mode {
        IterationMode::FastArrayValues | IterationMode::FastArrayKeys | IterationMode::FastArrayEntries => {
            let iterator = JSArrayIterator::from_value(&sync_iterator).expect("uncheckedDowncast<JSArrayIterator>");
            let step = iterator.next(global_object.vm(), &global_object.array_structure());
            // `RETURN_IF_EXCEPTION(scope, false)`: o getter indexado que lança deixa a exceção no `VM`.
            if global_object.vm().has_exception() {
                return Err(caught_exception(global_object));
            }
            step
        }
        IterationMode::FastMapKeys | IterationMode::FastMapValues | IterationMode::FastMapEntries => {
            let iterator = JSMapIterator::from_value(&sync_iterator).expect("uncheckedDowncast<JSMapIterator>");
            iterator_step_value(global_object, iterator.next())
        }
        IterationMode::FastSetValues | IterationMode::FastSetEntries => {
            let iterator = JSSetIterator::from_value(&sync_iterator).expect("uncheckedDowncast<JSSetIterator>");
            iterator_step_value(global_object, iterator.next())
        }
        other => unreachable!("RELEASE_ASSERT_NOT_REACHED: driveFastSyncIterator com {other:?}"),
    };
    Ok(match step {
        Some(value) => (value, false),
        None => (JSValue::undefined(), true),
    })
}

/// `driveSyncIterator(globalObject, vm, iterator, argument, value)`.
fn drive_sync_iterator(global_object: &JSGlobalObject, iterator: &JSAsyncFromSyncIterator, argument: JSValue) -> Result<(JSValue, bool), Thrown> {
    let sync_iterator = iterator.sync_iterator().as_value();
    if iterator.iteration_mode() != IterationMode::Generic {
        return drive_fast_sync_iterator(global_object, sync_iterator, iterator.iteration_mode());
    }
    call_sync_iterator_method_and_extract(global_object, iterator.next_method(), sync_iterator, argument)
}

/// O fim comum de `next`, `return` e `throw`: com `(value, done)` espera e continua; com a exceção rejeita a
/// promessa (`rejectWithCaughtException`; a terminação fica pendente no `VM` e a promessa sem liquidar).
fn continue_or_reject(
    global_object: &JSGlobalObject,
    iterator: &JSAsyncFromSyncIterator,
    promise: &JSPromiseRef,
    close_sync_iterator_on_rejection: bool,
    extracted: Result<(JSValue, bool), Thrown>,
) {
    match extracted {
        Ok((value, done)) => await_and_continue(global_object, iterator, promise.as_value(), close_sync_iterator_on_rejection, value, done),
        Err(thrown) => reject_with_thrown(global_object, promise, thrown),
    }
}

/// `promise->rejectWithCaughtException(vm, scope)`.
fn reject_with_thrown(global_object: &JSGlobalObject, promise: &JSPromiseRef, thrown: Thrown) {
    if let Thrown::Value(error) = thrown {
        promise.reject(global_object, error);
    }
}

/// `asyncFromSyncIteratorNext(globalObject, iterator, argument)`
/// (https://tc39.es/ecma262/#sec-%asyncfromsynciteratorprototype%.next): a promessa do pedido.
pub fn async_from_sync_iterator_next(global_object: &JSGlobalObject, iterator: &JSAsyncFromSyncIterator, argument: JSValue) -> JSValue {
    let promise = JSPromise::create(global_object.vm(), &global_object.promise_structure());
    let extracted = drive_sync_iterator(global_object, iterator, argument);
    continue_or_reject(global_object, iterator, &promise, /* close_sync_iterator_on_rejection */ true, extracted);
    promise.as_value()
}

/// `driveAsyncFromSyncIteratorWithDriver(globalObject, iterator, driver, resumeValue)`: o passo pedido por
/// um driver cooperativo (o ramo rápido de `for await`), que é retomado em vez de receber uma promessa.
pub fn drive_async_from_sync_iterator_with_driver(
    global_object: &JSGlobalObject,
    iterator: &JSAsyncFromSyncIterator,
    driver: JSValue,
    resume_value: JSValue,
) {
    match drive_sync_iterator(global_object, iterator, resume_value) {
        Ok((value, done)) => await_and_continue(global_object, iterator, driver, /* close_sync_iterator_on_rejection */ true, value, done),
        Err(Thrown::Value(error)) => JSPromise::reject_with_internal_microtask(
            global_object,
            error,
            InternalMicrotask::AsyncGeneratorDriverResume,
            driver,
            global_object.async_context(),
        ),
        Err(Thrown::Termination) => {}
    }
}

/// `callFrame->argumentCount() > 0 ? callFrame->uncheckedArgument(0) : JSValue()`.
fn optional_argument(call: &HostCall) -> JSValue {
    call.arguments().first().copied().unwrap_or_else(JSValue::empty)
}

/// `downcast<JSAsyncFromSyncIterator>(callFrame->thisValue())`.
fn this_iterator(call: &HostCall) -> crate::runtime::js_async_from_sync_iterator::JSAsyncFromSyncIteratorRef {
    JSAsyncFromSyncIterator::from_value(&call.this_value()).expect("downcast<JSAsyncFromSyncIterator>")
}

/// `asyncFromSyncIteratorPrototypeFuncNext`.
fn async_from_sync_iterator_prototype_func_next(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    Ok(async_from_sync_iterator_next(global_object, &this_iterator(call), optional_argument(call)))
}

/// `asyncFromSyncIteratorPrototypeFuncReturn`
/// (https://tc39.es/ecma262/#sec-%asyncfromsynciteratorprototype%.return).
fn async_from_sync_iterator_prototype_func_return(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    let vm = global_object.vm();
    let promise = JSPromise::create(vm, &global_object.promise_structure());

    let iterator = this_iterator(call);
    let sync_iterator = iterator.sync_iterator().as_value();

    let return_method = match get_property_named(global_object, sync_iterator, &vm.property_names.return_keyword) {
        Ok(return_method) => return_method,
        Err(thrown) => {
            reject_with_thrown(global_object, &promise, thrown);
            return Ok(promise.as_value());
        }
    };

    if return_method.is_undefined_or_null() {
        let iterator_result = create_iterator_result_object(global_object, call.argument(0), true);
        promise.resolve(global_object, iterator_result);
        return Ok(promise.as_value());
    }

    let extracted = call_sync_iterator_method_and_extract(global_object, return_method, sync_iterator, optional_argument(call));
    continue_or_reject(global_object, &iterator, &promise, /* close_sync_iterator_on_rejection */ false, extracted);
    Ok(promise.as_value())
}

/// O corpo de `asyncFromSyncIteratorPrototypeFuncThrow` até a chamada do método: `(value, done)` do `throw`
/// do iterador síncrono, ou o erro. Sem `throw`, fecha o iterador (`return`) e falha com um `TypeError`
/// (https://tc39.es/ecma262/#sec-%asyncfromsynciteratorprototype%.throw, passo 7).
fn call_sync_iterator_throw(
    global_object: &JSGlobalObject,
    sync_iterator: JSValue,
    argument: JSValue,
) -> Result<(JSValue, bool), Thrown> {
    let vm = global_object.vm();
    let throw_method = get_property_named(global_object, sync_iterator, &vm.property_names.throw_keyword)?;

    if throw_method.is_undefined_or_null() {
        let return_method = get_property_named(global_object, sync_iterator, &vm.property_names.return_keyword)?;
        if !return_method.is_undefined_or_null() {
            let return_result = call_microtask(global_object, return_method, sync_iterator, &[], "Iterator return method is not callable.")?;
            if !return_result.is_object() {
                return Err(type_error(global_object, "Iterator result interface is not an object."));
            }
        }

        return Err(type_error(global_object, "Iterator does not provide a throw method."));
    }

    call_sync_iterator_method_and_extract(global_object, throw_method, sync_iterator, argument)
}

/// `asyncFromSyncIteratorPrototypeFuncThrow`
/// (https://tc39.es/ecma262/#sec-%asyncfromsynciteratorprototype%.throw).
fn async_from_sync_iterator_prototype_func_throw(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    let promise = JSPromise::create(global_object.vm(), &global_object.promise_structure());

    let iterator = this_iterator(call);
    let sync_iterator = iterator.sync_iterator().as_value();

    let extracted = call_sync_iterator_throw(global_object, sync_iterator, optional_argument(call));
    continue_or_reject(global_object, &iterator, &promise, /* close_sync_iterator_on_rejection */ true, extracted);
    Ok(promise.as_value())
}

host_function!(async_from_sync_iterator_prototype_func_next_host, async_from_sync_iterator_prototype_func_next);
host_function!(async_from_sync_iterator_prototype_func_return_host, async_from_sync_iterator_prototype_func_return);
host_function!(async_from_sync_iterator_prototype_func_throw_host, async_from_sync_iterator_prototype_func_throw);

/// `JSFunction::create(vm, owner, 1, vm.propertyNames->next.string(), asyncFromSyncIteratorPrototypeFuncNext,
/// ImplementationVisibility::Public, NoIntrinsic)`, o `m_asyncFromSyncIteratorProtoNextFunction` do
/// `JSGlobalObject`.
pub fn create_async_from_sync_iterator_proto_next_function(vm: &VM, global_object: &JSGlobalObject) -> JSFunctionRef {
    JSFunction::create_native(
        vm,
        global_object,
        1,
        vm.property_names.next.string().string(),
        async_from_sync_iterator_prototype_func_next_host,
        ImplementationVisibility::Public,
        Intrinsic::NoIntrinsic,
        call_host_function_as_constructor,
    )
}

/// `class AsyncFromSyncIteratorPrototype final : public JSNonFinalObject`.
pub struct AsyncFromSyncIteratorPrototype;

impl AsyncFromSyncIteratorPrototype {
    /// `StructureFlags = Base::StructureFlags`.
    pub const STRUCTURE_FLAGS: u32 = JSNonFinalObject::STRUCTURE_FLAGS;

    /// `createStructure(vm, globalObject, prototype)` (`AsyncFromSyncIteratorPrototypeInlines.h`).
    pub fn create_structure(vm: &VM, global_object: &JSGlobalObject, prototype: JSValue) -> StructureRef {
        Structure::create(
            vm,
            Some(global_object),
            prototype,
            TypeInfo::new(JSType::ObjectType, AsyncFromSyncIteratorPrototype::STRUCTURE_FLAGS),
            &ASYNC_FROM_SYNC_ITERATOR_PROTOTYPE_S_INFO,
        )
    }

    /// `create(vm, globalObject, structure)`: `AsyncFromSyncIteratorPrototype(vm, structure)` e
    /// `finishCreation(vm, globalObject)`; `next_function` é o `asyncFromSyncIteratorPrototypeNextFunction()`
    /// do global (`create_async_from_sync_iterator_proto_next_function`).
    pub fn create(vm: &VM, global_object: &JSGlobalObject, structure: &StructureRef, next_function: JSValue) -> JSObjectRef {
        let prototype = JSObject::allocate(vm, structure);
        prototype.finish_creation(vm);
        prototype.put_direct_without_transition(vm, &PropertyName::from_identifier(&vm.property_names.next), next_function, DONT_ENUM);
        put_direct_native_function_without_transition(
            vm,
            global_object,
            &prototype,
            &vm.property_names.return_keyword,
            1,
            async_from_sync_iterator_prototype_func_return_host,
            ImplementationVisibility::Public,
            Intrinsic::NoIntrinsic,
            DONT_ENUM,
        );
        put_direct_native_function_without_transition(
            vm,
            global_object,
            &prototype,
            &vm.property_names.throw_keyword,
            1,
            async_from_sync_iterator_prototype_func_throw_host,
            ImplementationVisibility::Public,
            Intrinsic::NoIntrinsic,
            DONT_ENUM,
        );
        prototype
    }
}
