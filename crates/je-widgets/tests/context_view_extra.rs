//! Testes extras da `ContextView` (`src/vs/base/browser/ui/contextview/contextview.ts`): posicionamento com
//! `layout2d` em cada alinhamento, posição e eixo, os três tipos de âncora, `canRelayout`, camadas, troca
//! de contêiner e ouvintes de DOM. Não fazem parte do porte caso a caso (esse é `tests/context_view.rs`).

mod support;

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use je_base::layout::{AnchorAlignment, AnchorAxisAlignment, AnchorPosition, IAnchor};
use je_base::lifecycle::{DisposableRef, IDisposable};
use je_render::kurbo::Rect;
use je_ui::{Event, NodeId, PointerInput, PointerInputKind, Role, Ui};
use je_widgets::context_view::{ContextView, get_anchor_rect, has_required_ancestor_classes, is_anchor};
use je_widgets::context_view_api::{Anchor, ContextViewDOMPosition, IContextViewDelegate};
use support::ensure_no_disposables_are_leaked_in_test_suite;

/// Um delegado com tudo configurável e contadores das chamadas.
struct Delegate {
    anchor: RefCell<Anchor>,
    size: (f64, f64),
    ui: Ui,
    alignment: Option<AnchorAlignment>,
    position: Option<AnchorPosition>,
    axis: Option<AnchorAxisAlignment>,
    can_relayout: bool,
    layer: i32,
    handles_events: bool,
    focused: Cell<u32>,
    laid_out: Cell<u32>,
    hidden: RefCell<Vec<Option<bool>>>,
    events: RefCell<Vec<(je_ui::EventKind, Option<NodeId>)>>,
}

impl Delegate {
    fn new(ui: &Ui, anchor: Anchor, size: (f64, f64)) -> Self {
        Self { anchor: RefCell::new(anchor), size, ui: ui.clone(), alignment: None, position: None, axis: None, can_relayout: true, layer: 0, handles_events: false, focused: Cell::new(0), laid_out: Cell::new(0), hidden: RefCell::new(Vec::new()), events: RefCell::new(Vec::new()) }
    }
}

impl IContextViewDelegate for Delegate {
    fn get_anchor(&self) -> Anchor {
        *self.anchor.borrow()
    }
    fn render(&self, container: NodeId) -> Option<DisposableRef> {
        self.ui.tree_mut().set_bounds(container, Rect::new(0.0, 0.0, self.size.0, self.size.1));
        None
    }
    fn focus(&self) {
        self.focused.set(self.focused.get() + 1);
    }
    fn layout(&self) {
        self.laid_out.set(self.laid_out.get() + 1);
    }
    fn anchor_alignment(&self) -> Option<AnchorAlignment> {
        self.alignment
    }
    fn anchor_position(&self) -> Option<AnchorPosition> {
        self.position
    }
    fn anchor_axis_alignment(&self) -> Option<AnchorAxisAlignment> {
        self.axis
    }
    fn can_relayout(&self) -> bool {
        self.can_relayout
    }
    fn layer(&self) -> i32 {
        self.layer
    }
    fn handles_dom_events(&self) -> bool {
        self.handles_events
    }
    fn on_dom_event(&self, event: &Event, active_element: Option<NodeId>) {
        self.events.borrow_mut().push((event.kind, active_element));
    }
    fn on_hide(&self, did_cancel: Option<bool>) {
        self.hidden.borrow_mut().push(did_cancel);
    }
}

fn setup() -> (Ui, NodeId) {
    let ui = Ui::new();
    let root = ui.root();
    let container = {
        let mut tree = ui.tree_mut();
        tree.set_bounds(root, Rect::new(0.0, 0.0, 800.0, 600.0));
        let container = tree.create(root, Role::Generic);
        tree.set_bounds(container, Rect::new(0.0, 0.0, 800.0, 600.0));
        container
    };
    (ui, container)
}

fn rect_anchor(x: f64, y: f64, width: f64, height: f64) -> Anchor {
    Anchor::Rect(IAnchor { x, y, width: Some(width), height: Some(height) })
}

fn view_rect(ui: &Ui, cv: &ContextView) -> Rect {
    ui.tree().absolute_bounds(cv.get_view_element())
}

fn classes(ui: &Ui, cv: &ContextView) -> Vec<&'static str> {
    ["top", "bottom", "left", "right", "fixed"].into_iter().filter(|c| ui.tree().has_class(cv.get_view_element(), c)).collect()
}

#[test]
fn it_shows_below_the_anchor_aligned_left_by_default() {
    let _store = ensure_no_disposables_are_leaked_in_test_suite();
    let (ui, container) = setup();
    let cv = ContextView::new(&ui, container, ContextViewDOMPosition::Absolute);
    cv.show(Rc::new(Delegate::new(&ui, rect_anchor(100.0, 100.0, 40.0, 20.0), (50.0, 30.0))));
    assert_eq!(view_rect(&ui, &cv), Rect::new(100.0, 120.0, 150.0, 150.0));
    assert_eq!(classes(&ui, &cv), ["bottom", "left"]);
    assert!(ui.tree().has_class(cv.get_view_element(), "context-view") && ui.tree().has_class(cv.get_view_element(), "monaco-component"));
    assert!(ui.tree().style(cv.get_view_element()).visible);
    cv.dispose();
}

#[test]
fn it_flips_above_and_to_the_left_when_there_is_no_room() {
    let _store = ensure_no_disposables_are_leaked_in_test_suite();
    let (ui, container) = setup();
    let cv = ContextView::new(&ui, container, ContextViewDOMPosition::Absolute);
    cv.show(Rc::new(Delegate::new(&ui, rect_anchor(780.0, 590.0, 10.0, 10.0), (50.0, 30.0))));
    let r = view_rect(&ui, &cv);
    assert_eq!(r.y1, 590.0, "acima: a borda de baixo da visão fica no topo da âncora");
    assert_eq!(r.x1, 790.0, "à esquerda: a borda direita da visão alinha com a da âncora");
    assert_eq!(classes(&ui, &cv), ["top", "right"]);
    cv.dispose();
}

#[test]
fn the_delegate_picks_alignment_position_and_axis() {
    let _store = ensure_no_disposables_are_leaked_in_test_suite();
    let (ui, container) = setup();
    let cv = ContextView::new(&ui, container, ContextViewDOMPosition::Absolute);
    let mut right = Delegate::new(&ui, rect_anchor(300.0, 300.0, 40.0, 20.0), (50.0, 30.0));
    right.alignment = Some(AnchorAlignment::Right);
    cv.show(Rc::new(right));
    assert_eq!(view_rect(&ui, &cv).x1, 340.0, "alinhada à direita da âncora");
    assert_eq!(classes(&ui, &cv), ["bottom", "right"]);

    let mut above = Delegate::new(&ui, rect_anchor(300.0, 300.0, 40.0, 20.0), (50.0, 30.0));
    above.position = Some(AnchorPosition::Above);
    cv.show(Rc::new(above));
    assert_eq!(view_rect(&ui, &cv).y1, 300.0);
    assert_eq!(classes(&ui, &cv), ["top", "left"]);

    let mut horizontal = Delegate::new(&ui, rect_anchor(300.0, 300.0, 40.0, 20.0), (50.0, 30.0));
    horizontal.axis = Some(AnchorAxisAlignment::Horizontal);
    cv.show(Rc::new(horizontal));
    let r = view_rect(&ui, &cv);
    assert_eq!(r.x0, 340.0, "no eixo horizontal a visão fica ao lado da âncora, não embaixo");
    cv.dispose();
}

#[test]
fn the_three_anchor_kinds_have_the_documented_rectangles() {
    let _store = ensure_no_disposables_are_leaked_in_test_suite();
    let (ui, container) = setup();
    let node = {
        let mut tree = ui.tree_mut();
        let node = tree.create(container, Role::Generic);
        tree.set_bounds(node, Rect::new(30.0, 40.0, 130.0, 70.0));
        node
    };
    let r = get_anchor_rect(&ui, &Anchor::Node(node));
    assert_eq!((r.left, r.top, r.width, r.height), (30.0, 40.0, 100.0, 30.0));
    let r = get_anchor_rect(&ui, &Anchor::Mouse { x: 5.0, y: 6.0 });
    assert_eq!((r.left, r.top, r.width, r.height), (5.0, 6.0, 2.0, 2.0), "o ponteiro é um bloco de 2x2");
    let r = get_anchor_rect(&ui, &Anchor::Rect(IAnchor { x: 1.0, y: 2.0, width: None, height: None }));
    assert_eq!((r.width, r.height), (1.0, 2.0), "a âncora avulsa tem 1x2 por padrão");
    let r = get_anchor_rect(&ui, &Anchor::Rect(IAnchor { x: 1.0, y: 2.0, width: Some(0.0), height: Some(0.0) }));
    assert_eq!((r.width, r.height), (1.0, 2.0), "zero também cai no padrão (o `||` do original)");
    assert!(is_anchor(&Anchor::Rect(IAnchor { x: 0.0, y: 0.0, width: None, height: None })) && !is_anchor(&Anchor::Mouse { x: 0.0, y: 0.0 }));
}

#[test]
fn a_node_anchor_is_positioned_just_below_the_node() {
    let _store = ensure_no_disposables_are_leaked_in_test_suite();
    let (ui, container) = setup();
    let node = {
        let mut tree = ui.tree_mut();
        let node = tree.create(container, Role::Generic);
        tree.set_bounds(node, Rect::new(30.0, 40.0, 130.0, 70.0));
        node
    };
    let cv = ContextView::new(&ui, container, ContextViewDOMPosition::Absolute);
    cv.show(Rc::new(Delegate::new(&ui, Anchor::Node(node), (50.0, 30.0))));
    assert_eq!(view_rect(&ui, &cv).min_x(), 30.0);
    assert_eq!(view_rect(&ui, &cv).min_y(), 70.0);
    cv.dispose();
}

#[test]
fn a_view_without_a_size_adopts_the_box_of_its_children() {
    let _store = ensure_no_disposables_are_leaked_in_test_suite();
    let (ui, container) = setup();
    struct Child(Ui);
    impl IContextViewDelegate for Child {
        fn get_anchor(&self) -> Anchor {
            Anchor::Rect(IAnchor { x: 10.0, y: 10.0, width: Some(1.0), height: Some(1.0) })
        }
        fn render(&self, container: NodeId) -> Option<DisposableRef> {
            let mut tree = self.0.tree_mut();
            let child = tree.create(container, Role::Generic);
            tree.set_bounds(child, Rect::new(0.0, 0.0, 70.0, 20.0));
            None
        }
    }
    let cv = ContextView::new(&ui, container, ContextViewDOMPosition::Absolute);
    cv.show(Rc::new(Child(ui.clone())));
    let r = view_rect(&ui, &cv);
    assert_eq!((r.width(), r.height()), (70.0, 20.0));
    cv.dispose();
}

#[test]
fn layout_repositions_calls_the_delegate_and_can_hide_when_relayout_is_not_allowed() {
    let _store = ensure_no_disposables_are_leaked_in_test_suite();
    let (ui, container) = setup();
    let cv = ContextView::new(&ui, container, ContextViewDOMPosition::Absolute);
    let delegate = Rc::new(Delegate::new(&ui, rect_anchor(100.0, 100.0, 10.0, 10.0), (20.0, 20.0)));
    cv.show(delegate.clone());
    assert_eq!((delegate.focused.get(), delegate.laid_out.get()), (1, 0));
    *delegate.anchor.borrow_mut() = rect_anchor(200.0, 200.0, 10.0, 10.0);
    cv.layout();
    assert_eq!(delegate.laid_out.get(), 1);
    assert_eq!(view_rect(&ui, &cv).min_x(), 200.0);

    let strict = Rc::new(Delegate { can_relayout: false, ..Delegate::new(&ui, rect_anchor(100.0, 100.0, 10.0, 10.0), (20.0, 20.0)) });
    cv.show(strict.clone());
    cv.layout();
    assert_eq!(*strict.hidden.borrow(), [None], "sem canRelayout, layout() esconde a visão");
    assert!(!ui.tree().style(cv.get_view_element()).visible);
    cv.layout(); // sem delegado: nada
    cv.dispose();
}

#[test]
fn showing_again_hides_the_previous_view_without_animation_and_notifies_it() {
    let _store = ensure_no_disposables_are_leaked_in_test_suite();
    let (ui, container) = setup();
    let cv = ContextView::new(&ui, container, ContextViewDOMPosition::Absolute);
    let first = Rc::new(Delegate::new(&ui, rect_anchor(1.0, 1.0, 1.0, 1.0), (20.0, 20.0)));
    let second = Rc::new(Delegate::new(&ui, rect_anchor(1.0, 1.0, 1.0, 1.0), (20.0, 20.0)));
    cv.show(first.clone());
    cv.show(second.clone());
    assert_eq!(first.hidden.borrow().len(), 1);
    assert!(second.hidden.borrow().is_empty());
    cv.hide_with(Some(true), false);
    assert_eq!(*second.hidden.borrow(), [Some(true)], "o `data` do hide chega ao on_hide");
    cv.dispose();
}

#[test]
fn the_layer_raises_the_z_index_and_fixed_positions_toggle_the_fixed_class() {
    let _store = ensure_no_disposables_are_leaked_in_test_suite();
    let (ui, container) = setup();
    let cv = ContextView::new(&ui, container, ContextViewDOMPosition::Fixed);
    assert!(cv.is_fixed());
    let mut layered = Delegate::new(&ui, rect_anchor(1.0, 1.0, 1.0, 1.0), (20.0, 20.0));
    layered.layer = 7;
    cv.show(Rc::new(layered));
    assert_eq!(ui.tree().style(cv.get_view_element()).z_index, 2575 + 7);
    assert!(ui.tree().has_class(cv.get_view_element(), "fixed"));
    cv.dispose();
}

#[test]
fn set_container_moves_the_view_and_dispose_removes_it() {
    let _store = ensure_no_disposables_are_leaked_in_test_suite();
    let (ui, container) = setup();
    let other = {
        let mut tree = ui.tree_mut();
        let other = tree.create(container, Role::Generic);
        tree.set_bounds(other, Rect::new(100.0, 100.0, 300.0, 300.0));
        other
    };
    let cv = ContextView::new(&ui, container, ContextViewDOMPosition::Absolute);
    assert_eq!(ui.tree().parent(cv.get_view_element()), Some(container));
    cv.set_container(Some(other), ContextViewDOMPosition::Absolute);
    assert_eq!(ui.tree().parent(cv.get_view_element()), Some(other));
    cv.show(Rc::new(Delegate::new(&ui, rect_anchor(150.0, 150.0, 1.0, 1.0), (20.0, 20.0))));
    assert_eq!(view_rect(&ui, &cv).min_x(), 150.0, "a posição é em coordenadas da janela, sem importar o contêiner");
    cv.dispose();
    assert!(ui.tree().parent(cv.get_view_element()).is_none(), "dispose tira a visão do contêiner");
}

#[test]
fn dom_events_reach_the_delegate_and_a_click_inside_the_container_does_not_hide() {
    let _store = ensure_no_disposables_are_leaked_in_test_suite();
    let (ui, container) = setup();
    let cv = ContextView::new(&ui, container, ContextViewDOMPosition::Absolute);
    let mut with_events = Delegate::new(&ui, rect_anchor(100.0, 100.0, 1.0, 1.0), (20.0, 20.0));
    with_events.handles_events = true;
    let with_events = Rc::new(with_events);
    cv.show(with_events.clone());
    ui.pointer_input(PointerInput::new(PointerInputKind::Down, 500.0, 500.0));
    ui.pointer_input(PointerInput::new(PointerInputKind::Up, 500.0, 500.0));
    let kinds: Vec<_> = with_events.events.borrow().iter().map(|(k, _)| *k).collect();
    assert_eq!(kinds, [je_ui::EventKind::Click, je_ui::EventKind::Click], "um clique na captura e outro na borbulha");

    let plain = Rc::new(Delegate::new(&ui, rect_anchor(100.0, 100.0, 1.0, 1.0), (20.0, 20.0)));
    cv.show(plain.clone());
    ui.pointer_input(PointerInput::new(PointerInputKind::Down, 500.0, 500.0));
    ui.pointer_input(PointerInput::new(PointerInputKind::Up, 500.0, 500.0));
    // O alvo está dentro do contêiner (o `isAncestor(target, container)` do original), então não esconde.
    assert!(plain.hidden.borrow().is_empty());
    cv.dispose();
}

#[test]
fn required_ancestor_classes_match_one_ancestor_and_accept_alternatives() {
    let (ui, container) = setup();
    let inner = {
        let mut tree = ui.tree_mut();
        tree.add_class(container, "a");
        tree.add_class(container, "c");
        let inner = tree.create(container, Role::Generic);
        tree.add_class(inner, "b");
        inner
    };
    let req = |groups: &[&[&str]]| groups.iter().map(|g| g.iter().map(|c| (*c).to_owned()).collect::<Vec<_>>()).collect::<Vec<_>>();
    assert!(has_required_ancestor_classes(&ui, inner, &[]), "sem requisitos, vale");
    assert!(has_required_ancestor_classes(&ui, inner, &req(&[&["a", "x"], &["c"]])), "um antepassado cumpre todos, cada um com alternativas");
    assert!(!has_required_ancestor_classes(&ui, inner, &req(&[&["a"], &["b"]])), "os requisitos têm de valer no mesmo elemento");
    assert!(!has_required_ancestor_classes(&ui, inner, &req(&[&["x"]])));
}
