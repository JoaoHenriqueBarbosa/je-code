//! Porte de `src/vs/base/parts/storage` do VS Code: o `Storage` com cache e flush adiado, o banco em
//! memória e o `SQLiteStorageDatabase` sobre `turso_core` (SQLite em Rust puro).
//!
//! O formato em disco é o `state.vscdb` do VS Code: um SQLite com a tabela
//! `ItemTable (key TEXT UNIQUE ON CONFLICT REPLACE, value BLOB)` e `user_version = 1`, lido e escrito
//! pelos dois lados (ver o teste de ida e volta com o oráculo).

pub mod async_utils;
pub mod marshalling;
pub mod sqlite_storage;
pub mod storage;

pub use sqlite_storage::{
    LogFn, SqliteStorageDatabase, SqliteStorageDatabaseLoggingOptions, SqliteStorageDatabaseOptions,
};
pub use storage::{
    IStorage, InMemoryStorageDatabase, ItemMap, KeySet, MIGRATED_KEY, MigratingStorage, Recovery,
    Storage, StorageChangeEvent, StorageDatabase, StorageHint, StorageItemsChangeEvent,
    StorageOptions, StorageState, StorageValue, UpdateRequest, is_storage_items_change_event,
};
