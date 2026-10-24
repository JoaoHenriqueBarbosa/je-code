//! O módulo `node:net` do bun 1.4.2: o `net.ts` do bun (gerado em `api/node_net.js` por `scripts/gen-node-net.js`) sobre os
//! sockets do `bun_socket.js` (`Bun.listen`, `Bun.connect` e o que o bun guarda em nomes privados e funções nativas:
//! `$write`, `doConnect`, `newDetachedSocket`, `jsGetBufferedAmount`). O `host.socketInternals()` entrega esse conjunto ao
//! JavaScript gerado.

use crate::host_function;
use crate::runtime::host_call::{HostCall, HostResult};
use crate::runtime::identifier::Identifier;
use crate::runtime::js_global_object::JSGlobalObject;
use crate::runtime::string_regexp_support::get_object_property;

const NET: &str = include_str!("../api/node_net.js");

/// `socketInternals()`: o objeto `internals` que o `bun_socket.js` exporta.
fn socket_internals_body(global_object: &JSGlobalObject, _call: &HostCall) -> HostResult {
    let exports = crate::runtime::bun_http::socket_exports(global_object)?;
    get_object_property(global_object, exports, &Identifier::from_span(global_object.vm(), b"internals"))
}
host_function!(socket_internals_function, socket_internals_body);

/// Instalador de `node:net`.
pub(crate) fn install_net_module(global_object: &JSGlobalObject) -> HostResult {
    let host_object = crate::api::node_js_host::module_host(
        global_object,
        &[
            ("socketInternals", 0, socket_internals_function),
            ("requireInternal", 1, crate::api::node_stream::require_internal),
            ("internalModule", 1, crate::runtime::node_internals::internal_module_function),
        ],
    )?;
    Ok(crate::api::node_js_host::run_factory(global_object, NET, "net", &host_object))
}
