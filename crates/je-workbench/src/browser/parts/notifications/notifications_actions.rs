//! Porte de `src/vs/workbench/browser/parts/notifications/notificationsActions.ts`: os ícones das notificações, a escolha
//! deles pela posição e as classes de ação (`ClearNotificationAction`, `ClearAllNotificationsAction`,
//! `ToggleDoNotDisturbAction`, `ToggleDoNotDisturbBySourceAction`, `ConfigureDoNotDisturbAction`,
//! `ConfigureNotificationsPositionAction`, `HideNotificationsCenterAction`, `ExpandNotificationAction`,
//! `CollapseNotificationAction`, `ConfigureNotificationAction` e `CopyNotificationMessageAction`).
//!
//! Notas de tradução:
//! - `registerIcon(id, defaultIcon, description)` (`platform/theme/common/iconRegistry`) não tem porte: o
//!   registro de ícones aparece junto com o tema. O valor que ele devolve é o `ThemeIcon.fromId(id)`, e o que
//!   ele registra é o par id mais ícone padrão; [`RegisteredIcon`] guarda esse par (a descrição localizada
//!   fica no comentário de cada constante) e [`RegisteredIcon::to_theme_icon`] é o valor devolvido.
//! - `Codicon.chevronUp`, `Codicon.close` e os outros são os ids de `je-codicons` (`chevron-up`, `close`...), que o
//!   `je-workbench` não importa: ficam como constantes de [`codicon`].
//! - **`extends Action`** é composição: cada classe guarda uma [`Action`] e repassa a ela os métodos de [`IAction`]
//!   (a macro `impl_action_wrapper!`); `run` é o que a subclasse sobrescreve. O `instanceof` do original
//!   (`action instanceof ConfigureNotificationAction`) é o `downcast_ref` de [`IAction::as_any`].
//! - **`static readonly LABEL = localize(...)`** é a função `label()` de cada classe (a tradução é lida na chamada, não no
//!   carregamento do módulo); `static readonly ID` é a constante associada `ID`.
//! - **`@ICommandService`/`@IClipboardService`** são os parâmetros finais dos construtores.
//! - **`this.commandService.executeCommand(...)`** é chamado sem esperar o resultado, como no original (o `run` não faz
//!   `await`); a promessa devolvida resolve na hora.
//! - [`WorkbenchNotificationActionProvider`] é o [`NotificationActionProvider`] que o viewer pede: cria as classes
//!   acima com os serviços e responde ao `instanceof ConfigureNotificationAction`.

use std::rc::Rc;

use je_base::actions::{Action, ActionRef, IAction, IActionChangeEvent};
use je_base::async_::async_fn;
use je_base::dynamic::Dynamic;
use je_base::event::Event;
use je_base::lifecycle::{Disposable, DisposableRef, IDisposable};
use je_base::nls::localize;
use je_base::promise::Promise;
use je_base::themables::ThemeIcon;
use je_editor::services::clipboard_service::IClipboardService;
use je_platform::commands::ICommandService;
use je_platform::notification::notification_telemetry::{NotificationActionTelemetry, NotificationActionTelemetryId, with_notification_action_telemetry};

use crate::browser::parts::notifications::notifications_commands::{
    CLEAR_ALL_NOTIFICATIONS, CLEAR_NOTIFICATION, COLLAPSE_NOTIFICATION, EXPAND_NOTIFICATION, HIDE_NOTIFICATIONS_CENTER, TOGGLE_DO_NOT_DISTURB_MODE, TOGGLE_DO_NOT_DISTURB_MODE_BY_SOURCE,
};
use crate::browser::parts::notifications::notifications_viewer::NotificationActionProvider;
use crate::common::notifications::{NotificationViewItem, NotificationsPosition};

/// Os ids dos codicons que os ícones das notificações usam como padrão (`Codicon.<nome>.id`).
pub mod codicon {
    pub const CHEVRON_DOWN: &str = "chevron-down";
    pub const CHEVRON_UP: &str = "chevron-up";
    pub const CLOSE: &str = "close";
    pub const CLEAR_ALL: &str = "clear-all";
    pub const GEAR: &str = "gear";
    pub const BELL_SLASH: &str = "bell-slash";
    pub const ARROW_SWAP: &str = "arrow-swap";
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

    /// `ThemeIcon.asClassName(icon)`.
    pub fn to_class_name(self) -> String {
        ThemeIcon::as_class_name(&self.to_theme_icon())
    }
}

/// `clearIcon`: Icon for the clear action in notifications.
pub const CLEAR_ICON: RegisteredIcon = RegisteredIcon { id: "notifications-clear", default_codicon: codicon::CLOSE };

/// `clearAllIcon`: Icon for the clear all action in notifications.
pub const CLEAR_ALL_ICON: RegisteredIcon = RegisteredIcon { id: "notifications-clear-all", default_codicon: codicon::CLEAR_ALL };

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

/// `configureIcon`: Icon for the configure action in notifications.
const CONFIGURE_ICON: RegisteredIcon = RegisteredIcon { id: "notifications-configure", default_codicon: codicon::GEAR };

/// `doNotDisturbIcon`: Icon for the mute all action in notifications.
const DO_NOT_DISTURB_ICON: RegisteredIcon = RegisteredIcon { id: "notifications-do-not-disturb", default_codicon: codicon::BELL_SLASH };

/// `positionIcon`: Icon for the position action in notifications.
pub const POSITION_ICON: RegisteredIcon = RegisteredIcon { id: "notifications-position", default_codicon: codicon::ARROW_SWAP };

pub fn get_notification_expand_icon(position: NotificationsPosition) -> RegisteredIcon {
    if position == NotificationsPosition::TopRight { EXPAND_DOWN_ICON } else { EXPAND_ICON }
}

pub fn get_notification_collapse_icon(position: NotificationsPosition) -> RegisteredIcon {
    if position == NotificationsPosition::TopRight { COLLAPSE_UP_ICON } else { COLLAPSE_ICON }
}

/// Implementa [`IAction`], [`IDisposable`] e `on_did_change` para uma classe que guarda `base: Rc<Disposable>` e
/// `action: Rc<Action>` (o `extends Action` do original). O corpo dado é o `run` da classe.
macro_rules! impl_action_wrapper {
    ($name:ident, |$this:ident, $args:ident| $run:expr) => {
        impl $name {
            /// O `onDidChange` que a classe herda de `Action`.
            pub fn on_did_change(&self) -> Event<IActionChangeEvent> {
                self.action.on_did_change()
            }
        }

        impl IAction for $name {
            fn id(&self) -> String {
                self.action.id()
            }
            fn label(&self) -> String {
                self.action.label()
            }
            fn set_label(&self, value: String) {
                self.action.set_label(value);
            }
            fn tooltip(&self) -> String {
                self.action.tooltip()
            }
            fn set_tooltip(&self, value: String) {
                self.action.set_tooltip(value);
            }
            fn class(&self) -> Option<String> {
                self.action.class()
            }
            fn set_class(&self, value: Option<String>) {
                self.action.set_class(value);
            }
            fn enabled(&self) -> bool {
                self.action.enabled()
            }
            fn set_enabled(&self, value: bool) {
                self.action.set_enabled(value);
            }
            fn checked(&self) -> Option<bool> {
                self.action.checked()
            }
            fn set_checked(&self, value: Option<bool>) {
                self.action.set_checked(value);
            }
            fn run(&self, $args: &[Dynamic]) -> Promise<Dynamic> {
                let $this = self;
                $run
            }
            fn as_any(&self) -> &dyn std::any::Any {
                self
            }
            fn as_base_action(&self) -> Option<&Action> {
                Some(&*self.action)
            }
        }

        impl IDisposable for $name {
            fn dispose(&self) {
                self.base.dispose();
            }

            fn constructor_name(&self) -> &'static str {
                stringify!($name)
            }
        }
    };
}

/// Cria a [`Action`] interna de uma classe (`super(id, label, cssClass)`) e a registra no `base`.
fn create_inner_action(id: &str, label: &str, icon: Option<RegisteredIcon>) -> (Rc<Disposable>, Rc<Action>) {
    let base = Disposable::new();
    let class = icon.map(RegisteredIcon::to_class_name).unwrap_or_default();
    let action = Action::new(id, label, &class, true, None);
    base.register(action.clone());
    (base, action)
}

/// Declara uma classe de ação cujo `run` só executa um comando pelo `ICommandService`, repassando (ou não) a
/// notificação recebida como argumento.
macro_rules! command_forwarding_action {
    (
        $(#[$doc:meta])*
        $name:ident {
            id: $id:expr,
            label: ($key:literal, $message:literal),
            icon: $icon:expr,
            command: $command:expr,
            forwards_notification: $forwards:literal $(,)?
        }
    ) => {
        $(#[$doc])*
        pub struct $name {
            base: Rc<Disposable>,
            action: Rc<Action>,
            command_service: Rc<dyn ICommandService>,
        }

        impl $name {
            pub const ID: &'static str = $id;

            /// `static readonly LABEL`.
            pub fn label() -> String {
                localize($key, $message, &[])
            }

            pub fn new(id: &str, label: &str, command_service: Rc<dyn ICommandService>) -> Rc<Self> {
                let (base, action) = create_inner_action(id, label, $icon);
                Rc::new(Self { base, action, command_service })
            }
        }

        impl_action_wrapper!($name, |this, args| {
            let command_args = if $forwards { vec![args.first().cloned().unwrap_or(Dynamic::Undefined)] } else { Vec::new() };
            let _ = this.command_service.execute_command($command, command_args);
            Promise::resolved(Dynamic::Undefined)
        });
    };
}

command_forwarding_action! {
    /// `ClearNotificationAction`.
    ClearNotificationAction {
        id: CLEAR_NOTIFICATION,
        label: ("clearNotification", "Clear Notification"),
        icon: Some(CLEAR_ICON),
        command: CLEAR_NOTIFICATION,
        forwards_notification: true,
    }
}

command_forwarding_action! {
    /// `ClearAllNotificationsAction`.
    ClearAllNotificationsAction {
        id: CLEAR_ALL_NOTIFICATIONS,
        label: ("clearNotifications", "Clear All Notifications"),
        icon: Some(CLEAR_ALL_ICON),
        command: CLEAR_ALL_NOTIFICATIONS,
        forwards_notification: false,
    }
}

command_forwarding_action! {
    /// `ToggleDoNotDisturbAction`.
    ToggleDoNotDisturbAction {
        id: TOGGLE_DO_NOT_DISTURB_MODE,
        label: ("toggleDoNotDisturbMode", "Toggle Do Not Disturb Mode"),
        icon: Some(DO_NOT_DISTURB_ICON),
        command: TOGGLE_DO_NOT_DISTURB_MODE,
        forwards_notification: false,
    }
}

command_forwarding_action! {
    /// `ToggleDoNotDisturbBySourceAction`.
    ToggleDoNotDisturbBySourceAction {
        id: TOGGLE_DO_NOT_DISTURB_MODE_BY_SOURCE,
        label: ("toggleDoNotDisturbModeBySource", "Toggle Do Not Disturb Mode By Source..."),
        icon: None,
        command: TOGGLE_DO_NOT_DISTURB_MODE_BY_SOURCE,
        forwards_notification: false,
    }
}

command_forwarding_action! {
    /// `HideNotificationsCenterAction`.
    HideNotificationsCenterAction {
        id: HIDE_NOTIFICATIONS_CENTER,
        label: ("hideNotificationsCenter", "Hide Notifications"),
        icon: Some(HIDE_ICON),
        command: HIDE_NOTIFICATIONS_CENTER,
        forwards_notification: false,
    }
}

command_forwarding_action! {
    /// `ExpandNotificationAction`.
    ExpandNotificationAction {
        id: EXPAND_NOTIFICATION,
        label: ("expandNotification", "Expand Notification"),
        icon: Some(EXPAND_ICON),
        command: EXPAND_NOTIFICATION,
        forwards_notification: true,
    }
}

command_forwarding_action! {
    /// `CollapseNotificationAction`.
    CollapseNotificationAction {
        id: COLLAPSE_NOTIFICATION,
        label: ("collapseNotification", "Collapse Notification"),
        icon: Some(COLLAPSE_ICON),
        command: COLLAPSE_NOTIFICATION,
        forwards_notification: true,
    }
}

/// `ConfigureDoNotDisturbAction`: o clique é tratado pelo item de dropdown que o centro monta, a ação em si não faz nada.
pub struct ConfigureDoNotDisturbAction {
    base: Rc<Disposable>,
    action: Rc<Action>,
}

impl ConfigureDoNotDisturbAction {
    pub const ID: &'static str = "workbench.action.configureDoNotDisturbMode";

    /// `static readonly LABEL`.
    pub fn label() -> String {
        localize("configureDoNotDisturbMode", "Configure Do Not Disturb...", &[])
    }

    pub fn new(id: &str, label: &str) -> Rc<Self> {
        let (base, action) = create_inner_action(id, label, Some(DO_NOT_DISTURB_ICON));
        Rc::new(Self { base, action })
    }
}

impl_action_wrapper!(ConfigureDoNotDisturbAction, |this, args| this.action.run(args));

/// `ConfigureNotificationsPositionAction`: como a anterior, o menu é do item de dropdown do centro.
pub struct ConfigureNotificationsPositionAction {
    base: Rc<Disposable>,
    action: Rc<Action>,
}

impl ConfigureNotificationsPositionAction {
    pub const ID: &'static str = "workbench.action.configureNotificationsPosition";

    /// `static readonly LABEL`.
    pub fn label() -> String {
        localize("configureNotificationsPosition", "Configure Notifications Position...", &[])
    }

    pub fn new(id: &str, label: &str) -> Rc<Self> {
        let (base, action) = create_inner_action(id, label, Some(POSITION_ICON));
        Rc::new(Self { base, action })
    }
}

impl_action_wrapper!(ConfigureNotificationsPositionAction, |this, args| this.action.run(args));

/// `ConfigureNotificationAction`: guarda a notificação para o item de dropdown que monta o menu de configuração.
pub struct ConfigureNotificationAction {
    base: Rc<Disposable>,
    action: Rc<Action>,
    pub notification: Rc<NotificationViewItem>,
}

impl ConfigureNotificationAction {
    pub const ID: &'static str = "workbench.action.configureNotification";

    /// `static readonly LABEL`.
    pub fn label() -> String {
        localize("configureNotification", "More Actions...", &[])
    }

    pub fn new(id: &str, label: &str, notification: Rc<NotificationViewItem>) -> Rc<Self> {
        let (base, action) = create_inner_action(id, label, Some(CONFIGURE_ICON));
        Rc::new(Self { base, action, notification })
    }
}

impl_action_wrapper!(ConfigureNotificationAction, |this, args| this.action.run(args));

/// `CopyNotificationMessageAction`.
pub struct CopyNotificationMessageAction {
    base: Rc<Disposable>,
    action: Rc<Action>,
    clipboard_service: Rc<dyn IClipboardService>,
}

impl CopyNotificationMessageAction {
    pub const ID: &'static str = "workbench.action.copyNotificationMessage";

    /// `static readonly LABEL`.
    pub fn label() -> String {
        localize("copyNotification", "Copy Text", &[])
    }

    pub fn new(id: &str, label: &str, clipboard_service: Rc<dyn IClipboardService>) -> Rc<Self> {
        let (base, action) = create_inner_action(id, label, None);
        let this = Rc::new(Self { base, action, clipboard_service });
        with_notification_action_telemetry(this.clone(), Some(NotificationActionTelemetry::Id(NotificationActionTelemetryId::Copy)));
        this
    }
}

impl_action_wrapper!(CopyNotificationMessageAction, |this, args| {
    // `run(notification)`: sem a notificação (ou com outro contexto) não há texto a copiar.
    let raw = args.first().and_then(crate::browser::parts::notifications::notifications_commands::notification_view_item_from_context).map(|notification| notification.message().raw.clone());
    let clipboard_service = this.clipboard_service.clone();
    async_fn(async move {
        if let Some(raw) = raw {
            clipboard_service.write_text(&raw).await?;
        }
        Ok(Dynamic::Undefined)
    })
});

/// O [`NotificationActionProvider`] do workbench: as classes de ação com o `ICommandService` e o `IClipboardService`.
pub struct WorkbenchNotificationActionProvider {
    command_service: Rc<dyn ICommandService>,
    clipboard_service: Rc<dyn IClipboardService>,
}

impl WorkbenchNotificationActionProvider {
    pub fn new(command_service: Rc<dyn ICommandService>, clipboard_service: Rc<dyn IClipboardService>) -> Rc<Self> {
        Rc::new(Self { command_service, clipboard_service })
    }
}

impl NotificationActionProvider for WorkbenchNotificationActionProvider {
    fn create_close_action(&self) -> ActionRef {
        ClearNotificationAction::new(ClearNotificationAction::ID, &ClearNotificationAction::label(), self.command_service.clone())
    }

    fn create_expand_action(&self) -> ActionRef {
        ExpandNotificationAction::new(ExpandNotificationAction::ID, &ExpandNotificationAction::label(), self.command_service.clone())
    }

    fn create_collapse_action(&self) -> ActionRef {
        CollapseNotificationAction::new(CollapseNotificationAction::ID, &CollapseNotificationAction::label(), self.command_service.clone())
    }

    fn create_configure_action(&self, notification: &Rc<NotificationViewItem>) -> (ActionRef, DisposableRef) {
        let action = ConfigureNotificationAction::new(ConfigureNotificationAction::ID, &ConfigureNotificationAction::label(), notification.clone());
        let disposable: DisposableRef = action.clone();
        let action: ActionRef = action;
        (action, disposable)
    }

    fn configure_action_notification(&self, action: &ActionRef) -> Option<Rc<NotificationViewItem>> {
        action.as_any().downcast_ref::<ConfigureNotificationAction>().map(|configure| configure.notification.clone())
    }

    fn create_copy_message_action(&self) -> ActionRef {
        CopyNotificationMessageAction::new(CopyNotificationMessageAction::ID, &CopyNotificationMessageAction::label(), self.clipboard_service.clone())
    }
}
