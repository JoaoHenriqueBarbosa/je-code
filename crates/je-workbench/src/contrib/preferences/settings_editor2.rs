//! Porte da parte lógica de `src/vs/workbench/contrib/preferences/browser/settingsEditor2.ts`: o `SettingsEditor2` sem o
//! `EditorPane` de DOM. O que decide o que o editor mostra (a escuta das mudanças de configuração, de atribuição de
//! experimento, de política gerenciada, de confiança do espaço e de extensões, a reconstrução dos modelos do sumário e da
//! árvore, a busca com os provedores local, remoto e de IA, os filtros `@tag`, `@ext`, `@feature`, `@id` e `@lang`, as
//! contagens de resultado, o adiamento do redesenho enquanto uma configuração ou o menu de contexto tem foco) vive aqui.
//! O que desenha (a montagem do cabeçalho, do corpo, do sumário e da árvore, o `layout`, os atalhos de foco, o
//! `saveState`, o `setInput`, os eventos de edição de uma configuração e a abertura do arquivo JSON) é o widget e entra com
//! a árvore de widgets nativa.
//!
//! Divergências:
//! - **O widget.** Os elementos de DOM que o original lê e escreve (`rootElement`, `countElement`, `clearFilterLinkContainer`,
//!   `splitView`, `searchWidget`, `settingsTree`, `settingRenderers`, `tocTree`, `settingsTargetsWidget`, a barra de ação do
//!   botão de IA, a região `aria`, o foco do documento) são o trait [`SettingsEditorView`], que o editor recebe em
//!   [`SettingsEditor2::new`]. Cada método do trait é o trecho de DOM do original que o editor chamava, com os mesmos
//!   argumentos; o comportamento de produção não muda.
//! - **Injeção.** Os serviços decorados do construtor que o trecho lógico usa são o [`SettingsEditor2Services`] (o
//!   pacote da árvore, [`SettingsTreeServices`], já traz configuração, produto, ambiente, perfil, atribuições e política
//!   gerenciada). O `IPreferencesService`, o `IUserDataSyncWorkbenchService`, o `IUserDataSyncEnablementService`, o
//!   `ILanguageService`, o `IKeybindingService`, o `IContextKeyService` e o `ITextResourceConfigurationService` só servem ao
//!   widget e ao `EditorPane`.
//! - **`defaultSettingsEditorModel`.** O `Settings2EditorModel` chega como `Rc<dyn IDefaultSettingsEditorModel>`, o
//!   [`ISettingsEditorModel`] com o `setAdditionalGroups`, para o teste injetar um modelo de grupos fixos.
//! - **`tocTreeModel`** é criado no construtor (no original nasce no `createTOC`, quando o corpo é montado), porque o
//!   widget do sumário não está aqui; [`SettingsEditor2::set_toc_tree_model`] troca o modelo.
//! - **`restoreCachedState`** (a memória do editor, o alvo e o valor da busca) é do widget: [`SettingsEditorView::restore_cached_state`]
//!   devolve o `searchQuery` guardado.
//! - **Assíncrono.** As funções `async` do original são `Promise` (o corpo roda na hora, até o primeiro `await`, como o
//!   JavaScript). Os métodos que o teste lê (`searchResultModel`, `createFilterModel`, `triggerSearch`, `onConfigUpdate`,
//!   `searchWithProvider`, `shouldShowSetting`, `refreshTree`, `renderResultCountMessages`) são públicos.
//! - **Chave vazia.** O `key = ''` de `renderTree` e `scheduleRefresh` é o texto vazio, falso como em JavaScript.

use std::cell::{Cell, RefCell};
use std::collections::{HashMap, HashSet};
use std::rc::{Rc, Weak};

use je_base::async_::{CancelablePromise, Delayer, MaybePromise, async_fn, create_cancelable_promise};
use je_base::cancellation::{CancellationToken, CancellationTokenSource};
use je_base::console;
use je_base::dynamic::{Dynamic, Truthy};
use je_base::errors;
use je_base::lifecycle::{Disposable, DisposableRef, DisposableStore, IDisposable, MutableDisposable};
use je_base::nls::localize;
use je_base::promise::Promise;
use je_base::stopwatch::StopWatch;
use je_base::symbols::Delay;
use je_editor::code_action::platform::{IEditorProgressService, IProgressRunner, ProgressTotal};
use je_extensions::management::GalleryExtension;
use je_platform::configuration::ConfigurationTarget;
use je_platform::configuration::registry::{ConfigurationScope, ExtensionInfo};
use je_platform::log::{LogServiceRef, Logger};
use je_platform::storage::{StorageScope, StorageService, StorageTarget};
use je_platform::telemetry::telemetry::ITelemetryService;
use je_platform::workspace_trust::IWorkspaceTrustManagementService;
use je_storage::StorageValue;
use je_widgets::tree::tree_types::ITreeElement;
use serde_json::Value;

use crate::contrib::preferences::common::preferences::{
    ADVANCED_SETTING_TAG, AGENTS_WINDOW_SETTING_TAG, EMBEDDINGS_SEARCH_PROVIDER_NAME, ENABLE_LANGUAGE_FILTER, EXP_ASSIGNMENT_SETTING_TAG,
    EXTENSION_FETCH_TIMEOUT_MS, EXTENSION_SETTING_TAG, FEATURE_SETTING_TAG, FILTER_MODEL_SEARCH_PROVIDER_NAME, ID_SETTING_TAG, LANGUAGE_SETTING_TAG,
    LLM_RANKED_SEARCH_PROVIDER_NAME, MODIFIED_SETTING_TAG, POLICY_SETTING_TAG, REQUIRE_TRUSTED_WORKSPACE_SETTING_TAG, STRING_MATCH_SEARCH_PROVIDER_NAME,
    TF_IDF_SEARCH_PROVIDER_NAME, WORKSPACE_TRUST_SETTING_TAG, IPreferencesSearchService, ISearchProvider, WorkbenchSettingsEditorSettings,
    get_experimental_extension_toggle_data,
};
use crate::contrib::preferences::settings_layout::{ITOCEntry, ITOCFilter, ITOCFilterPatterns, get_commonly_used_data, toc_data};
use crate::contrib::preferences::settings_tree::{create_toc_tree_for_extension_settings, resolve_configured_untrusted_settings, resolve_settings_tree};
use crate::contrib::preferences::settings_tree_models::{
    ISettingsEditorViewState, SearchResultIdx, SearchResultModel, SettingsTreeElement, SettingsTreeGroupChild, SettingsTreeGroupElement, SettingsTreeModel,
    SettingsTreeServices, SettingsTreeSettingElement, ViewStateRef, parse_query,
};
use crate::contrib::preferences::preferences_widgets::SettingsTarget;
use crate::contrib::preferences::toc_tree::{TocTreeModel, create_toc_iterator};
use crate::services::extension_management::extension_gallery_service::IExtensionGalleryService;
use crate::services::extension_management::extension_management::IExtensionManagementService;
use crate::services::extension_management::service_contracts::IChatEntitlementService;
use crate::services::extensions::extensions::IExtensionService;
use crate::services::preferences::preferences::{
    ISearchResult, ISetting, ISettingMatch, ISettingsEditorModel, ISettingsGroup, ISettingsSection, SettingMatchType, SettingSchemaType,
};
use crate::services::preferences::preferences_models::{NULL_RANGE, Settings2EditorModel};

/// `ALWAYS_SHOW_ADVANCED_SETTINGS_SETTING` (`workbench/services/preferences/common/preferences.ts`).
pub const ALWAYS_SHOW_ADVANCED_SETTINGS_SETTING: &str = "workbench.settings.alwaysShowAdvancedSettings";

/// `SettingsFocusContext`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SettingsFocusContext {
    Search,
    TableOfContents,
    SettingTree,
    SettingControl,
}

/// `createGroupIterator`.
pub fn create_group_iterator(group: &Rc<SettingsTreeGroupElement>) -> Vec<ITreeElement<SettingsTreeGroupChild>> {
    group
        .children()
        .into_iter()
        .map(|child| {
            let children = child.as_group().map(create_group_iterator);
            ITreeElement { element: child, children, collapsible: None, collapsed: None }
        })
        .collect()
}

/// `isSettingsSearchUpToDate(searchPending, renderedSearchQuery, currentSearchValue)`: whether Settings search results are
/// safe to focus from the search input. `search_pending` must be the boolean from `Delayer::is_triggered`.
pub fn is_settings_search_up_to_date(search_pending: bool, rendered_search_query: Option<&str>, current_search_value: &str) -> bool {
    !search_pending && rendered_search_query == Some(js_trim(current_search_value))
}

/// `String.prototype.trim`: tira o espaço em branco do JavaScript (`WhiteSpace` e `LineTerminator`, o que o
/// `char::is_whitespace` cobre, mais o BOM `U+FEFF`).
pub(crate) fn js_trim(value: &str) -> &str {
    value.trim_matches(|c: char| c.is_whitespace() || c == '\u{FEFF}')
}

const SEARCH_TOC_BEHAVIOR_KEY: &str = "workbench.settings.settingsSearchTocBehavior";
const DISMISSED_EXTENSION_SETTINGS_STORAGE_KEY: &str = "settingsEditor2.dismissedExtensionSettings";
const DISMISSED_EXTENSION_SETTINGS_DELIMITER: char = '\t';
const SEARCH_HISTORY_STORAGE_KEY: &str = "settingsEditor2.searchHistory";
const SEARCH_DEBOUNCE: u64 = 200;
const CONFIG_SCHEMA_UPDATE_DELAYER: u64 = 500;

fn search_box_label() -> String {
    localize("SearchSettings.AriaLabel", "Search settings", &[])
}

fn search_box_placeholder_with_history() -> String {
    localize("SearchSettings.PlaceholderWithHistory", "Search settings ({0} for history)", &["\u{21C5}"])
}

/// O alvo cujo foco o editor acompanha para adiar um redesenho (o argumento `element` de `scheduleRefresh`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum FocusTarget {
    /// O elemento da configuração, achado pela chave do `data-key`.
    Setting(String),
    /// O `.context-view` aberto.
    ContextView,
}

/// A configuração com um controle focado dentro da árvore (`getActiveControlInSettingsTree` e
/// `getSettingDOMElementForDOMElement`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FocusedSettingControl {
    /// O `data-key` do elemento da configuração.
    pub key: String,
    /// `focusedSetting.parentElement?.classList.contains('setting-item-list')`: `None` quando não há elemento pai.
    pub parent_element_is_list: Option<bool>,
}

/// O widget do editor de configurações: o DOM que o `SettingsEditor2` do original lê e escreve (ver a divergência no topo).
pub trait SettingsEditorView {
    // ---- o editor (EditorPane) ----

    /// `isVisible()`.
    fn is_visible(&self) -> bool;

    /// `this.layout(this.dimension)`.
    fn layout(&self);

    /// `this.layoutSplitView(this.dimension)`.
    fn layout_split_view(&self);

    /// `this.splitView.setViewVisible(0, false)` e `this.splitView.style({ separatorBorder: Color.transparent })`: esconde o
    /// sumário durante a busca.
    fn hide_toc(&self);

    /// `restoreCachedState()`: restaura o estado guardado do editor (alvo e valor da busca, no widget e no `viewState`) e
    /// devolve o `searchQuery` guardado.
    fn restore_cached_state(&self, view_state: &ViewStateRef) -> Option<String>;

    // ---- elemento raiz e cabeçalho ----

    /// `rootElement.classList.toggle(className, on)`.
    fn set_root_class(&self, class_name: &str, on: bool);

    /// `countElement.style.display`.
    fn count_display(&self) -> String;

    /// `countElement.style.display = display`.
    fn set_count_display(&self, display: &str);

    /// `countElement.innerText = text`.
    fn set_count_text(&self, text: &str);

    /// `clearFilterLinkContainer.style.display = display`.
    fn set_clear_filter_link_display(&self, display: &str);

    /// `splitView.el.style.visibility = visibility`.
    fn set_body_visibility(&self, visibility: &str);

    /// `aria.status(message)`.
    fn aria_status(&self, message: &str);

    // ---- campo de busca ----

    /// `searchWidget.getValue()`.
    fn search_value(&self) -> String;

    /// `searchWidget.updateAriaLabel(label)`.
    fn update_search_aria_label(&self, label: &str);

    /// `searchWidget.addToHistory()`.
    fn add_search_to_history(&self);

    /// `searchWidget.getHistory()`.
    fn search_history(&self) -> Vec<String>;

    /// `searchWidget.setPlaceHolder(placeholder)`.
    fn set_search_placeholder(&self, placeholder: &str);

    /// `settingsTargetsWidget.updateLanguageFilterIndicators(languageFilter)`.
    fn update_language_filter_indicators(&self, language_filter: Option<&str>);

    // ---- botão de resultados de IA ----

    /// `this.searchInputActionBar.hasAction(this.showAiResultsAction)`.
    fn ai_results_toggle_shown(&self) -> bool;

    /// O corpo de `updateAiSearchToggleVisibility()` depois de calcular `canShowToggle`: põe ou tira a ação da barra.
    fn update_ai_search_toggle_visibility(&self, can_show_toggle: bool);

    /// `disableAiSearchToggle()`: desmarca e desabilita a ação, zera a chave de contexto e troca o rótulo.
    fn disable_ai_search_toggle(&self);

    /// O `then` da busca de IA: habilita a ação, liga a chave de contexto e troca o rótulo.
    fn enable_ai_results_action(&self);

    // ---- árvore de configurações ----

    /// `settingsTree.firstVisibleElement`.
    fn settings_tree_first_visible_element(&self) -> Option<SettingsTreeGroupChild>;

    /// `settingsTree.hasElement(element)`.
    fn settings_tree_has_element(&self, element: &Rc<SettingsTreeSettingElement>) -> bool;

    /// `settingsTree.rerender(element)`.
    fn settings_tree_rerender(&self, element: &Rc<SettingsTreeSettingElement>);

    /// `settingsTree.setChildren(null, children)`.
    fn settings_tree_set_children(&self, children: Vec<ITreeElement<SettingsTreeGroupChild>>);

    /// `settingsTree.reveal(element, 0)`; a exceção que o original ignora o widget engole.
    fn settings_tree_reveal(&self, element: &SettingsTreeGroupChild);

    /// `settingsTree.scrollTop = 0`.
    fn settings_tree_scroll_to_top(&self);

    /// `settingRenderers.getDOMElementsForSettingKey(settingsTree.getHTMLElement(), key)[0].classList.toggle('is-configured', configured)`.
    fn set_setting_configured_class(&self, key: &str, configured: bool);

    // ---- árvore do sumário ----

    /// `tocTree.setChildren(null, children)`.
    fn toc_tree_set_children(&self, children: Vec<ITreeElement<Rc<SettingsTreeGroupElement>>>);

    /// `tocTree.collapseAll()`.
    fn toc_tree_collapse_all(&self);

    /// `tocTree.expandAll()`.
    fn toc_tree_expand_all(&self);

    /// `tocTree.setFocus(elements)`.
    fn toc_tree_set_focus(&self, elements: &[Rc<SettingsTreeGroupElement>]);

    /// `tocTree.setSelection(elements)`.
    fn toc_tree_set_selection(&self, elements: &[Rc<SettingsTreeGroupElement>]);

    // ---- foco ----

    /// A configuração com um controle focado dentro da árvore (`getActiveControlInSettingsTree()` com
    /// `getSettingDOMElementForDOMElement`), ou `None`.
    fn focused_setting_control(&self) -> Option<FocusedSettingControl>;

    /// `contextViewFocused()`.
    fn context_view_focused(&self) -> bool;

    /// `DOM.trackFocus(element).onDidBlur(onBlur)`: chama `on_blur` quando o foco sai do alvo. O disposable devolvido para
    /// de acompanhar.
    fn track_blur(&self, target: FocusTarget, on_blur: Box<dyn Fn()>) -> DisposableRef;
}

/// O `Settings2EditorModel` que o editor lê: o [`ISettingsEditorModel`] com o `setAdditionalGroups`.
pub trait IDefaultSettingsEditorModel: ISettingsEditorModel {
    /// `setAdditionalGroups(groups)`.
    fn set_additional_groups(&self, groups: Vec<ISettingsGroup>);
}

impl IDefaultSettingsEditorModel for Settings2EditorModel {
    fn set_additional_groups(&self, groups: Vec<ISettingsGroup>) {
        Settings2EditorModel::set_additional_groups(self, groups);
    }
}

/// Os serviços que o construtor do `SettingsEditor2` recebe por injeção e que o trecho lógico usa (ver a divergência no
/// topo).
#[derive(Clone)]
pub struct SettingsEditor2Services {
    /// Configuração, linguagem, perfil, produto, ambiente, atribuições de experimento e política gerenciada.
    pub tree: SettingsTreeServices,
    pub telemetry_service: Rc<dyn ITelemetryService>,
    pub preferences_search_service: Rc<dyn IPreferencesSearchService>,
    pub log_service: LogServiceRef,
    pub storage_service: Rc<dyn StorageService>,
    pub workspace_trust_management_service: Rc<dyn IWorkspaceTrustManagementService>,
    pub extension_service: Rc<dyn IExtensionService>,
    pub extension_management_service: Rc<dyn IExtensionManagementService>,
    pub extension_gallery_service: Rc<dyn IExtensionGalleryService>,
    pub editor_progress_service: Rc<dyn IEditorProgressService>,
    pub chat_entitlement_service: Rc<dyn IChatEntitlementService>,
}

/// `SettingsEditor2`.
pub struct SettingsEditor2 {
    base: Rc<Disposable>,
    weak: Weak<SettingsEditor2>,
    services: SettingsEditor2Services,
    view: Rc<dyn SettingsEditorView>,

    default_settings_editor_model: RefCell<Option<Rc<dyn IDefaultSettingsEditorModel>>>,
    toc_tree_model: RefCell<Rc<TocTreeModel>>,
    settings_tree_model: Rc<MutableDisposable<SettingsTreeModel>>,
    search_result_model: Rc<MutableDisposable<SearchResultModel>>,

    search_delayer: Rc<Delayer<()>>,
    search_input_delayer: Rc<Delayer<()>>,
    updated_config_schema_delayer: Rc<Delayer<()>>,
    search_in_progress: RefCell<Option<CancellationTokenSource>>,
    ai_search_promise: RefCell<Option<CancelablePromise<()>>>,

    /// The trimmed query value that the currently rendered results reflect. Used to determine whether the displayed
    /// results are up to date with the current search input value before moving focus into the results.
    rendered_search_query: RefCell<Option<String>>,

    view_state: ViewStateRef,
    search_result_label: RefCell<Option<String>>,
    last_synced_label: RefCell<Option<String>>,
    settings_order_by_toc_index: RefCell<Option<HashMap<String, f64>>>,

    scheduled_refreshes: RefCell<HashMap<String, Rc<DisposableStore>>>,
    pending_assignment_refresh: Cell<bool>,

    /// Don't spam warnings
    has_warned_missing_settings: Cell<bool>,
    toc_tree_disposed: Cell<bool>,

    installed_extension_ids: RefCell<Vec<String>>,
    dismissed_extension_settings: RefCell<Vec<String>>,
}

impl SettingsEditor2 {
    pub const ID: &'static str = "workbench.editor.settings2";

    pub fn new(services: SettingsEditor2Services, view: Rc<dyn SettingsEditorView>) -> Rc<Self> {
        let base = Disposable::new();
        let view_state: ViewStateRef = Rc::new(RefCell::new(ISettingsEditorViewState::new(SettingsTarget::UserLocal)));

        let search_delayer = Rc::new(Delayer::new(Delay::Millis(SEARCH_DEBOUNCE)));
        let search_input_delayer = Rc::new(Delayer::new(Delay::Millis(SEARCH_DEBOUNCE)));
        let updated_config_schema_delayer = Rc::new(Delayer::new(Delay::Millis(CONFIG_SCHEMA_UPDATE_DELAYER)));
        base.register(search_delayer.clone());
        base.register(search_input_delayer.clone());
        base.register(updated_config_schema_delayer.clone());

        let settings_tree_model = MutableDisposable::new();
        let search_result_model = MutableDisposable::new();
        base.register(settings_tree_model.clone());
        base.register(search_result_model.clone());

        let dismissed_extension_settings = Self::read_dismissed_extension_settings(&*services.storage_service);
        let toc_tree_model = TocTreeModel::new(view_state.clone(), services.tree.environment_service.clone());

        let editor = Rc::new_cyclic(|weak| Self {
            base,
            weak: weak.clone(),
            services,
            view,
            default_settings_editor_model: RefCell::new(None),
            toc_tree_model: RefCell::new(toc_tree_model),
            settings_tree_model,
            search_result_model,
            search_delayer,
            search_input_delayer,
            updated_config_schema_delayer,
            search_in_progress: RefCell::new(None),
            ai_search_promise: RefCell::new(None),
            rendered_search_query: RefCell::new(Some(String::new())),
            view_state,
            search_result_label: RefCell::new(None),
            last_synced_label: RefCell::new(None),
            settings_order_by_toc_index: RefCell::new(None),
            scheduled_refreshes: RefCell::new(HashMap::new()),
            pending_assignment_refresh: Cell::new(false),
            has_warned_missing_settings: Cell::new(false),
            toc_tree_disposed: Cell::new(false),
            installed_extension_ids: RefCell::new(Vec::new()),
            dismissed_extension_settings: RefCell::new(dismissed_extension_settings),
        });
        editor.register_listeners();
        editor
    }

    fn me(&self) -> Rc<Self> {
        self.weak.upgrade().expect("o editor de configurações já foi descartado")
    }

    /// O `storageService.get(DISMISSED_EXTENSION_SETTINGS_STORAGE_KEY, PROFILE, '').split(DELIMITER)`.
    fn read_dismissed_extension_settings(storage_service: &dyn StorageService) -> Vec<String> {
        storage_service
            .get(DISMISSED_EXTENSION_SETTINGS_STORAGE_KEY, StorageScope::Profile, Some(""))
            .unwrap_or_default()
            .split(DISMISSED_EXTENSION_SETTINGS_DELIMITER)
            .map(str::to_owned)
            .collect()
    }

    /// As assinaturas do construtor do original.
    fn register_listeners(&self) {
        let weak = self.weak.clone();
        let services = &self.services;

        self.base.register(services.tree.configuration_service.on_did_change_configuration().listen({
            let weak = weak.clone();
            move |event| {
                let Some(this) = weak.upgrade() else {
                    return;
                };
                let affected_keys: HashSet<String> = event.affected_keys.iter().map(|key| key.to_string_lossy()).collect();
                if affected_keys.contains(WorkbenchSettingsEditorSettings::ShowAISearchToggle.as_str())
                    || affected_keys.contains(WorkbenchSettingsEditorSettings::EnableNaturalLanguageSearch.as_str())
                {
                    this.update_ai_search_toggle_visibility();
                }
                if event.affects_configuration(ALWAYS_SHOW_ADVANCED_SETTINGS_SETTING, None) {
                    let _ = this.on_config_update(None, true, true);
                }
                if event.source.get() != Some(ConfigurationTarget::Default) {
                    let _ = this.on_config_update(Some(affected_keys), false, false);
                }
            }
        }));

        self.base.register(services.chat_entitlement_service.on_did_change_sentiment().listen({
            let weak = weak.clone();
            move |()| {
                if let Some(this) = weak.upgrade() {
                    this.update_ai_search_toggle_visibility();
                }
            }
        }));

        self.base.register(services.tree.user_data_profile_service.on_did_change_current_profile().listen({
            let weak = weak.clone();
            move |event| {
                if let Some(this) = weak.upgrade() {
                    event.join(this.when_current_profile_changed());
                }
            }
        }));

        self.base.register(services.workspace_trust_management_service.on_did_change_trust().listen({
            let weak = weak.clone();
            move |_| {
                let Some(this) = weak.upgrade() else {
                    return;
                };
                let trusted = this.services.workspace_trust_management_service.is_workspace_trusted();
                if let Some(search_result_model) = this.search_result_model() {
                    search_result_model.update_workspace_trust(trusted);
                }

                if let Some(settings_tree_model) = this.settings_tree_model.value() {
                    settings_tree_model.update_workspace_trust(trusted);
                    this.render_tree("", false);
                }
            }
        }));

        self.base.register(services.tree.configuration_service.on_did_change_restricted_settings().listen({
            let weak = weak.clone();
            move |event| {
                let Some(this) = weak.upgrade() else {
                    return;
                };
                if !event.default.is_empty() && this.has_current_settings_model() {
                    let keys: HashSet<String> = event.default.iter().map(|key| key.to_string_lossy()).collect();
                    this.update_elements_by_key(&keys);
                }
            }
        }));

        self.base.register(services.tree.experimental_settings_service.on_did_change_assignments().listen({
            let weak = weak.clone();
            move |keys| {
                let Some(this) = weak.upgrade() else {
                    return;
                };
                let expassigned_filter = this.view_state.borrow().tag_filters.as_ref().is_some_and(|tag_filters| tag_filters.contains(EXP_ASSIGNMENT_SETTING_TAG));
                if this.search_result_model().is_some() && expassigned_filter {
                    if let Some(settings_tree_model) = this.settings_tree_model.value() {
                        for key in keys.iter() {
                            settings_tree_model.update_elements_by_name(key);
                        }
                    }
                    this.pending_assignment_refresh.set(true);
                    this.render_tree("", false);
                } else if this.has_current_settings_model() {
                    this.update_elements_by_key(keys);
                }
            }
        }));

        self.base.register(services.tree.managed_settings_presentation_service.on_did_change().listen({
            let weak = weak.clone();
            move |keys| {
                let Some(this) = weak.upgrade() else {
                    return;
                };
                if this.has_current_settings_model() {
                    for key in keys.iter() {
                        if let Some(settings_tree_model) = this.settings_tree_model.value() {
                            settings_tree_model.update_elements_by_name(key);
                        }
                        if let Some(search_result_model) = this.search_result_model() {
                            search_result_model.update_elements_by_name(key);
                        }
                    }
                    if let Some(search_result_model) = this.search_result_model() {
                        search_result_model.update_children();
                    }
                    this.refresh_toc_tree();
                    this.render_tree("", true);
                }
            }
        }));

        self.base.register(services.extension_management_service.on_did_install_extensions().listen({
            let weak = weak.clone();
            move |_| {
                if let Some(this) = weak.upgrade() {
                    let _ = this.refresh_installed_extensions_list();
                }
            }
        }));
        self.base.register(services.extension_management_service.on_did_uninstall_extension().listen(move |_| {
            if let Some(this) = weak.upgrade() {
                let _ = this.refresh_installed_extensions_list();
            }
        }));
    }

    /// `whenCurrentProfileChanged()`.
    fn when_current_profile_changed(&self) -> Promise<()> {
        let weak = self.weak.clone();
        // O original não espera o `trigger`, e a função `async` devolve uma promessa já resolvida.
        let _ = self.updated_config_schema_delayer.trigger(
            move || {
                if let Some(this) = weak.upgrade() {
                    *this.dismissed_extension_settings.borrow_mut() = Self::read_dismissed_extension_settings(&*this.services.storage_service);
                    let _ = this.on_config_update(None, true, false);
                }
                MaybePromise::Value(())
            },
            None,
        );
        Promise::resolved(())
    }

    /// `canShowAdvancedSettings()`.
    fn can_show_advanced_settings(&self) -> bool {
        if self.services.tree.configuration_service.get_value(Some(ALWAYS_SHOW_ADVANCED_SETTINGS_SETTING), None).is_truthy() {
            return true;
        }
        self.view_state.borrow().tag_filters.as_ref().is_some_and(|tag_filters| tag_filters.contains(ADVANCED_SETTING_TAG) || tag_filters.contains(EXP_ASSIGNMENT_SETTING_TAG))
    }

    /// Allows explicitly targeted settings to bypass the default advanced-settings filter.
    pub fn should_show_setting(&self, setting: &ISetting) -> bool {
        if !setting.tags.as_ref().is_some_and(|tags| tags.iter().any(|tag| tag == ADVANCED_SETTING_TAG)) {
            return true;
        }
        let view_state = self.view_state.borrow();
        if view_state.id_filters.as_ref().is_some_and(|id_filters| id_filters.contains(&setting.key)) {
            return true;
        }
        if view_state.query.as_ref().is_some_and(|query| query.to_lowercase().contains(&setting.key.to_lowercase())) {
            return true;
        }
        if view_state.tag_filters.as_ref().is_some_and(|tag_filters| tag_filters.contains(POLICY_SETTING_TAG)) {
            return true;
        }
        false
    }

    fn disable_ai_search_toggle(&self) {
        self.view.disable_ai_search_toggle();
    }

    /// `updateAiSearchToggleVisibility()`.
    fn update_ai_search_toggle_visibility(&self) {
        let configuration_service = &self.services.tree.configuration_service;
        let show_ai_toggle = configuration_service.get_value(Some(WorkbenchSettingsEditorSettings::ShowAISearchToggle.as_str()), None).is_truthy();
        let enable_natural_language_search = configuration_service.get_value(Some(WorkbenchSettingsEditorSettings::EnableNaturalLanguageSearch.as_str()), None).is_truthy();
        let sentiment = self.services.chat_entitlement_service.sentiment();
        let chat_hidden = sentiment.hidden || sentiment.disabled;
        let can_show_toggle = show_ai_toggle && enable_natural_language_search && !chat_hidden;

        self.view.update_ai_search_toggle_visibility(can_show_toggle);
    }

    // ---- modelos ----

    /// `get currentSettingsModel()`: o modelo da busca ou, sem busca, o da árvore.
    fn has_current_settings_model(&self) -> bool {
        self.search_result_model().is_some() || self.settings_tree_model.value().is_some()
    }

    /// `currentSettingsModel.root`.
    fn current_root(&self) -> Option<Rc<SettingsTreeGroupElement>> {
        match self.search_result_model() {
            Some(search_result_model) => Some(search_result_model.root()),
            None => self.settings_tree_model.value().map(|settings_tree_model| settings_tree_model.root()),
        }
    }

    /// `currentSettingsModel.getElementsByName(name)`.
    fn current_elements_by_name(&self, name: &str) -> Option<Vec<Rc<SettingsTreeSettingElement>>> {
        match self.search_result_model() {
            Some(search_result_model) => search_result_model.get_elements_by_name(name),
            None => self.settings_tree_model.value().and_then(|settings_tree_model| settings_tree_model.get_elements_by_name(name)),
        }
    }

    /// `get searchResultModel()`.
    pub fn search_result_model(&self) -> Option<Rc<SearchResultModel>> {
        self.search_result_model.value()
    }

    /// `set searchResultModel(value)`.
    pub fn set_search_result_model(&self, value: Option<Rc<SearchResultModel>>) {
        self.search_result_model.set(value);
        self.pending_assignment_refresh.set(false);

        self.view.set_root_class("search-mode", self.search_result_model.value().is_some());
    }

    /// `settingsTreeModel.value`.
    pub fn settings_tree_model(&self) -> Option<Rc<SettingsTreeModel>> {
        self.settings_tree_model.value()
    }

    /// `settingsTreeModel.value = model`.
    pub fn set_settings_tree_model(&self, model: Rc<SettingsTreeModel>) {
        self.settings_tree_model.set(Some(model));
    }

    /// `tocTreeModel`.
    pub fn toc_tree_model(&self) -> Rc<TocTreeModel> {
        self.toc_tree_model.borrow().clone()
    }

    /// `tocTreeModel = model`.
    pub fn set_toc_tree_model(&self, model: Rc<TocTreeModel>) {
        *self.toc_tree_model.borrow_mut() = model;
    }

    /// `defaultSettingsEditorModel = model`.
    pub fn set_default_settings_editor_model(&self, model: Rc<dyn IDefaultSettingsEditorModel>) {
        *self.default_settings_editor_model.borrow_mut() = Some(model);
    }

    /// `viewState`.
    pub fn view_state(&self) -> ViewStateRef {
        self.view_state.clone()
    }

    /// `searchInputDelayer`, que o campo de busca dispara a cada digitação.
    pub fn search_input_delayer(&self) -> Rc<Delayer<()>> {
        self.search_input_delayer.clone()
    }

    /// `isSearchUpToDate()`.
    pub fn is_search_up_to_date(&self) -> bool {
        is_settings_search_up_to_date(self.search_input_delayer.is_triggered(), self.rendered_search_query.borrow().as_deref(), &self.view.search_value())
    }

    // ---- atualização ----

    /// `refreshInstalledExtensionsList()`.
    pub fn refresh_installed_extensions_list(&self) -> Promise<()> {
        let this = self.me();
        async_fn(async move {
            let installed_extensions = this.services.extension_management_service.get_installed().await?;
            *this.installed_extension_ids.borrow_mut() = installed_extensions
                .iter()
                .filter(|extension| extension.manifest.contributes.as_ref().is_some_and(|contributes| !matches!(contributes.configuration, Value::Null | Value::Bool(false))))
                .map(|extension| extension.identifier.id.clone())
                .collect();
            Ok(())
        })
    }

    /// `createSettingsOrderByTocIndex`.
    fn create_settings_order_by_toc_index(resolved_settings_root: &ITOCEntry<Rc<ISetting>>) -> HashMap<String, f64> {
        fn index_settings(resolved_settings_root: &ITOCEntry<Rc<ISetting>>, index: &mut HashMap<String, f64>, mut counter: f64) -> f64 {
            if let Some(settings) = &resolved_settings_root.settings {
                for setting in settings {
                    if !index.contains_key(&setting.key) {
                        index.insert(setting.key.clone(), counter);
                        counter += 1.0;
                    }
                }
            }
            if let Some(children) = &resolved_settings_root.children {
                for child in children {
                    counter = index_settings(child, index, counter);
                }
            }
            counter
        }

        let mut index = HashMap::new();
        index_settings(resolved_settings_root, &mut index, 0.0);
        index
    }

    /// `refreshModels(resolvedSettingsRoot)`.
    fn refresh_models(&self, resolved_settings_root: &ITOCEntry<Rc<ISetting>>) {
        // Both calls to refreshModels require a valid settingsTreeModel.
        let settings_tree_model = self.settings_tree_model.value().expect("refreshModels exige o modelo da árvore");
        settings_tree_model.update(resolved_settings_root);
        self.toc_tree_model().set_settings_tree_root(settings_tree_model.root());
        *self.settings_order_by_toc_index.borrow_mut() = Some(Self::create_settings_order_by_toc_index(resolved_settings_root));
    }

    /// `onConfigUpdate(keys?, forceRefresh = false, triggerSearch = false)`.
    pub fn on_config_update(&self, keys: Option<HashSet<String>>, force_refresh: bool, trigger_search: bool) -> Promise<()> {
        let this = self.me();
        async_fn(async move {
            if let Some(keys) = keys {
                this.update_elements_by_key(&keys);
                return Ok(());
            }

            let Some(default_settings_editor_model) = this.default_settings_editor_model.borrow().clone() else {
                return Ok(());
            };

            let mut groups: Vec<ISettingsGroup> = default_settings_editor_model.settings_groups().into_iter().skip(1).collect(); // Without commonlyUsed
            let (extension_settings_groups, core_settings_groups): (Vec<ISettingsGroup>, Vec<ISettingsGroup>) = groups.iter().cloned().partition(|group| group.extension_info.is_some());
            let filter: Option<ITOCFilter> = if this.can_show_advanced_settings() {
                None
            } else {
                Some(ITOCFilter { include: None, exclude: Some(ITOCFilterPatterns { key_patterns: None, tags: Some(vec![ADVANCED_SETTING_TAG.to_owned()]) }) })
            };

            let settings_result = resolve_settings_tree(&toc_data(), &core_settings_groups, filter.as_ref());
            let mut resolved_settings_root = settings_result.tree;

            // Warn for settings not included in layout
            if !settings_result.leftover_settings.is_empty() && !this.has_warned_missing_settings.get() {
                let setting_key_list: Vec<&str> = settings_result.leftover_settings.values().map(|setting| setting.key.as_str()).collect();
                this.services.log_service.warn(&format!("SettingsEditor2: Settings not included in settingsLayout.ts: {}", setting_key_list.join(", ")));
                this.has_warned_missing_settings.set(true);
            }

            let mut additional_groups: Vec<ISettingsGroup> = Vec::new();
            let mut set_additional_groups = false;
            let toggle_data = get_experimental_extension_toggle_data(
                this.services.chat_entitlement_service.clone(),
                this.services.extension_gallery_service.clone(),
                this.services.tree.product_service.clone(),
            )
            .await?;
            if let Some(toggle_data) = &toggle_data
                && groups.iter().any(|group| group.extension_info.is_some())
                && !toggle_data.settings_editor_recommended_extensions.is_empty()
            {
                // Refresh installed extensions once per onConfigUpdate invocation for performance,
                // instead of per extension. The installed list may still change while iterating.
                this.refresh_installed_extensions_list().await?;
                for key in toggle_data.settings_editor_recommended_extensions.keys() {
                    let Some(extension) = toggle_data.recommended_extensions_gallery_info.get(key) else {
                        continue;
                    };

                    let extension_id = extension.identifier.id.clone();
                    let extension_installed = this.installed_extension_ids.borrow().contains(&extension_id);

                    // Drill down to see whether the group and setting already exist
                    // and need to be removed.
                    let matching_group_index = groups.iter().position(|group| {
                        group.extension_info.as_ref().is_some_and(|extension_info| extension_info.id.to_lowercase() == extension_id.to_lowercase())
                            && group.sections.len() == 1
                            && group.sections[0].settings.len() == 1
                            && group.sections[0].settings[0].display_extension_id.as_deref().is_some_and(|display_extension_id| !display_extension_id.is_empty())
                    });
                    if extension_installed || this.dismissed_extension_settings.borrow().contains(&extension_id) {
                        if let Some(matching_group_index) = matching_group_index {
                            groups.remove(matching_group_index);
                            set_additional_groups = true;
                        }
                        continue;
                    }

                    if matching_group_index.is_some() {
                        continue;
                    }

                    // Create the entry. extensionInstalled is false in this case.
                    let manifest_promise = this.services.extension_gallery_service.get_manifest(extension, CancellationToken::None);
                    let Ok(Some(Some(manifest))) = je_base::async_::race_timeout(&manifest_promise, EXTENSION_FETCH_TIMEOUT_MS, None).await else {
                        // Likely a networking issue, or no manifest.
                        // Skip adding a button for this extension to the Settings editor.
                        continue;
                    };

                    let group_title = extension_group_title_from_manifest(&manifest);

                    let recommendation_info = &toggle_data.settings_editor_recommended_extensions[key];
                    let extension_name = extension_display_name(extension, &extension_id);
                    let setting_key = format!("{key}.manageExtension");
                    let description_override = recommendation_info.on_settings_editor_open.as_ref().and_then(|on_open| on_open.description_override.clone());
                    let setting = ISetting {
                        key: setting_key,
                        value: Dynamic::Null,
                        description: vec![description_override.unwrap_or_else(|| extension.description.clone())],
                        description_is_markdown: Some(false),
                        scope: Some(ConfigurationScope::Window),
                        r#type: Some(SettingSchemaType::Single("null".to_owned())),
                        display_extension_id: Some(extension_id.clone()),
                        extension_group_title: Some(group_title.unwrap_or_else(|| extension_name.clone())),
                        category_label: Some("Extensions".to_owned()),
                        title: Some(extension_name),
                        ..ISetting::default()
                    };
                    let additional_group = ISettingsGroup {
                        sections: vec![ISettingsSection { title: None, settings: vec![Rc::new(setting.clone())] }],
                        id: extension_id.clone(),
                        title: setting.extension_group_title.clone().unwrap_or_default(),
                        title_range: NULL_RANGE,
                        range: NULL_RANGE,
                        order: None,
                        extension_info: Some(ExtensionInfo { id: extension_id, display_name: (!extension.display_name.is_empty()).then(|| extension.display_name.clone()) }),
                    };
                    groups.push(additional_group.clone());
                    additional_groups.push(additional_group);
                    set_additional_groups = true;
                }
            }

            let extensions_toc = create_toc_tree_for_extension_settings(this.services.extension_service.clone(), &extension_settings_groups, filter.as_ref()).await?;
            resolved_settings_root.children.get_or_insert_with(Vec::new).push(extensions_toc);

            resolved_settings_root.children.get_or_insert_with(Vec::new).insert(0, get_commonly_used_data(&groups));

            if toggle_data.is_some() && set_additional_groups {
                // Add the additional groups to the model to help with searching.
                default_settings_editor_model.set_additional_groups(additional_groups);
            }

            let settings_target = this.view_state.borrow().settings_target.clone();
            if !this.services.workspace_trust_management_service.is_workspace_trusted() && matches!(settings_target, SettingsTarget::Resource(_) | SettingsTarget::Workspace) {
                let language_filter = this.view_state.borrow().language_filter.clone();
                let configured_untrusted_workspace_settings =
                    resolve_configured_untrusted_settings(&groups, &settings_target, language_filter.as_deref(), &*this.services.tree.configuration_service);
                if !configured_untrusted_workspace_settings.is_empty() {
                    resolved_settings_root.children.get_or_insert_with(Vec::new).insert(
                        0,
                        ITOCEntry {
                            id: "workspaceTrust".to_owned(),
                            label: localize("settings require trust", "Workspace Trust", &[]),
                            order: None,
                            children: None,
                            settings: Some(configured_untrusted_workspace_settings),
                            hide: None,
                        },
                    );
                }
            }

            if let Some(search_result_model) = this.search_result_model() {
                search_result_model.update_children();
            }

            let anchor_id: Option<String> = match this.view.settings_tree_first_visible_element() {
                Some(SettingsTreeGroupChild::Setting(setting)) => Some(setting.setting.key.clone()),
                Some(SettingsTreeGroupChild::Group(group)) => Some(group.id()),
                _ => None,
            };

            if this.settings_tree_model.value().is_some() {
                this.refresh_models(&resolved_settings_root);

                if trigger_search && this.search_result_model().is_some() {
                    // If an extension's settings were just loaded and a search is active, retrigger the search so it shows up
                    return this.on_search_input_changed(false).await;
                }

                this.refresh_toc_tree();
                this.render_tree("", force_refresh);

                if let Some(anchor_id) = anchor_id {
                    let new_model = this.settings_tree_model.value().expect("o modelo da árvore existe");
                    let new_element: Option<SettingsTreeGroupChild> = match new_model.get_elements_by_name(&anchor_id) {
                        Some(settings) if !settings.is_empty() => Some(SettingsTreeGroupChild::Setting(settings[0].clone())),
                        _ => find_group(&[new_model.root()], &anchor_id).map(SettingsTreeGroupChild::Group),
                    };

                    if let Some(new_element) = new_element {
                        this.view.settings_tree_reveal(&new_element);
                    }
                }
            } else {
                let is_workspace_trusted = this.services.workspace_trust_management_service.is_workspace_trusted();
                this.set_settings_tree_model(SettingsTreeModel::new(this.view_state.clone(), is_workspace_trusted, this.services.tree.clone()));
                this.refresh_models(&resolved_settings_root);

                // Don't restore the cached state if we already have a query value from calling _setOptions().
                let has_query = this.view_state.borrow().query.as_deref().is_some_and(|query| !query.is_empty());
                let cached_search_query = if has_query { None } else { this.view.restore_cached_state(&this.view_state) };
                if cached_search_query.is_some_and(|search_query| !search_query.is_empty()) || !this.view.search_value().is_empty() {
                    this.on_search_input_changed(true).await?;
                } else {
                    this.refresh_toc_tree();

                    // Set initial category to the first one (Commonly Used)
                    let root_children = this.settings_tree_model.value().expect("o modelo da árvore existe").root().children();
                    if let Some(SettingsTreeGroupChild::Group(first_category)) = root_children.first() {
                        this.view_state.borrow_mut().category_filter = Some(first_category.clone());
                        this.view.toc_tree_set_focus(std::slice::from_ref(first_category));
                        this.view.toc_tree_set_selection(std::slice::from_ref(first_category));
                    }

                    this.refresh_tree();
                    this.view.toc_tree_collapse_all();
                }
            }
            Ok(())
        })
    }

    /// `updateElementsByKey(keys)`.
    fn update_elements_by_key(&self, keys: &HashSet<String>) {
        if !keys.is_empty() {
            if let Some(search_result_model) = self.search_result_model() {
                for key in keys {
                    search_result_model.update_elements_by_name(key);
                }
            }

            if let Some(settings_tree_model) = self.settings_tree_model.value() {
                for key in keys {
                    settings_tree_model.update_elements_by_name(key);
                }
            }

            for key in keys {
                self.render_tree(key, false);
            }
        } else {
            self.render_tree("", false);
        }
    }

    /// `renderTree(key?, force = false)`.
    pub fn render_tree(&self, key: &str, force: bool) {
        let key = if self.pending_assignment_refresh.get() { "" } else { key };
        if !force && !key.is_empty() && self.scheduled_refreshes.borrow().contains_key(key) {
            self.update_modified_label_for_key(key);
            return;
        }

        // If the context view is focused, delay rendering settings
        if self.view.context_view_focused() {
            self.schedule_refresh(FocusTarget::ContextView, key);
            return;
        }

        // If a setting control is currently focused, schedule a refresh for later
        if let Some(focused_setting) = self.view.focused_setting_control()
            && !force
        {
            // If a single setting is being refreshed, it's ok to refresh now if that is not the focused setting
            if !key.is_empty() {
                if focused_setting.key == key &&
                    // update `list`s live, as they have a separate "submit edit" step built in before this
                    focused_setting.parent_element_is_list == Some(false)
                {
                    self.update_modified_label_for_key(key);
                    self.schedule_refresh(FocusTarget::Setting(focused_setting.key), key);
                    return;
                }
            } else {
                self.schedule_refresh(FocusTarget::Setting(focused_setting.key), "");
                return;
            }
        }

        if self.pending_assignment_refresh.get() {
            self.pending_assignment_refresh.set(false);
            if let Some(search_result_model) = self.search_result_model() {
                search_result_model.update_children();
            }
            self.refresh_toc_tree();
        }

        self.render_result_count_messages(false);

        if !key.is_empty() {
            match self.current_elements_by_name(key).filter(|elements| !elements.is_empty()) {
                Some(elements) => {
                    if elements.len() >= 2 {
                        console::warn(&format!("More than one setting with key {key} found"));
                    }
                    self.refresh_single_element(&elements[0]);
                }
                // Refresh requested for a key that we don't know about
                None => (),
            }
        } else {
            self.refresh_tree();
        }
    }

    /// `refreshSingleElement(element)`.
    fn refresh_single_element(&self, element: &Rc<SettingsTreeSettingElement>) {
        if self.view.is_visible()
            && self.view.settings_tree_has_element(element)
            && (element.setting.deprecation_message.as_deref().is_none_or(str::is_empty) || element.setting.deprecation_message_show_in_settings.unwrap_or(false) || element.is_configured())
        {
            self.view.settings_tree_rerender(element);
        }
    }

    /// `refreshTree()`.
    pub fn refresh_tree(&self) {
        if self.view.is_visible()
            && let Some(root) = self.current_root()
        {
            self.view.settings_tree_set_children(create_group_iterator(&root));
        }
    }

    /// `refreshTOCTree()`.
    fn refresh_toc_tree(&self) {
        if self.view.is_visible() {
            let toc_tree_model = self.toc_tree_model();
            toc_tree_model.update();
            self.view.toc_tree_set_children(create_toc_iterator(&toc_tree_model.children()));
        }
    }

    /// `updateModifiedLabelForKey(key)`.
    fn update_modified_label_for_key(&self, key: &str) {
        if !self.has_current_settings_model() {
            return;
        }
        let data_elements = self.current_elements_by_name(key);
        // all elements are either configured or not
        let is_modified = data_elements.as_ref().and_then(|elements| elements.first()).is_some_and(|element| element.is_configured());
        self.view.set_setting_configured_class(key, is_modified);
    }

    /// `scheduleRefresh(element, key = '')`.
    fn schedule_refresh(&self, target: FocusTarget, key: &str) {
        if !key.is_empty() && self.scheduled_refreshes.borrow().contains_key(key) {
            return;
        }

        if key.is_empty() {
            let stores: Vec<Rc<DisposableStore>> = self.scheduled_refreshes.borrow_mut().drain().map(|(_, store)| store).collect();
            for store in stores {
                store.dispose();
            }
        }

        let store = DisposableStore::new();
        let weak = self.weak.clone();
        let blurred_key = key.to_owned();
        store.add(self.view.track_blur(
            target,
            Box::new(move || {
                let Some(this) = weak.upgrade() else {
                    return;
                };
                let store = this.scheduled_refreshes.borrow_mut().remove(&blurred_key);
                if let Some(store) = store {
                    store.dispose();
                }
                let _ = this.on_config_update(Some(HashSet::from([blurred_key.clone()])), false, false);
            }),
        ));
        self.scheduled_refreshes.borrow_mut().insert(key.to_owned(), store);
    }

    // ---- busca ----

    /// `onDidDismissExtensionSetting(extensionId)`.
    pub fn on_did_dismiss_extension_setting(&self, extension_id: &str) {
        let joined = {
            let mut dismissed_extension_settings = self.dismissed_extension_settings.borrow_mut();
            if !dismissed_extension_settings.iter().any(|dismissed| dismissed == extension_id) {
                dismissed_extension_settings.push(extension_id.to_owned());
            }
            dismissed_extension_settings.join(&DISMISSED_EXTENSION_SETTINGS_DELIMITER.to_string())
        };
        self.services.storage_service.store(DISMISSED_EXTENSION_SETTINGS_STORAGE_KEY, StorageValue::String(joined), StorageScope::Profile, StorageTarget::User);
        let _ = self.on_config_update(None, true, false);
    }

    /// `onDidSettingsTargetChange(target)`.
    pub fn on_did_settings_target_change(&self, target: SettingsTarget) {
        self.view_state.borrow_mut().settings_target = target;

        // TODO Instead of rebuilding the whole model, refresh and uncache the inspected setting value
        let _ = self.on_config_update(None, true, false);
    }

    /// `onDidToggleAiSearch()`, com o estado marcado da ação (`showAiResultsAction.checked ?? false`).
    pub fn on_did_toggle_ai_search(&self, checked: bool) {
        if let Some(search_result_model) = self.search_result_model() {
            search_result_model.set_show_ai_results(checked);
            self.render_result_count_messages(false);
            self.on_did_finish_search(true, None);
        }
    }

    /// `updateSearchPlaceholder()`: hints at history navigation (up/down arrows) once the user has search history.
    fn update_search_placeholder(&self) {
        let has_history = !self.view.search_history().is_empty();
        self.view.set_search_placeholder(&if has_history { search_box_placeholder_with_history() } else { search_box_label() });
    }

    /// `updateInputAriaLabel()`.
    fn update_input_aria_label(&self) {
        let mut label = search_box_label();
        if let Some(search_result_label) = self.search_result_label.borrow().as_deref().filter(|label| !label.is_empty()) {
            label.push_str(&format!(". {search_result_label}"));
        }

        if let Some(last_synced_label) = self.last_synced_label.borrow().as_deref().filter(|label| !label.is_empty()) {
            label.push_str(&format!(". {last_synced_label}"));
        }

        self.view.update_search_aria_label(&label);
    }

    /// `saveSearchHistory()`.
    fn save_search_history(&self) {
        let history = self.view.search_history();
        if !history.is_empty() {
            let serialized = Dynamic::array(history.iter().map(|entry| Dynamic::string(entry)));
            self.services.storage_service.store(
                SEARCH_HISTORY_STORAGE_KEY,
                StorageValue::String(je_base::dynamic::json_stringify(&serialized).unwrap_or_default()),
                StorageScope::Profile,
                StorageTarget::Machine,
            );
        } else {
            self.services.storage_service.remove(SEARCH_HISTORY_STORAGE_KEY, StorageScope::Profile);
        }
    }

    /// `onSearchInputChanged(expandResults)`.
    pub fn on_search_input_changed(&self, expand_results: bool) -> Promise<()> {
        let this = self.me();
        async_fn(async move {
            if !this.has_current_settings_model() {
                // Initializing search widget value
                return Ok(());
            }

            let query = js_trim(&this.view.search_value()).to_owned();
            this.view_state.borrow_mut().query = Some(query.clone());
            if !query.is_empty() {
                this.view.add_search_to_history();
                this.update_search_placeholder();
                this.save_search_history();
            }
            this.trigger_search(&query.replace('\u{203A}', " "), expand_results).await
        })
    }

    /// `toggleTocBySearchBehaviorType()`: toggles the visibility of the Settings editor table of contents during a search
    /// depending on the behavior.
    fn toggle_toc_by_search_behavior_type(&self) {
        let toc_behavior = self.services.tree.configuration_service.get_value(Some(SEARCH_TOC_BEHAVIOR_KEY), None);
        let hide_toc = matches!(&toc_behavior, Dynamic::String(behavior) if behavior.to_string_lossy() == "hide");
        if hide_toc {
            self.view.hide_toc();
        } else {
            self.view.layout_split_view();
        }
    }

    /// `triggerSearch(query, expandResults)`.
    pub fn trigger_search(&self, query: &str, expand_results: bool) -> Promise<()> {
        let this = self.me();
        let mut query = query.to_owned();
        async_fn(async move {
            let progress_runner = this.services.editor_progress_service.show(ProgressTotal::Infinite, Some(800));
            let show_advanced = this.can_show_advanced_settings();
            {
                let mut view_state = this.view_state.borrow_mut();
                view_state.tag_filters = Some(HashSet::new());
                view_state.extension_filters = Some(HashSet::new());
                view_state.feature_filters = Some(HashSet::new());
                view_state.id_filters = Some(HashSet::new());
                view_state.language_filter = None;
                if !query.is_empty() {
                    let parsed_query = parse_query(&query);
                    query = parsed_query.query;
                    view_state.tag_filters.get_or_insert_with(HashSet::new).extend(parsed_query.tags);
                    view_state.extension_filters.get_or_insert_with(HashSet::new).extend(parsed_query.extension_filters);
                    view_state.feature_filters.get_or_insert_with(HashSet::new).extend(parsed_query.feature_filters);
                    view_state.id_filters.get_or_insert_with(HashSet::new).extend(parsed_query.id_filters);
                    view_state.language_filter = parsed_query.language_filter;
                }
            }

            if show_advanced != this.can_show_advanced_settings() {
                this.on_config_update(None, false, false).await?;
            }

            let language_filter = this.view_state.borrow().language_filter.clone();
            this.view.update_language_filter_indicators(language_filter.as_deref());

            if !query.is_empty() && query != "@" {
                query = parse_setting_from_json(&query).unwrap_or(query);
                this.trigger_filter_preferences(&query, expand_results, progress_runner).await?;
                this.toggle_toc_by_search_behavior_type();
            } else {
                let has_filters = {
                    let view_state = this.view_state.borrow();
                    let non_empty = |filters: &Option<HashSet<String>>| filters.as_ref().is_some_and(|filters| !filters.is_empty());
                    non_empty(&view_state.tag_filters)
                        || non_empty(&view_state.extension_filters)
                        || non_empty(&view_state.feature_filters)
                        || non_empty(&view_state.id_filters)
                        || view_state.language_filter.as_deref().is_some_and(|language_filter| !language_filter.is_empty())
                };
                let filter_model = if has_filters { Some(this.create_filter_model()) } else { None };
                this.set_search_result_model(filter_model);

                this.search_delayer.cancel();
                if let Some(search_in_progress) = this.search_in_progress.borrow_mut().take() {
                    search_in_progress.dispose_with(true);
                }

                if expand_results {
                    this.view.toc_tree_set_focus(&[]);
                    this.view_state.borrow_mut().category_filter = None;
                }
                this.toc_tree_model().set_current_search_model(this.search_result_model());
                *this.rendered_search_query.borrow_mut() = this.view_state.borrow().query.clone();

                if this.search_result_model().is_some() {
                    // Added a filter model
                    if expand_results {
                        this.view.toc_tree_set_selection(&[]);
                        this.view.toc_tree_expand_all();
                    }
                    this.refresh_toc_tree();
                    this.render_result_count_messages(false);
                    this.refresh_tree();
                    this.toggle_toc_by_search_behavior_type();
                } else if !this.toc_tree_disposed.get() {
                    // Leaving search mode
                    this.view.toc_tree_collapse_all();
                    this.refresh_toc_tree();
                    this.render_result_count_messages(false);
                    this.refresh_tree();
                    this.view.layout_split_view();
                }
                progress_runner.done();
            }
            Ok(())
        })
    }

    /// Return a fake SearchResultModel which can hold a flat list of all settings, to be filtered (@modified etc).
    pub fn create_filter_model(&self) -> Rc<SearchResultModel> {
        let is_workspace_trusted = self.services.workspace_trust_management_service.is_workspace_trusted();
        let filter_model = SearchResultModel::new(self.view_state.clone(), self.settings_order_by_toc_index.borrow().clone(), is_workspace_trusted, self.services.tree.clone());

        let mut full_result = ISearchResult { filter_matches: Vec::new(), exact_match: false };
        let should_show_advanced = self.can_show_advanced_settings();
        let default_settings_editor_model = self.default_settings_editor_model.borrow().clone().expect("o modelo padrão do editor de configurações");
        for group in default_settings_editor_model.settings_groups().iter().skip(1) {
            for section in &group.sections {
                for setting in &section.settings {
                    if !should_show_advanced && !self.should_show_setting(setting) {
                        continue;
                    }
                    full_result.filter_matches.push(ISettingMatch {
                        setting: setting.clone(),
                        matches: Some(Vec::new()),
                        match_type: SettingMatchType::None,
                        key_match_score: 0.0,
                        score: 0.0,
                        provider_name: Some(FILTER_MODEL_SEARCH_PROVIDER_NAME.to_owned()),
                    });
                }
            }
        }

        filter_model.set_result(SearchResultIdx::Local, Some(full_result));
        filter_model
    }

    /// `triggerFilterPreferences(query, expandResults, progressRunner)`.
    fn trigger_filter_preferences(&self, query: &str, expand_results: bool, progress_runner: Rc<dyn IProgressRunner>) -> Promise<()> {
        if let Some(search_in_progress) = self.search_in_progress.borrow_mut().take() {
            search_in_progress.dispose_with(true);
        }

        let search_in_progress = CancellationTokenSource::new(None);
        let token = search_in_progress.token();
        *self.search_in_progress.borrow_mut() = Some(search_in_progress);

        let this = self.me();
        let query = query.to_owned();
        self.search_delayer.trigger(
            move || {
                MaybePromise::Promise(async_fn(async move {
                    if token.is_cancellation_requested() {
                        return Ok(());
                    }
                    this.disable_ai_search_toggle();
                    let local_results = this.do_local_search(&query, token.clone()).await?;
                    let Some(search_result_model) = this.search_result_model() else {
                        return Ok(());
                    };
                    if token.is_cancellation_requested() {
                        return Ok(());
                    }
                    search_result_model.set_show_ai_results(false);

                    if local_results.as_ref().is_some_and(|local_results| !local_results.filter_matches.is_empty()) {
                        // The remote results might take a while and
                        // are always appended to the end anyway, so
                        // show some results now.
                        this.on_did_finish_search(expand_results, None);
                    }

                    if local_results.as_ref().is_none_or(|local_results| !local_results.exact_match) {
                        this.do_remote_search(&query, token.clone()).await?;
                    }
                    if token.is_cancellation_requested() {
                        return Ok(());
                    }

                    if let Some(ai_search_promise) = this.ai_search_promise.borrow().as_ref() {
                        ai_search_promise.cancel();
                    }

                    // Kick off an AI search in the background if the toggle is shown.
                    // We purposely do not await it.
                    if this.view.ai_results_toggle_shown() {
                        let editor = this.clone();
                        let ai_query = query.clone();
                        *this.ai_search_promise.borrow_mut() = Some(create_cancelable_promise(move |token| {
                            editor.do_ai_search(&ai_query, token).then_result({
                                let editor = editor.clone();
                                move |result| {
                                    match result {
                                        Ok(results) => {
                                            if results.is_some() {
                                                editor.view.enable_ai_results_action();
                                                editor.render_result_count_messages(true);
                                            }
                                        }
                                        Err(error) => {
                                            if !errors::is_cancellation_error(&error) {
                                                editor.services.log_service.trace(&format!("Error during AI settings search: {error}"));
                                            }
                                        }
                                    }
                                    Ok(())
                                }
                            })
                        }));
                    }

                    this.on_did_finish_search(expand_results, Some(progress_runner));
                    Ok(())
                }))
            },
            None,
        )
    }

    /// `onDidFinishSearch(expandResults, progressRunner)`.
    fn on_did_finish_search(&self, expand_results: bool, progress_runner: Option<Rc<dyn IProgressRunner>>) {
        self.toc_tree_model().set_current_search_model(self.search_result_model());
        *self.rendered_search_query.borrow_mut() = self.view_state.borrow().query.clone();
        if expand_results {
            self.view.toc_tree_set_focus(&[]);
            self.view_state.borrow_mut().category_filter = None;
            self.view.toc_tree_expand_all();
            self.view.settings_tree_scroll_to_top();
        }
        self.refresh_toc_tree();
        self.render_tree("", true);
        if let Some(progress_runner) = progress_runner {
            progress_runner.done();
        }
    }

    /// `doLocalSearch(query, token)`.
    fn do_local_search(&self, query: &str, token: CancellationToken) -> Promise<Option<ISearchResult>> {
        let local_search_provider = self.services.preferences_search_service.get_local_search_provider(query);
        self.search_with_provider(SearchResultIdx::Local, local_search_provider, STRING_MATCH_SEARCH_PROVIDER_NAME, token)
    }

    /// `doRemoteSearch(query, token)`.
    fn do_remote_search(&self, query: &str, token: CancellationToken) -> Promise<Option<ISearchResult>> {
        match self.services.preferences_search_service.get_remote_search_provider(query, false) {
            Some(remote_search_provider) => self.search_with_provider(SearchResultIdx::Remote, remote_search_provider, TF_IDF_SEARCH_PROVIDER_NAME, token),
            None => Promise::resolved(None),
        }
    }

    /// `doAiSearch(query, token)`.
    fn do_ai_search(&self, query: &str, token: CancellationToken) -> Promise<Option<ISearchResult>> {
        let this = self.me();
        let query = query.to_owned();
        async_fn(async move {
            let Some(ai_search_provider) = this.services.preferences_search_service.get_ai_search_provider(&query) else {
                return Ok(None);
            };

            let ai_search_provider: Rc<dyn ISearchProvider> = ai_search_provider;
            let embeddings_results = this.search_with_provider(SearchResultIdx::Embeddings, ai_search_provider, EMBEDDINGS_SEARCH_PROVIDER_NAME, token.clone()).await?;
            let Some(embeddings_results) = embeddings_results else {
                return Ok(None);
            };
            if token.is_cancellation_requested() {
                return Ok(None);
            }

            let llm_results = this.get_llm_ranked_results(&query, token.clone()).await?;
            if token.is_cancellation_requested() {
                return Ok(None);
            }

            let mut filter_matches = embeddings_results.filter_matches;
            filter_matches.extend(llm_results.map(|llm_results| llm_results.filter_matches).unwrap_or_default());
            Ok(Some(ISearchResult { filter_matches, exact_match: false }))
        })
    }

    /// `getLLMRankedResults(query, token)`.
    fn get_llm_ranked_results(&self, query: &str, token: CancellationToken) -> Promise<Option<ISearchResult>> {
        let this = self.me();
        let query = query.to_owned();
        async_fn(async move {
            let Some(ai_search_provider) = this.services.preferences_search_service.get_ai_search_provider(&query) else {
                return Ok(None);
            };

            let mut stop_watch = StopWatch::new(Some(false));
            let result = ai_search_provider.get_llm_ranked_results(token.clone()).await?;
            stop_watch.stop();

            if token.is_cancellation_requested() {
                return Ok(None);
            }

            // Only log the elapsed time if there are actual results.
            if result.as_ref().is_some_and(|result| !result.filter_matches.is_empty()) {
                this.log_search_performance(LLM_RANKED_SEARCH_PROVIDER_NAME, stop_watch.elapsed());
            }

            this.search_result_model().expect("o modelo dos resultados da busca").set_result(SearchResultIdx::AiSelected, result.clone());
            Ok(result)
        })
    }

    /// `searchWithProvider(type, searchProvider, providerName, token)`.
    pub fn search_with_provider(&self, type_: SearchResultIdx, search_provider: Rc<dyn ISearchProvider>, provider_name: &str, token: CancellationToken) -> Promise<Option<ISearchResult>> {
        let this = self.me();
        let provider_name = provider_name.to_owned();
        async_fn(async move {
            let mut stop_watch = StopWatch::new(Some(false));
            let default_settings_editor_model: Rc<dyn ISettingsEditorModel> =
                this.default_settings_editor_model.borrow().clone().expect("o modelo padrão do editor de configurações");
            let mut result = Self::search_preferences_model(default_settings_editor_model, search_provider, token.clone()).await?;
            stop_watch.stop();

            if token.is_cancellation_requested() {
                // Handle cancellation like this because cancellation is lost inside the search provider due to async/await
                return Ok(None);
            }

            // Filter out advanced settings unless the advanced tag is explicitly set or setting matches an ID filter
            if let Some(result) = result.as_mut()
                && !this.can_show_advanced_settings()
            {
                result.filter_matches.retain(|filter_match| this.should_show_setting(&filter_match.setting));
            }

            // Only log the elapsed time if there are actual results.
            if result.as_ref().is_some_and(|result| !result.filter_matches.is_empty()) {
                this.log_search_performance(&provider_name, stop_watch.elapsed());
            }

            if this.search_result_model().is_none() {
                let is_workspace_trusted = this.services.workspace_trust_management_service.is_workspace_trusted();
                let search_result_model = SearchResultModel::new(this.view_state.clone(), this.settings_order_by_toc_index.borrow().clone(), is_workspace_trusted, this.services.tree.clone());
                this.set_search_result_model(Some(search_result_model));
            }
            this.search_result_model().expect("o modelo dos resultados da busca").set_result(type_, result.clone());
            Ok(result)
        })
    }

    /// `logSearchPerformance(providerName, elapsed)`.
    fn log_search_performance(&self, provider_name: &str, elapsed: f64) {
        self.services.telemetry_service.public_log2(
            "settingsEditor.searchPerformance",
            Some(Dynamic::object(vec![("providerName", Dynamic::string(provider_name)), ("elapsedMs", Dynamic::Number(elapsed))])),
        );
    }

    /// `renderResultCountMessages(showAiResultsMessage)`.
    pub fn render_result_count_messages(&self, show_ai_results_message: bool) {
        if !self.has_current_settings_model() {
            return;
        }

        let has_tag_filters = self.view_state.borrow().tag_filters.as_ref().is_some_and(|tag_filters| !tag_filters.is_empty());
        self.view.set_clear_filter_link_display(if has_tag_filters { "initial" } else { "none" });

        let Some(search_result_model) = self.search_result_model() else {
            if self.view.count_display() != "none" {
                *self.search_result_label.borrow_mut() = None;
                self.update_input_aria_label();
                self.view.set_count_display("none");
                self.view.set_count_text("");
                self.view.layout();
            }

            self.view.set_root_class("no-results", false);
            self.view.set_body_visibility("visible");
            return;
        };

        let count = search_result_model.get_unique_results_count();
        let result_string = if show_ai_results_message {
            match count {
                0 => localize("noResultsWithAiAvailable", "No Settings Found. AI Results Available", &[]),
                1 => localize("oneResultWithAiAvailable", "1 Setting Found. AI Results Available", &[]),
                _ => localize("moreThanOneResultWithAiAvailable", "{0} Settings Found. AI Results Available", &[count.to_string().as_str()]),
            }
        } else {
            match count {
                0 => localize("noResults", "No Settings Found", &[]),
                1 => localize("oneResult", "1 Setting Found", &[]),
                _ => localize("moreThanOneResult", "{0} Settings Found", &[count.to_string().as_str()]),
            }
        };

        *self.search_result_label.borrow_mut() = Some(result_string.clone());
        self.update_input_aria_label();
        self.view.set_count_text(&result_string);
        self.view.aria_status(&result_string);

        if self.view.count_display() != "block" {
            self.view.set_count_display("block");
        }
        self.view.layout();
        self.view.set_root_class("no-results", count == 0);
        self.view.set_body_visibility(if count == 0 { "hidden" } else { "visible" });
    }

    /// `_searchPreferencesModel(model, provider, token)`: um erro que não é de cancelamento vira `null`.
    fn search_preferences_model(model: Rc<dyn ISettingsEditorModel>, provider: Rc<dyn ISearchProvider>, token: CancellationToken) -> Promise<Option<ISearchResult>> {
        provider.search_model(model, token).then_result(|result| match result {
            Err(error) if !errors::is_cancellation_error(&error) => Ok(None),
            result => result,
        })
    }
}

impl IDisposable for SettingsEditor2 {
    fn dispose(&self) {
        let scheduled_refreshes: Vec<Rc<DisposableStore>> = self.scheduled_refreshes.borrow_mut().drain().map(|(_, store)| store).collect();
        for store in scheduled_refreshes {
            store.dispose();
        }
        if let Some(search_in_progress) = self.search_in_progress.borrow_mut().take() {
            search_in_progress.dispose_with(true);
        }
        self.base.dispose();
    }
}

/// O `findGroup` de `onConfigUpdate`: o grupo de `id` entre as raízes e os descendentes delas.
fn find_group(roots: &[Rc<SettingsTreeGroupElement>], id: &str) -> Option<Rc<SettingsTreeGroupElement>> {
    for group in roots {
        if group.id() == id {
            return Some(group.clone());
        }
        for child in group.children() {
            if let SettingsTreeGroupChild::Group(child) = child
                && let Some(found) = find_group(&[child], id)
            {
                return Some(found);
            }
        }
    }
    None
}

/// `contributes?.configuration` do manifesto: o título do grupo (`title` do objeto, ou do único item da lista).
fn extension_group_title_from_manifest(manifest: &Value) -> Option<String> {
    let contributes_configuration = manifest.get("contributes").and_then(|contributes| contributes.get("configuration"))?;
    let title = |configuration: &Value| configuration.get("title").and_then(Value::as_str).map(str::to_owned);
    match contributes_configuration {
        Value::Array(configurations) if configurations.len() == 1 => title(&configurations[0]),
        Value::Array(_) => None,
        configuration => title(configuration),
    }
}

/// `extension.displayName ?? extension.name ?? extensionId`.
fn extension_display_name(extension: &GalleryExtension, extension_id: &str) -> String {
    if !extension.display_name.is_empty() {
        extension.display_name.clone()
    } else if !extension.name.is_empty() {
        extension.name.clone()
    } else {
        extension_id.to_owned()
    }
}

/// `parseSettingFromJSON(query)`: a chave do primeiro `"chave": ` da consulta (`/"([a-zA-Z.]+)": /`).
fn parse_setting_from_json(query: &str) -> Option<String> {
    let bytes = query.as_bytes();
    for (start, _) in query.match_indices('"') {
        let key_start = start + 1;
        let key_len = bytes[key_start..].iter().take_while(|byte| byte.is_ascii_alphabetic() || **byte == b'.').count();
        if key_len > 0 && bytes[key_start + key_len..].starts_with(b"\": ") {
            return Some(query[key_start..key_start + key_len].to_owned());
        }
    }
    None
}

/// As sugestões de filtro do campo de busca (`SettingsEditor2.SUGGESTIONS`), com as condicionais do construtor.
pub fn settings_editor_suggestions(is_sessions_window: bool) -> Vec<String> {
    let mut suggestions: Vec<String> = vec![
        format!("@{MODIFIED_SETTING_TAG}"),
        "@tag:notebookLayout".to_owned(),
        "@tag:notebookOutputLayout".to_owned(),
        format!("@tag:{REQUIRE_TRUSTED_WORKSPACE_SETTING_TAG}"),
        format!("@tag:{WORKSPACE_TRUST_SETTING_TAG}"),
        "@tag:sync".to_owned(),
        "@tag:usesOnlineServices".to_owned(),
        "@tag:telemetry".to_owned(),
        "@tag:accessibility".to_owned(),
        "@tag:preview".to_owned(),
        "@tag:experimental".to_owned(),
        format!("@tag:{EXP_ASSIGNMENT_SETTING_TAG}"),
        "@tag:agentMerge".to_owned(),
        format!("@tag:{ADVANCED_SETTING_TAG}"),
        format!("@{ID_SETTING_TAG}"),
        format!("@{EXTENSION_SETTING_TAG}"),
    ];
    for feature in ["scm", "explorer", "search", "debug", "extensions", "terminal", "task", "problems", "output", "comments", "remote", "timeline", "notebook", "chat"] {
        suggestions.push(format!("@{FEATURE_SETTING_TAG}{feature}"));
    }
    suggestions.push(format!("@{POLICY_SETTING_TAG}"));
    if ENABLE_LANGUAGE_FILTER {
        suggestions.push(format!("@{LANGUAGE_SETTING_TAG}"));
    }
    if is_sessions_window {
        suggestions.push(format!("@{AGENTS_WINDOW_SETTING_TAG}"));
    }
    suggestions
}

/// `clearSearchFilters()`: o valor da busca sem as palavras que são sugestões de filtro.
pub fn clear_search_filters(query: &str, is_sessions_window: bool) -> String {
    let suggestions = settings_editor_suggestions(is_sessions_window);
    query
        .split(' ')
        .filter(|word| !word.is_empty() && !suggestions.iter().any(|suggestion| word.starts_with(suggestion.as_str())))
        .collect::<Vec<_>>()
        .join(" ")
}
