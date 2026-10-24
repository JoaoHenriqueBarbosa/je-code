//! Porte de `src/vs/base/test/common/virtualScheduling/virtualTimeApi.ts`: uma [`TimeApi`] que agenda
//! toda chamada de timer na fila virtual de um [`VirtualClock`], capturando o trace corrente na hora do
//! agendamento, de modo que as cadeias causais (`setTimeout` -> `setTimeout`, etc.) se preservam.
//!
//! A API devolvida serve para instalar com `push_global_time_api`, o que o `run_with_faked_timers` faz
//! por dentro.
//!
//! Notas de tradução:
//! - Os ids dos timers (no original, os próprios `IDisposable` do relógio) são números de uma faixa
//!   reservada (`1 << 62` em diante), fora da numeração do loop: um id virtual entregue ao
//!   `clear_timeout` de outra API (a real, depois de a virtual ser desinstalada) não cancela nada do
//!   loop. A tabela id -> descartável guarda cada timer até disparar (ou ser cancelado), o que no
//!   original o coletor de lixo faz sozinho.
//! - O `Error.stackTraceLimit = 50` do módulo não existe: a pilha capturada é o `Backtrace` inteiro.
//! - O `Date` falso (`VirtualDate`, que devolve o tempo virtual em `now()` e como argumento padrão do
//!   construtor) é [`TimeApi::date_now`], que devolve `clock.now`. `Date.parse` e `Date.UTC` são puros e
//!   não dependem do relógio.
//! - `originalFn` (o atalho do `setTimeout` virtual para o real, usado pelo polling do component-explorer)
//!   não é portado: a API real do loop está sempre à mão em [`real_time_api`](super::time_api::real_time_api).
//! - `void ROOT_TRACE` (uso só para o import não ser removido) não existe.
//! - Divergência: `requestAnimationFrame` falso e o `setTimeout` compartilham a tabela de timers.

use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::rc::{Rc, Weak};

use super::time_api::{AnimationFrameId, FrameCallback, Handler, IntervalId, TimeApi, TimeoutId};
use super::trace::{Trace, TraceContext};
use super::virtual_clock::{EventSource, VirtualClock, VirtualEvent, VirtualTime};
pub use super::global_time_api::push_global_time_api;
use crate::lifecycle::{DisposableRef, StackSource, plain_disposable};
use crate::runtime::TimerId;

/// O primeiro id virtual: acima de qualquer id que o loop chegue a numerar.
const VIRTUAL_ID_BASE: u64 = 1 << 62;

#[derive(Clone, Copy, Debug, Default)]
pub struct CreateVirtualTimeApiOptions {
    /// Se `true`, `requestAnimationFrame` é falsificado: os callbacks vão para a fila virtual em
    /// `now + 16ms` e o evento sugere ao embedding um `requestAnimationFrame` real, para o host poder
    /// fazer reflow antes de o callback rodar. Útil para fixtures que medem DOM depois de callbacks de
    /// rAF.
    ///
    /// Se `false` (padrão), `requestAnimationFrame` fica com o host (a API não o tem).
    pub fake_request_animation_frame: bool,
}

/// A tabela dos timers pendentes: id -> o descartável do evento (ou do intervalo).
type Timers = Rc<RefCell<HashMap<u64, DisposableRef>>>;

/// Um `setInterval` virtual: rearma o próximo disparo antes de chamar o handler. Guarda o relógio de
/// forma fraca porque o evento pendente na fila guarda o intervalo, e a fila é do relógio.
struct Interval {
    clock: Weak<VirtualClock>,
    handler: Rc<dyn Fn()>,
    interval: u64,
    stack: StackSource,
    base_trace: Rc<Trace>,
    iter: Cell<u64>,
    disposed: Cell<bool>,
    last: RefCell<Option<DisposableRef>>,
}

impl Interval {
    fn arm(self: &Rc<Self>) {
        let Some(clock) = self.clock.upgrade() else { return };
        let iter = self.iter.get() + 1;
        self.iter.set(iter);
        let interval = self.clone();
        let last = clock.schedule(
            VirtualEvent::new(
                clock.now().saturating_add(self.interval),
                EventSource::new(format!("setInterval (iteration {iter})")).with_stack(self.stack.clone()),
                move || {
                    if interval.disposed.get() {
                        return;
                    }
                    interval.arm(); // agenda o próximo tick primeiro, para um handler que lança
                    (interval.handler)(); // não matar o intervalo
                },
            )
            .with_trace(self.base_trace.child(format!("tick #{iter}"), None)),
        );
        *self.last.borrow_mut() = Some(last);
    }

    fn dispose(&self) {
        if self.disposed.replace(true) {
            return;
        }
        let last = self.last.borrow_mut().take();
        if let Some(last) = last {
            last.dispose();
        }
    }
}

struct VirtualTimeApi {
    clock: Rc<VirtualClock>,
    performance_time_origin: VirtualTime,
    fake_request_animation_frame: bool,
    timers: Timers,
    id_counter: Cell<u64>,
}

/// `performance.now()` do relógio: o tempo virtual desde a origem.
#[allow(clippy::cast_precision_loss)]
fn performance_now_of(clock: &VirtualClock, origin: VirtualTime) -> f64 {
    clock.now().saturating_sub(origin) as f64
}

impl VirtualTimeApi {
    fn next_id(&self) -> u64 {
        let id = VIRTUAL_ID_BASE + self.id_counter.get();
        self.id_counter.set(self.id_counter.get() + 1);
        id
    }

    /// Descarta o timer `id`, se ainda pendente.
    fn clear(&self, id: TimerId) {
        let timer = self.timers.borrow_mut().remove(&id.raw());
        if let Some(timer) = timer {
            timer.dispose();
        }
    }
}

impl TimeApi for VirtualTimeApi {
    fn set_timeout(&self, handler: Handler, timeout: u64) -> TimeoutId {
        let stack = StackSource::capture();
        let trace = TraceContext::instance().current_trace().child(format!("setTimeout({timeout}ms)"), Some(stack.clone()));
        let id = self.next_id();
        let timers = self.timers.clone();
        let handler = Cell::new(Some(handler));
        let event = VirtualEvent::new(self.clock.now().saturating_add(timeout), EventSource::new("setTimeout").with_stack(stack), move || {
            timers.borrow_mut().remove(&id);
            if let Some(handler) = handler.take() {
                handler();
            }
        })
        .with_trace(trace);
        let timer = self.clock.schedule(event);
        self.timers.borrow_mut().insert(id, timer);
        TimerId::from_raw(id)
    }

    fn clear_timeout(&self, id: TimeoutId) {
        self.clear(id);
    }

    fn set_interval(&self, handler: Rc<dyn Fn()>, interval: u64) -> IntervalId {
        let stack = StackSource::capture();
        let base_trace = TraceContext::instance().current_trace().child(format!("setInterval({interval}ms)"), Some(stack.clone()));
        let state = Rc::new(Interval {
            clock: Rc::downgrade(&self.clock),
            handler,
            interval,
            stack,
            base_trace,
            iter: Cell::new(0),
            disposed: Cell::new(false),
            last: RefCell::new(None),
        });
        state.arm();
        let id = self.next_id();
        self.timers.borrow_mut().insert(id, plain_disposable(move || state.dispose()));
        TimerId::from_raw(id)
    }

    fn clear_interval(&self, id: IntervalId) {
        self.clear(id);
    }

    fn request_animation_frame(&self, callback: FrameCallback) -> Result<AnimationFrameId, FrameCallback> {
        if !self.fake_request_animation_frame {
            return Err(callback);
        }
        let id = self.next_id();
        let stack = StackSource::capture();
        let trace = TraceContext::instance().current_trace().child("requestAnimationFrame", Some(stack.clone()));
        let timers = self.timers.clone();
        let clock = self.clock.clone();
        let origin = self.performance_time_origin;
        let callback = Cell::new(Some(callback));
        let event = VirtualEvent::new(self.clock.now().saturating_add(16), EventSource::new("requestAnimationFrame").with_stack(stack), move || {
            timers.borrow_mut().remove(&id);
            if let Some(callback) = callback.take() {
                callback(performance_now_of(&clock, origin));
            }
        })
        .with_trace(trace)
        .preferring_real_animation_frame();
        let timer = self.clock.schedule(event);
        self.timers.borrow_mut().insert(id, timer);
        Ok(TimerId::from_raw(id))
    }

    fn cancel_animation_frame(&self, id: AnimationFrameId) {
        self.clear(id);
    }

    fn date_now(&self) -> u64 {
        self.clock.now()
    }

    fn performance_now(&self) -> f64 {
        performance_now_of(&self.clock, self.performance_time_origin)
    }

    #[allow(clippy::cast_precision_loss)]
    fn performance_time_origin(&self) -> f64 {
        self.performance_time_origin as f64
    }
}

/// Monta uma [`TimeApi`] que agenda todo timer na fila virtual de `clock`.
pub fn create_virtual_time_api(clock: Rc<VirtualClock>, options: CreateVirtualTimeApiOptions) -> Rc<dyn TimeApi> {
    Rc::new(VirtualTimeApi {
        performance_time_origin: clock.now(),
        clock,
        fake_request_animation_frame: options.fake_request_animation_frame,
        timers: Timers::default(),
        id_counter: Cell::new(0),
    })
}
