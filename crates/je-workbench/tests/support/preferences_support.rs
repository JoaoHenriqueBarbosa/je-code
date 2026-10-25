//! Infraestrutura comum aos testes de `contrib/preferences`: `settingsTreeModels.test.ts`, `settingsEditor2.test.ts` e
//! `settingsTree.test.ts`. Os três montam os mesmos serviços (`TestConfigurationService` com `isSettingAppliedForAllProfiles`, o
//! `IManagedSettingsService` parcial sobre um objeto mutável, o `ILanguageService` parcial) e registram o mesmo nó de configuração
//! no `suiteSetup`, então o que era repetido vive aqui.
//!
//! Divergências:
//! - **`ensureNoDisposablesAreLeakedInTestSuite()`.** O módulo `common` de `je-platform/tests` (o guarda de vazamentos, os
//!   construtores de nós e de propriedades) é declarado aqui, porque o `support/mod.rs` é compilado em várias raízes de teste e
//!   nenhuma delas pode ser obrigada a ter o seu. Os testes de preferências importam tudo de [`common`] por este módulo, para
//!   o [`common::LeakGuard`] ser um tipo só.
//! - **`mock<T>()`.** Cada dublê implementa só o que os casos usam, e todo outro membro falha como o `undefined` do original
//!   falharia (ver [`absent`]).
//! - **Subclasse de `TestConfigurationService`.** `isSettingAppliedForAllProfiles() { return false; }` e a sobrescrita de `inspect`
//!   são o [`TestSettingsConfiguration`], que delega ao `TestConfigurationService` (`stub_inspect_with` é a sobrescrita).
//! - **`managedSettings[key] = value`, `delete managedSettings[key]`.** São [`ManagedStore::set`] e [`ManagedStore::delete`]; atribuir
//!   `undefined` é gravar `None`.
#![allow(dead_code)]

#[path = "../../../je-platform/tests/common/mod.rs"]
pub mod common;

use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

use common::{LeakGuard, node};
use je_base::JsString;
use je_base::dynamic::{Dynamic, Truthy};
use je_base::event::{Emitter, Event};
use je_base::lifecycle::DisposableRef;
use je_base::policy::{ManagedSettingValue, ManagedSettingsData, PolicyValue};
use je_base::promise::Promise;
use je_base::uri::URI;
use je_editor::language_service::{ILanguageSelection, ILanguageService};
use je_editor::languages::ILanguageIdCodec;
use je_editor::languages_registry::LanguageNameIdPair;
use je_editor::modes_registry::{LanguageExtensionPoint, LanguageIcon};
use je_platform::configuration::models::{ConfigurationChangeEvent, ConfigurationKeys};
use je_platform::configuration::registry::{ConfigurationNode, PropertySchema, configuration_registry};
use je_platform::configuration::service::IConfigurationService;
use je_platform::configuration::test_configuration_service::{TestConfigurationService, TestConfigurationValue};
use je_platform::configuration::{ConfigurationData, ConfigurationOverrides, ConfigurationTarget, ConfigurationUpdateOverrides, ConfigurationValue};
use je_platform::policy::copilot_managed_settings::{IManagedSettingsService, NullManagedSettingsService};
use je_platform::workspace::AnyWorkspaceIdentifier;
use je_workbench::contrib::chat::chat_network_configuration::chat_network_domain_configuration_properties;
use je_workbench::contrib::preferences::preferences_widgets::SettingsTarget;
use je_workbench::contrib::preferences::settings_layout::ITOCEntry;
use je_workbench::contrib::preferences::settings_tree_models::{ISettingsEditorViewState, SettingsTreeModel, SettingsTreeServices, SettingsTreeSettingElement, ViewStateRef};
use je_workbench::contrib::terminal_contrib::chat_agent_tools::terminal_chat_agent_tools_configuration::terminal_chat_agent_tools_managed_presentation_properties;
use je_workbench::services::configuration::common::configuration::{IWorkbenchConfigurationService, RestrictedSettings};
use je_workbench::services::configuration::common::experimental_settings::ExperimentalSettingsService;
use je_workbench::services::configuration::common::managed_settings_presentation::ManagedSettingsPresentationService;
use je_workbench::services::preferences::preferences::{
    ISearchResult, ISetting, ISettingMatch, ISettingsGroup, ISettingsSection, SettingMatchType, SettingSchemaType,
};
use je_workbench::services::preferences::preferences_models::NULL_RANGE;
use je_workbench::test::browser::workbench_test_services::{EnvironmentOverrides, TestEnvironmentService};
use je_workbench::test::common::workbench_test_services::{TestProductService, TestUserDataProfileService};

/// Um membro que o dublê parcial não tem (o `undefined` do original, chamado como função).
pub fn absent(method: &str) -> ! {
    panic!("`{method}` não existe no stub parcial")
}

/// `{ isRegisteredLanguageId: () => true }`: o resto não existe no objeto do original.
pub struct StubLanguageService;

impl ILanguageService for StubLanguageService {
    fn language_id_codec(&self) -> Rc<dyn ILanguageIdCodec> {
        absent("languageIdCodec")
    }
    fn on_did_request_basic_language_features(&self) -> Event<String> {
        absent("onDidRequestBasicLanguageFeatures")
    }
    fn on_did_request_rich_language_features(&self) -> Event<String> {
        absent("onDidRequestRichLanguageFeatures")
    }
    fn on_did_change(&self) -> Event<()> {
        absent("onDidChange")
    }
    fn register_language(&self, _def: LanguageExtensionPoint) -> DisposableRef {
        absent("registerLanguage")
    }
    fn is_registered_language_id(&self, _language_id: Option<&str>) -> bool {
        true
    }
    fn get_registered_language_ids(&self) -> Vec<String> {
        absent("getRegisteredLanguageIds")
    }
    fn get_sorted_registered_language_names(&self) -> Vec<LanguageNameIdPair> {
        absent("getSortedRegisteredLanguageNames")
    }
    fn get_language_name(&self, _language_id: &str) -> Option<String> {
        absent("getLanguageName")
    }
    fn get_mime_type(&self, _language_id: &str) -> Option<String> {
        absent("getMimeType")
    }
    fn get_icon(&self, _language_id: &str) -> Option<LanguageIcon> {
        absent("getIcon")
    }
    fn get_extensions(&self, _language_id: &str) -> Vec<String> {
        absent("getExtensions")
    }
    fn get_filenames(&self, _language_id: &str) -> Vec<String> {
        absent("getFilenames")
    }
    fn get_configuration_files(&self, _language_id: &str) -> Vec<URI> {
        absent("getConfigurationFiles")
    }
    fn get_language_id_by_language_name(&self, _language_name: &str) -> Option<String> {
        absent("getLanguageIdByLanguageName")
    }
    fn get_language_id_by_mime_type(&self, _mime_type: Option<&str>) -> Option<String> {
        absent("getLanguageIdByMimeType")
    }
    fn guess_language_id_by_filepath_or_first_line(&self, _resource: Option<&URI>, _first_line: Option<&JsString>) -> Option<String> {
        absent("guessLanguageIdByFilepathOrFirstLine")
    }
    fn create_by_id(&self, _language_id: Option<&str>) -> Rc<dyn ILanguageSelection> {
        absent("createById")
    }
    fn create_by_mime_type(&self, _mime_type: Option<&str>) -> Rc<dyn ILanguageSelection> {
        absent("createByMimeType")
    }
    fn create_by_filepath_or_first_line(&self, _resource: Option<&URI>, _first_line: Option<&JsString>) -> Rc<dyn ILanguageSelection> {
        absent("createByFilepathOrFirstLine")
    }
    fn request_basic_language_features(&self, _language_id: &str) {
        absent("requestBasicLanguageFeatures")
    }
    fn request_rich_language_features(&self, _language_id: &str) {
        absent("requestRichLanguageFeatures")
    }
}

/// `new TestConfigurationService(configuration)` com o emissor registrado no guarda (`store.add(service.onDidChangeConfigurationEmitter)`).
pub fn new_test_service(store: &LeakGuard, configuration: Option<Dynamic>) -> Rc<TestConfigurationService> {
    let service = TestConfigurationService::new(configuration);
    store.add_rc(service.on_did_change_configuration_emitter.clone());
    service
}

/// `new class extends TestConfigurationService { isSettingAppliedForAllProfiles(): boolean { return false; } }(configuration)`, com
/// `onDidChangeRestrictedSettings = Event.None`.
pub struct TestSettingsConfiguration {
    pub service: Rc<TestConfigurationService>,
    config: Dynamic,
}

impl TestSettingsConfiguration {
    pub fn new(store: &LeakGuard, config: Option<Dynamic>) -> Rc<Self> {
        let config = config.unwrap_or_else(|| Dynamic::object(Vec::<(&str, Dynamic)>::new()));
        let service = new_test_service(store, Some(config.clone()));
        Rc::new(Self { service, config })
    }

    /// O `super` da subclasse: um serviço sem substituto sobre o mesmo objeto de configuração.
    pub fn base(&self, store: &LeakGuard) -> Rc<TestConfigurationService> {
        new_test_service(store, Some(self.config.clone()))
    }

    /// `configuration.setUserConfiguration(key, value)`.
    pub fn set_user_configuration(&self, key: &str, value: Dynamic) {
        let _ = self.service.set_user_configuration(key, value, None);
    }

    /// `configuration.getValue(key)`.
    pub fn get_value(&self, key: &str) -> Dynamic {
        self.service.get_value(Some(key), None)
    }

    /// `configuration.inspect(key)`.
    pub fn inspect(&self, key: &str) -> Rc<dyn ConfigurationValue> {
        self.service.inspect(key, &ConfigurationOverrides::default())
    }
}

impl IConfigurationService for TestSettingsConfiguration {
    fn on_did_change_configuration(&self) -> Event<Rc<ConfigurationChangeEvent>> {
        self.service.on_did_change_configuration()
    }
    fn get_configuration_data(&self) -> Option<ConfigurationData> {
        self.service.get_configuration_data()
    }
    fn get_value(&self, section: Option<&str>, overrides: Option<&ConfigurationOverrides>) -> Dynamic {
        self.service.get_value(section, overrides)
    }
    fn update_value(&self, key: &str, value: Dynamic, overrides: Option<ConfigurationUpdateOverrides>, target: Option<ConfigurationTarget>) -> Promise<()> {
        self.service.update_value(key, value, overrides, target)
    }
    fn inspect(&self, key: &str, overrides: &ConfigurationOverrides) -> Rc<dyn ConfigurationValue> {
        self.service.inspect(key, overrides)
    }
    fn reload_configuration(&self) -> Promise<()> {
        self.service.reload_configuration()
    }
    fn keys(&self) -> ConfigurationKeys {
        self.service.keys()
    }
}

impl IWorkbenchConfigurationService for TestSettingsConfiguration {
    fn restricted_settings(&self) -> RestrictedSettings {
        RestrictedSettings::default()
    }
    fn on_did_change_restricted_settings(&self) -> Event<RestrictedSettings> {
        Event::none()
    }
    fn when_remote_configuration_loaded(&self) -> Promise<()> {
        Promise::resolved(())
    }
    fn initialize(&self, _arg: &AnyWorkspaceIdentifier) -> Promise<()> {
        Promise::resolved(())
    }
    fn is_setting_applied_for_all_profiles(&self, _setting: &str) -> bool {
        false
    }
}

/// `{ ...result }`: uma cópia dos valores de um `inspect`.
pub fn copy_of(result: &dyn ConfigurationValue) -> TestConfigurationValue {
    TestConfigurationValue {
        default_value: result.default_value(),
        application_value: result.application_value(),
        user_value: result.user_value(),
        user_local_value: result.user_local_value(),
        user_remote_value: result.user_remote_value(),
        workspace_value: result.workspace_value(),
        workspace_folder_value: result.workspace_folder_value(),
        memory_value: result.memory_value(),
        policy_value: result.policy_value(),
        value: result.value(),
        override_identifiers: result.override_identifiers(),
    }
}

/// `a ?? b ?? c`: o primeiro valor que não é nulo nem `undefined`.
pub fn first_defined(values: &[&Dynamic]) -> Dynamic {
    values.iter().find(|value| !value.is_nullish()).map_or(Dynamic::Undefined, |value| (*value).clone())
}

/// O objeto `managedSettings` que os casos mudam (`managedSettings[key] = value`, `delete managedSettings[key]`).
#[derive(Default)]
pub struct ManagedStore {
    values: RefCell<HashMap<String, Option<PolicyValue>>>,
}

impl ManagedStore {
    /// `managedSettings[key] = value` (`None` é o `undefined`).
    pub fn set(&self, key: &str, value: impl Into<Option<PolicyValue>>) {
        self.values.borrow_mut().insert(key.to_owned(), value.into());
    }

    /// `delete managedSettings[key]`.
    pub fn delete(&self, key: &str) {
        self.values.borrow_mut().remove(key);
    }
}

/// `new class extends mock<IManagedSettingsService>() { onDidChangeManagedSettings = ...; getManagedSettingValue(key) { return managedSettings[key]; } }`.
/// Sem emissor, `onDidChangeManagedSettings` é o `Event.None`.
pub struct StubManagedSettingsService {
    store: Rc<ManagedStore>,
    changed: Option<Rc<Emitter<()>>>,
}

impl StubManagedSettingsService {
    /// `onDidChangeManagedSettings = Event.None`.
    pub fn new(store: Rc<ManagedStore>) -> Self {
        Self { store, changed: None }
    }

    /// `onDidChangeManagedSettings = managedSettingsChanged.event`.
    pub fn notifying(store: Rc<ManagedStore>, changed: Rc<Emitter<()>>) -> Self {
        Self { store, changed: Some(changed) }
    }
}

impl IManagedSettingsService for StubManagedSettingsService {
    fn on_did_change_managed_settings(&self) -> Event<()> {
        self.changed.as_ref().map_or_else(Event::none, |changed| changed.event())
    }
    fn get_managed_settings(&self) -> ManagedSettingsData {
        absent("getManagedSettings")
    }
    fn get_managed_setting_value(&self, key: &str) -> Option<ManagedSettingValue> {
        self.store.values.borrow().get(key).cloned().flatten()
    }
}

/// `instantiationService.stub(IManagedSettingsService, new NullManagedSettingsService())`.
pub fn null_managed_settings_service() -> Rc<dyn IManagedSettingsService> {
    Rc::new(NullManagedSettingsService)
}

/// Os serviços que os testes empilham com `instantiationService.stub(...)`. O `ILanguageService` é o [`StubLanguageService`]; quem
/// precisa de outro troca o campo `language_service` do resultado.
pub fn create_services(
    store: &LeakGuard,
    configuration: &Rc<TestSettingsConfiguration>,
    is_sessions_window: bool,
    managed_settings_service: Rc<dyn IManagedSettingsService>,
) -> (SettingsTreeServices, Rc<ExperimentalSettingsService>) {
    let experimental_settings_service = store.add_rc(ExperimentalSettingsService::new());
    let managed_settings_presentation_service = store.add_rc(ManagedSettingsPresentationService::new(managed_settings_service));
    let services = SettingsTreeServices {
        configuration_service: configuration.clone(),
        language_service: Rc::new(StubLanguageService),
        user_data_profile_service: TestUserDataProfileService::with_default_profile(),
        product_service: Rc::new(TestProductService::default()),
        environment_service: TestEnvironmentService::with_overrides(EnvironmentOverrides { is_sessions_window, ..Default::default() }),
        experimental_settings_service: experimental_settings_service.clone(),
        managed_settings_presentation_service,
    };
    (services, experimental_settings_service)
}

/// O `registerConfiguration` do `suiteSetup` e o `deregisterConfigurations` do `suiteTeardown`.
pub struct ConfigurationRegistration(ConfigurationNode);

impl ConfigurationRegistration {
    pub fn new(node: ConfigurationNode) -> Self {
        configuration_registry().register_configuration(node.clone());
        Self(node)
    }
}

impl Drop for ConfigurationRegistration {
    fn drop(&mut self) {
        if !std::thread::panicking() {
            configuration_registry().deregister_configurations(std::slice::from_ref(&self.0));
        }
    }
}

/// O `configurationNode` das suítes: `Object.fromEntries(Object.entries(properties).filter(([, property]) => property.managedSettingsPresentation))`.
pub fn register_managed_presentation_configuration(id: &str, properties: Vec<(String, PropertySchema)>) -> ConfigurationRegistration {
    let properties: Vec<(&str, PropertySchema)> =
        properties.iter().filter(|(_, property)| property.get("managedSettingsPresentation").is_truthy()).map(|(key, property)| (key.as_str(), property.clone())).collect();
    ConfigurationRegistration::new(node(id, properties))
}

/// O `configurationNode` com `{ ...terminalContribConfiguration, ...chatNetworkDomainConfigurationProperties }` filtrado por
/// `managedSettingsPresentation` (as suítes de sandbox de `settingsTreeModels.test.ts` e `settingsTree.test.ts`).
pub fn register_sandbox_presentation_configuration(id: &str) -> ConfigurationRegistration {
    let mut properties = terminal_chat_agent_tools_managed_presentation_properties();
    properties.extend(chat_network_domain_configuration_properties());
    register_managed_presentation_configuration(id, properties)
}

/// `mock<ISetting>` com a chave e o tipo (`override key = key; override type = 'string'; override description = []`).
pub fn mock_setting(key: &str, type_: &str) -> ISetting {
    ISetting { key: key.to_owned(), r#type: Some(SettingSchemaType::Single(type_.to_owned())), description: Vec::new(), ..ISetting::default() }
}

/// `{ setting, matches: [], matchType, keyMatchScore: 0, score: 0 }`.
pub fn match_of(setting: ISetting, match_type: SettingMatchType) -> ISettingMatch {
    ISettingMatch::new(Rc::new(setting), Some(Vec::new()), match_type, 0.0, 0.0)
}

/// `{ filterMatches, exactMatch }`.
pub fn search_result(filter_matches: Vec<ISettingMatch>, exact_match: bool) -> ISearchResult {
    ISearchResult { filter_matches, exact_match }
}

/// `{ settingsTarget }`: o estado de vista dos testes.
pub fn view_state(settings_target: SettingsTarget) -> ViewStateRef {
    Rc::new(RefCell::new(ISettingsEditorViewState::new(settings_target)))
}

/// `model.getElementsByName(key)![0]`.
pub fn first_element(model: &SettingsTreeModel, key: &str) -> Rc<SettingsTreeSettingElement> {
    model.get_elements_by_name(key).expect("o modelo tem a configuração")[0].clone()
}

/// `{ id, label, settings }`: uma entrada do sumário só com configurações.
pub fn toc_entry(id: &str, label: &str, settings: Vec<Rc<ISetting>>) -> ITOCEntry<Rc<ISetting>> {
    ITOCEntry { id: id.to_owned(), label: label.to_owned(), order: None, children: None, settings: Some(settings), hide: None }
}

/// Um grupo de configurações do editor (`{ id: '', range: nullRange, title: '', titleRange: nullRange, sections }`).
pub fn settings_group(sections: Vec<ISettingsSection>) -> ISettingsGroup {
    ISettingsGroup { id: String::new(), range: NULL_RANGE, title: String::new(), title_range: NULL_RANGE, sections, order: None, extension_info: None }
}
