//! Porte de `src/vs/platform/files/node/watcher/nodejs/`: o watcher não recursivo.

pub mod watcher;
pub mod watcher_lib;

pub use watcher::{NodeJsWatcher, NodeJsWatcherInstance};
pub use watcher_lib::{NodeJsFileWatcherLibrary, watch_file_contents};
