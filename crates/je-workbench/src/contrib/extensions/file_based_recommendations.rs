//! Porte de `src/vs/workbench/contrib/extensions/browser/fileBasedRecommendations.ts`: as recomendações de extensões
//! para os arquivos que o usuário abre (por glob do caminho, por linguagem e por conteúdo), guardadas por uma semana, e o
//! aviso das importantes.
//!
//! Notas de tradução:
//! - **Injeção de dependência.** Os serviços do construtor entram agrupados em [`FileBasedRecommendationsParts`]; o
//!   `IExtensionsWorkbenchService` é o [`ExtensionsWorkbenchService`] concreto.
//! - **`IFileOpenCondition`.** O tipo do produto é um enum (condição por linguagem ou por caminho); os campos comuns
//!   (`important`, `whenInstalled`, `whenNotInstalled`, `contentPattern`) saem pelas funções `condition_*`.
//! - **`Date.now()`** é `je_base::runtime::date_now()` (o relógio virtual dentro dos testes com `with_faked_timers`).
//! - **`[...keys()].sort(...)`** de `recommendations` usa o `sort_by_compare` de `je_base::arrays`, que repete a sequência
//!   de chamadas ao comparador do V8: o comparador do original não é uma ordem total (devolve `1` para tempos iguais sem
//!   importante).

use std::cell::RefCell;
use std::rc::{Rc, Weak};

use indexmap::IndexMap;
use je_base::arrays::sort_by_compare;
use je_base::async_::{async_fn, disposable_timeout};
use je_base::lifecycle::{Disposable, DisposableStore, IDisposable};
use je_base::network::schemas;
use je_base::nls::localize;
use je_base::product::{IFileOpenCondition, IStringDictionary};
use je_base::promise::Promise;
use je_base::uri::{Field, UriChange, URI};
use je_editor::language_service::ILanguageService;
use je_editor::modes_registry::PLAINTEXT_LANGUAGE_ID;
use je_editor::services_model::IModelService;
use je_editor::text_model::{FindScope, TextModel};
use je_extensions::manifest::ExtensionIdentifierInfo;
use je_extensions::util::are_same_extensions;
use je_platform::extension_recommendations::extension_recommendations::{
    ExtensionRecommendationsRequest, IExtensionRecommendationNotificationService, RecommendationSource, RecommendationsNotificationResult, RecommendedExtension,
};
use je_platform::product::IProductService;
use je_platform::resources::ext_uri;
use je_platform::storage::{StorageScope, StorageService, StorageTarget};
use je_platform::workspace::IWorkspaceContextService;
use serde_json::Value;

use super::extension_recommendations::{Activation, ExtensionRecommendation, ExtensionRecommendations, GalleryExtensionRecommendation};
use super::extensions::IExtension;
use super::extensions_service::ExtensionsWorkbenchService;
use crate::contrib::notebook::common::notebook_common::CellUri;
use crate::services::extension_management::extension_management::EnablementState;
use crate::services::extension_recommendations::extension_recommendations::{ExtensionRecommendationReason, IExtensionIgnoredRecommendationsService, IExtensionRecommendationReason};
use crate::services::untitled::untitled_text_editor_service::IUntitledTextEditorService;

const PROMPTED_RECOMMENDATIONS_STORAGE_KEY: &str = "fileBasedRecommendations/promptedRecommendations";
const RECOMMENDATIONS_STORAGE_KEY: &str = "extensionsAssistant/recommendations";
const MILLI_SECONDS_IN_A_DAY: f64 = 1000.0 * 60.0 * 60.0 * 24.0;

/// Minimum length of untitled file to allow triggering extension recommendations for auto-detected language.
const UNTITLED_FILE_RECOMMENDATIONS_MIN_LENGTH: usize = 1000;

/// `IStringDictionary<IFileOpenCondition[]>`.
type ConditionsByExtension = IStringDictionary<Vec<IFileOpenCondition>>;

fn condition_important(condition: &IFileOpenCondition) -> bool {
    match condition {
        IFileOpenCondition::Language(condition) => condition.important.unwrap_or(false),
        IFileOpenCondition::Path(condition) => condition.important.unwrap_or(false),
    }
}

fn condition_when_installed(condition: &IFileOpenCondition) -> Option<&Vec<String>> {
    match condition {
        IFileOpenCondition::Language(condition) => condition.when_installed.as_ref(),
        IFileOpenCondition::Path(condition) => condition.when_installed.as_ref(),
    }
}

fn condition_when_not_installed(condition: &IFileOpenCondition) -> Option<&Vec<String>> {
    match condition {
        IFileOpenCondition::Language(condition) => condition.when_not_installed.as_ref(),
        IFileOpenCondition::Path(condition) => condition.when_not_installed.as_ref(),
    }
}

/// `condition.contentPattern` (a cadeia vazia é falsa, como no original).
fn condition_content_pattern(condition: &IFileOpenCondition) -> Option<&String> {
    let pattern = match condition {
        IFileOpenCondition::Language(condition) => condition.content_pattern.as_ref(),
        IFileOpenCondition::Path(condition) => condition.content_pattern.as_ref(),
    };
    pattern.filter(|pattern| !pattern.is_empty())
}

/// `(<IFileLanguageCondition>condition).languages`.
fn condition_languages(condition: &IFileOpenCondition) -> Option<&Vec<String>> {
    match condition {
        IFileOpenCondition::Language(condition) => Some(&condition.languages),
        IFileOpenCondition::Path(_) => None,
    }
}

/// `(<IFilePathCondition>condition).pathGlob` (a cadeia vazia é falsa).
fn condition_path_glob(condition: &IFileOpenCondition) -> Option<&String> {
    match condition {
        IFileOpenCondition::Path(condition) => Some(&condition.path_glob).filter(|glob| !glob.is_empty()),
        IFileOpenCondition::Language(_) => None,
    }
}

/// Os serviços que o construtor do original recebe por decorador.
pub struct FileBasedRecommendationsParts {
    pub extensions_workbench_service: Rc<ExtensionsWorkbenchService>,
    pub model_service: Rc<dyn IModelService>,
    pub language_service: Rc<dyn ILanguageService>,
    pub product_service: Rc<dyn IProductService>,
    pub storage_service: Rc<dyn StorageService>,
    pub extension_recommendation_notification_service: Rc<dyn IExtensionRecommendationNotificationService>,
    pub extension_ignored_recommendations_service: Rc<dyn IExtensionIgnoredRecommendationsService>,
    pub workspace_context_service: Rc<dyn IWorkspaceContextService>,
    pub untitled_text_editor_service: Rc<dyn IUntitledTextEditorService>,
}

/// `FileBasedRecommendations`.
pub struct FileBasedRecommendations {
    this: Weak<FileBasedRecommendations>,
    base: Rc<Disposable>,
    store: Rc<DisposableStore>,
    activation: Activation,

    file_open_recommendations: ConditionsByExtension,
    recommendations_by_pattern: RefCell<IndexMap<String, ConditionsByExtension>>,
    /// `fileBasedRecommendations`: `Map<string, { recommendedTime: number }>`.
    file_based_recommendations: RefCell<IndexMap<String, f64>>,
    file_based_important_recommendations: RefCell<Vec<String>>,

    p: FileBasedRecommendationsParts,
}

impl FileBasedRecommendations {
    /// `constructor(...)`.
    pub fn new(parts: FileBasedRecommendationsParts) -> Rc<Self> {
        let base = Disposable::new();
        let store = DisposableStore::new();
        base.register(store.clone());
        let mut file_open_recommendations = ConditionsByExtension::new();
        if let Some(extension_recommendations) = parts.product_service.extension_recommendations() {
            for (extension_id, recommendation) in extension_recommendations {
                if let Some(on_file_open) = &recommendation.on_file_open {
                    file_open_recommendations.insert(extension_id.to_lowercase(), on_file_open.clone());
                }
            }
        }
        Rc::new_cyclic(|weak| Self {
            this: weak.clone(),
            base,
            store,
            activation: Activation::default(),
            file_open_recommendations,
            recommendations_by_pattern: RefCell::new(IndexMap::new()),
            file_based_recommendations: RefCell::new(IndexMap::new()),
            file_based_important_recommendations: RefCell::new(Vec::new()),
            p: parts,
        })
    }

    fn rc(&self) -> Rc<Self> {
        self.this.upgrade().expect("o recomendador está vivo enquanto é usado")
    }

    /// `get recommendations()`.
    fn gallery_recommendations(&self) -> Vec<GalleryExtensionRecommendation> {
        let times = self.file_based_recommendations.borrow().clone();
        let important = self.file_based_important_recommendations.borrow().clone();
        let mut keys: Vec<String> = times.keys().cloned().collect();
        sort_by_compare(&mut keys, |a, b| {
            if times[a] == times[b] {
                if important.contains(a) {
                    return -1.0;
                }
                if important.contains(b) {
                    return 1.0;
                }
            }
            if times[a] > times[b] { -1.0 } else { 1.0 }
        });
        keys.into_iter()
            .map(|extension_id| GalleryExtensionRecommendation {
                extension: extension_id,
                reason: IExtensionRecommendationReason {
                    reason_id: ExtensionRecommendationReason::File,
                    reason_text: localize("fileBasedRecommendation", "This extension is recommended based on the files you recently opened.", &[]),
                },
            })
            .collect()
    }

    /// `get importantRecommendations()`.
    pub fn important_recommendations(&self) -> Vec<GalleryExtensionRecommendation> {
        let important = self.file_based_important_recommendations.borrow().clone();
        self.gallery_recommendations().into_iter().filter(|recommendation| important.contains(&recommendation.extension)).collect()
    }

    /// `get otherRecommendations()`.
    pub fn other_recommendations(&self) -> Vec<GalleryExtensionRecommendation> {
        let important = self.file_based_important_recommendations.borrow().clone();
        self.gallery_recommendations().into_iter().filter(|recommendation| !important.contains(&recommendation.extension)).collect()
    }

    /// `onModelAdded(model)`.
    fn on_model_added(&self, model: Rc<TextModel>) {
        let uri = if model.uri().scheme() == schemas::VSCODE_NOTEBOOK_CELL { CellUri::parse(&model.uri()).map(|parsed| parsed.notebook) } else { Some(model.uri()) };
        let Some(uri) = uri else { return };

        let mut supported_schemes: Vec<String> = vec![schemas::UNTITLED.to_owned(), schemas::FILE.to_owned(), schemas::VSCODE_REMOTE.to_owned()];
        for folder in self.p.workspace_context_service.get_workspace().folders() {
            let scheme = folder.uri.scheme().to_string_lossy();
            if !supported_schemes.contains(&scheme) {
                supported_schemes.push(scheme);
            }
        }
        if !supported_schemes.contains(&uri.scheme().to_string_lossy()) {
            return;
        }

        // re-schedule this bit of the operation to be off the critical path - in case glob-match is slow
        let this = self.rc();
        disposable_timeout(move || this.prompt_important_recommendations(&uri, &model, None), 0, Some(self.store.clone()));
    }

    /// Prompt the user to either install the recommended extension for the file type in the current editor model
    /// or prompt to search the marketplace if it has extensions that can support the file type.
    fn prompt_important_recommendations(&self, uri: &URI, model: &Rc<TextModel>, extension_recommendations: Option<ConditionsByExtension>) {
        if model.is_disposed() {
            return;
        }

        let pattern = ext_uri().extname(uri).to_lowercase();
        let extension_recommendations = extension_recommendations
            .or_else(|| self.recommendations_by_pattern.borrow().get(&pattern).cloned())
            .unwrap_or_else(|| self.file_open_recommendations.clone());
        if extension_recommendations.is_empty() {
            return;
        }

        let mut processed_path_globs: IndexMap<String, bool> = IndexMap::new();
        let installed = self.p.extensions_workbench_service.local();
        let mut recommendations_by_pattern = ConditionsByExtension::new();
        let mut matched_recommendations = ConditionsByExtension::new();
        let mut unmatched_recommendations = ConditionsByExtension::new();
        let mut listen_on_language_change = false;
        let language_id = model.get_language_id();

        // Allow language-specific recommendations for untitled files when language is auto-detected only when the file is large.
        let untitled_model = self.p.untitled_text_editor_service.get(uri);
        let allow_language_match =
            untitled_model.as_ref().is_none_or(|untitled_model| untitled_model.has_language_set_explicitly()) || model.get_value_length() > UNTITLED_FILE_RECOMMENDATIONS_MIN_LENGTH;

        let uri_without_fragment = uri.with(&UriChange { fragment: Field::Clear, ..UriChange::default() }).to_string(false).to_string_lossy();
        let glob_options = je_base::glob::GlobOptions { ignore_case: Some(true), ..je_base::glob::GlobOptions::default() };

        for (extension_id, conditions) in &extension_recommendations {
            let mut conditions_by_pattern: Vec<IFileOpenCondition> = Vec::new();
            let mut matched_conditions: Vec<IFileOpenCondition> = Vec::new();
            let mut unmatched_conditions: Vec<IFileOpenCondition> = Vec::new();
            for condition in conditions {
                let mut language_matched = false;
                let mut path_glob_matched = false;

                let is_language_condition = condition_languages(condition).is_some();
                let is_file_content_condition = condition_content_pattern(condition).is_some();
                if is_language_condition || is_file_content_condition {
                    conditions_by_pattern.push(condition.clone());
                }

                if is_language_condition && allow_language_match && condition_languages(condition).is_some_and(|languages| languages.contains(&language_id)) {
                    language_matched = true;
                }

                if let Some(path_glob) = condition_path_glob(condition) {
                    let processed = processed_path_globs.get(path_glob).copied();
                    if processed.unwrap_or_else(|| je_base::glob::match_pattern(path_glob.as_str(), &uri_without_fragment, &glob_options)) {
                        path_glob_matched = true;
                    }
                    processed_path_globs.insert(path_glob.clone(), path_glob_matched);
                }

                let mut matched = language_matched || path_glob_matched;

                // If the resource has pattern (extension) and not matched, then we don't need to check the other conditions.
                if !pattern.is_empty() && !matched {
                    continue;
                }

                if matched {
                    if let Some(when_installed) = condition_when_installed(condition) {
                        if !when_installed.iter().all(|id| installed.iter().any(|local| are_same_extensions(&ExtensionIdentifierInfo::new(id.clone()), &local.identifier()))) {
                            matched = false;
                        }
                    }
                }

                if matched {
                    if let Some(when_not_installed) = condition_when_not_installed(condition) {
                        if installed.iter().any(|local| when_not_installed.iter().any(|id| are_same_extensions(&ExtensionIdentifierInfo::new(id.clone()), &local.identifier()))) {
                            matched = false;
                        }
                    }
                }

                if matched && is_file_content_condition {
                    if let Some(content_pattern) = condition_content_pattern(condition) {
                        if model.find_matches(content_pattern.as_str(), FindScope::Whole, true, false, None, false, 999).is_empty() {
                            matched = false;
                        }
                    }
                }

                if matched {
                    matched_conditions.push(condition.clone());
                    conditions_by_pattern.pop();
                } else if is_language_condition || is_file_content_condition {
                    unmatched_conditions.push(condition.clone());
                    if is_language_condition {
                        listen_on_language_change = true;
                    }
                }
            }
            if !matched_conditions.is_empty() {
                matched_recommendations.insert(extension_id.clone(), matched_conditions);
            }
            if !unmatched_conditions.is_empty() {
                unmatched_recommendations.insert(extension_id.clone(), unmatched_conditions);
            }
            if !conditions_by_pattern.is_empty() {
                recommendations_by_pattern.insert(extension_id.clone(), conditions_by_pattern);
            }
        }

        if !pattern.is_empty() {
            self.recommendations_by_pattern.borrow_mut().insert(pattern, recommendations_by_pattern);
        }
        if !unmatched_recommendations.is_empty() && listen_on_language_change {
            let disposables = DisposableStore::new();
            let weak = self.this.clone();
            let (uri, model_for_language, unmatched) = (uri.clone(), model.clone(), unmatched_recommendations);
            let language_disposables = disposables.clone();
            disposables.add(model.on_did_change_language().listen(move |_| {
                // re-schedule this bit of the operation to be off the critical path - in case glob-match is slow
                let (weak, uri, model, unmatched, disposables) = (weak.clone(), uri.clone(), model_for_language.clone(), unmatched.clone(), language_disposables.clone());
                let inner = language_disposables.clone();
                // `disposableTimeout(..., 0, disposables)` já se registra no `disposables`.
                disposable_timeout(
                    move || {
                        if !disposables.is_disposed() {
                            if let Some(this) = weak.upgrade() {
                                this.prompt_important_recommendations(&uri, &model, Some(unmatched));
                            }
                            disposables.dispose();
                        }
                    },
                    0,
                    Some(inner),
                );
            }));
            let will_dispose = disposables.clone();
            disposables.add(model.on_will_dispose().listen(move |_| will_dispose.dispose()));
        }

        if !matched_recommendations.is_empty() {
            self.prompt_from_recommendations(uri, model, matched_recommendations);
        }
    }

    /// `promptFromRecommendations(uri, model, extensionRecommendations)`.
    fn prompt_from_recommendations(&self, uri: &URI, model: &Rc<TextModel>, extension_recommendations: ConditionsByExtension) {
        let mut is_important_recommendation_for_language = false;
        let mut important_recommendations: Vec<String> = Vec::new();
        let mut file_based_recommendations: Vec<String> = Vec::new();
        for (extension_id, conditions) in &extension_recommendations {
            for condition in conditions {
                if !file_based_recommendations.contains(extension_id) {
                    file_based_recommendations.push(extension_id.clone());
                }
                if condition_important(condition) {
                    if !important_recommendations.contains(extension_id) {
                        important_recommendations.push(extension_id.clone());
                    }
                    let mut important = self.file_based_important_recommendations.borrow_mut();
                    if !important.contains(extension_id) {
                        important.push(extension_id.clone());
                    }
                }
                if condition_languages(condition).is_some() {
                    is_important_recommendation_for_language = true;
                }
            }
        }

        // Update file based recommendations
        for recommendation in &file_based_recommendations {
            self.file_based_recommendations.borrow_mut().insert(recommendation.clone(), je_base::runtime::date_now() as f64);
        }

        self.store_cached_recommendations();

        if self.p.extension_recommendation_notification_service.has_to_ignore_recommendation_notifications() {
            return;
        }

        let language = model.get_language_id();
        let language_name = self.p.language_service.get_language_name(&language);
        let name = match language_name {
            Some(language_name) if !language_name.is_empty() && is_important_recommendation_for_language && language != PLAINTEXT_LANGUAGE_ID => {
                localize("languageName", "the {0} language", &[&language_name])
            }
            _ => ext_uri().basename(uri, None),
        };
        if !important_recommendations.is_empty() && self.prompt_recommended_extension_for_file_type(name, language, important_recommendations) {
            return;
        }
    }

    /// `promptRecommendedExtensionForFileType(name, language, recommendations)`.
    fn prompt_recommended_extension_for_file_type(&self, name: String, language: String, recommendations: Vec<String>) -> bool {
        let recommendations = self.filter_ignored_or_not_allowed(recommendations);
        if recommendations.is_empty() {
            return false;
        }

        let important = self.file_based_important_recommendations.borrow().clone();
        let mut recommendations: Vec<String> =
            self.filter_installed(recommendations, &self.p.extensions_workbench_service.local()).into_iter().filter(|extension_id| important.contains(extension_id)).collect();

        let prompted_recommendations = if language != PLAINTEXT_LANGUAGE_ID { self.get_prompted_recommendations().get(&language).cloned() } else { None };
        if let Some(prompted_recommendations) = prompted_recommendations {
            recommendations.retain(|extension_id| !prompted_recommendations.contains(extension_id));
        }

        if recommendations.is_empty() {
            return false;
        }

        let _ = self.prompt_important_extensions_install_notification(recommendations, name, language);
        true
    }

    /// `promptImportantExtensionsInstallNotification(extensions, name, language)`.
    fn prompt_important_extensions_install_notification(&self, extensions: Vec<String>, name: String, language: String) -> Promise<()> {
        let this = self.rc();
        async_fn(async move {
            // `try { ... } catch (error) { /* Ignore */ }`
            let result = this
                .p
                .extension_recommendation_notification_service
                .prompt_important_extensions_install_notification(ExtensionRecommendationsRequest {
                    extensions: extensions.iter().cloned().map(RecommendedExtension::Gallery).collect(),
                    name,
                    source: RecommendationSource::File,
                    search_value: None,
                })
                .await;
            if let Ok(RecommendationsNotificationResult::Accepted) = result {
                this.add_to_prompted_recommendations(&language, &extensions);
            }
            Ok(())
        })
    }

    /// `getPromptedRecommendations()`.
    fn get_prompted_recommendations(&self) -> IndexMap<String, Vec<String>> {
        let stored = self.p.storage_service.get(PROMPTED_RECOMMENDATIONS_STORAGE_KEY, StorageScope::Profile, Some("{}")).unwrap_or_else(|| "{}".to_owned());
        serde_json::from_str(&stored).unwrap_or_default()
    }

    /// `addToPromptedRecommendations(language, extensions)`.
    fn add_to_prompted_recommendations(&self, language: &str, extensions: &[String]) {
        let mut prompted_recommendations = self.get_prompted_recommendations();
        let mut merged: Vec<String> = prompted_recommendations.get(language).cloned().unwrap_or_default();
        for extension in extensions {
            if !merged.contains(extension) {
                merged.push(extension.clone());
            }
        }
        prompted_recommendations.insert(language.to_owned(), merged);
        let serialized = serde_json::to_string(&prompted_recommendations).unwrap_or_else(|_| "{}".to_owned());
        self.p.storage_service.store(PROMPTED_RECOMMENDATIONS_STORAGE_KEY, serialized.as_str().into(), StorageScope::Profile, StorageTarget::User);
    }

    /// `filterIgnoredOrNotAllowed(recommendationsToSuggest)`.
    fn filter_ignored_or_not_allowed(&self, recommendations_to_suggest: Vec<String>) -> Vec<String> {
        let mut ignored_recommendations = self.p.extension_ignored_recommendations_service.ignored_recommendations();
        ignored_recommendations.extend(self.p.extension_recommendation_notification_service.ignored_recommendations());
        recommendations_to_suggest.into_iter().filter(|id| !ignored_recommendations.contains(id)).collect()
    }

    /// `filterInstalled(recommendationsToSuggest, installed)`.
    fn filter_installed(&self, recommendations_to_suggest: Vec<String>, installed: &[Rc<super::extensions_workbench_service::Extension>]) -> Vec<String> {
        let installed_extensions_ids: Vec<String> =
            installed.iter().filter(|i| i.enablement_state() != EnablementState::DisabledByExtensionKind).map(|i| i.identifier().id.to_lowercase()).collect();
        recommendations_to_suggest.into_iter().filter(|id| !installed_extensions_ids.contains(&id.to_lowercase())).collect()
    }

    /// `getCachedRecommendations()`.
    fn get_cached_recommendations(&self) -> IndexMap<String, f64> {
        let stored = self.p.storage_service.get(RECOMMENDATIONS_STORAGE_KEY, StorageScope::Profile, Some("[]")).unwrap_or_else(|| "[]".to_owned());
        let parsed: Value = serde_json::from_str(&stored).unwrap_or(Value::Array(Vec::new()));
        let mut result: IndexMap<String, f64> = IndexMap::new();
        match parsed {
            // Formato antigo: a lista de ids, todos com o instante de agora.
            Value::Array(ids) => {
                for id in ids.iter().filter_map(Value::as_str) {
                    result.insert(id.to_lowercase(), je_base::runtime::date_now() as f64);
                }
            }
            Value::Object(entries) => {
                for (key, value) in entries {
                    if let Some(time) = value.as_f64() {
                        result.insert(key.to_lowercase(), time);
                    }
                }
            }
            _ => {}
        }
        result
    }

    /// `storeCachedRecommendations()`.
    fn store_cached_recommendations(&self) {
        let stored: serde_json::Map<String, Value> = self
            .file_based_recommendations
            .borrow()
            .iter()
            .filter_map(|(key, time)| serde_json::Number::from_f64(*time).map(|number| (key.clone(), Value::Number(number))))
            .collect();
        let serialized = serde_json::to_string(&Value::Object(stored)).unwrap_or_else(|_| "{}".to_owned());
        self.p.storage_service.store(RECOMMENDATIONS_STORAGE_KEY, serialized.as_str().into(), StorageScope::Profile, StorageTarget::Machine);
    }
}

impl ExtensionRecommendations for FileBasedRecommendations {
    fn recommendations(&self) -> Vec<ExtensionRecommendation> {
        self.gallery_recommendations().into_iter().map(ExtensionRecommendation::from).collect()
    }

    fn activation(&self) -> &Activation {
        &self.activation
    }

    fn do_activate(&self) -> Promise<()> {
        let this = self.rc();
        async_fn(async move {
            if this.file_open_recommendations.is_empty() {
                return Ok(());
            }

            let when_initialized = this.p.extensions_workbench_service.when_initialized.borrow().clone();
            if let Some(when_initialized) = when_initialized {
                when_initialized.await?;
            }

            let cached_recommendations = this.get_cached_recommendations();
            let now = je_base::runtime::date_now() as f64;
            // Retire existing recommendations if they are older than a week or are not part of this.productService.extensionTips anymore.
            for (key, value) in &cached_recommendations {
                let diff = (now - value) / MILLI_SECONDS_IN_A_DAY;
                if diff <= 7.0 && this.file_open_recommendations.contains_key(key) {
                    this.file_based_recommendations.borrow_mut().insert(key.to_lowercase(), *value);
                }
            }

            let weak = Rc::downgrade(&this);
            this.base.register(this.p.model_service.on_model_added().listen(move |model| {
                if let Some(this) = weak.upgrade() {
                    this.on_model_added(model.clone());
                }
            }));
            for model in this.p.model_service.get_models() {
                this.on_model_added(model);
            }
            Ok(())
        })
    }
}

impl IDisposable for FileBasedRecommendations {
    fn dispose(&self) {
        self.base.dispose();
    }
}
