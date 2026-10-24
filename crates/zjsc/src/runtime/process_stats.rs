//! As medidas do processo no `process` do bun 1.4.2: `hrtime`, `hrtime.bigint`, `memoryUsage`, `memoryUsage.rss`,
//! `cpuUsage` e `resourceUsage`. Os valores vêm do sistema (relógio monotônico desde o nascimento do `process`,
//! `/proc/self/statm`, `/proc/self/stat`, `/proc/self/status`, `/proc/self/io`), com a forma e as validações medidas
//! no bun. `hrtime` conta desde o início do processo, como o bun: o `hrtime.bigint()` fica sempre abaixo de
//! `uptime() * 1e9` lido depois dele.

use std::time::Duration;

use crate::host_function;
use crate::runtime::array_constructor::{is_array, IsArrayCaller};
use crate::runtime::host_call::{HostCall, HostResult};
use crate::runtime::intl_support::prop;
use crate::runtime::iterator_operations::get_value_property;
use crate::runtime::js_big_int_ops::make_big_int_from_i64;
use crate::runtime::js_global_object::JSGlobalObject;
use crate::runtime::js_value::{js_number, JSValue};
use crate::runtime::js_web_assembly::received_description;
use crate::runtime::node_error::{throw_coded_range_error, throw_coded_type_error};
use crate::runtime::object_constructor::construct_empty_object;
use crate::runtime::process_exit::number_text;
use crate::runtime::process_shape::{array_value, elapsed_since_start};
use crate::runtime::array_buffer::live_buffer_bytes;

/// O tamanho da página de memória do Debian x86-64.
const PAGE_BYTES: u64 = 4096;
/// `Number.MAX_SAFE_INTEGER`.
const MAX_SAFE_INTEGER: f64 = 9_007_199_254_740_991.0;
/// Os microssegundos de um tique de `_SC_CLK_TCK` (100 Hz no Linux).
const MICROS_PER_TICK: u64 = 10_000;

/// O campo `index` de `/proc/self/stat` contado a partir do estado (campo 3 do manual), que é o primeiro depois do `)`
/// do nome do comando.
pub(crate) fn stat_field(index: usize) -> Option<u64> {
    let stat = crate::runtime::host::read_text("/proc/self/stat")?;
    stat.rsplit_once(')')?.1.split_whitespace().nth(index)?.parse().ok()
}

/// O número da linha `key:` de `/proc/self/status` (kB ou contagem).
fn status_value(key: &str) -> u64 {
    let status = crate::runtime::host::read_text("/proc/self/status").unwrap_or_default();
    status
        .lines()
        .find_map(|line| line.strip_prefix(key)?.strip_prefix(':'))
        .and_then(|rest| rest.split_whitespace().next()?.parse().ok())
        .unwrap_or(0)
}

/// O número da linha `key:` de `/proc/self/io`.
fn io_value(key: &str) -> u64 {
    let io = crate::runtime::host::read_text("/proc/self/io").unwrap_or_default();
    io.lines().find_map(|line| line.strip_prefix(key)?.strip_prefix(':')?.trim().parse().ok()).unwrap_or(0)
}

/// Os campos `resident` e `shared` (em páginas) de `/proc/self/statm`.
fn statm_pages() -> (u64, u64) {
    let statm = crate::runtime::host::read_text("/proc/self/statm").unwrap_or_default();
    let mut fields = statm.split_whitespace().skip(1).map(|field| field.parse::<u64>().unwrap_or(0));
    (fields.next().unwrap_or(0), fields.next().unwrap_or(0))
}

fn resident_bytes() -> u64 {
    statm_pages().0.max(1) * PAGE_BYTES
}

/// Os microssegundos de CPU de usuário e de sistema do processo.
fn cpu_micros() -> (u64, u64) {
    (stat_field(11).unwrap_or(0) * MICROS_PER_TICK, stat_field(12).unwrap_or(0) * MICROS_PER_TICK)
}

fn named_number(global_object: &JSGlobalObject, object: &crate::runtime::js_object::JSObject, key: &str, value: f64) {
    let vm = global_object.vm();
    object.put_direct(vm, &prop(vm, key), js_number(value), 0);
}

fn element(global_object: &JSGlobalObject, array: JSValue, index: &str) -> Result<f64, crate::runtime::host_call::Thrown> {
    Ok(get_value_property(global_object, array, &prop(global_object.vm(), index))?.to_number())
}

/// A validação do argumento de `hrtime(time)`: um `Array` de tamanho 2, devolvido como os dois números.
fn previous_time(global_object: &JSGlobalObject, time: JSValue) -> Result<(f64, f64), crate::runtime::host_call::Thrown> {
    if !is_array(&time, IsArrayCaller::ArrayIsArray).unwrap_or(false) {
        let message = format!("The \"time\" argument must be an instance of Array. Received {}", received_description(global_object, time));
        return Err(throw_coded_type_error(global_object, &message, "ERR_INVALID_ARG_TYPE"));
    }
    let length = get_value_property(global_object, time, &prop(global_object.vm(), "length"))?.to_number();
    if length != 2.0 {
        let message = format!("The value of \"time\" is out of range. It must be 2. Received {}", number_text(global_object.vm(), length));
        return Err(throw_coded_range_error(global_object, &message, "ERR_OUT_OF_RANGE"));
    }
    Ok((element(global_object, time, "0")?, element(global_object, time, "1")?))
}

/// `process.hrtime([time])`: `[segundos, nanossegundos]` desde o início do processo, ou a diferença para `time`.
fn hrtime_body(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    let elapsed: Duration = elapsed_since_start();
    let mut seconds = elapsed.as_secs() as f64;
    let mut nanos = f64::from(elapsed.subsec_nanos());
    let previous = call.argument(0);
    if !previous.is_undefined() {
        let (previous_seconds, previous_nanos) = previous_time(global_object, previous)?;
        seconds -= previous_seconds;
        nanos -= previous_nanos;
        if nanos < 0.0 {
            seconds -= 1.0;
            nanos += 1e9;
        }
    }
    let pure = |number: f64| if number.is_nan() { f64::NAN } else { number };
    Ok(array_value(global_object, &[js_number(pure(seconds)), js_number(pure(nanos))]))
}
host_function!(pub hrtime, hrtime_body);

/// `process.hrtime.bigint()`: os nanossegundos desde o início do processo, como `BigInt`.
fn hrtime_bigint_body(_global_object: &JSGlobalObject, _call: &HostCall) -> HostResult {
    Ok(make_big_int_from_i64(elapsed_since_start().as_nanos().min(i64::MAX as u128) as i64))
}
host_function!(pub hrtime_bigint, hrtime_bigint_body);

/// `process.memoryUsage()`: `rss` do `statm`, o monte como as páginas anônimas residentes (o bun mostra `heapTotal` igual a
/// `heapUsed`), e `external`/`arrayBuffers` como os bytes dos `ArrayBuffer` vivos.
fn memory_usage_body(global_object: &JSGlobalObject, _call: &HostCall) -> HostResult {
    let (resident, shared) = statm_pages();
    let heap = (resident.saturating_sub(shared) * PAGE_BYTES / 4).max(PAGE_BYTES);
    let buffers = live_buffer_bytes() as f64;
    let usage = construct_empty_object(global_object);
    for (key, value) in [
        ("rss", resident_bytes() as f64),
        ("heapTotal", heap as f64),
        ("heapUsed", heap as f64),
        ("external", buffers),
        ("arrayBuffers", buffers),
    ] {
        named_number(global_object, &usage, key, value);
    }
    Ok(usage.as_value())
}
host_function!(pub memory_usage, memory_usage_body);

/// `process.memoryUsage.rss()`.
fn memory_rss_body(_global_object: &JSGlobalObject, _call: &HostCall) -> HostResult {
    Ok(js_number(resident_bytes() as f64))
}
host_function!(pub memory_rss, memory_rss_body);

/// Valida a propriedade `key` de `prevValue` em `cpuUsage`: número, entre 0 e `MAX_SAFE_INTEGER`.
fn previous_cpu_field(global_object: &JSGlobalObject, previous: JSValue, key: &str) -> Result<f64, crate::runtime::host_call::Thrown> {
    let vm = global_object.vm();
    let value = get_value_property(global_object, previous, &prop(vm, key))?;
    if !value.is_number() {
        let message = format!("The \"prevValue.{key}\" property must be of type number. Received {}", received_description(global_object, value));
        return Err(throw_coded_type_error(global_object, &message, "ERR_INVALID_ARG_TYPE"));
    }
    let number = value.as_number();
    if !(0.0..=MAX_SAFE_INTEGER).contains(&number) {
        let message = format!("The property 'prevValue.{key}' is invalid. Received {}", number_text(vm, number));
        return Err(throw_coded_range_error(global_object, &message, "ERR_INVALID_ARG_VALUE"));
    }
    Ok(number)
}

/// `process.cpuUsage([prevValue])`: `{user, system}` em microssegundos, ou a diferença para `prevValue`.
fn cpu_usage_body(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    let (mut user, mut system) = {
        let (user, system) = cpu_micros();
        (user as f64, system as f64)
    };
    let previous = call.argument(0);
    if !previous.is_undefined() {
        if !previous.is_object() {
            let message = format!("The \"prevValue\" argument must be of type object. Received {}", received_description(global_object, previous));
            return Err(throw_coded_type_error(global_object, &message, "ERR_INVALID_ARG_TYPE"));
        }
        user -= previous_cpu_field(global_object, previous, "user")?;
        system -= previous_cpu_field(global_object, previous, "system")?;
    }
    let usage = construct_empty_object(global_object);
    named_number(global_object, &usage, "user", user);
    named_number(global_object, &usage, "system", system);
    Ok(usage.as_value())
}
host_function!(pub cpu_usage, cpu_usage_body);

/// `process.resourceUsage()`: o `getrusage` do processo, lido de `/proc` (tempos em microssegundos, `maxRSS` em kB).
fn resource_usage_body(global_object: &JSGlobalObject, _call: &HostCall) -> HostResult {
    let (user, system) = cpu_micros();
    let usage = construct_empty_object(global_object);
    for (key, value) in [
        ("userCPUTime", user),
        ("systemCPUTime", system),
        ("maxRSS", status_value("VmHWM")),
        ("sharedMemorySize", 0),
        ("unsharedDataSize", 0),
        ("unsharedStackSize", 0),
        ("minorPageFault", stat_field(7).unwrap_or(0)),
        ("majorPageFault", stat_field(9).unwrap_or(0)),
        ("swappedOut", 0),
        ("fsRead", io_value("read_bytes") / 512),
        ("fsWrite", io_value("write_bytes") / 512),
        ("ipcSent", 0),
        ("ipcReceived", 0),
        ("signalsCount", 0),
        ("voluntaryContextSwitches", status_value("voluntary_ctxt_switches")),
        ("involuntaryContextSwitches", status_value("nonvoluntary_ctxt_switches")),
    ] {
        named_number(global_object, &usage, key, value as f64);
    }
    Ok(usage.as_value())
}
host_function!(pub resource_usage, resource_usage_body);
