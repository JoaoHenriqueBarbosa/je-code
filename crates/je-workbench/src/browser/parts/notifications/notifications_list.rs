//! Porte de `src/vs/workbench/browser/parts/notifications/notificationsList.ts`: a [`NotificationsList`] (a lista
//! de notificações do centro e dos toasts, sobre a [`List`] de [`je_widgets`]) e o
//! [`NotificationAccessibilityProvider`].
//!
//! Notas de tradução:
//! - **`WorkbenchList`** é a [`List`] do `je-widgets`: o `IListService`, o tema e as opções de configuração da
//!   `WorkbenchList` não têm porte. O `overrideStyles.listBackground` (`NOTIFICATIONS_BACKGROUND`) é o fundo do
//!   nó da lista, dado em [`NotificationsListOptions::background`]. `NotificationFocusedContext.bindTo(list.contextKeyService)`
//!   fica no gancho [`NotificationsListOptions::on_list_created`], porque a [`List`] não tem chave de contexto.
//! - **Provedor de acessibilidade.** A [`List`] não tem `accessibilityProvider`: o [`NotificationAccessibilityProvider`]
//!   calcula os mesmos textos e o [`NotificationsList`] os aplica ao nó de cada linha (rótulo) e ao nó da lista
//!   (rótulo do widget e papel `dialog`, https://github.com/microsoft/vscode/issues/82728), por um renderer que
//!   embrulha o [`NotificationRenderer`].
//! - **`@IInstantiationService`** são os serviços de [`NotificationRendererServices`]; o `ITelemetryService` e o
//!   `IContextMenuService` saem deles.
//! - **`getWindow(...).document`, `visibilitychange`, `document.hidden`, `isConnected` e o corte pelo viewport**
//!   (`getBoundingClientRect` contra `innerWidth` e `innerHeight`) não existem no `je-ui`: a visibilidade da
//!   telemetria vale pelo trecho que a própria lista mostra (`scrollTop` até `scrollTop + renderHeight`).
//!   `scheduleAtNextAnimationFrame` é [`Ui::schedule_at_next_animation_frame`].
//! - **`this.list.getHTMLElement().style.maxHeight`** é o teto de altura que [`NotificationsList::layout`] guarda e
//!   aplica à altura da lista (o menor entre o conteúdo e o teto).
//! - **`isAncestorOfActiveElement(listContainer)`** é [`is_ancestor_of_active_element`] sobre o contêiner.

use std::cell::{Cell, RefCell};
use std::rc::{Rc, Weak};

use je_base::actions::{ActionRef, IActionRunner};
use je_base::dynamic::Dynamic;
use je_base::lifecycle::{Disposable, DisposableRef, IDisposable, MutableDisposable};
use je_base::nls::localize;
use je_platform::configuration::service::IConfigurationService;
use je_platform::keybinding::keybinding::IKeybindingService;
use je_platform::notification::with_severity_prefix;
use je_ui::{NodeId, Role, Ui};
use je_widgets::context_view_api::{Anchor, IContextMenuDelegate, IContextMenuEvent};
use je_widgets::dom::{FocusTracker, create_el, is_ancestor_of_active_element};
use je_widgets::list::list_types::{IListRenderer, ListElementRenderDetails, TemplateData};
use je_widgets::list::list_widget::{List, ListOptions};
use je_widgets::list::list_view::ListViewOptions;

use crate::browser::parts::notifications::notifications_commands::{NotificationActionRunner, notification_view_item_context};
use crate::browser::parts::notifications::notifications_viewer::{NotificationRenderer, NotificationRendererServices, NotificationTemplateData, NotificationsListDelegate};
use crate::common::notification_telemetry::{NotificationTelemetrySurface, log_notification_expansion, log_notification_shown};
use crate::common::notifications::NotificationViewItem;

type Item = Rc<NotificationViewItem>;

/// `INotificationsListOptions` (que estende `IListOptions<INotificationViewItem>`).
#[derive(Clone)]
pub struct NotificationsListOptions {
    pub list_options: ListOptions<Item>,
    pub widget_aria_label: Option<String>,
    pub telemetry_surface: Option<NotificationTelemetrySurface>,
    /// `overrideStyles: { listBackground: NOTIFICATIONS_BACKGROUND }`: a cor já resolvida pelo tema.
    pub background: Option<je_render::Rgba>,
    /// O que o original faz com a lista recém criada e a `WorkbenchList` resolve sozinha:
    /// `NotificationFocusedContext.bindTo(list.contextKeyService)`.
    pub on_list_created: Option<Rc<dyn Fn(&Rc<List<Item>>)>>,
}

impl Default for NotificationsListOptions {
    fn default() -> Self {
        Self { list_options: ListOptions::default(), widget_aria_label: None, telemetry_surface: None, background: None, on_list_created: None }
    }
}

/// `NotificationsList`.
pub struct NotificationsList {
    base: Rc<Disposable>,
    weak_self: Weak<NotificationsList>,
    container: NodeId,
    options: NotificationsListOptions,
    services: NotificationRendererServices,
    list_container: Cell<Option<NodeId>>,
    list: RefCell<Option<Rc<List<Item>>>>,
    list_delegate: RefCell<Option<Rc<NotificationsListDelegate>>>,
    view_model: RefCell<Vec<Item>>,
    is_visible: Cell<Option<bool>>,
    telemetry_visible: Cell<bool>,
    max_height: Cell<Option<f64>>,
    visibility_update: Rc<MutableDisposable<dyn IDisposable>>,
}

impl NotificationsList {
    /// `new NotificationsList(container, options, instantiationService, contextMenuService, telemetryService)`.
    pub fn new(container: NodeId, options: NotificationsListOptions, services: NotificationRendererServices) -> Rc<Self> {
        let base = Disposable::new();
        let visibility_update = MutableDisposable::<dyn IDisposable>::new();
        base.register(visibility_update.clone());
        Rc::new_cyclic(|weak| Self {
            base,
            weak_self: weak.clone(),
            container,
            options,
            services,
            list_container: Cell::new(None),
            list: RefCell::new(None),
            list_delegate: RefCell::new(None),
            view_model: RefCell::new(Vec::new()),
            is_visible: Cell::new(None),
            telemetry_visible: Cell::new(false),
            max_height: Cell::new(None),
            visibility_update,
        })
    }

    fn ui(&self) -> &Ui {
        &self.services.ui
    }

    fn list(&self) -> Option<Rc<List<Item>>> {
        self.list.borrow().clone()
    }

    pub fn update_notification_heights(&self) {
        let Some(list) = self.list() else { return };

        let focus = list.get_focus();
        let focused_index = focus.first().copied();
        let focus_relative_top = focused_index.and_then(|index| list.get_relative_top(index, 0.0));
        let view_model = self.view_model.borrow().clone();
        list.splice(0, view_model.len(), &view_model);
        if let (Some(index), Some(relative_top)) = (focused_index, focus_relative_top) {
            list.reveal(index, Some(relative_top), 0.0);
        }
        list.set_focus(focus, None);
        self.schedule_visibility_update();
    }

    pub fn show(&self) {
        if self.is_visible.get() == Some(true) {
            return; // already visible
        }

        // Lazily create if showing for the first time
        if self.list().is_none() {
            self.create_notifications_list();
        }

        // Make visible
        self.is_visible.set(Some(true));
    }

    fn create_notifications_list(&self) {
        let ui = self.ui().clone();

        // List Container
        let list_container = {
            let mut tree = ui.tree_mut();
            create_el(&mut tree, self.container, Role::Generic, "notifications-list-container")
        };
        self.list_container.set(Some(list_container));

        let action_runner = NotificationActionRunner::new(self.options.telemetry_surface, self.services.telemetry_service.clone(), self.services.notification_service.clone());
        self.base.register(action_runner.clone());

        // Notification Renderer
        let renderer = NotificationRenderer::with_notification_action_runner(action_runner.clone(), self.services.clone());
        let accessibility_provider = Rc::new(NotificationAccessibilityProvider::new(self.options.widget_aria_label.clone(), self.services.keybinding_service.clone(), self.services.configuration_service.clone()));
        let accessible_renderer: Rc<dyn IListRenderer<Item>> = Rc::new(AccessibleNotificationRenderer { inner: renderer, provider: accessibility_provider.clone(), ui: ui.clone() });

        // List
        let delegate = Rc::new(NotificationsListDelegate::new());
        *self.list_delegate.borrow_mut() = Some(delegate.clone());
        let mut list_options = self.options.list_options.clone();
        list_options.view = ListViewOptions { set_row_line_height: false, support_dynamic_heights: false, ..list_options.view };
        let list = List::new("NotificationsList", &ui, list_container, delegate, vec![accessible_renderer], list_options);
        self.base.register(list.clone());
        *self.list.borrow_mut() = Some(list.clone());

        {
            let mut tree = ui.tree_mut();
            let list_node = list.get_html_element();
            tree.set_label(list_node, accessibility_provider.get_widget_aria_label());
            tree.set_role(list_node, accessibility_provider.get_role());
            if let Some(background) = self.options.background {
                tree.style_mut(list_node).background = Some(background);
            }
        }

        // Context menu to copy message
        let copy_action = self.services.actions.create_copy_message_action();
        {
            let (weak, runner, copy_action) = (self.weak_self.clone(), action_runner.clone() as Rc<dyn IActionRunner>, copy_action);
            self.base.register(list.view.on_context_menu().listen(move |e| {
                let (Some(element), Some(index), Some(this)) = (e.element.clone(), e.index, weak.upgrade()) else { return };
                let Some(list) = this.list() else { return };
                let anchor = match list.view.dom_element(index) {
                    Some(node) => Anchor::Node(node),
                    None => Anchor::Node(list.get_html_element()),
                };
                this.services.dropdown_context_menu.show_context_menu(Rc::new(CopyMessageMenu { anchor, copy_action: copy_action.clone(), context: notification_view_item_context(&element), runner: runner.clone() }));
            }));
        }

        // Toggle on double click
        {
            let weak = self.weak_self.clone();
            self.base.register(list.view.on_mouse_dbl_click().listen(move |event| {
                let (Some(element), Some(this)) = (event.element.clone(), weak.upgrade()) else { return };
                log_notification_expansion(&*this.services.telemetry_service, &element, !element.expanded(), this.options.telemetry_surface);
                element.toggle();
            }));
        }
        {
            let weak = self.weak_self.clone();
            self.base.register(list.view.on_did_scroll().listen(move |_| {
                if let Some(this) = weak.upgrade() {
                    this.schedule_visibility_update();
                }
            }));
        }

        // Clear focus when DOM focus moves out
        // Use document.hasFocus() to not clear the focus when the entire window lost focus
        // This ensures that when the focus comes back, the notification is still focused
        let list_focus_tracker = FocusTracker::new(&ui, list.get_html_element());
        self.base.register(list_focus_tracker.clone());
        {
            let list = Rc::downgrade(&list);
            self.base.register(list_focus_tracker.on_did_blur().listen(move |_| {
                if let Some(list) = list.upgrade() {
                    list.set_focus(Vec::new(), None);
                }
            }));
        }

        // Context key
        if let Some(on_list_created) = &self.options.on_list_created {
            on_list_created(&list);
        }

        // Only allow for focus in notifications, as the
        // selection is too strong over the contents of
        // the notification
        {
            let list_for_selection = Rc::downgrade(&list);
            self.base.register(list.on_did_change_selection().listen(move |e| {
                if !e.indexes.is_empty()
                    && let Some(list) = list_for_selection.upgrade()
                {
                    list.set_selection(Vec::new(), None);
                }
            }));
        }
    }

    pub fn update_notifications_list(&self, start: usize, delete_count: usize, items: Vec<Item>) {
        let (Some(list), Some(list_container)) = (self.list(), self.list_container.get()) else {
            je_base::errors::throw(je_base::errors::Error::new("NotificationsList: a lista ainda não foi criada"))
        };
        let list_has_dom_focus = is_ancestor_of_active_element(self.ui(), list_container);

        // Remember focus and relative top of that item
        let focused_index = list.get_focus().first().copied();
        let focused_item = focused_index.and_then(|index| self.view_model.borrow().get(index).cloned());

        let focus_relative_top = focused_index.and_then(|index| list.get_relative_top(index, 0.0));

        // Update view model
        {
            let mut view_model = self.view_model.borrow_mut();
            let end = (start + delete_count).min(view_model.len());
            view_model.splice(start..end, items.iter().cloned());
        }

        // Update list
        list.splice(start, delete_count, &items);
        self.layout_list(&list);

        // Hide if no more notifications to show
        let length = self.view_model.borrow().len();
        if length == 0 {
            self.hide();
        }
        // Otherwise restore focus if we had
        else if let Some(focused_index) = focused_index {
            let mut index_to_focus = 0;
            if let Some(focused_item) = focused_item {
                let mut candidate = self.view_model.borrow().iter().position(|item| Rc::ptr_eq(item, &focused_item)).map(|i| i as i64).unwrap_or(-1);
                if candidate == -1 {
                    candidate = focused_index as i64 - 1; // item could have been removed
                }

                if candidate < length as i64 && candidate >= 0 {
                    index_to_focus = candidate as usize;
                }
            }

            if let Some(relative_top) = focus_relative_top {
                list.reveal(index_to_focus, Some(relative_top), 0.0);
            }

            list.set_focus(vec![index_to_focus], None);
        }

        // Restore DOM focus if we had focus before
        if self.is_visible.get() == Some(true) && list_has_dom_focus {
            list.dom_focus();
        }
        self.schedule_visibility_update();
    }

    pub fn update_notification_height(&self, item: &Item) {
        let Some(index) = self.view_model.borrow().iter().position(|candidate| Rc::ptr_eq(candidate, item)) else { return };
        let (Some(list), Some(list_delegate)) = (self.list(), self.list_delegate.borrow().clone()) else {
            je_base::errors::throw(je_base::errors::Error::new("NotificationsList: a lista ainda não foi criada"))
        };

        use je_widgets::list::list_types::IListVirtualDelegate;
        list.update_element_height(index, Some(list_delegate.get_height(item)));
        self.layout_list(&list);
        self.schedule_visibility_update();
    }

    /// `list.layout()`: a altura é o conteúdo, limitada pelo teto (`style.maxHeight`) quando há.
    fn layout_list(&self, list: &Rc<List<Item>>) {
        let content_height = list.content_height();
        let height = self.max_height.get().map_or(content_height, |max| content_height.min(max));
        list.layout(Some(height), None);
    }

    /// Called after the owning surface finishes layout, not while toasts are being measured.
    pub fn set_telemetry_visibility(&self, visible: bool) {
        self.telemetry_visible.set(visible);
        if visible {
            self.schedule_visibility_update();
        } else {
            self.visibility_update.clear();
        }
    }

    fn schedule_visibility_update(&self) {
        if !self.telemetry_visible.get() || self.options.telemetry_surface.is_none() || self.visibility_update.value().is_some() {
            return;
        }
        let weak = self.weak_self.clone();
        let handle: DisposableRef = self.ui().schedule_at_next_animation_frame(0, move || {
            if let Some(this) = weak.upgrade() {
                this.visibility_update.clear();
                this.report_visible_notifications();
            }
        });
        self.visibility_update.set(Some(handle));
    }

    fn report_visible_notifications(&self) {
        let list = self.list();
        let surface = self.options.telemetry_surface;
        let (Some(list), Some(surface)) = (list, surface) else { return };
        if !self.telemetry_visible.get() || list.render_height() <= 0.0 {
            return;
        }

        let top = list.scroll_top();
        let bottom = list.scroll_top() + list.render_height();
        if bottom <= top {
            return;
        }
        let first = list.first_visible_index().max(0) as usize;
        for index in first..list.length() {
            let item_top = list.get_element_top(index);
            if item_top >= bottom {
                break;
            }
            if item_top + list.get_element_height(index) > top {
                log_notification_shown(&*self.services.telemetry_service, &list.element(index), surface);
            }
        }
    }

    pub fn hide(&self) {
        self.set_telemetry_visibility(false);
        let Some(list) = self.list() else { return };
        if self.is_visible.get() != Some(true) {
            return; // already hidden
        }

        // Hide
        self.is_visible.set(Some(false));

        // Clear list
        list.splice(0, self.view_model.borrow().len(), &[]);

        // Clear view model
        self.view_model.borrow_mut().clear();
    }

    pub fn focus_first(&self) {
        let Some(list) = self.list() else { return }; // not created yet

        list.focus_first(None, None);
        list.dom_focus();
    }

    pub fn has_focus(&self) -> bool {
        let Some(list_container) = self.list_container.get() else { return false }; // not created yet

        is_ancestor_of_active_element(self.ui(), list_container)
    }

    pub fn layout(&self, width: f64, max_height: Option<f64>) {
        let (Some(list_container), Some(list)) = (self.list_container.get(), self.list()) else { return };

        {
            let mut tree = self.ui().tree_mut();
            let bounds = tree.bounds(list_container);
            tree.set_bounds(list_container, je_render::kurbo::Rect::new(bounds.x0, bounds.y0, bounds.x0 + width, bounds.y1));
        }

        if let Some(max_height) = max_height {
            self.max_height.set(Some(max_height));
        }

        let height = self.max_height.get().map_or(list.content_height(), |max| list.content_height().min(max));
        list.layout(Some(height), Some(width));
        self.schedule_visibility_update();
    }
}

impl IDisposable for NotificationsList {
    fn dispose(&self) {
        self.hide();

        self.base.dispose();
    }

    fn constructor_name(&self) -> &'static str {
        "NotificationsList"
    }
}

/// O menu de contexto da lista: só a ação de copiar, com a notificação como contexto.
struct CopyMessageMenu {
    anchor: Anchor,
    copy_action: ActionRef,
    context: Dynamic,
    runner: Rc<dyn IActionRunner>,
}

impl IContextMenuDelegate for CopyMessageMenu {
    fn get_anchor(&self) -> Anchor {
        self.anchor.clone()
    }

    fn get_actions(&self) -> Vec<ActionRef> {
        vec![self.copy_action.clone()]
    }

    fn get_actions_context(&self, _event: Option<&IContextMenuEvent>) -> Option<Dynamic> {
        Some(self.context.clone())
    }

    fn action_runner(&self) -> Option<Rc<dyn IActionRunner>> {
        Some(self.runner.clone())
    }
}

/// O `IListRenderer` que aplica o rótulo de acessibilidade à linha (o que a `List` do original faz com o
/// `accessibilityProvider.getAriaLabel`).
struct AccessibleNotificationRenderer {
    inner: Rc<NotificationRenderer>,
    provider: Rc<NotificationAccessibilityProvider>,
    ui: Ui,
}

impl IListRenderer<Item> for AccessibleNotificationRenderer {
    fn template_id(&self) -> String {
        self.inner.template_id()
    }

    fn render_template(&self, container: NodeId) -> TemplateData {
        self.inner.render_template(container)
    }

    fn render_element(&self, element: &Item, index: usize, template_data: &TemplateData, details: Option<&ListElementRenderDetails>) {
        self.inner.render_element(element, index, template_data, details);
        let any: Rc<dyn std::any::Any> = template_data.clone();
        if let Ok(template) = any.downcast::<NotificationTemplateData>() {
            self.ui.tree_mut().set_label(template.container, self.provider.get_aria_label(element));
        }
    }

    fn dispose_element(&self, element: &Item, index: i64, template_data: &TemplateData, details: Option<&ListElementRenderDetails>) {
        self.inner.dispose_element(element, index, template_data, details);
    }

    fn dispose_template(&self, template_data: &TemplateData) {
        self.inner.dispose_template(template_data);
    }
}

/// `NotificationAccessibilityProvider`.
pub struct NotificationAccessibilityProvider {
    widget_aria_label: Option<String>,
    keybinding_service: Rc<dyn IKeybindingService>,
    configuration_service: Rc<dyn IConfigurationService>,
}

impl NotificationAccessibilityProvider {
    pub fn new(widget_aria_label: Option<String>, keybinding_service: Rc<dyn IKeybindingService>, configuration_service: Rc<dyn IConfigurationService>) -> Self {
        Self { widget_aria_label, keybinding_service, configuration_service }
    }

    pub fn get_aria_label(&self, element: &NotificationViewItem) -> String {
        let mut accessible_view_hint: Option<String> = None;
        let keybinding = self.keybinding_service.lookup_keybinding("editor.action.accessibleView", None, false).and_then(|keybinding| keybinding.get_aria_label());
        if self.configuration_service.get_value(Some("accessibility.verbosity.notification"), None) == Dynamic::from(true) {
            accessible_view_hint = Some(match &keybinding {
                Some(keybinding) => localize("notificationAccessibleViewHint", "Inspect the response in the accessible view with {0}", &[keybinding]),
                None => localize("notificationAccessibleViewHintNoKb", "Inspect the response in the accessible view via the command Open Accessible View which is currently not triggerable via keybinding", &[]),
            });
        }

        let raw = element.message().raw.clone();
        let Some(source) = element.source() else {
            return with_severity_prefix(
                &match &accessible_view_hint {
                    Some(hint) => localize("notificationAriaLabelHint", "{0}, notification, {1}", &[&raw, hint]),
                    None => localize("notificationAriaLabel", "{0}, notification", &[&raw]),
                },
                element.severity(),
            );
        };

        with_severity_prefix(
            &match &accessible_view_hint {
                Some(hint) => localize("notificationWithSourceAriaLabelHint", "{0}, source: {1}, notification, {2}", &[&raw, &source, hint]),
                None => localize("notificationWithSourceAriaLabel", "{0}, source: {1}, notification", &[&raw, &source]),
            },
            element.severity(),
        )
    }

    pub fn get_widget_aria_label(&self) -> String {
        self.widget_aria_label.clone().unwrap_or_else(|| localize("notificationsList", "Notifications List", &[]))
    }

    pub fn get_role(&self) -> Role {
        Role::Dialog // https://github.com/microsoft/vscode/issues/82728
    }
}
