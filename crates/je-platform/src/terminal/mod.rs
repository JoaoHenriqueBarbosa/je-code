//! Porte de `src/vs/platform/terminal` do VS Code.
//!
//! Portado até agora: as capacidades do terminal ([`capabilities`]) e as variáveis de ambiente que as
//! extensões contribuem ([`environment_variable`], [`environment_variable_collection`],
//! [`environment_variable_shared`]). O lado node do pty host (monitor de processos filhos, respostas automáticas,
//! o serviço do pty host e o canal local) está em [`child_process_monitor`], [`auto_replies_contrib_controller`],
//! [`pty_host_service`] e [`local_pty_channel`], com o contrato em [`pty_service`] e a codificação do canal em
//! [`pty_service_wire`].

pub mod capabilities;
pub mod environment_variable;
pub mod environment_variable_collection;
pub mod environment_variable_shared;

pub mod auto_approve;
pub mod terminal_recorder;
pub mod request_store;
pub mod terminal;
pub mod terminal_data_buffering;
pub mod terminal_environment;
pub mod terminal_profiles;
pub mod terminal_strings;

pub mod node_terminal_environment;
pub mod node_terminal_profiles;

pub mod shell_integration_addon;

pub mod terminal_settings;
pub mod xterm_headless;

pub mod pty_service;
pub mod pty_service_wire;
pub mod terminal_platform_configuration;

#[cfg(not(target_arch = "wasm32"))]
pub mod auto_replies_contrib_controller;
#[cfg(not(target_arch = "wasm32"))]
pub mod child_process_monitor;
#[cfg(not(target_arch = "wasm32"))]
pub mod local_pty_channel;
#[cfg(not(target_arch = "wasm32"))]
pub mod pty_host;
#[cfg(not(target_arch = "wasm32"))]
pub mod pty_host_service;
#[cfg(not(target_arch = "wasm32"))]
pub mod terminal_auto_responder;
#[cfg(not(target_arch = "wasm32"))]
pub mod terminal_process;
