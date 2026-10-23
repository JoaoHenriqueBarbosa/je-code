//! Testes de `async_.rs` que o `async.test.ts` do VS Code não cobre: a ordem das microtarefas conferida
//! contra o Node real (a saída de referência vem de uma cópia mecânica, sem tipos, do trecho de `async.ts`,
//! rodada com `node`; cada cenário cria a cadeia `t1..t24` antes do corpo e registra a ordem em que as
//! reações rodam), e as classes e funções sem teste no original (timers, agendadores, `TaskQueue`,
//! combinadores de `AsyncIterable`, `Barrier`, `StatefulPromise`, idle).

mod common;

use std::cell::{Cell, RefCell};
use std::ops::ControlFlow;
use std::rc::Rc;

use common::{ensure_no_disposables_are_leaked_in_test_suite, run_with_faked_timers};
use je_base::async_::{self, AsyncIterable, AsyncIterableObject, AsyncIterableProducer, MaybePromise, for_await};
use je_base::cancellation::CancellationTokenSource;
use je_base::errors::Error;
use je_base::lifecycle::{DisposableStore, IDisposable};
use je_base::promise::Promise;
use je_base::runtime;
use je_base::symbols::Delay;

type Log = Rc<RefCell<Vec<String>>>;

/// Roda o cenário com o relógio virtual: cria a cadeia `t1..t24`, roda o corpo e devolve o log.
fn scenario(body: impl FnOnce(&Log)) -> String {
    let log: Log = Rc::default();
    runtime::with_faked_timers(|| {
        let mut chain = Promise::resolved(());
        for i in 1..=24 {
            let log = log.clone();
            chain = chain.then(move |()| log.borrow_mut().push(format!("t{i}")));
        }
        body(&log);
    });
    log.borrow().join(" ")
}

fn pusher(log: &Log, label: &str) -> impl FnOnce(i32) + 'static {
    let (log, label) = (log.clone(), label.to_string());
    move |value| log.borrow_mut().push(format!("{label}:{value}"))
}

/// Um `then(() => log(ok), () => log(err))`.
fn on_settled(log: &Log, ok: &str, err: &str) -> impl FnOnce(Result<i32, Error>) -> Result<(), Error> + 'static {
    let (log, ok, err) = (log.clone(), ok.to_string(), err.to_string());
    move |result| {
        log.borrow_mut().push(if result.is_ok() { ok } else { err });
        Ok(())
    }
}

fn expected(prefix: &str) -> String {
    let mut parts: Vec<String> = prefix.split(' ').map(str::to_string).collect();
    let last: usize = parts.iter().filter_map(|p| p.strip_prefix('t')).filter_map(|n| n.parse().ok()).max().unwrap_or(0);
    parts.extend(((last + 1)..=24).map(|i| format!("t{i}")));
    parts.join(" ")
}

mod ordem_igual_ao_node {
    use super::*;

    #[test]
    fn throttler() {
        let _guard = ensure_no_disposables_are_leaked_in_test_suite();
        let log = scenario(|log| {
            let th = async_::Throttler::<i32>::new();
            th.queue(|_| Promise::resolved(1)).then(pusher(log, "q1"));
            th.queue(|_| Promise::resolved(2)).then(pusher(log, "q2"));
            th.queue(|_| Promise::resolved(3)).then(pusher(log, "q3"));
        });
        assert_eq!(log, expected("t1 t2 q1:1 t3 t4 t5 t6 q2:3 q3:3 t7"));
    }

    #[test]
    fn sequencer() {
        let _guard = ensure_no_disposables_are_leaked_in_test_suite();
        let log = scenario(|log| {
            let s = async_::Sequencer::new();
            s.queue(|| Promise::resolved(1)).then(pusher(log, "s1"));
            s.queue(|| Promise::<i32>::rejected(Error::new("x"))).then_result(on_settled(log, "s2:ok", "s2:err"));
            s.queue(|| Promise::resolved(3)).then(pusher(log, "s3"));
        });
        assert_eq!(log, expected("t1 t2 t3 t4 s1:1 t5 t6 t7 s2:err t8 t9 t10 s3:3 t11"));
    }

    #[test]
    fn sequencer_by_key() {
        let _guard = ensure_no_disposables_are_leaked_in_test_suite();
        let log = scenario(|log| {
            let s = async_::SequencerByKey::<&str>::new();
            s.queue("a", || Promise::resolved(1)).then(pusher(log, "a1"));
            s.queue("a", || Promise::<i32>::rejected(Error::new("x"))).then_result(on_settled(log, "a2:ok", "a2:err"));
            s.queue("b", || Promise::resolved(3)).then(pusher(log, "b1"));
            s.queue("a", || Promise::resolved(4)).then(pusher(log, "a3"));
        });
        assert_eq!(log, expected("t1 t2 t3 t4 t5 t6 t7 t8 a1:1 b1:3 t9 t10 t11 t12 t13 t14 t15 a2:err t16 t17 t18 t19 t20 t21 t22 a3:4 t23"));
    }

    #[test]
    fn delayer_microtask() {
        let _guard = ensure_no_disposables_are_leaked_in_test_suite();
        let log = scenario(|log| {
            let d = async_::Delayer::<i32>::new(Delay::Microtask);
            d.trigger(|| MaybePromise::Value(1), None).then(pusher(log, "d1"));
            d.trigger(|| MaybePromise::Promise(Promise::resolved(2)), None).then(pusher(log, "d2"));
        });
        assert_eq!(log, expected("t1 t2 t3 t4 t5 d1:2 d2:2 t6"));
    }

    #[test]
    fn delayer_cancel_retrigger() {
        let _guard = ensure_no_disposables_are_leaked_in_test_suite();
        let log = scenario(|log| {
            let d = Rc::new(async_::Delayer::<i32>::new(Delay::Microtask));
            d.trigger(|| MaybePromise::Value(1), None).then_result(on_settled(log, "c1:ok", "c1:cancelled"));
            let (d2, log2) = (d.clone(), log.clone());
            runtime::queue_microtask(move || {
                d2.cancel();
                d2.trigger(|| MaybePromise::Value(2), None).then_result(on_settled(&log2, "c2:2", "c2:cancelled"));
            });
        });
        assert_eq!(log, expected("t1 t2 t3 c1:cancelled t4 c2:2 t5"));
    }

    #[test]
    fn task_sequentializer() {
        let _guard = ensure_no_disposables_are_leaked_in_test_suite();
        let log = scenario(|log| {
            let s = async_::TaskSequentializer::new();
            let push = |label: &'static str| {
                let log = log.clone();
                move |()| log.borrow_mut().push(label.to_string())
            };
            s.run(1, Promise::resolved(()), None).then(push("r1"));
            let inner = s.clone();
            s.queue(move || inner.run(2, Promise::resolved(()), None)).then(push("q2"));
            s.join().then(push("join"));
        });
        assert_eq!(log, expected("t1 r1 t2 t3 q2 t4 join t5"));
    }

    #[test]
    fn limiter() {
        let _guard = ensure_no_disposables_are_leaked_in_test_suite();
        let log = scenario(|log| {
            let l = async_::Limiter::<i32>::new(2);
            for i in 1..=4 {
                l.queue(move || Promise::resolved(i)).then(pusher(log, "l"));
            }
        });
        assert_eq!(log, expected("t1 t2 l:1 l:2 t3 l:3 l:4 t4"));
    }

    #[test]
    fn sequence_e_first() {
        let _guard = ensure_no_disposables_are_leaked_in_test_suite();
        let log = scenario(|log| {
            let factories: Vec<async_::PromiseFactory<Option<i32>>> = (1..=3).map(|n| Box::new(move || Promise::resolved(Some(n))) as async_::PromiseFactory<Option<i32>>).collect();
            let log1 = log.clone();
            async_::sequence(factories).then(move |values| log1.borrow_mut().push(format!("seq:{}", values.iter().map(ToString::to_string).collect::<Vec<_>>().join(","))));
            let factories: Vec<async_::PromiseFactory<i32>> = [0, 5, 6].into_iter().map(|n| Box::new(move || Promise::resolved(n)) as async_::PromiseFactory<i32>).collect();
            let log2 = log.clone();
            async_::first(factories, |t| *t != 0, None).then(move |value| log2.borrow_mut().push(format!("first:{}", value.map_or("null".to_string(), |v| v.to_string()))));
        });
        assert_eq!(log, expected("t1 t2 t3 t4 t5 t6 first:5 t7 t8 t9 t10 seq:1,2,3 t11"));
    }

    #[test]
    fn race_timeout() {
        let _guard = ensure_no_disposables_are_leaked_in_test_suite();
        let log = scenario(|log| {
            let log = log.clone();
            async_::race_timeout(&Promise::resolved(7), 100, None).then(move |value| log.borrow_mut().push(format!("rt:{}", value.expect("o valor"))));
        });
        assert_eq!(log, expected("t1 t2 t3 t4 t5 rt:7 t6"));
    }

    #[test]
    fn task_queue() {
        let _guard = ensure_no_disposables_are_leaked_in_test_suite();
        let log = scenario(|log| {
            let q = async_::TaskQueue::new();
            q.schedule(|| MaybePromise::Value(1)).then(pusher(log, "tq1"));
            q.schedule(|| MaybePromise::Promise(Promise::resolved(2))).then(pusher(log, "tq2"));
            q.schedule(|| MaybePromise::Value(3)).then(pusher(log, "tq3"));
        });
        assert_eq!(log, expected("t1 t2 tq1:1 t3 tq2:2 t4 tq3:3 t5"));
    }
}

/// `async_test`, como no `async.rs`: roda com o relógio virtual e a conferência de vazamentos.
fn async_test<F: std::future::Future<Output = ()> + 'static>(body: impl FnOnce(Rc<DisposableStore>) -> F) {
    let ds = ensure_no_disposables_are_leaked_in_test_suite();
    let store = ds.store();
    run_with_faked_timers(body(store));
}

async fn ok<T: Clone + 'static>(promise: Promise<T>) -> T {
    promise.await.expect("a promessa devia cumprir")
}

mod timers {
    use super::*;

    #[test]
    fn timeout_timer_cancel_and_set_replaces_the_runner() {
        async_test(|store| async move {
            let calls = Rc::new(RefCell::new(Vec::new()));
            let timer = store.add_rc(async_::TimeoutTimer::new());
            let first = calls.clone();
            timer.cancel_and_set(move || first.borrow_mut().push("first"), 10);
            let second = calls.clone();
            timer.cancel_and_set(move || second.borrow_mut().push("second"), 10);
            // já armado: `set_if_not_set` não troca
            let third = calls.clone();
            timer.set_if_not_set(move || third.borrow_mut().push("third"), 10);
            ok(async_::timeout(20).promise.clone()).await;
            assert_eq!(*calls.borrow(), ["second"]);
        });
    }

    #[test]
    fn timeout_timer_throws_when_used_after_dispose() {
        let timer = async_::TimeoutTimer::new();
        timer.dispose();
        assert!(je_base::errors::catch(|| timer.cancel_and_set(|| {}, 1)).is_err());
        assert!(je_base::errors::catch(|| timer.set_if_not_set(|| {}, 1)).is_err());
    }

    #[test]
    fn interval_timer_repeats_until_cancelled() {
        async_test(|store| async move {
            let ticks = Rc::new(Cell::new(0));
            let timer = store.add_rc(async_::IntervalTimer::new());
            let counter = ticks.clone();
            timer.cancel_and_set(move || counter.set(counter.get() + 1), 10);
            ok(async_::timeout(35).promise.clone()).await;
            assert_eq!(ticks.get(), 3);
            timer.cancel();
            ok(async_::timeout(50).promise.clone()).await;
            assert_eq!(ticks.get(), 3);
        });
    }

    #[test]
    fn run_once_scheduler_runs_once_and_reschedules() {
        async_test(|store| async move {
            let runs = Rc::new(Cell::new(0));
            let counter = runs.clone();
            let scheduler = store.add_rc(async_::RunOnceScheduler::new(move || counter.set(counter.get() + 1), 10));
            assert!(!scheduler.is_scheduled());
            scheduler.schedule(None);
            assert!(scheduler.is_scheduled());
            scheduler.schedule(Some(20)); // cancela o anterior
            ok(async_::timeout(15).promise.clone()).await;
            assert_eq!(runs.get(), 0);
            ok(async_::timeout(10).promise.clone()).await;
            assert_eq!(runs.get(), 1);
            assert!(!scheduler.is_scheduled());

            scheduler.schedule(None);
            scheduler.flush();
            assert_eq!(runs.get(), 2);
        });
    }

    #[test]
    fn run_once_worker_delivers_the_units_together() {
        async_test(|store| async move {
            let delivered = Rc::new(RefCell::new(Vec::new()));
            let sink = delivered.clone();
            let worker = store.add_rc(async_::RunOnceWorker::new(move |units: Vec<i32>| sink.borrow_mut().push(units), 5));
            worker.work(1);
            worker.work(2);
            ok(async_::timeout(10).promise.clone()).await;
            worker.work(3);
            ok(async_::timeout(10).promise.clone()).await;
            assert_eq!(*delivered.borrow(), [vec![1, 2], vec![3]]);
        });
    }

    #[test]
    fn auto_open_barrier_opens_by_timeout_or_explicitly() {
        async_test(|_| async move {
            let by_time = async_::AutoOpenBarrier::new(10);
            assert!(!by_time.is_open());
            assert!(ok(by_time.wait()).await);
            assert!(by_time.is_open());

            let explicit = async_::AutoOpenBarrier::new(1000);
            explicit.open();
            assert!(ok(explicit.wait()).await);
        });
    }

    #[test]
    fn create_timeout_runs_and_cancels() {
        async_test(|_| async move {
            let fired = Rc::new(RefCell::new(Vec::new()));
            let (a, b) = (fired.clone(), fired.clone());
            let kept = async_::create_timeout(5, move || a.borrow_mut().push("a"));
            async_::create_timeout(5, move || b.borrow_mut().push("b")).dispose();
            ok(async_::timeout(10).promise.clone()).await;
            assert_eq!(*fired.borrow(), ["a"]);
            kept.dispose();
        });
    }

    #[test]
    fn idle_value_runs_on_idle_or_on_first_read() {
        async_test(|_| async move {
            let on_idle = async_::GlobalIdleValue::new(|| 41);
            assert!(!on_idle.is_initialized());
            ok(async_::timeout(1).promise.clone()).await;
            assert!(on_idle.is_initialized());
            assert_eq!(on_idle.value(), 41);

            let eager = async_::GlobalIdleValue::new(|| 42);
            assert_eq!(eager.value(), 42); // lido antes de ficar ocioso
            assert!(eager.is_initialized());
        });
    }

    #[test]
    fn run_when_global_idle_cancels_and_fake_replaces() {
        async_test(|_| async move {
            let ran = Rc::new(RefCell::new(Vec::new()));
            let a = ran.clone();
            async_::run_when_global_idle(move |deadline| a.borrow_mut().push(deadline.did_timeout), None);
            let b = ran.clone();
            async_::run_when_global_idle(move |_| b.borrow_mut().push(false), None).dispose();
            ok(async_::timeout(1).promise.clone()).await;
            assert_eq!(*ran.borrow(), [true]);

            let fake = async_::install_fake_run_when_idle(Rc::new(|callback, _| {
                callback(async_::IdleDeadline::new(false, 0));
                je_base::lifecycle::none()
            }));
            let c = ran.clone();
            async_::run_when_global_idle(move |_| c.borrow_mut().push(false), None);
            assert_eq!(*ran.borrow(), [true, false]); // o fake rodou na hora
            fake.dispose();
        });
    }
}

mod task_queue {
    use super::*;

    #[test]
    fn clear_pending_cancels_or_skips_the_queued_tasks() {
        async_test(|_| async move {
            let queue = async_::TaskQueue::new();
            let (running_promise, running) = Promise::<i32>::new();
            let running_task = queue.schedule(move || MaybePromise::Promise(running_promise));
            let cancelled = queue.schedule(|| MaybePromise::Value(2));
            let skipped = queue.schedule_skip_if_cleared(|| MaybePromise::Value(3));

            queue.clear_pending();
            running.resolve(1);

            assert_eq!(ok(running_task).await, 1); // a que já rodava continua
            assert!(je_base::errors::is_cancellation_error(&cancelled.await.expect_err("devia rejeitar")));
            assert_eq!(ok(skipped).await, None);
        });
    }

    #[test]
    fn a_failing_task_rejects_only_its_own_promise() {
        async_test(|_| async move {
            let queue = async_::TaskQueue::new();
            let failing = queue.schedule(|| MaybePromise::Promise(Promise::<i32>::rejected(Error::new("falhou"))));
            let next = queue.schedule(|| MaybePromise::Value(7));
            assert_eq!(failing.await.expect_err("devia rejeitar").message, "falhou");
            assert_eq!(ok(next).await, 7);
        });
    }
}

mod promessas {
    use super::*;

    #[test]
    fn throttler_by_key_coalesces_per_key_and_cleans_up() {
        async_test(|_| async move {
            let throttler = async_::ThrottlerByKey::<&str, i32>::new();
            let runs = Rc::new(RefCell::new(Vec::new()));
            let task = |label: &'static str, runs: &Rc<RefCell<Vec<&'static str>>>| {
                let runs = runs.clone();
                move || {
                    runs.borrow_mut().push(label);
                    async_::timeout(1).then(|_| 1)
                }
            };
            let promises = vec![
                throttler.queue("a", task("a1", &runs)),
                throttler.queue("a", task("a2", &runs)),
                throttler.queue("a", task("a3", &runs)),
                throttler.queue("b", task("b1", &runs)),
            ];
            ok(Promise::all(promises)).await;
            assert_eq!(*runs.borrow(), ["a1", "b1", "a3"]);
            throttler.dispose();
        });
    }

    #[test]
    fn sequencer_runs_in_order_even_after_a_rejection() {
        async_test(|_| async move {
            let sequencer = async_::Sequencer::new();
            let order = Rc::new(RefCell::new(Vec::new()));
            let push = |n: i32, order: &Rc<RefCell<Vec<i32>>>| {
                let order = order.clone();
                move || {
                    order.borrow_mut().push(n);
                    async_::timeout(5).then(move |_| n)
                }
            };
            let first = sequencer.queue(push(1, &order));
            let failing = sequencer.queue(|| Promise::<i32>::rejected(Error::new("x")));
            let last = sequencer.queue(push(3, &order));
            assert_eq!(ok(first).await, 1);
            assert!(failing.await.is_err());
            assert_eq!(ok(last).await, 3);
            assert_eq!(*order.borrow(), [1, 3]);
        });
    }

    #[test]
    fn first_stops_at_the_first_match_and_uses_the_default() {
        async_test(|_| async move {
            let ran = Rc::new(Cell::new(0));
            let factories = |values: Vec<i32>| -> Vec<async_::PromiseFactory<i32>> {
                values
                    .into_iter()
                    .map(|v| {
                        let ran = ran.clone();
                        Box::new(move || {
                            ran.set(ran.get() + 1);
                            Promise::resolved(v)
                        }) as async_::PromiseFactory<i32>
                    })
                    .collect()
            };
            assert_eq!(ok(async_::first(factories(vec![1, 2, 3]), |v| *v == 2, Some(9))).await, Some(2));
            assert_eq!(ran.get(), 2); // a terceira fábrica não rodou
            assert_eq!(ok(async_::first(factories(vec![1]), |v| *v == 2, Some(9))).await, Some(9));
        });
    }

    #[test]
    fn sequence_skips_none_results() {
        async_test(|_| async move {
            let factories: Vec<async_::PromiseFactory<Option<i32>>> = vec![
                Box::new(|| Promise::resolved(Some(1))),
                Box::new(|| Promise::resolved(None)),
                Box::new(|| Promise::resolved(Some(3))),
            ];
            assert_eq!(ok(async_::sequence(factories)).await, [1, 3]);
        });
    }

    #[test]
    fn race_cancellation_variants() {
        async_test(|store| async move {
            let cts = CancellationTokenSource::new(None);
            store.add(Rc::new(cts.clone()));
            let never = Promise::<i32>::new().0;
            let with_default = async_::race_cancellation_or(&never, &cts.token(), 5);
            let without_default = async_::race_cancellation(&never, &cts.token());
            let as_error = async_::race_cancellation_error(&never, &cts.token());
            cts.cancel();
            assert_eq!(ok(with_default).await, 5);
            assert_eq!(ok(without_default).await, None);
            assert!(je_base::errors::is_cancellation_error(&as_error.await.expect_err("devia rejeitar")));
        });
    }

    #[test]
    fn reject_if_not_canceled_swallows_only_cancellation() {
        async_test(|_| async move {
            assert!(async_::reject_if_not_canceled(Error::cancellation()).await.is_ok());
            assert!(async_::reject_if_not_canceled(Error::new("outro")).await.is_err());
        });
    }

    #[test]
    fn not_cancellable_promise_follows_the_original() {
        async_test(|_| async move {
            let cancelable = async_::create_cancelable_promise(|_| Promise::resolved(3));
            assert_eq!(ok(async_::not_cancellable_promise(&cancelable)).await, 3);
        });
    }

    #[test]
    fn deferred_promise_settle_with_follows_the_other_promise() {
        async_test(|_| async move {
            let ok_deferred = async_::DeferredPromise::from_promise(&Promise::resolved(4));
            assert_eq!(ok(ok_deferred.promise.clone()).await, 4);
            let err_deferred = async_::DeferredPromise::from_promise(&Promise::<i32>::rejected(Error::new("e")));
            assert!(err_deferred.promise.clone().await.is_err());
            assert!(err_deferred.is_rejected());
        });
    }

    #[test]
    fn stateful_promise_keeps_the_result() {
        async_test(|_| async move {
            let (promise, resolver) = Promise::<i32>::new();
            let stateful = async_::StatefulPromise::new(&promise);
            assert!(!stateful.is_resolved());
            assert!(je_base::errors::catch(|| stateful.require_value()).is_err());
            resolver.resolve(8);
            ok(stateful.promise.clone()).await;
            assert!(stateful.is_resolved());
            assert_eq!(stateful.require_value(), 8);
            assert_eq!(stateful.value(), Some(8));

            let failing = async_::StatefulPromise::new(&Promise::<i32>::rejected(Error::new("e")));
            assert!(failing.promise.clone().await.is_err());
            assert_eq!(failing.error().map(|e| e.message), Some("e".to_string()));
            assert!(je_base::errors::catch(|| failing.require_value()).is_err());
        });
    }

    #[test]
    fn lazy_stateful_promise_computes_on_first_use() {
        async_test(|_| async move {
            let computed = Rc::new(Cell::new(0));
            let counter = computed.clone();
            let lazy = async_::LazyStatefulPromise::new(move || {
                counter.set(counter.get() + 1);
                Promise::resolved(6)
            });
            assert_eq!(lazy.current_value(), None);
            assert_eq!(computed.get(), 0);
            assert_eq!(ok(lazy.get_promise()).await, 6);
            assert_eq!(lazy.require_value(), 6);
            assert_eq!(lazy.current_value(), Some(6));
            assert_eq!(computed.get(), 1);
        });
    }

    #[test]
    fn as_promise_wraps_values_and_promises() {
        async_test(|_| async move {
            assert_eq!(ok(async_::as_promise(|| 1)).await, 1);
            assert_eq!(ok(async_::as_promise_thenable(|| Promise::resolved(2))).await, 2);
            assert!(async_::as_promise_thenable(|| Promise::<i32>::rejected(Error::new("e"))).await.is_err());
            let (promise, resolver) = async_::promise_with_resolvers::<i32>();
            resolver.resolve(3);
            assert_eq!(ok(promise).await, 3);
        });
    }
}

mod async_iterables {
    use super::*;

    #[test]
    fn object_combinators() {
        async_test(|_| async move {
            let numbers = AsyncIterableObject::from_array(vec![1, 2, 3, 4]);
            let mapped = numbers.map(|n| n * 10).filter(|n| *n > 10);
            assert_eq!(ok(mapped.to_promise()).await, [20, 30, 40]);
            assert_eq!(ok(AsyncIterableObject::<i32>::empty().to_promise()).await, Vec::<i32>::new());

            let with_holes = AsyncIterableObject::from_array(vec![Some(1), None, Some(3)]);
            assert_eq!(ok(with_holes.coalesce().to_promise()).await, [1, 3]);

            let from_promise = AsyncIterableObject::from_promise(Promise::resolved(vec![7, 8]));
            assert_eq!(ok(from_promise.to_promise()).await, [7, 8]);

            // a ordem de resolução manda, não a ordem da lista
            let slow = async_::timeout(10).then(|_| 1);
            let fast = async_::timeout(1).then(|_| 2);
            let by_resolution = AsyncIterableObject::from_promises_resolve_order(vec![slow, fast]);
            assert_eq!(ok(by_resolution.to_promise()).await, [2, 1]);

            let merged = AsyncIterableObject::merge(vec![
                AsyncIterableObject::from_array(vec![1, 2]) as Rc<dyn AsyncIterable<i32>>,
                AsyncIterableObject::from_array(vec![3]) as Rc<dyn AsyncIterable<i32>>,
            ]);
            let mut all = ok(merged.to_promise()).await;
            all.sort_unstable();
            assert_eq!(all, [1, 2, 3]);
        });
    }

    #[test]
    fn object_rejection_reaches_every_iterator_and_late_emits_are_ignored() {
        async_test(|_| async move {
            let failing = AsyncIterableObject::<i32>::new(
                |emitter| {
                    emitter.emit_one(1);
                    emitter.reject(Error::new("falhou"));
                    emitter.emit_one(2); // depois do reject: sem efeito
                    MaybePromise::Value(())
                },
                None,
            );
            for _ in 0..2 {
                let mut seen = Vec::new();
                let outcome = for_await(&*failing, |item| {
                    seen.push(item);
                    Ok(ControlFlow::Continue(()))
                })
                .await;
                // o erro vem antes dos itens já emitidos: `next()` confere o estado de erro primeiro
                assert!(seen.is_empty());
                assert_eq!(outcome.expect_err("devia rejeitar").message, "falhou");
            }
        });
    }

    #[test]
    fn producer_combinators() {
        async_test(|_| async move {
            let numbers = AsyncIterableProducer::from_array(vec![1, 2, 3, 4]);
            let mapped = numbers.map(|n| n + 1).filter(|n| n % 2 == 0);
            assert_eq!(ok(async_::async_iterable_to_promise(mapped)).await, [2, 4]);

            let with_holes = AsyncIterableProducer::from_array(vec![Some(1), None, Some(3)]);
            assert_eq!(ok(async_::async_iterable_to_promise(with_holes.coalesce())).await, [1, 3]);

            let merged = AsyncIterableProducer::merge(vec![
                AsyncIterableProducer::from_array(vec![1]) as Rc<dyn AsyncIterable<i32>>,
                AsyncIterableProducer::from_promise(Promise::resolved(vec![2])) as Rc<dyn AsyncIterable<i32>>,
            ]);
            let mut all = ok(async_::async_iterable_to_promise(merged)).await;
            all.sort_unstable();
            assert_eq!(all, [1, 2]);
            assert_eq!(ok(async_::async_iterable_to_promise(AsyncIterableProducer::<i32>::empty())).await, Vec::<i32>::new());
        });
    }

    #[test]
    fn cancelable_producer_rejects_with_cancellation() {
        async_test(|_| async move {
            let (hold_promise, hold) = Promise::<()>::new();
            let producer = async_::create_cancelable_async_iterable_producer(move |_token| {
                let source = AsyncIterableProducer::<i32>::new(
                    move |emitter| {
                        emitter.emit_one(1);
                        MaybePromise::Promise(hold_promise)
                    },
                    None,
                );
                source as Rc<dyn AsyncIterable<i32>>
            });
            let iterator = producer.async_iterator();
            assert_eq!(ok(iterator.next()).await, Some(1));
            producer.cancel();
            let outcome = iterator.next().await;
            assert!(je_base::errors::is_cancellation_error(&outcome.expect_err("devia rejeitar")));
            hold.resolve(());
        });
    }
}
