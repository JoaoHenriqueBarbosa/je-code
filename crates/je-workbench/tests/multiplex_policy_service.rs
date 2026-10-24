//! Porte de `src/vs/workbench/services/policies/test/browser/multiplexPolicyService.test.ts` (suíte `MultiplexPolicyService`), caso a caso e na mesma ordem.
//!
//! Divergências (as mesmas de `account_policy_service.rs`):
//! - Todo caso roda sob o relógio virtual (`async_test`, que também cria o guarda de vazamentos e o `disposables`). O
//!   `suiteSetup`/`suiteTeardown` (registrar e desregistrar o nó de configuração) e o `setup` são feitos por caso, no
//!   [`setup`] (o registro global é um singleton por thread e cada caso roda no seu).
//! - `new DefaultAccountService(TestProductService)` é `DefaultAccountService::new(provedor padrão)`.
//! - Os callbacks `value` das políticas são [`policy_value_dynamic`] (a função dinâmica do `PolicyDefinition`).
//! - O `DefaultAccountProvider` do teste é o [`TestProvider`]: os eventos que o original deixa em `Event.None` nunca disparam.
//! - `assert.deepStrictEqual(C, undefined)` e `assert.strictEqual(x, undefined)` sobre valores de configuração são
//!   `Dynamic::Undefined`; sobre valores de política, `None`.

#[path = "../../je-platform/tests/common/mod.rs"]
mod common;

use std::cell::RefCell;
use std::rc::Rc;

use common::files::async_test;
use common::{RegistryGuard, assert_deep_strict_equal, j, node_full, s, uri_file};
use je_base::buffer::VSBuffer;
use je_base::default_account::{ICopilotTokenInfo, IDefaultAccount, IDefaultAccountAuthenticationProvider, IPolicyData};
use je_base::dynamic::Dynamic;
use je_base::event::{Emitter, Event};
use je_base::lifecycle::{DisposableStore, IDisposable};
use je_base::policy::{PolicyValue as BaseValue, PolicyValueFn};
use je_base::product::IChatProvider;
use je_base::promise::Promise;
use je_base::uri::{Field, URI, UriChange};
use je_platform::configuration::defaults::DefaultConfiguration;
use je_platform::configuration::policy_configuration::PolicyConfiguration;
use je_platform::configuration::registry::{ConfigurationNode, PropertySchema, configuration_registry};
use je_platform::files::file_service::FileService;
use je_platform::files::i_file_service::IFileService;
use je_platform::files::in_memory::InMemoryFileSystemProvider;
use je_platform::log::NullLogService;
use je_platform::policy::managed_settings_freshness::IManagedSettingsFreshness;
use je_platform::policy::{FilePolicyService, PolicyService, PolicyValue, PolicyValueSource};
use je_platform::policy::multiplex_policy_service::MultiplexPolicyService;
use je_workbench::services::accounts::default_account::{
    DefaultAccountService, IDefaultAccountProvider, IDefaultAccountRefreshOptions, IDefaultAccountService, ISignInOptions, ManagedSettingsFetchStatus,
};
use je_workbench::services::accounts::managed_settings::IManagedSettingsCompatibilityError;
use je_workbench::services::policies::account_policy_service::AccountPolicyService;
use je_workbench::services::policies::policy_value_callback::{empty_policy_data, policy_value_dynamic};

/// `BASE_DEFAULT_ACCOUNT`.
fn base_default_account() -> IDefaultAccount {
    IDefaultAccount {
        authentication_provider: IDefaultAccountAuthenticationProvider { id: "github".to_owned(), name: "GitHub".to_owned(), enterprise: false, extra: Default::default() },
        account_name: "testuser".to_owned(),
        session_id: "abc123".to_owned(),
        enterprise: false,
        entitlements_data: None,
        extra: Default::default(),
    }
}

/// `class DefaultAccountProvider implements IDefaultAccountProvider`.
struct TestProvider {
    default_account: IDefaultAccount,
    policy_data: IPolicyData,
    on_did_change_default_account: Rc<Emitter<Option<IDefaultAccount>>>,
    on_did_change_policy_data: Rc<Emitter<Option<IPolicyData>>>,
    on_did_change_copilot_token_info: Rc<Emitter<Option<ICopilotTokenInfo>>>,
    on_did_change_compatibility_error: Rc<Emitter<Option<IManagedSettingsCompatibilityError>>>,
    on_did_change_freshness: Rc<Emitter<IManagedSettingsFreshness>>,
}

impl TestProvider {
    /// `new DefaultAccountProvider(defaultAccount, policyData = {})`.
    fn new(default_account: IDefaultAccount, policy_data: IPolicyData) -> Rc<Self> {
        Rc::new(Self {
            default_account,
            policy_data,
            on_did_change_default_account: Emitter::new(),
            on_did_change_policy_data: Emitter::new(),
            on_did_change_copilot_token_info: Emitter::new(),
            on_did_change_compatibility_error: Emitter::new(),
            on_did_change_freshness: Emitter::new(),
        })
    }
}

impl IDefaultAccountProvider for TestProvider {
    fn default_account(&self) -> Option<IDefaultAccount> {
        Some(self.default_account.clone())
    }
    fn on_did_change_default_account(&self) -> Event<Option<IDefaultAccount>> {
        self.on_did_change_default_account.event()
    }
    fn policy_data(&self) -> Option<IPolicyData> {
        Some(self.policy_data.clone())
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
        IManagedSettingsFreshness::NotRequired
    }
    fn on_did_change_managed_settings_freshness(&self) -> Event<IManagedSettingsFreshness> {
        self.on_did_change_freshness.event()
    }
    fn get_default_account_authentication_provider(&self) -> IDefaultAccountAuthenticationProvider {
        self.default_account.authentication_provider.clone()
    }
    fn resolve_git_hub_url(&self, path: &str) -> Option<String> {
        Some(format!("https://github.com/{path}"))
    }
    fn refresh(&self, _options: Option<IDefaultAccountRefreshOptions>) -> Promise<Option<IDefaultAccount>> {
        Promise::resolved(Some(self.default_account.clone()))
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

/// Uma propriedade com política: `{ type, default, policy: { name, category, minimumVersion, localization, value? } }`.
fn policy_property(type_: Dynamic, default: Dynamic, name: &str, value: PolicyValueFn) -> PropertySchema {
    let policy = Dynamic::object(vec![
        ("name", s(name)),
        ("category", s("Extensions")),
        ("minimumVersion", s("1.0.0")),
        ("localization", j("{ 'description': { 'key': '', 'value': '' } }")),
        ("value", policy_value_dynamic(value)),
    ]);
    PropertySchema::from_entries(vec![("type", type_), ("default", default), ("policy", policy)])
}

/// A política sem `value`: o nome é o único campo de política além da categoria e da localização.
fn plain_policy_property(type_: &str, default: Dynamic, name: &str) -> PropertySchema {
    let policy = Dynamic::object(vec![
        ("name", s(name)),
        ("category", s("Extensions")),
        ("minimumVersion", s("1.0.0")),
        ("localization", j("{ 'description': { 'key': '', 'value': '' } }")),
    ]);
    PropertySchema::from_entries(vec![("type", s(type_)), ("default", default), ("policy", policy)])
}

/// `policyData => policyData.chat_preview_features_enabled === false ? payload : undefined`.
fn preview_disabled(payload: BaseValue) -> PolicyValueFn {
    PolicyValueFn::new(move |data| (data.chat_preview_features_enabled == Some(false)).then(|| payload.clone()))
}

/// O `policyConfigurationNode`.
fn policy_configuration_node() -> ConfigurationNode {
    node_full(
        "policyConfiguration",
        1.0,
        "a",
        vec![
            ("setting.A", plain_policy_property("string", s("defaultValueA"), "PolicySettingA")),
            ("setting.B", policy_property(s("string"), s("defaultValueB"), "PolicySettingB", preview_disabled(BaseValue::String("policyValueB".to_owned())))),
            (
                "setting.C",
                policy_property(s("array"), j("['defaultValueC1', 'defaultValueC2']"), "PolicySettingC", preview_disabled(BaseValue::String("[\"policyValueC1\",\"policyValueC2\"]".to_owned()))),
            ),
            ("setting.D", policy_property(s("boolean"), Dynamic::Bool(true), "PolicySettingD", preview_disabled(BaseValue::Boolean(false)))),
            ("setting.E", PropertySchema::from_entries(vec![("type", s("boolean")), ("default", Dynamic::Bool(true))])),
            (
                "setting.F",
                policy_property(
                    s("boolean"),
                    Dynamic::Bool(true),
                    "PolicySettingF",
                    PolicyValueFn::new(|data| (data.cloud_session_storage_enabled == Some(false)).then_some(BaseValue::Boolean(false))),
                ),
            ),
            (
                "setting.G",
                policy_property(j("['array', 'null']"), Dynamic::Null, "PolicySettingG", preview_disabled(BaseValue::String("[\"policyValueG1\",\"policyValueG2\"]".to_owned()))),
            ),
            ("setting.H", policy_property(j("['array', 'null']"), Dynamic::Null, "PolicySettingH", preview_disabled(BaseValue::String("[]".to_owned())))),
        ],
    )
}

/// O que o `setup` monta e os casos usam.
struct Fixture {
    _registry: RegistryGuard,
    file_service: Rc<dyn IFileService>,
    default_account_service: Rc<DefaultAccountService>,
    policy_service: Rc<MultiplexPolicyService>,
    policy_configuration: Rc<PolicyConfiguration>,
    policy_file: URI,
    disposables: Rc<DisposableStore>,
}

/// `const policyFile = URI.file('policyFile').with({ scheme: 'vscode-tests' })`.
fn policy_file() -> URI {
    uri_file("policyFile").with(&UriChange { scheme: Field::from("vscode-tests"), ..UriChange::default() })
}

/// O `suiteSetup` e o `setup`.
fn setup(d: &Rc<DisposableStore>) -> Fixture {
    let registry = RegistryGuard::new();
    configuration_registry().register_configuration(policy_configuration_node());

    let default_configuration = d.add_rc(DefaultConfiguration::new(NullLogService::shared()));
    default_configuration.initialize();

    let policy_file = policy_file();
    let file_service = d.add_rc(FileService::new(NullLogService::shared()));
    let disk_file_system_provider = d.add_rc(InMemoryFileSystemProvider::new());
    d.add(file_service.register_provider("vscode-tests", disk_file_system_provider).unwrap());
    let file_service: Rc<dyn IFileService> = file_service;

    let default_account_service = d.add_rc(DefaultAccountService::new(IChatProvider { id: "github".to_owned(), name: "GitHub".to_owned(), extra: Default::default() }));
    let file_policy_service = d.add_rc(FilePolicyService::new(policy_file.clone(), file_service.clone(), NullLogService::shared()));
    let account_policy_service = d.add_rc(AccountPolicyService::new(NullLogService::shared(), default_account_service.clone() as Rc<dyn IDefaultAccountService>, None, None, None));
    let policy_service = d.add_rc(MultiplexPolicyService::new(vec![file_policy_service as Rc<dyn PolicyService>, account_policy_service as Rc<dyn PolicyService>], NullLogService::shared()));
    let policy_configuration = d.add_rc(PolicyConfiguration::new(default_configuration, policy_service.clone() as Rc<dyn PolicyService>, NullLogService::shared()));
    Fixture { _registry: registry, file_service, default_account_service, policy_service, policy_configuration, policy_file, disposables: d.clone() }
}

impl Fixture {
    /// `fileService.writeFile(policyFile, VSBuffer.fromString(content))`.
    async fn write_policy_file(&self, content: &str) {
        self.file_service.write_file(&self.policy_file, VSBuffer::from_string(content).into(), None).await.unwrap();
    }

    /// `clear()`.
    async fn clear(&self) {
        self.write_policy_file("{}").await;
    }

    /// `defaultAccountService.setDefaultAccountProvider(new DefaultAccountProvider(BASE_DEFAULT_ACCOUNT, policyData))` e `refresh()`.
    async fn set_account(&self, policy_data: IPolicyData) {
        let provider = self.disposables.add_rc(TestProvider::new(base_default_account(), policy_data));
        self.default_account_service.set_default_account_provider(provider).unwrap();
        self.default_account_service.refresh(None).await.unwrap();
    }

    /// `policyService.getPolicyValue(name)`.
    fn policy_value(&self, name: &str) -> Option<PolicyValue> {
        self.policy_service.get_policy_value(name)
    }

    /// `policyConfiguration.configurationModel.getValue(key)`.
    fn configuration_value(&self, key: &str) -> Dynamic {
        self.policy_configuration.configuration_model().get_value(key)
    }
}

/// `{ chat_preview_features_enabled: false }`.
fn preview_disabled_data() -> IPolicyData {
    let mut data = empty_policy_data();
    data.chat_preview_features_enabled = Some(false);
    data
}

fn text(value: &str) -> Option<PolicyValue> {
    Some(PolicyValue::String(value.to_owned()))
}

#[test]
fn no_policy() {
    async_test(|d| async move {
        let f = setup(&d);
        f.clear().await;

        f.policy_configuration.initialize().await.unwrap();

        {
            // No policy is set
            assert_eq!(f.policy_value("PolicySettingA"), None);
            assert_eq!(f.policy_value("PolicySettingB"), None);
            assert_eq!(f.policy_value("PolicySettingC"), None);
            assert_eq!(f.policy_value("PolicySettingD"), None);
        }

        {
            assert_eq!(f.configuration_value("setting.A"), Dynamic::Undefined);
            assert_eq!(f.configuration_value("setting.B"), Dynamic::Undefined);
            assert_deep_strict_equal(&f.configuration_value("setting.C"), &Dynamic::Undefined);
            assert_eq!(f.configuration_value("setting.D"), Dynamic::Undefined);
            assert_eq!(f.configuration_value("setting.E"), Dynamic::Undefined);
        }
    });
}

#[test]
fn policy_from_file_only() {
    async_test(|d| async move {
        let f = setup(&d);
        f.clear().await;

        f.set_account(empty_policy_data()).await;

        f.write_policy_file("{\"PolicySettingA\":\"policyValueA\"}").await;

        f.policy_configuration.initialize().await.unwrap();

        {
            assert_eq!(f.policy_value("PolicySettingA"), text("policyValueA"));
            assert_eq!(f.policy_service.get_policy_value_source("PolicySettingA"), Some(PolicyValueSource::Device));
            assert_eq!(f.policy_value("PolicySettingB"), None);
            assert_eq!(f.policy_value("PolicySettingC"), None);
            assert_eq!(f.policy_value("PolicySettingD"), None);
        }

        {
            assert_eq!(f.configuration_value("setting.A"), s("policyValueA"));
            assert_eq!(f.configuration_value("setting.B"), Dynamic::Undefined);
            assert_deep_strict_equal(&f.configuration_value("setting.C"), &Dynamic::Undefined);
            assert_eq!(f.configuration_value("setting.D"), Dynamic::Undefined);
            assert_eq!(f.configuration_value("setting.E"), Dynamic::Undefined);
        }
    });
}

#[test]
fn policy_from_default_account_only() {
    async_test(|d| async move {
        let f = setup(&d);
        f.clear().await;

        f.set_account(preview_disabled_data()).await;

        f.write_policy_file("{}").await;

        f.policy_configuration.initialize().await.unwrap();
        let actual_configuration_model = f.policy_configuration.configuration_model();

        {
            assert_eq!(f.policy_value("PolicySettingA"), None); // Not tagged with preview tags
            assert_eq!(f.policy_value("PolicySettingB"), text("policyValueB"));
            assert_eq!(f.policy_value("PolicySettingC"), text("[\"policyValueC1\",\"policyValueC2\"]"));
            assert_eq!(f.policy_value("PolicySettingD"), Some(PolicyValue::Boolean(false)));
        }

        {
            assert_eq!(f.configuration_value("setting.A"), Dynamic::Undefined);
            assert_eq!(actual_configuration_model.get_value("setting.B"), s("policyValueB"));
            assert_deep_strict_equal(&actual_configuration_model.get_value("setting.C"), &j("['policyValueC1', 'policyValueC2']"));
            assert_eq!(actual_configuration_model.get_value("setting.D"), Dynamic::Bool(false));
        }
    });
}

#[test]
fn policy_from_file_and_default_account() {
    async_test(|d| async move {
        let f = setup(&d);
        f.clear().await;

        f.set_account(preview_disabled_data()).await;

        f.write_policy_file("{\"PolicySettingA\":\"policyValueA\",\"PolicySettingD\":false}").await;

        f.policy_configuration.initialize().await.unwrap();
        let actual_configuration_model = f.policy_configuration.configuration_model();

        {
            assert_eq!(f.policy_value("PolicySettingA"), text("policyValueA"));
            assert_eq!(f.policy_value("PolicySettingB"), text("policyValueB"));
            assert_eq!(f.policy_value("PolicySettingC"), text("[\"policyValueC1\",\"policyValueC2\"]"));
            assert_eq!(f.policy_value("PolicySettingD"), Some(PolicyValue::Boolean(false)));
            assert_eq!(f.policy_service.get_policy_value_source("PolicySettingA"), Some(PolicyValueSource::Device));
            assert_eq!(f.policy_service.get_policy_value_source("PolicySettingD"), Some(PolicyValueSource::Account));
        }

        {
            assert_eq!(actual_configuration_model.get_value("setting.A"), s("policyValueA"));
            assert_eq!(actual_configuration_model.get_value("setting.B"), s("policyValueB"));
            assert_deep_strict_equal(&actual_configuration_model.get_value("setting.C"), &j("['policyValueC1', 'policyValueC2']"));
            assert_eq!(actual_configuration_model.get_value("setting.D"), Dynamic::Bool(false));
        }
    });
}

#[test]
fn cloud_session_storage_enabled_policy_disabled_overrides_setting() {
    async_test(|d| async move {
        let f = setup(&d);
        f.clear().await;

        let mut policy_data = empty_policy_data();
        policy_data.cloud_session_storage_enabled = Some(false);
        f.set_account(policy_data).await;

        f.policy_configuration.initialize().await.unwrap();

        assert_eq!(f.policy_value("PolicySettingF"), Some(PolicyValue::Boolean(false)));
        assert_eq!(f.configuration_value("setting.F"), Dynamic::Bool(false));
    });
}

#[test]
fn cloud_session_storage_enabled_policy_enabled_does_not_override_setting() {
    async_test(|d| async move {
        let f = setup(&d);
        f.clear().await;

        let mut policy_data = empty_policy_data();
        policy_data.cloud_session_storage_enabled = Some(true);
        f.set_account(policy_data).await;

        f.policy_configuration.initialize().await.unwrap();

        assert_eq!(f.policy_value("PolicySettingF"), None);
        assert_eq!(f.configuration_value("setting.F"), Dynamic::Undefined);
    });
}

#[test]
fn cloud_session_storage_enabled_policy_unset_does_not_override_setting() {
    async_test(|d| async move {
        let f = setup(&d);
        f.clear().await;

        f.set_account(empty_policy_data()).await;

        f.policy_configuration.initialize().await.unwrap();

        assert_eq!(f.policy_value("PolicySettingF"), None);
        assert_eq!(f.configuration_value("setting.F"), Dynamic::Undefined);
    });
}

#[test]
fn union_typed_array_or_null_policy_registers_and_parses_json_string_value() {
    async_test(|d| async move {
        let f = setup(&d);
        f.clear().await;

        f.set_account(preview_disabled_data()).await;

        f.policy_configuration.initialize().await.unwrap();

        assert_eq!(f.policy_value("PolicySettingG"), text("[\"policyValueG1\",\"policyValueG2\"]"));
        assert_deep_strict_equal(&f.configuration_value("setting.G"), &j("['policyValueG1', 'policyValueG2']"));
    });
}

#[test]
fn union_typed_array_or_null_policy_preserves_an_empty_array_lockdown_distinct_from_unset() {
    async_test(|d| async move {
        let f = setup(&d);
        f.clear().await;

        // Policy set to an empty array (e.g. a lockdown allowlist): must round-trip to `[]`, not `undefined`.
        f.set_account(preview_disabled_data()).await;
        f.policy_configuration.initialize().await.unwrap();

        assert_eq!(f.policy_value("PolicySettingH"), text("[]"));
        assert_deep_strict_equal(&f.configuration_value("setting.H"), &j("[]"));
    });
}

#[test]
fn union_typed_array_or_null_policy_unset_leaves_the_setting_at_its_default_distinct_from_empty_array() {
    async_test(|d| async move {
        let f = setup(&d);
        f.clear().await;

        f.set_account(empty_policy_data()).await;
        f.policy_configuration.initialize().await.unwrap();

        assert_eq!(f.policy_value("PolicySettingH"), None);
        assert_eq!(f.configuration_value("setting.H"), Dynamic::Undefined);
    });
}
