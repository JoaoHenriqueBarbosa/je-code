//! Porte de `src/vs/base/test/browser/ui/list/listWidget.test.ts` (suíte `ListWidget`), caso a caso e na mesma ordem.
//!
//! Pendente: `aria-activedescendant references a rendered element` confere o atributo
//! `aria-activedescendant` do `.monaco-list`, que o `je-ui` não tem (só `role` e `label`); o que se pode
//! conferir sem o atributo (o nó do elemento focado existir antes e depois do `reveal`) está no teste.
//! Pendente: `styles a .hovered row like a pointer-hovered row` confere o texto CSS que o
//! `DefaultStyleController` gera, e não há CSS no `je-ui` (as cores entram pelo tema nos widgets).

mod support;

use std::any::Any;
use std::cell::Cell;
use std::rc::Rc;

use je_ui::{NodeId, Ui};
use je_widgets::list::list_types::{IListRenderer, IListVirtualDelegate, ListElementRenderDetails, TemplateData};
use je_widgets::list::list_widget::{List, ListOptions};
use support::{create_container, ensure_no_disposables_are_leaked_in_test_suite, run_with_faked_timers, timeout};

struct Delegate(f64);

impl IListVirtualDelegate<i64> for Delegate {
    fn get_height(&self, _: &i64) -> f64 {
        self.0
    }

    fn get_template_id(&self, _: &i64) -> String {
        "template".to_owned()
    }
}

struct Renderer {
    templates_count: Rc<Cell<i32>>,
}

impl IListRenderer<i64> for Renderer {
    fn template_id(&self) -> String {
        "template".to_owned()
    }

    fn render_template(&self, _container: NodeId) -> TemplateData {
        self.templates_count.set(self.templates_count.get() + 1);
        Rc::new(()) as Rc<dyn Any>
    }

    fn render_element(&self, _: &i64, _: usize, _: &TemplateData, _: Option<&ListElementRenderDetails>) {}

    fn dispose_template(&self, _: &TemplateData) {
        self.templates_count.set(self.templates_count.get() - 1);
    }
}

fn setup(ui: &Ui, height: f64, item_height: f64, templates_count: &Rc<Cell<i32>>) -> Rc<List<i64>> {
    let element = create_container(ui, 200.0, height);
    let root = ui.root();
    ui.tree_mut().append_child(root, element);
    List::new("test", ui, element, Rc::new(Delegate(item_height)), vec![Rc::new(Renderer { templates_count: templates_count.clone() })], ListOptions::default())
}

#[test]
fn page_up_and_down() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    run_with_faked_timers(async move {
        let ui = Ui::new();
        let templates_count = Rc::new(Cell::new(0));
        let list_widget = guard.add_rc(setup(&ui, 200.0, 20.0, &templates_count));

        list_widget.layout(Some(200.0), None);
        assert_eq!(templates_count.get(), 0, "no templates have been allocated");
        list_widget.splice(0, 0, &(0..100).collect::<Vec<i64>>());
        list_widget.focus_first(None, None);

        list_widget.focus_next_page(None, None);
        assert_eq!(list_widget.get_focus()[0], 9, "first page down moves focus to element at bottom");

        // scroll to next page is async
        list_widget.focus_next_page(None, None);
        timeout(0).await.unwrap();
        assert_eq!(list_widget.get_focus()[0], 19, "page down to next page");

        list_widget.focus_previous_page(None, None, Rc::new(|| 0.0));
        assert_eq!(list_widget.get_focus()[0], 10, "first page up moves focus to element at top");

        // scroll to previous page is async
        list_widget.focus_previous_page(None, None, Rc::new(|| 0.0));
        timeout(0).await.unwrap();
        assert_eq!(list_widget.get_focus()[0], 0, "page down to previous page");
    });
}

#[test]
fn page_up_and_down_with_item_taller_than_viewport_149502() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    run_with_faked_timers(async move {
        let ui = Ui::new();
        let templates_count = Rc::new(Cell::new(0));
        let list_widget = guard.add_rc(setup(&ui, 200.0, 200.0, &templates_count));

        list_widget.layout(Some(200.0), None);
        assert_eq!(templates_count.get(), 0, "no templates have been allocated");
        list_widget.splice(0, 0, &(0..100).collect::<Vec<i64>>());
        list_widget.focus_first(None, None);
        assert_eq!(list_widget.get_focus()[0], 0, "initial focus is first element");

        // scroll to next page is async
        list_widget.focus_next_page(None, None);
        timeout(0).await.unwrap();
        assert_eq!(list_widget.get_focus()[0], 1, "page down to next page");

        // scroll to previous page is async
        list_widget.focus_previous_page(None, None, Rc::new(|| 0.0));
        timeout(0).await.unwrap();
        assert_eq!(list_widget.get_focus()[0], 0, "page up to next page");
    });
}

#[test]
fn aria_activedescendant_references_a_rendered_element() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let ui = Ui::new();
    let templates_count = Rc::new(Cell::new(0));
    let list_widget = guard.add_rc(setup(&ui, 20.0, 20.0, &templates_count));
    list_widget.layout(Some(20.0), None);
    list_widget.splice(0, 0, &(0..100).collect::<Vec<i64>>());
    ui.run_animation_frame();

    // O nó do elemento 50 (`element.querySelector('#id')`) existe só quando a linha está renderizada.
    let rendered = |list: &List<i64>| list.view.dom_element(50).is_some();

    list_widget.set_focus(vec![50], None);
    let before_reveal = rendered(&list_widget);

    list_widget.reveal(50, None, 0.0);
    let after_reveal = rendered(&list_widget);

    assert!(!before_reveal);
    assert!(after_reveal);
    assert_eq!(list_widget.get_element_id(50), format!("{}_50", list_widget.dom_id()));
}
