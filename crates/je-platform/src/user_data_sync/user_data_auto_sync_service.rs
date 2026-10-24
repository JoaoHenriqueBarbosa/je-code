//! Porte de `src/vs/platform/userDataSync/common/userDataAutoSyncService.ts`: a sincronização automática
//! (`UserDataAutoSyncService`) e o executor de um ciclo de sincronização (`AutoSync`).
//!
//! Divergências:
//! - Os ganchos de teste `startAutoSync()` e `getSyncTriggerDelayTime()`, que o teste sobrescreve numa subclasse,
//!   são os campos de [`UserDataAutoSyncOverrides`]: `start_auto_sync` falso não inicia o ciclo e
//!   `sync_trigger_delay_time` substitui o cálculo do atraso (o `ThrottledDelayer` do construtor também o usa, como o
//!   construtor da base chama o método sobrescrito).
//! - `isWeb` é falso: o ramo web de `ServiceChanged`/`DefaultServiceChanged` (e o `hasProductQualityChanged` que só ele
//!   lê) não existe; o `else` (desligar, ligar de novo) sempre vale. A qualidade do produto continua gravada em
//!   `sync.productQuality`.
//! - O serviço de sincronização é o [`UserDataSyncService`] concreto (que não implementa o trait inteiro) e o
//!   `onError` entrega o [`Error`] (ver [`IUserDataAutoSyncService`]).
//! - `turnOff(everywhere, softTurnOffOnError?, donotRemoveMachine?)` é [`UserDataAutoSyncService::turn_off_with`]; o
//!   `turn_off` do trait usa os dois opcionais ausentes.
//! - `disableMachineEventually` não cancela o `timeout` ao descartar o serviço (o original também não).

use std::cell::{Cell, RefCell};
use std::rc::{Rc, Weak};

use je_base::async_::{ThrottledDelayer, async_fn, create_cancelable_promise, disposable_timeout, timeout};
use je_base::cancellation::CancellationToken;
use je_base::date::{local_time_zone, to_local_iso_string};
use je_base::dynamic::Dynamic;
use je_base::error_message::to_error_message_of;
use je_base::errors::{Error, is_cancellation_error};
use je_base::event::{Emitter, Event};
use je_base::lifecycle::{Disposable, DisposableRef, IDisposable, MutableDisposable, to_disposable};
use je_base::nls::localize;
use je_base::promise::{CancelablePromise, Promise};
use je_base::runtime::date_now;
use je_base::uri::URI;
use je_storage::StorageValue;

use super::user_data_sync::{
    IUserDataAutoSyncService, IUserDataManifest, IUserDataSyncEnablementService, IUserDataSyncLogService, IUserDataSyncStoreManagementService, IUserDataSyncStoreService, IUserDataSyncTask,
    SyncOptions, UserDataAutoSyncError, UserDataSyncError, UserDataSyncErrorCode, to_user_data_sync_error,
};
use super::user_data_sync_account::IUserDataSyncAccountService;
use super::user_data_sync_machines::IUserDataSyncMachinesService;
use super::user_data_sync_service::{UserDataSyncService, to_sync_error};
use crate::log::{LogArg, LogMessage};
use crate::metered_connection::IMeteredConnectionService;
use crate::product::IProductService;
use crate::resources::ext_uri;
use crate::storage::{StorageScope, StorageService, StorageTarget};
use crate::telemetry::ITelemetryService;

const DISABLE_MACHINE_EVENTUALLY_KEY: &str = "sync.disableMachineEventually";
const SESSION_ID_KEY: &str = "sync.sessionId";
const STORE_URL_KEY: &str = "sync.storeUrl";
const PRODUCT_QUALITY_KEY: &str = "sync.productQuality";

const INTERVAL_SYNCING: &str = "Interval";

/// Os serviços que o construtor recebe por injeção.
#[derive(Clone)]
pub struct UserDataAutoSyncServices {
    pub product_service: Rc<dyn IProductService>,
    pub user_data_sync_store_management_service: Rc<dyn IUserDataSyncStoreManagementService>,
    pub user_data_sync_store_service: Rc<dyn IUserDataSyncStoreService>,
    pub user_data_sync_enablement_service: Rc<dyn IUserDataSyncEnablementService>,
    pub user_data_sync_service: Rc<UserDataSyncService>,
    pub log_service: IUserDataSyncLogService,
    pub user_data_sync_account_service: Rc<dyn IUserDataSyncAccountService>,
    pub telemetry_service: Rc<dyn ITelemetryService>,
    pub user_data_sync_machines_service: Rc<dyn IUserDataSyncMachinesService>,
    pub storage_service: Rc<dyn StorageService>,
    pub metered_connection_service: Rc<dyn IMeteredConnectionService>,
}

/// Os ganchos que o teste sobrescreve (`startAutoSync` e `getSyncTriggerDelayTime`).
#[derive(Clone, Copy, Debug)]
pub struct UserDataAutoSyncOverrides {
    pub start_auto_sync: bool,
    pub sync_trigger_delay_time: Option<u64>,
}

impl Default for UserDataAutoSyncOverrides {
    fn default() -> Self {
        Self { start_auto_sync: true, sync_trigger_delay_time: None }
    }
}

fn is_equal(a: Option<&URI>, b: Option<&URI>) -> bool {
    ext_uri().is_equal(a, b, false)
}

fn uri_text(uri: &URI) -> String {
    uri.to_string(false).to_string_lossy()
}

// region: UserDataAutoSyncService

/// `UserDataAutoSyncService`.
pub struct UserDataAutoSyncService {
    me: Weak<Self>,
    base: Rc<Disposable>,
    services: UserDataAutoSyncServices,
    overrides: UserDataAutoSyncOverrides,
    auto_sync: Rc<MutableDisposable<AutoSync>>,
    successive_failures: Cell<u64>,
    last_sync_trigger_time: Cell<Option<f64>>,
    sync_trigger_delayer: Rc<ThrottledDelayer<()>>,
    suspend_until_restart: Cell<bool>,
    on_error: Rc<Emitter<Error>>,
    last_sync_url: RefCell<Option<URI>>,
    sources: RefCell<Vec<String>>,
}

impl UserDataAutoSyncService {
    pub fn new(services: UserDataAutoSyncServices, overrides: UserDataAutoSyncOverrides) -> Rc<Self> {
        let base = Disposable::new();
        let on_error = Emitter::new();
        base.register(on_error.clone());
        let auto_sync = MutableDisposable::<AutoSync>::new();
        base.register(auto_sync.clone());
        // `getSyncTriggerDelayTime()` sem `lastSyncTriggerTime`: 3 segundos
        let sync_trigger_delayer = Rc::new(ThrottledDelayer::<()>::new(overrides.sync_trigger_delay_time.unwrap_or(3_000)));
        base.register(sync_trigger_delayer.clone());

        let this = Rc::new_cyclic(|me| Self {
            me: me.clone(),
            base,
            services,
            overrides,
            auto_sync,
            successive_failures: Cell::new(0),
            last_sync_trigger_time: Cell::new(None),
            sync_trigger_delayer,
            suspend_until_restart: Cell::new(false),
            on_error,
            last_sync_url: RefCell::new(None),
            sources: RefCell::new(Vec::new()),
        });

        *this.last_sync_url.borrow_mut() = this.sync_url();
        this.set_sync_url(this.services.user_data_sync_store_management_service.user_data_sync_store().map(|store| store.url).as_ref());

        this.set_product_quality(this.services.product_service.quality());

        if let Some(sync_url) = this.sync_url() {
            let services = &this.services;
            services.log_service.info_args("[AutoSync] Using settings sync service", &[LogArg::from(uri_text(&sync_url).as_str())]);

            let weak = this.me.clone();
            this.base.register(services.user_data_sync_store_management_service.on_did_change_user_data_sync_store().listen(move |_| {
                let Some(this) = weak.upgrade() else { return };
                let new_url = this.services.user_data_sync_store_management_service.user_data_sync_store().map(|store| store.url);
                if !is_equal(this.sync_url().as_ref(), new_url.as_ref()) {
                    *this.last_sync_url.borrow_mut() = this.sync_url();
                    this.set_sync_url(new_url.as_ref());
                    if let Some(sync_url) = this.sync_url() {
                        this.services.log_service.info_args("[AutoSync] Using settings sync service", &[LogArg::from(uri_text(&sync_url).as_str())]);
                    }
                }
            }));

            if services.user_data_sync_enablement_service.is_enabled() {
                services.log_service.info("[AutoSync] Enabled.");
            } else {
                services.log_service.info("[AutoSync] Disabled.");
            }
            this.update_auto_sync();

            if this.has_to_disable_machine_eventually() {
                let _ = this.disable_machine_eventually();
            }

            let weak = this.me.clone();
            this.base.register(services.user_data_sync_account_service.on_did_change_account().listen({
                let weak = weak.clone();
                move |_| {
                    if let Some(this) = weak.upgrade() {
                        this.update_auto_sync();
                    }
                }
            }));
            this.base.register(services.user_data_sync_store_service.on_did_change_donot_make_requests_until().listen({
                let weak = weak.clone();
                move |_| {
                    if let Some(this) = weak.upgrade() {
                        this.update_auto_sync();
                    }
                }
            }));
            this.base.register(services.user_data_sync_service.on_did_change_local().listen({
                let weak = weak.clone();
                move |source| {
                    if let Some(this) = weak.upgrade() {
                        let _ = this.trigger_sync(&[source.as_str().to_string()], None);
                    }
                }
            }));
            this.base.register(services.user_data_sync_enablement_service.on_did_change_resource_enablement().listen({
                let weak = weak.clone();
                move |(_, enabled)| {
                    if !*enabled {
                        return;
                    }
                    if let Some(this) = weak.upgrade() {
                        let _ = this.trigger_sync(&["resourceEnablement".to_string()], None);
                    }
                }
            }));
            this.base.register(services.user_data_sync_store_management_service.on_did_change_user_data_sync_store().listen({
                let weak = weak.clone();
                move |_| {
                    if let Some(this) = weak.upgrade() {
                        let _ = this.trigger_sync(&["userDataSyncStoreChanged".to_string()], None);
                    }
                }
            }));
            this.base.register(services.metered_connection_service.on_did_change_is_connection_metered().listen(move |_| {
                if let Some(this) = weak.upgrade() {
                    this.update_auto_sync();
                }
            }));
        }
        this
    }

    fn rc(&self) -> Rc<Self> {
        self.me.upgrade().expect("o serviço de sincronização automática está vivo enquanto é usado")
    }

    /// `onError`.
    pub fn on_error(&self) -> Event<Error> {
        self.on_error.event()
    }

    fn sync_url(&self) -> Option<URI> {
        self.services.storage_service.get(STORE_URL_KEY, StorageScope::Application, None).filter(|value| !value.is_empty()).map(|value| URI::parse(value.as_str(), false))
    }

    fn set_sync_url(&self, sync_url: Option<&URI>) {
        match sync_url {
            Some(sync_url) => self.services.storage_service.store(STORE_URL_KEY, StorageValue::from(uri_text(sync_url).as_str()), StorageScope::Application, StorageTarget::Machine),
            None => self.services.storage_service.remove(STORE_URL_KEY, StorageScope::Application),
        }
    }

    fn set_product_quality(&self, product_quality: Option<&str>) {
        match product_quality.filter(|quality| !quality.is_empty()) {
            Some(quality) => self.services.storage_service.store(PRODUCT_QUALITY_KEY, StorageValue::from(quality), StorageScope::Application, StorageTarget::Machine),
            None => self.services.storage_service.remove(PRODUCT_QUALITY_KEY, StorageScope::Application),
        }
    }

    fn update_auto_sync(&self) {
        let (enabled, message) = self.is_auto_sync_enabled();
        if enabled {
            if self.auto_sync.value().is_none() {
                let auto_sync = AutoSync::new(self.last_sync_url.borrow().clone(), 1000 * 60 * 5 /* 5 miutes */, self.services.clone());
                self.auto_sync.set(Some(auto_sync.clone()));
                let weak = self.me.clone();
                auto_sync.register(auto_sync.on_did_start_sync().listen({
                    let weak = weak.clone();
                    move |_| {
                        if let Some(this) = weak.upgrade() {
                            this.last_sync_trigger_time.set(Some(date_now() as f64));
                        }
                    }
                }));
                auto_sync.register(auto_sync.on_did_finish_sync().listen(move |error| {
                    if let Some(this) = weak.upgrade() {
                        let _ = this.on_did_finish_sync(error.clone());
                    }
                }));
                if self.overrides.start_auto_sync {
                    auto_sync.start();
                }
            }
        } else {
            self.sync_trigger_delayer.cancel();
            if self.auto_sync.value().is_some() {
                if let Some(message) = message {
                    self.services.log_service.info(&message);
                }
                self.auto_sync.clear();
            }
            /* log message when auto sync is not disabled by user */
            else if let Some(message) = message
                && self.services.user_data_sync_enablement_service.is_enabled()
            {
                self.services.log_service.info(&message);
            }
        }
    }

    fn is_auto_sync_enabled(&self) -> (bool, Option<String>) {
        if !self.services.user_data_sync_enablement_service.is_enabled() {
            return (false, Some("[AutoSync] Disabled.".to_string()));
        }
        if self.services.user_data_sync_account_service.account().is_none() {
            return (false, Some("[AutoSync] Suspended until auth token is available.".to_string()));
        }
        if let Some(donot_make_requests_until) = self.services.user_data_sync_store_service.donot_make_requests_until() {
            return (
                false,
                Some(format!(
                    "[AutoSync] Suspended until {} because server is not accepting requests until then.",
                    to_local_iso_string(donot_make_requests_until, &local_time_zone())
                )),
            );
        }
        if self.suspend_until_restart.get() {
            return (false, Some("[AutoSync] Suspended until restart.".to_string()));
        }
        if self.services.metered_connection_service.is_connection_metered() {
            return (false, Some("[AutoSync] Suspended because connection is metered.".to_string()));
        }
        (true, None)
    }

    /// `turnOff(everywhere, softTurnOffOnError?, donotRemoveMachine?)`.
    pub fn turn_off_with(&self, everywhere: bool, soft_turn_off_on_error: bool, donot_remove_machine: bool) -> Promise<()> {
        let this = self.rc();
        async_fn(async move {
            let result: Result<(), Error> = async {
                // Remove machine
                if this.services.user_data_sync_account_service.account().is_some() && !donot_remove_machine {
                    this.services.user_data_sync_machines_service.remove_current_machine(None).await?;
                }

                // Disable Auto Sync
                this.update_enablement(false);

                // Reset Session
                this.services.storage_service.remove(SESSION_ID_KEY, StorageScope::Application);

                // Reset
                if everywhere {
                    this.services.user_data_sync_service.reset().await?;
                } else {
                    this.services.user_data_sync_service.reset_local().await?;
                }
                Ok(())
            }
            .await;
            if let Err(error) = result {
                this.services.log_service.error_args(&LogMessage::from(&error), &[]);
                if soft_turn_off_on_error {
                    this.update_enablement(false);
                } else {
                    return Err(error);
                }
            }
            Ok(())
        })
    }

    fn update_enablement(&self, enabled: bool) {
        if self.services.user_data_sync_enablement_service.is_enabled() != enabled {
            self.services.user_data_sync_enablement_service.set_enablement(enabled);
            self.update_auto_sync();
        }
    }

    fn on_did_finish_sync(&self, error: Option<Error>) -> Promise<()> {
        let this = self.rc();
        async_fn(async move {
            this.services.log_service.debug("[AutoSync] Sync Finished");
            let Some(error) = error else {
                // Sync finished without errors
                this.successive_failures.set(0);
                return Ok(());
            };

            // Error while syncing
            let user_data_sync_error = to_user_data_sync_error(&error);
            let log = |message: &str| this.services.log_service.info(message);

            match user_data_sync_error.code {
                // Session got expired
                UserDataSyncErrorCode::SessionExpired => {
                    this.turn_off_with(false, true /* force soft turnoff on error */, false).await?;
                    log("[AutoSync] Turned off sync because current session is expired");
                }

                // Turned off from another device
                UserDataSyncErrorCode::TurnedOff => {
                    this.turn_off_with(false, true /* force soft turnoff on error */, false).await?;
                    log("[AutoSync] Turned off sync because sync is turned off in the cloud");
                }

                // Exceeded Rate Limit on Client
                UserDataSyncErrorCode::LocalTooManyRequests => {
                    this.suspend_until_restart.set(true);
                    log("[AutoSync] Suspended sync because of making too many requests to server");
                    this.update_auto_sync();
                }

                // Exceeded Rate Limit on Server
                UserDataSyncErrorCode::TooManyRequests => {
                    this.turn_off_with(
                        false,
                        true, /* force soft turnoff on error */
                        true, /* do not disable machine because disabling a machine makes request to server and can fail with TooManyRequests */
                    )
                    .await?;
                    let _ = this.disable_machine_eventually();
                    log("[AutoSync] Turned off sync because of making too many requests to server");
                }

                // Method Not Found
                UserDataSyncErrorCode::MethodNotFound => {
                    this.turn_off_with(false, true /* force soft turnoff on error */, false).await?;
                    log("[AutoSync] Turned off sync because current client is making requests to server that are not supported");
                }

                // Upgrade Required or Gone
                UserDataSyncErrorCode::UpgradeRequired | UserDataSyncErrorCode::Gone => {
                    this.turn_off_with(
                        false,
                        true, /* force soft turnoff on error */
                        true, /* do not disable machine because disabling a machine makes request to server and can fail with upgrade required or gone */
                    )
                    .await?;
                    let _ = this.disable_machine_eventually();
                    log("[AutoSync] Turned off sync because current client is not compatible with server. Requires client upgrade.");
                }

                // Incompatible Local Content
                UserDataSyncErrorCode::IncompatibleLocalContent => {
                    this.turn_off_with(false, true /* force soft turnoff on error */, false).await?;
                    log(&format!(
                        "[AutoSync] Turned off sync because server has {} content with newer version than of client. Requires client upgrade.",
                        user_data_sync_error.resource.map_or("undefined", |resource| resource.as_str())
                    ));
                }

                // Incompatible Remote Content
                UserDataSyncErrorCode::IncompatibleRemoteContent => {
                    this.turn_off_with(false, true /* force soft turnoff on error */, false).await?;
                    log(&format!(
                        "[AutoSync] Turned off sync because server has {} content with older version than of client. Requires server reset.",
                        user_data_sync_error.resource.map_or("undefined", |resource| resource.as_str())
                    ));
                }

                // Service changed
                UserDataSyncErrorCode::ServiceChanged | UserDataSyncErrorCode::DefaultServiceChanged => {
                    // Service has changed by the user. So turn off and turn on sync.
                    // Show a prompt to the user about service change.
                    this.turn_off_with(false, true /* force soft turnoff on error */, true /* do not disable machine */).await?;
                    this.turn_on().await?;
                    log("[AutoSync] Sync Service changed. Turned off auto sync, reset local state and turned on auto sync.");
                }

                _ => {
                    this.services.log_service.error_args(&LogMessage::from(&error), &[]);
                    this.successive_failures.set(this.successive_failures.get() + 1);
                }
            }

            this.on_error.fire(to_sync_error(&error));
            Ok(())
        })
    }

    fn disable_machine_eventually(&self) -> Promise<()> {
        self.services.storage_service.store(DISABLE_MACHINE_EVENTUALLY_KEY, StorageValue::from(true), StorageScope::Application, StorageTarget::Machine);
        let this = self.rc();
        async_fn(async move {
            timeout(1000 * 60 * 10).await?;

            // Return if got stopped meanwhile.
            if !this.has_to_disable_machine_eventually() {
                return Ok(());
            }

            this.stop_disable_machine_eventually();

            // disable only if sync is disabled
            if !this.services.user_data_sync_enablement_service.is_enabled() && this.services.user_data_sync_account_service.account().is_some() {
                this.services.user_data_sync_machines_service.remove_current_machine(None).await?;
            }
            Ok(())
        })
    }

    fn has_to_disable_machine_eventually(&self) -> bool {
        self.services.storage_service.get_boolean(DISABLE_MACHINE_EVENTUALLY_KEY, StorageScope::Application, Some(false)).unwrap_or(false)
    }

    fn stop_disable_machine_eventually(&self) {
        self.services.storage_service.remove(DISABLE_MACHINE_EVENTUALLY_KEY, StorageScope::Application);
    }

    /// `getSyncTriggerDelayTime`.
    fn get_sync_trigger_delay_time(&self) -> u64 {
        if let Some(delay) = self.overrides.sync_trigger_delay_time {
            return delay;
        }
        if let Some(last_sync_trigger_time) = self.last_sync_trigger_time.get().filter(|time| *time != 0.0)
            && date_now() as f64 - last_sync_trigger_time > 10_000.0
        {
            self.services.log_service.debug("[AutoSync] Sync immediately because last sync was triggered more than 10 seconds ago.");
            return 0;
        }
        3_000 /* Debounce for 3 seconds if there are no failures */
    }

    /// `triggerSync`.
    pub fn trigger_sync(&self, sources: &[String], options: Option<SyncOptions>) -> Promise<()> {
        if self.auto_sync.value().is_none() {
            self.sync_trigger_delayer.cancel();
            return Promise::resolved(());
        }

        let options = options.unwrap_or_default();
        if options.skip_if_synced_recently == Some(true)
            && let Some(last_sync_trigger_time) = self.last_sync_trigger_time.get().filter(|time| *time != 0.0)
            && date_now() as f64 - last_sync_trigger_time < 10_000.0
        {
            let args: Vec<LogArg> = sources.iter().map(|source| LogArg::from(source.as_str())).collect();
            self.services.log_service.debug_args("[AutoSync] Skipping because sync was triggered recently.", &args);
            return Promise::resolved(());
        }

        self.sources.borrow_mut().extend(sources.iter().cloned());
        let this = self.rc();
        let disable_cache = options.disable_cache == Some(true);
        let failures = self.successive_failures.get();
        let delay = if failures != 0 {
            (self.get_sync_trigger_delay_time() * failures).min(60_000) /* Delay linearly until max 1 minute */
        } else if options.immediately == Some(true) {
            0
        } else {
            self.get_sync_trigger_delay_time()
        };
        self.sync_trigger_delayer.trigger(
            move |_token| {
                async_fn(async move {
                    let sources = std::mem::take(&mut *this.sources.borrow_mut());
                    let args: Vec<LogArg> = sources.iter().map(|source| LogArg::from(source.as_str())).collect();
                    this.services.log_service.trace_args("[AutoSync] Activity sources", &args);
                    if let Some(auto_sync) = this.auto_sync.value() {
                        auto_sync.sync("Activity", disable_cache).await?;
                    }
                    Ok(())
                })
            },
            Some(delay),
        )
    }
}

impl IDisposable for UserDataAutoSyncService {
    fn dispose(&self) {
        self.base.dispose();
    }
}

impl IUserDataAutoSyncService for UserDataAutoSyncService {
    fn on_error(&self) -> Event<Error> {
        self.on_error.event()
    }

    fn turn_on(&self) -> Promise<()> {
        self.stop_disable_machine_eventually();
        *self.last_sync_url.borrow_mut() = self.sync_url();
        self.update_enablement(true);
        Promise::resolved(())
    }

    fn turn_off(&self, everywhere: bool) -> Promise<()> {
        self.turn_off_with(everywhere, false, false)
    }

    fn trigger_sync(&self, sources: &[String], options: Option<SyncOptions>) -> Promise<()> {
        UserDataAutoSyncService::trigger_sync(self, sources, options)
    }
}

// endregion

// region: AutoSync

/// `AutoSync`: um ciclo de sincronização (a primeira, a periódica e as de atividade).
struct AutoSync {
    me: Weak<Self>,
    base: Rc<Disposable>,
    interval_handler: Rc<MutableDisposable<dyn IDisposable>>,
    on_did_start_sync: Rc<Emitter<()>>,
    on_did_finish_sync: Rc<Emitter<Option<Error>>>,
    manifest: RefCell<Option<IUserDataManifest>>,
    sync_task: RefCell<Option<Rc<dyn IUserDataSyncTask>>>,
    sync_promise: RefCell<Option<CancelablePromise<()>>>,
    last_sync_url: Option<URI>,
    /// Em milissegundos.
    interval: u64,
    services: UserDataAutoSyncServices,
}

impl AutoSync {
    fn new(last_sync_url: Option<URI>, interval: u64, services: UserDataAutoSyncServices) -> Rc<Self> {
        let base = Disposable::new();
        let interval_handler = MutableDisposable::<dyn IDisposable>::new();
        base.register(interval_handler.clone());
        let on_did_start_sync = Emitter::new();
        base.register(on_did_start_sync.clone());
        let on_did_finish_sync = Emitter::new();
        base.register(on_did_finish_sync.clone());
        Rc::new_cyclic(|me| Self {
            me: me.clone(),
            base,
            interval_handler,
            on_did_start_sync,
            on_did_finish_sync,
            manifest: RefCell::new(None),
            sync_task: RefCell::new(None),
            sync_promise: RefCell::new(None),
            last_sync_url,
            interval,
            services,
        })
    }

    fn rc(&self) -> Rc<Self> {
        self.me.upgrade().expect("o ciclo de sincronização está vivo enquanto é usado")
    }

    fn register(&self, disposable: DisposableRef) {
        self.base.register(disposable);
    }

    fn on_did_start_sync(&self) -> Event<()> {
        self.on_did_start_sync.event()
    }

    fn on_did_finish_sync(&self) -> Event<Option<Error>> {
        self.on_did_finish_sync.event()
    }

    fn start(&self) {
        let weak = self.me.clone();
        self.base.register(self.on_did_finish_sync().listen(move |_| {
            if let Some(this) = weak.upgrade() {
                this.wait_until_next_interval_and_sync();
            }
        }));
        let weak = self.me.clone();
        self.base.register(to_disposable(move || {
            let Some(this) = weak.upgrade() else { return };
            let sync_promise = this.sync_promise.borrow_mut().take();
            if let Some(sync_promise) = sync_promise {
                sync_promise.cancel();
                this.services.log_service.info("[AutoSync] Cancelled sync that is in progress");
            }
            let sync_task = this.sync_task.borrow().clone();
            if let Some(sync_task) = sync_task {
                let _ = sync_task.stop();
            }
            this.services.log_service.info("[AutoSync] Stopped");
        }));
        let _ = self.sync(INTERVAL_SYNCING, false);
    }

    fn wait_until_next_interval_and_sync(&self) {
        let weak = self.me.clone();
        let handler = disposable_timeout(
            move || {
                if let Some(this) = weak.upgrade() {
                    let _ = this.sync(INTERVAL_SYNCING, false);
                    this.interval_handler.set(None);
                }
            },
            self.interval,
            None,
        );
        self.interval_handler.set(Some(handler));
    }

    fn sync(&self, reason: &str, disable_cache: bool) -> Promise<()> {
        let this = self.rc();
        let reason = reason.to_string();
        let sync_promise = create_cancelable_promise(move |token| {
            async_fn(async move {
                let existing = this.sync_promise.borrow().clone();
                if let Some(existing) = existing {
                    // Wait until existing sync is finished
                    this.services.log_service.debug("[AutoSync] Waiting until sync is finished.");
                    if let Err(error) = existing.await
                        && is_cancellation_error(&error)
                    {
                        // Cancelled => Disposed. Donot continue sync.
                        return Ok(());
                    }
                }
                this.do_sync(reason, disable_cache, token).await
            })
        });
        *self.sync_promise.borrow_mut() = Some(sync_promise.clone());
        let weak = self.me.clone();
        let _ = sync_promise.finally(move || {
            if let Some(this) = weak.upgrade() {
                *this.sync_promise.borrow_mut() = None;
            }
        });
        (*sync_promise).clone()
    }

    fn has_sync_service_changed(&self) -> bool {
        let current = self.services.user_data_sync_store_management_service.user_data_sync_store().map(|store| store.url);
        self.last_sync_url.is_some() && !is_equal(self.last_sync_url.as_ref(), current.as_ref())
    }

    fn has_default_service_changed(&self) -> Promise<bool> {
        let this = self.rc();
        async_fn(async move {
            let previous = this.services.user_data_sync_store_management_service.get_previous_user_data_sync_store().await?;
            let current = this.services.user_data_sync_store_management_service.user_data_sync_store();
            // check if defaults changed
            Ok(match (current, previous) {
                (Some(current), Some(previous)) => {
                    !is_equal(Some(&current.default_url), Some(&previous.default_url))
                        || !is_equal(Some(&current.insiders_url), Some(&previous.insiders_url))
                        || !is_equal(Some(&current.stable_url), Some(&previous.stable_url))
                }
                _ => false,
            })
        })
    }

    fn do_sync(&self, reason: String, disable_cache: bool, token: CancellationToken) -> Promise<()> {
        let this = self.rc();
        async_fn(async move {
            this.services.log_service.info(&format!("[AutoSync] Triggered by {reason}"));

            this.on_did_start_sync.fire(());

            let mut error: Option<Error> = None;
            if let Err(e) = this.create_and_run_sync_task(disable_cache, token.clone()).await {
                this.services.log_service.error_args(&LogMessage::from(&e), &[]);
                let method_not_found = to_user_data_sync_error(&e).code == UserDataSyncErrorCode::MethodNotFound;
                error = Some(e);
                if method_not_found {
                    let retried: Result<(), Error> = async {
                        this.services.log_service.info("[AutoSync] Client is making invalid requests. Cleaning up data...");
                        this.services.user_data_sync_service.clean_up_remote_data().await?;
                        this.services.log_service.info("[AutoSync] Retrying sync...");
                        this.create_and_run_sync_task(disable_cache, token.clone()).await
                    }
                    .await;
                    match retried {
                        Ok(()) => error = None,
                        Err(e1) => {
                            this.services.log_service.error_args(&LogMessage::from(&e1), &[]);
                            error = Some(e1);
                        }
                    }
                }
            }

            this.on_did_finish_sync.fire(error);
            Ok(())
        })
    }

    fn create_and_run_sync_task(&self, disable_cache: bool, token: CancellationToken) -> Promise<()> {
        let this = self.rc();
        async_fn(async move {
            let services = &this.services;
            let manifest = this.manifest.borrow().clone();
            let sync_task = services.user_data_sync_service.create_sync_task(manifest, Some(disable_cache)).await?;
            *this.sync_task.borrow_mut() = Some(sync_task.clone());
            if token.is_cancellation_requested() {
                return Ok(());
            }
            *this.manifest.borrow_mut() = sync_task.manifest();

            // Server has no data but this machine was synced before
            if this.manifest.borrow().is_none() && services.user_data_sync_service.has_previously_synced().await? {
                if this.has_sync_service_changed() {
                    if this.has_default_service_changed().await? {
                        return Err(UserDataAutoSyncError::new(localize("default service changed", "Cannot sync because default service has changed", &[]), UserDataSyncErrorCode::DefaultServiceChanged)
                            .into_error());
                    }
                    return Err(UserDataAutoSyncError::new(localize("service changed", "Cannot sync because sync service has changed", &[]), UserDataSyncErrorCode::ServiceChanged).into_error());
                }
                // Sync was turned off in the cloud
                return Err(UserDataAutoSyncError::new(localize("turned off", "Cannot sync because syncing is turned off in the cloud", &[]), UserDataSyncErrorCode::TurnedOff).into_error());
            }

            let session_id = services.storage_service.get(SESSION_ID_KEY, StorageScope::Application, None).filter(|session_id| !session_id.is_empty());
            // Server session is different from client session
            let session_differs = match (&session_id, &*this.manifest.borrow()) {
                (Some(session_id), Some(manifest)) => *session_id != manifest.session,
                _ => false,
            };
            if session_differs {
                if this.has_sync_service_changed() {
                    if this.has_default_service_changed().await? {
                        return Err(UserDataAutoSyncError::new(localize("default service changed", "Cannot sync because default service has changed", &[]), UserDataSyncErrorCode::DefaultServiceChanged)
                            .into_error());
                    }
                    return Err(UserDataAutoSyncError::new(localize("service changed", "Cannot sync because sync service has changed", &[]), UserDataSyncErrorCode::ServiceChanged).into_error());
                }
                return Err(UserDataAutoSyncError::new(localize("session expired", "Cannot sync because current session is expired", &[]), UserDataSyncErrorCode::SessionExpired).into_error());
            }

            let manifest = this.manifest.borrow().clone();
            let machines = services.user_data_sync_machines_service.get_machines(manifest).await?;
            // Return if cancellation is requested
            if token.is_cancellation_requested() {
                return Ok(());
            }

            let current_machine = machines.into_iter().find(|machine| machine.is_current);
            // Check if sync was turned off from other machine
            if current_machine.as_ref().is_some_and(|machine| machine.disabled == Some(true)) {
                // Throw TurnedOff error
                return Err(UserDataAutoSyncError::new(
                    localize("turned off machine", "Cannot sync because syncing is turned off on this machine from another machine.", &[]),
                    UserDataSyncErrorCode::TurnedOff,
                )
                .into_error());
            }

            let start_time = date_now();
            sync_task.run().await?;
            services.telemetry_service.public_log2("settingsSync:sync", Some(Dynamic::object([("duration", Dynamic::Number((date_now() - start_time) as f64))])));

            // After syncing, get the manifest if it was not available before
            if this.manifest.borrow().is_none() {
                match services.user_data_sync_store_service.manifest(None, None).await {
                    Ok(manifest) => *this.manifest.borrow_mut() = manifest,
                    Err(error) => {
                        let code = UserDataSyncError::from_error(&error).map_or(UserDataSyncErrorCode::Unknown, |sync_error| sync_error.code);
                        return Err(UserDataAutoSyncError::new(to_error_message_of(&error, false), code).into_error());
                    }
                }
            }

            // Update local session id
            let manifest = this.manifest.borrow().clone();
            if let Some(manifest) = &manifest
                && session_id.as_deref() != Some(manifest.session.as_str())
            {
                services.storage_service.store(SESSION_ID_KEY, StorageValue::from(manifest.session.as_str()), StorageScope::Application, StorageTarget::Machine);
            }

            // Return if cancellation is requested
            if token.is_cancellation_requested() {
                return Ok(());
            }

            // Add current machine
            if current_machine.is_none() {
                services.user_data_sync_machines_service.add_current_machine(manifest).await?;
            }
            Ok(())
        })
    }
}

impl IDisposable for AutoSync {
    fn dispose(&self) {
        self.base.dispose();
    }
}

// endregion
