//! Porte de `src/vs/base/browser/ui/grid/gridview.ts`: o [`GridView`], um layout flex bidimensional feito de
//! [`SplitView`]s ortogonais entre si, com API por índices (a API por vistas, mais amigável, é o `Grid`).
//!
//! A árvore é a do original: [`BranchNode`] (um split view cujos filhos são nós) e [`LeafNode`] (uma view do
//! usuário), alternando a orientação a cada nível. O mesmo vale para o algoritmo de layout (cada nó repassa ao
//! filho o tamanho e a posição absoluta), para o 2x2 de sashes ligados (`trySet2x2`), para a maximização
//! (esconde tudo menos uma view e lembra o que estava visível), para a serialização e para os sashes de
//! fronteira (`boundarySashes`).
//!
//! Do DOM, só o contêiner de cada nó: `.monaco-grid-view` (`overflow: hidden`, 100% x 100%) contendo o nó raiz, e
//! cada `.monaco-grid-branch-node` (100% x 100%) contendo o `.monaco-split-view2` do ramo.
//!
//! Divergência: as ligações do 2x2 (`linkedWidthNode`, `linkedHeightNode`, `linkedSash`) são `Weak`, porque o
//! par se aponta mutuamente (um ciclo de `Rc`); os donos continuam sendo os ramos.

use std::cell::{Cell, RefCell};
use std::collections::HashSet;
use std::rc::{Rc, Weak};

use je_base::errors::{self, Error};
use je_base::event::{Emitter, Event as BaseEvent, Relay, any};
use je_base::lifecycle::{DisposableRef, DisposableStore, IDisposable, none, to_disposable};
use je_ui::{NodeId, Role, Ui};

pub use crate::sash::Orientation;
use crate::sash::{BoundarySashes, Sash};
pub use crate::splitview::{LayoutPriority, Sizing, ViewSize};
use crate::splitview::{SplitView, SplitViewDescriptor, SplitViewOptions, SplitViewStyles, SplitViewView, ViewDescriptor, ViewRef};

/// O `IGridViewStyles` (os do split view).
pub type GridViewStyles = SplitViewStyles;

/// O `IViewSize`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ViewDimensions {
    pub width: f64,
    pub height: f64,
}

/// O `Partial<IViewSize>`.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct PartialViewDimensions {
    pub width: Option<f64>,
    pub height: Option<f64>,
}

/// Os sashes de fronteira relativos à orientação do nó (início e fim, ortogonais).
#[derive(Clone, Default)]
pub struct RelativeBoundarySashes {
    pub start: Option<Rc<Sash>>,
    pub end: Option<Rc<Sash>>,
    pub orthogonal_start: Option<Rc<Sash>>,
    pub orthogonal_end: Option<Rc<Sash>>,
}

impl RelativeBoundarySashes {
    fn same_as(&self, other: &Self) -> bool {
        fn same(a: &Option<Rc<Sash>>, b: &Option<Rc<Sash>>) -> bool {
            match (a, b) {
                (Some(a), Some(b)) => Rc::ptr_eq(a, b),
                (None, None) => true,
                _ => false,
            }
        }
        same(&self.start, &other.start) && same(&self.end, &other.end) && same(&self.orthogonal_start, &other.orthogonal_start) && same(&self.orthogonal_end, &other.orthogonal_end)
    }
}

/// O `IView` de uma view do [`GridView`].
pub trait GridViewView {
    /// O nó desta view.
    fn element(&self) -> NodeId;

    /// A largura mínima (`0` se não houver).
    fn minimum_width(&self) -> f64;

    /// A largura máxima (`f64::INFINITY` se não houver).
    fn maximum_width(&self) -> f64;

    fn minimum_height(&self) -> f64;

    fn maximum_height(&self) -> f64;

    /// A prioridade quando o layout roda (só vale com layout não proporcional).
    fn priority(&self) -> Option<LayoutPriority> {
        None
    }

    fn proportional_layout(&self) -> Option<bool> {
        None
    }

    fn snap(&self) -> Option<bool> {
        None
    }

    /// Dispara quando alguma restrição muda; `Some` traz o tamanho preferido da view para o relayout.
    fn on_did_change(&self) -> BaseEvent<Option<ViewDimensions>>;

    fn layout(&self, width: f64, height: f64, top: f64, left: f64);

    fn set_visible(&self, _visible: bool) {}

    /// Chamado quando a view está numa borda do grid e os sashes de fronteira mudam.
    fn set_boundary_sashes(&self, _sashes: BoundarySashes) {}
}

pub type GridViewRef = Rc<dyn GridViewView>;

/// O `Box`: onde a view fica no grid.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct GridBox {
    pub top: f64,
    pub left: f64,
    pub width: f64,
    pub height: f64,
}

/// O `GridLeafNode`.
#[derive(Clone)]
pub struct GridLeafNode<V> {
    pub view: V,
    pub box_: GridBox,
    pub cached_visible_size: Option<f64>,
    pub maximized: bool,
}

/// O `GridBranchNode`.
#[derive(Clone)]
pub struct GridBranchNode<V> {
    pub children: Vec<GridNode<V>>,
    pub box_: GridBox,
}

/// O `GridNode`.
#[derive(Clone)]
pub enum GridNode<V> {
    Leaf(GridLeafNode<V>),
    Branch(GridBranchNode<V>),
}

impl<V> GridNode<V> {
    pub fn box_(&self) -> GridBox {
        match self {
            Self::Leaf(leaf) => leaf.box_,
            Self::Branch(branch) => branch.box_,
        }
    }

    /// Troca o tipo da view de cada folha (o `Grid<T>` devolve as views `T`, o `GridView` as genéricas).
    pub fn map_views<W>(&self, f: &impl Fn(&V) -> W) -> GridNode<W> {
        match self {
            Self::Leaf(leaf) => GridNode::Leaf(GridLeafNode { view: f(&leaf.view), box_: leaf.box_, cached_visible_size: leaf.cached_visible_size, maximized: leaf.maximized }),
            Self::Branch(branch) => GridNode::Branch(GridBranchNode { children: branch.children.iter().map(|c| c.map_views(f)).collect(), box_: branch.box_ }),
        }
    }
}

/// O `ISerializedNode`. `D` é o tipo do `data` de cada folha. `size` é `Option` porque a raiz de um
/// `createSerializedGrid` não tem tamanho (o `undefined` do original).
#[derive(Clone, Debug, PartialEq)]
pub enum SerializedNode<D> {
    Leaf { data: D, size: Option<f64>, visible: Option<bool>, maximized: Option<bool> },
    Branch { data: Vec<SerializedNode<D>>, size: Option<f64>, visible: Option<bool> },
}

impl<D> SerializedNode<D> {
    pub fn size(&self) -> Option<f64> {
        match self {
            Self::Leaf { size, .. } | Self::Branch { size, .. } => *size,
        }
    }

    pub fn visible(&self) -> Option<bool> {
        match self {
            Self::Leaf { visible, .. } | Self::Branch { visible, .. } => *visible,
        }
    }
}

/// O `ISerializedGridView`.
#[derive(Clone, Debug, PartialEq)]
pub struct SerializedGridView<D> {
    pub root: SerializedNode<D>,
    pub orientation: Orientation,
    pub width: f64,
    pub height: f64,
}

pub fn orthogonal(orientation: Orientation) -> Orientation {
    if orientation == Orientation::Vertical { Orientation::Horizontal } else { Orientation::Vertical }
}

/// O `GridLocation`: a sequência de índices, um por split view, da raiz até a view.
pub type GridLocation = Vec<usize>;

fn throw(message: &str) -> ! {
    errors::throw(Error::new(message))
}

/// O `LayoutController`: o layout só chega às views depois da primeira chamada a [`GridView::layout`].
struct LayoutController {
    is_layout_enabled: Cell<bool>,
}

#[derive(Clone, Copy)]
pub(crate) struct LayoutContext {
    orthogonal_size: f64,
    absolute_offset: f64,
    absolute_orthogonal_offset: f64,
    absolute_size: f64,
    absolute_orthogonal_size: f64,
}

fn to_absolute_boundary_sashes(sashes: &RelativeBoundarySashes, orientation: Orientation) -> BoundarySashes {
    if orientation == Orientation::Horizontal {
        BoundarySashes { left: sashes.start.clone(), right: sashes.end.clone(), top: sashes.orthogonal_start.clone(), bottom: sashes.orthogonal_end.clone() }
    } else {
        BoundarySashes { top: sashes.start.clone(), bottom: sashes.end.clone(), left: sashes.orthogonal_start.clone(), right: sashes.orthogonal_end.clone() }
    }
}

fn from_absolute_boundary_sashes(sashes: &BoundarySashes, orientation: Orientation) -> RelativeBoundarySashes {
    if orientation == Orientation::Horizontal {
        RelativeBoundarySashes { start: sashes.left.clone(), end: sashes.right.clone(), orthogonal_start: sashes.top.clone(), orthogonal_end: sashes.bottom.clone() }
    } else {
        RelativeBoundarySashes { start: sashes.top.clone(), end: sashes.bottom.clone(), orthogonal_start: sashes.left.clone(), orthogonal_end: sashes.right.clone() }
    }
}

fn validate_index(index: usize, num_children: usize) -> usize {
    if index > num_children {
        throw("Invalid index");
    }
    index
}

/// Um nó da árvore: ramo ou folha.
#[derive(Clone)]
pub(crate) enum Node {
    Branch(Rc<BranchNode>),
    Leaf(Rc<LeafNode>),
}

impl Node {
    fn view_ref(&self) -> ViewRef<LayoutContext> {
        match self {
            Self::Branch(b) => b.clone(),
            Self::Leaf(l) => l.clone(),
        }
    }

    /// A identidade do nó (o `===` do original).
    fn id(&self) -> usize {
        match self {
            Self::Branch(b) => Rc::as_ptr(b) as usize,
            Self::Leaf(l) => Rc::as_ptr(l) as usize,
        }
    }

    fn size(&self) -> f64 {
        match self {
            Self::Branch(b) => b.size.get(),
            Self::Leaf(l) => l.size.get(),
        }
    }

    fn width(&self) -> f64 {
        match self {
            Self::Branch(b) => b.width(),
            Self::Leaf(l) => l.width(),
        }
    }

    fn height(&self) -> f64 {
        match self {
            Self::Branch(b) => b.height(),
            Self::Leaf(l) => l.height(),
        }
    }

    fn top(&self) -> f64 {
        match self {
            Self::Branch(b) => b.top(),
            Self::Leaf(l) => l.top(),
        }
    }

    fn left(&self) -> f64 {
        match self {
            Self::Branch(b) => b.left(),
            Self::Leaf(l) => l.left(),
        }
    }

    fn minimum_orthogonal_size(&self) -> f64 {
        match self {
            Self::Branch(b) => b.minimum_orthogonal_size(),
            Self::Leaf(l) => l.minimum_orthogonal_size(),
        }
    }

    fn maximum_orthogonal_size(&self) -> f64 {
        match self {
            Self::Branch(b) => b.maximum_orthogonal_size(),
            Self::Leaf(l) => l.maximum_orthogonal_size(),
        }
    }

    fn priority(&self) -> Option<LayoutPriority> {
        match self {
            Self::Branch(b) => Some(b.priority()),
            Self::Leaf(l) => l.view.priority(),
        }
    }

    fn proportional_layout(&self) -> bool {
        match self {
            Self::Branch(b) => b.proportional_layout(),
            Self::Leaf(l) => l.view.proportional_layout().unwrap_or(true),
        }
    }

    fn on_did_change(&self) -> BaseEvent<Option<f64>> {
        match self {
            Self::Branch(b) => b.on_did_change.event(),
            Self::Leaf(l) => l.on_did_change.clone(),
        }
    }

    fn on_did_scroll(&self) -> BaseEvent<()> {
        match self {
            Self::Branch(b) => b.on_did_scroll.event(),
            Self::Leaf(_) => BaseEvent::none(),
        }
    }

    fn on_did_sash_reset(&self) -> BaseEvent<GridLocation> {
        match self {
            Self::Branch(b) => b.on_did_sash_reset.event(),
            Self::Leaf(_) => BaseEvent::none(),
        }
    }

    fn boundary_sashes(&self) -> RelativeBoundarySashes {
        match self {
            Self::Branch(b) => b.boundary_sashes.borrow().clone(),
            Self::Leaf(l) => l.boundary_sashes.borrow().clone(),
        }
    }

    fn set_boundary_sashes(&self, sashes: RelativeBoundarySashes) {
        match self {
            Self::Branch(b) => b.set_boundary_sashes(sashes),
            Self::Leaf(l) => l.set_boundary_sashes(sashes),
        }
    }

    fn set_visible(&self, visible: bool) {
        match self {
            Self::Branch(b) => b.set_visible_all(visible),
            Self::Leaf(l) => l.view.set_visible(visible),
        }
    }

    fn dispose(&self) {
        match self {
            Self::Branch(b) => b.dispose(),
            Self::Leaf(l) => l.dispose(),
        }
    }
}

/// O `INodeDescriptor`.
struct NodeDescriptor {
    node: Node,
    visible: Option<bool>,
}

pub(crate) struct BranchNode {
    ui: Ui,
    element: NodeId,
    orientation: Orientation,
    layout_controller: Rc<LayoutController>,
    styles: Cell<GridViewStyles>,
    splitview_proportional_layout: bool,
    children: RefCell<Vec<Node>>,
    splitview: Rc<SplitView<LayoutContext>>,
    size: Cell<f64>,
    orthogonal_size: Cell<f64>,
    absolute_offset: Cell<f64>,
    absolute_orthogonal_offset: Cell<f64>,
    absolute_orthogonal_size: Cell<f64>,
    on_did_change: Rc<Emitter<Option<f64>>>,
    on_did_visibility_change: Rc<Emitter<bool>>,
    children_visibility_change_disposable: Rc<DisposableStore>,
    on_did_scroll: Rc<Emitter<()>>,
    on_did_scroll_disposable: RefCell<DisposableRef>,
    children_change_disposable: RefCell<DisposableRef>,
    on_did_sash_reset: Rc<Emitter<GridLocation>>,
    splitview_sash_reset_disposable: RefCell<DisposableRef>,
    children_sash_reset_disposable: RefCell<DisposableRef>,
    boundary_sashes: RefCell<RelativeBoundarySashes>,
    edge_snapping: Cell<bool>,
    weak_self: Weak<BranchNode>,
    disposed: Cell<bool>,
}

impl BranchNode {
    #[allow(clippy::too_many_arguments)]
    fn new(
        ui: &Ui,
        orientation: Orientation,
        layout_controller: Rc<LayoutController>,
        styles: GridViewStyles,
        splitview_proportional_layout: bool,
        size: f64,
        orthogonal_size: f64,
        edge_snapping: bool,
        child_descriptors: Option<Vec<NodeDescriptor>>,
    ) -> Rc<BranchNode> {
        let element = {
            let mut tree = ui.tree_mut();
            let element = tree.create_detached(Role::Generic);
            tree.add_class(element, "monaco-grid-branch-node");
            tree.set_fill(element, true, true);
            element
        };

        let (splitview, children): (Rc<SplitView<LayoutContext>>, Vec<Node>) = match child_descriptors {
            // Comportamento normal: ainda sem filhos, só monta o split view.
            None => {
                let splitview = SplitView::new(ui, element, SplitViewOptions { orientation: Some(orientation), styles: Some(styles), proportional_layout: Some(splitview_proportional_layout), ..SplitViewOptions::default() });
                splitview.layout(size, Some(LayoutContext { orthogonal_size, absolute_offset: 0.0, absolute_orthogonal_offset: 0.0, absolute_size: size, absolute_orthogonal_size: orthogonal_size }));
                (splitview, Vec::new())
            }
            // Reconstrução: remonta um split view a partir dos descritores.
            Some(descriptors) => {
                let descriptor = SplitViewDescriptor {
                    views: descriptors.iter().map(|d| ViewDescriptor { view: d.node.view_ref(), size: d.node.size(), visible: Some(d.visible != Some(false)) }).collect(),
                    size: orthogonal_size,
                };
                let children = descriptors.into_iter().map(|d| d.node).collect();
                let options = SplitViewOptions { orientation: Some(orientation), styles: Some(styles), proportional_layout: Some(splitview_proportional_layout), descriptor: Some(descriptor), ..SplitViewOptions::default() };
                (SplitView::new(ui, element, options), children)
            }
        };

        let on_did_sash_reset = Emitter::<GridLocation>::new();
        let forward = on_did_sash_reset.clone();
        let splitview_sash_reset_disposable = splitview.on_did_sash_reset().listen(move |i| forward.fire(vec![*i]));

        let this = Rc::new_cyclic(|weak| BranchNode {
            ui: ui.clone(),
            element,
            orientation,
            layout_controller,
            styles: Cell::new(styles),
            splitview_proportional_layout,
            children: RefCell::new(children),
            splitview,
            size: Cell::new(size),
            orthogonal_size: Cell::new(orthogonal_size),
            absolute_offset: Cell::new(0.0),
            absolute_orthogonal_offset: Cell::new(0.0),
            absolute_orthogonal_size: Cell::new(0.0),
            on_did_change: Emitter::new(),
            on_did_visibility_change: Emitter::new(),
            children_visibility_change_disposable: DisposableStore::new(),
            on_did_scroll: Emitter::new(),
            on_did_scroll_disposable: RefCell::new(none()),
            children_change_disposable: RefCell::new(none()),
            on_did_sash_reset,
            splitview_sash_reset_disposable: RefCell::new(splitview_sash_reset_disposable),
            children_sash_reset_disposable: RefCell::new(none()),
            boundary_sashes: RefCell::new(RelativeBoundarySashes::default()),
            edge_snapping: Cell::new(edge_snapping),
            weak_self: weak.clone(),
            disposed: Cell::new(false),
        });

        // Na reconstrução, os filhos recebem os sashes de fronteira (`last` nunca é verdadeiro no
        // original: `index === this.children.length`).
        let sashes = this.splitview.sashes();
        let children = this.children();
        for (index, node) in children.iter().enumerate() {
            let first = index == 0;
            let boundary = this.boundary_sashes.borrow().clone();
            node.set_boundary_sashes(RelativeBoundarySashes {
                start: boundary.orthogonal_start.clone(),
                end: boundary.orthogonal_end.clone(),
                orthogonal_start: if first { boundary.start.clone() } else { sashes.get(index - 1).cloned() },
                orthogonal_end: sashes.get(index).cloned(),
            });
        }

        this.update_children_events();
        this
    }

    fn children(&self) -> Vec<Node> {
        self.children.borrow().clone()
    }

    fn child(&self, index: usize) -> Node {
        self.children.borrow()[index].clone()
    }

    fn width(&self) -> f64 {
        if self.orientation == Orientation::Horizontal { self.size.get() } else { self.orthogonal_size.get() }
    }

    fn height(&self) -> f64 {
        if self.orientation == Orientation::Horizontal { self.orthogonal_size.get() } else { self.size.get() }
    }

    fn top(&self) -> f64 {
        if self.orientation == Orientation::Horizontal { self.absolute_offset.get() } else { self.absolute_orthogonal_offset.get() }
    }

    fn left(&self) -> f64 {
        if self.orientation == Orientation::Horizontal { self.absolute_orthogonal_offset.get() } else { self.absolute_offset.get() }
    }

    fn minimum_size(&self) -> f64 {
        let children = self.children();
        if children.is_empty() {
            return 0.0;
        }
        children.iter().enumerate().map(|(index, c)| if self.splitview.is_view_visible(index) { c.minimum_orthogonal_size() } else { 0.0 }).fold(f64::NEG_INFINITY, f64::max)
    }

    fn maximum_size(&self) -> f64 {
        self.children().iter().enumerate().map(|(index, c)| if self.splitview.is_view_visible(index) { c.maximum_orthogonal_size() } else { f64::INFINITY }).fold(f64::INFINITY, f64::min)
    }

    fn priority(&self) -> LayoutPriority {
        let children = self.children();
        if children.is_empty() {
            return LayoutPriority::Normal;
        }

        let priorities: Vec<LayoutPriority> = children.iter().map(|c| c.priority().unwrap_or(LayoutPriority::Normal)).collect();

        if priorities.contains(&LayoutPriority::High) {
            LayoutPriority::High
        } else if priorities.contains(&LayoutPriority::Low) {
            LayoutPriority::Low
        } else {
            LayoutPriority::Normal
        }
    }

    fn proportional_layout(&self) -> bool {
        self.children().iter().all(Node::proportional_layout)
    }

    fn minimum_orthogonal_size(&self) -> f64 {
        self.splitview.minimum_size()
    }

    fn maximum_orthogonal_size(&self) -> f64 {
        self.splitview.maximum_size()
    }

    fn minimum_width(&self) -> f64 {
        if self.orientation == Orientation::Horizontal { self.minimum_orthogonal_size() } else { self.minimum_size() }
    }

    fn minimum_height(&self) -> f64 {
        if self.orientation == Orientation::Horizontal { self.minimum_size() } else { self.minimum_orthogonal_size() }
    }

    fn maximum_height(&self) -> f64 {
        if self.orientation == Orientation::Horizontal { self.maximum_size() } else { self.maximum_orthogonal_size() }
    }

    /// Ninguém lê: o `GridView.maximumWidth` do original devolve o `maximumHeight` da raiz (ver lá).
    #[allow(dead_code)]
    fn maximum_width(&self) -> f64 {
        if self.orientation == Orientation::Horizontal { self.maximum_orthogonal_size() } else { self.maximum_size() }
    }

    fn set_boundary_sashes(&self, boundary_sashes: RelativeBoundarySashes) {
        if self.boundary_sashes.borrow().same_as(&boundary_sashes) {
            return;
        }

        *self.boundary_sashes.borrow_mut() = boundary_sashes.clone();

        self.splitview.set_orthogonal_start_sash(boundary_sashes.orthogonal_start.clone());
        self.splitview.set_orthogonal_end_sash(boundary_sashes.orthogonal_end.clone());

        let children = self.children();
        for (index, child) in children.iter().enumerate() {
            let first = index == 0;
            let last = index == children.len() - 1;
            let current = child.boundary_sashes();

            child.set_boundary_sashes(RelativeBoundarySashes {
                start: boundary_sashes.orthogonal_start.clone(),
                end: boundary_sashes.orthogonal_end.clone(),
                orthogonal_start: if first { boundary_sashes.start.clone() } else { current.orthogonal_start },
                orthogonal_end: if last { boundary_sashes.end.clone() } else { current.orthogonal_end },
            });
        }
    }

    fn edge_snapping(&self) -> bool {
        self.edge_snapping.get()
    }

    fn set_edge_snapping(&self, edge_snapping: bool) {
        if self.edge_snapping.get() == edge_snapping {
            return;
        }

        self.edge_snapping.set(edge_snapping);

        for child in self.children() {
            if let Node::Branch(branch) = child {
                branch.set_edge_snapping(edge_snapping);
            }
        }

        self.update_splitview_edge_snapping_enablement();
    }

    fn style(&self, styles: GridViewStyles) {
        self.styles.set(styles);
        self.splitview.style(styles);

        for child in self.children() {
            if let Node::Branch(branch) = child {
                branch.style(styles);
            }
        }
    }

    fn layout_node(&self, size: f64, offset: f64, ctx: Option<&LayoutContext>) {
        if !self.layout_controller.is_layout_enabled.get() {
            return;
        }

        let Some(ctx) = ctx else { throw("Invalid state") };

        // Os ramos trocam as direções normal e ortogonal.
        self.size.set(ctx.orthogonal_size);
        self.orthogonal_size.set(size);
        self.absolute_offset.set(ctx.absolute_offset + offset);
        self.absolute_orthogonal_offset.set(ctx.absolute_orthogonal_offset);
        self.absolute_orthogonal_size.set(ctx.absolute_orthogonal_size);

        self.splitview.layout(
            ctx.orthogonal_size,
            Some(LayoutContext {
                orthogonal_size: size,
                absolute_offset: self.absolute_orthogonal_offset.get(),
                absolute_orthogonal_offset: self.absolute_offset.get(),
                absolute_size: ctx.absolute_orthogonal_size,
                absolute_orthogonal_size: ctx.absolute_size,
            }),
        );

        self.update_splitview_edge_snapping_enablement();
    }

    fn set_visible_all(&self, visible: bool) {
        for child in self.children() {
            child.set_visible(visible);
        }
    }

    fn add_child(&self, node: Node, size: impl Into<ViewSize>, index: usize, skip_layout: bool) {
        let index = validate_index(index, self.children.borrow().len());

        self.splitview.add_view(node.view_ref(), size, Some(index), skip_layout);
        self.children.borrow_mut().insert(index, node);

        self.update_boundary_sashes();
        self.on_did_children_change();
    }

    fn remove_child(&self, index: usize, sizing: Option<Sizing>) -> Node {
        let index = validate_index(index, self.children.borrow().len());

        self.splitview.remove_view(index, sizing);
        let result = self.children.borrow_mut().remove(index);

        self.update_boundary_sashes();
        self.on_did_children_change();

        result
    }

    fn remove_all_children(&self) -> Vec<Node> {
        self.splitview.remove_all_views();

        let result: Vec<Node> = self.children.borrow_mut().drain(..).collect();

        self.update_boundary_sashes();
        self.on_did_children_change();

        result
    }

    fn move_child(&self, from: usize, mut to: usize) {
        let len = self.children.borrow().len();
        let from = validate_index(from, len);
        to = validate_index(to, len);

        if from == to {
            return;
        }

        if from < to {
            to -= 1;
        }

        self.splitview.move_view(from, to);
        let mut children = self.children.borrow_mut();
        let moved = children.remove(from);
        children.insert(to, moved);
        drop(children);

        self.update_boundary_sashes();
        self.on_did_children_change();
    }

    fn swap_children(&self, from: usize, to: usize) {
        let len = self.children.borrow().len();
        let from = validate_index(from, len);
        let to = validate_index(to, len);

        if from == to {
            return;
        }

        self.splitview.swap_views(from, to);

        // No original a troca dos `boundarySashes` é um no-op (`[a, b] = [a, b]`), e continua sendo.

        // Troca os filhos.
        self.children.borrow_mut().swap(from, to);

        self.on_did_children_change();
    }

    fn resize_child(&self, index: usize, size: f64) {
        let index = validate_index(index, self.children.borrow().len());

        self.splitview.resize_view(index, size);
    }

    fn is_child_expanded(&self, index: usize) -> bool {
        self.splitview.is_view_expanded(index)
    }

    fn distribute_view_sizes(&self, recursive: bool) {
        self.splitview.distribute_view_sizes();

        if recursive {
            for child in self.children() {
                if let Node::Branch(branch) = child {
                    branch.distribute_view_sizes(true);
                }
            }
        }
    }

    fn get_child_size(&self, index: usize) -> f64 {
        let index = validate_index(index, self.children.borrow().len());

        self.splitview.get_view_size(index)
    }

    fn is_child_visible(&self, index: usize) -> bool {
        let index = validate_index(index, self.children.borrow().len());

        self.splitview.is_view_visible(index)
    }

    fn set_child_visible(&self, index: usize, visible: bool) {
        let index = validate_index(index, self.children.borrow().len());

        if self.splitview.is_view_visible(index) == visible {
            return;
        }

        let were_all_children_hidden = self.splitview.content_size() == 0.0;
        self.splitview.set_view_visible(index, visible);
        let are_all_children_hidden = self.splitview.content_size() == 0.0;

        // Se todos os filhos estão escondidos, o pai esconde o split view inteiro; se o split view inteiro
        // está escondido, o pai o mostra quando um filho é mostrado.
        if (visible && were_all_children_hidden) || (!visible && are_all_children_hidden) {
            self.on_did_visibility_change.fire(visible);
        }
    }

    fn get_child_cached_visible_size(&self, index: usize) -> Option<f64> {
        let index = validate_index(index, self.children.borrow().len());

        self.splitview.get_view_cached_visible_size(index)
    }

    fn update_boundary_sashes(&self) {
        let sashes = self.splitview.sashes();
        let children = self.children();
        let boundary = self.boundary_sashes.borrow().clone();
        for (i, child) in children.iter().enumerate() {
            child.set_boundary_sashes(RelativeBoundarySashes {
                start: boundary.orthogonal_start.clone(),
                end: boundary.orthogonal_end.clone(),
                orthogonal_start: if i == 0 { boundary.start.clone() } else { sashes.get(i - 1).cloned() },
                orthogonal_end: if i == children.len() - 1 { boundary.end.clone() } else { sashes.get(i).cloned() },
            });
        }
    }

    fn on_did_children_change(&self) {
        self.update_children_events();
        self.on_did_change.fire(None);
    }

    fn update_children_events(&self) {
        let children = self.children();

        let on_did_children_change = any(children.iter().map(Node::on_did_change).collect()).map(|_| None::<f64>, None);
        let previous = std::mem::replace(&mut *self.children_change_disposable.borrow_mut(), none());
        previous.dispose();
        let emitter = self.on_did_change.clone();
        *self.children_change_disposable.borrow_mut() = on_did_children_change.listen(move |v| emitter.fire(*v));

        let on_did_children_sash_reset = any(
            children
                .iter()
                .enumerate()
                .map(|(i, c)| {
                    c.on_did_sash_reset().map(
                        move |location: &GridLocation| {
                            let mut result = vec![i];
                            result.extend(location);
                            result
                        },
                        None,
                    )
                })
                .collect(),
        );
        let previous = std::mem::replace(&mut *self.children_sash_reset_disposable.borrow_mut(), none());
        previous.dispose();
        let emitter = self.on_did_sash_reset.clone();
        *self.children_sash_reset_disposable.borrow_mut() = on_did_children_sash_reset.listen(move |l| emitter.fire(l.clone()));

        let mut scroll_events = vec![self.splitview.on_did_scroll().signal()];
        scroll_events.extend(children.iter().map(Node::on_did_scroll));
        let on_did_scroll = any(scroll_events);
        let previous = std::mem::replace(&mut *self.on_did_scroll_disposable.borrow_mut(), none());
        previous.dispose();
        let emitter = self.on_did_scroll.clone();
        *self.on_did_scroll_disposable.borrow_mut() = on_did_scroll.listen(move |_| emitter.fire(()));

        self.children_visibility_change_disposable.clear();
        for (index, child) in children.iter().enumerate() {
            if let Node::Branch(branch) = child {
                let weak = self.weak_self.clone();
                self.children_visibility_change_disposable.add(branch.on_did_visibility_change.event().listen(move |visible| {
                    if let Some(this) = weak.upgrade() {
                        this.set_child_visible(index, *visible);
                    }
                }));
            }
        }
    }

    /// Tenta travar os sashes dos dois ramos para o grid se comportar como uma matriz 2x2 (`trySet2x2`).
    fn try_set_2x2(&self, other: &BranchNode) -> DisposableRef {
        let (children, other_children) = (self.children(), other.children());
        if children.len() != 2 || other_children.len() != 2 {
            return none();
        }

        if self.get_child_size(0) != other.get_child_size(0) {
            return none();
        }

        let (Node::Leaf(first_child), Node::Leaf(second_child)) = (&children[0], &children[1]) else { return none() };
        let (Node::Leaf(other_first_child), Node::Leaf(other_second_child)) = (&other_children[0], &other_children[1]) else { return none() };

        if self.orientation == Orientation::Vertical {
            second_child.set_linked_width_node(Some(other_first_child));
            other_first_child.set_linked_height_node(Some(first_child));
            first_child.set_linked_width_node(Some(other_second_child));
            other_second_child.set_linked_height_node(Some(second_child));
            other_second_child.set_linked_width_node(Some(first_child));
            first_child.set_linked_height_node(Some(other_first_child));
            other_first_child.set_linked_width_node(Some(second_child));
            second_child.set_linked_height_node(Some(other_second_child));
        } else {
            other_first_child.set_linked_width_node(Some(second_child));
            second_child.set_linked_height_node(Some(first_child));
            other_second_child.set_linked_width_node(Some(first_child));
            first_child.set_linked_height_node(Some(second_child));
            first_child.set_linked_width_node(Some(other_second_child));
            other_second_child.set_linked_height_node(Some(other_first_child));
            second_child.set_linked_width_node(Some(other_first_child));
            other_first_child.set_linked_height_node(Some(other_second_child));
        }

        let my_sash = self.splitview.sashes()[0].clone();
        let other_sash = other.splitview.sashes()[0].clone();
        my_sash.set_linked_sash(Some(&other_sash));
        other_sash.set_linked_sash(Some(&my_sash));

        self.on_did_change.fire(None);
        other.on_did_change.fire(None);

        let leaves = [first_child.clone(), second_child.clone(), other_first_child.clone(), other_second_child.clone()];
        to_disposable(move || {
            my_sash.set_linked_sash(None);
            other_sash.set_linked_sash(None);
            for leaf in &leaves {
                leaf.set_linked_height_node(None);
                leaf.set_linked_width_node(None);
            }
        })
    }

    fn update_splitview_edge_snapping_enablement(&self) {
        self.splitview.set_start_snapping_enabled(self.edge_snapping.get() || self.absolute_orthogonal_offset.get() > 0.0);
        self.splitview.set_end_snapping_enabled(self.edge_snapping.get() || self.absolute_orthogonal_offset.get() + self.size.get() < self.absolute_orthogonal_size.get());
    }

    fn dispose(&self) {
        // O `flipNode` e o `set root` do original descartam o mesmo ramo duas vezes; o `dispose` do
        // original é idempotente, e aqui a árvore reaproveita ids, então a segunda vez não pode agir.
        if self.disposed.replace(true) {
            return;
        }
        for child in self.children() {
            child.dispose();
        }

        self.on_did_change.dispose();
        self.on_did_scroll.dispose();
        self.on_did_sash_reset.dispose();
        self.on_did_visibility_change.dispose();

        self.children_visibility_change_disposable.dispose();
        self.splitview_sash_reset_disposable.borrow().dispose();
        self.children_sash_reset_disposable.borrow().dispose();
        self.children_change_disposable.borrow().dispose();
        self.on_did_scroll_disposable.borrow().dispose();
        self.splitview.dispose();
        // O contêiner do ramo sai da árvore com o nó (o original o deixa para o coletor de lixo).
        self.ui.remove(self.element);
    }
}

impl SplitViewView<LayoutContext> for BranchNode {
    fn element(&self) -> NodeId {
        self.element
    }

    fn minimum_size(&self) -> f64 {
        BranchNode::minimum_size(self)
    }

    fn maximum_size(&self) -> f64 {
        BranchNode::maximum_size(self)
    }

    fn priority(&self) -> Option<LayoutPriority> {
        Some(BranchNode::priority(self))
    }

    fn proportional_layout(&self) -> Option<bool> {
        Some(BranchNode::proportional_layout(self))
    }

    fn on_did_change(&self) -> BaseEvent<Option<f64>> {
        self.on_did_change.event()
    }

    fn layout(&self, size: f64, offset: f64, context: Option<&LayoutContext>) {
        self.layout_node(size, offset, context);
    }

    fn set_visible(&self, visible: bool) {
        self.set_visible_all(visible);
    }
}

/// Um evento de restrições de view que não dispara quando as restrições não mudam (`createLatchedOnDidChangeViewEvent`).
fn create_latched_on_did_change_view_event(view: &GridViewRef) -> BaseEvent<Option<ViewDimensions>> {
    let (on_did_change_view_constraints, on_did_set_view_size) = view.on_did_change().split(|e| e.is_none(), None);

    let constraints_view = view.clone();
    let constraints = on_did_change_view_constraints.map(
        move |_| {
            let v = &constraints_view;
            [v.minimum_width(), v.maximum_width(), v.minimum_height(), v.maximum_height()]
        },
        None,
    );
    let latched = constraints.latch_by(|a, b| a == b, None).map(|_| None::<ViewDimensions>, None);

    any(vec![on_did_set_view_size, latched])
}

pub(crate) struct LeafNode {
    element: NodeId,
    view: GridViewRef,
    orientation: Orientation,
    layout_controller: Rc<LayoutController>,
    size: Cell<f64>,
    orthogonal_size: Cell<f64>,
    absolute_offset: Cell<f64>,
    absolute_orthogonal_offset: Cell<f64>,
    on_did_linked_width_node_change: Rc<Relay<Option<f64>>>,
    linked_width_node: RefCell<Option<Weak<LeafNode>>>,
    on_did_linked_height_node_change: Rc<Relay<Option<f64>>>,
    linked_height_node: RefCell<Option<Weak<LeafNode>>>,
    on_did_set_linked_node: Rc<Emitter<Option<f64>>>,
    on_did_view_change: BaseEvent<Option<f64>>,
    on_did_change: BaseEvent<Option<f64>>,
    disposables: Rc<DisposableStore>,
    boundary_sashes: RefCell<RelativeBoundarySashes>,
    cached: Cell<(f64, f64, f64, f64)>,
    disposed: Cell<bool>,
}

impl LeafNode {
    fn new(_ui: &Ui, view: GridViewRef, orientation: Orientation, layout_controller: Rc<LayoutController>, orthogonal_size: f64, size: f64) -> Rc<LeafNode> {
        let disposables = DisposableStore::new();

        let on_did_change = create_latched_on_did_change_view_event(&view);
        let on_did_view_change = on_did_change.map(move |e| e.map(|e| if orientation == Orientation::Vertical { e.width } else { e.height }), Some(&disposables));

        let on_did_linked_width_node_change = Relay::new();
        let on_did_linked_height_node_change = Relay::new();
        let on_did_set_linked_node = Emitter::new();
        let on_did_change = any(vec![on_did_view_change.clone(), on_did_set_linked_node.event(), on_did_linked_width_node_change.event(), on_did_linked_height_node_change.event()]);

        // O `view.element` do original é lido a cada acesso; aqui, uma vez, na criação do nó.
        let element = view.element();

        Rc::new(LeafNode {
            element,
            view,
            orientation,
            layout_controller,
            size: Cell::new(size),
            orthogonal_size: Cell::new(orthogonal_size),
            absolute_offset: Cell::new(0.0),
            absolute_orthogonal_offset: Cell::new(0.0),
            on_did_linked_width_node_change,
            linked_width_node: RefCell::new(None),
            on_did_linked_height_node_change,
            linked_height_node: RefCell::new(None),
            on_did_set_linked_node,
            on_did_view_change,
            on_did_change,
            disposables,
            boundary_sashes: RefCell::new(RelativeBoundarySashes::default()),
            cached: Cell::new((0.0, 0.0, 0.0, 0.0)),
            disposed: Cell::new(false),
        })
    }

    fn linked_width_node(&self) -> Option<Rc<LeafNode>> {
        self.linked_width_node.borrow().as_ref().and_then(Weak::upgrade)
    }

    fn linked_height_node(&self) -> Option<Rc<LeafNode>> {
        self.linked_height_node.borrow().as_ref().and_then(Weak::upgrade)
    }

    fn set_linked_width_node(&self, node: Option<&Rc<LeafNode>>) {
        self.on_did_linked_width_node_change.set_input(node.map_or_else(BaseEvent::none, |n| n.on_did_view_change.clone()));
        *self.linked_width_node.borrow_mut() = node.map(Rc::downgrade);
        self.on_did_set_linked_node.fire(None);
    }

    fn set_linked_height_node(&self, node: Option<&Rc<LeafNode>>) {
        self.on_did_linked_height_node_change.set_input(node.map_or_else(BaseEvent::none, |n| n.on_did_view_change.clone()));
        *self.linked_height_node.borrow_mut() = node.map(Rc::downgrade);
        self.on_did_set_linked_node.fire(None);
    }

    fn width(&self) -> f64 {
        if self.orientation == Orientation::Horizontal { self.orthogonal_size.get() } else { self.size.get() }
    }

    fn height(&self) -> f64 {
        if self.orientation == Orientation::Horizontal { self.size.get() } else { self.orthogonal_size.get() }
    }

    fn top(&self) -> f64 {
        if self.orientation == Orientation::Horizontal { self.absolute_offset.get() } else { self.absolute_orthogonal_offset.get() }
    }

    fn left(&self) -> f64 {
        if self.orientation == Orientation::Horizontal { self.absolute_orthogonal_offset.get() } else { self.absolute_offset.get() }
    }

    fn minimum_width(&self) -> f64 {
        match self.linked_width_node() {
            Some(linked) => linked.view.minimum_width().max(self.view.minimum_width()),
            None => self.view.minimum_width(),
        }
    }

    fn maximum_width(&self) -> f64 {
        match self.linked_width_node() {
            Some(linked) => linked.view.maximum_width().min(self.view.maximum_width()),
            None => self.view.maximum_width(),
        }
    }

    fn minimum_height(&self) -> f64 {
        match self.linked_height_node() {
            Some(linked) => linked.view.minimum_height().max(self.view.minimum_height()),
            None => self.view.minimum_height(),
        }
    }

    fn maximum_height(&self) -> f64 {
        match self.linked_height_node() {
            Some(linked) => linked.view.maximum_height().min(self.view.maximum_height()),
            None => self.view.maximum_height(),
        }
    }

    fn minimum_size(&self) -> f64 {
        if self.orientation == Orientation::Horizontal { self.minimum_height() } else { self.minimum_width() }
    }

    fn maximum_size(&self) -> f64 {
        if self.orientation == Orientation::Horizontal { self.maximum_height() } else { self.maximum_width() }
    }

    fn minimum_orthogonal_size(&self) -> f64 {
        if self.orientation == Orientation::Horizontal { self.minimum_width() } else { self.minimum_height() }
    }

    fn maximum_orthogonal_size(&self) -> f64 {
        if self.orientation == Orientation::Horizontal { self.maximum_width() } else { self.maximum_height() }
    }

    fn set_boundary_sashes(&self, boundary_sashes: RelativeBoundarySashes) {
        *self.boundary_sashes.borrow_mut() = boundary_sashes.clone();

        self.view.set_boundary_sashes(to_absolute_boundary_sashes(&boundary_sashes, self.orientation));
    }

    fn layout_node(&self, size: f64, offset: f64, ctx: Option<&LayoutContext>) {
        if !self.layout_controller.is_layout_enabled.get() {
            return;
        }

        let Some(ctx) = ctx else { throw("Invalid state") };

        self.size.set(size);
        self.orthogonal_size.set(ctx.orthogonal_size);
        self.absolute_offset.set(ctx.absolute_offset + offset);
        self.absolute_orthogonal_offset.set(ctx.absolute_orthogonal_offset);

        self.layout_cached(self.width(), self.height(), self.top(), self.left());
    }

    fn layout_cached(&self, width: f64, height: f64, top: f64, left: f64) {
        if self.cached.get() == (width, height, top, left) {
            return;
        }

        self.cached.set((width, height, top, left));
        self.view.layout(width, height, top, left);
    }

    fn dispose(&self) {
        if self.disposed.replace(true) {
            return;
        }
        self.on_did_set_linked_node.dispose();
        self.disposables.dispose();
    }
}

impl SplitViewView<LayoutContext> for LeafNode {
    fn element(&self) -> NodeId {
        self.element
    }

    fn minimum_size(&self) -> f64 {
        LeafNode::minimum_size(self)
    }

    fn maximum_size(&self) -> f64 {
        LeafNode::maximum_size(self)
    }

    fn priority(&self) -> Option<LayoutPriority> {
        self.view.priority()
    }

    fn proportional_layout(&self) -> Option<bool> {
        Some(self.view.proportional_layout().unwrap_or(true))
    }

    fn snap(&self) -> Option<bool> {
        self.view.snap()
    }

    fn on_did_change(&self) -> BaseEvent<Option<f64>> {
        self.on_did_change.clone()
    }

    fn layout(&self, size: f64, offset: f64, context: Option<&LayoutContext>) {
        self.layout_node(size, offset, context);
    }

    fn set_visible(&self, visible: bool) {
        self.view.set_visible(visible);
    }
}

/// `flipNode`: refaz a árvore com a orientação trocada.
fn flip_node(ui: &Ui, node: &Node, size: f64, orthogonal_size: f64) -> Node {
    match node {
        Node::Branch(branch) => {
            let result = BranchNode::new(ui, orthogonal(branch.orientation), branch.layout_controller.clone(), branch.styles.get(), branch.splitview_proportional_layout, size, orthogonal_size, branch.edge_snapping(), None);

            let mut total_size = 0.0;
            let children = branch.children();

            for (i, child) in children.iter().enumerate().rev() {
                let child_size = match child {
                    Node::Branch(b) => b.orthogonal_size.get(),
                    Node::Leaf(l) => l.size.get(),
                };

                let mut new_size = if branch.size.get() == 0.0 { 0.0 } else { ((size * child_size) / branch.size.get() + 0.5).floor() };
                total_size += new_size;

                // A última view a entrar absorve o erro de arredondamento.
                if i == 0 {
                    new_size += size - total_size;
                }

                result.add_child(flip_node(ui, child, orthogonal_size, new_size), new_size, 0, true);
            }

            branch.dispose();
            Node::Branch(result)
        }
        Node::Leaf(leaf) => {
            // O elemento da view precisa sobreviver ao `dispose` do nó antigo: o novo nó o relê da view.
            let result = LeafNode::new(ui, leaf.view.clone(), orthogonal(leaf.orientation), leaf.layout_controller.clone(), orthogonal_size, 0.0);
            leaf.dispose();
            Node::Leaf(result)
        }
    }
}

/// Opções do [`GridView`].
#[derive(Clone, Copy, Default)]
pub struct GridViewOptions {
    pub styles: Option<GridViewStyles>,
    /// Redimensiona cada view proporcionalmente (padrão `true`).
    pub proportional_layout: Option<bool>,
}

/// O `GridView`.
pub struct GridView {
    ui: Ui,
    element: NodeId,
    styles: Cell<GridViewStyles>,
    proportional_layout: bool,
    root: RefCell<Option<Rc<BranchNode>>>,
    on_did_sash_reset_relay: Rc<Relay<GridLocation>>,
    on_did_scroll_relay: Rc<Relay<()>>,
    on_did_change_relay: Rc<Relay<Option<ViewDimensions>>>,
    boundary_sashes: RefCell<BoundarySashes>,
    /// O controlador de layout garante que o layout só propaga às views depois da primeira chamada a `layout`.
    layout_controller: Rc<LayoutController>,
    disposable_2x2: RefCell<DisposableRef>,
    maximized_node: RefCell<Option<Rc<LeafNode>>>,
    maximized_visible_nodes: RefCell<Option<HashSet<usize>>>,
    on_did_change_view_maximized: Rc<Emitter<bool>>,
    disposed: Cell<bool>,
}

impl GridView {
    /// Cria um `GridView`. O chamador pendura [`GridView::element`] na árvore.
    pub fn new(ui: &Ui, options: GridViewOptions) -> Rc<GridView> {
        let element = {
            let mut tree = ui.tree_mut();
            let element = tree.create_detached(Role::Generic);
            tree.add_class(element, "monaco-grid-view");
            tree.set_fill(element, true, true);
            tree.style_mut(element).clip = true;
            element
        };
        let styles = options.styles.unwrap_or_default();
        let proportional_layout = options.proportional_layout.unwrap_or(true);
        let layout_controller = Rc::new(LayoutController { is_layout_enabled: Cell::new(false) });
        let this = Rc::new(GridView {
            ui: ui.clone(),
            element,
            styles: Cell::new(styles),
            proportional_layout,
            root: RefCell::new(None),
            on_did_sash_reset_relay: Relay::new(),
            on_did_scroll_relay: Relay::new(),
            on_did_change_relay: Relay::new(),
            boundary_sashes: RefCell::new(BoundarySashes::default()),
            layout_controller: layout_controller.clone(),
            disposable_2x2: RefCell::new(none()),
            maximized_node: RefCell::new(None),
            maximized_visible_nodes: RefCell::new(None),
            on_did_change_view_maximized: Emitter::new(),
            disposed: Cell::new(false),
        });
        this.set_root(BranchNode::new(ui, Orientation::Vertical, layout_controller, styles, proportional_layout, 0.0, 0.0, false, None));
        this
    }

    /// O nó do grid (o `.monaco-grid-view`).
    pub fn element(&self) -> NodeId {
        self.element
    }

    fn root(&self) -> Rc<BranchNode> {
        self.root.borrow().clone().expect("o grid sempre tem raiz")
    }

    fn set_root(&self, root: Rc<BranchNode>) {
        let old_root = self.root.borrow_mut().take();

        // `oldRoot.element.remove(); oldRoot.dispose()`: o `dispose` do ramo já tira o contêiner da árvore.
        if let Some(old_root) = old_root {
            old_root.dispose();
        }

        self.ui.tree_mut().append_child(self.element, root.element);
        self.on_did_sash_reset_relay.set_input(root.on_did_sash_reset.event());
        self.on_did_change_relay.set_input(root.on_did_change.event().map(|_| None::<ViewDimensions>, None)); // TODO
        self.on_did_scroll_relay.set_input(root.on_did_scroll.event());
        *self.root.borrow_mut() = Some(root);
    }

    /// Dispara quando o usuário dá duplo clique num sash.
    pub fn on_did_sash_reset(&self) -> BaseEvent<GridLocation> {
        self.on_did_sash_reset_relay.event()
    }

    /// Dispara quando o usuário rola um split view do grid.
    pub fn on_did_scroll(&self) -> BaseEvent<()> {
        self.on_did_scroll_relay.event()
    }

    /// Dispara quando uma view muda as restrições de tamanho.
    pub fn on_did_change(&self) -> BaseEvent<Option<ViewDimensions>> {
        self.on_did_change_relay.event()
    }

    pub fn on_did_change_view_maximized(&self) -> BaseEvent<bool> {
        self.on_did_change_view_maximized.event()
    }

    pub fn width(&self) -> f64 {
        self.root().width()
    }

    pub fn height(&self) -> f64 {
        self.root().height()
    }

    pub fn minimum_width(&self) -> f64 {
        self.root().minimum_width()
    }

    pub fn minimum_height(&self) -> f64 {
        self.root().minimum_height()
    }

    /// No original o `maximumWidth` devolve o `maximumHeight` da raiz (um erro de digitação): mantido.
    pub fn maximum_width(&self) -> f64 {
        self.root().maximum_height()
    }

    pub fn maximum_height(&self) -> f64 {
        self.root().maximum_height()
    }

    pub fn orientation(&self) -> Orientation {
        self.root().orientation
    }

    pub fn boundary_sashes(&self) -> BoundarySashes {
        self.boundary_sashes.borrow().clone()
    }

    /// A orientação do grid: a da raiz.
    pub fn set_orientation(&self, orientation: Orientation) {
        if self.root().orientation == orientation {
            return;
        }

        self.exit_maximized_view();
        let root = self.root();
        let (size, orthogonal_size, absolute_offset, absolute_orthogonal_offset) = (root.size.get(), root.orthogonal_size.get(), root.absolute_offset.get(), root.absolute_orthogonal_offset.get());
        let Node::Branch(flipped) = flip_node(&self.ui, &Node::Branch(root), orthogonal_size, size) else { unreachable!("o ramo vira ramo") };
        self.set_root(flipped);
        self.root().layout_node(
            size,
            0.0,
            Some(&LayoutContext { orthogonal_size, absolute_offset: absolute_orthogonal_offset, absolute_orthogonal_offset: absolute_offset, absolute_size: size, absolute_orthogonal_size: orthogonal_size }),
        );
        self.set_boundary_sashes(self.boundary_sashes());
    }

    /// Os sashes perpendiculares a cada borda do grid; cada interseção ganha um sash de canto.
    pub fn set_boundary_sashes(&self, boundary_sashes: BoundarySashes) {
        *self.boundary_sashes.borrow_mut() = boundary_sashes.clone();
        self.root().set_boundary_sashes(from_absolute_boundary_sashes(&boundary_sashes, self.orientation()));
    }

    /// Liga ou desliga o snap nas bordas de todas as views.
    pub fn set_edge_snapping(&self, edge_snapping: bool) {
        self.root().set_edge_snapping(edge_snapping);
    }

    pub fn style(&self, styles: GridViewStyles) {
        self.styles.set(styles);
        self.root().style(styles);
    }

    /// Faz o layout do grid. `top` e `left` são a origem das posições passadas a [`GridViewView::layout`].
    pub fn layout(&self, width: f64, height: f64, top: f64, left: f64) {
        self.layout_controller.is_layout_enabled.set(true);

        let root = self.root();
        let (size, orthogonal_size, offset, orthogonal_offset) = if root.orientation == Orientation::Horizontal { (height, width, top, left) } else { (width, height, left, top) };
        root.layout_node(size, 0.0, Some(&LayoutContext { orthogonal_size, absolute_offset: offset, absolute_orthogonal_offset: orthogonal_offset, absolute_size: size, absolute_orthogonal_size: orthogonal_size }));
    }

    /// Adiciona uma view no `location`.
    pub fn add_view(&self, view: GridViewRef, size: impl Into<ViewSize>, location: &[usize]) {
        let mut size = size.into();
        if self.has_maximized_view() {
            self.exit_maximized_view();
        }

        self.dispose_2x2();

        let (rest, index) = tail(location);
        let (path_to_parent, parent) = self.get_node(&rest);

        match &parent {
            Node::Branch(parent) => {
                let node = LeafNode::new(&self.ui, view, orthogonal(parent.orientation), self.layout_controller.clone(), parent.orthogonal_size.get(), 0.0);

                // `try/catch` do original: se `addChild` lançar, descarta o nó e relança.
                let result = errors::catch(|| parent.add_child(Node::Leaf(node.clone()), size, index, false));
                if let Err(err) = result {
                    node.dispose();
                    errors::throw(err);
                }
            }
            Node::Leaf(parent) => {
                let grand_parent = path_to_parent.last().expect("uma folha sempre tem avô").clone();
                let (_, parent_index) = tail(&rest);

                let mut new_sibling_size: ViewSize = ViewSize::Px(0.0);

                if let Some(cached) = grand_parent.get_child_cached_visible_size(parent_index) {
                    new_sibling_size = ViewSize::Sizing(Sizing::Invisible(cached));
                }

                let old_child = grand_parent.remove_child(parent_index, None);
                old_child.dispose();

                let new_parent = BranchNode::new(&self.ui, parent.orientation, parent.layout_controller.clone(), self.styles.get(), self.proportional_layout, parent.size.get(), parent.orthogonal_size.get(), grand_parent.edge_snapping(), None);
                grand_parent.add_child(Node::Branch(new_parent.clone()), parent.size.get(), parent_index, false);

                let new_sibling = LeafNode::new(&self.ui, parent.view.clone(), grand_parent.orientation, self.layout_controller.clone(), parent.size.get(), 0.0);
                new_parent.add_child(Node::Leaf(new_sibling), new_sibling_size, 0, false);

                if let ViewSize::Sizing(Sizing::Split(_)) = size {
                    size = ViewSize::Sizing(Sizing::Split(0));
                }

                let node = LeafNode::new(&self.ui, view, grand_parent.orientation, self.layout_controller.clone(), parent.size.get(), 0.0);
                new_parent.add_child(Node::Leaf(node), size, index, false);
            }
        }

        self.try_set_2x2();
    }

    /// Remove a view do `location`.
    pub fn remove_view(&self, location: &[usize], sizing: Option<Sizing>) -> GridViewRef {
        if self.has_maximized_view() {
            self.exit_maximized_view();
        }

        self.dispose_2x2();

        let (rest, index) = tail(location);
        let (path_to_parent, parent) = self.get_node(&rest);

        let Node::Branch(parent) = parent else { throw("Invalid location") };

        let children = parent.children();
        let Some(Node::Leaf(node)) = children.get(index) else { throw("Invalid location") };
        let node = node.clone();

        parent.remove_child(index, sizing);
        node.dispose();

        let children = parent.children();
        if children.is_empty() {
            throw("Invalid grid state");
        }

        if children.len() > 1 {
            self.try_set_2x2();
            return node.view.clone();
        }

        if path_to_parent.is_empty() {
            // O pai é a raiz.
            let sibling = children[0].clone();

            let Node::Branch(sibling_branch) = &sibling else { return node.view.clone() };

            // O irmão vira a nova raiz.
            parent.remove_child(0, None);
            parent.dispose();
            self.set_root(sibling_branch.clone());
            self.set_boundary_sashes(self.boundary_sashes());
            self.try_set_2x2();
            return node.view.clone();
        }

        let grand_parent = path_to_parent.last().expect("o caminho não é vazio").clone();
        let (_, parent_index) = tail(&rest);

        let is_sibling_visible = parent.is_child_visible(0);
        let sibling = parent.remove_child(0, None);

        let mut sizes: Vec<f64> = (0..grand_parent.children.borrow().len()).map(|i| grand_parent.get_child_size(i)).collect();
        grand_parent.remove_child(parent_index, sizing);
        parent.dispose();

        match &sibling {
            Node::Branch(sibling) => {
                let sibling_children = sibling.children();
                sizes.splice(parent_index..=parent_index, sibling_children.iter().map(Node::size));
                let sibling_child_sizes: Vec<ViewSize> = sibling_children
                    .iter()
                    .enumerate()
                    .map(|(index, child)| match sibling.get_child_cached_visible_size(index) {
                        Some(cached) => ViewSize::Sizing(Sizing::Invisible(cached)),
                        None => ViewSize::Px(child.size()),
                    })
                    .collect();

                let sibling_children = sibling.remove_all_children();

                for (i, child) in sibling_children.into_iter().enumerate() {
                    grand_parent.add_child(child, sibling_child_sizes[i], parent_index + i, false);
                }
            }
            Node::Leaf(sibling) => {
                let new_sibling = LeafNode::new(&self.ui, sibling.view.clone(), orthogonal(sibling.orientation), self.layout_controller.clone(), sibling.size.get(), 0.0);
                let sizing: ViewSize = if is_sibling_visible { ViewSize::Px(sibling.orthogonal_size.get()) } else { ViewSize::Sizing(Sizing::Invisible(sibling.orthogonal_size.get())) };
                grand_parent.add_child(Node::Leaf(new_sibling), sizing, parent_index, false);
            }
        }

        sibling.dispose();

        for (i, size) in sizes.iter().enumerate() {
            grand_parent.resize_child(i, *size);
        }

        self.try_set_2x2();
        node.view.clone()
    }

    /// Move uma view dentro do pai.
    pub fn move_view(&self, parent_location: &[usize], from: usize, to: usize) {
        if self.has_maximized_view() {
            self.exit_maximized_view();
        }

        let (_, parent) = self.get_node(parent_location);

        let Node::Branch(parent) = parent else { throw("Invalid location") };

        parent.move_child(from, to);

        self.try_set_2x2();
    }

    /// Troca duas views do grid.
    pub fn swap_views(&self, from: &[usize], to: &[usize]) {
        if self.has_maximized_view() {
            self.exit_maximized_view();
        }

        let (from_rest, from_index) = tail(from);
        let (_, from_parent) = self.get_node(&from_rest);

        let Node::Branch(from_parent) = from_parent else { throw("Invalid from location") };

        let from_size = from_parent.get_child_size(from_index);
        let Some(Node::Leaf(_)) = from_parent.children().get(from_index) else { throw("Invalid from location") };
        let from_node = from_parent.child(from_index);

        let (to_rest, to_index) = tail(to);
        let (_, to_parent) = self.get_node(&to_rest);

        let Node::Branch(to_parent) = to_parent else { throw("Invalid to location") };

        let to_size = to_parent.get_child_size(to_index);
        let Some(Node::Leaf(_)) = to_parent.children().get(to_index) else { throw("Invalid to location") };
        let to_node = to_parent.child(to_index);

        if Rc::ptr_eq(&from_parent, &to_parent) {
            from_parent.swap_children(from_index, to_index);
        } else {
            from_parent.remove_child(from_index, None);
            to_parent.remove_child(to_index, None);

            from_parent.add_child(to_node, from_size, from_index, false);
            to_parent.add_child(from_node, to_size, to_index, false);
        }

        self.try_set_2x2();
    }

    /// Redimensiona uma view; `size` pode trazer só uma dimensão.
    pub fn resize_view(&self, location: &[usize], size: PartialViewDimensions) {
        if self.has_maximized_view() {
            self.exit_maximized_view();
        }

        let (rest, index) = tail(location);
        let (path_to_parent, parent) = self.get_node(&rest);

        let Node::Branch(parent) = parent else { throw("Invalid location") };

        let falsy = |v: Option<f64>| v.is_none_or(|v| v == 0.0 || v.is_nan());
        if falsy(size.width) && falsy(size.height) {
            return;
        }

        let (parent_size, grand_parent_size) = if parent.orientation == Orientation::Horizontal { (size.width, size.height) } else { (size.height, size.width) };

        if let Some(grand_parent_size) = grand_parent_size
            && let Some(grand_parent) = path_to_parent.last() {
                let (_, parent_index) = tail(&rest);

                grand_parent.resize_child(parent_index, grand_parent_size);
            }

        if let Some(parent_size) = parent_size {
            parent.resize_child(index, parent_size);
        }

        self.try_set_2x2();
    }

    /// O tamanho de uma view; `None` é o tamanho do próprio grid.
    pub fn get_view_size(&self, location: Option<&[usize]>) -> ViewDimensions {
        let Some(location) = location else {
            let root = self.root();
            return ViewDimensions { width: root.width(), height: root.height() };
        };

        let (_, node) = self.get_node(location);
        ViewDimensions { width: node.width(), height: node.height() }
    }

    /// O tamanho que a view tinha quando foi escondida.
    pub fn get_view_cached_visible_size(&self, location: &[usize]) -> Option<f64> {
        let (rest, index) = tail(location);
        let (_, parent) = self.get_node(&rest);

        let Node::Branch(parent) = parent else { throw("Invalid location") };

        parent.get_child_cached_visible_size(index)
    }

    /// Maximiza o tamanho de uma view recolhendo as outras ao mínimo.
    pub fn expand_view(&self, location: &[usize]) {
        if self.has_maximized_view() {
            self.exit_maximized_view();
        }

        let (ancestors, node) = self.get_node(location);

        let Node::Leaf(_) = node else { throw("Invalid location") };

        for (i, ancestor) in ancestors.iter().enumerate() {
            ancestor.resize_child(location[i], f64::INFINITY);
        }
    }

    /// Se todas as outras views estão no tamanho mínimo.
    pub fn is_view_expanded(&self, location: &[usize]) -> bool {
        if self.has_maximized_view() {
            // Nenhuma view pode estar expandida com uma maximizada.
            return false;
        }

        let (ancestors, node) = self.get_node(location);

        let Node::Leaf(_) = node else { throw("Invalid location") };

        for (i, ancestor) in ancestors.iter().enumerate() {
            if !ancestor.is_child_expanded(location[i]) {
                return false;
            }
        }

        true
    }

    pub fn maximize_view(&self, location: &[usize], exclude_views: &[GridViewRef]) {
        let (_, node_to_maximize) = self.get_node(location);
        let Node::Leaf(node_to_maximize) = node_to_maximize else { throw("Location is not a LeafNode") };

        if self.maximized_node.borrow().as_ref().is_some_and(|n| Rc::ptr_eq(n, &node_to_maximize)) {
            return;
        }

        if self.has_maximized_view() {
            self.exit_maximized_view();
        }

        let exclude: HashSet<usize> = exclude_views.iter().map(|v| Rc::as_ptr(v).cast::<()>() as usize).collect();
        let mut visible_nodes = HashSet::new();

        fn collect_visible_nodes(parent: &BranchNode, visible_nodes: &mut HashSet<usize>) {
            for (index, child) in parent.children().iter().enumerate() {
                if parent.is_child_visible(index) {
                    visible_nodes.insert(child.id());
                }
                if let Node::Branch(branch) = child {
                    collect_visible_nodes(branch, visible_nodes);
                }
            }
        }

        fn hide_all_views_but(parent: &BranchNode, exclude_node: &Rc<LeafNode>, exclude: &HashSet<usize>) {
            for (i, child) in parent.children().iter().enumerate() {
                match child {
                    Node::Leaf(leaf) => {
                        if !Rc::ptr_eq(leaf, exclude_node) && !exclude.contains(&(Rc::as_ptr(&leaf.view).cast::<()>() as usize)) {
                            parent.set_child_visible(i, false);
                        }
                    }
                    Node::Branch(branch) => hide_all_views_but(branch, exclude_node, exclude),
                }
            }
        }

        collect_visible_nodes(&self.root(), &mut visible_nodes);
        hide_all_views_but(&self.root(), &node_to_maximize, &exclude);

        *self.maximized_visible_nodes.borrow_mut() = Some(visible_nodes);
        *self.maximized_node.borrow_mut() = Some(node_to_maximize);
        self.on_did_change_view_maximized.fire(true);
    }

    pub fn exit_maximized_view(&self) {
        if self.maximized_node.borrow().is_none() {
            return;
        }
        let visible_nodes = self.maximized_visible_nodes.borrow_mut().take();
        *self.maximized_node.borrow_mut() = None;

        // Restaurar as views visíveis em ordem reversa também restaura os tamanhos guardados.
        fn restore_views_in_reverse_order(parent: &BranchNode, visible_nodes: &Option<HashSet<usize>>) {
            let children = parent.children();
            for index in (0..children.len()).rev() {
                let child = &children[index];
                if let Node::Branch(branch) = child {
                    restore_views_in_reverse_order(branch, visible_nodes);
                }
                parent.set_child_visible(index, visible_nodes.as_ref().is_none_or(|v| v.contains(&child.id())));
            }
        }

        restore_views_in_reverse_order(&self.root(), &visible_nodes);

        self.on_did_change_view_maximized.fire(false);
    }

    pub fn has_maximized_view(&self) -> bool {
        self.maximized_node.borrow().is_some()
    }

    /// Se a view está maximizada.
    pub fn is_view_maximized(&self, location: &[usize]) -> bool {
        let (_, node) = self.get_node(location);
        let Node::Leaf(node) = node else { throw("Location is not a LeafNode") };
        self.maximized_node.borrow().as_ref().is_some_and(|n| Rc::ptr_eq(n, &node))
    }

    /// Distribui o tamanho entre todas as views do grid (`None`) ou dentro de um só split view.
    pub fn distribute_view_sizes(&self, location: Option<&[usize]>) {
        if self.has_maximized_view() {
            self.exit_maximized_view();
        }

        let Some(location) = location else {
            self.root().distribute_view_sizes(true);
            return;
        };

        let (_, node) = self.get_node(location);

        let Node::Branch(node) = node else { throw("Invalid location") };

        node.distribute_view_sizes(false);
        self.try_set_2x2();
    }

    pub fn is_view_visible(&self, location: &[usize]) -> bool {
        let (rest, index) = tail(location);
        let (_, parent) = self.get_node(&rest);

        let Node::Branch(parent) = parent else { throw("Invalid from location") };

        parent.is_child_visible(index)
    }

    pub fn set_view_visible(&self, location: &[usize], visible: bool) {
        if self.has_maximized_view() {
            self.exit_maximized_view();
        }

        let (rest, index) = tail(location);
        let (_, parent) = self.get_node(&rest);

        let Node::Branch(parent) = parent else { throw("Invalid from location") };

        parent.set_child_visible(index, visible);
    }

    /// Um descritor do grid inteiro.
    pub fn get_view(&self) -> GridBranchNode<GridViewRef> {
        let GridNode::Branch(branch) = self.get_views(&Node::Branch(self.root()), self.orientation(), None) else { unreachable!("a raiz é um ramo") };
        branch
    }

    /// Um descritor da subárvore no `location`.
    pub fn get_view_at(&self, location: &[usize]) -> GridNode<GridViewRef> {
        let (_, node) = self.get_node(location);
        self.get_views(&node, self.orientation(), None)
    }

    /// Constrói um `GridView` a partir de um JSON já lido.
    pub fn deserialize<D>(ui: &Ui, json: &SerializedGridView<D>, from_json: &dyn Fn(&D) -> GridViewRef, options: GridViewOptions) -> Rc<GridView> {
        let SerializedNode::Branch { .. } = &json.root else { throw("Invalid JSON: 'root' property must have 'type' value of branch.") };

        let result = GridView::new(ui, options);
        result.deserialize_into(&json.root, json.orientation, from_json, json.height);

        result
    }

    fn deserialize_into<D>(&self, root: &SerializedNode<D>, orientation: Orientation, from_json: &dyn Fn(&D) -> GridViewRef, orthogonal_size: f64) {
        let Node::Branch(branch) = self.deserialize_node(root, orientation, from_json, orthogonal_size) else { unreachable!("a raiz é um ramo") };
        self.set_root(branch);
    }

    fn deserialize_node<D>(&self, node: &SerializedNode<D>, orientation: Orientation, from_json: &dyn Fn(&D) -> GridViewRef, orthogonal_size: f64) -> Node {
        match node {
            SerializedNode::Branch { data, size, .. } => {
                let size = size.unwrap_or(f64::NAN);
                let children = data
                    .iter()
                    .map(|serialized_child| NodeDescriptor { node: self.deserialize_node(serialized_child, orthogonal(orientation), from_json, size), visible: serialized_child.visible() })
                    .collect();

                Node::Branch(BranchNode::new(&self.ui, orientation, self.layout_controller.clone(), self.styles.get(), self.proportional_layout, size, orthogonal_size, false, Some(children)))
            }
            SerializedNode::Leaf { data, size, maximized, .. } => {
                let result = LeafNode::new(&self.ui, from_json(data), orientation, self.layout_controller.clone(), orthogonal_size, size.unwrap_or(f64::NAN));
                if *maximized == Some(true) && self.maximized_node.borrow().is_none() {
                    *self.maximized_node.borrow_mut() = Some(result.clone());
                    self.on_did_change_view_maximized.fire(true);
                }
                Node::Leaf(result)
            }
        }
    }

    fn get_views(&self, node: &Node, orientation: Orientation, cached_visible_size: Option<f64>) -> GridNode<GridViewRef> {
        let box_ = GridBox { top: node.top(), left: node.left(), width: node.width(), height: node.height() };

        match node {
            Node::Leaf(leaf) => GridNode::Leaf(GridLeafNode { view: leaf.view.clone(), box_, cached_visible_size, maximized: self.maximized_node.borrow().as_ref().is_some_and(|n| Rc::ptr_eq(n, leaf)) }),
            Node::Branch(branch) => {
                let children = branch.children();
                let mut result = Vec::new();

                for (i, child) in children.iter().enumerate() {
                    let cached_visible_size = branch.get_child_cached_visible_size(i);

                    result.push(self.get_views(child, orthogonal(orientation), cached_visible_size));
                }

                GridNode::Branch(GridBranchNode { children: result, box_ })
            }
        }
    }

    fn get_node(&self, location: &[usize]) -> (Vec<Rc<BranchNode>>, Node) {
        let mut path: Vec<Rc<BranchNode>> = Vec::new();
        let mut node = Node::Branch(self.root());

        for index in location {
            let Node::Branch(branch) = &node else { throw("Invalid location") };

            let child = {
                let children = branch.children.borrow();
                if *index >= children.len() {
                    throw("Invalid location");
                }
                children[*index].clone()
            };
            path.push(branch.clone());
            node = child;
        }

        (path, node)
    }

    fn dispose_2x2(&self) {
        let previous = std::mem::replace(&mut *self.disposable_2x2.borrow_mut(), none());
        previous.dispose();
    }

    /// Tenta travar os sashes para o grid se comportar como uma matriz 2x2, com um sash de canto no meio.
    /// Se o grid não for 2x2 com todos os sashes alinhados, não faz nada.
    pub fn try_set_2x2(&self) {
        self.dispose_2x2();

        let root = self.root();
        let children = root.children();
        if children.len() != 2 {
            return;
        }

        let (Node::Branch(first), Node::Branch(second)) = (&children[0], &children[1]) else { return };

        *self.disposable_2x2.borrow_mut() = first.try_set_2x2(second);
    }

    /// Preenche a lista com cada view e o nó dela (para uso interno).
    pub fn get_view_map(&self) -> Vec<(GridViewRef, NodeId)> {
        fn visit(node: &Node, out: &mut Vec<(GridViewRef, NodeId)>) {
            match node {
                Node::Branch(branch) => branch.children().iter().for_each(|c| visit(c, out)),
                Node::Leaf(leaf) => out.push((leaf.view.clone(), leaf.element)),
            }
        }
        let mut out = Vec::new();
        visit(&Node::Branch(self.root()), &mut out);
        out
    }
}

impl IDisposable for GridView {
    fn dispose(&self) {
        if self.disposed.replace(true) {
            return;
        }
        self.on_did_change_view_maximized.dispose();
        self.on_did_sash_reset_relay.dispose();
        self.on_did_scroll_relay.dispose();
        self.on_did_change_relay.dispose();
        self.root().dispose();
        self.dispose_2x2();
        self.ui.remove(self.element);
    }

    fn constructor_name(&self) -> &'static str {
        "GridView"
    }
}

/// `tail(array)`: tudo menos o último, e o último. Lança se vazio.
fn tail(location: &[usize]) -> (Vec<usize>, usize) {
    match location.split_last() {
        Some((last, rest)) => (rest.to_vec(), *last),
        None => throw("Invalid tail call"),
    }
}
