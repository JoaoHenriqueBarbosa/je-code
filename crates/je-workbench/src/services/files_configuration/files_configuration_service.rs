//! Porte de `src/vs/workbench/services/filesConfiguration/common/filesConfigurationService.ts`: o serviço que traduz a
//! configuração `files.*` (salvar automaticamente, associações, hot exit, somente leitura) para o resto do workbench.
//!
//! Divergências:
//! - **Injeção.** Os parâmetros decorados do construtor são a tupla `Deps` do [`Injectable`], na ordem do original.
//!   `IFilesConfigurationService` é o trait [`IFilesConfigurationService`]; a classe herda de `Disposable`, aqui um
//!   `Disposable` guardado no campo `base`. `registerSingleton(..., InstantiationType.Eager)` vira [`register`], que o
//!   chamador roda uma vez na montagem do workbench.
//! - **`IFileService`** é o contrato reduzido de `je_platform::files::i_file_service` (o porte do serviço de arquivos é
//!   uma struct concreta, sem trait): o serviço só lê dele `getProvider` e `resolve` com metadados.
//! - **`EditorInput | URI`.** O `EditorInput` ainda não foi portado. O serviço só lê dele o
//!   `EditorResourceAccessor.getOriginalUri(editor, { supportSideBySide: SideBySideEditor.PRIMARY })`, que é o único
//!   método do trait [`EditorOriginalUri`]; [`ResourceOrEditor`] é a união `EditorInput | URI`. Quando o `EditorInput`
//!   for portado, ele implementa o trait e nada mais aqui muda.
//! - **Sobrecargas de `updateReadonly`.** `(resource: URI, readonly: true | IMarkdownString | false | 'toggle' |
//!   'reset')` é [`IFilesConfigurationService::update_readonly`], com [`ReadonlyUpdate`] (sem `'toggle'`) ou o
//!   [`SingleReadonlyUpdate`] (com ele); `(resource: URI[], readonly: ...)` é [`IFilesConfigurationService::update_readonly_all`].
//! - **`boolean | IMarkdownString`** de `isReadonly` e das substituições de sessão é [`Readonly`]. `IBaseFileStat` (que
//!   o serviço lê só por `locked` e `readonly`) é o [`FileStat`], que o estende.
//! - **`IAutoSaveConfiguration`** é a struct [`IAutoSaveConfiguration`] e `ICachedAutoSaveConfiguration` (que a estende
//!   com `isOutOfWorkspace` e `isShortAutoSaveDelay`) é [`CachedAutoSaveConfiguration`]; `getAutoSaveConfiguration` do
//!   trait devolve a base e [`FilesConfigurationService::get_cached_auto_save_configuration`] a estendida. As unions
//!   `IAutoSaveMode`, `AutoSaveMode` e `AutoSaveDisabledReason` são enums.
//! - **Cache de configuração por recurso.** O `LRUCache<URI, ...>` do original guarda por identidade do objeto `URI`; o
//!   `URI` aqui não tem `Hash`, então a chave é o texto dele (`uri.toString()`). A configuração calculada só depende do
//!   valor do recurso, então o resultado é o mesmo, e o cache continua zerado a cada mudança de configuração.
//! - **`readOnlyMessage`** do provider é o texto do porte (`Option<String>`) e vira o `value` de um `IMarkdownString`.
//! - **`GlobalIdleValue`** não é `IDisposable` no porte: o descarte registrado no `base` chama o `dispose` dele.
//! - **`onFilesConfigurationChange`** é protegido no original e público aqui, porque Rust não tem herança:
//!   `TestFilesConfigurationService.testOnFilesConfigurationChange` o chama.
//! - **`getValue<IFilesConfiguration>()`** devolve um [`Dynamic`]; a leitura de `configuration?.files?.x` é
//!   [`optional_get`] (um valor ausente é `Undefined`).

use std::cell::RefCell;
use std::rc::{Rc, Weak};

use je_base::JsString;
use je_base::async_::{GlobalIdleValue, async_fn};
use je_base::dynamic::{Dynamic, Truthy};
use je_base::event::{Emitter, EmitterOptions, Event};
use je_base::html_content::{IMarkdownString, IsTrusted, markdown_string_equal};
use je_base::lifecycle::{self, Disposable, DisposableRef, IDisposable, to_disposable};
use je_base::map::{LRUCache, ResourceMap};
use je_base::nls::localize;
use je_base::objects;
use je_base::platform::Platform;
use je_base::promise::Promise;
use je_base::uri::{URI, encode_uri_component};
use je_instantiation::{Injectable, InstantiationType, register_singleton, service_id, service_impl};
use je_platform::configuration::ConfigurationOverrides;
use je_platform::configuration::service::IConfigurationService;
use je_platform::contextkey::{ContextKeyMeta, IContextKey, IContextKeyService, RawContextKey};
use je_platform::environment::environment::EnvironmentService;
use je_platform::files::files::{FileStat, HotExitConfiguration, has_readonly_capability};
use je_platform::files::i_file_service::IFileService;
use je_platform::markers::{IMarkerService, MarkerReadOptions, MarkerSeverity};
use je_platform::resources::ext_uri;
use je_platform::uri_identity::uri_identity::IUriIdentityService;
use je_platform::workspace::IWorkspaceContextService;
use je_editor::text_resource_configuration_service::ITextResourceConfigurationService;

use crate::common::editor::SaveReason;
use crate::common::resources::{GetExpression, ResourceGlobMatcher, ShouldUpdate};

/// `AutoSaveAfterShortDelayContext`: `new RawContextKey<boolean>('autoSaveAfterShortDelayContext', false, true)`.
pub fn auto_save_after_short_delay_context() -> RawContextKey {
    RawContextKey::new("autoSaveAfterShortDelayContext", false, ContextKeyMeta::Hide)
}

/// `IAutoSaveConfiguration['autoSave']`: `'afterDelay' | 'onFocusChange' | 'onWindowChange'`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AutoSave {
    AfterDelay,
    OnFocusChange,
    OnWindowChange,
}

/// `IAutoSaveConfiguration`.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct IAutoSaveConfiguration {
    pub auto_save: Option<AutoSave>,
    pub auto_save_delay: Option<f64>,
    pub auto_save_workspace_files_only: Option<bool>,
    pub auto_save_when_no_errors: Option<bool>,
}

/// `ICachedAutoSaveConfiguration`: a configuração com um pouco de estado extra que o serviço guarda para reduzir a
/// quantidade de consultas, já que os métodos de salvar automaticamente são chamados com muita frequência (por exemplo
/// a cada mudança de conteúdo).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct CachedAutoSaveConfiguration {
    pub configuration: IAutoSaveConfiguration,
    pub is_out_of_workspace: Option<bool>,
    pub is_short_auto_save_delay: Option<bool>,
}

/// `AutoSaveMode`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AutoSaveMode {
    Off,
    AfterShortDelay,
    AfterLongDelay,
    OnFocusChange,
    OnWindowChange,
}

/// `AutoSaveDisabledReason`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AutoSaveDisabledReason {
    Settings = 1,
    OutOfWorkspace,
    Errors,
    Disabled,
}

/// `IAutoSaveMode`: `IEnabledAutoSaveMode | IDisabledAutoSaveMode`. O `Enabled` leva um modo diferente de
/// [`AutoSaveMode::Off`] e o `Disabled` leva o motivo (o modo dele é sempre `Off`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum IAutoSaveMode {
    Enabled(AutoSaveMode),
    Disabled(AutoSaveDisabledReason),
}

impl IAutoSaveMode {
    /// `mode`.
    pub fn mode(&self) -> AutoSaveMode {
        match self {
            IAutoSaveMode::Enabled(mode) => *mode,
            IAutoSaveMode::Disabled(_) => AutoSaveMode::Off,
        }
    }
}

/// `AutoSaveConfiguration` (de `files.ts`): os valores da configuração `files.autoSave`.
pub struct AutoSaveConfiguration;

impl AutoSaveConfiguration {
    pub const OFF: &'static str = "off";
    pub const AFTER_DELAY: &'static str = "afterDelay";
    pub const ON_FOCUS_CHANGE: &'static str = "onFocusChange";
    pub const ON_WINDOW_CHANGE: &'static str = "onWindowChange";
}

/// `FILES_READONLY_INCLUDE_CONFIG`.
pub const FILES_READONLY_INCLUDE_CONFIG: &str = "files.readonlyInclude";

/// `FILES_READONLY_EXCLUDE_CONFIG`.
pub const FILES_READONLY_EXCLUDE_CONFIG: &str = "files.readonlyExclude";

/// O recorte de `EditorInput` que o serviço lê: `EditorResourceAccessor.getOriginalUri(editor, { supportSideBySide:
/// SideBySideEditor.PRIMARY })`.
pub trait EditorOriginalUri {
    fn primary_original_uri(&self) -> Option<URI>;
}

/// `EditorInput | URI`.
#[derive(Clone)]
pub enum ResourceOrEditor {
    Resource(URI),
    Editor(Rc<dyn EditorOriginalUri>),
}

impl From<&URI> for ResourceOrEditor {
    fn from(resource: &URI) -> Self {
        ResourceOrEditor::Resource(resource.clone())
    }
}

/// `boolean | IMarkdownString`: o resultado de `isReadonly` e o valor da substituição de sessão.
#[derive(Clone, Debug)]
pub enum Readonly {
    Bool(bool),
    Markdown(IMarkdownString),
}

impl Readonly {
    /// `!!readonly`: o markdown (um objeto) é sempre verdadeiro.
    pub fn is_readonly(&self) -> bool {
        match self {
            Readonly::Bool(readonly) => *readonly,
            Readonly::Markdown(_) => true,
        }
    }
}

/// `true | IMarkdownString | false | 'reset'`: o que `updateReadonly` aceita para vários recursos (e o que
/// `applyReadonly` aplica a cada um).
#[derive(Clone, Debug)]
pub enum ReadonlyUpdate {
    True,
    Markdown(IMarkdownString),
    False,
    Reset,
}

/// `true | IMarkdownString | false | 'toggle' | 'reset'`: o que `updateReadonly` aceita para um recurso só.
#[derive(Clone, Debug)]
pub enum SingleReadonlyUpdate {
    Update(ReadonlyUpdate),
    Toggle,
}

impl From<ReadonlyUpdate> for SingleReadonlyUpdate {
    fn from(update: ReadonlyUpdate) -> Self {
        SingleReadonlyUpdate::Update(update)
    }
}

/// `IFilesConfigurationService`.
pub trait IFilesConfigurationService {
    //#region Auto Save

    fn on_did_change_auto_save_configuration(&self) -> Event<()>;

    fn on_did_change_auto_save_disabled(&self) -> Event<URI>;

    fn get_auto_save_configuration(&self, resource_or_editor: Option<&ResourceOrEditor>) -> IAutoSaveConfiguration;

    fn has_short_auto_save_delay(&self, resource_or_editor: Option<&ResourceOrEditor>) -> bool;

    fn get_auto_save_mode(&self, resource_or_editor: Option<&ResourceOrEditor>, save_reason: Option<SaveReason>) -> IAutoSaveMode;

    fn toggle_auto_save(&self) -> Promise<()>;

    fn enable_auto_save_after_short_delay(&self, resource_or_editor: &ResourceOrEditor) -> DisposableRef;

    fn disable_auto_save(&self, resource_or_editor: &ResourceOrEditor) -> DisposableRef;

    //#endregion

    //#region Configured Readonly

    fn on_did_change_readonly(&self) -> Event<()>;

    fn is_readonly(&self, resource: &URI, stat: Option<&FileStat>) -> Readonly;

    /// Passing an `IMarkdownString` marks the resource read-only and displays it as the reason instead of the default
    /// session read-only message.
    fn update_readonly(&self, resource: &URI, readonly: SingleReadonlyUpdate) -> Promise<()>;

    /// `updateReadonly(resource: URI[], readonly)`.
    fn update_readonly_all(&self, resources: &[URI], readonly: ReadonlyUpdate) -> Promise<()>;

    //#endregion

    fn on_did_change_files_association(&self) -> Event<()>;

    fn is_hot_exit_enabled(&self) -> bool;

    fn hot_exit_configuration(&self) -> Option<String>;

    fn prevent_save_conflicts(&self, resource: &URI, language: Option<&str>) -> bool;
}

service_id!(pub I_FILES_CONFIGURATION_SERVICE: dyn IFilesConfigurationService = "filesConfigurationService");

/// `FilesConfigurationService.DEFAULT_AUTO_SAVE_DELAY`.
const DEFAULT_AUTO_SAVE_DELAY: f64 = 1000.0;

/// `FilesConfigurationService.DEFAULT_AUTO_SAVE_MODE`.
fn default_auto_save_mode() -> &'static str {
    if Platform::detect().is_web { AutoSaveConfiguration::AFTER_DELAY } else { AutoSaveConfiguration::OFF }
}

/// `FilesConfigurationService.READONLY_MESSAGES.providerReadonly`.
fn provider_readonly_message() -> IMarkdownString {
    trusted_markdown(localize("providerReadonly", "Editor is read-only because the file system of the file is read-only.", &[]))
}

/// `FilesConfigurationService.READONLY_MESSAGES.sessionReadonly`.
fn session_readonly_message() -> IMarkdownString {
    trusted_markdown(localize(
        "sessionReadonly",
        "Editor is read-only because the file was set read-only in this session. [Click here](command:{0}) to set writeable.",
        &["workbench.action.files.setActiveEditorWriteableInSession"],
    ))
}

/// `FilesConfigurationService.READONLY_MESSAGES.configuredReadonly`.
fn configured_readonly_message() -> IMarkdownString {
    let settings_command = encode_uri_component(&"[\"files.readonly\"]".encode_utf16().collect::<Vec<u16>>());
    let open_settings = format!("workbench.action.openSettings?{}", String::from_utf16_lossy(&settings_command));
    trusted_markdown(localize(
        "configuredReadonly",
        "Editor is read-only because the file was set read-only via settings. [Click here](command:{0}) to configure or [toggle for this session](command:{1}).",
        &[open_settings.as_str(), "workbench.action.files.toggleActiveEditorReadonlyInSession"],
    ))
}

/// `FilesConfigurationService.READONLY_MESSAGES.fileLocked`.
fn file_locked_message() -> IMarkdownString {
    trusted_markdown(localize(
        "fileLocked",
        "Editor is read-only because of file permissions. [Click here](command:{0}) to set writeable anyway.",
        &["workbench.action.files.setActiveEditorWriteableInSession"],
    ))
}

/// `FilesConfigurationService.READONLY_MESSAGES.fileReadonly`.
fn file_readonly_message() -> IMarkdownString {
    trusted_markdown(localize("fileReadonly", "Editor is read-only because the file is read-only.", &[]))
}

/// `{ value, isTrusted: true }`.
fn trusted_markdown(value: String) -> IMarkdownString {
    IMarkdownString { value: JsString::from(value), is_trusted: Some(IsTrusted::Bool(true)), ..IMarkdownString::default() }
}

/// `value?.key`: um valor ausente (`undefined` ou `null`) lê como `undefined`.
fn optional_get(value: &Dynamic, key: &str) -> Dynamic {
    if value.is_nullish() { Dynamic::Undefined } else { value.get_str(key) }
}

/// O estado mutável do serviço que muda com a configuração.
struct ConfigurationState {
    current_global_auto_save_configuration: CachedAutoSaveConfiguration,
    current_files_association_configuration: Dynamic,
    current_hot_exit_configuration: String,
    configured_readonly_from_permissions: Option<bool>,
}

/// `FilesConfigurationService`.
pub struct FilesConfigurationService {
    this: Weak<FilesConfigurationService>,
    base: Rc<Disposable>,

    on_did_change_auto_save_configuration: Rc<Emitter<()>>,
    on_did_change_auto_save_disabled: Rc<Emitter<URI>>,
    on_did_change_files_association: Rc<Emitter<()>>,
    on_did_change_readonly: Rc<Emitter<()>>,

    state: RefCell<ConfigurationState>,

    auto_save_configuration_cache: LRUCache<String, CachedAutoSaveConfiguration>,

    /// `autoSaveAfterShortDelayOverrides` (contador por recurso).
    auto_save_after_short_delay_overrides: RefCell<ResourceMap<usize>>,
    /// `autoSaveDisabledOverrides` (contador por recurso).
    auto_save_disabled_overrides: RefCell<ResourceMap<usize>>,

    auto_save_after_short_delay_context: Rc<dyn IContextKey>,

    readonly_include_matcher: Rc<GlobalIdleValue<Rc<ResourceGlobMatcher>>>,
    readonly_exclude_matcher: Rc<GlobalIdleValue<Rc<ResourceGlobMatcher>>>,

    session_readonly_overrides: RefCell<ResourceMap<Readonly>>,

    configuration_service: Rc<dyn IConfigurationService>,
    context_service: Rc<dyn IWorkspaceContextService>,
    environment_service: Rc<dyn EnvironmentService>,
    uri_identity_service: Rc<dyn IUriIdentityService>,
    file_service: Rc<dyn IFileService>,
    marker_service: Rc<dyn IMarkerService>,
    text_resource_configuration_service: Rc<dyn ITextResourceConfigurationService>,
}

impl Injectable for FilesConfigurationService {
    type Args = ();
    type Deps = (
        Rc<dyn IContextKeyService>,
        Rc<dyn IConfigurationService>,
        Rc<dyn IWorkspaceContextService>,
        Rc<dyn EnvironmentService>,
        Rc<dyn IUriIdentityService>,
        Rc<dyn IFileService>,
        Rc<dyn IMarkerService>,
        Rc<dyn ITextResourceConfigurationService>,
    );

    fn create(
        _: (),
        (
            context_key_service,
            configuration_service,
            context_service,
            environment_service,
            uri_identity_service,
            file_service,
            marker_service,
            text_resource_configuration_service,
        ): Self::Deps,
    ) -> Rc<Self> {
        let base = Disposable::new();

        let on_did_change_auto_save_configuration = Emitter::new();
        let on_did_change_auto_save_disabled = Emitter::new();
        let on_did_change_files_association = Emitter::new();
        // increased for users with hundreds of inputs opened
        let on_did_change_readonly = Emitter::with_options(EmitterOptions {
            leak_warning_threshold: Some(500),
            leak_warning_name: Some("FilesConfigurationService._onDidChangeReadonly".to_string()),
            ..EmitterOptions::default()
        });
        base.register(on_did_change_auto_save_configuration.clone());
        base.register(on_did_change_auto_save_disabled.clone());
        base.register(on_did_change_files_association.clone());
        base.register(on_did_change_readonly.clone());

        let auto_save_after_short_delay_context = auto_save_after_short_delay_context().bind_to(context_key_service.as_ref());

        let configuration = configuration_service.get_value(None, None);
        let files = optional_get(&configuration, "files");

        let readonly_include_matcher = Self::create_readonly_matcher(
            FILES_READONLY_INCLUDE_CONFIG,
            &base,
            &on_did_change_readonly,
            &configuration_service,
            &context_service,
        );
        let readonly_exclude_matcher = Self::create_readonly_matcher(
            FILES_READONLY_EXCLUDE_CONFIG,
            &base,
            &on_did_change_readonly,
            &configuration_service,
            &context_service,
        );

        let comparison_key_service = uri_identity_service.clone();
        let session_readonly_overrides = ResourceMap::with_key_fn(Rc::new(move |resource: &URI| comparison_key_service.ext_uri().get_comparison_key(resource, false)));

        let this = Rc::new_cyclic(|this| Self {
            this: this.clone(),
            base,
            on_did_change_auto_save_configuration,
            on_did_change_auto_save_disabled,
            on_did_change_files_association,
            on_did_change_readonly,
            state: RefCell::new(ConfigurationState {
                current_global_auto_save_configuration: Self::compute_auto_save_configuration(context_service.as_ref(), None, &files),
                current_files_association_configuration: optional_get(&files, "associations"),
                current_hot_exit_configuration: Self::hot_exit_from(&files).unwrap_or(HotExitConfiguration::ON_EXIT).to_string(),
                configured_readonly_from_permissions: None,
            }),
            auto_save_configuration_cache: LRUCache::new(1000, 1.0),
            auto_save_after_short_delay_overrides: RefCell::new(ResourceMap::new()),
            auto_save_disabled_overrides: RefCell::new(ResourceMap::new()),
            auto_save_after_short_delay_context,
            readonly_include_matcher,
            readonly_exclude_matcher,
            session_readonly_overrides: RefCell::new(session_readonly_overrides),
            configuration_service,
            context_service,
            environment_service,
            uri_identity_service,
            file_service,
            marker_service,
            text_resource_configuration_service,
        });

        // O original lê o hot exit do `configuration?.files?.hotExit` com o padrão `onExit`, e logo em seguida o
        // `onFilesConfigurationChange` o recalcula pelo mesmo caminho.
        this.on_files_configuration_change(&configuration, false);

        this.register_listeners();

        this
    }
}

service_impl!(FilesConfigurationService => dyn IFilesConfigurationService, disposable);

/// `registerSingleton(IFilesConfigurationService, FilesConfigurationService, InstantiationType.Eager)`.
pub fn register() {
    register_singleton::<FilesConfigurationService>(I_FILES_CONFIGURATION_SERVICE, InstantiationType::Eager);
}

impl IDisposable for FilesConfigurationService {
    fn dispose(&self) {
        self.base.dispose();
    }
}

impl FilesConfigurationService {
    /// `this._register(new GlobalIdleValue(() => this.createReadonlyMatcher(config)))`.
    fn create_readonly_matcher(
        config: &'static str,
        base: &Rc<Disposable>,
        on_did_change_readonly: &Rc<Emitter<()>>,
        configuration_service: &Rc<dyn IConfigurationService>,
        context_service: &Rc<dyn IWorkspaceContextService>,
    ) -> Rc<GlobalIdleValue<Rc<ResourceGlobMatcher>>> {
        let (idle_base, idle_emitter, idle_configuration, idle_context) =
            (base.clone(), on_did_change_readonly.clone(), configuration_service.clone(), context_service.clone());
        let idle_value = Rc::new(GlobalIdleValue::new(move || {
            let expression_configuration = idle_configuration.clone();
            let get_expression: GetExpression = Rc::new(move |resource: Option<&URI>| {
                let overrides = ConfigurationOverrides { resource: resource.cloned(), ..ConfigurationOverrides::default() };
                let expression = expression_configuration.get_value(Some(config), Some(&overrides));
                expression.is_truthy().then_some(expression)
            });
            let should_update: ShouldUpdate = Rc::new(move |event| event.affects_configuration(config, None));

            let matcher = ResourceGlobMatcher::new(get_expression, should_update, idle_context, idle_configuration);
            idle_base.register(matcher.clone());

            idle_base.register(matcher.on_expression_change().listen(move |_| idle_emitter.fire(())));

            matcher
        }));

        let registered = idle_value.clone();
        base.register(to_disposable(move || registered.dispose()));

        idle_value
    }

    fn register_listeners(&self) {
        // Files configuration changes
        let weak = self.this.clone();
        self.base.register(self.configuration_service.on_did_change_configuration().listen(move |e| {
            if e.affects_configuration("files", None)
                && let Some(this) = weak.upgrade()
            {
                this.on_files_configuration_change(&this.configuration_service.get_value(None, None), true);
            }
        }));
    }

    /// `hotExit` lido da configuração, quando é um dos dois valores que mudam o padrão.
    fn hot_exit_from(files: &Dynamic) -> Option<&'static str> {
        match optional_get(files, "hotExit") {
            Dynamic::String(mode) if mode.to_string_lossy() == HotExitConfiguration::OFF => Some(HotExitConfiguration::OFF),
            Dynamic::String(mode) if mode.to_string_lossy() == HotExitConfiguration::ON_EXIT_AND_WINDOW_CLOSE => Some(HotExitConfiguration::ON_EXIT_AND_WINDOW_CLOSE),
            _ => None,
        }
    }

    /// `onFilesConfigurationChange(configuration, fromEvent)`.
    pub fn on_files_configuration_change(&self, configuration: &Dynamic, from_event: bool) {
        let files = optional_get(configuration, "files");

        // Auto Save
        let global_auto_save_configuration = Self::compute_auto_save_configuration(self.context_service.as_ref(), None, &files);
        self.state.borrow_mut().current_global_auto_save_configuration = global_auto_save_configuration;
        self.auto_save_configuration_cache.clear();
        self.auto_save_after_short_delay_context.set(Dynamic::Bool(self.get_auto_save_mode(None, None).mode() == AutoSaveMode::AfterShortDelay));
        if from_event {
            self.on_did_change_auto_save_configuration.fire(());
        }

        // Check for change in files association
        let files_association = optional_get(&files, "associations");
        let association_changed = !objects::equals(&self.state.borrow().current_files_association_configuration, &files_association);
        if association_changed {
            self.state.borrow_mut().current_files_association_configuration = files_association;
            if from_event {
                self.on_did_change_files_association.fire(());
            }
        }

        // Hot exit
        self.state.borrow_mut().current_hot_exit_configuration = Self::hot_exit_from(&files).unwrap_or(HotExitConfiguration::ON_EXIT).to_string();

        // Readonly
        let readonly_from_permissions = optional_get(&files, "readonlyFromPermissions").is_truthy();
        let permissions_changed = readonly_from_permissions != self.state.borrow().configured_readonly_from_permissions.unwrap_or(false);
        if permissions_changed {
            self.state.borrow_mut().configured_readonly_from_permissions = Some(readonly_from_permissions);
            if from_event {
                self.on_did_change_readonly.fire(());
            }
        }
    }

    /// `getAutoSaveConfiguration` com o tipo que a classe devolve (`ICachedAutoSaveConfiguration`).
    pub fn get_cached_auto_save_configuration(&self, resource_or_editor: Option<&ResourceOrEditor>) -> CachedAutoSaveConfiguration {
        let Some(resource) = Self::to_resource(resource_or_editor) else {
            return self.state.borrow().current_global_auto_save_configuration.clone();
        };

        let cache_key = resource.to_string(false).to_string_lossy();
        if let Some(resource_auto_save_configuration) = self.auto_save_configuration_cache.get(&cache_key) {
            return resource_auto_save_configuration;
        }

        let files = self.text_resource_configuration_service.get_value(Some(&resource), Some("files"));
        let resource_auto_save_configuration = Self::compute_auto_save_configuration(self.context_service.as_ref(), Some(&resource), &files);
        self.auto_save_configuration_cache.set(cache_key, resource_auto_save_configuration.clone());

        resource_auto_save_configuration
    }

    /// `computeAutoSaveConfiguration(resource, filesConfiguration)`.
    fn compute_auto_save_configuration(context_service: &dyn IWorkspaceContextService, resource: Option<&URI>, files_configuration: &Dynamic) -> CachedAutoSaveConfiguration {
        let mut auto_save = None;
        let mut auto_save_delay = None;
        let mut auto_save_workspace_files_only = None;
        let mut auto_save_when_no_errors = None;

        let mut is_out_of_workspace = None;
        let mut is_short_auto_save_delay = None;

        let configured_auto_save = match optional_get(files_configuration, "autoSave") {
            Dynamic::Undefined | Dynamic::Null => default_auto_save_mode().to_string(),
            Dynamic::String(mode) => mode.to_string_lossy(),
            _ => String::new(),
        };
        match configured_auto_save.as_str() {
            AutoSaveConfiguration::AFTER_DELAY => {
                auto_save = Some(AutoSave::AfterDelay);
                let delay = match optional_get(files_configuration, "autoSaveDelay") {
                    Dynamic::Number(delay) if delay >= 0.0 => delay,
                    _ => DEFAULT_AUTO_SAVE_DELAY,
                };
                auto_save_delay = Some(delay);
                is_short_auto_save_delay = Some(delay <= DEFAULT_AUTO_SAVE_DELAY);
            }

            AutoSaveConfiguration::ON_FOCUS_CHANGE => auto_save = Some(AutoSave::OnFocusChange),

            AutoSaveConfiguration::ON_WINDOW_CHANGE => auto_save = Some(AutoSave::OnWindowChange),

            _ => {}
        }

        if matches!(optional_get(files_configuration, "autoSaveWorkspaceFilesOnly"), Dynamic::Bool(true)) {
            auto_save_workspace_files_only = Some(true);

            if let Some(resource) = resource
                && !context_service.is_inside_workspace(resource)
            {
                is_out_of_workspace = Some(true);
                is_short_auto_save_delay = None; // out of workspace file are not auto saved with this configuration
            }
        }

        if matches!(optional_get(files_configuration, "autoSaveWhenNoErrors"), Dynamic::Bool(true)) {
            auto_save_when_no_errors = Some(true);
            is_short_auto_save_delay = None; // this configuration disables short auto save delay
        }

        CachedAutoSaveConfiguration {
            configuration: IAutoSaveConfiguration { auto_save, auto_save_delay, auto_save_workspace_files_only, auto_save_when_no_errors },
            is_out_of_workspace,
            is_short_auto_save_delay,
        }
    }

    /// `toResource(resourceOrEditor)`.
    fn to_resource(resource_or_editor: Option<&ResourceOrEditor>) -> Option<URI> {
        match resource_or_editor {
            Some(ResourceOrEditor::Editor(editor)) => editor.primary_original_uri(),
            Some(ResourceOrEditor::Resource(resource)) => Some(resource.clone()),
            None => None,
        }
    }

    /// `applyReadonly(resource, readonly)`: se algo mudou.
    fn apply_readonly(&self, resource: &URI, readonly: &ReadonlyUpdate) -> bool {
        let new_value = match readonly {
            ReadonlyUpdate::Reset => return self.session_readonly_overrides.borrow_mut().delete(resource),
            ReadonlyUpdate::True => Readonly::Bool(true),
            ReadonlyUpdate::False => Readonly::Bool(false),
            ReadonlyUpdate::Markdown(markdown) => Readonly::Markdown(markdown.clone()),
        };

        let current = self.session_readonly_overrides.borrow().get(resource).cloned();
        let unchanged = match (&current, &new_value) {
            (Some(Readonly::Bool(current)), Readonly::Bool(new)) => current == new,
            (Some(Readonly::Markdown(current)), Readonly::Markdown(new)) => markdown_string_equal(current, new, &ext_uri()),
            _ => false,
        };
        if unchanged {
            return false;
        }

        self.session_readonly_overrides.borrow_mut().set(resource.clone(), new_value);
        true
    }
}

impl IFilesConfigurationService for FilesConfigurationService {
    fn on_did_change_auto_save_configuration(&self) -> Event<()> {
        self.on_did_change_auto_save_configuration.event()
    }

    fn on_did_change_auto_save_disabled(&self) -> Event<URI> {
        self.on_did_change_auto_save_disabled.event()
    }

    fn on_did_change_files_association(&self) -> Event<()> {
        self.on_did_change_files_association.event()
    }

    fn on_did_change_readonly(&self) -> Event<()> {
        self.on_did_change_readonly.event()
    }

    fn is_readonly(&self, resource: &URI, stat: Option<&FileStat>) -> Readonly {
        // if the entire file system provider is readonly, we respect that
        // and do not allow to change readonly. we take this as a hint that
        // the provider has no capabilities of writing.
        let provider = self.file_service.get_provider(&resource.scheme().to_string_lossy());
        if let Some(provider) = provider
            && has_readonly_capability(provider.as_ref())
        {
            return Readonly::Markdown(provider.read_only_message().map_or_else(provider_readonly_message, trusted_markdown));
        }

        // session override always wins over the others
        let session_readonly_override = self.session_readonly_overrides.borrow().get(resource).cloned();
        match session_readonly_override {
            Some(Readonly::Bool(true)) => return Readonly::Markdown(session_readonly_message()),
            Some(Readonly::Bool(false)) => return Readonly::Bool(false),
            Some(Readonly::Markdown(markdown)) => return Readonly::Markdown(markdown),
            None => {}
        }

        let uri_identity_ext_uri = self.uri_identity_service.ext_uri();
        if uri_identity_ext_uri.is_equal_or_parent(resource, &self.environment_service.user_roaming_data_home(), false, false)
            || uri_identity_ext_uri.is_equal(Some(resource), self.context_service.get_workspace().configuration().as_ref(), false)
        {
            return Readonly::Bool(false); // explicitly exclude some paths from readonly that we need for configuration
        }

        // configured glob patterns win over stat information
        if self.readonly_include_matcher.value().matches(resource, None) {
            return if !self.readonly_exclude_matcher.value().matches(resource, None) { Readonly::Markdown(configured_readonly_message()) } else { Readonly::Bool(false) };
        }

        // check if file is locked and configured to treat as readonly
        if self.state.borrow().configured_readonly_from_permissions == Some(true) && stat.is_some_and(|stat| stat.locked == Some(true)) {
            return Readonly::Markdown(file_locked_message());
        }

        // check if file is marked readonly from the file system provider
        if stat.is_some_and(|stat| stat.readonly == Some(true)) {
            return Readonly::Markdown(file_readonly_message());
        }

        Readonly::Bool(false)
    }

    fn update_readonly(&self, resource: &URI, readonly: SingleReadonlyUpdate) -> Promise<()> {
        let update = match readonly {
            SingleReadonlyUpdate::Update(update) => update,
            SingleReadonlyUpdate::Toggle => {
                let Some(this) = self.this.upgrade() else {
                    return Promise::resolved(());
                };
                let resource = resource.clone();
                return async_fn(async move {
                    let stat = this.file_service.resolve_with_metadata(&resource, None).await.ok(); // ignore errors: without a stat the file is not readonly by stat

                    let update = if this.is_readonly(&resource, stat.as_deref()).is_readonly() { ReadonlyUpdate::False } else { ReadonlyUpdate::True };
                    if this.apply_readonly(&resource, &update) {
                        this.on_did_change_readonly.fire(());
                    }
                    Ok(())
                });
            }
        };

        if self.apply_readonly(resource, &update) {
            self.on_did_change_readonly.fire(());
        }
        Promise::resolved(())
    }

    fn update_readonly_all(&self, resources: &[URI], readonly: ReadonlyUpdate) -> Promise<()> {
        let mut changed = false;
        for resource in resources {
            changed = self.apply_readonly(resource, &readonly) || changed;
        }
        if changed {
            self.on_did_change_readonly.fire(());
        }
        Promise::resolved(())
    }

    fn get_auto_save_configuration(&self, resource_or_editor: Option<&ResourceOrEditor>) -> IAutoSaveConfiguration {
        self.get_cached_auto_save_configuration(resource_or_editor).configuration
    }

    fn has_short_auto_save_delay(&self, resource_or_editor: Option<&ResourceOrEditor>) -> bool {
        let resource = Self::to_resource(resource_or_editor);

        if resource.as_ref().is_some_and(|resource| self.auto_save_after_short_delay_overrides.borrow().has(resource)) {
            return true; // overridden to be enabled after short delay
        }

        let resource_as_input = resource.as_ref().map(ResourceOrEditor::from);
        if self.get_cached_auto_save_configuration(resource_as_input.as_ref()).is_short_auto_save_delay == Some(true) {
            return resource.as_ref().is_none_or(|resource| !self.auto_save_disabled_overrides.borrow().has(resource));
        }

        false
    }

    fn get_auto_save_mode(&self, resource_or_editor: Option<&ResourceOrEditor>, save_reason: Option<SaveReason>) -> IAutoSaveMode {
        let resource = Self::to_resource(resource_or_editor);
        if resource.as_ref().is_some_and(|resource| self.auto_save_after_short_delay_overrides.borrow().has(resource)) {
            return IAutoSaveMode::Enabled(AutoSaveMode::AfterShortDelay); // overridden to be enabled after short delay
        }

        if resource.as_ref().is_some_and(|resource| self.auto_save_disabled_overrides.borrow().has(resource)) {
            return IAutoSaveMode::Disabled(AutoSaveDisabledReason::Disabled);
        }

        let resource_as_input = resource.as_ref().map(ResourceOrEditor::from);
        let auto_save_configuration = self.get_cached_auto_save_configuration(resource_as_input.as_ref());
        let Some(auto_save) = auto_save_configuration.configuration.auto_save else {
            return IAutoSaveMode::Disabled(AutoSaveDisabledReason::Settings);
        };

        if let Some(save_reason) = save_reason
            && ((auto_save == AutoSave::AfterDelay && save_reason != SaveReason::Auto)
                || (auto_save == AutoSave::OnFocusChange && save_reason != SaveReason::FocusChange && save_reason != SaveReason::WindowChange)
                || (auto_save == AutoSave::OnWindowChange && save_reason != SaveReason::WindowChange))
        {
            return IAutoSaveMode::Disabled(AutoSaveDisabledReason::Settings);
        }

        if let Some(resource) = &resource {
            if auto_save_configuration.configuration.auto_save_workspace_files_only == Some(true) && auto_save_configuration.is_out_of_workspace == Some(true) {
                return IAutoSaveMode::Disabled(AutoSaveDisabledReason::OutOfWorkspace);
            }

            if auto_save_configuration.configuration.auto_save_when_no_errors == Some(true)
                && !self
                    .marker_service
                    .read(Some(&MarkerReadOptions { resource: Some(resource.clone()), take: Some(1), severities: Some(MarkerSeverity::ERROR.0), ..MarkerReadOptions::default() }))
                    .is_empty()
            {
                return IAutoSaveMode::Disabled(AutoSaveDisabledReason::Errors);
            }
        }

        match auto_save {
            AutoSave::AfterDelay => {
                if auto_save_configuration.configuration.auto_save_delay.is_some_and(|delay| delay <= DEFAULT_AUTO_SAVE_DELAY) {
                    // Explicitly mark auto save configurations as long running
                    // if they are configured to not run when there are errors.
                    // The rationale here is that errors may come in after auto
                    // save has been scheduled and then further delay the auto
                    // save until resolved.
                    return IAutoSaveMode::Enabled(if auto_save_configuration.configuration.auto_save_when_no_errors == Some(true) {
                        AutoSaveMode::AfterLongDelay
                    } else {
                        AutoSaveMode::AfterShortDelay
                    });
                }
                IAutoSaveMode::Enabled(AutoSaveMode::AfterLongDelay)
            }
            AutoSave::OnFocusChange => IAutoSaveMode::Enabled(AutoSaveMode::OnFocusChange),
            AutoSave::OnWindowChange => IAutoSaveMode::Enabled(AutoSaveMode::OnWindowChange),
        }
    }

    fn toggle_auto_save(&self) -> Promise<()> {
        let current_setting = self.configuration_service.get_value(Some("files.autoSave"), None);

        let auto_save_enabled = [AutoSaveConfiguration::AFTER_DELAY, AutoSaveConfiguration::ON_FOCUS_CHANGE, AutoSaveConfiguration::ON_WINDOW_CHANGE]
            .iter()
            .any(|setting| matches!(&current_setting, Dynamic::String(current) if current.to_string_lossy() == *setting));
        let new_auto_save_value = if auto_save_enabled { AutoSaveConfiguration::OFF } else { AutoSaveConfiguration::AFTER_DELAY };

        self.configuration_service.update_value("files.autoSave", Dynamic::from(new_auto_save_value), None, None)
    }

    fn enable_auto_save_after_short_delay(&self, resource_or_editor: &ResourceOrEditor) -> DisposableRef {
        let Some(resource) = Self::to_resource(Some(resource_or_editor)) else {
            return lifecycle::none();
        };

        let counter = self.auto_save_after_short_delay_overrides.borrow().get(&resource).copied().unwrap_or(0);
        self.auto_save_after_short_delay_overrides.borrow_mut().set(resource.clone(), counter + 1);

        let this = self.this.clone();
        to_disposable(move || {
            let Some(this) = this.upgrade() else {
                return;
            };
            let mut overrides = this.auto_save_after_short_delay_overrides.borrow_mut();
            let counter = overrides.get(&resource).copied().unwrap_or(0);
            if counter <= 1 {
                overrides.delete(&resource);
            } else {
                overrides.set(resource, counter - 1);
            }
        })
    }

    fn disable_auto_save(&self, resource_or_editor: &ResourceOrEditor) -> DisposableRef {
        let Some(resource) = Self::to_resource(Some(resource_or_editor)) else {
            return lifecycle::none();
        };

        let counter = self.auto_save_disabled_overrides.borrow().get(&resource).copied().unwrap_or(0);
        self.auto_save_disabled_overrides.borrow_mut().set(resource.clone(), counter + 1);

        if counter == 0 {
            self.on_did_change_auto_save_disabled.fire(resource.clone());
        }

        let this = self.this.clone();
        to_disposable(move || {
            let Some(this) = this.upgrade() else {
                return;
            };
            let counter = this.auto_save_disabled_overrides.borrow().get(&resource).copied().unwrap_or(0);
            if counter <= 1 {
                this.auto_save_disabled_overrides.borrow_mut().delete(&resource);
                this.on_did_change_auto_save_disabled.fire(resource);
            } else {
                this.auto_save_disabled_overrides.borrow_mut().set(resource, counter - 1);
            }
        })
    }

    fn is_hot_exit_enabled(&self) -> bool {
        if self.context_service.get_workspace().transient() {
            // Transient workspace: hot exit is disabled because
            // transient workspaces are not restored upon restart
            return false;
        }

        self.state.borrow().current_hot_exit_configuration != HotExitConfiguration::OFF
    }

    fn hot_exit_configuration(&self) -> Option<String> {
        Some(self.state.borrow().current_hot_exit_configuration.clone())
    }

    fn prevent_save_conflicts(&self, resource: &URI, language: Option<&str>) -> bool {
        let overrides = ConfigurationOverrides { resource: Some(resource.clone()), override_identifier: language.map(JsString::from) };
        !matches!(
            self.configuration_service.get_value(Some("files.saveConflictResolution"), Some(&overrides)),
            Dynamic::String(resolution) if resolution.to_string_lossy() == "overwriteFileOnDisk"
        )
    }
}
