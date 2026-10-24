//! Porte de `src/vs/workbench/services/views/test/browser/viewDescriptorService.test.ts` (suíte `ViewDescriptorService`), caso a caso e na mesma ordem.
//!
//! Notas de tradução:
//! - **`ensureNoDisposablesAreLeakedInTestSuite()`, `setup` e `teardown`.** O [`TestEnv`] reúne o que o `mocha` roda em torno de
//!   cada caso, e a ordem dos campos é a ordem em que eles são descartados: o [`Fixture`] (o `setup`, que também esvazia as
//!   microtarefas antes de descartar), o guarda de vazamentos e a [`Suite`] (o `teardown` do teste, que tira as views dos
//!   contêineres de teste do registro).
//! - **Contêineres de módulo.** `sidebarContainer` e `panelContainer` são constantes de módulo no original; aqui a [`Suite`] os
//!   registra no começo de cada caso, porque os registros são globais por thread e cada `#[test]` roda na sua thread, com
//!   registros novos. Os ids levam o mesmo prefixo (`testViewContainer`) e um `generateUuid()`.
//! - **`workbenchInstantiationService(undefined, disposables)`.** Só os serviços de que o `ViewDescriptorService` depende:
//!   `IStorageService` (`InMemoryStorageService`), `IExtensionService` (`TestExtensionService`), `ITelemetryService` (o nulo),
//!   `ILoggerService` (o nulo), `IWorkbenchEnvironmentService` (`TestEnvironmentService`) e o `ContextKeyService` real, no
//!   lugar do `IContextKeyService` (`instantiationService.stub(IContextKeyService, ...)`).
//! - **`aViewDescriptorService()`** é [`TestEnv::view_descriptor_service`], que cria uma instância nova a cada chamada.
//! - **`ctorDescriptor: null!` e `new SyncDescriptor(<any>{})`** são `ctor_descriptor: None` (nenhum caso instancia o contêiner
//!   nem a view).
//! - **Identidade.** `assert.strictEqual(container, outro)` entre contêineres e `indexOf(viewDescriptor)` são `Rc::ptr_eq`.
//! - **`deepStrictEqual(x, null)`** para `getViewContainerById` é `Option::None`.
//! - **Casos `async`** deste arquivo não têm nenhum `await`, então rodam como funções comuns.

#[path = "../../je-base/tests/common/mod.rs"]
mod common;

use std::cell::RefCell;
use std::rc::Rc;

use common::{LeakGuard, ensure_no_disposables_are_leaked_in_test_suite};
use je_base::nls::localize2;
use je_base::runtime;
use je_base::uuid::generate_uuid;
use je_instantiation::{InstantiationService, ServiceCollection};
use je_platform::configuration::test_configuration_service::TestConfigurationService;
use je_platform::contextkey::service::ContextKeyService;
use je_platform::contextkey::{I_CONTEXT_KEY_SERVICE, IContextKeyService};
use je_platform::log::{I_LOGGER_SERVICE, LoggerService, null_logger_service};
use je_platform::storage::{I_STORAGE_SERVICE, InMemoryStorageService, StorageScope, StorageService, StorageTarget};
use je_platform::telemetry::telemetry::I_TELEMETRY_SERVICE;
use je_platform::telemetry::telemetry_utils::null_telemetry_service;
use je_storage::StorageValue;
use je_workbench::common::views::{
    IViewContainerDescriptor, IViewContainerModel, IViewDescriptor, IViewDescriptorService, ViewContainerLocation, ViewContainerRc, ViewContainersRegistry,
    ViewDescriptorRc, ViewsRegistry, view_container_location_to_string, view_containers_registry, views_registry,
};
use je_workbench::services::environment::environment_service::{I_WORKBENCH_ENVIRONMENT_SERVICE, IWorkbenchEnvironmentService};
use je_workbench::services::extensions::extensions::{I_EXTENSION_SERVICE, IExtensionService};
use je_workbench::services::views::view_descriptor_service::ViewDescriptorService;
use je_workbench::test::browser::workbench_test_services::{TestEnvironmentService, TestExtensionService};
use serde_json::{Value, json};

const VIEW_CONTAINER_ID_PREFIX: &str = "testViewContainer";

/// Os registros globais e o `teardown` do teste, mais os dois contêineres de módulo.
struct Suite {
    view_containers_registry: Rc<ViewContainersRegistry>,
    views_registry: Rc<ViewsRegistry>,
    sidebar_container: ViewContainerRc,
    panel_container: ViewContainerRc,
}

impl Suite {
    fn start() -> Self {
        let view_containers_registry = view_containers_registry();
        let views_registry = views_registry();
        let sidebar_container = Self::register_container_in(&view_containers_registry, ViewContainerLocation::Sidebar);
        let panel_container = Self::register_container_in(&view_containers_registry, ViewContainerLocation::Panel);
        Self { view_containers_registry, views_registry, sidebar_container, panel_container }
    }

    /// `ViewContainersRegistry.registerViewContainer({ id: \`${viewContainerIdPrefix}-${generateUuid()}\`, title: nls.localize2('test', 'test'), ctorDescriptor: ... }, location)`.
    fn register_container_in(registry: &ViewContainersRegistry, location: ViewContainerLocation) -> ViewContainerRc {
        registry.register_view_container(
            IViewContainerDescriptor::new(&format!("{VIEW_CONTAINER_ID_PREFIX}-{}", generate_uuid()), localize2("test", "test", &[])),
            location,
            None,
        )
    }

    fn register_container(&self, location: ViewContainerLocation) -> ViewContainerRc {
        Self::register_container_in(&self.view_containers_registry, location)
    }
}

impl Drop for Suite {
    /// `teardown`.
    fn drop(&mut self) {
        if std::thread::panicking() {
            return;
        }
        for view_container in self.view_containers_registry.all() {
            if view_container.id.starts_with(VIEW_CONTAINER_ID_PREFIX) {
                self.views_registry.deregister_views(&self.views_registry.get_views(&view_container), &view_container);
            }
        }
    }
}

/// O que o `setup` monta: os serviços, sem o `ViewDescriptorService`, que cada caso cria quando precisa.
struct Fixture {
    instantiation_service: Rc<InstantiationService>,
    storage_service: Rc<dyn StorageService>,
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
        services.set(I_CONTEXT_KEY_SERVICE, context_key_service as Rc<dyn IContextKeyService>);

        Self { instantiation_service, storage_service: storage }
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

    /// `aViewDescriptorService()`.
    fn view_descriptor_service(&self) -> Rc<ViewDescriptorService> {
        self.guard.add_rc(self.fixture.instantiation_service.create_instance::<ViewDescriptorService>(()))
    }

    /// `storageService.store('views.customizations', value, StorageScope.PROFILE, StorageTarget.USER)`.
    fn store_customizations(&self, value: &Value) {
        self.fixture.storage_service.store("views.customizations", StorageValue::String(value.to_string()), StorageScope::Profile, StorageTarget::User);
    }

    /// `JSON.parse(storageService.get('views.customizations', StorageScope.PROFILE)!)`.
    fn stored_customizations(&self) -> Value {
        let stored = self.fixture.storage_service.get("views.customizations", StorageScope::Profile, None).expect("views.customizations não foi gravado");
        serde_json::from_str(&stored).expect("views.customizations é um JSON válido")
    }
}

/// `{ id, ctorDescriptor: null!, name: nls.localize2(name, name), canMoveView: true }`, com a ordem opcional.
fn movable_view(id: &str, name: &str, order: Option<f64>) -> ViewDescriptorRc {
    Rc::new(IViewDescriptor { can_move_view: true, order, ..IViewDescriptor::new(id, localize2(name, name, &[])) })
}

/// Os descritores `view1`, `view2`, ... do original, com `order` 1, 2, ... quando `ordered`.
fn movable_views(count: usize, ordered: bool) -> Vec<ViewDescriptorRc> {
    (1..=count).map(|number| movable_view(&format!("view{number}"), &format!("Test View {number}"), ordered.then_some(number as f64))).collect()
}

/// `model.allViewDescriptors.map(v => v.id)`.
fn all_view_ids(model: &Rc<dyn IViewContainerModel>) -> Vec<String> {
    model.all_view_descriptors().iter().map(|view| view.id.clone()).collect()
}

fn ids(expected: &[&str]) -> Vec<String> {
    expected.iter().map(|id| id.to_string()).collect()
}

/// `${viewContainerIdPrefix}-${generateUuid()}`.
fn generated_container_id(location: ViewContainerLocation) -> String {
    format!("workbench.views.service.{}.{}", view_container_location_to_string(location), generate_uuid())
}

/// `viewDescriptors.indexOf(view) !== -1`.
fn contains(views: &[ViewDescriptorRc], view: &ViewDescriptorRc) -> bool {
    views.iter().any(|candidate| Rc::ptr_eq(candidate, view))
}

#[test]
fn empty_containers() {
    let env = TestEnv::new();
    let test_object = env.view_descriptor_service();
    let sidebar_views = test_object.get_view_container_model(&env.suite.sidebar_container);
    let panel_views = test_object.get_view_container_model(&env.suite.panel_container);
    assert_eq!(sidebar_views.all_view_descriptors().len(), 0, "The sidebar container should have no views yet.");
    assert_eq!(panel_views.all_view_descriptors().len(), 0, "The panel container should have no views yet.");
}

#[test]
fn register_deregister() {
    let env = TestEnv::new();
    let test_object = env.view_descriptor_service();
    let view_descriptors = movable_views(3, false);

    env.suite.views_registry.register_views(view_descriptors[..2].to_vec(), &env.suite.sidebar_container);
    env.suite.views_registry.register_views(view_descriptors[2..].to_vec(), &env.suite.panel_container);

    let mut sidebar_views = test_object.get_view_container_model(&env.suite.sidebar_container);
    let mut panel_views = test_object.get_view_container_model(&env.suite.panel_container);

    assert_eq!(sidebar_views.active_view_descriptors().len(), 2, "Sidebar should have 2 views");
    assert_eq!(panel_views.active_view_descriptors().len(), 1, "Panel should have 1 view");

    env.suite.views_registry.deregister_views(&view_descriptors[..2], &env.suite.sidebar_container);
    env.suite.views_registry.deregister_views(&view_descriptors[2..], &env.suite.panel_container);

    sidebar_views = test_object.get_view_container_model(&env.suite.sidebar_container);
    panel_views = test_object.get_view_container_model(&env.suite.panel_container);

    assert_eq!(sidebar_views.active_view_descriptors().len(), 0, "Sidebar should have no views");
    assert_eq!(panel_views.active_view_descriptors().len(), 0, "Panel should have no views");
}

#[test]
fn move_views_to_existing_containers() {
    let env = TestEnv::new();
    let test_object = env.view_descriptor_service();
    let view_descriptors = movable_views(3, false);

    env.suite.views_registry.register_views(view_descriptors[..2].to_vec(), &env.suite.sidebar_container);
    env.suite.views_registry.register_views(view_descriptors[2..].to_vec(), &env.suite.panel_container);

    test_object.move_views_to_container(&view_descriptors[2..], &env.suite.sidebar_container, None, None);
    test_object.move_views_to_container(&view_descriptors[..2], &env.suite.panel_container, None, None);

    let sidebar_views = test_object.get_view_container_model(&env.suite.sidebar_container);
    let panel_views = test_object.get_view_container_model(&env.suite.panel_container);

    assert_eq!(sidebar_views.active_view_descriptors().len(), 1, "Sidebar should have 2 views");
    assert_eq!(panel_views.active_view_descriptors().len(), 2, "Panel should have 1 view");

    assert!(contains(&sidebar_views.active_view_descriptors(), &view_descriptors[2]), "Sidebar should have {}", view_descriptors[2].name.value);
    assert!(contains(&panel_views.active_view_descriptors(), &view_descriptors[0]), "Panel should have {}", view_descriptors[0].name.value);
    assert!(contains(&panel_views.active_view_descriptors(), &view_descriptors[1]), "Panel should have {}", view_descriptors[1].name.value);
}

#[test]
fn move_views_to_generated_containers() {
    let env = TestEnv::new();
    let test_object = env.view_descriptor_service();
    let view_descriptors = movable_views(3, false);

    env.suite.views_registry.register_views(view_descriptors[..2].to_vec(), &env.suite.sidebar_container);
    env.suite.views_registry.register_views(view_descriptors[2..].to_vec(), &env.suite.panel_container);

    test_object.move_view_to_location(&view_descriptors[0], ViewContainerLocation::Panel, None);
    test_object.move_view_to_location(&view_descriptors[2], ViewContainerLocation::Sidebar, None);

    let mut sidebar_views = test_object.get_view_container_model(&env.suite.sidebar_container);
    let mut panel_views = test_object.get_view_container_model(&env.suite.panel_container);

    assert_eq!(sidebar_views.active_view_descriptors().len(), 1, "Sidebar container should have 1 view");
    assert_eq!(panel_views.active_view_descriptors().len(), 0, "Panel container should have no views");

    let generated_panel = test_object.get_view_container_by_view_id(&view_descriptors[0].id).expect("assertReturnsDefined");
    let generated_sidebar = test_object.get_view_container_by_view_id(&view_descriptors[2].id).expect("assertReturnsDefined");

    assert_eq!(test_object.get_view_container_location(&generated_panel), Some(ViewContainerLocation::Panel), "Generated Panel should be in located in the panel");
    assert_eq!(
        test_object.get_view_container_location(&generated_sidebar),
        Some(ViewContainerLocation::Sidebar),
        "Generated Sidebar should be in located in the sidebar"
    );

    assert_eq!(
        test_object.get_view_container_location(&generated_panel),
        test_object.get_view_location_by_id(&view_descriptors[0].id),
        "Panel view location and container location should match"
    );
    assert_eq!(
        test_object.get_view_container_location(&generated_sidebar),
        test_object.get_view_location_by_id(&view_descriptors[2].id),
        "Sidebar view location and container location should match"
    );

    let default_of_view3 = test_object.get_default_container_by_id(&view_descriptors[2].id);
    assert!(default_of_view3.is_some_and(|container| Rc::ptr_eq(&container, &env.suite.panel_container)), "{} has wrong default container", view_descriptors[2].name.value);
    let default_of_view1 = test_object.get_default_container_by_id(&view_descriptors[0].id);
    assert!(default_of_view1.is_some_and(|container| Rc::ptr_eq(&container, &env.suite.sidebar_container)), "{} has wrong default container", view_descriptors[0].name.value);

    test_object.move_view_to_location(&view_descriptors[0], ViewContainerLocation::Sidebar, None);
    test_object.move_view_to_location(&view_descriptors[2], ViewContainerLocation::Panel, None);

    sidebar_views = test_object.get_view_container_model(&env.suite.sidebar_container);
    panel_views = test_object.get_view_container_model(&env.suite.panel_container);

    assert_eq!(sidebar_views.active_view_descriptors().len(), 1, "Sidebar should have 2 views");
    assert_eq!(panel_views.active_view_descriptors().len(), 0, "Panel should have 1 view");

    assert_eq!(test_object.get_view_location_by_id(&view_descriptors[0].id), Some(ViewContainerLocation::Sidebar), "View should be located in the sidebar");
    assert_eq!(test_object.get_view_location_by_id(&view_descriptors[2].id), Some(ViewContainerLocation::Panel), "View should be located in the panel");
}

#[test]
fn move_view_events() {
    let env = TestEnv::new();
    let test_object = env.view_descriptor_service();
    let view_descriptors = movable_views(3, false);

    let mut expected_sequence = String::new();
    let actual_sequence = Rc::new(RefCell::new(String::new()));

    let container_move_string = |view: &IViewDescriptor, from: &ViewContainerRc, to: &ViewContainerRc| format!("Moved {} from {} to {}\n", view.id, from.id, to.id);

    let location_name = |location: ViewContainerLocation| if location == ViewContainerLocation::Sidebar { "Sidebar" } else { "Panel" };
    let location_move_string =
        |view: &IViewDescriptor, from: ViewContainerLocation, to: ViewContainerLocation| format!("Moved {} from {} to {}\n", view.id, location_name(from), location_name(to));

    let sequence = actual_sequence.clone();
    env.guard.add(test_object.on_did_change_container().listen(move |change| {
        for view in &change.views {
            sequence.borrow_mut().push_str(&format!("Moved {} from {} to {}\n", view.id, change.from.id, change.to.id));
        }
    }));

    let sequence = actual_sequence.clone();
    env.guard.add(test_object.on_did_change_location().listen(move |change| {
        for view in &change.views {
            let name = |location: ViewContainerLocation| if location == ViewContainerLocation::Sidebar { "Sidebar" } else { "Panel" };
            sequence.borrow_mut().push_str(&format!("Moved {} from {} to {}\n", view.id, name(change.from), name(change.to)));
        }
    }));

    env.suite.views_registry.register_views(view_descriptors[..2].to_vec(), &env.suite.sidebar_container);
    env.suite.views_registry.register_views(view_descriptors[2..].to_vec(), &env.suite.panel_container);

    let container_of = |view: &ViewDescriptorRc| test_object.get_view_container_by_view_id(&view.id).expect("getViewContainerByViewId(...)!");

    expected_sequence.push_str(&location_move_string(&view_descriptors[0], ViewContainerLocation::Sidebar, ViewContainerLocation::Panel));
    test_object.move_view_to_location(&view_descriptors[0], ViewContainerLocation::Panel, None);
    expected_sequence.push_str(&container_move_string(&view_descriptors[0], &env.suite.sidebar_container, &container_of(&view_descriptors[0])));

    expected_sequence.push_str(&location_move_string(&view_descriptors[2], ViewContainerLocation::Panel, ViewContainerLocation::Sidebar));
    test_object.move_view_to_location(&view_descriptors[2], ViewContainerLocation::Sidebar, None);
    expected_sequence.push_str(&container_move_string(&view_descriptors[2], &env.suite.panel_container, &container_of(&view_descriptors[2])));

    expected_sequence.push_str(&location_move_string(&view_descriptors[0], ViewContainerLocation::Panel, ViewContainerLocation::Sidebar));
    expected_sequence.push_str(&container_move_string(&view_descriptors[0], &container_of(&view_descriptors[0]), &env.suite.sidebar_container));
    test_object.move_views_to_container(&[view_descriptors[0].clone()], &env.suite.sidebar_container, None, None);

    expected_sequence.push_str(&location_move_string(&view_descriptors[2], ViewContainerLocation::Sidebar, ViewContainerLocation::Panel));
    expected_sequence.push_str(&container_move_string(&view_descriptors[2], &container_of(&view_descriptors[2]), &env.suite.panel_container));
    test_object.move_views_to_container(&[view_descriptors[2].clone()], &env.suite.panel_container, None, None);

    expected_sequence.push_str(&location_move_string(&view_descriptors[0], ViewContainerLocation::Sidebar, ViewContainerLocation::Panel));
    expected_sequence.push_str(&container_move_string(&view_descriptors[0], &env.suite.sidebar_container, &env.suite.panel_container));
    test_object.move_views_to_container(&[view_descriptors[0].clone()], &env.suite.panel_container, None, None);

    expected_sequence.push_str(&location_move_string(&view_descriptors[2], ViewContainerLocation::Panel, ViewContainerLocation::Sidebar));
    expected_sequence.push_str(&container_move_string(&view_descriptors[2], &env.suite.panel_container, &env.suite.sidebar_container));
    test_object.move_views_to_container(&[view_descriptors[2].clone()], &env.suite.sidebar_container, None, None);

    expected_sequence.push_str(&location_move_string(&view_descriptors[1], ViewContainerLocation::Sidebar, ViewContainerLocation::Panel));
    expected_sequence.push_str(&location_move_string(&view_descriptors[2], ViewContainerLocation::Sidebar, ViewContainerLocation::Panel));
    expected_sequence.push_str(&container_move_string(&view_descriptors[1], &env.suite.sidebar_container, &env.suite.panel_container));
    expected_sequence.push_str(&container_move_string(&view_descriptors[2], &env.suite.sidebar_container, &env.suite.panel_container));
    test_object.move_views_to_container(&[view_descriptors[1].clone(), view_descriptors[2].clone()], &env.suite.panel_container, None, None);

    assert_eq!(*actual_sequence.borrow(), expected_sequence, "Event sequence not matching expected sequence");
}

#[test]
fn reset() {
    let env = TestEnv::new();
    let test_object = env.view_descriptor_service();
    let view_descriptors = movable_views(3, true);

    env.suite.views_registry.register_views(view_descriptors[..2].to_vec(), &env.suite.sidebar_container);
    env.suite.views_registry.register_views(view_descriptors[2..].to_vec(), &env.suite.panel_container);

    test_object.move_view_to_location(&view_descriptors[0], ViewContainerLocation::Panel, None);
    test_object.move_views_to_container(&[view_descriptors[1].clone()], &env.suite.panel_container, None, None);
    test_object.move_view_to_location(&view_descriptors[2], ViewContainerLocation::Sidebar, None);

    let generated_panel = test_object.get_view_container_by_view_id(&view_descriptors[0].id).expect("assertReturnsDefined");
    let generated_sidebar = test_object.get_view_container_by_view_id(&view_descriptors[2].id).expect("assertReturnsDefined");

    test_object.reset();

    let sidebar_views = test_object.get_view_container_model(&env.suite.sidebar_container);
    assert_eq!(all_view_ids(&sidebar_views), ids(&["view1", "view2"]));
    let panel_views = test_object.get_view_container_model(&env.suite.panel_container);
    assert_eq!(all_view_ids(&panel_views), ids(&["view3"]));

    assert_eq!(env.stored_customizations(), json!({ "viewContainerLocations": {}, "viewLocations": {}, "viewContainerBadgeEnablementStates": {} }));

    assert!(test_object.get_view_container_by_id(&generated_panel.id).is_none());
    assert!(test_object.get_view_container_by_id(&generated_sidebar.id).is_none());
}

#[test]
fn initialize_with_custom_locations() {
    let env = TestEnv::new();
    let view_container1 = env.suite.register_container(ViewContainerLocation::Sidebar);
    let generate_view_container1 = generated_container_id(ViewContainerLocation::Sidebar);
    env.store_customizations(&json!({
        "viewContainerLocations": {
            generate_view_container1.clone(): ViewContainerLocation::Sidebar.value(),
            view_container1.id.clone(): ViewContainerLocation::AuxiliaryBar.value()
        },
        "viewLocations": {
            "view1": generate_view_container1.clone()
        }
    }));

    let view_descriptors = movable_views(4, false);

    env.suite.views_registry.register_views(view_descriptors[..3].to_vec(), &env.suite.sidebar_container);
    env.suite.views_registry.register_views(view_descriptors[3..].to_vec(), &view_container1);

    let test_object = env.view_descriptor_service();

    let sidebar_views = test_object.get_view_container_model(&env.suite.sidebar_container);
    assert_eq!(all_view_ids(&sidebar_views), ids(&["view2", "view3"]));

    let generated_view_container_views = test_object.get_view_container_model(&test_object.get_view_container_by_id(&generate_view_container1).unwrap());
    assert_eq!(all_view_ids(&generated_view_container_views), ids(&["view1"]));

    let view_container1_views = test_object.get_view_container_model(&view_container1);
    assert_eq!(test_object.get_view_container_location(&view_container1), Some(ViewContainerLocation::AuxiliaryBar));
    assert_eq!(all_view_ids(&view_container1_views), ids(&["view4"]));
}

#[test]
fn storage_change() {
    let env = TestEnv::new();
    let test_object = env.view_descriptor_service();

    let view_container1 = env.suite.register_container(ViewContainerLocation::Sidebar);
    let generate_view_container1 = generated_container_id(ViewContainerLocation::Sidebar);

    let view_descriptors = movable_views(4, false);

    env.suite.views_registry.register_views(view_descriptors[..3].to_vec(), &env.suite.sidebar_container);
    env.suite.views_registry.register_views(view_descriptors[3..].to_vec(), &view_container1);

    env.store_customizations(&json!({
        "viewContainerLocations": {
            generate_view_container1.clone(): ViewContainerLocation::Sidebar.value(),
            view_container1.id.clone(): ViewContainerLocation::AuxiliaryBar.value()
        },
        "viewLocations": {
            "view1": generate_view_container1.clone()
        }
    }));

    let sidebar_views = test_object.get_view_container_model(&env.suite.sidebar_container);
    assert_eq!(all_view_ids(&sidebar_views), ids(&["view2", "view3"]));

    let generated_view_container_views = test_object.get_view_container_model(&test_object.get_view_container_by_id(&generate_view_container1).unwrap());
    assert_eq!(all_view_ids(&generated_view_container_views), ids(&["view1"]));

    let view_container1_views = test_object.get_view_container_model(&view_container1);
    assert_eq!(test_object.get_view_container_location(&view_container1), Some(ViewContainerLocation::AuxiliaryBar));
    assert_eq!(all_view_ids(&view_container1_views), ids(&["view4"]));
}

#[test]
fn orphan_views() {
    let env = TestEnv::new();
    env.store_customizations(&json!({
        "viewContainerLocations": {},
        "viewLocations": {
            "view1": format!("{VIEW_CONTAINER_ID_PREFIX}-{}", generate_uuid())
        }
    }));

    let view_descriptors = movable_views(3, true);

    env.suite.views_registry.register_views(view_descriptors, &env.suite.sidebar_container);

    let test_object = env.view_descriptor_service();

    let sidebar_views = test_object.get_view_container_model(&env.suite.sidebar_container);
    assert_eq!(all_view_ids(&sidebar_views), ids(&["view2", "view3"]));

    test_object.when_extensions_registered();
    assert_eq!(all_view_ids(&sidebar_views), ids(&["view1", "view2", "view3"]));
}

#[test]
fn orphan_view_containers() {
    let env = TestEnv::new();
    let generated_view_container_id = generated_container_id(ViewContainerLocation::Sidebar);
    env.store_customizations(&json!({
        "viewContainerLocations": {
            generated_view_container_id.clone(): ViewContainerLocation::Sidebar.value()
        },
        "viewLocations": {}
    }));

    let view_descriptors = vec![movable_view("view1", "Test View 1", Some(1.0))];

    env.suite.views_registry.register_views(view_descriptors, &env.suite.sidebar_container);

    let test_object = env.view_descriptor_service();
    test_object.when_extensions_registered();

    assert!(test_object.get_view_container_by_id(&generated_view_container_id).is_none());
    assert!(test_object.is_view_container_removed_permanently(&generated_view_container_id));

    assert_eq!(env.stored_customizations(), json!({ "viewContainerLocations": {}, "viewLocations": {}, "viewContainerBadgeEnablementStates": {} }));
}

#[test]
fn custom_locations_take_precedence_when_default_view_container_of_views_change() {
    let env = TestEnv::new();
    let view_container1 = env.suite.register_container(ViewContainerLocation::Sidebar);
    let generate_view_container1 = generated_container_id(ViewContainerLocation::Sidebar);
    env.store_customizations(&json!({
        "viewContainerLocations": {
            generate_view_container1.clone(): ViewContainerLocation::Sidebar.value(),
            view_container1.id.clone(): ViewContainerLocation::AuxiliaryBar.value()
        },
        "viewLocations": {
            "view1": generate_view_container1.clone()
        }
    }));

    let view_descriptors = movable_views(4, false);

    env.suite.views_registry.register_views(view_descriptors[..3].to_vec(), &env.suite.sidebar_container);
    env.suite.views_registry.register_views(view_descriptors[3..].to_vec(), &view_container1);

    let test_object = env.view_descriptor_service();
    env.suite.views_registry.move_views(&[view_descriptors[0].clone(), view_descriptors[1].clone()], &env.suite.panel_container);

    let sidebar_views = test_object.get_view_container_model(&env.suite.sidebar_container);
    assert_eq!(all_view_ids(&sidebar_views), ids(&["view3"]));

    let panel_views = test_object.get_view_container_model(&env.suite.panel_container);
    assert_eq!(all_view_ids(&panel_views), ids(&["view2"]));

    let generated_view_container_views = test_object.get_view_container_model(&test_object.get_view_container_by_id(&generate_view_container1).unwrap());
    assert_eq!(all_view_ids(&generated_view_container_views), ids(&["view1"]));

    let view_container1_views = test_object.get_view_container_model(&view_container1);
    assert_eq!(test_object.get_view_container_location(&view_container1), Some(ViewContainerLocation::AuxiliaryBar));
    assert_eq!(all_view_ids(&view_container1_views), ids(&["view4"]));
}

#[test]
fn view_containers_with_not_existing_views_are_not_removed_from_customizations() {
    let env = TestEnv::new();
    let view_container1 = env.suite.register_container(ViewContainerLocation::Sidebar);
    let generate_view_container1 = generated_container_id(ViewContainerLocation::Sidebar);
    let views_customizations = json!({
        "viewContainerLocations": {
            generate_view_container1.clone(): ViewContainerLocation::Sidebar.value(),
            view_container1.id.clone(): ViewContainerLocation::AuxiliaryBar.value()
        },
        "viewLocations": {
            "view5": generate_view_container1.clone()
        }
    });
    env.store_customizations(&views_customizations);

    let view_descriptors = vec![movable_view("view1", "Test View 1", None)];

    env.suite.views_registry.register_views(view_descriptors, &view_container1);

    let test_object = env.view_descriptor_service();
    test_object.when_extensions_registered();

    let view_container1_views = test_object.get_view_container_model(&view_container1);
    assert_eq!(test_object.get_view_container_location(&view_container1), Some(ViewContainerLocation::AuxiliaryBar));
    assert_eq!(all_view_ids(&view_container1_views), ids(&["view1"]));

    assert_eq!(env.stored_customizations(), views_customizations);
}

#[test]
fn storage_change_also_updates_locations_even_if_views_do_not_exists_and_views_are_registered_later() {
    let env = TestEnv::new();
    let test_object = env.view_descriptor_service();

    let generate_view_container_id = generated_container_id(ViewContainerLocation::AuxiliaryBar);
    env.store_customizations(&json!({
        "viewContainerLocations": {
            generate_view_container_id.clone(): ViewContainerLocation::AuxiliaryBar.value(),
        },
        "viewLocations": {
            "view1": generate_view_container_id.clone()
        }
    }));

    let view_container = env.suite.register_container(ViewContainerLocation::Sidebar);
    let view_descriptors = movable_views(2, false);
    env.suite.views_registry.register_views(view_descriptors, &view_container);

    test_object.when_extensions_registered();

    let view_container1_views = test_object.get_view_container_model(&view_container);
    assert_eq!(all_view_ids(&view_container1_views), ids(&["view2"]));

    let generate_view_container = test_object.get_view_container_by_id(&generate_view_container_id).unwrap();
    assert_eq!(test_object.get_view_container_location(&generate_view_container), Some(ViewContainerLocation::AuxiliaryBar));
    let generated_view_container_model = test_object.get_view_container_model(&generate_view_container);
    assert_eq!(all_view_ids(&generated_view_container_model), ids(&["view1"]));
}

#[test]
fn storage_change_move_views_and_retain_visibility_state() {
    let env = TestEnv::new();
    let test_object = env.view_descriptor_service();

    let view_container = env.suite.register_container(ViewContainerLocation::Sidebar);
    let view_descriptors = vec![
        Rc::new(IViewDescriptor { can_toggle_visibility: true, ..(*movable_view("view1", "Test View 1", None)).clone() }),
        movable_view("view2", "Test View 2", None),
    ];
    env.suite.views_registry.register_views(view_descriptors, &view_container);

    test_object.when_extensions_registered();

    let view_container1_views = test_object.get_view_container_model(&view_container);
    view_container1_views.set_visible("view1", false);

    let generate_view_container_id = generated_container_id(ViewContainerLocation::AuxiliaryBar);
    env.store_customizations(&json!({
        "viewContainerLocations": {
            generate_view_container_id.clone(): ViewContainerLocation::AuxiliaryBar.value(),
        },
        "viewLocations": {
            "view1": generate_view_container_id.clone()
        }
    }));

    let generate_view_container = test_object.get_view_container_by_id(&generate_view_container_id).unwrap();
    let generated_view_container_model = test_object.get_view_container_model(&generate_view_container);

    assert_eq!(all_view_ids(&view_container1_views), ids(&["view2"]));
    assert_eq!(test_object.get_view_container_location(&generate_view_container), Some(ViewContainerLocation::AuxiliaryBar));
    assert_eq!(all_view_ids(&generated_view_container_model), ids(&["view1"]));

    env.store_customizations(&json!({}));

    let mut sorted_ids = all_view_ids(&view_container1_views);
    sorted_ids.sort();
    assert_eq!(sorted_ids, ids(&["view1", "view2"]));
    let visible_ids: Vec<String> = view_container1_views.visible_view_descriptors().iter().map(|view| view.id.clone()).collect();
    assert_eq!(visible_ids, ids(&["view2"]));
    assert_eq!(all_view_ids(&generated_view_container_model), Vec::<String>::new());
}
