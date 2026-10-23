//! Os itens de um [`Menu`]: `BaseMenuActionViewItem` (o item de ação) e `SubmenuMenuActionViewItem` (o item
//! que abre um submenu) de `menu.ts`, num tipo só, [`MenuActionViewItem`], com o que é do submenu em
//! [`SubmenuState`] (a herança do original vira um campo opcional, sem repasse de método).
//!
//! DOM para `je-ui`: `li.action-item` é o contêiner que a [`ActionBar`](crate::action_bar::ActionBar) cria;
//! dentro dele `a.action-menu-item` (papel `menuitem`, ou `menuitemcheckbox` com `checked`), e dentro do `a`
//! `span.menu-item-check`, `span.action-label` (um nó filho por trecho de texto: o `<u>` do mnemônico é um
//! trecho com a linha de sublinhado), `span.keybinding` e `span.submenu-indicator`. Os atributos ARIA que
//! o `je-ui` não tem (`aria-keyshortcuts`, `aria-checked`, `aria-expanded`, `aria-posinset`, `aria-setsize`,
//! `aria-haspopup`, `aria-disabled`) ficam em [`MenuActionViewItem::attribute`]; `aria-label` do rótulo é o
//! rótulo do nó.
//!
//! O CSS de `getMenuWidgetCSS` vira [`MenuActionViewItem::layout`] (flex: o rótulo cresce 1 e o atalho e o
//! indicador 2, `padding: 0 2em`, o `.menu-item-check` absoluto de `2em`) e [`MenuActionViewItem::apply_style`]
//! (cores, `:hover` e `:focus` do atalho, `opacity` do desabilitado, o contorno de seleção só com o teclado
//! `:focus-visible`).

use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::rc::{Rc, Weak};

use je_base::actions::{ActionRef, EmptySubmenuAction};
use je_base::async_::RunOnceScheduler;
use je_base::dynamic::Dynamic;
use je_base::icon_labels::strip_icons;
use je_base::key_codes::KeyCode;
use je_base::layout::{ILayoutAnchor, LayoutAnchorPosition, layout as layout_1d};
use je_base::lifecycle::{DisposableStore, IDisposable};
use je_base::runtime;
use je_render::Rgba;
use je_render::kurbo::Rect;
use je_ui::{Border, Content, Cursor, Event, EventKind, NodeId, Role, Ui};

use crate::action_view_items::{BaseActionViewItem, IActionViewItem, IActionViewItemOptions, ItemSize};
use crate::dom::{StandardKeyboardEvent, create_el, window_size};
use crate::modifier_key_emitter::ModifierKeyEmitter;
use crate::text::ui_font;

use super::mnemonics::{LabelPieces, menu_item_label};
use super::styles::{
    IMenuStyles, MENU_CORNER_RADIUS_MEDIUM, MENU_DISABLED_SECONDARY_OPACITY, MENU_FALLBACK_FOREGROUND, MENU_FONT_SIZE, MENU_ITEM_HEIGHT, MENU_ITEM_MARGIN, MENU_LABEL_PADDING,
    MENU_SECONDARY_OPACITY, MENU_SUBMENU_INDICATOR_PADDING, MENU_SUBMENU_INDICATOR_SIZE, MENU_VERTICAL_PADDING,
};
use super::{GetKeyBinding, HorizontalDirection, IMenuDirection, IMenuOptions, Menu, ParentData, PointerMotion, VerticalDirection};

/// `IMenuItemOptions`: as opções do item de ação mais o que o menu repassa (`enableMnemonics`, `getKeyBinding`).
#[derive(Clone, Default)]
pub struct IMenuItemOptions {
    pub base: IActionViewItemOptions,
    pub enable_mnemonics: bool,
    pub get_key_binding: Option<GetKeyBinding>,
}

/// Os nós do item, criados em `render`.
#[derive(Clone, Copy)]
struct ItemNodes {
    item: NodeId,
    check: NodeId,
    label: NodeId,
    keybinding: Option<NodeId>,
    submenu_indicator: Option<NodeId>,
}

/// Um trecho de texto do rótulo (um nó), com o sublinhado do `<u>`.
struct Piece {
    node: NodeId,
    width: f64,
    underline: Option<NodeId>,
}

#[derive(Clone, Copy, Default)]
struct Metrics {
    /// A largura do texto do rótulo.
    label_text: f64,
    /// A largura do texto do atalho (0 sem atalho).
    keybinding_text: f64,
    keybinding_visible: bool,
}

/// O que é só do item de submenu (`SubmenuMenuActionViewItem`).
struct SubmenuState {
    submenu_actions: Vec<ActionRef>,
    parent_data: Rc<ParentData>,
    submenu_options: IMenuOptions,
    mysubmenu: RefCell<Option<Rc<Menu>>>,
    submenu_container: Cell<Option<NodeId>>,
    submenu_disposables: Rc<DisposableStore>,
    mouse_over: Cell<bool>,
    show_scheduler: Rc<RunOnceScheduler>,
    hide_scheduler: Rc<RunOnceScheduler>,
    expand_direction: IMenuDirection,
    motion: Rc<PointerMotion>,
}

enum Kind {
    Action,
    Submenu(Box<SubmenuState>),
}

pub struct MenuActionViewItem {
    base: BaseActionViewItem,
    weak_self: Weak<MenuActionViewItem>,
    options: IMenuItemOptions,
    menu_style: Rc<IMenuStyles>,
    ui: Ui,
    displayed_action: RefCell<ActionRef>,
    nodes: Cell<Option<ItemNodes>>,
    container: Cell<Option<NodeId>>,
    mnemonic: RefCell<Option<String>>,
    css_class: RefCell<String>,
    attributes: RefCell<HashMap<&'static str, String>>,
    pieces: RefCell<Vec<Piece>>,
    keybinding_piece: Cell<Option<NodeId>>,
    metrics: Cell<Metrics>,
    last_size: Cell<ItemSize>,
    last_width: Cell<f64>,
    hovered: Cell<bool>,
    focus_visible: Rc<Cell<bool>>,
    run_once_to_enable_mouse_up: Rc<RunOnceScheduler>,
    on_size_changed: RefCell<Option<Rc<dyn Fn()>>>,
    kind: Kind,
}

/// O que `new` pede além da ação e das opções.
pub struct MenuItemContext {
    pub ui: Ui,
    pub menu_style: Rc<IMenuStyles>,
    /// `:focus-visible`: o menu foi usado pelo teclado (compartilhado por todos os itens do menu).
    pub focus_visible: Rc<Cell<bool>>,
    /// O movimento do ponteiro no menu (`movementX/Y`).
    pub motion: Rc<PointerMotion>,
}

/// O que `SubmenuMenuActionViewItem` pede a mais.
pub struct SubmenuInit {
    pub submenu_actions: Vec<ActionRef>,
    pub parent_data: Rc<ParentData>,
    pub submenu_options: IMenuOptions,
}

impl MenuActionViewItem {
    /// `new BaseMenuActionViewItem(ctx, action, options, menuStyle)`.
    pub fn new_action(ctx: MenuItemContext, context: Option<Dynamic>, action: ActionRef, options: IMenuItemOptions) -> Rc<Self> {
        Self::build(ctx, context, action, options, None)
    }

    /// `new SubmenuMenuActionViewItem(action, submenuActions, parentData, submenuOptions, menuStyles)`.
    pub fn new_submenu(ctx: MenuItemContext, action: ActionRef, options: IMenuItemOptions, init: SubmenuInit) -> Rc<Self> {
        Self::build(ctx, None, action, options, Some(init))
    }

    fn build(ctx: MenuItemContext, context: Option<Dynamic>, action: ActionRef, mut options: IMenuItemOptions, submenu: Option<SubmenuInit>) -> Rc<Self> {
        options.base.is_menu = true;
        options.base.icon = Some(options.base.icon.unwrap_or(false));
        options.base.label = Some(options.base.label.unwrap_or(true));
        let ui = ctx.ui.clone();
        let displayed = je_base::actions::get_action_for_alt_mode(&action, ModifierKeyEmitter::get_instance(&ui).key_status().alt_shift(), &crate::action_view_items::host_platform());
        let item = Rc::new_cyclic(|weak: &Weak<Self>| {
            let mnemonic = if options.base.label == Some(true) && options.enable_mnemonics {
                let label = displayed.label();
                (!label.is_empty()).then(|| super::mnemonics::mnemonic_of(&label)).flatten()
            } else {
                None
            };
            // Registra o ouvinte de `mouseup` só depois de 100ms, para evitar cliques acidentais.
            let run_once_to_enable_mouse_up = RunOnceScheduler::new(
                {
                    let weak = weak.clone();
                    move || {
                        if let Some(this) = weak.upgrade() {
                            this.enable_mouse_up();
                        }
                    }
                },
                100,
            );
            let kind = match submenu {
                None => Kind::Action,
                Some(init) => {
                    let show_scheduler = RunOnceScheduler::new(
                        {
                            let weak = weak.clone();
                            move || {
                                if let Some(this) = weak.upgrade()
                                    && let Kind::Submenu(state) = &this.kind
                                    && state.mouse_over.get()
                                {
                                    this.cleanup_existing_submenu(false);
                                    this.create_submenu(false);
                                }
                            }
                        },
                        250,
                    );
                    let hide_scheduler = RunOnceScheduler::new(
                        {
                            let weak = weak.clone();
                            move || {
                                if let Some(this) = weak.upgrade() {
                                    this.hide_submenu_if_unfocused();
                                }
                            }
                        },
                        750,
                    );
                    let expand_direction = init.submenu_options.expand_direction.unwrap_or(IMenuDirection { horizontal: HorizontalDirection::Right, vertical: VerticalDirection::Below });
                    Kind::Submenu(Box::new(SubmenuState {
                        submenu_actions: init.submenu_actions,
                        parent_data: init.parent_data,
                        submenu_options: init.submenu_options,
                        mysubmenu: RefCell::new(None),
                        submenu_container: Cell::new(None),
                        submenu_disposables: DisposableStore::new(),
                        mouse_over: Cell::new(false),
                        show_scheduler,
                        hide_scheduler,
                        expand_direction,
                        motion: ctx.motion.clone(),
                    }))
                }
            };
            Self {
                base: BaseActionViewItem::new_derived(&ui, context, action, options.base.clone(), weak.clone() as Weak<dyn IActionViewItem>),
                weak_self: weak.clone(),
                options,
                menu_style: ctx.menu_style,
                ui: ui.clone(),
                displayed_action: RefCell::new(displayed),
                nodes: Cell::new(None),
                container: Cell::new(None),
                mnemonic: RefCell::new(mnemonic),
                css_class: RefCell::new(String::new()),
                attributes: RefCell::new(HashMap::new()),
                pieces: RefCell::new(Vec::new()),
                keybinding_piece: Cell::new(None),
                metrics: Cell::new(Metrics::default()),
                last_size: Cell::new((0.0, 0.0)),
                last_width: Cell::new(0.0),
                hovered: Cell::new(false),
                focus_visible: ctx.focus_visible,
                run_once_to_enable_mouse_up,
                on_size_changed: RefCell::new(None),
                kind,
            }
        });
        item.base.register(item.run_once_to_enable_mouse_up.clone());
        item
    }

    /// O `container` recebido em `render` (o `li`).
    pub fn container(&self) -> Option<NodeId> {
        self.container.get()
    }

    /// O `a.action-menu-item`.
    pub fn item_node(&self) -> Option<NodeId> {
        self.nodes.get().map(|n| n.item)
    }

    /// O `span.action-label`.
    pub fn label_node(&self) -> Option<NodeId> {
        self.nodes.get().map(|n| n.label)
    }

    /// O `span.keybinding`.
    pub fn keybinding_node(&self) -> Option<NodeId> {
        self.nodes.get().and_then(|n| n.keybinding)
    }

    /// O `span.menu-item-check`.
    pub fn check_node(&self) -> Option<NodeId> {
        self.nodes.get().map(|n| n.check)
    }

    pub fn is_submenu(&self) -> bool {
        matches!(self.kind, Kind::Submenu(_))
    }

    /// O submenu aberto por este item (`mysubmenu`), se ele é de submenu e está aberto.
    pub fn submenu(&self) -> Option<Rc<Menu>> {
        self.state().and_then(|state| state.mysubmenu.borrow().clone())
    }

    /// Um atributo ARIA do `a.action-menu-item` que o `je-ui` não modela (`aria-keyshortcuts`...).
    pub fn attribute(&self, name: &str) -> Option<String> {
        self.attributes.borrow().get(name).cloned()
    }

    fn set_attribute(&self, name: &'static str, value: impl Into<String>) {
        self.attributes.borrow_mut().insert(name, value.into());
    }

    fn remove_attribute(&self, name: &'static str) {
        self.attributes.borrow_mut().remove(name);
    }

    /// `getMnemonic()`.
    pub fn get_mnemonic(&self) -> Option<String> {
        self.mnemonic.borrow().clone()
    }

    /// Avisa o menu de que o tamanho natural do item mudou (o `ModifierKeyEmitter` troca o rótulo).
    pub fn set_on_size_changed(&self, on_size_changed: Option<Rc<dyn Fn()>>) {
        *self.on_size_changed.borrow_mut() = on_size_changed;
    }

    fn state(&self) -> Option<&SubmenuState> {
        match &self.kind {
            Kind::Submenu(state) => Some(state),
            Kind::Action => None,
        }
    }

    /// O `RunOnceScheduler` de 100ms do construtor: liga o `mouseup` que roda a ação.
    fn enable_mouse_up(&self) {
        let Some(element) = self.base.element() else { return };
        let weak = self.weak_self.clone();
        self.base.register(self.ui.listen(element, EventKind::PointerUp, false, move |e| {
            // Sem `preventDefault` do original (conflita com o `BaseActionViewItem`, #101537); só para a
            // propagação, como `EventHelper.stop(e, true)`.
            e.stop(true);
            // Num `setTimeout(0)` para o cancelamento de um menu de contexto disparar antes.
            let mut copy = Event::new(e.kind, e.target);
            copy.pointer = e.pointer;
            copy.key = e.key;
            let weak = weak.clone();
            runtime::set_timeout(
                move || {
                    if let Some(this) = weak.upgrade() {
                        this.on_click(&mut copy);
                    }
                },
                0,
            );
        }));
    }

    /// `onClick(e)`: o do base roda a ação; o do submenu abre o submenu.
    pub fn on_click(&self, e: &mut Event) {
        match &self.kind {
            Kind::Action => self.base.on_click(e, false),
            Kind::Submenu(_) => {
                // Impede o clique de tentar rodar uma ação.
                e.stop(true);
                self.cleanup_existing_submenu(false);
                self.create_submenu(true);
            }
        }
    }

    /// `updatePositionInSet(pos, setSize)`.
    pub fn update_position_in_set(&self, pos: usize, set_size: usize) {
        if self.nodes.get().is_some() {
            self.set_attribute("aria-posinset", pos.to_string());
            self.set_attribute("aria-setsize", set_size.to_string());
        }
    }

    /// `updateAlternativeAction()`: troca a ação mostrada se a alternativa passou a valer.
    pub fn update_alternative_action(&self) {
        let modifiers = ModifierKeyEmitter::get_instance(&self.ui).key_status().alt_shift();
        let action = je_base::actions::get_action_for_alt_mode(&self.base.action(), modifiers, &crate::action_view_items::host_platform());
        let changed = !Rc::ptr_eq(&action, &self.displayed_action.borrow());
        if changed {
            *self.displayed_action.borrow_mut() = action;
            self.update_label();
            self.update_class();
            self.update_enabled();
            self.update_checked();
        }
    }

    fn font() -> je_text::TextStyle {
        ui_font(MENU_FONT_SIZE, 400)
    }

    fn line_height() -> f64 {
        f64::from(MENU_FONT_SIZE)
    }

    /// Um nó de texto de uma linha, na fonte do menu.
    fn text_node(&self, parent: NodeId, class: &str, text: &str, color: Rgba) -> (NodeId, f64) {
        let font = Self::font();
        let mut tree = self.ui.tree_mut();
        let node = create_el(&mut tree, parent, Role::Generic, class);
        let width = crate::text::set_node_text(&mut tree, node, text, &font, color, Self::line_height());
        (node, width)
    }

    /// O texto do atalho do item: `getKeyBinding(action)?.getLabel() ?? options.keybinding`.
    fn keybinding_label(&self) -> Option<String> {
        let action = self.action();
        self.options.get_key_binding.as_ref().and_then(|get| get(&action)).and_then(|kb| kb.get_label()).or_else(|| self.options.base.keybinding.clone())
    }

    /// `relayout`: o tamanho natural de um item (o do `a`, com a margem de 4px de cada lado).
    fn relayout(&self) {
        let metrics = self.metrics.get();
        let mut width = MENU_ITEM_MARGIN * 2.0 + metrics.label_text + 2.0 * MENU_LABEL_PADDING;
        if metrics.keybinding_visible {
            width += metrics.keybinding_text + 2.0 * MENU_LABEL_PADDING;
        }
        if self.is_submenu() {
            width += 2.0 * MENU_SUBMENU_INDICATOR_PADDING;
        }
        let size = (width, MENU_ITEM_HEIGHT);
        if size != self.last_size.replace(size) {
            self.base.set_size(size.0, size.1);
            let notify = self.on_size_changed.borrow().clone();
            if let Some(notify) = notify {
                notify();
            }
        }
        self.layout_inner(self.last_width.get());
    }

    /// Posiciona os nós do `a` dentro do `li` de `width` (o flex do `.action-menu-item`).
    fn layout_inner(&self, width: f64) {
        let (Some(nodes), true) = (self.nodes.get(), width > 0.0) else { return };
        self.last_width.set(width);
        let metrics = self.metrics.get();
        let inner = width - 2.0 * MENU_ITEM_MARGIN;
        let label_natural = metrics.label_text + 2.0 * MENU_LABEL_PADDING;
        // Os itens flex em ordem, com `flex-grow` (rótulo 1, atalho e indicador 2).
        let mut flex: Vec<(&'static str, f64, f64)> = vec![("label", label_natural, 1.0)];
        if metrics.keybinding_visible {
            flex.push(("keybinding", metrics.keybinding_text + 2.0 * MENU_LABEL_PADDING, 2.0));
        }
        if nodes.submenu_indicator.is_some() {
            flex.push(("indicator", 2.0 * MENU_SUBMENU_INDICATOR_PADDING, 2.0));
        }
        let natural: f64 = flex.iter().map(|f| f.1).sum();
        let grow: f64 = flex.iter().map(|f| f.2).sum();
        let extra = (inner - natural).max(0.0);
        let text_top = (MENU_ITEM_HEIGHT - Self::line_height()) / 2.0;
        let mut x = 0.0;
        let mut tree = self.ui.tree_mut();
        tree.set_bounds(nodes.item, Rect::new(MENU_ITEM_MARGIN, 0.0, MENU_ITEM_MARGIN + inner, MENU_ITEM_HEIGHT));
        tree.set_bounds(nodes.check, Rect::new(0.0, 0.0, MENU_LABEL_PADDING, MENU_ITEM_HEIGHT));
        for (name, natural, grow_factor) in flex {
            let w = natural + extra * grow_factor / grow;
            match name {
                "label" => {
                    tree.set_bounds(nodes.label, Rect::new(x, text_top, x + w, text_top + Self::line_height()));
                    let mut piece_x = MENU_LABEL_PADDING;
                    for piece in self.pieces.borrow().iter() {
                        tree.set_bounds(piece.node, Rect::new(piece_x, 0.0, piece_x + piece.width, Self::line_height()));
                        if let Some(underline) = piece.underline {
                            tree.set_bounds(underline, Rect::new(0.0, Self::line_height() - 1.0, piece.width, Self::line_height()));
                        }
                        piece_x += piece.width;
                    }
                }
                "keybinding" => {
                    if let Some(node) = nodes.keybinding {
                        tree.set_bounds(node, Rect::new(x, text_top, x + w, text_top + Self::line_height()));
                        if let Some(text) = self.keybinding_piece.get() {
                            // `text-align: right` com `padding: 0 2em`.
                            let tx = w - MENU_LABEL_PADDING - metrics.keybinding_text;
                            tree.set_bounds(text, Rect::new(tx, 0.0, tx + metrics.keybinding_text, Self::line_height()));
                        }
                    }
                }
                _ => {
                    if let Some(node) = nodes.submenu_indicator {
                        tree.set_bounds(node, Rect::new(x, 0.0, x + w, MENU_ITEM_HEIGHT));
                        // O `::before` de 16px (`margin-left: auto; margin-right: -20px`) fica no fim do conteúdo,
                        // depois do `padding: 0 1.8em`: a borda direita dele fica 20px depois do fim do conteúdo.
                        if let Some(glyph) = tree.children(node).first().copied() {
                            let left = w - MENU_SUBMENU_INDICATOR_PADDING + 4.0;
                            tree.set_bounds(glyph, Rect::new(left, (MENU_ITEM_HEIGHT - MENU_SUBMENU_INDICATOR_SIZE) / 2.0, left + MENU_SUBMENU_INDICATOR_SIZE, (MENU_ITEM_HEIGHT + MENU_SUBMENU_INDICATOR_SIZE) / 2.0));
                        }
                    }
                }
            }
            x += w;
        }
        drop(tree);
        self.apply_style();
    }

    /// `applyStyle()` mais as regras de CSS do `.action-menu-item` e dos filhos.
    pub fn apply_style(&self) {
        let (Some(nodes), Some(element)) = (self.nodes.get(), self.base.element()) else { return };
        let style = &*self.menu_style;
        let mut tree = self.ui.tree_mut();
        let selected = tree.has_class(element, "focused");
        let disabled = tree.has_class(element, "disabled");
        let fg = if selected { style.selection_foreground_color.or(style.foreground_color) } else { style.foreground_color };
        let bg = if selected { style.selection_background_color } else { None };
        // `.action-menu-item:focus:not(:focus-visible) { outline: none !important }`.
        let focused_dom = self.ui.focused() == Some(nodes.item);
        let outline = (selected && (self.focus_visible.get() || !focused_dom)).then_some(style.selection_border_color).flatten();
        let text_color = fg.unwrap_or(MENU_FALLBACK_FOREGROUND);
        let label_color = if disabled { style.disabled_foreground.unwrap_or(text_color) } else { text_color };
        let secondary_opacity = if disabled {
            MENU_DISABLED_SECONDARY_OPACITY
        } else if self.hovered.get() || focused_dom {
            1.0
        } else {
            MENU_SECONDARY_OPACITY
        };
        let item_style = tree.style_mut(nodes.item);
        item_style.background = bg;
        item_style.border = outline.map(|color| Border { width: 1.0, color });
        item_style.radius = MENU_CORNER_RADIUS_MEDIUM;
        item_style.cursor = Some(if disabled { Cursor::Default } else { Cursor::Pointer });
        if let Content::Codicon { color, .. } = &mut tree.style_mut(nodes.check).content {
            *color = text_color;
        }
        for piece in self.pieces.borrow().iter() {
            if let Content::Text { color, .. } = &mut tree.style_mut(piece.node).content {
                *color = label_color;
            }
            if let Some(underline) = piece.underline {
                tree.style_mut(underline).background = Some(label_color);
            }
        }
        if let Some(text) = self.keybinding_piece.get() {
            if let Content::Text { color, .. } = &mut tree.style_mut(text).content {
                *color = text_color;
            }
            if let Some(node) = nodes.keybinding {
                tree.style_mut(node).opacity = secondary_opacity;
            }
        }
        if let Some(node) = nodes.submenu_indicator {
            // `submenuIndicator.style.color = fgColor ?? ''` (o `applyStyle` do submenu).
            if let Some(glyph) = tree.children(node).first().copied()
                && let Content::Codicon { color, .. } = &mut tree.style_mut(glyph).content
            {
                *color = text_color;
            }
            tree.style_mut(node).opacity = if disabled { MENU_DISABLED_SECONDARY_OPACITY } else { MENU_SECONDARY_OPACITY };
        }
    }

    /// Refaz os nós de texto do rótulo.
    fn rebuild_label(&self, pieces: &LabelPieces) {
        let Some(nodes) = self.nodes.get() else { return };
        for piece in self.pieces.borrow_mut().drain(..) {
            self.ui.remove(piece.node);
        }
        let mut made = Vec::new();
        let parts: Vec<(&str, bool)> = match pieces {
            LabelPieces::Plain(text) => vec![(text.as_str(), false)],
            LabelPieces::Mnemonic { before, letter, after } => vec![(before.as_str(), false), (letter.as_str(), true), (after.as_str(), false)],
        };
        for (text, underlined) in parts {
            if text.is_empty() {
                continue;
            }
            let (node, width) = self.text_node(nodes.label, if underlined { "u" } else { "action-label-text" }, text, MENU_FALLBACK_FOREGROUND);
            let underline = underlined.then(|| create_el(&mut self.ui.tree_mut(), node, Role::Generic, "underline"));
            made.push(Piece { node, width, underline });
        }
        *self.pieces.borrow_mut() = made;
    }
}

impl IActionViewItem for MenuActionViewItem {
    fn base(&self) -> &BaseActionViewItem {
        &self.base
    }

    /// `get action()`: a ação mostrada (a alternativa, com Alt).
    fn action(&self) -> ActionRef {
        self.displayed_action.borrow().clone()
    }

    fn render(&self, container: NodeId) {
        self.base.base_render(container);
        self.container.set(Some(container));
        let action = self.action();
        let is_separator = action.id() == je_base::actions::Separator::ID;
        let has_keybinding = self.options.base.label == Some(true) && (self.options.get_key_binding.is_some() || self.options.base.keybinding.is_some());
        let nodes = {
            let mut tree = self.ui.tree_mut();
            let item = create_el(&mut tree, container, if is_separator { Role::Presentation } else { Role::MenuItem }, "action-menu-item");
            let check = create_el(&mut tree, item, Role::Presentation, "menu-item-check codicon codicon-menu-selection");
            tree.style_mut(check).content = Content::Codicon { id: "menu-selection".to_owned(), size: MENU_FONT_SIZE, color: MENU_FALLBACK_FOREGROUND };
            tree.style_mut(check).visible = false;
            let label = create_el(&mut tree, item, Role::Generic, "action-label");
            let keybinding = has_keybinding.then(|| create_el(&mut tree, item, Role::Generic, "keybinding"));
            let submenu_indicator = self.is_submenu().then(|| {
                let indicator = create_el(&mut tree, item, Role::Presentation, "submenu-indicator codicon codicon-menu-submenu");
                let glyph = tree.create(indicator, Role::Generic);
                tree.style_mut(glyph).content = Content::Codicon { id: "menu-submenu".to_owned(), size: MENU_SUBMENU_INDICATOR_SIZE as f32, color: MENU_FALLBACK_FOREGROUND };
                if self.is_submenu() {
                    tree.add_class(item, "monaco-submenu-item");
                }
                indicator
            });
            ItemNodes { item, check, label, keybinding, submenu_indicator }
        };
        self.nodes.set(Some(nodes));
        if !is_separator && let Some(mnemonic) = self.mnemonic.borrow().clone() {
            self.set_attribute("aria-keyshortcuts", mnemonic);
        }
        if self.is_submenu() {
            self.set_attribute("aria-haspopup", "true");
            self.update_aria_expanded("false");
        }

        for kind in [EventKind::PointerEnter, EventKind::PointerLeave] {
            let weak = self.weak_self.clone();
            self.base.register(self.ui.listen(nodes.item, kind, false, move |e| {
                if let Some(this) = weak.upgrade() {
                    this.hovered.set(e.kind == EventKind::PointerEnter);
                    this.apply_style();
                }
            }));
        }
        for kind in [EventKind::Focus, EventKind::Blur] {
            let weak = self.weak_self.clone();
            self.base.register(self.ui.listen(nodes.item, kind, false, move |_| {
                if let Some(this) = weak.upgrade() {
                    this.apply_style();
                }
            }));
        }
        // Adiciona o ouvinte de `mouseup` depois, para evitar cliques acidentais.
        self.run_once_to_enable_mouse_up.schedule(None);

        self.update_class();
        self.update_label();
        // `updateTooltip()`: menus funcionam como os nativos e não têm dica.
        self.update_enabled();
        self.update_checked();
        self.apply_style();

        if self.is_submenu() {
            self.render_submenu_listeners(container);
        }
    }

    fn blur(&self) {
        self.base.base_blur();
        self.apply_style();
    }

    fn focus(&self, _from_right: bool) {
        self.base.base_focus();
        if let Some(nodes) = self.nodes.get() {
            self.ui.focus(nodes.item);
        }
        self.apply_style();
    }

    fn update_label(&self) {
        let Some(nodes) = self.nodes.get() else { return };
        if self.options.base.label != Some(true) {
            return;
        }
        self.metrics.set(Metrics::default());
        *self.mnemonic.borrow_mut() = None;
        self.remove_attribute("aria-keyshortcuts");
        self.ui.tree_mut().clear_label(nodes.label);
        let mut metrics = Metrics::default();
        if let Some(keybinding_node) = nodes.keybinding {
            let keybinding = self.keybinding_label();
            if let Some(old) = self.keybinding_piece.take() {
                self.ui.remove(old);
            }
            let text = keybinding.clone().unwrap_or_default();
            if !text.is_empty() {
                let (node, width) = self.text_node(keybinding_node, "keybinding-text", &text, MENU_FALLBACK_FOREGROUND);
                self.keybinding_piece.set(Some(node));
                metrics.keybinding_text = width;
            }
            // `this._keybinding.hidden = !keybinding`
            metrics.keybinding_visible = keybinding.is_some_and(|k| !k.is_empty());
            self.ui.tree_mut().style_mut(keybinding_node).visible = metrics.keybinding_visible;
        }
        let stripped = String::from_utf16_lossy(strip_icons(&self.action().label().encode_utf16().collect::<Vec<_>>()).units());
        let pieces = if stripped.is_empty() {
            LabelPieces::Plain(String::new())
        } else {
            let (aria_label, pieces, mnemonic) = menu_item_label(&stripped, self.options.enable_mnemonics);
            self.ui.tree_mut().set_label(nodes.label, aria_label);
            if let Some(mnemonic) = mnemonic {
                self.set_attribute("aria-keyshortcuts", mnemonic.clone());
                *self.mnemonic.borrow_mut() = Some(mnemonic);
            }
            pieces
        };
        self.rebuild_label(&pieces);
        metrics.label_text = self.pieces.borrow().iter().map(|p| p.width).sum();
        self.metrics.set(metrics);
        self.relayout();
    }

    fn update_class(&self) {
        let Some(nodes) = self.nodes.get() else { return };
        let old = self.css_class.borrow().clone();
        {
            let mut tree = self.ui.tree_mut();
            for class in old.split_whitespace() {
                tree.remove_class(nodes.label, class);
            }
        }
        if self.options.base.icon == Some(true) {
            let class = self.action().class().unwrap_or_default();
            {
                let mut tree = self.ui.tree_mut();
                tree.add_class(nodes.label, "icon");
                for c in class.split_whitespace() {
                    tree.add_class(nodes.label, c);
                }
            }
            *self.css_class.borrow_mut() = class;
            self.update_enabled();
        } else {
            *self.css_class.borrow_mut() = String::new();
            self.ui.tree_mut().remove_class(nodes.label, "icon");
        }
    }

    /// `updateEnabled()`; o item de submenu não observa a habilitação (como os menus nativos).
    fn update_enabled(&self) {
        if self.is_submenu() {
            return;
        }
        let (Some(nodes), Some(element)) = (self.nodes.get(), self.base.element()) else { return };
        let enabled = self.action().enabled();
        {
            let mut tree = self.ui.tree_mut();
            tree.toggle_class(element, "disabled", !enabled);
            tree.toggle_class(nodes.item, "disabled", !enabled);
            if enabled {
                tree.style_mut(nodes.item).cursor = Some(Cursor::Pointer);
            }
        }
        if enabled {
            self.remove_attribute("aria-disabled");
        } else {
            self.set_attribute("aria-disabled", "true");
        }
        self.apply_style();
    }

    fn update_checked(&self) {
        let Some(nodes) = self.nodes.get() else { return };
        let checked = self.action().checked();
        {
            let mut tree = self.ui.tree_mut();
            tree.toggle_class(nodes.item, "checked", checked == Some(true));
            tree.style_mut(nodes.check).visible = checked == Some(true);
            tree.set_role(nodes.item, if checked.is_some() { Role::MenuItemCheckbox } else { Role::MenuItem });
        }
        match checked {
            Some(checked) => self.set_attribute("aria-checked", if checked { "true" } else { "false" }),
            None => self.set_attribute("aria-checked", ""),
        }
    }

    fn get_tooltip(&self) -> Option<String> {
        None
    }

    fn update_aria_label(&self) {}

    fn size(&self, _vertical: bool) -> ItemSize {
        self.last_size.get()
    }

    fn layout(&self, _vertical: bool, width: f64, _height: f64) {
        self.layout_inner(width);
    }
}

// ---------------------------------------------------------------------------------------------
// SubmenuMenuActionViewItem
// ---------------------------------------------------------------------------------------------

impl MenuActionViewItem {
    fn update_aria_expanded(&self, value: &'static str) {
        self.set_attribute("aria-expanded", value);
    }

    fn submenu_state(&self) -> &SubmenuState {
        self.state().expect("só o item de submenu abre submenus")
    }

    /// Os ouvintes que `SubmenuMenuActionViewItem.render` registra no `li`.
    fn render_submenu_listeners(&self, element: NodeId) {
        let state = self.submenu_state();
        let weak = self.weak_self.clone();
        self.base.register(self.ui.listen(element, EventKind::KeyUp, false, {
            let weak = weak.clone();
            move |e| {
                let Some(event) = StandardKeyboardEvent::from_event(e) else { return };
                if (event.equals(KeyCode::RightArrow.0 as u32) || event.equals(KeyCode::Enter.0 as u32))
                    && let Some(this) = weak.upgrade()
                {
                    e.stop(true);
                    this.create_submenu(true);
                }
            }
        }));
        self.base.register(self.ui.listen(element, EventKind::KeyDown, false, {
            let weak = weak.clone();
            move |e| {
                let Some(event) = StandardKeyboardEvent::from_event(e) else { return };
                if let Some(this) = weak.upgrade()
                    && this.ui.focused() == this.item_node()
                    && (event.equals(KeyCode::RightArrow.0 as u32) || event.equals(KeyCode::Enter.0 as u32))
                {
                    e.stop(true);
                }
            }
        }));
        self.base.register(self.ui.listen(element, EventKind::PointerMove, false, {
            let weak = weak.clone();
            move |_| {
                let Some(this) = weak.upgrade() else { return };
                // `movementX/Y` zero: o ponteiro parado sob o menu não abre o submenu (#110594, #148158).
                let state = this.submenu_state();
                if !state.motion.moved.get() {
                    return;
                }
                if !state.mouse_over.get() {
                    state.mouse_over.set(true);
                    state.show_scheduler.schedule(None);
                }
            }
        }));
        self.base.register(self.ui.listen(element, EventKind::PointerLeave, false, {
            let weak = weak.clone();
            move |_| {
                if let Some(this) = weak.upgrade() {
                    this.submenu_state().mouse_over.set(false);
                }
            }
        }));
        self.base.register(self.ui.listen(element, EventKind::Blur, true, {
            let weak = weak.clone();
            move |_| {
                if let Some(this) = weak.upgrade()
                    && let Some(element) = this.base.element()
                    && !this.ui.focused().is_some_and(|f| this.ui.tree().is_ancestor(element, f))
                {
                    this.submenu_state().hide_scheduler.schedule(None);
                }
            }
        }));
        if let Some(parent) = state.parent_data.parent.upgrade() {
            self.base.register(parent.on_scroll().listen({
                let weak = weak.clone();
                move |e| {
                    let Some(this) = weak.upgrade() else { return };
                    let state = this.submenu_state();
                    let same = match (&*state.parent_data.submenu.borrow(), &*state.mysubmenu.borrow()) {
                        (Some(a), Some(b)) => Rc::ptr_eq(a, b),
                        (None, None) => true,
                        _ => false,
                    };
                    if same {
                        if e.scroll_top_changed || e.scroll_left_changed {
                            if let Some(parent) = state.parent_data.parent.upgrade() {
                                parent.focus(false);
                            }
                            this.cleanup_existing_submenu(true);
                        } else {
                            this.layout_submenu();
                        }
                    }
                }
            }));
        }
    }

    /// O `hideScheduler`: o foco saiu do item e o submenu aberto é o dele.
    fn hide_submenu_if_unfocused(&self) {
        let state = self.submenu_state();
        let (Some(element), true) = (self.base.element(), true) else { return };
        let focus_inside = self.ui.focused().is_some_and(|f| self.ui.tree().is_ancestor(element, f));
        let same = match (&*state.parent_data.submenu.borrow(), &*state.mysubmenu.borrow()) {
            (Some(a), Some(b)) => Rc::ptr_eq(a, b),
            (None, None) => true,
            _ => false,
        };
        if !focus_inside && same {
            if let Some(parent) = state.parent_data.parent.upgrade() {
                parent.focus(false);
            }
            self.cleanup_existing_submenu(true);
        }
    }

    /// `open(selectFirst)`.
    pub fn open(&self, select_first: bool) {
        self.cleanup_existing_submenu(false);
        self.create_submenu(select_first);
    }

    fn cleanup_existing_submenu(&self, force: bool) {
        let state = self.submenu_state();
        let existing = state.parent_data.submenu.borrow().clone();
        let Some(existing) = existing else { return };
        let is_mine = state.mysubmenu.borrow().as_ref().is_some_and(|m| Rc::ptr_eq(m, &existing));
        if force || !is_mine {
            // O `dispose` pode lançar se o submenu já foi removido.
            let _ = je_base::errors::catch(|| existing.dispose());
            *state.parent_data.submenu.borrow_mut() = None;
            self.update_aria_expanded("false");
            if let Some(container) = state.submenu_container.take() {
                state.submenu_disposables.clear();
                self.ui.remove(container);
            }
        }
    }

    /// `calculateSubmenuMenuLayout`: `(top, left)` na janela. `entry` é `(left, top, width, height)`.
    fn calculate_submenu_layout(window: (f64, f64), submenu: (f64, f64), mut entry: (f64, f64, f64, f64), expand: IMenuDirection) -> (f64, f64) {
        let (entry_left, mut entry_top, entry_width, mut entry_height) = (entry.0, entry.1, entry.2, entry.3);
        let anchor_position = if expand.horizontal == HorizontalDirection::Right { LayoutAnchorPosition::Before } else { LayoutAnchorPosition::After };
        let mut left = layout_1d(window.0, submenu.0, ILayoutAnchor { position: anchor_position, offset: entry_left, size: entry_width, mode: None }).position;
        // Sem espaço para o menu inteiro: sobrepõe.
        if left >= entry_left && left < entry_left + entry_width {
            if entry_left + 10.0 + submenu.0 <= window.0 {
                left = entry_left + 10.0;
            }
            entry_top += 10.0;
            entry_height = 0.0;
        }
        entry = (entry_left, entry_top, entry_width, entry_height);
        // Com a posição horizontal, a vertical.
        let mut top = layout_1d(window.1, submenu.1, ILayoutAnchor { position: LayoutAnchorPosition::Before, offset: entry.1, size: 0.0, mode: None }).position;
        // Não coube embaixo mas coube em cima: desce para alinhar o menu.
        if top + submenu.1 == entry.1 && top + entry.3 + submenu.1 <= window.1 {
            top += entry.3;
        }
        (top, left)
    }

    fn create_submenu(&self, select_first_item: bool) {
        let Some(element) = self.base.element() else { return };
        let state = self.submenu_state();
        if state.parent_data.submenu.borrow().is_some() {
            if let Some(submenu) = state.parent_data.submenu.borrow().clone() {
                submenu.focus(false);
            }
            return;
        }
        self.update_aria_expanded("true");
        let container = {
            let mut tree = self.ui.tree_mut();
            let container = create_el(&mut tree, element, Role::Presentation, "monaco-submenu menubar-menu-items-holder context-view");
            // `position: fixed` com `z-index: 1`.
            tree.style_mut(container).z_index = 1;
            container
        };
        state.submenu_container.set(Some(container));
        let actions: Vec<ActionRef> = if state.submenu_actions.is_empty() {
            // O `new EmptySubmenuAction()` do original nunca é descartado; aqui o submenu o descarta ao fechar.
            vec![state.submenu_disposables.add_rc(EmptySubmenuAction::create()) as ActionRef]
        } else {
            state.submenu_actions.clone()
        };
        let submenu = Menu::new_sharing_modality(&self.ui, container, &actions, state.submenu_options.clone(), self.menu_style.clone(), self.focus_visible.clone());
        *state.parent_data.submenu.borrow_mut() = Some(submenu.clone());
        self.layout_submenu();

        let weak = self.weak_self.clone();
        state.submenu_disposables.add(self.ui.listen(container, EventKind::KeyUp, false, {
            let weak = weak.clone();
            move |e| {
                let Some(event) = StandardKeyboardEvent::from_event(e) else { return };
                if event.equals(KeyCode::LeftArrow.0 as u32)
                    && let Some(this) = weak.upgrade()
                {
                    e.stop(true);
                    if let Some(parent) = this.submenu_state().parent_data.parent.upgrade() {
                        parent.focus(true);
                    }
                    this.cleanup_existing_submenu(true);
                }
            }
        }));
        state.submenu_disposables.add(self.ui.listen(container, EventKind::KeyDown, false, move |e| {
            if StandardKeyboardEvent::from_event(e).is_some_and(|event| event.equals(KeyCode::LeftArrow.0 as u32)) {
                e.stop(true);
            }
        }));
        state.submenu_disposables.add(submenu.on_did_cancel().listen({
            let weak = weak.clone();
            move |_| {
                if let Some(this) = weak.upgrade() {
                    if let Some(parent) = this.submenu_state().parent_data.parent.upgrade() {
                        parent.focus(true);
                    }
                    this.cleanup_existing_submenu(true);
                }
            }
        }));
        submenu.focus(select_first_item);
        *state.mysubmenu.borrow_mut() = Some(submenu);
    }

    /// `_layoutSubmenu`: posiciona o contêiner do submenu ao lado do item, na janela.
    fn layout_submenu(&self) {
        let (Some(element), Some(container)) = (self.base.element(), self.submenu_state().submenu_container.get()) else { return };
        let state = self.submenu_state();
        let window = window_size(&self.ui);
        let (entry, size) = {
            let tree = self.ui.tree();
            let abs = tree.absolute_bounds(element);
            let view = tree.bounds(container);
            // O `paddingTop` da barra de ações do menu pai.
            let padding_top = MENU_VERTICAL_PADDING;
            ((abs.x0, abs.y0 - padding_top, abs.width(), abs.height() + 2.0 * padding_top), (view.width(), view.height()))
        };
        let (top, left) = Self::calculate_submenu_layout(window, size, entry, state.expand_direction);
        let origin = self.ui.tree().absolute_bounds(element);
        self.ui.tree_mut().set_bounds(container, Rect::new(left - origin.x0, top - origin.y0, left - origin.x0 + size.0, top - origin.y0 + size.1));
    }
}

impl IDisposable for MenuActionViewItem {
    fn dispose(&self) {
        self.base.base_dispose();
        self.run_once_to_enable_mouse_up.dispose();
        if let Kind::Submenu(state) = &self.kind {
            state.hide_scheduler.dispose();
            state.show_scheduler.dispose();
            state.submenu_disposables.dispose();
            if let Some(submenu) = state.mysubmenu.take() {
                submenu.dispose();
            }
            state.submenu_container.set(None);
        }
    }
}
