//! Porte de `src/vs/base/test/browser/ui/grid/gridview.test.ts` (suíte `Gridview`), caso a caso e na mesma ordem.
//!
//! Notas de tradução:
//! - `container.appendChild(gridview.element)` é `Tree::append_child` num nó solto de 200x200.
//! - `nodesToArrays` compara a identidade das views (`support::grid_util::Nested`).
//! - `assert.throws` é `support::assert_throws`.

mod support;

use std::rc::Rc;

use je_ui::Ui;
use je_widgets::gridview::{GridView, GridViewOptions, GridViewRef, Orientation, Sizing, ViewDimensions};
use support::grid_util::{Nested, TestView, list, nodes_to_arrays, rc_id, view};
use support::{assert_throws, create_container, ensure_no_disposables_are_leaked_in_test_suite};

const INF: f64 = f64::INFINITY;

fn create_grid_view(store: &support::LeakGuard, ui: &Ui) -> Rc<GridView> {
    let gridview = store.add_rc(GridView::new(ui, GridViewOptions::default()));
    let container = create_container(ui, 200.0, 200.0);
    ui.tree_mut().append_child(container, gridview.element());
    gridview
}

fn arrays(gridview: &GridView) -> Nested {
    nodes_to_arrays(&je_widgets::gridview::GridNode::Branch(gridview.get_view()), &|v: &GridViewRef| rc_id(v))
}

fn add(gridview: &GridView, v: &Rc<TestView>, size: impl Into<je_widgets::gridview::ViewSize>, location: &[usize]) {
    let view: GridViewRef = v.clone();
    gridview.add_view(view, size, location);
}

fn size_of(gridview: &GridView, location: &[usize]) -> ViewDimensions {
    gridview.get_view_size(Some(location))
}

fn dims(width: f64, height: f64) -> ViewDimensions {
    ViewDimensions { width, height }
}

fn new_view(store: &support::LeakGuard, ui: &Ui, min_w: f64, max_w: f64, min_h: f64, max_h: f64) -> Rc<TestView> {
    store.add_rc(TestView::new(ui, min_w, max_w, min_h, max_h))
}

#[test]
fn empty_gridview_is_empty() {
    let store = ensure_no_disposables_are_leaked_in_test_suite();
    let ui = Ui::new();
    let gridview = create_grid_view(&store, &ui);
    assert_eq!(arrays(&gridview), list(vec![]));
}

#[test]
fn gridview_add_view() {
    let store = ensure_no_disposables_are_leaked_in_test_suite();
    let ui = Ui::new();
    let gridview = create_grid_view(&store, &ui);

    let v = new_view(&store, &ui, 20.0, 20.0, 20.0, 20.0);
    assert_throws(|| add(&gridview, &v, 200.0, &[])); // empty location
    assert_throws(|| add(&gridview, &v, 200.0, &[1])); // index overflow
    assert_throws(|| add(&gridview, &v, 200.0, &[0, 0])); // hierarchy overflow

    let views = [new_view(&store, &ui, 20.0, 20.0, 20.0, 20.0), new_view(&store, &ui, 20.0, 20.0, 20.0, 20.0), new_view(&store, &ui, 20.0, 20.0, 20.0, 20.0)];

    add(&gridview, &views[0], 200.0, &[0]);
    add(&gridview, &views[1], 200.0, &[1]);
    add(&gridview, &views[2], 200.0, &[2]);

    assert_eq!(arrays(&gridview), list(views.iter().map(view).collect()));
}

#[test]
fn gridview_add_view_nested() {
    let store = ensure_no_disposables_are_leaked_in_test_suite();
    let ui = Ui::new();
    let gridview = create_grid_view(&store, &ui);

    let views = (new_view(&store, &ui, 20.0, 20.0, 20.0, 20.0), [new_view(&store, &ui, 20.0, 20.0, 20.0, 20.0), new_view(&store, &ui, 20.0, 20.0, 20.0, 20.0)]);

    add(&gridview, &views.0, 200.0, &[0]);
    add(&gridview, &views.1[0], 200.0, &[1]);
    add(&gridview, &views.1[1], 200.0, &[1, 1]);

    assert_eq!(arrays(&gridview), list(vec![view(&views.0), list(vec![view(&views.1[0]), view(&views.1[1])])]));
}

#[test]
fn gridview_add_view_deep_nested() {
    let store = ensure_no_disposables_are_leaked_in_test_suite();
    let ui = Ui::new();
    let gridview = create_grid_view(&store, &ui);

    let view1 = new_view(&store, &ui, 20.0, 20.0, 20.0, 20.0);
    add(&gridview, &view1, 200.0, &[0]);
    assert_eq!(arrays(&gridview), list(vec![view(&view1)]));

    let view2 = new_view(&store, &ui, 20.0, 20.0, 20.0, 20.0);
    add(&gridview, &view2, 200.0, &[1]);
    assert_eq!(arrays(&gridview), list(vec![view(&view1), view(&view2)]));

    let view3 = new_view(&store, &ui, 20.0, 20.0, 20.0, 20.0);
    add(&gridview, &view3, 200.0, &[1, 0]);
    assert_eq!(arrays(&gridview), list(vec![view(&view1), list(vec![view(&view3), view(&view2)])]));

    let view4 = new_view(&store, &ui, 20.0, 20.0, 20.0, 20.0);
    add(&gridview, &view4, 200.0, &[1, 0, 0]);
    assert_eq!(arrays(&gridview), list(vec![view(&view1), list(vec![list(vec![view(&view4), view(&view3)]), view(&view2)])]));

    let view5 = new_view(&store, &ui, 20.0, 20.0, 20.0, 20.0);
    add(&gridview, &view5, 200.0, &[1, 0]);
    assert_eq!(arrays(&gridview), list(vec![view(&view1), list(vec![view(&view5), list(vec![view(&view4), view(&view3)]), view(&view2)])]));

    let view6 = new_view(&store, &ui, 20.0, 20.0, 20.0, 20.0);
    add(&gridview, &view6, 200.0, &[2]);
    assert_eq!(arrays(&gridview), list(vec![view(&view1), list(vec![view(&view5), list(vec![view(&view4), view(&view3)]), view(&view2)]), view(&view6)]));

    let view7 = new_view(&store, &ui, 20.0, 20.0, 20.0, 20.0);
    add(&gridview, &view7, 200.0, &[1, 1]);
    assert_eq!(arrays(&gridview), list(vec![view(&view1), list(vec![view(&view5), view(&view7), list(vec![view(&view4), view(&view3)]), view(&view2)]), view(&view6)]));

    let view8 = new_view(&store, &ui, 20.0, 20.0, 20.0, 20.0);
    add(&gridview, &view8, 200.0, &[1, 1, 0]);
    assert_eq!(
        arrays(&gridview),
        list(vec![view(&view1), list(vec![view(&view5), list(vec![view(&view8), view(&view7)]), list(vec![view(&view4), view(&view3)]), view(&view2)]), view(&view6)])
    );
}

#[test]
fn simple_layout() {
    let store = ensure_no_disposables_are_leaked_in_test_suite();
    let ui = Ui::new();
    let gridview = create_grid_view(&store, &ui);
    gridview.layout(800.0, 600.0, 0.0, 0.0);

    let view1 = new_view(&store, &ui, 50.0, INF, 50.0, INF);
    add(&gridview, &view1, 200.0, &[0]);
    assert_eq!(view1.size(), (800.0, 600.0));
    assert_eq!(size_of(&gridview, &[0]), dims(800.0, 600.0));

    let view2 = new_view(&store, &ui, 50.0, INF, 50.0, INF);
    add(&gridview, &view2, 200.0, &[0]);
    assert_eq!(view1.size(), (800.0, 400.0));
    assert_eq!(size_of(&gridview, &[1]), dims(800.0, 400.0));
    assert_eq!(view2.size(), (800.0, 200.0));
    assert_eq!(size_of(&gridview, &[0]), dims(800.0, 200.0));

    let view3 = new_view(&store, &ui, 50.0, INF, 50.0, INF);
    add(&gridview, &view3, 200.0, &[1, 1]);
    assert_eq!(view1.size(), (600.0, 400.0));
    assert_eq!(size_of(&gridview, &[1, 0]), dims(600.0, 400.0));
    assert_eq!(view2.size(), (800.0, 200.0));
    assert_eq!(size_of(&gridview, &[0]), dims(800.0, 200.0));
    assert_eq!(view3.size(), (200.0, 400.0));
    assert_eq!(size_of(&gridview, &[1, 1]), dims(200.0, 400.0));

    let view4 = new_view(&store, &ui, 50.0, INF, 50.0, INF);
    add(&gridview, &view4, 200.0, &[0, 0]);
    assert_eq!(view1.size(), (600.0, 400.0));
    assert_eq!(size_of(&gridview, &[1, 0]), dims(600.0, 400.0));
    assert_eq!(view2.size(), (600.0, 200.0));
    assert_eq!(size_of(&gridview, &[0, 1]), dims(600.0, 200.0));
    assert_eq!(view3.size(), (200.0, 400.0));
    assert_eq!(size_of(&gridview, &[1, 1]), dims(200.0, 400.0));
    assert_eq!(view4.size(), (200.0, 200.0));
    assert_eq!(size_of(&gridview, &[0, 0]), dims(200.0, 200.0));

    let view5 = new_view(&store, &ui, 50.0, INF, 50.0, INF);
    add(&gridview, &view5, 100.0, &[1, 0, 1]);
    assert_eq!(view1.size(), (600.0, 300.0));
    assert_eq!(size_of(&gridview, &[1, 0, 0]), dims(600.0, 300.0));
    assert_eq!(view2.size(), (600.0, 200.0));
    assert_eq!(size_of(&gridview, &[0, 1]), dims(600.0, 200.0));
    assert_eq!(view3.size(), (200.0, 400.0));
    assert_eq!(size_of(&gridview, &[1, 1]), dims(200.0, 400.0));
    assert_eq!(view4.size(), (200.0, 200.0));
    assert_eq!(size_of(&gridview, &[0, 0]), dims(200.0, 200.0));
    assert_eq!(view5.size(), (600.0, 100.0));
    assert_eq!(size_of(&gridview, &[1, 0, 1]), dims(600.0, 100.0));
}

#[test]
fn simple_layout_with_automatic_size_distribution() {
    let store = ensure_no_disposables_are_leaked_in_test_suite();
    let ui = Ui::new();
    let gridview = create_grid_view(&store, &ui);
    gridview.layout(800.0, 600.0, 0.0, 0.0);

    let view1 = new_view(&store, &ui, 50.0, INF, 50.0, INF);
    add(&gridview, &view1, Sizing::Distribute, &[0]);
    assert_eq!(view1.size(), (800.0, 600.0));
    assert_eq!(size_of(&gridview, &[0]), dims(800.0, 600.0));

    let view2 = new_view(&store, &ui, 50.0, INF, 50.0, INF);
    add(&gridview, &view2, Sizing::Distribute, &[0]);
    assert_eq!(view1.size(), (800.0, 300.0));
    assert_eq!(view2.size(), (800.0, 300.0));

    let view3 = new_view(&store, &ui, 50.0, INF, 50.0, INF);
    add(&gridview, &view3, Sizing::Distribute, &[1, 1]);
    assert_eq!(view1.size(), (400.0, 300.0));
    assert_eq!(view2.size(), (800.0, 300.0));
    assert_eq!(view3.size(), (400.0, 300.0));

    let view4 = new_view(&store, &ui, 50.0, INF, 50.0, INF);
    add(&gridview, &view4, Sizing::Distribute, &[0, 0]);
    assert_eq!(view1.size(), (400.0, 300.0));
    assert_eq!(view2.size(), (400.0, 300.0));
    assert_eq!(view3.size(), (400.0, 300.0));
    assert_eq!(view4.size(), (400.0, 300.0));

    let view5 = new_view(&store, &ui, 50.0, INF, 50.0, INF);
    add(&gridview, &view5, Sizing::Distribute, &[1, 0, 1]);
    assert_eq!(view1.size(), (400.0, 150.0));
    assert_eq!(view2.size(), (400.0, 300.0));
    assert_eq!(view3.size(), (400.0, 300.0));
    assert_eq!(view4.size(), (400.0, 300.0));
    assert_eq!(view5.size(), (400.0, 150.0));
}

#[test]
fn addviews_before_layout_call_1() {
    let store = ensure_no_disposables_are_leaked_in_test_suite();
    let ui = Ui::new();
    let gridview = create_grid_view(&store, &ui);

    let view1 = new_view(&store, &ui, 50.0, INF, 50.0, INF);
    add(&gridview, &view1, 200.0, &[0]);

    let view2 = new_view(&store, &ui, 50.0, INF, 50.0, INF);
    add(&gridview, &view2, 200.0, &[0]);

    let view3 = new_view(&store, &ui, 50.0, INF, 50.0, INF);
    add(&gridview, &view3, 200.0, &[1, 1]);

    gridview.layout(800.0, 600.0, 0.0, 0.0);

    assert_eq!(view1.size(), (400.0, 300.0));
    assert_eq!(view2.size(), (800.0, 300.0));
    assert_eq!(view3.size(), (400.0, 300.0));
}

#[test]
fn addviews_before_layout_call_2() {
    let store = ensure_no_disposables_are_leaked_in_test_suite();
    let ui = Ui::new();
    let gridview = create_grid_view(&store, &ui);
    let view1 = new_view(&store, &ui, 50.0, INF, 50.0, INF);
    add(&gridview, &view1, 200.0, &[0]);

    let view2 = new_view(&store, &ui, 50.0, INF, 50.0, INF);
    add(&gridview, &view2, 200.0, &[0]);

    let view3 = new_view(&store, &ui, 50.0, INF, 50.0, INF);
    add(&gridview, &view3, 200.0, &[0, 0]);

    gridview.layout(800.0, 600.0, 0.0, 0.0);

    assert_eq!(view1.size(), (800.0, 300.0));
    assert_eq!(view2.size(), (400.0, 300.0));
    assert_eq!(view3.size(), (400.0, 300.0));
}

#[test]
fn flipping_orientation_should_preserve_absolute_offsets() {
    let store = ensure_no_disposables_are_leaked_in_test_suite();
    let ui = Ui::new();
    let gridview = create_grid_view(&store, &ui);
    let view1 = new_view(&store, &ui, 50.0, INF, 50.0, INF);
    add(&gridview, &view1, 200.0, &[0]);

    let view2 = new_view(&store, &ui, 50.0, INF, 50.0, INF);
    add(&gridview, &view2, 200.0, &[1]);

    gridview.layout(800.0, 600.0, 100.0, 200.0);

    assert_eq!([view1.top(), view1.left()], [100.0, 200.0]);
    assert_eq!([view2.top(), view2.left()], [100.0 + 300.0, 200.0]);

    gridview.set_orientation(Orientation::Horizontal);

    assert_eq!([view1.top(), view1.left()], [100.0, 200.0]);
    assert_eq!([view2.top(), view2.left()], [100.0, 200.0 + 400.0]);
}

