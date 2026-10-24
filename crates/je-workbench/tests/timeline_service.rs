//! Porte de `src/vs/workbench/contrib/timeline/test/common/timelineService.test.ts` (suíte `TimelineService`), caso a caso e na mesma ordem.
//!
//! Divergências:
//! - **`ensureNoDisposablesAreLeakedInTestSuite`.** É o [`LeakGuard`] criado no começo de cada caso, e o `store.add` do original é
//!   `guard.add` ou `guard.add_rc`.
//! - **`upcastPartial<IViewsService>({ openView })`.** É um [`IViewsService`] cuja `open_view` registra o evento
//!   `openView <id>`, espera a promessa do teste e registra `opened`; o do segundo caso (`{}`) falha se usado.
//! - **`await`.** Cada `await` esvazia as microtarefas (`runtime::run_microtasks`) e o `await result` confere que a promessa de
//!   `setUri` resolveu.
//! - **Provedores.** `{ id, label, scheme, provideTimeline: async () => undefined, dispose() { } }` é o [`TestProvider`] (a
//!   timeline do arquivo) e o provedor do workspace é o [`TestWorkspaceProvider`]; os dois devolvem `None` e não têm evento.
//! - **`deepStrictEqual`.** Os eventos de mudança de provedores (`TimelineProvidersChangeEvent`) comparam por valor, com
//!   `added` e `removed` ausentes como `None`.

#[path = "../../je-base/tests/common/mod.rs"]
mod common;

use std::cell::RefCell;
use std::rc::Rc;

use common::{LeakGuard, ensure_no_disposables_are_leaked_in_test_suite};
use je_base::cancellation::CancellationToken;
use je_base::event::Event;
use je_base::lifecycle::IDisposable;
use je_base::promise::{DeferredPromise, Promise};
use je_base::runtime;
use je_base::uri::URI;
use je_platform::configuration::service::IConfigurationService;
use je_platform::configuration::test_configuration_service::TestConfigurationService;
use je_platform::keybinding::mock_keybinding_service::MockContextKeyService;
use je_platform::log::NullLogService;
use je_platform::resources::uri_file;
use je_workbench::contrib::timeline::common::timeline::{
    ITimelineService, Timeline, TimelineOptions, TimelineProvider, TimelineProvidersChangeEvent, TimelineScheme, WorkspaceTimelineProvider,
    TIMELINE_PANE_ID,
};
use je_workbench::contrib::timeline::common::timeline_service::TimelineService;
use je_workbench::services::views::views_service::IViewsService;

/// O `IViewsService` do primeiro caso.
struct TestViewsService {
    events: Rc<RefCell<Vec<String>>>,
    open_view: Rc<DeferredPromise<()>>,
}

impl IViewsService for TestViewsService {
    fn open_view(&self, id: &str, _focus: bool) -> Promise<()> {
        self.events.borrow_mut().push(format!("openView {id}"));
        let events = self.events.clone();
        self.open_view.promise.then(move |_| events.borrow_mut().push("opened".to_string()))
    }
}

/// O `upcastPartial<IViewsService>({})` do segundo caso.
struct UnusedViewsService;

impl IViewsService for UnusedViewsService {
    fn open_view(&self, _id: &str, _focus: bool) -> Promise<()> {
        panic!("IViewsService.openView não é usado por este caso (o original passa um objeto vazio)")
    }
}

struct TestProvider;

impl IDisposable for TestProvider {
    fn dispose(&self) {}
}

impl TimelineProvider for TestProvider {
    fn id(&self) -> String {
        "shared".to_string()
    }

    fn label(&self) -> String {
        "Shared".to_string()
    }

    fn scheme(&self) -> TimelineScheme {
        TimelineScheme::One("file".to_string())
    }

    fn on_did_change(&self) -> Option<Event<Option<Vec<URI>>>> {
        None
    }

    fn provide_timeline(&self, _uri: &URI, _options: &TimelineOptions, _token: CancellationToken) -> Promise<Option<Timeline>> {
        Promise::resolved(None)
    }
}

struct TestWorkspaceProvider;

impl IDisposable for TestWorkspaceProvider {
    fn dispose(&self) {}
}

impl WorkspaceTimelineProvider for TestWorkspaceProvider {
    fn id(&self) -> String {
        "shared".to_string()
    }

    fn label(&self) -> String {
        "Shared".to_string()
    }

    fn on_did_change(&self) -> Option<Event<()>> {
        None
    }

    fn provide_workspace_timeline(&self, _options: &TimelineOptions, _token: CancellationToken) -> Promise<Option<Timeline>> {
        Promise::resolved(None)
    }
}

fn new_service(guard: &LeakGuard, views_service: Rc<dyn IViewsService>) -> Rc<TimelineService> {
    let configuration_service: Rc<dyn IConfigurationService> = TestConfigurationService::new(None);
    guard.add_rc(TimelineService::new(NullLogService::shared(), views_service, configuration_service, MockContextKeyService::new()))
}

fn change(removed: Option<&[&str]>, added: Option<&[&str]>) -> TimelineProvidersChangeEvent {
    let to_vec = |ids: &[&str]| ids.iter().map(|id| id.to_string()).collect::<Vec<_>>();
    TimelineProvidersChangeEvent { added: added.map(to_vec), removed: removed.map(to_vec) }
}

#[test]
fn set_uri_notifies_the_view_only_after_it_has_been_opened() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let events: Rc<RefCell<Vec<String>>> = Rc::new(RefCell::new(Vec::new()));
    let open_view = Rc::new(DeferredPromise::<()>::new());
    let views_service: Rc<dyn IViewsService> = Rc::new(TestViewsService { events: events.clone(), open_view: open_view.clone() });
    let service = new_service(&guard, views_service);
    let sink = events.clone();
    guard.add(service.on_did_change_uri().listen(move |uri| {
        sink.borrow_mut().push(format!("onDidChangeUri {}", uri.as_ref().map_or_else(|| "undefined".to_string(), |uri| uri.path().to_string_lossy())));
    }));

    let result = service.set_uri(Some(uri_file("/timeline.txt")));
    let _ = open_view.complete(());
    runtime::run_microtasks();
    assert!(matches!(result.result(), Some(Ok(()))), "setUri resolveu");

    assert_eq!(*events.borrow(), vec![format!("openView {TIMELINE_PANE_ID}"), "opened".to_string(), "onDidChangeUri /timeline.txt".to_string()]);
}

#[test]
fn removing_one_kind_of_provider_with_a_shared_id_reloads_the_other_kind() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let service = new_service(&guard, Rc::new(UnusedViewsService));
    let events: Rc<RefCell<Vec<TimelineProvidersChangeEvent>>> = Rc::new(RefCell::new(Vec::new()));
    let sink = events.clone();
    guard.add(service.on_did_change_providers().listen(move |event| sink.borrow_mut().push(event.clone())));
    let register_file = || service.register_timeline_provider(Rc::new(TestProvider));
    let register_workspace = || service.register_workspace_timeline_provider(Rc::new(TestWorkspaceProvider));

    let file = register_file();
    let workspace = register_workspace();
    events.borrow_mut().clear();
    workspace.dispose();
    file.dispose();
    let workspace_removed: Vec<TimelineProvidersChangeEvent> = events.borrow_mut().drain(..).collect();

    let file2 = register_file();
    let workspace2 = register_workspace();
    events.borrow_mut().clear();
    file2.dispose();
    workspace2.dispose();

    assert_eq!(workspace_removed, vec![change(Some(&["shared"]), Some(&["shared"])), change(Some(&["shared"]), None)]);
    assert_eq!(*events.borrow(), vec![change(Some(&["shared"]), Some(&["shared"])), change(Some(&["shared"]), None)]);
}
