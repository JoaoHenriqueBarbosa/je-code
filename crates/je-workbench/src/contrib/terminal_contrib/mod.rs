//! Porte de `src/vs/workbench/contrib/terminalContrib`.

pub mod links;

pub mod quick_fix;

pub mod suggest;

pub mod history;

// Configurações do sandbox do agente que o editor de configurações apresenta com a restrição gerenciada.
pub mod chat_agent_tools;

// O zoom da fonte do terminal (identificadores, configuração e o limite do tamanho de fonte).
pub mod zoom;
