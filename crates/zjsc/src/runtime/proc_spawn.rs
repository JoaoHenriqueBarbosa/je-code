//! Os processos filhos do `Bun.spawn`: as funções nativas do `bun_spawn.js` sobre o [`Host`] (`proc_*`). Os descritores
//! das pontas que ficam com o processo são sem bloqueio; as funções devolvem o resultado ou o errno negativo
//! (`-11` é `EAGAIN`), e o JavaScript decide o que fazer, como no `net_loop`.
//!
//! `procSpawn(pedido)` recebe o pedido num texto só, campos separados por NUL: `arquivo`, `cwd` (vazio herda),
//! `detached` (`0` ou `1`), a contagem e os itens do `argv`, `1` seguido da contagem e dos pares `CHAVE=valor` do ambiente
//! (ou `0` para herdar), a contagem e os códigos do `stdio` (`-1` ignora, `-2` herda, `-3` pipe, `-4` socketpair, `n` o
//! descritor `n`) e responde `[pid, fd0, fd1, ...]` (`-1` onde o processo não guardou ponta).

use crate::host_function;
use crate::runtime::host::{host, ProcExit, SpawnRequest, SpawnStdio};
use crate::runtime::host_call::{HostCall, HostResult};
use crate::runtime::intl_support::to_rust_string;
use crate::runtime::js_array::construct_array;
use crate::runtime::js_global_object::JSGlobalObject;
use crate::runtime::js_value::{js_number, JSValue};
use crate::runtime::native_function::NativeFunction;
use crate::runtime::text_decoder::input_bytes;
use crate::runtime::uint8_array_base64::create_uint8_array;

fn errno_value(errno: i32) -> JSValue {
    js_number(-f64::from(errno))
}

fn numbers(global_object: &JSGlobalObject, values: &[f64]) -> JSValue {
    let values: Vec<JSValue> = values.iter().map(|value| js_number(*value)).collect();
    construct_array(global_object.vm(), &global_object.array_structure(), &values).as_value()
}

/// Lê o pedido de `procSpawn` (o formato está no cabeçalho do módulo).
fn parse_request(text: &str) -> Option<SpawnRequest> {
    let mut fields = text.split('\0');
    let mut next = || fields.next();
    let file = next()?.to_owned();
    let cwd = Some(next()?.to_owned()).filter(|cwd| !cwd.is_empty());
    let detached = next()? == "1";
    let argc: usize = next()?.parse().ok()?;
    let argv = (0..argc).map(|_| next().map(str::to_owned)).collect::<Option<Vec<_>>>()?;
    let env = if next()? == "1" {
        let count: usize = next()?.parse().ok()?;
        let pairs = (0..count)
            .map(|_| next().map(|pair| pair.split_once('=').map_or((pair.to_owned(), String::new()), |(key, value)| (key.to_owned(), value.to_owned()))))
            .collect::<Option<Vec<_>>>()?;
        Some(pairs)
    } else {
        None
    };
    let count: usize = next()?.parse().ok()?;
    let stdio = (0..count)
        .map(|_| {
            next().and_then(|code| code.parse::<i32>().ok()).map(|code| match code {
                -1 => SpawnStdio::Ignore,
                -2 => SpawnStdio::Inherit,
                -3 => SpawnStdio::Pipe,
                -4 => SpawnStdio::SocketPair,
                fd => SpawnStdio::Fd(fd),
            })
        })
        .collect::<Option<Vec<_>>>()?;
    Some(SpawnRequest { file, argv, env, cwd, stdio, detached })
}

fn spawn_body(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    let text = to_rust_string(global_object, call.argument(0))?;
    let Some(request) = parse_request(&text) else { return Ok(errno_value(22)) };
    Ok(match host().proc_spawn(&request) {
        Ok(spawned) => {
            let mut values = vec![f64::from(spawned.pid)];
            values.extend(spawned.parent_fds.iter().map(|fd| fd.map_or(-1.0, f64::from)));
            numbers(global_object, &values)
        }
        Err(errno) => errno_value(errno),
    })
}

/// `procRead(fd, max)`: os bytes lidos (vazio é o fim) ou o errno negativo.
fn read_body(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    match host().fs_read(call.argument(0).to_number() as i32, call.argument(1).to_number() as usize, None) {
        Ok(bytes) => {
            let array = create_uint8_array(global_object, bytes.len())?;
            array.with_vector_mut(|destination| destination[..bytes.len()].copy_from_slice(&bytes));
            Ok(array.as_value())
        }
        Err(errno) => Ok(errno_value(errno)),
    }
}

/// `procWrite(fd, bytes)`: os bytes aceitos ou o errno negativo.
fn write_body(_global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    let bytes = input_bytes(call.argument(1)).unwrap_or_default();
    Ok(host().fs_write(call.argument(0).to_number() as i32, &bytes, None).map_or_else(errno_value, |written| js_number(written as f64)))
}

/// `procKill(pid, sinal)`.
fn kill_body(_global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    Ok(host().kill(call.argument(0).to_number() as i32, call.argument(1).to_number() as i32).map_or_else(errno_value, |()| js_number(0.0)))
}

/// A resposta de uma espera: `[código, sinal]` (um dos dois é `-1`).
pub(crate) fn exit_value(global_object: &JSGlobalObject, exit: ProcExit) -> JSValue {
    match exit {
        ProcExit::Exited(code) => numbers(global_object, &[f64::from(code), -1.0]),
        ProcExit::Signaled(signal) => numbers(global_object, &[-1.0, f64::from(signal)]),
    }
}

/// `procWait(pid, semEspera)`: `[código, sinal]`, `null` com o filho ainda rodando (`semEspera`), ou o errno negativo.
fn wait_body(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    Ok(match host().proc_wait(call.argument(0).to_number() as i32, call.argument(1).to_boolean()) {
        Ok(Some(exit)) => exit_value(global_object, exit),
        Ok(None) => JSValue::null(),
        Err(errno) => errno_value(errno),
    })
}

/// `procPoll(pedido, prazoMs)`: o `poll(2)` bloqueante do `spawnSync`. `pedido` é `fd:r`, `fd:w` ou `fd:rw` separados por
/// vírgula; a resposta tem um número por item, com os bits 1 (leitura), 2 (escrita), 4 (hangup) e 8 (erro).
fn poll_body(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    let text = to_rust_string(global_object, call.argument(0))?;
    let watches: Vec<crate::runtime::host::NetWatch> = text
        .split(',')
        .filter_map(|item| item.split_once(':'))
        .map(|(fd, events)| crate::runtime::host::NetWatch { fd: fd.parse().unwrap_or(-1), read: events.contains('r'), write: events.contains('w') })
        .collect();
    let timeout = call.argument(1).to_number();
    let timeout = (timeout >= 0.0).then(|| std::time::Duration::from_millis(timeout as u64));
    Ok(match host().net_poll(&watches, timeout) {
        Ok(ready) => numbers(
            global_object,
            &ready
                .iter()
                .map(|entry| f64::from(u8::from(entry.readable) | u8::from(entry.writable) << 1 | u8::from(entry.hangup) << 2 | u8::from(entry.error) << 3))
                .collect::<Vec<_>>(),
        ),
        Err(errno) => errno_value(errno),
    })
}

host_function!(spawn_function, spawn_body);
host_function!(read_function, read_body);
host_function!(write_function, write_body);
host_function!(kill_function, kill_body);
host_function!(wait_function, wait_body);
host_function!(poll_function, poll_body);

/// As funções do `bun_spawn.js` que não são de observação de filhos (essas ficam no `net_loop`).
pub(crate) const NATIVES: [(&str, u32, NativeFunction); 6] = [
    ("procSpawn", 1, spawn_function),
    ("procRead", 2, read_function),
    ("procWrite", 2, write_function),
    ("procKill", 2, kill_function),
    ("procWait", 2, wait_function),
    ("procPoll", 2, poll_function),
];

const SOURCE: &str = include_str!("bun_spawn.js");

/// Carrega (uma vez por programa) o `bun_spawn.js` e devolve os exports dele (`spawn`, `spawnSync`).
fn module(global_object: &JSGlobalObject) -> HostResult {
    crate::api::builtin_modules::cached("internal:bun-spawn", || {
        let mut natives: Vec<(&str, u32, NativeFunction)> = crate::runtime::net_loop::NATIVES.to_vec();
        natives.extend(NATIVES);
        let host = crate::api::node_js_host::module_host(global_object, &natives)?;
        Ok(crate::api::node_js_host::run_native_factory(global_object, SOURCE, "internal:bun-spawn", &host))
    })
}

/// Chama o export `name` do `bun_spawn.js` com os argumentos do chamador.
fn call_export(global_object: &JSGlobalObject, name: &str, call: &HostCall) -> HostResult {
    let exports = module(global_object)?;
    let identifier = crate::runtime::identifier::Identifier::from_span(global_object.vm(), name.as_bytes());
    let function = crate::runtime::string_regexp_support::get_object_property(global_object, exports, &identifier)?;
    crate::api::node_js_host::call_value(global_object, function, JSValue::undefined(), call.arguments(), "bun:spawn")
}

/// `Bun.spawn(cmd, opções)` e `Bun.spawn({ cmd, ... })`.
fn bun_spawn_body(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    call_export(global_object, "spawn", call)
}
host_function!(pub bun_spawn, bun_spawn_body);

/// `Bun.spawnSync(cmd, opções)`.
fn bun_spawn_sync_body(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    call_export(global_object, "spawnSync", call)
}
host_function!(pub bun_spawn_sync, bun_spawn_sync_body);
