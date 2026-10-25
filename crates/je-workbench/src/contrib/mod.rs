//! Contribuições do workbench (`src/vs/workbench/contrib`).

pub mod bulk_edit;
pub mod files;
pub mod markers;
pub mod merge_editor;
pub mod output;
pub mod terminal_contrib;
pub mod update;

pub mod terminal;

pub mod custom_editor;
pub mod debug;

pub mod extensions;
pub mod policy_export;
pub mod preferences;
pub mod scm;
pub mod search;
pub mod search_editor;
pub mod testing;

pub mod browser_view;
pub mod code_editor;
pub mod issue;
pub mod multi_diff_editor;
pub mod notebook;
pub mod onboarding;

pub mod tasks;

pub mod markdown;
pub mod snippets;
pub mod tags;
pub mod timeline;
// O que o editor de configurações usa do chat (configurações de rede do agente e descrição do modelo padrão sob política).
pub mod chat;

// Canvases do chat de agentes (`contrib/canvases`).
pub mod canvases;

// Webviews (`contrib/webview`): a leitura de recursos locais.
pub mod webview;

// Painéis de webview (`contrib/webviewPanel`): a entrada de editor e o contrato do serviço.
pub mod webview_panel;

// Compartilhamento (`contrib/share`): o contrato do serviço.
pub mod share;

// Telemetria de edições (`contrib/editTelemetry`): a base do rastreamento da origem de cada trecho.
pub mod edit_telemetry;

// Openers externos de URI (`contrib/externalUriOpener`): a configuração, o armazém de openers contribuídos e o serviço.
pub mod external_uri_opener;

// Links externos (`contrib/url`): o validador que pede confiança no workspace e nos domínios antes de abrir um link.
pub mod url;

// Acesso remoto (`contrib/remote`): o detector de URLs de servidores locais na saída dos terminais e das sessões de depuração.
pub mod remote;

// Modern UI (`contrib/modernUI`): a configuração do vidro fosco, o material e a contribuição que liga as classes.
pub mod modern_ui;

// Arquivos de workspace (`contrib/workspaces`): a contribuição que sugere abrir um `.code-workspace` da raiz da pasta.
pub mod workspaces;
// Reinício por configuração (`contrib/relauncher`): o `SettingsChangeRelauncher`, que pergunta se reinicia quando uma configuração que só vale após o reinício muda.
pub mod relauncher;
// Abridor de pastas (`contrib/opener`): o `WorkbenchOpenerContribution`, que revela pastas do workspace e abre pastas locais no sistema nas Sessions.
pub mod opener;
// Telemetria de configuração (`contrib/telemetry`): o `ConfigurationTelemetryContribution`, que reporta o valor explícito de configurações escolhidas.
pub mod telemetry;
// Formatar com (`contrib/format`): o `DefaultFormatter` (formatador padrão, seletor e status de conflito) e as ações "Format Document/Selection With...".
pub mod format;
// Indicador de limites (`contrib/limitIndicator`): a `LimitIndicatorContribution` e o `DiagnosticDecorationLimitReporter`.
pub mod limit_indicator;
