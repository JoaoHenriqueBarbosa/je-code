//! Porte de `src/vs/base/browser/ui/dropdown/dropdownActionViewItem.ts`: o [`DropdownMenuActionViewItem`], um
//! item de ação que abre um menu (o botão "mais" de uma barra de ferramentas), e o
//! [`ActionWithDropdownActionViewItem`], o botão dividido (a ação e, ao lado, a seta do menu).
//!
//! # API para quem consome (toolbar, workbench)
//!
//! - [`DropdownMenuActionViewItem::new`]`(ui, action, menu, context_menu_provider, options)`: `menu` é uma lista
//!   de ações ou um [`IActionProvider`] ([`MenuActions`], o `readonly IAction[] | IActionProvider` do original).
//!   Os eventos de visibilidade saem em [`DropdownMenuActionViewItem::on_did_change_visibility`] e
//!   [`DropdownMenuActionViewItem::show`] abre o menu.
//! - [`ActionWithDropdownActionViewItem::new`]`(ui, context, action, options, context_menu_provider)` herda do
//!   [`ActionViewItem`] por composição: guarda um e o consulta nos ganchos que o original não sobrescreve.
//!
//! Como o DOM e o CSS viram [`je_ui`]: `li.action-item > div.monaco-dropdown > div.dropdown-label >
//! a.action-label` (as classes de [`IDropdownMenuActionViewItemOptions::class_names`] mais `codicon` se não há
//! `icon`). O `a` guarda o codicon da classe `codicon-*` como [`Content::Codicon`] (16px com `padding: 3px`, 22px
//! de lado, como o `.action-label` do `actionbar.css`), e as regras de `:hover`, `:active`, `:focus` e
//! `.disabled` viram [`DropdownMenuActionViewItem::refresh_style`], com as mesmas cores do
//! [`ActionViewItem`](crate::action_view_items::ActionViewItem) (função compartilhada). O `BaseActionViewItem` do
//! original não recebe `render` aqui: o `element` do item é o `a` ([`BaseActionViewItem::set_element`]), como no
//! original, e é o item que tira o `li` da árvore quando a barra o descarta.
//!
//! O botão dividido é um `li` `display: flex` (`.action-dropdown-item`): o rótulo da ação, o separador
//! (`div.action-dropdown-item-separator`, 1px de largura e altura zero, não pinta) e o botão do menu, na ordem,
//! centrados na vertical.
//!
//! Fica de fora: `aria-expanded`, `aria-haspopup` e `aria-disabled` (o `je-ui` só tem `role` e `label`; o `role`
//! é `button` e o rótulo é o da ação) e o toque.

use std::cell::{Cell, RefCell};
use std::rc::{Rc, Weak};

use je_base::actions::{Action, ActionRef, IActionRunner};
use je_base::dynamic::Dynamic;
use je_base::event::{Emitter, Event as BaseEvent};
use je_base::key_codes::KeyCode;
use je_base::keybindings::ResolvedKeybinding;
use je_base::layout::AnchorAlignment;
use je_base::lifecycle::{DisposableRef, IDisposable};
use je_base::nls::localize;
use je_base::themables::ThemeIcon;
use je_render::Rgba;
use je_render::kurbo::Rect;
use je_ui::{Content, EventKind, NodeId, Role, Ui};

use crate::action_bar::IActionViewItemProvider;
use crate::action_view_items::{ACTION_ICON_SIZE, ACTION_LABEL_PADDING, ActionBarStyles, ActionViewItem, BaseActionViewItem, IActionViewItem, IActionViewItemOptions, ItemSize, LabelState};
use crate::context_view_api::{IContextMenuProvider, IContextViewCloseAnimation};
use crate::dom::{StandardKeyboardEvent, create_el};
use crate::dropdown::{DropdownMenu, DropdownMenuOptions, IActionProvider, IBaseDropdownOptions, IDropdown, IDropdownMenuOptions, ILabelRenderer};
use crate::hover_api::{HoverPlacement, ManagedHoverContentOrFactory, get_base_layer_hover_delegate, get_default_hover_delegate};

/// `IKeybindingProvider`.
pub type IKeybindingProvider = Rc<dyn Fn(&ActionRef) -> Option<Rc<dyn ResolvedKeybinding>>>;

/// `IAnchorAlignmentProvider`.
pub type IAnchorAlignmentProvider = Rc<dyn Fn() -> AnchorAlignment>;

/// `readonly IAction[] | IActionProvider`.
#[derive(Clone)]
pub enum MenuActions {
    Actions(Vec<ActionRef>),
    Provider(Rc<dyn IActionProvider>),
}

impl MenuActions {
    /// `Array.isArray(...) ? ... : provider.getActions()`.
    fn get_actions(&self) -> Vec<ActionRef> {
        match self {
            Self::Actions(actions) => actions.clone(),
            Self::Provider(provider) => provider.get_actions(),
        }
    }
}

/// `IDropdownMenuActionViewItemOptions`: as opções base do item mais as do menu.
#[derive(Clone, Default)]
pub struct IDropdownMenuActionViewItemOptions {
    pub base: IActionViewItemOptions,
    pub action_view_item_provider: Option<IActionViewItemProvider>,
    pub keybinding_provider: Option<IKeybindingProvider>,
    pub action_runner: Option<Rc<dyn IActionRunner>>,
    /// `classNames: string[] | string`; uma `string` vira lista com `split_whitespace`.
    pub class_names: Vec<String>,
    pub menu_class_name: Option<String>,
    pub close_animation: Option<IContextViewCloseAnimation>,
    pub get_anchor: Option<Rc<dyn Fn() -> NodeId>>,
    pub anchor_alignment_provider: Option<IAnchorAlignmentProvider>,
    pub menu_as_child: bool,
    pub skip_telemetry: bool,
}

/// O lado do `a.action-label.codicon` (`width: 16px` e `padding: 3px` dos dois lados).
const DROPDOWN_ICON_BOX: f64 = ACTION_ICON_SIZE + 2.0 * ACTION_LABEL_PADDING;

/// `DropdownMenuActionViewItem`.
pub struct DropdownMenuActionViewItem {
    base: BaseActionViewItem,
    weak_self: Weak<DropdownMenuActionViewItem>,
    menu_actions_or_provider: MenuActions,
    context_menu_provider: Rc<dyn IContextMenuProvider>,
    options: IDropdownMenuActionViewItemOptions,
    dropdown_menu: RefCell<Option<Rc<DropdownMenu>>>,
    action_item: Cell<Option<NodeId>>,
    on_did_change_visibility: Rc<Emitter<bool>>,
    styles: Rc<ActionBarStyles>,
    hovered: Cell<bool>,
    pressed: Cell<bool>,
}

impl DropdownMenuActionViewItem {
    /// `new DropdownMenuActionViewItem(action, menuActionsOrProvider, contextMenuProvider, options)`.
    pub fn new(ui: &Ui, action: ActionRef, menu_actions_or_provider: MenuActions, context_menu_provider: Rc<dyn IContextMenuProvider>, options: IDropdownMenuActionViewItemOptions) -> Rc<Self> {
        Rc::new_cyclic(|weak: &Weak<Self>| {
            let base = BaseActionViewItem::new_derived(ui, None, action, options.base.clone(), weak.clone() as Weak<dyn IActionViewItem>);
            if let Some(action_runner) = options.action_runner.clone() {
                base.set_action_runner(action_runner);
            }
            let on_did_change_visibility = Emitter::new();
            base.register(on_did_change_visibility.clone());
            Self {
                base,
                weak_self: weak.clone(),
                menu_actions_or_provider,
                context_menu_provider,
                dropdown_menu: RefCell::new(None),
                action_item: Cell::new(None),
                on_did_change_visibility,
                styles: options.base.styles.clone().unwrap_or_default(),
                hovered: Cell::new(false),
                pressed: Cell::new(false),
                options,
            }
        })
    }

    pub fn on_did_change_visibility(&self) -> BaseEvent<bool> {
        self.on_did_change_visibility.event()
    }

    /// O nó `a.action-label`, onde o menu ancora o foco e o `aria-haspopup`.
    pub fn element(&self) -> Option<NodeId> {
        self.base.element()
    }

    /// O [`DropdownMenu`] do item, depois do `render`.
    pub fn dropdown_menu(&self) -> Option<Rc<DropdownMenu>> {
        self.dropdown_menu.borrow().clone()
    }

    /// `show()`.
    pub fn show(&self) {
        if let Some(dropdown_menu) = self.dropdown_menu() {
            dropdown_menu.show();
        }
    }

    /// `setAriaLabelAttributes(element)`: `role="button"` e o rótulo da ação.
    fn set_aria_label_attributes(&self, element: NodeId) {
        let mut tree = self.base.ui.tree_mut();
        tree.set_role(element, Role::Button);
        tree.set_label(element, self.base.action().label());
    }

    /// `renderLabel(element)`: as classes do ícone, o codicon e a dica do rótulo da ação.
    fn render_label(&self, element: NodeId) {
        let mut class_names = self.options.class_names.clone();
        // todo@aeschli: remove codicon, should come through `this.options.classNames`
        if !class_names.iter().any(|c| c == "icon") {
            class_names.push("codicon".to_owned());
        }
        {
            let mut tree = self.base.ui.tree_mut();
            for class_name in &class_names {
                tree.add_class(element, class_name);
            }
            if let Some(id) = ActionViewItem::codicon_id(&class_names.join(" ")) {
                let color = self.styles.foreground.unwrap_or(Rgba::new(204, 204, 204, 255));
                tree.style_mut(element).content = Content::Codicon { id, size: ACTION_ICON_SIZE as f32, color };
            }
            tree.set_bounds(element, Rect::new(0.0, 0.0, DROPDOWN_ICON_BOX, DROPDOWN_ICON_BOX));
        }
        let label = self.base.action().label();
        if !label.is_empty() {
            let delegate = self.base.options.hover_delegate.clone().unwrap_or_else(|| get_default_hover_delegate(HoverPlacement::Mouse));
            let hover = get_base_layer_hover_delegate().setup_managed_hover(delegate, element, ManagedHoverContentOrFactory::from(label.as_str()), None);
            let disposable: DisposableRef = hover;
            self.base.register(disposable);
        }
    }

    /// As regras de `:hover`, `:active`, `:focus` e `.disabled` do rótulo (as mesmas do `ActionViewItem`).
    pub fn refresh_style(&self) {
        let Some(element) = self.base.element() else { return };
        let state = LabelState { disabled: !self.base.action().enabled(), hovered: self.hovered.get(), active: self.pressed.get(), focused: self.base.ui.focused() == Some(element), ..LabelState::default() };
        let mut tree = self.base.ui.tree_mut();
        let has_icon_class = tree.has_class(element, "icon");
        self.styles.apply_to_label(tree.style_mut(element), state, has_icon_class);
    }

    /// Põe o `.monaco-dropdown` em `x` com a largura e a altura do item e centra o rótulo. É o
    /// `align-items: center` do `li` quando o item está num botão dividido.
    fn place(&self, x: f64, width: f64, height: f64) {
        let Some(dropdown_menu) = self.dropdown_menu() else { return };
        self.base.ui.tree_mut().set_bounds(dropdown_menu.element(), Rect::new(x, 0.0, x + width, height));
        dropdown_menu.layout();
    }
}

impl IActionViewItem for DropdownMenuActionViewItem {
    fn base(&self) -> &BaseActionViewItem {
        &self.base
    }

    fn render(&self, container: NodeId) {
        self.action_item.set(Some(container));
        let weak = self.weak_self.clone();
        let label_renderer: ILabelRenderer = Rc::new(move |el: NodeId| {
            let this = weak.upgrade()?;
            let element = {
                let mut tree = this.base.ui.tree_mut();
                create_el(&mut tree, el, Role::Button, "action-label")
            };
            this.base.set_element(element);
            this.set_aria_label_attributes(element);
            this.render_label(element);
            None
        });

        let action = self.base.action();
        let (menu_actions, menu_provider): (Option<Vec<ActionRef>>, Option<Rc<dyn IActionProvider>>) = match &self.menu_actions_or_provider {
            MenuActions::Actions(actions) => (Some(actions.clone()), None),
            MenuActions::Provider(provider) => (None, Some(provider.clone())),
        };
        let dropdown_options = IDropdownMenuOptions {
            base: IBaseDropdownOptions { label_renderer: Some(label_renderer), is_enabled: Some(Rc::new(move || action.enabled())), ..IBaseDropdownOptions::default() },
            context_menu_provider: self.context_menu_provider.clone(),
            actions: menu_actions,
            action_provider: menu_provider,
            menu_class_name: self.options.menu_class_name.clone(),
            close_animation: self.options.close_animation.clone(),
            get_anchor: self.options.get_anchor.clone(),
            menu_as_child: self.options.menu_as_child,
            skip_telemetry: self.options.skip_telemetry,
        };
        let dropdown_menu = DropdownMenu::new(&self.base.ui, container, dropdown_options);
        self.base.register(dropdown_menu.clone());
        *self.dropdown_menu.borrow_mut() = Some(dropdown_menu.clone());
        let fire = self.on_did_change_visibility.clone();
        self.base.register(dropdown_menu.on_did_change_visibility().listen(move |visible| fire.fire(*visible)));

        dropdown_menu.set_menu_options(Some(DropdownMenuOptions {
            context: self.base.context(),
            action_view_item_provider: self.options.action_view_item_provider.clone(),
            action_runner: Some(self.action_runner()),
            get_key_binding: self.options.keybinding_provider.clone(),
            anchor_alignment: self.options.anchor_alignment_provider.clone(),
        }));

        if let Some(element) = self.base.element() {
            let ui = self.base.ui.clone();
            for kind in [EventKind::PointerEnter, EventKind::PointerLeave] {
                let weak = self.weak_self.clone();
                self.base.register(ui.listen(element, kind, false, move |e| {
                    if let Some(this) = weak.upgrade() {
                        this.hovered.set(e.kind == EventKind::PointerEnter);
                        this.pressed.set(this.pressed.get() && e.kind == EventKind::PointerEnter);
                        this.refresh_style();
                    }
                }));
            }
            for kind in [EventKind::PointerDown, EventKind::PointerUp, EventKind::Focus, EventKind::Blur] {
                let weak = self.weak_self.clone();
                self.base.register(ui.listen(element, kind, false, move |e| {
                    if let Some(this) = weak.upgrade() {
                        match e.kind {
                            EventKind::PointerDown => this.pressed.set(true),
                            EventKind::PointerUp => this.pressed.set(false),
                            _ => {}
                        }
                        this.refresh_style();
                    }
                }));
            }
        }

        self.base.update_tooltip();
        self.update_enabled();
    }

    fn get_tooltip(&self) -> Option<String> {
        let action = self.base.action();
        let tooltip = action.tooltip();
        if !tooltip.is_empty() {
            Some(tooltip)
        } else {
            Some(action.label()).filter(|label| !label.is_empty())
        }
    }

    fn set_action_context(&self, context: Option<Dynamic>) {
        self.base.set_action_context(context.clone());
        if let Some(dropdown_menu) = self.dropdown_menu() {
            dropdown_menu.set_menu_context(context);
        }
    }

    fn update_enabled(&self) {
        let disabled = !self.base.action().enabled();
        {
            let mut tree = self.base.ui.tree_mut();
            for node in [self.action_item.get(), self.base.element()].into_iter().flatten() {
                tree.toggle_class(node, "disabled", disabled);
            }
        }
        self.refresh_style();
    }

    fn size(&self, _vertical: bool) -> ItemSize {
        if self.base.element().is_some() { (DROPDOWN_ICON_BOX, DROPDOWN_ICON_BOX) } else { (0.0, 0.0) }
    }

    fn layout(&self, _vertical: bool, width: f64, height: f64) {
        self.place(0.0, width, height);
    }
}

crate::impl_action_view_item_dispose!(DropdownMenuActionViewItem, base);

/// `IActionWithDropdownActionViewItemOptions`: as opções do item mais as do menu.
#[derive(Clone)]
pub struct IActionWithDropdownActionViewItemOptions {
    pub base: IActionViewItemOptions,
    pub menu_actions_or_provider: MenuActions,
    pub menu_action_class_names: Vec<String>,
    pub keybinding_provider: Option<IKeybindingProvider>,
}

/// `ActionWithDropdownActionViewItem`.
pub struct ActionWithDropdownActionViewItem {
    inner: Rc<ActionViewItem>,
    weak_self: Weak<ActionWithDropdownActionViewItem>,
    options: IActionWithDropdownActionViewItemOptions,
    context_menu_provider: Rc<dyn IContextMenuProvider>,
    dropdown_menu_action_view_item: RefCell<Option<Rc<DropdownMenuActionViewItem>>>,
    separator: Cell<Option<NodeId>>,
}

impl ActionWithDropdownActionViewItem {
    /// `new ActionWithDropdownActionViewItem(context, action, options, contextMenuProvider)`.
    pub fn new(ui: &Ui, context: Option<Dynamic>, action: ActionRef, options: IActionWithDropdownActionViewItemOptions, context_menu_provider: Rc<dyn IContextMenuProvider>) -> Rc<Self> {
        Rc::new_cyclic(|weak: &Weak<Self>| Self {
            inner: ActionViewItem::new_with_owner(ui, context, action, options.base.clone(), weak.clone() as Weak<dyn IActionViewItem>),
            weak_self: weak.clone(),
            options,
            context_menu_provider,
            dropdown_menu_action_view_item: RefCell::new(None),
            separator: Cell::new(None),
        })
    }

    /// O botão do menu, depois do `render`.
    pub fn dropdown_menu_action_view_item(&self) -> Option<Rc<DropdownMenuActionViewItem>> {
        self.dropdown_menu_action_view_item.borrow().clone()
    }

    fn menu_actions(&self) -> Vec<ActionRef> {
        self.options.menu_actions_or_provider.get_actions()
    }

    /// O keydown do `li`: as setas movem o foco entre a ação e o botão do menu.
    fn on_key_down(&self, e: &mut je_ui::Event) {
        // Sem ações o botão do menu está escondido, então não tenta focá-lo (#164050)
        if self.menu_actions().is_empty() {
            return;
        }
        let (Some(event), Some(dropdown)) = (StandardKeyboardEvent::from_event(e), self.dropdown_menu_action_view_item()) else { return };
        let mut handled = false;
        if dropdown.is_focused() && event.equals(KeyCode::LeftArrow.0 as u32) {
            handled = true;
            dropdown.blur();
            self.focus(false);
        } else if self.is_focused() && event.equals(KeyCode::RightArrow.0 as u32) {
            handled = true;
            self.blur();
            dropdown.focus(false);
        }
        if handled {
            e.stop(true);
        }
    }
}

impl IActionViewItem for ActionWithDropdownActionViewItem {
    fn base(&self) -> &BaseActionViewItem {
        self.inner.base()
    }

    fn render(&self, container: NodeId) {
        self.inner.render(container);
        let Some(element) = self.inner.base().element() else { return };
        let ui = self.inner.base().ui().clone();
        {
            let mut tree = ui.tree_mut();
            tree.add_class(element, "action-dropdown-item");
            let separator = create_el(&mut tree, element, Role::Generic, "action-dropdown-item-separator");
            create_el(&mut tree, separator, Role::Generic, "");
            tree.toggle_class(separator, "prominent", self.options.menu_action_class_names.iter().any(|c| c == "prominent"));
            self.separator.set(Some(separator));
        }

        let weak = self.weak_self.clone();
        let menu_actions: Rc<dyn IActionProvider> = Rc::new(move || weak.upgrade().map(|this| this.menu_actions()).unwrap_or_default());
        let dropdown_action = Action::new("dropdownAction", &localize("moreActions", "More Actions...", &[]), "", self.base().action().enabled(), None);
        self.inner.base().register(dropdown_action.clone());
        let mut class_names = vec!["dropdown".to_owned()];
        class_names.extend(ThemeIcon::from_id("drop-down-button").as_class_name_array());
        class_names.extend(self.options.menu_action_class_names.iter().cloned());
        let dropdown = DropdownMenuActionViewItem::new(
            &ui,
            dropdown_action,
            MenuActions::Provider(menu_actions),
            self.context_menu_provider.clone(),
            IDropdownMenuActionViewItemOptions {
                base: IActionViewItemOptions { hover_delegate: self.inner.base().options.hover_delegate.clone(), styles: self.inner.base().options.styles.clone(), ..IActionViewItemOptions::default() },
                class_names,
                keybinding_provider: self.options.keybinding_provider.clone(),
                ..IDropdownMenuActionViewItemOptions::default()
            },
        );
        self.inner.base().register(dropdown.clone());
        dropdown.render(element);
        *self.dropdown_menu_action_view_item.borrow_mut() = Some(dropdown);

        let weak = self.weak_self.clone();
        self.inner.base().register(ui.listen(element, EventKind::KeyDown, false, move |e| {
            if let Some(this) = weak.upgrade() {
                this.on_key_down(e);
            }
        }));
        self.inner.base().request_layout();
    }

    fn focus(&self, from_right: bool) {
        self.inner.focus(from_right);
    }

    fn is_focused(&self) -> bool {
        self.inner.is_focused()
    }

    fn blur(&self) {
        self.inner.blur();
        if let Some(dropdown) = self.dropdown_menu_action_view_item() {
            dropdown.blur();
        }
    }

    fn set_focusable(&self, focusable: bool) {
        self.inner.set_focusable(focusable);
        if let Some(dropdown) = self.dropdown_menu_action_view_item() {
            dropdown.set_focusable(focusable);
        }
    }

    fn tab_stops(&self) -> usize {
        self.inner.tab_stops() + self.dropdown_menu_action_view_item().map_or(0, |dropdown| dropdown.tab_stops())
    }

    fn focus_descendant(&self, node: NodeId) -> bool {
        let Some(dropdown) = self.dropdown_menu_action_view_item().filter(|d| d.element() == Some(node)) else { return false };
        dropdown.set_focusable(true);
        self.inner.base().ui().focus(node);
        true
    }

    fn update_enabled(&self) {
        self.inner.update_enabled();
        if let Some(dropdown) = self.dropdown_menu_action_view_item() {
            dropdown.action().set_enabled(self.base().action().enabled());
        }
    }

    fn update_label(&self) {
        self.inner.update_label();
    }

    fn update_class(&self) {
        self.inner.update_class();
    }

    fn update_checked(&self) {
        self.inner.update_checked();
    }

    fn get_tooltip(&self) -> Option<String> {
        self.inner.get_tooltip()
    }

    fn update_aria_label(&self) {
        self.inner.update_aria_label();
    }

    fn size(&self, vertical: bool) -> ItemSize {
        let (label_width, label_height) = self.inner.size(vertical);
        match self.dropdown_menu_action_view_item() {
            Some(dropdown) => {
                let (dropdown_width, dropdown_height) = dropdown.size(vertical);
                (label_width + 1.0 + dropdown_width, label_height.max(dropdown_height))
            }
            None => (label_width, label_height),
        }
    }

    fn layout(&self, vertical: bool, width: f64, height: f64) {
        let (label_width, label_height) = self.inner.size(vertical);
        self.inner.layout(vertical, label_width, height);
        let Some(dropdown) = self.dropdown_menu_action_view_item() else { return };
        let ui = self.inner.base().ui().clone();
        {
            let mut tree = ui.tree_mut();
            if let Some(label) = self.inner.label() {
                let bounds = tree.bounds(label);
                let y = (height - label_height) / 2.0;
                tree.set_bounds(label, Rect::new(bounds.x0, y, bounds.x1, y + bounds.height()));
            }
            if let Some(separator) = self.separator.get() {
                tree.set_bounds(separator, Rect::new(label_width, height / 2.0, label_width + 1.0, height / 2.0));
            }
        }
        dropdown.place(label_width + 1.0, (width - label_width - 1.0).max(0.0), height);
    }
}

impl IDisposable for ActionWithDropdownActionViewItem {
    fn dispose(&self) {
        self.inner.dispose();
    }
}
