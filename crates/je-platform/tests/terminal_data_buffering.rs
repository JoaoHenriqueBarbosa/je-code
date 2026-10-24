//! Porte de `src/vs/workbench/contrib/terminal/test/common/terminalDataBuffering.test.ts` (suíte `Workbench - TerminalDataBufferer`),
//! caso a caso e na mesma ordem.
//!
//! Notas de tradução:
//! - **`setup` e `ensureNoDisposablesAreLeakedInTestSuite()`.** O [`Fixture`] cria o guarda de vazamentos antes de tudo, os mapas
//!   `counter` e `data` e o `TerminalDataBufferer` (registrado no store) cujo callback conta as entregas e guarda o último texto.
//! - **`wait(ms)`.** É o `timeout` do `common`, no relógio real como no original (o caso roda em [`run_with_real_timers`]).
//! - **`bufferer.startBuffering(id, event, 0)`.** O Rust recebe também o `to_data` que extrai o texto de cada evento; como o
//!   evento é de `String`, é a própria cadeia.
//! - **`Map<number, number>.get(id)`.** É [`Fixture::count`] e [`Fixture::data`], que devolvem `None` no lugar de `undefined`.

#[path = "../../je-base/tests/common/mod.rs"]
mod common;

use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

use common::{LeakGuard, ensure_no_disposables_are_leaked_in_test_suite, run_with_real_timers, timeout};
use je_base::event::Emitter;
use je_base::lifecycle::IDisposable;
use je_platform::terminal::terminal_data_buffering::TerminalDataBufferer;

struct Fixture {
    counter: Rc<RefCell<HashMap<u32, u32>>>,
    data: Rc<RefCell<HashMap<u32, String>>>,
    bufferer: Rc<TerminalDataBufferer>,
    /// O guarda é o último campo: é descartado depois dos demais.
    store: LeakGuard,
}

impl Fixture {
    fn new() -> Self {
        let store = ensure_no_disposables_are_leaked_in_test_suite();
        let counter: Rc<RefCell<HashMap<u32, u32>>> = Rc::default();
        let data: Rc<RefCell<HashMap<u32, String>>> = Rc::default();
        let (counter_sink, data_sink) = (counter.clone(), data.clone());
        let bufferer = store.add_rc(Rc::new(TerminalDataBufferer::new(move |id, e| {
            *counter_sink.borrow_mut().entry(id).or_insert(0) += 1;
            data_sink.borrow_mut().insert(id, e.to_string());
        })));
        Self { counter, data, bufferer, store }
    }

    fn count(&self, id: u32) -> Option<u32> {
        self.counter.borrow().get(&id).copied()
    }

    fn data(&self, id: u32) -> Option<String> {
        self.data.borrow().get(&id).cloned()
    }

    fn start(&self, id: u32, on_data: &Emitter<String>) -> je_base::lifecycle::DisposableRef {
        self.bufferer.start_buffering(id, &on_data.event(), |e: &String| e.clone(), Some(0))
    }
}

fn fire_all(emitter: &Emitter<String>, values: &[&str]) {
    for value in values {
        emitter.fire((*value).to_string());
    }
}

#[test]
fn start() {
    run_with_real_timers(async {
        let f = Fixture::new();
        let terminal_on_data = Emitter::<String>::new();

        f.store.add(f.start(1, &terminal_on_data));

        fire_all(&terminal_on_data, &["1", "2", "3"]);

        timeout(0).await;

        terminal_on_data.fire("4".to_string());

        assert_eq!(f.count(1), Some(1));
        assert_eq!(f.data(1).as_deref(), Some("123"));

        timeout(0).await;

        assert_eq!(f.count(1), Some(2));
        assert_eq!(f.data(1).as_deref(), Some("4"));
    });
}

#[test]
fn start_2() {
    run_with_real_timers(async {
        let f = Fixture::new();
        let terminal1_on_data = Emitter::<String>::new();
        let terminal2_on_data = Emitter::<String>::new();

        f.store.add(f.start(1, &terminal1_on_data));
        f.store.add(f.start(2, &terminal2_on_data));

        terminal1_on_data.fire("1".to_string());
        terminal2_on_data.fire("4".to_string());
        terminal1_on_data.fire("2".to_string());
        terminal2_on_data.fire("5".to_string());
        terminal1_on_data.fire("3".to_string());
        terminal2_on_data.fire("6".to_string());
        terminal2_on_data.fire("7".to_string());

        assert_eq!(f.count(1), None);
        assert_eq!(f.data(1), None);
        assert_eq!(f.count(2), None);
        assert_eq!(f.data(2), None);

        timeout(0).await;

        assert_eq!(f.count(1), Some(1));
        assert_eq!(f.data(1).as_deref(), Some("123"));
        assert_eq!(f.count(2), Some(1));
        assert_eq!(f.data(2).as_deref(), Some("4567"));
    });
}

#[test]
fn stop() {
    run_with_real_timers(async {
        let f = Fixture::new();
        let terminal_on_data = Emitter::<String>::new();

        f.start(1, &terminal_on_data);

        fire_all(&terminal_on_data, &["1", "2", "3"]);

        f.bufferer.stop_buffering(1);
        timeout(0).await;

        assert_eq!(f.count(1), Some(1));
        assert_eq!(f.data(1).as_deref(), Some("123"));
    });
}

#[test]
fn start_2_stop_1() {
    run_with_real_timers(async {
        let f = Fixture::new();
        let terminal1_on_data = Emitter::<String>::new();
        let terminal2_on_data = Emitter::<String>::new();

        f.start(1, &terminal1_on_data);
        f.store.add(f.start(2, &terminal2_on_data));

        terminal1_on_data.fire("1".to_string());
        terminal2_on_data.fire("4".to_string());
        terminal1_on_data.fire("2".to_string());
        terminal2_on_data.fire("5".to_string());
        terminal1_on_data.fire("3".to_string());
        terminal2_on_data.fire("6".to_string());
        terminal2_on_data.fire("7".to_string());

        assert_eq!(f.count(1), None);
        assert_eq!(f.data(1), None);
        assert_eq!(f.count(2), None);
        assert_eq!(f.data(2), None);

        f.bufferer.stop_buffering(1);
        timeout(0).await;

        assert_eq!(f.count(1), Some(1));
        assert_eq!(f.data(1).as_deref(), Some("123"));
        assert_eq!(f.count(2), Some(1));
        assert_eq!(f.data(2).as_deref(), Some("4567"));
    });
}

#[test]
fn dispose_should_flush_remaining_data_events() {
    run_with_real_timers(async {
        let f = Fixture::new();
        let terminal1_on_data = Emitter::<String>::new();
        let terminal2_on_data = Emitter::<String>::new();

        f.store.add(f.start(1, &terminal1_on_data));
        f.store.add(f.start(2, &terminal2_on_data));

        terminal1_on_data.fire("1".to_string());
        terminal2_on_data.fire("4".to_string());
        terminal1_on_data.fire("2".to_string());
        terminal2_on_data.fire("5".to_string());
        terminal1_on_data.fire("3".to_string());
        terminal2_on_data.fire("6".to_string());
        terminal2_on_data.fire("7".to_string());

        assert_eq!(f.count(1), None);
        assert_eq!(f.data(1), None);
        assert_eq!(f.count(2), None);
        assert_eq!(f.data(2), None);

        f.bufferer.dispose();
        timeout(0).await;

        assert_eq!(f.count(1), Some(1));
        assert_eq!(f.data(1).as_deref(), Some("123"));
        assert_eq!(f.count(2), Some(1));
        assert_eq!(f.data(2).as_deref(), Some("4567"));
    });
}
