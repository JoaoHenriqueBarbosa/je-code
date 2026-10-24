//! Porte de `runtime/MicrotaskQueue.{h,cpp}` e `MicrotaskQueueInlines.h` (a fila padrão do `VM`,
//! `m_defaultMicrotaskQueue`) e de `VM::drainMicrotasks` (`VM.cpp`). A tarefa em si, a
//! `runInternalMicrotask`, é o `PromiseHost::run_internal_microtask` do `JSGlobalObject`
//! (`promise_constructor.rs`) com os corpos de `js_microtask.rs`.
//!
//! DIVERGÊNCIAS:
//! - `QueuedTask` guarda o `cell_id` do `JSGlobalObject` (`dispatcher()` sem a flag
//!   `isJSMicrotaskDispatcherFlag`) e os quatro argumentos. Os `JSMicrotaskDispatcher` (Bun, WebCore,
//!   `DebuggableMicrotaskDispatcher`) e o `Debugger` não existem; sem eles a tarefa é sempre "interna",
//!   `microtaskRunnability()` é sempre `Executed` (então `m_toKeep` e `QueuedTaskResult::Suspended` não
//!   se aplicam) e há só o global do `VM` (a troca de `currentGlobalObject` e o `VMEntryScope` por
//!   global não se aplicam).
//! - `reportUncaughtExceptionAtEventLoop` (`GlobalObjectMethodTable`, que o porte não tem): é o gancho
//!   `VM::set_uncaught_exception_reporter` (o do JSC não faz nada, como o gancho ausente); a exceção de
//!   terminação (`clearExceptionExceptTermination` falha) interrompe o checkpoint e esvazia a fila,
//!   como o C++.
//! - `VM::didExhaustMicrotaskQueue` (rejeições não tratadas, `m_aboutToBeNotifiedRejectedPromises`) e
//!   `callOnEachMicrotaskTick` estão aqui, alimentados pelo `promiseRejectionTracker` do global
//!   (`promise_constructor.rs`); o `unhandledRejectionCallback` é o campo de `PromiseGlobalData`.
//! - `m_drainMicrotaskDelayScopeCount` e `executionForbidden()` não existem (nunca contam).

use std::cell::{Cell, RefCell};
use std::collections::VecDeque;
use std::rc::Rc;

use crate::llint::LLIntFailure;
use crate::runtime::call_data::call_with_error_message;
use crate::runtime::exception::Exception;
use crate::runtime::js_global_object::JSGlobalObject;
use crate::runtime::js_promise::JSPromiseRef;
use crate::runtime::js_scope::{JSScope, JSScopeRef};
use crate::runtime::js_promise_host::PromiseHost;
use crate::runtime::js_value::JSValue;
use crate::runtime::microtask::{InternalMicrotask, MAX_MICROTASK_ARGUMENTS};
use crate::runtime::vm::{TopExceptionScope, VM};

/// `class QueuedTask`.
#[derive(Clone, Copy, Debug)]
pub struct QueuedTask {
    /// `globalObject()`: o `cell_id` do `JSGlobalObject`.
    pub global_object: usize,
    /// `job()`.
    pub job: InternalMicrotask,
    /// `payload()`.
    pub payload: u8,
    /// `arguments()`: os que a tarefa não passa ficam vazios (`JSValue()`).
    pub arguments: [JSValue; MAX_MICROTASK_ARGUMENTS as usize],
}

impl QueuedTask {
    /// `QueuedTask(nullptr, job, payload, globalObject, args...)`.
    pub fn new(global_object: usize, job: InternalMicrotask, payload: u8, arguments: &[JSValue]) -> QueuedTask {
        assert!(arguments.len() <= MAX_MICROTASK_ARGUMENTS as usize);
        let mut padded = [JSValue::empty(); MAX_MICROTASK_ARGUMENTS as usize];
        padded[..arguments.len()].copy_from_slice(arguments);
        QueuedTask { global_object, job, payload, arguments: padded }
    }
}

/// `class MicrotaskQueue` (com `m_queue`; ver as DIVERGÊNCIAS do cabeçalho para `m_toKeep`).
#[derive(Debug, Default)]
pub struct MicrotaskQueue {
    queue: RefCell<VecDeque<QueuedTask>>,
    /// `m_isPerformingMicrotaskCheckpoint`.
    is_performing_microtask_checkpoint: Cell<bool>,
}

impl MicrotaskQueue {
    /// `MicrotaskQueue::visitAggregate`: o global e os argumentos de cada tarefa enfileirada.
    pub(crate) fn visit_children(&self, marker: &mut crate::runtime::gc::Marker) {
        let Ok(queue) = self.queue.try_borrow() else {
            marker.abort();
            return;
        };
        for task in queue.iter() {
            marker.cell(task.global_object);
            marker.values(&task.arguments);
        }
    }

    /// `enqueue(QueuedTask&&)`.
    pub fn enqueue(&self, task: QueuedTask) {
        self.queue.borrow_mut().push_back(task);
    }

    /// `isEmpty()`.
    pub fn is_empty(&self) -> bool {
        self.queue.borrow().is_empty()
    }

    /// `size()`.
    pub fn size(&self) -> usize {
        self.queue.borrow().len()
    }

    /// `clear()`.
    pub fn clear(&self) {
        self.queue.borrow_mut().clear();
    }

    /// `m_queue.dequeue()`: `None` com a fila vazia.
    fn dequeue(&self) -> Option<QueuedTask> {
        self.queue.borrow_mut().pop_front()
    }

    /// `isPerformingMicrotaskCheckpoint()`.
    pub fn is_performing_microtask_checkpoint(&self) -> bool {
        self.is_performing_microtask_checkpoint.get()
    }
}

impl VM {
    /// `drainMicrotasks()` (`VM.cpp`): o checkpoint, o `didExhaustMicrotaskQueue` (rejeições não
    /// tratadas) e, se este enfileirou mais tarefas, outra volta. Como no C++, não há trava de
    /// reentrada: uma chamada de dentro de uma tarefa roda um checkpoint aninhado (o
    /// `m_isPerformingMicrotaskCheckpoint` é restaurado, `SetForScope`). A terminação pendente
    /// interrompe tudo.
    ///
    /// DIVERGÊNCIA (bun): os `process.nextTick` pendentes rodam ANTES de qualquer microtarefa e a drenagem fica no
    /// modo "ticks" até as duas filas esvaziarem (`processTicksAndRejections`: um tick agendado por microtarefa roda
    /// depois das microtarefas irmãs). Com a fila de ticks vazia na entrada, a drenagem é a comum e um tick agendado
    /// por uma microtarefa roda logo depois DELA (`perform_microtask_checkpoint`). Uma drenagem iniciada de dentro de
    /// um tick (`is_processing_ticks`) é só de microtarefas.
    pub fn drain_microtasks(&self) {
        self.drain_microtasks_and_ticks();
        self.finalize_synchronous_js_execution();
    }

    /// O laço de `drainMicrotasks`: os ticks do `process.nextTick` e as microtarefas até as duas filas esvaziarem.
    fn drain_microtasks_and_ticks(&self) {
        use crate::runtime::process_object as ticks;
        if ticks::is_processing_ticks() {
            self.drain_microtasks_only();
            return;
        }
        loop {
            if ticks::has_pending_ticks() {
                let was_processing = ticks::set_processing_ticks(true);
                loop {
                    ticks::run_pending_ticks(self);
                    // `process.exit()` dentro de um tick deixa a terminação pendente: nada mais roda (chamar JS agora
                    // com exceção pendente quebra o contrato do interpretador).
                    if self.has_pending_termination_exception() {
                        break;
                    }
                    self.drain_microtasks_only();
                    if !ticks::has_pending_ticks() || self.has_pending_termination_exception() {
                        break;
                    }
                }
                ticks::set_processing_ticks(was_processing);
            } else {
                self.drain_microtasks_only();
            }
            if !ticks::has_pending_ticks() || self.has_pending_termination_exception() {
                return;
            }
        }
    }

    /// O laço de `drainMicrotasks` do C++, sem os ticks de `process.nextTick`.
    fn drain_microtasks_only(&self) {
        let queue = &self.default_microtask_queue;
        loop {
            self.perform_microtask_checkpoint();
            if self.has_pending_termination_exception() {
                return;
            }
            self.did_exhaust_microtask_queue();
            if self.has_pending_termination_exception() {
                return;
            }
            if queue.is_empty() {
                break;
            }
        }
    }

    /// `performMicrotaskCheckpoint` com `drainImpl` e `runMicrotask`: roda as tarefas até a fila esvaziar
    /// (as que as tarefas enfileiram entram no mesmo laço). A exceção comum que uma tarefa deixa é
    /// limpa e reportada por `reportUncaughtExceptionAtEventLoop`; a de terminação (que
    /// `clearExceptionExceptTermination` não limpa) esvazia a fila e fica pendente.
    fn perform_microtask_checkpoint(&self) {
        let queue = &self.default_microtask_queue;
        // `SetForScope inCheckpoint(m_isPerformingMicrotaskCheckpoint, true)`: restaura o valor anterior.
        let was_performing = queue.is_performing_microtask_checkpoint.replace(true);
        let scope = TopExceptionScope::new(self);
        while let Some(task) = queue.dequeue() {
            let Some(JSScopeRef::GlobalObject(global_object)) = JSScope::from_cell_id(task.global_object) else {
                panic!("QueuedTask de um global object que não está no registro de células");
            };
            let _roots = crate::runtime::gc::RootScope::new(&task.arguments);
            global_object.run_internal_microtask(task.job, task.payload, task.arguments);

            if let Some(exception) = scope.exception() {
                if !scope.clear_exception_except_termination() {
                    queue.clear();
                    break;
                }
                self.report_uncaught_exception_at_event_loop(&global_object, &exception);
                if !scope.clear_exception_except_termination() {
                    queue.clear();
                    break;
                }
            }

            // Tick agendado por esta microtarefa (fora do modo ticks): roda logo depois dela, antes das irmãs.
            if crate::runtime::process_object::has_pending_ticks() && !crate::runtime::process_object::is_processing_ticks() {
                let was_processing = crate::runtime::process_object::set_processing_ticks(true);
                crate::runtime::process_object::run_pending_ticks(self);
                crate::runtime::process_object::set_processing_ticks(was_processing);
            }

            self.call_on_each_microtask_tick();
            if !scope.clear_exception_except_termination() {
                queue.clear();
                break;
            }
        }
        queue.is_performing_microtask_checkpoint.set(was_performing);
    }

    /// `promiseRejected(promise)`: a promessa rejeitada sem tratador espera o fim da drenagem.
    pub fn promise_rejected(&self, promise: JSPromiseRef) {
        self.promise_rejection_state.about_to_be_notified.borrow_mut().push(promise);
    }

    /// `didExhaustMicrotaskQueue()`: chama o `unhandledRejectionCallback` de cada promessa que ainda não
    /// foi tratada, até a fila de notificações esvaziar (o callback pode rejeitar outras).
    fn did_exhaust_microtask_queue(&self) {
        loop {
            let unhandled_rejections = std::mem::take(&mut *self.promise_rejection_state.about_to_be_notified.borrow_mut());
            if unhandled_rejections.is_empty() {
                return;
            }
            for promise in unhandled_rejections {
                if promise.is_handled() {
                    continue;
                }
                self.call_promise_rejection_callback(&promise);
                if self.has_pending_termination_exception() {
                    return;
                }
            }
        }
    }

    /// `callPromiseRejectionCallback(promise)`: `callback(promise, reason)` com `this` `null`; qualquer
    /// exceção do callback é descartada.
    fn call_promise_rejection_callback(&self, promise: &JSPromiseRef) {
        let Some(realm) = promise.realm() else { return };
        let reporter = self.promise_rejection_state.unhandled_rejection_reporter.borrow().clone();
        if let Some(reporter) = reporter {
            reporter(&realm, promise.as_value(), promise.result());
            return;
        }
        let Some(callback) = realm.unhandled_rejection_callback() else { return };
        // O resultado (e a exceção) são descartados: `scope.clearException()`.
        // Só `Thrown` é descartado (`scope.clearException()`); lacuna do interpretador não some em silêncio.
        if let Err(failure @ (LLIntFailure::Unported(_) | LLIntFailure::UnportedOpcode(_))) = call_with_error_message(
            &realm,
            callback,
            JSValue::null(),
            &[promise.as_value(), promise.result()],
            "unhandledRejectionCallback is not a function",
        ) {
            panic!("unhandledRejectionCallback falhou por lacuna do interpretador: {failure:?}");
        }
        self.clear_exception();
    }

    /// `setOnEachMicrotaskTick(func)`.
    pub fn set_on_each_microtask_tick(&self, function: Option<Rc<dyn Fn(&VM)>>) {
        *self.promise_rejection_state.on_each_microtask_tick.borrow_mut() = function;
    }

    /// `callOnEachMicrotaskTick()`.
    fn call_on_each_microtask_tick(&self) {
        let function = self.promise_rejection_state.on_each_microtask_tick.borrow().clone();
        if let Some(function) = function {
            function(self);
        }
    }

    /// Instala o `reportUncaughtExceptionAtEventLoop` do `GlobalObjectMethodTable` (o do JSC não faz
    /// nada; o embedder o substitui).
    pub fn set_uncaught_exception_reporter(&self, reporter: Option<Rc<dyn Fn(&JSGlobalObject, &Rc<Exception>)>>) {
        *self.promise_rejection_state.uncaught_exception_reporter.borrow_mut() = reporter;
    }

    /// `globalObject->globalObjectMethodTable()->reportUncaughtExceptionAtEventLoop(globalObject, exception)`.
    pub fn report_uncaught_exception_at_event_loop(&self, global_object: &JSGlobalObject, exception: &Rc<Exception>) {
        let reporter = self.promise_rejection_state.uncaught_exception_reporter.borrow().clone();
        if let Some(reporter) = reporter {
            reporter(global_object, exception);
        }
    }

    /// Instala o relato nativo de rejeição sem tratador: recebe o motivo da promessa, ao fim da drenagem.
    pub fn set_unhandled_rejection_reporter(&self, reporter: Option<Rc<dyn Fn(&JSGlobalObject, JSValue, JSValue)>>) {
        *self.promise_rejection_state.unhandled_rejection_reporter.borrow_mut() = reporter;
    }

    /// Instala o relato do tratador que chega depois da rejeição: recebe a promessa.
    pub fn set_rejection_handled_reporter(&self, reporter: Option<Rc<dyn Fn(&JSGlobalObject, JSValue)>>) {
        *self.promise_rejection_state.rejection_handled_reporter.borrow_mut() = reporter;
    }

    /// O relato do tratador novo, quando o embedder instalou um.
    pub(crate) fn rejection_handled_reporter(&self) -> Option<Rc<dyn Fn(&JSGlobalObject, JSValue)>> {
        self.promise_rejection_state.rejection_handled_reporter.borrow().clone()
    }

    /// Instala o `Bun__reportUnhandledError` (símbolo fraco do embedder).
    pub fn set_unhandled_error_reporter(&self, reporter: Option<Rc<dyn Fn(&JSGlobalObject, JSValue)>>) {
        *self.promise_rejection_state.unhandled_error_reporter.borrow_mut() = reporter;
    }

    /// `if (Bun__reportUnhandledError) Bun__reportUnhandledError(globalObject, JSValue::encode(exception))`.
    pub fn report_unhandled_error(&self, global_object: &JSGlobalObject, error: JSValue) {
        let reporter = self.promise_rejection_state.unhandled_error_reporter.borrow().clone();
        if let Some(reporter) = reporter {
            reporter(global_object, error);
        }
    }
}

/// O estado de `VM` que o `promiseRejectionTracker` e a drenagem usam: `m_aboutToBeNotifiedRejectedPromises`,
/// `m_onEachMicrotaskTick` e os ganchos do embedder.
#[derive(Default)]
pub struct PromiseRejectionState {
    about_to_be_notified: RefCell<Vec<JSPromiseRef>>,
    on_each_microtask_tick: RefCell<Option<Rc<dyn Fn(&VM)>>>,
    uncaught_exception_reporter: RefCell<Option<Rc<dyn Fn(&JSGlobalObject, &Rc<Exception>)>>>,
    unhandled_error_reporter: RefCell<Option<Rc<dyn Fn(&JSGlobalObject, JSValue)>>>,
    /// O embedder que relata a rejeição sem tratador (o `reason`) no lugar do `unhandledRejectionCallback` de JS.
    unhandled_rejection_reporter: RefCell<Option<Rc<dyn Fn(&JSGlobalObject, JSValue, JSValue)>>>,
    /// O embedder que ouve o tratador novo numa promessa rejeitada (o `rejectionhandled` do navegador) no lugar do
    /// `rejectionHandled` do `process`.
    rejection_handled_reporter: RefCell<Option<Rc<dyn Fn(&JSGlobalObject, JSValue)>>>,
}

impl std::fmt::Debug for PromiseRejectionState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PromiseRejectionState")
            .field("about_to_be_notified", &self.about_to_be_notified.borrow().len())
            .finish_non_exhaustive()
    }
}

#[cfg(test)]
mod tests {
    use crate::api::eval::{evaluate, new_global_object};
    use crate::parser::source_code::make_source;
    use crate::parser::source_provider::SourceProviderSourceType;
    use crate::parser::source_tainted_origin::SourceTaintedOrigin;
    use crate::runtime::js_global_object::JSGlobalObjectRef;
    use crate::runtime::js_value::JSValue;
    use crate::runtime::source_origin::SourceOrigin;
    use crate::wtf::text::text_position::TextPosition;
    use crate::wtf::text::wtf_string::String as WtfString;

    /// Roda `text` como `Program` no `global_object` (sem esvaziar as microtasks).
    fn run(global_object: &JSGlobalObjectRef, text: &str) -> JSValue {
        let source = make_source(
            &WtfString::from_latin1(text.as_bytes()),
            &SourceOrigin::default(),
            SourceTaintedOrigin::Untainted,
            WtfString::default(),
            TextPosition::default(),
            SourceProviderSourceType::Program,
        );
        evaluate(global_object, &source).unwrap_or_else(|_| panic!("lançou exceção: {text}"))
    }

    #[test]
    fn unhandled_rejection_callback_sees_only_rejections_still_unhandled_after_the_drain() {
        let (vm, global_object) = new_global_object();
        run(
            &global_object,
            "var seen = []; var cb = function (promise, reason) { seen.push(reason); }; \
             var a = Promise.reject(1); \
             var b = Promise.reject(2); b.catch(function () {}); \
             var c = Promise.reject(3); Promise.resolve().then(function () { c.catch(function () {}); });",
        );
        let callback = run(&global_object, "cb");
        global_object.set_unhandled_rejection_callback(Some(callback));
        vm.drain_microtasks();
        // `a` ficou sem tratador; `b` foi tratada antes e `c` durante a drenagem, antes do
        // `didExhaustMicrotaskQueue`.
        assert!(run(&global_object, "seen.length === 1 && seen[0] === 1").is_true());
    }

    #[test]
    fn rejection_raised_by_the_callback_phase_is_notified_in_the_same_drain() {
        let (vm, global_object) = new_global_object();
        run(
            &global_object,
            "var seen = []; var cb = function (promise, reason) { seen.push(reason); if (reason === 1) Promise.reject(2); }; \
             Promise.reject(1);",
        );
        let callback = run(&global_object, "cb");
        global_object.set_unhandled_rejection_callback(Some(callback));
        vm.drain_microtasks();
        // `didExhaustMicrotaskQueue` repete enquanto a fila de notificações não esvazia.
        assert!(run(&global_object, "seen.length === 2 && seen[0] === 1 && seen[1] === 2").is_true());
    }

    #[test]
    fn microtasks_run_in_fifo_order_and_thenable_jobs_take_an_extra_turn() {
        let (vm, global_object) = new_global_object();
        run(
            &global_object,
            "var log = []; \
             var thenable = { then: function (resolve) { log.push('then'); resolve(1); } }; \
             Promise.resolve(thenable).then(function () { log.push('resolved'); }); \
             Promise.resolve().then(function () { log.push('a'); }).then(function () { log.push('b'); }); \
             log.push('sync');",
        );
        vm.drain_microtasks();
        assert!(run(&global_object, "log.join() === 'sync,then,a,resolved,b'").is_true());
    }

    #[test]
    fn nested_checkpoint_restores_the_in_progress_flag() {
        let (vm, global_object) = new_global_object();
        run(&global_object, "var n = 0; Promise.resolve().then(function () { n++; });");
        assert!(!vm.default_microtask_queue.is_performing_microtask_checkpoint());
        vm.drain_microtasks();
        assert!(!vm.default_microtask_queue.is_performing_microtask_checkpoint());
        assert!(vm.default_microtask_queue.is_empty());
        assert!(run(&global_object, "n === 1").is_true());
    }

    #[test]
    fn embedder_hears_unhandled_rejections_and_late_handlers() {
        use std::cell::RefCell;
        use std::rc::Rc;
        let (vm, global_object) = new_global_object();
        let events = Rc::new(RefCell::new(Vec::<String>::new()));
        let (unhandled, handled) = (events.clone(), events.clone());
        vm.set_unhandled_rejection_reporter(Some(Rc::new(move |_: &crate::runtime::js_global_object::JSGlobalObject, _: JSValue, reason: JSValue| {
            unhandled.borrow_mut().push(format!("unhandled {}", reason.as_number()));
        })));
        let seen_promise = Rc::new(RefCell::new(JSValue::undefined()));
        let handled_promise = seen_promise.clone();
        vm.set_rejection_handled_reporter(Some(Rc::new(move |_: &crate::runtime::js_global_object::JSGlobalObject, promise: JSValue| {
            handled.borrow_mut().push("handled".to_owned());
            *handled_promise.borrow_mut() = promise;
        })));
        run(&global_object, "var late = Promise.reject(7);");
        vm.drain_microtasks();
        run(&global_object, "late.catch(function () {});");
        vm.drain_microtasks();
        assert_eq!(*events.borrow(), vec!["unhandled 7".to_owned(), "handled".to_owned()]);
        assert!(*seen_promise.borrow() == run(&global_object, "late"));
    }
}
