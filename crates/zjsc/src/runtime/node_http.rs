//! Os módulos do bun 1.4.2 que só dependem do registro de `internal/*` (`api/node_internals.js`): `node:diagnostics_channel`,
//! `node:http`, `node:https`, `node:child_process`, `node:zlib` e os `node:_http_*`, gerados por `scripts/gen-node-modules.js` (um arquivo `api/node_<nome>.js` por
//! módulo). O servidor (`Bun.serve` com `onNodeHTTPRequest`) e o analisador HTTP nativo ainda não existem e lançam "not
//! implemented" no ponto de uso.

use crate::host_function;
use crate::runtime::host_call::{HostCall, HostResult};
use crate::runtime::js_global_object::JSGlobalObject;
use crate::runtime::native_function::NativeFunction;

/// Monta a fábrica `source` (o módulo `label`) com o `host` dos módulos gerados mais as funções `extra`.
fn install_with(global_object: &JSGlobalObject, source: &str, label: &str, extra: &[(&str, u32, NativeFunction)]) -> HostResult {
    let mut natives: Vec<(&str, u32, NativeFunction)> = vec![
        ("internalModule", 1, crate::runtime::node_internals::internal_module_function),
        ("getAsyncContext", 0, crate::api::node_async_hooks::get_async_context),
        ("setAsyncContext", 1, crate::api::node_async_hooks::set_async_context),
    ];
    natives.extend_from_slice(extra);
    let host_object = crate::api::node_js_host::module_host(global_object, &natives)?;
    Ok(crate::api::node_js_host::run_factory(global_object, source, label, &host_object))
}

macro_rules! installers {
    ($($name:ident => ($source:literal, $label:literal)),* $(,)?) => {
        $(
            #[doc = concat!("Instalador de `node:", $label, "`.")]
            pub(crate) fn $name(global_object: &JSGlobalObject) -> HostResult {
                install_with(global_object, include_str!($source), $label, &[])
            }
        )*
    };
}

installers! {
    install_diagnostics_channel_module => ("../api/node_diagnostics_channel.js", "diagnostics_channel"),
    install_http_module => ("../api/node_http.js", "http"),
    install_https_module => ("../api/node_https.js", "https"),
    install_zlib_module => ("../api/node_zlib.js", "zlib"),
    install_constants_module => ("../api/node_constants.js", "constants"),
    install_http2_module => ("../api/node_http2.js", "http2"),
    install_http2_upgrade_module => ("../api/node__http2_upgrade.js", "_http2_upgrade"),
    install_dns_module => ("../api/node_dns.js", "dns"),
    install_dns_promises_module => ("../api/node_dns_promises.js", "dns/promises"),
    install_http_agent_module => ("../api/node__http_agent.js", "_http_agent"),
    install_http_client_module => ("../api/node__http_client.js", "_http_client"),
    install_http_common_module => ("../api/node__http_common.js", "_http_common"),
    install_http_incoming_module => ("../api/node__http_incoming.js", "_http_incoming"),
    install_http_outgoing_module => ("../api/node__http_outgoing.js", "_http_outgoing"),
    install_http_server_module => ("../api/node__http_server.js", "_http_server"),
}

host_function!(fs_streams_function, fs_streams_body);

/// `host.fsStreams()`: os exports do módulo gerado de `internal/fs/streams.ts` (`writableFromFileSink`, `kWriteStreamFastPath`).
fn fs_streams_body(global_object: &JSGlobalObject, _call: &HostCall) -> HostResult {
    crate::runtime::node_fs::streams_module(global_object)
}

/// Instalador de `node:child_process`: o `host` leva, além do comum, o módulo de streams do `fs`.
pub(crate) fn install_child_process_module(global_object: &JSGlobalObject) -> HostResult {
    install_with(global_object, include_str!("../api/node_child_process.js"), "child_process", &[("fsStreams", 0, fs_streams_function)])
}
