//! Porte de `src/vs/base/browser/ui/tree/objectTreeModel.ts`: o modelo de árvore endereçado pelo próprio
//! elemento (em vez de por índices), com ordenação, preservação do estado de colapso por identidade e o
//! mapa elemento para nó que o `IndexTreeModel` por baixo não tem.
//!
//! Do TypeScript para o Rust:
//! - O `null` (a raiz) é o `None`: o elemento e a referência do modelo são `Option<T>`, e o nó é um
//!   `TreeNode<Option<T>, F>` do [`IndexTreeModel`] (a raiz tem `element == None`).
//! - O `Iterable` de filhos é um `Vec` (avaliado de uma vez, não sob demanda). Com isso, o
//!   `preserveCollapseState` consulta o mapa de nós antes de qualquer nó novo do mesmo `setChildren`
//!   ser criado: o original, preguiçoso, só enxergaria os nós recém-criados se o mesmo elemento
//!   aparecesse duas vezes na mesma lista.
//! - As opções (`IObjectTreeModelOptions`, `IObjectTreeModelSetChildrenOptions`) são structs com `Default`,
//!   e o filtro e o provedor de identidade são sobre `T`; os adaptadores para `Option<T>` ficam aqui.
//! - A mensagem de `Tree element not found` não leva o elemento (`T` não é `Debug`).
//! - O original não tem `dispose`: o modelo não é `IDisposable`. As inscrições nos eventos do índice por
//!   baixo seguem o ciclo de vida de quem se inscreveu.

use std::cell::RefCell;
use std::cmp::Ordering;
use std::collections::{HashMap, HashSet};
use std::hash::Hash;
use std::rc::Rc;

use je_base::event::Event as BaseEvent;

use crate::common::throw;
use crate::list::list_types::{GroupId, IIdentityProvider};
use crate::tree::index_tree_model::{IndexTreeModel, IndexTreeModelOptions, IndexTreeModelSpliceOptions, NodeCallback, TreeNode};
use crate::tree::tree_types::{
    CollapseStateChangeEvent, CollapsedValue, IObjectTreeElement, ITreeElement, ITreeFilter, ITreeModel, ITreeSorter, ObjectTreeElementCollapseState, TreeFilterResult, TreeListSpliceData,
    TreeModelSpliceEvent, TreeVisibility, tree_error,
};

/// `IObjectTreeModelOptions<T, TFilterData>` (que estende `IIndexTreeModelOptions`).
pub struct ObjectTreeModelOptions<T, F = ()> {
    pub sorter: Option<Rc<dyn ITreeSorter<T>>>,
    pub identity_provider: Option<Rc<dyn IIdentityProvider<T>>>,
    pub collapse_by_default: bool,
    pub allow_non_collapsible_parents: bool,
    pub filter: Option<Rc<dyn ITreeFilter<T, F>>>,
    pub auto_expand_single_children: bool,
}

impl<T, F> Default for ObjectTreeModelOptions<T, F> {
    fn default() -> Self {
        Self { sorter: None, identity_provider: None, collapse_by_default: false, allow_non_collapsible_parents: false, filter: None, auto_expand_single_children: false }
    }
}

/// `IObjectTreeModelSetChildrenOptions<T, TFilterData>` (que estende `IIndexTreeModelSpliceOptions`). Os
/// callbacks recebem o nó do índice por baixo (`TreeNode<Option<T>, F>`).
pub struct ObjectTreeModelSetChildrenOptions<T, F = ()> {
    pub diff_depth: Option<usize>,
    pub diff_identity_provider: Option<Rc<dyn IIdentityProvider<T>>>,
    pub on_did_create_node: Option<NodeCallback<Option<T>, F>>,
    pub on_did_delete_node: Option<NodeCallback<Option<T>, F>>,
}

impl<T, F> Default for ObjectTreeModelSetChildrenOptions<T, F> {
    fn default() -> Self {
        Self { diff_depth: None, diff_identity_provider: None, on_did_create_node: None, on_did_delete_node: None }
    }
}

impl<T, F> Clone for ObjectTreeModelSetChildrenOptions<T, F> {
    fn clone(&self) -> Self {
        Self {
            diff_depth: self.diff_depth,
            diff_identity_provider: self.diff_identity_provider.clone(),
            on_did_create_node: self.on_did_create_node.clone(),
            on_did_delete_node: self.on_did_delete_node.clone(),
        }
    }
}

/// O `IIdentityProvider<T>` visto como `IIdentityProvider<Option<T>>` (a raiz nunca entra no diff).
struct OptionIdentityProvider<T>(Rc<dyn IIdentityProvider<T>>);

impl<T> IIdentityProvider<Option<T>> for OptionIdentityProvider<T> {
    fn get_id(&self, element: &Option<T>) -> String {
        element.as_ref().map(|e| self.0.get_id(e)).unwrap_or_default()
    }

    fn get_group_id(&self, element: &Option<T>) -> Option<GroupId> {
        element.as_ref().and_then(|e| self.0.get_group_id(e))
    }
}

/// O `ITreeFilter<T, F>` visto como `ITreeFilter<Option<T>, F>` (a raiz nunca é filtrada).
struct OptionFilter<T, F>(Rc<dyn ITreeFilter<T, F>>);

impl<T, F> ITreeFilter<Option<T>, F> for OptionFilter<T, F> {
    fn filter(&self, element: &Option<T>, parent_visibility: TreeVisibility) -> TreeFilterResult<F> {
        match element {
            Some(e) => self.0.filter(e, parent_visibility),
            None => TreeFilterResult::Visibility(TreeVisibility::Visible),
        }
    }
}

type NodeOf<T, F> = TreeNode<Option<T>, F>;
type NodeMap<T, F> = Rc<RefCell<HashMap<T, Rc<NodeOf<T, F>>>>>;
type NodeIdentityMap<T, F> = Rc<RefCell<HashMap<String, Rc<NodeOf<T, F>>>>>;

/// O `number` que `compare` devolve como [`Ordering`] (o `sort` do JavaScript só olha o sinal).
fn sign_to_ordering(n: i32) -> Ordering {
    n.cmp(&0)
}

pub struct ObjectTreeModel<T: Clone + Eq + Hash + 'static, F: Clone + 'static = ()> {
    user: String,
    model: Rc<IndexTreeModel<Option<T>, F>>,
    nodes: NodeMap<T, F>,
    nodes_by_identity: NodeIdentityMap<T, F>,
    identity_provider: Option<Rc<dyn IIdentityProvider<T>>>,
    sorter: Option<Rc<dyn ITreeSorter<T>>>,
}

impl<T: Clone + Eq + Hash + 'static, F: Clone + 'static> ObjectTreeModel<T, F> {
    pub fn new(user: &str, options: ObjectTreeModelOptions<T, F>) -> Self {
        let model = IndexTreeModel::new(
            user,
            None,
            IndexTreeModelOptions {
                collapse_by_default: options.collapse_by_default,
                allow_non_collapsible_parents: options.allow_non_collapsible_parents,
                filter: options.filter.map(|f| Rc::new(OptionFilter(f)) as Rc<dyn ITreeFilter<Option<T>, F>>),
                auto_expand_single_children: options.auto_expand_single_children,
            },
        );

        Self {
            user: user.to_owned(),
            model,
            nodes: Rc::default(),
            nodes_by_identity: Rc::default(),
            identity_provider: options.identity_provider,
            sorter: options.sorter,
        }
    }

    /// `size`: o número de elementos (sem a raiz).
    pub fn size(&self) -> usize {
        self.nodes.borrow().len()
    }

    pub fn set_children(&self, element: &Option<T>, children: Vec<IObjectTreeElement<T>>, options: ObjectTreeModelSetChildrenOptions<T, F>) {
        let location = self.get_element_location(element);
        self.set_children_at(&location, self.preserve_collapse_state(children), options);
    }

    /// `_setChildren`.
    fn set_children_at(&self, location: &[usize], children: Vec<ITreeElement<Option<T>>>, options: ObjectTreeModelSetChildrenOptions<T, F>) {
        let inserted_elements: Rc<RefCell<HashSet<T>>> = Rc::default();
        let inserted_element_ids: Rc<RefCell<HashSet<String>>> = Rc::default();

        let on_did_create_node: NodeCallback<Option<T>, F> = {
            let (nodes, nodes_by_identity, identity_provider) = (self.nodes.clone(), self.nodes_by_identity.clone(), self.identity_provider.clone());
            let (inserted_elements, inserted_element_ids) = (inserted_elements.clone(), inserted_element_ids.clone());
            let user_callback = options.on_did_create_node;
            Rc::new(move |node: &Rc<NodeOf<T, F>>| {
                let Some(element) = &node.element else { return };

                inserted_elements.borrow_mut().insert(element.clone());
                nodes.borrow_mut().insert(element.clone(), node.clone());

                if let Some(identity_provider) = &identity_provider {
                    let id = identity_provider.get_id(element);
                    inserted_element_ids.borrow_mut().insert(id.clone());
                    nodes_by_identity.borrow_mut().insert(id, node.clone());
                }

                if let Some(callback) = &user_callback {
                    callback(node);
                }
            })
        };

        let on_did_delete_node: NodeCallback<Option<T>, F> = {
            let (nodes, nodes_by_identity, identity_provider) = (self.nodes.clone(), self.nodes_by_identity.clone(), self.identity_provider.clone());
            let user_callback = options.on_did_delete_node;
            Rc::new(move |node: &Rc<NodeOf<T, F>>| {
                let Some(element) = &node.element else { return };

                if !inserted_elements.borrow().contains(element) {
                    nodes.borrow_mut().remove(element);
                }

                if let Some(identity_provider) = &identity_provider {
                    let id = identity_provider.get_id(element);
                    if !inserted_element_ids.borrow().contains(&id) {
                        nodes_by_identity.borrow_mut().remove(&id);
                    }
                }

                if let Some(callback) = &user_callback {
                    callback(node);
                }
            })
        };

        let mut index_location = location.to_vec();
        index_location.push(0);

        // `Number.MAX_VALUE` do original: "todos os filhos".
        self.model.splice(
            &index_location,
            usize::MAX >> 11,
            children,
            &IndexTreeModelSpliceOptions {
                diff_depth: options.diff_depth,
                diff_identity_provider: options.diff_identity_provider.map(|p| Rc::new(OptionIdentityProvider(p)) as Rc<dyn IIdentityProvider<Option<T>>>),
                on_did_create_node: Some(on_did_create_node),
                on_did_delete_node: Some(on_did_delete_node),
            },
        );
    }

    fn preserve_collapse_state(&self, mut elements: Vec<IObjectTreeElement<T>>) -> Vec<ITreeElement<Option<T>>> {
        if let Some(sorter) = &self.sorter {
            elements.sort_by(|a, b| sign_to_ordering(sorter.compare(&a.element, &b.element)));
        }

        elements
            .into_iter()
            .map(|tree_element| {
                let mut node = self.nodes.borrow().get(&tree_element.element).cloned();

                if node.is_none()
                    && let Some(identity_provider) = &self.identity_provider
                {
                    let id = identity_provider.get_id(&tree_element.element);
                    node = self.nodes_by_identity.borrow().get(&id).cloned();
                }

                let children = Some(self.preserve_collapse_state(tree_element.children.unwrap_or_default()));

                let Some(node) = node else {
                    let collapsed = tree_element.collapsed.map(|collapsed| match collapsed {
                        CollapsedValue::State(ObjectTreeElementCollapseState::Collapsed | ObjectTreeElementCollapseState::PreserveOrCollapsed) => true,
                        CollapsedValue::State(ObjectTreeElementCollapseState::Expanded | ObjectTreeElementCollapseState::PreserveOrExpanded) => false,
                        CollapsedValue::Bool(b) => b,
                    });

                    return ITreeElement { element: Some(tree_element.element), children, collapsible: tree_element.collapsible, collapsed };
                };

                let collapsible = tree_element.collapsible.unwrap_or_else(|| node.collapsible.get());
                let collapsed = match tree_element.collapsed {
                    None | Some(CollapsedValue::State(ObjectTreeElementCollapseState::PreserveOrCollapsed | ObjectTreeElementCollapseState::PreserveOrExpanded)) => node.collapsed.get(),
                    Some(CollapsedValue::State(ObjectTreeElementCollapseState::Collapsed)) => true,
                    Some(CollapsedValue::State(ObjectTreeElementCollapseState::Expanded)) => false,
                    Some(CollapsedValue::Bool(b)) => b,
                };

                ITreeElement { element: Some(tree_element.element), children, collapsible: Some(collapsible), collapsed: Some(collapsed) }
            })
            .collect()
    }

    pub fn resort(&self, element: &Option<T>, recursive: bool) {
        let Some(sorter) = &self.sorter else { return };

        let location = self.get_element_location(element);
        let node = self.model.get_node(&location);

        self.set_children_at(&location, Self::resort_children(sorter, &node, recursive, true), Default::default());
    }

    fn resort_children(sorter: &Rc<dyn ITreeSorter<T>>, node: &Rc<NodeOf<T, F>>, recursive: bool, first: bool) -> Vec<ITreeElement<Option<T>>> {
        let mut children_nodes = node.children.borrow().clone();

        if recursive || first {
            children_nodes.sort_by(|a, b| sign_to_ordering(sorter.compare(Self::child_element(a), Self::child_element(b))));
        }

        children_nodes
            .iter()
            .map(|node| ITreeElement {
                element: node.element.clone(),
                collapsible: Some(node.collapsible.get()),
                collapsed: Some(node.collapsed.get()),
                children: Some(Self::resort_children(sorter, node, recursive, false)),
            })
            .collect()
    }

    /// O elemento de um nó que não é a raiz (o `node.element as T` do original).
    fn child_element(node: &Rc<NodeOf<T, F>>) -> &T {
        node.element.as_ref().expect("só a raiz tem elemento nulo")
    }

    /// `getElementLocation`.
    fn get_element_location(&self, element: &Option<T>) -> Vec<usize> {
        match element {
            None => Vec::new(),
            Some(_) => self.model.get_node_location(&self.get_node(element)),
        }
    }

    fn element_not_found(&self) -> ! {
        throw(&tree_error(&self.user, "Tree element not found").to_string())
    }
}

impl<T: Clone + Eq + Hash + 'static, F: Clone + 'static> ITreeModel for ObjectTreeModel<T, F> {
    type Element = Option<T>;
    type FilterData = F;
    type Ref = Option<T>;
    type Node = TreeNode<Option<T>, F>;

    fn root_ref(&self) -> Option<T> {
        None
    }

    fn ref_is_falsy(&self, location: &Option<T>) -> bool {
        location.is_none()
    }

    fn on_did_splice_model(&self) -> BaseEvent<TreeModelSpliceEvent<Self::Node>> {
        self.model.on_did_splice_model()
    }

    fn on_did_splice_rendered_nodes(&self) -> BaseEvent<TreeListSpliceData<Self::Node>> {
        self.model.on_did_splice_rendered_nodes()
    }

    fn on_did_change_collapse_state(&self) -> BaseEvent<CollapseStateChangeEvent<Self::Node>> {
        self.model.on_did_change_collapse_state()
    }

    fn on_did_change_render_node_count(&self) -> BaseEvent<Rc<Self::Node>> {
        self.model.on_did_change_render_node_count()
    }

    fn has(&self, element: &Option<T>) -> bool {
        element.as_ref().is_some_and(|e| self.nodes.borrow().contains_key(e))
    }

    fn get_list_index(&self, element: &Option<T>) -> i64 {
        self.model.get_list_index(&self.get_element_location(element))
    }

    fn get_list_render_count(&self, element: &Option<T>) -> usize {
        self.model.get_list_render_count(&self.get_element_location(element))
    }

    fn get_node(&self, element: &Option<T>) -> Rc<Self::Node> {
        match element {
            None => self.model.get_node(&self.model.root_ref()),
            Some(e) => {
                let node = self.nodes.borrow().get(e).cloned();
                node.unwrap_or_else(|| self.element_not_found())
            }
        }
    }

    fn get_node_location(&self, node: &Rc<Self::Node>) -> Option<T> {
        node.element.clone()
    }

    fn get_parent_node_location(&self, element: &Option<T>) -> Option<Option<T>> {
        let Some(element) = element else { throw(&tree_error(&self.user, "Invalid getParentNodeLocation call").to_string()) };

        let node = self.nodes.borrow().get(element).cloned().unwrap_or_else(|| self.element_not_found());
        let location = self.model.get_node_location(&node);
        let parent_location = self.model.get_parent_node_location(&location)?;

        Some(self.model.get_node(&parent_location).element.clone())
    }

    fn get_first_element_child(&self, element: &Option<T>) -> Option<Option<T>> {
        self.model.get_first_element_child(&self.get_element_location(element))
    }

    fn get_last_element_ancestor(&self, element: &Option<T>) -> Option<Option<T>> {
        self.model.get_last_element_ancestor(&self.get_element_location(element))
    }

    fn is_collapsible(&self, element: &Option<T>) -> bool {
        self.model.is_collapsible(&self.get_element_location(element))
    }

    fn set_collapsible(&self, element: &Option<T>, collapsible: Option<bool>) -> bool {
        self.model.set_collapsible(&self.get_element_location(element), collapsible)
    }

    fn is_collapsed(&self, element: &Option<T>) -> bool {
        self.model.is_collapsed(&self.get_element_location(element))
    }

    fn set_collapsed(&self, element: &Option<T>, collapsed: Option<bool>, recursive: Option<bool>) -> bool {
        self.model.set_collapsed(&self.get_element_location(element), collapsed, recursive)
    }

    fn expand_to(&self, element: &Option<T>) {
        self.model.expand_to(&self.get_element_location(element));
    }

    fn rerender(&self, element: &Option<T>) {
        self.model.rerender(&self.get_element_location(element));
    }

    fn refilter(&self) {
        self.model.refilter();
    }
}
