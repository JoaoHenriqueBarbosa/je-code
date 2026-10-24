//! Porte de `src/vs/workbench/contrib/snippets/browser/snippetsService.ts`: o serviço que descobre, carrega e vigia os
//! arquivos de snippets (do usuário, do workspace e das extensões), guarda o que o usuário desligou e o uso recente, e
//! devolve os snippets de uma linguagem filtrados e ordenados.
//!
//! Notas de tradução:
//! - Os serviços injetados (`@IEnvironmentService`...) são os campos de [`SnippetsServices`]; `SnippetEnablement` e
//!   `SnippetUsageTimestamps` (que o original cria por `createInstance`) recebem só o `IStorageService`.
//! - `this` capturado por ouvintes e por `async` é `Rc<Self>` (um `Weak` guardado em [`SnippetsService::me`], para o
//!   ouvinte guardado dentro do próprio serviço não formar ciclo): os ouvintes sobem o `Weak` e não fazem nada se o
//!   serviço já foi descartado.
//! - As funções `async` são [`async_fn`], que roda o corpo até o primeiro `await`; o que o original deixa sem `await`
//!   (`addFolderSnippets()` dentro de um ouvinte) é uma promessa descartada.
//! - `Set<Promise<void>>` (`_pendingWork`) é um vetor com a identidade da promessa ([`Promise::ptr_eq`]); `Map<string, number>`
//!   (`_usages`) e `Set<string>` (`_ignored`) guardam a ordem de inserção ([`OrderedMap`]), porque o `JSON.stringify`
//!   dela vai para o armazenamento. `ResourceMap<SnippetFile>` é o `ResourceMap` de `je_base::map` com a chave padrão.
//! - `JSON.parse` é o `je_base::json::parse_str`; erro de sintaxe é o `catch` do original.
//! - `file.defaultScopes.push(...)` mexe no array que os snippets já carregados compartilham ([`SnippetScopes`]).
//! - O corpo de `setHandler` do ponto de extensão é [`SnippetsService::set_extension_snippets`]: guarda as extensões
//!   (como o registro entrega a lista atual quando o tratador é instalado) e as aplica depois de `Restored`.
//!
//! Divergências:
//! - `IFileService` do porte é reduzido (`file_service.rs` o descreve): o serviço usa o [`FileService`] concreto (que tem
//!   `watch`, `createFolder` e `resolve`) e entrega ao `SnippetFile` o contrato [`IFileService`] dele.
//! - `ExtensionsRegistry.registerExtensionPoint({ extensionPoint: 'snippets', deps, jsonSchema })`: o registro do porte
//!   não tem `deps` nem esquema JSON (ver `extensions_registry.rs`); [`snippet_ext::point`] registra o ponto com o nome, e
//!   o `snippetsContribution` (a descrição do esquema) não tem onde ir. O `IExtensionPointUser` do porte não traz o
//!   `description` da extensão, que `toValidSnippet` e o `SnippetFile` leem, então o usuário do ponto é o
//!   [`SnippetsExtensionUser`].
//! - `ISimpleModel` é o contrato de uma operação (`getLineContent`) deste arquivo; os `ISimpleModel` do je-editor
//!   (leitor de tela) exigem outras.
//! - `SnippetUsageTimestamps` descarta do JSON as entradas que não são `[texto, número]` (o `new Map(data)` do original
//!   lançaria no construtor).

use std::cell::{Cell, RefCell};
use std::cmp::Ordering;
use std::rc::{Rc, Weak};

use je_base::JsString;
use je_base::async_::async_fn;
use je_base::dynamic::json_quote;
use je_base::js_string::is_js_whitespace;
use je_base::json::{Value, parse_str};
use je_base::lifecycle::{DisposableRef, DisposableStore, IDisposable, combined_disposable, plain_disposable};
use je_base::map::{OrderedMap, ResourceMap};
use je_base::nls::localize;
use je_base::promise::Promise;
use je_base::runtime;
use je_base::strings::is_falsy_or_whitespace;
use je_base::uri::URI;
use je_editor::cursor_common::CursorSimpleModel;
use je_editor::language_configuration_registry::ILanguageConfigurationService;
use je_editor::language_service::ILanguageService;
use je_editor::position::Position;
use je_editor::suggest::suggest::set_snippet_suggest_support;
use je_editor::text_model::TextModel;
use je_extensions::scanner::ExtensionDescription;
use je_platform::environment::environment::EnvironmentService;
use je_platform::files::file_service::FileService;
use je_platform::files::files::FileChangeType;
use je_platform::files::i_file_service::IFileService;
use je_platform::log::{LogArg, LogMessage, LogServiceRef, Logger};
use je_platform::resources::{ext_uri, uri_file};
use je_platform::storage::storage::{StorageScope, StorageService, StorageTarget};
use je_platform::workspace::{IWorkspaceContextService, Workspace};
use je_storage::StorageValue;

use super::snippet_completion_provider::SnippetCompletionProvider;
use super::snippets::{ISnippetGetOptions, ISnippetsService};
use super::snippets_file::{IExtensionResourceLoaderService, Snippet, SnippetFile, SnippetScopes, SnippetSource};
use crate::services::extensions::extensions_registry::{
    ExtensionMessageCollector, ExtensionPoint, ExtensionPointDescriptor, extensions_registry,
};
use crate::services::lifecycle::lifecycle::{ILifecycleService, LifecyclePhase};
use crate::services::textfile::textfiles::ITextFileService;
use crate::services::user_data_profile::user_data_profile::{DidChangeUserDataProfileEvent, IUserDataProfileService};
use crate::services::working_copy::working_copy::IWorkingCopy;

fn units(text: &str) -> Vec<u16> {
    text.encode_utf16().collect()
}

/// `JSON.parse(raw)`: `None` no lugar da exceção de sintaxe.
fn parse_json(raw: &str) -> Option<Value> {
    let mut errors = Vec::new();
    parse_str(raw, &mut errors).filter(|_| errors.is_empty())
}

pub mod snippet_ext {
    use super::*;

    /// `ISnippetsExtensionPoint`: o que uma extensão contribui em `contributes.snippets`.
    #[derive(Clone, Debug, PartialEq, Eq)]
    pub struct SnippetsExtensionPoint {
        pub language: Option<String>,
        pub path: Option<String>,
    }

    /// `IValidSnippetsExtensionPoint`.
    #[derive(Clone, Debug)]
    pub struct ValidSnippetsExtensionPoint {
        pub language: String,
        pub location: URI,
    }

    /// `IExtensionPointUser<ISnippetsExtensionPoint[]>` (ver as divergências do módulo).
    #[derive(Clone)]
    pub struct SnippetsExtensionUser {
        pub description: Rc<ExtensionDescription>,
        pub value: Vec<SnippetsExtensionPoint>,
        pub collector: Rc<dyn ExtensionMessageCollector>,
    }

    /// `String(value)` de um texto que pode faltar.
    fn text_of(value: &Option<String>) -> &str {
        value.as_deref().unwrap_or("undefined")
    }

    pub fn to_valid_snippet(
        extension: &SnippetsExtensionUser,
        snippet: &SnippetsExtensionPoint,
        language_service: &dyn ILanguageService,
    ) -> Option<ValidSnippetsExtensionPoint> {
        let name = extension.description.manifest.name.as_str();
        let path_units = snippet.path.as_deref().map(units);
        let language_units = snippet.language.as_deref().map(units);

        if is_falsy_or_whitespace(path_units.as_deref()) {
            extension.collector.error(&localize(
                "invalid.path.0",
                "Expected string in `contributes.{0}.path`. Provided value: {1}",
                &[name, text_of(&snippet.path)],
            ));
            return None;
        }
        let path = snippet.path.as_deref().unwrap_or_default();

        if is_falsy_or_whitespace(language_units.as_deref()) && !path.ends_with(".code-snippets") {
            extension.collector.error(&localize(
                "invalid.language.0",
                "When omitting the language, the value of `contributes.{0}.path` must be a `.code-snippets`-file. Provided value: {1}",
                &[name, text_of(&snippet.path)],
            ));
            return None;
        }

        if !is_falsy_or_whitespace(language_units.as_deref()) && !language_service.is_registered_language_id(snippet.language.as_deref()) {
            extension.collector.error(&localize(
                "invalid.language",
                "Unknown language in `contributes.{0}.language`. Provided value: {1}",
                &[name, text_of(&snippet.language)],
            ));
            return None;
        }

        let extension_location = uri_file(&extension.description.extension_location);
        let snippet_location = ext_uri().join_path(&extension_location, &[path]);
        if !ext_uri().is_equal_or_parent(&snippet_location, &extension_location, false, false) {
            let snippet_path = snippet_location.path().to_string_lossy();
            let extension_path = extension_location.path().to_string_lossy();
            extension.collector.error(&localize(
                "invalid.path.1",
                "Expected `contributes.{0}.path` ({1}) to be included inside extension's folder ({2}). This might make the extension non-portable.",
                &[name, snippet_path.as_str(), extension_path.as_str()],
            ));
            return None;
        }

        Some(ValidSnippetsExtensionPoint { language: snippet.language.clone().unwrap_or_default(), location: snippet_location })
    }

    /// `ExtensionsRegistry.registerExtensionPoint({ extensionPoint: 'snippets', ... })`: o ponto fica registrado uma
    /// vez só, e as chamadas seguintes devolvem o mesmo.
    pub fn point() -> Rc<ExtensionPoint> {
        let registry = extensions_registry();
        if let Some(existing) = registry.get_extension_points().into_iter().find(|point| point.name == "snippets") {
            return existing;
        }
        registry.register_extension_point(ExtensionPointDescriptor {
            extension_point: "snippets".to_owned(),
            default_extension_kind: None,
            can_handle_resolver: None,
        })
    }
}

use snippet_ext::SnippetsExtensionUser;

/// `watch(service, resource, callback)`.
fn watch(service: &Rc<FileService>, resource: &URI, callback: Rc<dyn Fn()>) -> DisposableRef {
    let resource = resource.clone();
    combined_disposable(vec![
        service.watch(&resource, None),
        service.on_did_files_change().listen(move |event| {
            if event.affects(&resource, &[]) {
                callback();
            }
        }),
    ])
}

struct SnippetEnablement {
    storage_service: Rc<dyn StorageService>,
    ignored: RefCell<OrderedMap<String, ()>>,
}

impl SnippetEnablement {
    const KEY: &'static str = "snippets.ignoredSnippets";

    fn new(storage_service: Rc<dyn StorageService>) -> Self {
        let raw = storage_service.get(Self::KEY, StorageScope::Profile, Some("")).unwrap_or_default();
        let mut ignored = OrderedMap::new();
        if let Some(Value::Array(items)) = parse_json(&raw) {
            // `isStringArray(data) ? new Set(data) : new Set()`
            if items.iter().all(|item| matches!(item, Value::String(_))) {
                for item in &items {
                    if let Value::String(id) = item {
                        ignored.insert(id.to_string_lossy(), ());
                    }
                }
            }
        }
        Self { storage_service, ignored: RefCell::new(ignored) }
    }

    fn is_ignored(&self, id: &str) -> bool {
        self.ignored.borrow().contains_key(&id.to_owned())
    }

    fn update_ignored(&self, id: &str, value: bool) {
        let key = id.to_owned();
        let mut changed = false;
        {
            let mut ignored = self.ignored.borrow_mut();
            if ignored.contains_key(&key) && !value {
                ignored.remove(&key);
                changed = true;
            } else if !ignored.contains_key(&key) && value {
                ignored.insert(key, ());
                changed = true;
            }
        }
        if changed {
            let ids: Vec<String> = self.ignored.borrow().keys().map(|id| json_quote(&JsString::from(id.as_str()))).collect();
            self.storage_service.store(Self::KEY, StorageValue::String(format!("[{}]", ids.join(","))), StorageScope::Profile, StorageTarget::User);
        }
    }
}

struct SnippetUsageTimestamps {
    storage_service: Rc<dyn StorageService>,
    usages: RefCell<OrderedMap<String, f64>>,
}

impl SnippetUsageTimestamps {
    const KEY: &'static str = "snippets.usageTimestamps";

    fn new(storage_service: Rc<dyn StorageService>) -> Self {
        let raw = storage_service.get(Self::KEY, StorageScope::Profile, Some("")).unwrap_or_default();
        let mut usages = OrderedMap::new();
        if let Some(Value::Array(entries)) = parse_json(&raw) {
            for entry in &entries {
                if let Value::Array(pair) = entry
                    && let (Some(Value::String(id)), Some(Value::Number(timestamp))) = (pair.first(), pair.get(1))
                {
                    usages.insert(id.to_string_lossy(), *timestamp);
                }
            }
        }
        Self { storage_service, usages: RefCell::new(usages) }
    }

    fn get_usage_timestamp(&self, id: &str) -> Option<f64> {
        self.usages.borrow().get(&id.to_owned()).copied()
    }

    fn update_usage_timestamp(&self, id: &str) {
        // map uses insertion order, we want most recent at the end
        let key = id.to_owned();
        let mut usages = self.usages.borrow_mut();
        usages.remove(&key);
        usages.insert(key, runtime::date_now() as f64);

        // persist last 100 item
        let all: Vec<(&String, &f64)> = usages.iter().collect();
        let start = all.len().saturating_sub(100);
        let entries: Vec<String> = all[start..].iter().map(|(id, timestamp)| format!("[{},{timestamp}]", json_quote(&JsString::from(id.as_str())))).collect();
        self.storage_service.store(Self::KEY, StorageValue::String(format!("[{}]", entries.join(","))), StorageScope::Profile, StorageTarget::User);
    }
}

/// Os serviços que o `SnippetsService` recebe por injeção.
pub struct SnippetsServices {
    pub environment_service: Rc<dyn EnvironmentService>,
    pub user_data_profile_service: Rc<dyn IUserDataProfileService>,
    pub context_service: Rc<dyn IWorkspaceContextService>,
    pub language_service: Rc<dyn ILanguageService>,
    pub log_service: LogServiceRef,
    pub file_service: Rc<FileService>,
    pub textfile_service: Rc<dyn ITextFileService>,
    pub extension_resource_loader_service: Rc<dyn IExtensionResourceLoaderService>,
    pub lifecycle_service: Rc<dyn ILifecycleService>,
    pub storage_service: Rc<dyn StorageService>,
    pub language_configuration_service: Rc<dyn ILanguageConfigurationService>,
}

pub struct SnippetsService {
    me: Weak<SnippetsService>,
    disposables: Rc<DisposableStore>,
    pending_work: RefCell<Vec<Promise<()>>>,
    files: RefCell<ResourceMap<Rc<SnippetFile>>>,
    enablement: SnippetEnablement,
    usage_timestamps: SnippetUsageTimestamps,
    extension_users: RefCell<Option<Vec<SnippetsExtensionUser>>>,
    extension_snippets_ready: Cell<bool>,

    environment_service: Rc<dyn EnvironmentService>,
    user_data_profile_service: Rc<dyn IUserDataProfileService>,
    context_service: Rc<dyn IWorkspaceContextService>,
    language_service: Rc<dyn ILanguageService>,
    log_service: LogServiceRef,
    file_service: Rc<FileService>,
    file_service_contract: Rc<dyn IFileService>,
    textfile_service: Rc<dyn ITextFileService>,
    extension_resource_loader_service: Rc<dyn IExtensionResourceLoaderService>,
}

impl SnippetsService {
    pub fn new(services: SnippetsServices) -> Rc<Self> {
        let SnippetsServices {
            environment_service,
            user_data_profile_service,
            context_service,
            language_service,
            log_service,
            file_service,
            textfile_service,
            extension_resource_loader_service,
            lifecycle_service,
            storage_service,
            language_configuration_service,
        } = services;

        let file_service_contract: Rc<dyn IFileService> = Rc::new(Rc::clone(&file_service));
        let service = Rc::new_cyclic(|me| SnippetsService {
            me: me.clone(),
            disposables: DisposableStore::new(),
            pending_work: RefCell::new(Vec::new()),
            files: RefCell::new(ResourceMap::new()),
            enablement: SnippetEnablement::new(Rc::clone(&storage_service)),
            usage_timestamps: SnippetUsageTimestamps::new(storage_service),
            extension_users: RefCell::new(None),
            extension_snippets_ready: Cell::new(false),
            environment_service,
            user_data_profile_service,
            context_service,
            language_service: Rc::clone(&language_service),
            log_service,
            file_service,
            file_service_contract,
            textfile_service,
            extension_resource_loader_service,
        });

        let this = Rc::clone(&service);
        service.track_pending_work(lifecycle_service.when(LifecyclePhase::Restored).then(move |()| {
            this.init_extension_snippets();
            // o original não espera por esta promessa dentro do `then`
            drop(this.init_user_snippets());
            this.init_workspace_snippets();
        }));

        let snippets: Rc<dyn ISnippetsService> = Rc::clone(&service) as Rc<dyn ISnippetsService>;
        set_snippet_suggest_support(Some(Rc::new(SnippetCompletionProvider::new(language_service, snippets, language_configuration_service))));

        service
    }

    fn this(&self) -> Rc<Self> {
        self.me.upgrade().expect("o serviço de snippets está vivo enquanto um método dele roda")
    }

    fn join_snippets(&self) -> Promise<()> {
        let promises = self.pending_work.borrow().clone();
        Promise::all(promises).then(|_| ())
    }

    fn track_pending_work(&self, work: Promise<()>) {
        self.pending_work.borrow_mut().push(work.clone());
        let me = self.me.clone();
        let tracked = work.clone();
        work.then_result(move |result| {
            if let Some(this) = me.upgrade() {
                this.pending_work.borrow_mut().retain(|candidate| !candidate.ptr_eq(&tracked));
                if let Err(error) = result {
                    this.log_service.error_args(&LogMessage::from(error), &[]);
                }
            }
            Ok(())
        });
    }

    fn filter_and_sort_snippets(&self, snippets: Vec<Rc<Snippet>>, resource_uri: Option<&URI>, opts: Option<&ISnippetGetOptions>) -> Vec<Rc<Snippet>> {
        let include_no_prefix_snippets = opts.and_then(|opts| opts.include_no_prefix_snippets).unwrap_or(false);
        let include_disabled_snippets = opts.and_then(|opts| opts.include_disabled_snippets).unwrap_or(false);
        let file_template_snippets = opts.and_then(|opts| opts.file_template_snippets);
        let no_recency_sort = opts.and_then(|opts| opts.no_recency_sort).unwrap_or(false);

        let mut result: Vec<Rc<Snippet>> = Vec::new();

        for snippet in snippets {
            if snippet.prefix.is_empty() && !include_no_prefix_snippets {
                // prefix or no-prefix wanted
                continue;
            }
            if !self.is_enabled(&snippet) && !include_disabled_snippets {
                // enabled or disabled wanted
                continue;
            }
            if file_template_snippets.is_some_and(|wanted| wanted != snippet.is_file_template) {
                // isTopLevel requested but mismatching
                continue;
            }
            if resource_uri.is_some_and(|uri| !snippet.is_file_included(uri)) {
                // include/exclude settings don't match
                continue;
            }
            result.push(snippet);
        }

        result.sort_by(|a, b| {
            let mut ordering = Ordering::Equal;
            if !no_recency_sort {
                let val1 = self.usage_timestamps.get_usage_timestamp(&a.snippet_identifier).unwrap_or(-1.0);
                let val2 = self.usage_timestamps.get_usage_timestamp(&b.snippet_identifier).unwrap_or(-1.0);
                ordering = (val2 - val1).partial_cmp(&0.0).unwrap_or(Ordering::Equal);
            }
            if ordering == Ordering::Equal {
                ordering = Self::compare_snippet(a, b);
            }
            ordering
        });
        result
    }

    fn compare_snippet(a: &Snippet, b: &Snippet) -> Ordering {
        a.snippet_source
            .cmp(&b.snippet_source)
            .then_with(|| a.source.encode_utf16().cmp(b.source.encode_utf16()))
            .then_with(|| a.name.encode_utf16().cmp(b.name.encode_utf16()))
    }

    // --- loading, watching

    fn init_extension_snippets(&self) {
        self.extension_snippets_ready.set(true);
        let extensions = self.extension_users.borrow().clone();
        if let Some(extensions) = extensions {
            self.apply_extension_snippets(&extensions);
        }
    }

    /// O corpo do tratador do ponto de extensão `snippets` (`snippetExt.point.setHandler(extensions => ...)`): `extensions`
    /// é a lista atual das extensões que contribuem snippets.
    pub fn set_extension_snippets(&self, extensions: Vec<SnippetsExtensionUser>) {
        *self.extension_users.borrow_mut() = Some(extensions.clone());
        if self.extension_snippets_ready.get() {
            self.apply_extension_snippets(&extensions);
        }
    }

    fn apply_extension_snippets(&self, extensions: &[SnippetsExtensionUser]) {
        let stale: Vec<URI> =
            self.files.borrow().entries().filter(|(_, file)| file.source == SnippetSource::Extension).map(|(key, _)| key.clone()).collect();
        for key in stale {
            self.files.borrow_mut().delete(&key);
        }

        for extension in extensions {
            for contribution in &extension.value {
                let Some(valid_contribution) = snippet_ext::to_valid_snippet(extension, contribution, self.language_service.as_ref()) else {
                    continue;
                };

                let existing = self.files.borrow().get(&valid_contribution.location).cloned();
                if let Some(file) = existing {
                    match file.default_scopes() {
                        Some(scopes) => scopes.push(valid_contribution.language.clone()),
                        None => file.set_default_scopes(Some(SnippetScopes::new(Vec::new()))),
                    }
                } else {
                    let default_scopes = (!valid_contribution.language.is_empty()).then(|| SnippetScopes::new(vec![valid_contribution.language.clone()]));
                    let file = SnippetFile::new(
                        SnippetSource::Extension,
                        valid_contribution.location.clone(),
                        default_scopes,
                        Some(Rc::clone(&extension.description)),
                        Rc::clone(&self.file_service_contract),
                        Rc::clone(&self.extension_resource_loader_service),
                    );
                    self.files.borrow_mut().set(file.location.clone(), Rc::clone(&file));

                    if self.environment_service.is_extension_development() {
                        let collector = Rc::clone(&extension.collector);
                        let name = extension.description.manifest.name.clone();
                        let location = file.location.to_string(false).to_string_lossy();
                        file.load().then_result(move |result| {
                            match result {
                                Ok(file) => {
                                    // warn about bad tabstop/variable usage
                                    if file.data.borrow().iter().any(|snippet| snippet.is_bogous()) {
                                        collector.warn(&localize(
                                            "badVariableUse",
                                            "One or more snippets from the extension '{0}' very likely confuse snippet-variables and snippet-placeholders (see https://code.visualstudio.com/docs/editor/userdefinedsnippets#_snippet-syntax for more details)",
                                            &[name.as_str()],
                                        ));
                                    }
                                }
                                Err(_) => {
                                    // generic error
                                    collector.warn(&localize("badFile", "The snippet file \"{0}\" could not be read.", &[location.as_str()]));
                                }
                            }
                            Ok(())
                        });
                    }
                }
            }
        }
    }

    fn init_workspace_snippets(&self) {
        // workspace stuff
        let disposables = DisposableStore::new();
        let update_workspace_snippets: Rc<dyn Fn()> = {
            let me = self.me.clone();
            let disposables = Rc::clone(&disposables);
            Rc::new(move || {
                let Some(this) = me.upgrade() else {
                    return;
                };
                disposables.clear();
                let work = this.init_workspace_folder_snippets(&this.context_service.get_workspace(), &disposables);
                this.track_pending_work(work);
            })
        };
        self.disposables.add_rc(disposables);
        let on_folders = Rc::clone(&update_workspace_snippets);
        self.disposables.add(self.context_service.on_did_change_workspace_folders().listen(move |_| on_folders()));
        let on_state = Rc::clone(&update_workspace_snippets);
        self.disposables.add(self.context_service.on_did_change_workbench_state().listen(move |_| on_state()));
        update_workspace_snippets();
    }

    fn init_workspace_folder_snippets(&self, workspace: &Workspace, bucket: &Rc<DisposableStore>) -> Promise<()> {
        let promises: Vec<Promise<()>> = workspace
            .folders()
            .into_iter()
            .map(|folder| {
                let this = self.this();
                let bucket = Rc::clone(bucket);
                async_fn(async move {
                    let snippet_folder = folder.to_resource(".vscode");
                    let value = this.file_service.exists(&snippet_folder).await?;
                    if value {
                        drop(this.init_folder_snippets(SnippetSource::Workspace, &snippet_folder, &bucket));
                    } else {
                        // watch
                        let me = this.me.clone();
                        let folder_to_watch = snippet_folder.clone();
                        let listener_bucket = Rc::clone(&bucket);
                        bucket.add(this.file_service.on_did_files_change().listen(move |event| {
                            if event.contains(&folder_to_watch, &[FileChangeType::Added])
                                && let Some(this) = me.upgrade()
                            {
                                drop(this.init_folder_snippets(SnippetSource::Workspace, &folder_to_watch, &listener_bucket));
                            }
                        }));
                    }
                    Ok(())
                })
            })
            .collect();
        async_fn(async move {
            Promise::all(promises).await?;
            Ok(())
        })
    }

    fn init_user_snippets(&self) -> Promise<()> {
        let disposables = DisposableStore::new();
        let update_user_snippets: Rc<dyn Fn() -> Promise<()>> = {
            let me = self.me.clone();
            let disposables = Rc::clone(&disposables);
            Rc::new(move || {
                let Some(this) = me.upgrade() else {
                    return Promise::resolved(());
                };
                let disposables = Rc::clone(&disposables);
                async_fn(async move {
                    disposables.clear();
                    let user_snippets_folder = this.user_data_profile_service.current_profile().snippets_home;
                    this.file_service.create_folder(&user_snippets_folder).await?;
                    this.init_folder_snippets(SnippetSource::User, &user_snippets_folder, &disposables).await?;
                    Ok(())
                })
            })
        };
        self.disposables.add_rc(disposables);
        let on_change = Rc::clone(&update_user_snippets);
        let me = self.me.clone();
        self.disposables.add(self.user_data_profile_service.on_did_change_current_profile().listen(move |event: &DidChangeUserDataProfileEvent| {
            let me = me.clone();
            let update = Rc::clone(&on_change);
            event.join(async_fn(async move {
                if let Some(this) = me.upgrade() {
                    this.track_pending_work(update());
                }
                Ok(())
            }));
        }));
        update_user_snippets()
    }

    fn init_folder_snippets(&self, source: SnippetSource, folder: &URI, bucket: &Rc<DisposableStore>) -> Promise<()> {
        let disposables = DisposableStore::new();
        let add_folder_snippets: Rc<dyn Fn() -> Promise<()>> = {
            let me = self.me.clone();
            let disposables = Rc::clone(&disposables);
            let folder = folder.clone();
            Rc::new(move || {
                let Some(this) = me.upgrade() else {
                    return Promise::resolved(());
                };
                let disposables = Rc::clone(&disposables);
                let folder = folder.clone();
                async_fn(async move {
                    disposables.clear();
                    if !this.file_service.exists(&folder).await? {
                        return Ok(());
                    }
                    match this.file_service.resolve(&folder, None).await {
                        Ok(stat) => {
                            for entry in stat.children.unwrap_or_default() {
                                disposables.add(this.add_snippet_file(&entry.resource, source));
                            }
                        }
                        Err(error) => {
                            this.log_service.error_args(
                                &LogMessage::from(format!("Failed snippets from folder '{}'", folder.to_string(false).to_string_lossy())),
                                &[LogArg::from(&error)],
                            );
                        }
                    }
                    Ok(())
                })
            })
        };

        {
            let on_save = Rc::clone(&add_folder_snippets);
            let folder = folder.clone();
            bucket.add(self.textfile_service.files().on_did_save().listen(move |event| {
                if ext_uri().is_equal_or_parent(&event.model.resource(), &folder, false, false) {
                    drop(on_save());
                }
            }));
        }
        let on_change = Rc::clone(&add_folder_snippets);
        bucket.add(watch(&self.file_service, folder, Rc::new(move || drop(on_change()))));
        bucket.add_rc(disposables);
        add_folder_snippets()
    }

    fn add_snippet_file(&self, uri: &URI, source: SnippetSource) -> DisposableRef {
        let ext = ext_uri().extname(uri);
        if source == SnippetSource::User && ext == ".json" {
            let lang_name = ext_uri().basename(uri, None).replacen(".json", "", 1);
            self.files.borrow_mut().set(
                uri.clone(),
                SnippetFile::new(
                    source,
                    uri.clone(),
                    Some(SnippetScopes::new(vec![lang_name])),
                    None,
                    Rc::clone(&self.file_service_contract),
                    Rc::clone(&self.extension_resource_loader_service),
                ),
            );
        } else if ext == ".code-snippets" {
            self.files.borrow_mut().set(
                uri.clone(),
                SnippetFile::new(
                    source,
                    uri.clone(),
                    None,
                    None,
                    Rc::clone(&self.file_service_contract),
                    Rc::clone(&self.extension_resource_loader_service),
                ),
            );
        }
        let me = self.me.clone();
        let uri = uri.clone();
        plain_disposable(move || {
            if let Some(this) = me.upgrade() {
                this.files.borrow_mut().delete(&uri);
            }
        })
    }
}

impl IDisposable for SnippetsService {
    fn dispose(&self) {
        self.disposables.dispose();
    }
}

impl ISnippetsService for SnippetsService {
    fn is_enabled(&self, snippet: &Snippet) -> bool {
        !self.enablement.is_ignored(&snippet.snippet_identifier)
    }

    fn update_enablement(&self, snippet: &Snippet, enabled: bool) {
        self.enablement.update_ignored(&snippet.snippet_identifier, !enabled);
    }

    fn update_usage_timestamp(&self, snippet: &Snippet) {
        self.usage_timestamps.update_usage_timestamp(&snippet.snippet_identifier);
    }

    fn get_snippet_files(&self) -> Promise<Vec<Rc<SnippetFile>>> {
        let this = self.this();
        async_fn(async move {
            this.join_snippets().await?;
            let files: Vec<Rc<SnippetFile>> = this.files.borrow().values().cloned().collect();
            Ok(files)
        })
    }

    fn get_snippets(&self, language_id: Option<&str>, resource_uri: Option<&URI>, opts: Option<&ISnippetGetOptions>) -> Promise<Vec<Rc<Snippet>>> {
        let this = self.this();
        let language_id = language_id.map(str::to_owned);
        let resource_uri = resource_uri.cloned();
        let opts = opts.copied();
        async_fn(async move {
            this.join_snippets().await?;

            let result: Rc<RefCell<Vec<Rc<Snippet>>>> = Rc::new(RefCell::new(Vec::new()));
            let mut promises: Vec<Promise<()>> = Vec::new();
            let files: Vec<Rc<SnippetFile>> = this.files.borrow().values().cloned().collect();

            // `.catch(err => this._logService.error(err, file.location.toString()))`
            let load_each = |file: &Rc<SnippetFile>, on_loaded: Box<dyn FnOnce(&SnippetFile)>| -> Promise<()> {
                let log_service = Rc::clone(&this.log_service);
                let location = file.location.to_string(false).to_string_lossy();
                file.load().then_result(move |result| {
                    match result {
                        Ok(file) => on_loaded(&*file),
                        Err(error) => log_service.error_args(&LogMessage::from(error), &[LogArg::from(location.as_str())]),
                    }
                    Ok(())
                })
            };

            match language_id.as_deref().filter(|language_id| !language_id.is_empty()) {
                Some(language_id) => {
                    if this.language_service.is_registered_language_id(Some(language_id)) {
                        for file in &files {
                            let bucket = Rc::clone(&result);
                            let selector = language_id.to_owned();
                            promises.push(load_each(file, Box::new(move |file| file.select(&selector, &mut bucket.borrow_mut()))));
                        }
                    }
                }
                None => {
                    for file in &files {
                        let bucket = Rc::clone(&result);
                        promises.push(load_each(file, Box::new(move |file| bucket.borrow_mut().extend(file.data.borrow().iter().cloned()))));
                    }
                }
            }
            Promise::all(promises).await?;
            let collected = result.borrow().clone();
            Ok(this.filter_and_sort_snippets(collected, resource_uri.as_ref(), opts.as_ref()))
        })
    }

    fn get_snippets_sync(&self, language_id: &str, resource_uri: Option<&URI>, opts: Option<&ISnippetGetOptions>) -> Vec<Rc<Snippet>> {
        let mut result: Vec<Rc<Snippet>> = Vec::new();
        if self.language_service.is_registered_language_id(Some(language_id)) {
            let files: Vec<Rc<SnippetFile>> = self.files.borrow().values().cloned().collect();
            for file in files {
                // kick off loading (which is a noop in case it's already loaded)
                // and optimistically collect snippets
                drop(file.load().then_result(|_| Ok(())));
                file.select(language_id, &mut result);
            }
        }
        self.filter_and_sort_snippets(result, resource_uri, opts)
    }
}

/// `ISimpleModel`.
pub trait ISimpleModel {
    fn get_line_content(&self, line_number: i32) -> JsString;
}

impl ISimpleModel for TextModel {
    fn get_line_content(&self, line_number: i32) -> JsString {
        <TextModel as CursorSimpleModel>::get_line_content(self, line_number)
    }
}

pub fn get_non_whitespace_prefix(model: &dyn ISimpleModel, position: Position) -> JsString {
    /// Do not analyze more characters.
    const MAX_PREFIX_LENGTH: i64 = 100;

    let content = model.get_line_content(position.line_number);
    let end = usize::try_from(position.column - 1).unwrap_or(0).min(content.len());
    let line = &content.units()[..end];

    let min_ch_index = (line.len() as i64 - MAX_PREFIX_LENGTH).max(0);
    for ch_index in (min_ch_index..line.len() as i64).rev() {
        if is_js_whitespace(line[ch_index as usize]) {
            return JsString::from_units(line[ch_index as usize + 1..].to_vec());
        }
    }

    if min_ch_index == 0 {
        return JsString::from_units(line.to_vec());
    }

    JsString::new()
}
