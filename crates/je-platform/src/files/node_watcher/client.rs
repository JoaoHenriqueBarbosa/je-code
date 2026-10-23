//! Porte de `src/vs/platform/files/node/watcher/watcherClient.ts` e `nodejs/nodejsClient.ts`: os clientes
//! que o provider de disco usa para criar e reiniciar o watcher.
//!
//! Divergência: o `UniversalWatcherClient` original cria o watcher num processo filho (`bootstrap-fork`
//! com `watcherMain`) e fala com ele por IPC, para um watcher que trava ou estoura memória não derrubar o
//! processo principal. Aqui o `notify` já roda nos seus próprios threads e entrega por canal (ver
//! `fs_events.rs`), então o [`UniversalWatcher`] roda no mesmo processo, e o cliente só cuida do que ele
//! sempre cuidou: criar o watcher, reiniciar em erro inesperado (até cinco vezes) e repassar mudanças e
//! log. O `onDidProcessExit` ("terminated by itself") não tem equivalente, pois não há processo.

use std::rc::Rc;

use je_base::lifecycle::DisposableStore;

use crate::files::files::FileChange;
use crate::files::watcher::{CreateWatcher, LogMessage, Watcher, WatcherClient};

use super::nodejs::NodeJsWatcher;
use super::universal_watcher::UniversalWatcher;

/// `new UniversalWatcherClient(onFileChanges, onLogMessage, verboseLogging)`.
pub fn universal_watcher_client(on_change: Rc<dyn Fn(Vec<FileChange>)>, on_log: Rc<dyn Fn(LogMessage)>, verbose: bool) -> Rc<WatcherClient> {
    let create_watcher: CreateWatcher = Rc::new(|disposables: &Rc<DisposableStore>| -> Rc<dyn Watcher> {
        let watcher = UniversalWatcher::new();
        disposables.add_rc(watcher.clone());
        watcher
    });
    WatcherClient::universal(on_change, on_log, verbose, create_watcher)
}

/// `new NodeJSWatcherClient(onFileChanges, onLogMessage, verboseLogging)`.
pub fn nodejs_watcher_client(on_change: Rc<dyn Fn(Vec<FileChange>)>, on_log: Rc<dyn Fn(LogMessage)>, verbose: bool) -> Rc<WatcherClient> {
    // no recursive watching support here
    let create_watcher: CreateWatcher = Rc::new(|disposables: &Rc<DisposableStore>| -> Rc<dyn Watcher> {
        let watcher = NodeJsWatcher::new(None);
        disposables.add_rc(watcher.clone());
        watcher
    });
    WatcherClient::non_recursive(on_change, on_log, verbose, create_watcher)
}
