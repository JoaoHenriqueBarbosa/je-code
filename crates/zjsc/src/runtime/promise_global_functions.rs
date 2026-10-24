//! As funções nativas de `Promise` que `JSGlobalObject.cpp` define (`resolvePromise`, `rejectPromise`,
//! `fulfillPromise`, `markPromiseAsHandledHostFunction`, `isPromiseStatePending`,
//! `...WithFirstResolvingFunctionCallCheck`, `newResolvedPromise`, `newRejectedPromise`,
//! `resolveWithInternalMicrotaskForAsyncAwait`, `newHandledRejectedPromise`,
//! `promiseReturnUndefinedOnFulfilled`, `promiseResolve`, `promiseReject`, `promiseResolveWithThen`,
//! `performPromiseThen`, `enqueueJob` e `asyncFunctionDrive`) e o `m_linkTimeConstants[...].initLater` de cada uma
//! (`install_promise_link_time_constants`, chamado por `JSGlobalObject::init_promise`). São as
//! `@resolvePromise` e companhia dos builtins de JavaScript (`PromiseOperations.js`).
//!
//! DIVERGÊNCIA: o `LazyProperty` de cada `LinkTimeConstant` é a função criada na hora, junto do global.

use crate::bytecode::bytecode_intrinsics_table::LinkTimeConstant;
use crate::host_function;
use crate::runtime::host_call::{HostCall, HostResult};
use crate::runtime::host_function_support::ObjectRef;
use crate::runtime::implementation_visibility::ImplementationVisibility;
use crate::runtime::intrinsic::Intrinsic;
use crate::runtime::js_async_function_generator::JSAsyncFunctionGenerator;
use crate::runtime::js_function::{call_host_function_as_constructor, JSFunction};
use crate::runtime::js_global_object::JSGlobalObject;
use crate::runtime::js_microtask;
use crate::runtime::js_promise::{JSPromise, JSPromiseRef, Status};
use crate::runtime::js_promise_host::{host_result, PromiseHost};
use crate::runtime::js_value::{js_boolean, JSValue};
use crate::runtime::microtask::InternalMicrotask;
use crate::runtime::microtask_queue::QueuedTask;
use crate::runtime::native_function::NativeFunction;
use crate::runtime::property_attribute::{DONT_DELETE, DONT_ENUM, READ_ONLY};
use crate::runtime::property_name::PropertyName;
use crate::wtf::text::wtf_string::String as WtfString;

/// `uncheckedDowncast<JSPromise>(callFrame->uncheckedArgument(0))`.
fn promise_argument(call: &HostCall) -> JSPromiseRef {
    JSPromise::from_value(&call.argument(0)).expect("uncheckedDowncast<JSPromise> em argumento que não é promessa")
}

/// `resolvePromise`.
fn resolve_promise(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    promise_argument(call).resolve_promise(global_object, call.argument(1));
    Ok(JSValue::undefined())
}

/// `rejectPromise`.
fn reject_promise(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    promise_argument(call).reject_promise(global_object, call.argument(1));
    Ok(JSValue::undefined())
}

/// `fulfillPromise`.
fn fulfill_promise(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    promise_argument(call).fulfill_promise(global_object, call.argument(1));
    Ok(JSValue::undefined())
}

/// `markPromiseAsHandledHostFunction`.
fn mark_promise_as_handled(_global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    promise_argument(call).mark_as_handled();
    Ok(JSValue::undefined())
}

/// `isPromiseStatePending`.
fn is_promise_state_pending(_global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    Ok(js_boolean(promise_argument(call).status() == Status::Pending))
}

/// `resolvePromiseWithFirstResolvingFunctionCallCheck`.
fn resolve_promise_with_first_resolving_function_call_check(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    promise_argument(call).resolve(global_object, call.argument(1));
    Ok(JSValue::undefined())
}

/// `rejectPromiseWithFirstResolvingFunctionCallCheck`.
fn reject_promise_with_first_resolving_function_call_check(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    promise_argument(call).reject(global_object, call.argument(1));
    Ok(JSValue::undefined())
}

/// `fulfillPromiseWithFirstResolvingFunctionCallCheck`.
fn fulfill_promise_with_first_resolving_function_call_check(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    promise_argument(call).fulfill(global_object, call.argument(1));
    Ok(JSValue::undefined())
}

/// `newResolvedPromise`.
fn new_resolved_promise(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    let promise = JSPromise::create(global_object.vm(), &global_object.promise_structure());
    promise.resolve(global_object, call.argument(0));
    Ok(promise.as_value())
}

/// `newRejectedPromise`.
fn new_rejected_promise(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    Ok(JSPromise::rejected_promise(global_object, call.argument(0)).as_value())
}

/// `resolveWithInternalMicrotaskForAsyncAwait`.
fn resolve_with_internal_microtask_for_async_await(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    let task = InternalMicrotask::from_u8(call.argument(1).as_number() as u8)
        .expect("resolveWithInternalMicrotaskForAsyncAwait com tarefa fora de InternalMicrotask");
    JSPromise::resolve_with_internal_microtask_for_async_await(global_object, call.argument(0), task, call.argument(2));
    Ok(JSValue::undefined())
}

/// `newHandledRejectedPromise`.
fn new_handled_rejected_promise(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    let promise = JSPromise::rejected_promise(global_object, call.argument(0));
    promise.mark_as_handled();
    Ok(promise.as_value())
}

/// `promiseReturnUndefinedOnFulfilled`.
fn promise_return_undefined_on_fulfilled(_global_object: &JSGlobalObject, _call: &HostCall) -> HostResult {
    Ok(JSValue::undefined())
}

/// `promiseResolve(constructor, argument)`.
fn promise_resolve(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    host_result(global_object, JSPromise::promise_resolve(global_object, call.argument(0), call.argument(1)))
}

/// `promiseReject(constructor, argument)`.
fn promise_reject(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    host_result(global_object, JSPromise::promise_reject(global_object, call.argument(0), call.argument(1)))
}

/// `promiseResolveWithThen(constructor, argument)`: `promiseResolve` e o `@then` próprio na promessa
/// devolvida (os builtins do Bun chamam `promise.@then()`).
fn promise_resolve_with_then(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    let vm = global_object.vm();
    let promise = host_result(global_object, JSPromise::promise_resolve(global_object, call.argument(0), call.argument(1)))?;

    // Set @then property on the promise if it doesn't already have one.
    let then_private_name = PropertyName::from_identifier(&vm.property_names.builtin_names().then_private_name());
    if let Some(object) = ObjectRef::from_value(&promise) {
        if !object.has_own_property(global_object, &then_private_name) {
            object.put_direct(vm, &then_private_name, global_object.promise_proto_then_function(), DONT_ENUM | DONT_DELETE | READ_ONLY);
        }
    }
    Ok(promise)
}

/// `performPromiseThen(promise, onFulfilled, onRejected, promiseOrCapability[, context])`.
fn perform_promise_then(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    let promise = promise_argument(call);
    if call.argument_count() > 4 {
        promise.perform_promise_then_with_context(global_object, call.argument(1), call.argument(2), call.argument(3), call.argument(4));
    } else {
        promise.perform_promise_then(global_object, call.argument(1), call.argument(2), call.argument(3));
    }
    Ok(JSValue::undefined())
}

/// `enqueueJob(job, argument0, argument1, argument2)` (`USE(BUN_JSC_ADDITIONS)`).
fn enqueue_job(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    // maxMicrotaskArguments=4: job + 3 user arguments.
    let arguments = [call.argument(0), call.argument(1), call.argument(2), call.argument(3)];
    global_object
        .vm()
        .default_microtask_queue
        .enqueue(QueuedTask::new(global_object.cell_id(), InternalMicrotask::BunInvokeJobWithArguments, 0, &arguments));
    Ok(JSValue::undefined())
}

/// `asyncFunctionDrive(resolution, generator)` (`JSMicrotask.cpp`): o primeiro `await` de uma async
/// function arma a retomada do gerador (`@asyncFunctionDrive` de `AsyncFunctionPrototype.js`).
fn async_function_drive(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    let generator = JSAsyncFunctionGenerator::from_value(&call.argument(1))
        .expect("uncheckedDowncast<JSAsyncFunctionGenerator> em argumento que não é o gerador de async function");
    js_microtask::async_function_arrange_await_resume(global_object, &generator, call.argument(0));
    Ok(JSValue::undefined())
}

host_function!(resolve_promise_host, resolve_promise);
host_function!(reject_promise_host, reject_promise);
host_function!(fulfill_promise_host, fulfill_promise);
host_function!(mark_promise_as_handled_host, mark_promise_as_handled);
host_function!(is_promise_state_pending_host, is_promise_state_pending);
host_function!(
    resolve_promise_with_first_resolving_function_call_check_host,
    resolve_promise_with_first_resolving_function_call_check
);
host_function!(
    reject_promise_with_first_resolving_function_call_check_host,
    reject_promise_with_first_resolving_function_call_check
);
host_function!(
    fulfill_promise_with_first_resolving_function_call_check_host,
    fulfill_promise_with_first_resolving_function_call_check
);
host_function!(new_resolved_promise_host, new_resolved_promise);
host_function!(new_rejected_promise_host, new_rejected_promise);
host_function!(resolve_with_internal_microtask_for_async_await_host, resolve_with_internal_microtask_for_async_await);
host_function!(new_handled_rejected_promise_host, new_handled_rejected_promise);
host_function!(promise_return_undefined_on_fulfilled_host, promise_return_undefined_on_fulfilled);
host_function!(promise_resolve_host, promise_resolve);
host_function!(promise_reject_host, promise_reject);
host_function!(promise_resolve_with_then_host, promise_resolve_with_then);
host_function!(perform_promise_then_host, perform_promise_then);
host_function!(enqueue_job_host, enqueue_job);
host_function!(async_function_drive_host, async_function_drive);

/// Uma linha de `m_linkTimeConstants[LinkTimeConstant::x].initLater(... JSFunction::create(vm, owner,
/// length, name, function, visibility[, intrinsic]) ...)`.
struct LinkTimeFunction {
    constant: LinkTimeConstant,
    length: u32,
    name: &'static str,
    function: NativeFunction,
    visibility: ImplementationVisibility,
    intrinsic: Intrinsic,
}

/// As `LinkTimeConstant` de função de `Promise` (JSGlobalObject.cpp:2021-2076), na ordem do C++.
pub fn install_promise_link_time_constants(global_object: &JSGlobalObject) {
    use ImplementationVisibility::{Private, Public};
    let no_intrinsic = Intrinsic::NoIntrinsic;
    let table = [
        LinkTimeFunction { constant: LinkTimeConstant::EnqueueJob, length: 0, name: "enqueueJob", function: enqueue_job_host, visibility: Public, intrinsic: no_intrinsic },
        LinkTimeFunction { constant: LinkTimeConstant::ResolvePromise, length: 2, name: "resolvePromise", function: resolve_promise_host, visibility: Private, intrinsic: no_intrinsic },
        LinkTimeFunction { constant: LinkTimeConstant::RejectPromise, length: 2, name: "rejectPromise", function: reject_promise_host, visibility: Private, intrinsic: no_intrinsic },
        LinkTimeFunction { constant: LinkTimeConstant::FulfillPromise, length: 2, name: "fulfillPromise", function: fulfill_promise_host, visibility: Private, intrinsic: no_intrinsic },
        LinkTimeFunction { constant: LinkTimeConstant::MarkPromiseAsHandled, length: 1, name: "markPromiseAsHandled", function: mark_promise_as_handled_host, visibility: Private, intrinsic: no_intrinsic },
        LinkTimeFunction { constant: LinkTimeConstant::IsPromiseStatePending, length: 1, name: "isPromiseStatePending", function: is_promise_state_pending_host, visibility: Private, intrinsic: no_intrinsic },
        LinkTimeFunction {
            constant: LinkTimeConstant::ResolvePromiseWithFirstResolvingFunctionCallCheck,
            length: 2,
            name: "resolvePromiseWithFirstResolvingFunctionCallCheck",
            function: resolve_promise_with_first_resolving_function_call_check_host,
            visibility: Private,
            intrinsic: Intrinsic::ResolvePromiseWithFirstResolvingFunctionCallCheckIntrinsic,
        },
        LinkTimeFunction {
            constant: LinkTimeConstant::RejectPromiseWithFirstResolvingFunctionCallCheck,
            length: 2,
            name: "rejectPromiseWithFirstResolvingFunctionCallCheck",
            function: reject_promise_with_first_resolving_function_call_check_host,
            visibility: Private,
            intrinsic: Intrinsic::RejectPromiseWithFirstResolvingFunctionCallCheckIntrinsic,
        },
        LinkTimeFunction {
            constant: LinkTimeConstant::FulfillPromiseWithFirstResolvingFunctionCallCheck,
            length: 2,
            name: "fulfillPromiseWithFirstResolvingFunctionCallCheck",
            function: fulfill_promise_with_first_resolving_function_call_check_host,
            visibility: Private,
            intrinsic: Intrinsic::FulfillPromiseWithFirstResolvingFunctionCallCheckIntrinsic,
        },
        LinkTimeFunction { constant: LinkTimeConstant::NewResolvedPromise, length: 1, name: "newResolvedPromise", function: new_resolved_promise_host, visibility: Private, intrinsic: Intrinsic::NewResolvedPromiseIntrinsic },
        LinkTimeFunction { constant: LinkTimeConstant::NewRejectedPromise, length: 1, name: "newRejectedPromise", function: new_rejected_promise_host, visibility: Private, intrinsic: Intrinsic::NewRejectedPromiseIntrinsic },
        LinkTimeFunction {
            constant: LinkTimeConstant::ResolveWithInternalMicrotaskForAsyncAwait,
            length: 3,
            name: "resolveWithInternalMicrotaskForAsyncAwait",
            function: resolve_with_internal_microtask_for_async_await_host,
            visibility: Private,
            intrinsic: no_intrinsic,
        },
        LinkTimeFunction { constant: LinkTimeConstant::NewHandledRejectedPromise, length: 1, name: "newHandledRejectedPromise", function: new_handled_rejected_promise_host, visibility: Private, intrinsic: no_intrinsic },
        LinkTimeFunction { constant: LinkTimeConstant::PromiseReturnUndefinedOnFulfilled, length: 1, name: "promiseReturnUndefinedOnFulfilled", function: promise_return_undefined_on_fulfilled_host, visibility: Private, intrinsic: no_intrinsic },
        LinkTimeFunction { constant: LinkTimeConstant::PromiseResolve, length: 2, name: "promiseResolve", function: promise_resolve_host, visibility: Private, intrinsic: Intrinsic::PromiseResolveIntrinsic },
        LinkTimeFunction { constant: LinkTimeConstant::PromiseReject, length: 2, name: "promiseReject", function: promise_reject_host, visibility: Private, intrinsic: Intrinsic::PromiseRejectIntrinsic },
        LinkTimeFunction { constant: LinkTimeConstant::PromiseResolveWithThen, length: 2, name: "promiseResolveWithThen", function: promise_resolve_with_then_host, visibility: Private, intrinsic: no_intrinsic },
        LinkTimeFunction { constant: LinkTimeConstant::PerformPromiseThen, length: 4, name: "performPromiseThen", function: perform_promise_then_host, visibility: Private, intrinsic: no_intrinsic },
        LinkTimeFunction { constant: LinkTimeConstant::AsyncFunctionDrive, length: 2, name: "asyncFunctionDrive", function: async_function_drive_host, visibility: Private, intrinsic: no_intrinsic },
    ];
    for entry in table {
        let function = JSFunction::create_native(
            global_object.vm(),
            global_object,
            entry.length,
            &WtfString::from_latin1(entry.name.as_bytes()),
            entry.function,
            entry.visibility,
            entry.intrinsic,
            call_host_function_as_constructor,
        );
        global_object.set_link_time_constant(entry.constant, function.as_value());
    }
}
