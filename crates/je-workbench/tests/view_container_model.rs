//! Porte de `src/vs/workbench/services/views/test/browser/viewContainerModel.test.ts` (suíte `ViewContainerModel`), caso a caso e na mesma ordem.
//!
//! Notas de tradução:
//! - **`ensureNoDisposablesAreLeakedInTestSuite()`, `setup` e `teardown`.** O [`TestEnv`] reúne o que o `mocha` roda em torno de
//!   cada caso, e a ordem dos campos é a ordem em que eles são descartados: o [`Fixture`] (o `setup`, que também esvazia as
//!   microtarefas antes de descartar, como o `mocha` faz entre o corpo e os `teardown`), o guarda de vazamentos (o `teardown` de
//!   `ensureNoDisposablesAreLeakedInTestSuite`, que no `mocha` roda primeiro porque foi registrado primeiro) e a [`Suite`] (o
//!   `teardown` do próprio teste, que tira as views e o contêiner dos registros).
//! - **Os registros são globais por thread.** `ViewContainerRegistry` e `ViewsRegistry` são constantes de módulo no original
//!   (criadas antes de qualquer `setup`); a [`Suite`] os obtém antes do guarda de vazamentos, então não são rastreados. Cada
//!   `#[test]` roda na sua thread, com registros novos.
//! - **`workbenchInstantiationService(undefined, disposableStore)`.** Só os serviços de que o `ViewDescriptorService` e o
//!   `ViewContainerModel` dependem: `IStorageService` (`TestStorageService`, o `InMemoryStorageService`),
//!   `IExtensionService` (`TestExtensionService`), `ITelemetryService` (o nulo), `ILoggerService` (o nulo),
//!   `IWorkbenchEnvironmentService` (`TestEnvironmentService`) e o `ContextKeyService` real, no lugar do `IContextKeyService`
//!   (`instantiationService.stub(IContextKeyService, contextKeyService)`).
//! - **`new SyncDescriptor(<any>{})` e `ctorDescriptor: null!`** são `ctor_descriptor: None` (nenhum caso instancia o contêiner
//!   nem a view).
//! - **Identidade.** `deepStrictEqual` entre descritores de view (objetos que o registro devolve como são) é a identidade
//!   (`Rc::ptr_eq`), que é mais forte; os `{ viewDescriptor, index }` dos eventos são conferidos campo a campo.
//! - **`sinon.spy()`** é o [`spy`], que guarda os argumentos de cada chamada.
//! - **`storageService.store(\`${container.id}.state.hidden\`, ...)` antes de registrar o contêiner** lê o `container` do caso
//!   anterior (sempre o de id `test`); como cada caso roda isolado, o id é o literal `test`.
//! - **Casos `async`** rodam em `run_with_faked_timers`, e `await new Promise(c => setTimeout(c, 30))` é `timeout(30)`.

#[path = "../../je-base/tests/common/mod.rs"]
mod common;

use std::cell::RefCell;
use std::rc::Rc;

use common::{LeakGuard, ensure_no_disposables_are_leaked_in_test_suite, run_with_faked_timers, timeout};
use je_base::arrays::r#move;
use je_base::dynamic::Dynamic;
use je_base::event::Event;
use je_base::lifecycle::{DisposableRef, IDisposable};
use je_base::nls::localize2;
use je_base::runtime;
use je_instantiation::{InstantiationService, ServiceCollection};
use je_platform::configuration::test_configuration_service::TestConfigurationService;
use je_platform::contextkey::service::ContextKeyService;
use je_platform::contextkey::{ContextKeyExpr, I_CONTEXT_KEY_SERVICE, IContextKeyService};
use je_platform::log::{I_LOGGER_SERVICE, LoggerService, null_logger_service};
use je_platform::storage::{I_STORAGE_SERVICE, InMemoryStorageService, StorageScope, StorageService, StorageTarget};
use je_platform::telemetry::telemetry::I_TELEMETRY_SERVICE;
use je_platform::telemetry::telemetry_utils::null_telemetry_service;
use je_storage::StorageValue;
use je_workbench::common::views::{
    IAddedViewDescriptorRef, IViewContainerDescriptor, IViewContainerModel, IViewDescriptor, IViewDescriptorRef, IViewDescriptorService, ViewContainerLocation,
    ViewContainerRc, ViewContainersRegistry, ViewDescriptorRc, ViewsRegistry, view_containers_registry, views_registry,
};
use je_workbench::services::environment::environment_service::{IWorkbenchEnvironmentService, I_WORKBENCH_ENVIRONMENT_SERVICE};
use je_workbench::services::extensions::extensions::{IExtensionService, I_EXTENSION_SERVICE};
use je_workbench::services::views::view_container_model::get_views_state_storage_id;
use je_workbench::services::views::view_descriptor_service::ViewDescriptorService;
use je_workbench::test::browser::workbench_test_services::{TestEnvironmentService, TestExtensionService};
use serde_json::json;

/// `class ViewDescriptorSequence`.
struct ViewDescriptorSequence {
    elements: Rc<RefCell<Vec<ViewDescriptorRc>>>,
    disposables: RefCell<Vec<DisposableRef>>,
}

impl ViewDescriptorSequence {
    fn new(model: &Rc<dyn IViewContainerModel>) -> Rc<Self> {
        let elements = Rc::new(RefCell::new(model.visible_view_descriptors()));

        let target = elements.clone();
        let on_add = model.on_did_add_visible_view_descriptors().listen(move |added| {
            for IAddedViewDescriptorRef { view_descriptor, index, .. } in added {
                let mut elements = target.borrow_mut();
                // `elements.splice(index, 0, viewDescriptor)`: um índice além do fim acrescenta.
                let at = (*index).min(elements.len());
                elements.insert(at, view_descriptor.clone());
            }
        });
        let target = elements.clone();
        let on_remove = model.on_did_remove_visible_view_descriptors().listen(move |removed| {
            let mut removed = removed.clone();
            removed.sort_by(|a, b| b.index.cmp(&a.index));
            for IViewDescriptorRef { index, .. } in removed {
                let mut elements = target.borrow_mut();
                // `elements.splice(index, 1)`: um índice além do fim não remove nada.
                if index < elements.len() {
                    elements.remove(index);
                }
            }
        });
        let target = elements.clone();
        let on_move = model.on_did_move_visible_view_descriptors().listen(move |change| {
            r#move(&mut target.borrow_mut(), change.from.index, change.to.index);
        });

        Rc::new(Self { elements, disposables: RefCell::new(vec![on_add, on_remove, on_move]) })
    }

    fn elements(&self) -> Vec<ViewDescriptorRc> {
        self.elements.borrow().clone()
    }
}

impl IDisposable for ViewDescriptorSequence {
    fn dispose(&self) {
        let disposables: Vec<DisposableRef> = self.disposables.borrow_mut().drain(..).collect();
        for disposable in disposables {
            disposable.dispose();
        }
    }
}

/// Os registros globais (`ViewContainerRegistry` e `ViewsRegistry`) e o `teardown` do teste.
struct Suite {
    view_containers_registry: Rc<ViewContainersRegistry>,
    views_registry: Rc<ViewsRegistry>,
    /// O `container` da suíte.
    container: RefCell<Option<ViewContainerRc>>,
}

impl Suite {
    fn start() -> Self {
        Self { view_containers_registry: view_containers_registry(), views_registry: views_registry(), container: RefCell::new(None) }
    }

    /// `container = ViewContainerRegistry.registerViewContainer({ id: 'test', title: nls.localize2('test', 'test'), ctorDescriptor: ... }, ViewContainerLocation.Sidebar)`.
    fn register_container(&self) -> ViewContainerRc {
        let container = self.view_containers_registry.register_view_container(
            IViewContainerDescriptor::new("test", localize2("test", "test", &[])),
            ViewContainerLocation::Sidebar,
            None,
        );
        *self.container.borrow_mut() = Some(container.clone());
        container
    }
}

impl Drop for Suite {
    /// `teardown`.
    fn drop(&mut self) {
        if std::thread::panicking() {
            return;
        }
        let container = self.container.borrow().clone();
        if let Some(container) = container {
            self.views_registry.deregister_views(&self.views_registry.get_views(&container), &container);
            self.view_containers_registry.deregister_view_container(&container);
        }
    }
}

/// O que o `setup` monta: o `contextKeyService`, o `storageService` e o `viewDescriptorService`.
struct Fixture {
    context_key_service: Rc<ContextKeyService>,
    storage_service: Rc<dyn StorageService>,
    view_descriptor_service: Rc<dyn IViewDescriptorService>,
}

impl Fixture {
    /// `setup`.
    fn setup(guard: &LeakGuard) -> Self {
        let services = ServiceCollection::new();
        let instantiation_service = guard.add_rc(InstantiationService::new(services.clone()));

        let storage_service = guard.add_rc(InMemoryStorageService::new());
        let storage: Rc<dyn StorageService> = Rc::new(storage_service.clone());
        let logger_service = guard.add_rc(null_logger_service());
        services.set(I_STORAGE_SERVICE, storage.clone());
        services.set(I_LOGGER_SERVICE, logger_service as Rc<dyn LoggerService>);
        services.set(I_EXTENSION_SERVICE, TestExtensionService::new() as Rc<dyn IExtensionService>);
        services.set(I_TELEMETRY_SERVICE, null_telemetry_service());
        services.set(I_WORKBENCH_ENVIRONMENT_SERVICE, TestEnvironmentService::new() as Rc<dyn IWorkbenchEnvironmentService>);

        let context_key_service = guard.add_rc(ContextKeyService::new(TestConfigurationService::new(None)));
        services.set(I_CONTEXT_KEY_SERVICE, context_key_service.clone() as Rc<dyn IContextKeyService>);
        let view_descriptor_service = guard.add_rc(instantiation_service.create_instance::<ViewDescriptorService>(()));

        Self { context_key_service, storage_service: storage, view_descriptor_service }
    }
}

impl Drop for Fixture {
    /// Entre o corpo do caso e os `teardown`, o `mocha` esvazia as microtarefas (`whenInstalledExtensionsRegistered().then(...)`).
    fn drop(&mut self) {
        if !std::thread::panicking() {
            runtime::run_microtasks();
        }
    }
}

/// Tudo o que cerca um caso. A ordem dos campos é a ordem de descarte (ver as notas do módulo).
struct TestEnv {
    fixture: Fixture,
    guard: LeakGuard,
    suite: Suite,
}

impl TestEnv {
    fn new() -> Self {
        let suite = Suite::start();
        let guard = ensure_no_disposables_are_leaked_in_test_suite();
        let fixture = Fixture::setup(&guard);
        Self { fixture, guard, suite }
    }

    /// `viewDescriptorService.getViewContainerModel(container)`.
    fn model(&self, container: &ViewContainerRc) -> Rc<dyn IViewContainerModel> {
        self.fixture.view_descriptor_service.get_view_container_model(container)
    }

    /// `disposableStore.add(new ViewDescriptorSequence(testObject))`.
    fn sequence(&self, model: &Rc<dyn IViewContainerModel>) -> Rc<ViewDescriptorSequence> {
        self.guard.add_rc(ViewDescriptorSequence::new(model))
    }

    /// `contextKeyService.createKey<boolean>(key, defaultValue)`.
    fn create_key(&self, key: &str, default_value: bool) -> Rc<dyn je_platform::contextkey::IContextKey> {
        self.fixture.context_key_service.create_key(key, Dynamic::Bool(default_value))
    }

    /// `storageService.store(key, value, StorageScope.PROFILE, target)`.
    fn store_in_profile(&self, key: &str, value: &serde_json::Value, target: StorageTarget) {
        self.fixture.storage_service.store(key, StorageValue::String(value.to_string()), StorageScope::Profile, target);
    }
}

/// `sinon.spy()` ligado a `event`: os argumentos de cada chamada, na ordem.
fn spy<T: Clone + 'static>(guard: &LeakGuard, event: Event<T>) -> Rc<RefCell<Vec<T>>> {
    let calls = Rc::new(RefCell::new(Vec::new()));
    let recorded = calls.clone();
    guard.add(event.listen(move |argument| recorded.borrow_mut().push(argument.clone())));
    calls
}

/// `{ id, ctorDescriptor: null!, name: nls.localize2(name, name) }`.
fn view_descriptor(id: &str, name: &str) -> IViewDescriptor {
    IViewDescriptor::new(id, localize2(name, name, &[]))
}

fn rc(view_descriptor: IViewDescriptor) -> ViewDescriptorRc {
    Rc::new(view_descriptor)
}

/// `assert.deepStrictEqual(actual, expected)` para listas de descritores (a mesma instância, na mesma ordem).
#[track_caller]
fn assert_views(actual: &[ViewDescriptorRc], expected: &[&ViewDescriptorRc], message: &str) {
    let ids = |views: &[&ViewDescriptorRc]| views.iter().map(|view| view.id.clone()).collect::<Vec<_>>();
    let actual_refs: Vec<&ViewDescriptorRc> = actual.iter().collect();
    assert!(
        actual.len() == expected.len() && actual.iter().zip(expected).all(|(actual, expected)| Rc::ptr_eq(actual, expected)),
        "{message}: esperado {:?}, recebido {:?}",
        ids(expected),
        ids(&actual_refs)
    );
}

/// `assert.deepStrictEqual(removeEvent.args[0][0], [{ viewDescriptor, index }, ...])`.
#[track_caller]
fn assert_removed(actual: &[IViewDescriptorRef], expected: &[(&ViewDescriptorRc, usize)]) {
    assert_eq!(actual.len(), expected.len());
    for (actual, (view_descriptor, index)) in actual.iter().zip(expected) {
        assert!(Rc::ptr_eq(&actual.view_descriptor, view_descriptor), "view {} no lugar de {}", actual.view_descriptor.id, view_descriptor.id);
        assert_eq!(actual.index, *index);
    }
}

/// `assert.deepStrictEqual(addEvent.args[0][0], [{ viewDescriptor, index, collapsed: false, size: undefined }, ...])`.
#[track_caller]
fn assert_added(actual: &[IAddedViewDescriptorRef], expected: &[(&ViewDescriptorRc, usize)]) {
    assert_eq!(actual.len(), expected.len());
    for (actual, (view_descriptor, index)) in actual.iter().zip(expected) {
        assert!(Rc::ptr_eq(&actual.view_descriptor, view_descriptor), "view {} no lugar de {}", actual.view_descriptor.id, view_descriptor.id);
        assert_eq!(actual.index, *index);
        assert!(!actual.collapsed);
        assert_eq!(actual.size, None);
    }
}

#[test]
fn empty_model() {
    let env = TestEnv::new();
    let container = env.suite.register_container();
    let test_object = env.model(&container);
    assert_eq!(test_object.visible_view_descriptors().len(), 0);
}

#[test]
fn register_unregister() {
    let env = TestEnv::new();
    let container = env.suite.register_container();
    let test_object = env.model(&container);
    let target = env.sequence(&test_object);

    assert_eq!(test_object.visible_view_descriptors().len(), 0);
    assert_eq!(target.elements().len(), 0);

    let view_descriptor = rc(view_descriptor("view1", "Test View 1"));

    env.suite.views_registry.register_views(vec![view_descriptor.clone()], &container);

    assert_eq!(test_object.visible_view_descriptors().len(), 1);
    assert_eq!(target.elements().len(), 1);
    assert!(Rc::ptr_eq(&test_object.visible_view_descriptors()[0], &view_descriptor));
    assert!(Rc::ptr_eq(&target.elements()[0], &view_descriptor));

    env.suite.views_registry.deregister_views(&[view_descriptor.clone()], &container);

    assert_eq!(test_object.visible_view_descriptors().len(), 0);
    assert_eq!(target.elements().len(), 0);
}

#[test]
fn when_contexts() {
    run_with_faked_timers(async {
        let env = TestEnv::new();
        let container = env.suite.register_container();
        let test_object = env.model(&container);
        let target = env.sequence(&test_object);
        assert_eq!(test_object.visible_view_descriptors().len(), 0);
        assert_eq!(target.elements().len(), 0);

        let view_descriptor = rc(IViewDescriptor { when: Some(ContextKeyExpr::equals("showview1", true)), ..view_descriptor("view1", "Test View 1") });

        env.suite.views_registry.register_views(vec![view_descriptor.clone()], &container);
        assert_eq!(test_object.visible_view_descriptors().len(), 0, "view should not appear since context isnt in");
        assert_eq!(target.elements().len(), 0);

        let key = env.create_key("showview1", false);
        assert_eq!(test_object.visible_view_descriptors().len(), 0, "view should still not appear since showview1 isnt true");
        assert_eq!(target.elements().len(), 0);

        key.set(Dynamic::Bool(true));
        timeout(30).await.unwrap();
        assert_eq!(test_object.visible_view_descriptors().len(), 1, "view should appear");
        assert_eq!(target.elements().len(), 1);
        assert!(Rc::ptr_eq(&test_object.visible_view_descriptors()[0], &view_descriptor));
        assert!(Rc::ptr_eq(&target.elements()[0], &view_descriptor));

        key.set(Dynamic::Bool(false));
        timeout(30).await.unwrap();
        assert_eq!(test_object.visible_view_descriptors().len(), 0, "view should disappear");
        assert_eq!(target.elements().len(), 0);

        env.suite.views_registry.deregister_views(&[view_descriptor.clone()], &container);
        assert_eq!(test_object.visible_view_descriptors().len(), 0, "view should not be there anymore");
        assert_eq!(target.elements().len(), 0);

        key.set(Dynamic::Bool(true));
        timeout(30).await.unwrap();
        assert_eq!(test_object.visible_view_descriptors().len(), 0, "view should not be there anymore");
        assert_eq!(target.elements().len(), 0);
    });
}

#[test]
fn when_contexts_multiple() {
    run_with_faked_timers(async {
        let env = TestEnv::new();
        let container = env.suite.register_container();
        let test_object = env.model(&container);
        let target = env.sequence(&test_object);
        let view1 = rc(view_descriptor("view1", "Test View 1"));
        let view2 = rc(IViewDescriptor { when: Some(ContextKeyExpr::equals("showview2", true)), ..view_descriptor("view2", "Test View 2") });

        env.suite.views_registry.register_views(vec![view1.clone(), view2.clone()], &container);
        assert_views(&test_object.visible_view_descriptors(), &[&view1], "only view1 should be visible");
        assert_views(&target.elements(), &[&view1], "only view1 should be visible");

        let key = env.create_key("showview2", false);
        assert_views(&test_object.visible_view_descriptors(), &[&view1], "still only view1 should be visible");
        assert_views(&target.elements(), &[&view1], "still only view1 should be visible");

        key.set(Dynamic::Bool(true));
        timeout(30).await.unwrap();
        assert_views(&test_object.visible_view_descriptors(), &[&view1, &view2], "both views should be visible");
        assert_views(&target.elements(), &[&view1, &view2], "both views should be visible");

        env.suite.views_registry.deregister_views(&[view1.clone(), view2.clone()], &container);
    });
}

#[test]
fn when_contexts_multiple_2() {
    run_with_faked_timers(async {
        let env = TestEnv::new();
        let container = env.suite.register_container();
        let test_object = env.model(&container);
        let target = env.sequence(&test_object);
        let view1 = rc(IViewDescriptor { when: Some(ContextKeyExpr::equals("showview1", true)), ..view_descriptor("view1", "Test View 1") });
        let view2 = rc(view_descriptor("view2", "Test View 2"));

        env.suite.views_registry.register_views(vec![view1.clone(), view2.clone()], &container);
        assert_views(&test_object.visible_view_descriptors(), &[&view2], "only view2 should be visible");
        assert_views(&target.elements(), &[&view2], "only view2 should be visible");

        let key = env.create_key("showview1", false);
        assert_views(&test_object.visible_view_descriptors(), &[&view2], "still only view2 should be visible");
        assert_views(&target.elements(), &[&view2], "still only view2 should be visible");

        key.set(Dynamic::Bool(true));
        timeout(30).await.unwrap();
        assert_views(&test_object.visible_view_descriptors(), &[&view1, &view2], "both views should be visible");
        assert_views(&target.elements(), &[&view1, &view2], "both views should be visible");

        env.suite.views_registry.deregister_views(&[view1.clone(), view2.clone()], &container);
    });
}

#[test]
fn set_visible() {
    let env = TestEnv::new();
    let container = env.suite.register_container();
    let test_object = env.model(&container);
    let target = env.sequence(&test_object);
    let toggleable = |id: &str, name: &str| rc(IViewDescriptor { can_toggle_visibility: true, ..view_descriptor(id, name) });
    let view1 = toggleable("view1", "Test View 1");
    let view2 = toggleable("view2", "Test View 2");
    let view3 = toggleable("view3", "Test View 3");

    env.suite.views_registry.register_views(vec![view1.clone(), view2.clone(), view3.clone()], &container);
    assert_views(&test_object.visible_view_descriptors(), &[&view1, &view2, &view3], "");
    assert_views(&target.elements(), &[&view1, &view2, &view3], "");

    test_object.set_visible("view2", true);
    assert_views(&test_object.visible_view_descriptors(), &[&view1, &view2, &view3], "nothing should happen");
    assert_views(&target.elements(), &[&view1, &view2, &view3], "");

    test_object.set_visible("view2", false);
    assert_views(&test_object.visible_view_descriptors(), &[&view1, &view3], "view2 should hide");
    assert_views(&target.elements(), &[&view1, &view3], "");

    test_object.set_visible("view1", false);
    assert_views(&test_object.visible_view_descriptors(), &[&view3], "view1 should hide");
    assert_views(&target.elements(), &[&view3], "");

    test_object.set_visible("view3", false);
    assert_views(&test_object.visible_view_descriptors(), &[], "view3 shoud hide");
    assert_views(&target.elements(), &[], "");

    test_object.set_visible("view1", true);
    assert_views(&test_object.visible_view_descriptors(), &[&view1], "view1 should show");
    assert_views(&target.elements(), &[&view1], "");

    test_object.set_visible("view3", true);
    assert_views(&test_object.visible_view_descriptors(), &[&view1, &view3], "view3 should show");
    assert_views(&target.elements(), &[&view1, &view3], "");

    test_object.set_visible("view2", true);
    assert_views(&test_object.visible_view_descriptors(), &[&view1, &view2, &view3], "view2 should show");
    assert_views(&target.elements(), &[&view1, &view2, &view3], "");

    env.suite.views_registry.deregister_views(&[view1.clone(), view2.clone(), view3.clone()], &container);
    assert_views(&test_object.visible_view_descriptors(), &[], "");
    assert_views(&target.elements(), &[], "");
}

#[test]
fn move_() {
    let env = TestEnv::new();
    let container = env.suite.register_container();
    let test_object = env.model(&container);
    let target = env.sequence(&test_object);
    let view1 = rc(view_descriptor("view1", "Test View 1"));
    let view2 = rc(view_descriptor("view2", "Test View 2"));
    let view3 = rc(view_descriptor("view3", "Test View 3"));

    env.suite.views_registry.register_views(vec![view1.clone(), view2.clone(), view3.clone()], &container);
    assert_views(&test_object.visible_view_descriptors(), &[&view1, &view2, &view3], "model views should be OK");
    assert_views(&target.elements(), &[&view1, &view2, &view3], "sql views should be OK");

    test_object.move_("view3", "view1");
    assert_views(&test_object.visible_view_descriptors(), &[&view3, &view1, &view2], "view3 should go to the front");
    assert_views(&target.elements(), &[&view3, &view1, &view2], "");

    test_object.move_("view1", "view2");
    assert_views(&test_object.visible_view_descriptors(), &[&view3, &view2, &view1], "view1 should go to the end");
    assert_views(&target.elements(), &[&view3, &view2, &view1], "");

    test_object.move_("view1", "view3");
    assert_views(&test_object.visible_view_descriptors(), &[&view1, &view3, &view2], "view1 should go to the front");
    assert_views(&target.elements(), &[&view1, &view3, &view2], "");

    test_object.move_("view2", "view3");
    assert_views(&test_object.visible_view_descriptors(), &[&view1, &view2, &view3], "view2 should go to the middle");
    assert_views(&target.elements(), &[&view1, &view2, &view3], "");
}

#[test]
fn view_states() {
    run_with_faked_timers(async {
        let env = TestEnv::new();
        env.store_in_profile("test.state.hidden", &json!([{ "id": "view1", "isHidden": true }]), StorageTarget::Machine);
        let container = env.suite.register_container();
        let test_object = env.model(&container);
        let target = env.sequence(&test_object);

        assert_eq!(test_object.visible_view_descriptors().len(), 0);
        assert_eq!(target.elements().len(), 0);

        let view_descriptor = rc(view_descriptor("view1", "Test View 1"));

        env.suite.views_registry.register_views(vec![view_descriptor.clone()], &container);
        assert_eq!(test_object.visible_view_descriptors().len(), 0, "view should not appear since it was set not visible in view state");
        assert_eq!(target.elements().len(), 0);
    });
}

#[test]
fn view_states_and_when_contexts() {
    run_with_faked_timers(async {
        let env = TestEnv::new();
        env.store_in_profile("test.state.hidden", &json!([{ "id": "view1", "isHidden": true }]), StorageTarget::Machine);
        let container = env.suite.register_container();
        let test_object = env.model(&container);
        let target = env.sequence(&test_object);

        assert_eq!(test_object.visible_view_descriptors().len(), 0);
        assert_eq!(target.elements().len(), 0);

        let view_descriptor = rc(IViewDescriptor { when: Some(ContextKeyExpr::equals("showview1", true)), ..view_descriptor("view1", "Test View 1") });

        env.suite.views_registry.register_views(vec![view_descriptor.clone()], &container);
        assert_eq!(test_object.visible_view_descriptors().len(), 0, "view should not appear since context isnt in");
        assert_eq!(target.elements().len(), 0);

        let key = env.create_key("showview1", false);
        assert_eq!(test_object.visible_view_descriptors().len(), 0, "view should still not appear since showview1 isnt true");
        assert_eq!(target.elements().len(), 0);

        key.set(Dynamic::Bool(true));
        timeout(30).await.unwrap();
        assert_eq!(test_object.visible_view_descriptors().len(), 0, "view should still not appear since it was set not visible in view state");
        assert_eq!(target.elements().len(), 0);
    });
}

#[test]
fn view_states_and_when_contexts_multiple_views() {
    run_with_faked_timers(async {
        let env = TestEnv::new();
        env.store_in_profile("test.state.hidden", &json!([{ "id": "view1", "isHidden": true }]), StorageTarget::Machine);
        let container = env.suite.register_container();
        let test_object = env.model(&container);
        let target = env.sequence(&test_object);

        assert_eq!(test_object.visible_view_descriptors().len(), 0);
        assert_eq!(target.elements().len(), 0);

        let view1 = rc(IViewDescriptor { when: Some(ContextKeyExpr::equals("showview", true)), ..view_descriptor("view1", "Test View 1") });
        let view2 = rc(view_descriptor("view2", "Test View 2"));
        let view3 = rc(IViewDescriptor { when: Some(ContextKeyExpr::equals("showview", true)), ..view_descriptor("view3", "Test View 3") });

        env.suite.views_registry.register_views(vec![view1.clone(), view2.clone(), view3.clone()], &container);
        assert_views(&test_object.visible_view_descriptors(), &[&view2], "Only view2 should be visible");
        assert_views(&target.elements(), &[&view2], "");

        let key = env.create_key("showview", false);
        assert_views(&test_object.visible_view_descriptors(), &[&view2], "Only view2 should be visible");
        assert_views(&target.elements(), &[&view2], "");

        key.set(Dynamic::Bool(true));
        timeout(30).await.unwrap();
        assert_views(&test_object.visible_view_descriptors(), &[&view2, &view3], "view3 should be visible");
        assert_views(&target.elements(), &[&view2, &view3], "");

        key.set(Dynamic::Bool(false));
        timeout(30).await.unwrap();
        assert_views(&test_object.visible_view_descriptors(), &[&view2], "Only view2 should be visible");
        assert_views(&target.elements(), &[&view2], "");
    });
}

#[test]
fn remove_event_is_not_triggered_if_view_was_hidden_and_removed() {
    run_with_faked_timers(async {
        let env = TestEnv::new();
        let container = env.suite.register_container();
        let test_object = env.model(&container);
        let target = env.sequence(&test_object);
        let view_descriptor = rc(IViewDescriptor {
            when: Some(ContextKeyExpr::equals("showview1", true)),
            can_toggle_visibility: true,
            ..view_descriptor("view1", "Test View 1")
        });

        env.suite.views_registry.register_views(vec![view_descriptor.clone()], &container);

        let key = env.create_key("showview1", true);
        timeout(30).await.unwrap();
        assert_eq!(test_object.visible_view_descriptors().len(), 1, "view should appear after context is set");
        assert_eq!(target.elements().len(), 1);

        test_object.set_visible("view1", false);
        assert_eq!(test_object.visible_view_descriptors().len(), 0, "view should disappear after setting visibility to false");
        assert_eq!(target.elements().len(), 0);

        let target_event = spy(&env.guard, test_object.on_did_remove_visible_view_descriptors());
        key.set(Dynamic::Bool(false));
        timeout(30).await.unwrap();
        assert!(target_event.borrow().is_empty(), "remove event should not be called since it is already hidden");
    });
}

#[test]
fn add_event_is_not_triggered_if_view_was_set_visible_when_visible_and_not_active() {
    run_with_faked_timers(async {
        let env = TestEnv::new();
        let container = env.suite.register_container();
        let test_object = env.model(&container);
        let target = env.sequence(&test_object);
        let view_descriptor = rc(IViewDescriptor {
            when: Some(ContextKeyExpr::equals("showview1", true)),
            can_toggle_visibility: true,
            ..view_descriptor("view1", "Test View 1")
        });

        let key = env.create_key("showview1", true);
        key.set(Dynamic::Bool(false));
        env.suite.views_registry.register_views(vec![view_descriptor.clone()], &container);

        assert_eq!(test_object.visible_view_descriptors().len(), 0);
        assert_eq!(target.elements().len(), 0);

        let target_event = spy(&env.guard, test_object.on_did_add_visible_view_descriptors());
        test_object.set_visible("view1", true);
        assert!(target_event.borrow().is_empty(), "add event should not be called since it is already visible");
        assert_eq!(test_object.visible_view_descriptors().len(), 0);
        assert_eq!(target.elements().len(), 0);
    });
}

#[test]
fn remove_event_is_not_triggered_if_view_was_hidden_and_not_active() {
    run_with_faked_timers(async {
        let env = TestEnv::new();
        let container = env.suite.register_container();
        let test_object = env.model(&container);
        let target = env.sequence(&test_object);
        let view_descriptor = rc(IViewDescriptor {
            when: Some(ContextKeyExpr::equals("showview1", true)),
            can_toggle_visibility: true,
            ..view_descriptor("view1", "Test View 1")
        });

        let key = env.create_key("showview1", true);
        key.set(Dynamic::Bool(false));
        env.suite.views_registry.register_views(vec![view_descriptor.clone()], &container);

        assert_eq!(test_object.visible_view_descriptors().len(), 0);
        assert_eq!(target.elements().len(), 0);

        let target_event = spy(&env.guard, test_object.on_did_add_visible_view_descriptors());
        test_object.set_visible("view1", false);
        assert!(target_event.borrow().is_empty(), "add event should not be called since it is disabled");
        assert_eq!(test_object.visible_view_descriptors().len(), 0);
        assert_eq!(target.elements().len(), 0);
    });
}

#[test]
fn add_event_is_not_triggered_if_view_was_set_visible_when_not_visible_and_not_active() {
    run_with_faked_timers(async {
        let env = TestEnv::new();
        let container = env.suite.register_container();
        let test_object = env.model(&container);
        let target = env.sequence(&test_object);
        let view_descriptor = rc(IViewDescriptor {
            when: Some(ContextKeyExpr::equals("showview1", true)),
            can_toggle_visibility: true,
            ..view_descriptor("view1", "Test View 1")
        });

        let key = env.create_key("showview1", true);
        key.set(Dynamic::Bool(false));
        env.suite.views_registry.register_views(vec![view_descriptor.clone()], &container);

        assert_eq!(test_object.visible_view_descriptors().len(), 0);
        assert_eq!(target.elements().len(), 0);

        test_object.set_visible("view1", false);
        assert_eq!(test_object.visible_view_descriptors().len(), 0);
        assert_eq!(target.elements().len(), 0);

        let target_event = spy(&env.guard, test_object.on_did_add_visible_view_descriptors());
        test_object.set_visible("view1", true);
        assert!(target_event.borrow().is_empty(), "add event should not be called since it is disabled");
        assert_eq!(test_object.visible_view_descriptors().len(), 0);
        assert_eq!(target.elements().len(), 0);
    });
}

#[test]
fn added_view_descriptors_are_in_ascending_order_in_the_event() {
    run_with_faked_timers(async {
        let env = TestEnv::new();
        let container = env.suite.register_container();
        let test_object = env.model(&container);
        let target = env.sequence(&test_object);
        let ordered = |id: &str, name: &str, order: f64| rc(IViewDescriptor { can_toggle_visibility: true, order: Some(order), ..view_descriptor(id, name) });

        env.suite.views_registry.register_views(vec![ordered("view5", "Test View 5", 5.0), ordered("view2", "Test View 2", 2.0)], &container);

        assert_eq!(target.elements().len(), 2);
        assert_eq!(target.elements()[0].id, "view2");
        assert_eq!(target.elements()[1].id, "view5");

        env.suite
            .views_registry
            .register_views(vec![ordered("view4", "Test View 4", 4.0), ordered("view3", "Test View 3", 3.0), ordered("view1", "Test View 1", 1.0)], &container);

        assert_eq!(target.elements().len(), 5);
        assert_eq!(target.elements()[0].id, "view1");
        assert_eq!(target.elements()[1].id, "view2");
        assert_eq!(target.elements()[2].id, "view3");
        assert_eq!(target.elements()[3].id, "view4");
        assert_eq!(target.elements()[4].id, "view5");
    });
}

#[test]
fn add_event_is_triggered_only_once_when_view_is_set_visible_while_it_is_set_active() {
    run_with_faked_timers(async {
        let env = TestEnv::new();
        let container = env.suite.register_container();
        let test_object = env.model(&container);
        let target = env.sequence(&test_object);
        let view_descriptor = rc(IViewDescriptor {
            when: Some(ContextKeyExpr::equals("showview1", true)),
            can_toggle_visibility: true,
            ..view_descriptor("view1", "Test View 1")
        });

        let key = env.create_key("showview1", true);
        key.set(Dynamic::Bool(false));
        env.suite.views_registry.register_views(vec![view_descriptor.clone()], &container);
        test_object.set_visible("view1", false);

        assert_eq!(test_object.visible_view_descriptors().len(), 0);
        assert_eq!(target.elements().len(), 0);

        let target_event = spy(&env.guard, test_object.on_did_add_visible_view_descriptors());
        let model = test_object.clone();
        env.guard.add(test_object.on_did_change_active_view_descriptors().once().listen(move |_| model.set_visible("view1", true)));
        key.set(Dynamic::Bool(true));
        timeout(30).await.unwrap();
        assert_eq!(target_event.borrow().len(), 1);
        assert_eq!(test_object.visible_view_descriptors().len(), 1);
        assert_eq!(target.elements().len(), 1);
        assert_eq!(target.elements()[0].id, "view1");
    });
}

#[test]
fn add_event_is_not_triggered_only_when_view_is_set_hidden_while_it_is_set_active() {
    run_with_faked_timers(async {
        let env = TestEnv::new();
        let container = env.suite.register_container();
        let test_object = env.model(&container);
        let target = env.sequence(&test_object);
        let view_descriptor = rc(IViewDescriptor {
            when: Some(ContextKeyExpr::equals("showview1", true)),
            can_toggle_visibility: true,
            ..view_descriptor("view1", "Test View 1")
        });

        let key = env.create_key("showview1", true);
        key.set(Dynamic::Bool(false));
        env.suite.views_registry.register_views(vec![view_descriptor.clone()], &container);

        assert_eq!(test_object.visible_view_descriptors().len(), 0);
        assert_eq!(target.elements().len(), 0);

        let target_event = spy(&env.guard, test_object.on_did_add_visible_view_descriptors());
        let model = test_object.clone();
        env.guard.add(test_object.on_did_change_active_view_descriptors().once().listen(move |_| model.set_visible("view1", false)));
        key.set(Dynamic::Bool(true));
        timeout(30).await.unwrap();
        assert_eq!(target_event.borrow().len(), 0);
        assert_eq!(test_object.visible_view_descriptors().len(), 0);
        assert_eq!(target.elements().len(), 0);
    });
}

#[test]
fn issue_142087_view_descriptor_visibility_is_not_reset() {
    run_with_faked_timers(async {
        let env = TestEnv::new();
        let container = env.suite.register_container();
        let test_object = env.model(&container);
        let view_descriptor = rc(IViewDescriptor { can_toggle_visibility: true, ..view_descriptor("view1", "Test View 1") });

        env.store_in_profile(
            &get_views_state_storage_id("test.state"),
            &json!([{ "id": view_descriptor.id, "isHidden": true }]),
            StorageTarget::User,
        );

        env.suite.views_registry.register_views(vec![view_descriptor.clone()], &container);

        assert!(!test_object.is_visible(&view_descriptor.id));
        assert_eq!(test_object.active_view_descriptors()[0].id, view_descriptor.id);
        assert_eq!(test_object.visible_view_descriptors().len(), 0);
    });
}

#[test]
fn remove_event_is_triggered_properly_if_multiple_views_are_hidden_at_the_same_time() {
    run_with_faked_timers(async {
        let env = TestEnv::new();
        let container = env.suite.register_container();
        let test_object = env.model(&container);
        let target = env.sequence(&test_object);
        let view_descriptor1 = rc(IViewDescriptor { can_toggle_visibility: true, ..view_descriptor("view1", "Test View 1") });
        let view_descriptor2 = rc(IViewDescriptor { can_toggle_visibility: true, ..view_descriptor("view2", "Test View 2") });
        let view_descriptor3 = rc(IViewDescriptor { can_toggle_visibility: true, ..view_descriptor("view3", "Test View 3") });

        env.suite.views_registry.register_views(vec![view_descriptor1.clone(), view_descriptor2.clone(), view_descriptor3.clone()], &container);

        let remove_event = spy(&env.guard, test_object.on_did_remove_visible_view_descriptors());
        let add_event = spy(&env.guard, test_object.on_did_add_visible_view_descriptors());

        env.store_in_profile(
            &get_views_state_storage_id("test.state"),
            &json!([
                { "id": view_descriptor1.id, "isHidden": false },
                { "id": view_descriptor2.id, "isHidden": true },
                { "id": view_descriptor3.id, "isHidden": true }
            ]),
            StorageTarget::User,
        );

        assert!(add_event.borrow().is_empty(), "add event should not be called");
        assert_eq!(remove_event.borrow().len(), 1, "remove event should be called");
        assert_removed(&remove_event.borrow()[0], &[(&view_descriptor3, 2), (&view_descriptor2, 1)]);
        assert_eq!(target.elements().len(), 1);
        assert_eq!(target.elements()[0].id, view_descriptor1.id);
    });
}

#[test]
fn add_event_is_triggered_properly_if_multiple_views_are_hidden_at_the_same_time() {
    run_with_faked_timers(async {
        let env = TestEnv::new();
        let container = env.suite.register_container();
        let test_object = env.model(&container);
        let target = env.sequence(&test_object);
        let view_descriptor1 = rc(IViewDescriptor { can_toggle_visibility: true, ..view_descriptor("view1", "Test View 1") });
        let view_descriptor2 = rc(IViewDescriptor { can_toggle_visibility: true, ..view_descriptor("view2", "Test View 2") });
        let view_descriptor3 = rc(IViewDescriptor { can_toggle_visibility: true, ..view_descriptor("view3", "Test View 3") });

        env.suite.views_registry.register_views(vec![view_descriptor1.clone(), view_descriptor2.clone(), view_descriptor3.clone()], &container);
        test_object.set_visible(&view_descriptor1.id, false);
        test_object.set_visible(&view_descriptor3.id, false);

        let remove_event = spy(&env.guard, test_object.on_did_remove_visible_view_descriptors());
        let add_event = spy(&env.guard, test_object.on_did_add_visible_view_descriptors());

        env.store_in_profile(
            &get_views_state_storage_id("test.state"),
            &json!([
                { "id": view_descriptor1.id, "isHidden": false },
                { "id": view_descriptor2.id, "isHidden": false },
                { "id": view_descriptor3.id, "isHidden": false }
            ]),
            StorageTarget::User,
        );

        assert!(remove_event.borrow().is_empty(), "remove event should not be called");

        assert_eq!(add_event.borrow().len(), 1, "add event should be called once");
        assert_added(&add_event.borrow()[0], &[(&view_descriptor1, 0), (&view_descriptor3, 2)]);

        assert_eq!(target.elements().len(), 3);
        assert_eq!(target.elements()[0].id, view_descriptor1.id);
        assert_eq!(target.elements()[1].id, view_descriptor2.id);
        assert_eq!(target.elements()[2].id, view_descriptor3.id);
    });
}

#[test]
fn add_and_remove_events_are_triggered_properly_if_multiple_views_are_hidden_and_added_at_the_same_time() {
    run_with_faked_timers(async {
        let env = TestEnv::new();
        let container = env.suite.register_container();
        let test_object = env.model(&container);
        let target = env.sequence(&test_object);
        let view_descriptor1 = rc(IViewDescriptor { can_toggle_visibility: true, ..view_descriptor("view1", "Test View 1") });
        let view_descriptor2 = rc(IViewDescriptor { can_toggle_visibility: true, ..view_descriptor("view2", "Test View 2") });
        let view_descriptor3 = rc(IViewDescriptor { can_toggle_visibility: true, ..view_descriptor("view3", "Test View 3") });
        let view_descriptor4 = rc(IViewDescriptor { can_toggle_visibility: true, ..view_descriptor("view4", "Test View 4") });

        env.suite
            .views_registry
            .register_views(vec![view_descriptor1.clone(), view_descriptor2.clone(), view_descriptor3.clone(), view_descriptor4.clone()], &container);
        test_object.set_visible(&view_descriptor1.id, false);

        let remove_event = spy(&env.guard, test_object.on_did_remove_visible_view_descriptors());
        let add_event = spy(&env.guard, test_object.on_did_add_visible_view_descriptors());

        env.store_in_profile(
            &get_views_state_storage_id("test.state"),
            &json!([
                { "id": view_descriptor1.id, "isHidden": false },
                { "id": view_descriptor2.id, "isHidden": true },
                { "id": view_descriptor3.id, "isHidden": false },
                { "id": view_descriptor4.id, "isHidden": true }
            ]),
            StorageTarget::User,
        );

        assert_eq!(remove_event.borrow().len(), 1, "remove event should be called once");
        assert_removed(&remove_event.borrow()[0], &[(&view_descriptor4, 2), (&view_descriptor2, 0)]);

        assert_eq!(add_event.borrow().len(), 1, "add event should be called once");
        assert_added(&add_event.borrow()[0], &[(&view_descriptor1, 0)]);
        assert_eq!(target.elements().len(), 2);
        assert_eq!(target.elements()[0].id, view_descriptor1.id);
        assert_eq!(target.elements()[1].id, view_descriptor3.id);
    });
}

#[test]
fn newly_added_view_descriptor_is_hidden_if_it_was_toggled_hidden_in_storage_before_adding() {
    run_with_faked_timers(async {
        let env = TestEnv::new();
        let container = env.suite.register_container();
        let view_descriptor = rc(IViewDescriptor { can_toggle_visibility: true, ..view_descriptor("view1", "Test View 1") });
        env.store_in_profile(
            &get_views_state_storage_id("test.state"),
            &json!([{ "id": view_descriptor.id, "isHidden": false }]),
            StorageTarget::User,
        );

        let test_object = env.model(&container);

        env.store_in_profile(
            &get_views_state_storage_id("test.state"),
            &json!([{ "id": view_descriptor.id, "isHidden": true }]),
            StorageTarget::User,
        );

        env.suite.views_registry.register_views(vec![view_descriptor.clone()], &container);

        assert!(!test_object.is_visible(&view_descriptor.id));
        assert_eq!(test_object.active_view_descriptors()[0].id, view_descriptor.id);
        assert_eq!(test_object.visible_view_descriptors().len(), 0);
    });
}
