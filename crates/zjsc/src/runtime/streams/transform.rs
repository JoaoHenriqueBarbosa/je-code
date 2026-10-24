//! Núcleo de `TransformStream` / `TransformStreamDefaultController`, portado de `TransformStreamOperations.cpp` e
//! `JSTransformStreamDefaultController.cpp`.
//!
//! Mesmo molde de `writable.rs`: MÁQUINA DE ESTADOS sem tocar no motor. As promessas viram ids (`P`) e o que o
//! C++ faz no mundo JS sai como `Effect`, em ordem. O que o núcleo precisa perguntar ou mandar aos dois lados
//! (o `ReadableStreamDefaultController` e o `WritableStreamDefaultController` do próprio par) passa pelas traits
//! `Readable` e `Writable`, síncronas, que o integrador implementa em cima de `readable.rs` e `writable.rs`.
//!
//! Ordem das reações de microtask (como `runtime->onTS*` no C++): o integrador liga cada `Effect::Call*` ao
//! resultado e chama o `on_*` correspondente quando a promessa do algoritmo se resolve.

use std::collections::HashMap;

pub type P = u32;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum PState {
    Pending,
    Fulfilled,
    Rejected,
}

/// Qual transformer roda (`TransformerKind`). Os nativos (TextEncoder etc.) o integrador resolve por conta própria.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Kind {
    JavaScript,
    Identity,
}

#[derive(Clone, Debug, PartialEq)]
pub enum Effect<V> {
    NewPromise(P),
    Resolve(P),
    Reject(P, V),
    /// `transformer.transform(chunk, controller)`; resultado em `on_transform_done(result_promise, ..)`.
    CallTransform(V, P),
    /// Resolve a primeira promessa com a segunda (como o `resolve(promise, thenable)` do C++: a tarefa do thenable
    /// custa dois tempos, o que o `result` do sink de escrita paga em `onTSSinkWriteBackpressureChangeFulfilled`).
    ResolveWith(P, P),
    /// `transformer.flush(controller)`; resultado em `on_flush_done`.
    CallFlush,
    /// `transformer.cancel(reason)`; resultado em `on_cancel_done` (`from_sink` diz qual dos dois caminhos).
    CallCancel(V, bool),
}

/// Lado de leitura do par (readable.rs). `None`/`false` em `controller_present` = controle já desmontado.
pub trait Readable<V> {
    fn controller_present(&self) -> bool;
    fn can_close_or_enqueue(&self) -> bool;
    /// `readableStreamDefaultControllerEnqueue`; `Err` se o `size()` do usuário lançou.
    fn enqueue(&mut self, chunk: V) -> Result<(), V>;
    fn has_backpressure(&self) -> bool;
    fn close(&mut self);
    fn error(&mut self, e: V);
    fn desired_size(&self) -> Option<f64>;
    fn is_errored(&self) -> bool;
    fn stored_error(&self) -> Option<V>;
}

/// Lado de escrita do par (writable.rs).
pub trait Writable<V> {
    fn is_writable(&self) -> bool;
    fn is_erroring(&self) -> bool;
    fn is_errored(&self) -> bool;
    fn stored_error(&self) -> Option<V>;
    /// `writableStreamDefaultControllerErrorIfNeeded`.
    fn error_if_needed(&mut self, e: V);
}

struct PendingWrite<V> {
    chunk: V,
    result: P,
}

pub struct Core<V: Clone> {
    pub kind: Kind,
    pub backpressure: bool,
    pub effects: Vec<Effect<V>>,
    promises: Vec<PState>,
    backpressure_change: Option<P>,
    pending_write: HashMap<P, PendingWrite<V>>,
    finish_promise: Option<P>,
    /// `transformStreamDefaultControllerClearAlgorithms` já rodou (kind virou Identity, métodos soltos).
    pub algorithms_cleared: bool,
}

impl<V: Clone> Core<V> {
    /// Cada valor JS que o núcleo segura: os efeitos ainda por aplicar e os chunks de escrita pendentes.
    pub fn for_each_value(&self, mut visit: impl FnMut(&V)) {
        for effect in &self.effects {
            match effect {
                Effect::Reject(_, value) | Effect::CallTransform(value, _) | Effect::CallCancel(value, _) => visit(value),
                Effect::NewPromise(_) | Effect::Resolve(_) | Effect::ResolveWith(..) | Effect::CallFlush => {}
            }
        }
        self.pending_write.values().for_each(|pending| visit(&pending.chunk));
    }

    /// `initializeTransformStream`: backpressure começa `false` e `set_backpressure(true)` o liga.
    pub fn new(kind: Kind) -> Self {
        let mut c = Core {
            kind,
            backpressure: false,
            effects: Vec::new(),
            promises: Vec::new(),
            backpressure_change: None,
            pending_write: HashMap::new(),
            finish_promise: None,
            algorithms_cleared: false,
        };
        c.set_backpressure(true);
        c
    }

    fn new_promise(&mut self) -> P {
        let id = self.promises.len() as P;
        self.promises.push(PState::Pending);
        self.effects.push(Effect::NewPromise(id));
        id
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

    /// Promessa de mudança de backpressure atual (a que o sink do writable espera).
    pub fn backpressure_change_promise(&self) -> Option<P> {
        self.backpressure_change
    }

    /// `transformStreamSetBackpressure`.
    pub fn set_backpressure(&mut self, backpressure: bool) {
        debug_assert!(self.backpressure != backpressure);
        if let Some(prev) = self.backpressure_change {
            self.resolve(prev);
        }
        self.backpressure_change = Some(self.new_promise());
        self.backpressure = backpressure;
    }

    /// `transformStreamUnblockWrite`.
    pub fn unblock_write(&mut self) {
        if self.backpressure {
            self.set_backpressure(false);
        }
    }

    /// `transformStreamDefaultControllerClearAlgorithms`.
    pub fn clear_algorithms(&mut self) {
        self.kind = Kind::Identity;
        self.algorithms_cleared = true;
    }

    /// `transformStreamErrorWritableAndUnblockWrite`.
    pub fn error_writable_and_unblock_write<W: Writable<V>>(&mut self, w: &mut W, e: V) {
        self.clear_algorithms();
        w.error_if_needed(e);
        self.unblock_write();
    }

    /// `transformStreamError` (e `controller.error(e)`).
    pub fn error<R: Readable<V>, W: Writable<V>>(&mut self, r: &mut R, w: &mut W, e: V) {
        if r.controller_present() {
            r.error(e.clone());
        }
        self.error_writable_and_unblock_write(w, e);
    }

    /// `controller.enqueue(chunk)`. `Err(v)` é a exceção que o método lança.
    pub fn controller_enqueue<R: Readable<V>, W: Writable<V>>(&mut self, r: &mut R, w: &mut W, chunk: V) -> Result<(), EnqueueError<V>> {
        Self::enqueue_check(r)?;
        let pushed = r.enqueue(chunk);
        self.enqueue_finish(r, w, pushed)
    }

    /// Primeira metade do `controller.enqueue`: o readable ainda aceita chunk? Não usa o estado, então o integrador
    /// pode rodá-la sem tê-lo em mãos.
    pub fn enqueue_check<R: Readable<V>>(r: &R) -> Result<(), EnqueueError<V>> {
        if !r.controller_present() || !r.can_close_or_enqueue() {
            return Err(EnqueueError::Closed(
                "Cannot enqueue a chunk into a TransformStream whose readable side is closed or has already requested close",
            ));
        }
        Ok(())
    }

    /// Segunda metade do `controller.enqueue`, depois de `r.enqueue(chunk)`. O `size()` do usuário roda dentro de
    /// `r.enqueue`, e uma volta ao controlador por dentro dele age de verdade porque o estado fica fora dessa chamada.
    pub fn enqueue_finish<R: Readable<V>, W: Writable<V>>(&mut self, r: &mut R, w: &mut W, pushed: Result<(), V>) -> Result<(), EnqueueError<V>> {
        if let Err(e) = pushed {
            self.error_writable_and_unblock_write(w, e);
            // Se o `size()` fechou o readable antes de lançar, não há storedError: lança undefined (None).
            return Err(EnqueueError::Stored(r.stored_error()));
        }
        let bp = r.has_backpressure();
        if bp != self.backpressure {
            self.set_backpressure(true);
        }
        Ok(())
    }

    /// `controller.terminate()`; o TypeError "The TransformStream has been terminated" o chamador monta e passa.
    pub fn controller_terminate<R: Readable<V>, W: Writable<V>>(&mut self, r: &mut R, w: &mut W, terminated: V) {
        if r.controller_present() {
            r.close();
        }
        self.error_writable_and_unblock_write(w, terminated);
    }

    /// `controller.desiredSize`: `None` vira `null`.
    pub fn desired_size<R: Readable<V>>(&self, r: &R) -> Option<f64> {
        if !r.controller_present() {
            return None;
        }
        r.desired_size()
    }

    /// `transformStreamDefaultControllerPerformTransform`: o integrador roda o algoritmo (o padrão chama
    /// `controller_enqueue`) e devolve o resultado em `on_transform_done`. Devolve a promessa do resultado.
    pub fn perform_transform(&mut self, chunk: V) -> P {
        let result = self.new_promise();
        self.effects.push(Effect::CallTransform(chunk, result));
        result
    }

    /// Rejeição do algoritmo de transform: `onTSPerformTransformRejected` erra o stream e relança.
    pub fn on_transform_done<R: Readable<V>, W: Writable<V>>(&mut self, r: &mut R, w: &mut W, result: P, outcome: Result<(), V>) {
        match outcome {
            Ok(()) => self.resolve(result),
            Err(e) => {
                self.error(r, w, e.clone());
                self.reject(result, e);
            }
        }
    }

    /// `transformStreamDefaultSinkWriteAlgorithm`. Com backpressure, guarda o chunk e espera a promessa de mudança;
    /// o integrador chama `on_backpressure_change_fulfilled(result)` quando ela cumpre.
    pub fn sink_write(&mut self, chunk: V) -> P {
        if self.backpressure {
            let result = self.new_promise();
            self.pending_write.insert(result, PendingWrite { chunk, result });
            return result;
        }
        self.perform_transform(chunk)
    }

    /// `onTSSinkWriteBackpressureChangeFulfilled`: com o writable em Erroring, rejeita com o storedError dele.
    pub fn on_backpressure_change_fulfilled<W: Writable<V>>(&mut self, w: &W, result: P) {
        let Some(pw) = self.pending_write.remove(&result) else { return };
        if w.is_erroring() {
            if let Some(e) = w.stored_error() {
                self.reject(pw.result, e);
            }
            return;
        }
        // O resultado do transform encadeia no `result` original: o integrador liga a nova promessa a ele.
        let inner = self.perform_transform(pw.chunk);
        self.effects.push(Effect::ResolveWith(pw.result, inner));
    }

    fn start_finish(&mut self) -> Result<P, P> {
        if let Some(p) = self.finish_promise {
            return Err(p);
        }
        let p = self.new_promise();
        self.finish_promise = Some(p);
        Ok(p)
    }

    /// `transformStreamDefaultSinkAbortAlgorithm`. Devolve a finishPromise; o integrador roda o cancel só se
    /// `Effect::CallCancel(reason, true)` foi emitido, e chama `on_cancel_done`.
    pub fn sink_abort(&mut self, reason: V) -> P {
        match self.start_finish() {
            Err(p) => p,
            Ok(p) => {
                self.effects.push(Effect::CallCancel(reason, true));
                self.clear_algorithms();
                p
            }
        }
    }

    /// `transformStreamDefaultSourceCancelAlgorithm`.
    pub fn source_cancel(&mut self, reason: V) -> P {
        match self.start_finish() {
            Err(p) => p,
            Ok(p) => {
                self.effects.push(Effect::CallCancel(reason, false));
                self.clear_algorithms();
                p
            }
        }
    }

    /// `transformStreamDefaultSinkCloseAlgorithm`.
    pub fn sink_close(&mut self) -> P {
        match self.start_finish() {
            Err(p) => p,
            Ok(p) => {
                self.effects.push(Effect::CallFlush);
                self.clear_algorithms();
                p
            }
        }
    }

    /// `onTSSinkAbortCancel*` (`from_sink = true`) e `onTSSourceCancel*` (`false`).
    pub fn on_cancel_done<R: Readable<V>, W: Writable<V>>(&mut self, r: &mut R, w: &mut W, from_sink: bool, reason: V, outcome: Result<(), V>) {
        let Some(finish) = self.finish_promise else { return };
        if from_sink {
            match outcome {
                Ok(()) => {
                    if r.is_errored() {
                        if let Some(e) = r.stored_error() {
                            return self.reject(finish, e);
                        }
                    }
                    if r.controller_present() {
                        r.error(reason);
                    }
                    self.resolve(finish);
                }
                Err(e) => {
                    if r.controller_present() {
                        r.error(e.clone());
                    }
                    self.reject(finish, e);
                }
            }
        } else {
            match outcome {
                Ok(()) => {
                    if w.is_errored() {
                        if let Some(e) = w.stored_error() {
                            return self.reject(finish, e);
                        }
                    }
                    w.error_if_needed(reason);
                    self.unblock_write();
                    self.resolve(finish);
                }
                Err(e) => {
                    w.error_if_needed(e.clone());
                    self.unblock_write();
                    self.reject(finish, e);
                }
            }
        }
    }

    /// `onTSSinkCloseFlush*`.
    pub fn on_flush_done<R: Readable<V>>(&mut self, r: &mut R, outcome: Result<(), V>) {
        let Some(finish) = self.finish_promise else { return };
        match outcome {
            Ok(()) => {
                if r.is_errored() {
                    if let Some(e) = r.stored_error() {
                        return self.reject(finish, e);
                    }
                }
                if r.controller_present() {
                    r.close();
                }
                self.resolve(finish);
            }
            Err(e) => {
                if r.controller_present() {
                    r.error(e.clone());
                }
                self.reject(finish, e);
            }
        }
    }

    /// `transformStreamDefaultSourcePullAlgorithm`: desliga o backpressure e devolve a promessa de mudança
    /// (a próxima pull só cumpre quando o lado de escrita voltar a empurrar).
    pub fn source_pull(&mut self) -> Option<P> {
        self.set_backpressure(false);
        self.backpressure_change
    }
}

/// Exceção do `controller.enqueue`.
pub enum EnqueueError<V> {
    /// `TypeError` com essa mensagem.
    Closed(&'static str),
    /// Lança o `storedError` do readable (`None` = `undefined`).
    Stored(Option<V>),
}
