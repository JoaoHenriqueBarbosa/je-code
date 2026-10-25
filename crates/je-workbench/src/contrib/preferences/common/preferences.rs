//! Porte parcial de `src/vs/workbench/contrib/preferences/common/preferences.ts`: as constantes de comando, de tag e de
//! provedor de busca, os contratos `IPreferencesSearchService`, `ISearchProvider`, `IRemoteSearchProvider` e
//! `IAiSearchProvider` (a busca de configurações), o `getExperimentalExtensionToggleData`, o `compareTwoNullableNumbers`,
//! as descrições dos indicadores e o `wordifyKey`. Ficam de fora o que depende de serviços ainda não portados: as
//! `RawContextKey` do editor de configurações e de atalhos, `IWorkbenchSettingsConfiguration` e `IEndpointDetails`. Cada
//! um entra com o porte do serviço de que depende.
//!
//! Divergências:
//! - **`knownAcronyms`** (um `Set`) é a lista [`KNOWN_ACRONYMS`] e **`knownTermMappings`** (um `Map`, cuja ordem de
//!   inserção define a ordem das trocas do `wordifyKey`) é a lista de pares [`KNOWN_TERM_MAPPINGS`].
//! - **`wordifyKey`** trabalha em UTF-16 com a `RegExp` ECMAScript do `zyarr`, como o original, e as expressões são
//!   compiladas uma vez por thread.
//! - **`WorkbenchSettingsEditorSettings`** (um `enum` de strings) é o enum [`WorkbenchSettingsEditorSettings`] com
//!   [`WorkbenchSettingsEditorSettings::as_str`].
//! - **`PREVIEW_INDICATOR_DESCRIPTION`**, **`EXPERIMENTAL_INDICATOR_DESCRIPTION`** e **`ADVANCED_INDICATOR_DESCRIPTION`**
//!   (constantes que chamam `localize` ao carregar o módulo) são funções, porque o idioma é lido na chamada.
//! - **Provedores de busca.** Os quatro contratos são traits sobre `Rc<dyn ...>`; o `searchModel` recebe o modelo como
//!   `Rc<dyn ISettingsEditorModel>` e devolve uma `Promise` de `Option<ISearchResult>` (o `ISearchResult | null`).
//! - **`cachedExtensionToggleData`** (uma variável do módulo) é um `thread_local`, e o `getExperimentalExtensionToggleData`
//!   é uma função `async` que devolve uma `Promise`. O `IGalleryExtension` é o [`GalleryExtension`] do `je-extensions`.

use std::cell::RefCell;
use std::rc::Rc;

use indexmap::IndexMap;
use je_base::JsString;
use je_base::async_::{async_fn, race_timeout};
use je_base::cancellation::CancellationToken;
use je_base::js_regexp::replace_all;
use je_base::nls::localize;
use je_base::product::IExtensionRecommendations;
use je_base::promise::Promise;
use je_extensions::management::{ExtensionInfo, ExtensionQueryOptions};
use je_instantiation::service_id;
use je_platform::product::IProductService;
use zyarr::RegExp;

use crate::services::extension_management::extension_gallery_service::{IExtensionGalleryService, IGalleryExtension};
use crate::services::extension_management::service_contracts::IChatEntitlementService;
use crate::services::preferences::preferences::{ISearchResult, ISettingsEditorModel};

/// `ISearchProvider`.
pub trait ISearchProvider {
    /// `searchModel(preferencesModel, token)`.
    fn search_model(&self, preferences_model: Rc<dyn ISettingsEditorModel>, token: CancellationToken) -> Promise<Option<ISearchResult>>;
}

/// `IRemoteSearchProvider`.
pub trait IRemoteSearchProvider: ISearchProvider {
    /// `setFilter(filter)`.
    fn set_filter(&self, filter: &str);
}

/// `IAiSearchProvider`.
pub trait IAiSearchProvider: IRemoteSearchProvider {
    /// `getLLMRankedResults(token)`.
    fn get_llm_ranked_results(&self, token: CancellationToken) -> Promise<Option<ISearchResult>>;
}

/// `IPreferencesSearchService`.
pub trait IPreferencesSearchService {
    /// `getLocalSearchProvider(filter)`.
    fn get_local_search_provider(&self, filter: &str) -> Rc<dyn ISearchProvider>;

    /// `getRemoteSearchProvider(filter, newExtensionsOnly?)`.
    fn get_remote_search_provider(&self, filter: &str, new_extensions_only: bool) -> Option<Rc<dyn ISearchProvider>>;

    /// `getAiSearchProvider(filter)`.
    fn get_ai_search_provider(&self, filter: &str) -> Option<Rc<dyn IAiSearchProvider>>;
}

service_id!(pub I_PREFERENCES_SEARCH_SERVICE: dyn IPreferencesSearchService = "preferencesSearchService");

/// `ExtensionToggleData`.
#[derive(Clone)]
pub struct ExtensionToggleData {
    pub settings_editor_recommended_extensions: IndexMap<String, IExtensionRecommendations>,
    pub recommended_extensions_gallery_info: IndexMap<String, IGalleryExtension>,
}

thread_local! {
    /// `cachedExtensionToggleData`.
    static CACHED_EXTENSION_TOGGLE_DATA: RefCell<Option<ExtensionToggleData>> = const { RefCell::new(None) };
}

/// `getExperimentalExtensionToggleData`.
pub fn get_experimental_extension_toggle_data(
    chat_entitlement_service: Rc<dyn IChatEntitlementService>,
    extension_gallery_service: Rc<dyn IExtensionGalleryService>,
    product_service: Rc<dyn IProductService>,
) -> Promise<Option<ExtensionToggleData>> {
    async_fn(async move {
        if !ENABLE_EXTENSION_TOGGLE_SETTINGS {
            return Ok(None);
        }

        if !extension_gallery_service.is_enabled() {
            return Ok(None);
        }

        let sentiment = chat_entitlement_service.sentiment();
        if sentiment.hidden || sentiment.disabled {
            return Ok(None);
        }

        if let Some(cached) = CACHED_EXTENSION_TOGGLE_DATA.with(|cached| cached.borrow().clone()) {
            return Ok(Some(cached));
        }

        let Some(extension_recommendations) = product_service.extension_recommendations() else {
            return Ok(None);
        };

        let mut settings_editor_recommended_extensions: IndexMap<String, IExtensionRecommendations> = IndexMap::new();
        for (extension_id, extension_info) in extension_recommendations {
            if extension_info.on_settings_editor_open.is_some() {
                settings_editor_recommended_extensions.insert(extension_id.clone(), extension_info.clone());
            }
        }

        let mut recommended_extensions_gallery_info: IndexMap<String, IGalleryExtension> = IndexMap::new();
        for key in settings_editor_recommended_extensions.keys() {
            // Recommend prerelease if not on Stable.
            let is_stable = product_service.quality() == Some("stable");
            let extension_info = ExtensionInfo {
                id: key.clone(),
                uuid: None,
                version: None,
                pre_release: Some(!is_stable),
                has_pre_release: None,
                current_version: None,
            };
            let query = extension_gallery_service.get_extensions(vec![extension_info], ExtensionQueryOptions::default(), CancellationToken::None);
            match race_timeout(&query, EXTENSION_FETCH_TIMEOUT_MS, None).await {
                Ok(Some(mut extensions)) if extensions.len() == 1 => {
                    recommended_extensions_gallery_info.insert(key.clone(), extensions.remove(0));
                }
                // same as network connection fail. we do not want a blank settings page: https://github.com/microsoft/vscode/issues/195722
                // so instead of returning partial data we return undefined here
                //
                // Network connection fail. Return nothing rather than partial data.
                _ => return Ok(None),
            }
        }

        let data = ExtensionToggleData { settings_editor_recommended_extensions, recommended_extensions_gallery_info };
        CACHED_EXTENSION_TOGGLE_DATA.with(|cached| *cached.borrow_mut() = Some(data.clone()));
        Ok(Some(data))
    })
}

pub const SETTINGS_EDITOR_COMMAND_CLEAR_SEARCH_RESULTS: &str = "settings.action.clearSearchResults";
pub const SETTINGS_EDITOR_COMMAND_SHOW_AI_RESULTS: &str = "settings.action.showAIResults";
pub const SETTINGS_EDITOR_COMMAND_TOGGLE_AI_SEARCH: &str = "settings.action.toggleAiSearch";
pub const SETTINGS_EDITOR_COMMAND_SHOW_CONTEXT_MENU: &str = "settings.action.showContextMenu";
pub const SETTINGS_EDITOR_COMMAND_SUGGEST_FILTERS: &str = "settings.action.suggestFilters";

pub const KEYBINDINGS_EDITOR_COMMAND_SEARCH: &str = "keybindings.editor.searchKeybindings";
pub const KEYBINDINGS_EDITOR_COMMAND_CLEAR_SEARCH_RESULTS: &str = "keybindings.editor.clearSearchResults";
pub const KEYBINDINGS_EDITOR_COMMAND_CLEAR_SEARCH_HISTORY: &str = "keybindings.editor.clearSearchHistory";
pub const KEYBINDINGS_EDITOR_COMMAND_RECORD_SEARCH_KEYS: &str = "keybindings.editor.recordSearchKeys";
pub const KEYBINDINGS_EDITOR_COMMAND_SORTBY_PRECEDENCE: &str = "keybindings.editor.toggleSortByPrecedence";
pub const KEYBINDINGS_EDITOR_COMMAND_DEFINE: &str = "keybindings.editor.defineKeybinding";
pub const KEYBINDINGS_EDITOR_COMMAND_ADD: &str = "keybindings.editor.addKeybinding";
pub const KEYBINDINGS_EDITOR_COMMAND_DEFINE_WHEN: &str = "keybindings.editor.defineWhenExpression";
pub const KEYBINDINGS_EDITOR_COMMAND_ACCEPT_WHEN: &str = "keybindings.editor.acceptWhenExpression";
pub const KEYBINDINGS_EDITOR_COMMAND_REJECT_WHEN: &str = "keybindings.editor.rejectWhenExpression";
pub const KEYBINDINGS_EDITOR_COMMAND_REMOVE: &str = "keybindings.editor.removeKeybinding";
pub const KEYBINDINGS_EDITOR_COMMAND_RESET: &str = "keybindings.editor.resetKeybinding";
pub const KEYBINDINGS_EDITOR_COMMAND_COPY: &str = "keybindings.editor.copyKeybindingEntry";
pub const KEYBINDINGS_EDITOR_COMMAND_COPY_COMMAND: &str = "keybindings.editor.copyCommandKeybindingEntry";
pub const KEYBINDINGS_EDITOR_COMMAND_COPY_COMMAND_TITLE: &str = "keybindings.editor.copyCommandTitle";
pub const KEYBINDINGS_EDITOR_COMMAND_SHOW_SIMILAR: &str = "keybindings.editor.showConflicts";
pub const KEYBINDINGS_EDITOR_COMMAND_FOCUS_KEYBINDINGS: &str = "keybindings.editor.focusKeybindings";
pub const KEYBINDINGS_EDITOR_SHOW_DEFAULT_KEYBINDINGS: &str = "keybindings.editor.showDefaultKeybindings";
pub const KEYBINDINGS_EDITOR_SHOW_USER_KEYBINDINGS: &str = "keybindings.editor.showUserKeybindings";
pub const KEYBINDINGS_EDITOR_SHOW_EXTENSION_KEYBINDINGS: &str = "keybindings.editor.showExtensionKeybindings";

pub const MODIFIED_SETTING_TAG: &str = "modified";
pub const EXTENSION_SETTING_TAG: &str = "ext:";
pub const FEATURE_SETTING_TAG: &str = "feature:";
pub const ID_SETTING_TAG: &str = "id:";
pub const LANGUAGE_SETTING_TAG: &str = "lang:";
pub const GENERAL_TAG_SETTING_TAG: &str = "tag:";
pub const POLICY_SETTING_TAG: &str = "hasPolicy";
pub const AGENTS_WINDOW_SETTING_TAG: &str = "override:agentsWindow";
pub const WORKSPACE_TRUST_SETTING_TAG: &str = "workspaceTrust";
pub const REQUIRE_TRUSTED_WORKSPACE_SETTING_TAG: &str = "requireTrustedWorkspace";
pub const ADVANCED_SETTING_TAG: &str = "advanced";
pub const EXP_ASSIGNMENT_SETTING_TAG: &str = "expassigned";
pub const KEYBOARD_LAYOUT_OPEN_PICKER: &str = "workbench.action.openKeyboardLayoutPicker";

pub const ENABLE_LANGUAGE_FILTER: bool = true;

pub const ENABLE_EXTENSION_TOGGLE_SETTINGS: bool = true;
pub const EXTENSION_FETCH_TIMEOUT_MS: u64 = 1000;

pub const STRING_MATCH_SEARCH_PROVIDER_NAME: &str = "local";
pub const TF_IDF_SEARCH_PROVIDER_NAME: &str = "tfIdf";
pub const FILTER_MODEL_SEARCH_PROVIDER_NAME: &str = "filterModel";
pub const EMBEDDINGS_SEARCH_PROVIDER_NAME: &str = "embeddingsFull";
pub const LLM_RANKED_SEARCH_PROVIDER_NAME: &str = "llmRanked";

/// `WorkbenchSettingsEditorSettings`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum WorkbenchSettingsEditorSettings {
    ShowAISearchToggle,
    EnableNaturalLanguageSearch,
}

impl WorkbenchSettingsEditorSettings {
    /// O valor de texto do membro do `enum` do original.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::ShowAISearchToggle => "workbench.settings.showAISearchToggle",
            Self::EnableNaturalLanguageSearch => "workbench.settings.enableNaturalLanguageSearch",
        }
    }
}

/// `compareTwoNullableNumbers`: compara dois números opcionais de modo que os ausentes venham sempre depois dos
/// definidos (`Number.MAX_SAFE_INTEGER` no lugar do ausente).
pub fn compare_two_nullable_numbers(a: Option<f64>, b: Option<f64>) -> f64 {
    const MAX_SAFE_INTEGER: f64 = 9_007_199_254_740_991.0;
    let a_or_max = a.unwrap_or(MAX_SAFE_INTEGER);
    let b_or_max = b.unwrap_or(MAX_SAFE_INTEGER);
    if a_or_max < b_or_max {
        -1.0
    } else if a_or_max > b_or_max {
        1.0
    } else {
        0.0
    }
}

/// `PREVIEW_INDICATOR_DESCRIPTION`.
pub fn preview_indicator_description() -> String {
    localize(
        "previewIndicatorDescription",
        "Preview setting: this setting controls a new feature that is still under refinement yet ready to use. Feedback is welcome.",
        &[],
    )
}

/// `EXPERIMENTAL_INDICATOR_DESCRIPTION`.
pub fn experimental_indicator_description() -> String {
    localize(
        "experimentalIndicatorDescription",
        "Experimental setting: this setting controls a new feature that is actively being developed and may be unstable. It is subject to change or removal.",
        &[],
    )
}

/// `ADVANCED_INDICATOR_DESCRIPTION`.
pub fn advanced_indicator_description() -> String {
    localize(
        "advancedIndicatorDescription",
        "Advanced setting: this setting is intended for advanced scenarios and configurations. Only modify this if you know what it does.",
        &[],
    )
}

/// `knownAcronyms`.
pub const KNOWN_ACRONYMS: [&str; 11] = ["css", "html", "scss", "less", "json", "js", "ts", "ie", "id", "php", "scm"];

/// `knownTermMappings`, na ordem de inserção do `Map`.
pub const KNOWN_TERM_MAPPINGS: [(&str, &str); 10] = [
    ("power shell", "PowerShell"),
    ("powershell", "PowerShell"),
    ("javascript", "JavaScript"),
    ("typescript", "TypeScript"),
    ("github", "GitHub"),
    ("ocaml", "OCaml"),
    ("jet brains", "JetBrains"),
    ("jetbrains", "JetBrains"),
    ("re sharper", "ReSharper"),
    ("resharper", "ReSharper"),
];

/// As expressões de `wordifyKey`, compiladas uma vez por thread.
struct WordifyRegexes {
    dot_then_alphanumeric: RegExp,
    lower_then_upper: RegExp,
    consecutive_capitals: RegExp,
    leading_lower: RegExp,
    word: RegExp,
    term_mappings: Vec<(RegExp, &'static str)>,
}

impl WordifyRegexes {
    fn new() -> Self {
        let regex = |pattern: &str, flags: &str| RegExp::from_str(pattern, flags).expect("a regex do wordifyKey é válida");
        Self {
            dot_then_alphanumeric: regex(r"\.([a-z0-9])", "g"),
            lower_then_upper: regex(r"([a-z0-9])([A-Z])", "g"),
            consecutive_capitals: regex(r"([A-Z]{1,})([A-Z][a-z])", "g"),
            leading_lower: regex(r"^[a-z]", "g"),
            word: regex(r"\b\w+\b", "g"),
            term_mappings: KNOWN_TERM_MAPPINGS.iter().map(|(term, replacement)| (regex(&format!(r"\b{term}\b"), "gi"), *replacement)).collect(),
        }
    }
}

thread_local! {
    static WORDIFY_REGEXES: WordifyRegexes = WordifyRegexes::new();
}

/// `$1 $2` de um `String#replace`: os dois primeiros grupos separados por um espaço.
pub(crate) fn join_first_two_groups(captures: &je_base::js_regexp::Captures<'_>) -> JsString {
    let mut out = JsString::from(captures.group(1).unwrap_or_default());
    out.push_unit(u16::from(b' '));
    out.push_units(captures.group(2).unwrap_or_default());
    out
}

/// `wordifyKey`: `editor.fontFamily` vira `Editor › Font Family`.
pub fn wordify_key(key: &str) -> String {
    WORDIFY_REGEXES.with(|regexes| {
        let units: Vec<u16> = key.encode_utf16().collect();
        // Replace dot with spaced '>'
        let units = replace_all(&regexes.dot_then_alphanumeric, &units, |captures| {
            let mut out = JsString::from(" \u{203A} ");
            out.push_units(JsString::from(captures.group(1).unwrap_or_default()).to_upper_case().units());
            out
        });
        // Camel case to spacing, fooBar => foo Bar
        let units = replace_all(&regexes.lower_then_upper, units.units(), join_first_two_groups);
        // Split consecutive capitals letters, AISearch => AI Search
        let units = replace_all(&regexes.consecutive_capitals, units.units(), join_first_two_groups);
        // Upper casing all first letters, foo => Foo
        let units = replace_all(&regexes.leading_lower, units.units(), |captures| JsString::from(captures.whole()).to_upper_case());
        // Upper casing known acronyms
        let units = replace_all(&regexes.word, units.units(), |captures| {
            let word = JsString::from(captures.whole());
            if KNOWN_ACRONYMS.contains(&word.to_lower_case().to_string_lossy().as_str()) { word.to_upper_case() } else { word }
        });

        let mut result = units;
        for (term, replacement) in &regexes.term_mappings {
            result = replace_all(term, result.units(), |_| JsString::from(*replacement));
        }
        result.to_string_lossy()
    })
}
