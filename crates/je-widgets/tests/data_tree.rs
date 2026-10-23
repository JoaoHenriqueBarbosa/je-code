//! Porte de `src/vs/base/test/browser/ui/tree/dataTree.test.ts` (suíte `DataTree`), caso a caso e na mesma ordem.
//!
//! Notas de tradução:
//! - `interface E { value, children? }` é `E` com `children: Vec<ByRef<E>>` (vazio no lugar do `undefined`); os
//!   elementos são `ByRef<E>` porque a árvore os compara por identidade de objeto, como o `===` do original.
//! - `TInput` e `T` são o mesmo `ByRef<E>`: a entrada vai como `InputOrElement::Input` ao `getChildren`.
//! - `container.style.width/height` é `create_container` + `append_child` na raiz da `Ui`; `tree.layout(200)`
//!   é `layout(Some(200.0), None)`.
//! - O `null` do `navigator.next()!` do fim da árvore é o elemento nulo (a raiz) que o `TreeNavigator` do
//!   `ObjectTreeModel` devolve: `flat` junta o "sem próximo" e o `null`.
//! - `teardown(() => tree.dispose())` e `ensureNoDisposablesAreLeakedInTestSuite()` são o `Drop` do `Fixture`.

mod support;

use std::any::Any;
use std::rc::Rc;

use je_base::lifecycle::IDisposable;
use je_ui::{NodeId, Ui};
use je_widgets::list::list_types::{IIdentityProvider, IListVirtualDelegate, TemplateData};
use je_widgets::tree::abstract_tree::AbstractTreeOptions;
use je_widgets::tree::data_tree::{DataTree, DataTreeOptions};
use je_widgets::tree::object_tree_model::ObjectTreeModel;
use je_widgets::tree::tree_types::{ByRef, IDataSource, ITreeNavigator, ITreeRenderer, InputOrElement, TreeElementRenderDetails};
use support::{create_container, ensure_no_disposables_are_leaked_in_test_suite};

#[derive(Debug)]
struct E {
    value: i64,
    children: Vec<ByRef<E>>,
}

type El = ByRef<E>;
type Model = ObjectTreeModel<El>;

fn e(value: i64, children: Vec<El>) -> El {
    ByRef::new(E { value, children })
}

/// `getHeight() { return 20; }`.
struct Delegate;

impl IListVirtualDelegate<El> for Delegate {
    fn get_height(&self, _: &El) -> f64 {
        20.0
    }

    fn get_template_id(&self, _: &El) -> String {
        "default".to_owned()
    }
}

/// `renderTemplate(container) { return container; }` e `textContent = `${element.element.value}``.
struct Renderer {
    ui: Ui,
}

impl ITreeRenderer<Model> for Renderer {
    fn template_id(&self) -> String {
        "default".to_owned()
    }

    fn render_template(&self, container: NodeId) -> TemplateData {
        Rc::new(container) as Rc<dyn Any>
    }

    fn render_element(&self, node: &Rc<je_widgets::tree::index_tree_model::TreeNode<Option<El>>>, _index: usize, template_data: &TemplateData, _details: Option<&TreeElementRenderDetails>) {
        let container = *template_data.downcast_ref::<NodeId>().expect("template");
        self.ui.tree_mut().set_label(container, format!("{}", node.element.as_ref().expect("elemento").value));
    }

    fn dispose_template(&self, _: &TemplateData) {}
}

/// `getChildren(element) { return element.children || []; }`.
struct DataSource;

impl IDataSource<El, El> for DataSource {
    fn get_children(&self, element: &InputOrElement<El, El>) -> Vec<El> {
        match element {
            InputOrElement::Input(e) | InputOrElement::Element(e) => e.children.clone(),
        }
    }
}

/// `getId(element) { return `${element.value}`; }`.
struct IdentityProvider;

impl IIdentityProvider<Option<El>> for IdentityProvider {
    fn get_id(&self, element: &Option<El>) -> String {
        format!("{}", element.as_ref().expect("elemento").value)
    }
}

/// `flat`: o elemento do `Option<Option<El>>` do navegador (o "sem próximo" e o nulo são o mesmo `null`).
fn flat(value: Option<Option<El>>) -> Option<El> {
    value.flatten()
}

fn value_of(value: Option<El>) -> i64 {
    value.expect("elemento").value
}

struct Fixture {
    tree: Rc<DataTree<El, El>>,
    root: El,
    empty: El,
    _guard: support::LeakGuard,
}

impl Drop for Fixture {
    fn drop(&mut self) {
        self.tree.dispose();
    }
}

/// `setup`.
fn setup() -> Fixture {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let ui = Ui::new();
    let container = create_container(&ui, 200.0, 200.0);
    let ui_root = ui.root();
    ui.tree_mut().append_child(ui_root, container);

    let root = e(-1, vec![e(0, vec![e(10, vec![]), e(11, vec![]), e(12, vec![])]), e(1, vec![]), e(2, vec![])]);
    let empty = e(-1, vec![]);

    let options = DataTreeOptions { tree: AbstractTreeOptions { identity_provider: Some(Rc::new(IdentityProvider)), ..AbstractTreeOptions::default() }, sorter: None };
    let tree = DataTree::<El, El>::new("test", &ui, container, Rc::new(Delegate), vec![Rc::new(Renderer { ui: ui.clone() })], Rc::new(DataSource), options);
    tree.layout(Some(200.0), None);

    Fixture { tree, root, empty, _guard: guard }
}

/// A sequência `navigator.next()` do original até o fim, como os valores.
fn walk(tree: &DataTree<El, El>) -> Vec<i64> {
    let mut navigator = tree.navigate(None);
    let mut values = Vec::new();

    while let Some(element) = flat(navigator.next()) {
        values.push(element.value);
    }

    values
}

#[test]
fn view_state_is_lost_implicitly() {
    let f = setup();
    f.tree.set_input(Some(f.root.clone()), None);

    let mut navigator = f.tree.navigate(None);
    assert_eq!(value_of(flat(navigator.next())), 0);
    assert_eq!(value_of(flat(navigator.next())), 10);
    assert_eq!(value_of(flat(navigator.next())), 11);
    assert_eq!(value_of(flat(navigator.next())), 12);
    assert_eq!(value_of(flat(navigator.next())), 1);
    assert_eq!(value_of(flat(navigator.next())), 2);
    assert!(flat(navigator.next()).is_none());

    f.tree.collapse(&Some(f.root.children[0].clone()), false);
    let mut navigator = f.tree.navigate(None);
    assert_eq!(value_of(flat(navigator.next())), 0);
    assert_eq!(value_of(flat(navigator.next())), 1);
    assert_eq!(value_of(flat(navigator.next())), 2);
    assert!(flat(navigator.next()).is_none());

    f.tree.set_selection(&[Some(f.root.children[1].clone())], None);
    f.tree.set_focus(&[Some(f.root.children[2].clone())], None);

    f.tree.set_input(Some(f.empty.clone()), None);
    f.tree.set_input(Some(f.root.clone()), None);
    assert_eq!(walk(&f.tree), vec![0, 10, 11, 12, 1, 2]);

    assert_eq!(f.tree.get_selection(), Vec::<Option<El>>::new());
    assert_eq!(f.tree.get_focus(), Vec::<Option<El>>::new());
}

#[test]
fn view_state_can_be_preserved() {
    let f = setup();
    f.tree.set_input(Some(f.root.clone()), None);

    let mut navigator = f.tree.navigate(None);
    assert_eq!(value_of(flat(navigator.next())), 0);
    assert_eq!(value_of(flat(navigator.next())), 10);
    assert_eq!(value_of(flat(navigator.next())), 11);
    assert_eq!(value_of(flat(navigator.next())), 12);
    assert_eq!(value_of(flat(navigator.next())), 1);
    assert_eq!(value_of(flat(navigator.next())), 2);
    assert!(flat(navigator.next()).is_none());

    f.tree.collapse(&Some(f.root.children[0].clone()), false);
    let mut navigator = f.tree.navigate(None);
    assert_eq!(value_of(flat(navigator.next())), 0);
    assert_eq!(value_of(flat(navigator.next())), 1);
    assert_eq!(value_of(flat(navigator.next())), 2);
    assert!(flat(navigator.next()).is_none());

    f.tree.set_selection(&[Some(f.root.children[1].clone())], None);
    f.tree.set_focus(&[Some(f.root.children[2].clone())], None);

    let view_state = f.tree.get_view_state(None);

    f.tree.set_input(Some(f.empty.clone()), None);
    f.tree.set_input(Some(f.root.clone()), Some(&view_state));
    assert_eq!(walk(&f.tree), vec![0, 1, 2]);

    assert_eq!(f.tree.get_selection(), vec![Some(f.root.children[1].clone())]);
    assert_eq!(f.tree.get_focus(), vec![Some(f.root.children[2].clone())]);
}
