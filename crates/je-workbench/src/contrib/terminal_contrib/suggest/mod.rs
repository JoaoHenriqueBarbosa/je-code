//! Porte de `src/vs/workbench/contrib/terminalContrib/suggest`: o item e o modelo de completions do terminal, o
//! serviço de completions (com a completação de caminhos), os auxiliares do Git Bash e os identificadores de
//! configuração que o serviço lê.

pub mod common;

pub mod terminal_completion_item;
pub mod terminal_completion_model;
pub mod terminal_completion_service;
pub mod terminal_git_bash_helpers;
