//! Porte de `src/vs/workbench/contrib/terminal/common/terminalStorageKeys.ts`: as chaves que o terminal grava no
//! `IStorageService` (o `const enum TerminalStorageKeys` do original, aqui constantes de texto).

pub const SUGGESTED_RENDERER_TYPE: &str = "terminal.integrated.suggestedRendererType";
pub const TABS_LIST_WIDTH_HORIZONTAL: &str = "tabs-list-width-horizontal";
pub const TABS_LIST_WIDTH_VERTICAL: &str = "tabs-list-width-vertical";
pub const TABS_SHOW_DETAILED: &str = "terminal.integrated.tabs.showDetailed";
pub const DEPRECATED_ENVIRONMENT_VARIABLE_COLLECTIONS: &str = "terminal.integrated.environmentVariableCollections";
pub const ENVIRONMENT_VARIABLE_COLLECTIONS: &str = "terminal.integrated.environmentVariableCollectionsV2";
pub const TERMINAL_BUFFER_STATE: &str = "terminal.integrated.bufferState";
pub const TERMINAL_LAYOUT_INFO: &str = "terminal.integrated.layoutInfo";
pub const PINNED_RECENT_COMMANDS_PREFIX: &str = "terminal.pinnedRecentCommands";
pub const TERMINAL_SUGGEST_SIZE: &str = "terminal.integrated.suggestSize";
