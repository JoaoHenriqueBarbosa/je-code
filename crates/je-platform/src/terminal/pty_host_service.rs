//! Porte de `src/vs/platform/terminal/node/ptyHostService.ts`: o serviço que lança o pty host, encaminha as chamadas e
//! eventos para ele e cuida da conexão (batimento, reinício, resposta a variáveis a resolver).
//!
//! Representação:
//! - `extends Disposable implements IPtyHostService` é um [`Disposable`] por composição mais os traits [`IPtyService`] e
//!   [`IPtyHostController`] (ou seja, [`IPtyHostService`]). O serviço guarda um `Weak` de si mesmo (`Rc::new_cyclic`) para
//!   os ouvintes e os timers, como o original captura `this`.
//! - `ProxyChannel.toService<IPtyService>(channel)` é o [`ServiceProxy`] do canal: cada método do contrato escreve os
//!   argumentos e lê o resultado com [`Wire`]. O proxy de um evento (`proxy.onProcessData(listener)`) é
//!   `proxy.event("onProcessData")`, lido com [`Wire`] a cada disparo; um valor que não decodifica vai para o
//!   `onUnexpectedError`.
//! - O serviço também é um [`IProxyService`] (por nome, como o objeto que o `ProxyChannel.fromService` do
//!   `createLocalPtyChannel` percorre): lê os argumentos, chama o método tipado e escreve o resultado. O que o
//!   original não tem (`refreshIgnoreProcessNames`, que só existe no proxy) não existe aqui.
//! - `process.env` é o ambiente do processo (`SafeProcess::native().env`) e `OS` é `Platform::detect().os`.
//! - `getProfiles` roda `detectAvailableProfiles` com o `RequestStore` como resolvedor de variáveis. O resolvedor devolve
//!   a lista de volta; uma requisição que dá tempo esgotado (a promessa rejeitada) devolve a lista vazia, porque o
//!   `VariableResolver` do porte não carrega erro.
//! - `this._logService.getLevel() === LogLevel.Trace` com `new Error().stack` é o `Backtrace` do Rust.
//! - O `if (!this._proxy.freePortKillProcess) throw` de `freePortKillProcess` não existe: o método é obrigatório no
//!   contrato tipado.
//! - `IPtyHostStarter.onRequestConnection?` e `onWillShutdown?` são `Option<Event<()>>`.

use std::cell::{Cell, RefCell};
use std::rc::{Rc, Weak};

use je_base::async_::async_fn;
use je_base::dynamic::Dynamic;
use je_base::errors::{self, Error};
use je_base::event::{Emitter, Event};
use je_base::lifecycle::{Disposable, DisposableRef, DisposableStore, IDisposable, to_disposable};
use je_base::marshalling::Marshalled;
use je_base::node::pfs::LocalFuture;
use je_base::node::shell::get_system_shell;
use je_base::parts::ipc::ProxyChannel::{IProxyService, ServiceProxy, to_service};
use je_base::performance::PerformanceMark;
use je_base::platform::{OperatingSystem, Platform};
use je_base::process::{ProcessEnvironment, SafeProcess};
use je_base::promise::Promise;
use je_base::runtime::{self, TimerId};
use je_base::stopwatch::StopWatch;

use super::node_terminal_profiles::detect_available_profiles;
use super::pty_host::IPtyHostStarter;
use super::pty_service::{
    HeartbeatConstants, IGetTerminalLayoutInfoArgs, IPtyHostController, IPtyHostLatencyMeasurement, IPtyService, IRequestResolveVariablesEvent, ISerializedTerminalState,
    ISetTerminalLayoutInfoArgs, IProcessDetails, ITerminalsLayoutInfo, PtyDidChangeProperty, PtyDidRequestDetach, PtyProcessData, PtyProcessExit, PtyProcessOrphanQuestion, PtyProcessReady,
    PtyProcessReplay, TerminalIpcChannels, WslPathDirection,
};
use super::pty_service_wire::Wire;
use super::request_store::RequestStore;
use super::terminal::{
    IFreedPortProcess, IProcessProperty, IShellLaunchConfig, ITerminalLaunchOutcome, ITerminalProcessOptions, ITerminalProfile, ProcessPropertyType, TerminalIcon, TitleEventSource,
    UnicodeVersion,
};
use super::terminal_platform_configuration::register_terminal_platform_configuration;
use super::terminal_settings::TerminalSettingId;
use crate::configuration::service::IConfigurationService;
use crate::environment::native_parsed_args::NativeParsedArgs;
use crate::log::log_ipc::RemoteLoggerChannelClient;
use crate::log::{LogArg, LogLevel, LogMessage, LogServiceRef, LoggerService};
use crate::shell::shell_env::get_resolved_shell_env;

const MAX_RESTARTS: u32 = 5;

/// `{ workspaceId: string; originalText: string[] }`: os argumentos do `RequestStore` de variáveis a resolver.
struct ResolveVariablesRequest {
    workspace_id: String,
    original_text: Vec<String>,
}

fn emitter<T: 'static>(base: &Disposable) -> Rc<Emitter<T>> {
    let emitter = Emitter::new();
    base.register(emitter.clone());
    emitter
}

/// `proxy.onX(e => this._onX.fire(e))`: o evento do canal, lido com [`Wire`], reemitido no emissor.
fn forward_event<T: Wire + 'static>(event: Event<Marshalled>, emitter: Rc<Emitter<T>>) -> DisposableRef {
    event.listen(move |value| match T::from_wire(value) {
        Ok(event) => emitter.fire(event),
        Err(error) => errors::on_unexpected_error(&error),
    })
}

/// This service implements IPtyService by launching a pty host process, forwarding messages to and
/// from the pty host process and manages the connection.
pub struct PtyHostService {
    me: Weak<Self>,
    base: Rc<Disposable>,
    pty_host_starter: Rc<dyn IPtyHostStarter>,
    configuration_service: Rc<dyn IConfigurationService>,
    log_service: LogServiceRef,
    logger_service: Rc<dyn LoggerService>,

    connection: RefCell<Option<super::pty_host::IPtyHostConnection>>,
    /// ProxyChannel is not used here because events get lost when forwarding across multiple proxies
    proxy: RefCell<Option<Rc<ServiceProxy>>>,

    resolve_variables_request_store: Rc<RequestStore<Vec<String>, ResolveVariablesRequest>>,
    was_quit_requested: Cell<bool>,
    restart_count: Cell<u32>,
    is_responsive: Cell<bool>,
    heartbeat_first_timeout: Cell<Option<TimerId>>,
    heartbeat_second_timeout: Cell<Option<TimerId>>,

    on_pty_host_exit: Rc<Emitter<i32>>,
    on_pty_host_start: Rc<Emitter<()>>,
    on_pty_host_unresponsive: Rc<Emitter<()>>,
    on_pty_host_responsive: Rc<Emitter<()>>,
    on_pty_host_request_resolve_variables: Rc<Emitter<IRequestResolveVariablesEvent>>,

    on_process_data: Rc<Emitter<PtyProcessData>>,
    on_process_ready: Rc<Emitter<PtyProcessReady>>,
    on_process_replay: Rc<Emitter<PtyProcessReplay>>,
    on_process_orphan_question: Rc<Emitter<PtyProcessOrphanQuestion>>,
    on_did_request_detach: Rc<Emitter<PtyDidRequestDetach>>,
    on_did_change_property: Rc<Emitter<PtyDidChangeProperty>>,
    on_process_exit: Rc<Emitter<PtyProcessExit>>,

    pty_host_store: Rc<DisposableStore>,
}

impl PtyHostService {
    pub fn new(
        pty_host_starter: Rc<dyn IPtyHostStarter>,
        configuration_service: Rc<dyn IConfigurationService>,
        log_service: LogServiceRef,
        logger_service: Rc<dyn LoggerService>,
    ) -> Rc<Self> {
        let base = Disposable::new();
        let on_pty_host_exit = emitter::<i32>(&base);
        let on_pty_host_start = emitter::<()>(&base);
        let on_pty_host_unresponsive = emitter::<()>(&base);
        let on_pty_host_responsive = emitter::<()>(&base);
        let on_pty_host_request_resolve_variables = emitter::<IRequestResolveVariablesEvent>(&base);
        let on_process_data = emitter::<PtyProcessData>(&base);
        let on_process_ready = emitter::<PtyProcessReady>(&base);
        let on_process_replay = emitter::<PtyProcessReplay>(&base);
        let on_process_orphan_question = emitter::<PtyProcessOrphanQuestion>(&base);
        let on_did_request_detach = emitter::<PtyDidRequestDetach>(&base);
        let on_did_change_property = emitter::<PtyDidChangeProperty>(&base);
        let on_process_exit = emitter::<PtyProcessExit>(&base);
        let pty_host_store = DisposableStore::new();
        base.register(pty_host_store.clone());

        // Platform configuration is required on the process running the pty host (shared process or
        // remote server).
        register_terminal_platform_configuration();

        base.register(pty_host_starter.clone());

        let resolve_variables_request_store = RequestStore::<Vec<String>, ResolveVariablesRequest>::new(None, log_service.clone());
        base.register(resolve_variables_request_store.clone());
        base.register(resolve_variables_request_store.on_create_request().listen({
            let emitter = on_pty_host_request_resolve_variables.clone();
            move |event| {
                emitter.fire(IRequestResolveVariablesEvent {
                    request_id: event.request_id,
                    workspace_id: event.args.workspace_id.clone(),
                    original_text: event.args.original_text.clone(),
                });
            }
        }));

        let this = Rc::new_cyclic(|me: &Weak<Self>| Self {
            me: me.clone(),
            base,
            pty_host_starter,
            configuration_service,
            log_service,
            logger_service,
            connection: RefCell::new(None),
            proxy: RefCell::new(None),
            resolve_variables_request_store,
            was_quit_requested: Cell::new(false),
            restart_count: Cell::new(0),
            is_responsive: Cell::new(true),
            heartbeat_first_timeout: Cell::new(None),
            heartbeat_second_timeout: Cell::new(None),
            on_pty_host_exit,
            on_pty_host_start,
            on_pty_host_unresponsive,
            on_pty_host_responsive,
            on_pty_host_request_resolve_variables,
            on_process_data,
            on_process_ready,
            on_process_replay,
            on_process_orphan_question,
            on_did_request_detach,
            on_did_change_property,
            on_process_exit,
            pty_host_store,
        });

        this.base.register(to_disposable({
            let me = this.me.clone();
            move || {
                if let Some(this) = me.upgrade() {
                    this.dispose_pty_host();
                }
            }
        }));

        // Start the pty host when a window requests a connection, if the starter has that capability.
        if let Some(on_request_connection) = this.pty_host_starter.on_request_connection() {
            let me = this.me.clone();
            this.base.register(on_request_connection.once().listen(move |()| {
                if let Some(this) = me.upgrade() {
                    this.ensure_pty_host();
                }
            }));
        }

        if let Some(on_will_shutdown) = this.pty_host_starter.on_will_shutdown() {
            let me = this.me.clone();
            this.base.register(on_will_shutdown.listen(move |()| {
                if let Some(this) = me.upgrade() {
                    this.was_quit_requested.set(true);
                }
            }));
        }

        this
    }

    /// `getValue<string[]>(TerminalSettingId.IgnoreProcessNames)`.
    fn ignore_process_names(&self) -> Vec<String> {
        match self.configuration_service.get_value(Some(TerminalSettingId::IgnoreProcessNames.as_str()), None) {
            Dynamic::Array(items) => items
                .to_vec()
                .into_iter()
                .filter_map(|item| match item {
                    Dynamic::String(text) => Some(text.to_string_lossy()),
                    _ => None,
                })
                .collect(),
            _ => Vec::new(),
        }
    }

    /// `this._optionalProxy?.refreshIgnoreProcessNames?.(this._ignoreProcessNames)`.
    fn refresh_ignore_process_names_on_host(&self) -> Option<Promise<()>> {
        let proxy = self.optional_proxy()?;
        Some(Self::call_proxy(&proxy, "refreshIgnoreProcessNames", vec![self.ignore_process_names().to_wire()]))
    }

    /// `process.env` quando o shell não é resolvido (Windows), senão o ambiente do shell de login. Nunca rejeita: o erro é
    /// registrado e o resultado é o ambiente vazio.
    fn resolve_shell_env(&self) -> Promise<ProcessEnvironment> {
        let process_env = SafeProcess::native().env;
        if Platform::detect().is_windows {
            return Promise::resolved(process_env);
        }

        let log_service = self.log_service.clone();
        get_resolved_shell_env(&self.configuration_service, &self.log_service, &NativeParsedArgs::default(), &process_env).then_result(move |result| match result {
            Ok(env) => Ok(env),
            Err(error) => {
                log_service.error_args(&LogMessage::from("ptyHost was unable to resolve shell environment"), &[LogArg::from(&error)]);
                Ok(ProcessEnvironment::new())
            }
        })
    }

    fn start_pty_host(&self) {
        let connection = self.pty_host_starter.start();
        let client = connection.client.clone();
        let store = self.pty_host_store.clone();
        // Transfer ownership of the per-host connection store so it is disposed together with the listeners below on the next restart.
        store.add(connection.store.clone());

        // Log a full stack trace which will tell the exact reason the pty host is starting up
        if self.log_service.get_level() == LogLevel::Trace {
            self.log_service.trace(&format!("PtyHostService#_startPtyHost {}", std::backtrace::Backtrace::force_capture()));
        }

        // Setup heartbeat service and trigger a heartbeat immediately to reset the timeouts
        let heartbeat_service = to_service(client.get_channel(TerminalIpcChannels::Heartbeat.as_str()), None);
        store.add(heartbeat_service.event("onBeat").listen({
            let me = self.me.clone();
            move |_| {
                if let Some(this) = me.upgrade() {
                    this.handle_heartbeat(false);
                }
            }
        }));
        self.handle_heartbeat(true);

        // Handle exit
        store.add(connection.on_did_process_exit.listen({
            let me = self.me.clone();
            move |exit| {
                let Some(this) = me.upgrade() else { return };
                this.on_pty_host_exit.fire(exit.code);
                if !this.was_quit_requested.get() && !this.base.store().is_disposed() {
                    if this.restart_count.get() <= MAX_RESTARTS {
                        this.log_service.error(&format!("ptyHost terminated unexpectedly with code {}", exit.code));
                        this.restart_count.set(this.restart_count.get() + 1);
                        let _ = this.restart_pty_host();
                    } else {
                        this.log_service.error(&format!("ptyHost terminated unexpectedly with code {}, giving up", exit.code));
                    }
                }
            }
        }));

        // Create proxy and forward events
        let proxy = Rc::new(to_service(client.get_channel(TerminalIpcChannels::PtyHost.as_str()), None));
        store.add(forward_event(proxy.event("onProcessData"), self.on_process_data.clone()));
        store.add(forward_event(proxy.event("onProcessReady"), self.on_process_ready.clone()));
        store.add(forward_event(proxy.event("onProcessExit"), self.on_process_exit.clone()));
        store.add(forward_event(proxy.event("onDidChangeProperty"), self.on_did_change_property.clone()));
        store.add(forward_event(proxy.event("onProcessReplay"), self.on_process_replay.clone()));
        store.add(forward_event(proxy.event("onProcessOrphanQuestion"), self.on_process_orphan_question.clone()));
        store.add(forward_event(proxy.event("onDidRequestDetach"), self.on_did_request_detach.clone()));

        store.add_rc(RemoteLoggerChannelClient::new(self.logger_service.clone(), client.get_channel(TerminalIpcChannels::Logger.as_str())));

        *self.connection.borrow_mut() = Some(connection);
        *self.proxy.borrow_mut() = Some(proxy);

        self.on_pty_host_start.fire(());

        store.add(self.configuration_service.on_did_change_configuration().listen({
            let me = self.me.clone();
            move |event| {
                if event.affects_configuration(TerminalSettingId::IgnoreProcessNames.as_str(), None)
                    && let Some(this) = me.upgrade()
                {
                    let _ = this.refresh_ignore_process_names_on_host();
                }
            }
        }));
        let _ = self.refresh_ignore_process_names_on_host();
    }

    fn ensure_pty_host(&self) {
        let started = self.connection.borrow().is_some();
        if !started {
            self.start_pty_host();
        }
    }

    fn proxy(&self) -> Rc<ServiceProxy> {
        self.ensure_pty_host();
        self.proxy.borrow().clone().expect("o proxy existe depois de ensure_pty_host")
    }

    /// Get the proxy if it exists, otherwise `None`. This is used when calls are not needed to be
    /// passed through to the pty host if it has not yet been spawned.
    fn optional_proxy(&self) -> Option<Rc<ServiceProxy>> {
        self.proxy.borrow().clone()
    }

    /// `await proxy.method(...args)`, com o resultado lido como `R`.
    fn call_proxy<R: Wire + Clone + 'static>(proxy: &ServiceProxy, method: &str, args: Vec<Marshalled>) -> Promise<R> {
        proxy.call(method, args).then_result(|result| result.and_then(|value| R::from_wire(&value)))
    }

    /// `this._proxy.method(...args)`.
    fn forward<R: Wire + Clone + 'static>(&self, method: &str, args: Vec<Marshalled>) -> Promise<R> {
        Self::call_proxy(&self.proxy(), method, args)
    }

    /// `this._optionalProxy?.method(...args)`: sem proxy, o `None` do original é o `fallback`.
    fn forward_optional<R: Wire + Clone + 'static>(&self, method: &str, args: Vec<Marshalled>, fallback: R) -> Promise<R> {
        match self.optional_proxy() {
            Some(proxy) => Self::call_proxy(&proxy, method, args),
            None => Promise::resolved(fallback),
        }
    }

    fn dispose_pty_host(&self) {
        // Heartbeat timers are bare setTimeout handles, not disposables in the store, so they need an explicit clear.
        // shutdownAll() is fired before clearing the store so any in-flight exit listener still has a live proxy to read from;
        // the per-host listener store is cleared last so the on-exit signal isn't dropped on the floor.
        self.clear_heartbeat_timeouts();
        // Fire-and-forget: the IPC channel may already be gone; swallow rejections so we don't surface an unhandled promise.
        if let Some(proxy) = self.optional_proxy() {
            proxy.call("shutdownAll", Vec::new()).catch(|_| Marshalled::Undefined);
        }
        *self.connection.borrow_mut() = None;
        *self.proxy.borrow_mut() = None;
        self.pty_host_store.clear();
    }

    fn handle_heartbeat(&self, is_connecting: bool) {
        self.clear_heartbeat_timeouts();
        let delay = if is_connecting { HeartbeatConstants::CONNECTING_BEAT_INTERVAL } else { (HeartbeatConstants::BEAT_INTERVAL as f64 * HeartbeatConstants::FIRST_WAIT_MULTIPLIER) as u64 };
        let me = self.me.clone();
        self.heartbeat_first_timeout.set(Some(runtime::set_timeout(
            move || {
                if let Some(this) = me.upgrade() {
                    this.handle_heartbeat_first_timeout();
                }
            },
            delay,
        )));
        if !self.is_responsive.get() {
            self.is_responsive.set(true);
            self.on_pty_host_responsive.fire(());
        }
    }

    fn handle_heartbeat_first_timeout(&self) {
        self.log_service.warn(&format!("No ptyHost heartbeat after {} seconds", HeartbeatConstants::BEAT_INTERVAL as f64 * HeartbeatConstants::FIRST_WAIT_MULTIPLIER / 1000.0));
        self.heartbeat_first_timeout.set(None);
        let me = self.me.clone();
        self.heartbeat_second_timeout.set(Some(runtime::set_timeout(
            move || {
                if let Some(this) = me.upgrade() {
                    this.handle_heartbeat_second_timeout();
                }
            },
            (HeartbeatConstants::BEAT_INTERVAL as f64 * HeartbeatConstants::SECOND_WAIT_MULTIPLIER) as u64,
        )));
    }

    fn handle_heartbeat_second_timeout(&self) {
        let seconds = (HeartbeatConstants::BEAT_INTERVAL as f64 * HeartbeatConstants::FIRST_WAIT_MULTIPLIER + HeartbeatConstants::BEAT_INTERVAL as f64 * HeartbeatConstants::FIRST_WAIT_MULTIPLIER) / 1000.0;
        self.log_service.error(&format!("No ptyHost heartbeat after {seconds} seconds"));
        self.heartbeat_second_timeout.set(None);
        if self.is_responsive.get() {
            self.is_responsive.set(false);
            self.on_pty_host_unresponsive.fire(());
        }
    }

    fn handle_unresponsive_create_process(&self) {
        self.clear_heartbeat_timeouts();
        self.log_service.error(&format!("No ptyHost response to createProcess after {} seconds", HeartbeatConstants::CREATE_PROCESS_TIMEOUT as f64 / 1000.0));
        if self.is_responsive.get() {
            self.is_responsive.set(false);
            self.on_pty_host_unresponsive.fire(());
        }
    }

    fn clear_heartbeat_timeouts(&self) {
        if let Some(timeout) = self.heartbeat_first_timeout.take() {
            runtime::clear_timeout(timeout);
        }
        if let Some(timeout) = self.heartbeat_second_timeout.take() {
            runtime::clear_timeout(timeout);
        }
    }
}

impl IDisposable for PtyHostService {
    fn dispose(&self) {
        self.base.dispose();
    }
}

impl IPtyService for PtyHostService {
    fn on_process_data(&self) -> Event<PtyProcessData> {
        self.on_process_data.event()
    }

    fn on_process_ready(&self) -> Event<PtyProcessReady> {
        self.on_process_ready.event()
    }

    fn on_process_replay(&self) -> Event<PtyProcessReplay> {
        self.on_process_replay.event()
    }

    fn on_process_orphan_question(&self) -> Event<PtyProcessOrphanQuestion> {
        self.on_process_orphan_question.event()
    }

    fn on_did_request_detach(&self) -> Event<PtyDidRequestDetach> {
        self.on_did_request_detach.event()
    }

    fn on_did_change_property(&self) -> Event<PtyDidChangeProperty> {
        self.on_did_change_property.event()
    }

    fn on_process_exit(&self) -> Event<PtyProcessExit> {
        self.on_process_exit.event()
    }

    fn create_process(
        &self,
        shell_launch_config: &IShellLaunchConfig,
        cwd: &str,
        cols: i32,
        rows: i32,
        unicode_version: UnicodeVersion,
        env: &ProcessEnvironment,
        executable_env: &ProcessEnvironment,
        options: &ITerminalProcessOptions,
        should_persist: bool,
        workspace_id: &str,
        workspace_name: &str,
    ) -> Promise<u32> {
        let me = self.me.clone();
        let timeout = runtime::set_timeout(
            move || {
                if let Some(this) = me.upgrade() {
                    this.handle_unresponsive_create_process();
                }
            },
            HeartbeatConstants::CREATE_PROCESS_TIMEOUT,
        );
        self.forward::<u32>(
            "createProcess",
            vec![
                shell_launch_config.to_wire(),
                cwd.to_owned().to_wire(),
                cols.to_wire(),
                rows.to_wire(),
                unicode_version.to_wire(),
                env.to_wire(),
                executable_env.to_wire(),
                options.to_wire(),
                should_persist.to_wire(),
                workspace_id.to_owned().to_wire(),
                workspace_name.to_owned().to_wire(),
            ],
        )
        .then(move |id| {
            runtime::clear_timeout(timeout);
            id
        })
    }

    fn update_title(&self, id: u32, title: &str, title_source: TitleEventSource) -> Promise<()> {
        self.forward("updateTitle", vec![id.to_wire(), title.to_owned().to_wire(), title_source.to_wire()])
    }

    fn update_icon(&self, id: u32, user_initiated: bool, icon: &TerminalIcon, color: Option<&str>) -> Promise<()> {
        self.forward("updateIcon", vec![id.to_wire(), user_initiated.to_wire(), icon.to_wire(), color.map(str::to_owned).to_wire()])
    }

    fn attach_to_process(&self, id: u32, pid: Option<i32>) -> Promise<()> {
        self.forward("attachToProcess", vec![id.to_wire(), pid.to_wire()])
    }

    fn detach_from_process(&self, id: u32, force_persist: Option<bool>) -> Promise<()> {
        self.forward("detachFromProcess", vec![id.to_wire(), force_persist.to_wire()])
    }

    fn shutdown_all(&self) -> Promise<()> {
        self.forward("shutdownAll", Vec::new())
    }

    fn list_processes(&self) -> Promise<Vec<IProcessDetails>> {
        self.forward("listProcesses", Vec::new())
    }

    fn get_performance_marks(&self) -> Promise<Vec<PerformanceMark>> {
        self.forward_optional("getPerformanceMarks", Vec::new(), Vec::new())
    }

    fn reduce_connection_grace_time(&self) -> Promise<()> {
        self.forward_optional("reduceConnectionGraceTime", Vec::new(), ())
    }

    fn start(&self, id: u32) -> Promise<Option<ITerminalLaunchOutcome>> {
        self.forward("start", vec![id.to_wire()])
    }

    fn shutdown(&self, id: u32, immediate: bool) -> Promise<()> {
        self.forward("shutdown", vec![id.to_wire(), immediate.to_wire()])
    }

    fn input(&self, id: u32, data: &str) -> Promise<()> {
        self.forward("input", vec![id.to_wire(), data.to_owned().to_wire()])
    }

    fn send_signal(&self, id: u32, signal: &str) -> Promise<()> {
        self.forward("sendSignal", vec![id.to_wire(), signal.to_owned().to_wire()])
    }

    fn process_binary(&self, id: u32, data: &str) -> Promise<()> {
        self.forward("processBinary", vec![id.to_wire(), data.to_owned().to_wire()])
    }

    fn resize(&self, id: u32, cols: i32, rows: i32, pixel_width: Option<i32>, pixel_height: Option<i32>) -> Promise<()> {
        self.forward("resize", vec![id.to_wire(), cols.to_wire(), rows.to_wire(), pixel_width.to_wire(), pixel_height.to_wire()])
    }

    fn clear_buffer(&self, id: u32) -> Promise<()> {
        self.forward("clearBuffer", vec![id.to_wire()])
    }

    fn acknowledge_data_event(&self, id: u32, char_count: usize) -> Promise<()> {
        self.forward("acknowledgeDataEvent", vec![id.to_wire(), char_count.to_wire()])
    }

    fn set_unicode_version(&self, id: u32, version: UnicodeVersion) -> Promise<()> {
        self.forward("setUnicodeVersion", vec![id.to_wire(), version.to_wire()])
    }

    fn set_next_command_id(&self, id: u32, command_line: &str, command_id: &str) -> Promise<()> {
        self.forward("setNextCommandId", vec![id.to_wire(), command_line.to_owned().to_wire(), command_id.to_owned().to_wire()])
    }

    fn get_initial_cwd(&self, id: u32) -> Promise<String> {
        self.forward("getInitialCwd", vec![id.to_wire()])
    }

    fn get_cwd(&self, id: u32) -> Promise<String> {
        self.forward("getCwd", vec![id.to_wire()])
    }

    fn get_latency(&self) -> Promise<Vec<IPtyHostLatencyMeasurement>> {
        let mut stop_watch = StopWatch::create(None);
        self.forward::<Vec<IPtyHostLatencyMeasurement>>("getLatency", Vec::new()).then(move |results| {
            stop_watch.stop();
            let mut measurements = vec![IPtyHostLatencyMeasurement { label: "ptyhostservice<->ptyhost".to_owned(), latency: stop_watch.elapsed() }];
            measurements.extend(results);
            measurements
        })
    }

    fn orphan_question_reply(&self, id: u32) -> Promise<()> {
        self.forward("orphanQuestionReply", vec![id.to_wire()])
    }

    fn install_auto_reply(&self, match_: &str, reply: &str) -> Promise<()> {
        self.forward("installAutoReply", vec![match_.to_owned().to_wire(), reply.to_owned().to_wire()])
    }

    fn uninstall_all_auto_replies(&self) -> Promise<()> {
        self.forward("uninstallAllAutoReplies", Vec::new())
    }

    fn get_default_system_shell(&self, os_override: Option<OperatingSystem>) -> Promise<String> {
        match self.optional_proxy() {
            Some(proxy) => Self::call_proxy(&proxy, "getDefaultSystemShell", vec![os_override.to_wire()]),
            None => async_fn(async move { Ok(get_system_shell(os_override.unwrap_or_else(|| Platform::detect().os), &SafeProcess::native().env).await) }),
        }
    }

    fn get_environment(&self) -> Promise<ProcessEnvironment> {
        // If the pty host is yet to be launched, just return the environment of this process as it
        // is essentially the same when used to evaluate terminal profiles.
        match self.optional_proxy() {
            None => Promise::resolved(SafeProcess::native().env),
            Some(proxy) => Self::call_proxy(&proxy, "getEnvironment", Vec::new()),
        }
    }

    fn get_wsl_path(&self, original: &str, direction: WslPathDirection) -> Promise<String> {
        self.forward("getWslPath", vec![original.to_owned().to_wire(), direction.to_wire()])
    }

    fn get_revived_pty_new_id(&self, workspace_id: &str, id: u32) -> Promise<Option<u32>> {
        self.forward("getRevivedPtyNewId", vec![workspace_id.to_owned().to_wire(), id.to_wire()])
    }

    fn set_terminal_layout_info(&self, args: &ISetTerminalLayoutInfoArgs) -> Promise<()> {
        self.forward("setTerminalLayoutInfo", vec![args.to_wire()])
    }

    fn get_terminal_layout_info(&self, args: &IGetTerminalLayoutInfoArgs) -> Promise<Option<ITerminalsLayoutInfo>> {
        // This is optional as we want reconnect requests to go through only if the pty host exists.
        // Revive is handled specially as reviveTerminalProcesses is guaranteed to be called before
        // the request for layout info.
        self.forward_optional("getTerminalLayoutInfo", vec![args.to_wire()], None)
    }

    fn request_detach_instance(&self, workspace_id: &str, instance_id: u32) -> Promise<Option<IProcessDetails>> {
        self.forward("requestDetachInstance", vec![workspace_id.to_owned().to_wire(), instance_id.to_wire()])
    }

    fn accept_detach_instance_reply(&self, request_id: u32, persistent_process_id: Option<u32>) -> Promise<()> {
        self.forward("acceptDetachInstanceReply", vec![request_id.to_wire(), persistent_process_id.to_wire()])
    }

    fn free_port_kill_process(&self, port: &str) -> Promise<IFreedPortProcess> {
        self.forward("freePortKillProcess", vec![port.to_owned().to_wire()])
    }

    fn serialize_terminal_state(&self, ids: &[u32]) -> Promise<String> {
        self.forward("serializeTerminalState", vec![ids.to_vec().to_wire()])
    }

    fn revive_terminal_processes(&self, workspace_id: &str, state: &[ISerializedTerminalState], date_time_format_locate: &str) -> Promise<()> {
        self.forward("reviveTerminalProcesses", vec![workspace_id.to_owned().to_wire(), state.to_vec().to_wire(), date_time_format_locate.to_owned().to_wire()])
    }

    fn refresh_property(&self, id: u32, property: ProcessPropertyType) -> Promise<IProcessProperty> {
        self.forward("refreshProperty", vec![id.to_wire(), property.to_wire()])
    }

    fn update_property(&self, id: u32, property: &IProcessProperty) -> Promise<()> {
        // `updateProperty(id, property, value)`: o tipo e o valor vão como dois argumentos.
        let wire = property.to_wire();
        let value = match &wire {
            Marshalled::Object(entries) => entries.iter().find(|(key, _)| *key == "value").map_or(Marshalled::Undefined, |(_, value)| value.clone()),
            _ => Marshalled::Undefined,
        };
        self.forward("updateProperty", vec![id.to_wire(), property.property_type().to_wire(), value])
    }
}

impl IPtyHostController for PtyHostService {
    fn on_pty_host_exit(&self) -> Event<i32> {
        self.on_pty_host_exit.event()
    }

    fn on_pty_host_start(&self) -> Event<()> {
        self.on_pty_host_start.event()
    }

    fn on_pty_host_unresponsive(&self) -> Event<()> {
        self.on_pty_host_unresponsive.event()
    }

    fn on_pty_host_responsive(&self) -> Event<()> {
        self.on_pty_host_responsive.event()
    }

    fn on_pty_host_request_resolve_variables(&self) -> Event<IRequestResolveVariablesEvent> {
        self.on_pty_host_request_resolve_variables.event()
    }

    fn restart_pty_host(&self) -> Promise<()> {
        self.dispose_pty_host();
        self.is_responsive.set(true);
        self.start_pty_host();
        Promise::resolved(())
    }

    fn accept_pty_host_resolved_variables(&self, request_id: u64, resolved: Vec<String>) -> Promise<()> {
        self.resolve_variables_request_store.accept_reply(request_id, resolved);
        Promise::resolved(())
    }

    fn get_profiles(&self, workspace_id: &str, profiles: &Dynamic, default_profile: &Dynamic, include_detected_profiles: bool) -> Promise<Vec<ITerminalProfile>> {
        let (profiles, default_profile) = (profiles.clone(), default_profile.clone());
        let configuration_service = self.configuration_service.clone();
        let log_service = self.log_service.clone();
        let request_store = self.resolve_variables_request_store.clone();
        let workspace_id = workspace_id.to_owned();
        let shell_env = self.resolve_shell_env();
        async_fn(async move {
            let shell_env = shell_env.await?;
            let resolve_variables = move |text: Vec<String>| -> LocalFuture<'static, Vec<String>> {
                let promise = request_store.create_request(ResolveVariablesRequest { workspace_id: workspace_id.clone(), original_text: text });
                Box::pin(async move { promise.await.unwrap_or_default() })
            };
            let variable_resolver: &dyn Fn(Vec<String>) -> LocalFuture<'static, Vec<String>> = &resolve_variables;
            let defined = |value: &Dynamic| (!matches!(value, Dynamic::Undefined)).then_some(value.clone());
            let (profiles, default_profile) = (defined(&profiles), defined(&default_profile));
            Ok(detect_available_profiles(
                profiles.as_ref(),
                default_profile.as_ref(),
                include_detected_profiles,
                &*configuration_service,
                &shell_env,
                None,
                Some(&*log_service),
                Some(variable_resolver),
                None,
            )
            .await)
        })
    }
}

/// `args[index]`, `undefined` quando falta.
fn arg<T: Wire>(args: &[Marshalled], index: usize) -> Result<T, Error> {
    const UNDEFINED: Marshalled = Marshalled::Undefined;
    T::from_wire(args.get(index).unwrap_or(&UNDEFINED))
}

fn reply<R: Wire + Clone + 'static>(promise: Promise<R>) -> Promise<Marshalled> {
    promise.then(|value| value.to_wire())
}

/// Uma chamada de `IProxyService::call`: o corpo lê os argumentos com `?` e devolve a promessa tipada; um argumento que não
/// decodifica rejeita a chamada.
macro_rules! invoke {
    ($body:expr) => {
        Some((|| -> Result<Promise<Marshalled>, Error> { Ok(reply($body)) })().unwrap_or_else(Promise::rejected))
    };
}

impl IProxyService for PtyHostService {
    fn event_names(&self) -> Vec<String> {
        [
            "onProcessData",
            "onProcessReady",
            "onProcessReplay",
            "onProcessOrphanQuestion",
            "onDidRequestDetach",
            "onDidChangeProperty",
            "onProcessExit",
            "onPtyHostExit",
            "onPtyHostStart",
            "onPtyHostUnresponsive",
            "onPtyHostResponsive",
            "onPtyHostRequestResolveVariables",
        ]
        .map(str::to_owned)
        .to_vec()
    }

    fn event(&self, name: &str) -> Option<Event<Marshalled>> {
        Some(match name {
            "onProcessData" => IPtyService::on_process_data(self).map(|event| event.to_wire(), None),
            "onProcessReady" => IPtyService::on_process_ready(self).map(|event| event.to_wire(), None),
            "onProcessReplay" => IPtyService::on_process_replay(self).map(|event| event.to_wire(), None),
            "onProcessOrphanQuestion" => IPtyService::on_process_orphan_question(self).map(|event| event.to_wire(), None),
            "onDidRequestDetach" => IPtyService::on_did_request_detach(self).map(|event| event.to_wire(), None),
            "onDidChangeProperty" => IPtyService::on_did_change_property(self).map(|event| event.to_wire(), None),
            "onProcessExit" => IPtyService::on_process_exit(self).map(|event| event.to_wire(), None),
            "onPtyHostExit" => IPtyHostController::on_pty_host_exit(self).map(|code| code.to_wire(), None),
            "onPtyHostStart" => IPtyHostController::on_pty_host_start(self).map(|()| Marshalled::Undefined, None),
            "onPtyHostUnresponsive" => IPtyHostController::on_pty_host_unresponsive(self).map(|()| Marshalled::Undefined, None),
            "onPtyHostResponsive" => IPtyHostController::on_pty_host_responsive(self).map(|()| Marshalled::Undefined, None),
            "onPtyHostRequestResolveVariables" => IPtyHostController::on_pty_host_request_resolve_variables(self).map(|event| event.to_wire(), None),
            _ => return None,
        })
    }

    fn dynamic_event(&self, _name: &str, _arg: Marshalled) -> Option<Event<Marshalled>> {
        None
    }

    fn call(&self, name: &str, args: Vec<Marshalled>) -> Option<Promise<Marshalled>> {
        let args = args.as_slice();
        match name {
            "createProcess" => invoke!(self.create_process(
                &arg(args, 0)?,
                &arg::<String>(args, 1)?,
                arg(args, 2)?,
                arg(args, 3)?,
                arg(args, 4)?,
                &arg(args, 5)?,
                &arg(args, 6)?,
                &arg(args, 7)?,
                arg(args, 8)?,
                &arg::<String>(args, 9)?,
                &arg::<String>(args, 10)?,
            )),
            "updateTitle" => invoke!(self.update_title(arg(args, 0)?, &arg::<String>(args, 1)?, arg(args, 2)?)),
            "updateIcon" => invoke!(self.update_icon(arg(args, 0)?, arg(args, 1)?, &arg(args, 2)?, arg::<Option<String>>(args, 3)?.as_deref())),
            "attachToProcess" => invoke!(self.attach_to_process(arg(args, 0)?, arg(args, 1)?)),
            "detachFromProcess" => invoke!(self.detach_from_process(arg(args, 0)?, arg(args, 1)?)),
            "shutdownAll" => invoke!(self.shutdown_all()),
            "listProcesses" => invoke!(self.list_processes()),
            "getPerformanceMarks" => invoke!(self.get_performance_marks()),
            "reduceConnectionGraceTime" => invoke!(self.reduce_connection_grace_time()),
            "start" => invoke!(self.start(arg(args, 0)?)),
            "shutdown" => invoke!(self.shutdown(arg(args, 0)?, arg(args, 1)?)),
            "input" => invoke!(self.input(arg(args, 0)?, &arg::<String>(args, 1)?)),
            "sendSignal" => invoke!(self.send_signal(arg(args, 0)?, &arg::<String>(args, 1)?)),
            "processBinary" => invoke!(self.process_binary(arg(args, 0)?, &arg::<String>(args, 1)?)),
            "resize" => invoke!(self.resize(arg(args, 0)?, arg(args, 1)?, arg(args, 2)?, arg(args, 3)?, arg(args, 4)?)),
            "clearBuffer" => invoke!(self.clear_buffer(arg(args, 0)?)),
            "acknowledgeDataEvent" => invoke!(self.acknowledge_data_event(arg(args, 0)?, arg(args, 1)?)),
            "setUnicodeVersion" => invoke!(self.set_unicode_version(arg(args, 0)?, arg(args, 1)?)),
            "setNextCommandId" => invoke!(self.set_next_command_id(arg(args, 0)?, &arg::<String>(args, 1)?, &arg::<String>(args, 2)?)),
            "getInitialCwd" => invoke!(self.get_initial_cwd(arg(args, 0)?)),
            "getCwd" => invoke!(self.get_cwd(arg(args, 0)?)),
            "getLatency" => invoke!(self.get_latency()),
            "orphanQuestionReply" => invoke!(self.orphan_question_reply(arg(args, 0)?)),
            "installAutoReply" => invoke!(self.install_auto_reply(&arg::<String>(args, 0)?, &arg::<String>(args, 1)?)),
            "uninstallAllAutoReplies" => invoke!(self.uninstall_all_auto_replies()),
            "getDefaultSystemShell" => invoke!(self.get_default_system_shell(arg(args, 0)?)),
            "getEnvironment" => invoke!(self.get_environment()),
            "getWslPath" => invoke!(self.get_wsl_path(&arg::<String>(args, 0)?, arg(args, 1)?)),
            "getRevivedPtyNewId" => invoke!(self.get_revived_pty_new_id(&arg::<String>(args, 0)?, arg(args, 1)?)),
            "setTerminalLayoutInfo" => invoke!(self.set_terminal_layout_info(&arg(args, 0)?)),
            "getTerminalLayoutInfo" => invoke!(self.get_terminal_layout_info(&arg(args, 0)?)),
            "requestDetachInstance" => invoke!(self.request_detach_instance(&arg::<String>(args, 0)?, arg(args, 1)?)),
            "acceptDetachInstanceReply" => invoke!(self.accept_detach_instance_reply(arg(args, 0)?, arg(args, 1)?)),
            "freePortKillProcess" => invoke!(self.free_port_kill_process(&arg::<String>(args, 0)?)),
            "serializeTerminalState" => invoke!(self.serialize_terminal_state(&arg::<Vec<u32>>(args, 0)?)),
            "reviveTerminalProcesses" => invoke!(self.revive_terminal_processes(&arg::<String>(args, 0)?, &arg::<Vec<ISerializedTerminalState>>(args, 1)?, &arg::<String>(args, 2)?)),
            "refreshProperty" => invoke!(self.refresh_property(arg(args, 0)?, arg(args, 1)?)),
            "updateProperty" => invoke!(self.update_property(arg(args, 0)?, &update_property_argument(args)?)),
            "restartPtyHost" => invoke!(self.restart_pty_host()),
            "acceptPtyHostResolvedVariables" => invoke!(self.accept_pty_host_resolved_variables(arg(args, 0)?, arg(args, 1)?)),
            "getProfiles" => invoke!(self.get_profiles(
                &arg::<String>(args, 0)?,
                &super::pty_service_wire::dynamic_from_wire(args.get(1).unwrap_or(&Marshalled::Undefined)),
                &super::pty_service_wire::dynamic_from_wire(args.get(2).unwrap_or(&Marshalled::Undefined)),
                arg::<Option<bool>>(args, 3)?.unwrap_or(false),
            )),
            _ => None,
        }
    }
}

/// `updateProperty(id, property, value)`: o `{ type, value }` do [`IProcessProperty`] é remontado dos dois argumentos.
fn update_property_argument(args: &[Marshalled]) -> Result<IProcessProperty, Error> {
    const UNDEFINED: Marshalled = Marshalled::Undefined;
    let property = Marshalled::Object(vec![
        (je_base::JsString::from("type"), args.get(1).unwrap_or(&UNDEFINED).clone()),
        (je_base::JsString::from("value"), args.get(2).unwrap_or(&UNDEFINED).clone()),
    ]);
    IProcessProperty::from_wire(&property)
}
