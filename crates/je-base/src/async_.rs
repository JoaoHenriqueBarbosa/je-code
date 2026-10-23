//! Porte de `src/vs/base/common/async.ts` (`async` é palavra reservada do Rust, daí `async_`).
//!
//! Tudo roda sobre o [`runtime`] (microtarefas e timers na ordem do JavaScript, com relógio virtual nos
//! testes) e a [`Promise`](crate::promise::Promise) do porte. Decisões de tradução:
//!
//! - **Funções `async`**: [`async_fn`] roda o corpo já, de forma síncrona, até o primeiro `await`;
//!   `return v` resolve, erro devolvido com `?` (ou pânico de [`errors::throw`]) rejeita. Quando o
//!   original devolve uma promessa de dentro de `async` (a adoção custa duas rodadas), use
//!   [`async_fn_maybe`]. `await x` é `x.await` (uma rodada, como no motor JS); `await` de um valor que
//!   pode não ser promessa é [`await_maybe`].
//! - **`T | Promise<T>`** é [`MaybePromise`].
//! - **`AsyncIterable`/`AsyncIterator`**: dois traits, [`AsyncIterable`] e [`AsyncIterator`], com
//!   `next() -> Promise<Option<T>>` (`None` é o `{ done: true }`) e `return`/`throw` opcionais (o método
//!   ausente do JS é `None`). O `for await` é [`for_await`], que fecha o iterador (`return`) ao sair
//!   antes do fim, como o JS. Todo o módulo usa essa representação.
//! - **Valores `null`/`undefined`** viram `Option` onde o original os usa de propósito.
//! - **Tarefas** (`ITask`, `ICancellableTask`): `FnOnce`, porque cada fábrica roda uma vez.
//! - **Estado compartilhado com callbacks** (`this` capturado por timers e reações) fica em `Rc` com
//!   mutabilidade interior, e as classes que o original descarta com `dispose()` implementam
//!   [`IDisposable`].
//!
//! Divergências de semântica, todas por tipos que o JS não precisa declarar:
//! - [`Throttler`] e [`ThrottledDelayer`] exigem `T: Default`: descartado com tarefa enfileirada, o
//!   original resolve a promessa pendente com `undefined`, aqui com `T::default()` (`()` no uso real).
//! - [`create_cancelable_promise`] não descarta o valor ao cancelar (o `isDisposable(value)` do original
//!   é uma checagem em tempo de execução); [`create_cancelable_promise_disposing`] faz isso para `T: IDisposable`.
//! - [`first`] não tem o predicado padrão `t => !!t`: o chamador passa o predicado.
//! - [`sequence`] descarta os `None` (o `undefined`/`null` do original).
//! - `AsyncIterableObject.coalesce` e `AsyncIterableProducer.coalesce` descartam só os `None` (o `!!item`
//!   do original também descartaria `0`, `''` e `false`).
//! - `AsyncIterableObject.EMPTY`/`AsyncIterableProducer.EMPTY` são as funções `empty()`, que criam um novo.
//! - `runWhenGlobalIdle` só tem o caminho de `setTimeout0` (sem `requestIdleCallback`, que não existe em Rust).
//! - [`ResourceQueue`] guarda o [`ExtUri`] padrão (o `defaultExtUri` do original é um singleton global).
//! - `isThenable` e `ValueCallback` não têm equivalente (o sistema de tipos já diz o que é promessa).

use std::cell::{Cell, RefCell};
use std::collections::VecDeque;
use std::future::Future;
use std::hash::Hash;
use std::ops::{ControlFlow, Deref};
use std::pin::Pin;
use std::rc::{Rc, Weak};
use std::task::{Context, Poll};

pub use crate::promise::{CancelablePromise, DeferredPromise, MaybePromise};

use crate::cancellation::{CancellationToken, CancellationTokenSource};
use crate::errors::{self, Error};
use crate::event::{Emitter, Event};
use crate::js_string::JsString;
use crate::lazy::Lazy;
use crate::lifecycle::{Disposable, DisposableMap, DisposableRef, DisposableStore, IDisposable, MutableDisposable, to_disposable};
use crate::map::OrderedMap;
use crate::promise::{Promise, Resolver, Thenable};
use crate::resources::ExtUri;
use crate::runtime::{self, TimerId};
use crate::symbols::Delay;
use crate::uri::URI;

/// `Task<T>`: `() => Promise<T> | T`.
pub type Task<T> = Box<dyn FnOnce() -> MaybePromise<T>>;

// region: funções async

struct AsyncFn<T: Clone + 'static> {
    body: Pin<Box<dyn Future<Output = Result<MaybePromise<T>, Error>>>>,
    resolver: Resolver<T>,
}

impl<T: Clone + 'static> Future for AsyncFn<T> {
    type Output = ();

    fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<()> {
        let this = &mut *self;
        match errors::catch(|| this.body.as_mut().poll(cx)) {
            Ok(Poll::Pending) => Poll::Pending,
            Ok(Poll::Ready(Ok(value))) => {
                this.resolver.resolve_maybe(value);
                Poll::Ready(())
            }
            Ok(Poll::Ready(Err(error))) | Err(error) => {
                this.resolver.reject(error);
                Poll::Ready(())
            }
        }
    }
}

/// Uma função `async` cujo `return` devolve `T | Promise<T>`: a promessa devolvida é adotada pela
/// promessa da função (duas rodadas a mais, como no JavaScript).
pub fn async_fn_maybe<T: Clone + 'static>(body: impl Future<Output = Result<MaybePromise<T>, Error>> + 'static) -> Promise<T> {
    let (promise, resolver) = Promise::new();
    runtime::spawn_local(AsyncFn { body: Box::pin(body), resolver });
    promise
}

/// Uma função `async`: o corpo roda já, de forma síncrona, até o primeiro `await`.
pub fn async_fn<T: Clone + 'static>(body: impl Future<Output = Result<T, Error>> + 'static) -> Promise<T> {
    async_fn_maybe(async move { body.await.map(MaybePromise::Value) })
}

/// `await valor` quando `valor` pode ou não ser uma promessa: uma rodada nos dois casos.
pub async fn await_maybe<T: Clone + 'static>(value: MaybePromise<T>) -> Result<T, Error> {
    match value {
        MaybePromise::Value(value) => Promise::resolved(value).await,
        MaybePromise::Promise(promise) => promise.await,
    }
}

/// A promessa de uma função `async` sem `await` que devolve `T | Promise<T>`.
fn adopt<T: Clone + 'static>(value: MaybePromise<T>) -> Promise<T> {
    let (promise, resolver) = Promise::new();
    resolver.resolve_maybe(value);
    promise
}

// endregion

// region: promessas cancelável, corrida e utilitários

fn dispose_slot(slot: &RefCell<Option<DisposableRef>>) {
    let taken = slot.borrow_mut().take();
    if let Some(taken) = taken {
        taken.dispose();
    }
}

/// O que descartar do resultado de uma promessa cancelada que chega depois do cancelamento.
type DisposeValue<T> = Rc<dyn Fn(&T)>;

fn create_cancelable_promise_inner<T: Clone + 'static>(
    callback: impl FnOnce(CancellationToken) -> Promise<T>,
    dispose_value: Option<DisposeValue<T>>,
) -> CancelablePromise<T> {
    let source = CancellationTokenSource::new(None);
    let thenable = callback(source.token());
    let is_cancelled = Rc::new(Cell::new(false));
    let promise = {
        let (source, is_cancelled) = (source.clone(), is_cancelled.clone());
        Promise::with_executor(move |resolver| {
            let subscription: Rc<RefCell<Option<DisposableRef>>> = Rc::default();
            let listener = source.token().on_cancellation_requested().listen({
                let (is_cancelled, subscription, resolver) = (is_cancelled.clone(), subscription.clone(), resolver.clone());
                move |_| {
                    is_cancelled.set(true);
                    dispose_slot(&subscription);
                    resolver.reject(Error::cancellation());
                }
            });
            *subscription.borrow_mut() = Some(listener);
            thenable.then_result(move |result| {
                dispose_slot(&subscription);
                source.dispose();
                match result {
                    Ok(value) => {
                        if !is_cancelled.get() {
                            resolver.resolve(value);
                        } else if let Some(dispose_value) = &dispose_value {
                            // a promessa foi cancelada e o resultado é descartável: ele é limpo aqui
                            dispose_value(&value);
                        }
                    }
                    Err(error) => resolver.reject(error),
                }
                Ok(())
            });
        })
    };
    CancelablePromise::new(promise, move || {
        source.cancel();
        source.dispose();
    })
}

/// `createCancelablePromise(callback)`: uma promessa que se cancela com o token entregue ao `callback`.
/// Ao cancelar, ela rejeita com [`Error::cancellation`].
pub fn create_cancelable_promise<T: Clone + 'static>(callback: impl FnOnce(CancellationToken) -> Promise<T>) -> CancelablePromise<T> {
    create_cancelable_promise_inner(callback, None)
}

/// Como [`create_cancelable_promise`] para um resultado `IDisposable`: se a promessa foi cancelada e o
/// resultado chega depois, ele é descartado.
pub fn create_cancelable_promise_disposing<T: Clone + IDisposable + 'static>(callback: impl FnOnce(CancellationToken) -> Promise<T>) -> CancelablePromise<T> {
    create_cancelable_promise_inner(callback, Some(Rc::new(|value: &T| value.dispose())))
}

/// A corrida entre a promessa e o cancelamento do token: `on_cancel` assenta o resultado quando o token
/// cancela, `map` converte o valor da promessa.
fn race_token<T: Clone + 'static, R: Clone + 'static>(
    promise: &Promise<T>,
    token: &CancellationToken,
    on_cancel: impl Fn(&Resolver<R>) + 'static,
    map: impl FnOnce(T) -> R + 'static,
) -> Promise<R> {
    let promise = promise.clone();
    let token = token.clone();
    Promise::with_executor(move |resolver| {
        let reference: Rc<RefCell<Option<DisposableRef>>> = Rc::default();
        let listener = token.on_cancellation_requested().listen({
            let (reference, resolver) = (reference.clone(), resolver.clone());
            move |_| {
                dispose_slot(&reference);
                on_cancel(&resolver);
            }
        });
        *reference.borrow_mut() = Some(listener);
        promise
            .then_result(move |result| {
                match result {
                    Ok(value) => resolver.resolve(map(value)),
                    Err(error) => resolver.reject(error),
                }
                Ok(())
            })
            .finally(move || dispose_slot(&reference));
    })
}

/// `raceCancellation(promise, token)`: resolve com `None` assim que o token cancela.
pub fn race_cancellation<T: Clone + 'static>(promise: &Promise<T>, token: &CancellationToken) -> Promise<Option<T>> {
    race_token(promise, token, |resolver| resolver.resolve(None), Some)
}

/// `raceCancellation(promise, token, defaultValue)`: resolve com `default_value` assim que o token cancela.
pub fn race_cancellation_or<T: Clone + 'static>(promise: &Promise<T>, token: &CancellationToken, default_value: T) -> Promise<T> {
    race_token(promise, token, move |resolver| resolver.resolve(default_value.clone()), |value| value)
}

/// `raceCancellationError(promise, token)`: rejeita com [`Error::cancellation`] assim que o token cancela.
pub fn race_cancellation_error<T: Clone + 'static>(promise: &Promise<T>, token: &CancellationToken) -> Promise<T> {
    race_token(promise, token, |resolver| resolver.reject(Error::cancellation()), |value| value)
}

/// `rejectIfNotCanceled(err)`, o tratador de `catch`: engole o erro de cancelamento e devolve uma promessa
/// rejeitada com qualquer outro.
pub fn reject_if_not_canceled(error: Error) -> Promise<()> {
    if errors::is_cancellation_error(&error) { Promise::resolved(()) } else { Promise::rejected(error) }
}

/// `notCancellablePromise(promise)`: a mesma promessa sem o `cancel`.
pub fn not_cancellable_promise<T: Clone + 'static>(promise: &CancelablePromise<T>) -> Promise<T> {
    let (result, resolver) = Promise::new();
    promise.pipe_to(&resolver);
    result
}

/// `raceCancellablePromises(promises)`: assenta como a primeira a assentar e cancela as demais.
pub fn race_cancellable_promises<T: Clone + 'static>(cancellable_promises: Vec<CancelablePromise<T>>) -> CancelablePromise<T> {
    let resolved_promise_index = Rc::new(Cell::new(usize::MAX));
    let promises: Vec<Promise<T>> = cancellable_promises
        .iter()
        .enumerate()
        .map(|(index, promise)| {
            let resolved_promise_index = resolved_promise_index.clone();
            promise.then(move |result| {
                resolved_promise_index.set(index);
                result
            })
        })
        .collect();
    let race = Promise::race(promises);
    let cancel: Rc<dyn Fn()> = Rc::new(move || {
        for (index, cancellable_promise) in cancellable_promises.iter().enumerate() {
            if index != resolved_promise_index.get() {
                cancellable_promise.cancel();
            }
        }
    });
    let on_settled = cancel.clone();
    race.then_result(move |_| {
        on_settled();
        Ok(())
    });
    CancelablePromise::new(race, move || cancel())
}

/// `raceTimeout(promise, timeout, onTimeout)`: resolve com `None` se o prazo vence antes da promessa.
pub fn race_timeout<T: Clone + 'static>(promise: &Promise<T>, timeout_ms: u64, on_timeout: Option<Box<dyn FnOnce()>>) -> Promise<Option<T>> {
    let (timed_out, timed_out_resolver) = Promise::<Option<T>>::new();
    let timer = runtime::set_timeout(
        move || {
            timed_out_resolver.resolve(None);
            if let Some(on_timeout) = on_timeout {
                on_timeout();
            }
        },
        timeout_ms,
    );
    let finished = promise.finally(move || runtime::clear_timeout(timer));
    // `Promise.race([finished, timedOut])`: cada elemento registra a sua reação, na ordem.
    let (race, resolver) = Promise::new();
    let on_finished = resolver.clone();
    finished.react(move |result| match result {
        Ok(value) => on_finished.resolve(Some(value)),
        Err(error) => on_finished.reject(error),
    });
    timed_out.react(move |result| match result {
        Ok(value) => resolver.resolve(value),
        Err(error) => resolver.reject(error),
    });
    race
}

/// `asPromise(callback)` para um retorno que não é promessa.
pub fn as_promise<T: Clone + 'static>(callback: impl FnOnce() -> T) -> Promise<T> {
    Promise::with_executor(|resolver| resolver.resolve(callback()))
}

/// `asPromise(callback)` para um retorno que é promessa.
pub fn as_promise_thenable<T: Clone + 'static>(callback: impl FnOnce() -> Promise<T>) -> Promise<T> {
    Promise::with_executor(|resolver| callback().pipe_to(&resolver))
}

/// `promiseWithResolvers()`.
pub fn promise_with_resolvers<T: Clone + 'static>() -> (Promise<T>, Resolver<T>) {
    Promise::new()
}

/// `p.then(() => this.next(), () => this.next())`: roda `task` uma rodada depois de `previous` assentar
/// (cumprida ou rejeitada) e adota a promessa devolvida.
fn chain_after<T: Clone + 'static>(previous: &dyn Thenable, task: impl FnOnce() -> Promise<T> + 'static) -> Promise<T> {
    let (derived, resolver) = Promise::new();
    previous.on_settled(Box::new(move || match errors::catch(task) {
        Ok(promise) => resolver.resolve_with(&promise),
        Err(error) => resolver.reject(error),
    }));
    derived
}

// endregion

// region: Throttler, Sequencer

type CancellableTask<T> = Box<dyn FnOnce(CancellationToken) -> Promise<T>>;

struct ThrottlerInner<T: Clone + Default + 'static> {
    active_promise: RefCell<Option<Promise<T>>>,
    queued_promise: RefCell<Option<Promise<T>>>,
    queued_promise_factory: RefCell<Option<CancellableTask<T>>>,
    cancellation_token_source: CancellationTokenSource,
}

/// `Throttler`: evita o acúmulo de tarefas assíncronas em sequência. Enquanto uma roda, as chamadas de
/// `queue` só guardam a última fábrica, que roda uma vez quando a ativa termina.
pub struct Throttler<T: Clone + Default + 'static> {
    inner: Rc<ThrottlerInner<T>>,
}

impl<T: Clone + Default + 'static> Default for Throttler<T> {
    fn default() -> Self {
        Self::new()
    }
}

impl<T: Clone + Default + 'static> Throttler<T> {
    pub fn new() -> Self {
        Self {
            inner: Rc::new(ThrottlerInner {
                active_promise: RefCell::new(None),
                queued_promise: RefCell::new(None),
                queued_promise_factory: RefCell::new(None),
                cancellation_token_source: CancellationTokenSource::new(None),
            }),
        }
    }

    pub fn queue(&self, promise_factory: impl FnOnce(CancellationToken) -> Promise<T> + 'static) -> Promise<T> {
        ThrottlerInner::queue(&self.inner, Box::new(promise_factory))
    }

    /// Gancho de teste: a fábrica enfileirada ainda está guardada (`throttler.queuedPromiseFactory`).
    pub fn has_queued_factory(&self) -> bool {
        self.inner.queued_promise_factory.borrow().is_some()
    }
}

impl<T: Clone + Default + 'static> ThrottlerInner<T> {
    fn queue(this: &Rc<Self>, promise_factory: CancellableTask<T>) -> Promise<T> {
        if this.cancellation_token_source.token().is_cancellation_requested() {
            return Promise::rejected(Error::new("Throttler is disposed"));
        }

        let active = this.active_promise.borrow().clone();
        if let Some(active) = active {
            *this.queued_promise_factory.borrow_mut() = Some(promise_factory);

            if this.queued_promise.borrow().is_none() {
                let on_complete = {
                    let this = this.clone();
                    move || -> MaybePromise<T> {
                        *this.queued_promise.borrow_mut() = None;

                        if this.cancellation_token_source.token().is_cancellation_requested() {
                            return MaybePromise::Value(T::default());
                        }

                        let factory = this.queued_promise_factory.borrow_mut().take().expect("a fábrica enfileirada");
                        MaybePromise::Promise(Self::queue(&this, factory))
                    }
                };
                let on_complete = Rc::new(RefCell::new(Some(on_complete)));
                let (queued, queued_resolver) = Promise::new();
                // `activePromise.then(onComplete, onComplete).then(resolve, reject)`
                let after_complete = active.then_result_maybe(move |_| (on_complete.borrow_mut().take().expect("onComplete roda uma vez"))());
                after_complete.pipe_to(&queued_resolver);
                *this.queued_promise.borrow_mut() = Some(queued);
            }

            let queued = this.queued_promise.borrow().clone().expect("a promessa enfileirada");
            let (result, resolver) = Promise::new();
            queued.pipe_to(&resolver);
            return result;
        }

        let active = promise_factory(this.cancellation_token_source.token());
        *this.active_promise.borrow_mut() = Some(active.clone());

        let (result, resolver) = Promise::new();
        let this = this.clone();
        active.then_result(move |outcome| {
            *this.active_promise.borrow_mut() = None;
            match outcome {
                Ok(value) => resolver.resolve(value),
                Err(error) => resolver.reject(error),
            }
            Ok(())
        });
        result
    }
}

impl<T: Clone + Default + 'static> IDisposable for Throttler<T> {
    fn dispose(&self) {
        self.inner.cancellation_token_source.cancel();
        *self.inner.queued_promise_factory.borrow_mut() = None;
    }
}

/// `Sequencer`: roda as tarefas uma depois da outra, mesmo que alguma rejeite.
pub struct Sequencer {
    current: RefCell<Rc<dyn Thenable>>,
}

impl Default for Sequencer {
    fn default() -> Self {
        Self::new()
    }
}

impl Sequencer {
    pub fn new() -> Self {
        Self { current: RefCell::new(Rc::new(Promise::resolved(()))) }
    }

    pub fn queue<T: Clone + 'static>(&self, promise_task: impl FnOnce() -> Promise<T> + 'static) -> Promise<T> {
        let previous = self.current.borrow().clone();
        let next = chain_after(&*previous, promise_task);
        *self.current.borrow_mut() = Rc::new(next.clone());
        next
    }
}

/// Um [`Throttler`] por chave: as chamadas da mesma chave se fundem (só a última tarefa roda depois da
/// ativa), as de chaves diferentes são independentes, e a chave ociosa é removida.
/// O throttler de uma chave e quantas chamadas dela ainda estão em curso.
type ThrottlerEntry<T> = (Rc<Throttler<T>>, Rc<Cell<usize>>);

pub struct ThrottlerByKey<K: Eq + Hash + Clone + 'static, T: Clone + Default + 'static> {
    throttlers: Rc<RefCell<OrderedMap<K, ThrottlerEntry<T>>>>,
}

impl<K: Eq + Hash + Clone + 'static, T: Clone + Default + 'static> Default for ThrottlerByKey<K, T> {
    fn default() -> Self {
        Self::new()
    }
}

impl<K: Eq + Hash + Clone + 'static, T: Clone + Default + 'static> ThrottlerByKey<K, T> {
    pub fn new() -> Self {
        Self { throttlers: Rc::new(RefCell::new(OrderedMap::new())) }
    }

    pub fn queue(&self, key: K, task: impl FnOnce() -> Promise<T> + 'static) -> Promise<T> {
        let existing = self.throttlers.borrow().get(&key).cloned();
        let (throttler, count) = existing.unwrap_or_else(|| {
            let entry = (Rc::new(Throttler::new()), Rc::new(Cell::new(0)));
            self.throttlers.borrow_mut().insert(key.clone(), entry.clone());
            entry
        });

        count.set(count.get() + 1);
        let throttlers = self.throttlers.clone();
        throttler.clone().queue(move |_| task()).finally(move || {
            count.set(count.get() - 1);
            if count.get() == 0 {
                throttler.dispose();
                throttlers.borrow_mut().remove(&key);
            }
        })
    }
}

impl<K: Eq + Hash + Clone + 'static, T: Clone + Default + 'static> IDisposable for ThrottlerByKey<K, T> {
    fn dispose(&self) {
        let all: Vec<_> = self.throttlers.borrow().values().map(|(throttler, _)| throttler.clone()).collect();
        for throttler in all {
            throttler.dispose();
        }
        self.throttlers.borrow_mut().clear();
    }
}

/// `SequencerByKey`: um [`Sequencer`] por chave.
pub struct SequencerByKey<K: Eq + Hash + Clone + 'static> {
    promise_map: Rc<RefCell<OrderedMap<K, Rc<dyn Thenable>>>>,
}

impl<K: Eq + Hash + Clone + 'static> Default for SequencerByKey<K> {
    fn default() -> Self {
        Self::new()
    }
}

impl<K: Eq + Hash + Clone + 'static> SequencerByKey<K> {
    pub fn new() -> Self {
        Self { promise_map: Rc::new(RefCell::new(OrderedMap::new())) }
    }

    pub fn queue<T: Clone + 'static>(&self, key: K, promise_task: impl FnOnce() -> Promise<T> + 'static) -> Promise<T> {
        let running: Rc<dyn Thenable> = self.promise_map.borrow().get(&key).cloned().unwrap_or_else(|| Rc::new(Promise::resolved(())));
        // `runningPromise.catch(() => { })`
        let (recovered, recovered_resolver) = Promise::<()>::new();
        running.on_settled(Box::new(move || recovered_resolver.resolve(())));
        let id = Rc::new(Cell::new(0));
        let new_promise = {
            let (promise_map, key, id) = (self.promise_map.clone(), key.clone(), id.clone());
            recovered.then_promise(move |_| promise_task()).finally(move || {
                let is_current = promise_map.borrow().get(&key).is_some_and(|current| current.identity() == id.get());
                if is_current {
                    promise_map.borrow_mut().remove(&key);
                }
            })
        };
        id.set(new_promise.identity());
        self.promise_map.borrow_mut().insert(key, Rc::new(new_promise.clone()));
        new_promise
    }

    pub fn peek(&self, key: &K) -> Option<Rc<dyn Thenable>> {
        self.promise_map.borrow().get(key).cloned()
    }

    pub fn keys(&self) -> Vec<K> {
        self.promise_map.borrow().keys().cloned().collect()
    }
}

// endregion

// region: Delayer, ThrottledDelayer, Barrier, timeout

/// `IScheduledLater`.
trait ScheduledLater {
    fn is_triggered(&self) -> bool;
    fn dispose(&self);
}

struct TimeoutDeferred {
    scheduled: Rc<Cell<bool>>,
    handle: TimerId,
}

impl ScheduledLater for TimeoutDeferred {
    fn is_triggered(&self) -> bool {
        self.scheduled.get()
    }

    fn dispose(&self) {
        runtime::clear_timeout(self.handle);
        self.scheduled.set(false);
    }
}

fn timeout_deferred(timeout_ms: u64, f: impl FnOnce() + 'static) -> Rc<dyn ScheduledLater> {
    let scheduled = Rc::new(Cell::new(true));
    let flag = scheduled.clone();
    let handle = runtime::set_timeout(
        move || {
            flag.set(false);
            f();
        },
        timeout_ms,
    );
    Rc::new(TimeoutDeferred { scheduled, handle })
}

struct MicrotaskDeferred {
    scheduled: Rc<Cell<bool>>,
}

impl ScheduledLater for MicrotaskDeferred {
    fn is_triggered(&self) -> bool {
        self.scheduled.get()
    }

    fn dispose(&self) {
        self.scheduled.set(false);
    }
}

fn microtask_deferred(f: impl FnOnce() + 'static) -> Rc<dyn ScheduledLater> {
    let scheduled = Rc::new(Cell::new(true));
    let flag = scheduled.clone();
    runtime::queue_microtask(move || {
        if flag.get() {
            flag.set(false);
            f();
        }
    });
    Rc::new(MicrotaskDeferred { scheduled })
}

struct DelayerInner<T: Clone + 'static> {
    deferred: RefCell<Option<Rc<dyn ScheduledLater>>>,
    completion_promise: RefCell<Option<Promise<T>>>,
    do_resolve: RefCell<Option<Resolver<()>>>,
    do_reject: RefCell<Option<Resolver<()>>>,
    task: RefCell<Option<Task<T>>>,
}

impl<T: Clone + 'static> DelayerInner<T> {
    fn cancel_timeout(&self) {
        let deferred = self.deferred.borrow_mut().take();
        if let Some(deferred) = deferred {
            deferred.dispose();
        }
    }
}

/// `Delayer`: adia (debounce) a execução de uma tarefa pedida com frequência; só a última roda.
pub struct Delayer<T: Clone + 'static> {
    inner: Rc<DelayerInner<T>>,
    pub default_delay: Cell<Delay>,
}

impl<T: Clone + 'static> Delayer<T> {
    pub fn new(default_delay: Delay) -> Self {
        Self {
            inner: Rc::new(DelayerInner {
                deferred: RefCell::new(None),
                completion_promise: RefCell::new(None),
                do_resolve: RefCell::new(None),
                do_reject: RefCell::new(None),
                task: RefCell::new(None),
            }),
            default_delay: Cell::new(default_delay),
        }
    }

    /// `trigger(task, delay)`: `delay` ausente usa o `default_delay`.
    pub fn trigger(&self, task: impl FnOnce() -> MaybePromise<T> + 'static, delay: Option<Delay>) -> Promise<T> {
        let delay = delay.unwrap_or(self.default_delay.get());
        let inner = &self.inner;
        *inner.task.borrow_mut() = Some(Box::new(task));
        inner.cancel_timeout();

        if inner.completion_promise.borrow().is_none() {
            let (trigger_promise, trigger_resolver) = Promise::<()>::new();
            *inner.do_resolve.borrow_mut() = Some(trigger_resolver.clone());
            *inner.do_reject.borrow_mut() = Some(trigger_resolver);

            let mine: Rc<RefCell<Option<Promise<T>>>> = Rc::default();
            let completion_promise = {
                let (inner, mine) = (inner.clone(), mine.clone());
                trigger_promise.then_maybe(move |()| {
                    let current = inner.completion_promise.borrow().clone();
                    let is_mine = match (&current, &*mine.borrow()) {
                        (Some(current), Some(mine)) => current.ptr_eq(mine),
                        _ => false,
                    };
                    if !is_mine {
                        // canceled after the delay elapsed, possibly followed by a new trigger
                        errors::throw(Error::cancellation());
                    }
                    *inner.completion_promise.borrow_mut() = None;
                    *inner.do_resolve.borrow_mut() = None;
                    let task = inner.task.borrow_mut().take().expect("a tarefa do trigger");
                    task()
                })
            };
            *mine.borrow_mut() = Some(completion_promise.clone());
            *inner.completion_promise.borrow_mut() = Some(completion_promise);
        }

        let fire = {
            let inner = inner.clone();
            move || {
                *inner.deferred.borrow_mut() = None;
                let do_resolve = inner.do_resolve.borrow().clone();
                if let Some(do_resolve) = do_resolve {
                    do_resolve.resolve(());
                }
            }
        };
        *inner.deferred.borrow_mut() = Some(match delay {
            Delay::Microtask => microtask_deferred(fire),
            Delay::Millis(ms) => timeout_deferred(ms, fire),
        });

        inner.completion_promise.borrow().clone().expect("a promessa de conclusão")
    }

    pub fn is_triggered(&self) -> bool {
        self.inner.deferred.borrow().as_ref().is_some_and(|deferred| deferred.is_triggered())
    }

    pub fn cancel(&self) {
        self.inner.cancel_timeout();
        *self.inner.task.borrow_mut() = None;

        if self.inner.completion_promise.borrow().is_some() {
            let do_reject = self.inner.do_reject.borrow().clone();
            if let Some(do_reject) = do_reject {
                do_reject.reject(Error::cancellation());
            }
            *self.inner.completion_promise.borrow_mut() = None;
        }
    }

    /// Gancho de teste: a tarefa pendente ainda está guardada (`delayer.task`).
    pub fn has_task(&self) -> bool {
        self.inner.task.borrow().is_some()
    }
}

impl<T: Clone + 'static> IDisposable for Delayer<T> {
    fn dispose(&self) {
        self.cancel();
    }
}

/// `ThrottledDelayer`: adia a tarefa e, enquanto ela roda, junta as chamadas seguintes numa só.
pub struct ThrottledDelayer<T: Clone + Default + 'static> {
    delayer: Delayer<T>,
    throttler: Rc<Throttler<T>>,
}

impl<T: Clone + Default + 'static> ThrottledDelayer<T> {
    pub fn new(default_delay: u64) -> Self {
        Self { delayer: Delayer::new(Delay::Millis(default_delay)), throttler: Rc::new(Throttler::new()) }
    }

    pub fn trigger(&self, promise_factory: impl FnOnce(CancellationToken) -> Promise<T> + 'static, delay: Option<u64>) -> Promise<T> {
        let throttler = self.throttler.clone();
        self.delayer.trigger(move || MaybePromise::Promise(throttler.queue(promise_factory)), delay.map(Delay::Millis))
    }

    pub fn is_triggered(&self) -> bool {
        self.delayer.is_triggered()
    }

    pub fn cancel(&self) {
        self.delayer.cancel();
    }

    /// Gancho de teste: o `Delayer` interno ainda guarda a tarefa (`throttledDelayer.delayer.task`).
    pub fn has_delayer_task(&self) -> bool {
        self.delayer.has_task()
    }
}

impl<T: Clone + Default + 'static> IDisposable for ThrottledDelayer<T> {
    fn dispose(&self) {
        self.delayer.dispose();
        self.throttler.dispose();
    }
}

struct BarrierInner {
    is_open: Cell<bool>,
    promise: Promise<bool>,
    resolver: Resolver<bool>,
}

/// `Barrier`: começa fechada e abre para sempre.
#[derive(Clone)]
pub struct Barrier {
    inner: Rc<BarrierInner>,
}

impl Default for Barrier {
    fn default() -> Self {
        Self::new()
    }
}

impl Barrier {
    pub fn new() -> Self {
        let (promise, resolver) = Promise::new();
        Self { inner: Rc::new(BarrierInner { is_open: Cell::new(false), promise, resolver }) }
    }

    pub fn is_open(&self) -> bool {
        self.inner.is_open.get()
    }

    pub fn open(&self) {
        self.inner.is_open.set(true);
        self.inner.resolver.resolve(true);
    }

    pub fn wait(&self) -> Promise<bool> {
        self.inner.promise.clone()
    }
}

/// `AutoOpenBarrier`: abre sozinha depois de um prazo, ou antes, se `open` for chamado.
pub struct AutoOpenBarrier {
    barrier: Barrier,
    timeout: TimerId,
}

impl AutoOpenBarrier {
    pub fn new(auto_open_time_ms: u64) -> Self {
        let barrier = Barrier::new();
        let to_open = barrier.clone();
        let timeout = runtime::set_timeout(move || to_open.open(), auto_open_time_ms);
        Self { barrier, timeout }
    }

    pub fn open(&self) {
        runtime::clear_timeout(self.timeout);
        self.barrier.open();
    }
}

impl Deref for AutoOpenBarrier {
    type Target = Barrier;

    fn deref(&self) -> &Barrier {
        &self.barrier
    }
}

/// `timeout(millis, token)`: resolve depois de `millis`, ou rejeita com [`Error::cancellation`] se o token
/// cancelar antes.
pub fn timeout_with_token(millis: u64, token: &CancellationToken) -> Promise<()> {
    let token = token.clone();
    Promise::with_executor(move |resolver| {
        let disposable: Rc<RefCell<Option<DisposableRef>>> = Rc::default();
        let handle = runtime::set_timeout(
            {
                let (disposable, resolver) = (disposable.clone(), resolver.clone());
                move || {
                    dispose_slot(&disposable);
                    resolver.resolve(());
                }
            },
            millis,
        );
        let listener = token.on_cancellation_requested().listen({
            let disposable = disposable.clone();
            move |_| {
                runtime::clear_timeout(handle);
                dispose_slot(&disposable);
                resolver.reject(Error::cancellation());
            }
        });
        *disposable.borrow_mut() = Some(listener);
    })
}

/// `timeout(millis)`: uma promessa cancelável que resolve depois de `millis`.
pub fn timeout(millis: u64) -> CancelablePromise<()> {
    create_cancelable_promise(move |token| timeout_with_token(millis, &token))
}

/// `disposableTimeout(handler, timeout, store)`: um timer que se cancela com o `dispose()` devolvido e, se
/// houver `store`, sai dele ao disparar.
pub fn disposable_timeout(handler: impl FnOnce() + 'static, timeout_ms: u64, store: Option<Rc<DisposableStore>>) -> DisposableRef {
    let own: Rc<RefCell<Option<Weak<dyn IDisposable>>>> = Rc::default();
    let timer = runtime::set_timeout(
        {
            let (own, has_store) = (own.clone(), store.is_some());
            move || {
                handler();
                if has_store {
                    let disposable = own.borrow().as_ref().and_then(Weak::upgrade);
                    if let Some(disposable) = disposable {
                        disposable.dispose();
                    }
                }
            }
        },
        timeout_ms,
    );
    let disposable = to_disposable({
        let (own, store) = (own.clone(), store.clone());
        move || {
            runtime::clear_timeout(timer);
            if let Some(store) = &store {
                let disposable = own.borrow().as_ref().and_then(Weak::upgrade);
                if let Some(disposable) = disposable {
                    store.delete(&disposable);
                }
            }
        }
    });
    *own.borrow_mut() = Some(Rc::downgrade(&disposable));
    if let Some(store) = &store {
        store.add(disposable.clone());
    }
    disposable
}

/// O maior atraso, em milissegundos, que um `setTimeout` representa (inteiro de 32 bits com sinal); acima
/// disso o JavaScript dispara quase na hora.
pub const MAX_TIMEOUT_DELAY: u64 = (1 << 31) - 1;

struct LongTimeout {
    target: u64,
    timer: Cell<Option<TimerId>>,
    handler: RefCell<Option<Box<dyn FnOnce()>>>,
    store: Option<Rc<DisposableStore>>,
    disposable: RefCell<Option<Weak<dyn IDisposable>>>,
}

impl LongTimeout {
    fn arm(this: &Rc<Self>) {
        let remaining = i128::from(this.target) - i128::from(runtime::date_now());
        if remaining <= 0 {
            let handler = this.handler.borrow_mut().take();
            if let Some(handler) = handler {
                handler();
            }
            if this.store.is_some() {
                let disposable = this.disposable.borrow().as_ref().and_then(Weak::upgrade);
                if let Some(disposable) = disposable {
                    disposable.dispose();
                }
            }
            return;
        }
        let next = this.clone();
        let delay = u64::try_from(remaining).unwrap_or(u64::MAX).min(MAX_TIMEOUT_DELAY);
        this.timer.set(Some(runtime::set_timeout(move || Self::arm(&next), delay)));
    }
}

/// `disposableLongTimeout(handler, timeout, store)`: como [`disposable_timeout`], mas aceita atrasos maiores
/// que [`MAX_TIMEOUT_DELAY`] (a espera é dividida em pedaços e rearmada até chegar ao alvo).
pub fn disposable_long_timeout(handler: impl FnOnce() + 'static, timeout_ms: u64, store: Option<Rc<DisposableStore>>) -> DisposableRef {
    let state = Rc::new(LongTimeout {
        target: runtime::date_now().saturating_add(timeout_ms),
        timer: Cell::new(None),
        handler: RefCell::new(Some(Box::new(handler))),
        store: store.clone(),
        disposable: RefCell::new(None),
    });
    let disposable = to_disposable({
        let state = state.clone();
        move || {
            if let Some(timer) = state.timer.get() {
                runtime::clear_timeout(timer);
            }
            if let Some(store) = &state.store {
                let disposable = state.disposable.borrow().as_ref().and_then(Weak::upgrade);
                if let Some(disposable) = disposable {
                    store.delete(&disposable);
                }
            }
        }
    });
    *state.disposable.borrow_mut() = Some(Rc::downgrade(&disposable));
    let first = state.clone();
    state.timer.set(Some(runtime::set_timeout(move || LongTimeout::arm(&first), timeout_ms.min(MAX_TIMEOUT_DELAY))));
    if let Some(store) = &store {
        store.add(disposable.clone());
    }
    disposable
}

// endregion

// region: sequence, first, firstParallel

/// `sequence(promiseFactories)`: roda as fábricas em ordem e junta os resultados (os `None` ficam de fora,
/// o `undefined`/`null` do original).
/// `ITask<Promise<T>>`: uma fábrica de promessa que roda uma vez.
pub type PromiseFactory<T> = Box<dyn FnOnce() -> Promise<T>>;

pub fn sequence<T: Clone + 'static>(promise_factories: Vec<PromiseFactory<Option<T>>>) -> Promise<Vec<T>> {
    struct State<T: Clone + 'static> {
        results: RefCell<Vec<T>>,
        factories: RefCell<VecDeque<PromiseFactory<Option<T>>>>,
    }

    fn then_handler<T: Clone + 'static>(state: Rc<State<T>>, result: Option<T>) -> Promise<Vec<T>> {
        if let Some(result) = result {
            state.results.borrow_mut().push(result);
        }

        let next = state.factories.borrow_mut().pop_front();
        if let Some(next) = next {
            return next().then_promise(move |result| then_handler(state, result));
        }

        Promise::resolved(state.results.borrow().clone())
    }

    let state = Rc::new(State { results: RefCell::new(Vec::new()), factories: RefCell::new(promise_factories.into()) });
    Promise::resolved(()).then_promise(move |()| then_handler(state, None))
}

/// `first(promiseFactories, shouldStop, defaultValue)`: o resultado da primeira fábrica que o predicado
/// aceita; as fábricas rodam em sequência.
pub fn first<T: Clone + 'static>(
    promise_factories: Vec<PromiseFactory<T>>,
    should_stop: impl Fn(&T) -> bool + 'static,
    default_value: Option<T>,
) -> Promise<Option<T>> {
    struct State<T: Clone + 'static> {
        factories: RefCell<VecDeque<PromiseFactory<T>>>,
        should_stop: Box<dyn Fn(&T) -> bool>,
        default_value: Option<T>,
    }

    fn run_loop<T: Clone + 'static>(state: Rc<State<T>>) -> Promise<Option<T>> {
        let factory = state.factories.borrow_mut().pop_front();
        let Some(factory) = factory else {
            return Promise::resolved(state.default_value.clone());
        };
        factory().then_promise(move |result| {
            if (state.should_stop)(&result) {
                return Promise::resolved(Some(result));
            }
            run_loop(state)
        })
    }

    run_loop(Rc::new(State { factories: RefCell::new(promise_factories.into()), should_stop: Box::new(should_stop), default_value }))
}

/// `firstParallel(promiseList, shouldStop, defaultValue)`: o resultado da primeira promessa que o
/// predicado aceita, com todas rodando em paralelo; ao decidir, cancela as demais.
pub fn first_parallel<T: Clone + 'static>(
    promise_list: Vec<CancelablePromise<T>>,
    should_stop: impl Fn(&T) -> bool + 'static,
    default_value: Option<T>,
) -> Promise<Option<T>> {
    if promise_list.is_empty() {
        return Promise::resolved(default_value);
    }

    let todo = Rc::new(Cell::new(isize::try_from(promise_list.len()).unwrap_or(isize::MAX)));
    let list = Rc::new(promise_list);
    let finish: Rc<dyn Fn()> = {
        let (todo, list) = (todo.clone(), list.clone());
        Rc::new(move || {
            todo.set(-1);
            for promise in list.iter() {
                promise.cancel();
            }
        })
    };
    let should_stop: Rc<dyn Fn(&T) -> bool> = Rc::new(should_stop);

    let (result, resolver) = Promise::new();
    for promise in list.iter() {
        let (on_value, on_error) = (resolver.clone(), resolver.clone());
        let (todo_value, finish_value, should_stop, default_value) = (todo.clone(), finish.clone(), should_stop.clone(), default_value.clone());
        let (todo_error, finish_error) = (todo.clone(), finish.clone());
        promise
            .then(move |value| {
                todo_value.set(todo_value.get() - 1);
                if todo_value.get() >= 0 && should_stop(&value) {
                    finish_value();
                    on_value.resolve(Some(value));
                } else if todo_value.get() == 0 {
                    on_value.resolve(default_value);
                }
            })
            .catch(move |error| {
                todo_error.set(todo_error.get() - 1);
                if todo_error.get() >= 0 {
                    finish_error();
                    on_error.reject(error);
                }
            });
    }
    result
}

// endregion

// region: Limiter, Queue, LimitedQueue, ResourceQueue

/// `ILimiter<T>`.
pub trait ILimiter<T: Clone + 'static> {
    fn size(&self) -> usize;
    fn queue(&self, factory: Box<dyn FnOnce() -> Promise<T>>) -> Promise<T>;
    fn clear(&self);
}

struct LimitedTaskFactory<T: Clone + 'static> {
    factory: Box<dyn FnOnce() -> Promise<T>>,
    resolver: Resolver<T>,
}

struct LimiterInner<T: Clone + 'static> {
    size: Cell<usize>,
    is_disposed: Cell<bool>,
    running_promises: Cell<usize>,
    max_degree_of_paralellism: usize,
    outstanding_promises: RefCell<VecDeque<LimitedTaskFactory<T>>>,
    on_drained: Rc<Emitter<()>>,
}

impl<T: Clone + 'static> LimiterInner<T> {
    fn consume(this: &Rc<Self>) {
        loop {
            if this.running_promises.get() >= this.max_degree_of_paralellism {
                return;
            }
            let task = this.outstanding_promises.borrow_mut().pop_front();
            let Some(task) = task else { return };
            this.running_promises.set(this.running_promises.get() + 1);

            let promise = (task.factory)();
            promise.pipe_to(&task.resolver);
            let consumed_by = this.clone();
            promise.then_result(move |_| {
                Self::consumed(&consumed_by);
                Ok(())
            });
        }
    }

    fn consumed(this: &Rc<Self>) {
        if this.is_disposed.get() {
            return;
        }
        this.running_promises.set(this.running_promises.get() - 1);
        this.size.set(this.size.get() - 1);
        if this.size.get() == 0 {
            this.on_drained.fire(());
        }

        if !this.outstanding_promises.borrow().is_empty() {
            Self::consume(this);
        }
    }
}

/// `Limiter`: enfileira N promessas e roda no máximo M ao mesmo tempo.
pub struct Limiter<T: Clone + 'static> {
    inner: Rc<LimiterInner<T>>,
}

impl<T: Clone + 'static> Clone for Limiter<T> {
    fn clone(&self) -> Self {
        Self { inner: self.inner.clone() }
    }
}

impl<T: Clone + 'static> Limiter<T> {
    pub fn new(max_degree_of_paralellism: usize) -> Self {
        Self {
            inner: Rc::new(LimiterInner {
                size: Cell::new(0),
                is_disposed: Cell::new(false),
                running_promises: Cell::new(0),
                max_degree_of_paralellism,
                outstanding_promises: RefCell::new(VecDeque::new()),
                on_drained: Emitter::new(),
            }),
        }
    }

    /// Resolve quando todo o trabalho termina (`onDrained`) ou quando não há nada a fazer.
    pub fn when_idle(&self) -> Promise<()> {
        if self.size() > 0 { self.on_drained().to_promise(None).promise } else { Promise::resolved(()) }
    }

    pub fn on_drained(&self) -> Event<()> {
        self.inner.on_drained.event()
    }

    pub fn size(&self) -> usize {
        self.inner.size.get()
    }

    pub fn queue(&self, factory: impl FnOnce() -> Promise<T> + 'static) -> Promise<T> {
        if self.inner.is_disposed.get() {
            errors::throw(Error::new("Object has been disposed"));
        }
        self.inner.size.set(self.inner.size.get() + 1);

        let inner = self.inner.clone();
        Promise::with_executor(move |resolver| {
            inner.outstanding_promises.borrow_mut().push_back(LimitedTaskFactory { factory: Box::new(factory), resolver });
            LimiterInner::consume(&inner);
        })
    }

    pub fn clear(&self) {
        if self.inner.is_disposed.get() {
            errors::throw(Error::new("Object has been disposed"));
        }
        self.inner.outstanding_promises.borrow_mut().clear();
        self.inner.size.set(self.inner.running_promises.get());
    }
}

impl<T: Clone + 'static> ILimiter<T> for Limiter<T> {
    fn size(&self) -> usize {
        Limiter::size(self)
    }

    fn queue(&self, factory: Box<dyn FnOnce() -> Promise<T>>) -> Promise<T> {
        Limiter::queue(self, factory)
    }

    fn clear(&self) {
        Limiter::clear(self);
    }
}

impl<T: Clone + 'static> IDisposable for Limiter<T> {
    fn dispose(&self) {
        self.inner.is_disposed.set(true);
        self.inner.outstanding_promises.borrow_mut().clear(); // stop further processing
        self.inner.size.set(0);
        self.inner.on_drained.dispose();
    }
}

/// `Queue`: um [`Limiter`] de grau 1, uma promessa por vez.
pub struct Queue<T: Clone + 'static>(Limiter<T>);

impl<T: Clone + 'static> Default for Queue<T> {
    fn default() -> Self {
        Self::new()
    }
}

impl<T: Clone + 'static> Queue<T> {
    pub fn new() -> Self {
        Self(Limiter::new(1))
    }
}

impl<T: Clone + 'static> Clone for Queue<T> {
    fn clone(&self) -> Self {
        Self(self.0.clone())
    }
}

impl<T: Clone + 'static> Deref for Queue<T> {
    type Target = Limiter<T>;

    fn deref(&self) -> &Limiter<T> {
        &self.0
    }
}

impl<T: Clone + 'static> IDisposable for Queue<T> {
    fn dispose(&self) {
        self.0.dispose();
    }
}

struct RunningTask {
    task_id: usize,
    cancel: Rc<dyn Fn()>,
    promise: Promise<()>,
}

struct QueuedTask {
    promise: Promise<()>,
    resolver: Resolver<()>,
    run: Option<Box<dyn FnOnce() -> Promise<()>>>,
}

#[derive(Default)]
struct SequentializerInner {
    running: RefCell<Option<RunningTask>>,
    queued: RefCell<Option<QueuedTask>>,
}

/// `TaskSequentializer`: uma tarefa roda por vez e só a última pedida durante a execução fica na fila.
#[derive(Clone, Default)]
pub struct TaskSequentializer {
    inner: Rc<SequentializerInner>,
}

impl TaskSequentializer {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn is_running(&self) -> bool {
        self.inner.running.borrow().is_some()
    }

    /// `isRunning(taskId)`.
    pub fn is_running_task(&self, task_id: usize) -> bool {
        self.inner.running.borrow().as_ref().is_some_and(|running| running.task_id == task_id)
    }

    pub fn running(&self) -> Option<Promise<()>> {
        self.inner.running.borrow().as_ref().map(|running| running.promise.clone())
    }

    pub fn cancel_running(&self) {
        let cancel = self.inner.running.borrow().as_ref().map(|running| running.cancel.clone());
        if let Some(cancel) = cancel {
            cancel();
        }
    }

    pub fn run(&self, task_id: usize, promise: Promise<()>, on_cancel: Option<Rc<dyn Fn()>>) -> Promise<()> {
        *self.inner.running.borrow_mut() = Some(RunningTask {
            task_id,
            cancel: Rc::new(move || {
                if let Some(on_cancel) = &on_cancel {
                    on_cancel();
                }
            }),
            promise: promise.clone(),
        });

        let this = self.clone();
        promise.then_result(move |_| {
            this.done_running(task_id);
            Ok(())
        });

        promise
    }

    fn done_running(&self, task_id: usize) {
        let is_current = self.inner.running.borrow().as_ref().is_some_and(|running| running.task_id == task_id);
        if is_current {
            // only set running to done if the promise finished that is associated with that taskId
            *self.inner.running.borrow_mut() = None;

            // schedule the queued task now that we are free if we have any
            self.run_queued();
        }
    }

    fn run_queued(&self) {
        let queued = self.inner.queued.borrow_mut().take();
        if let Some(mut queued) = queued {
            // Run queued task and complete on the associated promise
            let run = queued.run.take().expect("a tarefa enfileirada");
            run().pipe_to(&queued.resolver);
        }
    }

    /// Enfileira `run` para depois da tarefa em execução. A promessa devolvida por `run` DEVE chamar
    /// [`TaskSequentializer::run`]; senão `is_running` dá `false` com a tarefa rodando e várias rodam em paralelo.
    pub fn queue(&self, run: impl FnOnce() -> Promise<()> + 'static) -> Promise<()> {
        let mut queued = self.inner.queued.borrow_mut();
        match &mut *queued {
            // we have a previous queued task, just overwrite it
            Some(queued) => {
                queued.run = Some(Box::new(run));
                queued.promise.clone()
            }
            // this is our first queued task, so we create associated promise with it
            // so that we can return a promise that completes when the task has completed.
            None => {
                let (promise, resolver) = Promise::new();
                *queued = Some(QueuedTask { promise: promise.clone(), resolver, run: Some(Box::new(run)) });
                promise
            }
        }
    }

    pub fn has_queued(&self) -> bool {
        self.inner.queued.borrow().is_some()
    }

    pub fn join(&self) -> Promise<()> {
        let queued = self.inner.queued.borrow().as_ref().map(|queued| queued.promise.clone());
        let promise = queued.or_else(|| self.running());
        adopt(promise.map_or(MaybePromise::Value(()), MaybePromise::Promise))
    }
}

/// `LimitedQueue`: como a [`Queue`], mas só há uma tarefa a agendar; chamar `queue` com uma tarefa em
/// execução troca a que está esperando, e a promessa devolvida pode ser a da tarefa que de fato rodou.
pub struct LimitedQueue {
    sequentializer: TaskSequentializer,
    tasks: Rc<Cell<usize>>,
}

impl Default for LimitedQueue {
    fn default() -> Self {
        Self::new()
    }
}

impl LimitedQueue {
    pub fn new() -> Self {
        Self { sequentializer: TaskSequentializer::new(), tasks: Rc::new(Cell::new(0)) }
    }

    pub fn queue(&self, factory: impl FnOnce() -> Promise<()> + 'static) -> Promise<()> {
        let next_task_id = |tasks: &Cell<usize>| {
            let id = tasks.get();
            tasks.set(id + 1);
            id
        };
        if !self.sequentializer.is_running() {
            let id = next_task_id(&self.tasks);
            return self.sequentializer.run(id, factory(), None);
        }

        let (sequentializer, tasks) = (self.sequentializer.clone(), self.tasks.clone());
        self.sequentializer.queue(move || {
            let id = next_task_id(&tasks);
            sequentializer.run(id, factory(), None)
        })
    }
}

struct ResourceQueueInner {
    queues: RefCell<OrderedMap<JsString, Queue<()>>>,
    drainers: RefCell<Vec<Rc<DeferredPromise<()>>>>,
    drain_listeners: RefCell<Option<Rc<DisposableMap<usize, dyn IDisposable>>>>,
    drain_listener_count: Cell<usize>,
}

impl ResourceQueueInner {
    fn is_drained(&self) -> bool {
        self.queues.borrow().values().all(|queue| queue.size() == 0)
    }

    fn release_drainers(&self) {
        let drainers = std::mem::take(&mut *self.drainers.borrow_mut());
        for drainer in drainers {
            drainer.complete(());
        }
    }

    fn on_did_queue_drain(&self) {
        if !self.is_drained() {
            return; // not done yet
        }
        self.release_drainers();
    }
}

/// `ResourceQueue`: uma fila por recurso, descartada quando esvazia.
pub struct ResourceQueue {
    inner: Rc<ResourceQueueInner>,
    ext_uri: ExtUri,
}

impl ResourceQueue {
    /// `ext_uri` é o `defaultExtUri` do original, usado por `queue_for` e `queue_size`.
    pub fn new(ext_uri: ExtUri) -> Self {
        Self {
            inner: Rc::new(ResourceQueueInner {
                queues: RefCell::new(OrderedMap::new()),
                drainers: RefCell::new(Vec::new()),
                drain_listeners: RefCell::new(None),
                drain_listener_count: Cell::new(0),
            }),
            ext_uri,
        }
    }

    pub fn when_drained(&self) -> Promise<()> {
        let inner = self.inner.clone();
        async_fn_maybe(async move {
            if inner.is_drained() {
                return Ok(MaybePromise::Value(()));
            }

            let promise = Rc::new(DeferredPromise::<()>::new());
            inner.drainers.borrow_mut().push(promise.clone());

            Ok(MaybePromise::Promise(promise.promise.clone()))
        })
    }

    pub fn queue_size(&self, resource: &URI) -> usize {
        self.queue_size_with(resource, &self.ext_uri)
    }

    pub fn queue_size_with(&self, resource: &URI, ext_uri: &ExtUri) -> usize {
        let key = ext_uri.get_comparison_key(resource, false);
        self.inner.queues.borrow().get(&key).map_or(0, |queue| queue.size())
    }

    pub fn queue_for(&self, resource: &URI, factory: impl FnOnce() -> Promise<()> + 'static) -> Promise<()> {
        self.queue_for_with(resource, factory, &self.ext_uri)
    }

    pub fn queue_for_with(&self, resource: &URI, factory: impl FnOnce() -> Promise<()> + 'static, ext_uri: &ExtUri) -> Promise<()> {
        let key = ext_uri.get_comparison_key(resource, false);

        let existing = self.inner.queues.borrow().get(&key).cloned();
        let queue = existing.unwrap_or_else(|| {
            let queue = Queue::<()>::new();
            let drain_listener_id = self.inner.drain_listener_count.get();
            self.inner.drain_listener_count.set(drain_listener_id + 1);
            let drain_listener = queue.on_drained().once().listen({
                let (inner, queue, key) = (self.inner.clone(), queue.clone(), key.clone());
                move |_| {
                    queue.dispose();
                    inner.queues.borrow_mut().remove(&key);
                    inner.on_did_queue_drain();

                    let listeners = inner.drain_listeners.borrow().clone();
                    if let Some(listeners) = listeners {
                        listeners.delete_and_dispose(&drain_listener_id);
                        if listeners.is_empty() {
                            listeners.dispose();
                            *inner.drain_listeners.borrow_mut() = None;
                        }
                    }
                }
            });

            let listeners = self.inner.drain_listeners.borrow_mut().get_or_insert_with(DisposableMap::new).clone();
            listeners.set(drain_listener_id, drain_listener, false);

            self.inner.queues.borrow_mut().insert(key, queue.clone());
            queue
        });

        queue.queue(factory)
    }
}

impl IDisposable for ResourceQueue {
    fn dispose(&self) {
        let queues: Vec<Queue<()>> = self.inner.queues.borrow().values().cloned().collect();
        for queue in queues {
            queue.dispose();
        }
        self.inner.queues.borrow_mut().clear();

        // Even though we might still have pending tasks queued, after the queues have been disposed, we can
        // no longer track them, so we release drainers to prevent hanging promises when the resource queue
        // is being disposed.
        self.inner.release_drainers();

        let listeners = self.inner.drain_listeners.borrow().clone();
        if let Some(listeners) = listeners {
            listeners.dispose();
        }
    }
}

struct PendingTask {
    run: Box<dyn FnOnce() -> Pin<Box<dyn Future<Output = ()>>>>,
    on_clear: Box<dyn FnOnce()>,
}

#[derive(Default)]
struct TaskQueueInner {
    running_task: Cell<bool>,
    pending_tasks: RefCell<VecDeque<PendingTask>>,
}

impl TaskQueueInner {
    fn run_if_not_running(this: &Rc<Self>) {
        if !this.running_task.get() {
            Self::process_queue(this);
        }
    }

    /// `private async _processQueue()`: o `finally` rearma a fila, então cada tarefa começa na rodada em
    /// que a anterior termina.
    fn process_queue(this: &Rc<Self>) {
        let this = this.clone();
        runtime::spawn_local(async move {
            let next = this.pending_tasks.borrow_mut().pop_front();
            let Some(next) = next else { return };

            if this.running_task.get() {
                errors::throw(Error::bug_indicating(None));
            }

            this.running_task.set(true);

            (next.run)().await;

            this.running_task.set(false);
            Self::process_queue(&this);
        });
    }
}

/// `TaskQueue`: processa as tarefas na ordem em que foram agendadas.
#[derive(Default)]
pub struct TaskQueue {
    inner: Rc<TaskQueueInner>,
}

impl TaskQueue {
    pub fn new() -> Self {
        Self::default()
    }

    fn enqueue<T: Clone + 'static, R: Clone + 'static>(
        &self,
        task: impl FnOnce() -> MaybePromise<T> + 'static,
        wrap: impl FnOnce(T) -> R + 'static,
        on_clear: impl FnOnce(&DeferredPromise<R>) + 'static,
    ) -> Promise<R> {
        let deferred = Rc::new(DeferredPromise::<R>::new());
        let promise = deferred.promise.clone();
        let on_done = deferred.clone();
        self.inner.pending_tasks.borrow_mut().push_back(PendingTask {
            run: Box::new(move || {
                Box::pin(async move {
                    let outcome = match errors::catch(task) {
                        Ok(value) => await_maybe(value).await,
                        Err(error) => Err(error),
                    };
                    match outcome {
                        Ok(result) => on_done.complete(wrap(result)),
                        Err(error) => on_done.error(error),
                    };
                })
            }),
            on_clear: Box::new(move || on_clear(&deferred)),
        });
        TaskQueueInner::run_if_not_running(&self.inner);
        promise
    }

    /// Espera as tarefas em curso e pendentes terminarem, depois roda e espera a tarefa. Se ela for pulada
    /// por `clear_pending`, a promessa rejeita com [`Error::cancellation`].
    pub fn schedule<T: Clone + 'static>(&self, task: impl FnOnce() -> MaybePromise<T> + 'static) -> Promise<T> {
        self.enqueue(task, |value| value, |deferred| {
            deferred.error(Error::cancellation());
        })
    }

    /// Como `schedule`, mas se a tarefa for pulada por `clear_pending` a promessa resolve com `None`.
    pub fn schedule_skip_if_cleared<T: Clone + 'static>(&self, task: impl FnOnce() -> MaybePromise<T> + 'static) -> Promise<Option<T>> {
        self.enqueue(task, Some, |deferred| {
            deferred.complete(None);
        })
    }

    /// Limpa as tarefas pendentes; a que está rodando continua.
    pub fn clear_pending(&self) {
        let tasks = std::mem::take(&mut *self.inner.pending_tasks.borrow_mut());
        for task in tasks {
            (task.on_clear)();
        }
    }
}

// endregion

// region: timers e agendadores

/// `TimeoutTimer`: um `setTimeout` que se cancela e se rearma.
pub struct TimeoutTimer {
    token: Rc<Cell<Option<TimerId>>>,
    is_disposed: Cell<bool>,
}

impl TimeoutTimer {
    pub fn new() -> Rc<Self> {
        Rc::new(Self { token: Rc::new(Cell::new(None)), is_disposed: Cell::new(false) })
    }

    /// `new TimeoutTimer(runner, timeout)`.
    pub fn with_runner(runner: impl FnOnce() + 'static, timeout_ms: u64) -> Rc<Self> {
        let timer = Self::new();
        timer.set_if_not_set(runner, timeout_ms);
        timer
    }

    pub fn cancel(&self) {
        if let Some(token) = self.token.take() {
            runtime::clear_timeout(token);
        }
    }

    fn arm(&self, runner: impl FnOnce() + 'static, timeout_ms: u64) {
        let token = self.token.clone();
        self.token.set(Some(runtime::set_timeout(
            move || {
                token.set(None);
                runner();
            },
            timeout_ms,
        )));
    }

    pub fn cancel_and_set(&self, runner: impl FnOnce() + 'static, timeout_ms: u64) {
        if self.is_disposed.get() {
            errors::throw(Error::bug_indicating(Some("Calling 'cancelAndSet' on a disposed TimeoutTimer")));
        }

        self.cancel();
        self.arm(runner, timeout_ms);
    }

    pub fn set_if_not_set(&self, runner: impl FnOnce() + 'static, timeout_ms: u64) {
        if self.is_disposed.get() {
            errors::throw(Error::bug_indicating(Some("Calling 'setIfNotSet' on a disposed TimeoutTimer")));
        }

        if self.token.get().is_some() {
            // timer is already set
            return;
        }
        self.arm(runner, timeout_ms);
    }
}

impl IDisposable for TimeoutTimer {
    fn dispose(&self) {
        self.cancel();
        self.is_disposed.set(true);
    }
}

struct IntervalState {
    handler: Rc<dyn Fn()>,
    interval_ms: u64,
    timer: Cell<Option<TimerId>>,
    cleared: Cell<bool>,
}

/// O handle de um `setInterval`; `clear_interval` é o `clearInterval`.
#[derive(Clone)]
pub struct IntervalHandle(Rc<IntervalState>);

fn arm_interval(state: &Rc<IntervalState>) {
    let next = state.clone();
    state.timer.set(Some(runtime::set_timeout(
        move || {
            (next.handler)();
            // o intervalo só rearma depois do handler, e se o handler não o limpou
            if !next.cleared.get() {
                arm_interval(&next);
            }
        },
        state.interval_ms,
    )));
}

/// `setInterval(handler, interval)`, sobre o `set_timeout` do loop (como no Node, `0` vale `1`).
pub fn set_interval(handler: impl Fn() + 'static, interval_ms: u64) -> IntervalHandle {
    let state = Rc::new(IntervalState { handler: Rc::new(handler), interval_ms: interval_ms.max(1), timer: Cell::new(None), cleared: Cell::new(false) });
    arm_interval(&state);
    IntervalHandle(state)
}

/// `clearInterval(handle)`.
pub fn clear_interval(handle: &IntervalHandle) {
    handle.0.cleared.set(true);
    if let Some(timer) = handle.0.timer.take() {
        runtime::clear_timeout(timer);
    }
}

/// `IntervalTimer`: um `setInterval` que se cancela e se rearma.
pub struct IntervalTimer {
    disposable: Rc<RefCell<Option<DisposableRef>>>,
    is_disposed: Cell<bool>,
}

impl IntervalTimer {
    pub fn new() -> Rc<Self> {
        Rc::new(Self { disposable: Rc::default(), is_disposed: Cell::new(false) })
    }

    pub fn cancel(&self) {
        dispose_slot(&self.disposable);
    }

    pub fn cancel_and_set(&self, runner: impl Fn() + 'static, interval_ms: u64) {
        if self.is_disposed.get() {
            errors::throw(Error::bug_indicating(Some("Calling 'cancelAndSet' on a disposed IntervalTimer")));
        }

        self.cancel();
        let handle = set_interval(runner, interval_ms);

        let slot = self.disposable.clone();
        *self.disposable.borrow_mut() = Some(to_disposable(move || {
            clear_interval(&handle);
            *slot.borrow_mut() = None;
        }));
    }
}

impl IDisposable for IntervalTimer {
    fn dispose(&self) {
        self.cancel();
        self.is_disposed.set(true);
    }
}

/// `RunOnceScheduler`: agenda um `runner` para daqui a `delay`; agendar de novo cancela o anterior.
/// O `runner` de um agendador, que o `dispose()` solta.
type SharedRunner = Rc<RefCell<Option<Rc<dyn Fn()>>>>;

pub struct RunOnceScheduler {
    runner: SharedRunner,
    timeout_token: Rc<Cell<Option<TimerId>>>,
    timeout: Cell<u64>,
}

impl RunOnceScheduler {
    pub fn new(runner: impl Fn() + 'static, delay: u64) -> Rc<Self> {
        Rc::new(Self { runner: Rc::new(RefCell::new(Some(Rc::new(runner)))), timeout_token: Rc::new(Cell::new(None)), timeout: Cell::new(delay) })
    }

    /// Cancela o agendamento atual, se houver.
    pub fn cancel(&self) {
        if let Some(token) = self.timeout_token.take() {
            runtime::clear_timeout(token);
        }
    }

    /// Cancela o agendamento anterior e agenda um novo; `delay` ausente usa o atraso do agendador.
    pub fn schedule(&self, delay: Option<u64>) {
        self.cancel();
        let (runner, token) = (self.runner.clone(), self.timeout_token.clone());
        self.timeout_token.set(Some(runtime::set_timeout(
            move || {
                token.set(None);
                let runner = runner.borrow().clone();
                if let Some(runner) = runner {
                    runner();
                }
            },
            delay.unwrap_or(self.timeout.get()),
        )));
    }

    pub fn delay(&self) -> u64 {
        self.timeout.get()
    }

    pub fn set_delay(&self, value: u64) {
        self.timeout.set(value);
    }

    pub fn is_scheduled(&self) -> bool {
        self.timeout_token.get().is_some()
    }

    pub fn flush(&self) {
        if self.is_scheduled() {
            self.cancel();
            let runner = self.runner.borrow().clone();
            if let Some(runner) = runner {
                runner();
            }
        }
    }
}

impl IDisposable for RunOnceScheduler {
    fn dispose(&self) {
        self.cancel();
        *self.runner.borrow_mut() = None;
    }
}

fn warn_process_time_resolution(delay_ms: u64) {
    if !delay_ms.is_multiple_of(1000) {
        eprintln!("ProcessTimeRunOnceScheduler resolution is 1s, {delay_ms}ms is not a multiple of 1000ms.");
    }
}

/// Como o [`RunOnceScheduler`], mas não conta o tempo em que o computador dormiu (só tem resolução de 1 s).
pub struct ProcessTimeRunOnceScheduler {
    runner: SharedRunner,
    timeout: Cell<u64>,
    counter: Rc<Cell<u64>>,
    interval_token: Rc<RefCell<Option<IntervalHandle>>>,
}

impl ProcessTimeRunOnceScheduler {
    pub fn new(runner: impl Fn() + 'static, delay: u64) -> Rc<Self> {
        warn_process_time_resolution(delay);
        Rc::new(Self {
            runner: Rc::new(RefCell::new(Some(Rc::new(runner)))),
            timeout: Cell::new(delay),
            counter: Rc::new(Cell::new(0)),
            interval_token: Rc::default(),
        })
    }

    pub fn cancel(&self) {
        let token = self.interval_token.borrow_mut().take();
        if let Some(token) = token {
            clear_interval(&token);
        }
    }

    /// Cancela o agendamento anterior e agenda um novo; `delay` ausente usa o atraso do agendador.
    pub fn schedule(&self, delay: Option<u64>) {
        let delay = delay.unwrap_or(self.timeout.get());
        warn_process_time_resolution(delay);
        self.cancel();
        self.counter.set(delay.div_ceil(1000));
        let (counter, runner, interval_token) = (self.counter.clone(), self.runner.clone(), self.interval_token.clone());
        let handle = set_interval(
            move || {
                counter.set(counter.get().saturating_sub(1));
                if counter.get() > 0 {
                    // still need to wait
                    return;
                }

                // time elapsed
                let token = interval_token.borrow_mut().take();
                if let Some(token) = token {
                    clear_interval(&token);
                }
                let runner = runner.borrow().clone();
                if let Some(runner) = runner {
                    runner();
                }
            },
            1000,
        );
        *self.interval_token.borrow_mut() = Some(handle);
    }

    pub fn is_scheduled(&self) -> bool {
        self.interval_token.borrow().is_some()
    }
}

impl IDisposable for ProcessTimeRunOnceScheduler {
    fn dispose(&self) {
        self.cancel();
        *self.runner.borrow_mut() = None;
    }
}

/// `RunOnceWorker`: junta as unidades de trabalho e as entrega de uma vez ao `runner`.
pub struct RunOnceWorker<T: 'static> {
    scheduler: Rc<RunOnceScheduler>,
    units: Rc<RefCell<Vec<T>>>,
}

impl<T: 'static> RunOnceWorker<T> {
    pub fn new(runner: impl Fn(Vec<T>) + 'static, timeout: u64) -> Rc<Self> {
        let units: Rc<RefCell<Vec<T>>> = Rc::new(RefCell::new(Vec::new()));
        let pending = units.clone();
        let scheduler = RunOnceScheduler::new(move || runner(std::mem::take(&mut *pending.borrow_mut())), timeout);
        Rc::new(Self { scheduler, units })
    }

    pub fn work(&self, unit: T) {
        self.units.borrow_mut().push(unit);

        if !self.scheduler.is_scheduled() {
            self.scheduler.schedule(None);
        }
    }
}

impl<T: 'static> Deref for RunOnceWorker<T> {
    type Target = RunOnceScheduler;

    fn deref(&self) -> &RunOnceScheduler {
        &self.scheduler
    }
}

impl<T: 'static> IDisposable for RunOnceWorker<T> {
    fn dispose(&self) {
        self.units.borrow_mut().clear();

        self.scheduler.dispose();
    }
}

/// `IThrottledWorkerOptions`.
#[derive(Clone, Copy, Debug)]
pub struct ThrottledWorkerOptions {
    /// O máximo de unidades que o worker entrega ao handler de uma vez.
    pub max_work_chunk_size: usize,
    /// O máximo de unidades que o worker guarda em memória para processar.
    pub max_buffered_work: Option<usize>,
    /// O atraso antes da próxima rodada de pedaços quando o tamanho passa do limite.
    pub throttle_delay: u64,
    /// Garante que duas chamadas distintas de `work()` não rodam sem o atraso entre elas; senão, se o
    /// worker não está em espera, o trabalho roda na hora.
    pub wait_throttle_delay_between_work_units: bool,
}

/// `ThrottledWorker`: aceita unidades de trabalho, entrega no máximo `max_work_chunk_size` por vez e, depois
/// de cada pedaço, descansa `throttle_delay`.
pub struct ThrottledWorker<T: Clone + 'static> {
    base: Rc<Disposable>,
    me: Weak<Self>,
    pending_work: RefCell<Vec<T>>,
    throttler: Rc<MutableDisposable<RunOnceScheduler>>,
    disposed: Cell<bool>,
    last_execution_time: Cell<u64>,
    options: ThrottledWorkerOptions,
    handler: Rc<dyn Fn(Vec<T>)>,
}

impl<T: Clone + 'static> ThrottledWorker<T> {
    pub fn new(options: ThrottledWorkerOptions, handler: impl Fn(Vec<T>) + 'static) -> Rc<Self> {
        let base = Disposable::new();
        let throttler = MutableDisposable::<RunOnceScheduler>::new();
        base.register(throttler.clone());
        Rc::new_cyclic(|me| Self {
            base,
            me: me.clone(),
            pending_work: RefCell::new(Vec::new()),
            throttler,
            disposed: Cell::new(false),
            last_execution_time: Cell::new(0),
            options,
            handler: Rc::new(handler),
        })
    }

    /// O número de unidades que ainda esperam processamento.
    pub fn pending(&self) -> usize {
        self.pending_work.borrow().len()
    }

    /// Adiciona unidades ao trabalho; `pending` diz quantas ainda esperam. Devolve se o trabalho foi aceito:
    /// descartado, ou com mais pendência que `max_buffered_work`, o worker não aceita.
    pub fn work(&self, units: &[T]) -> bool {
        if self.disposed.get() {
            return false; // work not accepted: disposed
        }

        // Check for reaching maximum of pending work
        if let Some(max_buffered_work) = self.options.max_buffered_work {
            // Throttled: simple check if pending + units exceeds max pending
            if self.throttler.value().is_some() {
                if self.pending() + units.len() > max_buffered_work {
                    return false; // work not accepted: too much pending work
                }
            }
            // Unthrottled: same as throttled, but account for max chunk getting worked on directly without being pending
            else if (self.pending() + units.len()).saturating_sub(self.options.max_work_chunk_size) > max_buffered_work {
                return false; // work not accepted: too much pending work
            }
        }

        // Add to pending units first
        self.pending_work.borrow_mut().extend_from_slice(units);

        let time_since_last_execution = runtime::date_now().saturating_sub(self.last_execution_time.get());
        let throttling = self.throttler.value().is_some();

        if !throttling && (!self.options.wait_throttle_delay_between_work_units || time_since_last_execution >= self.options.throttle_delay) {
            // Work directly if we are not throttling and we are not enforced to throttle between `work()` calls.
            self.do_work();
        } else if !throttling && self.options.wait_throttle_delay_between_work_units {
            // Otherwise, schedule the throttler to work.
            self.schedule_throttler(self.options.throttle_delay.saturating_sub(time_since_last_execution));
        } else {
            // Otherwise, our work will be picked up by the running throttler
        }

        true // work accepted
    }

    fn do_work(&self) {
        self.last_execution_time.set(runtime::date_now());

        // Extract chunk to handle and handle it
        let chunk: Vec<T> = {
            let mut pending = self.pending_work.borrow_mut();
            let end = self.options.max_work_chunk_size.min(pending.len());
            pending.drain(..end).collect()
        };
        (self.handler)(chunk);

        // If we have remaining work, schedule it after a delay
        if !self.pending_work.borrow().is_empty() {
            self.schedule_throttler(self.options.throttle_delay);
        }
    }

    fn schedule_throttler(&self, delay: u64) {
        let me = self.me.clone();
        let scheduler = RunOnceScheduler::new(
            move || {
                if let Some(this) = me.upgrade() {
                    this.throttler.clear();

                    this.do_work();
                }
            },
            delay,
        );
        self.throttler.set(Some(scheduler.clone()));
        scheduler.schedule(None);
    }
}

impl<T: Clone + 'static> IDisposable for ThrottledWorker<T> {
    fn dispose(&self) {
        self.base.dispose();

        self.pending_work.borrow_mut().clear();
        self.disposed.set(true);
    }
}

// endregion

// region: run on idle

/// `IdleDeadline`.
#[derive(Clone, Copy, Debug)]
pub struct IdleDeadline {
    pub did_timeout: bool,
    end: u64,
}

impl IdleDeadline {
    /// Um prazo que termina em `end` (milissegundos do `Date.now()`); para quem implementa um agendador falso.
    pub fn new(did_timeout: bool, end: u64) -> Self {
        Self { did_timeout, end }
    }

    pub fn time_remaining(&self) -> u64 {
        self.end.saturating_sub(runtime::date_now())
    }
}

/// O que o original guarda em `_runWhenIdle`/`runWhenGlobalIdle`: agenda o callback para quando o motor
/// estiver ocioso e devolve o disposable que o cancela.
pub type RunWhenIdle = Rc<dyn Fn(Box<dyn FnOnce(IdleDeadline)>, Option<u64>) -> DisposableRef>;

/// O caminho sem `requestIdleCallback` do original: um `setTimeout0` que entrega um prazo de 15 ms (um
/// quadro a 64 fps).
fn run_when_idle_with_timeout0(runner: Box<dyn FnOnce(IdleDeadline)>, _timeout: Option<u64>) -> DisposableRef {
    let disposed = Rc::new(Cell::new(false));
    let flag = disposed.clone();
    runtime::set_timeout(
        move || {
            if flag.get() {
                return;
            }
            let end = runtime::date_now() + 15; // one frame at 64fps
            runner(IdleDeadline { did_timeout: true, end });
        },
        0,
    );
    // objeto literal `{ dispose() { ... } }` no original: não é rastreado
    crate::lifecycle::plain_disposable(move || disposed.set(true))
}

thread_local! {
    static RUN_WHEN_IDLE: RefCell<RunWhenIdle> = RefCell::new(Rc::new(run_when_idle_with_timeout0));
}

fn current_run_when_idle() -> RunWhenIdle {
    RUN_WHEN_IDLE.with(|f| f.borrow().clone())
}

/// `runWhenGlobalIdle(callback, timeout)`: roda o callback na próxima vez em que o motor estiver ocioso;
/// o `dispose()` devolvido cancela.
pub fn run_when_global_idle(callback: impl FnOnce(IdleDeadline) + 'static, timeout: Option<u64>) -> DisposableRef {
    current_run_when_idle()(Box::new(callback), timeout)
}

/// `installFakeRunWhenIdle(fakeImpl)`: troca a implementação até o `dispose()` do que devolve.
pub fn install_fake_run_when_idle(fake_impl: RunWhenIdle) -> DisposableRef {
    let original = RUN_WHEN_IDLE.with(|f| f.replace(fake_impl));
    to_disposable(move || {
        RUN_WHEN_IDLE.with(|f| *f.borrow_mut() = original);
    })
}

struct IdleValueState<T> {
    did_run: Cell<bool>,
    value: RefCell<Option<T>>,
    error: RefCell<Option<Error>>,
    executor: RefCell<Option<Box<dyn FnOnce() -> T>>>,
}

impl<T> IdleValueState<T> {
    fn run(&self) {
        let executor = self.executor.borrow_mut().take();
        if let Some(executor) = executor {
            match errors::catch(executor) {
                Ok(value) => *self.value.borrow_mut() = Some(value),
                Err(error) => *self.error.borrow_mut() = Some(error),
            }
        }
        self.did_run.set(true);
    }
}

/// `AbstractIdleValue`: um valor calculado quando o motor fica ocioso, ou na primeira leitura. O
/// agendador (o `targetWindow` do original) é um parâmetro.
pub struct AbstractIdleValue<T: Clone + 'static> {
    state: Rc<IdleValueState<T>>,
    handle: DisposableRef,
}

impl<T: Clone + 'static> AbstractIdleValue<T> {
    pub fn new(run_when_idle: &RunWhenIdle, executor: impl FnOnce() -> T + 'static) -> Self {
        let state = Rc::new(IdleValueState {
            did_run: Cell::new(false),
            value: RefCell::new(None),
            error: RefCell::new(None),
            executor: RefCell::new(Some(Box::new(executor))),
        });
        let on_idle = state.clone();
        let handle = run_when_idle(Box::new(move |_| on_idle.run()), None);
        Self { state, handle }
    }

    pub fn dispose(&self) {
        self.handle.dispose();
    }

    /// `get value()`: força o cálculo se ainda não houve e relança o erro do executor.
    pub fn value(&self) -> T {
        if !self.state.did_run.get() {
            self.handle.dispose();
            self.state.run();
        }
        if let Some(error) = self.state.error.borrow().clone() {
            errors::throw(error);
        }
        self.state.value.borrow().clone().expect("valor calculado")
    }

    pub fn is_initialized(&self) -> bool {
        self.state.did_run.get()
    }
}

/// `GlobalIdleValue`: um [`AbstractIdleValue`] que usa o `runWhenGlobalIdle` atual.
pub struct GlobalIdleValue<T: Clone + 'static>(AbstractIdleValue<T>);

impl<T: Clone + 'static> GlobalIdleValue<T> {
    pub fn new(executor: impl FnOnce() -> T + 'static) -> Self {
        Self(AbstractIdleValue::new(&current_run_when_idle(), executor))
    }
}

impl<T: Clone + 'static> Deref for GlobalIdleValue<T> {
    type Target = AbstractIdleValue<T>;

    fn deref(&self) -> &AbstractIdleValue<T> {
        &self.0
    }
}

// endregion

/// `retry(task, delay, retries)`: tenta a tarefa até `retries` vezes, esperando `delay` entre as tentativas,
/// e rejeita com o último erro.
pub fn retry<T: Clone + 'static>(mut task: impl FnMut() -> Promise<T> + 'static, delay: u64, retries: usize) -> Promise<T> {
    async_fn(async move {
        let mut last_error: Option<Error> = None;

        for _ in 0..retries {
            match task().await {
                Ok(value) => return Ok(value),
                Err(error) => {
                    last_error = Some(error);

                    timeout(delay).await?;
                }
            }
        }

        // `throw undefined` quando não houve tentativa
        Err(last_error.unwrap_or_else(|| Error::new("undefined")))
    })
}

/// `IntervalCounter`: conta as chamadas de `increment()` num intervalo, para limitar uma tarefa frequente
/// quando passa de um limiar.
pub struct IntervalCounter {
    last_increment_time: Cell<u64>,
    value: Cell<usize>,
    interval: u64,
    now_fn: Box<dyn Fn() -> u64>,
}

impl IntervalCounter {
    /// `new IntervalCounter(interval)`, com `Date.now()` como relógio.
    pub fn new(interval: u64) -> Self {
        Self::with_now_fn(interval, runtime::date_now)
    }

    pub fn with_now_fn(interval: u64, now_fn: impl Fn() -> u64 + 'static) -> Self {
        Self { last_increment_time: Cell::new(0), value: Cell::new(0), interval, now_fn: Box::new(now_fn) }
    }

    pub fn increment(&self) -> usize {
        let now = (self.now_fn)();

        // We are outside of the range of `interval` and as such start counting from 0 and remember the time
        if now.saturating_sub(self.last_increment_time.get()) > self.interval {
            self.last_increment_time.set(now);
            self.value.set(0);
        }

        self.value.set(self.value.get() + 1);

        self.value.get()
    }
}

/// `namespace Promises`.
pub mod promises {
    use super::*;

    /// `Promises.settled(promises)`: como `Promise.all`, mas espera todas assentarem (cumprir ou rejeitar) e
    /// rejeita só com o primeiro erro, se houver.
    pub fn settled<T: Clone + 'static>(promises: Vec<Promise<T>>) -> Promise<Vec<T>> {
        async_fn(async move {
            let first_error: Rc<RefCell<Option<Error>>> = Rc::default();

            let guarded: Vec<Promise<Option<T>>> = promises
                .iter()
                .map(|promise| {
                    let first_error = first_error.clone();
                    promise.then_result(move |result| match result {
                        Ok(value) => Ok(Some(value)),
                        Err(error) => {
                            first_error.borrow_mut().get_or_insert(error);

                            Ok(None) // do not rethrow so that other promises can settle
                        }
                    })
                })
                .collect();
            let result = Promise::all(guarded).await?;

            if let Some(error) = first_error.borrow().clone() {
                return Err(error);
            }

            Ok(result.into_iter().flatten().collect())
        })
    }

    /// `Promises.withAsyncBody(bodyFn)`: um `new Promise` com corpo assíncrono. Um erro do corpo rejeita a
    /// promessa, em vez de virar uma rejeição não tratada.
    pub fn with_async_body<T: Clone + 'static>(body_fn: impl FnOnce(Resolver<T>) -> Promise<()> + 'static) -> Promise<T> {
        let (promise, resolver) = Promise::new();
        runtime::spawn_local(async move {
            match errors::catch(|| body_fn(resolver.clone())) {
                Ok(body) => {
                    if let Err(error) = body.await {
                        resolver.reject(error);
                    }
                }
                Err(error) => resolver.reject(error),
            }
        });
        promise
    }
}

/// `StatefulPromise`: uma promessa que guarda o próprio resultado.
pub struct StatefulPromise<T: Clone + 'static> {
    value: Rc<RefCell<Option<T>>>,
    error: Rc<RefCell<Option<Error>>>,
    is_resolved: Rc<Cell<bool>>,
    pub promise: Promise<T>,
}

impl<T: Clone + 'static> StatefulPromise<T> {
    pub fn new(promise: &Promise<T>) -> Self {
        let value: Rc<RefCell<Option<T>>> = Rc::default();
        let error: Rc<RefCell<Option<Error>>> = Rc::default();
        let is_resolved = Rc::new(Cell::new(false));
        let derived = {
            let (value, error, is_resolved) = (value.clone(), error.clone(), is_resolved.clone());
            promise.then_result(move |result| {
                match &result {
                    Ok(v) => *value.borrow_mut() = Some(v.clone()),
                    Err(e) => *error.borrow_mut() = Some(e.clone()),
                }
                is_resolved.set(true);
                result
            })
        };
        Self { value, error, is_resolved, promise: derived }
    }

    pub fn value(&self) -> Option<T> {
        self.value.borrow().clone()
    }

    pub fn error(&self) -> Option<Error> {
        self.error.borrow().clone()
    }

    pub fn is_resolved(&self) -> bool {
        self.is_resolved.get()
    }

    /// O valor resolvido; lança se a promessa ainda não resolveu (ou se rejeitou, o erro dela).
    pub fn require_value(&self) -> T {
        if !self.is_resolved.get() {
            errors::throw(Error::bug_indicating(Some("Promise is not resolved yet")));
        }
        if let Some(error) = self.error.borrow().clone() {
            errors::throw(error);
        }
        self.value.borrow().clone().expect("valor resolvido")
    }
}

/// `LazyStatefulPromise`: calcula a promessa só na primeira vez que alguém a pede.
pub struct LazyStatefulPromise<T: Clone + 'static> {
    promise: Lazy<Rc<StatefulPromise<T>>>,
}

impl<T: Clone + 'static> LazyStatefulPromise<T> {
    pub fn new(compute: impl FnOnce() -> Promise<T> + 'static) -> Self {
        Self { promise: Lazy::new(move || Rc::new(StatefulPromise::new(&compute()))) }
    }

    /// O valor resolvido; lança se ainda não resolveu.
    pub fn require_value(&self) -> T {
        self.promise.value().require_value()
    }

    /// A promessa (e dispara o cálculo, se ainda não houve).
    pub fn get_promise(&self) -> Promise<T> {
        self.promise.value().promise.clone()
    }

    /// Lê o valor atual sem disparar o cálculo.
    pub fn current_value(&self) -> Option<T> {
        self.promise.raw_value().and_then(|promise| promise.value())
    }
}

/// `createTimeout(ms, cb)`.
pub fn create_timeout(ms: u64, callback: impl FnOnce() + 'static) -> DisposableRef {
    let timer = runtime::set_timeout(callback, ms);
    to_disposable(move || runtime::clear_timeout(timer))
}

// region: AsyncIterable

/// O `AsyncIterator<T>` do JavaScript: `next()` devolve `None` no fim (o `{ done: true }`); `return_` e
/// `throw_` são opcionais no JS, e `None` é o método ausente.
pub trait AsyncIterator<T: Clone + 'static> {
    fn next(&self) -> Promise<Option<T>>;

    fn return_(&self) -> Option<Promise<()>> {
        None
    }

    fn throw_(&self, _error: Error) -> Option<Promise<()>> {
        None
    }
}

/// O `AsyncIterable<T>` do JavaScript (`[Symbol.asyncIterator]()`).
pub trait AsyncIterable<T: Clone + 'static> {
    fn async_iterator(&self) -> Rc<dyn AsyncIterator<T>>;
}

/// O `for await (const item of iterable) { ... }`. O corpo devolve `Break` para sair antes do fim (o
/// `break`/`return` do JS) e `Err` para lançar; nos dois casos o iterador é fechado com `return()`, e a
/// espera por ele vem antes de seguir. Um erro de `next()` não fecha o iterador, como no JS.
pub async fn for_await<T: Clone + 'static>(iterable: &dyn AsyncIterable<T>, mut body: impl FnMut(T) -> Result<ControlFlow<()>, Error>) -> Result<(), Error> {
    let iterator = iterable.async_iterator();
    loop {
        let Some(item) = iterator.next().await? else {
            return Ok(());
        };
        match body(item) {
            Ok(ControlFlow::Continue(())) => {}
            Ok(ControlFlow::Break(())) => {
                if let Some(returned) = iterator.return_() {
                    returned.await?;
                }
                return Ok(());
            }
            Err(error) => {
                if let Some(returned) = iterator.return_() {
                    // o erro do corpo vence o de `return()`
                    let _ = returned.await;
                }
                return Err(error);
            }
        }
    }
}

/// `Iterable.asyncToArray(iterable)` (de `iterator.ts`): junta os itens de um `AsyncIterable` num vetor.
pub fn async_to_array<T: Clone + 'static>(iterable: Rc<dyn AsyncIterable<T>>) -> Promise<Vec<T>> {
    async_fn_maybe(async move {
        let mut result = Vec::new();
        for_await(&*iterable, |item| {
            result.push(item);
            Ok(ControlFlow::Continue(()))
        })
        .await?;
        Ok(MaybePromise::Promise(Promise::all(result.into_iter().map(Promise::resolved).collect())))
    })
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum AsyncIterableSourceState {
    Initial,
    DoneOk,
    DoneError,
}

/// O destino das emissões de um [`AsyncIterableEmitter`].
trait EmitTarget<T> {
    fn emit_one(&self, value: T);
    fn emit_many(&self, values: Vec<T>);
    fn reject(&self, error: Error);
}

/// `AsyncIterableEmitter<T>`: emite valores de forma assíncrona ou leva o iterável ao estado de erro com
/// `reject()`. Só vale durante o executor.
pub struct AsyncIterableEmitter<T: Clone + 'static>(Rc<dyn EmitTarget<T>>);

impl<T: Clone + 'static> Clone for AsyncIterableEmitter<T> {
    fn clone(&self) -> Self {
        Self(self.0.clone())
    }
}

impl<T: Clone + 'static> AsyncIterableEmitter<T> {
    /// O valor entra no fim.
    pub fn emit_one(&self, value: T) {
        self.0.emit_one(value);
    }

    /// Os valores entram no fim.
    pub fn emit_many(&self, values: Vec<T>) {
        self.0.emit_many(values);
    }

    /// Escrever um erro invalida o iterável para sempre: quem o usa agora e depois recebe o erro.
    pub fn reject(&self, error: Error) {
        self.0.reject(error);
    }
}

/// `AsyncIterableExecutor<T>`: `(emitter) => unknown | Promise<unknown>`.
pub type AsyncIterableExecutor<T> = Box<dyn FnOnce(AsyncIterableEmitter<T>) -> MaybePromise<()>>;

/// Um executor que é uma função `async`.
fn async_executor<T: Clone + 'static, F: Future<Output = Result<(), Error>> + 'static>(body: impl FnOnce(AsyncIterableEmitter<T>) -> F + 'static) -> AsyncIterableExecutor<T> {
    Box::new(move |emitter| MaybePromise::Promise(async_fn(body(emitter))))
}

fn array_executor<T: Clone + 'static>(items: Vec<T>) -> AsyncIterableExecutor<T> {
    Box::new(move |writer| {
        writer.emit_many(items);
        MaybePromise::Value(())
    })
}

fn from_promise_executor<T: Clone + 'static>(promise: Promise<Vec<T>>) -> AsyncIterableExecutor<T> {
    async_executor(move |emitter| async move {
        emitter.emit_many(promise.await?);
        Ok(())
    })
}

fn promises_resolve_order_executor<T: Clone + 'static>(promises: Vec<Promise<T>>) -> AsyncIterableExecutor<T> {
    async_executor(move |emitter| async move {
        let all: Vec<Promise<()>> = promises
            .into_iter()
            .map(|promise| {
                let emitter = emitter.clone();
                async_fn(async move {
                    emitter.emit_one(promise.await?);
                    Ok(())
                })
            })
            .collect();
        Promise::all(all).await?;
        Ok(())
    })
}

fn merge_executor<T: Clone + 'static>(iterables: Vec<Rc<dyn AsyncIterable<T>>>) -> AsyncIterableExecutor<T> {
    async_executor(move |emitter| async move {
        let all: Vec<Promise<()>> = iterables
            .into_iter()
            .map(|iterable| {
                let emitter = emitter.clone();
                async_fn(async move {
                    for_await(&*iterable, |item| {
                        emitter.emit_one(item);
                        Ok(ControlFlow::Continue(()))
                    })
                    .await
                })
            })
            .collect();
        Promise::all(all).await?;
        Ok(())
    })
}

fn map_executor<T: Clone + 'static, R: Clone + 'static>(iterable: Rc<dyn AsyncIterable<T>>, map_fn: impl Fn(T) -> R + 'static) -> AsyncIterableExecutor<R> {
    async_executor(move |emitter| async move {
        for_await(&*iterable, |item| {
            emitter.emit_one(map_fn(item));
            Ok(ControlFlow::Continue(()))
        })
        .await
    })
}

fn filter_executor<T: Clone + 'static>(iterable: Rc<dyn AsyncIterable<T>>, filter_fn: impl Fn(&T) -> bool + 'static) -> AsyncIterableExecutor<T> {
    async_executor(move |emitter| async move {
        for_await(&*iterable, |item| {
            if filter_fn(&item) {
                emitter.emit_one(item);
            }
            Ok(ControlFlow::Continue(()))
        })
        .await
    })
}

fn coalesce_executor<T: Clone + 'static>(iterable: Rc<dyn AsyncIterable<Option<T>>>) -> AsyncIterableExecutor<T> {
    async_executor(move |emitter| async move {
        for_await(&*iterable, |item| {
            if let Some(item) = item {
                emitter.emit_one(item);
            }
            Ok(ControlFlow::Continue(()))
        })
        .await
    })
}

/// `AsyncIterableObject.toPromise(iterable)`.
pub fn async_iterable_to_promise<T: Clone + 'static>(iterable: Rc<dyn AsyncIterable<T>>) -> Promise<Vec<T>> {
    async_fn(async move {
        let mut result = Vec::new();
        for_await(&*iterable, |item| {
            result.push(item);
            Ok(ControlFlow::Continue(()))
        })
        .await?;
        Ok(result)
    })
}

struct ObjectWriter<T: Clone + 'static> {
    target: Rc<AsyncIterableObject<T>>,
    active: Cell<bool>,
}

impl<T: Clone + 'static> EmitTarget<T> for ObjectWriter<T> {
    fn emit_one(&self, value: T) {
        if self.active.get() {
            self.target.emit_one(value);
        }
    }

    fn emit_many(&self, values: Vec<T>) {
        if self.active.get() {
            self.target.emit_many(values);
        }
    }

    fn reject(&self, error: Error) {
        if self.active.get() {
            self.target.reject(error);
        }
    }
}

/// Uma implementação rica de `AsyncIterable<T>`: o executor emite valores e cada iterador vê todos, desde o
/// primeiro.
pub struct AsyncIterableObject<T: Clone + 'static> {
    me: Weak<Self>,
    state: Cell<AsyncIterableSourceState>,
    results: RefCell<Vec<T>>,
    error: RefCell<Option<Error>>,
    on_return: Option<Rc<dyn Fn()>>,
    on_state_changed: Rc<Emitter<()>>,
}

impl<T: Clone + 'static> AsyncIterableObject<T> {
    pub fn from_array(items: Vec<T>) -> Rc<Self> {
        Self::new(array_executor(items), None)
    }

    pub fn from_promise(promise: Promise<Vec<T>>) -> Rc<Self> {
        Self::new(from_promise_executor(promise), None)
    }

    pub fn from_promises_resolve_order(promises: Vec<Promise<T>>) -> Rc<Self> {
        Self::new(promises_resolve_order_executor(promises), None)
    }

    pub fn merge(iterables: Vec<Rc<dyn AsyncIterable<T>>>) -> Rc<Self> {
        Self::new(merge_executor(iterables), None)
    }

    /// `AsyncIterableObject.EMPTY`: aqui um iterável novo e vazio a cada chamada.
    pub fn empty() -> Rc<Self> {
        Self::from_array(Vec::new())
    }

    /// `new AsyncIterableObject(executor, onReturn)`. O executor roda numa microtarefa.
    pub fn new(executor: impl FnOnce(AsyncIterableEmitter<T>) -> MaybePromise<()> + 'static, on_return: Option<Rc<dyn Fn()>>) -> Rc<Self> {
        let this = Rc::new_cyclic(|me| Self {
            me: me.clone(),
            state: Cell::new(AsyncIterableSourceState::Initial),
            results: RefCell::new(Vec::new()),
            error: RefCell::new(None),
            on_return,
            on_state_changed: Emitter::new(),
        });

        let source = this.clone();
        runtime::queue_microtask(move || {
            runtime::spawn_local(async move {
                let writer = Rc::new(ObjectWriter { target: source.clone(), active: Cell::new(true) });
                let outcome = match errors::catch(|| executor(AsyncIterableEmitter(writer.clone()))) {
                    Ok(returned) => await_maybe(returned).await,
                    Err(error) => Err(error),
                };
                match outcome {
                    Ok(()) => source.resolve(),
                    Err(error) => source.reject(error),
                }
                // The executor has settled; emitting afterwards must be a no-op per the documented
                // "no effect after resolve()/reject()" contract (see emit_one).
                writer.active.set(false);
            });
        });
        this
    }

    pub fn map<R: Clone + 'static>(self: &Rc<Self>, map_fn: impl Fn(T) -> R + 'static) -> Rc<AsyncIterableObject<R>> {
        AsyncIterableObject::<R>::map_of(self.clone(), map_fn)
    }

    /// `AsyncIterableObject.map(iterable, mapFn)`: `T` é o tipo do resultado.
    pub fn map_of<S: Clone + 'static>(iterable: Rc<dyn AsyncIterable<S>>, map_fn: impl Fn(S) -> T + 'static) -> Rc<Self> {
        Self::new(map_executor(iterable, map_fn), None)
    }

    pub fn filter(self: &Rc<Self>, filter_fn: impl Fn(&T) -> bool + 'static) -> Rc<Self> {
        Self::filter_of(self.clone(), filter_fn)
    }

    /// `AsyncIterableObject.filter(iterable, filterFn)`.
    pub fn filter_of(iterable: Rc<dyn AsyncIterable<T>>, filter_fn: impl Fn(&T) -> bool + 'static) -> Rc<Self> {
        Self::new(filter_executor(iterable, filter_fn), None)
    }

    pub fn to_promise(self: &Rc<Self>) -> Promise<Vec<T>> {
        async_iterable_to_promise(self.clone())
    }

    /// O valor entra no fim. Sem efeito depois de `resolve()` ou `reject()`.
    fn emit_one(&self, value: T) {
        if self.state.get() != AsyncIterableSourceState::Initial {
            return;
        }
        // it is important to add new values at the end, as we may have iterators already running on the array
        self.results.borrow_mut().push(value);
        self.on_state_changed.fire(());
    }

    /// Os valores entram no fim. Sem efeito depois de `resolve()` ou `reject()`.
    fn emit_many(&self, values: Vec<T>) {
        if self.state.get() != AsyncIterableSourceState::Initial {
            return;
        }
        self.results.borrow_mut().extend(values);
        self.on_state_changed.fire(());
    }

    /// Marca o vetor de resultados como completo. Sem efeito depois de `resolve()` ou `reject()`.
    fn resolve(&self) {
        if self.state.get() != AsyncIterableSourceState::Initial {
            return;
        }
        self.state.set(AsyncIterableSourceState::DoneOk);
        self.on_state_changed.fire(());
    }

    /// Escrever um erro invalida o iterável para sempre. Sem efeito depois de `resolve()` ou `reject()`.
    fn reject(&self, error: Error) {
        if self.state.get() != AsyncIterableSourceState::Initial {
            return;
        }
        self.state.set(AsyncIterableSourceState::DoneError);
        *self.error.borrow_mut() = Some(error);
        self.on_state_changed.fire(());
    }
}

impl<T: Clone + 'static> AsyncIterableObject<Option<T>> {
    /// `AsyncIterableObject.coalesce(iterable)`: descarta os `None`.
    pub fn coalesce_of(iterable: Rc<dyn AsyncIterable<Option<T>>>) -> Rc<AsyncIterableObject<T>> {
        AsyncIterableObject::new(coalesce_executor(iterable), None)
    }

    pub fn coalesce(self: &Rc<Self>) -> Rc<AsyncIterableObject<T>> {
        Self::coalesce_of(self.clone())
    }
}

struct ObjectIterator<T: Clone + 'static> {
    source: Rc<AsyncIterableObject<T>>,
    index: Rc<Cell<usize>>,
}

impl<T: Clone + 'static> AsyncIterator<T> for ObjectIterator<T> {
    fn next(&self) -> Promise<Option<T>> {
        let (source, index) = (self.source.clone(), self.index.clone());
        async_fn(async move {
            loop {
                if source.state.get() == AsyncIterableSourceState::DoneError {
                    return Err(source.error.borrow().clone().unwrap_or_else(|| Error::new("null")));
                }
                let current = index.get();
                let item = source.results.borrow().get(current).cloned();
                if let Some(item) = item {
                    index.set(current + 1);
                    return Ok(Some(item));
                }
                if source.state.get() == AsyncIterableSourceState::DoneOk {
                    return Ok(None);
                }
                source.on_state_changed.event().to_promise(None).await?;
            }
        })
    }

    fn return_(&self) -> Option<Promise<()>> {
        if let Some(on_return) = &self.source.on_return {
            on_return();
        }
        Some(Promise::resolved(()))
    }
}

impl<T: Clone + 'static> AsyncIterable<T> for AsyncIterableObject<T> {
    fn async_iterator(&self) -> Rc<dyn AsyncIterator<T>> {
        Rc::new(ObjectIterator { source: self.me.upgrade().expect("o iterável está vivo"), index: Rc::new(Cell::new(0)) })
    }
}

/// `createCancelableAsyncIterableProducer(callback)`.
pub fn create_cancelable_async_iterable_producer<T: Clone + 'static>(callback: impl FnOnce(CancellationToken) -> Rc<dyn AsyncIterable<T>>) -> CancelableAsyncIterableProducer<T> {
    let source = CancellationTokenSource::new(None);
    let inner_iterable = callback(source.token());

    let executor_source = source.clone();
    CancelableAsyncIterableProducer::new(
        source,
        async_executor(move |emitter| async move {
            let source = executor_source;
            let subscription: Rc<RefCell<Option<DisposableRef>>> = Rc::default();
            let listener = source.token().on_cancellation_requested().listen({
                let (subscription, source, emitter) = (subscription.clone(), source.clone(), emitter.clone());
                move |_| {
                    dispose_slot(&subscription);
                    source.dispose();
                    emitter.reject(Error::cancellation());
                }
            });
            *subscription.borrow_mut() = Some(listener);

            let outcome = for_await(&*inner_iterable, |item| {
                if source.token().is_cancellation_requested() {
                    // canceled in the meantime
                    return Ok(ControlFlow::Break(()));
                }
                emitter.emit_one(item);
                Ok(ControlFlow::Continue(()))
            })
            .await;
            match outcome {
                Ok(()) => {
                    if !source.token().is_cancellation_requested() {
                        dispose_slot(&subscription);
                        source.dispose();
                    }
                }
                Err(error) => {
                    dispose_slot(&subscription);
                    source.dispose();
                    emitter.reject(error);
                }
            }
            Ok(())
        }),
    )
}

/// `AsyncIterableSource`: o dono emite valores e resolve/rejeita o iterável.
/// Uma função que o dono da fonte troca quando o executor assume (`this._emitOneFn = ...`).
type SwappableFn<A> = Rc<RefCell<Rc<dyn Fn(A)>>>;

pub struct AsyncIterableSource<T: Clone + 'static> {
    deferred: Rc<DeferredPromise<()>>,
    async_iterable: Rc<AsyncIterableObject<T>>,
    error_fn: SwappableFn<Error>,
    emit_one_fn: SwappableFn<T>,
    emit_many_fn: SwappableFn<Vec<T>>,
}

impl<T: Clone + 'static> AsyncIterableSource<T> {
    /// `on_return` roda quando quem consome termina cedo (o `break`/`return` do `for await`); NÃO roda
    /// quando o dono resolve a fonte.
    pub fn new(on_return: Option<Rc<dyn Fn()>>) -> Self {
        let deferred = Rc::new(DeferredPromise::<()>::new());
        let early_error: Rc<RefCell<Option<Error>>> = Rc::default();
        let early_items: Rc<RefCell<Option<Vec<T>>>> = Rc::default();

        let error_fn: SwappableFn<Error> = {
            let early_error = early_error.clone();
            Rc::new(RefCell::new(Rc::new(move |error| {
                early_error.borrow_mut().get_or_insert(error);
            })))
        };
        let emit_one_fn: SwappableFn<T> = {
            let early_items = early_items.clone();
            Rc::new(RefCell::new(Rc::new(move |item| early_items.borrow_mut().get_or_insert_with(Vec::new).push(item))))
        };
        let emit_many_fn: SwappableFn<Vec<T>> = {
            let early_items = early_items.clone();
            Rc::new(RefCell::new(Rc::new(move |items: Vec<T>| early_items.borrow_mut().get_or_insert_with(Vec::new).extend(items))))
        };

        let async_iterable = {
            let (deferred, error_fn, emit_one_fn, emit_many_fn) = (deferred.clone(), error_fn.clone(), emit_one_fn.clone(), emit_many_fn.clone());
            AsyncIterableObject::new(
                move |emitter| {
                    let early_error = early_error.borrow().clone();
                    if let Some(early_error) = early_error {
                        emitter.reject(early_error);
                        return MaybePromise::Value(());
                    }
                    let early_items = early_items.borrow().clone();
                    if let Some(early_items) = early_items {
                        emitter.emit_many(early_items);
                    }
                    let on_error = emitter.clone();
                    *error_fn.borrow_mut() = Rc::new(move |error| on_error.reject(error));
                    let on_one = emitter.clone();
                    *emit_one_fn.borrow_mut() = Rc::new(move |item| on_one.emit_one(item));
                    *emit_many_fn.borrow_mut() = Rc::new(move |items| emitter.emit_many(items));
                    MaybePromise::Promise(deferred.promise.clone())
                },
                on_return,
            )
        };

        Self { deferred, async_iterable, error_fn, emit_one_fn, emit_many_fn }
    }

    pub fn async_iterable(&self) -> Rc<AsyncIterableObject<T>> {
        self.async_iterable.clone()
    }

    pub fn resolve(&self) {
        self.deferred.complete(());
    }

    pub fn reject(&self, error: Error) {
        let error_fn = self.error_fn.borrow().clone();
        error_fn(error);
        self.deferred.complete(());
    }

    pub fn emit_one(&self, item: T) {
        let emit_one_fn = self.emit_one_fn.borrow().clone();
        emit_one_fn(item);
    }

    pub fn emit_many(&self, items: Vec<T>) {
        let emit_many_fn = self.emit_many_fn.borrow().clone();
        emit_many_fn(items);
    }
}

struct CancellableIterator<T: Clone + 'static> {
    iterator: Rc<dyn AsyncIterator<T>>,
    token: CancellationToken,
    me: Weak<Self>,
}

impl<T: Clone + 'static> AsyncIterator<T> for CancellableIterator<T> {
    fn next(&self) -> Promise<Option<T>> {
        let (iterator, token) = (self.iterator.clone(), self.token.clone());
        async_fn(async move {
            if token.is_cancellation_requested() {
                return Ok(None);
            }
            let result = race_cancellation(&iterator.next(), &token).await?;
            Ok(result.flatten())
        })
    }

    fn return_(&self) -> Option<Promise<()>> {
        self.iterator.return_()
    }

    fn throw_(&self, error: Error) -> Option<Promise<()>> {
        self.iterator.throw_(error)
    }
}

impl<T: Clone + 'static> AsyncIterable<T> for CancellableIterator<T> {
    fn async_iterator(&self) -> Rc<dyn AsyncIterator<T>> {
        self.me.upgrade().expect("o iterador está vivo")
    }
}

/// `cancellableIterable(iterator, token)` para um `AsyncIterator`: para de entregar itens quando o token cancela.
pub fn cancellable_iterator<T: Clone + 'static>(iterator: Rc<dyn AsyncIterator<T>>, token: CancellationToken) -> Rc<dyn AsyncIterableIterator<T>> {
    Rc::new_cyclic(|me| CancellableIterator { iterator, token, me: me.clone() })
}

/// `cancellableIterable(iterable, token)` para um `AsyncIterable`.
pub fn cancellable_iterable<T: Clone + 'static>(iterable: &dyn AsyncIterable<T>, token: CancellationToken) -> Rc<dyn AsyncIterableIterator<T>> {
    cancellable_iterator(iterable.async_iterator(), token)
}

/// `AsyncIterableIterator<T>`: ao mesmo tempo iterador e iterável (`[Symbol.asyncIterator]() { return this }`).
pub trait AsyncIterableIterator<T: Clone + 'static>: AsyncIterator<T> + AsyncIterable<T> {}

impl<T: Clone + 'static, I: AsyncIterator<T> + AsyncIterable<T>> AsyncIterableIterator<T> for I {}

/// O que `ProducerConsumer` guarda: um valor, ou o erro que o produtor escreveu.
type ProducerConsumerValue<V> = Result<V, Error>;

struct ProducerConsumer<V: Clone + 'static> {
    unsatisfied_consumers: RefCell<VecDeque<Rc<DeferredPromise<V>>>>,
    unconsumed_values: RefCell<VecDeque<ProducerConsumerValue<V>>>,
    final_value: RefCell<Option<ProducerConsumerValue<V>>>,
}

impl<V: Clone + 'static> ProducerConsumer<V> {
    fn new() -> Self {
        Self { unsatisfied_consumers: RefCell::new(VecDeque::new()), unconsumed_values: RefCell::new(VecDeque::new()), final_value: RefCell::new(None) }
    }

    fn has_final_value(&self) -> bool {
        self.final_value.borrow().is_some()
    }

    fn produce(&self, value: ProducerConsumerValue<V>) {
        self.ensure_no_final_value();
        let deferred = self.unsatisfied_consumers.borrow_mut().pop_front();
        match deferred {
            Some(deferred) => Self::resolve_or_reject_deferred(&deferred, value),
            None => self.unconsumed_values.borrow_mut().push_back(value),
        }
    }

    fn produce_final(&self, value: ProducerConsumerValue<V>) {
        self.ensure_no_final_value();
        *self.final_value.borrow_mut() = Some(value.clone());
        let consumers = std::mem::take(&mut *self.unsatisfied_consumers.borrow_mut());
        for deferred in consumers {
            Self::resolve_or_reject_deferred(&deferred, value.clone());
        }
    }

    fn ensure_no_final_value(&self) {
        if self.has_final_value() {
            errors::throw(Error::bug_indicating(Some("ProducerConsumer: cannot produce after final value has been set")));
        }
    }

    fn resolve_or_reject_deferred(deferred: &DeferredPromise<V>, value: ProducerConsumerValue<V>) {
        match value {
            Ok(value) => deferred.complete(value),
            Err(error) => deferred.error(error),
        };
    }

    fn consume(&self) -> Promise<V> {
        let unconsumed = self.unconsumed_values.borrow_mut().pop_front();
        let value = unconsumed.or_else(|| self.final_value.borrow().clone());
        match value {
            Some(Ok(value)) => Promise::resolved(value),
            Some(Err(error)) => Promise::rejected(error),
            None => {
                let deferred = Rc::new(DeferredPromise::<V>::new());
                self.unsatisfied_consumers.borrow_mut().push_back(deferred.clone());
                deferred.promise.clone()
            }
        }
    }
}

struct ProducerWriter<T: Clone + 'static> {
    producer: Rc<AsyncIterableProducer<T>>,
}

impl<T: Clone + 'static> EmitTarget<T> for ProducerWriter<T> {
    fn emit_one(&self, value: T) {
        self.producer.producer_consumer.produce(Ok(Some(value)));
    }

    fn emit_many(&self, values: Vec<T>) {
        for value in values {
            self.producer.producer_consumer.produce(Ok(Some(value)));
        }
    }

    fn reject(&self, error: Error) {
        self.producer.finish_error(error);
    }
}

/// Diferença importante para o [`AsyncIterableObject`]: se for iterado duas vezes, o segundo iterador não
/// vê os valores que o primeiro já consumiu.
pub struct AsyncIterableProducer<T: Clone + 'static> {
    me: Weak<Self>,
    producer_consumer: ProducerConsumer<Option<T>>,
    on_return: Option<Rc<dyn Fn()>>,
}

impl<T: Clone + 'static> AsyncIterableProducer<T> {
    /// `new AsyncIterableProducer(executor, onReturn)`. O executor roda numa microtarefa.
    pub fn new(executor: impl FnOnce(AsyncIterableEmitter<T>) -> MaybePromise<()> + 'static, on_return: Option<Rc<dyn Fn()>>) -> Rc<Self> {
        let this = Rc::new_cyclic(|me| Self { me: me.clone(), producer_consumer: ProducerConsumer::new(), on_return });

        let producer = this.clone();
        runtime::queue_microtask(move || {
            runtime::spawn_local(async move {
                let p = executor(AsyncIterableEmitter(Rc::new(ProducerWriter { producer: producer.clone() })));

                if !producer.producer_consumer.has_final_value() {
                    match await_maybe(p).await {
                        Ok(()) => producer.finish_ok(),
                        Err(error) => producer.finish_error(error),
                    }
                }
            });
        });
        this
    }

    pub fn from_array(items: Vec<T>) -> Rc<Self> {
        Self::new(array_executor(items), None)
    }

    pub fn from_promise(promise: Promise<Vec<T>>) -> Rc<Self> {
        Self::new(from_promise_executor(promise), None)
    }

    pub fn from_promises_resolve_order(promises: Vec<Promise<T>>) -> Rc<Self> {
        Self::new(promises_resolve_order_executor(promises), None)
    }

    pub fn merge(iterables: Vec<Rc<dyn AsyncIterable<T>>>) -> Rc<Self> {
        Self::new(merge_executor(iterables), None)
    }

    /// `AsyncIterableProducer.EMPTY`: aqui um produtor novo e vazio a cada chamada.
    pub fn empty() -> Rc<Self> {
        Self::from_array(Vec::new())
    }

    /// `AsyncIterableProducer.map(iterable, mapFn)`: `T` é o tipo do resultado.
    pub fn map_of<S: Clone + 'static>(iterable: Rc<dyn AsyncIterable<S>>, map_fn: impl Fn(S) -> T + 'static) -> Rc<Self> {
        Self::new(map_executor(iterable, map_fn), None)
    }

    pub fn map<R: Clone + 'static>(self: &Rc<Self>, map_fn: impl Fn(T) -> R + 'static) -> Rc<AsyncIterableProducer<R>> {
        AsyncIterableProducer::<R>::map_of(self.clone(), map_fn)
    }

    /// `AsyncIterableProducer.filter(iterable, filterFn)`.
    pub fn filter_of(iterable: Rc<dyn AsyncIterable<T>>, filter_fn: impl Fn(&T) -> bool + 'static) -> Rc<Self> {
        Self::new(filter_executor(iterable, filter_fn), None)
    }

    pub fn filter(self: &Rc<Self>, filter_fn: impl Fn(&T) -> bool + 'static) -> Rc<Self> {
        Self::filter_of(self.clone(), filter_fn)
    }

    /// `AsyncIterableProducer.tee(iterable)`: dois produtores que recebem os mesmos itens.
    pub fn tee(iterable: Rc<dyn AsyncIterable<T>>) -> (Rc<Self>, Rc<Self>) {
        let emitter1: Rc<RefCell<Option<AsyncIterableEmitter<T>>>> = Rc::default();
        let emitter2: Rc<RefCell<Option<AsyncIterableEmitter<T>>>> = Rc::default();

        let defer = Rc::new(DeferredPromise::<()>::new());

        let start = {
            let (emitter1, emitter2, defer) = (emitter1.clone(), emitter2.clone(), defer.clone());
            move || {
                let (emitter1, emitter2, defer, iterable) = (emitter1.clone(), emitter2.clone(), defer.clone(), iterable.clone());
                // `const start = async () => { ... }`: roda já, até o primeiro `await`
                async_fn(async move {
                    let (first, second) = match (emitter1.borrow().clone(), emitter2.borrow().clone()) {
                        (Some(first), Some(second)) => (first, second),
                        _ => return Ok(()), // not yet ready
                    };
                    let outcome = for_await(&*iterable, |item| {
                        first.emit_one(item.clone());
                        second.emit_one(item);
                        Ok(ControlFlow::Continue(()))
                    })
                    .await;
                    if let Err(error) = outcome {
                        first.reject(error.clone());
                        second.reject(error);
                    }
                    defer.complete(());
                    Ok(())
                });
            }
        };
        let start: Rc<dyn Fn()> = Rc::new(start);

        let tee_executor = |slot: Rc<RefCell<Option<AsyncIterableEmitter<T>>>>| {
            let (start, defer) = (start.clone(), defer.clone());
            async_executor(move |emitter| async move {
                *slot.borrow_mut() = Some(emitter);
                start();
                defer.promise.clone().await
            })
        };
        let p1 = Self::new(tee_executor(emitter1), None);
        let p2 = Self::new(tee_executor(emitter2), None);
        (p1, p2)
    }

    pub fn coalesce_of(iterable: Rc<dyn AsyncIterable<Option<T>>>) -> Rc<AsyncIterableProducer<T>>
    where
        T: Clone,
    {
        AsyncIterableProducer::new(coalesce_executor(iterable), None)
    }

    fn finish_ok(&self) {
        if !self.producer_consumer.has_final_value() {
            self.producer_consumer.produce_final(Ok(None));
        }
    }

    fn finish_error(&self, error: Error) {
        if !self.producer_consumer.has_final_value() {
            self.producer_consumer.produce_final(Err(error));
        }
        // Warning: this can cause to dropped errors.
    }
}

impl<T: Clone + 'static> AsyncIterableProducer<Option<T>> {
    pub fn coalesce(self: &Rc<Self>) -> Rc<AsyncIterableProducer<T>> {
        AsyncIterableProducer::coalesce_of(self.clone())
    }
}

struct ProducerIterator<T: Clone + 'static> {
    producer: Rc<AsyncIterableProducer<T>>,
}

impl<T: Clone + 'static> AsyncIterator<T> for ProducerIterator<T> {
    fn next(&self) -> Promise<Option<T>> {
        self.producer.producer_consumer.consume()
    }

    fn return_(&self) -> Option<Promise<()>> {
        if let Some(on_return) = &self.producer.on_return {
            on_return();
        }
        Some(Promise::resolved(()))
    }

    fn throw_(&self, error: Error) -> Option<Promise<()>> {
        self.producer.finish_error(error);
        Some(Promise::resolved(()))
    }
}

impl<T: Clone + 'static> AsyncIterable<T> for AsyncIterableProducer<T> {
    fn async_iterator(&self) -> Rc<dyn AsyncIterator<T>> {
        Rc::new(ProducerIterator { producer: self.me.upgrade().expect("o produtor está vivo") })
    }
}

/// `CancelableAsyncIterableProducer`: um [`AsyncIterableProducer`] que se cancela com uma fonte de token.
pub struct CancelableAsyncIterableProducer<T: Clone + 'static> {
    producer: Rc<AsyncIterableProducer<T>>,
    source: CancellationTokenSource,
}

impl<T: Clone + 'static> CancelableAsyncIterableProducer<T> {
    pub fn new(source: CancellationTokenSource, executor: AsyncIterableExecutor<T>) -> Self {
        Self { producer: AsyncIterableProducer::new(executor, None), source }
    }

    pub fn cancel(&self) {
        self.source.cancel();
    }
}

impl<T: Clone + 'static> Deref for CancelableAsyncIterableProducer<T> {
    type Target = AsyncIterableProducer<T>;

    fn deref(&self) -> &AsyncIterableProducer<T> {
        &self.producer
    }
}

impl<T: Clone + 'static> AsyncIterable<T> for CancelableAsyncIterableProducer<T> {
    fn async_iterator(&self) -> Rc<dyn AsyncIterator<T>> {
        self.producer.async_iterator()
    }
}

// endregion

// region: AsyncReader

/// O que `AsyncReader` devolve: um item, ou o `AsyncReaderEndOfStream` do original.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum AsyncRead<T> {
    Item(T),
    EndOfStream,
}

struct AsyncReaderInner<T: Clone + 'static> {
    source: Rc<dyn AsyncIterator<T>>,
    buffer: RefCell<VecDeque<T>>,
    at_end: Cell<bool>,
    extend_buffer_promise: RefCell<Option<Promise<()>>>,
}

impl<T: Clone + 'static> AsyncReaderInner<T> {
    fn extend_buffer(this: &Rc<Self>) -> Promise<()> {
        if this.at_end.get() {
            return Promise::resolved(());
        }

        if this.extend_buffer_promise.borrow().is_none() {
            let reader = this.clone();
            let extending = async_fn(async move {
                let next = reader.source.next().await?;
                *reader.extend_buffer_promise.borrow_mut() = None;
                match next {
                    None => reader.at_end.set(true),
                    Some(value) => reader.buffer.borrow_mut().push_back(value),
                }
                Ok(())
            });
            // o corpo só avança depois do primeiro `await`, então a promessa já foi guardada quando ele mexe nela
            *this.extend_buffer_promise.borrow_mut() = Some(extending);
        }

        this.extend_buffer_promise.borrow().clone().expect("a promessa de extensão")
    }
}

/// `AsyncReader`: lê um `AsyncIterator` com buffer, para espiar e consumir.
pub struct AsyncReader<T: Clone + 'static> {
    inner: Rc<AsyncReaderInner<T>>,
}

impl<T: Clone + 'static> Clone for AsyncReader<T> {
    fn clone(&self) -> Self {
        Self { inner: self.inner.clone() }
    }
}

impl<T: Clone + 'static> AsyncReader<T> {
    pub fn new(source: Rc<dyn AsyncIterator<T>>) -> Self {
        Self { inner: Rc::new(AsyncReaderInner { source, buffer: RefCell::new(VecDeque::new()), at_end: Cell::new(false), extend_buffer_promise: RefCell::new(None) }) }
    }

    pub fn end_of_stream(&self) -> bool {
        self.inner.buffer.borrow().is_empty() && self.inner.at_end.get()
    }

    pub fn read(&self) -> Promise<AsyncRead<T>> {
        let inner = self.inner.clone();
        async_fn(async move {
            if inner.buffer.borrow().is_empty() && !inner.at_end.get() {
                AsyncReaderInner::extend_buffer(&inner).await?;
            }
            let front = inner.buffer.borrow_mut().pop_front();
            Ok(front.map_or(AsyncRead::EndOfStream, AsyncRead::Item))
        })
    }

    pub fn read_while(&self, predicate: impl Fn(&T) -> bool + 'static, mut callback: impl FnMut(T) -> MaybePromise<()> + 'static) -> Promise<()> {
        let this = self.clone();
        async_fn(async move {
            loop {
                let AsyncRead::Item(piece) = this.peek().await? else {
                    break;
                };
                if !predicate(&piece) {
                    break;
                }
                this.read().await?; // consume
                await_maybe(callback(piece)).await?;
            }
            Ok(())
        })
    }

    pub fn read_buffered_or_throw(&self) -> AsyncRead<T> {
        let value = self.peek_buffered_or_throw();
        self.inner.buffer.borrow_mut().pop_front();
        value
    }

    pub fn consume_to_end(&self) -> Promise<()> {
        let this = self.clone();
        async_fn(async move {
            while !this.end_of_stream() {
                this.read().await?;
            }
            Ok(())
        })
    }

    pub fn peek(&self) -> Promise<AsyncRead<T>> {
        let inner = self.inner.clone();
        async_fn(async move {
            if inner.buffer.borrow().is_empty() && !inner.at_end.get() {
                AsyncReaderInner::extend_buffer(&inner).await?;
            }
            let front = inner.buffer.borrow().front().cloned();
            Ok(front.map_or(AsyncRead::EndOfStream, AsyncRead::Item))
        })
    }

    pub fn peek_buffered_or_throw(&self) -> AsyncRead<T> {
        let front = self.inner.buffer.borrow().front().cloned();
        match front {
            Some(front) => AsyncRead::Item(front),
            None if self.inner.at_end.get() => AsyncRead::EndOfStream,
            None => errors::throw(Error::bug_indicating(Some("No buffered elements"))),
        }
    }

    /// Espia o próximo item esperando no máximo `timeout_ms`; `None` se o prazo vence antes de haver item.
    pub fn peek_timeout(&self, timeout_ms: u64) -> Promise<Option<AsyncRead<T>>> {
        let inner = self.inner.clone();
        async_fn(async move {
            if inner.buffer.borrow().is_empty() && !inner.at_end.get() {
                race_timeout(&AsyncReaderInner::extend_buffer(&inner), timeout_ms, None).await?;
            }
            if inner.at_end.get() {
                return Ok(Some(AsyncRead::EndOfStream));
            }
            let front = inner.buffer.borrow().front().cloned();
            Ok(front.map(AsyncRead::Item))
        })
    }
}

// endregion
