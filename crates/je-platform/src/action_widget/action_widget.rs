//! Porte de `src/vs/platform/actionWidget/browser/actionWidget.ts`: o `ActionWidgetService` (mostrar e esconder o widget
//! de ações, a lista focada, aceitar, a animação de fechamento e o esconder ao perder o foco), as chaves de contexto
//! `codeActionMenuVisible` e `codeActionMenuFilterFocused` e as ações com os atalhos (esconder, limpar o filtro,
//! anterior, próximo, recolher, expandir, alternar a seção, aceitar e visualizar).
//!
//! Divergências de tradução:
//! - **DOM.** O widget é o [`ViewElement`] `action-widget` (classes, estilo, medida) dentro da visão de contexto. O que o
//!   original faz com ouvintes de DOM entra por métodos: [`ActionWidgetService::dispatch_key_down`] (o `keydown` do
//!   widget e do campo de filtro, com o alvo explícito), [`ActionWidgetService::notify_mouse_down`] (o `mousedown` do
//!   widget, que suspende o esconder-ao-perder-foco por 1 segundo) e [`ActionWidgetService::notify_focus`] e
//!   [`ActionWidgetService::notify_blur`] (o `trackFocus(element)`). O `dom.getActiveElement()` do tratador de `blur` é o
//!   [`ActiveElement`] que o chamador entrega. O `preventDefault` no cabeçalho e o `stopPropagation` no
//!   `context-view-block` são do pintor (a árvore nativa não propaga mouse pelo DOM).
//! - **Injeção.** `IInstantiationService` (que cria o `ActionList` com `IContextViewService`, `ILayoutService`,
//!   `IKeybindingService`, `IOpenerService` e `IHoverService`) é substituído pelas dependências diretas que o recorte do
//!   `ActionList` portado usa: [`IContextViewService`] e [`ILayoutService`] (os demais entram com a renderização e o hover).
//! - **Métodos genéricos.** `show<T>`, `updateItems<T>` e `getFocusedElement<T>` não cabem num `dyn`. O
//!   [`IActionWidgetService`] guarda os métodos sem tipo (`hide`, `isVisible`, `focusItemById`, `setFilter`) e os
//!   genéricos são do [`ActionWidgetService`]; o `instanceof ActionWidgetService` das ações é
//!   [`IActionWidgetService::as_action_widget_service`].
//! - **`accessibilityProvider`.** O `IListAccessibilityProvider` do `show` é da fatia da renderização (`aria-*`).
//! - **Barra de ações.** O `ActionBar` é um widget do `je-widgets`; o serviço cria o contêiner `.action-widget-action-bar`
//!   e lê a largura dele (`actionBar.getContainer().offsetWidth`), que o `ActionBar` nativo define ao se desenhar nele.
//! - **Cor `actionBar.toggledBackground`.** O `registerColor` é do registro de cores do `je-theme`.
//! - **Registro.** Os `registerSingleton` e `registerAction2` do carregamento do módulo são [`register`] e
//!   [`register_action_widget_actions`].

use std::any::Any;
use std::cell::{Cell, RefCell};
use std::rc::{Rc, Weak};

use je_base::actions::ActionRef;
use je_base::async_::disposable_timeout;
use je_base::dynamic::{Dynamic, Truthy};
use je_base::key_codes::{KeyCode, KeyMod};
use je_base::layout::AnchorPosition;
use je_base::lifecycle::{DisposableRef, DisposableStore, IDisposable, MutableDisposable, combined_disposable, to_disposable};
use je_base::promise::Promise;
use je_base::runtime::date_now;
use je_instantiation::{Injectable, InstantiationType, ServicesAccessor, delayed_proxy, register_singleton, service_id, service_impl};

use super::action_list::{
    ACCEPT_SELECTED_ACTION_COMMAND, ACTION_WIDGET_VERTICAL_CHROME_HEIGHT, ActionList, ActionListItemData, FilterInput, IActionListDelegate, IActionListItem,
    IActionListOptions, IActionListUpdateOptions, KeyTarget, PREVIEW_SELECTED_ACTION_COMMAND,
};
use super::action_widget_motion::{ACTION_WIDGET_ANIMATED_CLASS, ACTION_WIDGET_DROPDOWN_MOTION_CLASS, finish_action_widget_opening_animation};
use crate::action::{CommandAction, CommandActionTitle, Title, localize2};
use crate::actions::actions::{Action2, Action2Keybinding, Action2Options, register_action2};
use crate::contextkey::contextkey::{ContextKeyExpr, ContextKeyExpression, IContextKey, IContextKeyService, RawContextKey};
use crate::contextview::context_view::{AnchorTarget, IContextViewCloseAnimation, IContextViewDelegate, IContextViewService, has_required_ancestor_classes};
use crate::contextview::view_element::{UiKeyEvent, ViewElement};
use crate::keybinding::keybindings_registry::{KeybindingWeight, Keybindings, PlatformKeybindings};
use crate::layout::layout_service::ILayoutService;

const ACTION_WIDGET_CLOSE_START_OPACITY_VARIABLE: &str = "--action-widget-close-start-opacity";
const ACTION_WIDGET_CLOSE_START_TRANSFORM_VARIABLE: &str = "--action-widget-close-start-transform";

/// `ActionWidgetContextKeys`.
pub struct ActionWidgetContextKeys;

thread_local! {
    static VISIBLE: RawContextKey = RawContextKey::new(
        "codeActionMenuVisible",
        false,
        je_base::nls::localize("codeActionMenuVisible", "Whether the action widget list is visible", &[]),
    );
    static FILTER_FOCUSED: RawContextKey = RawContextKey::new(
        "codeActionMenuFilterFocused",
        false,
        je_base::nls::localize("codeActionMenuFilterFocused", "Whether the action widget filter input is focused", &[]),
    );
}

impl ActionWidgetContextKeys {
    /// `ActionWidgetContextKeys.Visible`.
    pub fn visible() -> RawContextKey {
        VISIBLE.with(RawContextKey::clone)
    }

    /// `ActionWidgetContextKeys.FilterFocused`.
    pub fn filter_focused() -> RawContextKey {
        FILTER_FOCUSED.with(RawContextKey::clone)
    }
}

/// `IActionWidgetService`: o recorte sem tipo (ver a nota do módulo).
pub trait IActionWidgetService {
    /// O `instanceof ActionWidgetService` das ações: o serviço concreto, também por trás do proxy da instanciação atrasada.
    fn as_action_widget_service(&self) -> Option<Rc<ActionWidgetService>>;

    /// Focuses the item with the given id in the currently shown widget, without rebuilding the list.
    fn focus_item_by_id(&self, item_id: &str);

    fn set_filter(&self, value: &str, focus_item_id: Option<&str>);

    fn hide(&self, did_cancel: Option<bool>);

    fn is_visible(&self) -> bool;
}

delayed_proxy!(pub ActionWidgetServiceProxy for dyn IActionWidgetService {
    methods {
        fn as_action_widget_service(&self) -> Option<Rc<ActionWidgetService>>;
        fn focus_item_by_id(&self, item_id: &str);
        fn set_filter(&self, value: &str, focus_item_id: Option<&str>);
        fn hide(&self, did_cancel: Option<bool>);
        fn is_visible(&self) -> bool;
    }
    events {}
});

service_id!(pub I_ACTION_WIDGET_SERVICE: dyn IActionWidgetService = "actionWidgetService", proxy ActionWidgetServiceProxy);

/// O `ActionList<unknown>` sem o tipo do item: o que o serviço chama nele.
pub trait ActionListHandle: IDisposable {
    fn as_any(&self) -> &dyn Any;
    fn dom_node(&self) -> Rc<ViewElement>;
    fn filter_container(&self) -> Option<Rc<ViewElement>>;
    fn footer_container(&self) -> Option<Rc<ViewElement>>;
    fn header_container(&self) -> Option<Rc<ViewElement>>;
    fn filter_input(&self) -> Option<Rc<FilterInput>>;
    fn close_animation(&self) -> Option<IContextViewCloseAnimation>;
    fn widget_class_name(&self) -> Option<String>;
    fn anchor_position(&self) -> Option<AnchorPosition>;
    fn chrome_height(&self) -> f64;
    fn focus(&self);
    fn hide(&self, did_cancel: Option<bool>, hide_context_view: bool);
    fn set_filter(&self, value: &str, focus_item_id: Option<&str>);
    fn clear_filter(&self) -> bool;
    fn focus_previous(&self);
    fn focus_next(&self);
    fn collapse_focused_section(&self) -> bool;
    fn expand_focused_section(&self) -> bool;
    fn toggle_focused_section(&self) -> bool;
    fn accept_selected(&self, preview: bool);
    fn focus_item_by_id(&self, item_id: &str);
    fn layout(&self, min_width: f64) -> f64;
    fn dispatch_key_down(&self, e: &UiKeyEvent, target: KeyTarget);
    fn filter_input_input(&self, value: &str);
    fn filter_input_composition(&self, start: bool);
    fn visible_labels(&self) -> Vec<Option<String>>;
    fn submenu_rows(&self) -> Option<Vec<Option<String>>>;
    fn run_submenu_toolbar_action(&self, row_label: &str, action_id: &str) -> Option<Promise<()>>;
}

impl<T: ActionListItemData> ActionListHandle for ActionList<T> {
    fn as_any(&self) -> &dyn Any {
        self
    }
    fn dom_node(&self) -> Rc<ViewElement> {
        ActionList::dom_node(self)
    }
    fn filter_container(&self) -> Option<Rc<ViewElement>> {
        ActionList::filter_container(self)
    }
    fn footer_container(&self) -> Option<Rc<ViewElement>> {
        ActionList::footer_container(self)
    }
    fn header_container(&self) -> Option<Rc<ViewElement>> {
        ActionList::header_container(self)
    }
    fn filter_input(&self) -> Option<Rc<FilterInput>> {
        ActionList::filter_input(self)
    }
    fn close_animation(&self) -> Option<IContextViewCloseAnimation> {
        ActionList::close_animation(self)
    }
    fn widget_class_name(&self) -> Option<String> {
        ActionList::widget_class_name(self)
    }
    fn anchor_position(&self) -> Option<AnchorPosition> {
        ActionList::anchor_position(self)
    }
    fn chrome_height(&self) -> f64 {
        ActionList::chrome_height(self)
    }
    fn focus(&self) {
        ActionList::focus(self);
    }
    fn hide(&self, did_cancel: Option<bool>, hide_context_view: bool) {
        ActionList::hide(self, did_cancel, hide_context_view);
    }
    fn set_filter(&self, value: &str, focus_item_id: Option<&str>) {
        ActionList::set_filter(self, value, focus_item_id);
    }
    fn clear_filter(&self) -> bool {
        ActionList::clear_filter(self)
    }
    fn focus_previous(&self) {
        ActionList::focus_previous(self);
    }
    fn focus_next(&self) {
        ActionList::focus_next(self);
    }
    fn collapse_focused_section(&self) -> bool {
        ActionList::collapse_focused_section(self)
    }
    fn expand_focused_section(&self) -> bool {
        ActionList::expand_focused_section(self)
    }
    fn toggle_focused_section(&self) -> bool {
        ActionList::toggle_focused_section(self)
    }
    fn accept_selected(&self, preview: bool) {
        ActionList::accept_selected(self, preview);
    }
    fn focus_item_by_id(&self, item_id: &str) {
        ActionList::focus_item_by_id(self, item_id);
    }
    fn layout(&self, min_width: f64) -> f64 {
        ActionList::layout(self, min_width, None)
    }
    fn dispatch_key_down(&self, e: &UiKeyEvent, target: KeyTarget) {
        self.widget().dispatch_key_down(e, target);
    }
    fn filter_input_input(&self, value: &str) {
        self.widget().filter_input_input(value);
    }
    fn filter_input_composition(&self, start: bool) {
        if start {
            self.widget().filter_input_composition_start();
        } else {
            self.widget().filter_input_composition_end();
        }
    }
    fn visible_labels(&self) -> Vec<Option<String>> {
        self.widget().visible_labels()
    }
    fn submenu_rows(&self) -> Option<Vec<Option<String>>> {
        self.widget().submenu_rows()
    }
    fn run_submenu_toolbar_action(&self, row_label: &str, action_id: &str) -> Option<Promise<()>> {
        self.widget().submenu_widget()?.run_item_toolbar_action(row_label, action_id)
    }
}

/// `dom.getActiveElement()` no momento em que o `blur` é tratado.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ActiveElement {
    /// Nenhum elemento tem o foco.
    None,
    /// Dentro do contêiner do widget (`element.contains(activeElement)`).
    InsideWidget,
    /// Num hover (`.action-widget-hover`) ou no painel de submenu (`.action-list-submenu-panel`) deste widget.
    HoverOrSubmenu,
    /// Fora de tudo isso.
    Elsewhere,
}

/// O estado de um widget renderizado que o `_renderWidget` fecha em volta dos ouvintes.
struct RenderState {
    /// `suppressBlurHideUntil`.
    suppress_blur_hide_until: Cell<u64>,
    pending_blur_hide: Rc<MutableDisposable<dyn IDisposable>>,
    filter_as_combobox: bool,
    action_bar_actions: Vec<ActionRef>,
    action_bar: Option<Rc<ViewElement>>,
}

/// `ActionWidgetService`.
pub struct ActionWidgetService {
    this: Weak<ActionWidgetService>,
    context_view_service: Rc<dyn IContextViewService>,
    context_key_service: Rc<dyn IContextKeyService>,
    layout_service: Rc<dyn ILayoutService>,
    list: Rc<MutableDisposable<dyn ActionListHandle>>,
    close_animation: Rc<MutableDisposable<dyn IDisposable>>,
    widget_element: RefCell<Option<Rc<ViewElement>>>,
    closing_list: RefCell<Option<Rc<dyn ActionListHandle>>>,
    render_state: RefCell<Option<Rc<RenderState>>>,
}

impl Injectable for ActionWidgetService {
    type Args = ();
    type Deps = (Rc<dyn IContextViewService>, Rc<dyn IContextKeyService>, Rc<dyn ILayoutService>);

    fn create(_: (), (context_view_service, context_key_service, layout_service): Self::Deps) -> Rc<Self> {
        Rc::new_cyclic(|this| Self {
            this: this.clone(),
            context_view_service,
            context_key_service,
            layout_service,
            list: MutableDisposable::new(),
            close_animation: MutableDisposable::new(),
            widget_element: RefCell::new(None),
            closing_list: RefCell::new(None),
            render_state: RefCell::new(None),
        })
    }
}

service_impl!(ActionWidgetService => dyn IActionWidgetService, disposable);

/// `registerSingleton(IActionWidgetService, ActionWidgetService, InstantiationType.Delayed)`.
pub fn register() {
    register_singleton::<ActionWidgetService>(I_ACTION_WIDGET_SERVICE, InstantiationType::Delayed);
}

impl IDisposable for ActionWidgetService {
    fn dispose(&self) {
        self.list.dispose();
        self.close_animation.dispose();
        *self.render_state.borrow_mut() = None;
        *self.closing_list.borrow_mut() = None;
        *self.widget_element.borrow_mut() = None;
    }
}

/// O delegate da visão de contexto de um `show`.
struct ActionWidgetViewDelegate {
    service: Weak<ActionWidgetService>,
    list: Rc<dyn ActionListHandle>,
    anchor: AnchorTarget,
    visible_context: Rc<dyn IContextKey>,
    action_bar_actions: Vec<ActionRef>,
    list_options: Option<Rc<IActionListOptions>>,
    layer: Option<i32>,
}

impl IContextViewDelegate for ActionWidgetViewDelegate {
    fn get_anchor(&self) -> AnchorTarget {
        self.anchor.clone()
    }

    fn render(&self, container: &Rc<ViewElement>) -> Option<DisposableRef> {
        self.visible_context.set(Dynamic::Bool(true));
        let service = self.service.upgrade()?;
        Some(service.render_widget(container, &self.list, &self.action_bar_actions, self.list_options.as_deref()))
    }

    fn on_hide(&self, did_cancel: Option<bool>) {
        self.visible_context.reset();
        if let Some(service) = self.service.upgrade() {
            service.on_widget_closed(did_cancel);
        }
    }

    fn anchor_position(&self) -> Option<AnchorPosition> {
        self.list.anchor_position()
    }

    fn focus(&self) {
        self.list.focus();
    }

    fn layer(&self) -> Option<i32> {
        self.layer
    }
}

impl ActionWidgetService {
    /// `isVisible`.
    pub fn is_visible(&self) -> bool {
        ActionWidgetContextKeys::visible().get_value(self.context_key_service.as_ref()).is_truthy()
    }

    /// `show(user, supportsPreview, items, delegate, anchor, container, actionBarActions, accessibilityProvider,
    /// listOptions, contextViewLayer)`.
    #[allow(clippy::too_many_arguments)]
    pub fn show<T: ActionListItemData>(
        &self,
        _user: &str,
        supports_preview: bool,
        items: &[Rc<IActionListItem<T>>],
        delegate: Rc<dyn IActionListDelegate<T>>,
        anchor: AnchorTarget,
        container: Option<Rc<ViewElement>>,
        action_bar_actions: Option<Vec<ActionRef>>,
        list_options: Option<Rc<IActionListOptions>>,
        context_view_layer: Option<i32>,
    ) {
        let visible_context = ActionWidgetContextKeys::visible().bind_to(self.context_key_service.as_ref());
        let list = ActionList::new(
            supports_preview,
            items,
            delegate,
            list_options.clone(),
            anchor.clone(),
            self.context_view_service.clone(),
            self.layout_service.clone(),
        );
        let list: Rc<dyn ActionListHandle> = list;
        self.context_view_service.show_context_view(
            Rc::new(ActionWidgetViewDelegate {
                service: self.this.clone(),
                list,
                anchor,
                visible_context,
                action_bar_actions: action_bar_actions.unwrap_or_default(),
                list_options,
                layer: context_view_layer,
            }),
            container,
            false,
        );
    }

    /// `acceptSelected(preview)`.
    pub fn accept_selected(&self, preview: bool) {
        if let Some(list) = self.list.value() {
            list.accept_selected(preview);
        }
    }

    /// `updateItems(items, focusItemId, options)`: replaces the items of the currently shown widget in place, without
    /// closing or repositioning it. Preserves the current filter. When `focusItemId` is provided, focuses that item;
    /// otherwise preserves the focused item.
    pub fn update_items<T: ActionListItemData>(&self, items: &[Rc<IActionListItem<T>>], focus_item_id: Option<&str>, options: Option<IActionListUpdateOptions>) {
        let widget = self.widget_element.borrow().clone();
        if let Some(widget) = widget {
            finish_action_widget_opening_animation(&widget);
        }
        if let Some(list) = self.list.value() {
            if let Some(list) = list.as_any().downcast_ref::<ActionList<T>>() {
                list.update_items(items, focus_item_id, options);
            }
        }
    }

    /// `getFocusedElement()`: the item currently focused in the shown widget, if any.
    pub fn get_focused_element<T: ActionListItemData>(&self) -> Option<Rc<IActionListItem<T>>> {
        let list = self.list.value()?;
        list.as_any().downcast_ref::<ActionList<T>>()?.get_focused_element()
    }

    /// `focusPrevious()`.
    pub fn focus_previous(&self) {
        if let Some(list) = self.list.value() {
            list.focus_previous();
        }
    }

    /// `focusNext()`.
    pub fn focus_next(&self) {
        if let Some(list) = self.list.value() {
            list.focus_next();
        }
    }

    /// `collapseSection()`.
    pub fn collapse_section(&self) {
        if let Some(list) = self.list.value() {
            list.collapse_focused_section();
        }
    }

    /// `expandSection()`.
    pub fn expand_section(&self) {
        if let Some(list) = self.list.value() {
            list.expand_focused_section();
        }
    }

    /// `toggleSection()`.
    pub fn toggle_section(&self) -> bool {
        self.list.value().is_some_and(|list| list.toggle_focused_section())
    }

    /// `clearFilter()`.
    pub fn clear_filter(&self) -> bool {
        self.list.value().is_some_and(|list| list.clear_filter())
    }

    /// `document.activeElement === input`: o campo de filtro da lista mostrada tem o foco.
    pub fn is_filter_input_focused(&self) -> bool {
        self.list.value().and_then(|list| list.filter_input()).is_some_and(|input| input.is_focused())
    }

    /// O `input` do usuário no campo de filtro (`input.value = value; input.dispatchEvent(new Event('input'))`).
    pub fn dispatch_filter_input(&self, value: &str) {
        if let Some(list) = self.list.value() {
            list.filter_input_input(value);
        }
    }

    /// Os eventos `compositionstart` (`start`) e `compositionend` do campo de filtro.
    pub fn dispatch_filter_composition(&self, start: bool) {
        if let Some(list) = self.list.value() {
            list.filter_input_composition(start);
        }
    }

    /// Os rótulos das linhas visíveis da lista mostrada.
    pub fn visible_labels(&self) -> Vec<Option<String>> {
        self.list.value().map(|list| list.visible_labels()).unwrap_or_default()
    }

    /// Os rótulos das linhas do painel de submenu aberto (`.action-list-submenu-panel .monaco-list-row.action`), ou `None`
    /// se não há painel.
    pub fn submenu_rows(&self) -> Option<Vec<Option<String>>> {
        self.list.value()?.submenu_rows()
    }

    /// O clique no botão da barra da linha `row_label` do submenu aberto. Devolve a promessa da ação, ou `None` se a linha
    /// ou a ação não existem.
    pub fn click_submenu_toolbar_action(&self, row_label: &str, action_id: &str) -> Option<Promise<()>> {
        self.list.value()?.run_submenu_toolbar_action(row_label, action_id)
    }

    /// `hide(didCancel)`.
    pub fn hide(&self, did_cancel: Option<bool>) {
        let list = self.list.value();
        let widget = self.widget_element.borrow().clone();
        let Some(list) = list else {
            return;
        };
        if self.closing_list.borrow().as_ref().is_some_and(|closing| Rc::ptr_eq(closing, &list)) {
            return;
        }

        let close_animation = list.close_animation();
        let animation = match (&widget, close_animation) {
            (Some(widget), Some(close_animation))
                if close_animation.duration > 0.0 && has_required_ancestor_classes(widget, &close_animation.required_ancestor_classes) =>
            {
                Some((widget.clone(), close_animation))
            }
            _ => None,
        };
        let Some((widget, close_animation)) = animation else {
            *self.closing_list.borrow_mut() = Some(list.clone());
            list.hide(did_cancel, true);
            return;
        };

        *self.closing_list.borrow_mut() = Some(list.clone());
        let opacity = widget.style_get_property("opacity").unwrap_or_else(|| "1".to_owned());
        let transform = widget.style_get_property("transform").unwrap_or_else(|| "none".to_owned());
        widget.style_set_property(ACTION_WIDGET_CLOSE_START_OPACITY_VARIABLE, &opacity);
        widget.style_set_property(ACTION_WIDGET_CLOSE_START_TRANSFORM_VARIABLE, &transform);
        widget.class_list_remove(ACTION_WIDGET_ANIMATED_CLASS);
        widget.class_list_add(&close_animation.class_name);
        list.hide(did_cancel, false);
        let weak = self.this.clone();
        let hidden_list = list.clone();
        self.close_animation.set(Some(disposable_timeout(
            move || {
                let Some(service) = weak.upgrade() else { return };
                let still_current = service.list.value().is_some_and(|current| Rc::ptr_eq(&current, &hidden_list));
                if still_current {
                    service.context_view_service.hide_context_view(did_cancel);
                }
            },
            close_animation.duration as u64,
            None,
        )));
    }

    /// `clear()`.
    pub fn clear(&self) {
        self.close_animation.clear();
        *self.closing_list.borrow_mut() = None;
        *self.render_state.borrow_mut() = None;
        if let Some(widget) = self.widget_element.borrow_mut().take() {
            widget.style_remove_property(ACTION_WIDGET_CLOSE_START_OPACITY_VARIABLE);
            widget.style_remove_property(ACTION_WIDGET_CLOSE_START_TRANSFORM_VARIABLE);
        }
        self.list.clear();
    }

    /// O elemento `.action-widget` do widget mostrado.
    pub fn widget_element(&self) -> Option<Rc<ViewElement>> {
        self.widget_element.borrow().clone()
    }

    /// A barra de ações do widget mostrado (o contêiner `.action-widget-action-bar`) e as ações dela.
    pub fn action_bar(&self) -> Option<(Rc<ViewElement>, Vec<ActionRef>)> {
        let state = self.render_state.borrow().clone()?;
        Some((state.action_bar.clone()?, state.action_bar_actions.clone()))
    }

    fn render_widget(
        &self,
        element: &Rc<ViewElement>,
        list: &Rc<dyn ActionListHandle>,
        action_bar_actions: &[ActionRef],
        list_options: Option<&IActionListOptions>,
    ) -> DisposableRef {
        let widget = ViewElement::with_classes(&["action-widget"]);
        let widget_class_names: Vec<String> =
            list.widget_class_name().map(|names| names.split_whitespace().map(str::to_owned).collect()).unwrap_or_default();
        for class_name in widget_class_names.iter().filter(|class_name| class_name.as_str() != ACTION_WIDGET_DROPDOWN_MOTION_CLASS) {
            widget.class_list_add(class_name);
        }
        widget.set_parent(Some(element.clone()));
        *self.widget_element.borrow_mut() = Some(widget.clone());

        self.list.set(Some(list.clone()));
        for child in [list.header_container(), list.filter_container(), Some(list.dom_node()), list.footer_container()].into_iter().flatten() {
            child.set_parent(Some(widget.clone()));
        }

        let render_disposables = DisposableStore::new();
        let filter_as_combobox = list_options.is_some_and(|options| options.filter_as_combobox);

        // Action bar
        let mut action_bar_width = 0.0;
        let mut action_bar = None;
        if !action_bar_actions.is_empty() {
            let container = ViewElement::with_classes(&["action-widget-action-bar"]);
            container.set_parent(Some(widget.clone()));
            action_bar_width = container.size().width;
            action_bar = Some(container);
        }

        let blur_state = Rc::new(RenderState {
            suppress_blur_hide_until: Cell::new(0),
            pending_blur_hide: MutableDisposable::new(),
            filter_as_combobox,
            action_bar_actions: action_bar_actions.to_vec(),
            action_bar: action_bar.clone(),
        });
        *self.render_state.borrow_mut() = Some(blur_state.clone());
        render_disposables.add(blur_state.pending_blur_hide.clone());

        let width = list.layout(action_bar_width);
        let widget_width = Rc::new(Cell::new(width));
        widget.style_set_property("width", &format!("{width}px"));

        // O refluxo: o widget mede o que contém e a visão de contexto mede o widget.
        let measured_list = Rc::downgrade(list);
        let measured_widget_width = widget_width.clone();
        let measured_action_bar = action_bar;
        widget.set_measure(Rc::new(move || {
            let (list_height, chrome_height) = measured_list.upgrade().map_or((0.0, 0.0), |list| (list.dom_node().size().height, list.chrome_height()));
            let action_bar_height = measured_action_bar.as_ref().map_or(0.0, |bar| bar.size().height);
            je_base::layout::ISize {
                width: measured_widget_width.get(),
                height: list_height + chrome_height + action_bar_height + ACTION_WIDGET_VERTICAL_CHROME_HEIGHT,
            }
        }));
        let measured_widget = Rc::downgrade(&widget);
        element.set_measure(Rc::new(move || {
            measured_widget.upgrade().map_or(je_base::layout::ISize { width: 0.0, height: 0.0 }, |widget| widget.size())
        }));
        let (view, widget_for_cleanup) = (element.clone(), widget.clone());
        render_disposables.add(to_disposable(move || {
            view.clear_measure();
            widget_for_cleanup.clear_measure();
        }));

        // Track filter input focus state
        let filter_focused_context = ActionWidgetContextKeys::filter_focused().bind_to(self.context_key_service.as_ref());
        let key_to_reset = filter_focused_context.clone();
        render_disposables.add(to_disposable(move || key_to_reset.reset()));
        if let Some(filter_input) = list.filter_input() {
            let key = filter_focused_context.clone();
            render_disposables.add(filter_input.on_did_focus().listen(move |_| key.set(Dynamic::Bool(true))));
            let key = filter_focused_context;
            render_disposables.add(filter_input.on_did_blur().listen(move |_| key.set(Dynamic::Bool(false))));
        }

        // Measure the popup before applying its entrance transform.
        for class_name in &widget_class_names {
            widget.class_list_add(class_name);
        }
        widget.class_list_add(ACTION_WIDGET_ANIMATED_CLASS);
        render_disposables
    }

    /// `_onWidgetClosed(didCancel)`.
    fn on_widget_closed(&self, did_cancel: Option<bool>) {
        let current = self.list.value();
        let closing_current = match (&*self.closing_list.borrow(), &current) {
            (Some(closing), Some(current)) => Rc::ptr_eq(closing, current),
            (None, None) => true,
            _ => false,
        };
        if closing_current {
            self.clear();
            return;
        }
        self.close_animation.clear();
        *self.closing_list.borrow_mut() = None;
        *self.widget_element.borrow_mut() = None;
        if let Some(list) = self.list.value() {
            list.hide(did_cancel, true);
        }
    }

    /// O `keydown` do widget e do que está dentro dele. A lista trata primeiro (campo de filtro, depois `domNode`); se o
    /// evento sobe, o widget de um filtro combobox esconde com Escape (fora de uma composição de IME).
    pub fn dispatch_key_down(&self, e: &UiKeyEvent, target: KeyTarget) {
        let Some(list) = self.list.value() else {
            return;
        };
        list.dispatch_key_down(e, target);
        if e.propagation_stopped() {
            return;
        }
        let combobox = self.render_state.borrow().as_ref().is_some_and(|state| state.filter_as_combobox);
        if combobox && e.key_code == KeyCode::Escape && !e.is_composing {
            e.stop();
            self.hide(Some(true));
        }
    }

    /// O `mousedown` no widget: o foco que sair dele por 1 segundo não o esconde.
    pub fn notify_mouse_down(&self) {
        if let Some(state) = self.render_state.borrow().as_ref() {
            state.suppress_blur_hide_until.set(date_now() + 1000);
        }
    }

    /// O `onDidFocus` do `trackFocus(element)`: cancela o esconder pendente.
    pub fn notify_focus(&self) {
        if let Some(state) = self.render_state.borrow().as_ref() {
            state.pending_blur_hide.clear();
        }
    }

    /// O `onDidBlur` do `trackFocus(element)`: esconde se, passados 75 ms, o foco não estiver no widget nem num hover ou
    /// submenu dele. `active_element` lê o elemento em foco no momento em que o temporizador dispara.
    pub fn notify_blur(&self, active_element: Rc<dyn Fn() -> ActiveElement>) {
        let Some(state) = self.render_state.borrow().clone() else {
            return;
        };
        self.schedule_blur_hide(state, active_element, 75);
    }

    fn schedule_blur_hide(&self, state: Rc<RenderState>, active_element: Rc<dyn Fn() -> ActiveElement>, delay: u64) {
        let weak = self.this.clone();
        let scheduled_state = state.clone();
        state.pending_blur_hide.set(Some(disposable_timeout(
            move || {
                let Some(service) = weak.upgrade() else { return };
                let remaining_suppression = scheduled_state.suppress_blur_hide_until.get() as i64 - date_now() as i64;
                if remaining_suppression > 0 {
                    service.schedule_blur_hide(scheduled_state.clone(), active_element.clone(), remaining_suppression as u64);
                    return;
                }
                // Don't hide if focus moved to a hover or submenu that belongs to this action widget
                if matches!(active_element(), ActiveElement::InsideWidget | ActiveElement::HoverOrSubmenu) {
                    return;
                }
                service.hide(Some(true));
            },
            delay,
            None,
        )));
    }
}

impl IActionWidgetService for ActionWidgetService {
    fn as_action_widget_service(&self) -> Option<Rc<ActionWidgetService>> {
        self.this.upgrade()
    }

    fn focus_item_by_id(&self, item_id: &str) {
        if let Some(list) = self.list.value() {
            list.focus_item_by_id(item_id);
        }
    }

    fn set_filter(&self, value: &str, focus_item_id: Option<&str>) {
        if let Some(list) = self.list.value() {
            list.set_filter(value, focus_item_id);
        }
    }

    fn hide(&self, did_cancel: Option<bool>) {
        ActionWidgetService::hide(self, did_cancel);
    }

    fn is_visible(&self) -> bool {
        ActionWidgetService::is_visible(self)
    }
}

// #region ações

/// Uma das ações `registerAction2(class extends Action2 { ... })` do arquivo: o id, o título, a pré-condição, o atalho e
/// o corpo do `run`, que recebe o serviço.
struct ActionWidgetCommand {
    id: &'static str,
    title_key: &'static str,
    title: &'static str,
    precondition: Option<ContextKeyExpression>,
    keybindings: Keybindings,
    weight: i64,
    run: fn(&Rc<dyn IActionWidgetService>),
}

impl Action2 for ActionWidgetCommand {
    fn desc(&self) -> Action2Options {
        let localized = localize2(self.title_key, self.title);
        Action2Options {
            command: CommandAction {
                precondition: self.precondition.clone(),
                ..CommandAction::new(self.id, Title::Localized(CommandActionTitle { value: localized.value, original: localized.original, mnemonic_title: None }))
            },
            menu: Vec::new(),
            keybinding: vec![Action2Keybinding { keybindings: self.keybindings.clone(), weight: self.weight, args: Dynamic::Undefined, when: None }],
            f1: false,
        }
    }

    fn run(&self, accessor: &ServicesAccessor, _args: &[Dynamic]) {
        (self.run)(&accessor.get(I_ACTION_WIDGET_SERVICE));
    }
}

/// `if (widgetService instanceof ActionWidgetService) { ... }`.
fn with_concrete_service(service: &Rc<dyn IActionWidgetService>, body: impl FnOnce(&ActionWidgetService)) {
    if let Some(concrete) = service.as_action_widget_service() {
        body(&concrete);
    }
}

fn keybindings(primary: u32, secondary: &[u32], mac: Option<(u32, Vec<u32>)>) -> Keybindings {
    Keybindings {
        primary: Some(primary),
        secondary: if secondary.is_empty() { None } else { Some(secondary.to_vec()) },
        mac: mac.map(|(primary, secondary)| PlatformKeybindings { primary: Some(primary), secondary: Some(secondary) }),
        ..Keybindings::default()
    }
}

/// `registerAction2` das oito ações do arquivo. Devolve o que as desregistra.
pub fn register_action_widget_actions() -> DisposableRef {
    let weight = KeybindingWeight::EDITOR_CONTRIB + 1000;
    let visible = ActionWidgetContextKeys::visible();
    let filter_focused = ActionWidgetContextKeys::filter_focused();
    let visible_expr = Some(visible.expr());
    let visible_and_filter_not_focused = ContextKeyExpr::and([Some(visible.expr()), Some(filter_focused.to_negated())]);
    let escape = KeyCode::Escape.bits();
    let up = KeyCode::UpArrow.bits();
    let down = KeyCode::DownArrow.bits();

    let commands = vec![
        ActionWidgetCommand {
            id: "hideCodeActionWidget",
            title_key: "hideCodeActionWidget.title",
            title: "Hide action widget",
            precondition: visible_expr.clone(),
            keybindings: keybindings(escape, &[KeyMod::SHIFT | escape], None),
            weight,
            run: |service| service.hide(Some(true)),
        },
        ActionWidgetCommand {
            id: "clearFilterCodeActionWidget",
            title_key: "clearFilterCodeActionWidget.title",
            title: "Clear action widget filter",
            precondition: ContextKeyExpr::and([Some(visible.expr()), Some(filter_focused.expr())]),
            keybindings: keybindings(escape, &[], None),
            weight: weight + 1,
            run: |service| {
                with_concrete_service(service, |widget_service| {
                    if !widget_service.clear_filter() {
                        widget_service.hide(Some(true));
                    }
                });
            },
        },
        ActionWidgetCommand {
            id: "selectPrevCodeAction",
            title_key: "selectPrevCodeAction.title",
            title: "Select previous action",
            precondition: visible_expr.clone(),
            keybindings: keybindings(up, &[KeyMod::CTRL_CMD | up], Some((up, vec![KeyMod::CTRL_CMD | up, KeyMod::WIN_CTRL | KeyCode::KeyP.bits()]))),
            weight,
            run: |service| with_concrete_service(service, ActionWidgetService::focus_previous),
        },
        ActionWidgetCommand {
            id: "selectNextCodeAction",
            title_key: "selectNextCodeAction.title",
            title: "Select next action",
            precondition: visible_expr.clone(),
            keybindings: keybindings(down, &[KeyMod::CTRL_CMD | down], Some((down, vec![KeyMod::CTRL_CMD | down, KeyMod::WIN_CTRL | KeyCode::KeyN.bits()]))),
            weight,
            run: |service| with_concrete_service(service, ActionWidgetService::focus_next),
        },
        ActionWidgetCommand {
            id: "collapseSectionCodeAction",
            title_key: "collapseSectionCodeAction.title",
            title: "Collapse section",
            precondition: visible_and_filter_not_focused.clone(),
            keybindings: keybindings(KeyCode::LeftArrow.bits(), &[], None),
            weight,
            run: |service| with_concrete_service(service, ActionWidgetService::collapse_section),
        },
        ActionWidgetCommand {
            id: "expandSectionCodeAction",
            title_key: "expandSectionCodeAction.title",
            title: "Expand section",
            precondition: visible_and_filter_not_focused.clone(),
            keybindings: keybindings(KeyCode::RightArrow.bits(), &[], None),
            weight,
            run: |service| with_concrete_service(service, ActionWidgetService::expand_section),
        },
        ActionWidgetCommand {
            id: "toggleSectionCodeAction",
            title_key: "toggleSectionCodeAction.title",
            title: "Toggle section",
            precondition: visible_and_filter_not_focused,
            keybindings: keybindings(KeyCode::Space.bits(), &[], None),
            weight,
            run: |service| {
                with_concrete_service(service, |widget_service| {
                    if !widget_service.toggle_section() {
                        widget_service.accept_selected(false);
                    }
                });
            },
        },
        ActionWidgetCommand {
            id: ACCEPT_SELECTED_ACTION_COMMAND,
            title_key: "acceptSelected.title",
            title: "Accept selected action",
            precondition: visible_expr.clone(),
            keybindings: keybindings(KeyCode::Enter.bits(), &[KeyMod::CTRL_CMD | KeyCode::Period.bits()], None),
            weight,
            run: |service| with_concrete_service(service, |widget_service| widget_service.accept_selected(false)),
        },
        ActionWidgetCommand {
            id: PREVIEW_SELECTED_ACTION_COMMAND,
            title_key: "previewSelected.title",
            title: "Preview selected action",
            precondition: visible_expr,
            keybindings: keybindings(KeyMod::CTRL_CMD | KeyCode::Enter.bits(), &[], None),
            weight,
            run: |service| with_concrete_service(service, |widget_service| widget_service.accept_selected(true)),
        },
    ];
    combined_disposable(commands.into_iter().map(|command| register_action2(Rc::new(command))).collect())
}

// #endregion
