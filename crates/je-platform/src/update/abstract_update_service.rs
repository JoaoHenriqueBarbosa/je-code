//! Porte de `src/vs/platform/update/electron-main/abstractUpdateService.ts`: o `AbstractUpdateService`, a base dos
//! serviços de atualização de cada plataforma. Guarda o estado do update (a máquina de [`State`]), reage à
//! configuração `update.mode` e à política, agenda as verificações periódicas, respeita a conexão cobrada por volume e
//! coordena o download, a aplicação e o `quitAndInstall` (inclusive a checagem de update "overwrite" do estado `Ready`).
//!
//! Não há processo principal do Electron no je-code, mas o papel dele existe no processo único; por isso o serviço é o
//! mesmo, sem janelas nem IPC. As dependências são os contratos reais do porte (o `LifecycleMainService`, o
//! `NativeEnvironmentServiceApi`, o `IProductService` e o `ApplicationStorageMainService`), cada um com o membro que a
//! base lê (`quit`, `disableUpdates` e `updateUrl` foram acrescentados a eles).
//!
//! Decisões de tradução:
//! - **Classe abstrata e membros `protected`**: o Rust não tem herança. O serviço concreto é um [`AbstractUpdateService`]
//!   mais um [`AbstractUpdateServiceHooks`], o conjunto de métodos que a subclasse sobrescreve (`buildUpdateFeedUrl` e
//!   `doCheckForUpdates` são abstratos; `initialize`, `doDownloadUpdate`, `resumeDeferredDownload`, `doApplyUpdate`,
//!   `doQuitAndInstall`, `postInitialize`, `cancelPendingUpdate`, `cancelUpdate`, `doIsLatestVersion`, `getUpdateType` e
//!   `_applySpecificUpdate` têm o corpo da base como padrão). Cada gancho recebe o serviço, para chamar os membros
//!   `protected` (`setState`, `quality`, `deferAutomaticDownload`...), que aqui são `pub`. O `super.xxx()` de uma
//!   sobrescrita é o método `*_default` do serviço ([`AbstractUpdateService::initialize_default`],
//!   [`AbstractUpdateService::cancel_update_default`], [`AbstractUpdateService::do_is_latest_version_default`]).
//! - **`IUpdateService`** é o trait [`UpdateService`] de `update.rs`. `isLatestVersion(commit?, token?)` tem argumentos
//!   a mais que o contrato (`isLatestVersion()`), então a forma completa é [`AbstractUpdateService::is_latest_version_with`].
//! - **`async`**: as funções `async` do original são [`async_fn`]: o corpo roda já, de forma síncrona, até o primeiro
//!   `await`, como no JavaScript. `void promise` é descartar a [`Promise`].
//! - **`@IProductService`**: o [`IProductService`] do porte (com `updateUrl`).
//! - **`@IEnvironmentMainService`**: o `IEnvironmentMainService` estende o `INativeEnvironmentService`, que aqui é o
//!   [`NativeEnvironmentServiceApi`] (com `disableUpdates`); `isBuilt` vem do `EnvironmentService`.
//! - **`@ILifecycleMainService`**: o [`LifecycleMainService`] (com `quit(willRestart)`).
//! - **`@IApplicationStorageMainService`**: o [`ApplicationStorageMainService`] portado (`whenReady` e o armazenamento
//!   no escopo da aplicação, por [`ApplicationStorageMainService::service`]).
//! - **`IUpdateURLOptions`** é [`UpdateUrlOptions`]; `createUpdateURL` é [`create_update_url`], com o `new URL` do
//!   padrão WHATWG (lança `TypeError: Invalid URL`, como o Node).
//! - **`getUpdateRequestHeaders`**: o `User-Agent` do Windows leva `Electron/${process.versions.electron}` no original;
//!   o je-code não tem Electron, então o token não existe e o cabeçalho é `Code/<versão> Windows NT <maior.menor>`.
//!   No macOS é `Code/<versão> Darwin/<release do kernel>`, e nas demais plataformas não há cabeçalhos.
//! - **`telemetryService.publicLog2<E, T>`**: a classificação GDPR é só tipo do TypeScript; o evento é um [`Dynamic`].
//! - **`State.Restarting(this.state.update)`** com um estado sem `update` (só acontece se o estado mudou durante o
//!   `await` da checagem de overwrite) usa um [`Update`] vazio, no lugar do `undefined` do original.

use std::cell::{Cell, RefCell};
use std::rc::{Rc, Weak};

use ::url::Url;
use je_base::JsString;
use je_base::async_::{IntervalTimer, Throttler, async_fn, timeout, timeout_with_token};
use je_base::cancellation::{CancellationToken, CancellationTokenSource};
use je_base::dynamic::{Dynamic, Truthy, json_stringify};
use je_base::errors::{Error, is_cancellation_error};
use je_base::event::{Emitter, Event};
use je_base::json;
use je_base::lifecycle::{Disposable, IDisposable, MutableDisposable, to_disposable};
use je_base::node::windows_version::{get_windows_release_sync, os_release};
use je_base::parts::request::{HeaderValue, IHeaders, IRequestOptions};
use je_base::platform::Platform;
use je_base::promise::Promise;
use je_base::runtime;

use super::update::{AvailableForDownload, DisablementReason, State, StateType, Update, UpdateService, UpdateType};
use crate::configuration::service::IConfigurationService;
use crate::configuration::ConfigurationOverrides;
use crate::js::from_json;
use crate::log::{LogArg, LogMessage, LogServiceRef, Logger};
use crate::metered_connection::IMeteredConnectionService;
use crate::request::IRequestService;
use crate::storage::{ApplicationStorageMainService, StorageScope, StorageService, StorageTarget};
use crate::telemetry::ITelemetryService;
use crate::environment::environment::{EnvironmentService, NativeEnvironmentServiceApi};
use crate::product::IProductService;
use crate::user_data_profile::user_data_profiles_handler::{LifecycleMainPhase, LifecycleMainService};
use je_storage::StorageValue;

const LAST_KNOWN_VERSION_STORAGE_KEY: &str = "abstractUpdateService/lastKnownVersion";

/// A chave da configuração que escolhe o modo de atualização (`none`, `manual`, `start` ou `default`).
const UPDATE_MODE_KEY: &str = "update.mode";

const DEFAULT_CHECK_DELAY_MS: u64 = 60 * 60 * 1000;
const INITIAL_CHECK_DELAY_MS: u64 = 30 * 1000;
const OVERWRITE_CHECK_INTERVAL_MS: u64 = 5 * 60 * 1000;
const OVERWRITE_PROBE_TIMEOUT_MS: u64 = 2000;

/// `IUpdateURLOptions`.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct UpdateUrlOptions {
    pub background: Option<bool>,
    pub internal_org: Option<String>,
}

/// `createUpdateURL`.
pub fn create_update_url(base_update_url: &str, platform: &str, quality: &str, commit: &str, options: Option<&UpdateUrlOptions>) -> String {
    let mut url = crate::url::whatwg::new_url(&JsString::from(format!("{base_update_url}/api/update/{platform}/{quality}/{commit}").as_str()));

    if options.is_some_and(|options| options.background == Some(true)) {
        set_search_param(&mut url, "bg", "true");
    }

    set_search_param(&mut url, "u", options.and_then(|options| options.internal_org.as_deref()).unwrap_or("none"));

    url.to_string()
}

/// `url.searchParams.set(name, value)`: troca o primeiro par de mesmo nome e remove os demais.
fn set_search_param(url: &mut Url, name: &str, value: &str) {
    let mut pairs: Vec<(String, String)> = url.query_pairs().map(|(key, value)| (key.into_owned(), value.into_owned())).collect();
    match pairs.iter().position(|(key, _)| key == name) {
        Some(index) => {
            pairs[index].1 = value.to_string();
            let mut seen = false;
            pairs.retain(|(key, _)| key != name || !std::mem::replace(&mut seen, true));
        }
        None => pairs.push((name.to_string(), value.to_string())),
    }
    url.query_pairs_mut().clear().extend_pairs(pairs);
}

/// `/^(\d+\.\d+)/`: o começo `maior.menor` de uma versão.
fn leading_major_minor(release: &str) -> Option<&str> {
    let digits = |text: &str| text.len() - text.trim_start_matches(|c: char| c.is_ascii_digit()).len();
    let major = digits(release);
    if major == 0 || !release[major..].starts_with('.') {
        return None;
    }
    let minor = digits(&release[major + 1..]);
    (minor > 0).then(|| &release[..major + 1 + minor])
}

/// Builds common headers for update requests, including those issued via the auto updater (e.g. `setFeedURL({ url,
/// headers })`) and manual HTTP requests that bypass the auto updater. The headers include OS version information
/// which the update server uses for EOL detection.
///
/// On macOS, the User-Agent includes the Darwin kernel version. On Windows, the User-Agent includes accurate Windows
/// version from the registry.
pub fn get_update_request_headers(product_version: &str) -> Option<IHeaders> {
    let platform = Platform::detect();
    let user_agent = if platform.is_macintosh {
        Some(format!("Code/{product_version} Darwin/{}", os_release()))
    } else if platform.is_windows {
        leading_major_minor(&get_windows_release_sync()).map(|version| format!("Code/{product_version} Windows NT {version}"))
    } else {
        None
    };

    user_agent.map(|user_agent| IHeaders::from([("User-Agent".to_string(), HeaderValue::One(user_agent))]))
}

/// States representing in-flight or pending update work that takes time to tear down when updates are disabled at
/// runtime. Used to decide whether to surface a transient `Cancelling` state.
fn is_cancellable_state(state_type: StateType) -> bool {
    matches!(
        state_type,
        StateType::CheckingForUpdates
            | StateType::AvailableForDownload
            | StateType::Downloading
            | StateType::Downloaded
            | StateType::Updating
            | StateType::Ready
            | StateType::Overwriting
    )
}

/// O `update` de um estado, nos que o têm.
fn state_update(state: &State) -> Option<Update> {
    match state {
        State::AvailableForDownload(state) => Some(state.update.clone()),
        State::Downloading(state) => state.update.clone(),
        State::Downloaded(state) => Some(state.update.clone()),
        State::Updating(state) => Some(state.update.clone()),
        State::Ready(state) => Some(state.update.clone()),
        State::Overwriting(state) => Some(state.update.clone()),
        State::Restarting(state) => Some(state.update.clone()),
        State::Uninitialized | State::Disabled(_) | State::Idle(_) | State::CheckingForUpdates(_) | State::Cancelling => None,
    }
}

/// O texto de um valor dinâmico, quando é uma string.
fn dynamic_text(value: &Dynamic) -> Option<String> {
    match value {
        Dynamic::String(text) => Some(text.to_string_lossy()),
        _ => None,
    }
}

/// `value?.[key]`: `undefined` para o que não é objeto.
fn property(value: &Dynamic, key: &str) -> Dynamic {
    match value {
        Dynamic::Object(_) => value.get_str(key),
        _ => Dynamic::Undefined,
    }
}

fn text_arg(text: &str) -> LogArg {
    LogArg::Value(Dynamic::string(text))
}

fn headers_to_dynamic(headers: &IHeaders) -> Dynamic {
    Dynamic::object(headers.iter().map(|(name, value)| {
        let value = match value {
            HeaderValue::One(text) => Dynamic::string(text),
            HeaderValue::Many(items) => Dynamic::array(items.iter().map(|item| Dynamic::string(item))),
        };
        (name.as_str(), value)
    }))
}

/// Os métodos que as subclasses de `AbstractUpdateService` sobrescrevem. Os corpos padrão são os da base.
pub trait AbstractUpdateServiceHooks {
    /// `protected async initialize()`.
    fn initialize(&self, core: &Rc<AbstractUpdateService>) -> Promise<()> {
        core.initialize_default()
    }

    /// `protected abstract buildUpdateFeedUrl(quality, commit, options?)`.
    fn build_update_feed_url(&self, core: &Rc<AbstractUpdateService>, quality: &str, commit: &str, options: Option<&UpdateUrlOptions>) -> Option<String>;

    /// `protected abstract doCheckForUpdates(explicit, pendingCommit?)`.
    fn do_check_for_updates(&self, core: &Rc<AbstractUpdateService>, explicit: bool, pending_commit: Option<&str>);

    /// `protected async doDownloadUpdate(state)`.
    fn do_download_update(&self, _core: &Rc<AbstractUpdateService>, _state: &AvailableForDownload) -> Promise<()> {
        Promise::resolved(())
    }

    /// `protected resumeDeferredDownload()`.
    fn resume_deferred_download(&self, core: &Rc<AbstractUpdateService>) {
        let _ = core.download_update(false);
    }

    /// `protected async doApplyUpdate()`.
    fn do_apply_update(&self, _core: &Rc<AbstractUpdateService>) -> Promise<()> {
        Promise::resolved(())
    }

    /// `protected doQuitAndInstall()`.
    fn do_quit_and_install(&self, _core: &Rc<AbstractUpdateService>) {}

    /// `protected async postInitialize()`.
    fn post_initialize(&self, _core: &Rc<AbstractUpdateService>) -> Promise<()> {
        Promise::resolved(())
    }

    /// `protected async cancelPendingUpdate()`.
    fn cancel_pending_update(&self, _core: &Rc<AbstractUpdateService>) -> Promise<()> {
        Promise::resolved(())
    }

    /// Aborts in-flight or pending update work when updates are being disabled at runtime. The default cancels a pending
    /// update; platform services override this to also abort in-flight checks/downloads.
    fn cancel_update(&self, core: &Rc<AbstractUpdateService>) -> Promise<()> {
        core.cancel_update_default()
    }

    /// `protected async doIsLatestVersion(commit?, token)`.
    fn do_is_latest_version(&self, core: &Rc<AbstractUpdateService>, commit: Option<&str>, token: CancellationToken) -> Promise<Option<bool>> {
        core.do_is_latest_version_default(commit, token)
    }

    /// `protected getUpdateType()`.
    fn get_update_type(&self) -> UpdateType {
        UpdateType::Archive
    }

    /// `async _applySpecificUpdate(packagePath)`.
    fn apply_specific_update(&self, _core: &Rc<AbstractUpdateService>, _package_path: &str) -> Promise<()> {
        Promise::resolved(())
    }
}

/// `IInternalUpdateState`.
struct InternalUpdateState {
    state: State,
    deferred: bool,
}

/// `AbstractUpdateService`.
pub struct AbstractUpdateService {
    this: Weak<AbstractUpdateService>,
    base: Rc<Disposable>,
    hooks: Rc<dyn AbstractUpdateServiceHooks>,

    pub lifecycle_main_service: Rc<dyn LifecycleMainService>,
    pub configuration_service: Rc<dyn IConfigurationService>,
    pub environment_main_service: Rc<dyn NativeEnvironmentServiceApi>,
    pub request_service: Rc<dyn IRequestService>,
    pub log_service: LogServiceRef,
    pub product_service: Rc<dyn IProductService>,
    pub telemetry_service: Rc<dyn ITelemetryService>,
    pub application_storage_main_service: Rc<ApplicationStorageMainService>,
    pub metered_connection_service: Rc<dyn IMeteredConnectionService>,
    pub supports_update_overwrite: bool,

    quality: RefCell<Option<String>>,
    internal_state: RefCell<InternalUpdateState>,
    /// `_overwrite`.
    pub overwrite: Cell<bool>,
    has_checked_for_overwrite_on_quit: Cell<bool>,
    overwrite_updates_check_interval: Rc<IntervalTimer>,
    internal_org: RefCell<Option<String>>,

    /// Disabled for a non-reversible reason (e.g. not built, missing config); ignores `update.mode` changes.
    disabled_permanently: Cell<bool>,
    /// Whether one-time platform init (e.g. background update GC, pending update resume) has run.
    post_initialized: Cell<bool>,
    /// Cancels the pending scheduled update check, if any.
    scheduler: Rc<MutableDisposable<dyn IDisposable>>,
    /// Serializes reconfiguration so overlapping `update.mode` changes settle on the latest value.
    reconfigure_throttler: Rc<Throttler<()>>,

    on_state_change: Rc<Emitter<State>>,
}

impl AbstractUpdateService {
    /// O construtor do original: assim que a janela abre (`LifecycleMainPhase.AfterWindowOpen`) o gancho `initialize`
    /// roda, e quando a conexão deixa de ser cobrada as atualizações automáticas são retomadas.
    #[allow(clippy::too_many_arguments)] // são as dependências injetadas do construtor do original
    pub fn new(
        lifecycle_main_service: Rc<dyn LifecycleMainService>,
        configuration_service: Rc<dyn IConfigurationService>,
        environment_main_service: Rc<dyn NativeEnvironmentServiceApi>,
        request_service: Rc<dyn IRequestService>,
        log_service: LogServiceRef,
        product_service: Rc<dyn IProductService>,
        telemetry_service: Rc<dyn ITelemetryService>,
        application_storage_main_service: Rc<ApplicationStorageMainService>,
        metered_connection_service: Rc<dyn IMeteredConnectionService>,
        supports_update_overwrite: bool,
        hooks: Rc<dyn AbstractUpdateServiceHooks>,
    ) -> Rc<Self> {
        let base = Disposable::new();
        let on_state_change = Emitter::new();
        base.register(on_state_change.clone());
        let overwrite_updates_check_interval = IntervalTimer::new();
        base.register(overwrite_updates_check_interval.clone());
        let scheduler = MutableDisposable::<dyn IDisposable>::new();
        base.register(scheduler.clone());
        let reconfigure_throttler = Rc::new(Throttler::new());
        base.register(reconfigure_throttler.clone());

        let this = Rc::new_cyclic(|this| Self {
            this: this.clone(),
            base,
            hooks,
            lifecycle_main_service,
            configuration_service,
            environment_main_service,
            request_service,
            log_service,
            product_service,
            telemetry_service,
            application_storage_main_service,
            metered_connection_service,
            supports_update_overwrite,
            quality: RefCell::new(None),
            internal_state: RefCell::new(InternalUpdateState { state: State::Uninitialized, deferred: false }),
            overwrite: Cell::new(false),
            has_checked_for_overwrite_on_quit: Cell::new(false),
            overwrite_updates_check_interval,
            internal_org: RefCell::new(None),
            disabled_permanently: Cell::new(false),
            post_initialized: Cell::new(false),
            scheduler,
            reconfigure_throttler,
            on_state_change,
        });

        let initializing = this.clone();
        this.lifecycle_main_service.when(LifecycleMainPhase::AfterWindowOpen).finally_with(move || initializing.hooks.initialize(&initializing));

        let weak = Rc::downgrade(&this);
        this.base.register(this.metered_connection_service.on_did_change_is_connection_metered().listen(move |is_metered| {
            if !*is_metered && let Some(this) = weak.upgrade() {
                this.resume_automatic_updates();
            }
        }));

        this
    }

    /// `this`, para as promessas e os temporizadores que sobrevivem à chamada.
    fn shared(&self) -> Rc<Self> {
        self.this.upgrade().expect("o serviço está vivo enquanto um método dele roda")
    }

    /// `protected quality`.
    pub fn quality(&self) -> Option<String> {
        self.quality.borrow().clone()
    }

    fn state_type(&self) -> StateType {
        self.internal_state.borrow().state.state_type()
    }

    fn is_deferred(&self) -> bool {
        self.internal_state.borrow().deferred
    }

    fn type_arg(&self) -> LogArg {
        text_arg(self.state_type().as_str())
    }

    /// `protected setState(state, options?)`.
    pub fn set_state(&self, state: State, deferred: Option<bool>) {
        if state.state_type() == StateType::Updating {
            self.log_service.trace_args("update#setState", &[text_arg(state.state_type().as_str())]);
        } else {
            self.log_service.info_args("update#setState", &[text_arg(state.state_type().as_str())]);
        }
        *self.internal_state.borrow_mut() = InternalUpdateState { state: state.clone(), deferred: deferred.unwrap_or(false) };
        self.on_state_change.fire(state.clone());

        // Clear transient one-time properties from Idle state after delivering the event.
        // This prevents new windows from seeing stale error/notAvailable messages.
        if let State::Idle(idle) = &state
            && (idle.error.as_deref().is_some_and(|error| !error.is_empty()) || idle.not_available == Some(true))
        {
            *self.internal_state.borrow_mut() = InternalUpdateState { state: State::idle(idle.update_type, None, None), deferred: false };
        }

        // Schedule 5-minute checks when in Ready state and overwrite is supported
        if self.supports_update_overwrite {
            if state.state_type() == StateType::Ready {
                let weak = self.this.clone();
                self.overwrite_updates_check_interval.cancel_and_set(
                    move || {
                        if let Some(this) = weak.upgrade() {
                            let _ = this.check_for_overwrite_updates(false);
                        }
                    },
                    OVERWRITE_CHECK_INTERVAL_MS,
                );
            } else {
                self.overwrite_updates_check_interval.cancel();
            }
        }
    }

    fn set_deferred(&self, deferred: bool) {
        self.internal_state.borrow_mut().deferred = deferred;
    }

    /// `update.mode`, quando é texto.
    fn update_mode(&self) -> Option<String> {
        dynamic_text(&self.configuration_service.get_value(Some(UPDATE_MODE_KEY), None))
    }

    fn get_product_quality(&self, update_mode: Option<&str>) -> Option<String> {
        if update_mode == Some("none") { None } else { self.product_service.quality().map(str::to_owned) }
    }

    /// `protected getInternalOrg()`.
    pub fn get_internal_org(&self) -> Option<String> {
        self.internal_org.borrow().clone()
    }

    /// The default `initialize`: this must be called before any other call. This is a performance optimization, to
    /// avoid using extra CPU cycles before first window open.
    /// https://github.com/microsoft/vscode/issues/89784
    pub fn initialize_default(self: &Rc<Self>) -> Promise<()> {
        let this = self.clone();
        async_fn(async move {
            if !this.environment_main_service.is_built() {
                this.set_disabled_permanently(DisablementReason::NotBuilt);
                return Ok(()); // updates are never enabled when running out of sources
            }

            this.track_version_change().await?;

            if this.environment_main_service.disable_updates() {
                this.set_disabled_permanently(DisablementReason::DisabledByEnvironment);
                this.log_service.info("update#ctor - updates are disabled by the environment");
                return Ok(());
            }

            if this.product_service.update_url().is_none_or(str::is_empty) || this.product_service.commit().is_none_or(str::is_empty) {
                this.set_disabled_permanently(DisablementReason::MissingConfiguration);
                this.log_service.info("update#ctor - updates are disabled as there is no update URL");
                return Ok(());
            }

            match this.metered_connection_service.when_connection_state_initialized() {
                Some(initialized) => initialized.await?,
                None => Promise::resolved(()).await?,
            }

            // React to runtime `update.mode`/policy changes so switching to/from `none` applies without a restart.
            let weak = Rc::downgrade(&this);
            this.base.register(this.configuration_service.on_did_change_configuration().listen(move |event| {
                if event.affects_configuration(UPDATE_MODE_KEY, None)
                    && let Some(this) = weak.upgrade()
                {
                    let log_service = this.log_service.clone();
                    let _ = this.reconfigure().catch(move |error| {
                        log_service.error_args(&LogMessage::from("update#reconfigure - failed to apply update mode change"), &[LogArg::Error(error)]);
                    });
                }
            }));

            // Apply the currently configured update mode.
            this.reconfigure().await
        })
    }

    /// Evaluates the current `update.mode` setting (and its policy) and brings the service into the matching state.
    /// Runs on startup and on every change, enabling or disabling updates without a restart.
    fn reconfigure(self: &Rc<Self>) -> Promise<()> {
        let this = self.clone();
        self.reconfigure_throttler.queue(move |_| this.do_reconfigure())
    }

    fn do_reconfigure(self: &Rc<Self>) -> Promise<()> {
        let this = self.clone();
        async_fn(async move {
            if this.disabled_permanently.get() {
                return Ok(());
            }

            let update_mode = this.update_mode();
            let policy_value = this.configuration_service.inspect(UPDATE_MODE_KEY, &ConfigurationOverrides::default()).policy_value();
            let policy_disables_updates = !policy_value.is_undefined() && this.get_product_quality(dynamic_text(&policy_value).as_deref()).is_none();
            let quality = this.get_product_quality(update_mode.as_deref());

            let Some(quality) = quality else {
                let reason = if policy_disables_updates { DisablementReason::Policy } else { DisablementReason::ManuallyDisabled };

                // Skip if already disabled for this reason, so a repeated write or policy refresh is a no-op.
                if matches!(this.state(), State::Disabled(disabled) if disabled.reason == reason) {
                    return Ok(());
                }

                this.disable(reason).await?;
                return Ok(());
            };

            let commit = this.product_service.commit().unwrap_or_default().to_owned();
            if this.hooks.build_update_feed_url(&this, &quality, &commit, None).is_none() {
                this.set_disabled_permanently(DisablementReason::InvalidConfiguration);
                this.log_service.info("update#ctor - updates are disabled as the update URL is badly formed");
                return Ok(());
            }

            *this.quality.borrow_mut() = Some(quality);

            // Move to Idle so one-time platform init (which may resume a pending update) can act; it requires Idle.
            if matches!(this.state_type(), StateType::Disabled | StateType::Uninitialized) {
                this.set_state(State::idle(this.hooks.get_update_type(), None, None), None);
            }

            // One-time platform init, gated behind updates being enabled so a pending update is never resumed under `none`.
            if !this.post_initialized.get() {
                this.hooks.post_initialize(&this).await?;
                this.post_initialized.set(true);
            }

            this.schedule_according_to_mode(update_mode.as_deref());
            Ok(())
        })
    }

    /// Disables updates for a reversible reason (user preference or policy), cancelling the scheduled check loop and any
    /// in-flight or pending update before moving to Disabled.
    fn disable(self: &Rc<Self>, reason: DisablementReason) -> Promise<()> {
        let this = self.clone();
        async_fn(async move {
            this.scheduler.clear();

            // Show a transient Cancelling state only when there is in-flight or pending work to tear down.
            if is_cancellable_state(this.state_type()) {
                this.set_state(State::Cancelling, None);
            }

            if let Err(error) = this.hooks.cancel_update(&this).await {
                this.log_service.warn_args(&LogMessage::from("update#disable - failed to cancel pending update"), &[LogArg::Error(error)]);
            }

            *this.quality.borrow_mut() = None;

            if reason == DisablementReason::Policy {
                this.log_service.info("update#disable - updates are disabled by policy");
            } else {
                this.log_service.info("update#disable - updates are disabled by user preference");
            }

            this.set_state(State::disabled(reason), None);
            Ok(())
        })
    }

    /// Disables updates for a non-reversible reason; subsequent `update.mode` changes are ignored.
    fn set_disabled_permanently(&self, reason: DisablementReason) {
        self.disabled_permanently.set(true);
        self.scheduler.clear();
        self.set_state(State::disabled(reason), None);
    }

    fn schedule_according_to_mode(self: &Rc<Self>, update_mode: Option<&str>) {
        self.scheduler.clear();

        if update_mode == Some("manual") {
            self.log_service.info("update#ctor - manual checks only; automatic updates are disabled by user preference");
            return;
        }

        if self.is_deferred() && !self.metered_connection_service.is_connection_metered() {
            self.resume_automatic_updates();
            return;
        }

        if self.state_type() != StateType::Idle {
            return;
        }
        self.set_deferred(false);

        if update_mode == Some("start") {
            self.log_service.info("update#ctor - startup checks only; automatic updates are disabled by user preference");

            // Check for updates only once after 30 seconds
            self.schedule_check_for_updates(INITIAL_CHECK_DELAY_MS, false);
        } else {
            // Start checking for updates after 30 seconds
            self.schedule_check_for_updates(INITIAL_CHECK_DELAY_MS, true);
        }
    }

    fn resume_automatic_updates(self: &Rc<Self>) {
        if self.disabled_permanently.get() || !self.post_initialized.get() || self.quality.borrow().is_none() {
            return;
        }

        let update_mode = self.update_mode();
        if matches!(update_mode.as_deref(), Some("none" | "manual")) {
            return;
        }

        match self.state_type() {
            StateType::AvailableForDownload => {
                if self.is_deferred() {
                    self.hooks.resume_deferred_download(self);
                }
                return;
            }
            StateType::Ready => {
                if self.is_deferred() {
                    let _ = self.check_for_overwrite_updates(false);
                }
                return;
            }
            StateType::Idle => {}
            _ => return,
        }

        if update_mode.as_deref() == Some("start") && !self.is_deferred() {
            return;
        }
        self.set_deferred(false);
        self.schedule_check_for_updates(0, update_mode.as_deref() == Some("default"));
    }

    fn track_version_change(self: &Rc<Self>) -> Promise<()> {
        let this = self.clone();
        async_fn(async move {
            this.application_storage_main_service.when_ready().await?;

            let application_storage = this.application_storage_main_service.service();
            let from: Option<Dynamic> = StorageService::get(&application_storage, LAST_KNOWN_VERSION_STORAGE_KEY, StorageScope::Application, None).and_then(|raw| {
                let mut errors = Vec::new();
                json::parse_str(&raw, &mut errors).filter(|_| errors.is_empty()).map(|value| from_json(&value))
            });

            let to_version = this.product_service.version().to_owned();
            let to_commit = this.product_service.commit().map(str::to_owned);
            let to_timestamp = runtime::date_now() as f64;

            let from_commit = from.as_ref().map_or(Dynamic::Undefined, |from| property(from, "commit"));
            let same_commit = match (&from_commit, &to_commit) {
                (Dynamic::String(from_commit), Some(to_commit)) => from_commit.to_string_lossy() == *to_commit,
                (Dynamic::Undefined, None) => true,
                _ => false,
            };
            if same_commit {
                return Ok(());
            }

            let mut to = vec![("version", Dynamic::string(&to_version))];
            if let Some(commit) = &to_commit {
                to.push(("commit", Dynamic::string(commit)));
            }
            to.push(("timestamp", Dynamic::Number(to_timestamp)));
            StorageService::store(
                &application_storage,
                LAST_KNOWN_VERSION_STORAGE_KEY,
                StorageValue::from(json_stringify(&Dynamic::object(to)).unwrap_or_default().as_str()),
                StorageScope::Application,
                StorageTarget::Machine,
            );

            let Some(from) = from.filter(Truthy::is_truthy) else {
                return Ok(());
            };

            let from_timestamp = match property(&from, "timestamp") {
                Dynamic::Number(timestamp) => timestamp,
                _ => f64::NAN,
            };
            this.telemetry_service.public_log2(
                "update:versionChanged",
                Some(Dynamic::object([
                    ("fromVersion", property(&from, "version")),
                    ("fromCommit", from_commit),
                    ("fromVersionTime", property(&from, "timestamp")),
                    ("toVersion", Dynamic::string(&to_version)),
                    ("toCommit", to_commit.as_deref().map_or(Dynamic::Undefined, Dynamic::string)),
                    ("timeToUpdateMs", Dynamic::Number(to_timestamp - from_timestamp)),
                    ("updateMode", this.configuration_service.get_value(Some(UPDATE_MODE_KEY), None)),
                ])),
            );
            Ok(())
        })
    }

    fn schedule_check_for_updates(self: &Rc<Self>, delay: u64, repeat: bool) {
        let promise = timeout(delay);
        let cancel = promise.clone();
        self.scheduler.set(Some(to_disposable(move || cancel.cancel())));

        let checking = self.clone();
        let repeating = self.clone();
        let log_service = self.log_service.clone();
        let _ = promise
            .then_promise(move |_| checking.check_for_updates(false))
            .then(move |_| {
                if repeat {
                    // Check again after 1 hour
                    repeating.schedule_check_for_updates(DEFAULT_CHECK_DELAY_MS, true);
                }
            })
            .catch(move |error| {
                if !is_cancellation_error(&error) {
                    log_service.error_args(&LogMessage::from(&error), &[]);
                }
            });
    }

    /// `protected deferAutomaticDownload(update, explicit)`.
    pub fn defer_automatic_download(&self, update: &Update, explicit: bool) -> bool {
        if explicit || !self.metered_connection_service.is_connection_metered() {
            return false;
        }

        self.log_service.info("update#deferAutomaticDownload - deferring download because connection is metered");
        self.set_state(State::available_for_download(update.clone(), None), Some(true));
        true
    }

    fn check_for_overwrite_updates(self: &Rc<Self>, explicit: bool) -> Promise<bool> {
        let this = self.clone();
        async_fn(async move {
            if this.state_type() != StateType::Ready {
                return Ok(false);
            }

            if this.defer_overwrite_check_if_metered(explicit) {
                return Ok(false);
            }

            this.set_deferred(false);
            let pending_update_commit = state_update(&this.state()).map(|update| update.version).unwrap_or_default();

            if pending_update_commit.is_empty() || pending_update_commit == "unknown" {
                return Ok(false);
            }

            let cts = CancellationTokenSource::new(None);
            let timeout_promise = {
                let cts = cts.clone();
                timeout_with_token(OVERWRITE_PROBE_TIMEOUT_MS, &cts.token()).then(move |_| {
                    cts.cancel();
                    None::<bool>
                })
            };
            let probe = this.hooks.do_is_latest_version(&this, Some(&pending_update_commit), cts.token());
            let raced = Promise::race(vec![probe, timeout_promise]).await;
            cts.dispose_with(true);
            let is_latest = match raced {
                Ok(is_latest) => is_latest,
                Err(error) => {
                    this.log_service.warn("update#checkForOverwriteUpdates(): failed to check for updates, proceeding with restart");
                    this.log_service.warn_args(&LogMessage::from(&error), &[]);
                    return Ok(false);
                }
            };

            if is_latest == Some(false) && this.state_type() == StateType::Ready {
                if this.defer_overwrite_check_if_metered(explicit) {
                    return Ok(false);
                }

                this.log_service.info("update#readyStateCheck: newer update available, restarting update machinery");

                if let Err(error) = this.hooks.cancel_pending_update(&this).await {
                    this.log_service.error("update#checkForOverwriteUpdates(): failed to cancel pending update, aborting overwrite");
                    this.log_service.error_args(&LogMessage::from(&error), &[]);
                    return Ok(false);
                }

                if this.defer_overwrite_check_if_metered(explicit) {
                    return Ok(false);
                }

                this.overwrite.set(true);
                let update = state_update(&this.state()).unwrap_or_default();
                this.set_state(State::overwriting(update, explicit), None);
                this.hooks.do_check_for_updates(&this, explicit, Some(&pending_update_commit));
                return Ok(true);
            }

            Ok(false)
        })
    }

    fn defer_overwrite_check_if_metered(&self, explicit: bool) -> bool {
        if explicit || !self.metered_connection_service.is_connection_metered() {
            return false;
        }

        self.set_deferred(true);
        self.log_service.info("update#checkForOverwriteUpdates - deferring overwrite because connection is metered");
        true
    }

    /// `isLatestVersion(commit?, token = CancellationToken.None)`: a forma completa do `isLatestVersion()` do contrato.
    pub fn is_latest_version_with(self: &Rc<Self>, commit: Option<&str>, token: CancellationToken) -> Promise<Option<bool>> {
        let this = self.clone();
        let commit = commit.map(str::to_owned);
        async_fn(async move {
            if this.metered_connection_service.is_connection_metered() {
                this.log_service.info("update#isLatestVersion - skipping automatic check because connection is metered");
                return Ok(None);
            }

            this.hooks.do_is_latest_version(&this, commit.as_deref(), token).await
        })
    }

    /// The default `doIsLatestVersion`: asks the update server whether the given commit (or the running one) is the latest.
    pub fn do_is_latest_version_default(self: &Rc<Self>, commit: Option<&str>, token: CancellationToken) -> Promise<Option<bool>> {
        let this = self.clone();
        let commit = commit.map(str::to_owned);
        async_fn(async move {
            let Some(quality) = this.quality() else {
                return Ok(None);
            };

            if this.update_mode().as_deref() == Some("none") {
                return Ok(None);
            }

            let commit = commit.unwrap_or_else(|| this.product_service.commit().unwrap_or_default().to_owned());
            let options = UpdateUrlOptions { background: None, internal_org: this.get_internal_org() };
            let Some(url) = this.hooks.build_update_feed_url(&this, &quality, &commit, Some(&options)) else {
                return Ok(None);
            };

            let headers = get_update_request_headers(this.product_service.version());
            this.log_service.trace_args(
                "update#isLatestVersion() - checking update server",
                &[LogArg::Value(Dynamic::object([
                    ("url", Dynamic::string(&url)),
                    ("headers", headers.as_ref().map_or(Dynamic::Undefined, headers_to_dynamic)),
                ]))],
            );

            let request = IRequestOptions { url: Some(url), headers, call_site: "updateService.isLatestVersion".to_string(), ..Default::default() };
            match this.request_service.request(request, token).await {
                Ok(context) => {
                    let status_code = context.res.status_code;
                    this.log_service.trace_args(
                        "update#isLatestVersion() - response",
                        &[LogArg::Value(Dynamic::object([("statusCode", status_code.map_or(Dynamic::Undefined, |code| Dynamic::Number(f64::from(code))))]))],
                    );
                    // The update server replies with 204 (No Content) when no update is available.
                    Ok(Some(status_code == Some(204)))
                }
                Err(error) => {
                    this.log_service.error("update#isLatestVersion(): failed to check for updates");
                    this.log_service.error_args(&LogMessage::from(&error), &[]);
                    Ok(None)
                }
            }
        })
    }

    /// The default `cancelUpdate`: cancels a pending update.
    pub fn cancel_update_default(self: &Rc<Self>) -> Promise<()> {
        self.hooks.cancel_pending_update(self)
    }
}

impl UpdateService for AbstractUpdateService {
    fn on_state_change(&self) -> Event<State> {
        self.on_state_change.event()
    }

    fn state(&self) -> State {
        self.internal_state.borrow().state.clone()
    }

    fn check_for_updates(&self, explicit: bool) -> Promise<()> {
        let this = self.shared();
        async_fn(async move {
            this.log_service.trace_args("update#checkForUpdates, state = ", &[this.type_arg()]);

            if this.state_type() != StateType::Idle {
                return Ok(());
            }

            if !explicit && this.metered_connection_service.is_connection_metered() {
                this.set_deferred(true);
                this.log_service.info("update#checkForUpdates - skipping automatic check because connection is metered");
                return Ok(());
            }

            this.set_deferred(false);
            this.hooks.do_check_for_updates(&this, explicit, None);
            Ok(())
        })
    }

    fn download_update(&self, explicit: bool) -> Promise<()> {
        let this = self.shared();
        async_fn(async move {
            this.log_service.trace_args("update#downloadUpdate, state = ", &[this.type_arg()]);

            let State::AvailableForDownload(available) = this.state() else {
                return Ok(());
            };

            if !explicit && this.metered_connection_service.is_connection_metered() {
                this.set_deferred(true);
                this.log_service.info("update#downloadUpdate - skipping download because connection is metered");
                return Ok(());
            }

            this.set_deferred(false);
            this.hooks.do_download_update(&this, &available).await
        })
    }

    fn apply_update(&self) -> Promise<()> {
        let this = self.shared();
        async_fn(async move {
            this.log_service.trace_args("update#applyUpdate, state = ", &[this.type_arg()]);

            if this.state_type() != StateType::Downloaded {
                return Ok(());
            }

            this.hooks.do_apply_update(&this).await
        })
    }

    fn quit_and_install(&self) -> Promise<()> {
        let this = self.shared();
        async_fn(async move {
            this.log_service.trace_args("update#quitAndInstall, state = ", &[this.type_arg()]);

            if this.state_type() != StateType::Ready {
                return Ok(());
            }

            if this.supports_update_overwrite && !this.has_checked_for_overwrite_on_quit.get() {
                this.has_checked_for_overwrite_on_quit.set(true);
                let did_overwrite = this.check_for_overwrite_updates(true).await?;

                if did_overwrite {
                    this.log_service.info("update#quitAndInstall(): overwrite update detected, postponing quitAndInstall");
                    return Ok(());
                }
            }

            // Remember the Ready state so we can restore it if the quit is vetoed
            let ready_state = this.state();

            this.set_state(State::restarting(state_update(&ready_state).unwrap_or_default()), None);
            this.log_service.trace("update#quitAndInstall(): before lifecycle quit()");

            let quitting = this.clone();
            let _ = this.lifecycle_main_service.quit(true /* will restart */).then(move |vetoed| {
                quitting.log_service.trace(&format!("update#quitAndInstall(): after lifecycle quit() with veto: {vetoed}"));
                if vetoed {
                    quitting.log_service.info("update#quitAndInstall(): quit was vetoed, restoring Ready state");
                    quitting.set_state(ready_state, None);
                    return;
                }

                quitting.log_service.trace("update#quitAndInstall(): running raw#quitAndInstall()");
                quitting.hooks.do_quit_and_install(&quitting);
            });

            Ok(())
        })
    }

    fn is_latest_version(&self) -> Promise<Option<bool>> {
        self.shared().is_latest_version_with(None, CancellationToken::None)
    }

    fn apply_specific_update(&self, package_path: &str) -> Promise<()> {
        self.hooks.apply_specific_update(&self.shared(), package_path)
    }

    fn set_internal_org(&self, internal_org: Option<&str>) -> Promise<()> {
        if self.internal_org.borrow().as_deref() != internal_org {
            self.log_service.info_args("update#setInternalOrg", &[internal_org.map_or(LogArg::Value(Dynamic::Undefined), text_arg)]);
            *self.internal_org.borrow_mut() = internal_org.map(str::to_owned);
        }
        Promise::resolved(())
    }
}

impl IDisposable for AbstractUpdateService {
    fn dispose(&self) {
        self.base.dispose();
    }
}
