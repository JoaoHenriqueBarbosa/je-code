//! Porte de `src/vs/workbench/services/extensions/common/extensionHostManager.ts`: o `ExtensionHostManager` (cria o `RPCProtocol` sobre
//! o `IMessagePassingProtocol` do host, instancia os clientes `MainThread*` do `ExtHostCustomersRegistry` e conversa com o host pelo
//! `IExtensionHostProxy`), `friendlyExtHostName` e os registradores do RPC (`RPCLogger`, `TelemetryRPCLogger`), mais o registro de
//! provedores de latência.
//!
//! Divergências:
//! - **Injeção de dependências.** `@IInstantiationService`, `@IWorkbenchEnvironmentService`, `@ITelemetryService` e `@ILogService` são
//!   os campos de [`ExtensionHostManagerServices`]. Os clientes não são instanciados por `createInstance(ctor, extHostContext)`: o
//!   registro guarda fechos que capturam os próprios serviços (ver `ext_host_customers`).
//! - **`extends Disposable`.** O gerenciador guarda um [`Disposable`] e implementa [`IDisposable`] por cima dele. Os métodos são `&self`
//!   e usam o `Weak` do próprio gerenciador ([`ExtensionHostManager::this`]) para dar um `Rc` às continuações; os `async` do original
//!   são [`async_fn`] (rodam de forma síncrona até o primeiro `await`, como o motor JS).
//! - **`this._proxy` (`Promise<IExtensionHostProxy | null> | null`)** é `Option<Promise<Option<Rc<dyn IExtensionHostProxy>>>>`. Um erro
//!   ao criar os clientes (`throw` dentro do `then`) rejeita a promessa, como no original.
//! - **`this._extensionHost.extensions!`** (asserção de não nulo) é um erro de tipo quando o host devolve `None`.
//! - **`RPCLogger`.** O `console.log` com estilos `%c` é `println!` sem as cores (`LOG_USE_COLORS` só decide a cor do texto no console
//!   do navegador); o `data` do `pretty` é o JSON do valor. `TelemetryRPCLogger.isEnabled()` (`Math.random() < 0.0001`) lê o relógio do
//!   sistema como fonte da fração, porque o porte não tem gerador de números aleatórios.
//! - **`MeasureExtHostLatencyAction`** (`registerAction2`, o comando "Measure Extension Host Latency", que abre o resultado no editor)
//!   não foi portada: depende do registro de ações e do `IEditorService`. A lógica que ela lê, os provedores de latência
//!   ([`register_latency_test_provider`], [`get_latency_test_providers`]) e a formatação ([`print_ext_host_latency_result`]), está aqui.
//! - `err.name`, `err.message` e `err.stack` da falha de início viram os campos do [`Error`] (`stack` só quando existe).
//! - **Formas do RPC.** O `RPCProtocol` do original cria o proxy de qualquer identificador; o do porte precisa que cada forma tipada
//!   seja registrada, e o gerenciador o faz logo depois de criar o protocolo ([`register_known_shapes`]).

use std::cell::{Cell, RefCell};
use std::collections::{HashMap, HashSet};
use std::rc::{Rc, Weak};
use std::time::{SystemTime, UNIX_EPOCH};

use je_base::async_::{IntervalTimer, async_fn};
use je_base::buffer::VSBuffer;
use je_base::dynamic::{Dynamic, json_stringify};
use je_base::errors::{self, Error};
use je_base::event::{Emitter, Event};
use je_base::lifecycle::{Disposable, DisposableRef, IDisposable, to_disposable};
use je_base::parts::ipc::{IMessagePassingProtocol, RequestInitiator};
use je_base::promise::Promise;
use je_base::stopwatch::StopWatch;
use je_base::uri::URI;
use je_extensions::manifest::ExtensionIdentifier;
use je_platform::log::{LogMessage, LogServiceRef, Logger};
use je_platform::remote::remote_authority_resolver::{RemoteAuthorityResolverErrorCode, get_remote_authority_prefix};
use je_platform::telemetry::telemetry::ITelemetryService;

use super::ext_host_customers::{CustomerBinding, ExtHostCustomersRegistry, IExtHostContext, IInternalExtHostContext};
use super::extension_activation::ExtensionActivationReason;
use super::extension_description_registry::SharedExtensionDescription;
use super::extension_host_kind::{ExtensionHostKind, extension_host_kind_to_string};
use super::extension_host_managers::IExtensionHostManager;
use super::extension_host_protocol::IExtensionDescriptionDelta;
use super::extension_host_proxy::{IExtensionHostProxy, IResolveAuthorityResult, ResolveAuthorityError};
use super::extension_running_location::ExtensionRunningLocation;
use super::extension_running_location_tracker::IRepresentsRunningLocation;
use super::extensions::{ActivationKind, ExtensionHostStartup, IExtensionHost, IExtensionInspectInfo, IInternalExtensionService};
use super::rpc_protocol::{IRPCProtocol, IRpcProtocolLogger, ResponsiveState, RpcProtocol};
use crate::api::common::ext_host_protocol::IMainContext;
use crate::api::common::ext_host_rpc_shapes::register_known_shapes;
use crate::services::environment::environment_service::IWorkbenchEnvironmentService;

/// Enable to see detailed message communication between window and extension host.
const LOG_EXTENSION_HOST_COMMUNICATION: bool = false;

/// Os serviços que o construtor do `ExtensionHostManager` recebe por decoradores (ver a nota do módulo).
#[derive(Clone)]
pub struct ExtensionHostManagerServices {
    pub environment_service: Rc<dyn IWorkbenchEnvironmentService>,
    pub telemetry_service: Rc<dyn ITelemetryService>,
    pub log_service: LogServiceRef,
}

/// `Date.now()`.
fn date_now() -> f64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map_or(0.0, |elapsed| elapsed.as_millis() as f64)
}

/// `ExtensionHostStartupEvent`.
fn startup_event(action: &str, kind: ExtensionHostKind, error: Option<&Error>) -> Dynamic {
    let mut entries = vec![
        ("time", Dynamic::Number(date_now())),
        ("action", Dynamic::string(action)),
        ("kind", Dynamic::string(extension_host_kind_to_string(Some(kind)))),
    ];
    if let Some(error) = error {
        if !error.name.is_empty() {
            entries.push(("errorName", Dynamic::string(&error.name)));
        }
        if !error.message.is_empty() {
            entries.push(("errorMessage", Dynamic::string(&error.message)));
        }
        if let Some(stack) = error.stack.as_deref().filter(|stack| !stack.is_empty()) {
            entries.push(("errorStack", Dynamic::string(stack)));
        }
    }
    Dynamic::object(entries)
}

/// O `IExtHostContext` que o gerenciador entrega a cada cliente: o `RPCProtocol` do host mais o que `IInternalExtHostContext` guarda.
struct ExtHostContext {
    remote_authority: Option<String>,
    extension_host_kind: ExtensionHostKind,
    rpc_protocol: Rc<RpcProtocol>,
    internal_extension_service: Rc<dyn IInternalExtensionService>,
    extension_host_proxy: RefCell<Option<Rc<dyn IExtensionHostProxy>>>,
    main_proxy_identifiers: RefCell<Vec<&'static str>>,
}

impl IMainContext for ExtHostContext {
    fn get_proxy_erased(&self, sid: &'static str) -> Rc<dyn std::any::Any> {
        self.rpc_protocol.get_proxy_erased(sid)
    }

    fn set_erased(&self, sid: &'static str, value: Rc<dyn std::any::Any>) {
        self.rpc_protocol.set_erased(sid, value);
    }
}

impl IDisposable for ExtHostContext {
    fn dispose(&self) {
        self.rpc_protocol.dispose();
    }
}

impl IRPCProtocol for ExtHostContext {
    fn assert_registered(&self, sids: &[&'static str]) -> Result<(), Error> {
        self.rpc_protocol.assert_registered(sids)
    }

    fn drain(&self) -> Promise<()> {
        self.rpc_protocol.drain()
    }
}

impl IExtHostContext for ExtHostContext {
    fn remote_authority(&self) -> Option<String> {
        self.remote_authority.clone()
    }

    fn extension_host_kind(&self) -> ExtensionHostKind {
        self.extension_host_kind
    }

    fn rpc_protocol(&self) -> Rc<RpcProtocol> {
        self.rpc_protocol.clone()
    }
}

impl IInternalExtHostContext for ExtHostContext {
    fn internal_extension_service(&self) -> Rc<dyn IInternalExtensionService> {
        self.internal_extension_service.clone()
    }

    fn set_extension_host_proxy(&self, extension_host_proxy: Rc<dyn IExtensionHostProxy>) {
        *self.extension_host_proxy.borrow_mut() = Some(extension_host_proxy);
    }

    fn set_all_main_proxy_identifiers(&self, main_proxy_identifiers: Vec<&'static str>) {
        *self.main_proxy_identifiers.borrow_mut() = main_proxy_identifiers;
    }
}

type ProxyPromise = Promise<Option<Rc<dyn IExtensionHostProxy>>>;

/// `ExtensionHostManager`.
pub struct ExtensionHostManager {
    me: Weak<ExtensionHostManager>,
    base: Rc<Disposable>,

    on_did_change_responsive_state: Rc<Emitter<ResponsiveState>>,

    /// A map of already requested activation events to speed things up if the same activation event is triggered multiple times.
    cached_activation_events: RefCell<HashMap<String, Promise<()>>>,
    resolved_activation_events: RefCell<HashSet<String>>,
    rpc_protocol: RefCell<Option<Rc<RpcProtocol>>>,
    customers: RefCell<Vec<Rc<dyn IDisposable>>>,
    extension_host: Rc<dyn IExtensionHost>,
    proxy: RefCell<Option<ProxyPromise>>,
    has_started: Cell<bool>,

    internal_extension_service: Rc<dyn IInternalExtensionService>,
    services: ExtensionHostManagerServices,
}

impl ExtensionHostManager {
    pub fn new(
        extension_host: Rc<dyn IExtensionHost>,
        initial_activation_events: Vec<String>,
        internal_extension_service: Rc<dyn IInternalExtensionService>,
        services: ExtensionHostManagerServices,
    ) -> Rc<Self> {
        let base = Disposable::new();
        let on_did_change_responsive_state = Emitter::new();
        base.register(on_did_change_responsive_state.clone());
        let this = Rc::new_cyclic(|me| Self {
            me: me.clone(),
            base,
            on_did_change_responsive_state,
            cached_activation_events: RefCell::new(HashMap::new()),
            resolved_activation_events: RefCell::new(HashSet::new()),
            rpc_protocol: RefCell::new(None),
            customers: RefCell::new(Vec::new()),
            extension_host,
            proxy: RefCell::new(None),
            has_started: Cell::new(false),
            internal_extension_service,
            services,
        });
        this.begin(initial_activation_events);
        this
    }

    fn this(&self) -> Rc<Self> {
        self.me.upgrade().expect("o gerenciador está vivo enquanto um método dele roda")
    }

    /// O corpo do construtor depois de os campos existirem: publica a telemetria de início e dispara `extensionHost.start()`.
    fn begin(&self, initial_activation_events: Vec<String>) {
        let this = self.this();
        let kind = self.kind();
        self.services.telemetry_service.public_log2("extensionHostStartup", Some(startup_event("starting", kind, None)));

        let starter = this.clone();
        let proxy: ProxyPromise = self.extension_host.start().then_result(move |result| match result {
            Ok(protocol) => {
                // Track healthy extension host startup
                starter.services.telemetry_service.public_log2("extensionHostStartup", Some(startup_event("success", kind, None)));
                starter.create_extension_host_customers(kind, protocol).map(Some)
            }
            Err(err) => {
                starter.services.log_service.error(&format!("Error received from starting extension host (kind: {})", extension_host_kind_to_string(Some(kind))));
                starter.services.log_service.error_args(&LogMessage::Error(err.clone()), &[]);

                // Track errors during extension host startup
                starter.services.telemetry_service.public_log2("extensionHostStartup", Some(startup_event("error", kind, Some(&err))));
                Ok(None)
            }
        });
        *self.proxy.borrow_mut() = Some(proxy.clone());
        let _ = proxy.then(move |_| {
            this.has_started.set(true);
            for activation_event in &initial_activation_events {
                let _ = this.activate_by_event_impl(activation_event, ActivationKind::Normal);
            }
            let weak = this.me.clone();
            this.base.register(register_latency_test_provider(Rc::new(ManagerLatencyProvider { manager: weak })));
        });
    }

    /// `await this._proxy` quando o campo é `null` (o gerenciador já foi descartado): `undefined`.
    fn proxy_promise(&self) -> ProxyPromise {
        self.proxy.borrow().clone().unwrap_or_else(|| Promise::resolved(None))
    }

    fn rpc(&self) -> Option<Rc<RpcProtocol>> {
        self.rpc_protocol.borrow().clone()
    }

    fn extensions(&self, member: &str) -> Result<Rc<super::extensions::ExtensionHostExtensions>, Error> {
        self.extension_host.extensions().ok_or_else(|| Error::type_error(format!("Cannot read properties of null (reading '{member}')")))
    }

    fn measure(&self) -> Promise<Option<ExtHostLatencyResult>> {
        let this = self.this();
        async_fn(async move {
            let Some(proxy) = this.proxy_promise().await? else { return Ok(None) };
            let latency = measure_latency(&proxy).await?;
            let down = measure_down(&proxy).await?;
            let up = measure_up(&proxy).await?;
            Ok(Some(ExtHostLatencyResult { remote_authority: this.extension_host.remote_authority(), latency, down, up }))
        })
    }

    fn create_extension_host_customers(&self, kind: ExtensionHostKind, protocol: Rc<dyn IMessagePassingProtocol>) -> Result<Rc<dyn IExtensionHostProxy>, Error> {
        let logger: Option<Rc<dyn IRpcProtocolLogger>> =
            if LOG_EXTENSION_HOST_COMMUNICATION || self.services.environment_service.log_extension_host_communication() == Some(true) {
                Some(Rc::new(RpcLogger::new(kind)))
            } else if TelemetryRpcLogger::is_enabled() {
                Some(Rc::new(TelemetryRpcLogger::new(self.services.telemetry_service.clone())))
            } else {
                None
            };

        let rpc_protocol = RpcProtocol::new(protocol, logger);
        register_known_shapes(&rpc_protocol);
        *self.rpc_protocol.borrow_mut() = Some(rpc_protocol.clone());
        let weak = self.me.clone();
        self.base.register(rpc_protocol.on_did_change_responsive_state().listen(move |responsive_state: &ResponsiveState| {
            if let Some(this) = weak.upgrade() {
                this.on_did_change_responsive_state.fire(*responsive_state);
            }
        }));
        let ext_host_context = Rc::new(ExtHostContext {
            remote_authority: self.extension_host.remote_authority(),
            extension_host_kind: kind,
            rpc_protocol: rpc_protocol.clone(),
            internal_extension_service: self.internal_extension_service.clone(),
            extension_host_proxy: RefCell::new(None),
            main_proxy_identifiers: RefCell::new(Vec::new()),
        });
        let context: Rc<dyn IInternalExtHostContext> = ext_host_context.clone();

        // Named customers
        for named_customer in ExtHostCustomersRegistry::get_named_customers() {
            let id = named_customer.id;
            let created = (named_customer.ctor)(&context).and_then(|instance| {
                self.customers.borrow_mut().push(instance.disposable.clone());
                errors::catch(|| match instance.binding {
                    CustomerBinding::Shape(value) => rpc_protocol.set_erased(id, value),
                    CustomerBinding::Actor(actor) => {
                        rpc_protocol.set_actor(id, actor);
                    }
                })
            });
            if let Err(err) = created {
                self.services.log_service.error(&format!("Cannot instantiate named customer: '{id}'"));
                self.services.log_service.error_args(&LogMessage::Error(err.clone()), &[]);
                errors::on_unexpected_error(&err);
            }
        }

        // Customers
        for ctor in ExtHostCustomersRegistry::get_customers() {
            match ctor(&context) {
                Ok(instance) => self.customers.borrow_mut().push(instance),
                Err(err) => {
                    self.services.log_service.error_args(&LogMessage::Error(err.clone()), &[]);
                    errors::on_unexpected_error(&err);
                }
            }
        }

        let extension_host_proxy = ext_host_context.extension_host_proxy.borrow().clone();
        let Some(extension_host_proxy) = extension_host_proxy else { return Err(Error::new("Missing IExtensionHostProxy!")) };

        // Check that no named customers are missing
        let main_proxy_identifiers = ext_host_context.main_proxy_identifiers.borrow().clone();
        rpc_protocol.assert_registered(&main_proxy_identifiers)?;

        Ok(extension_host_proxy)
    }

    /// `activateByEvent(activationEvent, activationKind)`.
    fn activate_by_event_impl(&self, activation_event: &str, activation_kind: ActivationKind) -> Promise<()> {
        let cached = self.cached_activation_events.borrow().get(activation_event).cloned();
        if let Some(cached) = cached {
            return cached;
        }
        let promise = self.do_activate_by_event(activation_event, activation_kind);
        self.cached_activation_events.borrow_mut().insert(activation_event.to_owned(), promise.clone());
        promise
    }

    /// `_activateByEvent(activationEvent, activationKind)`.
    fn do_activate_by_event(&self, activation_event: &str, activation_kind: ActivationKind) -> Promise<()> {
        let this = self.this();
        let activation_event = activation_event.to_owned();
        async_fn(async move {
            if this.proxy.borrow().is_none() {
                return Ok(());
            }
            let Some(proxy) = this.proxy_promise().await? else {
                // this case is already covered above and logged.
                // i.e. the extension host could not be started
                return Ok(());
            };

            if !this.extensions("containsActivationEvent")?.contains_activation_event(&activation_event) {
                this.resolved_activation_events.borrow_mut().insert(activation_event);
                return Ok(());
            }

            proxy.activate_by_event(&activation_event, activation_kind).await?;
            this.resolved_activation_events.borrow_mut().insert(activation_event);
            Ok(())
        })
    }
}

impl IRepresentsRunningLocation for ExtensionHostManager {
    fn represents_running_location(&self, running_location: &ExtensionRunningLocation) -> bool {
        self.extension_host.running_location().equals(running_location)
    }
}

impl IDisposable for ExtensionHostManager {
    fn dispose(&self) {
        self.extension_host.dispose();
        if let Some(rpc_protocol) = self.rpc() {
            rpc_protocol.dispose();
        }

        let customers = std::mem::take(&mut *self.customers.borrow_mut());
        for customer in customers {
            if let Err(err) = errors::catch(|| customer.dispose()) {
                errors::on_unexpected_error(&err);
            }
        }
        *self.proxy.borrow_mut() = None;

        self.base.dispose();
    }
}

impl IExtensionHostManager for ExtensionHostManager {
    fn pid(&self) -> Option<u32> {
        self.extension_host.pid()
    }

    fn kind(&self) -> ExtensionHostKind {
        self.extension_host.running_location().kind()
    }

    fn startup(&self) -> ExtensionHostStartup {
        self.extension_host.startup()
    }

    fn friendly_name(&self) -> String {
        friendly_ext_host_name(self.kind(), self.pid())
    }

    fn on_did_exit(&self) -> Event<(i32, Option<String>)> {
        self.extension_host.on_exit()
    }

    fn on_did_change_responsive_state(&self) -> Event<ResponsiveState> {
        self.on_did_change_responsive_state.event()
    }

    fn disconnect(&self) -> Promise<()> {
        self.extension_host.disconnect().unwrap_or_else(|| Promise::resolved(()))
    }

    fn is_ready(&self) -> bool {
        self.has_started.get()
    }

    fn ready(&self) -> Promise<()> {
        let proxy = self.proxy_promise();
        async_fn(async move {
            proxy.await?;
            Ok(())
        })
    }

    fn delta_extensions(&self, incoming_extensions_delta: IExtensionDescriptionDelta) -> Promise<()> {
        let this = self.this();
        async_fn(async move {
            let Some(proxy) = this.proxy_promise().await? else { return Ok(()) };
            let Some(outgoing_extensions_delta) = this.extensions("delta")?.delta(&incoming_extensions_delta) else {
                // The extension host already has this version of the extensions.
                return Ok(());
            };
            proxy.delta_extensions(outgoing_extensions_delta).await
        })
    }

    fn contains_extension(&self, extension_id: &ExtensionIdentifier) -> bool {
        self.extension_host.extensions().is_some_and(|extensions| extensions.contains_extension(extension_id))
    }

    fn activate(&self, extension: &ExtensionIdentifier, reason: ExtensionActivationReason) -> Promise<bool> {
        let proxy = self.proxy_promise();
        let extension = extension.clone();
        async_fn(async move {
            let Some(proxy) = proxy.await? else { return Ok(false) };
            proxy.activate(&extension, reason).await
        })
    }

    fn activate_by_event(&self, activation_event: &str, activation_kind: ActivationKind) -> Promise<()> {
        self.activate_by_event_impl(activation_event, activation_kind)
    }

    fn activation_event_is_done(&self, activation_event: &str) -> bool {
        self.resolved_activation_events.borrow().contains(activation_event)
    }

    fn get_inspect_port(&self, try_enable_inspector: bool) -> Promise<Option<IExtensionInspectInfo>> {
        let this = self.this();
        async_fn(async move {
            if try_enable_inspector {
                this.extension_host.enable_inspect_port().await?;
            }
            Ok(this.extension_host.get_inspect_port())
        })
    }

    fn resolve_authority(&self, remote_authority: &str, resolve_attempt: u32) -> Promise<IResolveAuthorityResult> {
        let this = self.this();
        let remote_authority = remote_authority.to_owned();
        async_fn(async move {
            let sw = Rc::new(StopWatch::create(Some(false)));
            let running_location = this.extension_host.running_location();
            let prefix: Rc<dyn Fn() -> String> = {
                let sw = sw.clone();
                let remote_authority = remote_authority.clone();
                Rc::new(move || {
                    format!(
                        "[{}{}][resolveAuthority({},{})][{}ms] ",
                        extension_host_kind_to_string(Some(running_location.kind())),
                        running_location.affinity(),
                        get_remote_authority_prefix(&remote_authority),
                        resolve_attempt,
                        sw.elapsed()
                    )
                })
            };
            let log_info = {
                let (this, prefix) = (this.clone(), prefix.clone());
                Rc::new(move |msg: &str| this.services.log_service.info(&format!("{}{msg}", prefix())))
            };
            let log_error = {
                let (this, prefix) = (this.clone(), prefix.clone());
                move |msg: &str, err: Option<&Error>| {
                    this.services.log_service.error(&format!("{}{msg}", prefix()));
                    if let Some(err) = err {
                        this.services.log_service.error_args(&LogMessage::Error(err.clone()), &[]);
                    }
                }
            };

            log_info("obtaining proxy...");
            let proxy = this.proxy_promise().await?;
            let Some(proxy) = proxy else {
                log_error("no proxy", None);
                return Ok(IResolveAuthorityResult::Error(ResolveAuthorityError {
                    message: Some("Cannot resolve authority".to_owned()),
                    code: RemoteAuthorityResolverErrorCode::Unknown,
                    detail: None,
                }));
            };
            log_info("invoking...");
            let interval_logger = IntervalTimer::new();
            let waiting = log_info.clone();
            interval_logger.cancel_and_set(move || waiting("waiting..."), 1000);
            match proxy.resolve_authority(&remote_authority, resolve_attempt).await {
                Ok(resolver_result) => {
                    interval_logger.dispose();
                    match &resolver_result {
                        IResolveAuthorityResult::Ok(resolved) => log_info(&format!("returned {}", resolved.authority.connect_to)),
                        IResolveAuthorityResult::Error(error) => {
                            log_error("returned an error", Some(&Error::new(error.message.clone().unwrap_or_default())));
                        }
                    }
                    Ok(resolver_result)
                }
                Err(err) => {
                    interval_logger.dispose();
                    log_error("returned an error", Some(&err));
                    Ok(IResolveAuthorityResult::Error(ResolveAuthorityError {
                        message: Some(err.message.clone()),
                        code: RemoteAuthorityResolverErrorCode::Unknown,
                        detail: Some(Dynamic::object([("name", Dynamic::string(&err.name)), ("message", Dynamic::string(&err.message))])),
                    }))
                }
            }
        })
    }

    fn get_canonical_uri(&self, remote_authority: &str, uri: &URI) -> Promise<Option<URI>> {
        let proxy = self.proxy_promise();
        let (remote_authority, uri) = (remote_authority.to_owned(), uri.clone());
        async_fn(async move {
            let Some(proxy) = proxy.await? else { return Err(Error::new("Cannot resolve canonical URI")) };
            proxy.get_canonical_uri(&remote_authority, &uri).await
        })
    }

    fn start(&self, extension_registry_version_id: i64, all_extensions: Vec<SharedExtensionDescription>, my_extensions: Vec<ExtensionIdentifier>) -> Promise<()> {
        let this = self.this();
        async_fn(async move {
            let Some(proxy) = this.proxy_promise().await? else { return Ok(()) };
            let delta_extensions = this.extensions("set")?.set(extension_registry_version_id, &all_extensions, &my_extensions)?;
            proxy.start_extension_host(delta_extensions).await
        })
    }

    fn extension_tests_execute(&self) -> Promise<i32> {
        let proxy = self.proxy_promise();
        async_fn(async move {
            let Some(proxy) = proxy.await? else { return Err(Error::new("Could not obtain Extension Host Proxy")) };
            proxy.extension_tests_execute().await
        })
    }

    fn set_remote_environment(&self, env: Vec<(String, Option<String>)>) -> Promise<()> {
        let proxy = self.proxy_promise();
        async_fn(async move {
            let Some(proxy) = proxy.await? else { return Ok(()) };
            proxy.set_remote_environment(env).await
        })
    }
}

/// `friendlyExtHostName(kind, pid)`.
pub fn friendly_ext_host_name(kind: ExtensionHostKind, pid: Option<u32>) -> String {
    match pid {
        Some(pid) if pid != 0 => format!("{} pid: {pid}", extension_host_kind_to_string(Some(kind))),
        _ => extension_host_kind_to_string(Some(kind)).to_owned(),
    }
}

// #region Latência

const LATENCY_PROBE_SIZE: usize = 10 * 1024 * 1024; // 10MB

async fn measure_latency(proxy: &Rc<dyn IExtensionHostProxy>) -> Result<f64, Error> {
    const COUNT: u32 = 10;

    let mut sum = 0.0;
    for i in 0..COUNT {
        let mut sw = StopWatch::create(None);
        proxy.test_latency(f64::from(i)).await?;
        sw.stop();
        sum += sw.elapsed();
    }
    Ok(sum / f64::from(COUNT))
}

fn convert_bits_per_second(byte_count: usize, elapsed_millis: f64) -> f64 {
    (byte_count as f64 * 1000.0 * 8.0) / elapsed_millis
}

async fn measure_up(proxy: &Rc<dyn IExtensionHostProxy>) -> Result<f64, Error> {
    let buff = VSBuffer::alloc(LATENCY_PROBE_SIZE);
    let value = ((date_now() % 256.0) as usize) + 1;
    for i in 0..buff.byte_length() {
        // `buff.writeUInt8(i, value)`: o original passa os argumentos na ordem (valor, posição) trocados, e o porte os mantém.
        buff.write_u_int8(i as u8, value);
    }
    let mut sw = StopWatch::create(None);
    proxy.test_up(buff).await?;
    sw.stop();
    Ok(convert_bits_per_second(LATENCY_PROBE_SIZE, sw.elapsed()))
}

async fn measure_down(proxy: &Rc<dyn IExtensionHostProxy>) -> Result<f64, Error> {
    let mut sw = StopWatch::create(None);
    proxy.test_down(LATENCY_PROBE_SIZE).await?;
    sw.stop();
    Ok(convert_bits_per_second(LATENCY_PROBE_SIZE, sw.elapsed()))
}

/// `ExtHostLatencyResult`.
#[derive(Clone, Debug)]
pub struct ExtHostLatencyResult {
    pub remote_authority: Option<String>,
    pub up: f64,
    pub down: f64,
    pub latency: f64,
}

/// `ExtHostLatencyProvider`.
pub trait ExtHostLatencyProvider {
    fn measure(&self) -> Promise<Option<ExtHostLatencyResult>>;
}

struct ManagerLatencyProvider {
    manager: Weak<ExtensionHostManager>,
}

impl ExtHostLatencyProvider for ManagerLatencyProvider {
    fn measure(&self) -> Promise<Option<ExtHostLatencyResult>> {
        match self.manager.upgrade() {
            Some(manager) => manager.measure(),
            None => Promise::resolved(None),
        }
    }
}

thread_local! {
    static PROVIDERS: RefCell<Vec<Rc<dyn ExtHostLatencyProvider>>> = const { RefCell::new(Vec::new()) };
}

/// `registerLatencyTestProvider(provider)`.
pub fn register_latency_test_provider(provider: Rc<dyn ExtHostLatencyProvider>) -> DisposableRef {
    PROVIDERS.with(|providers| providers.borrow_mut().push(provider.clone()));
    to_disposable(move || {
        PROVIDERS.with(|providers| {
            let mut providers = providers.borrow_mut();
            if let Some(index) = providers.iter().position(|candidate| Rc::ptr_eq(candidate, &provider)) {
                providers.remove(index);
            }
        });
    })
}

/// `getLatencyTestProviders()`.
pub fn get_latency_test_providers() -> Vec<Rc<dyn ExtHostLatencyProvider>> {
    PROVIDERS.with(|providers| providers.borrow().clone())
}

/// `MeasureExtHostLatencyAction._printSpeed(n)`.
fn print_speed(n: f64) -> String {
    if n <= 1024.0 {
        return format!("{n} bps");
    }
    if n < 1024.0 * 1024.0 {
        return format!("{:.1} kbps", n / 1024.0);
    }
    format!("{:.1} Mbps", n / 1024.0 / 1024.0)
}

/// `MeasureExtHostLatencyAction._print(m)`.
pub fn print_ext_host_latency_result(m: Option<&ExtHostLatencyResult>) -> String {
    let Some(m) = m else { return String::new() };
    let authority = m.remote_authority.as_ref().filter(|authority| !authority.is_empty()).map_or_else(String::new, |authority| format!("Authority: {authority}\n"));
    format!("{authority}Roundtrip latency: {:.3}ms\nUp: {}\nDown: {}\n", m.latency, print_speed(m.up), print_speed(m.down))
}

// #endregion

// #region Registradores do RPC

/// `pretty(data)`: o texto de um valor para o console (o JSON; o que não serializa vira o texto de depuração).
fn pretty(data: Option<&Dynamic>) -> String {
    match data {
        None => String::new(),
        Some(data) => json_stringify(data).unwrap_or_else(|| format!("{data:?}")),
    }
}

/// `RPCLogger`.
pub struct RpcLogger {
    kind: ExtensionHostKind,
    total_incoming: Cell<usize>,
    total_outgoing: Cell<usize>,
}

impl RpcLogger {
    pub fn new(kind: ExtensionHostKind) -> Self {
        Self { kind, total_incoming: Cell::new(0), total_outgoing: Cell::new(0) }
    }

    fn log(&self, direction: &str, total_length: usize, msg_length: usize, req: u32, str: &str, data: Option<&Dynamic>) {
        let head = format!("[{}][{direction}][{total_length:>7}][len: {msg_length:>5}]{req:>5} - {str}", extension_host_kind_to_string(Some(self.kind)));
        if str.ends_with('(') {
            println!("{head}{})", pretty(data));
        } else {
            println!("{head} {}", pretty(data));
        }
    }
}

impl IRpcProtocolLogger for RpcLogger {
    fn log_incoming(&self, msg_length: usize, req: u32, _initiator: RequestInitiator, str: &str, data: Option<&Dynamic>) {
        self.total_incoming.set(self.total_incoming.get() + msg_length);
        self.log("Ext \u{2192} Win", self.total_incoming.get(), msg_length, req, str, data);
    }

    fn log_outgoing(&self, msg_length: usize, req: u32, _initiator: RequestInitiator, str: &str, data: Option<&Dynamic>) {
        self.total_outgoing.set(self.total_outgoing.get() + msg_length);
        self.log("Win \u{2192} Ext", self.total_outgoing.get(), msg_length, req, str, data);
    }
}

/// `TelemetryRPCLogger`.
pub struct TelemetryRpcLogger {
    telemetry_service: Rc<dyn ITelemetryService>,
    pending_requests: RefCell<HashMap<u32, String>>,
}

impl TelemetryRpcLogger {
    pub fn new(telemetry_service: Rc<dyn ITelemetryService>) -> Self {
        Self { telemetry_service, pending_requests: RefCell::new(HashMap::new()) }
    }

    /// `isEnabled()`: 0.01% of users.
    pub fn is_enabled() -> bool {
        let nanos = SystemTime::now().duration_since(UNIX_EPOCH).map_or(1, |elapsed| elapsed.subsec_nanos());
        f64::from(nanos % 1_000_000) / 1_000_000.0 < 0.0001
    }

    fn publish(&self, event_name: &str, kind: &str, msg_length: usize) {
        self.telemetry_service.public_log2(event_name, Some(Dynamic::object([("type", Dynamic::string(kind)), ("length", Dynamic::Number(msg_length as f64))])));
    }
}

impl IRpcProtocolLogger for TelemetryRpcLogger {
    fn log_incoming(&self, msg_length: usize, req: u32, initiator: RequestInitiator, str: &str, _data: Option<&Dynamic>) {
        if initiator == RequestInitiator::LocalSide && (str.starts_with("receiveReply:") || str.starts_with("receiveReplyErr:")) {
            // log the size of reply messages
            let request_str = self.pending_requests.borrow_mut().remove(&req).unwrap_or_else(|| "unknown_reply".to_owned());
            self.publish("extensionhost.incoming", &format!("{str} {request_str}"), msg_length);
        }

        if initiator == RequestInitiator::OtherSide && str.starts_with("receiveRequest ") {
            // incoming request
            self.publish("extensionhost.incoming", str, msg_length);
        }
    }

    fn log_outgoing(&self, msg_length: usize, req: u32, initiator: RequestInitiator, str: &str, _data: Option<&Dynamic>) {
        if initiator == RequestInitiator::LocalSide && str.starts_with("request: ") {
            self.pending_requests.borrow_mut().insert(req, str.to_owned());
            self.publish("extensionhost.outgoing", str, msg_length);
        }
    }
}

// #endregion
