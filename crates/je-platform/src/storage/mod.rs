//! Porte de `src/vs/platform/storage/common`: o serviço de armazenamento por escopo (aplicação, perfil e
//! workspace), o `AbstractStorageService`, o `InMemoryStorageService` e o [`SqliteStorageService`] sobre o
//! `je-storage`, que lê e escreve o mesmo `state.vscdb` do VS Code.
//!
//! Do lado `electron-main`, o dono das conexões é o [`StorageMainService`] (`storageMainService.ts`, com os armazéns de
//! `storageMain.ts`) e o canal que o expõe é o [`StorageDatabaseChannel`] (`storageIpc.ts`). O je-code é um processo só:
//! o [`SqliteStorageService`] chama o [`StorageMainService`] direto, sem o IPC do `storageIpc.ts` de `common` (o cliente
//! e o servidor do canal).

#[allow(clippy::module_inception)] // `storage.rs` é o porte de `storage.ts`, como no original
pub mod storage;
pub mod sqlite_storage_service;
pub mod storage_ipc;
pub mod storage_main;
pub mod storage_main_service;

pub use sqlite_storage_service::{SqliteStorageService, StorageServiceOptions};
pub use storage::*;
pub use storage_ipc::{SerializableItemsChangeEvent, StorageCallResult, StorageDatabaseChannel, StorageRequest};
pub use storage_main::{
    ApplicationSharedStorageMain, ApplicationStorageMain, BaseStorageMain, IStorageMain, InMemoryStorageMain, ProfileStorageMain, StorageMainChangeEvent, StorageMainOptions, WorkspaceStorageMain,
};
pub use storage_main_service::{
    ApplicationStorageMainService, I_STORAGE_MAIN_SERVICE, IStorageMainService, ProfileStorageChangeEvent, StorageMainService,
};
