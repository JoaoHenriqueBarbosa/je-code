//! Porte de `src/vs/workbench/contrib/terminalContrib/suggest`: o item e o modelo de completions do terminal, o
//! serviço de completions (com a completação de caminhos), os auxiliares do Git Bash e os identificadores de
//! configuração que o serviço lê.

pub mod common;

pub mod terminal_completion_item;
pub mod terminal_completion_model;
pub mod terminal_completion_service;
pub mod terminal_git_bash_helpers;

// O documento virtual que o terminal expõe ao servidor de linguagem.
pub mod lsp_terminal_model_content_provider;
pub mod lsp_terminal_util;

// A regra de sugestão em linha por shell (o resto do `SuggestAddon` fica fora, ver o cabeçalho do módulo).
pub mod terminal_suggest_addon;
