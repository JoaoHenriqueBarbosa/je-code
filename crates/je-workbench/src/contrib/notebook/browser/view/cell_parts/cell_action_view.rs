//! Porte de `src/vs/workbench/contrib/notebook/browser/view/cellParts/cellActionView.ts`: os itens de ação das barras de ferramentas
//! do notebook, `CodiconActionViewItem`, `ActionViewWithLabel` e `UnifiedSubmenuActionView`. O primeiro é o que o `BetweenCellToolbar`
//! (`cellToolbars.ts`) cria quando a barra de inserção é centralizada.
//!
//! Herança vira composição (ver `crate::platform::actions::menu_entry_action_view_item`, onde estão as classes-base
//! `MenuEntryActionViewItem` e `SubmenuEntryActionViewItem`).
//!
//! Divergências:
//! - **`DOM.reset(this.label, ...renderLabelWithIcons(label))`** do `CodiconActionViewItem` monta as partes do rótulo
//!   (`$(icon) texto`) no nó de texto do rótulo do item (o filho `action-label-text` do `a.action-label`), no lugar de esvaziar o
//!   próprio rótulo, porque o `ActionViewItem` do `je-widgets` guarda esse nó e o redesenha; a largura vem de `layout_parts` e o
//!   `size` do item a usa.
//! - **`document.createElement('a')` e `innerText`** (`ActionViewWithLabel` e `UnifiedSubmenuActionView`) são um nó da árvore com a
//!   classe `notebook-label` e um filho de texto; o `padding: 0 5px 0 2px` do `.notebook-label` (`notebookToolbar.css`) é o deslocamento
//!   do filho e a largura somada ao tamanho do item.
//! - **Rótulo próprio dos itens com ícone.** Num item sem ícone (`options.label`), o `MenuEntryActionViewItem` não desenha o texto
//!   (o `updateLabel` do `ActionViewWithLabel` não chama o do pai), então o nó de texto do pai é escondido.
//! - **`UnifiedSubmenuActionView`.** O `SubmenuEntryActionViewItem` pai é um `DropdownMenuActionViewItem`, cujo `owner` é ele mesmo: os
//!   ganchos virtuais dele (`updateLabel`) não chegam a este item, então `update_label` roda no `render` e a cada mudança dos
//!   `subActionProvider` pedida por quem chama. `setupManagedHover(..., this._actionLabel, '')` é o `setup_managed_hover` do hover base
//!   no nó do rótulo; o toque (`TouchEventType.Tap`) não existe no `je-ui`. O `IHoverService` é o hover base da camada.
//! - **`classList.forEach(...remove('codicon-...'))` e `add(...iconClasses)`** trocam também o conteúdo codicon do nó do elemento, que
//!   na árvore nativa é o que desenha o ícone (a classe sozinha não o muda).

use std::cell::{Cell, RefCell};
use std::rc::{Rc, Weak};

use je_base::actions::{ActionRef, IAction};
use je_base::dynamic::{Dynamic, ObjectRef};
use je_base::lifecycle::IDisposable;
use je_platform::action::Icon;
use je_platform::actions::{MenuItemAction, SubmenuItemAction};
use je_platform::keybinding::keybinding::IKeybindingService;
use je_render::Rgba;
use je_render::kurbo::Rect;
use je_ui::{Content, EventKind, NodeId, Role, Ui};
use je_widgets::action_bar::IActionViewItemProvider;
use je_widgets::action_view_items::{ACTION_ICON_SIZE, ACTION_LABEL_FONT_SIZE, ACTION_LABEL_PADDING, ActionBarStyles, ActionViewItem, BaseActionViewItem, IActionViewItem, ItemSize, event_as_context};
use je_widgets::context_view_api::IContextMenuProvider;
use je_widgets::dom::{clear_node, create_el};
use je_widgets::dropdown::IActionProvider;
use je_widgets::dropdown_action_view_item::IDropdownMenuActionViewItemOptions;
use je_widgets::hover_api::{HoverPlacement, IManagedHover, ManagedHoverContentOrFactory, get_base_layer_hover_delegate, get_default_hover_delegate};
use je_widgets::icon_labels::{InlineStyle, build_parts, layout_parts, render_label_with_icons};
use je_widgets::text::{WORKBENCH_LINE_HEIGHT, set_node_text, ui_font};

use crate::platform::actions::menu_entry_action_view_item::{IMenuEntryActionViewItemOptions, MenuEntryActionViewItem, MenuEntryServices, SubmenuEntryActionViewItem, delegate_action_view_item};

/// O `padding: 0px 5px 0px 2px` do `.notebook-label`: esquerda e direita.
const NOTEBOOK_LABEL_PADDING_LEFT: f64 = 2.0;
const NOTEBOOK_LABEL_PADDING_RIGHT: f64 = 5.0;

/// A cor do texto de um item: a de primeiro plano do tema, ou a do tema escuro padrão.
fn foreground(styles: &ActionBarStyles) -> Rgba {
    styles.foreground.unwrap_or(Rgba::new(204, 204, 204, 255))
}

/// O filho `action-label-text` do `a.action-label` do [`ActionViewItem`] (o nó onde ele desenha o texto).
fn label_text_node(item: &ActionViewItem) -> Option<NodeId> {
    let label = item.label()?;
    let ui = item.base().ui();
    let tree = ui.tree();
    let found = tree.children(label).iter().copied().find(|child| tree.has_class(*child, "action-label-text"));
    found
}

/// `a.classList.add('notebook-label'); a.innerText = text`: devolve a largura do rótulo, com o `padding` do `.notebook-label`.
fn set_notebook_label(ui: &Ui, node: NodeId, text: &str, styles: &ActionBarStyles) -> f64 {
    let font = ui_font(ACTION_LABEL_FONT_SIZE, 400);
    let mut tree = ui.tree_mut();
    tree.add_class(node, "notebook-label");
    clear_node(&mut tree, node);
    let child = create_el(&mut tree, node, Role::Generic, "");
    let width = set_node_text(&mut tree, child, text, &font, foreground(styles), WORKBENCH_LINE_HEIGHT);
    tree.set_bounds(child, Rect::new(NOTEBOOK_LABEL_PADDING_LEFT, 0.0, NOTEBOOK_LABEL_PADDING_LEFT + width, WORKBENCH_LINE_HEIGHT));
    NOTEBOOK_LABEL_PADDING_LEFT + width + NOTEBOOK_LABEL_PADDING_RIGHT
}

/// `CodiconActionViewItem`.
pub struct CodiconActionViewItem {
    inner: Rc<MenuEntryActionViewItem>,
    /// A largura do rótulo montado por `update_label`; sem ela, o tamanho é o do pai.
    content_width: Cell<Option<f64>>,
}

impl CodiconActionViewItem {
    /// `new CodiconActionViewItem(action, options, ...)`. A `action` é a de um `MenuItemAction`.
    pub fn new(ui: &Ui, action: ActionRef, options: Option<IMenuEntryActionViewItemOptions>, services: MenuEntryServices) -> Rc<Self> {
        Rc::new_cyclic(|weak: &Weak<Self>| Self {
            inner: MenuEntryActionViewItem::new_with_owner(ui, action, options, services, weak.clone() as Weak<dyn IActionViewItem>),
            content_width: Cell::new(None),
        })
    }

    /// O tamanho natural do rótulo montado: a largura das partes e a altura de uma linha, com o `padding` do `.action-label`.
    fn content_size(&self, width: f64) -> ItemSize {
        (width + 2.0 * ACTION_LABEL_PADDING, WORKBENCH_LINE_HEIGHT + 2.0 * ACTION_LABEL_PADDING)
    }
}

delegate_action_view_item!(CodiconActionViewItem, inner, {
    fn render(&self, container: NodeId) {
        self.inner.render(container);
        self.update_label();
    }

    /// `if (this.options.label && this.label) DOM.reset(this.label, ...renderLabelWithIcons(this._commandAction.label ?? ''))`.
    fn update_label(&self) {
        if !self.inner.is_label_mode() {
            return;
        }
        let item = self.inner.inner();
        let Some(text_node) = label_text_node(item) else { return };
        let styles = item.base().options.styles.clone().unwrap_or_default();
        let style = InlineStyle { size_px: ACTION_LABEL_FONT_SIZE, weight: 400, color: foreground(&styles), line_height: WORKBENCH_LINE_HEIGHT, italic: false, strikethrough: false };
        let parts = render_label_with_icons(&self.inner.command_action().label(), false);
        let width = {
            let mut tree = item.base().ui().tree_mut();
            clear_node(&mut tree, text_node);
            tree.style_mut(text_node).content = Content::None;
            tree.style_mut(text_node).visible = true;
            let rendered = build_parts(&mut tree, text_node, &parts, &style);
            let width = layout_parts(&mut tree, &rendered, style.line_height, None);
            tree.set_bounds(text_node, Rect::new(ACTION_LABEL_PADDING, ACTION_LABEL_PADDING, ACTION_LABEL_PADDING + width, ACTION_LABEL_PADDING + WORKBENCH_LINE_HEIGHT));
            width
        };
        self.content_width.set(Some(width));
        item.base().request_layout();
    }

    fn update_class(&self) {
        self.inner.update_class();
        self.update_label();
    }

    fn get_tooltip(&self) -> Option<String> {
        self.inner.get_tooltip()
    }

    fn size(&self, vertical: bool) -> ItemSize {
        match self.content_width.get() {
            Some(width) if self.inner.is_label_mode() => self.content_size(width),
            _ => self.inner.size(vertical),
        }
    }

    fn layout(&self, vertical: bool, width: f64, height: f64) {
        self.inner.layout(vertical, width, height);
        if let (Some(content_width), true, Some(label)) = (self.content_width.get(), self.inner.is_label_mode(), self.inner.inner().label()) {
            let (label_width, label_height) = self.content_size(content_width);
            let ui = self.inner.inner().base().ui();
            ui.tree_mut().set_bounds(label, Rect::new(0.0, 0.0, if vertical { width.max(label_width) } else { label_width }, label_height));
            self.inner.inner().refresh_style();
        }
    }
});

/// `ActionViewWithLabel`.
pub struct ActionViewWithLabel {
    inner: Rc<MenuEntryActionViewItem>,
    action_label: Cell<Option<NodeId>>,
    label_width: Cell<f64>,
}

impl ActionViewWithLabel {
    /// `new ActionViewWithLabel(action, options, ...)`. A `action` é a de um `MenuItemAction`.
    pub fn new(ui: &Ui, action: ActionRef, options: Option<IMenuEntryActionViewItemOptions>, services: MenuEntryServices) -> Rc<Self> {
        Rc::new_cyclic(|weak: &Weak<Self>| Self {
            inner: MenuEntryActionViewItem::new_with_owner(ui, action, options, services, weak.clone() as Weak<dyn IActionViewItem>),
            action_label: Cell::new(None),
            label_width: Cell::new(0.0),
        })
    }

    /// Esconde o texto que o pai desenharia: o `updateLabel` desta classe não chama o do pai.
    fn hide_parent_text(&self) {
        let item = self.inner.inner();
        if let Some(text_node) = label_text_node(item) {
            item.base().ui().tree_mut().style_mut(text_node).visible = false;
        }
    }
}

delegate_action_view_item!(ActionViewWithLabel, inner, {
    fn render(&self, container: NodeId) {
        self.inner.render(container);
        let ui = self.inner.inner().base().ui().clone();
        {
            let mut tree = ui.tree_mut();
            tree.add_class(container, "notebook-action-view-item");
            let action_label = create_el(&mut tree, container, Role::Generic, "");
            self.action_label.set(Some(action_label));
        }
        self.hide_parent_text();
        self.update_label();
    }

    fn update_label(&self) {
        let Some(action_label) = self.action_label.get() else { return };
        let item = self.inner.inner();
        let styles = item.base().options.styles.clone().unwrap_or_default();
        let width = set_notebook_label(item.base().ui(), action_label, &item.base().action().label(), &styles);
        self.label_width.set(width);
        item.base().request_layout();
    }

    fn update_class(&self) {
        self.inner.update_class();
        self.hide_parent_text();
    }

    fn get_tooltip(&self) -> Option<String> {
        self.inner.get_tooltip()
    }

    fn size(&self, vertical: bool) -> ItemSize {
        let (width, height) = self.inner.size(vertical);
        (width + self.label_width.get(), height.max(WORKBENCH_LINE_HEIGHT))
    }

    fn layout(&self, vertical: bool, width: f64, height: f64) {
        let (parent_width, parent_height) = self.inner.size(vertical);
        self.inner.layout(vertical, width, height);
        if let Some(action_label) = self.action_label.get() {
            let ui = self.inner.inner().base().ui();
            ui.tree_mut().set_bounds(action_label, Rect::new(parent_width, 0.0, parent_width + self.label_width.get(), parent_height.max(WORKBENCH_LINE_HEIGHT)));
        }
    }
});

/// `UnifiedSubmenuActionView`.
pub struct UnifiedSubmenuActionView {
    inner: Rc<SubmenuEntryActionViewItem>,
    weak_self: Weak<UnifiedSubmenuActionView>,
    render_label: bool,
    sub_action_provider: Rc<dyn IActionProvider>,
    sub_action_view_item_provider: Option<IActionViewItemProvider>,
    action_label: Cell<Option<NodeId>>,
    label_width: Cell<f64>,
    hover: RefCell<Option<Rc<dyn IManagedHover>>>,
    primary_action: RefCell<Option<ActionRef>>,
}

impl UnifiedSubmenuActionView {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        ui: &Ui,
        action: Rc<SubmenuItemAction>,
        options: Option<IDropdownMenuActionViewItemOptions>,
        render_label: bool,
        sub_action_provider: Rc<dyn IActionProvider>,
        sub_action_view_item_provider: Option<IActionViewItemProvider>,
        keybinding_service: Rc<dyn IKeybindingService>,
        context_menu_provider: Rc<dyn IContextMenuProvider>,
    ) -> Rc<Self> {
        let mut options = options.unwrap_or_default();
        if options.base.hover_delegate.is_none() {
            options.base.hover_delegate = Some(get_default_hover_delegate(HoverPlacement::Element));
        }
        Rc::new_cyclic(|weak: &Weak<Self>| Self {
            inner: SubmenuEntryActionViewItem::new(ui, action, Some(options), keybinding_service, context_menu_provider),
            weak_self: weak.clone(),
            render_label,
            sub_action_provider,
            sub_action_view_item_provider,
            action_label: Cell::new(None),
            label_width: Cell::new(0.0),
            hover: RefCell::new(None),
            primary_action: RefCell::new(None),
        })
    }

    /// `readonly subActionViewItemProvider`.
    pub fn sub_action_view_item_provider(&self) -> Option<&IActionViewItemProvider> {
        self.sub_action_view_item_provider.as_ref()
    }

    /// `onClick(event, preserveFocus = false)`.
    pub fn on_click(&self, event: &mut je_ui::Event, preserve_focus: bool) {
        event.stop(true);
        let base = self.inner.base();
        let context = base.context().unwrap_or_else(|| {
            if base.options.use_event_as_context { event_as_context(event) } else { Dynamic::Object(ObjectRef::from_entries([("preserveFocus", Dynamic::Bool(preserve_focus))])) }
        });
        let action = self.primary_action.borrow().clone().unwrap_or_else(|| base.action());
        // O `run` do runner devolve a promessa e o original a ignora.
        let _ = base.action_runner().run(&action, Some(context));
    }

    /// `updateLabel()`. Chamada no `render` e quando as ações do `subActionProvider` mudam.
    pub fn update_label(&self) {
        let actions = self.sub_action_provider.get_actions();
        let Some(action_label) = self.action_label.get() else { return };
        let base = self.inner.base();
        let ui = base.ui().clone();
        let primary_action = actions.first().cloned();
        *self.primary_action.borrow_mut() = primary_action.clone();

        // O rótulo e a dica vêm da ação principal quando ela é um `MenuItemAction`, e da própria ação nos demais casos.
        let (label, tooltip) = match primary_action.as_ref().and_then(|primary| primary.as_any().downcast_ref::<MenuItemAction>().map(|menu_item| (primary, menu_item))) {
            Some((primary, menu_item)) => {
                if let (Some(element), Some(Icon::Theme(icon))) = (self.inner.inner().element(), &menu_item.item.icon) {
                    let icon_classes = icon.as_class_name_array();
                    let mut tree = ui.tree_mut();
                    // remove all classes started with 'codicon-'
                    let stale: Vec<String> = tree.classes(element).iter().filter(|class| class.starts_with("codicon-")).cloned().collect();
                    for class in &stale {
                        tree.remove_class(element, class);
                    }
                    for class in &icon_classes {
                        tree.add_class(element, class);
                    }
                    if let Some(id) = ActionViewItem::codicon_id(&icon_classes.join(" ")) {
                        let color = match &tree.style_mut(element).content {
                            Content::Codicon { color, .. } => *color,
                            _ => foreground(&base.options.styles.clone().unwrap_or_default()),
                        };
                        tree.style_mut(element).content = Content::Codicon { id, size: ACTION_ICON_SIZE as f32, color };
                    }
                }
                (base.action().label(), if primary.tooltip().is_empty() { primary.label() } else { primary.tooltip() })
            }
            None => {
                let action = base.action();
                (action.label(), if action.tooltip().is_empty() { action.label() } else { action.tooltip() })
            }
        };

        if self.render_label {
            let styles = base.options.styles.clone().unwrap_or_default();
            self.label_width.set(set_notebook_label(&ui, action_label, &label, &styles));
            if let Some(hover) = self.hover.borrow().as_ref() {
                hover.update(ManagedHoverContentOrFactory::from(tooltip.as_str()), None);
            }
            base.request_layout();
        }
    }
}

impl IActionViewItem for UnifiedSubmenuActionView {
    fn base(&self) -> &BaseActionViewItem {
        self.inner.base()
    }

    fn render(&self, container: NodeId) {
        self.inner.render(container);
        let base = self.inner.base();
        let ui = base.ui().clone();
        let action_label = {
            let mut tree = ui.tree_mut();
            tree.add_class(container, "notebook-action-view-item");
            tree.add_class(container, "notebook-action-view-item-unified");
            create_el(&mut tree, container, Role::Generic, "")
        };
        self.action_label.set(Some(action_label));

        let delegate = base.options.hover_delegate.clone().unwrap_or_else(|| get_default_hover_delegate(HoverPlacement::Element));
        let hover = get_base_layer_hover_delegate().setup_managed_hover(delegate, action_label, ManagedHoverContentOrFactory::from(""), None);
        *self.hover.borrow_mut() = Some(hover);

        self.update_label();

        for kind in [EventKind::Click, EventKind::PointerDown] {
            let weak = self.weak_self.clone();
            base.register(ui.listen(container, kind, false, move |e| {
                if let Some(this) = weak.upgrade() {
                    this.on_click(e, true);
                }
            }));
        }
    }

    fn focus(&self, from_right: bool) {
        self.inner.focus(from_right);
    }

    fn is_focused(&self) -> bool {
        self.inner.is_focused()
    }

    fn blur(&self) {
        self.inner.blur();
    }

    fn set_focusable(&self, focusable: bool) {
        self.inner.set_focusable(focusable);
    }

    fn tab_stops(&self) -> usize {
        self.inner.tab_stops()
    }

    fn get_tooltip(&self) -> Option<String> {
        self.inner.get_tooltip()
    }

    fn set_action_context(&self, context: Option<Dynamic>) {
        self.inner.set_action_context(context);
    }

    fn update_enabled(&self) {
        self.inner.update_enabled();
    }

    fn update_label(&self) {
        UnifiedSubmenuActionView::update_label(self);
    }

    fn size(&self, vertical: bool) -> ItemSize {
        let (width, height) = self.inner.size(vertical);
        (width + self.label_width.get(), height.max(WORKBENCH_LINE_HEIGHT))
    }

    fn layout(&self, vertical: bool, width: f64, height: f64) {
        let (icon_width, icon_height) = self.inner.size(vertical);
        self.inner.layout(vertical, width, height);
        if let Some(action_label) = self.action_label.get() {
            let ui = self.inner.base().ui();
            ui.tree_mut().set_bounds(action_label, Rect::new(icon_width, 0.0, icon_width + self.label_width.get(), icon_height.max(WORKBENCH_LINE_HEIGHT)));
        }
    }
}

impl IDisposable for UnifiedSubmenuActionView {
    fn dispose(&self) {
        let hover = self.hover.borrow_mut().take();
        if let Some(hover) = hover {
            hover.dispose();
        }
        self.inner.dispose();
    }
}
