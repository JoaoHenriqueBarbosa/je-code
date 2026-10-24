//! Porte de `src/vs/workbench/contrib/terminal/browser`.

pub mod terminal;

pub mod terminal_configuration_service;

pub mod terminal_events;

pub mod terminal_instance;

pub mod terminal_process_ext_host_proxy;

pub mod terminal_process_manager;

pub mod terminal_status_list;

pub mod environment_variable_info;

pub mod xterm;

// As contribuições do terminal, os eventos de entrada e os widgets (gerenciador e hover).
pub mod terminal_extensions;
pub mod terminal_input_events;
pub mod widgets;
