//! O que o storage precisa de `src/vs/base/common/async.ts` (`timeout`, `Delayer`, `Throttler`,
//! `ThrottledDelayer`) e a ponte entre código `async` do Rust e as promessas de `je_base::promise`.
//!
//! O `je-base` ainda não porta o `async.ts`; estas peças seguem o original linha a linha e devem migrar
//! para lá quando o porte chegar. Só há atraso em milissegundos (o `MicrotaskDelay` não é usado pelo
//! storage), e o token de cancelamento que o `Throttler` passa à tarefa não existe aqui porque nenhuma
//! tarefa do storage o lê.

use std::cell::{Cell, RefCell};
use std::future::Future;
use std::rc::Rc;

use je_base::errors::{self, Error};
use je_base::lifecycle::IDisposable;
use je_base::promise::{Promise, Resolver};
use je_base::runtime::{self, TimerId};

/// Uma função `async` do TypeScript: o primeiro trecho roda já, de forma síncrona, e o resultado
/// (valor ou erro) assenta a promessa devolvida.
pub fn spawn_promise<T: Clone + 'static>(
    future: impl Future<Output = Result<T, Error>> + 'static,
) -> Promise<T> {
    let (promise, resolver) = Promise::new();
    runtime::spawn_local(async move {
        match future.await {
            Ok(value) => resolver.resolve(value),
            Err(error) => resolver.reject(error),
        }
    });
    promise
}

/// Um trabalho que o original entrega a uma thread do `libuv` (o `@vscode/sqlite3`, o `fs.promises`): o
/// resultado volta numa macrotarefa, depois das microtarefas do trecho que o pediu. As tarefas rodam
/// na ordem em que foram pedidas, como a fila de uma conexão do `sqlite3`.
pub fn in_macrotask<T: Clone + 'static>(
    work: impl FnOnce() -> Result<T, Error> + 'static,
) -> Promise<T> {
    Promise::with_executor(move |resolver| {
        runtime::set_timeout(
            move || match work() {
                Ok(value) => resolver.resolve(value),
                Err(error) => resolver.reject(error),
            },
            0,
        );
    })
}

/// `timeout(millis)` do `async.ts`, sem o cancelamento.
pub fn timeout(millis: u64) -> Promise<()> {
    Promise::with_executor(|resolver| {
        runtime::set_timeout(move || resolver.resolve(()), millis);
    })
}

type Factory<T> = Box<dyn FnOnce() -> Promise<T>>;

// ---- Throttler -------------------------------------------------------------------------------------

struct ThrottlerState<T: Clone + 'static> {
    active_promise: Option<Promise<T>>,
    queued_promise: Option<Promise<T>>,
    queued_promise_factory: Option<Factory<T>>,
    disposed: bool,
}

/// `Throttler`: só uma tarefa roda por vez, e as que chegam enquanto ela roda se fundem na última.
pub struct Throttler<T: Clone + Default + 'static> {
    state: Rc<RefCell<ThrottlerState<T>>>,
}

impl<T: Clone + Default + 'static> Default for Throttler<T> {
    fn default() -> Self {
        Self {
            state: Rc::new(RefCell::new(ThrottlerState {
                active_promise: None,
                queued_promise: None,
                queued_promise_factory: None,
                disposed: false,
            })),
        }
    }
}

fn queue_on<T: Clone + Default + 'static>(
    state: &Rc<RefCell<ThrottlerState<T>>>,
    promise_factory: Factory<T>,
) -> Promise<T> {
    if state.borrow().disposed {
        return Promise::rejected(Error::new("Throttler is disposed"));
    }
    let active = state.borrow().active_promise.clone();
    if let Some(active) = active {
        state.borrow_mut().queued_promise_factory = Some(promise_factory);
        if state.borrow().queued_promise.is_none() {
            let (queued, resolver) = Promise::new();
            let st = state.clone();
            // `activePromise.then(onComplete, onComplete)`: roda com sucesso ou com erro, e a promessa
            // derivada segue o que `onComplete` devolver.
            drop(active.then_result(move |_| {
                st.borrow_mut().queued_promise = None;
                if st.borrow().disposed {
                    resolver.resolve(T::default());
                    return Ok(());
                }
                let factory = st.borrow_mut().queued_promise_factory.take();
                match factory {
                    Some(factory) => resolver.resolve_with(&queue_on(&st, factory)),
                    None => resolver.resolve(T::default()),
                }
                Ok(())
            }));
            state.borrow_mut().queued_promise = Some(queued);
        }
        let queued = state
            .borrow()
            .queued_promise
            .clone()
            .expect("acabou de ser criada");
        return queued.then(|value| value);
    }

    let active = promise_factory();
    state.borrow_mut().active_promise = Some(active.clone());
    let st = state.clone();
    active.then_result(move |result| {
        st.borrow_mut().active_promise = None;
        result
    })
}

impl<T: Clone + Default + 'static> Throttler<T> {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn queue(&self, promise_factory: impl FnOnce() -> Promise<T> + 'static) -> Promise<T> {
        queue_on(&self.state, Box::new(promise_factory))
    }
}

impl<T: Clone + Default + 'static> IDisposable for Throttler<T> {
    fn dispose(&self) {
        let mut state = self.state.borrow_mut();
        state.disposed = true;
        state.queued_promise_factory = None;
    }
}

// ---- Delayer ---------------------------------------------------------------------------------------

struct DelayerState<T: Clone + 'static> {
    /// O timer pendente e o flag que o `isTriggered` do `timeoutDeferred` consulta.
    deferred: Option<(TimerId, Rc<Cell<bool>>)>,
    /// A promessa de conclusão e a geração em que nasceu (o `this.completionPromise !== completionPromise`).
    completion_promise: Option<(u64, Promise<T>)>,
    resolver: Option<Resolver<()>>,
    task: Option<Factory<T>>,
    generation: u64,
}

/// `Delayer`: adia uma tarefa pedida com frequência; cada `trigger` troca a tarefa e reinicia o prazo.
pub struct Delayer<T: Clone + 'static> {
    pub default_delay: u64,
    state: Rc<RefCell<DelayerState<T>>>,
}

impl<T: Clone + 'static> Delayer<T> {
    pub fn new(default_delay: u64) -> Self {
        Self {
            default_delay,
            state: Rc::new(RefCell::new(DelayerState {
                deferred: None,
                completion_promise: None,
                resolver: None,
                task: None,
                generation: 0,
            })),
        }
    }

    pub fn trigger(
        &self,
        task: impl FnOnce() -> Promise<T> + 'static,
        delay: Option<u64>,
    ) -> Promise<T> {
        let delay = delay.unwrap_or(self.default_delay);
        self.state.borrow_mut().task = Some(Box::new(task));
        self.cancel_timeout();

        if self.state.borrow().completion_promise.is_none() {
            let (gate, resolver) = Promise::<()>::new();
            let generation = {
                let mut state = self.state.borrow_mut();
                state.generation += 1;
                state.resolver = Some(resolver);
                state.generation
            };
            let state = self.state.clone();
            let completion_promise = gate.then_promise(move |()| {
                let task = {
                    let mut state = state.borrow_mut();
                    if state.completion_promise.as_ref().map(|(id, _)| *id) != Some(generation) {
                        // cancelado depois do prazo, possivelmente seguido de um novo trigger
                        drop(state);
                        errors::throw(Error::cancellation());
                    }
                    state.completion_promise = None;
                    state.resolver = None;
                    state.task.take().expect("a tarefa foi guardada no trigger")
                };
                task()
            });
            self.state.borrow_mut().completion_promise = Some((generation, completion_promise));
        }

        let scheduled = Rc::new(Cell::new(true));
        let state = self.state.clone();
        let flag = scheduled.clone();
        let timer = runtime::set_timeout(
            move || {
                flag.set(false);
                let resolver = {
                    let mut state = state.borrow_mut();
                    state.deferred = None;
                    state.resolver.clone()
                };
                if let Some(resolver) = resolver {
                    resolver.resolve(());
                }
            },
            delay,
        );
        let mut state = self.state.borrow_mut();
        state.deferred = Some((timer, scheduled));
        state
            .completion_promise
            .as_ref()
            .map(|(_, promise)| promise.clone())
            .expect("criada acima")
    }

    pub fn is_triggered(&self) -> bool {
        self.state
            .borrow()
            .deferred
            .as_ref()
            .is_some_and(|(_, scheduled)| scheduled.get())
    }

    pub fn cancel(&self) {
        self.cancel_timeout();
        let resolver = {
            let mut state = self.state.borrow_mut();
            state.task = None;
            let had_completion = state.completion_promise.take().is_some();
            if had_completion {
                state.resolver.clone()
            } else {
                None
            }
        };
        if let Some(resolver) = resolver {
            resolver.reject(Error::cancellation());
        }
    }

    fn cancel_timeout(&self) {
        let deferred = self.state.borrow_mut().deferred.take();
        if let Some((timer, scheduled)) = deferred {
            runtime::clear_timeout(timer);
            scheduled.set(false);
        }
    }
}

impl<T: Clone + 'static> IDisposable for Delayer<T> {
    fn dispose(&self) {
        self.cancel();
    }
}

// ---- ThrottledDelayer ------------------------------------------------------------------------------

/// `ThrottledDelayer`: o carteiro espera um tempo antes de sair, e enquanto ele está fora as cartas novas
/// só saem na próxima viagem.
pub struct ThrottledDelayer<T: Clone + Default + 'static> {
    delayer: Delayer<T>,
    throttler: Rc<Throttler<T>>,
}

impl<T: Clone + Default + 'static> ThrottledDelayer<T> {
    pub fn new(default_delay: u64) -> Self {
        Self {
            delayer: Delayer::new(default_delay),
            throttler: Rc::new(Throttler::new()),
        }
    }

    pub fn trigger(
        &self,
        promise_factory: impl FnOnce() -> Promise<T> + 'static,
        delay: Option<u64>,
    ) -> Promise<T> {
        let throttler = self.throttler.clone();
        self.delayer
            .trigger(move || throttler.queue(promise_factory), delay)
    }

    pub fn is_triggered(&self) -> bool {
        self.delayer.is_triggered()
    }

    pub fn cancel(&self) {
        self.delayer.cancel();
    }
}

impl<T: Clone + Default + 'static> IDisposable for ThrottledDelayer<T> {
    fn dispose(&self) {
        self.delayer.dispose();
        self.throttler.dispose();
    }
}
