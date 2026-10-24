//! Porte de `src/vs/workbench/api/test/browser/mainThreadConfiguration.test.ts` (suíte `MainThreadConfiguration`), caso a
//! caso e na mesma ordem.
//!
//! Notas de tradução:
//! - `ensureNoDisposablesAreLeakedInTestSuite()` vira o guarda criado no começo de cada caso. O `MainThreadConfiguration`
//!   fica no guarda: no original o `onDidChangeConfiguration` é um `sinon.mock()` que devolve `undefined`, então não há
//!   assinatura a descartar; aqui o serviço de teste devolve a assinatura de verdade, e o guarda a descarta no fim.
//! - `suiteSetup` (registro das duas propriedades) é [`with_registered_configuration`], que registra antes do corpo do caso
//!   e remove depois (o registro é por thread e cada caso roda na sua).
//! - `instantiationService.stub(IConfigurationService, WorkspaceService)` mais o `updateValue` trocado pelo espião `target`
//!   é o `TestConfigurationService`, que registra as chamadas de `update_value`; `target.args[0][3]` é o `target` da
//!   primeira chamada ([`Setup::target`]). `stub(IEnvironmentService, { isBuilt: false })` é o
//!   `AbstractNativeEnvironmentService` com `VSCODE_DEV` definido (que é como o serviço nativo decide `isBuilt`).
//! - `stub(IWorkspaceContextService, { getWorkbenchState })` é o `TestContextService` com o estado forçado.
//! - `{ $initializeConfiguration: () => { } }` é [`InitializeOnlyProxy`]; o `$acceptConfigurationChanged`, ausente no objeto
//!   do original, falha se for chamado.
//! - `ConfigurationTarget.USER` é `ConfigurationTarget::User`; `null` é `None` e `undefined` é `None`.

#[path = "../../je-base/tests/common/mod.rs"]
mod common;
mod support;

use std::rc::Rc;

use common::{LeakGuard, ensure_no_disposables_are_leaked_in_test_suite};
use je_base::dynamic::Dynamic;
use je_base::path::{FixedEnvironment, Platform};
use je_platform::configuration::registry::{ConfigurationNode, ConfigurationNodeData, ConfigurationScope, PropertySchema, configuration_registry};
use je_platform::configuration::test_configuration_service::TestConfigurationService;
use je_platform::configuration::{ConfigurationChange, ConfigurationOverrides, ConfigurationTarget};
use je_platform::environment::environment::EnvironmentService;
use je_platform::environment::environment_service::{AbstractNativeEnvironmentService, NativeEnvironmentPaths};
use je_platform::environment::native_parsed_args::NativeParsedArgs;
use je_platform::product::ProductService;
use je_platform::resources::uri_file;
use je_platform::workspace::WorkbenchState;
use je_workbench::api::browser::main_thread_configuration::MainThreadConfiguration;
use je_workbench::api::common::ext_host_protocol::{ExtHostConfigurationShape, IConfigurationInitData, MainThreadConfigurationShape};
use je_workbench::test::common::workbench_test_services::TestContextService;
use support::test_rpc_protocol::single_proxy_rpc_protocol;

/// `const proxy = { $initializeConfiguration: () => { } }`.
struct InitializeOnlyProxy;

impl ExtHostConfigurationShape for InitializeOnlyProxy {
    fn rpc_initialize_configuration(&self, _data: IConfigurationInitData) {}

    fn rpc_accept_configuration_changed(&self, _data: IConfigurationInitData, _change: ConfigurationChange) {
        panic!("proxy.$acceptConfigurationChanged is not a function")
    }
}

/// O `instantiationService` do `setup`, já com o `MainThreadConfiguration` criado (`createInstance`) para o estado dado.
struct Setup {
    test_object: Rc<MainThreadConfiguration>,
    configuration_service: Rc<TestConfigurationService>,
}

impl Setup {
    fn new(guard: &LeakGuard, workbench_state: WorkbenchState) -> Self {
        let configuration_service = TestConfigurationService::new(None);
        let context_service = TestContextService::new(None, None);
        context_service.stub_workbench_state(Some(workbench_state));
        let mut env = FixedEnvironment::new(Platform::Posix, "/");
        env.vars.insert("VSCODE_DEV".to_owned(), "1".to_owned());
        let paths = NativeEnvironmentPaths {
            user_data_dir: "/user-data".to_owned(),
            home_dir: "/home".to_owned(),
            tmp_dir: "/tmp".to_owned(),
            app_root: "/app".to_owned(),
        };
        let environment_service: Rc<dyn EnvironmentService> =
            Rc::new(AbstractNativeEnvironmentService::new(NativeParsedArgs::default(), paths, ProductService::default(), Rc::new(env)));
        let proxy: Rc<dyn ExtHostConfigurationShape> = Rc::new(InitializeOnlyProxy);
        let test_object = guard.add_rc(MainThreadConfiguration::new(
            single_proxy_rpc_protocol(proxy).as_ref(),
            context_service,
            configuration_service.clone(),
            environment_service,
        ));
        Self { test_object, configuration_service }
    }

    /// `target.args[0][3]`: o alvo da primeira chamada de `updateValue`.
    fn target(&self) -> Option<ConfigurationTarget> {
        self.configuration_service.update_value_calls()[0].target
    }

    /// `testObject.$updateConfigurationOption(target, key, 'value', overrides, undefined)`.
    fn update(&self, target: Option<ConfigurationTarget>, key: &str, overrides: Option<ConfigurationOverrides>) {
        let _ = self.test_object.rpc_update_configuration_option(target, key, Dynamic::string("value"), overrides, None);
    }

    /// `testObject.$removeConfigurationOption(target, key, overrides, undefined)`.
    fn remove(&self, target: Option<ConfigurationTarget>, key: &str, overrides: Option<ConfigurationOverrides>) {
        let _ = self.test_object.rpc_remove_configuration_option(target, key, overrides, None);
    }
}

/// `{ resource: URI.file('abc') }`.
fn resource_overrides() -> Option<ConfigurationOverrides> {
    Some(ConfigurationOverrides { resource: Some(uri_file("abc")), override_identifier: None })
}

fn property(scope: ConfigurationScope) -> PropertySchema {
    PropertySchema::from_entries(vec![
        ("description", Dynamic::string("extHostConfiguration.resource")),
        ("type", Dynamic::string("boolean")),
        ("default", Dynamic::Bool(true)),
        ("scope", Dynamic::Number(scope as i32 as f64)),
    ])
}

/// `suiteSetup`, o corpo do caso e o descarte do registro.
fn with_registered_configuration(body: impl FnOnce()) {
    let registry = configuration_registry();
    let node = ConfigurationNode::new(
        ConfigurationNodeData {
            id: Some("extHostConfiguration".to_owned()),
            title: Some("a".to_owned()),
            type_: Some(Dynamic::string("object")),
            ..ConfigurationNodeData::default()
        }
        .with_properties([
            ("extHostConfiguration.resource", property(ConfigurationScope::Resource)),
            ("extHostConfiguration.window", property(ConfigurationScope::Window)),
        ]),
    );
    registry.register_configuration(node.clone());
    body();
    registry.deregister_configurations(&[node]);
}

const RESOURCE: &str = "extHostConfiguration.resource";
const WINDOW: &str = "extHostConfiguration.window";

#[test]
fn update_resource_configuration_without_configuration_target_defaults_to_workspace_in_multi_root_workspace_when_no_resource_is_provided() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    with_registered_configuration(|| {
        let setup = Setup::new(&guard, WorkbenchState::Workspace);

        setup.update(None, RESOURCE, None);

        assert_eq!(Some(ConfigurationTarget::Workspace), setup.target());
    });
}

#[test]
fn update_resource_configuration_without_configuration_target_defaults_to_workspace_in_folder_workspace_when_resource_is_provider() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    with_registered_configuration(|| {
        let setup = Setup::new(&guard, WorkbenchState::Folder);

        setup.update(None, RESOURCE, resource_overrides());

        assert_eq!(Some(ConfigurationTarget::Workspace), setup.target());
    });
}

#[test]
fn update_resource_configuration_without_configuration_target_defaults_to_workspace_in_folder_workspace_when_no_resource_is_provider() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    with_registered_configuration(|| {
        let setup = Setup::new(&guard, WorkbenchState::Folder);

        setup.update(None, RESOURCE, None);

        assert_eq!(Some(ConfigurationTarget::Workspace), setup.target());
    });
}

#[test]
fn update_window_configuration_without_configuration_target_defaults_to_workspace_in_multi_root_workspace_when_no_resource_is_provided() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    with_registered_configuration(|| {
        let setup = Setup::new(&guard, WorkbenchState::Workspace);

        setup.update(None, WINDOW, None);

        assert_eq!(Some(ConfigurationTarget::Workspace), setup.target());
    });
}

#[test]
fn update_window_configuration_without_configuration_target_defaults_to_workspace_in_multi_root_workspace_when_resource_is_provided() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    with_registered_configuration(|| {
        let setup = Setup::new(&guard, WorkbenchState::Workspace);

        setup.update(None, WINDOW, resource_overrides());

        assert_eq!(Some(ConfigurationTarget::Workspace), setup.target());
    });
}

#[test]
fn update_window_configuration_without_configuration_target_defaults_to_workspace_in_folder_workspace_when_resource_is_provider() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    with_registered_configuration(|| {
        let setup = Setup::new(&guard, WorkbenchState::Folder);

        setup.update(None, WINDOW, resource_overrides());

        assert_eq!(Some(ConfigurationTarget::Workspace), setup.target());
    });
}

#[test]
fn update_window_configuration_without_configuration_target_defaults_to_workspace_in_folder_workspace_when_no_resource_is_provider() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    with_registered_configuration(|| {
        let setup = Setup::new(&guard, WorkbenchState::Folder);

        setup.update(None, WINDOW, None);

        assert_eq!(Some(ConfigurationTarget::Workspace), setup.target());
    });
}

#[test]
fn update_resource_configuration_without_configuration_target_defaults_to_folder() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    with_registered_configuration(|| {
        let setup = Setup::new(&guard, WorkbenchState::Workspace);

        setup.update(None, RESOURCE, resource_overrides());

        assert_eq!(Some(ConfigurationTarget::WorkspaceFolder), setup.target());
    });
}

#[test]
fn update_configuration_with_user_configuration_target() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    with_registered_configuration(|| {
        let setup = Setup::new(&guard, WorkbenchState::Folder);

        setup.update(Some(ConfigurationTarget::User), WINDOW, resource_overrides());

        assert_eq!(Some(ConfigurationTarget::User), setup.target());
    });
}

#[test]
fn update_configuration_with_workspace_configuration_target() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    with_registered_configuration(|| {
        let setup = Setup::new(&guard, WorkbenchState::Folder);

        setup.update(Some(ConfigurationTarget::Workspace), WINDOW, resource_overrides());

        assert_eq!(Some(ConfigurationTarget::Workspace), setup.target());
    });
}

#[test]
fn update_configuration_with_folder_configuration_target() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    with_registered_configuration(|| {
        let setup = Setup::new(&guard, WorkbenchState::Folder);

        setup.update(Some(ConfigurationTarget::WorkspaceFolder), WINDOW, resource_overrides());

        assert_eq!(Some(ConfigurationTarget::WorkspaceFolder), setup.target());
    });
}

#[test]
fn remove_resource_configuration_without_configuration_target_defaults_to_workspace_in_multi_root_workspace_when_no_resource_is_provided() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    with_registered_configuration(|| {
        let setup = Setup::new(&guard, WorkbenchState::Workspace);

        setup.remove(None, RESOURCE, None);

        assert_eq!(Some(ConfigurationTarget::Workspace), setup.target());
    });
}

#[test]
fn remove_resource_configuration_without_configuration_target_defaults_to_workspace_in_folder_workspace_when_resource_is_provider() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    with_registered_configuration(|| {
        let setup = Setup::new(&guard, WorkbenchState::Folder);

        setup.remove(None, RESOURCE, resource_overrides());

        assert_eq!(Some(ConfigurationTarget::Workspace), setup.target());
    });
}

#[test]
fn remove_resource_configuration_without_configuration_target_defaults_to_workspace_in_folder_workspace_when_no_resource_is_provider() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    with_registered_configuration(|| {
        let setup = Setup::new(&guard, WorkbenchState::Folder);

        setup.remove(None, RESOURCE, None);

        assert_eq!(Some(ConfigurationTarget::Workspace), setup.target());
    });
}

#[test]
fn remove_window_configuration_without_configuration_target_defaults_to_workspace_in_multi_root_workspace_when_no_resource_is_provided() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    with_registered_configuration(|| {
        let setup = Setup::new(&guard, WorkbenchState::Workspace);

        setup.remove(None, WINDOW, None);

        assert_eq!(Some(ConfigurationTarget::Workspace), setup.target());
    });
}

#[test]
fn remove_window_configuration_without_configuration_target_defaults_to_workspace_in_multi_root_workspace_when_resource_is_provided() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    with_registered_configuration(|| {
        let setup = Setup::new(&guard, WorkbenchState::Workspace);

        setup.remove(None, WINDOW, resource_overrides());

        assert_eq!(Some(ConfigurationTarget::Workspace), setup.target());
    });
}

#[test]
fn remove_window_configuration_without_configuration_target_defaults_to_workspace_in_folder_workspace_when_resource_is_provider() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    with_registered_configuration(|| {
        let setup = Setup::new(&guard, WorkbenchState::Folder);

        setup.remove(None, WINDOW, resource_overrides());

        assert_eq!(Some(ConfigurationTarget::Workspace), setup.target());
    });
}

#[test]
fn remove_window_configuration_without_configuration_target_defaults_to_workspace_in_folder_workspace_when_no_resource_is_provider() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    with_registered_configuration(|| {
        let setup = Setup::new(&guard, WorkbenchState::Folder);

        setup.remove(None, WINDOW, None);

        assert_eq!(Some(ConfigurationTarget::Workspace), setup.target());
    });
}

#[test]
fn remove_configuration_without_configuration_target_defaults_to_folder() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    with_registered_configuration(|| {
        let setup = Setup::new(&guard, WorkbenchState::Workspace);

        setup.remove(None, RESOURCE, resource_overrides());

        assert_eq!(Some(ConfigurationTarget::WorkspaceFolder), setup.target());
    });
}

