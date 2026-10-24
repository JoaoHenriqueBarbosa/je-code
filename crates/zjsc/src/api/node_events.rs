//! `require("events")` do bun 1.4.2: o `EventEmitter` completo.
//!
//! O módulo é o `src/js/node/events.ts` do bun portado para JavaScript comum (`node_events.js`) e avaliado como
//! programa de nome `node:events`: as funções ficam visíveis na pilha, como as do módulo no bun, e a mensagem de
//! erro cita a expressão do próprio módulo (`evaluating 'this._events'`). Este arquivo só entrega ao JavaScript os
//! ouvintes de um `EventTarget`, que ele não alcança sozinho; o resto do `host` está em `node_js_host.rs`.

use crate::api::node_js_host::{module_host, run_factory};
use crate::host_function;
use crate::runtime::event_target::{is_target, kind_argument, listener_callbacks};
use crate::runtime::host_call::{HostCall, HostResult, Thrown};
use crate::runtime::js_global_object::JSGlobalObject;
use crate::runtime::js_value::{js_boolean, js_number, JSValue};
use crate::runtime::object_constructor::construct_array_of;

const SOURCE: &str = include_str!("node_events.js");

/// `host.isEventTarget(valor)`.
fn is_event_target_body(_global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    Ok(js_boolean(is_target(call.argument(0))))
}
host_function!(is_event_target, is_event_target_body);

/// Os ouvintes de `addEventListener` de `target` no tipo `type`; `None` quando `target` não é um `EventTarget`.
fn event_target_callbacks(global_object: &JSGlobalObject, call: &HostCall) -> Result<Option<Vec<JSValue>>, Thrown> {
    let kind = kind_argument(global_object, call.argument(1))?;
    Ok(listener_callbacks(call.argument(0), &kind))
}

/// `host.eventTargetListeners(alvo, tipo)`: `getEventListeners` de um `EventTarget` (`[]` quando não é um).
fn event_target_listeners_body(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    let callbacks = event_target_callbacks(global_object, call)?.unwrap_or_default();
    Ok(construct_array_of(global_object, &callbacks).as_value())
}
host_function!(event_target_listeners, event_target_listeners_body);

/// `host.eventTargetListenerCount(alvo, tipo)`: o número de ouvintes, ou `undefined` quando `alvo` não é um `EventTarget`.
fn event_target_listener_count_body(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    Ok(event_target_callbacks(global_object, call)?.map_or_else(JSValue::undefined, |callbacks| js_number(callbacks.len() as u32)))
}
host_function!(event_target_listener_count, event_target_listener_count_body);

/// Instalador de `node:events`.
pub(crate) fn install_events_module(global_object: &JSGlobalObject) -> HostResult {
    let host = module_host(
        global_object,
        &[("isEventTarget", 1, is_event_target), ("eventTargetListeners", 2, event_target_listeners), ("eventTargetListenerCount", 2, event_target_listener_count)],
    )?;
    Ok(run_factory(global_object, SOURCE, "events", &host))
}
