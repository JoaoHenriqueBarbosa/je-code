//! Porte de `src/vs/platform/files/node/watcher/nodejs/nodejsWatcherLib.ts`: o watcher de UM caminho, sem
//! recursão, que no Node usa `fs.watch`.
//!
//! Aqui o `fs.watch` é o `notify` em modo não recursivo (ver `fs_events.rs`). O que a lógica original
//! consome do `fs.watch` é o par `(tipo, nome)` com `tipo` `rename` ou `change`, e o porte o reconstrói a
//! partir dos eventos do `notify` em [`node_events`], com a mesma regra do libuv no Linux: criar, apagar e
//! mover são `rename`; escrever e mudar atributo são `change`; o nome é o do filho para pasta observada e o
//! nome do próprio arquivo para arquivo observado. Daí em diante a máquina de estados (pasta contra
//! arquivo, atraso de 100 ms para apagar, recriação do watcher, `includes`/`excludes`, `correlationId`,
//! filtro, coalescência e agrupamento de 75 ms) é a do original, linha a linha.
//!
//! Divergências de semântica em relação ao `fs.watch`:
//! - Eventos que o inotify do `notify` traz e o libuv não pede (`OPEN`, `CLOSE_*`, `Modify(Name(Both))`
//!   que repete o par `From`/`To`) são descartados em [`node_events`].
//! - Ao apagar a pasta observada, o Windows repete o caminho absoluto no `rename` e o macOS não avisa; o
//!   `notify` não reproduz nenhum dos dois (no Linux, o `DELETE_SELF` chega como o nome da própria pasta,
//!   que é o ramo `isEqual(changedFileName, pathBasename)` do original). Os ramos específicos de Windows
//!   do original estão portados, mas só um teste em Windows os exercita.
//! - Eventos que o `notify` entrega entre o `watch` e a leitura das pastas filhas (o `await readdir` do
//!   original) são descartados, como o `EventEmitter` do Node descarta o que sai antes do `on('change')`.

use std::cell::{Cell, RefCell};
use std::collections::{HashMap, HashSet};
use std::path::Path;
use std::rc::{Rc, Weak};

use je_base::async_::{RunOnceWorker, ThrottledWorker, ThrottledWorkerOptions, async_fn};
use je_base::cancellation::{CancellationToken, CancellationTokenSource};
use je_base::errors::Error;
use je_base::glob::ParsedPattern;
use je_base::lifecycle::{DisposableRef, DisposableStore, IDisposable, MutableDisposable, then_register_or_dispose, to_disposable};
use je_base::promise::{Promise, Resolver};
use je_base::runtime::{clear_timeout, set_timeout};

use crate::files::files::{FileChange, FileChangeFilter, FileChangeType};
use crate::files::watcher::{
    LogMessage, LogMessageType, RecursiveWatcherWithSubscribe, WatchRequest, coalesce_events, is_filtered, is_linux, parse_watcher_patterns,
};

use super::super::base_watcher::{change_type_label, string_patterns};
use super::super::fs_events::{FsEvent, FsWatch};
use super::super::fs_util::{
    basename, dirname, exists, file_uri, fs_path, is_absolute, is_equal, is_equal_or_parent, join, normalize_nfc, read_dir_names, real_path,
};

/// A delay in reacting to file deletes to support atomic save operations where a tool may chose to delete
/// a file before creating it again for an update.
const FILE_DELETE_HANDLER_DELAY: u64 = 100;

/// A delay for collecting file changes from node.js before collecting them for coalescing and emitting.
/// Same delay as used for the recursive watcher.
const FILE_CHANGES_HANDLER_DELAY: u64 = 75;

/// O `type` do evento do `fs.watch`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum NodeEventType {
    Rename,
    Change,
}

/// Um evento do `notify` visto como o `fs.watch` o entregaria: o tipo e o nome (relativo à pasta
/// observada, ou o nome do arquivo observado).
fn node_events(event: &notify::Event, real_path: &str, is_directory: bool) -> Vec<(NodeEventType, String)> {
    use notify::EventKind;
    use notify::event::{ModifyKind, RenameMode};
    let event_type = match event.kind {
        EventKind::Create(_) | EventKind::Remove(_) => NodeEventType::Rename,
        EventKind::Modify(ModifyKind::Name(RenameMode::Both)) => return Vec::new(), // repete o par From/To
        EventKind::Modify(ModifyKind::Name(_)) => NodeEventType::Rename,
        EventKind::Modify(_) | EventKind::Any | EventKind::Other => NodeEventType::Change,
        EventKind::Access(_) => return Vec::new(),
    };
    let watched = Path::new(real_path);
    event
        .paths
        .iter()
        .filter_map(|path| {
            let name = if path == watched || !is_directory {
                basename(real_path) // um evento do próprio caminho observado (o `DELETE_SELF`)
            } else {
                path.strip_prefix(watched).ok()?.to_string_lossy().into_owned()
            };
            Some((event_type, name))
        })
        .collect()
}

#[derive(Clone)]
struct StatError {
    not_found: bool,
    message: String,
}

/// `promises.stat(path)` e `stat.isDirectory()`.
fn stat_is_directory(path: &str) -> Result<bool, StatError> {
    std::fs::metadata(path).map(|m| m.is_dir()).map_err(|e| StatError { not_found: e.kind() == std::io::ErrorKind::NotFound, message: e.to_string() })
}

/// O que vale por uma rodada de `doWatchWithNodeJS`: o watcher é recriado de dentro do próprio handler em
/// alguns casos, então cada geração tem o seu estado.
struct Generation {
    real_path: String,
    path_basename: String,
    is_directory: bool,
    /// O `cts` local de `doWatchWithNodeJS`.
    cts: CancellationTokenSource,
    /// `watcherDisposables`: separado porque o watcher é recriado de dentro do handler.
    watcher_disposables: Rc<DisposableStore>,
    folder_children: RefCell<HashSet<String>>,
    map_path_to_stat_disposable: RefCell<HashMap<String, DisposableRef>>,
    active_watch: Rc<MutableDisposable<DisposableStore>>,
    /// Vira `true` quando os listeners entram (depois do `readdir`).
    listening: Cell<bool>,
}

pub struct NodeJsFileWatcherLibrary {
    this: Weak<NodeJsFileWatcherLibrary>,
    request: WatchRequest,
    recursive_watcher: Option<Rc<dyn RecursiveWatcherWithSubscribe>>,
    on_did_watch_fail: Option<Rc<dyn Fn()>>,
    on_log_message: Option<Rc<dyn Fn(LogMessage)>>,
    verbose_logging: Cell<bool>,

    store: Rc<DisposableStore>,
    /// Reduce likelyhood of spam from file events via throttling. These numbers are a bit more aggressive
    /// compared to the recursive watcher because we can have many individual node.js watchers per request.
    /// (https://github.com/microsoft/vscode/issues/124723)
    throttled_file_changes_emitter: Rc<ThrottledWorker<FileChange>>,
    /// Aggregate file changes over FILE_CHANGES_HANDLER_DELAY to coalesce events and reduce spam.
    file_changes_aggregator: Rc<RunOnceWorker<FileChange>>,

    excludes: Vec<ParsedPattern>,
    includes: Option<Vec<ParsedPattern>>,
    filter: Option<FileChangeFilter>,

    cts: CancellationTokenSource,
    real_path: RefCell<Option<Promise<String>>>,
    ready: RefCell<Option<Promise<()>>>,
    is_reusing_recursive_watcher: Cell<bool>,
    did_fail: Cell<bool>,
}

impl NodeJsFileWatcherLibrary {
    pub fn new(
        request: WatchRequest,
        recursive_watcher: Option<Rc<dyn RecursiveWatcherWithSubscribe>>,
        on_did_files_change: Rc<dyn Fn(Vec<FileChange>)>,
        on_did_watch_fail: Option<Rc<dyn Fn()>>,
        on_log_message: Option<Rc<dyn Fn(LogMessage)>>,
        verbose_logging: bool,
    ) -> Rc<Self> {
        let ignore_case = !is_linux();
        let excludes = parse_watcher_patterns(&request.path, &string_patterns(&request.excludes), ignore_case);
        let includes = request.includes.as_ref().map(|includes| parse_watcher_patterns(&request.path, includes, ignore_case));
        // filtering is only enabled when correlating because watchers are otherwise potentially reused
        let filter = if request.has_correlation() { request.filter } else { None };

        let store = DisposableStore::new();
        let throttled_file_changes_emitter = store.add_rc(ThrottledWorker::new(
            ThrottledWorkerOptions {
                max_work_chunk_size: 100, // only process up to 100 changes at once before...
                throttle_delay: 200,      // ...resting for 200ms until we process events again...
                max_buffered_work: Some(10000), // ...but never buffering more than 10000 events in memory
                wait_throttle_delay_between_work_units: false,
            },
            move |events| on_did_files_change(events),
        ));

        let library = Rc::new_cyclic(|this: &Weak<Self>| {
            let weak = this.clone();
            let file_changes_aggregator = store.add_rc(RunOnceWorker::new(
                move |events| {
                    if let Some(this) = weak.upgrade() {
                        this.handle_file_changes(events);
                    }
                },
                FILE_CHANGES_HANDLER_DELAY,
            ));
            Self {
                this: this.clone(),
                request,
                recursive_watcher,
                on_did_watch_fail,
                on_log_message,
                verbose_logging: Cell::new(verbose_logging),
                store,
                throttled_file_changes_emitter,
                file_changes_aggregator,
                excludes,
                includes,
                filter,
                cts: CancellationTokenSource::default(),
                real_path: RefCell::new(None),
                ready: RefCell::new(None),
                is_reusing_recursive_watcher: Cell::new(false),
                did_fail: Cell::new(false),
            }
        });
        let ready = library.watch();
        *library.ready.borrow_mut() = Some(ready);
        library
    }

    /// `ready`.
    pub fn ready(&self) -> Promise<()> {
        self.ready.borrow().clone().unwrap_or_else(|| Promise::resolved(()))
    }

    /// `isReusingRecursiveWatcher`.
    pub fn is_reusing_recursive_watcher(&self) -> bool {
        self.is_reusing_recursive_watcher.get()
    }

    /// `failed`.
    pub fn failed(&self) -> bool {
        self.did_fail.get()
    }

    fn me(&self) -> Rc<Self> {
        self.this.upgrade().expect("o watcher vive enquanto é chamado")
    }

    fn cancelled(&self) -> bool {
        self.cts.token().is_cancellation_requested()
    }

    /// `realPath` (um `Lazy` assíncrono): this property is intentionally `Lazy` and not using `realcase()`
    /// as the counterpart in the recursive watcher because of the amount of paths this watcher is dealing
    /// with. We try as much as possible to avoid even needing `realpath()` if we can because even that
    /// method does an `lstat()` per segment of the path.
    fn real_path(&self) -> Promise<String> {
        if let Some(promise) = self.real_path.borrow().clone() {
            return promise;
        }
        let this = self.me();
        let promise = async_fn(async move {
            let mut result = this.request.path.clone();
            if let Some(real) = Promise::resolved(real_path(&this.request.path)).await? {
                if this.request.path != real {
                    this.trace(&format!("correcting a path to watch that seems to be a symbolic link (original: {}, real: {real})", this.request.path));
                }
                result = real;
            }
            Ok(result)
        });
        *self.real_path.borrow_mut() = Some(promise.clone());
        promise
    }

    fn watch(self: &Rc<Self>) -> Promise<()> {
        let this = self.clone();
        async_fn(async move {
            let outcome: Result<(), StatError> = async {
                let is_directory = Promise::resolved(stat_is_directory(&this.request.path)).await.map_err(|e| StatError { not_found: false, message: e.to_string() })?;
                let is_directory = is_directory?;

                if this.cancelled() {
                    return Ok(());
                }

                let watch = this.do_watch(is_directory);
                then_register_or_dispose(&watch, this.store.clone())
                    .await
                    .map_err(|e| StatError { not_found: false, message: e.to_string() })?;
                Ok(())
            }
            .await;

            if let Err(error) = outcome {
                if !error.not_found {
                    this.error(&error.message);
                } else {
                    this.trace(&format!("ignoring a path for watching who's stat info failed to resolve: {} (error: {})", this.request.path, error.message));
                }

                this.notify_watch_failed();
            }
            Ok(())
        })
    }

    fn notify_watch_failed(&self) {
        self.did_fail.set(true);

        if let Some(on_did_watch_fail) = &self.on_did_watch_fail {
            on_did_watch_fail();
        }
    }

    /// `doWatch(isDirectory)`: o `MutableDisposable` que guarda o watch ativo.
    fn do_watch(self: &Rc<Self>, is_directory: bool) -> Promise<DisposableRef> {
        let this = self.clone();
        async_fn(async move {
            let active_watch: Rc<MutableDisposable<DisposableStore>> = MutableDisposable::new();
            match this.do_watch_generation(is_directory, active_watch.clone()).await {
                Ok(()) => Ok(active_watch as DisposableRef),
                Err(error) => {
                    active_watch.dispose();
                    Err(error)
                }
            }
        })
    }

    fn do_watch_generation(self: &Rc<Self>, is_directory: bool, active_watch: Rc<MutableDisposable<DisposableStore>>) -> Promise<()> {
        let this = self.clone();
        async_fn(async move {
            let disposables = DisposableStore::new();
            active_watch.set(Some(disposables.clone()));
            if !active_watch.value().is_some_and(|current| Rc::ptr_eq(&current, &disposables)) {
                disposables.dispose();
                return Ok(());
            }

            if this.do_watch_with_existing_watcher(is_directory, &disposables, &active_watch) {
                this.trace(&format!("reusing an existing recursive watcher for {}", this.request.path));
                this.is_reusing_recursive_watcher.set(true);
            } else {
                this.is_reusing_recursive_watcher.set(false);
                this.do_watch_with_node_js(is_directory, disposables, active_watch).await?;
            }
            Ok(())
        })
    }

    fn do_watch_with_existing_watcher(
        self: &Rc<Self>,
        is_directory: bool,
        disposables: &Rc<DisposableStore>,
        active_watch: &Rc<MutableDisposable<DisposableStore>>,
    ) -> bool {
        if is_directory {
            // Recursive watcher re-use is currently not enabled for when
            // folders are watched. this is because the dispatching in the
            // recursive watcher for non-recurive requests is optimized for
            // file changes  where we really only match on the exact path
            // and not child paths.
            return false;
        }

        let Some(recursive_watcher) = &self.recursive_watcher else { return false };
        let resource = file_uri(&self.request.path);
        let this = self.clone();
        let store = disposables.clone();
        let active_watch = active_watch.clone();
        let subscription = recursive_watcher.subscribe(
            &self.request.path,
            Rc::new(move |error, change| {
                if store.is_disposed() {
                    return; // return early if already disposed
                }

                if error {
                    let _ = this.do_watch_generation(is_directory, active_watch.clone());
                } else if let Some(change) = change
                    && (change.c_id.is_some() || this.request.correlation_id.is_some())
                {
                    // Re-emit this change with the correlation id of the request
                    // so that the client can correlate the event with the request
                    // properly. Without correlation, we do not have to do that
                    // because the event will appear on the global listener already.
                    this.on_file_change(
                        FileChange { change_type: change.change_type, resource: resource.clone(), c_id: this.request.correlation_id },
                        true, /* skip excludes/includes (file is explicitly watched) */
                    );
                }
            }),
        );

        match subscription {
            Some(subscription) => {
                disposables.add(subscription);
                true
            }
            None => false,
        }
    }

    fn do_watch_with_node_js(
        self: &Rc<Self>,
        is_directory: bool,
        disposables: Rc<DisposableStore>,
        active_watch: Rc<MutableDisposable<DisposableStore>>,
    ) -> Promise<()> {
        let this = self.clone();
        async_fn(async move {
            let real_path = this.real_path().await?;

            if this.cancelled() || disposables.is_disposed() {
                return Ok(());
            }

            // macOS: watching samba shares can crash VSCode so we do
            // a simple check for the file path pointing to /Volumes
            // (https://github.com/microsoft/vscode/issues/106879)
            if cfg!(target_os = "macos") && is_equal_or_parent(&real_path, "/Volumes/") {
                this.error(&format!(
                    "Refusing to watch {real_path} for changes using fs.watch() for possibly being a network share where watching is unreliable and unstable."
                ));
                return Ok(());
            }

            let cts = CancellationTokenSource::new(Some(&this.cts.token()));
            {
                let cts = cts.clone();
                disposables.add(to_disposable(move || cts.dispose_with(true)));
            }

            let watcher_disposables = DisposableStore::new(); // we need a separate disposable store because we re-create the watcher from within in some cases
            disposables.add_rc(watcher_disposables.clone());

            let generation = Rc::new(Generation {
                real_path: real_path.clone(),
                path_basename: basename(&real_path),
                is_directory,
                cts,
                watcher_disposables: watcher_disposables.clone(),
                folder_children: RefCell::new(HashSet::new()),
                map_path_to_stat_disposable: RefCell::new(HashMap::new()),
                active_watch,
                listening: Cell::new(false),
            });

            // Creating watcher can fail with an exception
            let handler = {
                let this = this.clone();
                let generation = generation.clone();
                Rc::new(move |event: FsEvent| this.on_fs_event(&generation, event))
            };
            match FsWatch::start(Path::new(&real_path), false, None, Vec::new(), handler) {
                Ok(watch) => {
                    watcher_disposables.add_rc(watch);
                }
                Err(error) => {
                    if generation.cts.token().is_cancellation_requested() {
                        return Ok(());
                    }

                    this.error(&format!("Failed to watch {real_path} for changes using fs.watch() ({error})"));

                    this.notify_watch_failed();
                    return Ok(());
                }
            }

            this.trace(&format!("Started watching: '{real_path}'"));

            // Folder: resolve children to emit proper events
            if is_directory {
                match Promise::resolved(read_dir_names(&real_path).map_err(|e| e.to_string())).await? {
                    Ok(children) => generation.folder_children.borrow_mut().extend(children),
                    Err(error) => this.error(&error),
                }
            }

            if generation.cts.token().is_cancellation_requested() {
                return Ok(());
            }

            {
                let generation = generation.clone();
                watcher_disposables.add(to_disposable(move || {
                    let pending: Vec<DisposableRef> = generation.map_path_to_stat_disposable.borrow_mut().drain().map(|(_, d)| d).collect();
                    for disposable in pending {
                        disposable.dispose();
                    }
                }));
            }
            generation.listening.set(true);
            Ok(())
        })
    }

    fn on_fs_event(self: &Rc<Self>, generation: &Rc<Generation>, event: FsEvent) {
        if !generation.listening.get() {
            return; // o `fs.watch` do Node só entrega aos listeners já registrados
        }
        match event {
            FsEvent::Error(error) => {
                if generation.cts.token().is_cancellation_requested() {
                    return;
                }

                self.error(&format!("Failed to watch {} for changes using fs.watch() ({error})", generation.real_path));

                self.notify_watch_failed();
            }
            FsEvent::Event(event) => {
                for (event_type, raw) in node_events(&event, &generation.real_path, generation.is_directory) {
                    self.on_raw_change(generation, event_type, raw);
                }
            }
        }
    }

    /// O listener de `watcher.on('change', (type, raw) => ...)`.
    fn on_raw_change(self: &Rc<Self>, generation: &Rc<Generation>, event_type: NodeEventType, raw: String) {
        if generation.cts.token().is_cancellation_requested() {
            return; // ignore if already disposed
        }

        if self.verbose_logging.get() {
            let label = if event_type == NodeEventType::Rename { "rename" } else { "change" };
            self.trace_with_correlation(&format!("[raw] [\"{label}\"] {raw}"));
        }

        // Normalize file name
        let mut changed_file_name = raw; // https://github.com/microsoft/vscode/issues/38191 (sem nome, ignora)
        if cfg!(target_os = "macos") {
            // Mac: uses NFD unicode form on disk, but we want NFC
            // See also https://github.com/nodejs/node/issues/2165
            changed_file_name = normalize_nfc(&changed_file_name);
        }

        if changed_file_name.is_empty() {
            return; // ignore unexpected events
        }

        let request_resource = file_uri(&self.request.path);
        if generation.is_directory {
            self.on_folder_change(generation, event_type, changed_file_name, request_resource);
        } else {
            self.on_file_change_raw(generation, event_type, &changed_file_name, request_resource);
        }
    }

    fn on_folder_change(self: &Rc<Self>, generation: &Rc<Generation>, event_type: NodeEventType, changed_file_name: String, request_resource: je_base::uri::URI) {
        // Folder child added/deleted
        if event_type == NodeEventType::Rename {
            // Windows repeatedly reports the full watched path when a folder is deleted.
            let is_absolute_watched_path_change = is_absolute(&changed_file_name) && is_equal(&basename(&changed_file_name), &generation.path_basename);

            // Cancel any previous stats for this file if existing
            let pending_stat = generation.map_path_to_stat_disposable.borrow_mut().remove(&changed_file_name);
            if let Some(pending_stat) = pending_stat {
                if is_absolute_watched_path_change {
                    generation.map_path_to_stat_disposable.borrow_mut().insert(changed_file_name, pending_stat);
                    return;
                }
                pending_stat.dispose();
            }

            // Wait a bit and try see if the file still exists on disk
            // to decide on the resulting event
            let this = self.clone();
            let task_generation = generation.clone();
            let name = changed_file_name.clone();
            let timeout_handle = set_timeout(
                move || this.after_folder_rename_delay(&task_generation, name, request_resource, is_absolute_watched_path_change),
                FILE_DELETE_HANDLER_DELAY,
            );
            generation.map_path_to_stat_disposable.borrow_mut().insert(changed_file_name, to_disposable(move || clear_timeout(timeout_handle)));
        }
        // Folder child changed
        else {
            // Figure out the correct event type: if this is the
            // first time we see this child, it can only be added
            let change_type = if generation.folder_children.borrow().contains(&changed_file_name) {
                FileChangeType::Updated
            } else {
                generation.folder_children.borrow_mut().insert(changed_file_name.clone());
                FileChangeType::Added
            };

            self.on_file_change(FileChange { change_type, resource: file_uri(&join(&[&self.request.path, &changed_file_name])), c_id: self.request.correlation_id }, false);
        }
    }

    fn after_folder_rename_delay(
        self: &Rc<Self>,
        generation: &Rc<Generation>,
        changed_file_name: String,
        request_resource: je_base::uri::URI,
        is_absolute_watched_path_change: bool,
    ) {
        generation.map_path_to_stat_disposable.borrow_mut().remove(&changed_file_name);

        // Depending on the OS the watcher runs on, there
        // is different behaviour for when the watched
        // folder path is being deleted:
        //
        // -   macOS: not reported but events continue to
        //            work even when the folder is brought
        //            back, though it seems every change
        //            to a file is reported as "rename"
        // -   Linux: "rename" event is reported with the
        //            name of the folder and events stop
        //            working
        // - Windows: repeated "rename" events report the
        //            full watched path
        //
        if is_absolute_watched_path_change {
            generation.active_watch.clear();
            let watched_path_exists = exists(&generation.real_path);
            if self.cancelled() {
                return;
            }
            if watched_path_exists {
                let _ = self.do_watch_generation(true, generation.active_watch.clone());
            } else {
                self.on_watched_path_deleted(&request_resource);
            }

            return;
        }

        if is_equal(&changed_file_name, &generation.path_basename) && !exists(&generation.real_path) {
            generation.active_watch.clear();
            self.on_watched_path_deleted(&request_resource);

            return;
        }

        if generation.cts.token().is_cancellation_requested() {
            return;
        }

        // In order to properly detect renames on a case-insensitive
        // file system, we need to use `existsChildStrictCase` helper
        // because otherwise we would wrongly assume a file exists
        // when it was renamed to same name but different case.
        let file_exists = self.exists_child_strict_case(&join(&[&generation.real_path, &changed_file_name]));

        if generation.cts.token().is_cancellation_requested() {
            return; // ignore if disposed by now
        }

        // Figure out the correct event type:
        // File Exists: either 'added' or 'updated' if known before
        // File Does not Exist: always 'deleted'
        let change_type = if file_exists {
            if generation.folder_children.borrow().contains(&changed_file_name) {
                FileChangeType::Updated
            } else {
                generation.folder_children.borrow_mut().insert(changed_file_name.clone());
                FileChangeType::Added
            }
        } else {
            generation.folder_children.borrow_mut().remove(&changed_file_name);
            FileChangeType::Deleted
        };

        self.on_file_change(
            FileChange { change_type, resource: file_uri(&join(&[&self.request.path, &changed_file_name])), c_id: self.request.correlation_id },
            false,
        );
    }

    fn on_file_change_raw(self: &Rc<Self>, generation: &Rc<Generation>, event_type: NodeEventType, changed_file_name: &str, request_resource: je_base::uri::URI) {
        // File added/deleted
        if event_type == NodeEventType::Rename || !is_equal(changed_file_name, &generation.path_basename) {
            // Depending on the OS the watcher runs on, there
            // is different behaviour for when the watched
            // file path is being deleted:
            //
            // -   macOS: "rename" event is reported and events
            //            stop working
            // -   Linux: "rename" event is reported and events
            //            stop working
            // - Windows: "rename" event is reported and events
            //            continue to work when file is restored
            //
            // As opposed to folder watching, we re-attach the
            // watcher after brief timeout to support "atomic save"
            // operations where a tool may decide to delete a file
            // and then create it with the updated contents.
            //
            // Different to folder watching, we emit a delete event
            // though we never detect when the file is brought back
            // because the watcher is disposed then.
            let this = self.clone();
            let task_generation = generation.clone();
            let timeout_handle = set_timeout(
                move || {
                    let file_exists = exists(&task_generation.real_path);

                    if task_generation.cts.token().is_cancellation_requested() {
                        return; // ignore if disposed by now
                    }

                    // File still exists, so emit as change event and reapply the watcher
                    if file_exists {
                        task_generation.active_watch.clear();
                        this.on_file_change(
                            FileChange { change_type: FileChangeType::Updated, resource: request_resource, c_id: this.request.correlation_id },
                            true, /* skip excludes/includes (file is explicitly watched) */
                        );

                        let _ = this.do_watch_generation(false, task_generation.active_watch.clone());
                    }
                    // File seems to be really gone, so emit a deleted and failed event
                    else {
                        task_generation.active_watch.clear();
                        this.on_watched_path_deleted(&request_resource);
                    }
                },
                FILE_DELETE_HANDLER_DELAY,
            );

            // Very important to dispose the watcher which now points to a stale inode
            // and wire in a new disposable that tracks our timeout that is installed
            generation.watcher_disposables.clear();
            generation.watcher_disposables.add(to_disposable(move || clear_timeout(timeout_handle)));
        }
        // File changed
        else {
            self.on_file_change(
                FileChange { change_type: FileChangeType::Updated, resource: request_resource, c_id: self.request.correlation_id },
                true, /* skip excludes/includes (file is explicitly watched) */
            );
        }
    }

    fn on_watched_path_deleted(&self, resource: &je_base::uri::URI) {
        self.warn("Watcher shutdown because watched path got deleted");

        // Emit events and flush in case the watcher gets disposed
        self.on_file_change(FileChange { change_type: FileChangeType::Deleted, resource: resource.clone(), c_id: self.request.correlation_id }, true /* skip excludes/includes (file is explicitly watched) */);
        self.file_changes_aggregator.flush();

        self.notify_watch_failed();
    }

    fn on_file_change(&self, event: FileChange, skip_include_exclude_checks: bool) {
        if self.cancelled() {
            return;
        }

        let path = fs_path(&event.resource);

        // Logging
        if self.verbose_logging.get() {
            self.trace_with_correlation(&format!("{} {path}", change_type_label(event.change_type)));
        }

        // Add to aggregator unless excluded or not included (not if explicitly disabled)
        if !skip_include_exclude_checks && self.excludes.iter().any(|exclude| exclude.matches(&path, None)) {
            if self.verbose_logging.get() {
                self.trace_with_correlation(&format!(" >> ignored (excluded) {path}"));
            }
        } else if !skip_include_exclude_checks
            && self.includes.as_ref().is_some_and(|includes| !includes.is_empty() && !includes.iter().any(|include| include.matches(&path, None)))
        {
            if self.verbose_logging.get() {
                self.trace_with_correlation(&format!(" >> ignored (not included) {path}"));
            }
        } else {
            self.file_changes_aggregator.work(event);
        }
    }

    fn handle_file_changes(&self, file_changes: Vec<FileChange>) {
        // Coalesce events: merge events of same kind
        let coalesced_file_changes = coalesce_events(file_changes);

        // Filter events: based on request filter property
        let mut filtered_events = Vec::new();
        for event in coalesced_file_changes {
            if is_filtered(&event, self.filter) {
                if self.verbose_logging.get() {
                    self.trace_with_correlation(&format!(" >> ignored (filtered) {}", fs_path(&event.resource)));
                }

                continue;
            }

            filtered_events.push(event);
        }

        if filtered_events.is_empty() {
            return;
        }

        // Logging
        if self.verbose_logging.get() {
            for event in &filtered_events {
                self.trace_with_correlation(&format!(" >> normalized {} {}", change_type_label(event.change_type), fs_path(&event.resource)));
            }
        }

        // Broadcast to clients via throttled emitter
        let worked = self.throttled_file_changes_emitter.work(&filtered_events);

        // Logging
        let most_recent = fs_path(&filtered_events[0].resource);
        if !worked {
            self.warn(&format!(
                "started ignoring events due to too many file change events at once (incoming: {}, most recent change: {most_recent}). Use 'files.watcherExclude' setting to exclude folders with lots of changing files (e.g. compilation output).",
                filtered_events.len()
            ));
        } else if self.throttled_file_changes_emitter.pending() > 0 {
            self.trace(&format!(
                "started throttling events due to large amount of file change events at once (pending: {}, most recent change: {most_recent}). Use 'files.watcherExclude' setting to exclude folders with lots of changing files (e.g. compilation output).",
                self.throttled_file_changes_emitter.pending()
            ));
        }
    }

    fn exists_child_strict_case(&self, path: &str) -> bool {
        if is_linux() {
            return exists(path);
        }

        match read_dir_names(&dirname(path)) {
            Ok(children) => {
                let path_basename = basename(path);
                children.contains(&path_basename)
            }
            Err(error) => {
                self.trace(&error.to_string());
                false
            }
        }
    }

    /// `setVerboseLogging(verboseLogging)`.
    pub fn set_verbose_logging(&self, verbose_logging: bool) {
        self.verbose_logging.set(verbose_logging);
    }

    fn log(&self, message_type: LogMessageType, message: &str) {
        if !self.cancelled()
            && let Some(on_log_message) = &self.on_log_message
        {
            on_log_message(LogMessage { message_type, message: format!("[File Watcher (node.js)] {message}") });
        }
    }

    fn error(&self, error: &str) {
        self.log(LogMessageType::Error, error);
    }

    fn warn(&self, message: &str) {
        self.log(LogMessageType::Warn, message);
    }

    fn trace(&self, message: &str) {
        if self.verbose_logging.get() {
            self.log(LogMessageType::Trace, message);
        }
    }

    fn trace_with_correlation(&self, message: &str) {
        if self.verbose_logging.get() {
            match self.request.correlation_id {
                Some(id) => self.trace(&format!("{message} <{id}> ")),
                None => self.trace(message),
            }
        }
    }
}

impl IDisposable for NodeJsFileWatcherLibrary {
    fn dispose(&self) {
        self.cts.dispose_with(true);

        self.store.dispose();
    }
}

/// O `onData` de `watchFileContents`: recebe cada pedaço lido.
pub type OnData = Rc<dyn Fn(&[u8])>;

/// `watchFileContents(path, onData, onReady, token, bufferSize)`: observa `path` e entrega o conteúdo que
/// vai sendo acrescentado, em pedaços, até o `token` ser cancelado.
pub fn watch_file_contents(
    path: &str,
    on_data: OnData,
    on_ready: Rc<dyn Fn()>,
    token: CancellationToken,
    buffer_size: usize,
) -> Promise<()> {
    use std::io::Read;
    let path = path.to_string();
    async_fn(async move {
        let handle = Promise::resolved(std::fs::File::open(&path).map(|f| Rc::new(RefCell::new(Some(f)))).map_err(|e| e.to_string()))
            .await?
            .map_err(Error::new)?;

        let cts = CancellationTokenSource::new(Some(&token));

        let error: Rc<RefCell<Option<Error>>> = Rc::new(RefCell::new(None));
        let is_reading = Rc::new(Cell::new(false));

        let request = WatchRequest { path: path.clone(), recursive: false, excludes: Vec::new(), includes: None, correlation_id: None, filter: None, polling_interval: None };
        let on_changes: Rc<dyn Fn(Vec<FileChange>)> = {
            let (handle, cts, error, is_reading) = (handle.clone(), cts.clone(), error.clone(), is_reading.clone());
            Rc::new(move |changes| {
                for change in changes {
                    if change.change_type == FileChangeType::Updated {
                        if is_reading.get() {
                            return; // return early if we are already reading the output
                        }

                        is_reading.set(true);

                        // Consume the new contents of the file until finished
                        // everytime there is a change event signalling a change
                        let mut buffer = vec![0u8; buffer_size];
                        while !cts.token().is_cancellation_requested() {
                            let read = match handle.borrow_mut().as_mut() {
                                Some(file) => file.read(&mut buffer),
                                None => Ok(0),
                            };
                            match read {
                                Ok(0) => break,
                                Ok(bytes_read) => {
                                    if cts.token().is_cancellation_requested() {
                                        break;
                                    }
                                    on_data(&buffer[..bytes_read]);
                                }
                                Err(e) => {
                                    *error.borrow_mut() = Some(Error::new(e.to_string()));
                                    cts.dispose_with(true);
                                    break;
                                }
                            }
                        }
                        is_reading.set(false);
                    }
                }
            })
        };
        let watcher = NodeJsFileWatcherLibrary::new(request, None, on_changes, None, None, false);

        watcher.ready().await?;
        on_ready();

        let (promise, resolver): (Promise<()>, Resolver<()>) = Promise::new();
        let watcher_to_dispose = watcher.clone();
        let handle_to_close = handle.clone();
        let error_to_report = error.clone();
        let _listener = cts.token().on_cancellation_requested().listen(move |_| {
            watcher_to_dispose.dispose();

            drop(handle_to_close.borrow_mut().take()); // close

            match error_to_report.borrow_mut().take() {
                Some(error) => resolver.reject(error),
                None => resolver.resolve(()),
            }
        });
        promise.await
    })
}
