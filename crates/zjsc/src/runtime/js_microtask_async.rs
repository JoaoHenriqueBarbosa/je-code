//! Porte das partes de `runtime/JSMicrotask.cpp` que dirigem async generators e o `AsyncFromSyncIterator`:
//! `asyncGeneratorBodyCall`, `...CompleteStep`, `...DrainQueue`, `...AwaitReturn`,
//! `...UnwrapYieldResumption`, `...Yield`, `...YieldAwaited`, `...DispatchSuspend`,
//! `settleDriverWithIteratorResult`, `asyncFromSyncIteratorContinueOrDone`, o ramo de `JSAsyncGenerator` do
//! `asyncGeneratorDriverResume`, `enqueueAsyncGeneratorDriver`, `asyncIteratorNextWithDriver` e o ramo do
//! `runInternalMicrotask` das seis tarefas `AsyncFromSyncIteratorContinue`, `AsyncFromSyncIteratorDone`,
//! `AsyncGeneratorYieldAwaited`, `AsyncGeneratorBodyCallNormal`, `AsyncGeneratorBodyCallReturn` e
//! `AsyncGeneratorAwaitReturn` (`run_async_internal_microtask`, chamado pelo
//! `PromiseHost::run_internal_microtask` do `JSGlobalObject`). A tarefa de async function
//! (`AsyncFunctionResume`), o `asyncFunctionDrive` e o despacho de `AsyncGeneratorDriverResume` são de
//! `js_microtask.rs` e `promise_global_functions.rs`.
//!
//! DIVERGÊNCIAS:
//! - `MicrotaskCallCache` não existe (ver `js_microtask.rs`): `callMicrotask` é `call_microtask`.
//! - `AsyncContextSwapScope` é o de `js_microtask.rs`: as tarefas daqui reinstalam o contexto de `arguments[3]`.
//! - `generator->realm()` é o realm da `Structure` da célula (`Structure::realm`).
//! - Uma exceção de terminação vinda de `callMicrotask` (`clearExceptionExceptTermination` falso) aborta o
//!   passo e fica pendente no `VM`, como no C++; `PromiseHost::resolve`/`reject` não deixam exceção, então
//!   os `RETURN_IF_EXCEPTION` depois delas não têm o que testar.
//! - `asyncModuleExecutionResume` (o `JSModuleRecord` que `asyncGeneratorDriverResume` aceita como
//!   driver) ainda não existe (ver `js_microtask.rs`).
//! - `promiseSpeciesWatchpointSet().state() != IsWatched` é `promiseSpeciesWatchpointIsValid` sobre uma
//!   promessa recém criada na `promiseStructure` do realm: o `JSGlobalObject` não guarda o conjunto de
//!   vigilância, e com a `Structure` original só restam as duas checagens do conjunto (o `constructor` do
//!   protótipo e o `@@species` do construtor).

use crate::runtime::iterator_operations::{ITERATOR_RESULT_OBJECT_DONE_PROPERTY_OFFSET, ITERATOR_RESULT_OBJECT_VALUE_PROPERTY_OFFSET};
use crate::runtime::js_async_from_sync_iterator::JSAsyncFromSyncIterator;
use crate::runtime::js_async_function_generator::ResumeMode;
use crate::runtime::js_async_generator::{
    is_suspended_yield_state, AsyncGeneratorResumeMode, AsyncGeneratorState, AsyncGeneratorSuspendReason, JSAsyncGenerator,
    JSAsyncGeneratorRef, REASON_MASK, REASON_SHIFT,
};
use crate::runtime::js_global_object::{JSGlobalObject, JSGlobalObjectRef};
use crate::runtime::js_microtask::call_microtask;
use crate::runtime::js_object::JSObject;
use crate::runtime::js_promise::{JSPromise, Status};
use crate::runtime::js_promise_host::{get_property_named, PromiseHost, Thrown};
use crate::runtime::js_value::{js_boolean, js_number_i32, JSValue};
use crate::runtime::microtask::InternalMicrotask;
use crate::runtime::string_regexp_support::create_iterator_result_object;

/// O `generator->realm()`: o realm da `Structure` da célula.
pub fn realm_of(cell: &JSObject) -> JSGlobalObjectRef {
    cell.structure().realm().expect("a Structure da célula do gerador sempre tem realm")
}

/// `static_cast<JSPromise::Status>(payload)`.
fn status_of_payload(payload: u8) -> Status {
    Status::from_flags(u16::from(payload))
}

/// `(state & ~reasonMask) | static_cast<int32_t>(reason)`.
pub fn with_suspend_reason(state: i32, reason: AsyncGeneratorSuspendReason) -> i32 {
    (state & !REASON_MASK) | reason as i32
}

/// O `Producer` de `settleDriverWithIteratorResult<Producer>`: o `JSAsyncGenerator` ou o
/// `JSAsyncFromSyncIterator` que entrega o resultado a um driver cooperativo.
pub trait DriverResultProducer {
    /// `producer->realm()`.
    fn realm(&self) -> JSGlobalObjectRef;

    /// `producer->cachedDriverResult()`.
    fn cached_driver_result(&self) -> JSValue;

    /// A condição de reuso do objeto em cache: o `JSAsyncGenerator` é visível ao usuário e pode ter vários
    /// consumidores, então só reusa para o driver a quem o objeto foi entregue por último
    /// (`producer->cachedDriverResultTarget() == target`); o `JSAsyncFromSyncIterator` atende um driver só.
    fn can_reuse_cached_driver_result(&self, target: JSValue) -> bool;

    /// `producer->setCachedDriverResult(vm, iteratorResult)` e, no gerador, o
    /// `setCachedDriverResultTarget(vm, target)`.
    fn store_driver_result(&self, result: JSValue, target: JSValue);
}

impl DriverResultProducer for JSAsyncGenerator {
    fn realm(&self) -> JSGlobalObjectRef {
        realm_of(self)
    }

    fn cached_driver_result(&self) -> JSValue {
        JSAsyncGenerator::cached_driver_result(self)
    }

    fn can_reuse_cached_driver_result(&self, target: JSValue) -> bool {
        self.cached_driver_result_target() == target
    }

    fn store_driver_result(&self, result: JSValue, target: JSValue) {
        self.set_cached_driver_result(result);
        self.set_cached_driver_result_target(target);
    }
}

impl DriverResultProducer for JSAsyncFromSyncIterator {
    fn realm(&self) -> JSGlobalObjectRef {
        realm_of(self)
    }

    fn cached_driver_result(&self) -> JSValue {
        JSAsyncFromSyncIterator::cached_driver_result(self)
    }

    fn can_reuse_cached_driver_result(&self, _target: JSValue) -> bool {
        true
    }

    fn store_driver_result(&self, result: JSValue, _target: JSValue) {
        let object = JSObject::from_value(&result).expect("o resultado de iterador é um objeto");
        self.set_cached_driver_result(&object);
    }
}

/// `settleDriverWithIteratorResult(globalObject, vm, producer, value, done, target)`: liquida o pedido de
/// um driver cooperativo (o alvo é um driver, não a promessa de um `.next()`). O resultado pertence ao
/// realm do produtor. Enquanto o watchpoint de `then` vale, o resultado é entregue internamente e nunca
/// escapa, então o objeto em cache é reaproveitado; invalidado, `.then` é observável e o objeto é novo,
/// pelo caminho completo de `resolve`.
fn settle_driver_with_iterator_result(
    global_object: &JSGlobalObject,
    producer: &dyn DriverResultProducer,
    value: JSValue,
    done: bool,
    target: JSValue,
) {
    let realm = producer.realm();
    let vm = realm.vm();
    // AsyncGeneratorDriverResume runs the driver under the async context active now.
    let async_context = global_object.async_context();
    if realm.promise_then_watchpoint_is_valid() {
        let cached = producer.cached_driver_result();
        // O objeto em cache é entregue ao `target` numa microtask de cumprimento e só lê `value`/`done`
        // quando ela roda; um driver consome em série, então mutar e reusar é seguro enquanto a entrega
        // anterior já foi consumida (ver `can_reuse_cached_driver_result`).
        let iterator_result = if cached.is_object() && producer.can_reuse_cached_driver_result(target) {
            let object = cached.as_object();
            object.put_direct_offset(vm, ITERATOR_RESULT_OBJECT_VALUE_PROPERTY_OFFSET, value);
            object.put_direct_offset(vm, ITERATOR_RESULT_OBJECT_DONE_PROPERTY_OFFSET, js_boolean(done));
            cached
        } else {
            let created = create_iterator_result_object(&realm, value, done);
            producer.store_driver_result(created, target);
            created
        };
        JSPromise::fulfill_with_internal_microtask(&*realm, iterator_result, InternalMicrotask::AsyncGeneratorDriverResume, target, async_context);
        return;
    }

    let iterator_result = create_iterator_result_object(&realm, value, done);
    JSPromise::resolve_with_internal_microtask(&*realm, iterator_result, InternalMicrotask::AsyncGeneratorDriverResume, target, async_context);
}

/// `asyncFromSyncIteratorContinueOrDone(globalObject, vm, iterator, result, status, done, cache)`.
fn async_from_sync_iterator_continue_or_done(
    global_object: &JSGlobalObject,
    iterator: &JSAsyncFromSyncIterator,
    result: JSValue,
    status: Status,
    done: bool,
) {
    let (target, close_sync_iterator_on_rejection) = iterator.extract_target();
    let target = target.expect("AsyncFromSyncIteratorContinue sem alvo");

    match status {
        Status::Pending => unreachable!("RELEASE_ASSERT_NOT_REACHED: AsyncFromSyncIteratorContinue com a promessa pendente"),
        Status::Rejected => {
            if !done && close_sync_iterator_on_rejection {
                let sync_iterator = iterator.sync_iterator().as_value();
                let vm = global_object.vm();
                let closed = get_property_named(global_object, sync_iterator, &vm.property_names.return_keyword).and_then(|return_method| {
                    if return_method.is_callable() {
                        call_microtask(global_object, return_method, sync_iterator, &[], "return is not a function").map(|_| ())
                    } else {
                        Ok(())
                    }
                });
                // O `catchScope`: o lançamento do `return` é descartado, a terminação aborta o passo.
                if let Err(Thrown::Termination) = closed {
                    return;
                }
            }
            if let Some(promise) = JSPromise::from_value(&target) {
                promise.reject(global_object, result);
            } else {
                JSPromise::reject_with_internal_microtask(
                    global_object,
                    result,
                    InternalMicrotask::AsyncGeneratorDriverResume,
                    target,
                    global_object.async_context(),
                );
            }
        }
        Status::Fulfilled => {
            // A real .next()/.return()/.throw() settles its result JSPromise; the result object is created
            // fresh (it is observable by user code).
            if let Some(promise) = JSPromise::from_value(&target) {
                let result_object = create_iterator_result_object(global_object, result, done);
                promise.resolve(global_object, result_object);
                return;
            }
            settle_driver_with_iterator_result(global_object, iterator, result, done, target);
        }
    }
}

/// `asyncGeneratorCompleteStep(globalObject, generator, value, isThrow, done)`
/// (https://tc39.es/ecma262/#sec-asyncgeneratorcompletestep).
fn async_generator_complete_step(global_object: &JSGlobalObject, generator: &JSAsyncGenerator, value: JSValue, is_throw: bool, done: bool) {
    // 1-4. Remove the first request from the queue.
    let target = generator.dequeue();

    // A real .next()/.throw()/.return() settles its result JSPromise directly.
    if let Some(promise) = JSPromise::from_value(&target) {
        // 6. throw completion -> reject.
        if is_throw {
            promise.reject(global_object, value);
            return;
        }

        // 7. normal completion -> resolve with CreateIteratorResultObject(value, done). The iterator
        // result object belongs to the generator's realm, not the realm of whoever called next().
        let iterator_result = create_iterator_result_object(&realm_of(generator), value, done);
        promise.resolve(global_object, iterator_result);
        return;
    }

    // resolveWithInternalMicrotask keeps resolvePromise's thenable check, matching a real Promise settlement.
    if is_throw {
        JSPromise::reject_with_internal_microtask(
            global_object,
            value,
            InternalMicrotask::AsyncGeneratorDriverResume,
            target,
            global_object.async_context(),
        );
        return;
    }

    settle_driver_with_iterator_result(global_object, generator, value, done, target);
}

/// `asyncGeneratorAwaitReturn(globalObject, generator)` (https://tc39.es/ecma262/#sec-asyncgeneratorawaitreturn).
pub fn async_generator_await_return(global_object: &JSGlobalObject, generator: &JSAsyncGenerator) {
    debug_assert_eq!(generator.state(), AsyncGeneratorState::DrainingQueue as i32);
    JSPromise::resolve_with_internal_microtask_for_async_await(
        global_object,
        generator.resume_value(),
        InternalMicrotask::AsyncGeneratorAwaitReturn,
        generator.as_value(),
    );
}

/// `asyncGeneratorDrainQueue(globalObject, generator)` (https://tc39.es/ecma262/#sec-asyncgeneratordrainqueue).
fn async_generator_drain_queue(global_object: &JSGlobalObject, generator: &JSAsyncGenerator) {
    debug_assert_eq!(generator.state(), AsyncGeneratorState::DrainingQueue as i32);

    // 3. Repeat, while the queue is not empty.
    while !generator.is_queue_empty() {
        let resume_mode = generator.resume_mode();

        // 5c. return completion -> AsyncGeneratorAwaitReturn and stop.
        if resume_mode == AsyncGeneratorResumeMode::Return as i32 {
            async_generator_await_return(global_object, generator);
            return;
        }

        // 5d. throw -> reject; normal -> resolve { undefined, true }.
        let is_throw = resume_mode == AsyncGeneratorResumeMode::Throw as i32;
        let value = if is_throw { generator.resume_value() } else { JSValue::undefined() };
        async_generator_complete_step(global_object, generator, value, is_throw, /* done */ true);
    }

    // 3a. queue empty -> completed.
    generator.set_state(AsyncGeneratorState::Completed as i32);
}

/// `asyncGeneratorBodyCall(globalObject, generator, resumeValue, resumeMode, cache)`
/// (https://tc39.es/ecma262/#sec-asyncgeneratorresume, e o tratamento de conclusão do `AsyncGeneratorStart`).
fn async_generator_body_call(global_object: &JSGlobalObject, generator: &JSAsyncGenerator, resume_value: JSValue, resume_mode: i32) {
    let state = generator.state();
    generator.set_state(AsyncGeneratorState::Executing as i32);

    let generator_function = generator.next();
    let generator_this = generator.this_value();
    let generator_frame = generator.frame();

    let arguments = [generator.as_value(), js_number_i32(state >> REASON_SHIFT), resume_value, js_number_i32(resume_mode), generator_frame];
    let (value, error) = match call_microtask(global_object, generator_function, generator_this, &arguments, "handler is not a function") {
        Ok(value) => (value, None),
        Err(Thrown::Value(error)) => (JSValue::empty(), Some(error)),
        Err(Thrown::Termination) => return,
    };

    let state = generator.state();

    // The body suspended at an `await` or a `yield`/`yield*`.
    if state > 0 {
        async_generator_dispatch_suspend(global_object, generator, value);
        return;
    }

    // https://tc39.es/ecma262/#sec-asyncgeneratorstart
    debug_assert_eq!(state, AsyncGeneratorState::Executing as i32);
    // 4.g. Set acGen.[[AsyncGeneratorState]] to draining-queue.
    generator.set_state(AsyncGeneratorState::DrainingQueue as i32);
    // 4.h. If result is a normal completion, set result to NormalCompletion(undefined).
    // 4.i. If result is a return completion, set result to NormalCompletion(result.[[Value]]).
    // 4.j. Perform AsyncGeneratorCompleteStep(acGen, result, true).
    async_generator_complete_step(global_object, generator, error.unwrap_or(value), error.is_some(), /* done */ true);
    // 4.k. Perform AsyncGeneratorDrainQueue(acGen).
    async_generator_drain_queue(global_object, generator);
}

/// `asyncGeneratorUnwrapYieldResumption(globalObject, generator, resumeValue, resumeMode, cache)`
/// (https://tc39.es/ecma262/#sec-asyncgeneratorunwrapyieldresumption).
fn async_generator_unwrap_yield_resumption(global_object: &JSGlobalObject, generator: &JSAsyncGenerator, resume_value: JSValue, resume_mode: i32) {
    let state = generator.state();
    // A suspended-start (Init, state 0) generator may be resumed here -- e.g. the for-await driver
    // starting a fresh producer -- which is always a Normal-mode resume handled by asyncGeneratorBodyCall
    // below. Only the ReturnMode branch, which edits the suspend-reason bits, needs a positive state.
    debug_assert!(state > 0 || (state == AsyncGeneratorState::Init as i32 && resume_mode != AsyncGeneratorResumeMode::Return as i32));
    // 1. If resumptionValue is not a return completion, return ? resumptionValue.
    if resume_mode != AsyncGeneratorResumeMode::Return as i32 {
        async_generator_body_call(global_object, generator, resume_value, resume_mode);
        return;
    }

    // 2. Let awaited be Completion(Await(resumptionValue.[[Value]])).
    // 3. If awaited is a throw completion, return ? awaited.
    // 4. Assert: awaited is a normal completion.
    // 5. Return ReturnCompletion(awaited.[[Value]]).
    generator.set_state(with_suspend_reason(state, AsyncGeneratorSuspendReason::Await));
    JSPromise::resolve_with_internal_microtask_for_async_await(
        global_object,
        resume_value,
        InternalMicrotask::AsyncGeneratorBodyCallReturn,
        generator.as_value(),
    );
}

/// `asyncGeneratorResume(globalObject, generator, cache)` (https://tc39.es/ecma262/#sec-asyncgeneratorresume).
pub fn async_generator_resume(global_object: &JSGlobalObject, generator: &JSAsyncGenerator) {
    // 1. Assert: gen.[[AsyncGeneratorState]] is either suspended-start or suspended-yield.
    debug_assert!(generator.state() == AsyncGeneratorState::Init as i32 || is_suspended_yield_state(generator.state()));
    async_generator_unwrap_yield_resumption(global_object, generator, generator.resume_value(), generator.resume_mode());
}

/// `resumeValueOrUndefined(resumeValue)`.
fn resume_value_or_undefined(resume_value: JSValue) -> JSValue {
    if resume_value.is_empty() {
        JSValue::undefined()
    } else {
        resume_value
    }
}

/// `enqueueAsyncGeneratorDriver(globalObject, iterator, driver, resumeValue, cache)`.
pub fn enqueue_async_generator_driver(global_object: &JSGlobalObject, iterator: &JSAsyncGenerator, driver: JSValue, resume_value: JSValue) {
    let resume_value = resume_value_or_undefined(resume_value);

    // Mirror AsyncGeneratorEnqueue's completed-state fast path: settle { undefined, true } without enqueuing.
    let state = iterator.state();
    if state == AsyncGeneratorState::Completed as i32 {
        settle_driver_with_iterator_result(global_object, iterator, JSValue::undefined(), /* done */ true, driver);
        return;
    }

    iterator.enqueue(resume_value, AsyncGeneratorResumeMode::Normal as i32, driver);

    // https://tc39.es/ecma262/#sec-asyncgeneratorenqueue step 6: a non-busy generator resumes immediately.
    if state == AsyncGeneratorState::Init as i32 || is_suspended_yield_state(state) {
        async_generator_resume(global_object, iterator);
    }
}

/// `globalObject->promiseSpeciesWatchpointSet().state() == IsWatched`: o conjunto vigia
/// `Promise.prototype.constructor` e `Promise[@@species]` (ver `promise_species_watchpoint_is_valid`), que
/// confere uma promessa recém-criada no realm.
pub fn promise_species_is_watched(global_object: &JSGlobalObject) -> bool {
    let probe = JSPromise::create(global_object.vm(), &global_object.promise_structure());
    global_object.promise_species_watchpoint_is_valid(&probe)
}

/// `asyncIteratorNextWithDriver(globalObject, iterator, driver, resumeValue, cache)`: se a espécie de
/// `Promise` foi adulterada depois da abertura fundida, volta ao `next()` de verdade, para o `Await` do
/// consumidor ainda fazer o `PromiseResolve` observável (a leitura de `Promise.prototype.constructor`)
/// que o driver fundido pularia.
pub fn async_iterator_next_with_driver(global_object: &JSGlobalObject, iterator: JSValue, driver: JSValue, resume_value: JSValue) -> JSValue {
    let vm = global_object.vm();
    let generator = JSAsyncGenerator::from_value(&iterator);

    if !promise_species_is_watched(global_object) {
        if let Some(generator) = generator {
            return crate::runtime::async_generator_prototype::async_generator_next(global_object, &generator, resume_value_or_undefined(resume_value));
        }
        let iterator = JSAsyncFromSyncIterator::from_value(&iterator).expect("uncheckedDowncast<JSAsyncFromSyncIterator>");
        return crate::runtime::async_from_sync_iterator_prototype::async_from_sync_iterator_next(global_object, &iterator, resume_value);
    }

    match generator {
        Some(generator) => enqueue_async_generator_driver(global_object, &generator, driver, resume_value),
        None => {
            let iterator = JSAsyncFromSyncIterator::from_value(&iterator).expect("uncheckedDowncast<JSAsyncFromSyncIterator>");
            crate::runtime::async_from_sync_iterator_prototype::drive_async_from_sync_iterator_with_driver(global_object, &iterator, driver, resume_value);
        }
    }
    vm.fast_async_generator_sentinel()
}

/// `asyncGeneratorYield(globalObject, generator, value, cache)` (https://tc39.es/ecma262/#sec-asyncgeneratoryield).
fn async_generator_yield(global_object: &JSGlobalObject, generator: &JSAsyncGenerator, value: JSValue) {
    // Stay executing across CompleteStep so reentrant requests only enqueue.
    let state = generator.state();
    generator.set_state(with_suspend_reason(state, AsyncGeneratorSuspendReason::Await));

    // 9. Perform AsyncGeneratorCompleteStep(gen, completion, false, previousRealm).
    async_generator_complete_step(global_object, generator, value, /* is_throw */ false, /* done */ false);

    // 10. Let queue be gen.[[AsyncGeneratorQueue]].
    // 11. If queue is not empty, then
    if !generator.is_queue_empty() {
        // 11.a. NOTE: Execution continues without suspending the generator.
        // 11.b. Let toYield be the first element of queue.
        // 11.c. Let resumptionValue be Completion(toYield.[[Completion]]).
        // 11.d. Return ? AsyncGeneratorUnwrapYieldResumption(resumptionValue).
        async_generator_unwrap_yield_resumption(global_object, generator, generator.resume_value(), generator.resume_mode());
        return;
    }
    // 12. Set gen.[[AsyncGeneratorState]] to suspended-yield.
    let state = generator.state();
    generator.set_state(with_suspend_reason(state, AsyncGeneratorSuspendReason::Yield));
}

/// `asyncGeneratorYieldAwaited(globalObject, generator, result, status, cache)`: o `Await` do operando de
/// `yield` simples (`AsyncGeneratorYield(? Await(value))`) liquidou.
fn async_generator_yield_awaited(global_object: &JSGlobalObject, generator: &JSAsyncGenerator, result: JSValue, status: Status) {
    match status {
        Status::Pending => unreachable!("RELEASE_ASSERT_NOT_REACHED: AsyncGeneratorYieldAwaited com a promessa pendente"),
        // `? Await(value)` threw -> resume the body with a throw at the yield.
        Status::Rejected => async_generator_body_call(global_object, generator, result, AsyncGeneratorResumeMode::Throw as i32),
        Status::Fulfilled => async_generator_yield(global_object, generator, result),
    }
}

/// `asyncGeneratorDispatchSuspend(globalObject, generator, value, cache)`: o corpo suspendeu (`state > 0`);
/// despacha pela razão:
///   Await        `await x`   -> resume o corpo quando `x` liquidar (AsyncGeneratorBodyCallNormal).
///   Yield        `yield x`   -> AsyncGeneratorYield(? Await(value)): o Await primeiro (AsyncGeneratorYieldAwaited).
///   YieldNoAwait `yield* x`  -> AsyncGeneratorYield(value): entrega direto.
fn async_generator_dispatch_suspend(global_object: &JSGlobalObject, generator: &JSAsyncGenerator, value: JSValue) {
    if value == global_object.vm().fast_async_generator_sentinel() {
        return;
    }

    let state = generator.state();
    match state & REASON_MASK {
        reason if reason == AsyncGeneratorSuspendReason::Await as i32 => {
            JSPromise::resolve_with_internal_microtask_for_async_await(
                global_object,
                value,
                InternalMicrotask::AsyncGeneratorBodyCallNormal,
                generator.as_value(),
            );
        }
        reason if reason == AsyncGeneratorSuspendReason::Yield as i32 => {
            generator.set_state(with_suspend_reason(state, AsyncGeneratorSuspendReason::Await));
            JSPromise::resolve_with_internal_microtask_for_async_await(
                global_object,
                value,
                InternalMicrotask::AsyncGeneratorYieldAwaited,
                generator.as_value(),
            );
        }
        reason if reason == AsyncGeneratorSuspendReason::YieldNoAwait as i32 => async_generator_yield(global_object, generator, value),
        reason => unreachable!("razão de suspensão do async generator inválida: {reason}"),
    }
}

/// `asyncGeneratorBodyCallNormal(...)` e `asyncGeneratorBodyCallReturn(...)`: o `Await` do corpo liquidou; a
/// rejeição retoma com `throw`, o cumprimento com `fulfilled_mode` (`Normal` ou `Return`).
fn async_generator_body_call_after_await(
    global_object: &JSGlobalObject,
    generator: &JSAsyncGenerator,
    result: JSValue,
    status: Status,
    fulfilled_mode: AsyncGeneratorResumeMode,
) {
    match status {
        Status::Pending => unreachable!("RELEASE_ASSERT_NOT_REACHED: AsyncGeneratorBodyCall com a promessa pendente"),
        Status::Rejected => async_generator_body_call(global_object, generator, result, AsyncGeneratorResumeMode::Throw as i32),
        Status::Fulfilled => async_generator_body_call(global_object, generator, result, fulfilled_mode as i32),
    }
}

/// `asyncGeneratorAwaitReturnContinuation(globalObject, generator, result, status)`
/// (https://tc39.es/ecma262/#sec-asyncgeneratorawaitreturn).
fn async_generator_await_return_continuation(global_object: &JSGlobalObject, generator: &JSAsyncGenerator, result: JSValue, status: Status) {
    debug_assert_eq!(generator.state(), AsyncGeneratorState::DrainingQueue as i32);

    match status {
        Status::Pending => unreachable!("RELEASE_ASSERT_NOT_REACHED: AsyncGeneratorAwaitReturn com a promessa pendente"),
        Status::Fulfilled => async_generator_complete_step(global_object, generator, result, /* is_throw */ false, /* done */ true),
        Status::Rejected => async_generator_complete_step(global_object, generator, result, /* is_throw */ true, /* done */ true),
    }

    async_generator_drain_queue(global_object, generator);
}

/// `uncheckedDowncast<JSAsyncGenerator>(value)`.
fn async_generator_of(value: &JSValue) -> JSAsyncGeneratorRef {
    JSAsyncGenerator::from_value(value).expect("uncheckedDowncast<JSAsyncGenerator>")
}

/// O ramo de `JSAsyncGenerator` de `asyncGeneratorDriverResume(vm, context, resolution, status, cache)`: um
/// driver de `for await` (o ramo rápido de `op_async_iterator_next`) retomado direto pelo produtor que ele
/// consome; o `context` é o gerador que é o driver. Os outros drivers (async function e módulo) são o
/// `async_generator_driver_resume` de `js_microtask.rs`, que chama esta função.
pub fn async_generator_driver_resume_generator(generator: &JSAsyncGenerator, resolution: JSValue, resume_mode: ResumeMode) {
    async_generator_body_call(&realm_of(generator), generator, resolution, resume_mode as i32);
}

/// O ramo do `runInternalMicrotask` das seis tarefas de async generator e `AsyncFromSyncIterator`
/// (`AsyncFromSyncIteratorContinue`, `...Done`, `AsyncGeneratorYieldAwaited`, `...BodyCallNormal`,
/// `...BodyCallReturn` e `...AwaitReturn`); `arguments[1]` é a resolução e `arguments[2]` o contexto. Cada
/// tarefa roda no realm da célula do contexto (`generator->realm()`), não no do laço de microtasks, por isso
/// o `global_object` do laço não é usado. `AsyncFunctionResume` e `AsyncGeneratorDriverResume` são de
/// `js_microtask.rs`.
pub fn run_async_internal_microtask(global_object: &JSGlobalObject, task: InternalMicrotask, payload: u8, arguments: [JSValue; 4]) {
    let resolution = arguments[1];
    let context = arguments[2];
    let status = status_of_payload(payload);
    // `arguments[3]` é o contexto assíncrono do agendamento.
    let _async_context_scope = crate::runtime::js_microtask::AsyncContextSwapScope::new(global_object.vm(), arguments[3]);

    match task {
        InternalMicrotask::AsyncFromSyncIteratorContinue | InternalMicrotask::AsyncFromSyncIteratorDone => {
            let iterator = JSAsyncFromSyncIterator::from_value(&context).expect("uncheckedDowncast<JSAsyncFromSyncIterator>");
            async_from_sync_iterator_continue_or_done(
                &realm_of(&iterator),
                &iterator,
                resolution,
                status,
                task == InternalMicrotask::AsyncFromSyncIteratorDone,
            );
        }
        InternalMicrotask::AsyncGeneratorYieldAwaited => {
            let generator = async_generator_of(&context);
            async_generator_yield_awaited(&realm_of(&generator), &generator, resolution, status);
        }
        InternalMicrotask::AsyncGeneratorBodyCallNormal => {
            let generator = async_generator_of(&context);
            async_generator_body_call_after_await(&realm_of(&generator), &generator, resolution, status, AsyncGeneratorResumeMode::Normal);
        }
        InternalMicrotask::AsyncGeneratorBodyCallReturn => {
            let generator = async_generator_of(&context);
            async_generator_body_call_after_await(&realm_of(&generator), &generator, resolution, status, AsyncGeneratorResumeMode::Return);
        }
        InternalMicrotask::AsyncGeneratorAwaitReturn => {
            let generator = async_generator_of(&context);
            async_generator_await_return_continuation(&realm_of(&generator), &generator, resolution, status);
        }
        other => unreachable!("run_async_internal_microtask recebeu {other:?}, que não é tarefa de async generator nem de AsyncFromSyncIterator"),
    }
}
