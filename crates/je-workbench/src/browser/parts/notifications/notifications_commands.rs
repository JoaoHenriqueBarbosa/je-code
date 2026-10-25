//! Porte de `src/vs/workbench/browser/parts/notifications/notificationsCommands.ts`: os ids dos comandos, os contratos
//! do centro e dos toasts, `getNotificationFromContext`, o `registerNotificationCommands` (comandos, atalhos e itens de
//! menu), as três ações de posição das notificações e o `NotificationActionRunner`, que executa as ações de uma
//! notificação registrando a telemetria e reportando o erro.
//!
//! Notas de tradução:
//! - **`extends ActionRunner`**: a struct guarda o [`ActionRunner`] base, criado com o `runAction` sobrescrito
//!   ([`ActionRunner::with_run_action`]); `super.runAction` é o `run` da própria ação.
//! - **O `context: unknown`** é um [`Dynamic`]; o item de notificação (`NotificationViewItem`) viaja nele como carga
//!   nativa de um objeto ([`notification_view_item_context`]) e `isNotificationViewItem(context)` é
//!   [`notification_view_item_from_context`].
//! - **`this.notificationService.error(error)`** recebe o [`Error`] da rejeição como [`NotificationMessage::Error`].
//! - **`listService.lastFocusedList`** é o [`IListWidget`] do `je-platform`: o elemento em foco vem de
//!   [`IListWidget::get_focused_element`] e a notificação é o `downcast` dele. O ramo `list.isDOMFocused()` ->
//!   `list.element(0)` (https://github.com/microsoft/vscode/issues/191705, a lista recebe o foco pelo teclado sem
//!   elemento em foco) não existe: o contrato não expõe o primeiro elemento; a lista focada sem elemento em foco
//!   devolve `None`.
//! - **`element.closest('.notifications-center')` e `'.notification-toast'`** (a superfície do telemetria da lista
//!   focada) não existem sem DOM: a superfície da lista com o foco é registrada por quem cria a lista, em
//!   [`set_focused_notification_surface`] (o centro faz isso ao receber e perder o foco).
//! - **`getNotificationFromContext(listService, context?)`** recebe o contexto como `Option<&Dynamic>`.
//! - **`@IInstantiationService.createInstance(NotificationActionRunner, surface)`** do "Accept Primary Action" é
//!   [`NotificationActionRunner::new`] com a telemetria e o serviço de notificações do accessor.
//! - **`localeCompare`** da ordenação das fontes é a comparação sem diferenciar maiúsculas, com o texto original como
//!   desempate (o `localeCompare` do ICU põe a minúscula antes da maiúscula igual).
//! - **`registerAction2`** das posições é [`register_notification_position_actions`], chamada pelo registro do
//!   workbench (o original registra no carregamento do módulo).
//! - **`CONTEXT_ACCESSIBILITY_MODE_ENABLED`** (`accessibilityModeEnabled`) não tem porte da `RawContextKey`: a
//!   expressão é `ContextKeyExpr::has("accessibilityModeEnabled")`, a mesma chave.

use std::cell::Cell;
use std::rc::Rc;

use je_base::actions::{ActionRef, ActionRunner, IActionRunner, IRunEvent};
use je_base::async_::async_fn;
use je_base::dynamic::{Dynamic, ObjectRef};
use je_base::errors::{self, Error};
use je_base::event::Event;
use je_base::key_codes::{KeyCode, KeyMod, key_chord};
use je_base::lifecycle::{DisposableRef, DisposableStore, IDisposable, to_disposable};
use je_base::nls::localize;
use je_base::promise::Promise;
use je_base::themables::ThemeIcon;
use je_base::JsString;
use je_editor::code_action::platform::{AccessibilitySignal, I_ACCESSIBILITY_SIGNAL_SERVICE};
use je_platform::action::{Category, CommandAction, CommandActionTitle, Icon, Title, Toggled, localize2};
use je_platform::actions::menu_ids::{command_palette, notifications_center_position_menu, title_bar};
use je_platform::actions::{Action2, Action2MenuItem, Action2Options, AnyMenuItem, MenuItem, menu_registry, register_action2};
use je_platform::commands::CommandHandler;
use je_platform::configuration::service::I_CONFIGURATION_SERVICE;
use je_platform::contextkey::{ContextKeyExpr, ContextKeyExpression};
use je_platform::keybinding::keybindings_registry::{CommandAndKeybindingRule, KeybindingRule, KeybindingWeight, Keybindings, PlatformKeybindings, keybindings_registry};
use je_platform::list::list_service::{I_LIST_SERVICE, IListService};
use je_platform::notification::{INotificationService, I_NOTIFICATION_SERVICE, NotificationFilterSetting, NotificationMessage, NotificationSource, NotificationSourceFilter, NotificationsFilter};
use je_platform::quickinput::quick_input::{I_QUICK_INPUT_SERVICE, IQuickItem, IQuickPick, IQuickPickItem, QuickPickInput, QuickPickTooltip};
use je_platform::telemetry::{I_TELEMETRY_SERVICE, ITelemetryService};

use crate::common::contextkeys::{notification_focused_context, notifications_center_visible_context, notifications_toasts_visible_context};
use crate::common::notification_telemetry::{
    NotificationActionRole, NotificationInteraction, NotificationTelemetrySurface, log_notification_action, log_notification_expansion, log_notification_interaction,
};
use crate::common::notifications::{INotificationsModel, NotificationViewItem, NotificationsModel, NotificationsPosition, NotificationsSettings};

// Center
pub const SHOW_NOTIFICATIONS_CENTER: &str = "notifications.showList";
pub const HIDE_NOTIFICATIONS_CENTER: &str = "notifications.hideList";
const TOGGLE_NOTIFICATIONS_CENTER: &str = "notifications.toggleList";

// Toasts
pub const HIDE_NOTIFICATION_TOAST: &str = "notifications.hideToasts";
const FOCUS_NOTIFICATION_TOAST: &str = "notifications.focusToasts";
const FOCUS_NEXT_NOTIFICATION_TOAST: &str = "notifications.focusNextToast";
const FOCUS_PREVIOUS_NOTIFICATION_TOAST: &str = "notifications.focusPreviousToast";
const FOCUS_FIRST_NOTIFICATION_TOAST: &str = "notifications.focusFirstToast";
const FOCUS_LAST_NOTIFICATION_TOAST: &str = "notifications.focusLastToast";

// Notification
pub const COLLAPSE_NOTIFICATION: &str = "notification.collapse";
pub const EXPAND_NOTIFICATION: &str = "notification.expand";
pub const ACCEPT_PRIMARY_ACTION_NOTIFICATION: &str = "notification.acceptPrimaryAction";
const TOGGLE_NOTIFICATION: &str = "notification.toggle";
pub const CLEAR_NOTIFICATION: &str = "notification.clear";
pub const CLEAR_ALL_NOTIFICATIONS: &str = "notifications.clearAll";
pub const TOGGLE_DO_NOT_DISTURB_MODE: &str = "notifications.toggleDoNotDisturbMode";
pub const TOGGLE_DO_NOT_DISTURB_MODE_BY_SOURCE: &str = "notifications.toggleDoNotDisturbModeBySource";

/// `INotificationsCenterController`.
pub trait INotificationsCenterController {
    fn is_visible(&self) -> bool;

    fn show(&self);
    fn hide(&self);

    fn clear_all(&self);
}

/// `INotificationsToastController`.
pub trait INotificationsToastController {
    fn focus(&self);
    fn focus_next(&self);
    fn focus_previous(&self);
    fn focus_first(&self);
    fn focus_last(&self);

    fn hide(&self);
}

thread_local! {
    /// A superfície (`center` ou `toast`) da lista de notificações com o foco: o que o original lê pelo `closest` do
    /// elemento da lista.
    static FOCUSED_NOTIFICATION_SURFACE: Cell<Option<NotificationTelemetrySurface>> = const { Cell::new(None) };
}

/// Registra a superfície da lista de notificações que tem o foco (`None` quando perde).
pub fn set_focused_notification_surface(surface: Option<NotificationTelemetrySurface>) {
    FOCUSED_NOTIFICATION_SURFACE.with(|cell| cell.set(surface));
}

/// O `NotificationViewItem` que o elemento focado da lista é, se for um.
fn notification_from_list_element(element: Rc<dyn std::any::Any>) -> Option<Rc<NotificationViewItem>> {
    if let Some(item) = element.downcast_ref::<Rc<NotificationViewItem>>() {
        return Some(item.clone());
    }
    element.downcast::<NotificationViewItem>().ok()
}

/// `getNotificationFromContext`.
pub fn get_notification_from_context(list_service: &dyn IListService, context: Option<&Dynamic>) -> Option<Rc<NotificationViewItem>> {
    if let Some(item) = context.and_then(notification_view_item_from_context) {
        return Some(item);
    }

    list_service.last_focused_list()?.get_focused_element().and_then(notification_from_list_element)
}

fn get_focused_notification_surface(list_service: &dyn IListService, context: Option<&Dynamic>) -> Option<NotificationTelemetrySurface> {
    if context.and_then(notification_view_item_from_context).is_some() {
        return None; // The invoking action runner already supplied its surface.
    }
    if list_service.last_focused_list().is_some() && get_notification_from_context(list_service, None).is_some() {
        return FOCUSED_NOTIFICATION_SURFACE.with(Cell::get);
    }
    None
}

fn primary(key: u32) -> Keybindings {
    Keybindings { primary: Some(key), ..Default::default() }
}

fn register_command_and_keybinding(id: &str, weight: i64, when: Option<ContextKeyExpression>, keybindings: Keybindings, handler: CommandHandler) -> DisposableRef {
    keybindings_registry().register_command_and_keybinding_rule(CommandAndKeybindingRule { rule: KeybindingRule { keybindings, id: id.to_owned(), weight, args: Dynamic::Undefined, when }, handler, metadata: None })
}

fn register_keybinding(id: &str, weight: i64, when: Option<ContextKeyExpression>, keybindings: Keybindings) -> DisposableRef {
    keybindings_registry().register_keybinding_rule(KeybindingRule { keybindings, id: id.to_owned(), weight, args: Dynamic::Undefined, when })
}

/// `CommandsRegistry.registerCommand(id, handler)`.
fn register_command(id: &str, handler: CommandHandler) -> DisposableRef {
    je_platform::commands::commands_registry().register_command_handler(id, Some(handler))
}

/// `localize2(key, message)` como o título de um item de menu.
fn localized_title(key: &str, message: &str) -> Title {
    let localized = localize2(key, message);
    Title::Localized(CommandActionTitle { value: localized.value, original: localized.original, mnemonic_title: None })
}

/// `{ command: { id, title: localize2(...), category }, when }` do menu da paleta de comandos.
fn command_palette_item(id: &str, title: Title, category: &Category, when: Option<ContextKeyExpression>) -> AnyMenuItem {
    let command = CommandAction { category: Some(category.clone()), ..CommandAction::new(id, title) };
    AnyMenuItem::Menu(MenuItem { when, ..MenuItem::new(command) })
}

/// `registerNotificationCommands`.
pub fn register_notification_commands(center: Rc<dyn INotificationsCenterController>, toasts: Rc<dyn INotificationsToastController>, model: Rc<NotificationsModel>) -> DisposableRef {
    let registrations = DisposableStore::new();
    let weight = KeybindingWeight::WORKBENCH_CONTRIB;
    let focused = || Some(notification_focused_context().expr());
    let ctrl_cmd = KeyMod::CTRL_CMD;

    // Show Notifications Cneter
    {
        let (center, toasts) = (center.clone(), toasts.clone());
        registrations.add(register_command_and_keybinding(
            SHOW_NOTIFICATIONS_CENTER,
            weight,
            None,
            primary(key_chord(ctrl_cmd | KeyCode::KeyK.bits(), ctrl_cmd | KeyMod::SHIFT | KeyCode::KeyN.bits())),
            Rc::new(move |_, _| {
                toasts.hide();
                center.show();
                Dynamic::Undefined
            }),
        ));
    }

    // Hide Notifications Center
    {
        let center = center.clone();
        registrations.add(register_command_and_keybinding(
            HIDE_NOTIFICATIONS_CENTER,
            weight + 50,
            Some(notifications_center_visible_context().expr()),
            primary(KeyCode::Escape.bits()),
            Rc::new(move |_, _| {
                center.hide();
                Dynamic::Undefined
            }),
        ));
    }

    // Toggle Notifications Center
    {
        let (center, toasts) = (center.clone(), toasts.clone());
        registrations.add(register_command(
            TOGGLE_NOTIFICATIONS_CENTER,
            Rc::new(move |_, _| {
                if center.is_visible() {
                    center.hide();
                } else {
                    toasts.hide();
                    center.show();
                }
                Dynamic::Undefined
            }),
        ));
    }

    // Clear Notification
    registrations.add(register_command_and_keybinding(
        CLEAR_NOTIFICATION,
        weight,
        focused(),
        Keybindings {
            primary: Some(KeyCode::Delete.bits()),
            mac: Some(PlatformKeybindings { primary: Some(ctrl_cmd | KeyCode::Backspace.bits()), secondary: None }),
            ..Default::default()
        },
        Rc::new(|accessor, args| {
            let accessibility_signal_service = accessor.get(I_ACCESSIBILITY_SIGNAL_SERVICE);
            let list_service = accessor.get(I_LIST_SERVICE);
            let args = args.first();
            if let Some(notification) = get_notification_from_context(&*list_service, args)
                && !notification.has_active_progress()
            {
                log_notification_interaction(
                    &*accessor.get(I_TELEMETRY_SERVICE),
                    &notification,
                    NotificationInteraction::Dismiss,
                    get_focused_notification_surface(&*list_service, args),
                    None,
                    NotificationActionRole::None,
                );
                notification.close();
                accessibility_signal_service.play_signal(AccessibilitySignal::Clear);
            }
            Dynamic::Undefined
        }),
    ));

    // Expand Notification
    registrations.add(register_command_and_keybinding(
        EXPAND_NOTIFICATION,
        weight,
        focused(),
        primary(KeyCode::RightArrow.bits()),
        Rc::new(|accessor, args| {
            let list_service = accessor.get(I_LIST_SERVICE);
            let args = args.first();
            if let Some(notification) = get_notification_from_context(&*list_service, args) {
                log_notification_expansion(&*accessor.get(I_TELEMETRY_SERVICE), &notification, true, get_focused_notification_surface(&*list_service, args));
                notification.expand();
            }
            Dynamic::Undefined
        }),
    ));

    // Accept Primary Action
    {
        let model = model.clone();
        registrations.add(register_command_and_keybinding(
            ACCEPT_PRIMARY_ACTION_NOTIFICATION,
            weight + 1,
            ContextKeyExpr::and([Some(ContextKeyExpr::has("accessibilityModeEnabled")), ContextKeyExpr::or([notification_focused_context().expr(), notifications_toasts_visible_context().expr()])]),
            primary(ctrl_cmd | KeyMod::SHIFT | KeyCode::KeyA.bits()),
            Rc::new(move |accessor, _| {
                let list_service = accessor.get(I_LIST_SERVICE);
                let Some(notification) = get_notification_from_context(&*list_service, None).or_else(|| model.notifications().first().cloned()) else {
                    return Dynamic::Undefined;
                };
                let Some(primary_action) = notification.actions().primary.first().cloned() else {
                    return Dynamic::Undefined;
                };
                let action_runner = NotificationActionRunner::new(get_focused_notification_surface(&*list_service, None), accessor.get(I_TELEMETRY_SERVICE), accessor.get(I_NOTIFICATION_SERVICE));
                let _ = action_runner.run(&primary_action, Some(notification_view_item_context(&notification)));
                notification.close();
                action_runner.dispose();
                Dynamic::Undefined
            }),
        ));
    }

    // Collapse Notification
    registrations.add(register_command_and_keybinding(
        COLLAPSE_NOTIFICATION,
        weight,
        focused(),
        primary(KeyCode::LeftArrow.bits()),
        Rc::new(|accessor, args| {
            let list_service = accessor.get(I_LIST_SERVICE);
            let args = args.first();
            if let Some(notification) = get_notification_from_context(&*list_service, args) {
                log_notification_expansion(&*accessor.get(I_TELEMETRY_SERVICE), &notification, false, get_focused_notification_surface(&*list_service, args));
                notification.collapse(false);
            }
            Dynamic::Undefined
        }),
    ));

    // Toggle Notification
    registrations.add(register_command_and_keybinding(
        TOGGLE_NOTIFICATION,
        weight,
        focused(),
        Keybindings { primary: Some(KeyCode::Space.bits()), secondary: Some(vec![KeyCode::Enter.bits()]), ..Default::default() },
        Rc::new(|accessor, _| {
            let list_service = accessor.get(I_LIST_SERVICE);
            if let Some(notification) = get_notification_from_context(&*list_service, None) {
                log_notification_expansion(&*accessor.get(I_TELEMETRY_SERVICE), &notification, !notification.expanded(), get_focused_notification_surface(&*list_service, None));
                notification.toggle();
            }
            Dynamic::Undefined
        }),
    ));

    // Hide Toasts
    {
        let toasts = toasts.clone();
        registrations.add(register_command(
            HIDE_NOTIFICATION_TOAST,
            Rc::new(move |_, _| {
                toasts.hide();
                Dynamic::Undefined
            }),
        ));
    }

    registrations.add(register_keybinding(
        HIDE_NOTIFICATION_TOAST,
        weight - 50, // lower when not focused (e.g. let editor suggest win over this command)
        Some(notifications_toasts_visible_context().expr()),
        primary(KeyCode::Escape.bits()),
    ));

    registrations.add(register_keybinding(
        HIDE_NOTIFICATION_TOAST,
        weight + 100, // higher when focused
        ContextKeyExpr::and([notifications_toasts_visible_context().expr(), notification_focused_context().expr()]),
        primary(KeyCode::Escape.bits()),
    ));

    // Focus Toasts
    {
        let toasts = toasts.clone();
        registrations.add(register_command(
            FOCUS_NOTIFICATION_TOAST,
            Rc::new(move |_, _| {
                toasts.focus();
                Dynamic::Undefined
            }),
        ));
    }

    // Focus Next / Previous / First / Last Toast: `NotificationFocusedContext && NotificationsToastsVisibleContext`
    let focused_toast = || ContextKeyExpr::and([notification_focused_context().expr(), notifications_toasts_visible_context().expr()]);
    let toast_focus: [(&str, Keybindings, fn(&dyn INotificationsToastController)); 4] = [
        (FOCUS_NEXT_NOTIFICATION_TOAST, primary(KeyCode::DownArrow.bits()), |toasts| toasts.focus_next()),
        (FOCUS_PREVIOUS_NOTIFICATION_TOAST, primary(KeyCode::UpArrow.bits()), |toasts| toasts.focus_previous()),
        (FOCUS_FIRST_NOTIFICATION_TOAST, Keybindings { primary: Some(KeyCode::PageUp.bits()), secondary: Some(vec![KeyCode::Home.bits()]), ..Default::default() }, |toasts| toasts.focus_first()),
        (FOCUS_LAST_NOTIFICATION_TOAST, Keybindings { primary: Some(KeyCode::PageDown.bits()), secondary: Some(vec![KeyCode::End.bits()]), ..Default::default() }, |toasts| toasts.focus_last()),
    ];
    for (id, keybindings, focus) in toast_focus {
        let toasts = toasts.clone();
        registrations.add(register_command_and_keybinding(
            id,
            weight,
            focused_toast(),
            keybindings,
            Rc::new(move |_, _| {
                focus(&*toasts);
                Dynamic::Undefined
            }),
        ));
    }

    // Clear All Notifications
    {
        let center = center.clone();
        registrations.add(register_command(
            CLEAR_ALL_NOTIFICATIONS,
            Rc::new(move |_, _| {
                center.clear_all();
                Dynamic::Undefined
            }),
        ));
    }

    // Toggle Do Not Disturb Mode
    registrations.add(register_command(
        TOGGLE_DO_NOT_DISTURB_MODE,
        Rc::new(|accessor, _| {
            let notification_service = accessor.get(I_NOTIFICATION_SERVICE);

            let filter = if notification_service.get_filter(None) == NotificationsFilter::Error { NotificationsFilter::Off } else { NotificationsFilter::Error };
            notification_service.set_filter(NotificationFilterSetting::Global(filter));
            Dynamic::Undefined
        }),
    ));

    // Configure Do Not Disturb by Source
    registrations.add(register_command(TOGGLE_DO_NOT_DISTURB_MODE_BY_SOURCE, Rc::new(|accessor, _| toggle_do_not_disturb_by_source(&*accessor.get(I_NOTIFICATION_SERVICE), &*accessor.get(I_QUICK_INPUT_SERVICE)))));

    // Commands for Command Palette
    let category = Category::Localized(localize2("notifications", "Notifications"));
    let palette = command_palette();
    let menus = menu_registry();
    for (id, key, message, when) in [
        (SHOW_NOTIFICATIONS_CENTER, "showNotifications", "Show Notifications", None),
        (HIDE_NOTIFICATIONS_CENTER, "hideNotifications", "Hide Notifications", Some(notifications_center_visible_context().expr())),
        (CLEAR_ALL_NOTIFICATIONS, "clearAllNotifications", "Clear All Notifications", None),
        (ACCEPT_PRIMARY_ACTION_NOTIFICATION, "acceptNotificationPrimaryAction", "Accept Notification Primary Action", None),
        (TOGGLE_DO_NOT_DISTURB_MODE, "toggleDoNotDisturbMode", "Toggle Do Not Disturb Mode", None),
        (TOGGLE_DO_NOT_DISTURB_MODE_BY_SOURCE, "toggleDoNotDisturbModeBySource", "Toggle Do Not Disturb Mode By Source...", None),
        (FOCUS_NOTIFICATION_TOAST, "focusNotificationToasts", "Focus Notification Toast", Some(notifications_toasts_visible_context().expr())),
    ] {
        registrations.add(menus.append_menu_item(&palette, command_palette_item(id, localized_title(key, message), &category, when)));
    }

    // Bell icon in the title bar (when notifications are positioned at top-right)
    let bell = CommandAction { icon: Some(Icon::Theme(ThemeIcon::from_id("bell"))), ..CommandAction::new(TOGGLE_NOTIFICATIONS_CENTER, Title::Plain(localize("toggleNotifications", "Toggle Notifications", &[]))) };
    registrations.add(menus.append_menu_item(
        &title_bar(),
        AnyMenuItem::Menu(MenuItem {
            group: Some("navigation".to_owned()),
            order: Some(10000.0),
            when: ContextKeyExpr::and([
                ContextKeyExpr::equals(&format!("config.{}", NotificationsSettings::NotificationsPosition.as_str()), NotificationsPosition::TopRight.as_str()),
                ContextKeyExpr::equals(&format!("config.{}", NotificationsSettings::NotificationsButton.as_str()), true),
            ]),
            ..MenuItem::new(bell)
        }),
    ));
    registrations
}

/// O comando `TOGGLE_DO_NOT_DISTURB_MODE_BY_SOURCE`: o picker com as fontes que podem mostrar todas as notificações.
fn toggle_do_not_disturb_by_source(notification_service: &Rc<dyn INotificationService>, quick_input_service: &Rc<dyn je_platform::quickinput::quick_input::IQuickInputService>) -> Dynamic {
    let mut sorted_filters = notification_service.get_filters();
    sorted_filters.sort_by(|a, b| a.source.label.to_lowercase().cmp(&b.source.label.to_lowercase()).then_with(|| a.source.label.cmp(&b.source.label)));

    let disposables = DisposableStore::new();
    let picker: Rc<dyn IQuickPick> = quick_input_service.create_quick_pick(None);
    {
        let picker = picker.clone();
        disposables.add(to_disposable(move || picker.dispose()));
    }

    let items: Vec<IQuickPickItem> = sorted_filters
        .iter()
        .map(|source| IQuickPickItem {
            base: IQuickItem { id: Some(source.source.id.clone()), label: JsString::from(source.source.label.as_str()), ..Default::default() },
            tooltip: Some(QuickPickTooltip::Text(format!("{} ({})", source.source.label, source.source.id))),
            ..Default::default()
        })
        .collect();
    picker.set_items(items.iter().cloned().map(QuickPickInput::Item).collect());

    picker.set_can_select_many(true);
    picker.set_placeholder(Some(localize("selectSources", "Select sources to enable all notifications from", &[])));
    let selected: Vec<IQuickPickItem> = items.iter().zip(&sorted_filters).filter(|(_, source)| source.filter == NotificationsFilter::Off).map(|(item, _)| item.clone()).collect();
    picker.set_selected_items(selected);

    picker.show();

    {
        let (picker, notification_service) = (picker.clone(), notification_service.clone());
        disposables.add(picker.clone().on_did_accept().listen(move |_| {
            let selected = picker.selected_items();
            for item in picker.items().into_iter().filter_map(QuickPickInput::into_item) {
                let is_selected = selected.iter().any(|candidate| candidate.base.id == item.base.id);
                notification_service.set_filter(NotificationFilterSetting::Source(NotificationSourceFilter {
                    source: NotificationSource { id: item.base.id.clone().unwrap_or_default(), label: item.base.label.to_string_lossy() },
                    filter: if is_selected { NotificationsFilter::Off } else { NotificationsFilter::Error },
                }));
            }

            picker.hide();
        }));
    }

    {
        let disposables_on_hide = disposables.clone();
        disposables.add(picker.on_did_hide().listen(move |_| disposables_on_hide.dispose()));
    }
    Dynamic::Undefined
}

// Notification Position Actions

/// Uma das três ações `workbench.action.setNotificationsPosition.*`.
struct SetNotificationsPosition {
    id: &'static str,
    title: (&'static str, &'static str),
    position: NotificationsPosition,
    order: f64,
}

impl Action2 for SetNotificationsPosition {
    fn desc(&self) -> Action2Options {
        let setting = format!("config.{}", NotificationsSettings::NotificationsPosition.as_str());
        let command = CommandAction {
            toggled: Some(Toggled::Expression(ContextKeyExpr::equals(&setting, self.position.as_str()))),
            ..CommandAction::new(self.id, localized_title(self.title.0, self.title.1))
        };
        Action2Options {
            command,
            menu: vec![Action2MenuItem { id: notifications_center_position_menu(), null_precondition: false, alt: None, when: None, group: None, order: Some(self.order), is_hidden_by_default: None }],
            keybinding: Vec::new(),
            f1: false,
        }
    }

    fn run(&self, accessor: &je_instantiation::ServicesAccessor, _args: &[Dynamic]) {
        let _ = accessor.get(I_CONFIGURATION_SERVICE).update_value(NotificationsSettings::NotificationsPosition.as_str(), Dynamic::from(self.position.as_str()), None, None);
    }
}

/// Os três `registerAction2` das posições (`SetNotificationsPositionBottomRight`, `...BottomLeft` e `...TopRight`).
pub fn register_notification_position_actions() -> DisposableRef {
    let positions = [
        ("workbench.action.setNotificationsPosition.bottomRight", ("positionBottomRight", "Bottom Right"), NotificationsPosition::BottomRight, 1.0),
        ("workbench.action.setNotificationsPosition.bottomLeft", ("positionBottomLeft", "Bottom Left"), NotificationsPosition::BottomLeft, 2.0),
        ("workbench.action.setNotificationsPosition.topRight", ("positionTopRight", "Top Right"), NotificationsPosition::TopRight, 3.0),
    ];
    je_base::lifecycle::combined_disposable(positions.into_iter().map(|(id, title, position, order)| register_action2(Rc::new(SetNotificationsPosition { id, title, position, order }))).collect())
}

/// O `NotificationViewItem` como contexto de uma ação (o item vivo, não uma cópia).
pub fn notification_view_item_context(item: &Rc<NotificationViewItem>) -> Dynamic {
    Dynamic::Object(ObjectRef::with_host(item.clone()))
}

/// `isNotificationViewItem(context)`: o item que o contexto carrega, se for um.
pub fn notification_view_item_from_context(context: &Dynamic) -> Option<Rc<NotificationViewItem>> {
    match context {
        Dynamic::Object(object) => object.host::<NotificationViewItem>(),
        _ => None,
    }
}

/// `NotificationActionRunner`.
pub struct NotificationActionRunner {
    runner: Rc<ActionRunner>,
    pub telemetry_surface: Option<NotificationTelemetrySurface>,
}

impl NotificationActionRunner {
    pub fn new(telemetry_surface: Option<NotificationTelemetrySurface>, telemetry_service: Rc<dyn ITelemetryService>, notification_service: Rc<dyn INotificationService>) -> Rc<Self> {
        let runner = ActionRunner::with_run_action(Rc::new(move |action: &ActionRef, context: Option<Dynamic>| -> Promise<()> {
            telemetry_service.public_log2("workbenchActionExecuted", Some(Dynamic::object([("id", Dynamic::from(action.id())), ("from", Dynamic::from("message"))])));
            if let Some(item) = context.as_ref().and_then(notification_view_item_from_context) {
                log_notification_action(&*telemetry_service, &item, action, telemetry_surface);
            }

            // Run and make sure to notify on any error again
            let started = errors::catch(|| action.run(&[context.unwrap_or(Dynamic::Undefined)]));
            let notification_service = notification_service.clone();
            async_fn(async move {
                let result: Result<(), Error> = match started {
                    Ok(promise) => promise.await.map(|_| ()),
                    Err(error) => Err(error),
                };
                if let Err(error) = result {
                    notification_service.error(vec![NotificationMessage::Error(error)]);
                }
                Ok(())
            })
        }));
        Rc::new(Self { runner, telemetry_surface })
    }
}

impl IActionRunner for NotificationActionRunner {
    fn on_did_run(&self) -> Event<IRunEvent> {
        self.runner.on_did_run()
    }

    fn on_will_run(&self) -> Event<IRunEvent> {
        self.runner.on_will_run()
    }

    fn run(&self, action: &ActionRef, context: Option<Dynamic>) -> Promise<()> {
        self.runner.run(action, context)
    }
}

impl IDisposable for NotificationActionRunner {
    fn dispose(&self) {
        self.runner.dispose();
    }

    fn constructor_name(&self) -> &'static str {
        "NotificationActionRunner"
    }
}
