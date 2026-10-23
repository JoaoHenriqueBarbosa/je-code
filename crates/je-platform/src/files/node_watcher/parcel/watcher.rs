//! Porte de `src/vs/platform/files/node/watcher/parcel/parcelWatcher.ts`: o watcher RECURSIVO.
//!
//! No original ele usa o `@parcel/watcher` nativo; aqui, o `notify` em modo recursivo (inotify,
//! `ReadDirectoryChangesW`, FSEvents), ou um polling próprio (`fs_events::poll`, com `mtime` em
//! nanossegundos) quando o pedido traz `pollingInterval` (o `getEventsSince`/`writeSnapshot` do Parcel;
//! o `PollWatcher` do `notify` compara `mtime` em segundos e perderia escritas no mesmo segundo). As divergências que o `notify` impõe estão documentadas em
//! `events.rs` (tradução dos eventos, varredura de pasta nova, `ignore` só como filtro) e nestes pontos:
//!
//! - **macOS**: FSEvents (`macos_fsevent`), como o `fs-events` do Parcel; o `kqueue` gastaria um
//!   descritor por arquivo, inviável numa árvore recursiva.
//! - **Reinício**: o `restartWatching` do original (reiniciar o watcher depois de erro) não é chamado por
//!   nada na versão 1.143.1 (o reinício de erro inesperado é do `WatcherClient`, que recria o watcher
//!   inteiro), então não foi portado.
//! - O erro de `notify` (`FsEvent::Error`) tem o papel do `error` do callback do Parcel: o `ENOSPC`
//!   (limite de watches do inotify) é registrado uma vez; os demais disparam `onDidError`.
//! - O processo do Node registra `uncaughtException`/`unhandledRejection` no construtor
//!   (`registerListeners`) para tratá-los como erro inesperado; aqui o `notify` não levanta exceção no
//!   thread do loop, e as rejeições vão para o `onUnexpectedError` do `je_base`, então o equivalente é
//!   desnecessário.

use std::cell::{Cell, RefCell};
use std::path::Path;
use std::rc::{Rc, Weak};
use std::time::Duration;

use je_base::async_::{ThrottledWorker, ThrottledWorkerOptions, async_fn};
use je_base::cancellation::CancellationTokenSource;
use je_base::event::Event;
use je_base::glob::{GLOBSTAR, patterns_equals};
use je_base::lifecycle::{DisposableRef, DisposableStore, IDisposable};
use je_base::map::OrderedMap;
use je_base::promise::{DeferredPromise, Promise};
use je_base::ternary_search_tree::TernarySearchTree;

use crate::files::files::{FileChange, FileChangeType};
use crate::files::watcher::{
    LogMessage, LogMessageType, RecursiveWatcherWithSubscribe, SubscribeCallback, WatchRequest, Watcher, WatcherErrorEvent, coalesce_events, is_filtered, is_linux,
};

use super::super::base_watcher::{
    BaseWatcher, BaseWatcherOptions, WatcherBackend, WatcherKey, change_type_label, request_to_string, request_to_watcher_key, string_patterns,
};
use super::super::fs_events::{FsEvent, FsWatch};
use super::super::fs_util::{file_uri, fs_path, is_equal, is_equal_or_parent, normalize_nfc, real_case, real_path};
use super::events::{IgnoreMatcher, ParcelEvent, ParcelEventType, parcel_events};
use super::instance::{ParcelWatcherInstance, StopFn};

/// A delay for collecting file changes from Parcel before collecting them for coalescing and emitting.
/// Parcel internally uses 50ms as delay, so we use 75ms, to schedule sufficiently after Parcel.
const FILE_CHANGES_HANDLER_DELAY: u64 = 75;

/// `PREDEFINED_EXCLUDES`: no macOS, `~/Library/Containers` abre o diálogo de acesso do macOS 14
/// (https://github.com/microsoft/vscode/issues/208105).
fn predefined_excludes() -> Vec<String> {
    if cfg!(target_os = "macos")
        && let Ok(home) = std::env::var("HOME")
    {
        return vec![super::super::fs_util::join(&[&home, "Library", "Containers"])];
    }
    Vec::new()
}

/// `addPredefinedExcludes(request.excludes)`.
fn ignore_list(request: &WatchRequest) -> Vec<String> {
    let mut excludes = request.excludes.clone();
    for exclude in predefined_excludes() {
        if !excludes.contains(&exclude) {
            excludes.push(exclude);
        }
    }
    excludes
}

pub struct ParcelWatcher {
    this: Weak<ParcelWatcher>,
    base: Rc<BaseWatcher>,
    on_did_error: Rc<je_base::event::Emitter<WatcherErrorEvent>>,
    watchers: RefCell<OrderedMap<WatcherKey, Rc<ParcelWatcherInstance>>>,
    /// Reduce likelyhood of spam from file events via throttling.
    /// (https://github.com/microsoft/vscode/issues/124723)
    throttled_file_changes_emitter: Rc<ThrottledWorker<FileChange>>,
    enospc_error_logged: Cell<bool>,
    store: Rc<DisposableStore>,
}

impl ParcelWatcher {
    pub fn new() -> Rc<Self> {
        Self::with_options(BaseWatcherOptions::default())
    }

    pub fn with_options(options: BaseWatcherOptions) -> Rc<Self> {
        Rc::new_cyclic(|this: &Weak<Self>| {
            let base = BaseWatcher::new(this.clone() as Weak<dyn WatcherBackend>, options);
            let store = DisposableStore::new();
            let on_did_error = store.add_rc(je_base::event::Emitter::new());
            let on_change = base.on_did_change_file.clone();
            let throttled_file_changes_emitter = store.add_rc(ThrottledWorker::new(
                ThrottledWorkerOptions {
                    max_work_chunk_size: 500, // only process up to 500 changes at once before...
                    throttle_delay: 200,      // ...resting for 200ms until we process events again...
                    max_buffered_work: Some(30000), // ...but never buffering more than 30000 events in memory
                    wait_throttle_delay_between_work_units: false,
                },
                move |events| on_change.fire(events),
            ));
            Self {
                this: this.clone(),
                base,
                on_did_error,
                watchers: RefCell::new(OrderedMap::new()),
                throttled_file_changes_emitter,
                enospc_error_logged: Cell::new(false),
                store,
            }
        })
    }

    pub fn base(&self) -> &Rc<BaseWatcher> {
        &self.base
    }

    /// `get watchers()`.
    pub fn watchers(&self) -> Vec<Rc<ParcelWatcherInstance>> {
        self.watchers.borrow().values().cloned().collect()
    }

    fn me(&self) -> Rc<Self> {
        self.this.upgrade().expect("o watcher vive enquanto é chamado")
    }

    fn start_polling(&self, request: WatchRequest, polling_interval: u64, restarts: usize) -> Promise<()> {
        let this = self.me();
        async_fn(async move {
            let cts = CancellationTokenSource::default();
            let instance = DeferredPromise::<()>::new();
            let fs_watch: Rc<RefCell<Option<Rc<FsWatch>>>> = Rc::new(RefCell::new(None));

            let stop_fn: StopFn = {
                let (cts, fs_watch) = (cts.clone(), fs_watch.clone());
                Box::new(move |watcher: &ParcelWatcherInstance| {
                    cts.dispose_with(true);

                    watcher.worker.flush();
                    watcher.worker.dispose();

                    if let Some(watch) = fs_watch.borrow_mut().take() {
                        watch.dispose();
                    }
                    Promise::resolved(())
                })
            };
            let watcher = this.new_instance(instance.promise.clone(), &request, restarts, &cts, stop_fn);
            this.watchers.borrow_mut().insert(request_to_watcher_key(&request), watcher.clone());

            // Path checks for symbolic links / wrong casing
            let (real_path, real_path_differs, real_path_length) = this.normalize_path(&request).await?;

            this.trace(&format!("Started watching: '{real_path}' with polling interval '{polling_interval}'"), None);

            let handler = this.event_handler(&watcher, &request, real_path.clone(), real_path_differs, real_path_length, true);
            match FsWatch::start(Path::new(&real_path), true, Some(Duration::from_millis(polling_interval)), ignore_list(&request), handler) {
                Ok(watch) => *fs_watch.borrow_mut() = Some(watch),
                Err(error) => this.on_unexpected_error(&error, Some(&request)),
            }
            // Signal we are ready now when the first snapshot was written
            instance.complete(());
            Ok(())
        })
    }

    fn start_watching(&self, request: WatchRequest, restarts: usize) -> Promise<()> {
        let this = self.me();
        async_fn(async move {
            let cts = CancellationTokenSource::default();
            let instance = DeferredPromise::<()>::new();
            let fs_watch: Rc<RefCell<Option<Rc<FsWatch>>>> = Rc::new(RefCell::new(None));

            // Remember as watcher instance
            let stop_fn: StopFn = {
                let (cts, ready, fs_watch) = (cts.clone(), instance.promise.clone(), fs_watch.clone());
                Box::new(move |watcher: &ParcelWatcherInstance| {
                    cts.dispose_with(true);

                    watcher.worker.flush();
                    watcher.worker.dispose();

                    ready.then(move |()| {
                        if let Some(watch) = fs_watch.borrow_mut().take() {
                            watch.dispose();
                        }
                    })
                })
            };
            let watcher = this.new_instance(instance.promise.clone(), &request, restarts, &cts, stop_fn);
            this.watchers.borrow_mut().insert(request_to_watcher_key(&request), watcher.clone());

            // Path checks for symbolic links / wrong casing
            let (real_path, real_path_differs, real_path_length) = this.normalize_path(&request).await?;

            let handler = this.event_handler(&watcher, &request, real_path.clone(), real_path_differs, real_path_length, false);
            match FsWatch::start(Path::new(&real_path), true, None, Vec::new(), handler) {
                Ok(watch) => {
                    *fs_watch.borrow_mut() = Some(watch);
                    this.trace(&format!("Started watching: '{real_path}' with backend 'notify'"), None);

                    instance.complete(());
                }
                Err(error) => {
                    this.on_unexpected_error(&error, Some(&request));

                    instance.complete(());

                    watcher.notify_watch_failed();
                    this.base.on_did_watch_fail.fire(request);
                }
            }
            Ok(())
        })
    }

    fn new_instance(&self, ready: Promise<()>, request: &WatchRequest, restarts: usize, cts: &CancellationTokenSource, stop_fn: StopFn) -> Rc<ParcelWatcherInstance> {
        let this = self.this.clone();
        ParcelWatcherInstance::new(
            ready,
            request.clone(),
            restarts,
            cts.token(),
            Rc::new(move |events, watcher| {
                if let Some(this) = this.upgrade() {
                    this.handle_parcel_events(events, watcher);
                }
            }),
            FILE_CHANGES_HANDLER_DELAY,
            stop_fn,
        )
    }

    /// O callback de `parcelWatcher.subscribe(realPath, (error, parcelEvents) => ...)`.
    fn event_handler(
        &self,
        watcher: &Rc<ParcelWatcherInstance>,
        request: &WatchRequest,
        real_path: String,
        real_path_differs: bool,
        real_path_length: usize,
        polling: bool,
    ) -> Rc<dyn Fn(FsEvent)> {
        let this = self.this.clone();
        let watcher = Rc::downgrade(watcher);
        let request = request.clone();
        let ignore = IgnoreMatcher::new(&real_path, &ignore_list(&request));
        Rc::new(move |event: FsEvent| {
            let (Some(this), Some(watcher)) = (this.upgrade(), watcher.upgrade()) else { return };
            if watcher.token.is_cancellation_requested() {
                return; // return early when disposed
            }

            match event {
                // In any case of an error, treat this like a unhandled exception
                // that might require the watcher to restart. We do not really know
                // the state of parcel at this point and as such will try to restart
                // up to our maximum of restarts.
                FsEvent::Error(error) => this.on_unexpected_error(&error, Some(&request)),
                // Handle & emit events
                FsEvent::Event(event) => {
                    let events = parcel_events(&event, polling, &ignore).into_iter().filter(|e| !ignore.is_ignored(&e.path)).collect();
                    this.on_parcel_events(events, &watcher, real_path_differs, real_path_length);
                }
            }
        })
    }

    fn on_parcel_events(&self, mut parcel_events: Vec<ParcelEvent>, watcher: &Rc<ParcelWatcherInstance>, real_path_differs: bool, real_path_length: usize) {
        if parcel_events.is_empty() {
            return;
        }

        // Normalize events: handle NFC normalization and symlinks
        // It is important to do this before checking for includes
        // to check on the original path.
        self.normalize_events(&mut parcel_events, &watcher.request, real_path_differs, real_path_length);

        // Check for includes
        let included_events = self.handle_includes(watcher, parcel_events);

        // Add to event aggregator for later processing
        for included_event in included_events {
            watcher.worker.work(included_event);
        }
    }

    fn handle_includes(&self, watcher: &Rc<ParcelWatcherInstance>, parcel_events: Vec<ParcelEvent>) -> Vec<FileChange> {
        let mut events = Vec::new();

        for ParcelEvent { path, event_type } in parcel_events {
            let change_type = match event_type {
                ParcelEventType::Create => FileChangeType::Added,
                ParcelEventType::Update => FileChangeType::Updated,
                ParcelEventType::Delete => FileChangeType::Deleted,
            };
            if self.base.verbose_logging() {
                self.trace_with_correlation(&format!("{} {path}", change_type_label(change_type)), &watcher.request);
            }

            // Apply include filter if any
            if !watcher.include(&path) {
                if self.base.verbose_logging() {
                    self.trace_with_correlation(&format!(" >> ignored (not included) {path}"), &watcher.request);
                }
            } else {
                events.push(FileChange { change_type, resource: file_uri(&path), c_id: watcher.request.correlation_id });
            }
        }

        events
    }

    fn handle_parcel_events(&self, parcel_events: Vec<FileChange>, watcher: &Rc<ParcelWatcherInstance>) {
        // Coalesce events: merge events of same kind
        let coalesced_events = coalesce_events(parcel_events);

        // Filter events: check for specific events we want to exclude
        let (filtered_events, root_deleted) = self.filter_events(coalesced_events, watcher);

        // Broadcast to clients
        self.emit_events(&filtered_events, watcher);

        // Handle root path deletes
        if root_deleted {
            self.on_watched_path_deleted(watcher);
        }
    }

    fn emit_events(&self, events: &[FileChange], watcher: &Rc<ParcelWatcherInstance>) {
        if events.is_empty() {
            return;
        }

        // Broadcast to clients via throttler
        let worked = self.throttled_file_changes_emitter.work(events);

        // Logging
        let most_recent = fs_path(&events[0].resource);
        if !worked {
            self.warn(
                &format!(
                    "started ignoring events due to too many file change events at once (incoming: {}, most recent change: {most_recent}). Use 'files.watcherExclude' setting to exclude folders with lots of changing files (e.g. compilation output).",
                    events.len()
                ),
                None,
            );
        } else if self.throttled_file_changes_emitter.pending() > 0 {
            self.trace(
                &format!(
                    "started throttling events due to large amount of file change events at once (pending: {}, most recent change: {most_recent}). Use 'files.watcherExclude' setting to exclude folders with lots of changing files (e.g. compilation output).",
                    self.throttled_file_changes_emitter.pending()
                ),
                Some(watcher),
            );
        }
    }

    /// `normalizePath(request)`: o caminho real (resolve link simbólico e caixa), se difere do pedido, e o
    /// tamanho dele.
    fn normalize_path(&self, request: &WatchRequest) -> Promise<(String, bool, usize)> {
        let this = self.me();
        let request = request.clone();
        async_fn(async move {
            let mut real = request.path.clone();
            let mut real_path_differs = false;
            let mut real_path_length = request.path.len();

            // First check for symbolic link
            if let Some(resolved) = Promise::resolved(real_path(&request.path)).await? {
                real = resolved;

                // Second check for casing difference
                // Note: this will be a no-op on Linux platforms
                if request.path == real {
                    real = real_case(&request.path).unwrap_or_else(|| request.path.clone());
                }

                // Correct watch path as needed
                if request.path != real {
                    real_path_length = real.len();
                    real_path_differs = true;

                    this.trace(&format!("correcting a path to watch that seems to be a symbolic link or wrong casing (original: {}, real: {real})", request.path), None);
                }
            }

            Ok((real, real_path_differs, real_path_length))
        })
    }

    fn normalize_events(&self, events: &mut [ParcelEvent], request: &WatchRequest, real_path_differs: bool, real_path_length: usize) {
        for event in events {
            // Mac uses NFD unicode form on disk, but we want NFC
            if cfg!(target_os = "macos") {
                event.path = normalize_nfc(&event.path);
            }

            // Workaround for https://github.com/parcel-bundler/watcher/issues/68
            // where watching root drive letter adds extra backslashes.
            if cfg!(windows) && request.path.len() <= 3 {
                // for ex. c:, C:\
                event.path = je_base::path::PathModule::native(&je_base::path::ProcessEnvironment).normalize(&event.path);
            }

            // Convert paths back to original form in case it differs
            if real_path_differs {
                event.path = format!("{}{}", request.path, event.path.get(real_path_length..).unwrap_or(""));
            }
        }
    }

    fn filter_events(&self, events: Vec<FileChange>, watcher: &Rc<ParcelWatcherInstance>) -> (Vec<FileChange>, bool) {
        let mut filtered_events = Vec::new();
        let mut root_deleted = false;

        // filtering is only enabled when correlating because watchers are otherwise potentially reused
        let filter = if watcher.request.has_correlation() { watcher.request.filter } else { None };
        for event in events {
            let path = fs_path(&event.resource);

            // Emit to instance subscriptions if any before filtering
            if watcher.subscriptions_count() > 0 {
                watcher.notify_file_change(&path, &event);
            }

            // Filtering. Como no original, `root_deleted` é atribuído (não acumulado) a cada evento: só o
            // último evento do lote decide.
            root_deleted = event.change_type == FileChangeType::Deleted && is_equal(&path, &watcher.request.path);
            if is_filtered(&event, filter) {
                if self.base.verbose_logging() {
                    self.trace_with_correlation(&format!(" >> ignored (filtered) {path}"), &watcher.request);
                }

                continue;
            }

            // Logging
            self.base.trace_event(&event, watcher.request.correlation_id);

            filtered_events.push(event);
        }

        (filtered_events, root_deleted)
    }

    fn on_watched_path_deleted(&self, watcher: &Rc<ParcelWatcherInstance>) {
        self.warn("Watcher shutdown because watched path got deleted", Some(watcher));

        watcher.notify_watch_failed();
        self.base.on_did_watch_fail.fire(watcher.request.clone());
    }

    fn on_unexpected_error(&self, msg: &str, request: Option<&WatchRequest>) {
        // Specially handle ENOSPC errors that can happen when
        // the watcher consumes so many file descriptors that
        // we are running into a limit. We only want to warn
        // once in this case to avoid log spam.
        // See https://github.com/microsoft/vscode/issues/7950
        if msg.contains("No space left on device") {
            if !self.enospc_error_logged.get() {
                self.error("Inotify limit reached (ENOSPC)", request);

                self.enospc_error_logged.set(true);
            }
        }
        // Version 2.5.1 introduces 3 new errors on macOS
        // via https://github.dev/parcel-bundler/watcher/pull/196
        else if msg.contains("File system must be re-scanned") {
            self.error(msg, request);
        }
        // Any other error is unexpected and we should try to
        // restart the watcher as a result to get into healthy
        // state again if possible and if not attempted too much
        else {
            self.error(&format!("Unexpected error: {msg} (EUNKNOWN)"), request);

            self.on_did_error.fire(WatcherErrorEvent { error: msg.to_string(), request: request.cloned() });
        }
    }

    fn stop_watching(&self, watcher: &Rc<ParcelWatcherInstance>, join_restart: Option<Promise<()>>) -> Promise<()> {
        self.trace("stopping file watcher", Some(watcher));

        self.watchers.borrow_mut().remove(&request_to_watcher_key(&watcher.request));

        let this = self.this.clone();
        let request = watcher.request.clone();
        watcher.stop(join_restart).then_result(move |result| {
            if let (Err(error), Some(this)) = (result, this.upgrade()) {
                this.error(&format!("Unexpected error stopping watcher: {error}"), Some(&request));
            }
            Ok(())
        })
    }

    /// `removeDuplicateRequests(requests, validatePaths)`.
    pub fn remove_duplicate_requests(&self, mut requests: Vec<WatchRequest>, validate_paths: bool) -> Vec<WatchRequest> {
        // Sort requests by path length to have shortest first
        // to have a way to prevent children to be watched if
        // parents exist.
        requests.sort_by_key(|request| request.path.encode_utf16().count());

        // Ignore requests for the same paths that have the same correlation
        let mut map_correlation_to_requests: OrderedMap<Option<i64>, OrderedMap<String, WatchRequest>> = OrderedMap::new();
        for request in requests {
            if request.excludes.iter().any(|exclude| exclude == GLOBSTAR) {
                continue; // path is ignored entirely (via `**` glob exclude)
            }

            if !map_correlation_to_requests.contains_key(&request.correlation_id) {
                map_correlation_to_requests.insert(request.correlation_id, OrderedMap::new());
            }
            let requests_for_correlation = map_correlation_to_requests.get_mut(&request.correlation_id).expect("acabou de inserir");

            let path = super::super::fs_util::path_to_watcher_key(&request.path);
            let previous = requests_for_correlation.get(&path).cloned();
            match previous {
                Some(previous) if request.correlation_id.is_none() => {
                    let includes = match (&previous.includes, &request.includes) {
                        (Some(a), Some(b)) if !a.is_empty() && !b.is_empty() => Some([a.as_slice(), b.as_slice()].concat()),
                        _ => None,
                    };
                    let excludes: Vec<String> = previous.excludes.iter().filter(|exclude| request.excludes.contains(exclude)).cloned().collect();
                    let broad = if request.includes.as_ref().is_none_or(Vec::is_empty) { &request } else { &previous };
                    let merged = match includes {
                        None => WatchRequest { excludes, ..broad.clone() },
                        Some(includes) => WatchRequest { includes: Some(includes), excludes, ..previous.clone() },
                    };
                    requests_for_correlation.insert(path, merged);
                }
                previous => {
                    if previous.is_some() {
                        self.trace(&format!("ignoring a request for watching who's path is already watched: {}", request_to_string(&request)), None);
                    }

                    requests_for_correlation.insert(path, request);
                }
            }
        }

        let mut normalized_requests = Vec::new();

        for requests_for_correlation in map_correlation_to_requests.values() {
            // Only consider requests for watching that are not
            // a child of an existing request path to prevent
            // duplication. In addition, drop any request where
            // everything is excluded (via `**` glob).
            //
            // However, allow explicit requests to watch folders
            // that are symbolic links because the Parcel watcher
            // does not allow to recursively watch symbolic links.
            let mut request_trie: TernarySearchTree<String, WatchRequest> = TernarySearchTree::for_paths(!is_linux());

            for request in requests_for_correlation.values() {
                // Check for overlapping request paths (but preserve symbolic links)
                if request_trie.find_substr(&request.path).is_some() {
                    if request_trie.has(&request.path) {
                        self.trace(&format!("ignoring a request for watching who's path is already watched: {}", request_to_string(request)), None);
                    } else {
                        match std::fs::symlink_metadata(&request.path) {
                            Ok(metadata) => {
                                if !metadata.file_type().is_symlink() {
                                    self.trace(&format!("ignoring a request for watching who's parent is already watched: {}", request_to_string(request)), None);

                                    continue;
                                }
                            }
                            Err(error) => {
                                self.trace(&format!("ignoring a request for watching who's lstat failed to resolve: {} (error: {error})", request_to_string(request)), None);

                                self.base.on_did_watch_fail.fire(request.clone());

                                continue;
                            }
                        }
                    }
                }

                // Check for invalid paths
                if validate_paths && !self.is_path_valid(&request.path) {
                    self.base.on_did_watch_fail.fire(request.clone());

                    continue;
                }

                request_trie.set(request.path.clone(), request.clone());
            }

            normalized_requests.extend(request_trie.iter_entries().map(|(_, request)| request));
        }

        normalized_requests
    }

    fn is_path_valid(&self, path: &str) -> bool {
        match std::fs::metadata(path) {
            Ok(metadata) => {
                if !metadata.is_dir() {
                    self.trace(&format!("ignoring a path for watching that is a file and not a folder: {path}"), None);

                    return false;
                }
            }
            Err(error) => {
                self.trace(&format!("ignoring a path for watching who's stat info failed to resolve: {path} (error: {error})"), None);

                return false;
            }
        }

        true
    }

    fn trace_with_correlation(&self, message: &str, request: &WatchRequest) {
        self.base.trace_with_correlation(message, request.correlation_id);
    }

    fn trace(&self, message: &str, watcher: Option<&Rc<ParcelWatcherInstance>>) {
        if self.base.verbose_logging() {
            self.base.log(LogMessageType::Trace, to_message(message, watcher.map(|w| &w.request)));
        }
    }

    fn warn(&self, message: &str, watcher: Option<&Rc<ParcelWatcherInstance>>) {
        self.base.log(LogMessageType::Warn, to_message(message, watcher.map(|w| &w.request)));
    }

    fn error(&self, message: &str, request: Option<&WatchRequest>) {
        self.base.log(LogMessageType::Error, to_message(message, request));
    }
}

fn to_message(message: &str, request: Option<&WatchRequest>) -> String {
    match request {
        Some(request) => format!("[File Watcher ('parcel')] {message} (path: {})", request.path),
        None => format!("[File Watcher ('parcel')] {message}"),
    }
}

impl WatcherBackend for ParcelWatcher {
    fn do_watch(&self, requests: Vec<WatchRequest>) -> Promise<()> {
        let this = self.me();
        async_fn(async move {
            // Figure out duplicates to remove from the requests
            let requests = this.remove_duplicate_requests(requests, true);

            // Figure out which watchers to start and which to stop
            let mut requests_to_start = Vec::new();
            let mut watchers_to_stop = this.watchers();
            for request in requests {
                let watcher = this.watchers.borrow().get(&request_to_watcher_key(&request)).cloned();
                match watcher {
                    Some(watcher)
                        if patterns_equals(Some(&string_patterns(&watcher.request.excludes)), Some(&string_patterns(&request.excludes)))
                            && patterns_equals(watcher.request.includes.as_deref(), request.includes.as_deref())
                            && watcher.request.polling_interval == request.polling_interval =>
                    {
                        watchers_to_stop.retain(|w| !Rc::ptr_eq(w, &watcher)); // keep watcher
                    }
                    _ => requests_to_start.push(request), // start watching
                }
            }

            // Logging
            if !requests_to_start.is_empty() {
                let list: Vec<String> = requests_to_start.iter().map(request_to_string).collect();
                this.trace(&format!("Request to start watching: {}", list.join(",")), None);
            }

            if !watchers_to_stop.is_empty() {
                let list: Vec<String> = watchers_to_stop.iter().map(|w| request_to_string(&w.request)).collect();
                this.trace(&format!("Request to stop watching: {}", list.join(",")), None);
            }

            // Stop watching as instructed
            for watcher in &watchers_to_stop {
                this.stop_watching(watcher, None).await?;
            }

            // Start watching as instructed
            for request in requests_to_start {
                match request.polling_interval.filter(|interval| *interval > 0) {
                    Some(interval) => this.start_polling(request, interval, 0).await?,
                    None => this.start_watching(request, 0).await?,
                }
            }
            Ok(())
        })
    }

    fn when_ready(&self) -> Promise<()> {
        let readies: Vec<Promise<()>> = self.watchers().iter().map(|w| w.ready.clone()).collect();
        Promise::all(readies).then(|_| ())
    }

    fn recursive_watcher(&self) -> Option<Rc<dyn RecursiveWatcherWithSubscribe>> {
        Some(self.me())
    }

    fn trace(&self, message: &str) {
        ParcelWatcher::trace(self, message, None);
    }

    fn warn(&self, message: &str) {
        ParcelWatcher::warn(self, message, None);
    }
}

impl Watcher for ParcelWatcher {
    fn on_did_change_file(&self) -> Event<Vec<FileChange>> {
        self.base.on_did_change_file.event()
    }

    fn on_did_log_message(&self) -> Event<LogMessage> {
        self.base.on_did_log_message.event()
    }

    fn on_did_error(&self) -> Event<WatcherErrorEvent> {
        self.on_did_error.event()
    }

    fn watch(&self, requests: Vec<WatchRequest>) -> Promise<()> {
        self.base.watch(requests)
    }

    fn set_verbose_logging(&self, enabled: bool) -> Promise<()> {
        self.base.set_verbose_logging(enabled)
    }

    fn stop(&self) -> Promise<()> {
        let this = self.me();
        let base_stop = self.base.stop();
        async_fn(async move {
            base_stop.await?;

            for watcher in this.watchers() {
                this.stop_watching(&watcher, None).await?;
            }
            Ok(())
        })
    }
}

impl RecursiveWatcherWithSubscribe for ParcelWatcher {
    fn subscribe(&self, path: &str, callback: SubscribeCallback) -> Option<DisposableRef> {
        for watcher in self.watchers() {
            if watcher.failed() {
                continue; // watcher has already failed
            }

            if !is_equal_or_parent(path, &watcher.request.path) {
                continue; // watcher does not consider this path
            }

            if watcher.exclude(path) || !watcher.include(path) {
                continue; // parcel instance does not consider this path
            }

            let disposables = DisposableStore::new();

            let (store, stop_callback) = (disposables.clone(), callback.clone());
            disposables.add(watcher.on_did_stop.event().once().listen(move |join_restart| {
                // if we are restarting, await that so that we can possibly reuse this watcher again
                let (store, callback) = (store.clone(), stop_callback.clone());
                let _ = join_restart.clone().unwrap_or_else(|| Promise::resolved(())).then(move |()| {
                    if store.is_disposed() {
                        return;
                    }

                    callback(true /* error */, None);
                });
            }));
            let fail_callback = callback.clone();
            disposables.add(watcher.on_did_fail.event().once().listen(move |()| fail_callback(true /* error */, None)));
            let change_callback = callback.clone();
            disposables.add(watcher.subscribe(path, Rc::new(move |change| change_callback(false, Some(change)))));

            return Some(disposables);
        }

        None
    }
}

impl IDisposable for ParcelWatcher {
    fn dispose(&self) {
        for watcher in self.watchers() {
            let _ = self.stop_watching(&watcher, None);
        }
        self.base.dispose();
        self.store.dispose();
    }
}
