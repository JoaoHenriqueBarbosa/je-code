//! Porte de `src/vs/workbench/contrib/debug/test/browser/debugConfigurationManager.test.ts` (suíte `debugConfigurationManager`), caso a caso e na mesma ordem.
//!
//! Notas de tradução:
//! - **`setup`.** `workbenchInstantiationService(undefined, disposables)` e `instantiationService.stub(...)` viram os serviços
//!   de teste do workbench passados ao [`ConfigurationManagerServices`]: `TestContextService`, `TestConfigurationService`,
//!   `TestRemoteAgentService` (ou o `LinuxRemoteAgentService` do terceiro caso), `TestStorageService`, `TestExtensionService`,
//!   o `MockContextKeyService`, o serviço de identidade de URI de teste, e dublês que nada usam para o histórico (sem raiz
//!   ativa), o quick input, os arquivos e as preferências (`userSettingsResource` é `/tmp/settings.json`).
//! - `adapterManager` (`<IAdapterManager>{ getDebugAdapterDescriptor, activateDebuggers, onDidDebuggersExtPointRead }`) é o
//!   [`StubAdapterManager`].
//! - `ensureNoDisposablesAreLeakedInTestSuite()` vira o `LeakGuard` de cada caso (o `setup` e o `teardown` do original
//!   rodam em volta); o gerenciador é registrado no guarda, e o `disposables.delete(...)` + recriação do terceiro e quarto
//!   casos é descartar o primeiro gerenciador e criar o segundo.
//! - `Event.toPromise(onDidSelectConfiguration)` (só quando o SO local não é Linux) é o `to_promise` do evento.
//! - `{ ...config, configurationResolved: true }` é o objeto JSON com a chave a mais.

#[path = "../../je-base/tests/common/mod.rs"]
mod common;
mod support;

use std::rc::Rc;

use common::{LeakGuard, ensure_no_disposables_are_leaked_in_test_suite, run_with_faked_timers};
use je_base::cancellation::CancellationToken;
use je_base::event::Event;
use je_base::lifecycle::IDisposable;
use je_base::platform::{OperatingSystem, Platform};
use je_base::promise::Promise;
use je_base::uri::URI;
use je_platform::configuration::service::IConfigurationService;
use je_platform::configuration::test_configuration_service::TestConfigurationService;
use je_platform::keybinding::mock_keybinding_service::MockContextKeyService;
use je_platform::log::NullLogService;
use je_platform::remote::remote_agent_environment::{RemoteAgentEnvironment, RemoteAgentProfiles};
use je_platform::resources::uri_file;
use je_platform::workspace::IWorkspaceContextService;
use je_workbench::common::editor::{EditorInputOrUntyped, GroupIdentifier};
use je_workbench::contrib::debug::configuration_manager::{
    ConfigurationManager, ConfigurationManagerServices, GuessedDebugger, IDynamicQuickInput, IDynamicQuickPick, ILaunchIo, IManagerAdapterManager,
    value_to_dynamic,
};
use je_workbench::contrib::debug::debug::{DebugConfigurationProviderTriggerKind, IConfig, IDebugConfigurationProvider};
use je_workbench::contrib::debug::debugger::Debugger;
use je_workbench::services::history::history::{GoFilter, IHistoryService};
use je_workbench::services::remote::remote_agent_service::{IRemoteAgentConnection, IRemoteAgentService};
use je_workbench::test::browser::workbench_test_services::TestRemoteAgentService;
use je_workbench::test::common::workbench_test_services::{TestContextService, TestStorageService};
use je_workbench::test::browser::workbench_test_services::TestExtensionService;
use serde_json::{Value, json};
use support::mock_debug::{mock_uri_identity_service, storage_service_of};

const CONFIGURATION_PROVIDER_TYPE: &str = "custom-type";

/// O `adapterManager` do teste.
struct StubAdapterManager;

impl IManagerAdapterManager for StubAdapterManager {
    fn activate_debuggers(&self, _activation_event: &str, _debug_type: Option<&str>) -> Promise<()> {
        Promise::resolved(())
    }

    fn get_debugger_label(&self, _debug_type: &str) -> Option<String> {
        None
    }

    fn get_enabled_debugger(&self, _debug_type: &str) -> Option<Rc<Debugger>> {
        None
    }

    fn guess_debugger(&self, _getting_configurations: bool) -> Promise<Option<GuessedDebugger>> {
        Promise::resolved(None)
    }

    fn on_did_debuggers_ext_point_read(&self) -> Event<()> {
        Event::none()
    }
}

/// O `IHistoryService` do `workbenchInstantiationService`: sem raiz ativa.
struct NoHistoryService;

impl IHistoryService for NoHistoryService {
    fn go_forward(&self, _filter: Option<GoFilter>) -> Promise<()> {
        panic!("goForward não é usado")
    }

    fn go_back(&self, _filter: Option<GoFilter>) -> Promise<()> {
        panic!("goBack não é usado")
    }

    fn go_previous(&self, _filter: Option<GoFilter>) -> Promise<()> {
        panic!("goPrevious não é usado")
    }

    fn go_last(&self, _filter: Option<GoFilter>) -> Promise<()> {
        panic!("goLast não é usado")
    }

    fn reopen_last_closed_editor(&self) -> Promise<()> {
        panic!("reopenLastClosedEditor não é usado")
    }

    fn get_history(&self) -> Vec<EditorInputOrUntyped> {
        Vec::new()
    }

    fn remove_from_history(&self, _input: &EditorInputOrUntyped) {}

    fn get_last_active_workspace_root(&self, _scheme_filter: Option<&str>, _authority_filter: Option<&str>) -> Option<URI> {
        None
    }

    fn get_last_active_file(&self, _scheme_filter: &str, _authority_filter: Option<&str>) -> Option<URI> {
        None
    }

    fn open_next_recently_used_editor(&self, _group: Option<GroupIdentifier>) -> Promise<()> {
        panic!("openNextRecentlyUsedEditor não é usado")
    }

    fn open_previously_used_editor(&self, _group: Option<GroupIdentifier>) -> Promise<()> {
        panic!("openPreviouslyUsedEditor não é usado")
    }

    fn clear(&self) {}

    fn clear_recently_opened(&self) {}
}

/// Os arquivos, o editor e as preferências: nenhum caso os chama (o `preferencesService` do original só tem o `userSettingsResource`).
struct TestLaunchIo;

impl ILaunchIo for TestLaunchIo {
    fn read_file(&self, _resource: &URI) -> Promise<String> {
        panic!("readFile não é usado")
    }

    fn write_text(&self, _resource: &URI, _content: &str) -> Promise<()> {
        panic!("write não é usado")
    }

    fn open_editor(&self, _resource: &URI, _selection: Option<(i64, i64)>, _preserve_focus: bool, _pinned: bool) -> Promise<bool> {
        panic!("openEditor não é usado")
    }

    fn user_settings_resource(&self) -> URI {
        uri_file("/tmp/settings.json")
    }

    fn open_user_settings(&self, _preserve_focus: bool) -> Promise<bool> {
        panic!("openUserSettings não é usado")
    }
}

struct NoQuickInput;

impl IDynamicQuickInput for NoQuickInput {
    fn create_quick_pick(&self) -> Rc<dyn IDynamicQuickPick> {
        panic!("createQuickPick não é usado")
    }
}

/// O `LinuxRemoteAgentService` do terceiro caso.
struct LinuxRemoteAgentService;

impl IRemoteAgentService for LinuxRemoteAgentService {
    fn get_environment(&self) -> Promise<Option<RemoteAgentEnvironment>> {
        let dir = |path: &str| uri_file(path);
        Promise::resolved(Some(RemoteAgentEnvironment {
            pid: 1,
            connection_token: "token".to_owned(),
            app_root: dir("/remote/app"),
            exec_path: "/remote/app/node".to_owned(),
            tmp_dir: dir("/remote/tmp"),
            settings_path: dir("/remote/settings.json"),
            mcp_resource: dir("/remote/mcp.json"),
            logs_path: dir("/remote/logs"),
            extension_host_logs_path: dir("/remote/ext-logs"),
            global_storage_home: dir("/remote/global-storage"),
            workspace_storage_home: dir("/remote/workspace-storage"),
            local_history_home: dir("/remote/local-history"),
            user_home: dir("/remote/home"),
            os: OperatingSystem::Linux,
            arch: "x64".to_owned(),
            marks: Vec::new(),
            use_host_proxy: false,
            profiles: RemoteAgentProfiles { all: Vec::new(), home: dir("/remote/profiles") },
            is_unsupported_glibc: false,
            reconnection_grace_time: None,
        }))
    }

    fn get_raw_environment(&self) -> Promise<Option<RemoteAgentEnvironment>> {
        self.get_environment()
    }

    fn get_connection(&self) -> Option<Rc<dyn IRemoteAgentConnection>> {
        None
    }
}

fn create_configuration_manager(
    guard: &LeakGuard,
    context_service: Rc<TestContextService>,
    configuration_service: Rc<TestConfigurationService>,
    remote_agent_service: Rc<dyn IRemoteAgentService>,
) -> Rc<ConfigurationManager> {
    let storage = guard.add_rc(TestStorageService::new());
    let services = ConfigurationManagerServices {
        context_service: context_service as Rc<dyn IWorkspaceContextService>,
        configuration_service: configuration_service as Rc<dyn IConfigurationService>,
        quick_input: Rc::new(NoQuickInput),
        storage_service: storage_service_of(&storage),
        extension_service: TestExtensionService::new(),
        history_service: Rc::new(NoHistoryService),
        uri_identity_service: mock_uri_identity_service(),
        remote_agent_service,
        context_key_service: MockContextKeyService::new(),
        log_service: NullLogService::shared(),
        launch_io: Rc::new(TestLaunchIo),
    };
    guard.add_rc(ConfigurationManager::new(Rc::new(StubAdapterManager), services))
}

fn provider(
    debug_type: &str,
    resolve: impl Fn(Option<URI>, IConfig, CancellationToken) -> Promise<Option<IConfig>> + 'static,
) -> Rc<IDebugConfigurationProvider> {
    Rc::new(IDebugConfigurationProvider {
        r#type: debug_type.to_owned(),
        trigger_kind: DebugConfigurationProviderTriggerKind::Initial,
        resolve_debug_configuration: Some(Rc::new(resolve)),
        resolve_debug_configuration_with_substituted_variables: None,
        provide_debug_configurations: None,
    })
}

fn with_key(config: &IConfig, key: &str, value: Value) -> IConfig {
    let mut object = config.as_object().cloned().unwrap();
    object.insert(key.to_owned(), value);
    Value::Object(object)
}

fn initial_config() -> IConfig {
    json!({ "type": CONFIGURATION_PROVIDER_TYPE, "request": "launch", "name": "configName" })
}

#[test]
fn resolves_configuration_based_on_type() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    run_with_faked_timers(async move {
        let configuration_service = TestConfigurationService::new(None);
        let manager = create_configuration_manager(&guard, TestContextService::new(None, None), configuration_service, Rc::new(TestRemoteAgentService));
        guard.add(manager.register_debug_configuration_provider(provider(CONFIGURATION_PROVIDER_TYPE, |_folder_uri, config, _token| {
            assert_eq!(config["type"], json!(CONFIGURATION_PROVIDER_TYPE));
            Promise::resolved(Some(with_key(&config, "configurationResolved", json!(true))))
        })));

        let result_config = manager
            .resolve_configuration_by_providers(None, Some(CONFIGURATION_PROVIDER_TYPE.to_owned()), initial_config(), CancellationToken::None)
            .await
            .unwrap();
        assert_eq!(result_config.unwrap()["configurationResolved"], json!(true), "Configuration should be updated by test provider");
    });
}

#[test]
fn resolves_configuration_from_second_provider_if_type_changes() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    run_with_faked_timers(async move {
        let second_provider_type = "second-provider";
        let manager = create_configuration_manager(&guard, TestContextService::new(None, None), TestConfigurationService::new(None), Rc::new(TestRemoteAgentService));
        guard.add(manager.register_debug_configuration_provider(provider(CONFIGURATION_PROVIDER_TYPE, move |_folder_uri, config, _token| {
            assert_eq!(config["type"], json!(CONFIGURATION_PROVIDER_TYPE));
            Promise::resolved(Some(with_key(&config, "type", json!(second_provider_type))))
        })));
        guard.add(manager.register_debug_configuration_provider(provider(second_provider_type, move |_folder_uri, config, _token| {
            assert_eq!(config["type"], json!(second_provider_type));
            Promise::resolved(Some(with_key(&config, "configurationResolved", json!(true))))
        })));

        let result_config = manager
            .resolve_configuration_by_providers(None, Some(CONFIGURATION_PROVIDER_TYPE.to_owned()), initial_config(), CancellationToken::None)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(result_config["type"], json!(second_provider_type));
        assert_eq!(result_config["configurationResolved"], json!(true), "Configuration should be updated by test provider");
    });
}

fn names(manager: &Rc<ConfigurationManager>) -> Vec<String> {
    manager.get_all_configurations().into_iter().map(|entry| entry.name).collect()
}

#[test]
fn uses_remote_target_os_when_computing_visible_configurations() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    run_with_faked_timers(async move {
        let configuration_service = TestConfigurationService::new(None);
        // `setUserConfiguration('launch', ...)`
        drop(configuration_service.set_user_configuration(
            "launch",
            value_to_dynamic(&json!({
                "version": "0.2.0",
                "configurations": [
                    { "type": "node", "request": "launch", "name": "visible", "presentation": { "hidden": false } },
                    { "type": "node", "request": "launch", "name": "linux-hidden", "linux": { "presentation": { "hidden": true } } }
                ]
            })),
            None,
        ));
        let manager = create_configuration_manager(&guard, TestContextService::new(None, None), configuration_service, Rc::new(LinuxRemoteAgentService));

        if Platform::detect().os != OperatingSystem::Linux {
            manager.on_did_select_configuration().to_promise(None).promise.await.unwrap();
        }

        assert_eq!(names(&manager), vec!["visible".to_owned()]);
    });
}

#[test]
fn ignores_null_entries_in_launch_configurations() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let configuration_service = TestConfigurationService::new(None);
    drop(configuration_service.set_user_configuration(
        "launch",
        value_to_dynamic(&json!({
            "version": "0.2.0",
            "configurations": [
                { "type": "node", "request": "launch", "name": "valid" },
                null
            ]
        })),
        None,
    ));

    let manager = create_configuration_manager(&guard, TestContextService::new(None, None), configuration_service, Rc::new(TestRemoteAgentService));

    assert_eq!(names(&manager), vec!["valid".to_owned()]);
}
