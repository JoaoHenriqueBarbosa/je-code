//! `Bun.dns`: o objeto do `bun_dns.js` (lookup, resolve*, getServers...), montado uma vez por programa no primeiro acesso.

use crate::runtime::host_call::HostResult;
use crate::runtime::js_global_object::JSGlobalObject;

const SOURCE: &str = include_str!("bun_dns.js");

/// O objeto `Bun.dns`.
pub(crate) fn dns_object(global_object: &JSGlobalObject) -> HostResult {
    crate::api::builtin_modules::cached("internal:bun-dns", || {
        let host = crate::api::node_js_host::module_host(global_object, &[])?;
        Ok(crate::api::node_js_host::run_native_factory(global_object, SOURCE, "internal:bun-dns", &host))
    })
}
