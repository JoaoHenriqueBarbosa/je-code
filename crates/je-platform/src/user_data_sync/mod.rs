//! Porte de `src/vs/platform/userDataSync` do VS Code: a sincronização de dados do usuário.
//!
//! Portado até agora: o merge de `keybindings.json` ([`keybindings_merge`]), os auxiliares de edição de
//! texto JSON ([`content`]) e o contrato do serviço de utilidades ([`user_data_sync::IUserDataSyncUtilService`]).

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
