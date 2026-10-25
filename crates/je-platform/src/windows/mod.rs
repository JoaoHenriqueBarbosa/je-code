//! Porte de `src/vs/platform/windows/electron-main`: o lado "main" das janelas no processo único. O
//! `windowsFinder.ts` acha a janela aberta que cobre um arquivo ou workspace, e o `windowsStateHandler.ts` salva e
//! restaura o estado das janelas.
//!
//! Fora do porte (e por quê): o `windowsMainService.ts` (abre, fecha e reúne as janelas nativas; depende de
//! `BrowserWindow`), que é o contrato [`crate::user_data_profile::user_data_profiles_handler::WindowsMainService`] para
//! quem só o lê.

pub mod windows_finder;
pub mod windows_state_handler;
