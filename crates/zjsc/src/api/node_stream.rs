//! `require("string_decoder")`, `require("stream")`, `require("querystring")`, `require("timers")` e
//! `require("timers/promises")` do bun 1.4.2, escritos em JavaScript (`node_string_decoder.js`, `node_stream_modules.js`,
//! gerado por `scripts/gen-node-stream.js` a partir dos fontes do stream do bun, `node_querystring.js`, o
//! `querystring.ts` do bun, e `node_timers.js` e `node_timers_promises.js`, os `timers*.ts` do bun).

use crate::api::builtin_modules::{cached, registered_by};
use crate::api::node_js_host::{install_js_module, module_host, run_factory};
use crate::host_function;
use crate::runtime::host_call::{HostCall, HostResult, Thrown};
use crate::runtime::js_global_object::JSGlobalObject;
use crate::runtime::js_module_loader::rust_string;
use crate::runtime::js_value::JSValue;
use crate::runtime::node_error::throw_coded_error;
use crate::runtime::object_to_primitive::call_function;

const STRING_DECODER: &str = include_str!("node_string_decoder.js");
const STREAM: &str = include_str!("node_stream_modules.js");
const QUERYSTRING: &str = include_str!("node_querystring.js");
const TIMERS: &str = include_str!("node_timers.js");
const TIMERS_PROMISES: &str = include_str!("node_timers_promises.js");

/// O nome em `builtin_modules::cached` do `require` interno do pacote de `stream` (o que resolve `internal/streams/*`).
const STREAM_REQUIRE: &str = "internal:stream-require";

/// `host.registerRequire(require)`: o pacote de `stream` entrega o `require` interno dele ao hospedeiro.
fn register_require_body(_global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    let function = call.argument(0);
    cached(STREAM_REQUIRE, || Ok(function))?;
    Ok(JSValue::undefined())
}
host_function!(register_require, register_require_body);

/// `host.requireInternal(nome)`: o `require("internal/streams/utils")` que o `util.ts` do bun faz no registro interno de
/// módulos. Carrega `node:stream` se ainda não foi carregado e resolve `nome` no `require` interno dele.
fn require_internal_body(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    let name = rust_string(&call.argument(0).to_wtf_string());
    let require = registered_by(global_object, "stream", STREAM_REQUIRE, || throw_coded_error(global_object, &format!("Cannot find module '{name}'"), "MODULE_NOT_FOUND"))?;
    call_function(global_object, require, JSValue::undefined(), &[call.argument(0)]).ok_or(Thrown::Pending)
}
host_function!(pub require_internal, require_internal_body);

/// Instalador de `node:string_decoder`.
pub(crate) fn install_string_decoder_module(global_object: &JSGlobalObject) -> HostResult {
    install_js_module(global_object, STRING_DECODER, "string_decoder")
}

/// Instalador de `node:stream`.
pub(crate) fn install_stream_module(global_object: &JSGlobalObject) -> HostResult {
    let host = module_host(global_object, &[("registerRequire", 1, register_require)])?;
    Ok(run_factory(global_object, STREAM, "stream", &host))
}

/// Instalador de `node:stream/promises`: o `promises` de `node:stream`.
pub(crate) fn install_stream_promises_module(global_object: &JSGlobalObject) -> HostResult {
    crate::api::builtin_modules::member_of(global_object, "stream", "promises")
}

/// Instalador de `node:querystring`.
pub(crate) fn install_querystring_module(global_object: &JSGlobalObject) -> HostResult {
    install_js_module(global_object, QUERYSTRING, "querystring")
}

/// Instalador de `node:timers`.
pub(crate) fn install_timers_module(global_object: &JSGlobalObject) -> HostResult {
    install_js_module(global_object, TIMERS, "timers")
}

/// Instalador de `node:timers/promises`.
pub(crate) fn install_timers_promises_module(global_object: &JSGlobalObject) -> HostResult {
    install_js_module(global_object, TIMERS_PROMISES, "timers/promises")
}
