//! Porte de `src/vs/workbench/contrib/testing/browser/explorerProjections/index.ts`: o contrato das projeções
//! ([`ITestTreeProjection`]), os elementos da árvore do explorador e o provedor de identidade deles.
//!
//! Divergências:
//! - **`TestItemTreeElement` é abstrata** e tem duas subclasses (`ListTestItemElement` e `TreeTestItemElement`, privadas
//!   de cada projeção). Aqui é uma struct só com o [`TestItemElementKind`], que carrega o que cada subclasse tem de
//!   próprio (a cadeia de pais e o `descriptionParts` da lista; o `ownState`, o `ownDuration` e o aviso de filhos
//!   alterados da árvore); `description`, `update`, `fireChange` e a visibilidade do filho de erro, que as duas
//!   subclasses repetem, ficam na struct. `instanceof ListTestItemElement` é `kind` ser `List`.
//! - **Identidade.** Os elementos são `Rc`, e a igualdade e o `hash` do [`TestExplorerTreeElement`] são pelo endereço
//!   (o `===` do JavaScript), que é o que a árvore de objetos usa como chave. O `parent` é um `Weak` (o pai guarda os
//!   filhos, e um `Rc` nos dois sentidos nunca seria liberado).
//! - **`children: Set<...>`** é um `Vec` sem repetição por identidade, na ordem de inserção
//!   ([`TestItemTreeElement::add_child`] e [`TestItemTreeElement::delete_child`]).
//! - **`Iterable<IObjectTreeElement>`** de [`get_children_for_parent`] é um `Vec` calculado de uma vez (o modelo da
//!   árvore percorre todos os descendentes de qualquer jeito).
//! - **`expandElement(element, depth)`** recebe `depth` como `f64` (aceita `Infinity`).
//! - **`lastState`** (campo público e mutável) é [`ITestTreeProjection::last_state`] e
//!   [`ITestTreeProjection::set_last_state`].
//! - **`toJSON`** devolve o enum [`TestItemTreeElementJson`].
//! - **`rootsWithChildren`**, a conversão do nó da coleção em `InternalTestItem` (no original o nó já tem essa forma) e o
//!   começo do tratamento de `onTestChanged` (idênticos nas duas projeções) são as funções [`roots_with_children`],
//!   [`internal_test_item_of`] e [`latest_state_for_change`].

use std::cell::{Cell, RefCell};
use std::hash::{Hash, Hasher};
use std::rc::{Rc, Weak};

use indexmap::IndexMap;
use je_base::errors::{Error, throw};
use je_base::event::{Emitter, Event};
use je_base::filters::FuzzyScore;
use je_base::lifecycle::{IDisposable, id_of};
use je_base::marshalling_ids::MarshalledId;
use je_widgets::list::list_types::IIdentityProvider;
use je_widgets::tree::object_tree::ObjectTree;
use je_widgets::tree::tree_types::{CollapsedValue, IObjectTreeElement, ObjectTreeElementCollapseState};

use super::display::FLAT_TEST_ITEM_DELIMITER;
use super::testing_view_state::{ISerializedTestTreeCollapseState, is_collapsed_in_serialized_test_tree};
use crate::contrib::testing::common::get_computed_state::ComputedStateNode;
use crate::contrib::testing::common::test_result::{TestResultItemChange, TestResultItemChangeReason, TestResultItemRef};
use crate::contrib::testing::common::test_result_service::ITestResultService;
use crate::contrib::testing::common::test_service::IMainThreadTestCollection;
use crate::contrib::testing::common::testing_states::TestStateCount;
use crate::contrib::testing::common::test_types::{
    ITestItemContext, ITestItemUpdate, IncrementalNodeRef, IncrementalTestCollectionItem, InternalTestItem, InternalTestItemSerialized, TestItemExpandState, TestMessageText,
    TestResultState, apply_test_item_update,
};

/// Describes a rendering of tests in the explorer view. Different
/// implementations of this are used for trees and lists, and groupings.
/// Originally this was implemented as inline logic within the ViewModel and
/// using a single IncrementalTestChangeCollector, but this became hairy
/// with status projections.
pub trait ITestTreeProjection: IDisposable {
    /// Event that fires when the projection changes.
    fn on_update(&self) -> Event<()>;

    /// State to use for applying default collapse state of items.
    fn last_state(&self) -> ISerializedTestTreeCollapseState;

    fn set_last_state(&self, state: ISerializedTestTreeCollapseState);

    /// Fired when an element in the tree is expanded.
    fn expand_element(&self, element: &Rc<TestItemTreeElement>, depth: f64);

    /// Gets an element by its extension-assigned ID.
    fn get_element_by_test_id(&self, test_id: &str) -> Option<Rc<TestItemTreeElement>>;

    /// Applies pending update to the tree.
    fn apply_to(&self, tree: &ObjectTree<TestExplorerTreeElement, FuzzyScore>);
}

thread_local! {
    static ID_COUNTER: Cell<u64> = const { Cell::new(0) };
}

/// `getId`.
fn get_id() -> String {
    ID_COUNTER.with(|counter| {
        let current = counter.get();
        counter.set(current + 1);
        current.to_string()
    })
}

/// O que a subclasse da lista tem de próprio.
pub struct ListItemData {
    chain: Vec<InternalTestItem>,
    pub description_parts: RefCell<Vec<String>>,
}

/// O que a subclasse da árvore tem de próprio.
pub struct TreeItemData {
    /// Own, non-computed state.
    pub own_state: Cell<TestResultState>,
    /// Own, non-computed duration.
    pub own_duration: Cell<Option<f64>>,
    added_or_removed: Rc<dyn Fn(&Rc<TestItemTreeElement>)>,
}

/// Qual das duas subclasses de `TestItemTreeElement` o elemento é.
pub enum TestItemElementKind {
    List(ListItemData),
    Tree(TreeItemData),
}

/// `toJSON()` do elemento: o contexto das ações do explorador (ou só o controlador, para a raiz).
#[derive(Clone, Debug, PartialEq)]
pub enum TestItemTreeElementJson {
    Root { controller_id: String },
    Context(ITestItemContext),
}

/// A mensagem de erro como o JavaScript a lê em `if (error)`: um texto vazio é falso.
fn truthy_error(error: Option<&TestMessageText>) -> Option<TestMessageText> {
    match error {
        Some(TestMessageText::Text(text)) if text.is_empty() => None,
        other => other.cloned(),
    }
}

pub struct TestItemTreeElement {
    changed: Rc<Emitter<()>>,

    /// Tree children of this item.
    children: RefCell<Vec<TestExplorerTreeElement>>,

    /// Unique ID of the element in the tree.
    pub tree_id: String,

    /// Depth of the element in the tree.
    pub depth: Cell<usize>,

    /// Whether the node's test result is 'retired' -- from an outdated test run.
    pub retired: Cell<bool>,

    /// State to show on the item. This is generally the item's computed state
    /// from its children.
    pub state: Cell<TestResultState>,

    /// Time it took this test/item to run.
    pub duration: Cell<Option<f64>>,

    pub test: RefCell<InternalTestItem>,

    /// Parent tree item. May not actually be the test item who owns this one
    /// in a 'flat' projection.
    parent: Option<Weak<TestItemTreeElement>>,

    pub kind: TestItemElementKind,

    error_child: RefCell<Option<Rc<TestTreeErrorMessage>>>,

    /// O slot da contagem por estado dos filhos que o cálculo do estado usa (o `WeakMap` do original).
    large_children_states: RefCell<Option<TestStateCount>>,
}

impl ComputedStateNode for Rc<TestItemTreeElement> {
    fn identity(&self) -> usize {
        Rc::as_ptr(self) as usize
    }

    fn large_children_states(&self) -> &RefCell<Option<TestStateCount>> {
        &self.large_children_states
    }
}

impl TestItemTreeElement {
    fn new(test: &InternalTestItem, parent: Option<&Rc<TestItemTreeElement>>, kind: TestItemElementKind) -> Rc<Self> {
        let element = Rc::new(Self {
            changed: Emitter::new(),
            children: RefCell::new(Vec::new()),
            tree_id: get_id(),
            depth: Cell::new(parent.map_or(0, |p| p.depth.get() + 1)),
            retired: Cell::new(false),
            state: Cell::new(TestResultState::Unset),
            duration: Cell::new(None),
            test: RefCell::new(test.clone()),
            parent: parent.map(Rc::downgrade),
            kind,
            error_child: RefCell::new(None),
            large_children_states: RefCell::new(None),
        });
        element.update_error_visibility(None);
        element
    }

    /// `new ListTestItemElement(test, parent, chain)`.
    pub fn new_list(test: &InternalTestItem, parent: Option<&Rc<TestItemTreeElement>>, chain: Vec<InternalTestItem>) -> Rc<Self> {
        Self::new(test, parent, TestItemElementKind::List(ListItemData { chain, description_parts: RefCell::new(Vec::new()) }))
    }

    /// `new TreeTestItemElement(test, parent, addedOrRemoved)`.
    pub fn new_tree(test: &InternalTestItem, parent: Option<&Rc<TestItemTreeElement>>, added_or_removed: Rc<dyn Fn(&Rc<TestItemTreeElement>)>) -> Rc<Self> {
        Self::new(
            test,
            parent,
            TestItemElementKind::Tree(TreeItemData { own_state: Cell::new(TestResultState::Unset), own_duration: Cell::new(None), added_or_removed }),
        )
    }

    /// Os dados próprios da subclasse da árvore (`TreeTestItemElement`); lança para um elemento da lista.
    pub fn tree_data(&self) -> &TreeItemData {
        match &self.kind {
            TestItemElementKind::Tree(tree) => tree,
            TestItemElementKind::List(_) => throw(Error::type_error("o elemento não é um TreeTestItemElement")),
        }
    }

    /// Fired whenever the element or test properties change.
    pub fn on_change(&self) -> Event<()> {
        self.changed.event()
    }

    pub fn parent(&self) -> Option<Rc<TestItemTreeElement>> {
        self.parent.as_ref().and_then(Weak::upgrade)
    }

    /// Tree element description.
    pub fn description(&self) -> Option<String> {
        match &self.kind {
            TestItemElementKind::List(list) => Some(list.chain.iter().map(|c| c.item.label.as_str()).collect::<Vec<_>>().join(FLAT_TEST_ITEM_DELIMITER)),
            TestItemElementKind::Tree(_) => self.test.borrow().item.description.clone(),
        }
    }

    /// `children` (o `Set` em ordem de inserção), como um instantâneo.
    pub fn children(&self) -> Vec<TestExplorerTreeElement> {
        self.children.borrow().clone()
    }

    /// `children.size`.
    pub fn children_size(&self) -> usize {
        self.children.borrow().len()
    }

    /// `children.add(child)`: um elemento já presente mantém a posição.
    pub fn add_child(&self, child: TestExplorerTreeElement) {
        let mut children = self.children.borrow_mut();
        if !children.contains(&child) {
            children.push(child);
        }
    }

    /// `children.delete(child)`.
    pub fn delete_child(&self, child: &TestExplorerTreeElement) {
        self.children.borrow_mut().retain(|c| c != child);
    }

    pub fn update(self: &Rc<Self>, patch: &ITestItemUpdate) {
        apply_test_item_update(&mut self.test.borrow_mut(), patch);
        self.update_error_visibility(Some(patch));
        self.fire_change();
    }

    pub fn fire_change(&self) {
        self.changed.fire(());
    }

    /// O aviso da árvore de que os filhos do item mudaram (na lista, não há quem avise).
    fn notify_added_or_removed(self: &Rc<Self>) {
        if let TestItemElementKind::Tree(tree) = &self.kind {
            (tree.added_or_removed)(self);
        }
    }

    fn update_error_visibility(self: &Rc<Self>, patch: Option<&ITestItemUpdate>) {
        let patched_error = patch
            .and_then(|p| p.item.as_ref())
            .and_then(|item| item.error.as_ref())
            .is_some_and(|error| truthy_error(error.as_ref()).is_some());
        let current_error = truthy_error(self.test.borrow().item.error.as_ref());

        let previous = self.error_child.borrow().clone();
        if let Some(previous) = previous
            && (current_error.is_none() || patched_error)
        {
            self.notify_added_or_removed();
            self.delete_child(&TestExplorerTreeElement::Error(previous));
            *self.error_child.borrow_mut() = None;
        }

        if let Some(error) = current_error
            && self.error_child.borrow().is_none()
        {
            let child = Rc::new(TestTreeErrorMessage::new(error, self));
            *self.error_child.borrow_mut() = Some(child.clone());
            self.add_child(TestExplorerTreeElement::Error(child));
            self.notify_added_or_removed();
        }
    }

    pub fn to_json(&self) -> TestItemTreeElementJson {
        if self.depth.get() == 0 {
            return TestItemTreeElementJson::Root { controller_id: self.test.borrow().controller_id.clone() };
        }

        let mut tests: Vec<InternalTestItemSerialized> = vec![InternalTestItem::serialize(&self.test.borrow())];
        let mut parent = self.parent();
        while let Some(p) = parent {
            if p.depth.get() == 0 {
                break;
            }
            tests.insert(0, InternalTestItem::serialize(&p.test.borrow()));
            parent = p.parent();
        }

        TestItemTreeElementJson::Context(ITestItemContext { mid: MarshalledId::TestItemContext, tests })
    }
}

pub struct TestTreeErrorMessage {
    pub tree_id: String,
    pub message: TestMessageText,
    parent: Weak<TestItemTreeElement>,
}

impl TestTreeErrorMessage {
    pub fn new(message: TestMessageText, parent: &Rc<TestItemTreeElement>) -> Self {
        Self { tree_id: get_id(), message, parent: Rc::downgrade(parent) }
    }

    pub fn parent(&self) -> Option<Rc<TestItemTreeElement>> {
        self.parent.upgrade()
    }

    pub fn description(&self) -> String {
        match &self.message {
            TestMessageText::Text(text) => text.clone(),
            TestMessageText::Markdown(markdown) => markdown.value.to_string(),
        }
    }
}

/// `TestExplorerTreeElement = TestItemTreeElement | TestTreeErrorMessage`.
#[derive(Clone)]
pub enum TestExplorerTreeElement {
    Item(Rc<TestItemTreeElement>),
    Error(Rc<TestTreeErrorMessage>),
}

impl TestExplorerTreeElement {
    fn address(&self) -> usize {
        match self {
            TestExplorerTreeElement::Item(item) => Rc::as_ptr(item) as usize,
            TestExplorerTreeElement::Error(error) => Rc::as_ptr(error) as usize,
        }
    }

    pub fn tree_id(&self) -> &str {
        match self {
            TestExplorerTreeElement::Item(item) => &item.tree_id,
            TestExplorerTreeElement::Error(error) => &error.tree_id,
        }
    }

    /// `children` (o do erro é sempre vazio).
    pub fn children(&self) -> Vec<TestExplorerTreeElement> {
        match self {
            TestExplorerTreeElement::Item(item) => item.children(),
            TestExplorerTreeElement::Error(_) => Vec::new(),
        }
    }

    pub fn description(&self) -> Option<String> {
        match self {
            TestExplorerTreeElement::Item(item) => item.description(),
            TestExplorerTreeElement::Error(error) => Some(error.description()),
        }
    }

    /// `element instanceof TestItemTreeElement`.
    pub fn as_item(&self) -> Option<&Rc<TestItemTreeElement>> {
        match self {
            TestExplorerTreeElement::Item(item) => Some(item),
            TestExplorerTreeElement::Error(_) => None,
        }
    }
}

impl PartialEq for TestExplorerTreeElement {
    fn eq(&self, other: &Self) -> bool {
        self.address() == other.address()
    }
}

impl Eq for TestExplorerTreeElement {}

impl Hash for TestExplorerTreeElement {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.address().hash(state);
    }
}

/// `testIdentityProvider`.
pub struct TestIdentityProvider;

impl IIdentityProvider<TestExplorerTreeElement> for TestIdentityProvider {
    fn get_id(&self, element: &TestExplorerTreeElement) -> String {
        // For "not expandable" elements, whether they have children is part of the
        // ID so they're rerendered if that changes (#204805)
        let expand_component = match element {
            TestExplorerTreeElement::Error(_) => "error".to_string(),
            TestExplorerTreeElement::Item(item) => {
                let expand = item.test.borrow().expand;
                if expand == TestItemExpandState::NotExpandable { (item.children_size() != 0).to_string() } else { (expand as u8).to_string() }
            }
        };

        format!("{}\0{}", element.tree_id(), expand_component)
    }
}

/// `rootsWithChildren` das duas projeções: as raízes da coleção que têm elemento na árvore e filhos.
pub fn roots_with_children(collection: &dyn IMainThreadTestCollection, items: &IndexMap<String, Rc<TestItemTreeElement>>) -> Vec<Rc<TestItemTreeElement>> {
    collection
        .root_items()
        .iter()
        .filter_map(|root| items.get(&root.borrow().item.ext_id))
        .filter(|root| root.children_size() != 0)
        .cloned()
        .collect()
}

pub fn get_children_for_parent(
    serialized: &ISerializedTestTreeCollapseState,
    roots_with_children: &[Rc<TestItemTreeElement>],
    node: Option<&TestExplorerTreeElement>,
) -> Vec<IObjectTreeElement<TestExplorerTreeElement>> {
    let it: Vec<TestExplorerTreeElement> = match node {
        // roots
        None => {
            if let [only] = roots_with_children {
                return get_children_for_parent(serialized, roots_with_children, Some(&TestExplorerTreeElement::Item(only.clone())));
            }
            roots_with_children.iter().cloned().map(TestExplorerTreeElement::Item).collect()
        }
        Some(node) => node.children(),
    };

    it.into_iter()
        .map(|element| match &element {
            TestExplorerTreeElement::Error(_) => IObjectTreeElement::new(element.clone()),
            TestExplorerTreeElement::Item(item) => {
                let (expand, has_error, ext_id) = {
                    let test = item.test.borrow();
                    (test.expand, truthy_error(test.item.error.as_ref()).is_some(), test.item.ext_id.clone())
                };
                let collapsed = if has_error {
                    ObjectTreeElementCollapseState::PreserveOrExpanded
                } else if is_collapsed_in_serialized_test_tree(serialized, &ext_id).unwrap_or(item.depth.get() > 0) {
                    ObjectTreeElementCollapseState::PreserveOrCollapsed
                } else {
                    ObjectTreeElementCollapseState::PreserveOrExpanded
                };

                IObjectTreeElement {
                    element: element.clone(),
                    collapsible: Some(expand != TestItemExpandState::NotExpandable),
                    collapsed: Some(CollapsedValue::State(collapsed)),
                    children: Some(get_children_for_parent(serialized, roots_with_children, Some(&element))),
                }
            }
        })
        .collect()
}

/// O nó da coleção da thread principal como `InternalTestItem` (o original os usa como o mesmo tipo estrutural).
pub fn internal_test_item_of(node: &IncrementalNodeRef<IncrementalTestCollectionItem>) -> InternalTestItem {
    let node = node.borrow();
    InternalTestItem { controller_id: node.controller_id.clone(), expand: node.expand, item: node.item.clone() }
}

/// O início do `onTestChanged` das projeções: o resultado a refletir na árvore, ou `None` se a mudança não tem efeito
/// nela (uma mensagem nova).
pub fn latest_state_for_change(results: &dyn ITestResultService, ev: &TestResultItemChange) -> Option<TestResultItemRef> {
    if ev.reason() == TestResultItemChangeReason::NewMessage {
        return None; // no effect in the tree
    }

    let mut result = ev.item.clone();
    // if the state is unset, or the latest run is not making the change,
    // double check that it's valid. Retire calls might cause previous
    // emit a state change for a test run that's already long completed.
    let is_latest_run = results.results().first().is_some_and(|latest| id_of(latest) == id_of(&ev.result));
    if result.data.borrow().own_computed_state == TestResultState::Unset || !is_latest_run {
        let ext_id = result.data.borrow().item.ext_id.clone();
        if let Some((_, fallback)) = results.get_state_by_id(&ext_id) {
            result = fallback;
        }
    }

    Some(result)
}
