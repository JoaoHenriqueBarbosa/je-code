//! Porte de `src/vs/workbench/contrib/issue/test/browser/issueFormService.test.ts` (suíte `IssueFormService`), caso a caso
//! e na mesma ordem.
//!
//! Notas de tradução:
//! - `ensureNoDisposablesAreLeakedInTestSuite()` é o `LeakGuard`, criado no início de cada caso dentro de [`run_faked`] ou
//!   [`run_real`]; o `store` do original é o `DisposableStore` do guarda, que recebe o serviço. `runWithFakedTimers({
//!   useFakeTimers: true }, ...)` é o `run_with_faked_timers`; os demais casos `async` do mocha rodam no relógio real
//!   (`run_with_real_timers`).
//! - O recorte do porte do serviço não tem DOM (ver `issue_form_service.rs`): o `iframe` e o `CodeWindow` são o
//!   [`TargetWindow`] (guarda as escutas de `beforeunload`; `dispatchEvent(new Event('beforeunload'))` é
//!   `dispatch_before_unload()`), o `IAuxiliaryWindow` com `container` e `whenStylesHaveLoaded` é o [`StubAuxiliaryWindow`], e
//!   `stubInstance(IssueWebReporter, { render, dispose })` é a [`StubReporterFactory`], que entrega um [`StubReporter`].
//!   Os `stub`s do `TestInstantiationService` dos demais serviços (menu, área de transferência, diálogos, arquivos,
//!   abertura, editor, host, envio ao GitHub) não entram: o serviço portado só usa a janela auxiliar, o serviço do GitHub,
//!   o log e a fábrica do relator; os dublês [`UnusedAuxiliaryWindowService`] e [`UnusedReporterFactory`] ocupam o lugar
//!   do que o caso não usa.
//! - `mock<IWorkbenchGitHubService>` com `acquireAnonymousApiClient` é o [`StubGitHubService`]; o cliente de
//!   `upcastPartial<IGitHubAnonymousClient>({ search: { issues }, dispose })` é o [`StubClient`]. O `AbortSignal` é o de
//!   `je_base::cancellation`; o `ILogService` `NullLogService` é o `NullLogService::shared()`.
//! - `GitHubRequestError('Not Found', 'notFound', 404)` e `SchemaError` são `github_request_error` e `schema_error`.
//! - A identidade `result === items` vira igualdade de valor: o serviço devolve `page.items` como uma lista de
//!   [`SimilarIssue`], que no Rust é um valor e não uma referência. Os campos extras do item do GitHub (`id`, `node_id`,
//!   `number`, `repository_url`, `user`, `score`, `updated_at`) não existem em [`SimilarIssue`] e ficam de fora.
//! - `error => error === expected` é a igualdade do `Error`; `isCancellationError` é `is_cancellation_error`.

#[path = "../../je-base/tests/common/mod.rs"]
mod common;

use std::cell::{Cell, RefCell};
use std::future::Future;
use std::rc::Rc;

use common::{ensure_no_disposables_are_leaked_in_test_suite, run_with_faked_timers, run_with_real_timers};
use je_base::cancellation::{AbortController, AbortSignal};
use je_base::errors::{Error, is_cancellation_error};
use je_base::lifecycle::{DisposableStore, IDisposable};
use je_base::promise::Promise;
use je_base::runtime::date_now;
use je_platform::log::NullLogService;
use je_workbench::contrib::issue::base_issue_reporter_service::{GitHubIssueSearch, IssueReporterProduct, OsInfo};
use je_workbench::contrib::issue::issue_common::{IssueReporterData, IssueReporterStyles, SimilarIssue, WindowData, WindowStyles};
use je_workbench::contrib::issue::issue_form_service::{
    ApiRequestOptions, AuxiliaryWindow, AuxiliaryWindowOptions, AuxiliaryWindowService, GitHubAnonymousClient, IssueFormService,
    IssueFormServiceDeps, IssueReporterWindow, IssueWebReporter, IssueWebReporterFactory, SearchIssuesPage, WorkbenchGitHubService,
    github_request_error, schema_error,
};

/// `runWithFakedTimers({ useFakeTimers: true }, async () => ...)` com o guarda dentro do relógio virtual.
fn run_faked<F: Future<Output = ()> + 'static>(body: impl FnOnce(Rc<DisposableStore>) -> F + 'static) {
    run_with_faked_timers(async move {
        let guard = ensure_no_disposables_are_leaked_in_test_suite();
        body(guard.store()).await;
    });
}

/// O caso `async` do mocha sem relógio virtual.
fn run_real<F: Future<Output = ()> + 'static>(body: impl FnOnce(Rc<DisposableStore>) -> F + 'static) {
    run_with_real_timers(async move {
        let guard = ensure_no_disposables_are_leaked_in_test_suite();
        body(guard.store()).await;
    });
}

type SearchIssuesFn = Rc<dyn Fn(&str, AbortSignal, ApiRequestOptions) -> Promise<SearchIssuesPage>>;

/// `leases`: `{ acquired, released }`.
#[derive(Default)]
struct Leases {
    acquired: Cell<u32>,
    released: Cell<u32>,
}

impl Leases {
    fn counts(&self) -> (u32, u32) {
        (self.acquired.get(), self.released.get())
    }
}

/// `{ search: { issues: search }, dispose: () => { leases.released++; } }`.
struct StubClient {
    search: SearchIssuesFn,
    leases: Rc<Leases>,
}

impl GitHubAnonymousClient for StubClient {
    fn search_issues(&self, query: &str, signal: AbortSignal, options: ApiRequestOptions) -> Promise<SearchIssuesPage> {
        (self.search)(query, signal, options)
    }
}

impl IDisposable for StubClient {
    fn dispose(&self) {
        self.leases.released.set(self.leases.released.get() + 1);
    }
}

/// `new class extends mock<IWorkbenchGitHubService>() { override async acquireAnonymousApiClient() { ... } }`.
struct StubGitHubService {
    search: SearchIssuesFn,
    leases: Rc<Leases>,
}

impl WorkbenchGitHubService for StubGitHubService {
    fn acquire_anonymous_api_client(&self, _signal: AbortSignal) -> Promise<Rc<dyn GitHubAnonymousClient>> {
        self.leases.acquired.set(self.leases.acquired.get() + 1);
        Promise::resolved(Rc::new(StubClient { search: self.search.clone(), leases: self.leases.clone() }))
    }
}

/// O `IAuxiliaryWindowService` que o caso não usa (o original o deixa como `{}`).
struct UnusedAuxiliaryWindowService;

impl AuxiliaryWindowService for UnusedAuxiliaryWindowService {
    fn open(&self, _options: AuxiliaryWindowOptions) -> Promise<Rc<dyn AuxiliaryWindow>> {
        panic!("the auxiliary window service is not used by this case");
    }
}

/// A criação do relator legado que o caso não usa.
struct UnusedReporterFactory;

impl IssueWebReporterFactory for UnusedReporterFactory {
    fn create(&self, _disable_extensions: bool, _data: &IssueReporterData, _os: OsInfo, _product: &IssueReporterProduct, _window: Rc<dyn IssueReporterWindow>) -> Rc<dyn IssueWebReporter> {
        panic!("the legacy reporter is not used by this case");
    }
}

/// `async () => assert.fail(message)`: a busca que reprova o caso se for chamada.
fn failing_search(message: &'static str) -> SearchIssuesFn {
    Rc::new(move |_: &str, _: AbortSignal, _: ApiRequestOptions| -> Promise<SearchIssuesPage> { panic!("{message}") })
}

fn product() -> IssueReporterProduct {
    IssueReporterProduct { name_short: "Test".to_string(), version: "1.0.0".to_string(), ..IssueReporterProduct::default() }
}

/// `createSearchService(search)`.
fn create_search_service(store: &Rc<DisposableStore>, search: SearchIssuesFn) -> (Rc<IssueFormService>, Rc<Leases>) {
    let leases = Rc::new(Leases::default());
    let service = IssueFormService::new(IssueFormServiceDeps {
        auxiliary_window_service: Rc::new(UnusedAuxiliaryWindowService),
        git_hub_service: Rc::new(StubGitHubService { search, leases: leases.clone() }),
        log_service: NullLogService::shared(),
        web_reporter_factory: Rc::new(UnusedReporterFactory),
        product: product(),
    });
    store.add_rc(service.clone());
    (service, leases)
}

fn reporter_data() -> IssueReporterData {
    IssueReporterData {
        window: WindowData { styles: WindowStyles::default(), zoom_level: 0.0 },
        styles: IssueReporterStyles::default(),
        enabled_extensions: Vec::new(),
        when_extensions_loaded: None,
        when_data_complete: None,
        issue_type: None,
        issue_source: None,
        extension_id: None,
        experiments: None,
        restricted_mode: false,
        is_installation_pure: true,
        is_sessions_window: false,
        github_access_token: String::new(),
        issue_title: None,
        issue_body: None,
        data: None,
        uri: None,
        private_uri: None,
    }
}

#[test]
fn search_uses_the_anonymous_client_with_a_ten_second_deadline_and_returns_its_items() {
    run_faked(|store| async move {
        let requests: Rc<RefCell<Vec<(String, Option<ApiRequestOptions>)>>> = Rc::new(RefCell::new(Vec::new()));
        let deadline = date_now() + 10_000;
        let items = vec![SimilarIssue {
            html_url: "https://github.com/owner/repo/issues/1".to_string(),
            title: "Issue title".to_string(),
            state: Some("open".to_string()),
        }];
        let recorded = requests.clone();
        let page_items = items.clone();
        let (service, leases) = create_search_service(
            &store,
            Rc::new(move |query, _signal, options| {
                recorded.borrow_mut().push((query.to_string(), Some(options)));
                Promise::resolved(SearchIssuesPage { total_count: 1, incomplete_results: false, items: page_items.clone() })
            }),
        );
        let query = "title & labels #1";
        let results = Promise::all(vec![
            service.search_github_issues("owner/repo", query, AbortController::new().signal()),
            service.search_github_issues("owner/repo", query, AbortController::new().signal()),
        ])
        .await
        .expect("search");
        let expected_request = || (format!("repo:owner/repo {query}"), Some(ApiRequestOptions { caller: Some("github.query".to_string()), deadline: Some(deadline) }));
        assert_eq!(
            (requests.borrow().clone(), results.iter().map(|result| *result == items).collect::<Vec<_>>(), leases.counts()),
            (vec![expected_request(), expected_request()], vec![true, true], (2, 2)),
        );
    });
}

#[test]
fn malformed_and_failed_searches_remain_failures_rather_than_empty_successful_results() {
    run_real(|store| async move {
        let errors = [github_request_error("Not Found", "notFound", Some(404)), schema_error("Malformed search response")];
        let calls = Rc::new(Cell::new(0));
        let failures = errors.clone();
        let (service, leases) = create_search_service(
            &store,
            Rc::new(move |_, _, _| {
                let index = calls.get();
                calls.set(index + 1);
                Promise::rejected(failures[index].clone())
            }),
        );
        for (index, expected) in errors.iter().enumerate() {
            let outcome = service.search_github_issues("owner/repo", &index.to_string(), AbortController::new().signal()).await;
            assert_eq!(outcome.err().as_ref(), Some(expected));
        }
        assert_eq!(leases.counts(), (2, 2));
    });
}

#[test]
fn cancelled_and_disposed_reporters_do_not_issue_search_requests() {
    run_real(|store| async move {
        let (service, leases) = create_search_service(&store, failing_search("Cancelled reporters must not search"));
        let controller = AbortController::new();
        let reason = Error::new("cancelled");
        controller.abort(Some(reason.clone()));
        let outcome = service.search_github_issues("owner/repo", "query", controller.signal()).await;
        assert_eq!(outcome.err(), Some(reason));
        service.dispose();
        let outcome = service.search_github_issues("owner/repo", "query", AbortController::new().signal()).await;
        assert!(outcome.err().is_some_and(|error| is_cancellation_error(&error)));
        assert_eq!(leases.counts(), (0, 0));
    });
}

/// `{ window, container, whenStylesHaveLoaded, dispose }` já com o `CodeWindow` do `iframe`: guarda as escutas de
/// `beforeunload`.
#[derive(Default)]
struct TargetWindow {
    before_unload_listeners: RefCell<Vec<Rc<dyn Fn()>>>,
}

impl TargetWindow {
    /// `targetWindow.dispatchEvent(new Event('beforeunload'))`.
    fn dispatch_before_unload(&self) {
        let listeners = self.before_unload_listeners.borrow().clone();
        for listener in listeners {
            listener();
        }
    }
}

impl IssueReporterWindow for TargetWindow {
    fn add_before_unload_listener(&self, listener: Rc<dyn Fn()>) {
        self.before_unload_listeners.borrow_mut().push(listener);
    }

    fn focus(&self) {}
    fn close(&self) {}
}

struct StubAuxiliaryWindow {
    window: Rc<TargetWindow>,
    events: Rc<RefCell<Vec<String>>>,
}

impl AuxiliaryWindow for StubAuxiliaryWindow {
    fn window(&self) -> Rc<dyn IssueReporterWindow> {
        self.window.clone()
    }

    fn when_styles_have_loaded(&self) -> Promise<()> {
        Promise::resolved(())
    }
}

impl IDisposable for StubAuxiliaryWindow {
    fn dispose(&self) {
        self.events.borrow_mut().push("window disposed".to_string());
    }
}

/// `stub(IAuxiliaryWindowService, { open: async () => new class extends mock<IAuxiliaryWindow>() { ... }() })`.
struct StubAuxiliaryWindowService {
    window: Rc<TargetWindow>,
    events: Rc<RefCell<Vec<String>>>,
}

impl AuxiliaryWindowService for StubAuxiliaryWindowService {
    fn open(&self, _options: AuxiliaryWindowOptions) -> Promise<Rc<dyn AuxiliaryWindow>> {
        Promise::resolved(Rc::new(StubAuxiliaryWindow { window: self.window.clone(), events: self.events.clone() }))
    }
}

/// `stubInstance(IssueWebReporter, { render, dispose })`.
struct StubReporter {
    events: Rc<RefCell<Vec<String>>>,
}

impl IssueWebReporter for StubReporter {
    fn render(&self) {
        self.events.borrow_mut().push("reporter rendered".to_string());
    }
}

impl IDisposable for StubReporter {
    fn dispose(&self) {
        self.events.borrow_mut().push("reporter disposed".to_string());
    }
}

struct StubReporterFactory {
    events: Rc<RefCell<Vec<String>>>,
}

impl IssueWebReporterFactory for StubReporterFactory {
    fn create(&self, _disable_extensions: bool, _data: &IssueReporterData, _os: OsInfo, _product: &IssueReporterProduct, _window: Rc<dyn IssueReporterWindow>) -> Rc<dyn IssueWebReporter> {
        Rc::new(StubReporter { events: self.events.clone() })
    }
}

#[test]
fn disposes_the_legacy_web_reporter_when_its_auxiliary_window_closes() {
    run_real(|store| async move {
        let target_window = Rc::new(TargetWindow::default());
        let events: Rc<RefCell<Vec<String>>> = Rc::new(RefCell::new(Vec::new()));
        let service = IssueFormService::new(IssueFormServiceDeps {
            auxiliary_window_service: Rc::new(StubAuxiliaryWindowService { window: target_window.clone(), events: events.clone() }),
            git_hub_service: Rc::new(StubGitHubService {
                search: failing_search("this case does not search"),
                leases: Rc::new(Leases::default()),
            }),
            log_service: NullLogService::shared(),
            web_reporter_factory: Rc::new(StubReporterFactory { events: events.clone() }),
            product: product(),
        });
        store.add_rc(service.clone());
        service.open_aux_issue_reporter_legacy(reporter_data()).await.expect("open");
        target_window.dispatch_before_unload();
        assert_eq!(*events.borrow(), vec!["reporter rendered", "window disposed", "reporter disposed"]);
    });
}
