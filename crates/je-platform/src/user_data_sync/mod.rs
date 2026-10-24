//! Porte de `src/vs/platform/userDataSync` do VS Code: a sincronização de dados do usuário.
//!
//! Portado até agora: o merge de `keybindings.json` ([`keybindings_merge`]), os auxiliares de edição de
//! texto JSON ([`content`]), o contrato do serviço de utilidades ([`user_data_sync::IUserDataSyncUtilService`]),
//! a base dos sincronizadores ([`abstract_synchronizer`]), o sincronizador de perfis
//! ([`user_data_profiles_manifest_sync`]) e os serviços que o cliente de teste monta: o servidor
//! ([`user_data_sync_store_service`]), a ativação ([`user_data_sync_enablement_service`]), o histórico local
//! ([`user_data_sync_local_store_service`]), a conta ([`user_data_sync_account`]) e o serviço de sincronização
//! ([`user_data_sync_service`], sem a sincronização manual).

pub mod content;
pub mod global_state_merge;
pub mod keybindings_merge;
pub mod settings_merge;
pub mod snippets_merge;
pub mod user_data_profiles_manifest_merge;
#[allow(clippy::module_inception)] // `user_data_sync.rs` é o porte de `userDataSync.ts`, como no original
pub mod user_data_sync;

pub mod abstract_synchronizer;
pub mod user_data_profiles_manifest_sync;

pub mod user_data_sync_account;
pub mod user_data_sync_enablement_service;
pub mod user_data_sync_local_store_service;
pub mod user_data_sync_service;
pub mod user_data_sync_store_service;
