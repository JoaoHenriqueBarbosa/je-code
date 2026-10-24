//! Núcleo de `WritableStream` / `WritableStreamDefaultController` / `WritableStreamDefaultWriter`, portado de
//! `WritableStreamOperations.cpp`, `JSWritableStreamDefaultController.cpp` e `JSWritableStreamDefaultWriter.cpp`.
//!
//! Esta fatia é a MÁQUINA DE ESTADOS, sem tocar no motor: as promessas viram ids (`P`) numa arena local e tudo
//! que o C++ faz no mundo JS (resolver/rejeitar promessa, chamar `write`/`close`/`abort` do sink, abortar o
//! `AbortSignal`) sai como `Effect`, na mesma ordem em que o C++ executa. O integrador cria a promessa JS ao ver
//! `Effect::NewPromise`, e aplica cada efeito em ordem; os callbacks de volta (`on_*`) são chamados dos
//! reações de microtask, como `onWSControllerStartFulfilled` etc. no C++. Assim a ordem de microtasks é a do bun.
//!
//! `V` é o valor JS (erro, chunk, motivo). O tamanho do chunk (`size()` da estratégia) o chamador calcula e passa.

use std::collections::VecDeque;

pub type P = u32;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum State {
    Writable,
    Erroring,
    Errored,
    Closed,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum PState {
    Pending,
    Fulfilled,
    Rejected,
}

/// O que o integrador executa, em ordem.
#[derive(Clone, Debug, PartialEq)]
pub enum Effect<V> {
    /// Cria a promessa JS do id (pendente).
    NewPromise(P),
    /// `resolvePromise(p, undefined)`.
    Resolve(P),
    /// `rejectPromise(p, v)`.
    Reject(P, V),
    /// `markPromiseAsHandled(p)`.
    MarkHandled(P),
    /// `underlyingSink.write(chunk, controller)`; o resultado volta em `on_write_done`.
    SinkWrite(V),
    /// `underlyingSink.close()`; resultado em `on_close_done`.
    SinkClose,
    /// `underlyingSink.abort(reason)`; resultado em `on_abort_steps_done`.
    SinkAbort(V),
    /// `controller.signal`: `abortController.abort(reason)`.
    AbortSignal(V),
    /// `controller[ErrorSteps]`: zera a fila (já feito no núcleo); o integrador solta os algoritmos.
    ErrorSteps,
}

enum Item<V> {
    Chunk(V, f64),
    Close,
}

struct PendingAbort<V> {
    promise: P,
    reason: V,
    was_already_erroring: bool,
}

struct Writer {
    ready: Option<P>,
    closed: P,
}

pub enum WriteOutcome<V> {
    Promise(P),
    Reject(V),
    /// Rejeitar com `TypeError` (`Invalid state: ...`): o integrador monta o erro.
    RejectTypeError(&'static str),
}

pub struct Core<V: Clone> {
    pub state: State,
    pub stored_error: Option<V>,
    pub backpressure: bool,
    pub started: bool,
    pub hwm: f64,
    pub effects: Vec<Effect<V>>,
    promises: Vec<PState>,
    writer: Option<Writer>,
    close_request: Option<P>,
    in_flight_close: Option<P>,
    in_flight_write: Option<P>,
    write_requests: VecDeque<P>,
    pending_abort: Option<PendingAbort<V>>,
    abort_steps_promise: Option<(P, bool)>,
    queue: VecDeque<Item<V>>,
    queue_total: f64,
}

impl<V: Clone> Core<V> {
    /// Cada valor JS que o núcleo segura (o `visitChildren` do controlador): o erro guardado, os efeitos ainda por
    /// aplicar, o motivo do aborto pendente e os chunks da fila.
    pub fn for_each_value(&self, mut visit: impl FnMut(&V)) {
        self.stored_error.iter().for_each(&mut visit);
        for effect in &self.effects {
            match effect {
                Effect::Reject(_, value) | Effect::SinkWrite(value) | Effect::SinkAbort(value) | Effect::AbortSignal(value) => visit(value),
                Effect::NewPromise(_) | Effect::Resolve(_) | Effect::MarkHandled(_) | Effect::SinkClose | Effect::ErrorSteps => {}
            }
        }
        if let Some(pending) = &self.pending_abort {
            visit(&pending.reason);
        }
        for item in &self.queue {
            if let Item::Chunk(chunk, _) = item {
                visit(chunk);
            }
        }
    }

    /// `setUpWritableStreamDefaultControllerBeforeStart`: estado inicial e backpressure.
    pub fn new(hwm: f64) -> Self {
        let mut c = Core {
            state: State::Writable,
            stored_error: None,
            backpressure: false,
            started: false,
            hwm,
            effects: Vec::new(),
            promises: Vec::new(),
            writer: None,
            close_request: None,
            in_flight_close: None,
            in_flight_write: None,
            write_requests: VecDeque::new(),
            pending_abort: None,
            abort_steps_promise: None,
            queue: VecDeque::new(),
            queue_total: 0.0,
        };
        let bp = c.get_backpressure();
        c.update_backpressure(bp);
        c
    }

    fn new_promise(&mut self) -> P {
        let id = self.promises.len() as P;
        self.promises.push(PState::Pending);
        self.effects.push(Effect::NewPromise(id));
        id
    }

    fn fulfilled(&mut self) -> P {
        let p = self.new_promise();
        self.resolve(p);
        p
    }

    fn rejected(&mut self, v: V) -> P {
        let p = self.new_promise();
        self.reject(p, v);
        self.effects.push(Effect::MarkHandled(p));
        p
    }

    fn resolve(&mut self, p: P) {
        if self.promises[p as usize] == PState::Pending {
            self.promises[p as usize] = PState::Fulfilled;
            self.effects.push(Effect::Resolve(p));
        }
    }

    fn reject(&mut self, p: P, v: V) {
        if self.promises[p as usize] == PState::Pending {
            self.promises[p as usize] = PState::Rejected;
            self.effects.push(Effect::Reject(p, v));
        }
    }

    pub fn promise_state(&self, p: P) -> PState {
        self.promises[p as usize]
    }

    pub fn is_locked(&self) -> bool {
        self.writer.is_some()
    }

    pub fn close_queued_or_in_flight(&self) -> bool {
        self.close_request.is_some() || self.in_flight_close.is_some()
    }

    fn has_operation_in_flight(&self) -> bool {
        self.in_flight_write.is_some() || self.in_flight_close.is_some()
    }

    // ---- controller ----

    pub fn desired_size(&self) -> f64 {
        self.hwm - self.queue_total
    }

    fn get_backpressure(&self) -> bool {
        self.desired_size() <= 0.0
    }

    /// Reação a `start()` cumprido (`onWSControllerStartFulfilled`).
    pub fn on_start_fulfilled(&mut self) {
        self.started = true;
        self.advance_queue_if_needed();
    }

    /// Reação a `start()` rejeitado: `stream.[[state]]` Writable ou Erroring.
    pub fn on_start_rejected(&mut self, error: V) {
        self.started = true;
        self.deal_with_rejection(error);
    }

    /// `WritableStreamDefaultControllerClose`: enfileira o marcador de fechamento.
    fn controller_close(&mut self) {
        self.queue.push_back(Item::Close);
        self.advance_queue_if_needed();
    }

    /// `controller.error(e)` (método público): ignora se o stream não está Writable.
    pub fn controller_error(&mut self, error: V) {
        if self.state == State::Writable {
            self.start_erroring(error);
        }
    }

    fn advance_queue_if_needed(&mut self) {
        if !self.started || self.in_flight_write.is_some() {
            return;
        }
        match self.state {
            State::Errored | State::Closed => return,
            State::Erroring => return self.finish_erroring(),
            State::Writable => {}
        }
        match self.queue.front() {
            None => {}
            Some(Item::Close) => {
                self.mark_close_request_in_flight();
                self.queue.pop_front();
                self.queue_total = 0.0;
                self.effects.push(Effect::SinkClose);
            }
            Some(Item::Chunk(v, _)) => {
                let v = v.clone();
                self.mark_first_write_request_in_flight();
                self.effects.push(Effect::SinkWrite(v));
            }
        }
    }

    /// `WritableStreamDefaultControllerWrite` (enfileira chunk com tamanho já calculado).
    /// `size` `Err(e)` (NaN, negativo ou infinito: o RangeError que o chamador monta): `ErrorIfNeeded(e)` e nada entra.
    fn controller_write(&mut self, chunk: V, size: Result<f64, V>) {
        let size = match size {
            Ok(size) => size,
            Err(error) => return self.controller_error(error),
        };
        self.queue.push_back(Item::Chunk(chunk, size));
        self.queue_total += size;
        if !self.close_queued_or_in_flight() && self.state == State::Writable {
            let bp = self.get_backpressure();
            self.update_backpressure(bp);
        }
        self.advance_queue_if_needed();
    }

    /// Resultado de `sink.write` (cumprido ou rejeitado): `Ok` = cumprido, `Err(e)` = rejeitado.
    pub fn on_write_done(&mut self, result: Result<(), V>) {
        match result {
            Ok(()) => {
                self.finish_in_flight_write();
                let state = self.state;
                if let Some(Item::Chunk(_, size)) = self.queue.pop_front() {
                    self.queue_total = (self.queue_total - size).max(0.0);
                }
                if self.queue.is_empty() {
                    self.queue_total = 0.0;
                }
                if !self.close_queued_or_in_flight() && state == State::Writable {
                    let bp = self.get_backpressure();
                    self.update_backpressure(bp);
                }
                self.advance_queue_if_needed();
            }
            Err(e) => self.finish_in_flight_write_with_error(e),
        }
    }

    pub fn on_close_done(&mut self, result: Result<(), V>) {
        match result {
            Ok(()) => self.finish_in_flight_close(),
            Err(e) => self.finish_in_flight_close_with_error(e),
        }
    }

    // ---- operações do stream ----

    /// `writableStreamAbort`. Devolve a promessa.
    pub fn abort(&mut self, reason: V) -> P {
        if matches!(self.state, State::Closed | State::Errored) {
            return self.fulfilled();
        }
        self.effects.push(Effect::AbortSignal(reason.clone()));
        self.abort_after_signal(reason)
    }

    /// `writableStreamAbort` quando o stream já está fechado ou com erro: a promessa cumprida, sem sinalizar.
    pub fn abort_if_settled(&mut self) -> Option<P> {
        matches!(self.state, State::Closed | State::Errored).then(|| self.fulfilled())
    }

    /// O resto do `writableStreamAbort`, depois de o `AbortSignal` ter rodado os ouvintes (que podem ter mudado o
    /// estado, por isso ele é reconferido).
    pub fn abort_after_signal(&mut self, reason: V) -> P {
        if matches!(self.state, State::Closed | State::Errored) {
            return self.fulfilled();
        }
        if let Some(pa) = &self.pending_abort {
            return pa.promise;
        }
        let was = self.state == State::Erroring;
        let promise = self.new_promise();
        // Com `was`, o C++ troca o motivo por undefined; o ramo `was` nunca lê o motivo (rejeita com storedError).
        self.pending_abort = Some(PendingAbort { promise, reason: reason.clone(), was_already_erroring: was });
        if !was {
            self.start_erroring(reason);
        }
        promise
    }

    /// `writableStreamClose`.
    pub fn close(&mut self) -> Result<P, &'static str> {
        if matches!(self.state, State::Closed | State::Errored) {
            return Err("Invalid state: Cannot close a stream that is already closed or errored");
        }
        let promise = self.new_promise();
        self.close_request = Some(promise);
        if let Some(r) = self.writer.as_ref().and_then(|w| w.ready) {
            if self.backpressure && self.state == State::Writable {
                self.resolve(r);
            }
        }
        self.controller_close();
        Ok(promise)
    }

    fn add_write_request(&mut self) -> P {
        let p = self.new_promise();
        self.write_requests.push_back(p);
        p
    }

    fn deal_with_rejection(&mut self, error: V) {
        if self.state == State::Writable {
            self.start_erroring(error);
        } else {
            self.finish_erroring();
        }
    }

    fn start_erroring(&mut self, reason: V) {
        self.state = State::Erroring;
        self.stored_error = Some(reason.clone());
        self.writer_ensure_ready_rejected(reason);
        if !self.has_operation_in_flight() && self.started {
            self.finish_erroring();
        }
    }

    fn finish_erroring(&mut self) {
        self.state = State::Errored;
        // rejectStreamClosedPromise do C++ é o fechamento interno do stream (`m_closedPromise`), sem writer.
        self.effects.push(Effect::ErrorSteps);
        self.queue.clear();
        self.queue_total = 0.0;
        let stored = self.stored_error.clone().expect("storedError");
        while let Some(w) = self.write_requests.pop_front() {
            self.reject(w, stored.clone());
        }
        let Some(pa) = self.pending_abort.take() else {
            return self.reject_close_and_closed_if_needed();
        };
        if pa.was_already_erroring {
            self.reject(pa.promise, stored);
            return self.reject_close_and_closed_if_needed();
        }
        self.abort_steps_promise = Some((pa.promise, true));
        self.effects.push(Effect::SinkAbort(pa.reason));
    }

    /// Resultado de `sink.abort` (`onWSAbortStepsFulfilled/Rejected`).
    pub fn on_abort_steps_done(&mut self, result: Result<(), V>) {
        let (abort_promise, _) = self.abort_steps_promise.take().expect("abort em voo");
        match result {
            Ok(()) => self.resolve(abort_promise),
            Err(e) => self.reject(abort_promise, e),
        }
        self.reject_close_and_closed_if_needed();
    }

    fn finish_in_flight_write(&mut self) {
        let p = self.in_flight_write.take().expect("write em voo");
        self.resolve(p);
    }

    fn finish_in_flight_write_with_error(&mut self, error: V) {
        let p = self.in_flight_write.take().expect("write em voo");
        self.reject(p, error.clone());
        self.deal_with_rejection(error);
    }

    fn finish_in_flight_close(&mut self) {
        let p = self.in_flight_close.take().expect("close em voo");
        self.resolve(p);
        if self.state == State::Erroring {
            self.stored_error = None;
            if let Some(pa) = self.pending_abort.take() {
                self.resolve(pa.promise);
            }
        }
        self.state = State::Closed;
        if let Some(c) = self.writer.as_ref().map(|w| w.closed) {
            self.resolve(c);
        }
    }

    fn finish_in_flight_close_with_error(&mut self, error: V) {
        let p = self.in_flight_close.take().expect("close em voo");
        self.reject(p, error.clone());
        if let Some(pa) = self.pending_abort.take() {
            self.reject(pa.promise, error.clone());
        }
        self.deal_with_rejection(error);
    }

    fn mark_close_request_in_flight(&mut self) {
        self.in_flight_close = self.close_request.take();
    }

    fn mark_first_write_request_in_flight(&mut self) {
        self.in_flight_write = self.write_requests.pop_front();
    }

    fn reject_close_and_closed_if_needed(&mut self) {
        let stored = self.stored_error.clone().expect("storedError");
        if let Some(c) = self.close_request.take() {
            self.reject(c, stored.clone());
        }
        if let Some(c) = self.writer.as_ref().map(|w| w.closed) {
            self.reject(c, stored);
            self.effects.push(Effect::MarkHandled(c));
        }
    }

    fn update_backpressure(&mut self, backpressure: bool) {
        if self.writer.is_some() && backpressure != self.backpressure {
            if backpressure {
                self.writer.as_mut().unwrap().ready = None;
            } else if let Some(r) = self.writer.as_ref().and_then(|w| w.ready) {
                self.resolve(r);
            }
        }
        self.backpressure = backpressure;
    }

    // ---- writer ----

    /// `setUpWritableStreamDefaultWriter`: `Err` = `Invalid state: WritableStream is locked`.
    /// Devolve `(ready, closed)`.
    pub fn acquire_writer(&mut self) -> Result<(P, P), &'static str> {
        if self.is_locked() {
            return Err("Invalid state: WritableStream is locked");
        }
        let (ready, closed) = match self.state {
            State::Writable => {
                let r = if !self.close_queued_or_in_flight() && self.backpressure { self.new_promise() } else { self.fulfilled() };
                (Some(r), self.new_promise())
            }
            State::Erroring => {
                let e = self.stored_error.clone().expect("storedError");
                (Some(self.rejected(e)), self.new_promise())
            }
            State::Closed => {
                let r = self.fulfilled();
                (Some(r), self.fulfilled())
            }
            State::Errored => {
                let e = self.stored_error.clone().expect("storedError");
                let r = self.rejected(e.clone());
                (Some(r), self.rejected(e))
            }
        };
        self.writer = Some(Writer { ready, closed });
        Ok((ready.unwrap(), closed))
    }

    /// Getter `ready` do writer (o C++ recria uma promessa pendente quando `update_backpressure` a zerou).
    pub fn writer_ready(&mut self) -> Option<P> {
        let w = self.writer.as_ref()?;
        if w.ready.is_none() {
            let p = if self.backpressure { self.new_promise() } else { self.fulfilled() };
            self.writer.as_mut().unwrap().ready = Some(p);
        }
        self.writer.as_ref().unwrap().ready
    }

    pub fn writer_closed(&self) -> Option<P> {
        self.writer.as_ref().map(|w| w.closed)
    }

    fn writer_ensure_ready_rejected(&mut self, error: V) {
        let Some(w) = self.writer.as_ref() else { return };
        match w.ready {
            Some(r) if self.promises[r as usize] == PState::Pending => {
                self.reject(r, error);
                self.effects.push(Effect::MarkHandled(r));
            }
            _ => {
                let n = self.rejected(error);
                self.writer.as_mut().unwrap().ready = Some(n);
            }
        }
    }

    fn writer_ensure_closed_rejected(&mut self, error: V) {
        let Some(w) = self.writer.as_ref() else { return };
        let c = w.closed;
        if self.promises[c as usize] == PState::Pending {
            self.reject(c, error);
            self.effects.push(Effect::MarkHandled(c));
        } else {
            let n = self.rejected(error);
            self.writer.as_mut().unwrap().closed = n;
        }
    }

    /// `writer.desiredSize`: `None` = `null`.
    pub fn writer_desired_size(&self) -> Option<f64> {
        match self.state {
            State::Errored | State::Erroring => None,
            State::Closed => Some(0.0),
            _ => Some(self.desired_size()),
        }
    }

    /// `writer.write(chunk)`. `size` já calculado pela estratégia.
    /// A ordem das checagens é a de `writableStreamDefaultWriterWrite`: Errored, fechando ou fechado, Erroring.
    pub fn writer_write(&mut self, chunk: V, size: Result<f64, V>) -> WriteOutcome<V> {
        if self.state == State::Errored {
            return WriteOutcome::Reject(self.stored_error.clone().expect("storedError"));
        }
        if self.close_queued_or_in_flight() || self.state == State::Closed {
            return WriteOutcome::RejectTypeError("Cannot write to a WritableStream that is closing or closed");
        }
        if self.state == State::Erroring {
            return WriteOutcome::Reject(self.stored_error.clone().expect("storedError"));
        }
        let p = self.add_write_request();
        self.controller_write(chunk, size);
        WriteOutcome::Promise(p)
    }

    /// `writer.releaseLock()`: `released_error` é o `TypeError` que o integrador monta. Devolve as promessas
    /// `(ready, closed)` do escritor já rejeitadas, que o escritor solto continua expondo; `None` sem escritor.
    pub fn release_lock(&mut self, released_error: V) -> Option<(P, P)> {
        self.writer.as_ref()?;
        self.writer_ensure_ready_rejected(released_error.clone());
        self.writer_ensure_closed_rejected(released_error);
        let writer = self.writer.take()?;
        Some((writer.ready?, writer.closed))
    }
}
