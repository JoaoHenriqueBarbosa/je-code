//! Porte de `src/vs/workbench/services/configuration/browser/configuration.ts`: as camadas de configuração do
//! workbench (padrões com cache, aplicação, usuário local e remoto, workspace e pasta), cada uma lida pelo
//! `IFileService` ou, antes de o provedor existir, pelo cache em disco.
//!
//! Decisões de tradução:
//! - **Herança**: `DefaultConfiguration` e `ApplicationConfiguration` estendem classes da plataforma. A primeira
//!   guarda a `DefaultConfiguration` da plataforma e instala os dois pontos que o original troca
//!   ([`DefaultConfigurationHooks`]); a segunda guarda o `UserSettings`. As duas expõem a base por `Deref`.
//! - **Uniões** (`UserSettings | FileServiceBasedConfiguration`, `CachedFolderConfiguration |
//!   FileServiceBasedConfiguration` e as do workspace e do usuário remoto) são enums com os métodos que o original
//!   chama nas duas variantes.
//! - **`localStorage`** (a marca "existe cache de padrões") é um mapa por thread: um teste, um processo, como no
//!   original.
//! - **`Partial<ConfigurationParseOptions>`** de `UserConfiguration#reparse` é [`ParseOptionsPatch`]: o único campo que
//!   os chamadores trocam é `exclude`.
//! - **`IBrowserWorkbenchEnvironmentService.options.configurationDefaults`** chega como o argumento
//!   `configuration_defaults`, porque o serviço de ambiente do navegador não foi portado.
//! - Os `async` do original são [`Promise`]; as rotinas que o original dispara sem esperar (`updateCache` e companhia)
//!   têm a promessa descartada.

use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::ops::Deref;
use std::rc::Rc;

use je_base::JsString;
use je_base::async_::{RunOnceScheduler, async_fn};
use je_base::dynamic::{Dynamic, ObjectRef, Truthy};
use je_base::errors::Error;
use je_base::event::{DebounceOptions, Emitter, Event, any};
use je_base::hash;
use je_base::json::{self, ParseOptions};
use je_base::lifecycle::{Disposable, DisposableRef, DisposableStore, IDisposable, MutableDisposable};
use je_base::objects;
use je_base::promise::Promise;
use je_base::resources::ExtUri;
use je_base::types;
use je_base::uri::URI;
use je_platform::configuration::defaults::{DefaultConfiguration as BaseDefaultConfiguration, DefaultConfigurationHooks};
use je_platform::configuration::models::{ConfigurationModel, ConfigurationModelParser, ConfigurationParseOptions};
use je_platform::configuration::registry::{ConfigurationDefaults, ConfigurationRegistry, ConfigurationScope, configuration_registry, is_override_property};
use je_platform::configuration::user_settings::UserSettings;
use je_platform::files::file_service::{FileService, when_provider_registered};
use je_platform::files::files::{FileChangeType, FileChangesEvent, FileOperation, FileOperationError, FileOperationEvent, FileOperationResult, ReadFileOptions};
use je_platform::js::{for_in_entries, from_json, object_keys};
use je_platform::log::LogServiceRef;
use je_platform::uri_identity::uri_identity::IUriIdentityService;
use je_platform::user_data_profile::user_data_profile::IUserDataProfilesService;
use je_platform::workspace::{WorkbenchState, WorkspaceFolder, WorkspaceIdentifier};

use crate::services::configuration::common::configuration::{
    APPLICATION_SCOPES, APPLY_ALL_PROFILES_SETTING, ConfigurationKey, ConfigurationKeyType, FOLDER_SCOPES, FOLDER_SETTINGS_NAME, IConfigurationCache, LAUNCH_CONFIGURATION_KEY,
    MCP_CONFIGURATION_KEY, REMOTE_MACHINE_SCOPES, TASKS_CONFIGURATION_KEY, WORKSPACE_SCOPES,
};
use crate::services::configuration::common::configuration_models::{StandaloneConfigurationModelParser, StoredWorkspaceFolder, WorkspaceConfigurationModelParser};
use crate::services::configuration::common::json_editing::{IJsonEditingService, JsonValue};
use crate::services::remote::remote_agent_service::IRemoteAgentService;
use je_base::json_edit::EditSegment;
use je_platform::workspace::RawWorkspaceFolder;

// ---------------------------------------------------------------------------------------------
// Auxiliares.
// ---------------------------------------------------------------------------------------------

thread_local! {
    /// O `localStorage` do original.
    static LOCAL_STORAGE: RefCell<HashMap<String, String>> = RefCell::new(HashMap::new());
}

fn local_storage_get(key: &str) -> Option<String> {
    LOCAL_STORAGE.with(|storage| storage.borrow().get(key).cloned())
}

fn local_storage_set(key: &str, value: &str) {
    LOCAL_STORAGE.with(|storage| storage.borrow_mut().insert(key.to_owned(), value.to_owned()));
}

fn local_storage_remove(key: &str) {
    LOCAL_STORAGE.with(|storage| storage.borrow_mut().remove(key));
}

/// `JSON.parse(text)`; `None` quando o original lançaria.
fn json_parse(text: &str) -> Option<Dynamic> {
    let units: Vec<u16> = text.encode_utf16().collect();
    let mut errors = Vec::new();
    let value = json::parse(&units, &mut errors, ParseOptions::default())?;
    errors.is_empty().then(|| from_json(&value))
}

fn empty_object() -> Dynamic {
    Dynamic::Object(ObjectRef::new())
}

/// `{ content }` como o cache guarda (`JSON.stringify({ content })`).
fn content_envelope(content: &str) -> String {
    je_base::dynamic::json_stringify(&Dynamic::object([("content", Dynamic::string(content))])).expect("um objeto sempre tem JSON")
}

/// `JSON.parse(text).content`, se for texto não vazio.
fn parse_content_envelope(text: &str) -> Option<String> {
    match json_parse(text)?.get_str("content") {
        Dynamic::String(content) if !content.is_empty() => Some(content.to_string_lossy()),
        _ => None,
    }
}

/// `hash(value).toString(16)`.
fn hash_hex(value: &str) -> String {
    let hashed = hash::hash(&Dynamic::string(value));
    if hashed < 0 { format!("-{:x}", hashed.unsigned_abs()) } else { format!("{hashed:x}") }
}

/// O valor de uma lista de textos de configuração (`string[]`); qualquer outra coisa não é lista.
fn dynamic_to_string_list(value: &Dynamic) -> Option<Vec<JsString>> {
    match value {
        Dynamic::Array(items) => Some(
            items
                .to_vec()
                .into_iter()
                .filter_map(|item| match item {
                    Dynamic::String(text) => Some(text),
                    _ => None,
                })
                .collect(),
        ),
        _ => None,
    }
}

/// O `IStoredWorkspaceFolder` como objeto (`{ path | uri, name? }`).
pub fn stored_workspace_folder_to_dynamic(folder: &StoredWorkspaceFolder) -> Dynamic {
    let (key, value, name) = match folder {
        RawWorkspaceFolder::Path { path, name } => ("path", path, name),
        RawWorkspaceFolder::Uri { uri, name } => ("uri", uri, name),
    };
    let mut entries = vec![(key, Dynamic::string(value))];
    if let Some(name) = name {
        entries.push(("name", Dynamic::string(name)));
    }
    Dynamic::object(entries)
}

fn is_file_change_operation(event: &FileOperationEvent) -> bool {
    event.is_operation(FileOperation::Create) || event.is_operation(FileOperation::Copy) || event.is_operation(FileOperation::Delete) || event.is_operation(FileOperation::Write)
}

fn error_result(error: &Error) -> Option<FileOperationResult> {
    FileOperationError::from_error(error).map(|e| e.file_operation_result)
}

// ---------------------------------------------------------------------------------------------
// DefaultConfiguration
// ---------------------------------------------------------------------------------------------

struct DefaultConfigurationState {
    configuration_registry: Rc<ConfigurationRegistry>,
    cached_configuration_defaults_overrides: RefCell<Dynamic>,
    cache_key: ConfigurationKey,
    configuration_cache: Rc<dyn IConfigurationCache>,
    initialize_cached_configuration_defaults_overrides_promise: RefCell<Option<Promise<()>>>,
}

/// `updateCachedConfigurationDefaultsOverrides()`.
fn update_cached_configuration_defaults_overrides(registry: Rc<ConfigurationRegistry>, cache: Rc<dyn IConfigurationCache>, key: ConfigurationKey) -> Promise<()> {
    let cached_configuration_defaults_overrides = ObjectRef::new();
    let default_configurations = registry.get_registered_default_configurations();
    for default_configuration in &default_configurations {
        if default_configuration.donot_cache {
            continue;
        }
        for (property, value) in for_in_entries(&default_configuration.overrides) {
            if !is_override_property(&property) && !value.is_undefined() {
                let existing_value = cached_configuration_defaults_overrides.get(&property).unwrap_or(Dynamic::Undefined);
                if types::is_object(&existing_value) && types::is_object(&value) {
                    let merged = ObjectRef::new();
                    for (k, v) in existing_value.own_entries().into_iter().chain(value.own_entries()) {
                        merged.set(&k, v);
                    }
                    cached_configuration_defaults_overrides.set(&property, Dynamic::Object(merged));
                } else {
                    cached_configuration_defaults_overrides.set(&property, value);
                }
            }
        }
    }
    let cached = Dynamic::Object(cached_configuration_defaults_overrides);
    if !object_keys(&cached).is_empty() {
        local_storage_set(DEFAULT_OVERRIDES_CACHE_EXISTS_KEY, "yes");
        match je_base::dynamic::json_stringify(&cached) {
            Some(content) => cache.write(&key, &content).catch(|_| ()),
            None => Promise::resolved(()),
        }
    } else {
        local_storage_remove(DEFAULT_OVERRIDES_CACHE_EXISTS_KEY);
        cache.remove(&key).catch(|_| ())
    }
}

/// `DefaultConfiguration.DEFAULT_OVERRIDES_CACHE_EXISTS_KEY`.
pub const DEFAULT_OVERRIDES_CACHE_EXISTS_KEY: &str = "DefaultOverridesCacheExists";

impl DefaultConfigurationState {
    fn update_cached_configuration_defaults_overrides(&self) -> Promise<()> {
        update_cached_configuration_defaults_overrides(self.configuration_registry.clone(), self.configuration_cache.clone(), self.cache_key.clone())
    }

    /// `initializeCachedConfigurationDefaultsOverrides()`: roda uma vez.
    fn initialize_cached_configuration_defaults_overrides(self: &Rc<Self>) -> Promise<()> {
        if let Some(promise) = self.initialize_cached_configuration_defaults_overrides_promise.borrow().clone() {
            return promise;
        }
        let this = self.clone();
        let promise = async_fn(async move {
            // Read only when the cache exists
            if local_storage_get(DEFAULT_OVERRIDES_CACHE_EXISTS_KEY).is_some_and(|value| !value.is_empty())
                && let Ok(content) = this.configuration_cache.read(&this.cache_key).await
                && !content.is_empty()
                && let Some(parsed) = json_parse(&content)
            {
                *this.cached_configuration_defaults_overrides.borrow_mut() = parsed;
            }
            let is_object = types::is_object(&this.cached_configuration_defaults_overrides.borrow());
            if !is_object {
                *this.cached_configuration_defaults_overrides.borrow_mut() = empty_object();
            }
            Ok(())
        });
        *self.initialize_cached_configuration_defaults_overrides_promise.borrow_mut() = Some(promise.clone());
        promise
    }
}

impl DefaultConfigurationHooks for DefaultConfigurationState {
    fn get_configuration_default_overrides(&self) -> Dynamic {
        self.cached_configuration_defaults_overrides.borrow().clone()
    }

    fn on_did_update_configuration(&self, _properties: &[JsString], defaults_overrides: Option<bool>) {
        if defaults_overrides == Some(true) {
            let _ = self.update_cached_configuration_defaults_overrides();
        }
    }
}

/// `DefaultConfiguration` (a do workbench).
pub struct DefaultConfiguration {
    base: Rc<BaseDefaultConfiguration>,
    state: Rc<DefaultConfigurationState>,
}

impl DefaultConfiguration {
    pub fn new(cache_scope: &str, configuration_cache: Rc<dyn IConfigurationCache>, configuration_defaults: Option<Dynamic>, log: LogServiceRef) -> Rc<Self> {
        let configuration_registry = configuration_registry();
        let state = Rc::new(DefaultConfigurationState {
            configuration_registry: configuration_registry.clone(),
            cached_configuration_defaults_overrides: RefCell::new(empty_object()),
            cache_key: ConfigurationKey { type_: ConfigurationKeyType::Defaults, key: format!("{cache_scope}-configurationDefaultsOverrides") },
            configuration_cache,
            initialize_cached_configuration_defaults_overrides_promise: RefCell::new(None),
        });
        let base = BaseDefaultConfiguration::with_hooks(log, Some(state.clone() as Rc<dyn DefaultConfigurationHooks>));
        if let Some(configuration_defaults) = configuration_defaults {
            configuration_registry.register_default_configurations(&[ConfigurationDefaults::new(configuration_defaults, None)]);
        }
        Rc::new(Self { base, state })
    }

    /// A `DefaultConfiguration` da plataforma (a classe base), que a `PolicyConfiguration` recebe.
    pub fn base(&self) -> Rc<BaseDefaultConfiguration> {
        self.base.clone()
    }

    /// `initialize()`.
    pub fn initialize(&self) -> Promise<ConfigurationModel> {
        let base = self.base.clone();
        self.state.initialize_cached_configuration_defaults_overrides().then(move |()| base.initialize())
    }

    /// `reload()`.
    pub fn reload(&self) -> ConfigurationModel {
        *self.state.cached_configuration_defaults_overrides.borrow_mut() = empty_object();
        let _ = self.state.update_cached_configuration_defaults_overrides();
        self.base.reload()
    }

    /// `hasCachedConfigurationDefaultsOverrides()`.
    pub fn has_cached_configuration_defaults_overrides(&self) -> bool {
        !object_keys(&self.state.cached_configuration_defaults_overrides.borrow()).is_empty()
    }
}

impl Deref for DefaultConfiguration {
    type Target = BaseDefaultConfiguration;

    fn deref(&self) -> &BaseDefaultConfiguration {
        &self.base
    }
}

impl IDisposable for DefaultConfiguration {
    fn dispose(&self) {
        self.base.dispose();
    }
}

// ---------------------------------------------------------------------------------------------
// ApplicationConfiguration
// ---------------------------------------------------------------------------------------------

/// `ApplicationConfiguration`.
pub struct ApplicationConfiguration {
    base: Rc<Disposable>,
    settings: Rc<UserSettings>,
    on_did_change_configuration: Rc<Emitter<ConfigurationModel>>,
    reload_configuration_scheduler: Rc<RunOnceScheduler>,
}

impl ApplicationConfiguration {
    pub fn new(
        user_data_profiles_service: &dyn IUserDataProfilesService,
        file_service: Rc<FileService>,
        uri_identity_service: &dyn IUriIdentityService,
        log: LogServiceRef,
    ) -> Rc<Self> {
        let base = Disposable::new();
        let settings = UserSettings::new(
            user_data_profiles_service.default_profile().settings_resource,
            ConfigurationParseOptions { scopes: Some(APPLICATION_SCOPES.to_vec()), skip_unregistered: true, ..Default::default() },
            uri_identity_service.ext_uri(),
            file_service,
            log,
        );
        base.register(settings.clone());
        let on_did_change_configuration = Emitter::new();
        base.register(on_did_change_configuration.clone());
        let this = Rc::new_cyclic(|weak: &std::rc::Weak<Self>| {
            let weak = weak.clone();
            let reload_configuration_scheduler = RunOnceScheduler::new(
                move || {
                    if let Some(this) = weak.upgrade() {
                        let emitter = this.on_did_change_configuration.clone();
                        let _ = this.load_configuration().then(move |model| emitter.fire(model));
                    }
                },
                50,
            );
            base.register(reload_configuration_scheduler.clone());
            Self { base, settings, on_did_change_configuration, reload_configuration_scheduler }
        });
        let weak = Rc::downgrade(&this);
        this.base.register(this.settings.on_did_change().listen(move |()| {
            if let Some(this) = weak.upgrade() {
                this.reload_configuration_scheduler.schedule(None);
            }
        }));
        this
    }

    /// `onDidChangeConfiguration`.
    pub fn on_did_change_configuration(&self) -> Event<ConfigurationModel> {
        self.on_did_change_configuration.event()
    }

    /// `initialize()`.
    pub fn initialize(self: &Rc<Self>) -> Promise<ConfigurationModel> {
        self.load_configuration()
    }

    /// `loadConfiguration()`.
    pub fn load_configuration(self: &Rc<Self>) -> Promise<ConfigurationModel> {
        let this = self.clone();
        async_fn(async move {
            let model = this.settings.load_configuration().await?;
            let value = model.get_value(APPLY_ALL_PROFILES_SETTING);
            let all_profiles_settings = dynamic_to_string_list(&value).unwrap_or_default();
            let parse_options = this.settings.parse_options();
            if parse_options.include.is_some() || !all_profiles_settings.is_empty() {
                Ok(this.settings.reparse(Some(ConfigurationParseOptions { include: Some(all_profiles_settings), ..parse_options })))
            } else {
                Ok(model)
            }
        })
    }
}

impl Deref for ApplicationConfiguration {
    type Target = UserSettings;

    fn deref(&self) -> &UserSettings {
        &self.settings
    }
}

impl IDisposable for ApplicationConfiguration {
    fn dispose(&self) {
        self.base.dispose();
    }
}

// ---------------------------------------------------------------------------------------------
// UserConfiguration
// ---------------------------------------------------------------------------------------------

/// `Partial<ConfigurationParseOptions>` de `UserConfiguration#reparse`.
#[derive(Clone, Debug, Default)]
pub struct ParseOptionsPatch {
    pub exclude: Option<Vec<JsString>>,
}

impl ParseOptionsPatch {
    /// `{ exclude: value }` com o valor de uma configuração (`unknown`).
    pub fn exclude_of(value: &Dynamic) -> Self {
        Self { exclude: dynamic_to_string_list(value) }
    }
}

/// `UserSettings | FileServiceBasedConfiguration`.
enum UserConfigurationSource {
    Settings(Rc<UserSettings>),
    FileService(Rc<FileServiceBasedConfiguration>),
}

impl UserConfigurationSource {
    fn on_did_change(&self) -> Event<()> {
        match self {
            UserConfigurationSource::Settings(settings) => settings.on_did_change(),
            UserConfigurationSource::FileService(configuration) => configuration.on_did_change(),
        }
    }

    fn load_configuration(&self) -> Promise<ConfigurationModel> {
        match self {
            UserConfigurationSource::Settings(settings) => settings.load_configuration(),
            UserConfigurationSource::FileService(configuration) => configuration.load_configuration(None),
        }
    }

    fn reparse(&self, parse_options: &ConfigurationParseOptions) -> ConfigurationModel {
        match self {
            UserConfigurationSource::Settings(settings) => settings.reparse(Some(parse_options.clone())),
            UserConfigurationSource::FileService(configuration) => configuration.reparse(parse_options),
        }
    }

    fn get_restricted_settings(&self) -> Vec<JsString> {
        match self {
            UserConfigurationSource::Settings(settings) => settings.get_restricted_settings(),
            UserConfigurationSource::FileService(configuration) => configuration.get_restricted_settings(),
        }
    }
}

impl IDisposable for UserConfigurationSource {
    fn dispose(&self) {
        match self {
            UserConfigurationSource::Settings(settings) => settings.dispose(),
            UserConfigurationSource::FileService(configuration) => configuration.dispose(),
        }
    }
}

/// `UserConfiguration`.
pub struct UserConfiguration {
    base: Rc<Disposable>,
    on_did_change_configuration: Rc<Emitter<ConfigurationModel>>,
    user_configuration: Rc<MutableDisposable<UserConfigurationSource>>,
    user_configuration_change_disposable: Rc<MutableDisposable<dyn IDisposable>>,
    reload_configuration_scheduler: Rc<RunOnceScheduler>,
    settings_resource: RefCell<URI>,
    tasks_resource: RefCell<Option<URI>>,
    mcp_resource: RefCell<Option<URI>>,
    configuration_parse_options: RefCell<ConfigurationParseOptions>,
    file_service: Rc<FileService>,
    ext_uri: ExtUri,
    log: LogServiceRef,
}

impl UserConfiguration {
    pub fn new(
        settings_resource: URI,
        tasks_resource: Option<URI>,
        mcp_resource: Option<URI>,
        configuration_parse_options: ConfigurationParseOptions,
        file_service: Rc<FileService>,
        uri_identity_service: &dyn IUriIdentityService,
        log: LogServiceRef,
    ) -> Rc<Self> {
        let base = Disposable::new();
        let on_did_change_configuration = Emitter::new();
        base.register(on_did_change_configuration.clone());
        let user_configuration = MutableDisposable::<UserConfigurationSource>::new();
        base.register(user_configuration.clone());
        let user_configuration_change_disposable = MutableDisposable::<dyn IDisposable>::new();
        base.register(user_configuration_change_disposable.clone());
        let ext_uri = uri_identity_service.ext_uri();
        user_configuration.set(Some(Rc::new(UserConfigurationSource::Settings(UserSettings::new(
            settings_resource.clone(),
            configuration_parse_options.clone(),
            ext_uri.clone(),
            file_service.clone(),
            log.clone(),
        )))));

        let this = Rc::new_cyclic(|weak: &std::rc::Weak<Self>| {
            let weak = weak.clone();
            let reload_configuration_scheduler = RunOnceScheduler::new(
                move || {
                    if let Some(this) = weak.upgrade()
                        && let Some(source) = this.user_configuration.value()
                    {
                        let emitter = this.on_did_change_configuration.clone();
                        let _ = source.load_configuration().then(move |model| emitter.fire(model));
                    }
                },
                50,
            );
            base.register(reload_configuration_scheduler.clone());
            Self {
                base,
                on_did_change_configuration,
                user_configuration,
                user_configuration_change_disposable,
                reload_configuration_scheduler,
                settings_resource: RefCell::new(settings_resource),
                tasks_resource: RefCell::new(tasks_resource),
                mcp_resource: RefCell::new(mcp_resource),
                configuration_parse_options: RefCell::new(configuration_parse_options),
                file_service,
                ext_uri,
                log,
            }
        });
        this.watch_current_source();
        this
    }

    /// `this.userConfigurationChangeDisposable.value = this.userConfiguration.value.onDidChange(() => scheduler.schedule())`.
    fn watch_current_source(self: &Rc<Self>) {
        if let Some(source) = self.user_configuration.value() {
            let weak = Rc::downgrade(self);
            let listener: DisposableRef = source.on_did_change().listen(move |()| {
                if let Some(this) = weak.upgrade() {
                    this.reload_configuration_scheduler.schedule(None);
                }
            });
            self.user_configuration_change_disposable.set(Some(listener));
        }
    }

    /// `onDidChangeConfiguration`.
    pub fn on_did_change_configuration(&self) -> Event<ConfigurationModel> {
        self.on_did_change_configuration.event()
    }

    /// `hasTasksLoaded`.
    pub fn has_tasks_loaded(&self) -> bool {
        matches!(self.user_configuration.value().as_deref(), Some(UserConfigurationSource::FileService(_)))
    }

    /// `reset(settingsResource, tasksResource, mcpResource, configurationParseOptions)`.
    pub fn reset(
        self: &Rc<Self>,
        settings_resource: URI,
        tasks_resource: Option<URI>,
        mcp_resource: Option<URI>,
        configuration_parse_options: ConfigurationParseOptions,
    ) -> Promise<ConfigurationModel> {
        *self.settings_resource.borrow_mut() = settings_resource;
        *self.tasks_resource.borrow_mut() = tasks_resource;
        *self.mcp_resource.borrow_mut() = mcp_resource;
        *self.configuration_parse_options.borrow_mut() = configuration_parse_options;
        self.do_reset(None)
    }

    fn do_reset(self: &Rc<Self>, settings_configuration: Option<ConfigurationModel>) -> Promise<ConfigurationModel> {
        let this = self.clone();
        async_fn(async move {
            let settings_resource = this.settings_resource.borrow().clone();
            let folder = this.ext_uri.dirname(&settings_resource);
            let mut stand_alone_configuration_resources: Vec<(String, URI)> = Vec::new();
            if let Some(tasks_resource) = this.tasks_resource.borrow().clone() {
                stand_alone_configuration_resources.push((TASKS_CONFIGURATION_KEY.to_owned(), tasks_resource));
            }
            if let Some(mcp_resource) = this.mcp_resource.borrow().clone() {
                stand_alone_configuration_resources.push((MCP_CONFIGURATION_KEY.to_owned(), mcp_resource));
            }
            let file_service_based_configuration = FileServiceBasedConfiguration::new(
                folder.to_string(false).to_string_lossy(),
                settings_resource,
                stand_alone_configuration_resources,
                this.configuration_parse_options.borrow().clone(),
                this.file_service.clone(),
                this.ext_uri.clone(),
                this.log.clone(),
            );
            let configuration_model = file_service_based_configuration.load_configuration(settings_configuration).await?;
            this.user_configuration.set(Some(Rc::new(UserConfigurationSource::FileService(file_service_based_configuration))));

            // Check for value because userConfiguration might have been disposed.
            if this.user_configuration_change_disposable.value().is_some() {
                this.watch_current_source();
            }

            Ok(configuration_model)
        })
    }

    /// `initialize()`.
    pub fn initialize(&self) -> Promise<ConfigurationModel> {
        self.user_configuration.value().expect("a configuração do usuário existe").load_configuration()
    }

    /// `reload(settingsConfiguration?)`.
    pub fn reload(self: &Rc<Self>, settings_configuration: Option<ConfigurationModel>) -> Promise<ConfigurationModel> {
        if self.has_tasks_loaded() {
            return self.user_configuration.value().expect("a configuração do usuário existe").load_configuration();
        }
        self.do_reset(settings_configuration)
    }

    /// `reparse(parseOptions?)`.
    pub fn reparse(&self, parse_options: Option<ParseOptionsPatch>) -> ConfigurationModel {
        if let Some(patch) = parse_options {
            self.configuration_parse_options.borrow_mut().exclude = patch.exclude;
        }
        let options = self.configuration_parse_options.borrow().clone();
        self.user_configuration.value().expect("a configuração do usuário existe").reparse(&options)
    }

    /// `getRestrictedSettings()`.
    pub fn get_restricted_settings(&self) -> Vec<JsString> {
        self.user_configuration.value().expect("a configuração do usuário existe").get_restricted_settings()
    }
}

impl IDisposable for UserConfiguration {
    fn dispose(&self) {
        self.base.dispose();
    }
}

// ---------------------------------------------------------------------------------------------
// FileServiceBasedConfiguration
// ---------------------------------------------------------------------------------------------

/// `[settingsContent, [[key, content]]]` de `resolveContents`.
pub type ResolvedContents = (Option<String>, Vec<(String, Option<String>)>);

struct FileServiceBasedConfiguration {
    base: Rc<Disposable>,
    all_resources: Vec<URI>,
    stand_alone_configuration_resources: Vec<(String, URI)>,
    settings_resource: URI,
    folder_settings_model_parser: RefCell<ConfigurationModelParser>,
    folder_settings_parse_options: RefCell<ConfigurationParseOptions>,
    stand_alone_configurations: RefCell<Vec<ConfigurationModel>>,
    cache: RefCell<ConfigurationModel>,
    on_did_change: Rc<Emitter<()>>,
    file_service: Rc<FileService>,
    log: LogServiceRef,
}

impl FileServiceBasedConfiguration {
    fn new(
        name: String,
        settings_resource: URI,
        stand_alone_configuration_resources: Vec<(String, URI)>,
        configuration_parse_options: ConfigurationParseOptions,
        file_service: Rc<FileService>,
        ext_uri: ExtUri,
        log: LogServiceRef,
    ) -> Rc<Self> {
        let base = Disposable::new();
        let mut all_resources = vec![settings_resource.clone()];
        all_resources.extend(stand_alone_configuration_resources.iter().map(|(_, resource)| resource.clone()));
        for resource in &all_resources {
            base.register(file_service.watch(&ext_uri.dirname(resource), None));
            // Also listen to the resource incase the resource is a symlink - https://github.com/microsoft/vscode/issues/118134
            base.register(file_service.watch(resource, None));
        }
        let on_did_change = Emitter::new();
        base.register(on_did_change.clone());

        let files_change = file_service.on_did_files_change().filter(
            {
                let (all_resources, ext_uri) = (all_resources.clone(), ext_uri.clone());
                move |event: &Rc<FileChangesEvent>| Self::handle_file_changes_event(&all_resources, &ext_uri, event)
            },
            None,
        );
        let operation = file_service.on_did_run_operation().filter(
            {
                let (all_resources, ext_uri) = (all_resources.clone(), ext_uri);
                move |event: &FileOperationEvent| Self::handle_file_operation_event(&all_resources, &ext_uri, event)
            },
            None,
        );
        let emitter = on_did_change.clone();
        let debounced = any(vec![files_change.signal(), operation.signal()]).debounce(|_: Option<()>, _: &()| (), DebounceOptions::default(), None);
        base.register(debounced.listen(move |()| emitter.fire(())));

        Rc::new(Self {
            base,
            all_resources,
            stand_alone_configuration_resources,
            settings_resource,
            folder_settings_model_parser: RefCell::new(ConfigurationModelParser::new(name, log.clone())),
            folder_settings_parse_options: RefCell::new(configuration_parse_options),
            stand_alone_configurations: RefCell::new(Vec::new()),
            cache: RefCell::new(ConfigurationModel::create_empty_model(log.clone())),
            on_did_change,
            file_service,
            log,
        })
    }

    fn on_did_change(&self) -> Event<()> {
        self.on_did_change.event()
    }

    fn resolve_resources(self: &Rc<Self>, resources: Vec<URI>) -> Promise<Vec<String>> {
        Promise::all(
            resources
                .into_iter()
                .map(|resource| {
                    let this = self.clone();
                    async_fn(async move {
                        match this.file_service.read_file(&resource, Some(ReadFileOptions { atomic: true, ..Default::default() }), None).await {
                            Ok(content) => Ok(content.value.to_string()),
                            Err(error) => {
                                this.log.trace(&format!("Error while resolving configuration file '{}': {}", resource.to_string(false), error.message));
                                let result = error_result(&error);
                                if result != Some(FileOperationResult::FileNotFound) && result != Some(FileOperationResult::FileNotDirectory) {
                                    this.log.error(&error.message);
                                }
                                Ok("{}".to_owned())
                            }
                        }
                    })
                })
                .collect(),
        )
    }

    /// `resolveContents(donotResolveSettings?)`.
    fn resolve_contents(self: &Rc<Self>, donot_resolve_settings: bool) -> Promise<ResolvedContents> {
        let this = self.clone();
        async_fn(async move {
            let stand_alone_keys: Vec<String> = this.stand_alone_configuration_resources.iter().map(|(key, _)| key.clone()).collect();
            let stand_alone_resources: Vec<URI> = this.stand_alone_configuration_resources.iter().map(|(_, resource)| resource.clone()).collect();
            let settings = if donot_resolve_settings { Promise::resolved(None) } else { this.resolve_resources(vec![this.settings_resource.clone()]).then(|mut contents| contents.pop()) };
            let stand_alone = this.resolve_resources(stand_alone_resources);
            let settings_content = settings.await?;
            let stand_alone_contents = stand_alone.await?;
            Ok((settings_content, stand_alone_keys.into_iter().zip(stand_alone_contents.into_iter().map(Some)).collect()))
        })
    }

    /// `loadConfiguration(settingsConfiguration?)`.
    fn load_configuration(self: &Rc<Self>, settings_configuration: Option<ConfigurationModel>) -> Promise<ConfigurationModel> {
        let this = self.clone();
        async_fn(async move {
            let (settings_content, stand_alone_configuration_contents) = this.resolve_contents(settings_configuration.is_some()).await?;

            // reset
            this.stand_alone_configurations.borrow_mut().clear();
            let options = this.folder_settings_parse_options.borrow().clone();
            this.folder_settings_model_parser.borrow_mut().parse(Some(""), Some(&options));

            // parse
            if let Some(settings_content) = settings_content {
                this.folder_settings_model_parser.borrow_mut().parse(Some(&settings_content), Some(&options));
            }
            for (index, (_, contents)) in stand_alone_configuration_contents.iter().enumerate() {
                if let Some(contents) = contents {
                    let (key, resource) = &this.stand_alone_configuration_resources[index];
                    let mut parser = StandaloneConfigurationModelParser::new(resource.to_string(false).to_string_lossy(), key.clone(), this.log.clone());
                    parser.parse(Some(contents), None);
                    this.stand_alone_configurations.borrow_mut().push(parser.configuration_model());
                }
            }

            // Consolidate (support *.json files in the workspace settings folder)
            this.consolidate(settings_configuration);

            Ok(this.cache.borrow().clone())
        })
    }

    fn get_restricted_settings(&self) -> Vec<JsString> {
        self.folder_settings_model_parser.borrow().restricted_configurations().to_vec()
    }

    fn reparse(&self, configuration_parse_options: &ConfigurationParseOptions) -> ConfigurationModel {
        let old_contents = self.folder_settings_model_parser.borrow().configuration_model().contents();
        *self.folder_settings_parse_options.borrow_mut() = configuration_parse_options.clone();
        self.folder_settings_model_parser.borrow_mut().reparse(configuration_parse_options);
        if !objects::equals(&old_contents, &self.folder_settings_model_parser.borrow().configuration_model().contents()) {
            self.consolidate(None);
        }
        self.cache.borrow().clone()
    }

    fn consolidate(&self, settings_configuration: Option<ConfigurationModel>) {
        let settings = settings_configuration.unwrap_or_else(|| self.folder_settings_model_parser.borrow().configuration_model());
        *self.cache.borrow_mut() = settings.merge(&self.stand_alone_configurations.borrow());
    }

    fn handle_file_changes_event(all_resources: &[URI], ext_uri: &ExtUri, event: &FileChangesEvent) -> bool {
        // One of the resources has changed
        if all_resources.iter().any(|resource| event.contains(resource, &[])) {
            return true;
        }
        // One of the resource's parent got deleted
        all_resources.iter().any(|resource| event.contains(&ext_uri.dirname(resource), &[FileChangeType::Deleted]))
    }

    fn handle_file_operation_event(all_resources: &[URI], ext_uri: &ExtUri, event: &FileOperationEvent) -> bool {
        // One of the resources has changed
        if is_file_change_operation(event) && all_resources.iter().any(|resource| ext_uri.is_equal(Some(&event.resource), Some(resource), false)) {
            return true;
        }
        // One of the resource's parent got deleted
        event.is_operation(FileOperation::Delete) && all_resources.iter().any(|resource| ext_uri.is_equal(Some(&event.resource), Some(&ext_uri.dirname(resource)), false))
    }
}

impl IDisposable for FileServiceBasedConfiguration {
    fn dispose(&self) {
        self.base.dispose();
    }
}

// ---------------------------------------------------------------------------------------------
// RemoteUserConfiguration
// ---------------------------------------------------------------------------------------------

/// `FileServiceBasedRemoteUserConfiguration | CachedRemoteUserConfiguration`.
enum RemoteUserConfigurationSource {
    Cached(Rc<CachedRemoteUserConfiguration>),
    FileService(Rc<FileServiceBasedRemoteUserConfiguration>),
}

impl RemoteUserConfigurationSource {
    fn reload(&self) -> Promise<ConfigurationModel> {
        match self {
            RemoteUserConfigurationSource::Cached(configuration) => configuration.reload(),
            RemoteUserConfigurationSource::FileService(configuration) => configuration.reload(),
        }
    }

    fn reparse(&self, options: &ConfigurationParseOptions) -> ConfigurationModel {
        match self {
            RemoteUserConfigurationSource::Cached(configuration) => configuration.reparse(options),
            RemoteUserConfigurationSource::FileService(configuration) => configuration.reparse(options),
        }
    }

    fn get_restricted_settings(&self) -> Vec<JsString> {
        match self {
            RemoteUserConfigurationSource::Cached(configuration) => configuration.get_restricted_settings(),
            RemoteUserConfigurationSource::FileService(configuration) => configuration.get_restricted_settings(),
        }
    }

    fn dispose(&self) {
        match self {
            RemoteUserConfigurationSource::Cached(configuration) => configuration.dispose(),
            RemoteUserConfigurationSource::FileService(configuration) => configuration.dispose(),
        }
    }
}

/// `RemoteUserConfiguration`.
pub struct RemoteUserConfiguration {
    base: Rc<Disposable>,
    cached_configuration: Rc<CachedRemoteUserConfiguration>,
    file_service: Rc<FileService>,
    user_configuration: RefCell<RemoteUserConfigurationSource>,
    user_configuration_initialization_promise: RefCell<Option<Promise<ConfigurationModel>>>,
    on_did_change_configuration: Rc<Emitter<ConfigurationModel>>,
    on_did_initialize: Rc<Emitter<ConfigurationModel>>,
}

impl RemoteUserConfiguration {
    pub fn new(
        remote_authority: &str,
        configuration_cache: Rc<dyn IConfigurationCache>,
        file_service: Rc<FileService>,
        uri_identity_service: &dyn IUriIdentityService,
        remote_agent_service: &dyn IRemoteAgentService,
        log: LogServiceRef,
    ) -> Rc<Self> {
        let base = Disposable::new();
        let on_did_change_configuration = Emitter::new();
        base.register(on_did_change_configuration.clone());
        let on_did_initialize = Emitter::new();
        base.register(on_did_initialize.clone());
        let cached_configuration = Rc::new(CachedRemoteUserConfiguration::new(
            remote_authority,
            configuration_cache,
            ConfigurationParseOptions { scopes: Some(REMOTE_MACHINE_SCOPES.to_vec()), ..Default::default() },
            log.clone(),
        ));
        let this = Rc::new(Self {
            base,
            cached_configuration: cached_configuration.clone(),
            file_service,
            user_configuration: RefCell::new(RemoteUserConfigurationSource::Cached(cached_configuration)),
            user_configuration_initialization_promise: RefCell::new(None),
            on_did_change_configuration,
            on_did_initialize,
        });
        let (target, ext_uri) = (this.clone(), uri_identity_service.ext_uri());
        let _ = remote_agent_service.get_environment().then_promise(move |environment| {
            let Some(environment) = environment else {
                return Promise::resolved(());
            };
            async_fn(async move {
                let user_configuration = FileServiceBasedRemoteUserConfiguration::new(
                    environment.settings_path,
                    ConfigurationParseOptions { scopes: Some(REMOTE_MACHINE_SCOPES.to_vec()), ..Default::default() },
                    target.file_service.clone(),
                    ext_uri,
                    log,
                );
                target.base.register(user_configuration.clone());
                let weak = Rc::downgrade(&target);
                target.base.register(user_configuration.on_did_change_configuration().listen(move |model| {
                    if let Some(this) = weak.upgrade() {
                        this.on_did_user_configuration_change(model.clone());
                    }
                }));
                let initialization = user_configuration.initialize();
                *target.user_configuration_initialization_promise.borrow_mut() = Some(initialization.clone());
                let configuration_model = initialization.await?;
                target.user_configuration.borrow().dispose();
                *target.user_configuration.borrow_mut() = RemoteUserConfigurationSource::FileService(user_configuration);
                target.on_did_user_configuration_change(configuration_model.clone());
                target.on_did_initialize.fire(configuration_model);
                Ok(())
            })
        });
        this
    }

    /// `onDidChangeConfiguration`.
    pub fn on_did_change_configuration(&self) -> Event<ConfigurationModel> {
        self.on_did_change_configuration.event()
    }

    /// `onDidInitialize`.
    pub fn on_did_initialize(&self) -> Event<ConfigurationModel> {
        self.on_did_initialize.event()
    }

    /// `initialize()`.
    pub fn initialize(self: &Rc<Self>) -> Promise<ConfigurationModel> {
        let this = self.clone();
        async_fn(async move {
            let file_service_based = match &*this.user_configuration.borrow() {
                RemoteUserConfigurationSource::FileService(configuration) => Some(configuration.clone()),
                RemoteUserConfigurationSource::Cached(_) => None,
            };
            if let Some(configuration) = file_service_based {
                return configuration.initialize().await;
            }

            // Initialize cached configuration
            let mut configuration_model = this.cached_configuration.initialize().await?;
            let initialization_promise = this.user_configuration_initialization_promise.borrow().clone();
            if let Some(promise) = initialization_promise {
                // Use user configuration
                configuration_model = promise.await?;
                *this.user_configuration_initialization_promise.borrow_mut() = None;
            }
            Ok(configuration_model)
        })
    }

    /// `reload()`.
    pub fn reload(&self) -> Promise<ConfigurationModel> {
        self.user_configuration.borrow().reload()
    }

    /// `reparse()`.
    pub fn reparse(&self) -> ConfigurationModel {
        self.user_configuration.borrow().reparse(&ConfigurationParseOptions { scopes: Some(REMOTE_MACHINE_SCOPES.to_vec()), ..Default::default() })
    }

    /// `getRestrictedSettings()`.
    pub fn get_restricted_settings(&self) -> Vec<JsString> {
        self.user_configuration.borrow().get_restricted_settings()
    }

    fn on_did_user_configuration_change(self: &Rc<Self>, configuration_model: ConfigurationModel) {
        let _ = self.update_cache();
        self.on_did_change_configuration.fire(configuration_model);
    }

    fn update_cache(self: &Rc<Self>) -> Promise<()> {
        let this = self.clone();
        async_fn(async move {
            let file_service_based = match &*this.user_configuration.borrow() {
                RemoteUserConfigurationSource::FileService(configuration) => Some(configuration.clone()),
                RemoteUserConfigurationSource::Cached(_) => None,
            };
            if let Some(configuration) = file_service_based {
                let content = match configuration.resolve_content().await {
                    Ok(content) => Some(content),
                    Err(error) => {
                        if error_result(&error) != Some(FileOperationResult::FileNotFound) {
                            return Ok(());
                        }
                        None
                    }
                };
                this.cached_configuration.update_configuration(content).await?;
            }
            Ok(())
        })
    }
}

impl IDisposable for RemoteUserConfiguration {
    fn dispose(&self) {
        self.base.dispose();
    }
}

struct FileServiceBasedRemoteUserConfiguration {
    base: Rc<Disposable>,
    parser: RefCell<ConfigurationModelParser>,
    parse_options: RefCell<ConfigurationParseOptions>,
    on_did_change_configuration: Rc<Emitter<ConfigurationModel>>,
    file_watcher_disposable: Rc<MutableDisposable<dyn IDisposable>>,
    directory_watcher_disposable: Rc<MutableDisposable<dyn IDisposable>>,
    reload_configuration_scheduler: Rc<RunOnceScheduler>,
    configuration_resource: URI,
    file_service: Rc<FileService>,
    ext_uri: ExtUri,
    log: LogServiceRef,
}

impl FileServiceBasedRemoteUserConfiguration {
    fn new(configuration_resource: URI, configuration_parse_options: ConfigurationParseOptions, file_service: Rc<FileService>, ext_uri: ExtUri, log: LogServiceRef) -> Rc<Self> {
        let base = Disposable::new();
        let on_did_change_configuration = Emitter::new();
        base.register(on_did_change_configuration.clone());
        let file_watcher_disposable = MutableDisposable::<dyn IDisposable>::new();
        base.register(file_watcher_disposable.clone());
        let directory_watcher_disposable = MutableDisposable::<dyn IDisposable>::new();
        base.register(directory_watcher_disposable.clone());
        let this = Rc::new_cyclic(|weak: &std::rc::Weak<Self>| {
            let weak = weak.clone();
            let reload_configuration_scheduler = RunOnceScheduler::new(
                move || {
                    if let Some(this) = weak.upgrade() {
                        let emitter = this.on_did_change_configuration.clone();
                        let _ = this.reload().then(move |model| emitter.fire(model));
                    }
                },
                50,
            );
            base.register(reload_configuration_scheduler.clone());
            Self {
                base,
                parser: RefCell::new(ConfigurationModelParser::new(configuration_resource.to_string(false).to_string_lossy(), log.clone())),
                parse_options: RefCell::new(configuration_parse_options),
                on_did_change_configuration,
                file_watcher_disposable,
                directory_watcher_disposable,
                reload_configuration_scheduler,
                configuration_resource,
                file_service,
                ext_uri,
                log,
            }
        });
        let weak = Rc::downgrade(&this);
        this.base.register(this.file_service.on_did_files_change().listen(move |event| {
            if let Some(this) = weak.upgrade() {
                this.handle_file_changes_event(event);
            }
        }));
        let weak = Rc::downgrade(&this);
        this.base.register(this.file_service.on_did_run_operation().listen(move |event| {
            if let Some(this) = weak.upgrade() {
                this.handle_file_operation_event(event);
            }
        }));
        this
    }

    fn on_did_change_configuration(&self) -> Event<ConfigurationModel> {
        self.on_did_change_configuration.event()
    }

    fn watch_resource(&self) {
        self.file_watcher_disposable.set(Some(self.file_service.watch(&self.configuration_resource, None)));
    }

    fn stop_watching_resource(&self) {
        self.file_watcher_disposable.set(None);
    }

    fn watch_directory(&self) {
        let directory = self.ext_uri.dirname(&self.configuration_resource);
        self.directory_watcher_disposable.set(Some(self.file_service.watch(&directory, None)));
    }

    fn stop_watching_directory(&self) {
        self.directory_watcher_disposable.set(None);
    }

    fn initialize(self: &Rc<Self>) -> Promise<ConfigurationModel> {
        let this = self.clone();
        async_fn(async move {
            let exists = this.file_service.exists(&this.configuration_resource).await?;
            this.on_resource_exists(exists);
            this.reload().await
        })
    }

    fn resolve_content(self: &Rc<Self>) -> Promise<String> {
        self.file_service.read_file(&self.configuration_resource, Some(ReadFileOptions { atomic: true, ..Default::default() }), None).then(|content| content.value.to_string())
    }

    fn reload(self: &Rc<Self>) -> Promise<ConfigurationModel> {
        let this = self.clone();
        async_fn(async move {
            match this.resolve_content().await {
                Ok(content) => {
                    let options = this.parse_options.borrow().clone();
                    let mut parser = this.parser.borrow_mut();
                    parser.parse(Some(&content), Some(&options));
                    Ok(parser.configuration_model())
                }
                Err(_) => Ok(ConfigurationModel::create_empty_model(this.log.clone())),
            }
        })
    }

    fn reparse(&self, configuration_parse_options: &ConfigurationParseOptions) -> ConfigurationModel {
        *self.parse_options.borrow_mut() = configuration_parse_options.clone();
        let mut parser = self.parser.borrow_mut();
        parser.reparse(configuration_parse_options);
        parser.configuration_model()
    }

    fn get_restricted_settings(&self) -> Vec<JsString> {
        self.parser.borrow().restricted_configurations().to_vec()
    }

    fn handle_file_changes_event(&self, event: &FileChangesEvent) {
        // Find changes that affect the resource
        let mut affected_by_changes = false;
        if event.contains(&self.configuration_resource, &[FileChangeType::Added]) {
            affected_by_changes = true;
            self.on_resource_exists(true);
        } else if event.contains(&self.configuration_resource, &[FileChangeType::Deleted]) {
            affected_by_changes = true;
            self.on_resource_exists(false);
        } else if event.contains(&self.configuration_resource, &[FileChangeType::Updated]) {
            affected_by_changes = true;
        }

        if affected_by_changes {
            self.reload_configuration_scheduler.schedule(None);
        }
    }

    fn handle_file_operation_event(&self, event: &FileOperationEvent) {
        if is_file_change_operation(event) && self.ext_uri.is_equal(Some(&event.resource), Some(&self.configuration_resource), false) {
            self.reload_configuration_scheduler.schedule(None);
        }
    }

    fn on_resource_exists(&self, exists: bool) {
        if exists {
            self.stop_watching_directory();
            self.watch_resource();
        } else {
            self.stop_watching_resource();
            self.watch_directory();
        }
    }
}

impl IDisposable for FileServiceBasedRemoteUserConfiguration {
    fn dispose(&self) {
        self.stop_watching_resource();
        self.stop_watching_directory();
        self.base.dispose();
    }
}

struct CachedRemoteUserConfiguration {
    base: Rc<Disposable>,
    key: ConfigurationKey,
    configuration_cache: Rc<dyn IConfigurationCache>,
    parser: RefCell<ConfigurationModelParser>,
    parse_options: RefCell<ConfigurationParseOptions>,
    configuration_model: RefCell<ConfigurationModel>,
}

impl CachedRemoteUserConfiguration {
    fn new(remote_authority: &str, configuration_cache: Rc<dyn IConfigurationCache>, configuration_parse_options: ConfigurationParseOptions, log: LogServiceRef) -> Self {
        Self {
            base: Disposable::new(),
            key: ConfigurationKey { type_: ConfigurationKeyType::User, key: remote_authority.to_owned() },
            configuration_cache,
            parser: RefCell::new(ConfigurationModelParser::new("CachedRemoteUserConfiguration", log.clone())),
            parse_options: RefCell::new(configuration_parse_options),
            configuration_model: RefCell::new(ConfigurationModel::create_empty_model(log)),
        }
    }

    fn initialize(self: &Rc<Self>) -> Promise<ConfigurationModel> {
        self.reload()
    }

    fn reparse(&self, configuration_parse_options: &ConfigurationParseOptions) -> ConfigurationModel {
        *self.parse_options.borrow_mut() = configuration_parse_options.clone();
        self.parser.borrow_mut().reparse(configuration_parse_options);
        *self.configuration_model.borrow_mut() = self.parser.borrow().configuration_model();
        self.configuration_model.borrow().clone()
    }

    fn get_restricted_settings(&self) -> Vec<JsString> {
        self.parser.borrow().restricted_configurations().to_vec()
    }

    fn reload(self: &Rc<Self>) -> Promise<ConfigurationModel> {
        let this = self.clone();
        async_fn(async move {
            if let Ok(content) = this.configuration_cache.read(&this.key).await
                && let Some(parsed) = parse_content_envelope(&content)
            {
                let options = this.parse_options.borrow().clone();
                this.parser.borrow_mut().parse(Some(&parsed), Some(&options));
                *this.configuration_model.borrow_mut() = this.parser.borrow().configuration_model();
            }
            Ok(this.configuration_model.borrow().clone())
        })
    }

    fn update_configuration(&self, content: Option<String>) -> Promise<()> {
        match content.filter(|content| !content.is_empty()) {
            Some(content) => self.configuration_cache.write(&self.key, &content_envelope(&content)),
            None => self.configuration_cache.remove(&self.key),
        }
    }

    fn dispose(&self) {
        self.base.dispose();
    }
}

// ---------------------------------------------------------------------------------------------
// WorkspaceConfiguration
// ---------------------------------------------------------------------------------------------

/// `CachedWorkspaceConfiguration | FileServiceBasedWorkspaceConfiguration`.
#[derive(Clone)]
enum WorkspaceConfigurationSource {
    Cached(Rc<CachedWorkspaceConfiguration>),
    FileService(Rc<FileServiceBasedWorkspaceConfiguration>),
}

impl WorkspaceConfigurationSource {
    fn load(&self, workspace_identifier: &WorkspaceIdentifier, configuration_parse_options: &ConfigurationParseOptions) -> Promise<()> {
        match self {
            WorkspaceConfigurationSource::Cached(configuration) => configuration.load(workspace_identifier, configuration_parse_options),
            WorkspaceConfigurationSource::FileService(configuration) => configuration.load(workspace_identifier, configuration_parse_options),
        }
    }

    fn get_folders(&self) -> Vec<StoredWorkspaceFolder> {
        match self {
            WorkspaceConfigurationSource::Cached(configuration) => configuration.parser.borrow().folders(),
            WorkspaceConfigurationSource::FileService(configuration) => configuration.parser.borrow().folders(),
        }
    }

    fn is_transient(&self) -> bool {
        match self {
            WorkspaceConfigurationSource::Cached(configuration) => configuration.parser.borrow().transient(),
            WorkspaceConfigurationSource::FileService(configuration) => configuration.parser.borrow().transient(),
        }
    }

    fn get_workspace_settings(&self) -> ConfigurationModel {
        match self {
            WorkspaceConfigurationSource::Cached(configuration) => configuration.workspace_settings.borrow().clone(),
            WorkspaceConfigurationSource::FileService(configuration) => configuration.workspace_settings.borrow().clone(),
        }
    }

    fn reparse_workspace_settings(&self, configuration_parse_options: &ConfigurationParseOptions) -> ConfigurationModel {
        match self {
            WorkspaceConfigurationSource::Cached(configuration) => configuration.reparse_workspace_settings(configuration_parse_options),
            WorkspaceConfigurationSource::FileService(configuration) => configuration.reparse_workspace_settings(configuration_parse_options),
        }
    }

    fn get_restricted_settings(&self) -> Vec<JsString> {
        match self {
            WorkspaceConfigurationSource::Cached(configuration) => configuration.parser.borrow().get_restricted_workspace_settings(),
            WorkspaceConfigurationSource::FileService(configuration) => configuration.parser.borrow().get_restricted_workspace_settings(),
        }
    }
}

/// `WorkspaceConfiguration`.
pub struct WorkspaceConfiguration {
    base: Rc<Disposable>,
    configuration_cache: Rc<dyn IConfigurationCache>,
    file_service: Rc<FileService>,
    ext_uri: ExtUri,
    log: LogServiceRef,
    cached_configuration: Rc<CachedWorkspaceConfiguration>,
    workspace_configuration: RefCell<WorkspaceConfigurationSource>,
    workspace_configuration_disposables: Rc<DisposableStore>,
    workspace_identifier: RefCell<Option<WorkspaceIdentifier>>,
    is_workspace_trusted: Cell<bool>,
    on_did_update_configuration: Rc<Emitter<bool>>,
    initialized: Cell<bool>,
}

fn workspace_parse_options(is_untrusted: bool) -> ConfigurationParseOptions {
    ConfigurationParseOptions { scopes: Some(WORKSPACE_SCOPES.to_vec()), skip_restricted: is_untrusted, ..Default::default() }
}

impl WorkspaceConfiguration {
    pub fn new(configuration_cache: Rc<dyn IConfigurationCache>, file_service: Rc<FileService>, uri_identity_service: &dyn IUriIdentityService, log: LogServiceRef) -> Rc<Self> {
        let base = Disposable::new();
        let on_did_update_configuration = Emitter::new();
        base.register(on_did_update_configuration.clone());
        let workspace_configuration_disposables = DisposableStore::new();
        base.register(workspace_configuration_disposables.clone());
        let cached_configuration = Rc::new(CachedWorkspaceConfiguration::new(configuration_cache.clone(), log.clone()));
        Rc::new(Self {
            base,
            configuration_cache,
            file_service,
            ext_uri: uri_identity_service.ext_uri(),
            log,
            workspace_configuration: RefCell::new(WorkspaceConfigurationSource::Cached(cached_configuration.clone())),
            cached_configuration,
            workspace_configuration_disposables,
            workspace_identifier: RefCell::new(None),
            is_workspace_trusted: Cell::new(false),
            on_did_update_configuration,
            initialized: Cell::new(false),
        })
    }

    /// `onDidUpdateConfiguration`: o argumento diz se veio do cache.
    pub fn on_did_update_configuration(&self) -> Event<bool> {
        self.on_did_update_configuration.event()
    }

    /// `initialized`.
    pub fn initialized(&self) -> bool {
        self.initialized.get()
    }

    fn is_untrusted(&self) -> bool {
        !self.is_workspace_trusted.get()
    }

    fn new_file_service_based_configuration(&self) -> Rc<FileServiceBasedWorkspaceConfiguration> {
        FileServiceBasedWorkspaceConfiguration::new(self.file_service.clone(), self.ext_uri.clone(), self.log.clone())
    }

    /// `initialize(workspaceIdentifier, workspaceTrusted)`.
    pub fn initialize(self: &Rc<Self>, workspace_identifier: WorkspaceIdentifier, workspace_trusted: bool) -> Promise<()> {
        let this = self.clone();
        async_fn(async move {
            *this.workspace_identifier.borrow_mut() = Some(workspace_identifier.clone());
            this.is_workspace_trusted.set(workspace_trusted);
            if !this.initialized.get() {
                if this.configuration_cache.needs_caching(&workspace_identifier.config_path) {
                    *this.workspace_configuration.borrow_mut() = WorkspaceConfigurationSource::Cached(this.cached_configuration.clone());
                    let _ = this.wait_and_initialize(workspace_identifier);
                } else {
                    this.do_initialize(this.new_file_service_based_configuration());
                }
            }
            this.reload().await
        })
    }

    /// `reload()`.
    pub fn reload(&self) -> Promise<()> {
        let identifier = self.workspace_identifier.borrow().clone();
        match identifier {
            Some(identifier) => {
                let source = self.workspace_configuration.borrow().clone();
                source.load(&identifier, &workspace_parse_options(self.is_untrusted()))
            }
            None => Promise::resolved(()),
        }
    }

    /// `getFolders()`.
    pub fn get_folders(&self) -> Vec<StoredWorkspaceFolder> {
        self.workspace_configuration.borrow().get_folders()
    }

    /// `setFolders(folders, jsonEditingService)`.
    pub fn set_folders(self: &Rc<Self>, folders: &[StoredWorkspaceFolder], json_editing_service: &dyn IJsonEditingService) -> Promise<()> {
        let identifier = self.workspace_identifier.borrow().clone();
        match identifier {
            Some(identifier) => {
                let this = self.clone();
                let value = Dynamic::array(folders.iter().map(stored_workspace_folder_to_dynamic));
                json_editing_service
                    .write(&identifier.config_path, vec![JsonValue { path: vec![EditSegment::Property(JsString::from("folders"))], value }], true)
                    .then_promise(move |()| this.reload())
            }
            None => Promise::resolved(()),
        }
    }

    /// `isTransient()`.
    pub fn is_transient(&self) -> bool {
        self.workspace_configuration.borrow().is_transient()
    }

    /// `getConfiguration()`.
    pub fn get_configuration(&self) -> ConfigurationModel {
        self.workspace_configuration.borrow().get_workspace_settings()
    }

    /// `updateWorkspaceTrust(trusted)`.
    pub fn update_workspace_trust(&self, trusted: bool) -> ConfigurationModel {
        self.is_workspace_trusted.set(trusted);
        self.reparse_workspace_settings()
    }

    /// `reparseWorkspaceSettings()`.
    pub fn reparse_workspace_settings(&self) -> ConfigurationModel {
        let source = self.workspace_configuration.borrow().clone();
        source.reparse_workspace_settings(&workspace_parse_options(self.is_untrusted()));
        self.get_configuration()
    }

    /// `getRestrictedSettings()`.
    pub fn get_restricted_settings(&self) -> Vec<JsString> {
        self.workspace_configuration.borrow().get_restricted_settings()
    }

    fn wait_and_initialize(self: &Rc<Self>, workspace_identifier: WorkspaceIdentifier) -> Promise<()> {
        let this = self.clone();
        async_fn(async move {
            when_provider_registered(&workspace_identifier.config_path, &this.file_service).await?;
            let is_file_service_based = matches!(&*this.workspace_configuration.borrow(), WorkspaceConfigurationSource::FileService(_));
            if !is_file_service_based {
                let file_service_based_workspace_configuration = this.new_file_service_based_configuration();
                this.base.register(file_service_based_workspace_configuration.clone());
                file_service_based_workspace_configuration.load(&workspace_identifier, &workspace_parse_options(this.is_untrusted())).await?;
                this.do_initialize(file_service_based_workspace_configuration);
                let _ = this.on_did_workspace_configuration_change(false, true);
            }
            Ok(())
        })
    }

    fn do_initialize(self: &Rc<Self>, file_service_based_workspace_configuration: Rc<FileServiceBasedWorkspaceConfiguration>) {
        self.workspace_configuration_disposables.clear();
        *self.workspace_configuration.borrow_mut() = WorkspaceConfigurationSource::FileService(file_service_based_workspace_configuration.clone());
        self.workspace_configuration_disposables.add(file_service_based_workspace_configuration.clone());
        let weak = Rc::downgrade(self);
        self.workspace_configuration_disposables.add(file_service_based_workspace_configuration.on_did_change().listen(move |()| {
            if let Some(this) = weak.upgrade() {
                let _ = this.on_did_workspace_configuration_change(true, false);
            }
        }));
        self.initialized.set(true);
    }

    fn on_did_workspace_configuration_change(self: &Rc<Self>, reload: bool, from_cache: bool) -> Promise<()> {
        let this = self.clone();
        async_fn(async move {
            if reload {
                this.reload().await?;
            }
            let _ = this.update_cache();
            this.on_did_update_configuration.fire(from_cache);
            Ok(())
        })
    }

    fn update_cache(self: &Rc<Self>) -> Promise<()> {
        let this = self.clone();
        async_fn(async move {
            let identifier = this.workspace_identifier.borrow().clone();
            let source = this.workspace_configuration.borrow().clone();
            if let (Some(identifier), WorkspaceConfigurationSource::FileService(configuration)) = (identifier, source)
                && this.configuration_cache.needs_caching(&identifier.config_path)
            {
                let content = configuration.resolve_content(&identifier).await?;
                this.cached_configuration.update_workspace(&identifier, Some(content)).await?;
            }
            Ok(())
        })
    }
}

impl IDisposable for WorkspaceConfiguration {
    fn dispose(&self) {
        self.base.dispose();
    }
}

struct FileServiceBasedWorkspaceConfiguration {
    base: Rc<Disposable>,
    parser: RefCell<WorkspaceConfigurationModelParser>,
    workspace_settings: RefCell<ConfigurationModel>,
    workspace_identifier: Rc<RefCell<Option<WorkspaceIdentifier>>>,
    workspace_config_watcher: Rc<MutableDisposable<dyn IDisposable>>,
    on_did_change: Rc<Emitter<()>>,
    file_service: Rc<FileService>,
    log: LogServiceRef,
}

impl FileServiceBasedWorkspaceConfiguration {
    fn new(file_service: Rc<FileService>, ext_uri: ExtUri, log: LogServiceRef) -> Rc<Self> {
        let base = Disposable::new();
        let workspace_identifier: Rc<RefCell<Option<WorkspaceIdentifier>>> = Rc::new(RefCell::new(None));
        let on_did_change = Emitter::new();
        base.register(on_did_change.clone());
        let emitter = on_did_change.clone();
        let reload_configuration_scheduler = RunOnceScheduler::new(move || emitter.fire(()), 50);
        base.register(reload_configuration_scheduler.clone());

        let files_change = file_service.on_did_files_change().filter(
            {
                let identifier = workspace_identifier.clone();
                move |event: &Rc<FileChangesEvent>| identifier.borrow().as_ref().is_some_and(|identifier| event.contains(&identifier.config_path, &[]))
            },
            None,
        );
        let operation = file_service.on_did_run_operation().filter(
            {
                let identifier = workspace_identifier.clone();
                move |event: &FileOperationEvent| {
                    identifier.borrow().as_ref().is_some_and(|identifier| {
                        is_file_change_operation(event) && ext_uri.is_equal(Some(&event.resource), Some(&identifier.config_path), false)
                    })
                }
            },
            None,
        );
        base.register(any(vec![files_change.signal(), operation.signal()]).listen(move |()| reload_configuration_scheduler.schedule(None)));

        let workspace_config_watcher = MutableDisposable::<dyn IDisposable>::new();
        base.register(workspace_config_watcher.clone());
        Rc::new(Self {
            base,
            parser: RefCell::new(WorkspaceConfigurationModelParser::new("", log.clone())),
            workspace_settings: RefCell::new(ConfigurationModel::create_empty_model(log.clone())),
            workspace_identifier,
            workspace_config_watcher,
            on_did_change,
            file_service,
            log,
        })
    }

    fn on_did_change(&self) -> Event<()> {
        self.on_did_change.event()
    }

    fn resolve_content(self: &Rc<Self>, workspace_identifier: &WorkspaceIdentifier) -> Promise<String> {
        self.file_service
            .read_file(&workspace_identifier.config_path, Some(ReadFileOptions { atomic: true, ..Default::default() }), None)
            .then(|content| content.value.to_string())
    }

    /// `load(workspaceIdentifier, configurationParseOptions)`.
    fn load(self: &Rc<Self>, workspace_identifier: &WorkspaceIdentifier, configuration_parse_options: &ConfigurationParseOptions) -> Promise<()> {
        let (this, workspace_identifier, configuration_parse_options) = (self.clone(), workspace_identifier.clone(), configuration_parse_options.clone());
        async_fn(async move {
            let changed = this.workspace_identifier.borrow().as_ref().is_none_or(|current| current.id != workspace_identifier.id);
            if changed {
                *this.workspace_identifier.borrow_mut() = Some(workspace_identifier.clone());
                *this.parser.borrow_mut() = WorkspaceConfigurationModelParser::new(workspace_identifier.id.clone(), this.log.clone());
                this.workspace_config_watcher.set(Some(this.file_service.watch(&workspace_identifier.config_path, None)));
            }
            let mut contents = String::new();
            match this.resolve_content(&workspace_identifier).await {
                Ok(content) => contents = content,
                Err(error) => {
                    let exists = this.file_service.exists(&workspace_identifier.config_path).await?;
                    if exists {
                        this.log.error(&error.message);
                    }
                }
            }
            this.parser.borrow_mut().parse(Some(&contents), Some(&configuration_parse_options));
            this.consolidate();
            Ok(())
        })
    }

    fn reparse_workspace_settings(&self, configuration_parse_options: &ConfigurationParseOptions) -> ConfigurationModel {
        self.parser.borrow().reparse_workspace_settings(configuration_parse_options);
        self.consolidate();
        self.workspace_settings.borrow().clone()
    }

    fn consolidate(&self) {
        let parser = self.parser.borrow();
        *self.workspace_settings.borrow_mut() = parser.settings_model().merge(&[parser.launch_model(), parser.tasks_model()]);
    }
}

impl IDisposable for FileServiceBasedWorkspaceConfiguration {
    fn dispose(&self) {
        self.base.dispose();
    }
}

struct CachedWorkspaceConfiguration {
    configuration_cache: Rc<dyn IConfigurationCache>,
    parser: RefCell<WorkspaceConfigurationModelParser>,
    workspace_settings: RefCell<ConfigurationModel>,
    log: LogServiceRef,
}

impl CachedWorkspaceConfiguration {
    fn new(configuration_cache: Rc<dyn IConfigurationCache>, log: LogServiceRef) -> Self {
        Self {
            configuration_cache,
            parser: RefCell::new(WorkspaceConfigurationModelParser::new("", log.clone())),
            workspace_settings: RefCell::new(ConfigurationModel::create_empty_model(log.clone())),
            log,
        }
    }

    fn get_key(workspace_identifier: &WorkspaceIdentifier) -> ConfigurationKey {
        ConfigurationKey { type_: ConfigurationKeyType::Workspaces, key: workspace_identifier.id.clone() }
    }

    fn load(self: &Rc<Self>, workspace_identifier: &WorkspaceIdentifier, configuration_parse_options: &ConfigurationParseOptions) -> Promise<()> {
        let (this, key, configuration_parse_options) = (self.clone(), Self::get_key(workspace_identifier), configuration_parse_options.clone());
        async_fn(async move {
            if let Ok(contents) = this.configuration_cache.read(&key).await
                && let Some(parsed) = parse_content_envelope(&contents)
            {
                *this.parser.borrow_mut() = WorkspaceConfigurationModelParser::new(key.key.clone(), this.log.clone());
                this.parser.borrow_mut().parse(Some(&parsed), Some(&configuration_parse_options));
                this.consolidate();
            }
            Ok(())
        })
    }

    fn reparse_workspace_settings(&self, configuration_parse_options: &ConfigurationParseOptions) -> ConfigurationModel {
        self.parser.borrow().reparse_workspace_settings(configuration_parse_options);
        self.consolidate();
        self.workspace_settings.borrow().clone()
    }

    fn consolidate(&self) {
        let parser = self.parser.borrow();
        *self.workspace_settings.borrow_mut() = parser.settings_model().merge(&[parser.launch_model(), parser.tasks_model()]);
    }

    fn update_workspace(&self, workspace_identifier: &WorkspaceIdentifier, content: Option<String>) -> Promise<()> {
        let key = Self::get_key(workspace_identifier);
        match content.filter(|content| !content.is_empty()) {
            Some(content) => self.configuration_cache.write(&key, &content_envelope(&content)).catch(|_| ()),
            None => self.configuration_cache.remove(&key).catch(|_| ()),
        }
    }
}

// ---------------------------------------------------------------------------------------------
// FolderConfiguration
// ---------------------------------------------------------------------------------------------

struct CachedFolderConfiguration {
    key: ConfigurationKey,
    folder_settings_model_parser: RefCell<ConfigurationModelParser>,
    folder_settings_parse_options: RefCell<ConfigurationParseOptions>,
    stand_alone_configurations: RefCell<Vec<ConfigurationModel>>,
    configuration_model: RefCell<ConfigurationModel>,
    configuration_cache: Rc<dyn IConfigurationCache>,
    log: LogServiceRef,
}

impl CachedFolderConfiguration {
    fn new(
        folder: &URI,
        config_folder_relative_path: &str,
        configuration_parse_options: ConfigurationParseOptions,
        configuration_cache: Rc<dyn IConfigurationCache>,
        ext_uri: &ExtUri,
        log: LogServiceRef,
    ) -> Self {
        let resource = ext_uri.join_path(folder, &[config_folder_relative_path]);
        Self {
            key: ConfigurationKey { type_: ConfigurationKeyType::Folder, key: hash_hex(&resource.to_string(false).to_string_lossy()) },
            folder_settings_model_parser: RefCell::new(ConfigurationModelParser::new("CachedFolderConfiguration", log.clone())),
            folder_settings_parse_options: RefCell::new(configuration_parse_options),
            stand_alone_configurations: RefCell::new(Vec::new()),
            configuration_model: RefCell::new(ConfigurationModel::create_empty_model(log.clone())),
            configuration_cache,
            log,
        }
    }

    fn load_configuration(self: &Rc<Self>) -> Promise<ConfigurationModel> {
        let this = self.clone();
        async_fn(async move {
            if let Ok(contents) = this.configuration_cache.read(&this.key).await
                && let Some(Dynamic::Object(configuration_contents)) = json_parse(&contents).map(|parsed| parsed.get_str("content")).filter(Truthy::is_truthy)
            {
                let configuration_contents = Dynamic::Object(configuration_contents);
                for key in object_keys(&configuration_contents) {
                    let content = match configuration_contents.get(&key) {
                        Dynamic::String(content) => content.to_string_lossy(),
                        _ => continue,
                    };
                    if key == FOLDER_SETTINGS_NAME {
                        let options = this.folder_settings_parse_options.borrow().clone();
                        this.folder_settings_model_parser.borrow_mut().parse(Some(&content), Some(&options));
                    } else {
                        let mut parser = StandaloneConfigurationModelParser::new(key.to_string_lossy(), key.to_string_lossy(), this.log.clone());
                        parser.parse(Some(&content), None);
                        this.stand_alone_configurations.borrow_mut().push(parser.configuration_model());
                    }
                }
            }
            this.consolidate();
            Ok(this.configuration_model.borrow().clone())
        })
    }

    fn update_configuration(&self, settings_content: Option<String>, stand_alone_configuration_contents: Vec<(String, Option<String>)>) -> Promise<()> {
        let content = ObjectRef::new();
        if let Some(settings_content) = settings_content.filter(|content| !content.is_empty()) {
            content.set(&JsString::from(FOLDER_SETTINGS_NAME), Dynamic::string(&settings_content));
        }
        for (key, contents) in stand_alone_configuration_contents {
            if let Some(contents) = contents.filter(|contents| !contents.is_empty()) {
                content.set(&JsString::from(key.as_str()), Dynamic::string(&contents));
            }
        }
        let content = Dynamic::Object(content);
        if !object_keys(&content).is_empty() {
            let envelope = je_base::dynamic::json_stringify(&Dynamic::object([("content", content)])).expect("um objeto sempre tem JSON");
            self.configuration_cache.write(&self.key, &envelope)
        } else {
            self.configuration_cache.remove(&self.key)
        }
    }

    fn get_restricted_settings(&self) -> Vec<JsString> {
        self.folder_settings_model_parser.borrow().restricted_configurations().to_vec()
    }

    fn reparse(&self, configuration_parse_options: &ConfigurationParseOptions) -> ConfigurationModel {
        *self.folder_settings_parse_options.borrow_mut() = configuration_parse_options.clone();
        self.folder_settings_model_parser.borrow_mut().reparse(configuration_parse_options);
        self.consolidate();
        self.configuration_model.borrow().clone()
    }

    fn consolidate(&self) {
        *self.configuration_model.borrow_mut() = self.folder_settings_model_parser.borrow().configuration_model().merge(&self.stand_alone_configurations.borrow());
    }
}

/// `CachedFolderConfiguration | FileServiceBasedConfiguration`.
#[derive(Clone)]
enum FolderConfigurationSource {
    Cached(Rc<CachedFolderConfiguration>),
    FileService(Rc<FileServiceBasedConfiguration>),
}

/// `FolderConfiguration`.
pub struct FolderConfiguration {
    base: Rc<Disposable>,
    on_did_change: Rc<Emitter<()>>,
    folder_configuration: RefCell<FolderConfigurationSource>,
    scopes: Vec<ConfigurationScope>,
    configuration_folder: URI,
    cached_folder_configuration: Rc<CachedFolderConfiguration>,
    workspace_trusted: Cell<bool>,
    configuration_cache: Rc<dyn IConfigurationCache>,
    file_service: Rc<FileService>,
    ext_uri: ExtUri,
    log: LogServiceRef,
    /// `workspaceFolder`.
    pub workspace_folder: WorkspaceFolder,
}

impl FolderConfiguration {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        use_cache: bool,
        workspace_folder: WorkspaceFolder,
        config_folder_relative_path: &str,
        workbench_state: WorkbenchState,
        workspace_trusted: bool,
        file_service: Rc<FileService>,
        uri_identity_service: &dyn IUriIdentityService,
        log: LogServiceRef,
        configuration_cache: Rc<dyn IConfigurationCache>,
    ) -> Rc<Self> {
        let base = Disposable::new();
        let on_did_change = Emitter::new();
        base.register(on_did_change.clone());
        let ext_uri = uri_identity_service.ext_uri();
        let scopes = if WorkbenchState::Workspace == workbench_state { FOLDER_SCOPES.to_vec() } else { WORKSPACE_SCOPES.to_vec() };
        let configuration_folder = ext_uri.join_path(&workspace_folder.uri, &[config_folder_relative_path]);
        let cached_folder_configuration = Rc::new(CachedFolderConfiguration::new(
            &workspace_folder.uri,
            config_folder_relative_path,
            ConfigurationParseOptions { scopes: Some(scopes.clone()), skip_restricted: !workspace_trusted, ..Default::default() },
            configuration_cache.clone(),
            &ext_uri,
            log.clone(),
        ));
        let uses_cache = use_cache && configuration_cache.needs_caching(&workspace_folder.uri);
        let this = Rc::new(Self {
            base,
            on_did_change,
            folder_configuration: RefCell::new(FolderConfigurationSource::Cached(cached_folder_configuration.clone())),
            scopes,
            configuration_folder,
            cached_folder_configuration,
            workspace_trusted: Cell::new(workspace_trusted),
            configuration_cache,
            file_service,
            ext_uri,
            log,
            workspace_folder,
        });
        if uses_cache {
            let target = this.clone();
            let _ = when_provider_registered(&this.workspace_folder.uri, &this.file_service).then(move |()| {
                target.switch_to_file_service_based_configuration();
                target.on_did_folder_configuration_change();
            });
        } else {
            this.switch_to_file_service_based_configuration();
        }
        this
    }

    /// `this.folderConfiguration = this._register(this.createFileServiceBasedConfiguration(...))` mais o ouvinte de mudança.
    fn switch_to_file_service_based_configuration(self: &Rc<Self>) {
        let configuration = self.create_file_service_based_configuration();
        self.base.register(configuration.clone());
        let weak = Rc::downgrade(self);
        self.base.register(configuration.on_did_change().listen(move |()| {
            if let Some(this) = weak.upgrade() {
                this.on_did_folder_configuration_change();
            }
        }));
        *self.folder_configuration.borrow_mut() = FolderConfigurationSource::FileService(configuration);
    }

    /// `onDidChange`.
    pub fn on_did_change(&self) -> Event<()> {
        self.on_did_change.event()
    }

    /// `loadConfiguration()`.
    pub fn load_configuration(&self) -> Promise<ConfigurationModel> {
        match self.folder_configuration.borrow().clone() {
            FolderConfigurationSource::Cached(configuration) => configuration.load_configuration(),
            FolderConfigurationSource::FileService(configuration) => configuration.load_configuration(None),
        }
    }

    /// `updateWorkspaceTrust(trusted)`.
    pub fn update_workspace_trust(self: &Rc<Self>, trusted: bool) -> ConfigurationModel {
        self.workspace_trusted.set(trusted);
        self.reparse()
    }

    /// `reparse()`.
    pub fn reparse(self: &Rc<Self>) -> ConfigurationModel {
        let options = ConfigurationParseOptions { scopes: Some(self.scopes.clone()), skip_restricted: self.is_untrusted(), ..Default::default() };
        let configuration_model = match self.folder_configuration.borrow().clone() {
            FolderConfigurationSource::Cached(configuration) => configuration.reparse(&options),
            FolderConfigurationSource::FileService(configuration) => configuration.reparse(&options),
        };
        let _ = self.update_cache();
        configuration_model
    }

    /// `getRestrictedSettings()`.
    pub fn get_restricted_settings(&self) -> Vec<JsString> {
        match &*self.folder_configuration.borrow() {
            FolderConfigurationSource::Cached(configuration) => configuration.get_restricted_settings(),
            FolderConfigurationSource::FileService(configuration) => configuration.get_restricted_settings(),
        }
    }

    fn is_untrusted(&self) -> bool {
        !self.workspace_trusted.get()
    }

    fn on_did_folder_configuration_change(self: &Rc<Self>) {
        let _ = self.update_cache();
        self.on_did_change.fire(());
    }

    fn create_file_service_based_configuration(&self) -> Rc<FileServiceBasedConfiguration> {
        let settings_resource = self.ext_uri.join_path(&self.configuration_folder, &[&format!("{FOLDER_SETTINGS_NAME}.json")]);
        let stand_alone_configuration_resources: Vec<(String, URI)> = [TASKS_CONFIGURATION_KEY, LAUNCH_CONFIGURATION_KEY, MCP_CONFIGURATION_KEY]
            .iter()
            .map(|name| ((*name).to_owned(), self.ext_uri.join_path(&self.configuration_folder, &[&format!("{name}.json")])))
            .collect();
        FileServiceBasedConfiguration::new(
            self.configuration_folder.to_string(false).to_string_lossy(),
            settings_resource,
            stand_alone_configuration_resources,
            ConfigurationParseOptions { scopes: Some(self.scopes.clone()), skip_restricted: self.is_untrusted(), ..Default::default() },
            self.file_service.clone(),
            self.ext_uri.clone(),
            self.log.clone(),
        )
    }

    fn update_cache(self: &Rc<Self>) -> Promise<()> {
        let this = self.clone();
        async_fn(async move {
            let source = this.folder_configuration.borrow().clone();
            if let FolderConfigurationSource::FileService(configuration) = source
                && this.configuration_cache.needs_caching(&this.configuration_folder)
            {
                let (settings_content, stand_alone_configuration_contents) = configuration.resolve_contents(false).await?;
                let _ = this.cached_folder_configuration.update_configuration(settings_content, stand_alone_configuration_contents);
            }
            Ok(())
        })
    }

    /// `addRelated(disposable)`.
    pub fn add_related(&self, disposable: DisposableRef) {
        self.base.register(disposable);
    }
}

impl IDisposable for FolderConfiguration {
    fn dispose(&self) {
        self.base.dispose();
    }
}
