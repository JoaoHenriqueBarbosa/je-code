//! Porte de `src/vs/base/browser/ui/grid/grid.ts`: o [`Grid`], o `GridView` com uma API por vistas (adicionar,
//! mover e remover "ao lado de" outra view, em vez de por índices), e o [`SerializableGrid`].
//!
//! `Grid<T>` é genérico no tipo da view, como o original: as views entram e saem como `Rc<T>` e a identidade
//! (o `Map<T, HTMLElement>` do original) é a do `Rc`. O `GridView` por baixo trabalha com `Rc<dyn GridViewView>`;
//! o `Grid` guarda as `Rc<T>` para devolvê-las em `getViews` e `getNeighborViews`.
//!
//! `getViewLocation` acha o local de uma view subindo a árvore de nós (o `getGridLocation` do original sobe o
//! DOM), então depende da estrutura que o `SplitView` e o `GridView` montam: ver a documentação deles.
//!
//! Divergências de tipo, nenhuma de comportamento:
//!
//! - O `data` de uma folha serializada é o tipo `D` (um parâmetro, no lugar do `unknown`/`any` do original).
//!   O `SerializableView::Json` é o `D` do `toJSON`; o [`SerializableGrid::from`] usa `D = Rc<T>` (a view como o
//!   próprio `data`, como o original). O `data` ausente do descritor é `D::default()`.
//! - `Sizing` do grid (sem índice) é um tipo à parte do `Sizing` do split view, como no original.

use std::cell::{Cell, RefCell};
use std::ops::Deref;
use std::rc::Rc;

use je_base::errors::{self, Error};
use je_base::event::Event as BaseEvent;
use je_base::lifecycle::{Disposable, IDisposable};
use je_ui::{NodeId, Ui};

use crate::gridview::{GridBox, GridBranchNode, GridLeafNode, GridNode, GridView, GridViewOptions, GridViewRef, GridViewStyles, GridViewView, PartialViewDimensions, SerializedGridView, SerializedNode, ViewDimensions};
pub use crate::gridview::{GridLocation, LayoutPriority, Orientation, orthogonal};
use crate::sash::BoundarySashes;
use crate::splitview::{Sizing as GridViewSizing, ViewSize};

fn throw(message: &str) -> ! {
    errors::throw(Error::new(message))
}

/// A direção de um vizinho.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Direction {
    Up,
    Down,
    Left,
    Right,
}

fn opposite_direction(direction: Direction) -> Direction {
    match direction {
        Direction::Up => Direction::Down,
        Direction::Down => Direction::Up,
        Direction::Left => Direction::Right,
        Direction::Right => Direction::Left,
    }
}

/// O `IView` de uma view do [`Grid`]: o do `GridView` mais os tamanhos preferidos.
pub trait GridItem: GridViewView {
    /// A largura preferida, para o duplo clique num sash vizinho desta view.
    fn preferred_width(&self) -> Option<f64> {
        None
    }

    /// A altura preferida.
    fn preferred_height(&self) -> Option<f64> {
        None
    }
}

fn get_grid_node<V: Clone>(node: &GridNode<V>, location: &[usize]) -> GridNode<V> {
    let Some((index, rest)) = location.split_first() else { return node.clone() };

    let GridNode::Branch(branch) = node else { throw("Invalid location") };

    get_grid_node(&branch.children[*index], rest)
}

#[derive(Clone, Copy)]
struct Range {
    start: f64,
    end: f64,
}

fn intersects(one: Range, other: Range) -> bool {
    !(one.start >= other.end || other.start >= one.end)
}

#[derive(Clone, Copy)]
struct Boundary {
    offset: f64,
    range: Range,
}

fn get_box_boundary(box_: GridBox, direction: Direction) -> Boundary {
    let orientation = get_direction_orientation(direction);
    let offset = match direction {
        Direction::Up => box_.top,
        Direction::Right => box_.left + box_.width,
        Direction::Down => box_.top + box_.height,
        Direction::Left => box_.left,
    };

    let range = Range {
        start: if orientation == Orientation::Horizontal { box_.top } else { box_.left },
        end: if orientation == Orientation::Horizontal { box_.top + box_.height } else { box_.left + box_.width },
    };

    Boundary { offset, range }
}

fn find_adjacent_box_leaf_nodes<V: Clone>(box_node: &GridNode<V>, direction: Direction, boundary: Boundary) -> Vec<GridLeafNode<V>> {
    fn visit<V: Clone>(box_node: &GridNode<V>, direction: Direction, boundary: Boundary, result: &mut Vec<GridLeafNode<V>>) {
        match box_node {
            GridNode::Branch(branch) => {
                for child in &branch.children {
                    visit(child, direction, boundary, result);
                }
            }
            GridNode::Leaf(leaf) => {
                let Boundary { offset, range } = get_box_boundary(leaf.box_, direction);

                if offset == boundary.offset && intersects(range, boundary.range) {
                    result.push(leaf.clone());
                }
            }
        }
    }

    let mut result = Vec::new();
    visit(box_node, direction, boundary, &mut result);
    result
}

fn get_location_orientation(root_orientation: Orientation, location: &[usize]) -> Orientation {
    if location.len().is_multiple_of(2) { orthogonal(root_orientation) } else { root_orientation }
}

fn get_direction_orientation(direction: Direction) -> Orientation {
    if direction == Direction::Up || direction == Direction::Down { Orientation::Vertical } else { Orientation::Horizontal }
}

/// O local onde uma view entra ao lado de outra, na direção dada.
pub fn get_relative_location(root_orientation: Orientation, location: &[usize], direction: Direction) -> GridLocation {
    let orientation = get_location_orientation(root_orientation, location);
    let direction_orientation = get_direction_orientation(direction);

    if orientation == direction_orientation {
        let (rest, mut index) = tail(location);

        if direction == Direction::Right || direction == Direction::Down {
            index += 1;
        }

        let mut result = rest;
        result.push(index);
        result
    } else {
        let index = usize::from(direction == Direction::Right || direction == Direction::Down);
        let mut result = location.to_vec();
        result.push(index);
        result
    }
}

fn tail(location: &[usize]) -> (Vec<usize>, usize) {
    match location.split_last() {
        Some((last, rest)) => (rest.to_vec(), *last),
        None => throw("Invalid tail call"),
    }
}

/// Acha o local de um elemento subindo a cadeia de pais e achando o índice de cada filho no caminho. Quebra
/// assim que a estrutura de nós do `SplitView` ou do `GridView` mudar.
fn get_grid_location(ui: &Ui, element: NodeId) -> GridLocation {
    let tree = ui.tree();
    let Some(parent_element) = tree.parent(element) else { throw("Invalid grid element") };

    if tree.has_class(parent_element, "monaco-grid-view") {
        return Vec::new();
    }

    // O índice do contêiner `.split-view-view` entre os irmãos; o ramo está quatro níveis acima dele
    // (`.split-view-container`, `.monaco-scrollable-element`, `.monaco-split-view2`, `.monaco-grid-branch-node`).
    let Some(grandparent) = tree.parent(parent_element) else { throw("Invalid grid element") };
    let index = tree.children(grandparent).iter().position(|c| *c == parent_element).expect("o filho está no pai");
    let mut ancestor = parent_element;
    for _ in 0..4 {
        ancestor = tree.parent(ancestor).unwrap_or_else(|| throw("Invalid grid element"));
    }
    drop(tree);

    let mut location = get_grid_location(ui, ancestor);
    location.push(index);
    location
}

/// Como redistribuir o espaço ao mexer numa view do [`Grid`].
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Sizing {
    Distribute,
    Split,
    Auto,
    Invisible(f64),
}

/// `number | Sizing`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum GridSize {
    Px(f64),
    Sizing(Sizing),
}

impl From<f64> for GridSize {
    fn from(size: f64) -> Self {
        Self::Px(size)
    }
}

impl From<Sizing> for GridSize {
    fn from(sizing: Sizing) -> Self {
        Self::Sizing(sizing)
    }
}

pub type GridStyles = GridViewStyles;
pub type GridOptions = GridViewOptions;

/// O `Grid`.
pub struct Grid<T: GridItem + 'static> {
    base: Rc<Disposable>,
    ui: Ui,
    gridview: Rc<GridView>,
    views: RefCell<Vec<(Rc<T>, NodeId)>>,
    did_layout: Cell<bool>,
}

impl<T: GridItem + 'static> Grid<T> {
    /// Cria um grid com uma view inicial (um grid sempre tem pelo menos uma view).
    pub fn new(ui: &Ui, view: Rc<T>, options: GridOptions) -> Rc<Grid<T>> {
        let this = Self::with_gridview(ui, GridView::new(ui, options), Vec::new());
        this._add_view(view, ViewSize::Px(0.0), &[0]);
        this
    }

    /// Cria um grid sobre um `GridView` existente, com as `Rc<T>` das views dele.
    pub fn from_gridview(ui: &Ui, gridview: Rc<GridView>, views: Vec<Rc<T>>) -> Rc<Grid<T>> {
        let elements = gridview.get_view_map();
        let views = views
            .into_iter()
            .map(|view| {
                let identity = Rc::as_ptr(&view).cast::<()>();
                let element = elements.iter().find(|(v, _)| Rc::as_ptr(v).cast::<()>() == identity).map(|(_, e)| *e).unwrap_or_else(|| throw("View not found"));
                (view, element)
            })
            .collect();
        Self::with_gridview(ui, gridview, views)
    }

    fn with_gridview(ui: &Ui, gridview: Rc<GridView>, views: Vec<(Rc<T>, NodeId)>) -> Rc<Grid<T>> {
        let base = Disposable::new();
        base.register(gridview.clone());
        let this = Rc::new(Grid { base, ui: ui.clone(), gridview, views: RefCell::new(views), did_layout: Cell::new(false) });

        let weak = Rc::downgrade(&this);
        this.base.register(this.gridview.on_did_sash_reset().listen(move |location| {
            if let Some(grid) = weak.upgrade() {
                grid.on_did_sash_reset(location);
            }
        }));
        this
    }

    pub fn orientation(&self) -> Orientation {
        self.gridview.orientation()
    }

    pub fn set_orientation(&self, orientation: Orientation) {
        self.gridview.set_orientation(orientation);
    }

    pub fn width(&self) -> f64 {
        self.gridview.width()
    }

    pub fn height(&self) -> f64 {
        self.gridview.height()
    }

    pub fn minimum_width(&self) -> f64 {
        self.gridview.minimum_width()
    }

    pub fn minimum_height(&self) -> f64 {
        self.gridview.minimum_height()
    }

    pub fn maximum_width(&self) -> f64 {
        self.gridview.maximum_width()
    }

    pub fn maximum_height(&self) -> f64 {
        self.gridview.maximum_height()
    }

    /// Dispara quando uma view do grid muda as restrições de tamanho.
    pub fn on_did_change(&self) -> BaseEvent<Option<ViewDimensions>> {
        self.gridview.on_did_change()
    }

    /// Dispara quando o usuário rola um split view do grid.
    pub fn on_did_scroll(&self) -> BaseEvent<()> {
        self.gridview.on_did_scroll()
    }

    pub fn on_did_change_view_maximized(&self) -> BaseEvent<bool> {
        self.gridview.on_did_change_view_maximized()
    }

    pub fn boundary_sashes(&self) -> BoundarySashes {
        self.gridview.boundary_sashes()
    }

    /// Os sashes perpendiculares a cada borda do grid.
    pub fn set_boundary_sashes(&self, boundary_sashes: BoundarySashes) {
        self.gridview.set_boundary_sashes(boundary_sashes);
    }

    pub fn set_edge_snapping(&self, edge_snapping: bool) {
        self.gridview.set_edge_snapping(edge_snapping);
    }

    /// O nó do grid.
    pub fn element(&self) -> NodeId {
        self.gridview.element()
    }

    pub fn style(&self, styles: GridStyles) {
        self.gridview.style(styles);
    }

    /// Faz o layout do grid, com `top` e `left` opcionais como origem das posições das views.
    pub fn layout(&self, width: f64, height: f64, top: f64, left: f64) {
        self.gridview.layout(width, height, top, left);
        self.did_layout.set(true);
    }

    /// Adiciona uma view ao lado de uma view de referência.
    ///
    /// Com este grid:
    ///
    /// ```text
    ///  +-----+---------------+
    ///  |  A  |      B        |
    ///  +-----+---------+-----+
    ///  |        C      |     |
    ///  +---------------+  D  |
    ///  |        E      |     |
    ///  +---------------+-----+
    /// ```
    ///
    /// `add_view(X, Sizing::Distribute, C, Direction::Right)` põe o X à direita do C, e
    /// `add_view(X, Sizing::Distribute, D, Direction::Down)` o põe embaixo do D.
    pub fn add_view(&self, new_view: Rc<T>, size: impl Into<GridSize>, reference_view: &Rc<T>, direction: Direction) {
        if self.has_view(&new_view) {
            throw("Can't add same view twice");
        }

        let orientation = get_direction_orientation(direction);

        if self.views.borrow().len() == 1 && self.orientation() != orientation {
            self.set_orientation(orientation);
        }

        let reference_location = self.get_view_location(reference_view);
        let location = get_relative_location(self.gridview.orientation(), &reference_location, direction);

        let view_size = match size.into() {
            GridSize::Px(px) => ViewSize::Px(px),
            GridSize::Sizing(Sizing::Split) => ViewSize::Sizing(GridViewSizing::Split(tail(&reference_location).1)),
            GridSize::Sizing(Sizing::Distribute) => ViewSize::Sizing(GridViewSizing::Distribute),
            GridSize::Sizing(Sizing::Auto) => ViewSize::Sizing(GridViewSizing::Auto(tail(&reference_location).1)),
            GridSize::Sizing(Sizing::Invisible(cached)) => ViewSize::Sizing(GridViewSizing::Invisible(cached)),
        };

        self._add_view(new_view, view_size, &location);
    }

    fn add_view_at(&self, new_view: Rc<T>, size: GridSize, location: &[usize]) {
        if self.has_view(&new_view) {
            throw("Can't add same view twice");
        }

        let view_size = match size {
            GridSize::Px(px) => ViewSize::Px(px),
            GridSize::Sizing(Sizing::Distribute) => ViewSize::Sizing(GridViewSizing::Distribute),
            GridSize::Sizing(Sizing::Invisible(cached)) => ViewSize::Sizing(GridViewSizing::Invisible(cached)),
            // `number | DistributeSizing | InvisibleSizing` no original: `Split` e `Auto` não existem aqui.
            GridSize::Sizing(Sizing::Split | Sizing::Auto) => throw("Invalid sizing"),
        };

        self._add_view(new_view, view_size, location);
    }

    fn _add_view(&self, new_view: Rc<T>, size: ViewSize, location: &[usize]) {
        let element = new_view.element();
        self.views.borrow_mut().push((new_view.clone(), element));
        let view: GridViewRef = new_view;
        self.gridview.add_view(view, size, location);
    }

    /// Remove uma view do grid. `sizing` diz se redistribui o espaço das outras.
    pub fn remove_view(&self, view: &Rc<T>, sizing: Option<Sizing>) {
        if self.views.borrow().len() == 1 {
            throw("Can't remove last view");
        }

        let location = self.get_view_location(view);

        let grid_view_sizing = match sizing {
            Some(Sizing::Distribute) => Some(GridViewSizing::Distribute),
            Some(Sizing::Auto) => {
                let index = *location.last().expect("o local de uma view não é vazio");
                Some(GridViewSizing::Auto(if index == 0 { 1 } else { index - 1 }))
            }
            _ => None,
        };

        self.gridview.remove_view(&location, grid_view_sizing);
        self.views.borrow_mut().retain(|(v, _)| !Rc::ptr_eq(v, view));
    }

    /// Move uma view para outro lugar do grid.
    pub fn move_view(&self, view: &Rc<T>, sizing: impl Into<GridSize>, reference_view: &Rc<T>, direction: Direction) {
        let sizing = sizing.into();
        let source_location = self.get_view_location(view);
        let (source_parent_location, from) = tail(&source_location);

        let reference_location = self.get_view_location(reference_view);
        let target_location = get_relative_location(self.gridview.orientation(), &reference_location, direction);
        let (target_parent_location, to) = tail(&target_location);

        if source_parent_location == target_parent_location {
            self.gridview.move_view(&source_parent_location, from, to);
        } else {
            self.remove_view(view, match sizing {
                GridSize::Px(_) => None,
                GridSize::Sizing(sizing) => Some(sizing),
            });
            self.add_view(view.clone(), sizing, reference_view, direction);
        }
    }

    /// Move uma view para um local do grid. Método interno: ver `GridView::move_view`.
    pub fn move_view_to(&self, view: &Rc<T>, location: &[usize]) {
        let source_location = self.get_view_location(view);
        let (source_parent_location, from) = tail(&source_location);
        let (target_parent_location, to) = tail(location);

        if source_parent_location == target_parent_location {
            self.gridview.move_view(&source_parent_location, from, to);
        } else {
            let size = self.get_view_size(Some(view));
            let orientation = get_location_orientation(self.gridview.orientation(), &source_location);
            let cached_view_size = self.get_view_cached_visible_size(view);
            let sizing = match cached_view_size {
                None => GridSize::Px(if orientation == Orientation::Horizontal { size.width } else { size.height }),
                Some(cached) => GridSize::Sizing(Sizing::Invisible(cached)),
            };

            self.remove_view(view, None);
            self.add_view_at(view.clone(), sizing, location);
        }
    }

    /// Troca duas views do grid.
    pub fn swap_views(&self, from: &Rc<T>, to: &Rc<T>) {
        let from_location = self.get_view_location(from);
        let to_location = self.get_view_location(to);
        self.gridview.swap_views(&from_location, &to_location);
    }

    pub fn resize_view(&self, view: &Rc<T>, size: ViewDimensions) {
        let location = self.get_view_location(view);
        self.gridview.resize_view(&location, PartialViewDimensions { width: Some(size.width), height: Some(size.height) });
    }

    /// Se todas as outras views estão no tamanho mínimo.
    pub fn is_view_expanded(&self, view: &Rc<T>) -> bool {
        let location = self.get_view_location(view);
        self.gridview.is_view_expanded(&location)
    }

    /// Se a view está maximizada.
    pub fn is_view_maximized(&self, view: &Rc<T>) -> bool {
        let location = self.get_view_location(view);
        self.gridview.is_view_maximized(&location)
    }

    /// Se alguma view está maximizada.
    pub fn has_maximized_view(&self) -> bool {
        self.gridview.has_maximized_view()
    }

    /// O tamanho de uma view; `None` é o tamanho do grid.
    pub fn get_view_size(&self, view: Option<&Rc<T>>) -> ViewDimensions {
        let Some(view) = view else { return self.gridview.get_view_size(None) };

        let location = self.get_view_location(view);
        self.gridview.get_view_size(Some(&location))
    }

    /// O tamanho que a view tinha quando foi escondida.
    pub fn get_view_cached_visible_size(&self, view: &Rc<T>) -> Option<f64> {
        let location = self.get_view_location(view);
        self.gridview.get_view_cached_visible_size(&location)
    }

    /// Maximiza a view e esconde as outras, menos as de `exclude_views`.
    pub fn maximize_view(&self, view: &Rc<T>, exclude_views: &[Rc<T>]) {
        if self.views.borrow().len() < 2 {
            throw("At least two views are required to maximize a view");
        }
        let location = self.get_view_location(view);
        let exclude: Vec<GridViewRef> = exclude_views.iter().map(|v| v.clone() as GridViewRef).collect();
        self.gridview.maximize_view(&location, &exclude);
    }

    pub fn exit_maximized_view(&self) {
        self.gridview.exit_maximized_view();
    }

    /// Expande a view recolhendo todas as outras ao mínimo.
    pub fn expand_view(&self, view: &Rc<T>) {
        let location = self.get_view_location(view);
        self.gridview.expand_view(&location);
    }

    /// Distribui o tamanho entre todas as views do grid.
    pub fn distribute_view_sizes(&self) {
        self.gridview.distribute_view_sizes(None);
    }

    pub fn is_view_visible(&self, view: &Rc<T>) -> bool {
        let location = self.get_view_location(view);
        self.gridview.is_view_visible(&location)
    }

    /// Define a visibilidade; com `Sizing::Distribute` ao mostrar, redistribui o split view que a contém.
    pub fn set_view_visible(&self, view: &Rc<T>, visible: bool, sizing: Option<Sizing>) {
        let location = self.get_view_location(view);
        self.gridview.set_view_visible(&location, visible);
        if visible && sizing == Some(Sizing::Distribute) {
            let parent_location = (!location.is_empty()).then(|| tail(&location).0);
            self.gridview.distribute_view_sizes(parent_location.as_deref());
        }
    }

    /// Um descritor do grid inteiro.
    pub fn get_views(&self) -> GridBranchNode<Rc<T>> {
        let branch = self.gridview.get_view();
        let GridNode::Branch(branch) = GridNode::Branch(branch).map_views(&|view: &GridViewRef| self.view_of(view)) else { unreachable!("a raiz é um ramo") };
        branch
    }

    /// Todas as views que tocam a borda de uma view, na direção dada. `wrap` dá a volta no grid.
    pub fn get_neighbor_views(&self, view: &Rc<T>, direction: Direction, wrap: bool) -> Vec<Rc<T>> {
        if !self.did_layout.get() {
            throw("Can't call getNeighborViews before first layout");
        }

        let location = self.get_view_location(view);
        let root = GridNode::Branch(self.get_views());
        let node = get_grid_node(&root, &location);
        let mut boundary = get_box_boundary(node.box_(), direction);

        if wrap {
            let (node_box, root_box) = (node.box_(), root.box_());
            if direction == Direction::Up && node_box.top == 0.0 {
                boundary = Boundary { offset: root_box.top + root_box.height, range: boundary.range };
            } else if (direction == Direction::Right && node_box.left + node_box.width == root_box.width) || (direction == Direction::Down && node_box.top + node_box.height == root_box.height) {
                boundary = Boundary { offset: 0.0, range: boundary.range };
            } else if direction == Direction::Left && node_box.left == 0.0 {
                boundary = Boundary { offset: root_box.left + root_box.width, range: boundary.range };
            }
        }

        find_adjacent_box_leaf_nodes(&root, opposite_direction(direction), boundary).into_iter().map(|node| node.view).collect()
    }

    fn has_view(&self, view: &Rc<T>) -> bool {
        self.views.borrow().iter().any(|(v, _)| Rc::ptr_eq(v, view))
    }

    /// A `Rc<T>` de uma view do grid (pela identidade).
    fn view_of(&self, view: &GridViewRef) -> Rc<T> {
        let identity = Rc::as_ptr(view).cast::<()>();
        self.views.borrow().iter().find(|(v, _)| Rc::as_ptr(v).cast::<()>() == identity).map(|(v, _)| v.clone()).unwrap_or_else(|| throw("View not found"))
    }

    fn get_view_location(&self, view: &Rc<T>) -> GridLocation {
        let element = self.views.borrow().iter().find(|(v, _)| Rc::ptr_eq(v, view)).map(|(_, e)| *e);
        let Some(element) = element else { throw("View not found") };

        get_grid_location(&self.ui, element)
    }

    fn on_did_sash_reset(&self, location: &GridLocation) {
        let resize_to_preferred_size = |location: &[usize]| -> bool {
            let GridNode::Leaf(node) = self.gridview.get_view_at(location) else { return false };
            let view = self.view_of(&node.view);

            let direction = get_location_orientation(self.orientation(), location);
            let size = if direction == Orientation::Horizontal { view.preferred_width() } else { view.preferred_height() };

            let Some(size) = size else { return false };

            let rounded = (size + 0.5).floor();
            let view_size = if direction == Orientation::Horizontal { PartialViewDimensions { width: Some(rounded), height: None } } else { PartialViewDimensions { width: None, height: Some(rounded) } };
            self.gridview.resize_view(location, view_size);
            true
        };

        if resize_to_preferred_size(location) {
            return;
        }

        let (parent_location, index) = tail(location);
        let mut next = parent_location.clone();
        next.push(index + 1);

        if resize_to_preferred_size(&next) {
            return;
        }

        self.gridview.distribute_view_sizes(Some(&parent_location));
    }
}

impl<T: GridItem + 'static> IDisposable for Grid<T> {
    fn dispose(&self) {
        self.base.dispose();
    }

    fn constructor_name(&self) -> &'static str {
        "Grid"
    }
}

/// O `ISerializableView`: uma view do grid que sabe virar JSON (`Json`, o `data` da folha serializada).
pub trait SerializableView: GridItem {
    type Json: Clone + 'static;

    fn to_json(&self) -> Self::Json;
}

/// O `IViewDeserializer`: refaz uma view a partir do `data` da folha.
pub trait ViewDeserializer<T, D> {
    #[allow(clippy::wrong_self_convention)] // o nome é o `fromJSON` do `IViewDeserializer`
    fn from_json(&self, json: &D) -> Rc<T>;
}

impl<T, D, F: Fn(&D) -> Rc<T>> ViewDeserializer<T, D> for F {
    fn from_json(&self, json: &D) -> Rc<T> {
        self(json)
    }
}

/// O `ISerializedGrid`.
pub type SerializedGrid<D> = SerializedGridView<D>;

/// Um `Grid` que sabe se serializar.
pub struct SerializableGrid<T: SerializableView + 'static> {
    grid: Rc<Grid<T>>,
    /// Para restaurar as proporções das views no primeiro `layout`.
    initial_layout_context: Cell<bool>,
}

impl<T: SerializableView + 'static> Deref for SerializableGrid<T> {
    type Target = Grid<T>;

    fn deref(&self) -> &Grid<T> {
        &self.grid
    }
}

impl<T: SerializableView + 'static> SerializableGrid<T> {
    /// Cria um grid serializável com uma view inicial.
    pub fn new(ui: &Ui, view: Rc<T>, options: GridOptions) -> Rc<Self> {
        Rc::new(Self { grid: Grid::new(ui, view, options), initial_layout_context: Cell::new(true) })
    }

    fn serialize_node(node: &GridNode<Rc<T>>, orientation: Orientation) -> SerializedNode<T::Json> {
        let box_ = node.box_();
        let size = Some(if orientation == Orientation::Vertical { box_.width } else { box_.height });

        match node {
            GridNode::Leaf(leaf) => {
                let data = leaf.view.to_json();

                if let Some(cached) = leaf.cached_visible_size {
                    SerializedNode::Leaf { data, size: Some(cached), visible: Some(false), maximized: None }
                } else if leaf.maximized {
                    SerializedNode::Leaf { data, size, visible: None, maximized: Some(true) }
                } else {
                    SerializedNode::Leaf { data, size, visible: None, maximized: None }
                }
            }
            GridNode::Branch(branch) => {
                let data: Vec<SerializedNode<T::Json>> = branch.children.iter().map(|c| Self::serialize_node(c, orthogonal(orientation))).collect();
                if data.iter().any(|c| c.visible() != Some(false)) {
                    return SerializedNode::Branch { data, size, visible: None };
                }
                SerializedNode::Branch { data, size, visible: Some(false) }
            }
        }
    }

    /// Constrói um grid serializável a partir de um JSON.
    pub fn deserialize<D>(ui: &Ui, json: &SerializedGrid<D>, deserializer: &dyn ViewDeserializer<T, D>, options: GridOptions) -> Rc<Self> {
        let created: RefCell<Vec<Rc<T>>> = RefCell::new(Vec::new());
        let from_json = |data: &D| -> GridViewRef {
            let view = deserializer.from_json(data);
            created.borrow_mut().push(view.clone());
            view
        };
        let gridview = GridView::deserialize(ui, json, &from_json, options);
        let views = created.into_inner();
        Rc::new(Self { grid: Grid::from_gridview(ui, gridview, views), initial_layout_context: Cell::new(true) })
    }

    /// Constrói um grid serializável a partir de um descritor cujas folhas apontam para as views de verdade.
    pub fn from(ui: &Ui, grid_descriptor: GridDescriptor<Option<Rc<T>>>, options: GridOptions) -> Rc<Self> {
        let json = create_serialized_grid(grid_descriptor);
        Self::deserialize(ui, &json, &|view: &Option<Rc<T>>| view.clone().unwrap_or_else(|| throw("Invalid grid descriptor: a leaf without a view")), options)
    }

    /// Serializa o grid.
    pub fn serialize(&self) -> SerializedGrid<T::Json> {
        SerializedGrid { root: Self::serialize_node(&GridNode::Branch(self.get_views()), self.orientation()), orientation: self.orientation(), width: self.width(), height: self.height() }
    }

    pub fn layout(&self, width: f64, height: f64, top: f64, left: f64) {
        self.grid.layout(width, height, top, left);

        if self.initial_layout_context.replace(false) {
            self.grid.gridview.try_set_2x2();
        }
    }
}

impl<T: SerializableView + 'static> IDisposable for SerializableGrid<T> {
    fn dispose(&self) {
        self.grid.dispose();
    }

    fn constructor_name(&self) -> &'static str {
        "SerializableGrid"
    }
}

/// O `GridNodeDescriptor`: uma folha (`size`, `data`) ou um ramo (`size`, `groups`).
#[derive(Clone, Debug, PartialEq)]
pub enum GridNodeDescriptor<D> {
    Leaf { size: Option<f64>, data: D },
    Branch { size: Option<f64>, groups: Vec<GridNodeDescriptor<D>> },
}

impl<D> GridNodeDescriptor<D> {
    fn size(&self) -> Option<f64> {
        match self {
            Self::Leaf { size, .. } | Self::Branch { size, .. } => *size,
        }
    }

    fn set_size(&mut self, new_size: f64) {
        match self {
            Self::Leaf { size, .. } | Self::Branch { size, .. } => *size = Some(new_size),
        }
    }
}

/// O `GridDescriptor`: o ramo raiz com a orientação.
#[derive(Clone, Debug, PartialEq)]
pub struct GridDescriptor<D> {
    pub orientation: Orientation,
    pub size: Option<f64>,
    pub groups: Vec<GridNodeDescriptor<D>>,
}

/// `if (child.size)`: tamanho presente e diferente de zero.
fn truthy(size: Option<f64>) -> bool {
    size.is_some_and(|s| s != 0.0 && !s.is_nan())
}

/// Um grupo com uma view só vira a própria view, e os tamanhos ausentes dividem o que sobra do definido.
pub fn sanitize_grid_node_descriptor<D: Default>(node_descriptor: &mut GridNodeDescriptor<D>, root_node: bool) {
    if let GridNodeDescriptor::Branch { size, groups } = node_descriptor
        && !root_node && groups.len() <= 1 {
            *node_descriptor = GridNodeDescriptor::Leaf { size: *size, data: D::default() };
        }

    let GridNodeDescriptor::Branch { groups, .. } = node_descriptor else { return };

    let mut total_defined_size = 0.0;
    let mut total_defined_size_count = 0;

    for child in groups.iter_mut() {
        sanitize_grid_node_descriptor(child, false);

        if truthy(child.size()) {
            total_defined_size += child.size().unwrap_or(0.0);
            total_defined_size_count += 1;
        }
    }

    let total_undefined_size = if total_defined_size_count > 0 { total_defined_size } else { 1.0 };
    let total_undefined_size_count = groups.len() - total_defined_size_count;
    let each_undefined_size = total_undefined_size / total_undefined_size_count as f64;

    for child in groups.iter_mut() {
        if !truthy(child.size()) {
            child.set_size(each_undefined_size);
        }
    }
}

fn create_serialized_node<D>(node_descriptor: GridNodeDescriptor<D>) -> SerializedNode<D> {
    match node_descriptor {
        GridNodeDescriptor::Branch { size, groups } => SerializedNode::Branch { data: groups.into_iter().map(create_serialized_node).collect(), size, visible: None },
        GridNodeDescriptor::Leaf { size, data } => SerializedNode::Leaf { data, size, visible: None, maximized: None },
    }
}

/// `{ width?, height? }` do `getDimensions`.
type Dimensions = (Option<f64>, Option<f64>);

fn get_dimensions<D>(node: &SerializedNode<D>, orientation: Orientation) -> Dimensions {
    match node {
        SerializedNode::Branch { data, size, .. } => {
            let children_dimensions: Vec<Dimensions> = data.iter().map(|c| get_dimensions(c, orthogonal(orientation))).collect();
            let or_zero = |v: Option<f64>| v.filter(|v| *v != 0.0 && !v.is_nan()).unwrap_or(0.0);
            let max = |select: fn(&Dimensions) -> Option<f64>| {
                if children_dimensions.is_empty() { None } else { Some(children_dimensions.iter().map(|d| or_zero(select(d))).fold(f64::NEG_INFINITY, f64::max)) }
            };
            let sum = |select: fn(&Dimensions) -> Option<f64>| if children_dimensions.is_empty() { None } else { Some(children_dimensions.iter().map(|d| or_zero(select(d))).sum()) };
            let own = size.filter(|s| *s != 0.0 && !s.is_nan());

            if orientation == Orientation::Vertical {
                (own.or_else(|| max(|d| d.0)), sum(|d| d.1))
            } else {
                (sum(|d| d.0), own.or_else(|| max(|d| d.1)))
            }
        }
        SerializedNode::Leaf { size, .. } => {
            if orientation == Orientation::Vertical { (*size, None) } else { (None, *size) }
        }
    }
}

/// Cria o JSON de um `GridDescriptor`, que o [`SerializableGrid::deserialize`] sabe ler.
pub fn create_serialized_grid<D: Default>(grid_descriptor: GridDescriptor<D>) -> SerializedGrid<D> {
    let orientation = grid_descriptor.orientation;
    let mut root_descriptor = GridNodeDescriptor::Branch { size: grid_descriptor.size, groups: grid_descriptor.groups };
    sanitize_grid_node_descriptor(&mut root_descriptor, true);

    let root = create_serialized_node(root_descriptor);
    let (width, height) = get_dimensions(&root, orientation);
    let or_one = |v: Option<f64>| v.filter(|v| *v != 0.0 && !v.is_nan()).unwrap_or(1.0);

    SerializedGrid { root, orientation, width: or_one(width), height: or_one(height) }
}
