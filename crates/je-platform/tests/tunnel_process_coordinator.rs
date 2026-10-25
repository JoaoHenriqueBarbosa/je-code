//! Porte de `src/vs/platform/remoteTunnel/test/node/tunnelProcessCoordinator.test.ts`.
//!
//! Os 12 casos saem um a um e na mesma ordem do original, com os mesmos esperados literais.
//!
//! Divergências:
//! - O `spawn` falso do original devolve um `ChildProcess` de `EventEmitter` e `PassThrough`; aqui é o `TestChild`
//!   (`tests/common/remote_tunnel.rs`), que classifica o comando pelos argumentos (`complete`, `isTunnelProcess`) como o
//!   `createCoordinator` do original. O `sinon.spy(telemetryService, 'publicLog2')` é o `RecordingTelemetryService`.
//! - `ensureNoDisposablesAreLeakedInTestSuite()` é o `LeakGuard` criado no começo de cada caso. O `try/finally` do original
//!   vira: capturar os valores, fazer a limpeza e só então conferir (o `finally` não muda o que a conferência vê).
//! - `await new Promise(resolve => setImmediate(resolve))` é um temporizador de 0 ms no laço de eventos (o `setImmediate` do
//!   Node não existe aqui). Os casos rodam em `run_with_faked_timers`.
//! - `String(process.pid)` é `std::process::id()`.
#![cfg(not(target_arch = "wasm32"))]

mod common;
#[path = "common/remote_tunnel.rs"]
mod remote_tunnel_support;

use std::cell::RefCell;
use std::collections::{BTreeMap, BTreeSet};
use std::rc::Rc;

use common::{ensure_no_disposables_are_leaked_in_test_suite, run_with_faked_timers};
use je_base::dynamic::Dynamic;
use je_base::errors::Error;
use je_base::lifecycle::IDisposable;
use je_base::promise::Promise;
use je_base::runtime;
use je_platform::configuration::test_configuration_service::TestConfigurationService;
use je_platform::log::LogLevel;
use je_platform::remote_tunnel::code_tunnel_cli_process::{
    CodeTunnelCli, CodeTunnelCliOptions, CodeTunnelLog, CodeTunnelSpawn, SpawnOptions, TunnelChildHandlers, TunnelChildProcess,
};
use je_platform::remote_tunnel::remote_tunnel::{ActiveTunnelMode, IRemoteTunnelSession, TunnelMode, TunnelStatus, inactive_tunnel_mode, tunnel_states};
use je_platform::remote_tunnel::tunnel_process_coordinator::{
    ITunnelProcessCoordinator, ITunnelProcessMachineStatus, ITunnelProcessStatus, TunnelCliFactory, TunnelProcessConnectionState, TunnelProcessCoordinator, TunnelProcessMode,
    resolve_tunnel_process_mode,
};
use je_platform::remote_tunnel::tunnel_service_headers::ITunnelServiceCorrelation;
use je_platform::telemetry::telemetry::ITelemetryService;
use remote_tunnel_support::{RecordingTelemetryService, TestChild, test_native_environment, test_tunnel_product};

/// `TestChildProcess`: o filho falso e o que o `spawn` recebeu.
struct TestProcess {
    child: Rc<TestChild>,
    args: Vec<String>,
    env: BTreeMap<String, String>,
}

type Processes = Rc<RefCell<Vec<Rc<TestProcess>>>>;

fn active_mode(as_service: bool) -> TunnelMode {
    TunnelMode::Active(ActiveTunnelMode {
        as_service,
        session: IRemoteTunnelSession { provider_id: "github".to_owned(), session_id: "session".to_owned(), account_label: "account".to_owned(), token: Some("token".to_owned()) },
    })
}

fn inactive_mode() -> TunnelMode {
    TunnelMode::Inactive(inactive_tunnel_mode())
}

fn has(args: &[String], name: &str) -> bool {
    args.iter().any(|arg| arg == name)
}

/// `args[0] === 'tunnel' && !args.includes('status') && ...`
fn is_tunnel_process(args: &[String]) -> bool {
    args.first().is_some_and(|arg| arg == "tunnel")
        && !has(args, "status")
        && !has(args, "login")
        && !has(args, "install")
        && !has(args, "kill")
        && !has(args, "uninstall")
}

fn strings(items: &[&str]) -> Vec<String> {
    items.iter().map(|item| (*item).to_owned()).collect()
}

struct CoordinatorOptions {
    exit_on_kill: bool,
    ordering: Option<Rc<RefCell<Vec<String>>>>,
    install_exit_code: i32,
    telemetry: Rc<RecordingTelemetryService>,
}

impl Default for CoordinatorOptions {
    fn default() -> Self {
        Self { exit_on_kill: true, ordering: None, install_exit_code: 0, telemetry: Rc::default() }
    }
}

fn create_coordinator(options: CoordinatorOptions) -> (Rc<TunnelProcessCoordinator>, Processes) {
    let processes: Processes = Rc::default();
    let CoordinatorOptions { exit_on_kill, ordering, install_exit_code, telemetry } = options;
    let spawn: CodeTunnelSpawn = {
        let processes = processes.clone();
        Rc::new(move |_command: &str, args: &[String], spawn_options: &SpawnOptions, handlers: TunnelChildHandlers| -> Result<Rc<dyn TunnelChildProcess>, Error> {
            let complete = has(args, "login") || has(args, "status") || has(args, "install") || has(args, "kill") || has(args, "uninstall");
            let tunnel_process = is_tunnel_process(args);
            if tunnel_process && let Some(ordering) = &ordering {
                ordering.borrow_mut().push("spawn-remote-access".to_owned());
            }
            let child = TestChild::new(handlers, 123, exit_on_kill || complete);
            if complete {
                let status_output = has(args, "status").then_some("{\"service_installed\":false,\"tunnel\":null}\n");
                let exit_code = if has(args, "install") { install_exit_code } else { 0 };
                let child = child.clone();
                runtime::queue_microtask(move || {
                    if let Some(status_output) = status_output {
                        child.write_stdout(status_output);
                    }
                    child.emit_exit(Some(exit_code));
                });
            }
            if tunnel_process && let Some(ordering) = &ordering {
                child.on_exit({
                    let ordering = ordering.clone();
                    move || ordering.borrow_mut().push("exit-remote-access".to_owned())
                });
                child.on_kill({
                    let ordering = ordering.clone();
                    move || ordering.borrow_mut().push("kill-remote-access".to_owned())
                });
            }
            processes.borrow_mut().push(Rc::new(TestProcess { child: child.clone(), args: args.to_vec(), env: spawn_options.env.clone() }));
            Ok(child)
        })
    };
    let factory: TunnelCliFactory = Box::new(move |on_log: CodeTunnelLog| {
        CodeTunnelCli::new(CodeTunnelCliOptions {
            app_root: "installation".to_owned(),
            is_built: true,
            tunnel_application_name: Some("code-tunnel".to_owned()),
            win32_versioned_update: false,
            spawn: Some(spawn.clone()),
            on_log: Some(on_log),
        })
    });
    let configuration = TestConfigurationService::new(Some(Dynamic::object([("remote.tunnels.access.hostNameOverride", Dynamic::string("Test_Host"))])));
    let telemetry: Rc<dyn ITelemetryService> = telemetry;
    let coordinator = TunnelProcessCoordinator::new(
        Some(factory),
        configuration,
        test_native_environment(),
        &test_tunnel_product(),
        telemetry,
    );
    (coordinator, processes)
}

/// `setImmediate`.
fn set_immediate() -> Promise<()> {
    Promise::with_executor(|resolver| {
        runtime::set_timeout(move || resolver.resolve(()), 0);
    })
}

/// O `finally` dos casos: os processos que sobraram saem e o laço dá uma volta.
async fn exit_all(processes: &Processes) {
    let all: Vec<Rc<TestProcess>> = processes.borrow().clone();
    for process in all {
        process.child.emit_exit(None);
    }
    set_immediate().await.unwrap();
}

fn find_process(processes: &Processes, predicate: impl Fn(&TestProcess) -> bool) -> Rc<TestProcess> {
    processes.borrow().iter().find(|process| predicate(process)).expect("o processo esperado").clone()
}

fn count_tunnels(processes: &Processes) -> usize {
    processes.borrow().iter().filter(|process| is_tunnel_process(&process.args)).count()
}

#[test]
fn resolves_remote_tunnel_access_modes() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    assert_eq!(
        vec![resolve_tunnel_process_mode(&inactive_mode()), resolve_tunnel_process_mode(&active_mode(false)), resolve_tunnel_process_mode(&active_mode(true))],
        vec![TunnelProcessMode::None, TunnelProcessMode::RemoteAccess, TunnelProcessMode::Service]
    );
}

#[test]
fn passes_correlation_to_every_cli_invocation_but_only_logs_operations_for_host_launches() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    run_with_faked_timers(async move {
        let telemetry = Rc::new(RecordingTelemetryService::default());
        let (coordinator, processes) = create_coordinator(CoordinatorOptions { telemetry: telemetry.clone(), ..CoordinatorOptions::default() });
        let statuses: Rc<RefCell<Vec<Option<ITunnelServiceCorrelation>>>> = Rc::default();
        let listener = coordinator.on_did_machine_status().listen({
            let statuses = statuses.clone();
            move |status: &ITunnelProcessMachineStatus| statuses.borrow_mut().push(status.correlation.clone())
        });

        coordinator.set_remote_access(active_mode(true), LogLevel::Info).await.unwrap();
        let tunnel = find_process(&processes, |process| has(&process.args, "--parent-process-id"));
        tunnel.child.write_stdout("__VSCODE_CLI_STATUS__{\"type\":\"connected\",\"tunnelName\":\"test_host\",\"isAttached\":false}\n");
        tunnel.child.write_stdout("__VSCODE_CLI_STATUS__{\"type\":\"connected\",\"tunnelName\":\"test_host\",\"isAttached\":true}\n");
        coordinator.set_remote_access(inactive_mode(), LogLevel::Info).await.unwrap();

        let env_of = |process: &Rc<TestProcess>, key: &str| process.env.get(key).cloned();
        let all: Vec<Rc<TestProcess>> = processes.borrow().clone();
        let session_ids: BTreeSet<Option<String>> = all.iter().map(|process| env_of(process, "VSCODE_TUNNEL_SESSION_ID")).collect();
        let unique_operations = all.iter().map(|process| env_of(process, "VSCODE_TUNNEL_OPERATION_ID")).collect::<BTreeSet<_>>().len();
        let has_operations = all.iter().all(|process| env_of(process, "VSCODE_TUNNEL_OPERATION_ID").is_some_and(|operation| !operation.is_empty()));
        let correlation = statuses.borrow().clone();
        let tunnel_session_id = env_of(&tunnel, "VSCODE_TUNNEL_SESSION_ID").unwrap();
        let tunnel_operation_id = env_of(&tunnel, "VSCODE_TUNNEL_OPERATION_ID").unwrap();
        let calls = telemetry.calls();

        listener.dispose();
        exit_all(&processes).await;
        coordinator.dispose();

        assert_eq!(session_ids, BTreeSet::from([Some(RecordingTelemetryService::SESSION_ID.to_owned())]));
        assert_eq!(unique_operations, all.len());
        assert!(has_operations);
        assert_eq!(correlation, vec![Some(ITunnelServiceCorrelation { session_id: tunnel_session_id.clone(), operation_id: tunnel_operation_id.clone() }), None]);
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].0, "tunnelServiceOperation");
        let expected = Dynamic::object([
            ("tunnelSessionId", Dynamic::string(RecordingTelemetryService::SESSION_ID)),
            ("operationId", Dynamic::string(&tunnel_operation_id)),
            ("operation", Dynamic::string("host")),
        ]);
        assert!(calls[0].1.as_ref().is_some_and(|data| data.deep_strict_equals(&expected)), "dados do evento: {:?}", calls[0].1);
    });
}

#[test]
fn stops_the_tunnel_instead_of_resuming_a_narrower_mode_when_remote_tunnel_access_is_disabled() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    run_with_faked_timers(async move {
        let (coordinator, processes) = create_coordinator(CoordinatorOptions::default());
        coordinator.set_remote_access(active_mode(false), LogLevel::Info).await.unwrap();
        let tunnel = find_process(&processes, |process| has(&process.args, "--accept-server-license-terms"));
        coordinator.set_remote_access(inactive_mode(), LogLevel::Info).await.unwrap();

        let tunnel_was_stopped = tunnel.child.was_killed();
        let status = coordinator.get_status();
        let agent_host_process_started = processes.borrow().iter().any(|process| has(&process.args, "--agent-host-only"));

        exit_all(&processes).await;
        coordinator.dispose();

        assert!(tunnel_was_stopped);
        assert_eq!(
            status,
            ITunnelProcessStatus {
                mode: TunnelProcessMode::None,
                tunnel_name: None,
                tunnel_id: None,
                connection_state: TunnelProcessConnectionState::Disconnected,
                service_install_failed: false,
            }
        );
        assert!(!agent_host_process_started);
    });
}

#[test]
fn restarts_when_the_session_token_changes_even_though_mode_and_name_do_not() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    run_with_faked_timers(async move {
        let (coordinator, processes) = create_coordinator(CoordinatorOptions::default());
        coordinator.set_remote_access(active_mode(false), LogLevel::Info).await.unwrap();
        let before = count_tunnels(&processes);

        // A refreshed token has to reach a new process; skipping the
        // reconcile would leave the tunnel running on the stale one.
        let refreshed = TunnelMode::Active(ActiveTunnelMode {
            as_service: false,
            session: IRemoteTunnelSession { provider_id: "github".to_owned(), session_id: "session".to_owned(), account_label: "account".to_owned(), token: Some("refreshed-token".to_owned()) },
        });
        coordinator.set_remote_access(refreshed, LogLevel::Info).await.unwrap();
        let after = count_tunnels(&processes);

        exit_all(&processes).await;
        coordinator.dispose();

        assert_eq!((before, after), (1, 2));
    });
}

#[test]
fn restarts_a_run_that_already_reported_disconnected() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    run_with_faked_timers(async move {
        let (coordinator, processes) = create_coordinator(CoordinatorOptions::default());
        coordinator.set_remote_access(active_mode(false), LogLevel::Info).await.unwrap();
        let before = count_tunnels(&processes);

        // A token error cancels the child and reports disconnected before it
        // exits. Treating that run as healthy would skip the reconcile and
        // leave nothing running once the cancelled child goes away.
        coordinator.set_remote_access_status(&TunnelStatus::Disconnected(tunnel_states::disconnected(None)));
        coordinator.set_remote_access(active_mode(false), LogLevel::Info).await.unwrap();
        let after = count_tunnels(&processes);

        exit_all(&processes).await;
        coordinator.dispose();

        assert_eq!((before, after), (1, 2));
    });
}

#[test]
fn restart_still_replaces_a_healthy_tunnel() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    run_with_faked_timers(async move {
        let (coordinator, processes) = create_coordinator(CoordinatorOptions::default());
        coordinator.set_remote_access(active_mode(false), LogLevel::Info).await.unwrap();
        let tunnel = find_process(&processes, |process| has(&process.args, "--accept-server-license-terms"));
        coordinator.restart().await.unwrap();

        let was_killed = tunnel.child.was_killed();

        exit_all(&processes).await;
        coordinator.dispose();

        assert!(was_killed);
    });
}

#[test]
fn starts_a_session_tunnel_alongside_the_installed_service_so_readiness_can_advance() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    run_with_faked_timers(async move {
        let (coordinator, processes) = create_coordinator(CoordinatorOptions::default());
        coordinator.set_remote_access(active_mode(true), LogLevel::Info).await.unwrap();
        let session_tunnel = processes.borrow().iter().find(|process| has(&process.args, "--accept-server-license-terms") && !has(&process.args, "install")).cloned();

        // Without a session process nothing ever reports connected, so the
        // UI stays stuck on "connecting" after the service is installed.
        let installed = processes.borrow().iter().any(|process| has(&process.args, "install"));
        let started_session_tunnel = session_tunnel.is_some();
        let mode = coordinator.get_status().mode;

        exit_all(&processes).await;
        coordinator.dispose();

        assert_eq!((installed, started_session_tunnel, mode), (true, true, TunnelProcessMode::Service));
    });
}

#[test]
fn falls_back_to_hosting_in_session_when_the_service_install_fails() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    run_with_faked_timers(async move {
        let (coordinator, processes) = create_coordinator(CoordinatorOptions { install_exit_code: 1, ..CoordinatorOptions::default() });
        coordinator.set_remote_access(active_mode(true), LogLevel::Info).await.unwrap();

        let service_install_failed = coordinator.get_status().service_install_failed;
        let started_session_tunnel = processes.borrow().iter().any(|process| has(&process.args, "--accept-server-license-terms") && !has(&process.args, "install"));

        exit_all(&processes).await;
        coordinator.dispose();

        assert_eq!((service_install_failed, started_session_tunnel), (true, true));
    });
}

#[test]
fn waits_for_the_prior_tunnel_process_to_exit_before_spawning_its_replacement() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    run_with_faked_timers(async move {
        let ordering: Rc<RefCell<Vec<String>>> = Rc::default();
        let (coordinator, processes) = create_coordinator(CoordinatorOptions { exit_on_kill: false, ordering: Some(ordering.clone()), ..CoordinatorOptions::default() });
        coordinator.set_remote_access(active_mode(false), LogLevel::Info).await.unwrap();
        let tunnel = find_process(&processes, |process| has(&process.args, "--accept-server-license-terms"));
        let transition = coordinator.set_remote_access(
            TunnelMode::Active(ActiveTunnelMode {
                as_service: false,
                session: IRemoteTunnelSession { provider_id: "github".to_owned(), session_id: "session".to_owned(), account_label: "account".to_owned(), token: Some("refreshed-token".to_owned()) },
            }),
            LogLevel::Info,
        );
        set_immediate().await.unwrap();
        let before_exit = ordering.borrow().clone();

        tunnel.child.emit_exit(None);
        transition.await.unwrap();
        let after_exit = ordering.borrow().clone();

        exit_all(&processes).await;
        coordinator.dispose();

        assert_eq!(before_exit, strings(&["spawn-remote-access", "kill-remote-access"]));
        assert_eq!(after_exit, strings(&["spawn-remote-access", "kill-remote-access", "exit-remote-access", "spawn-remote-access"]));
    });
}

#[test]
fn preserves_remote_access_session_and_service_cli_arguments() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    run_with_faked_timers(async move {
        let (session_coordinator, session_processes) = create_coordinator(CoordinatorOptions::default());
        let (service_coordinator, service_processes) = create_coordinator(CoordinatorOptions::default());
        session_coordinator.set_remote_access(active_mode(false), LogLevel::Info).await.unwrap();
        service_coordinator.set_remote_access(active_mode(true), LogLevel::Info).await.unwrap();

        let session_args = find_process(&session_processes, |process| has(&process.args, "--accept-server-license-terms")).args.clone();
        let service_args = find_process(&service_processes, |process| has(&process.args, "install")).args.clone();

        exit_all(&session_processes).await;
        exit_all(&service_processes).await;
        session_coordinator.dispose();
        service_coordinator.dispose();

        let pid = std::process::id().to_string();
        assert_eq!(
            session_args,
            strings(&[
                "tunnel",
                "--accept-server-license-terms",
                "--log",
                "info",
                "--user-data-dir",
                "custom-user-data",
                "--delegate-to-editor",
                "--name",
                "test_host",
                "--parent-process-id",
                pid.as_str(),
            ])
        );
        assert_eq!(
            service_args,
            strings(&["tunnel", "service", "install", "--accept-server-license-terms", "--log", "info", "--user-data-dir", "custom-user-data", "--name", "test_host"])
        );
    });
}

#[test]
fn uninstalls_the_service_when_a_restart_preempts_the_reconcile() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    run_with_faked_timers(async move {
        let (coordinator, processes) = create_coordinator(CoordinatorOptions::default());
        coordinator.set_remote_access(active_mode(true), LogLevel::Info).await.unwrap();
        // Turning the service off owes an uninstall. A restart in the same
        // tick preempts that reconcile, so the requirement must survive.
        let stop_service = coordinator.set_remote_access(inactive_mode(), LogLevel::Info);
        let restart = coordinator.restart();
        stop_service.await.unwrap();
        restart.await.unwrap();

        let uninstalled = processes.borrow().iter().any(|process| has(&process.args, "uninstall"));
        let status = coordinator.get_status().mode;

        coordinator.dispose();

        assert_eq!((uninstalled, status), (true, TunnelProcessMode::None));
    });
}

#[test]
fn parses_and_fans_machine_status_events_to_every_registered_consumer() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    run_with_faked_timers(async move {
        let (coordinator, processes) = create_coordinator(CoordinatorOptions::default());
        let first: Rc<RefCell<Vec<&'static str>>> = Rc::default();
        let second: Rc<RefCell<Vec<&'static str>>> = Rc::default();
        let first_listener = coordinator.on_did_machine_status().listen({
            let first = first.clone();
            move |event: &ITunnelProcessMachineStatus| first.borrow_mut().push(event.status.type_())
        });
        let second_listener = coordinator.on_did_machine_status().listen({
            let second = second.clone();
            move |event: &ITunnelProcessMachineStatus| second.borrow_mut().push(event.status.type_())
        });

        coordinator.set_remote_access(active_mode(false), LogLevel::Info).await.unwrap();
        let tunnel = find_process(&processes, |process| has(&process.args, "--accept-server-license-terms"));
        tunnel.child.write_stdout("__VSCODE_CLI_STATUS__{\"type\":\"connected\",\"tunnelName\":\"test_host\",\"tunnelId\":\"tunnel-id\",\"isAttached\":false}\n");
        set_immediate().await.unwrap();

        let status = coordinator.get_status();
        let observed = (first.borrow().clone(), second.borrow().clone(), status.connection_state, status.tunnel_id, tunnel.env.get("VSCODE_CLI_MACHINE_STATUS").cloned());

        first_listener.dispose();
        second_listener.dispose();
        coordinator.dispose();

        assert_eq!(
            observed,
            (
                vec!["connected"],
                vec!["connected"],
                TunnelProcessConnectionState::Connected,
                Some("tunnel-id".to_owned()),
                Some("1".to_owned()),
            )
        );
    });
}
