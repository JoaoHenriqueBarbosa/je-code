//! O global `process` do bun, por fatias. Esta traz o objeto e `process.nextTick`; o resto das propriedades
//! entra nas próximas fatias de `wip/notes/process-plan.md`, medidas no bun 1.4.2, sem propriedade inventada.
//!
//! `process.nextTick(callback, ...args)`: propriedade de dados comum (`writable`, `enumerable`, `configurable`),
//! `length` 1, nativa (sem `prototype`). Valida só o `callback`: o que não é função lança `TypeError`
//! `The "callback" argument must be of type function. Received ...` com `code` `ERR_INVALID_ARG_TYPE` (no
//! protótipo do erro, não próprio). Devolve `undefined`. O callback roda com `this` `undefined` e os argumentos
//! extras; uma classe só falha dentro do tick (`Cannot call a class constructor`).
//!
//! A fila de ticks é FIFO e própria; quem a drena é `VM::drain_microtasks` (`microtask_queue.rs`), que conhece os
//! dois modos de drenagem medidos. Exceção de um tick vai ao relato de erro não capturado e os ticks seguintes
//! continuam.
//!
//! O `this` `undefined` de um `function` sem diretiva vem do wrapper CJS estrito do bun (medido: uma chamada
//! comum `f()` também vê `undefined` ali), não do `nextTick`; aqui o tick chama com `this` `undefined` cru e o
//! `toThis` só age em código realmente sloppy.

use std::cell::RefCell;
use std::collections::VecDeque;

use crate::host_function;
use crate::runtime::host_call::{HostCall, HostResult};
use crate::runtime::js_global_object::JSGlobalObject;
use crate::runtime::js_microtask::call_microtask;
use crate::runtime::js_promise_host::Thrown as CallThrown;
use crate::runtime::js_scope::{JSScope, JSScopeRef};
use crate::runtime::js_value::JSValue;
use crate::runtime::js_web_assembly::received_description;
use crate::runtime::native_class_support::install_global;
use crate::runtime::node_error::throw_coded_type_error;
use crate::runtime::vm::VM;

/// Um `process.nextTick` pendente.
struct Tick {
    /// `cell_id` do global em que foi agendado.
    global_object: usize,
    callback: JSValue,
    args: Vec<JSValue>,
    /// O contexto assíncrono do agendamento (`AsyncContextFrame::withAsyncContextIfNeeded`), vazio sem rastreamento.
    async_context: JSValue,
}

#[derive(Default)]
struct TickState {
    queue: VecDeque<Tick>,
    /// `true` enquanto os ticks rodam (ou a drenagem que eles dirigem): microtarefa nova não dispara tick no meio.
    processing: bool,
}

thread_local! {
    static TICKS: RefCell<TickState> = RefCell::new(TickState::default());
}

/// Fim do programa: os ticks pendentes guardam valores de um global que já caiu.
pub(crate) fn reset_for_program() {
    // `try_with`: ver `process_stdio::reset_for_program`.
    let _ = TICKS.try_with(|state| *state.borrow_mut() = TickState::default());
}

/// Há tick esperando?
pub(crate) fn has_pending_ticks() -> bool {
    TICKS.with(|state| !state.borrow().queue.is_empty())
}

/// As raízes da fila de `process.nextTick` para o coletor: o callback, os argumentos e o contexto assíncrono de cada
/// tick pendente (o `TickObject` do node segura os três até rodar).
pub(crate) fn visit_gc_roots(marker: &mut crate::runtime::gc::Marker) {
    let _ = TICKS.try_with(|state| {
        let Ok(state) = state.try_borrow() else {
            marker.abort();
            return;
        };
        for tick in &state.queue {
            marker.value(tick.callback);
            marker.values(&tick.args);
            marker.value(tick.async_context);
        }
    });
}

/// Estamos dentro da drenagem dirigida pelos ticks?
pub(crate) fn is_processing_ticks() -> bool {
    TICKS.with(|state| state.borrow().processing)
}

/// Liga ou desliga o modo ticks e devolve o valor anterior.
pub(crate) fn set_processing_ticks(on: bool) -> bool {
    TICKS.with(|state| std::mem::replace(&mut state.borrow_mut().processing, on))
}

/// Roda os ticks em FIFO até a fila esvaziar (os que os ticks agendam entram no mesmo laço).
pub(crate) fn run_pending_ticks(vm: &VM) {
    while let Some(tick) = TICKS.with(|state| state.borrow_mut().queue.pop_front()) {
        let Some(JSScopeRef::GlobalObject(global_object)) = JSScope::from_cell_id(tick.global_object) else {
            panic!("tick de um global object que não está no registro de células");
        };
        let _async_context_scope = crate::runtime::js_microtask::AsyncContextSwapScope::new(vm, tick.async_context);
        match call_microtask(&global_object, tick.callback, JSValue::undefined(), &tick.args, "callback is not a function") {
            Err(CallThrown::Value(error)) => vm.report_unhandled_error(&global_object, error),
            Err(CallThrown::Termination) => {
                TICKS.with(|state| state.borrow_mut().queue.clear());
                return;
            }
            Ok(_) => {}
        }
    }
}

/// `process.nextTick(callback, ...args)`.
fn next_tick_body(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    let callback = call.argument(0);
    if !callback.is_callable() {
        let message = format!(
            "The \"callback\" argument must be of type function. Received {}",
            received_description(global_object, callback)
        );
        return Err(throw_coded_type_error(global_object, &message, "ERR_INVALID_ARG_TYPE"));
    }
    let args = call.arguments().get(1..).unwrap_or_default().to_vec();
    queue_tick(global_object, callback, args);
    Ok(JSValue::undefined())
}

/// Agenda `callback(...args)` no fim da fila de ticks (o que `process.nextTick` faz depois de validar).
pub(crate) fn queue_tick(global_object: &JSGlobalObject, callback: JSValue, args: Vec<JSValue>) {
    let async_context = global_object.vm().captured_async_context();
    TICKS.with(|state| state.borrow_mut().queue.push_back(Tick { global_object: global_object.cell_id(), callback, args, async_context }));
}
host_function!(pub process_next_tick, next_tick_body);

/// Reserva o global `process` na posição do bun (entre `structuredClone` e `isNaN`). O objeto só pode ser montado
/// depois que o global tem as estruturas e os construtores que a forma usa (`Object`, `Set`, o `EventEmitter`): no
/// bun ele é uma `LazyProperty`, reificada no primeiro acesso. Aqui o valor entra em [`complete_process`], no fim do
/// `init`, na mesma entrada.
pub fn add_process(global_object: &JSGlobalObject) {
    install_global(global_object, "process", JSValue::undefined());
}

/// Monta o `process` (a forma, com chaves na ordem do bun, descritores e protótipo com o `EventEmitter`, está em
/// `process_shape.rs`) e grava na entrada reservada por [`add_process`].
pub fn complete_process(global_object: &JSGlobalObject) {
    let process = crate::runtime::process_shape::build_process(global_object);
    let vm = global_object.vm();
    let name = crate::runtime::property_name::PropertyName::from_identifier(&crate::runtime::identifier::Identifier::from_span(vm, b"process"));
    global_object.put_direct(vm, &name, process.as_value(), 0);
}
