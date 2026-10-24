//! O módulo `node:tty` do bun 1.4.2: o `tty.ts` do bun (gerado em `api/node_tty.js` por `scripts/gen-node-tty.js`) sobre
//! os terminais do `Host`. O `$cpp("ProcessBindingTTYWrap.cpp", "createBunTTYFunctions")` do bun é o objeto `host.tty`
//! montado aqui: `isatty`, `getWindowSize`, `setRawMode` e `rawModeStateSize`.
//!
//! Medido no oráculo (caso `module-tty`): `isatty` de qualquer valor que não é inteiro de 0 a 2147483647 dá `false` (a
//! string `"1"` também), `isatty.length` é 0 e o nome é `isatty`.

use crate::api::eval::key_of;
use crate::host_function;
use crate::runtime::host::host;
use crate::runtime::host_call::{HostCall, HostResult};
use crate::runtime::js_global_object::JSGlobalObject;
use crate::runtime::js_value::{js_boolean, js_number, JSValue};
use crate::runtime::object_constructor::construct_empty_object;

/// O texto da fábrica de `node:tty`.
const TTY: &str = include_str!("../api/node_tty.js");

/// O descritor de um argumento que é inteiro de 0 a `i32::MAX` (número de verdade, sem conversão).
fn descriptor(value: JSValue) -> Option<i32> {
    (value.is_number() && value.as_number().fract() == 0.0 && (0.0..=f64::from(i32::MAX)).contains(&value.as_number())).then(|| value.as_number() as i32)
}

fn isatty_body(_global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    Ok(js_boolean(descriptor(call.argument(0)).is_some_and(|fd| host().tty_isatty(fd))))
}
host_function!(isatty_function, isatty_body);

/// `getWindowSize(fd, [colunas, linhas])`: preenche a lista e devolve `true`, ou `false` sem terminal.
fn get_window_size_body(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    let Some((columns, rows)) = descriptor(call.argument(0)).and_then(|fd| host().tty_window_size(fd).ok()) else { return Ok(js_boolean(false)) };
    let Some(list) = call.argument(1).to_object(global_object) else { return Ok(js_boolean(false)) };
    let vm = global_object.vm();
    let stored = list.put_by_index(vm, 0, js_number(f64::from(columns)), false).is_ok() && list.put_by_index(vm, 1, js_number(f64::from(rows)), false).is_ok();
    Ok(js_boolean(stored))
}
host_function!(get_window_size_function, get_window_size_body);

/// `setRawMode(fd, bruto, estado)`: 0, ou o errno que o terminal devolveu (o estado do bun é do hospedeiro aqui).
fn set_raw_mode_body(_global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    let Some(fd) = descriptor(call.argument(0)) else { return Ok(js_number(f64::from(crate::runtime::host::ENOTTY))) };
    let errno = host().tty_set_raw_mode(fd, call.argument(1).to_boolean()).err().unwrap_or(0);
    Ok(js_number(f64::from(errno)))
}
host_function!(set_raw_mode_function, set_raw_mode_body);

/// Instalador de `node:tty`.
pub(crate) fn install_tty_module(global_object: &JSGlobalObject) -> HostResult {
    let host_object = crate::api::node_js_host::module_host(global_object, &[])?;
    let vm = global_object.vm();
    let tty = construct_empty_object(global_object);
    crate::api::node_js_host::put_functions(
        global_object,
        &tty,
        &[("setRawMode", 3, set_raw_mode_function), ("isatty", 0, isatty_function), ("getWindowSize", 2, get_window_size_function)],
    );
    // O tamanho do estado por fluxo no bun; aqui o estado fica no hospedeiro, e o número só existe para a desestruturação.
    tty.put_direct(vm, &key_of(vm, "rawModeStateSize"), js_number(0.0), 0);
    host_object.put_direct(vm, &key_of(vm, "tty"), tty.as_value(), 0);
    Ok(crate::api::node_js_host::run_factory(global_object, TTY, "tty", &host_object))
}
