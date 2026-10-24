//! `postMessage` do global. O JavaScriptCore não o define: quem o instala é o bun (WebCore), como propriedade de
//! dados comum (`writable`, `enumerable`, `configurable`), `length` 1, `name` "postMessage", sem `prototype`, e
//! `new postMessage()` é `TypeError`. Na ordem de chaves do bun vem depois de `fetch` e antes de `prompt`.
//!
//! Na thread principal, sem `Worker`, medido no bun 1.4.2 que a função não faz nada observável: aceita qualquer
//! número de argumentos de qualquer tipo (função, Symbol, objeto cíclico, getter que lança) sem clonar nem tocar
//! em nenhum deles, qualquer `this`, e devolve `undefined`. Não há `DataCloneError`.

use crate::host_function;
use crate::runtime::host_call::{HostCall, HostResult};
use crate::runtime::js_global_object::JSGlobalObject;
use crate::runtime::js_value::JSValue;

fn post_message_body(_global_object: &JSGlobalObject, _call: &HostCall) -> HostResult {
    Ok(JSValue::undefined())
}

host_function!(global_func_post_message, post_message_body);

/// Instala `postMessage` no global como propriedade de dados comum.
pub fn add_post_message(global_object: &JSGlobalObject) {
    crate::runtime::native_class_support::install_global_function(global_object, "postMessage", 1, global_func_post_message);
}
