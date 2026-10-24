//! Porte de `src/vs/workbench/contrib/terminalContrib`.

pub mod links;

pub mod suggest;

pub mod history;

// Configurações do sandbox do agente que o editor de configurações apresenta com a restrição gerenciada.
pub mod chat_agent_tools;

// Os quick fixes do terminal (contrato, quick fixes internos, serviço e addon).
pub mod quick_fix;

// As notificações do protocolo OSC 99 do terminal.
pub mod notification;

// O eco local (type ahead) do terminal.
pub mod type_ahead;

// O zoom da fonte do terminal (identificadores, configuração e o limite do tamanho de fonte).
pub mod zoom;
