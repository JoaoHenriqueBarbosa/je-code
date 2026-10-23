//! Porte de `src/vs/platform/storage/common`: o serviço de armazenamento por escopo (aplicação, perfil e
//! workspace), o `AbstractStorageService`, o `InMemoryStorageService` e o [`SqliteStorageService`] sobre o
//! `je-storage`, que lê e escreve o mesmo `state.vscdb` do VS Code.
//!
//! Não portados: `storageIpc.ts` (canais de IPC entre processos) e o lado `electron-main`
//! (`storageMain*.ts`, o dono das conexões no processo principal); o je-code é um processo só, e o
//! [`SqliteStorageService`] abre os bancos diretamente.

#[allow(clippy::module_inception)] // `storage.rs` é o porte de `storage.ts`, como no original
pub mod storage;
pub mod sqlite_storage_service;

pub use sqlite_storage_service::{SqliteStorageService, StorageServiceOptions};
pub use storage::*;
