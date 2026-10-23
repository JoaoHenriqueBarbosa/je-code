//! Porte de `src/vs/base/test/browser/ui/grid/grid.test.ts` (suítes `Grid` e `SerializableGrid`), caso a caso e na mesma ordem.
//!
//! Notas de tradução:
//! - `TestSerializableView` e `TestViewDeserializer` ficam em `support::grid_util` (`TestView::named`) e aqui
//!   (`TestViewDeserializer`); o `data` das folhas serializadas é `TestJson` (`{ name }`).
//! - `nodesToArrays` e `nodesToNames` comparam a identidade das views e os nomes (`Nested`, `Names`).
//! - O `Number.MAX_VALUE` do original é `f64::MAX`.
//! - `deepClone` é `Clone`, e o `undefined` do `size` da raiz serializada é `None`.

mod support;

use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

use je_base::lifecycle::{DisposableStore, IDisposable};
use je_ui::{NodeId, Ui};
use je_widgets::grid::{
    Direction, Grid, GridDescriptor, GridNodeDescriptor, GridOptions, Orientation, SerializableGrid, SerializedGrid, Sizing, ViewDeserializer, create_serialized_grid, get_relative_location, sanitize_grid_node_descriptor,
};
use je_widgets::gridview::{GridNode, SerializedNode, ViewDimensions};
use support::grid_util::{Nested, TestJson, TestView, list, nodes_to_arrays, rc_id, view};
use support::{create_container, ensure_no_disposables_are_leaked_in_test_suite, LeakGuard};

const MAX: f64 = f64::MAX;
const INF: f64 = f64::INFINITY;

fn mk(store: &LeakGuard, ui: &Ui) -> Rc<TestView> {
    store.add_rc(TestView::new(ui, 50.0, MAX, 50.0, MAX))
}

fn named(store: &LeakGuard, ui: &Ui, name: &str) -> Rc<TestView> {
    store.add_rc(TestView::named(ui, name, 50.0, MAX, 50.0, MAX))
}

/// `container` de 800x600 do `setup`, com o elemento do grid pendurado.
fn attach(ui: &Ui, element: NodeId) {
    let container = create_container(ui, 800.0, 600.0);
    ui.tree_mut().append_child(container, element);
}

fn new_grid(store: &LeakGuard, ui: &Ui, first: &Rc<TestView>) -> Rc<Grid<TestView>> {
    let grid = store.add_rc(Grid::new(ui, first.clone(), GridOptions::default()));
    attach(ui, grid.element());
    grid
}

fn new_serializable_grid(store: &LeakGuard, ui: &Ui, first: &Rc<TestView>) -> Rc<SerializableGrid<TestView>> {
    let grid = store.add_rc(SerializableGrid::new(ui, first.clone(), GridOptions::default()));
    attach(ui, grid.element());
    grid
}

fn layout(grid: &Grid<TestView>) {
    grid.layout(800.0, 600.0, 0.0, 0.0);
}

fn ids(views: Vec<Rc<TestView>>) -> Vec<usize> {
    views.iter().map(rc_id).collect()
}

fn ids_of(views: &[&Rc<TestView>]) -> Vec<usize> {
    views.iter().map(|v| rc_id(v)).collect()
}

fn dims(width: f64, height: f64) -> ViewDimensions {
    ViewDimensions { width, height }
}

fn arrays(grid: &Grid<TestView>) -> Nested {
    nodes_to_arrays(&GridNode::Branch(grid.get_views()), &|v: &Rc<TestView>| rc_id(v))
}

#[test]
fn get_relative_location_case() {
    use Direction::{Down, Left, Right, Up};
    use Orientation::{Horizontal, Vertical};
    let _ = ensure_no_disposables_are_leaked_in_test_suite();
    assert_eq!(get_relative_location(Vertical, &[0], Up), [0]);
    assert_eq!(get_relative_location(Vertical, &[0], Down), [1]);
    assert_eq!(get_relative_location(Vertical, &[0], Left), [0, 0]);
    assert_eq!(get_relative_location(Vertical, &[0], Right), [0, 1]);

    assert_eq!(get_relative_location(Horizontal, &[0], Up), [0, 0]);
    assert_eq!(get_relative_location(Horizontal, &[0], Down), [0, 1]);
    assert_eq!(get_relative_location(Horizontal, &[0], Left), [0]);
    assert_eq!(get_relative_location(Horizontal, &[0], Right), [1]);

    assert_eq!(get_relative_location(Vertical, &[4], Up), [4]);
    assert_eq!(get_relative_location(Vertical, &[4], Down), [5]);
    assert_eq!(get_relative_location(Vertical, &[4], Left), [4, 0]);
    assert_eq!(get_relative_location(Vertical, &[4], Right), [4, 1]);

    assert_eq!(get_relative_location(Vertical, &[0, 0], Up), [0, 0, 0]);
    assert_eq!(get_relative_location(Vertical, &[0, 0], Down), [0, 0, 1]);
    assert_eq!(get_relative_location(Vertical, &[0, 0], Left), [0, 0]);
    assert_eq!(get_relative_location(Vertical, &[0, 0], Right), [0, 1]);

    assert_eq!(get_relative_location(Vertical, &[1, 2], Up), [1, 2, 0]);
    assert_eq!(get_relative_location(Vertical, &[1, 2], Down), [1, 2, 1]);
    assert_eq!(get_relative_location(Vertical, &[1, 2], Left), [1, 2]);
    assert_eq!(get_relative_location(Vertical, &[1, 2], Right), [1, 3]);

    assert_eq!(get_relative_location(Vertical, &[1, 2, 3], Up), [1, 2, 3]);
    assert_eq!(get_relative_location(Vertical, &[1, 2, 3], Down), [1, 2, 4]);
    assert_eq!(get_relative_location(Vertical, &[1, 2, 3], Left), [1, 2, 3, 0]);
    assert_eq!(get_relative_location(Vertical, &[1, 2, 3], Right), [1, 2, 3, 1]);
}

#[test]
fn empty() {
    let store = ensure_no_disposables_are_leaked_in_test_suite();
    let ui = Ui::new();
    let view1 = store.add_rc(TestView::new(&ui, 100.0, MAX, 100.0, MAX));
    let gridview = new_grid(&store, &ui, &view1);
    layout(&gridview);

    assert_eq!(view1.size(), (800.0, 600.0));
}

#[test]
fn two_views_vertically() {
    let store = ensure_no_disposables_are_leaked_in_test_suite();
    let ui = Ui::new();
    let view1 = mk(&store, &ui);
    let grid = new_grid(&store, &ui, &view1);
    layout(&grid);
    assert_eq!(view1.size(), (800.0, 600.0));

    let view2 = mk(&store, &ui);
    grid.add_view(view2.clone(), 200.0, &view1, Direction::Up);
    assert_eq!(view1.size(), (800.0, 400.0));
    assert_eq!(view2.size(), (800.0, 200.0));
}

#[test]
fn two_views_horizontally() {
    let store = ensure_no_disposables_are_leaked_in_test_suite();
    let ui = Ui::new();
    let view1 = mk(&store, &ui);
    let grid = new_grid(&store, &ui, &view1);

    layout(&grid);
    assert_eq!(view1.size(), (800.0, 600.0));

    let view2 = mk(&store, &ui);
    grid.add_view(view2.clone(), 300.0, &view1, Direction::Right);
    assert_eq!(view1.size(), (500.0, 600.0));
    assert_eq!(view2.size(), (300.0, 600.0));
}

#[test]
fn simple_layout() {
    let store = ensure_no_disposables_are_leaked_in_test_suite();
    let ui = Ui::new();
    let view1 = mk(&store, &ui);
    let grid = new_grid(&store, &ui, &view1);

    layout(&grid);
    assert_eq!(view1.size(), (800.0, 600.0));

    let view2 = mk(&store, &ui);
    grid.add_view(view2.clone(), 200.0, &view1, Direction::Up);
    assert_eq!(view1.size(), (800.0, 400.0));
    assert_eq!(view2.size(), (800.0, 200.0));

    let view3 = mk(&store, &ui);
    grid.add_view(view3.clone(), 200.0, &view1, Direction::Right);
    assert_eq!(view1.size(), (600.0, 400.0));
    assert_eq!(view2.size(), (800.0, 200.0));
    assert_eq!(view3.size(), (200.0, 400.0));

    let view4 = mk(&store, &ui);
    grid.add_view(view4.clone(), 200.0, &view2, Direction::Left);
    assert_eq!(view1.size(), (600.0, 400.0));
    assert_eq!(view2.size(), (600.0, 200.0));
    assert_eq!(view3.size(), (200.0, 400.0));
    assert_eq!(view4.size(), (200.0, 200.0));

    let view5 = mk(&store, &ui);
    grid.add_view(view5.clone(), 100.0, &view1, Direction::Down);
    assert_eq!(view1.size(), (600.0, 300.0));
    assert_eq!(view2.size(), (600.0, 200.0));
    assert_eq!(view3.size(), (200.0, 400.0));
    assert_eq!(view4.size(), (200.0, 200.0));
    assert_eq!(view5.size(), (600.0, 100.0));
}

#[test]
fn another_simple_layout_with_automatic_size_distribution() {
    let store = ensure_no_disposables_are_leaked_in_test_suite();
    let ui = Ui::new();
    let view1 = mk(&store, &ui);
    let grid = new_grid(&store, &ui, &view1);

    layout(&grid);
    assert_eq!(view1.size(), (800.0, 600.0));

    let view2 = mk(&store, &ui);
    grid.add_view(view2.clone(), Sizing::Distribute, &view1, Direction::Left);
    assert_eq!(view1.size(), (400.0, 600.0));
    assert_eq!(view2.size(), (400.0, 600.0));

    let view3 = mk(&store, &ui);
    grid.add_view(view3.clone(), Sizing::Distribute, &view1, Direction::Right);
    assert_eq!(view1.size(), (266.0, 600.0));
    assert_eq!(view2.size(), (266.0, 600.0));
    assert_eq!(view3.size(), (268.0, 600.0));

    let view4 = mk(&store, &ui);
    grid.add_view(view4.clone(), Sizing::Distribute, &view2, Direction::Down);
    assert_eq!(view1.size(), (266.0, 600.0));
    assert_eq!(view2.size(), (266.0, 300.0));
    assert_eq!(view3.size(), (268.0, 600.0));
    assert_eq!(view4.size(), (266.0, 300.0));

    let view5 = mk(&store, &ui);
    grid.add_view(view5.clone(), Sizing::Distribute, &view3, Direction::Up);
    assert_eq!(view1.size(), (266.0, 600.0));
    assert_eq!(view2.size(), (266.0, 300.0));
    assert_eq!(view3.size(), (268.0, 300.0));
    assert_eq!(view4.size(), (266.0, 300.0));
    assert_eq!(view5.size(), (268.0, 300.0));

    let view6 = mk(&store, &ui);
    grid.add_view(view6.clone(), Sizing::Distribute, &view3, Direction::Down);
    assert_eq!(view1.size(), (266.0, 600.0));
    assert_eq!(view2.size(), (266.0, 300.0));
    assert_eq!(view3.size(), (268.0, 200.0));
    assert_eq!(view4.size(), (266.0, 300.0));
    assert_eq!(view5.size(), (268.0, 200.0));
    assert_eq!(view6.size(), (268.0, 200.0));
}

#[test]
fn another_simple_layout_with_split_size_distribution() {
    let store = ensure_no_disposables_are_leaked_in_test_suite();
    let ui = Ui::new();
    let view1 = mk(&store, &ui);
    let grid = new_grid(&store, &ui, &view1);

    layout(&grid);
    assert_eq!(view1.size(), (800.0, 600.0));

    let view2 = mk(&store, &ui);
    grid.add_view(view2.clone(), Sizing::Split, &view1, Direction::Left);
    assert_eq!(view1.size(), (400.0, 600.0));
    assert_eq!(view2.size(), (400.0, 600.0));

    let view3 = mk(&store, &ui);
    grid.add_view(view3.clone(), Sizing::Split, &view1, Direction::Right);
    assert_eq!(view1.size(), (200.0, 600.0));
    assert_eq!(view2.size(), (400.0, 600.0));
    assert_eq!(view3.size(), (200.0, 600.0));

    let view4 = mk(&store, &ui);
    grid.add_view(view4.clone(), Sizing::Split, &view2, Direction::Down);
    assert_eq!(view1.size(), (200.0, 600.0));
    assert_eq!(view2.size(), (400.0, 300.0));
    assert_eq!(view3.size(), (200.0, 600.0));
    assert_eq!(view4.size(), (400.0, 300.0));

    let view5 = mk(&store, &ui);
    grid.add_view(view5.clone(), Sizing::Split, &view3, Direction::Up);
    assert_eq!(view1.size(), (200.0, 600.0));
    assert_eq!(view2.size(), (400.0, 300.0));
    assert_eq!(view3.size(), (200.0, 300.0));
    assert_eq!(view4.size(), (400.0, 300.0));
    assert_eq!(view5.size(), (200.0, 300.0));

    let view6 = mk(&store, &ui);
    grid.add_view(view6.clone(), Sizing::Split, &view3, Direction::Down);
    assert_eq!(view1.size(), (200.0, 600.0));
    assert_eq!(view2.size(), (400.0, 300.0));
    assert_eq!(view3.size(), (200.0, 150.0));
    assert_eq!(view4.size(), (400.0, 300.0));
    assert_eq!(view5.size(), (200.0, 300.0));
    assert_eq!(view6.size(), (200.0, 150.0));
}

#[test]
fn three_two_layout_with_split() {
    let store = ensure_no_disposables_are_leaked_in_test_suite();
    let ui = Ui::new();
    let view1 = mk(&store, &ui);
    let grid = new_grid(&store, &ui, &view1);

    layout(&grid);
    assert_eq!(view1.size(), (800.0, 600.0));

    let view2 = mk(&store, &ui);
    grid.add_view(view2.clone(), Sizing::Split, &view1, Direction::Down);
    assert_eq!(view1.size(), (800.0, 300.0));
    assert_eq!(view2.size(), (800.0, 300.0));

    let view3 = mk(&store, &ui);
    grid.add_view(view3.clone(), Sizing::Split, &view2, Direction::Right);
    assert_eq!(view1.size(), (800.0, 300.0));
    assert_eq!(view2.size(), (400.0, 300.0));
    assert_eq!(view3.size(), (400.0, 300.0));

    let view4 = mk(&store, &ui);
    grid.add_view(view4.clone(), Sizing::Split, &view1, Direction::Right);
    assert_eq!(view1.size(), (400.0, 300.0));
    assert_eq!(view2.size(), (400.0, 300.0));
    assert_eq!(view3.size(), (400.0, 300.0));
    assert_eq!(view4.size(), (400.0, 300.0));

    let view5 = mk(&store, &ui);
    grid.add_view(view5.clone(), Sizing::Split, &view1, Direction::Right);
    assert_eq!(view1.size(), (200.0, 300.0));
    assert_eq!(view2.size(), (400.0, 300.0));
    assert_eq!(view3.size(), (400.0, 300.0));
    assert_eq!(view4.size(), (400.0, 300.0));
    assert_eq!(view5.size(), (200.0, 300.0));
}

#[test]
fn sizing_should_be_correct_after_branch_demotion_50564() {
    let store = ensure_no_disposables_are_leaked_in_test_suite();
    let ui = Ui::new();
    let view1 = mk(&store, &ui);
    let grid = new_grid(&store, &ui, &view1);

    layout(&grid);

    let view2 = mk(&store, &ui);
    grid.add_view(view2.clone(), Sizing::Split, &view1, Direction::Right);

    let view3 = mk(&store, &ui);
    grid.add_view(view3.clone(), Sizing::Split, &view2, Direction::Down);

    let view4 = mk(&store, &ui);
    grid.add_view(view4.clone(), Sizing::Split, &view2, Direction::Right);
    assert_eq!(view1.size(), (400.0, 600.0));
    assert_eq!(view2.size(), (200.0, 300.0));
    assert_eq!(view3.size(), (400.0, 300.0));
    assert_eq!(view4.size(), (200.0, 300.0));

    grid.remove_view(&view3, None);
    assert_eq!(view1.size(), (400.0, 600.0));
    assert_eq!(view2.size(), (200.0, 600.0));
    assert_eq!(view4.size(), (200.0, 600.0));
}

#[test]
fn branch_demotion_preserves_hidden_child_visibility() {
    let store = ensure_no_disposables_are_leaked_in_test_suite();
    let ui = Ui::new();
    let sessions_view = mk(&store, &ui);
    let grid = new_grid(&store, &ui, &sessions_view);
    layout(&grid);

    let panel_view = mk(&store, &ui);
    grid.add_view(panel_view.clone(), 200.0, &sessions_view, Direction::Down);

    let editor_view = mk(&store, &ui);
    grid.add_view(editor_view.clone(), 300.0, &sessions_view, Direction::Right);

    let custom_view = mk(&store, &ui);
    grid.add_view(custom_view.clone(), 200.0, &editor_view, Direction::Right);
    grid.set_view_visible(&custom_view, false, None);

    grid.move_view(&panel_view, 200.0, &sessions_view, Direction::Down);

    assert_eq!(
        (
            grid.is_view_visible(&sessions_view),
            grid.is_view_visible(&editor_view),
            grid.is_view_visible(&custom_view),
            grid.is_view_visible(&panel_view),
            grid.get_view_cached_visible_size(&custom_view),
        ),
        (true, true, false, true, Some(200.0))
    );
}

#[test]
fn sizing_should_be_correct_after_branch_demotion_50675() {
    let store = ensure_no_disposables_are_leaked_in_test_suite();
    let ui = Ui::new();
    let view1 = mk(&store, &ui);
    let grid = new_grid(&store, &ui, &view1);

    layout(&grid);

    let view2 = mk(&store, &ui);
    grid.add_view(view2.clone(), Sizing::Distribute, &view1, Direction::Down);

    let view3 = mk(&store, &ui);
    grid.add_view(view3.clone(), Sizing::Distribute, &view2, Direction::Down);

    let view4 = mk(&store, &ui);
    grid.add_view(view4.clone(), Sizing::Distribute, &view3, Direction::Right);
    assert_eq!(view1.size(), (800.0, 200.0));
    assert_eq!(view2.size(), (800.0, 200.0));
    assert_eq!(view3.size(), (400.0, 200.0));
    assert_eq!(view4.size(), (400.0, 200.0));

    grid.remove_view(&view3, Some(Sizing::Distribute));
    assert_eq!(view1.size(), (800.0, 200.0));
    assert_eq!(view2.size(), (800.0, 200.0));
    assert_eq!(view4.size(), (800.0, 200.0));
}

#[test]
fn get_neighbor_views_should_work_on_single_view_layout() {
    use Direction::{Down, Left, Right, Up};
    let store = ensure_no_disposables_are_leaked_in_test_suite();
    let ui = Ui::new();
    let view1 = mk(&store, &ui);
    let grid = new_grid(&store, &ui, &view1);

    layout(&grid);

    assert_eq!(ids(grid.get_neighbor_views(&view1, Up, false)), Vec::<usize>::new());
    assert_eq!(ids(grid.get_neighbor_views(&view1, Right, false)), Vec::<usize>::new());
    assert_eq!(ids(grid.get_neighbor_views(&view1, Down, false)), Vec::<usize>::new());
    assert_eq!(ids(grid.get_neighbor_views(&view1, Left, false)), Vec::<usize>::new());

    assert_eq!(ids(grid.get_neighbor_views(&view1, Up, true)), ids_of(&[&view1]));
    assert_eq!(ids(grid.get_neighbor_views(&view1, Right, true)), ids_of(&[&view1]));
    assert_eq!(ids(grid.get_neighbor_views(&view1, Down, true)), ids_of(&[&view1]));
    assert_eq!(ids(grid.get_neighbor_views(&view1, Left, true)), ids_of(&[&view1]));
}

#[test]
fn get_neighbor_views_should_work_on_simple_layout() {
    use Direction::{Down, Left, Right, Up};
    let store = ensure_no_disposables_are_leaked_in_test_suite();
    let ui = Ui::new();
    let view1 = mk(&store, &ui);
    let grid = new_grid(&store, &ui, &view1);

    layout(&grid);

    let view2 = mk(&store, &ui);
    grid.add_view(view2.clone(), Sizing::Distribute, &view1, Direction::Down);

    let view3 = mk(&store, &ui);
    grid.add_view(view3.clone(), Sizing::Distribute, &view2, Direction::Down);

    let n = |view: &Rc<TestView>, direction, wrap| ids(grid.get_neighbor_views(view, direction, wrap));
    let none = Vec::<usize>::new();

    assert_eq!(n(&view1, Up, false), none);
    assert_eq!(n(&view1, Right, false), none);
    assert_eq!(n(&view1, Down, false), ids_of(&[&view2]));
    assert_eq!(n(&view1, Left, false), none);

    assert_eq!(n(&view1, Up, true), ids_of(&[&view3]));
    assert_eq!(n(&view1, Right, true), ids_of(&[&view1]));
    assert_eq!(n(&view1, Down, true), ids_of(&[&view2]));
    assert_eq!(n(&view1, Left, true), ids_of(&[&view1]));

    assert_eq!(n(&view2, Up, false), ids_of(&[&view1]));
    assert_eq!(n(&view2, Right, false), none);
    assert_eq!(n(&view2, Down, false), ids_of(&[&view3]));
    assert_eq!(n(&view2, Left, false), none);

    assert_eq!(n(&view2, Up, true), ids_of(&[&view1]));
    assert_eq!(n(&view2, Right, true), ids_of(&[&view2]));
    assert_eq!(n(&view2, Down, true), ids_of(&[&view3]));
    assert_eq!(n(&view2, Left, true), ids_of(&[&view2]));

    assert_eq!(n(&view3, Up, false), ids_of(&[&view2]));
    assert_eq!(n(&view3, Right, false), none);
    assert_eq!(n(&view3, Down, false), none);
    assert_eq!(n(&view3, Left, false), none);

    assert_eq!(n(&view3, Up, true), ids_of(&[&view2]));
    assert_eq!(n(&view3, Right, true), ids_of(&[&view3]));
    assert_eq!(n(&view3, Down, true), ids_of(&[&view1]));
    assert_eq!(n(&view3, Left, true), ids_of(&[&view3]));
}

#[test]
fn get_neighbor_views_should_work_on_a_complex_layout() {
    use Direction::{Down, Left, Right, Up};
    let store = ensure_no_disposables_are_leaked_in_test_suite();
    let ui = Ui::new();
    let view1 = mk(&store, &ui);
    let grid = new_grid(&store, &ui, &view1);

    layout(&grid);

    let view2 = mk(&store, &ui);
    grid.add_view(view2.clone(), Sizing::Distribute, &view1, Direction::Down);

    let view3 = mk(&store, &ui);
    grid.add_view(view3.clone(), Sizing::Distribute, &view2, Direction::Down);

    let view4 = mk(&store, &ui);
    grid.add_view(view4.clone(), Sizing::Distribute, &view2, Direction::Right);

    let view5 = mk(&store, &ui);
    grid.add_view(view5.clone(), Sizing::Distribute, &view4, Direction::Down);

    let n = |view: &Rc<TestView>, direction| ids(grid.get_neighbor_views(view, direction, false));
    let none = Vec::<usize>::new();

    assert_eq!(n(&view1, Up), none);
    assert_eq!(n(&view1, Right), none);
    assert_eq!(n(&view1, Down), ids_of(&[&view2, &view4]));
    assert_eq!(n(&view1, Left), none);
    assert_eq!(n(&view2, Up), ids_of(&[&view1]));
    assert_eq!(n(&view2, Right), ids_of(&[&view4, &view5]));
    assert_eq!(n(&view2, Down), ids_of(&[&view3]));
    assert_eq!(n(&view2, Left), none);
    assert_eq!(n(&view4, Up), ids_of(&[&view1]));
    assert_eq!(n(&view4, Right), none);
    assert_eq!(n(&view4, Down), ids_of(&[&view5]));
    assert_eq!(n(&view4, Left), ids_of(&[&view2]));
    assert_eq!(n(&view5, Up), ids_of(&[&view4]));
    assert_eq!(n(&view5, Right), none);
    assert_eq!(n(&view5, Down), ids_of(&[&view3]));
    assert_eq!(n(&view5, Left), ids_of(&[&view2]));
    assert_eq!(n(&view3, Up), ids_of(&[&view2, &view5]));
    assert_eq!(n(&view3, Right), none);
    assert_eq!(n(&view3, Down), none);
    assert_eq!(n(&view3, Left), none);
}

#[test]
fn get_neighbor_views_should_work_on_another_simple_layout() {
    use Direction::{Down, Left, Right, Up};
    let store = ensure_no_disposables_are_leaked_in_test_suite();
    let ui = Ui::new();
    let view1 = mk(&store, &ui);
    let grid = new_grid(&store, &ui, &view1);

    layout(&grid);

    let view2 = mk(&store, &ui);
    grid.add_view(view2.clone(), Sizing::Distribute, &view1, Direction::Right);

    let view3 = mk(&store, &ui);
    grid.add_view(view3.clone(), Sizing::Distribute, &view2, Direction::Down);

    let view4 = mk(&store, &ui);
    grid.add_view(view4.clone(), Sizing::Distribute, &view2, Direction::Right);

    let n = |view: &Rc<TestView>, direction| ids(grid.get_neighbor_views(view, direction, false));

    assert_eq!(n(&view4, Up), Vec::<usize>::new());
    assert_eq!(n(&view4, Right), Vec::<usize>::new());
    assert_eq!(n(&view4, Down), ids_of(&[&view3]));
    assert_eq!(n(&view4, Left), ids_of(&[&view2]));
}

#[test]
fn get_neighbor_views_should_only_return_immediate_neighbors() {
    let store = ensure_no_disposables_are_leaked_in_test_suite();
    let ui = Ui::new();
    let view1 = mk(&store, &ui);
    let grid = new_grid(&store, &ui, &view1);

    layout(&grid);

    let view2 = mk(&store, &ui);
    grid.add_view(view2.clone(), Sizing::Distribute, &view1, Direction::Right);

    let view3 = mk(&store, &ui);
    grid.add_view(view3.clone(), Sizing::Distribute, &view2, Direction::Down);

    let view4 = mk(&store, &ui);
    grid.add_view(view4.clone(), Sizing::Distribute, &view2, Direction::Right);

    assert_eq!(ids(grid.get_neighbor_views(&view1, Direction::Right, false)), ids_of(&[&view2, &view3]));
}

#[test]
fn hiding_splitviews_and_restoring_sizes() {
    let store = ensure_no_disposables_are_leaked_in_test_suite();
    let ui = Ui::new();
    let view1 = mk(&store, &ui);
    let grid = new_grid(&store, &ui, &view1);

    layout(&grid);

    let view2 = mk(&store, &ui);
    grid.add_view(view2.clone(), Sizing::Distribute, &view1, Direction::Right);

    let view3 = mk(&store, &ui);
    grid.add_view(view3.clone(), Sizing::Distribute, &view2, Direction::Down);

    let view4 = mk(&store, &ui);
    grid.add_view(view4.clone(), Sizing::Distribute, &view2, Direction::Right);

    let (size1, size2, size3, size4) = (view1.size(), view2.size(), view3.size(), view4.size());

    grid.maximize_view(&view1, &[]);

    // Views 2, 3, 4 are hidden
    // Splitview (2,4) and ((2,4),3) are hidden
    assert_eq!(view1.size(), (800.0, 600.0));
    assert_eq!(view2.size(), (0.0, 0.0));
    assert_eq!(view3.size(), (0.0, 0.0));
    assert_eq!(view4.size(), (0.0, 0.0));

    grid.exit_maximized_view();

    assert_eq!(view1.size(), size1);
    assert_eq!(view2.size(), size2);
    assert_eq!(view3.size(), size3);
    assert_eq!(view4.size(), size4);

    // Views 1, 3, 4 are hidden
    // All splitviews are still visible => only orthogonalsize is 0
    grid.maximize_view(&view2, &[]);

    assert_eq!(view1.size(), (0.0, 600.0));
    assert_eq!(view2.size(), (800.0, 600.0));
    assert_eq!(view3.size(), (800.0, 0.0));
    assert_eq!(view4.size(), (0.0, 600.0));

    grid.exit_maximized_view();

    assert_eq!(view1.size(), size1);
    assert_eq!(view2.size(), size2);
    assert_eq!(view3.size(), size3);
    assert_eq!(view4.size(), size4);
}

#[test]
fn maximization_preserves_hidden_view_visibility() {
    let store = ensure_no_disposables_are_leaked_in_test_suite();
    let ui = Ui::new();
    let view1 = mk(&store, &ui);
    let grid = new_grid(&store, &ui, &view1);

    layout(&grid);

    let view2 = mk(&store, &ui);
    grid.add_view(view2.clone(), Sizing::Distribute, &view1, Direction::Right);

    let view3 = mk(&store, &ui);
    grid.add_view(view3.clone(), Sizing::Distribute, &view2, Direction::Right);

    let view4 = mk(&store, &ui);
    grid.add_view(view4.clone(), Sizing::Distribute, &view3, Direction::Right);

    grid.set_view_visible(&view3, false, None);
    grid.set_view_visible(&view4, false, None);
    let cached_hidden_sizes = (grid.get_view_cached_visible_size(&view3), grid.get_view_cached_visible_size(&view4));

    grid.maximize_view(&view1, std::slice::from_ref(&view4));
    grid.exit_maximized_view();

    assert_eq!([&view1, &view2, &view3, &view4].map(|v| grid.is_view_visible(v)), [true, true, false, false]);
    assert_eq!((grid.get_view_cached_visible_size(&view3), grid.get_view_cached_visible_size(&view4)), cached_hidden_sizes);
}

#[test]
fn changing_orientation_restores_maximized_visibility_before_replacing_nodes() {
    let store = ensure_no_disposables_are_leaked_in_test_suite();
    let ui = Ui::new();
    let view1 = mk(&store, &ui);
    let grid = new_grid(&store, &ui, &view1);
    layout(&grid);

    let view2 = mk(&store, &ui);
    grid.add_view(view2.clone(), Sizing::Distribute, &view1, Direction::Right);
    let view3 = mk(&store, &ui);
    grid.add_view(view3.clone(), Sizing::Distribute, &view2, Direction::Down);

    grid.maximize_view(&view1, &[]);
    grid.set_orientation(if grid.orientation() == Orientation::Vertical { Orientation::Horizontal } else { Orientation::Vertical });
    grid.exit_maximized_view();

    assert!(!grid.has_maximized_view());
    assert_eq!([&view1, &view2, &view3].map(|v| grid.is_view_visible(v)), [true, true, true]);
}

#[test]
fn maximization_preserves_hidden_branch_visibility() {
    let store = ensure_no_disposables_are_leaked_in_test_suite();
    let ui = Ui::new();
    let deserializer = TestViewDeserializer::new(&store, &ui);
    let grid = store.add_rc(SerializableGrid::deserialize(
        &ui,
        &SerializedGrid {
            orientation: Orientation::Vertical,
            width: 800.0,
            height: 600.0,
            root: branch(vec![leaf("view1", 400.0), SerializedNode::Branch { data: vec![leaf("view2", 300.0), leaf("view3", 300.0)], size: Some(400.0), visible: Some(false) }], 800.0),
        },
        &deserializer,
        GridOptions::default(),
    ));
    grid.layout(800.0, 600.0, 0.0, 0.0);

    let view1 = deserializer.get_view("view1");
    let initial_root = grid.serialize().root;
    grid.maximize_view(&view1, &[]);
    grid.exit_maximized_view();

    assert_eq!(grid.serialize().root, initial_root);
}

#[test]
fn has_maximized_view() {
    let store = ensure_no_disposables_are_leaked_in_test_suite();
    let ui = Ui::new();
    let view1 = mk(&store, &ui);
    let grid = new_grid(&store, &ui, &view1);

    layout(&grid);

    let view2 = mk(&store, &ui);
    grid.add_view(view2.clone(), Sizing::Distribute, &view1, Direction::Right);

    let view3 = mk(&store, &ui);
    grid.add_view(view3.clone(), Sizing::Distribute, &view2, Direction::Down);

    let view4 = mk(&store, &ui);
    grid.add_view(view4.clone(), Sizing::Distribute, &view2, Direction::Right);

    let check_is_maximized = |v: &Rc<TestView>| {
        grid.maximize_view(v, &[]);

        assert!(grid.has_maximized_view());

        // When a view is maximized, no view can be expanded even if it is maximized
        assert!(!grid.is_view_expanded(&view1));
        assert!(!grid.is_view_expanded(&view2));
        assert!(!grid.is_view_expanded(&view3));
        assert!(!grid.is_view_expanded(&view4));

        grid.exit_maximized_view();

        assert!(!grid.has_maximized_view());
    };

    check_is_maximized(&view1);
    check_is_maximized(&view2);
    check_is_maximized(&view3);
    check_is_maximized(&view4);
}

#[test]
fn changes_to_the_grid_unmaximize_the_view() {
    let store = ensure_no_disposables_are_leaked_in_test_suite();
    let ui = Ui::new();
    let view1 = mk(&store, &ui);
    let grid = new_grid(&store, &ui, &view1);

    layout(&grid);

    let view2 = mk(&store, &ui);
    grid.add_view(view2.clone(), Sizing::Distribute, &view1, Direction::Right);

    let view3 = mk(&store, &ui);
    grid.add_view(view3.clone(), Sizing::Distribute, &view2, Direction::Down);

    let view4 = mk(&store, &ui);

    // Adding a view unmaximizes the view
    grid.maximize_view(&view1, &[]);
    assert!(grid.has_maximized_view());
    grid.add_view(view4.clone(), Sizing::Distribute, &view2, Direction::Right);

    assert!(!grid.has_maximized_view());
    assert!(grid.is_view_visible(&view1));
    assert!(grid.is_view_visible(&view2));
    assert!(grid.is_view_visible(&view3));
    assert!(grid.is_view_visible(&view4));

    // Removing a view unmaximizes the view
    grid.maximize_view(&view1, &[]);
    assert!(grid.has_maximized_view());
    grid.remove_view(&view4, None);

    assert!(!grid.has_maximized_view());
    assert!(grid.is_view_visible(&view1));
    assert!(grid.is_view_visible(&view2));
    assert!(grid.is_view_visible(&view3));

    // Revealing a hidden view while a view is maximized both restores the grid
    // and applies the requested visibility.
    grid.set_view_visible(&view3, false, None);
    grid.maximize_view(&view1, &[]);
    assert!(grid.has_maximized_view());
    grid.set_view_visible(&view3, true, None);

    assert!(!grid.has_maximized_view());
    assert!(grid.is_view_visible(&view1));
    assert!(grid.is_view_visible(&view2));
    assert!(grid.is_view_visible(&view3));
}

#[test]
fn changes_to_the_grid_sizing_unmaximize_the_view() {
    let store = ensure_no_disposables_are_leaked_in_test_suite();
    let ui = Ui::new();
    let view1 = mk(&store, &ui);
    let grid = new_grid(&store, &ui, &view1);

    layout(&grid);

    let view2 = mk(&store, &ui);
    grid.add_view(view2.clone(), Sizing::Distribute, &view1, Direction::Right);

    let view3 = mk(&store, &ui);
    grid.add_view(view3.clone(), Sizing::Distribute, &view2, Direction::Down);

    let view4 = mk(&store, &ui);
    grid.add_view(view4.clone(), Sizing::Distribute, &view2, Direction::Right);

    // Maximizing a different view unmaximizes the current one and maximizes the new one
    grid.maximize_view(&view1, &[]);
    assert!(grid.has_maximized_view());
    grid.maximize_view(&view2, &[]);

    assert!(grid.has_maximized_view());
    assert!(!grid.is_view_visible(&view1));
    assert!(grid.is_view_visible(&view2));
    assert!(!grid.is_view_visible(&view3));
    assert!(!grid.is_view_visible(&view4));

    // Distributing the size unmaximizes the view
    grid.maximize_view(&view1, &[]);
    assert!(grid.has_maximized_view());
    grid.distribute_view_sizes();

    assert!(!grid.has_maximized_view());
    assert!(grid.is_view_visible(&view1));
    assert!(grid.is_view_visible(&view2));
    assert!(grid.is_view_visible(&view3));
    assert!(grid.is_view_visible(&view4));

    // Expanding a different view unmaximizes the view
    grid.maximize_view(&view1, &[]);
    assert!(grid.has_maximized_view());
    grid.expand_view(&view2);

    assert!(!grid.has_maximized_view());
    assert!(grid.is_view_visible(&view1));
    assert!(grid.is_view_visible(&view2));
    assert!(grid.is_view_visible(&view3));
    assert!(grid.is_view_visible(&view4));

    // Expanding the maximized view unmaximizes the view
    grid.maximize_view(&view1, &[]);
    assert!(grid.has_maximized_view());
    grid.expand_view(&view1);

    assert!(!grid.has_maximized_view());
    assert!(grid.is_view_visible(&view1));
    assert!(grid.is_view_visible(&view2));
    assert!(grid.is_view_visible(&view3));
    assert!(grid.is_view_visible(&view4));
}

#[test]
fn distribute_sibling_sizes_when_revealing_a_view() {
    let store = ensure_no_disposables_are_leaked_in_test_suite();
    let ui = Ui::new();
    let view1 = mk(&store, &ui);
    let grid = new_grid(&store, &ui, &view1);
    layout(&grid);

    let view2 = mk(&store, &ui);
    grid.add_view(view2.clone(), Sizing::Distribute, &view1, Direction::Right);
    let view3 = mk(&store, &ui);
    grid.add_view(view3.clone(), Sizing::Distribute, &view2, Direction::Down);

    grid.set_view_visible(&view1, false, None);
    grid.resize_view(&view2, dims(200.0, 450.0));
    grid.set_view_visible(&view1, true, Some(Sizing::Distribute));

    assert_eq!((grid.get_view_size(Some(&view1)), grid.get_view_size(Some(&view2)), grid.get_view_size(Some(&view3))), (dims(400.0, 600.0), dims(400.0, 450.0), dims(400.0, 150.0)));
}

// --- SerializableGrid ---

fn leaf(name: &str, size: f64) -> SerializedNode<TestJson> {
    SerializedNode::Leaf { data: TestJson { name: name.to_owned() }, size: Some(size), visible: None, maximized: None }
}

fn leaf_hidden(name: &str, size: f64) -> SerializedNode<TestJson> {
    SerializedNode::Leaf { data: TestJson { name: name.to_owned() }, size: Some(size), visible: Some(false), maximized: None }
}

fn branch(data: Vec<SerializedNode<TestJson>>, size: f64) -> SerializedNode<TestJson> {
    SerializedNode::Branch { data, size: Some(size), visible: None }
}

fn serialized(root: SerializedNode<TestJson>) -> SerializedGrid<TestJson> {
    SerializedGrid { orientation: Orientation::Vertical, width: 800.0, height: 600.0, root }
}

struct TestViewDeserializer {
    store: Rc<DisposableStore>,
    ui: Ui,
    views: RefCell<HashMap<String, Rc<TestView>>>,
}

impl TestViewDeserializer {
    fn new(store: &LeakGuard, ui: &Ui) -> Self {
        Self { store: store.store(), ui: ui.clone(), views: RefCell::new(HashMap::new()) }
    }

    fn get_view(&self, id: &str) -> Rc<TestView> {
        self.views.borrow().get(id).cloned().unwrap_or_else(|| panic!("Unknown view"))
    }
}

impl ViewDeserializer<TestView, TestJson> for TestViewDeserializer {
    fn from_json(&self, json: &TestJson) -> Rc<TestView> {
        let view = self.store.add_rc(TestView::named(&self.ui, &json.name, 50.0, MAX, 50.0, MAX));
        self.views.borrow_mut().insert(json.name.clone(), view.clone());
        view
    }
}

/// `nodesToNames`.
#[derive(Debug, PartialEq)]
enum Names {
    Name(String),
    List(Vec<Names>),
}

fn nodes_to_names(node: &GridNode<Rc<TestView>>) -> Names {
    match node {
        GridNode::Branch(branch) => Names::List(branch.children.iter().map(nodes_to_names).collect()),
        GridNode::Leaf(leaf) => Names::Name(leaf.view.name.clone()),
    }
}

fn names(items: &[&str]) -> Names {
    Names::List(items.iter().map(|n| Names::Name((*n).to_owned())).collect())
}

/// O grid de cinco views de `serialize simple layout` e companhia.
fn five_view_grid(store: &LeakGuard, ui: &Ui) -> Rc<SerializableGrid<TestView>> {
    let view1 = named(store, ui, "view1");
    let grid = new_serializable_grid(store, ui, &view1);
    grid.layout(800.0, 600.0, 0.0, 0.0);

    let view2 = named(store, ui, "view2");
    grid.add_view(view2.clone(), 200.0, &view1, Direction::Up);

    let view3 = named(store, ui, "view3");
    grid.add_view(view3.clone(), 200.0, &view1, Direction::Right);

    let view4 = named(store, ui, "view4");
    grid.add_view(view4.clone(), 200.0, &view2, Direction::Left);

    let view5 = named(store, ui, "view5");
    grid.add_view(view5.clone(), 100.0, &view1, Direction::Down);
    grid
}

fn five_view_json() -> SerializedGrid<TestJson> {
    serialized(branch(
        vec![
            branch(vec![leaf("view4", 200.0), leaf("view2", 600.0)], 200.0),
            branch(vec![branch(vec![leaf("view1", 300.0), leaf("view5", 100.0)], 600.0), leaf("view3", 200.0)], 400.0),
        ],
        800.0,
    ))
}

#[test]
fn serialize_empty() {
    let store = ensure_no_disposables_are_leaked_in_test_suite();
    let ui = Ui::new();
    let view1 = named(&store, &ui, "view1");
    let grid = new_serializable_grid(&store, &ui, &view1);
    grid.layout(800.0, 600.0, 0.0, 0.0);

    let actual = grid.serialize();
    assert_eq!(actual, serialized(branch(vec![leaf("view1", 600.0)], 800.0)));
}

#[test]
fn serialize_simple_layout() {
    let store = ensure_no_disposables_are_leaked_in_test_suite();
    let ui = Ui::new();
    let grid = five_view_grid(&store, &ui);

    assert_eq!(grid.serialize(), five_view_json());
}

#[test]
fn deserialize_empty() {
    let store = ensure_no_disposables_are_leaked_in_test_suite();
    let ui = Ui::new();
    let view1 = named(&store, &ui, "view1");
    let grid = new_serializable_grid(&store, &ui, &view1);
    grid.layout(800.0, 600.0, 0.0, 0.0);

    let json = grid.serialize();
    grid.dispose();

    let deserializer = TestViewDeserializer::new(&store, &ui);
    let grid2 = store.add_rc(SerializableGrid::deserialize(&ui, &json, &deserializer, GridOptions::default()));
    grid2.layout(800.0, 600.0, 0.0, 0.0);

    assert_eq!(nodes_to_names(&GridNode::Branch(grid2.get_views())), names(&["view1"]));
}

#[test]
fn deserialize_simple_layout() {
    let store = ensure_no_disposables_are_leaked_in_test_suite();
    let ui = Ui::new();
    let grid = five_view_grid(&store, &ui);

    let json = grid.serialize();
    grid.dispose();

    let deserializer = TestViewDeserializer::new(&store, &ui);
    let grid2 = store.add_rc(SerializableGrid::deserialize(&ui, &json, &deserializer, GridOptions::default()));

    let view1_copy = deserializer.get_view("view1");
    let view2_copy = deserializer.get_view("view2");
    let view3_copy = deserializer.get_view("view3");
    let view4_copy = deserializer.get_view("view4");
    let view5_copy = deserializer.get_view("view5");

    assert_eq!(
        arrays(&grid2),
        list(vec![list(vec![view(&view4_copy), view(&view2_copy)]), list(vec![list(vec![view(&view1_copy), view(&view5_copy)]), view(&view3_copy)])])
    );

    grid2.layout(800.0, 600.0, 0.0, 0.0);

    assert_eq!(view1_copy.size(), (600.0, 300.0));
    assert_eq!(view2_copy.size(), (600.0, 200.0));
    assert_eq!(view3_copy.size(), (200.0, 400.0));
    assert_eq!(view4_copy.size(), (200.0, 200.0));
    assert_eq!(view5_copy.size(), (600.0, 100.0));
}

#[test]
fn deserialize_simple_layout_with_scaling() {
    let store = ensure_no_disposables_are_leaked_in_test_suite();
    let ui = Ui::new();
    let grid = five_view_grid(&store, &ui);

    let json = grid.serialize();
    grid.dispose();

    let deserializer = TestViewDeserializer::new(&store, &ui);
    let grid2 = store.add_rc(SerializableGrid::deserialize(&ui, &json, &deserializer, GridOptions::default()));

    let view1_copy = deserializer.get_view("view1");
    let view2_copy = deserializer.get_view("view2");
    let view3_copy = deserializer.get_view("view3");
    let view4_copy = deserializer.get_view("view4");
    let view5_copy = deserializer.get_view("view5");

    grid2.layout(400.0, 800.0, 0.0, 0.0); // [/2, *4/3]
    assert_eq!(view1_copy.size(), (300.0, 400.0));
    assert_eq!(view2_copy.size(), (300.0, 267.0));
    assert_eq!(view3_copy.size(), (100.0, 533.0));
    assert_eq!(view4_copy.size(), (100.0, 267.0));
    assert_eq!(view5_copy.size(), (300.0, 133.0));
}

#[test]
fn deserialize_4_view_layout_ben_issue_2() {
    let store = ensure_no_disposables_are_leaked_in_test_suite();
    let ui = Ui::new();
    let view1 = named(&store, &ui, "view1");
    let grid = new_serializable_grid(&store, &ui, &view1);
    grid.layout(800.0, 600.0, 0.0, 0.0);

    let view2 = named(&store, &ui, "view2");
    grid.add_view(view2.clone(), Sizing::Split, &view1, Direction::Down);

    let view3 = named(&store, &ui, "view3");
    grid.add_view(view3.clone(), Sizing::Split, &view2, Direction::Down);

    let view4 = named(&store, &ui, "view4");
    grid.add_view(view4.clone(), Sizing::Split, &view3, Direction::Right);

    let json = grid.serialize();
    grid.dispose();

    let deserializer = TestViewDeserializer::new(&store, &ui);
    let grid2 = store.add_rc(SerializableGrid::deserialize(&ui, &json, &deserializer, GridOptions::default()));

    let view1_copy = deserializer.get_view("view1");
    let view2_copy = deserializer.get_view("view2");
    let view3_copy = deserializer.get_view("view3");
    let view4_copy = deserializer.get_view("view4");

    grid2.layout(800.0, 600.0, 0.0, 0.0);

    assert_eq!(view1_copy.size(), (800.0, 300.0));
    assert_eq!(view2_copy.size(), (800.0, 150.0));
    assert_eq!(view3_copy.size(), (400.0, 150.0));
    assert_eq!(view4_copy.size(), (400.0, 150.0));
}

#[test]
fn deserialize_2_view_layout_ben_issue_3() {
    let store = ensure_no_disposables_are_leaked_in_test_suite();
    let ui = Ui::new();
    let view1 = named(&store, &ui, "view1");
    let grid = new_serializable_grid(&store, &ui, &view1);

    grid.layout(800.0, 600.0, 0.0, 0.0);

    let view2 = named(&store, &ui, "view2");
    grid.add_view(view2.clone(), Sizing::Split, &view1, Direction::Right);

    let json = grid.serialize();
    grid.dispose();

    let deserializer = TestViewDeserializer::new(&store, &ui);
    let grid2 = store.add_rc(SerializableGrid::deserialize(&ui, &json, &deserializer, GridOptions::default()));

    let view1_copy = deserializer.get_view("view1");
    let view2_copy = deserializer.get_view("view2");

    grid2.layout(800.0, 600.0, 0.0, 0.0);

    assert_eq!(view1_copy.size(), (400.0, 600.0));
    assert_eq!(view2_copy.size(), (400.0, 600.0));
}

#[test]
fn deserialize_simple_view_layout_50609() {
    let store = ensure_no_disposables_are_leaked_in_test_suite();
    let ui = Ui::new();
    let view1 = named(&store, &ui, "view1");
    let grid = new_serializable_grid(&store, &ui, &view1);

    grid.layout(800.0, 600.0, 0.0, 0.0);

    let view2 = named(&store, &ui, "view2");
    grid.add_view(view2.clone(), Sizing::Split, &view1, Direction::Right);

    let view3 = named(&store, &ui, "view3");
    grid.add_view(view3.clone(), Sizing::Split, &view2, Direction::Down);

    grid.remove_view(&view1, Some(Sizing::Split));

    let json = grid.serialize();
    grid.dispose();

    let deserializer = TestViewDeserializer::new(&store, &ui);
    let grid2 = store.add_rc(SerializableGrid::deserialize(&ui, &json, &deserializer, GridOptions::default()));

    let view2_copy = deserializer.get_view("view2");
    let view3_copy = deserializer.get_view("view3");

    grid2.layout(800.0, 600.0, 0.0, 0.0);

    assert_eq!(view2_copy.size(), (800.0, 300.0));
    assert_eq!(view3_copy.size(), (800.0, 300.0));
}

#[test]
fn sanitize_grid_node_descriptor_case() {
    let _ = ensure_no_disposables_are_leaked_in_test_suite();
    type Node = GridNodeDescriptor<()>;
    let leaf = |size: Option<f64>| Node::Leaf { size, data: () };
    let node_descriptor: Node = Node::Branch {
        size: None,
        groups: vec![leaf(Some(0.2)), leaf(Some(0.2)), Node::Branch { size: Some(0.6), groups: vec![leaf(None), leaf(None)] }],
    };
    let mut node_descriptor_copy = node_descriptor.clone();
    sanitize_grid_node_descriptor(&mut node_descriptor_copy, true);
    assert_eq!(
        node_descriptor_copy,
        Node::Branch { size: None, groups: vec![leaf(Some(0.2)), leaf(Some(0.2)), Node::Branch { size: Some(0.6), groups: vec![leaf(Some(0.5)), leaf(Some(0.5))] }] }
    );
}

#[test]
fn create_serialized_grid_case() {
    let _ = ensure_no_disposables_are_leaked_in_test_suite();
    type Node = GridNodeDescriptor<String>;
    let leaf = |size: Option<f64>, data: &str| Node::Leaf { size, data: data.to_owned() };
    let grid_descriptor = GridDescriptor {
        orientation: Orientation::Vertical,
        size: None,
        groups: vec![leaf(Some(0.2), "a"), leaf(Some(0.2), "b"), Node::Branch { size: Some(0.6), groups: vec![leaf(None, "c"), leaf(None, "d")] }],
    };
    let serialized_grid = create_serialized_grid(grid_descriptor);
    let l = |size: f64, data: &str| SerializedNode::Leaf { data: data.to_owned(), size: Some(size), visible: None, maximized: None };
    assert_eq!(
        serialized_grid,
        SerializedGrid {
            root: SerializedNode::Branch {
                size: None,
                data: vec![l(0.2, "a"), l(0.2, "b"), SerializedNode::Branch { size: Some(0.6), data: vec![l(0.5, "c"), l(0.5, "d")], visible: None }],
                visible: None,
            },
            orientation: Orientation::Vertical,
            width: 1.0,
            height: 1.0,
        }
    );
}

#[test]
fn create_serialized_grid_issue_85601_should_not_allow_single_children_groups() {
    let store = ensure_no_disposables_are_leaked_in_test_suite();
    let ui = Ui::new();
    type Node = GridNodeDescriptor<TestJson>;
    let l = || Node::Leaf { size: None, data: TestJson::default() };
    let serialized_grid = create_serialized_grid(GridDescriptor {
        orientation: Orientation::Horizontal,
        size: None,
        groups: vec![Node::Branch { size: Some(0.5), groups: vec![l(), l()] }, Node::Branch { size: Some(0.5), groups: vec![l()] }],
    });
    let views: RefCell<Vec<Rc<TestView>>> = RefCell::new(Vec::new());
    let guard_store = store.store();
    let deserializer = |_: &TestJson| -> Rc<TestView> {
        let view = guard_store.add_rc(TestView::new(&ui, 0.0, INF, 0.0, INF));
        views.borrow_mut().push(view.clone());
        view
    };

    let grid = store.add_rc(SerializableGrid::deserialize(&ui, &serialized_grid, &deserializer, GridOptions::default()));
    assert_eq!(views.borrow().len(), 3);

    // should not throw
    let third = views.borrow()[2].clone();
    grid.remove_view(&third, None);
}

#[test]
fn from() {
    let store = ensure_no_disposables_are_leaked_in_test_suite();
    let ui = Ui::new();
    let create_view = || store.add_rc(TestView::new(&ui, 0.0, INF, 0.0, INF));

    let (a, b, c, d) = (create_view(), create_view(), create_view(), create_view());

    type Node = GridNodeDescriptor<Option<Rc<TestView>>>;
    let leaf = |size: Option<f64>, data: &Rc<TestView>| Node::Leaf { size, data: Some(data.clone()) };
    let grid_descriptor = GridDescriptor {
        orientation: Orientation::Vertical,
        size: None,
        groups: vec![leaf(Some(0.2), &a), leaf(Some(0.2), &b), Node::Branch { size: Some(0.6), groups: vec![leaf(None, &c), leaf(None, &d)] }],
    };
    let grid = SerializableGrid::from(&ui, grid_descriptor, GridOptions::default());

    assert_eq!(arrays(&grid), list(vec![view(&a), view(&b), list(vec![view(&c), view(&d)])]));
    grid.dispose();
}

#[test]
fn serialize_should_store_visibility_and_previous_size() {
    let store = ensure_no_disposables_are_leaked_in_test_suite();
    let ui = Ui::new();
    let view1 = named(&store, &ui, "view1");
    let grid = new_serializable_grid(&store, &ui, &view1);
    grid.layout(800.0, 600.0, 0.0, 0.0);

    let view2 = named(&store, &ui, "view2");
    grid.add_view(view2.clone(), 200.0, &view1, Direction::Up);

    let view3 = named(&store, &ui, "view3");
    grid.add_view(view3.clone(), 200.0, &view1, Direction::Right);

    let view4 = named(&store, &ui, "view4");
    grid.add_view(view4.clone(), 200.0, &view2, Direction::Left);

    let view5 = named(&store, &ui, "view5");
    grid.add_view(view5.clone(), 100.0, &view1, Direction::Down);

    assert_eq!(view1.size(), (600.0, 300.0));
    assert_eq!(view2.size(), (600.0, 200.0));
    assert_eq!(view3.size(), (200.0, 400.0));
    assert_eq!(view4.size(), (200.0, 200.0));
    assert_eq!(view5.size(), (600.0, 100.0));

    grid.set_view_visible(&view5, false, None);

    assert_eq!(view1.size(), (600.0, 400.0));
    assert_eq!(view2.size(), (600.0, 200.0));
    assert_eq!(view3.size(), (200.0, 400.0));
    assert_eq!(view4.size(), (200.0, 200.0));
    assert_eq!(view5.size(), (600.0, 0.0));

    grid.set_view_visible(&view5, true, None);

    assert_eq!(view1.size(), (600.0, 300.0));
    assert_eq!(view2.size(), (600.0, 200.0));
    assert_eq!(view3.size(), (200.0, 400.0));
    assert_eq!(view4.size(), (200.0, 200.0));
    assert_eq!(view5.size(), (600.0, 100.0));

    grid.set_view_visible(&view5, false, None);

    assert_eq!(view1.size(), (600.0, 400.0));
    assert_eq!(view2.size(), (600.0, 200.0));
    assert_eq!(view3.size(), (200.0, 400.0));
    assert_eq!(view4.size(), (200.0, 200.0));
    assert_eq!(view5.size(), (600.0, 0.0));

    grid.set_view_visible(&view5, false, None);

    let json = grid.serialize();
    assert_eq!(
        json,
        serialized(branch(
            vec![
                branch(vec![leaf("view4", 200.0), leaf("view2", 600.0)], 200.0),
                branch(vec![branch(vec![leaf("view1", 400.0), leaf_hidden("view5", 100.0)], 600.0), leaf("view3", 200.0)], 400.0),
            ],
            800.0,
        ))
    );

    grid.dispose();

    let deserializer = TestViewDeserializer::new(&store, &ui);
    let grid2 = store.add_rc(SerializableGrid::deserialize(&ui, &json, &deserializer, GridOptions::default()));

    let view1_copy = deserializer.get_view("view1");
    let view2_copy = deserializer.get_view("view2");
    let view3_copy = deserializer.get_view("view3");
    let view4_copy = deserializer.get_view("view4");
    let view5_copy = deserializer.get_view("view5");

    assert_eq!(
        arrays(&grid2),
        list(vec![list(vec![view(&view4_copy), view(&view2_copy)]), list(vec![list(vec![view(&view1_copy), view(&view5_copy)]), view(&view3_copy)])])
    );

    grid2.layout(800.0, 600.0, 0.0, 0.0);
    assert_eq!(view1_copy.size(), (600.0, 400.0));
    assert_eq!(view2_copy.size(), (600.0, 200.0));
    assert_eq!(view3_copy.size(), (200.0, 400.0));
    assert_eq!(view4_copy.size(), (200.0, 200.0));
    assert_eq!(view5_copy.size(), (600.0, 0.0));

    assert!(grid2.is_view_visible(&view1_copy));
    assert!(grid2.is_view_visible(&view2_copy));
    assert!(grid2.is_view_visible(&view3_copy));
    assert!(grid2.is_view_visible(&view4_copy));
    assert!(!grid2.is_view_visible(&view5_copy));

    grid2.set_view_visible(&view5_copy, true, None);

    assert_eq!(view1_copy.size(), (600.0, 300.0));
    assert_eq!(view2_copy.size(), (600.0, 200.0));
    assert_eq!(view3_copy.size(), (200.0, 400.0));
    assert_eq!(view4_copy.size(), (200.0, 200.0));
    assert_eq!(view5_copy.size(), (600.0, 100.0));

    assert!(grid2.is_view_visible(&view1_copy));
    assert!(grid2.is_view_visible(&view2_copy));
    assert!(grid2.is_view_visible(&view3_copy));
    assert!(grid2.is_view_visible(&view4_copy));
    assert!(grid2.is_view_visible(&view5_copy));
}

#[test]
fn serialize_should_store_visibility_and_previous_size_even_for_first_leaf() {
    let store = ensure_no_disposables_are_leaked_in_test_suite();
    let ui = Ui::new();
    let view1 = named(&store, &ui, "view1");
    let grid = new_serializable_grid(&store, &ui, &view1);
    grid.layout(800.0, 600.0, 0.0, 0.0);

    let view2 = named(&store, &ui, "view2");
    grid.add_view(view2.clone(), 200.0, &view1, Direction::Up);

    let view3 = named(&store, &ui, "view3");
    grid.add_view(view3.clone(), 200.0, &view1, Direction::Right);

    let view4 = named(&store, &ui, "view4");
    grid.add_view(view4.clone(), 200.0, &view2, Direction::Left);

    let view5 = named(&store, &ui, "view5");
    grid.add_view(view5.clone(), 100.0, &view1, Direction::Down);

    assert_eq!(view1.size(), (600.0, 300.0));
    assert_eq!(view2.size(), (600.0, 200.0));
    assert_eq!(view3.size(), (200.0, 400.0));
    assert_eq!(view4.size(), (200.0, 200.0));
    assert_eq!(view5.size(), (600.0, 100.0));

    grid.set_view_visible(&view4, false, None);

    assert_eq!(view1.size(), (600.0, 300.0));
    assert_eq!(view2.size(), (800.0, 200.0));
    assert_eq!(view3.size(), (200.0, 400.0));
    assert_eq!(view4.size(), (0.0, 200.0));
    assert_eq!(view5.size(), (600.0, 100.0));

    let json = grid.serialize();
    assert_eq!(
        json,
        serialized(branch(
            vec![
                branch(vec![leaf_hidden("view4", 200.0), leaf("view2", 800.0)], 200.0),
                branch(vec![branch(vec![leaf("view1", 300.0), leaf("view5", 100.0)], 600.0), leaf("view3", 200.0)], 400.0),
            ],
            800.0,
        ))
    );

    grid.dispose();

    let deserializer = TestViewDeserializer::new(&store, &ui);
    let grid2 = store.add_rc(SerializableGrid::deserialize(&ui, &json, &deserializer, GridOptions::default()));

    let view1_copy = deserializer.get_view("view1");
    let view2_copy = deserializer.get_view("view2");
    let view3_copy = deserializer.get_view("view3");
    let view4_copy = deserializer.get_view("view4");
    let view5_copy = deserializer.get_view("view5");

    assert_eq!(
        arrays(&grid2),
        list(vec![list(vec![view(&view4_copy), view(&view2_copy)]), list(vec![list(vec![view(&view1_copy), view(&view5_copy)]), view(&view3_copy)])])
    );

    grid2.layout(800.0, 600.0, 0.0, 0.0);
    assert_eq!(view1_copy.size(), (600.0, 300.0));
    assert_eq!(view2_copy.size(), (800.0, 200.0));
    assert_eq!(view3_copy.size(), (200.0, 400.0));
    assert_eq!(view4_copy.size(), (0.0, 200.0));
    assert_eq!(view5_copy.size(), (600.0, 100.0));

    assert!(grid2.is_view_visible(&view1_copy));
    assert!(grid2.is_view_visible(&view2_copy));
    assert!(grid2.is_view_visible(&view3_copy));
    assert!(!grid2.is_view_visible(&view4_copy));
    assert!(grid2.is_view_visible(&view5_copy));

    grid2.set_view_visible(&view4_copy, true, None);

    assert_eq!(view1_copy.size(), (600.0, 300.0));
    assert_eq!(view2_copy.size(), (600.0, 200.0));
    assert_eq!(view3_copy.size(), (200.0, 400.0));
    assert_eq!(view4_copy.size(), (200.0, 200.0));
    assert_eq!(view5_copy.size(), (600.0, 100.0));

    assert!(grid2.is_view_visible(&view1_copy));
    assert!(grid2.is_view_visible(&view2_copy));
    assert!(grid2.is_view_visible(&view3_copy));
    assert!(grid2.is_view_visible(&view4_copy));
    assert!(grid2.is_view_visible(&view5_copy));
}
