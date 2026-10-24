//! Porte de `src/vs/base/test/common/virtualScheduling/virtualClock.ts`: um relógio virtual mais uma fila
//! de prioridade de eventos.
//!
//! Estrutura de dados pura: o relógio não conhece "tempo real". Ele só avança por [`VirtualClock::run_next`],
//! que põe `now` no `time` do próximo evento antes de rodá-lo. O `VirtualTimeProcessor` é o único
//! condutor previsto, mas o relógio serve sozinho (testar um agendador, ou avançar um cenário à mão).
//!
//! Notas de tradução:
//! - `VirtualTime` (`number`) é `u64` em milissegundos, como o relógio do loop.
//! - `EventSource` (`{ toString(): string; stackTrace?: string }`) é uma struct com o texto do `toString`
//!   já calculado (a descrição do evento é fixa desde o agendamento) e a pilha capturada, formatada só
//!   quando [`EventSource::stack_trace`] é chamada (o getter `stackTrace` do original).
//! - `VirtualEvent.run()` é o campo `run` (`Rc<dyn Fn()>`, porque o evento é copiável e o relógio guarda
//!   uma cópia na fila, como o `{ ...event, id }` do original).
//! - A `SimplePriorityQueue` (vetor reordenado por `(time, id)` na leitura) vira um `BTreeMap` com chave
//!   `(time, id)`: a mesma ordem total (tempo, depois ordem de agendamento), sem reordenar.
//! - `new VirtualClock()` é `VirtualClock::default()`; o relógio é compartilhado por `Rc`.

use std::cell::{Cell, RefCell};
use std::collections::BTreeMap;
use std::fmt;
use std::rc::Rc;

use super::trace::Trace;
use crate::errors::{self, Error};
use crate::event::{Emitter, Event};
use crate::lifecycle::{DisposableRef, StackSource, plain_disposable};

pub type VirtualTime = u64;

/// Descrição de depuração de um evento (`EventSource`).
#[derive(Clone)]
pub struct EventSource {
    text: String,
    stack: Option<StackSource>,
}

impl EventSource {
    /// Uma fonte com o resultado do `toString()`.
    pub fn new(text: impl Into<String>) -> Self {
        Self { text: text.into(), stack: None }
    }

    /// A fonte com a pilha de quem agendou.
    #[must_use]
    pub fn with_stack(mut self, stack: StackSource) -> Self {
        self.stack = Some(stack);
        self
    }

    /// `stackTrace`: formatado agora.
    pub fn stack_trace(&self) -> Option<String> {
        self.stack.as_ref().map(StackSource::stack)
    }
}

impl fmt::Display for EventSource {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.text)
    }
}

/// Uma unidade de trabalho agendada num ponto do tempo virtual.
///
/// Callbacks de timer são eventos. Conclusões externas (uma leitura de arquivo falsa, por exemplo)
/// também podem ser modeladas como eventos cujo tempo de conclusão virtual é escolhido por uma política
/// de agendamento: para o [`VirtualClock`] elas são indistinguíveis.
#[derive(Clone)]
pub struct VirtualEvent {
    pub time: VirtualTime,
    pub source: EventSource,
    pub trace: Option<Rc<Trace>>,
    /// Dica para o `Embedding`: este evento prefere rodar num `requestAnimationFrame` real (para que
    /// medições de DOM depois dele vejam um reflow real). Testes de tempo puro podem ignorar.
    pub prefer_real_animation_frame: bool,
    pub run: Rc<dyn Fn()>,
}

impl VirtualEvent {
    pub fn new(time: VirtualTime, source: EventSource, run: impl Fn() + 'static) -> Self {
        Self { time, source, trace: None, prefer_real_animation_frame: false, run: Rc::new(run) }
    }

    #[must_use]
    pub fn with_trace(mut self, trace: Rc<Trace>) -> Self {
        self.trace = Some(trace);
        self
    }

    #[must_use]
    pub fn preferring_real_animation_frame(mut self) -> Self {
        self.prefer_real_animation_frame = true;
        self
    }
}

type Queue = BTreeMap<(VirtualTime, u64), VirtualEvent>;

/// `VirtualClock`.
pub struct VirtualClock {
    now: Cell<VirtualTime>,
    id_counter: Cell<u64>,
    queue: Rc<RefCell<Queue>>,
    on_event_scheduled: Rc<Emitter<VirtualEvent>>,
}

impl VirtualClock {
    pub fn new(start_time: VirtualTime) -> Self {
        Self { now: Cell::new(start_time), id_counter: Cell::new(0), queue: Rc::default(), on_event_scheduled: Emitter::new() }
    }

    pub fn now(&self) -> VirtualTime {
        self.now.get()
    }

    pub fn has_events(&self) -> bool {
        !self.queue.borrow().is_empty()
    }

    /// `onEventScheduled`.
    pub fn on_event_scheduled(&self) -> Event<VirtualEvent> {
        self.on_event_scheduled.event()
    }

    /// Agenda o evento. Lança se `event.time < now`. Descartar o handle devolvido tira o evento da fila.
    pub fn schedule(&self, event: VirtualEvent) -> DisposableRef {
        if event.time < self.now.get() {
            errors::throw(Error::new(format!("Scheduled time ({}) must be >= now ({}).", event.time, self.now.get())));
        }
        let key = (event.time, self.id_counter.get());
        self.id_counter.set(key.1 + 1);
        self.queue.borrow_mut().insert(key, event.clone());
        self.on_event_scheduled.fire(event);
        let queue = Rc::downgrade(&self.queue);
        plain_disposable(move || {
            // O handle não mantém a fila viva: quem guarda handles de eventos que guardam a própria fila
            // (a tabela de timers da API virtual) não forma ciclo.
            if let Some(queue) = queue.upgrade() {
                queue.borrow_mut().remove(&key);
            }
        })
    }

    pub fn peek_next(&self) -> Option<VirtualEvent> {
        self.queue.borrow().values().next().cloned()
    }

    /// Tira o próximo evento, põe `now` no tempo dele e o roda. Devolve o evento rodado.
    pub fn run_next(&self) -> Option<VirtualEvent> {
        let next = self.queue.borrow_mut().pop_first();
        let (_, event) = next?;
        self.now.set(event.time);
        (event.run)();
        Some(event)
    }

    /// Os eventos pendentes, em ordem.
    pub fn get_events(&self) -> Vec<VirtualEvent> {
        self.queue.borrow().values().cloned().collect()
    }
}

impl Default for VirtualClock {
    fn default() -> Self {
        Self::new(0)
    }
}
