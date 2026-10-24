//! Porte de `src/vs/workbench/api/test/browser/mainThreadTimeline.test.ts` (suíte `MainThreadTimeline`), caso a caso e
//! na mesma ordem.
//!
//! Notas de tradução:
//! - `ensureNoDisposablesAreLeakedInTestSuite()` vira o guarda criado no começo de cada caso; os dois primeiros casos
//!   aguardam `getTimeline(...)?.result` e rodam em `run_with_faked_timers`.
//! - `mock<ExtHostTimelineShape>()` com `$getTimeline` e `SingleProxyRPCProtocol` são o [`MockExtHostTimeline`] e o
//!   `single_proxy_rpc_protocol`. `TestViewsService` (de `workbenchTestServices.ts`, só `openView` aqui) é o
//!   [`TestViewsService`], e `TestConfigurationService`, `MockContextKeyService` e `NullLogService` são os de `je-platform`.
//! - `Reflect.get(customer, '_providerEmitters')` é `provider_emitters_for_testing`. O `store.add({ dispose: ... })` que
//!   descarta os emissores não corrigidos é um `plain_disposable`.
//! - `emitters.size` é `len()`, `[...emitters.keys()]` é `keys()`, e `===`/`notStrictEqual` entre emissores é `Rc::ptr_eq`.
//! - `customer.dispose()` do nono e do décimo caso é a chamada direta, e o `store` descarta de novo ao fim (idempotente).
//! - O `{ id, label, scheme, provideTimeline, dispose() {} }` do décimo caso é o [`OtherProvider`] (um `TimelineProvider`).

#[path = "../../je-base/tests/common/mod.rs"]
mod common;
mod support;

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use common::{ensure_no_disposables_are_leaked_in_test_suite, run_with_faked_timers};
use je_base::cancellation::{CancellationToken, CancellationTokenSource};
use je_base::event::{Emitter, Event};
use je_base::lifecycle::{plain_disposable, DisposableMap, DisposableStore, IDisposable};
use je_base::promise::Promise;
use je_base::uri::URI;
use je_platform::configuration::test_configuration_service::TestConfigurationService;
use je_platform::keybinding::mock_keybinding_service::MockContextKeyService;
use je_platform::log::NullLogService;
use je_platform::resources::uri_file;
use je_workbench::api::browser::main_thread_timeline::MainThreadTimeline;
use je_workbench::api::common::ext_host_protocol::{ExtHostTimelineShape, MainThreadTimelineShape};
use je_workbench::contrib::timeline::common::timeline::{
    ITimelineService, Timeline, TimelineItem, TimelineOptions, TimelineProvider, TimelineProviderDescriptor, TimelineScheme, TimelineSource,
    TimelineTriggerKind, WorkspaceTimelineProviderDescriptor,
};
use je_workbench::contrib::timeline::common::timeline_service::TimelineService;
use je_workbench::services::views::views_service::IViewsService;
use support::test_rpc_protocol::single_proxy_rpc_protocol;

// ---- os dublês --------------------------------------------------------------------------------------

/// `new class extends mock<ExtHostTimelineShape>() { override async $getTimeline(id, uri) { ... } }`.
struct MockExtHostTimeline;

impl ExtHostTimelineShape for MockExtHostTimeline {
    fn rpc_get_timeline(&self, id: &str, uri: Option<URI>, _options: TimelineOptions, _token: CancellationToken) -> Promise<Option<Timeline>> {
        Promise::resolved(Some(Timeline {
            source: id.to_owned(),
            items: vec![TimelineItem {
                handle: "item".to_owned(),
                source: id.to_owned(),
                label: if uri.is_some() { "Timeline item" } else { "Workspace item" }.to_owned(),
                timestamp: 1.0,
                ..TimelineItem::default()
            }],
            cursor: None,
        }))
    }
}

/// `TestViewsService`.
struct TestViewsService;

impl IViewsService for TestViewsService {
    fn open_view(&self, _id: &str, _focus: bool) -> Promise<()> {
        Promise::resolved(())
    }
}

/// `{ id: 'other', label: 'Other', scheme: 'file', provideTimeline: async () => ({ source: 'other', items: [] }), dispose() { } }`.
struct OtherProvider;

impl IDisposable for OtherProvider {
    fn dispose(&self) {}
}

impl TimelineProvider for OtherProvider {
    fn id(&self) -> String {
        "other".to_owned()
    }

    fn label(&self) -> String {
        "Other".to_owned()
    }

    fn scheme(&self) -> TimelineScheme {
        TimelineScheme::One("file".to_owned())
    }

    fn on_did_change(&self) -> Option<Event<Option<Vec<URI>>>> {
        None
    }

    fn provide_timeline(&self, _uri: &URI, _options: &TimelineOptions, _token: CancellationToken) -> Promise<Option<Timeline>> {
        Promise::resolved(Some(Timeline { source: "other".to_owned(), items: vec![], cursor: None }))
    }
}

// ---- o ambiente -------------------------------------------------------------------------------------

type Emitters = Rc<DisposableMap<String, Emitter<Option<Vec<URI>>>>>;

/// O que `createCustomer()` devolve.
struct Customer {
    customer: Rc<MainThreadTimeline>,
    service: Rc<TimelineService>,
    emitters: Emitters,
}

/// `createCustomer()`.
fn create_customer(store: &Rc<DisposableStore>) -> Customer {
    let service = store.add_rc(TimelineService::new(
        NullLogService::shared(),
        Rc::new(TestViewsService),
        TestConfigurationService::new(None),
        MockContextKeyService::new(),
    ));
    let proxy: Rc<dyn ExtHostTimelineShape> = Rc::new(MockExtHostTimeline);
    let context = single_proxy_rpc_protocol(proxy);
    let customer = store.add_rc(MainThreadTimeline::new(&*context, NullLogService::shared(), service.clone()));
    let emitters = customer.provider_emitters_for_testing();
    // Cleanup the unfixed allocations after red assertions, without hiding their observed lifetime.
    let leftovers = emitters.clone();
    store.add(plain_disposable(move || {
        for emitter in leftovers.values() {
            emitter.dispose();
        }
    }));
    Customer { customer, service, emitters }
}

/// `register(customer, id)`.
fn register(customer: &MainThreadTimeline, id: &str) {
    customer.rpc_register_timeline_provider(TimelineProviderDescriptor { id: id.to_owned(), label: id.to_owned(), scheme: TimelineScheme::One("file".to_owned()) });
}

fn source_ids(service: &TimelineService) -> Vec<String> {
    service.get_sources().into_iter().map(|source| source.id).collect()
}

fn automatic() -> TimelineOptions {
    TimelineOptions::new(TimelineTriggerKind::Automatic)
}

fn key(id: &str) -> String {
    id.to_owned()
}

// ---- os casos ---------------------------------------------------------------------------------------

#[test]
fn live_provider_forwards_timeline_requests_and_change_events() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let store = guard.store();
    run_with_faked_timers(async move {
        let Customer { customer, service, emitters } = create_customer(&store);
        register(&customer, "live");
        let changes: Rc<RefCell<Vec<(String, bool, Option<Vec<String>>)>>> = Rc::default();
        let recorded = changes.clone();
        store.add(service.on_did_change_timeline().listen(move |event| {
            recorded.borrow_mut().push((event.id.clone(), event.workspace, event.uris.as_ref().map(|uris| uris.iter().map(|uri| uri.to_string(false).to_string_lossy()).collect())));
        }));
        customer.rpc_emit_timeline_change_event("live", None);
        customer.rpc_emit_timeline_change_event("live", Some(vec![uri_file("/timeline.txt")]));
        let token = CancellationTokenSource::new(None);
        store.add_rc(Rc::new(token.clone()));
        let request = service.get_timeline("live", Some(&uri_file("/timeline.txt")), &automatic(), token);
        let result = match request {
            Some(request) => request.result.await.unwrap(),
            None => None,
        };
        let actual = (
            changes.borrow().clone(),
            result.map(|result| result.items.into_iter().map(|item| item.label).collect::<Vec<_>>()),
            emitters.len(),
        );
        assert_eq!(
            actual,
            (
                vec![("live".to_owned(), false, None), ("live".to_owned(), false, Some(vec!["file:///timeline.txt".to_owned()]))],
                Some(vec!["Timeline item".to_owned()]),
                1,
            )
        );
        customer.rpc_unregister_timeline_provider("live");
    });
}

#[test]
fn workspace_provider_forwards_workspace_timeline_requests_and_change_events() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let store = guard.store();
    run_with_faked_timers(async move {
        let Customer { customer, service, .. } = create_customer(&store);
        customer.rpc_register_timeline_provider(TimelineProviderDescriptor { id: "shared".to_owned(), label: "Shared".to_owned(), scheme: TimelineScheme::One("file".to_owned()) });
        customer.rpc_register_workspace_timeline_provider(WorkspaceTimelineProviderDescriptor { id: "shared".to_owned(), label: "Shared".to_owned() });
        let changes: Rc<RefCell<Vec<(String, bool)>>> = Rc::default();
        let recorded = changes.clone();
        store.add(service.on_did_change_timeline().listen(move |event| recorded.borrow_mut().push((event.id.clone(), event.workspace))));
        customer.rpc_emit_workspace_timeline_change_event("shared");
        let token = CancellationTokenSource::new(None);
        store.add_rc(Rc::new(token.clone()));
        let request = service.get_timeline("shared", None, &automatic(), token);
        let result = match request {
            Some(request) => request.result.await.unwrap(),
            None => None,
        };
        let sources = source_ids(&service);
        customer.rpc_unregister_workspace_timeline_provider("shared");
        let actual = (
            changes.borrow().clone(),
            result.map(|result| result.items.into_iter().map(|item| item.label).collect::<Vec<_>>()),
            sources,
            service.has_workspace_providers(),
            source_ids(&service),
        );
        assert_eq!(
            actual,
            (
                vec![("shared".to_owned(), true)],
                Some(vec!["Workspace item".to_owned()]),
                vec!["shared".to_owned()],
                false,
                vec!["shared".to_owned()],
            )
        );
        customer.rpc_unregister_timeline_provider("shared");
    });
}

#[test]
fn unregister_releases_its_change_emitter() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let store = guard.store();
    let Customer { customer, service, emitters } = create_customer(&store);
    register(&customer, "retired");
    customer.rpc_unregister_timeline_provider("retired");
    assert_eq!((service.get_sources(), emitters.len()), (Vec::<TimelineSource>::new(), 0));
}

#[test]
fn unregister_disposes_observers_of_its_emitter() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let store = guard.store();
    let Customer { customer, emitters, .. } = create_customer(&store);
    register(&customer, "retired");
    let emitter = emitters.get(&key("retired")).unwrap();
    let calls = Rc::new(Cell::new(0));
    let counter = calls.clone();
    store.add(emitter.event().listen(move |_| counter.set(counter.get() + 1)));
    customer.rpc_unregister_timeline_provider("retired");
    emitter.fire(None);
    assert_eq!(calls.get(), 0);
}

#[test]
fn unregister_leaves_another_live_provider_intact() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let store = guard.store();
    let Customer { customer, service, emitters } = create_customer(&store);
    register(&customer, "retired");
    register(&customer, "live");
    let live_emitter = emitters.get(&key("live"));
    customer.rpc_unregister_timeline_provider("retired");
    let same_live = match (emitters.get(&key("live")), &live_emitter) {
        (Some(current), Some(previous)) => Rc::ptr_eq(&current, previous),
        (None, None) => true,
        _ => false,
    };
    assert_eq!((source_ids(&service), emitters.keys(), same_live), (vec!["live".to_owned()], vec!["live".to_owned()], true));
    customer.rpc_unregister_timeline_provider("live");
}

#[test]
fn repeated_and_unknown_unregister_preserve_live_providers() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let store = guard.store();
    let Customer { customer, service, emitters } = create_customer(&store);
    register(&customer, "live");
    customer.rpc_unregister_timeline_provider("missing");
    customer.rpc_unregister_timeline_provider("missing");
    assert_eq!((source_ids(&service), emitters.keys()), (vec!["live".to_owned()], vec!["live".to_owned()]));
    customer.rpc_unregister_timeline_provider("live");
}

#[test]
fn successive_disposed_providers_do_not_accumulate_emitters() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let store = guard.store();
    let Customer { customer, service, emitters } = create_customer(&store);
    for index in 0..37 {
        let id = format!("retired-{index}");
        register(&customer, &id);
        customer.rpc_unregister_timeline_provider(&id);
    }
    assert_eq!((service.get_sources(), emitters.len()), (Vec::<TimelineSource>::new(), 0));
}

#[test]
fn re_registering_a_retired_id_uses_a_fresh_emitter() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let store = guard.store();
    let Customer { customer, emitters, .. } = create_customer(&store);
    register(&customer, "reused");
    let retired = emitters.get(&key("reused")).unwrap();
    customer.rpc_unregister_timeline_provider("reused");
    register(&customer, "reused");
    assert!(!Rc::ptr_eq(&emitters.get(&key("reused")).unwrap(), &retired));
    customer.rpc_unregister_timeline_provider("reused");
}

#[test]
fn customer_disposal_unregisters_its_providers_and_releases_emitters() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let store = guard.store();
    let Customer { customer, service, emitters } = create_customer(&store);
    register(&customer, "first");
    register(&customer, "second");
    customer.dispose();
    assert_eq!((service.get_sources(), emitters.len()), (Vec::<TimelineSource>::new(), 0));
    service.unregister_timeline_provider("first");
    service.unregister_timeline_provider("second");
}

#[test]
fn customer_disposal_preserves_providers_owned_by_another_customer() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let store = guard.store();
    let Customer { customer, service, emitters } = create_customer(&store);
    let other = store.add(service.register_timeline_provider(Rc::new(OtherProvider)));
    register(&customer, "retired");
    customer.dispose();
    assert_eq!((source_ids(&service), emitters.len()), (vec!["other".to_owned()], 0));
    other.dispose();
}
