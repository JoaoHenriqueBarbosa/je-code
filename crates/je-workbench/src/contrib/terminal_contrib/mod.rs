//! Porte de `src/vs/workbench/contrib/terminalContrib`.

pub mod links;

// A decisão de colar texto de várias linhas no terminal.
pub mod clipboard;

// O cache do conteúdo do buffer do terminal.
pub mod accessibility;

// A telemetria das configurações de perfil do terminal.
pub mod telemetry;

pub mod quick_fix;

pub mod suggest;

pub mod history;

// Configurações do sandbox do agente que o editor de configurações apresenta com a restrição gerenciada.
pub mod chat_agent_tools;

// As notificações do protocolo OSC 99 do terminal.
pub mod notification;

// O eco local (type ahead) do terminal.
pub mod type_ahead;

// O zoom da fonte do terminal (identificadores, configuração e o limite do tamanho de fonte).
pub mod zoom;
