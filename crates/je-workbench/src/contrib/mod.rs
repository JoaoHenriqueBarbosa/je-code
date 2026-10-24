//! Contribuições do workbench (`src/vs/workbench/contrib`).

pub mod files;
pub mod markers;
pub mod merge_editor;
pub mod terminal_contrib;
pub mod update;

pub mod terminal;

pub mod custom_editor;
pub mod debug;

pub mod extensions;
pub mod policy_export;
pub mod preferences;
pub mod search;
pub mod testing;

pub mod browser_view;
pub mod issue;
pub mod multi_diff_editor;
pub mod notebook;
pub mod onboarding;

pub mod tasks;

pub mod snippets;
// O que o editor de configurações usa do chat (configurações de rede do agente e descrição do modelo padrão sob política).
pub mod chat;

// Canvases do chat de agentes (`contrib/canvases`).
pub mod canvases;

// Webviews (`contrib/webview`): a leitura de recursos locais.
pub mod webview;

// Telemetria de edições (`contrib/editTelemetry`): a base do rastreamento da origem de cada trecho.
pub mod edit_telemetry;
