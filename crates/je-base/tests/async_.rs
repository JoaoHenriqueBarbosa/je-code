//! Porte de `src/vs/base/test/common/async.test.ts` (suíte `Async`), caso a caso e na mesma ordem.
//!
//! Notas de tradução:
//! - Os testes `async` rodam em `common::run_with_faked_timers` (o `runWithFakedTimers` do original, com
//!   `await` sobre as `Promise` do `je_base::promise`); os que esperavam `setTimeout` real também, o que só
//!   encurta a espera, a ordem é a mesma. Os que já usavam `runWithFakedTimers` ou `sinon.useFakeTimers`
//!   usam o relógio virtual do mesmo jeito.
//! - Um `then(...)` com asserção dentro, que o original devolve ou espera, é esperado aqui com `unwrap`:
//!   uma asserção que falha rejeita a promessa derivada, e só o `unwrap` a transforma em falha do teste. Os
//!   poucos que o original deixa soltos (sem `await` nem `return`) são esperados também, o que só endurece o teste.
//! - `Throttler`, `Delayer`, `Queue` e as demais são genéricas no tipo do valor; os testes usam `i32`, `()`,
//!   `usize`, o que as tarefas devolvem no original.
//! - As tarefas são `FnOnce`: o que o original reaproveita (`factory`) vira um fecho `Clone` passado por cópia.
//! - `async function*` não existe em Rust: `AsyncGenerator` (abaixo) é uma emulação com `yield_` e `await`,
//!   que entrega um item por `next()` e só avança quando pedem o próximo, como o gerador do JS. O `finally`
//!   de um gerador interrompido por `return()` não roda (o corpo fica suspenso).
//! - `Object.assign(new Promise(...), { cancel })` vira `CancelablePromise::new(promise, cancel)`.
//! - Os campos privados que o original lê por `as unknown as { ... }` (`queuedPromiseFactory`, `task`) viram
//!   os ganchos `has_queued_factory`, `has_task` e `has_delayer_task`.
//! - `process.on('unhandledRejection')` vira `promise::set_unhandled_rejection_hook`, que o loop consulta
//!   quando as microtarefas acabam, como o Node.
//! - `sinon.useFakeTimers()` e `clock.tick`/`clock.setSystemTime` são `runtime::with_faked_timers`,
//!   `advance_fake_time` e `set_system_time`.
//! - `throw 1`/`Promise.reject(1)` (valores que não são `Error`) viram um `Error` com a mensagem `1`.
//! - `ResourceQueue` leva o `ExtUri` padrão no construtor (o `defaultExtUri` é um singleton no original).
//! - `Iterable.asyncToArray` é `async_::async_to_array`.
//! - Os dois `test.skip` do original (`tee - empty source`, `tee - handles errors in source`) ficam como
//!   `#[ignore]`, e o `waitThrottleDelayBetweenWorkUnits` comentado no original não é portado.

mod common;

use std::cell::{Cell, RefCell};
use std::collections::VecDeque;
use std::future::Future;
use std::ops::ControlFlow;
use std::pin::Pin;
use std::rc::Rc;

use common::{ensure_no_disposables_are_leaked_in_test_suite, run_with_faked_timers};
use je_base::async_::{
    self, AsyncIterable, AsyncIterableEmitter, AsyncIterableObject, AsyncIterableProducer, AsyncIterableSource, AsyncIterator, AsyncRead, AsyncReader,
    CancelablePromise, DeferredPromise, Delayer, Limiter, MaybePromise, Queue, ResourceQueue, Throttler, ThrottledWorkerOptions, for_await,
};
use je_base::cancellation::{CancellationToken, CancellationTokenSource};
use je_base::errors::{self, Error};
use je_base::lifecycle::{DisposableStore, IDisposable};
use je_base::path::{FixedEnvironment, Platform};
use je_base::promise::{self, Promise, Resolver};
use je_base::runtime;
use je_base::symbols::Delay;
use je_base::uri::URI;

type Shared<T> = Rc<RefCell<Vec<T>>>;

fn shared<T>() -> Shared<T> {
    Rc::new(RefCell::new(Vec::new()))
}

fn snapshot<T: Clone>(values: &Shared<T>) -> Vec<T> {
    values.borrow().clone()
}

/// Roda o corpo com o `ensureNoDisposablesAreLeakedInTestSuite()` e o relógio virtual. O `store` é o da suíte.
fn async_test<F: Future<Output = ()> + 'static>(body: impl FnOnce(Rc<DisposableStore>) -> F) {
    let ds = ensure_no_disposables_are_leaked_in_test_suite();
    let store = ds.store();
    run_with_faked_timers(body(store));
}

fn is_cancel(error: &Error) -> bool {
    errors::is_cancellation_error(error)
}

/// `await promise` de um teste: a rejeição vira falha.
async fn ok<T: Clone + 'static>(promise: Promise<T>) -> T {
    promise.await.expect("a promessa devia cumprir")
}

/// `async () => value`.
fn resolved_async<T: Clone + 'static>(value: T) -> Promise<T> {
    async_::async_fn(async move { Ok(value) })
}

// region: async function* (emulação)

type GeneratorBody<T> = Box<dyn FnOnce(Yielder<T>) -> Pin<Box<dyn Future<Output = Result<(), Error>>>>>;

struct GeneratorInner<T: Clone + 'static> {
    body: RefCell<Option<GeneratorBody<T>>>,
    requests: RefCell<VecDeque<Resolver<Option<T>>>>,
    resume: RefCell<Option<Resolver<()>>>,
    done: Cell<bool>,
    me: std::rc::Weak<GeneratorInner<T>>,
}

/// O `yield` do corpo do gerador.
struct Yielder<T: Clone + 'static> {
    inner: Rc<GeneratorInner<T>>,
}

impl<T: Clone + 'static> Yielder<T> {
    /// `yield value`: entrega o item ao `next()` mais antigo e suspende até o próximo `next()`.
    async fn yield_(&self, value: T) -> Result<(), Error> {
        let request = self.inner.requests.borrow_mut().pop_front().expect("o corpo só roda com um next() esperando");
        request.resolve(Some(value));
        if self.inner.requests.borrow().is_empty() {
            let (resumed, resolver) = Promise::<()>::new();
            *self.inner.resume.borrow_mut() = Some(resolver);
            resumed.await?;
        } else {
            Promise::resolved(()).await?;
        }
        Ok(())
    }
}

/// Um `async function*`: o corpo começa no primeiro `next()`.
struct AsyncGenerator<T: Clone + 'static> {
    inner: Rc<GeneratorInner<T>>,
}

impl<T: Clone + 'static> AsyncGenerator<T> {
    fn new<F: Future<Output = Result<(), Error>> + 'static>(body: impl FnOnce(Yielder<T>) -> F + 'static) -> Rc<Self> {
        let inner = Rc::new_cyclic(|me| GeneratorInner {
            body: RefCell::new(Some(Box::new(move |y| Box::pin(body(y)) as Pin<Box<dyn Future<Output = Result<(), Error>>>>))),
            requests: RefCell::new(VecDeque::new()),
            resume: RefCell::new(None),
            done: Cell::new(false),
            me: me.clone(),
        });
        Rc::new(Self { inner })
    }
}

impl<T: Clone + 'static> AsyncIterator<T> for AsyncGenerator<T> {
    fn next(&self) -> Promise<Option<T>> {
        let inner = &self.inner;
        let (promise, resolver) = Promise::new();
        if inner.done.get() {
            resolver.resolve(None);
            return promise;
        }
        inner.requests.borrow_mut().push_back(resolver);
        let body = inner.body.borrow_mut().take();
        if let Some(body) = body {
            let inner = inner.clone();
            runtime::spawn_local(async move {
                let outcome = body(Yielder { inner: inner.clone() }).await;
                inner.done.set(true);
                let mut pending = std::mem::take(&mut *inner.requests.borrow_mut());
                match (outcome, pending.pop_front()) {
                    (Err(error), Some(first)) => first.reject(error),
                    (Ok(()), Some(first)) => first.resolve(None),
                    (_, None) => {}
                }
                for rest in pending {
                    rest.resolve(None);
                }
            });
        } else {
            let resume = inner.resume.borrow_mut().take();
            if let Some(resume) = resume {
                resume.resolve(());
            }
        }
        promise
    }

    fn return_(&self) -> Option<Promise<()>> {
        self.inner.done.set(true);
        Some(Promise::resolved(()))
    }
}

impl<T: Clone + 'static> AsyncIterable<T> for AsyncGenerator<T> {
    fn async_iterator(&self) -> Rc<dyn AsyncIterator<T>> {
        let inner = self.inner.me.upgrade().expect("o gerador está vivo");
        Rc::new(AsyncGenerator { inner })
    }
}

/// O objeto `{ async *[Symbol.asyncIterator]() { ... } }`: cada iteração cria um gerador novo.
struct GeneratorIterable<T: Clone + 'static> {
    create: Box<dyn Fn() -> Rc<AsyncGenerator<T>>>,
}

impl<T: Clone + 'static> AsyncIterable<T> for GeneratorIterable<T> {
    fn async_iterator(&self) -> Rc<dyn AsyncIterator<T>> {
        (self.create)()
    }
}

/// `{ [Symbol.asyncIterator]: () => iterator }`.
struct FixedIterable<T: Clone + 'static> {
    iterator: Rc<dyn AsyncIterator<T>>,
}

impl<T: Clone + 'static> AsyncIterable<T> for FixedIterable<T> {
    fn async_iterator(&self) -> Rc<dyn AsyncIterator<T>> {
        self.iterator.clone()
    }
}

// endregion

/// Um fecho `Fn` que cria a tarefa de uma contagem: `() => Promise.resolve(++count)`.
fn counting_factory(count: &Rc<Cell<i32>>) -> impl Fn() -> MaybePromise<i32> + Clone + 'static {
    let count = count.clone();
    move || {
        count.set(count.get() + 1);
        MaybePromise::Promise(Promise::resolved(count.get()))
    }
}

mod cancelable_promise {
    use super::*;

    #[test]
    fn set_token_dont_wait_for_inner_promise() {
        async_test(|store| async move {
            let canceled = Rc::new(Cell::new(0));
            let promise = async_::create_cancelable_promise({
                let canceled = canceled.clone();
                move |token| {
                    store.add(token.on_cancellation_requested().listen(move |_| canceled.set(canceled.get() + 1)));
                    Promise::<()>::new().0 /*never*/
                }
            });
            let result = promise.then_result({
                let canceled = canceled.clone();
                move |outcome| {
                    let err = outcome.expect_err("devia rejeitar");
                    assert_eq!(canceled.get(), 1);
                    assert!(is_cancel(&err));
                    Ok(())
                }
            });
            promise.cancel();
            promise.cancel(); // cancel only once
            ok(result).await;
        });
    }

    #[test]
    fn cancel_despite_inner_promise_being_resolved() {
        async_test(|store| async move {
            let canceled = Rc::new(Cell::new(0));
            let promise = async_::create_cancelable_promise({
                let canceled = canceled.clone();
                move |token| {
                    store.add(token.on_cancellation_requested().listen(move |_| canceled.set(canceled.get() + 1)));
                    Promise::resolved(1234)
                }
            });
            let result = promise.then_result({
                let canceled = canceled.clone();
                move |outcome| {
                    let err = outcome.expect_err("devia rejeitar");
                    assert_eq!(canceled.get(), 1);
                    assert!(is_cancel(&err));
                    Ok(())
                }
            });
            promise.cancel();
            ok(result).await;
        });
    }

    #[test]
    fn cancel_disposes_result() {
        async_test(|_| async move {
            let store = DisposableStore::new();

            let promise = async_::create_cancelable_promise_disposing({
                let store = store.clone();
                move |_token| resolved_async(store)
            });
            let result = promise.then_result({
                let store = store.clone();
                move |outcome| {
                    let err = outcome.err().expect("devia rejeitar");
                    assert!(is_cancel(&err));
                    assert!(store.is_disposed());
                    Ok(())
                }
            });

            promise.cancel();
            ok(result).await;
        });
    }

    // Cancelling a sync cancelable promise will fire the cancelled token.
    // Also, every `then` callback runs in another execution frame.
    #[test]
    fn execution_order_sync() {
        async_test(|store| async move {
            let order = shared::<&str>();

            let cancellable_promise = async_::create_cancelable_promise({
                let order = order.clone();
                move |token| {
                    order.borrow_mut().push("in callback");
                    let order = order.clone();
                    store.add(token.on_cancellation_requested().listen(move |_| order.borrow_mut().push("cancelled")));
                    Promise::resolved(1234)
                }
            });

            order.borrow_mut().push("afterCreate");

            let promise = cancellable_promise.catch(|_| 0).then({
                let order = order.clone();
                move |_| order.borrow_mut().push("finally")
            });

            cancellable_promise.cancel();
            order.borrow_mut().push("afterCancel");

            ok(promise).await;
            assert_eq!(snapshot(&order), ["in callback", "afterCreate", "cancelled", "afterCancel", "finally"]);
        });
    }

    // Cancelling an async cancelable promise is just the same as a sync cancellable promise.
    #[test]
    fn execution_order_async() {
        async_test(|store| async move {
            let order = shared::<&str>();

            let cancellable_promise = async_::create_cancelable_promise({
                let order = order.clone();
                move |token| {
                    order.borrow_mut().push("in callback");
                    let order = order.clone();
                    store.add(token.on_cancellation_requested().listen(move |_| order.borrow_mut().push("cancelled")));
                    Promise::with_executor(|resolver| {
                        runtime::set_timeout(move || resolver.resolve(()), 0);
                    })
                }
            });

            order.borrow_mut().push("afterCreate");

            let promise = cancellable_promise.catch(|_| ()).then({
                let order = order.clone();
                move |_| order.borrow_mut().push("finally")
            });

            cancellable_promise.cancel();
            order.borrow_mut().push("afterCancel");

            ok(promise).await;
            assert_eq!(snapshot(&order), ["in callback", "afterCreate", "cancelled", "afterCancel", "finally"]);
        });
    }

    #[test]
    fn execution_order_async_with_late_listener() {
        async_test(|store| async move {
            let order = shared::<&str>();

            let cancellable_slot: Rc<RefCell<Option<CancelablePromise<()>>>> = Rc::default();
            let cancellable_promise = async_::create_cancelable_promise({
                let (order, cancellable_slot) = (order.clone(), cancellable_slot.clone());
                move |token| {
                    async_::async_fn(async move {
                        order.borrow_mut().push("in callback");

                        async_::timeout(0).await?;
                        let listener_order = order.clone();
                        store.add(token.on_cancellation_requested().listen(move |_| listener_order.borrow_mut().push("cancelled")));
                        let cancellable = cancellable_slot.borrow().clone().expect("a promessa cancelável");
                        cancellable.cancel();
                        order.borrow_mut().push("afterCancel");
                        Ok(())
                    })
                }
            });
            *cancellable_slot.borrow_mut() = Some(cancellable_promise.clone());

            order.borrow_mut().push("afterCreate");

            let promise = cancellable_promise.catch(|_| ()).then({
                let order = order.clone();
                move |_| order.borrow_mut().push("finally")
            });

            ok(promise).await;
            assert_eq!(snapshot(&order), ["in callback", "afterCreate", "cancelled", "afterCancel", "finally"]);
        });
    }

    #[test]
    fn get_inner_result() {
        async_test(|_| async move {
            let promise = async_::create_cancelable_promise(|_token| async_::timeout(12).then(|_| 1234));

            let result = ok(promise.promise.clone()).await;
            assert_eq!(result, 1234);
        });
    }
}

mod race_cancellable_promises {
    use super::*;

    #[test]
    fn preserves_the_result_and_cancels_only_the_losing_promises() {
        async_test(|_| async move {
            let winner_cancellations = Rc::new(Cell::new(0));
            let loser_cancellations = Rc::new(Cell::new(0));
            let (winner_promise, resolve_winner) = Promise::<i32>::new();
            let winner = CancelablePromise::new(winner_promise, {
                let winner_cancellations = winner_cancellations.clone();
                move || winner_cancellations.set(winner_cancellations.get() + 1)
            });
            let loser = CancelablePromise::new(Promise::<i32>::new().0, {
                let loser_cancellations = loser_cancellations.clone();
                move || loser_cancellations.set(loser_cancellations.get() + 1)
            });
            let race = async_::race_cancellable_promises(vec![winner, loser]);

            resolve_winner.resolve(42);

            let result = ok(race.promise.clone()).await;
            assert_eq!((result, winner_cancellations.get(), loser_cancellations.get()), (42, 0, 1));
        });
    }

    #[test]
    fn preserves_the_error_cancels_all_promises_and_handles_cleanup_rejection() {
        async_test(|_| async move {
            let expected_error = Error::new("expected");
            let rejecting_promise_cancellations = Rc::new(Cell::new(0));
            let pending_promise_cancellations = Rc::new(Cell::new(0));
            let unhandled_rejections = shared::<Error>();
            promise::set_unhandled_rejection_hook(Some({
                let unhandled_rejections = unhandled_rejections.clone();
                move |reason: &Error| unhandled_rejections.borrow_mut().push(reason.clone())
            }));
            let rejecting_promise = CancelablePromise::new(Promise::<()>::rejected(expected_error.clone()), {
                let count = rejecting_promise_cancellations.clone();
                move || count.set(count.get() + 1)
            });
            let pending_promise = CancelablePromise::new(Promise::<()>::new().0, {
                let count = pending_promise_cancellations.clone();
                move || count.set(count.get() + 1)
            });

            let actual_error = async_::race_cancellable_promises(vec![rejecting_promise, pending_promise]).promise.clone().await.expect_err("devia rejeitar");
            async_::timeout(0).await.expect("timeout");
            promise::set_unhandled_rejection_hook(None::<fn(&Error)>);
            assert_eq!(
                (actual_error == expected_error, rejecting_promise_cancellations.get(), pending_promise_cancellations.get(), snapshot(&unhandled_rejections).len()),
                (true, 1, 1, 0)
            );
        });
    }

    #[test]
    fn explicit_cancellation_cancels_all_pending_promises() {
        async_test(|store| async move {
            let cancellation_counts: Vec<Rc<Cell<i32>>> = vec![Rc::new(Cell::new(0)), Rc::new(Cell::new(0))];
            let promises: Vec<CancelablePromise<()>> = cancellation_counts
                .iter()
                .map(|count| {
                    let (count, store) = (count.clone(), store.clone());
                    async_::create_cancelable_promise(move |token| {
                        store.add(token.on_cancellation_requested().listen(move |_| count.set(count.get() + 1)));
                        Promise::<()>::new().0
                    })
                })
                .collect();
            let race = async_::race_cancellable_promises(promises);

            race.cancel();
            let cancellation_error = race.promise.clone().await.expect_err("devia rejeitar");

            assert!(is_cancel(&cancellation_error));
            assert_eq!(cancellation_counts.iter().map(|count| count.get()).collect::<Vec<_>>(), [1, 1]);
        });
    }
}

mod throttler {
    use super::*;

    /// `() => Promise.resolve(++count)` como fábrica do `Throttler` (ignora o token).
    fn count_factory(count: &Rc<Cell<i32>>) -> impl Fn(CancellationToken) -> Promise<i32> + Clone + 'static {
        let count = count.clone();
        move |_| {
            count.set(count.get() + 1);
            Promise::resolved(count.get())
        }
    }

    fn expect_result(promise: Promise<i32>, expected: i32) -> Promise<()> {
        promise.then(move |result| assert_eq!(result, expected))
    }

    #[test]
    fn non_async() {
        async_test(|_| async move {
            let count = Rc::new(Cell::new(0));
            let factory = count_factory(&count);

            let throttler = Throttler::<i32>::new();

            ok(Promise::all(vec![
                expect_result(throttler.queue(factory.clone()), 1),
                expect_result(throttler.queue(factory.clone()), 2),
                expect_result(throttler.queue(factory.clone()), 2),
                expect_result(throttler.queue(factory.clone()), 2),
                expect_result(throttler.queue(factory.clone()), 2),
            ]))
            .await;
            assert_eq!(count.get(), 2);
        });
    }

    #[test]
    fn async_() {
        async_test(|_| async move {
            let count = Rc::new(Cell::new(0));
            let factory = {
                let count = count.clone();
                move |_: CancellationToken| {
                    let count = count.clone();
                    async_::timeout(0).then(move |_| {
                        count.set(count.get() + 1);
                        count.get()
                    })
                }
            };

            let throttler = Throttler::<i32>::new();

            ok(Promise::all(vec![
                expect_result(throttler.queue(factory.clone()), 1),
                expect_result(throttler.queue(factory.clone()), 2),
                expect_result(throttler.queue(factory.clone()), 2),
                expect_result(throttler.queue(factory.clone()), 2),
                expect_result(throttler.queue(factory.clone()), 2),
            ]))
            .await;
            ok(Promise::all(vec![
                expect_result(throttler.queue(factory.clone()), 3),
                expect_result(throttler.queue(factory.clone()), 4),
                expect_result(throttler.queue(factory.clone()), 4),
                expect_result(throttler.queue(factory.clone()), 4),
                expect_result(throttler.queue(factory.clone()), 4),
            ]))
            .await;
        });
    }

    #[test]
    fn last_factory_should_be_the_one_getting_called() {
        async_test(|_| async move {
            let factory_factory = |n: i32| move |_: CancellationToken| async_::timeout(0).then(move |_| n);

            let throttler = Throttler::<i32>::new();

            let promises = vec![
                expect_result(throttler.queue(factory_factory(1)), 1),
                expect_result(throttler.queue(factory_factory(2)), 3),
                expect_result(throttler.queue(factory_factory(3)), 3),
            ];

            ok(Promise::all(promises)).await;
        });
    }

    fn propagates_queued_errors_after_the_active_task(active_rejects: bool) {
        async_test(|store| async move {
            let throttler = store.add_rc(Rc::new(Throttler::<i32>::new()));
            let active_task = Rc::new(DeferredPromise::<i32>::new());
            let active_error = Error::new("Active task failed");
            let queued_error = Error::new("Queued task failed");
            let queued_calls = Rc::new(Cell::new(0));
            let factory = {
                let (queued_calls, queued_error) = (queued_calls.clone(), queued_error.clone());
                move |_: CancellationToken| {
                    let (queued_calls, queued_error) = (queued_calls.clone(), queued_error.clone());
                    async_::async_fn::<i32>(async move {
                        queued_calls.set(queued_calls.get() + 1);
                        Err(queued_error)
                    })
                }
            };
            let active = active_task.promise.clone();
            let results = Promise::all_settled(vec![throttler.queue(move |_| active), throttler.queue(factory.clone()), throttler.queue(factory.clone())]);

            if active_rejects {
                active_task.error(active_error.clone());
            } else {
                active_task.complete(1);
            }

            let settled = ok(results).await;
            let recovered = ok(throttler.queue(|_| resolved_async(2))).await;
            assert_eq!(
                (settled, queued_calls.get(), recovered),
                (
                    vec![if active_rejects { Err(active_error) } else { Ok(1) }, Err(queued_error.clone()), Err(queued_error)],
                    1,
                    2
                )
            );
        });
    }

    #[test]
    fn propagates_queued_errors_after_the_active_task_succeeds() {
        propagates_queued_errors_after_the_active_task(false);
    }

    #[test]
    fn propagates_queued_errors_after_the_active_task_fails() {
        propagates_queued_errors_after_the_active_task(true);
    }

    #[test]
    fn disposal_after_queueing() {
        async_test(|_| async move {
            let factory_calls = Rc::new(Cell::new(0));
            let factory = {
                let factory_calls = factory_calls.clone();
                move |_: CancellationToken| {
                    let factory_calls = factory_calls.clone();
                    async_::async_fn_maybe(async move {
                        factory_calls.set(factory_calls.get() + 1);
                        Ok(MaybePromise::Promise(async_::timeout(0).promise.clone()))
                    })
                }
            };

            let throttler = Throttler::<()>::new();
            let promises = vec![throttler.queue(factory.clone()), throttler.queue(factory.clone())];
            throttler.dispose();

            ok(Promise::all(promises)).await;
            assert_eq!(factory_calls.get(), 1);
        });
    }

    #[test]
    fn disposal_before_queueing() {
        async_test(|_| async move {
            let factory_calls = Rc::new(Cell::new(0));
            let factory = {
                let factory_calls = factory_calls.clone();
                move |_: CancellationToken| {
                    let factory_calls = factory_calls.clone();
                    async_::async_fn_maybe(async move {
                        factory_calls.set(factory_calls.get() + 1);
                        Ok(MaybePromise::Promise(async_::timeout(0).promise.clone()))
                    })
                }
            };

            let throttler = Throttler::<()>::new();

            throttler.dispose();
            let promises = vec![throttler.queue(factory)];

            assert!(Promise::all(promises).await.is_err(), "should fail");
            assert_eq!(factory_calls.get(), 0);
        });
    }

    #[test]
    fn disposal_releases_the_queued_factory() {
        async_test(|_| async move {
            let throttler = Throttler::<()>::new();
            let active_task = DeferredPromise::<()>::new();
            let queued_calls = Rc::new(Cell::new(0));

            let active_promise = active_task.promise.clone();
            let active = throttler.queue(move |_| active_promise);
            let queued = throttler.queue({
                let queued_calls = queued_calls.clone();
                move |_| {
                    async_::async_fn(async move {
                        queued_calls.set(queued_calls.get() + 1);
                        Ok(())
                    })
                }
            });
            throttler.dispose();
            let retained_after_dispose = throttler.has_queued_factory();

            active_task.complete(());
            ok(Promise::all(vec![active, queued])).await;
            assert_eq!((retained_after_dispose, queued_calls.get()), (false, 0));
        });
    }
}

mod delayer {
    use super::*;

    fn delayer_of<T: Clone + 'static>(delay: Delay) -> Rc<Delayer<T>> {
        Rc::new(Delayer::new(delay))
    }

    /// O que cada `trigger` do teste confere ao assentar: o resultado e que o delayer já não está disparado.
    fn expect_result_and_not_triggered(promise: Promise<i32>, delayer: &Rc<Delayer<i32>>, expected: i32) -> Promise<()> {
        let delayer = delayer.clone();
        promise.then(move |result| {
            assert_eq!(result, expected);
            assert!(!delayer.is_triggered());
        })
    }

    fn simple(delay: Delay) {
        async_test(|_| async move {
            let count = Rc::new(Cell::new(0));
            let factory = counting_factory(&count);

            let delayer = delayer_of::<i32>(delay);
            let mut promises = Vec::new();

            assert!(!delayer.is_triggered());

            promises.push(expect_result_and_not_triggered(delayer.trigger(factory.clone(), None), &delayer, 1));
            assert!(delayer.is_triggered());

            promises.push(expect_result_and_not_triggered(delayer.trigger(factory.clone(), None), &delayer, 1));
            assert!(delayer.is_triggered());

            promises.push(expect_result_and_not_triggered(delayer.trigger(factory.clone(), None), &delayer, 1));
            assert!(delayer.is_triggered());

            ok(Promise::all(promises)).await;
            assert!(!delayer.is_triggered());
        });
    }

    #[test]
    fn simple_() {
        simple(Delay::Millis(0));
    }

    #[test]
    fn microtask_delay_simple() {
        simple(Delay::Microtask);
    }

    mod throttled_delayer {
        use super::*;

        #[test]
        fn promise_should_resolve_if_disposed() {
            async_test(|_| async move {
                let throttled_delayer = async_::ThrottledDelayer::<()>::new(100);
                let promise = throttled_delayer.trigger(|_| resolved_async(()), Some(0));
                throttled_delayer.dispose();

                // O original espera a promessa dentro de um `try` que também captura o `assert.fail`: o teste
                // só exige que a espera termine, cumprida ou rejeitada.
                let _ = promise.await;
            });
        }

        #[test]
        fn trigger_after_dispose_throws() {
            async_test(|_| async move {
                let throttled_delayer = async_::ThrottledDelayer::<()>::new(100);
                throttled_delayer.dispose();
                assert!(throttled_delayer.trigger(|_| resolved_async(()), Some(0)).await.is_err());
            });
        }

        #[test]
        fn cancel_releases_the_pending_task() {
            async_test(|store| async move {
                let throttled_delayer = store.add_rc(Rc::new(async_::ThrottledDelayer::<i32>::new(0)));
                let task_calls = Rc::new(Cell::new(0));

                let canceled = throttled_delayer.trigger(
                    {
                        let task_calls = task_calls.clone();
                        move |_| {
                            async_::async_fn(async move {
                                task_calls.set(task_calls.get() + 1);
                                Ok(task_calls.get())
                            })
                        }
                    },
                    None,
                );
                throttled_delayer.cancel();
                let retained_after_cancel = throttled_delayer.has_delayer_task();

                let canceled_with_cancellation_error = ok(canceled.then_result(|outcome| Ok(outcome.err().is_some_and(|e| is_cancel(&e))))).await;
                let result = ok(throttled_delayer.trigger(|_| resolved_async(42), None)).await;
                assert_eq!((retained_after_cancel, canceled_with_cancellation_error, task_calls.get(), result), (false, true, 0, 42));
            });
        }
    }

    #[test]
    fn simple_cancel() {
        async_test(|_| async move {
            let count = Rc::new(Cell::new(0));
            let factory = counting_factory(&count);

            let delayer = delayer_of::<i32>(Delay::Millis(0));

            assert!(!delayer.is_triggered());

            let p = delayer.trigger(factory, None).then_result(|outcome| {
                assert!(outcome.is_err(), "yes, it was cancelled");
                Ok(())
            });

            assert!(delayer.is_triggered());
            delayer.cancel();
            assert!(!delayer.is_triggered());

            ok(p).await;
        });
    }

    #[test]
    fn simple_cancel_microtask() {
        async_test(|_| async move {
            let count = Rc::new(Cell::new(0));
            let factory = counting_factory(&count);

            let delayer = delayer_of::<i32>(Delay::Microtask);

            assert!(!delayer.is_triggered());

            let p = delayer.trigger(factory, None).then_result(|outcome| {
                assert!(outcome.is_err(), "yes, it was cancelled");
                Ok(())
            });

            assert!(delayer.is_triggered());
            delayer.cancel();
            assert!(!delayer.is_triggered());

            ok(p).await;
        });
    }

    #[test]
    fn cancel_should_cancel_all_calls_to_trigger() {
        async_test(|_| async move {
            let count = Rc::new(Cell::new(0));
            let factory = counting_factory(&count);

            let delayer = delayer_of::<i32>(Delay::Millis(0));
            let mut promises = Vec::new();

            assert!(!delayer.is_triggered());

            promises.push(delayer.trigger(factory.clone(), None).catch(|_| 0));
            assert!(delayer.is_triggered());

            promises.push(delayer.trigger(factory.clone(), None).catch(|_| 0));
            assert!(delayer.is_triggered());

            promises.push(delayer.trigger(factory.clone(), None).catch(|_| 0));
            assert!(delayer.is_triggered());

            delayer.cancel();

            ok(Promise::all(promises)).await;
            assert!(!delayer.is_triggered());
        });
    }

    #[test]
    fn trigger_cancel_then_trigger_again() {
        async_test(|_| async move {
            let count = Rc::new(Cell::new(0));
            let factory = counting_factory(&count);

            let delayer = delayer_of::<i32>(Delay::Millis(0));

            assert!(!delayer.is_triggered());

            let p = delayer.trigger(factory.clone(), None).then_promise({
                let (delayer, factory) = (delayer.clone(), factory.clone());
                move |result| {
                    assert_eq!(result, 1);
                    assert!(!delayer.is_triggered());

                    let mut promises = Vec::new();
                    promises.push(delayer.trigger(factory.clone(), None).catch(|_| 0));
                    assert!(delayer.is_triggered());

                    promises.push(delayer.trigger(factory.clone(), None).catch(|_| 0));
                    assert!(delayer.is_triggered());

                    delayer.cancel();

                    let (delayer, factory) = (delayer.clone(), factory.clone());
                    Promise::all(promises).then_promise(move |_| {
                        assert!(!delayer.is_triggered());

                        let mut promises = Vec::new();
                        let check = |delayer: &Rc<Delayer<i32>>| {
                            let delayer = delayer.clone();
                            move |_: i32| {
                                assert_eq!(result, 1);
                                assert!(!delayer.is_triggered());
                            }
                        };
                        promises.push(delayer.trigger(factory.clone(), None).then(check(&delayer)));
                        assert!(delayer.is_triggered());

                        promises.push(delayer.trigger(factory.clone(), None).then(check(&delayer)));
                        assert!(delayer.is_triggered());

                        let p = Promise::all(promises).then({
                            let delayer = delayer.clone();
                            move |_| assert!(!delayer.is_triggered())
                        });

                        assert!(delayer.is_triggered());

                        p
                    })
                }
            });

            assert!(delayer.is_triggered());

            ok(p).await;
        });
    }

    #[test]
    fn last_task_should_be_the_one_getting_called() {
        async_test(|_| async move {
            let factory_factory = |n: i32| move || MaybePromise::Promise(Promise::resolved(n));

            let delayer = delayer_of::<i32>(Delay::Millis(0));
            let mut promises = Vec::new();

            assert!(!delayer.is_triggered());

            promises.push(delayer.trigger(factory_factory(1), None).then(|n| assert_eq!(n, 3)));
            promises.push(delayer.trigger(factory_factory(2), None).then(|n| assert_eq!(n, 3)));
            promises.push(delayer.trigger(factory_factory(3), None).then(|n| assert_eq!(n, 3)));

            let p = Promise::all(promises).then({
                let delayer = delayer.clone();
                move |_| assert!(!delayer.is_triggered())
            });

            assert!(delayer.is_triggered());

            ok(p).await;
        });
    }

    fn releases_the_pending_task(release: &'static str) {
        async_test(|store| async move {
            let delayer = store.add_rc(delayer_of::<i32>(Delay::Millis(0)));
            let task_calls = Rc::new(Cell::new(0));

            let canceled = delayer.trigger(
                {
                    let task_calls = task_calls.clone();
                    move || {
                        task_calls.set(task_calls.get() + 1);
                        MaybePromise::Value(task_calls.get())
                    }
                },
                None,
            );
            match release {
                "cancel" => delayer.cancel(),
                _ => delayer.dispose(),
            }
            let retained_after_release = delayer.has_task();

            let canceled_with_cancellation_error = ok(canceled.then_result(|outcome| Ok(outcome.err().is_some_and(|e| is_cancel(&e))))).await;
            let result = ok(delayer.trigger(|| MaybePromise::Value(42), None)).await;
            assert_eq!((retained_after_release, canceled_with_cancellation_error, task_calls.get(), result), (false, true, 0, 42));
        });
    }

    #[test]
    fn cancel_releases_the_pending_task() {
        releases_the_pending_task("cancel");
    }

    #[test]
    fn dispose_releases_the_pending_task() {
        releases_the_pending_task("dispose");
    }

    #[test]
    fn cancel_after_the_delay_elapsed_does_not_run_the_task() {
        async_test(|store| async move {
            let delayer = store.add_rc(delayer_of::<i32>(Delay::Microtask));
            let task_calls = Rc::new(Cell::new(0));

            let canceled = delayer.trigger(
                {
                    let task_calls = task_calls.clone();
                    move || {
                        task_calls.set(task_calls.get() + 1);
                        MaybePromise::Value(task_calls.get())
                    }
                },
                None,
            );
            // runs after the delay elapsed but before the task is invoked
            runtime::queue_microtask({
                let delayer = delayer.clone();
                move || delayer.cancel()
            });

            let canceled_with_cancellation_error = ok(canceled.then_result(|outcome| Ok(outcome.err().is_some_and(|e| is_cancel(&e))))).await;
            assert_eq!((canceled_with_cancellation_error, task_calls.get()), (true, 0));
        });
    }

    #[test]
    fn cancel_and_trigger_after_the_delay_elapsed_settles_both_triggers() {
        async_test(|store| async move {
            let delayer = store.add_rc(delayer_of::<i32>(Delay::Microtask));
            let calls = shared::<&str>();

            let canceled = delayer.trigger(
                {
                    let calls = calls.clone();
                    move || {
                        calls.borrow_mut().push("canceled");
                        MaybePromise::Value(1)
                    }
                },
                None,
            );
            let retriggered: Rc<RefCell<Option<Promise<i32>>>> = Rc::default();
            // runs after the delay elapsed but before the canceled task is invoked
            runtime::queue_microtask({
                let (delayer, calls, retriggered) = (delayer.clone(), calls.clone(), retriggered.clone());
                move || {
                    delayer.cancel();
                    *retriggered.borrow_mut() = Some(delayer.trigger(
                        move || {
                            calls.borrow_mut().push("retriggered");
                            MaybePromise::Value(2)
                        },
                        None,
                    ));
                }
            });

            let canceled_result = ok(canceled.then_result(|outcome| {
                Ok(match outcome {
                    Ok(value) => Some(value),
                    Err(error) => {
                        assert!(is_cancel(&error));
                        None
                    }
                })
            }))
            .await;
            let still_pending = async_::timeout(10);
            let retriggered_promise = retriggered.borrow().clone().expect("o segundo trigger");
            // 'pending' do original é -1 aqui, para manter os dois lados da corrida com o mesmo tipo
            let retriggered_result = ok(Promise::race(vec![retriggered_promise, still_pending.then(|_| -1)])).await;
            still_pending.cancel();
            assert_eq!((canceled_result, retriggered_result, snapshot(&calls)), (None, 2, vec!["retriggered"]));
        });
    }
}

mod sequence {
    use super::*;

    #[test]
    fn simple() {
        async_test(|_| async move {
            let factory_factory = |n: i32| -> Box<dyn FnOnce() -> Promise<Option<i32>>> { Box::new(move || Promise::resolved(Some(n))) };

            let result = ok(async_::sequence(vec![factory_factory(1), factory_factory(2), factory_factory(3), factory_factory(4), factory_factory(5)])).await;
            assert_eq!(5, result.len());
            assert_eq!(1, result[0]);
            assert_eq!(2, result[1]);
            assert_eq!(3, result[2]);
            assert_eq!(4, result[3]);
            assert_eq!(5, result[4]);
        });
    }
}

mod limiter {
    use super::*;

    #[test]
    fn assert_degree_of_paralellism() {
        async_test(|_| async move {
            let active_promises = Rc::new(Cell::new(0));
            let factory_factory = |n: i32| {
                let active_promises = active_promises.clone();
                move || {
                    active_promises.set(active_promises.get() + 1);
                    assert!(active_promises.get() < 6);
                    async_::timeout(0).then(move |_| {
                        active_promises.set(active_promises.get() - 1);
                        n
                    })
                }
            };

            let limiter = Limiter::<i32>::new(5);

            let promises: Vec<Promise<i32>> = (0..10).map(|n| limiter.queue(factory_factory(n))).collect();

            let res = ok(Promise::all(promises)).await;
            assert_eq!(10, res.len());
            assert_eq!(vec![0, 1, 2, 3, 4, 5, 6, 7, 8, 9], res);
        });
    }
}

mod queue {
    use super::*;

    /// `res.push(n)` devolvendo o novo tamanho, como `Array.push`.
    fn push_n(res: &Shared<i32>, n: i32) -> impl FnOnce(()) -> usize + 'static {
        let res = res.clone();
        move |_| {
            res.borrow_mut().push(n);
            res.borrow().len()
        }
    }

    /// `() => Promise.resolve(true).then(() => res.push(n))`.
    fn resolved_then(res: &Shared<i32>, n: i32) -> impl FnOnce() -> Promise<usize> + 'static {
        let res = res.clone();
        move || Promise::resolved(()).then(push_n(&res, n))
    }

    /// `() => async.timeout(ms).then(() => res.push(n))`.
    fn timeout_then(res: &Shared<i32>, ms: u64, n: i32) -> impl FnOnce() -> Promise<usize> + 'static {
        let res = res.clone();
        move || async_::timeout(ms).then(push_n(&res, n))
    }

    /// `async () => { await timeout(0); workCounter++; queue.<action>(); }`.
    fn counting_task(queue: &Queue<()>, work_counter: &Rc<Cell<i32>>, action: impl Fn(&Queue<()>) + Clone + 'static) -> impl Fn() -> Promise<()> + Clone + 'static {
        let (queue, work_counter) = (queue.clone(), work_counter.clone());
        move || {
            let (queue, work_counter, action) = (queue.clone(), work_counter.clone(), action.clone());
            async_::async_fn(async move {
                async_::timeout(0).await?;
                work_counter.set(work_counter.get() + 1);
                action(&queue);
                Ok(())
            })
        }
    }

    #[test]
    fn simple() {
        async_test(|_| async move {
            let queue = Queue::<bool>::new();

            let sync_promise = Rc::new(Cell::new(false));
            let f1 = {
                let sync_promise = sync_promise.clone();
                move || {
                    let sync_promise = sync_promise.clone();
                    Promise::resolved(true).then(move |_| {
                        sync_promise.set(true);
                        true
                    })
                }
            };

            let async_promise = Rc::new(Cell::new(false));
            let f2 = {
                let async_promise = async_promise.clone();
                move || {
                    let async_promise = async_promise.clone();
                    async_::timeout(10).then(move |_| {
                        async_promise.set(true);
                        true
                    })
                }
            };

            assert_eq!(queue.size(), 0);

            queue.queue(f1);
            assert_eq!(queue.size(), 1);

            let p = queue.queue(f2);
            assert_eq!(queue.size(), 2);
            ok(p).await;
            assert_eq!(queue.size(), 0);
            assert!(sync_promise.get());
            assert!(async_promise.get());
        });
    }

    #[test]
    fn stop_processing_on_dispose() {
        async_test(|_| async move {
            let queue = Queue::<()>::new();

            let work_counter = Rc::new(Cell::new(0));
            let task = counting_task(&queue, &work_counter, |queue| queue.dispose() /* DISPOSE HERE */);

            let p1 = queue.queue(task.clone());
            queue.queue(task.clone());
            queue.queue(task.clone());
            assert_eq!(queue.size(), 3);

            ok(p1).await;

            assert_eq!(work_counter.get(), 1);
        });
    }

    #[test]
    fn stop_on_clear() {
        async_test(|_| async move {
            let queue = Queue::<()>::new();

            let work_counter = Rc::new(Cell::new(0));
            let task = counting_task(&queue, &work_counter, |queue| {
                queue.clear(); // CLEAR HERE
                assert_eq!(queue.size(), 1); // THIS task is still running
            });

            let p1 = queue.queue(task.clone());
            queue.queue(task.clone());
            queue.queue(task.clone());
            assert_eq!(queue.size(), 3);

            ok(p1).await;
            assert_eq!(work_counter.get(), 1);
            assert_eq!(queue.size(), 0); // has been cleared

            let p2 = queue.queue(task.clone());
            ok(p2).await;
            assert_eq!(work_counter.get(), 2);
        });
    }

    #[test]
    fn clear_and_drain_1() {
        async_test(|_| async move {
            let queue = Queue::<()>::new();

            let work_counter = Rc::new(Cell::new(0));
            let task = counting_task(&queue, &work_counter, |queue| queue.clear() /* CLEAR HERE */);

            let p0 = queue.on_drained().to_promise(None);
            let p1 = queue.queue(task);

            ok(p1).await;
            ok(p0.promise.clone()).await; // expect drain to fire because a task was running
            assert_eq!(work_counter.get(), 1);
            queue.dispose();
        });
    }

    #[test]
    fn clear_and_drain_2() {
        async_test(|_| async move {
            let queue = Queue::<()>::new();

            let did_fire = Rc::new(Cell::new(false));
            let d = queue.on_drained().listen({
                let did_fire = did_fire.clone();
                move |_| did_fire.set(true)
            });

            queue.clear();

            assert!(!did_fire.get()); // no work, no drain!
            d.dispose();
            queue.dispose();
        });
    }

    #[test]
    fn drain_timing() {
        async_test(|_| async move {
            let queue = Queue::<()>::new();

            let logic_clock = Rc::new(Cell::new(0));
            let tick = {
                let logic_clock = logic_clock.clone();
                move || {
                    let time = logic_clock.get();
                    logic_clock.set(time + 1);
                    time
                }
            };

            let did_drain_time = Rc::new(Cell::new(0));
            let did_finish_time1 = Rc::new(Cell::new(0));
            let did_finish_time2 = Rc::new(Cell::new(0));
            let d = queue.on_drained().listen({
                let (did_drain_time, tick) = (did_drain_time.clone(), tick.clone());
                move |_| did_drain_time.set(tick())
            });

            let p1 = queue.queue({
                let (did_finish_time1, tick) = (did_finish_time1.clone(), tick.clone());
                move || {
                    // await async.timeout(10);
                    did_finish_time1.set(tick());
                    Promise::resolved(())
                }
            });

            let p2 = queue.queue({
                let (did_finish_time2, tick) = (did_finish_time2.clone(), tick.clone());
                move || {
                    async_::async_fn(async move {
                        async_::timeout(10).await?;
                        did_finish_time2.set(tick());
                        Ok(())
                    })
                }
            });

            ok(Promise::all(vec![p1, p2])).await;

            assert_eq!(did_finish_time1.get(), 0);
            assert_eq!(did_finish_time2.get(), 1);
            assert_eq!(did_drain_time.get(), 2);

            d.dispose();
            queue.dispose();
        });
    }

    #[test]
    fn drain_event_is_send_only_once() {
        async_test(|_| async move {
            let queue = Queue::<()>::new();

            let drain_count = Rc::new(Cell::new(0));
            let d = queue.on_drained().listen({
                let drain_count = drain_count.clone();
                move |_| drain_count.set(drain_count.get() + 1)
            });
            queue.queue(|| resolved_async(()));
            queue.queue(|| resolved_async(()));
            queue.queue(|| resolved_async(()));
            queue.queue(|| resolved_async(()));
            assert_eq!(drain_count.get(), 0);
            assert_eq!(queue.size(), 4);

            ok(queue.when_idle()).await;

            assert_eq!(drain_count.get(), 1);

            d.dispose();
            queue.dispose();
        });
    }

    #[test]
    fn order_is_kept() {
        async_test(|_| async move {
            let queue = Queue::<usize>::new();

            let res = shared::<i32>();

            let f1 = resolved_then(&res, 1);
            let f2 = timeout_then(&res, 10, 2);
            let f3 = resolved_then(&res, 3);
            let f4 = timeout_then(&res, 20, 4);
            let f5 = timeout_then(&res, 0, 5);

            queue.queue(f1);
            queue.queue(f2);
            queue.queue(f3);
            queue.queue(f4);
            ok(queue.queue(f5)).await;
            assert_eq!(res.borrow()[0], 1);
            assert_eq!(res.borrow()[1], 2);
            assert_eq!(res.borrow()[2], 3);
            assert_eq!(res.borrow()[3], 4);
            assert_eq!(res.borrow()[4], 5);
        });
    }

    #[test]
    fn errors_bubble_individually_but_not_cause_stop() {
        async_test(|_| async move {
            let queue = Queue::<usize>::new();

            let res = shared::<i32>();
            let error = Rc::new(Cell::new(false));

            let f1 = resolved_then(&res, 1);
            let f2 = timeout_then(&res, 10, 2);
            let f3 = || Promise::resolved(()).then_promise(|_| Promise::<usize>::rejected(Error::new("error")));
            let f4 = timeout_then(&res, 20, 4);
            let f5 = timeout_then(&res, 0, 5);

            queue.queue(f1);
            queue.queue(f2);
            let failed = queue.queue(f3).then_result({
                let error = error.clone();
                move |_| {
                    error.set(true);
                    Ok(())
                }
            });
            queue.queue(f4);
            ok(queue.queue(f5)).await;
            ok(failed).await;
            assert_eq!(res.borrow()[0], 1);
            assert_eq!(res.borrow()[1], 2);
            assert!(error.get());
            assert_eq!(res.borrow()[2], 4);
            assert_eq!(res.borrow()[3], 5);
        });
    }

    #[test]
    fn order_is_kept_chained() {
        async_test(|_| async move {
            let queue = Queue::<usize>::new();

            let res = shared::<i32>();

            let f1 = resolved_then(&res, 1);
            let f2 = timeout_then(&res, 10, 2);
            let f3 = resolved_then(&res, 3);
            let f4 = timeout_then(&res, 20, 4);
            let f5 = timeout_then(&res, 0, 5);

            ok(queue.queue(f1)).await;
            ok(queue.queue(f2)).await;
            ok(queue.queue(f3)).await;
            ok(queue.queue(f4)).await;
            ok(queue.queue(f5)).await;
            assert_eq!(res.borrow()[0], 1);
            assert_eq!(res.borrow()[1], 2);
            assert_eq!(res.borrow()[2], 3);
            assert_eq!(res.borrow()[3], 4);
            assert_eq!(res.borrow()[4], 5);
        });
    }

    #[test]
    fn events() {
        async_test(|_| async move {
            let queue = Queue::<usize>::new();

            let drained = Rc::new(Cell::new(false));
            let on_drained = queue.on_drained().to_promise(None).then({
                let drained = drained.clone();
                move |_| drained.set(true)
            });

            let res = shared::<i32>();

            let f1 = timeout_then(&res, 10, 2);
            let f2 = timeout_then(&res, 20, 4);
            let f3 = timeout_then(&res, 0, 5);

            let q1 = queue.queue(f1);
            let q2 = queue.queue(f2);
            queue.queue(f3);

            let checked = q1.then_promise({
                let drained = drained.clone();
                move |_| {
                    assert!(!drained.get());
                    q2.then({
                        let drained = drained.clone();
                        move |_| assert!(!drained.get())
                    })
                }
            });

            ok(on_drained).await;
            assert!(drained.get());
            ok(checked).await;
        });
    }
}

mod resource_queue {
    use super::*;

    #[test]
    fn simple() {
        async_test(|_| async move {
            let env = Rc::new(FixedEnvironment::new(Platform::Posix, "/workspace/project"));
            let queue = ResourceQueue::new(je_base::resources::ExtUri::unbiased(env));
            let file = |path: &str| URI::file(path, Platform::Posix);

            ok(queue.when_drained()).await; // returns immediately since empty

            let done1 = Rc::new(Cell::new(false));
            queue.queue_for(&file("/some/path"), {
                let done1 = done1.clone();
                move || {
                    async_::async_fn(async move {
                        done1.set(true);
                        Ok(())
                    })
                }
            });
            ok(queue.when_drained()).await; // returns immediately since no work scheduled
            assert!(done1.get());

            let done2 = Rc::new(Cell::new(false));
            queue.queue_for(&file("/some/other/path"), {
                let done2 = done2.clone();
                move || {
                    async_::async_fn(async move {
                        done2.set(true);
                        Ok(())
                    })
                }
            });
            ok(queue.when_drained()).await; // returns immediately since no work scheduled
            assert!(done2.get());

            // schedule some work
            let w1 = DeferredPromise::<()>::new();
            let w1_promise = w1.promise.clone();
            queue.queue_for(&file("/some/path"), move || w1_promise);

            let drained = Rc::new(Cell::new(false));
            let drained_check = queue.when_drained().then({
                let drained = drained.clone();
                move |_| drained.set(true)
            });
            assert!(!drained.get());
            ok(w1.complete(())).await;
            ok(async_::timeout(0).promise.clone()).await;
            assert!(drained.get());
            ok(drained_check).await;

            // schedule some work
            let w2 = DeferredPromise::<()>::new();
            let w3 = DeferredPromise::<()>::new();
            let (w2_promise, w3_promise) = (w2.promise.clone(), w3.promise.clone());
            queue.queue_for(&file("/some/path"), move || w2_promise);
            queue.queue_for(&file("/some/other/path"), move || w3_promise);

            drained.set(false);
            let drained_check = queue.when_drained().then({
                let drained = drained.clone();
                move |_| drained.set(true)
            });

            queue.dispose();
            ok(async_::timeout(0).promise.clone()).await;
            assert!(drained.get());
            ok(drained_check).await;
        });
    }
}

mod retry {
    use super::*;

    #[test]
    fn success_case() {
        async_test(|_| async move {
            let counter = Rc::new(Cell::new(0));

            let res = ok(async_::retry(
                {
                    let counter = counter.clone();
                    move || {
                        counter.set(counter.get() + 1);
                        if counter.get() < 2 {
                            return Promise::rejected(Error::new("fail"));
                        }

                        Promise::resolved(true)
                    }
                },
                10,
                3,
            ))
            .await;

            assert!(res);
        });
    }

    #[test]
    fn error_case() {
        async_test(|_| async move {
            let expected_error = Error::new("fail");
            let result = async_::retry({
                let expected_error = expected_error.clone();
                move || Promise::<()>::rejected(expected_error.clone())
            }, 10, 3)
            .await;
            // `assert.strictEqual(error, error)` do original compara o erro com ele mesmo; aqui confere que é o esperado
            assert_eq!(result.expect_err("devia rejeitar"), expected_error);
        });
    }
}

mod task_sequentializer {
    use super::*;

    fn run_unit(sequentializer: &async_::TaskSequentializer, task_id: usize, promise: Promise<()>) -> Promise<()> {
        sequentializer.run(task_id, promise, None)
    }

    #[test]
    fn execution_basics() {
        async_test(|_| async move {
            let sequentializer = async_::TaskSequentializer::new();

            assert!(!sequentializer.is_running());
            assert!(!sequentializer.has_queued());
            assert!(!sequentializer.is_running_task(2323));
            assert!(sequentializer.running().is_none());

            // pending removes itself after done
            ok(run_unit(&sequentializer, 1, Promise::resolved(()))).await;
            assert!(!sequentializer.is_running());
            assert!(!sequentializer.is_running_task(1));
            assert!(sequentializer.running().is_none());
            assert!(!sequentializer.has_queued());

            // pending removes itself after done (use async.timeout)
            run_unit(&sequentializer, 2, async_::timeout(1).promise.clone());
            assert!(sequentializer.is_running());
            assert!(sequentializer.is_running_task(2));
            assert!(!sequentializer.has_queued());
            assert!(!sequentializer.is_running_task(1));
            assert!(sequentializer.running().is_some());

            ok(async_::timeout(2).promise.clone()).await;
            assert!(!sequentializer.is_running());
            assert!(!sequentializer.is_running_task(2));
            assert!(sequentializer.running().is_none());
        });
    }

    /// `async.timeout(ms).then(() => { flag = true; return; })`.
    fn timeout_then_set(ms: u64, flag: &Rc<Cell<bool>>) -> Promise<()> {
        let flag = flag.clone();
        async_::timeout(ms).then(move |_| flag.set(true))
    }

    #[test]
    fn executing_and_queued_finishes_instantly() {
        async_test(|_| async move {
            let sequentializer = async_::TaskSequentializer::new();

            let pending_done = Rc::new(Cell::new(false));
            run_unit(&sequentializer, 1, timeout_then_set(1, &pending_done));

            // queued finishes instantly
            let queued_done = Rc::new(Cell::new(false));
            let res = sequentializer.queue({
                let queued_done = queued_done.clone();
                move || {
                    Promise::resolved(()).then(move |_| {
                        queued_done.set(true);
                    })
                }
            });

            assert!(sequentializer.has_queued());

            ok(res).await;
            assert!(pending_done.get());
            assert!(queued_done.get());
            assert!(!sequentializer.has_queued());
        });
    }

    #[test]
    fn executing_and_queued_finishes_after_timeout() {
        async_test(|_| async move {
            let sequentializer = async_::TaskSequentializer::new();

            let pending_done = Rc::new(Cell::new(false));
            run_unit(&sequentializer, 1, timeout_then_set(1, &pending_done));

            // queued finishes after async.timeout
            let queued_done = Rc::new(Cell::new(false));
            let res = sequentializer.queue({
                let queued_done = queued_done.clone();
                move || timeout_then_set(1, &queued_done)
            });

            ok(res).await;
            assert!(pending_done.get());
            assert!(queued_done.get());
            assert!(!sequentializer.has_queued());
        });
    }

    #[test]
    fn join_without_executing_or_queued() {
        async_test(|_| async move {
            let sequentializer = async_::TaskSequentializer::new();

            ok(sequentializer.join()).await;
            assert!(!sequentializer.has_queued());
        });
    }

    #[test]
    fn join_without_queued() {
        async_test(|_| async move {
            let sequentializer = async_::TaskSequentializer::new();

            let pending_done = Rc::new(Cell::new(false));
            run_unit(&sequentializer, 1, timeout_then_set(1, &pending_done));

            ok(sequentializer.join()).await;
            assert!(pending_done.get());
            assert!(!sequentializer.is_running());
        });
    }

    #[test]
    fn join_with_executing_and_queued() {
        async_test(|_| async move {
            let sequentializer = async_::TaskSequentializer::new();

            let pending_done = Rc::new(Cell::new(false));
            run_unit(&sequentializer, 1, timeout_then_set(1, &pending_done));

            // queued finishes after async.timeout
            let queued_done = Rc::new(Cell::new(false));
            sequentializer.queue({
                let queued_done = queued_done.clone();
                move || timeout_then_set(1, &queued_done)
            });

            ok(sequentializer.join()).await;
            assert!(pending_done.get());
            assert!(queued_done.get());
            assert!(!sequentializer.is_running());
            assert!(!sequentializer.has_queued());
        });
    }

    #[test]
    fn executing_and_multiple_queued_last_one_wins() {
        async_test(|_| async move {
            let sequentializer = async_::TaskSequentializer::new();

            let pending_done = Rc::new(Cell::new(false));
            run_unit(&sequentializer, 1, timeout_then_set(1, &pending_done));

            // queued finishes after async.timeout
            let first_done = Rc::new(Cell::new(false));
            let first_res = sequentializer.queue({
                let first_done = first_done.clone();
                move || timeout_then_set(2, &first_done)
            });

            let second_done = Rc::new(Cell::new(false));
            let second_res = sequentializer.queue({
                let second_done = second_done.clone();
                move || timeout_then_set(3, &second_done)
            });

            let third_done = Rc::new(Cell::new(false));
            let third_res = sequentializer.queue({
                let third_done = third_done.clone();
                move || timeout_then_set(4, &third_done)
            });

            ok(Promise::all(vec![first_res, second_res, third_res])).await;
            assert!(pending_done.get());
            assert!(!first_done.get());
            assert!(!second_done.get());
            assert!(third_done.get());
        });
    }

    #[test]
    fn cancel_executing() {
        async_test(|store| async move {
            let sequentializer = async_::TaskSequentializer::new();
            let cts_timeout = CancellationTokenSource::new(None);
            store.add(Rc::new(cts_timeout.clone()));

            let pending_cancelled = Rc::new(Cell::new(false));
            let timeout = async_::timeout_with_token(1, &cts_timeout.token());
            sequentializer.run(1, timeout, Some(Rc::new({
                let pending_cancelled = pending_cancelled.clone();
                move || pending_cancelled.set(true)
            })));
            sequentializer.cancel_running();

            assert!(pending_cancelled.get());
            cts_timeout.cancel();
        });
    }
}

mod disposable_timeout {
    use super::*;

    #[test]
    fn handler_only_success() {
        async_test(|_| async move {
            let cb = Rc::new(Cell::new(false));
            let t = async_::disposable_timeout({
                let cb = cb.clone();
                move || cb.set(true)
            }, 0, None);

            ok(async_::timeout(0).promise.clone()).await;

            assert!(cb.get());

            t.dispose();
        });
    }

    #[test]
    fn handler_only_cancel() {
        async_test(|_| async move {
            let cb = Rc::new(Cell::new(false));
            let t = async_::disposable_timeout({
                let cb = cb.clone();
                move || cb.set(true)
            }, 0, None);
            t.dispose();

            ok(async_::timeout(0).promise.clone()).await;

            assert!(!cb.get());
        });
    }

    #[test]
    fn store_managed_success() {
        async_test(|_| async move {
            let cb = Rc::new(Cell::new(false));
            let s = DisposableStore::new();
            async_::disposable_timeout({
                let cb = cb.clone();
                move || cb.set(true)
            }, 0, Some(s.clone()));

            ok(async_::timeout(0).promise.clone()).await;

            assert!(cb.get());

            s.dispose();
        });
    }

    #[test]
    fn store_managed_cancel_via_disposable() {
        async_test(|_| async move {
            let cb = Rc::new(Cell::new(false));
            let s = DisposableStore::new();
            let t = async_::disposable_timeout({
                let cb = cb.clone();
                move || cb.set(true)
            }, 0, Some(s.clone()));
            t.dispose();

            ok(async_::timeout(0).promise.clone()).await;

            assert!(!cb.get());

            s.dispose();
        });
    }

    #[test]
    fn store_managed_cancel_via_store() {
        async_test(|_| async move {
            let cb = Rc::new(Cell::new(false));
            let s = DisposableStore::new();
            async_::disposable_timeout({
                let cb = cb.clone();
                move || cb.set(true)
            }, 0, Some(s.clone()));
            s.dispose();

            ok(async_::timeout(0).promise.clone()).await;

            assert!(!cb.get());
        });
    }
}

mod disposable_long_timeout {
    use super::*;

    const MAX: u64 = async_::MAX_TIMEOUT_DELAY;

    #[test]
    fn fires_after_a_delay_larger_than_the_set_timeout_maximum() {
        async_test(|_| async move {
            let cb = Rc::new(Cell::new(false));
            let t = async_::disposable_long_timeout({
                let cb = cb.clone();
                move || cb.set(true)
            }, MAX * 2 + 1000, None);

            ok(async_::timeout(MAX * 2 + 2000).promise.clone()).await;

            assert!(cb.get());
            t.dispose();
        });
    }

    #[test]
    fn does_not_fire_after_disposal_mid_wait() {
        async_test(|_| async move {
            let cb = Rc::new(Cell::new(false));
            let t = async_::disposable_long_timeout({
                let cb = cb.clone();
                move || cb.set(true)
            }, MAX * 2, None);

            ok(async_::timeout(MAX).promise.clone()).await; // advance one chunk, then re-armed
            t.dispose();
            ok(async_::timeout(MAX * 2).promise.clone()).await;

            assert!(!cb.get());
        });
    }

    #[test]
    fn store_managed_success_evicts_on_fire() {
        async_test(|_| async move {
            let cb = Rc::new(Cell::new(false));
            let s = DisposableStore::new();
            async_::disposable_long_timeout({
                let cb = cb.clone();
                move || cb.set(true)
            }, MAX + 500, Some(s.clone()));

            ok(async_::timeout(MAX + 1000).promise.clone()).await;

            assert!(cb.get());
            s.dispose();
        });
    }

    #[test]
    fn store_managed_cancel_via_store() {
        async_test(|_| async move {
            let cb = Rc::new(Cell::new(false));
            let s = DisposableStore::new();
            async_::disposable_long_timeout({
                let cb = cb.clone();
                move || cb.set(true)
            }, MAX * 2, Some(s.clone()));
            s.dispose();

            ok(async_::timeout(MAX * 2).promise.clone()).await;

            assert!(!cb.get());
        });
    }
}

mod process_time_run_once_scheduler {
    use super::*;

    #[test]
    fn does_not_count_wall_clock_jumps() {
        let _guard = ensure_no_disposables_are_leaked_in_test_suite();
        runtime::with_faked_timers(|| {
            let calls = shared::<bool>();
            let scheduler = async_::ProcessTimeRunOnceScheduler::new(
                {
                    let calls = calls.clone();
                    move || calls.borrow_mut().push(true)
                },
                3000,
            );

            scheduler.schedule(None);
            runtime::advance_fake_time(1000);
            runtime::set_system_time(runtime::date_now() + 8 * 60 * 60 * 1000);
            runtime::advance_fake_time(1000);
            assert_eq!(snapshot(&calls), Vec::<bool>::new());

            runtime::advance_fake_time(1000);
            assert_eq!(snapshot(&calls), vec![true]);

            scheduler.dispose();
        });
    }
}

#[test]
fn race_cancellation() {
    async_test(|store| async move {
        let cts = CancellationTokenSource::new(None);
        store.add(Rc::new(cts.clone()));
        let cts_timeout = CancellationTokenSource::new(None);
        store.add(Rc::new(cts_timeout.clone()));

        let triggered = Rc::new(Cell::new(false));
        let timeout = async_::timeout_with_token(100, &cts_timeout.token());
        let p = async_::race_cancellation(
            &timeout.then({
                let triggered = triggered.clone();
                move |_| {
                    triggered.set(true);
                    true
                }
            }),
            &cts.token(),
        );
        cts.cancel();

        ok(p).await;

        assert!(!triggered.get());
        cts_timeout.cancel();
    });
}

#[test]
fn race_timeout() {
    async_test(|store| async move {
        let cts = CancellationTokenSource::new(None);
        store.add(Rc::new(cts.clone()));

        // timeout wins
        let timedout = Rc::new(Cell::new(false));
        let triggered = Rc::new(Cell::new(false));

        let cts_timeout1 = CancellationTokenSource::new(None);
        store.add(Rc::new(cts_timeout1.clone()));
        let timeout1 = async_::timeout_with_token(100, &cts_timeout1.token());
        let p1 = async_::race_timeout(
            &timeout1.then({
                let triggered = triggered.clone();
                move |_| {
                    triggered.set(true);
                    true
                }
            }),
            1,
            Some(Box::new({
                let timedout = timedout.clone();
                move || timedout.set(true)
            })),
        );
        cts.cancel();

        ok(p1).await;

        assert!(!triggered.get());
        assert!(timedout.get());
        cts_timeout1.cancel();

        // promise wins
        timedout.set(false);

        let cts_timeout2 = CancellationTokenSource::new(None);
        store.add(Rc::new(cts_timeout2.clone()));
        let timeout2 = async_::timeout_with_token(1, &cts_timeout2.token());
        let p2 = async_::race_timeout(
            &timeout2.then({
                let triggered = triggered.clone();
                move |_| {
                    triggered.set(true);
                    true
                }
            }),
            100,
            Some(Box::new({
                let timedout = timedout.clone();
                move || timedout.set(true)
            })),
        );
        cts.cancel();

        ok(p2).await;

        assert!(triggered.get());
        assert!(!timedout.get());
        cts_timeout2.cancel();
    });
}

#[test]
fn sequencer_by_key() {
    async_test(|_| async move {
        let s = async_::SequencerByKey::<String>::new();

        let r1 = ok(s.queue("key1".to_string(), || Promise::resolved("hello".to_string()))).await;
        assert_eq!(r1, "hello");

        let outcome = s.queue("key2".to_string(), || Promise::<String>::rejected(Error::new("failed"))).await;
        // Expected error
        assert_eq!(outcome.expect_err("devia rejeitar").message, "failed");

        // Still works after a queued promise is rejected
        let r3 = ok(s.queue("key2".to_string(), || Promise::resolved("hello".to_string()))).await;
        assert_eq!(r3, "hello");
    });
}

#[test]
fn interval_counter() {
    async_test(|_| async move {
        let now = Rc::new(Cell::new(0));
        let counter = async_::IntervalCounter::with_now_fn(5, {
            let now = now.clone();
            move || now.get()
        });

        assert_eq!(counter.increment(), 1);
        assert_eq!(counter.increment(), 2);
        assert_eq!(counter.increment(), 3);

        now.set(10);

        assert_eq!(counter.increment(), 1);
        assert_eq!(counter.increment(), 2);
        assert_eq!(counter.increment(), 3);
    });
}

mod first_parallel {
    use super::*;

    fn plain(values: Vec<i32>) -> Vec<CancelablePromise<i32>> {
        values.into_iter().map(|v| Promise::resolved(v).into()).collect()
    }

    #[test]
    fn simple() {
        async_test(|_| async move {
            let a = ok(async_::first_parallel(plain(vec![1, 2, 3]), |v| *v == 2, None)).await;
            assert_eq!(a, Some(2));
        });
    }

    #[test]
    fn uses_null_default() {
        async_test(|_| async move {
            assert_eq!(ok(async_::first_parallel(plain(vec![1]), |v| *v == 2, None)).await, None);
        });
    }

    #[test]
    fn uses_value_default() {
        async_test(|_| async move {
            assert_eq!(ok(async_::first_parallel(plain(vec![1]), |v| *v == 2, Some(4))).await, Some(4));
        });
    }

    #[test]
    fn empty() {
        async_test(|_| async move {
            assert_eq!(ok(async_::first_parallel(plain(vec![]), |v| *v == 2, Some(4))).await, Some(4));
        });
    }

    /// `createCancelablePromise(async ct => { ct1 = ct; await timeout(ms, ct); <body> })`.
    fn guarded(ms: u64, slot: &Rc<RefCell<Option<CancellationToken>>>, result: Result<i32, Error>) -> CancelablePromise<i32> {
        let slot = slot.clone();
        async_::create_cancelable_promise(move |ct| {
            *slot.borrow_mut() = Some(ct.clone());
            async_::async_fn(async move {
                async_::timeout_with_token(ms, &ct).await?;
                result
            })
        })
    }

    #[test]
    fn cancels() {
        async_test(|_| async move {
            let ct1 = Rc::new(RefCell::new(None));
            let p1 = guarded(200, &ct1, Ok(1));
            let ct2 = Rc::new(RefCell::new(None));
            let p2 = guarded(2, &ct2, Ok(2));

            assert_eq!(ok(async_::first_parallel(vec![p1, p2], |v| *v == 2, Some(4))).await, Some(2));
            assert!(ct1.borrow().as_ref().expect("ct1").is_cancellation_requested(), "should cancel a");
            assert!(ct2.borrow().as_ref().expect("ct2").is_cancellation_requested(), "should cancel b");
        });
    }

    #[test]
    fn rejection_handling() {
        async_test(|_| async move {
            let ct1 = Rc::new(RefCell::new(None));
            let p1 = guarded(200, &ct1, Ok(1));
            let ct2 = Rc::new(RefCell::new(None));
            let p2 = guarded(2, &ct2, Err(Error::new("oh no")));

            assert!(async_::first_parallel(vec![p1, p2], |v| *v == 2, Some(4)).await.is_err(), "ok");
            assert!(ct1.borrow().as_ref().expect("ct1").is_cancellation_requested(), "should cancel a");
            assert!(ct2.borrow().as_ref().expect("ct2").is_cancellation_requested(), "should cancel b");
        });
    }
}

mod deferred_promise {
    use super::*;

    #[test]
    fn resolves() {
        async_test(|_| async move {
            let deferred = DeferredPromise::<i32>::new();
            assert!(!deferred.is_resolved());
            deferred.complete(42);
            assert_eq!(ok(deferred.promise.clone()).await, 42);
            assert!(deferred.is_resolved());
        });
    }

    #[test]
    fn rejects() {
        async_test(|_| async move {
            let deferred = DeferredPromise::<i32>::new();
            assert!(!deferred.is_rejected());
            let err = Error::new("oh no!");
            deferred.error(err.clone());
            assert_eq!(deferred.promise.clone().await.expect_err("devia rejeitar"), err);
            assert!(deferred.is_rejected());
        });
    }

    #[test]
    fn cancels() {
        async_test(|_| async move {
            let deferred = DeferredPromise::<i32>::new();
            assert!(!deferred.is_rejected());
            deferred.cancel();
            assert_eq!(deferred.promise.clone().await.expect_err("devia rejeitar").name, "Canceled");
            assert!(deferred.is_rejected());
        });
    }

    #[test]
    fn retains_the_original_settled_value() {
        async_test(|_| async move {
            let deferred = DeferredPromise::<i32>::new();
            assert!(!deferred.is_resolved());
            assert_eq!(deferred.value(), None);

            deferred.complete(42);
            assert_eq!(ok(deferred.promise.clone()).await, 42);
            assert_eq!(deferred.value(), Some(42));
            assert!(deferred.is_resolved());

            deferred.complete(-1);
            assert_eq!(ok(deferred.promise.clone()).await, 42);
            assert_eq!(deferred.value(), Some(42));
            assert!(deferred.is_resolved());
        });
    }
}

mod promises_settled {
    use super::*;

    #[test]
    fn resolves() {
        async_test(|_| async move {
            let p1 = Promise::resolved(1);
            let p2 = async_::timeout(1).then(|_| 2);
            let p3 = async_::timeout(2).then(|_| 3);

            let result = ok(async_::promises::settled(vec![p1, p2, p3])).await;

            assert_eq!(result.len(), 3);
            assert_eq!(result[0], 1);
            assert_eq!(result[1], 2);
            assert_eq!(result[2], 3);
        });
    }

    #[test]
    fn resolves_in_order() {
        async_test(|_| async move {
            let p1 = async_::timeout(2).then(|_| 1);
            let p2 = async_::timeout(1).then(|_| 2);
            let p3 = Promise::resolved(3);

            let result = ok(async_::promises::settled(vec![p1, p2, p3])).await;

            assert_eq!(result.len(), 3);
            assert_eq!(result[0], 1);
            assert_eq!(result[1], 2);
            assert_eq!(result[2], 3);
        });
    }

    #[test]
    fn rejects_with_first_error_but_handles_all_promises_all_errors() {
        async_test(|_| async move {
            let p1 = Promise::<i32>::rejected(Error::new("1"));

            let p2_handled = Rc::new(Cell::new(false));
            let p2_error = Error::new("2");
            let p2 = async_::timeout(1).then_result({
                let (p2_handled, p2_error) = (p2_handled.clone(), p2_error.clone());
                move |_| {
                    p2_handled.set(true);
                    Err::<i32, _>(p2_error)
                }
            });

            let p3_handled = Rc::new(Cell::new(false));
            let p3_error = Error::new("3");
            let p3 = async_::timeout(2).then_result({
                let (p3_handled, p3_error) = (p3_handled.clone(), p3_error.clone());
                move |_| {
                    p3_handled.set(true);
                    Err::<i32, _>(p3_error)
                }
            });

            let error = async_::promises::settled(vec![p1, p2, p3]).await.expect_err("devia rejeitar");

            assert_ne!(error, p2_error);
            assert_ne!(error, p3_error);
            assert!(p2_handled.get());
            assert!(p3_handled.get());
        });
    }

    #[test]
    fn rejects_with_first_error_but_handles_all_promises_1_error() {
        async_test(|_| async move {
            let p1 = Promise::resolved(1);

            let p2_handled = Rc::new(Cell::new(false));
            let p2_error = Error::new("2");
            let p2 = async_::timeout(1).then_result({
                let (p2_handled, p2_error) = (p2_handled.clone(), p2_error.clone());
                move |_| {
                    p2_handled.set(true);
                    Err::<i32, _>(p2_error)
                }
            });

            let p3_handled = Rc::new(Cell::new(false));
            let p3 = async_::timeout(2).then({
                let p3_handled = p3_handled.clone();
                move |_| {
                    p3_handled.set(true);
                    3
                }
            });

            let error = async_::promises::settled(vec![p1, p2, p3]).await.expect_err("devia rejeitar");

            assert_eq!(error, p2_error);
            assert!(p2_handled.get());
            assert!(p3_handled.get());
        });
    }
}

mod promises_with_async_body {
    use super::*;

    #[test]
    fn basics() {
        async_test(|_| async move {
            let p1 = async_::promises::with_async_body(|resolver: Resolver<i32>| {
                async_::async_fn(async move {
                    resolver.resolve(1);
                    Ok(())
                })
            });

            let p2 = async_::promises::with_async_body(|resolver: Resolver<i32>| {
                async_::async_fn(async move {
                    resolver.reject(Error::new("error"));
                    Ok(())
                })
            });

            let p3 = async_::promises::with_async_body(|_resolver: Resolver<i32>| async_::async_fn(async move { Err(Error::new("error")) }));

            let r1 = ok(p1).await;
            assert_eq!(r1, 1);

            let e2 = p2.await;
            assert!(e2.is_err());

            let e3 = p3.await;
            assert!(e3.is_err());
        });
    }
}

mod throttled_worker {
    use super::*;

    fn assert_array_equals(actual: &[i32], expected: &[i32]) {
        assert_eq!(actual.len(), expected.len());

        for (a, e) in actual.iter().zip(expected) {
            assert_eq!(a, e);
        }
    }

    fn options(max_work_chunk_size: usize, max_buffered_work: Option<usize>) -> ThrottledWorkerOptions {
        ThrottledWorkerOptions { max_work_chunk_size, max_buffered_work, throttle_delay: 1, wait_throttle_delay_between_work_units: false }
    }

    #[test]
    fn basics() {
        async_test(|store| async move {
            let handled = shared::<i32>();

            let handled_promise: Rc<RefCell<Promise<()>>> = Rc::new(RefCell::new(Promise::new().0));
            let handled_callback: Rc<RefCell<Option<Resolver<()>>>> = Rc::default();
            let renew = {
                let (handled_promise, handled_callback) = (handled_promise.clone(), handled_callback.clone());
                move || {
                    let (promise, resolver) = Promise::<()>::new();
                    *handled_promise.borrow_mut() = promise;
                    *handled_callback.borrow_mut() = Some(resolver);
                }
            };
            renew();
            let handled_counter_to_resolve = Rc::new(Cell::new(1));
            let current_handled_counter = Rc::new(Cell::new(0));

            let handler = {
                let (handled, handled_callback, to_resolve, current, renew) =
                    (handled.clone(), handled_callback.clone(), handled_counter_to_resolve.clone(), current_handled_counter.clone(), renew.clone());
                move |units: Vec<i32>| {
                    handled.borrow_mut().extend(units);

                    current.set(current.get() + 1);
                    if current.get() == to_resolve.get() {
                        let callback = handled_callback.borrow().clone().expect("o resolvedor");
                        callback.resolve(());

                        renew();
                        current.set(0);
                    }
                }
            };

            let worker = store.add_rc(async_::ThrottledWorker::new(options(5, None), handler));

            let wait_handled = || {
                let promise = handled_promise.borrow().clone();
                ok(promise)
            };

            // Work less than chunk size

            let mut worked = worker.work(&[1, 2, 3]);

            assert_array_equals(&handled.borrow(), &[1, 2, 3]);
            assert_eq!(worker.pending(), 0);
            assert!(worked);

            worker.work(&[4, 5]);
            worked = worker.work(&[6]);

            assert_array_equals(&handled.borrow(), &[1, 2, 3, 4, 5, 6]);
            assert_eq!(worker.pending(), 0);
            assert!(worked);

            // Work more than chunk size (variant 1)

            handled.borrow_mut().clear();
            handled_counter_to_resolve.set(2);

            worked = worker.work(&[1, 2, 3, 4, 5, 6, 7]);

            assert_array_equals(&handled.borrow(), &[1, 2, 3, 4, 5]);
            assert_eq!(worker.pending(), 2);
            assert!(worked);

            wait_handled().await;

            assert_array_equals(&handled.borrow(), &[1, 2, 3, 4, 5, 6, 7]);

            handled.borrow_mut().clear();
            handled_counter_to_resolve.set(4);

            worked = worker.work(&[1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16, 17, 18, 19]);

            assert_array_equals(&handled.borrow(), &[1, 2, 3, 4, 5]);
            assert_eq!(worker.pending(), 14);
            assert!(worked);

            wait_handled().await;

            assert_array_equals(&handled.borrow(), &[1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16, 17, 18, 19]);

            // Work more than chunk size (variant 2)

            handled.borrow_mut().clear();
            handled_counter_to_resolve.set(2);

            worked = worker.work(&[1, 2, 3, 4, 5, 6, 7, 8, 9, 10]);

            assert_array_equals(&handled.borrow(), &[1, 2, 3, 4, 5]);
            assert_eq!(worker.pending(), 5);
            assert!(worked);

            wait_handled().await;

            assert_array_equals(&handled.borrow(), &[1, 2, 3, 4, 5, 6, 7, 8, 9, 10]);

            // Work more while throttled (variant 1)

            handled.borrow_mut().clear();
            handled_counter_to_resolve.set(3);

            worked = worker.work(&[1, 2, 3, 4, 5, 6, 7]);

            assert_array_equals(&handled.borrow(), &[1, 2, 3, 4, 5]);
            assert_eq!(worker.pending(), 2);
            assert!(worked);

            worker.work(&[8]);
            worked = worker.work(&[9, 10, 11]);

            assert_array_equals(&handled.borrow(), &[1, 2, 3, 4, 5]);
            assert_eq!(worker.pending(), 6);
            assert!(worked);

            wait_handled().await;

            assert_array_equals(&handled.borrow(), &[1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11]);
            assert_eq!(worker.pending(), 0);

            // Work more while throttled (variant 2)

            handled.borrow_mut().clear();
            handled_counter_to_resolve.set(2);

            worked = worker.work(&[1, 2, 3, 4, 5, 6, 7]);

            assert_array_equals(&handled.borrow(), &[1, 2, 3, 4, 5]);
            assert!(worked);

            worker.work(&[8]);
            worked = worker.work(&[9, 10]);

            assert_array_equals(&handled.borrow(), &[1, 2, 3, 4, 5]);
            assert!(worked);

            wait_handled().await;

            assert_array_equals(&handled.borrow(), &[1, 2, 3, 4, 5, 6, 7, 8, 9, 10]);
        });
    }

    #[test]
    fn do_not_accept_too_much_work() {
        async_test(|store| async move {
            let handled = shared::<i32>();
            let handler = {
                let handled = handled.clone();
                move |units: Vec<i32>| handled.borrow_mut().extend(units)
            };

            let worker = store.add_rc(async_::ThrottledWorker::new(options(5, Some(5)), handler));

            let mut worked = worker.work(&[1, 2, 3]);
            assert!(worked);

            worked = worker.work(&[1, 2, 3, 4, 5, 6]);
            assert!(worked);
            assert_eq!(worker.pending(), 1);

            worked = worker.work(&[7]);
            assert!(worked);
            assert_eq!(worker.pending(), 2);

            worked = worker.work(&[8, 9, 10, 11]);
            assert!(!worked);
            assert_eq!(worker.pending(), 2);
        });
    }

    #[test]
    fn do_not_accept_too_much_work_account_for_max_chunk_size() {
        async_test(|store| async move {
            let handled = shared::<i32>();
            let handler = {
                let handled = handled.clone();
                move |units: Vec<i32>| handled.borrow_mut().extend(units)
            };

            let worker = store.add_rc(async_::ThrottledWorker::new(options(5, Some(5)), handler));

            let mut worked = worker.work(&[1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11]);
            assert!(!worked);
            assert_eq!(worker.pending(), 0);

            worked = worker.work(&[1, 2, 3, 4, 5, 6, 7, 8, 9, 10]);
            assert!(worked);
            assert_eq!(worker.pending(), 5);
        });
    }

    #[test]
    fn disposed() {
        async_test(|store| async move {
            let handled = shared::<i32>();
            let handler = {
                let handled = handled.clone();
                move |units: Vec<i32>| handled.borrow_mut().extend(units)
            };

            let worker = store.add_rc(async_::ThrottledWorker::new(options(5, None), handler));
            worker.dispose();
            let worked = worker.work(&[1, 2, 3]);

            assert_array_equals(&handled.borrow(), &[]);
            assert_eq!(worker.pending(), 0);
            assert!(!worked);
        });
    }
}

mod limited_queue {
    use super::*;

    #[test]
    fn basics_with_long_running_task() {
        async_test(|_| async move {
            let limited_queue = async_::LimitedQueue::new();

            let counter = Rc::new(Cell::new(0));
            let mut promises = Vec::new();
            for i in 0..5 {
                let counter = counter.clone();
                promises.push(limited_queue.queue(move || {
                    async_::async_fn(async move {
                        counter.set(i);
                        async_::timeout(1).await
                    })
                }));
            }

            ok(Promise::all(promises)).await;

            // only the last task executed
            assert_eq!(counter.get(), 4);
        });
    }

    #[test]
    fn basics_with_sync_running_task() {
        async_test(|_| async move {
            let limited_queue = async_::LimitedQueue::new();

            let counter = Rc::new(Cell::new(0));
            let mut promises = Vec::new();
            for i in 0..5 {
                let counter = counter.clone();
                promises.push(limited_queue.queue(move || {
                    async_::async_fn(async move {
                        counter.set(i);
                        Ok(())
                    })
                }));
            }

            ok(Promise::all(promises)).await;

            // only the last task executed
            assert_eq!(counter.get(), 4);
        });
    }
}

/// `new AsyncIterableObject<number>(writer => { writer.emitMany([1, 2, 3, 4, 5]); }, () => { calledOnReturn = true; })`.
fn five_items_iterable(called_on_return: &Rc<Cell<bool>>) -> Rc<AsyncIterableObject<i32>> {
    let called_on_return = called_on_return.clone();
    AsyncIterableObject::new(
        |writer| {
            writer.emit_many(vec![1, 2, 3, 4, 5]);
            MaybePromise::Value(())
        },
        Some(Rc::new(move || called_on_return.set(true))),
    )
}

mod async_iterable_object {
    use super::*;

    #[test]
    fn on_return_not_called() {
        async_test(|_| async move {
            let called_on_return = Rc::new(Cell::new(false));
            let iter = five_items_iterable(&called_on_return);

            for_await(&*iter, |_item| Ok(ControlFlow::Continue(()))).await.expect("for await");

            assert!(!called_on_return.get());
        });
    }

    #[test]
    fn on_return_called_on_break() {
        async_test(|_| async move {
            let called_on_return = Rc::new(Cell::new(false));
            let iter = five_items_iterable(&called_on_return);

            for_await(&*iter, |item| {
                assert_eq!(item, 1);
                Ok(ControlFlow::Break(()))
            })
            .await
            .expect("for await");

            assert!(called_on_return.get());
        });
    }

    #[test]
    fn on_return_called_on_return() {
        async_test(|_| async move {
            let called_on_return = Rc::new(Cell::new(false));
            let iter = five_items_iterable(&called_on_return);

            ok(async_::async_fn(async move {
                for_await(&*iter, |item| {
                    assert_eq!(item, 1);
                    Ok(ControlFlow::Break(())) // return
                })
                .await
            }))
            .await;

            assert!(called_on_return.get());
        });
    }

    #[test]
    fn on_return_called_on_throwing() {
        async_test(|_| async move {
            let called_on_return = Rc::new(Cell::new(false));
            let iter = five_items_iterable(&called_on_return);

            let outcome = for_await(&*iter, |item| {
                assert_eq!(item, 1);
                Err(Error::new(""))
            })
            .await;
            assert!(outcome.is_err());

            assert!(called_on_return.get());
        });
    }
}

mod async_iterable_source {
    use super::*;

    #[test]
    fn on_return_is_wired_up() {
        async_test(|_| async move {
            let called_on_return = Rc::new(Cell::new(false));
            let source = AsyncIterableSource::<i32>::new(Some(Rc::new({
                let called_on_return = called_on_return.clone();
                move || called_on_return.set(true)
            })));

            source.emit_one(1);
            source.emit_one(2);
            source.emit_one(3);
            source.resolve();

            for_await(&*source.async_iterable(), |item| {
                assert_eq!(item, 1);
                Ok(ControlFlow::Break(()))
            })
            .await
            .expect("for await");

            assert!(called_on_return.get());
        });
    }

    #[test]
    fn on_return_is_wired_up_2() {
        async_test(|_| async move {
            let called_on_return = Rc::new(Cell::new(false));
            let source = AsyncIterableSource::<i32>::new(Some(Rc::new({
                let called_on_return = called_on_return.clone();
                move || called_on_return.set(true)
            })));

            source.emit_one(1);
            source.emit_one(2);
            source.emit_one(3);
            source.resolve();

            for_await(&*source.async_iterable(), |_item| Ok(ControlFlow::Continue(()))).await.expect("for await");

            assert!(!called_on_return.get());
        });
    }

    #[test]
    fn emit_many_emits_all_items() {
        async_test(|_| async move {
            let source = AsyncIterableSource::<i32>::new(None);
            let values = vec![10, 20, 30, 40];
            source.emit_many(values.clone());
            source.resolve();

            let result = ok(async_::async_iterable_to_promise(source.async_iterable())).await;

            assert_eq!(result, values);
        });
    }
}

mod cancellable_iterable {
    use super::*;

    fn cts(store: &Rc<DisposableStore>) -> CancellationTokenSource {
        let cts = CancellationTokenSource::new(None);
        store.add(Rc::new(cts.clone()));
        cts
    }

    #[test]
    fn should_iterate_through_all_values_when_not_canceled() {
        async_test(|store| async move {
            let cts = cts(&store);
            let async_iterable = GeneratorIterable {
                create: Box::new(|| {
                    AsyncGenerator::new(|y| async move {
                        y.yield_("a").await?;
                        y.yield_("b").await?;
                        y.yield_("c").await?;
                        Ok(())
                    })
                }),
            };

            let cancelable_iterable = async_::cancellable_iterable(&async_iterable, cts.token());

            let result = ok(async_::async_to_array(cancelable_iterable)).await;
            assert_eq!(result, ["a", "b", "c"]);
        });
    }

    #[test]
    fn should_stop_iteration_immediately_when_cancelled_before_starting() {
        async_test(|store| async move {
            let cts = cts(&store);
            let values = shared::<&str>();

            let async_iterable = GeneratorIterable {
                create: Box::new({
                    let values = values.clone();
                    move || {
                        let values = values.clone();
                        AsyncGenerator::new(|y| async move {
                            values.borrow_mut().push("iterator created");
                            y.yield_("a").await?;
                            values.borrow_mut().push("after a");
                            y.yield_("b").await?;
                            values.borrow_mut().push("after b");
                            y.yield_("c").await?;
                            values.borrow_mut().push("after c");
                            Ok(())
                        })
                    }
                }),
            };

            // Cancel before iteration starts
            cts.cancel();
            let cancelable_iterable = async_::cancellable_iterable(&async_iterable, cts.token());

            let result = ok(async_::async_to_array(cancelable_iterable)).await;
            assert_eq!(result, Vec::<&str>::new());
            assert_eq!(snapshot(&values), Vec::<&str>::new());
        });
    }

    #[test]
    fn should_stop_iteration_when_cancelled_during_iteration() {
        async_test(|_| async move {
            let cts = CancellationTokenSource::new(None);
            let deferred_a = Rc::new(DeferredPromise::<()>::new());
            let deferred_b = Rc::new(DeferredPromise::<()>::new());
            let deferred_c = Rc::new(DeferredPromise::<()>::new());

            let values = shared::<&str>();

            let async_iterable = GeneratorIterable {
                create: Box::new({
                    let (values, deferred_a, deferred_b, deferred_c) = (values.clone(), deferred_a.clone(), deferred_b.clone(), deferred_c.clone());
                    move || {
                        let (values, deferred_a, deferred_b, deferred_c) = (values.clone(), deferred_a.clone(), deferred_b.clone(), deferred_c.clone());
                        AsyncGenerator::new(|y| async move {
                            values.borrow_mut().push("a yielded");
                            y.yield_("a").await?;
                            deferred_a.promise.clone().await?;

                            values.borrow_mut().push("b yielded");
                            y.yield_("b").await?;
                            deferred_b.promise.clone().await?;

                            values.borrow_mut().push("c yielded");
                            y.yield_("c").await?;
                            deferred_c.promise.clone().await?;
                            Ok(())
                        })
                    }
                }),
            };

            let cancelable = async_::cancellable_iterable(&async_iterable, cts.token());
            for_await(&*cancelable, |value| {
                if value == "a" {
                    deferred_a.complete(());
                } else if value == "b" {
                    cts.cancel();
                    deferred_b.complete(());
                } else {
                    return Err(Error::new("Unexpected value"));
                }
                Ok(ControlFlow::Continue(()))
            })
            .await
            .expect("for await");

            assert_eq!(snapshot(&values), ["a yielded", "b yielded"]);
        });
    }

    #[test]
    fn should_handle_return_method_correctly() {
        async_test(|store| async move {
            let cts = cts(&store);
            let return_called = Rc::new(Cell::new(false));
            let n = Rc::new(Cell::new(0));

            let original_iterable = {
                let (return_called, n) = (return_called.clone(), n.clone());
                AsyncGenerator::new(move |y| async move {
                    let body = async {
                        y.yield_("a").await?;
                        n.set(n.get() + 1);
                        y.yield_("b").await?;
                        n.set(n.get() + 1);
                        y.yield_("c").await?;
                        n.set(n.get() + 1);
                        Ok::<(), Error>(())
                    };
                    let outcome = body.await;
                    return_called.set(true);
                    outcome
                })
            };

            // Add a return method to the iterator
            struct WithReturn {
                inner: Rc<AsyncGenerator<&'static str>>,
                return_called: Rc<Cell<bool>>,
            }
            impl AsyncIterator<&'static str> for WithReturn {
                fn next(&self) -> Promise<Option<&'static str>> {
                    self.inner.next()
                }

                fn return_(&self) -> Option<Promise<()>> {
                    self.return_called.set(true);
                    Some(Promise::resolved(()))
                }
            }
            let original_iterator: Rc<dyn AsyncIterator<&'static str>> = Rc::new(WithReturn { inner: original_iterable, return_called: return_called.clone() });

            // Create a test-specific iterable with our mocked iterator
            let test_iterable = FixedIterable { iterator: original_iterator };

            let cancelable = async_::cancellable_iterable(&test_iterable, cts.token());
            for_await(&*cancelable, |value| Ok(if value == "b" { ControlFlow::Break(()) } else { ControlFlow::Continue(()) })).await.expect("for await");

            assert!(return_called.get());
            assert!(n.get() < 2);
        });
    }
}

/// `async executor` do original: `async emitter => { ... }`.
fn async_executor<T: Clone + 'static, F: Future<Output = Result<(), Error>> + 'static>(body: impl FnOnce(AsyncIterableEmitter<T>) -> F + 'static) -> impl FnOnce(AsyncIterableEmitter<T>) -> MaybePromise<()> {
    move |emitter| MaybePromise::Promise(async_::async_fn(body(emitter)))
}

/// `emitter => { ... }` (executor síncrono).
fn sync_executor<T: Clone + 'static>(body: impl FnOnce(AsyncIterableEmitter<T>) + 'static) -> impl FnOnce(AsyncIterableEmitter<T>) -> MaybePromise<()> {
    move |emitter| {
        body(emitter);
        MaybePromise::Value(())
    }
}

/// `for await (const item of producer) result.push(item)`, devolvendo o resultado ou o erro que a espera lançou.
async fn collect<T: Clone + 'static>(iterable: &dyn AsyncIterable<T>) -> (Vec<T>, Option<Error>) {
    let mut result = Vec::new();
    let outcome = for_await(iterable, |item| {
        result.push(item);
        Ok(ControlFlow::Continue(()))
    })
    .await;
    (result, outcome.err())
}

mod async_iterable_producer {
    use super::*;

    #[test]
    fn emit_one_produces_single_values() {
        async_test(|_| async move {
            let producer = AsyncIterableProducer::<i32>::new(
                sync_executor(|emitter| {
                    emitter.emit_one(1);
                    emitter.emit_one(2);
                    emitter.emit_one(3);
                }),
                None,
            );

            let (result, error) = collect(&*producer).await;

            assert!(error.is_none());
            assert_eq!(result, [1, 2, 3]);
        });
    }

    #[test]
    fn emit_many_produces_multiple_values() {
        async_test(|_| async move {
            let producer = AsyncIterableProducer::<i32>::new(
                sync_executor(|emitter| {
                    emitter.emit_many(vec![1, 2, 3]);
                    emitter.emit_many(vec![4, 5]);
                }),
                None,
            );

            let (result, _) = collect(&*producer).await;

            assert_eq!(result, [1, 2, 3, 4, 5]);
        });
    }

    #[test]
    fn mixed_emit_one_and_emit_many() {
        async_test(|_| async move {
            let producer = AsyncIterableProducer::<i32>::new(
                sync_executor(|emitter| {
                    emitter.emit_one(1);
                    emitter.emit_many(vec![2, 3]);
                    emitter.emit_one(4);
                }),
                None,
            );

            let (result, _) = collect(&*producer).await;

            assert_eq!(result, [1, 2, 3, 4]);
        });
    }

    #[test]
    fn async_executor_with_emit_one() {
        async_test(|_| async move {
            let producer = AsyncIterableProducer::<i32>::new(
                async_executor(|emitter| async move {
                    emitter.emit_one(1);
                    async_::timeout(1).await?;
                    emitter.emit_one(2);
                    async_::timeout(1).await?;
                    emitter.emit_one(3);
                    Ok(())
                }),
                None,
            );

            let (result, _) = collect(&*producer).await;

            assert_eq!(result, [1, 2, 3]);
        });
    }

    #[test]
    fn async_executor_with_emit_many() {
        async_test(|_| async move {
            let producer = AsyncIterableProducer::<i32>::new(
                async_executor(|emitter| async move {
                    emitter.emit_many(vec![1, 2]);
                    async_::timeout(1).await?;
                    emitter.emit_many(vec![3, 4]);
                    Ok(())
                }),
                None,
            );

            let (result, _) = collect(&*producer).await;

            assert_eq!(result, [1, 2, 3, 4]);
        });
    }

    #[test]
    fn reject_with_error() {
        async_test(|_| async move {
            let expected_error = Error::new("test error");
            let producer = AsyncIterableProducer::<i32>::new(
                sync_executor({
                    let expected_error = expected_error.clone();
                    move |emitter| {
                        emitter.emit_one(1);
                        emitter.reject(expected_error);
                    }
                }),
                None,
            );

            let (result, caught_error) = collect(&*producer).await;

            assert_eq!(result, [1]);
            assert_eq!(caught_error, Some(expected_error));
        });
    }

    #[test]
    fn async_executor_throws_error() {
        async_test(|_| async move {
            let expected_error = Error::new("executor error");
            let producer = AsyncIterableProducer::<i32>::new(
                async_executor({
                    let expected_error = expected_error.clone();
                    move |emitter| async move {
                        emitter.emit_one(1);
                        Err(expected_error)
                    }
                }),
                None,
            );

            let (result, caught_error) = collect(&*producer).await;

            assert_eq!(result, [1]);
            assert_eq!(caught_error, Some(expected_error));
        });
    }

    #[test]
    fn empty_producer() {
        async_test(|_| async move {
            let producer = AsyncIterableProducer::<i32>::new(sync_executor(|_emitter| {}), None);

            let (result, _) = collect(&*producer).await;

            assert_eq!(result, Vec::<i32>::new());
        });
    }

    #[test]
    fn async_executor_resolves_without_emitting() {
        async_test(|_| async move {
            let producer = AsyncIterableProducer::<i32>::new(
                async_executor(|_emitter| async move {
                    async_::timeout(1).await?;
                    // Don't emit anything
                    Ok(())
                }),
                None,
            );

            let (result, _) = collect(&*producer).await;

            assert_eq!(result, Vec::<i32>::new());
        });
    }

    #[test]
    fn multiple_iterators_on_same_producer() {
        async_test(|_| async move {
            let producer = AsyncIterableProducer::<i32>::new(sync_executor(|emitter| emitter.emit_many(vec![1, 2, 3])), None);

            // First iterator should consume all values
            let (result1, _) = collect(&*producer).await;

            // Second iterator should not see any values (already consumed)
            let (result2, _) = collect(&*producer).await;

            assert_eq!(result1, [1, 2, 3]);
            assert_eq!(result2, Vec::<i32>::new());
        });
    }

    #[test]
    fn concurrent_iteration() {
        async_test(|_| async move {
            let producer = AsyncIterableProducer::<i32>::new(
                async_executor(|emitter| async move {
                    emitter.emit_one(1);
                    async_::timeout(1).await?;
                    emitter.emit_one(2);
                    async_::timeout(1).await?;
                    emitter.emit_one(3);
                    Ok(())
                }),
                None,
            );

            let iterator1 = producer.async_iterator();
            let iterator2 = producer.async_iterator();

            // Both iterators share the same underlying producer
            let first1 = ok(iterator1.next()).await;
            let first2 = ok(iterator2.next()).await;
            let second1 = ok(iterator1.next()).await;
            let second2 = ok(iterator2.next()).await;

            // Since they share the same producer, values are consumed in order
            assert_eq!(first1, Some(1));
            assert_eq!(first2, Some(2));
            assert_eq!(second1, Some(3));
            assert_eq!(second2, None); // done
        });
    }

    #[test]
    fn executor_with_promise_return_value() {
        async_test(|_| async move {
            let producer = AsyncIterableProducer::<i32>::new(
                |emitter| {
                    emitter.emit_one(1);
                    emitter.emit_one(2);
                    MaybePromise::Promise(Promise::resolved(()))
                },
                None,
            );

            let (result, _) = collect(&*producer).await;

            assert_eq!(result, [1, 2]);
        });
    }

    #[test]
    fn executor_with_non_promise_return_value() {
        async_test(|_| async move {
            let producer = AsyncIterableProducer::<i32>::new(
                |emitter| {
                    emitter.emit_one(1);
                    emitter.emit_one(2);
                    MaybePromise::Value(()) // 'some value'
                },
                None,
            );

            let (result, _) = collect(&*producer).await;

            assert_eq!(result, [1, 2]);
        });
    }

    #[test]
    fn emit_many_with_empty_array() {
        async_test(|_| async move {
            let producer = AsyncIterableProducer::<i32>::new(
                sync_executor(|emitter| {
                    emitter.emit_one(1);
                    emitter.emit_many(vec![]);
                    emitter.emit_one(2);
                }),
                None,
            );

            let (result, _) = collect(&*producer).await;

            assert_eq!(result, [1, 2]);
        });
    }

    #[test]
    fn reject_immediately_without_emitting() {
        async_test(|_| async move {
            let expected_error = Error::new("immediate error");
            let producer = AsyncIterableProducer::<i32>::new(
                sync_executor({
                    let expected_error = expected_error.clone();
                    move |emitter| emitter.reject(expected_error)
                }),
                None,
            );

            let (result, caught_error) = collect(&*producer).await;

            assert!(result.is_empty(), "Should not iterate when rejected immediately");
            assert_eq!(caught_error, Some(expected_error));
        });
    }

    #[test]
    fn string_values() {
        async_test(|_| async move {
            let producer = AsyncIterableProducer::<String>::new(
                sync_executor(|emitter| {
                    emitter.emit_one("hello".to_string());
                    emitter.emit_many(vec!["world".to_string(), "test".to_string()]);
                }),
                None,
            );

            let (result, _) = collect(&*producer).await;

            assert_eq!(result, ["hello", "world", "test"]);
        });
    }

    #[test]
    fn object_values() {
        #[derive(Clone, Debug, PartialEq)]
        struct TestObject {
            id: i32,
            name: &'static str,
        }

        async_test(|_| async move {
            let producer = AsyncIterableProducer::<TestObject>::new(
                sync_executor(|emitter| {
                    emitter.emit_one(TestObject { id: 1, name: "first" });
                    emitter.emit_many(vec![TestObject { id: 2, name: "second" }, TestObject { id: 3, name: "third" }]);
                }),
                None,
            );

            let (result, _) = collect(&*producer).await;

            assert_eq!(result, [TestObject { id: 1, name: "first" }, TestObject { id: 2, name: "second" }, TestObject { id: 3, name: "third" }]);
        });
    }

    #[test]
    fn tee_both_iterators_receive_all_values() {
        async_test(|_| async move {
            // TODO: Implementation bug - executors don't await start(), causing producers to finalize early
            let source_generator = AsyncGenerator::<i32>::new(|y| async move {
                y.yield_(1).await?;
                y.yield_(2).await?;
                y.yield_(3).await?;
                y.yield_(4).await?;
                y.yield_(5).await?;
                Ok(())
            });

            let (iter1, iter2) = AsyncIterableProducer::tee(source_generator);

            // Consume both iterables concurrently
            let consume = |iter: Rc<AsyncIterableProducer<i32>>| async_::async_fn(async move { Ok(collect(&*iter).await.0) });
            let results = ok(Promise::all(vec![consume(iter1), consume(iter2)])).await;

            assert_eq!(results[0], [1, 2, 3, 4, 5]);
            assert_eq!(results[1], [1, 2, 3, 4, 5]);
        });
    }

    #[test]
    fn tee_sequential_consumption() {
        async_test(|_| async move {
            // TODO: Implementation bug - executors don't await start(), causing producers to finalize early
            let source = AsyncIterableProducer::<i32>::new(sync_executor(|emitter| emitter.emit_many(vec![1, 2, 3])), None);

            let (iter1, iter2) = AsyncIterableProducer::tee(source);

            // Consume first iterator completely
            let (result1, _) = collect(&*iter1).await;

            // Then consume second iterator
            let (result2, _) = collect(&*iter2).await;

            assert_eq!(result1, [1, 2, 3]);
            assert_eq!(result2, [1, 2, 3]);
        });
    }

    #[test]
    #[ignore = "test.skip no original: tee depende de um bug conhecido do executor (TODO do VS Code)"]
    fn tee_empty_source() {
        async_test(|_| async move {
            // TODO: Implementation bug - executors don't await start(), causing producers to finalize early
            let source = AsyncIterableProducer::<i32>::new(sync_executor(|_emitter| {}), None);

            let (iter1, iter2) = AsyncIterableProducer::tee(source);

            let consume = |iter: Rc<AsyncIterableProducer<i32>>| async_::async_fn(async move { Ok(collect(&*iter).await.0) });
            let results = ok(Promise::all(vec![consume(iter1), consume(iter2)])).await;

            assert_eq!(results[0], Vec::<i32>::new());
            assert_eq!(results[1], Vec::<i32>::new());
        });
    }

    #[test]
    #[ignore = "test.skip no original: tee depende de um bug conhecido do executor (TODO do VS Code)"]
    fn tee_handles_errors_in_source() {
        async_test(|_| async move {
            // TODO: Implementation bug - executors don't await start(), causing producers to finalize early
            let expected_error = Error::new("source error");
            let source = AsyncIterableProducer::<i32>::new(
                async_executor({
                    let expected_error = expected_error.clone();
                    move |emitter| async move {
                        emitter.emit_one(1);
                        emitter.emit_one(2);
                        Err(expected_error)
                    }
                }),
                None,
            );

            let (iter1, iter2) = AsyncIterableProducer::tee(source);

            let consume = |iter: Rc<AsyncIterableProducer<i32>>| async_::async_fn(async move { Ok(collect(&*iter).await) });
            let results = ok(Promise::all(vec![consume(iter1), consume(iter2)])).await;

            // Both iterators should have received the same values before error
            assert_eq!(results[0].0, [1, 2]);
            assert_eq!(results[1].0, [1, 2]);

            // Both should have received the error
            assert_eq!(results[0].1, Some(expected_error.clone()));
            assert_eq!(results[1].1, Some(expected_error));
        });
    }
}

mod async_reader {
    use super::*;

    /// `async function* createAsyncIterator<T>(values: T[])`.
    fn create_async_iterator<T: Clone + 'static>(values: Vec<T>) -> Rc<dyn AsyncIterator<T>> {
        AsyncGenerator::new(|y| async move {
            for value in values {
                y.yield_(value).await?;
            }
            Ok(())
        })
    }

    /// `async function* createDelayedAsyncIterator<T>(values: T[], delayMs = 1)`.
    fn create_delayed_async_iterator<T: Clone + 'static>(values: Vec<T>, delay_ms: u64) -> Rc<dyn AsyncIterator<T>> {
        AsyncGenerator::new(move |y| async move {
            for value in values {
                async_::timeout(delay_ms).await?;
                y.yield_(value).await?;
            }
            Ok(())
        })
    }

    use AsyncRead::{EndOfStream, Item};

    #[test]
    fn read_basic_functionality() {
        async_test(|_| async move {
            let reader = AsyncReader::new(create_async_iterator(vec![1, 2, 3]));

            assert_eq!(ok(reader.read()).await, Item(1));
            assert_eq!(ok(reader.read()).await, Item(2));
            assert_eq!(ok(reader.read()).await, Item(3));
            assert_eq!(ok(reader.read()).await, EndOfStream);
        });
    }

    #[test]
    fn read_empty_iterator() {
        async_test(|_| async move {
            let reader = AsyncReader::new(create_async_iterator(Vec::<i32>::new()));

            assert_eq!(ok(reader.read()).await, EndOfStream);
            assert_eq!(ok(reader.read()).await, EndOfStream);
        });
    }

    #[test]
    fn end_of_stream_property() {
        async_test(|_| async move {
            let reader = AsyncReader::new(create_async_iterator(vec![1, 2]));

            assert!(!reader.end_of_stream());

            ok(reader.read()).await; // 1
            assert!(!reader.end_of_stream());

            ok(reader.read()).await; // 2
            assert!(!reader.end_of_stream());

            ok(reader.read()).await; // end
            assert!(reader.end_of_stream());
        });
    }

    #[test]
    fn peek_basic_functionality() {
        async_test(|_| async move {
            let reader = AsyncReader::new(create_async_iterator(vec![1, 2, 3]));

            assert_eq!(ok(reader.peek()).await, Item(1));
            assert_eq!(ok(reader.peek()).await, Item(1)); // Should return same value
            assert_eq!(ok(reader.read()).await, Item(1)); // Should consume the peeked value

            assert_eq!(ok(reader.peek()).await, Item(2));
            assert_eq!(ok(reader.read()).await, Item(2));
        });
    }

    #[test]
    fn peek_empty_iterator() {
        async_test(|_| async move {
            let reader = AsyncReader::new(create_async_iterator(Vec::<i32>::new()));

            assert_eq!(ok(reader.peek()).await, EndOfStream);
        });
    }

    #[test]
    fn read_sync_or_throw_throws_when_no_data_available() {
        async_test(|_| async move {
            let reader = AsyncReader::new(create_async_iterator(vec![1]));

            // Read the only item
            ok(reader.read()).await;

            // Should throw since no more data and not at end yet
            assert!(errors::catch(|| reader.read_buffered_or_throw()).is_err());
        });
    }

    #[test]
    fn read_sync_or_throw_returns_end_of_stream_when_at_end() {
        async_test(|_| async move {
            let reader = AsyncReader::new(create_async_iterator(Vec::<i32>::new()));

            // Trigger end detection
            ok(reader.read()).await;

            assert_eq!(reader.read_buffered_or_throw(), EndOfStream);
        });
    }

    #[test]
    fn peek_sync_or_throw_with_buffered_data() {
        async_test(|_| async move {
            let reader = AsyncReader::new(create_async_iterator(vec![1, 2, 3]));

            // First peek to populate buffer
            ok(reader.peek()).await;

            // Should be able to peek sync now
            assert_eq!(reader.peek_buffered_or_throw(), Item(1));
            assert_eq!(reader.peek_buffered_or_throw(), Item(1)); // Should return same value
        });
    }

    #[test]
    fn peek_sync_or_throw_throws_when_no_data_available() {
        async_test(|_| async move {
            let reader = AsyncReader::new(create_async_iterator(vec![1]));

            // Should throw since buffer is empty and we haven't loaded anything
            assert!(errors::catch(|| reader.peek_buffered_or_throw()).is_err());
        });
    }

    #[test]
    fn peek_sync_or_throw_returns_end_of_stream_when_at_end() {
        async_test(|_| async move {
            let reader = AsyncReader::new(create_async_iterator(Vec::<i32>::new()));

            // Trigger end detection
            ok(reader.peek()).await;

            assert_eq!(reader.peek_buffered_or_throw(), EndOfStream);
        });
    }

    #[test]
    fn consume_to_end_consumes_all_remaining_data() {
        async_test(|_| async move {
            let reader = AsyncReader::new(create_async_iterator(vec![1, 2, 3, 4, 5]));

            // Read some data first
            assert_eq!(ok(reader.read()).await, Item(1));
            assert_eq!(ok(reader.read()).await, Item(2));

            // Consume the rest
            ok(reader.consume_to_end()).await;

            assert!(reader.end_of_stream());
            assert_eq!(ok(reader.read()).await, EndOfStream);
        });
    }

    #[test]
    fn consume_to_end_on_empty_reader() {
        async_test(|_| async move {
            let reader = AsyncReader::new(create_async_iterator(Vec::<i32>::new()));

            ok(reader.consume_to_end()).await;

            assert!(reader.end_of_stream());
        });
    }

    /// `async value => collected.push(value)`.
    fn collector(collected: &Shared<i32>) -> impl FnMut(i32) -> MaybePromise<()> + 'static {
        let collected = collected.clone();
        move |value| {
            let collected = collected.clone();
            MaybePromise::Promise(async_::async_fn(async move {
                collected.borrow_mut().push(value);
                Ok(())
            }))
        }
    }

    #[test]
    fn read_while_basic_functionality() {
        async_test(|_| async move {
            let reader = AsyncReader::new(create_async_iterator(vec![1, 2, 3, 4, 5]));
            let collected = shared::<i32>();

            ok(reader.read_while(|value| *value < 4, collector(&collected))).await;

            assert_eq!(snapshot(&collected), [1, 2, 3]);

            // Next read should return 4
            assert_eq!(ok(reader.read()).await, Item(4));
        });
    }

    #[test]
    fn read_while_stops_at_end_of_stream() {
        async_test(|_| async move {
            let reader = AsyncReader::new(create_async_iterator(vec![1, 2, 3]));
            let collected = shared::<i32>();

            ok(reader.read_while(
                |value| *value < 10, // Always true
                collector(&collected),
            ))
            .await;

            assert_eq!(snapshot(&collected), [1, 2, 3]);
            assert!(reader.end_of_stream());
        });
    }

    #[test]
    fn read_while_empty_iterator() {
        async_test(|_| async move {
            let reader = AsyncReader::new(create_async_iterator(Vec::<i32>::new()));
            let collected = shared::<i32>();

            ok(reader.read_while(|_| true, collector(&collected))).await;

            assert_eq!(snapshot(&collected), Vec::<i32>::new());
        });
    }

    #[test]
    fn read_while_predicate_returns_false_immediately() {
        async_test(|_| async move {
            let reader = AsyncReader::new(create_async_iterator(vec![1, 2, 3]));
            let collected = shared::<i32>();

            ok(reader.read_while(
                |_| false, // Always false
                collector(&collected),
            ))
            .await;

            assert_eq!(snapshot(&collected), Vec::<i32>::new());

            // First item should still be available
            assert_eq!(ok(reader.read()).await, Item(1));
        });
    }

    #[test]
    fn peek_timeout_with_immediate_data() {
        async_test(|_| async move {
            let reader = AsyncReader::new(create_async_iterator(vec![1, 2, 3]));

            let result = ok(reader.peek_timeout(100)).await;
            assert_eq!(result, Some(Item(1)));
        });
    }

    #[test]
    fn peek_timeout_with_delayed_data() {
        async_test(|_| async move {
            let reader = AsyncReader::new(create_delayed_async_iterator(vec![1, 2, 3], 10));

            let result = ok(reader.peek_timeout(50)).await;
            assert_eq!(result, Some(Item(1)));
        });
    }

    #[test]
    fn peek_timeout_timeout_occurs() {
        async_test(|_| async move {
            let reader = AsyncReader::new(create_delayed_async_iterator(vec![1, 2, 3], 50));

            let result = ok(reader.peek_timeout(10)).await;
            assert_eq!(result, None);

            ok(reader.consume_to_end()).await;
        });
    }

    #[test]
    fn peek_timeout_empty_iterator() {
        async_test(|_| async move {
            let reader = AsyncReader::new(create_async_iterator(Vec::<i32>::new()));

            let result = ok(reader.peek_timeout(10)).await;
            assert_eq!(result, Some(EndOfStream));
        });
    }

    #[test]
    fn peek_timeout_after_consuming_all_data() {
        async_test(|_| async move {
            let reader = AsyncReader::new(create_async_iterator(vec![1]));

            ok(reader.consume_to_end()).await;
            let result = ok(reader.peek_timeout(10)).await;
            assert_eq!(result, Some(EndOfStream));
        });
    }

    #[test]
    fn mixed_operations_complex_scenario() {
        async_test(|_| async move {
            let reader = AsyncReader::new(create_async_iterator(vec![1, 2, 3, 4, 5, 6, 7, 8, 9, 10]));

            // Peek first
            assert_eq!(ok(reader.peek()).await, Item(1));

            // Read some
            assert_eq!(ok(reader.read()).await, Item(1));
            assert_eq!(ok(reader.read()).await, Item(2));

            // Peek again
            assert_eq!(ok(reader.peek()).await, Item(3));

            // Read while
            let collected = shared::<i32>();
            ok(reader.read_while(|value| *value <= 5, collector(&collected))).await;
            assert_eq!(snapshot(&collected), [3, 4, 5]);

            // Use sync operations
            assert_eq!(ok(reader.peek()).await, Item(6));
            assert_eq!(reader.peek_buffered_or_throw(), Item(6));
            assert_eq!(reader.read_buffered_or_throw(), Item(6));

            // Consume rest
            ok(reader.consume_to_end()).await;
            assert!(reader.end_of_stream());
        });
    }

    #[test]
    fn string_values() {
        async_test(|_| async move {
            let reader = AsyncReader::new(create_async_iterator(vec!["hello", "world", "test"]));

            assert_eq!(ok(reader.read()).await, Item("hello"));
            assert_eq!(ok(reader.peek()).await, Item("world"));
            assert_eq!(ok(reader.read()).await, Item("world"));
            assert_eq!(ok(reader.read()).await, Item("test"));
            assert_eq!(ok(reader.read()).await, EndOfStream);
        });
    }

    #[test]
    fn object_values() {
        #[derive(Clone, Debug, PartialEq)]
        struct TestObj {
            id: i32,
            name: &'static str,
        }

        async_test(|_| async move {
            let objects = vec![TestObj { id: 1, name: "first" }, TestObj { id: 2, name: "second" }, TestObj { id: 3, name: "third" }];

            let reader = AsyncReader::new(create_async_iterator(objects));

            assert_eq!(ok(reader.read()).await, Item(TestObj { id: 1, name: "first" }));
            assert_eq!(ok(reader.peek()).await, Item(TestObj { id: 2, name: "second" }));
            assert_eq!(ok(reader.read()).await, Item(TestObj { id: 2, name: "second" }));
        });
    }

    #[test]
    fn concurrent_operations() {
        async_test(|_| async move {
            let reader = AsyncReader::new(create_delayed_async_iterator(vec![1, 2, 3], 5));

            // Start multiple operations concurrently
            let peek_promise = reader.peek();
            let read_promise = reader.read();

            let peek_result = ok(peek_promise).await;
            let read_result = ok(read_promise).await;

            // Both should return the same first value
            assert_eq!(peek_result, Item(1));
            assert_eq!(read_result, Item(1));

            // Next read should get the second value
            assert_eq!(ok(reader.read()).await, Item(2));
        });
    }

    #[test]
    fn buffer_management_single_extend_buffer_call() {
        async_test(|_| async move {
            struct MockIterator {
                next_call_count: Rc<Cell<i32>>,
            }
            impl AsyncIterator<i32> for MockIterator {
                fn next(&self) -> Promise<Option<i32>> {
                    let next_call_count = self.next_call_count.clone();
                    async_::async_fn(async move {
                        next_call_count.set(next_call_count.get() + 1);
                        if next_call_count.get() == 1 {
                            async_::timeout(1).await?;
                            return Ok(Some(1));
                        }
                        Ok(None)
                    })
                }
            }
            let next_call_count = Rc::new(Cell::new(0));
            let mock_iterator: Rc<dyn AsyncIterator<i32>> = Rc::new(MockIterator { next_call_count: next_call_count.clone() });

            let reader = AsyncReader::new(mock_iterator);

            // Multiple concurrent operations should only trigger one extend buffer call
            let peeks = vec![reader.peek(), reader.peek()];
            let read = reader.read();

            ok(Promise::all(peeks)).await;
            ok(read).await;

            // Should have called next() only once despite multiple concurrent operations
            assert_eq!(next_call_count.get(), 1);
        });
    }
}
