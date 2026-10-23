//! Promessa com a semântica da `Promise` do JavaScript sobre o [`runtime`](crate::runtime).
//!
//! As reações (`then`) rodam como microtarefas, nunca de forma síncrona, na ordem em que foram
//! registradas; resolver ou rejeitar uma promessa já assentada não faz nada. `Promise<T>` também é um
//! `Future`, para o `await` dentro de [`runtime::spawn_local`](crate::runtime::spawn_local).

use std::cell::{Cell, RefCell};
use std::future::{Future, IntoFuture};
use std::ops::Deref;
use std::pin::Pin;
use std::rc::Rc;
use std::task::{Context, Poll};

use crate::errors::{self, Error};
use crate::runtime::queue_microtask;

type Reaction<T> = Box<dyn FnOnce(Result<T, Error>)>;
type Settled<T> = Option<Result<T, Error>>;

enum State<T> {
    Pending { reactions: Vec<Reaction<T>> },
    Settled(Result<T, Error>),
}

/// `Promise<T>`. Clonar a promessa compartilha o mesmo estado, como duas referências no JS.
pub struct Promise<T: Clone + 'static> {
    state: Rc<RefCell<State<T>>>,
}

impl<T: Clone + 'static> Clone for Promise<T> {
    fn clone(&self) -> Self {
        Self { state: self.state.clone() }
    }
}

/// O par `resolve`/`reject` do executor de `new Promise((resolve, reject) => ...)`.
pub struct Resolver<T: Clone + 'static> {
    promise: Promise<T>,
}

impl<T: Clone + 'static> Clone for Resolver<T> {
    fn clone(&self) -> Self {
        Self { promise: self.promise.clone() }
    }
}

impl<T: Clone + 'static> Resolver<T> {
    pub fn resolve(&self, value: T) {
        self.promise.settle(Ok(value));
    }

    pub fn reject(&self, error: Error) {
        self.promise.settle(Err(error));
    }

    /// `resolve(otherPromise)`: assenta quando a outra assentar, com o resultado dela.
    pub fn resolve_with(&self, other: &Promise<T>) {
        let me = self.clone();
        other.react(move |result| me.promise.settle(result));
    }
}

impl<T: Clone + 'static> Promise<T> {
    /// `new Promise(...)` sem executor: devolve a promessa e o resolvedor.
    pub fn new() -> (Self, Resolver<T>) {
        let promise = Self { state: Rc::new(RefCell::new(State::Pending { reactions: Vec::new() })) };
        let resolver = Resolver { promise: promise.clone() };
        (promise, resolver)
    }

    /// `Promise.resolve(value)`.
    pub fn resolved(value: T) -> Self {
        Self { state: Rc::new(RefCell::new(State::Settled(Ok(value)))) }
    }

    /// `Promise.reject(error)`.
    pub fn rejected(error: Error) -> Self {
        Self { state: Rc::new(RefCell::new(State::Settled(Err(error)))) }
    }

    /// `new Promise((resolve, reject) => executor(resolver))`: erro lançado pelo executor rejeita.
    pub fn with_executor(executor: impl FnOnce(Resolver<T>)) -> Self {
        let (promise, resolver) = Self::new();
        let r = resolver.clone();
        if let Err(e) = errors::catch(move || executor(r)) {
            resolver.reject(e);
        }
        promise
    }

    fn settle(&self, result: Result<T, Error>) {
        let reactions = {
            let mut state = self.state.borrow_mut();
            if matches!(*state, State::Settled(_)) {
                return;
            }
            match std::mem::replace(&mut *state, State::Settled(result.clone())) {
                State::Pending { reactions } => reactions,
                State::Settled(_) => unreachable!("conferido acima"),
            }
        };
        for reaction in reactions {
            let result = result.clone();
            queue_microtask(move || reaction(result));
        }
    }

    /// Registra uma reação; se já assentada, ela entra na fila de microtarefas agora.
    fn react(&self, reaction: impl FnOnce(Result<T, Error>) + 'static) {
        let mut state = self.state.borrow_mut();
        match &mut *state {
            State::Pending { reactions, .. } => reactions.push(Box::new(reaction)),
            State::Settled(result) => {
                let result = result.clone();
                queue_microtask(move || reaction(result));
            }
        }
    }

    pub fn is_settled(&self) -> bool {
        matches!(*self.state.borrow(), State::Settled(_))
    }

    /// O resultado, se já assentada.
    pub fn result(&self) -> Option<Result<T, Error>> {
        match &*self.state.borrow() {
            State::Settled(r) => Some(r.clone()),
            State::Pending { .. } => None,
        }
    }

    /// `then(onFulfilled, onRejected)` na forma geral: o tratador recebe o resultado e devolve o
    /// resultado da promessa derivada; erro lançado dentro dele rejeita a derivada.
    pub fn then_result<U: Clone + 'static>(&self, handler: impl FnOnce(Result<T, Error>) -> Result<U, Error> + 'static) -> Promise<U> {
        let (derived, resolver) = Promise::new();
        self.react(move |result| match errors::catch(move || handler(result)) {
            Ok(Ok(v)) => resolver.resolve(v),
            Ok(Err(e)) | Err(e) => resolver.reject(e),
        });
        derived
    }

    /// `then(onFulfilled)`: rejeição passa adiante.
    pub fn then<U: Clone + 'static>(&self, on_fulfilled: impl FnOnce(T) -> U + 'static) -> Promise<U> {
        self.then_result(move |r| r.map(on_fulfilled))
    }

    /// `then(v => otherPromise)`: a derivada segue a promessa devolvida.
    pub fn then_promise<U: Clone + 'static>(&self, on_fulfilled: impl FnOnce(T) -> Promise<U> + 'static) -> Promise<U> {
        let (derived, resolver) = Promise::new();
        self.react(move |result| match result {
            Ok(v) => match errors::catch(move || on_fulfilled(v)) {
                Ok(next) => resolver.resolve_with(&next),
                Err(e) => resolver.reject(e),
            },
            Err(e) => resolver.reject(e),
        });
        derived
    }

    /// `Promise.allSettled(promises)`: cada promessa registra a sua reação (uma microtarefa depois de
    /// assentar) e a última a assentar resolve o resultado, como na especificação; lista vazia resolve já.
    pub fn all_settled(promises: Vec<Promise<T>>) -> Promise<Vec<Result<T, Error>>> {
        let (all, resolver) = Promise::new();
        if promises.is_empty() {
            resolver.resolve(Vec::new());
            return all;
        }
        let results: Rc<RefCell<Vec<Settled<T>>>> = Rc::new(RefCell::new(vec![None; promises.len()]));
        let remaining = Rc::new(Cell::new(promises.len()));
        for (index, promise) in promises.iter().enumerate() {
            let (results, remaining, resolver) = (results.clone(), remaining.clone(), resolver.clone());
            promise.react(move |result| {
                results.borrow_mut()[index] = Some(result);
                remaining.set(remaining.get() - 1);
                if remaining.get() == 0 {
                    resolver.resolve(results.borrow_mut().drain(..).flatten().collect());
                }
            });
        }
        all
    }

    /// `catch(onRejected)`.
    pub fn catch(&self, on_rejected: impl FnOnce(Error) -> T + 'static) -> Promise<T> {
        self.then_result(move |r| Ok(r.unwrap_or_else(on_rejected)))
    }

    /// `finally(onFinally)`: o resultado passa adiante sem mudança.
    pub fn finally(&self, on_finally: impl FnOnce() + 'static) -> Promise<T> {
        self.then_result(move |r| {
            on_finally();
            r
        })
    }
}

/// O `await` de uma promessa. Como no JavaScript, `await` sempre cede: mesmo com a promessa já
/// assentada, a retomada é uma microtarefa (a reação registrada no primeiro `poll`), nunca síncrona.
pub struct PromiseFuture<T: Clone + 'static> {
    promise: Promise<T>,
    registered: bool,
}

impl<T: Clone + 'static> std::future::IntoFuture for Promise<T> {
    type Output = Result<T, Error>;
    type IntoFuture = PromiseFuture<T>;

    fn into_future(self) -> Self::IntoFuture {
        PromiseFuture { promise: self, registered: false }
    }
}

impl<T: Clone + 'static> Future for PromiseFuture<T> {
    type Output = Result<T, Error>;

    fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        if self.registered
            && let Some(result) = self.promise.result()
        {
            return Poll::Ready(result);
        }
        if !self.registered {
            self.registered = true;
            let waker = cx.waker().clone();
            self.promise.react(move |_| waker.wake());
        }
        Poll::Pending
    }
}

/// `CancelablePromise<T>` (de `async.ts`): a promessa com o `cancel()` pendurado. Deref para a promessa,
/// então `then`, `catch` e `result` valem direto, e o `await` também.
pub struct CancelablePromise<T: Clone + 'static> {
    pub promise: Promise<T>,
    cancel: Rc<dyn Fn()>,
}

impl<T: Clone + 'static> CancelablePromise<T> {
    pub fn new(promise: Promise<T>, cancel: impl Fn() + 'static) -> Self {
        Self { promise, cancel: Rc::new(cancel) }
    }

    pub fn cancel(&self) {
        (self.cancel)();
    }
}

impl<T: Clone + 'static> Deref for CancelablePromise<T> {
    type Target = Promise<T>;

    fn deref(&self) -> &Promise<T> {
        &self.promise
    }
}

impl<T: Clone + 'static> IntoFuture for CancelablePromise<T> {
    type Output = Result<T, Error>;
    type IntoFuture = PromiseFuture<T>;

    fn into_future(self) -> PromiseFuture<T> {
        self.promise.into_future()
    }
}

/// `DeferredPromise<T>` (de `async.ts`): promessa com `complete`/`error` e o estado consultável.
pub struct DeferredPromise<T: Clone + 'static> {
    pub promise: Promise<T>,
    resolver: Resolver<T>,
}

impl<T: Clone + 'static> Default for DeferredPromise<T> {
    fn default() -> Self {
        let (promise, resolver) = Promise::new();
        Self { promise, resolver }
    }
}

impl<T: Clone + 'static> DeferredPromise<T> {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn complete(&self, value: T) -> Promise<()> {
        self.resolver.resolve(value);
        Promise::resolved(())
    }

    pub fn error(&self, error: Error) -> Promise<()> {
        self.resolver.reject(error);
        Promise::resolved(())
    }

    /// `cancel()`: rejeita com `CancellationError`.
    pub fn cancel(&self) -> Promise<()> {
        self.error(Error::cancellation())
    }

    pub fn is_settled(&self) -> bool {
        self.promise.is_settled()
    }

    pub fn is_resolved(&self) -> bool {
        matches!(self.promise.result(), Some(Ok(_)))
    }

    pub fn is_rejected(&self) -> bool {
        matches!(self.promise.result(), Some(Err(_)))
    }

    pub fn value(&self) -> Option<T> {
        self.promise.result().and_then(Result::ok)
    }
}
