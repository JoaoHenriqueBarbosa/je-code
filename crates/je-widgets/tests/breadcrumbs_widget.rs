//! Porte de `src/vs/base/test/browser/ui/breadcrumbs/breadcrumbsWidget.test.ts` (suíte `BreadcrumbsWidget`), caso a caso e na mesma ordem.
//!
//! Notas de tradução:
//! - O `TestBreadcrumbsItem` implementa `BreadcrumbsItem`; `container.textContent = this.label` é um nó de texto
//!   com a largura medida (o widget posiciona os filhos do item). `other instanceof TestBreadcrumbsItem` é o
//!   `downcast_ref` de `as_any`.
//! - O `measure` injetado do teste é o [`MeasureFn`] (sem a janela: o `je-ui` tem uma só).
//! - `Codicon.chevronRight` é o `ThemeIcon` de id `chevron-right`.

mod support;

use std::any::Any;
use std::cell::RefCell;
use std::rc::Rc;

use je_base::lifecycle::to_disposable;
use je_base::themables::ThemeIcon;
use je_render::Rgba;
use je_render::kurbo::Rect;
use je_ui::{NodeId, Role, Ui};
use je_widgets::breadcrumbs_widget::{BreadcrumbsItem, BreadcrumbsItemRef, BreadcrumbsWidget, IBreadcrumbsWidgetStyles, MeasureFn};
use je_widgets::dom::create_el;
use je_widgets::text::{UI_FONT_SIZE, natural_line_height, set_node_text, ui_font};
use support::ensure_no_disposables_are_leaked_in_test_suite;

struct TestBreadcrumbsItem {
    label: String,
}

impl TestBreadcrumbsItem {
    fn item(label: &str) -> BreadcrumbsItemRef {
        Rc::new(Self { label: label.to_owned() })
    }
}

impl BreadcrumbsItem for TestBreadcrumbsItem {
    fn dispose(&self) {}

    fn equals(&self, other: &dyn BreadcrumbsItem) -> bool {
        other.as_any().downcast_ref::<TestBreadcrumbsItem>().is_some_and(|other| other.label == self.label)
    }

    fn render(&self, ui: &Ui, container: NodeId, foreground: Rgba) {
        let mut tree = ui.tree_mut();
        let node = create_el(&mut tree, container, Role::Generic, "");
        let font = ui_font(UI_FONT_SIZE, 400);
        let height = natural_line_height(&font);
        let width = set_node_text(&mut tree, node, &self.label, &font, foreground, height);
        tree.set_bounds(node, Rect::new(0.0, 0.0, width, height));
    }

    fn as_any(&self) -> &dyn Any {
        self
    }
}

#[test]
fn defers_non_focus_reveals_and_cancels_them_when_focus_changes() {
    let store = ensure_no_disposables_are_leaked_in_test_suite();
    let operations: Rc<RefCell<Vec<&'static str>>> = Rc::new(RefCell::new(Vec::new()));
    let measure: MeasureFn = {
        let operations = operations.clone();
        Rc::new(move |_callback: Box<dyn FnOnce()>| {
            operations.borrow_mut().push("schedule");
            let operations = operations.clone();
            to_disposable(move || operations.borrow_mut().push("cancel"))
        })
    };
    let ui = Ui::new();
    let container = {
        let mut tree = ui.tree_mut();
        let root = tree.root();
        create_el(&mut tree, root, Role::Generic, "")
    };
    {
        let ui = ui.clone();
        store.add(to_disposable(move || ui.tree_mut().remove(container)));
    }
    let widget = store.add_rc(BreadcrumbsWidget::new(&ui, container, 3.0, None, ThemeIcon::from_id("chevron-right"), IBreadcrumbsWidgetStyles::default(), Some(measure)));
    let first = TestBreadcrumbsItem::item("first");
    let last = TestBreadcrumbsItem::item("last");
    widget.set_items(vec![first.clone(), last]);

    widget.reveal_last();
    widget.reveal(&first);
    widget.dom_focus();

    assert_eq!(*operations.borrow(), ["schedule", "cancel", "schedule", "cancel"]);
}
