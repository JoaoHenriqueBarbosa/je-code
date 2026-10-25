//! Porte de `src/vs/platform/remoteTunnel/test/node/remoteTunnelService.test.ts`.
//!
//! Os 3 casos saem um a um e na mesma ordem do original, com os mesmos esperados literais.
//!
//! Divergências:
//! - O `IProductService` que o original passa ao construtor não existe no `RemoteTunnelService` do porte (o serviço nunca o
//!   lê), e o `INativeEnvironmentService` é o `RemoteTunnelEnvironment`, com os mesmos valores do original.
//! - O `sinon.spy(telemetryService, 'publicLog2')` é o `RecordingTelemetryService`; `Event.None` do
//!   `ISharedProcessLifecycleService` é `Event::none()`.
//! - `ensureNoDisposablesAreLeakedInTestSuite()` é o `LeakGuard` criado no começo de cada caso; os casos rodam em
//!   `run_with_faked_timers`, porque o `initialize` espera o `Delayer` de 100 ms.
//! - `assert.deepStrictEqual` sobre o objeto de resultado é a comparação de cada campo, na ordem do original.
#![cfg(not(target_arch = "wasm32"))]

mod common;
#[path = "common/remote_tunnel.rs"]
mod remote_tunnel_support;

use std::cell::RefCell;
use std::rc::Rc;

use common::{ensure_no_disposables_are_leaked_in_test_suite, run_with_faked_timers};
use je_base::dynamic::Dynamic;
use je_base::event::{Emitter, Event};
use je_base::lifecycle::IDisposable;
use je_base::promise::Promise;
use je_base::uri::URI;
use je_platform::configuration::test_configuration_service::TestConfigurationService;
use je_platform::log::{LogLevel, LoggerService, null_logger_service};
use je_platform::remote_tunnel::remote_tunnel::{ActiveTunnelMode, ConnectionInfo, IRemoteTunnelService, IRemoteTunnelSession, TunnelMode, TunnelStatus, tunnel_states};
use je_platform::remote_tunnel::remote_tunnel_environment::RemoteTunnelEnvironment;
use je_platform::remote_tunnel::remote_tunnel_service::{ISharedProcessLifecycleService, RemoteTunnelService};
use je_platform::remote_tunnel::tunnel_machine_status::{IConnectedTunnelMachineStatus, ITokenErrorTunnelMachineStatus, TunnelMachineStatus};
use je_platform::remote_tunnel::tunnel_process_coordinator::{
    ITunnelProcessCoordinator, ITunnelProcessMachineStatus, ITunnelProcessOutput, ITunnelProcessStatus, TunnelProcessConnectionState, TunnelProcessMode,
};
use je_platform::remote_tunnel::tunnel_service_headers::ITunnelServiceCorrelation;
use je_platform::resources::uri_file;
use je_platform::storage::storage::{InMemoryStorageService, StorageService};
use remote_tunnel_support::RecordingTelemetryService;

struct TestTunnelProcessCoordinator {
    on_did_change_status: Rc<Emitter<ITunnelProcessStatus>>,
    on_did_output: Rc<Emitter<ITunnelProcessOutput>>,
    on_did_machine_status: Rc<Emitter<ITunnelProcessMachineStatus>>,
    status: RefCell<ITunnelProcessStatus>,
}

impl TestTunnelProcessCoordinator {
    fn new() -> Rc<Self> {
        Rc::new(Self {
            on_did_change_status: Emitter::new(),
            on_did_output: Emitter::new(),
            on_did_machine_status: Emitter::new(),
            status: RefCell::new(ITunnelProcessStatus {
                mode: TunnelProcessMode::RemoteAccess,
                tunnel_name: Some("test_host".to_owned()),
                tunnel_id: None,
                connection_state: TunnelProcessConnectionState::Connecting,
                service_install_failed: false,
            }),
        })
    }

    /// Models the coordinator having no tunnel running, where `getStatus().tunnelName` is undefined.
    fn set_idle(&self) {
        *self.status.borrow_mut() = ITunnelProcessStatus {
            mode: TunnelProcessMode::None,
            tunnel_name: None,
            tunnel_id: None,
            connection_state: TunnelProcessConnectionState::Disconnected,
            service_install_failed: false,
        };
    }

    fn fire_machine_status(&self, mode: TunnelProcessMode, status: TunnelMachineStatus, cancel: Rc<dyn Fn()>, correlation: Option<ITunnelServiceCorrelation>) {
        self.on_did_machine_status.fire(ITunnelProcessMachineStatus { mode, status, cancel, correlation });
    }
}

impl ITunnelProcessCoordinator for TestTunnelProcessCoordinator {
    fn on_did_change_status(&self) -> Event<ITunnelProcessStatus> {
        self.on_did_change_status.event()
    }

    fn on_did_output(&self) -> Event<ITunnelProcessOutput> {
        self.on_did_output.event()
    }

    fn on_did_machine_status(&self) -> Event<ITunnelProcessMachineStatus> {
        self.on_did_machine_status.event()
    }

    fn get_status(&self) -> ITunnelProcessStatus {
        self.status.borrow().clone()
    }

    fn get_intended_tunnel_name(&self) -> String {
        "test_host".to_owned()
    }

    fn set_remote_access(&self, _mode: TunnelMode, _log_level: LogLevel) -> Promise<()> {
        Promise::resolved(())
    }

    fn restart(&self) -> Promise<()> {
        Promise::resolved(())
    }

    fn set_remote_access_status(&self, status: &TunnelStatus) {
        let connection_state = match status {
            TunnelStatus::Connected(_) => TunnelProcessConnectionState::Connected,
            TunnelStatus::Connecting(_) => TunnelProcessConnectionState::Connecting,
            TunnelStatus::Disconnected(_) | TunnelStatus::Uninitialized(_) => TunnelProcessConnectionState::Disconnected,
        };
        let updated = {
            let mut current = self.status.borrow_mut();
            if current.connection_state == connection_state {
                return;
            }
            current.connection_state = connection_state;
            current.clone()
        };
        self.on_did_change_status.fire(updated);
    }
}

impl IDisposable for TestTunnelProcessCoordinator {
    fn dispose(&self) {
        self.on_did_change_status.dispose();
        self.on_did_output.dispose();
        self.on_did_machine_status.dispose();
    }
}

struct TestEnvironment;

impl RemoteTunnelEnvironment for TestEnvironment {
    fn app_root(&self) -> String {
        "installation".to_owned()
    }

    fn is_built(&self) -> bool {
        true
    }

    fn user_data_path(&self) -> String {
        "custom-user-data".to_owned()
    }

    fn logs_home(&self) -> URI {
        uri_file("logs")
    }
}

/// `{ _serviceBrand: undefined, onWillShutdown: Event.None }`.
struct TestSharedProcessLifecycleService;

impl ISharedProcessLifecycleService for TestSharedProcessLifecycleService {
    fn on_will_shutdown(&self) -> Event<()> {
        Event::none()
    }
}

/// O que cada caso cria e descarta no `finally`.
struct Harness {
    coordinator: Rc<TestTunnelProcessCoordinator>,
    logger_service: Rc<je_platform::log::AbstractLoggerService>,
    storage_service: Rc<je_platform::storage::storage::AbstractStorageService>,
    service: Rc<RemoteTunnelService>,
}

impl Harness {
    fn new(telemetry: Rc<RecordingTelemetryService>) -> Self {
        let coordinator = TestTunnelProcessCoordinator::new();
        let logger_service = null_logger_service();
        let storage_service = InMemoryStorageService::new();
        let storage: Rc<dyn StorageService> = Rc::new(storage_service.clone());
        let logger: Rc<dyn LoggerService> = logger_service.clone();
        let service = RemoteTunnelService::new(
            telemetry,
            Rc::new(TestEnvironment),
            logger,
            Rc::new(TestSharedProcessLifecycleService),
            TestConfigurationService::new(None),
            storage,
            coordinator.clone(),
        );
        Self { coordinator, logger_service, storage_service, service }
    }

    fn dispose(&self) {
        self.service.dispose();
        self.coordinator.dispose();
        self.logger_service.dispose();
        self.storage_service.dispose();
    }
}

fn session(token: Option<&str>) -> IRemoteTunnelSession {
    IRemoteTunnelSession { provider_id: "github".to_owned(), session_id: "session".to_owned(), account_label: "account".to_owned(), token: token.map(str::to_owned) }
}

fn no_cancel() -> Rc<dyn Fn()> {
    Rc::new(|| {})
}

#[test]
fn handles_machine_status_events_through_coordinator_fan_out() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    run_with_faked_timers(async move {
        let telemetry = Rc::new(RecordingTelemetryService::default());
        let harness = Harness::new(telemetry.clone());
        let service = harness.service.clone();
        let mode = ActiveTunnelMode { as_service: false, session: session(None) };
        let token_failures: Rc<RefCell<Vec<Option<IRemoteTunnelSession>>>> = Rc::default();
        let token_failure_listener = service.on_did_token_failed().listen({
            let token_failures = token_failures.clone();
            move |session: &Option<IRemoteTunnelSession>| token_failures.borrow_mut().push(session.clone())
        });
        let status_changes: Rc<RefCell<Vec<TunnelStatus>>> = Rc::default();

        service.initialize(TunnelMode::Active(mode.clone())).await.unwrap();
        let status_change_listener = service.on_did_change_tunnel_status().listen({
            let status_changes = status_changes.clone();
            move |status: &TunnelStatus| status_changes.borrow_mut().push(status.clone())
        });
        let did_cancel = Rc::new(std::cell::Cell::new(false));
        harness.coordinator.fire_machine_status(
            TunnelProcessMode::RemoteAccess,
            TunnelMachineStatus::Connected(IConnectedTunnelMachineStatus {
                tunnel_name: "test_host".to_owned(),
                tunnel_id: Some("tunnel-id".to_owned()),
                is_attached: true,
                link: Some("https://vscode.dev/tunnel/test_host".to_owned()),
                domain: Some("vscode.dev".to_owned()),
            }),
            no_cancel(),
            None,
        );
        let linked_status = service.get_tunnel_status().await.unwrap();
        harness.coordinator.fire_machine_status(
            TunnelProcessMode::RemoteAccess,
            TunnelMachineStatus::Connected(IConnectedTunnelMachineStatus { tunnel_name: "test_host".to_owned(), tunnel_id: None, is_attached: false, link: None, domain: None }),
            no_cancel(),
            Some(ITunnelServiceCorrelation { session_id: "telemetry-session".to_owned(), operation_id: "operation-id".to_owned() }),
        );
        let no_link_status = service.get_tunnel_status().await.unwrap();
        harness.coordinator.fire_machine_status(
            TunnelProcessMode::RemoteAccess,
            TunnelMachineStatus::TokenError(ITokenErrorTunnelMachineStatus { message: "token expired".to_owned() }),
            {
                let did_cancel = did_cancel.clone();
                Rc::new(move || did_cancel.set(true))
            },
            None,
        );
        let disconnected_status = service.get_tunnel_status().await.unwrap();

        let calls = telemetry.calls();
        let status_changes_seen = status_changes.borrow().clone();
        let token_failures_seen = token_failures.borrow().clone();

        status_change_listener.dispose();
        token_failure_listener.dispose();
        harness.dispose();

        let connected_telemetry: Vec<Dynamic> = calls.iter().filter(|call| call.0 == "remoteTunnel.connected").filter_map(|call| call.1.clone()).collect();
        assert_eq!(connected_telemetry.len(), 2);
        assert!(connected_telemetry[0].deep_strict_equals(&Dynamic::object([("tunnelName", Dynamic::string("test_host")), ("isAttached", Dynamic::Bool(true))])));
        assert!(connected_telemetry[1].deep_strict_equals(&Dynamic::object([
            ("tunnelName", Dynamic::string("test_host")),
            ("isAttached", Dynamic::Bool(false)),
            ("tunnelSessionId", Dynamic::string("telemetry-session")),
            ("operationId", Dynamic::string("operation-id")),
        ])));
        let linked = TunnelStatus::Connected(tunnel_states::connected(
            ConnectionInfo {
                link: Some("https://vscode.dev/tunnel/test_host".to_owned()),
                domain: Some("vscode.dev".to_owned()),
                tunnel_name: "test_host".to_owned(),
                tunnel_id: Some("tunnel-id".to_owned()),
                is_attached: true,
            },
            false,
        ));
        let no_link = TunnelStatus::Connected(tunnel_states::connected(
            ConnectionInfo { link: None, domain: None, tunnel_name: "test_host".to_owned(), tunnel_id: None, is_attached: false },
            false,
        ));
        let disconnected = TunnelStatus::Disconnected(tunnel_states::disconnected(Some(mode.session.clone())));
        assert_eq!(linked_status, linked);
        assert_eq!(no_link_status, no_link);
        assert_eq!(disconnected_status, disconnected);
        assert_eq!(status_changes_seen, vec![linked, no_link, disconnected]);
        assert_eq!(token_failures_seen, vec![Some(mode.session.clone())]);
        assert!(did_cancel.get());
        assert_eq!(calls.len(), 3);
    });
}

#[test]
fn reaches_connected_from_service_mode_machine_status() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    run_with_faked_timers(async move {
        let harness = Harness::new(Rc::default());
        let service = harness.service.clone();

        service.initialize(TunnelMode::Active(ActiveTunnelMode { as_service: true, session: session(None) })).await.unwrap();

        // The session process that attaches to the installed service reports
        // under `service`; dropping those events left the UI on "connecting".
        harness.coordinator.fire_machine_status(
            TunnelProcessMode::Service,
            TunnelMachineStatus::Connected(IConnectedTunnelMachineStatus { tunnel_name: "test_host".to_owned(), tunnel_id: None, is_attached: true, link: None, domain: None }),
            no_cancel(),
            None,
        );
        let status = service.get_tunnel_status().await.unwrap();

        harness.dispose();

        assert_eq!(
            status,
            TunnelStatus::Connected(tunnel_states::connected(
                ConnectionInfo { link: None, domain: None, tunnel_name: "test_host".to_owned(), tunnel_id: None, is_attached: true },
                false
            ))
        );
    });
}

#[test]
fn reports_the_intended_tunnel_name_while_access_is_inactive() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    run_with_faked_timers(async move {
        let harness = Harness::new(Rc::default());
        let service = harness.service.clone();

        harness.coordinator.set_idle();
        // The remote-extension recommendation compares the name this machine
        // would use against a previously used one, and asks while no tunnel
        // is running.
        let name = service.get_tunnel_name().await.unwrap();

        harness.dispose();

        assert_eq!(name, Some("test_host".to_owned()));
    });
}
