//! Porte de `src/vs/workbench/api/browser`: a camada `MainThreadXxx`, o lado do workbench do RPC com o host de
//! extensões (cada classe implementa o `MainThreadXxxShape` do protocolo e fala com o `ExtHostXxxShape` pelo proxy).

pub mod main_thread_commands;
pub mod main_thread_configuration;
pub mod main_thread_custom_text_editor_navigation;
pub mod main_thread_documents;
pub mod main_thread_managed_sockets;
pub mod main_thread_progress;
pub mod main_thread_status_bar;
pub mod main_thread_quick_open;
pub mod main_thread_workspace;
