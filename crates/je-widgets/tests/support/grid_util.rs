//! Porte de `src/vs/base/test/browser/ui/grid/util.ts`: o `TestView` e o `nodesToArrays` dos testes do grid.
//!
//! `TestSerializableView extends TestView` (do `grid.test.ts`) é o mesmo `TestView` com um `name`: as views sem
//! nome são as do `util.ts`, as nomeadas implementam também `SerializableView`.
//!
//! O `deepStrictEqual` entre árvores de views compara a identidade de cada view; [`Nested`] é a árvore de
//! arrays do original, com cada view reduzida à própria identidade (o endereço do `Rc`).

use std::cell::Cell;
use std::rc::Rc;

use je_base::event::{Emitter, Event};
use je_base::lifecycle::IDisposable;
use je_ui::{NodeId, Role, Ui};
use je_widgets::grid::{GridItem, SerializableView};
use je_widgets::gridview::{GridNode, GridViewView, ViewDimensions};

pub struct TestView {
    ui: Ui,
    on_did_change: Rc<Emitter<Option<ViewDimensions>>>,
    minimum_width: Cell<f64>,
    maximum_width: Cell<f64>,
    minimum_height: Cell<f64>,
    maximum_height: Cell<f64>,
    element: NodeId,
    on_did_get_element: Rc<Emitter<()>>,
    width: Cell<f64>,
    height: Cell<f64>,
    top: Cell<f64>,
    left: Cell<f64>,
    on_did_layout: Rc<Emitter<(f64, f64, f64, f64)>>,
    on_did_focus: Rc<Emitter<()>>,
    pub name: String,
}

impl TestView {
    pub fn new(ui: &Ui, minimum_width: f64, maximum_width: f64, minimum_height: f64, maximum_height: f64) -> Rc<Self> {
        Self::named(ui, "", minimum_width, maximum_width, minimum_height, maximum_height)
    }

    /// O `new TestSerializableView(name, ...)`.
    pub fn named(ui: &Ui, name: &str, minimum_width: f64, maximum_width: f64, minimum_height: f64, maximum_height: f64) -> Rc<Self> {
        assert!(minimum_width <= maximum_width, "gridview view minimum width must be <= maximum width");
        assert!(minimum_height <= maximum_height, "gridview view minimum height must be <= maximum height");
        Rc::new(Self {
            ui: ui.clone(),
            on_did_change: Emitter::new(),
            minimum_width: Cell::new(minimum_width),
            maximum_width: Cell::new(maximum_width),
            minimum_height: Cell::new(minimum_height),
            maximum_height: Cell::new(maximum_height),
            element: ui.tree_mut().create_detached(Role::Generic),
            on_did_get_element: Emitter::new(),
            width: Cell::new(0.0),
            height: Cell::new(0.0),
            top: Cell::new(0.0),
            left: Cell::new(0.0),
            on_did_layout: Emitter::new(),
            on_did_focus: Emitter::new(),
            name: name.to_owned(),
        })
    }

    pub fn width(&self) -> f64 {
        self.width.get()
    }

    pub fn height(&self) -> f64 {
        self.height.get()
    }

    pub fn top(&self) -> f64 {
        self.top.get()
    }

    pub fn left(&self) -> f64 {
        self.left.get()
    }

    /// `get size(): [number, number]`.
    pub fn size(&self) -> (f64, f64) {
        (self.width(), self.height())
    }

    pub fn focus(&self) {
        self.on_did_focus.fire(());
    }
}

impl GridViewView for TestView {
    fn element(&self) -> NodeId {
        self.on_did_get_element.fire(());
        self.element
    }

    fn minimum_width(&self) -> f64 {
        self.minimum_width.get()
    }

    fn maximum_width(&self) -> f64 {
        self.maximum_width.get()
    }

    fn minimum_height(&self) -> f64 {
        self.minimum_height.get()
    }

    fn maximum_height(&self) -> f64 {
        self.maximum_height.get()
    }

    fn on_did_change(&self) -> Event<Option<ViewDimensions>> {
        self.on_did_change.event()
    }

    fn layout(&self, width: f64, height: f64, top: f64, left: f64) {
        self.width.set(width);
        self.height.set(height);
        self.top.set(top);
        self.left.set(left);
        self.on_did_layout.fire((width, height, top, left));
    }
}

impl GridItem for TestView {}

/// O `toJSON()` do `TestSerializableView`: `{ name }`.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct TestJson {
    pub name: String,
}

impl SerializableView for TestView {
    type Json = TestJson;

    fn to_json(&self) -> TestJson {
        TestJson { name: self.name.clone() }
    }
}

impl IDisposable for TestView {
    fn dispose(&self) {
        self.on_did_change.dispose();
        self.on_did_get_element.dispose();
        self.on_did_layout.dispose();
        self.on_did_focus.dispose();
        self.ui.remove(self.element);
    }
}

/// A identidade de uma view (o `===` do JavaScript): o endereço do dado dentro do `Rc`.
pub fn rc_id<U: ?Sized>(rc: &Rc<U>) -> usize {
    Rc::as_ptr(rc).cast::<()>() as usize
}

/// As árvores de arrays do `nodesToArrays`: uma view (pela identidade) ou uma lista.
#[derive(Clone, Debug, PartialEq)]
pub enum Nested {
    View(usize),
    List(Vec<Nested>),
}

/// Uma view na árvore esperada.
pub fn view<U: ?Sized>(rc: &Rc<U>) -> Nested {
    Nested::View(rc_id(rc))
}

/// Uma lista na árvore esperada.
pub fn list(items: Vec<Nested>) -> Nested {
    Nested::List(items)
}

pub fn nodes_to_arrays<V>(node: &GridNode<V>, id: &impl Fn(&V) -> usize) -> Nested {
    match node {
        GridNode::Branch(branch) => Nested::List(branch.children.iter().map(|c| nodes_to_arrays(c, id)).collect()),
        GridNode::Leaf(leaf) => Nested::View(id(&leaf.view)),
    }
}
