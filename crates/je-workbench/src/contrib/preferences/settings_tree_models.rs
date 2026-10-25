//! Porte de `src/vs/workbench/contrib/preferences/browser/settingsTreeModels.ts`: o modelo da árvore do editor de
//! configurações. Os elementos da árvore (grupo, configuração, extensões novas), o modelo que os cria a partir do
//! sumário (`SettingsTreeModel`), o modelo dos resultados da busca (`SearchResultModel`), a tradução da chave em
//! rótulo (`settingKeyToDisplayFormat`) e o analisador da consulta (`parseQuery`).
//!
//! Divergências:
//! - **Injeção.** Os serviços decorados (`@IWorkbenchConfigurationService`, `@ILanguageService`,
//!   `@IUserDataProfileService`, `@IProductService`, `@IWorkbenchEnvironmentService`, `@IExperimentalSettingsService`,
//!   `@IManagedSettingsPresentationService`) vão no [`SettingsTreeServices`], um pacote de `Rc<dyn ...>` que o modelo e
//!   cada elemento recebem (no original os mesmos sete parâmetros se repetem em três construtores). O `isSessionsWindow`
//!   que o modelo lê do ambiente chega ao elemento como `bool`, como no original.
//! - **Herança.** `SettingsTreeElement` (uma classe abstrata com `id`, `parent` e `tabbable`) é o trait
//!   [`SettingsTreeElement`], e cada elemento guarda um `ElementBase` com o `Disposable`, o `id`, o `parent` e o emissor de
//!   `onDidChangeTabbable`. `SearchResultModel extends SettingsTreeModel` guarda o modelo base e o expõe por `Deref`.
//! - **Elementos compartilhados.** Os filhos de um grupo são o enum [`SettingsTreeGroupChild`] de `Rc`. O `parent` é
//!   um `Rc` forte (o original guarda a referência e o coletor de lixo desfaz o ciclo): o `dispose` de um grupo esvazia
//!   a lista de filhos, e é isso que desfaz o ciclo pai e filho aqui. Um filho que passa de um grupo descartado para
//!   outro (o `update` do modelo) continua apontando o grupo antigo, como no original, e o `id` dele vale o mesmo.
//! - **`children`** de um grupo é um método que devolve a lista copiada e [`SettingsTreeGroupElement::set_children`], que
//!   refaz o conjunto de chaves (o `set children` do original). O `this._root.children.push(newExtElement)` do
//!   `updateChildren`, que mexe no array sem passar pelo setter, é [`SettingsTreeGroupElement::push_child`].
//! - **`ISettingsEditorViewState`** é compartilhado e mutável (os testes e o editor trocam `settingsTarget` e os filtros
//!   depois de criar o modelo): é um [`ViewStateRef`], `Rc<RefCell<...>>`. Os `Set<string>` são `HashSet<String>`.
//! - **`update(newTocRoot = this._tocRoot)`**: o `_tocRoot` nunca recebe valor no original (é sempre `undefined`),
//!   então chamar `update()` sem argumento lança `TypeError`; aqui o argumento é obrigatório e `_tocRoot` não existe.
//! - **`_root`** é `Option` até o primeiro `update`; [`SettingsTreeModel::root`] falha (como o original, que devolve
//!   `undefined` e quebra no primeiro uso) quando o modelo ainda não foi atualizado.
//! - **`rawSearchResults: ISearchResult[]`** (um array esparso, com `delete` em posições) é `Vec<Option<ISearchResult>>`.
//!   O `combinedFilterMatches = localResult.filterMatches` do `getUniqueSearchResults`, que ordena no lugar a lista do
//!   próprio resultado local (o array é o mesmo), é reproduzido gravando a ordem de volta no resultado local.
//! - **`ISetting.internalOrder`**, que o `sortResults` grava em objetos compartilhados, é um `Cell` (ver `preferences.rs`).
//! - **`Registry.as<IConfigurationRegistry>(Extensions.Configuration)`** é o [`configuration_registry`], e as funções do
//!   esquema da propriedade (`policyValueDescription`) são chamadas como [`Dynamic::Function`].
//! - **`localize`** das etiquetas de `settingKeyToDisplayFormat` é o `localize` de `je_base`, em inglês sem pacote de
//!   idioma. As expressões regulares são as `RegExp` ECMAScript do `zyarr`, e o texto delas é UTF-16.

use std::cell::{Cell, RefCell};
use std::collections::{HashMap, HashSet};
use std::ops::Deref;
use std::rc::Rc;

use indexmap::IndexMap;
use je_base::JsString;
use je_base::arrays::{self, CompareResult};
use je_base::dynamic::{Dynamic, Truthy};
use je_base::errors::{self, Error};
use je_base::event::{Emitter, Event};
use je_base::js_regexp::{Captures, replace_all};
use je_base::js_string::{index_of, trim};
use je_base::lifecycle::{Disposable, IDisposable};
use je_base::nls::localize;
use je_base::strings::escape_reg_exp_characters;
use je_editor::language_service::ILanguageService;
use je_platform::configuration::models::InspectValue;
use je_platform::configuration::registry::{ConfigurationDefaultValueSource, ConfigurationScope, configuration_registry, edit_presentation_types};
use je_platform::configuration::service::IConfigurationService;
use je_platform::configuration::{ConfigurationOverrides, ConfigurationValue, get_language_tag_setting_plain_key};
use je_platform::product::IProductService;
use je_platform::request::USER_LOCAL_AND_REMOTE_SETTINGS;
use je_platform::sandbox::settings::AgentSandboxSettingId;
use zyarr::RegExp;

use crate::contrib::preferences::common::preferences::{
    AGENTS_WINDOW_SETTING_TAG, ENABLE_EXTENSION_TOGGLE_SETTINGS, ENABLE_LANGUAGE_FILTER, EXP_ASSIGNMENT_SETTING_TAG, MODIFIED_SETTING_TAG, POLICY_SETTING_TAG,
    REQUIRE_TRUSTED_WORKSPACE_SETTING_TAG, compare_two_nullable_numbers, wordify_key,
};
use crate::contrib::preferences::preferences_widgets::SettingsTarget;
use crate::contrib::preferences::settings_layout::{ITOCEntry, toc_data};
use crate::services::configuration::common::configuration::{
    APPLICATION_SCOPES, FOLDER_SCOPES, IWorkbenchConfigurationService, LOCAL_MACHINE_SCOPES, REMOTE_MACHINE_SCOPES, WORKSPACE_SCOPES,
};
use crate::services::configuration::common::experimental_settings::IExperimentalSettingsService;
use crate::services::configuration::common::managed_settings_presentation::IManagedSettingsPresentationService;
use crate::services::environment::environment_service::IWorkbenchEnvironmentService;
use crate::services::preferences::preferences::{
    IExtensionSetting, ISearchResult, ISetting, ISettingMatch, SettingMatchType, SettingSchemaType, SettingValueType,
};
use crate::services::user_data_profile::user_data_profile::IUserDataProfileService;

pub const ONLINE_SERVICES_SETTING_TAG: &str = "usesOnlineServices";

/// O pacote de serviços que o modelo e os elementos da árvore recebem por injeção (ver a divergência no topo).
#[derive(Clone)]
pub struct SettingsTreeServices {
    pub configuration_service: Rc<dyn IWorkbenchConfigurationService>,
    pub language_service: Rc<dyn ILanguageService>,
    pub user_data_profile_service: Rc<dyn IUserDataProfileService>,
    pub product_service: Rc<dyn IProductService>,
    pub environment_service: Rc<dyn IWorkbenchEnvironmentService>,
    pub experimental_settings_service: Rc<dyn IExperimentalSettingsService>,
    pub managed_settings_presentation_service: Rc<dyn IManagedSettingsPresentationService>,
}

/// `ISettingsEditorViewState`.
#[derive(Clone)]
pub struct ISettingsEditorViewState {
    pub settings_target: SettingsTarget,
    /// Used to keep track of loading from setInput vs loading from cache.
    pub query: Option<String>,
    pub tag_filters: Option<HashSet<String>>,
    pub extension_filters: Option<HashSet<String>>,
    pub feature_filters: Option<HashSet<String>>,
    pub id_filters: Option<HashSet<String>>,
    pub language_filter: Option<String>,
    pub category_filter: Option<Rc<SettingsTreeGroupElement>>,
}

impl ISettingsEditorViewState {
    /// Um estado só com o alvo (`{ settingsTarget }`), sem consulta nem filtros.
    pub fn new(settings_target: SettingsTarget) -> Self {
        Self {
            settings_target,
            query: None,
            tag_filters: None,
            extension_filters: None,
            feature_filters: None,
            id_filters: None,
            language_filter: None,
            category_filter: None,
        }
    }
}

/// O estado de visão compartilhado entre o editor e o modelo.
pub type ViewStateRef = Rc<RefCell<ISettingsEditorViewState>>;

/// A parte comum dos elementos da árvore (`SettingsTreeElement`).
struct ElementBase {
    base: Rc<Disposable>,
    id: RefCell<String>,
    parent: RefCell<Option<Rc<SettingsTreeGroupElement>>>,
    tabbable: Cell<bool>,
    on_did_change_tabbable: Rc<Emitter<()>>,
}

impl ElementBase {
    fn new(id: &str) -> Self {
        let base = Disposable::new();
        let on_did_change_tabbable = Emitter::new();
        base.register(on_did_change_tabbable.clone());
        Self { base, id: RefCell::new(id.to_owned()), parent: RefCell::new(None), tabbable: Cell::new(false), on_did_change_tabbable }
    }
}

/// `SettingsTreeElement`.
pub trait SettingsTreeElement: IDisposable {
    fn id(&self) -> String;
    fn parent(&self) -> Option<Rc<SettingsTreeGroupElement>>;
    fn set_parent(&self, parent: Option<&Rc<SettingsTreeGroupElement>>);
    fn tabbable(&self) -> bool;
    fn set_tabbable(&self, value: bool);
    fn on_did_change_tabbable(&self) -> Event<()>;
}

/// Os membros de `SettingsTreeElement` a partir do `ElementBase` do campo `element`.
macro_rules! impl_settings_tree_element {
    ($ty:ty) => {
        impl SettingsTreeElement for $ty {
            fn id(&self) -> String {
                self.element.id.borrow().clone()
            }

            fn parent(&self) -> Option<Rc<SettingsTreeGroupElement>> {
                self.element.parent.borrow().clone()
            }

            fn set_parent(&self, parent: Option<&Rc<SettingsTreeGroupElement>>) {
                *self.element.parent.borrow_mut() = parent.cloned();
            }

            fn tabbable(&self) -> bool {
                self.element.tabbable.get()
            }

            fn set_tabbable(&self, value: bool) {
                self.element.tabbable.set(value);
                self.element.on_did_change_tabbable.fire(());
            }

            fn on_did_change_tabbable(&self) -> Event<()> {
                self.element.on_did_change_tabbable.event()
            }
        }
    };
}

/// `SettingsTreeGroupChild`.
#[derive(Clone)]
pub enum SettingsTreeGroupChild {
    Group(Rc<SettingsTreeGroupElement>),
    Setting(Rc<SettingsTreeSettingElement>),
    NewExtensions(Rc<SettingsTreeNewExtensionsElement>),
}

impl SettingsTreeGroupChild {
    fn element(&self) -> &dyn SettingsTreeElement {
        match self {
            Self::Group(element) => &**element,
            Self::Setting(element) => &**element,
            Self::NewExtensions(element) => &**element,
        }
    }

    pub fn id(&self) -> String {
        self.element().id()
    }

    pub fn parent(&self) -> Option<Rc<SettingsTreeGroupElement>> {
        self.element().parent()
    }

    pub fn set_parent(&self, parent: Option<&Rc<SettingsTreeGroupElement>>) {
        self.element().set_parent(parent);
    }

    pub fn dispose(&self) {
        self.element().dispose();
    }

    /// `child instanceof SettingsTreeSettingElement`.
    pub fn as_setting(&self) -> Option<&Rc<SettingsTreeSettingElement>> {
        match self {
            Self::Setting(element) => Some(element),
            _ => None,
        }
    }

    /// `child instanceof SettingsTreeGroupElement`.
    pub fn as_group(&self) -> Option<&Rc<SettingsTreeGroupElement>> {
        match self {
            Self::Group(element) => Some(element),
            _ => None,
        }
    }
}

/// `SettingsTreeGroupElement`.
pub struct SettingsTreeGroupElement {
    element: ElementBase,
    count: Cell<Option<usize>>,
    label: RefCell<String>,
    level: Cell<usize>,
    is_first_group: Cell<bool>,
    child_setting_keys: RefCell<HashSet<String>>,
    children: RefCell<Vec<SettingsTreeGroupChild>>,
}

impl_settings_tree_element!(SettingsTreeGroupElement);

impl SettingsTreeGroupElement {
    pub fn new(id: &str, count: Option<usize>, label: &str, level: usize, is_first_group: bool) -> Rc<Self> {
        Rc::new(Self {
            element: ElementBase::new(id),
            count: Cell::new(count),
            label: RefCell::new(label.to_owned()),
            level: Cell::new(level),
            is_first_group: Cell::new(is_first_group),
            child_setting_keys: RefCell::new(HashSet::new()),
            children: RefCell::new(Vec::new()),
        })
    }

    pub fn count(&self) -> Option<usize> {
        self.count.get()
    }

    pub fn set_count(&self, count: Option<usize>) {
        self.count.set(count);
    }

    pub fn label(&self) -> String {
        self.label.borrow().clone()
    }

    pub fn set_label(&self, label: &str) {
        *self.label.borrow_mut() = label.to_owned();
    }

    pub fn level(&self) -> usize {
        self.level.get()
    }

    pub fn set_level(&self, level: usize) {
        self.level.set(level);
    }

    pub fn is_first_group(&self) -> bool {
        self.is_first_group.get()
    }

    pub fn set_is_first_group(&self, is_first_group: bool) {
        self.is_first_group.set(is_first_group);
    }

    /// `get children()`.
    pub fn children(&self) -> Vec<SettingsTreeGroupChild> {
        self.children.borrow().clone()
    }

    /// `set children(newChildren)`: troca os filhos e refaz o conjunto de chaves dos filhos que são configurações.
    pub fn set_children(&self, new_children: Vec<SettingsTreeGroupChild>) {
        let child_setting_keys: HashSet<String> =
            new_children.iter().filter_map(SettingsTreeGroupChild::as_setting).map(|child| child.setting.key.clone()).collect();
        *self.children.borrow_mut() = new_children;
        *self.child_setting_keys.borrow_mut() = child_setting_keys;
    }

    /// `children.push(child)` direto no array, sem passar pelo setter (o conjunto de chaves não muda).
    pub fn push_child(&self, child: SettingsTreeGroupChild) {
        self.children.borrow_mut().push(child);
    }

    /// Returns whether this group contains the given child key (to a depth of 1 only).
    pub fn contains_setting(&self, key: &str) -> bool {
        self.child_setting_keys.borrow().contains(key)
    }
}

impl IDisposable for SettingsTreeGroupElement {
    fn dispose(&self) {
        self.element.base.dispose();
        // Desfaz o ciclo entre o grupo e os filhos, que apontam para ele (ver a divergência no topo).
        self.children.borrow_mut().clear();
    }
}

/// `SettingsTreeNewExtensionsElement`.
pub struct SettingsTreeNewExtensionsElement {
    element: ElementBase,
    pub extension_ids: Vec<String>,
}

impl_settings_tree_element!(SettingsTreeNewExtensionsElement);

impl SettingsTreeNewExtensionsElement {
    pub fn new(id: &str, extension_ids: Vec<String>) -> Rc<Self> {
        Rc::new(Self { element: ElementBase::new(id), extension_ids })
    }
}

impl IDisposable for SettingsTreeNewExtensionsElement {
    fn dispose(&self) {
        self.element.base.dispose();
    }
}

/// Os valores de `IInspectResult['targetSelector']`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TargetSelector {
    ApplicationValue,
    UserLocalValue,
    UserRemoteValue,
    WorkspaceValue,
    WorkspaceFolderValue,
}

impl TargetSelector {
    /// `inspected[targetSelector]`.
    pub fn value_of(self, inspected: &dyn ConfigurationValue) -> Dynamic {
        match self {
            Self::ApplicationValue => inspected.application_value(),
            Self::UserLocalValue => inspected.user_local_value(),
            Self::UserRemoteValue => inspected.user_remote_value(),
            Self::WorkspaceValue => inspected.workspace_value(),
            Self::WorkspaceFolderValue => inspected.workspace_folder_value(),
        }
    }

    /// `inspected[targetOverrideSelector]` (`application`, `userLocal`, `userRemote`, `workspace`, `workspaceFolder`).
    fn override_of(self, inspected: &dyn ConfigurationValue) -> Option<InspectValue> {
        match self {
            Self::ApplicationValue => inspected.application(),
            Self::UserLocalValue => inspected.user_local(),
            Self::UserRemoteValue => inspected.user_remote(),
            Self::WorkspaceValue => inspected.workspace(),
            Self::WorkspaceFolderValue => inspected.workspace_folder(),
        }
    }
}

/// `typeof inspectValue?.override !== 'undefined'`.
fn has_override(inspect_value: Option<InspectValue>) -> bool {
    inspect_value.is_some_and(|inspect_value| !inspect_value.override_value().is_undefined())
}

/// `IInspectResult`.
pub struct IInspectResult {
    pub is_configured: bool,
    pub inspected: Rc<dyn ConfigurationValue>,
    pub target_selector: TargetSelector,
    pub inspected_language_overrides: HashMap<String, Rc<dyn ConfigurationValue>>,
    pub language_selector: Option<String>,
}

/// `!value` de um texto opcional: ausente ou vazio.
fn is_falsy_text(value: Option<&str>) -> bool {
    value.is_none_or(str::is_empty)
}

/// `SettingsTreeSettingElement`.
pub struct SettingsTreeSettingElement {
    element: ElementBase,
    pub setting: Rc<ISetting>,
    pub settings_target: SettingsTarget,
    is_workspace_trusted: bool,
    language_filter: Option<String>,
    services: SettingsTreeServices,
    is_sessions_window: bool,

    display_category: RefCell<Option<String>>,
    display_label: RefCell<Option<String>>,
    value: RefCell<Dynamic>,
    scope_value: RefCell<Dynamic>,
    default_value: RefCell<Dynamic>,
    default_value_source: RefCell<Option<ConfigurationDefaultValueSource>>,
    is_configured: Cell<bool>,
    is_untrusted: Cell<bool>,
    has_policy_value: Cell<bool>,
    policy_value_description: RefCell<Option<String>>,
    is_agents_window_read_only: Cell<bool>,
    tags: RefCell<Option<HashSet<String>>>,
    overridden_scope_list: RefCell<Vec<String>>,
    overridden_defaults_language_list: RefCell<Vec<String>>,
    language_override_values: RefCell<HashMap<String, Rc<dyn ConfigurationValue>>>,
    description: String,
    value_type: SettingValueType,
}

impl_settings_tree_element!(SettingsTreeSettingElement);

impl SettingsTreeSettingElement {
    const MAX_DESC_LINES: usize = 20;

    #[allow(clippy::too_many_arguments)]
    pub fn new(
        setting: Rc<ISetting>,
        parent: &Rc<SettingsTreeGroupElement>,
        settings_target: SettingsTarget,
        is_workspace_trusted: bool,
        language_filter: Option<String>,
        services: SettingsTreeServices,
        is_sessions_window: bool,
    ) -> Rc<Self> {
        let element = ElementBase::new(&sanitize_id(&format!("{}_{}", parent.id(), setting.key)));
        *element.parent.borrow_mut() = Some(parent.clone());
        // Make sure description and valueType are initialized
        let description = Self::init_setting_description(&setting);
        let value_type = init_setting_value_type(&setting, &*services.product_service);
        Rc::new(Self {
            element,
            default_value_source: RefCell::new(None),
            setting,
            settings_target,
            is_workspace_trusted,
            language_filter,
            services,
            is_sessions_window,
            display_category: RefCell::new(None),
            display_label: RefCell::new(None),
            value: RefCell::new(Dynamic::Undefined),
            scope_value: RefCell::new(Dynamic::Undefined),
            default_value: RefCell::new(Dynamic::Undefined),
            is_configured: Cell::new(false),
            is_untrusted: Cell::new(false),
            has_policy_value: Cell::new(false),
            policy_value_description: RefCell::new(None),
            is_agents_window_read_only: Cell::new(false),
            tags: RefCell::new(None),
            overridden_scope_list: RefCell::new(Vec::new()),
            overridden_defaults_language_list: RefCell::new(Vec::new()),
            language_override_values: RefCell::new(HashMap::new()),
            description,
            value_type,
        })
    }

    /// The displayed value, including inherited values in the Agents Window scope.
    pub fn value(&self) -> Dynamic {
        self.value.borrow().clone()
    }

    /// The value in the current settings scope.
    pub fn scope_value(&self) -> Dynamic {
        self.scope_value.borrow().clone()
    }

    /// The default value.
    pub fn default_value(&self) -> Dynamic {
        self.default_value.borrow().clone()
    }

    /// The source of the default value to display.
    /// This value also accounts for extension-contributed language-specific default value overrides.
    pub fn default_value_source(&self) -> Option<ConfigurationDefaultValueSource> {
        self.default_value_source.borrow().clone()
    }

    /// Whether the setting is configured in the selected scope.
    pub fn is_configured(&self) -> bool {
        self.is_configured.get()
    }

    /// Whether the setting requires trusted target.
    pub fn is_untrusted(&self) -> bool {
        self.is_untrusted.get()
    }

    /// Whether the setting is under a policy that blocks all changes.
    pub fn has_policy_value(&self) -> bool {
        self.has_policy_value.get()
    }

    pub fn policy_value_description(&self) -> Option<String> {
        self.policy_value_description.borrow().clone()
    }

    /// Whether the setting is read-only in the Agents window.
    pub fn is_agents_window_read_only(&self) -> bool {
        self.is_agents_window_read_only.get()
    }

    pub fn tags(&self) -> Option<HashSet<String>> {
        self.tags.borrow().clone()
    }

    pub fn overridden_scope_list(&self) -> Vec<String> {
        self.overridden_scope_list.borrow().clone()
    }

    pub fn overridden_defaults_language_list(&self) -> Vec<String> {
        self.overridden_defaults_language_list.borrow().clone()
    }

    /// For each language that contributes setting values or default overrides, we can see those values here.
    pub fn language_override_values(&self) -> HashMap<String, Rc<dyn ConfigurationValue>> {
        self.language_override_values.borrow().clone()
    }

    pub fn description(&self) -> &str {
        &self.description
    }

    pub fn value_type(&self) -> SettingValueType {
        self.value_type
    }

    pub fn has_ex_p_assignment(&self) -> bool {
        self.services.experimental_settings_service.has_assignment(&self.setting.key)
    }

    pub fn display_category(&self) -> String {
        if is_falsy_text(self.display_category.borrow().as_deref()) {
            self.init_labels();
        }
        self.display_category.borrow().clone().unwrap_or_default()
    }

    pub fn display_label(&self) -> String {
        if is_falsy_text(self.display_label.borrow().as_deref()) {
            self.init_labels();
        }
        self.display_label.borrow().clone().unwrap_or_default()
    }

    fn init_labels(&self) {
        if let Some(title) = self.setting.title.as_deref().filter(|title| !title.is_empty()) {
            *self.display_label.borrow_mut() = Some(title.to_owned());
            *self.display_category.borrow_mut() = self.setting.category_label.clone();
            return;
        }
        let parent_id = self.parent().map(|parent| parent.id()).unwrap_or_default();
        let display_key_format = setting_key_to_display_format(&self.setting.key, &parent_id, self.setting.is_language_tag_setting.unwrap_or(false));
        *self.display_label.borrow_mut() = Some(display_key_format.label);
        *self.display_category.borrow_mut() = Some(display_key_format.category);
    }

    fn init_setting_description(setting: &ISetting) -> String {
        if setting.description.len() > Self::MAX_DESC_LINES {
            let mut truncated_desc_lines: Vec<&str> = setting.description[..Self::MAX_DESC_LINES].iter().map(String::as_str).collect();
            truncated_desc_lines.push("[...]");
            truncated_desc_lines.join("\n")
        } else {
            setting.description.join("\n")
        }
    }

    pub fn inspect_self(&self) {
        let target_to_inspect = self.get_target_to_inspect(&self.setting);
        let inspect_result = inspect_setting(&self.setting.key, &target_to_inspect, self.language_filter.as_deref(), &*self.services.configuration_service);
        self.update(inspect_result, self.is_workspace_trusted);
    }

    fn get_target_to_inspect(&self, setting: &ISetting) -> SettingsTarget {
        let current_profile = self.services.user_data_profile_service.current_profile();
        if !current_profile.is_default && !current_profile.use_default_flags.and_then(|flags| flags.settings).unwrap_or(false) {
            if setting.scope == Some(ConfigurationScope::Application) {
                return SettingsTarget::Application;
            }
            if self.services.configuration_service.is_setting_applied_for_all_profiles(&setting.key) && self.settings_target == SettingsTarget::UserLocal {
                return SettingsTarget::Application;
            }
        }
        self.settings_target.clone()
    }

    fn update(&self, inspect_result: IInspectResult, is_workspace_trusted: bool) {
        let IInspectResult { mut is_configured, inspected, target_selector, inspected_language_overrides, language_selector } = inspect_result;
        let inspected: &dyn ConfigurationValue = &*inspected;
        let language_selector_text = language_selector.as_deref().filter(|selector| !selector.is_empty());
        let language_service = &self.services.language_service;

        if matches!(target_selector, TargetSelector::WorkspaceFolderValue | TargetSelector::WorkspaceValue) {
            self.is_untrusted.set(self.setting.restricted.unwrap_or(false) && !is_workspace_trusted);
        }

        let inherited_value =
            if self.is_sessions_window && target_selector == TargetSelector::WorkspaceValue && language_selector_text.is_none() { inspected.value() } else { inspected.default_value() };
        let mut display_value = if is_configured { target_selector.value_of(inspected) } else { inherited_value };
        let mut overridden_scope_list: Vec<String> = Vec::new();
        let mut overridden_defaults_language_list: Vec<String> = Vec::new();
        if (language_selector_text.is_some() || target_selector != TargetSelector::WorkspaceValue) && !inspected.workspace_value().is_undefined() {
            overridden_scope_list.push("workspace:".to_owned());
        }
        if (language_selector_text.is_some() || target_selector != TargetSelector::UserRemoteValue) && !inspected.user_remote_value().is_undefined() {
            overridden_scope_list.push("remote:".to_owned());
        }
        if (language_selector_text.is_some() || target_selector != TargetSelector::UserLocalValue) && !inspected.user_local_value().is_undefined() {
            overridden_scope_list.push("user:".to_owned());
        }

        if let Some(override_identifiers) = inspected.override_identifiers() {
            for override_identifier in override_identifiers {
                let override_identifier = override_identifier.to_string_lossy();
                let Some(inspected_override) = inspected_language_overrides.get(&override_identifier) else {
                    continue;
                };
                if language_service.is_registered_language_id(Some(&override_identifier)) {
                    let is_selected_language = language_selector.as_deref() == Some(override_identifier.as_str());
                    if !is_selected_language && has_override(inspected_override.default_inspect()) {
                        overridden_defaults_language_list.push(override_identifier.clone());
                    }
                    if (!is_selected_language || target_selector != TargetSelector::WorkspaceValue) && has_override(inspected_override.workspace()) {
                        overridden_scope_list.push(format!("workspace:{override_identifier}"));
                    }
                    if (!is_selected_language || target_selector != TargetSelector::UserRemoteValue) && has_override(inspected_override.user_remote()) {
                        overridden_scope_list.push(format!("remote:{override_identifier}"));
                    }
                    if (!is_selected_language || target_selector != TargetSelector::UserLocalValue) && has_override(inspected_override.user_local()) {
                        overridden_scope_list.push(format!("user:{override_identifier}"));
                    }
                }
                self.language_override_values.borrow_mut().insert(override_identifier, inspected_override.clone());
            }
        }
        *self.overridden_scope_list.borrow_mut() = overridden_scope_list;
        *self.overridden_defaults_language_list.borrow_mut() = overridden_defaults_language_list;

        // The user might have added, removed, or modified a language filter,
        // so we reset the default value source to the non-language-specific default value source for now.
        *self.default_value_source.borrow_mut() = self.setting.non_language_specific_default_value_source.clone();

        // Runtime restrictions affect presentation only; they are not VS Code configuration policies.
        let managed_value = self.services.managed_settings_presentation_service.get_value(&self.setting.key, Some(&inspected.value()));
        let policy_value = match managed_value {
            Some(value) if !value.is_nullish() => value,
            _ => inspected.policy_value(),
        };
        self.has_policy_value.set(!policy_value.is_undefined());
        let policy_value_description = if inspected.policy_value().is_undefined() {
            None
        } else {
            match configuration_registry().get_configuration_properties().get(self.setting.key.as_str()).map(|property| property.get("policyValueDescription")) {
                Some(Dynamic::Function(describe)) => match describe.call(&[inspected.policy_value(), target_selector.value_of(inspected)]) {
                    Dynamic::String(description) => Some(description.to_string_lossy()),
                    _ => None,
                },
                _ => None,
            }
        };
        *self.policy_value_description.borrow_mut() = policy_value_description;
        let language_override = language_selector_text.and_then(|selector| self.language_override_values.borrow().get(selector).cloned());
        if self.has_policy_value.get() {
            is_configured = false; // The user did not manually configure the setting themselves.
            display_value = policy_value.clone();
            *self.scope_value.borrow_mut() = policy_value;
            *self.default_value.borrow_mut() = inspected.default_value();
        } else if let (Some(selector), Some(override_values)) = (language_selector_text, language_override) {
            // In the worst case, go back to using the previous display value.
            // Also, sometimes the override is in the form of a default value override, so consider that second.
            let candidate = if is_configured { target_selector.value_of(&*override_values) } else { override_values.default_value() };
            if !candidate.is_nullish() {
                display_value = candidate;
            }
            *self.scope_value.borrow_mut() = if is_configured { target_selector.value_of(&*override_values) } else { Dynamic::Bool(false) };
            let override_default = override_values.default_value();
            *self.default_value.borrow_mut() = if override_default.is_nullish() { inspected.default_value() } else { override_default };

            let registry_values = configuration_registry().get_configuration_defaults_overrides();
            let source = registry_values.get(&JsString::from(format!("[{selector}]"))).and_then(|entry| entry.source.clone());
            let override_value_source = match source {
                Some(ConfigurationDefaultValueSource::Map(map)) => map.borrow().get(&JsString::from(self.setting.key.as_str())).cloned(),
                _ => None,
            };
            if let Some(override_value_source) = override_value_source {
                *self.default_value_source.borrow_mut() = Some(ConfigurationDefaultValueSource::Source(override_value_source));
            }
        } else {
            *self.scope_value.borrow_mut() = if is_configured { target_selector.value_of(inspected) } else { Dynamic::Bool(false) };
            *self.default_value.borrow_mut() = inspected.default_value();
        }

        let mut has_agents_window_override = false;
        if self.is_sessions_window {
            let agents_window = configuration_registry()
                .get_configuration_properties()
                .get(self.setting.key.as_str())
                .map_or(Dynamic::Undefined, |property| property.get("agentsWindow"));
            has_agents_window_override = agents_window.is_truthy();
            let read_only = matches!(agents_window, Dynamic::Object(_)) && agents_window.get_str("readOnly").is_truthy();
            self.is_agents_window_read_only.set(read_only);
            if read_only {
                is_configured = false;
            }
        }

        *self.value.borrow_mut() = display_value;
        self.is_configured.set(is_configured);
        let has_policy_value = self.has_policy_value.get();
        let has_ex_p_assignment = self.has_ex_p_assignment();
        if is_configured
            || self.setting.tags.is_some()
            || self.tags.borrow().is_some()
            || self.setting.restricted.unwrap_or(false)
            || has_policy_value
            || has_agents_window_override
            || has_ex_p_assignment
        {
            // Don't create an empty Set for all 1000 settings, only if needed
            let mut tags: HashSet<String> = HashSet::new();
            if is_configured {
                tags.insert(MODIFIED_SETTING_TAG.to_owned());
            }

            if let Some(setting_tags) = &self.setting.tags {
                tags.extend(setting_tags.iter().cloned());
            }
            tags.remove(EXP_ASSIGNMENT_SETTING_TAG);
            if has_ex_p_assignment {
                tags.insert(EXP_ASSIGNMENT_SETTING_TAG.to_owned());
            }

            if self.setting.restricted.unwrap_or(false) {
                tags.insert(REQUIRE_TRUSTED_WORKSPACE_SETTING_TAG.to_owned());
            }

            if has_policy_value {
                tags.insert(POLICY_SETTING_TAG.to_owned());
            }

            if has_agents_window_override {
                tags.insert(AGENTS_WINDOW_SETTING_TAG.to_owned());
            }
            *self.tags.borrow_mut() = Some(tags);
        }
    }

    pub fn matches_all_tags(&self, tag_filters: Option<&HashSet<String>>) -> bool {
        let Some(tag_filters) = tag_filters.filter(|filters| !filters.is_empty()) else {
            // This setting, which may have tags,
            // matches against a query with no tags.
            return true;
        };

        if self.tags.borrow().is_none() {
            // The setting must inspect itself to get tag information
            // including for the hasPolicy tag.
            self.inspect_self();
        }
        let tags = self.tags.borrow();
        let tags = tags.as_ref();

        // Handle the special 'stable' tag filter
        if tag_filters.contains("stable") {
            // For stable filter, exclude preview and experimental settings
            if tags.is_some_and(|tags| tags.contains("preview") || tags.contains("experimental")) {
                return false;
            }
            // Check other filters (excluding 'stable' itself)
            let other_filters: Vec<&String> = tag_filters.iter().filter(|tag| tag.as_str() != "stable").collect();
            if other_filters.is_empty() {
                return true;
            }
            return tags.is_some_and(|tags| !tags.is_empty() && other_filters.iter().all(|tag| tags.contains(*tag)));
        }

        // Check that the filter tags are a subset of this setting's tags
        tags.is_some_and(|tags| !tags.is_empty() && tag_filters.iter().all(|tag| tags.contains(tag)))
    }

    pub fn matches_scope(&self, scope: &SettingsTarget, is_remote: bool) -> bool {
        let config_target = scope.configuration_target();

        let Some(setting_scope) = self.setting.scope else {
            return true;
        };
        let key = self.setting.key.as_str();

        use je_platform::configuration::ConfigurationTarget;
        match config_target {
            ConfigurationTarget::Application => APPLICATION_SCOPES.contains(&setting_scope),
            ConfigurationTarget::WorkspaceFolder => FOLDER_SCOPES.contains(&setting_scope),
            ConfigurationTarget::Workspace => WORKSPACE_SCOPES.contains(&setting_scope),
            ConfigurationTarget::UserRemote => REMOTE_MACHINE_SCOPES.contains(&setting_scope) || USER_LOCAL_AND_REMOTE_SETTINGS.contains(&key),
            ConfigurationTarget::UserLocal if is_remote => LOCAL_MACHINE_SCOPES.contains(&setting_scope) || USER_LOCAL_AND_REMOTE_SETTINGS.contains(&key),
            _ => true,
        }
    }

    pub fn matches_any_extension(&self, extension_filters: Option<&HashSet<String>>) -> bool {
        let Some(extension_filters) = extension_filters.filter(|filters| !filters.is_empty()) else {
            return true;
        };

        let Some(extension_info) = &self.setting.extension_info else {
            return false;
        };

        extension_filters.iter().any(|extension_id| extension_id.to_lowercase() == extension_info.id.to_lowercase())
    }

    pub fn matches_any_feature(&self, feature_filters: Option<&HashSet<String>>) -> bool {
        let Some(feature_filters) = feature_filters.filter(|filters| !filters.is_empty()) else {
            return true;
        };

        // Restrict to core settings
        if self.setting.extension_info.is_some() {
            return false;
        }

        let toc = toc_data();
        let toc_children = toc.children.as_ref().expect("o sumário tem filhos");
        let key_matches = |pattern: &String| create_setting_match_reg_exp(pattern).exec(&self.setting.key.encode_utf16().collect::<Vec<u16>>(), 0).is_some();

        // Chat settings are now in their own top-level category
        if feature_filters.contains("chat") {
            let chat_features = toc_children.iter().find(|child| child.id == "chat");
            if let Some(children) = chat_features.and_then(|chat_features| chat_features.children.as_ref())
                && children.iter().flat_map(|feature| feature.settings.iter().flatten()).any(key_matches)
            {
                return true;
            }
        }

        let features = toc_children.iter().find(|child| child.id == "features");
        feature_filters.iter().any(|filter| {
            let Some(children) = features.and_then(|features| features.children.as_ref()) else {
                return false;
            };
            let feature = children.iter().find(|feature| format!("features/{filter}") == feature.id);
            match feature.and_then(|feature| feature.settings.as_ref()) {
                Some(settings) => settings.iter().any(key_matches),
                None => false,
            }
        })
    }

    pub fn matches_any_id(&self, id_filters: Option<&HashSet<String>>) -> bool {
        let Some(id_filters) = id_filters.filter(|filters| !filters.is_empty()) else {
            return true;
        };

        // Check for exact match first
        if id_filters.contains(&self.setting.key) {
            return true;
        }

        // Check for wildcard patterns (ending with .*)
        for filter in id_filters {
            if let Some(prefix) = filter.strip_suffix('*') {
                // Remove '*' suffix
                if self.setting.key.starts_with(prefix) {
                    return true;
                }
            }
        }

        false
    }

    pub fn matches_all_languages(&self, language_filter: Option<&str>) -> bool {
        let Some(language_filter) = language_filter.filter(|filter| !filter.is_empty()) else {
            // We're not filtering by language.
            return true;
        };

        if !self.services.language_service.is_registered_language_id(Some(language_filter)) {
            // We're trying to filter by an invalid language.
            return false;
        }

        // We have a language filter in the search widget at this point.
        // We decide to show all language overridable settings to make the
        // lang filter act more like a scope filter,
        // rather than adding on an implicit @modified as well.
        self.setting.scope == Some(ConfigurationScope::LanguageOverridable)
    }
}

impl IDisposable for SettingsTreeSettingElement {
    fn dispose(&self) {
        self.element.base.dispose();
    }
}

/// `createSettingMatchRegExp`: o padrão com `*` como curinga, sem distinguir a caixa.
pub(crate) fn create_setting_match_reg_exp(pattern: &str) -> RegExp {
    let units: Vec<u16> = pattern.encode_utf16().collect();
    let pattern = escape_reg_exp_characters(&units).to_string_lossy().replace("\\*", ".*");
    RegExp::from_str(&format!("^{pattern}$"), "i").unwrap_or_else(|error| errors::throw(Error::named("SyntaxError", error.message())))
}

/// `SettingsTreeModel`.
pub struct SettingsTreeModel {
    view_state: ViewStateRef,
    services: SettingsTreeServices,
    is_workspace_trusted: Cell<bool>,
    root: RefCell<Option<Rc<SettingsTreeGroupElement>>>,
    tree_elements_by_setting_name: RefCell<IndexMap<String, Vec<Rc<SettingsTreeSettingElement>>>>,
}

impl SettingsTreeModel {
    pub fn new(view_state: ViewStateRef, is_workspace_trusted: bool, services: SettingsTreeServices) -> Rc<Self> {
        Rc::new(Self {
            view_state,
            services,
            is_workspace_trusted: Cell::new(is_workspace_trusted),
            root: RefCell::new(None),
            tree_elements_by_setting_name: RefCell::new(IndexMap::new()),
        })
    }

    pub fn root(&self) -> Rc<SettingsTreeGroupElement> {
        self.root.borrow().clone().unwrap_or_else(|| errors::throw(Error::named("TypeError", "Cannot read properties of undefined (reading 'children')")))
    }

    pub fn update(&self, new_toc_root: &ITOCEntry<Rc<ISetting>>) {
        self.tree_elements_by_setting_name.borrow_mut().clear();

        let new_root = self.create_settings_tree_group_element(new_toc_root, None);
        if let Some(SettingsTreeGroupChild::Group(first)) = new_root.children().first() {
            first.set_is_first_group(true);
        }

        let existing_root = self.root.borrow().clone();
        match existing_root {
            Some(root) => {
                self.dispose_children(&root.children());
                root.set_children(new_root.children());
                new_root.dispose();
            }
            None => *self.root.borrow_mut() = Some(new_root),
        }
    }

    pub fn update_workspace_trust(&self, workspace_trusted: bool) {
        self.is_workspace_trusted.set(workspace_trusted);
        self.update_require_trusted_target_elements();
    }

    fn dispose_children(&self, children: &[SettingsTreeGroupChild]) {
        for child in children {
            self.dispose_child_and_recurse(child);
        }
    }

    fn dispose_child_and_recurse(&self, element: &SettingsTreeGroupChild) {
        if let SettingsTreeGroupChild::Group(group) = element {
            self.dispose_children(&group.children());
        }

        element.dispose();
    }

    pub fn get_elements_by_name(&self, name: &str) -> Option<Vec<Rc<SettingsTreeSettingElement>>> {
        self.tree_elements_by_setting_name.borrow().get(name).cloned()
    }

    pub fn update_elements_by_name(&self, name: &str) {
        let elements = self.tree_elements_by_setting_name.borrow().get(name).cloned();
        if let Some(elements) = elements {
            self.reinspect_settings(&elements);
        }
    }

    fn update_require_trusted_target_elements(&self) {
        let untrusted: Vec<Rc<SettingsTreeSettingElement>> =
            self.tree_elements_by_setting_name.borrow().values().flatten().filter(|setting| setting.is_untrusted()).cloned().collect();
        self.reinspect_settings(&untrusted);
    }

    fn reinspect_settings(&self, settings: &[Rc<SettingsTreeSettingElement>]) {
        for element in settings {
            element.inspect_self();
        }
    }

    fn create_settings_tree_group_element(&self, toc_entry: &ITOCEntry<Rc<ISetting>>, parent: Option<&Rc<SettingsTreeGroupElement>>) -> Rc<SettingsTreeGroupElement> {
        let depth = parent.map_or(0, |parent| self.get_depth(parent) + 1);
        let element = SettingsTreeGroupElement::new(&toc_entry.id, None, &toc_entry.label, depth, false);
        element.set_parent(parent);

        let mut children: Vec<SettingsTreeGroupChild> = Vec::new();
        if let Some(settings) = &toc_entry.settings {
            let setting_children: Vec<Rc<SettingsTreeSettingElement>> =
                settings.iter().map(|setting| self.create_settings_tree_setting_element(setting.clone(), &element)).collect();
            for child in setting_children {
                if is_falsy_text(child.setting.deprecation_message.as_deref()) || child.setting.deprecation_message_show_in_settings.unwrap_or(false) {
                    children.push(SettingsTreeGroupChild::Setting(child));
                } else {
                    child.inspect_self();
                    if child.is_configured() {
                        children.push(SettingsTreeGroupChild::Setting(child));
                    } else {
                        child.dispose();
                    }
                }
            }
        }

        if let Some(toc_children) = &toc_entry.children {
            let group_children: Vec<Rc<SettingsTreeGroupElement>> =
                toc_children.iter().map(|child| self.create_settings_tree_group_element(child, Some(&element))).collect();
            children.extend(group_children.into_iter().map(SettingsTreeGroupChild::Group));
        }

        element.set_children(children);

        element
    }

    fn get_depth(&self, element: &Rc<SettingsTreeGroupElement>) -> usize {
        match element.parent() {
            Some(parent) => 1 + self.get_depth(&parent),
            None => 0,
        }
    }

    fn create_settings_tree_setting_element(&self, setting: Rc<ISetting>, parent: &Rc<SettingsTreeGroupElement>) -> Rc<SettingsTreeSettingElement> {
        let (settings_target, language_filter) = {
            let view_state = self.view_state.borrow();
            (view_state.settings_target.clone(), view_state.language_filter.clone())
        };
        let element = SettingsTreeSettingElement::new(
            setting.clone(),
            parent,
            settings_target,
            self.is_workspace_trusted.get(),
            language_filter,
            self.services.clone(),
            self.services.environment_service.is_sessions_window(),
        );

        self.tree_elements_by_setting_name.borrow_mut().entry(setting.key.clone()).or_default().push(element.clone());
        element
    }
}

impl IDisposable for SettingsTreeModel {
    fn dispose(&self) {
        self.tree_elements_by_setting_name.borrow_mut().clear();
        let root = self.root.borrow().clone();
        if let Some(root) = root {
            self.dispose_child_and_recurse(&SettingsTreeGroupChild::Group(root));
        }
    }
}

/// `inspectSetting`.
pub fn inspect_setting(key: &str, target: &SettingsTarget, language_filter: Option<&str>, configuration_service: &dyn IWorkbenchConfigurationService) -> IInspectResult {
    let inspect_overrides = match target {
        SettingsTarget::Resource(resource) => ConfigurationOverrides::with_resource(resource.clone()),
        _ => ConfigurationOverrides::default(),
    };
    let inspected = configuration_service.inspect(key, &inspect_overrides);
    let target_selector = match target {
        SettingsTarget::Application => TargetSelector::ApplicationValue,
        SettingsTarget::UserLocal => TargetSelector::UserLocalValue,
        SettingsTarget::UserRemote => TargetSelector::UserRemoteValue,
        SettingsTarget::Workspace => TargetSelector::WorkspaceValue,
        SettingsTarget::Resource(_) => TargetSelector::WorkspaceFolderValue,
    };
    let mut is_configured = !target_selector.value_of(&*inspected).is_undefined();

    let override_identifiers = inspected.override_identifiers();
    let mut inspected_language_overrides: HashMap<String, Rc<dyn ConfigurationValue>> = HashMap::new();

    // We must reset isConfigured to be false if languageFilter is set, and manually
    // determine whether it can be set to true later.
    let language_filter_text = language_filter.filter(|filter| !filter.is_empty());
    if language_filter_text.is_some() {
        is_configured = false;
    }
    if let Some(override_identifiers) = override_identifiers {
        // The setting we're looking at has language overrides.
        for override_identifier in override_identifiers {
            let inspected_override = configuration_service.inspect(key, &ConfigurationOverrides::with_identifier(override_identifier.clone()));
            inspected_language_overrides.insert(override_identifier.to_string_lossy(), inspected_override);
        }

        // For all language filters, see if there's an override for that filter.
        if let Some(language_filter) = language_filter_text
            && let Some(inspected_override) = inspected_language_overrides.get(language_filter)
            && let Some(override_value) = target_selector.override_of(&**inspected_override).map(|inspect_value| inspect_value.override_value())
            && !override_value.is_undefined()
        {
            is_configured = true;
        }
    }

    IInspectResult { is_configured, inspected, target_selector, inspected_language_overrides, language_selector: language_filter.map(str::to_owned) }
}

/// `sanitizeId`.
pub fn sanitize_id(id: &str) -> String {
    id.replace(['.', '/'], "_")
}

/// `{ category: string; label: string }`, o que `settingKeyToDisplayFormat` devolve.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SettingDisplayFormat {
    pub category: String,
    pub label: String,
}

/// `settingKeyToDisplayFormat(key, groupId = '', isLanguageTagSetting = false)`.
pub fn setting_key_to_display_format(key: &str, group_id: &str, is_language_tag_setting: bool) -> SettingDisplayFormat {
    let display_label: Option<String> = if key == AgentSandboxSettingId::AgentSandboxAllowNetwork.as_str() {
        Some(localize("agentSandbox.allowNetwork.label", "Allow Outbound Connections", &[]))
    } else if key == AgentSandboxSettingId::AgentSandboxAllowUnsandboxedCommands.as_str() {
        Some(localize("agentSandbox.allowUnsandboxedCommands.label", "Allow Sandbox Bypass", &[]))
    } else if key == AgentSandboxSettingId::AgentSandboxMcpServers.as_str() {
        Some(localize("agentSandbox.mcpServers.label", "Sandbox MCP Servers", &[]))
    } else if key == AgentSandboxSettingId::AgentSandboxLspServers.as_str() {
        Some(localize("agentSandbox.lspServers.label", "Sandbox LSP Servers", &[]))
    } else if key == AgentSandboxSettingId::AgentSandboxAuthenticateGit.as_str() {
        Some(localize("agentSandbox.authenticateGit.label", "Authenticate git", &[]))
    } else if key == AgentSandboxSettingId::AgentSandboxAuthenticateGh.as_str() {
        Some(localize("agentSandbox.authenticateGh.label", "Authenticate gh", &[]))
    } else if key == AgentSandboxSettingId::AgentSandboxUserConfiguredPaths.as_str() {
        Some(localize("agentSandbox.userConfiguredPaths.label", "User-Configured Paths", &[]))
    } else {
        None
    };
    let (category, key) = match key.rfind('.') {
        Some(last_dot_idx) => (key[..last_dot_idx].to_owned(), key[last_dot_idx + 1..].to_owned()),
        None => (String::new(), key.to_owned()),
    };

    let group_id = group_id.replace('/', ".");
    let category = trim_category_for_group(&category, &group_id);
    let category = wordify_key(&category);

    let key = if is_language_tag_setting { format!("$(bracket) {}", get_language_tag_setting_plain_key(key.as_str()).to_string_lossy()) } else { key };

    let label = display_label.unwrap_or_else(|| wordify_key(&key));
    SettingDisplayFormat { category, label }
}

/// Uma `RegExp` ECMAScript, ou o `SyntaxError` do `new RegExp` do original.
fn compile_reg_exp(pattern: &str, flags: &str) -> RegExp {
    RegExp::from_str(pattern, flags).unwrap_or_else(|error| errors::throw(Error::named("SyntaxError", format!("Invalid regular expression: /{pattern}/{flags}: {}", error.message()))))
}

/// `regex.test(text)`.
fn reg_exp_test(regex: &RegExp, text: &str) -> bool {
    regex.exec(&text.encode_utf16().collect::<Vec<u16>>(), 0).is_some()
}

/// `text.replace(regexSemGlobal, replacement)`: troca só o primeiro casamento.
fn replace_first(regex: &RegExp, text: &str, replacement: &str) -> String {
    let units: Vec<u16> = text.encode_utf16().collect();
    match regex.exec(&units, 0) {
        Some(found) => {
            let mut out = JsString::from(&units[..found.start()]);
            out.push_units(&replacement.encode_utf16().collect::<Vec<u16>>());
            out.push_units(&units[found.end()..]);
            out.to_string_lossy()
        }
        None => text.to_owned(),
    }
}

/// O `doTrim` de `trimCategoryForGroup`. O `groupId` é mutável porque o original o reatribui dentro da função, e a
/// segunda chamada (de trás para a frente) enxerga o valor já alterado pela primeira.
fn do_trim(category: &str, group_id: &mut String, forward: bool) -> Option<String> {
    // Remove the Insiders portion if the category doesn't use it.
    if !reg_exp_test(&compile_reg_exp("insiders$", "i"), category) {
        *group_id = replace_first(&compile_reg_exp("-?insiders$", "i"), group_id, "");
    }
    let mut parts: Vec<String> = group_id
        .split('.')
        .map(|part| {
            // Remove hyphens, but only if that results in a match with the category.
            if part.replace('-', "").to_lowercase() == category.to_lowercase() { part.replace('-', "") } else { part.to_owned() }
        })
        .collect();
    while !parts.is_empty() {
        let reg = compile_reg_exp(&format!("^{}(\\.|$)", parts.join("\\.")), "i");
        if reg_exp_test(&reg, category) {
            return Some(replace_first(&reg, category, ""));
        }

        if forward {
            parts.pop();
        } else {
            parts.remove(0);
        }
    }

    None
}

/// Removes redundant sections of the category label.
/// A redundant section is a section already reflected in the groupId.
///
/// - `category`: the category of the specific setting.
/// - `group_id`: the author + extension ID.
///
/// Returns the new category label to use.
fn trim_category_for_group(category: &str, group_id: &str) -> String {
    let mut group_id = group_id.to_owned();
    let mut trimmed = do_trim(category, &mut group_id, true);
    if trimmed.is_none() {
        trimmed = do_trim(category, &mut group_id, false);
    }

    trimmed.unwrap_or_else(|| category.to_owned())
}

fn is_extension_toggle_setting(setting: &ISetting, product_service: &dyn IProductService) -> bool {
    ENABLE_EXTENSION_TOGGLE_SETTINGS && product_service.extension_recommendations().is_some() && !is_falsy_text(setting.display_extension_id.as_deref())
}

fn is_exclude_setting(setting: &ISetting) -> bool {
    matches!(
        setting.key.as_str(),
        "files.exclude" | "search.exclude" | "workbench.localHistory.exclude" | "explorer.autoRevealExclude" | "files.readonlyExclude" | "files.watcherExclude"
    )
}

fn is_include_setting(setting: &ISetting) -> bool {
    setting.key == "files.readonlyInclude"
}

/// The values of the following settings when a default values has been removed.
pub fn object_setting_supports_remove_default_value(key: &str) -> bool {
    key == "workbench.editor.customLabels.patterns"
}

fn is_simple_type(type_: Option<&str>) -> bool {
    matches!(type_, Some("string" | "boolean" | "integer" | "number"))
}

/// O `'simple' | 'complex'` de `getObjectSettingSchemaType` e de `getObjectRenderableSchemaType` (`false` é `None`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum SchemaRenderType {
    Simple,
    Complex,
}

/// O texto de um valor dinâmico, se ele é um texto (o `type` de um esquema que é `string`).
fn text_of(value: &Dynamic) -> Option<String> {
    match value {
        Dynamic::String(text) => Some(text.to_string_lossy()),
        _ => None,
    }
}

/// O `const { type } = schema` do original: desestruturar `undefined` ou `null` lança `TypeError`.
fn schema_type_of(schema: &Dynamic) -> Dynamic {
    if schema.is_nullish() {
        errors::throw(Error::named("TypeError", "Cannot destructure property 'type' of 'schema' as it is undefined."));
    }
    schema.get_str("type")
}

fn get_object_renderable_schema_type(schema: &Dynamic, key: &str) -> Option<SchemaRenderType> {
    let type_ = schema_type_of(schema);

    if let Dynamic::Array(types) = &type_ {
        let types: Vec<Option<String>> = types.to_vec().iter().map(text_of).collect();
        if object_setting_supports_remove_default_value(key) && types.len() == 2 {
            let includes = |name: &str| types.iter().any(|t| t.as_deref() == Some(name));
            if includes("null") && (includes("string") || includes("boolean") || includes("integer") || includes("number")) {
                return Some(SchemaRenderType::Simple);
            }
        }

        for t in &types {
            if !is_simple_type(t.as_deref()) {
                return None;
            }
        }
        return Some(SchemaRenderType::Complex);
    }

    let type_text = text_of(&type_);
    if is_simple_type(type_text.as_deref()) {
        return Some(SchemaRenderType::Simple);
    }

    if type_text.as_deref() == Some("array") {
        let items = schema.get_str("items");
        if items.is_truthy() {
            let item_schemas: Vec<Dynamic> = match &items {
                Dynamic::Array(items) => items.to_vec(),
                other => vec![other.clone()],
            };
            // O laço do original devolve na primeira iteração.
            if let Some(item_schema) = item_schemas.first() {
                let item_type = schema_type_of(item_schema);
                if let Dynamic::Array(types) = &item_type {
                    for t in types.to_vec() {
                        if !is_simple_type(text_of(&t).as_deref()) {
                            return None;
                        }
                    }
                    return Some(SchemaRenderType::Complex);
                }
                if !is_simple_type(text_of(&item_type).as_deref()) {
                    return None;
                }
                return Some(SchemaRenderType::Complex);
            }
        }
        return None;
    }

    None
}

/// `Object.values(map ?? {})`.
fn object_values(map: Option<&Dynamic>) -> Vec<Dynamic> {
    match map {
        Some(map) if !map.is_nullish() => map.own_entries().into_iter().map(|(_, value)| value).collect(),
        _ => Vec::new(),
    }
}

fn get_object_setting_schema_type(setting: &ISetting) -> Option<SchemaRenderType> {
    let ISetting { key, r#type, object_properties, object_pattern_properties, object_additional_properties, .. } = setting;
    if !r#type.as_ref().is_some_and(|type_| type_.is("object")) {
        return None;
    }

    let is_undefined_or_null = |value: &Option<Dynamic>| value.as_ref().is_none_or(Dynamic::is_nullish);

    // object can have any shape
    if is_undefined_or_null(object_properties) && is_undefined_or_null(object_pattern_properties) && is_undefined_or_null(object_additional_properties) {
        return None;
    }

    // objectAdditionalProperties allow the setting to have any shape,
    // but if there's a pattern property that handles everything, then every
    // property will match that patternProperty, so we don't need to look at
    // the value of objectAdditionalProperties in that case.
    let additional_allows_any_shape = match object_additional_properties {
        None => true,
        Some(additional) => additional.strict_equals(&Dynamic::Bool(true)) || additional.is_undefined(),
    };
    let pattern_handles_everything = match object_pattern_properties {
        Some(patterns) if !patterns.is_nullish() => patterns.own_keys().iter().any(|pattern| *pattern == ".*"),
        _ => false,
    };
    if additional_allows_any_shape && !pattern_handles_everything {
        return None;
    }

    let mut schemas: Vec<Dynamic> = object_values(object_properties.as_ref());
    schemas.extend(object_values(object_pattern_properties.as_ref()));

    if let Some(additional) = object_additional_properties
        && additional.is_truthy()
        && matches!(additional, Dynamic::Object(_) | Dynamic::Array(_))
    {
        schemas.push(additional.clone());
    }

    let mut schema_type = SchemaRenderType::Simple;
    for schema in &schemas {
        let any_of = schema.get_str("anyOf");
        let sub_schemas: Vec<Dynamic> = match &any_of {
            Dynamic::Array(sub_schemas) => sub_schemas.to_vec(),
            _ => vec![schema.clone()],
        };
        for sub_schema in &sub_schemas {
            match get_object_renderable_schema_type(sub_schema, key)? {
                SchemaRenderType::Complex => schema_type = SchemaRenderType::Complex,
                SchemaRenderType::Simple => {}
            }
        }
    }

    Some(schema_type)
}

fn setting_type_enum_renderable(type_: &SettingSchemaType) -> bool {
    const ENUM_RENDERABLE_SETTING_TYPES: [&str; 5] = ["string", "boolean", "null", "integer", "number"];
    type_.as_list().iter().all(|type_| ENUM_RENDERABLE_SETTING_TYPES.contains(type_))
}

/// O `initSettingValueType` do elemento: o tipo de valor que a interface usa para editar a configuração.
fn init_setting_value_type(setting: &ISetting, product_service: &dyn IProductService) -> SettingValueType {
    if is_extension_toggle_setting(setting, product_service) {
        SettingValueType::ExtensionToggle
    } else if setting.r#enum.is_some() && setting.r#type.as_ref().is_none_or(|type_| matches!(type_, SettingSchemaType::Single(text) if text.is_empty()) || setting_type_enum_renderable(type_)) {
        SettingValueType::Enum
    } else if setting.r#type.as_ref().is_some_and(|type_| type_.is("string")) {
        if setting.edit_presentation.as_deref() == Some(edit_presentation_types::MULTILINE) { SettingValueType::MultilineString } else { SettingValueType::String }
    } else if is_exclude_setting(setting) {
        SettingValueType::Exclude
    } else if is_include_setting(setting) {
        SettingValueType::Include
    } else if setting.r#type.as_ref().is_some_and(|type_| type_.is("integer")) {
        SettingValueType::Integer
    } else if setting.r#type.as_ref().is_some_and(|type_| type_.is("number")) {
        SettingValueType::Number
    } else if setting.r#type.as_ref().is_some_and(|type_| type_.is("boolean")) {
        SettingValueType::Boolean
    } else if setting.r#type.as_ref().is_some_and(|type_| type_.is("array"))
        && setting.array_item_type.as_deref().is_some_and(|item_type| ["string", "enum", "number", "integer"].contains(&item_type))
    {
        SettingValueType::Array
    } else if let Some(SettingSchemaType::List(types)) = &setting.r#type
        && types.iter().any(|type_| type_ == SettingValueType::Null.as_str())
        && types.len() == 2
    {
        if types.iter().any(|type_| type_ == SettingValueType::Integer.as_str()) {
            SettingValueType::NullableInteger
        } else if types.iter().any(|type_| type_ == SettingValueType::Number.as_str()) {
            SettingValueType::NullableNumber
        } else {
            SettingValueType::Complex
        }
    } else if let Some(schema_type) = get_object_setting_schema_type(setting) {
        if setting.all_keys_are_boolean.unwrap_or(false) {
            SettingValueType::BooleanObject
        } else if schema_type == SchemaRenderType::Simple {
            SettingValueType::Object
        } else {
            SettingValueType::ComplexObject
        }
    } else if setting.is_language_tag_setting.unwrap_or(false) {
        SettingValueType::LanguageTag
    } else {
        SettingValueType::Complex
    }
}

/// `SearchResultIdx`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SearchResultIdx {
    Local = 0,
    Remote = 1,
    NewExtensions = 2,
    Embeddings = 3,
    AiSelected = 4,
}

/// `SearchResultModel`.
pub struct SearchResultModel {
    base: Rc<SettingsTreeModel>,
    environment_service: Rc<dyn IWorkbenchEnvironmentService>,
    raw_search_results: RefCell<Option<Vec<Option<ISearchResult>>>>,
    cached_unique_search_results: RefCell<HashMap<bool, ISearchResult>>,
    new_extension_search_results: RefCell<Option<ISearchResult>>,
    search_result_count: Cell<Option<usize>>,
    settings_order_by_toc_index: Option<HashMap<String, f64>>,
    ai_filter_enabled: Cell<bool>,
    /// Gancho de teste: quantas vezes `update_children` rodou (o `sinon.spy(model, 'updateChildren').callCount` do original).
    update_children_calls: Cell<usize>,
}

impl Deref for SearchResultModel {
    type Target = SettingsTreeModel;

    fn deref(&self) -> &SettingsTreeModel {
        &self.base
    }
}

impl SearchResultModel {
    pub const ID: &'static str = "searchResultModel";

    pub fn new(
        view_state: ViewStateRef,
        settings_order_by_toc_index: Option<HashMap<String, f64>>,
        is_workspace_trusted: bool,
        services: SettingsTreeServices,
    ) -> Rc<Self> {
        let environment_service = services.environment_service.clone();
        let model = Rc::new(Self {
            base: SettingsTreeModel::new(view_state, is_workspace_trusted, services),
            environment_service,
            raw_search_results: RefCell::new(None),
            cached_unique_search_results: RefCell::new(HashMap::new()),
            new_extension_search_results: RefCell::new(None),
            search_result_count: Cell::new(None),
            settings_order_by_toc_index,
            ai_filter_enabled: Cell::new(false),
            update_children_calls: Cell::new(0),
        });
        model.update(&ITOCEntry { id: Self::ID.to_owned(), label: String::new(), order: None, children: None, settings: None, hide: None });
        model
    }

    /// `set showAiResults(show)`.
    pub fn set_show_ai_results(&self, show: bool) {
        self.ai_filter_enabled.set(show);
        self.update_children();
    }

    /// `sortResults`: ordena `filter_matches` no lugar (o `sort` do original) e devolve a lista final, sem repetições
    /// quando há consulta.
    fn sort_results(&self, filter_matches: &mut Vec<ISettingMatch>) -> Vec<ISettingMatch> {
        if let Some(order_by_toc_index) = &self.settings_order_by_toc_index {
            for filter_match in filter_matches.iter() {
                filter_match.setting.internal_order.set(order_by_toc_index.get(&filter_match.setting.key).copied());
            }
        }

        // The search only has filters, so we can sort by the order in the TOC.
        if is_falsy_text(self.base.view_state.borrow().query.as_deref()) {
            arrays::sort_by_compare(filter_matches, |a, b| compare_two_nullable_numbers(a.setting.internal_order.get(), b.setting.internal_order.get()));
            return filter_matches.clone();
        }

        // Sort the settings according to their relevancy.
        // https://github.com/microsoft/vscode/issues/197773
        arrays::sort_by_compare(filter_matches, |a, b| -> CompareResult {
            if a.match_type != b.match_type {
                // Sort by match type if the match types are not the same.
                // The priority of the match type is given by the SettingMatchType enum.
                f64::from(b.match_type.0) - f64::from(a.match_type.0)
            } else if a.match_type.intersects(SettingMatchType::NonContiguousWordsInSettingsLabel) || a.match_type.intersects(SettingMatchType::ContiguousWordsInSettingsLabel) {
                // The match types of a and b are the same and can be sorted by their number of matched words.
                // If those numbers are the same, sort by the order in the table of contents.
                let by_score = b.key_match_score - a.key_match_score;
                if by_score != 0.0 { by_score } else { compare_two_nullable_numbers(a.setting.internal_order.get(), b.setting.internal_order.get()) }
            } else if a.match_type == SettingMatchType::RemoteMatch {
                // The match types are the same and are RemoteMatch.
                // Sort by score.
                b.score - a.score
            } else {
                // The match types are the same but are not RemoteMatch.
                // Sort by their order in the table of contents.
                compare_two_nullable_numbers(a.setting.internal_order.get(), b.setting.internal_order.get())
            }
        });

        // Remove duplicates, which sometimes occur with settings
        // such as the experimental toggle setting.
        arrays::distinct(filter_matches, |filter_match| filter_match.setting.key.clone())
    }

    /// `this.rawSearchResults[idx]` (`undefined` é `None`).
    fn raw_result(&self, idx: SearchResultIdx) -> Option<ISearchResult> {
        self.raw_search_results.borrow().as_ref().and_then(|raw| raw.get(idx as usize).cloned().flatten())
    }

    pub fn get_unique_search_results(&self) -> Option<ISearchResult> {
        let ai_filter_enabled = self.ai_filter_enabled.get();
        if let Some(cached_results) = self.cached_unique_search_results.borrow().get(&ai_filter_enabled) {
            return Some(cached_results.clone());
        }

        self.raw_search_results.borrow().as_ref()?;

        let mut combined_filter_matches: Vec<ISettingMatch> = Vec::new();

        if ai_filter_enabled {
            let mut ai_selected_keys: HashSet<String> = HashSet::new();
            if let Some(ai_selected_result) = self.raw_result(SearchResultIdx::AiSelected) {
                ai_selected_keys.extend(ai_selected_result.filter_matches.iter().map(|m| m.setting.key.clone()));
                combined_filter_matches = ai_selected_result.filter_matches;
            }

            if self.raw_result(SearchResultIdx::Embeddings).is_some()
                && let Some(Some(embeddings_result)) = self.raw_search_results.borrow_mut().as_mut().map(|raw| &mut raw[SearchResultIdx::Embeddings as usize])
            {
                embeddings_result.filter_matches.retain(|m| !ai_selected_keys.contains(&m.setting.key));
                combined_filter_matches.extend(embeddings_result.filter_matches.iter().cloned());
            }
            let result = ISearchResult { filter_matches: combined_filter_matches, exact_match: false };
            self.cached_unique_search_results.borrow_mut().insert(true, result.clone());
            return Some(result);
        }

        let mut local_match_keys: HashSet<String> = HashSet::new();
        let local_result = self.raw_result(SearchResultIdx::Local);
        if let Some(local_result) = &local_result {
            local_match_keys.extend(local_result.filter_matches.iter().map(|m| m.setting.key.clone()));
            combined_filter_matches = local_result.filter_matches.clone();
        }

        let has_remote_result = self.raw_result(SearchResultIdx::Remote).is_some();
        if has_remote_result {
            if let Some(Some(remote_result)) = self.raw_search_results.borrow_mut().as_mut().map(|raw| &mut raw[SearchResultIdx::Remote as usize]) {
                remote_result.filter_matches.retain(|m| !local_match_keys.contains(&m.setting.key));
                combined_filter_matches.extend(remote_result.filter_matches.iter().cloned());
            }

            *self.new_extension_search_results.borrow_mut() = self.raw_result(SearchResultIdx::NewExtensions);
        }
        let sorted = self.sort_results(&mut combined_filter_matches);
        if !has_remote_result && local_result.is_some() {
            // `combinedFilterMatches` é o mesmo array de `localResult.filterMatches`, e o `sort` o reordena no lugar.
            if let Some(Some(local_result)) = self.raw_search_results.borrow_mut().as_mut().map(|raw| &mut raw[SearchResultIdx::Local as usize]) {
                local_result.filter_matches = combined_filter_matches;
            }
        }
        let result = ISearchResult { filter_matches: sorted, exact_match: local_result.is_some_and(|local_result| local_result.exact_match) }; // remote results should never have an exact match
        self.cached_unique_search_results.borrow_mut().insert(false, result.clone());
        Some(result)
    }

    pub fn get_raw_results(&self) -> Vec<Option<ISearchResult>> {
        self.raw_search_results.borrow().clone().unwrap_or_default()
    }

    fn get_unique_search_result_settings(&self) -> Vec<Rc<ISetting>> {
        self.get_unique_search_results().map(|results| results.filter_matches.into_iter().map(|m| m.setting).collect()).unwrap_or_default()
    }

    pub fn update_children(&self) {
        self.update_children_calls.set(self.update_children_calls.get() + 1);
        self.base.update(&ITOCEntry {
            id: Self::ID.to_owned(),
            label: Self::ID.to_owned(),
            order: None,
            children: None,
            settings: Some(self.get_unique_search_result_settings()),
            hide: None,
        });

        // Save time by filtering children in the search model instead of relying on the tree filter, which still requires heights to be calculated.
        let is_remote = self.environment_service.remote_authority().is_some_and(|authority| !authority.is_empty());

        let root = self.base.root();
        let mut new_children: Vec<SettingsTreeGroupChild> = Vec::new();
        {
            let view_state = self.base.view_state.borrow();
            for child in root.children() {
                let matches = match child.as_setting() {
                    Some(setting) => {
                        setting.matches_all_tags(view_state.tag_filters.as_ref())
                            && setting.matches_scope(&view_state.settings_target, is_remote)
                            && setting.matches_any_extension(view_state.extension_filters.as_ref())
                            && setting.matches_any_id(view_state.id_filters.as_ref())
                            && setting.matches_any_feature(view_state.feature_filters.as_ref())
                            && setting.matches_all_languages(view_state.language_filter.as_deref())
                    }
                    None => false,
                };
                if matches {
                    new_children.push(child);
                } else {
                    child.dispose();
                }
            }
        }
        self.search_result_count.set(Some(new_children.len()));
        root.set_children(new_children);

        let new_extension_search_results = self.new_extension_search_results.borrow().clone();
        if let Some(new_extension_search_results) = new_extension_search_results.filter(|results| !results.filter_matches.is_empty()) {
            let result_extension_ids: Vec<String> = new_extension_search_results
                .filter_matches
                .iter()
                .map(|result| &result.setting)
                .filter(|setting: &&Rc<IExtensionSetting>| !is_falsy_text(setting.extension_name.as_deref()) && !is_falsy_text(setting.extension_publisher.as_deref()))
                .map(|setting| format!("{}.{}", setting.extension_publisher.as_deref().unwrap_or_default(), setting.extension_name.as_deref().unwrap_or_default()))
                .collect();
            let result_extension_ids = arrays::distinct(&result_extension_ids, |id| id.clone());

            if !result_extension_ids.is_empty() {
                let new_ext_element = SettingsTreeNewExtensionsElement::new("newExtensions", result_extension_ids);
                new_ext_element.set_parent(Some(&root));
                root.push_child(SettingsTreeGroupChild::NewExtensions(new_ext_element));
            }
        }
    }

    pub fn set_result(&self, order: SearchResultIdx, result: Option<ISearchResult>) {
        self.cached_unique_search_results.borrow_mut().clear();
        *self.new_extension_search_results.borrow_mut() = None;

        {
            let mut raw_search_results = self.raw_search_results.borrow_mut();
            if let Some(raw) = raw_search_results.as_mut()
                && order == SearchResultIdx::Local
                && let Some(remote) = raw.get_mut(SearchResultIdx::Remote as usize)
            {
                // To prevent the Settings editor from showing
                // stale remote results mid-search.
                *remote = None;
            }

            let raw = raw_search_results.get_or_insert_with(Vec::new);
            let Some(result) = result else {
                if let Some(slot) = raw.get_mut(order as usize) {
                    *slot = None;
                }
                return;
            };

            if raw.len() <= order as usize {
                raw.resize(order as usize + 1, None);
            }
            raw[order as usize] = Some(result);
        }
        self.update_children();
    }

    /// Gancho de teste: quantas vezes `update_children` rodou desde o último [`Self::reset_update_children_calls`].
    pub fn update_children_calls(&self) -> usize {
        self.update_children_calls.get()
    }

    /// Gancho de teste: o `spy.resetHistory()`.
    pub fn reset_update_children_calls(&self) {
        self.update_children_calls.set(0);
    }

    pub fn get_unique_results_count(&self) -> usize {
        self.search_result_count.get().unwrap_or(0)
    }
}

impl IDisposable for SearchResultModel {
    fn dispose(&self) {
        self.base.dispose();
    }
}

/// `IParsedQuery`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct IParsedQuery {
    pub tags: Vec<String>,
    pub query: String,
    pub extension_filters: Vec<String>,
    pub id_filters: Vec<String>,
    pub feature_filters: Vec<String>,
    pub language_filter: Option<String>,
}

/// As expressões de `parseQuery` (`tagRegex`, `extensionRegex`, `featureRegex`, `idRegex`, `languageRegex` e
/// `/@stable/g`), compiladas uma vez por thread.
struct QueryRegexes {
    tag: RegExp,
    extension: RegExp,
    feature: RegExp,
    id: RegExp,
    language: RegExp,
    stable: RegExp,
}

thread_local! {
    static QUERY_REGEXES: QueryRegexes = QueryRegexes {
        tag: compile_reg_exp(r#"(^|\s)@tag:("([^"]*)"|[^"]\S*)"#, "g"),
        extension: compile_reg_exp(r#"(^|\s)@ext:("([^"]*)"|[^"]\S*)?"#, "g"),
        feature: compile_reg_exp(r#"(^|\s)@feature:("([^"]*)"|[^"]\S*)?"#, "g"),
        id: compile_reg_exp(r#"(^|\s)@id:("([^"]*)"|[^"]\S*)?"#, "g"),
        language: compile_reg_exp(r#"(^|\s)@lang:("([^"]*)"|[^"]\S*)?"#, "g"),
        stable: compile_reg_exp("@stable", "g"),
    };
}

/// `query.replace('literal', () => { ... return '' })`: troca só a primeira ocorrência do texto, e só então chama
/// `on_found`.
fn remove_first_occurrence(query: JsString, needle: &str, on_found: impl FnOnce()) -> JsString {
    let needle: Vec<u16> = needle.encode_utf16().collect();
    match index_of(query.units(), &needle, 0) {
        Some(index) => {
            on_found();
            let mut out = JsString::from(&query.units()[..index]);
            out.push_units(&query.units()[index + needle.len()..]);
            out
        }
        None => query,
    }
}

/// `(group || otherGroup)`: o primeiro grupo que casou e não é vazio.
fn group_text(captures: &Captures<'_>, index: usize) -> Option<String> {
    captures.group(index).filter(|group| !group.is_empty()).map(|group| String::from_utf16_lossy(group))
}

/// A helper function to parse the query on one type of regex.
///
/// - `query`: the search query.
/// - `filter_regex`: the regex to use on the query.
/// - `parsed_parts`: the parts that the regex parses out will be appended to the array passed in here.
///
/// Returns the query with the parsed parts removed.
fn get_tags_for_type(query: &JsString, filter_regex: &RegExp, parsed_parts: &mut Vec<String>) -> JsString {
    replace_all(filter_regex, query.units(), |captures| {
        let parsed_element = group_text(captures, 3).or_else(|| group_text(captures, 2));
        if let Some(parsed_element) = parsed_element {
            parsed_parts.extend(
                parsed_element
                    .split(',')
                    .map(|part| String::from_utf16_lossy(trim(&part.encode_utf16().collect::<Vec<u16>>())))
                    .filter(|part| !part.is_empty()),
            );
        }
        JsString::new()
    })
}

/// `parseQuery`.
pub fn parse_query(query: &str) -> IParsedQuery {
    QUERY_REGEXES.with(|regexes| {
        let mut tags: Vec<String> = Vec::new();
        let query = replace_all(&regexes.tag, &query.encode_utf16().collect::<Vec<u16>>(), |captures| {
            if let Some(tag) = group_text(captures, 3).or_else(|| group_text(captures, 2)) {
                tags.push(tag);
            } else {
                tags.push(String::new());
            }
            JsString::new()
        });

        let mut found_tags: Vec<String> = Vec::new();
        let query = remove_first_occurrence(query, &format!("@{MODIFIED_SETTING_TAG}"), || found_tags.push(MODIFIED_SETTING_TAG.to_owned()));
        let query = remove_first_occurrence(query, &format!("@{POLICY_SETTING_TAG}"), || found_tags.push(POLICY_SETTING_TAG.to_owned()));
        let query = remove_first_occurrence(query, &format!("@{AGENTS_WINDOW_SETTING_TAG}"), || found_tags.push(AGENTS_WINDOW_SETTING_TAG.to_owned()));
        tags.extend(found_tags);

        // Handle @stable by excluding preview and experimental tags
        let query = replace_all(&regexes.stable, query.units(), |_| {
            tags.push("stable".to_owned());
            JsString::new()
        });

        let mut extensions: Vec<String> = Vec::new();
        let mut features: Vec<String> = Vec::new();
        let mut ids: Vec<String> = Vec::new();
        let mut langs: Vec<String> = Vec::new();
        let query = get_tags_for_type(&query, &regexes.extension, &mut extensions);
        let query = get_tags_for_type(&query, &regexes.feature, &mut features);
        let mut query = get_tags_for_type(&query, &regexes.id, &mut ids);

        if ENABLE_LANGUAGE_FILTER {
            query = get_tags_for_type(&query, &regexes.language, &mut langs);
        }

        let query = String::from_utf16_lossy(trim(query.units()));

        // For now, only return the first found language filter
        IParsedQuery { tags, extension_filters: extensions, feature_filters: features, id_filters: ids, language_filter: langs.into_iter().next(), query }
    })
}
