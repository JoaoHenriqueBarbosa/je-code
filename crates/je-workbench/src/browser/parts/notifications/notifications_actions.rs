//! Porte de `src/vs/workbench/browser/parts/notifications/notificationsActions.ts`, só no que o teste de
//! posição das notificações usa: os ícones de esconder, expandir e recolher e a escolha deles pela posição.
//!
//! Notas de tradução:
//! - `registerIcon(id, defaultIcon, description)` (`platform/theme/common/iconRegistry`) não tem porte: o
//!   registro de ícones aparece junto com o tema. O valor que ele devolve é o `ThemeIcon.fromId(id)`, e o que
//!   ele registra é o par id mais ícone padrão; [`RegisteredIcon`] guarda esse par (a descrição localizada
//!   fica no comentário de cada constante) e [`RegisteredIcon::to_theme_icon`] é o valor devolvido.
//! - `Codicon.chevronUp` e `Codicon.chevronDown` são os ids de `je-codicons` (`chevron-up`, `chevron-down`),
//!   que o `je-workbench` não importa: ficam como constantes de [`codicon`].
//! - As classes de ação (`ClearNotificationAction`, `ClearAllNotificationsAction`, `HideNotificationsCenterAction`,
//!   `ExpandNotificationAction`...) dependem de `ICommandService`, `IClipboardService` e dos comandos do
//!   `notificationsCommands.ts`, e entram com o centro de notificações. Os ícones que só elas usam
//!   (`notifications-clear`, `notifications-clear-all`, `notifications-configure`,
//!   `notifications-do-not-disturb`, `notifications-position`) entram com elas.

use je_base::themables::ThemeIcon;

use crate::common::notifications::NotificationsPosition;

/// Os ids dos codicons que os ícones das notificações usam como padrão (`Codicon.<nome>.id`).
pub mod codicon {
    pub const CHEVRON_DOWN: &str = "chevron-down";
    pub const CHEVRON_UP: &str = "chevron-up";
}

/// O que `registerIcon` registra: o id do ícone e o codicon padrão dele.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RegisteredIcon {
    pub id: &'static str,
    pub default_codicon: &'static str,
}

impl RegisteredIcon {
    /// O `ThemeIcon.fromId(id)` que `registerIcon` devolve.
    pub fn to_theme_icon(self) -> ThemeIcon {
        ThemeIcon::from_id(self.id)
    }
}

/// `hideIcon`: Icon for the hide action in notifications.
pub const HIDE_ICON: RegisteredIcon = RegisteredIcon { id: "notifications-hide", default_codicon: codicon::CHEVRON_DOWN };

/// `hideUpIcon`: Icon for the hide action in notifications when positioned at the top.
pub const HIDE_UP_ICON: RegisteredIcon = RegisteredIcon { id: "notifications-hide-up", default_codicon: codicon::CHEVRON_UP };

/// `expandIcon`: Icon for the expand action in notifications.
const EXPAND_ICON: RegisteredIcon = RegisteredIcon { id: "notifications-expand", default_codicon: codicon::CHEVRON_UP };

/// `expandDownIcon`: Icon for the expand action in notifications when the notification center is at the top.
const EXPAND_DOWN_ICON: RegisteredIcon = RegisteredIcon { id: "notifications-expand-down", default_codicon: codicon::CHEVRON_DOWN };

/// `collapseIcon`: Icon for the collapse action in notifications.
const COLLAPSE_ICON: RegisteredIcon = RegisteredIcon { id: "notifications-collapse", default_codicon: codicon::CHEVRON_DOWN };

/// `collapseUpIcon`: Icon for the collapse action in notifications when the notification center is at the top.
const COLLAPSE_UP_ICON: RegisteredIcon = RegisteredIcon { id: "notifications-collapse-up", default_codicon: codicon::CHEVRON_UP };

pub fn get_notification_expand_icon(position: NotificationsPosition) -> RegisteredIcon {
    if position == NotificationsPosition::TopRight { EXPAND_DOWN_ICON } else { EXPAND_ICON }
}

pub fn get_notification_collapse_icon(position: NotificationsPosition) -> RegisteredIcon {
    if position == NotificationsPosition::TopRight { COLLAPSE_UP_ICON } else { COLLAPSE_ICON }
}
