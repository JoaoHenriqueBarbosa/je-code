//! Porte de `src/vs/workbench/browser/parts/notifications/notificationsToasts.ts` (e das regras de
//! `media/notificationsToasts.css` que os toasts usam): as [`NotificationsToasts`], a pilha de toasts no canto do
//! workbench, com o limite de toasts visíveis, a purga por tempo, o foco e o layout.
//!
//! Notas de tradução:
//! - **DOM e CSS viram árvore do `je-ui` com layout calculado.** O `position: absolute` do `.notifications-toasts`
//!   (`right: 3px`, `bottom: 25px`, ou `3px` sem barra de status, `left: 3px` em `bottom-left`, `top: 3px` mais a barra de
//!   título em `top-right`), a coluna de `flex` com a margem de 4px do `.notification-toast`, a borda de 1px do tema e o
//!   raio de 4px estão em [`NotificationsToasts::place`]. `appendChild`/`remove` são `append_child`/`detach` da árvore, e
//!   `isToastInDOM` é "o contêiner do toast tem pai". `offsetHeight` do contêiner é a altura da lista mais a borda e as
//!   margens, que [`NotificationsToasts::toast_outer_height`] calcula (a medição não depende de o nó estar visível).
//! - **`transform`/`opacity`/`transition` e `transitionend`.** O `je-ui` não anima por CSS: o toast entra já no estado
//!   final, então as classes `notification-fade-in` e `notification-fade-in-done` são postas em sequência (o
//!   `transition: 0ms` do `monaco-reduce-motion`) e a opacidade `0` da medição de [`NotificationsToasts::layout_container`]
//!   é a do estilo do nó. A sombra (`box-shadow`) do `.notifications-list-container` não tem equivalente.
//! - **`extends Themable`** é o [`IThemeService`] de [`NotificationsToastsServices`]: `getColor(id)` é a cor do tema atual
//!   convertida para [`Rgba`], e o `updateStyles` roda também quando o tema muda.
//! - **`@IInstantiationService`/`@IWorkbenchLayoutService`/...** são os serviços de [`NotificationsToastsServices`] e de
//!   [`NotificationRendererServices`], passados à mão.
//! - **`mouseover`/`mouseout` do contêiner do toast** são `PointerEnter`/`PointerLeave` (`mouseenter`/`mouseleave`), que
//!   para o rastreio de "o mouse está sobre o toast" têm o mesmo efeito.
//! - **`NotificationFocusedContext.bindTo(list.contextKeyService)`** (uma chave por lista no original) é a chave do
//!   contexto global, ligada ao foco de cada lista de toast pelo gancho `on_list_created`: ela vale `true` enquanto
//!   alguma lista de toast tem o foco, e a superfície `toast` vai para [`set_focused_notification_surface`].
//! - **`getActiveElement()`/`isEditableElement`** é o nó com foco da [`Ui`] com o papel `TextInput`;
//!   **`getDomNodePagePosition`** é `absolute_bounds` e **`getClientArea(mainContainer)`** é a dimensão do contêiner
//!   principal do layout.
//! - **`Event.once(item.onDidClose)(...)`** fica registrado nos descartáveis do item, para sair junto com o toast.
//! - **`INotificationsToastController`** não tem retorno nos métodos de foco; os métodos próprios devolvem o `boolean`
//!   do original e o trait descarta o valor.

use std::cell::{Cell, RefCell};
use std::rc::{Rc, Weak};

use je_base::async_::IntervalCounter;
use je_base::dynamic::Dynamic;
use je_base::event::{Emitter, Event};
use je_base::lifecycle::{Disposable, DisposableRef, DisposableStore, IDisposable, to_disposable};
use je_base::nls::localize;
use je_base::runtime::{TimerId, clear_timeout, set_timeout};
use je_base::scrollable::ScrollbarVisibility;
use je_editor::core_2d::dimension::Dimension;
use je_editor::services::theme_service::IThemeService;
use je_platform::contextkey::{IContextKey, IContextKeyService};
use je_platform::notification::{NotificationPriority, NotificationsFilter, Severity, with_severity_prefix};
use je_platform::window::DEFAULT_CUSTOM_TITLEBAR_HEIGHT;
use je_render::Rgba;
use je_render::kurbo::Rect;
use je_ui::{Border, EventKind, NodeId, Role, Ui};
use je_widgets::dom::{FocusTracker, create_detached_el, create_el, is_ancestor_of_active_element};

use crate::browser::parts::notifications::notifications_commands::{INotificationsToastController, set_focused_notification_surface};
use crate::browser::parts::notifications::notifications_list::{NotificationsList, NotificationsListOptions};
use crate::browser::parts::notifications::notifications_viewer::{NotificationRendererServices, on_did_change_notification_row_height};
use crate::browser::parts::notifications::pending_notification_toasts::PendingNotificationToasts;
use crate::common::contextkeys::{notification_focused_context, notifications_toasts_visible_context};
use crate::common::notification_telemetry::NotificationTelemetrySurface;
use crate::common::notifications::{
    INotificationsModel, NotificationChangeEvent, NotificationChangeType, NotificationViewItem, NotificationViewItemContentChangeKind, NotificationsPosition, NotificationsSettings, get_notifications_position,
};
use crate::common::theme::{NOTIFICATIONS_BACKGROUND, NOTIFICATIONS_TOAST_BORDER};
use crate::services::editor::editor_groups_service::IEditorGroupsService;
use crate::services::environment::environment_service::IWorkbenchEnvironmentService;
use crate::services::host::host::IHostService;
use crate::services::layout::layout_service::{IWorkbenchLayoutService, MAIN_WINDOW, Parts};
use crate::services::lifecycle::lifecycle::{ILifecycleService, LifecyclePhase};

type Item = Rc<NotificationViewItem>;

/// Os serviços que o construtor das notificações em toast recebe por injeção no original (além dos de
/// [`NotificationRendererServices`], que a lista usa).
#[derive(Clone)]
pub struct NotificationsToastsServices {
    pub renderer: NotificationRendererServices,
    pub layout_service: Rc<dyn IWorkbenchLayoutService>,
    pub theme_service: Rc<dyn IThemeService>,
    pub editor_groups_service: Rc<dyn IEditorGroupsService>,
    pub context_key_service: Rc<dyn IContextKeyService>,
    pub lifecycle_service: Rc<dyn ILifecycleService>,
    pub host_service: Rc<dyn IHostService>,
    pub environment_service: Rc<dyn IWorkbenchEnvironmentService>,
}

/// `INotificationToast`.
struct NotificationToast {
    item: Item,
    list: Rc<NotificationsList>,
    container: NodeId,
    toast: NodeId,
}

/// `ToastVisibility`.
#[derive(Clone, Copy, PartialEq, Eq)]
enum ToastVisibility {
    HiddenOrVisible,
    Hidden,
    Visible,
}

/// O que `purgeNotification` guarda em variáveis locais: o rastreio do mouse, o timer e o ouvinte de foco da janela.
struct PurgeState {
    toasts: Weak<NotificationsToasts>,
    item: Item,
    list: Rc<NotificationsList>,
    disposables: Rc<DisposableStore>,
    is_mouse_over_toast: Cell<bool>,
    purge_timeout_handle: Cell<Option<TimerId>>,
    listener: RefCell<Option<DisposableRef>>,
}

/// `.notification-toast` (`margin: 4px`).
const TOAST_MARGIN: f64 = 4.0;

/// `right: 3px`, `left: 3px` e `top: 3px`.
const EDGE_OFFSET: f64 = 3.0;

/// `bottom: 25px` (22px da barra de status mais 3px) e `bottom: 3px` sem barra de status.
const BOTTOM_OFFSET_WITH_STATUS_BAR: f64 = 25.0;
const BOTTOM_OFFSET_WITHOUT_STATUS_BAR: f64 = 3.0;

/// `border-radius: 4px` do toast.
const TOAST_RADIUS: f64 = 4.0;

/// `NotificationsToasts`.
pub struct NotificationsToasts {
    base: Rc<Disposable>,
    weak_self: Weak<NotificationsToasts>,
    container: NodeId,
    model: Rc<dyn INotificationsModel>,
    services: NotificationsToastsServices,

    on_did_change_visibility: Rc<Emitter<()>>,

    is_visible: Cell<bool>,

    notifications_toasts_container: Cell<Option<NodeId>>,
    workbench_dimensions: Cell<Option<(f64, f64)>>,
    is_notifications_center_visible: Cell<bool>,

    map_notification_to_toast: RefCell<Vec<(Item, Rc<NotificationToast>)>>,
    map_notification_to_disposable: RefCell<Vec<(Item, Rc<DisposableStore>)>>,
    pending_toasts: PendingNotificationToasts<Item>,

    notifications_toasts_visible_context_key: Rc<dyn IContextKey>,

    added_toasts_interval_counter: IntervalCounter,
}

impl NotificationsToasts {
    const MAX_WIDTH: f64 = 450.0;
    const MAX_NOTIFICATIONS: usize = 3;

    /// `SPAM_PROTECTION`: contar as notificações em 800ms e garantir que não passam de `MAX_NOTIFICATIONS`.
    const SPAM_PROTECTION_INTERVAL: u64 = 800;
    const SPAM_PROTECTION_LIMIT: usize = Self::MAX_NOTIFICATIONS;

    /// `PURGE_TIMEOUT[severity]`. Uma severidade sem entrada (`Ignore`) faz o `setTimeout` do original receber
    /// `undefined`, que vale zero.
    fn purge_timeout(severity: Severity) -> u64 {
        match severity {
            Severity::Info => 10000,
            Severity::Warning => 12000,
            Severity::Error => 15000,
            _ => 0,
        }
    }

    pub fn new(container: NodeId, model: Rc<dyn INotificationsModel>, services: NotificationsToastsServices) -> Rc<Self> {
        let notifications_toasts_visible_context_key = notifications_toasts_visible_context().bind_to(&*services.context_key_service);
        let ui = services.renderer.ui.clone();
        let this = Rc::new_cyclic(|weak: &Weak<NotificationsToasts>| {
            let is_current_model = model.clone();
            let schedule_ui = ui.clone();
            Self {
                base: Disposable::new(),
                weak_self: weak.clone(),
                container,
                model: model.clone(),
                services,
                on_did_change_visibility: Emitter::new(),
                is_visible: Cell::new(false),
                notifications_toasts_container: Cell::new(None),
                workbench_dimensions: Cell::new(None),
                is_notifications_center_visible: Cell::new(false),
                map_notification_to_toast: RefCell::new(Vec::new()),
                map_notification_to_disposable: RefCell::new(Vec::new()),
                pending_toasts: PendingNotificationToasts::new(
                    move |item: &Item| is_current_model.notifications().iter().any(|candidate| Rc::ptr_eq(candidate, item)),
                    |item: &Item, other: &Item| item.equals(other),
                    |item: &Item, other: &Item| Rc::ptr_eq(item, other),
                    move |callback: Box<dyn FnOnce()>| schedule_ui.schedule_at_next_animation_frame(0, callback),
                ),
                notifications_toasts_visible_context_key,
                added_toasts_interval_counter: IntervalCounter::new(Self::SPAM_PROTECTION_INTERVAL),
            }
        });
        this.base.register(this.on_did_change_visibility.clone());
        {
            let weak = this.weak_self.clone();
            this.base.register(to_disposable(move || {
                if let Some(this) = weak.upgrade() {
                    this.remove_toasts();
                }
            }));
        }
        {
            let weak = this.weak_self.clone();
            this.base.register(on_did_change_notification_row_height().listen(move |_| {
                if let Some(this) = weak.upgrade() {
                    this.update_notification_heights();
                }
            }));
        }

        this.register_listeners();
        this
    }

    /// `onDidChangeVisibility`.
    pub fn on_did_change_visibility(&self) -> Event<()> {
        self.on_did_change_visibility.event()
    }

    /// `isVisible`.
    pub fn is_visible(&self) -> bool {
        self.is_visible.get()
    }

    fn ui(&self) -> &Ui {
        &self.services.renderer.ui
    }

    /// `getColor(id)`: a cor do tema atual.
    fn get_color(&self, id: &str) -> Option<Rgba> {
        self.services.theme_service.get_color_theme().get_color(id, None).map(|color| je_theme::to_rgba8(&color))
    }

    fn position(&self) -> NotificationsPosition {
        get_notifications_position(&*self.services.renderer.configuration_service)
    }

    fn update_notification_heights(&self) {
        for toast in self.toasts_snapshot() {
            toast.list.update_notification_heights();
        }

        let (_, max_height) = self.compute_max_dimensions();
        if max_height != 0.0 {
            self.layout_container(max_height);
        }
    }

    fn register_listeners(&self) {
        let weak = self.weak_self.clone();

        // Layout
        self.base.register(self.services.layout_service.on_did_layout_main_container().listen({
            let weak = weak.clone();
            move |dimension| {
                if let Some(this) = weak.upgrade() {
                    this.layout(Some(Dimension { width: dimension.width, height: dimension.height }));
                }
            }
        }));

        // Position changes
        self.base.register(self.services.renderer.configuration_service.on_did_change_configuration().listen({
            let weak = weak.clone();
            move |event| {
                if event.affects_configuration(NotificationsSettings::NotificationsPosition.as_str(), None)
                    && let Some(this) = weak.upgrade()
                {
                    this.update_notification_position();
                }
            }
        }));

        // Delay some tasks until after we have restored
        // to reduce UI pressure from the startup phase
        let restored_weak = weak.clone();
        self.services.lifecycle_service.when(LifecyclePhase::Restored).then(move |_| {
            let Some(this) = restored_weak.upgrade() else { return };

            // Show toast for initial notifications if any
            for notification in this.model.notifications() {
                this.add_toast(&notification);
            }

            // Update toasts on notification changes
            let weak = restored_weak.clone();
            this.base.register(this.model.on_did_change_notification().listen(move |event| {
                if let Some(this) = weak.upgrade() {
                    this.on_did_change_notification(event);
                }
            }));
        });

        // Filter
        self.base.register(self.model.on_did_change_filter().listen({
            let weak = weak.clone();
            move |update| {
                let Some(this) = weak.upgrade() else { return };
                if update.global == Some(NotificationsFilter::Error) {
                    this.hide();
                } else if let Some(sources) = &update.sources {
                    for (notification, _) in this.map_notification_to_toast.borrow().clone() {
                        let is_error_source = notification.source_id().is_some_and(|source_id| sources.get(&source_id) == Some(&NotificationsFilter::Error));
                        if is_error_source && notification.severity() != Severity::Error && notification.priority() != NotificationPriority::Urgent {
                            this.remove_toast(&notification);
                        }
                    }
                }
            }
        }));

        // Theme
        self.base.register(self.services.theme_service.on_did_color_theme_change().listen(move |_| {
            if let Some(this) = weak.upgrade() {
                this.update_styles();
            }
        }));
    }

    fn update_notification_position(&self) {
        let Some(toasts_container) = self.notifications_toasts_container.get() else { return };

        let position = self.position();
        {
            let mut tree = self.ui().tree_mut();
            for class in ["bottom-right", "bottom-left", "top-right"] {
                tree.remove_class(toasts_container, class);
            }
            tree.add_class(toasts_container, position.as_str());
        }

        // `updateTopOffset`: o `top` só vale em `top-right`; o deslocamento entra no cálculo de `place`.
        self.place();
    }

    fn top_offset(&self) -> f64 {
        let mut top_offset = EDGE_OFFSET;
        if self.services.layout_service.is_visible(Parts::TitlebarPart, Some(MAIN_WINDOW)) {
            top_offset += f64::from(DEFAULT_CUSTOM_TITLEBAR_HEIGHT);
        }
        top_offset
    }

    fn on_did_change_notification(&self, e: &NotificationChangeEvent) {
        match e.kind {
            NotificationChangeType::Add => self.add_toast(&e.item),
            NotificationChangeType::Remove => self.remove_toast(&e.item),
            _ => {}
        }
    }

    fn add_toast(&self, item: &Item) {
        if self.is_notifications_center_visible.get() {
            return; // do not show toasts while notification center is visible
        }

        if self.services.environment_service.enable_smoke_test_driver() == Some(true) {
            return; // disable in smoke tests to prevent covering elements
        }

        if item.priority() == NotificationPriority::Silent {
            return; // do not show toasts for silenced notifications
        }

        if item.priority() == NotificationPriority::Optional
            && let Some(active_element) = self.ui().focused()
            && self.is_editable_element(active_element)
            && self.is_element_in_notification_quarter(active_element)
        {
            return; // skip showing optional toast that potentially covers input fields
        }

        if self.pending_toasts.try_replace(item) {
            return;
        }

        // Optimization: it is possible that a lot of notifications are being
        // added in a very short time. To prevent this kind of spam, we protect
        // against showing too many notifications at once. Since they can always
        // be accessed from the notification center, a user can always get to
        // them later on.
        // (see also https://github.com/microsoft/vscode/issues/107935)
        if self.added_toasts_interval_counter.increment() > Self::SPAM_PROTECTION_LIMIT {
            return;
        }

        let weak = self.weak_self.clone();
        self.pending_toasts.add(item, move |pending_item, item_disposables| {
            let Some(this) = weak.upgrade() else { return };
            this.map_notification_to_disposable.borrow_mut().push((pending_item.clone(), item_disposables.clone()));
            this.do_add_toast(&pending_item, &item_disposables);
        });
    }

    /// `isEditableElement(activeElement)`: um campo de texto.
    fn is_editable_element(&self, node: NodeId) -> bool {
        self.ui().tree().role(node) == Role::TextInput
    }

    fn is_element_in_notification_quarter(&self, element: NodeId) -> bool {
        let dom_position = self.ui().tree().absolute_bounds(element);
        let client_area = self.services.layout_service.main_container_dimension();

        match self.position() {
            NotificationsPosition::BottomLeft => dom_position.x0 < client_area.width / 2.0 && dom_position.y0 > client_area.height / 2.0,
            NotificationsPosition::TopRight => dom_position.x0 > client_area.width / 2.0 && dom_position.y0 < client_area.height / 2.0,
            NotificationsPosition::BottomRight => dom_position.x0 > client_area.width / 2.0 && dom_position.y0 > client_area.height / 2.0,
        }
    }

    fn do_add_toast(&self, item: &Item, item_disposables: &Rc<DisposableStore>) {
        let ui = self.ui().clone();

        // Lazily create toasts containers
        let notifications_toasts_container = match self.notifications_toasts_container.get() {
            Some(container) => container,
            None => {
                let created = {
                    let mut tree = ui.tree_mut();
                    let created = create_el(&mut tree, self.container, Role::Generic, "notifications-toasts");
                    tree.style_mut(created).visible = false;
                    tree.style_mut(created).radius = TOAST_RADIUS;
                    created
                };
                self.notifications_toasts_container.set(Some(created));
                created
            }
        };

        // Apply position class
        self.update_notification_position();

        // Make Visible
        {
            let mut tree = ui.tree_mut();
            tree.add_class(notifications_toasts_container, "visible");
            tree.style_mut(notifications_toasts_container).visible = true;
        }

        // Container
        let (notification_toast_container, notification_toast) = {
            let mut tree = ui.tree_mut();
            let notification_toast_container = create_detached_el(&mut tree, Role::Generic, "notification-toast-container");
            let first_toast = tree.children(notifications_toasts_container).first().copied();
            if first_toast.is_some() {
                tree.insert_before(notifications_toasts_container, notification_toast_container, first_toast); // always first
            } else {
                tree.append_child(notifications_toasts_container, notification_toast_container);
            }

            // Toast
            let notification_toast = create_el(&mut tree, notification_toast_container, Role::Generic, "notification-toast");
            tree.style_mut(notification_toast).radius = TOAST_RADIUS;
            tree.style_mut(notification_toast).clip = true;
            (notification_toast_container, notification_toast)
        };

        // Create toast with item and show
        let raw = item.message().raw.clone();
        let widget_aria_label = match item.source() {
            None => with_severity_prefix(&localize("notificationAriaLabel", "{0}, notification", &[&raw]), item.severity()),
            Some(source) => with_severity_prefix(&localize("notificationWithSourceAriaLabel", "{0}, source: {1}, notification", &[&raw, &source]), item.severity()),
        };
        let mut list_options = NotificationsListOptions {
            telemetry_surface: Some(NotificationTelemetrySurface::Toast),
            widget_aria_label: Some(widget_aria_label),
            background: self.get_color(NOTIFICATIONS_BACKGROUND),
            on_list_created: Some(self.focus_tracking_hook(item_disposables)),
            ..Default::default()
        };
        list_options.list_options.view.vertical_scroll_mode = ScrollbarVisibility::Hidden;
        let notification_list = NotificationsList::new(notification_toast, list_options, self.services.renderer.clone());
        item_disposables.add(notification_list.clone());

        let toast = Rc::new(NotificationToast { item: item.clone(), list: notification_list.clone(), container: notification_toast_container, toast: notification_toast });
        self.map_notification_to_toast.borrow_mut().push((item.clone(), toast.clone()));

        // When disposed, remove as visible
        {
            let weak = self.weak_self.clone();
            let toast = toast.clone();
            item_disposables.add(to_disposable(move || {
                if let Some(this) = weak.upgrade() {
                    this.update_toast_visibility(&toast, false);
                    let mut tree = this.ui().tree_mut();
                    if tree.is_alive(toast.container) {
                        tree.remove(toast.container);
                    }
                }
            }));
        }

        // Make visible
        notification_list.show();

        // Layout lists
        let (max_width, max_height) = self.compute_max_dimensions();
        self.layout_lists(max_width);

        // Show notification
        notification_list.update_notifications_list(0, 0, vec![item.clone()]);

        // Layout container: only after we show the notification to ensure that
        // the height computation takes the content of it into account!
        self.layout_container(max_height);

        // Re-draw entire item when expansion changes to reveal or hide details
        {
            let (list, item) = (notification_list.clone(), item.clone());
            item_disposables.add(item.clone().on_did_change_expansion().listen(move |_| {
                list.update_notifications_list(0, 1, vec![item.clone()]);
            }));
        }

        // Handle content changes
        // - actions: re-draw to properly show them
        // - message: update notification height unless collapsed
        {
            let (list, item) = (notification_list.clone(), item.clone());
            item_disposables.add(item.clone().on_did_change_content().listen(move |e| match e.kind {
                NotificationViewItemContentChangeKind::Actions => list.update_notifications_list(0, 1, vec![item.clone()]),
                NotificationViewItemContentChangeKind::Message => {
                    if item.expanded() {
                        list.update_notification_height(&item);
                    }
                }
                NotificationViewItemContentChangeKind::Progress => {
                    if e.active_progress_changed == Some(true) {
                        list.update_notifications_list(0, 1, vec![item.clone()]);
                    }
                }
                _ => {}
            }));
        }

        // Remove when item gets closed
        {
            let (weak, item) = (self.weak_self.clone(), item.clone());
            item_disposables.add(item.clone().on_did_close().once().listen(move |_| {
                if let Some(this) = weak.upgrade() {
                    this.remove_toast(&item);
                }
            }));
        }

        // Automatically purge non-sticky notifications
        self.purge_notification(item, notification_toast_container, &notification_list, item_disposables);

        // Theming
        self.update_styles();

        // Context Key
        self.notifications_toasts_visible_context_key.set(Dynamic::from(true));

        // Animate in
        {
            let mut tree = ui.tree_mut();
            tree.add_class(notification_toast, "notification-fade-in");
            tree.remove_class(notification_toast, "notification-fade-in");
            tree.add_class(notification_toast, "notification-fade-in-done");
        }

        // Mark as visible
        item.update_visibility(true);

        // Events
        if !self.is_visible.get() {
            self.is_visible.set(true);
            self.on_did_change_visibility.fire(());
        }
    }

    /// O que a `WorkbenchList` resolve sozinha no original: a chave `NotificationFocusedContext` da lista de toast e a
    /// superfície com o foco.
    fn focus_tracking_hook(&self, disposables: &Rc<DisposableStore>) -> Rc<dyn Fn(&Rc<je_widgets::list::list_widget::List<Item>>)> {
        let focus_key = notification_focused_context().bind_to(&*self.services.context_key_service);
        let (ui, disposables, weak) = (self.ui().clone(), disposables.clone(), self.weak_self.clone());
        Rc::new(move |list| {
            let tracker = FocusTracker::new(&ui, list.get_html_element());
            disposables.add(tracker.clone());
            let key = focus_key.clone();
            disposables.add(tracker.on_did_focus().listen(move |_| {
                key.set(Dynamic::from(true));
                set_focused_notification_surface(Some(NotificationTelemetrySurface::Toast));
            }));
            let (key, weak) = (focus_key.clone(), weak.clone());
            disposables.add(tracker.on_did_blur().listen(move |_| {
                let any_toast_has_focus = weak.upgrade().is_some_and(|this| this.toasts_snapshot().iter().any(|toast| toast.list.has_focus()));
                key.set(Dynamic::from(any_toast_has_focus));
                if !any_toast_has_focus {
                    set_focused_notification_surface(None);
                }
            }));
        })
    }

    fn purge_notification(&self, item: &Item, notification_toast_container: NodeId, notification_list: &Rc<NotificationsList>, disposables: &Rc<DisposableStore>) {
        let state = Rc::new(PurgeState {
            toasts: self.weak_self.clone(),
            item: item.clone(),
            list: notification_list.clone(),
            disposables: disposables.clone(),
            is_mouse_over_toast: Cell::new(false),
            purge_timeout_handle: Cell::new(None),
            listener: RefCell::new(None),
        });

        // Track mouse over item
        let ui = self.ui();
        let over = state.clone();
        disposables.add(ui.listen(notification_toast_container, EventKind::PointerEnter, false, move |_| over.is_mouse_over_toast.set(true)));
        let out = state.clone();
        disposables.add(ui.listen(notification_toast_container, EventKind::PointerLeave, false, move |_| out.is_mouse_over_toast.set(false)));

        // Install Timers to Purge Notification
        Self::hide_after_timeout(&state);

        let cleared = state.clone();
        disposables.add(to_disposable(move || {
            if let Some(handle) = cleared.purge_timeout_handle.get() {
                clear_timeout(handle);
            }
        }));
    }

    fn hide_after_timeout(state: &Rc<PurgeState>) {
        let timer_state = state.clone();
        state.purge_timeout_handle.set(Some(set_timeout(
            move || {
                let Some(this) = timer_state.toasts.upgrade() else { return };

                // If the window does not have focus, we wait for the window to gain focus
                // again before triggering the timeout again. This prevents an issue where
                // focussing the window could immediately hide the notification because the
                // timeout was triggered again.
                if !this.services.host_service.has_focus() {
                    if timer_state.listener.borrow().is_none() {
                        let focus_state = timer_state.clone();
                        let listener = this.services.host_service.on_did_change_focus().listen(move |focus| {
                            if *focus {
                                Self::hide_after_timeout(&focus_state);
                            }
                        });
                        timer_state.disposables.add(listener.clone());
                        *timer_state.listener.borrow_mut() = Some(listener);
                    }
                }
                // Otherwise...
                else if timer_state.item.sticky()                       // never hide sticky notifications
                    || timer_state.list.has_focus()                     // never hide notifications with focus
                    || timer_state.is_mouse_over_toast.get()
                // never hide notifications under mouse
                {
                    Self::hide_after_timeout(&timer_state);
                } else {
                    this.remove_toast(&timer_state.item);
                }
            },
            Self::purge_timeout(state.item.severity()),
        )));
    }

    fn find_toast(&self, item: &Item) -> Option<Rc<NotificationToast>> {
        self.map_notification_to_toast.borrow().iter().find(|(candidate, _)| Rc::ptr_eq(candidate, item)).map(|(_, toast)| toast.clone())
    }

    fn remove_toast(&self, item: &Item) {
        let mut focus_editor = false;

        self.pending_toasts.remove(item);

        // UI
        if let Some(notification_toast) = self.find_toast(item) {
            let toast_has_dom_focus = is_ancestor_of_active_element(self.ui(), notification_toast.container);
            if toast_has_dom_focus {
                focus_editor = !(self.focus_next() || self.focus_previous()); // focus next if any, otherwise focus editor
            }

            self.map_notification_to_toast.borrow_mut().retain(|(candidate, _)| !Rc::ptr_eq(candidate, item));
        }

        // Disposables
        let notification_disposables = {
            let mut disposables = self.map_notification_to_disposable.borrow_mut();
            disposables.iter().position(|(candidate, _)| Rc::ptr_eq(candidate, item)).map(|index| disposables.remove(index).1)
        };
        if let Some(notification_disposables) = notification_disposables {
            notification_disposables.dispose();
        }

        // Layout if we still have toasts
        if !self.map_notification_to_toast.borrow().is_empty() {
            self.layout(self.workbench_dimensions.get().map(|(width, height)| Dimension { width, height }));
        }
        // Otherwise hide if no more toasts to show
        else {
            self.do_hide();

            // Move focus back to editor group as needed
            if focus_editor {
                self.services.editor_groups_service.active_group().focus();
            }
        }
    }

    fn remove_toasts(&self) {
        // Pending
        self.pending_toasts.clear();

        // Toast
        self.map_notification_to_toast.borrow_mut().clear();

        // Disposables
        let disposables = std::mem::take(&mut *self.map_notification_to_disposable.borrow_mut());
        for (_, disposable) in disposables {
            disposable.dispose();
        }

        self.do_hide();
    }

    fn do_hide(&self) {
        if let Some(toasts_container) = self.notifications_toasts_container.get() {
            let mut tree = self.ui().tree_mut();
            tree.remove_class(toasts_container, "visible");
            tree.style_mut(toasts_container).visible = false;
        }

        // Context Key
        self.notifications_toasts_visible_context_key.set(Dynamic::from(false));

        // Events
        if self.is_visible.get() {
            self.is_visible.set(false);
            self.on_did_change_visibility.fire(());
        }
    }

    pub fn hide(&self) {
        let focus_editor = self.notifications_toasts_container.get().is_some_and(|toasts_container| is_ancestor_of_active_element(self.ui(), toasts_container));

        self.remove_toasts();

        if focus_editor {
            self.services.editor_groups_service.active_group().focus();
        }
    }

    pub fn focus(&self) -> bool {
        let toasts = self.get_toasts(ToastVisibility::Visible);
        if let Some(toast) = toasts.first() {
            toast.list.focus_first();

            return true;
        }

        false
    }

    pub fn focus_next(&self) -> bool {
        let toasts = self.get_toasts(ToastVisibility::Visible);
        for (index, toast) in toasts.iter().enumerate() {
            if toast.list.has_focus() {
                if let Some(next_toast) = toasts.get(index + 1) {
                    next_toast.list.focus_first();

                    return true;
                }

                break;
            }
        }

        false
    }

    pub fn focus_previous(&self) -> bool {
        let toasts = self.get_toasts(ToastVisibility::Visible);
        for (index, toast) in toasts.iter().enumerate() {
            if toast.list.has_focus() {
                if let Some(previous_toast) = index.checked_sub(1).and_then(|previous| toasts.get(previous)) {
                    previous_toast.list.focus_first();

                    return true;
                }

                break;
            }
        }

        false
    }

    pub fn focus_first(&self) -> bool {
        if let Some(toast) = self.get_toasts(ToastVisibility::Visible).first() {
            toast.list.focus_first();

            return true;
        }

        false
    }

    pub fn focus_last(&self) -> bool {
        if let Some(toast) = self.get_toasts(ToastVisibility::Visible).last() {
            toast.list.focus_first();

            return true;
        }

        false
    }

    pub fn update(&self, is_center_visible: bool) {
        if self.is_notifications_center_visible.get() != is_center_visible {
            self.is_notifications_center_visible.set(is_center_visible);

            // Hide all toasts when the notificationcenter gets visible
            if is_center_visible {
                self.remove_toasts();
            }
        }
    }

    pub fn update_styles(&self) {
        let background = self.get_color(NOTIFICATIONS_BACKGROUND);
        let border_color = self.get_color(NOTIFICATIONS_TOAST_BORDER);
        {
            let mut tree = self.ui().tree_mut();
            for toast in self.toasts_snapshot() {
                tree.style_mut(toast.toast).background = background;
                tree.style_mut(toast.toast).border = border_color.map(|color| Border { width: 1.0, color });
            }
        }
        self.place();
    }

    fn toasts_snapshot(&self) -> Vec<Rc<NotificationToast>> {
        self.map_notification_to_toast.borrow().iter().map(|(_, toast)| toast.clone()).collect()
    }

    fn get_toasts(&self, state: ToastVisibility) -> Vec<Rc<NotificationToast>> {
        let mut notification_toasts: Vec<Rc<NotificationToast>> = self
            .toasts_snapshot()
            .into_iter()
            .filter(|toast| match state {
                ToastVisibility::HiddenOrVisible => true,
                ToastVisibility::Hidden => !self.is_toast_in_dom(toast),
                ToastVisibility::Visible => self.is_toast_in_dom(toast),
            })
            .collect();

        notification_toasts.reverse(); // from newest to oldest
        notification_toasts
    }

    pub fn layout(&self, dimension: Option<Dimension>) {
        self.workbench_dimensions.set(dimension.map(|dimension| (dimension.width, dimension.height)));

        let (max_width, max_height) = self.compute_max_dimensions();

        // Update position offset
        self.place();

        // Hide toasts that exceed height
        if max_height != 0.0 {
            self.layout_container(max_height);
        }

        // Layout all lists of toasts
        self.layout_lists(max_width);
    }

    /// `computeMaxDimensions`: a largura e a altura (zero sem as dimensões do workbench) disponíveis aos toasts.
    fn compute_max_dimensions(&self) -> (f64, f64) {
        let max_width = Self::MAX_WIDTH;

        let mut available_width = max_width;
        let mut available_height: Option<f64> = None;

        if let Some((width, height)) = self.workbench_dimensions.get() {
            // Make sure notifications are not exceding available width
            available_width = width;
            available_width -= 2.0 * 8.0; // adjust for paddings left and right

            // Make sure notifications are not exceeding available height
            let mut height = height;
            if self.services.layout_service.is_visible(Parts::StatusbarPart, Some(MAIN_WINDOW)) {
                height -= 22.0; // adjust for status bar
            }

            if self.services.layout_service.is_visible(Parts::TitlebarPart, Some(MAIN_WINDOW)) {
                height -= 22.0; // adjust for title bar
            }

            height -= 2.0 * 12.0; // adjust for paddings top and bottom
            available_height = Some(height);
        }

        (max_width.min(available_width), available_height.unwrap_or(0.0))
    }

    fn layout_lists(&self, width: f64) {
        for toast in self.toasts_snapshot() {
            toast.list.layout(width, None);
        }
        self.place();
    }

    fn layout_container(&self, height_to_give: f64) {
        // Allow the full height for 1 toast but adjust for multiple toasts
        // so that a stack of notifications does not exceed all the way up

        let mut single_toast_height_to_give = height_to_give;
        let mut multiple_toasts_height_to_give = (height_to_give * 0.618).round();

        let toasts = self.get_toasts(ToastVisibility::HiddenOrVisible);
        for toast in &toasts {
            // In order to measure the client height, the element cannot have display: none
            self.ui().tree_mut().style_mut(toast.container).opacity = 0.0;
            self.update_toast_visibility(toast, true);
        }

        let toast_heights: Vec<f64> = toasts.iter().map(|toast| self.toast_outer_height(toast)).collect();
        let mut visible_toasts = 0;
        for (toast, toast_height) in toasts.iter().zip(toast_heights) {
            single_toast_height_to_give -= toast_height;
            multiple_toasts_height_to_give -= toast_height;

            let make_visible = if visible_toasts == Self::MAX_NOTIFICATIONS {
                false // never show more than MAX_NOTIFICATIONS
            } else {
                (visible_toasts == 0 && single_toast_height_to_give >= 0.0) || (visible_toasts > 0 && multiple_toasts_height_to_give >= 0.0) // hide toast if available height is too little
            };

            // Hide or show toast based on context
            self.update_toast_visibility(toast, make_visible);
            self.ui().tree_mut().style_mut(toast.container).opacity = 1.0;
            toast.list.set_telemetry_visibility(make_visible);

            if make_visible {
                visible_toasts += 1;
            }
        }

        self.place();
    }

    fn update_toast_visibility(&self, toast: &Rc<NotificationToast>, visible: bool) {
        if !visible {
            toast.list.set_telemetry_visibility(false);
        }
        if self.is_toast_in_dom(toast) == visible {
            return;
        }

        // Update visibility in DOM
        let Some(toasts_container) = self.notifications_toasts_container.get() else { return };
        {
            let mut tree = self.ui().tree_mut();
            if !tree.is_alive(toast.container) {
                return;
            }
            if visible {
                tree.append_child(toasts_container, toast.container);
            } else {
                tree.detach(toast.container);
            }
        }

        // Update visibility in model
        toast.item.update_visibility(visible);
    }

    fn is_toast_in_dom(&self, toast: &NotificationToast) -> bool {
        let tree = self.ui().tree();
        tree.is_alive(toast.container) && tree.parent(toast.container).is_some()
    }

    /// `toast.container.offsetHeight`: a altura da lista, a borda do toast (1px quando o tema define a cor) e as margens
    /// de 4px do `.notification-toast`.
    fn toast_outer_height(&self, toast: &NotificationToast) -> f64 {
        let border = self.toast_border(toast);
        toast.list.display_height() + 2.0 * border + 2.0 * TOAST_MARGIN
    }

    fn toast_border(&self, toast: &NotificationToast) -> f64 {
        let tree = self.ui().tree();
        if !tree.is_alive(toast.toast) {
            return 0.0;
        }
        tree.style(toast.toast).border.map_or(0.0, |border| border.width)
    }

    /// As regras de CSS do `.notifications-toasts` e dos contêineres: posição absoluta no workbench ancorada no canto da
    /// posição configurada, uma coluna de toasts na ordem do DOM (o mais novo primeiro), cada um com a margem de 4px.
    fn place(&self) {
        let Some(toasts_container) = self.notifications_toasts_container.get() else { return };
        let ui = self.ui();

        let children: Vec<NodeId> = ui.tree().children(toasts_container).to_vec();
        let toasts = self.toasts_snapshot();
        let rows: Vec<(NodeId, Rc<NotificationToast>, f64, f64)> = children
            .iter()
            .filter_map(|child| {
                let toast = toasts.iter().find(|toast| toast.container == *child)?.clone();
                let border = self.toast_border(&toast);
                let width = toast.list.container_node().map_or(0.0, |list_container| ui.tree().bounds(list_container).width()) + 2.0 * border + 2.0 * TOAST_MARGIN;
                Some((*child, toast.clone(), width, self.toast_outer_height(&toast)))
            })
            .collect();

        let width = rows.iter().fold(0.0_f64, |widest, row| widest.max(row.2));
        let height: f64 = rows.iter().map(|row| row.3).sum();

        let (workbench_width, workbench_height) = self.workbench_dimensions.get().unwrap_or_else(|| {
            let dimension = self.services.layout_service.main_container_dimension();
            (dimension.width, dimension.height)
        });
        let bottom_offset = if self.services.layout_service.is_visible(Parts::StatusbarPart, Some(MAIN_WINDOW)) { BOTTOM_OFFSET_WITH_STATUS_BAR } else { BOTTOM_OFFSET_WITHOUT_STATUS_BAR };
        let bottom_y = workbench_height - bottom_offset - height;
        let (x, y) = match self.position() {
            NotificationsPosition::BottomRight => (workbench_width - EDGE_OFFSET - width, bottom_y),
            NotificationsPosition::BottomLeft => (EDGE_OFFSET, bottom_y),
            NotificationsPosition::TopRight => (workbench_width - EDGE_OFFSET - width, self.top_offset()),
        };

        let mut tree = ui.tree_mut();
        tree.set_bounds(toasts_container, Rect::new(x, y, x + width, y + height));
        let mut row_top = 0.0;
        for (child, toast, _, row_height) in rows {
            let border = tree.style(toast.toast).border.map_or(0.0, |border| border.width);
            tree.set_bounds(child, Rect::new(0.0, row_top, width, row_top + row_height));
            tree.set_bounds(toast.toast, Rect::new(TOAST_MARGIN, TOAST_MARGIN, width - TOAST_MARGIN, row_height - TOAST_MARGIN));
            if let Some(list_container) = toast.list.container_node() {
                let list_width = tree.bounds(list_container).width();
                tree.set_bounds(list_container, Rect::new(border, border, border + list_width, border + toast.list.display_height()));
            }
            row_top += row_height;
        }
    }
}

impl INotificationsToastController for NotificationsToasts {
    fn focus(&self) {
        NotificationsToasts::focus(self);
    }

    fn focus_next(&self) {
        NotificationsToasts::focus_next(self);
    }

    fn focus_previous(&self) {
        NotificationsToasts::focus_previous(self);
    }

    fn focus_first(&self) {
        NotificationsToasts::focus_first(self);
    }

    fn focus_last(&self) {
        NotificationsToasts::focus_last(self);
    }

    fn hide(&self) {
        NotificationsToasts::hide(self);
    }
}

impl IDisposable for NotificationsToasts {
    fn dispose(&self) {
        self.pending_toasts.dispose();
        self.base.dispose();
    }

    fn constructor_name(&self) -> &'static str {
        "NotificationsToasts"
    }
}
