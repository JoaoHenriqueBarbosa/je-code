//! Porte de `src/vs/workbench/contrib/testing/common/mainThreadTestCollection.ts`: a coleção de testes da thread
//! principal, montada incrementalmente a partir dos diffs dos controladores, com índice por URL e cache das expansões.
//!
//! Divergências:
//! - **`extends AbstractIncrementalTestCollection`:** a base é o campo `base` (os membros abstratos `createItem` e
//!   `createChangeCollector` são os [`IncrementalCollectionHooks`] de [`MainThreadHooks`]). `apply` repete a base e dispara
//!   `onBusyProvidersChange`, como o `override` do original.
//! - **`WeakMap<IncrementalTestCollectionItem, ...>` (`expandPromises`)** é um mapa pelo endereço do nó, cujo registro
//!   guarda um `Weak` do nó para a conferência de que o endereço ainda é o do mesmo objeto.
//! - **`Set<IncrementalTestCollectionItem>` de `testsByUrl`** é um `Vec` sem repetição por identidade, na ordem de inserção.
//! - **Iteráveis** (`rootIds`, `all`, `getNodeByUrl`) são `Vec`.
//! - **`expandActual`** é `Rc<dyn Fn(&str, f64) -> Promise<()>>`.

use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::rc::{Rc, Weak};

use indexmap::IndexSet;
use je_base::event::{Emitter, Event};
use je_base::map::ResourceMap;
use je_base::promise::Promise;
use je_base::uri::URI;

use super::test_service::IMainThreadTestCollection;
use super::test_types::{
    AbstractIncrementalTestCollection, IncrementalChangeCollector, IncrementalCollectionHooks, IncrementalNodeRef, IncrementalTestCollectionItem, InternalTestItem,
    ITestUriCanonicalizer, TestsDiff, TestsDiffOp,
};

type Node = IncrementalNodeRef<IncrementalTestCollectionItem>;
type TestsByUrl = Rc<RefCell<ResourceMap<Vec<Node>>>>;

/// O valor de `expandPromises`.
struct ExpandRecord {
    pending_lvl: f64,
    done_lvl: Rc<Cell<f64>>,
    prom: Promise<()>,
    node: Weak<RefCell<IncrementalTestCollectionItem>>,
}

/// Os membros abstratos e o `changeCollector` da subclasse.
struct MainThreadHooks {
    tests_by_url: TestsByUrl,
}

impl IncrementalCollectionHooks<IncrementalTestCollectionItem> for MainThreadHooks {
    /// `{ ...internal, children: new Set() }`
    fn create_item(&self, internal: &InternalTestItem, _parent: Option<&Node>) -> IncrementalTestCollectionItem {
        IncrementalTestCollectionItem {
            controller_id: internal.controller_id.clone(),
            expand: internal.expand,
            item: internal.item.clone(),
            children: IndexSet::new(),
        }
    }

    fn create_change_collector(&self) -> Rc<dyn IncrementalChangeCollector<IncrementalTestCollectionItem>> {
        Rc::new(MainThreadChangeCollector { tests_by_url: self.tests_by_url.clone() })
    }
}

struct MainThreadChangeCollector {
    tests_by_url: TestsByUrl,
}

impl IncrementalChangeCollector<IncrementalTestCollectionItem> for MainThreadChangeCollector {
    fn add(&self, node: &Node) {
        let Some(uri) = node.borrow().item.uri.clone() else {
            return;
        };

        let mut tests_by_url = self.tests_by_url.borrow_mut();
        let mut nodes = tests_by_url.get(&uri).cloned().unwrap_or_default();
        if !nodes.iter().any(|n| Rc::ptr_eq(n, node)) {
            nodes.push(node.clone());
        }
        tests_by_url.set(uri, nodes);
    }

    fn remove(&self, node: &Node, _is_nested_operation: bool) {
        let Some(uri) = node.borrow().item.uri.clone() else {
            return;
        };

        let mut tests_by_url = self.tests_by_url.borrow_mut();
        let Some(mut nodes) = tests_by_url.get(&uri).cloned() else {
            return;
        };

        nodes.retain(|n| !Rc::ptr_eq(n, node));
        if nodes.is_empty() {
            tests_by_url.delete(&uri);
        } else {
            tests_by_url.set(uri, nodes);
        }
    }
}

pub struct MainThreadTestCollection {
    pub base: AbstractIncrementalTestCollection<IncrementalTestCollectionItem>,
    tests_by_url: TestsByUrl,
    busy_providers_change_emitter: Rc<Emitter<i32>>,
    expand_promises: RefCell<HashMap<usize, ExpandRecord>>,
    expand_actual: Rc<dyn Fn(&str, f64) -> Promise<()>>,
}

impl MainThreadTestCollection {
    pub fn new(uri_identity_service: Rc<dyn ITestUriCanonicalizer>, expand_actual: Rc<dyn Fn(&str, f64) -> Promise<()>>) -> Self {
        let tests_by_url: TestsByUrl = Rc::new(RefCell::new(ResourceMap::new()));
        let hooks = Rc::new(MainThreadHooks { tests_by_url: tests_by_url.clone() });
        Self {
            base: AbstractIncrementalTestCollection::new(uri_identity_service, hooks),
            tests_by_url,
            busy_providers_change_emitter: Emitter::new(),
            expand_promises: RefCell::new(HashMap::new()),
            expand_actual,
        }
    }

    /// Clears everything from the collection, and returns a diff that applies
    /// that action.
    pub fn clear(&self) -> TestsDiff {
        let mut ops: TestsDiff = Vec::new();
        for root in self.base.roots.borrow().iter() {
            ops.push(TestsDiffOp::Remove { item_id: root.borrow().item.ext_id.clone() });
        }

        self.base.roots.borrow_mut().clear();
        self.base.items.borrow_mut().clear();

        ops
    }
}

impl IMainThreadTestCollection for MainThreadTestCollection {
    fn on_busy_providers_change(&self) -> Event<i32> {
        self.busy_providers_change_emitter.event()
    }

    /// @inheritdoc
    fn busy_providers(&self) -> i32 {
        self.base.busy_controller_count.get()
    }

    /// `rootIds`.
    fn root_ids(&self) -> Vec<String> {
        self.base.roots.borrow().iter().map(|r| r.borrow().item.ext_id.clone()).collect()
    }

    /// @inheritdoc
    fn root_items(&self) -> Vec<Node> {
        self.base.roots.borrow().clone()
    }

    /// @inheritdoc
    fn all(&self) -> Vec<Node> {
        let mut result = Vec::new();
        let mut queue: Vec<Vec<String>> = vec![self.root_ids()];

        while let Some(ids) = queue.pop() {
            for id in ids {
                let node = self.get_node_by_id(&id).expect("o nó listado existe na coleção");
                queue.push(node.borrow().children.iter().cloned().collect());
                result.push(node);
            }
        }

        result
    }

    /// @inheritdoc
    fn expand(&self, test_id: &str, levels: f64) -> Promise<()> {
        let Some(test) = self.base.items.borrow().get(test_id).cloned() else {
            return Promise::resolved(());
        };

        // simple cache to avoid duplicate/unnecessary expansion calls
        let key = Rc::as_ptr(&test) as usize;
        let existing = self
            .expand_promises
            .borrow()
            .get(&key)
            .filter(|record| record.node.upgrade().is_some_and(|node| Rc::ptr_eq(&node, &test)))
            .map(|record| (record.pending_lvl, record.done_lvl.get(), record.prom.clone()));
        if let Some((pending_lvl, _, prom)) = &existing
            && *pending_lvl >= levels
        {
            return prom.clone();
        }

        let ext_id = test.borrow().item.ext_id.clone();
        let prom = (self.expand_actual)(&ext_id, levels);
        let done_lvl = Rc::new(Cell::new(existing.map_or(-1.0, |(_, done, _)| done)));
        self.expand_promises
            .borrow_mut()
            .insert(key, ExpandRecord { done_lvl: done_lvl.clone(), pending_lvl: levels, prom: prom.clone(), node: Rc::downgrade(&test) });

        prom.then(move |_| {
            done_lvl.set(levels);
        })
    }

    /// @inheritdoc
    fn get_node_by_id(&self, id: &str) -> Option<Node> {
        self.base.items.borrow().get(id).cloned()
    }

    /// @inheritdoc
    fn get_node_by_url(&self, uri: &URI) -> Vec<Node> {
        self.tests_by_url.borrow().get(uri).cloned().unwrap_or_default()
    }

    /// @inheritdoc
    fn get_reviver_diff(&self) -> TestsDiff {
        let mut ops: TestsDiff = vec![TestsDiffOp::IncrementPendingExtHosts { amount: self.base.pending_root_count.get() }];

        let mut queue: Vec<Vec<String>> = vec![self.root_ids()];
        while let Some(children) = queue.pop() {
            for child in children {
                let item = self.base.items.borrow().get(&child).cloned().expect("o filho listado existe na coleção");
                let item = item.borrow();
                ops.push(TestsDiffOp::Add {
                    item: InternalTestItem { controller_id: item.controller_id.clone(), expand: item.expand, item: item.item.clone() },
                });
                queue.push(item.children.iter().cloned().collect());
            }
        }

        ops
    }

    /// Applies the diff to the collection.
    fn apply(&self, diff: &TestsDiff) {
        let prev_busy = self.base.busy_controller_count.get();
        self.base.apply(diff);

        let busy = self.base.busy_controller_count.get();
        if prev_busy != busy {
            self.busy_providers_change_emitter.fire(busy);
        }
    }
}
