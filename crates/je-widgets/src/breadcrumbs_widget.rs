//! Porte de `src/vs/base/browser/ui/breadcrumbs/breadcrumbsWidget.ts` e `breadcrumbsWidget.css`: o
//! [`BreadcrumbsWidget`], a trilha de itens separados por um ícone (`chevron-right`) com foco, seleção,
//! rolagem horizontal e revelação do item.
//!
//! Do DOM e do CSS para o `je-ui`:
//!
//! - `div.monaco-breadcrumbs` (role `list`) é o nó dono dos itens, dentro do nó do elemento rolável
//!   ([`AbstractScrollableElement::scrollable_element`], o `DomScrollableElement` do original); cada item é um
//!   `div.monaco-breadcrumb-item` (role `listitem`) com o que `BreadcrumbsItem::render` pôs dentro e, no fim, o
//!   ícone separador (`span.codicon.codicon-<id>`).
//! - O flex do CSS (`display: flex; flex-direction: row; flex-wrap: nowrap`, item com `align-items: center`,
//!   `height: 100%`) é [`BreadcrumbsWidget::relayout`]: os filhos de cada item (os "flex items" que o `render`
//!   criou, com tamanho próprio) entram da esquerda para a direita, centrados na vertical, o ícone separador
//!   por último (16 x 16), e os itens lado a lado. A posição de cada filho é do widget; o `render` só dá o
//!   tamanho. A rolagem (`scrollLeft`) desloca os itens; `offsetLeft` é a posição sem deslocamento. O
//!   `DomScrollableElement.scanDomNode` é [`BreadcrumbsWidget::scan_dom_node`]: lê a largura do nó e a soma
//!   das larguras dos itens como dimensões do elemento rolável.
//! - A folha de estilo dinâmica de `_style` (`createStyleSheet` + `.monaco-breadcrumbs { background-color }`, a cor
//!   do item, `.focused`, `.focused.selected` e o `:hover` fora de `.disabled`) é
//!   [`BreadcrumbsWidget::refresh_styles`]: o fundo do nó e a cor aplicada ao conteúdo do item. A cor é herdada no
//!   CSS; aqui o widget troca a cor de todo conteúdo (texto e ícone) do item que tem a cor herdada em vigor, e o
//!   `render` recebe essa cor para criar o conteúdo.
//! - `dom.measure`/`dom.modify` (o `scheduleAtNextAnimationFrame` com prioridades 10000 e -10000) usam o
//!   `Ui`; o `_measure` injetável do original é [`MeasureFn`].
//!
//! Fica de fora: o `payload` do evento de clique é só o [`je_ui::PointerData`] (o `IMouseEvent` do original).

use std::any::Any;
use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::rc::{Rc, Weak};

use je_base::arrays::common_prefix_length;
use je_base::errors::{self, Error};
use je_base::event::{Emitter, Event as BaseEvent};
use je_base::lifecycle::{Disposable, DisposableRef, DisposableStore, IDisposable, MutableDisposable};
use je_base::scrollable::{INewScrollDimensions, INewScrollPosition, ScrollbarVisibility};
use je_base::themables::ThemeIcon;
use je_render::Rgba;
use je_render::kurbo::Rect;
use je_theme::ColorTheme;
use je_ui::{Content, Cursor, EventKind, NodeId, Role, Tree, Ui};

use crate::common::registered_emitter;
use crate::dom::{FocusTracker, clear_node, create_detached_el, is_ancestor_of_active_element};
use crate::icon_labels::INLINE_ICON_SIZE;
use crate::scrollbar::scrollable_element::AbstractScrollableElement;
use crate::scrollbar::scrollable_element_options::{ScrollableElementChangeOptions, ScrollableElementCreationOptions};

/// A prioridade de `dom.measure` (precisa rodar cedo no quadro) e a de `dom.modify` (tarde).
const MEASURE_PRIORITY: i32 = 10000;
const MODIFY_PRIORITY: i32 = -10000;

/// A cor herdada de fora quando o estilo não define `breadcrumbsForeground`.
const DEFAULT_INHERITED_FOREGROUND: Rgba = Rgba::opaque(0xCC, 0xCC, 0xCC);

/// O `BreadcrumbsItem` (classe abstrata do original).
pub trait BreadcrumbsItem {
    fn dispose(&self);
    fn equals(&self, other: &dyn BreadcrumbsItem) -> bool;
    /// Cria o conteúdo do item em `container`. Cada filho direto é um "flex item": dê o tamanho (largura e
    /// altura) dele, e o widget o posiciona. `foreground` é a cor herdada do item, para o texto e os ícones.
    fn render(&self, ui: &Ui, container: NodeId, foreground: Rgba);
    /// O `instanceof` de `equals` no original.
    fn as_any(&self) -> &dyn Any;
}

pub type BreadcrumbsItemRef = Rc<dyn BreadcrumbsItem>;

/// O `IBreadcrumbsWidgetStyles`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct IBreadcrumbsWidgetStyles {
    pub breadcrumbs_background: Option<Rgba>,
    pub breadcrumbs_foreground: Option<Rgba>,
    pub breadcrumbs_hover_foreground: Option<Rgba>,
    pub breadcrumbs_focus_foreground: Option<Rgba>,
    pub breadcrumbs_focus_and_selection_foreground: Option<Rgba>,
}

/// `defaultBreadcrumbsWidgetStyles`.
pub fn default_breadcrumbs_widget_styles(theme: &ColorTheme) -> IBreadcrumbsWidgetStyles {
    IBreadcrumbsWidgetStyles {
        breadcrumbs_background: theme.get_color("breadcrumb.background"),
        breadcrumbs_foreground: theme.get_color("breadcrumb.foreground"),
        breadcrumbs_hover_foreground: theme.get_color("breadcrumb.focusForeground"),
        breadcrumbs_focus_foreground: theme.get_color("breadcrumb.focusForeground"),
        breadcrumbs_focus_and_selection_foreground: theme.get_color("breadcrumb.activeSelectionForeground"),
    }
}

/// O `type` de [`IBreadcrumbsItemEvent`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BreadcrumbsItemEventType {
    Select,
    Focus,
}

/// O `IBreadcrumbsItemEvent`. `item` e `node` são `None` quando o índice está fora (o `undefined` do original).
#[derive(Clone)]
pub struct IBreadcrumbsItemEvent {
    pub kind: BreadcrumbsItemEventType,
    pub item: Option<BreadcrumbsItemRef>,
    pub node: Option<NodeId>,
    pub payload: Option<Rc<dyn Any>>,
}

/// `typeof dom.measure`: agenda um callback para ler o layout no próximo quadro.
pub type MeasureFn = Rc<dyn Fn(Box<dyn FnOnce()>) -> DisposableRef>;

pub struct BreadcrumbsWidget {
    base: Rc<Disposable>,
    weak_self: Weak<BreadcrumbsWidget>,
    ui: Ui,
    dom_node: NodeId,
    scrollable: Rc<AbstractScrollableElement>,
    on_did_select_item: Rc<Emitter<IBreadcrumbsItemEvent>>,
    on_did_focus_item: Rc<Emitter<IBreadcrumbsItemEvent>>,
    on_did_change_focus: Rc<Emitter<bool>>,
    items: RefCell<Vec<BreadcrumbsItemRef>>,
    nodes: RefCell<Vec<NodeId>>,
    free_nodes: RefCell<Vec<NodeId>>,
    separator_icon: ThemeIcon,
    enabled: Cell<bool>,
    focused_item_idx: Cell<i64>,
    selected_item_idx: Cell<i64>,
    pending_dim_layout: RefCell<Option<DisposableRef>>,
    pending_layout: RefCell<Option<DisposableRef>>,
    pending_reveal: Rc<MutableDisposable<dyn IDisposable>>,
    dimension: Cell<Option<(f64, f64)>>,
    measure: MeasureFn,
    styles: IBreadcrumbsWidgetStyles,
    inherited_foreground: Cell<Rgba>,
    hovered: Cell<Option<NodeId>>,
    /// A cor em vigor no conteúdo de cada item (a que o `render` recebeu, ou a que o estilo trocou).
    node_colors: RefCell<HashMap<NodeId, Rgba>>,
    /// `offsetLeft` de cada item, sem o deslocamento da rolagem.
    offsets: RefCell<Vec<f64>>,
    disposed: Cell<bool>,
}

impl BreadcrumbsWidget {
    pub fn new(
        ui: &Ui,
        container: NodeId,
        horizontal_scrollbar_size: f64,
        horizontal_scrollbar_visibility: Option<ScrollbarVisibility>,
        separator_icon: ThemeIcon,
        styles: IBreadcrumbsWidgetStyles,
        measure: Option<MeasureFn>,
    ) -> Rc<Self> {
        let base = Disposable::new();
        let dom_node = {
            let mut tree = ui.tree_mut();
            let node = create_detached_el(&mut tree, Role::List, "monaco-breadcrumbs");
            tree.set_fill(node, true, false);
            tree.style_mut(node).background = styles.breadcrumbs_background;
            node
        };
        let scrollable = AbstractScrollableElement::scrollable_element(
            ui,
            dom_node,
            ScrollableElementCreationOptions {
                vertical: Some(ScrollbarVisibility::Hidden),
                horizontal: Some(horizontal_scrollbar_visibility.unwrap_or(ScrollbarVisibility::Auto)),
                horizontal_scrollbar_size: Some(horizontal_scrollbar_size),
                use_shadows: Some(false),
                scroll_y_to_x: Some(true),
                ..Default::default()
            },
        );
        base.register(scrollable.clone());
        ui.tree_mut().append_child(container, scrollable.get_dom_node());

        let focus_tracker = FocusTracker::new(ui, dom_node);
        base.register(focus_tracker.clone());
        let on_did_change_focus = registered_emitter::<bool>(&base);
        {
            let emitter = on_did_change_focus.clone();
            base.register(focus_tracker.on_did_blur().listen(move |_| emitter.fire(false)));
            let emitter = on_did_change_focus.clone();
            base.register(focus_tracker.on_did_focus().listen(move |_| emitter.fire(true)));
        }
        let pending_reveal = MutableDisposable::<dyn IDisposable>::new();
        base.register(pending_reveal.clone());
        let measure = measure.unwrap_or_else(|| {
            let ui = ui.clone();
            Rc::new(move |callback: Box<dyn FnOnce()>| ui.schedule_at_next_animation_frame(MEASURE_PRIORITY, callback))
        });

        let this = Rc::new_cyclic(|weak: &Weak<BreadcrumbsWidget>| Self {
            on_did_select_item: registered_emitter(&base),
            on_did_focus_item: registered_emitter(&base),
            on_did_change_focus,
            base,
            weak_self: weak.clone(),
            ui: ui.clone(),
            dom_node,
            scrollable,
            items: RefCell::new(Vec::new()),
            nodes: RefCell::new(Vec::new()),
            free_nodes: RefCell::new(Vec::new()),
            separator_icon,
            enabled: Cell::new(true),
            focused_item_idx: Cell::new(-1),
            selected_item_idx: Cell::new(-1),
            pending_dim_layout: RefCell::new(None),
            pending_layout: RefCell::new(None),
            pending_reveal,
            dimension: Cell::new(None),
            measure,
            styles,
            inherited_foreground: Cell::new(DEFAULT_INHERITED_FOREGROUND),
            hovered: Cell::new(None),
            node_colors: RefCell::new(HashMap::new()),
            offsets: RefCell::new(Vec::new()),
            disposed: Cell::new(false),
        });
        {
            let weak = this.weak_self.clone();
            this.base.register(ui.listen(dom_node, EventKind::Click, false, move |e| {
                if let (Some(this), Some(pointer)) = (weak.upgrade(), e.pointer) {
                    this.on_click(e.target, pointer);
                }
            }));
            let weak = this.weak_self.clone();
            this.base.register(this.scrollable.on_scroll().listen(move |_| {
                if let Some(this) = weak.upgrade() {
                    this.apply_scroll();
                }
            }));
        }
        this
    }

    /// O nó do elemento rolável: o que o dono acrescenta à árvore (`this._scrollable.getDomNode()`).
    pub fn element(&self) -> NodeId {
        self.scrollable.get_dom_node()
    }

    /// O `div.monaco-breadcrumbs`.
    pub fn dom_node(&self) -> NodeId {
        self.dom_node
    }

    /// Os nós dos itens (`_nodes`), na ordem.
    pub fn item_nodes(&self) -> Vec<NodeId> {
        self.nodes.borrow().clone()
    }

    pub fn on_did_select_item(&self) -> BaseEvent<IBreadcrumbsItemEvent> {
        self.on_did_select_item.event()
    }

    pub fn on_did_focus_item(&self) -> BaseEvent<IBreadcrumbsItemEvent> {
        self.on_did_focus_item.event()
    }

    pub fn on_did_change_focus(&self) -> BaseEvent<bool> {
        self.on_did_change_focus.event()
    }

    /// A cor que o conteúdo herda quando o estilo não define `breadcrumbsForeground` (o `color` do dono).
    pub fn set_inherited_foreground(&self, color: Rgba) {
        self.inherited_foreground.set(color);
        self.refresh_styles();
    }

    pub fn set_horizontal_scrollbar_size(&self, size: f64) {
        self.scrollable.update_options(&ScrollableElementChangeOptions { horizontal_scrollbar_size: Some(size), ..Default::default() });
    }

    pub fn set_horizontal_scrollbar_visibility(&self, visibility: ScrollbarVisibility) {
        self.scrollable.update_options(&ScrollableElementChangeOptions { horizontal: Some(visibility), ..Default::default() });
    }

    /// `layout(dim)`.
    pub fn layout(&self, dim: Option<(f64, f64)>) {
        if dim.is_some() && dim == self.dimension.get() {
            return;
        }
        if let Some(dim) = dim {
            let previous = self.pending_dim_layout.borrow_mut().take();
            if let Some(previous) = previous {
                previous.dispose();
            }
            *self.pending_dim_layout.borrow_mut() = Some(self.update_dimensions(dim));
        } else {
            let previous = self.pending_layout.borrow_mut().take();
            if let Some(previous) = previous {
                previous.dispose();
            }
            *self.pending_layout.borrow_mut() = Some(self.update_scrollbar());
        }
    }

    fn update_dimensions(&self, dim: (f64, f64)) -> DisposableRef {
        let disposables = DisposableStore::new();
        let (weak, store) = (self.weak_self.clone(), disposables.clone());
        disposables.add(self.ui.schedule_at_next_animation_frame(MODIFY_PRIORITY, move || {
            let Some(this) = weak.upgrade() else { return };
            this.dimension.set(Some(dim));
            this.relayout();
            store.add(this.update_scrollbar());
        }));
        disposables
    }

    /// O `dom.measure` duplo (`double RAF`) que lê o layout e atualiza as barras.
    fn update_scrollbar(&self) -> DisposableRef {
        let weak = self.weak_self.clone();
        let ui = self.ui.clone();
        self.ui.schedule_at_next_animation_frame(MEASURE_PRIORITY, move || {
            ui.schedule_at_next_animation_frame(MEASURE_PRIORITY, move || {
                if let Some(this) = weak.upgrade() {
                    this.scrollable.set_reveal_on_scroll(false);
                    this.scan_dom_node();
                    this.scrollable.set_reveal_on_scroll(true);
                }
            });
        })
    }

    /// `DomScrollableElement.scanDomNode`: a largura e a altura do nó e a do conteúdo como dimensões da rolagem.
    pub fn scan_dom_node(&self) {
        let (width, height) = {
            let tree = self.ui.tree();
            let r = tree.bounds(self.dom_node);
            (r.width(), r.height())
        };
        let (scroll_width, scroll_height) = self.content_size();
        self.scrollable.set_scroll_dimensions(&INewScrollDimensions { width: Some(width), scroll_width: Some(scroll_width), height: Some(height), scroll_height: Some(scroll_height) });
        let position = self.scrollable.get_scroll_position();
        self.scrollable.set_scroll_position(&INewScrollPosition { scroll_left: Some(position.scroll_left), scroll_top: Some(position.scroll_top) });
    }

    /// `scrollWidth` e `scrollHeight` do nó: o conteúdo dos itens.
    fn content_size(&self) -> (f64, f64) {
        let tree = self.ui.tree();
        let nodes = self.nodes.borrow();
        let width: f64 = nodes.iter().map(|n| tree.bounds(*n).width()).sum();
        let height = nodes.iter().map(|n| tree.bounds(*n).height()).fold(0.0, f64::max);
        (width, height)
    }

    /// O `scrollLeft` (o `onScroll` do `DomScrollableElement` põe no `_element.scrollLeft`).
    fn scroll_left(&self) -> f64 {
        self.scrollable.get_scroll_position().scroll_left
    }

    fn apply_scroll(&self) {
        let scroll_left = self.scroll_left();
        let mut tree = self.ui.tree_mut();
        for (node, offset) in self.nodes.borrow().iter().zip(self.offsets.borrow().iter()) {
            let r = tree.bounds(*node);
            tree.set_bounds(*node, Rect::new(offset - scroll_left, r.y0, offset - scroll_left + r.width(), r.y1));
        }
    }

    pub fn set_enabled(&self, value: bool) {
        self.enabled.set(value);
        let mut tree = self.ui.tree_mut();
        tree.toggle_class(self.dom_node, "disabled", !value);
        let cursor = if value { Cursor::Pointer } else { Cursor::Default };
        for node in self.nodes.borrow().iter() {
            tree.style_mut(*node).cursor = Some(cursor);
        }
    }

    /// `domFocus()`.
    pub fn dom_focus(&self) {
        let len = self.items.borrow().len() as i64;
        let idx = if self.focused_item_idx.get() >= 0 { self.focused_item_idx.get() } else { len - 1 };
        if idx >= 0 && idx < len {
            self.focus_item(idx, None);
        } else {
            self.ui.focus(self.dom_node);
        }
    }

    /// `isDOMFocused()`.
    pub fn is_dom_focused(&self) -> bool {
        is_ancestor_of_active_element(&self.ui, self.dom_node)
    }

    pub fn get_focused(&self) -> Option<BreadcrumbsItemRef> {
        self.item_at(self.focused_item_idx.get())
    }

    pub fn set_focused(&self, item: Option<&BreadcrumbsItemRef>, payload: Option<Rc<dyn Any>>) {
        self.focus_item(self.index_of(item), payload);
    }

    pub fn focus_prev(&self, payload: Option<Rc<dyn Any>>) {
        if self.focused_item_idx.get() > 0 {
            self.focus_item(self.focused_item_idx.get() - 1, payload);
        }
    }

    pub fn focus_next(&self, payload: Option<Rc<dyn Any>>) {
        if self.focused_item_idx.get() + 1 < self.nodes.borrow().len() as i64 {
            self.focus_item(self.focused_item_idx.get() + 1, payload);
        }
    }

    fn item_at(&self, idx: i64) -> Option<BreadcrumbsItemRef> {
        usize::try_from(idx).ok().and_then(|i| self.items.borrow().get(i).cloned())
    }

    fn node_at(&self, idx: i64) -> Option<NodeId> {
        usize::try_from(idx).ok().and_then(|i| self.nodes.borrow().get(i).copied())
    }

    /// `this._items.indexOf(item)`: por identidade; -1 se não está (ou se é `undefined`).
    fn index_of(&self, item: Option<&BreadcrumbsItemRef>) -> i64 {
        item.and_then(|item| self.items.borrow().iter().position(|i| Rc::ptr_eq(i, item))).map_or(-1, |i| i as i64)
    }

    fn focus_item(&self, nth: i64, payload: Option<Rc<dyn Any>>) {
        self.focused_item_idx.set(-1);
        let nodes = self.nodes.borrow().clone();
        for (i, node) in nodes.iter().enumerate() {
            if i as i64 != nth {
                self.ui.tree_mut().remove_class(*node, "focused");
            } else {
                self.focused_item_idx.set(i as i64);
                self.ui.tree_mut().add_class(*node, "focused");
                self.ui.focus(*node);
            }
        }
        self.refresh_styles();
        self.reveal_idx(self.focused_item_idx.get(), true);
        let idx = self.focused_item_idx.get();
        self.on_did_focus_item.fire(IBreadcrumbsItemEvent { kind: BreadcrumbsItemEventType::Focus, item: self.item_at(idx), node: self.node_at(idx), payload });
    }

    /// `reveal(item)`.
    pub fn reveal(&self, item: &BreadcrumbsItemRef) {
        let idx = self.index_of(Some(item));
        if idx >= 0 {
            self.reveal_idx(idx, false);
        }
    }

    /// `revealLast()`.
    pub fn reveal_last(&self) {
        self.reveal_idx(self.items.borrow().len() as i64 - 1, false);
    }

    fn reveal_idx(&self, nth: i64, minimal: bool) {
        self.pending_reveal.clear();
        let Some(node) = self.node_at(nth) else { return };
        if !minimal {
            let (weak, idx) = (self.weak_self.clone(), nth);
            let pending = (self.measure)(Box::new(move || {
                if let Some(this) = weak.upgrade()
                    && this.node_at(idx) == Some(node)
                {
                    this.reveal_node(idx, false);
                }
            }));
            self.pending_reveal.set(Some(pending));
            return;
        }

        self.reveal_node(nth, true);
    }

    fn reveal_node(&self, nth: i64, minimal: bool) {
        let width = self.scrollable.get_scroll_dimensions().width;
        let scroll_left = self.scroll_left();
        let node_offset_left = self.offsets.borrow().get(usize::try_from(nth).unwrap_or(usize::MAX)).copied().unwrap_or(0.0);
        if !minimal || node_offset_left > scroll_left + width || node_offset_left < scroll_left {
            self.scrollable.set_reveal_on_scroll(false);
            self.scrollable.set_scroll_position(&INewScrollPosition { scroll_left: Some(node_offset_left), scroll_top: None });
            self.scrollable.set_reveal_on_scroll(true);
        }
    }

    pub fn get_selection(&self) -> Option<BreadcrumbsItemRef> {
        self.item_at(self.selected_item_idx.get())
    }

    pub fn set_selection(&self, item: Option<&BreadcrumbsItemRef>, payload: Option<Rc<dyn Any>>) {
        self.select_item(self.index_of(item), payload);
    }

    fn select_item(&self, nth: i64, payload: Option<Rc<dyn Any>>) {
        self.selected_item_idx.set(-1);
        let nodes = self.nodes.borrow().clone();
        for (i, node) in nodes.iter().enumerate() {
            if i as i64 != nth {
                self.ui.tree_mut().remove_class(*node, "selected");
            } else {
                self.selected_item_idx.set(i as i64);
                self.ui.tree_mut().add_class(*node, "selected");
            }
        }
        self.refresh_styles();
        let idx = self.selected_item_idx.get();
        self.on_did_select_item.fire(IBreadcrumbsItemEvent { kind: BreadcrumbsItemEventType::Select, item: self.item_at(idx), node: self.node_at(idx), payload });
    }

    pub fn get_items(&self) -> Vec<BreadcrumbsItemRef> {
        self.items.borrow().clone()
    }

    /// `setItems(items)`.
    pub fn set_items(&self, items: Vec<BreadcrumbsItemRef>) {
        let (mut prefix, mut removed_count) = (None, 0);
        let outcome = errors::catch(|| {
            let current = self.items.borrow().clone();
            let prefix_len = common_prefix_length(&current, &items, |a, b| a.equals(&**b));
            prefix = Some(prefix_len);
            let removed: Vec<BreadcrumbsItemRef> = {
                let mut list = self.items.borrow_mut();
                let removed = list.split_off(prefix_len);
                list.extend(items[prefix_len..].iter().cloned());
                removed
            };
            removed_count = removed.len();
            self.render_from(prefix_len);
            for item in removed.iter().chain(items[..prefix_len].iter()) {
                item.dispose();
            }
            self.focus_item(-1, None);
        });
        if let Err(e) = outcome {
            let mut error = Error::new(format!("BreadcrumbsItem#setItems: newItems: {}, prefix: {}, removed: {}", items.len(), prefix.map_or_else(|| "undefined".to_owned(), |p| p.to_string()), removed_count));
            error.name = e.name;
            errors::throw(error);
        }
    }

    fn render_from(&self, mut start: usize) {
        let mut did_change = false;
        let item_count = self.items.borrow().len();
        while start < item_count && start < self.nodes.borrow().len() {
            let (item, node) = (self.items.borrow()[start].clone(), self.nodes.borrow()[start]);
            self.render_item(&item, node);
            did_change = true;
            start += 1;
        }
        while start < self.nodes.borrow().len() {
            let free = self.nodes.borrow_mut().pop();
            if let Some(free) = free {
                self.free_nodes.borrow_mut().push(free);
                self.ui.tree_mut().detach(free);
                did_change = true;
            }
        }

        while start < item_count {
            let item = self.items.borrow()[start].clone();
            let node = self.free_nodes.borrow_mut().pop().unwrap_or_else(|| self.create_node());
            self.render_item(&item, node);
            self.ui.tree_mut().append_child(self.dom_node, node);
            self.nodes.borrow_mut().push(node);
            did_change = true;
            start += 1;
        }
        self.relayout();
        if did_change {
            self.layout(None);
        }
    }

    fn create_node(&self) -> NodeId {
        let node = create_detached_el(&mut self.ui.tree_mut(), Role::ListItem, "");
        let (weak, enter) = (self.weak_self.clone(), node);
        self.base.register(self.ui.listen(node, EventKind::PointerEnter, false, move |_| {
            if let Some(this) = weak.upgrade() {
                this.hovered.set(Some(enter));
                this.refresh_styles();
            }
        }));
        let weak = self.weak_self.clone();
        self.base.register(self.ui.listen(node, EventKind::PointerLeave, false, move |_| {
            if let Some(this) = weak.upgrade() {
                if this.hovered.get() == Some(node) {
                    this.hovered.set(None);
                }
                this.refresh_styles();
            }
        }));
        node
    }

    fn render_item(&self, item: &BreadcrumbsItemRef, container: NodeId) {
        let foreground = self.styles.breadcrumbs_foreground.unwrap_or(self.inherited_foreground.get());
        {
            let mut tree = self.ui.tree_mut();
            clear_node(&mut tree, container);
            tree.set_class_name(container, "");
        }
        if let Err(err) = errors::catch(|| item.render(&self.ui, container, foreground)) {
            let mut tree = self.ui.tree_mut();
            clear_node(&mut tree, container);
            let text = tree.create(container, Role::Generic);
            let style = crate::text::ui_font(crate::text::UI_FONT_SIZE, 400);
            let width = crate::text::measure_text("<<RENDER ERROR>>", &style);
            tree.set_bounds(text, Rect::new(0.0, 0.0, width, crate::text::natural_line_height(&style)));
            tree.style_mut(text).content = Content::Text { text: "<<RENDER ERROR>>".to_owned(), style, color: foreground, line_height: crate::text::natural_line_height(&crate::text::ui_font(crate::text::UI_FONT_SIZE, 400)) };
            eprintln!("{err}");
        }
        let mut tree = self.ui.tree_mut();
        tree.add_class(container, "monaco-breadcrumb-item");
        tree.style_mut(container).cursor = Some(if self.enabled.get() { Cursor::Pointer } else { Cursor::Default });
        let icon_container = tree.create(container, Role::Generic);
        tree.set_class_name(icon_container, &self.separator_icon.as_class_name());
        tree.set_bounds(icon_container, Rect::new(0.0, 0.0, INLINE_ICON_SIZE, INLINE_ICON_SIZE));
        tree.style_mut(icon_container).content = Content::Codicon { id: self.separator_icon.id.split('~').next().unwrap_or_default().to_owned(), size: INLINE_ICON_SIZE as f32, color: foreground };
        drop(tree);
        self.node_colors.borrow_mut().insert(container, foreground);
        // O `.focused` e o `.selected` vão embora com o `className = ''`: o item recém-renderizado volta ao padrão.
        self.refresh_styles();
    }

    /// O flex do CSS: posiciona os filhos de cada item e os itens, e dá ao nó dono o tamanho.
    pub fn relayout(&self) {
        let dim = self.dimension.get();
        let scroll_left = self.scroll_left();
        let mut tree = self.ui.tree_mut();
        let nodes = self.nodes.borrow().clone();
        let content_height = nodes.iter().map(|n| item_content_height(&tree, *n)).fold(0.0, f64::max);
        let height = dim.map_or(content_height, |d| d.1);
        let mut offsets = Vec::with_capacity(nodes.len());
        let mut x = 0.0;
        for node in &nodes {
            let width = layout_item_children(&mut tree, *node, height);
            offsets.push(x);
            tree.set_bounds(*node, Rect::new(x - scroll_left, 0.0, x - scroll_left + width, height));
            x += width;
        }
        *self.offsets.borrow_mut() = offsets;
        let origin = tree.bounds(self.dom_node);
        match dim {
            Some((w, h)) => {
                tree.set_fill(self.dom_node, false, false);
                tree.set_bounds(self.dom_node, Rect::new(origin.x0, origin.y0, origin.x0 + w, origin.y0 + h));
            }
            None => {
                tree.set_fill(self.dom_node, true, false);
                tree.set_bounds(self.dom_node, Rect::new(origin.x0, origin.y0, origin.x0, origin.y0 + height));
            }
        }
        // O elemento rolável tem o tamanho do conteúdo: largura do dono, altura do nó.
        let scrollable = self.scrollable.get_dom_node();
        let outer = tree.bounds(scrollable);
        tree.set_fill(scrollable, dim.is_none(), false);
        tree.set_bounds(scrollable, Rect::new(outer.x0, outer.y0, outer.x0 + dim.map_or(0.0, |d| d.0), outer.y0 + height));
    }

    /// O `_style` e as regras `:hover`, `.focused` e `.focused.selected`: o fundo do nó e a cor do conteúdo de cada item.
    pub fn refresh_styles(&self) {
        let styles = self.styles;
        let disabled = !self.enabled.get();
        let nodes = self.nodes.borrow().clone();
        let mut tree = self.ui.tree_mut();
        tree.style_mut(self.dom_node).background = styles.breadcrumbs_background;
        for node in nodes {
            let (focused, selected) = (tree.has_class(node, "focused"), tree.has_class(node, "selected"));
            let mut color = styles.breadcrumbs_foreground.unwrap_or(self.inherited_foreground.get());
            if focused && let Some(c) = styles.breadcrumbs_focus_foreground {
                color = c;
            }
            if focused && selected && let Some(c) = styles.breadcrumbs_focus_and_selection_foreground {
                color = c;
            }
            if !disabled && !focused && !selected && self.hovered.get() == Some(node) && let Some(c) = styles.breadcrumbs_hover_foreground {
                color = c;
            }
            let current = self.node_colors.borrow().get(&node).copied();
            if let Some(current) = current.filter(|c| *c != color) {
                recolor(&mut tree, node, current, color);
                self.node_colors.borrow_mut().insert(node, color);
            }
        }
    }

    fn on_click(&self, target: NodeId, pointer: je_ui::PointerData) {
        if !self.enabled.get() {
            return;
        }
        let mut el = Some(target);
        while let Some(current) = el {
            let idx = self.nodes.borrow().iter().position(|n| *n == current);
            if let Some(idx) = idx {
                let payload: Rc<dyn Any> = Rc::new(pointer);
                self.focus_item(idx as i64, Some(payload.clone()));
                self.select_item(idx as i64, Some(payload));
                break;
            }
            el = self.ui.tree().parent(current);
        }
    }
}

/// A altura do conteúdo de um item: o filho mais alto (o ícone separador conta).
fn item_content_height(tree: &Tree, node: NodeId) -> f64 {
    tree.children(node).iter().map(|c| tree.bounds(*c).height()).fold(0.0, f64::max)
}

/// Posiciona os filhos do item em fila, centrados na vertical em `height`; devolve a largura do item.
fn layout_item_children(tree: &mut Tree, node: NodeId, height: f64) -> f64 {
    let mut x = 0.0;
    for child in tree.children(node).to_vec() {
        let r = tree.bounds(child);
        let top = (height - r.height()) / 2.0;
        tree.set_bounds(child, Rect::new(x, top, x + r.width(), top + r.height()));
        x += r.width();
    }
    x
}

/// Troca `from` por `to` em todo o conteúdo do item (a herança da propriedade `color`).
fn recolor(tree: &mut Tree, root: NodeId, from: Rgba, to: Rgba) {
    for node in tree.subtree(root) {
        let style = tree.style_mut(node);
        match &mut style.content {
            Content::Text { color, .. } | Content::Codicon { color, .. } if *color == from => *color = to,
            _ => {}
        }
        if style.background == Some(from) && node != root {
            style.background = Some(to);
        }
    }
}

impl IDisposable for BreadcrumbsWidget {
    fn dispose(&self) {
        if self.disposed.replace(true) {
            return;
        }
        self.base.dispose();
        for pending in [self.pending_layout.borrow_mut().take(), self.pending_dim_layout.borrow_mut().take()].into_iter().flatten() {
            pending.dispose();
        }
        let (items, nodes, free) = (std::mem::take(&mut *self.items.borrow_mut()), std::mem::take(&mut *self.nodes.borrow_mut()), std::mem::take(&mut *self.free_nodes.borrow_mut()));
        {
            let mut tree = self.ui.tree_mut();
            for node in nodes.into_iter().chain(free) {
                tree.remove(node);
            }
            tree.remove(self.dom_node);
        }
        self.ui.remove(self.scrollable.get_dom_node());
        for item in &items {
            item.dispose();
        }
    }

    fn constructor_name(&self) -> &'static str {
        "BreadcrumbsWidget"
    }
}
