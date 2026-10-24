//! `reportError` do global. O JavaScriptCore não o define: quem o instala é o bun (WebCore), como propriedade de
//! dados comum (`writable`, `enumerable`, `configurable`), `length` 1, `name` "reportError". Na ordem de chaves do
//! bun vem logo depois de `removeEventListener` e antes de `setImmediate`.
//!
//! A função entrega o primeiro argumento (qualquer valor, ou `undefined` sem argumento) ao relatório de erro não
//! capturado, o mesmo caminho de uma exceção que escapa de um callback de timer, e devolve `undefined`. Não lança
//! e não interrompe o programa: o código seguinte roda.
//!
//! DIVERGÊNCIAS: o bun dispara antes um `ErrorEvent` no global (`addEventListener("error")`, `onerror`); o porte
//! não tem `EventTarget` ainda, então o evento não existe e a entrega vai direto ao relatório.

use crate::host_function;
use crate::runtime::host_call::{HostCall, HostResult};
use crate::runtime::js_global_object::JSGlobalObject;
use crate::runtime::js_value::JSValue;

fn report_error_body(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    global_object.vm().report_unhandled_error(global_object, call.argument(0));
    Ok(JSValue::undefined())
}

host_function!(global_func_report_error, report_error_body);

/// Instala `reportError` no global como propriedade de dados comum.
pub fn add_report_error(global_object: &JSGlobalObject) {
    crate::runtime::native_class_support::install_global_function(global_object, "reportError", 1, global_func_report_error);
}
