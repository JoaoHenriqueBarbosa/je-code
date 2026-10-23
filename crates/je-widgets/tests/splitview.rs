//! Porte de `src/vs/base/test/browser/ui/splitview/splitview.test.ts` (suíte `Splitview`), caso a caso e na mesma ordem.
//!
//! Notas de tradução:
//! - `container.firstElementChild`, `querySelectorAll` e `style.height` viram leituras da árvore do `je-ui`
//!   (`Tree::children`, `support::query_all`, `Tree::bounds`).
//! - `assert.throws` é `support::assert_throws` (o porte lança com `errors::throw`, um pânico com `Error`).
//! - `TestView` implementa `SplitViewView<f64>`: o contexto de layout do original é `number` (o tamanho
//!   ortogonal), e `undefined` é `None`.

mod support;

use std::cell::Cell;
use std::rc::Rc;

use je_base::event::{Emitter, Event};
use je_base::lifecycle::IDisposable;
use je_ui::{NodeId, Role, Ui};
use je_widgets::sash::{Sash, SashState};
use je_widgets::splitview::{LayoutPriority, Sizing, SplitView, SplitViewOptions, SplitViewView};
use support::{assert_throws, create_container, ensure_no_disposables_are_leaked_in_test_suite, query_all};

const INF: f64 = f64::INFINITY;

struct TestView {
    ui: Ui,
    on_did_change: Rc<Emitter<Option<f64>>>,
    minimum_size: Cell<f64>,
    maximum_size: Cell<f64>,
    priority: LayoutPriority,
    element: NodeId,
    on_did_get_element: Rc<Emitter<()>>,
    size: Cell<f64>,
    orthogonal_size: Cell<Option<f64>>,
    on_did_layout: Rc<Emitter<(f64, Option<f64>)>>,
    on_did_focus: Rc<Emitter<()>>,
    on_did_set_visible: Rc<Emitter<bool>>,
}

impl TestView {
    fn new(ui: &Ui, minimum_size: f64, maximum_size: f64) -> Rc<Self> {
        Self::with_priority(ui, minimum_size, maximum_size, LayoutPriority::Normal)
    }

    fn with_priority(ui: &Ui, minimum_size: f64, maximum_size: f64, priority: LayoutPriority) -> Rc<Self> {
        assert!(minimum_size <= maximum_size, "splitview view minimum size must be <= maximum size");
        Rc::new(Self {
            ui: ui.clone(),
            on_did_change: Emitter::new(),
            minimum_size: Cell::new(minimum_size),
            maximum_size: Cell::new(maximum_size),
            priority,
            element: ui.tree_mut().create_detached(Role::Generic),
            on_did_get_element: Emitter::new(),
            size: Cell::new(0.0),
            orthogonal_size: Cell::new(Some(0.0)),
            on_did_layout: Emitter::new(),
            on_did_focus: Emitter::new(),
            on_did_set_visible: Emitter::new(),
        })
    }

    fn set_minimum_size(&self, size: f64) {
        self.minimum_size.set(size);
        self.on_did_change.fire(None);
    }

    fn set_maximum_size(&self, size: f64) {
        self.maximum_size.set(size);
        self.on_did_change.fire(None);
    }

    fn size(&self) -> f64 {
        self.size.get()
    }

    /// Change both size constraints at once, as a view with a fixed width does when that width changes.
    fn set_size_constraints(&self, minimum_size: f64, maximum_size: f64) {
        self.minimum_size.set(minimum_size);
        self.maximum_size.set(maximum_size);
        self.on_did_change.fire(None);
    }

    #[allow(dead_code)]
    fn focus(&self) {
        self.on_did_focus.fire(());
    }
}

impl SplitViewView<f64> for TestView {
    fn element(&self) -> NodeId {
        self.on_did_get_element.fire(());
        self.element
    }

    fn minimum_size(&self) -> f64 {
        self.minimum_size.get()
    }

    fn maximum_size(&self) -> f64 {
        self.maximum_size.get()
    }

    fn priority(&self) -> Option<LayoutPriority> {
        Some(self.priority)
    }

    fn on_did_change(&self) -> Event<Option<f64>> {
        self.on_did_change.event()
    }

    fn layout(&self, size: f64, _offset: f64, orthogonal_size: Option<&f64>) {
        self.size.set(size);
        self.orthogonal_size.set(orthogonal_size.copied());
        self.on_did_layout.fire((size, orthogonal_size.copied()));
    }

    fn set_visible(&self, visible: bool) {
        self.on_did_set_visible.fire(visible);
    }
}

impl IDisposable for TestView {
    fn dispose(&self) {
        self.on_did_change.dispose();
        self.on_did_get_element.dispose();
        self.on_did_layout.dispose();
        self.on_did_focus.dispose();
        self.on_did_set_visible.dispose();
        self.ui.remove(self.element);
    }
}

fn get_sashes(splitview: &SplitView<f64>) -> Vec<Rc<Sash>> {
    splitview.sashes()
}

fn views_of(ui: &Ui, container: NodeId) -> Vec<NodeId> {
    query_all(&ui.tree(), container, &["monaco-split-view2", "monaco-scrollable-element", "split-view-container", "split-view-view"])
}

fn sashes_of(ui: &Ui, container: NodeId) -> Vec<NodeId> {
    query_all(&ui.tree(), container, &["monaco-split-view2", "sash-container", "monaco-sash"])
}

fn new_split_view(ui: &Ui, container: NodeId, options: SplitViewOptions<f64>) -> Rc<SplitView<f64>> {
    SplitView::new(ui, container, options)
}

fn setup() -> (Ui, NodeId) {
    let ui = Ui::new();
    let container = create_container(&ui, 200.0, 200.0);
    (ui, container)
}

fn sizes(views: &[&Rc<TestView>]) -> Vec<f64> {
    views.iter().map(|v| v.size()).collect()
}

#[test]
fn empty_splitview_has_empty_dom() {
    let store = ensure_no_disposables_are_leaked_in_test_suite();
    let (ui, container) = setup();
    store.add_rc(new_split_view(&ui, container, SplitViewOptions::default()));
    let tree = ui.tree();
    let split_view_el = tree.children(container)[0];
    let first = tree.children(split_view_el)[0];
    assert_eq!(tree.children(first).len(), 0, "split view should be empty");
}

#[test]
fn has_views_and_sashes_as_children() {
    let store = ensure_no_disposables_are_leaked_in_test_suite();
    let (ui, container) = setup();
    let view1 = store.add_rc(TestView::new(&ui, 20.0, 20.0));
    let view2 = store.add_rc(TestView::new(&ui, 20.0, 20.0));
    let view3 = store.add_rc(TestView::new(&ui, 20.0, 20.0));
    let splitview = store.add_rc(new_split_view(&ui, container, SplitViewOptions::default()));

    splitview.add_view(view1.clone(), 20.0, None, false);
    splitview.add_view(view2.clone(), 20.0, None, false);
    splitview.add_view(view3.clone(), 20.0, None, false);

    assert_eq!(views_of(&ui, container).len(), 3, "split view should have 3 views");
    assert_eq!(sashes_of(&ui, container).len(), 2, "split view should have 2 sashes");

    splitview.remove_view(2, None);

    assert_eq!(views_of(&ui, container).len(), 2, "split view should have 2 views");
    assert_eq!(sashes_of(&ui, container).len(), 1, "split view should have 1 sash");

    splitview.remove_view(0, None);

    assert_eq!(views_of(&ui, container).len(), 1, "split view should have 1 view");
    assert_eq!(sashes_of(&ui, container).len(), 0, "split view should have no sashes");

    splitview.remove_view(0, None);

    assert_eq!(views_of(&ui, container).len(), 0, "split view should have no views");
    assert_eq!(sashes_of(&ui, container).len(), 0, "split view should have no sashes");
}

#[test]
fn calls_view_methods_on_add_view_and_remove_view() {
    let store = ensure_no_disposables_are_leaked_in_test_suite();
    let (ui, container) = setup();
    let view = store.add_rc(TestView::new(&ui, 20.0, 20.0));
    let splitview = store.add_rc(new_split_view(&ui, container, SplitViewOptions::default()));

    let did_layout = Rc::new(Cell::new(false));
    let flag = did_layout.clone();
    store.add(view.on_did_layout.event().listen(move |_| flag.set(true)));
    store.add(view.on_did_get_element.event().listen(|_| ()));

    splitview.add_view(view.clone(), 20.0, None, false);

    assert_eq!(view.size(), 20.0, "view has right size");
    assert!(did_layout.get(), "layout is called");
    assert!(did_layout.get(), "render is called");
}

#[test]
fn stretches_view_to_viewport() {
    let store = ensure_no_disposables_are_leaked_in_test_suite();
    let (ui, container) = setup();
    let view = store.add_rc(TestView::new(&ui, 20.0, INF));
    let splitview = store.add_rc(new_split_view(&ui, container, SplitViewOptions::default()));
    splitview.layout(200.0, None);

    splitview.add_view(view.clone(), 20.0, None, false);
    assert_eq!(view.size(), 200.0, "view is stretched");

    splitview.layout(200.0, None);
    assert_eq!(view.size(), 200.0, "view stayed the same");

    splitview.layout(100.0, None);
    assert_eq!(view.size(), 100.0, "view is collapsed");

    splitview.layout(20.0, None);
    assert_eq!(view.size(), 20.0, "view is collapsed");

    splitview.layout(10.0, None);
    assert_eq!(view.size(), 20.0, "view is clamped");

    splitview.layout(200.0, None);
    assert_eq!(view.size(), 200.0, "view is stretched");
}

#[test]
fn can_resize_views() {
    let store = ensure_no_disposables_are_leaked_in_test_suite();
    let (ui, container) = setup();
    let view1 = store.add_rc(TestView::new(&ui, 20.0, INF));
    let view2 = store.add_rc(TestView::new(&ui, 20.0, INF));
    let view3 = store.add_rc(TestView::new(&ui, 20.0, INF));
    let splitview = store.add_rc(new_split_view(&ui, container, SplitViewOptions::default()));
    splitview.layout(200.0, None);

    splitview.add_view(view1.clone(), 20.0, None, false);
    splitview.add_view(view2.clone(), 20.0, None, false);
    splitview.add_view(view3.clone(), 20.0, None, false);

    assert_eq!(view1.size(), 160.0, "view1 is stretched");
    assert_eq!(view2.size(), 20.0, "view2 size is 20");
    assert_eq!(view3.size(), 20.0, "view3 size is 20");

    splitview.resize_view(1, 40.0);

    assert_eq!(view1.size(), 140.0, "view1 is collapsed");
    assert_eq!(view2.size(), 40.0, "view2 is stretched");
    assert_eq!(view3.size(), 20.0, "view3 stays the same");

    splitview.resize_view(0, 70.0);

    assert_eq!(view1.size(), 70.0, "view1 is collapsed");
    assert_eq!(view2.size(), 40.0, "view2 stays the same");
    assert_eq!(view3.size(), 90.0, "view3 is stretched");

    splitview.resize_view(2, 40.0);

    assert_eq!(view1.size(), 70.0, "view1 stays the same");
    assert_eq!(view2.size(), 90.0, "view2 is collapsed");
    assert_eq!(view3.size(), 40.0, "view3 is stretched");
}

#[test]
fn reacts_to_view_changes() {
    let store = ensure_no_disposables_are_leaked_in_test_suite();
    let (ui, container) = setup();
    let view1 = store.add_rc(TestView::new(&ui, 20.0, INF));
    let view2 = store.add_rc(TestView::new(&ui, 20.0, INF));
    let view3 = store.add_rc(TestView::new(&ui, 20.0, INF));
    let splitview = store.add_rc(new_split_view(&ui, container, SplitViewOptions::default()));
    splitview.layout(200.0, None);

    splitview.add_view(view1.clone(), 20.0, None, false);
    splitview.add_view(view2.clone(), 20.0, None, false);
    splitview.add_view(view3.clone(), 20.0, None, false);

    assert_eq!(view1.size(), 160.0, "view1 is stretched");
    assert_eq!(view2.size(), 20.0, "view2 size is 20");
    assert_eq!(view3.size(), 20.0, "view3 size is 20");

    view1.set_maximum_size(20.0);

    assert_eq!(view1.size(), 20.0, "view1 is collapsed");
    assert_eq!(view2.size(), 20.0, "view2 stays the same");
    assert_eq!(view3.size(), 160.0, "view3 is stretched");

    view3.set_maximum_size(40.0);

    assert_eq!(view1.size(), 20.0, "view1 stays the same");
    assert_eq!(view2.size(), 140.0, "view2 is stretched");
    assert_eq!(view3.size(), 40.0, "view3 is collapsed");

    view2.set_maximum_size(200.0);

    assert_eq!(view1.size(), 20.0, "view1 stays the same");
    assert_eq!(view2.size(), 140.0, "view2 stays the same");
    assert_eq!(view3.size(), 40.0, "view3 stays the same");

    view3.set_maximum_size(INF);
    view3.set_minimum_size(100.0);

    assert_eq!(view1.size(), 20.0, "view1 is collapsed");
    assert_eq!(view2.size(), 80.0, "view2 is collapsed");
    assert_eq!(view3.size(), 100.0, "view3 is stretched");
}

#[test]
fn sashes_are_properly_enabled_disabled() {
    let store = ensure_no_disposables_are_leaked_in_test_suite();
    let (ui, container) = setup();
    let view1 = store.add_rc(TestView::new(&ui, 20.0, INF));
    let view2 = store.add_rc(TestView::new(&ui, 20.0, INF));
    let view3 = store.add_rc(TestView::new(&ui, 20.0, INF));
    let splitview = store.add_rc(new_split_view(&ui, container, SplitViewOptions::default()));
    splitview.layout(200.0, None);

    splitview.add_view(view1.clone(), Sizing::Distribute, None, false);
    splitview.add_view(view2.clone(), Sizing::Distribute, None, false);
    splitview.add_view(view3.clone(), Sizing::Distribute, None, false);

    let sashes = get_sashes(&splitview);
    assert_eq!(sashes.len(), 2, "there are two sashes");
    assert_eq!(sashes[0].state(), SashState::Enabled, "first sash is enabled");
    assert_eq!(sashes[1].state(), SashState::Enabled, "second sash is enabled");

    splitview.layout(60.0, None);
    assert_eq!(sashes[0].state(), SashState::Disabled, "first sash is disabled");
    assert_eq!(sashes[1].state(), SashState::Disabled, "second sash is disabled");

    splitview.layout(20.0, None);
    assert_eq!(sashes[0].state(), SashState::Disabled, "first sash is disabled");
    assert_eq!(sashes[1].state(), SashState::Disabled, "second sash is disabled");

    splitview.layout(200.0, None);
    assert_eq!(sashes[0].state(), SashState::Enabled, "first sash is enabled");
    assert_eq!(sashes[1].state(), SashState::Enabled, "second sash is enabled");

    view1.set_maximum_size(20.0);
    assert_eq!(sashes[0].state(), SashState::Disabled, "first sash is disabled");
    assert_eq!(sashes[1].state(), SashState::Enabled, "second sash is enabled");

    view2.set_maximum_size(20.0);
    assert_eq!(sashes[0].state(), SashState::Disabled, "first sash is disabled");
    assert_eq!(sashes[1].state(), SashState::Disabled, "second sash is disabled");

    view1.set_maximum_size(300.0);
    assert_eq!(sashes[0].state(), SashState::AtMinimum, "first sash is enabled");
    assert_eq!(sashes[1].state(), SashState::AtMinimum, "second sash is enabled");

    view2.set_maximum_size(200.0);
    assert_eq!(sashes[0].state(), SashState::AtMinimum, "first sash is enabled");
    assert_eq!(sashes[1].state(), SashState::AtMinimum, "second sash is enabled");

    splitview.resize_view(0, 40.0);
    assert_eq!(sashes[0].state(), SashState::Enabled, "first sash is enabled");
    assert_eq!(sashes[1].state(), SashState::Enabled, "second sash is enabled");
}

#[test]
fn issue_35497() {
    let store = ensure_no_disposables_are_leaked_in_test_suite();
    let (ui, container) = setup();
    let view1 = store.add_rc(TestView::new(&ui, 160.0, INF));
    let view2 = store.add_rc(TestView::new(&ui, 66.0, 66.0));

    let splitview = store.add_rc(new_split_view(&ui, container, SplitViewOptions::default()));
    splitview.layout(986.0, None);

    splitview.add_view(view1.clone(), 142.0, Some(0), false);
    assert_eq!(view1.size(), 986.0, "first view is stretched");

    let during_add = splitview.clone();
    store.add(view2.on_did_get_element.event().listen(move |_| {
        assert_throws(|| during_add.resize_view(1, 922.0));
        assert_throws(|| during_add.resize_view(1, 922.0));
    }));

    splitview.add_view(view2.clone(), 66.0, Some(0), false);
    assert_eq!(view2.size(), 66.0, "second view is fixed");
    assert_eq!(view1.size(), 986.0 - 66.0, "first view is collapsed");

    let view_containers = query_all(&ui.tree(), container, &["split-view-view"]);
    assert_eq!(view_containers.len(), 2, "there are two view containers");
    assert_eq!(ui.tree().bounds(view_containers[0]).height(), 66.0, "second view container is 66px");
    assert_eq!(ui.tree().bounds(view_containers[1]).height(), 986.0 - 66.0, "first view container is 66px");
}

#[test]
fn automatic_size_distribution() {
    let store = ensure_no_disposables_are_leaked_in_test_suite();
    let (ui, container) = setup();
    let view1 = store.add_rc(TestView::new(&ui, 20.0, INF));
    let view2 = store.add_rc(TestView::new(&ui, 20.0, INF));
    let view3 = store.add_rc(TestView::new(&ui, 20.0, INF));
    let splitview = store.add_rc(new_split_view(&ui, container, SplitViewOptions::default()));
    splitview.layout(200.0, None);

    splitview.add_view(view1.clone(), Sizing::Distribute, None, false);
    assert_eq!(view1.size(), 200.0);

    splitview.add_view(view2.clone(), 50.0, None, false);
    assert_eq!(sizes(&[&view1, &view2]), [150.0, 50.0]);

    splitview.add_view(view3.clone(), Sizing::Distribute, None, false);
    assert_eq!(sizes(&[&view1, &view2, &view3]), [66.0, 66.0, 68.0]);

    splitview.remove_view(1, Some(Sizing::Distribute));
    assert_eq!(sizes(&[&view1, &view3]), [100.0, 100.0]);
}

#[test]
fn add_views_before_layout() {
    let store = ensure_no_disposables_are_leaked_in_test_suite();
    let (ui, container) = setup();
    let view1 = store.add_rc(TestView::new(&ui, 20.0, INF));
    let view2 = store.add_rc(TestView::new(&ui, 20.0, INF));
    let view3 = store.add_rc(TestView::new(&ui, 20.0, INF));
    let splitview = store.add_rc(new_split_view(&ui, container, SplitViewOptions::default()));

    splitview.add_view(view1.clone(), 100.0, None, false);
    splitview.add_view(view2.clone(), 75.0, None, false);
    splitview.add_view(view3.clone(), 25.0, None, false);

    splitview.layout(200.0, None);
    assert_eq!(sizes(&[&view1, &view2, &view3]), [67.0, 67.0, 66.0]);
}

#[test]
fn split_sizing() {
    let store = ensure_no_disposables_are_leaked_in_test_suite();
    let (ui, container) = setup();
    let view1 = store.add_rc(TestView::new(&ui, 20.0, INF));
    let view2 = store.add_rc(TestView::new(&ui, 20.0, INF));
    let view3 = store.add_rc(TestView::new(&ui, 20.0, INF));
    let splitview = store.add_rc(new_split_view(&ui, container, SplitViewOptions::default()));
    splitview.layout(200.0, None);

    splitview.add_view(view1.clone(), Sizing::Distribute, None, false);
    assert_eq!(view1.size(), 200.0);

    splitview.add_view(view2.clone(), Sizing::Split(0), None, false);
    assert_eq!(sizes(&[&view1, &view2]), [100.0, 100.0]);

    splitview.add_view(view3.clone(), Sizing::Split(1), None, false);
    assert_eq!(sizes(&[&view1, &view2, &view3]), [100.0, 50.0, 50.0]);
}

#[test]
fn split_sizing_2() {
    let store = ensure_no_disposables_are_leaked_in_test_suite();
    let (ui, container) = setup();
    let view1 = store.add_rc(TestView::new(&ui, 20.0, INF));
    let view2 = store.add_rc(TestView::new(&ui, 20.0, INF));
    let view3 = store.add_rc(TestView::new(&ui, 20.0, INF));
    let splitview = store.add_rc(new_split_view(&ui, container, SplitViewOptions::default()));
    splitview.layout(200.0, None);

    splitview.add_view(view1.clone(), Sizing::Distribute, None, false);
    assert_eq!(view1.size(), 200.0);

    splitview.add_view(view2.clone(), Sizing::Split(0), None, false);
    assert_eq!(sizes(&[&view1, &view2]), [100.0, 100.0]);

    splitview.add_view(view3.clone(), Sizing::Split(0), None, false);
    assert_eq!(sizes(&[&view1, &view2, &view3]), [50.0, 100.0, 50.0]);
}

#[test]
fn proportional_layout() {
    let store = ensure_no_disposables_are_leaked_in_test_suite();
    let (ui, container) = setup();
    let view1 = store.add_rc(TestView::new(&ui, 20.0, INF));
    let view2 = store.add_rc(TestView::new(&ui, 20.0, INF));
    let splitview = store.add_rc(new_split_view(&ui, container, SplitViewOptions::default()));
    splitview.layout(200.0, None);

    splitview.add_view(view1.clone(), Sizing::Distribute, None, false);
    splitview.add_view(view2.clone(), Sizing::Distribute, None, false);
    assert_eq!(sizes(&[&view1, &view2]), [100.0, 100.0]);

    splitview.layout(100.0, None);
    assert_eq!(sizes(&[&view1, &view2]), [50.0, 50.0]);
}

#[test]
fn disable_proportional_layout() {
    let store = ensure_no_disposables_are_leaked_in_test_suite();
    let (ui, container) = setup();
    let view1 = store.add_rc(TestView::new(&ui, 20.0, INF));
    let view2 = store.add_rc(TestView::new(&ui, 20.0, INF));
    let splitview = store.add_rc(new_split_view(&ui, container, SplitViewOptions { proportional_layout: Some(false), ..SplitViewOptions::default() }));
    splitview.layout(200.0, None);

    splitview.add_view(view1.clone(), Sizing::Distribute, None, false);
    splitview.add_view(view2.clone(), Sizing::Distribute, None, false);
    assert_eq!(sizes(&[&view1, &view2]), [100.0, 100.0]);

    splitview.layout(100.0, None);
    assert_eq!(sizes(&[&view1, &view2]), [80.0, 20.0]);
}

#[test]
fn high_layout_priority() {
    let store = ensure_no_disposables_are_leaked_in_test_suite();
    let (ui, container) = setup();
    let view1 = store.add_rc(TestView::new(&ui, 20.0, INF));
    let view2 = store.add_rc(TestView::with_priority(&ui, 20.0, INF, LayoutPriority::High));
    let view3 = store.add_rc(TestView::new(&ui, 20.0, INF));
    let splitview = store.add_rc(new_split_view(&ui, container, SplitViewOptions { proportional_layout: Some(false), ..SplitViewOptions::default() }));
    splitview.layout(200.0, None);

    splitview.add_view(view1.clone(), Sizing::Distribute, None, false);
    splitview.add_view(view2.clone(), Sizing::Distribute, None, false);
    splitview.add_view(view3.clone(), Sizing::Distribute, None, false);
    assert_eq!(sizes(&[&view1, &view2, &view3]), [66.0, 68.0, 66.0]);

    splitview.layout(180.0, None);
    assert_eq!(sizes(&[&view1, &view2, &view3]), [66.0, 48.0, 66.0]);

    splitview.layout(124.0, None);
    assert_eq!(sizes(&[&view1, &view2, &view3]), [66.0, 20.0, 38.0]);

    splitview.layout(60.0, None);
    assert_eq!(sizes(&[&view1, &view2, &view3]), [20.0, 20.0, 20.0]);

    splitview.layout(200.0, None);
    assert_eq!(sizes(&[&view1, &view2, &view3]), [20.0, 160.0, 20.0]);
}

#[test]
fn low_layout_priority() {
    let store = ensure_no_disposables_are_leaked_in_test_suite();
    let (ui, container) = setup();
    let view1 = store.add_rc(TestView::new(&ui, 20.0, INF));
    let view2 = store.add_rc(TestView::new(&ui, 20.0, INF));
    let view3 = store.add_rc(TestView::with_priority(&ui, 20.0, INF, LayoutPriority::Low));
    let splitview = store.add_rc(new_split_view(&ui, container, SplitViewOptions { proportional_layout: Some(false), ..SplitViewOptions::default() }));
    splitview.layout(200.0, None);

    splitview.add_view(view1.clone(), Sizing::Distribute, None, false);
    splitview.add_view(view2.clone(), Sizing::Distribute, None, false);
    splitview.add_view(view3.clone(), Sizing::Distribute, None, false);
    assert_eq!(sizes(&[&view1, &view2, &view3]), [66.0, 68.0, 66.0]);

    splitview.layout(180.0, None);
    assert_eq!(sizes(&[&view1, &view2, &view3]), [66.0, 48.0, 66.0]);

    splitview.layout(132.0, None);
    assert_eq!(sizes(&[&view1, &view2, &view3]), [46.0, 20.0, 66.0]);

    splitview.layout(60.0, None);
    assert_eq!(sizes(&[&view1, &view2, &view3]), [20.0, 20.0, 20.0]);

    splitview.layout(200.0, None);
    assert_eq!(sizes(&[&view1, &view2, &view3]), [20.0, 160.0, 20.0]);
}

#[test]
fn view_changing_its_size_constraints_while_a_sibling_is_toggled_does_not_squeeze_that_sibling_334167() {
    let store = ensure_no_disposables_are_leaked_in_test_suite();
    let (ui, container) = setup();

    // Mirrors the workbench with the side bar on the right: the activity bar reports its new fixed width from within the visibility change of the side bar
    let editor = store.add_rc(TestView::with_priority(&ui, 20.0, INF, LayoutPriority::High));
    let side_bar = store.add_rc(TestView::with_priority(&ui, 20.0, INF, LayoutPriority::Low));
    let activity_bar = store.add_rc(TestView::new(&ui, 10.0, 10.0));
    let splitview = store.add_rc(new_split_view(&ui, container, SplitViewOptions { proportional_layout: Some(false), ..SplitViewOptions::default() }));
    splitview.layout(200.0, None);

    splitview.add_view(editor.clone(), 120.0, None, false);
    splitview.add_view(side_bar.clone(), 70.0, None, false);
    splitview.add_view(activity_bar.clone(), 10.0, None, false);

    // `addView` distributes in index order too, so restore the side bar to a known starting point
    splitview.resize_view(1, 70.0);
    assert_eq!(sizes(&[&editor, &side_bar, &activity_bar]), [120.0, 70.0, 10.0]);

    let activity = activity_bar.clone();
    store.add(side_bar.on_did_set_visible.event().listen(move |visible| {
        let size = if *visible { 10.0 } else { 12.0 };
        activity.set_size_constraints(size, size);
    }));

    splitview.set_view_visible(1, false);
    assert_eq!(sizes(&[&editor, &side_bar, &activity_bar]), [188.0, 0.0, 12.0]);

    splitview.set_view_visible(1, true);
    assert_eq!(sizes(&[&editor, &side_bar, &activity_bar]), [120.0, 70.0, 10.0]);
}

#[test]
fn context_propagates_to_views() {
    let store = ensure_no_disposables_are_leaked_in_test_suite();
    let (ui, container) = setup();
    let view1 = store.add_rc(TestView::new(&ui, 20.0, INF));
    let view2 = store.add_rc(TestView::new(&ui, 20.0, INF));
    let view3 = store.add_rc(TestView::with_priority(&ui, 20.0, INF, LayoutPriority::Low));
    let splitview = store.add_rc(new_split_view(&ui, container, SplitViewOptions { proportional_layout: Some(false), ..SplitViewOptions::default() }));
    splitview.layout(200.0, None);

    splitview.add_view(view1.clone(), Sizing::Distribute, None, false);
    splitview.add_view(view2.clone(), Sizing::Distribute, None, false);
    splitview.add_view(view3.clone(), Sizing::Distribute, None, false);

    splitview.layout(200.0, Some(100.0));
    let orthogonal: Vec<Option<f64>> = [&view1, &view2, &view3].iter().map(|v| v.orthogonal_size.get()).collect();
    assert_eq!(orthogonal, [Some(100.0), Some(100.0), Some(100.0)]);
}

