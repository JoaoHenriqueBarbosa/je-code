//! Porte de `src/vs/platform/files/node/watcher/baseWatcher.ts`: o que os dois watchers de disco (o
//! recursivo e o não recursivo) têm em comum, que é suspender os pedidos que falham, monitorá-los até o
//! caminho voltar a existir e retomá-los.
//!
//! `BaseWatcher` do original é uma classe abstrata; aqui o estado compartilhado é [`BaseWatcher`] e os
//! membros abstratos (`doWatch`, `recursiveWatcher`, `trace`, `warn`) são o trait [`WatcherBackend`], que
//! o watcher concreto implementa e entrega ao `BaseWatcher` como `Weak`.
//!
//! Divergências:
//! - `fs.watchFile` (o monitoramento do pedido suspenso, que faz polling de `stat`) vira um timer do loop
//!   que chama `std::fs::metadata` a cada `suspended_poll_interval` ms, com a mesma regra de disparo do
//!   Node: o callback roda quando o `stat` muda e, se o caminho não existe na partida, o monitor já nasce
//!   sabendo que ele falta (o callback inicial com zeros do Node).
//! - O `hash(request)` do identificador de pedido sem correlação é um hash de `std` sobre os campos do
//!   pedido; o valor não é observável, só precisa ser estável e distinguir pedidos.
//! - Os ganchos que a subclasse de teste do original acrescenta (`suspendedWatchRequestPollingInterval`,
//!   `getUpdateWatchersDelay`, `onWatchFail`, e o `doWatch` que espera o `ready` e dispara `onDidWatch`)
//!   são [`BaseWatcherOptions`] e os eventos públicos `on_did_watch_fail` e `on_did_watch`.

use std::cell::{Cell, RefCell};
use std::collections::HashSet;
use std::hash::{Hash, Hasher};
use std::rc::{Rc, Weak};
use std::time::SystemTime;

use je_base::async_::{set_interval, clear_interval, ThrottledDelayer};
use je_base::errors::on_unexpected_error;
use je_base::event::Emitter;
use je_base::lifecycle::{DisposableMap, DisposableStore, IDisposable, to_disposable};
use je_base::map::OrderedMap;
use je_base::promise::{DeferredPromise, Promise};

use crate::files::files::{FileChange, FileChangeType};
use crate::files::watcher::{LogMessage, LogMessageType, RecursiveWatcherWithSubscribe, WatchRequest, request_filter_to_string};

use super::fs_util::{file_uri, fs_path};

/// O retorno de `isSuspended`: `'polling' | boolean` no original.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Suspended {
    /// `false`: o pedido está sendo observado.
    No,
    /// `true`: suspenso, e um watcher recursivo existente avisa quando o caminho volta.
    Yes,
    /// `'polling'`: suspenso, e um polling de `stat` avisa quando o caminho volta.
    Polling,
}

#[derive(Clone)]
struct SuspendedWatchRequest {
    id: i64,
    correlation_id: Option<i64>,
    path: String,
}

/// Os membros abstratos de `BaseWatcher`.
pub trait WatcherBackend {
    /// `doWatch(requests)`: os pedidos não suspensos, que o watcher concreto passa a observar.
    fn do_watch(&self, requests: Vec<WatchRequest>) -> Promise<()>;

    /// Espera os watchers que `do_watch` iniciou ficarem prontos (o `await watcher.ready` que a subclasse
    /// de teste do original faz depois do `super.doWatch`).
    fn when_ready(&self) -> Promise<()>;

    /// `recursiveWatcher`: o watcher recursivo que pode ser reaproveitado, se há.
    fn recursive_watcher(&self) -> Option<Rc<dyn RecursiveWatcherWithSubscribe>>;

    fn trace(&self, message: &str);
    fn warn(&self, message: &str);
}

/// Os valores que a subclasse de teste do original sobrescreve.
#[derive(Clone, Copy, Debug)]
pub struct BaseWatcherOptions {
    /// `getUpdateWatchersDelay()`: 800 ms.
    pub update_watchers_delay: u64,
    /// `suspendedWatchRequestPollingInterval`: 5007 ms, o padrão do Node.
    pub suspended_poll_interval: u64,
    /// Faz o `watch()` esperar os watchers ficarem prontos e dispara `on_did_watch` (o `doWatch` da
    /// subclasse de teste do original).
    pub join_ready: bool,
}

impl Default for BaseWatcherOptions {
    fn default() -> Self {
        Self { update_watchers_delay: 800, suspended_poll_interval: 5007, join_ready: false }
    }
}

pub struct BaseWatcher {
    this: Weak<BaseWatcher>,
    backend: Weak<dyn WatcherBackend>,
    options: BaseWatcherOptions,

    pub on_did_change_file: Rc<Emitter<Vec<FileChange>>>,
    pub on_did_log_message: Rc<Emitter<LogMessage>>,
    /// `_onDidWatchFail`: dispara quando um pedido falha (os watchers concretos disparam, o base escuta).
    pub on_did_watch_fail: Rc<Emitter<WatchRequest>>,
    /// Dispara quando um `do_watch` terminou, com `join_ready` ligado.
    pub on_did_watch: Rc<Emitter<()>>,

    correlated_watch_requests: RefCell<OrderedMap<i64, WatchRequest>>,
    non_correlated_watch_requests: RefCell<OrderedMap<i64, WatchRequest>>,
    suspended_watch_requests: Rc<DisposableMap<i64, DisposableStore>>,
    suspended_watch_requests_with_polling: RefCell<HashSet<i64>>,
    update_watchers_delayer: ThrottledDelayer<()>,
    join_watch: RefCell<DeferredPromise<()>>,
    verbose_logging: Cell<bool>,
    disposables: Rc<DisposableStore>,
}

impl BaseWatcher {
    pub fn new(backend: Weak<dyn WatcherBackend>, options: BaseWatcherOptions) -> Rc<Self> {
        let disposables = DisposableStore::new();
        let on_did_change_file = disposables.add_rc(Emitter::new());
        let on_did_log_message = disposables.add_rc(Emitter::new());
        let on_did_watch_fail = disposables.add_rc(Emitter::new());
        let on_did_watch = disposables.add_rc(Emitter::new());
        let suspended_watch_requests = disposables.add_rc(DisposableMap::new());
        let join_watch = DeferredPromise::new();
        let base = Rc::new_cyclic(|this| Self {
            this: this.clone(),
            backend,
            options,
            on_did_change_file,
            on_did_log_message,
            on_did_watch_fail,
            on_did_watch,
            correlated_watch_requests: RefCell::new(OrderedMap::new()),
            non_correlated_watch_requests: RefCell::new(OrderedMap::new()),
            suspended_watch_requests,
            suspended_watch_requests_with_polling: RefCell::new(HashSet::new()),
            update_watchers_delayer: ThrottledDelayer::new(options.update_watchers_delay),
            join_watch: RefCell::new(join_watch),
            verbose_logging: Cell::new(false),
            disposables: disposables.clone(),
        });

        let weak = base.this.clone();
        disposables.add(base.on_did_watch_fail.event().listen(move |request| {
            if let Some(base) = weak.upgrade() {
                base.suspend_watch_request(SuspendedWatchRequest {
                    id: compute_id(request),
                    correlation_id: request.correlation_id,
                    path: request.path.clone(),
                });
            }
        }));
        base
    }

    pub fn options(&self) -> &BaseWatcherOptions {
        &self.options
    }

    pub fn verbose_logging(&self) -> bool {
        self.verbose_logging.get()
    }

    /// `setVerboseLogging(enabled)`.
    pub fn set_verbose_logging(&self, enabled: bool) -> Promise<()> {
        self.verbose_logging.set(enabled);
        Promise::resolved(())
    }

    /// `watch(requests)`.
    pub fn watch(&self, requests: Vec<WatchRequest>) -> Promise<()> {
        if !self.join_watch.borrow().is_settled() {
            self.join_watch.borrow().complete(());
        }
        *self.join_watch.borrow_mut() = DeferredPromise::new();

        self.correlated_watch_requests.borrow_mut().clear();
        self.non_correlated_watch_requests.borrow_mut().clear();

        // Figure out correlated vs. non-correlated requests
        for request in requests {
            match request.correlation_id {
                Some(correlation_id) => self.correlated_watch_requests.borrow_mut().insert(correlation_id, request),
                None => self.non_correlated_watch_requests.borrow_mut().insert(compute_id(&request), request),
            };
        }

        // Remove all suspended watch requests that are no longer watched
        for id in self.suspended_watch_requests.keys() {
            if !self.non_correlated_watch_requests.borrow().contains_key(&id) && !self.correlated_watch_requests.borrow().contains_key(&id) {
                self.suspended_watch_requests.delete_and_dispose(&id);
                self.suspended_watch_requests_with_polling.borrow_mut().remove(&id);
            }
        }

        let this = self.this.clone();
        self.update_watchers(false /* not delayed */).finally(move || {
            if let Some(this) = this.upgrade() {
                this.join_watch.borrow().complete(());
            }
        })
    }

    fn update_watchers(&self, delayed: bool) -> Promise<()> {
        let mut non_suspended_requests = Vec::new();
        {
            let non_correlated = self.non_correlated_watch_requests.borrow();
            let correlated = self.correlated_watch_requests.borrow();
            for (id, request) in non_correlated.iter().chain(correlated.iter()) {
                if !self.suspended_watch_requests.has(id) {
                    non_suspended_requests.push(request.clone());
                }
            }
        }

        let backend = self.backend.clone();
        let join_ready = self.options.join_ready;
        let on_did_watch = self.on_did_watch.clone();
        let delay = if delayed { self.options.update_watchers_delay } else { 0 };
        self.update_watchers_delayer
            .trigger(
                move |_token| {
                    let Some(backend) = backend.upgrade() else { return Promise::resolved(()) };
                    backend.do_watch(non_suspended_requests).then_promise(move |()| {
                        if join_ready {
                            backend.when_ready().then(move |()| on_did_watch.fire(()))
                        } else {
                            Promise::resolved(())
                        }
                    })
                },
                Some(delay),
            )
            .then_result(|result| {
                if let Err(error) = result {
                    on_unexpected_error(&error);
                }
                Ok(())
            })
    }

    /// `isSuspended(request)`.
    pub fn is_suspended(&self, request: &WatchRequest) -> Suspended {
        let id = compute_id(request);
        if self.suspended_watch_requests_with_polling.borrow().contains(&id) {
            Suspended::Polling
        } else if self.suspended_watch_requests.has(&id) {
            Suspended::Yes
        } else {
            Suspended::No
        }
    }

    fn suspend_watch_request(&self, request: SuspendedWatchRequest) {
        if self.suspended_watch_requests.has(&request.id) {
            return; // already suspended
        }

        let disposables = DisposableStore::new();
        self.suspended_watch_requests.set(request.id, disposables.clone(), false);

        // It is possible that a watch request fails right during watch()
        // phase while other requests succeed. To increase the chance of
        // reusing another watcher for suspend/resume tracking, we await
        // all watch requests having processed.
        let join = self.join_watch.borrow().promise.clone();
        let this = self.this.clone();
        let _ = join.then(move |()| {
            let Some(this) = this.upgrade() else { return };
            if disposables.is_disposed() {
                return;
            }
            this.monitor_suspended_watch_request(&request, &disposables);
            let _ = this.update_watchers(true /* delay this call as we might accumulate many failing watch requests on startup */);
        });
    }

    fn resume_watch_request(&self, request: &SuspendedWatchRequest) {
        self.suspended_watch_requests.delete_and_dispose(&request.id);
        self.suspended_watch_requests_with_polling.borrow_mut().remove(&request.id);

        let _ = self.update_watchers(false);
    }

    fn monitor_suspended_watch_request(&self, request: &SuspendedWatchRequest, disposables: &Rc<DisposableStore>) {
        if self.do_monitor_with_existing_watcher(request, disposables) {
            self.trace(&format!("reusing an existing recursive watcher to monitor {}", request.path));
            self.suspended_watch_requests_with_polling.borrow_mut().remove(&request.id);
        } else {
            self.do_monitor_with_poll(request, disposables);
            self.suspended_watch_requests_with_polling.borrow_mut().insert(request.id);
        }
    }

    fn do_monitor_with_existing_watcher(&self, request: &SuspendedWatchRequest, disposables: &Rc<DisposableStore>) -> bool {
        let Some(recursive_watcher) = self.backend.upgrade().and_then(|b| b.recursive_watcher()) else {
            return false;
        };
        let this = self.this.clone();
        let request_for_callback = request.clone();
        let store = disposables.clone();
        let subscription = recursive_watcher.subscribe(
            &request.path,
            Rc::new(move |error, change| {
                if store.is_disposed() {
                    return; // return early if already disposed
                }
                let Some(this) = this.upgrade() else { return };
                if error {
                    this.monitor_suspended_watch_request(&request_for_callback, &store);
                } else if change.is_some_and(|c| c.change_type == FileChangeType::Added) {
                    this.on_monitored_path_added(&request_for_callback);
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

    /// O `fs.watchFile` do original: polling de `stat` que avisa quando o caminho passa a existir.
    fn do_monitor_with_poll(&self, request: &SuspendedWatchRequest, disposables: &Rc<DisposableStore>) {
        let path_not_found = Rc::new(Cell::new(false));
        let previous = Rc::new(RefCell::new(PathStat::of(&request.path)));
        // O Node chama o listener uma vez, com zeros, quando o `stat` inicial falha com ENOENT.
        path_not_found.set(previous.borrow().is_none());

        let watch_file_callback = {
            let this = self.this.clone();
            let request = request.clone();
            let store = disposables.clone();
            let path_not_found = path_not_found.clone();
            move |current: Option<PathStat>, previous: Option<PathStat>| {
                if store.is_disposed() {
                    return; // return early if already disposed
                }
                let current_path_not_found = current.is_none();
                let previous_path_not_found = previous.is_none();
                let old_path_not_found = path_not_found.get();
                path_not_found.set(current_path_not_found);

                // Watch path created: resume watching request
                if !current_path_not_found && (previous_path_not_found || old_path_not_found)
                    && let Some(this) = this.upgrade()
                {
                    this.on_monitored_path_added(&request);
                }
            }
        };

        self.trace(&format!("starting stat polling on {} (correlationId: {})", request.path, correlation_to_string(request.correlation_id)));
        let path = request.path.clone();
        let handle = set_interval(
            move || {
                let current = PathStat::of(&path);
                let changed = *previous.borrow() != current;
                if changed {
                    let before = previous.replace(current.clone());
                    watch_file_callback(current, before);
                }
            },
            self.options.suspended_poll_interval,
        );

        let this = self.this.clone();
        let request = request.clone();
        disposables.add(to_disposable(move || {
            if let Some(this) = this.upgrade() {
                this.trace(&format!("stopping stat polling on {} (correlationId: {})", request.path, correlation_to_string(request.correlation_id)));
            }
            clear_interval(&handle);
        }));
    }

    fn on_monitored_path_added(&self, request: &SuspendedWatchRequest) {
        self.trace(&format!("detected {} exists again, resuming watcher (correlationId: {})", request.path, correlation_to_string(request.correlation_id)));

        // Emit as event
        let event = FileChange { change_type: FileChangeType::Added, resource: file_uri(&request.path), c_id: request.correlation_id };
        self.on_did_change_file.fire(vec![event.clone()]);
        self.trace_event(&event, request.correlation_id);

        // Resume watching
        self.resume_watch_request(request);
    }

    /// `stop()`.
    pub fn stop(&self) -> Promise<()> {
        self.suspended_watch_requests.clear_and_dispose_all();
        self.suspended_watch_requests_with_polling.borrow_mut().clear();
        Promise::resolved(())
    }

    /// `traceEvent(event, request)`.
    pub fn trace_event(&self, event: &FileChange, correlation_id: Option<i64>) {
        if self.verbose_logging.get() {
            let trace_msg = format!(" >> normalized {} {}", change_type_label(event.change_type), fs_path(&event.resource));
            self.trace_with_correlation(&trace_msg, correlation_id);
        }
    }

    /// `traceWithCorrelation(message, request)`.
    pub fn trace_with_correlation(&self, message: &str, correlation_id: Option<i64>) {
        if self.verbose_logging.get() {
            match correlation_id {
                Some(id) => self.trace(&format!("{message} <{id}> ")),
                None => self.trace(message),
            }
        }
    }

    fn trace(&self, message: &str) {
        if let Some(backend) = self.backend.upgrade() {
            backend.trace(message);
        }
    }

    /// Emite uma mensagem de log (o `_onDidLogMessage.fire` do original).
    pub fn log(&self, message_type: LogMessageType, message: String) {
        self.on_did_log_message.fire(LogMessage { message_type, message });
    }
}

impl IDisposable for BaseWatcher {
    fn dispose(&self) {
        self.update_watchers_delayer.dispose();
        self.disposables.dispose();
    }
}

fn correlation_to_string(correlation_id: Option<i64>) -> String {
    correlation_id.map_or_else(|| "undefined".to_string(), |id| id.to_string())
}

/// `[ADDED]`, `[DELETED]` ou `[CHANGED]`.
pub fn change_type_label(change_type: FileChangeType) -> &'static str {
    match change_type {
        FileChangeType::Added => "[ADDED]",
        FileChangeType::Deleted => "[DELETED]",
        FileChangeType::Updated => "[CHANGED]",
    }
}

/// `computeId(request)`: a correlação, ou um hash do pedido (que não carrega identificador próprio).
pub fn compute_id(request: &WatchRequest) -> i64 {
    if let Some(correlation_id) = request.correlation_id {
        return correlation_id;
    }
    // Requests without correlation do not carry any unique identifier, so we have to
    // come up with one based on the options of the request. This matches what the
    // file service does (vs/platform/files/common/fileService.ts#L1178).
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    request.path.hash(&mut hasher);
    request.recursive.hash(&mut hasher);
    request.excludes.hash(&mut hasher);
    format!("{:?}", request.includes).hash(&mut hasher);
    request.filter.map(|f| f.bits()).hash(&mut hasher);
    request.polling_interval.hash(&mut hasher);
    // Os 32 bits baixos, como o `hash` do original devolve um inteiro de 32 bits.
    i64::from(hasher.finish() as u32 as i32)
}

/// A chave do mapa de watchers: a correlação, ou o caminho (em minúsculas onde a caixa não conta).
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum WatcherKey {
    Path(String),
    Correlation(i64),
}

/// `requestToWatcherKey(request)`.
pub fn request_to_watcher_key(request: &WatchRequest) -> WatcherKey {
    match request.correlation_id {
        Some(id) => WatcherKey::Correlation(id),
        None => WatcherKey::Path(super::fs_util::path_to_watcher_key(&request.path)),
    }
}

/// `string[]` de `excludes` como itens de padrão, para `patternsEquals` e `parseWatcherPatterns`.
pub fn string_patterns(patterns: &[String]) -> Vec<je_base::glob::PatternItem> {
    patterns.iter().map(|p| je_base::glob::PatternItem::Str(p.clone())).collect()
}

/// `requestToString` sem o caminho: `excludes`, `includes`, `filter` e `correlationId`.
pub fn request_details_to_string(request: &WatchRequest) -> String {
    let excludes = if request.excludes.is_empty() { "<none>".to_string() } else { request.excludes.join(",") };
    let includes = match &request.includes {
        Some(includes) if !includes.is_empty() => patterns_to_json(includes),
        _ => "<all>".to_string(),
    };
    let correlation = request.correlation_id.map_or_else(|| "<none>".to_string(), |id| id.to_string());
    format!("excludes: {excludes}, includes: {includes}, filter: {}, correlationId: {correlation}", request_filter_to_string(request.filter))
}

/// `requestToString(request)`.
pub fn request_to_string(request: &WatchRequest) -> String {
    format!("{} ({})", request.path, request_details_to_string(request))
}

/// `JSON.stringify(includes)`.
fn patterns_to_json(patterns: &[je_base::glob::PatternItem]) -> String {
    use je_base::glob::PatternItem;
    let items: Vec<String> = patterns
        .iter()
        .map(|p| match p {
            PatternItem::Str(text) => json_string(text),
            PatternItem::Relative(r) => format!("{{\"base\":{},\"pattern\":{}}}", json_string(&r.base), json_string(&r.pattern)),
        })
        .collect();
    format!("[{}]", items.join(","))
}

fn json_string(text: &str) -> String {
    let mut out = String::with_capacity(text.len() + 2);
    out.push('"');
    for c in text.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if (c as u32) < 0x20 => out.push_str(&format!("\\u{:04x}", c as u32)),
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

/// O que o monitor compara entre dois ticks, no lugar do `fs.Stats` do `watchFile`.
#[derive(Clone, Debug, PartialEq, Eq)]
struct PathStat {
    len: u64,
    modified: Option<SystemTime>,
    identity: u64,
}

impl PathStat {
    /// `None` quando o caminho não existe (o `Stats` zerado do Node, ver `isPathNotFound`).
    fn of(path: &str) -> Option<Self> {
        let metadata = std::fs::metadata(path).ok()?;
        #[cfg(unix)]
        let identity = {
            use std::os::unix::fs::MetadataExt;
            metadata.ino() ^ ((metadata.ctime() as u64) << 32) ^ metadata.ctime_nsec() as u64
        };
        #[cfg(not(unix))]
        let identity = metadata.created().ok().and_then(|t| t.duration_since(SystemTime::UNIX_EPOCH).ok()).map_or(0, |d| d.as_nanos() as u64);
        Some(Self { len: metadata.len(), modified: metadata.modified().ok(), identity })
    }
}
