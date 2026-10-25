//! Porte de `src/vs/workbench/services/log/common/logConstants.ts`: o identificador e o grupo do log da janela, e o
//! identificador da ação que o mostra.
//!
//! Divergência: `windowLogGroup` é uma constante calculada com `localize` no carregamento do módulo; aqui é a função
//! [`window_log_group`], que monta o grupo a cada chamada (sem tradução ativa o texto padrão é o mesmo).

use je_base::nls::localize;
use je_platform::log::LoggerGroup;

/// `windowLogId`.
pub const WINDOW_LOG_ID: &str = "rendererLog";

/// `showWindowLogActionId`.
pub const SHOW_WINDOW_LOG_ACTION_ID: &str = "workbench.action.showWindowLog";

/// `windowLogGroup`.
pub fn window_log_group() -> LoggerGroup {
    LoggerGroup { id: WINDOW_LOG_ID.to_string(), name: localize("window", "Window", &[]) }
}
