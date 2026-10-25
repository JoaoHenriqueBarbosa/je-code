//! Porte de `src/vs/platform/remoteTunnel/node/tunnelProcessCoordinator.ts`: o dono do único processo de túnel do
//! shared process (`code tunnel`), que o Remote Tunnel Access liga, desliga e reinicia.
//!
//! A fila serial sempre espera o processo filho anterior sair antes de iniciar o substituto. Assim uma transição de
//! modo libera o `--name` antes de outro modo reivindicá-lo.
//!
//! Divergências:
//! - `ITunnelProcessCoordinator` é um trait usado como `Rc<dyn ITunnelProcessCoordinator>` (sem `createDecorator` nem
//!   `_serviceBrand`); os eventos são [`Event`]s e os métodos assíncronos devolvem [`Promise`]. Os literais de texto
//!   (`'none' | 'remoteAccess' | 'service'` e `'disconnected' | 'connecting' | 'connected'`) são enums.
//! - Os serviços injetados (`IConfigurationService`, `INativeEnvironmentService`, `IProductService`,
//!   `ITelemetryService`) entram pelo construtor, como os traits do porte.
//! - As funções `async` são [`async_fn`] sobre um estado compartilhado (`Rc`); o estado mutável do original (`this._x`) é
//!   um `RefCell`, e nenhum empréstimo fica aberto quando um evento é disparado ou uma promessa é aguardada.
//! - `process.pid` é `std::process::id()`; `os.hostname()` lê o nome da máquina do sistema (`/proc`, `/etc/hostname`,
//!   `COMPUTERNAME` ou o utilitário `hostname`), porque a biblioteca padrão não o expõe.
//! - `cancel` do evento de status de máquina é um `Rc<dyn Fn()>`; as comparações por identidade do original
//!   (`this._currentProcess === run`) são `Rc::ptr_eq`.

use std::cell::RefCell;
use std::collections::BTreeMap;
use std::process::Command;
use std::rc::Rc;

use je_base::async_::async_fn;
use je_base::dynamic::{Dynamic, Truthy};
use je_base::errors::Error;
use je_base::event::{Emitter, Event};
use je_base::json::{self, ParseOptions, Value};
use je_base::lifecycle::{Disposable, IDisposable};
use je_base::promise::Promise;

use super::code_tunnel_cli_process::{CodeTunnelCli, CodeTunnelCliOptions, CodeTunnelCliOutput, CodeTunnelCliRun, CodeTunnelLog};
use super::remote_tunnel::{
    CONFIGURATION_KEY_HOST_NAME, CONFIGURATION_KEY_PREVENT_SLEEP, TunnelMode, TunnelStatus, normalize_tunnel_name, tunnel_name_from_hostname,
};
use super::tunnel_machine_status::{TunnelMachineStatus, parse_tunnel_machine_status};
use super::tunnel_service_headers::{ITunnelServiceCorrelation, TunnelServiceOperation, create_tunnel_service_correlation};
use crate::configuration::service::IConfigurationService;
use crate::environment::environment::{EnvironmentService, NativeEnvironmentServiceApi};
use crate::log::{LogLevel, log_level_to_string};
use crate::product::IProductService;
use crate::telemetry::telemetry::ITelemetryService;

/// `TunnelCliFactory`: recebe o `onLog` e devolve a CLI.
pub type TunnelCliFactory = Box<dyn Fn(CodeTunnelLog) -> CodeTunnelCli>;

/// The process mode selected from the Remote Tunnel Access intent.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TunnelProcessMode {
    /// `'none'`.
    None,
    /// `'remoteAccess'`.
    RemoteAccess,
    /// `'service'`.
    Service,
}

/// The connection lifecycle state reported by the coordinator.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TunnelProcessConnectionState {
    Disconnected,
    Connecting,
    Connected,
}

/// A line emitted by a CLI invocation owned by the coordinator.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ITunnelProcessOutput {
    pub mode: TunnelProcessMode,
    pub message: String,
    pub is_error: bool,
}

/// A machine-status event emitted by a CLI invocation owned by the coordinator.
#[derive(Clone)]
pub struct ITunnelProcessMachineStatus {
    pub mode: TunnelProcessMode,
    pub status: TunnelMachineStatus,
    pub correlation: Option<ITunnelServiceCorrelation>,
    pub cancel: Rc<dyn Fn()>,
}

impl ITunnelProcessMachineStatus {
    /// `cancel()`.
    pub fn cancel(&self) {
        (self.cancel)();
    }
}

/// The tunnel process state shared by Remote Tunnel Access consumers.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ITunnelProcessStatus {
    pub mode: TunnelProcessMode,
    pub tunnel_name: Option<String>,
    pub tunnel_id: Option<String>,
    pub connection_state: TunnelProcessConnectionState,
    pub service_install_failed: bool,
}

impl ITunnelProcessStatus {
    /// O estado de quando não há túnel: `{ mode: 'none', ..., connectionState: 'disconnected' }`.
    pub fn idle() -> Self {
        Self { mode: TunnelProcessMode::None, tunnel_name: None, tunnel_id: None, connection_state: TunnelProcessConnectionState::Disconnected, service_install_failed: false }
    }
}

/// Coordinates the `code tunnel` process used by Remote Tunnel Access.
pub trait ITunnelProcessCoordinator {
    fn on_did_change_status(&self) -> Event<ITunnelProcessStatus>;
    fn on_did_output(&self) -> Event<ITunnelProcessOutput>;
    fn on_did_machine_status(&self) -> Event<ITunnelProcessMachineStatus>;
    fn get_status(&self) -> ITunnelProcessStatus;
    fn get_intended_tunnel_name(&self) -> String;
    fn set_remote_access(&self, mode: TunnelMode, log_level: LogLevel) -> Promise<()>;
    fn restart(&self) -> Promise<()>;
    fn set_remote_access_status(&self, status: &TunnelStatus);
}

/// Resolves the process mode from the Remote Tunnel Access intent.
pub fn resolve_tunnel_process_mode(remote_access: &TunnelMode) -> TunnelProcessMode {
    match remote_access {
        TunnelMode::Active(active) if active.as_service => TunnelProcessMode::Service,
        TunnelMode::Active(_) => TunnelProcessMode::RemoteAccess,
        TunnelMode::Inactive(_) => TunnelProcessMode::None,
    }
}

/// Credentials passed to `tunnel user login`.
struct TunnelLoginCredentials {
    provider_id: String,
    token: String,
}

/// The tunnel the current intents resolve to.
struct TunnelTarget {
    mode: TunnelProcessMode,
    login: Option<TunnelLoginCredentials>,
    log_level: LogLevel,
}

/// A launched process's inputs, kept so a later intent update can tell whether
/// it would produce the same process or genuinely needs a new one.
#[derive(Clone, Debug, PartialEq, Eq)]
struct LaunchDescription {
    mode: TunnelProcessMode,
    tunnel_name: String,
    provider_id: Option<String>,
    token: Option<String>,
    log_level: LogLevel,
    prevent_sleep: bool,
}

/// De onde vem uma linha de saída: o que `_fireOutput` recebe além da mensagem.
struct OutputContext {
    mode: TunnelProcessMode,
    is_tunnel_process: bool,
    generation: u64,
    correlation: ITunnelServiceCorrelation,
    cancel: Rc<dyn Fn()>,
}

struct CoordinatorState {
    remote_access_mode: TunnelMode,
    remote_access_log_level: LogLevel,
    current_process: Option<Rc<CodeTunnelCliRun>>,
    queue: Promise<()>,
    generation: u64,
    /// Survives across generations: a newer request can preempt the reconcile
    /// that was going to uninstall the service, and that newer request has no
    /// idea an uninstall was owed. Cleared only once an uninstall succeeds.
    uninstall_service_pending: bool,
    /// Inputs of the currently running process, or undefined when none runs.
    launched: Option<LaunchDescription>,
    status: ITunnelProcessStatus,
}

struct CoordinatorInner {
    tunnel_cli: CodeTunnelCli,
    state: RefCell<CoordinatorState>,
    configuration_service: Rc<dyn IConfigurationService>,
    environment: Rc<dyn NativeEnvironmentServiceApi>,
    telemetry_service: Rc<dyn ITelemetryService>,
    on_did_change_status: Rc<Emitter<ITunnelProcessStatus>>,
    on_did_output: Rc<Emitter<ITunnelProcessOutput>>,
    on_did_machine_status: Rc<Emitter<ITunnelProcessMachineStatus>>,
}

/// Owns the only tunnel process in the shared process.
///
/// The serial queue always waits for the previous child process to exit before
/// spawning its replacement. This makes a mode transition release `--name`
/// before another mode can claim it.
pub struct TunnelProcessCoordinator {
    base: Rc<Disposable>,
    inner: Rc<CoordinatorInner>,
}

impl TunnelProcessCoordinator {
    pub fn new(
        tunnel_cli_factory: Option<TunnelCliFactory>,
        configuration_service: Rc<dyn IConfigurationService>,
        environment: Rc<dyn NativeEnvironmentServiceApi>,
        product: &dyn IProductService,
        telemetry_service: Rc<dyn ITelemetryService>,
    ) -> Rc<Self> {
        let base = Disposable::new();
        let on_did_change_status: Rc<Emitter<ITunnelProcessStatus>> = Emitter::new();
        let on_did_output: Rc<Emitter<ITunnelProcessOutput>> = Emitter::new();
        let on_did_machine_status: Rc<Emitter<ITunnelProcessMachineStatus>> = Emitter::new();
        base.register(on_did_change_status.clone());
        base.register(on_did_output.clone());
        base.register(on_did_machine_status.clone());

        let tunnel_cli = match tunnel_cli_factory {
            Some(factory) => factory(Rc::new(|_: &str| {}) as CodeTunnelLog),
            None => CodeTunnelCli::new(CodeTunnelCliOptions {
                app_root: environment.app_root(),
                is_built: environment.is_built(),
                tunnel_application_name: product.tunnel_application_name().map(str::to_owned),
                win32_versioned_update: product.win32_versioned_update(),
                spawn: None,
                on_log: None,
            }),
        };

        let inner = Rc::new(CoordinatorInner {
            tunnel_cli,
            state: RefCell::new(CoordinatorState {
                remote_access_mode: TunnelMode::Inactive(super::remote_tunnel::inactive_tunnel_mode()),
                remote_access_log_level: LogLevel::Info,
                current_process: None,
                queue: Promise::resolved(()),
                generation: 0,
                uninstall_service_pending: false,
                launched: None,
                status: ITunnelProcessStatus::idle(),
            }),
            configuration_service,
            environment,
            telemetry_service,
            on_did_change_status,
            on_did_output,
            on_did_machine_status,
        });
        Rc::new(Self { base, inner })
    }
}

impl ITunnelProcessCoordinator for TunnelProcessCoordinator {
    fn on_did_change_status(&self) -> Event<ITunnelProcessStatus> {
        self.inner.on_did_change_status.event()
    }

    fn on_did_output(&self) -> Event<ITunnelProcessOutput> {
        self.inner.on_did_output.event()
    }

    fn on_did_machine_status(&self) -> Event<ITunnelProcessMachineStatus> {
        self.inner.on_did_machine_status.event()
    }

    fn get_status(&self) -> ITunnelProcessStatus {
        self.inner.status()
    }

    /// The name a tunnel would be given right now, from configuration or the
    /// hostname. Unlike [`ITunnelProcessCoordinator::get_status`]'s `tunnelName` this is defined even
    /// when no tunnel process is running, so callers can compare the intended
    /// name against a previously used one.
    fn get_intended_tunnel_name(&self) -> String {
        self.inner.get_tunnel_name()
    }

    fn set_remote_access(&self, mode: TunnelMode, log_level: LogLevel) -> Promise<()> {
        let uninstall_service = {
            let mut state = self.inner.state.borrow_mut();
            let was_service = matches!(&state.remote_access_mode, TunnelMode::Active(active) if active.as_service);
            let keeps_service = matches!(&mode, TunnelMode::Active(active) if active.as_service);
            state.remote_access_mode = mode;
            state.remote_access_log_level = log_level;
            was_service && !keeps_service
        };
        self.inner.schedule(uninstall_service, false)
    }

    fn restart(&self) -> Promise<()> {
        self.inner.schedule(false, true)
    }

    fn set_remote_access_status(&self, status: &TunnelStatus) {
        let current = self.inner.status();
        if current.mode != TunnelProcessMode::RemoteAccess && current.mode != TunnelProcessMode::Service {
            return;
        }
        let connection_state = match status {
            TunnelStatus::Connected(_) => TunnelProcessConnectionState::Connected,
            TunnelStatus::Connecting(_) => TunnelProcessConnectionState::Connecting,
            TunnelStatus::Disconnected(_) | TunnelStatus::Uninitialized(_) => TunnelProcessConnectionState::Disconnected,
        };
        self.inner.set_status(ITunnelProcessStatus { connection_state, ..current });
    }
}

impl IDisposable for TunnelProcessCoordinator {
    fn dispose(&self) {
        let run = {
            let mut state = self.inner.state.borrow_mut();
            state.generation += 1;
            state.current_process.clone()
        };
        if let Some(run) = run {
            drop(run.stop());
        }
        self.base.dispose();
    }
}

impl CoordinatorInner {
    fn status(&self) -> ITunnelProcessStatus {
        self.state.borrow().status.clone()
    }

    fn generation(&self) -> u64 {
        self.state.borrow().generation
    }

    fn schedule(self: &Rc<Self>, uninstall_service: bool, force_restart: bool) -> Promise<()> {
        let uninstall_service_pending = {
            let mut state = self.state.borrow_mut();
            state.uninstall_service_pending = state.uninstall_service_pending || uninstall_service;
            state.uninstall_service_pending
        };

        // Checked before stopping anything: `_reconcile` can only observe a
        // process this method already stopped, so a no-op update would
        // otherwise tear down a perfectly healthy tunnel.
        if !force_restart && !uninstall_service_pending && self.is_target_satisfied() {
            return Promise::resolved(());
        }

        let (generation, current_process, queue) = {
            let mut state = self.state.borrow_mut();
            state.generation += 1;
            (state.generation, state.current_process.clone(), state.queue.clone())
        };
        if let Some(run) = current_process {
            drop(run.stop());
        }
        let this = self.clone();
        let operation = queue.then_promise(move |()| this.reconcile(generation));
        self.state.borrow_mut().queue = operation.catch(|_| ());
        operation
    }

    /// Whether the running process was launched from exactly the inputs the
    /// current intent resolves to. Compares the full launch description, not
    /// just mode and name: callers also update the session token, log level and
    /// sleep prevention, and each of those has to reach a new process.
    fn is_target_satisfied(&self) -> bool {
        let target = self.get_target();
        if target.mode == TunnelProcessMode::None {
            return self.status().mode == TunnelProcessMode::None;
        }
        let wanted = self.describe_launch(&target);
        let state = self.state.borrow();
        // A run that already reported failure cannot satisfy anything: a token
        // error cancels the child and marks us disconnected before it exits, so
        // treating it as healthy would skip the reconcile and leave nothing
        // running once the cancelled child finally goes away.
        if state.current_process.is_none() || state.status.connection_state == TunnelProcessConnectionState::Disconnected {
            return false;
        }
        state.launched.as_ref() == Some(&wanted)
    }

    /// Everything that changes what a launched tunnel process actually does.
    fn describe_launch(&self, target: &TunnelTarget) -> LaunchDescription {
        LaunchDescription {
            mode: target.mode,
            tunnel_name: self.get_tunnel_name(),
            provider_id: target.login.as_ref().map(|login| login.provider_id.clone()),
            token: target.login.as_ref().map(|login| login.token.clone()),
            log_level: target.log_level,
            prevent_sleep: self.prevent_sleep(),
        }
    }

    fn reconcile(self: &Rc<Self>, generation: u64) -> Promise<()> {
        let this = self.clone();
        async_fn(async move {
            this.stop_current_process().await?;
            if generation != this.generation() {
                return Ok(());
            }

            let uninstall_service_pending = this.state.borrow().uninstall_service_pending;
            if uninstall_service_pending {
                let exit_code = this.run_transient("serviceUninstall", strings(&["tunnel", "service", "uninstall"]), TunnelProcessMode::None, generation, None, None).await?;
                if exit_code == 0 {
                    this.state.borrow_mut().uninstall_service_pending = false;
                }
                if generation != this.generation() {
                    return Ok(());
                }
            }

            let target = this.get_target();

            if target.mode == TunnelProcessMode::None {
                this.run_transient("kill", strings(&["tunnel", "kill"]), TunnelProcessMode::None, generation, None, None).await?;
                if generation == this.generation() {
                    this.set_status(ITunnelProcessStatus::idle());
                }
                return Ok(());
            }

            let tunnel_name = this.get_tunnel_name();
            this.set_status(ITunnelProcessStatus {
                mode: target.mode,
                tunnel_name: Some(tunnel_name.clone()),
                tunnel_id: None,
                connection_state: TunnelProcessConnectionState::Connecting,
                service_install_failed: false,
            });
            let is_service_installed = this.is_service_installed(generation).await?;
            if generation != this.generation() {
                return Ok(());
            }
            if target.mode == TunnelProcessMode::Service && !is_service_installed {
                let service_install_failed = !this.install_service(target.log_level, &tunnel_name, generation).await?;
                if generation != this.generation() {
                    return Ok(());
                }
                // A failed install is not fatal: the session tunnel below still
                // runs, matching the pre-CLI behaviour of falling back to hosting
                // in-session and reporting the failure alongside it.
                this.set_status(ITunnelProcessStatus { service_install_failed, ..this.status() });
            }

            if let Some(login) = &target.login {
                let args = vec![
                    "tunnel".to_owned(),
                    "user".to_owned(),
                    "login".to_owned(),
                    "--provider".to_owned(),
                    login.provider_id.clone(),
                    "--log".to_owned(),
                    log_level_to_string(target.log_level).to_owned(),
                ];
                let environment = BTreeMap::from([("VSCODE_CLI_ACCESS_TOKEN".to_owned(), login.token.clone())]);
                let login_exit_code = this.run_transient("login", args, target.mode, generation, Some(environment), None).await?;
                if generation != this.generation() || login_exit_code != 0 {
                    if generation == this.generation() {
                        this.set_status(ITunnelProcessStatus { connection_state: TunnelProcessConnectionState::Disconnected, ..this.status() });
                    }
                    return Ok(());
                }
            }

            let mut args = vec![
                "tunnel".to_owned(),
                "--accept-server-license-terms".to_owned(),
                "--log".to_owned(),
                log_level_to_string(target.log_level).to_owned(),
                "--user-data-dir".to_owned(),
                this.environment.user_data_path(),
                "--delegate-to-editor".to_owned(),
                "--name".to_owned(),
                tunnel_name,
                "--parent-process-id".to_owned(),
                std::process::id().to_string(),
            ];
            if this.prevent_sleep() {
                args.push("--no-sleep".to_owned());
            }
            let launched = this.describe_launch(&target);
            this.state.borrow_mut().launched = Some(launched);
            this.start_tunnel(args, target.mode, generation);
            Ok(())
        })
    }

    /// The credentials the CLI needs for `tunnel user login`.
    fn get_target(&self) -> TunnelTarget {
        let (mode, log_level) = {
            let state = self.state.borrow();
            (state.remote_access_mode.clone(), state.remote_access_log_level)
        };
        match &mode {
            TunnelMode::Active(active) => TunnelTarget {
                mode: resolve_tunnel_process_mode(&mode),
                login: active
                    .session
                    .token
                    .as_ref()
                    .filter(|token| !token.is_empty())
                    .map(|token| TunnelLoginCredentials { provider_id: active.session.provider_id.clone(), token: token.clone() }),
                log_level,
            },
            TunnelMode::Inactive(_) => TunnelTarget { mode: TunnelProcessMode::None, login: None, log_level: LogLevel::Info },
        }
    }

    fn is_service_installed(self: &Rc<Self>, generation: u64) -> Promise<bool> {
        let this = self.clone();
        async_fn(async move {
            let output = Rc::new(RefCell::new(String::new()));
            let collector: CodeTunnelCliOutput = {
                let output = output.clone();
                Rc::new(move |message: &str, is_error: bool| {
                    if !is_error {
                        output.borrow_mut().push_str(message);
                    }
                })
            };
            let exit_code = this.run_transient("status", strings(&["tunnel", "status"]), TunnelProcessMode::Service, generation, None, Some(collector)).await?;
            if exit_code != 0 {
                return Ok(false);
            }
            let text = output.borrow().clone();
            Ok(parse_service_installed(&text))
        })
    }

    fn install_service(self: &Rc<Self>, log_level: LogLevel, tunnel_name: &str, generation: u64) -> Promise<bool> {
        let this = self.clone();
        let tunnel_name = tunnel_name.to_owned();
        async_fn(async move {
            let args = vec![
                "tunnel".to_owned(),
                "service".to_owned(),
                "install".to_owned(),
                "--accept-server-license-terms".to_owned(),
                "--log".to_owned(),
                log_level_to_string(log_level).to_owned(),
                "--user-data-dir".to_owned(),
                this.environment.user_data_path(),
                "--name".to_owned(),
                tunnel_name,
            ];
            Ok(this.run_transient("serviceInstall", args, TunnelProcessMode::Service, generation, None, None).await? == 0)
        })
    }

    fn start_tunnel(self: &Rc<Self>, args: Vec<String>, mode: TunnelProcessMode, generation: u64) {
        let correlation = create_tunnel_service_correlation(&*self.telemetry_service, Some(TunnelServiceOperation::Host));
        let run_slot: Rc<RefCell<Option<Rc<CodeTunnelCliRun>>>> = Rc::default();
        let context = OutputContext { mode, is_tunnel_process: true, generation, correlation: correlation.clone(), cancel: cancel_of(&run_slot) };
        let on_output: CodeTunnelCliOutput = {
            let this = self.clone();
            Rc::new(move |message: &str, is_error: bool| this.fire_output(&context, message, is_error))
        };
        let mut environment = BTreeMap::from([("VSCODE_CLI_MACHINE_STATUS".to_owned(), "1".to_owned())]);
        environment.extend(correlation_environment(&correlation));
        let tunnel_run = Rc::new(self.tunnel_cli.run("tunnel", &args, on_output, Some(environment)));
        *run_slot.borrow_mut() = Some(tunnel_run.clone());
        self.state.borrow_mut().current_process = Some(tunnel_run.clone());
        let this = self.clone();
        let run = tunnel_run.clone();
        // `void tunnelRun.result.then(onSettled, onSettled)`.
        tunnel_run.result.then_result(move |_| {
            this.on_tunnel_settled(&run, generation);
            Ok::<(), Error>(())
        });
    }

    fn on_tunnel_settled(&self, run: &Rc<CodeTunnelCliRun>, generation: u64) {
        let is_current = {
            let mut state = self.state.borrow_mut();
            let is_current = state.current_process.as_ref().is_some_and(|current| Rc::ptr_eq(current, run));
            if is_current {
                state.current_process = None;
                state.launched = None;
            }
            is_current
        };
        if is_current && generation == self.generation() {
            self.set_status(ITunnelProcessStatus { connection_state: TunnelProcessConnectionState::Disconnected, ..self.status() });
        }
    }

    fn run_transient(
        self: &Rc<Self>,
        log_label: &str,
        args: Vec<String>,
        mode: TunnelProcessMode,
        generation: u64,
        env: Option<BTreeMap<String, String>>,
        on_output: Option<CodeTunnelCliOutput>,
    ) -> Promise<i32> {
        let this = self.clone();
        let log_label = log_label.to_owned();
        async_fn(async move {
            let correlation = create_tunnel_service_correlation(&*this.telemetry_service, None);
            let run_slot: Rc<RefCell<Option<Rc<CodeTunnelCliRun>>>> = Rc::default();
            let context = OutputContext { mode, is_tunnel_process: false, generation, correlation: correlation.clone(), cancel: cancel_of(&run_slot) };
            let output: CodeTunnelCliOutput = {
                let this = this.clone();
                Rc::new(move |message: &str, is_error: bool| {
                    if let Some(on_output) = &on_output {
                        on_output(message, is_error);
                    }
                    this.fire_output(&context, message, is_error);
                })
            };
            let mut environment = env.unwrap_or_default();
            environment.extend(correlation_environment(&correlation));
            let run = Rc::new(this.tunnel_cli.run(&log_label, &args, output, Some(environment)));
            *run_slot.borrow_mut() = Some(run.clone());
            this.state.borrow_mut().current_process = Some(run.clone());
            let result = run.result.promise.clone().await;
            {
                let mut state = this.state.borrow_mut();
                if state.current_process.as_ref().is_some_and(|current| Rc::ptr_eq(current, &run)) {
                    state.current_process = None;
                }
            }
            // `catch { return 1; }`
            Ok(result.unwrap_or(1))
        })
    }

    fn stop_current_process(self: &Rc<Self>) -> Promise<()> {
        let this = self.clone();
        async_fn(async move {
            let run = this.state.borrow().current_process.clone();
            let Some(run) = run else {
                return Ok(());
            };
            run.stop().await?;
            let mut state = this.state.borrow_mut();
            if state.current_process.as_ref().is_some_and(|current| Rc::ptr_eq(current, &run)) {
                state.current_process = None;
                state.launched = None;
            }
            Ok(())
        })
    }

    fn fire_output(&self, context: &OutputContext, message: &str, is_error: bool) {
        self.on_did_output.fire(ITunnelProcessOutput { mode: context.mode, message: message.to_owned(), is_error });
        if is_error || !context.is_tunnel_process || context.generation != self.generation() {
            return;
        }
        let Some(status) = parse_tunnel_machine_status(message) else {
            return;
        };
        let mut is_attached = false;
        if let TunnelMachineStatus::Connected(connected) = &status {
            is_attached = connected.is_attached;
            let current = self.status();
            if current.mode == context.mode {
                self.set_status(ITunnelProcessStatus {
                    tunnel_id: connected.tunnel_id.clone(),
                    connection_state: TunnelProcessConnectionState::Connected,
                    ..current
                });
            }
        }
        self.on_did_machine_status.fire(ITunnelProcessMachineStatus {
            mode: context.mode,
            status,
            cancel: context.cancel.clone(),
            correlation: if is_attached { None } else { Some(context.correlation.clone()) },
        });
    }

    fn set_status(&self, status: ITunnelProcessStatus) {
        {
            let mut state = self.state.borrow_mut();
            if state.status == status {
                return;
            }
            state.status = status.clone();
        }
        self.on_did_change_status.fire(status);
    }

    fn get_tunnel_name(&self) -> String {
        let configured = match self.configuration_service.get_value(Some(CONFIGURATION_KEY_HOST_NAME), None) {
            Dynamic::String(name) => Some(name.to_string_lossy()).filter(|name| !name.is_empty()),
            _ => None,
        };
        let name = configured.map_or_else(|| tunnel_name_from_hostname(&os_hostname()), |configured| normalize_tunnel_name(&configured));
        if name.is_empty() { "vscode".to_owned() } else { name }
    }

    fn prevent_sleep(&self) -> bool {
        self.configuration_service.get_value(Some(CONFIGURATION_KEY_PREVENT_SLEEP), None).is_truthy()
    }
}

/// `JSON.parse(output.trim().split('\n').find(line => line.startsWith('{'))).service_installed`; qualquer falha é `false`.
fn parse_service_installed(output: &str) -> bool {
    let Some(line) = output.trim().split('\n').find(|line| line.starts_with('{')) else {
        return false;
    };
    let units: Vec<u16> = line.encode_utf16().collect();
    let mut errors = Vec::new();
    let options = ParseOptions { disallow_comments: true, allow_trailing_comma: false, allow_empty_content: false };
    match json::parse(&units, &mut errors, options) {
        Some(Value::Object(status)) if errors.is_empty() => matches!(status.get("service_installed"), Some(Value::Bool(true))),
        _ => false,
    }
}

/// `{ VSCODE_TUNNEL_SESSION_ID, VSCODE_TUNNEL_OPERATION_ID }`.
fn correlation_environment(correlation: &ITunnelServiceCorrelation) -> BTreeMap<String, String> {
    BTreeMap::from([
        ("VSCODE_TUNNEL_SESSION_ID".to_owned(), correlation.session_id.clone()),
        ("VSCODE_TUNNEL_OPERATION_ID".to_owned(), correlation.operation_id.clone()),
    ])
}

/// O `() => tunnelRun.result.cancel()`: a corrida só existe depois de `run` devolver, então o cancelamento lê o espaço
/// que `run` preenche.
fn cancel_of(slot: &Rc<RefCell<Option<Rc<CodeTunnelCliRun>>>>) -> Rc<dyn Fn()> {
    let slot = slot.clone();
    Rc::new(move || {
        let run = slot.borrow().clone();
        if let Some(run) = run {
            run.result.cancel();
        }
    })
}

fn strings(items: &[&str]) -> Vec<String> {
    items.iter().map(|item| (*item).to_owned()).collect()
}

/// `os.hostname()`.
fn os_hostname() -> String {
    let from_file = ["/proc/sys/kernel/hostname", "/etc/hostname"]
        .iter()
        .find_map(|path| std::fs::read_to_string(path).ok().map(|text| text.trim().to_owned()).filter(|text| !text.is_empty()));
    from_file
        .or_else(|| std::env::var("COMPUTERNAME").ok().filter(|name| !name.is_empty()))
        .or_else(|| {
            Command::new("hostname")
                .output()
                .ok()
                .and_then(|output| String::from_utf8(output.stdout).ok())
                .map(|text| text.trim().to_owned())
                .filter(|text| !text.is_empty())
        })
        .unwrap_or_default()
}
