//! Porte de `src/vs/platform/files/node/watcher/watcher.ts`: o `UniversalWatcher`, que junta o watcher
//! recursivo e o não recursivo atrás de uma interface só.

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use je_base::async_::async_fn;
use je_base::errors::Error;
use je_base::event::{Emitter, Event, any};
use je_base::lifecycle::{DisposableStore, IDisposable};
use je_base::promise::Promise;

use crate::files::files::FileChange;
use crate::files::watcher::{LogMessage, LogMessageType, RecursiveWatcherWithSubscribe, WatchRequest, Watcher, WatcherErrorEvent};

use super::base_watcher::BaseWatcherOptions;
use super::nodejs::NodeJsWatcher;
use super::parcel::ParcelWatcher;
use super::watcher_stats::compute_stats;

pub struct UniversalWatcher {
    recursive_watcher: Rc<ParcelWatcher>,
    non_recursive_watcher: Rc<NodeJsWatcher>,
    on_did_log_message_emitter: Rc<Emitter<LogMessage>>,
    requests: RefCell<Vec<WatchRequest>>,
    failed_recursive_requests: Rc<Cell<usize>>,
    store: Rc<DisposableStore>,
}

impl UniversalWatcher {
    pub fn new() -> Rc<Self> {
        Self::with_options(BaseWatcherOptions::default())
    }

    pub fn with_options(options: BaseWatcherOptions) -> Rc<Self> {
        let store = DisposableStore::new();
        let recursive_watcher = ParcelWatcher::with_options(options);
        let non_recursive_watcher = NodeJsWatcher::with_options(Some(recursive_watcher.clone() as Rc<dyn RecursiveWatcherWithSubscribe>), options);
        let failed_recursive_requests = Rc::new(Cell::new(0));

        let failed = failed_recursive_requests.clone();
        store.add(recursive_watcher.on_did_error().listen(move |e| {
            if e.request.is_some() {
                failed.set(failed.get() + 1);
            }
        }));

        Rc::new(Self {
            recursive_watcher,
            non_recursive_watcher,
            on_did_log_message_emitter: store.add_rc(Emitter::new()),
            requests: RefCell::new(Vec::new()),
            failed_recursive_requests,
            store,
        })
    }

    pub fn recursive_watcher(&self) -> &Rc<ParcelWatcher> {
        &self.recursive_watcher
    }

    pub fn non_recursive_watcher(&self) -> &Rc<NodeJsWatcher> {
        &self.non_recursive_watcher
    }
}

impl Watcher for UniversalWatcher {
    fn on_did_change_file(&self) -> Event<Vec<FileChange>> {
        any(vec![self.recursive_watcher.on_did_change_file(), self.non_recursive_watcher.on_did_change_file()])
    }

    fn on_did_log_message(&self) -> Event<LogMessage> {
        any(vec![self.on_did_log_message_emitter.event(), self.recursive_watcher.on_did_log_message(), self.non_recursive_watcher.on_did_log_message()])
    }

    fn on_did_error(&self) -> Event<WatcherErrorEvent> {
        any(vec![self.recursive_watcher.on_did_error(), self.non_recursive_watcher.on_did_error()])
    }

    fn watch(&self, requests: Vec<WatchRequest>) -> Promise<()> {
        *self.requests.borrow_mut() = requests.clone();
        self.failed_recursive_requests.set(0);

        let (recursive, non_recursive): (Vec<_>, Vec<_>) = requests.into_iter().partition(WatchRequest::is_recursive);
        let (recursive_watcher, non_recursive_watcher) = (self.recursive_watcher.clone(), self.non_recursive_watcher.clone());
        async_fn(async move {
            // Watch recursively first to give recursive watchers a chance
            // to step in for non-recursive watch requests, thus reducing
            // watcher duplication.
            let mut error: Option<Error> = None;
            if let Err(e) = recursive_watcher.watch(recursive).await {
                error = Some(e);
            }

            if let Err(e) = non_recursive_watcher.watch(non_recursive).await
                && error.is_none()
            {
                error = Some(e);
            }

            match error {
                Some(error) => Err(error),
                None => Ok(()),
            }
        })
    }

    fn set_verbose_logging(&self, enabled: bool) -> Promise<()> {
        // Log stats
        if enabled && !self.requests.borrow().is_empty() {
            self.on_did_log_message_emitter.fire(LogMessage {
                message_type: LogMessageType::Trace,
                message: compute_stats(&self.requests.borrow(), self.failed_recursive_requests.get(), &self.recursive_watcher, &self.non_recursive_watcher),
            });
        }

        // Forward to watchers
        settled(vec![self.recursive_watcher.set_verbose_logging(enabled), self.non_recursive_watcher.set_verbose_logging(enabled)])
    }

    fn stop(&self) -> Promise<()> {
        settled(vec![self.recursive_watcher.stop(), self.non_recursive_watcher.stop()])
    }
}

/// `Promises.settled(promises)`: espera todas terminarem e, se alguma falhou, falha com o primeiro erro.
fn settled(promises: Vec<Promise<()>>) -> Promise<()> {
    Promise::all_settled(promises).then_result(|results| match results?.into_iter().find_map(Result::err) {
        Some(error) => Err(error),
        None => Ok(()),
    })
}

impl IDisposable for UniversalWatcher {
    fn dispose(&self) {
        self.recursive_watcher.dispose();
        self.non_recursive_watcher.dispose();
        self.store.dispose();
    }
}
