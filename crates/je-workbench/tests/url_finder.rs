//! Porte de `src/vs/workbench/contrib/remote/test/browser/urlFinder.test.ts` (suíte `UrlFinder`), caso a caso e na mesma
//! ordem.
//!
//! Notas de tradução:
//! - `ensureNoDisposablesAreLeakedInTestSuite()` vira o guarda criado no começo de cada caso.
//! - Os casos esperam o debounce de verdade (`setTimeout` sem relógio falso), então rodam em `run_with_real_timers`, e
//!   `await new Promise(resolve => setTimeout(resolve, DEBOUNCE_WAIT_MS))` é `common::timeout(DEBOUNCE_WAIT_MS)`.
//! - Os literais `as unknown as ITerminalService`/`IDebugService` são structs que implementam os contratos estreitos do
//!   módulo de produção (ver as divergências dele), com só os eventos que o original define.
//! - `MockTerminalInstance` é a struct de mesmo nome; o `ITerminalInstance` do original só precisa de `onData` e `title`, e
//!   o `instanceId` fixo substitui a identidade do objeto.
//! - A preparação que os cinco casos repetem (store, instância, serviços, `UrlFinder` e a lista de URLs achadas) é
//!   `create_fixture`.

#[path = "../../je-base/tests/common/mod.rs"]
mod common;

use std::cell::RefCell;
use std::rc::Rc;

use common::{LeakGuard, ensure_no_disposables_are_leaked_in_test_suite, run_with_real_timers, timeout};
use je_base::event::{Emitter, Event};
use je_base::lifecycle::{DisposableStore, IDisposable};
use je_workbench::contrib::remote::url_finder::{
    LocalUrl, UrlFinder, UrlFinderDebugService, UrlFinderDebugSession, UrlFinderTerminalInstance, UrlFinderTerminalService,
};

/// Wait time for debounce tests - should be greater than the UrlFinder debounce timeout (500ms)
const DEBOUNCE_WAIT_MS: u64 = 600;

/// `MockTerminalInstance`.
struct MockTerminalInstance {
    on_data: Rc<Emitter<String>>,
}

impl MockTerminalInstance {
    fn new() -> Rc<Self> {
        Rc::new(Self { on_data: Emitter::new() })
    }

    fn fire_data(&self, data: &str) {
        self.on_data.fire(data.to_string());
    }
}

impl IDisposable for MockTerminalInstance {
    fn dispose(&self) {
        self.on_data.dispose();
    }
}

impl UrlFinderTerminalInstance for MockTerminalInstance {
    fn instance_id(&self) -> u32 {
        1
    }
    fn title(&self) -> String {
        "test-terminal".to_string()
    }
    fn on_data(&self) -> Event<String> {
        self.on_data.event()
    }
}

/// `createMockTerminalService`.
struct MockTerminalService {
    instances: Vec<Rc<dyn UrlFinderTerminalInstance>>,
    on_did_create_instance: Rc<Emitter<Rc<dyn UrlFinderTerminalInstance>>>,
    on_did_dispose_instance: Rc<Emitter<Rc<dyn UrlFinderTerminalInstance>>>,
}

impl UrlFinderTerminalService for MockTerminalService {
    fn instances(&self) -> Vec<Rc<dyn UrlFinderTerminalInstance>> {
        self.instances.clone()
    }
    fn on_did_create_instance(&self) -> Event<Rc<dyn UrlFinderTerminalInstance>> {
        self.on_did_create_instance.event()
    }
    fn on_did_dispose_instance(&self) -> Event<Rc<dyn UrlFinderTerminalInstance>> {
        self.on_did_dispose_instance.event()
    }
}

fn create_mock_terminal_service(instances: Vec<Rc<dyn UrlFinderTerminalInstance>>, local_store: &Rc<DisposableStore>) -> Rc<dyn UrlFinderTerminalService> {
    let on_did_create_instance = Emitter::new();
    local_store.add(on_did_create_instance.clone());
    let on_did_dispose_instance = Emitter::new();
    local_store.add(on_did_dispose_instance.clone());
    Rc::new(MockTerminalService { instances, on_did_create_instance, on_did_dispose_instance })
}

/// `createMockDebugService`.
struct MockDebugService {
    on_did_new_session: Rc<Emitter<Rc<dyn UrlFinderDebugSession>>>,
    on_did_end_session: Rc<Emitter<Rc<dyn UrlFinderDebugSession>>>,
}

impl UrlFinderDebugService for MockDebugService {
    fn on_did_new_session(&self) -> Event<Rc<dyn UrlFinderDebugSession>> {
        self.on_did_new_session.event()
    }
    fn on_did_end_session(&self) -> Event<Rc<dyn UrlFinderDebugSession>> {
        self.on_did_end_session.event()
    }
}

fn create_mock_debug_service(local_store: &Rc<DisposableStore>) -> Rc<dyn UrlFinderDebugService> {
    let on_did_new_session = Emitter::new();
    local_store.add(on_did_new_session.clone());
    let on_did_end_session = Emitter::new();
    local_store.add(on_did_end_session.clone());
    Rc::new(MockDebugService { on_did_new_session, on_did_end_session })
}

struct Fixture {
    mock_instance: Rc<MockTerminalInstance>,
    matched_urls: Rc<RefCell<Vec<LocalUrl>>>,
}

fn create_fixture(ds: &LeakGuard) -> Fixture {
    let store = DisposableStore::new();
    ds.add(store.clone());
    let mock_instance = MockTerminalInstance::new();
    store.add(mock_instance.clone());
    let terminal_service = create_mock_terminal_service(vec![mock_instance.clone()], &store);
    let debug_service = create_mock_debug_service(&store);

    let url_finder = UrlFinder::new(&terminal_service, &debug_service);
    store.add(url_finder.clone());

    let matched_urls: Rc<RefCell<Vec<LocalUrl>>> = Rc::new(RefCell::new(Vec::new()));
    let sink = matched_urls.clone();
    store.add(url_finder.on_did_match_local_url().listen(move |url| sink.borrow_mut().push(url.clone())));

    Fixture { mock_instance, matched_urls }
}

fn url(host: &str, port: u64) -> LocalUrl {
    LocalUrl { host: host.to_string(), port }
}

#[test]
fn should_debounce_terminal_data_processing() {
    let ds = ensure_no_disposables_are_leaked_in_test_suite();
    let Fixture { mock_instance, matched_urls } = create_fixture(&ds);

    run_with_real_timers(async move {
        // Fire data events rapidly - data is accumulated and processed together after debounce
        mock_instance.fire_data("http://localhost:3000/\n");
        mock_instance.fire_data("http://127.0.0.1:8080/\n");

        // Initially, no matches should be processed (debounced)
        assert_eq!(matched_urls.borrow().len(), 0, "URLs should not be processed immediately");

        // Wait for debounce timeout
        timeout(DEBOUNCE_WAIT_MS).await.unwrap();

        // Both URLs should be processed after debounce
        let matched = matched_urls.borrow();
        assert_eq!(matched.len(), 2, "Both URLs should be processed after debounce");
        assert_eq!(matched[0].host, "localhost");
        assert_eq!(matched[0].port, 3000);
        assert_eq!(matched[1].host, "127.0.0.1");
        assert_eq!(matched[1].port, 8080);
    });
}

#[test]
fn should_skip_processing_when_data_exceeds_threshold() {
    let ds = ensure_no_disposables_are_leaked_in_test_suite();
    let Fixture { mock_instance, matched_urls } = create_fixture(&ds);

    run_with_real_timers(async move {
        // Fire a valid URL
        mock_instance.fire_data("http://localhost:3000/");

        // Then flood with lots of data (simulating high-throughput like games)
        // Generate 10001 characters to exceed the 10000 character threshold (uses > comparison)
        let large_data = "x".repeat(10001);
        mock_instance.fire_data(&large_data);

        // Wait for debounce timeout
        timeout(DEBOUNCE_WAIT_MS).await.unwrap();

        // URL should not be processed because total data exceeded threshold
        assert_eq!(matched_urls.borrow().len(), 0, "URLs should not be processed when data exceeds threshold");
    });
}

#[test]
fn should_find_localhost_urls() {
    let ds = ensure_no_disposables_are_leaked_in_test_suite();
    let Fixture { mock_instance, matched_urls } = create_fixture(&ds);

    run_with_real_timers(async move {
        mock_instance.fire_data("Server running at http://localhost:3000/");

        // Wait for debounce timeout
        timeout(DEBOUNCE_WAIT_MS).await.unwrap();

        assert_eq!(*matched_urls.borrow(), vec![url("localhost", 3000)]);
    });
}

#[test]
fn should_find_127_0_0_1_urls() {
    let ds = ensure_no_disposables_are_leaked_in_test_suite();
    let Fixture { mock_instance, matched_urls } = create_fixture(&ds);

    run_with_real_timers(async move {
        mock_instance.fire_data("https://127.0.0.1:5001/api");

        // Wait for debounce timeout
        timeout(DEBOUNCE_WAIT_MS).await.unwrap();

        assert_eq!(*matched_urls.borrow(), vec![url("127.0.0.1", 5001)]);
    });
}

#[test]
fn should_find_0_0_0_0_urls() {
    let ds = ensure_no_disposables_are_leaked_in_test_suite();
    let Fixture { mock_instance, matched_urls } = create_fixture(&ds);

    run_with_real_timers(async move {
        mock_instance.fire_data("http://0.0.0.0:4000");

        // Wait for debounce timeout
        timeout(DEBOUNCE_WAIT_MS).await.unwrap();

        assert_eq!(*matched_urls.borrow(), vec![url("0.0.0.0", 4000)]);
    });
}
