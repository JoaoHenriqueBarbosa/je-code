//! O alvo web (`wasm32-unknown-unknown`) não tem sistema de arquivos: os clientes compilam e se comportam
//! como um watcher que não observa nada, avisando no log (uma vez por cliente) que a observação de disco
//! não existe aqui. Os mesmos nomes públicos do `client.rs` nativo.

use std::cell::Cell;
use std::rc::Rc;

use je_base::event::{Emitter, Event};
use je_base::lifecycle::DisposableStore;
use je_base::promise::Promise;

use crate::files::files::FileChange;
use crate::files::watcher::{CreateWatcher, LogMessage, LogMessageType, WatchRequest, Watcher, WatcherClient, WatcherErrorEvent};

struct NoopWatcher {
    changes: Rc<Emitter<Vec<FileChange>>>,
    logs: Rc<Emitter<LogMessage>>,
    errors: Rc<Emitter<WatcherErrorEvent>>,
    warned: Cell<bool>,
}

impl Watcher for NoopWatcher {
    fn on_did_change_file(&self) -> Event<Vec<FileChange>> {
        self.changes.event()
    }
    fn on_did_log_message(&self) -> Event<LogMessage> {
        self.logs.event()
    }
    fn on_did_error(&self) -> Event<WatcherErrorEvent> {
        self.errors.event()
    }
    fn watch(&self, requests: Vec<WatchRequest>) -> Promise<()> {
        if !requests.is_empty() && !self.warned.replace(true) {
            self.logs.fire(LogMessage {
                message_type: LogMessageType::Warn,
                message: "[File Watcher] file watching is not supported on this platform (no file system), the requested paths are not observed".to_string(),
            });
        }
        Promise::resolved(())
    }
    fn set_verbose_logging(&self, _enabled: bool) -> Promise<()> {
        Promise::resolved(())
    }
    fn stop(&self) -> Promise<()> {
        Promise::resolved(())
    }
}

fn noop_factory() -> CreateWatcher {
    Rc::new(|_store: &Rc<DisposableStore>| -> Rc<dyn Watcher> {
        Rc::new(NoopWatcher { changes: Emitter::new(), logs: Emitter::new(), errors: Emitter::new(), warned: Cell::new(false) })
    })
}

/// `new UniversalWatcherClient(onChange, onLogMessage, verboseLogging)`.
pub fn universal_watcher_client(on_change: Rc<dyn Fn(Vec<FileChange>)>, on_log: Rc<dyn Fn(LogMessage)>, verbose: bool) -> Rc<WatcherClient> {
    WatcherClient::universal(on_change, on_log, verbose, noop_factory())
}

/// `new NodeJSWatcherClient(onChange, onLogMessage, verboseLogging)`.
pub fn nodejs_watcher_client(on_change: Rc<dyn Fn(Vec<FileChange>)>, on_log: Rc<dyn Fn(LogMessage)>, verbose: bool) -> Rc<WatcherClient> {
    WatcherClient::non_recursive(on_change, on_log, verbose, noop_factory())
}
