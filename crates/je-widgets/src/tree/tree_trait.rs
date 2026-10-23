//! Porte do `Trait` de `src/vs/base/browser/ui/tree/abstractTree.ts`: o conjunto de nós com uma
//! característica (foco, seleção, âncora). Existe no nível da árvore porque os nós colapsados não são
//! conhecidos pela lista.

use std::cell::RefCell;
use std::collections::{HashMap, HashSet};
use std::hash::Hash;
use std::rc::Rc;

use je_base::event::{Emitter, Event as BaseEvent};
use je_base::lifecycle::{ById, IDisposable};

use crate::list::list_types::{BrowserEvent, IIdentityProvider};
use crate::tree::tree_types::{ITreeModel, TreeEvent, TreeModelSpliceEvent, TreeNodeApi};

/// `dfs`.
fn dfs<M: ITreeModel>(node: &Rc<M::Node>, f: &mut impl FnMut(&Rc<M::Node>)) {
    f(node);

    for child in node.children() {
        dfs::<M>(&child, f);
    }
}

pub struct TreeTrait<M: ITreeModel> {
    nodes: RefCell<Vec<Rc<M::Node>>>,
    node_set: RefCell<Option<HashSet<ById<M::Node>>>>,
    on_did_change: Rc<Emitter<TreeEvent<M::Element>>>,
    get_first_view_element_with_trait: Box<dyn Fn() -> Option<Rc<M::Node>>>,
    identity_provider: Option<Rc<dyn IIdentityProvider<M::Element>>>,
}

impl<M: ITreeModel> TreeTrait<M> {
    pub fn new(get_first_view_element_with_trait: impl Fn() -> Option<Rc<M::Node>> + 'static, identity_provider: Option<Rc<dyn IIdentityProvider<M::Element>>>) -> Rc<Self> {
        Rc::new(Self {
            nodes: RefCell::new(Vec::new()),
            node_set: RefCell::new(None),
            on_did_change: Emitter::new(),
            get_first_view_element_with_trait: Box::new(get_first_view_element_with_trait),
            identity_provider,
        })
    }

    pub fn on_did_change(&self) -> BaseEvent<TreeEvent<M::Element>> {
        self.on_did_change.event()
    }

    /// `set`: não dispara quando os nós são os mesmos (a menos de `force_event`).
    pub fn set(&self, nodes: Vec<Rc<M::Node>>, browser_event: Option<BrowserEvent>) {
        let force = browser_event.is_some_and(|e| e.force_event);

        if !force && {
            let current = self.nodes.borrow();
            current.len() == nodes.len() && current.iter().zip(&nodes).all(|(a, b)| Rc::ptr_eq(a, b))
        } {
            return;
        }

        self.set_inner(nodes, false, browser_event);
    }

    fn set_inner(&self, nodes: Vec<Rc<M::Node>>, silent: bool, browser_event: Option<BrowserEvent>) {
        *self.nodes.borrow_mut() = nodes;
        *self.node_set.borrow_mut() = None;

        if !silent {
            self.on_did_change.fire(TreeEvent { elements: self.get(), browser_event });
        }
    }

    pub fn get(&self) -> Vec<M::Element> {
        self.nodes.borrow().iter().map(|n| n.element()).collect()
    }

    pub fn get_nodes(&self) -> Vec<Rc<M::Node>> {
        self.nodes.borrow().clone()
    }

    pub fn has(&self, node: &Rc<M::Node>) -> bool {
        self.node_set.borrow_mut().get_or_insert_with(|| self.nodes.borrow().iter().map(|n| ById(n.clone())).collect()).contains(&ById(node.clone()))
    }

    /// `onDidModelSplice`.
    pub fn on_did_model_splice(&self, event: &TreeModelSpliceEvent<M::Node>) {
        let Some(identity_provider) = &self.identity_provider else {
            let mut set: HashSet<ById<M::Node>> = self.nodes.borrow().iter().map(|n| ById(n.clone())).collect();
            let order = self.get_nodes();

            for node in &event.deleted_nodes {
                dfs::<M>(node, &mut |n| {
                    set.remove(&ById(n.clone()));
                });
            }

            // `[...set.values()]` mantém a ordem de inserção do conjunto original.
            self.set(order.into_iter().filter(|n| set.contains(&ById(n.clone()))).collect(), None);
            return;
        };

        let mut deleted_ids: HashSet<String> = HashSet::new();

        for node in &event.deleted_nodes {
            dfs::<M>(node, &mut |n| {
                deleted_ids.insert(identity_provider.get_id(&n.element()));
            });
        }

        let mut inserted: HashMap<String, Rc<M::Node>> = HashMap::new();

        for node in &event.inserted_nodes {
            dfs::<M>(node, &mut |n| {
                inserted.insert(identity_provider.get_id(&n.element()), n.clone());
            });
        }

        let mut nodes: Vec<Rc<M::Node>> = Vec::new();

        for node in self.nodes.borrow().iter() {
            let id = identity_provider.get_id(&node.element());

            if !deleted_ids.contains(&id) {
                nodes.push(node.clone());
            } else if let Some(inserted_node) = inserted.get(&id)
                && inserted_node.visible()
            {
                nodes.push(inserted_node.clone());
            }
        }

        if !self.nodes.borrow().is_empty()
            && nodes.is_empty()
            && let Some(node) = (self.get_first_view_element_with_trait)()
        {
            nodes.push(node);
        }

        self.set_inner(nodes, true, None);
    }
}

impl<M: ITreeModel> IDisposable for TreeTrait<M>
where
    M::Element: Eq + Hash,
{
    fn dispose(&self) {
        self.on_did_change.dispose();
    }
}
