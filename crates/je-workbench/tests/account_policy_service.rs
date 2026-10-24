//! Porte de `src/vs/workbench/services/policies/test/browser/accountPolicyService.test.ts` (suíte
//! `AccountPolicyService`), caso a caso e na mesma ordem.
//!
//! Divergências:
//! - Toda a suíte roda sob o relógio virtual (`async_test`). O `suiteSetup`/`suiteTeardown` (registrar e
//!   desregistrar o nó de configuração) e o `setup` são feitos por teste, no [`setup`] (o registro global é um
//!   singleton por thread e cada teste roda no seu).
//! - `new DefaultAccountService(TestProductService)` é `DefaultAccountService::new(provedor padrão)`: o serviço só
//!   lê `defaultChatAgent.provider.default`. O `DefaultAccountService` com `getDefaultAccount` que lança
//!   (suíte de falhas de inicialização) é o `FailingDefaultAccountService`, um decorador que repete o contrato.
//! - Os callbacks `value` das políticas são [`policy_value_dynamic`] (a função dinâmica do `PolicyDefinition`).
//! - A política `ChatTemporaryDefaultToLocalHarness`, que o original recebe do módulo de chat já carregado, é
//!   registrada pelo próprio teste (o chat está fora do escopo do porte), com a mesma forma: `boolean`,
//!   `managedSettings` com a chave e `value: managedSettingValue(chave)`.
//! - `FakeNativeManagedSettingsService` e `FakeFileManagedSettingsService` implementam os traits da plataforma; o
//!   `holdUpdate`/`failUpdate` das subclasses anônimas são campos dele.
//! - O caso `managed sandbox policy refresh selects Agent Host Copilot...` depende de `AgentHostEnablementService`,
//!   de `getComputedDefaultSessionType` e do `chatSessionsService`, todos do chat (fora do escopo e sem porte): fica
//!   `#[ignore]` com o motivo, como os demais casos pendentes do porte.
//! - `assert.deepStrictEqual` sobre registros com `undefined` compara `Option`s; `timeout(0)` é `timeout(0).await`.

#[path = "../../je-platform/tests/common/mod.rs"]
mod common;

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use common::files::async_test;
use common::{RegistryGuard, assert_deep_strict_equal, j, node_full, s};
use indexmap::IndexMap;
use je_base::async_::timeout;
use je_base::default_account::{ICopilotTokenInfo, IDefaultAccount, IDefaultAccountAuthenticationProvider, IEntitlementsData, IPolicyData};
use je_base::dynamic::Dynamic;
use je_base::errors::Error;
use je_base::event::{Emitter, Event};
use je_base::lifecycle::{DisposableStore, IDisposable};
use je_base::policy::{ManagedSettingsData, PolicyValue as BaseValue, PolicyValueFn};
use je_base::product::IChatProvider;
use je_base::promise::{DeferredPromise, Promise};
use je_platform::configuration::defaults::DefaultConfiguration;
use je_platform::configuration::policy_configuration::PolicyConfiguration;
use je_platform::configuration::registry::{ConfigurationNode, PropertySchema, configuration_registry};
use je_platform::configuration::test_configuration_service::TestConfigurationService;
use je_platform::log::{LogArg, LogMessage, LogServiceRef, NullLogService};
use je_platform::policy::copilot_managed_settings::{
    COPILOT_AUTO_TIER_KEY, COPILOT_DISABLE_BYPASS_PERMISSIONS_MODE_KEY, COPILOT_ENABLED_PLUGINS_KEY, COPILOT_PERMISSION_RULES_KEYS, COPILOT_SANDBOX_ENABLED_KEY,
    COPILOT_TEMPORARY_ALLOW_LOCAL_HARNESS_KEY, COPILOT_TEMPORARY_DEFAULT_TO_LOCAL_HARNESS_KEY, IFileManagedSettingsService, IManagedSettingsService, INativeManagedSettingsService,
    RawManagedSettingsData, TEMPORARY_DEFAULT_TO_LOCAL_HARNESS_SETTING_ID, has_managed_permission_denials, managed_setting_value, managed_settings_disabled_value, normalize_managed_settings,
    requires_copilot_agent_host, third_party_agent_enabled_value,
};
use je_platform::policy::managed_settings_freshness::{IManagedSettingsFreshness, IManagedSettingsFreshnessEffective, ManagedSettingsFreshnessFailure};
use je_platform::policy::{AbstractPolicyService, PolicyDefinition, PolicyDefinitionType, PolicyDefinitions, PolicyService, PolicyUpdater, PolicyValue, PolicyValueSource, PolicyValues, SerializedPolicy};
use je_workbench::services::accounts::default_account::{
    DefaultAccountService, IDefaultAccountProvider, IDefaultAccountRefreshOptions, IDefaultAccountService, ISignInOptions, ManagedSettingsFetchStatus,
};
use je_workbench::services::accounts::managed_settings::{IManagedSettingsCompatibilityError, adapt_managed_settings};
use je_workbench::services::policies::account_policy_service::{
    APPROVED_ACCOUNT_ORGANIZATIONS_POLICY_NAME, AccountPolicyGateState, AccountPolicyGateUnsatisfiedReason, AccountPolicyService, IAccountPolicyGateInfo, IAccountPolicyGateService,
    when_account_policy_settled,
};
use je_workbench::services::policies::policy_value_callback::policy_value_dynamic;

// ---- dados e fakes ----

fn auth_provider(id: &str, name: &str) -> IDefaultAccountAuthenticationProvider {
    IDefaultAccountAuthenticationProvider { id: id.to_owned(), name: name.to_owned(), enterprise: false, extra: Default::default() }
}

fn base_default_account() -> IDefaultAccount {
    IDefaultAccount { authentication_provider: auth_provider("github", "GitHub"), account_name: "testuser".to_owned(), session_id: "abc123".to_owned(), enterprise: false, entitlements_data: None, extra: Default::default() }
}

fn account_in_orgs(orgs: &[&str]) -> IDefaultAccount {
    let mut account = base_default_account();
    account.entitlements_data = Some(IEntitlementsData {
        access_type_sku: "sku".to_owned(),
        chat_enabled: true,
        assigned_date: String::new(),
        can_signup_for_limited: false,
        copilot_plan: "pro".to_owned(),
        organization_login_list: orgs.iter().map(|org| (*org).to_owned()).collect(),
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
    });
    account
}

fn policy_data(chat_preview: Option<bool>, managed: Option<Vec<(&str, BaseValue)>>) -> IPolicyData {
    let mut data = je_workbench::services::policies::policy_value_callback::empty_policy_data();
    data.chat_preview_features_enabled = chat_preview;
    data.managed_settings = managed.map(|entries| entries.into_iter().map(|(key, value)| (key.to_owned(), value)).collect());
    data
}

fn managed(entries: Vec<(&str, BaseValue)>) -> ManagedSettingsData {
    entries.into_iter().map(|(key, value)| (key.to_owned(), value)).collect()
}

fn text(value: &str) -> BaseValue {
    BaseValue::String(value.to_owned())
}

/// O `DefaultAccountProvider` do teste: os eventos que o original deixa em `Event.None` nunca disparam.
struct TestProvider {
    default_account: RefCell<Option<IDefaultAccount>>,
    policy_data: RefCell<Option<IPolicyData>>,
    freshness: RefCell<IManagedSettingsFreshness>,
    configured_provider: Option<IDefaultAccountAuthenticationProvider>,
    refresh_override: RefCell<Option<Promise<Option<IDefaultAccount>>>>,
    on_did_change_default_account: Rc<Emitter<Option<IDefaultAccount>>>,
    on_did_change_policy_data: Rc<Emitter<Option<IPolicyData>>>,
    on_did_change_copilot_token_info: Rc<Emitter<Option<ICopilotTokenInfo>>>,
    on_did_change_compatibility_error: Rc<Emitter<Option<IManagedSettingsCompatibilityError>>>,
    on_did_change_freshness: Rc<Emitter<IManagedSettingsFreshness>>,
}

impl TestProvider {
    fn new(account: IDefaultAccount, policy_data: Option<IPolicyData>) -> Rc<Self> {
        Self::with_freshness(account, policy_data, IManagedSettingsFreshness::NotRequired)
    }

    fn with_freshness(account: IDefaultAccount, policy_data: Option<IPolicyData>, freshness: IManagedSettingsFreshness) -> Rc<Self> {
        Rc::new(Self {
            default_account: RefCell::new(Some(account)),
            policy_data: RefCell::new(policy_data),
            freshness: RefCell::new(freshness),
            configured_provider: None,
            refresh_override: RefCell::new(None),
            on_did_change_default_account: Emitter::new(),
            on_did_change_policy_data: Emitter::new(),
            on_did_change_copilot_token_info: Emitter::new(),
            on_did_change_compatibility_error: Emitter::new(),
            on_did_change_freshness: Emitter::new(),
        })
    }

    /// Um provedor cujo provedor de autenticação configurado é `configured`, e não o da conta.
    fn mismatched(account: IDefaultAccount, configured: IDefaultAccountAuthenticationProvider) -> Rc<Self> {
        let provider = Self::new(account, Some(policy_data(None, None)));
        Rc::new(Self {
            default_account: RefCell::new(provider.default_account.borrow().clone()),
            policy_data: RefCell::new(provider.policy_data.borrow().clone()),
            freshness: RefCell::new(IManagedSettingsFreshness::NotRequired),
            configured_provider: Some(configured),
            refresh_override: RefCell::new(None),
            on_did_change_default_account: Emitter::new(),
            on_did_change_policy_data: Emitter::new(),
            on_did_change_copilot_token_info: Emitter::new(),
            on_did_change_compatibility_error: Emitter::new(),
            on_did_change_freshness: Emitter::new(),
        })
    }

    fn set_policy_data(&self, data: Option<IPolicyData>) {
        *self.policy_data.borrow_mut() = data.clone();
        self.on_did_change_policy_data.fire(data);
    }

    fn set_freshness(&self, freshness: IManagedSettingsFreshness) {
        *self.freshness.borrow_mut() = freshness.clone();
        self.on_did_change_freshness.fire(freshness);
    }
}

impl IDefaultAccountProvider for TestProvider {
    fn default_account(&self) -> Option<IDefaultAccount> {
        self.default_account.borrow().clone()
    }
    fn on_did_change_default_account(&self) -> Event<Option<IDefaultAccount>> {
        self.on_did_change_default_account.event()
    }
    fn policy_data(&self) -> Option<IPolicyData> {
        self.policy_data.borrow().clone()
    }
    fn on_did_change_policy_data(&self) -> Event<Option<IPolicyData>> {
        self.on_did_change_policy_data.event()
    }
    fn copilot_token_info(&self) -> Option<ICopilotTokenInfo> {
        None
    }
    fn on_did_change_copilot_token_info(&self) -> Event<Option<ICopilotTokenInfo>> {
        self.on_did_change_copilot_token_info.event()
    }
    fn managed_settings_fetch_status(&self) -> Option<ManagedSettingsFetchStatus> {
        None
    }
    fn managed_settings_fetched_at(&self) -> Option<f64> {
        None
    }
    fn managed_settings_raw_response(&self) -> Dynamic {
        Dynamic::Null
    }
    fn managed_settings_compatibility_error(&self) -> Option<IManagedSettingsCompatibilityError> {
        None
    }
    fn on_did_change_managed_settings_compatibility_error(&self) -> Event<Option<IManagedSettingsCompatibilityError>> {
        self.on_did_change_compatibility_error.event()
    }
    fn managed_settings_freshness(&self) -> IManagedSettingsFreshness {
        self.freshness.borrow().clone()
    }
    fn on_did_change_managed_settings_freshness(&self) -> Event<IManagedSettingsFreshness> {
        self.on_did_change_freshness.event()
    }
    fn get_default_account_authentication_provider(&self) -> IDefaultAccountAuthenticationProvider {
        self.configured_provider.clone().unwrap_or_else(|| self.default_account.borrow().as_ref().expect("conta").authentication_provider.clone())
    }
    fn resolve_git_hub_url(&self, path: &str) -> Option<String> {
        Some(format!("https://github.com/{path}"))
    }
    fn refresh(&self, _options: Option<IDefaultAccountRefreshOptions>) -> Promise<Option<IDefaultAccount>> {
        match self.refresh_override.borrow().clone() {
            Some(promise) => promise,
            None => Promise::resolved(self.default_account.borrow().clone()),
        }
    }
    fn sign_in(&self, _options: Option<ISignInOptions>) -> Promise<Option<IDefaultAccount>> {
        Promise::resolved(None)
    }
    fn sign_out(&self) -> Promise<()> {
        Promise::resolved(())
    }
}

impl IDisposable for TestProvider {
    fn dispose(&self) {
        self.on_did_change_default_account.dispose();
        self.on_did_change_policy_data.dispose();
        self.on_did_change_copilot_token_info.dispose();
        self.on_did_change_compatibility_error.dispose();
        self.on_did_change_freshness.dispose();
    }
}

fn new_default_account_service(d: &Rc<DisposableStore>) -> Rc<DefaultAccountService> {
    d.add_rc(DefaultAccountService::new(IChatProvider { id: "github".to_owned(), name: "GitHub".to_owned(), extra: Default::default() }))
}

fn install(d: &Rc<DisposableStore>, service: &DefaultAccountService, provider: Rc<TestProvider>) -> Rc<TestProvider> {
    let provider = d.add_rc(provider);
    service.set_default_account_provider(provider.clone()).unwrap();
    provider
}

/// O `DefaultAccountService` que lança em `getDefaultAccount` (a subclasse anônima da suíte de falhas): repete o
/// contrato do serviço e troca só esse método.
struct FailingDefaultAccountService {
    inner: Rc<DefaultAccountService>,
    error: Error,
}

impl IDefaultAccountService for FailingDefaultAccountService {
    fn on_did_change_default_account(&self) -> Event<Option<IDefaultAccount>> {
        self.inner.on_did_change_default_account()
    }
    fn on_did_change_policy_data(&self) -> Event<Option<IPolicyData>> {
        self.inner.on_did_change_policy_data()
    }
    fn policy_data(&self) -> Option<IPolicyData> {
        self.inner.policy_data()
    }
    fn current_default_account(&self) -> Option<IDefaultAccount> {
        self.inner.current_default_account()
    }
    fn copilot_token_info(&self) -> Option<ICopilotTokenInfo> {
        self.inner.copilot_token_info()
    }
    fn on_did_change_copilot_token_info(&self) -> Event<Option<ICopilotTokenInfo>> {
        self.inner.on_did_change_copilot_token_info()
    }
    fn managed_settings_fetch_status(&self) -> Option<ManagedSettingsFetchStatus> {
        self.inner.managed_settings_fetch_status()
    }
    fn managed_settings_fetched_at(&self) -> Option<f64> {
        self.inner.managed_settings_fetched_at()
    }
    fn managed_settings_raw_response(&self) -> Dynamic {
        self.inner.managed_settings_raw_response()
    }
    fn managed_settings_compatibility_error(&self) -> Option<IManagedSettingsCompatibilityError> {
        self.inner.managed_settings_compatibility_error()
    }
    fn on_did_change_managed_settings_compatibility_error(&self) -> Event<Option<IManagedSettingsCompatibilityError>> {
        self.inner.on_did_change_managed_settings_compatibility_error()
    }
    fn managed_settings_freshness(&self) -> IManagedSettingsFreshness {
        self.inner.managed_settings_freshness()
    }
    fn on_did_change_managed_settings_freshness(&self) -> Event<IManagedSettingsFreshness> {
        self.inner.on_did_change_managed_settings_freshness()
    }
    fn get_default_account(&self) -> Promise<Option<IDefaultAccount>> {
        let error = self.error.clone();
        self.inner.get_default_account().then_result(move |result| {
            result?;
            Err(error)
        })
    }
    fn get_default_account_authentication_provider(&self) -> IDefaultAccountAuthenticationProvider {
        IDefaultAccountService::get_default_account_authentication_provider(&*self.inner)
    }
    fn set_default_account_provider(&self, provider: Rc<dyn IDefaultAccountProvider>) -> Result<(), Error> {
        self.inner.set_default_account_provider(provider)
    }
    fn refresh(&self, options: Option<IDefaultAccountRefreshOptions>) -> Promise<Option<IDefaultAccount>> {
        self.inner.refresh(options)
    }
    fn sign_in(&self, options: Option<ISignInOptions>) -> Promise<Option<IDefaultAccount>> {
        self.inner.sign_in(options)
    }
    fn sign_out(&self) -> Promise<()> {
        self.inner.sign_out()
    }
    fn resolve_git_hub_url(&self, path: &str) -> Option<String> {
        self.inner.resolve_git_hub_url(path)
    }
}

/// O `NullLogService` que guarda cada `error(message, ...args)` (a subclasse anônima da suíte de falhas).
struct RecordingLogService {
    inner: LogServiceRef,
    logged: Rc<RefCell<Vec<(String, Vec<LogArg>)>>>,
}

je_platform::delegate_logger!(RecordingLogService, inner, error_args: |this, message, args| {
    let text = match message {
        LogMessage::Text(text) => text.clone(),
        LogMessage::Error(error) => format!("{error:?}"),
    };
    this.logged.borrow_mut().push((text, args.to_vec()));
});


/// `FakeManagedPolicyService`: o `getPolicyValue` lê um mapa próprio; `gated` faz o `AsyncManagedPolicyService`
/// (nada visível até o `seed`).
struct FakeManagedPolicyService {
    base: Rc<AbstractPolicyService>,
    fake_policies: RefCell<IndexMap<String, PolicyValue>>,
    gated: Cell<bool>,
    seeded: Cell<bool>,
}

impl FakeManagedPolicyService {
    fn new() -> Rc<Self> {
        let updater: PolicyUpdater = Rc::new(|_| Promise::resolved(()));
        Rc::new(Self { base: AbstractPolicyService::new(updater), fake_policies: RefCell::new(IndexMap::new()), gated: Cell::new(false), seeded: Cell::new(true) })
    }

    fn gated() -> Rc<Self> {
        let service = Self::new();
        service.gated.set(true);
        service.seeded.set(false);
        service
    }

    fn set_policy(&self, name: &str, value: Option<PolicyValue>) {
        match value {
            None => {
                if self.fake_policies.borrow_mut().shift_remove(name).is_some() {
                    self.base.fire_did_change(vec![name.to_owned()]);
                }
            }
            Some(value) => {
                self.fake_policies.borrow_mut().insert(name.to_owned(), value);
                self.base.fire_did_change(vec![name.to_owned()]);
            }
        }
    }

    /// `AsyncManagedPolicyService.seed()`.
    async fn seed(&self, name: &str, value: PolicyValue) {
        timeout(0).await.unwrap();
        self.seeded.set(true);
        self.set_policy(name, Some(value));
    }
}

impl PolicyService for FakeManagedPolicyService {
    fn on_did_change(&self) -> Event<Vec<String>> {
        PolicyService::on_did_change(&self.base)
    }
    fn update_policy_definitions(&self, _policy_definitions: PolicyDefinitions) -> Promise<PolicyValues> {
        Promise::resolved(Vec::new())
    }
    fn get_policy_value(&self, name: &str) -> Option<PolicyValue> {
        if self.gated.get() && !self.seeded.get() {
            return None;
        }
        self.fake_policies.borrow().get(name).cloned()
    }
    fn get_policy_value_source(&self, name: &str) -> Option<PolicyValueSource> {
        PolicyService::get_policy_value_source(&self.base, name)
    }
    fn serialize(&self) -> Option<Vec<(String, SerializedPolicy)>> {
        None
    }
    fn policy_definitions(&self) -> PolicyDefinitions {
        Vec::new()
    }
}

impl IDisposable for FakeManagedPolicyService {
    fn dispose(&self) {
        self.base.dispose();
    }
}

/// `FakeNativeManagedSettingsService`, com o `holdUpdate` e o `failUpdate` das subclasses anônimas.
struct FakeNativeManagedSettingsService {
    managed_settings: RefCell<ManagedSettingsData>,
    on_did_change: Rc<Emitter<ManagedSettingsData>>,
    registered_managed_settings: RefCell<IndexMap<String, String>>,
    hold_update: Cell<bool>,
    update_started: DeferredPromise<()>,
    update_completed: DeferredPromise<ManagedSettingsData>,
    fail_update: RefCell<Option<Error>>,
}

impl FakeNativeManagedSettingsService {
    fn new(managed_settings: ManagedSettingsData) -> Rc<Self> {
        Rc::new(Self {
            managed_settings: RefCell::new(managed_settings),
            on_did_change: Emitter::new(),
            registered_managed_settings: RefCell::new(IndexMap::new()),
            hold_update: Cell::new(false),
            update_started: DeferredPromise::new(),
            update_completed: DeferredPromise::new(),
            fail_update: RefCell::new(None),
        })
    }

    fn set_managed_settings(&self, managed_settings: ManagedSettingsData) {
        *self.managed_settings.borrow_mut() = managed_settings.clone();
        self.on_did_change.fire(managed_settings);
    }

    fn registered(&self, key: &str) -> Option<String> {
        self.registered_managed_settings.borrow().get(key).cloned()
    }
}

impl INativeManagedSettingsService for FakeNativeManagedSettingsService {
    fn managed_settings(&self) -> ManagedSettingsData {
        self.managed_settings.borrow().clone()
    }
    fn on_did_change_managed_settings(&self) -> Event<ManagedSettingsData> {
        self.on_did_change.event()
    }
    fn initialize(&self) -> Promise<ManagedSettingsData> {
        Promise::resolved(self.managed_settings())
    }
    fn update_policy_definitions(&self, policy_definitions: PolicyDefinitions) -> Promise<ManagedSettingsData> {
        if self.hold_update.get() {
            let started = self.update_started.complete(());
            let completed = self.update_completed.promise.clone();
            return started.then_promise(move |()| completed);
        }
        if let Some(error) = self.fail_update.borrow().clone() {
            return Promise::rejected(error);
        }
        self.registered_managed_settings.borrow_mut().clear();
        for (_, definition) in &policy_definitions {
            if let Some(declared) = &definition.managed_settings {
                for (key, entry) in declared.own_entries() {
                    let type_name = match entry.get_str("type") {
                        Dynamic::String(name) => name.to_string_lossy(),
                        _ => String::new(),
                    };
                    self.registered_managed_settings.borrow_mut().insert(key.to_string_lossy(), type_name);
                }
            }
        }
        Promise::resolved(self.managed_settings())
    }
}

impl IDisposable for FakeNativeManagedSettingsService {
    fn dispose(&self) {
        self.on_did_change.dispose();
    }
}

/// O `ManagedSettingsData` como o documento bruto (o `rawManagedSettings = managedSettings` do construtor).
fn raw_of(values: &ManagedSettingsData) -> RawManagedSettingsData {
    Dynamic::object(values.iter().map(|(key, value)| {
        let value = match value {
            BaseValue::Boolean(flag) => Dynamic::Bool(*flag),
            BaseValue::Number(number) => Dynamic::Number(*number),
            BaseValue::String(text) => Dynamic::string(text),
        };
        (key.as_str(), value)
    }))
}

struct FakeFileManagedSettingsService {
    managed_settings: RefCell<ManagedSettingsData>,
    raw_managed_settings: RefCell<RawManagedSettingsData>,
    on_did_change_raw: Rc<Emitter<RawManagedSettingsData>>,
}

impl FakeFileManagedSettingsService {
    fn new(managed_settings: ManagedSettingsData) -> Rc<Self> {
        let raw = raw_of(&managed_settings);
        Self::with_raw(managed_settings, raw)
    }

    fn with_raw(managed_settings: ManagedSettingsData, raw: RawManagedSettingsData) -> Rc<Self> {
        Rc::new(Self { managed_settings: RefCell::new(managed_settings), raw_managed_settings: RefCell::new(raw), on_did_change_raw: Emitter::new() })
    }

    fn set_managed_settings(&self, managed_settings: ManagedSettingsData, raw: Option<RawManagedSettingsData>) {
        let raw = raw.unwrap_or_else(|| raw_of(&managed_settings));
        *self.managed_settings.borrow_mut() = managed_settings;
        *self.raw_managed_settings.borrow_mut() = raw.clone();
        self.on_did_change_raw.fire(raw);
    }
}

impl IFileManagedSettingsService for FakeFileManagedSettingsService {
    fn raw_managed_settings(&self) -> RawManagedSettingsData {
        self.raw_managed_settings.borrow().clone()
    }
    fn managed_settings(&self) -> ManagedSettingsData {
        self.managed_settings.borrow().clone()
    }
    fn on_did_change_raw_managed_settings(&self) -> Event<RawManagedSettingsData> {
        self.on_did_change_raw.event()
    }
    fn on_did_change_managed_settings(&self) -> Event<ManagedSettingsData> {
        Event::none()
    }
    fn initialize(&self) -> Promise<ManagedSettingsData> {
        Promise::resolved(self.managed_settings())
    }
}

impl IDisposable for FakeFileManagedSettingsService {
    fn dispose(&self) {
        self.on_did_change_raw.dispose();
    }
}

// ---- definições de política ----

fn managed_declaration(entries: &[(&str, &str)]) -> Dynamic {
    Dynamic::object(entries.iter().map(|(key, type_name)| (*key, Dynamic::object([("type", s(type_name))]))))
}

fn definition(type_: PolicyDefinitionType, value: Option<PolicyValueFn>, managed_settings: &[(&str, &str)], restricted: Option<PolicyValue>) -> Rc<PolicyDefinition> {
    Rc::new(PolicyDefinition {
        type_,
        value: value.map(policy_value_dynamic),
        managed_settings: if managed_settings.is_empty() { None } else { Some(managed_declaration(managed_settings)) },
        restricted_value: restricted,
    })
}

fn third_party_agent_policies() -> PolicyDefinitions {
    ["Claude", "Codex"]
        .into_iter()
        .map(|name| {
            (
                name.to_owned(),
                definition(PolicyDefinitionType::Boolean, Some(PolicyValueFn::new(third_party_agent_enabled_value)), &[(COPILOT_TEMPORARY_ALLOW_LOCAL_HARNESS_KEY, "boolean")], None),
            )
        })
        .collect()
}

fn presence_policy() -> (String, Rc<PolicyDefinition>) {
    ("Presence".to_owned(), definition(PolicyDefinitionType::Boolean, Some(PolicyValueFn::new(managed_settings_disabled_value)), &[], None))
}

fn managed_value(data: &IPolicyData, key: &str) -> Option<BaseValue> {
    data.managed_settings.as_ref().and_then(|settings| settings.get(key)).cloned()
}

/// Uma propriedade de configuração com política (`policy: { name, category, minimumVersion, localization, ... }`).
fn policy_property(type_: &str, default: Dynamic, name: &str, value: Option<PolicyValueFn>, managed_settings: &[(&str, &str)], restricted: Option<Dynamic>) -> PropertySchema {
    let mut entries: Vec<(&str, Dynamic)> = vec![
        ("name", s(name)),
        ("category", s("Extensions")),
        ("minimumVersion", s("1.0.0")),
        ("localization", j("{ 'description': { 'key': '', 'value': '' } }")),
    ];
    if let Some(value) = value {
        entries.push(("value", policy_value_dynamic(value)));
    }
    if !managed_settings.is_empty() {
        entries.push(("managedSettings", managed_declaration(managed_settings)));
    }
    if let Some(restricted) = restricted {
        entries.push(("restrictedValue", restricted));
    }
    PropertySchema::from_entries(vec![("type", s(type_)), ("default", default), ("policy", Dynamic::object(entries))])
}

fn policy_configuration_node() -> ConfigurationNode {
    let preview_disabled = |payload: Dynamic| PolicyValueFn::new(move |data| payload_value(data.chat_preview_features_enabled == Some(false), &payload));
    let bypass = COPILOT_DISABLE_BYPASS_PERMISSIONS_MODE_KEY;
    let disables_bypass = |data: &IPolicyData| managed_value(data, bypass) == Some(text("disable"));
    node_full(
        "policyConfiguration",
        1.0,
        "a",
        vec![
            ("setting.A", policy_property("string", s("defaultValueA"), "PolicySettingA", None, &[], None)),
            ("setting.B", policy_property("string", s("defaultValueB"), "PolicySettingB", Some(preview_disabled(s("policyValueB"))), &[], None)),
            (
                "setting.C",
                policy_property("array", j("['defaultValueC1', 'defaultValueC2']"), "PolicySettingC", Some(preview_disabled(s("[\"policyValueC1\",\"policyValueC2\"]"))), &[], None),
            ),
            ("setting.D", policy_property("boolean", Dynamic::Bool(true), "PolicySettingD", Some(preview_disabled(Dynamic::Bool(false))), &[], None)),
            ("setting.E", PropertySchema::from_entries(vec![("type", s("boolean")), ("default", Dynamic::Bool(true))])),
            (
                "setting.F",
                policy_property(
                    "boolean",
                    Dynamic::Bool(true),
                    "PolicySettingF",
                    Some(PolicyValueFn::new(move |data| disables_bypass(data).then_some(BaseValue::Boolean(false)))),
                    &[(bypass, "string")],
                    None,
                ),
            ),
            (
                "setting.G",
                PropertySchema::from_entries(vec![
                    ("type", s("object")),
                    ("additionalProperties", j("{ 'type': 'boolean' }")),
                    ("default", j("{}")),
                    (
                        "policy",
                        policy_property("object", j("{}"), "PolicySettingG", Some(PolicyValueFn::new(|data| managed_value(data, COPILOT_ENABLED_PLUGINS_KEY))), &[(COPILOT_ENABLED_PLUGINS_KEY, "string")], None).get("policy"),
                    ),
                ]),
            ),
            (
                "setting.H",
                policy_property(
                    "boolean",
                    Dynamic::Bool(true),
                    "PolicySettingH",
                    Some(PolicyValueFn::new(move |data| (managed_value(data, bypass) == Some(text("disable")) || data.chat_preview_features_enabled == Some(false)).then_some(BaseValue::Boolean(false)))),
                    &[(bypass, "string")],
                    None,
                ),
            ),
            (
                "setting.I",
                policy_property(
                    "boolean",
                    Dynamic::Bool(true),
                    "PolicySettingI",
                    Some(PolicyValueFn::new(move |data| {
                        (managed_value(data, bypass) == Some(text("disable")) && managed_value(data, COPILOT_ENABLED_PLUGINS_KEY).is_some()).then_some(BaseValue::Boolean(false))
                    })),
                    &[(bypass, "string"), (COPILOT_ENABLED_PLUGINS_KEY, "string")],
                    None,
                ),
            ),
            ("setting.J", policy_property("boolean", Dynamic::Bool(true), "PolicySettingJ", Some(PolicyValueFn::new(managed_settings_disabled_value)), &[], None)),
            // A política do chat que o original recebe do módulo de chat já carregado.
            (
                TEMPORARY_DEFAULT_TO_LOCAL_HARNESS_SETTING_ID,
                policy_property(
                    "boolean",
                    Dynamic::Bool(false),
                    "ChatTemporaryDefaultToLocalHarness",
                    Some(managed_setting_value(COPILOT_TEMPORARY_DEFAULT_TO_LOCAL_HARNESS_KEY)),
                    &[(COPILOT_TEMPORARY_DEFAULT_TO_LOCAL_HARNESS_KEY, "boolean")],
                    None,
                ),
            ),
        ],
    )
}

/// `cond ? payload : undefined`, na forma do callback tipado.
fn payload_value(condition: bool, payload: &Dynamic) -> Option<BaseValue> {
    if !condition {
        return None;
    }
    match payload {
        Dynamic::Bool(flag) => Some(BaseValue::Boolean(*flag)),
        Dynamic::String(text) => Some(BaseValue::String(text.to_string_lossy())),
        _ => None,
    }
}

// ---- fixture ----

struct Fixture {
    _registry: RegistryGuard,
    default_account_service: Rc<DefaultAccountService>,
    policy_service: Rc<AccountPolicyService>,
    policy_configuration: Rc<PolicyConfiguration>,
}

fn new_policy_configuration(d: &Rc<DisposableStore>, policy_service: &Rc<AccountPolicyService>) -> Rc<PolicyConfiguration> {
    let default_configuration = d.add_rc(DefaultConfiguration::new(NullLogService::shared()));
    default_configuration.initialize();
    d.add_rc(PolicyConfiguration::new(default_configuration, policy_service.clone() as Rc<dyn PolicyService>, NullLogService::shared()))
}

fn new_policy_service(
    d: &Rc<DisposableStore>,
    default_account_service: &Rc<DefaultAccountService>,
    managed: Option<Rc<FakeManagedPolicyService>>,
    native: Option<Rc<FakeNativeManagedSettingsService>>,
    file: Option<Rc<FakeFileManagedSettingsService>>,
) -> Rc<AccountPolicyService> {
    d.add_rc(AccountPolicyService::new(
        NullLogService::shared(),
        default_account_service.clone() as Rc<dyn IDefaultAccountService>,
        managed.map(|service| service as Rc<dyn PolicyService>),
        native.map(|service| service as Rc<dyn INativeManagedSettingsService>),
        file.map(|service| service as Rc<dyn IFileManagedSettingsService>),
    ))
}

/// O `suiteSetup` e o `setup`.
async fn setup(d: &Rc<DisposableStore>) -> Fixture {
    let registry = RegistryGuard::new();
    configuration_registry().register_configuration(policy_configuration_node());
    let default_account_service = new_default_account_service(d);
    let policy_service = new_policy_service(d, &default_account_service, None, None, None);
    let policy_configuration = new_policy_configuration(d, &policy_service);
    Fixture { _registry: registry, default_account_service, policy_service, policy_configuration }
}

/// `assertDefaultBehavior(policyData)`.
async fn assert_default_behavior(d: &Rc<DisposableStore>, policy_data: Option<IPolicyData>) {
    let f = setup(d).await;
    install(d, &f.default_account_service, TestProvider::new(base_default_account(), policy_data));
    f.default_account_service.refresh(None).await.unwrap();

    f.policy_configuration.initialize().await.unwrap();

    for name in ["PolicySettingA", "PolicySettingB", "PolicySettingC", "PolicySettingD"] {
        // No policy is set
        assert_eq!(f.policy_service.get_policy_value(name), None, "{name}");
    }
    let model = f.policy_configuration.configuration_model();
    for key in ["setting.B", "setting.C", "setting.D"] {
        assert_eq!(model.get_value(key), Dynamic::Undefined, "{key}");
    }
}

fn b(value: bool) -> Option<PolicyValue> {
    Some(PolicyValue::Boolean(value))
}

fn flag(value: Option<bool>) -> Dynamic {
    value.map_or(Dynamic::Undefined, Dynamic::Bool)
}

// ---- casos ----

#[test]
fn should_initialize_with_default_account() {
    async_test(|d| async move { assert_default_behavior(&d, None).await });
}

#[test]
fn should_initialize_with_default_account_and_preview_features_enabled() {
    async_test(|d| async move { assert_default_behavior(&d, Some(policy_data(Some(true), None))).await });
}

#[test]
fn should_initialize_with_default_account_and_preview_features_disabled() {
    async_test(|d| async move {
        let f = setup(&d).await;
        install(&d, &f.default_account_service, TestProvider::new(base_default_account(), Some(policy_data(Some(false), None))));
        f.default_account_service.refresh(None).await.unwrap();

        f.policy_configuration.initialize().await.unwrap();
        let actual = f.policy_configuration.configuration_model();

        assert_eq!(f.policy_service.get_policy_value("PolicySettingA"), None); // Not tagged with chat preview tags
        assert_eq!(f.policy_service.get_policy_value("PolicySettingB"), Some(PolicyValue::String("policyValueB".to_owned())));
        assert_eq!(f.policy_service.get_policy_value("PolicySettingC"), Some(PolicyValue::String("[\"policyValueC1\",\"policyValueC2\"]".to_owned())));
        assert_eq!(f.policy_service.get_policy_value("PolicySettingD"), b(false));
        assert_eq!(f.policy_service.get_policy_value_source("PolicySettingD"), Some(PolicyValueSource::Account));

        assert_eq!(actual.get_value("setting.B"), s("policyValueB"));
        assert_deep_strict_equal(&actual.get_value("setting.C"), &j("['policyValueC1', 'policyValueC2']"));
        assert_eq!(actual.get_value("setting.D"), Dynamic::Bool(false));
    });
}

#[test]
fn should_apply_managed_settings_policy_data_from_default_account() {
    async_test(|d| async move {
        let f = setup(&d).await;
        let data = policy_data(None, Some(vec![(COPILOT_DISABLE_BYPASS_PERMISSIONS_MODE_KEY, text("disable"))]));
        install(&d, &f.default_account_service, TestProvider::new(base_default_account(), Some(data)));
        f.default_account_service.refresh(None).await.unwrap();

        f.policy_configuration.initialize().await.unwrap();

        assert_eq!(
            (
                f.policy_service.get_policy_value("PolicySettingF"),
                f.policy_service.get_policy_value_source("PolicySettingF"),
                f.policy_configuration.configuration_model().get_value("setting.F")
            ),
            (b(false), Some(PolicyValueSource::ServerManagedSettings), Dynamic::Bool(false))
        );
    });
}

#[test]
fn should_apply_managed_settings_policy_data_from_native_managed_settings_service() {
    async_test(|d| async move {
        let f = setup(&d).await;
        let native = d.add_rc(FakeNativeManagedSettingsService::new(managed(vec![(COPILOT_DISABLE_BYPASS_PERMISSIONS_MODE_KEY, text("disable"))])));
        let policy_service = new_policy_service(&d, &f.default_account_service, None, Some(native.clone()), None);
        let policy_configuration = new_policy_configuration(&d, &policy_service);

        install(&d, &f.default_account_service, TestProvider::new(base_default_account(), Some(policy_data(None, None))));
        f.default_account_service.refresh(None).await.unwrap();

        policy_configuration.initialize().await.unwrap();

        assert_eq!(
            (policy_service.get_policy_value("PolicySettingF"), policy_service.get_policy_value_source("PolicySettingF"), policy_configuration.configuration_model().get_value("setting.F")),
            (b(false), Some(PolicyValueSource::NativeMdm), Dynamic::Bool(false))
        );
        assert_eq!(native.registered(COPILOT_DISABLE_BYPASS_PERMISSIONS_MODE_KEY).as_deref(), Some("string"));
        assert_eq!(native.registered(COPILOT_ENABLED_PLUGINS_KEY).as_deref(), Some("string"));
        assert_eq!(native.registered(COPILOT_TEMPORARY_DEFAULT_TO_LOCAL_HARNESS_KEY).as_deref(), Some("boolean"));
    });
}

#[test]
fn managed_settings_native_mdm_value_wins_over_server_for_the_same_declared_key() {
    async_test(|d| async move {
        // Server says 'enable', native MDM says 'disable'. Native MDM is the authoritative
        // source when present, so the server value is ignored entirely and the gated policy IS
        // forced to `false`.
        let f = setup(&d).await;
        let native = d.add_rc(FakeNativeManagedSettingsService::new(managed(vec![(COPILOT_DISABLE_BYPASS_PERMISSIONS_MODE_KEY, text("disable"))])));
        let policy_service = new_policy_service(&d, &f.default_account_service, None, Some(native), None);
        let policy_configuration = new_policy_configuration(&d, &policy_service);

        let data = policy_data(None, Some(vec![(COPILOT_DISABLE_BYPASS_PERMISSIONS_MODE_KEY, text("enable"))]));
        install(&d, &f.default_account_service, TestProvider::new(base_default_account(), Some(data)));
        f.default_account_service.refresh(None).await.unwrap();

        policy_configuration.initialize().await.unwrap();

        assert_eq!(policy_service.get_policy_value("PolicySettingF"), b(false));
        assert_eq!(policy_service.get_policy_value_source("PolicySettingF"), Some(PolicyValueSource::NativeMdm));
    });
}

#[test]
fn managed_settings_non_causal_setting_does_not_take_attribution_from_account_data() {
    async_test(|d| async move {
        let f = setup(&d).await;
        let native = d.add_rc(FakeNativeManagedSettingsService::new(managed(vec![(COPILOT_DISABLE_BYPASS_PERMISSIONS_MODE_KEY, text("enable"))])));
        let policy_service = new_policy_service(&d, &f.default_account_service, None, Some(native.clone()), None);
        let policy_configuration = new_policy_configuration(&d, &policy_service);

        install(&d, &f.default_account_service, TestProvider::new(base_default_account(), Some(policy_data(Some(false), None))));
        f.default_account_service.refresh(None).await.unwrap();
        policy_configuration.initialize().await.unwrap();

        assert_eq!((policy_service.get_policy_value("PolicySettingH"), policy_service.get_policy_value_source("PolicySettingH")), (b(false), Some(PolicyValueSource::Account)));

        let change = policy_service.on_did_change().to_promise(None);
        native.set_managed_settings(managed(vec![(COPILOT_DISABLE_BYPASS_PERMISSIONS_MODE_KEY, text("disable"))]));

        assert_eq!(change.await.unwrap(), vec!["PolicySettingF".to_owned()]);
        assert_eq!((policy_service.get_policy_value("PolicySettingH"), policy_service.get_policy_value_source("PolicySettingH")), (b(false), Some(PolicyValueSource::Account)));
    });
}

#[test]
fn managed_settings_native_mdm_applies_when_the_server_provides_no_managed_settings() {
    async_test(|d| async move {
        // No server managed settings: native MDM is the authoritative source and forces the
        // gated policy to `false`.
        let f = setup(&d).await;
        let native = d.add_rc(FakeNativeManagedSettingsService::new(managed(vec![(COPILOT_DISABLE_BYPASS_PERMISSIONS_MODE_KEY, text("disable"))])));
        let policy_service = new_policy_service(&d, &f.default_account_service, None, Some(native), None);
        let policy_configuration = new_policy_configuration(&d, &policy_service);

        install(&d, &f.default_account_service, TestProvider::new(base_default_account(), Some(policy_data(None, None))));
        f.default_account_service.refresh(None).await.unwrap();

        policy_configuration.initialize().await.unwrap();

        assert_eq!(policy_service.get_policy_value("PolicySettingF"), b(false));
        assert_eq!(policy_service.get_policy_value_source("PolicySettingF"), Some(PolicyValueSource::NativeMdm));
    });
}

#[test]
fn managed_settings_three_channel_precedence_preserves_the_sandbox_force_on_exception() {
    async_test(|d| async move {
        // All three channels provide the same key with different values.
        // Server says 'enable', MDM says 'disable', File says 'file-value'.
        // Native MDM should win.
        let f = setup(&d).await;
        let file = d.add_rc(FakeFileManagedSettingsService::new(managed(vec![(COPILOT_DISABLE_BYPASS_PERMISSIONS_MODE_KEY, text("file-value")), (COPILOT_SANDBOX_ENABLED_KEY, BaseValue::Boolean(true))])));
        let native = d.add_rc(FakeNativeManagedSettingsService::new(managed(vec![(COPILOT_DISABLE_BYPASS_PERMISSIONS_MODE_KEY, text("disable")), (COPILOT_SANDBOX_ENABLED_KEY, BaseValue::Boolean(false))])));
        let policy_service = new_policy_service(&d, &f.default_account_service, None, Some(native), Some(file));
        let policy_configuration = new_policy_configuration(&d, &policy_service);

        let data = policy_data(None, Some(vec![(COPILOT_DISABLE_BYPASS_PERMISSIONS_MODE_KEY, text("enable")), (COPILOT_SANDBOX_ENABLED_KEY, BaseValue::Boolean(true))]));
        install(&d, &f.default_account_service, TestProvider::new(base_default_account(), Some(data)));
        f.default_account_service.refresh(None).await.unwrap();

        policy_configuration.initialize().await.unwrap();

        assert_eq!(
            (policy_service.get_policy_value("PolicySettingF"), policy_service.get_policy_value_source("PolicySettingF"), policy_service.get_managed_setting_value(COPILOT_SANDBOX_ENABLED_KEY)),
            (b(false), Some(PolicyValueSource::NativeMdm), Some(BaseValue::Boolean(true)))
        );
    });
}

#[test]
fn managed_telemetry_block_replacement_removes_lower_source_identity_policy_and_withdrawal_restores_it() {
    async_test(|d| async move {
        let f = setup(&d).await;
        let key = je_platform::policy::copilot_managed_settings::COPILOT_OTEL_CAPTURE_IDENTITY_KEY;
        let file = d.add_rc(FakeFileManagedSettingsService::new(normalize_managed_settings(&j("{ 'telemetry': { 'capture': { 'identity': true } } }"), None)));
        let native = d.add_rc(FakeNativeManagedSettingsService::new(ManagedSettingsData::new()));
        let policy_service = new_policy_service(&d, &f.default_account_service, None, Some(native.clone()), Some(file));
        let definitions = vec![("TelemetryIdentity".to_owned(), definition(PolicyDefinitionType::Boolean, Some(managed_setting_value(key)), &[(key, "boolean")], None))];
        policy_service.update_policy_definitions(definitions).await.unwrap();
        let initial = policy_service.get_policy_value("TelemetryIdentity");
        let replaced = policy_service.on_did_change().to_promise(None);
        native.set_managed_settings(normalize_managed_settings(&j("{ 'telemetry': {} }"), None));
        replaced.await.unwrap();
        let empty = policy_service.get_policy_value("TelemetryIdentity");
        let denied = policy_service.on_did_change().to_promise(None);
        native.set_managed_settings(normalize_managed_settings(&j("{ 'telemetry': { 'capture': { 'identity': false } } }"), None));
        denied.await.unwrap();
        let explicit_false = policy_service.get_policy_value("TelemetryIdentity");
        let withdrawn = policy_service.on_did_change().to_promise(None);
        native.set_managed_settings(ManagedSettingsData::new());
        withdrawn.await.unwrap();
        assert_eq!((initial, empty, explicit_false, policy_service.get_policy_value("TelemetryIdentity")), (b(true), None, b(false), b(true)));
    });
}

#[test]
fn auto_defaults_discard_the_previous_account_value_while_new_policy_is_unresolved_and_when_removed() {
    async_test(|d| async move {
        let f = setup(&d).await;
        let provider = install(&d, &f.default_account_service, TestProvider::new(base_default_account(), Some(policy_data(None, Some(vec![(COPILOT_AUTO_TIER_KEY, text("intelligence"))])))));
        f.default_account_service.refresh(None).await.unwrap();
        let policy_service = new_policy_service(&d, &f.default_account_service, None, None, None);
        policy_service.update_policy_definitions(Vec::new()).await.unwrap();
        let initial = policy_service.get_managed_setting_value(COPILOT_AUTO_TIER_KEY);
        let mut second = base_default_account();
        second.account_name = "second-account".to_owned();
        second.session_id = "second-session".to_owned();
        *provider.default_account.borrow_mut() = Some(second.clone());
        *provider.policy_data.borrow_mut() = None;
        provider.on_did_change_default_account.fire(Some(second));
        provider.on_did_change_policy_data.fire(None);
        f.default_account_service.refresh(None).await.unwrap();
        policy_service.update_policy_definitions(Vec::new()).await.unwrap();
        let unresolved = policy_service.get_managed_setting_value(COPILOT_AUTO_TIER_KEY);
        let loaded = policy_service.on_did_change_managed_settings().to_promise(None);
        provider.set_policy_data(Some(policy_data(None, Some(vec![(COPILOT_AUTO_TIER_KEY, text("efficiency"))]))));
        loaded.await.unwrap();
        let next_account = policy_service.get_managed_setting_value(COPILOT_AUTO_TIER_KEY);
        let cleared = policy_service.on_did_change_managed_settings().to_promise(None);
        provider.set_policy_data(Some(policy_data(None, None)));
        cleared.await.unwrap();
        assert_eq!(
            (initial, unresolved, next_account, policy_service.get_managed_setting_value(COPILOT_AUTO_TIER_KEY)),
            (Some(text("intelligence")), None, Some(text("efficiency")), None)
        );
    });
}

#[test]
#[ignore = "depende de AgentHostEnablementService, getComputedDefaultSessionType e do chatSessionsService, do chat (fora do escopo do porte, sem porte)"]
fn managed_sandbox_policy_refresh_selects_agent_host_copilot_despite_device_false_and_remembered_local() {}

fn channel_source(channel: &str) -> PolicyValueSource {
    match channel {
        "nativeMdm" => PolicyValueSource::NativeMdm,
        "file" => PolicyValueSource::FileManagedSettings,
        _ => PolicyValueSource::ServerManagedSettings,
    }
}

/// A pilha com as três fontes e o `changed` do provedor, compartilhada pelos casos por canal.
struct ChannelStack {
    native: Rc<FakeNativeManagedSettingsService>,
    file: Rc<FakeFileManagedSettingsService>,
    provider: Rc<TestProvider>,
    policy_service: Rc<AccountPolicyService>,
}

impl ChannelStack {
    async fn new(d: &Rc<DisposableStore>, f: &Fixture) -> Self {
        let native = d.add_rc(FakeNativeManagedSettingsService::new(ManagedSettingsData::new()));
        let file = d.add_rc(FakeFileManagedSettingsService::new(ManagedSettingsData::new()));
        let provider = install(d, &f.default_account_service, TestProvider::new(base_default_account(), Some(policy_data(None, None))));
        f.default_account_service.refresh(None).await.unwrap();
        let policy_service = new_policy_service(d, &f.default_account_service, None, Some(native.clone()), Some(file.clone()));
        Self { native, file, provider, policy_service }
    }

    /// Entrega `values` (e o documento bruto `raw`) pelo canal.
    fn deliver(&self, channel: &str, values: ManagedSettingsData, raw: Option<Dynamic>, server: Option<IPolicyData>) {
        match channel {
            "nativeMdm" => self.native.set_managed_settings(values),
            "file" => self.file.set_managed_settings(values, raw),
            _ => self.provider.set_policy_data(server),
        }
    }
}

fn dynamic_values() -> Vec<Dynamic> {
    vec![Dynamic::Undefined, Dynamic::Bool(false), Dynamic::Bool(true), s("false"), Dynamic::Number(0.0), Dynamic::Null, j("{}"), j("[]"), j("{ 'future': true }"), Dynamic::Undefined]
}

#[test]
fn strict_known_marketplaces_from_each_channel_is_available_without_a_configuration_policy() {
    for channel in ["nativeMdm", "server", "file"] {
        async_test(move |d| async move {
            let f = setup(&d).await;
            let stack = ChannelStack::new(&d, &f).await;
            stack.provider.set_policy_data(Some(policy_data(None, None)));
            stack.policy_service.update_policy_definitions(Vec::new()).await.unwrap();
            let mut results = Vec::new();
            for raw in ["{ 'strictKnownMarketplaces': [{ 'source': 'github', 'repo': 'enterprise/catalog' }] }", "{ 'strictKnownMarketplaces': [] }", "{}"] {
                let raw = j(raw);
                let update = stack.policy_service.on_did_change_managed_settings().to_promise(None);
                let values = normalize_managed_settings(&raw, None);
                stack.deliver(channel, values, Some(raw.clone()), Some(adapt_managed_settings(&raw, None)));
                update.await.unwrap();
                results.push(stack.policy_service.get_managed_setting_value("strictKnownMarketplaces"));
            }
            assert_eq!(results, vec![Some(text("[{\"source\":\"github\",\"repo\":\"enterprise/catalog\"}]")), Some(text("[]")), None], "{channel}");
        });
    }
}

#[test]
fn temporary_local_permission_from_each_channel_disables_third_party_harnesses_across_updates_and_removal() {
    for channel in ["nativeMdm", "server", "file"] {
        async_test(move |d| async move {
            let f = setup(&d).await;
            let configuration = TestConfigurationService::new(None);
            let stack = ChannelStack::new(&d, &f).await;
            stack.provider.set_policy_data(Some(policy_data(None, None)));
            let mut definitions = third_party_agent_policies();
            definitions.push(presence_policy());
            stack.policy_service.update_policy_definitions(definitions.clone()).await.unwrap();
            for value in dynamic_values() {
                let raw = Dynamic::object([("vscode", Dynamic::object([("temporaryAllowLocalHarness", value.clone())]))]);
                let values = normalize_managed_settings(&raw, None);
                stack.deliver(channel, values, Some(raw.clone()), Some(adapt_managed_settings(&raw, None)));
                let mut next = definitions.clone();
                next.extend(third_party_agent_policies());
                stack.policy_service.update_policy_definitions(next).await.unwrap();
                let is_false = value == Dynamic::Bool(false);
                let service = &stack.policy_service;
                assert_eq!(requires_copilot_agent_host(&**service, &*configuration), is_false, "{channel} {value:?}");
                assert!(!has_managed_permission_denials(&**service));
                assert_eq!(service.get_managed_setting_value(COPILOT_SANDBOX_ENABLED_KEY), None);
                assert_eq!(service.get_policy_value("Claude"), if is_false { b(false) } else { None });
                assert_eq!(service.get_policy_value("Codex"), if is_false { b(false) } else { None });
                assert_eq!(service.get_policy_value("Presence"), None);
                assert_eq!(service.get_policy_value_source("Claude"), if is_false { Some(channel_source(channel)) } else { None });
            }
        });
    }
}

#[test]
fn temporary_local_escape_hatch_projects_from_each_channel_and_refreshes_on_false_and_removal() {
    for channel in ["nativeMdm", "server", "file"] {
        async_test(move |d| async move {
            let key = COPILOT_TEMPORARY_DEFAULT_TO_LOCAL_HARNESS_KEY;
            let f = setup(&d).await;
            let configuration = TestConfigurationService::new(None);
            let stack = ChannelStack::new(&d, &f).await;
            let policy_configuration = new_policy_configuration(&d, &stack.policy_service);
            policy_configuration.initialize().await.unwrap();
            let mut results = Vec::new();
            for value in [Some(true), Some(false), None] {
                let raw = Dynamic::object([
                    ("vscode", Dynamic::object([("temporaryDefaultToLocalHarness", flag(value))])),
                    ("sandbox", j("{ 'enabled': true }")),
                    ("permissions", j("{ 'allow': [] }")),
                ]);
                let values = normalize_managed_settings(&raw, None);
                let updated = policy_configuration.on_did_change_configuration().to_promise(None);
                stack.deliver(channel, values.clone(), None, Some(policy_data(None, Some(values.iter().map(|(k, v)| (k.as_str(), v.clone())).collect()))));
                updated.await.unwrap();
                let service = &stack.policy_service;
                results.push((
                    service.get_managed_setting_value(key),
                    service.get_policy_value("ChatTemporaryDefaultToLocalHarness"),
                    policy_configuration.configuration_model().get_value(TEMPORARY_DEFAULT_TO_LOCAL_HARNESS_SETTING_ID),
                    requires_copilot_agent_host(&**service, &*configuration),
                    service.get_policy_value_source("ChatTemporaryDefaultToLocalHarness"),
                ));
            }
            let source = channel_source(channel);
            assert_eq!(
                results,
                vec![
                    (Some(BaseValue::Boolean(true)), b(true), Dynamic::Bool(true), false, Some(source)),
                    (Some(BaseValue::Boolean(false)), b(false), Dynamic::Bool(false), true, Some(source)),
                    (None, None, Dynamic::Undefined, true, None),
                ],
                "{channel}"
            );
            assert_eq!(stack.native.registered(key).as_deref(), Some("boolean"));
        });
    }
}

#[test]
fn only_permission_denials_require_agent_host_across_each_channel_refresh_and_removal_without_relaxing_third_party_governance() {
    for channel in ["nativeMdm", "server", "file"] {
        async_test(move |d| async move {
            let f = setup(&d).await;
            let stack = ChannelStack::new(&d, &f).await;
            let definitions: PolicyDefinitions = ["Claude", "Codex"]
                .into_iter()
                .map(|name| (name.to_owned(), definition(PolicyDefinitionType::Boolean, Some(PolicyValueFn::new(third_party_agent_enabled_value)), &[], None)))
                .collect();
            stack.policy_service.update_policy_definitions(definitions.clone()).await.unwrap();
            let mut keys: Vec<&str> = COPILOT_PERMISSION_RULES_KEYS.to_vec();
            keys.extend(["permissions.defaultMode", "permissions.disableBypassPermissionsMode", "permissions.disableAssistedPermissionsMode", "permissions.limitTo"]);
            for key in keys {
                let cases: [(Option<BaseValue>, bool, Option<bool>); 5] = [
                    (Some(text("[\"Shell\"]")), key == "permissions.deny", Some(false)),
                    (Some(text("[]")), false, Some(false)),
                    (Some(BaseValue::Boolean(false)), key == "permissions.deny", Some(false)),
                    (Some(text("enable")), key == "permissions.deny", Some(false)),
                    (None, false, None),
                ];
                for (rules, required, governed) in cases {
                    let values = rules.map_or_else(ManagedSettingsData::new, |rules| managed(vec![(key, rules)]));
                    stack.deliver(channel, values.clone(), None, Some(policy_data(None, Some(values.iter().map(|(k, v)| (k.as_str(), v.clone())).collect()))));
                    stack.policy_service.update_policy_definitions(definitions.clone()).await.unwrap();
                    let service = &stack.policy_service;
                    assert_eq!(has_managed_permission_denials(&**service), required, "{channel} {key}");
                    assert_eq!(service.get_managed_setting_value(COPILOT_SANDBOX_ENABLED_KEY), None);
                    assert_eq!(service.get_policy_value("Claude"), governed.and_then(b));
                    assert_eq!(service.get_policy_value("Codex"), governed.and_then(b));
                }
            }
        });
    }
}

#[test]
fn allowing_local_preserves_other_governance_and_winning_source_attribution() {
    async_test(|d| async move {
        let key = COPILOT_TEMPORARY_ALLOW_LOCAL_HARNESS_KEY;
        let f = setup(&d).await;
        let stack = ChannelStack::new(&d, &f).await;
        let mut definitions = third_party_agent_policies();
        definitions.push(presence_policy());
        let configuration = TestConfigurationService::new(None);
        let others = [j("{}"), j("{ 'sandbox': { 'enabled': false } }"), j("{ 'permissions': { 'allow': [] } }"), j("{ 'vscode': { 'temporaryDefaultToLocalHarness': false } }"), j("{ 'futureControl': {} }")];
        for channel in ["nativeMdm", "server", "file"] {
            for other in &others {
                let mut snapshots = Vec::new();
                for include_inactive_control in [false, true] {
                    let values = if include_inactive_control {
                        let mut entries = other.own_entries();
                        entries.push((key.into(), Dynamic::Bool(true)));
                        Dynamic::object(entries)
                    } else {
                        other.clone()
                    };
                    let normalized = normalize_managed_settings(&values, None);
                    stack.native.set_managed_settings(if channel == "nativeMdm" { normalized.clone() } else { ManagedSettingsData::new() });
                    stack.file.set_managed_settings(if channel == "file" { normalized.clone() } else { ManagedSettingsData::new() }, Some(if channel == "file" { values.clone() } else { j("{}") }));
                    stack.provider.set_policy_data(Some(if channel == "server" { adapt_managed_settings(&values, None) } else { policy_data(None, None) }));
                    let mut next = definitions.clone();
                    next.extend(third_party_agent_policies());
                    stack.policy_service.update_policy_definitions(next).await.unwrap();
                    let service = &stack.policy_service;
                    snapshots.push((
                        requires_copilot_agent_host(&**service, &*configuration),
                        service.get_policy_value("Claude"),
                        service.get_policy_value("Codex"),
                        service.get_policy_value("Presence"),
                        service.get_policy_value_source("Claude"),
                    ));
                }
                assert_eq!(snapshots[1], snapshots[0], "{channel} {other:?}");
            }
        }
    });
}

#[test]
fn temporary_local_permission_disables_third_party_harnesses_only_at_the_effective_false_value() {
    async_test(|d| async move {
        let key = COPILOT_TEMPORARY_ALLOW_LOCAL_HARNESS_KEY;
        let f = setup(&d).await;
        let stack = ChannelStack::new(&d, &f).await;
        let mut definitions = third_party_agent_policies();
        definitions.push(presence_policy());
        let cases: [[Option<bool>; 3]; 7] = [
            [Some(false), Some(true), Some(true)],
            [None, Some(true), Some(true)],
            [Some(true), Some(false), Some(false)],
            [None, Some(false), Some(false)],
            [None, Some(true), Some(false)],
            [None, None, Some(false)],
            [None, None, None],
        ];
        let expected: [(Option<bool>, Option<bool>, Option<PolicyValueSource>); 7] = [
            (Some(false), Some(false), Some(PolicyValueSource::NativeMdm)),
            (Some(true), None, None),
            (Some(true), None, None),
            (Some(false), Some(false), Some(PolicyValueSource::ServerManagedSettings)),
            (Some(true), None, None),
            (Some(false), Some(false), Some(PolicyValueSource::FileManagedSettings)),
            (None, None, None),
        ];
        let setting = |value: Option<bool>| value.map_or_else(ManagedSettingsData::new, |value| managed(vec![(key, BaseValue::Boolean(value))]));
        for ([native_value, server_value, file_value], (effective, governed, source)) in cases.into_iter().zip(expected) {
            stack.native.set_managed_settings(setting(native_value));
            stack.file.set_managed_settings(setting(file_value), None);
            let raw = server_value.map_or_else(|| j("{}"), |value| Dynamic::object([(key, Dynamic::Bool(value))]));
            stack.provider.set_policy_data(Some(adapt_managed_settings(&raw, None)));
            let mut next = definitions.clone();
            next.extend(third_party_agent_policies());
            stack.policy_service.update_policy_definitions(next).await.unwrap();
            let service = &stack.policy_service;
            assert_eq!(service.get_managed_setting_value(key), effective.map(BaseValue::Boolean));
            assert_eq!(service.get_policy_value("Claude"), governed.and_then(b));
            assert_eq!(service.get_policy_value("Codex"), governed.and_then(b));
            assert_eq!(service.get_policy_value("Presence"), None);
            assert_eq!(service.get_policy_value_source("Claude"), source);
        }
    });
}

#[test]
fn temporary_local_permission_from_each_channel_disables_third_party_harnesses_without_an_account() {
    for channel in ["nativeMdm", "file"] {
        async_test(move |d| async move {
            let key = COPILOT_TEMPORARY_ALLOW_LOCAL_HARNESS_KEY;
            let f = setup(&d).await;
            let native = d.add_rc(FakeNativeManagedSettingsService::new(if channel == "nativeMdm" { managed(vec![(key, BaseValue::Boolean(false))]) } else { ManagedSettingsData::new() }));
            let file = d.add_rc(FakeFileManagedSettingsService::new(if channel == "file" { managed(vec![(key, BaseValue::Boolean(false))]) } else { ManagedSettingsData::new() }));
            let policy_service = new_policy_service(&d, &f.default_account_service, None, Some(native), Some(file));
            policy_service.update_policy_definitions(third_party_agent_policies()).await.unwrap();
            assert_eq!(
                (policy_service.get_policy_value("Claude"), policy_service.get_policy_value("Codex"), policy_service.get_policy_value_source("Claude")),
                (b(false), b(false), Some(channel_source(channel)))
            );
        });
    }
}

#[test]
fn local_only_permission_preserves_unrepresentable_server_governance() {
    async_test(|d| async move {
        let key = COPILOT_TEMPORARY_ALLOW_LOCAL_HARNESS_KEY;
        let f = setup(&d).await;
        let native = d.add_rc(FakeNativeManagedSettingsService::new(managed(vec![(key, BaseValue::Boolean(false))])));
        install(&d, &f.default_account_service, TestProvider::new(base_default_account(), Some(adapt_managed_settings(&j("{ 'vscode': { 'temporaryAllowLocalHarness': true }, 'futureControl': {} }"), None))));
        f.default_account_service.refresh(None).await.unwrap();
        let policy_service = new_policy_service(&d, &f.default_account_service, None, Some(native), None);
        policy_service.update_policy_definitions(third_party_agent_policies()).await.unwrap();
        assert_eq!(
            (policy_service.get_managed_setting_value(key), policy_service.get_policy_value("Claude"), policy_service.get_policy_value_source("Claude")),
            (Some(BaseValue::Boolean(false)), b(false), Some(PolicyValueSource::ServerManagedSettings))
        );
    });
}

/// O corpo comum dos casos de `FakeFileManagedSettingsService` + `FakeNativeManagedSettingsService` com um
/// `PolicyConfiguration`.
async fn file_and_native_stack(d: &Rc<DisposableStore>, file: Rc<FakeFileManagedSettingsService>, native: Rc<FakeNativeManagedSettingsService>) -> (Fixture, Rc<AccountPolicyService>, Rc<PolicyConfiguration>) {
    let f = setup(d).await;
    let policy_service = new_policy_service(d, &f.default_account_service, None, Some(native), Some(file));
    let policy_configuration = new_policy_configuration(d, &policy_service);
    install(d, &f.default_account_service, TestProvider::new(base_default_account(), Some(policy_data(None, None))));
    f.default_account_service.refresh(None).await.unwrap();
    policy_configuration.initialize().await.unwrap();
    (f, policy_service, policy_configuration)
}

#[test]
fn managed_settings_file_based_settings_apply_when_server_and_mdm_are_empty() {
    async_test(|d| async move {
        // Only the file channel provides a value: it should be used.
        let file = d.add_rc(FakeFileManagedSettingsService::new(managed(vec![(COPILOT_DISABLE_BYPASS_PERMISSIONS_MODE_KEY, text("disable"))])));
        let native = d.add_rc(FakeNativeManagedSettingsService::new(ManagedSettingsData::new()));
        let (_f, policy_service, _config) = file_and_native_stack(&d, file, native).await;
        // File value 'disable' applies: policy is forced to false
        assert_eq!(policy_service.get_policy_value("PolicySettingF"), b(false));
        assert_eq!(policy_service.get_policy_value_source("PolicySettingF"), Some(PolicyValueSource::FileManagedSettings));
    });
}

#[test]
fn managed_settings_per_key_precedence_merges_across_channels_different_keys_win_from_different_channels() {
    async_test(|d| async move {
        // Native MDM supplies only the disableBypass key; the file supplies only the enabledPlugins
        // key. Neither overrides the other, so BOTH reach policy evaluation.
        let json = "{\"assign-issue@skills\":true}";
        let file = d.add_rc(FakeFileManagedSettingsService::new(managed(vec![(COPILOT_ENABLED_PLUGINS_KEY, text(json))])));
        let native = d.add_rc(FakeNativeManagedSettingsService::new(managed(vec![(COPILOT_DISABLE_BYPASS_PERMISSIONS_MODE_KEY, text("disable"))])));
        let (_f, policy_service, config) = file_and_native_stack(&d, file, native).await;
        let model = config.configuration_model();
        assert_eq!(model.get_value("setting.F"), Dynamic::Bool(false));
        assert_deep_strict_equal(&model.get_value("setting.G"), &j("{ 'assign-issue@skills': true }"));
        assert_eq!(policy_service.get_policy_value_source("PolicySettingF"), Some(PolicyValueSource::NativeMdm));
        assert_eq!(policy_service.get_policy_value_source("PolicySettingG"), Some(PolicyValueSource::FileManagedSettings));
    });
}

#[test]
fn managed_settings_attributes_policies_caused_by_multiple_channels_as_mixed() {
    async_test(|d| async move {
        let file = d.add_rc(FakeFileManagedSettingsService::new(managed(vec![(COPILOT_ENABLED_PLUGINS_KEY, text("{\"assign-issue@skills\":true}"))])));
        let native = d.add_rc(FakeNativeManagedSettingsService::new(managed(vec![(COPILOT_DISABLE_BYPASS_PERMISSIONS_MODE_KEY, text("disable"))])));
        let (_f, policy_service, _config) = file_and_native_stack(&d, file, native).await;
        assert_eq!((policy_service.get_policy_value("PolicySettingI"), policy_service.get_policy_value_source("PolicySettingI")), (b(false), Some(PolicyValueSource::MixedManagedSettings)));
    });
}

/// Pilha só com arquivo (sem MDM) cujo documento bruto é `raw`.
async fn raw_file_stack(d: &Rc<DisposableStore>, raw: Dynamic) -> (Rc<FakeFileManagedSettingsService>, Rc<AccountPolicyService>, Rc<PolicyConfiguration>) {
    let f = setup(d).await;
    let file = d.add_rc(FakeFileManagedSettingsService::with_raw(ManagedSettingsData::new(), raw));
    let policy_service = new_policy_service(d, &f.default_account_service, None, None, Some(file.clone()));
    let policy_configuration = new_policy_configuration(d, &policy_service);
    install(d, &f.default_account_service, TestProvider::new(base_default_account(), Some(policy_data(None, None))));
    f.default_account_service.refresh(None).await.unwrap();
    policy_configuration.initialize().await.unwrap();
    (file, policy_service, policy_configuration)
}

#[test]
fn managed_settings_raw_permission_rules_disable_presence_gated_settings() {
    async_test(|d| async move {
        let (_file, policy_service, config) = raw_file_stack(&d, j("{ 'permissions': { 'deny': ['Shell'] } }")).await;
        assert_eq!(
            (config.configuration_model().get_value("setting.J"), policy_service.get_policy_value("PolicySettingJ"), policy_service.get_policy_value_source("PolicySettingJ")),
            (Dynamic::Bool(false), b(false), Some(PolicyValueSource::FileManagedSettings))
        );
    });
}

#[test]
fn managed_settings_removing_raw_permission_rules_releases_presence_gated_settings() {
    async_test(|d| async move {
        let (file, policy_service, config) = raw_file_stack(&d, j("{ 'permissions': { 'deny': ['Shell'] } }")).await;
        let change = policy_service.on_did_change().to_promise(None);
        file.set_managed_settings(ManagedSettingsData::new(), Some(j("{}")));
        assert_eq!(change.await.unwrap(), vec!["PolicySettingJ".to_owned()]);
        assert_eq!(config.configuration_model().get_value("setting.J"), Dynamic::Undefined);
        assert_eq!(policy_service.get_policy_value("PolicySettingJ"), None);
    });
}

async fn presence_case(d: &Rc<DisposableStore>, data: IPolicyData) -> Fixture {
    let f = setup(d).await;
    install(d, &f.default_account_service, TestProvider::new(base_default_account(), Some(data)));
    f.default_account_service.refresh(None).await.unwrap();
    f.policy_configuration.initialize().await.unwrap();
    f
}

#[test]
fn managed_settings_sandbox_settings_disable_presence_gated_settings_even_when_false() {
    async_test(|d| async move {
        let f = presence_case(&d, policy_data(None, Some(vec![(COPILOT_SANDBOX_ENABLED_KEY, BaseValue::Boolean(false))]))).await;
        assert_eq!(
            (f.policy_configuration.configuration_model().get_value("setting.J"), f.policy_service.get_policy_value("PolicySettingJ"), f.policy_service.get_policy_value_source("PolicySettingJ")),
            (Dynamic::Bool(false), b(false), Some(PolicyValueSource::ServerManagedSettings))
        );
    });
}

#[test]
fn managed_settings_raw_server_settings_disable_presence_gated_settings() {
    async_test(|d| async move {
        let mut data = policy_data(None, Some(Vec::new()));
        data.managed_settings_active = Some(true);
        let f = presence_case(&d, data).await;
        assert_eq!(
            (f.policy_configuration.configuration_model().get_value("setting.J"), f.policy_service.get_policy_value("PolicySettingJ"), f.policy_service.get_policy_value_source("PolicySettingJ")),
            (Dynamic::Bool(false), b(false), Some(PolicyValueSource::ServerManagedSettings))
        );
    });
}

#[test]
fn managed_settings_an_ungoverned_account_leaves_presence_gated_settings_alone() {
    async_test(|d| async move {
        let f = presence_case(&d, policy_data(Some(true), None)).await;
        assert_eq!((f.policy_configuration.configuration_model().get_value("setting.J"), f.policy_service.get_policy_value("PolicySettingJ")), (Dynamic::Undefined, None));
    });
}

#[test]
fn managed_settings_an_object_typed_setting_resolves_identically_from_server_and_native_mdm_json_strings() {
    async_test(|d| async move {
        // Structured-setting invariant: whether the canonical JSON string arrives via the server
        // account policy bag or via native MDM, PolicyConfiguration must parse it back into the
        // SAME typed object for an `object`-typed setting. The only difference is the source.
        let json = "{\"assign-issue@skills\":true,\"other@acme\":false}";
        let expected = j("{ 'assign-issue@skills': true, 'other@acme': false }");
        let _registry = RegistryGuard::new();
        configuration_registry().register_configuration(policy_configuration_node());

        let resolve = |server: Option<&'static str>, mdm: Option<&'static str>| {
            let d = d.clone();
            async move {
                let account_service = new_default_account_service(&d);
                let native = d.add_rc(FakeNativeManagedSettingsService::new(mdm.map_or_else(ManagedSettingsData::new, |value| managed(vec![(COPILOT_ENABLED_PLUGINS_KEY, text(value))]))));
                let service = new_policy_service(&d, &account_service, None, Some(native), None);
                let config = new_policy_configuration(&d, &service);
                let data = server.map_or_else(|| policy_data(None, None), |value| policy_data(None, Some(vec![(COPILOT_ENABLED_PLUGINS_KEY, text(value))])));
                install(&d, &account_service, TestProvider::new(base_default_account(), Some(data)));
                account_service.refresh(None).await.unwrap();
                config.initialize().await.unwrap();
                config.configuration_model().get_value("setting.G")
            }
        };
        let server_config = resolve(Some(json), None).await;
        let mdm_config = resolve(None, Some(json)).await;
        assert_deep_strict_equal(&server_config, &expected);
        assert_deep_strict_equal(&mdm_config, &expected);
    });
}

// ---- "Require Approved Account" gate ----

struct GateOptions {
    approved_orgs: Option<PolicyValue>,
    account: Option<IDefaultAccount>,
    policy_data: Option<Option<IPolicyData>>,
    freshness: Option<IManagedSettingsFreshness>,
}

fn orgs(orgs: &[&str]) -> Option<PolicyValue> {
    Some(PolicyValue::String(format!("[{}]", orgs.iter().map(|org| format!("{org:?}")).collect::<Vec<_>>().join(","))))
}

async fn setup_gate(d: &Rc<DisposableStore>, opts: GateOptions) -> (Fixture, Rc<AccountPolicyService>, Rc<FakeManagedPolicyService>) {
    let f = setup(d).await;
    let managed = d.add_rc(FakeManagedPolicyService::new());
    if let Some(value) = opts.approved_orgs {
        // Mirror how the platform delivers array-typed policy values: as a JSON-stringified array.
        managed.set_policy(APPROVED_ACCOUNT_ORGANIZATIONS_POLICY_NAME, Some(value));
    }
    let account_service = new_default_account_service(d);
    if let Some(account) = opts.account {
        let policy_data = opts.policy_data.unwrap_or_else(|| Some(policy_data_empty()));
        install(d, &account_service, TestProvider::with_freshness(account, policy_data, opts.freshness.unwrap_or(IManagedSettingsFreshness::NotRequired)));
        account_service.refresh(None).await.unwrap();
    }
    let service = new_policy_service(d, &account_service, Some(managed.clone()), None, None);
    let config = new_policy_configuration(d, &service);
    config.initialize().await.unwrap();
    (f, service, managed)
}

fn policy_data_empty() -> IPolicyData {
    policy_data(None, None)
}

fn network_blocked() -> IManagedSettingsFreshness {
    IManagedSettingsFreshness::Blocked {
        effective: IManagedSettingsFreshnessEffective { source: je_platform::policy::copilot_managed_settings::ManagedSettingsChannel::Server, scope: None, last_attempt_at: Some(42) },
        failure: ManagedSettingsFreshnessFailure::Network,
    }
}

fn pending() -> IManagedSettingsFreshness {
    IManagedSettingsFreshness::Pending(IManagedSettingsFreshnessEffective { source: je_platform::policy::copilot_managed_settings::ManagedSettingsChannel::Server, scope: None, last_attempt_at: None })
}

fn gate_default() -> GateOptions {
    GateOptions { approved_orgs: None, account: None, policy_data: None, freshness: None }
}

#[test]
fn policy_settlement_waits_for_gate_initialization() {
    for scenario in ["pending", "unresolved", "inactive", "signedOut"] {
        async_test(move |d| async move {
            let _registry = RegistryGuard::new();
            let account_initialized: DeferredPromise<Option<IDefaultAccount>> = DeferredPromise::new();
            let gate_update_started: DeferredPromise<()> = DeferredPromise::new();
            let provider = d.add_rc(TestProvider::new(base_default_account(), None));
            *provider.refresh_override.borrow_mut() = Some(account_initialized.promise.clone());
            let account_service = new_default_account_service(&d);
            account_service.set_default_account_provider(provider.clone()).unwrap();
            let managed = d.add_rc(FakeManagedPolicyService::new());
            if scenario == "unresolved" || scenario == "signedOut" {
                managed.set_policy(APPROVED_ACCOUNT_ORGANIZATIONS_POLICY_NAME, orgs(&["*"]));
            }
            let native = d.add_rc(FakeNativeManagedSettingsService::new(ManagedSettingsData::new()));
            let service = new_policy_service(&d, &account_service, Some(managed), Some(native.clone()), None);
            let definitions: PolicyDefinitions = vec![
                ("enabled".to_owned(), definition(PolicyDefinitionType::Boolean, None, &[("enabled", "boolean")], Some(PolicyValue::Boolean(false)))),
                ("exporterType".to_owned(), definition(PolicyDefinitionType::String, None, &[], Some(PolicyValue::String(String::new())))),
            ];
            service.update_policy_definitions(definitions).await.unwrap();
            let initial_gate = service.gate_info();
            let gate_changes = Rc::new(Cell::new(0usize));
            let counter = gate_changes.clone();
            d.add(service.on_did_change_gate_info().listen(move |_| counter.set(counter.get() + 1)));
            native.hold_update.set(true);
            let settled = Rc::new(Cell::new(false));
            let flag = settled.clone();
            let waiting = when_account_policy_settled(service.clone() as Rc<dyn IAccountPolicyGateService>).then(move |()| flag.set(true));
            if scenario == "pending" {
                provider.set_freshness(pending());
            }
            account_initialized.complete(if scenario == "signedOut" { None } else { Some(base_default_account()) });
            let _ = gate_update_started.promise.clone();
            native.update_started.promise.clone().await.unwrap();
            timeout(0).await.unwrap();
            assert_eq!((settled.get(), service.gate_info()), (false, initial_gate.clone()));

            native.hold_update.set(false);
            if scenario == "inactive" {
                native.set_managed_settings(ManagedSettingsData::new());
                timeout(0).await.unwrap();
                assert!(!settled.get(), "an overlapping update must not complete gate initialization");
            }
            native.update_completed.complete(ManagedSettingsData::new());
            if scenario == "pending" || scenario == "unresolved" {
                timeout(0).await.unwrap();
                assert!(!settled.get());
                let policy_changes = Rc::new(Cell::new(0usize));
                let counter = policy_changes.clone();
                d.add(service.on_did_change().listen(move |_| counter.set(counter.get() + 1)));
                if scenario == "pending" {
                    provider.set_freshness(network_blocked());
                } else {
                    provider.set_policy_data(Some(policy_data_empty()));
                }
                waiting.await.unwrap();
                let values = (service.get_policy_value("enabled"), service.get_policy_value("exporterType"));
                let expected = if scenario == "pending" { (b(false), Some(PolicyValue::String(String::new()))) } else { (None, None) };
                assert_eq!((settled.get(), policy_changes.get(), values), (true, 0, expected), "{scenario}");
            } else {
                waiting.await.unwrap();
                assert_eq!((settled.get(), gate_changes.get(), service.gate_info()), (true, 0, initial_gate), "{scenario}");
            }
        });
    }
}

#[test]
fn policy_settlement_propagates_and_logs_initialization_failures() {
    for failing_service in ["account", "managedSettings"] {
        async_test(move |d| async move {
            let _registry = RegistryGuard::new();
            let failure = Error::new(format!("{failing_service} unavailable"));
            let logged: Rc<RefCell<Vec<(String, Vec<LogArg>)>>> = Rc::new(RefCell::new(Vec::new()));
            let logger: LogServiceRef = Rc::new(RecordingLogService { inner: NullLogService::shared(), logged: logged.clone() });
            let account_initialized: DeferredPromise<Option<IDefaultAccount>> = DeferredPromise::new();
            let inner = new_default_account_service(&d);
            let provider = d.add_rc(TestProvider::new(base_default_account(), None));
            *provider.refresh_override.borrow_mut() = Some(account_initialized.promise.clone());
            inner.set_default_account_provider(provider).unwrap();
            let account_service: Rc<dyn IDefaultAccountService> =
                if failing_service == "account" { Rc::new(FailingDefaultAccountService { inner: inner.clone(), error: failure.clone() }) } else { inner.clone() };
            let native = d.add_rc(FakeNativeManagedSettingsService::new(ManagedSettingsData::new()));
            let service = d.add_rc(AccountPolicyService::new(logger, account_service, None, Some(native.clone() as Rc<dyn INativeManagedSettingsService>), None));
            let definitions: PolicyDefinitions = vec![("enabled".to_owned(), definition(PolicyDefinitionType::Boolean, None, &[("enabled", "boolean")], None))];
            service.update_policy_definitions(definitions).await.unwrap();
            *native.fail_update.borrow_mut() = Some(failure.clone());
            account_initialized.complete(Some(base_default_account()));
            timeout(0).await.unwrap();
            let rejected = when_account_policy_settled(service.clone() as Rc<dyn IAccountPolicyGateService>).await;
            assert_eq!(rejected, Err(failure.clone()));
            assert_eq!(*logged.borrow(), vec![("AccountPolicyService: Failed to initialize account policy".to_owned(), vec![LogArg::Error(failure)])]);
        });
    }
}

#[test]
fn gate_inactive_no_approved_orgs_set_behaves_identically_to_today() {
    async_test(|d| async move {
        let (_f, service, _m) = setup_gate(&d, GateOptions { account: Some(account_in_orgs(&["ApprovedOrg"])), policy_data: Some(Some(policy_data(Some(false), None))), ..gate_default() }).await;
        assert_eq!(service.gate_info().state, AccountPolicyGateState::Inactive);
        assert_eq!(service.get_policy_value("PolicySettingD"), b(false)); // account policy still flows
    });
}

#[test]
fn forced_managed_settings_refresh_blocks_independently_of_approved_account_policy() {
    async_test(|d| async move {
        let freshness = network_blocked();
        let (_f, service, _m) = setup_gate(&d, GateOptions { account: Some(account_in_orgs(&["ApprovedOrg"])), policy_data: Some(Some(policy_data_empty())), freshness: Some(freshness.clone()), ..gate_default() }).await;
        assert_eq!(
            service.gate_info(),
            IAccountPolicyGateInfo {
                state: AccountPolicyGateState::Restricted,
                reason: Some(AccountPolicyGateUnsatisfiedReason::ManagedSettingsRefresh),
                approved_organizations: None,
                managed_settings_freshness: Some(freshness),
            }
        );
        assert_eq!(service.get_policy_value_source("PolicySettingD"), Some(PolicyValueSource::AccountGate));
    });
}

#[test]
fn policy_settlement_resumes_on_pending_to_blocked_without_changed_policy_values() {
    async_test(|d| async move {
        let _registry = RegistryGuard::new();
        let provider = d.add_rc(TestProvider::with_freshness(account_in_orgs(&["ApprovedOrg"]), Some(policy_data_empty()), pending()));
        let account_service = new_default_account_service(&d);
        account_service.set_default_account_provider(provider.clone()).unwrap();
        account_service.refresh(None).await.unwrap();
        let service = new_policy_service(&d, &account_service, None, None, None);
        let definitions: PolicyDefinitions = vec![
            ("enabled".to_owned(), definition(PolicyDefinitionType::Boolean, None, &[], Some(PolicyValue::Boolean(false)))),
            ("exporterType".to_owned(), definition(PolicyDefinitionType::String, None, &[], Some(PolicyValue::String(String::new())))),
        ];
        service.update_policy_definitions(definitions).await.unwrap();
        let values_before = (service.get_policy_value("enabled"), service.get_policy_value("exporterType"));
        let policy_changes = Rc::new(Cell::new(0usize));
        let counter = policy_changes.clone();
        d.add(service.on_did_change().listen(move |_| counter.set(counter.get() + 1)));
        let settled = Rc::new(Cell::new(false));
        let flag = settled.clone();
        let waiting = when_account_policy_settled(service.clone() as Rc<dyn IAccountPolicyGateService>).then(move |()| flag.set(true));
        timeout(0).await.unwrap();
        assert!(!settled.get());

        provider.set_freshness(network_blocked());
        waiting.await.unwrap();
        let values_after = (service.get_policy_value("enabled"), service.get_policy_value("exporterType"));
        let expected = (b(false), Some(PolicyValue::String(String::new())));
        assert_eq!((settled.get(), policy_changes.get(), values_before, values_after), (true, 0, expected.clone(), expected));
    });
}

#[test]
fn gate_active_no_account_signed_in_restricted() {
    async_test(|d| async move {
        let (_f, service, _m) = setup_gate(&d, GateOptions { approved_orgs: orgs(&["ApprovedOrg"]), ..gate_default() }).await;
        assert_eq!(service.gate_info().state, AccountPolicyGateState::Restricted);
        assert_eq!(service.gate_info().reason, Some(AccountPolicyGateUnsatisfiedReason::NoAccount));
        // Restricted values applied to policies that opt into the gate.
        // PolicySettingD has a `value` callback: falls back to type-default `false`.
        assert_eq!(service.get_policy_value("PolicySettingD"), b(false));
        assert_eq!(service.get_policy_value_source("PolicySettingD"), Some(PolicyValueSource::AccountGate));
        // PolicySettingA does NOT opt in (no `value`, no `restrictedValue`): unchanged.
        assert_eq!(service.get_policy_value("PolicySettingA"), None);
    });
}

#[test]
fn gate_active_signed_in_but_org_not_approved_restricted() {
    async_test(|d| async move {
        let (_f, service, _m) = setup_gate(&d, GateOptions { approved_orgs: orgs(&["ApprovedOrg"]), account: Some(account_in_orgs(&["SomeOtherOrg"])), policy_data: Some(Some(policy_data_empty())), ..gate_default() }).await;
        assert_eq!(service.gate_info().state, AccountPolicyGateState::Restricted);
        assert_eq!(service.gate_info().reason, Some(AccountPolicyGateUnsatisfiedReason::OrgNotApproved));
    });
}

#[test]
fn gate_active_account_in_approved_org_but_policy_data_null_pre_resolution_restricted() {
    async_test(|d| async move {
        let (_f, service, _m) = setup_gate(&d, GateOptions { approved_orgs: orgs(&["approvedorg"]), account: Some(account_in_orgs(&["ApprovedOrg"])), policy_data: Some(None), ..gate_default() }).await;
        assert_eq!(service.gate_info().state, AccountPolicyGateState::Restricted);
        assert_eq!(service.gate_info().reason, Some(AccountPolicyGateUnsatisfiedReason::PolicyNotResolved));
    });
}

#[test]
fn gate_active_satisfied_case_insensitive_org_match_account_policy_values_flow_normally() {
    async_test(|d| async move {
        let (_f, service, _m) = setup_gate(
            &d,
            GateOptions { approved_orgs: orgs(&[" approvedorg ", " Other "]), account: Some(account_in_orgs(&["ApprovedOrg"])), policy_data: Some(Some(policy_data(Some(false), None))), ..gate_default() },
        )
        .await;
        assert_eq!(service.gate_info().state, AccountPolicyGateState::Satisfied);
        assert_eq!(service.get_policy_value("PolicySettingD"), b(false)); // from account policy data, not restricted
        assert_eq!(service.get_policy_value_source("PolicySettingD"), Some(PolicyValueSource::Account));
        assert_eq!(service.get_policy_value("PolicySettingA"), None); // not driven by account
    });
}

#[test]
fn gate_active_wildcard_satisfies_any_signed_in_account() {
    async_test(|d| async move {
        let (_f, service, _m) = setup_gate(&d, GateOptions { approved_orgs: orgs(&["*"]), account: Some(account_in_orgs(&["SomeOtherOrg"])), policy_data: Some(Some(policy_data_empty())), ..gate_default() }).await;
        assert_eq!(service.gate_info().state, AccountPolicyGateState::Satisfied);
    });
}

#[test]
fn approved_org_list_empty_gate_inactive() {
    async_test(|d| async move {
        let (_f, service, _m) = setup_gate(&d, GateOptions { approved_orgs: orgs(&[]), account: Some(account_in_orgs(&["ApprovedOrg"])), policy_data: Some(Some(policy_data_empty())), ..gate_default() }).await;
        assert_eq!(service.gate_info().state, AccountPolicyGateState::Inactive);
    });
}

#[test]
fn approved_orgs_raw_non_array_string_from_policy_service_gate_inactive_fail_safe() {
    async_test(|d| async move {
        // Defensive: if some platform delivers the policy as a non-JSON string, treat it as no-orgs
        // rather than half-parsing CSV.
        let (_f, service, _m) = setup_gate(
            &d,
            GateOptions { approved_orgs: Some(PolicyValue::String("github".to_owned())), account: Some(account_in_orgs(&["ApprovedOrg"])), policy_data: Some(Some(policy_data_empty())), ..gate_default() },
        )
        .await;
        assert_eq!(service.gate_info().state, AccountPolicyGateState::Inactive);
    });
}

#[test]
fn gate_active_signed_in_with_non_github_provider_wrong_provider_reason() {
    async_test(|d| async move {
        // Custom provider whose configured GitHub provider differs from the account's actual provider.
        let _registry = RegistryGuard::new();
        configuration_registry().register_configuration(policy_configuration_node());
        let mut account = account_in_orgs(&["ApprovedOrg"]);
        account.authentication_provider = auth_provider("microsoft", "Microsoft");
        let managed = d.add_rc(FakeManagedPolicyService::new());
        managed.set_policy(APPROVED_ACCOUNT_ORGANIZATIONS_POLICY_NAME, orgs(&["ApprovedOrg"]));
        let account_service = new_default_account_service(&d);
        install(&d, &account_service, TestProvider::mismatched(account, auth_provider("github", "GitHub")));
        account_service.refresh(None).await.unwrap();
        let service = new_policy_service(&d, &account_service, Some(managed), None, None);
        let config = new_policy_configuration(&d, &service);
        config.initialize().await.unwrap();

        assert_eq!(service.gate_info().state, AccountPolicyGateState::Restricted);
        assert_eq!(service.gate_info().reason, Some(AccountPolicyGateUnsatisfiedReason::WrongProvider));
    });
}

#[test]
fn explicit_restricted_value_is_honored_when_gate_is_restricted() {
    async_test(|d| async move {
        let registry = RegistryGuard::new();
        let node = node_full(
            "restrictedValueConfig",
            2.0,
            "r",
            vec![("setting.RV", policy_property("string", s("open"), "PolicySettingRV", None, &[], Some(s("locked"))))],
        );
        configuration_registry().register_configuration(node.clone());
        let (_f, service, _m) = setup_gate(&d, GateOptions { approved_orgs: orgs(&["ApprovedOrg"]), ..gate_default() }).await;
        assert_eq!(service.gate_info().state, AccountPolicyGateState::Restricted);
        assert_eq!(service.get_policy_value("PolicySettingRV"), Some(PolicyValue::String("locked".to_owned())));
        configuration_registry().deregister_configurations(&[node]);
        drop(registry);
    });
}

#[test]
fn on_did_change_gate_info_fires_on_state_reason_transitions() {
    async_test(|d| async move {
        let (_f, service, managed) = setup_gate(&d, GateOptions { approved_orgs: orgs(&["ApprovedOrg"]), account: Some(account_in_orgs(&["ApprovedOrg"])), policy_data: Some(Some(policy_data_empty())), ..gate_default() }).await;
        assert_eq!(service.gate_info().state, AccountPolicyGateState::Satisfied);

        let events: Rc<RefCell<Vec<(AccountPolicyGateState, Option<AccountPolicyGateUnsatisfiedReason>)>>> = Rc::new(RefCell::new(Vec::new()));
        let sink = events.clone();
        d.add(service.on_did_change_gate_info().listen(move |info| sink.borrow_mut().push((info.state, info.reason))));

        // Satisfied -> Restricted (org no longer approved)
        managed.set_policy(APPROVED_ACCOUNT_ORGANIZATIONS_POLICY_NAME, orgs(&["OnlyOtherOrg"]));
        timeout(0).await.unwrap();
        // Restricted -> Inactive (gate disabled)
        managed.set_policy(APPROVED_ACCOUNT_ORGANIZATIONS_POLICY_NAME, orgs(&[]));
        timeout(0).await.unwrap();

        assert_eq!(
            *events.borrow(),
            vec![(AccountPolicyGateState::Restricted, Some(AccountPolicyGateUnsatisfiedReason::OrgNotApproved)), (AccountPolicyGateState::Inactive, None)]
        );
    });
}

#[test]
fn boot_race_gate_is_fail_closed_until_async_managed_policy_service_resolves() {
    async_test(|d| async move {
        // Simulate the IPC boundary: managed service only knows about its policies AFTER
        // `updatePolicyDefinitions` has been called by the MultiplexPolicyService.
        let _registry = RegistryGuard::new();
        configuration_registry().register_configuration(policy_configuration_node());
        let managed = d.add_rc(FakeManagedPolicyService::gated());
        let account_service = new_default_account_service(&d);
        install(&d, &account_service, TestProvider::new(account_in_orgs(&["ApprovedOrg"]), Some(policy_data_empty())));
        account_service.refresh(None).await.unwrap();

        let service = new_policy_service(&d, &account_service, Some(managed.clone()), None, None);
        let config = new_policy_configuration(&d, &service);
        config.initialize().await.unwrap();

        // Before managed service resolves, the gate sees no approved-org policy -> Inactive.
        assert_eq!(service.gate_info().state, AccountPolicyGateState::Inactive);

        // Simulate the multiplex seeding the managed service (IPC completes).
        managed.seed(APPROVED_ACCOUNT_ORGANIZATIONS_POLICY_NAME, orgs(&["OnlyOtherOrg"]).unwrap()).await;
        timeout(0).await.unwrap();

        // Gate must now reflect the admin policy; account is NOT in 'OnlyOtherOrg'.
        assert_eq!(service.gate_info().state, AccountPolicyGateState::Restricted);
        assert_eq!(service.gate_info().reason, Some(AccountPolicyGateUnsatisfiedReason::OrgNotApproved));
    });
}

#[test]
fn managed_policy_change_re_evaluates_the_gate_and_fires_on_did_change_for_a_source_only_change() {
    async_test(|d| async move {
        let (_f, service, managed) = setup_gate(
            &d,
            GateOptions { approved_orgs: orgs(&["ApprovedOrg"]), account: Some(account_in_orgs(&["ApprovedOrg"])), policy_data: Some(Some(policy_data(Some(false), None))), ..gate_default() },
        )
        .await;
        assert_eq!(service.gate_info().state, AccountPolicyGateState::Satisfied);
        assert_eq!(service.get_policy_value_source("PolicySettingD"), Some(PolicyValueSource::Account));

        let changes: Rc<RefCell<Vec<String>>> = Rc::new(RefCell::new(Vec::new()));
        let sink = changes.clone();
        d.add(service.on_did_change().listen(move |names| sink.borrow_mut().extend(names.iter().cloned())));

        // Change the approved-org list to one the account is NOT in: flip Satisfied -> Restricted,
        // which forces restricted values onto opted-in policies and emits onDidChange.
        managed.set_policy(APPROVED_ACCOUNT_ORGANIZATIONS_POLICY_NAME, orgs(&["OnlyOtherOrg"]));
        // `_updatePolicyDefinitions` is async: wait one turn for it to resolve.
        timeout(0).await.unwrap();

        assert_eq!(service.gate_info().state, AccountPolicyGateState::Restricted);
        assert_eq!(service.get_policy_value_source("PolicySettingD"), Some(PolicyValueSource::AccountGate));
        assert!(changes.borrow().iter().any(|name| name == "PolicySettingD"), "expected onDidChange to fire for the source-only change");
    });
}
