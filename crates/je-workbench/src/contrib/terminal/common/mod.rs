//! Porte de `src/vs/workbench/contrib/terminal/common`.

pub mod terminal_environment;

pub mod terminal_configuration;

pub mod terminal;

pub mod terminal_backend;

pub mod environment_variable;

pub mod terminal_color_registry;

pub mod terminal_storage_keys;

pub mod environment_variable_service;

// Porte parcial de `terminalContextKey.ts` (a chave `terminalShellType`) e o registro de backends de `platform/terminal`.
pub mod terminal_context_key;
pub mod terminal_backend_registry;
