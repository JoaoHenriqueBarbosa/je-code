//! Porte de `src/vs/workbench/contrib/preferences/test/browser/settingsEditor2.test.ts` (suíte `SettingsEditor2`), caso a caso e
//! na mesma ordem: os 12 casos da suíte aninhada `Settings refresh` (com o `for` de cinco chaves de política e o `for` de duas
//! chaves de atribuição expandidos em um `#[test]` por chave) e os 5 da suíte `isSettingsSearchUpToDate`.
//!
//! Notas de tradução:
//! - **`ensureNoDisposablesAreLeakedInTestSuite()`.** O guarda no começo de cada caso, com o store dele no lugar do `store`.
//! - **`isSettingsSearchUpToDate(..., undefined, '')`.** O `renderedSearchQuery` indefinido é `None`.
//! - **`assert.rejects(delayer.trigger(() => { }))`.** A promessa de `trigger` é esperada com o `await` depois do `cancel`,
//!   e a rejeição tem de ser um erro de cancelamento.
//! - **`createEditor`.** O `workbenchInstantiationService` com os `stub` e os `mock<...>` que montam o `SettingsEditor2` e escrevem
//!   nos campos privados dele (`editor['x'] = ...`) é o [`create_editor`]: os serviços são o [`SettingsEditor2Services`] (os
//!   dublês `stub` dos serviços que o caso não lê falham como o `undefined` do original falharia), os campos privados são os
//!   setters públicos do editor (`set_settings_tree_model`, `set_toc_tree_model`, `set_search_result_model`,
//!   `set_default_settings_editor_model`), e os elementos de DOM que o caso lê (`.settings-count`, `input`, `.settings-body`, a
//!   árvore com as linhas `.setting-item-contents`, o foco, o `.context-view`, a região `aria`) são o [`FakeView`], que implementa o
//!   [`SettingsEditorView`] com o mesmo estado que o DOM do original guarda. `sinon.spy(searchModel, 'updateChildren')` é o contador
//!   `update_children_calls` do `SearchResultModel`, e o `Event.toPromise(onDidRender)` é um sinalizador ligado ao evento de desenho.
//! - **Foco.** O `input.focus()` e o `searchInput.focus()` são [`FakeView::focus`]; o `dispatchEvent(new FocusEvent('blur', ...))`
//!   é [`FakeView::dispatch_blur`], que chama os ouvintes de `track_blur` do alvo (o evento `focusout` sobe até o elemento da
//!   configuração ou do `.context-view`, que o `DOM.trackFocus` observa).
//! - **`changeConfiguration(key)`.** O evento com `affectedKeys = Set([key])`, `source = USER` e `affectsConfiguration(section)`
//!   é o `ConfigurationChangeEvent` real com a mudança `{ keys: [key] }` e a fonte `User`.
//! - **`await timeout(0)`.** O `timeout` real de `je_base::async_` sob o `run_with_real_timers` (o `ExperimentalSettingsService`
//!   agenda o evento com um `RunOnceScheduler(0)`, que dispara antes do `timeout(0)` do caso, na ordem do relógio).
//! - **`registry.registerConfiguration(configurationNode)`.** O `suiteSetup` e o `suiteTeardown` são o [`ConfigurationRegistration`], criado
//!   no começo de cada caso. `terminalContribConfiguration` filtrado por `managedSettingsPresentation` é o que
//!   `terminal_chat_agent_tools_managed_presentation_properties` devolve, e o `read => read('test.genericPolicy') === false ? false :
//!   undefined` de `test.managedGeneric` é uma função nativa sobre o `read` do serviço de apresentação.
//! - **`mock<ISetting>()`** é o `ISetting` com `..ISetting::default()`, e o `mock<Settings2EditorModel>` com `settingsGroups` é o
//!   [`MockSettingsModel`].

use std::cell::{Cell, RefCell};
use std::collections::{HashMap, HashSet};
use std::rc::Rc;

use je_base::async_::{Delayer, MaybePromise, timeout};
use je_base::cancellation::CancellationToken;
use je_base::dynamic::{Dynamic, FunctionRef};
use je_base::errors;
use je_base::event::{Emitter, Event};
use je_base::lifecycle::{DisposableRef, IDisposable, to_disposable};
use je_base::policy::{ManagedSettingValue, ManagedSettingsData, PolicyValue};
use je_base::promise::{Promise, Thenable};
use je_base::symbols::Delay;
use je_editor::code_action::platform::{IEditorProgressService, IProgressIndicator, IProgressRunner, ProgressTotal};
use je_editor::language_service::LanguageService;
use je_extensions::management::{
    ExtensionInfo, ExtensionQueryOptions, ExtensionsControlManifest, GalleryExtension, GalleryExtensionVersion, InstallOperation, ProductVersion, QueryOptions, StatisticType,
};
use je_extensions::manifest::{ExtensionIdentifierInfo, TargetPlatform};
use je_platform::configuration::models::{ConfigurationChangeEvent, ConfigurationKeys};
use je_platform::configuration::registry::{ConfigurationNode, ConfigurationScope, PropertySchema, configuration_registry};
use je_platform::configuration::service::IConfigurationService;
use je_platform::configuration::test_configuration_service::TestConfigurationService;
use je_platform::configuration::{ConfigurationData, ConfigurationOverrides, ConfigurationTarget, ConfigurationUpdateOverrides, ConfigurationValue};
use je_platform::policy::copilot_managed_settings::{
    COPILOT_SANDBOX_ALLOW_BYPASS_KEY, COPILOT_SANDBOX_ALLOW_DEV_TOOL_ACCESS_KEY, COPILOT_SANDBOX_ALLOW_LOCAL_NETWORK_KEY, COPILOT_SANDBOX_ALLOW_OUTBOUND_KEY,
    COPILOT_SANDBOX_ENABLED_KEY, COPILOT_SANDBOX_LSP_SERVERS_KEY, COPILOT_SANDBOX_MCP_SERVERS_KEY, IManagedSettingsService,
};
use je_platform::sandbox::settings::AgentSandboxSettingId;
use je_platform::storage::{InMemoryStorageService, StorageService};
use je_platform::telemetry::telemetry_utils::null_telemetry_service;
use je_platform::workspace::AnyWorkspaceIdentifier;
use je_widgets::tree::tree_types::{ITreeElement, ITreeFilter, TreeFilterResult, TreeVisibility};
use je_workbench::common::editor::editor_model::{EditorModel, IEditorModel};
use je_workbench::contrib::preferences::common::preferences::{
    ADVANCED_SETTING_TAG, IAiSearchProvider, IPreferencesSearchService, ISearchProvider, POLICY_SETTING_TAG,
};
use je_workbench::contrib::preferences::settings_editor2::{
    FocusTarget, FocusedSettingControl, IDefaultSettingsEditorModel, SettingsEditor2, SettingsEditor2Services, SettingsEditorView, is_settings_search_up_to_date,
};
use je_workbench::contrib::preferences::settings_layout::ITOCEntry;
use je_workbench::contrib::preferences::settings_tree::SettingsTreeFilter;
use je_workbench::contrib::preferences::settings_tree_models::{
    SearchResultIdx, SearchResultModel, SettingsTreeElement, SettingsTreeGroupChild, SettingsTreeGroupElement, SettingsTreeModel, SettingsTreeServices, ViewStateRef,
    parse_query,
};
use je_workbench::contrib::preferences::toc_tree::TocTreeModel;
use je_workbench::services::configuration::common::configuration::{IWorkbenchConfigurationService, RestrictedSettings};
use je_workbench::services::configuration::common::experimental_settings::ExperimentalSettingsService;
use je_workbench::services::configuration::common::managed_settings_presentation::{ManagedSettingsPresentationService, read_managed_setting};
use je_workbench::services::extension_management::extension_gallery_service::IExtensionGalleryService;
use je_workbench::services::extension_management::extension_management::{DidUninstallExtensionEvent, ExtensionRef, IExtensionManagementService, InstallExtensionResult};
use je_workbench::services::preferences::preferences::{
    IFilterResult, IGroupFilter, ISearchResult, ISearchResultGroup, ISetting, ISettingMatch, ISettingMatcher, ISettingsEditorModel, ISettingsGroup, ISettingsSection,
    SettingMatchType, SettingSchemaType,
};
use je_workbench::services::preferences::preferences_models::NULL_RANGE;
use je_workbench::contrib::terminal_contrib::chat_agent_tools::terminal_chat_agent_tools_configuration::terminal_chat_agent_tools_managed_presentation_properties;
use je_workbench::test::browser::workbench_test_services::{EnvironmentOverrides, TestEnvironmentService, TestExtensionService};
use je_workbench::test::common::workbench_test_services::{TestChatEntitlementService, TestProductService, TestUserDataProfileService, TestWorkspaceTrustManagementService};

#[path = "../../je-platform/tests/common/mod.rs"]
mod common;

use common::{LeakGuard, change, empty_test_configuration, ensure_no_disposables_are_leaked_in_test_suite, node, null_log, prop_with, run_with_faked_timers, run_with_real_timers};

// ---------------------------------------------------------------------------------------------------------------------
// Dublês e construtores comuns às suítes.

fn absent(method: &str) -> ! {
    panic!("`{method}` não existe no stub parcial")
}

/// `new class extends TestConfigurationService { readonly onDidChangeRestrictedSettings = Event.None; isSettingAppliedForAllProfiles() { return false; } }()`.
struct TestSettingsConfiguration {
    service: Rc<TestConfigurationService>,
}

impl TestSettingsConfiguration {
    fn new(guard: &LeakGuard) -> Rc<Self> {
        let service = TestConfigurationService::new(None);
        guard.add_rc(service.on_did_change_configuration_emitter.clone());
        Rc::new(Self { service })
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

/// O objeto `managedSettings` que os casos mudam (`managedSettings[key] = value`, `delete managedSettings[key]`).
#[derive(Default)]
struct ManagedStore {
    values: RefCell<HashMap<String, Option<PolicyValue>>>,
}

impl ManagedStore {
    /// `managedSettings[key] = value`.
    fn set(&self, key: &str, value: PolicyValue) {
        self.values.borrow_mut().insert(key.to_owned(), Some(value));
    }

    /// `delete managedSettings[key]`.
    fn delete(&self, key: &str) {
        self.values.borrow_mut().remove(key);
    }
}

/// `new class extends mock<IManagedSettingsService>() { onDidChangeManagedSettings = managedSettingsChanged.event; getManagedSettingValue(key) { return managedSettings[key]; } }`.
struct StubManagedSettingsService {
    store: Rc<ManagedStore>,
    changed: Rc<Emitter<()>>,
}

impl IManagedSettingsService for StubManagedSettingsService {
    fn on_did_change_managed_settings(&self) -> Event<()> {
        self.changed.event()
    }
    fn get_managed_settings(&self) -> ManagedSettingsData {
        absent("getManagedSettings")
    }
    fn get_managed_setting_value(&self, key: &str) -> Option<ManagedSettingValue> {
        self.store.values.borrow().get(key).cloned().flatten()
    }
}

/// O `registerConfiguration` do `suiteSetup` e o `deregisterConfigurations` do `suiteTeardown`.
struct ConfigurationRegistration(ConfigurationNode);

impl ConfigurationRegistration {
    fn new(node: ConfigurationNode) -> Self {
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

/// O `configurationNode` da suíte: as propriedades de `terminalContribConfiguration` com `managedSettingsPresentation` e o
/// `test.managedGeneric`.
fn register_managed_editor_presentation_configuration() -> ConfigurationRegistration {
    let mut properties: Vec<(String, PropertySchema)> = terminal_chat_agent_tools_managed_presentation_properties()
        .into_iter()
        .filter(|(_, property)| matches!(property.get("managedSettingsPresentation"), Dynamic::Function(_)))
        .collect();
    let managed_generic = FunctionRef::native("managedGeneric", 1, |args| {
        let read = args.first().cloned().unwrap_or(Dynamic::Undefined);
        if matches!(read_managed_setting(&read, "test.genericPolicy"), Dynamic::Bool(false)) { Dynamic::Bool(false) } else { Dynamic::Undefined }
    });
    properties.push(("test.managedGeneric".to_owned(), prop_with("{ 'type': 'boolean' }", vec![("managedSettingsPresentation", Dynamic::Function(managed_generic))])));
    let properties: Vec<(&str, PropertySchema)> = properties.iter().map(|(key, property)| (key.as_str(), property.clone())).collect();
    ConfigurationRegistration::new(node("managedEditorPresentationTest", properties))
}

/// O `IPreferencesSearchService` que o caso não lê (`{}`).
struct StubPreferencesSearchService;

impl IPreferencesSearchService for StubPreferencesSearchService {
    fn get_local_search_provider(&self, _filter: &str) -> Rc<dyn ISearchProvider> {
        absent("getLocalSearchProvider")
    }
    fn get_remote_search_provider(&self, _filter: &str, _new_extensions_only: bool) -> Option<Rc<dyn ISearchProvider>> {
        absent("getRemoteSearchProvider")
    }
    fn get_ai_search_provider(&self, _filter: &str) -> Option<Rc<dyn IAiSearchProvider>> {
        absent("getAiSearchProvider")
    }
}

/// `{ onDidInstallExtensions: Event.None, onDidUninstallExtension: Event.None }`.
struct StubExtensionManagementService;

impl IExtensionManagementService for StubExtensionManagementService {
    fn on_did_install_extensions(&self) -> Event<Vec<InstallExtensionResult>> {
        Event::none()
    }
    fn on_did_uninstall_extension(&self) -> Event<DidUninstallExtensionEvent> {
        Event::none()
    }
    fn get_installed(&self) -> Promise<Vec<ExtensionRef>> {
        absent("getInstalled")
    }
    fn get_extensions_control_manifest(&self) -> Promise<ExtensionsControlManifest> {
        absent("getExtensionsControlManifest")
    }
}

/// `{ isEnabled: () => false }`.
struct StubExtensionGalleryService;

impl IExtensionGalleryService for StubExtensionGalleryService {
    fn is_enabled(&self) -> bool {
        false
    }
    fn query(&self, _options: QueryOptions, _token: CancellationToken) -> Promise<Rc<dyn je_base::paging::IPager<GalleryExtension>>> {
        absent("query")
    }
    fn get_extensions(&self, _extension_infos: Vec<ExtensionInfo>, _options: ExtensionQueryOptions, _token: CancellationToken) -> Promise<Vec<GalleryExtension>> {
        absent("getExtensions")
    }
    fn is_extension_compatible(&self, _extension: &GalleryExtension, _include_pre_release: bool, _target_platform: TargetPlatform, _product_version: Option<ProductVersion>) -> Promise<bool> {
        absent("isExtensionCompatible")
    }
    fn get_compatible_extension(
        &self,
        _extension: &GalleryExtension,
        _include_pre_release: bool,
        _target_platform: TargetPlatform,
        _product_version: Option<ProductVersion>,
    ) -> Promise<Option<GalleryExtension>> {
        absent("getCompatibleExtension")
    }
    fn get_all_compatible_versions(&self, _extension_identifier: &ExtensionIdentifierInfo, _include_pre_release: bool, _target_platform: TargetPlatform) -> Promise<Vec<GalleryExtensionVersion>> {
        absent("getAllCompatibleVersions")
    }
    fn get_all_versions(&self, _extension_identifier: &ExtensionIdentifierInfo) -> Promise<Vec<GalleryExtensionVersion>> {
        absent("getAllVersions")
    }
    fn download(&self, _extension: &GalleryExtension, _location: &je_base::uri::URI, _operation: InstallOperation) -> Promise<()> {
        absent("download")
    }
    fn download_signature_archive(&self, _extension: &GalleryExtension, _location: &je_base::uri::URI) -> Promise<()> {
        absent("downloadSignatureArchive")
    }
    fn report_statistic(&self, _publisher: &str, _name: &str, _version: &str, _statistic_type: StatisticType) -> Promise<()> {
        absent("reportStatistic")
    }
    fn get_readme(&self, _extension: &GalleryExtension, _token: CancellationToken) -> Promise<String> {
        absent("getReadme")
    }
    fn get_manifest(&self, _extension: &GalleryExtension, _token: CancellationToken) -> Promise<Option<serde_json::Value>> {
        absent("getManifest")
    }
    fn get_changelog(&self, _extension: &GalleryExtension, _token: CancellationToken) -> Promise<String> {
        absent("getChangelog")
    }
    fn get_core_translation(&self, _extension: &GalleryExtension, _language_id: &str) -> Promise<Option<serde_json::Value>> {
        absent("getCoreTranslation")
    }
    fn get_extensions_control_manifest(&self) -> Promise<ExtensionsControlManifest> {
        absent("getExtensionsControlManifest")
    }
}

/// `new class extends mock<IProgressRunner>() { override done(): void { } }()`.
struct StubProgressRunner;

impl IProgressRunner for StubProgressRunner {
    fn total(&self, _value: f64) {
        absent("total")
    }
    fn worked(&self, _value: f64) {
        absent("worked")
    }
    fn done(&self) {}
}

/// `{ show: () => new class extends mock<IProgressRunner>() { override done(): void { } }() }`.
struct StubEditorProgressService;

impl IProgressIndicator for StubEditorProgressService {
    fn show(&self, _total: ProgressTotal, _delay: Option<u64>) -> Rc<dyn IProgressRunner> {
        Rc::new(StubProgressRunner)
    }
    fn show_while(&self, _promise: &dyn Thenable, _delay: Option<u64>) -> Promise<()> {
        absent("showWhile")
    }
}

impl IEditorProgressService for StubEditorProgressService {}

/// `new class extends mock<Settings2EditorModel>() { override get settingsGroups() { return [...]; } }()`.
struct MockSettingsModel {
    base: EditorModel,
    groups: Vec<ISettingsGroup>,
}

impl IDisposable for MockSettingsModel {
    fn dispose(&self) {
        self.base.dispose();
    }
}

impl IEditorModel for MockSettingsModel {
    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
    fn on_will_dispose(&self) -> Event<()> {
        self.base.on_will_dispose()
    }
    fn resolve(&self) -> Promise<()> {
        self.base.resolve()
    }
    fn is_resolved(&self) -> bool {
        self.base.is_resolved()
    }
    fn is_disposed(&self) -> bool {
        self.base.is_disposed()
    }
}

impl ISettingsEditorModel for MockSettingsModel {
    fn on_did_change_groups(&self) -> Event<()> {
        absent("onDidChangeGroups")
    }
    fn settings_groups(&self) -> Vec<ISettingsGroup> {
        self.groups.clone()
    }
    fn filter_settings(&self, _filter: &str, _group_filter: IGroupFilter, _setting_matcher: ISettingMatcher) -> Vec<ISettingMatch> {
        absent("filterSettings")
    }
    fn update_result_group(&self, _id: &str, _result_group: Option<ISearchResultGroup>) -> Option<IFilterResult> {
        absent("updateResultGroup")
    }
    fn get_preference(&self, _key: &str) -> Option<Rc<ISetting>> {
        absent("getPreference")
    }
}

impl IDefaultSettingsEditorModel for MockSettingsModel {
    fn set_additional_groups(&self, _groups: Vec<ISettingsGroup>) {
        absent("setAdditionalGroups")
    }
}

/// `{ searchModel: async () => result }`.
struct FixedSearchProvider {
    result: ISearchResult,
}

impl ISearchProvider for FixedSearchProvider {
    fn search_model(&self, _preferences_model: Rc<dyn ISettingsEditorModel>, _token: CancellationToken) -> Promise<Option<ISearchResult>> {
        Promise::resolved(Some(self.result.clone()))
    }
}

/// Quem tem o foco (`document.activeElement`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Active {
    Body,
    SearchInput,
    SettingInput,
    ContextMenuItem,
}

/// Um ouvinte de `DOM.trackFocus(...).onDidBlur`.
struct BlurListener {
    id: u64,
    target: FocusTarget,
    on_blur: Rc<dyn Fn()>,
}

/// O DOM que o `createEditor` do original monta (`.settings-editor`, `.settings-count`, `input`, `.settings-body`, a árvore e as
/// linhas `.setting-item-contents`) e o foco do documento.
struct FakeView {
    root_classes: RefCell<HashSet<String>>,
    count_display: RefCell<String>,
    count_text: RefCell<String>,
    clear_filter_link_display: RefCell<String>,
    body_visibility: RefCell<String>,
    announcement: RefCell<String>,
    input_aria_label: RefCell<Option<String>>,
    rendered_keys: RefCell<Vec<String>>,
    active: Cell<Active>,
    blur_listeners: Rc<RefCell<Vec<BlurListener>>>,
    next_listener_id: Cell<u64>,
    on_did_render: Rc<Emitter<()>>,
}

impl FakeView {
    fn new(guard: &LeakGuard) -> Rc<Self> {
        let on_did_render = Emitter::new();
        guard.add_rc(on_did_render.clone());
        Rc::new(Self {
            root_classes: RefCell::new(HashSet::new()),
            count_display: RefCell::new(String::new()),
            count_text: RefCell::new(String::new()),
            clear_filter_link_display: RefCell::new(String::new()),
            body_visibility: RefCell::new(String::new()),
            announcement: RefCell::new(String::new()),
            input_aria_label: RefCell::new(None),
            rendered_keys: RefCell::new(Vec::new()),
            active: Cell::new(Active::Body),
            blur_listeners: Rc::new(RefCell::new(Vec::new())),
            next_listener_id: Cell::new(0),
            on_did_render,
        })
    }

    /// `element.focus()`.
    fn focus(&self, element: Active) {
        self.active.set(element);
    }

    /// `document.activeElement === element`.
    fn is_active(&self, element: Active) -> bool {
        self.active.get() == element
    }

    /// `element.dispatchEvent(new FocusEvent('blur', ...))`: o `focusout` sobe até o elemento da configuração (o `input` da primeira
    /// linha) ou até o `.context-view` (o botão do menu), e os ouvintes do alvo rodam.
    fn dispatch_blur(&self, element: Active) {
        let listeners: Vec<Rc<dyn Fn()>> = self
            .blur_listeners
            .borrow()
            .iter()
            .filter(|listener| match (&listener.target, element) {
                (FocusTarget::Setting(key), Active::SettingInput) => self.rendered_keys.borrow().first() == Some(key),
                (FocusTarget::ContextView, Active::ContextMenuItem) => true,
                _ => false,
            })
            .map(|listener| listener.on_blur.clone())
            .collect();
        for on_blur in listeners {
            on_blur();
        }
    }

    fn on_did_render(&self) -> Event<()> {
        self.on_did_render.event()
    }
}

impl SettingsEditorView for FakeView {
    fn is_visible(&self) -> bool {
        true
    }
    fn layout(&self) {}
    fn layout_split_view(&self) {}
    fn hide_toc(&self) {}
    fn restore_cached_state(&self, _view_state: &ViewStateRef) -> Option<String> {
        None
    }

    fn set_root_class(&self, class_name: &str, on: bool) {
        if on {
            self.root_classes.borrow_mut().insert(class_name.to_owned());
        } else {
            self.root_classes.borrow_mut().remove(class_name);
        }
    }
    fn count_display(&self) -> String {
        self.count_display.borrow().clone()
    }
    fn set_count_display(&self, display: &str) {
        *self.count_display.borrow_mut() = display.to_owned();
    }
    fn set_count_text(&self, text: &str) {
        *self.count_text.borrow_mut() = text.to_owned();
    }
    fn set_clear_filter_link_display(&self, display: &str) {
        *self.clear_filter_link_display.borrow_mut() = display.to_owned();
    }
    fn set_body_visibility(&self, visibility: &str) {
        *self.body_visibility.borrow_mut() = visibility.to_owned();
    }
    fn aria_status(&self, message: &str) {
        *self.announcement.borrow_mut() = message.to_owned();
    }

    fn search_value(&self) -> String {
        String::new()
    }
    fn update_search_aria_label(&self, label: &str) {
        *self.input_aria_label.borrow_mut() = Some(label.to_owned());
    }
    fn add_search_to_history(&self) {}
    fn search_history(&self) -> Vec<String> {
        Vec::new()
    }
    fn set_search_placeholder(&self, _placeholder: &str) {}
    fn update_language_filter_indicators(&self, _language_filter: Option<&str>) {}

    fn ai_results_toggle_shown(&self) -> bool {
        false
    }
    fn update_ai_search_toggle_visibility(&self, _can_show_toggle: bool) {}
    fn disable_ai_search_toggle(&self) {}
    fn enable_ai_results_action(&self) {}

    fn settings_tree_first_visible_element(&self) -> Option<SettingsTreeGroupChild> {
        None
    }
    fn settings_tree_has_element(&self, _element: &Rc<je_workbench::contrib::preferences::settings_tree_models::SettingsTreeSettingElement>) -> bool {
        true
    }
    fn settings_tree_rerender(&self, _element: &Rc<je_workbench::contrib::preferences::settings_tree_models::SettingsTreeSettingElement>) {}
    fn settings_tree_set_children(&self, children: Vec<ITreeElement<SettingsTreeGroupChild>>) {
        fn render(children: &[ITreeElement<SettingsTreeGroupChild>], rendered_keys: &mut Vec<String>) {
            for child in children {
                if let SettingsTreeGroupChild::Setting(setting) = &child.element {
                    rendered_keys.push(setting.setting.key.clone());
                } else if let Some(grandchildren) = &child.children {
                    render(grandchildren, rendered_keys);
                }
            }
        }

        let mut rendered_keys = Vec::new();
        render(&children, &mut rendered_keys);
        *self.rendered_keys.borrow_mut() = rendered_keys;
        // O `DOM.clearNode(tree)` remove a linha focada, e o foco volta ao corpo.
        if self.active.get() == Active::SettingInput {
            self.active.set(Active::Body);
        }
        self.on_did_render.fire(());
    }
    fn settings_tree_reveal(&self, _element: &SettingsTreeGroupChild) {}
    fn settings_tree_scroll_to_top(&self) {}
    fn set_setting_configured_class(&self, _key: &str, _configured: bool) {}

    fn toc_tree_set_children(&self, _children: Vec<ITreeElement<Rc<SettingsTreeGroupElement>>>) {}
    fn toc_tree_collapse_all(&self) {}
    fn toc_tree_expand_all(&self) {}
    fn toc_tree_set_focus(&self, _elements: &[Rc<SettingsTreeGroupElement>]) {}
    fn toc_tree_set_selection(&self, _elements: &[Rc<SettingsTreeGroupElement>]) {}

    fn focused_setting_control(&self) -> Option<FocusedSettingControl> {
        if self.active.get() != Active::SettingInput {
            return None;
        }
        let key = self.rendered_keys.borrow().first().cloned()?;
        Some(FocusedSettingControl { key, parent_element_is_list: Some(false) })
    }
    fn context_view_focused(&self) -> bool {
        self.active.get() == Active::ContextMenuItem
    }
    fn track_blur(&self, target: FocusTarget, on_blur: Box<dyn Fn()>) -> DisposableRef {
        let id = self.next_listener_id.get();
        self.next_listener_id.set(id + 1);
        self.blur_listeners.borrow_mut().push(BlurListener { id, target, on_blur: Rc::from(on_blur) });
        let blur_listeners = self.blur_listeners.clone();
        to_disposable(move || blur_listeners.borrow_mut().retain(|listener| listener.id != id))
    }
}

/// O que o `read` do `createEditor` devolve.
#[derive(Debug, PartialEq)]
struct EditorState {
    count: String,
    no_results: bool,
    visibility: String,
    input_label: Option<String>,
    announcement: String,
    toc_count: Option<usize>,
    rendered_keys: Vec<String>,
}

/// O estado esperado quando há resultados (`count` é `"{n} Setting(s) Found"`).
fn found_state(count: usize, rendered_keys: &[&str]) -> EditorState {
    let message = if count == 1 { "1 Setting Found".to_owned() } else { format!("{count} Settings Found") };
    EditorState {
        count: message.clone(),
        no_results: false,
        visibility: "visible".to_owned(),
        input_label: Some(format!("Search settings. {message}")),
        announcement: message,
        toc_count: Some(count),
        rendered_keys: rendered_keys.iter().map(|key| (*key).to_owned()).collect(),
    }
}

/// O estado esperado sem resultados.
fn empty_state() -> EditorState {
    EditorState {
        count: "No Settings Found".to_owned(),
        no_results: true,
        visibility: "hidden".to_owned(),
        input_label: Some("Search settings. No Settings Found".to_owned()),
        announcement: "No Settings Found".to_owned(),
        toc_count: Some(0),
        rendered_keys: Vec::new(),
    }
}

/// O objeto que `createEditor` devolve.
struct Fixture {
    editor: Rc<SettingsEditor2>,
    assignments: Rc<ExperimentalSettingsService>,
    managed_settings: Rc<ManagedStore>,
    managed_settings_changed: Rc<Emitter<()>>,
    search_model: Rc<SearchResultModel>,
    settings: Vec<Rc<ISetting>>,
    view: Rc<FakeView>,
    settings_model: Rc<SettingsTreeModel>,
    tree_filter: SettingsTreeFilter,
    view_state: ViewStateRef,
    configuration: Rc<TestSettingsConfiguration>,
    _registration: Rc<ConfigurationRegistration>,
}

impl Fixture {
    /// `read()`.
    fn read(&self) -> EditorState {
        EditorState {
            count: self.view.count_text.borrow().clone(),
            no_results: self.view.root_classes.borrow().contains("no-results"),
            visibility: self.view.body_visibility.borrow().clone(),
            input_label: self.view.input_aria_label.borrow().clone(),
            announcement: self.view.announcement.borrow().clone(),
            toc_count: self.settings_model.root().count(),
            rendered_keys: self.view.rendered_keys.borrow().clone(),
        }
    }

    /// `rebuild.callCount`.
    fn rebuilds(&self) -> usize {
        self.search_model.update_children_calls()
    }

    /// `changeConfiguration(key)`.
    fn change_configuration(&self, key: &str) {
        let event = ConfigurationChangeEvent::new(change(&[key], &[]), None, Rc::new(empty_test_configuration()), None, null_log());
        event.source.set(Some(ConfigurationTarget::User));
        self.configuration.service.on_did_change_configuration_emitter.fire(Rc::new(event));
    }
}

/// `createEditor(query = '@tag:expassigned', advanced = false, settingKeys = ['test.assigned', 'test.unassigned'])`.
fn create_editor(guard: &LeakGuard, query: &str, advanced: bool, setting_keys: &[&str]) -> Fixture {
    let registration = Rc::new(register_managed_editor_presentation_configuration());
    let configuration = TestSettingsConfiguration::new(guard);
    let assignments = guard.add_rc(ExperimentalSettingsService::new());
    let managed_settings_changed = Emitter::new();
    guard.add_rc(managed_settings_changed.clone());
    let managed_settings = Rc::new(ManagedStore::default());
    let managed_settings_service = Rc::new(StubManagedSettingsService { store: managed_settings.clone(), changed: managed_settings_changed.clone() });
    let managed_settings_presentation_service = guard.add_rc(ManagedSettingsPresentationService::new(managed_settings_service));

    let tree_services = SettingsTreeServices {
        configuration_service: configuration.clone(),
        language_service: guard.add_rc(LanguageService::new(false)),
        user_data_profile_service: TestUserDataProfileService::new(),
        product_service: Rc::new(TestProductService::default()),
        environment_service: TestEnvironmentService::with_overrides(EnvironmentOverrides::default()),
        experimental_settings_service: assignments.clone(),
        managed_settings_presentation_service,
    };
    let storage_service: Rc<dyn StorageService> = Rc::new(guard.add_rc(InMemoryStorageService::new()));
    let services = SettingsEditor2Services {
        tree: tree_services.clone(),
        telemetry_service: null_telemetry_service(),
        preferences_search_service: Rc::new(StubPreferencesSearchService),
        log_service: null_log(),
        storage_service,
        workspace_trust_management_service: guard.add_rc(TestWorkspaceTrustManagementService::new(None, None)),
        extension_service: TestExtensionService::new(),
        extension_management_service: Rc::new(StubExtensionManagementService),
        extension_gallery_service: Rc::new(StubExtensionGalleryService),
        editor_progress_service: Rc::new(StubEditorProgressService),
        chat_entitlement_service: TestChatEntitlementService::new(),
    };

    let view = FakeView::new(guard);
    let editor = guard.add_rc(SettingsEditor2::new(services, view.clone()));

    let settings: Vec<Rc<ISetting>> = setting_keys
        .iter()
        .map(|key| {
            Rc::new(ISetting {
                key: (*key).to_owned(),
                r#type: Some(SettingSchemaType::Single("boolean".to_owned())),
                description: vec!["Example setting".to_owned()],
                scope: Some(ConfigurationScope::Resource),
                tags: Some(if advanced { vec![ADVANCED_SETTING_TAG.to_owned()] } else { Vec::new() }),
                ..ISetting::default()
            })
        })
        .collect();
    let group = |sections: Vec<ISettingsSection>| ISettingsGroup {
        id: String::new(),
        range: NULL_RANGE,
        title: String::new(),
        title_range: NULL_RANGE,
        sections,
        order: None,
        extension_info: None,
    };
    editor.set_default_settings_editor_model(guard.add_rc(Rc::new(MockSettingsModel {
        base: EditorModel::new_inner(),
        groups: vec![group(Vec::new()), group(vec![ISettingsSection { title: None, settings: settings.clone() }])],
    })));

    let view_state = editor.view_state();
    {
        let mut state = view_state.borrow_mut();
        state.query = Some(query.to_owned());
        state.tag_filters = Some(parse_query(query).tags.into_iter().collect());
    }
    let settings_model = SettingsTreeModel::new(view_state.clone(), true, tree_services.clone());
    editor.set_settings_tree_model(settings_model.clone());
    settings_model.update(&ITOCEntry { id: "root".to_owned(), label: "Test".to_owned(), order: None, children: None, settings: Some(settings.clone()), hide: None });
    let toc_model = TocTreeModel::new(view_state.clone(), tree_services.environment_service.clone());
    editor.set_toc_tree_model(toc_model.clone());
    toc_model.set_settings_tree_root(settings_model.root());
    editor.set_search_result_model(Some(editor.create_filter_model()));
    let search_model = editor.search_result_model().expect("o modelo dos resultados da busca");
    toc_model.set_current_search_model(Some(search_model.clone()));
    editor.render_result_count_messages(false);
    editor.refresh_tree();
    // `sinon.spy(searchModel, 'updateChildren')`: a contagem começa agora.
    search_model.reset_update_children_calls();

    let tree_filter = SettingsTreeFilter::new(view_state.clone(), false, tree_services.environment_service.clone());
    Fixture { editor, assignments, managed_settings, managed_settings_changed, search_model, settings, view, settings_model, tree_filter, view_state, configuration, _registration: registration }
}

/// `searchModel.root.children[0]`.
fn first_row(model: &SearchResultModel) -> SettingsTreeGroupChild {
    model.root().children().first().cloned().expect("o modelo tem uma linha")
}

/// `a === b` entre duas linhas da árvore.
fn same_row(a: &SettingsTreeGroupChild, b: &SettingsTreeGroupChild) -> bool {
    match (a, b) {
        (SettingsTreeGroupChild::Setting(a), SettingsTreeGroupChild::Setting(b)) => Rc::ptr_eq(a, b),
        (SettingsTreeGroupChild::Group(a), SettingsTreeGroupChild::Group(b)) => Rc::ptr_eq(a, b),
        (SettingsTreeGroupChild::NewExtensions(a), SettingsTreeGroupChild::NewExtensions(b)) => Rc::ptr_eq(a, b),
        _ => false,
    }
}

/// As chaves das linhas de configuração dos filhos de um modelo (`children.map(element => { assert.ok(element instanceof SettingsTreeSettingElement); return element.setting.key; })`).
fn setting_keys_of(model: &SearchResultModel) -> Vec<String> {
    model
        .root()
        .children()
        .iter()
        .map(|element| element.as_setting().expect("assert.ok(element instanceof SettingsTreeSettingElement)").setting.key.clone())
        .collect()
}

/// `await timeout(0)`.
async fn timeout_zero() {
    timeout(0).await.expect("timeout(0)");
}

// ---------------------------------------------------------------------------------------------------------------------
// suite('Settings refresh')

#[test]
fn resolved_sandbox_policies_refresh_policy_filtered_results_and_removal_without_configuration_changes() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let keys = [AgentSandboxSettingId::AgentSandboxEnabled.as_str(), AgentSandboxSettingId::AgentSandboxAllowUnsandboxedCommands.as_str()];
    let f = create_editor(&guard, "@hasPolicy", false, &keys);
    let initial = f.read().rendered_keys;
    f.managed_settings.set(COPILOT_SANDBOX_ENABLED_KEY, PolicyValue::Boolean(true));
    f.managed_settings_changed.fire(());
    let required = f.read();
    f.managed_settings_changed.fire(());
    let unchanged_rebuilds = f.rebuilds();
    f.managed_settings.set(COPILOT_SANDBOX_ALLOW_BYPASS_KEY, PolicyValue::Boolean(true));
    f.managed_settings_changed.fire(());
    let bypass_allowed = f.read().rendered_keys;
    f.managed_settings.set(COPILOT_SANDBOX_ALLOW_BYPASS_KEY, PolicyValue::Boolean(false));
    f.managed_settings_changed.fire(());
    let bypass_denied = f.read().rendered_keys;
    f.managed_settings.delete(COPILOT_SANDBOX_ENABLED_KEY);
    f.managed_settings.delete(COPILOT_SANDBOX_ALLOW_BYPASS_KEY);
    f.managed_settings_changed.fire(());

    assert_eq!(initial, Vec::<String>::new());
    assert_eq!((required.rendered_keys, required.count), (keys.map(str::to_owned).to_vec(), "2 Settings Found".to_owned()));
    assert_eq!(unchanged_rebuilds, 1);
    assert_eq!(bypass_allowed, keys[..1].iter().map(|key| (*key).to_owned()).collect::<Vec<_>>());
    assert_eq!(bypass_denied, keys.map(str::to_owned).to_vec());
    assert_eq!(f.read().rendered_keys, Vec::<String>::new());
}

#[test]
fn outbound_policy_changes_refresh_managed_results_independently_of_sandbox_enablement() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let key = AgentSandboxSettingId::AgentSandboxAllowNetwork.as_str();
    let f = create_editor(&guard, "@hasPolicy", false, &[key]);
    let initial = f.read().rendered_keys;
    f.managed_settings.set(COPILOT_SANDBOX_ALLOW_OUTBOUND_KEY, PolicyValue::Boolean(false));
    f.managed_settings_changed.fire(());
    let denied = f.read();
    f.managed_settings_changed.fire(());
    let unchanged_rebuilds = f.rebuilds();
    f.managed_settings.set(COPILOT_SANDBOX_ALLOW_OUTBOUND_KEY, PolicyValue::Boolean(true));
    f.managed_settings_changed.fire(());
    let allowed = f.read().rendered_keys;
    f.managed_settings.set(COPILOT_SANDBOX_ALLOW_OUTBOUND_KEY, PolicyValue::Boolean(false));
    f.managed_settings_changed.fire(());
    f.managed_settings.delete(COPILOT_SANDBOX_ALLOW_OUTBOUND_KEY);
    f.managed_settings_changed.fire(());

    assert_eq!(initial, Vec::<String>::new());
    assert_eq!((denied.rendered_keys, denied.count), (vec![key.to_owned()], "1 Setting Found".to_owned()));
    assert_eq!(unchanged_rebuilds, 1);
    assert_eq!(allowed, Vec::<String>::new());
    assert_eq!(f.read().rendered_keys, Vec::<String>::new());
}

/// O corpo do `for (const [key, policyKey, managedValue] of [...])`: `{key} policy changes refresh managed results and unlock when not restricted`.
fn policy_changes_refresh_managed_results_and_unlock_when_not_restricted(key: &str, policy_key: &str, managed_value: bool) {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let f = create_editor(&guard, "@hasPolicy", false, &[key]);
    let initial = f.read().rendered_keys;
    f.managed_settings.set(policy_key, PolicyValue::Boolean(managed_value));
    f.managed_settings_changed.fire(());
    let required = f.read();
    f.managed_settings_changed.fire(());
    let unchanged_rebuilds = f.rebuilds();
    f.managed_settings.set(policy_key, PolicyValue::Boolean(!managed_value));
    f.managed_settings_changed.fire(());
    let optional = f.read().rendered_keys;
    f.managed_settings.set(policy_key, PolicyValue::Boolean(managed_value));
    f.managed_settings_changed.fire(());
    f.managed_settings.delete(policy_key);
    f.managed_settings_changed.fire(());

    assert_eq!(initial, Vec::<String>::new());
    assert_eq!((required.rendered_keys, required.count), (vec![key.to_owned()], "1 Setting Found".to_owned()));
    assert_eq!(unchanged_rebuilds, 1);
    assert_eq!(optional, Vec::<String>::new());
    assert_eq!(f.read().rendered_keys, Vec::<String>::new());
}

#[test]
fn agent_sandbox_mcp_servers_policy_changes_refresh_managed_results_and_unlock_when_not_restricted() {
    policy_changes_refresh_managed_results_and_unlock_when_not_restricted(AgentSandboxSettingId::AgentSandboxMcpServers.as_str(), COPILOT_SANDBOX_MCP_SERVERS_KEY, true);
}

#[test]
fn agent_sandbox_lsp_servers_policy_changes_refresh_managed_results_and_unlock_when_not_restricted() {
    policy_changes_refresh_managed_results_and_unlock_when_not_restricted(AgentSandboxSettingId::AgentSandboxLspServers.as_str(), COPILOT_SANDBOX_LSP_SERVERS_KEY, true);
}

#[test]
fn agent_sandbox_allow_dev_tool_access_policy_changes_refresh_managed_results_and_unlock_when_not_restricted() {
    policy_changes_refresh_managed_results_and_unlock_when_not_restricted(AgentSandboxSettingId::AgentSandboxAllowDevToolAccess.as_str(), COPILOT_SANDBOX_ALLOW_DEV_TOOL_ACCESS_KEY, false);
}

#[test]
fn agent_sandbox_allow_local_network_policy_changes_refresh_managed_results_and_unlock_when_not_restricted() {
    policy_changes_refresh_managed_results_and_unlock_when_not_restricted(AgentSandboxSettingId::AgentSandboxAllowLocalNetwork.as_str(), COPILOT_SANDBOX_ALLOW_LOCAL_NETWORK_KEY, false);
}

#[test]
fn test_managed_generic_policy_changes_refresh_managed_results_and_unlock_when_not_restricted() {
    policy_changes_refresh_managed_results_and_unlock_when_not_restricted("test.managedGeneric", "test.genericPolicy", false);
}

#[test]
fn configuration_changes_do_not_rebuild_assignment_filtered_results() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let f = create_editor(&guard, "@tag:expassigned", false, &["test.assigned", "test.unassigned"]);
    run_with_real_timers(async move {
        f.assignments.set_assignment("test.assigned", true);
        timeout_zero().await;
        f.search_model.reset_update_children_calls();
        let before = first_row(&f.search_model);
        f.change_configuration("test.assigned");
        f.change_configuration("editor.fontSize");
        assert_eq!((f.rebuilds(), same_row(&first_row(&f.search_model), &before)), (0, true));
    });
}

#[test]
fn assignment_changes_refresh_counts_empty_state_and_aria_when_no_setting_is_focused() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let f = create_editor(&guard, "@tag:expassigned", false, &["test.assigned", "test.unassigned"]);
    run_with_real_timers(async move {
        let initial = f.read();
        f.assignments.set_assignment("test.assigned", true);
        timeout_zero().await;
        let assigned = f.read();
        f.assignments.set_assignment("test.assigned", false);
        timeout_zero().await;
        assert_eq!(initial, empty_state());
        assert_eq!(assigned, found_state(1, &["test.assigned"]));
        assert_eq!(f.read(), empty_state());
    });
}

/// O corpo do `for (const changedKey of ['test.assigned', 'test.unassigned'])`: `defers assignment refresh for {changedKey} until the focused setting loses focus`.
fn defers_assignment_refresh_until_the_focused_setting_loses_focus(changed_key: &'static str) {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let store = guard.store();
    let f = create_editor(&guard, "@tag:expassigned", false, &["test.assigned", "test.unassigned"]);
    run_with_real_timers(async move {
        f.assignments.set_assignment("test.assigned", true);
        timeout_zero().await;
        f.search_model.reset_update_children_calls();
        let before = f.read();
        let row = first_row(&f.search_model);
        f.view.focus(Active::SettingInput);
        let assigned = changed_key == "test.unassigned";
        f.assignments.set_assignment(changed_key, assigned);
        f.assignments.set_assignment(changed_key, !assigned);
        f.assignments.set_assignment(changed_key, assigned);
        timeout_zero().await;
        f.change_configuration("editor.fontSize");
        let during = (f.read(), f.view.is_active(Active::SettingInput), same_row(&first_row(&f.search_model), &row), f.rebuilds());
        let rendered = Rc::new(Cell::new(false));
        let rendered_flag = rendered.clone();
        store.add(f.view.on_did_render().listen(move |()| rendered_flag.set(true)));
        f.view.focus(Active::SearchInput);
        // Hidden test windows change activeElement without dispatching native blur events.
        f.view.dispatch_blur(Active::SettingInput);
        assert!(rendered.get(), "a árvore foi redesenhada ao perder o foco");

        assert_eq!(during, (before, true, true, 0));
        assert_eq!(f.read(), if assigned { found_state(2, &["test.assigned", "test.unassigned"]) } else { empty_state() });
        assert_eq!(f.rebuilds(), 1);
        assert!(f.view.is_active(Active::SearchInput));
    });
}

#[test]
fn defers_assignment_refresh_for_test_assigned_until_the_focused_setting_loses_focus() {
    defers_assignment_refresh_until_the_focused_setting_loses_focus("test.assigned");
}

#[test]
fn defers_assignment_refresh_for_test_unassigned_until_the_focused_setting_loses_focus() {
    defers_assignment_refresh_until_the_focused_setting_loses_focus("test.unassigned");
}

#[test]
fn defers_assignment_refresh_while_a_context_menu_is_focused() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let store = guard.store();
    let f = create_editor(&guard, "@tag:expassigned", false, &["test.assigned", "test.unassigned"]);
    run_with_real_timers(async move {
        f.assignments.set_assignment("test.assigned", true);
        timeout_zero().await;
        f.search_model.reset_update_children_calls();
        let before = f.read();
        // `contextViewService.showContextView(...)` e `menuItem.focus()`.
        f.view.focus(Active::ContextMenuItem);
        f.assignments.set_assignment("test.assigned", false);
        timeout_zero().await;
        let during = (f.read(), f.view.is_active(Active::ContextMenuItem), f.rebuilds());
        let rendered = Rc::new(Cell::new(false));
        let rendered_flag = rendered.clone();
        store.add(f.view.on_did_render().listen(move |()| rendered_flag.set(true)));
        f.view.focus(Active::SearchInput);
        f.view.dispatch_blur(Active::ContextMenuItem);
        assert!(rendered.get(), "a árvore foi redesenhada ao fechar o menu");

        assert_eq!(during, (before, true, 0));
        assert_eq!(f.read(), empty_state());
        assert_eq!(f.rebuilds(), 1);
    });
}

#[test]
fn assignment_changes_do_not_rebuild_when_the_assignment_filter_is_inactive() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let f = create_editor(&guard, "@modified", false, &["test.assigned", "test.unassigned"]);
    run_with_real_timers(async move {
        f.assignments.set_assignment("test.assigned", true);
        timeout_zero().await;
        assert_eq!(f.rebuilds(), 0);
    });
}

#[test]
fn batches_sequential_assignment_resolutions_into_one_editor_rebuild() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let f = create_editor(&guard, "@tag:expassigned", false, &["test.assigned", "test.unassigned"]);
    run_with_real_timers(async move {
        for key in ["test.assigned", "test.unassigned"] {
            Promise::resolved(()).await.expect("await Promise.resolve()");
            f.assignments.set_assignment(key, true);
        }
        let before_flush = f.rebuilds();
        timeout_zero().await;
        let state = f.read();
        assert_eq!(
            (before_flush, f.rebuilds(), state.rendered_keys, state.count),
            (0, 1, vec!["test.assigned".to_owned(), "test.unassigned".to_owned()], "2 Settings Found".to_owned())
        );
    });
}

#[test]
fn advanced_assigned_settings_survive_editor_filtering_while_unassigned_settings_do_not() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let f = create_editor(&guard, "@tag:expassigned", true, &["test.assigned", "test.unassigned"]);
    run_with_real_timers(async move {
        f.assignments.set_assignment("test.assigned", true);
        timeout_zero().await;
        assert_eq!((setting_keys_of(&f.search_model), f.read().count), (vec!["test.assigned".to_owned()], "1 Setting Found".to_owned()));
    });
}

#[test]
fn assignment_filter_includes_advanced_settings_in_the_resolved_toc_and_category_filtering() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let setting_key = "chat.detectParticipant.enabled";
    let f = create_editor(&guard, "", true, &[setting_key, "chat.detectParticipant.unassigned"]);
    run_with_real_timers(async move {
        f.editor.on_config_update(None, false, false).await.expect("onConfigUpdate");
        let in_root = || f.settings_model.get_elements_by_name(setting_key).is_some_and(|elements| !elements.is_empty());
        let hidden_initially = !in_root();
        f.assignments.set_assignment(setting_key, true);
        timeout_zero().await;

        let change_query = |query: &'static str| {
            f.view_state.borrow_mut().query = Some(query.to_owned());
            f.editor.trigger_search(query, true)
        };
        change_query("@tag:expassigned").await.expect("triggerSearch");
        let search_model = f.editor.search_result_model().expect("o modelo dos resultados da busca");
        let assigned = first_row(&search_model);
        assert!(assigned.as_setting().is_some(), "assert.ok(assigned instanceof SettingsTreeSettingElement)");
        let find_group = |children: Vec<SettingsTreeGroupChild>, id: &str| children.iter().find_map(|child| child.as_group().filter(|group| group.id() == id).cloned());
        let chat = find_group(f.settings_model.root().children(), "chat");
        let context = chat.and_then(|chat| find_group(chat.children(), "chat/context"));
        let category_visible = context.as_ref().is_some_and(|context| matches!(f.tree_filter.filter(&SettingsTreeGroupChild::Group(context.clone()), TreeVisibility::Visible), TreeFilterResult::Bool(true)));
        f.view_state.borrow_mut().category_filter = Some(context.clone().unwrap_or_else(|| f.settings_model.root()));
        let category_contains_assignment = matches!(f.tree_filter.filter(&assigned, TreeVisibility::Visible), TreeFilterResult::Bool(true));
        let active = (
            in_root(),
            search_model.get_unique_results_count(),
            f.settings_model.root().count(),
            context.as_ref().and_then(|context| context.count()),
            category_visible,
            category_contains_assignment,
        );
        f.view_state.borrow_mut().category_filter = None;
        change_query("").await.expect("triggerSearch");
        let hidden_after_removing_filter = !in_root();
        change_query("@tag:expassigned").await.expect("triggerSearch");
        let restored = in_root();

        assert!(hidden_initially);
        assert_eq!(active, (true, 1, Some(1), Some(1), true, true));
        assert!(hidden_after_removing_filter);
        assert!(restored);
    });
}

#[test]
fn advanced_assignment_exemption_applies_to_text_search_and_preserves_policy_filtering() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let f = create_editor(&guard, "@tag:expassigned example", true, &["test.assigned", "test.unassigned"]);
    run_with_real_timers(async move {
        f.assignments.set_assignment("test.assigned", true);
        timeout_zero().await;
        let filter_matches = f
            .settings
            .iter()
            .map(|setting| ISettingMatch::new(setting.clone(), Some(Vec::new()), SettingMatchType::DescriptionOrValueMatch, 0.0, 1.0))
            .collect();
        let provider = Rc::new(FixedSearchProvider { result: ISearchResult { filter_matches, exact_match: false } });
        f.editor.search_with_provider(SearchResultIdx::Local, provider, "test", CancellationToken::None).await.expect("searchWithProvider");
        let assigned_keys = setting_keys_of(&f.editor.search_result_model().expect("o modelo dos resultados da busca"));
        {
            let mut view_state = f.view_state.borrow_mut();
            view_state.query = Some(String::new());
            view_state.tag_filters = Some(HashSet::new());
        }
        let hidden_normally = !f.editor.should_show_setting(&f.settings[0]);
        f.view_state.borrow_mut().tag_filters.get_or_insert_with(HashSet::new).insert(POLICY_SETTING_TAG.to_owned());
        assert_eq!((assigned_keys, hidden_normally, f.editor.should_show_setting(&f.settings[0])), (vec!["test.assigned".to_owned()], true, true));
    });
}

// ---------------------------------------------------------------------------------------------------------------------
// suite('isSettingsSearchUpToDate')

#[test]
fn allows_focus_when_search_is_idle_and_query_matches_rendered_results() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    assert!(is_settings_search_up_to_date(false, Some("font"), "font"));
    assert!(is_settings_search_up_to_date(false, Some(""), ""));
}

#[test]
fn trims_the_current_search_value_before_comparing() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    assert!(is_settings_search_up_to_date(false, Some("font"), "  font  "));
    assert!(!is_settings_search_up_to_date(false, Some("font"), " font size "));
}

#[test]
fn blocks_focus_while_a_debounced_search_is_pending() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    assert!(!is_settings_search_up_to_date(true, Some("font"), "font"));
    assert!(!is_settings_search_up_to_date(true, Some(""), ""));
}

#[test]
fn blocks_focus_when_rendered_results_are_stale() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    assert!(!is_settings_search_up_to_date(false, Some("font"), "theme"));
    assert!(!is_settings_search_up_to_date(false, Some("font"), ""));
    assert!(!is_settings_search_up_to_date(false, None, ""));
}

#[test]
fn uses_delayer_is_triggered_return_value_regression_327360() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let store = guard.store();
    run_with_faked_timers(async move {
        let delayer = store.add_rc(Rc::new(Delayer::<()>::new(Delay::Millis(1000))));

        assert!(!delayer.is_triggered());
        assert!(is_settings_search_up_to_date(delayer.is_triggered(), Some("font"), "font"));

        let pending = delayer.trigger(|| MaybePromise::Value(()), None);
        assert!(delayer.is_triggered());
        assert!(!is_settings_search_up_to_date(delayer.is_triggered(), Some("font"), "font"));

        delayer.cancel();
        let outcome = pending.await;
        assert!(outcome.as_ref().is_err_and(errors::is_cancellation_error), "assert.rejects: o trigger cancelado rejeita");
        assert!(!delayer.is_triggered());
        assert!(is_settings_search_up_to_date(delayer.is_triggered(), Some("font"), "font"));
    });
}
