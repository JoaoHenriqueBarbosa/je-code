//! Porte de `src/vs/base/browser/ui/splitview/paneview.ts`: o [`Pane`] (uma view do split view com cabeçalho
//! que recolhe e expande) e o [`PaneView`] (um split view de panes).
//!
//! `Pane` é uma classe abstrata no original, com `renderHeader`, `renderBody` e `layoutBody` abstratos. Aqui
//! `Pane` é um struct concreto que guarda um [`PaneContent`], o trait com esses três métodos (e o
//! `getAriaHeaderLabel` sobrescrevível). Como no original, é preciso chamar [`Pane::render`] depois de criar.
//!
//! Regras de CSS que viram código do widget:
//!
//! - O `.pane` é um flex em coluna (cabeçalho em cima, corpo ocupando o resto), ou em linha quando está na
//!   horizontal e recolhido (o cabeçalho vira uma faixa em pé). [`Pane::arrange`] resolve as posições a cada
//!   `layout`, expansão, mudança de orientação ou de visibilidade do cabeçalho.
//! - `border-top` do cabeçalho e `border-left` do painel (uma borda de um lado só) são nós de 1px filhos.
//! - `.hidden` do cabeçalho é `display: none` (`NodeStyle::visible`).
//!
//! Fica de fora, por enquanto:
//!
//! - **Arrastar e soltar** (`PaneDraggable`, `IPaneDndController` usado no `dragstart`/`drop`): o `je-ui` ainda
//!   não tem DnD. O trait [`PaneDndController`] existe e `dnd` é aceito, mas o evento `on_did_drop` nunca dispara.
//! - **Rolagem**: o `onDidScroll` vem do split view, que ainda não tem barra de rolagem.
//! - **`aria-expanded`** e `tabindex` do cabeçalho: o `je-ui` modela só papel e rótulo. O papel `button` sai
//!   quando o painel não é recolhível, como no original.
//! - O `animation` por CSS (`.animated .split-view-view { transition }`) não existe: a classe `animated` é
//!   posta e tirada, mas nada anima.

use std::cell::{Cell, RefCell};
use std::rc::{Rc, Weak};

use je_base::event::{Emitter, Event as BaseEvent};
use je_base::key_codes::KeyCode;
use je_base::lifecycle::{Disposable, DisposableRef, DisposableStore, IDisposable};
use je_base::runtime::{self, TimerId};
use je_render::Rgba;
use je_render::kurbo::Rect;
use je_ui::{Cursor, EventKind, NodeId, PointerEvents, Role, Ui};

use crate::sash::{BoundarySashes, Orientation, registered_emitter};
use crate::splitview::{LayoutPriority, ScrollEvent, Sizing, SplitView, SplitViewOptions, SplitViewView, ViewRef};

/// O tamanho padrão do cabeçalho (`DEFAULT_PANE_HEADER_SIZE`).
pub const DEFAULT_PANE_HEADER_SIZE: f64 = 22.0;

thread_local! {
    static GLOBAL_PANE_HEADER_SIZE: Cell<f64> = const { Cell::new(DEFAULT_PANE_HEADER_SIZE) };
}

/// `setGlobalPaneHeaderSize`: o tamanho do cabeçalho usado por todos os panes.
pub fn set_global_pane_header_size(size: f64) {
    GLOBAL_PANE_HEADER_SIZE.with(|s| s.set(size));
}

fn global_pane_header_size() -> f64 {
    GLOBAL_PANE_HEADER_SIZE.with(Cell::get)
}

/// O `IPaneOptions`.
#[derive(Clone, Debug, Default)]
pub struct PaneOptions {
    pub minimum_body_size: Option<f64>,
    pub maximum_body_size: Option<f64>,
    pub expanded: Option<bool>,
    pub orientation: Option<Orientation>,
    pub title: String,
    pub title_description: Option<String>,
}

/// O `IPaneStyles`.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct PaneStyles {
    pub drop_background: Option<Rgba>,
    pub header_foreground: Option<Rgba>,
    pub header_background: Option<Rgba>,
    pub header_border: Option<Rgba>,
    pub left_border: Option<Rgba>,
}

/// Os três métodos abstratos de `Pane`, mais o rótulo de acessibilidade.
pub trait PaneContent {
    /// `getAriaHeaderLabel(title)`.
    fn get_aria_header_label(&self, title: &str) -> String {
        je_base::nls::localize("viewSection", "{0} Section", &[title])
    }

    fn render_header(&self, container: NodeId);
    fn render_body(&self, container: NodeId);
    fn layout_body(&self, height: f64, width: f64);
}

/// Um pane.
pub struct Pane {
    base: Rc<Disposable>,
    weak_self: Weak<Pane>,
    ui: Ui,
    content: Rc<dyn PaneContent>,
    element: NodeId,
    header: Cell<Option<NodeId>>,
    body: Cell<Option<NodeId>>,
    header_border: Cell<Option<NodeId>>,
    left_border: Cell<Option<NodeId>>,
    expanded: Cell<bool>,
    orientation: Cell<Orientation>,
    expanded_size: Cell<Option<f64>>,
    header_visible: Cell<bool>,
    collapsible: Cell<bool>,
    body_rendered: Cell<bool>,
    minimum_body_size: Cell<f64>,
    maximum_body_size: Cell<f64>,
    aria_header_label: RefCell<String>,
    styles: Cell<PaneStyles>,
    animation_timer: Cell<Option<TimerId>>,
    orthogonal_size: Cell<f64>,
    on_did_change: Rc<Emitter<Option<f64>>>,
    on_did_change_expansion_state: Rc<Emitter<bool>>,
    /// Os ouvintes de `render` (foco, teclado, clique), descartados com o pane.
    render_disposables: Rc<DisposableStore>,
}

impl Pane {
    pub fn new(ui: &Ui, options: PaneOptions, content: Rc<dyn PaneContent>) -> Rc<Pane> {
        let base = Disposable::new();
        let expanded = options.expanded.unwrap_or(true);
        let orientation = options.orientation.unwrap_or(Orientation::Vertical);
        let element = {
            let mut tree = ui.tree_mut();
            let element = tree.create_detached(Role::Generic);
            tree.add_class(element, "pane");
            element
        };
        let aria_header_label = content.get_aria_header_label(&options.title);
        let render_disposables = DisposableStore::new();
        base.register(render_disposables.clone());
        Rc::new_cyclic(|weak| Pane {
            weak_self: weak.clone(),
            ui: ui.clone(),
            content,
            element,
            header: Cell::new(None),
            body: Cell::new(None),
            header_border: Cell::new(None),
            left_border: Cell::new(None),
            expanded: Cell::new(expanded),
            orientation: Cell::new(orientation),
            expanded_size: Cell::new(None),
            header_visible: Cell::new(true),
            collapsible: Cell::new(true),
            body_rendered: Cell::new(false),
            minimum_body_size: Cell::new(options.minimum_body_size.unwrap_or(if orientation == Orientation::Horizontal { 200.0 } else { 120.0 })),
            maximum_body_size: Cell::new(options.maximum_body_size.unwrap_or(f64::INFINITY)),
            aria_header_label: RefCell::new(aria_header_label),
            styles: Cell::new(PaneStyles::default()),
            animation_timer: Cell::new(None),
            orthogonal_size: Cell::new(0.0),
            on_did_change: registered_emitter(&base),
            on_did_change_expansion_state: registered_emitter(&base),
            render_disposables,
            base,
        })
    }

    /// O nó do pane (o `.pane`).
    pub fn element(&self) -> NodeId {
        self.element
    }

    /// Dispara quando o pane expande ou recolhe, com o novo estado.
    pub fn on_did_change_expansion_state(&self) -> BaseEvent<bool> {
        self.on_did_change_expansion_state.event()
    }

    pub fn aria_header_label(&self) -> String {
        self.aria_header_label.borrow().clone()
    }

    pub fn set_aria_header_label(&self, label: &str) {
        *self.aria_header_label.borrow_mut() = label.to_owned();
        if let Some(header) = self.header.get() {
            self.ui.tree_mut().set_label(header, label);
        }
    }

    /// O elemento que se arrasta (o cabeçalho).
    pub fn draggable_element(&self) -> Option<NodeId> {
        self.header.get()
    }

    /// O elemento onde se solta (o próprio pane).
    pub fn drop_target_element(&self) -> NodeId {
        self.element
    }

    pub fn drop_background(&self) -> Option<Rgba> {
        self.styles.get().drop_background
    }

    pub fn minimum_body_size(&self) -> f64 {
        self.minimum_body_size.get()
    }

    pub fn set_minimum_body_size(&self, size: f64) {
        self.minimum_body_size.set(size);
        self.on_did_change.fire(None);
    }

    pub fn maximum_body_size(&self) -> f64 {
        self.maximum_body_size.get()
    }

    pub fn set_maximum_body_size(&self, size: f64) {
        self.maximum_body_size.set(size);
        self.on_did_change.fire(None);
    }

    fn header_size(&self) -> f64 {
        if self.header_visible() { global_pane_header_size() } else { 0.0 }
    }

    /// O tamanho que o pane recebe do split view na direção ortogonal (a largura de um pane vertical).
    pub fn orthogonal_size(&self) -> f64 {
        self.orthogonal_size.get()
    }

    pub fn set_orthogonal_size(&self, size: f64) {
        self.orthogonal_size.set(size);
    }

    pub fn is_expanded(&self) -> bool {
        self.expanded.get()
    }

    pub fn set_expanded(&self, expanded: bool) -> bool {
        if !expanded && !self.collapsible() {
            return false;
        }

        if self.expanded.get() == expanded {
            return false;
        }

        self.ui.tree_mut().toggle_class(self.element, "expanded", expanded);

        self.expanded.set(expanded);
        self.update_header();

        let body = self.body.get().expect("`render` não foi chamado");
        if expanded {
            if !self.body_rendered.get() {
                self.content.render_body(body);
                self.body_rendered.set(true);
            }

            if let Some(timer) = self.animation_timer.take() {
                runtime::clear_timeout(timer);
            }
            self.ui.tree_mut().append_child(self.element, body);
        } else {
            let (weak, ui) = (self.weak_self.clone(), self.ui.clone());
            self.animation_timer.set(Some(runtime::set_timeout(
                move || {
                    if weak.upgrade().is_some() {
                        ui.tree_mut().detach(body);
                    }
                },
                200,
            )));
        }
        self.rearrange();

        self.on_did_change_expansion_state.fire(expanded);
        self.on_did_change.fire(if expanded { self.expanded_size.get() } else { None });
        true
    }

    pub fn header_visible(&self) -> bool {
        self.header_visible.get()
    }

    pub fn set_header_visible(&self, visible: bool) {
        if self.header_visible.get() == visible {
            return;
        }

        self.header_visible.set(visible);
        self.update_header();
        self.on_did_change.fire(None);
    }

    pub fn collapsible(&self) -> bool {
        self.collapsible.get()
    }

    pub fn set_collapsible(&self, collapsible: bool) {
        if self.collapsible.get() == collapsible {
            return;
        }

        self.collapsible.set(collapsible);
        self.update_header();
    }

    pub fn orientation(&self) -> Orientation {
        self.orientation.get()
    }

    pub fn set_orientation(&self, orientation: Orientation) {
        if self.orientation.get() == orientation {
            return;
        }

        self.orientation.set(orientation);

        {
            let mut tree = self.ui.tree_mut();
            tree.toggle_class(self.element, "horizontal", orientation == Orientation::Horizontal);
            tree.toggle_class(self.element, "vertical", orientation == Orientation::Vertical);
        }

        if self.header.get().is_some() {
            self.update_header();
        }
    }

    /// Monta o cabeçalho e o corpo. Precisa ser chamado depois de criar o pane (ver o topo do módulo).
    pub fn render(self: &Rc<Self>) {
        let orientation = self.orientation.get();
        let (header, body) = {
            let mut tree = self.ui.tree_mut();
            tree.toggle_class(self.element, "expanded", self.expanded.get());
            tree.toggle_class(self.element, "horizontal", orientation == Orientation::Horizontal);
            tree.toggle_class(self.element, "vertical", orientation == Orientation::Vertical);
            tree.set_fill(self.element, true, true);
            tree.style_mut(self.element).clip = true;

            let header = tree.create(self.element, Role::Button);
            tree.add_class(header, "pane-header");
            tree.set_label(header, self.aria_header_label.borrow().as_str());
            tree.style_mut(header).clip = true;
            (header, tree.create_detached(Role::Generic))
        };
        self.header.set(Some(header));
        self.content.render_header(header);

        // `trackFocus(header)`: o `focus`/`blur` do cabeçalho marcam a classe `focused`.
        let weak = self.weak_self.clone();
        self.render_disposables.add(self.ui.listen(header, EventKind::Focus, true, move |_| {
            if let Some(pane) = weak.upgrade() {
                pane.ui.tree_mut().add_class(header, "focused");
            }
        }));
        let weak = self.weak_self.clone();
        self.render_disposables.add(self.ui.listen(header, EventKind::Blur, true, move |_| {
            if let Some(pane) = weak.upgrade() {
                pane.ui.tree_mut().remove_class(header, "focused");
            }
        }));

        self.update_header();

        let weak = self.weak_self.clone();
        self.render_disposables.add(self.ui.listen(header, EventKind::KeyDown, false, move |e| {
            let (Some(pane), Some(key)) = (weak.upgrade(), e.key) else { return };
            if key.key_code == KeyCode::Enter || key.key_code == KeyCode::Space {
                pane.set_expanded(!pane.is_expanded());
            } else if key.key_code == KeyCode::LeftArrow {
                pane.set_expanded(false);
            } else if key.key_code == KeyCode::RightArrow {
                pane.set_expanded(true);
            }
        }));

        let weak = self.weak_self.clone();
        self.render_disposables.add(self.ui.listen(header, EventKind::Click, false, move |e| {
            if let Some(pane) = weak.upgrade()
                && !e.default_prevented() {
                    pane.set_expanded(!pane.is_expanded());
                }
        }));

        {
            let mut tree = self.ui.tree_mut();
            tree.append_child(self.element, body);
            tree.add_class(body, "pane-body");
            tree.style_mut(body).clip = true;
        }
        self.body.set(Some(body));

        // Só renderiza o corpo se ele ficar visível; senão, renderiza quando o pane expandir.
        if !self.body_rendered.get() && self.is_expanded() {
            self.content.render_body(body);
            self.body_rendered.set(true);
        }

        if !self.is_expanded() {
            self.ui.tree_mut().detach(body);
        }
        self.rearrange();
    }

    /// O `layout(size)` do pane: `size` é o tamanho na direção do split view.
    pub fn layout(&self, size: f64) {
        let header_size = self.header_size();
        let vertical = self.orientation.get() == Orientation::Vertical;

        let width = if vertical { self.orthogonal_size.get() } else { size };
        let height = if vertical { size - header_size } else { self.orthogonal_size.get() - header_size };

        if self.is_expanded() {
            if let Some(body) = self.body.get() {
                self.ui.tree_mut().toggle_class(body, "wide", width >= 600.0);
            }
            self.content.layout_body(height, width);
            self.expanded_size.set(Some(size));
        }
        let (element_width, element_height) = if vertical { (self.orthogonal_size.get(), size) } else { (size, self.orthogonal_size.get()) };
        self.arrange(element_width, element_height);
    }

    pub fn style(&self, styles: PaneStyles) {
        self.styles.set(styles);

        if self.header.get().is_none() {
            return;
        }

        self.update_header();
    }

    pub fn styles(&self) -> PaneStyles {
        self.styles.get()
    }

    fn update_header(&self) {
        let Some(header) = self.header.get() else { return };
        let expanded = !self.header_visible() || self.is_expanded();
        let styles = self.styles.get();
        let orientation = self.orientation.get();
        let collapsible = self.collapsible();

        let header_border = (styles.header_border.is_some() && orientation == Orientation::Vertical).then_some(styles.header_border).flatten();
        let left_border = (styles.left_border.is_some() && orientation == Orientation::Horizontal).then_some(styles.left_border).flatten();
        let (header_border_node, left_border_node) = {
            let mut tree = self.ui.tree_mut();
            // `tabindex` e `role`: só um pane recolhível é um botão.
            tree.set_role(header, if collapsible { Role::Button } else { Role::Generic });

            tree.toggle_class(header, "hidden", !self.header_visible());
            tree.toggle_class(header, "expanded", expanded);
            tree.toggle_class(header, "not-collapsible", !collapsible);

            let style = tree.style_mut(header);
            style.visible = self.header_visible();
            style.cursor = Some(if collapsible { Cursor::Pointer } else { Cursor::Default });
            // `header.style.backgroundColor = collapsible ? headerBackground : 'transparent'`
            style.background = if collapsible { styles.header_background } else { None };

            let header_border_node = self.header_border.get().or_else(|| {
                header_border.map(|_| {
                    let node = tree.create(header, Role::Generic);
                    tree.style_mut(node).pointer_events = PointerEvents::None;
                    tree.style_mut(node).z_index = 1;
                    node
                })
            });
            let left_border_node = self.left_border.get().or_else(|| {
                left_border.map(|_| {
                    let node = tree.create(self.element, Role::Generic);
                    tree.style_mut(node).pointer_events = PointerEvents::None;
                    tree.style_mut(node).z_index = 1;
                    node
                })
            });
            (header_border_node, left_border_node)
        };

        // `header.style.borderTop` e `element.style.borderLeft`: uma linha de 1px de um lado só.
        for (slot, node, color) in [(&self.header_border, header_border_node, header_border), (&self.left_border, left_border_node, left_border)] {
            match (node, color) {
                (Some(node), Some(color)) => {
                    self.ui.tree_mut().style_mut(node).background = Some(color);
                    slot.set(Some(node));
                }
                (Some(node), None) => {
                    self.ui.remove(node);
                    slot.set(None);
                }
                (None, _) => {}
            }
        }
        self.rearrange();
    }

    /// `arrange` com o tamanho atual do pane na árvore (o CSS recalcula sozinho; aqui o widget pede).
    fn rearrange(&self) {
        let size = self.ui.tree().bounds(self.element);
        self.arrange(size.width(), size.height());
    }

    /// O flex do `.pane`: cabeçalho e corpo dentro de `width` por `height`.
    fn arrange(&self, width: f64, height: f64) {
        let (Some(header), Some(body)) = (self.header.get(), self.body.get()) else { return };
        let row = self.orientation.get() == Orientation::Horizontal && !self.is_expanded();
        let global = global_pane_header_size();
        let mut tree = self.ui.tree_mut();

        if row {
            // `flex-direction: row`: o cabeçalho é uma faixa em pé de largura `--pane-header-size`.
            tree.set_bounds(header, Rect::new(0.0, 0.0, global, height));
            tree.set_bounds(body, Rect::new(global, 0.0, width.max(global), height));
        } else {
            let header_size = self.header_size();
            tree.set_bounds(header, Rect::new(0.0, 0.0, width, global));
            tree.set_bounds(body, Rect::new(0.0, header_size, width, height.max(header_size)));
        }
        if let Some(node) = self.header_border.get() {
            tree.set_bounds(node, Rect::new(0.0, 0.0, if row { global } else { width }, 1.0));
        }
        if let Some(node) = self.left_border.get() {
            tree.set_bounds(node, Rect::new(0.0, 0.0, 1.0, height));
        }
    }
}

impl SplitViewView<()> for Pane {
    fn element(&self) -> NodeId {
        self.element
    }

    fn minimum_size(&self) -> f64 {
        let header_size = self.header_size();
        let expanded = !self.header_visible() || self.is_expanded();
        let minimum_body_size = if expanded { self.minimum_body_size() } else { 0.0 };

        header_size + minimum_body_size
    }

    fn maximum_size(&self) -> f64 {
        let header_size = self.header_size();
        let expanded = !self.header_visible() || self.is_expanded();
        let maximum_body_size = if expanded { self.maximum_body_size() } else { 0.0 };

        header_size + maximum_body_size
    }

    fn priority(&self) -> Option<LayoutPriority> {
        None
    }

    fn on_did_change(&self) -> BaseEvent<Option<f64>> {
        self.on_did_change.event()
    }

    fn layout(&self, size: f64, _offset: f64, _context: Option<&()>) {
        Pane::layout(self, size);
    }
}

impl IDisposable for Pane {
    fn dispose(&self) {
        if let Some(timer) = self.animation_timer.take() {
            runtime::clear_timeout(timer);
        }
        self.base.dispose();
        // O corpo recolhido está solto da árvore: destrói à parte. O resto sai com o elemento.
        if let Some(body) = self.body.get()
            && self.ui.tree().parent(body).is_none() {
                self.ui.remove(body);
            }
        self.ui.remove(self.element);
    }

    fn constructor_name(&self) -> &'static str {
        "Pane"
    }
}

/// O `IPaneDndController`.
pub trait PaneDndController {
    fn can_drag(&self, pane: &Pane) -> bool;
    fn can_drop(&self, pane: &Pane, over_pane: &Pane) -> bool;
}

/// O `DefaultPaneDndController`.
#[derive(Default)]
pub struct DefaultPaneDndController;

impl PaneDndController for DefaultPaneDndController {
    fn can_drag(&self, _pane: &Pane) -> bool {
        true
    }

    fn can_drop(&self, _pane: &Pane, _over_pane: &Pane) -> bool {
        true
    }
}

/// O `IPaneViewOptions`.
#[derive(Default)]
pub struct PaneViewOptions {
    pub dnd: Option<Rc<dyn PaneDndController>>,
    pub orientation: Option<Orientation>,
}

struct PaneItem {
    pane: Rc<Pane>,
    disposable: DisposableRef,
}

/// O `PaneView`.
pub struct PaneView {
    base: Rc<Disposable>,
    weak_self: Weak<PaneView>,
    ui: Ui,
    // TODO(integração): o `PaneDraggable` ainda não existe (sem DnD no `je-ui`), então nada lê `dnd`.
    #[allow(dead_code)]
    dnd: Option<Rc<dyn PaneDndController>>,
    element: NodeId,
    pane_items: RefCell<Vec<PaneItem>>,
    orthogonal_size: Cell<f64>,
    size: Cell<f64>,
    splitview: RefCell<Rc<SplitView<()>>>,
    animation_timer: Cell<Option<TimerId>>,
    on_did_drop: Rc<Emitter<(Rc<Pane>, Rc<Pane>)>>,
    orientation: Cell<Orientation>,
    boundary_sashes: RefCell<Option<BoundarySashes>>,
    on_did_sash_change: BaseEvent<usize>,
    on_did_sash_reset: BaseEvent<usize>,
    on_did_scroll: BaseEvent<ScrollEvent>,
}

impl PaneView {
    pub fn new(ui: &Ui, container: NodeId, options: PaneViewOptions) -> Rc<PaneView> {
        let base = Disposable::new();
        let orientation = options.orientation.unwrap_or(Orientation::Vertical);
        let element = {
            let mut tree = ui.tree_mut();
            let element = tree.create_detached(Role::Generic);
            tree.append_child(container, element);
            tree.add_class(element, "monaco-pane-view");
            tree.set_fill(element, true, true);
            element
        };
        let splitview = SplitView::new(ui, element, SplitViewOptions { orientation: Some(orientation), ..SplitViewOptions::default() });
        base.register(splitview.clone());
        let this = Rc::new_cyclic(|weak| PaneView {
            weak_self: weak.clone(),
            ui: ui.clone(),
            dnd: options.dnd,
            element,
            pane_items: RefCell::new(Vec::new()),
            orthogonal_size: Cell::new(0.0),
            size: Cell::new(0.0),
            on_did_sash_reset: splitview.on_did_sash_reset(),
            on_did_sash_change: splitview.on_did_sash_change(),
            on_did_scroll: splitview.on_did_scroll(),
            splitview: RefCell::new(splitview),
            animation_timer: Cell::new(None),
            on_did_drop: registered_emitter(&base),
            orientation: Cell::new(orientation),
            boundary_sashes: RefCell::new(None),
            base,
        });

        // As setas com o foco num cabeçalho de pane movem o foco entre os cabeçalhos.
        let weak = this.weak_self.clone();
        this.base.register(ui.listen(element, EventKind::KeyDown, false, move |e| {
            let (Some(pane_view), Some(key)) = (weak.upgrade(), e.key) else { return };
            if !pane_view.ui.tree().has_class(e.target, "pane-header") {
                return;
            }
            if key.key_code == KeyCode::UpArrow {
                pane_view.focus_previous();
            } else if key.key_code == KeyCode::DownArrow {
                pane_view.focus_next();
            }
        }));
        this
    }

    pub fn element(&self) -> NodeId {
        self.element
    }

    pub fn orientation(&self) -> Orientation {
        self.orientation.get()
    }

    /// Dispara quando um pane é solto sobre outro. Nunca dispara por enquanto (sem DnD no `je-ui`).
    pub fn on_did_drop(&self) -> BaseEvent<(Rc<Pane>, Rc<Pane>)> {
        self.on_did_drop.event()
    }

    pub fn on_did_sash_change(&self) -> BaseEvent<usize> {
        self.on_did_sash_change.clone()
    }

    pub fn on_did_sash_reset(&self) -> BaseEvent<usize> {
        self.on_did_sash_reset.clone()
    }

    pub fn on_did_scroll(&self) -> BaseEvent<ScrollEvent> {
        self.on_did_scroll.clone()
    }

    fn splitview(&self) -> Rc<SplitView<()>> {
        self.splitview.borrow().clone()
    }

    pub fn add_pane(self: &Rc<Self>, pane: Rc<Pane>, size: f64, index: Option<usize>) {
        let splitview = self.splitview();
        let index = index.unwrap_or_else(|| splitview.length());
        let disposables = DisposableStore::new();
        let weak = self.weak_self.clone();
        disposables.add(pane.on_did_change_expansion_state().listen(move |_| {
            if let Some(pane_view) = weak.upgrade() {
                pane_view.setup_animation();
            }
        }));

        self.pane_items.borrow_mut().insert(index, PaneItem { pane: pane.clone(), disposable: disposables });
        pane.set_orientation(self.orientation.get());
        pane.set_orthogonal_size(self.orthogonal_size.get());
        let view: ViewRef<()> = pane;
        splitview.add_view(view, size, Some(index), false);
        // TODO(integração): `new PaneDraggable(pane, dnd, dndContext)` quando houver DnD no `je-ui`.
    }

    pub fn remove_pane(&self, pane: &Rc<Pane>) {
        let Some(index) = self.index_of(pane) else { return };

        self.splitview().remove_view(index, pane.is_expanded().then_some(Sizing::Distribute));
        let item = self.pane_items.borrow_mut().remove(index);
        item.disposable.dispose();
    }

    pub fn move_pane(&self, from: &Rc<Pane>, to: &Rc<Pane>) {
        let (Some(from_index), Some(to_index)) = (self.index_of(from), self.index_of(to)) else { return };

        let item = self.pane_items.borrow_mut().remove(from_index);
        self.pane_items.borrow_mut().insert(to_index, item);

        self.splitview().move_view(from_index, to_index);
    }

    pub fn resize_pane(&self, pane: &Rc<Pane>, size: f64) {
        let Some(index) = self.index_of(pane) else { return };

        self.splitview().resize_view(index, size);
    }

    pub fn get_pane_size(&self, pane: &Rc<Pane>) -> f64 {
        let Some(index) = self.index_of(pane) else { return -1.0 };

        self.splitview().get_view_size(index)
    }

    fn index_of(&self, pane: &Rc<Pane>) -> Option<usize> {
        self.pane_items.borrow().iter().position(|item| Rc::ptr_eq(&item.pane, pane))
    }

    pub fn layout(&self, height: f64, width: f64) {
        let vertical = self.orientation.get() == Orientation::Vertical;
        self.orthogonal_size.set(if vertical { width } else { height });
        self.size.set(if self.orientation.get() == Orientation::Horizontal { width } else { height });

        for item in self.pane_items.borrow().iter() {
            item.pane.set_orthogonal_size(self.orthogonal_size.get());
        }

        self.splitview().layout(self.size.get(), None);
    }

    pub fn set_boundary_sashes(&self, sashes: BoundarySashes) {
        *self.boundary_sashes.borrow_mut() = Some(sashes.clone());
        self.update_splitview_orthogonal_sashes(Some(&sashes));
    }

    fn update_splitview_orthogonal_sashes(&self, sashes: Option<&BoundarySashes>) {
        let splitview = self.splitview();
        if self.orientation.get() == Orientation::Vertical {
            splitview.set_orthogonal_start_sash(sashes.and_then(|s| s.left.clone()));
            splitview.set_orthogonal_end_sash(sashes.and_then(|s| s.right.clone()));
        } else {
            splitview.set_orthogonal_end_sash(sashes.and_then(|s| s.bottom.clone()));
        }
    }

    pub fn flip_orientation(self: &Rc<Self>, height: f64, width: f64) {
        let orientation = if self.orientation.get() == Orientation::Vertical { Orientation::Horizontal } else { Orientation::Vertical };
        self.orientation.set(orientation);
        let pane_sizes: Vec<f64> = self.pane_items.borrow().iter().map(|item| self.get_pane_size(&item.pane)).collect();

        self.splitview().dispose();
        // `clearNode(this.element)`: o split view já saiu; o que sobrar também.
        let leftovers: Vec<NodeId> = self.ui.tree().children(self.element).to_vec();
        for node in leftovers {
            self.ui.remove(node);
        }

        let splitview = SplitView::new(&self.ui, self.element, SplitViewOptions { orientation: Some(orientation), ..SplitViewOptions::default() });
        self.base.register(splitview.clone());
        *self.splitview.borrow_mut() = splitview.clone();
        let boundary_sashes = self.boundary_sashes.borrow().clone();
        self.update_splitview_orthogonal_sashes(boundary_sashes.as_ref());

        let new_orthogonal_size = if orientation == Orientation::Vertical { width } else { height };
        let new_size = if orientation == Orientation::Horizontal { width } else { height };

        let panes: Vec<Rc<Pane>> = self.pane_items.borrow().iter().map(|item| item.pane.clone()).collect();
        for (index, pane) in panes.into_iter().enumerate() {
            pane.set_orthogonal_size(new_orthogonal_size);
            pane.set_orientation(orientation);

            let view_size = if self.size.get() == 0.0 { 0.0 } else { (new_size * pane_sizes[index]) / self.size.get() };
            let view: ViewRef<()> = pane;
            splitview.add_view(view, view_size, Some(index), false);
        }

        self.size.set(new_size);
        self.orthogonal_size.set(new_orthogonal_size);

        splitview.layout(self.size.get(), None);
    }

    fn setup_animation(&self) {
        if let Some(timer) = self.animation_timer.take() {
            runtime::clear_timeout(timer);
        }

        self.ui.tree_mut().add_class(self.element, "animated");

        let (weak, ui) = (self.weak_self.clone(), self.ui.clone());
        self.animation_timer.set(Some(runtime::set_timeout(
            move || {
                if let Some(pane_view) = weak.upgrade() {
                    pane_view.animation_timer.set(None);
                    ui.tree_mut().remove_class(pane_view.element, "animated");
                }
            },
            200,
        )));
    }

    fn pane_header_elements(&self) -> Vec<NodeId> {
        let tree = self.ui.tree();
        tree.subtree(self.element).into_iter().filter(|n| tree.has_class(*n, "pane-header")).collect()
    }

    fn focus_previous(&self) {
        let headers = self.pane_header_elements();
        let Some(index) = self.ui.focused().and_then(|f| headers.iter().position(|h| *h == f)) else { return };

        self.ui.focus(headers[index.saturating_sub(1)]);
    }

    fn focus_next(&self) {
        let headers = self.pane_header_elements();
        let Some(index) = self.ui.focused().and_then(|f| headers.iter().position(|h| *h == f)) else { return };

        self.ui.focus(headers[(index + 1).min(headers.len() - 1)]);
    }
}

impl IDisposable for PaneView {
    fn dispose(&self) {
        if let Some(timer) = self.animation_timer.take() {
            runtime::clear_timeout(timer);
        }
        self.base.dispose();

        let items: Vec<PaneItem> = self.pane_items.borrow_mut().drain(..).collect();
        for item in items {
            item.disposable.dispose();
        }
        self.ui.remove(self.element);
    }

    fn constructor_name(&self) -> &'static str {
        "PaneView"
    }
}
