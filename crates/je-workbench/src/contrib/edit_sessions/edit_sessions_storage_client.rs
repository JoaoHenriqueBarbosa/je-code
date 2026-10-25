//! Porte de `src/vs/workbench/contrib/editSessions/common/editSessionsStorageClient.ts`: o cliente do servidor do Cloud Changes.
//!
//! Divergência: `class EditSessionsStoreClient extends UserDataSyncStoreClient { _serviceBrand: undefined; }` só acrescenta a marca de
//! serviço e nenhum comportamento, então é um apelido do [`UserDataSyncStoreClient`] (como o `UserDataSyncStoreService` do porte
//! de `userDataSyncStoreService.ts`).

use je_platform::user_data_sync::user_data_sync_store_service::UserDataSyncStoreClient;

/// `EditSessionsStoreClient`.
pub type EditSessionsStoreClient = UserDataSyncStoreClient;
