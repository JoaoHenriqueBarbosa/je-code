//! Porte de `src/vs/platform/update/test/electron-main/abstractUpdateService.test.ts`.
//!
//! Um `#[test]` por caso do original, na mesma ordem.
//!
//! Notas de tradução:
//! - A subclasse `TestUpdateService` não existe: o serviço é um [`AbstractUpdateService`] com os ganchos de
//!   [`TestHooks`] (as sobrescritas de `initialize`, `buildUpdateFeedUrl`, `doCheckForUpdates`, `doDownloadUpdate`,
//!   `doIsLatestVersion`, `postInitialize` e `cancelUpdate`), e os membros auxiliares dela (`whenInitialized`,
//!   `checkCount`, `forceState`...) são campos dos ganchos ou chamadas ao serviço. O `super.xxx()` é o método
//!   `*_default` do serviço.
//! - `sinon.useFakeTimers()` e `clock.tickAsync(ms)` são o relógio virtual de [`run_with_faked_timers`] e o auxiliar
//!   `tick_async` (esvazia as microtarefas e avança o relógio). Os casos sem relógio falso rodam no laço real
//!   ([`run_with_real_timers`]) e esperam `timeout(0)` de verdade.
//! - O `ensureNoDisposablesAreLeakedInTestSuite()` é o [`LeakGuard`] criado no começo do corpo do caso (antes do `setup`,
//!   que é o `create_service`) e descartado no fim dele, que é o `teardown` do original; `sinon.restore()` não tem
//!   equivalente (o relógio virtual é restaurado pelo próprio auxiliar).
//! - O `PolicyTestConfigurationService` (subclasse do `TestConfigurationService`) é [`PolicyTestConfigurationService`],
//!   que delega ao serviço de teste do porte e sobrescreve `getValue` e `inspect`. O
//!   `{ affectsConfiguration: () => true }` do original é um `ConfigurationChangeEvent` real com a chave `update.mode`.
//! - Os mocks por literal (`lifecycleMainService`, `environmentMainService`, `requestService`, `productService`,
//!   `applicationStorageMainService`) usam os contratos reais do porte: o `lifecycleMainService` é uma estrutura que
//!   implementa o [`LifecycleMainService`] (`when` resolve, `quit` não é vetado), o `environmentMainService` é o
//!   [`AbstractNativeEnvironmentService`] com o ambiente fixo (`isBuilt` é a ausência de `VSCODE_DEV`) e o argumento
//!   `disable-updates`, o `productService` é o [`ProductService`] e o `applicationStorageMainService` é o
//!   [`ApplicationStorageMainService`] sobre o `InMemoryStorageService`. O `requestService` é uma estrutura que conta a
//!   requisição.
//! - O `teardown` do mocha roda depois de as microtarefas do caso esvaziarem (o `await changeMode()` retoma no meio do
//!   `doReconfigure`, que ainda agenda a verificação periódica antes de o `teardown` descartar o serviço). O [`Teardown`]
//!   esvazia as microtarefas antes de descartar o [`LeakGuard`]; sem isso o agendamento acontecia com o serviço já
//!   descartado, o temporizador ficava sem dono e se repetia a cada hora para sempre no laço real.

mod common;

use std::cell::{Cell, RefCell};
use std::ops::Deref;
use std::rc::Rc;

use common::{LeakGuard, change, empty_test_configuration, ensure_no_disposables_are_leaked_in_test_suite as ensure_leak_guard, null_log, run_with_faked_timers, run_with_real_timers};
use je_base::async_::{async_fn, timeout};
use je_base::cancellation::CancellationToken;
use je_base::dynamic::Dynamic;
use je_base::errors::Error;
use je_base::event::{Emitter, Event};
use je_base::lifecycle::{Disposable, IDisposable};
use je_base::parts::request::{IRequestContext, IRequestOptions};
use je_base::path::{FixedEnvironment, Platform};
use je_base::promise::{DeferredPromise, Promise};
use je_base::runtime;
use je_platform::configuration::models::{ConfigurationChangeEvent, ConfigurationKeys};
use je_platform::configuration::service::IConfigurationService;
use je_platform::configuration::test_configuration_service::{TestConfigurationService, TestConfigurationValue};
use je_platform::configuration::{ConfigurationData, ConfigurationOverrides, ConfigurationTarget, ConfigurationUpdateOverrides, ConfigurationValue};
use je_platform::environment::environment_service::{AbstractNativeEnvironmentService, NativeEnvironmentPaths};
use je_platform::environment::native_parsed_args::NativeParsedArgs;
use je_platform::log::NullLogService;
use je_platform::metered_connection::IMeteredConnectionService;
use je_platform::product::ProductService;
use je_platform::request::{AuthInfo, Credentials, IRequestService, RequestCompleteEvent};
use je_platform::storage::{ApplicationStorageMainService, InMemoryStorageService};
use je_platform::telemetry::telemetry_utils::null_telemetry_service;
use je_platform::update::abstract_update_service::{AbstractUpdateService, AbstractUpdateServiceHooks, UpdateUrlOptions};
use je_platform::update::{AvailableForDownload, DisablementReason, State, StateType, Update, UpdateService};
use je_platform::user_data_profile::user_data_profiles_handler::{LifecycleMainPhase, LifecycleMainService, ProfilesWindow, ShutdownEvent, WindowLoadEvent};

/// O [`LeakGuard`] com o `teardown` do mocha: as microtarefas pendentes do caso esvaziam antes de o serviço ser descartado.
struct Teardown(LeakGuard);

impl Deref for Teardown {
    type Target = LeakGuard;

    fn deref(&self) -> &LeakGuard {
        &self.0
    }
}

impl Drop for Teardown {
    fn drop(&mut self) {
        if !std::thread::panicking() {
            runtime::run_microtasks();
        }
    }
}

/// `ensureNoDisposablesAreLeakedInTestSuite()`, com o `teardown` depois das microtarefas.
fn ensure_no_disposables_are_leaked_in_test_suite() -> Teardown {
    Teardown(ensure_leak_guard())
}

/// `TestMeteredConnectionService`.
struct TestMeteredConnectionService {
    base: Rc<Disposable>,
    on_did_change_is_connection_metered: Rc<Emitter<bool>>,
    is_connection_metered: Cell<bool>,
    when_connection_state_initialized: Option<Promise<()>>,
}

impl TestMeteredConnectionService {
    fn new(is_connection_metered: bool, when_connection_state_initialized: Option<Promise<()>>) -> Rc<Self> {
        let base = Disposable::new();
        let on_did_change_is_connection_metered = Emitter::new();
        base.register(on_did_change_is_connection_metered.clone());
        Rc::new(Self { base, on_did_change_is_connection_metered, is_connection_metered: Cell::new(is_connection_metered), when_connection_state_initialized })
    }

    fn set_is_connection_metered(&self, is_connection_metered: bool) {
        self.is_connection_metered.set(is_connection_metered);
        self.on_did_change_is_connection_metered.fire(is_connection_metered);
    }
}

impl IMeteredConnectionService for TestMeteredConnectionService {
    fn is_connection_metered(&self) -> bool {
        self.is_connection_metered.get()
    }

    fn when_connection_state_initialized(&self) -> Option<Promise<()>> {
        self.when_connection_state_initialized.clone()
    }

    fn on_did_change_is_connection_metered(&self) -> Event<bool> {
        self.on_did_change_is_connection_metered.event()
    }
}

impl IDisposable for TestMeteredConnectionService {
    fn dispose(&self) {
        self.base.dispose();
    }
}

/// As sobrescritas da subclasse `TestUpdateService`, mais o estado que ela expõe.
struct TestHooks {
    initialized: Rc<DeferredPromise<()>>,
    post_initialize_started: Rc<DeferredPromise<()>>,
    post_initialize_gate: RefCell<Option<Promise<()>>>,
    check_count: Cell<u32>,
    cancel_count: Cell<u32>,
    download_count: Cell<u32>,
    latest_version_result: RefCell<Option<Promise<Option<bool>>>>,
    /// When set, `cancelUpdate` blocks on this promise so tests can observe the transient Cancelling state.
    cancel_gate: RefCell<Option<Promise<()>>>,
    feed_url: RefCell<Option<String>>,
}

impl TestHooks {
    fn new() -> Rc<Self> {
        Rc::new(Self {
            initialized: Rc::new(DeferredPromise::new()),
            post_initialize_started: Rc::new(DeferredPromise::new()),
            post_initialize_gate: RefCell::new(None),
            check_count: Cell::new(0),
            cancel_count: Cell::new(0),
            download_count: Cell::new(0),
            latest_version_result: RefCell::new(None),
            cancel_gate: RefCell::new(None),
            feed_url: RefCell::new(Some("https://update.example/feed".to_string())),
        })
    }
}

impl AbstractUpdateServiceHooks for TestHooks {
    fn initialize(&self, core: &Rc<AbstractUpdateService>) -> Promise<()> {
        let initialized = self.initialized.clone();
        let initializing = core.initialize_default();
        async_fn(async move {
            let result = initializing.await;
            let _ = initialized.complete(());
            result
        })
    }

    fn build_update_feed_url(&self, _core: &Rc<AbstractUpdateService>, _quality: &str, _commit: &str, _options: Option<&UpdateUrlOptions>) -> Option<String> {
        self.feed_url.borrow().clone()
    }

    fn do_check_for_updates(&self, _core: &Rc<AbstractUpdateService>, _explicit: bool, _pending_commit: Option<&str>) {
        self.check_count.set(self.check_count.get() + 1);
    }

    fn do_download_update(&self, _core: &Rc<AbstractUpdateService>, _state: &AvailableForDownload) -> Promise<()> {
        self.download_count.set(self.download_count.get() + 1);
        Promise::resolved(())
    }

    fn do_is_latest_version(&self, core: &Rc<AbstractUpdateService>, commit: Option<&str>, token: CancellationToken) -> Promise<Option<bool>> {
        let result = self.latest_version_result.borrow().clone();
        result.unwrap_or_else(|| core.do_is_latest_version_default(commit, token))
    }

    fn post_initialize(&self, _core: &Rc<AbstractUpdateService>) -> Promise<()> {
        let _ = self.post_initialize_started.complete(());
        let gate = self.post_initialize_gate.borrow().clone();
        async_fn(async move {
            match gate {
                Some(gate) => gate.await?,
                None => Promise::resolved(()).await?,
            }
            Ok(())
        })
    }

    fn cancel_update(&self, core: &Rc<AbstractUpdateService>) -> Promise<()> {
        self.cancel_count.set(self.cancel_count.get() + 1);
        let gate = self.cancel_gate.borrow().clone();
        let core = core.clone();
        async_fn(async move {
            if let Some(gate) = gate {
                gate.await?;
            }
            core.cancel_update_default().await
        })
    }
}

/// `PolicyTestConfigurationService`: a política vence a configuração do usuário, como no serviço real.
struct PolicyTestConfigurationService {
    inner: Rc<TestConfigurationService>,
    policy_value: RefCell<Option<String>>,
}

impl PolicyTestConfigurationService {
    fn new() -> Rc<Self> {
        Rc::new(Self { inner: TestConfigurationService::new(None), policy_value: RefCell::new(None) })
    }

    fn set_user_mode(&self, mode: &str) {
        let _ = self.inner.set_user_configuration("update.mode", Dynamic::string(mode), None);
    }

    /// `onDidChangeConfigurationEmitter.fire({ affectsConfiguration: () => true })`.
    fn fire_change(&self) {
        let event = ConfigurationChangeEvent::new(change(&["update.mode"], &[]), None, Rc::new(empty_test_configuration()), None, null_log());
        self.inner.on_did_change_configuration_emitter.fire(Rc::new(event));
    }
}

impl IConfigurationService for PolicyTestConfigurationService {
    fn on_did_change_configuration(&self) -> Event<Rc<ConfigurationChangeEvent>> {
        self.inner.on_did_change_configuration()
    }

    fn get_configuration_data(&self) -> Option<ConfigurationData> {
        self.inner.get_configuration_data()
    }

    fn get_value(&self, section: Option<&str>, overrides: Option<&ConfigurationOverrides>) -> Dynamic {
        // Mirror the real configuration service: a policy value overrides the user setting.
        if section == Some("update.mode")
            && let Some(policy) = self.policy_value.borrow().as_deref()
        {
            return Dynamic::string(policy);
        }
        self.inner.get_value(section, overrides)
    }

    fn update_value(&self, key: &str, value: Dynamic, overrides: Option<ConfigurationUpdateOverrides>, target: Option<ConfigurationTarget>) -> Promise<()> {
        self.inner.update_value(key, value, overrides, target)
    }

    fn inspect(&self, key: &str, overrides: &ConfigurationOverrides) -> Rc<dyn ConfigurationValue> {
        let result = self.inner.inspect(key, overrides);
        if key == "update.mode" {
            return Rc::new(TestConfigurationValue {
                default_value: result.default_value(),
                application_value: result.application_value(),
                user_value: result.user_value(),
                user_local_value: result.user_local_value(),
                user_remote_value: result.user_remote_value(),
                workspace_value: result.workspace_value(),
                workspace_folder_value: result.workspace_folder_value(),
                memory_value: result.memory_value(),
                policy_value: self.policy_value.borrow().as_deref().map_or(Dynamic::Undefined, Dynamic::string),
                value: result.value(),
                override_identifiers: result.override_identifiers(),
            });
        }
        result
    }

    fn reload_configuration(&self) -> Promise<()> {
        self.inner.reload_configuration()
    }

    fn keys(&self) -> ConfigurationKeys {
        self.inner.keys()
    }
}

/// O mock de `ILifecycleMainService` do literal do original: `when` resolve, `quit` não é vetado.
struct TestLifecycleMainService;

impl LifecycleMainService for TestLifecycleMainService {
    fn on_will_load_window(&self) -> Event<WindowLoadEvent> {
        Event::none()
    }

    fn on_before_close_window(&self) -> Event<Rc<dyn ProfilesWindow>> {
        Event::none()
    }

    fn on_will_shutdown(&self) -> Event<ShutdownEvent> {
        Event::none()
    }

    fn on_before_shutdown(&self) -> Event<()> {
        Event::none()
    }

    fn quit_requested(&self) -> bool {
        false
    }

    fn was_restarted(&self) -> bool {
        false
    }

    fn when(&self, _phase: LifecycleMainPhase) -> Promise<()> {
        Promise::resolved(())
    }

    fn quit(&self, _will_restart: bool) -> Promise<bool> {
        Promise::resolved(false)
    }
}

/// O `environmentMainService` do literal do original (`isBuilt` e `disableUpdates`): o serviço de ambiente nativo, com
/// `isBuilt` falso quando `VSCODE_DEV` está definida.
fn environment_main_service(is_built: bool, disable_updates: bool) -> Rc<AbstractNativeEnvironmentService> {
    let mut environment = FixedEnvironment::new(Platform::Posix, "/");
    if !is_built {
        environment.vars.insert("VSCODE_DEV".to_owned(), "1".to_owned());
    }
    let args = NativeParsedArgs::default();
    if disable_updates {
        args.set("disable-updates", Dynamic::Bool(true));
    }
    let paths = NativeEnvironmentPaths { user_data_dir: "/user-data".to_owned(), home_dir: "/home".to_owned(), tmp_dir: "/tmp".to_owned(), app_root: "/app".to_owned() };
    Rc::new(AbstractNativeEnvironmentService::new(args, paths, ProductService::default(), Rc::new(environment)))
}

/// O mock de `IRequestService`: conta a requisição e a rejeita (`not expected`).
struct TestRequestService {
    request_count: Rc<Cell<u32>>,
}

impl IRequestService for TestRequestService {
    fn on_did_complete_request(&self) -> Event<RequestCompleteEvent> {
        Event::none()
    }

    fn request(&self, _options: IRequestOptions, _token: CancellationToken) -> Promise<IRequestContext> {
        self.request_count.set(self.request_count.get() + 1);
        Promise::rejected(Error::new("not expected"))
    }

    fn resolve_proxy(&self, _url: &str) -> Promise<Option<String>> {
        Promise::resolved(None)
    }

    fn lookup_authorization(&self, _auth_info: AuthInfo) -> Promise<Option<Credentials>> {
        Promise::resolved(None)
    }

    fn lookup_kerberos_authorization(&self, _url: &str) -> Promise<Option<String>> {
        Promise::resolved(None)
    }

    fn load_certificates(&self) -> Promise<Vec<String>> {
        Promise::resolved(Vec::new())
    }
}

/// O `productService` do literal do original.
fn product_service(update_url: String) -> ProductService {
    ProductService { update_url: Some(update_url), commit: Some("abc123".to_owned()), quality: Some("stable".to_owned()), version: "1.0.0".to_owned(), ..ProductService::default() }
}

/// O `applicationStorageMainService` do literal do original: já pronto, sobre o armazenamento em memória.
fn application_storage_main_service() -> Rc<ApplicationStorageMainService> {
    Rc::new(ApplicationStorageMainService::from_service(InMemoryStorageService::new(), Promise::resolved(())))
}

#[derive(Default)]
struct Options {
    is_built: Option<bool>,
    disable_updates: Option<bool>,
    update_url: Option<String>,
    is_connection_metered: Option<bool>,
    metered_connection_initialization: Option<Promise<()>>,
    post_initialize_gate: Option<Promise<()>>,
    supports_update_overwrite: Option<bool>,
}

/// O que o `createService` deixa em variáveis da suíte (`configurationService`, `requestCount`,
/// `meteredConnectionService`) junto com o serviço.
struct Fixture {
    core: Rc<AbstractUpdateService>,
    hooks: Rc<TestHooks>,
    configuration_service: Rc<PolicyTestConfigurationService>,
    request_count: Rc<Cell<u32>>,
    metered_connection_service: Rc<TestMeteredConnectionService>,
}

impl Fixture {
    /// `whenInitialized`.
    fn when_initialized(&self) -> Promise<()> {
        self.hooks.initialized.promise.clone()
    }

    /// `forceState(state, options?)`: forces the service into a given state so tests can exercise cancellation from a
    /// cancellable state.
    fn force_state(&self, state: State, deferred: Option<bool>) {
        self.core.set_state(state, deferred);
    }
}

fn create_service(store: &LeakGuard, mode: &str, options: Options) -> Fixture {
    let configuration_service = PolicyTestConfigurationService::new();
    configuration_service.set_user_mode(mode);
    let request_count = Rc::new(Cell::new(0));

    let metered_connection_service =
        store.add_rc(TestMeteredConnectionService::new(options.is_connection_metered.unwrap_or(false), options.metered_connection_initialization.clone()));

    let hooks = TestHooks::new();
    let core = AbstractUpdateService::new(
        Rc::new(TestLifecycleMainService),
        configuration_service.clone(),
        environment_main_service(options.is_built.unwrap_or(true), options.disable_updates.unwrap_or(false)),
        Rc::new(TestRequestService { request_count: request_count.clone() }),
        NullLogService::shared(),
        Rc::new(product_service(options.update_url.clone().unwrap_or_else(|| "https://update.example".to_string()))),
        null_telemetry_service(),
        store.add_rc(application_storage_main_service()),
        metered_connection_service.clone(),
        options.supports_update_overwrite.unwrap_or(false),
        hooks.clone(),
    );
    if let Some(gate) = options.post_initialize_gate {
        *hooks.post_initialize_gate.borrow_mut() = Some(gate);
    }

    store.add_rc(core.clone());
    Fixture { core, hooks, configuration_service, request_count, metered_connection_service }
}

fn change_mode(fixture: &Fixture, mode: &str) -> Promise<State> {
    fixture.configuration_service.set_user_mode(mode);
    let next = fixture.core.on_state_change().to_promise(None);
    fixture.configuration_service.fire_change();
    next.promise
}

fn set_policy(fixture: &Fixture, policy_value: Option<&str>) -> Promise<State> {
    *fixture.configuration_service.policy_value.borrow_mut() = policy_value.map(str::to_owned);
    let next = fixture.core.on_state_change().to_promise(None);
    fixture.configuration_service.fire_change();
    next.promise
}

/// `clock.tickAsync(ms)`: esvazia as microtarefas e avança o relógio virtual.
fn tick_async(ms: u64) {
    runtime::run_microtasks();
    runtime::advance_fake_time(ms);
}

fn update(version: &str) -> Update {
    Update { version: version.to_string(), product_version: Some(version.to_string()), url: Some("https://update.example/download".to_string()), ..Default::default() }
}

fn available_for_download() -> State {
    State::available_for_download(update("1.1.0"), None)
}

#[test]
fn mode_none_disables_updates_at_startup() {
    run_with_real_timers(async {
        let store = ensure_no_disposables_are_leaked_in_test_suite();
        let service = create_service(&store, "none", Options::default());
        service.when_initialized().await.unwrap();

        assert_eq!(service.core.state(), State::disabled(DisablementReason::ManuallyDisabled));
    });
}

#[test]
fn mode_default_enables_updates_at_startup() {
    run_with_real_timers(async {
        let store = ensure_no_disposables_are_leaked_in_test_suite();
        let service = create_service(&store, "default", Options::default());
        service.when_initialized().await.unwrap();

        assert_eq!(service.core.state().state_type(), StateType::Idle);
    });
}

#[test]
fn policy_forces_updates_off_even_when_the_user_setting_keeps_them_enabled() {
    run_with_real_timers(async {
        let store = ensure_no_disposables_are_leaked_in_test_suite();
        let service = create_service(&store, "default", Options::default());
        service.when_initialized().await.unwrap();
        assert_eq!(service.core.state().state_type(), StateType::Idle);

        // User setting stays 'default' (enabled); policy alone forces 'none'.
        set_policy(&service, Some("none")).await.unwrap();

        assert_eq!(service.core.state(), State::disabled(DisablementReason::Policy));
    });
}

#[test]
fn switching_to_none_at_runtime_cancels_and_disables() {
    run_with_real_timers(async {
        let store = ensure_no_disposables_are_leaked_in_test_suite();
        let service = create_service(&store, "default", Options::default());
        service.when_initialized().await.unwrap();

        change_mode(&service, "none").await.unwrap();

        assert_eq!(service.core.state(), State::disabled(DisablementReason::ManuallyDisabled));
        assert_eq!(service.hooks.cancel_count.get(), 1);
    });
}

#[test]
fn switching_from_none_to_default_at_runtime_re_enables() {
    run_with_real_timers(async {
        let store = ensure_no_disposables_are_leaked_in_test_suite();
        let service = create_service(&store, "none", Options::default());
        service.when_initialized().await.unwrap();
        assert_eq!(service.core.state().state_type(), StateType::Disabled);

        change_mode(&service, "default").await.unwrap();

        assert_eq!(service.core.state().state_type(), StateType::Idle);
    });
}

#[test]
fn default_schedules_a_background_check_none_does_not() {
    run_with_faked_timers(async {
        let store = ensure_no_disposables_are_leaked_in_test_suite();
        let service = create_service(&store, "default", Options::default());
        service.when_initialized().await.unwrap();
        tick_async(30 * 1000);
        assert_eq!(service.hooks.check_count.get(), 1, "default should schedule a check");

        change_mode(&service, "none").await.unwrap();
        tick_async(60 * 60 * 1000);
        assert_eq!(service.hooks.check_count.get(), 1, "none should not schedule further checks");
    });
}

#[test]
fn automatic_scheduling_waits_for_the_initial_metered_connection_state() {
    run_with_faked_timers(async {
        let store = ensure_no_disposables_are_leaked_in_test_suite();
        let connection_initialized = DeferredPromise::<()>::new();
        let service = create_service(&store, "default", Options { metered_connection_initialization: Some(connection_initialized.promise.clone()), ..Default::default() });

        tick_async(30 * 1000);
        assert_eq!(service.hooks.check_count.get(), 0);

        service.metered_connection_service.set_is_connection_metered(true);
        let _ = connection_initialized.complete(());
        service.when_initialized().await.unwrap();
        tick_async(30 * 1000);
        assert_eq!(service.hooks.check_count.get(), 0);

        service.metered_connection_service.set_is_connection_metered(false);
        tick_async(0);
        assert_eq!(service.hooks.check_count.get(), 1);
    });
}

#[test]
fn unmetering_during_post_initialization_does_not_start_a_check() {
    run_with_faked_timers(async {
        let store = ensure_no_disposables_are_leaked_in_test_suite();
        let post_initialize_gate = DeferredPromise::<()>::new();
        let service = create_service(&store, "default", Options { is_connection_metered: Some(true), post_initialize_gate: Some(post_initialize_gate.promise.clone()), ..Default::default() });
        service.hooks.post_initialize_started.promise.clone().await.unwrap();

        service.metered_connection_service.set_is_connection_metered(false);
        tick_async(0);
        assert_eq!(service.hooks.check_count.get(), 0);

        let _ = post_initialize_gate.complete(());
        service.when_initialized().await.unwrap();
        tick_async(0);
        assert_eq!(service.hooks.check_count.get(), 0);

        tick_async(30 * 1000);
        assert_eq!(service.hooks.check_count.get(), 1);
    });
}

#[test]
fn metered_connections_skip_automatic_update_requests_but_allow_explicit_actions() {
    run_with_real_timers(async {
        let store = ensure_no_disposables_are_leaked_in_test_suite();
        let service = create_service(&store, "default", Options { is_connection_metered: Some(true), ..Default::default() });
        service.when_initialized().await.unwrap();

        service.core.check_for_updates(false).await.unwrap();
        service.core.is_latest_version().await.unwrap();
        service.core.check_for_updates(true).await.unwrap();
        // `checkLatestVersionExplicitly()`
        service.hooks.do_is_latest_version(&service.core, None, CancellationToken::None).await.unwrap();

        service.force_state(available_for_download(), None);
        service.core.download_update(false).await.unwrap();
        service.core.download_update(true).await.unwrap();
        service.metered_connection_service.set_is_connection_metered(false);
        timeout(0).await.unwrap();

        assert_eq!((service.hooks.check_count.get(), service.hooks.download_count.get(), service.request_count.get()), (1, 1, 1));
    });
}

#[test]
fn automatic_checks_resume_when_the_connection_is_no_longer_metered() {
    run_with_faked_timers(async {
        let store = ensure_no_disposables_are_leaked_in_test_suite();
        let service = create_service(&store, "start", Options { is_connection_metered: Some(true), ..Default::default() });
        service.when_initialized().await.unwrap();
        tick_async(30 * 1000);
        assert_eq!(service.hooks.check_count.get(), 0);

        service.metered_connection_service.set_is_connection_metered(false);
        tick_async(0);
        assert_eq!(service.hooks.check_count.get(), 1);
    });
}

#[test]
fn completed_startup_checks_do_not_run_again_after_a_metered_transition() {
    run_with_faked_timers(async {
        let store = ensure_no_disposables_are_leaked_in_test_suite();
        let service = create_service(&store, "start", Options::default());
        service.when_initialized().await.unwrap();
        tick_async(30 * 1000);
        assert_eq!(service.hooks.check_count.get(), 1);

        service.metered_connection_service.set_is_connection_metered(true);
        service.metered_connection_service.set_is_connection_metered(false);
        tick_async(0);

        assert_eq!(service.hooks.check_count.get(), 1);
    });
}

#[test]
fn only_resumes_automatic_downloads_that_were_deferred_by_metering() {
    run_with_real_timers(async {
        let store = ensure_no_disposables_are_leaked_in_test_suite();
        let service = create_service(&store, "default", Options::default());
        service.when_initialized().await.unwrap();
        service.force_state(available_for_download(), None);

        service.metered_connection_service.set_is_connection_metered(true);
        service.metered_connection_service.set_is_connection_metered(false);
        timeout(0).await.unwrap();
        let downloads_without_deferred_intent = service.hooks.download_count.get();

        service.metered_connection_service.set_is_connection_metered(true);
        service.core.download_update(false).await.unwrap();
        service.metered_connection_service.set_is_connection_metered(false);
        timeout(0).await.unwrap();

        assert_eq!((downloads_without_deferred_intent, service.hooks.download_count.get()), (0, 1));
    });
}

#[test]
fn resumes_an_automatic_download_deferred_after_an_update_check() {
    run_with_real_timers(async {
        let store = ensure_no_disposables_are_leaked_in_test_suite();
        let service = create_service(&store, "default", Options { is_connection_metered: Some(true), ..Default::default() });
        service.when_initialized().await.unwrap();
        service.force_state(available_for_download(), Some(true));

        service.metered_connection_service.set_is_connection_metered(false);
        timeout(0).await.unwrap();

        assert_eq!(service.hooks.download_count.get(), 1);
    });
}

#[test]
fn defers_an_automatic_download_when_connection_becomes_metered_during_preparation() {
    run_with_real_timers(async {
        let store = ensure_no_disposables_are_leaked_in_test_suite();
        let service = create_service(&store, "default", Options::default());
        service.when_initialized().await.unwrap();
        let pending = update("1.1.0");
        service.force_state(State::downloading(Some(pending.clone()), false, false, None, None, None), None);

        service.metered_connection_service.set_is_connection_metered(true);
        let deferred = service.core.defer_automatic_download(&pending, false);
        assert_eq!((deferred, service.core.state().state_type(), service.hooks.download_count.get()), (true, StateType::AvailableForDownload, 0));

        service.metered_connection_service.set_is_connection_metered(false);
        timeout(0).await.unwrap();
        assert_eq!(service.hooks.download_count.get(), 1);
    });
}

#[test]
fn resumes_deferred_work_when_automatic_mode_is_re_enabled_while_unmetered() {
    run_with_real_timers(async {
        let store = ensure_no_disposables_are_leaked_in_test_suite();
        let service = create_service(&store, "manual", Options { is_connection_metered: Some(true), ..Default::default() });
        service.when_initialized().await.unwrap();
        service.force_state(available_for_download(), Some(true));

        service.metered_connection_service.set_is_connection_metered(false);
        timeout(0).await.unwrap();
        assert_eq!(service.hooks.download_count.get(), 0);

        service.configuration_service.set_user_mode("default");
        service.configuration_service.fire_change();
        timeout(0).await.unwrap();
        timeout(0).await.unwrap();

        assert_eq!(service.hooks.download_count.get(), 1);
    });
}

#[test]
fn resumes_overwrite_checks_that_were_deferred_by_metering() {
    run_with_faked_timers(async {
        let store = ensure_no_disposables_are_leaked_in_test_suite();
        let service = create_service(&store, "default", Options { is_connection_metered: Some(true), supports_update_overwrite: Some(true), ..Default::default() });
        service.when_initialized().await.unwrap();
        service.force_state(State::ready(update("pending"), false, false), None);

        tick_async(5 * 60 * 1000);
        assert_eq!(service.request_count.get(), 0);

        service.metered_connection_service.set_is_connection_metered(false);
        tick_async(0);

        assert_eq!(service.request_count.get(), 1);
    });
}

#[test]
fn defers_overwrite_continuation_when_connection_becomes_metered_during_latest_version_probe() {
    run_with_faked_timers(async {
        let store = ensure_no_disposables_are_leaked_in_test_suite();
        let latest_version_result = DeferredPromise::<Option<bool>>::new();
        let service = create_service(&store, "default", Options { supports_update_overwrite: Some(true), ..Default::default() });
        service.when_initialized().await.unwrap();
        *service.hooks.latest_version_result.borrow_mut() = Some(latest_version_result.promise.clone());
        service.force_state(State::ready(update("pending"), false, false), None);

        tick_async(5 * 60 * 1000);
        service.metered_connection_service.set_is_connection_metered(true);
        let _ = latest_version_result.complete(Some(false));
        tick_async(0);
        assert_eq!((service.hooks.check_count.get(), service.core.state().state_type()), (0, StateType::Ready));

        service.metered_connection_service.set_is_connection_metered(false);
        tick_async(0);
        assert_eq!((service.hooks.check_count.get(), service.core.state().state_type()), (1, StateType::Overwriting));
    });
}

#[test]
fn permanent_disablement_ignores_runtime_mode_changes() {
    run_with_real_timers(async {
        let store = ensure_no_disposables_are_leaked_in_test_suite();
        let service = create_service(&store, "default", Options { is_built: Some(false), ..Default::default() });
        service.when_initialized().await.unwrap();
        assert_eq!(service.core.state(), State::disabled(DisablementReason::NotBuilt));

        service.configuration_service.set_user_mode("none");
        service.configuration_service.fire_change();

        assert_eq!(service.core.state(), State::disabled(DisablementReason::NotBuilt));
    });
}

#[test]
fn redundant_update_mode_write_does_not_re_disable() {
    run_with_real_timers(async {
        let store = ensure_no_disposables_are_leaked_in_test_suite();
        let service = create_service(&store, "none", Options::default());
        service.when_initialized().await.unwrap();
        assert_eq!(service.core.state(), State::disabled(DisablementReason::ManuallyDisabled));

        let cancels_after_init = service.hooks.cancel_count.get();
        let state_changes = Rc::new(Cell::new(0));
        let counter = state_changes.clone();
        store.add(service.core.on_state_change().listen(move |_| counter.set(counter.get() + 1)));

        // Re-write the same 'none' value: this affects `update.mode` but does not change the outcome.
        service.configuration_service.set_user_mode("none");
        service.configuration_service.fire_change();
        timeout(0).await.unwrap();

        assert_eq!(service.hooks.cancel_count.get(), cancels_after_init, "should not cancel again while already disabled");
        assert_eq!(state_changes.get(), 0, "should not re-fire the Disabled state");
    });
}

#[test]
fn surfaces_cancelling_while_tearing_down_in_flight_work_then_disabled() {
    run_with_real_timers(async {
        let store = ensure_no_disposables_are_leaked_in_test_suite();
        let service = create_service(&store, "default", Options::default());
        service.when_initialized().await.unwrap();

        // Put the service into a cancellable state and make cancellation block until we release it.
        service.force_state(State::checking_for_updates(false), None);
        let gate = DeferredPromise::<()>::new();
        *service.hooks.cancel_gate.borrow_mut() = Some(gate.promise.clone());

        let states: Rc<RefCell<Vec<StateType>>> = Rc::default();
        let recorded = states.clone();
        store.add(service.core.on_state_change().listen(move |state| recorded.borrow_mut().push(state.state_type())));

        service.configuration_service.set_user_mode("none");
        service.configuration_service.fire_change();
        timeout(0).await.unwrap();

        assert_eq!(service.core.state().state_type(), StateType::Cancelling, "should show Cancelling while cancellation is in progress");

        let _ = gate.complete(());
        timeout(0).await.unwrap();

        assert_eq!(service.core.state(), State::disabled(DisablementReason::ManuallyDisabled));
        assert_eq!(*states.borrow(), vec![StateType::Cancelling, StateType::Disabled]);
    });
}

#[test]
fn does_not_enter_cancelling_when_nothing_is_in_flight() {
    run_with_real_timers(async {
        let store = ensure_no_disposables_are_leaked_in_test_suite();
        let service = create_service(&store, "default", Options::default());
        service.when_initialized().await.unwrap();
        assert_eq!(service.core.state().state_type(), StateType::Idle);

        let states: Rc<RefCell<Vec<StateType>>> = Rc::default();
        let recorded = states.clone();
        store.add(service.core.on_state_change().listen(move |state| recorded.borrow_mut().push(state.state_type())));

        change_mode(&service, "none").await.unwrap();

        assert_eq!(service.core.state(), State::disabled(DisablementReason::ManuallyDisabled));
        assert_eq!(*states.borrow(), vec![StateType::Disabled], "should go straight to Disabled without a Cancelling flash");
    });
}
