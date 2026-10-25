//! Porte de `src/vs/platform/userDataProfile/common/userDataProfile.ts` (e `node/userDataProfile.ts`): os perfis do
//! usuário, a leitura e a escrita da configuração persistida (`userDataProfiles` e `profileAssociations` do
//! `storage.json`, o mesmo formato do VS Code) e as associações de workspace.
//!
//! Fora do porte (e por quê): os arquivos de `browser`/`electron-browser` (a interface de troca de perfil) e o
//! `RemoteUserDataProfileStorageService` (o armazenamento do estado global de um perfil entre processos); o
//! `AbstractUserDataProfileStorageService` está em [`user_data_profile_storage_service`]. O lado `electron-main` está nos
//! três módulos do bloco abaixo (o papel do processo principal existe no processo único).

#[allow(clippy::module_inception)] // `user_data_profile.rs` é o porte de `userDataProfile.ts`, como no original
pub mod user_data_profile;
pub mod user_data_profile_storage_service;

pub use user_data_profile::*;

// O lado `electron-main`: `userDataProfile.ts` (o serviço), `userDataProfilesHandler.ts` e `userDataProfileStorageIpc.ts`.
pub mod user_data_profile_storage_ipc;
pub mod user_data_profiles_handler;
pub mod user_data_profiles_main_service;

pub use user_data_profiles_main_service::{UserDataProfilesMainEnvironment, UserDataProfilesMainService};
