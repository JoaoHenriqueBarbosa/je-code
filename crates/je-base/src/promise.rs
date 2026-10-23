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

/// O estado de uma promessa e se alguém já registrou reação (`handled`, para o monitor de rejeições não
/// tratadas).
struct Shared<T> {
    state: RefCell<State<T>>,
    handled: Cell<bool>,
}

impl<T> Shared<T> {
    fn new(state: State<T>) -> Rc<Self> {
        Rc::new(Self { state: RefCell::new(state), handled: Cell::new(false) })
    }
}

impl<T> Deref for Shared<T> {
    type Target = RefCell<State<T>>;

    fn deref(&self) -> &RefCell<State<T>> {
        &self.state
    }
}

type UnhandledHook = Rc<dyn Fn(&Error)>;

/// Uma rejeição sem tratador no momento em que assentou, e a consulta de se ganhou um depois.
type UnhandledEntry = (Error, Box<dyn Fn() -> bool>);

struct UnhandledTracker {
    hook: UnhandledHook,
    pending: Vec<UnhandledEntry>,
}

thread_local! {
    static UNHANDLED: RefCell<Option<UnhandledTracker>> = const { RefCell::new(None) };
}

/// O `process.on('unhandledRejection', hook)` do Node, ligado só quando o teste pede (o padrão não
/// monitora nada). Uma promessa rejeitada sem tratador ao fim do esvaziamento das microtarefas chama o
/// `hook` com o erro; `None` desliga o monitor.
pub fn set_unhandled_rejection_hook(hook: Option<impl Fn(&Error) + 'static>) {
    UNHANDLED.with(|u| *u.borrow_mut() = hook.map(|hook| UnhandledTracker { hook: Rc::new(hook), pending: Vec::new() }));
}

fn note_rejection<T: 'static>(shared: &Rc<Shared<T>>, error: &Error) {
    UNHANDLED.with(|u| {
        if let Some(tracker) = u.borrow_mut().as_mut() {
            let shared = shared.clone();
            tracker.pending.push((error.clone(), Box::new(move || shared.handled.get())));
        }
    });
}

/// Chamado pelo loop quando as microtarefas acabam: entrega ao hook as rejeições que continuam sem tratador.
pub(crate) fn flush_unhandled_rejections() {
    let (hook, pending) = UNHANDLED.with(|u| match u.borrow_mut().as_mut() {
        Some(tracker) if !tracker.pending.is_empty() => (Some(tracker.hook.clone()), std::mem::take(&mut tracker.pending)),
        _ => (None, Vec::new()),
    });
    if let Some(hook) = hook {
        for (error, is_handled) in pending {
            if !is_handled() {
                hook(&error);
            }
        }
    }
}

/// `Promise<T>`. Clonar a promessa compartilha o mesmo estado, como duas referências no JS.
pub struct Promise<T: Clone + 'static> {
    state: Rc<Shared<T>>,
}

impl<T: Clone + 'static> Clone for Promise<T> {
    fn clone(&self) -> Self {
        Self { state: self.state.clone() }
    }
}

/// O par `resolve`/`reject` do executor de `new Promise((resolve, reject) => ...)`. As cópias dividem a
/// trava das funções de resolução: depois de `resolve(x)`, `reject(e)` ou `resolve_with(p)`, as chamadas
/// seguintes não fazem nada, mesmo que a promessa ainda esteja pendente esperando `p`.
pub struct Resolver<T: Clone + 'static> {
    promise: Promise<T>,
    locked: Rc<Cell<bool>>,
}

impl<T: Clone + 'static> Clone for Resolver<T> {
    fn clone(&self) -> Self {
        Self { promise: self.promise.clone(), locked: self.locked.clone() }
    }
}

impl<T: Clone + 'static> Resolver<T> {
    pub fn resolve(&self, value: T) {
        if !self.locked.replace(true) {
            self.promise.settle(Ok(value));
        }
    }

    pub fn reject(&self, error: Error) {
        if !self.locked.replace(true) {
            self.promise.settle(Err(error));
        }
    }

    /// `resolve(otherPromise)`: assenta quando a outra assentar, com o resultado dela. Como na
    /// especificação, a adoção custa duas rodadas de microtarefas: uma para o `NewPromiseResolveThenableJob`
    /// chamar `other.then(...)` e outra para a reação (conferido no Node, `tests/runtime.rs`).
    pub fn resolve_with(&self, other: &Promise<T>) {
        if self.locked.replace(true) {
            return;
        }
        let promise = self.promise.clone();
        let other = other.clone();
        queue_microtask(move || other.react(move |result| promise.settle(result)));
    }

    /// `resolve(valueOrPromise)`.
    pub fn resolve_maybe(&self, value: MaybePromise<T>) {
        match value {
            MaybePromise::Value(v) => self.resolve(v),
            MaybePromise::Promise(p) => self.resolve_with(&p),
        }
    }
}

/// `T | Promise<T>` (o `MaybePromise` do `async.ts`): o retorno de um tratador `then` que pode ou não ser
/// uma promessa. O valor direto assenta já, a promessa custa a adoção de [`Resolver::resolve_with`].
#[derive(Clone)]
pub enum MaybePromise<T: Clone + 'static> {
    Value(T),
    Promise(Promise<T>),
}

impl<T: Clone + 'static> From<T> for MaybePromise<T> {
    fn from(value: T) -> Self {
        Self::Value(value)
    }
}

/// O que `async.ts` precisa de uma promessa sem saber o tipo do valor: avisar quando assentar e comparar
/// identidade (`Sequencer` e `SequencerByKey` guardam `Promise<unknown>`).
pub trait Thenable {
    /// `promise.then(f, f)` sem olhar o resultado: `f` roda como microtarefa, uma rodada depois de assentar.
    fn on_settled(&self, f: Box<dyn FnOnce()>);
    /// A identidade da promessa (o `===` do JavaScript).
    fn identity(&self) -> usize;
}

impl<T: Clone + 'static> Thenable for Promise<T> {
    fn on_settled(&self, f: Box<dyn FnOnce()>) {
        self.react(move |_| f());
    }

    fn identity(&self) -> usize {
        Rc::as_ptr(&self.state).cast::<()>() as usize
    }
}

impl<T: Clone + 'static> Promise<T> {
    /// `new Promise(...)` sem executor: devolve a promessa e o resolvedor.
    pub fn new() -> (Self, Resolver<T>) {
        let promise = Self { state: Shared::new(State::Pending { reactions: Vec::new() }) };
        let resolver = Resolver { promise: promise.clone(), locked: Rc::new(Cell::new(false)) };
        (promise, resolver)
    }

    /// `a === b`: as duas referências são a mesma promessa.
    pub fn ptr_eq(&self, other: &Self) -> bool {
        Rc::ptr_eq(&self.state, &other.state)
    }

    /// `Promise.resolve(value)`.
    pub fn resolved(value: T) -> Self {
        Self { state: Shared::new(State::Settled(Ok(value))) }
    }

    /// `Promise.reject(error)`.
    pub fn rejected(error: Error) -> Self {
        let state = Shared::new(State::Settled(Err(error.clone())));
        note_rejection(&state, &error);
        Self { state }
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
        if let Err(error) = &result
            && !self.state.handled.get()
        {
            note_rejection(&self.state, error);
        }
        for reaction in reactions {
            let result = result.clone();
            queue_microtask(move || reaction(result));
        }
    }

    /// Registra uma reação; se já assentada, ela entra na fila de microtarefas agora.
    pub(crate) fn react(&self, reaction: impl FnOnce(Result<T, Error>) + 'static) {
        self.state.handled.set(true);
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

    /// `then(onFulfilled)` com tratador que devolve `T | Promise<T>`.
    pub fn then_maybe<U: Clone + 'static>(&self, on_fulfilled: impl FnOnce(T) -> MaybePromise<U> + 'static) -> Promise<U> {
        let (derived, resolver) = Promise::new();
        self.react(move |result| match result {
            Ok(v) => match errors::catch(move || on_fulfilled(v)) {
                Ok(next) => resolver.resolve_maybe(next),
                Err(e) => resolver.reject(e),
            },
            Err(e) => resolver.reject(e),
        });
        derived
    }

    /// `then(onFulfilled, onRejected)` com um tratador só, que vê o resultado e devolve `U | Promise<U>`.
    pub fn then_result_maybe<U: Clone + 'static>(&self, handler: impl FnOnce(Result<T, Error>) -> MaybePromise<U> + 'static) -> Promise<U> {
        let (derived, resolver) = Promise::new();
        self.react(move |result| match errors::catch(move || handler(result)) {
            Ok(next) => resolver.resolve_maybe(next),
            Err(e) => resolver.reject(e),
        });
        derived
    }

    /// `then(onFulfilled, onRejected)` com um tratador só, que vê o resultado e devolve uma promessa
    /// (a derivada segue a promessa devolvida).
    pub fn then_result_promise<U: Clone + 'static>(&self, handler: impl FnOnce(Result<T, Error>) -> Promise<U> + 'static) -> Promise<U> {
        self.then_result_maybe(move |r| MaybePromise::Promise(handler(r)))
    }

    /// `then(resolve, reject)` com as funções de resolução de outra promessa: o resultado passa para ela
    /// uma rodada depois de esta assentar.
    pub fn pipe_to(&self, resolver: &Resolver<T>) {
        let resolver = resolver.clone();
        self.react(move |result| match result {
            Ok(value) => resolver.resolve(value),
            Err(error) => resolver.reject(error),
        });
    }

    /// `Promise.all(promises)`: cada elemento registra a sua reação (uma rodada depois de assentar), a
    /// primeira rejeição rejeita tudo e o último a cumprir resolve com os valores na ordem da lista;
    /// lista vazia resolve já.
    pub fn all(promises: Vec<Promise<T>>) -> Promise<Vec<T>> {
        let (all, resolver) = Promise::new();
        if promises.is_empty() {
            resolver.resolve(Vec::new());
            return all;
        }
        let values: Rc<RefCell<Vec<Option<T>>>> = Rc::new(RefCell::new(vec![None; promises.len()]));
        let remaining = Rc::new(Cell::new(promises.len()));
        for (index, promise) in promises.iter().enumerate() {
            let (values, remaining, resolver) = (values.clone(), remaining.clone(), resolver.clone());
            promise.react(move |result| match result {
                Ok(value) => {
                    values.borrow_mut()[index] = Some(value);
                    remaining.set(remaining.get() - 1);
                    if remaining.get() == 0 {
                        resolver.resolve(values.borrow_mut().drain(..).flatten().collect());
                    }
                }
                Err(error) => resolver.reject(error),
            });
        }
        all
    }

    /// `Promise.race(promises)`: assenta como o primeiro a assentar (uma rodada depois dele). Lista vazia
    /// nunca assenta, como no JavaScript.
    pub fn race(promises: Vec<Promise<T>>) -> Promise<T> {
        let (race, resolver) = Promise::new();
        for promise in promises {
            let resolver = resolver.clone();
            promise.react(move |result| match result {
                Ok(value) => resolver.resolve(value),
                Err(error) => resolver.reject(error),
            });
        }
        race
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

    /// `finally(onFinally)`: o resultado passa adiante sem mudança. Como na especificação, o resultado
    /// volta por `Promise.resolve(onFinally()).then(() => resultado)` adotado pela derivada, o que custa
    /// duas rodadas a mais que um `then` (conferido no Node, `tests/runtime.rs`).
    pub fn finally(&self, on_finally: impl FnOnce() + 'static) -> Promise<T> {
        self.finally_with(move || {
            on_finally();
            Promise::resolved(())
        })
    }

    /// `finally(() => promessa)`: espera a promessa devolvida pelo tratador, e se ela rejeitar a derivada
    /// rejeita com esse erro.
    pub fn finally_with(&self, on_finally: impl FnOnce() -> Promise<()> + 'static) -> Promise<T> {
        let (derived, resolver) = Promise::new();
        self.react(move |result| match errors::catch(on_finally) {
            Ok(promise) => {
                let restored = promise.then_result(move |_| result);
                resolver.resolve_with(&restored);
            }
            Err(e) => resolver.reject(e),
        });
        derived
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

impl<T: Clone + 'static> From<Promise<T>> for CancelablePromise<T> {
    /// Uma promessa sem `cancel`: o `cancel?.()` do original, que não faz nada quando falta.
    fn from(promise: Promise<T>) -> Self {
        Self::new(promise, || {})
    }
}

/// `CancelablePromise<T>` (de `async.ts`): a promessa com o `cancel()` pendurado. Deref para a promessa,
/// então `then`, `catch` e `result` valem direto, e o `await` também.
pub struct CancelablePromise<T: Clone + 'static> {
    pub promise: Promise<T>,
    cancel: Rc<dyn Fn()>,
}

impl<T: Clone + 'static> Clone for CancelablePromise<T> {
    /// A cópia é a mesma promessa com o mesmo `cancel` (duas referências ao mesmo objeto no JS).
    fn clone(&self) -> Self {
        Self { promise: self.promise.clone(), cancel: self.cancel.clone() }
    }
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

    /// `DeferredPromise.fromPromise(promise)`.
    pub fn from_promise(promise: &Promise<T>) -> Self {
        let deferred = Self::new();
        deferred.settle_with(promise);
        deferred
    }

    /// `settleWith(promise)`: completa ou rejeita conforme a outra promessa.
    pub fn settle_with(&self, promise: &Promise<T>) -> Promise<()> {
        let resolver = self.resolver.clone();
        promise.then_result_promise(move |result| {
            match result {
                Ok(value) => resolver.resolve(value),
                Err(error) => resolver.reject(error),
            }
            Promise::resolved(())
        })
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
