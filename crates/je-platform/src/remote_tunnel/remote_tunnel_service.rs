//! Porte de `src/vs/platform/remoteTunnel/node/remoteTunnelService.ts`: publica o estado do Remote Tunnel Access
//! enquanto o coordenador é dono do processo da CLI.
//!
//! Divergências:
//! - `IRemoteTunnelService` é o trait de `remote_tunnel.rs`, e o serviço é um `Rc<RemoteTunnelService>` (como o
//!   `ConfigurationService`): os métodos assíncronos e os ouvintes precisam do próprio `Rc`, que o serviço guarda
//!   como `Weak` (`me`).
//! - O `IProductService` do construtor (`_productService`) nunca é lido pelo serviço e não existe aqui; o
//!   `INativeEnvironmentService` é o trait reduzido [`RemoteTunnelEnvironment`], e o `ISharedProcessLifecycleService`
//!   (que ainda não tem porte) é o trait [`ISharedProcessLifecycleService`] deste arquivo, só com o `onWillShutdown`.
//! - `publicLog2<E, T>` é `public_log2` com o dado como [`Dynamic`]; os campos `undefined` do original ficam fora do
//!   objeto, exceto `tunnelName` do evento `remoteTunnel.enablement`, que o original envia como `undefined`.
//! - `this._logger.error(error)` é `error_args` com o [`Error`]; `JSON.parse` do `restoreMode` é o `json::parse` estrito,
//!   e um JSON inválido vira o erro `SyntaxError` registrado em `Problems restoring session from storage`.
//! - `this._startTunnelProcessDelayer.trigger(...)` sem `await` (o ouvinte de configuração) descarta a promessa.

use std::cell::{Cell, RefCell};
use std::rc::{Rc, Weak};

use je_base::async_::{Delayer, async_fn};
use je_base::dynamic::{Dynamic, json_stringify};
use je_base::errors::Error;
use je_base::event::{Emitter, Event};
use je_base::json::{self, ParseOptions, Value};
use je_base::lifecycle::{Disposable, IDisposable, to_disposable};
use je_base::nls::localize;
use je_base::promise::{MaybePromise, Promise};
use je_base::symbols::Delay;
use je_storage::StorageValue;

use super::remote_tunnel::{
    ActiveTunnelMode, CONFIGURATION_KEY_HOST_NAME, CONFIGURATION_KEY_PREVENT_SLEEP, ConnectionInfo, IRemoteTunnelService, IRemoteTunnelSession, LOG_ID, TunnelMode,
    TunnelStatus, inactive_tunnel_mode, logger_name, tunnel_states,
};
use super::remote_tunnel_environment::RemoteTunnelEnvironment;
use super::tunnel_machine_status::TunnelMachineStatus;
use super::tunnel_process_coordinator::{ITunnelProcessCoordinator, ITunnelProcessMachineStatus, ITunnelProcessOutput, ITunnelProcessStatus, TunnelProcessMode};
use crate::configuration::models::ConfigurationChangeEvent;
use crate::configuration::service::IConfigurationService;
use crate::js::from_json;
use crate::log::{LogArg, LogMessage, Logger, LoggerOptions, LoggerRef, LoggerService, ResourceOrId};
use crate::resources::join_path;
use crate::storage::storage::{StorageScope, StorageService, StorageTarget};
use crate::telemetry::telemetry::ITelemetryService;

const RESTART_TUNNEL_ON_CONFIGURATION_CHANGES: [&str; 2] = [CONFIGURATION_KEY_HOST_NAME, CONFIGURATION_KEY_PREVENT_SLEEP];

const TUNNEL_ACCESS_SESSION: &str = "remoteTunnelSession";
const TUNNEL_ACCESS_IS_SERVICE: &str = "remoteTunnelIsService";

/// `ISharedProcessLifecycleService`, reduzido ao que o serviço lê: o `onWillShutdown`.
pub trait ISharedProcessLifecycleService {
    fn on_will_shutdown(&self) -> Event<()>;
}

/// Publishes Remote Tunnel Access status while the coordinator owns the CLI process.
pub struct RemoteTunnelService {
    me: Weak<RemoteTunnelService>,
    base: Rc<Disposable>,
    on_did_token_failed: Rc<Emitter<Option<IRemoteTunnelSession>>>,
    on_did_change_tunnel_status: Rc<Emitter<TunnelStatus>>,
    on_did_change_mode: Rc<Emitter<TunnelMode>>,
    logger: LoggerRef,
    start_tunnel_process_delayer: Rc<Delayer<()>>,
    mode: RefCell<TunnelMode>,
    tunnel_status: RefCell<TunnelStatus>,
    initialized: Cell<bool>,
    telemetry_service: Rc<dyn ITelemetryService>,
    environment: Rc<dyn RemoteTunnelEnvironment>,
    storage_service: Rc<dyn StorageService>,
    tunnel_process_coordinator: Rc<dyn ITunnelProcessCoordinator>,
}

/// Um ouvinte que repassa o evento ao serviço enquanto ele vive (sem o manter vivo).
fn forward<T: 'static>(weak: &Weak<RemoteTunnelService>, handler: impl Fn(&RemoteTunnelService, &T) + 'static) -> impl Fn(&T) + 'static {
    let weak = weak.clone();
    move |value: &T| {
        if let Some(service) = weak.upgrade() {
            handler(&service, value);
        }
    }
}

impl RemoteTunnelService {
    pub fn new(
        telemetry_service: Rc<dyn ITelemetryService>,
        environment: Rc<dyn RemoteTunnelEnvironment>,
        logger_service: Rc<dyn LoggerService>,
        shared_process_lifecycle_service: Rc<dyn ISharedProcessLifecycleService>,
        configuration_service: Rc<dyn IConfigurationService>,
        storage_service: Rc<dyn StorageService>,
        tunnel_process_coordinator: Rc<dyn ITunnelProcessCoordinator>,
    ) -> Rc<Self> {
        let base = Disposable::new();
        let on_did_token_failed: Rc<Emitter<Option<IRemoteTunnelSession>>> = Emitter::new();
        let on_did_change_tunnel_status: Rc<Emitter<TunnelStatus>> = Emitter::new();
        let on_did_change_mode: Rc<Emitter<TunnelMode>> = Emitter::new();
        base.register(on_did_token_failed.clone());
        base.register(on_did_change_tunnel_status.clone());
        base.register(on_did_change_mode.clone());

        let logs_file = format!("{LOG_ID}.log");
        let logger = logger_service.create_logger(
            ResourceOrId::Resource(join_path(&environment.logs_home(), &[logs_file.as_str()])),
            Some(LoggerOptions { id: Some(LOG_ID.to_owned()), name: Some(logger_name()), ..LoggerOptions::default() }),
        );
        base.register(to_disposable({
            let logger = logger.clone();
            move || logger.dispose()
        }));

        let start_tunnel_process_delayer: Rc<Delayer<()>> = Rc::new(Delayer::new(Delay::Millis(100)));
        base.register(start_tunnel_process_delayer.clone());

        let service = Rc::new_cyclic(|me| Self {
            me: me.clone(),
            base,
            on_did_token_failed,
            on_did_change_tunnel_status,
            on_did_change_mode,
            logger,
            start_tunnel_process_delayer,
            mode: RefCell::new(TunnelMode::Inactive(inactive_tunnel_mode())),
            tunnel_status: RefCell::new(TunnelStatus::Uninitialized(tunnel_states::uninitialized())),
            initialized: Cell::new(false),
            telemetry_service,
            environment,
            storage_service,
            tunnel_process_coordinator,
        });

        let weak = Rc::downgrade(&service);
        let coordinator = &service.tunnel_process_coordinator;
        service.base.register(coordinator.on_did_change_status().listen(forward(&weak, |service, status: &ITunnelProcessStatus| service.handle_coordinator_status(status))));
        service.base.register(coordinator.on_did_output().listen(forward(&weak, |service, output: &ITunnelProcessOutput| service.handle_coordinator_output(output))));
        service.base.register(coordinator.on_did_machine_status().listen(forward(&weak, |service, event: &ITunnelProcessMachineStatus| service.handle_machine_status(event))));
        service.base.register(shared_process_lifecycle_service.on_will_shutdown().listen(forward(&weak, |service, _: &()| service.dispose())));
        service.base.register(configuration_service.on_did_change_configuration().listen(forward(&weak, |service, event: &Rc<ConfigurationChangeEvent>| {
            if RESTART_TUNNEL_ON_CONFIGURATION_CHANGES.iter().any(|key| event.affects_configuration(*key, None)) {
                drop(service.trigger_update_tunnel_process());
            }
        })));

        let restored = service.restore_mode();
        *service.mode.borrow_mut() = restored;
        service
    }

    fn this(&self) -> Rc<Self> {
        self.me.upgrade().expect("o serviço está vivo enquanto um método dele roda")
    }

    /// `this._startTunnelProcessDelayer.trigger(() => this.updateTunnelProcess())`.
    fn trigger_update_tunnel_process(&self) -> Promise<()> {
        let this = self.this();
        self.start_tunnel_process_delayer.trigger(move || MaybePromise::Promise(this.update_tunnel_process()), None)
    }

    /// `try { await this._startTunnelProcessDelayer.trigger(...) } catch (error) { this._logger.error(error) }`.
    fn trigger_update_tunnel_process_logging_errors(&self) -> Promise<()> {
        let this = self.this();
        async_fn(async move {
            if let Err(error) = this.trigger_update_tunnel_process().await {
                this.logger.error_args(&LogMessage::Error(error), &[]);
            }
            Ok(())
        })
    }

    fn set_mode(&self, mode: TunnelMode) {
        let is_same = is_same_mode(&self.mode.borrow(), &mode);
        if is_same {
            return;
        }
        *self.mode.borrow_mut() = mode.clone();
        self.store_mode(&mode);
        self.on_did_change_mode.fire(mode.clone());
        match &mode {
            TunnelMode::Active(active) => {
                let session = &active.session;
                self.logger.info(&format!("Session updated: {} ({}) (service={})", session.account_label, session.provider_id, active.as_service));
                if session.token.as_ref().is_some_and(|token| !token.is_empty()) {
                    self.logger.info(&format!("Session token updated: {} ({})", session.account_label, session.provider_id));
                }
            }
            TunnelMode::Inactive(_) => self.logger.info("Session reset"),
        }
    }

    fn update_tunnel_process(&self) -> Promise<()> {
        let this = self.this();
        async_fn(async move {
            let coordinator = &this.tunnel_process_coordinator;
            let tunnel_name = coordinator.get_status().tunnel_name;
            let mode = this.mode.borrow().clone();
            let (enabled, service) = match &mode {
                TunnelMode::Active(active) => (true, active.as_service),
                TunnelMode::Inactive(_) => (false, false),
            };
            this.telemetry_service.public_log2(
                "remoteTunnel.enablement",
                Some(Dynamic::object([
                    ("enabled", Dynamic::Bool(enabled)),
                    ("service", Dynamic::Bool(service)),
                    ("tunnelName", tunnel_name.map_or(Dynamic::Undefined, |name| Dynamic::string(&name))),
                ])),
            );
            let coordinator_mode = coordinator.get_status().mode;
            if mode.active() || coordinator_mode == TunnelProcessMode::RemoteAccess || coordinator_mode == TunnelProcessMode::Service {
                coordinator.set_remote_access(mode, this.logger.get_level()).await?;
            } else {
                coordinator.restart().await?;
            }
            Ok(())
        })
    }

    fn handle_coordinator_status(&self, status: &ITunnelProcessStatus) {
        use super::tunnel_process_coordinator::TunnelProcessConnectionState;

        if status.mode != TunnelProcessMode::RemoteAccess && status.mode != TunnelProcessMode::Service {
            let inactive = !self.mode.borrow().active();
            let initialized = self.tunnel_status.borrow().type_() != "uninitialized";
            if inactive && initialized {
                self.set_tunnel_status(TunnelStatus::Disconnected(tunnel_states::disconnected(None)));
            }
            return;
        }
        match status.connection_state {
            TunnelProcessConnectionState::Connecting => self.set_tunnel_status(TunnelStatus::Connecting(tunnel_states::connecting(None))),
            TunnelProcessConnectionState::Disconnected => {
                let already_disconnected = self.tunnel_status.borrow().type_() == "disconnected";
                if !already_disconnected {
                    self.set_tunnel_status(TunnelStatus::Disconnected(tunnel_states::disconnected(None)));
                }
            }
            TunnelProcessConnectionState::Connected => {}
        }
    }

    fn handle_coordinator_output(&self, output: &ITunnelProcessOutput) {
        if output.mode != TunnelProcessMode::RemoteAccess && output.mode != TunnelProcessMode::Service {
            return;
        }
        let mut message = output.message.clone();
        if let Some(token) = self.current_session().and_then(|session| session.token).filter(|token| !token.is_empty()) {
            message = message.replace(&token, &"*".repeat(4));
        }
        if output.is_error {
            self.logger.error(&message);
        } else {
            self.logger.info(&message);
        }
        if !self.environment.is_built() && message.starts_with("   Compiling") {
            // Cargo compilation output is not emitted by the tunnel CLI.
            self.set_tunnel_status(TunnelStatus::Connecting(tunnel_states::connecting(Some(localize("remoteTunnelService.building", "Building CLI from sources", &[])))));
        }
    }

    fn handle_machine_status(&self, event: &ITunnelProcessMachineStatus) {
        // `service` mode also runs a session process, which attaches to the
        // installed service's singleton; its events are what move the public
        // status off `connecting`.
        if event.mode != TunnelProcessMode::RemoteAccess && event.mode != TunnelProcessMode::Service {
            return;
        }
        match &event.status {
            TunnelMachineStatus::Connected(connected) => {
                let info = ConnectionInfo {
                    link: connected.link.clone(),
                    domain: connected.link.as_ref().and(connected.domain.clone()),
                    tunnel_name: connected.tunnel_name.clone(),
                    tunnel_id: connected.tunnel_id.clone(),
                    is_attached: connected.is_attached,
                };
                let mut data = vec![("tunnelName", Dynamic::string(&info.tunnel_name)), ("isAttached", Dynamic::Bool(info.is_attached))];
                if let Some(correlation) = &event.correlation {
                    data.push(("tunnelSessionId", Dynamic::string(&correlation.session_id)));
                    data.push(("operationId", Dynamic::string(&correlation.operation_id)));
                }
                self.telemetry_service.public_log2("remoteTunnel.connected", Some(Dynamic::object(data)));
                let service_install_failed = self.tunnel_process_coordinator.get_status().service_install_failed;
                self.set_tunnel_status(TunnelStatus::Connected(tunnel_states::connected(info, service_install_failed)));
            }
            TunnelMachineStatus::TokenError(_) => {
                event.cancel();
                let session = self.current_session();
                self.on_did_token_failed.fire(session.clone());
                self.set_tunnel_status(TunnelStatus::Disconnected(tunnel_states::disconnected(session)));
            }
        }
    }

    fn set_tunnel_status(&self, tunnel_status: TunnelStatus) {
        *self.tunnel_status.borrow_mut() = tunnel_status.clone();
        self.on_did_change_tunnel_status.fire(tunnel_status.clone());
        self.tunnel_process_coordinator.set_remote_access_status(&tunnel_status);
    }

    /// A sessão do modo ativo, se houver.
    fn current_session(&self) -> Option<IRemoteTunnelSession> {
        match &*self.mode.borrow() {
            TunnelMode::Active(active) => Some(active.session.clone()),
            TunnelMode::Inactive(_) => None,
        }
    }

    fn restore_mode(&self) -> TunnelMode {
        let inactive = TunnelMode::Inactive(inactive_tunnel_mode());
        let tunnel_access_session = self.storage_service.get(TUNNEL_ACCESS_SESSION, StorageScope::Application, None);
        let as_service = self.storage_service.get_boolean(TUNNEL_ACCESS_IS_SERVICE, StorageScope::Application, Some(false)).unwrap_or(false);
        let Some(text) = tunnel_access_session.filter(|text| !text.is_empty()) else {
            return inactive;
        };

        let units: Vec<u16> = text.encode_utf16().collect();
        let mut errors = Vec::new();
        let options = ParseOptions { disallow_comments: true, allow_trailing_comma: false, allow_empty_content: false };
        let parsed = json::parse(&units, &mut errors, options).filter(|_| errors.is_empty());
        let Some(parsed) = parsed else {
            self.logger.error_args(
                &LogMessage::from("Problems restoring session from storage"),
                &[LogArg::Error(Error::named("SyntaxError", "Unexpected token in JSON"))],
            );
            return inactive;
        };

        if let Value::Object(session) = &parsed {
            let text_field = |key: &str| match session.get(key) {
                Some(Value::String(text)) => Some(text.to_string_lossy()),
                _ => None,
            };
            if let (Some(account_label), Some(session_id), Some(provider_id)) = (text_field("accountLabel"), text_field("sessionId"), text_field("providerId")) {
                return TunnelMode::Active(ActiveTunnelMode { session: IRemoteTunnelSession { provider_id, session_id, account_label, token: text_field("token") }, as_service });
            }
        }
        self.logger.error_args(&LogMessage::from("Problems restoring session from storage, invalid format"), &[LogArg::Value(from_json(&parsed))]);
        inactive
    }

    fn store_mode(&self, mode: &TunnelMode) {
        match mode {
            TunnelMode::Active(active) => {
                let session_without_token = Dynamic::object([
                    ("providerId", Dynamic::string(&active.session.provider_id)),
                    ("sessionId", Dynamic::string(&active.session.session_id)),
                    ("accountLabel", Dynamic::string(&active.session.account_label)),
                ]);
                let serialized = json_stringify(&session_without_token).unwrap_or_default();
                self.storage_service.store(TUNNEL_ACCESS_SESSION, StorageValue::String(serialized), StorageScope::Application, StorageTarget::Machine);
                self.storage_service.store(TUNNEL_ACCESS_IS_SERVICE, StorageValue::Bool(active.as_service), StorageScope::Application, StorageTarget::Machine);
            }
            TunnelMode::Inactive(_) => {
                self.storage_service.remove(TUNNEL_ACCESS_SESSION, StorageScope::Application);
                self.storage_service.remove(TUNNEL_ACCESS_IS_SERVICE, StorageScope::Application);
            }
        }
    }
}

impl IDisposable for RemoteTunnelService {
    fn dispose(&self) {
        self.base.dispose();
    }
}

impl IRemoteTunnelService for RemoteTunnelService {
    fn on_did_change_tunnel_status(&self) -> Event<TunnelStatus> {
        self.on_did_change_tunnel_status.event()
    }

    fn get_tunnel_status(&self) -> Promise<TunnelStatus> {
        Promise::resolved(self.tunnel_status.borrow().clone())
    }

    fn get_mode(&self) -> Promise<TunnelMode> {
        Promise::resolved(self.mode.borrow().clone())
    }

    fn on_did_change_mode(&self) -> Event<TunnelMode> {
        self.on_did_change_mode.event()
    }

    fn on_did_token_failed(&self) -> Event<Option<IRemoteTunnelSession>> {
        self.on_did_token_failed.event()
    }

    fn initialize(&self, mode: TunnelMode) -> Promise<TunnelStatus> {
        let this = self.this();
        async_fn(async move {
            if this.initialized.get() {
                return Ok(this.tunnel_status.borrow().clone());
            }
            this.initialized.set(true);
            this.set_mode(mode);
            this.trigger_update_tunnel_process_logging_errors().await?;
            Ok(this.tunnel_status.borrow().clone())
        })
    }

    fn start_tunnel(&self, mode: ActiveTunnelMode) -> Promise<TunnelStatus> {
        let this = self.this();
        async_fn(async move {
            let mode = TunnelMode::Active(mode);
            let is_connected_or_connecting = this.tunnel_status.borrow().type_() != "disconnected";
            if is_same_mode(&this.mode.borrow(), &mode) && is_connected_or_connecting {
                return Ok(this.tunnel_status.borrow().clone());
            }
            this.set_mode(mode);
            this.trigger_update_tunnel_process_logging_errors().await?;
            Ok(this.tunnel_status.borrow().clone())
        })
    }

    fn stop_tunnel(&self) -> Promise<()> {
        let this = self.this();
        async_fn(async move {
            let active = this.mode.borrow().active();
            if active {
                this.set_mode(TunnelMode::Inactive(inactive_tunnel_mode()));
            }
            let stopped = this.tunnel_process_coordinator.set_remote_access(TunnelMode::Inactive(inactive_tunnel_mode()), this.logger.get_level()).await;
            if let Err(error) = stopped {
                this.logger.error_args(&LogMessage::Error(error), &[]);
            }
            this.set_tunnel_status(TunnelStatus::Disconnected(tunnel_states::disconnected(None)));
            Ok(())
        })
    }

    fn get_tunnel_name(&self) -> Promise<Option<String>> {
        Promise::resolved(Some(self.tunnel_process_coordinator.get_intended_tunnel_name()))
    }
}

fn is_same_session(a1: Option<&IRemoteTunnelSession>, a2: Option<&IRemoteTunnelSession>) -> bool {
    match (a1, a2) {
        (Some(a1), Some(a2)) => a1.session_id == a2.session_id && a1.provider_id == a2.provider_id && a1.token == a2.token,
        (None, None) => true,
        _ => false,
    }
}

fn is_same_mode(a: &TunnelMode, b: &TunnelMode) -> bool {
    match (a, b) {
        (TunnelMode::Active(a), TunnelMode::Active(b)) => a.as_service == b.as_service && is_same_session(Some(&a.session), Some(&b.session)),
        (TunnelMode::Inactive(_), TunnelMode::Inactive(_)) => true,
        _ => false,
    }
}
