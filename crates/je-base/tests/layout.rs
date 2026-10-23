//! Porte de `src/vs/base/test/common/layout.test.ts` (suíte `Layout`), caso a caso e na mesma ordem.

use je_base::layout::{ILayoutAnchor, LayoutAnchorPosition, layout as layout_fn};

fn position(viewport: f64, view: f64, offset: f64, size: f64, position: LayoutAnchorPosition) -> f64 {
    layout_fn(viewport, view, ILayoutAnchor { offset, size, mode: None, position }).position
}

#[test]
fn layout() {
    use LayoutAnchorPosition::{After, Before};
    assert_eq!(position(200.0, 20.0, 0.0, 0.0, Before), 0.0);
    assert_eq!(position(200.0, 20.0, 50.0, 0.0, Before), 50.0);
    assert_eq!(position(200.0, 20.0, 200.0, 0.0, Before), 180.0);

    assert_eq!(position(200.0, 20.0, 0.0, 0.0, After), 0.0);
    assert_eq!(position(200.0, 20.0, 50.0, 0.0, After), 30.0);
    assert_eq!(position(200.0, 20.0, 200.0, 0.0, After), 180.0);
    assert_eq!(position(200.0, 20.0, 0.0, 50.0, Before), 50.0);
    assert_eq!(position(200.0, 20.0, 50.0, 50.0, Before), 100.0);
    assert_eq!(position(200.0, 20.0, 150.0, 50.0, Before), 130.0);

    assert_eq!(position(200.0, 20.0, 0.0, 50.0, After), 50.0);
    assert_eq!(position(200.0, 20.0, 50.0, 50.0, After), 30.0);
    assert_eq!(position(200.0, 20.0, 150.0, 50.0, After), 130.0);
}
