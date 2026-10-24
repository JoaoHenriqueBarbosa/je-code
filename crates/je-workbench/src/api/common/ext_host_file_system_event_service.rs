//! Porte de `src/vs/workbench/api/common/extHostFileSystemEventService.ts`: os observadores de arquivo
//! (`createFileSystemWatcher`), os eventos `onDid*File` e os participantes `onWill*File` (com as `WorkspaceEdit`s que
//! eles devolvem).
//!
//! Divergências:
//! - `IExtHostWorkspace` e `ExtHostConfigProvider` do `createFileSystemWatcher` são os traits [`FileWatcherWorkspace`]
//!   (só o `getWorkspaceFolder`) e [`FileWatcherConfiguration`] (só a leitura de `files.watcherExclude`), que
//!   [`ExtHostWorkspace`] e [`ExtHostConfigProvider`] implementam: é só o que o observador usa dos dois, e permite
//!   o dublê que a suíte do original monta com `Pick<>` e `as unknown as`.
//! - `Disposable.from(...)` do observador é um [`DisposableStore`]; `Math.random()` do `session` é um sorteio em
//!   `[0, 1)` feito com o `RandomState` da biblioteca padrão.
//! - Os eventos são `Rc<FileSystemEvents>` já revividos (`UriComponents` chega como [`URI`]); o `Lazy` de cada lista
//!   continua, e só revive e minusculiza o caminho quando alguém lê a lista.
//! - `waitUntil(thenable)` do original aceita uma promessa de qualquer valor e o serviço só guarda as
//!   `WorkspaceEdit`s; aqui o participante usa [`wait_until_edit`] com uma promessa de `Option<WorkspaceEdit>` (o `None`
//!   é "qualquer outro valor", que o original ignora). `IExtensionListener.extension` é o registro de ouvintes de
//!   [`WillEmitter`], e `Object.freeze` dos eventos é o `Clone` dos dados.
//! - `typeConverter.WorkspaceEdit.from` ainda não existe no porte de `extHostTypeConverters` e o seu ramo de notebook
//!   precisa de `NotebookCellData.from`, também ausente: [`workspace_edit_from`] converte as edições de arquivo, de
//!   texto e de snippet, e as edições de célula de notebook (`Cell` e `CellReplace`) falham com erro explícito em vez de
//!   sumir em silêncio. O `contents` de uma operação de arquivo só aceita bytes (o ramo `DataTransferFile` do original
//!   não existe em `FileOperationOptions`).

use std::cell::RefCell;
use std::collections::hash_map::RandomState;
use std::hash::{BuildHasher, Hasher};
use std::borrow::Cow;
use std::rc::Rc;
use std::time::Instant;

use je_base::async_::async_fn;
use je_base::buffer::{VSBuffer, encode_base64};
use je_base::cancellation::CancellationToken;
use je_base::dynamic::{Dynamic, Truthy};
use je_base::errors::Error;
use je_base::event::{AsyncEmitter, Emitter, Event, Listener, PromiseJoin, WaitUntilEvent};
use je_base::glob::{self, GLOB_SPLIT, GLOBSTAR, GlobOptions, ParsedPattern, PatternItem};
use je_base::lazy::Lazy;
use je_base::lifecycle::{DisposableRef, DisposableStore, IDisposable, to_disposable};
use je_base::network::schemas;
use je_base::map::ResourceSet;
use je_base::promise::Promise;
use je_base::uri::URI;
use je_extensions::scanner::ExtensionDescription;
use je_platform::files::files::{FileChangeFilter, FileOperation, FileSystemProviderCapabilities, WatchOptions};
use je_platform::files::watcher::normalize_watcher_pattern;
use je_platform::log::{LogArg, LogMessage, LogServiceRef};
use je_platform::resources::{fs_path, host_platform};

use super::ext_host_configuration::{ConfigurationScopeArg, ExtHostConfigProvider};
use super::ext_host_documents_and_editors::ExtHostDocumentsAndEditors;
use super::ext_host_file_system_info::ExtHostFileSystemInfo;
use super::ext_host_protocol::{
    ExtHostFileSystemEventServiceShape, FileSystemEvents, IMainContext, IWillRunFileOperationParticipation, IWorkspaceEditDto, IWorkspaceFileEditDto, MainContext,
    SourceTargetPair, WorkspaceEditEntryDto, WorkspaceFileEditContentsDto, WorkspaceFileEditOptionsDto,
};
use super::ext_host_type_converters::{Range as RangeConverter, TextEdit as TextEditConverter};
use super::ext_host_types::{IconPath, WorkspaceEdit, WorkspaceEditEntry, WorkspaceEditEntryMetadata};
use super::ext_host_workspace::{ExtHostWorkspace, GlobPattern, GlobPatternDto, IExtHostFileSystemInfo, WorkspaceFolder, glob_pattern_from};
use je_editor::code_action::languages::{IWorkspaceTextEdit, IconPath as EditorIconPath, TextEdit as EditorTextEdit, WorkspaceEditMetadata};

// ---- o que o observador usa do workspace e da configuração ------------------------------------------------

/// `Pick<IExtHostWorkspace, 'getWorkspaceFolder'>`.
pub trait FileWatcherWorkspace {
    fn get_workspace_folder(&self, uri: &URI) -> Option<Rc<WorkspaceFolder>>;
}

impl FileWatcherWorkspace for ExtHostWorkspace {
    fn get_workspace_folder(&self, uri: &URI) -> Option<Rc<WorkspaceFolder>> {
        ExtHostWorkspace::get_workspace_folder(self, uri, None)
    }
}

/// `configuration.getConfiguration('files', workspaceFolder).get<IGlobPatterns>('watcherExclude')`.
pub trait FileWatcherConfiguration {
    fn get_files_watcher_exclude(&self, workspace_folder: Option<&Rc<WorkspaceFolder>>) -> Dynamic;
}

impl FileWatcherConfiguration for ExtHostConfigProvider {
    fn get_files_watcher_exclude(&self, workspace_folder: Option<&Rc<WorkspaceFolder>>) -> Dynamic {
        let scope = workspace_folder.map(|folder| ConfigurationScopeArg::WorkspaceFolder(folder.clone()));
        self.get_configuration(Some("files"), scope.as_ref(), None).get("watcherExclude", None)
    }
}

// ---- eventos de arquivo ----------------------------------------------------------------------------------------

/// `FileSystemWatcherCreateOptions`.
#[derive(Clone, Copy, Debug, Default)]
pub struct FileSystemWatcherCreateOptions {
    pub ignore_create_events: Option<bool>,
    pub ignore_change_events: Option<bool>,
    pub ignore_delete_events: Option<bool>,
}

/// `RevivedFileSystemEvent`.
#[derive(Clone, Debug)]
pub struct RevivedFileSystemEvent {
    pub uri: URI,
    pub lower_case_fs_path: String,
}

/// `LazyRevivedFileSystemEvents`.
pub struct LazyRevivedFileSystemEvents {
    pub session: Option<f64>,
    created: Lazy<Rc<Vec<RevivedFileSystemEvent>>>,
    changed: Lazy<Rc<Vec<RevivedFileSystemEvent>>>,
    deleted: Lazy<Rc<Vec<RevivedFileSystemEvent>>>,
}

impl LazyRevivedFileSystemEvents {
    fn revive(uri: URI) -> RevivedFileSystemEvent {
        let lower_case_fs_path = uri.fs_path(host_platform()).to_lower_case().to_string_lossy();
        RevivedFileSystemEvent { uri, lower_case_fs_path }
    }

    fn lazy(uris: Vec<URI>) -> Lazy<Rc<Vec<RevivedFileSystemEvent>>> {
        Lazy::new(move || Rc::new(uris.into_iter().map(Self::revive).collect()))
    }

    pub fn new(events: FileSystemEvents) -> LazyRevivedFileSystemEvents {
        LazyRevivedFileSystemEvents {
            session: events.session,
            created: Self::lazy(events.created),
            changed: Self::lazy(events.changed),
            deleted: Self::lazy(events.deleted),
        }
    }

    pub fn created(&self) -> Rc<Vec<RevivedFileSystemEvent>> {
        self.created.value()
    }

    pub fn changed(&self) -> Rc<Vec<RevivedFileSystemEvent>> {
        self.changed.value()
    }

    pub fn deleted(&self) -> Rc<Vec<RevivedFileSystemEvent>> {
        self.deleted.value()
    }
}

// ---- FileSystemWatcher ----------------------------------------------------------------------------------------

/// `FileSystemWatcher`.
pub struct FileSystemWatcher {
    on_did_create: Rc<Emitter<URI>>,
    on_did_change: Rc<Emitter<URI>>,
    on_did_delete: Rc<Emitter<URI>>,
    config: u8,
    disposable: Rc<DisposableStore>,
}

impl FileSystemWatcher {
    #[allow(clippy::too_many_arguments)]
    fn new(
        main_context: &dyn IMainContext,
        configuration: &dyn FileWatcherConfiguration,
        file_system_info: &ExtHostFileSystemInfo,
        workspace: Rc<dyn FileWatcherWorkspace>,
        extension: &ExtensionDescription,
        dispatcher: Event<Rc<LazyRevivedFileSystemEvents>>,
        glob_pattern: GlobPatternDto,
        options: FileSystemWatcherCreateOptions,
    ) -> Rc<FileSystemWatcher> {
        let session = random_session();
        let mut config = 0u8;
        if options.ignore_create_events.is_some_and(|value| value) {
            config += 0b001;
        }
        if options.ignore_change_events.is_some_and(|value| value) {
            config += 0b010;
        }
        if options.ignore_delete_events.is_some_and(|value| value) {
            config += 0b100;
        }

        let ignore_case = match &glob_pattern {
            GlobPatternDto::Pattern(_) => {
                !file_system_info.get_capabilities(schemas::FILE).is_some_and(|capabilities| capabilities.intersects(FileSystemProviderCapabilities::PATH_CASE_SENSITIVE))
            }
            GlobPatternDto::Relative(relative) => file_system_info.ext_uri().ignore_path_casing(&relative.base_uri),
        };

        // Performance: pre-lowercase pattern and paths to use fast case-sensitive
        // matching instead of repeated case-insensitive comparisons in the hot loop.
        // By normalizing to lowercase upfront, we enforce `ignoreCase: false` so the
        // glob parser uses strict `===` / `endsWith` instead of character-by-character
        // case-folding on every comparison.
        let parse_options = GlobOptions { ignore_case: Some(false), ..GlobOptions::default() };
        let parsed_pattern: ParsedPattern = match (&glob_pattern, ignore_case) {
            (GlobPatternDto::Pattern(pattern), true) => glob::parse_pattern(pattern.to_lowercase().as_str(), &parse_options),
            (GlobPatternDto::Pattern(pattern), false) => glob::parse_pattern(pattern.as_str(), &parse_options),
            (GlobPatternDto::Relative(relative), true) => {
                glob::parse_pattern(&glob::RelativePattern { base: relative.base.to_lowercase(), pattern: relative.pattern.to_lowercase() }, &parse_options)
            }
            (GlobPatternDto::Relative(relative), false) => {
                glob::parse_pattern(&glob::RelativePattern { base: relative.base.clone(), pattern: relative.pattern.clone() }, &parse_options)
            }
        };

        // 1.64.x behavior change: given the new support to watch any folder
        // we start to ignore events outside the workspace when only a string
        // pattern is provided to avoid sending events to extensions that are
        // unexpected.
        // https://github.com/microsoft/vscode/issues/3025
        let exclude_out_of_workspace_events = matches!(glob_pattern, GlobPatternDto::Pattern(_));

        // 1.84.x introduces new proposed API for a watcher to set exclude
        // rules. In these cases, we turn the file watcher into correlation
        // mode and ignore any event that does not match the correlation ID.
        //
        // Update (Feb 2025): proposal is discontinued, so the previous
        // `options.correlate` is always `false`.
        let exclude_uncorrelated_events = false;

        let on_did_create = Emitter::<URI>::new();
        let on_did_change = Emitter::<URI>::new();
        let on_did_delete = Emitter::<URI>::new();

        let listener_workspace = workspace.clone();
        let (create, change, delete) = (on_did_create.clone(), on_did_change.clone(), on_did_delete.clone());
        let subscription = dispatcher.listen(move |events: &Rc<LazyRevivedFileSystemEvents>| {
            if events.session.is_some_and(|events_session| events_session != session) {
                return; // ignore events from other file watchers that are in correlation mode
            }

            if exclude_uncorrelated_events && events.session.is_none() {
                return; // ignore events from other non-correlating file watcher when we are in correlation mode
            }

            let matches = |entry: &RevivedFileSystemEvent| {
                let path: Cow<'_, str> = if ignore_case { Cow::Borrowed(&entry.lower_case_fs_path) } else { Cow::Owned(fs_path(&entry.uri)) };
                parsed_pattern.matches(&path, None)
                    && (!exclude_out_of_workspace_events || listener_workspace.get_workspace_folder(&entry.uri).is_some())
            };
            if !options.ignore_create_events.is_some_and(|value| value) {
                for entry in events.created().iter() {
                    if matches(entry) {
                        create.fire(entry.uri.clone());
                    }
                }
            }
            if !options.ignore_change_events.is_some_and(|value| value) {
                for entry in events.changed().iter() {
                    if matches(entry) {
                        change.fire(entry.uri.clone());
                    }
                }
            }
            if !options.ignore_delete_events.is_some_and(|value| value) {
                for entry in events.deleted().iter() {
                    if matches(entry) {
                        delete.fire(entry.uri.clone());
                    }
                }
            }
        });

        let disposable = DisposableStore::new();
        disposable.add(Self::ensure_watching(main_context, workspace.as_ref(), configuration, extension, &glob_pattern, &options, session, false));
        disposable.add_rc(on_did_create.clone());
        disposable.add_rc(on_did_change.clone());
        disposable.add_rc(on_did_delete.clone());
        disposable.add(subscription);

        Rc::new(FileSystemWatcher {
            on_did_create,
            on_did_change,
            on_did_delete,
            config,
            disposable,
        })
    }

    #[allow(clippy::too_many_arguments)]
    fn ensure_watching(
        main_context: &dyn IMainContext,
        workspace: &dyn FileWatcherWorkspace,
        configuration: &dyn FileWatcherConfiguration,
        extension: &ExtensionDescription,
        glob_pattern: &GlobPatternDto,
        options: &FileSystemWatcherCreateOptions,
        session: f64,
        correlate: bool,
    ) -> DisposableRef {
        let nothing = || to_disposable(|| {});

        let GlobPatternDto::Relative(glob_pattern) = glob_pattern else {
            return nothing(); // workspace is already watched by default, no need to watch again!
        };

        let (ignore_change, ignore_create, ignore_delete) =
            (options.ignore_change_events.is_some_and(|v| v), options.ignore_create_events.is_some_and(|v| v), options.ignore_delete_events.is_some_and(|v| v));
        if ignore_change && ignore_create && ignore_delete {
            return nothing(); // no need to watch if we ignore all events
        }

        let proxy = main_context.get_proxy(&MainContext::MAIN_THREAD_FILE_SYSTEM_EVENT_SERVICE);

        let mut recursive = false;
        if glob_pattern.pattern.contains(GLOBSTAR) || glob_pattern.pattern.contains(GLOB_SPLIT) {
            recursive = true; // only watch recursively if pattern indicates the need for it
        }

        let mut excludes: Vec<String> = Vec::new();
        let mut includes: Option<Vec<PatternItem>> = None;
        let mut filter: Option<FileChangeFilter> = None;

        // Correlated: adjust filter based on arguments
        if correlate {
            if ignore_change || ignore_create || ignore_delete {
                let mut adjusted = FileChangeFilter::UPDATED | FileChangeFilter::ADDED | FileChangeFilter::DELETED;

                if ignore_change {
                    adjusted = adjusted & !FileChangeFilter::UPDATED;
                }

                if ignore_create {
                    adjusted = adjusted & !FileChangeFilter::ADDED;
                }

                if ignore_delete {
                    adjusted = adjusted & !FileChangeFilter::DELETED;
                }
                filter = Some(adjusted);
            }
        }
        // Uncorrelated: adjust includes and excludes based on settings
        else {
            // Automatically add `files.watcherExclude` patterns when watching
            // recursively to give users a chance to configure exclude rules
            // for reducing the overhead of watching recursively
            if recursive && excludes.is_empty() {
                let workspace_folder = workspace.get_workspace_folder(&glob_pattern.base_uri);
                let watcher_excludes = configuration.get_files_watcher_exclude(workspace_folder.as_ref());
                if watcher_excludes.is_truthy() {
                    for key in watcher_excludes.own_keys() {
                        let key = key.to_string_lossy();
                        if !key.is_empty() && matches!(watcher_excludes.get_str(&key), Dynamic::Bool(true)) {
                            excludes.push(key);
                        }
                    }
                }
            }
            // Non-recursive watching inside the workspace will overlap with
            // our standard workspace watchers. To prevent duplicate events,
            // we only want to include events for files that are otherwise
            // excluded via `files.watcherExclude`. As such, we configure
            // to include each configured exclude pattern so that only those
            // events are reported that are otherwise excluded.
            // However, we cannot just use the pattern as is, because a pattern
            // such as `bar` for a exclude, will work to exclude any of
            // `<workspace path>/bar` but will not work as include for files within
            // `bar` unless a suffix of `/**` if added.
            // (https://github.com/microsoft/vscode/issues/148245)
            else if !recursive {
                let workspace_folder = workspace.get_workspace_folder(&glob_pattern.base_uri);
                if let Some(workspace_folder) = workspace_folder {
                    let watcher_excludes = configuration.get_files_watcher_exclude(Some(&workspace_folder));
                    if watcher_excludes.is_truthy() {
                        for key in watcher_excludes.own_keys() {
                            let key = key.to_string_lossy();
                            if !key.is_empty() && matches!(watcher_excludes.get_str(&key), Dynamic::Bool(true)) {
                                let include_pattern = format!("{}/{}", key.trim_end_matches('/'), GLOBSTAR);
                                includes
                                    .get_or_insert_with(Vec::new)
                                    .push(normalize_watcher_pattern(&fs_path(workspace_folder.uri()), &PatternItem::Str(include_pattern)));
                            }
                        }
                    }

                    // Still ignore watch request if there are actually no configured
                    // exclude rules, because in that case our default recursive watcher
                    // should be able to take care of all events.
                    if includes.as_ref().is_none_or(|includes| includes.is_empty()) {
                        return nothing();
                    }
                }
            }
        }

        proxy.rpc_watch(&extension.identifier.value, session, &glob_pattern.base_uri, WatchOptions { recursive, excludes, includes, filter, correlation_id: None }, correlate);

        to_disposable(move || proxy.rpc_unwatch(session))
    }

    pub fn ignore_create_events(&self) -> bool {
        self.config & 0b001 != 0
    }

    pub fn ignore_change_events(&self) -> bool {
        self.config & 0b010 != 0
    }

    pub fn ignore_delete_events(&self) -> bool {
        self.config & 0b100 != 0
    }

    pub fn on_did_create(&self) -> Event<URI> {
        self.on_did_create.event()
    }

    pub fn on_did_change(&self) -> Event<URI> {
        self.on_did_change.event()
    }

    pub fn on_did_delete(&self) -> Event<URI> {
        self.on_did_delete.event()
    }
}

impl IDisposable for FileSystemWatcher {
    fn dispose(&self) {
        self.disposable.dispose();
    }
}

/// `Math.random()`: um número em `[0, 1)`.
fn random_session() -> f64 {
    let bits = RandomState::new().build_hasher().finish() >> 11;
    bits as f64 / (1u64 << 53) as f64
}

// ---- eventos de operação de arquivo ----------------------------------------------------------------------------

/// Um item de `FileRenameEvent.files`.
#[derive(Clone, Debug)]
pub struct FileRename {
    pub old_uri: URI,
    pub new_uri: URI,
}

/// `vscode.FileRenameEvent`.
#[derive(Clone, Debug)]
pub struct FileRenameEvent {
    pub files: Vec<FileRename>,
}

/// `vscode.FileCreateEvent`.
#[derive(Clone, Debug)]
pub struct FileCreateEvent {
    pub files: Vec<URI>,
}

/// `vscode.FileDeleteEvent`.
#[derive(Clone, Debug)]
pub struct FileDeleteEvent {
    pub files: Vec<URI>,
}

/// O resultado de um `waitUntil` que ainda não terminou: a `WorkspaceEdit` (se houver) que a promessa devolveu.
type EditSlot = Rc<RefCell<Option<WorkspaceEdit>>>;

/// Os dados de `vscode.FileWill{Rename,Create,Delete}Event` (sem o `waitUntil`, que é do [`WaitUntilEvent`]): os arquivos
/// da operação, mais o canal pelo qual [`wait_until_edit`] entrega a `WorkspaceEdit` ao serviço.
#[derive(Clone)]
pub struct FileWillEvent<F: Clone> {
    pub files: F,
    pending: Rc<RefCell<Option<EditSlot>>>,
}

/// `event.waitUntil(thenable)` de um `FileWill*Event`: a `WorkspaceEdit` que a promessa devolve entra na operação; qualquer
/// outro resultado (`None`) é ignorado.
pub fn wait_until_edit<F: Clone + 'static>(event: &WaitUntilEvent<FileWillEvent<F>>, thenable: Promise<Option<WorkspaceEdit>>) {
    let slot: EditSlot = Rc::new(RefCell::new(None));
    let filled = slot.clone();
    let settled = thenable.then(move |edit| {
        *filled.borrow_mut() = edit;
    });
    *event.data.pending.borrow_mut() = Some(slot);
    event.wait_until(settled);
}

type WillListener<F> = Listener<WaitUntilEvent<FileWillEvent<F>>>;

/// Um `AsyncEmitter` de `onWill*File` e os ouvintes registrados com a extensão de cada um (`IExtensionListener`).
struct WillEmitter<F: Clone + 'static> {
    emitter: Rc<AsyncEmitter<FileWillEvent<F>>>,
    listeners: RefCell<Vec<(WillListener<F>, Rc<ExtensionDescription>)>>,
}

impl<F: Clone + 'static> WillEmitter<F> {
    fn new() -> Rc<WillEmitter<F>> {
        Rc::new(WillEmitter { emitter: AsyncEmitter::new(), listeners: RefCell::new(Vec::new()) })
    }

    /// `_createWillExecuteEvent(extension, emitter)`.
    fn create_will_execute_event(self: &Rc<Self>, extension: &ExtensionDescription) -> Event<WaitUntilEvent<FileWillEvent<F>>> {
        let this = self.clone();
        let extension = Rc::new(extension.clone());
        Event::new(move |listener: WillListener<F>| {
            let wrapped: WillListener<F> = Rc::new(move |event: &WaitUntilEvent<FileWillEvent<F>>| listener(event));
            this.listeners.borrow_mut().push((wrapped.clone(), extension.clone()));
            let subscription = this.emitter.event().listen_rc(wrapped.clone());
            let this = this.clone();
            to_disposable(move || {
                subscription.dispose();
                this.listeners.borrow_mut().retain(|(candidate, _)| !Rc::ptr_eq(candidate, &wrapped));
            })
        })
    }

    fn extension_of(&self, listener: &WillListener<F>) -> Option<Rc<ExtensionDescription>> {
        self.listeners.borrow().iter().find(|(candidate, _)| Rc::ptr_eq(candidate, listener)).map(|(_, extension)| extension.clone())
    }
}

// ---- ExtHostFileSystemEventService ------------------------------------------------------------------------------

/// `ExtHostFileSystemEventService`.
pub struct ExtHostFileSystemEventService {
    main_context: Rc<dyn IMainContext>,
    log_service: LogServiceRef,
    ext_host_documents_and_editors: Rc<ExtHostDocumentsAndEditors>,
    on_file_system_event: Rc<Emitter<Rc<LazyRevivedFileSystemEvents>>>,
    on_did_rename_file: Rc<Emitter<FileRenameEvent>>,
    on_did_create_file: Rc<Emitter<FileCreateEvent>>,
    on_did_delete_file: Rc<Emitter<FileDeleteEvent>>,
    on_will_rename_file: Rc<WillEmitter<Vec<FileRename>>>,
    on_will_create_file: Rc<WillEmitter<Vec<URI>>>,
    on_will_delete_file: Rc<WillEmitter<Vec<URI>>>,
}

impl ExtHostFileSystemEventService {
    pub fn new(main_context: Rc<dyn IMainContext>, log_service: LogServiceRef, ext_host_documents_and_editors: Rc<ExtHostDocumentsAndEditors>) -> Rc<ExtHostFileSystemEventService> {
        Rc::new(ExtHostFileSystemEventService {
            main_context,
            log_service,
            ext_host_documents_and_editors,
            on_file_system_event: Emitter::new(),
            on_did_rename_file: Emitter::new(),
            on_did_create_file: Emitter::new(),
            on_did_delete_file: Emitter::new(),
            on_will_rename_file: WillEmitter::new(),
            on_will_create_file: WillEmitter::new(),
            on_will_delete_file: WillEmitter::new(),
        })
    }

    pub fn on_did_rename_file(&self) -> Event<FileRenameEvent> {
        self.on_did_rename_file.event()
    }

    pub fn on_did_create_file(&self) -> Event<FileCreateEvent> {
        self.on_did_create_file.event()
    }

    pub fn on_did_delete_file(&self) -> Event<FileDeleteEvent> {
        self.on_did_delete_file.event()
    }

    //--- file events

    pub fn create_file_system_watcher(
        &self,
        workspace: Rc<dyn FileWatcherWorkspace>,
        config_provider: &dyn FileWatcherConfiguration,
        file_system_info: &ExtHostFileSystemInfo,
        extension: &ExtensionDescription,
        glob_pattern: &GlobPattern,
        options: FileSystemWatcherCreateOptions,
    ) -> Rc<FileSystemWatcher> {
        FileSystemWatcher::new(
            self.main_context.as_ref(),
            config_provider,
            file_system_info,
            workspace,
            extension,
            self.on_file_system_event.event(),
            glob_pattern_from(glob_pattern),
            options,
        )
    }

    //--- file operations

    pub fn get_on_will_rename_file_event(&self, extension: &ExtensionDescription) -> Event<WaitUntilEvent<FileWillEvent<Vec<FileRename>>>> {
        self.on_will_rename_file.create_will_execute_event(extension)
    }

    pub fn get_on_will_create_file_event(&self, extension: &ExtensionDescription) -> Event<WaitUntilEvent<FileWillEvent<Vec<URI>>>> {
        self.on_will_create_file.create_will_execute_event(extension)
    }

    pub fn get_on_will_delete_file_event(&self, extension: &ExtensionDescription) -> Event<WaitUntilEvent<FileWillEvent<Vec<URI>>>> {
        self.on_will_delete_file.create_will_execute_event(extension)
    }

    fn renames(files: &[SourceTargetPair]) -> Vec<FileRename> {
        files.iter().map(|file| FileRename { old_uri: file.source.clone().expect("MOVE sempre tem origem"), new_uri: file.target.clone() }).collect()
    }

    fn targets(files: &[SourceTargetPair]) -> Vec<URI> {
        files.iter().map(|file| file.target.clone()).collect()
    }

    fn fire_will_event<F: Clone + 'static>(&self, emitter: &Rc<WillEmitter<F>>, files: F, timeout: f64, token: CancellationToken) -> Promise<Option<IWillRunFileOperationParticipation>> {
        let emitter = emitter.clone();
        let log_service = self.log_service.clone();
        let documents = self.ext_host_documents_and_editors.clone();
        async_fn(async move {
            let extension_names: Rc<RefCell<Vec<String>>> = Rc::new(RefCell::new(Vec::new()));
            let edits: Rc<RefCell<Vec<WorkspaceEdit>>> = Rc::new(RefCell::new(Vec::new()));
            let data = FileWillEvent { files, pending: Rc::new(RefCell::new(None)) };
            let pending = data.pending.clone();

            let (join_emitter, join_names, join_edits) = (emitter.clone(), extension_names.clone(), edits.clone());
            let promise_join: PromiseJoin<FileWillEvent<F>> = Rc::new(move |thenable: Promise<()>, listener: &WillListener<F>| {
                let slot = pending.borrow_mut().take();
                let extension = join_emitter.extension_of(listener);
                let (log_service, names, edits) = (log_service.clone(), join_names.clone(), join_edits.clone());
                async_fn(async move {
                    // ignore all results except for WorkspaceEdits. Those are stored in an array.
                    let now = Instant::now();
                    thenable.await?;
                    let edit = slot.and_then(|slot| slot.borrow_mut().take());
                    if let (Some(edit), Some(extension)) = (edit, &extension) {
                        edits.borrow_mut().push(edit);
                        let name = extension.manifest.display_name.clone().unwrap_or_else(|| extension.identifier.value.clone());
                        if !names.borrow().contains(&name) {
                            names.borrow_mut().push(name);
                        }
                    }

                    if now.elapsed().as_secs_f64() * 1000.0 > timeout
                        && let Some(extension) = &extension
                    {
                        log_service.warn_args(&LogMessage::from("SLOW file-participant"), &[LogArg::from(extension.identifier.value.as_str())]);
                    }
                    Ok(())
                })
            });
            emitter.emitter.fire_async(data, token.clone(), Some(promise_join)).await?;

            if token.is_cancellation_requested() {
                return Ok(None);
            }

            if edits.borrow().is_empty() {
                return Ok(None);
            }

            // concat all WorkspaceEdits collected via waitUntil-call and send them over to the renderer
            let mut dto = IWorkspaceEditDto { edits: Vec::new() };
            let versions = DocumentVersions { documents };
            for edit in edits.borrow().iter() {
                dto.edits.extend(workspace_edit_from(edit, &versions)?.edits);
            }
            Ok(Some(IWillRunFileOperationParticipation { edit: dto, extension_names: extension_names.borrow().clone() }))
        })
    }
}

impl ExtHostFileSystemEventServiceShape for ExtHostFileSystemEventService {
    fn rpc_on_file_event(&self, events: FileSystemEvents) {
        self.on_file_system_event.fire(Rc::new(LazyRevivedFileSystemEvents::new(events)));
    }

    fn rpc_on_did_run_file_operation(&self, operation: FileOperation, files: Vec<SourceTargetPair>) {
        match operation {
            FileOperation::Move => self.on_did_rename_file.fire(FileRenameEvent { files: Self::renames(&files) }),
            FileOperation::Delete => self.on_did_delete_file.fire(FileDeleteEvent { files: Self::targets(&files) }),
            FileOperation::Create | FileOperation::Copy => self.on_did_create_file.fire(FileCreateEvent { files: Self::targets(&files) }),
            // ignore, dont send
            FileOperation::Write => {}
        }
    }

    fn rpc_on_will_run_file_operation(
        &self,
        operation: FileOperation,
        files: Vec<SourceTargetPair>,
        timeout: f64,
        token: CancellationToken,
    ) -> Promise<Option<IWillRunFileOperationParticipation>> {
        match operation {
            FileOperation::Move => self.fire_will_event(&self.on_will_rename_file, Self::renames(&files), timeout, token),
            FileOperation::Delete => self.fire_will_event(&self.on_will_delete_file, Self::targets(&files), timeout, token),
            FileOperation::Create | FileOperation::Copy => self.fire_will_event(&self.on_will_create_file, Self::targets(&files), timeout, token),
            FileOperation::Write => Promise::resolved(None),
        }
    }
}

// ---- WorkspaceEdit -> IWorkspaceEditDto ------------------------------------------------------------------------

/// `typeConverters.WorkspaceEdit.IVersionInformationProvider`.
pub trait VersionInformationProvider {
    fn get_text_document_version(&self, uri: &URI) -> Option<i64>;
}

/// `{ getTextDocumentVersion: uri => this._extHostDocumentsAndEditors.getDocument(uri)?.version, ... }`: o
/// `getNotebookDocumentVersion` (sempre `undefined`) só serve às edições de notebook, que [`workspace_edit_from`] não converte.
struct DocumentVersions {
    documents: Rc<ExtHostDocumentsAndEditors>,
}

impl VersionInformationProvider for DocumentVersions {
    fn get_text_document_version(&self, uri: &URI) -> Option<i64> {
        self.documents.get_document(uri).map(|document| document.version() as i64)
    }
}

fn metadata_from(metadata: &Option<WorkspaceEditEntryMetadata>) -> Option<WorkspaceEditMetadata> {
    metadata.as_ref().map(|metadata| WorkspaceEditMetadata {
        needs_confirmation: metadata.needs_confirmation,
        label: metadata.label.clone(),
        description: metadata.description.clone(),
        icon_path: metadata.icon_path.as_ref().map(|icon_path| match icon_path {
            IconPath::Uri(uri) => EditorIconPath::Uri(uri.clone()),
            IconPath::LightDark { light, dark } => EditorIconPath::LightDark { light: light.clone(), dark: dark.clone() },
            IconPath::ThemeIcon(icon) => EditorIconPath::ThemeIcon(icon.clone()),
        }),
    })
}

/// `typeConverters.WorkspaceEdit.from(value, versionInfo)`. As edições de célula de notebook falham (ver o módulo).
pub fn workspace_edit_from(value: &WorkspaceEdit, version_info: &dyn VersionInformationProvider) -> Result<IWorkspaceEditDto, Error> {
    let mut result = IWorkspaceEditDto { edits: Vec::new() };

    // collect all files that are to be created so that their version
    // information (in case they exist as text model already) can be ignored
    let mut to_create = ResourceSet::new(None);
    for entry in value.all_entries() {
        if let WorkspaceEditEntry::File(file) = entry
            && let (Some(to), None) = (&file.to, &file.from)
        {
            to_create.add(to.clone());
        }
    }

    for entry in value.all_entries() {
        match entry {
            WorkspaceEditEntry::File(file) => {
                let options = file.options.clone().unwrap_or_default();
                let contents = options.contents.map(|bytes| WorkspaceFileEditContentsDto::Base64(encode_base64(&VSBuffer::wrap(bytes), true, false)));

                // file operation
                result.edits.push(WorkspaceEditEntryDto::File(IWorkspaceFileEditDto {
                    old_resource: file.from.clone(),
                    new_resource: file.to.clone(),
                    options: Some(WorkspaceFileEditOptionsDto {
                        overwrite: options.overwrite,
                        ignore_if_not_exists: options.ignore_if_not_exists,
                        ignore_if_exists: options.ignore_if_exists,
                        recursive: options.recursive,
                        contents,
                        ..WorkspaceFileEditOptionsDto::default()
                    }),
                    metadata: metadata_from(&file.metadata),
                }));
            }
            WorkspaceEditEntry::Text(text) => {
                // text edits
                result.edits.push(WorkspaceEditEntryDto::Text(IWorkspaceTextEdit {
                    resource: text.uri.clone(),
                    text_edit: TextEditConverter::from(&text.edit),
                    version_id: if to_create.has(&text.uri) { None } else { version_info.get_text_document_version(&text.uri) },
                    metadata: metadata_from(&text.metadata),
                }));
            }
            WorkspaceEditEntry::Snippet(snippet) => {
                result.edits.push(WorkspaceEditEntryDto::Text(IWorkspaceTextEdit {
                    resource: snippet.uri.clone(),
                    text_edit: EditorTextEdit {
                        range: RangeConverter::from(&snippet.range),
                        text: snippet.edit.value.clone(),
                        eol: None,
                        insert_as_snippet: Some(true),
                        keep_whitespace: snippet.keep_whitespace,
                    },
                    version_id: if to_create.has(&snippet.uri) { None } else { version_info.get_text_document_version(&snippet.uri) },
                    metadata: metadata_from(&snippet.metadata),
                }));
            }
            WorkspaceEditEntry::Cell(_) | WorkspaceEditEntry::CellReplace(_) => {
                return Err(Error::new(format!(
                    "WorkspaceEdit.from: a conversão das edições de notebook (FileEditType {}) ainda não foi portada",
                    entry.edit_type().value()
                )));
            }
        }
    }
    Ok(result)
}
