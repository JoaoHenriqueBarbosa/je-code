//! Porte de `src/vs/workbench/contrib/terminal/test/browser/terminalProcessManager.test.ts` (suíte `Workbench -
//! TerminalProcessManager`), caso a caso e na mesma ordem.
//!
//! Notas de tradução:
//! - **`ensureNoDisposablesAreLeakedInTestSuite()` e `setup(async () => ...)`.** O [`Fixture`] cria o guarda de vazamentos antes
//!   de tudo; o `setup` assíncrono é [`Fixture::new`], que roda dentro do corpo de cada caso (sob `run_with_faked_timers`: os
//!   temporizadores do gerenciador, de 500 ms e de 0 ms, são drenados no relógio virtual). O gerenciador e os serviços passam
//!   pelo store do guarda.
//! - **`workbenchInstantiationService(undefined, store)` e os `stub`.** São os 15 serviços que o gerenciador lê, de
//!   `support/terminal_process_manager_support.rs`, com o `EnvironmentVariableService` real e o `TestTerminalInstanceService`
//!   do original (com `ptyHostRestartEmitter`, `revivedAttachArgs`, `createdProcesses` e `refuseRevivedAttach`).
//! - **`configurationService.setUserConfiguration(...)` e `onDidChangeConfigurationEmitter.fire({ affectsConfiguration: () => true
//!   })`.** O evento é o real da chave `terminal.integrated` (`fire_configuration_change`), cuja `affectsConfiguration` para a
//!   seção do terminal é verdadeira (é o que o `TerminalConfigurationService` confere).
//! - **`listenerCount(emitter)` e `_onDidChangeCollections`.** É o `_size` do emissor, que o `EnvironmentVariableService` do porte
//!   expõe em `on_did_change_collections_size_for_testing`.
//! - **`createProcess(slc, cols, rows, reset)`.** É o `create_process_with_reset` (o `createProcess` do trait só tem os três
//!   primeiros argumentos); `relaunch` é o `relaunch_process`. `createProcess({}, ...)` recebe um `IShellLaunchConfig` novo
//!   (`launch_config`), compartilhado como o objeto do original.
//! - **`manager.os = os`.** É [`TerminalProcessManager::set_os`].
//! - **`strictEqual(p, undefined)`.** É `p.is_none()`; `manager.shouldPersist`, `persistentProcessId` e `shellProcessId` são os
//!   métodos do `ITerminalProcessInfo`, e `pid: undefined` é `None`.
//! - **`URI.from({ scheme: Schemas.vscodeRemote, path: 'test/cwd' })`.** Sem autoridade, o `remoteAuthority` é vazio, como no
//!   original (o gerenciador trata o terminal como local).
//! - **`captured!` do `fireRestartAndCaptureData`.** O evento do reinício do pty host dispara os dados de forma síncrona (o
//!   corpo `async` do original roda até o primeiro `await` na hora), e o `expect` do caso é o `!`.
//! - **`deepStrictEqual({ ... }, { ... })`.** São tuplas de `assert_eq!`.

#[path = "../../je-base/tests/common/mod.rs"]
mod common;
#[path = "configuration_event_support/mod.rs"]
mod configuration_event_support;
mod support;

use std::cell::RefCell;
use std::rc::Rc;

use common::{LeakGuard, ensure_no_disposables_are_leaked_in_test_suite, run_with_faked_timers};
use configuration_event_support::fire_configuration_change;
use je_base::dynamic::Dynamic;
use je_base::platform::OperatingSystem;
use je_base::uri::{URI, UriComponents};
use je_platform::terminal::terminal::{IAttachPersistentProcess, IShellLaunchConfig, ShellCwd, TitleEventSource};
use je_workbench::contrib::terminal::browser::terminal_process_manager::{TerminalProcessManager, TerminalProcessManagerArgs};
use je_workbench::contrib::terminal::common::environment_variable_service::EnvironmentVariableService;
use je_workbench::contrib::terminal::common::terminal::{ITerminalProcessInfo, ITerminalProcessManager, SharedShellLaunchConfig};
use support::terminal_process_manager_support::{ProcessManagerInstanceService, create_process_manager_services};

struct Fixture {
    manager: Rc<TerminalProcessManager>,
    terminal_instance_service: Rc<ProcessManagerInstanceService>,
    environment_variable_service: Rc<EnvironmentVariableService>,
    // Declarado por último: o `Drop` dele descarta o que o `Fixture` registrou e confere os vazamentos.
    store: LeakGuard,
}

impl Fixture {
    /// O `setup` da suíte.
    async fn new() -> Self {
        let store = ensure_no_disposables_are_leaked_in_test_suite();
        let services = create_process_manager_services(&store.store());
        let configuration_service = services.configuration_service.clone();
        configuration_service.set_user_configuration("editor", Dynamic::object([("fontFamily", Dynamic::string("foo"))]), None).await.unwrap();
        configuration_service
            .set_user_configuration(
                "terminal",
                Dynamic::object([(
                    "integrated",
                    Dynamic::object([
                        ("fontFamily", Dynamic::string("bar")),
                        ("enablePersistentSessions", Dynamic::Bool(true)),
                        ("shellIntegration", Dynamic::object([("enabled", Dynamic::Bool(false))])),
                    ]),
                )]),
                None,
            )
            .await
            .unwrap();
        fire_configuration_change(&configuration_service, &["terminal.integrated"]);

        let manager = store.add_rc(services.instantiation_service.create_instance::<TerminalProcessManager>(TerminalProcessManagerArgs {
            instance_id: 1,
            cwd: None,
            environment_variable_collections: None,
            shell_integration_nonce: None,
        }));
        Self { manager, terminal_instance_service: services.terminal_instance_service, environment_variable_service: services.environment_variable_service, store }
    }

    /// `listenerCount(environmentVariableService._onDidChangeCollections)`.
    fn listener_count(&self) -> usize {
        self.environment_variable_service.on_did_change_collections_size_for_testing()
    }
}

/// O `{}` ou o literal passado a `createProcess` e `relaunch`.
fn launch_config(config: IShellLaunchConfig) -> SharedShellLaunchConfig {
    Rc::new(RefCell::new(config))
}

/// `attachPersistentProcess: { id: 21, pid: 72265, findRevivedId: true, title: 'zsh', titleSource: TitleEventSource.Process, cwd: '',
/// shellIntegrationNonce: '' }`.
fn revived_launch_config() -> IShellLaunchConfig {
    IShellLaunchConfig {
        attach_persistent_process: Some(IAttachPersistentProcess {
            id: 21,
            find_revived_id: Some(true),
            pid: 72265,
            title: "zsh".to_owned(),
            title_source: TitleEventSource::Process,
            cwd: String::new(),
            icon: None,
            color: None,
            has_child_processes: None,
            fixed_dimensions: None,
            environment_variable_collections: None,
            reconnection_properties: None,
            type_: None,
            wait_on_exit: None,
            hide_from_user: None,
            is_feature_terminal: None,
            shell_integration_nonce: String::new(),
            tab_actions: None,
        }),
        ..IShellLaunchConfig::default()
    }
}

/// `URI.from({ scheme: Schemas.vscodeRemote, path: 'test/cwd' })`.
fn remote_cwd() -> ShellCwd {
    ShellCwd::Uri(URI::from(&UriComponents { scheme: Some("vscode-remote".into()), path: Some("test/cwd".into()), ..UriComponents::default() }, false))
}

#[test]
fn does_not_accumulate_environment_variable_collection_listeners_when_relaunching() {
    run_with_faked_timers(async move {
        let fixture = Fixture::new().await;
        let initial_listener_count = fixture.listener_count();

        fixture.manager.create_process_with_reset(launch_config(IShellLaunchConfig::default()), 80, 24, false).await.unwrap();
        assert_eq!(fixture.listener_count(), initial_listener_count + 1);

        for _ in 0..3 {
            fixture.manager.relaunch_process(launch_config(IShellLaunchConfig::default()), 80, 24, false).await.unwrap();
            assert_eq!(fixture.listener_count(), initial_listener_count + 1);
        }
    });
}

#[test]
fn relaunch_does_not_report_the_previous_shell_pid_for_the_new_process_before_it_is_ready() {
    run_with_faked_timers(async move {
        let fixture = Fixture::new().await;
        fixture.manager.create_process_with_reset(launch_config(IShellLaunchConfig::default()), 80, 24, false).await.unwrap();
        fixture.terminal_instance_service.created_process(0).fire_ready(1001);
        let before_relaunch = (fixture.manager.persistent_process_id(), fixture.manager.shell_process_id());

        fixture.manager.relaunch_process(launch_config(IShellLaunchConfig::default()), 80, 24, false).await.unwrap();
        let while_starting = (fixture.manager.persistent_process_id(), fixture.manager.shell_process_id());

        fixture.terminal_instance_service.created_process(1).fire_ready(2002);
        let after_ready = (fixture.manager.persistent_process_id(), fixture.manager.shell_process_id());

        assert_eq!(
            (before_relaunch, while_starting, after_ready),
            ((Some(1), Some(1001)), (Some(2), None), (Some(2), Some(2002)))
        );
    });
}

// ---------------------------------------------------------------------------------------------------------------------
// suite('process persistence') > suite('local')
// ---------------------------------------------------------------------------------------------------------------------

#[test]
fn process_persistence_local_regular_terminal_should_persist() {
    run_with_faked_timers(async move {
        let fixture = Fixture::new().await;
        let p = fixture.manager.create_process_with_reset(launch_config(IShellLaunchConfig::default()), 1, 1, false).await.unwrap();
        assert!(p.is_none());
        assert!(fixture.manager.should_persist());
    });
}

#[test]
fn process_persistence_local_task_terminal_should_not_persist() {
    run_with_faked_timers(async move {
        let fixture = Fixture::new().await;
        let p = fixture
            .manager
            .create_process_with_reset(launch_config(IShellLaunchConfig { is_feature_terminal: Some(true), ..IShellLaunchConfig::default() }), 1, 1, false)
            .await
            .unwrap();
        assert!(p.is_none());
        assert!(!fixture.manager.should_persist());
    });
}

#[test]
fn process_persistence_local_restored_terminal_reattaches_with_the_pid_it_was_saved_with() {
    run_with_faked_timers(async move {
        let fixture = Fixture::new().await;
        fixture.manager.create_process_with_reset(launch_config(revived_launch_config()), 1, 1, false).await.unwrap();
        assert_eq!(fixture.terminal_instance_service.revived_attach_args(), vec![(21, 72265)]);
    });
}

#[test]
fn process_persistence_local_restored_terminal_starts_a_new_process_when_reattaching_is_refused() {
    run_with_faked_timers(async move {
        let fixture = Fixture::new().await;
        fixture.terminal_instance_service.set_refuse_revived_attach(true);
        fixture.manager.create_process_with_reset(launch_config(revived_launch_config()), 1, 1, false).await.unwrap();
        assert_eq!(
            (fixture.terminal_instance_service.revived_attach_args(), fixture.manager.persistent_process_id()),
            (vec![(21, 72265)], Some(1))
        );
    });
}

// ---------------------------------------------------------------------------------------------------------------------
// suite('process persistence') > suite('remote')
// ---------------------------------------------------------------------------------------------------------------------

#[test]
fn process_persistence_remote_regular_terminal_should_persist() {
    run_with_faked_timers(async move {
        let fixture = Fixture::new().await;
        let p = fixture
            .manager
            .create_process_with_reset(launch_config(IShellLaunchConfig { cwd: Some(remote_cwd()), ..IShellLaunchConfig::default() }), 1, 1, false)
            .await
            .unwrap();
        assert!(p.is_none());
        assert!(fixture.manager.should_persist());
    });
}

#[test]
fn process_persistence_remote_task_terminal_should_not_persist() {
    run_with_faked_timers(async move {
        let fixture = Fixture::new().await;
        let p = fixture
            .manager
            .create_process_with_reset(
                launch_config(IShellLaunchConfig { is_feature_terminal: Some(true), cwd: Some(remote_cwd()), ..IShellLaunchConfig::default() }),
                1,
                1,
                false,
            )
            .await
            .unwrap();
        assert!(p.is_none());
        assert!(!fixture.manager.should_persist());
    });
}

// ---------------------------------------------------------------------------------------------------------------------
// suite('pty host restart')
// ---------------------------------------------------------------------------------------------------------------------

/// `fireRestartAndCaptureData(os, rows)`.
async fn fire_restart_and_capture_data(fixture: &Fixture, os: OperatingSystem, rows: i32) -> String {
    fixture.manager.create_process_with_reset(launch_config(IShellLaunchConfig::default()), 80, rows, false).await.unwrap();
    fixture.manager.set_os(Some(os));
    let captured: Rc<RefCell<Option<String>>> = Rc::new(RefCell::new(None));
    let sink = captured.clone();
    fixture.store.add(fixture.manager.on_process_data().listen(move |event| *sink.borrow_mut() = Some(event.data.borrow().clone())));
    fixture.terminal_instance_service.fire_pty_host_restart();
    let data = captured.borrow().clone();
    data.expect("captured!")
}

#[test]
fn pty_host_restart_appends_viewport_clearing_newlines_and_esc_h_on_windows() {
    run_with_faked_timers(async move {
        let fixture = Fixture::new().await;
        let data = fire_restart_and_capture_data(&fixture, OperatingSystem::Windows, 24).await;
        assert!(data.ends_with(&format!("{}\x1b[H", "\r\n".repeat(23))));
    });
}

#[test]
fn pty_host_restart_does_not_append_viewport_clearing_sequence_on_non_windows() {
    run_with_faked_timers(async move {
        let fixture = Fixture::new().await;
        let data = fire_restart_and_capture_data(&fixture, OperatingSystem::Linux, 24).await;
        assert!(!data.contains("\x1b[H"));
    });
}

#[test]
fn pty_host_restart_does_not_append_viewport_clearing_sequence_on_windows_when_rows_is_0() {
    run_with_faked_timers(async move {
        let fixture = Fixture::new().await;
        let data = fire_restart_and_capture_data(&fixture, OperatingSystem::Windows, 0).await;
        assert!(!data.contains("\x1b[H"));
    });
}
