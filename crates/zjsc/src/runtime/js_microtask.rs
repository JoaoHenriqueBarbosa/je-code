//! Porte de `runtime/JSMicrotask.cpp` (`runInternalMicrotask` e as tarefas de promessa que ela despacha):
//! o corpo de cada `InternalMicrotask` de `Promise` (`PromiseResolveThenableJob*`,
//! `PromiseResolveWithoutHandlerJob`, `PromiseFulfillWithoutHandlerJob`, `PromiseReactionJob`,
//! `PromiseRace/All/AllSettled/AnyResolveJob`, `PromiseFinallyReactionJob`, `PromiseFinallyAwaitJob`) e as
//! três tarefas de chamada de função (`InvokeFunctionJob`, `BunPerformMicrotaskJob`,
//! `BunInvokeJobWithArguments`). O despacho (`switch (task)`) é o
//! `PromiseHost::run_internal_microtask` do `JSGlobalObject` (`promise_constructor.rs`), e a fila é a de
//! `microtask_queue.rs`.
//!
//! Cada função devolve `Err(Thrown)` onde o C++ deixa a exceção pendente no `VM` (o `RETURN_IF_EXCEPTION`
//! e o `RELEASE_AND_RETURN` que a tarefa não captura): o despacho a deixa pendente (`rethrow`) para o
//! laço de `VM::drain_microtasks` descartá-la, e a de terminação segue pendente.
//!
//! A tarefa de async function (`AsyncFunctionResume`, com `asyncFunctionGeneratorBodyCall` e
//! `asyncFunctionArrangeAwaitResume`) e o `asyncFunctionDrive` (a `LinkTimeConstant` de
//! `promise_global_functions.rs`) também vivem aqui.
//!
//! LACUNAS, e por quê (o `match` do despacho responde `panic!` com o nome do que falta):
//! - As tarefas do carregador de módulos assíncrono (`Module*`, `DynamicImport*`,
//!   `ImportModuleNamespace`; o carregamento do porte é síncrono, ver `js_module_loader.rs`) e as de
//!   WebAssembly e a `Opaque`: dependem do `JSWebAssembly` e do carregador de promessas, que não existem
//!   aqui. As tarefas de async generator e de `AsyncFromSyncIterator` são de `js_microtask_async.rs`.
//!
//! O top-level await de módulo vive aqui, como no C++: `asyncModuleResolveEvaluation`,
//! `asyncModuleExecutionResume`, `asyncModuleExecutionDone` e o ramo `JSModuleRecord` de
//! `asyncGeneratorDriverResume`. O `module->realm()` é o realm do porte (um só: `js_module_record::realm`).
//!
//! DIVERGÊNCIAS:
//! - `MicrotaskCallCache` (cache de chamada do laço de drenagem) não existe: `callMicrotask` é
//!   `call_with_error_message` (o caminho lento do C++, que é o que o cache só acelera).
//! - `AsyncContextSwapScope` (o contexto assíncrono do Bun, `AsyncLocalStorage`): o `m_asyncContextData` é o campo
//!   `async_context` do `VM`, ligado por `vm.set_async_context_tracking` (o `node:async_hooks`). O escopo instala o
//!   contexto capturado (`arguments[3]` de `PromiseReactionJob` com o bit do payload, de `AsyncFunctionResume` e do
//!   driver de async function; `arguments[1]` das tarefas de chamada de função) e devolve o anterior ao sair; sem
//!   rastreamento, ou com o argumento vazio, não faz nada.
//! - `Bun__reportUnhandledError` (relatório de exceção de `BunPerformMicrotaskJob`) é o gancho
//!   `unhandled_error_reporter` do `VM` (`vm.set_unhandled_error_reporter`); sem gancho a exceção
//!   comum é descartada.

use crate::runtime::abstract_module_record::{AbstractModuleRecord, AbstractModuleRecordRef, ModulePhase};
use crate::runtime::call_data::call_with_error_message;
use crate::runtime::host_call::Thrown as ModuleThrown;
use crate::runtime::js_module_record::{realm, ModuleResult};
use crate::runtime::js_async_function_generator::{JSAsyncFunctionGenerator, ResumeMode, State};
use crate::runtime::js_global_object::JSGlobalObject;
use crate::runtime::js_object::JSObject;
use crate::runtime::js_promise::{is_definitely_non_thenable, JSPromise, JSPromiseRef, Status};
use crate::runtime::js_promise_capability::promise_species_constructor;
use crate::runtime::js_promise_combinators_context::JSPromiseCombinatorsGlobalContext;
use crate::runtime::js_promise_host::{get_property_named, thrown_from_llint_failure, PromiseHost, Thrown};
use crate::runtime::js_promise_reaction::JSSlimPromiseReaction;
use crate::runtime::js_value::{js_number_i32, JSValue};
use crate::runtime::microtask::{InternalMicrotask, PROMISE_REACTION_JOB_ASYNC_CONTEXT_FLAG};
use crate::runtime::promise_constructor::{
    create_aggregate_error, create_promise_all_settled_fulfilled_result, create_promise_all_settled_rejected_result,
    promise_of,
};

/// O `Result` de uma tarefa (ver o cabeçalho).
pub type MicrotaskResult = Result<(), Thrown>;

/// `callMicrotask(globalObject, functionObject, thisValue, context, message, cache, args...)` sem o
/// cache: a exceção (inclusive o `TypeError` de não ser chamável) sai como `Thrown`.
pub(crate) fn call_microtask(
    global_object: &JSGlobalObject,
    function: JSValue,
    this_value: JSValue,
    args: &[JSValue],
    message: &str,
) -> Result<JSValue, Thrown> {
    call_with_error_message(global_object, function, this_value, args, message)
        .map_err(|failure| thrown_from_llint_failure(global_object, failure))
}

/// `uncheckedDowncast<JSPromiseCombinatorsGlobalContext>(value)`.
fn global_context_of(value: &JSValue) -> crate::runtime::js_promise_combinators_context::JSPromiseCombinatorsGlobalContextRef {
    JSPromiseCombinatorsGlobalContext::from_value(value)
        .expect("uncheckedDowncast<JSPromiseCombinatorsGlobalContext> em valor que não é o contexto global")
}

/// O ramo comum de `promiseResolveWithoutHandlerJob(Slow)` e do final de `PromiseReactionJob`: resolve
/// ou rejeita `promiseOrCapability`, que é a promessa (caminho rápido) ou o objeto `{ resolve, reject }`
/// da capacidade (caminho lento, chamando a função que ele guarda).
fn settle_promise_or_capability(
    global_object: &JSGlobalObject,
    promise_or_capability: JSValue,
    value: JSValue,
    status: Status,
) -> MicrotaskResult {
    let rejected = match status {
        Status::Pending => unreachable!("RELEASE_ASSERT_NOT_REACHED: tarefa de promessa com status Pending"),
        Status::Fulfilled => false,
        Status::Rejected => true,
    };

    if let Some(promise) = JSPromise::from_value(&promise_or_capability) {
        if rejected {
            promise.reject_promise(global_object, value);
        } else {
            promise.resolve_promise(global_object, value);
        }
        return Ok(());
    }

    let vm = global_object.vm();
    let (name, message) =
        if rejected { (&vm.property_names.reject, "reject is not a function") } else { (&vm.property_names.resolve, "resolve is not a function") };
    let function = get_property_named(global_object, promise_or_capability, name)?;
    global_object.call(function, &[value], message).map(|_| ())
}

/// `promiseResolveThenableJobFastSlow` e `promiseResolveThenableJobWithInternalMicrotaskFastSlow`: o
/// `then` embutido (`SpeciesConstructor` e `NewPromiseCapability` podem lançar), onde `resolve` e `reject`
/// são as funções de resolução que cada tarefa cria antes, para que o lançamento vá para `reject(error)`.
fn promise_resolve_thenable_job_fast_slow(
    global_object: &JSGlobalObject,
    promise: &JSPromise,
    resolve: JSValue,
    reject: JSValue,
) -> MicrotaskResult {
    let capability = promise_species_constructor(global_object, promise.as_value())
        .and_then(|constructor| JSPromise::create_new_promise_capability(global_object, constructor));
    match capability {
        Ok(capability) => {
            promise.perform_promise_then(global_object, resolve, reject, capability);
            Ok(())
        }
        Err(Thrown::Termination) => Err(Thrown::Termination),
        Err(Thrown::Value(error)) => global_object.call(reject, &[error], "|reject| is not a function").map(|_| ()),
    }
}

/// `promiseResolveThenableJob(globalObject, promise, then, resolve, reject)`.
fn promise_resolve_thenable_job(
    global_object: &JSGlobalObject,
    promise: JSValue,
    then: JSValue,
    resolve: JSValue,
    reject: JSValue,
) -> MicrotaskResult {
    match call_microtask(global_object, then, promise, &[resolve, reject], "|then| is not a function") {
        Ok(_) => Ok(()),
        Err(Thrown::Termination) => Err(Thrown::Termination),
        Err(Thrown::Value(error)) => global_object.call(reject, &[error], "|reject| is not a function").map(|_| ()),
    }
}

/// `InternalMicrotask::PromiseResolveThenableJobFast`.
pub fn promise_resolve_thenable_job_fast(global_object: &JSGlobalObject, arguments: [JSValue; 4]) -> MicrotaskResult {
    let promise = promise_of(&arguments[0]);
    let promise_to_resolve = promise_of(&arguments[1]);

    if !global_object.promise_species_watchpoint_is_valid(&promise) {
        // NewPromiseResolveThenableJob step a: create resolving functions first so an abrupt completion
        // of the inlined `then` routes to reject(error) per step c.
        let (resolve, reject) = promise_to_resolve.create_resolving_functions(global_object);
        return promise_resolve_thenable_job_fast_slow(global_object, &promise, resolve, reject);
    }

    promise.perform_promise_then_with_internal_microtask(
        global_object,
        InternalMicrotask::PromiseResolveWithoutHandlerJob,
        Some(promise_to_resolve.cell_id()),
        JSValue::undefined(),
        JSValue::empty(),
    );
    Ok(())
}

/// `InternalMicrotask::PromiseResolveThenableJobWithInternalMicrotaskFast`.
pub fn promise_resolve_thenable_job_with_internal_microtask_fast(
    global_object: &JSGlobalObject,
    payload: u8,
    arguments: [JSValue; 4],
) -> MicrotaskResult {
    let promise = promise_of(&arguments[0]);
    let context = arguments[1];
    let task = InternalMicrotask::from_u8(payload).expect("payload de PromiseResolveThenableJobWithInternalMicrotaskFast fora de InternalMicrotask");

    // arguments[2] is the async context captured for `task`, if any.
    if !global_object.promise_species_watchpoint_is_valid(&promise) {
        let (resolve, reject) = JSPromise::create_resolving_functions_with_internal_microtask(global_object, task, context, arguments[2]);
        return promise_resolve_thenable_job_fast_slow(global_object, &promise, resolve, reject);
    }

    promise.perform_promise_then_with_internal_microtask(global_object, task, None, context, arguments[2]);
    Ok(())
}

/// `InternalMicrotask::PromiseResolveThenableJob`.
pub fn promise_resolve_thenable_job_task(global_object: &JSGlobalObject, arguments: [JSValue; 4]) -> MicrotaskResult {
    let promise_to_resolve = promise_of(&arguments[2]);
    let (resolve, reject) = promise_to_resolve.create_resolving_functions(global_object);
    promise_resolve_thenable_job(global_object, arguments[0], arguments[1], resolve, reject)
}

/// `InternalMicrotask::PromiseResolveThenableJobWithInternalMicrotask`.
pub fn promise_resolve_thenable_job_with_internal_microtask(
    global_object: &JSGlobalObject,
    payload: u8,
    arguments: [JSValue; 4],
) -> MicrotaskResult {
    let task = InternalMicrotask::from_u8(payload).expect("payload de PromiseResolveThenableJobWithInternalMicrotask fora de InternalMicrotask");
    // arguments[3] is the async context captured for `task`, if any.
    let (resolve, reject) = JSPromise::create_resolving_functions_with_internal_microtask(global_object, task, arguments[2], arguments[3]);
    promise_resolve_thenable_job(global_object, arguments[0], arguments[1], resolve, reject)
}

/// `InternalMicrotask::PromiseResolveWithoutHandlerJob`.
pub fn promise_resolve_without_handler_job(global_object: &JSGlobalObject, payload: u8, arguments: [JSValue; 4]) -> MicrotaskResult {
    settle_promise_or_capability(global_object, arguments[0], arguments[1], Status::from_flags(u16::from(payload)))
}

/// `InternalMicrotask::PromiseFulfillWithoutHandlerJob`.
pub fn promise_fulfill_without_handler_job(global_object: &JSGlobalObject, payload: u8, arguments: [JSValue; 4]) {
    let promise = promise_of(&arguments[0]);
    // hostLoadImportedModule may have force-settled this promise inline while a synchronous loadModule
    // was active; the queued pipeFrom job is now redundant.
    if promise.status() != Status::Pending {
        return;
    }
    match Status::from_flags(u16::from(payload)) {
        Status::Pending => unreachable!("RELEASE_ASSERT_NOT_REACHED: PromiseFulfillWithoutHandlerJob com status Pending"),
        Status::Fulfilled => promise.fulfill_promise(global_object, arguments[1]),
        Status::Rejected => promise.reject_promise(global_object, arguments[1]),
    }
}

/// `InternalMicrotask::PromiseRaceResolveJob`: `promiseRaceResolveJob`.
pub fn promise_race_resolve_job(global_object: &JSGlobalObject, payload: u8, arguments: [JSValue; 4]) {
    let promise = promise_of(&arguments[0]);
    if promise.status() != Status::Pending {
        return;
    }
    match Status::from_flags(u16::from(payload)) {
        Status::Pending => unreachable!("RELEASE_ASSERT_NOT_REACHED: PromiseRaceResolveJob com status Pending"),
        Status::Fulfilled => promise.resolve(global_object, arguments[1]),
        Status::Rejected => promise.reject(global_object, arguments[1]),
    }
}

/// Os elementos de `Promise.all`, `allSettled` e `any` terminam do mesmo jeito: decrementa a contagem e,
/// no zero, chama `finish` (`promise->resolve(values)` ou a rejeição com `AggregateError`).
fn finish_combinator_element(
    global_context: &JSPromiseCombinatorsGlobalContext,
    finish: impl FnOnce(&JSPromise, JSValue),
) {
    let count = global_context.remaining_elements_count() - 1;
    global_context.set_remaining_elements_count(count);
    if count == 0 {
        let promise = promise_of(&global_context.promise());
        finish(&promise, global_context.values());
    }
}

/// `InternalMicrotask::PromiseAllResolveJob`: `promiseAllResolveJob`.
pub fn promise_all_resolve_job(global_object: &JSGlobalObject, payload: u8, arguments: [JSValue; 4]) -> MicrotaskResult {
    let global_context = global_context_of(&arguments[0]);
    let index = arguments[2].as_number() as u64;
    match Status::from_flags(u16::from(payload)) {
        Status::Pending => unreachable!("RELEASE_ASSERT_NOT_REACHED: PromiseAllResolveJob com status Pending"),
        Status::Fulfilled => {
            global_context.put_direct_index(global_object, index, arguments[1])?;
            finish_combinator_element(&global_context, |promise, values| promise.resolve(global_object, values));
        }
        Status::Rejected => promise_of(&global_context.promise()).reject(global_object, arguments[1]),
    }
    Ok(())
}

/// `InternalMicrotask::PromiseAllSettledResolveJob`: `promiseAllSettledResolveJob`.
pub fn promise_all_settled_resolve_job(global_object: &JSGlobalObject, payload: u8, arguments: [JSValue; 4]) -> MicrotaskResult {
    let global_context = global_context_of(&arguments[0]);
    let index = arguments[2].as_number() as u64;
    let result_object = match Status::from_flags(u16::from(payload)) {
        Status::Pending => unreachable!("RELEASE_ASSERT_NOT_REACHED: PromiseAllSettledResolveJob com status Pending"),
        Status::Fulfilled => create_promise_all_settled_fulfilled_result(global_object, arguments[1]),
        Status::Rejected => create_promise_all_settled_rejected_result(global_object, arguments[1]),
    };
    global_context.put_direct_index(global_object, index, result_object)?;
    finish_combinator_element(&global_context, |promise, values| promise.resolve(global_object, values));
    Ok(())
}

/// `InternalMicrotask::PromiseAnyResolveJob`: `promiseAnyResolveJob`.
pub fn promise_any_resolve_job(global_object: &JSGlobalObject, payload: u8, arguments: [JSValue; 4]) -> MicrotaskResult {
    let global_context = global_context_of(&arguments[0]);
    let index = arguments[2].as_number() as u64;
    match Status::from_flags(u16::from(payload)) {
        Status::Pending => unreachable!("RELEASE_ASSERT_NOT_REACHED: PromiseAnyResolveJob com status Pending"),
        Status::Fulfilled => promise_of(&global_context.promise()).resolve(global_object, arguments[1]),
        Status::Rejected => {
            global_context.put_direct_index(global_object, index, arguments[1])?;
            finish_combinator_element(&global_context, |promise, errors| {
                promise.reject(global_object, create_aggregate_error(global_object, errors, None));
            });
        }
    }
    Ok(())
}

/// `AsyncContextSwapScope` (`JSMicrotask.cpp`): instala o contexto assíncrono capturado na criação da reação
/// enquanto o tratador roda e devolve o anterior ao sair. Sem rastreamento ligado, ou sem contexto capturado
/// (`empty`), não faz nada.
pub(crate) struct AsyncContextSwapScope<'a> {
    vm: &'a crate::runtime::vm::VM,
    previous: Option<JSValue>,
}

impl<'a> AsyncContextSwapScope<'a> {
    pub(crate) fn new(vm: &'a crate::runtime::vm::VM, context: JSValue) -> Self {
        if !vm.async_context_tracking_enabled() || context.is_empty() {
            return AsyncContextSwapScope { vm, previous: None };
        }
        let previous = vm.async_context();
        vm.set_async_context(context);
        AsyncContextSwapScope { vm, previous: Some(previous) }
    }
}

impl Drop for AsyncContextSwapScope<'_> {
    fn drop(&mut self) {
        if let Some(previous) = self.previous {
            self.vm.set_async_context(previous);
        }
    }
}

/// `InternalMicrotask::PromiseReactionJob`.
pub fn promise_reaction_job(global_object: &JSGlobalObject, payload: u8, arguments: [JSValue; 4]) -> MicrotaskResult {
    let promise_or_capability = arguments[0];
    let handler = arguments[1];

    // arguments[3] is the async context captured by performPromiseThen when the payload says so;
    // otherwise it is performPromiseThenWithContext's handler context (ver o cabeçalho do módulo).
    let has_async_context = payload & PROMISE_REACTION_JOB_ASYNC_CONTEXT_FLAG != 0;
    let user_context = if has_async_context { JSValue::empty() } else { arguments[3] };
    let _async_context_scope = AsyncContextSwapScope::new(global_object.vm(), if has_async_context { arguments[3] } else { JSValue::empty() });

    // When userContext is defined (not empty, undefined, or null), pass 2 arguments; otherwise 1.
    let outcome = if user_context.is_empty() || user_context.is_undefined_or_null() {
        call_microtask(global_object, handler, JSValue::undefined(), &[arguments[2]], "handler is not a function")
    } else {
        call_microtask(global_object, handler, JSValue::undefined(), &[arguments[2], user_context], "handler is not a function")
    };

    if promise_or_capability.is_undefined_or_null() {
        // Sem promessa para liquidar, a exceção do tratador fica pendente (o laço a descarta).
        return outcome.map(|_| ());
    }

    match outcome {
        Err(Thrown::Termination) => Err(Thrown::Termination),
        Err(Thrown::Value(error)) => settle_promise_or_capability(global_object, promise_or_capability, error, Status::Rejected),
        Ok(result) => settle_promise_or_capability(global_object, promise_or_capability, result, Status::Fulfilled),
    }
}

/// `promiseFinallyAwaitJob`.
fn promise_finally_await_job_body(
    global_object: &JSGlobalObject,
    settled_value: JSValue,
    context: &JSSlimPromiseReaction,
    status: Status,
) {
    let result_promise = promise_of(&context.promise());
    let original_value = context.handler_or_context();
    let was_fulfilled = context.is_fulfill_handler();

    if status == Status::Rejected {
        result_promise.reject_promise(global_object, settled_value);
        return;
    }

    if was_fulfilled {
        result_promise.resolve_promise(global_object, original_value);
    } else {
        result_promise.reject_promise(global_object, original_value);
    }
}

/// `InternalMicrotask::PromiseFinallyAwaitJob`: o contexto (`arguments[2]`) é a `JSSlimPromiseReaction`.
pub fn promise_finally_await_job(global_object: &JSGlobalObject, payload: u8, arguments: [JSValue; 4]) {
    let context = slim_reaction_of(&arguments[2]);
    promise_finally_await_job_body(global_object, arguments[1], &context, Status::from_flags(u16::from(payload)));
}

/// `uncheckedDowncast<JSSlimPromiseReaction>(value)`.
fn slim_reaction_of(value: &JSValue) -> crate::runtime::js_promise_reaction::JSSlimPromiseReactionRef {
    match crate::runtime::js_promise_reaction::JSPromiseReactionRef::from_value(value) {
        Some(crate::runtime::js_promise_reaction::JSPromiseReactionRef::Slim(slim)) => slim,
        _ => panic!("uncheckedDowncast<JSSlimPromiseReaction> em valor que não é reação slim"),
    }
}

/// `InternalMicrotask::PromiseFinallyReactionJob` (fase 1: a promessa original liquidou).
pub fn promise_finally_reaction_job(global_object: &JSGlobalObject, payload: u8, arguments: [JSValue; 4]) -> MicrotaskResult {
    let vm = global_object.vm();
    let context = slim_reaction_of(&arguments[2]);
    let result_promise = promise_of(&context.promise());
    let value_or_reason = arguments[1];
    let status = Status::from_flags(u16::from(payload));

    let on_finally = context.handler_or_context();
    let result = match call_microtask(global_object, on_finally, JSValue::undefined(), &[], "onFinally is not a function") {
        Ok(result) => result,
        Err(Thrown::Termination) => return Err(Thrown::Termination),
        Err(Thrown::Value(error)) => {
            result_promise.reject_promise(global_object, error);
            return Ok(());
        }
    };

    context.set_handler_or_context(value_or_reason);
    context.set_per_cell_bit(status == Status::Fulfilled);

    if let Some(promise) = JSPromise::from_value(&result) {
        if global_object.owns_promise(&promise) && promise.is_then_fast_and_non_observable(global_object) {
            promise.perform_promise_then_with_internal_microtask(
                global_object,
                InternalMicrotask::PromiseFinallyAwaitJob,
                Some(result_promise.cell_id()),
                context.as_value(),
                JSValue::empty(),
            );
            return Ok(());
        }
    }

    let Some(resolution_object) = JSObject::from_value(&result) else {
        promise_finally_await_job_body(global_object, result, &context, Status::Fulfilled);
        return Ok(());
    };
    if is_definitely_non_thenable(&resolution_object, global_object) {
        promise_finally_await_job_body(global_object, result, &context, Status::Fulfilled);
        return Ok(());
    }

    let then = match get_property_named(global_object, result, &vm.property_names.then) {
        Ok(then) => then,
        Err(Thrown::Termination) => return Err(Thrown::Termination),
        Err(Thrown::Value(error)) => {
            promise_finally_await_job_body(global_object, error, &context, Status::Rejected);
            return Ok(());
        }
    };

    if !then.is_callable() {
        promise_finally_await_job_body(global_object, result, &context, Status::Fulfilled);
        return Ok(());
    }

    let (resolve, reject) = JSPromise::create_resolving_functions_with_internal_microtask(
        global_object,
        InternalMicrotask::PromiseFinallyAwaitJob,
        context.as_value(),
        JSValue::empty(),
    );
    promise_resolve_thenable_job(global_object, result, then, resolve, reject)
}

/// `InternalMicrotask::InvokeFunctionJob`: `arguments[1]` é o contexto assíncrono capturado no agendamento (o
/// `queueMicrotask`), vazio quando não há.
pub fn invoke_function_job(global_object: &JSGlobalObject, arguments: [JSValue; 4]) -> MicrotaskResult {
    let _async_context_scope = AsyncContextSwapScope::new(global_object.vm(), arguments[1]);
    call_microtask(global_object, arguments[0], JSValue::undefined(), &[], "handler is not a function").map(|_| ())
}

/// `InternalMicrotask::BunPerformMicrotaskJob`: `arguments[0]` é a função, `arguments[1]` o contexto assíncrono e
/// `arguments[2]` e `arguments[3]` os argumentos opcionais.
pub fn bun_perform_microtask_job(global_object: &JSGlobalObject, arguments: [JSValue; 4]) -> MicrotaskResult {
    let job = arguments[0];
    if job.is_empty() || job.is_undefined_or_null() || !job.is_callable() {
        return Ok(());
    }
    let _async_context_scope = AsyncContextSwapScope::new(global_object.vm(), arguments[1]);

    let extra: &[JSValue] = if !arguments[3].is_empty() {
        &arguments[2..4]
    } else if !arguments[2].is_empty() {
        &arguments[2..3]
    } else {
        &[]
    };
    match call_microtask(global_object, job, JSValue::undefined(), extra, "performMicrotask is not a function") {
        // A TerminationException is not an error to report: it stays pending so the checkpoint stops.
        Err(Thrown::Termination) => Err(Thrown::Termination),
        Err(Thrown::Value(error)) => {
            global_object.vm().report_unhandled_error(global_object, error);
            Ok(())
        }
        Ok(_) => Ok(()),
    }
}

/// `JSGenerator::ResumeMode` de `resumeModeForStatus(status)`.
fn resume_mode_for_status(status: Status) -> ResumeMode {
    assert!(status != Status::Pending, "RELEASE_ASSERT: resumeModeForStatus com Status::Pending");
    if status == Status::Rejected { ResumeMode::ThrowMode } else { ResumeMode::NormalMode }
}

/// `uncheckedDowncast<JSAsyncFunctionGenerator>(value)`.
fn async_function_generator_of(value: &JSValue) -> crate::runtime::js_async_function_generator::JSAsyncFunctionGeneratorRef {
    JSAsyncFunctionGenerator::from_value(value)
        .expect("uncheckedDowncast<JSAsyncFunctionGenerator> em valor que não é o gerador de async function")
}

/// `generator->realm()`: o `JSGlobalObject` da `Structure` do gerador.
fn generator_realm(generator: &JSAsyncFunctionGenerator) -> crate::runtime::js_global_object::JSGlobalObjectRef {
    generator.structure().realm().expect("o gerador de async function tem a Structure de um realm")
}

/// `asyncFunctionArrangeAwaitResume(globalObject, vm, generator, value)`: o `await` do corpo suspenso
/// (o `value` que ele devolveu) retoma o gerador por `AsyncFunctionResume`, a menos que seja o
/// `vm.fastAsyncGeneratorSentinel()` (a retomada já foi armada no produtor). Também é o corpo de
/// `asyncFunctionDrive`.
pub fn async_function_arrange_await_resume(global_object: &JSGlobalObject, generator: &JSAsyncFunctionGenerator, value: JSValue) {
    if value == global_object.vm().fast_async_generator_sentinel() {
        return;
    }
    JSPromise::resolve_with_internal_microtask_for_async_await(
        global_object,
        value,
        InternalMicrotask::AsyncFunctionResume,
        generator.as_value(),
    );
}

/// `asyncFunctionGeneratorBodyCall(generatorGlobalObject, vm, generator, resolution, resumeMode, cache)`:
/// um passo da continuação de uma async function suspensa. A exceção do corpo rejeita a promessa da
/// função (`generator->context()`); o retorno normal a resolve; um corpo que suspendeu de novo
/// (`state` ainda `Executing` é o retorno, qualquer outro é um `await`) arma a retomada.
fn async_function_generator_body_call(
    generator_global_object: &JSGlobalObject,
    generator: &JSAsyncFunctionGenerator,
    resolution: JSValue,
    resume_mode: ResumeMode,
) -> MicrotaskResult {
    let state = generator.state();
    generator.set_state(State::Executing as i32);
    let next = generator.next();
    let this_value = generator.this_value();
    let frame = generator.frame();
    let arguments = [generator.as_value(), js_number_i32(state), resolution, js_number_i32(resume_mode as i32), frame];
    match call_microtask(generator_global_object, next, this_value, &arguments, "handler is not a function") {
        // A TerminationException stays pending so the checkpoint stops.
        Err(Thrown::Termination) => Err(Thrown::Termination),
        Err(Thrown::Value(error)) => {
            promise_of(&generator.context()).reject(generator_global_object, error);
            Ok(())
        }
        Ok(value) => {
            if generator.state() == State::Executing as i32 {
                promise_of(&generator.context()).resolve(generator_global_object, value);
            } else {
                async_function_arrange_await_resume(generator_global_object, generator, value);
            }
            Ok(())
        }
    }
}

/// `InternalMicrotask::AsyncFunctionResume`: `arguments[1]` é a resolução do `await`, `arguments[2]` o
/// gerador, `arguments[3]` o contexto assíncrono do `await` e o payload o `JSPromise::Status` dela.
pub fn async_function_resume(payload: u8, arguments: [JSValue; 4]) -> MicrotaskResult {
    let generator = async_function_generator_of(&arguments[2]);
    let generator_global_object = generator_realm(&generator);
    let resume_mode = resume_mode_for_status(Status::from_flags(u16::from(payload)));
    let _async_context_scope = AsyncContextSwapScope::new(generator_global_object.vm(), arguments[3]);
    // `VMEntryRecord::m_context` do C++: de onde `getStackTrace` tira os frames `async`.
    let _origin = crate::interpreter::unwind::enter_async_origin(&generator);
    async_function_generator_body_call(&generator_global_object, &generator, arguments[1], resume_mode)
}

/// `InternalMicrotask::AsyncGeneratorDriverResume` / `asyncGeneratorDriverResume`: o `for await` de uma
/// async function ou de um async generator retomado direto pelo produtor. O outro tipo de driver
/// (`JSModuleRecord` de top-level await) é a LACUNA do cabeçalho.
pub fn async_generator_driver_resume(payload: u8, arguments: [JSValue; 4]) -> MicrotaskResult {
    let resume_mode = resume_mode_for_status(Status::from_flags(u16::from(payload)));
    if let Some(generator) = JSAsyncFunctionGenerator::from_value(&arguments[2]) {
        let generator_global_object = generator_realm(&generator);
        let _async_context_scope = AsyncContextSwapScope::new(generator_global_object.vm(), arguments[3]);
        let _origin = crate::interpreter::unwind::enter_async_origin(&generator);
        return async_function_generator_body_call(&generator_global_object, &generator, arguments[1], resume_mode);
    }
    match crate::runtime::js_async_generator::JSAsyncGenerator::from_value(&arguments[2]) {
        Some(generator) => {
            crate::runtime::js_microtask_async::async_generator_driver_resume_generator(&generator, arguments[1], resume_mode);
            Ok(())
        }
        None => {
            // The only remaining for-await driver kind is a top-level-await module.
            let module = async_module_of(&arguments[2]);
            async_module_execution_resume_record(&module, arguments[1], Status::from_flags(u16::from(payload)))
        }
    }
}

/// `uncheckedDowncast<JSModuleRecord>(value)`.
fn async_module_of(value: &JSValue) -> AbstractModuleRecordRef {
    let JSValue::Cell(cell_id) = value else {
        panic!("uncheckedDowncast<JSModuleRecord> em valor que não é uma célula");
    };
    AbstractModuleRecord::from_cell_id(*cell_id).expect("uncheckedDowncast<JSModuleRecord> em célula que não é um registro de módulo")
}

/// `vm.exception()->value()` seguido de `clearException`: o valor da exceção que o corpo deixou pendente
/// (o `TRY_CLEAR_EXCEPTION` do C++).
pub(crate) fn take_pending_exception(global_object: &JSGlobalObject) -> JSValue {
    let vm = global_object.vm();
    let exception = vm.exception().expect("Err(Pending) sem exceção pendente");
    let value = exception.value();
    vm.clear_exception();
    value
}

/// `asyncModuleResolveEvaluation(globalObject, vm, scope, module, result)`: o fim de um passo do corpo do
/// módulo com top-level await. A exceção pendente (`Err(Pending)`) rejeita a capacidade; o sentinela do
/// `for await` rápido não arma nada; o corpo terminado resolve a capacidade; o suspenso num `await` arma
/// a retomada por `AsyncModuleExecutionResume`. A lacuna do porte sobe.
pub fn async_module_resolve_evaluation(
    global_object: &JSGlobalObject,
    module: &AbstractModuleRecordRef,
    result: ModuleResult<JSValue>,
) -> ModuleResult<()> {
    let capability = module.async_capability().expect("módulo com top-level await sem asyncCapability");

    let result = match result {
        Ok(result) => result,
        Err(ModuleThrown::Pending) => {
            let error = take_pending_exception(global_object);
            capability.reject(global_object, error);
            return Ok(());
        }
        Err(other) => return Err(other),
    };

    if result == global_object.vm().fast_async_generator_sentinel() {
        // The module suspended cooperatively driving an async generator, which already holds this module
        // in its queue as the driver. Do not schedule our own resume.
        return Ok(());
    }

    if module.is_top_level_execution_finished() {
        capability.resolve(global_object, result);
    } else {
        JSPromise::resolve_with_internal_microtask_for_async_await(
            global_object,
            result,
            InternalMicrotask::AsyncModuleExecutionResume,
            module.as_value(),
        );
    }
    Ok(())
}

/// `asyncModuleExecutionResume(globalObject, vm, module, resolution, status)`.
fn async_module_execution_resume_record(module: &AbstractModuleRecordRef, resolution: JSValue, status: Status) -> MicrotaskResult {
    let global_object = realm();
    let resume_mode = js_number_i32(resume_mode_for_status(status) as i32);
    let result = module.evaluate_body(&global_object, resolution, resume_mode);
    module_step_outcome(async_module_resolve_evaluation(&global_object, module, result))
}

/// O desfecho de uma tarefa de módulo: a lacuna do porte é `panic!` (como o resto do despacho). Os
/// passos de módulo não deixam exceção pendente.
fn module_step_outcome(outcome: ModuleResult<()>) -> MicrotaskResult {
    match outcome {
        Ok(()) => Ok(()),
        Err(ModuleThrown::Unported(what)) => panic!("módulos: {what} ainda não portado"),
        Err(_) => Ok(()),
    }
}

/// `InternalMicrotask::AsyncModuleExecutionDone`: `arguments[1]` é o valor da promessa do módulo,
/// `arguments[2]` o módulo e o payload o `JSPromise::Status` dela (`asyncModuleExecutionDone`).
pub fn async_module_execution_done(payload: u8, arguments: [JSValue; 4]) -> MicrotaskResult {
    let module = async_module_of(&arguments[2]);
    if Status::from_flags(u16::from(payload)) == Status::Fulfilled {
        return module_step_outcome(module.async_execution_fulfilled());
    }
    debug_assert!(Status::from_flags(u16::from(payload)) == Status::Rejected);
    module.async_execution_rejected(arguments[1]);
    Ok(())
}

/// `InternalMicrotask::AsyncModuleExecutionResume`: `arguments[1]` é a resolução do `await` de topo,
/// `arguments[2]` o módulo (ver a DIVERGÊNCIA de `AsyncContextSwapScope`).
pub fn async_module_execution_resume(payload: u8, arguments: [JSValue; 4]) -> MicrotaskResult {
    let module = async_module_of(&arguments[2]);
    async_module_execution_resume_record(&module, arguments[1], Status::from_flags(u16::from(payload)))
}

/// `uncheckedDowncast<JSPromise>(value)`.
fn promise_from_value(value: &JSValue) -> JSPromiseRef {
    JSPromise::from_value(value).expect("uncheckedDowncast<JSPromise> em valor que não é uma promessa")
}

/// Liquida `capability` com o namespace de `module` na fase `phase`, ou rejeita com o erro de
/// `getModuleNamespace` (o `fulfill` no lugar do `resolve`: ver `dynamicImportEvaluateSettled`).
fn settle_with_module_namespace(global_object: &JSGlobalObject, capability: &JSPromise, module: &AbstractModuleRecordRef, phase: ModulePhase) {
    match module.get_module_namespace(global_object, phase, true) {
        Ok(namespace) => capability.fulfill(global_object, namespace.as_value()),
        Err(ModuleThrown::Pending) => capability.reject(global_object, take_pending_exception(global_object)),
        Err(other) => module_step_outcome(Err(other)).expect("module_step_outcome só devolve Ok"),
    }
}

/// `InternalMicrotask::DynamicImportEvaluateSettled` (`dynamicImportEvaluateSettled`): `arguments[0]` é a
/// promessa interna do `import()`, `arguments[1]` a resolução ou o erro da avaliação e `arguments[2]` o
/// módulo.
pub fn dynamic_import_evaluate_settled(global_object: &JSGlobalObject, payload: u8, arguments: [JSValue; 4]) -> MicrotaskResult {
    // https://tc39.es/ecma262/#sec-ContinueDynamicImport
    let capability = promise_from_value(&arguments[0]);
    let module = async_module_of(&arguments[2]);
    if Status::from_flags(u16::from(payload)) == Status::Fulfilled {
        // 6.d.i. Let namespace be GetModuleNamespace(module).
        // 6.d.ii. The resolve happens on the user-visible promise (`importModuleNamespace`).
        settle_with_module_namespace(global_object, &capability, &module, ModulePhase::Evaluation);
        return Ok(());
    }
    capability.reject(global_object, arguments[1]);
    Ok(())
}

/// `resolveDeferredImportNamespace(globalObject, vm, scope, capabilityPromise, module)`: o fim do
/// `ContinueDynamicImport` com `phase = defer` (https://tc39.es/proposal-defer-import-eval/#sec-ContinueDynamicImport).
pub fn resolve_deferred_import_namespace(global_object: &JSGlobalObject, capability: &JSPromise, module: &AbstractModuleRecordRef) {
    settle_with_module_namespace(global_object, capability, module, ModulePhase::Defer);
}

/// `InternalMicrotask::DynamicImportDeferDependencySettled` (`dynamicImportDeferDependencySettled`): o
/// AND-join do `SafePerformPromiseAll` do `ContinueDynamicImport` adiado. `arguments[0]` é a promessa
/// interna do `import()`, `arguments[1]` a resolução ou o erro e `arguments[2]` o contexto do join (o
/// `m_values` é o módulo e o `m_remainingElementsCount` a contagem).
pub fn dynamic_import_defer_dependency_settled(global_object: &JSGlobalObject, payload: u8, arguments: [JSValue; 4]) -> MicrotaskResult {
    let capability = promise_from_value(&arguments[0]);
    let join_context = JSPromiseCombinatorsGlobalContext::from_value(&arguments[2])
        .expect("uncheckedDowncast<JSPromiseCombinatorsGlobalContext> em valor que não é o contexto do join");
    if Status::from_flags(u16::from(payload)) != Status::Fulfilled {
        // First rejection wins; reject() on a settled promise is a no-op.
        capability.reject(global_object, arguments[1]);
        return Ok(());
    }
    let count = join_context.remaining_elements_count();
    debug_assert!(count > 0);
    let remaining = count - 1;
    join_context.set_remaining_elements_count(remaining);
    if remaining != 0 {
        return Ok(());
    }
    let module = async_module_of(&join_context.values());
    resolve_deferred_import_namespace(global_object, &capability, &module);
    Ok(())
}

/// Passos do carregador entre o fim da avaliação e a resolução da promessa do `import()` (ver
/// `import_module_namespace`).
const IMPORT_NAMESPACE_EXTRA_HOPS: i32 = 4;

/// `InternalMicrotask::ImportModuleNamespace` (`importModuleNamespace`): `arguments[0]` é a promessa que o
/// `import()` devolveu ao usuário e `arguments[1]` o namespace (ou o erro) que o `ContinueDynamicImport`
/// entregou.
pub fn import_module_namespace(global_object: &JSGlobalObject, payload: u8, arguments: [JSValue; 4]) -> MicrotaskResult {
    let result_promise = promise_from_value(&arguments[0]);
    let fulfilled = Status::from_flags(u16::from(payload)) == Status::Fulfilled;
    // Medido no bun 1.4.2: entre o fim do corpo do módulo e o callback do `.then` do `import()` passam 7
    // microtasks; `dynamicImportEvaluateSettled`, esta e o `.then` do usuário são 3, as 4 que faltam
    // são os passos do carregador (`arguments[2]` conta os que restam; ausente, começa em 4).
    let remaining = if arguments[2].is_int32() { arguments[2].as_int32() } else { IMPORT_NAMESPACE_EXTRA_HOPS };
    if remaining > 0 {
        let relay = JSPromise::create(global_object.vm(), &global_object.promise_structure());
        if fulfilled {
            relay.fulfill(global_object, arguments[1]);
        } else {
            relay.reject(global_object, arguments[1]);
        }
        relay.perform_promise_then_with_internal_microtask(
            global_object,
            InternalMicrotask::ImportModuleNamespace,
            Some(result_promise.cell_id()),
            js_number_i32(remaining - 1),
            JSValue::empty(),
        );
        return Ok(());
    }
    if fulfilled {
        // ContinueDynamicImport step 6.d.ii: Call(promiseCapability.[[Resolve]], undefined, « namespace »).
        // A module namespace that exports "then" is a thenable per spec.
        result_promise.resolve(global_object, arguments[1]);
    } else {
        result_promise.reject(global_object, arguments[1]);
    }
    Ok(())
}

/// `InternalMicrotask::BunInvokeJobWithArguments`: `arguments[0]` é a função e as demais, as não vazias,
/// os argumentos.
pub fn bun_invoke_job_with_arguments(global_object: &JSGlobalObject, arguments: [JSValue; 4]) -> MicrotaskResult {
    let args: Vec<JSValue> = arguments[1..].iter().copied().filter(|argument| !argument.is_empty()).collect();
    call_microtask(global_object, arguments[0], JSValue::undefined(), &args, "job is not a function").map(|_| ())
}
