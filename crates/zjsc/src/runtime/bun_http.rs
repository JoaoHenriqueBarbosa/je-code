//! `Bun.serve` e o `fetch` de `http:` em loopback: o programa JavaScript `bun_http.js` sobre as funções de rede do
//! `net_loop` (ver os dois). Aqui ficam o carregamento do módulo e as duas entradas que o runtime chama.

use crate::host_function;
use crate::runtime::host_call::{HostCall, HostResult};
use crate::runtime::identifier::Identifier;
use crate::runtime::js_global_object::JSGlobalObject;
use crate::runtime::native_function::NativeFunction;
use crate::runtime::net_loop::NATIVES;
use crate::runtime::string_regexp_support::get_object_property;

const SOURCE: &str = include_str!("bun_http.js");
const SOCKET_SOURCE: &str = include_str!("bun_socket.js");

/// `refused(url)`: lança o `ConnectionRefused` do `fetch` (a conexão a `url` foi recusada).
fn refused_body(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    let path = crate::runtime::js_module_loader::rust_string(&call.argument(0).to_wtf_string());
    Err(crate::runtime::fetch::connection_refused(global_object, &path))
}
host_function!(refused_function, refused_body);

/// `fatal(mensagem)`: o erro fatal do bun (ver `process_exit::fatal_message`).
fn fatal_body(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    let message = crate::runtime::js_module_loader::rust_string(&call.argument(0).to_wtf_string());
    crate::runtime::process_exit::fatal_message(global_object, &message)
}
host_function!(fatal_function, fatal_body);

/// `reportError(erro, rótulo)`: o erro de um handler sem tratador no stderr (ver `uncaught_report`).
fn report_error_body(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    let label = crate::runtime::js_module_loader::rust_string(&call.argument(1).to_wtf_string());
    crate::runtime::uncaught_report::report_handler_error(global_object, call.argument(0), &label);
    Ok(crate::runtime::js_value::JSValue::undefined())
}
host_function!(report_error_function, report_error_body);

/// Carrega (uma vez por programa) um programa escondido de rede e devolve os exports dele: `bun_http.js` (`serve`, `fetch`)
/// e `bun_socket.js` (`listen`, `connect`), os dois sobre as mesmas funções `net*` do `Host`.
fn module(global_object: &JSGlobalObject, cache_name: &'static str, source: &str) -> HostResult {
    crate::api::builtin_modules::cached(cache_name, || {
        let mut natives: Vec<(&str, u32, NativeFunction)> = NATIVES.to_vec();
        natives.push(("responseWire", 1, crate::runtime::response::response_wire));
        natives.push(("markFetched", 3, crate::runtime::response::mark_fetched));
        natives.push(("refused", 1, refused_function));
        natives.push(("fatal", 1, fatal_function));
        natives.push(("reportError", 2, report_error_function));
        let host = crate::api::node_js_host::module_host(global_object, &natives)?;
        Ok(crate::api::node_js_host::run_native_factory(global_object, source, cache_name, &host))
    })
}

/// Os exports do `bun_socket.js` (`listen`, `connect`, `internals`), para o `node:net`.
pub(crate) fn socket_exports(global_object: &JSGlobalObject) -> HostResult {
    module(global_object, "internal:bun-socket", SOCKET_SOURCE)
}

/// Chama o export `name` do programa `cache_name` com os argumentos do chamador.
fn call_program(global_object: &JSGlobalObject, cache_name: &'static str, source: &str, name: &str, arguments: &[crate::runtime::js_value::JSValue]) -> HostResult {
    let exports = module(global_object, cache_name, source)?;
    let function = get_object_property(global_object, exports, &Identifier::from_span(global_object.vm(), name.as_bytes()))?;
    crate::api::node_js_host::call_value(global_object, function, crate::runtime::js_value::JSValue::undefined(), arguments, "bun:http")
}

/// Chama `serve` ou `fetch` do `bun_http.js` com os argumentos do chamador.
pub(crate) fn call_module(global_object: &JSGlobalObject, name: &str, arguments: &[crate::runtime::js_value::JSValue]) -> HostResult {
    call_program(global_object, "internal:bun-http", SOURCE, name, arguments)
}

/// `Bun.serve(opções)`.
fn serve_body(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    call_module(global_object, "serve", call.arguments())
}
host_function!(pub bun_serve, serve_body);

/// `Bun.listen(opções)` e `Bun.connect(opções)`: os sockets TCP do `bun_socket.js`.
fn listen_body(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    call_program(global_object, "internal:bun-socket", SOCKET_SOURCE, "listen", call.arguments())
}
host_function!(pub bun_listen, listen_body);

fn connect_body(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    call_program(global_object, "internal:bun-socket", SOCKET_SOURCE, "connect", call.arguments())
}
host_function!(pub bun_connect, connect_body);

/// O texto do endereço é de loopback (`localhost` ou `127.x.x.x`): o `bun_http.js` atende; qualquer outro host segue
/// o caminho de uma máquina sem rede.
pub(crate) fn is_loopback_host(host: &str) -> bool {
    host == "localhost" || (host.split('.').count() == 4 && host.starts_with("127.") && host.split('.').all(|part| part.parse::<u8>().is_ok()))
}
