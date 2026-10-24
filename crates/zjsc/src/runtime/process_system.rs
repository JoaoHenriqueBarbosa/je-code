//! O lado "sistema" do `process` do bun 1.4.2: `argv` (com o caminho do script), `cwd`/`chdir` (o diretório de trabalho
//! é do programa, não do host: os testes em paralelo dividem o processo real), `umask`, `kill` e `abort`.
//!
//! Erros medidos: `chdir` de caminho inexistente lança `Error` `ENOENT: no such file or directory, chdir '/app' ->
//! '/x'` com `errno`, `code`, `syscall` e `path` (o diretório antigo); argumento que não é texto lança
//! `ERR_INVALID_ARG_TYPE`; `kill` de pid morto lança o `SystemError` `kill() failed: ESRCH: No such process` e sinal
//! desconhecido lança `ERR_UNKNOWN_SIGNAL`. `chdir` devolve o novo diretório (medido: `String(process.chdir('/'))` é
//! `/`). Sinal para si mesmo vai aos ouvintes do `process`; sem ouvinte o programa morre pelo sinal (código 128 + n).

use std::cell::{Cell, RefCell};
use std::path::{Component, Path, PathBuf};

use crate::host_function;
use crate::runtime::host::host;
use crate::runtime::host_call::{HostCall, HostResult};
use crate::runtime::js_global_object::JSGlobalObject;
use crate::runtime::js_module_loader::rust_string;
use crate::runtime::js_value::{js_number, JSValue};
use crate::runtime::js_web_assembly::received_description;
use crate::runtime::node_error::{throw_coded_range_error, throw_coded_type_error, throw_error_with_properties, throw_system_error, NetworkProperty};
use crate::runtime::process_exit::{deliver_signal, die_by_signal, number_text};
use crate::runtime::process_shape::{text_value, EXEC_PATH};

/// O que o programa principal sabe de si: o caminho do script, os argumentos depois dele e o diretório de trabalho.
#[derive(Default)]
struct ProgramState {
    script: Option<String>,
    arguments: Vec<String>,
    cwd: Option<String>,
}

/// O `umask` inicial do Debian.
const DEFAULT_UMASK: u32 = 0o022;

thread_local! {
    static PROGRAM: RefCell<ProgramState> = RefCell::new(ProgramState::default());
    static UMASK: Cell<u32> = const { Cell::new(DEFAULT_UMASK) };
}

/// Fim do programa (`cell_registry::reset_program_state`).
pub(crate) fn reset_for_program() {
    // Argumentos e diretório de trabalho são configuração do embedder (`set_script_arguments`, `set_working_directory`),
    // feita antes de `run_program`, cujo início solta o programa anterior e chega aqui: não são estado de programa.
    let _ = PROGRAM.try_with(|state| {
        let mut state = state.borrow_mut();
        *state = ProgramState { arguments: std::mem::take(&mut state.arguments), cwd: state.cwd.take(), ..ProgramState::default() };
    });
    let _ = UMASK.try_with(|mask| mask.set(DEFAULT_UMASK));
}

/// Os argumentos que seguem o script na linha de comando (`bun main.js x --flag`); valem para o próximo programa.
pub fn set_script_arguments(arguments: &[&str]) {
    PROGRAM.with(|state| state.borrow_mut().arguments = arguments.iter().map(|argument| (*argument).to_string()).collect());
}

/// O diretório de trabalho inicial do próximo programa (sem isso, o do processo).
pub fn set_working_directory(directory: &str) {
    PROGRAM.with(|state| state.borrow_mut().cwd = Some(directory.to_string()));
}

/// O programa principal começou: guarda o caminho do script, que é o `argv[1]`.
pub(crate) fn begin_main_script(path: &str) {
    // `bun -e` não tem script: o `argv` é `[execPath, ...argumentos]` e o `main_path` cai em `<cwd>/[eval]`.
    let script = (!path.ends_with("/[eval]")).then(|| path.to_string());
    PROGRAM.with(|state| state.borrow_mut().script = script);
}

/// O `vm.main()` do bun: o caminho do script principal, ou `<cwd>/[eval]` sem script (`bun -e`).
pub(crate) fn main_path() -> String {
    if let Some(script) = PROGRAM.with(|state| state.borrow().script.clone()) {
        return script;
    }
    resolve_in_current_directory("[eval]")
}

/// `target` resolvido no diretório corrente do programa, com `.` e `..` normalizados (o `joinAbsString` do bun).
pub(crate) fn resolve_in_current_directory(target: &str) -> String {
    resolve_path(&current_directory(), target).to_string_lossy().into_owned()
}

/// O `argv` inicial: o executável, o script (quando há) e os argumentos.
pub(crate) fn script_argv() -> Vec<String> {
    PROGRAM.with(|state| {
        let state = state.borrow();
        let mut argv = vec![EXEC_PATH.to_string()];
        argv.extend(state.script.clone());
        argv.extend(state.arguments.iter().cloned());
        argv
    })
}

pub(crate) fn current_directory() -> String {
    if let Some(directory) = PROGRAM.with(|state| state.borrow().cwd.clone()) {
        return directory;
    }
    host().cwd()
}

/// Junta `target` ao diretório `base` e normaliza `.` e `..` (sem seguir ligações simbólicas).
fn resolve_path(base: &str, target: &str) -> PathBuf {
    let joined = Path::new(base).join(target);
    let mut resolved = PathBuf::from("/");
    for component in joined.components() {
        match component {
            Component::ParentDir => {
                resolved.pop();
            }
            Component::Normal(name) => resolved.push(name),
            _ => {}
        }
    }
    resolved
}

fn cwd_body(global_object: &JSGlobalObject, _call: &HostCall) -> HostResult {
    Ok(text_value(global_object.vm(), &current_directory()))
}
host_function!(pub cwd_function, cwd_body);

/// `(errno, nome, descrição)` do erro de sistema de `chdir`, pelo `errno` do host.
fn chdir_error(errno: i32) -> (i32, &'static str, &'static str) {
    match errno {
        13 => (-13, "EACCES", "permission denied"),
        20 => (-20, "ENOTDIR", "not a directory"),
        36 => (-36, "ENAMETOOLONG", "name too long"),
        40 => (-40, "ELOOP", "too many symbolic links encountered"),
        _ => (-2, "ENOENT", "no such file or directory"),
    }
}

fn chdir_body(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    let argument = call.argument(0);
    if !argument.is_string() {
        let message = format!("The \"directory\" argument must be of type string. Received {}", received_description(global_object, argument));
        return Err(throw_coded_type_error(global_object, &message, "ERR_INVALID_ARG_TYPE"));
    }
    let given = rust_string(&argument.as_js_string().value());
    let old = current_directory();
    let target = resolve_path(&old, &given);
    let failure = match host().stat(&target.to_string_lossy()) {
        Ok(stat) if stat.is_dir => None,
        Ok(_) => Some(chdir_error(20)),
        Err(errno) => Some(chdir_error(errno)),
    };
    if let Some((errno, code, description)) = failure {
        let message = format!("{code}: {description}, chdir '{old}' -> '{given}'");
        let properties = [
            ("errno", NetworkProperty::Number(errno)),
            ("code", NetworkProperty::Text(code)),
            ("syscall", NetworkProperty::Text("chdir")),
            ("path", NetworkProperty::Text(&old)),
            ("dest", NetworkProperty::Text(&given)),
        ];
        return Err(throw_error_with_properties(global_object, &message, &properties));
    }
    let new_directory = target.to_string_lossy().into_owned();
    PROGRAM.with(|state| state.borrow_mut().cwd = Some(new_directory.clone()));
    Ok(text_value(global_object.vm(), &new_directory))
}
host_function!(pub chdir_function, chdir_body);

/// Valida o `mask` de `umask` como o node: inteiro de 32 bits sem sinal ou texto octal.
fn parse_mask(global_object: &JSGlobalObject, value: JSValue) -> Result<u32, crate::runtime::host_call::Thrown> {
    if value.is_string() {
        let text = rust_string(&value.as_js_string().value());
        if !text.is_empty() && text.bytes().all(|byte| (b'0'..=b'7').contains(&byte)) {
            if let Ok(mask) = u32::from_str_radix(&text, 8) {
                return Ok(mask);
            }
        }
        let message = format!("The argument 'mask' must be a 32-bit unsigned integer or an octal string. Received '{text}'");
        return Err(throw_coded_type_error(global_object, &message, "ERR_INVALID_ARG_VALUE"));
    }
    if !value.is_number() {
        let message = format!("The \"mask\" argument must be of type number. Received {}", received_description(global_object, value));
        return Err(throw_coded_type_error(global_object, &message, "ERR_INVALID_ARG_TYPE"));
    }
    let number = value.as_number();
    let vm = global_object.vm();
    if !number.is_finite() || number.fract() != 0.0 {
        let message = format!("The value of \"mask\" is out of range. It must be an integer. Received {}", number_text(vm, number));
        return Err(throw_coded_range_error(global_object, &message, "ERR_OUT_OF_RANGE"));
    }
    if !(0.0..=4_294_967_295.0).contains(&number) {
        let message = format!("The value of \"mask\" is out of range. It must be >= 0 && <= 4294967295. Received {}", number_text(vm, number));
        return Err(throw_coded_range_error(global_object, &message, "ERR_OUT_OF_RANGE"));
    }
    Ok(number as u32)
}

/// `process.umask([mask])`: sem argumento devolve a máscara; com argumento troca e devolve a antiga.
fn umask_body(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    let argument = call.argument(0);
    if argument.is_undefined() {
        return Ok(js_number(UMASK.with(Cell::get)));
    }
    let mask = parse_mask(global_object, argument)?;
    Ok(js_number(UMASK.with(|current| current.replace(mask & 0o777))))
}
host_function!(pub umask_function, umask_body);

/// Os sinais do Linux x86-64 pelo nome, na ordem do número.
const SIGNALS: &[(&str, i32)] = &[
    ("SIGHUP", 1),
    ("SIGINT", 2),
    ("SIGQUIT", 3),
    ("SIGILL", 4),
    ("SIGTRAP", 5),
    ("SIGABRT", 6),
    ("SIGIOT", 6),
    ("SIGBUS", 7),
    ("SIGFPE", 8),
    ("SIGKILL", 9),
    ("SIGUSR1", 10),
    ("SIGSEGV", 11),
    ("SIGUSR2", 12),
    ("SIGPIPE", 13),
    ("SIGALRM", 14),
    ("SIGTERM", 15),
    ("SIGSTKFLT", 16),
    ("SIGCHLD", 17),
    ("SIGCONT", 18),
    ("SIGSTOP", 19),
    ("SIGTSTP", 20),
    ("SIGTTIN", 21),
    ("SIGTTOU", 22),
    ("SIGURG", 23),
    ("SIGXCPU", 24),
    ("SIGXFSZ", 25),
    ("SIGVTALRM", 26),
    ("SIGPROF", 27),
    ("SIGWINCH", 28),
    ("SIGIO", 29),
    ("SIGPOLL", 29),
    ("SIGPWR", 30),
    ("SIGSYS", 31),
];

/// Os sinais cuja ação padrão é ignorar.
const IGNORED_BY_DEFAULT: [i32; 4] = [17, 18, 23, 28];

/// O nome (`SIGTERM`) do sinal de número `number`.
pub fn signal_name(number: i32) -> Option<&'static str> {
    SIGNALS.iter().find(|(_, candidate)| *candidate == number).map(|(name, _)| *name)
}

/// A falha de uma chamada `kill(2)`: `(errno, código, descrição)`.
type KillFailure = (i32, &'static str, &'static str);

const NO_SUCH_PROCESS: KillFailure = (3, "ESRCH", "No such process");
const NOT_PERMITTED: KillFailure = (1, "EPERM", "Operation not permitted");

/// Envia `signal` a `pid`: sinal 0 só testa a existência; para o próprio pid os ouvintes de `process.on(SINAL)` recebem
/// o sinal (sem ouvinte o programa morre por ele); para os outros vai um `kill` de verdade.
fn send_signal(global_object: &JSGlobalObject, pid: i32, signal: i32) -> Result<Result<(), KillFailure>, crate::runtime::host_call::Thrown> {
    let alive = |pid: i32| host().stat(&format!("/proc/{pid}")).is_ok();
    if pid > 0 && !alive(pid) {
        return Ok(Err(NO_SUCH_PROCESS));
    }
    if signal == 0 {
        return Ok(Ok(()));
    }
    if u32::try_from(pid).is_ok_and(|pid| pid == host().pid()) {
        if !IGNORED_BY_DEFAULT.contains(&signal) {
            deliver_signal(global_object, signal_name(signal).unwrap_or("SIGTERM"), signal)?;
        }
        return Ok(Ok(()));
    }
    let delivered = host().kill(pid, signal).is_ok();
    if delivered {
        return Ok(Ok(()));
    }
    Ok(Err(if pid > 0 && !alive(pid) { NO_SUCH_PROCESS } else { NOT_PERMITTED }))
}

/// `process.kill(pid[, signal])`.
fn kill_body(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    let pid_value = call.argument(0);
    let pid_number = if pid_value.is_number() { pid_value.as_number() } else { f64::NAN };
    if pid_number.fract() != 0.0 || !pid_number.is_finite() || pid_number.abs() > f64::from(i32::MAX) {
        let message = format!("The \"pid\" argument must be of type number. Received {}", received_description(global_object, pid_value));
        return Err(throw_coded_type_error(global_object, &message, "ERR_INVALID_ARG_TYPE"));
    }
    let pid = pid_number as i32;
    let signal_value = call.argument(1);
    let signal = if signal_value.is_undefined() {
        15
    } else if signal_value.is_number() {
        signal_value.as_number() as i32
    } else {
        let text = if signal_value.is_string() { rust_string(&signal_value.as_js_string().value()) } else { rust_string(&signal_value.to_string(global_object.vm()).value()) };
        match SIGNALS.iter().find(|(name, _)| signal_value.is_string() && *name == text) {
            Some((_, number)) => *number,
            None => return Err(throw_coded_type_error(global_object, &format!("Unknown signal: {text}"), "ERR_UNKNOWN_SIGNAL")),
        }
    };
    match send_signal(global_object, pid, signal)? {
        Ok(()) => Ok(JSValue::Bool(true)),
        Err((errno, code, description)) => Err(throw_system_error(global_object, "kill", errno, code, description)),
    }
}
host_function!(pub kill_function, kill_body);

/// `process._kill(pid, signal)`: o `kill(2)` cru. Os dois argumentos viram inteiros (`ToInt32`, texto ilegível vira 0) e o
/// resultado é 0 ou o `errno` positivo (3 `ESRCH`, 1 `EPERM`), sem lançar; sem argumentos lança `Error: Not enough
/// arguments`. Medido no bun 1.4.2.
fn raw_kill_body(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    if call.argument_count() == 0 {
        return Err(throw_error_with_properties(global_object, "Not enough arguments", &[]));
    }
    let outcome = send_signal(global_object, call.argument(0).to_int32(), call.argument(1).to_int32())?;
    Ok(js_number(outcome.err().map_or(0, |(errno, _, _)| errno)))
}
host_function!(pub raw_kill_function, raw_kill_body);

/// `process.abort()`: o programa morre por `SIGABRT`, sem emitir `exit`.
fn abort_body(global_object: &JSGlobalObject, _call: &HostCall) -> HostResult {
    die_by_signal(global_object, 6)
}
host_function!(pub abort_function, abort_body);
