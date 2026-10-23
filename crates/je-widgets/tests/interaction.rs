//! Testes de interação do `Sash`, do `SplitView` e do `Grid` pelo modelo de eventos do `je-ui`.
//!
//! O VS Code não testa o arraste do sash no mocha (precisaria de um navegador); aqui o `je-ui` entrega a
//! entrada de ponteiro crua e os testes conferem o que o original faz com `mousedown`, `mousemove`, `mouseup` e
//! `dblclick`: eventos `onDidStart`/`onDidChange`/`onDidEnd`/`onDidReset`, hover com atraso, redimensionamento
//! das views, snap, Alt, e o arraste duplo pelo canto (sash ortogonal) e pelo sash ligado.

mod support;

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use je_base::runtime;
use je_ui::{Cursor, Modifiers, PointerInput, PointerInputKind, Role, Ui};
use je_widgets::sash::{HorizontalSashLayoutProvider, Orientation, Sash, SashOptions, SashState, set_global_hover_delay, set_global_sash_hover_border};
use je_widgets::splitview::{LayoutPriority, Sizing, SplitView, SplitViewDescriptor, SplitViewOptions, SplitViewView, ViewDescriptor};
use je_render::Rgba;
use je_render::kurbo::Rect;
use support::{create_container, ensure_no_disposables_are_leaked_in_test_suite};

fn ptr(ui: &Ui, kind: PointerInputKind, x: f64, y: f64) {
    ui.pointer_input(PointerInput::new(kind, x, y));
}

fn vertical_sash(ui: &Ui, container: je_ui::NodeId, left: f64, options: SashOptions) -> Rc<Sash> {
    ui.tree_mut().set_bounds(container, Rect::new(0.0, 0.0, 200.0, 200.0));
    Sash::new_vertical(ui, container, Rc::new(move |_: &Sash| left), options)
}

#[test]
fn dragging_a_sash_fires_start_change_end_with_the_window_coordinates() {
    let store = ensure_no_disposables_are_leaked_in_test_suite();
    let ui = Ui::new();
    let container = create_container(&ui, 200.0, 200.0);
    { let root = ui.root(); ui.tree_mut().append_child(root, container); }
    { let root = ui.root(); ui.tree_mut().set_bounds(root, Rect::new(0.0, 0.0, 200.0, 200.0)); }
    let sash = store.add_rc(vertical_sash(&ui, container, 100.0, SashOptions { size: Some(4.0), ..SashOptions::default() }));
    sash.set_state(SashState::Enabled);
    sash.layout();
    let log = Rc::new(RefCell::new(Vec::new()));
    let (l1, l2, l3) = (log.clone(), log.clone(), log.clone());
    store.add(sash.on_did_start().listen(move |e| l1.borrow_mut().push(format!("start {} {}", e.start_x, e.current_x))));
    store.add(sash.on_did_change().listen(move |e| l2.borrow_mut().push(format!("change {} {}", e.start_x, e.current_x))));
    store.add(sash.on_did_end().listen(move |()| l3.borrow_mut().push("end".to_owned())));

    // O sash vertical em x=100 com 4 de largura ocupa 98..102.
    ptr(&ui, PointerInputKind::Down, 99.0, 50.0);
    assert!(sash.has_class("active"));
    assert_eq!(ui.cursor_at(10.0, 10.0), Cursor::EwResize, "durante o arraste o cursor vale para a janela inteira");
    ptr(&ui, PointerInputKind::Move, 120.0, 55.0);
    ptr(&ui, PointerInputKind::Move, 150.0, 55.0);
    ptr(&ui, PointerInputKind::Up, 150.0, 55.0);

    assert_eq!(*log.borrow(), ["start 99 99", "change 99 120", "change 99 150", "end"]);
    assert!(!sash.has_class("active"));
    assert_eq!(ui.cursor_at(10.0, 10.0), Cursor::Default, "o override some ao soltar");
}

#[test]
fn a_disabled_sash_ignores_the_pointer() {
    let store = ensure_no_disposables_are_leaked_in_test_suite();
    let ui = Ui::new();
    let container = create_container(&ui, 200.0, 200.0);
    { let root = ui.root(); ui.tree_mut().append_child(root, container); }
    { let root = ui.root(); ui.tree_mut().set_bounds(root, Rect::new(0.0, 0.0, 200.0, 200.0)); }
    let sash = store.add_rc(vertical_sash(&ui, container, 100.0, SashOptions { size: Some(4.0), ..SashOptions::default() }));
    sash.set_state(SashState::Disabled);
    let started = Rc::new(Cell::new(false));
    let flag = started.clone();
    store.add(sash.on_did_start().listen(move |_| flag.set(true)));
    ptr(&ui, PointerInputKind::Down, 99.0, 50.0);
    ptr(&ui, PointerInputKind::Up, 99.0, 50.0);
    assert!(!started.get());
    assert!(sash.has_class("disabled"));
}

#[test]
fn double_click_resets() {
    let store = ensure_no_disposables_are_leaked_in_test_suite();
    let ui = Ui::new();
    let container = create_container(&ui, 200.0, 200.0);
    { let root = ui.root(); ui.tree_mut().append_child(root, container); }
    { let root = ui.root(); ui.tree_mut().set_bounds(root, Rect::new(0.0, 0.0, 200.0, 200.0)); }
    let sash = store.add_rc(vertical_sash(&ui, container, 100.0, SashOptions { size: Some(4.0), ..SashOptions::default() }));
    let resets = Rc::new(Cell::new(0));
    let counter = resets.clone();
    store.add(sash.on_did_reset().listen(move |()| counter.set(counter.get() + 1)));
    ptr(&ui, PointerInputKind::Down, 99.0, 50.0);
    let mut up = PointerInput::new(PointerInputKind::Up, 99.0, 50.0);
    up.click_count = 2;
    ui.pointer_input(up);
    assert_eq!(resets.get(), 1);
}

#[test]
fn hover_class_comes_after_the_hover_delay_and_goes_on_leave() {
    let store = ensure_no_disposables_are_leaked_in_test_suite();
    let ui = Ui::new();
    let container = create_container(&ui, 200.0, 200.0);
    { let root = ui.root(); ui.tree_mut().append_child(root, container); }
    { let root = ui.root(); ui.tree_mut().set_bounds(root, Rect::new(0.0, 0.0, 200.0, 200.0)); }
    set_global_hover_delay(300);
    set_global_sash_hover_border(Some(Rgba::new(0, 127, 212, 255)));
    let sash = store.add_rc(vertical_sash(&ui, container, 100.0, SashOptions { size: Some(4.0), ..SashOptions::default() }));
    runtime::with_faked_timers(|| {
        ptr(&ui, PointerInputKind::Move, 99.0, 50.0);
        assert!(!sash.has_class("hover"), "antes do atraso não há destaque");
        runtime::advance_fake_time(299);
        assert!(!sash.has_class("hover"));
        runtime::advance_fake_time(1);
        assert!(sash.has_class("hover"));
        ptr(&ui, PointerInputKind::Move, 10.0, 10.0);
        assert!(!sash.has_class("hover"));
    });
    set_global_sash_hover_border(None);
}

struct Top;

impl HorizontalSashLayoutProvider for Top {
    fn get_horizontal_sash_top(&self, _sash: &Sash) -> f64 {
        50.0
    }
}

#[test]
fn horizontal_sash_is_positioned_by_its_provider_and_global_size_changes_relayout() {
    let store = ensure_no_disposables_are_leaked_in_test_suite();
    let ui = Ui::new();
    let container = create_container(&ui, 200.0, 200.0);
    let sash = store.add_rc(Sash::new_horizontal(&ui, container, Rc::new(Top), SashOptions::default()));
    assert_eq!(sash.orientation(), Orientation::Horizontal);
    let rect = ui.tree().bounds(sash.element());
    assert_eq!((rect.y0, rect.height(), rect.width()), (48.0, 4.0, 200.0));
    je_widgets::sash::set_global_sash_size(6.0);
    let rect = ui.tree().bounds(sash.element());
    je_widgets::sash::set_global_sash_size(4.0);
    assert_eq!((rect.y0, rect.height()), (47.0, 6.0));
}

struct View {
    ui: Ui,
    element: je_ui::NodeId,
    min: f64,
    max: f64,
    snap: bool,
    size: Cell<f64>,
    changes: Rc<je_base::event::Emitter<Option<f64>>>,
    priority: Option<LayoutPriority>,
}

impl View {
    fn new(ui: &Ui, min: f64, max: f64) -> Rc<Self> {
        Rc::new(Self { ui: ui.clone(), element: ui.tree_mut().create_detached(Role::Generic), min, max, snap: false, size: Cell::new(0.0), changes: je_base::event::Emitter::new(), priority: None })
    }

    fn snapping(ui: &Ui, min: f64, max: f64) -> Rc<Self> {
        Rc::new(Self { ui: ui.clone(), element: ui.tree_mut().create_detached(Role::Generic), min, max, snap: true, size: Cell::new(0.0), changes: je_base::event::Emitter::new(), priority: None })
    }
}

impl SplitViewView<()> for View {
    fn element(&self) -> je_ui::NodeId {
        self.element
    }

    fn minimum_size(&self) -> f64 {
        self.min
    }

    fn maximum_size(&self) -> f64 {
        self.max
    }

    fn snap(&self) -> Option<bool> {
        Some(self.snap)
    }

    fn priority(&self) -> Option<LayoutPriority> {
        self.priority
    }

    fn on_did_change(&self) -> je_base::event::Event<Option<f64>> {
        self.changes.event()
    }

    fn layout(&self, size: f64, _offset: f64, _context: Option<&()>) {
        self.size.set(size);
    }
}

impl je_base::lifecycle::IDisposable for View {
    fn dispose(&self) {
        self.changes.dispose();
        self.ui.remove(self.element);
    }
}

/// Um split view horizontal de 300 de largura, já com as views nos tamanhos dados (pelo descritor do
/// construtor, que não redistribui nada).
fn horizontal_split(store: &support::LeakGuard, ui: &Ui, views: &[(&Rc<View>, f64)]) -> Rc<SplitView<()>> {
    let container = create_container(ui, 300.0, 100.0);
    { let root = ui.root(); ui.tree_mut().append_child(root, container); }
    { let root = ui.root(); ui.tree_mut().set_bounds(root, Rect::new(0.0, 0.0, 300.0, 100.0)); }
    let descriptor = SplitViewDescriptor { size: 300.0, views: views.iter().map(|(view, size)| ViewDescriptor { visible: None, size: *size, view: (*view).clone() as _ }).collect() };
    let split = store.add_rc(SplitView::new(ui, container, SplitViewOptions { orientation: Some(Orientation::Horizontal), descriptor: Some(descriptor), ..SplitViewOptions::default() }));
    split.layout(300.0, None);
    split
}

#[test]
fn dragging_a_splitview_sash_resizes_the_neighbours_and_fires_did_sash_change() {
    let store = ensure_no_disposables_are_leaked_in_test_suite();
    let ui = Ui::new();
    let (a, b, c) = (store.add_rc(View::new(&ui, 20.0, f64::INFINITY)), store.add_rc(View::new(&ui, 20.0, f64::INFINITY)), store.add_rc(View::new(&ui, 20.0, f64::INFINITY)));
    let split = horizontal_split(&store, &ui, &[(&a, 100.0), (&b, 100.0), (&c, 100.0)]);
    assert_eq!([a.size.get(), b.size.get(), c.size.get()], [100.0, 100.0, 100.0]);
    let changed = Rc::new(RefCell::new(Vec::new()));
    let sink = changed.clone();
    store.add(split.on_did_sash_change().listen(move |i| sink.borrow_mut().push(*i)));

    // O primeiro sash está em x = 100 (96..104 com o tamanho global de 4).
    ptr(&ui, PointerInputKind::Down, 100.0, 10.0);
    ptr(&ui, PointerInputKind::Move, 130.0, 10.0);
    assert_eq!([a.size.get(), b.size.get(), c.size.get()], [130.0, 70.0, 100.0]);
    // Com o ponteiro longe, o delta é limitado pelo mínimo das views depois do sash: 80 + 80 de folga.
    ptr(&ui, PointerInputKind::Move, 500.0, 10.0);
    assert_eq!([a.size.get(), b.size.get(), c.size.get()], [260.0, 20.0, 20.0]);
    ptr(&ui, PointerInputKind::Up, 500.0, 10.0);
    assert_eq!(*changed.borrow(), [0]);
}

#[test]
fn views_are_not_targets_while_a_sash_drags() {
    let store = ensure_no_disposables_are_leaked_in_test_suite();
    let ui = Ui::new();
    let (a, b) = (store.add_rc(View::new(&ui, 20.0, f64::INFINITY)), store.add_rc(View::new(&ui, 20.0, f64::INFINITY)));
    let split = horizontal_split(&store, &ui, &[(&a, 100.0), (&b, 200.0)]);
    let views = ui.tree().children(split.view_container()).to_vec();
    let inside_first_view = |ui: &Ui| ui.tree().hit_test(10.0, 10.0).is_some_and(|n| ui.tree().is_ancestor(views[0], n));
    assert!(inside_first_view(&ui));
    ptr(&ui, PointerInputKind::Down, 100.0, 10.0);
    assert!(!inside_first_view(&ui), "enquanto arrasta, os contêineres das views não recebem o ponteiro");
    ptr(&ui, PointerInputKind::Up, 100.0, 10.0);
    assert!(inside_first_view(&ui), "depois de soltar, voltam a receber");
}

#[test]
fn snap_hides_the_view_when_dragged_past_half_its_minimum_and_shows_it_back() {
    let store = ensure_no_disposables_are_leaked_in_test_suite();
    let ui = Ui::new();
    let (a, b) = (store.add_rc(View::snapping(&ui, 50.0, f64::INFINITY)), store.add_rc(View::new(&ui, 20.0, f64::INFINITY)));
    let split = horizontal_split(&store, &ui, &[(&a, 100.0), (&b, 200.0)]);
    assert_eq!([a.size.get(), b.size.get()], [100.0, 200.0]);
    assert!(split.is_view_visible(0));
    ptr(&ui, PointerInputKind::Down, 100.0, 10.0);
    // Mínimo 50: o limite do snap é -50 - 25 = -75; arrastar -80 esconde a view.
    ptr(&ui, PointerInputKind::Move, 20.0, 10.0);
    assert!(!split.is_view_visible(0));
    assert_eq!([a.size.get(), b.size.get()], [0.0, 300.0]);
    ptr(&ui, PointerInputKind::Move, 200.0, 10.0);
    assert!(split.is_view_visible(0));
    ptr(&ui, PointerInputKind::Up, 200.0, 10.0);
    assert_eq!([a.size.get(), b.size.get()], [200.0, 100.0]);
}

#[test]
fn alt_while_dragging_resizes_the_view_after_the_sash_on_both_sides() {
    let store = ensure_no_disposables_are_leaked_in_test_suite();
    let ui = Ui::new();
    let (a, b, c) = (store.add_rc(View::new(&ui, 20.0, f64::INFINITY)), store.add_rc(View::new(&ui, 20.0, f64::INFINITY)), store.add_rc(View::new(&ui, 20.0, f64::INFINITY)));
    let split = horizontal_split(&store, &ui, &[(&a, 100.0), (&b, 100.0), (&c, 100.0)]);
    let mut down = PointerInput::new(PointerInputKind::Down, 100.0, 10.0);
    down.modifiers = Modifiers { alt: true, ..Modifiers::default() };
    ui.pointer_input(down);
    ptr(&ui, PointerInputKind::Move, 120.0, 10.0);
    ptr(&ui, PointerInputKind::Up, 120.0, 10.0);
    // Com Alt, a view depois do sash (b) encolhe 40 e as duas vizinhas crescem 20 cada.
    assert_eq!([a.size.get(), b.size.get(), c.size.get()], [120.0, 60.0, 120.0]);
    drop(split);
}

#[test]
fn sizing_split_gives_half_of_the_reference_view() {
    let store = ensure_no_disposables_are_leaked_in_test_suite();
    let ui = Ui::new();
    let (a, b) = (store.add_rc(View::new(&ui, 20.0, f64::INFINITY)), store.add_rc(View::new(&ui, 20.0, f64::INFINITY)));
    let split = horizontal_split(&store, &ui, &[(&a, 300.0)]);
    split.add_view(b.clone(), Sizing::Split(0), None, false);
    assert_eq!([a.size.get(), b.size.get()], [150.0, 150.0]);
}

#[test]
fn dragging_a_sash_of_a_2x2_grid_moves_the_linked_sash_too() {
    use je_widgets::grid::{Direction, Grid, GridOptions, Sizing as GridSizing};
    use support::grid_util::TestView;
    let store = ensure_no_disposables_are_leaked_in_test_suite();
    let ui = Ui::new();
    let mk = || store.add_rc(TestView::new(&ui, 50.0, f64::MAX, 50.0, f64::MAX));
    let view1 = mk();
    let grid = store.add_rc(Grid::new(&ui, view1.clone(), GridOptions::default()));
    let container = create_container(&ui, 800.0, 600.0);
    ui.tree_mut().append_child(container, grid.element());
    { let root = ui.root(); ui.tree_mut().append_child(root, container); }
    { let root = ui.root(); ui.tree_mut().set_bounds(root, Rect::new(0.0, 0.0, 800.0, 600.0)); }
    grid.layout(800.0, 600.0, 0.0, 0.0);

    let (view2, view3, view4) = (mk(), mk(), mk());
    grid.add_view(view2.clone(), GridSizing::Distribute, &view1, Direction::Right);
    grid.add_view(view3.clone(), GridSizing::Distribute, &view1, Direction::Down);
    grid.add_view(view4.clone(), GridSizing::Distribute, &view2, Direction::Down);
    assert_eq!([view1.height(), view2.height(), view3.height(), view4.height()], [300.0, 300.0, 300.0, 300.0]);

    // O sash horizontal da coluna da esquerda está em y = 300.
    ptr(&ui, PointerInputKind::Down, 200.0, 300.0);
    ptr(&ui, PointerInputKind::Move, 200.0, 400.0);
    ptr(&ui, PointerInputKind::Up, 200.0, 400.0);
    assert_eq!([view1.height(), view2.height(), view3.height(), view4.height()], [400.0, 400.0, 200.0, 200.0], "o sash ligado da coluna da direita acompanha");
}
