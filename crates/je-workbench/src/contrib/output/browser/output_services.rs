//! Porte de `src/vs/workbench/contrib/output/browser/outputServices.ts`: o `OutputChannel`, o `OutputViewFilters` e o
//! `OutputService` (canais, canal ativo, filtros, `showChannel`, `saveOutputAs`, canal composto, nível de log e criação dos
//! modelos de canal).
//!
//! Notas de tradução:
//! - **Injeção de dependências.** Os decoradores `@I...` dos construtores (`IStorageService`, `IInstantiationService`,
//!   `ITextModelService`, `ILogService`, `ILoggerService`, `ILifecycleService`, `IViewsService`, `IContextKeyService`,
//!   `IDefaultLogLevelsService`, `IFileDialogService`, `IFileService`, `IWorkbenchEnvironmentService`, `ILanguageService`) viram o
//!   [`OutputServiceServices`], passado ao construtor. O `IInstantiationService` que só serve para `createInstance(OutputChannel,
//!   ...)`, `createInstance(FileOutputChannelModel, ...)`, `createInstance(OutputLinkProvider)` e afins desaparece: cada tipo é
//!   construído direto, com os serviços que o original lhe injeta (o [`OutputChannelModelServices`] da fatia 1 para os modelos, e o
//!   `IWorkspaceContextService`, o `IModelService` e o `ILanguageFeaturesService` para o `OutputLinkProvider`).
//! - **`OutputViewPane`** (`outputView.ts`) não foi portado: o `IViewsService` devolve o [`IOutputViewPane`], com o que o serviço lê
//!   dele (`showChannel`). `getActiveViewWithId<OutputViewPane>` e `openView<OutputViewPane>` são `get_active_output_view_with_id` e
//!   `open_output_view`.
//! - **`OutputService implements ITextModelContentProvider`.** O registro no `ITextModelService` guarda um adaptador com referência
//!   fraca ao serviço (o [`OutputContentProvider`]), para o serviço não se prender pelo próprio registro; o `provideTextContent` é o
//!   [`OutputService::provide_text_content`].
//! - **`getChannel`.** O método do serviço devolve o [`OutputChannel`] concreto (o `OutputChannel | undefined` do original, de que o
//!   `provideTextContent` e os testes leem o `model`); o `IOutputService::get_channel` o devolve como `IOutputChannel`.
//! - **Descarte do canal.** A função de descarte que o `OutputChannel` recebe (`onDispose`) não guarda o canal: o `channel` que o
//!   original captura no fecho é aqui uma referência fraca ([`Weak`]), para o canal descartado deixar de ser alcançável (o que o
//!   teste `OutputService channel disposal` confere).
//! - **`async`/`await`.** `showChannel` e `saveOutputAs` são cadeias de [`Promise`] (`try`/`finally` é `Promise::finally`). As
//!   funções `async` que não esperam nada (`onDidRegisterChannel`, `setLevelIsDefaultContext`) rodam de forma síncrona.
//! - **`getLogLevel`.** O `loggerService.getLogLevel(curr.resource) ?? logLevel` do original usa o nível padrão quando o logger não
//!   tem nível; o `get_log_level` do porte devolve sempre um nível (o padrão, quando for o caso), então o `?? logLevel` não tem o que
//!   fazer e a leitura do nível padrão some.
//! - **`registerCompoundLogChannel(descriptors)`.** Ordena a lista recebida no lugar (`&mut [...]`), como o `Array.sort` do original, e o
//!   `a.label.localeCompare(b.label)` é o `locale_compare` do porte.
//! - **`service['_store']['_toDispose']`** do teste: [`OutputService::registrations_len`] é o tamanho do `DisposableStore` do serviço.

use std::cell::{Cell, RefCell};
use std::rc::{Rc, Weak};

use je_base::collator::locale_compare;
use je_base::date::{local_time_zone, to_local_iso_string};
use je_base::dynamic::{Dynamic, Truthy};
use je_base::errors::{self, Error};
use je_base::event::{Emitter, Event};
use je_base::lifecycle::{DisposableMap, DisposableStore, IDisposable};
use je_base::network::schemas;
use je_base::nls::localize;
use je_base::promise::{MaybePromise, Promise};
use je_base::runtime;
use je_base::uri::{URI, UriComponents};
use je_base::buffer::VSBuffer;
use je_editor::language_features_service::LanguageFeaturesService;
use je_editor::language_service::ILanguageService;
use je_editor::text_model::TextModel;
use je_platform::contextkey::contextkey::{IContextKey, IContextKeyService};
use je_platform::dialogs::dialogs::{FileFilter, IFileDialogService, SaveDialogOptions};
use je_platform::environment::environment_service::strip_log_key_separators;
use je_platform::files::file_service::FileContentSource;
use je_platform::log::{LogLevel, Logger, LoggerService, LoggerServiceLogLevelChange, log_level_to_string};
use je_platform::resources::join_path;
use je_platform::storage::storage::{StorageScope, StorageService, StorageTarget};
use je_platform::telemetry::telemetry_utils::TELEMETRY_LOG_ID;
use je_platform::workspace::IWorkspaceContextService;

use crate::contrib::output::browser::output_link_provider::OutputLinkProvider;
use crate::contrib::output::common::output_channel_model::{
    DelegatedOutputChannelModel, FileOutputChannelModel, IOutputChannelModel, MultiFileOutputChannelModel, OutputChannelModelServices,
};
use crate::services::environment::environment_service::IWorkbenchEnvironmentService;
use crate::services::lifecycle::lifecycle::ILifecycleService;
use crate::services::log::default_log_levels::IDefaultLogLevelsService;
use crate::services::output::output::{
    active_output_channel_context, context_active_file_output, context_active_log_file_output, context_active_output_level,
    context_active_output_level_is_default, context_active_output_level_settable, hide_category_filter_context, is_multi_source_output_channel_descriptor,
    is_single_source_output_channel_descriptor, output_channel_registry, show_debug_filter_context, show_error_filter_context, show_info_filter_context,
    show_trace_filter_context, show_warning_filter_context, ILogEntry, IOutputChannel, IOutputChannelDescriptor, IOutputChannelRegistry, IOutputContentSource,
    IOutputService, IOutputViewFilters, OutputChannelSource, OutputChannelUpdateMode, LOG_MIME, OUTPUT_MIME, OUTPUT_VIEW_ID,
};
use crate::services::textmodel_resolver::resolver_service::{ITextEditorModel, ITextModelContentProvider, ITextModelService};
use crate::services::views::views_service::IViewsService;

const OUTPUT_ACTIVE_CHANNEL_KEY: &str = "output.activechannel";

/// Os serviços que os decoradores `@I...` do construtor do `OutputService` injetam (ver as notas do módulo).
#[derive(Clone)]
pub struct OutputServiceServices {
    /// `IFileService`, `IModelService`, `ILogService`, `ILoggerService`, `IEditorWorkerService` e o `UndoRedoService` dos modelos de canal.
    pub channel_model: OutputChannelModelServices,
    pub storage_service: Rc<dyn StorageService>,
    pub text_model_service: Rc<dyn ITextModelService>,
    pub lifecycle_service: Rc<dyn ILifecycleService>,
    pub views_service: Rc<dyn IViewsService>,
    pub context_key_service: Rc<dyn IContextKeyService>,
    pub default_log_levels_service: Rc<dyn IDefaultLogLevelsService>,
    pub file_dialog_service: Rc<dyn IFileDialogService>,
    pub environment_service: Rc<dyn IWorkbenchEnvironmentService>,
    pub language_service: Rc<dyn ILanguageService>,
    /// O que o `OutputLinkProvider` lê.
    pub context_service: Rc<dyn IWorkspaceContextService>,
    pub language_features_service: Rc<LanguageFeaturesService>,
}

/// `OutputChannel`.
pub struct OutputChannel {
    store: Rc<DisposableStore>,
    /// `scrollLock`.
    pub scroll_lock: Cell<bool>,
    pub model: Rc<dyn IOutputChannelModel>,
    id: String,
    label: String,
    uri: URI,
    /// `outputChannelDescriptor`.
    pub output_channel_descriptor: IOutputChannelDescriptor,
}

impl OutputChannel {
    pub fn new(
        output_channel_descriptor: IOutputChannelDescriptor,
        output_location: &URI,
        output_dir_promise: Promise<()>,
        on_dispose: Rc<dyn Fn()>,
        services: &OutputServiceServices,
    ) -> Rc<Self> {
        let store = DisposableStore::new();
        let id = output_channel_descriptor.id.clone();
        let label = output_channel_descriptor.label.clone();
        let uri = URI::from(&UriComponents { scheme: Some(schemas::OUTPUT_CHANNEL.into()), path: Some(id.as_str().into()), ..UriComponents::default() }, false);
        let model = Self::create_output_channel_model(&id, &uri, &output_channel_descriptor, output_location, output_dir_promise, services);
        store.add(model.clone());
        store.add(model.on_dispose().once().listen(move |_| on_dispose()));
        Rc::new(Self { store, scroll_lock: Cell::new(false), model, id, label, uri, output_channel_descriptor })
    }

    fn create_output_channel_model(
        id: &str,
        uri: &URI,
        output_channel_descriptor: &IOutputChannelDescriptor,
        output_location: &URI,
        output_dir_promise: Promise<()>,
        services: &OutputServiceServices,
    ) -> Rc<dyn IOutputChannelModel> {
        let language = match &output_channel_descriptor.language_id {
            Some(language_id) => services.language_service.create_by_id(Some(language_id.as_str())),
            None => services.language_service.create_by_mime_type(Some(if output_channel_descriptor.log { LOG_MIME } else { OUTPUT_MIME })),
        };
        if let Some(sources) = output_channel_descriptor.multi_source() {
            return MultiFileOutputChannelModel::create(uri.clone(), language, sources.to_vec(), &services.channel_model);
        }
        if let Some(source) = output_channel_descriptor.single_source() {
            return FileOutputChannelModel::create(uri.clone(), language, source.clone(), &services.channel_model);
        }
        DelegatedOutputChannelModel::new(id, uri.clone(), language, output_location.clone(), output_dir_promise, &services.channel_model)
    }
}

impl IOutputChannel for OutputChannel {
    fn id(&self) -> String {
        self.id.clone()
    }

    fn label(&self) -> String {
        self.label.clone()
    }

    fn uri(&self) -> URI {
        self.uri.clone()
    }

    fn get_log_entries(&self) -> Vec<ILogEntry> {
        self.model.get_log_entries()
    }

    fn append(&self, output: &str) {
        self.model.append(output);
    }

    fn update(&self, mode: OutputChannelUpdateMode, till: Option<u64>) {
        self.model.update(mode, till, true);
    }

    fn clear(&self) {
        self.model.clear();
    }

    fn replace(&self, value: &str) {
        self.model.replace(value);
    }
}

impl IDisposable for OutputChannel {
    fn dispose(&self) {
        self.store.dispose();
    }
}

/// `IOutputFilterOptions`.
pub struct OutputFilterOptions {
    pub filter_history: Vec<String>,
    pub trace: bool,
    pub debug: bool,
    pub info: bool,
    pub warning: bool,
    pub error: bool,
    pub sources: String,
}

/// O texto de uma chave de contexto de texto (`''` quando não tem valor).
fn text_of(value: &Dynamic) -> String {
    match value {
        Dynamic::String(text) => text.to_string_lossy(),
        _ => String::new(),
    }
}

/// `OutputViewFilters`.
pub struct OutputViewFilters {
    store: Rc<DisposableStore>,
    on_did_change: Rc<Emitter<()>>,
    /// `filterHistory`.
    pub filter_history: RefCell<Vec<String>>,
    filter_text: RefCell<String>,
    include_patterns: RefCell<Vec<String>>,
    exclude_patterns: RefCell<Vec<String>>,
    trace: Rc<dyn IContextKey>,
    debug: Rc<dyn IContextKey>,
    info: Rc<dyn IContextKey>,
    warning: Rc<dyn IContextKey>,
    error: Rc<dyn IContextKey>,
    categories: Rc<dyn IContextKey>,
}

impl OutputViewFilters {
    pub fn new(options: OutputFilterOptions, context_key_service: &dyn IContextKeyService) -> Rc<Self> {
        let store = DisposableStore::new();
        let on_did_change = Emitter::new();
        store.add(on_did_change.clone());

        let trace = show_trace_filter_context().bind_to(context_key_service);
        trace.set(Dynamic::Bool(options.trace));

        let debug = show_debug_filter_context().bind_to(context_key_service);
        debug.set(Dynamic::Bool(options.debug));

        let info = show_info_filter_context().bind_to(context_key_service);
        info.set(Dynamic::Bool(options.info));

        let warning = show_warning_filter_context().bind_to(context_key_service);
        warning.set(Dynamic::Bool(options.warning));

        let error = show_error_filter_context().bind_to(context_key_service);
        error.set(Dynamic::Bool(options.error));

        let categories = hide_category_filter_context().bind_to(context_key_service);
        categories.set(Dynamic::string(&options.sources));

        Rc::new(Self {
            store,
            on_did_change,
            filter_history: RefCell::new(options.filter_history),
            filter_text: RefCell::new(String::new()),
            include_patterns: RefCell::new(Vec::new()),
            exclude_patterns: RefCell::new(Vec::new()),
            trace,
            debug,
            info,
            warning,
            error,
            categories,
        })
    }

    fn parse_text(filter_text: &str) -> (Vec<String>, Vec<String>) {
        let mut include_patterns = Vec::new();
        let mut exclude_patterns = Vec::new();

        // Parse patterns respecting quoted strings
        let patterns = Self::split_by_comma_respecting_quotes(filter_text);

        for pattern in patterns {
            let trimmed = pattern.trim();
            if trimmed.is_empty() {
                continue;
            }

            if let Some(negative) = trimmed.strip_prefix('!') {
                // Negative filter - remove the ! prefix
                let negative_pattern = negative.trim();
                if !negative_pattern.is_empty() {
                    exclude_patterns.push(negative_pattern.to_owned());
                }
            } else {
                include_patterns.push(trimmed.to_owned());
            }
        }

        (include_patterns, exclude_patterns)
    }

    fn split_by_comma_respecting_quotes(text: &str) -> Vec<String> {
        let mut patterns = Vec::new();
        let mut current = String::new();
        let mut in_quotes = false;
        let mut quote_char = '\0';

        for char in text.chars() {
            if !in_quotes && char == '"' {
                // Start of quoted string
                in_quotes = true;
                quote_char = char;
                current.push(char);
            } else if in_quotes && char == quote_char {
                // End of quoted string
                in_quotes = false;
                current.push(char);
            } else if !in_quotes && char == ',' {
                // Comma outside quotes - split here
                if !current.is_empty() {
                    patterns.push(std::mem::take(&mut current));
                }
                current.clear();
            } else {
                current.push(char);
            }
        }

        // Add the last pattern
        if !current.is_empty() {
            patterns.push(current);
        }

        patterns
    }

    /// `this._x.get() !== value` e, se mudou, `this._x.set(value)` e `_onDidChange.fire()`.
    fn set_flag(&self, key: &Rc<dyn IContextKey>, value: bool) {
        let value = Dynamic::Bool(value);
        if !key.get().strict_equals(&value) {
            key.set(value);
            self.on_did_change.fire(());
        }
    }
}

impl IOutputViewFilters for OutputViewFilters {
    fn on_did_change(&self) -> Event<()> {
        self.on_did_change.event()
    }

    fn text(&self) -> String {
        self.filter_text.borrow().clone()
    }

    fn set_text(&self, filter_text: &str) {
        if *self.filter_text.borrow() != filter_text {
            *self.filter_text.borrow_mut() = filter_text.to_owned();
            let (include_patterns, exclude_patterns) = Self::parse_text(filter_text);
            *self.include_patterns.borrow_mut() = include_patterns;
            *self.exclude_patterns.borrow_mut() = exclude_patterns;
            self.on_did_change.fire(());
        }
    }

    fn include_patterns(&self) -> Vec<String> {
        self.include_patterns.borrow().clone()
    }

    fn exclude_patterns(&self) -> Vec<String> {
        self.exclude_patterns.borrow().clone()
    }

    fn trace(&self) -> bool {
        self.trace.get().is_truthy()
    }

    fn set_trace(&self, trace: bool) {
        self.set_flag(&self.trace, trace);
    }

    fn debug(&self) -> bool {
        self.debug.get().is_truthy()
    }

    fn set_debug(&self, debug: bool) {
        self.set_flag(&self.debug, debug);
    }

    fn info(&self) -> bool {
        self.info.get().is_truthy()
    }

    fn set_info(&self, info: bool) {
        self.set_flag(&self.info, info);
    }

    fn warning(&self) -> bool {
        self.warning.get().is_truthy()
    }

    fn set_warning(&self, warning: bool) {
        self.set_flag(&self.warning, warning);
    }

    fn error(&self) -> bool {
        self.error.get().is_truthy()
    }

    fn set_error(&self, error: bool) {
        self.set_flag(&self.error, error);
    }

    fn categories(&self) -> String {
        let categories = self.categories.get();
        if categories.is_truthy() { text_of(&categories) } else { ",".to_owned() }
    }

    fn set_categories(&self, categories: &str) {
        self.categories.set(Dynamic::string(categories));
        self.on_did_change.fire(());
    }

    fn toggle_category(&self, category: &str) {
        let categories = self.categories();
        if self.has_category(category) {
            self.set_categories(&categories.replacen(&format!(",{category},"), ",", 1));
        } else {
            self.set_categories(&format!("{categories}{category},"));
        }
    }

    fn has_category(&self, category: &str) -> bool {
        if category == "," {
            return false;
        }
        self.categories().contains(&format!(",{category},"))
    }
}

impl IDisposable for OutputViewFilters {
    fn dispose(&self) {
        self.store.dispose();
    }
}

/// O `ITextModelContentProvider` que o `OutputService` registra de si mesmo (ver as notas do módulo).
struct OutputContentProvider {
    service: Weak<OutputService>,
}

impl ITextModelContentProvider for OutputContentProvider {
    fn provide_text_content(&self, resource: &URI) -> MaybePromise<Option<Rc<TextModel>>> {
        match self.service.upgrade() {
            Some(service) => service.provide_text_content(resource),
            None => MaybePromise::Value(None),
        }
    }
}

/// `OutputService`.
pub struct OutputService {
    me: Weak<OutputService>,
    store: Rc<DisposableStore>,
    services: OutputServiceServices,

    channels: Rc<DisposableMap<String, OutputChannel>>,
    active_channel_id_in_storage: String,
    active_channel: RefCell<Option<Rc<OutputChannel>>>,

    on_active_output_channel: Rc<Emitter<String>>,

    active_output_channel_context: Rc<dyn IContextKey>,
    active_file_output_channel_context: Rc<dyn IContextKey>,
    active_log_output_channel_context: Rc<dyn IContextKey>,
    active_output_channel_level_settable_context: Rc<dyn IContextKey>,
    active_output_channel_level_context: Rc<dyn IContextKey>,
    active_output_channel_level_is_default_context: Rc<dyn IContextKey>,

    output_location: URI,

    /// `filters`.
    pub filters: Rc<OutputViewFilters>,

    output_folder_creation_promise: RefCell<Option<Promise<()>>>,
}

impl OutputService {
    pub fn new(services: OutputServiceServices) -> Rc<Self> {
        let store = DisposableStore::new();
        let channels = DisposableMap::<String, OutputChannel>::new();
        store.add(channels.clone());

        let on_active_output_channel = Emitter::new();
        store.add(on_active_output_channel.clone());

        let active_channel_id_in_storage = services.storage_service.get(OUTPUT_ACTIVE_CHANNEL_KEY, StorageScope::Workspace, Some("")).unwrap_or_default();
        let context_key_service = services.context_key_service.clone();
        let context_key_service = &*context_key_service;
        let active_output_channel_context = active_output_channel_context().bind_to(context_key_service);
        active_output_channel_context.set(Dynamic::string(&active_channel_id_in_storage));

        let output_location = join_path(
            &services.environment_service.window_logs_path(),
            &[&format!("output_{}", strip_log_key_separators(&to_local_iso_string(runtime::date_now() as f64, &local_time_zone())))],
        );

        let filters = OutputViewFilters::new(
            OutputFilterOptions { filter_history: Vec::new(), trace: true, debug: true, info: true, warning: true, error: true, sources: String::new() },
            context_key_service,
        );
        store.add(filters.clone());

        let this = Rc::new_cyclic(|me| Self {
            me: me.clone(),
            store,
            channels,
            active_channel_id_in_storage,
            active_channel: RefCell::new(None),
            on_active_output_channel,
            active_output_channel_context,
            active_file_output_channel_context: context_active_file_output().bind_to(context_key_service),
            active_log_output_channel_context: context_active_log_file_output().bind_to(context_key_service),
            active_output_channel_level_settable_context: context_active_output_level_settable().bind_to(context_key_service),
            active_output_channel_level_context: context_active_output_level().bind_to(context_key_service),
            active_output_channel_level_is_default_context: context_active_output_level_is_default().bind_to(context_key_service),
            output_location,
            filters,
            output_folder_creation_promise: RefCell::new(None),
            services,
        });

        let active_context = this.active_output_channel_context.clone();
        this.listen(&this.on_active_output_channel.event(), move |_, channel| active_context.set(Dynamic::string(channel)));

        // Register as text model content provider for output
        this.store.add(
            this.services
                .text_model_service
                .register_text_model_content_provider(schemas::OUTPUT_CHANNEL, Rc::new(OutputContentProvider { service: this.me.clone() })),
        );
        this.store.add(OutputLinkProvider::new(
            this.services.context_service.clone(),
            this.services.channel_model.model_service.clone(),
            this.services.language_features_service.clone(),
        ));

        // Create output channels for already registered channels
        let registry = output_channel_registry();
        for channel_identifier in registry.get_channels() {
            this.on_did_register_channel(&channel_identifier.id);
        }
        this.listen(&registry.on_did_register_channel(), |this, id| this.on_did_register_channel(id));
        this.listen(&registry.on_did_update_channel_sources(), |this, channel| this.on_did_update_channel_sources(channel));
        this.listen(&registry.on_did_remove_channel(), |this, channel| this.on_did_remove_channel(channel));

        // Set active channel to first channel if not set
        if this.active_channel().is_none() {
            let channels = this.get_channel_descriptors();
            this.set_active_channel(channels.first().and_then(|descriptor| this.get_channel(&descriptor.id)));
        }

        let visibility = this.services.views_service.on_did_change_view_visibility().filter(|e| e.id == OUTPUT_VIEW_ID && e.visible, None);
        this.listen(&visibility, |this, _| {
            if let Some(active_channel) = this.active_channel() {
                if let Some(view) = this.services.views_service.get_active_output_view_with_id(OUTPUT_VIEW_ID) {
                    view.show_channel(active_channel, true);
                }
            }
        });

        this.listen(&this.services.channel_model.logger_service.on_did_change_log_level(), |this, _: &LoggerServiceLogLevelChange| {
            this.set_level_context();
            this.set_level_is_default_context();
        });
        this.listen(&this.services.default_log_levels_service.on_did_change_default_log_levels(), |this, _| {
            this.set_level_is_default_context();
        });

        this.listen(&this.services.lifecycle_service.on_did_shutdown(), |this, _| this.dispose());

        this
    }

    /// `this._register(event(handler))`, com o serviço como referência fraca no fecho.
    fn listen<T: 'static>(&self, event: &Event<T>, handler: impl Fn(&Rc<OutputService>, &T) + 'static) {
        let weak = self.me.clone();
        self.store.add(event.listen(move |value| {
            if let Some(this) = weak.upgrade() {
                handler(&this, value);
            }
        }));
    }

    /// `activeChannel`.
    fn active_channel(&self) -> Option<Rc<OutputChannel>> {
        self.active_channel.borrow().clone()
    }

    /// `service['_store']['_toDispose'].size` (ver as notas do módulo).
    pub fn registrations_len(&self) -> usize {
        self.store.len()
    }

    /// `provideTextContent(resource)`.
    pub fn provide_text_content(&self, resource: &URI) -> MaybePromise<Option<Rc<TextModel>>> {
        match self.get_channel(&resource.path().to_string_lossy()) {
            Some(channel) => MaybePromise::Promise(channel.model.load_model().then(Some)),
            None => MaybePromise::Value(None),
        }
    }

    /// `getChannel(id)`.
    pub fn get_channel(&self, id: &str) -> Option<Rc<OutputChannel>> {
        self.channels.get(&id.to_owned())
    }

    fn on_did_register_channel(&self, channel_id: &str) {
        let channel = self.create_channel(channel_id);
        self.channels.set(channel_id.to_owned(), channel.clone(), false);
        if self.active_channel().is_none() || self.active_channel_id_in_storage == channel_id {
            self.set_active_channel(Some(channel.clone()));
            self.on_active_output_channel.fire(channel_id.to_owned());
            let output_view = self.services.views_service.get_active_output_view_with_id(OUTPUT_VIEW_ID);
            if let Some(output_view) = output_view {
                output_view.show_channel(channel, true);
            }
        }
    }

    fn on_did_update_channel_sources(&self, channel: &IOutputChannelDescriptor) {
        if let Some(output_channel) = self.channels.get(&channel.id) {
            if let Some(sources) = channel.multi_source() {
                output_channel.model.update_channel_sources(sources.to_vec());
            }
        }
    }

    fn on_did_remove_channel(&self, channel: &IOutputChannelDescriptor) {
        if self.active_channel().is_some_and(|active| active.id == channel.id) {
            let channels = self.get_channel_descriptors();
            if let Some(first) = channels.first() {
                self.show_channel_by_id(&first.id, false);
            }
        }
        self.channels.delete_and_dispose(&channel.id);
    }

    fn create_channel(&self, id: &str) -> Rc<OutputChannel> {
        // O `channel` que o fecho do original captura: fraco, para o canal descartado deixar de ser alcançável.
        let channel_slot: Rc<RefCell<Weak<OutputChannel>>> = Rc::new(RefCell::new(Weak::new()));
        let weak_service = self.me.clone();
        let slot = channel_slot.clone();
        let channel_id = id.to_owned();
        let channel = self.instantiate_channel(
            id,
            Rc::new(move || {
                let Some(service) = weak_service.upgrade() else {
                    return;
                };
                let this_channel = slot.borrow().upgrade();
                let is_active = match (service.active_channel(), this_channel) {
                    (Some(active), Some(this_channel)) => Rc::ptr_eq(&active, &this_channel),
                    _ => false,
                };
                if is_active {
                    let channels = service.get_channel_descriptors();
                    let channel = if channels.is_empty() { None } else { service.get_channel(&channels[0].id) };
                    match channel {
                        Some(channel) if service.services.views_service.is_view_visible(OUTPUT_VIEW_ID) => {
                            service.show_channel_by_id(&channel.id, false);
                        }
                        _ => service.set_active_channel(None),
                    }
                }
                output_channel_registry().remove_channel(&channel_id);
            }),
        );
        *channel_slot.borrow_mut() = Rc::downgrade(&channel);
        channel
    }

    fn instantiate_channel(&self, id: &str, on_dispose: Rc<dyn Fn()>) -> Rc<OutputChannel> {
        let Some(channel_data) = output_channel_registry().get_channel(id) else {
            self.services.channel_model.log_service.error(&format!("Channel '{id}' is not registered yet"));
            errors::throw(Error::new(format!("Channel '{id}' is not registered yet")))
        };
        let promise = self
            .output_folder_creation_promise
            .borrow_mut()
            .get_or_insert_with(|| self.services.channel_model.file_service.create_folder(&self.output_location).then(|_| ()))
            .clone();
        OutputChannel::new(channel_data, &self.output_location, promise, on_dispose, &self.services)
    }

    fn set_level_context(&self) {
        let descriptor = self.active_channel().map(|channel| channel.output_channel_descriptor.clone());
        let channel_log_level = descriptor.as_ref().and_then(|descriptor| self.get_log_level(descriptor));
        self.active_output_channel_level_context.set(Dynamic::string(channel_log_level.map_or("", log_level_to_string)));
    }

    fn set_level_is_default_context(&self) {
        let descriptor = self.active_channel().map(|channel| channel.output_channel_descriptor.clone());
        let channel_log_level = descriptor.as_ref().and_then(|descriptor| self.get_log_level(descriptor));
        match channel_log_level {
            Some(channel_log_level) => {
                let channel_default_log_level = self
                    .services
                    .default_log_levels_service
                    .get_default_log_level(descriptor.as_ref().and_then(|descriptor| descriptor.extension_id.as_deref()));
                self.active_output_channel_level_is_default_context.set(Dynamic::Bool(channel_default_log_level == channel_log_level));
            }
            None => self.active_output_channel_level_is_default_context.set(Dynamic::Bool(false)),
        }
    }

    fn set_active_channel(&self, channel: Option<Rc<OutputChannel>>) {
        *self.active_channel.borrow_mut() = channel;
        let active_channel = self.active_channel();
        let descriptor = active_channel.as_ref().map(|channel| channel.output_channel_descriptor.clone());
        self.active_file_output_channel_context.set(Dynamic::Bool(descriptor.as_ref().is_some_and(is_single_source_output_channel_descriptor)));
        self.active_log_output_channel_context.set(Dynamic::Bool(descriptor.as_ref().is_some_and(|descriptor| descriptor.log)));
        self.active_output_channel_level_settable_context.set(Dynamic::Bool(descriptor.as_ref().is_some_and(|descriptor| self.can_set_log_level(descriptor))));
        self.set_level_is_default_context();
        self.set_level_context();

        match active_channel {
            Some(active_channel) => {
                self.services.storage_service.store(OUTPUT_ACTIVE_CHANNEL_KEY, active_channel.id.as_str().into(), StorageScope::Workspace, StorageTarget::Machine)
            }
            None => self.services.storage_service.remove(OUTPUT_ACTIVE_CHANNEL_KEY, StorageScope::Workspace),
        }
    }

    /// `showChannel(id, preserveFocus)`.
    fn show_channel_by_id(&self, id: &str, preserve_focus: bool) -> Promise<()> {
        let channel = self.get_channel(id);
        if self.active_channel().map(|active| active.id.clone()) != channel.as_ref().map(|channel| channel.id.clone()) {
            self.set_active_channel(channel.clone());
            self.on_active_output_channel.fire(id.to_owned());
        }
        self.services.views_service.open_output_view(OUTPUT_VIEW_ID, !preserve_focus).then(move |output_view| {
            if let (Some(output_view), Some(channel)) = (output_view, channel) {
                output_view.show_channel(channel, preserve_focus);
            }
        })
    }

    /// `getChannelDescriptors()`.
    fn get_channel_descriptors(&self) -> Vec<IOutputChannelDescriptor> {
        output_channel_registry().get_channels()
    }

    /// As fontes de um descritor de canal de log: `[channel.source]`, `channel.source` ou `[]`.
    fn sources_of(channel: &IOutputChannelDescriptor) -> Vec<IOutputContentSource> {
        match &channel.source {
            Some(OutputChannelSource::Single(source)) => vec![source.clone()],
            Some(OutputChannelSource::Multi(sources)) => sources.clone(),
            None => Vec::new(),
        }
    }

    /// O `fileDialogService.showSaveDialog` de `saveOutputAs`, quando não há caminho.
    fn pick_output_path(&self, name: String) -> Promise<Option<URI>> {
        let file_dialog_service = self.services.file_dialog_service.clone();
        file_dialog_service.default_file_path(None).then_promise(move |default_path| {
            file_dialog_service.show_save_dialog(SaveDialogOptions {
                title: Some(localize("saveLog.dialogTitle", "Save Output As", &[])),
                available_file_systems: Some(vec![schemas::FILE.to_owned()]),
                default_uri: Some(join_path(&default_path, &[&format!("{name}.log")])),
                filters: Some(vec![FileFilter { name, extensions: vec!["log".to_owned()] }]),
                ..SaveDialogOptions::default()
            })
        })
    }
}

impl IOutputService for OutputService {
    fn filters(&self) -> Rc<dyn IOutputViewFilters> {
        self.filters.clone()
    }

    fn get_channel(&self, id: &str) -> Option<Rc<dyn IOutputChannel>> {
        OutputService::get_channel(self, id).map(|channel| channel as Rc<dyn IOutputChannel>)
    }

    fn get_channel_descriptor(&self, id: &str) -> Option<IOutputChannelDescriptor> {
        output_channel_registry().get_channel(id)
    }

    fn get_channel_descriptors(&self) -> Vec<IOutputChannelDescriptor> {
        OutputService::get_channel_descriptors(self)
    }

    fn get_active_channel(&self) -> Option<Rc<dyn IOutputChannel>> {
        self.active_channel().map(|channel| channel as Rc<dyn IOutputChannel>)
    }

    fn show_channel(&self, id: &str, preserve_focus: bool) -> Promise<()> {
        self.show_channel_by_id(id, preserve_focus)
    }

    fn on_active_output_channel(&self) -> Event<String> {
        self.on_active_output_channel.event()
    }

    fn can_set_log_level(&self, channel: &IOutputChannelDescriptor) -> bool {
        channel.log && channel.id != TELEMETRY_LOG_ID
    }

    fn get_log_level(&self, channel: &IOutputChannelDescriptor) -> Option<LogLevel> {
        if !channel.log {
            return None;
        }
        let sources = Self::sources_of(channel);
        if sources.is_empty() {
            return None;
        }

        Some(sources.iter().fold(LogLevel::Error, |prev, curr| prev.min(self.services.channel_model.logger_service.get_log_level(Some(&curr.resource)))))
    }

    fn set_log_level(&self, channel: &IOutputChannelDescriptor, log_level: LogLevel) {
        if !channel.log {
            return;
        }
        let sources = Self::sources_of(channel);
        if sources.is_empty() {
            return;
        }
        for source in sources {
            self.services.channel_model.logger_service.set_log_level(&source.resource, log_level);
        }
    }

    fn register_compound_log_channel(&self, descriptors: &mut [IOutputChannelDescriptor]) -> String {
        let output_channel_registry = output_channel_registry();
        descriptors.sort_by(|a, b| locale_compare(&a.label, &b.label));
        let id = descriptors.iter().map(|descriptor| descriptor.id.to_lowercase()).collect::<Vec<_>>().join("-");
        if output_channel_registry.get_channel(&id).is_none() {
            let source = descriptors
                .iter()
                .flat_map(|descriptor| {
                    if let Some(source) = descriptor.single_source() {
                        return vec![IOutputContentSource { resource: source.resource.clone(), name: Some(source.name.clone().unwrap_or_else(|| descriptor.label.clone())) }];
                    }
                    if let Some(sources) = descriptor.multi_source() {
                        return sources.to_vec();
                    }
                    match self.get_channel(&descriptor.id) {
                        Some(channel) => channel.model.source(),
                        None => Vec::new(),
                    }
                })
                .collect();
            output_channel_registry.register_channel(IOutputChannelDescriptor {
                id: id.clone(),
                label: descriptors.iter().map(|descriptor| descriptor.label.as_str()).collect::<Vec<_>>().join(", "),
                log: descriptors.iter().any(|descriptor| descriptor.log),
                language_id: None,
                source: Some(OutputChannelSource::Multi(source)),
                extension_id: None,
                user: Some(true),
            });
        }
        id
    }

    fn save_output_as(&self, output_path: Option<URI>, mut channels: Vec<IOutputChannelDescriptor>) -> Promise<()> {
        let Some(first_channel) = channels.first() else {
            return Promise::rejected(Error::type_error("Cannot read properties of undefined (reading 'id')"));
        };
        let is_compound = channels.len() > 1;
        let channel = if is_compound {
            let compound_channel_id = self.register_compound_log_channel(&mut channels);
            OutputService::get_channel(self, &compound_channel_id)
        } else {
            OutputService::get_channel(self, &first_channel.id)
        };

        let Some(channel) = channel else {
            return Promise::resolved(());
        };

        let name = if is_compound { "output".to_owned() } else { channels[0].label.clone() };
        let uri = match output_path {
            Some(uri) => Promise::resolved(Some(uri)),
            None => self.pick_output_path(name),
        };

        let text_model_service = self.services.text_model_service.clone();
        let file_service = self.services.channel_model.file_service.clone();
        let channel_id = channel.id.clone();
        uri.then_promise(move |uri| {
            let Some(uri) = uri else {
                return Promise::resolved(());
            };
            text_model_service.create_model_reference(&channel.uri()).then_promise(move |model_ref| {
                let value = model_ref.object.text_editor_model().map(|model| model.get_value().to_string_lossy()).unwrap_or_default();
                file_service
                    .write_file(&uri, FileContentSource::from(VSBuffer::from_string(&value)), None)
                    .then(|_| ())
                    .finally(move || model_ref.dispose())
            })
        })
        .finally(move || {
            if is_compound {
                output_channel_registry().remove_channel(&channel_id);
            }
        })
    }
}

impl IDisposable for OutputService {
    fn dispose(&self) {
        self.store.dispose();
    }
}
