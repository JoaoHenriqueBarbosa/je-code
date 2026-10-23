//! Porte de `src/vs/base/test/browser/ui/splitview/paneview.test.ts` (suíte `Paneview`), caso a caso e na mesma ordem.
//!
//! Notas de tradução:
//! - `TestPane extends Pane` vira um `PaneContent` (`TestPane`) guardado dentro de um `Pane`; `lastLayout` é o
//!   campo `last_layout` do conteúdo.
//! - O `teardown` que restaura o tamanho global do cabeçalho é um `Drop` (`ResetHeaderSize`).

mod support;

use std::cell::Cell;
use std::rc::Rc;

use je_ui::{NodeId, Ui};
use je_widgets::paneview::{DEFAULT_PANE_HEADER_SIZE, Pane, PaneContent, PaneOptions, PaneView, PaneViewOptions, set_global_pane_header_size};
use je_widgets::splitview::SplitViewView;
use support::{create_container, ensure_no_disposables_are_leaked_in_test_suite};

struct TestPane {
    last_layout: Cell<Option<(f64, f64)>>,
}

impl PaneContent for TestPane {
    fn render_header(&self, _container: NodeId) {}
    fn render_body(&self, _container: NodeId) {}

    fn layout_body(&self, height: f64, width: f64) {
        self.last_layout.set(Some((height, width)));
    }
}

fn new_test_pane(ui: &Ui, expanded: bool) -> (Rc<Pane>, Rc<TestPane>) {
    let content = Rc::new(TestPane { last_layout: Cell::new(None) });
    let pane = Pane::new(ui, PaneOptions { title: "Test".to_owned(), minimum_body_size: Some(10.0), maximum_body_size: Some(100.0), expanded: Some(expanded), ..PaneOptions::default() }, content.clone());
    pane.render();
    (pane, content)
}

/// `teardown(() => setGlobalPaneHeaderSize(DEFAULT_PANE_HEADER_SIZE))`.
struct ResetHeaderSize;

impl Drop for ResetHeaderSize {
    fn drop(&mut self) {
        set_global_pane_header_size(DEFAULT_PANE_HEADER_SIZE);
    }
}

#[test]
fn uses_the_global_pane_header_size() {
    let _teardown = ResetHeaderSize;
    let store = ensure_no_disposables_are_leaked_in_test_suite();
    let ui = Ui::new();
    let (pane, content) = new_test_pane(&ui, true);
    let pane = store.add_rc(pane);
    pane.set_orthogonal_size(200.0);

    let default_sizes = (pane.minimum_size(), pane.maximum_size());
    pane.layout(50.0);
    let default_layout = content.last_layout.get();

    set_global_pane_header_size(28.0);

    let override_sizes = (pane.minimum_size(), pane.maximum_size());
    pane.layout(50.0);

    assert_eq!(default_sizes, (32.0, 122.0));
    assert_eq!(default_layout, Some((28.0, 200.0)));
    assert_eq!(override_sizes, (38.0, 128.0));
    assert_eq!(content.last_layout.get(), Some((22.0, 200.0)));
}

#[test]
fn reclamps_a_collapsed_pane_when_the_global_header_size_changes() {
    let _teardown = ResetHeaderSize;
    let store = ensure_no_disposables_are_leaked_in_test_suite();
    let ui = Ui::new();
    let container = create_container(&ui, 0.0, 0.0);
    let pane_view = store.add_rc(PaneView::new(&ui, container, PaneViewOptions::default()));
    let (pane, _content) = new_test_pane(&ui, false);
    let pane = store.add_rc(pane);
    pane_view.add_pane(pane.clone(), pane.minimum_size(), None);
    pane_view.layout(100.0, 200.0);
    let default_size = pane_view.get_pane_size(&pane);

    set_global_pane_header_size(28.0);
    pane_view.layout(100.0, 200.0);
    let override_size = pane_view.get_pane_size(&pane);

    set_global_pane_header_size(DEFAULT_PANE_HEADER_SIZE);
    pane_view.layout(100.0, 200.0);

    assert_eq!((default_size, override_size, pane_view.get_pane_size(&pane)), (22.0, 28.0, 22.0));
}
