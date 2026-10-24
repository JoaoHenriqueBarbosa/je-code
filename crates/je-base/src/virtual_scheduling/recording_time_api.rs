//! Porte de `src/vs/base/test/common/virtualScheduling/recordingTimeApi.ts`: embrulha a API de tempo real
//! para registrar cada timer que dispara.
//!
//! Notas de tradução:
//! - `RecordedTimerEvent` (estruturalmente compatível com `VirtualEvent` e com `ScheduledTaskLike`, que
//!   o `buildHistoryFromTasks` consome) é uma struct própria com `From<&VirtualEvent>` e
//!   [`RecordedTimerEvent::to_scheduled_task_like`] para o renderizador de raias do `execution_graph`.
//! - O `history` é compartilhado (`Rc<RefCell<Vec<..>>>`), porque quem chama o lê depois, como o array do
//!   original.
//! - `realTimeApi.Date.now()` é [`TimeApi::date_now`] da API real, e o tempo registrado é esse
//!   (`u64`, o relógio do loop).
//! - Os handlers de `setImmediate`/`requestAnimationFrame` são embrulhados antes de ir para a API real;
//!   se ela não os tem, o handler original volta intacto em `Err` (guardado numa célula compartilhada).
//! - `originalFn` não é portado (ver `global_time_api`).

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use super::time_api::{
    AnimationFrameId, FrameCallback, Handler, ImmediateId, IntervalId, TimeApi, TimeoutId, delegate_time_api_passthrough,
    real_time_api,
};
use super::trace::{RunAsHandlerOptions, Trace, TraceContext};
use super::virtual_clock::{EventSource, VirtualEvent};
use crate::execution_graph::{ScheduledTaskLike, TaskSourceLike};
use crate::lifecycle::StackSource;

/// Uma entrada de uma gravação de tempo real.
#[derive(Clone)]
pub struct RecordedTimerEvent {
    pub time: u64,
    pub source: EventSource,
    pub trace: Option<Rc<Trace>>,
}

impl RecordedTimerEvent {
    /// O mesmo evento no formato que o `build_history_from_tasks` consome.
    #[allow(clippy::cast_precision_loss)]
    pub fn to_scheduled_task_like(&self) -> ScheduledTaskLike {
        ScheduledTaskLike {
            time: self.time as f64,
            source: TaskSourceLike { text: self.source.to_string(), stack_trace: self.source.stack_trace() },
            trace: self.trace.as_ref().map(|trace| trace.as_trace_like()),
        }
    }
}

impl From<&VirtualEvent> for RecordedTimerEvent {
    fn from(event: &VirtualEvent) -> Self {
        Self { time: event.time, source: event.source.clone(), trace: event.trace.clone() }
    }
}

/// O histórico de uma gravação.
pub type RecordedHistory = Rc<RefCell<Vec<RecordedTimerEvent>>>;

/// O que os handlers embrulhados levam consigo: onde gravar e qual é a API real.
#[derive(Clone)]
struct Recorder {
    history: RecordedHistory,
    real: Rc<dyn TimeApi>,
}

impl Recorder {
    /// `record(label, stack, trace)` do original.
    fn record(&self, label: impl Into<String>, stack: &StackSource, trace: &Rc<Trace>) {
        self.history.borrow_mut().push(RecordedTimerEvent {
            time: self.real.date_now(),
            source: EventSource::new(label).with_stack(stack.clone()),
            trace: Some(trace.clone()),
        });
    }

    /// `runAsHandlerReal(trace, handler)`: o reset do trace vai para um `setTimeout(cb, 0)` real.
    fn run_as_handler(&self, trace: &Rc<Trace>, handler: impl FnOnce()) {
        let real = self.real.clone();
        TraceContext::instance().run_as_handler(
            trace.clone(),
            handler,
            &RunAsHandlerOptions::new(move |cb| {
                real.set_timeout(cb, 0);
            }),
        );
    }

    /// Embrulha `handler` para gravar e rodar sob `trace` quando a API real o disparar.
    fn wrap(&self, label: &'static str, stack: StackSource, trace: Rc<Trace>, handler: Handler) -> Handler {
        let recorder = self.clone();
        Box::new(move || {
            recorder.record(label, &stack, &trace);
            recorder.run_as_handler(&trace, handler);
        })
    }
}

struct RecordingRealTimeApi {
    recorder: Recorder,
}

impl TimeApi for RecordingRealTimeApi {
    fn set_timeout(&self, handler: Handler, timeout: u64) -> TimeoutId {
        let stack = StackSource::capture();
        let trace = TraceContext::instance().current_trace().child(format!("setTimeout({timeout}ms)"), Some(stack.clone()));
        self.recorder.real.set_timeout(self.recorder.wrap("setTimeout", stack, trace, handler), timeout)
    }

    fn set_interval(&self, handler: Rc<dyn Fn()>, interval: u64) -> IntervalId {
        let stack = StackSource::capture();
        let base_trace = TraceContext::instance().current_trace().child(format!("setInterval({interval}ms)"), Some(stack.clone()));
        let iter = Cell::new(0_u64);
        let recorder = self.recorder.clone();
        self.recorder.real.set_interval(
            Rc::new(move || {
                iter.set(iter.get() + 1);
                let tick_trace = base_trace.child(format!("tick #{}", iter.get()), None);
                recorder.record(format!("setInterval (iteration {})", iter.get()), &stack, &tick_trace);
                recorder.run_as_handler(&tick_trace, || handler());
            }),
            interval,
        )
    }

    fn set_immediate(&self, handler: Handler) -> Result<ImmediateId, Handler> {
        let stack = StackSource::capture();
        let trace = TraceContext::instance().current_trace().child("setImmediate", Some(stack.clone()));
        let original = Rc::new(Cell::new(Some(handler)));
        let slot = original.clone();
        let inner: Handler = Box::new(move || {
            if let Some(handler) = slot.take() {
                handler();
            }
        });
        self.recorder
            .real
            .set_immediate(self.recorder.wrap("setImmediate", stack, trace, inner))
            .map_err(|_| original.take().expect("a API real devolveu o handler sem o ter rodado"))
    }

    fn request_animation_frame(&self, callback: FrameCallback) -> Result<AnimationFrameId, FrameCallback> {
        let stack = StackSource::capture();
        let trace = TraceContext::instance().current_trace().child("requestAnimationFrame", Some(stack.clone()));
        let original = Rc::new(Cell::new(Some(callback)));
        let slot = original.clone();
        let recorder = self.recorder.clone();
        self.recorder
            .real
            .request_animation_frame(Box::new(move |time| {
                recorder.record("requestAnimationFrame", &stack, &trace);
                recorder.run_as_handler(&trace, move || {
                    if let Some(callback) = slot.take() {
                        callback(time);
                    }
                });
            }))
            .map_err(|_| original.take().expect("a API real devolveu o callback sem o ter rodado"))
    }

    delegate_time_api_passthrough!(recorder.real);
}

/// Embrulha a API de tempo real para que cada `setTimeout`/`setInterval`/`requestAnimationFrame` leve um
/// [`Trace`] filho e empurre um [`RecordedTimerEvent`] em `history` quando o handler de fato roda.
///
/// Os handlers rodam por [`TraceContext::run_as_handler`], então as cadeias causais atravessam os
/// `await` dentro de um handler. Atenção: como o reset adiado do trace de cada handler dispara como a
/// sua própria macrotarefa real, a atribuição pode derivar um pouco quando muitos handlers disparam em
/// sequência rápida: serve para diagnóstico, não para asserção.
pub fn create_recording_real_time_api(history: RecordedHistory) -> Rc<dyn TimeApi> {
    Rc::new(RecordingRealTimeApi { recorder: Recorder { history, real: real_time_api() } })
}
