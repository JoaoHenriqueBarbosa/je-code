//! A FORMA do global `process` do bun 1.4.2: as chaves próprias na ordem medida (`Object.getOwnPropertyNames`), o tipo
//! e o descritor de cada uma, o protótipo com o `constructor` `EventEmitter` e os métodos de emissor, o
//! `Symbol.toStringTag` próprio e o `name`/`length` de cada função.
//!
//! Descritores medidos: tudo é `{writable, enumerable, configurable}`, exceto os acessores `_eval`, `argv`, `connected`,
//! `debugPort`, `execArgv`, `ppid` e `title` (enumeráveis e configuráveis, com `get` e `set`) e `exitCode` (enumerável
//! e não configurável, em `process_exit.rs`). Os métodos de emissor ficam no PROTÓTIPO (enumeráveis), não no `process`.
//!
//! Valores: os literais do bun que o sandbox finge (`version`, `versions`, `release`, `config`, `features`,
//! `revision`, `report`) vêm do golden; `cwd`, `env`, `pid`, `ppid`, `uptime`, `umask` e os ids de usuário vêm do
//! sistema. `hrtime`, `memoryUsage`, `cpuUsage` e `resourceUsage` estão em `process_stats.rs`. As funções cujo
//! comportamento é de uma fatia futura do plano (`kill`, `chdir`,
//! `emitWarning`...) já têm o nome, o `length` e o descritor certos e devolvem `undefined`
//! até a fatia delas medir o comportamento.

use std::cell::RefCell;
use std::collections::HashMap;
use std::time::Duration;

use crate::host_function;
use crate::runtime::collection_support::put_to_string_tag;
use crate::runtime::host::host;
use crate::runtime::host_call::{HostCall, HostResult};
use crate::runtime::intl_support::prop;
use crate::runtime::js_array::construct_array;
use crate::runtime::js_global_object::JSGlobalObject;
use crate::runtime::js_object::{JSObject, JSObjectRef};
use crate::runtime::js_set::JSSet;
use crate::runtime::js_string::js_string;
use crate::runtime::js_value::{js_number, JSValue};
use crate::runtime::native_function::NativeFunction;
use crate::runtime::object_constructor::construct_empty_object;
use crate::runtime::process_exit::{install_emitter_methods, install_exit_code, own_function};
use crate::runtime::process_object::process_next_tick;
use crate::runtime::process_stats::{self, stat_field};
use crate::runtime::process_system::{abort_function, chdir_function, cwd_function, kill_function, raw_kill_function, script_argv, umask_function};
use crate::runtime::property_attribute::DONT_ENUM;
use crate::runtime::property_name::PropertyName;
use crate::runtime::timers::{native, native_function, put_accessor_with};
use crate::runtime::vm::VM;
use crate::wtf::text::wtf_string::String as WtfString;

/// O caminho do executável que o sandbox mostra em `execPath` e `argv[0]`.
pub const EXEC_PATH: &str = "/usr/local/bin/bun";

/// A versão do node que o bun 1.4.2 declara (`process.versions.node`, `process.version` com o `v`).
pub const NODE_VERSION: &str = "26.3.0";

/// A versão do bun (`process.versions.bun`, `Bun.version`).
pub const BUN_VERSION: &str = "1.4.2";

/// O commit do bun 1.4.2 (`process.revision`, `Bun.revision`; o uWebSockets e o uSockets mostram o mesmo).
pub const BUN_REVISION: &str = "744846f844374847c902b5e7fd59b4342a51ef99";

/// Um valor de dados do `process`, descrito em tabela e construído na instalação.
enum Value {
    Str(&'static str),
    Bool(bool),
    Int(i32),
    Undefined,
    Array(&'static [Value]),
    Object(&'static [(&'static str, Value)]),
    /// Função nativa cujo `name` é a chave onde ela mora e o `length` é o número.
    Function(u32),
    /// Um `Set` vazio.
    Set,
    /// O objeto `process.env`, com o ambiente do processo.
    Env,
    /// `process.stdin/stdout/stderr` do descritor dado (`process_stdio.rs`).
    Stream(u8),
}

/// Uma chave própria do `process`, na ordem do bun.
enum Member {
    Function(&'static str, u32),
    /// Função com `name` vazio.
    Anonymous(&'static str, u32),
    /// Função definida em `process_exit.rs`.
    Own(&'static str),
    /// Acessor enumerável e configurável, com `get` e `set`.
    Accessor(&'static str),
    Data(&'static str, Value),
    ExitCode,
}

use Member::{Accessor, Anonymous, Data, ExitCode, Function, Own};

const FINALIZATION: Value = Value::Object(&[("register", Value::Function(2)), ("registerBeforeExit", Value::Function(2)), ("unregister", Value::Function(1))]);

const CONFIG: Value = Value::Object(&[
    ("target_defaults", Value::Object(&[])),
    (
        "variables",
        Value::Object(&[
            ("v8_enable_i18n_support", Value::Int(1)),
            ("enable_lto", Value::Bool(false)),
            ("enable_thin_lto", Value::Bool(false)),
            ("lto_jobs", Value::Str("")),
            ("node_module_version", Value::Int(147)),
            ("napi_build_version", Value::Int(10)),
            ("node_builtin_shareable_builtins", Value::Array(&[])),
            ("node_byteorder", Value::Str("little")),
            ("node_without_node_options", Value::Bool(true)),
            ("clang", Value::Int(0)),
            ("control_flow_guard", Value::Bool(false)),
            ("coverage", Value::Bool(false)),
            ("dcheck_always_on", Value::Int(0)),
            ("debug_nghttp2", Value::Bool(false)),
            ("debug_node", Value::Bool(false)),
            ("enable_pgo_generate", Value::Bool(false)),
            ("enable_pgo_use", Value::Bool(false)),
            ("error_on_warn", Value::Bool(false)),
            ("force_dynamic_crt", Value::Int(0)),
            ("napi_build", Value::Str("0.0")),
            ("host_arch", Value::Str("x64")),
            ("target_arch", Value::Str("x64")),
            ("asan", Value::Int(0)),
        ]),
    ),
]);

const FEATURES: Value = Value::Object(&[
    ("inspector", Value::Bool(true)),
    ("debug", Value::Bool(false)),
    ("uv", Value::Bool(true)),
    ("ipv6", Value::Bool(true)),
    ("tls_alpn", Value::Bool(true)),
    ("tls_sni", Value::Bool(true)),
    ("tls_ocsp", Value::Bool(true)),
    ("tls", Value::Bool(true)),
    ("cached_builtins", Value::Bool(true)),
    ("openssl_is_boringssl", Value::Bool(true)),
    ("quic", Value::Bool(true)),
    ("require_module", Value::Bool(true)),
    ("typescript", Value::Str("transform")),
]);

const RELEASE: Value = Value::Object(&[
    ("name", Value::Str("node")),
    ("sourceUrl", Value::Str("https://github.com/oven-sh/bun/releases/download/bun-v1.4.2/bun-linux-x64.zip")),
    ("headersUrl", Value::Str("https://nodejs.org/download/release/v26.3.0/node-v26.3.0-headers.tar.gz")),
]);

const REPORT: Value = Value::Object(&[
    ("compact", Value::Bool(false)),
    ("directory", Value::Str("")),
    ("filename", Value::Str("")),
    ("getReport", Value::Function(1)),
    ("reportOnFatalError", Value::Bool(false)),
    ("reportOnSignal", Value::Bool(false)),
    ("reportOnUncaughtException", Value::Bool(false)),
    ("excludeEnv", Value::Str("SIGUSR2")),
    ("writeReport", Value::Function(1)),
]);

const VERSIONS: Value = Value::Object(&[
    ("node", Value::Str(NODE_VERSION)),
    ("bun", Value::Str(BUN_VERSION)),
    ("boringssl", Value::Str("41bf9b59c2ebf277a7aa427e1ecad5cc80dd4d4f")),
    ("openssl", Value::Str("1.1.0")),
    ("llhttp", Value::Str("9.3.0")),
    ("libarchive", Value::Str("ded82291ab41d5e355831b96b0e1ff49e24d8939")),
    ("mimalloc", Value::Str("6a64e1ba7f5b2130d4efccb67ec87fd0003f0f6a")),
    ("picohttpparser", Value::Str("066d2b1e9ab820703db0837a7255d92d30f0c9f5")),
    ("uwebsockets", Value::Str(BUN_REVISION)),
    ("webkit", Value::Str("2e2aa2290fac856d6f451ceacb58f7f5b44dd057")),
    ("zig", Value::Str("04e7f6ac1e009525bc00934f20199c68f04e0a24")),
    ("zlib", Value::Str("12731092979c6d07f42da27da673a9f6c7b13586")),
    ("tinycc", Value::Str("05f0fafaa3be31e31d7b4b5c17dc60f62c991171")),
    ("lolhtml", Value::Str("725ce499aa9b71e38b7a2d0a9fbb6d7294a4079e")),
    ("ares", Value::Str("c7a3138dcfe3bb0eaaf10c0c24c36dc66dc790ab")),
    ("libdeflate", Value::Str("c8c56a20f8f621e6a966b716b31f1dedab6a41e3")),
    ("usockets", Value::Str(BUN_REVISION)),
    ("lshpack", Value::Str("8905c024b6d052f083a3d11d0a169b3c2735c8a1")),
    ("zstd", Value::Str("f8745da6ff1ad1e7bab384bd1f9d742439278e99")),
    ("v8", Value::Str("14.6.202.34-node.20")),
    ("uv", Value::Str("1.48.0")),
    ("napi", Value::Str("10")),
    ("icu", Value::Str("78.3")),
    ("unicode", Value::Str("17.0")),
    ("sqlite", Value::Str("3.53.2")),
    ("modules", Value::Str("147")),
]);

/// As chaves próprias do `process` na ordem de `Object.getOwnPropertyNames(process)` do bun 1.4.2.
const MEMBERS: &[Member] = &[
    Accessor("_eval"),
    Function("_getActiveHandles", 0),
    Function("_getActiveRequests", 0),
    Function("_kill", 2),
    Function("_linkedBinding", 0),
    Data("_preload_modules", Value::Array(&[])),
    Anonymous("_rawDebug", 0),
    Function("_tickCallback", 0),
    Function("abort", 1),
    Data("allowedNodeEnvironmentFlags", Value::Set),
    Anonymous("loadEnvFile", 1),
    Data("finalization", FINALIZATION),
    Data("arch", Value::Str("x64")),
    Accessor("argv"),
    Data("argv0", Value::Str("bun")),
    Function("assert", 1),
    Function("availableMemory", 0),
    Function("binding", 1),
    Data("browser", Value::Bool(false)),
    Data("channel", Value::Undefined),
    Function("chdir", 1),
    Data("config", CONFIG),
    Accessor("connected"),
    Function("constrainedMemory", 0),
    Function("cpuUsage", 1),
    Function("threadCpuUsage", 1),
    Function("cwd", 1),
    Accessor("debugPort"),
    Data("disconnect", Value::Undefined),
    Function("dlopen", 1),
    Function("emitWarning", 1),
    Data("env", Value::Env),
    Accessor("execArgv"),
    Data("execPath", Value::Str(EXEC_PATH)),
    Function("execve", 3),
    Own("exit"),
    ExitCode,
    Function("_fatalException", 1),
    Data("features", FEATURES),
    Function("getActiveResourcesInfo", 0),
    Function("getBuiltinModule", 1),
    Own("hasUncaughtExceptionCaptureCallback"),
    Function("hrtime", 0),
    Data("isBun", Value::Bool(true)),
    Function("kill", 2),
    Data("mainModule", Value::Undefined),
    Function("memoryUsage", 0),
    Data("moduleLoadList", Value::Array(&[])),
    Function("nextTick", 1),
    Function("openStdin", 0),
    Data("pid", Value::Int(0)),
    Data("platform", Value::Str("linux")),
    Accessor("ppid"),
    Own("reallyExit"),
    Function("ref", 1),
    Data("release", RELEASE),
    Data("report", REPORT),
    Function("resourceUsage", 0),
    Data("revision", Value::Str(BUN_REVISION)),
    Data("send", Value::Undefined),
    Function("setSourceMapsEnabled", 1),
    Own("setUncaughtExceptionCaptureCallback"),
    Data("stderr", Value::Stream(2)),
    Data("stdin", Value::Stream(0)),
    Data("stdout", Value::Stream(1)),
    Accessor("title"),
    Function("umask", 1),
    Function("unref", 1),
    Function("uptime", 1),
    Data("version", Value::Str("v26.3.0")),
    Data("versions", VERSIONS),
    Function("getegid", 0),
    Function("geteuid", 0),
    Function("getgid", 0),
    Function("getgroups", 0),
    Function("getuid", 0),
    Function("setegid", 1),
    Function("seteuid", 1),
    Function("setgid", 1),
    Function("setgroups", 1),
    Function("initgroups", 2),
    Function("setuid", 1),
    Data("_exiting", Value::Bool(false)),
    Function("_debugEnd", 0),
    Function("_debugProcess", 0),
    Function("_startProfilerIdleNotifier", 0),
    Function("_stopProfilerIdleNotifier", 0),
];

thread_local! {
    /// O instante em que o `process` nasceu, base de `uptime()`.
    static START: u64 = ul_common::time::monotonic_nanos();
    /// Os valores gravados pelos setters dos acessores (`process.argv = [...]`), por nome.
    static SLOTS: RefCell<HashMap<&'static str, JSValue>> = RefCell::new(HashMap::new());
}

pub(crate) fn text_value(vm: &VM, text: &str) -> JSValue {
    JSValue::from_js_string(js_string(vm, &WtfString::from_utf8(text.as_bytes())))
}

pub(crate) fn array_value(global_object: &JSGlobalObject, values: &[JSValue]) -> JSValue {
    construct_array(global_object.vm(), &global_object.array_structure(), values).as_value()
}

/// `process.env`: o objeto exótico de `process_env.rs`, com o ambiente do processo.
fn env_object(global_object: &JSGlobalObject) -> JSValue {
    crate::runtime::process_env::create(global_object)
}

fn build(global_object: &JSGlobalObject, value: &Value, name: &str) -> JSValue {
    let vm = global_object.vm();
    match value {
        Value::Str(text) => text_value(vm, text),
        Value::Bool(flag) => JSValue::Bool(*flag),
        Value::Int(number) => js_number(*number),
        Value::Undefined => JSValue::undefined(),
        Value::Array(items) => {
            let values: Vec<JSValue> = items.iter().map(|item| build(global_object, item, "")).collect();
            array_value(global_object, &values)
        }
        Value::Object(entries) => {
            let object = construct_empty_object(global_object);
            for (key, entry) in entries.iter() {
                object.put_direct(vm, &prop(vm, key), build(global_object, entry, key), 0);
            }
            object.as_value()
        }
        Value::Function(length) => native(global_object, name, *length, undefined_function),
        Value::Set => JSSet::create(vm, &global_object.set_structure()).as_value(),
        Value::Env => env_object(global_object),
        Value::Stream(fd) => crate::runtime::process_stdio::create(global_object, *fd),
    }
}

fn undefined_body(_global_object: &JSGlobalObject, _call: &HostCall) -> HostResult {
    Ok(JSValue::undefined())
}
host_function!(pub undefined_function, undefined_body);

/// Fim do programa (`cell_registry::reset_program_state`): os valores gravados são células do programa.
/// As raízes do coletor: os valores guardados do `process`.
pub(crate) fn visit_gc_roots(marker: &mut crate::runtime::gc::Marker) {
    SLOTS.with(|slots| slots.borrow().values().for_each(|value| marker.value(*value)));
}

pub(crate) fn reset_for_program() {
    let _ = SLOTS.try_with(|slots| slots.borrow_mut().clear());
    crate::runtime::process_env::reset_for_program();
}

fn empty_array_body(global_object: &JSGlobalObject, _call: &HostCall) -> HostResult {
    Ok(array_value(global_object, &[]))
}
host_function!(empty_array_function, empty_array_body);

/// O tempo desde que o `process` nasceu, base de `uptime()` e de `hrtime`.
pub(crate) fn elapsed_since_start() -> Duration {
    Duration::from_nanos(ul_common::time::monotonic_nanos().saturating_sub(START.with(|start| *start)))
}

fn uptime_body(_global_object: &JSGlobalObject, _call: &HostCall) -> HostResult {
    Ok(js_number(elapsed_since_start().as_secs_f64()))
}
host_function!(uptime_function, uptime_body);

fn owner_ids() -> (u32, u32) {
    host().stat("/proc/self").map_or((0, 0), |stat| (stat.uid, stat.gid))
}

fn user_id_body(_global_object: &JSGlobalObject, _call: &HostCall) -> HostResult {
    Ok(js_number(owner_ids().0))
}
host_function!(user_id_function, user_id_body);

fn group_id_body(_global_object: &JSGlobalObject, _call: &HostCall) -> HostResult {
    Ok(js_number(owner_ids().1))
}
host_function!(group_id_function, group_id_body);

fn groups_body(global_object: &JSGlobalObject, _call: &HostCall) -> HostResult {
    Ok(array_value(global_object, &[js_number(owner_ids().1)]))
}
host_function!(groups_function, groups_body);

/// O pai do processo, do campo 4 de `/proc/self/stat`.
fn parent_pid() -> u32 {
    stat_field(1).and_then(|pid| u32::try_from(pid).ok()).unwrap_or(1)
}

/// A função nativa de cada nome do `process` (as sem corpo medido ainda devolvem `undefined`).
fn function_body(name: &str) -> NativeFunction {
    match name {
        "nextTick" => process_next_tick,
        "emitWarning" => crate::runtime::process_warning::process_emit_warning,
        "cwd" => cwd_function,
        "chdir" => chdir_function,
        "kill" => kill_function,
        "_kill" => raw_kill_function,
        "abort" => abort_function,
        "uptime" => uptime_function,
        "hrtime" => process_stats::hrtime,
        "memoryUsage" => process_stats::memory_usage,
        "cpuUsage" => process_stats::cpu_usage,
        "resourceUsage" => process_stats::resource_usage,
        "umask" => umask_function,
        "getuid" | "geteuid" => user_id_function,
        "getgid" | "getegid" => group_id_function,
        "getgroups" => groups_function,
        "_getActiveHandles" | "_getActiveRequests" | "getActiveResourcesInfo" => empty_array_function,
        _ => undefined_function,
    }
}

/// A propriedade pendurada na função, quando o bun tem uma (`hrtime.bigint`, `memoryUsage.rss`).
fn function_property(name: &str) -> Option<(&'static str, NativeFunction)> {
    match name {
        "hrtime" => Some(("bigint", process_stats::hrtime_bigint)),
        "memoryUsage" => Some(("rss", process_stats::memory_rss)),
        _ => None,
    }
}

/// O valor do acessor `name`: o gravado pelo setter ou o padrão, que passa a ser o gravado (identidade estável).
fn slot_value(global_object: &JSGlobalObject, name: &'static str, default: fn(&JSGlobalObject) -> JSValue) -> JSValue {
    if let Some(value) = read_slot(name) {
        return value;
    }
    let value = default(global_object);
    write_slot(name, value);
    value
}

fn default_undefined(_global_object: &JSGlobalObject) -> JSValue {
    JSValue::undefined()
}

fn default_argv(global_object: &JSGlobalObject) -> JSValue {
    let vm = global_object.vm();
    let values: Vec<JSValue> = script_argv().iter().map(|argument| text_value(vm, argument)).collect();
    array_value(global_object, &values)
}

/// O array de `process.argv` (o mesmo que `Bun.argv` entrega).
pub(crate) fn process_argv(global_object: &JSGlobalObject) -> JSValue {
    slot_value(global_object, "argv", default_argv)
}

/// O array de `process.execArgv`.
pub(crate) fn process_exec_argv(global_object: &JSGlobalObject) -> JSValue {
    slot_value(global_object, "execArgv", default_empty_array)
}

/// O valor gravado no acessor `name`, se houve (o `overridden_main` do `Bun.main` mora aqui, sob `"Bun.main"`).
pub(crate) fn read_slot(name: &'static str) -> Option<JSValue> {
    SLOTS.with(|slots| slots.borrow().get(name).copied())
}

/// Grava `value` no acessor `name`; some no fim do programa, com os demais.
pub(crate) fn write_slot(name: &'static str, value: JSValue) {
    SLOTS.with(|slots| slots.borrow_mut().insert(name, value));
}

fn default_false(_global_object: &JSGlobalObject) -> JSValue {
    JSValue::Bool(false)
}

fn default_debug_port(_global_object: &JSGlobalObject) -> JSValue {
    js_number(9229)
}

fn default_empty_array(global_object: &JSGlobalObject) -> JSValue {
    array_value(global_object, &[])
}

fn default_parent_pid(_global_object: &JSGlobalObject) -> JSValue {
    js_number(parent_pid())
}

fn default_title(global_object: &JSGlobalObject) -> JSValue {
    text_value(global_object.vm(), "bun")
}

/// Define o par `get`/`set` de um acessor guardado em `SLOTS`.
macro_rules! slot_accessor {
    ($getter:ident, $getter_body:ident, $setter:ident, $setter_body:ident, $name:literal, $default:path) => {
        fn $getter_body(global_object: &JSGlobalObject, _call: &HostCall) -> HostResult {
            Ok(slot_value(global_object, $name, $default))
        }
        host_function!($getter, $getter_body);
        fn $setter_body(_global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
            write_slot($name, call.argument(0));
            Ok(JSValue::undefined())
        }
        host_function!($setter, $setter_body);
    };
}

slot_accessor!(eval_get, eval_get_body, eval_set, eval_set_body, "_eval", default_undefined);
slot_accessor!(argv_get, argv_get_body, argv_set, argv_set_body, "argv", default_argv);
slot_accessor!(connected_get, connected_get_body, connected_set, connected_set_body, "connected", default_false);
slot_accessor!(debug_port_get, debug_port_get_body, debug_port_set, debug_port_set_body, "debugPort", default_debug_port);
slot_accessor!(exec_argv_get, exec_argv_get_body, exec_argv_set, exec_argv_set_body, "execArgv", default_empty_array);
slot_accessor!(ppid_get, ppid_get_body, ppid_set, ppid_set_body, "ppid", default_parent_pid);
slot_accessor!(title_get, title_get_body, title_set, title_set_body, "title", default_title);

/// O par `(get, set)` do acessor `name`.
fn accessor_functions(name: &str) -> (NativeFunction, NativeFunction) {
    match name {
        "argv" => (argv_get as NativeFunction, argv_set as NativeFunction),
        "connected" => (connected_get as NativeFunction, connected_set as NativeFunction),
        "debugPort" => (debug_port_get as NativeFunction, debug_port_set as NativeFunction),
        "execArgv" => (exec_argv_get as NativeFunction, exec_argv_set as NativeFunction),
        "ppid" => (ppid_get as NativeFunction, ppid_set as NativeFunction),
        "title" => (title_get as NativeFunction, title_set as NativeFunction),
        _ => (eval_get as NativeFunction, eval_set as NativeFunction),
    }
}

/// O `EventEmitter` do `constructor` do protótipo do `process`.
fn event_emitter_body(_global_object: &JSGlobalObject, _call: &HostCall) -> HostResult {
    Ok(JSValue::undefined())
}
host_function!(event_emitter_function, event_emitter_body);

fn install_member(global_object: &JSGlobalObject, process: &JSObject, member: &Member) {
    let vm = global_object.vm();
    match member {
        Function(name, length) => {
            // `nextTick` é função de JS no bun: `new process.nextTick(fn)` agenda o tick e não lança.
            let function = if *name == "nextTick" {
                crate::runtime::timers::native_function_with_constructor(global_object, name, *length, function_body(name), function_body(name))
            } else {
                native_function(global_object, name, *length, function_body(name))
            };
            if let Some((property, body)) = function_property(name) {
                function.put_direct(vm, &prop(vm, property), native(global_object, property, 0, body), 0);
            }
            process.put_direct(vm, &prop(vm, name), function.as_value(), 0);
        }
        Anonymous(name, length) => {
            process.put_direct(vm, &prop(vm, name), native(global_object, "", *length, undefined_function), 0);
        }
        Own(name) => {
            if let Some((length, function)) = own_function(name) {
                process.put_direct(vm, &prop(vm, name), native(global_object, name, length, function), 0);
            }
        }
        Accessor(name) => {
            let (getter, setter) = accessor_functions(name);
            put_accessor_with(global_object, process, name, getter, Some(setter), 0);
        }
        Data("pid", _) => {
            process.put_direct(vm, &prop(vm, "pid"), js_number(host().pid()), 0);
        }
        Data(name, value) => {
            process.put_direct(vm, &prop(vm, name), build(global_object, value, name), 0);
        }
        ExitCode => install_exit_code(global_object, process),
    }
}

/// Monta o objeto `process`: protótipo `EventEmitter`, as chaves na ordem do bun e o `Symbol.toStringTag` próprio.
pub(crate) fn build_process(global_object: &JSGlobalObject) -> JSObjectRef {
    let vm = global_object.vm();
    START.with(|_| {});
    let prototype = construct_empty_object(global_object);
    prototype.put_direct(vm, &prop(vm, "constructor"), native(global_object, "EventEmitter", 0, event_emitter_function), DONT_ENUM);
    install_emitter_methods(global_object, &prototype);
    put_to_string_tag(vm, &prototype, "EventEmitter");
    let process = construct_empty_object(global_object);
    process.set_prototype_direct(vm, prototype.as_value());
    for member in MEMBERS {
        install_member(global_object, &process, member);
    }
    process.put_direct(vm, &PropertyName::from_identifier(&vm.property_names.to_string_tag_symbol), text_value(vm, "process"), 0);
    process
}
