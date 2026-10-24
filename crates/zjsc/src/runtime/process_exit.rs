//! `process.exit`, `process.exitCode`, `process.reallyExit`, os eventos `exit` e `beforeExit` e o código de saída final
//! (fatia 3 de `wip/notes/process-plan.md`, regras 10 a 16, medidas no bun 1.4.2). Também instala o mínimo do
//! emissor de eventos do `process` (`on`, `addListener`, `once`, `off`, `removeListener`, `emit`, `listenerCount`) sobre
//! `event_emitter_core`; a fatia 5 completa o resto.
//!
//! - `exitCode` é um acessor enumerável e NÃO configurável. Setar `undefined` ou `null` não faz nada (não limpa um valor
//!   anterior). Número inteiro ou texto numérico não vazio é aceito; o valor guardado é o inteiro e a leitura devolve
//!   `valor mod 256` (300 vira 44, -1 vira 255). Outros tipos lançam `TypeError ERR_INVALID_ARG_TYPE`; número não
//!   inteiro ou fora de ±(2^53-1) lança `RangeError ERR_OUT_OF_RANGE`.
//! - `process.exit(code)` valida como `exitCode`, grava o código, emite `exit` uma vez (uma chamada dentro do ouvinte não
//!   reentra) e termina o programa com a exceção de terminação do VM: nada mais do usuário roda (ticks, microtarefas,
//!   timers e immediates pendentes são descartados; o laço de eventos para).
//! - Ao fim natural (laço vazio) sai `beforeExit` com o código atual; timer ou immediate agendado nele revive o laço e o
//!   evento se repete; o `nextTick` agendado nele roda uma vez; promessas e microtarefas dele nunca rodam. Depois sai
//!   `exit`, e o que se agenda dentro dele não roda.
//!
//! DIVERGÊNCIA: o nome do evento é um texto (símbolo ainda não); `exit` e `beforeExit` em módulo ESM com `await` no topo
//! ficam para a fatia dos módulos.

use std::cell::{Cell, RefCell};

use crate::host_function;
use crate::runtime::event_emitter_core::{emit, ListenerTable};
use crate::runtime::host_call::{HostCall, HostResult, Thrown};
use crate::runtime::intl_support::prop;
use crate::runtime::js_array::construct_array;
use crate::runtime::js_global_object::JSGlobalObject;
use crate::runtime::js_string::js_string;
use crate::runtime::js_module_loader::rust_string;
use crate::runtime::js_object::JSObject;
use crate::runtime::js_promise_host::Thrown as CallThrown;
use crate::runtime::js_value::{js_number, JSValue};
use crate::runtime::js_value_conversions::number_to_string_radix10;
use crate::runtime::js_web_assembly::received_description;
use crate::runtime::native_function::NativeFunction;
use crate::runtime::error_type::ErrorType;
use crate::runtime::node_error::{throw_coded_error, throw_coded_range_error, throw_coded_type_error, throw_validation_error};
use crate::runtime::process_object::{run_pending_ticks, set_processing_ticks};
use crate::runtime::process_warning::print_warning;
use crate::runtime::throw_scope::{throw_exception, ThrowScope};
use crate::runtime::timers::{halt_event_loop, native, put_accessor};
use crate::runtime::vm::VM;

/// `Number.MAX_SAFE_INTEGER`.
const MAX_SAFE_INTEGER: f64 = 9_007_199_254_740_991.0;

#[derive(Default)]
struct ExitState {
    /// O inteiro gravado por `exitCode` ou `exit(code)`; a leitura aplica `mod 256`.
    exit_code: Option<i64>,
    /// O evento `exit` já foi (ou está sendo) emitido: não reentra.
    exiting: bool,
    /// `exit()`/`reallyExit()` terminaram o programa.
    requested: bool,
    /// Uma exceção fatal foi relatada: o evento `exit` vê o código 1.
    fatal: bool,
    process: Option<JSValue>,
    /// O callback de `setUncaughtExceptionCaptureCallback`.
    capture: Option<JSValue>,
    /// Um ouvinte (de `uncaughtException` ou do monitor: código 7) ou o callback de captura (código 1) lançou: é o código
    /// de saída do processo e o que o evento `exit` vê.
    failure_code: Option<i32>,
    /// O sinal que matou o programa (`die_by_signal`).
    killed_by: Option<i32>,
    /// As promessas (`cell_id`) já entregues a `unhandledRejection`: um handler novo nelas emite `rejectionHandled`.
    notified: Vec<usize>,
}

thread_local! {
    static EXIT: RefCell<ExitState> = RefCell::new(ExitState::default());
    static LISTENERS: RefCell<ListenerTable> = RefCell::new(ListenerTable::default());
    /// `process.setMaxListeners(n)`; o padrão do node é 10.
    static MAX_LISTENERS: Cell<f64> = const { Cell::new(10.0) };
}

/// Fim do programa (`cell_registry::reset_program_state`).
pub(crate) fn reset_for_program() {
    let _ = EXIT.try_with(|state| *state.borrow_mut() = ExitState::default());
    let _ = LISTENERS.try_with(|table| *table.borrow_mut() = ListenerTable::default());
    let _ = MAX_LISTENERS.try_with(|max| max.set(10.0));
    crate::runtime::process_warning::reset_for_program();
}

/// `process.exit()` ou `reallyExit()` já terminaram o programa?
pub(crate) fn exit_requested() -> bool {
    EXIT.with(|state| state.borrow().requested)
}

/// Uma exceção fatal foi relatada: o evento `exit` passa a ver o código 1.
pub(crate) fn mark_fatal() {
    EXIT.with(|state| state.borrow_mut().fatal = true);
}

fn current_code() -> i64 {
    EXIT.with(|state| state.borrow().exit_code.map_or(0, |code| code.rem_euclid(256)))
}

/// O código de saída do processo quando nada fatal foi relatado.
pub(crate) fn resolved_exit_code() -> i32 {
    current_code() as i32
}

pub(crate) fn number_text(vm: &VM, number: f64) -> String {
    rust_string(&number_to_string_radix10(vm, number).value())
}

/// `addNumericalSeparator` do node: grupos de três dígitos separados por `_` (o `1e+300` vira `1e+_300`, como no bun).
fn with_separators(text: &str) -> String {
    let start = usize::from(text.starts_with('-'));
    let mut result = String::new();
    let mut end = text.len();
    while end >= start + 4 {
        result = format!("_{}{result}", &text[end - 3..end]);
        end -= 3;
    }
    format!("{}{result}", &text[..end])
}

fn invalid_code_type(global_object: &JSGlobalObject, value: JSValue) -> Thrown {
    let message = format!("The \"code\" argument must be of type number. Received {}", received_description(global_object, value));
    throw_validation_error(global_object, ErrorType::TypeError, &message, "ERR_INVALID_ARG_TYPE")
}

/// A validação de `exitCode` e de `exit(code)`: `Ok(None)` para `undefined` e `null`.
fn parse_exit_code(global_object: &JSGlobalObject, value: JSValue) -> Result<Option<i64>, Thrown> {
    if value.is_undefined_or_null() {
        return Ok(None);
    }
    let numeric_text = value.is_string() && !rust_string(&value.as_js_string().value()).trim().is_empty() && !value.to_number().is_nan();
    let number = if value.is_number() {
        value.as_number()
    } else if numeric_text {
        value.to_number()
    } else {
        return Err(invalid_code_type(global_object, value));
    };
    let vm = global_object.vm();
    if !number.is_finite() || number.fract() != 0.0 {
        let message = format!("The value of \"code\" is out of range. It must be an integer. Received {}", number_text(vm, number));
        return Err(throw_validation_error(global_object, ErrorType::RangeError, &message, "ERR_OUT_OF_RANGE"));
    }
    if number.abs() > MAX_SAFE_INTEGER {
        let shown = with_separators(&number_text(vm, number));
        let message = format!(
            "The value of \"code\" is out of range. It must be >= -9007199254740991 && <= 9007199254740991. Received {shown}"
        );
        return Err(throw_validation_error(global_object, ErrorType::RangeError, &message, "ERR_OUT_OF_RANGE"));
    }
    Ok(Some(number as i64))
}

fn exit_code_getter_body(_global_object: &JSGlobalObject, _call: &HostCall) -> HostResult {
    Ok(EXIT.with(|state| state.borrow().exit_code).map_or_else(JSValue::undefined, |code| js_number(code.rem_euclid(256) as i32)))
}
host_function!(exit_code_getter, exit_code_getter_body);

fn exit_code_setter_body(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    if let Some(code) = parse_exit_code(global_object, call.argument(0))? {
        EXIT.with(|state| state.borrow_mut().exit_code = Some(code));
    }
    Ok(JSValue::undefined())
}
host_function!(exit_code_setter, exit_code_setter_body);

/// Relança no VM o desfecho de uma chamada de ouvinte.
pub(crate) fn rethrow(global_object: &JSGlobalObject, thrown: CallThrown) -> Thrown {
    let vm = global_object.vm();
    let mut scope = ThrowScope::new(vm);
    match thrown {
        CallThrown::Value(value) => throw_exception(global_object, &mut scope, value),
        CallThrown::Termination => throw_exception(global_object, &mut scope, vm.ensure_termination_exception()),
    };
    Thrown::Pending
}

/// Termina o programa: o laço de eventos para e a exceção de terminação desfaz a pilha do usuário.
fn terminate(global_object: &JSGlobalObject) -> HostResult {
    EXIT.with(|state| {
        let mut state = state.borrow_mut();
        state.requested = true;
        state.exiting = true;
    });
    halt_event_loop();
    Err(rethrow(global_object, CallThrown::Termination))
}

/// Um erro fatal do próprio bun, sem exceção JavaScript (`Bun.serve` que recebe `undefined` do handler em
/// desenvolvimento): `error: <mensagem>`, a linha em branco e o rodapé no stderr, código de saída 1 e o programa
/// termina como numa exceção não capturada (medido no bun 1.4.2).
pub(crate) fn fatal_message(global_object: &JSGlobalObject, message: &str) -> HostResult {
    crate::runtime::uncaught_report::finish_report(global_object, format!("error: {message}\n").as_bytes());
    mark_fatal();
    EXIT.with(|state| state.borrow_mut().exit_code = Some(1));
    terminate(global_object)
}

/// O objeto `process` registrado (`undefined` antes da instalação).
pub(crate) fn process_value() -> JSValue {
    EXIT.with(|state| state.borrow().process).unwrap_or_else(JSValue::undefined)
}

/// Emite `event` no `process`. O evento `warning` com um aviso imprime o texto padrão no stderr antes dos ouvintes.
pub(crate) fn emit_event(global_object: &JSGlobalObject, event: &str, args: &[JSValue]) -> Result<bool, CallThrown> {
    let process = process_value();
    if event == "warning" && args.first().is_some_and(|value| value.is_cell()) {
        print_warning(global_object, args[0]);
    }
    emit(global_object, &LISTENERS, process, event, args)
}

/// O programa morre pelo sinal `number` (código 128 + n, como o shell mostra), sem emitir `exit`.
pub(crate) fn die_by_signal(global_object: &JSGlobalObject, number: i32) -> HostResult {
    EXIT.with(|state| {
        let mut state = state.borrow_mut();
        state.exit_code = Some(i64::from(128 + number));
        state.killed_by = Some(number);
    });
    terminate(global_object)
}

/// O sinal que matou o programa, se ele morreu por um (`process.kill` no próprio pid sem ouvinte, `process.abort()`).
pub fn killing_signal() -> Option<i32> {
    EXIT.with(|state| state.borrow().killed_by)
}

/// Um sinal enviado ao próprio processo: os ouvintes de `name` recebem `(name, number)`; sem ouvinte o programa morre
/// pelo sinal.
pub(crate) fn deliver_signal(global_object: &JSGlobalObject, name: &str, number: i32) -> Result<(), Thrown> {
    if LISTENERS.with(|table| table.borrow().count(name)) == 0 {
        return die_by_signal(global_object, number).map(|_| ());
    }
    let name_value = JSValue::from_js_string(js_string(global_object.vm(), &crate::wtf::text::wtf_string::String::from_utf8(name.as_bytes())));
    emit_event(global_object, name, &[name_value, js_number(number)]).map(|_| ()).map_err(|thrown| rethrow(global_object, thrown))
}

/// Emite `event` para o programa: uma exceção do ouvinte vira exceção fatal relatada; a terminação é ignorada.
fn emit_or_report(global_object: &JSGlobalObject, event: &str, args: &[JSValue]) {
    if let Err(CallThrown::Value(error)) = emit_event(global_object, event, args) {
        global_object.vm().report_unhandled_error(global_object, error);
    }
}

/// `exit` e `beforeExit`: um ouvinte que lança vai a `uncaughtException` como qualquer erro do laço; sem tratamento o
/// relato sai sem o rodapé e o código é 1 (medido no bun 1.4.2: `process.on('exit', () => { throw ... })` e o mesmo
/// com `beforeExit` terminam com o trecho, a pilha e `exit 1`, sem `Bun v1.4.2 (Linux x64)`).
fn emit_exit_event(global_object: &JSGlobalObject, event: &str, code: i32) {
    if let Err(CallThrown::Value(error)) = emit_event(global_object, event, &[js_number(code)]) {
        let handled = LISTENERS.with(|table| table.borrow().count("uncaughtException")) > 0 || EXIT.with(|state| state.borrow().capture.is_some());
        if !handled && handler_failure_code().is_none() {
            mark_handler_failed(EXIT_HANDLER_FAILURE_CODE);
        }
        global_object.vm().report_unhandled_error(global_object, error);
    }
}

/// Código de saída quando um ouvinte de `exit` ou `beforeExit` lança.
const EXIT_HANDLER_FAILURE_CODE: i32 = 1;

/// O desfecho de uma exceção não capturada oferecida aos ouvintes do usuário.
pub(crate) enum Dispatch {
    /// Um ouvinte (ou o callback de captura) tratou: o programa segue.
    Handled,
    /// Ninguém tratou: o relato fatal padrão.
    NotHandled,
    /// O ouvinte lançou este novo erro: relato sem rodapé e saída com o código dado (7 para ouvinte e monitor, 1 para o
    /// callback de captura).
    HandlerThrew(JSValue, i32),
}

/// Código de saída quando um ouvinte (de `uncaughtException` ou do monitor) lança.
const LISTENER_FAILURE_CODE: i32 = 7;
/// Código de saída quando o callback de captura lança.
const CAPTURE_FAILURE_CODE: i32 = 1;

/// Oferece `error` ao `uncaughtExceptionMonitor`, ao callback de captura e a `uncaughtException`, nessa ordem (o
/// callback de captura tem prioridade sobre os ouvintes). `origin` é `'uncaughtException'` ou `'unhandledRejection'`.
pub(crate) fn dispatch_uncaught(global_object: &JSGlobalObject, error: JSValue, origin: &str) -> Dispatch {
    let origin_value = JSValue::from_js_string(crate::runtime::js_string::js_string(
        global_object.vm(),
        &crate::wtf::text::wtf_string::String::from_latin1(origin.as_bytes()),
    ));
    let args = [error, origin_value];
    // O monitor roda antes de tudo. Se ele lança, o erro novo é relatado e o processo sai com 7 sem chamar o callback de
    // captura nem os ouvintes; se ele chama `process.exit()`, o programa já terminou.
    match emit_event(global_object, "uncaughtExceptionMonitor", &args) {
        Err(CallThrown::Value(thrown)) => return Dispatch::HandlerThrew(thrown, LISTENER_FAILURE_CODE),
        Err(CallThrown::Termination) => return Dispatch::Handled,
        Ok(_) => {}
    }
    let capture = EXIT.with(|state| state.borrow().capture);
    let outcome = if let Some(callback) = capture {
        crate::runtime::js_microtask::call_microtask(global_object, callback, JSValue::undefined(), &[error], "callback is not a function").map(|_| true)
    } else {
        emit_event(global_object, "uncaughtException", &args)
    };
    match outcome {
        Ok(true) => Dispatch::Handled,
        Ok(false) => Dispatch::NotHandled,
        Err(CallThrown::Value(thrown)) => {
            Dispatch::HandlerThrew(thrown, if capture.is_some() { CAPTURE_FAILURE_CODE } else { LISTENER_FAILURE_CODE })
        }
        // `process.exit()` dentro do ouvinte: o programa já terminou.
        Err(CallThrown::Termination) => Dispatch::Handled,
    }
}

/// O ouvinte de exceção (ou o callback de captura) lançou: o relato do novo erro sai sem rodapé e o código de saída é
/// `code`.
pub(crate) fn mark_handler_failed(code: i32) {
    EXIT.with(|state| state.borrow_mut().failure_code = Some(code));
}

/// O código de saída fixado por um ouvinte que lançou, se algum lançou.
pub(crate) fn handler_failure_code() -> Option<i32> {
    EXIT.with(|state| state.borrow().failure_code)
}

/// Uma rejeição sem tratador chegou ao fim da drenagem. Com ouvinte de `unhandledRejection` entrega `(reason, promise)` e
/// devolve `true`; um erro lançado pelo ouvinte vai para `uncaughtException` (origem `'uncaughtException'`) ou, sem
/// ouvinte, vira erro fatal. Sem ouvinte devolve `false` e quem chamou aplica o padrão (relato e código 1).
pub(crate) fn deliver_unhandled_rejection(
    global_object: &JSGlobalObject,
    promise: JSValue,
    reason: JSValue,
    fatal: &dyn Fn(&JSGlobalObject, JSValue),
) -> bool {
    if LISTENERS.with(|table| table.borrow().count("unhandledRejection")) == 0 {
        return false;
    }
    if let JSValue::Cell(cell_id) = promise {
        EXIT.with(|state| state.borrow_mut().notified.push(cell_id));
    }
    if let Err(CallThrown::Value(thrown)) = emit_event(global_object, "unhandledRejection", &[reason, promise]) {
        report_from_event_loop(global_object, thrown, fatal);
    }
    true
}

/// Um erro que escapou de um callback do laço de eventos: ouvintes primeiro, depois o relato fatal (`fatal`).
pub(crate) fn report_from_event_loop(global_object: &JSGlobalObject, error: JSValue, fatal: &dyn Fn(&JSGlobalObject, JSValue)) {
    match dispatch_uncaught(global_object, error, "uncaughtException") {
        Dispatch::Handled => {}
        Dispatch::NotHandled => fatal(global_object, error),
        Dispatch::HandlerThrew(thrown, code) => {
            mark_handler_failed(code);
            fatal(global_object, thrown);
        }
    }
}

/// Um handler foi posto numa promessa rejeitada: se ela já foi entregue a `unhandledRejection`, emite `rejectionHandled`.
pub(crate) fn promise_handled(global_object: &JSGlobalObject, cell_id: usize) {
    let known = EXIT.with(|state| {
        let mut state = state.borrow_mut();
        state.notified.iter().position(|id| *id == cell_id).map(|position| state.notified.remove(position)).is_some()
    });
    if known {
        emit_or_report(global_object, "rejectionHandled", &[JSValue::Cell(cell_id)]);
    }
}

fn set_capture_body(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    let callback = call.argument(0);
    if callback.is_null() {
        EXIT.with(|state| state.borrow_mut().capture = None);
        return Ok(JSValue::undefined());
    }
    if !callback.is_callable() {
        let message = format!("The \"fn\" argument must be of type function or null. Received {}", received_description(global_object, callback));
        return Err(throw_coded_type_error(global_object, &message, "ERR_INVALID_ARG_TYPE"));
    }
    if EXIT.with(|state| state.borrow().capture.is_some()) {
        let message = "`process.setupUncaughtExceptionCapture()` was called while a capture callback was already active";
        return Err(throw_coded_error(global_object, message, "ERR_UNCAUGHT_EXCEPTION_CAPTURE_ALREADY_SET"));
    }
    EXIT.with(|state| state.borrow_mut().capture = Some(callback));
    Ok(JSValue::undefined())
}
host_function!(process_set_capture, set_capture_body);

fn has_capture_body(_global_object: &JSGlobalObject, _call: &HostCall) -> HostResult {
    Ok(JSValue::Bool(EXIT.with(|state| state.borrow().capture.is_some())))
}
host_function!(process_has_capture, has_capture_body);

fn exit_body(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    if let Some(code) = parse_exit_code(global_object, call.argument(0))? {
        EXIT.with(|state| state.borrow_mut().exit_code = Some(code));
    }
    let first = EXIT.with(|state| !std::mem::replace(&mut state.borrow_mut().exiting, true));
    if first {
        emit_or_report(global_object, "exit", &[js_number(current_code() as i32)]);
    }
    terminate(global_object)
}
host_function!(process_exit, exit_body);

/// `process.reallyExit(code)`: sai sem emitir `exit`.
fn really_exit_body(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    let code = call.argument(0);
    let number = if code.is_number() { code.as_number() } else { 0.0 };
    EXIT.with(|state| state.borrow_mut().exit_code = Some(if number.is_finite() { number as i64 } else { 0 }));
    terminate(global_object)
}
host_function!(process_really_exit, really_exit_body);

/// Fim natural do programa: emite `exit` (se `exit()` ainda não emitiu); o que os ouvintes agendam não roda.
pub(crate) fn emit_exit_at_end(global_object: &JSGlobalObject) {
    let first = EXIT.with(|state| !std::mem::replace(&mut state.borrow_mut().exiting, true));
    if first {
        let code = handler_failure_code().unwrap_or_else(|| if EXIT.with(|state| state.borrow().fatal) { 1 } else { current_code() as i32 });
        emit_exit_event(global_object, "exit", code);
    }
}

/// O laço de eventos ficou vazio: emite `beforeExit` e roda os ticks que os ouvintes agendaram (as microtarefas deles
/// nunca rodam). Quem chama olha depois se o laço reviveu.
pub(crate) fn emit_before_exit(global_object: &JSGlobalObject) {
    emit_exit_event(global_object, "beforeExit", current_code() as i32);
    let was_processing = set_processing_ticks(true);
    run_pending_ticks(global_object.vm());
    set_processing_ticks(was_processing);
}

/// A chave de texto de um nome de evento.
pub(crate) fn event_key(global_object: &JSGlobalObject, value: JSValue) -> String {
    if value.is_string() {
        rust_string(&value.as_js_string().value())
    } else if value.is_number() {
        number_text(global_object.vm(), value.as_number())
    } else {
        String::from("\u{0}unsupported")
    }
}

/// Um nome de evento como valor JS (texto latin1).
pub(crate) fn event_value(global_object: &JSGlobalObject, name: &str) -> JSValue {
    JSValue::from_js_string(js_string(global_object.vm(), &crate::wtf::text::wtf_string::String::from_latin1(name.as_bytes())))
}

/// `process.on` e irmãos: valida o ouvinte, avisa `newListener` (quando alguém escuta) e registra no fim ou no começo.
/// O `process` não emite `MaxListenersExceededWarning` (regra 27 do plano), então o limite só é guardado.
fn add_listener(global_object: &JSGlobalObject, call: &HostCall, once: bool, prepend: bool) -> HostResult {
    let listener = call.argument(1);
    if !listener.is_callable() {
        let message = format!("The \"listener\" argument must be of type function. Received {}", received_description(global_object, listener));
        return Err(throw_coded_type_error(global_object, &message, "ERR_INVALID_ARG_TYPE"));
    }
    let event = event_key(global_object, call.argument(0));
    if LISTENERS.with(|table| table.borrow().count("newListener")) > 0 {
        if let Err(thrown) = emit_event(global_object, "newListener", &[call.argument(0), listener]) {
            return Err(rethrow(global_object, thrown));
        }
    }
    LISTENERS.with(|table| {
        let mut table = table.borrow_mut();
        if prepend {
            table.add_front(&event, listener, once);
        } else {
            table.add(&event, listener, once);
        }
    });
    Ok(call.this_value())
}

fn on_body(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    add_listener(global_object, call, false, false)
}
host_function!(process_on, on_body);

fn once_body(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    add_listener(global_object, call, true, false)
}
host_function!(process_once, once_body);

fn prepend_body(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    add_listener(global_object, call, false, true)
}
host_function!(process_prepend_listener, prepend_body);

fn prepend_once_body(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    add_listener(global_object, call, true, true)
}
host_function!(process_prepend_once_listener, prepend_once_body);

fn off_body(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    let event = event_key(global_object, call.argument(0));
    let removed = LISTENERS.with(|table| table.borrow_mut().remove(&event, call.argument(1)));
    if removed && LISTENERS.with(|table| table.borrow().count("removeListener")) > 0 {
        if let Err(thrown) = emit_event(global_object, "removeListener", &[call.argument(0), call.argument(1)]) {
            return Err(rethrow(global_object, thrown));
        }
    }
    Ok(call.this_value())
}
host_function!(process_off, off_body);

fn remove_all_body(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    let event = if call.argument(0).is_undefined() { None } else { Some(event_key(global_object, call.argument(0))) };
    let removed = LISTENERS.with(|table| table.borrow_mut().remove_all(event.as_deref()));
    if LISTENERS.with(|table| table.borrow().count("removeListener")) > 0 {
        for (name, function) in removed.into_iter().rev() {
            let name = event_value(global_object, &name);
            if let Err(thrown) = emit_event(global_object, "removeListener", &[name, function]) {
                return Err(rethrow(global_object, thrown));
            }
        }
    }
    Ok(call.this_value())
}
host_function!(process_remove_all_listeners, remove_all_body);

fn array_value(global_object: &JSGlobalObject, values: &[JSValue]) -> JSValue {
    construct_array(global_object.vm(), &global_object.array_structure(), values).as_value()
}

/// `eventNames()`: `'warning'` vem primeiro porque o bun registra um ouvinte interno dele antes do script.
fn event_names_body(global_object: &JSGlobalObject, _call: &HostCall) -> HostResult {
    let mut names = vec![String::from("warning")];
    names.extend(LISTENERS.with(|table| table.borrow().event_names()).into_iter().filter(|name| name != "warning"));
    let values: Vec<JSValue> = names.iter().map(|name| event_value(global_object, name)).collect();
    Ok(array_value(global_object, &values))
}
host_function!(process_event_names, event_names_body);

/// `listeners(event)` e `rawListeners(event)`: sem embrulho de `once`, as duas devolvem as funções registradas.
fn listeners_body(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    let event = event_key(global_object, call.argument(0));
    let functions = LISTENERS.with(|table| table.borrow().functions(&event));
    Ok(array_value(global_object, &functions))
}
host_function!(process_listeners, listeners_body);

fn set_max_listeners_body(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    let value = call.argument(0);
    if !value.is_number() || value.as_number().is_nan() || value.as_number() < 0.0 {
        let message = format!(
            "The value of \"n\" is out of range. It must be a non-negative number. Received {}",
            received_description(global_object, value)
        );
        return Err(throw_validation_error(global_object, ErrorType::RangeError, &message, "ERR_OUT_OF_RANGE"));
    }
    MAX_LISTENERS.with(|max| max.set(value.as_number()));
    Ok(call.this_value())
}
host_function!(process_set_max_listeners, set_max_listeners_body);

fn get_max_listeners_body(_global_object: &JSGlobalObject, _call: &HostCall) -> HostResult {
    Ok(js_number(MAX_LISTENERS.with(Cell::get)))
}
host_function!(process_get_max_listeners, get_max_listeners_body);

fn emit_body(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    let event = event_key(global_object, call.argument(0));
    let args = call.arguments().get(1..).unwrap_or_default();
    match emit_event(global_object, &event, args) {
        Ok(had_listeners) => Ok(JSValue::Bool(had_listeners)),
        Err(thrown) => Err(rethrow(global_object, thrown)),
    }
}
host_function!(process_emit, emit_body);

fn listener_count_body(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    let event = event_key(global_object, call.argument(0));
    Ok(js_number(LISTENERS.with(|table| table.borrow().count(&event)) as i32))
}
host_function!(process_listener_count, listener_count_body);

/// Os métodos próprios do `process` que esta fatia de `exit` e captura define: `(nome, length, função)`.
pub(crate) fn own_function(name: &str) -> Option<(u32, NativeFunction)> {
    match name {
        "setUncaughtExceptionCaptureCallback" => Some((1, process_set_capture as NativeFunction)),
        "hasUncaughtExceptionCaptureCallback" => Some((0, process_has_capture as NativeFunction)),
        "exit" => Some((1, process_exit as NativeFunction)),
        "reallyExit" => Some((1, process_really_exit as NativeFunction)),
        _ => None,
    }
}

/// `delete process.exitCode` (o objeto `process`): o bun lança `TypeError` mesmo em modo frouxo (o acessor não é configurável).
pub(crate) fn is_process_object(object: &JSObject) -> bool {
    EXIT.with(|state| state.borrow().process).is_some_and(|process| process.is_cell() && process.as_cell() == object.cell_id())
}

/// Registra o `process` do `exit` e instala o acessor `exitCode` (enumerável, não configurável).
pub(crate) fn install_exit_code(global_object: &JSGlobalObject, process: &JSObject) {
    EXIT.with(|state| state.borrow_mut().process = Some(process.as_value()));
    put_accessor(global_object, process, "exitCode", exit_code_getter, Some(exit_code_setter));
}

/// Instala no protótipo do `process` os métodos de `EventEmitter`, na ordem do bun (enumeráveis).
pub(crate) fn install_emitter_methods(global_object: &JSGlobalObject, prototype: &JSObject) {
    let vm = global_object.vm();
    let methods: [(&str, u32, NativeFunction); 15] = [
        ("addListener", 2, process_on as NativeFunction),
        ("on", 2, process_on as NativeFunction),
        ("once", 2, process_once as NativeFunction),
        ("prependListener", 2, process_prepend_listener as NativeFunction),
        ("prependOnceListener", 2, process_prepend_once_listener as NativeFunction),
        ("removeListener", 2, process_off as NativeFunction),
        ("off", 2, process_off as NativeFunction),
        ("removeAllListeners", 1, process_remove_all_listeners as NativeFunction),
        ("emit", 1, process_emit as NativeFunction),
        ("eventNames", 0, process_event_names as NativeFunction),
        ("listenerCount", 1, process_listener_count as NativeFunction),
        ("listeners", 1, process_listeners as NativeFunction),
        ("rawListeners", 1, process_listeners as NativeFunction),
        ("setMaxListeners", 1, process_set_max_listeners as NativeFunction),
        ("getMaxListeners", 0, process_get_max_listeners as NativeFunction),
    ];
    for (name, length, function) in methods {
        prototype.put_direct(vm, &prop(vm, name), native(global_object, name, length, function), 0);
    }
}

