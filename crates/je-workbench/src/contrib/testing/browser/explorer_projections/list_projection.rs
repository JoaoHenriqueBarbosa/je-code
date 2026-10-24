//! Porte de `src/vs/workbench/contrib/testing/browser/explorerProjections/listProjection.ts`: a projeção que lista os
//! testes por nome, numa lista plana (cada folha com a cadeia de pais na descrição).
//!
//! Divergências:
//! - **`ListTestItemElement`** é um [`TestItemTreeElement`] do tipo `List` (ver `index.rs`).
//! - **`'removed' in evt`** é o `ResultChangeEvent::Removed`.
//! - **`items: Map`** é um `IndexMap` (a ordem de inserção do `Map`); nenhuma célula fica emprestada durante uma chamada
//!   que pode reentrar (`expand` dispara o diff de volta).
//! - **`diffDepth: Infinity`** é `usize::MAX`.

use std::cell::RefCell;
use std::rc::Rc;

use indexmap::IndexMap;
use je_base::event::{Emitter, Event};
use je_base::filters::FuzzyScore;
use je_base::lifecycle::{Disposable, IDisposable};
use je_widgets::tree::object_tree::ObjectTree;
use je_widgets::tree::object_tree_model::ObjectTreeModelSetChildrenOptions;

use super::index::{
    ITestTreeProjection, TestExplorerTreeElement, TestIdentityProvider, TestItemElementKind, TestItemTreeElement, get_children_for_parent, internal_test_item_of,
    latest_state_for_change, roots_with_children,
};
use super::testing_view_state::{ISerializedTestTreeCollapseState, is_collapsed_in_serialized_test_tree};
use crate::contrib::testing::common::test_id::TestId;
use crate::contrib::testing::common::test_result_service::{ITestResultService, ResultChangeEvent};
use crate::contrib::testing::common::test_service::ITestService;
use crate::contrib::testing::common::test_types::{InternalTestItem, TestItemExpandState, TestResultState, TestsDiff, TestsDiffOp};

/// Projection that lists tests in their traditional tree view.
pub struct ListProjection {
    base: Rc<Disposable>,
    update_emitter: Rc<Emitter<()>>,
    items: RefCell<IndexMap<String, Rc<TestItemTreeElement>>>,
    last_state: RefCell<ISerializedTestTreeCollapseState>,
    test_service: Rc<dyn ITestService>,
    results: Rc<dyn ITestResultService>,
}

impl ListProjection {
    pub fn new(last_state: ISerializedTestTreeCollapseState, test_service: Rc<dyn ITestService>, results: Rc<dyn ITestResultService>) -> Rc<Self> {
        let base = Disposable::new();
        let update_emitter = Emitter::new();
        base.register(update_emitter.clone());

        let this = Rc::new(Self {
            base: base.clone(),
            update_emitter,
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

                let in_tree: Vec<Rc<TestItemTreeElement>> = this.items.borrow().values().cloned().collect();
                for in_tree in in_tree {
                    // Simple logic here, because we know in this projection states
                    // are never inherited.
                    let ext_id = in_tree.test.borrow().item.ext_id.clone();
                    let lookup = this.results.get_state_by_id(&ext_id).map(|(_, item)| item);
                    in_tree.duration.set(lookup.as_ref().and_then(|l| l.data.borrow().own_duration));
                    in_tree.state.set(lookup.map_or(TestResultState::Unset, |l| l.data.borrow().own_computed_state));
                    in_tree.fire_change();
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

                {
                    let data = result.data.borrow();
                    item.retired.set(data.retired.unwrap_or(false));
                    item.state.set(data.computed_state);
                    item.duration.set(data.own_duration);
                }
                item.fire_change();
            }
        }));

        for test in test_service.collection().all() {
            this.store_item(&internal_test_item_of(&test));
        }

        this
    }

    /// `applyDiff`.
    fn apply_diff(&self, diff: &TestsDiff) {
        for op in diff {
            match op {
                TestsDiffOp::Add { item } => self.store_item(item),

                TestsDiffOp::Update { item } => {
                    let existing = self.items.borrow().get(&item.ext_id).cloned();
                    if let Some(existing) = existing {
                        existing.update(item);
                    }
                }

                TestsDiffOp::Remove { item_id } => {
                    let all: Vec<(String, Rc<TestItemTreeElement>)> = self.items.borrow().iter().map(|(id, item)| (id.clone(), item.clone())).collect();
                    for (id, item) in all {
                        if &id == item_id || TestId::is_child(item_id, &id) {
                            self.unstore_item(&item);
                        }
                    }
                }

                _ => {}
            }
        }

        if !diff.is_empty() {
            self.update_emitter.fire(());
        }
    }

    fn roots_with_children(&self) -> Vec<Rc<TestItemTreeElement>> {
        roots_with_children(self.test_service.collection().as_ref(), &self.items.borrow())
    }

    fn unstore_item(&self, tree_element: &Rc<TestItemTreeElement>) {
        let ext_id = tree_element.test.borrow().item.ext_id.clone();
        self.items.borrow_mut().shift_remove(&ext_id);
        if let Some(parent) = tree_element.parent() {
            parent.delete_child(&TestExplorerTreeElement::Item(tree_element.clone()));
        }

        let Some(parent_id) = TestId::from_string(&ext_id).parent_id() else { return };

        // create the parent if it's now its own leaf
        for id in parent_id.ids_to_root() {
            let id_string = id.to_string();
            if let Some(parent_test) = self.test_service.collection().get_node_by_id(&id_string) {
                let is_leaf = parent_test.borrow().children.is_empty();
                if is_leaf && !self.items.borrow().contains_key(&id_string) {
                    self.store_item_at(&parent_id, &internal_test_item_of(&parent_test));
                }
                break;
            }
        }
    }

    /// `_storeItem`.
    fn store_item_at(&self, test_id: &TestId, item: &InternalTestItem) {
        let displayed_parent = if test_id.is_root() { None } else { self.items.borrow().get(&item.controller_id).cloned() };
        let ids = test_id.ids_from_root();
        let chain_ids = if ids.len() >= 2 { &ids[1..ids.len() - 1] } else { &[] };
        let collection = self.test_service.collection();
        let chain: Vec<InternalTestItem> = chain_ids
            .iter()
            .map(|id| internal_test_item_of(&collection.get_node_by_id(&id.to_string()).expect("o pai da cadeia existe na coleção")))
            .collect();

        let tree_element = TestItemTreeElement::new_list(item, displayed_parent.as_ref(), chain);
        if let Some(parent) = &displayed_parent {
            parent.add_child(TestExplorerTreeElement::Item(tree_element.clone()));
        }
        let ext_id = tree_element.test.borrow().item.ext_id.clone();
        self.items.borrow_mut().insert(ext_id.clone(), tree_element.clone());

        let last_state = self.last_state.borrow().clone();
        if tree_element.depth.get() == 0 || is_collapsed_in_serialized_test_tree(&last_state, &ext_id) == Some(false) {
            self.expand_element(&tree_element, f64::INFINITY);
        }

        if let Some((_, prev_state)) = self.results.get_state_by_id(&ext_id) {
            let data = prev_state.data.borrow();
            tree_element.retired.set(data.retired.unwrap_or(false));
            tree_element.state.set(data.computed_state);
            tree_element.duration.set(data.own_duration);
        }
    }

    fn store_item(&self, item: &InternalTestItem) {
        let test_id = TestId::from_string(&item.item.ext_id);

        // Remove any non-root parent of this item which is no longer a leaf.
        for parent_id in test_id.ids_to_root() {
            if !parent_id.is_root() {
                let prev_parent = self.items.borrow().get(&parent_id.to_string()).cloned();
                if let Some(prev_parent) = prev_parent {
                    self.unstore_item(&prev_parent);
                    break;
                }
            }
        }

        self.store_item_at(&test_id, item);
    }
}

impl ITestTreeProjection for ListProjection {
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
        // We don't bother doing a very specific update like we do in the TreeProjection.
        // It's a flat list, so chances are we need to render everything anyway.
        // Let the diffIdentityProvider handle that.
        let children = get_children_for_parent(&self.last_state.borrow(), &self.roots_with_children(), None);
        tree.set_children(
            &None,
            children,
            ObjectTreeModelSetChildrenOptions { diff_depth: Some(usize::MAX), diff_identity_provider: Some(Rc::new(TestIdentityProvider)), ..Default::default() },
        );
    }

    fn expand_element(&self, element: &Rc<TestItemTreeElement>, depth: f64) {
        if !matches!(element.kind, TestItemElementKind::List(_)) {
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

impl IDisposable for ListProjection {
    fn dispose(&self) {
        self.base.dispose();
    }
}
