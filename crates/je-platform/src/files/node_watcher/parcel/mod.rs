//! Porte de `src/vs/platform/files/node/watcher/parcel/`: o watcher recursivo.

pub mod events;
pub mod instance;
pub mod watcher;

pub use instance::ParcelWatcherInstance;
pub use watcher::ParcelWatcher;
