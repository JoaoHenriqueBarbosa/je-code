//! Porte de `src/vs/workbench/contrib/testing/test/browser/testObjectTree.ts`: o `TestObjectTree`, o `ByLabelTreeSorter` e o
//! `TestTreeTestHarness`, que os testes das projeções do explorador usam (mais o `upcastPartial<ITestResultService>`
//! que cada suíte monta).
//!
//! Divergências:
//! - **DOM.** O `element` (div de 200x1000 compartilhado pelo módulo) é um contêiner novo por árvore, sem pai. O
//!   `textContent` é o `label` do nó `monaco-tl-contents` (`profundidade:rótulo`), e a consulta
//!   `.monaco-tl-contents` ordenada pelo `aria-posinset` da linha é a das linhas `monaco-list-row` ordenadas pelo topo.
//! - **`Object.assign(container.dataset, element)`** guarda só o que os testes leem, o `state` do item (como texto), por
//!   contêiner; `getRendered('state')` lê dali, e qualquer outra propriedade devolve `data` ausente. O elemento de erro
//!   não tem `state`.
//! - **`serializer`** de um erro (`t.message.toString()`) é a `description` da mensagem (para o texto é o próprio texto).
//! - **`onFolderChange`** (um `Emitter<IWorkspaceFoldersChangeEvent>` que nada dispara nem ouve) não entra.
//! - **`c = testStubs.nested()`** é o `c` opcional do construtor; `harness.flush()` devolve a árvore serializada.
//! - **`upcastPartial<ITestResultService>({ results, onResultsChanged, onTestChanged, getStateById })`** é o
//!   [`PartialTestResultService`]: o `getStateById` é um fecho trocável (o teste o reatribui) e todo outro membro lança
//!   `X is not a function`.
#![allow(dead_code)]

use std::any::Any;
use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::ops::Deref;
use std::rc::Rc;

use je_base::collator::locale_compare;
use je_base::errors::{Error, throw};
use je_base::event::{Emitter, Event};
use je_base::filters::FuzzyScore;
use je_base::lifecycle::{Disposable, DisposableStore, IDisposable};
use je_ui::{NodeId, Role, Ui};
use je_widgets::list::list_types::{IListVirtualDelegate, TemplateData};
use je_widgets::tree::abstract_tree::AbstractTreeOptions;
use je_widgets::tree::index_tree_model::TreeNode;
use je_widgets::tree::object_tree::{ObjectTree, ObjectTreeOptions};
use je_widgets::tree::object_tree_model::ObjectTreeModel;
use je_widgets::tree::tree_types::{ITreeRenderer, ITreeSorter, TreeElementRenderDetails};
use je_workbench::contrib::testing::browser::explorer_projections::index::{ITestTreeProjection, TestExplorerTreeElement};
use je_workbench::contrib::testing::common::main_thread_test_collection::MainThreadTestCollection;
use je_workbench::contrib::testing::common::test_result::{ITestResult, LiveTestResult, TestResultItemChange, TestResultItemRef};
use je_workbench::contrib::testing::common::test_result_service::{ITestResultService, ResultChangeEvent, RunTestsRequest};
use je_workbench::contrib::testing::common::test_service::{IMainThreadTestCollection, ITestService};
use je_workbench::contrib::testing::common::test_types::TestsDiffOp;

use super::test_service_mocks::MockTestService;
use super::test_stubs::{self, DEFAULT_ID_PREFIX, TestTestCollection, TestUriIdentityService};

type Model = ObjectTreeModel<TestExplorerTreeElement, FuzzyScore>;

/// `type SerializedTree = { e: string; children?: SerializedTree[]; data?: string }`.
#[derive(Clone, Debug, PartialEq)]
pub struct SerializedTree {
    pub e: String,
    pub children: Option<Vec<SerializedTree>>,
    pub data: Option<String>,
}

/// `{ e }`.
pub fn leaf(e: &str) -> SerializedTree {
    SerializedTree { e: e.to_string(), children: None, data: None }
}

/// `{ e, children }`.
pub fn node(e: &str, children: Vec<SerializedTree>) -> SerializedTree {
    SerializedTree { e: e.to_string(), children: Some(children), data: None }
}

/// `{ e, data }`.
pub fn leaf_with_data(e: &str, data: &str) -> SerializedTree {
    SerializedTree { e: e.to_string(), children: None, data: Some(data.to_string()) }
}

/// `serializer`: `t => 'test' in t ? t.test.item.label : t.message.toString()`.
fn serialize_element(element: &TestExplorerTreeElement) -> String {
    match element {
        TestExplorerTreeElement::Item(item) => item.test.borrow().item.label.clone(),
        TestExplorerTreeElement::Error(error) => error.description(),
    }
}

struct Delegate;

impl IListVirtualDelegate<TestExplorerTreeElement> for Delegate {
    fn get_height(&self, _: &TestExplorerTreeElement) -> f64 {
        20.0
    }

    fn get_template_id(&self, _: &TestExplorerTreeElement) -> String {
        "default".to_string()
    }
}

/// `{ container, store }` do `renderTemplate`.
struct RowTemplate {
    container: NodeId,
    store: Rc<DisposableStore>,
}

struct TestTreeRenderer {
    ui: Ui,
    serializer: Rc<dyn Fn(&TestExplorerTreeElement) -> String>,
    dataset: Rc<RefCell<HashMap<NodeId, String>>>,
}

impl ITreeRenderer<Model> for TestTreeRenderer {
    fn template_id(&self) -> String {
        "default".to_string()
    }

    fn render_template(&self, container: NodeId) -> TemplateData {
        Rc::new(RowTemplate { container, store: DisposableStore::new() }) as Rc<dyn Any>
    }

    fn render_element(&self, node: &Rc<TreeNode<Option<TestExplorerTreeElement>, FuzzyScore>>, _index: usize, template_data: &TemplateData, _details: Option<&TreeElementRenderDetails>) {
        let template = template_data.downcast_ref::<RowTemplate>().expect("template");
        let element = node.element.clone().expect("elemento");
        let depth = node.depth;
        let container = template.container;

        let render: Rc<dyn Fn()> = {
            let (ui, serializer, dataset, element) = (self.ui.clone(), self.serializer.clone(), self.dataset.clone(), element.clone());
            Rc::new(move || {
                ui.tree_mut().set_label(container, format!("{}:{}", depth, serializer(&element)));
                match &element {
                    TestExplorerTreeElement::Item(item) => {
                        dataset.borrow_mut().insert(container, (item.state.get() as u8).to_string());
                    }
                    TestExplorerTreeElement::Error(_) => {
                        dataset.borrow_mut().remove(&container);
                    }
                }
            })
        };
        render();

        if let TestExplorerTreeElement::Item(item) = &element {
            template.store.add(item.on_change().listen(move |_| render()));
        }
    }

    fn dispose_element(&self, _node: &Rc<TreeNode<Option<TestExplorerTreeElement>, FuzzyScore>>, _index: i64, template_data: &TemplateData, _details: Option<&TreeElementRenderDetails>) {
        template_data.downcast_ref::<RowTemplate>().expect("template").store.clear();
    }

    fn dispose_template(&self, template_data: &TemplateData) {
        template_data.downcast_ref::<RowTemplate>().expect("template").store.dispose();
    }
}

/// `sorter ?? { compare: (a, b) => serializer(a).localeCompare(serializer(b)) }`.
struct SerializerSorter {
    serializer: Rc<dyn Fn(&TestExplorerTreeElement) -> String>,
}

impl ITreeSorter<TestExplorerTreeElement> for SerializerSorter {
    fn compare(&self, a: &TestExplorerTreeElement, b: &TestExplorerTreeElement) -> i32 {
        locale_compare(&(self.serializer)(a), &(self.serializer)(b)) as i32
    }
}

pub struct TestObjectTree {
    ui: Ui,
    container: NodeId,
    tree: Rc<ObjectTree<TestExplorerTreeElement, FuzzyScore>>,
    dataset: Rc<RefCell<HashMap<NodeId, String>>>,
}

impl TestObjectTree {
    pub fn new(serializer: Rc<dyn Fn(&TestExplorerTreeElement) -> String>, sorter: Option<Rc<dyn ITreeSorter<TestExplorerTreeElement>>>) -> Rc<Self> {
        let ui = Ui::new();
        let container = {
            let mut tree = ui.tree_mut();
            let container = tree.create_detached(Role::Generic);
            tree.set_bounds(container, je_render::kurbo::Rect::new(0.0, 0.0, 200.0, 1000.0));
            container
        };
        let dataset: Rc<RefCell<HashMap<NodeId, String>>> = Rc::default();
        let renderer = Rc::new(TestTreeRenderer { ui: ui.clone(), serializer: serializer.clone(), dataset: dataset.clone() });
        let sorter = sorter.unwrap_or_else(|| Rc::new(SerializerSorter { serializer }));

        let tree = ObjectTree::new("test", &ui, container, Rc::new(Delegate), vec![renderer], ObjectTreeOptions { tree: AbstractTreeOptions::default(), sorter: Some(sorter) });
        tree.layout(Some(1000.0), Some(200.0));

        Rc::new(Self { ui, container, tree, dataset })
    }

    /// `getRendered(getProperty?)`: a árvore desenhada, na ordem das linhas.
    pub fn get_rendered(&self, get_property: Option<&str>) -> Vec<SerializedTree> {
        let tree = self.ui.tree();
        let descendants_with_class = |root: NodeId, class: &str| -> Vec<NodeId> { tree.subtree(root).into_iter().skip(1).filter(|n| tree.has_class(*n, class)).collect() };

        let mut rows = descendants_with_class(self.container, "monaco-list-row");
        rows.sort_by(|a, b| tree.bounds(*a).y0.total_cmp(&tree.bounds(*b).y0));

        let mut stack: Vec<SerializedTree> = vec![SerializedTree { e: String::new(), children: Some(Vec::new()), data: None }];
        let attach = |stack: &mut Vec<SerializedTree>| {
            let done = stack.pop().expect("pilha não vazia");
            stack.last_mut().expect("raiz").children.get_or_insert_with(Vec::new).push(done);
        };

        for row in rows {
            let Some(contents) = descendants_with_class(row, "monaco-tl-contents").first().copied() else { continue };
            let text = tree.label(contents).unwrap_or_default();
            let mut parts = text.split(':');
            let depth: usize = parts.next().and_then(|d| d.parse().ok()).expect("profundidade");
            let label = parts.next().unwrap_or_default().to_string();
            let data = match get_property {
                Some("state") => self.dataset.borrow().get(&contents).cloned(),
                _ => None,
            };

            while stack.len() > depth {
                attach(&mut stack);
            }
            stack.push(SerializedTree { e: label, children: None, data });
        }

        while stack.len() > 1 {
            attach(&mut stack);
        }

        stack.pop().and_then(|root| root.children).unwrap_or_default()
    }
}

impl Deref for TestObjectTree {
    type Target = ObjectTree<TestExplorerTreeElement, FuzzyScore>;

    fn deref(&self) -> &Self::Target {
        &self.tree
    }
}

impl IDisposable for TestObjectTree {
    fn dispose(&self) {
        self.tree.dispose();
    }
}

/// `ByLabelTreeSorter`.
pub struct ByLabelTreeSorter;

impl ITreeSorter<TestExplorerTreeElement> for ByLabelTreeSorter {
    fn compare(&self, a: &TestExplorerTreeElement, b: &TestExplorerTreeElement) -> i32 {
        let (a, b) = match (a, b) {
            (TestExplorerTreeElement::Item(a), TestExplorerTreeElement::Item(b)) => (a, b),
            _ => {
                let is_error = |e: &TestExplorerTreeElement| matches!(e, TestExplorerTreeElement::Error(_));
                return (if is_error(a) { -1 } else { 0 }) + (if is_error(b) { 1 } else { 0 });
            }
        };

        let (a, b) = (a.test.borrow(), b.test.borrow());
        if let (Some(a_uri), Some(b_uri), Some(a_range), Some(b_range)) = (&a.item.uri, &b.item.uri, &a.item.range, &b.item.range)
            && a_uri.to_string(false) == b_uri.to_string(false)
        {
            let delta = a_range.start_line_number - b_range.start_line_number;
            if delta != 0 {
                return delta;
            }
        }

        let key = |item: &je_workbench::contrib::testing::common::test_types::ITestItem| match &item.sort_text {
            Some(sort_text) if !sort_text.is_empty() => sort_text.clone(),
            _ => item.label.clone(),
        };
        locale_compare(&key(&a.item), &key(&b.item)) as i32
    }
}

/// `names are hard`.
pub struct TestTreeTestHarness<T: ITestTreeProjection + 'static> {
    base: Rc<Disposable>,
    on_diff: Rc<Emitter<Rc<Vec<TestsDiffOp>>>>,
    is_processing_diff: Rc<Cell<bool>>,
    pub projection: Rc<T>,
    pub tree: Rc<TestObjectTree>,
    pub c: Rc<TestTestCollection>,
}

impl<T: ITestTreeProjection + 'static> TestTreeTestHarness<T> {
    pub fn new(make_tree: impl FnOnce(Rc<dyn ITestService>) -> Rc<T>, c: Option<Rc<TestTestCollection>>) -> Self {
        let c = c.unwrap_or_else(|| test_stubs::nested(DEFAULT_ID_PREFIX));
        let base = Disposable::new();
        let on_diff: Rc<Emitter<Rc<Vec<TestsDiffOp>>>> = Emitter::new();
        base.register(on_diff.clone());
        let is_processing_diff = Rc::new(Cell::new(false));

        base.register(c.clone());
        base.register(c.on_did_generate_diff().listen({
            let c = c.clone();
            move |d| c.set_diff(d.clone()) /* don't clear during testing */
        }));

        let collection = Rc::new(MainThreadTestCollection::new(
            Rc::new(TestUriIdentityService),
            Rc::new({
                let (c, on_diff, is_processing_diff) = (c.clone(), on_diff.clone(), is_processing_diff.clone());
                move |test_id: &str, levels: f64| {
                    let _ = c.expand(test_id, levels);
                    if !is_processing_diff.get() {
                        on_diff.fire(Rc::new(c.collect_diff()));
                    }
                    je_base::promise::Promise::resolved(())
                }
            }),
        ));
        base.register(on_diff.event().listen({
            let collection = collection.clone();
            move |diff| collection.apply(diff)
        }));

        let service = Rc::new(MockTestService::default());
        *service.collection.borrow_mut() = Some(collection.clone() as Rc<dyn IMainThreadTestCollection>);
        *service.on_did_process_diff.borrow_mut() = Some(on_diff.event());
        let projection = make_tree(service);
        base.register(projection.clone());

        let tree = TestObjectTree::new(Rc::new(serialize_element), Some(Rc::new(ByLabelTreeSorter)));
        base.register(tree.clone());
        base.register(tree.on_did_change_collapse_state().listen({
            let projection = projection.clone();
            move |evt| {
                if let Some(item) = evt.node.element.as_ref().and_then(|element| element.as_item()) {
                    projection.expand_element(item, if evt.deep { f64::INFINITY } else { 0.0 });
                }
            }
        }));

        Self { base, on_diff, is_processing_diff, projection, tree, c }
    }

    pub fn push_diff(&self, diff: Vec<TestsDiffOp>) {
        self.on_diff.fire(Rc::new(diff));
    }

    pub fn flush(&self) -> Vec<SerializedTree> {
        self.is_processing_diff.set(true);
        while !self.c.current_diff().is_empty() {
            self.on_diff.fire(Rc::new(self.c.collect_diff()));
        }
        self.is_processing_diff.set(false);

        self.projection.apply_to(&self.tree);
        self.tree.get_rendered(None)
    }
}

impl<T: ITestTreeProjection + 'static> IDisposable for TestTreeTestHarness<T> {
    fn dispose(&self) {
        self.base.dispose();
    }
}

type GetStateById = Box<dyn Fn(&str) -> Option<(Rc<dyn ITestResult>, TestResultItemRef)>>;

/// `upcastPartial<ITestResultService>({ results, onResultsChanged, onTestChanged, getStateById })`.
pub struct PartialTestResultService {
    pub results: RefCell<Vec<Rc<dyn ITestResult>>>,
    pub on_results_changed: Event<ResultChangeEvent>,
    pub on_test_changed: Event<TestResultItemChange>,
    pub get_state_by_id: RefCell<GetStateById>,
}

impl PartialTestResultService {
    pub fn new(on_test_changed: Event<TestResultItemChange>) -> Rc<Self> {
        Rc::new(Self {
            results: RefCell::new(Vec::new()),
            on_results_changed: Event::none(),
            on_test_changed,
            get_state_by_id: RefCell::new(Box::new(|_| None)),
        })
    }
}

impl ITestResultService for PartialTestResultService {
    fn on_results_changed(&self) -> Event<ResultChangeEvent> {
        self.on_results_changed.clone()
    }

    fn on_test_changed(&self) -> Event<TestResultItemChange> {
        self.on_test_changed.clone()
    }

    fn results(&self) -> Vec<Rc<dyn ITestResult>> {
        self.results.borrow().clone()
    }

    fn clear(&self) {
        throw(Error::new("clear is not a function"))
    }

    fn create_live_result(&self, _req: RunTestsRequest) -> Rc<LiveTestResult> {
        throw(Error::new("createLiveResult is not a function"))
    }

    fn push(&self, _result: Rc<dyn ITestResult>) -> Rc<dyn ITestResult> {
        throw(Error::new("push is not a function"))
    }

    fn get_result(&self, _result_id: &str) -> Option<Rc<dyn ITestResult>> {
        throw(Error::new("getResult is not a function"))
    }

    fn get_state_by_id(&self, ext_id: &str) -> Option<(Rc<dyn ITestResult>, TestResultItemRef)> {
        (self.get_state_by_id.borrow())(ext_id)
    }
}
