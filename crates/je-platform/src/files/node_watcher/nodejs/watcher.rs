//! Porte de `src/vs/platform/files/node/watcher/nodejs/nodejsWatcher.ts`: o watcher não recursivo, que
//! mantém um [`NodeJsFileWatcherLibrary`] por pedido.
//!
//! Divergência: o original não descarta as instâncias no `dispose()` (só no `stop()`); aqui o `dispose()`
//! também as descarta, para um watcher descartado sem `stop` não vazar o `notify` por baixo.

use std::cell::RefCell;
use std::rc::{Rc, Weak};

use je_base::async_::{ThrottledWorker, ThrottledWorkerOptions};
use je_base::event::Event;
use je_base::glob::patterns_equals;
use je_base::lifecycle::{IDisposable, MutableDisposable};
use je_base::map::OrderedMap;
use je_base::promise::Promise;

use crate::files::files::FileChange;
use crate::files::watcher::{LogMessage, LogMessageType, RecursiveWatcherWithSubscribe, WatchRequest, Watcher, WatcherErrorEvent};

use super::super::base_watcher::{BaseWatcher, BaseWatcherOptions, WatcherBackend, WatcherKey, request_to_string, request_to_watcher_key, string_patterns};
use super::super::fs_util::path_to_watcher_key;
use super::watcher_lib::NodeJsFileWatcherLibrary;

/// `INodeJSWatcherInstance`.
pub struct NodeJsWatcherInstance {
    /// The watcher instance.
    pub instance: Rc<NodeJsFileWatcherLibrary>,
    /// The watch request associated to the watcher.
    pub request: WatchRequest,
}

pub struct NodeJsWatcher {
    this: Weak<NodeJsWatcher>,
    base: Rc<BaseWatcher>,
    recursive_watcher: Option<Rc<dyn RecursiveWatcherWithSubscribe>>,
    watchers: RefCell<OrderedMap<WatcherKey, Rc<NodeJsWatcherInstance>>>,
    worker: Rc<MutableDisposable<ThrottledWorker<WatchRequest>>>,
}

impl NodeJsWatcher {
    /// `new NodeJSWatcher(recursiveWatcher)`.
    pub fn new(recursive_watcher: Option<Rc<dyn RecursiveWatcherWithSubscribe>>) -> Rc<Self> {
        Self::with_options(recursive_watcher, BaseWatcherOptions::default())
    }

    pub fn with_options(recursive_watcher: Option<Rc<dyn RecursiveWatcherWithSubscribe>>, options: BaseWatcherOptions) -> Rc<Self> {
        Rc::new_cyclic(|this: &Weak<Self>| Self {
            this: this.clone(),
            base: BaseWatcher::new(this.clone() as Weak<dyn WatcherBackend>, options),
            recursive_watcher,
            watchers: RefCell::new(OrderedMap::new()),
            worker: MutableDisposable::new(),
        })
    }

    /// O estado e os eventos compartilhados com o `BaseWatcher` (`isSuspended`, `onWatchFail`, ...).
    pub fn base(&self) -> &Rc<BaseWatcher> {
        &self.base
    }

    /// `get watchers()`.
    pub fn watchers(&self) -> Vec<Rc<NodeJsWatcherInstance>> {
        self.watchers.borrow().values().cloned().collect()
    }

    fn create_watch_worker(&self) -> Rc<ThrottledWorker<WatchRequest>> {
        // We see very large amount of non-recursive file watcher requests
        // in large workspaces. To prevent the overhead of starting thousands
        // of watchers at once, we use a throttled worker to distribute this
        // work over time.
        let this = self.this.clone();
        let worker = ThrottledWorker::new(
            ThrottledWorkerOptions {
                max_work_chunk_size: 100, // only start 100 watchers at once before...
                throttle_delay: 100,      // ...resting for 100ms until we start watchers again...
                max_buffered_work: None,  // ...and never refuse any work.
                wait_throttle_delay_between_work_units: false,
            },
            move |requests| {
                if let Some(this) = this.upgrade() {
                    for request in requests {
                        this.start_watching(request);
                    }
                }
            },
        );
        self.worker.set(Some(worker.clone()));
        worker
    }

    fn start_watching(&self, request: WatchRequest) {
        // Start via node.js lib
        let base = &self.base;
        let on_change = base.on_did_change_file.clone();
        let on_fail = base.on_did_watch_fail.clone();
        let on_log = base.on_did_log_message.clone();
        let failed_request = request.clone();
        let instance = NodeJsFileWatcherLibrary::new(
            request.clone(),
            self.recursive_watcher.clone(),
            Rc::new(move |changes| on_change.fire(changes)),
            Some(Rc::new(move || on_fail.fire(failed_request.clone()))),
            Some(Rc::new(move |msg| on_log.fire(msg))),
            base.verbose_logging(),
        );

        // Remember as watcher instance
        let key = request_to_watcher_key(&request);
        self.watchers.borrow_mut().insert(key, Rc::new(NodeJsWatcherInstance { request, instance }));
    }

    fn stop_watching(&self, watcher: &Rc<NodeJsWatcherInstance>) {
        self.trace_for(Some(watcher), "stopping file watcher");

        self.watchers.borrow_mut().remove(&request_to_watcher_key(&watcher.request));

        watcher.instance.dispose();
    }

    fn remove_duplicate_requests(&self, requests: Vec<WatchRequest>) -> Vec<WatchRequest> {
        // Ignore requests for the same paths that have the same correlation
        let mut map_correlation_to_requests: OrderedMap<Option<i64>, OrderedMap<String, WatchRequest>> = OrderedMap::new();
        for request in requests {
            if !map_correlation_to_requests.contains_key(&request.correlation_id) {
                map_correlation_to_requests.insert(request.correlation_id, OrderedMap::new());
            }
            let for_correlation = map_correlation_to_requests.get_mut(&request.correlation_id).expect("acabou de inserir");

            let path = path_to_watcher_key(&request.path);
            if for_correlation.contains_key(&path) {
                self.trace_for(None, &format!("ignoring a request for watching who's path is already watched: {}", request_to_string(&request)));
            }

            for_correlation.insert(path, request);
        }

        map_correlation_to_requests.values().flat_map(|requests| requests.values().cloned()).collect()
    }

    fn trace_for(&self, watcher: Option<&Rc<NodeJsWatcherInstance>>, message: &str) {
        if self.base.verbose_logging() {
            self.base.log(LogMessageType::Trace, self.to_message(message, watcher));
        }
    }

    fn to_message(&self, message: &str, watcher: Option<&Rc<NodeJsWatcherInstance>>) -> String {
        match watcher {
            Some(watcher) => format!("[File Watcher (node.js)] {message} ({})", request_to_string(&watcher.request)),
            None => format!("[File Watcher (node.js)] {message}"),
        }
    }
}

impl WatcherBackend for NodeJsWatcher {
    fn do_watch(&self, requests: Vec<WatchRequest>) -> Promise<()> {
        // Figure out duplicates to remove from the requests
        let requests = self.remove_duplicate_requests(requests);

        // Figure out which watchers to start and which to stop
        let mut requests_to_start = Vec::new();
        let mut watchers_to_stop = self.watchers();
        for request in requests {
            let watcher = self.watchers.borrow().get(&request_to_watcher_key(&request)).cloned();
            match watcher {
                Some(watcher)
                    if patterns_equals(Some(&string_patterns(&watcher.request.excludes)), Some(&string_patterns(&request.excludes)))
                        && patterns_equals(watcher.request.includes.as_deref(), request.includes.as_deref()) =>
                {
                    watchers_to_stop.retain(|w| !Rc::ptr_eq(w, &watcher)); // keep watcher
                }
                _ => requests_to_start.push(request), // start watching
            }
        }

        // Logging
        if !requests_to_start.is_empty() {
            let list: Vec<String> = requests_to_start.iter().map(request_to_string).collect();
            self.trace_for(None, &format!("Request to start watching: {}", list.join(",")));
        }

        if !watchers_to_stop.is_empty() {
            let list: Vec<String> = watchers_to_stop.iter().map(|w| request_to_string(&w.request)).collect();
            self.trace_for(None, &format!("Request to stop watching: {}", list.join(",")));
        }

        // Stop the worker
        self.worker.clear();

        // Stop watching as instructed
        for watcher in &watchers_to_stop {
            self.stop_watching(watcher);
        }

        // Start watching as instructed
        self.create_watch_worker().work(&requests_to_start);
        Promise::resolved(())
    }

    fn when_ready(&self) -> Promise<()> {
        let readies: Vec<Promise<()>> = self.watchers().iter().map(|w| w.instance.ready()).collect();
        Promise::all(readies).then(|_| ())
    }

    fn recursive_watcher(&self) -> Option<Rc<dyn RecursiveWatcherWithSubscribe>> {
        self.recursive_watcher.clone()
    }

    fn trace(&self, message: &str) {
        self.trace_for(None, message);
    }

    fn warn(&self, message: &str) {
        self.base.log(LogMessageType::Warn, self.to_message(message, None));
    }
}

impl Watcher for NodeJsWatcher {
    fn on_did_change_file(&self) -> Event<Vec<FileChange>> {
        self.base.on_did_change_file.event()
    }

    fn on_did_log_message(&self) -> Event<LogMessage> {
        self.base.on_did_log_message.event()
    }

    fn on_did_error(&self) -> Event<WatcherErrorEvent> {
        Event::none()
    }

    fn watch(&self, requests: Vec<WatchRequest>) -> Promise<()> {
        self.base.watch(requests)
    }

    fn set_verbose_logging(&self, enabled: bool) -> Promise<()> {
        let result = self.base.set_verbose_logging(enabled);

        for watcher in self.watchers() {
            watcher.instance.set_verbose_logging(enabled);
        }
        result
    }

    fn stop(&self) -> Promise<()> {
        let result = self.base.stop();

        for watcher in self.watchers() {
            self.stop_watching(&watcher);
        }
        result
    }
}

impl IDisposable for NodeJsWatcher {
    fn dispose(&self) {
        self.worker.dispose();
        for watcher in self.watchers() {
            self.stop_watching(&watcher);
        }
        self.base.dispose();
    }
}
