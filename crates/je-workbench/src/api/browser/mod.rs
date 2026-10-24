//! Porte de `src/vs/workbench/api/browser`: a camada `MainThreadXxx`, o lado do workbench do RPC com o host de
//! extensões (cada classe implementa o `MainThreadXxxShape` do protocolo e fala com o `ExtHostXxxShape` pelo proxy).

pub mod main_thread_commands;
pub mod main_thread_progress;
