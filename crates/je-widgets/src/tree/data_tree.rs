//! Porte de `src/vs/base/browser/ui/tree/dataTree.ts`: a [`DataTree`], a árvore que pergunta os filhos a
//! uma fonte de dados síncrona ([`IDataSource`]) e guarda uma entrada (`TInput`) como raiz.
//!
//! Do TypeScript para o Rust:
//! - `DataTree<TInput, T, TFilterData> extends AbstractTree<T | null, TFilterData, T | null>` é uma struct
//!   que faz `Deref` para a [`AbstractTree`] do [`ObjectTreeModel`], como a [`ObjectTree`]: o elemento e a
//!   referência são `Option<T>` (o `null` é a raiz). Os métodos herdados valem por esse `Deref`; o
//!   `refresh(element?)` do original é o `rerender_element` do [`ObjectTreeMethods`] (o corpo é o mesmo, e
//!   uma função de repasse não existe no projeto).
//! - O `TInput | T` é o [`InputOrElement`]: a variante `Input` faz o papel do `element === this.input`.
//! - As opções são as do `ObjectTree` ([`ObjectTreeOptions`]: as da árvore abstrata mais o `sorter`), como
//!   o `IDataTreeOptions` do original. Os filtros e o provedor de identidade da árvore são sobre
//!   `Option<T>`, e o `identityProvider` que a `DataTree` lê é o mesmo, chamado com `Some(elemento)`.
//! - O `Iterable` de filhos é um `Vec` (avaliado de uma vez).
//! - O `viewState.focus`/`selection` (`Set<string>` no original) são `Vec<String>` do
//!   [`AbstractTreeViewState`], e o `scrollTop` é sempre um número.

use std::cell::RefCell;
use std::collections::HashMap;
use std::hash::Hash;
use std::ops::Deref;
use std::rc::Rc;

use je_base::lifecycle::IDisposable;
use je_ui::{NodeId, Ui};

use crate::common::throw;
use crate::list::list_types::{IIdentityProvider, IListVirtualDelegate};
use crate::tree::abstract_tree::{AbstractTree, AbstractTreeViewState};
use crate::tree::index_tree_model::{NodeCallback, TreeNode};
use crate::tree::object_tree::{ObjectTree, ObjectTreeOptions};
use crate::tree::object_tree_model::{ObjectTreeModel, ObjectTreeModelSetChildrenOptions};
use crate::tree::tree_types::{IDataSource, IObjectTreeElement, ITreeRenderer, InputOrElement, tree_error};

/// `IDataTreeOptions<T, TFilterData>`: as opções da árvore de objetos (as da árvore abstrata e o `sorter`).
pub type DataTreeOptions<T, F = ()> = ObjectTreeOptions<T, F>;

type Node<T, F> = TreeNode<Option<T>, F>;

/// O `isCollapsed` do `_refresh`: `undefined` quando a árvore não sabe.
type IsCollapsed<'a, T> = Option<&'a dyn Fn(&T) -> Option<bool>>;

/// Os nós por identidade (o `nodesByIdentity` do original).
type NodesByIdentity<T, F> = Rc<RefCell<HashMap<String, Rc<Node<T, F>>>>>;

/// `DataTree<TInput, T, TFilterData>`.
pub struct DataTree<TInput: Clone + 'static, T: Clone + Eq + Hash + 'static, F: Clone + 'static = ()> {
    user: String,
    tree: Rc<AbstractTree<ObjectTreeModel<T, F>>>,
    data_source: Rc<dyn IDataSource<TInput, T>>,
    input: RefCell<Option<TInput>>,
    identity_provider: Option<Rc<dyn IIdentityProvider<Option<T>>>>,
    nodes_by_identity: NodesByIdentity<T, F>,
}

impl<TInput: Clone + 'static, T: Clone + Eq + Hash + 'static, F: Clone + 'static> Deref for DataTree<TInput, T, F> {
    type Target = AbstractTree<ObjectTreeModel<T, F>>;

    fn deref(&self) -> &Self::Target {
        &self.tree
    }
}

impl<TInput: Clone + 'static, T: Clone + Eq + Hash + 'static, F: Clone + 'static> DataTree<TInput, T, F> {
    pub fn new(
        user: &str,
        ui: &Ui,
        container: NodeId,
        delegate: Rc<dyn IListVirtualDelegate<T>>,
        renderers: Vec<Rc<dyn ITreeRenderer<ObjectTreeModel<T, F>>>>,
        data_source: Rc<dyn IDataSource<TInput, T>>,
        options: DataTreeOptions<T, F>,
    ) -> Rc<Self> {
        let identity_provider = options.tree.identity_provider.clone();
        let tree = ObjectTree::new(user, ui, container, delegate, renderers, options).abstract_tree().clone();

        Rc::new(Self { user: user.to_owned(), tree, data_source, input: RefCell::new(None), identity_provider, nodes_by_identity: Rc::default() })
    }

    /// O id do elemento (`this.identityProvider!.getId(element).toString()`).
    fn id_of(provider: &Rc<dyn IIdentityProvider<Option<T>>>, element: &T) -> String {
        provider.get_id(&Some(element.clone()))
    }

    // Model

    /// `getInput`.
    pub fn get_input(&self) -> Option<TInput> {
        self.input.borrow().clone()
    }

    /// `setInput`.
    pub fn set_input(&self, input: Option<TInput>, view_state: Option<&AbstractTreeViewState>) {
        if view_state.is_some() && self.identity_provider.is_none() {
            throw(&tree_error(&self.user, "Can't restore tree view state without an identity provider").to_string());
        }

        *self.input.borrow_mut() = input.clone();

        let Some(input) = input else {
            self.nodes_by_identity.borrow_mut().clear();
            self.tree.model.set_children(&None, Vec::new(), ObjectTreeModelSetChildrenOptions::default());
            return;
        };

        let input = InputOrElement::Input(input);

        let Some(view_state) = view_state else {
            self.refresh_element(&input, None, None);
            return;
        };

        let provider = self.identity_provider.clone().expect("conferido acima");
        let focus: Rc<RefCell<Vec<T>>> = Rc::default();
        let selection: Rc<RefCell<Vec<T>>> = Rc::default();

        let is_collapsed = {
            let provider = provider.clone();
            let expanded = view_state.expanded.clone();
            move |element: &T| Some(!expanded.get(&Self::id_of(&provider, element)).copied().unwrap_or(false))
        };

        let on_did_create_node: NodeCallback<Option<T>, F> = {
            let (focus, selection) = (focus.clone(), selection.clone());
            let (focus_ids, selection_ids) = (view_state.focus.clone(), view_state.selection.clone());
            Rc::new(move |node: &Rc<Node<T, F>>| {
                let Some(element) = &node.element else { return };
                let id = Self::id_of(&provider, element);

                if focus_ids.contains(&id) {
                    focus.borrow_mut().push(element.clone());
                }

                if selection_ids.contains(&id) {
                    selection.borrow_mut().push(element.clone());
                }
            })
        };

        self.refresh_element(&input, Some(&is_collapsed), Some(on_did_create_node));
        self.tree.set_focus(&focus.borrow().iter().cloned().map(Some).collect::<Vec<_>>(), None);
        self.tree.set_selection(&selection.borrow().iter().cloned().map(Some).collect::<Vec<_>>(), None);
        self.tree.set_scroll_top(view_state.scroll_top);
    }

    /// `updateChildren(element = this.input)`.
    pub fn update_children(&self, element: Option<&InputOrElement<TInput, T>>) {
        let Some(input) = self.get_input() else {
            throw(&tree_error(&self.user, "Tree input not set").to_string());
        };

        let element = element.cloned().unwrap_or(InputOrElement::Input(input));

        let nodes_by_identity = self.nodes_by_identity.clone();
        let is_collapsed = self.identity_provider.clone().map(|provider| {
            move |element: &T| {
                let id = Self::id_of(&provider, element);
                let node = nodes_by_identity.borrow().get(&id).cloned();
                node.map(|node| node.collapsed.get())
            }
        });

        self.refresh_element(&element, is_collapsed.as_ref().map(|f| f as &dyn Fn(&T) -> Option<bool>), None);
    }

    /// `resort(element = this.input, recursive = true)`.
    pub fn resort(&self, element: Option<&InputOrElement<TInput, T>>, recursive: bool) {
        let element = element.and_then(InputOrElement::as_element).cloned();
        self.tree.model.resort(&element, recursive);
    }

    // Implementation

    /// `_refresh`.
    fn refresh_element(&self, element: &InputOrElement<TInput, T>, is_collapsed: IsCollapsed<'_, T>, on_did_create_node: Option<NodeCallback<Option<T>, F>>) {
        let mut on_did_create_node = on_did_create_node;
        let mut on_did_delete_node: Option<NodeCallback<Option<T>, F>> = None;

        if let Some(provider) = self.identity_provider.clone() {
            let inserted_elements: Rc<RefCell<std::collections::HashSet<String>>> = Rc::default();

            let outer_on_did_create_node = on_did_create_node.take();
            on_did_create_node = Some({
                let (provider, inserted_elements, nodes_by_identity) = (provider.clone(), inserted_elements.clone(), self.nodes_by_identity.clone());
                Rc::new(move |node: &Rc<Node<T, F>>| {
                    if let Some(element) = &node.element {
                        let id = Self::id_of(&provider, element);

                        inserted_elements.borrow_mut().insert(id.clone());
                        nodes_by_identity.borrow_mut().insert(id, node.clone());
                    }

                    if let Some(outer) = &outer_on_did_create_node {
                        outer(node);
                    }
                })
            });

            on_did_delete_node = Some({
                let nodes_by_identity = self.nodes_by_identity.clone();
                Rc::new(move |node: &Rc<Node<T, F>>| {
                    let Some(element) = &node.element else { return };
                    let id = Self::id_of(&provider, element);

                    if !inserted_elements.borrow().contains(&id) {
                        nodes_by_identity.borrow_mut().remove(&id);
                    }
                })
            });
        }

        let children = self.iterate(element, is_collapsed);
        let location = element.as_element().cloned();
        self.tree.model.set_children(&location, children, ObjectTreeModelSetChildrenOptions { on_did_create_node, on_did_delete_node, ..Default::default() });
    }

    /// `iterate`: a árvore de elementos (o tamanho do original é o `len` dos filhos).
    fn iterate(&self, element: &InputOrElement<TInput, T>, is_collapsed: IsCollapsed<'_, T>) -> Vec<IObjectTreeElement<T>> {
        self.data_source
            .get_children(element)
            .into_iter()
            .map(|child| {
                let child_element = InputOrElement::Element(child.clone());
                let children = self.iterate(&child_element, is_collapsed);
                let collapsible = self.data_source.has_children(&child_element);
                let collapsed = if children.is_empty() { None } else { is_collapsed.and_then(|is_collapsed| is_collapsed(&child)) };

                IObjectTreeElement { element: child, children: Some(children), collapsible, collapsed: collapsed.map(crate::tree::tree_types::CollapsedValue::Bool) }
            })
            .collect()
    }
}

impl<TInput: Clone + 'static, T: Clone + Eq + Hash + 'static, F: Clone + 'static> IDisposable for DataTree<TInput, T, F> {
    fn dispose(&self) {
        self.tree.dispose();
    }

    fn constructor_name(&self) -> &'static str {
        "DataTree"
    }
}
