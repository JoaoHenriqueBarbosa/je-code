//! Porte de `src/vs/workbench/browser/parts/notifications/notificationsCenter.ts` (e das regras de
//! `media/notificationsCenter.css` e `media/notificationsActions.css` que o centro usa): o [`NotificationsCenter`], a
//! janela flutuante com o cabeçalho (título e barra de ações) e a [`NotificationsList`].
//!
//! Notas de tradução:
//! - **DOM e CSS viram árvore do `je-ui` com layout calculado.** O `position: absolute` do `.notifications-center`
//!   (`right: 7px`, `bottom: 29px`, ou `11px` sem barra de status, `left: 7px` em `bottom-left`, `top: 7px` mais a barra de
//!   título em `top-right`), a borda de 1px, o cabeçalho de 35px (`padding-left: 8px`, `padding-right: 5px`, título em
//!   caixa alta de 11px, barra de ações à direita) estão em [`NotificationsCenter::place`], com as mesmas medidas do CSS.
//!   As classes `visible`, `bottom-right`, `bottom-left` e `top-right` ficam no nó (o `display: none` é o `visible` do
//!   estilo do nó).
//! - **`extends Themable`** é o [`IThemeService`] de [`NotificationsCenterServices`]: `getColor(id)` é a cor do tema
//!   atual convertida para [`Rgba`], e o `updateStyles` roda também quando o tema muda.
//! - **`@IInstantiationService`/`@IContextMenuService`/`@IKeybindingService`...** são os serviços de
//!   [`NotificationsCenterServices`] e de [`NotificationRendererServices`], passados à mão.
//! - **`createActionViewItem(instantiationService, action, options)`** (o item padrão das ações de menu) é o item que a
//!   [`ActionBar`] cria quando o provedor devolve `None`.
//! - **`NotificationFocusedContext.bindTo(list.contextKeyService)`** (a chave com o valor padrão `true` num contexto
//!   escopado da lista) é a chave do contexto global ligada ao foco da lista: `true` com o foco na lista, `false` sem ele;
//!   a superfície `center` com o foco vai para [`set_focused_notification_surface`] (o `closest('.notifications-center')`
//!   do original).
//! - **`Dimension.lift(dimension)`** não existe: o `Dimension` do `je-editor` já é a dimensão.
//! - **`isAncestorOfActiveElement(container)`** é [`is_ancestor_of_active_element`] sobre o nó do contêiner.

use std::cell::{Cell, RefCell};
use std::rc::{Rc, Weak};

use je_base::actions::{ActionRef, IAction, IActionRunner, Separator, ToActionProps, to_action};
use je_base::dynamic::Dynamic;
use je_base::event::{Emitter, Event};
use je_base::lifecycle::{Disposable, IDisposable};
use je_base::nls::localize;
use je_base::promise::MaybePromise;
use je_base::themables::ThemeIcon;
use je_editor::code_action::platform::{AccessibilitySignal, IAccessibilitySignalService};
use je_editor::core_2d::dimension::Dimension;
use je_editor::services::theme_service::IThemeService;
use je_platform::actions::menu_ids::notifications_center_position_menu;
use je_platform::actions::{IMenuService, MenuAction};
use je_platform::commands::ICommandService;
use je_platform::contextkey::{IContextKey, IContextKeyService};
use je_platform::notification::{NotificationFilterSetting, NotificationSourceFilter, NotificationsFilter};
use je_platform::window::DEFAULT_CUSTOM_TITLEBAR_HEIGHT;
use je_render::Rgba;
use je_render::kurbo::Rect;
use je_ui::{Border, NodeId, Role, Ui};
use je_widgets::action_bar::{ActionBar, IActionBarOptions, IActionOptions};
use je_widgets::action_view_items::{IActionViewItem, IActionViewItemOptions};
use je_widgets::dom::{FocusTracker, create_el, is_ancestor_of_active_element};
use je_widgets::dropdown::IActionProvider;
use je_widgets::dropdown_action_view_item::{DropdownMenuActionViewItem, IDropdownMenuActionViewItemOptions, MenuActions};
use je_widgets::text::{set_node_text, ui_font};

use crate::browser::parts::notifications::notifications_actions::{
    ClearAllNotificationsAction, ConfigureDoNotDisturbAction, ConfigureNotificationsPositionAction, HIDE_ICON, HIDE_UP_ICON, HideNotificationsCenterAction, ToggleDoNotDisturbAction, ToggleDoNotDisturbBySourceAction,
};
use crate::browser::parts::notifications::notifications_commands::{INotificationsCenterController, NotificationActionRunner, set_focused_notification_surface};
use crate::browser::parts::notifications::notifications_list::{NotificationsList, NotificationsListOptions};
use crate::browser::parts::notifications::notifications_viewer::{NotificationRendererServices, on_did_change_notification_row_height};
use crate::common::contextkeys::{notification_focused_context, notifications_center_visible_context};
use crate::common::notification_telemetry::{NotificationActionRole, NotificationInteraction, NotificationTelemetrySurface, log_notification_interaction};
use crate::common::notifications::{INotificationsModel, NotificationChangeEvent, NotificationChangeType, NotificationViewItemContentChangeKind, NotificationsPosition, NotificationsSettings, get_notifications_position};
use crate::common::theme::{NOTIFICATIONS_BACKGROUND, NOTIFICATIONS_CENTER_BORDER, NOTIFICATIONS_CENTER_HEADER_BACKGROUND, NOTIFICATIONS_CENTER_HEADER_FOREGROUND};
use crate::services::editor::editor_groups_service::IEditorGroupsService;
use crate::services::layout::layout_service::{IWorkbenchLayoutService, MAIN_WINDOW, Parts};

/// Os serviços que o construtor do centro recebe por injeção no original (além dos de
/// [`NotificationRendererServices`], que a lista usa).
#[derive(Clone)]
pub struct NotificationsCenterServices {
    pub renderer: NotificationRendererServices,
    pub theme_service: Rc<dyn IThemeService>,
    pub layout_service: Rc<dyn IWorkbenchLayoutService>,
    pub context_key_service: Rc<dyn IContextKeyService>,
    pub editor_groups_service: Rc<dyn IEditorGroupsService>,
    pub accessibility_signal_service: Rc<dyn IAccessibilitySignalService>,
    pub menu_service: Rc<dyn IMenuService>,
    pub command_service: Rc<dyn ICommandService>,
}

/// `.notifications-center-header` (`height: 35px`).
const HEADER_HEIGHT: f64 = 35.0;

/// `padding-left: 8px` e `padding-right: 5px` do cabeçalho.
const HEADER_PADDING_LEFT: f64 = 8.0;
const HEADER_PADDING_RIGHT: f64 = 5.0;

/// `font-size: 11px` do título do cabeçalho.
const TITLE_FONT_SIZE: f32 = 11.0;

/// O espaço entre o centro e a borda do workbench (`right: 7px`, `left: 7px`, `top: 7px`).
const EDGE_OFFSET: f64 = 7.0;

/// `bottom: 29px` (22px da barra de status mais 7px) e `bottom: 11px` sem barra de status.
const BOTTOM_OFFSET_WITH_STATUS_BAR: f64 = 29.0;
const BOTTOM_OFFSET_WITHOUT_STATUS_BAR: f64 = 11.0;

/// `NotificationsCenter`.
pub struct NotificationsCenter {
    base: Rc<Disposable>,
    weak_self: Weak<NotificationsCenter>,
    container: NodeId,
    model: Rc<dyn INotificationsModel>,
    services: NotificationsCenterServices,

    on_did_change_visibility: Rc<Emitter<()>>,

    center_container: Cell<Option<NodeId>>,
    center_header: Cell<Option<NodeId>>,
    center_title: Cell<Option<NodeId>>,
    toolbar_container: Cell<Option<NodeId>>,
    toolbar: RefCell<Option<Rc<ActionBar>>>,
    notifications_list: RefCell<Option<Rc<NotificationsList>>>,
    is_visible: Cell<Option<bool>>,
    workbench_dimensions: Cell<Option<(f64, f64)>>,
    /// A largura que a lista recebeu em [`NotificationsCenter::layout`].
    list_width: Cell<f64>,
    /// A largura do texto do título (o que o `flex` do cabeçalho mede).
    title_width: Cell<f64>,
    notifications_center_visible_context_key: Rc<dyn IContextKey>,
    clear_all_action: RefCell<Option<Rc<ClearAllNotificationsAction>>>,
    configure_do_not_disturb_action: RefCell<Option<Rc<ConfigureDoNotDisturbAction>>>,
    hide_action: RefCell<Option<Rc<HideNotificationsCenterAction>>>,
}

impl NotificationsCenter {
    const MAX_DIMENSIONS: (f64, f64) = (450.0, 400.0);

    /// Maximum number of notification sources to show in configure dropdown.
    const MAX_NOTIFICATION_SOURCES: usize = 10;

    pub fn new(container: NodeId, model: Rc<dyn INotificationsModel>, services: NotificationsCenterServices) -> Rc<Self> {
        let notifications_center_visible_context_key = notifications_center_visible_context().bind_to(&*services.context_key_service);
        let this = Rc::new_cyclic(|weak| Self {
            base: Disposable::new(),
            weak_self: weak.clone(),
            container,
            model,
            services,
            on_did_change_visibility: Emitter::new(),
            center_container: Cell::new(None),
            center_header: Cell::new(None),
            center_title: Cell::new(None),
            toolbar_container: Cell::new(None),
            toolbar: RefCell::new(None),
            notifications_list: RefCell::new(None),
            is_visible: Cell::new(None),
            workbench_dimensions: Cell::new(None),
            list_width: Cell::new(Self::MAX_DIMENSIONS.0),
            title_width: Cell::new(0.0),
            notifications_center_visible_context_key,
            clear_all_action: RefCell::new(None),
            configure_do_not_disturb_action: RefCell::new(None),
            hide_action: RefCell::new(None),
        });
        this.base.register(this.on_did_change_visibility.clone());

        this.register_listeners();
        this
    }

    /// `onDidChangeVisibility`.
    pub fn on_did_change_visibility(&self) -> Event<()> {
        self.on_did_change_visibility.event()
    }

    fn ui(&self) -> &Ui {
        &self.services.renderer.ui
    }

    /// `getColor(id)`: a cor do tema atual.
    fn get_color(&self, id: &str) -> Option<Rgba> {
        self.services.theme_service.get_color_theme().get_color(id, None).map(|color| je_theme::to_rgba8(&color))
    }

    fn register_listeners(&self) {
        let weak = self.weak_self.clone();
        self.base.register(self.model.on_did_change_notification().listen({
            let weak = weak.clone();
            move |event| {
                if let Some(this) = weak.upgrade() {
                    this.on_did_change_notification(event);
                }
            }
        }));
        self.base.register(self.services.layout_service.on_did_layout_main_container().listen({
            let weak = weak.clone();
            move |dimension| {
                if let Some(this) = weak.upgrade() {
                    this.layout(Some(Dimension { width: dimension.width, height: dimension.height }));
                }
            }
        }));
        self.base.register(self.services.renderer.notification_service.on_did_change_filter().listen({
            let weak = weak.clone();
            move |_| {
                if let Some(this) = weak.upgrade() {
                    this.on_did_change_filter();
                }
            }
        }));
        self.base.register(self.services.renderer.configuration_service.on_did_change_configuration().listen({
            let weak = weak.clone();
            move |event| {
                if event.affects_configuration(NotificationsSettings::NotificationsPosition.as_str(), None)
                    && let Some(this) = weak.upgrade()
                {
                    this.update_position_class();
                }
            }
        }));
        self.base.register(self.services.theme_service.on_did_color_theme_change().listen(move |_| {
            if let Some(this) = weak.upgrade() {
                this.update_styles();
            }
        }));
    }

    fn position(&self) -> NotificationsPosition {
        get_notifications_position(&*self.services.renderer.configuration_service)
    }

    fn update_position_class(&self) {
        let Some(center) = self.center_container.get() else { return };

        let position = self.position();
        {
            let mut tree = self.ui().tree_mut();
            for class in ["bottom-right", "bottom-left", "top-right"] {
                tree.remove_class(center, class);
            }
            tree.add_class(center, position.as_str());
        }

        self.update_hide_action_icon();
        self.place();
    }

    fn update_hide_action_icon(&self) {
        let Some(hide_action) = self.hide_action.borrow().clone() else { return };

        let icon = if self.position() == NotificationsPosition::TopRight { HIDE_UP_ICON } else { HIDE_ICON };
        hide_action.set_class(Some(ThemeIcon::as_class_name(&icon.to_theme_icon())));
    }

    fn on_did_change_filter(&self) {
        if self.services.renderer.notification_service.get_filter(None) == NotificationsFilter::Error {
            self.hide(); // hide the notification center when we have a error filter enabled
        }
    }

    pub fn is_visible(&self) -> bool {
        self.is_visible.get() == Some(true)
    }

    fn list(&self) -> Option<Rc<NotificationsList>> {
        self.notifications_list.borrow().clone()
    }

    pub fn show(&self) {
        if self.is_visible() {
            let Some(notifications_list) = self.list() else { return };

            // Make visible
            notifications_list.show();

            // Focus first
            notifications_list.focus_first();

            return; // already visible
        }

        // Lazily create if showing for the first time
        if self.center_container.get().is_none() {
            self.create();
        }

        // Title
        self.update_title();

        // Make visible
        let (Some(notifications_list), Some(center)) = (self.list(), self.center_container.get()) else { return };
        self.is_visible.set(Some(true));
        {
            let mut tree = self.ui().tree_mut();
            tree.add_class(center, "visible");
            tree.style_mut(center).visible = true;
        }
        notifications_list.show();

        // Layout
        self.layout(self.workbench_dimensions.get().map(|(width, height)| Dimension { width, height }));

        // Show all notifications that are present now
        notifications_list.update_notifications_list(0, 0, self.model.notifications());
        self.place();

        // Focus first
        notifications_list.focus_first();

        // Theming
        self.update_styles();

        // Mark as visible
        for notification in self.model.notifications() {
            notification.update_visibility(true);
        }
        notifications_list.set_telemetry_visibility(true);

        // Context Key
        self.notifications_center_visible_context_key.set(Dynamic::from(true));

        // Event
        self.on_did_change_visibility.fire(());
    }

    fn update_title(&self) {
        let (Some(title), Some(clear_all_action)) = (self.center_title.get(), self.clear_all_action.borrow().clone()) else { return };

        let notifications = self.model.notifications();
        let text = if notifications.is_empty() {
            clear_all_action.set_enabled(false);
            localize("notificationsEmpty", "No new notifications", &[])
        } else {
            clear_all_action.set_enabled(notifications.iter().any(|notification| !notification.has_active_progress()));
            localize("notifications", "Notifications", &[])
        };
        self.render_title(title, &text);
        self.place();
    }

    /// `textContent` do título, em caixa alta (`text-transform: uppercase`) e a 11px.
    fn render_title(&self, title: NodeId, text: &str) {
        let color = self.get_color(NOTIFICATIONS_CENTER_HEADER_FOREGROUND).or_else(|| self.get_color("foreground")).unwrap_or(self.services.renderer.styles.foreground);
        let width = set_node_text(&mut self.ui().tree_mut(), title, &text.to_uppercase(), &ui_font(TITLE_FONT_SIZE, 400), color, HEADER_HEIGHT);
        self.title_width.set(width);
    }

    fn create(&self) {
        let ui = self.ui().clone();
        let services = &self.services;

        // Container
        let (center, header, title, toolbar_container) = {
            let mut tree = ui.tree_mut();
            let center = create_el(&mut tree, self.container, Role::Generic, "notifications-center");
            tree.style_mut(center).clip = true;
            tree.style_mut(center).visible = false;
            tree.style_mut(center).radius = 0.0;

            // Header
            let header = create_el(&mut tree, center, Role::Generic, "notifications-center-header");

            // Header Title
            let title = create_el(&mut tree, header, Role::Generic, "notifications-center-header-title");

            // Header Toolbar
            let toolbar_container = create_el(&mut tree, header, Role::Generic, "notifications-center-header-toolbar");
            (center, header, title, toolbar_container)
        };
        self.center_container.set(Some(center));
        self.center_header.set(Some(header));
        self.center_title.set(Some(title));
        self.toolbar_container.set(Some(toolbar_container));

        // Apply position class
        self.update_position_class();

        let action_runner = NotificationActionRunner::new(Some(NotificationTelemetrySurface::Center), services.renderer.telemetry_service.clone(), services.renderer.notification_service.clone());
        self.base.register(action_runner.clone());
        let action_runner: Rc<dyn IActionRunner> = action_runner;

        let notification_service = services.renderer.notification_service.clone();
        let keybinding_service = services.renderer.keybinding_service.clone();
        let command_service = services.command_service.clone();
        let menu_service = services.menu_service.clone();
        let context_key_service = services.context_key_service.clone();
        let context_menu = services.renderer.dropdown_context_menu.clone();
        let ui_for_provider = ui.clone();
        let runner_for_provider = action_runner.clone();
        let provider = Rc::new(move |action: &ActionRef, options: &IActionViewItemOptions| -> Option<Rc<dyn IActionViewItem>> {
            let class_names: Vec<String> = action.class().map(|class| class.split_whitespace().map(str::to_owned).collect()).unwrap_or_default();
            let item_options = {
                let keybinding_service = keybinding_service.clone();
                IDropdownMenuActionViewItemOptions {
                    base: options.clone(),
                    action_runner: Some(runner_for_provider.clone()),
                    class_names,
                    keybinding_provider: Some(Rc::new(move |action: &ActionRef| keybinding_service.lookup_keybinding(&action.id(), None, false))),
                    ..Default::default()
                }
            };

            if action.id() == ConfigureNotificationsPositionAction::ID {
                let (menu_service, context_key_service) = (menu_service.clone(), context_key_service.clone());
                let actions: Rc<dyn IActionProvider> = Rc::new(move || {
                    let groups = menu_service.get_menu_actions(&notifications_center_position_menu(), &*context_key_service, None);
                    let lists: Vec<Vec<ActionRef>> = groups.into_iter().map(|(_, actions)| actions.into_iter().map(menu_action_ref).collect()).collect();
                    Separator::join(&lists)
                });
                return Some(DropdownMenuActionViewItem::new(&ui_for_provider, action.clone(), MenuActions::Provider(actions), context_menu.clone(), item_options) as Rc<dyn IActionViewItem>);
            }

            if action.id() == ConfigureDoNotDisturbAction::ID {
                let (notification_service, command_service) = (notification_service.clone(), command_service.clone());
                let actions: Rc<dyn IActionProvider> = Rc::new(move || do_not_disturb_menu_actions(&notification_service, &command_service));
                return Some(DropdownMenuActionViewItem::new(&ui_for_provider, action.clone(), MenuActions::Provider(actions), context_menu.clone(), item_options) as Rc<dyn IActionViewItem>);
            }

            None
        });

        let toolbar = ActionBar::new(
            &ui,
            toolbar_container,
            IActionBarOptions { aria_label: Some(localize("notificationsToolbar", "Notification Center Actions", &[])), action_runner: Some(action_runner), action_view_item_provider: Some(provider), ..Default::default() },
        );
        self.base.register(toolbar.clone());
        *self.toolbar.borrow_mut() = Some(toolbar.clone());

        let command_service = services.command_service.clone();
        let clear_all_action = ClearAllNotificationsAction::new(ClearAllNotificationsAction::ID, &ClearAllNotificationsAction::label(), command_service.clone());
        self.base.register(clear_all_action.clone());
        *self.clear_all_action.borrow_mut() = Some(clear_all_action.clone());
        self.push_toolbar_action(&toolbar, clear_all_action, true);

        let configure_do_not_disturb_action = ConfigureDoNotDisturbAction::new(ConfigureDoNotDisturbAction::ID, &ConfigureDoNotDisturbAction::label());
        self.base.register(configure_do_not_disturb_action.clone());
        *self.configure_do_not_disturb_action.borrow_mut() = Some(configure_do_not_disturb_action.clone());
        self.push_toolbar_action(&toolbar, configure_do_not_disturb_action, false);

        let configure_notifications_position_action = ConfigureNotificationsPositionAction::new(ConfigureNotificationsPositionAction::ID, &ConfigureNotificationsPositionAction::label());
        self.base.register(configure_notifications_position_action.clone());
        self.push_toolbar_action(&toolbar, configure_notifications_position_action, false);

        let hide_action = HideNotificationsCenterAction::new(HideNotificationsCenterAction::ID, &HideNotificationsCenterAction::label(), command_service);
        self.base.register(hide_action.clone());
        *self.hide_action.borrow_mut() = Some(hide_action.clone());
        self.update_hide_action_icon();
        self.push_toolbar_action(&toolbar, hide_action, true);

        // Notifications List
        let list_options = {
            let focus_key = notification_focused_context().bind_to(&*services.context_key_service);
            focus_key.set(Dynamic::from(false));
            let (ui, base) = (ui.clone(), self.base.clone());
            NotificationsListOptions {
                telemetry_surface: Some(NotificationTelemetrySurface::Center),
                widget_aria_label: Some(localize("notificationsCenterWidgetAriaLabel", "Notifications Center", &[])),
                background: self.get_color(NOTIFICATIONS_BACKGROUND),
                on_list_created: Some(Rc::new(move |list| {
                    let tracker = FocusTracker::new(&ui, list.get_html_element());
                    base.register(tracker.clone());
                    let key = focus_key.clone();
                    base.register(tracker.on_did_focus().listen(move |_| {
                        key.set(Dynamic::from(true));
                        set_focused_notification_surface(Some(NotificationTelemetrySurface::Center));
                    }));
                    let key = focus_key.clone();
                    base.register(tracker.on_did_blur().listen(move |_| {
                        key.set(Dynamic::from(false));
                        set_focused_notification_surface(None);
                    }));
                })),
                ..Default::default()
            }
        };
        let notifications_list = NotificationsList::new(center, list_options, services.renderer.clone());
        self.base.register(notifications_list.clone());
        *self.notifications_list.borrow_mut() = Some(notifications_list);

        let weak = self.weak_self.clone();
        self.base.register(on_did_change_notification_row_height().listen(move |_| {
            if let Some(list) = weak.upgrade().and_then(|this| this.list()) {
                list.update_notification_heights();
            }
        }));
    }

    /// `notificationsToolBar.push(action, { icon: true, label: false, keybinding })`.
    fn push_toolbar_action(&self, toolbar: &Rc<ActionBar>, action: ActionRef, with_keybinding: bool) {
        let keybinding = if with_keybinding { self.get_keybinding_label(&action) } else { None };
        toolbar.push_action(action, IActionOptions { item: IActionViewItemOptions { icon: Some(true), label: Some(false), keybinding, ..Default::default() }, index: None });
    }

    fn get_keybinding_label(&self, action: &ActionRef) -> Option<String> {
        self.services.renderer.keybinding_service.lookup_keybinding(&action.id(), None, false).and_then(|keybinding| keybinding.get_label())
    }

    fn on_did_change_notification(&self, e: &NotificationChangeEvent) {
        if !self.is_visible() {
            return; // only if visible
        }

        let mut focus_editor = false;

        // Update notifications list based on event kind
        let (Some(notifications_list), Some(center)) = (self.list(), self.center_container.get()) else { return };
        match e.kind {
            NotificationChangeType::Add => {
                notifications_list.update_notifications_list(e.index, 0, vec![e.item.clone()]);
                e.item.update_visibility(true);
            }
            NotificationChangeType::Change => {
                // Handle content changes
                // - actions: re-draw to properly show them
                // - message: update notification height unless collapsed
                match e.detail {
                    Some(NotificationViewItemContentChangeKind::Actions) => notifications_list.update_notifications_list(e.index, 1, vec![e.item.clone()]),
                    Some(NotificationViewItemContentChangeKind::Message) => {
                        if e.item.expanded() {
                            notifications_list.update_notification_height(&e.item);
                        }
                    }
                    Some(NotificationViewItemContentChangeKind::Progress) => {
                        if e.active_progress_changed == Some(true) {
                            notifications_list.update_notifications_list(e.index, 1, vec![e.item.clone()]);
                        }
                    }
                    _ => {}
                }
            }
            NotificationChangeType::ExpandCollapse => {
                // Re-draw entire item when expansion changes to reveal or hide details
                notifications_list.update_notifications_list(e.index, 1, vec![e.item.clone()]);
            }
            NotificationChangeType::Remove => {
                focus_editor = is_ancestor_of_active_element(self.ui(), center);
                notifications_list.update_notifications_list(e.index, 1, Vec::new());
                e.item.update_visibility(false);
            }
        }

        // Update title
        self.update_title();

        // Hide if no more notifications to show
        if self.model.notifications().is_empty() {
            self.hide();

            // Restore focus to editor group if we had focus
            if focus_editor {
                self.services.editor_groups_service.active_group().focus();
            }
        }
    }

    pub fn hide(&self) {
        let (Some(center), Some(notifications_list)) = (self.center_container.get(), self.list()) else { return };
        if !self.is_visible() {
            return; // already hidden
        }

        let focus_editor = is_ancestor_of_active_element(self.ui(), center);

        // Hide
        self.is_visible.set(Some(false));
        {
            let mut tree = self.ui().tree_mut();
            tree.remove_class(center, "visible");
            tree.style_mut(center).visible = false;
        }
        notifications_list.hide();

        // Mark as hidden
        for notification in self.model.notifications() {
            notification.update_visibility(false);
        }

        // Context Key
        self.notifications_center_visible_context_key.set(Dynamic::from(false));

        // Event
        self.on_did_change_visibility.fire(());

        // Restore focus to editor group if we had focus
        if focus_editor {
            self.services.editor_groups_service.active_group().focus();
        }
    }

    pub fn update_styles(&self) {
        let (Some(center), Some(header)) = (self.center_container.get(), self.center_header.get()) else { return };

        let border_color = self.get_color(NOTIFICATIONS_CENTER_BORDER);
        let header_background = self.get_color(NOTIFICATIONS_CENTER_HEADER_BACKGROUND);
        {
            let mut tree = self.ui().tree_mut();
            tree.style_mut(center).border = border_color.map(|color| Border { width: 1.0, color });
            tree.style_mut(header).background = header_background;
        }

        // `headerForeground` é a cor do título.
        if let Some(title) = self.center_title.get() {
            let text = self.title_text();
            self.render_title(title, &text);
        }
        self.place();
    }

    /// O texto do título para o estado atual do modelo.
    fn title_text(&self) -> String {
        if self.model.notifications().is_empty() { localize("notificationsEmpty", "No new notifications", &[]) } else { localize("notifications", "Notifications", &[]) }
    }

    /// `updateTopOffset`: o `top` em `top-right` (7px mais a barra de título), o `bottom` nas outras posições.
    fn top_offset(&self) -> f64 {
        let mut top_offset = EDGE_OFFSET;
        if self.services.layout_service.is_visible(Parts::TitlebarPart, Some(MAIN_WINDOW)) {
            top_offset += f64::from(DEFAULT_CUSTOM_TITLEBAR_HEIGHT);
        }
        top_offset
    }

    /// As regras de CSS do `.notifications-center` e do cabeçalho: posição absoluta no workbench, borda, cabeçalho de 35px
    /// com o título à esquerda e a barra de ações à direita (`justify-content: flex-end`), a lista logo abaixo.
    fn place(&self) {
        let (Some(center), Some(header), Some(title), Some(toolbar_container), Some(list), Some(toolbar)) =
            (self.center_container.get(), self.center_header.get(), self.center_title.get(), self.toolbar_container.get(), self.list(), self.toolbar.borrow().clone())
        else {
            return;
        };
        let ui = self.ui();

        let border = ui.tree().style(center).border.map_or(0.0, |border| border.width);
        let list_width = self.list_width.get();
        let list_height = if self.is_visible() { list.display_height() } else { 0.0 };
        let outer_width = list_width + 2.0 * border;
        let outer_height = HEADER_HEIGHT + list_height + 2.0 * border;

        let (workbench_width, workbench_height) = self.workbench_dimensions.get().unwrap_or((Self::MAX_DIMENSIONS.0 + 2.0 * EDGE_OFFSET, Self::MAX_DIMENSIONS.1 + HEADER_HEIGHT + 2.0 * EDGE_OFFSET));
        let bottom_offset = if self.services.layout_service.is_visible(Parts::StatusbarPart, Some(MAIN_WINDOW)) { BOTTOM_OFFSET_WITH_STATUS_BAR } else { BOTTOM_OFFSET_WITHOUT_STATUS_BAR };
        let bottom_y = workbench_height - bottom_offset - outer_height;
        let (x, y) = match self.position() {
            NotificationsPosition::BottomRight => (workbench_width - EDGE_OFFSET - outer_width, bottom_y),
            NotificationsPosition::BottomLeft => (EDGE_OFFSET, bottom_y),
            NotificationsPosition::TopRight => (workbench_width - EDGE_OFFSET - outer_width, self.top_offset()),
        };

        // Toolbar: `flex: 1` com `justify-content: flex-end`, as ações coladas à direita
        let toolbar_width: f64 = (0..toolbar.length()).map(|index| toolbar.get_width(index)).sum();
        let title_width = self.title_width.get();
        let toolbar_right = list_width - HEADER_PADDING_RIGHT;
        let toolbar_left = (toolbar_right - toolbar_width).max(HEADER_PADDING_LEFT + title_width);

        {
            let mut tree = ui.tree_mut();
            tree.set_bounds(center, Rect::new(x, y, x + outer_width, y + outer_height));
            tree.set_bounds(header, Rect::new(border, border, border + list_width, border + HEADER_HEIGHT));
            tree.set_bounds(title, Rect::new(HEADER_PADDING_LEFT, 0.0, HEADER_PADDING_LEFT + title_width, HEADER_HEIGHT));
            tree.set_bounds(toolbar_container, Rect::new(toolbar_left, 0.0, toolbar_right.max(toolbar_left), HEADER_HEIGHT));
            if let Some(list_container) = list.container_node() {
                let top = border + HEADER_HEIGHT;
                let bounds = tree.bounds(list_container);
                tree.set_bounds(list_container, Rect::new(border, top, border + bounds.width().max(list_width), top + list_height));
            }
        }
        toolbar.layout();
    }

    pub fn layout(&self, dimension: Option<Dimension>) {
        self.workbench_dimensions.set(dimension.map(|dimension| (dimension.width, dimension.height)));

        if self.is_visible() && self.center_container.get().is_some() {
            let (max_width, max_height) = Self::MAX_DIMENSIONS;

            let mut available_width = max_width;
            let mut available_height = max_height;

            if let Some(dimension) = dimension {
                // Make sure notifications are not exceding available width
                available_width = dimension.width;
                available_width -= 2.0 * 8.0; // adjust for paddings left and right

                // Make sure notifications are not exceeding available height
                available_height = dimension.height - HEADER_HEIGHT;
                if self.services.layout_service.is_visible(Parts::StatusbarPart, Some(MAIN_WINDOW)) {
                    available_height -= 22.0; // adjust for status bar
                }

                if self.services.layout_service.is_visible(Parts::TitlebarPart, Some(MAIN_WINDOW)) {
                    available_height -= 22.0; // adjust for title bar
                }

                available_height -= 2.0 * 12.0; // adjust for paddings top and bottom
            }

            // Apply to list
            let Some(notifications_list) = self.list() else { return };
            let list_width = max_width.min(available_width);
            self.list_width.set(list_width);
            notifications_list.layout(list_width, Some(max_height.min(available_height)));
            self.place();
        }
    }

    pub fn clear_all(&self) {
        let surface = if self.is_visible() { Some(NotificationTelemetrySurface::Center) } else { None };

        // Hide notifications center first
        self.hide();

        // Close all
        for notification in self.model.notifications() /* copy array since we modify it from closing */ {
            if !notification.has_active_progress() {
                log_notification_interaction(&*self.services.renderer.telemetry_service, &notification, NotificationInteraction::ClearAll, surface, None, NotificationActionRole::None);
                notification.close();
            }
            self.services.accessibility_signal_service.play_signal(AccessibilitySignal::Clear);
        }
    }
}

impl INotificationsCenterController for NotificationsCenter {
    fn is_visible(&self) -> bool {
        NotificationsCenter::is_visible(self)
    }

    fn show(&self) {
        NotificationsCenter::show(self);
    }

    fn hide(&self) {
        NotificationsCenter::hide(self);
    }

    fn clear_all(&self) {
        NotificationsCenter::clear_all(self);
    }
}

impl IDisposable for NotificationsCenter {
    fn dispose(&self) {
        self.base.dispose();
    }

    fn constructor_name(&self) -> &'static str {
        "NotificationsCenter"
    }
}

/// O `MenuItemAction | SubmenuItemAction` do menu como ação.
fn menu_action_ref(action: MenuAction) -> ActionRef {
    match action {
        MenuAction::Item(item) => item,
        MenuAction::Submenu(submenu) => submenu.as_action(),
    }
}

/// O `getActions()` do dropdown de "não perturbe": ligar ou desligar tudo e, por fonte (as dez primeiras), ligar ou
/// desligar as notificações de informação e aviso.
fn do_not_disturb_menu_actions(notification_service: &Rc<dyn je_platform::notification::INotificationService>, command_service: &Rc<dyn ICommandService>) -> Vec<ActionRef> {
    let toggle_service = notification_service.clone();
    let mut actions: Vec<ActionRef> = vec![to_action(ToActionProps {
        id: ToggleDoNotDisturbAction::ID.to_owned(),
        label: if notification_service.get_filter(None) == NotificationsFilter::Off {
            localize("turnOnNotifications", "Enable Do Not Disturb Mode", &[])
        } else {
            localize("turnOffNotifications", "Disable Do Not Disturb Mode", &[])
        },
        tooltip: None,
        enabled: None,
        checked: None,
        class: None,
        alt: None,
        run: Rc::new(move |_| {
            let filter = if toggle_service.get_filter(None) == NotificationsFilter::Off { NotificationsFilter::Error } else { NotificationsFilter::Off };
            toggle_service.set_filter(NotificationFilterSetting::Global(filter));
            MaybePromise::from(Dynamic::Undefined)
        }),
    })];

    let mut sorted_filters = notification_service.get_filters();
    sorted_filters.sort_by(|a, b| a.source.label.to_lowercase().cmp(&b.source.label.to_lowercase()).then_with(|| a.source.label.cmp(&b.source.label)));
    for source in sorted_filters.iter().take(NotificationsCenter::MAX_NOTIFICATION_SOURCES) {
        if actions.len() == 1 {
            actions.push(Separator::new());
        }

        let (service, toggled) = (notification_service.clone(), source.clone());
        actions.push(to_action(ToActionProps {
            id: format!("{}.{}", ToggleDoNotDisturbAction::ID, source.source.id),
            label: source.source.label.clone(),
            tooltip: None,
            enabled: None,
            checked: Some(source.filter != NotificationsFilter::Error),
            class: None,
            alt: None,
            run: Rc::new(move |_| {
                service.set_filter(NotificationFilterSetting::Source(NotificationSourceFilter {
                    source: toggled.source.clone(),
                    filter: if toggled.filter == NotificationsFilter::Error { NotificationsFilter::Off } else { NotificationsFilter::Error },
                }));
                MaybePromise::from(Dynamic::Undefined)
            }),
        }));
    }

    if sorted_filters.len() > NotificationsCenter::MAX_NOTIFICATION_SOURCES {
        actions.push(Separator::new());
        actions.push(ToggleDoNotDisturbBySourceAction::new(ToggleDoNotDisturbBySourceAction::ID, &localize("moreSources", "More…", &[]), command_service.clone()));
    }

    actions
}
