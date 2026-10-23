//! Porte de `src/vs/base/test/browser/ui/tree/asyncDataTree.test.ts` (suíte `AsyncDataTree`), caso a caso e na mesma ordem.
//!
//! Notas de tradução:
//! - `interface Element { id, suffix?, children? }` é `Element` com `children: RefCell<Vec<El>>` (o teste
//!   atribui `children`; o `undefined` é o vazio, o que o `!!element.children && length > 0` e o
//!   `element.children || []` do original tratam igual). Os elementos são `El` (`ByRef`): a árvore os compara
//!   por identidade de objeto, como o `===` do original. `TInput` e `T` são o mesmo `El`; a entrada vai como
//!   `InputOrElement::Input` e os elementos como `InputOrElement::Element`.
//! - `async () => { ... await ... }` é `run_with_faked_timers(async move { ... .await.unwrap() ... })`: a ordem
//!   das microtarefas é a do JavaScript (ver `async_data_tree.rs`). O teste `issue #80098 - first expand should
//!   call getChildren` já usava `runWithFakedTimers`; os demais rodam também com o relógio virtual, o que não
//!   muda a ordem (o único timer, os 800 ms do estado lento, é cancelado ao resolver os filhos).
//! - `container.style/querySelectorAll('.monaco-list-row')` é `attach` + `rows` (as linhas ordenadas pelo
//!   topo), `:first-child` é a primeira linha, `:nth-child(2)` a segunda, `textContent` o `label` do conteúdo
//!   e `classList.contains` o `has_class` do twistie. `container.innerHTML === ''` é o container sem filhos.
//! - O `Renderer` implementa `ICompressibleAsyncDataTreeRenderer` (o `ICompressibleTreeRenderer` do original) e
//!   serve às duas árvores.
//! - `getChildren` devolve `Promise.resolve(children)` ou `async` (que resolve na hora): ambos são
//!   `MaybePromise::Promise(Promise::resolved(..))`. `new Promise(c => calls.push(() => c(...)))` é um
//!   `Promise::with_executor` que guarda o fecho resolvedor.
//! - `Iterable.empty()` é o `Vec` vazio. `assert.equal(x, null)` é `is_none()`.
//! - O último teste (`#[ignore]`) é o de desempenho do `refresh` de uma pasta com 10 mil filhos, que não existe
//!   no original: `cargo test -p je-widgets --release --test async_data_tree -- --ignored --nocapture`.

mod support;

use std::any::Any;
use std::cell::{Cell, RefCell};
use std::rc::Rc;

use je_base::lifecycle::IDisposable;
use je_base::promise::{MaybePromise, Promise};
use je_ui::{NodeId, Ui};
use je_widgets::list::list_types::{IIdentityProvider, IListVirtualDelegate, TemplateData};
use je_widgets::tree::async_data_tree::{AsyncDataTree, ObjectBackend};
use je_widgets::tree::async_data_tree_node::{AsyncDataTreeNodeWrapper, AsyncDataTreeOptions, IAsyncDataTreeRenderer};
use je_widgets::tree::compressible_async_data_tree::{
    CompressibleAsyncDataTree, CompressibleAsyncDataTreeNodeWrapper, CompressibleAsyncDataTreeOptions, ICompressibleAsyncDataTreeRenderer, ITreeCompressionDelegate,
};
use je_widgets::tree::tree_types::{ByRef, IAsyncDataSource, ITreeNavigator, InputOrElement, TreeElementRenderDetails, TreeNodeApi};
use support::{create_container, ensure_no_disposables_are_leaked_in_test_suite, query_all, run_with_faked_timers, timeout};

struct Element {
    id: String,
    suffix: String,
    children: RefCell<Vec<El>>,
}

type El = ByRef<Element>;
type Input = InputOrElement<El, El>;
type Wrapper = AsyncDataTreeNodeWrapper<El, El, ()>;
type Tree = AsyncDataTree<El, El, (), ObjectBackend>;
type CompressibleTree = CompressibleAsyncDataTree<El, El>;

/// `{ id, children }`.
fn el(id: &str, children: Vec<El>) -> El {
    ByRef::new(Element { id: id.to_owned(), suffix: String::new(), children: RefCell::new(children) })
}

/// `{ id, suffix }`.
fn el_suffix(id: &str, suffix: &str) -> El {
    ByRef::new(Element { id: id.to_owned(), suffix: suffix.to_owned(), children: RefCell::default() })
}

fn input(element: &El) -> Input {
    InputOrElement::Input(element.clone())
}

fn elem(element: &El) -> Input {
    InputOrElement::Element(element.clone())
}

/// O `Element` de um `TInput | T` (os dois são o mesmo tipo).
fn el_of(element: &Input) -> &El {
    match element {
        InputOrElement::Input(element) | InputOrElement::Element(element) => element,
    }
}

/// `find`.
fn find(element: &El, id: &str) -> Option<El> {
    if element.id == id {
        return Some(element.clone());
    }

    element.children.borrow().iter().find_map(|child| find(child, id))
}

/// `Renderer`: `renderElement` põe `id + suffix` e `renderCompressedElements` os ids juntos por `/`.
struct Renderer {
    ui: Ui,
}

impl Renderer {
    fn text(&self, template_data: &TemplateData, text: String) {
        let container = *template_data.downcast_ref::<NodeId>().expect("template");
        self.ui.tree_mut().set_label(container, text);
    }
}

impl IAsyncDataTreeRenderer<El, El, ()> for Renderer {
    fn template_id(&self) -> String {
        "default".to_owned()
    }

    fn render_template(&self, container: NodeId) -> TemplateData {
        Rc::new(container) as Rc<dyn Any>
    }

    fn render_element(&self, node: &Rc<Wrapper>, _index: usize, template_data: &TemplateData, _details: Option<&TreeElementRenderDetails>) {
        let element = node.element();
        let element = el_of(&element);
        self.text(template_data, format!("{}{}", element.id, element.suffix));
    }

    fn dispose_template(&self, _: &TemplateData) {}
}

impl ICompressibleAsyncDataTreeRenderer<El, El, ()> for Renderer {
    fn render_compressed_elements(&self, node: &Rc<CompressibleAsyncDataTreeNodeWrapper<El, El, ()>>, _index: usize, template_data: &TemplateData, _details: Option<&TreeElementRenderDetails>) {
        let result: Vec<String> = node.element().elements.iter().map(|element| format!("{}{}", el_of(element).id, el_of(element).suffix)).collect();
        self.text(template_data, result.join("/"));
    }
}

/// `IdentityProvider`.
struct IdentityProvider;

impl IIdentityProvider<El> for IdentityProvider {
    fn get_id(&self, element: &El) -> String {
        element.id.clone()
    }
}

/// `VirtualDelegate`.
struct VirtualDelegate;

impl IListVirtualDelegate<El> for VirtualDelegate {
    fn get_height(&self, _: &El) -> f64 {
        20.0
    }

    fn get_template_id(&self, _: &El) -> String {
        "default".to_owned()
    }
}

type GetChildren = Box<dyn Fn(&El) -> MaybePromise<Vec<El>>>;

/// Os fechos resolvedores guardados pelo `new Promise(c => calls.push(() => c(...)))`.
type Calls = Rc<RefCell<Vec<Box<dyn FnOnce()>>>>;

/// `DataSource`: `hasChildren` é "tem filhos" e `getChildren` é o `getChildren` de cada teste.
struct DataSource {
    get_children: GetChildren,
}

impl DataSource {
    fn new(get_children: impl Fn(&El) -> MaybePromise<Vec<El>> + 'static) -> Rc<Self> {
        Rc::new(Self { get_children: Box::new(get_children) })
    }

    /// `getChildren(element) { return Promise.resolve(element.children || []); }`.
    fn resolved() -> Rc<Self> {
        Self::new(|element| MaybePromise::Promise(Promise::resolved(element.children.borrow().clone())))
    }
}

impl IAsyncDataSource<El, El> for DataSource {
    fn has_children(&self, element: &Input) -> bool {
        !el_of(element).children.borrow().is_empty()
    }

    fn get_children(&self, element: &Input) -> MaybePromise<Vec<El>> {
        (self.get_children)(el_of(element))
    }
}

/// `Model`.
struct Model {
    root: El,
}

impl Model {
    fn new(root: El) -> Self {
        Self { root }
    }

    fn get(&self, id: &str) -> El {
        find(&self.root, id).expect("element not found")
    }
}

fn attach(ui: &Ui, width: f64, height: f64) -> NodeId {
    let container = create_container(ui, width, height);
    let root = ui.root();
    ui.tree_mut().append_child(root, container);
    container
}

/// `container.querySelectorAll('.monaco-list-row')`, na ordem das linhas.
fn rows(ui: &Ui, container: NodeId) -> Vec<NodeId> {
    let tree = ui.tree();
    let mut rows = query_all(&tree, container, &["monaco-list-row"]);
    rows.sort_by(|a, b| tree.bounds(*a).y0.total_cmp(&tree.bounds(*b).y0));
    rows
}

/// O `textContent` de uma linha.
fn row_text(ui: &Ui, row: NodeId) -> String {
    let tree = ui.tree();
    query_all(&tree, row, &["monaco-tl-contents"]).first().and_then(|contents| tree.label(*contents)).unwrap_or_default().to_owned()
}

/// `Array.from(container.querySelectorAll('.monaco-list-row')).map(e => e.textContent)`.
fn rows_text(ui: &Ui, container: NodeId) -> Vec<String> {
    rows(ui, container).into_iter().map(|row| row_text(ui, row)).collect()
}

/// `container.querySelector('.monaco-list-row:first-child .monaco-tl-twistie')`: `(collapsible, collapsed)`.
fn first_twistie(ui: &Ui, container: NodeId) -> (bool, bool) {
    let first = rows(ui, container)[0];
    let tree = ui.tree();
    let twistie = query_all(&tree, first, &["monaco-tl-twistie"])[0];
    (tree.has_class(twistie, "collapsible"), tree.has_class(twistie, "collapsed"))
}

fn wrapper_id(node: &Rc<Wrapper>) -> String {
    el_of(&node.element()).id.clone()
}

fn identity_options() -> AsyncDataTreeOptions<El> {
    AsyncDataTreeOptions { identity_provider: Some(Rc::new(IdentityProvider)), ..AsyncDataTreeOptions::default() }
}

/// `new AsyncDataTree<Element, Element>('test', container, new VirtualDelegate(), [new Renderer()], dataSource, options)`
/// e `tree.layout(200)`; a árvore é registrada no store do teste (`store.add`).
fn new_tree(ui: &Ui, container: NodeId, store: &je_base::lifecycle::DisposableStore, data_source: Rc<DataSource>, options: AsyncDataTreeOptions<El>) -> Rc<Tree> {
    let tree = Tree::new("test", ui, container, Rc::new(VirtualDelegate), vec![Rc::new(Renderer { ui: ui.clone() })], data_source, options);
    store.add_rc(tree.clone());
    tree.layout(Some(200.0), None);
    tree
}

/// `new CompressibleAsyncDataTree<Element, Element>('test', container, new VirtualDelegate(), compressionDelegate, [new Renderer()], dataSource, options)`.
fn new_compressible_tree(
    ui: &Ui,
    container: NodeId,
    store: &je_base::lifecycle::DisposableStore,
    compression_delegate: Rc<dyn ITreeCompressionDelegate<El>>,
    data_source: Rc<DataSource>,
    options: CompressibleAsyncDataTreeOptions<El>,
) -> Rc<CompressibleTree> {
    let tree = CompressibleTree::new("test", ui, container, Rc::new(VirtualDelegate), compression_delegate, vec![Rc::new(Renderer { ui: ui.clone() })], data_source, options);
    store.add_rc(tree.clone());
    tree.layout(Some(200.0), None);
    tree
}

/// `isIncompressible(element) { return !dataSource.hasChildren(element); }`.
struct IncompressibleLeaves;

impl ITreeCompressionDelegate<El> for IncompressibleLeaves {
    fn is_incompressible(&self, element: &El) -> bool {
        element.children.borrow().is_empty()
    }
}

#[test]
fn collapse_state_should_be_preserved_across_refresh_calls() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let store = guard.store();

    run_with_faked_timers(async move {
        let ui = Ui::new();
        let container = attach(&ui, 200.0, 200.0);

        let model = Model::new(el("root", vec![el("a", vec![])]));

        let tree = new_tree(&ui, container, &store, DataSource::resolved(), identity_options());
        assert_eq!(rows(&ui, container).len(), 0);

        tree.set_input(model.root.clone(), None).await.unwrap();
        assert_eq!(rows(&ui, container).len(), 1);
        assert_eq!(first_twistie(&ui, container), (false, false));

        *model.get("a").children.borrow_mut() = vec![el("aa", vec![]), el("ab", vec![]), el("ac", vec![])];

        tree.update_children(Some(&input(&model.root)), true, false, None).await.unwrap();
        assert_eq!(rows(&ui, container).len(), 1);

        tree.expand(&model.get("a"), false).await.unwrap();
        assert_eq!(rows(&ui, container).len(), 4);

        *model.get("a").children.borrow_mut() = Vec::new();
        tree.update_children(Some(&input(&model.root)), true, false, None).await.unwrap();
        assert_eq!(rows(&ui, container).len(), 1);
    });
}

#[test]
fn issue_68648() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let store = guard.store();

    run_with_faked_timers(async move {
        let ui = Ui::new();
        let container = attach(&ui, 200.0, 200.0);

        let get_children_calls: Rc<RefCell<Vec<String>>> = Rc::default();
        let data_source = DataSource::new({
            let get_children_calls = get_children_calls.clone();
            move |element| {
                get_children_calls.borrow_mut().push(element.id.clone());
                MaybePromise::Promise(Promise::resolved(element.children.borrow().clone()))
            }
        });

        let model = Model::new(el("root", vec![el("a", vec![])]));

        let tree = new_tree(&ui, container, &store, data_source, identity_options());

        tree.set_input(model.root.clone(), None).await.unwrap();
        assert_eq!(*get_children_calls.borrow(), vec!["root"]);

        assert_eq!(first_twistie(&ui, container), (false, false));
        assert!(tree.get_node(None).children()[0].collapsed());

        *model.get("a").children.borrow_mut() = vec![el("aa", vec![]), el("ab", vec![]), el("ac", vec![])];
        tree.update_children(Some(&input(&model.root)), true, false, None).await.unwrap();

        assert_eq!(*get_children_calls.borrow(), vec!["root", "root"]);
        assert_eq!(first_twistie(&ui, container), (true, true));
        assert!(tree.get_node(None).children()[0].collapsed());

        *model.get("a").children.borrow_mut() = Vec::new();
        tree.update_children(Some(&input(&model.root)), true, false, None).await.unwrap();

        assert_eq!(*get_children_calls.borrow(), vec!["root", "root", "root"]);
        assert_eq!(first_twistie(&ui, container), (false, false));
        assert!(tree.get_node(None).children()[0].collapsed());

        *model.get("a").children.borrow_mut() = vec![el("aa", vec![]), el("ab", vec![]), el("ac", vec![])];
        tree.update_children(Some(&input(&model.root)), true, false, None).await.unwrap();

        assert_eq!(*get_children_calls.borrow(), vec!["root", "root", "root", "root"]);
        assert_eq!(first_twistie(&ui, container), (true, true));
        assert!(tree.get_node(None).children()[0].collapsed());
    });
}

#[test]
fn issue_67722_once_resolved_refreshed_collapsed_nodes_should_only_get_children_when_expanded() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let store = guard.store();

    run_with_faked_timers(async move {
        let ui = Ui::new();
        let container = attach(&ui, 200.0, 200.0);

        let get_children_calls: Rc<RefCell<Vec<String>>> = Rc::default();
        let data_source = DataSource::new({
            let get_children_calls = get_children_calls.clone();
            move |element| {
                get_children_calls.borrow_mut().push(element.id.clone());
                MaybePromise::Promise(Promise::resolved(element.children.borrow().clone()))
            }
        });

        let model = Model::new(el("root", vec![el("a", vec![el("aa", vec![]), el("ab", vec![]), el("ac", vec![])])]));

        let tree = new_tree(&ui, container, &store, data_source, identity_options());

        tree.set_input(model.root.clone(), None).await.unwrap();
        assert!(tree.get_node(Some(&elem(&model.get("a")))).collapsed());
        assert_eq!(*get_children_calls.borrow(), vec!["root"]);

        tree.expand(&model.get("a"), false).await.unwrap();
        assert!(!tree.get_node(Some(&elem(&model.get("a")))).collapsed());
        assert_eq!(*get_children_calls.borrow(), vec!["root", "a"]);

        tree.collapse(&model.get("a"), false);
        assert!(tree.get_node(Some(&elem(&model.get("a")))).collapsed());
        assert_eq!(*get_children_calls.borrow(), vec!["root", "a"]);

        tree.update_children(None, true, false, None).await.unwrap();
        assert!(tree.get_node(Some(&elem(&model.get("a")))).collapsed());
        assert_eq!(*get_children_calls.borrow(), vec!["root", "a", "root"], "a should not be refreshed, since it' collapsed");
    });
}

#[test]
fn resolved_collapsed_nodes_which_lose_children_should_lose_twistie_as_well() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let store = guard.store();

    run_with_faked_timers(async move {
        let ui = Ui::new();
        let container = attach(&ui, 200.0, 200.0);

        let model = Model::new(el("root", vec![el("a", vec![el("aa", vec![]), el("ab", vec![]), el("ac", vec![])])]));

        let tree = new_tree(&ui, container, &store, DataSource::resolved(), identity_options());

        tree.set_input(model.root.clone(), None).await.unwrap();
        tree.expand(&model.get("a"), false).await.unwrap();

        assert_eq!(first_twistie(&ui, container), (true, false));
        assert!(!tree.get_node(Some(&elem(&model.get("a")))).collapsed());

        tree.collapse(&model.get("a"), false);
        *model.get("a").children.borrow_mut() = Vec::new();
        tree.update_children(Some(&input(&model.root)), true, false, None).await.unwrap();

        assert_eq!(first_twistie(&ui, container), (false, false));
        assert!(tree.get_node(Some(&elem(&model.get("a")))).collapsed());
    });
}

#[test]
fn issue_192422_resolved_collapsed_nodes_with_changed_children_dont_show_old_children() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let store = guard.store();

    run_with_faked_timers(async move {
        let ui = Ui::new();
        let container = attach(&ui, 200.0, 200.0);

        let has_gotten_a_children = Rc::new(Cell::new(false));
        let data_source = DataSource::new({
            let has_gotten_a_children = has_gotten_a_children.clone();
            move |element| {
                if element.id == "a" {
                    if !has_gotten_a_children.get() {
                        has_gotten_a_children.set(true);
                    } else {
                        return MaybePromise::Promise(Promise::resolved(vec![el("c", vec![])]));
                    }
                }

                MaybePromise::Promise(Promise::resolved(element.children.borrow().clone()))
            }
        });

        let model = Model::new(el("root", vec![el("a", vec![el("b", vec![])])]));

        let tree = new_tree(&ui, container, &store, data_source, identity_options());

        tree.set_input(model.root.clone(), None).await.unwrap();
        let a = model.get("a");
        let a_node = tree.get_node(Some(&elem(&a)));
        assert!(a_node.collapsed());
        tree.expand(&a, false).await.unwrap();
        assert!(!a_node.collapsed());
        assert_eq!(a_node.children().len(), 1);
        assert_eq!(wrapper_id(&a_node.children()[0]), "b");
        let b_child = rows(&ui, container).get(1).map(|row| row_text(&ui, *row));
        assert_eq!(b_child.as_deref(), Some("b"));
        tree.collapse(&a, false);
        assert!(a_node.collapsed());

        tree.update_children(Some(&elem(&a)), true, false, None).await.unwrap();
        let a_updated_1 = model.get("a");
        let a_node_updated_1 = tree.get_node(Some(&elem(&a)));
        assert!(a_node_updated_1.collapsed());
        assert_eq!(a_node_updated_1.children().len(), 0);
        let did_check_no_children = Rc::new(Cell::new(false));
        let event = tree.on_did_change_collapse_state().listen({
            let (ui, did_check_no_children) = (ui.clone(), did_check_no_children.clone());
            move |_| {
                let child = rows(&ui, container).get(1).copied();
                assert!(child.is_none());
                did_check_no_children.set(true);
            }
        });
        tree.expand(&a_updated_1, false).await.unwrap();
        event.dispose();
        assert!(did_check_no_children.get());

        let a_node_updated_2 = tree.get_node(Some(&elem(&a)));
        assert!(!a_node_updated_2.collapsed());
        assert_eq!(a_node_updated_2.children().len(), 1);
        assert_eq!(wrapper_id(&a_node_updated_2.children()[0]), "c");
        let child = rows(&ui, container).get(1).map(|row| row_text(&ui, *row));
        assert_eq!(child.as_deref(), Some("c"));
    });
}

#[test]
fn issue_192422_resolved_collapsed_nodes_with_unchanged_children_immediately_show_children() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let store = guard.store();

    run_with_faked_timers(async move {
        let ui = Ui::new();
        let container = attach(&ui, 200.0, 200.0);

        let data_source = DataSource::resolved();

        let model = Model::new(el("root", vec![el("a", vec![el("b", vec![])])]));

        let tree = new_tree(&ui, container, &store, data_source, identity_options());

        tree.set_input(model.root.clone(), None).await.unwrap();
        let a = model.get("a");
        let a_node = tree.get_node(Some(&elem(&a)));
        assert!(a_node.collapsed());
        tree.expand(&a, false).await.unwrap();
        assert!(!a_node.collapsed());
        assert_eq!(a_node.children().len(), 1);
        assert_eq!(wrapper_id(&a_node.children()[0]), "b");
        let b_child = rows(&ui, container).get(1).map(|row| row_text(&ui, *row));
        assert_eq!(b_child.as_deref(), Some("b"));
        tree.collapse(&a, false);
        assert!(a_node.collapsed());

        let a_updated_1 = model.get("a");
        let a_node_updated_1 = tree.get_node(Some(&elem(&a)));
        assert!(a_node_updated_1.collapsed());
        assert_eq!(a_node_updated_1.children().len(), 1);
        let did_check_same_children = Rc::new(Cell::new(false));
        let event = tree.on_did_change_collapse_state().listen({
            let (ui, did_check_same_children) = (ui.clone(), did_check_same_children.clone());
            move |_| {
                let child = rows(&ui, container).get(1).map(|row| row_text(&ui, *row));
                assert_eq!(child.as_deref(), Some("b"));
                did_check_same_children.set(true);
            }
        });
        tree.expand(&a_updated_1, false).await.unwrap();
        event.dispose();
        assert!(did_check_same_children.get());

        let a_node_updated_2 = tree.get_node(Some(&elem(&a)));
        assert!(!a_node_updated_2.collapsed());
        assert_eq!(a_node_updated_2.children().len(), 1);
        assert_eq!(wrapper_id(&a_node_updated_2.children()[0]), "b");
        let child = rows(&ui, container).get(1).map(|row| row_text(&ui, *row));
        assert_eq!(child.as_deref(), Some("b"));
    });
}

#[test]
fn support_default_collapse_state_per_element() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let store = guard.store();

    run_with_faked_timers(async move {
        let ui = Ui::new();
        let container = attach(&ui, 200.0, 200.0);

        let get_children_calls: Rc<RefCell<Vec<String>>> = Rc::default();
        let data_source = DataSource::new({
            let get_children_calls = get_children_calls.clone();
            move |element| {
                get_children_calls.borrow_mut().push(element.id.clone());
                MaybePromise::Promise(Promise::resolved(element.children.borrow().clone()))
            }
        });

        let model = Model::new(el("root", vec![el("a", vec![el("aa", vec![]), el("ab", vec![]), el("ac", vec![])])]));

        let options = AsyncDataTreeOptions { collapse_by_default: Some(Rc::new(|element: &El| element.id != "a")), ..AsyncDataTreeOptions::default() };
        let tree = new_tree(&ui, container, &store, data_source, options);

        tree.set_input(model.root.clone(), None).await.unwrap();
        assert!(!tree.get_node(Some(&elem(&model.get("a")))).collapsed());
        assert_eq!(*get_children_calls.borrow(), vec!["root", "a"]);
    });
}

#[test]
fn issue_80098_concurrent_refresh_and_expand() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let store = guard.store();

    run_with_faked_timers(async move {
        let ui = Ui::new();
        let container = attach(&ui, 200.0, 200.0);

        let calls: Calls = Rc::default();
        let data_source = DataSource::new({
            let calls = calls.clone();
            move |element| {
                let children = element.children.borrow().clone();
                let calls = calls.clone();
                MaybePromise::Promise(Promise::with_executor(move |resolver| calls.borrow_mut().push(Box::new(move || resolver.resolve(children)))))
            }
        });

        let model = Model::new(el("root", vec![el("a", vec![el("aa", vec![])])]));

        let tree = new_tree(&ui, container, &store, data_source, identity_options());

        let p_set_input = tree.set_input(model.root.clone(), None);
        let call = calls.borrow_mut().pop().unwrap();
        call(); // resolve getChildren(root)
        p_set_input.await.unwrap();

        let p_update_children_a = tree.update_children(Some(&elem(&model.get("a"))), true, false, None);
        let p_expand_a = tree.expand(&model.get("a"), false);
        assert_eq!(calls.borrow().len(), 1, "expand(a) still hasn't called getChildren(a)");

        let call = calls.borrow_mut().pop().unwrap();
        call();
        assert_eq!(calls.borrow().len(), 0, "no pending getChildren calls");

        p_update_children_a.await.unwrap();
        assert_eq!(calls.borrow().len(), 0, "expand(a) should not have forced a second refresh");

        let result = p_expand_a.await.unwrap();
        assert!(result, "expand(a) should be done");
    });
}

#[test]
fn issue_80098_first_expand_should_call_get_children() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let store = guard.store();

    run_with_faked_timers(async move {
        let ui = Ui::new();
        let container = attach(&ui, 200.0, 200.0);

        let calls: Calls = Rc::default();
        let data_source = DataSource::new({
            let calls = calls.clone();
            move |element| {
                let children = element.children.borrow().clone();
                let calls = calls.clone();
                MaybePromise::Promise(Promise::with_executor(move |resolver| calls.borrow_mut().push(Box::new(move || resolver.resolve(children)))))
            }
        });

        let model = Model::new(el("root", vec![el("a", vec![el("aa", vec![])])]));

        let tree = new_tree(&ui, container, &store, data_source, identity_options());

        let p_set_input = tree.set_input(model.root.clone(), None);
        let call = calls.borrow_mut().pop().unwrap();
        call(); // resolve getChildren(root)
        p_set_input.await.unwrap();

        let p_expand_a = tree.expand(&model.get("a"), false);
        assert_eq!(calls.borrow().len(), 1, "expand(a) should've called getChildren(a)");

        let mut race = Promise::race(vec![p_expand_a.then(|_| "expand"), timeout(1).then(|()| "timeout")]).await.unwrap();
        assert_eq!(race, "timeout", "expand(a) should not be yet done");

        let call = calls.borrow_mut().pop().unwrap();
        call();
        assert_eq!(calls.borrow().len(), 0, "no pending getChildren calls");

        race = Promise::race(vec![p_expand_a.then(|_| "expand"), timeout(1).then(|()| "timeout")]).await.unwrap();
        assert_eq!(race, "expand", "expand(a) should now be done");
    });
}

#[test]
fn issue_78388_tree_should_react_to_has_children_toggles() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let store = guard.store();

    run_with_faked_timers(async move {
        let ui = Ui::new();
        let container = attach(&ui, 200.0, 200.0);

        let model = Model::new(el("root", vec![el("a", vec![])]));

        let tree = new_tree(&ui, container, &store, DataSource::resolved(), identity_options());

        tree.set_input(model.root.clone(), None).await.unwrap();
        assert_eq!(rows(&ui, container).len(), 1);

        assert_eq!(first_twistie(&ui, container), (false, false));

        *model.get("a").children.borrow_mut() = vec![el("aa", vec![])];
        tree.update_children(Some(&elem(&model.get("a"))), false, false, None).await.unwrap();
        assert_eq!(rows(&ui, container).len(), 1);
        assert_eq!(first_twistie(&ui, container), (true, true));

        *model.get("a").children.borrow_mut() = Vec::new();
        tree.update_children(Some(&elem(&model.get("a"))), false, false, None).await.unwrap();
        assert_eq!(rows(&ui, container).len(), 1);
        assert_eq!(first_twistie(&ui, container), (false, false));
    });
}

#[test]
fn issues_84569_82629_rerender() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let store = guard.store();

    run_with_faked_timers(async move {
        let ui = Ui::new();
        let container = attach(&ui, 200.0, 200.0);

        let model = Model::new(el("root", vec![el("a", vec![el_suffix("b", "1")])]));

        let tree = new_tree(&ui, container, &store, DataSource::resolved(), identity_options());

        tree.set_input(model.root.clone(), None).await.unwrap();
        tree.expand(&model.get("a"), false).await.unwrap();
        assert_eq!(rows_text(&ui, container), vec!["a", "b1"]);

        let a = model.get("a");
        let b = model.get("b");
        a.children.borrow_mut().splice(0..1, [el_suffix("b", "2")]);

        Promise::all(vec![tree.update_children(Some(&elem(&a)), true, true, None), tree.update_children(Some(&elem(&b)), true, true, None)]).await.unwrap();

        assert_eq!(rows_text(&ui, container), vec!["a", "b2"]);
    });
}

#[test]
fn issue_199264_dispose_during_render() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let store = guard.store();

    run_with_faked_timers(async move {
        let ui = Ui::new();
        let container = attach(&ui, 200.0, 200.0);

        let model1 = Model::new(el("root", vec![el("a", vec![el("aa", vec![]), el("ab", vec![]), el("ac", vec![])])]));
        let model2 = Model::new(el("root", vec![el("a", vec![el("aa", vec![]), el("ab", vec![]), el("ac", vec![])])]));

        let tree = new_tree(&ui, container, &store, DataSource::resolved(), identity_options());

        tree.set_input(model1.root.clone(), None).await.unwrap();
        let input = tree.set_input(model2.root.clone(), None);
        tree.dispose();
        input.await.unwrap();
        assert!(ui.tree().children(container).is_empty());
    });
}

#[test]
fn issue_121567() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let store = guard.store();

    run_with_faked_timers(async move {
        let ui = Ui::new();
        let container = attach(&ui, 200.0, 200.0);

        let calls: Rc<RefCell<Vec<El>>> = Rc::default();
        let data_source = DataSource::new({
            let calls = calls.clone();
            move |element| {
                calls.borrow_mut().push(element.clone());
                MaybePromise::Promise(Promise::resolved(element.children.borrow().clone()))
            }
        });

        let model = Model::new(el("root", vec![el("a", vec![el("aa", vec![])])]));
        let a = model.get("a");

        let tree = new_tree(&ui, container, &store, data_source, identity_options());

        tree.set_input(model.root.clone(), None).await.unwrap();
        assert_eq!(calls.borrow().len(), 1, "There should be a single getChildren call for the root");
        assert!(tree.is_collapsible(&a), "a is collapsible");
        assert!(tree.is_collapsed(&elem(&a)), "a is collapsed");

        tree.update_children(Some(&elem(&a)), false, false, None).await.unwrap();
        assert_eq!(calls.borrow().len(), 1, "There should be no changes to the calls list, since a was collapsed");
        assert!(tree.is_collapsible(&a), "a is collapsible");
        assert!(tree.is_collapsed(&elem(&a)), "a is collapsed");

        let children = a.children.replace(Vec::new());
        tree.update_children(Some(&elem(&a)), false, false, None).await.unwrap();
        assert_eq!(calls.borrow().len(), 1, "There should still be no changes to the calls list, since a was collapsed");
        assert!(!tree.is_collapsible(&a), "a is no longer collapsible");
        assert!(tree.is_collapsed(&elem(&a)), "a is collapsed");

        *a.children.borrow_mut() = children;
        tree.update_children(Some(&elem(&a)), false, false, None).await.unwrap();
        assert_eq!(calls.borrow().len(), 1, "There should still be no changes to the calls list, since a was collapsed");
        assert!(tree.is_collapsible(&a), "a is collapsible again");
        assert!(tree.is_collapsed(&elem(&a)), "a is collapsed");

        tree.expand(&a, false).await.unwrap();
        assert_eq!(calls.borrow().len(), 2, "Finally, there should be a getChildren call for a");
        assert!(tree.is_collapsible(&a), "a is still collapsible");
        assert!(!tree.is_collapsed(&elem(&a)), "a is expanded");
    });
}

#[test]
fn issue_199441() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let store = guard.store();

    run_with_faked_timers(async move {
        let ui = Ui::new();
        let container = attach(&ui, 200.0, 200.0);

        let model = Model::new(el("root", vec![el("a", vec![el("b", vec![el("b.txt", vec![])])])]));

        let options = CompressibleAsyncDataTreeOptions { base: AsyncDataTreeOptions { collapse_by_default: Some(Rc::new(|_: &El| false)), ..identity_options() }, ..CompressibleAsyncDataTreeOptions::default() };
        let tree = new_compressible_tree(&ui, container, &store, Rc::new(IncompressibleLeaves), DataSource::resolved(), options);

        tree.set_input(model.root.clone(), None).await.unwrap();
        assert_eq!(rows_text(&ui, container), vec!["a/b", "b.txt"]);

        model.get("a").children.borrow_mut().push(el("c", vec![el("c.txt", vec![])]));

        tree.update_children(Some(&input(&model.root)), true, false, None).await.unwrap();
        assert_eq!(rows_text(&ui, container), vec!["a", "b", "b.txt", "c", "c.txt"]);
    });
}

/// `isIncompressible(element) { return !dataSource.hasChildren(element) || element.id === 'nest'; }`.
struct IncompressibleLeavesAndNest;

impl ITreeCompressionDelegate<El> for IncompressibleLeavesAndNest {
    fn is_incompressible(&self, element: &El) -> bool {
        element.children.borrow().is_empty() || element.id == "nest"
    }
}

#[test]
fn issues_145510_149084_auto_expand_compressed_children_only_expands_single_children_compressed_with_their_parent() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let store = guard.store();

    run_with_faked_timers(async move {
        let model = Rc::new(Model::new(el(
            "root",
            vec![el("a", vec![el("b", vec![el("b.txt", vec![])])]), el("n", vec![el("nest", vec![el("nest.txt", vec![])])])],
        )));

        let expand_and_refresh = {
            let store = store.clone();
            let model = model.clone();

            move |compression_enabled: bool| {
                let store = store.clone();
                let model = model.clone();

                async move {
                    let ui = Ui::new();
                    let container = attach(&ui, 200.0, 200.0);
                    let options = CompressibleAsyncDataTreeOptions {
                        base: identity_options(),
                        compression_enabled: Some(compression_enabled),
                        auto_expand_compressed_children: true,
                        ..CompressibleAsyncDataTreeOptions::default()
                    };
                    let tree = new_compressible_tree(&ui, container, &store, Rc::new(IncompressibleLeavesAndNest), DataSource::resolved(), options);

                    tree.set_input(model.root.clone(), None).await.unwrap();
                    tree.expand(&model.get("a"), false).await.unwrap();
                    tree.expand(&model.get("n"), false).await.unwrap();
                    tree.update_children(Some(&input(&model.root)), true, false, None).await.unwrap();
                    rows_text(&ui, container)
                }
            }
        };

        let compressed = expand_and_refresh(true).await;
        let uncompressed = expand_and_refresh(false).await;

        assert_eq!(compressed, vec!["a/b", "b.txt", "n", "nest"], "compressed");
        assert_eq!(uncompressed, vec!["a", "b", "n", "nest"], "uncompressed");
    });
}

#[test]
fn auto_expand_single_children_expands_single_children_that_are_not_compressed() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let store = guard.store();

    run_with_faked_timers(async move {
        let ui = Ui::new();
        let container = attach(&ui, 200.0, 200.0);

        struct AllIncompressible;

        impl ITreeCompressionDelegate<El> for AllIncompressible {
            fn is_incompressible(&self, _: &El) -> bool {
                true
            }
        }

        let model = Model::new(el("root", vec![el("a", vec![el("b", vec![el("b.txt", vec![])])])]));

        let options = CompressibleAsyncDataTreeOptions { base: AsyncDataTreeOptions { auto_expand_single_children: true, ..identity_options() }, ..CompressibleAsyncDataTreeOptions::default() };
        let tree = new_compressible_tree(&ui, container, &store, Rc::new(AllIncompressible), DataSource::resolved(), options);

        tree.set_input(model.root.clone(), None).await.unwrap();
        tree.expand(&model.get("a"), false).await.unwrap();
        assert_eq!(rows_text(&ui, container), vec!["a", "b", "b.txt"]);
    });
}

fn id_of(element: Option<El>) -> Option<String> {
    element.map(|element| element.id.clone())
}

#[test]
fn tree_navigation_async_data_tree() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let store = guard.store();

    run_with_faked_timers(async move {
        let ui = Ui::new();
        let container = attach(&ui, 200.0, 200.0);

        let model = Model::new(el(
            "root",
            vec![
                el("a", vec![el("aa", vec![el("aa.txt", vec![])]), el("ab", vec![el("ab.txt", vec![])])]),
                el("b", vec![el("ba", vec![el("ba.txt", vec![])]), el("bb", vec![el("bb.txt", vec![])])]),
                el("c", vec![el("ca", vec![el("ca.txt", vec![])]), el("cb", vec![el("cb.txt", vec![])])]),
            ],
        ));

        let tree = new_tree(&ui, container, &store, DataSource::resolved(), identity_options());

        tree.set_input(model.root.clone(), None).await.unwrap();
        assert_eq!(rows_text(&ui, container), vec!["a", "b", "c"]);

        assert_eq!(id_of(tree.navigate(None).current()), None);
        assert_eq!(id_of(tree.navigate(None).first()).as_deref(), Some("a"));
        assert_eq!(id_of(tree.navigate(None).last()).as_deref(), Some("c"));

        assert_eq!(id_of(tree.navigate(Some(&model.get("b"))).previous()).as_deref(), Some("a"));
        assert_eq!(id_of(tree.navigate(Some(&model.get("b"))).next()).as_deref(), Some("c"));

        tree.expand(&model.get("a"), false).await.unwrap();
        tree.expand(&model.get("aa"), false).await.unwrap();
        tree.expand(&model.get("ab"), false).await.unwrap();

        tree.expand(&model.get("b"), false).await.unwrap();
        tree.expand(&model.get("ba"), false).await.unwrap();
        tree.expand(&model.get("bb"), false).await.unwrap();

        tree.expand(&model.get("c"), false).await.unwrap();
        tree.expand(&model.get("ca"), false).await.unwrap();
        tree.expand(&model.get("cb"), false).await.unwrap();

        // Only the first 10 elements are rendered (total height is 200px, each element is 20px)
        assert_eq!(rows_text(&ui, container), vec!["a", "aa", "aa.txt", "ab", "ab.txt", "b", "ba", "ba.txt", "bb", "bb.txt"]);

        assert_eq!(id_of(tree.navigate(None).first()).as_deref(), Some("a"));
        assert_eq!(id_of(tree.navigate(None).last()).as_deref(), Some("cb.txt"));

        assert_eq!(id_of(tree.navigate(Some(&model.get("b"))).previous()).as_deref(), Some("ab.txt"));
        assert_eq!(id_of(tree.navigate(Some(&model.get("b"))).next()).as_deref(), Some("ba"));

        assert_eq!(id_of(tree.navigate(Some(&model.get("ab.txt"))).previous()).as_deref(), Some("ab"));
        assert_eq!(id_of(tree.navigate(Some(&model.get("ab.txt"))).next()).as_deref(), Some("b"));

        assert_eq!(id_of(tree.navigate(Some(&model.get("bb.txt"))).next()).as_deref(), Some("c"));

        tree.collapse(&model.get("b"), false);
        assert_eq!(rows_text(&ui, container), vec!["a", "aa", "aa.txt", "ab", "ab.txt", "b", "c", "ca", "ca.txt", "cb"]);

        assert_eq!(id_of(tree.navigate(Some(&model.get("b"))).next()).as_deref(), Some("c"));
    });
}

#[test]
fn test_navigation_compressible_async_data_tree() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let store = guard.store();

    run_with_faked_timers(async move {
        let ui = Ui::new();
        let container = attach(&ui, 200.0, 200.0);

        let data_source = DataSource::new(|element| MaybePromise::Promise(Promise::resolved(element.children.borrow().clone())));

        let model = Model::new(el(
            "root",
            vec![
                el("a", vec![el("aa", vec![el("aa.txt", vec![])])]),
                el("b", vec![el("ba", vec![el("ba.txt", vec![])])]),
                el("c", vec![el("ca", vec![el("ca.txt", vec![])]), el("cb", vec![el("cb.txt", vec![])])]),
            ],
        ));

        let tree = new_compressible_tree(&ui, container, &store, Rc::new(IncompressibleLeaves), data_source, CompressibleAsyncDataTreeOptions { base: identity_options(), ..CompressibleAsyncDataTreeOptions::default() });

        tree.set_input(model.root.clone(), None).await.unwrap();
        assert_eq!(rows_text(&ui, container), vec!["a", "b", "c"]);

        assert_eq!(id_of(tree.navigate(None).current()), None);
        assert_eq!(id_of(tree.navigate(None).first()).as_deref(), Some("a"));
        assert_eq!(id_of(tree.navigate(None).last()).as_deref(), Some("c"));

        assert_eq!(id_of(tree.navigate(Some(&model.get("b"))).previous()).as_deref(), Some("a"));
        assert_eq!(id_of(tree.navigate(Some(&model.get("b"))).next()).as_deref(), Some("c"));

        tree.expand(&model.get("a"), false).await.unwrap();
        tree.expand(&model.get("aa"), false).await.unwrap();

        tree.expand(&model.get("b"), false).await.unwrap();
        tree.expand(&model.get("ba"), false).await.unwrap();

        tree.expand(&model.get("c"), false).await.unwrap();
        tree.expand(&model.get("ca"), false).await.unwrap();
        tree.expand(&model.get("cb"), false).await.unwrap();

        // Only the first 10 elements are rendered (total height is 200px, each element is 20px)
        assert_eq!(rows_text(&ui, container), vec!["a/aa", "aa.txt", "b/ba", "ba.txt", "c", "ca", "ca.txt", "cb", "cb.txt"]);

        assert_eq!(id_of(tree.navigate(None).first()).as_deref(), Some("aa"));
        assert_eq!(id_of(tree.navigate(None).last()).as_deref(), Some("cb.txt"));

        assert_eq!(id_of(tree.navigate(Some(&model.get("b"))).previous()).as_deref(), Some("aa.txt"));
        assert_eq!(id_of(tree.navigate(Some(&model.get("ba"))).previous()).as_deref(), Some("aa.txt"));

        assert_eq!(id_of(tree.navigate(Some(&model.get("b"))).next()).as_deref(), Some("ba.txt"));
        assert_eq!(id_of(tree.navigate(Some(&model.get("ba"))).next()).as_deref(), Some("ba.txt"));

        assert_eq!(id_of(tree.navigate(Some(&model.get("aa.txt"))).previous()).as_deref(), Some("aa"));
        assert_eq!(id_of(tree.navigate(Some(&model.get("aa.txt"))).next()).as_deref(), Some("ba"));
    });
}

/// O desempenho do `refresh` de uma pasta com 10 mil filhos (árvore de arquivos: uma raiz expandida, a
/// pasta com 10 mil filhos sem filhos próprios, `getChildren` devolvendo `Promise::resolved`).
#[test]
#[ignore = "medição de desempenho: rode com --ignored --nocapture (de preferência com --release)"]
fn async_data_tree_refresh_10k_children_performance() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let store = guard.store();

    run_with_faked_timers(async move {
        let ui = Ui::new();
        let container = attach(&ui, 200.0, 200.0);

        let files: Vec<El> = (0..10_000).map(|index| el(&format!("file{index}"), vec![])).collect();
        let model = Model::new(el("root", vec![el("folder", files)]));

        let options = AsyncDataTreeOptions { collapse_by_default: Some(Rc::new(|_: &El| false)), ..identity_options() };
        let tree = new_tree(&ui, container, &store, DataSource::resolved(), options);

        let started = std::time::Instant::now();
        tree.set_input(model.root.clone(), None).await.unwrap();
        println!("async_data_tree set_input 10k filhos: {} us", started.elapsed().as_micros());
        assert_eq!(model.get("folder").children.borrow().len(), 10_000);

        let started = std::time::Instant::now();
        tree.update_children(Some(&elem(&model.get("folder"))), true, false, None).await.unwrap();
        println!("async_data_tree refresh 10k filhos: {} us", started.elapsed().as_micros());
    });
}
