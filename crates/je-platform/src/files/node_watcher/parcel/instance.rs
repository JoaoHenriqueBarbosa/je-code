//! O `ParcelWatcherInstance` de `parcelWatcher.ts`: um watcher recursivo em andamento, com as
//! inscrições (`subscribe`) que o `BaseWatcher` e o watcher não recursivo usam para reaproveitá-lo.

use std::cell::{Cell, RefCell};
use std::rc::{Rc, Weak};

use je_base::async_::RunOnceWorker;
use je_base::cancellation::CancellationToken;
use je_base::event::Emitter;
use je_base::glob::ParsedPattern;
use je_base::lifecycle::{DisposableRef, DisposableStore, IDisposable, to_disposable};
use je_base::map::OrderedMap;
use je_base::promise::Promise;

use crate::files::files::FileChange;
use crate::files::watcher::{WatchRequest, is_linux, parse_watcher_patterns};

use super::super::base_watcher::string_patterns;
use super::super::fs_util::{file_uri, fs_path};

/// O que o instância faz para parar o que está por baixo (cancelar, soltar o `notify`...).
pub type StopFn = Box<dyn FnOnce(&ParcelWatcherInstance) -> Promise<()>>;

/// Chamado com os eventos que o agrupador juntou.
pub type OnEvents = Rc<dyn Fn(Vec<FileChange>, &Rc<ParcelWatcherInstance>)>;

type SubscriptionCallback = Rc<dyn Fn(&FileChange)>;

pub struct ParcelWatcherInstance {
    this: Weak<ParcelWatcherInstance>,
    /// Signals when the watcher is ready to watch.
    pub ready: Promise<()>,
    pub request: WatchRequest,
    /// How often this watcher has been restarted in case of an unexpected shutdown.
    pub restarts: usize,
    /// The cancellation token associated with the lifecycle of the watcher.
    pub token: CancellationToken,
    /// An event aggregator to coalesce events and reduce duplicates.
    pub worker: Rc<RunOnceWorker<FileChange>>,
    stop_fn: RefCell<Option<StopFn>>,

    /// `onDidStop`: carrega o `joinRestart` (a promessa de que o reinício terminou, se há).
    pub on_did_stop: Rc<Emitter<Option<Promise<()>>>>,
    pub on_did_fail: Rc<Emitter<()>>,
    did_fail: Cell<bool>,
    did_stop: Cell<bool>,

    includes: Option<Vec<ParsedPattern>>,
    excludes: Option<Vec<ParsedPattern>>,

    subscriptions: RefCell<OrderedMap<String, Vec<(u64, SubscriptionCallback)>>>,
    next_subscription: Cell<u64>,
    store: Rc<DisposableStore>,
}

impl ParcelWatcherInstance {
    pub fn new(ready: Promise<()>, request: WatchRequest, restarts: usize, token: CancellationToken, on_events: OnEvents, delay: u64, stop_fn: StopFn) -> Rc<Self> {
        let ignore_case = !is_linux();
        let includes = request.includes.as_ref().map(|includes| parse_watcher_patterns(&request.path, includes, ignore_case));
        let excludes = Some(parse_watcher_patterns(&request.path, &string_patterns(&request.excludes), ignore_case));
        let store = DisposableStore::new();
        let on_did_stop = store.add_rc(Emitter::new());
        let on_did_fail = store.add_rc(Emitter::new());
        Rc::new_cyclic(|this: &Weak<Self>| {
            let weak = this.clone();
            let worker = RunOnceWorker::new(
                move |events| {
                    if let Some(this) = weak.upgrade() {
                        on_events(events, &this);
                    }
                },
                delay,
            );
            let subscriptions_owner = this.clone();
            store.add(to_disposable(move || {
                if let Some(this) = subscriptions_owner.upgrade() {
                    this.subscriptions.borrow_mut().clear();
                }
            }));
            Self {
                this: this.clone(),
                ready,
                request,
                restarts,
                token,
                worker,
                stop_fn: RefCell::new(Some(stop_fn)),
                on_did_stop,
                on_did_fail,
                did_fail: Cell::new(false),
                did_stop: Cell::new(false),
                includes,
                excludes,
                subscriptions: RefCell::new(OrderedMap::new()),
                next_subscription: Cell::new(0),
                store,
            }
        })
    }

    /// `failed`.
    pub fn failed(&self) -> bool {
        self.did_fail.get()
    }

    /// `stopped`.
    pub fn stopped(&self) -> bool {
        self.did_stop.get()
    }

    /// `subscribe(path, callback)`.
    pub fn subscribe(&self, path: &str, callback: SubscriptionCallback) -> DisposableRef {
        let path = fs_path(&file_uri(path)); // make sure to store the path in `fsPath` form to match it with events later
        let id = self.next_subscription.get();
        self.next_subscription.set(id + 1);
        {
            let mut subscriptions = self.subscriptions.borrow_mut();
            match subscriptions.get_mut(&path) {
                Some(callbacks) => callbacks.push((id, callback)),
                None => {
                    subscriptions.insert(path.clone(), vec![(id, callback)]);
                }
            }
        }

        let this = self.this.clone();
        to_disposable(move || {
            let Some(this) = this.upgrade() else { return };
            let mut subscriptions = this.subscriptions.borrow_mut();
            let empty = subscriptions.get_mut(&path).map(|callbacks| {
                callbacks.retain(|(candidate, _)| *candidate != id);
                callbacks.is_empty()
            });
            if empty == Some(true) {
                subscriptions.remove(&path);
            }
        })
    }

    /// `subscriptionsCount`: o número de caminhos com inscrição.
    pub fn subscriptions_count(&self) -> usize {
        self.subscriptions.borrow().len()
    }

    /// `notifyFileChange(path, change)`.
    pub fn notify_file_change(&self, path: &str, change: &FileChange) {
        let callbacks: Vec<SubscriptionCallback> = self.subscriptions.borrow().get(&path.to_string()).map(|c| c.iter().map(|(_, cb)| cb.clone()).collect()).unwrap_or_default();
        for callback in callbacks {
            callback(change);
        }
    }

    /// `notifyWatchFailed()`.
    pub fn notify_watch_failed(&self) {
        self.did_fail.set(true);

        self.on_did_fail.fire(());
    }

    /// `include(path)`.
    pub fn include(&self, path: &str) -> bool {
        match &self.includes {
            Some(includes) if !includes.is_empty() => includes.iter().any(|include| include.matches(path, None)),
            _ => true, // no specific includes defined, include all
        }
    }

    /// `exclude(path)`.
    pub fn exclude(&self, path: &str) -> bool {
        self.excludes.as_ref().is_some_and(|excludes| excludes.iter().any(|exclude| exclude.matches(path, None)))
    }

    /// `stop(joinRestart)`.
    pub fn stop(&self, join_restart: Option<Promise<()>>) -> Promise<()> {
        self.did_stop.set(true);

        let stop_fn = self.stop_fn.borrow_mut().take();
        let result = match stop_fn {
            Some(stop_fn) => stop_fn(self),
            None => Promise::resolved(()),
        };
        let this = self.this.clone();
        result.finally(move || {
            if let Some(this) = this.upgrade() {
                this.on_did_stop.fire(join_restart);
                this.dispose();
            }
        })
    }
}

impl IDisposable for ParcelWatcherInstance {
    fn dispose(&self) {
        self.store.dispose();
    }
}
