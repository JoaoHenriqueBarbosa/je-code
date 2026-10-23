//! Testes extras do `CenteredViewLayout` (a 1.143.1 não tem teste dedicado): as contas das margens (por
//! proporção e por largura fixa), a ativação com o `SplitView`, o estado, o reset por duplo clique no sash e o
//! layout sem o split view.

mod support;

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use je_base::event::{Emitter, Event};
use je_base::lifecycle::IDisposable;
use je_render::Rgba;
use je_ui::{Event as UiEvent, EventKind, NodeId, Role, Ui};
use je_widgets::centered_view_layout::{CenteredViewLayout, CenteredViewState, CenteredViewStyles, DEFAULT_STATE};
use je_widgets::gridview::{GridViewView, ViewDimensions};
use je_widgets::splitview::SplitViewStyles;
use support::{create_container, ensure_no_disposables_are_leaked_in_test_suite};

struct TestView {
    element: NodeId,
    minimum_width: Cell<f64>,
    maximum_width: Cell<f64>,
    on_did_change: Rc<Emitter<Option<ViewDimensions>>>,
    layouts: RefCell<Vec<(f64, f64, f64, f64)>>,
}

impl TestView {
    fn new(ui: &Ui) -> Rc<Self> {
        let element = ui.tree_mut().create_detached(Role::Generic);
        Rc::new(Self { element, minimum_width: Cell::new(100.0), maximum_width: Cell::new(f64::INFINITY), on_did_change: Emitter::new(), layouts: RefCell::new(Vec::new()) })
    }

    fn last_layout(&self) -> (f64, f64, f64, f64) {
        *self.layouts.borrow().last().expect("o layout foi chamado")
    }
}

impl GridViewView for TestView {
    fn element(&self) -> NodeId {
        self.element
    }

    fn minimum_width(&self) -> f64 {
        self.minimum_width.get()
    }

    fn maximum_width(&self) -> f64 {
        self.maximum_width.get()
    }

    fn minimum_height(&self) -> f64 {
        20.0
    }

    fn maximum_height(&self) -> f64 {
        500.0
    }

    fn on_did_change(&self) -> Event<Option<ViewDimensions>> {
        self.on_did_change.event()
    }

    fn layout(&self, width: f64, height: f64, top: f64, left: f64) {
        self.layouts.borrow_mut().push((width, height, top, left));
    }
}

struct Fixture {
    ui: Ui,
    container: NodeId,
    view: Rc<TestView>,
    layout: Rc<CenteredViewLayout>,
}

fn fixture(store: &support::LeakGuard, state: Option<CenteredViewState>, fixed_width: bool) -> Fixture {
    let ui = Ui::new();
    let container = create_container(&ui, 1000.0, 600.0);
    let view = TestView::new(&ui);
    let layout = store.add_rc(CenteredViewLayout::new(&ui, container, view.clone(), state, fixed_width));
    Fixture { ui, container, view, layout }
}

fn sizes(layout: &CenteredViewLayout) -> [f64; 3] {
    let split_view = layout.split_view().expect("ativo");
    [split_view.get_view_size(0), split_view.get_view_size(1), split_view.get_view_size(2)]
}

#[test]
fn the_view_goes_in_the_container_which_clips() {
    let store = ensure_no_disposables_are_leaked_in_test_suite();
    let f = fixture(&store, None, false);
    assert_eq!(f.ui.tree().children(f.container), [f.view.element]);
    assert!(f.ui.tree().style(f.container).clip);
    assert!(!f.layout.is_active());
    assert_eq!(f.layout.state(), DEFAULT_STATE);
}

#[test]
fn inactive_it_forwards_layout_and_the_limits_of_the_view() {
    let store = ensure_no_disposables_are_leaked_in_test_suite();
    let f = fixture(&store, None, false);
    f.layout.layout(800.0, 500.0, 7.0, 9.0);
    assert_eq!(f.view.last_layout(), (800.0, 500.0, 7.0, 9.0));
    assert_eq!((f.layout.minimum_width(), f.layout.maximum_width()), (100.0, f64::INFINITY));
    assert_eq!((f.layout.minimum_height(), f.layout.maximum_height()), (20.0, 500.0));
}

#[test]
fn activating_puts_the_view_between_two_margins_split_by_the_ratios() {
    let store = ensure_no_disposables_are_leaked_in_test_suite();
    let f = fixture(&store, None, false);
    f.layout.layout(1000.0, 500.0, 0.0, 0.0);
    f.layout.activate(true);
    assert!(f.layout.is_active());
    let tree = f.ui.tree();
    assert!(!tree.children(f.container).contains(&f.view.element), "a view saiu do contêiner");
    assert_eq!(tree.children_with_class(f.container, "monaco-split-view2").len(), 1);
    let margins = query_by_class(&tree, f.container, "centered-layout-margin");
    assert_eq!(margins.len(), 2);
    drop(tree);

    // 0,1909 de 1000 é 190,9 e o centro 618,2: o `SplitView` arredonda os tamanhos das views.
    assert_eq!(sizes(&f.layout), [191.0, 618.0, 191.0]);
    assert_eq!(f.view.last_layout(), (618.0, 500.0, 0.0, 191.0), "o centro recebe a altura, o topo e a posição esquerda");
}

fn query_by_class(tree: &je_ui::Tree, root: NodeId, class: &str) -> Vec<NodeId> {
    support::query_all(tree, root, &[class])
}

#[test]
fn the_margins_have_the_style_background() {
    let store = ensure_no_disposables_are_leaked_in_test_suite();
    let f = fixture(&store, None, false);
    let background = Rgba::opaque(10, 20, 30);
    f.layout.styles(CenteredViewStyles { split_view: SplitViewStyles::default(), background });
    f.layout.layout(1000.0, 500.0, 0.0, 0.0);
    f.layout.activate(true);
    let tree = f.ui.tree();
    for margin in query_by_class(&tree, f.container, "centered-layout-margin") {
        assert_eq!(tree.style(margin).background, Some(background));
    }
    drop(tree);
    let other = Rgba::opaque(1, 2, 3);
    f.layout.styles(CenteredViewStyles { split_view: SplitViewStyles::default(), background: other });
    let tree = f.ui.tree();
    for margin in query_by_class(&tree, f.container, "centered-layout-margin") {
        assert_eq!(tree.style(margin).background, Some(other));
    }
}

#[test]
fn fixed_width_centers_the_target_width_with_floor_and_ceil_margins() {
    let store = ensure_no_disposables_are_leaked_in_test_suite();
    let f = fixture(&store, None, true);
    f.layout.layout(1101.0, 500.0, 0.0, 0.0);
    f.layout.activate(true);
    assert_eq!(sizes(&f.layout), [100.0, 900.0, 101.0], "100,5 de margem: `floor` à esquerda, `ceil` à direita");
    // Com largura fixa cada `layout` refaz as contas.
    f.layout.layout(1200.0, 500.0, 0.0, 0.0);
    assert_eq!(sizes(&f.layout), [150.0, 900.0, 150.0]);
}

#[test]
fn fixed_width_keeps_the_minimum_margin_of_60_px() {
    let store = ensure_no_disposables_are_leaked_in_test_suite();
    let f = fixture(&store, None, true);
    f.layout.layout(1001.0, 500.0, 0.0, 0.0);
    f.layout.activate(true);
    assert_eq!(sizes(&f.layout), [60.0, 881.0, 60.0], "as margens não passam do mínimo e o centro cede");
}

#[test]
fn proportional_layout_only_resizes_on_the_first_layout() {
    let store = ensure_no_disposables_are_leaked_in_test_suite();
    let f = fixture(&store, None, false);
    f.layout.layout(1000.0, 500.0, 0.0, 0.0);
    f.layout.activate(true);
    let before = sizes(&f.layout);
    f.layout.layout(2000.0, 500.0, 0.0, 0.0);
    let after = sizes(&f.layout);
    assert_ne!(before, after, "o split view redistribui por conta própria");
    assert!((after.iter().sum::<f64>() - 2000.0).abs() < 1e-6);
}

#[test]
fn set_fixed_width_reads_the_state_before_resizing() {
    let store = ensure_no_disposables_are_leaked_in_test_suite();
    let f = fixture(&store, None, false);
    f.layout.layout(1000.0, 500.0, 0.0, 0.0);
    f.layout.activate(true);
    f.layout.set_fixed_width(true);
    // `updateState` guardou a largura do centro (618,2) antes de o modo fixo refazer as contas.
    assert_eq!(f.layout.state().target_width, 618.0);
    assert_eq!((f.layout.state().left_margin_ratio, f.layout.state().right_margin_ratio), (0.191, 0.191));
    assert_eq!(sizes(&f.layout), [191.0, 618.0, 191.0]);
}

#[test]
fn double_clicking_a_sash_restores_the_default_state() {
    let store = ensure_no_disposables_are_leaked_in_test_suite();
    let state = CenteredViewState { target_width: 700.0, left_margin_ratio: 0.3, right_margin_ratio: 0.1 };
    let f = fixture(&store, Some(state), false);
    f.layout.layout(1000.0, 500.0, 0.0, 0.0);
    f.layout.activate(true);
    assert_eq!(sizes(&f.layout), [300.0, 600.0, 100.0]);
    let sash = f.layout.split_view().expect("ativo").sashes()[0].element();
    f.ui.dispatch(&mut UiEvent::new(EventKind::DoubleClick, sash));
    assert_eq!(f.layout.state(), DEFAULT_STATE);
    assert_eq!(sizes(&f.layout), [191.0, 618.0, 191.0]);
}

#[test]
fn deactivating_puts_the_view_back_and_lays_it_out_in_the_last_position() {
    let store = ensure_no_disposables_are_leaked_in_test_suite();
    let f = fixture(&store, None, false);
    f.layout.layout(1000.0, 500.0, 4.0, 6.0);
    f.layout.activate(true);
    f.layout.activate(false);
    assert!(!f.layout.is_active());
    assert_eq!(f.ui.tree().children(f.container), [f.view.element]);
    assert_eq!(f.view.last_layout(), (1000.0, 500.0, 4.0, 6.0));
    assert!(query_by_class(&f.ui.tree(), f.container, "centered-layout-margin").is_empty());
    assert_eq!(f.layout.minimum_width(), 100.0);
}

#[test]
fn activating_twice_does_nothing_the_second_time() {
    let store = ensure_no_disposables_are_leaked_in_test_suite();
    let f = fixture(&store, None, false);
    f.layout.layout(1000.0, 500.0, 0.0, 0.0);
    f.layout.activate(true);
    let split_view = f.layout.split_view().expect("ativo");
    f.layout.activate(true);
    assert!(Rc::ptr_eq(&split_view, &f.layout.split_view().expect("ativo")));
}

#[test]
fn active_the_limits_are_the_ones_of_the_split_view() {
    let store = ensure_no_disposables_are_leaked_in_test_suite();
    let f = fixture(&store, None, false);
    f.layout.layout(1000.0, 500.0, 0.0, 0.0);
    f.layout.activate(true);
    // Duas margens de 60 px e a view de 100 px no mínimo.
    assert_eq!(f.layout.minimum_width(), 60.0 + 100.0 + 60.0);
    assert_eq!(f.layout.maximum_width(), f64::INFINITY);
}

#[test]
fn is_default_compares_the_ratios_or_the_target_width() {
    let store = ensure_no_disposables_are_leaked_in_test_suite();
    let proportional = fixture(&store, None, false);
    assert!(proportional.layout.is_default(&DEFAULT_STATE));
    assert!(!proportional.layout.is_default(&CenteredViewState { left_margin_ratio: 0.2, ..DEFAULT_STATE }));
    assert!(proportional.layout.is_default(&CenteredViewState { target_width: 1.0, ..DEFAULT_STATE }), "a largura não conta no modo proporcional");
    let fixed = fixture(&store, None, true);
    assert!(fixed.layout.is_default(&CenteredViewState { left_margin_ratio: 0.5, ..DEFAULT_STATE }), "as proporções não contam no modo fixo");
    assert!(!fixed.layout.is_default(&CenteredViewState { target_width: 901.0, ..DEFAULT_STATE }));
}

#[test]
fn dispose_disposes_the_split_view() {
    let ui = Ui::new();
    let container = create_container(&ui, 1000.0, 600.0);
    let view = TestView::new(&ui);
    let layout = CenteredViewLayout::new(&ui, container, view, None, false);
    layout.layout(1000.0, 500.0, 0.0, 0.0);
    layout.activate(true);
    layout.dispose();
    assert!(!layout.is_active());
}
