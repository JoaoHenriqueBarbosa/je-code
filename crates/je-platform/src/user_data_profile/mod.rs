//! Porte de `src/vs/platform/userDataProfile/common/userDataProfile.ts` (e `node/userDataProfile.ts`): os perfis do
//! usuário, a leitura e a escrita da configuração persistida (`userDataProfiles` e `profileAssociations` do
//! `storage.json`, o mesmo formato do VS Code) e as associações de workspace.
//!
//! Fora do porte (e por quê): `userDataProfileStorageService` e os arquivos de `electron-main`/`browser`/`electron-browser`
//! (o armazenamento do estado global de um perfil entre processos e a interface de troca de perfil).

#[allow(clippy::module_inception)] // `user_data_profile.rs` é o porte de `userDataProfile.ts`, como no original
pub mod user_data_profile;

pub use user_data_profile::*;
