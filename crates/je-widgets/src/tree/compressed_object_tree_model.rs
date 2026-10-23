//! Porte de `src/vs/base/browser/ui/tree/compressedObjectTreeModel.ts`: a árvore de objetos em que as
//! cadeias de nós com um único filho viram um só nó (`[1, 11, 111]`, como as pastas encadeadas do
//! explorador), a `CompressedObjectTreeModel` (que fala em nós comprimidos) e a
//! `CompressibleObjectTreeModel` (que fala nos elementos, escondendo a compressão).
//!
//! Do TypeScript para o Rust:
//! - O `Iterable` é `Vec` (avaliado de uma vez, não sob demanda); `compress`, `decompress` e o `splice`
//!   do original viram funções ansiosas sobre `Vec`.
//! - O `ICompressedTreeNode<T>` (`{ elements, incompressible }`) é o elemento interno da árvore e tem
//!   identidade por referência no original: aqui é um [`ByRef`] ([`CompressedRef`]).
//! - O `compress`/`noCompress` do original são a mesma função, com a compressão ligada ou desligada
//!   ([`compress_with`]); só o `compress` é público. Os nós da árvore (`ITreeNode`), que no original são
//!   passados direto ao `decompress` por terem a mesma forma de um elemento, são convertidos por
//!   `node_to_element`.
//! - Os callbacks `onDidCreateNode` e `onDidDeleteNode` das opções de `setChildren` são ignorados pelo
//!   modelo comprimido (o original também só repassa o `diffIdentityProvider`).
//! - A `CompressibleObjectTreeModel` expõe `setChildren`, `resort`, `isCompressionEnabled` e
//!   `setCompressionEnabled` por [`Deref`] para a `CompressedObjectTreeModel` que ela envolve (no original
//!   são métodos repassadores, que o DRY do projeto proíbe); o `getCompressedTreeNode` é
//!   `compressed_model().get_node(..)`.
//! - O `WeakMapper` do original (um `WeakMap` do nó comprimido para o envoltório) não cabe em Rust:
//!   o envoltório guarda o nó por baixo (precisa lê-lo ao vivo, inclusive depois de removido da árvore),
//!   então uma entrada com `Weak` nunca morreria. O cache é por endereço do nó e descarta as entradas cujo
//!   nó só o próprio envoltório ainda referencia (ninguém mais consegue pedir aquele nó).
//! - O `children` do envoltório devolve envoltórios novos a cada leitura, como o original (só o
//!   `getNode` e os eventos passam pelo cache).
//! - A mensagem de `Tree element not found` não leva o elemento (`T` não é `Debug`).

use std::cell::{Cell, RefCell};
use std::collections::{HashMap, HashSet};
use std::hash::Hash;
use std::ops::Deref;
use std::rc::Rc;

use je_base::event::Event as BaseEvent;

use crate::common::throw;
use crate::list::list_types::{GroupId, IIdentityProvider};
use crate::tree::index_tree_model::{NodeCallback, TreeNode, get_visible_state};
use crate::tree::object_tree_model::{ObjectTreeModel, ObjectTreeModelOptions, ObjectTreeModelSetChildrenOptions};
use crate::tree::tree_types::{
    ByRef, CollapseStateChangeEvent, CollapsedValue, IObjectTreeElement, ITreeFilter, ITreeModel, ITreeSorter, TreeFilterResult, TreeListSpliceData, TreeModelSpliceEvent, TreeNodeApi, TreeVisibility,
    VisibilityValue, tree_error,
};

/// `ICompressedTreeElement<T>`: um `IObjectTreeElement` que pode ser marcado como incompressível.
#[derive(Clone, Debug)]
pub struct ICompressedTreeElement<T> {
    pub element: T,
    pub children: Option<Vec<ICompressedTreeElement<T>>>,
    pub collapsible: Option<bool>,
    pub collapsed: Option<CollapsedValue>,
    pub incompressible: bool,
}

impl<T: Clone> ICompressedTreeElement<T> {
    /// `{ element }`.
    pub fn new(element: T) -> Self {
        Self { element, children: None, collapsible: None, collapsed: None, incompressible: false }
    }

    /// `{ element, children }`.
    pub fn with_children(element: T, children: Vec<ICompressedTreeElement<T>>) -> Self {
        Self { children: Some(children), ..Self::new(element) }
    }

    /// `{ ..., incompressible: true }`.
    pub fn incompressible(mut self) -> Self {
        self.incompressible = true;
        self
    }

    /// `{ ..., collapsed }` com um booleano.
    pub fn collapsed(mut self, collapsed: bool) -> Self {
        self.collapsed = Some(CollapsedValue::Bool(collapsed));
        self
    }

    /// Uma cópia com outros filhos (o `{ ...treeElement, children }` do original).
    fn replace_children(&self, children: Vec<ICompressedTreeElement<T>>) -> Self {
        Self { element: self.element.clone(), children: Some(children), collapsible: self.collapsible, collapsed: self.collapsed, incompressible: self.incompressible }
    }
}

/// `ICompressedTreeNode<T>`.
#[derive(Debug, PartialEq, Eq)]
pub struct ICompressedTreeNode<T> {
    pub elements: Vec<T>,
    pub incompressible: bool,
}

/// O nó comprimido, com identidade por referência.
pub type CompressedRef<T> = ByRef<ICompressedTreeNode<T>>;

type CompressedNodeOf<T, F> = TreeNode<Option<CompressedRef<T>>, F>;

/// `compress` e `noCompress`: com `enabled`, funde as cadeias de filho único (que não sejam
/// incompressíveis); sem, cada elemento vira um nó.
fn compress_with<T: Clone>(element: &ICompressedTreeElement<T>, enabled: bool) -> ICompressedTreeElement<CompressedRef<T>> {
    let mut elements = vec![element.element.clone()];
    let incompressible = element.incompressible;
    let mut element = element;

    let children = loop {
        let children = element.children.as_deref().unwrap_or_default();

        if !enabled || children.len() != 1 || children[0].incompressible {
            break children;
        }

        element = &children[0];
        elements.push(element.element.clone());
    };

    ICompressedTreeElement {
        element: ByRef::new(ICompressedTreeNode { elements, incompressible }),
        children: Some(children.iter().map(|child| compress_with(child, enabled)).collect()),
        collapsible: element.collapsible,
        collapsed: element.collapsed,
        incompressible: false,
    }
}

/// `compress`: exportado só para os testes no original.
pub fn compress<T: Clone>(element: &ICompressedTreeElement<T>) -> ICompressedTreeElement<CompressedRef<T>> {
    compress_with(element, true)
}

fn decompress_at<T: Clone>(element: &ICompressedTreeElement<CompressedRef<T>>, index: usize) -> ICompressedTreeElement<T> {
    let node = &element.element;

    let children = if index + 1 < node.elements.len() {
        vec![decompress_at(element, index + 1)]
    } else {
        element.children.iter().flatten().map(|child| decompress_at(child, 0)).collect()
    };

    ICompressedTreeElement {
        element: node.elements[index].clone(),
        children: Some(children),
        collapsible: element.collapsible,
        collapsed: element.collapsed,
        incompressible: index == 0 && node.incompressible,
    }
}

/// `decompress`: exportado só para os testes no original.
pub fn decompress<T: Clone>(element: &ICompressedTreeElement<CompressedRef<T>>) -> ICompressedTreeElement<T> {
    decompress_at(element, 0)
}

/// `splice`: troca os filhos do elemento `element` dentro de `tree_element`.
fn splice_children<T: Clone + PartialEq>(tree_element: &ICompressedTreeElement<T>, element: &T, children: &[ICompressedTreeElement<T>]) -> ICompressedTreeElement<T> {
    if tree_element.element == *element {
        return tree_element.replace_children(children.to_vec());
    }

    tree_element.replace_children(tree_element.children.iter().flatten().map(|e| splice_children(e, element, children)).collect())
}

/// O nó da árvore lido como o elemento que o originou (o `ITreeNode` que o original passa ao
/// `decompress`).
fn node_to_element<T, F>(node: &Rc<CompressedNodeOf<T, F>>) -> ICompressedTreeElement<CompressedRef<T>> {
    ICompressedTreeElement {
        element: node.element.clone().expect("só a raiz tem elemento nulo"),
        children: Some(node.children.borrow().iter().map(node_to_element).collect()),
        collapsible: Some(node.collapsible.get()),
        collapsed: Some(CollapsedValue::Bool(node.collapsed.get())),
        incompressible: false,
    }
}

/// O elemento sem o `incompressible`, como o `ObjectTreeModel` o recebe.
fn to_object_element<E: Clone>(element: &ICompressedTreeElement<E>) -> IObjectTreeElement<E> {
    IObjectTreeElement {
        element: element.element.clone(),
        children: element.children.as_ref().map(|children| children.iter().map(to_object_element).collect()),
        collapsible: element.collapsible,
        collapsed: element.collapsed,
    }
}

/// `ICompressedObjectTreeModelOptions<T, TFilterData>`: as opções do modelo de objetos sobre os nós
/// comprimidos, mais a compressão.
pub struct CompressedObjectTreeModelOptions<T, F = ()> {
    pub base: ObjectTreeModelOptions<CompressedRef<T>, F>,
    pub compression_enabled: Option<bool>,
}

impl<T, F> Default for CompressedObjectTreeModelOptions<T, F> {
    fn default() -> Self {
        Self { base: Default::default(), compression_enabled: None }
    }
}

/// `wrapIdentityProvider`: o id do nó comprimido é o dos elementos, juntos por `\0`.
struct CompressedIdentityProvider<T>(Rc<dyn IIdentityProvider<T>>);

impl<T> IIdentityProvider<CompressedRef<T>> for CompressedIdentityProvider<T> {
    fn get_id(&self, node: &CompressedRef<T>) -> String {
        node.elements.iter().map(|e| self.0.get_id(e)).collect::<Vec<_>>().join("\0")
    }

    fn get_group_id(&self, node: &CompressedRef<T>) -> Option<GroupId> {
        node.elements.last().and_then(|e| self.0.get_group_id(e))
    }
}

/// `CompressedObjectTreeModel<T, TFilterData>`.
pub struct CompressedObjectTreeModel<T: Clone + Eq + Hash + 'static, F: Clone + 'static = ()> {
    user: String,
    model: ObjectTreeModel<CompressedRef<T>, F>,
    nodes: Rc<RefCell<HashMap<T, CompressedRef<T>>>>,
    enabled: Cell<bool>,
    identity_provider: Option<Rc<dyn IIdentityProvider<CompressedRef<T>>>>,
}

impl<T: Clone + Eq + Hash + 'static, F: Clone + 'static> CompressedObjectTreeModel<T, F> {
    pub fn new(user: &str, options: CompressedObjectTreeModelOptions<T, F>) -> Self {
        let identity_provider = options.base.identity_provider.clone();

        Self {
            user: user.to_owned(),
            model: ObjectTreeModel::new(user, options.base),
            nodes: Rc::default(),
            enabled: Cell::new(options.compression_enabled.unwrap_or(true)),
            identity_provider,
        }
    }

    /// `size`.
    pub fn size(&self) -> usize {
        self.nodes.borrow().len()
    }

    pub fn set_children(&self, element: &Option<T>, children: Vec<ICompressedTreeElement<T>>, options: ObjectTreeModelSetChildrenOptions<T, F>) {
        // Diffs must be deep, since the compression can affect nested elements.
        // @see https://github.com/microsoft/vscode/pull/114237#issuecomment-759425034
        let diff_identity_provider = options.diff_identity_provider.clone().map(|p| Rc::new(CompressedIdentityProvider(p)) as Rc<dyn IIdentityProvider<CompressedRef<T>>>);
        let enabled = self.enabled.get();

        let Some(element) = element else {
            let compressed_children = children.iter().map(|c| compress_with(c, enabled)).collect::<Vec<_>>();
            self.set_children_of(&None, &compressed_children, ObjectTreeModelSetChildrenOptions { diff_identity_provider, diff_depth: Some(usize::MAX), ..Default::default() });
            return;
        };

        let compressed_node = self.nodes.borrow().get(element).cloned().unwrap_or_else(|| throw(&tree_error(&self.user, "Unknown compressed tree node").to_string()));
        let compressed_node_ref = Some(compressed_node.clone());

        let node = self.model.get_node(&compressed_node_ref);
        let compressed_parent_node = self.model.get_parent_node_location(&compressed_node_ref).flatten();
        let parent = self.model.get_node(&compressed_parent_node);

        let decompressed_element = decompress(&node_to_element(&node));
        let spliced_element = splice_children(&decompressed_element, element, &children);
        let recompressed_element = compress_with(&spliced_element, enabled);

        // If the recompressed node is identical to the original, just set its children.
        // Saves work and churn diffing the parent element.
        let node_elements = &node.element.as_ref().expect("o nó não é a raiz").elements;
        let same_elements = {
            let recompressed_elements = &recompressed_element.element.elements;
            recompressed_elements.len() == node_elements.len()
                && recompressed_elements.iter().zip(node_elements).all(|(a, b)| match &options.diff_identity_provider {
                    Some(provider) => provider.get_id(a) == provider.get_id(b),
                    None => a == b,
                })
        };

        if same_elements {
            self.set_children_of(&compressed_node_ref, recompressed_element.children.as_deref().unwrap_or_default(), ObjectTreeModelSetChildrenOptions { diff_identity_provider, diff_depth: Some(1), ..Default::default() });
            return;
        }

        let parent_children = parent.children.borrow().iter().map(|child| if Rc::ptr_eq(child, &node) { recompressed_element.clone() } else { node_to_element(child) }).collect::<Vec<_>>();

        self.set_children_of(&parent.element, &parent_children, ObjectTreeModelSetChildrenOptions { diff_identity_provider, diff_depth: Some(node.depth - parent.depth), ..Default::default() });
    }

    pub fn is_compression_enabled(&self) -> bool {
        self.enabled.get()
    }

    pub fn set_compression_enabled(&self, enabled: bool) {
        if enabled == self.enabled.get() {
            return;
        }

        self.enabled.set(enabled);

        let root = self.model.get_node(&None);
        let recompressed_root_children =
            root.children.borrow().iter().map(|node| compress_with(&decompress(&node_to_element(node)), enabled)).collect::<Vec<_>>();

        // it should be safe to always use deep diff mode here if an identity
        // provider is available, since we know the raw nodes are unchanged.
        self.set_children_of(&None, &recompressed_root_children, ObjectTreeModelSetChildrenOptions { diff_identity_provider: self.identity_provider.clone(), diff_depth: Some(usize::MAX), ..Default::default() });
    }

    /// `_setChildren`: define os filhos de um nó comprimido e mantém o mapa elemento para nó.
    fn set_children_of(&self, node: &Option<CompressedRef<T>>, children: &[ICompressedTreeElement<CompressedRef<T>>], options: ObjectTreeModelSetChildrenOptions<CompressedRef<T>, F>) {
        let inserted_elements: Rc<RefCell<HashSet<T>>> = Rc::default();

        let on_did_create_node: NodeCallback<Option<CompressedRef<T>>, F> = {
            let (nodes, inserted_elements) = (self.nodes.clone(), inserted_elements.clone());
            Rc::new(move |node: &Rc<CompressedNodeOf<T, F>>| {
                let Some(compressed) = &node.element else { return };
                for element in &compressed.elements {
                    inserted_elements.borrow_mut().insert(element.clone());
                    nodes.borrow_mut().insert(element.clone(), compressed.clone());
                }
            })
        };

        let on_did_delete_node: NodeCallback<Option<CompressedRef<T>>, F> = {
            let nodes = self.nodes.clone();
            Rc::new(move |node: &Rc<CompressedNodeOf<T, F>>| {
                let Some(compressed) = &node.element else { return };
                for element in &compressed.elements {
                    if !inserted_elements.borrow().contains(element) {
                        nodes.borrow_mut().remove(element);
                    }
                }
            })
        };

        self.model.set_children(
            node,
            children.iter().map(to_object_element).collect(),
            ObjectTreeModelSetChildrenOptions { on_did_create_node: Some(on_did_create_node), on_did_delete_node: Some(on_did_delete_node), ..options },
        );
    }

    pub fn resort(&self, location: &Option<T>, recursive: bool) {
        self.model.resort(&self.get_compressed_node(location), recursive);
    }

    /// `getCompressedNode`.
    pub fn get_compressed_node(&self, element: &Option<T>) -> Option<CompressedRef<T>> {
        let element = element.as_ref()?;
        let node = self.nodes.borrow().get(element).cloned();
        Some(node.unwrap_or_else(|| throw(&tree_error(&self.user, "Tree element not found").to_string())))
    }
}

/// O elemento mais profundo do nó comprimido (a localização de um nó comprimido).
fn last_element<T: Clone>(node: &CompressedRef<T>) -> T {
    node.elements.last().expect("um nó comprimido tem ao menos um elemento").clone()
}

impl<T: Clone + Eq + Hash + 'static, F: Clone + 'static> ITreeModel for CompressedObjectTreeModel<T, F> {
    type Element = Option<CompressedRef<T>>;
    type FilterData = F;
    type Ref = Option<T>;
    type Node = TreeNode<Option<CompressedRef<T>>, F>;

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

    fn get_list_index(&self, location: &Option<T>) -> i64 {
        self.model.get_list_index(&self.get_compressed_node(location))
    }

    fn get_list_render_count(&self, location: &Option<T>) -> usize {
        self.model.get_list_render_count(&self.get_compressed_node(location))
    }

    fn get_node(&self, location: &Option<T>) -> Rc<Self::Node> {
        self.model.get_node(&self.get_compressed_node(location))
    }

    // TODO: review this
    fn get_node_location(&self, node: &Rc<Self::Node>) -> Option<T> {
        self.model.get_node_location(node).as_ref().map(last_element)
    }

    // TODO: review this
    fn get_parent_node_location(&self, location: &Option<T>) -> Option<Option<T>> {
        let compressed_node = self.get_compressed_node(location);
        let parent_node = self.model.get_parent_node_location(&compressed_node)?;
        Some(parent_node.as_ref().map(last_element))
    }

    fn get_first_element_child(&self, location: &Option<T>) -> Option<Option<CompressedRef<T>>> {
        self.model.get_first_element_child(&self.get_compressed_node(location))
    }

    fn get_last_element_ancestor(&self, location: &Option<T>) -> Option<Option<CompressedRef<T>>> {
        self.model.get_last_element_ancestor(&self.get_compressed_node(location))
    }

    fn is_collapsible(&self, location: &Option<T>) -> bool {
        self.model.is_collapsible(&self.get_compressed_node(location))
    }

    fn set_collapsible(&self, location: &Option<T>, collapsible: Option<bool>) -> bool {
        self.model.set_collapsible(&self.get_compressed_node(location), collapsible)
    }

    fn is_collapsed(&self, location: &Option<T>) -> bool {
        self.model.is_collapsed(&self.get_compressed_node(location))
    }

    fn set_collapsed(&self, location: &Option<T>, collapsed: Option<bool>, recursive: Option<bool>) -> bool {
        self.model.set_collapsed(&self.get_compressed_node(location), collapsed, recursive)
    }

    fn expand_to(&self, location: &Option<T>) {
        self.model.expand_to(&self.get_compressed_node(location));
    }

    fn rerender(&self, location: &Option<T>) {
        self.model.rerender(&self.get_compressed_node(location));
    }

    fn refilter(&self) {
        self.model.refilter();
    }
}

// Compressible Object Tree

/// `ElementMapper<T>`: o elemento que representa um nó comprimido, dado os elementos dele.
pub type ElementMapper<T> = Rc<dyn Fn(&[T]) -> T>;

/// `CompressedNodeUnwrapper<T>`.
pub type CompressedNodeUnwrapper<T> = Rc<dyn Fn(&ICompressedTreeNode<T>) -> T>;

/// `DefaultElementMapper`: o último elemento.
pub fn default_element_mapper<T: Clone + 'static>() -> ElementMapper<T> {
    Rc::new(|elements: &[T]| elements.last().expect("um nó comprimido tem ao menos um elemento").clone())
}

/// `CompressedTreeNodeWrapper<T, TFilterData>`: o nó comprimido visto como o nó do elemento (o
/// `unwrapper` escolhe o elemento), lendo o nó por baixo ao vivo.
pub struct CompressedTreeNodeWrapper<T: Clone + 'static, F: Clone + 'static> {
    unwrapper: CompressedNodeUnwrapper<T>,
    node: Rc<CompressedNodeOf<T, F>>,
}

impl<T: Clone + 'static, F: Clone + 'static> TreeNodeApi<Option<T>, F> for CompressedTreeNodeWrapper<T, F> {
    fn element(&self) -> Option<T> {
        self.node.element.as_ref().map(|node| (self.unwrapper)(node))
    }

    fn children(&self) -> Vec<Rc<Self>> {
        self.node.children.borrow().iter().map(|node| Rc::new(Self { unwrapper: self.unwrapper.clone(), node: node.clone() })).collect()
    }

    fn depth(&self) -> usize {
        self.node.depth
    }

    fn visible_children_count(&self) -> usize {
        self.node.visible_children_count.get()
    }

    fn visible_child_index(&self) -> i64 {
        self.node.visible_child_index.get()
    }

    fn collapsible(&self) -> bool {
        self.node.collapsible.get()
    }

    fn collapsed(&self) -> bool {
        self.node.collapsed.get()
    }

    fn visible(&self) -> bool {
        self.node.visible.get()
    }

    fn filter_data(&self) -> Option<F> {
        self.node.filter_data.borrow().clone()
    }
}

/// `CompressedNodeWeakMapper`: a mesma instância de envoltório para o mesmo nó comprimido (ver o `//!`).
struct CompressedNodeWeakMapper<T: Clone + 'static, F: Clone + 'static> {
    unwrapper: CompressedNodeUnwrapper<T>,
    /// Do endereço do nó comprimido para o envoltório (que guarda o nó vivo).
    entries: RefCell<HashMap<usize, Rc<CompressedTreeNodeWrapper<T, F>>>>,
    /// O tamanho do mapa a partir do qual a próxima inserção varre as entradas órfãs.
    prune_at: Cell<usize>,
}

impl<T: Clone + 'static, F: Clone + 'static> CompressedNodeWeakMapper<T, F> {
    const MIN_PRUNE_SIZE: usize = 64;

    fn new(unwrapper: CompressedNodeUnwrapper<T>) -> Self {
        Self { unwrapper, entries: RefCell::default(), prune_at: Cell::new(Self::MIN_PRUNE_SIZE) }
    }

    fn map(&self, node: &Rc<CompressedNodeOf<T, F>>) -> Rc<CompressedTreeNodeWrapper<T, F>> {
        let key = Rc::as_ptr(node) as *const () as usize;
        let mut entries = self.entries.borrow_mut();

        if let Some(wrapper) = entries.get(&key) {
            return wrapper.clone();
        }

        if entries.len() >= self.prune_at.get() {
            entries.retain(|_, wrapper| Rc::strong_count(&wrapper.node) > 1);
            self.prune_at.set((entries.len() * 2).max(Self::MIN_PRUNE_SIZE));
        }

        let wrapper = Rc::new(CompressedTreeNodeWrapper { unwrapper: self.unwrapper.clone(), node: node.clone() });
        entries.insert(key, wrapper.clone());
        wrapper
    }
}

/// O `ITreeSorter<T>` visto como o sorter dos nós comprimidos (compara os primeiros elementos).
struct CompressedSorter<T>(Rc<dyn ITreeSorter<T>>);

impl<T> ITreeSorter<CompressedRef<T>> for CompressedSorter<T> {
    fn compare(&self, node: &CompressedRef<T>, other_node: &CompressedRef<T>) -> i32 {
        self.0.compare(&node.elements[0], &other_node.elements[0])
    }
}

/// O `IIdentityProvider<T>` visto como o dos nós comprimidos (o id do elemento que o `unwrapper` escolhe).
struct UnwrappedIdentityProvider<T> {
    base: Rc<dyn IIdentityProvider<T>>,
    unwrapper: CompressedNodeUnwrapper<T>,
}

impl<T> IIdentityProvider<CompressedRef<T>> for UnwrappedIdentityProvider<T> {
    fn get_id(&self, node: &CompressedRef<T>) -> String {
        self.base.get_id(&(self.unwrapper)(node))
    }

    fn get_group_id(&self, node: &CompressedRef<T>) -> Option<GroupId> {
        self.base.get_group_id(&(self.unwrapper)(node))
    }
}

/// O filtro dos elementos aplicado em cadeia aos elementos do nó comprimido: a visibilidade de cada um é
/// a visibilidade do pai do seguinte.
struct CompressedFilter<T, F>(Rc<dyn ITreeFilter<T, F>>);

impl<T, F> ITreeFilter<CompressedRef<T>, F> for CompressedFilter<T, F> {
    fn filter(&self, node: &CompressedRef<T>, mut parent_visibility: TreeVisibility) -> TreeFilterResult<F> {
        let (last, rest) = node.elements.split_last().expect("um nó comprimido tem ao menos um elemento");

        for element in rest {
            parent_visibility = get_visible_state(match self.0.filter(element, parent_visibility) {
                TreeFilterResult::Bool(b) => VisibilityValue::Bool(b),
                TreeFilterResult::Visibility(v) => VisibilityValue::Visibility(v),
                TreeFilterResult::Data { visibility, .. } => visibility,
            });
        }

        self.0.filter(last, parent_visibility)
    }
}

/// `ICompressibleObjectTreeModelOptions<T, TFilterData>`.
pub struct CompressibleObjectTreeModelOptions<T, F = ()> {
    pub base: ObjectTreeModelOptions<T, F>,
    pub compression_enabled: Option<bool>,
    pub element_mapper: Option<ElementMapper<T>>,
}

impl<T, F> Default for CompressibleObjectTreeModelOptions<T, F> {
    fn default() -> Self {
        Self { base: Default::default(), compression_enabled: None, element_mapper: None }
    }
}

/// `mapOptions`.
fn map_options<T: 'static, F: 'static>(unwrapper: &CompressedNodeUnwrapper<T>, options: CompressibleObjectTreeModelOptions<T, F>) -> CompressedObjectTreeModelOptions<T, F> {
    let base = options.base;

    CompressedObjectTreeModelOptions {
        base: ObjectTreeModelOptions {
            sorter: base.sorter.map(|s| Rc::new(CompressedSorter(s)) as Rc<dyn ITreeSorter<CompressedRef<T>>>),
            identity_provider: base.identity_provider.map(|base| Rc::new(UnwrappedIdentityProvider { base, unwrapper: unwrapper.clone() }) as Rc<dyn IIdentityProvider<CompressedRef<T>>>),
            collapse_by_default: base.collapse_by_default,
            allow_non_collapsible_parents: base.allow_non_collapsible_parents,
            filter: base.filter.map(|f| Rc::new(CompressedFilter(f)) as Rc<dyn ITreeFilter<CompressedRef<T>, F>>),
            auto_expand_single_children: base.auto_expand_single_children,
        },
        compression_enabled: options.compression_enabled,
    }
}

/// `CompressibleObjectTreeModel<T, TFilterData>`.
pub struct CompressibleObjectTreeModel<T: Clone + Eq + Hash + 'static, F: Clone + 'static = ()> {
    element_mapper: ElementMapper<T>,
    node_mapper: Rc<CompressedNodeWeakMapper<T, F>>,
    model: CompressedObjectTreeModel<T, F>,
}

impl<T: Clone + Eq + Hash + 'static, F: Clone + 'static> CompressibleObjectTreeModel<T, F> {
    pub fn new(user: &str, mut options: CompressibleObjectTreeModelOptions<T, F>) -> Self {
        let element_mapper = options.element_mapper.take().unwrap_or_else(default_element_mapper);
        let compressed_node_unwrapper: CompressedNodeUnwrapper<T> = {
            let element_mapper = element_mapper.clone();
            Rc::new(move |node: &ICompressedTreeNode<T>| element_mapper(&node.elements))
        };

        Self {
            element_mapper,
            node_mapper: Rc::new(CompressedNodeWeakMapper::new(compressed_node_unwrapper.clone())),
            model: CompressedObjectTreeModel::new(user, map_options(&compressed_node_unwrapper, options)),
        }
    }

    /// `getCompressedTreeNode` é `compressed_model().get_node(location)`.
    pub fn compressed_model(&self) -> &CompressedObjectTreeModel<T, F> {
        &self.model
    }

    /// O elemento que o `elementMapper` escolhe para o resultado de `getFirstElementChild` e
    /// `getLastElementAncestor` (que no modelo comprimido é o nó comprimido).
    fn map_result(&self, result: Option<Option<CompressedRef<T>>>) -> Option<Option<T>> {
        result.map(|node| node.map(|node| (self.element_mapper)(&node.elements)))
    }
}

/// Os métodos que o `CompressibleObjectTreeModel` repassa sem mudança (ver o `//!`).
impl<T: Clone + Eq + Hash + 'static, F: Clone + 'static> Deref for CompressibleObjectTreeModel<T, F> {
    type Target = CompressedObjectTreeModel<T, F>;

    fn deref(&self) -> &Self::Target {
        &self.model
    }
}

impl<T: Clone + Eq + Hash + 'static, F: Clone + 'static> ITreeModel for CompressibleObjectTreeModel<T, F> {
    type Element = Option<T>;
    type FilterData = F;
    type Ref = Option<T>;
    type Node = CompressedTreeNodeWrapper<T, F>;

    fn root_ref(&self) -> Option<T> {
        None
    }

    fn ref_is_falsy(&self, location: &Option<T>) -> bool {
        location.is_none()
    }

    fn on_did_splice_model(&self) -> BaseEvent<TreeModelSpliceEvent<Self::Node>> {
        let mapper = self.node_mapper.clone();
        self.model.on_did_splice_model().map(
            move |e| TreeModelSpliceEvent {
                inserted_nodes: e.inserted_nodes.iter().map(|node| mapper.map(node)).collect(),
                deleted_nodes: e.deleted_nodes.iter().map(|node| mapper.map(node)).collect(),
            },
            None,
        )
    }

    fn on_did_splice_rendered_nodes(&self) -> BaseEvent<TreeListSpliceData<Self::Node>> {
        let mapper = self.node_mapper.clone();
        self.model.on_did_splice_rendered_nodes().map(
            move |e| TreeListSpliceData { start: e.start, delete_count: e.delete_count, elements: e.elements.iter().map(|node| mapper.map(node)).collect() },
            None,
        )
    }

    fn on_did_change_collapse_state(&self) -> BaseEvent<CollapseStateChangeEvent<Self::Node>> {
        let mapper = self.node_mapper.clone();
        self.model.on_did_change_collapse_state().map(move |e| CollapseStateChangeEvent { node: mapper.map(&e.node), deep: e.deep }, None)
    }

    fn on_did_change_render_node_count(&self) -> BaseEvent<Rc<Self::Node>> {
        let mapper = self.node_mapper.clone();
        self.model.on_did_change_render_node_count().map(move |node| mapper.map(node), None)
    }

    fn has(&self, location: &Option<T>) -> bool {
        self.model.has(location)
    }

    fn get_list_index(&self, location: &Option<T>) -> i64 {
        self.model.get_list_index(location)
    }

    fn get_list_render_count(&self, location: &Option<T>) -> usize {
        self.model.get_list_render_count(location)
    }

    fn get_node(&self, location: &Option<T>) -> Rc<Self::Node> {
        self.node_mapper.map(&self.model.get_node(location))
    }

    fn get_node_location(&self, node: &Rc<Self::Node>) -> Option<T> {
        node.element()
    }

    fn get_parent_node_location(&self, location: &Option<T>) -> Option<Option<T>> {
        self.model.get_parent_node_location(location)
    }

    fn get_first_element_child(&self, location: &Option<T>) -> Option<Option<T>> {
        self.map_result(self.model.get_first_element_child(location))
    }

    fn get_last_element_ancestor(&self, location: &Option<T>) -> Option<Option<T>> {
        self.map_result(self.model.get_last_element_ancestor(location))
    }

    fn is_collapsible(&self, location: &Option<T>) -> bool {
        self.model.is_collapsible(location)
    }

    fn set_collapsible(&self, location: &Option<T>, collapsible: Option<bool>) -> bool {
        self.model.set_collapsible(location, collapsible)
    }

    fn is_collapsed(&self, location: &Option<T>) -> bool {
        self.model.is_collapsed(location)
    }

    fn set_collapsed(&self, location: &Option<T>, collapsed: Option<bool>, recursive: Option<bool>) -> bool {
        self.model.set_collapsed(location, collapsed, recursive)
    }

    fn expand_to(&self, location: &Option<T>) {
        self.model.expand_to(location);
    }

    fn rerender(&self, location: &Option<T>) {
        self.model.rerender(location);
    }

    fn refilter(&self) {
        self.model.refilter();
    }
}
