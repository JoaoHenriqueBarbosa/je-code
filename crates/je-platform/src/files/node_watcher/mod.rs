//! Porte de `src/vs/platform/files/node/watcher/`: os watchers do disco (recursivo e não recursivo) e os
//! clientes que o `DiskFileSystemProvider` usa.
//!
//! - `base_watcher`: `baseWatcher.ts` (suspender e retomar pedidos que falham).
//! - `nodejs`: `nodejsWatcher.ts` e `nodejsWatcherLib.ts` (não recursivo; no Node, `fs.watch`).
//! - `parcel`: `parcelWatcher.ts` (recursivo; no Node, o `@parcel/watcher`).
//! - `universal_watcher`: `watcher.ts`; `watcher_stats`: `watcherStats.ts`; `client`: os dois clientes.
//! - `fs_events`: a ponte entre os threads do `notify` e o loop (decisão documentada lá).
//!
//! Fora do alvo web (`wasm32-unknown-unknown`, sem sistema de arquivos), os clientes são os de `web`, que
//! não observam nada e avisam no log.

macro_rules! native {
    ($($item:item)*) => { $( #[cfg(not(all(target_arch = "wasm32", target_os = "unknown")))] $item )* };
}

macro_rules! web {
    ($($item:item)*) => { $( #[cfg(all(target_arch = "wasm32", target_os = "unknown"))] $item )* };
}

native! {
    pub mod base_watcher;
    mod client;
    mod fs_events;
    mod fs_util;
    pub mod nodejs;
    pub mod parcel;
    pub mod universal_watcher;
    pub mod watcher_stats;

    pub use client::{nodejs_watcher_client, universal_watcher_client};
}

web! {
    mod web;

    pub use web::{nodejs_watcher_client, universal_watcher_client};
}
