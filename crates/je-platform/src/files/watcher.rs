//! Porte de `src/vs/platform/files/common/watcher.ts`: os pedidos de watch, o contrato dos watchers,
//! o cliente que reinicia um watcher que cai, a coalescência de eventos e os padrões de inclusão.
//!
//! O original separa `INonRecursiveWatchRequest`, `IRecursiveWatchRequest` e `IUniversalWatchRequest` só
//! pelo tipo; aqui existe um único [`WatchRequest`] e o `recursive` decide (as funções de filtro por
//! variante são [`WatchRequest::is_recursive`] e seu inverso).

use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::rc::{Rc, Weak};

use je_base::glob::{self, Glob, GlobOptions, ParsedGlob, PatternItem, RelativePattern};
use je_base::event::Event;
use je_base::lifecycle::{DisposableRef, DisposableStore, IDisposable, MutableDisposable};
use je_base::path::{PathModule, ProcessEnvironment};
use je_base::promise::Promise;

use super::files::{FileChange, FileChangeFilter, FileChangeType, is_parent};

/// `IWatchRequest` (as três variantes numa só).
#[derive(Clone, Debug, PartialEq)]
pub struct WatchRequest {
    pub path: String,
    pub recursive: bool,
    pub excludes: Vec<String>,
    pub includes: Option<Vec<PatternItem>>,
    pub correlation_id: Option<i64>,
    pub filter: Option<FileChangeFilter>,
    /// `IRecursiveWatchRequest.pollingInterval`.
    pub polling_interval: Option<u64>,
}

impl WatchRequest {
    /// `isRecursiveWatchRequest`.
    pub fn is_recursive(&self) -> bool {
        self.recursive
    }

    /// `isWatchRequestWithCorrelation`.
    pub fn has_correlation(&self) -> bool {
        self.correlation_id.is_some()
    }
}

/// `IWatcherErrorEvent`.
#[derive(Clone, Debug)]
pub struct WatcherErrorEvent {
    pub error: String,
    pub request: Option<WatchRequest>,
}

/// `ILogMessage`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LogMessageType {
    Trace,
    Warn,
    Error,
    Info,
    Debug,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LogMessage {
    pub message_type: LogMessageType,
    pub message: String,
}

/// `IWatcher` (e `IRecursiveWatcher`, `INonRecursiveWatcher`, `IUniversalWatcher`: o conjunto de pedidos
/// que cada um aceita é responsabilidade de quem chama).
pub trait Watcher {
    fn on_did_change_file(&self) -> Event<Vec<FileChange>>;
    fn on_did_log_message(&self) -> Event<LogMessage>;
    fn on_did_error(&self) -> Event<WatcherErrorEvent>;
    fn watch(&self, requests: Vec<WatchRequest>) -> Promise<()>;
    fn set_verbose_logging(&self, enabled: bool) -> Promise<()>;
    fn stop(&self) -> Promise<()>;
}

/// `callback(error: true | null, change?)` de `IRecursiveWatcherWithSubscribe.subscribe`: `error` é o
/// `true`; sem erro vem a mudança.
pub type SubscribeCallback = Rc<dyn Fn(bool, Option<&FileChange>)>;

/// `IRecursiveWatcherWithSubscribe`.
pub trait RecursiveWatcherWithSubscribe: Watcher {
    fn subscribe(&self, path: &str, callback: SubscribeCallback) -> Option<DisposableRef>;
}

/// `IRecursiveWatcherOptions.usePolling`: `boolean | string[]`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum UsePolling {
    Bool(bool),
    Paths(Vec<String>),
}

/// `IRecursiveWatcherOptions`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RecursiveWatcherOptions {
    pub use_polling: UsePolling,
    pub polling_interval: Option<u64>,
}

/// Como o cliente cria o watcher a cada (re)início: recebe o store que o cliente descarta ao reiniciar.
pub type CreateWatcher = Rc<dyn Fn(&Rc<DisposableStore>) -> Rc<dyn Watcher>>;

/// `AbstractWatcherClient` (e as duas subclasses abstratas, que só diferem em `type` e `restartOnError`).
pub struct WatcherClient {
    this: Weak<WatcherClient>,
    disposables: Rc<DisposableStore>,
    on_file_changes: Rc<dyn Fn(Vec<FileChange>)>,
    on_log_message: Rc<dyn Fn(LogMessage)>,
    verbose_logging: Cell<bool>,
    watcher_type: &'static str,
    restart_on_error: bool,
    create_watcher: CreateWatcher,
    watcher: RefCell<Option<Rc<dyn Watcher>>>,
    watcher_disposables: Rc<MutableDisposable<DisposableStore>>,
    requests: RefCell<Option<Vec<WatchRequest>>>,
    restart_counter: Cell<usize>,
}

const MAX_RESTARTS: usize = 5;

impl WatcherClient {
    /// `AbstractUniversalWatcherClient`: `{ type: 'universal', restartOnError: true }`.
    pub fn universal(
        on_file_changes: Rc<dyn Fn(Vec<FileChange>)>,
        on_log_message: Rc<dyn Fn(LogMessage)>,
        verbose_logging: bool,
        create_watcher: CreateWatcher,
    ) -> Rc<Self> {
        Self::new(on_file_changes, on_log_message, verbose_logging, "universal", true, create_watcher)
    }

    /// `AbstractNonRecursiveWatcherClient`: `{ type: 'node.js', restartOnError: false }`.
    pub fn non_recursive(
        on_file_changes: Rc<dyn Fn(Vec<FileChange>)>,
        on_log_message: Rc<dyn Fn(LogMessage)>,
        verbose_logging: bool,
        create_watcher: CreateWatcher,
    ) -> Rc<Self> {
        Self::new(on_file_changes, on_log_message, verbose_logging, "node.js", false, create_watcher)
    }

    fn new(
        on_file_changes: Rc<dyn Fn(Vec<FileChange>)>,
        on_log_message: Rc<dyn Fn(LogMessage)>,
        verbose_logging: bool,
        watcher_type: &'static str,
        restart_on_error: bool,
        create_watcher: CreateWatcher,
    ) -> Rc<Self> {
        let client = Rc::new_cyclic(|this| Self {
            this: this.clone(),
            disposables: DisposableStore::new(),
            on_file_changes,
            on_log_message,
            verbose_logging: Cell::new(verbose_logging),
            watcher_type,
            restart_on_error,
            create_watcher,
            watcher: RefCell::new(None),
            watcher_disposables: MutableDisposable::new(),
            requests: RefCell::new(None),
            restart_counter: Cell::new(0),
        });
        client.disposables.add_rc(client.watcher_disposables.clone());
        client.init();
        client
    }

    fn init(&self) {
        let disposables = DisposableStore::new();
        self.watcher_disposables.set(Some(disposables.clone()));

        let watcher = (self.create_watcher)(&disposables);
        *self.watcher.borrow_mut() = Some(watcher.clone());
        let _ = watcher.set_verbose_logging(self.verbose_logging.get());

        let on_changes = self.on_file_changes.clone();
        disposables.add(watcher.on_did_change_file().listen(move |changes| on_changes(changes.clone())));
        let on_log = self.on_log_message.clone();
        disposables.add(watcher.on_did_log_message().listen(move |msg| on_log(msg.clone())));
        let this = self.this.clone();
        disposables.add(watcher.on_did_error().listen(move |e| {
            if let Some(this) = this.upgrade() {
                this.on_error(&e.error, e.request.as_ref());
            }
        }));
    }

    fn on_error(&self, error: &str, failed_request: Option<&WatchRequest>) {
        if self.can_restart(error, failed_request) {
            let requests = self.requests.borrow().clone();
            match requests {
                Some(requests) if self.restart_counter.get() < MAX_RESTARTS => {
                    self.log_error(&format!("restarting watcher after unexpected error: {error}"));
                    self.restart(requests);
                }
                _ => self.log_error(&format!("gave up attempting to restart watcher after unexpected error: {error}")),
            }
        } else {
            self.log_error(error);
        }
    }

    fn can_restart(&self, error: &str, failed_request: Option<&WatchRequest>) -> bool {
        if !self.restart_on_error {
            return false; // disabled by options
        }
        // do not attempt to restart if the error is related to a specific watch request
        if failed_request.is_some() {
            return false;
        }
        // do not attempt to restart if the error indicates that we ran out of file handles / disk space
        !(error.contains("No space left on device") || error.contains("EMFILE"))
    }

    fn restart(&self, requests: Vec<WatchRequest>) {
        self.restart_counter.set(self.restart_counter.get() + 1);
        self.init();
        let _ = self.watch(requests);
    }

    /// `watch(requests)`.
    pub fn watch(&self, requests: Vec<WatchRequest>) -> Promise<()> {
        *self.requests.borrow_mut() = Some(requests.clone());
        match self.watcher.borrow().clone() {
            Some(watcher) => watcher.watch(requests),
            None => Promise::resolved(()),
        }
    }

    /// `setVerboseLogging(verboseLogging)`.
    pub fn set_verbose_logging(&self, verbose_logging: bool) -> Promise<()> {
        self.verbose_logging.set(verbose_logging);
        match self.watcher.borrow().clone() {
            Some(watcher) => watcher.set_verbose_logging(verbose_logging),
            None => Promise::resolved(()),
        }
    }

    fn log_error(&self, message: &str) {
        (self.on_log_message)(LogMessage { message_type: LogMessageType::Error, message: format!("[File Watcher ({})] {message}", self.watcher_type) });
    }

    /// `trace(message)`.
    pub fn trace(&self, message: &str) {
        (self.on_log_message)(LogMessage { message_type: LogMessageType::Trace, message: format!("[File Watcher ({})] {message}", self.watcher_type) });
    }
}

impl IDisposable for WatcherClient {
    fn dispose(&self) {
        *self.watcher.borrow_mut() = None;
        self.disposables.dispose();
    }
}

// region: coalescência e padrões

/// `coalesceEvents(changes)`.
pub fn coalesce_events(changes: Vec<FileChange>) -> Vec<FileChange> {
    let mut coalescer = EventCoalescer::default();
    for event in changes {
        coalescer.process_event(event);
    }
    coalescer.coalesce()
}

/// `normalizeWatcherPattern(path, pattern)`: um padrão que não começa por `**` e não é absoluto vira
/// relativo ao caminho observado.
pub fn normalize_watcher_pattern(path: &str, pattern: &PatternItem) -> PatternItem {
    if let PatternItem::Str(text) = pattern
        && !text.starts_with("**")
        && !PathModule::native(&ProcessEnvironment).is_absolute(text)
    {
        return PatternItem::Relative(RelativePattern { base: path.to_string(), pattern: text.clone() });
    }
    pattern.clone()
}

/// `parseWatcherPatterns(path, patterns, ignoreCase)`.
pub fn parse_watcher_patterns(path: &str, patterns: &[PatternItem], ignore_case: bool) -> Vec<glob::ParsedPattern> {
    let options = GlobOptions { ignore_case: Some(ignore_case), ..GlobOptions::default() };
    patterns
        .iter()
        .map(|pattern| match normalize_watcher_pattern(path, pattern) {
            PatternItem::Str(text) => match glob::parse(Glob::Pattern(&text), &options) {
                ParsedGlob::Pattern(parsed) => parsed,
                ParsedGlob::Expression(_) => unreachable!("um padrão de texto nunca vira expressão"),
            },
            PatternItem::Relative(relative) => glob::parse_pattern(&relative, &options),
        })
        .collect()
}

pub(crate) fn is_linux() -> bool {
    cfg!(target_os = "linux")
}

#[derive(Default)]
struct EventCoalescer {
    /// `Set<IFileChange>`: a posição é a ordem de inserção; `None` é o evento já removido do conjunto.
    coalesced: Vec<Option<FileChange>>,
    map_path_to_change: HashMap<String, usize>,
}

impl EventCoalescer {
    fn to_key(event: &FileChange) -> String {
        let fs_path = event.resource.fs_path(native_platform()).to_string_lossy();
        if is_linux() { fs_path } else { fs_path.to_lowercase() } // normalise to file system case sensitivity
    }

    fn process_event(&mut self, event: FileChange) {
        let key = Self::to_key(&event);
        let mut keep_event = false;
        match self.map_path_to_change.get(&key).copied() {
            Some(index) => {
                let (current_type, existing_path) = {
                    let existing = self.coalesced[index].as_ref().expect("o mapa só aponta para eventos vivos");
                    (existing.change_type, existing.resource.fs_path(native_platform()))
                };
                let new_type = event.change_type;
                // Special case: there are cases of watchers that will send DELETE and ADD events for
                // the same path with different case, so keep both.
                if existing_path != event.resource.fs_path(native_platform()) && matches!(new_type, FileChangeType::Deleted | FileChangeType::Added) {
                    keep_event = true;
                }
                // Ignore CREATE followed by DELETE in one go
                else if current_type == FileChangeType::Added && new_type == FileChangeType::Deleted {
                    self.map_path_to_change.remove(&key);
                    self.coalesced[index] = None;
                }
                // Flatten DELETE followed by CREATE into CHANGE
                else if current_type == FileChangeType::Deleted && new_type == FileChangeType::Added {
                    if let Some(existing) = self.coalesced[index].as_mut() {
                        existing.change_type = FileChangeType::Updated;
                    }
                }
                // Do nothing. Keep the created event
                else if current_type == FileChangeType::Added && new_type == FileChangeType::Updated {
                }
                // Otherwise apply change type
                else if let Some(existing) = self.coalesced[index].as_mut() {
                    existing.change_type = new_type;
                }
            }
            None => keep_event = true,
        }
        if keep_event {
            self.coalesced.push(Some(event));
            self.map_path_to_change.insert(key, self.coalesced.len() - 1);
        }
    }

    fn coalesce(self) -> Vec<FileChange> {
        let mut add_or_change_events = Vec::new();
        let mut deleted_paths: Vec<String> = Vec::new();
        let platform = native_platform();

        // This algorithm will remove all DELETE events up to the root folder that got deleted if any.
        // This ensures that we are not producing DELETE events for each file inside a folder that gets
        // deleted.
        //
        // 1.) split ADD/CHANGE and DELETED events
        // 2.) sort short deleted paths to the top
        // 3.) for each DELETE, check if there is a deleted parent and ignore the event in that case
        let mut deletes: Vec<FileChange> = Vec::new();
        for event in self.coalesced.into_iter().flatten() {
            if event.change_type != FileChangeType::Deleted {
                add_or_change_events.push(event); // remove ADD / CHANGE
            } else {
                deletes.push(event); // keep DELETE
            }
        }
        deletes.sort_by_key(|e| e.resource.fs_path(platform).len()); // shortest path first
        let mut result: Vec<FileChange> = Vec::new();
        for event in deletes {
            let fs_path = event.resource.fs_path(platform).to_string_lossy();
            if deleted_paths.iter().any(|deleted| is_parent(&fs_path, deleted, !is_linux() /* ignorecase */)) {
                continue; // DELETE is ignored if parent is deleted already
            }
            // otherwise mark as deleted
            deleted_paths.push(fs_path);
            result.push(event);
        }
        result.extend(add_or_change_events);
        result
    }
}

pub(crate) fn native_platform() -> je_base::path::Platform {
    use je_base::path::Environment;
    ProcessEnvironment.platform()
}

/// `isFiltered(event, filter)`.
pub fn is_filtered(event: &FileChange, filter: Option<FileChangeFilter>) -> bool {
    if let Some(filter) = filter {
        return match event.change_type {
            FileChangeType::Added => !filter.intersects(FileChangeFilter::ADDED),
            FileChangeType::Deleted => !filter.intersects(FileChangeFilter::DELETED),
            FileChangeType::Updated => !filter.intersects(FileChangeFilter::UPDATED),
        };
    }
    false
}

/// `requestFilterToString(filter)`.
pub fn request_filter_to_string(filter: Option<FileChangeFilter>) -> String {
    let Some(filter) = filter else {
        return "<none>".to_string();
    };
    let mut filters = Vec::new();
    if filter.intersects(FileChangeFilter::ADDED) {
        filters.push("Added");
    }
    if filter.intersects(FileChangeFilter::DELETED) {
        filters.push("Deleted");
    }
    if filter.intersects(FileChangeFilter::UPDATED) {
        filters.push("Updated");
    }
    if filters.is_empty() {
        return "<all>".to_string();
    }
    format!("[{}]", filters.join(", "))
}

// endregion
