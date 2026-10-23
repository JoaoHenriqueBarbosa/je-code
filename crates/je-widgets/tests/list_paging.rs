//! Testes de `src/vs/base/browser/ui/list/listPaging.ts` (sem teste no VS Code: casos novos, não é porte).

mod support;

use std::any::Any;
use std::cell::RefCell;
use std::rc::Rc;

use je_base::cancellation::CancellationToken;
use je_base::lifecycle::IDisposable;
use je_base::event::{Emitter, Event};
use je_base::paging::IPagedModel;
use je_base::promise::{Promise, Resolver};
use je_ui::{NodeId, Ui};
use je_widgets::list::list_paging::{IPagedRenderer, PagedList, PagedListOptions};
use je_widgets::list::list_types::{IListRenderer, IListVirtualDelegate, ListElementRenderDetails, TemplateData};
use support::{create_container, ensure_no_disposables_are_leaked_in_test_suite, run_with_faked_timers};

struct Delegate;

impl IListVirtualDelegate<usize> for Delegate {
    fn get_height(&self, _: &usize) -> f64 {
        20.0
    }

    fn get_template_id(&self, _: &usize) -> String {
        "template".to_owned()
    }
}

/// Registra o que foi desenhado: `placeholder:<índice>` e `element:<índice>:<valor>`.
struct Renderer {
    log: Rc<RefCell<Vec<String>>>,
}

impl IListRenderer<String> for Renderer {
    fn template_id(&self) -> String {
        "template".to_owned()
    }

    fn render_template(&self, _container: NodeId) -> TemplateData {
        Rc::new(()) as Rc<dyn Any>
    }

    fn render_element(&self, element: &String, index: usize, _: &TemplateData, _: Option<&ListElementRenderDetails>) {
        self.log.borrow_mut().push(format!("element:{index}:{element}"));
    }

    fn dispose_template(&self, _: &TemplateData) {}
}

impl IPagedRenderer<String> for Renderer {
    fn render_placeholder(&self, index: usize, _: &TemplateData) {
        self.log.borrow_mut().push(format!("placeholder:{index}"));
    }
}

/// Um modelo em que cada índice está resolvido (`Some`) ou pendente (`None`, resolve na mão).
struct Model {
    elements: RefCell<Vec<(String, bool)>>,
    pending: RefCell<Vec<(usize, CancellationToken, Resolver<String>)>>,
    on_did_increment_length: Rc<Emitter<usize>>,
}

impl Model {
    fn new(elements: &[(&str, bool)]) -> Rc<Self> {
        Rc::new(Self { elements: RefCell::new(elements.iter().map(|(v, r)| ((*v).to_owned(), *r)).collect()), pending: RefCell::default(), on_did_increment_length: Emitter::new() })
    }

    fn grow(&self, more: &[(&str, bool)]) {
        self.elements.borrow_mut().extend(more.iter().map(|(v, r)| ((*v).to_owned(), *r)));
        self.on_did_increment_length.fire(self.elements.borrow().len());
    }

    /// Resolve a promessa pendente do índice.
    fn settle(&self, index: usize) {
        let value = self.elements.borrow()[index].0.clone();
        let resolvers: Vec<_> = self.pending.borrow().iter().filter(|(i, ..)| *i == index).map(|(.., r)| r.clone()).collect();
        for resolver in resolvers {
            resolver.resolve(value.clone());
        }
    }
}

impl IPagedModel<String> for Model {
    fn length(&self) -> usize {
        self.elements.borrow().len()
    }

    fn on_did_increment_length(&self) -> Event<usize> {
        self.on_did_increment_length.event()
    }

    fn is_resolved(&self, index: usize) -> bool {
        self.elements.borrow()[index].1
    }

    fn get(&self, index: usize) -> String {
        self.elements.borrow()[index].0.clone()
    }

    fn resolve(&self, index: usize, token: CancellationToken) -> Promise<String> {
        let (promise, resolver) = Promise::new();
        let rejecting = resolver.clone();
        let listener = token.on_cancellation_requested().listen(move |_| rejecting.reject(je_base::errors::Error::cancellation()));
        self.pending.borrow_mut().push((index, token, resolver));
        promise.finally(move || listener.dispose())
    }
}

fn setup(ui: &Ui, log: &Rc<RefCell<Vec<String>>>) -> PagedList<String> {
    let container = create_container(ui, 200.0, 60.0);
    let root = ui.root();
    ui.tree_mut().append_child(root, container);
    let list = PagedList::new("test", ui, container, Rc::new(Delegate), vec![Rc::new(Renderer { log: log.clone() })], PagedListOptions::default());
    list.widget().layout(Some(60.0), Some(200.0));
    list
}

/// O que foi desenhado, em ordem crescente de índice (a lista insere as linhas do fim para o começo).
fn sorted(log: &Rc<RefCell<Vec<String>>>) -> Vec<String> {
    let mut entries = log.borrow().clone();
    entries.sort();
    entries
}

#[test]
fn renders_resolved_elements_directly() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    run_with_faked_timers(async move {
        let ui = Ui::new();
        let log = Rc::default();
        let list = Rc::new(setup(&ui, &log));
        guard.add_rc(list.clone());

        list.set_model(Model::new(&[("a", true), ("b", true), ("c", true)]));

        assert_eq!(list.widget().length(), 3);
        assert_eq!(sorted(&log), ["element:0:a", "element:1:b", "element:2:c"]);
    });
}

#[test]
fn renders_placeholder_then_element_when_the_promise_resolves() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    run_with_faked_timers(async move {
        let ui = Ui::new();
        let log = Rc::default();
        let list = Rc::new(setup(&ui, &log));
        guard.add_rc(list.clone());
        let model = Model::new(&[("a", true), ("b", false)]);

        list.set_model(model.clone());
        assert_eq!(sorted(&log), ["element:0:a", "placeholder:1"]);

        model.settle(1);
        je_base::runtime::run_microtasks();
        assert_eq!(sorted(&log), ["element:0:a", "element:1:b", "placeholder:1"]);
        assert_eq!(log.borrow().last().map(String::as_str), Some("element:1:b"));
    });
}

#[test]
fn model_growth_extends_the_list() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    run_with_faked_timers(async move {
        let ui = Ui::new();
        let log = Rc::default();
        let list = Rc::new(setup(&ui, &log));
        guard.add_rc(list.clone());
        let model = Model::new(&[("a", true)]);

        list.set_model(model.clone());
        assert_eq!(list.widget().length(), 1);

        model.grow(&[("b", true), ("c", true)]);
        assert_eq!(list.widget().length(), 3);
        assert_eq!(sorted(&log), ["element:0:a", "element:1:b", "element:2:c"]);

        // Trocar o modelo solta o ouvinte do anterior.
        let other = Model::new(&[("x", true)]);
        list.set_model(other);
        model.grow(&[("d", true)]);
        assert_eq!(list.widget().length(), 1);
    });
}

#[test]
fn reusing_the_row_cancels_the_pending_resolution() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    run_with_faked_timers(async move {
        let ui = Ui::new();
        let log = Rc::default();
        let list = setup(&ui, &log);
        let model = Model::new(&[("a", false), ("b", false), ("c", false), ("d", false), ("e", false), ("f", false)]);

        list.set_model(model.clone());
        assert_eq!(sorted(&log), ["placeholder:0", "placeholder:1", "placeholder:2"]);
        let cancelled = |model: &Model| -> Vec<usize> {
            let mut indexes: Vec<usize> = model.pending.borrow().iter().filter(|(_, token, _)| token.is_cancellation_requested()).map(|(i, ..)| *i).collect();
            indexes.sort_unstable();
            indexes
        };
        assert_eq!(cancelled(&model), Vec::<usize>::new());

        // Rolar três linhas reaproveita as três linhas do topo para os índices 3 a 5: renderizar de novo
        // a linha cancela a resolução que ela esperava.
        list.widget().set_scroll_top(60.0);
        assert_eq!(cancelled(&model), [0, 1, 2]);
        assert_eq!(sorted(&log).len(), 6);

        // A resolução cancelada não desenha nada; a que continua pendente desenha.
        model.settle(0);
        model.settle(3);
        je_base::runtime::run_microtasks();
        assert!(!log.borrow().iter().any(|e| e == "element:0:a"));
        assert!(log.borrow().iter().any(|e| e == "element:3:d"));

        // Descartar a lista (`disposeTemplate`) cancela o que ainda esperava.
        list.dispose();
        assert_eq!(cancelled(&model), [0, 1, 2, 3, 4, 5]);
    });
}
