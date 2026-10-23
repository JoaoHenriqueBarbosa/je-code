//! Porte de `src/vs/base/test/browser/ui/list/listView.test.ts` (suíte `ListView`), caso a caso e na mesma ordem.
//!
//! Adaptado: `recovers when rendering collapses the viewport (horizontal: false, dynamic: false|true)` perde
//! o `dnd` do original (`draggable`, `getDragURI` e a lista `dragURIs`: arrastar e soltar não existe no
//! `ListView`); o resto do caso (a reentrada de `layout(0, ...)` dentro de `renderElement`, as linhas
//! renderizadas e descartadas, a contagem de templates) está igual. O `textContent` da linha é o `label` do nó.
//!
//! Pendente: `recovers when rendering collapses the viewport (horizontal: true, dynamic: false)` precisa de
//! `horizontalScrolling`, que o `ListView` não tem (mede a largura de conteúdo dos nós; `content_width` é 0).
//! Pendente: `batches horizontal width measurements` (`horizontalScrolling` e `offsetWidth`).
//! Pendente: `batches dynamic height measurements`, `preserves existing rows while measuring overestimated
//! appended rows`, `reuses a bounded row pool while measuring appended rows`, `cleans up retained dynamic
//! height rows after a render error`, `does not promote a retained row after a reentrant splice`,
//! `handles removing all items during dynamic height measurement` e `publishes freshly measured dynamic
//! heights`: todos dependem de medir a caixa da linha (`offsetHeight`, ou o `style.height` do conteúdo
//! dentro dela) e o `je-ui` não tem layout intrínseco; o `probe_dynamic_heights` do `ListView` só usa a
//! altura que o delegate informa (`// TODO(integração)` em `list_view.rs`).
//! Pendente: `updates data-index when reusing dynamic height measurement rows` (mede `offsetHeight` e usa
//! `onContextMenu`, que o `ListView` não tem).
//! Pendente: `preserves delegated height measurements after a reentrant replacement` (mede `offsetHeight` e
//! sobrescreve o `getRenderRange` protegido numa subclasse; o `ListView` não tem ponto de extensão).
//! Pendente: `preserves offscreen rows when extending user selection with shift click` (`userSelection` e
//! seleção de texto do documento).

mod support;

use std::any::Any;
use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::rc::Rc;

use je_base::lifecycle::IDisposable;
use je_ui::{NodeId, Ui};
use je_widgets::list::list_types::{IListRenderer, IListVirtualDelegate, ListElementRenderDetails, TemplateData};
use je_widgets::list::list_view::{ListView, ListViewOptions};
use support::{create_container, ensure_no_disposables_are_leaked_in_test_suite, query_all};

/// `{ getHeight() { return 20; }, getTemplateId() { return 'template'; } }`, com `getDynamicHeight` opcional.
struct Delegate {
    height: f64,
    dynamic_height: Option<f64>,
}

impl IListVirtualDelegate<i64> for Delegate {
    fn get_height(&self, _: &i64) -> f64 {
        self.height
    }

    fn get_template_id(&self, _: &i64) -> String {
        "template".to_owned()
    }

    fn get_dynamic_height(&self, _: &i64) -> Option<f64> {
        self.dynamic_height
    }
}

type ListViewRef = Rc<RefCell<Option<Rc<ListView<i64>>>>>;

/// O renderer dos dois primeiros casos: o template é o contêiner da linha (o `renderTemplate(container) {
/// return container; }` do original), o `textContent` é o `label`, e `collapse_on_render` faz o primeiro
/// `renderElement` encolher o viewport de dentro da renderização.
struct Renderer {
    ui: Ui,
    list_view: ListViewRef,
    templates: Rc<Cell<i32>>,
    rendered: Rc<RefCell<Vec<i64>>>,
    disposed: Rc<RefCell<Vec<i64>>>,
    collapse_on_render: Rc<Cell<bool>>,
}

impl IListRenderer<i64> for Renderer {
    fn template_id(&self) -> String {
        "template".to_owned()
    }

    fn render_template(&self, container: NodeId) -> TemplateData {
        self.templates.set(self.templates.get() + 1);
        Rc::new(container) as Rc<dyn Any>
    }

    fn render_element(&self, value: &i64, _index: usize, template_data: &TemplateData, _: Option<&ListElementRenderDetails>) {
        let container = *template_data.downcast_ref::<NodeId>().expect("template de nó");
        self.rendered.borrow_mut().push(*value);
        self.ui.tree_mut().set_label(container, value.to_string());
        if self.collapse_on_render.replace(false) {
            let list_view = self.list_view.borrow().clone().expect("a lista já existe");
            list_view.layout(Some(0.0), Some(200.0));
        }
    }

    fn dispose_element(&self, value: &i64, _: i64, _: &TemplateData, _: Option<&ListElementRenderDetails>) {
        self.disposed.borrow_mut().push(*value);
    }

    fn dispose_template(&self, _: &TemplateData) {
        self.templates.set(self.templates.get() - 1);
    }
}

fn text_of(ui: &Ui, node: NodeId) -> Option<String> {
    ui.tree().label(node).map(str::to_owned)
}

fn row_count(ui: &Ui, container: NodeId) -> usize {
    query_all(&ui.tree(), container, &["monaco-list-row"]).len()
}

fn recovers_when_rendering_collapses_the_viewport(support_dynamic_heights: bool) {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let ui = Ui::new();
    let element = create_container(&ui, 0.0, 0.0);
    let delegate = Delegate { height: 20.0, dynamic_height: Some(20.0) };
    let (rendered, disposed) = (Rc::new(RefCell::new(Vec::new())), Rc::new(RefCell::new(Vec::new())));
    let templates = Rc::new(Cell::new(0));
    let list_view_ref: ListViewRef = Rc::default();
    let renderer = Renderer { ui: ui.clone(), list_view: list_view_ref.clone(), templates: templates.clone(), rendered: rendered.clone(), disposed: disposed.clone(), collapse_on_render: Rc::new(Cell::new(true)) };
    let list_view = guard.add_rc(ListView::new(&ui, element, Rc::new(delegate), vec![Rc::new(renderer)], ListViewOptions { support_dynamic_heights, ..Default::default() }));
    *list_view_ref.borrow_mut() = Some(list_view.clone());
    list_view.layout(Some(200.0), Some(200.0));
    list_view.splice(0, 0, &[42]);

    assert_eq!(list_view.length(), 1);
    assert_eq!(list_view.dom_element(0), None);
    assert_eq!(row_count(&ui, element), 0);
    assert_eq!(*rendered.borrow(), [42]);
    assert_eq!(*disposed.borrow(), [42]);

    list_view.layout(Some(200.0), Some(200.0));
    let row = list_view.dom_element(0).expect("a linha existe");
    assert_eq!(text_of(&ui, row).as_deref(), Some("42"));
    assert_eq!(*rendered.borrow(), [42, 42]);
    assert_eq!(templates.get(), 1);

    list_view.splice(0, 1, &[7]);
    assert_eq!(list_view.dom_element(0), Some(row), "a linha foi reaproveitada");
    assert_eq!(text_of(&ui, row).as_deref(), Some("7"));
    assert_eq!(*rendered.borrow(), [42, 42, 7]);
    assert_eq!(*disposed.borrow(), [42, 42]);

    list_view.dispose();
    assert_eq!(templates.get(), 0);
    assert_eq!(*disposed.borrow(), [42, 42, 7]);
    *list_view_ref.borrow_mut() = None;
}

#[test]
fn recovers_when_rendering_collapses_the_viewport_horizontal_false_dynamic_false() {
    recovers_when_rendering_collapses_the_viewport(false);
}

#[test]
fn recovers_when_rendering_collapses_the_viewport_horizontal_false_dynamic_true() {
    recovers_when_rendering_collapses_the_viewport(true);
}

#[test]
fn does_not_render_stale_insertion_ranges_after_a_reentrant_viewport_collapse() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let ui = Ui::new();
    let element = create_container(&ui, 0.0, 0.0);
    let list_view_ref: ListViewRef = Rc::default();
    let collapse_on_render = Rc::new(Cell::new(false));
    let renderer = Renderer {
        ui: ui.clone(),
        list_view: list_view_ref.clone(),
        templates: Rc::default(),
        rendered: Rc::default(),
        disposed: Rc::default(),
        collapse_on_render: collapse_on_render.clone(),
    };
    let list_view = guard.add_rc(ListView::new(&ui, element, Rc::new(Delegate { height: 20.0, dynamic_height: None }), vec![Rc::new(renderer)], ListViewOptions::default()));
    *list_view_ref.borrow_mut() = Some(list_view.clone());
    list_view.layout(Some(200.0), Some(200.0));
    list_view.splice(0, 0, &[1, 2, 3]);
    collapse_on_render.set(true);
    list_view.splice(0, 3, &[4, 5, 6]);
    let collapsed_rows = row_count(&ui, element);

    list_view.splice(0, 3, &[7]);
    list_view.layout(Some(200.0), Some(200.0));

    assert_eq!(collapsed_rows, 0);
    assert_eq!(list_view.length(), 1);
    let rows: Vec<Option<String>> = query_all(&ui.tree(), element, &["monaco-list-row"]).into_iter().map(|row| text_of(&ui, row)).collect();
    assert_eq!(rows, [Some("7".to_owned())]);
    *list_view_ref.borrow_mut() = None;
}

#[test]
fn all_rows_get_disposed() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let ui = Ui::new();
    let element = create_container(&ui, 200.0, 200.0);
    let templates_count = Rc::new(Cell::new(0));
    let renderer = Renderer {
        ui: ui.clone(),
        list_view: Rc::default(),
        templates: templates_count.clone(),
        rendered: Rc::default(),
        disposed: Rc::default(),
        collapse_on_render: Rc::default(),
    };

    let list_view = guard.add_rc(ListView::new(&ui, element, Rc::new(Delegate { height: 20.0, dynamic_height: None }), vec![Rc::new(renderer)], ListViewOptions::default()));
    list_view.layout(Some(200.0), None);

    assert_eq!(templates_count.get(), 0, "no templates have been allocated");
    list_view.splice(0, 0, &(0..100).collect::<Vec<i64>>());
    assert_eq!(templates_count.get(), 10, "some templates have been allocated");
    list_view.dispose();
    assert_eq!(templates_count.get(), 0, "all templates have been disposed");
}

/// `{ height: number }` do original, pelo índice: a altura do elemento `i` é `heights[i]`.
struct HeightsDelegate {
    heights: Vec<f64>,
    published: RefCell<HashMap<usize, f64>>,
}

impl IListVirtualDelegate<usize> for HeightsDelegate {
    fn get_height(&self, _: &usize) -> f64 {
        100.0
    }

    fn get_template_id(&self, _: &usize) -> String {
        "template".to_owned()
    }

    fn get_dynamic_height(&self, element: &usize) -> Option<f64> {
        Some(self.heights[*element])
    }

    fn set_dynamic_height(&self, element: &usize, height: f64) {
        self.published.borrow_mut().insert(*element, height);
    }
}

/// `renderTemplate() { }`, `renderElement() { }`, `disposeTemplate() { }`.
struct EmptyRenderer;

impl IListRenderer<usize> for EmptyRenderer {
    fn template_id(&self) -> String {
        "template".to_owned()
    }

    fn render_template(&self, _: NodeId) -> TemplateData {
        Rc::new(()) as Rc<dyn Any>
    }

    fn render_element(&self, _: &usize, _: usize, _: &TemplateData, _: Option<&ListElementRenderDetails>) {}

    fn dispose_template(&self, _: &TemplateData) {}
}

#[test]
fn publishes_positive_delegate_provided_dynamic_heights() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let ui = Ui::new();
    let element = create_container(&ui, 0.0, 0.0);
    let delegate = Rc::new(HeightsDelegate { heights: vec![0.0, 40.0, 100.0, 160.0], published: RefCell::default() });
    let list_view = guard.add_rc(ListView::new(&ui, element, delegate.clone() as Rc<dyn IListVirtualDelegate<usize>>, vec![Rc::new(EmptyRenderer)], ListViewOptions { support_dynamic_heights: true, ..Default::default() }));

    list_view.layout(Some(400.0), Some(200.0));
    list_view.splice(0, 0, &[0, 1, 2, 3]);

    let published: Vec<Option<f64>> = (0..4).map(|i| delegate.published.borrow().get(&i).copied()).collect();
    assert_eq!(published, [None, Some(40.0), Some(100.0), Some(160.0)]);
}

#[test]
fn does_not_throw_when_laid_out_with_a_collapsed_viewport_and_zero_height_dynamic_items() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let ui = Ui::new();
    let element = create_container(&ui, 0.0, 0.0);
    // Collapsing the viewport before splicing zero-height items previously yielded an inverted range that crashed probeDynamicHeights.
    let delegate = Rc::new(HeightsDelegate { heights: vec![0.0, 0.0, 0.0], published: RefCell::default() });
    let list_view = guard.add_rc(ListView::new(&ui, element, delegate as Rc<dyn IListVirtualDelegate<usize>>, vec![Rc::new(EmptyRenderer)], ListViewOptions { support_dynamic_heights: true, ..Default::default() }));

    list_view.layout(Some(0.0), Some(200.0));
    // `assert.doesNotThrow`: um pânico reprova o teste.
    list_view.splice(0, 0, &[0, 1, 2]);
}
