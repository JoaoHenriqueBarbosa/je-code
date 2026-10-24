//! Porte de `src/vs/workbench/contrib/issue/test/browser/baseIssueReporterService.test.ts` (suíte
//! `BaseIssueReporterService`), caso a caso e na mesma ordem.
//!
//! Notas de tradução:
//! - `ensureNoDisposablesAreLeakedInTestSuite()` é o `LeakGuard`, criado no início de cada caso dentro do relógio virtual de
//!   [`run`]; `runWithFakedTimers({}, async () => ...)` é o `run_with_faked_timers`, e `await timeout(ms)` é [`sleep`]. O
//!   `store` do original é o `DisposableStore` do guarda, que recebe o relator.
//! - O recorte do porte do relator não tem DOM (ver `base_issue_reporter_service.rs`): o `iframe` com `input#issue-title`,
//!   `textarea#description`, `div#similar-issues` e `select#issue-source` são os [`FormField`] e o [`SimilarIssuesView`]
//!   de [`IssueReporterElements`]; `querySelectorAll('a')` é `link_titles()`, `results.textContent` é `text_content()`,
//!   `description.dispatchEvent(new Event('input'))` é `dispatch_input()`. Os `stub`s de serviço do
//!   `TestInstantiationService` (tema, arquivos, diálogos, menu de contexto, autenticação, abertura de links) não entram: o
//!   relator portado só usa o serviço de busca, o log e o `fetch`.
//! - `Response` com `json()` é um [`FetchResponse`]; `typeof fetch` é o [`Fetcher`]; `AbortSignal` é o de `je_base::cancellation`.
//! - O `IProductConfiguration` de `mock<...>` é o [`IssueReporterProduct`] com `nameShort`, `version` e
//!   `reportMarketplaceIssueUrl`; o `ILogService` com `warn` é um [`Logger`] que guarda as mensagens de `warn`.
//! - Os laços `for (const source ...) for (const timing ...)` e `for (const backend ...) for (const action ...)` viram um
//!   `#[test]` por combinação, na ordem em que o laço as gera.

#[path = "../../je-base/tests/common/mod.rs"]
mod common;

use std::cell::{Cell, RefCell};
use std::future::Future;
use std::rc::Rc;

use common::{ensure_no_disposables_are_leaked_in_test_suite, run_with_faked_timers, timeout};
use je_base::cancellation::AbortSignal;
use je_base::errors::Error;
use je_base::event::Event;
use je_base::lifecycle::{DisposableStore, IDisposable};
use je_base::promise::{DeferredPromise, Promise};
use je_platform::log::{LogArg, LogLevel, LogMessage, Logger};
use je_workbench::contrib::issue::base_issue_reporter_service::{
    BaseIssueReporterService, DuplicateCandidates, FetchInit, FetchResponse, Fetcher, FormField, GitHubIssueSearch, IssueReporterDeps,
    IssueReporterElements, IssueReporterProduct, OsInfo, SimilarIssuesView,
};
use je_workbench::contrib::issue::issue_common::{
    IssueReporterData, IssueReporterExtensionData, IssueReporterStyles, SimilarIssue, WindowData, WindowStyles,
};

/// `runWithFakedTimers({}, async () => ...)` com o `ensureNoDisposablesAreLeakedInTestSuite()` dentro do relógio virtual; o
/// corpo recebe o `store` do guarda.
fn run<F: Future<Output = ()> + 'static>(body: impl FnOnce(Rc<DisposableStore>) -> F + 'static) {
    run_with_faked_timers(async move {
        let guard = ensure_no_disposables_are_leaked_in_test_suite();
        body(guard.store()).await;
    });
}

/// `await timeout(ms)`.
async fn sleep(ms: u64) {
    timeout(ms).await.expect("timeout");
}

fn issue(html_url: &str, title: &str) -> SimilarIssue {
    SimilarIssue { html_url: html_url.to_string(), title: title.to_string(), state: None }
}

fn current_issue() -> SimilarIssue {
    issue("https://github.com/owner/repo/issues/1", "Current issue")
}

fn stale_issue() -> SimilarIssue {
    issue("https://github.com/owner/repo/issues/2", "Stale issue")
}

/// `Response` cuja `json()` devolve `{ candidates }`.
struct DuplicateResponse(Vec<SimilarIssue>);

impl FetchResponse for DuplicateResponse {
    fn json(&self) -> Promise<Option<DuplicateCandidates>> {
        Promise::resolved(Some(DuplicateCandidates { candidates: Some(self.0.clone()) }))
    }
}

/// `createDuplicateResponse(candidates)`.
fn create_duplicate_response(candidates: Vec<SimilarIssue>) -> Rc<dyn FetchResponse> {
    Rc::new(DuplicateResponse(candidates))
}

type SearchFn = Rc<dyn Fn(&str, &str, AbortSignal) -> Promise<Vec<SimilarIssue>>>;

/// `{ searchGitHubIssues }`.
struct StubIssueFormService(SearchFn);

impl GitHubIssueSearch for StubIssueFormService {
    fn search_github_issues(&self, repo: &str, title: &str, signal: AbortSignal) -> Promise<Vec<SimilarIssue>> {
        (self.0)(repo, title, signal)
    }
}

/// `{ warn: message => { warnings.push(message); } }`.
struct WarningLog(Rc<RefCell<Vec<String>>>);

impl Logger for WarningLog {
    fn on_did_change_log_level(&self) -> Event<LogLevel> {
        Event::none()
    }

    fn get_level(&self) -> LogLevel {
        LogLevel::Info
    }

    fn set_level(&self, _level: LogLevel) {}
    fn trace_args(&self, _message: &str, _args: &[LogArg]) {}
    fn debug_args(&self, _message: &str, _args: &[LogArg]) {}
    fn info_args(&self, _message: &str, _args: &[LogArg]) {}

    fn warn_args(&self, message: &LogMessage, _args: &[LogArg]) {
        if let LogMessage::Text(text) = message {
            self.0.borrow_mut().push(text.clone());
        }
    }

    fn error_args(&self, _message: &LogMessage, _args: &[LogArg]) {}
    fn flush(&self) {}
}

impl IDisposable for WarningLog {
    fn dispose(&self) {}
}

struct Fixture {
    reporter: Rc<BaseIssueReporterService>,
    title: Rc<FormField>,
    description: Rc<FormField>,
    results: Rc<SimilarIssuesView>,
    warnings: Rc<RefCell<Vec<String>>>,
}

/// `searchGitHubIssues: async () => [currentIssue]`.
fn default_search() -> SearchFn {
    Rc::new(|_, _, _| Promise::resolved(vec![current_issue()]))
}

/// `fetcher: async () => { throw new Error('Unexpected duplicate search'); }`.
fn default_fetcher() -> Fetcher {
    Rc::new(|_, _| Promise::rejected(Error::new("Unexpected duplicate search")))
}

fn create_reporter(store: &Rc<DisposableStore>, search_github_issues: SearchFn, fetcher: Fetcher) -> Fixture {
    let title = FormField::new("");
    let description = FormField::new("");
    let results = SimilarIssuesView::new();
    let warnings = Rc::new(RefCell::new(Vec::new()));
    let data = IssueReporterData {
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
    };
    let product = IssueReporterProduct {
        name_short: "Test".to_string(),
        version: "1.0.0".to_string(),
        report_marketplace_issue_url: Some("https://github.com/owner/marketplace/issues".to_string()),
        ..IssueReporterProduct::default()
    };
    let reporter = BaseIssueReporterService::new(
        false,
        &data,
        OsInfo { r#type: "test".to_string(), arch: "test".to_string(), release: "test".to_string() },
        product,
        true,
        IssueReporterDeps {
            issue_form_service: Rc::new(StubIssueFormService(search_github_issues)),
            log_service: Rc::new(WarningLog(warnings.clone())),
            fetcher,
            elements: IssueReporterElements {
                issue_title: title.clone(),
                description: description.clone(),
                issue_source: FormField::new(""),
                similar_issues: results.clone(),
            },
        },
    );
    store.add_rc(reporter.clone());
    reporter.issue_reporter_model.update(|model| {
        model.selected_extension = Some(IssueReporterExtensionData {
            id: "publisher.extension".to_string(),
            name: "extension".to_string(),
            display_name: Some("Extension".to_string()),
            version: "1.0.0".to_string(),
            publisher: Some("publisher".to_string()),
            is_builtin: false,
            is_theme: false,
            repository_url: Some("https://github.com/owner/extension".to_string()),
            bugs_url: None,
            extension_data: None,
            extension_template: None,
            data: None,
            uri: None,
            private_uri: None,
        });
    });
    Fixture { reporter, title, description, results, warnings }
}

/// `stale duplicate results do not cancel a ${source} search during ${timing}`.
fn stale_duplicate_results_do_not_cancel_a_search(source: &'static str, timing: &'static str) {
    run(move |store| async move {
        let duplicates = DeferredPromise::<Rc<dyn FetchResponse>>::new();
        let search = DeferredPromise::<Vec<SimilarIssue>>::new();
        let calls: Rc<RefCell<Vec<(String, String, AbortSignal)>>> = Rc::new(RefCell::new(Vec::new()));
        let recorded = calls.clone();
        let search_promise = search.promise.clone();
        let duplicates_promise = duplicates.promise.clone();
        let Fixture { reporter, results, .. } = create_reporter(
            &store,
            Rc::new(move |repo, title, signal| {
                recorded.borrow_mut().push((repo.to_string(), title.to_string(), signal));
                search_promise.clone()
            }),
            Rc::new(move |_, _| duplicates_promise.clone()),
        );
        reporter.search_vscode_issues("Previous title", Some("Previous description"));
        sleep(301).await;
        reporter.search_issues("Current title", Some(source == "extension"), Some(source == "marketplace"));
        if timing == "active" {
            sleep(301).await;
        }
        duplicates.complete(create_duplicate_response(vec![stale_issue()])).await.expect("complete");
        sleep(0).await;
        if timing == "debounce" {
            assert_eq!(calls.borrow().len(), 0);
        }
        sleep(301).await;
        search.complete(vec![current_issue()]).await.expect("complete");
        sleep(0).await;
        let seen: Vec<(String, String, bool)> = calls.borrow().iter().map(|(repo, title, signal)| (repo.clone(), title.clone(), signal.aborted())).collect();
        assert_eq!(
            (seen, results.link_titles()),
            (vec![(format!("owner/{source}"), "Current title".to_string(), false)], vec![current_issue().title]),
        );
    });
}

#[test]
fn stale_duplicate_results_do_not_cancel_a_marketplace_search_during_debounce() {
    stale_duplicate_results_do_not_cancel_a_search("marketplace", "debounce");
}

#[test]
fn stale_duplicate_results_do_not_cancel_a_marketplace_search_during_active() {
    stale_duplicate_results_do_not_cancel_a_search("marketplace", "active");
}

#[test]
fn stale_duplicate_results_do_not_cancel_a_extension_search_during_debounce() {
    stale_duplicate_results_do_not_cancel_a_search("extension", "debounce");
}

#[test]
fn stale_duplicate_results_do_not_cancel_a_extension_search_during_active() {
    stale_duplicate_results_do_not_cancel_a_search("extension", "active");
}

#[test]
fn renders_current_product_duplicate_results_without_cancelling_their_request() {
    run(|store| async move {
        let request: Rc<RefCell<Option<FetchInit>>> = Rc::new(RefCell::new(None));
        let recorded = request.clone();
        let Fixture { reporter, results, .. } = create_reporter(
            &store,
            default_search(),
            Rc::new(move |_, init| {
                *recorded.borrow_mut() = Some(init);
                Promise::resolved(create_duplicate_response(vec![current_issue()]))
            }),
        );
        reporter.search_vscode_issues("Current title", Some("Current description"));
        sleep(301).await;
        let request = request.borrow();
        let request = request.as_ref();
        assert_eq!(
            (
                request.map(|init| init.method.clone()),
                request.map(|init| init.body.clone()),
                request.map(|init| init.signal.aborted()),
                results.link_titles(),
            ),
            (
                Some("POST".to_string()),
                Some(r#"{"title":"Current title","body":"Current description"}"#.to_string()),
                Some(false),
                vec![current_issue().title],
            ),
        );
    });
}

#[test]
fn editing_a_marketplace_description_does_not_start_a_product_duplicate_search() {
    run(|store| async move {
        let duplicate_calls = Rc::new(Cell::new(0));
        let counted = duplicate_calls.clone();
        let Fixture { reporter, title, description, results, .. } = create_reporter(
            &store,
            default_search(),
            Rc::new(move |_, _| {
                counted.set(counted.get() + 1);
                Promise::resolved(create_duplicate_response(vec![stale_issue()]))
            }),
        );
        reporter.issue_reporter_model.update(|model| {
            model.file_on_extension = Some(false);
            model.file_on_marketplace = Some(true);
        });
        reporter.set_event_handlers();
        title.set_value("Current title");
        reporter.search_issues(&title.value(), Some(false), Some(true));
        description.set_value("Updated description");
        description.dispatch_input();
        sleep(301).await;
        assert_eq!((duplicate_calls.get(), results.link_titles()), (0, vec![current_issue().title]));
    });
}

#[test]
fn failed_github_searches_do_not_keep_results_for_the_previous_title() {
    run(|store| async move {
        let failure = DeferredPromise::<Vec<SimilarIssue>>::new();
        let failure_promise = failure.promise.clone();
        let Fixture { reporter, results, warnings, .. } = create_reporter(
            &store,
            Rc::new(move |_, title, _| {
                if title == "First title" { Promise::resolved(vec![stale_issue()]) } else { failure_promise.clone() }
            }),
            default_fetcher(),
        );
        reporter.search_issues("First title", Some(false), Some(true));
        sleep(301).await;
        assert_eq!(results.link_titles().first(), Some(&stale_issue().title));
        reporter.search_issues("Second title", Some(false), Some(true));
        sleep(301).await;
        failure.error(Error::new("Search quota exhausted")).await.expect("error");
        sleep(0).await;
        assert_eq!(
            (results.link_titles(), reporter.number_of_search_results_displayed.get(), warnings.borrow().clone()),
            (Vec::<String>::new(), 0, vec!["[IssueReporter] Error fetching GitHub issues".to_string()]),
        );
    });
}

/// Ou limpa os resultados (`clear`) ou descarta o relator (`dispose`).
fn finish_search(reporter: &BaseIssueReporterService, action: &str) {
    if action == "clear" {
        reporter.clear_search_results();
    } else {
        reporter.dispose();
    }
}

/// `${action} cancels a debounced ${backend} search`.
fn action_cancels_a_debounced_search(action: &'static str, backend: &'static str) {
    run(move |store| async move {
        let calls = Rc::new(Cell::new(0));
        let github_calls = calls.clone();
        let duplicate_calls = calls.clone();
        let Fixture { reporter, .. } = create_reporter(
            &store,
            Rc::new(move |_, _, _| {
                github_calls.set(github_calls.get() + 1);
                Promise::resolved(Vec::new())
            }),
            Rc::new(move |_, _| {
                duplicate_calls.set(duplicate_calls.get() + 1);
                Promise::resolved(create_duplicate_response(Vec::new()))
            }),
        );
        reporter.search_issues("Title", Some(false), Some(backend == "github"));
        finish_search(&reporter, action);
        sleep(301).await;
        assert_eq!(calls.get(), 0);
    });
}

/// `${action} aborts an active ${backend} search and suppresses late results`.
fn action_aborts_an_active_search(action: &'static str, backend: &'static str) {
    run(move |store| async move {
        let pending = DeferredPromise::<()>::new();
        let signal: Rc<RefCell<Option<AbortSignal>>> = Rc::new(RefCell::new(None));
        let github_signal = signal.clone();
        let fetch_signal = signal.clone();
        let github_pending = pending.promise.clone();
        let fetch_pending = pending.promise.clone();
        let Fixture { reporter, results, .. } = create_reporter(
            &store,
            Rc::new(move |_, _, request_signal| {
                *github_signal.borrow_mut() = Some(request_signal);
                github_pending.then(|()| vec![stale_issue()])
            }),
            Rc::new(move |_, init| {
                *fetch_signal.borrow_mut() = Some(init.signal);
                fetch_pending.then(|()| create_duplicate_response(vec![stale_issue()]))
            }),
        );
        reporter.search_issues("Title", Some(false), Some(backend == "github"));
        sleep(301).await;
        finish_search(&reporter, action);
        pending.complete(()).await.expect("complete");
        sleep(0).await;
        let aborted = signal.borrow().as_ref().map(AbortSignal::aborted);
        assert_eq!((aborted, results.text_content()), (Some(true), String::new()));
    });
}

#[test]
fn clear_cancels_a_debounced_github_search() {
    action_cancels_a_debounced_search("clear", "github");
}

#[test]
fn clear_aborts_an_active_github_search_and_suppresses_late_results() {
    action_aborts_an_active_search("clear", "github");
}

#[test]
fn dispose_cancels_a_debounced_github_search() {
    action_cancels_a_debounced_search("dispose", "github");
}

#[test]
fn dispose_aborts_an_active_github_search_and_suppresses_late_results() {
    action_aborts_an_active_search("dispose", "github");
}

#[test]
fn clear_cancels_a_debounced_duplicates_search() {
    action_cancels_a_debounced_search("clear", "duplicates");
}

#[test]
fn clear_aborts_an_active_duplicates_search_and_suppresses_late_results() {
    action_aborts_an_active_search("clear", "duplicates");
}

#[test]
fn dispose_cancels_a_debounced_duplicates_search() {
    action_cancels_a_debounced_search("dispose", "duplicates");
}

#[test]
fn dispose_aborts_an_active_duplicates_search_and_suppresses_late_results() {
    action_aborts_an_active_search("dispose", "duplicates");
}
