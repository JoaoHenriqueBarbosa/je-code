//! Porte de `src/vs/workbench/common/theme.ts`, no que as notificações e o grupo de editor usam: os identificadores das cores
//! `NOTIFICATIONS_*` e `EDITOR_GROUP_*`.
//!
//! Divergências:
//! - **Registro.** Os `registerColor(...)` do original não são código de execução aqui: o `je-theme` gera o registro de
//!   cores inteiro (`registered_colors.rs`, a partir das chamadas `registerColor` de `ref/vscode/src/vs`) e o expõe em
//!   `je_theme::registry::color_registry()`. Os padrões (`editorWidgetBackground`, `lighten(NOTIFICATIONS_BACKGROUND, 0.3)`
//!   no tema escuro, `darken(..., 0.05)` no claro, `NOTIFICATIONS_BACKGROUND` nos de alto contraste), as descrições e a
//!   ordem de registro vivem lá, uma vez só.
//! - **Constantes.** `registerColor` devolve o id da cor; cada `NOTIFICATIONS_*` é esse id (`&str`).

/// `notificationCenter.border`.
pub const NOTIFICATIONS_CENTER_BORDER: &str = "notificationCenter.border";

/// `notificationToast.border`.
pub const NOTIFICATIONS_TOAST_BORDER: &str = "notificationToast.border";

/// `notifications.foreground`.
pub const NOTIFICATIONS_FOREGROUND: &str = "notifications.foreground";

/// `notifications.background`.
pub const NOTIFICATIONS_BACKGROUND: &str = "notifications.background";

/// `notificationLink.foreground`.
pub const NOTIFICATIONS_LINKS: &str = "notificationLink.foreground";

/// `notificationCenterHeader.foreground`.
pub const NOTIFICATIONS_CENTER_HEADER_FOREGROUND: &str = "notificationCenterHeader.foreground";

/// `notificationCenterHeader.background`.
pub const NOTIFICATIONS_CENTER_HEADER_BACKGROUND: &str = "notificationCenterHeader.background";

/// `notifications.border`.
pub const NOTIFICATIONS_BORDER: &str = "notifications.border";

/// `notificationsErrorIcon.foreground`.
pub const NOTIFICATIONS_ERROR_ICON_FOREGROUND: &str = "notificationsErrorIcon.foreground";

/// `notificationsWarningIcon.foreground`.
pub const NOTIFICATIONS_WARNING_ICON_FOREGROUND: &str = "notificationsWarningIcon.foreground";

/// `notificationsInfoIcon.foreground`.
pub const NOTIFICATIONS_INFO_ICON_FOREGROUND: &str = "notificationsInfoIcon.foreground";

/// `editorPane.background`.
pub const EDITOR_PANE_BACKGROUND: &str = "editorPane.background";

/// `editorGroup.emptyBackground`.
pub const EDITOR_GROUP_EMPTY_BACKGROUND: &str = "editorGroup.emptyBackground";

/// `editorGroup.focusedEmptyBorder`.
pub const EDITOR_GROUP_FOCUSED_EMPTY_BORDER: &str = "editorGroup.focusedEmptyBorder";

/// `editorGroupHeader.tabsBackground`.
pub const EDITOR_GROUP_HEADER_TABS_BACKGROUND: &str = "editorGroupHeader.tabsBackground";

/// `editorGroupHeader.connectedTabsBackground`.
pub const EDITOR_GROUP_HEADER_CONNECTED_TABS_BACKGROUND: &str = "editorGroupHeader.connectedTabsBackground";

/// `editorGroupHeader.tabsBorder`.
pub const EDITOR_GROUP_HEADER_TABS_BORDER: &str = "editorGroupHeader.tabsBorder";

/// `editorGroupHeader.noTabsBackground`.
pub const EDITOR_GROUP_HEADER_NO_TABS_BACKGROUND: &str = "editorGroupHeader.noTabsBackground";

/// `editorGroupHeader.border`.
pub const EDITOR_GROUP_HEADER_BORDER: &str = "editorGroupHeader.border";

/// `editor.background` (`editorBackground` de `colorRegistry.ts`).
pub const EDITOR_BACKGROUND: &str = "editor.background";

/// `contrastBorder` (`colorRegistry.ts`).
pub const CONTRAST_BORDER: &str = "contrastBorder";
