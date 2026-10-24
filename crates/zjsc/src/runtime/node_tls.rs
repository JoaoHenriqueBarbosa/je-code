//! O módulo `node:tls` do bun 1.4.2: o `tls.ts` do bun (gerado em `api/node_tls.js` por `scripts/gen-node-tls.js`). O que no bun
//! é BoringSSL (`SecureContext`, o handshake) ainda não existe e lança "not implemented" no ponto de uso; o que é JavaScript no
//! bun (`checkServerIdentity`, `canonicalizeIP`, as listas de cifras e os certificados raiz embutidos) está inteiro no texto gerado.

use crate::runtime::host_call::HostResult;
use crate::runtime::js_global_object::JSGlobalObject;

const TLS: &str = include_str!("../api/node_tls.js");

/// Instalador de `node:tls`.
pub(crate) fn install_tls_module(global_object: &JSGlobalObject) -> HostResult {
    let host_object = crate::api::node_js_host::module_host(
        global_object,
        &[
            ("requireInternal", 1, crate::api::node_stream::require_internal),
            ("internalModule", 1, crate::runtime::node_internals::internal_module_function),
        ],
    )?;
    Ok(crate::api::node_js_host::run_factory(global_object, TLS, "tls", &host_object))
}
