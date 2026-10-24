//! O módulo `node:os` do bun 1.4.2.
//!
//! As chaves e a ordem foram medidas com `Object.keys(require("os"))`. Os valores que dependem da máquina saem do mesmo
//! lugar que o resto do porte lê (`process_system.rs` consulta `/proc/{pid}` pelo `Host`): `/proc/sys/kernel/*` para
//! `hostname`, `type`, `release` e `version`, `/proc/uptime`, `/proc/loadavg`, `/proc/meminfo`, `/proc/cpuinfo`,
//! `/proc/stat`, `/proc/self/status` e `/etc/passwd` para o usuário, e o ambiente do programa (`process_env`) para
//! `HOME` e `TMPDIR`. Dentro do pseudo-linus esses arquivos são os do sistema simulado, então nada aqui denuncia o host.
//!
//! Erros medidos: `setPriority()` sem argumentos lança `TypeError ERR_MISSING_ARGS "Not enough arguments"`; prioridade
//! que não é número lança `ERR_INVALID_ARG_TYPE` e fora de -20..19 lança `RangeError ERR_OUT_OF_RANGE`; processo
//! inexistente lança o `SystemError ERR_SYSTEM_ERROR` `A system error occurred: uv_os_getpriority returned ESRCH (no such
//! process)`; `userInfo(1)` lança `TypeError: Type error`.

use crate::runtime::host::{host, read_text};

use crate::host_function;
use crate::runtime::host_call::{HostCall, HostResult, Thrown};
use crate::runtime::identifier::Identifier;
use crate::runtime::implementation_visibility::ImplementationVisibility;
use crate::runtime::intrinsic::Intrinsic;
use crate::runtime::js_function::put_direct_native_function_with_display_name;
use crate::runtime::js_global_object::JSGlobalObject;
use crate::runtime::js_module_loader::rust_string;
use crate::runtime::js_value::{js_boolean, js_number, JSValue};
use crate::runtime::iterator_operations::get_value_property;
use crate::runtime::js_custom_accessor_function::create_host_custom_accessor_getter_function;
use crate::runtime::js_getter_setter::GetterSetter;
use crate::runtime::property_attribute::ACCESSOR;
use crate::runtime::js_web_assembly::received_description;
use crate::runtime::native_class_support::property_key;
use crate::runtime::node_error::{throw_coded_range_error, throw_coded_type_error, throw_error_with_properties, throw_native_type_error, NetworkProperty};
use crate::runtime::object_constructor::{construct_array_of, construct_empty_object};
use crate::runtime::process_env::environment_variable;
use crate::runtime::process_shape::text_value;
use crate::wtf::text::wtf_string::String as WtfString;

/// Os tiques de `/proc/stat` são `USER_HZ` = 100 por segundo; o `os.cpus()` devolve milissegundos.
const MILLISECONDS_PER_TICK: f64 = 10.0;

/// `-20..=19`, a faixa de `nice`.
const PRIORITY_RANGE: std::ops::RangeInclusive<i32> = -20..=19;

/// `os.constants.errno`, na ordem do bun (alfabética).
const ERRNO: &[(&str, i32)] = &[
    ("E2BIG", 7), ("EACCES", 13), ("EADDRINUSE", 98), ("EADDRNOTAVAIL", 99), ("EAFNOSUPPORT", 97), ("EAGAIN", 11),
    ("EALREADY", 114), ("EBADF", 9), ("EBADMSG", 74), ("EBUSY", 16), ("ECANCELED", 125), ("ECHILD", 10),
    ("ECONNABORTED", 103), ("ECONNREFUSED", 111), ("ECONNRESET", 104), ("EDEADLK", 35), ("EDESTADDRREQ", 89), ("EDOM", 33),
    ("EDQUOT", 122), ("EEXIST", 17), ("EFAULT", 14), ("EFBIG", 27), ("EHOSTUNREACH", 113), ("EIDRM", 43), ("EILSEQ", 84),
    ("EINPROGRESS", 115), ("EINTR", 4), ("EINVAL", 22), ("EIO", 5), ("EISCONN", 106), ("EISDIR", 21), ("ELOOP", 40),
    ("EMFILE", 24), ("EMLINK", 31), ("EMSGSIZE", 90), ("EMULTIHOP", 72), ("ENAMETOOLONG", 36), ("ENETDOWN", 100),
    ("ENETRESET", 102), ("ENETUNREACH", 101), ("ENFILE", 23), ("ENOBUFS", 105), ("ENODATA", 61), ("ENODEV", 19),
    ("ENOENT", 2), ("ENOEXEC", 8), ("ENOLCK", 37), ("ENOLINK", 67), ("ENOMEM", 12), ("ENOMSG", 42), ("ENOPROTOOPT", 92),
    ("ENOSPC", 28), ("ENOSR", 63), ("ENOSTR", 60), ("ENOSYS", 38), ("ENOTCONN", 107), ("ENOTDIR", 20), ("ENOTEMPTY", 39),
    ("ENOTSOCK", 88), ("ENOTSUP", 95), ("ENOTTY", 25), ("ENXIO", 6), ("EOPNOTSUPP", 95), ("EOVERFLOW", 75), ("EPERM", 1),
    ("EPIPE", 32), ("EPROTO", 71), ("EPROTONOSUPPORT", 93), ("EPROTOTYPE", 91), ("ERANGE", 34), ("EROFS", 30),
    ("ESPIPE", 29), ("ESRCH", 3), ("ESTALE", 116), ("ETIME", 62), ("ETIMEDOUT", 110), ("ETXTBSY", 26), ("EWOULDBLOCK", 11),
    ("EXDEV", 18),
];

/// `os.constants.signals`: a ordem do bun põe `SIGCHLD` (17) antes de `SIGSTKFLT` (16).
const SIGNALS: &[(&str, i32)] = &[
    ("SIGHUP", 1), ("SIGINT", 2), ("SIGQUIT", 3), ("SIGILL", 4), ("SIGTRAP", 5), ("SIGABRT", 6), ("SIGIOT", 6), ("SIGBUS", 7),
    ("SIGFPE", 8), ("SIGKILL", 9), ("SIGUSR1", 10), ("SIGSEGV", 11), ("SIGUSR2", 12), ("SIGPIPE", 13), ("SIGALRM", 14),
    ("SIGTERM", 15), ("SIGCHLD", 17), ("SIGSTKFLT", 16), ("SIGCONT", 18), ("SIGSTOP", 19), ("SIGTSTP", 20), ("SIGTTIN", 21),
    ("SIGTTOU", 22), ("SIGURG", 23), ("SIGXCPU", 24), ("SIGXFSZ", 25), ("SIGVTALRM", 26), ("SIGPROF", 27), ("SIGWINCH", 28),
    ("SIGIO", 29), ("SIGPOLL", 29), ("SIGPWR", 30), ("SIGSYS", 31),
];

const PRIORITY: &[(&str, i32)] = &[
    ("PRIORITY_LOW", 19), ("PRIORITY_BELOW_NORMAL", 10), ("PRIORITY_NORMAL", 0), ("PRIORITY_ABOVE_NORMAL", -7),
    ("PRIORITY_HIGH", -14), ("PRIORITY_HIGHEST", -20),
];

const DLOPEN: &[(&str, i32)] = &[("RTLD_LAZY", 1), ("RTLD_NOW", 2), ("RTLD_GLOBAL", 256), ("RTLD_LOCAL", 0), ("RTLD_DEEPBIND", 8)];

fn kernel_value(name: &str) -> String {
    read_text(&format!("/proc/sys/kernel/{name}")).unwrap_or_default()
}

/// O número depois de `key:` em um arquivo no formato de `/proc/meminfo` (valores em kB).
fn meminfo_bytes(key: &str) -> Option<f64> {
    let text = read_text("/proc/meminfo")?;
    let line = text.lines().find(|line| line.strip_prefix(key).is_some_and(|rest| rest.starts_with(':')))?;
    let kilobytes: f64 = line.split_whitespace().nth(1)?.parse().ok()?;
    Some(kilobytes * 1024.0)
}

/// Um objeto sem protótipo, como os de `os.constants`.
fn null_prototype_object(global_object: &JSGlobalObject) -> crate::runtime::js_object::JSObjectRef {
    let object = construct_empty_object(global_object);
    object.set_prototype_direct(global_object.vm(), JSValue::null());
    object
}

fn constants_table(global_object: &JSGlobalObject, entries: &[(&str, i32)]) -> JSValue {
    let vm = global_object.vm();
    let object = null_prototype_object(global_object);
    for (name, value) in entries {
        object.put_direct(vm, &property_key(vm, name), js_number(*value), 0);
    }
    object.as_value()
}

fn build_constants(global_object: &JSGlobalObject) -> JSValue {
    let vm = global_object.vm();
    let constants = null_prototype_object(global_object);
    constants.put_direct(vm, &property_key(vm, "UV_UDP_REUSEADDR"), js_number(4), 0);
    for (name, entries) in [("dlopen", DLOPEN), ("errno", ERRNO), ("signals", SIGNALS), ("priority", PRIORITY)] {
        constants.put_direct(vm, &property_key(vm, name), constants_table(global_object, entries), 0);
    }
    constants.as_value()
}

fn hostname_body(global_object: &JSGlobalObject, _call: &HostCall) -> HostResult {
    Ok(text_value(global_object.vm(), &kernel_value("hostname")))
}
host_function!(hostname_function, hostname_body);

fn type_body(global_object: &JSGlobalObject, _call: &HostCall) -> HostResult {
    Ok(text_value(global_object.vm(), &kernel_value("ostype")))
}
host_function!(type_function, type_body);

fn release_body(global_object: &JSGlobalObject, _call: &HostCall) -> HostResult {
    Ok(text_value(global_object.vm(), &kernel_value("osrelease")))
}
host_function!(release_function, release_body);

fn version_body(global_object: &JSGlobalObject, _call: &HostCall) -> HostResult {
    Ok(text_value(global_object.vm(), &kernel_value("version")))
}
host_function!(version_function, version_body);

/// `process.arch` do porte é `x64`; a máquina é a `x86_64` do `uname -m` do Debian amd64.
fn machine_body(global_object: &JSGlobalObject, _call: &HostCall) -> HostResult {
    Ok(text_value(global_object.vm(), "x86_64"))
}
host_function!(machine_function, machine_body);

fn eol_body(global_object: &JSGlobalObject, _call: &HostCall) -> HostResult {
    Ok(text_value(global_object.vm(), "\n"))
}
host_function!(eol_function, eol_body);

fn arch_body(global_object: &JSGlobalObject, _call: &HostCall) -> HostResult {
    Ok(text_value(global_object.vm(), "x64"))
}
host_function!(arch_function, arch_body);

fn platform_body(global_object: &JSGlobalObject, _call: &HostCall) -> HostResult {
    Ok(text_value(global_object.vm(), "linux"))
}
host_function!(platform_function, platform_body);

fn endianness_body(global_object: &JSGlobalObject, _call: &HostCall) -> HostResult {
    Ok(text_value(global_object.vm(), "LE"))
}
host_function!(endianness_function, endianness_body);

/// Uma linha de `/etc/passwd`: `(nome, uid, gid, home, shell)`.
struct PasswdEntry {
    name: String,
    uid: u32,
    gid: u32,
    home: String,
    shell: String,
}

fn passwd_entry(uid: u32) -> Option<PasswdEntry> {
    read_text("/etc/passwd")?.lines().find_map(|line| {
        let fields: Vec<&str> = line.split(':').collect();
        if fields.len() < 7 || fields[2].parse::<u32>().ok()? != uid {
            return None;
        }
        Some(PasswdEntry { name: fields[0].to_string(), uid, gid: fields[3].parse().ok()?, home: fields[5].to_string(), shell: fields[6].to_string() })
    })
}

/// O uid efetivo do programa, de `/proc/self/status`.
fn current_uid() -> Option<u32> {
    let status = read_text("/proc/self/status")?;
    status.lines().find_map(|line| line.strip_prefix("Uid:"))?.split_whitespace().nth(1)?.parse().ok()
}

fn homedir_body(global_object: &JSGlobalObject, _call: &HostCall) -> HostResult {
    let home = environment_variable("HOME").filter(|home| !home.is_empty()).or_else(|| passwd_entry(current_uid()?).map(|entry| entry.home)).unwrap_or_default();
    Ok(text_value(global_object.vm(), &home))
}
host_function!(homedir_function, homedir_body);

fn tmpdir_body(global_object: &JSGlobalObject, _call: &HostCall) -> HostResult {
    let chosen = ["TMPDIR", "TMP", "TEMP"].iter().find_map(|name| environment_variable(name).filter(|value| !value.is_empty())).unwrap_or_else(|| "/tmp".to_string());
    let trimmed = if chosen.len() > 1 { chosen.trim_end_matches('/').to_string() } else { chosen };
    Ok(text_value(global_object.vm(), if trimmed.is_empty() { "/" } else { &trimmed }))
}
host_function!(tmpdir_function, tmpdir_body);

/// `os.userInfo([options])`: `homedir`, `username`, `shell`, `uid`, `gid`, do `/etc/passwd` do uid corrente.
fn user_info_body(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    let options = call.argument(0);
    if !options.is_undefined_or_null() && !options.is_object() {
        return Err(throw_native_type_error(global_object, "Type error"));
    }
    let vm = global_object.vm();
    let uid = current_uid().unwrap_or(0);
    let Some(entry) = passwd_entry(uid) else {
        let properties = [
            ("name", NetworkProperty::Text("SystemError")),
            ("code", NetworkProperty::Text("ERR_SYSTEM_ERROR")),
            ("errno", NetworkProperty::Number(-2)),
            ("syscall", NetworkProperty::Text("uv_os_get_passwd")),
        ];
        return Err(throw_error_with_properties(global_object, "A system error occurred: uv_os_get_passwd returned ENOENT (no such file or directory)", &properties));
    };
    let object = construct_empty_object(global_object);
    object.put_direct(vm, &property_key(vm, "homedir"), text_value(vm, &entry.home), 0);
    object.put_direct(vm, &property_key(vm, "username"), text_value(vm, &entry.name), 0);
    object.put_direct(vm, &property_key(vm, "shell"), text_value(vm, &entry.shell), 0);
    object.put_direct(vm, &property_key(vm, "uid"), js_number(entry.uid), 0);
    object.put_direct(vm, &property_key(vm, "gid"), js_number(entry.gid), 0);
    Ok(object.as_value())
}
host_function!(user_info_function, user_info_body);

/// As linhas `cpuN ...` de `/proc/stat`, como listas de números (sem o nome).
fn cpu_stat_lines() -> Vec<Vec<f64>> {
    read_text("/proc/stat")
        .map(|text| {
            text.lines()
                .filter(|line| line.strip_prefix("cpu").is_some_and(|rest| rest.starts_with(|c: char| c.is_ascii_digit())))
                .map(|line| line.split_whitespace().skip(1).filter_map(|field| field.parse().ok()).collect())
                .collect()
        })
        .unwrap_or_default()
}

/// O valor de `key` na `index`-ésima ocorrência em `/proc/cpuinfo`.
fn cpuinfo_value(key: &str, index: usize) -> Option<String> {
    let text = read_text("/proc/cpuinfo")?;
    text.lines().filter_map(|line| line.split_once(':').filter(|(name, _)| name.trim() == key)).nth(index).map(|(_, value)| value.trim().to_string())
}

/// `toJSON()` de cada item de `cpus()`: no bun devolve `{times, model, speed}` (nesta ordem, sem o próprio `toJSON`).
fn cpu_to_json_body(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    let vm = global_object.vm();
    let this_value = call.this_value();
    let result = construct_empty_object(global_object);
    for name in ["times", "model", "speed"] {
        let key = property_key(vm, name);
        let value = get_value_property(global_object, this_value, &key)?;
        result.put_direct(vm, &key, value, 0);
    }
    Ok(result.as_value())
}
host_function!(cpu_to_json_function, cpu_to_json_body);

fn cpus_body(global_object: &JSGlobalObject, _call: &HostCall) -> HostResult {
    let vm = global_object.vm();
    let mut entries = Vec::new();
    for (index, ticks) in cpu_stat_lines().iter().enumerate() {
        let tick = |position: usize| js_number(ticks.get(position).copied().unwrap_or(0.0) * MILLISECONDS_PER_TICK);
        let times = construct_empty_object(global_object);
        // user, nice, system, idle, iowait, irq: o `irq` do libuv é o sexto campo.
        for (name, position) in [("user", 0), ("nice", 1), ("sys", 2), ("idle", 3), ("irq", 5)] {
            times.put_direct(vm, &property_key(vm, name), tick(position), 0);
        }
        let model = cpuinfo_value("model name", index).unwrap_or_default();
        let speed: f64 = cpuinfo_value("cpu MHz", index).and_then(|mhz| mhz.parse::<f64>().ok()).map_or(0.0, f64::trunc);
        let cpu = construct_empty_object(global_object);
        cpu.put_direct(vm, &property_key(vm, "model"), text_value(vm, &model), 0);
        cpu.put_direct(vm, &property_key(vm, "speed"), js_number(speed), 0);
        cpu.put_direct(vm, &property_key(vm, "times"), times.as_value(), 0);
        put_direct_native_function_with_display_name(
            vm,
            global_object,
            &cpu,
            &Identifier::from_span(vm, b"toJSON"),
            &WtfString::from_latin1(b"toJSON"),
            0,
            cpu_to_json_function,
            ImplementationVisibility::Public,
            Intrinsic::NoIntrinsic,
            0,
        );
        entries.push(cpu.as_value());
    }
    Ok(construct_array_of(global_object, &entries).as_value())
}
host_function!(cpus_function, cpus_body);

/// Quantos CPUs a lista de `Cpus_allowed_list` (`0-3,8,10-11`) nomeia.
fn count_cpu_list(list: &str) -> Option<usize> {
    let mut total = 0usize;
    for part in list.split(',').filter(|part| !part.trim().is_empty()) {
        match part.trim().split_once('-') {
            Some((first, last)) => total += last.parse::<usize>().ok()?.checked_sub(first.parse::<usize>().ok()?)? + 1,
            None => {
                part.trim().parse::<usize>().ok()?;
                total += 1;
            }
        }
    }
    Some(total)
}

/// O `uv_available_parallelism`: a máscara de afinidade (`sched_getaffinity`), lida de `Cpus_allowed_list` em
/// `/proc/self/status`; sem ela, os `cpuN` de `/proc/stat`.
fn available_parallelism_body(_global_object: &JSGlobalObject, _call: &HostCall) -> HostResult {
    let allowed = read_text("/proc/self/status")
        .and_then(|status| status.lines().find_map(|line| line.strip_prefix("Cpus_allowed_list:").map(|rest| rest.trim().to_string())))
        .and_then(|list| count_cpu_list(&list))
        .filter(|count| *count > 0);
    Ok(js_number(allowed.unwrap_or_else(|| cpu_stat_lines().len().max(1)) as f64))
}
host_function!(available_parallelism_function, available_parallelism_body);

fn totalmem_body(_global_object: &JSGlobalObject, _call: &HostCall) -> HostResult {
    Ok(js_number(meminfo_bytes("MemTotal").unwrap_or(0.0)))
}
host_function!(totalmem_function, totalmem_body);

/// O libuv devolve `MemAvailable` (o que cabe ser alocado sem trocar), não `MemFree`.
fn freemem_body(_global_object: &JSGlobalObject, _call: &HostCall) -> HostResult {
    Ok(js_number(meminfo_bytes("MemAvailable").or_else(|| meminfo_bytes("MemFree")).unwrap_or(0.0)))
}
host_function!(freemem_function, freemem_body);

fn uptime_body(_global_object: &JSGlobalObject, _call: &HostCall) -> HostResult {
    let seconds: f64 = read_text("/proc/uptime").and_then(|text| text.split_whitespace().next()?.parse().ok()).unwrap_or(0.0);
    Ok(js_number(seconds))
}
host_function!(uptime_function, uptime_body);

fn loadavg_body(global_object: &JSGlobalObject, _call: &HostCall) -> HostResult {
    let text = read_text("/proc/loadavg").unwrap_or_default();
    let mut averages: Vec<JSValue> = text.split_whitespace().take(3).map(|field| js_number(field.parse::<f64>().unwrap_or(0.0))).collect();
    averages.resize(3, js_number(0.0));
    Ok(construct_array_of(global_object, &averages).as_value())
}
host_function!(loadavg_function, loadavg_body);

/// Um endereço de interface, como `os.networkInterfaces()` o mostra.
struct InterfaceAddress {
    address: String,
    netmask: String,
    ipv6: bool,
    prefix: u32,
    scope_id: u32,
}

/// `inet_ntop` de IPv6: o maior trecho de zeros (de 2 grupos para cima, o primeiro em empate) vira `::`.
fn format_ipv6(groups: [u16; 8]) -> String {
    let (mut best_start, mut best_len, mut start, mut len) = (0usize, 0usize, 0usize, 0usize);
    for (index, group) in groups.iter().enumerate() {
        if *group == 0 {
            if len == 0 {
                start = index;
            }
            len += 1;
            if len > best_len {
                best_start = start;
                best_len = len;
            }
        } else {
            len = 0;
        }
    }
    if best_len < 2 {
        return groups.iter().map(|group| format!("{group:x}")).collect::<Vec<_>>().join(":");
    }
    let head: Vec<String> = groups[..best_start].iter().map(|group| format!("{group:x}")).collect();
    let tail: Vec<String> = groups[best_start + best_len..].iter().map(|group| format!("{group:x}")).collect();
    format!("{}::{}", head.join(":"), tail.join(":"))
}

fn ipv6_from_hex(hex: &str) -> Option<[u16; 8]> {
    if hex.len() != 32 {
        return None;
    }
    let mut groups = [0u16; 8];
    for (index, group) in groups.iter_mut().enumerate() {
        *group = u16::from_str_radix(&hex[index * 4..index * 4 + 4], 16).ok()?;
    }
    Some(groups)
}

fn ipv6_netmask(prefix: u32) -> String {
    let mut groups = [0u16; 8];
    for (index, group) in groups.iter_mut().enumerate() {
        let bits = prefix.saturating_sub(index as u32 * 16).min(16);
        *group = if bits == 0 { 0 } else { (u32::from(u16::MAX) << (16 - bits)) as u16 };
    }
    format_ipv6(groups)
}

fn ipv4_netmask(prefix: u32) -> String {
    let mask = if prefix == 0 { 0 } else { u32::MAX << (32 - prefix.min(32)) };
    let octets = mask.to_be_bytes();
    format!("{}.{}.{}.{}", octets[0], octets[1], octets[2], octets[3])
}

/// Os nomes de `/proc/net/dev`, na ordem do arquivo (a do `getifaddrs`).
fn interface_names() -> Vec<String> {
    read_text("/proc/net/dev")
        .map(|text| text.lines().skip(2).filter_map(|line| line.split_once(':').map(|(name, _)| name.trim().to_string())).collect())
        .unwrap_or_default()
}

/// As linhas de `/proc/net/route`: `(interface, destino, máscara)`, com os octetos em ordem de rede.
fn route_entries() -> Vec<(String, u32, u32)> {
    let Some(text) = read_text("/proc/net/route") else { return Vec::new() };
    text.lines()
        .skip(1)
        .filter_map(|line| {
            let fields: Vec<&str> = line.split_whitespace().collect();
            let destination = u32::from_str_radix(fields.get(1)?, 16).ok()?;
            let mask = u32::from_str_radix(fields.get(7)?, 16).ok()?;
            Some((fields[0].to_string(), u32::from_be_bytes(destination.to_le_bytes()), u32::from_be_bytes(mask.to_le_bytes())))
        })
        .collect()
}

/// Os endereços IPv4 locais de `/proc/net/fib_trie` (seção `Local:`, entradas `/32 host LOCAL`) e as redes
/// `host LOCAL` ou `link UNICAST` mais curtas que /32, como `(endereço, prefixo)`.
fn fib_trie_addresses() -> (Vec<u32>, Vec<(u32, u32)>) {
    let (mut hosts, mut networks) = (Vec::new(), Vec::new());
    let Some(text) = read_text("/proc/net/fib_trie") else { return (hosts, networks) };
    let mut in_local = false;
    let mut current: Option<u32> = None;
    for line in text.lines() {
        let trimmed = line.trim();
        if !line.starts_with(' ') {
            in_local = trimmed == "Local:";
            current = None;
        } else if let Some(address) = trimmed.strip_prefix("|-- ") {
            current = address.parse::<core::net::Ipv4Addr>().ok().map(u32::from);
        } else if in_local {
            let Some(address) = current else { continue };
            let Some((prefix, kind)) = trimmed.strip_prefix('/').and_then(|rest| rest.split_once(' ')) else { continue };
            let Ok(prefix) = prefix.parse::<u32>() else { continue };
            if prefix == 32 && kind == "host LOCAL" {
                if !hosts.contains(&address) {
                    hosts.push(address);
                }
            } else if prefix < 32 && (kind == "host LOCAL" || kind == "link UNICAST") {
                networks.push((address, prefix));
            }
        }
    }
    (hosts, networks)
}

fn prefix_of_mask(mask: u32) -> u32 {
    mask.leading_ones()
}

/// A interface e o prefixo de um endereço IPv4: loopback é `lo`; os demais acham a interface pela rota que os contém.
fn locate_ipv4(address: u32, networks: &[(u32, u32)], routes: &[(String, u32, u32)]) -> Option<(String, u32)> {
    let contains = |network: u32, mask: u32| mask != 0 && address & mask == network & mask;
    if address >> 24 == 127 {
        let prefix = networks.iter().filter(|(network, prefix)| *prefix > 0 && contains(*network, u32::MAX << (32 - prefix))).map(|(_, prefix)| *prefix).max().unwrap_or(8);
        return Some(("lo".to_string(), prefix));
    }
    routes.iter().filter(|(_, destination, mask)| contains(*destination, *mask)).max_by_key(|(_, _, mask)| prefix_of_mask(*mask)).map(|(name, _, mask)| (name.clone(), prefix_of_mask(*mask)))
}

/// `os.networkInterfaces()`: nomes de `/proc/net/dev`, IPv4 de `/proc/net/fib_trie` com as rotas de `/proc/net/route`,
/// IPv6 de `/proc/net/if_inet6`, MAC de `/sys/class/net/NOME/address` (zeros se não existir, como o `lo`).
fn network_interfaces_body(global_object: &JSGlobalObject, _call: &HostCall) -> HostResult {
    let vm = global_object.vm();
    let (hosts, networks) = fib_trie_addresses();
    let routes = route_entries();
    let inet6 = read_text("/proc/net/if_inet6").unwrap_or_default();
    let result = construct_empty_object(global_object);
    for name in interface_names() {
        let mut addresses: Vec<InterfaceAddress> = hosts
            .iter()
            .filter_map(|address| {
                let (interface, prefix) = locate_ipv4(*address, &networks, &routes)?;
                (interface == name).then(|| InterfaceAddress {
                    address: core::net::Ipv4Addr::from(*address).to_string(),
                    netmask: ipv4_netmask(prefix),
                    ipv6: false,
                    prefix,
                    scope_id: 0,
                })
            })
            .collect();
        for line in inet6.lines() {
            let fields: Vec<&str> = line.split_whitespace().collect();
            if fields.len() < 6 || fields[5] != name {
                continue;
            }
            let (Some(groups), Ok(index), Ok(prefix), Ok(scope)) = (ipv6_from_hex(fields[0]), u32::from_str_radix(fields[1], 16), u32::from_str_radix(fields[2], 16), u32::from_str_radix(fields[3], 16)) else {
                continue;
            };
            // O libuv só preenche o `scope_id` do sockaddr dos endereços de escopo link (0x20).
            addresses.push(InterfaceAddress { address: format_ipv6(groups), netmask: ipv6_netmask(prefix), ipv6: true, prefix, scope_id: if scope == 0x20 { index } else { 0 } });
        }
        if addresses.is_empty() {
            continue;
        }
        let mac = read_text(&format!("/sys/class/net/{name}/address")).filter(|mac| !mac.is_empty()).unwrap_or_else(|| "00:00:00:00:00:00".to_string());
        let internal = name == "lo";
        let mut entries = Vec::new();
        for item in &addresses {
            let entry = construct_empty_object(global_object);
            let fields = [
                ("address", text_value(vm, &item.address)),
                ("cidr", text_value(vm, &format!("{}/{}", item.address, item.prefix))),
                ("netmask", text_value(vm, &item.netmask)),
                ("family", text_value(vm, if item.ipv6 { "IPv6" } else { "IPv4" })),
                ("mac", text_value(vm, &mac)),
                ("internal", js_boolean(internal)),
            ];
            for (key, value) in fields {
                entry.put_direct(vm, &property_key(vm, key), value, 0);
            }
            if item.ipv6 {
                entry.put_direct(vm, &property_key(vm, "scopeid"), js_number(item.scope_id), 0);
            }
            entries.push(entry.as_value());
        }
        result.put_direct(vm, &property_key(vm, &name), construct_array_of(global_object, &entries).as_value(), 0);
    }
    Ok(result.as_value())
}
host_function!(network_interfaces_function, network_interfaces_body);

/// O erro de `uv_os_getpriority`/`uv_os_setpriority`.
fn priority_system_error(global_object: &JSGlobalObject, syscall: &str, errno: i32, code: &str, description: &str) -> Thrown {
    let message = format!("A system error occurred: {syscall} returned {code} ({description})");
    let info = [
        ("code", NetworkProperty::Text(code)),
        ("syscall", NetworkProperty::Text(syscall)),
        ("message", NetworkProperty::Text(description)),
        ("errno", NetworkProperty::Number(errno)),
    ];
    // Ordem medida no bun 1.4.2: message, name, code, info, syscall, errno.
    let properties = [
        ("name", NetworkProperty::Text("SystemError")),
        ("code", NetworkProperty::Text("ERR_SYSTEM_ERROR")),
        ("info", NetworkProperty::Object(&info)),
        ("syscall", NetworkProperty::Text(syscall)),
        ("errno", NetworkProperty::Number(errno)),
    ];
    throw_error_with_properties(global_object, &message, &properties)
}

/// O `pid` (0 é o próprio processo): número inteiro de 32 bits, senão `ERR_INVALID_ARG_TYPE`.
fn pid_argument(global_object: &JSGlobalObject, value: JSValue) -> Result<i32, Thrown> {
    if value.is_undefined() {
        return Ok(0);
    }
    if !value.is_number() || value.as_number().fract() != 0.0 || value.as_number().abs() > f64::from(i32::MAX) {
        let message = format!("The \"pid\" argument must be of type number. Received {}", received_description(global_object, value));
        return Err(throw_coded_type_error(global_object, &message, "ERR_INVALID_ARG_TYPE"));
    }
    Ok(value.as_number() as i32)
}

/// O `nice` do processo, o campo 19 de `/proc/PID/stat` (depois do `)` do nome, que pode ter espaços).
fn read_nice(pid: i32) -> Option<i32> {
    let path = if pid == 0 { "/proc/self/stat".to_string() } else { format!("/proc/{pid}/stat") };
    let text = read_text(&path)?;
    let after_name = &text[text.rfind(')')? + 1..];
    after_name.split_whitespace().nth(16)?.parse().ok()
}

fn get_priority_body(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    let pid = pid_argument(global_object, call.argument(0))?;
    match read_nice(pid) {
        Some(nice) => Ok(js_number(nice)),
        None => Err(priority_system_error(global_object, "uv_os_getpriority", -3, "ESRCH", "no such process")),
    }
}
host_function!(get_priority_function, get_priority_body);

/// `os.setPriority([pid, ]priority)`: valida como o node e aplica com `renice`, como `process_system.rs` faz com `kill`.
fn set_priority_body(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    if call.argument_count() == 0 {
        return Err(throw_coded_type_error(global_object, "Not enough arguments", "ERR_MISSING_ARGS"));
    }
    let (pid_value, priority_value) = if call.argument_count() == 1 { (JSValue::undefined(), call.argument(0)) } else { (call.argument(0), call.argument(1)) };
    let pid = pid_argument(global_object, pid_value)?;
    if !priority_value.is_number() {
        let message = format!("The \"priority\" argument must be of type number. Received {}", received_description(global_object, priority_value));
        return Err(throw_coded_type_error(global_object, &message, "ERR_INVALID_ARG_TYPE"));
    }
    let number = priority_value.as_number();
    if !number.is_finite() || number.fract() != 0.0 || !PRIORITY_RANGE.contains(&(number as i32)) || number.abs() > 100.0 {
        let message = format!("The value of \"priority\" is out of range. It must be >= -20 and <= 19. Received {}", rust_string(&priority_value.to_string(global_object.vm()).value()));
        return Err(throw_coded_range_error(global_object, &message, "ERR_OUT_OF_RANGE"));
    }
    let priority = number as i32;
    let target = if pid == 0 { host().pid() as i32 } else { pid };
    let exists = host().stat(&format!("/proc/{target}")).is_ok();
    if !exists {
        return Err(priority_system_error(global_object, "uv_os_setpriority", -3, "ESRCH", "no such process"));
    }
    let applied = host().set_priority(target, priority).is_ok();
    if !applied {
        return Err(priority_system_error(global_object, "uv_os_setpriority", -13, "EACCES", "permission denied"));
    }
    Ok(JSValue::undefined())
}
host_function!(set_priority_function, set_priority_body);

/// `require("os")`, na ordem exata de chaves do bun.
pub(crate) fn install_os_module(global_object: &JSGlobalObject) -> HostResult {
    let vm = global_object.vm();
    let module = construct_empty_object(global_object);
    let methods: [(&str, &str, u32, crate::runtime::native_function::NativeFunction); 19] = [
        ("availableParallelism", "availableParallelism", 0, available_parallelism_function),
        ("arch", "arch", 0, arch_function),
        ("cpus", "", 0, cpus_function),
        ("endianness", "endianness", 0, endianness_function),
        ("freemem", "freemem", 0, freemem_function),
        ("getPriority", "getPriority", 2, get_priority_function),
        ("homedir", "homedir", 1, homedir_function),
        ("hostname", "hostname", 1, hostname_function),
        ("loadavg", "loadavg", 1, loadavg_function),
        ("networkInterfaces", "networkInterfaces", 1, network_interfaces_function),
        ("platform", "platform", 0, platform_function),
        ("release", "release", 0, release_function),
        ("setPriority", "setPriority", 2, set_priority_function),
        ("tmpdir", "tmpdir", 0, tmpdir_function),
        ("totalmem", "totalmem", 0, totalmem_function),
        ("type", "type", 0, type_function),
        ("uptime", "uptime", 1, uptime_function),
        ("userInfo", "userInfo", 2, user_info_function),
        ("version", "version", 0, version_function),
    ];
    for (key, display_name, length, function) in methods {
        put_direct_native_function_with_display_name(
            vm,
            global_object,
            &module,
            &Identifier::from_span(vm, key.as_bytes()),
            &WtfString::from_latin1(display_name.as_bytes()),
            length,
            function,
            ImplementationVisibility::Public,
            Intrinsic::NoIntrinsic,
            0,
        );
    }
    put_direct_native_function_with_display_name(
        vm,
        global_object,
        &module,
        &Identifier::from_span(vm, b"machine"),
        &WtfString::from_latin1(b"machine"),
        0,
        machine_function,
        ImplementationVisibility::Public,
        Intrinsic::NoIntrinsic,
        0,
    );
    module.put_direct(vm, &property_key(vm, "devNull"), text_value(vm, "/dev/null"), 0);
    // `EOL` é um acessor `get EOL` enumerável e configurável, sem setter (medido no bun 1.4.2).
    let eol_getter = create_host_custom_accessor_getter_function(vm, global_object, "EOL", eol_function);
    let eol_accessor = GetterSetter::create_from_values(vm, eol_getter.as_value(), JSValue::undefined());
    module.put_direct_non_index_accessor_without_transition(vm, &property_key(vm, "EOL"), &eol_accessor, ACCESSOR);
    module.put_direct(vm, &property_key(vm, "constants"), build_constants(global_object), 0);
    Ok(module.as_value())
}
