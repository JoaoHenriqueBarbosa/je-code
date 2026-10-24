//! Porte de `src/vs/workbench/services/accounts/test/browser/defaultAccount.test.ts` (suítes `DefaultAccountProvider` e
//! `DefaultAccountProvider sign in scopes`), caso a caso e na mesma ordem.
//!
//! Divergências:
//! - Toda a suíte roda sob o relógio virtual ([`account_test`]), que começa o `Date.now()` numa época realista
//!   (`setSystemTime`), porque o relógio virtual parte de zero e os casos fazem `Date.now() - 7 dias`. Ao fim de cada
//!   caso o store é descartado, o que cancela o agendador de 1 hora do provedor (sem isso o esvaziamento de timers
//!   do relógio virtual o reagendaria sem fim).
//! - `provider['método']` são os métodos `pub` do provedor. `getManagedSettings(sessions, policy, options?, ...)`
//!   passa por [`managed`], que completa os quatro últimos parâmetros com [`IManagedSettingsFetchContext::default`].
//! - `Partial<IPolicyData>` de `getManagedSettings` é [`IManagedSettingsPatch`]; `{ managedSettings: undefined }` é
//!   `IManagedSettingsPatch::of(None)`.
//! - `describeFreshness` devolve um [`FreshnessSummary`] com as mesmas chaves (as ausentes são `None`).
//! - Respostas de direitos que omitem campos obrigatórios (`{ chat_enabled: true }`) valem como o [`entitlements`]
//!   vazio (ver o porte do provedor).
//! - `CustomizationMarketplaceConfiguration` (`customizationMarketplaceSources.ts`) depende do serviço de
//!   marketplace, sem porte: as duas chaves que o teste lê são constantes locais.
//! - `TestInstantiationService.stub` são os campos de [`DefaultAccountProviderServices`]; `IExtensionService`,
//!   `ICommandService` e `IHostService` ficam como dublês mínimos.

#[path = "../../je-platform/tests/common/mod.rs"]
mod common;

use std::cell::{Cell, RefCell};
use std::future::Future;
use std::rc::Rc;

use common::files::async_test;
use indexmap::IndexMap;
use je_base::JsString;
use je_base::buffer::{VSBuffer, buffer_to_stream};
use je_base::cancellation::CancellationToken;
use je_base::default_account::{IDefaultAccount, IDefaultAccountAuthenticationProvider, IEntitlementsData, IPolicyData};
use je_base::dynamic::{Dynamic, json_stringify};
use je_base::errors::Error;
use je_base::event::{Emitter, Event};
use je_base::lifecycle::{DisposableStore, IDisposable};
use je_base::parts::request::{HeaderValue, IHeaders, IRequestContext, IRequestOptions, IRequestResponse};
use je_base::policy::{ManagedSettingsData, PolicyValue};
use je_base::product::{IChatProvider, ICopilotVersions};
use je_base::promise::{DeferredPromise, Promise};
use je_base::runtime;
use je_base::uri::URI;
use je_platform::commands::{CommandEvent, ICommandService};
use je_platform::configuration::service::IConfigurationService;
use je_platform::configuration::test_configuration_service::TestConfigurationService;
use je_platform::keybinding::mock_keybinding_service::MockContextKeyService;
use je_platform::log::NullLogService;
use je_platform::policy::PolicyDefinitions;
use je_platform::policy::copilot_managed_settings::{
    COPILOT_FORCE_REMOTE_SETTINGS_REFRESH_KEY, IFileManagedSettingsService, INativeManagedSettingsService, ManagedSettingsChannel, NullFileManagedSettingsService,
    NullNativeManagedSettingsService, RawManagedSettingsData,
};
use je_platform::policy::managed_settings_freshness::{
    IManagedSettingsFreshness, IManagedSettingsFreshnessEffective, IManagedSettingsFreshnessScope, ManagedSettingsFreshnessFailure, ManagedSettingsFreshnessState,
};
use je_platform::product::ProductService;
use je_platform::request::{AuthInfo, Credentials, IRequestService, RequestCompleteEvent};
use je_platform::storage::InMemoryStorageService;
use je_platform::telemetry::null_telemetry_service;
use je_platform::url::whatwg::{hostname, new_url};
use je_extensions::scanner::ExtensionDescription;
use je_workbench::services::accounts::default_account::{
    DefaultAccountService, IDefaultAccountProvider, IDefaultAccountRefreshOptions, IDefaultAccountService, ISignInOptions, ManagedSettingsFetchStatus,
};
use je_workbench::services::accounts::default_account_provider::{
    DefaultAccountProvider, DefaultAccountProviderServices, IAccountPolicyData, IDefaultAccountConfig, IDefaultAccountConfigAuthenticationProvider,
    IDefaultAccountConfigProvider, IDefaultAccountData, IManagedSettingsFetchContext, IManagedSettingsPatch, IManagedSettingsResult,
};
use je_workbench::services::accounts::managed_settings::IManagedSettingsCompatibilityError;
use je_workbench::services::authentication::authentication::{
    AccountPreferenceChange, AuthenticationScopeListOrRequest, AuthenticationSession, AuthenticationSessionAccount, AuthenticationSessionsChange,
    AuthenticationSessionsChangeEvent, IAuthenticationCreateSessionOptions, IAuthenticationExtensionsService, IAuthenticationGetSessionsOptions, IAuthenticationService,
};
use je_workbench::services::extensions::extensions::{ActivationKind, IExtensionDescription, IExtensionService};
use je_workbench::services::host::host::{IHostService, ReloadOptions};
use je_workbench::services::policies::account_policy_service::{AccountPolicyGateState, AccountPolicyService, IAccountPolicyGateService};
use je_workbench::test::browser::workbench_test_services::TestEnvironmentService;
use serde_json::json;

const ACCOUNT_ID: &str = "account";
/// O `Date.now()` em que cada caso começa.
const START_TIME: u64 = 1_800_000_000_000;
const HOUR: u64 = 60 * 60 * 1000;
const COPILOT_CONNECTORS_ENABLED: &str = "chat.customizations.copilotConnectors.enabled";
const AGENT_FINDER_PUBLIC_FEED_ENABLED: &str = "chat.customizations.marketplace.sources.publicFeed.enabled";
const AUTH_PROVIDER_SETTING: &str = "github.copilot.advanced.authProvider";
const MANAGED_SETTINGS_URL: &str = "https://api.github.com/copilot_internal/managed_settings";

// ---- infraestrutura ----

fn account_test<F: Future<Output = ()> + 'static>(body: impl FnOnce(Rc<DisposableStore>) -> F + 'static) {
    async_test(|disposables| async move {
        runtime::set_system_time(START_TIME);
        body(disposables.clone()).await;
        disposables.dispose();
    });
}

fn now() -> u64 {
    runtime::date_now()
}

fn sessions() -> Vec<AuthenticationSession> {
    vec![AuthenticationSession {
        id: "session".to_owned(),
        access_token: "token".to_owned(),
        account: AuthenticationSessionAccount::new("octocat", ACCOUNT_ID),
        scopes: vec!["user:email".to_owned()],
        id_token: None,
        expires_after: None,
        authorization_server: Some(URI::parse("https://github.com/login/oauth", false)),
    }]
}

fn second_session() -> AuthenticationSession {
    AuthenticationSession { id: "second-session".to_owned(), ..sessions().remove(0) }
}

fn second_account_session() -> AuthenticationSession {
    AuthenticationSession { account: AuthenticationSessionAccount::new("hubot", "second-account"), ..sessions().remove(0) }
}

fn json_response(data: serde_json::Value, status: u16, headers: &[(&str, &str)]) -> IRequestContext {
    raw_response(&data.to_string(), status, headers)
}

fn raw_response(text: &str, status: u16, headers: &[(&str, &str)]) -> IRequestContext {
    let mut response_headers = IHeaders::new();
    for (name, value) in headers {
        response_headers.insert((*name).to_owned(), HeaderValue::One((*value).to_owned()));
    }
    IRequestContext { res: IRequestResponse { headers: response_headers, status_code: Some(status) }, stream: buffer_to_stream(VSBuffer::from_string(text)), timings: None }
}

fn ok(data: serde_json::Value) -> Promise<IRequestContext> {
    Promise::resolved(json_response(data, 200, &[]))
}

fn status_response(data: serde_json::Value, status: u16) -> Promise<IRequestContext> {
    Promise::resolved(json_response(data, status, &[]))
}

fn unavailable() -> Promise<IRequestContext> {
    Promise::rejected(Error::new("managed settings unavailable"))
}

type RequestHandler = Box<dyn Fn(&IRequestOptions) -> Promise<IRequestContext>>;

struct TestRequestService {
    handler: RequestHandler,
    requests: RefCell<Vec<IRequestOptions>>,
}

impl TestRequestService {
    fn new(handler: impl Fn(&IRequestOptions) -> Promise<IRequestContext> + 'static) -> Rc<Self> {
        Rc::new(Self { handler: Box::new(handler), requests: RefCell::new(Vec::new()) })
    }

    fn request_count(&self) -> usize {
        self.requests.borrow().len()
    }

    fn requests(&self) -> Vec<IRequestOptions> {
        self.requests.borrow().clone()
    }

    fn managed_settings_requests(&self) -> Vec<IRequestOptions> {
        self.requests().into_iter().filter(|request| request.url.as_deref().is_some_and(|url| url.contains("/copilot_internal/managed_settings"))).collect()
    }
}

impl IRequestService for TestRequestService {
    fn on_did_complete_request(&self) -> Event<RequestCompleteEvent> {
        Event::none()
    }

    fn request(&self, options: IRequestOptions, _token: CancellationToken) -> Promise<IRequestContext> {
        self.requests.borrow_mut().push(options.clone());
        (self.handler)(&options)
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

type SessionsHandler = Rc<dyn Fn() -> Promise<Vec<AuthenticationSession>>>;

fn fixed_sessions(list: Vec<AuthenticationSession>) -> SessionsHandler {
    Rc::new(move || Promise::resolved(list.clone()))
}

#[derive(Clone, Debug, PartialEq)]
struct CreateSessionCall {
    scopes: Vec<String>,
    options: String,
}

struct TestAuthenticationService {
    get_sessions: SessionsHandler,
    on_did_change_sessions: Event<AuthenticationSessionsChange>,
    created: Rc<RefCell<Vec<CreateSessionCall>>>,
}

impl IAuthenticationService for TestAuthenticationService {
    fn on_did_register_authentication_provider(&self) -> Event<je_workbench::services::authentication::authentication::AuthenticationProviderInformation> {
        Event::none()
    }

    fn on_did_change_sessions(&self) -> Event<AuthenticationSessionsChange> {
        self.on_did_change_sessions.clone()
    }

    fn get_provider_ids(&self) -> Vec<String> {
        Vec::new()
    }

    fn get_accounts(&self, _id: &str) -> Promise<Vec<AuthenticationSessionAccount>> {
        Promise::resolved(Vec::new())
    }

    fn get_sessions(
        &self,
        _id: &str,
        _scope_list_or_request: Option<AuthenticationScopeListOrRequest>,
        _options: Option<IAuthenticationGetSessionsOptions>,
        _activate_immediate: Option<bool>,
    ) -> Promise<Vec<AuthenticationSession>> {
        (self.get_sessions)()
    }

    fn create_session(
        &self,
        _provider_id: &str,
        scope_list_or_request: AuthenticationScopeListOrRequest,
        options: Option<IAuthenticationCreateSessionOptions>,
    ) -> Promise<AuthenticationSession> {
        let AuthenticationScopeListOrRequest::Scopes(scopes) = scope_list_or_request else { panic!("escopos esperados") };
        let extra = options.and_then(|options| options.session_options.extra).unwrap_or(Dynamic::Undefined);
        self.created.borrow_mut().push(CreateSessionCall { scopes: scopes.clone(), options: json_stringify(&extra).unwrap_or_default() });
        Promise::resolved(AuthenticationSession::new("session", "token", AuthenticationSessionAccount::new("octocat", "account"), scopes))
    }

    /// `isAuthenticationProviderRegistered: () => true`.
    fn is_authentication_provider_registered(&self, _id: &str) -> bool {
        true
    }
}

struct TestAuthenticationExtensionsService;

impl IAuthenticationExtensionsService for TestAuthenticationExtensionsService {
    fn on_did_change_account_preference(&self) -> Event<AccountPreferenceChange> {
        Event::none()
    }

    fn get_account_preference(&self, _extension_id: &str, _provider_id: &str) -> Option<String> {
        None
    }

    fn update_account_preference(&self, _extension_id: &str, _provider_id: &str, _account: &AuthenticationSessionAccount) {}

    fn remove_account_preference(&self, _extension_id: &str, _provider_id: &str) {}
}

struct StubExtensionService;

impl IExtensionService for StubExtensionService {
    fn activate_by_event(&self, _activation_event: &str, _activation_kind: Option<ActivationKind>) -> Promise<()> {
        panic!("activateByEvent is not a function")
    }

    fn activation_event_is_done(&self, _activation_event: &str) -> bool {
        panic!("activationEventIsDone is not a function")
    }

    fn get_extension(&self, _id: &str) -> Promise<Option<IExtensionDescription>> {
        panic!("getExtension is not a function")
    }

    fn on_did_register_extensions(&self) -> Event<()> {
        panic!("onDidRegisterExtensions is not defined")
    }

    fn when_installed_extensions_registered(&self) -> Promise<bool> {
        panic!("whenInstalledExtensionsRegistered is not a function")
    }

    fn extensions(&self) -> Vec<ExtensionDescription> {
        panic!("extensions is not defined")
    }
}

struct StubCommandService;

impl ICommandService for StubCommandService {
    fn on_will_execute_command(&self) -> Event<CommandEvent> {
        Event::none()
    }

    fn on_did_execute_command(&self) -> Event<CommandEvent> {
        Event::none()
    }

    fn execute_command(&self, _command_id: &str, _args: Vec<Dynamic>) -> Promise<Dynamic> {
        panic!("executeCommand is not a function")
    }
}

/// `{ hasFocus: true, onDidChangeFocus: Event.None }`.
struct StubHostService;

impl IHostService for StubHostService {
    fn reload(&self, _options: Option<ReloadOptions>) -> Promise<()> {
        Promise::resolved(())
    }

    fn restart(&self) -> Promise<()> {
        Promise::resolved(())
    }
}

struct TestNativeManagedSettingsService(ManagedSettingsData);

impl INativeManagedSettingsService for TestNativeManagedSettingsService {
    fn managed_settings(&self) -> ManagedSettingsData {
        self.0.clone()
    }

    fn on_did_change_managed_settings(&self) -> Event<ManagedSettingsData> {
        Event::none()
    }

    fn initialize(&self) -> Promise<ManagedSettingsData> {
        Promise::resolved(self.0.clone())
    }

    fn update_policy_definitions(&self, _policy_definitions: PolicyDefinitions) -> Promise<ManagedSettingsData> {
        Promise::resolved(self.0.clone())
    }
}

struct TestFileManagedSettingsService(ManagedSettingsData);

impl IFileManagedSettingsService for TestFileManagedSettingsService {
    fn raw_managed_settings(&self) -> RawManagedSettingsData {
        NullFileManagedSettingsService.raw_managed_settings()
    }

    fn managed_settings(&self) -> ManagedSettingsData {
        self.0.clone()
    }

    fn on_did_change_raw_managed_settings(&self) -> Event<RawManagedSettingsData> {
        Event::none()
    }

    fn on_did_change_managed_settings(&self) -> Event<ManagedSettingsData> {
        Event::none()
    }

    fn initialize(&self) -> Promise<ManagedSettingsData> {
        Promise::resolved(self.0.clone())
    }
}

fn managed_map(entries: &[(&str, PolicyValue)]) -> ManagedSettingsData {
    entries.iter().map(|(key, value)| ((*key).to_owned(), value.clone())).collect()
}

fn force_refresh_settings(value: bool) -> ManagedSettingsData {
    managed_map(&[(COPILOT_FORCE_REMOTE_SETTINGS_REFRESH_KEY, PolicyValue::Boolean(value))])
}

struct ProviderSetup {
    native: ManagedSettingsData,
    file: ManagedSettingsData,
    managed_settings_url: String,
    get_sessions: Option<SessionsHandler>,
    on_did_change_sessions: Option<Event<AuthenticationSessionsChange>>,
    configuration: Option<Rc<TestConfigurationService>>,
}

impl Default for ProviderSetup {
    fn default() -> Self {
        Self {
            native: ManagedSettingsData::new(),
            file: ManagedSettingsData::new(),
            managed_settings_url: MANAGED_SETTINGS_URL.to_owned(),
            get_sessions: None,
            on_did_change_sessions: None,
            configuration: None,
        }
    }
}

impl ProviderSetup {
    fn native_force() -> Self {
        Self { native: force_refresh_settings(true), ..Self::default() }
    }

    fn with_sessions(get_sessions: SessionsHandler) -> Self {
        Self { get_sessions: Some(get_sessions), ..Self::default() }
    }

    fn with_url(managed_settings_url: &str) -> Self {
        Self { managed_settings_url: managed_settings_url.to_owned(), ..Self::default() }
    }
}

fn github_provider() -> IDefaultAccountAuthenticationProvider {
    IDefaultAccountAuthenticationProvider { id: "github".to_owned(), name: "GitHub".to_owned(), enterprise: false, extra: Default::default() }
}

fn account_config(scopes: Vec<Vec<String>>, managed_settings_url: &str) -> IDefaultAccountConfig {
    IDefaultAccountConfig {
        preferred_extensions: Vec::new(),
        authentication_provider: IDefaultAccountConfigAuthenticationProvider {
            default: IDefaultAccountConfigProvider { id: "github".to_owned(), name: "GitHub".to_owned() },
            enterprise: IDefaultAccountConfigProvider { id: "github-enterprise".to_owned(), name: "GitHub Enterprise".to_owned() },
            enterprise_provider_config: AUTH_PROVIDER_SETTING.to_owned(),
            scopes,
        },
        token_entitlement_url: String::new(),
        entitlement_url: "https://api.github.com/copilot_internal/user".to_owned(),
        mcp_registry_data_url: String::new(),
        managed_settings_url: managed_settings_url.to_owned(),
    }
}

async fn create_provider(disposables: &Rc<DisposableStore>, request_service: Rc<TestRequestService>, setup: ProviderSetup) -> Rc<DefaultAccountProvider> {
    let configuration = setup.configuration.unwrap_or_else(|| TestConfigurationService::new(None));
    disposables.add(configuration.on_did_change_configuration_emitter.clone());
    let storage = disposables.add_rc(InMemoryStorageService::new());
    let services = DefaultAccountProviderServices {
        configuration_service: configuration,
        authentication_service: Rc::new(TestAuthenticationService {
            get_sessions: setup.get_sessions.unwrap_or_else(|| fixed_sessions(Vec::new())),
            on_did_change_sessions: setup.on_did_change_sessions.unwrap_or_else(Event::none),
            created: Rc::default(),
        }),
        authentication_extensions_service: Rc::new(TestAuthenticationExtensionsService),
        telemetry_service: null_telemetry_service(),
        extension_service: Rc::new(StubExtensionService),
        request_service,
        log_service: NullLogService::shared(),
        environment_service: TestEnvironmentService::new(),
        product_service: Rc::new(ProductService {
            version: "1.132.0".to_owned(),
            copilot_versions: Some(ICopilotVersions { runtime: "0.0.344".to_owned(), sdk: "0.1.0".to_owned(), extra: Default::default() }),
            ..ProductService::default()
        }),
        context_key_service: MockContextKeyService::new(),
        storage_service: Rc::new(storage),
        host_service: Rc::new(StubHostService),
        command_service: Rc::new(StubCommandService),
        native_managed_settings_service: Rc::new(TestNativeManagedSettingsService(setup.native)),
        file_managed_settings_service: Rc::new(TestFileManagedSettingsService(setup.file)),
    };
    let provider = disposables.add_rc(DefaultAccountProvider::new(account_config(vec![vec!["user:email".to_owned()]], &setup.managed_settings_url), services));
    provider.refresh(None).await.unwrap();
    provider
}

fn empty_policy() -> IPolicyData {
    je_workbench::services::policies::policy_value_callback::empty_policy_data()
}

fn create_cached_policy(force_remote_settings_refresh: bool) -> IAccountPolicyData {
    let mut policy_data = empty_policy();
    policy_data.managed_settings = Some(managed_map(&[
        (COPILOT_FORCE_REMOTE_SETTINGS_REFRESH_KEY, PolicyValue::Boolean(force_remote_settings_refresh)),
        ("permissions.disableBypassPermissionsMode", PolicyValue::String("disable".to_owned())),
    ]));
    IAccountPolicyData { managed_settings_fetched_at: Some(now()), ..IAccountPolicyData::new(ACCOUNT_ID, policy_data) }
}

fn with_scope(policy: IAccountPolicyData, scope: IManagedSettingsFreshnessScope) -> IAccountPolicyData {
    IAccountPolicyData { managed_settings_scope: Some(scope), ..policy }
}

fn scope_of(provider_id: &str, origin: &str) -> IManagedSettingsFreshnessScope {
    IManagedSettingsFreshnessScope { account_id: ACCOUNT_ID.to_owned(), authentication_provider_id: provider_id.to_owned(), endpoint_origin: origin.to_owned() }
}

fn github_scope() -> IManagedSettingsFreshnessScope {
    scope_of("github", "https://api.github.com")
}

fn patch_of(policy: &IAccountPolicyData) -> Option<IManagedSettingsPatch> {
    Some(IManagedSettingsPatch::of(policy.policy_data.managed_settings.clone()))
}

fn no_patch() -> Option<IManagedSettingsPatch> {
    Some(IManagedSettingsPatch::of(None))
}

fn force() -> Option<IDefaultAccountRefreshOptions> {
    Some(IDefaultAccountRefreshOptions { force_refresh: Some(true), ..Default::default() })
}

fn force_retry() -> Option<IDefaultAccountRefreshOptions> {
    Some(IDefaultAccountRefreshOptions { force_refresh: Some(true), retry_managed_settings: Some(true), ..Default::default() })
}

async fn managed(provider: &Rc<DefaultAccountProvider>, sessions: Vec<AuthenticationSession>, policy: Option<IAccountPolicyData>, options: Option<IDefaultAccountRefreshOptions>) -> IManagedSettingsResult {
    provider.get_managed_settings(sessions, policy, options, IManagedSettingsFetchContext::default()).await.unwrap()
}

#[derive(Debug, PartialEq, Default)]
struct FreshnessSummary {
    state: &'static str,
    source: Option<&'static str>,
    scope: Option<IManagedSettingsFreshnessScope>,
    failure: Option<&'static str>,
    http_status: Option<u32>,
    has_last_attempt: Option<bool>,
    has_satisfied_at: Option<bool>,
    has_scope: Option<bool>,
}

fn channel_name(channel: ManagedSettingsChannel) -> &'static str {
    channel.as_str()
}

fn effective_summary(state: &'static str, effective: &IManagedSettingsFreshnessEffective) -> FreshnessSummary {
    FreshnessSummary {
        state,
        source: Some(channel_name(effective.source)),
        has_last_attempt: Some(effective.last_attempt_at.is_some()),
        has_scope: effective.scope.is_some().then_some(true),
        ..Default::default()
    }
}

fn describe_freshness(freshness: &IManagedSettingsFreshness) -> FreshnessSummary {
    match freshness {
        IManagedSettingsFreshness::NotRequired => FreshnessSummary { state: "notRequired", ..Default::default() },
        IManagedSettingsFreshness::Satisfied { source, scope, last_attempt_at, .. } => FreshnessSummary {
            state: "satisfied",
            source: Some(channel_name(*source)),
            scope: Some(scope.clone()),
            has_last_attempt: Some(last_attempt_at.is_some()),
            has_satisfied_at: Some(true),
            ..Default::default()
        },
        IManagedSettingsFreshness::Pending(effective) => effective_summary("pending", effective),
        IManagedSettingsFreshness::Blocked { effective, failure } => FreshnessSummary {
            failure: Some(failure.as_str()),
            http_status: match failure {
                ManagedSettingsFreshnessFailure::HttpError { http_status } => Some(*http_status),
                _ => None,
            },
            ..effective_summary("blocked", effective)
        },
    }
}

fn satisfied(source: &'static str, scope: IManagedSettingsFreshnessScope) -> FreshnessSummary {
    FreshnessSummary { state: "satisfied", source: Some(source), scope: Some(scope), has_last_attempt: Some(true), has_satisfied_at: Some(true), ..Default::default() }
}

fn pending(source: &'static str, has_last_attempt: bool, has_scope: bool) -> FreshnessSummary {
    FreshnessSummary { state: "pending", source: Some(source), has_last_attempt: Some(has_last_attempt), has_scope: has_scope.then_some(true), ..Default::default() }
}

fn blocked(source: &'static str, failure: &'static str, has_last_attempt: bool, has_scope: bool) -> FreshnessSummary {
    FreshnessSummary { state: "blocked", source: Some(source), failure: Some(failure), has_last_attempt: Some(has_last_attempt), has_scope: has_scope.then_some(true), ..Default::default() }
}

fn entitlements_with(chat_enabled: bool, plan: &str) -> IEntitlementsData {
    IEntitlementsData {
        access_type_sku: String::new(),
        chat_enabled,
        assigned_date: String::new(),
        can_signup_for_limited: false,
        copilot_plan: plan.to_owned(),
        organization_login_list: Vec::new(),
        analytics_tracking_id: String::new(),
        limited_user_quotas: None,
        monthly_quotas: None,
        is_staff: None,
        limited_user_reset_date: None,
        quota_reset_date: None,
        quota_reset_date_utc: None,
        token_based_billing: None,
        can_upgrade_plan: None,
        cloud_session_storage_enabled: None,
        quota_snapshots: None,
        endpoints: None,
        extra: Default::default(),
    }
}

fn entitlements(chat_enabled: bool) -> IEntitlementsData {
    entitlements_with(chat_enabled, "")
}

fn compat_error() -> IManagedSettingsCompatibilityError {
    IManagedSettingsCompatibilityError { error_code: "client_update_required", client_version: None, minimum_client_version: None }
}

fn is_user(options: &IRequestOptions) -> bool {
    options.url.as_deref().is_some_and(|url| url.ends_with("/copilot_internal/user"))
}

fn is_managed(options: &IRequestOptions) -> bool {
    options.url.as_deref().is_some_and(|url| url.contains("/copilot_internal/managed_settings"))
}

fn unexpected(options: &IRequestOptions) -> Promise<IRequestContext> {
    Promise::rejected(Error::new(format!("Unexpected request: {}", options.url.clone().unwrap_or_default())))
}

fn url_hostname(url: &str) -> String {
    hostname(&new_url(&JsString::from(url))).to_string_lossy()
}

fn authorization(options: &IRequestOptions) -> Option<String> {
    match options.headers.as_ref()?.get("Authorization")? {
        HeaderValue::One(value) => Some(value.clone()),
        HeaderValue::Many(values) => values.first().cloned(),
    }
}

fn policy_model(provider: &Rc<DefaultAccountProvider>) -> Option<PolicyValue> {
    provider.policy_data().and_then(|data| data.managed_settings).and_then(|settings| settings.get("model").cloned())
}

fn account_session_id(provider: &Rc<DefaultAccountProvider>) -> Option<String> {
    provider.default_account().map(|account| account.session_id)
}

// ---- suíte DefaultAccountProvider ----

#[test]
fn cached_settings_perform_one_startup_compatibility_fetch() {
    account_test(|disposables| async move {
        let request_service = TestRequestService::new(|_| ok(json!({ "forceRemoteSettingsRefresh": true, "permissions": { "disableBypassPermissionsMode": "disable" } })));
        let provider = create_provider(&disposables, request_service.clone(), ProviderSetup::default()).await;
        let cached_policy = create_cached_policy(true);

        let first = managed(&provider, sessions(), Some(cached_policy.clone()), None).await;
        let second = managed(&provider, sessions(), Some(cached_policy.clone()), None).await;

        let requests = request_service.requests();
        assert_eq!(request_service.request_count(), 1);
        assert_eq!(requests[0].url.as_deref().unwrap().split_once('?').map(|(_, query)| query), Some("client_id=vscode&client_version=1.132.0&copilot_runtime_version=0.0.344"));
        assert_eq!(requests[0].disable_cache, Some(true));
        assert_eq!(first.data, patch_of(&cached_policy));
        assert_eq!(second.data, patch_of(&cached_policy));
    });
}

#[test]
fn settings_without_a_refresh_requirement_use_the_cache_without_fetching() {
    account_test(|disposables| async move {
        let request_service = TestRequestService::new(|_| ok(json!({ "permissions": { "disableBypassPermissionsMode": "disable" } })));
        let provider = create_provider(&disposables, request_service.clone(), ProviderSetup::default()).await;
        let cached_policy = create_cached_policy(false);

        let first = managed(&provider, sessions(), Some(cached_policy.clone()), None).await;
        let second = managed(&provider, sessions(), Some(cached_policy.clone()), None).await;

        assert_eq!(request_service.request_count(), 0);
        assert_eq!(first.data, patch_of(&cached_policy));
        assert_eq!(second.data, patch_of(&cached_policy));
    });
}

#[test]
fn force_refresh_fetches_fresh_even_when_the_cache_is_fresh_without_it_the_cache_is_honored() {
    account_test(|disposables| async move {
        let request_service = TestRequestService::new(|_| ok(json!({ "permissions": { "disableBypassPermissionsMode": "disable" } })));
        let provider = create_provider(&disposables, request_service.clone(), ProviderSetup::default()).await;
        let cached_policy = create_cached_policy(false);

        // Without forceRefresh the fresh cache is served with no network round-trip.
        let cached = managed(&provider, sessions(), Some(cached_policy.clone()), None).await;
        // The forceRefresh command bypasses the fresh cache and fetches.
        let forced = managed(&provider, sessions(), Some(cached_policy.clone()), force_retry()).await;

        assert_eq!(request_service.request_count(), 1);
        assert_eq!(cached.data, patch_of(&cached_policy));
        assert_eq!(forced.data, Some(IManagedSettingsPatch::of(Some(managed_map(&[("permissions.disableBypassPermissionsMode", PolicyValue::String("disable".to_owned()))])))));
    });
}

#[test]
fn entitlement_refresh_preserves_satisfied_governed_managed_settings() {
    account_test(|disposables| async move {
        let fresh = json!({
            "access_type_sku": "copilot_business_seat",
            "chat_enabled": true,
            "assigned_date": "2026-01-01",
            "can_signup_for_limited": false,
            "copilot_plan": "business",
            "organization_login_list": [],
            "analytics_tracking_id": "tracking-id",
        });
        let request_service = TestRequestService::new(move |options| if options.call_site == "defaultAccount.entitlements" { ok(fresh.clone()) } else { unexpected(options) });
        let provider = create_provider(&disposables, request_service.clone(), ProviderSetup::default()).await;
        let fetched_at = now() - 1000;
        let managed_settings_scope = github_scope();
        let cached_policy = with_scope(create_cached_policy(true), managed_settings_scope.clone());
        let policy_data = IAccountPolicyData { entitlements_fetched_at: Some(fetched_at), managed_settings_fetched_at: Some(fetched_at), ..cached_policy };
        let mut individual = entitlements_with(true, "individual");
        individual.access_type_sku = "copilot_business_seat".to_owned();
        individual.assigned_date = "2026-01-01".to_owned();
        individual.analytics_tracking_id = "tracking-id".to_owned();
        provider.set_default_account(Some(IDefaultAccountData {
            default_account: IDefaultAccount {
                authentication_provider: github_provider(),
                account_name: sessions()[0].account.label.clone(),
                session_id: sessions()[0].id.clone(),
                enterprise: false,
                entitlements_data: Some(individual),
                extra: Default::default(),
            },
            account_id: ACCOUNT_ID.to_owned(),
            enterprise_uri: None,
            policy_data: Some(policy_data),
            copilot_token_info: None,
        }));
        provider.set_managed_settings_freshness(IManagedSettingsFreshness::Satisfied {
            source: ManagedSettingsChannel::Server,
            scope: managed_settings_scope.clone(),
            last_attempt_at: Some(fetched_at),
            satisfied_at: fetched_at,
        });

        let refreshed = provider
            .get_default_account_from_authenticated_sessions(github_provider(), sessions(), Some(IDefaultAccountRefreshOptions { refresh_entitlements: Some(true), ..Default::default() }), None)
            .await
            .unwrap();

        assert_eq!(request_service.requests().iter().map(|request| request.call_site.clone()).collect::<Vec<_>>(), vec!["defaultAccount.entitlements".to_owned()]);
        assert_eq!(refreshed.and_then(|data| data.default_account.entitlements_data).map(|data| data.copilot_plan), Some("business".to_owned()));
        assert_eq!(describe_freshness(&provider.managed_settings_freshness()), satisfied("server", managed_settings_scope));
    });
}

#[test]
fn settings_without_a_refresh_requirement_refetch_only_after_the_cache_becomes_stale() {
    account_test(|disposables| async move {
        let request_service = TestRequestService::new(|_| ok(json!({})));
        let provider = create_provider(&disposables, request_service.clone(), ProviderSetup::default()).await;
        let cached_policy = create_cached_policy(false);

        managed(&provider, sessions(), Some(cached_policy.clone()), None).await;
        managed(&provider, sessions(), Some(cached_policy.clone()), None).await;
        managed(&provider, sessions(), Some(IAccountPolicyData { managed_settings_fetched_at: Some(now() - HOUR), ..cached_policy }), None).await;

        assert_eq!(request_service.request_count(), 1);
    });
}

#[test]
fn outstanding_compatibility_error_revalidates_instead_of_serving_a_fresh_cache() {
    account_test(|disposables| async move {
        let request_service = TestRequestService::new(|_| ok(json!({})));
        let provider = create_provider(&disposables, request_service.clone(), ProviderSetup::default()).await;
        provider.set_managed_settings_compatibility_error(Some(compat_error()));
        let cached_policy = create_cached_policy(false);

        let result = managed(&provider, sessions(), Some(cached_policy), None).await;

        assert_eq!(request_service.request_count(), 1);
        assert_eq!(result.compatibility_error, None);
    });
}

#[test]
fn a_fresh_cache_from_a_different_scope_is_not_reused() {
    account_test(|disposables| async move {
        let request_service = TestRequestService::new(|_| ok(json!({ "permissions": { "disableBypassPermissionsMode": "disable" } })));
        let provider = create_provider(&disposables, request_service.clone(), ProviderSetup::default()).await;
        let cached_policy = with_scope(create_cached_policy(false), scope_of("github-enterprise", "https://api.ghe.example.com"));

        // The cache was captured for a different provider/endpoint than the current github/api.github.com scope, so
        // it must be revalidated rather than served for the rest of the cache lifetime.
        let result = managed(&provider, sessions(), Some(cached_policy), None).await;

        assert_eq!(request_service.request_count(), 1);
        assert_eq!(result.data, Some(IManagedSettingsPatch::of(Some(managed_map(&[("permissions.disableBypassPermissionsMode", PolicyValue::String("disable".to_owned()))])))));
    });
}

#[test]
fn a_fresh_cache_from_a_different_scope_is_not_retained_when_refetch_fails() {
    account_test(|disposables| async move {
        let request_service = TestRequestService::new(|_| unavailable());
        let provider = create_provider(&disposables, request_service.clone(), ProviderSetup::default()).await;
        let cached_policy = with_scope(create_cached_policy(false), scope_of("github-enterprise", "https://api.ghe.example.com"));

        let result = managed(&provider, sessions(), Some(cached_policy), None).await;

        assert_eq!(request_service.request_count(), 1);
        assert_eq!(result.data, no_patch());
        assert_eq!(result.fetched_at, None);
        assert_eq!(result.scope, Some(github_scope()));
    });
}

#[test]
fn a_cache_from_a_different_scope_is_not_retained_by_forced_refresh_failure_or_retry_blocking() {
    account_test(|disposables| async move {
        let request_service = TestRequestService::new(|_| unavailable());
        let provider = create_provider(&disposables, request_service.clone(), ProviderSetup::native_force()).await;
        let cached_policy = with_scope(create_cached_policy(false), scope_of("github-enterprise", "https://api.ghe.example.com"));

        let failed = managed(&provider, sessions(), Some(cached_policy.clone()), None).await;
        let blocked = managed(&provider, sessions(), Some(cached_policy), None).await;

        assert_eq!(request_service.request_count(), 1);
        for result in [failed, blocked] {
            assert_eq!(result.data, no_patch());
            assert_eq!(result.fetched_at, None);
            assert_eq!(result.scope, Some(github_scope()));
        }
    });
}

#[test]
fn fresh_404_clears_a_cached_server_requirement() {
    account_test(|disposables| async move {
        let request_service = TestRequestService::new(|_| status_response(json!({}), 404));
        let provider = create_provider(&disposables, request_service.clone(), ProviderSetup::default()).await;
        let cached_policy = create_cached_policy(true);

        let result = managed(&provider, sessions(), Some(cached_policy), None).await;

        assert_eq!(request_service.request_count(), 1);
        assert_eq!(provider.managed_settings_fetch_status(), Some(ManagedSettingsFetchStatus::Http(404)));
        assert_eq!(result.data, no_patch());
        assert_eq!(provider.managed_settings_compatibility_error(), None);
        assert_eq!(provider.managed_settings_freshness(), IManagedSettingsFreshness::NotRequired);
    });
}

#[test]
fn managed_settings_404_does_not_retry_with_another_authentication_session() {
    account_test(|disposables| async move {
        let request_service = TestRequestService::new(|_| status_response(json!({}), 404));
        let provider = create_provider(&disposables, request_service.clone(), ProviderSetup::default()).await;

        let mut both = sessions();
        both.push(second_session());
        let result = managed(&provider, both, None, None).await;

        assert_eq!(request_service.request_count(), 1);
        assert_eq!(provider.managed_settings_fetch_status(), Some(ManagedSettingsFetchStatus::Http(404)));
        assert_eq!(result.data, no_patch());
    });
}

#[test]
fn fresh_404_satisfies_a_native_refresh_requirement() {
    account_test(|disposables| async move {
        let request_service = TestRequestService::new(|_| status_response(json!({}), 404));
        let provider = create_provider(&disposables, request_service, ProviderSetup::native_force()).await;

        managed(&provider, sessions(), None, None).await;

        assert_eq!(describe_freshness(&provider.managed_settings_freshness()), satisfied("nativeMdm", github_scope()));
    });
}

#[test]
fn a_466_blocks_and_does_not_fall_back_to_cached_server_managed_settings() {
    account_test(|disposables| async move {
        let request_service = TestRequestService::new(|_| {
            status_response(json!({ "error_code": "client_update_required", "client_id": "vscode", "client_version": "1.132.0", "minimum_client_version": "1.133.0" }), 466)
        });
        let provider = create_provider(&disposables, request_service, ProviderSetup::default()).await;
        let cached_policy = create_cached_policy(false);

        let result = managed(&provider, sessions(), Some(cached_policy), force()).await;

        assert_eq!(provider.managed_settings_fetch_status(), Some(ManagedSettingsFetchStatus::Http(466)));
        assert_eq!(result.data, no_patch());
        assert_eq!(
            provider.managed_settings_compatibility_error(),
            Some(IManagedSettingsCompatibilityError {
                error_code: "client_update_required",
                client_version: Some(JsString::from("1.132.0")),
                minimum_client_version: Some(JsString::from("1.133.0")),
            })
        );
    });
}

#[test]
fn malformed_466_still_blocks_without_compatibility_metadata() {
    account_test(|disposables| async move {
        let request_service = TestRequestService::new(|_| status_response(json!({ "error_code": "unexpected" }), 466));
        let provider = create_provider(&disposables, request_service, ProviderSetup::default()).await;

        let result = managed(&provider, sessions(), Some(create_cached_policy(false)), force()).await;

        assert_eq!(result.data, no_patch());
        assert_eq!(provider.managed_settings_compatibility_error(), Some(compat_error()));
    });
}

#[test]
fn a_466_is_a_blocked_forced_refresh_and_keeps_cached_restrictions() {
    account_test(|disposables| async move {
        let request_service = TestRequestService::new(|_| status_response(json!({ "error_code": "client_update_required", "client_id": "vscode" }), 466));
        let provider = create_provider(&disposables, request_service, ProviderSetup::default()).await;
        let cached_policy = create_cached_policy(true);

        let result = managed(&provider, sessions(), Some(cached_policy.clone()), None).await;

        assert_eq!(describe_freshness(&provider.managed_settings_freshness()), blocked("server", "updateRequired", true, true));
        assert_eq!(result.data, patch_of(&cached_policy));
    });
}

#[test]
fn failed_forced_refresh_retains_cached_managed_settings_when_no_rejection_is_known() {
    account_test(|disposables| async move {
        let request_service = TestRequestService::new(|_| unavailable());
        let provider = create_provider(&disposables, request_service.clone(), ProviderSetup::default()).await;
        let cached_policy = create_cached_policy(false);

        let result = managed(&provider, sessions(), Some(cached_policy.clone()), force()).await;

        assert_eq!(request_service.request_count(), 1);
        assert_eq!(provider.managed_settings_fetch_status(), Some(ManagedSettingsFetchStatus::NoResponse));
        assert_eq!(result.data, patch_of(&cached_policy));
    });
}

#[test]
fn failed_forced_refresh_blocks_without_treating_cached_settings_as_fresh() {
    account_test(|disposables| async move {
        let request_service = TestRequestService::new(|_| unavailable());
        let provider = create_provider(&disposables, request_service.clone(), ProviderSetup::default()).await;
        let cached_policy = create_cached_policy(true);

        let result = managed(&provider, sessions(), Some(cached_policy.clone()), None).await;

        assert_eq!(request_service.request_count(), 1);
        assert_eq!(provider.managed_settings_fetch_status(), Some(ManagedSettingsFetchStatus::NoResponse));
        assert_eq!(describe_freshness(&provider.managed_settings_freshness()), blocked("server", "network", true, true));
        assert_eq!(result.data, patch_of(&cached_policy));
        assert_eq!(result.fetched_at, cached_policy.managed_settings_fetched_at);
    });
}

#[test]
fn retry_after_a_failed_forced_refresh_stays_forced_and_blocked() {
    account_test(|disposables| async move {
        let request_service = TestRequestService::new(|_| unavailable());
        let provider = create_provider(&disposables, request_service.clone(), ProviderSetup::default()).await;
        let cached_policy = create_cached_policy(true);

        let first = managed(&provider, sessions(), Some(cached_policy.clone()), None).await;
        let retry_policy = IAccountPolicyData {
            policy_data: first.data.map_or_else(empty_policy, IManagedSettingsPatch::into_policy_data),
            managed_settings_fetched_at: first.fetched_at,
            ..cached_policy
        };
        managed(&provider, sessions(), Some(retry_policy), force_retry()).await;

        assert_eq!(request_service.request_count(), 2);
        assert_eq!(describe_freshness(&provider.managed_settings_freshness()), blocked("server", "network", true, true));
    });
}

#[test]
fn automatic_refreshes_stop_after_failure_while_explicit_retry_bypasses_the_guard() {
    account_test(|disposables| async move {
        let request_service = TestRequestService::new(|_| unavailable());
        let provider = create_provider(&disposables, request_service.clone(), ProviderSetup::default()).await;
        let cached_policy = create_cached_policy(true);

        managed(&provider, sessions(), Some(cached_policy.clone()), None).await;
        managed(&provider, sessions(), Some(cached_policy.clone()), force()).await;
        managed(&provider, sessions(), Some(cached_policy), force_retry()).await;

        assert_eq!(request_service.request_count(), 2);
        assert_eq!(describe_freshness(&provider.managed_settings_freshness()), blocked("server", "network", true, true));
    });
}

#[test]
fn settings_without_a_refresh_requirement_do_not_latch_failures() {
    account_test(|disposables| async move {
        let request_service = TestRequestService::new(|_| unavailable());
        let provider = create_provider(&disposables, request_service.clone(), ProviderSetup::default()).await;
        let cached_policy = create_cached_policy(false);

        managed(&provider, sessions(), Some(cached_policy.clone()), force()).await;
        managed(&provider, sessions(), Some(cached_policy), force()).await;

        assert_eq!(request_service.request_count(), 2);
    });
}

#[test]
fn managed_settings_endpoint_outage_without_a_refresh_requirement_keeps_freshness_non_blocking() {
    account_test(|disposables| async move {
        let managed_settings_request_count = Rc::new(Cell::new(0));
        let counter = managed_settings_request_count.clone();
        let request_service = TestRequestService::new(move |options| {
            if is_user(options) {
                return ok(json!({ "chat_enabled": true }));
            }
            if is_managed(options) {
                counter.set(counter.get() + 1);
                if counter.get() == 1 {
                    return ok(json!({ "permissions": { "disableBypassPermissionsMode": "disable" } }));
                }
                return Promise::rejected(Error::new("managed settings request timed out"));
            }
            unexpected(options)
        });
        let provider = create_provider(&disposables, request_service.clone(), ProviderSetup::with_sessions(fixed_sessions(sessions()))).await;

        provider.refresh(force()).await.unwrap();
        let account_service = disposables.add_rc(DefaultAccountService::new(IChatProvider { id: "github".to_owned(), name: "GitHub".to_owned(), extra: Default::default() }));
        account_service.set_default_account_provider(provider.clone()).unwrap();
        account_service.refresh(None).await.unwrap();
        let gate_service = disposables.add_rc(AccountPolicyService::new(NullLogService::shared(), account_service.clone(), None, None, None));
        je_base::async_::timeout(0).promise.await.unwrap();

        assert_eq!(managed_settings_request_count.get(), 2);
        assert_eq!(request_service.managed_settings_requests().iter().map(|request| request.timeout).collect::<Vec<_>>(), vec![Some(5000), Some(5000)]);
        assert_eq!(
            provider.default_account(),
            Some(IDefaultAccount { authentication_provider: github_provider(), account_name: "octocat".to_owned(), session_id: "session".to_owned(), enterprise: false, entitlements_data: Some(entitlements(true)), extra: Default::default() })
        );
        assert_eq!(
            provider.policy_data().and_then(|data| data.managed_settings),
            Some(managed_map(&[("permissions.disableBypassPermissionsMode", PolicyValue::String("disable".to_owned()))]))
        );
        assert_eq!(provider.managed_settings_freshness(), IManagedSettingsFreshness::NotRequired);
        assert_eq!(provider.managed_settings_compatibility_error(), None);
        assert_eq!(gate_service.gate_info().state, AccountPolicyGateState::Inactive);
    });
}

#[test]
fn refreshing_to_local_only_permission_or_empty_settings_clears_cached_raw_governance() {
    account_test(|disposables| async move {
        let response = Rc::new(RefCell::new(json!({ "futureControl": {} })));
        let current = response.clone();
        let request_service = TestRequestService::new(move |options| {
            if is_user(options) {
                return ok(json!({ "chat_enabled": true }));
            }
            if is_managed(options) {
                return ok(current.borrow().clone());
            }
            unexpected(options)
        });
        let provider = create_provider(&disposables, request_service, ProviderSetup::with_sessions(fixed_sessions(sessions()))).await;
        let responses = [
            json!({ "futureControl": {} }),
            json!({ "vscode": { "temporaryAllowLocalHarness": false } }),
            json!({ "vscode": { "temporaryAllowLocalHarness": true }, "futureControl": {} }),
            json!({}),
        ];
        let mut results = Vec::new();
        for next in responses {
            *response.borrow_mut() = next;
            provider.refresh(force()).await.unwrap();
            let refreshed = provider.policy_data().and_then(|data| data.managed_settings_active);
            provider.refresh(None).await.unwrap();
            results.push((refreshed, provider.policy_data().and_then(|data| data.managed_settings_active)));
        }
        assert_eq!(results, vec![(Some(true), Some(true)), (Some(false), Some(false)), (Some(true), Some(true)), (Some(false), Some(false))]);
    });
}

#[test]
fn truthy_non_boolean_server_refresh_control_remains_non_blocking_after_an_endpoint_outage() {
    account_test(|disposables| async move {
        let managed_settings_request_count = Rc::new(Cell::new(0));
        let counter = managed_settings_request_count.clone();
        let request_service = TestRequestService::new(move |options| {
            if is_user(options) {
                return ok(json!({ "chat_enabled": true }));
            }
            if is_managed(options) {
                counter.set(counter.get() + 1);
                if counter.get() == 1 {
                    return ok(json!({ "forceRemoteSettingsRefresh": "true", "permissions": { "disableBypassPermissionsMode": "disable" } }));
                }
                return Promise::rejected(Error::new("managed settings request timed out"));
            }
            unexpected(options)
        });
        let provider = create_provider(&disposables, request_service, ProviderSetup::with_sessions(fixed_sessions(sessions()))).await;

        provider.refresh(force()).await.unwrap();

        assert_eq!(managed_settings_request_count.get(), 2);
        assert_eq!(
            provider.policy_data().and_then(|data| data.managed_settings),
            Some(managed_map(&[
                ("forceRemoteSettingsRefresh", PolicyValue::String("true".to_owned())),
                ("permissions.disableBypassPermissionsMode", PolicyValue::String("disable".to_owned())),
            ]))
        );
        assert_eq!(provider.managed_settings_freshness(), IManagedSettingsFreshness::NotRequired);
        assert_eq!(provider.managed_settings_compatibility_error(), None);
    });
}

#[test]
fn non_466_managed_settings_failures_remain_non_blocking_without_a_refresh_requirement() {
    account_test(|disposables| async move {
        type Failure = (&'static str, Rc<dyn Fn() -> IRequestContext>);
        let failures: Vec<Failure> = vec![
            ("http error", Rc::new(|| json_response(json!({}), 500, &[]))),
            ("malformed response", Rc::new(|| raw_response("{", 200, &[]))),
            ("rate limited", Rc::new(|| json_response(json!({}), 429, &[("retry-after", "60")]))),
        ];
        let mut outcomes = Vec::new();
        for (name, response) in failures {
            let request_service = TestRequestService::new(move |_| Promise::resolved(response()));
            let provider = create_provider(&disposables, request_service, ProviderSetup::default()).await;
            let cached_policy = create_cached_policy(false);
            let result = managed(&provider, sessions(), Some(cached_policy.clone()), force()).await;
            outcomes.push((
                name,
                provider.managed_settings_fetch_status(),
                result.data,
                result.fetched_at == cached_policy.managed_settings_fetched_at,
                provider.managed_settings_freshness(),
                provider.managed_settings_compatibility_error(),
            ));
        }

        let cached = patch_of(&create_cached_policy(false));
        assert_eq!(
            outcomes,
            vec![
                ("http error", Some(ManagedSettingsFetchStatus::Http(500)), cached.clone(), true, IManagedSettingsFreshness::NotRequired, None),
                ("malformed response", Some(ManagedSettingsFetchStatus::ParseError), cached.clone(), true, IManagedSettingsFreshness::NotRequired, None),
                ("rate limited", Some(ManagedSettingsFetchStatus::Http(429)), cached, true, IManagedSettingsFreshness::NotRequired, None),
            ]
        );
    });
}

#[test]
fn managed_settings_source_change_preserves_a_prior_blocked_state() {
    account_test(|disposables| async move {
        let request_service = TestRequestService::new(|_| unavailable());
        let provider = create_provider(&disposables, request_service.clone(), ProviderSetup::native_force()).await;

        managed(&provider, sessions(), None, None).await;
        provider.set_initialized(false);
        provider.on_managed_settings_source_changed();
        provider.get_default_account_from_authenticated_sessions(github_provider(), sessions(), force(), None).await.unwrap();

        assert_eq!(request_service.managed_settings_requests().len(), 1);
        assert_eq!(describe_freshness(&provider.managed_settings_freshness()), blocked("nativeMdm", "network", true, true));
    });
}

#[test]
fn managed_settings_failure_guard_is_scoped_to_the_account() {
    account_test(|disposables| async move {
        let request_service = TestRequestService::new(|_| unavailable());
        let provider = create_provider(&disposables, request_service.clone(), ProviderSetup::native_force()).await;

        managed(&provider, sessions(), None, None).await;
        managed(&provider, vec![second_account_session()], None, None).await;

        assert_eq!(request_service.request_count(), 2);
    });
}

#[test]
fn forced_managed_settings_attempt_uses_only_the_selected_authentication_session() {
    account_test(|disposables| async move {
        let request_service = TestRequestService::new(|_| unavailable());
        let provider = create_provider(&disposables, request_service.clone(), ProviderSetup::native_force()).await;

        let mut both = sessions();
        both.push(second_session());
        managed(&provider, both, None, None).await;

        assert_eq!(request_service.request_count(), 1);
    });
}

#[test]
fn settings_without_a_refresh_requirement_retain_session_fallback() {
    account_test(|disposables| async move {
        let request_count = Rc::new(Cell::new(0));
        let counter = request_count.clone();
        let request_service = TestRequestService::new(move |_| {
            counter.set(counter.get() + 1);
            if counter.get() == 1 { status_response(json!({}), 401) } else { ok(json!({})) }
        });
        let provider = create_provider(&disposables, request_service.clone(), ProviderSetup::default()).await;

        let mut both = sessions();
        both.push(second_session());
        managed(&provider, both, None, None).await;

        assert_eq!(request_service.request_count(), 2);
    });
}

#[test]
fn public_github_issuer_metadata_does_not_classify_the_account_as_enterprise() {
    account_test(|disposables| async move {
        let request_service = TestRequestService::new(|_| ok(json!({ "chat_enabled": false })));
        let provider = create_provider(&disposables, request_service.clone(), ProviderSetup::with_sessions(fixed_sessions(sessions()))).await;

        assert_eq!(sessions()[0].authorization_server.as_ref().map(|uri| uri.to_string(false).to_string_lossy()), Some("https://github.com/login/oauth".to_owned()));
        assert_eq!(provider.resolve_git_hub_url(""), Some("https://github.com/".to_owned()));
        assert_eq!(provider.default_account().map(|account| account.authentication_provider), Some(github_provider()));
        assert_eq!(provider.default_account().map(|account| account.enterprise), Some(false));
        assert_eq!(request_service.requests().iter().map(|request| url_hostname(request.url.as_deref().unwrap())).collect::<Vec<_>>(), vec!["api.github.com".to_owned()]);
    });
}

#[test]
fn matching_authentication_sessions_are_not_duplicated_by_overlapping_accepted_scopes() {
    account_test(|disposables| async move {
        let broad_session = AuthenticationSession { scopes: ["read:user", "user:email", "repo", "workflow"].map(str::to_owned).to_vec(), ..sessions().remove(0) };
        let provider = create_provider(&disposables, TestRequestService::new(|_| ok(json!({}))), ProviderSetup { managed_settings_url: String::new(), get_sessions: Some(fixed_sessions(vec![broad_session])), ..ProviderSetup::default() }).await;

        let matching = provider
            .find_matching_provider_session(
                "github",
                vec![["read:user", "user:email", "repo", "workflow"].map(str::to_owned).to_vec(), vec!["user:email".to_owned()], vec!["read:user".to_owned()]],
            )
            .await
            .unwrap();

        assert_eq!(matching.map(|found| found.into_iter().map(|session| session.id).collect::<Vec<_>>()), Some(vec!["session".to_owned()]));
    });
}

#[test]
fn first_server_response_can_establish_and_satisfy_a_refresh_requirement() {
    account_test(|disposables| async move {
        let request_service = TestRequestService::new(|_| ok(json!({ "forceRemoteSettingsRefresh": true })));
        let provider = create_provider(&disposables, request_service, ProviderSetup::default()).await;

        managed(&provider, sessions(), None, None).await;

        assert_eq!(describe_freshness(&provider.managed_settings_freshness()), satisfied("server", github_scope()));
    });
}

#[test]
fn forced_refresh_remains_pending_until_the_live_request_completes() {
    account_test(|disposables| async move {
        let response = DeferredPromise::<IRequestContext>::new();
        let pending_response = response.promise.clone();
        let provider = create_provider(&disposables, TestRequestService::new(move |_| pending_response.clone()), ProviderSetup::default()).await;

        let refresh = provider.get_managed_settings(sessions(), Some(create_cached_policy(true)), None, IManagedSettingsFetchContext::default());
        je_base::async_::timeout(0).promise.await.unwrap();
        assert_eq!(describe_freshness(&provider.managed_settings_freshness()), pending("server", true, true));

        response.complete(json_response(json!({ "forceRemoteSettingsRefresh": true }), 200, &[]));
        refresh.await.unwrap();
        assert_eq!(provider.managed_settings_freshness().state(), ManagedSettingsFreshnessState::Satisfied);
    });
}

fn background_refresh_preserves_accepted_policy_while_fetching(force_refresh: bool) {
    account_test(move |disposables| async move {
        let started = Rc::new(DeferredPromise::<()>::new());
        let response = DeferredPromise::<IRequestContext>::new();
        let delay_settings = Rc::new(Cell::new(false));
        let (signal, handler_response, handler_delay) = (started.clone(), response.promise.clone(), delay_settings.clone());
        let started_signal = started;
        let request_service = TestRequestService::new(move |options| {
            if options.call_site == "defaultAccount.entitlements" {
                return ok(json!({ "chat_enabled": true }));
            }
            if options.call_site == "defaultAccount.managedSettings" {
                if handler_delay.get() {
                    signal.complete(());
                    return handler_response.clone();
                }
                return ok(json!({ "forceRemoteSettingsRefresh": true, "model": "original-model" }));
            }
            Promise::rejected(Error::new(format!("Unexpected request: {}", options.call_site)))
        });
        let provider = create_provider(&disposables, request_service, ProviderSetup::with_sessions(fixed_sessions(sessions()))).await;
        let account_service = disposables.add_rc(DefaultAccountService::new(IChatProvider { id: "github".to_owned(), name: "GitHub".to_owned(), extra: Default::default() }));
        account_service.set_default_account_provider(provider.clone()).unwrap();
        account_service.refresh(None).await.unwrap();
        let gate_service = disposables.add_rc(AccountPolicyService::new(NullLogService::shared(), account_service.clone(), None, None, None));
        gate_service.when_initialized().await.unwrap();
        let gate_states = Rc::new(RefCell::new(Vec::new()));
        let states = gate_states.clone();
        disposables.add(gate_service.on_did_change_gate_info().listen(move |info| states.borrow_mut().push(info.state)));

        if !force_refresh {
            let policy_data = provider.account_policy_data().expect("política em cache");
            provider.set_policy_data(Some(IAccountPolicyData { managed_settings_fetched_at: Some(now() - HOUR), ..policy_data }));
        }
        let accepted_freshness = provider.managed_settings_freshness();
        delay_settings.set(true);
        let refresh = account_service.refresh(Some(IDefaultAccountRefreshOptions { force_refresh: Some(force_refresh), ..Default::default() }));
        started_signal.promise.clone().await.unwrap();
        let during_refresh = (provider.managed_settings_freshness(), policy_model(&provider), gate_service.gate_info().state);
        response.complete(json_response(json!({ "forceRemoteSettingsRefresh": true, "model": "updated-model" }), 200, &[]));
        refresh.await.unwrap();

        assert_eq!(during_refresh, (accepted_freshness, Some(PolicyValue::String("original-model".to_owned())), AccountPolicyGateState::Inactive));
        assert_eq!(provider.managed_settings_freshness().state(), ManagedSettingsFreshnessState::Satisfied);
        assert_eq!(policy_model(&provider), Some(PolicyValue::String("updated-model".to_owned())));
        assert_eq!(gate_service.gate_info().state, AccountPolicyGateState::Inactive);
        assert_eq!(*gate_states.borrow(), Vec::<AccountPolicyGateState>::new());
    });
}

#[test]
fn expired_cache_background_refresh_preserves_accepted_policy_while_fetching() {
    background_refresh_preserves_accepted_policy_while_fetching(false);
}

#[test]
fn forced_background_refresh_preserves_accepted_policy_while_fetching() {
    background_refresh_preserves_accepted_policy_while_fetching(true);
}

#[test]
fn background_refresh_failure_blocks_until_a_live_retry_succeeds() {
    account_test(|disposables| async move {
        let started = Rc::new(RefCell::new(DeferredPromise::<()>::new()));
        let response = Rc::new(RefCell::new(DeferredPromise::<IRequestContext>::new()));
        let delay_settings = Rc::new(Cell::new(false));
        let (handler_started, handler_response, handler_delay) = (started.clone(), response.clone(), delay_settings.clone());
        let provider = create_provider(
            &disposables,
            TestRequestService::new(move |_| {
                if handler_delay.get() {
                    handler_started.borrow().complete(());
                    return handler_response.borrow().promise.clone();
                }
                ok(json!({ "forceRemoteSettingsRefresh": true }))
            }),
            ProviderSetup::default(),
        )
        .await;
        let cached_policy = create_cached_policy(true);
        managed(&provider, sessions(), Some(cached_policy.clone()), None).await;
        delay_settings.set(true);

        let refresh = provider.get_managed_settings(sessions(), Some(cached_policy.clone()), force(), IManagedSettingsFetchContext::default());
        let started_promise = started.borrow().promise.clone();
        started_promise.await.unwrap();
        let during_refresh = provider.managed_settings_freshness().state();
        response.borrow().error(Error::new("managed settings request timed out"));
        let failed = refresh.await.unwrap();
        let after_failure = provider.managed_settings_freshness();

        *started.borrow_mut() = DeferredPromise::new();
        *response.borrow_mut() = DeferredPromise::new();
        let retry = provider.get_managed_settings(sessions(), Some(cached_policy.clone()), force_retry(), IManagedSettingsFetchContext::default());
        let started_promise = started.borrow().promise.clone();
        started_promise.await.unwrap();
        let during_retry = provider.managed_settings_freshness().state();
        response.borrow().complete(json_response(json!({ "forceRemoteSettingsRefresh": true }), 200, &[]));
        retry.await.unwrap();

        assert_eq!(during_refresh, ManagedSettingsFreshnessState::Satisfied);
        assert_eq!(describe_freshness(&after_failure), blocked("server", "network", true, true));
        assert_eq!(failed.data, patch_of(&cached_policy));
        assert_eq!(during_retry, ManagedSettingsFreshnessState::Pending);
        assert_eq!(provider.managed_settings_freshness().state(), ManagedSettingsFreshnessState::Satisfied);
    });
}

#[test]
fn a_satisfied_refresh_does_not_authorize_a_different_account_while_fetching() {
    account_test(|disposables| async move {
        let started = Rc::new(DeferredPromise::<()>::new());
        let response = DeferredPromise::<IRequestContext>::new();
        let delay_settings = Rc::new(Cell::new(false));
        let (signal, pending_response, handler_delay) = (started.clone(), response.promise.clone(), delay_settings.clone());
        let provider = create_provider(
            &disposables,
            TestRequestService::new(move |_| {
                if handler_delay.get() {
                    signal.complete(());
                    return pending_response.clone();
                }
                ok(json!({}))
            }),
            ProviderSetup::native_force(),
        )
        .await;
        managed(&provider, sessions(), None, None).await;
        delay_settings.set(true);

        let refresh = provider.get_managed_settings(vec![second_account_session()], None, None, IManagedSettingsFetchContext::default());
        started.promise.clone().await.unwrap();
        let during_refresh = provider.managed_settings_freshness().state();
        response.complete(json_response(json!({}), 200, &[]));
        refresh.await.unwrap();

        assert_eq!(during_refresh, ManagedSettingsFreshnessState::Pending);
        assert_eq!(provider.managed_settings_freshness().state(), ManagedSettingsFreshnessState::Satisfied);
        match provider.managed_settings_freshness() {
            IManagedSettingsFreshness::Satisfied { scope, .. } => assert_eq!(scope.account_id, "second-account"),
            other => panic!("esperava Satisfied, veio {other:?}"),
        }
    });
}

#[test]
fn successful_retry_clears_a_blocked_requirement_when_the_server_removes_it() {
    account_test(|disposables| async move {
        let request_count = Rc::new(Cell::new(0));
        let counter = request_count.clone();
        let request_service = TestRequestService::new(move |_| {
            counter.set(counter.get() + 1);
            if counter.get() == 1 { unavailable() } else { ok(json!({})) }
        });
        let provider = create_provider(&disposables, request_service, ProviderSetup::default()).await;
        let cached_policy = create_cached_policy(true);

        let failed = managed(&provider, sessions(), Some(cached_policy.clone()), None).await;
        let retry_policy = IAccountPolicyData {
            policy_data: failed.data.map_or_else(empty_policy, IManagedSettingsPatch::into_policy_data),
            managed_settings_fetched_at: failed.fetched_at,
            ..cached_policy
        };
        managed(&provider, sessions(), Some(retry_policy), force_retry()).await;

        assert_eq!(request_count.get(), 2);
        assert_eq!(provider.managed_settings_freshness(), IManagedSettingsFreshness::NotRequired);
    });
}

#[test]
fn sign_out_closes_a_satisfied_native_refresh_gate() {
    account_test(|disposables| async move {
        let request_service = TestRequestService::new(|options| {
            if is_user(options) {
                return ok(json!({ "chat_enabled": true }));
            }
            if is_managed(options) {
                return ok(json!({}));
            }
            unexpected(options)
        });
        let provider = create_provider(&disposables, request_service, ProviderSetup::native_force()).await;
        let account = provider.get_default_account_from_authenticated_sessions(github_provider(), sessions(), force(), None).await.unwrap();
        assert!(account.is_some());
        provider.set_default_account(account);
        assert_eq!(provider.managed_settings_freshness().state(), ManagedSettingsFreshnessState::Satisfied);

        provider.set_default_account(None);

        assert_eq!(
            provider.managed_settings_freshness(),
            IManagedSettingsFreshness::Blocked {
                effective: IManagedSettingsFreshnessEffective { source: ManagedSettingsChannel::NativeMdm, scope: None, last_attempt_at: None },
                failure: ManagedSettingsFreshnessFailure::NoToken,
            }
        );
    });
}

#[test]
fn a_failed_session_lookup_keeps_the_signed_in_account_and_satisfied_refresh_gate() {
    account_test(|disposables| async move {
        let session_lookup_fails = Rc::new(Cell::new(false));
        let request_service = TestRequestService::new(|options| {
            if is_user(options) {
                return ok(json!({ "chat_enabled": true }));
            }
            if is_managed(options) {
                return ok(json!({ COPILOT_FORCE_REMOTE_SETTINGS_REFRESH_KEY: true }));
            }
            unexpected(options)
        });
        let fails = session_lookup_fails.clone();
        let get_sessions: SessionsHandler = Rc::new(move || {
            if fails.get() {
                // Mirrors the extension host restart race: the provider's RPC is torn down mid-lookup.
                return Promise::rejected(Error::cancellation());
            }
            Promise::resolved(sessions())
        });
        let provider = create_provider(&disposables, request_service, ProviderSetup::with_sessions(get_sessions)).await;
        let observed = Rc::new(RefCell::new(Vec::<Option<String>>::new()));
        let seen = observed.clone();
        disposables.add(provider.on_did_change_default_account().listen(move |account| seen.borrow_mut().push(account.as_ref().map(|account| account.session_id.clone()))));
        let snapshot = || (account_session_id(&provider), describe_freshness(&provider.managed_settings_freshness()), provider.policy_data().and_then(|data| data.managed_settings).and_then(|settings| settings.get(COPILOT_FORCE_REMOTE_SETTINGS_REFRESH_KEY).cloned()));
        let expected = || (Some("session".to_owned()), satisfied("server", github_scope()), Some(PolicyValue::Boolean(true)));
        let before_lookup_failure = snapshot();

        session_lookup_fails.set(true);
        provider.refresh(None).await.unwrap();

        assert_eq!(before_lookup_failure, expected());
        assert_eq!(snapshot(), expected());
        assert_eq!(*observed.borrow(), Vec::<Option<String>>::new());
    });
}

#[test]
fn a_failed_session_lookup_without_a_known_account_still_reports_a_missing_token() {
    account_test(|disposables| async move {
        let get_sessions: SessionsHandler = Rc::new(|| Promise::rejected(Error::cancellation()));
        let provider = create_provider(
            &disposables,
            TestRequestService::new(unexpected_handler),
            ProviderSetup { native: force_refresh_settings(true), get_sessions: Some(get_sessions), ..ProviderSetup::default() },
        )
        .await;

        assert_eq!(provider.default_account(), None);
        assert_eq!(describe_freshness(&provider.managed_settings_freshness()), blocked("nativeMdm", "noToken", false, false));
    });
}

fn unexpected_handler(options: &IRequestOptions) -> Promise<IRequestContext> {
    unexpected(options)
}

#[test]
fn native_false_disables_a_cached_server_refresh_requirement() {
    account_test(|disposables| async move {
        let request_service = TestRequestService::new(|_| unavailable());
        let provider = create_provider(&disposables, request_service, ProviderSetup { native: force_refresh_settings(false), ..ProviderSetup::default() }).await;
        let cached_policy = create_cached_policy(true);

        let result = managed(&provider, sessions(), Some(cached_policy.clone()), None).await;

        assert_eq!(provider.managed_settings_freshness(), IManagedSettingsFreshness::NotRequired);
        assert_eq!(result.data, patch_of(&cached_policy));
    });
}

fn http_error_503_summary(source: &'static str) -> FreshnessSummary {
    FreshnessSummary { http_status: Some(503), ..blocked(source, "httpError", true, true) }
}

#[test]
fn file_delivered_refresh_requirement_fails_closed() {
    account_test(|disposables| async move {
        let request_service = TestRequestService::new(|_| status_response(json!({}), 503));
        let provider = create_provider(&disposables, request_service, ProviderSetup { file: force_refresh_settings(true), ..ProviderSetup::default() }).await;

        managed(&provider, sessions(), None, None).await;

        assert_eq!(describe_freshness(&provider.managed_settings_freshness()), http_error_503_summary("file"));
    });
}

#[test]
fn stale_server_scope_cannot_override_a_file_delivered_refresh_requirement() {
    account_test(|disposables| async move {
        let request_service = TestRequestService::new(|_| status_response(json!({}), 503));
        let provider = create_provider(&disposables, request_service, ProviderSetup { file: force_refresh_settings(true), ..ProviderSetup::default() }).await;
        let cached_policy = with_scope(create_cached_policy(false), scope_of("github-enterprise", "https://api.enterprise.example.com"));

        managed(&provider, sessions(), Some(cached_policy), None).await;

        assert_eq!(describe_freshness(&provider.managed_settings_freshness()), http_error_503_summary("file"));
    });
}

#[test]
fn rate_limited_forced_refresh_blocks_automatic_retries() {
    account_test(|disposables| async move {
        let request_service = TestRequestService::new(|_| Promise::resolved(json_response(json!({}), 429, &[("retry-after", "60")])));
        let provider = create_provider(&disposables, request_service.clone(), ProviderSetup::native_force()).await;

        managed(&provider, sessions(), None, None).await;
        managed(&provider, sessions(), None, force()).await;
        provider.set_rate_limit_backoff_until(0);
        managed(&provider, sessions(), None, force_retry()).await;

        assert_eq!(request_service.request_count(), 2);
        assert_eq!(describe_freshness(&provider.managed_settings_freshness()), blocked("nativeMdm", "rateLimited", true, true));
    });
}

#[test]
fn shared_backoff_does_not_permanently_latch_managed_settings_without_an_attempted_request() {
    account_test(|disposables| async move {
        let request_service = TestRequestService::new(|_| ok(json!({})));
        let provider = create_provider(&disposables, request_service.clone(), ProviderSetup::default()).await;
        let cached_policy = create_cached_policy(true);

        provider.set_rate_limit_backoff_until(now() + 60_000);
        managed(&provider, sessions(), Some(cached_policy.clone()), None).await;
        provider.set_rate_limit_backoff_until(0);
        managed(&provider, sessions(), Some(cached_policy), None).await;

        assert_eq!(request_service.request_count(), 1);
        assert_eq!(provider.managed_settings_freshness(), IManagedSettingsFreshness::NotRequired);
    });
}

#[test]
fn malformed_forced_refresh_response_fails_closed() {
    account_test(|disposables| async move {
        let request_service = TestRequestService::new(|_| Promise::resolved(raw_response("{", 200, &[])));
        let provider = create_provider(&disposables, request_service, ProviderSetup::native_force()).await;

        managed(&provider, sessions(), None, None).await;

        assert_eq!(describe_freshness(&provider.managed_settings_freshness()), blocked("nativeMdm", "malformed", true, true));
    });
}

#[test]
fn forced_refresh_without_an_endpoint_fails_closed() {
    account_test(|disposables| async move {
        let provider = create_provider(&disposables, TestRequestService::new(|_| ok(json!({}))), ProviderSetup { native: force_refresh_settings(true), ..ProviderSetup::with_url("") }).await;

        managed(&provider, sessions(), None, None).await;

        assert_eq!(describe_freshness(&provider.managed_settings_freshness()), blocked("nativeMdm", "noUrl", false, false));
    });
}

#[test]
fn scoped_cached_server_requirement_fails_closed_when_the_endpoint_is_missing() {
    account_test(|disposables| async move {
        let provider = create_provider(&disposables, TestRequestService::new(|_| ok(json!({}))), ProviderSetup::with_url("")).await;
        let cached_policy = with_scope(create_cached_policy(true), github_scope());

        managed(&provider, sessions(), Some(cached_policy), None).await;

        assert_eq!(describe_freshness(&provider.managed_settings_freshness()), blocked("server", "noUrl", false, false));
    });
}

#[test]
fn re_enabled_refresh_requirement_restores_the_prior_blocked_state() {
    account_test(|disposables| async move {
        let request_service = TestRequestService::new(|_| unavailable());
        let provider = create_provider(&disposables, request_service.clone(), ProviderSetup::native_force()).await;

        managed(&provider, sessions(), None, None).await;
        provider.set_managed_settings_freshness(IManagedSettingsFreshness::NotRequired);
        managed(&provider, sessions(), None, force()).await;

        assert_eq!(request_service.request_count(), 1);
        assert_eq!(describe_freshness(&provider.managed_settings_freshness()), blocked("nativeMdm", "network", true, true));
    });
}

#[test]
fn forced_refresh_without_authentication_fails_closed_but_leaves_sign_in_available() {
    account_test(|disposables| async move {
        let provider = create_provider(&disposables, TestRequestService::new(|_| ok(json!({}))), ProviderSetup::native_force()).await;
        provider.refresh(None).await.unwrap();

        assert_eq!(describe_freshness(&provider.managed_settings_freshness()), blocked("nativeMdm", "noToken", false, false));
    });
}

#[test]
fn failed_fetches_retain_the_last_successful_managed_settings_without_renewing_them() {
    account_test(|disposables| async move {
        type Failure = (&'static str, Rc<dyn Fn() -> Promise<IRequestContext>>);
        let failures: Vec<Failure> = vec![
            ("no response", Rc::new(unavailable)),
            ("401", Rc::new(|| status_response(json!({}), 401))),
            ("403", Rc::new(|| status_response(json!({}), 403))),
        ];
        let fresh_cache = create_cached_policy(false);
        let stale_fetched_at = now() - 7 * 24 * HOUR; // long past the one-hour cache boundary
        let mut outcomes = Vec::new();

        for (name, response) in failures {
            let provider = create_provider(&disposables, TestRequestService::new(move |_| response()), ProviderSetup::default()).await;
            let while_fresh = managed(&provider, sessions(), Some(fresh_cache.clone()), force()).await;
            let once_stale = managed(&provider, sessions(), Some(IAccountPolicyData { managed_settings_fetched_at: Some(stale_fetched_at), ..fresh_cache.clone() }), force()).await;
            outcomes.push((name, (while_fresh.data, while_fresh.fetched_at), (once_stale.data, once_stale.fetched_at)));
        }

        // Only an affirmative service answer withdraws policy, and a failure never renews the cached timestamp.
        let expected: Vec<_> = ["no response", "401", "403"]
            .into_iter()
            .map(|name| (name, (patch_of(&fresh_cache), fresh_cache.managed_settings_fetched_at), (patch_of(&fresh_cache), Some(stale_fetched_at))))
            .collect();
        assert_eq!(outcomes, expected);
    });
}

#[test]
fn transient_failure_does_not_clear_an_update_required_state() {
    account_test(|disposables| async move {
        let request_count = Rc::new(Cell::new(0));
        let counter = request_count.clone();
        let request_service = TestRequestService::new(move |_| {
            counter.set(counter.get() + 1);
            if counter.get() == 1 {
                return status_response(json!({ "error_code": "client_update_required", "client_id": "vscode" }), 466);
            }
            unavailable()
        });
        let provider = create_provider(&disposables, request_service, ProviderSetup::default()).await;
        let cached_policy = create_cached_policy(false);

        managed(&provider, sessions(), Some(cached_policy.clone()), force()).await;
        let result = managed(&provider, sessions(), Some(cached_policy), force()).await;

        assert_eq!(provider.managed_settings_fetch_status(), Some(ManagedSettingsFetchStatus::NoResponse));
        assert_eq!(result.data, no_patch());
        assert_eq!(provider.managed_settings_compatibility_error(), Some(compat_error()));
    });
}

#[test]
fn successful_full_refresh_clears_a_prior_update_required_state() {
    account_test(|disposables| async move {
        let request_service = TestRequestService::new(|options| {
            if is_user(options) {
                return ok(json!({ "chat_enabled": true }));
            }
            if is_managed(options) {
                return ok(json!({}));
            }
            unexpected(options)
        });
        let provider = create_provider(&disposables, request_service, ProviderSetup::default()).await;
        provider.set_managed_settings_compatibility_error(Some(compat_error()));

        let account = provider.get_default_account_from_authenticated_sessions(github_provider(), sessions(), force(), None).await.unwrap();
        assert!(account.is_some());
        provider.set_default_account(account);

        assert_eq!(provider.managed_settings_compatibility_error(), None);
        assert_eq!(provider.policy_data().and_then(|data| data.managed_settings), Some(ManagedSettingsData::new()));
    });
}

fn session_change(provider_id: &str, label: &str, added: Vec<AuthenticationSession>, removed: Vec<AuthenticationSession>, changed: Vec<AuthenticationSession>) -> AuthenticationSessionsChange {
    AuthenticationSessionsChange {
        provider_id: provider_id.to_owned(),
        label: label.to_owned(),
        event: AuthenticationSessionsChangeEvent { added: Some(added), removed: Some(removed), changed: Some(changed) },
    }
}

#[test]
fn reconciles_a_replacement_without_a_signed_out_gap_and_preserves_removal_only_behavior() {
    account_test(|disposables| async move {
        let session_changes = Emitter::<AuthenticationSessionsChange>::new();
        disposables.add(session_changes.clone());
        let authentication_sessions = Rc::new(RefCell::new(sessions()));
        let current = authentication_sessions.clone();
        let provider = create_provider(
            &disposables,
            TestRequestService::new(|_| ok(json!({ "chat_enabled": true }))),
            ProviderSetup {
                managed_settings_url: String::new(),
                get_sessions: Some(Rc::new(move || Promise::resolved(current.borrow().clone()))),
                on_did_change_sessions: Some(session_changes.event()),
                ..ProviderSetup::default()
            },
        )
        .await;
        let observed = Rc::new(RefCell::new(Vec::<Option<String>>::new()));
        let seen = observed.clone();
        disposables.add(provider.on_did_change_default_account().listen(move |account| seen.borrow_mut().push(account.as_ref().map(|account| account.session_id.clone()))));
        let replacement_session = AuthenticationSession { id: "replacement-session".to_owned(), access_token: "replacement-token".to_owned(), ..sessions().remove(0) };
        *authentication_sessions.borrow_mut() = vec![replacement_session.clone()];
        let before_replacement = account_session_id(&provider);
        let replacement = provider
            .on_did_change_default_account()
            .filter(|account| account.as_ref().is_some_and(|account| account.session_id == "replacement-session"), None)
            .to_promise(None);

        session_changes.fire(session_change("github", "GitHub", vec![replacement_session.clone()], sessions(), Vec::new()));
        let after_replacement_event = account_session_id(&provider);
        let after_replacement = replacement.await.unwrap().map(|account| account.session_id);

        authentication_sessions.borrow_mut().clear();
        session_changes.fire(session_change("github", "GitHub", Vec::new(), vec![replacement_session], Vec::new()));

        assert_eq!(before_replacement, Some("session".to_owned()));
        assert_eq!(after_replacement_event, Some("session".to_owned()));
        assert_eq!(after_replacement, Some("replacement-session".to_owned()));
        assert_eq!(account_session_id(&provider), None);
        assert_eq!(*observed.borrow(), vec![Some("replacement-session".to_owned()), None]);
    });
}

#[test]
fn does_not_restore_a_removed_session_from_an_in_flight_replacement_refresh() {
    account_test(|disposables| async move {
        let session_changes = Emitter::<AuthenticationSessionsChange>::new();
        disposables.add(session_changes.clone());
        let refresh_started = Rc::new(DeferredPromise::<()>::new());
        let release_refresh = DeferredPromise::<IRequestContext>::new();
        let authentication_sessions = Rc::new(RefCell::new(sessions()));
        let block_refresh = Rc::new(Cell::new(false));
        let (signal, held, blocking, current) = (refresh_started.clone(), release_refresh.promise.clone(), block_refresh.clone(), authentication_sessions.clone());
        let provider = create_provider(
            &disposables,
            TestRequestService::new(move |options| {
                if blocking.get() && options.call_site == "defaultAccount.entitlements" {
                    signal.complete(());
                    return held.clone();
                }
                ok(json!({ "chat_enabled": true }))
            }),
            ProviderSetup {
                managed_settings_url: String::new(),
                get_sessions: Some(Rc::new(move || Promise::resolved(current.borrow().clone()))),
                on_did_change_sessions: Some(session_changes.event()),
                ..ProviderSetup::default()
            },
        )
        .await;
        let observed = Rc::new(RefCell::new(Vec::<Option<String>>::new()));
        let seen = observed.clone();
        disposables.add(provider.on_did_change_default_account().listen(move |account| seen.borrow_mut().push(account.as_ref().map(|account| account.session_id.clone()))));
        let replacement_session = AuthenticationSession { access_token: "replacement-token".to_owned(), ..sessions().remove(0) };
        *authentication_sessions.borrow_mut() = vec![replacement_session.clone()];
        block_refresh.set(true);

        session_changes.fire(session_change("github", "GitHub", vec![replacement_session.clone()], sessions(), Vec::new()));
        let replacement_refresh = provider.refresh(force());
        refresh_started.promise.clone().await.unwrap();

        authentication_sessions.borrow_mut().clear();
        session_changes.fire(session_change("github", "GitHub", Vec::new(), vec![replacement_session], Vec::new()));
        let after_removal = account_session_id(&provider);
        release_refresh.complete(json_response(json!({ "chat_enabled": false }), 200, &[]));
        replacement_refresh.await.unwrap();

        assert_eq!(after_removal, None);
        assert_eq!(account_session_id(&provider), None);
        assert_eq!(*observed.borrow(), vec![None]);
    });
}

fn enterprise_session(host: &str) -> AuthenticationSession {
    AuthenticationSession {
        id: format!("{host}-session"),
        access_token: format!("{host}-token"),
        authorization_server: Some(URI::parse(format!("https://{host}.ghe.com/login/oauth"), false)),
        ..sessions().remove(0)
    }
}

fn enterprise_configuration() -> Rc<TestConfigurationService> {
    TestConfigurationService::new(Some(Dynamic::object([(AUTH_PROVIDER_SETTING, Dynamic::string("github-enterprise"))])))
}

fn enterprise_response(request: &IRequestOptions) -> Promise<IRequestContext> {
    match request.call_site.as_str() {
        "defaultAccount.entitlements" => ok(json!({ "chat_enabled": true })),
        "defaultAccount.tokenEntitlements" => ok(json!({ "token": "mcp=1;sn=test:signature" })),
        "defaultAccount.managedSettings" => ok(json!({ "model": url_hostname(request.url.as_deref().unwrap()) })),
        "defaultAccount.mcpRegistryProvider" => ok(json!({ "mcp_registries": [{ "url": "https://registry.example.test", "registry_access": "registry_only" }] })),
        other => Promise::rejected(Error::new(format!("Unexpected request: {other}"))),
    }
}

fn set_enrollment(configuration: &TestConfigurationService, value: Dynamic) {
    let _ = configuration.set_user_configuration("github-enterprise.uri", value, None);
}

#[test]
fn signed_out_url_availability_depends_on_the_authentication_provider_not_enrollment_configuration() {
    account_test(|disposables| async move {
        let mut outcomes = Vec::new();
        for enterprise in [false, true] {
            let configuration = if enterprise { enterprise_configuration() } else { TestConfigurationService::new(None) };
            set_enrollment(&configuration, Dynamic::string("https://enrollment.ghe.com"));
            let provider = create_provider(&disposables, TestRequestService::new(|_| ok(json!({}))), ProviderSetup { configuration: Some(configuration), ..ProviderSetup::default() }).await;
            outcomes.push((provider.resolve_git_hub_url(""), provider.resolve_git_hub_url("settings/copilot")));
        }

        assert_eq!(
            outcomes,
            vec![(Some("https://github.com/".to_owned()), Some("https://github.com/settings/copilot".to_owned())), (None, None)]
        );
    });
}

#[test]
fn enterprise_urls_use_the_selected_issuer_and_retain_their_source_specific_mapping_regardless_of_settings() {
    account_test(|disposables| async move {
        let cases: [(&str, Option<&str>, &str, &str); 4] = [
            ("https://b.ghe.com/login/oauth", None, "https://b.ghe.com/", "https://api.b.ghe.com"),
            ("https://b.ghe.com/login/oauth", Some("https://other.ghe.com"), "https://b.ghe.com/", "https://api.b.ghe.com"),
            ("http://ghe.local:8080/login/oauth", Some("https://other.ghe.com"), "http://ghe.local:8080/", "http://api.ghe.local:8080"),
            ("https://B.GHE.COM:443/Team/login/oauth", None, "https://b.ghe.com/", "https://api.b.ghe.com"),
        ];
        let paths = ["/copilot_internal/user", "/copilot_internal/v2/token", "/copilot_internal/managed_settings", "/copilot/mcp_registry"];
        for (issuer, enrollment, base, api) in cases {
            let selected = AuthenticationSession { authorization_server: Some(URI::parse(issuer, false)), ..enterprise_session("b") };
            let configuration = enterprise_configuration();
            set_enrollment(&configuration, enrollment.map_or(Dynamic::Undefined, Dynamic::string));
            let request_service = TestRequestService::new(enterprise_response);
            let provider = create_provider(
                &disposables,
                request_service.clone(),
                ProviderSetup { get_sessions: Some(fixed_sessions(vec![selected])), configuration: Some(configuration), ..ProviderSetup::default() },
            )
            .await;

            assert_eq!(provider.resolve_git_hub_url("settings/copilot"), Some(format!("{base}settings/copilot")));
            let requests: Vec<(Option<String>, Option<String>)> =
                request_service.requests().iter().map(|request| (request.url.as_deref().map(|url| url.split('?').next().unwrap().to_owned()), authorization(request))).collect();
            let expected: Vec<(Option<String>, Option<String>)> = paths.iter().map(|path| (Some(format!("{api}{path}")), Some("Bearer b-token".to_owned()))).collect();
            assert_eq!(requests, expected);
        }
    });
}

#[test]
fn enterprise_scope_retries_never_send_another_host_token_to_the_selected_endpoint() {
    account_test(|disposables| async move {
        let selected = enterprise_session("b");
        let other_host = enterprise_session("a");
        let broader = AuthenticationSession { id: "b-broad".to_owned(), access_token: "b-broad-token".to_owned(), scopes: vec!["user:email".to_owned(), "repo".to_owned()], ..selected.clone() };
        let request_service = TestRequestService::new(|request| {
            if authorization(request).as_deref() == Some("Bearer b-token") { status_response(json!({}), 401) } else { enterprise_response(request) }
        });
        create_provider(
            &disposables,
            request_service.clone(),
            ProviderSetup { get_sessions: Some(fixed_sessions(vec![selected, other_host, broader])), configuration: Some(enterprise_configuration()), ..ProviderSetup::default() },
        )
        .await;

        let observed: Vec<(String, Option<String>)> = request_service.requests().iter().map(|request| (url_hostname(request.url.as_deref().unwrap()), authorization(request))).collect();
        let expected: Vec<(String, Option<String>)> = ["b-token", "b-broad-token", "b-token", "b-token", "b-broad-token", "b-broad-token", "b-token", "b-broad-token"]
            .iter()
            .map(|token| ("api.b.ghe.com".to_owned(), Some(format!("Bearer {token}"))))
            .collect();
        assert_eq!(observed, expected);
    });
}

#[test]
fn enterprise_endpoint_and_token_pairing_survives_an_authentication_provider_switch_during_a_request() {
    account_test(|disposables| async move {
        let configuration = enterprise_configuration();
        let switching = configuration.clone();
        let request_service = TestRequestService::new(move |request| {
            if request.call_site == "defaultAccount.entitlements" {
                set_enrollment_key(&switching);
            }
            enterprise_response(request)
        });
        let provider = create_provider(
            &disposables,
            request_service.clone(),
            ProviderSetup { get_sessions: Some(fixed_sessions(vec![enterprise_session("b")])), configuration: Some(configuration), ..ProviderSetup::default() },
        )
        .await;

        assert_eq!(provider.default_account().map(|account| account.authentication_provider.id), Some("github-enterprise".to_owned()));
        let observed: Vec<(String, Option<String>)> = request_service.requests().iter().map(|request| (url_hostname(request.url.as_deref().unwrap()), authorization(request))).collect();
        assert_eq!(observed, vec![("api.b.ghe.com".to_owned(), Some("Bearer b-token".to_owned())); 4]);
    });
}

/// `configuration.setUserConfiguration('github.copilot.advanced.authProvider', undefined)`.
fn set_enrollment_key(configuration: &TestConfigurationService) {
    let _ = configuration.set_user_configuration(AUTH_PROVIDER_SETTING, Dynamic::Undefined, None);
}

#[test]
fn missing_or_invalid_enterprise_provenance_never_falls_back_to_enrollment_configuration() {
    account_test(|disposables| async move {
        let mut outcomes = Vec::new();
        for authorization_server in [None, Some(URI::parse("https://github.com/login/oauth", false)), Some(URI::parse("https://a.ghe.com/not-an-oauth-server", false))] {
            let configuration = enterprise_configuration();
            set_enrollment(&configuration, Dynamic::string("https://enrollment.ghe.com"));
            let request_service = TestRequestService::new(|_| ok(json!({ "chat_enabled": false })));
            let session = AuthenticationSession { authorization_server, ..sessions().remove(0) };
            let provider = create_provider(
                &disposables,
                request_service.clone(),
                ProviderSetup { get_sessions: Some(fixed_sessions(vec![session])), configuration: Some(configuration), ..ProviderSetup::default() },
            )
            .await;
            outcomes.push((provider.default_account(), provider.resolve_git_hub_url(""), request_service.request_count()));
        }
        assert_eq!(outcomes, vec![(None, None, 0); 3]);
    });
}

#[test]
fn a_changed_issuer_refreshes_the_selected_native_account_cache_without_configuration_changes() {
    account_test(|disposables| async move {
        let current_session = Rc::new(RefCell::new(enterprise_session("a")));
        let session_changes = Emitter::<AuthenticationSessionsChange>::new();
        disposables.add(session_changes.clone());
        let request_service = TestRequestService::new(enterprise_response);
        let current = current_session.clone();
        let provider = create_provider(
            &disposables,
            request_service.clone(),
            ProviderSetup {
                get_sessions: Some(Rc::new(move || Promise::resolved(vec![current.borrow().clone()]))),
                on_did_change_sessions: Some(session_changes.event()),
                configuration: Some(enterprise_configuration()),
                ..ProviderSetup::default()
            },
        )
        .await;
        let before_change = request_service.request_count();
        let changed = provider.on_did_change_default_account().to_promise(None);

        let updated = AuthenticationSession { authorization_server: Some(URI::parse("https://b.ghe.com/login/oauth", false)), ..current_session.borrow().clone() };
        *current_session.borrow_mut() = updated.clone();
        session_changes.fire(session_change("github-enterprise", "GitHub Enterprise", Vec::new(), Vec::new(), vec![updated]));
        changed.await.unwrap();

        assert_eq!(provider.resolve_git_hub_url(""), Some("https://b.ghe.com/".to_owned()));
        assert_eq!(provider.default_account().map(|account| account.account_name), Some("octocat".to_owned()));
        assert_eq!(account_session_id(&provider), Some("a-session".to_owned()));
        assert_eq!(policy_model(&provider), Some(PolicyValue::String("api.b.ghe.com".to_owned())));
        let hosts: Vec<String> = request_service.requests().iter().skip(before_change).map(|request| url_hostname(request.url.as_deref().unwrap())).collect();
        assert_eq!(hosts, vec!["api.b.ghe.com".to_owned(); 4]);
    });
}

// ---- suíte DefaultAccountProvider sign in scopes ----

async fn sign_in(disposables: &Rc<DisposableStore>, options: Option<ISignInOptions>, configuration: Vec<(&str, Dynamic)>) -> Vec<CreateSessionCall> {
    let created = Rc::new(RefCell::new(Vec::new()));
    let configuration_service = TestConfigurationService::new(Some(Dynamic::object(configuration)));
    disposables.add(configuration_service.on_did_change_configuration_emitter.clone());
    let storage = disposables.add_rc(InMemoryStorageService::new());
    let services = DefaultAccountProviderServices {
        configuration_service,
        authentication_service: Rc::new(TestAuthenticationService { get_sessions: fixed_sessions(Vec::new()), on_did_change_sessions: Event::none(), created: created.clone() }),
        authentication_extensions_service: Rc::new(TestAuthenticationExtensionsService),
        telemetry_service: null_telemetry_service(),
        extension_service: Rc::new(StubExtensionService),
        request_service: TestRequestService::new(|_| ok(json!({}))),
        log_service: NullLogService::shared(),
        environment_service: TestEnvironmentService::new(),
        product_service: Rc::new(ProductService::default()),
        context_key_service: MockContextKeyService::new(),
        storage_service: Rc::new(storage),
        host_service: Rc::new(StubHostService),
        command_service: Rc::new(StubCommandService),
        native_managed_settings_service: Rc::new(NullNativeManagedSettingsService),
        file_managed_settings_service: Rc::new(NullFileManagedSettingsService),
    };
    let provider = disposables.add_rc(DefaultAccountProvider::new(account_config(vec![["read:user", "user:email", "repo"].map(str::to_owned).to_vec()], ""), services));
    provider.sign_in(options).await.unwrap();
    let calls = created.borrow().clone();
    calls
}

fn default_scopes_call() -> CreateSessionCall {
    CreateSessionCall { scopes: ["read:user", "user:email", "repo"].map(str::to_owned).to_vec(), options: "{}".to_owned() }
}

fn google_options(additional_scopes: &[&str]) -> Option<ISignInOptions> {
    Some(ISignInOptions {
        additional_scopes: Some(additional_scopes.iter().map(|scope| (*scope).to_owned()).collect()),
        session_options: Some(Dynamic::object([("provider", Dynamic::string("google"))])),
    })
}

#[test]
fn widens_the_default_scopes_and_never_forwards_the_scope_option_to_the_session() {
    account_test(|disposables| async move {
        let none = sign_in(&disposables, None, Vec::new()).await;
        let additive = sign_in(&disposables, google_options(&["workflow", "repo"]), Vec::new()).await;

        assert_eq!(none, vec![default_scopes_call()]);
        // The broad defaults plus the extra scopes, deduplicated.
        assert_eq!(
            additive,
            vec![CreateSessionCall { scopes: ["read:user", "user:email", "repo", "workflow"].map(str::to_owned).to_vec(), options: r#"{"provider":"google"}"#.to_owned() }]
        );
    });
}

#[test]
fn connector_experiments_never_change_default_sign_in_scopes() {
    account_test(|disposables| async move {
        let unset = sign_in(&disposables, None, Vec::new()).await;
        let disabled = sign_in(&disposables, None, vec![(COPILOT_CONNECTORS_ENABLED, Dynamic::Bool(false))]).await;
        let public_feed_only = sign_in(&disposables, None, vec![(AGENT_FINDER_PUBLIC_FEED_ENABLED, Dynamic::Bool(true))]).await;
        let enabled = sign_in(&disposables, None, vec![(COPILOT_CONNECTORS_ENABLED, Dynamic::Bool(true))]).await;
        let enterprise = sign_in(&disposables, None, vec![(COPILOT_CONNECTORS_ENABLED, Dynamic::Bool(true)), (AUTH_PROVIDER_SETTING, Dynamic::string("github-enterprise"))]).await;

        for outcome in [unset, disabled, public_feed_only, enabled, enterprise] {
            assert_eq!(outcome, vec![default_scopes_call()]);
        }
    });
}

#[test]
fn preserves_only_explicitly_requested_additional_scopes_when_connectors_are_enabled() {
    account_test(|disposables| async move {
        let calls = sign_in(&disposables, google_options(&["workflow", "workflow"]), vec![(COPILOT_CONNECTORS_ENABLED, Dynamic::Bool(true))]).await;

        assert_eq!(
            calls,
            vec![CreateSessionCall { scopes: ["read:user", "user:email", "repo", "workflow"].map(str::to_owned).to_vec(), options: r#"{"provider":"google"}"#.to_owned() }]
        );
    });
}
