//! O registro dos módulos `internal/*` do bun 1.4.2 que `node:net`, `node:tls` e afins leem (`api/node_internals.js`, gerado por
//! `scripts/gen-node-internals.js`): uma fábrica por programa, para que os módulos gerados dividam os mesmos símbolos e o mesmo
//! `internal/shared`. `host.internalModule(nome)` devolve o módulo do registro.

use crate::host_function;
use crate::runtime::host_call::{HostCall, HostResult};
use crate::runtime::js_global_object::JSGlobalObject;
use crate::runtime::js_value::JSValue;

const INTERNALS: &str = include_str!("../api/node_internals.js");

/// O `require(nome)` do registro (avaliado uma vez por programa).
fn registry(global_object: &JSGlobalObject) -> HostResult {
    crate::api::builtin_modules::cached("internal:node-internals", || {
        let host_object = crate::api::node_js_host::module_host(
            global_object,
            &[
                ("requireInternal", 1, crate::api::node_stream::require_internal),
                ("getAsyncContext", 0, crate::api::node_async_hooks::get_async_context),
                ("setAsyncContext", 1, crate::api::node_async_hooks::set_async_context),
            ],
        )?;
        Ok(crate::api::node_js_host::run_factory(global_object, INTERNALS, "internals", &host_object))
    })
}

fn internal_module_body(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    let require = registry(global_object)?;
    crate::api::node_js_host::call_value(global_object, require, JSValue::undefined(), call.arguments(), "internal")
}
host_function!(pub internal_module_function, internal_module_body);
