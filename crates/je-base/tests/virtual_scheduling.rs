//! Porte de `src/vs/base/test/common/virtualScheduling/virtualScheduling.test.ts` (suítes
//! `virtualScheduling - ...`), caso a caso e na mesma ordem.
//!
//! Notas de tradução:
//! - Os testes `async` rodam no loop real (`run_test`: `spawn_local` mais `runtime::run_until_idle`),
//!   porque o `runWithFakedTimers` novo troca a `TimeApi` do loop e o processador cede a vez ao host pelos
//!   timers nativos dele. Não usam `common::run_with_faked_timers`, que é o `runtime::with_faked_timers`
//!   antigo (o relógio do próprio loop); aqui o relógio é o `VirtualClock`.
//! - `setTimeout`/`setInterval`/`requestAnimationFrame`/`performance` globais são `runtime::set_timeout`
//!   e `runtime::time_api()`; `Date.now()` é `runtime::date_now()`.
//! - `Promise<T>` do original é `je_base::promise::Promise<T>`, esperada com `await` (devolve
//!   `Result<T, Error>`); `deferred<T>()` é `Promise::new()`.
//! - `assert.throws`/`assert.rejects` com regex viram `errors::catch`/`unwrap_err` e `contains` na
//!   mensagem.
//! - `ensureNoDisposablesAreLeakedInTestSuite()` e o `teardown(() => TraceContext.instance._resetForTesting())`
//!   são `suite()`: um `LeakGuard` e um guarda que reseta o contexto de trace no `Drop`.
//! - O teste "recorded timers retain their scheduling stack" e o "timer source and trace retain their
//!   scheduling site" procuram o nome da função que agendou (`schedule_recorded_timer`,
//!   `schedule_timers`) na pilha capturada, que precisa de símbolos (o perfil de teste os tem) e de
//!   `#[inline(never)]` nessas funções.
//! - O teste "Trace formats captured stacks only when requested" conta as leituras da pilha com um
//!   `StackSource` cujo fecho incrementa um contador (o `Object.defineProperty(error, 'stack', { get })`).
//! - O teste "virtual wall and monotonic clocks stay consistent" troca o `performance.now ===
//!   originalPerformanceNow` e a comparação dos descritores de `timeOrigin` pela identidade da `TimeApi`
//!   restaurada e pelo valor de `performance_time_origin`, porque aqui o "global" que se restaura é a API
//!   instalada.

mod common;

use std::cell::{Cell, RefCell};
use std::future::Future;
use std::rc::Rc;

use common::{LeakGuard, ensure_no_disposables_are_leaked_in_test_suite, fail_on_unexpected_errors};
use je_base::async_::async_fn;
use je_base::cancellation::CancellationTokenSource;
use je_base::errors::{self, Error};
use je_base::lifecycle::{DisposableStore, IDisposable, StackSource};
use je_base::promise::{Promise, Resolver};
use je_base::runtime;
use je_base::virtual_scheduling::time_api::{IntervalId, TimeApi};
use je_base::virtual_scheduling::{
    CreateVirtualTimeApiOptions, EventSource, RecordedHistory, RunAsHandlerOptions, RunOptions, RunWithFakedTimersOptions, Trace,
    TraceContext, VirtualClock, VirtualEvent, VirtualTimeProcessor, VirtualTimeProcessorOptions, create_recording_real_time_api,
    create_trace_root, create_virtual_time_api, drain_microtasks_embedding, next_macrotask, push_global_time_api, real_time_api,
    run_with_faked_timers, until_idle, until_time, until_token,
};

/// O `teardown(() => TraceContext.instance._resetForTesting())`.
struct ResetTrace;

impl Drop for ResetTrace {
    fn drop(&mut self) {
        TraceContext::instance().reset_for_testing();
    }
}

/// `ensureNoDisposablesAreLeakedInTestSuite()` mais o `teardown` que reseta o contexto de trace.
fn suite() -> (LeakGuard, ResetTrace) {
    (ensure_no_disposables_are_leaked_in_test_suite(), ResetTrace)
}

/// Roda um teste `async` no loop real até não sobrar nada, e reprova se ele não terminou.
fn run_test(body: impl Future<Output = ()> + 'static) {
    let done = Rc::new(Cell::new(false));
    let finished = done.clone();
    runtime::spawn_local(async move {
        body.await;
        finished.set(true);
    });
    runtime::run_until_idle();
    fail_on_unexpected_errors();
    assert!(done.get(), "o teste assíncrono não terminou");
}

#[derive(Debug, PartialEq)]
struct TraceInfo {
    labels: Vec<String>,
    root_label: String,
    depth: usize,
}

fn trace_info(t: &Rc<Trace>) -> TraceInfo {
    let mut labels = Vec::new();
    let mut current = Some(t.clone());
    while let Some(trace) = current {
        labels.push(trace.label.clone());
        current = trace.parent.clone();
    }
    TraceInfo { labels, root_label: t.root().label.clone(), depth: t.depth }
}

fn info(labels: &[&str], root_label: &str, depth: usize) -> TraceInfo {
    TraceInfo { labels: labels.iter().map(|l| (*l).to_owned()).collect(), root_label: root_label.to_owned(), depth }
}

/// `realSink`.
fn real_sink() -> RunAsHandlerOptions {
    RunAsHandlerOptions::new(|cb| next_macrotask(&*real_time_api(), cb))
}

type Log = Rc<RefCell<Vec<&'static str>>>;

/// `clock.schedule({ time, source: { toString: () => source }, run: () => log.push(entry) })`.
fn schedule_log(clock: &VirtualClock, time: u64, source: &str, log: &Log, entry: &'static str) {
    let log = log.clone();
    clock.schedule(VirtualEvent::new(time, EventSource::new(source), move || log.borrow_mut().push(entry)));
}

/// Os dois pontos da identidade de uma API de tempo (o `===` do original).
fn same_api(a: &Rc<dyn TimeApi>, b: &Rc<dyn TimeApi>) -> bool {
    std::ptr::addr_eq(Rc::as_ptr(a), Rc::as_ptr(b))
}

// virtualScheduling - Trace + TraceContext

#[test]
fn trace_describe_builds_causal_chain_from_leaf_to_root() {
    let _suite = suite();
    let root = create_trace_root("fixture", None);
    let t1 = root.child("setTimeout(100ms)", None);
    let t2 = t1.child("await continuation", None);
    assert_eq!(trace_info(&t2), info(&["await continuation", "setTimeout(100ms)", "fixture"], "fixture", 2));
}

#[test]
fn trace_formats_captured_stacks_only_when_requested() {
    let _suite = suite();
    let reads = Rc::new(Cell::new(0_u32));
    let counter = reads.clone();
    let error = StackSource::new(move || {
        counter.set(counter.get() + 1);
        "captured stack".to_owned()
    });
    let root = create_trace_root("root", Some(StackSource::new(|| "root stack".to_owned())));
    let trace = root.child("timer", Some(error));
    trace.describe();
    let reads_before_access = reads.get();

    let root_stack = root.stack();
    let stack = trace.stack();
    let stack_reads = reads.get();
    let child_stack = trace.child("tick", None).stack();
    assert_eq!(
        (reads_before_access, root_stack, stack, stack_reads, child_stack),
        (0, Some("root stack".to_owned()), Some("captured stack".to_owned()), 1, None)
    );
}

#[inline(never)]
fn schedule_recorded_timer(api: &dyn TimeApi, resolver: Resolver<()>) {
    api.set_timeout(Box::new(move || resolver.resolve(())), 0);
}

#[test]
fn recorded_timers_retain_their_scheduling_stack_after_running() {
    let _suite = suite();
    let history: RecordedHistory = Rc::new(RefCell::new(Vec::new()));
    let api = create_recording_real_time_api(history.clone());
    run_test(async move {
        Promise::<()>::with_executor(|resolver| schedule_recorded_timer(&*api, resolver)).await.unwrap();
    });

    let observed: Vec<(String, bool, bool)> = history
        .borrow()
        .iter()
        .map(|event| {
            (
                event.source.to_string(),
                event.source.stack_trace().is_some_and(|stack| stack.contains("schedule_recorded_timer")),
                event.source.stack_trace() == event.trace.as_ref().and_then(|trace| trace.stack()),
            )
        })
        .collect();
    assert_eq!(observed, vec![("setTimeout".to_owned(), true, true)]);
}

#[test]
fn run_with_trace_installs_and_restores_synchronously_and_supports_nesting() {
    let _suite = suite();
    let a = create_trace_root("a", None);
    let b = create_trace_root("b", None);
    let observations = RefCell::new(Vec::<String>::new());
    let observe = || observations.borrow_mut().push(TraceContext::instance().current_trace().label.clone());
    observe();
    TraceContext::instance().run_with_trace(a, || {
        observe();
        TraceContext::instance().run_with_trace(b, observe);
        observe();
    });
    observe();
    assert_eq!(observations.into_inner(), ["<root>", "a", "b", "a", "<root>"]);
}

#[test]
fn run_as_handler_throws_on_sync_re_entry() {
    let _suite = suite();
    let a = create_trace_root("a", None);
    let b = create_trace_root("b", None);
    let sink = real_sink();
    let context = TraceContext::instance();
    let error = errors::catch(|| context.run_as_handler(a, || context.run_as_handler(b, || {}, &sink), &sink)).unwrap_err();
    assert!(error.message.contains("re-entrant"), "{error}");
}

#[test]
fn run_as_handler_leaks_trace_across_awaited_microtasks() {
    let _suite = suite();
    let root = create_trace_root("fixture", None);
    let observed = Rc::new(RefCell::new(Vec::<String>::new()));
    let sink = real_sink();
    let log = observed.clone();
    run_test(async move {
        let promise = TraceContext::instance().run_as_handler(
            root,
            move || {
                async_fn(async move {
                    let observe = || log.borrow_mut().push(TraceContext::instance().current_trace().label.clone());
                    observe();
                    Promise::<()>::resolved(()).await?;
                    observe();
                    Promise::<()>::resolved(()).then_promise(|()| Promise::<()>::resolved(())).await?;
                    observe();
                    Ok::<(), Error>(())
                })
            },
            &sink,
        );
        promise.await.unwrap();
    });
    assert_eq!(*observed.borrow(), ["fixture", "fixture", "fixture"]);
}

// virtualScheduling - createVirtualTimeApi trace propagation

#[inline(never)]
fn schedule_timers(api: &dyn TimeApi) {
    api.set_timeout(Box::new(|| {}), 5);
    api.set_interval(Rc::new(|| {}), 10);
    assert!(api.request_animation_frame(Box::new(|_| {})).is_ok());
}

#[test]
fn timer_source_and_trace_retain_their_scheduling_site() {
    let _suite = suite();
    let clock = Rc::new(VirtualClock::default());
    let api = create_virtual_time_api(clock.clone(), CreateVirtualTimeApiOptions { fake_request_animation_frame: true });
    schedule_timers(&*api);

    let observed: Vec<(String, bool, bool)> = clock
        .get_events()
        .iter()
        .map(|event| {
            let trace_stack = event.trace.as_ref().and_then(|trace| trace.stack().or_else(|| trace.parent.as_ref().and_then(|p| p.stack())));
            (
                event.source.to_string(),
                event.source.stack_trace().is_some_and(|stack| stack.contains("schedule_timers")),
                event.source.stack_trace() == trace_stack,
            )
        })
        .collect();
    assert_eq!(
        observed,
        vec![
            ("setTimeout".to_owned(), true, true),
            ("setInterval (iteration 1)".to_owned(), true, true),
            ("requestAnimationFrame".to_owned(), true, true),
        ]
    );
}

#[test]
fn virtual_set_timeout_callback_fires_under_trace_child_of_schedule_time_trace() {
    let _suite = suite();
    run_test(async {
        run_with_faked_timers(RunWithFakedTimersOptions::default(), || async {
            let root = create_trace_root("root", None);
            let (promise, resolver) = Promise::<Rc<Trace>>::new();
            TraceContext::instance().run_as_handler(
                root,
                || {
                    runtime::set_timeout(move || resolver.resolve(TraceContext::instance().current_trace()), 0);
                },
                &real_sink(),
            );
            let observed = promise.await?;
            assert_eq!(trace_info(&observed), info(&["setTimeout(0ms)", "root"], "root", 1));
            Ok::<(), Error>(())
        })
        .await
        .unwrap();
    });
}

#[test]
fn virtual_nested_set_timeout_preserves_full_causal_chain() {
    let _suite = suite();
    run_test(async {
        run_with_faked_timers(RunWithFakedTimersOptions::default(), || async {
            let root = create_trace_root("root", None);
            let (promise, resolver) = Promise::<Rc<Trace>>::new();
            TraceContext::instance().run_as_handler(
                root,
                || {
                    runtime::set_timeout(
                        move || {
                            runtime::set_timeout(move || resolver.resolve(TraceContext::instance().current_trace()), 0);
                        },
                        0,
                    );
                },
                &real_sink(),
            );
            let observed = promise.await?;
            assert_eq!(trace_info(&observed), info(&["setTimeout(0ms)", "setTimeout(0ms)", "root"], "root", 2));
            Ok::<(), Error>(())
        })
        .await
        .unwrap();
    });
}

#[test]
fn virtual_set_interval_each_tick_gets_a_fresh_child_trace() {
    let _suite = suite();
    run_test(async {
        run_with_faked_timers(RunWithFakedTimersOptions::default(), || async {
            let root = create_trace_root("root", None);
            let observed = Rc::new(RefCell::new(Vec::<Rc<Trace>>::new()));
            let (promise, resolver) = Promise::<()>::new();
            let interval_id = Rc::new(Cell::new(None::<IntervalId>));
            let tick = {
                let observed = observed.clone();
                let interval_id = interval_id.clone();
                Rc::new(move || {
                    observed.borrow_mut().push(TraceContext::instance().current_trace());
                    if observed.borrow().len() == 3 {
                        if let Some(id) = interval_id.get() {
                            runtime::time_api().clear_interval(id);
                        }
                        resolver.resolve(());
                    }
                })
            };
            TraceContext::instance().run_as_handler(
                root,
                || interval_id.set(Some(runtime::time_api().set_interval(tick, 5))),
                &real_sink(),
            );
            promise.await?;
            let infos: Vec<TraceInfo> = observed.borrow().iter().map(trace_info).collect();
            assert_eq!(
                infos,
                vec![
                    info(&["tick #1", "setInterval(5ms)", "root"], "root", 2),
                    info(&["tick #2", "setInterval(5ms)", "root"], "root", 2),
                    info(&["tick #3", "setInterval(5ms)", "root"], "root", 2),
                ]
            );
            Ok::<(), Error>(())
        })
        .await
        .unwrap();
    });
}

#[test]
fn concurrent_run_as_handler_via_set_timeout_0_traces_do_not_leak_across_handlers() {
    let _suite = suite();
    run_test(async {
        run_with_faked_timers(RunWithFakedTimersOptions::default(), || async {
            let a = create_trace_root("a", None);
            let b = create_trace_root("b", None);
            let (done_a, resolve_a) = Promise::<Rc<Trace>>::new();
            let (done_b, resolve_b) = Promise::<Rc<Trace>>::new();
            TraceContext::instance().run_as_handler(
                a,
                || {
                    runtime::set_timeout(move || resolve_a.resolve(TraceContext::instance().current_trace()), 0);
                },
                &real_sink(),
            );
            TraceContext::instance().run_as_handler(
                b,
                || {
                    runtime::set_timeout(move || resolve_b.resolve(TraceContext::instance().current_trace()), 0);
                },
                &real_sink(),
            );
            let traces = Promise::all(vec![done_a, done_b]).await?;
            let (t_a, t_b) = (&traces[0], &traces[1]);
            assert_eq!(
                (t_a.root().label.clone(), trace_info(t_a).labels, t_b.root().label.clone(), trace_info(t_b).labels),
                ("a".to_owned(), vec!["setTimeout(0ms)".to_owned(), "a".to_owned()], "b".to_owned(), vec!["setTimeout(0ms)".to_owned(), "b".to_owned()])
            );
            Ok::<(), Error>(())
        })
        .await
        .unwrap();
    });
}

// virtualScheduling - VirtualTimeProcessor termination policies

fn make_processor(store: &DisposableStore, clock: &Rc<VirtualClock>) -> Rc<VirtualTimeProcessor> {
    store.add_rc(VirtualTimeProcessor::new(
        clock.clone(),
        drain_microtasks_embedding(real_time_api()),
        real_time_api(),
        VirtualTimeProcessorOptions { default_max_events: Some(50) },
    ))
}

#[test]
fn until_idle_resolves_when_queue_drains() {
    let _suite = suite();
    let store = DisposableStore::new();
    let clock = Rc::new(VirtualClock::default());
    let p = make_processor(&store, &clock);
    let log = Log::default();

    schedule_log(&clock, 5, "t1", &log, "a");
    schedule_log(&clock, 10, "t2", &log, "b");

    run_test(async move {
        p.run(RunOptions::new(until_idle())).await.unwrap();
        assert_eq!(*log.borrow(), ["a", "b"]);
        store.dispose();
    });
}

#[test]
fn until_time_resolves_at_deadline_even_when_no_events_scheduled() {
    // O prazo sozinho, sem outro evento na fila, ainda tem de levar o tempo virtual até o prazo. O
    // processador insere um evento sentinela no prazo para garantir isso.
    let _suite = suite();
    let store = DisposableStore::new();
    let clock = Rc::new(VirtualClock::default());
    let p = make_processor(&store, &clock);

    run_test({
        let clock = clock.clone();
        async move {
            p.run(RunOptions::new(until_time(100))).await.unwrap();
            assert_eq!(clock.now(), 100);
            store.dispose();
        }
    });
}

#[test]
fn until_time_pre_scheduled_events_run_before_deadline() {
    let _suite = suite();
    let store = DisposableStore::new();
    let clock = Rc::new(VirtualClock::default());
    let p = make_processor(&store, &clock);
    let log = Log::default();

    schedule_log(&clock, 50, "t", &log, "a");

    run_test({
        let clock = clock.clone();
        async move {
            p.run(RunOptions::new(until_time(100))).await.unwrap();
            assert_eq!((log.borrow().clone(), clock.now()), (vec!["a"], 100));
            store.dispose();
        }
    });
}

#[test]
fn until_time_events_strictly_past_the_deadline_are_not_executed() {
    let _suite = suite();
    let store = DisposableStore::new();
    let clock = Rc::new(VirtualClock::default());
    let p = make_processor(&store, &clock);
    let log = Log::default();

    schedule_log(&clock, 50, "a", &log, "a");
    schedule_log(&clock, 100, "b", &log, "b");
    schedule_log(&clock, 101, "c", &log, "c");

    run_test(async move {
        p.run(RunOptions::new(until_time(100))).await.unwrap();
        assert_eq!(*log.borrow(), ["a", "b"]);
        store.dispose();
    });
}

#[test]
fn until_token_resolves_only_after_token_cancellation_and_drain() {
    let _suite = suite();
    let store = DisposableStore::new();
    let clock = Rc::new(VirtualClock::default());
    let p = make_processor(&store, &clock);
    let cts = Rc::new(CancellationTokenSource::new(None));
    store.add_rc(cts.clone());
    let log = Log::default();

    run_test(async move {
        let run_p = p.run(RunOptions::new(until_token(cts.token())));

        // Enquanto o run está estacionado (sem eventos), agenda e cancela.
        Promise::<()>::resolved(()).await.unwrap();
        schedule_log(&clock, 5, "t", &log, "a");
        cts.cancel();

        run_p.await.unwrap();
        assert_eq!(*log.borrow(), ["a"]);
        store.dispose();
    });
}

/// O timer que se reagenda para sempre (`tick(n)` do original). Guarda o relógio de forma fraca, para a
/// fila que o guarda não formar ciclo com ele.
fn tick(clock: &Rc<VirtualClock>, n: u32) {
    let next = Rc::downgrade(clock);
    clock.schedule(VirtualEvent::new(clock.now() + 1, EventSource::new(format!("t{n}")), move || {
        if let Some(clock) = next.upgrade() {
            tick(&clock, n + 1);
        }
    }));
}

#[test]
fn max_events_rejects_when_too_many_events_are_executed() {
    let _suite = suite();
    let store = DisposableStore::new();
    let clock = Rc::new(VirtualClock::default());
    let p = make_processor(&store, &clock);

    // Timer que se reagenda.
    tick(&clock, 0);

    run_test(async move {
        let error = p.run(RunOptions::new(until_idle()).with_max_events(5)).await.unwrap_err();
        assert!(error.message.contains("exceeded maxEvents"), "{error}");
        store.dispose();
    });
}

#[test]
fn disposal_rejects_all_active_runs() {
    let _suite = suite();
    let store = DisposableStore::new();
    let clock = Rc::new(VirtualClock::default());
    let p = make_processor(&store, &clock);
    let cts = Rc::new(CancellationTokenSource::new(None));
    store.add_rc(cts.clone());

    run_test(async move {
        let run_p = p.run(RunOptions::new(until_token(cts.token())));
        p.dispose();

        let error = run_p.await.unwrap_err();
        assert!(error.message.contains("disposed"), "{error}");
        store.dispose();
    });
}

// virtualScheduling - runWithFakedTimers

#[test]
fn run_with_faked_timers_drains_queue_after_fn_resolves() {
    let _suite = suite();
    let log = Log::default();
    let timers_log = log.clone();
    run_test(async move {
        // O original assere que o esvaziamento roda depois de `fn()` resolver, o que só se observa fora
        // da chamada.
        run_with_faked_timers(RunWithFakedTimersOptions::default(), move || async move {
            let slow = timers_log.clone();
            runtime::set_timeout(move || slow.borrow_mut().push("a"), 100);
            runtime::set_timeout(move || timers_log.borrow_mut().push("b"), 50);
            Ok::<(), Error>(())
        })
        .await
        .unwrap();
        assert_eq!(*log.borrow(), ["b", "a"]);
    });
}

#[test]
fn run_with_faked_timers_use_fake_timers_false_bypasses_virtual_time() {
    let _suite = suite();
    run_test(async {
        let before = runtime::time_api();
        let options = RunWithFakedTimersOptions { use_fake_timers: Some(false), ..RunWithFakedTimersOptions::default() };
        run_with_faked_timers(options, move || async move {
            assert!(same_api(&runtime::time_api(), &before));
            Ok::<(), Error>(())
        })
        .await
        .unwrap();
    });
}

#[test]
fn run_with_faked_timers_promise_chains_awaited_inside_fn_resolve_deterministically() {
    let _suite = suite();
    let log = Log::default();
    let timers_log = log.clone();
    run_test(async move {
        run_with_faked_timers(RunWithFakedTimersOptions::default(), move || async move {
            Promise::<()>::with_executor(move |resolve| {
                runtime::set_timeout(
                    move || {
                        runtime::spawn_local(async move {
                            timers_log.borrow_mut().push("1");
                            Promise::<()>::resolved(()).await.unwrap();
                            timers_log.borrow_mut().push("2");
                            runtime::set_timeout(
                                move || {
                                    timers_log.borrow_mut().push("3");
                                    resolve.resolve(());
                                },
                                10,
                            );
                        });
                    },
                    5,
                );
            })
            .await?;
            Ok::<(), Error>(())
        })
        .await
        .unwrap();
        assert_eq!(*log.borrow(), ["1", "2", "3"]);
    });
}

// virtualScheduling - createVirtualTimeApi without processor

#[test]
fn virtual_wall_and_monotonic_clocks_stay_consistent() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    let clock = Rc::new(VirtualClock::new(12345));
    let api = create_virtual_time_api(clock.clone(), CreateVirtualTimeApiOptions { fake_request_animation_frame: true });
    let original = runtime::time_api();
    let original_time_origin = original.performance_time_origin();
    let restore = push_global_time_api(api);
    let animation_frame_time = Rc::new(Cell::new(None::<f64>));
    let frame = animation_frame_time.clone();
    let scheduled = runtime::time_api().request_animation_frame(Box::new(move |time| frame.set(Some(time))));
    assert!(scheduled.is_ok());
    clock.run_next();
    let actual = (
        runtime::date_now(),
        runtime::time_api().performance_now(),
        runtime::time_api().performance_time_origin(),
        animation_frame_time.get(),
    );
    restore.dispose();

    assert_eq!(actual, (12361, 16.0, 12345.0, Some(16.0)));
    assert!(same_api(&runtime::time_api(), &original), "a API restaurada não é a original");
    assert_eq!(runtime::time_api().performance_time_origin(), original_time_origin);
}
