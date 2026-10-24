//! `process.emitWarning` e o evento `warning` (fatia 9 de `wip/notes/process-plan.md`, regra 27, medida no bun 1.4.2).
//!
//! - Assinaturas: `emitWarning(warning, type?, code?, ctor?)` e `emitWarning(warning, options)` com `type`, `code` e
//!   `detail`. Um `type` ou `code` que é função vira o `ctor` (ignorado). `warning` é texto ou `Error`; outra coisa lança
//!   `ERR_INVALID_ARG_TYPE`, assim como um `type` ou `code` que não é texto (`undefined` vale como ausente).
//! - O aviso nascido de texto é um `Error` comum com `name` (padrão `Warning`) e, quando dados, `code` e `detail`
//!   próprios. Um `Error` recebido é usado como está.
//! - A entrega é num tick (`nextTick`): o evento `warning` sai depois do código síncrono e antes dos ticks e das
//!   promessas agendados depois. `process.noDeprecation` suprime só `DeprecationWarning` (o evento também) e
//!   `process.throwDeprecation` lança o aviso num tick (vira exceção não capturada).
//! - O texto no stderr sai a cada emissão do evento `warning` (também a manual por `process.emit`), antes dos ouvintes do
//!   usuário: `(node:PID) [CODE] Name: message`, a `detail` na linha seguinte e, só na primeira vez, a linha
//!   `(Use \`bun --trace-warnings ...\` to show where the warning was created)`. Com `process.traceDeprecation` o aviso
//!   de depreciação sai com a pilha no lugar de `Name: message`.

use std::cell::Cell;

use crate::host_function;
use crate::runtime::error_instance::ErrorInstance;
use crate::interpreter::call_frame::CallFrame;
use crate::runtime::error_natives::{capture_frames, put_message_property};
use crate::runtime::error_type::ErrorType;
use crate::runtime::host_call::{pending_or, HostCall, HostResult, Thrown};
use crate::runtime::iterator_operations::get_value_property;
use crate::runtime::js_global_object::JSGlobalObject;
use crate::runtime::js_module_loader::rust_string;
use crate::runtime::js_string::js_string;
use crate::runtime::js_value::JSValue;
use crate::runtime::js_web_assembly::received_description;
use crate::runtime::native_class_support::property_key;
use crate::runtime::node_error::throw_coded_type_error;
use crate::runtime::process_exit::{emit_event, process_value, rethrow};
use crate::runtime::process_object::queue_tick;
use crate::runtime::throw_scope::{throw_exception, ThrowScope};
use crate::runtime::timers::native;
use crate::wtf::text::wtf_string::String as WtfString;

thread_local! {
    /// A linha `(Use \`bun --trace-warnings ...\`...)` já saiu?
    static HINT_PRINTED: Cell<bool> = const { Cell::new(false) };
}

/// Fim do programa (`process_exit::reset_for_program`).
pub(crate) fn reset_for_program() {
    let _ = HINT_PRINTED.try_with(|printed| printed.set(false));
}

fn text_value(global_object: &JSGlobalObject, text: &str) -> JSValue {
    JSValue::from_js_string(js_string(global_object.vm(), &WtfString::from_utf8(text.as_bytes())))
}

fn property(global_object: &JSGlobalObject, value: JSValue, name: &str) -> Result<JSValue, Thrown> {
    get_value_property(global_object, value, &property_key(global_object.vm(), name))
}

fn text_of(global_object: &JSGlobalObject, value: JSValue) -> Result<String, Thrown> {
    Ok(rust_string(&pending_or(global_object, value.to_wtf_string())?))
}

/// Um objeto de opções: célula que não é texto nem função.
fn is_options(value: JSValue) -> bool {
    value.is_cell() && !value.is_string() && !value.is_callable()
}

fn invalid_type(global_object: &JSGlobalObject, name: &str, expected: &str, value: JSValue) -> Thrown {
    let message = format!("The \"{name}\" argument must be of type {expected}. Received {}", received_description(global_object, value));
    throw_coded_type_error(global_object, &message, "ERR_INVALID_ARG_TYPE")
}

/// `undefined` passa; qualquer outro valor que não seja texto lança.
fn validate_optional_string(global_object: &JSGlobalObject, name: &str, value: JSValue) -> Result<(), Thrown> {
    if value.is_undefined() || value.is_string() {
        Ok(())
    } else {
        Err(invalid_type(global_object, name, "string", value))
    }
}

fn is_deprecation(global_object: &JSGlobalObject, warning: JSValue) -> bool {
    property(global_object, warning, "name").ok().filter(JSValue::is_string).and_then(|name| text_of(global_object, name).ok()).as_deref()
        == Some("DeprecationWarning")
}

fn process_flag(global_object: &JSGlobalObject, name: &str) -> bool {
    property(global_object, process_value(), name).map(|value| value.to_boolean()).unwrap_or(false)
}

/// O texto que o bun escreve no stderr para o aviso `warning`.
pub(crate) fn print_warning(global_object: &JSGlobalObject, warning: JSValue) {
    let Some(host) = global_object.console_host() else { return };
    let field = |name: &str| property(global_object, warning, name).ok().filter(JSValue::is_string).and_then(|value| text_of(global_object, value).ok());
    let mut line = format!("(node:{}) ", crate::runtime::host::host().pid());
    if let Some(code) = field("code") {
        line.push_str(&format!("[{code}] "));
    }
    let traced = is_deprecation(global_object, warning) && process_flag(global_object, "traceDeprecation");
    match field("stack").filter(|_| traced) {
        Some(stack) => line.push_str(&stack),
        None => {
            let name = field("name").unwrap_or_else(|| String::from("Error"));
            let message = property(global_object, warning, "message").ok().and_then(|value| text_of(global_object, value).ok()).unwrap_or_default();
            line.push_str(&format!("{name}: {message}"));
        }
    }
    line.push('\n');
    if let Some(detail) = field("detail") {
        line.push_str(&detail);
        line.push('\n');
    }
    if !HINT_PRINTED.with(|printed| printed.replace(true)) {
        line.push_str("(Use `bun --trace-warnings ...` to show where the warning was created)\n");
    }
    host.write_stderr(line.as_bytes());
}

/// O tick que entrega o aviso: emite `warning` (o texto sai em `emit_event`).
fn emit_tick_body(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    emit_event(global_object, "warning", &[call.argument(0)]).map(|_| JSValue::undefined()).map_err(|thrown| rethrow(global_object, thrown))
}
host_function!(emit_tick, emit_tick_body);

/// O tick de `throwDeprecation`: lança o aviso.
fn throw_tick_body(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    let mut scope = ThrowScope::new(global_object.vm());
    throw_exception(global_object, &mut scope, call.argument(0));
    Err(Thrown::Pending)
}
host_function!(throw_tick, throw_tick_body);

/// O `Error` de um aviso nascido de texto: `name` (e `code`, `detail`) próprios e enumeráveis, nesta ordem.
fn build_warning(global_object: &JSGlobalObject, message: JSValue, kind: JSValue, code: JSValue, detail: JSValue) -> Result<JSValue, Thrown> {
    let vm = global_object.vm();
    let message = pending_or(global_object, message.to_wtf_string())?;
    let instance = ErrorInstance::create(vm, global_object.error_structure_for(ErrorType::Error), message.clone(), ErrorType::Error);
    if !message.is_empty() {
        put_message_property(vm, &instance, &message);
    }
    let name = if kind.is_string() { kind } else { text_value(global_object, "Warning") };
    instance.put_direct(vm, &property_key(vm, "name"), name, 0);
    if code.is_string() {
        instance.put_direct(vm, &property_key(vm, "code"), code, 0);
    }
    if detail.is_string() {
        instance.put_direct(vm, &property_key(vm, "detail"), detail, 0);
    }
    // O bun cria o aviso com `Error.captureStackTrace` dentro de `emitWarning`: a pilha parte do chamador.
    let top = vm.top_call_frame();
    if top != 0 {
        if let Some(frames) = capture_frames(global_object, CallFrame::create(top), None, false) {
            instance.set_pending_stack(frames);
        }
    }
    Ok(instance.as_value())
}

/// `process.emitWarning(warning, type, code, ctor)` / `process.emitWarning(warning, options)`.
fn emit_warning_body(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    let warning = call.argument(0);
    let (mut kind, mut code, mut detail) = (call.argument(1), call.argument(2), JSValue::undefined());
    if is_options(kind) {
        let options = kind;
        kind = property(global_object, options, "type")?;
        code = property(global_object, options, "code")?;
        detail = property(global_object, options, "detail")?;
    } else if kind.is_callable() {
        kind = JSValue::undefined();
        code = JSValue::undefined();
    } else if code.is_callable() {
        code = JSValue::undefined();
    }
    validate_optional_string(global_object, "type", kind)?;
    validate_optional_string(global_object, "code", code)?;
    let warning = if warning.is_string() {
        build_warning(global_object, warning, kind, code, detail)?
    } else if warning.is_cell() && ErrorInstance::from_cell_id(warning.as_cell()).is_some() {
        warning
    } else {
        return Err(invalid_type(global_object, "warning", "string or Error", warning));
    };
    let deprecation = is_deprecation(global_object, warning);
    if deprecation && process_flag(global_object, "noDeprecation") {
        return Ok(JSValue::undefined());
    }
    if deprecation && process_flag(global_object, "throwDeprecation") {
        queue_tick(global_object, native(global_object, "throwTick", 1, throw_tick), vec![warning]);
    } else {
        // No bun o tick que emite `warning` é JS interno (`node:process`), fora da pilha: o erro lançado por um ouvinte
        // de `warning` tem só o frame do ouvinte (medido no bun 1.4.2, sem `at emitTick`).
        let emit = crate::runtime::js_function::JSFunction::create_native_with_own_structure(
            global_object.vm(),
            global_object,
            1,
            &WtfString::from_latin1(b"emitTick"),
            emit_tick,
            crate::runtime::implementation_visibility::ImplementationVisibility::Private,
            crate::runtime::intrinsic::Intrinsic::NoIntrinsic,
            crate::runtime::js_function::call_host_function_as_constructor,
        );
        queue_tick(global_object, emit.as_value(), vec![warning]);
    }
    Ok(JSValue::undefined())
}
host_function!(pub process_emit_warning, emit_warning_body);

/// `process.emitWarning(message, type)` chamado pelo próprio runtime (os avisos de `setTimeout`). Um erro ao
/// emitir não é do chamador, então é descartado.
pub(crate) fn emit_runtime_warning(global_object: &JSGlobalObject, message: &str, kind: &str) {
    let arguments = vec![text_value(global_object, message), text_value(global_object, kind)];
    let _ = emit_warning_body(global_object, &HostCall::new(JSValue::undefined(), arguments));
}
