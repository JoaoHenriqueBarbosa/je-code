//! `require("async_hooks")` do bun 1.4.2: `AsyncLocalStorage`, `AsyncResource`, `createHook` e afins, escritos em
//! JavaScript (`node_async_hooks.js`). Este arquivo entrega ao JavaScript o `$asyncContext` do JSC do bun (o contexto
//! assíncrono que o `VM` guarda e que promessas, `queueMicrotask`, `process.nextTick` e timers capturam e reinstalam) e o
//! `jsSetAsyncHooksEnabled`, que liga o rastreamento.

use crate::api::node_js_host::{module_host, run_factory};
use crate::host_function;
use crate::runtime::host_call::{HostCall, HostResult};
use crate::runtime::js_global_object::JSGlobalObject;
use crate::runtime::js_value::JSValue;

const SOURCE: &str = include_str!("node_async_hooks.js");

/// `host.getAsyncContext()`: `$getInternalField($asyncContext, 0)`.
fn get_async_context_body(global_object: &JSGlobalObject, _call: &HostCall) -> HostResult {
    Ok(global_object.vm().async_context())
}
host_function!(pub get_async_context, get_async_context_body);

/// `host.setAsyncContext(frame)`: `$putInternalField($asyncContext, 0, frame)`.
fn set_async_context_body(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    global_object.vm().set_async_context(call.argument(0));
    Ok(JSValue::undefined())
}
host_function!(pub set_async_context, set_async_context_body);

/// `host.setAsyncHooksEnabled(ligado)`: `jsSetAsyncHooksEnabled` (`NodeAsyncHooks.cpp`).
fn set_async_hooks_enabled_body(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    global_object.vm().set_async_context_tracking(call.argument(0).to_boolean());
    Ok(JSValue::undefined())
}
host_function!(set_async_hooks_enabled, set_async_hooks_enabled_body);

/// Instalador de `node:async_hooks`.
pub(crate) fn install_async_hooks_module(global_object: &JSGlobalObject) -> HostResult {
    let host = module_host(
        global_object,
        &[("getAsyncContext", 0, get_async_context), ("setAsyncContext", 1, set_async_context), ("setAsyncHooksEnabled", 1, set_async_hooks_enabled)],
    )?;
    Ok(run_factory(global_object, SOURCE, "async_hooks", &host))
}
