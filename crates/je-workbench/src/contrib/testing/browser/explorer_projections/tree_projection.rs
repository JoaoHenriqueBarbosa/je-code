//! Porte de `src/vs/workbench/contrib/testing/browser/explorerProjections/treeProjection.ts`: a projeção que mostra os
//! testes na hierarquia da coleção, com o estado e a duração calculados a partir dos filhos.
//!
//! Divergências:
//! - **`TreeTestItemElement`** é um [`TestItemTreeElement`] do tipo `Tree` (ver `index.rs`); `ownState` e `ownDuration` são
//!   os campos de [`super::index::TreeItemData`].
//! - **`computedStateAccessor`** é o [`TreeComputedStateAccessor`], com as mesmas oito funções; `getChildren` devolve os
//!   filhos que são itens (os de erro ficam de fora), e `getParents` a cadeia de pais até a raiz.
//! - **`changedParents` e `resortedParents`** (`Set<TestItemTreeElement | null>`) são `IndexSet<Option<TestExplorerTreeElement>>`
//!   (igualdade por identidade). `applyTo` os percorre por índice, como o `Set` vivo do JavaScript, e depois os esvazia. O
//!   fecho `addedOrRemoved` dos elementos guarda a célula por `Weak` (o conjunto guarda os elementos).
//! - **`!!parent.duration`** é [`truthy_duration`] (zero e `NaN` são falsos).
//! - **`patch.expand` em `needsParentUpdate`** é verdadeiro só para um estado diferente de `NotExpandable` (que vale 0).
//! - **`Iterable.some(this.rootsWithChildren, (_, i) => i === 1)`** é "há duas ou mais raízes com filhos".
//! - **`items: Map`** é um `IndexMap`; nenhuma célula fica emprestada durante uma chamada que pode reentrar.

use std::cell::RefCell;
use std::rc::{Rc, Weak};

use indexmap::{IndexMap, IndexSet};
use je_base::event::{Emitter, Event};
use je_base::filters::FuzzyScore;
use je_base::lifecycle::{Disposable, IDisposable};
use je_widgets::tree::object_tree::{ObjectTree, ObjectTreeMethods};
use je_widgets::tree::object_tree_model::ObjectTreeModelSetChildrenOptions;

use super::index::{
    ITestTreeProjection, TestExplorerTreeElement, TestIdentityProvider, TestItemElementKind, TestItemTreeElement, get_children_for_parent, internal_test_item_of,
    latest_state_for_change, roots_with_children,
};
use super::testing_view_state::{ISerializedTestTreeCollapseState, is_collapsed_in_serialized_test_tree};
use crate::contrib::testing::common::get_computed_state::{IComputedStateAccessor, IComputedStateAndDurationAccessor, refresh_computed_state};
use crate::contrib::testing::common::test_id::TestId;
use crate::contrib::testing::common::test_result::TestResultItemChangeKind;
use crate::contrib::testing::common::test_result_service::{ITestResultService, ResultChangeEvent};
use crate::contrib::testing::common::test_service::ITestService;
use crate::contrib::testing::common::test_types::{InternalTestItem, TestItemExpandState, TestResultState, TestsDiff, TestsDiffOp};

type ParentSet = RefCell<IndexSet<Option<TestExplorerTreeElement>>>;

/// `!!duration`.
fn truthy_duration(duration: Option<f64>) -> bool {
    duration.is_some_and(|d| d != 0.0 && !d.is_nan())
}

/// O `computedStateAccessor` da projeção.
struct TreeComputedStateAccessor;

impl IComputedStateAccessor<Rc<TestItemTreeElement>> for TreeComputedStateAccessor {
    fn get_own_state(&self, item: &Rc<TestItemTreeElement>) -> Option<TestResultState> {
        Some(item.tree_data().own_state.get())
    }

    fn get_current_computed_state(&self, item: &Rc<TestItemTreeElement>) -> Option<TestResultState> {
        Some(item.state.get())
    }

    fn set_computed_state(&self, item: &Rc<TestItemTreeElement>, state: TestResultState) {
        item.state.set(state);
    }

    fn get_children(&self, item: &Rc<TestItemTreeElement>) -> Vec<Rc<TestItemTreeElement>> {
        item.children().iter().filter_map(|child| child.as_item().cloned()).collect()
    }

    fn get_parents(&self, item: &Rc<TestItemTreeElement>) -> Vec<Rc<TestItemTreeElement>> {
        let mut parents = Vec::new();
        let mut parent = item.parent();
        while let Some(p) = parent {
            parent = p.parent();
            parents.push(p);
        }
        parents
    }

    fn as_duration_accessor(&self) -> Option<&dyn IComputedStateAndDurationAccessor<Rc<TestItemTreeElement>>> {
        Some(self)
    }
}

impl IComputedStateAndDurationAccessor<Rc<TestItemTreeElement>> for TreeComputedStateAccessor {
    fn get_own_duration(&self, item: &Rc<TestItemTreeElement>) -> Option<f64> {
        item.tree_data().own_duration.get()
    }

    fn get_current_computed_duration(&self, item: &Rc<TestItemTreeElement>) -> Option<f64> {
        item.duration.get()
    }

    fn set_computed_duration(&self, item: &Rc<TestItemTreeElement>, duration: Option<f64>) {
        item.duration.set(duration);
    }
}

/// `refreshComputedState(...).forEach(i => i.fireChange())`.
fn refresh_and_notify(node: &Rc<TestItemTreeElement>, explicit_new_computed_state: Option<TestResultState>, refresh_duration: bool) {
    for changed in refresh_computed_state(&TreeComputedStateAccessor, node, explicit_new_computed_state, refresh_duration) {
        changed.fire_change();
    }
}

/// Projection that lists tests in their traditional tree view.
pub struct TreeProjection {
    base: Rc<Disposable>,
    update_emitter: Rc<Emitter<()>>,
    changed_parents: Rc<ParentSet>,
    resorted_parents: ParentSet,
    items: RefCell<IndexMap<String, Rc<TestItemTreeElement>>>,
    last_state: RefCell<ISerializedTestTreeCollapseState>,
    test_service: Rc<dyn ITestService>,
    results: Rc<dyn ITestResultService>,
}

impl TreeProjection {
    pub fn new(last_state: ISerializedTestTreeCollapseState, test_service: Rc<dyn ITestService>, results: Rc<dyn ITestResultService>) -> Rc<Self> {
        let base = Disposable::new();
        let update_emitter = Emitter::new();
        base.register(update_emitter.clone());

        let this = Rc::new(Self {
            base: base.clone(),
            update_emitter,
            changed_parents: Rc::new(RefCell::new(IndexSet::new())),
            resorted_parents: RefCell::new(IndexSet::new()),
            items: RefCell::new(IndexMap::new()),
            last_state: RefCell::new(last_state),
            test_service: test_service.clone(),
            results: results.clone(),
        });

        let weak = Rc::downgrade(&this);
        base.register(test_service.on_did_process_diff().listen({
            let weak = weak.clone();
            move |diff| {
                if let Some(this) = weak.upgrade() {
                    this.apply_diff(diff);
                }
            }
        }));

        // when test results are cleared, recalculate all state
        base.register(results.on_results_changed().listen({
            let weak = weak.clone();
            move |evt| {
                let Some(this) = weak.upgrade() else { return };
                if !matches!(evt, ResultChangeEvent::Removed(_)) {
                    return;
                }

                let mut in_trees: Vec<Rc<TestItemTreeElement>> = this.items.borrow().values().cloned().collect();
                in_trees.sort_by(|a, b| b.depth.get().cmp(&a.depth.get()));
                for in_tree in in_trees {
                    let ext_id = in_tree.test.borrow().item.ext_id.clone();
                    let lookup = this.results.get_state_by_id(&ext_id).map(|(_, item)| item);
                    in_tree.tree_data().own_duration.set(lookup.as_ref().and_then(|l| l.data.borrow().own_duration));
                    refresh_and_notify(&in_tree, Some(lookup.map_or(TestResultState::Unset, |l| l.data.borrow().own_computed_state)), true);
                }
            }
        }));

        // when test states change, reflect in the tree
        base.register(results.on_test_changed().listen({
            let weak = weak.clone();
            move |ev| {
                let Some(this) = weak.upgrade() else { return };
                let Some(result) = latest_state_for_change(this.results.as_ref(), ev) else { return };

                let ext_id = result.data.borrow().item.ext_id.clone();
                let Some(item) = this.items.borrow().get(&ext_id).cloned() else { return };

                let (own_duration, own_computed_state, computed_state, retired) = {
                    let data = result.data.borrow();
                    (data.own_duration, data.own_computed_state, data.computed_state, data.retired.unwrap_or(false))
                };

                // Skip refreshing the duration if we can trivially tell it didn't change.
                let refresh_duration = matches!(&ev.change, TestResultItemChangeKind::OwnStateChange { previous_own_duration, .. } if *previous_own_duration != own_duration);
                // For items without children, always use the computed state. They are
                // either leaves (for which it's fine) or nodes where we haven't expanded
                // children and should trust whatever the result service gives us.
                let explicit_computed = if item.children_size() != 0 { None } else { Some(computed_state) };

                item.retired.set(retired);
                item.tree_data().own_state.set(own_computed_state);
                item.tree_data().own_duration.set(own_duration);
                item.fire_change();

                refresh_and_notify(&item, explicit_computed, refresh_duration);
            }
        }));

        for test in test_service.collection().all() {
            let item = this.create_item(&internal_test_item_of(&test));
            this.store_item(&item);
        }

        this
    }

    /// `rootsWithChildren`.
    fn roots_with_children(&self) -> Vec<Rc<TestItemTreeElement>> {
        roots_with_children(self.test_service.collection().as_ref(), &self.items.borrow())
    }

    fn add_changed_parent(&self, parent: Option<Rc<TestItemTreeElement>>) {
        self.changed_parents.borrow_mut().insert(parent.map(TestExplorerTreeElement::Item));
    }

    /// `applyDiff`.
    fn apply_diff(&self, diff: &TestsDiff) {
        for op in diff {
            match op {
                TestsDiffOp::Add { item } => {
                    let item = self.create_item(item);
                    self.store_item(&item);
                }

                TestsDiffOp::Update { item: patch } => {
                    let existing = self.items.borrow().get(&patch.ext_id).cloned();
                    let Some(existing) = existing else { continue };

                    // parent needs to be re-rendered on an expand update, so that its
                    // children are rewritten.
                    let needs_parent_update =
                        existing.test.borrow().expand == TestItemExpandState::NotExpandable && patch.expand.is_some_and(|expand| expand != TestItemExpandState::NotExpandable);
                    existing.update(patch);
                    let parent = existing.parent().map(TestExplorerTreeElement::Item);
                    if needs_parent_update {
                        self.changed_parents.borrow_mut().insert(parent);
                    } else {
                        self.resorted_parents.borrow_mut().insert(parent);
                    }
                }

                TestsDiffOp::Remove { item_id } => {
                    let to_remove = self.items.borrow().get(item_id).cloned();
                    let Some(to_remove) = to_remove else { continue };

                    // Removing the first element will cause the root to be hidden.
                    // Changing first-level elements will need the root to re-render if
                    // there are no other controllers with items.
                    let parent = to_remove.parent();
                    let affects_root_element = to_remove.depth.get() == 1 && (parent.as_ref().is_some_and(|p| p.children_size() == 1) || self.roots_with_children().len() < 2);
                    self.add_changed_parent(if affects_root_element { None } else { parent.clone() });

                    let mut queue: Vec<Vec<TestExplorerTreeElement>> = vec![vec![TestExplorerTreeElement::Item(to_remove)]];
                    while let Some(batch) = queue.pop() {
                        for item in batch {
                            if let TestExplorerTreeElement::Item(item) = item {
                                queue.push(self.unstore_item(&item));
                            }
                        }
                    }

                    if let Some(parent) = parent {
                        refresh_and_notify(&parent, None, truthy_duration(parent.duration.get()));
                    }
                }

                _ => {}
            }
        }

        if !diff.is_empty() {
            self.update_emitter.fire(());
        }
    }

    fn create_item(&self, item: &InternalTestItem) -> Rc<TestItemTreeElement> {
        let parent = TestId::parent_id_string(&item.item.ext_id).and_then(|parent_id| self.items.borrow().get(parent_id).cloned());
        let changed_parents = Rc::downgrade(&self.changed_parents);
        TestItemTreeElement::new_tree(
            item,
            parent.as_ref(),
            Rc::new(move |n: &Rc<TestItemTreeElement>| {
                if let Some(changed_parents) = Weak::upgrade(&changed_parents) {
                    changed_parents.borrow_mut().insert(Some(TestExplorerTreeElement::Item(n.clone())));
                }
            }),
        )
    }

    fn unstore_item(&self, tree_element: &Rc<TestItemTreeElement>) -> Vec<TestExplorerTreeElement> {
        if let Some(parent) = tree_element.parent() {
            parent.delete_child(&TestExplorerTreeElement::Item(tree_element.clone()));
        }
        let ext_id = tree_element.test.borrow().item.ext_id.clone();
        self.items.borrow_mut().shift_remove(&ext_id);
        tree_element.children()
    }

    fn store_item(&self, tree_element: &Rc<TestItemTreeElement>) {
        let parent = tree_element.parent();
        if let Some(parent) = &parent {
            parent.add_child(TestExplorerTreeElement::Item(tree_element.clone()));
        }
        let ext_id = tree_element.test.borrow().item.ext_id.clone();
        self.items.borrow_mut().insert(ext_id.clone(), tree_element.clone());

        // The first element will cause the root to be shown. The first element of
        // a parent may need to re-render it for #204805.
        let affects_parent = parent.as_ref().is_some_and(|p| p.children_size() == 1);
        let affected_parent = if affects_parent { parent.as_ref().and_then(|p| p.parent()) } else { parent };
        let affected_is_root = affected_parent.as_ref().is_some_and(|p| p.depth.get() == 0);
        self.add_changed_parent(affected_parent);
        if affected_is_root {
            self.add_changed_parent(None);
        }

        let last_state = self.last_state.borrow().clone();
        if tree_element.depth.get() == 0 || is_collapsed_in_serialized_test_tree(&last_state, &ext_id) == Some(false) {
            self.expand_element(tree_element, 0.0);
        }

        if let Some((_, prev_state)) = self.results.get_state_by_id(&ext_id) {
            let (retired, computed_state, own_duration) = {
                let data = prev_state.data.borrow();
                (data.retired.unwrap_or(false), data.computed_state, data.own_duration)
            };
            tree_element.retired.set(retired);
            tree_element.tree_data().own_state.set(computed_state);
            tree_element.tree_data().own_duration.set(own_duration);

            refresh_and_notify(tree_element, None, truthy_duration(tree_element.tree_data().own_duration.get()));
        }
    }
}

/// O elemento de `set` na posição `index` (a iteração de um `Set` vivo), se ainda houver.
fn parent_at(set: &ParentSet, index: usize) -> Option<Option<TestExplorerTreeElement>> {
    set.borrow().get_index(index).cloned()
}

impl ITestTreeProjection for TreeProjection {
    fn on_update(&self) -> Event<()> {
        self.update_emitter.event()
    }

    fn last_state(&self) -> ISerializedTestTreeCollapseState {
        self.last_state.borrow().clone()
    }

    fn set_last_state(&self, state: ISerializedTestTreeCollapseState) {
        *self.last_state.borrow_mut() = state;
    }

    fn get_element_by_test_id(&self, test_id: &str) -> Option<Rc<TestItemTreeElement>> {
        self.items.borrow().get(test_id).cloned()
    }

    fn apply_to(&self, tree: &ObjectTree<TestExplorerTreeElement, FuzzyScore>) {
        let mut index = 0;
        while let Some(parent) = parent_at(&self.changed_parents, index) {
            index += 1;
            if parent.as_ref().is_none_or(|p| tree.has_element(p)) {
                let children = get_children_for_parent(&self.last_state.borrow(), &self.roots_with_children(), parent.as_ref());
                tree.set_children(&parent, children, ObjectTreeModelSetChildrenOptions { diff_identity_provider: Some(Rc::new(TestIdentityProvider)), ..Default::default() });
            }
        }

        let mut index = 0;
        while let Some(parent) = parent_at(&self.resorted_parents, index) {
            index += 1;
            if parent.as_ref().is_none_or(|p| tree.has_element(p)) {
                tree.resort(&parent, false);
            }
        }

        self.changed_parents.borrow_mut().clear();
        self.resorted_parents.borrow_mut().clear();
    }

    fn expand_element(&self, element: &Rc<TestItemTreeElement>, depth: f64) {
        if !matches!(element.kind, TestItemElementKind::Tree(_)) {
            return;
        }

        let (expand, ext_id) = {
            let test = element.test.borrow();
            (test.expand, test.item.ext_id.clone())
        };
        if expand == TestItemExpandState::NotExpandable {
            return;
        }

        let _ = self.test_service.collection().expand(&ext_id, depth);
    }
}

impl IDisposable for TreeProjection {
    fn dispose(&self) {
        self.base.dispose();
    }
}
