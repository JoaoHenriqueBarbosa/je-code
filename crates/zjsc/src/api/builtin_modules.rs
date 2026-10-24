//! Registro dos módulos embutidos do `require` (`node:vm`, `node:fs`, ...).
//!
//! O bun aceita um módulo embutido com e sem o prefixo `node:` (`require("vm") === require("node:vm")`), exceto os que
//! só existem com prefixo (`node:test`, `node:sqlite`: `require("test")` cai na busca em `node_modules`). Com o prefixo
//! `node:` e nome fora da tabela, o `require` lança `ERR_UNKNOWN_BUILTIN_MODULE` (`throw_require_failure`).
//!
//! Cada entrada tem um instalador (`Installer`) que monta o objeto do módulo na primeira chamada; o resultado fica num
//! cache por nome canônico (sem prefixo), então `require("vm")` e `require("node:vm")` devolvem o mesmo objeto. Os
//! embutidos não entram em `require.cache` (medido no bun 1.4.2: `require.cache["os"]` é `undefined`).
//!
//! Para acrescentar um módulo: escrever o instalador no arquivo do módulo e pôr uma linha em `BUILTINS`.

use std::cell::RefCell;

use crate::api::eval::{js_text, key_of};
use crate::runtime::host_call::{HostResult, Thrown};
use crate::runtime::identifier::Identifier;
use crate::runtime::js_global_object::JSGlobalObject;
use crate::runtime::js_value::JSValue;
use crate::runtime::object_constructor::{construct_array_of, construct_empty_object, enumerable_own_entries};
use crate::runtime::string_regexp_support::get_object_property;

/// Monta o objeto do módulo. Roda uma vez por programa; o resultado é guardado no cache do registro.
pub(crate) type Installer = fn(&JSGlobalObject) -> HostResult;

/// Como o nome do módulo é aceito pelo `require`.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum Scheme {
    /// `vm` e `node:vm`.
    Both,
    /// Só `node:test`: o nome nu segue para a busca em `node_modules`.
    NodeOnly,
}

#[derive(Debug)]
pub(crate) struct BuiltinEntry {
    /// O nome canônico, sem prefixo (`vm`, `fs/promises`).
    pub name: &'static str,
    pub scheme: Scheme,
    pub install: Installer,
}

/// O que o `require` faz com um pedido.
#[derive(Debug)]
pub(crate) enum Resolved<'a> {
    Builtin(&'a BuiltinEntry),
    /// Tem prefixo `node:` e o nome não está na tabela.
    UnknownNode,
    /// Não é embutido: segue a resolução de arquivos.
    NotBuiltin,
}

/// Os módulos instalados. Acrescentar aqui, na ordem alfabética.
pub(crate) static BUILTINS: &[BuiltinEntry] = &[
    BuiltinEntry { name: "_http2_upgrade", scheme: Scheme::NodeOnly, install: crate::runtime::node_http::install_http2_upgrade_module },
    BuiltinEntry { name: "_http_agent", scheme: Scheme::Both, install: crate::runtime::node_http::install_http_agent_module },
    BuiltinEntry { name: "_http_client", scheme: Scheme::Both, install: crate::runtime::node_http::install_http_client_module },
    BuiltinEntry { name: "_http_common", scheme: Scheme::Both, install: crate::runtime::node_http::install_http_common_module },
    BuiltinEntry { name: "_http_incoming", scheme: Scheme::Both, install: crate::runtime::node_http::install_http_incoming_module },
    BuiltinEntry { name: "_http_outgoing", scheme: Scheme::Both, install: crate::runtime::node_http::install_http_outgoing_module },
    BuiltinEntry { name: "_http_server", scheme: Scheme::Both, install: crate::runtime::node_http::install_http_server_module },
    BuiltinEntry { name: "assert", scheme: Scheme::Both, install: crate::api::node_assert::install_assert_module },
    BuiltinEntry { name: "assert/strict", scheme: Scheme::Both, install: crate::api::node_assert::install_assert_strict_module },
    BuiltinEntry { name: "async_hooks", scheme: Scheme::Both, install: crate::api::node_async_hooks::install_async_hooks_module },
    BuiltinEntry { name: "buffer", scheme: Scheme::Both, install: crate::runtime::node_buffer::install_buffer_module },
    BuiltinEntry { name: "child_process", scheme: Scheme::Both, install: crate::runtime::node_http::install_child_process_module },
    BuiltinEntry { name: "constants", scheme: Scheme::Both, install: crate::runtime::node_http::install_constants_module },
    BuiltinEntry { name: "crypto", scheme: Scheme::Both, install: crate::runtime::node_crypto::install_crypto_module },
    BuiltinEntry { name: "diagnostics_channel", scheme: Scheme::Both, install: crate::runtime::node_http::install_diagnostics_channel_module },
    BuiltinEntry { name: "dns", scheme: Scheme::Both, install: crate::runtime::node_http::install_dns_module },
    BuiltinEntry { name: "dns/promises", scheme: Scheme::Both, install: crate::runtime::node_http::install_dns_promises_module },
    BuiltinEntry { name: "events", scheme: Scheme::Both, install: crate::api::node_events::install_events_module },
    BuiltinEntry { name: "fs", scheme: Scheme::Both, install: crate::runtime::node_fs::install_fs_module },
    BuiltinEntry { name: "fs/promises", scheme: Scheme::Both, install: crate::runtime::node_fs::install_fs_promises_module },
    BuiltinEntry { name: "http", scheme: Scheme::Both, install: crate::runtime::node_http::install_http_module },
    BuiltinEntry { name: "http2", scheme: Scheme::Both, install: crate::runtime::node_http::install_http2_module },
    BuiltinEntry { name: "https", scheme: Scheme::Both, install: crate::runtime::node_http::install_https_module },
    BuiltinEntry { name: "module", scheme: Scheme::Both, install: install_module_module },
    BuiltinEntry { name: "net", scheme: Scheme::Both, install: crate::runtime::node_net::install_net_module },
    BuiltinEntry { name: "os", scheme: Scheme::Both, install: crate::runtime::node_os::install_os_module },
    BuiltinEntry { name: "path", scheme: Scheme::Both, install: crate::runtime::node_path::install_path_module },
    BuiltinEntry { name: "path/posix", scheme: Scheme::Both, install: install_path_posix },
    BuiltinEntry { name: "path/win32", scheme: Scheme::Both, install: install_path_win32 },
    BuiltinEntry { name: "process", scheme: Scheme::Both, install: install_process_module },
    BuiltinEntry { name: "querystring", scheme: Scheme::Both, install: crate::api::node_stream::install_querystring_module },
    BuiltinEntry { name: "stream", scheme: Scheme::Both, install: crate::api::node_stream::install_stream_module },
    BuiltinEntry { name: "stream/promises", scheme: Scheme::Both, install: crate::api::node_stream::install_stream_promises_module },
    BuiltinEntry { name: "string_decoder", scheme: Scheme::Both, install: crate::api::node_stream::install_string_decoder_module },
    BuiltinEntry { name: "timers", scheme: Scheme::Both, install: crate::api::node_stream::install_timers_module },
    BuiltinEntry { name: "timers/promises", scheme: Scheme::Both, install: crate::api::node_stream::install_timers_promises_module },
    BuiltinEntry { name: "tls", scheme: Scheme::Both, install: crate::runtime::node_tls::install_tls_module },
    BuiltinEntry { name: "tty", scheme: Scheme::Both, install: crate::runtime::node_tty::install_tty_module },
    BuiltinEntry { name: "url", scheme: Scheme::Both, install: crate::api::node_url::install_url_module },
    BuiltinEntry { name: "util", scheme: Scheme::Both, install: crate::api::node_util::install_util_module },
    BuiltinEntry { name: "util/types", scheme: Scheme::Both, install: crate::api::node_util_types::install_util_types_module },
    BuiltinEntry { name: "vm", scheme: Scheme::Both, install: crate::api::eval::install_vm_module },
    BuiltinEntry { name: "zlib", scheme: Scheme::Both, install: crate::runtime::node_http::install_zlib_module },
];

/// `require("module").builtinModules` do bun 1.4.2, na ordem exata medida com `bun -e` (76 nomes). Os `bun:*` já
/// constam, embora o `require("bun:test")` ainda falhe como módulo inexistente até haver instalador (a resolução cai
/// em `MODULE_NOT_FOUND`, igual ao `bun:zz`); `node:sqlite` é o único com prefixo na lista.
pub(crate) static BUILTIN_MODULE_NAMES: &[&str] = &[
    "_http_agent", "_http_client", "_http_common", "_http_incoming", "_http_outgoing", "_http_server", "_stream_duplex",
    "_stream_passthrough", "_stream_readable", "_stream_transform", "_stream_wrap", "_stream_writable", "_tls_common",
    "_tls_wrap", "assert", "assert/strict", "async_hooks", "buffer", "bun:ffi", "bun:jsc", "bun:sqlite", "bun:test", "bun",
    "child_process", "cluster", "console", "constants", "crypto", "dgram", "diagnostics_channel", "dns", "dns/promises",
    "domain", "events", "fs", "fs/promises", "http", "http2", "https", "inspector", "inspector/promises", "module", "net",
    "node:sqlite", "os", "path", "path/posix", "path/win32", "perf_hooks", "process", "punycode", "querystring", "readline",
    "readline/promises", "repl", "stream", "stream/consumers", "stream/promises", "stream/web", "string_decoder", "sys",
    "timers", "timers/promises", "tls", "trace_events", "tty", "undici", "url", "util", "util/types", "v8", "vm", "wasi",
    "worker_threads", "ws", "zlib",
];

/// O que `import.meta.resolveSync` devolve a um módulo que o resolvedor do bun conhece, qualquer que seja o
/// `parent` (medido no bun 1.4.2): `node:x` como veio, o nome nu com o prefixo (`fs` dá `node:fs`) e os `bun*`
/// sem prefixo. `None` para o resto.
pub(crate) fn known_builtin_specifier(specifier: &str) -> Option<String> {
    let known = |name: &str| BUILTIN_MODULE_NAMES.iter().any(|entry| *entry == name);
    if let Some(name) = specifier.strip_prefix("node:") {
        return (known(name) || known(specifier)).then(|| specifier.to_owned());
    }
    if !known(specifier) {
        return None;
    }
    Some(if specifier == "bun" || specifier.starts_with("bun:") { specifier.to_owned() } else { format!("node:{specifier}") })
}

/// `require("path/posix")` e `require("path/win32")` devolvem o mesmo objeto de `require("path").posix` e `.win32`
/// (medido no bun 1.4.2: `require("path/posix") === require("path")`), então saem do módulo `path` já carregado.
fn path_flavor(global_object: &JSGlobalObject, flavor: &str) -> HostResult {
    let path = BUILTINS.iter().find(|entry| entry.name == "path").expect("o módulo path consta em BUILTINS");
    let module = load(global_object, path)?;
    get_object_property(global_object, module, &Identifier::from_span(global_object.vm(), flavor.as_bytes()))
}

/// `require("process")` é o próprio global `process` (medido no bun 1.4.2: `require("process") === process`).
fn install_process_module(global_object: &JSGlobalObject) -> HostResult {
    get_object_property(global_object, global_object.as_value(), &Identifier::from_span(global_object.vm(), b"process"))
}

fn install_path_posix(global_object: &JSGlobalObject) -> HostResult {
    path_flavor(global_object, "posix")
}

fn install_path_win32(global_object: &JSGlobalObject) -> HostResult {
    path_flavor(global_object, "win32")
}

/// Instalador de `node:module`: por ora só `builtinModules`, o array da tabela `BUILTIN_MODULE_NAMES`.
fn install_module_module(global_object: &JSGlobalObject) -> HostResult {
    let vm = global_object.vm();
    let module_object = construct_empty_object(global_object);
    let names: Vec<JSValue> = BUILTIN_MODULE_NAMES.iter().map(|name| js_text(vm, name)).collect();
    let array = construct_array_of(global_object, &names);
    module_object.put_direct(vm, &key_of(vm, "builtinModules"), array.as_value(), 0);
    Ok(module_object.as_value())
}

/// O módulo interno `bun:wrap`: o `src/runtime.js` do bun 1.4.2 (MIT), que o transpilador importa quando um arquivo usa
/// os helpers dele (decorators do TC39, `__toESM`, `__require` e afins). É um módulo ES de fonte própria, então o
/// carregador o lê daqui em vez de pedir ao host (`bun_wrap_source`).
pub(crate) const BUN_WRAP: &str = "bun:wrap";

/// O texto de `bun:wrap` quando `key` é a chave dele.
pub(crate) fn bun_wrap_source(key: &str) -> Option<&'static str> {
    (key == BUN_WRAP).then_some(include_str!("bun_wrap.js"))
}

/// O que o resolvedor do bun faz com um `import` de embutido, antes de olhar o disco: a chave canônica (`node:path`)
/// do embutido instalado, a falha de `node:` fora da tabela (`ERR_UNKNOWN_BUILTIN_MODULE`), ou `None` quando
/// `specifier` não é embutido e segue a resolução de arquivos.
pub(crate) fn resolve_import(specifier: &str) -> Option<Result<String, String>> {
    if specifier == BUN_WRAP {
        return Some(Ok(BUN_WRAP.to_owned()));
    }
    match resolve(specifier) {
        Resolved::Builtin(entry) => Some(Ok(format!("node:{}", entry.name))),
        Resolved::UnknownNode => Some(Err(crate::runtime::js_module_loader::unknown_builtin_message(specifier))),
        Resolved::NotBuiltin => None,
    }
}

/// Os exports do namespace de `import "node:X"`: `default` é o mesmo objeto do `require`, e cada propriedade própria
/// enumerável dele vira export nomeado (medido: `import * as os from "node:os"` tem as chaves do `require("os")` mais
/// `default`). `None` quando `key` não é uma chave de `resolve_import`.
pub(crate) fn import_exports(global_object: &JSGlobalObject, key: &str) -> Option<Result<(Vec<Identifier>, Vec<JSValue>), Thrown>> {
    let name = key.strip_prefix("node:")?;
    let entry = BUILTINS.iter().find(|entry| entry.name == name)?;
    Some(exports_of(global_object, entry))
}

fn exports_of(global_object: &JSGlobalObject, entry: &'static BuiltinEntry) -> Result<(Vec<Identifier>, Vec<JSValue>), Thrown> {
    let vm = global_object.vm();
    let object = load(global_object, entry)?;
    let mut names = vec![Identifier::from_span(vm, b"default")];
    let mut values = vec![object];
    for (name, value) in enumerable_own_entries(global_object, object)? {
        if names.contains(&name) {
            continue;
        }
        names.push(name);
        values.push(value);
    }
    Ok((names, values))
}

thread_local! {
    /// Nome canônico para o objeto do módulo, por programa.
    static CACHE: RefCell<Vec<(&'static str, JSValue)>> = const { RefCell::new(Vec::new()) };
}

/// Classifica `request` contra `table`.
pub(crate) fn resolve_in<'a>(table: &'a [BuiltinEntry], request: &str) -> Resolved<'a> {
    if let Some(name) = request.strip_prefix("node:") {
        return table.iter().find(|entry| entry.name == name).map_or(Resolved::UnknownNode, Resolved::Builtin);
    }
    table
        .iter()
        .find(|entry| entry.scheme == Scheme::Both && entry.name == request)
        .map_or(Resolved::NotBuiltin, Resolved::Builtin)
}

pub(crate) fn resolve(request: &str) -> Resolved<'static> {
    resolve_in(BUILTINS, request)
}

/// O objeto do módulo: do cache, ou recém-instalado (e guardado, com o texto do bun nas funções dele).
pub(crate) fn load(global_object: &JSGlobalObject, entry: &'static BuiltinEntry) -> HostResult {
    cached(entry.name, || {
        let installed = (entry.install)(global_object)?;
        crate::api::builtin_source_texts::apply(global_object, entry.name, installed);
        Ok(installed)
    })
}

/// A propriedade `member` do módulo embutido `module` (`fs/promises` é o `promises` de `fs`).
pub(crate) fn member_of(global_object: &JSGlobalObject, module: &str, member: &str) -> HostResult {
    let Resolved::Builtin(entry) = resolve(module) else { return Ok(JSValue::undefined()) };
    let loaded = load(global_object, entry)?;
    get_object_property(global_object, loaded, &Identifier::from_span(global_object.vm(), member.as_bytes()))
}

/// O objeto de `name` guardado para o programa: `build` roda na primeira chamada (serve também aos auxiliares que não
/// são módulos do `require`, como o `node_internal.js`).
pub(crate) fn cached(name: &'static str, build: impl FnOnce() -> HostResult) -> HostResult {
    if let Some(cached) = CACHE.with(|cache| cache.borrow().iter().find(|(known, _)| *known == name).map(|(_, value)| value.clone())) {
        return Ok(cached);
    }
    // Sem empréstimo durante a montagem: ela pode reentrar no `require`.
    let installed = build()?;
    CACHE.with(|cache| cache.borrow_mut().push((name, installed.clone())));
    Ok(installed)
}

/// O que o módulo embutido `owner` entregou ao hospedeiro em `key` ao ser montado (o `require` interno do `stream`, os
/// internos do `util`): carrega `owner` se ainda não foi carregado; `missing` é o erro de quando nada foi entregue.
pub(crate) fn registered_by(global_object: &JSGlobalObject, owner: &str, key: &'static str, missing: impl FnOnce() -> Thrown) -> HostResult {
    let Resolved::Builtin(entry) = resolve(owner) else { unreachable!("{owner} consta em BUILTINS") };
    load(global_object, entry)?;
    cached(key, || Err(missing()))
}

/// Fim do programa (`eval::reset_for_program`): o cache é do programa.
/// As raízes do coletor: os módulos embutidos já criados.
pub(crate) fn visit_gc_roots(marker: &mut crate::runtime::gc::Marker) {
    CACHE.with(|cache| cache.borrow().iter().for_each(|(_, value)| marker.value(*value)));
}

pub(crate) fn reset_for_program() {
    let taken = CACHE.try_with(|cache| std::mem::take(&mut *cache.borrow_mut()));
    drop(taken);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn install_nothing(_: &JSGlobalObject) -> HostResult {
        Ok(JSValue::undefined())
    }

    #[test]
    fn prefix_rules_follow_bun() {
        let table = [
            BuiltinEntry { name: "vm", scheme: Scheme::Both, install: install_nothing },
            BuiltinEntry { name: "test", scheme: Scheme::NodeOnly, install: install_nothing },
        ];
        assert!(matches!(resolve_in(&table, "vm"), Resolved::Builtin(entry) if entry.name == "vm"));
        assert!(matches!(resolve_in(&table, "node:vm"), Resolved::Builtin(entry) if entry.name == "vm"));
        assert!(matches!(resolve_in(&table, "node:test"), Resolved::Builtin(entry) if entry.name == "test"));
        assert!(matches!(resolve_in(&table, "test"), Resolved::NotBuiltin));
        assert!(matches!(resolve_in(&table, "node:nope"), Resolved::UnknownNode));
        assert!(matches!(resolve_in(&table, "node:"), Resolved::UnknownNode));
        assert!(matches!(resolve_in(&table, "./vm"), Resolved::NotBuiltin));
    }
}
