//! `queueMicrotask` do global. O JavaScriptCore não o define (não há `queueMicrotask` em
//! `JSGlobalObject*.cpp` nem em `JSGlobalObjectFunctions.cpp`): quem o instala é o bun, pelo lado do
//! WebCore/Zig, com o descritor de uma propriedade de dados comum (`writable`, `enumerable`,
//! `configurable`), `name` "queueMicrotask" e `length` 1.
//!
//! A tarefa é a `InvokeFunctionJob` da fila padrão: chama o callback com `this` indefinido e sem
//! argumentos, na ordem da fila relativa a `Promise.resolve().then`.
//!
//! O erro de argumento que não é função é um `TypeError` com `code` `ERR_INVALID_ARG_TYPE` (herdado do
//! protótipo codificado, como no bun), via `throw_coded_type_error`.

use crate::host_function;
use crate::runtime::host_call::{HostCall, HostResult};
use crate::runtime::node_error::throw_coded_type_error;
use crate::runtime::js_global_object::JSGlobalObject;
use crate::runtime::js_value::JSValue;
use crate::runtime::js_web_assembly::received_description;
use crate::runtime::microtask::InternalMicrotask;
use crate::runtime::microtask_queue::QueuedTask;

/// O corpo do `queueMicrotask(callback)`.
fn queue_microtask_body(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    let callback = call.argument(0);
    if !callback.is_callable() {
        let message = format!(
            "The \"callback\" argument must be of type function. Received {}",
            received_description(global_object, callback)
        );
        return Err(throw_coded_type_error(global_object, &message, "ERR_INVALID_ARG_TYPE"));
    }
    // `arguments[1]` é o contexto assíncrono do agendamento (o `AsyncContextFrame` do bun), vazio sem rastreamento.
    let vm = global_object.vm();
    let async_context = vm.captured_async_context();
    vm.default_microtask_queue
        .enqueue(QueuedTask::new(global_object.cell_id(), InternalMicrotask::InvokeFunctionJob, 0, &[callback, async_context]));
    Ok(JSValue::undefined())
}

host_function!(global_func_queue_microtask, queue_microtask_body);

/// Instala `queueMicrotask` no global como propriedade de dados comum.
pub fn add_queue_microtask(global_object: &JSGlobalObject) {
    crate::runtime::native_class_support::install_global_function(global_object, "queueMicrotask", 1, global_func_queue_microtask);
}
