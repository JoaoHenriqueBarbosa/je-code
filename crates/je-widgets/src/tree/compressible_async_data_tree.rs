//! Porte da parte `CompressibleAsyncDataTree` de `src/vs/base/browser/ui/tree/asyncDataTree.ts`: a árvore de
//! dados assíncrona cujos nós de filho único se fundem num só (`a/b`), sobre a `CompressibleObjectTree`.
//!
//! Do TypeScript para o Rust:
//! - `CompressibleAsyncDataTree extends AsyncDataTree` é a [`AsyncDataTree`] com o backend
//!   [`CompressibleBackend`] (ver o `//!` de [`crate::tree::async_data_tree`]): o backend troca a árvore de
//!   baixo e sobrescreve `shouldAutoExpandSingleChild`, `asTreeElement`, `getViewState`, `render` e
//!   `processChildren`. O `asTreeElement` do original devolve o elemento com `incompressible`; aqui a
//!   `AsyncDataTree` monta o `IObjectTreeElement` e o backend o converte (recursivamente, com o
//!   `incompressible` de cada elemento) para o `ICompressedTreeElement` no `set_children`.
//! - O renderer do usuário é o [`ICompressibleAsyncDataTreeRenderer`] (sobre os envoltórios dos nós
//!   comuns e comprimidos). O [`AsyncDataTreeRenderer`] é o mesmo da árvore comum, com o mapa de
//!   envoltórios dos nós comprimidos no campo `extra`.
//! - O `navigate` do original só repete o da `AsyncDataTree` (com um comentário sobre as suposições da
//!   navegação em árvores comprimidas): não é sobrescrito aqui.
//! - O `updateOptions` com `compressionEnabled` do original (que a `CompressibleObjectTree` trata) não é
//!   exposto: `Deref` leva ao `update_options` da árvore abstrata, sem compressão. // TODO(integração): se
//!   um chamador precisar ligar e desligar a compressão depois do construtor.
//! - O `getCompressedTreeNode` devolve o nó comprimido de baixo (cujo `element` tem os nós internos, como no
//!   original, que tipa isso como `ITreeNode<ICompressedTreeNode<T>>`).

use std::collections::HashSet;
use std::hash::Hash;
use std::rc::Rc;

use je_ui::{NodeId, Ui};

use crate::common::throw;
use crate::list::list_types::{IKeyboardNavigationLabelProvider, IListVirtualDelegate, TemplateData};
use crate::tree::abstract_tree::AbstractTree;
use crate::tree::async_data_tree::{AsyncDataTree, AsyncDataTreeBackend, TreeParts};
use crate::tree::async_data_tree_node::{
    AsyncDataTreeOptions, AsyncDataTreeRenderer, AsyncDataTreeUpdateChildrenOptions, AsyncNode, IAsyncDataTreeRenderer, Orphanable, ViewStateContext, WeakMapper, as_object_tree_options, user_template_data,
};
use crate::tree::compressed_object_tree_model::{CompressedRef, CompressibleObjectTreeModel, ICompressedTreeElement, ICompressedTreeNode};
use crate::tree::compressible_object_tree::{CompressibleObjectTree, CompressibleObjectTreeOptions, ICompressibleKeyboardNavigationLabelProvider, ICompressibleTreeRenderer};
use crate::tree::index_tree_model::{TreeNode, get_visible_state};
use crate::tree::object_tree_model::ObjectTreeModelSetChildrenOptions;
use crate::tree::tree_types::{
    IAsyncDataSource, IObjectTreeElement, ITreeFilter, ITreeModel, InputOrElement, TreeElementRenderDetails, TreeFilterResult, TreeNodeApi, TreeVisibility, VisibilityValue,
};

/// O nó comprimido de baixo (o `ITreeNode<ICompressedTreeNode<IAsyncDataTreeNode>, TFilterData>`).
pub type CompressedAsyncNode<TInput, T, F> = TreeNode<Option<CompressedRef<AsyncNode<TInput, T>>>, F>;

/// `CompressibleAsyncDataTreeNodeWrapper<TInput, T, TFilterData>`: o nó comprimido visto como
/// `ITreeNode<ICompressedTreeNode<TInput | T>, TFilterData>`.
pub struct CompressibleAsyncDataTreeNodeWrapper<TInput, T, F> {
    node: Rc<CompressedAsyncNode<TInput, T, F>>,
}

impl<TInput, T, F> Orphanable for CompressibleAsyncDataTreeNodeWrapper<TInput, T, F> {
    fn is_orphan(&self) -> bool {
        Rc::strong_count(&self.node) <= 1
    }
}

impl<TInput: Clone + 'static, T: Clone + 'static, F: Clone + 'static> CompressibleAsyncDataTreeNodeWrapper<TInput, T, F> {
    /// O envoltório do nó comprimido, pelo cache.
    fn mapped(mapper: &CompressibleNodeMapper<TInput, T, F>, node: &Rc<CompressedAsyncNode<TInput, T, F>>) -> Rc<Self> {
        mapper.map(node, || Self { node: node.clone() })
    }
}

impl<TInput: Clone + 'static, T: Clone + 'static, F: Clone + 'static> TreeNodeApi<ICompressedTreeNode<InputOrElement<TInput, T>>, F> for CompressibleAsyncDataTreeNodeWrapper<TInput, T, F> {
    fn element(&self) -> ICompressedTreeNode<InputOrElement<TInput, T>> {
        let compressed = self.node.element.as_ref().expect("o nó raiz não tem elemento");

        ICompressedTreeNode { elements: compressed.elements.iter().map(|element| element.element_value()).collect(), incompressible: compressed.incompressible }
    }

    fn children(&self) -> Vec<Rc<Self>> {
        self.node.children.borrow().iter().map(|node| Rc::new(Self { node: node.clone() })).collect()
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

/// O cache de envoltórios dos nós comprimidos (`CompressibleAsyncDataTreeNodeMapper`).
pub type CompressibleNodeMapper<TInput, T, F> = WeakMapper<CompressibleAsyncDataTreeNodeWrapper<TInput, T, F>>;

/// O `ICompressibleTreeRenderer<T, TFilterData, TTemplateData>` que o usuário implementa para a árvore de
/// dados assíncrona comprimível.
pub trait ICompressibleAsyncDataTreeRenderer<TInput: Clone + 'static, T: Clone + 'static, F: Clone + 'static>: IAsyncDataTreeRenderer<TInput, T, F> {
    fn render_compressed_elements(
        &self,
        node: &Rc<CompressibleAsyncDataTreeNodeWrapper<TInput, T, F>>,
        index: usize,
        template_data: &TemplateData,
        details: Option<&TreeElementRenderDetails>,
    );

    fn dispose_compressed_elements(
        &self,
        _node: &Rc<CompressibleAsyncDataTreeNodeWrapper<TInput, T, F>>,
        _index: i64,
        _template_data: &TemplateData,
        _details: Option<&TreeElementRenderDetails>,
    ) {
    }
}

/// `CompressibleAsyncDataTreeRenderer`: o [`AsyncDataTreeRenderer`] com o cache dos nós comprimidos.
type CompressibleAsyncDataTreeRenderer<TInput, T, F> =
    AsyncDataTreeRenderer<TInput, T, F, dyn ICompressibleAsyncDataTreeRenderer<TInput, T, F>, Rc<CompressibleNodeMapper<TInput, T, F>>>;

impl<TInput: Clone + 'static, T: Clone + Eq + Hash + 'static, F: Clone + 'static> ICompressibleTreeRenderer<AsyncNode<TInput, T>, F> for CompressibleAsyncDataTreeRenderer<TInput, T, F> {
    fn render_compressed_elements(&self, node: &Rc<CompressedAsyncNode<TInput, T, F>>, index: usize, template_data: &TemplateData, details: Option<&TreeElementRenderDetails>) {
        self.renderer.render_compressed_elements(&CompressibleAsyncDataTreeNodeWrapper::mapped(&self.extra, node), index, &user_template_data(template_data), details);
    }

    fn dispose_compressed_elements(&self, node: &Rc<CompressedAsyncNode<TInput, T, F>>, index: i64, template_data: &TemplateData, details: Option<&TreeElementRenderDetails>) {
        self.renderer.dispose_compressed_elements(&CompressibleAsyncDataTreeNodeWrapper::mapped(&self.extra, node), index, &user_template_data(template_data), details);
    }
}

/// `ITreeCompressionDelegate<T>`.
pub trait ITreeCompressionDelegate<T> {
    fn is_incompressible(&self, element: &T) -> bool;
}

/// `ICompressibleKeyboardNavigationLabelProvider<T>` sobre `T` (o do `ObjectTree` é sobre o elemento nulo).
pub trait ICompressibleAsyncKeyboardNavigationLabelProvider<T>: IKeyboardNavigationLabelProvider<T> {
    /// `getCompressedNodeKeyboardNavigationLabel`.
    fn get_compressed_node_keyboard_navigation_label(&self, elements: &[T]) -> Vec<Option<String>>;
}

/// `ICompressibleAsyncDataTreeOptions<T, TFilterData>`.
pub struct CompressibleAsyncDataTreeOptions<T, F = ()> {
    pub base: AsyncDataTreeOptions<T, F>,
    pub compression_enabled: Option<bool>,
    /// O provedor que também rotula nós comprimidos (substitui o `keyboard_navigation_label_provider` de `base`).
    pub keyboard_navigation_label_provider: Option<Rc<dyn ICompressibleAsyncKeyboardNavigationLabelProvider<T>>>,
    /// Expande o filho único de um elemento que a compressão funde com ele, para que expandir o elemento
    /// fundido revele os filhos. Padrão: falso.
    pub auto_expand_compressed_children: bool,
}

impl<T, F> Default for CompressibleAsyncDataTreeOptions<T, F> {
    fn default() -> Self {
        Self { base: AsyncDataTreeOptions::default(), compression_enabled: None, keyboard_navigation_label_provider: None, auto_expand_compressed_children: false }
    }
}

/// O provedor do usuário (sobre `T`) visto como o dos nós internos (o `asCompressibleObjectTreeOptions`).
struct NodeCompressibleLabelProvider<T>(Rc<dyn ICompressibleAsyncKeyboardNavigationLabelProvider<T>>);

impl<TInput: Clone, T: Clone> IKeyboardNavigationLabelProvider<Option<AsyncNode<TInput, T>>> for NodeCompressibleLabelProvider<T> {
    fn get_keyboard_navigation_label(&self, element: &Option<AsyncNode<TInput, T>>) -> Vec<Option<String>> {
        self.0.get_keyboard_navigation_label(&element.as_ref().expect("a raiz não tem rótulo").element_t())
    }
}

impl<TInput: Clone, T: Clone> ICompressibleKeyboardNavigationLabelProvider<AsyncNode<TInput, T>> for NodeCompressibleLabelProvider<T> {
    fn get_compressed_node_keyboard_navigation_label(&self, elements: &[AsyncNode<TInput, T>]) -> Vec<Option<String>> {
        self.0.get_compressed_node_keyboard_navigation_label(&elements.iter().map(|element| element.element_t()).collect::<Vec<_>>())
    }
}

/// O backend da `CompressibleAsyncDataTree`: uma `CompressibleObjectTree` por baixo.
pub struct CompressibleBackend<T, F: Clone + 'static> {
    compression_delegate: Rc<dyn ITreeCompressionDelegate<T>>,
    filter: Option<Rc<dyn ITreeFilter<T, F>>>,
    auto_expand_compressed_children: bool,
    compression_enabled: Option<bool>,
    keyboard_navigation_label_provider: Option<Rc<dyn ICompressibleAsyncKeyboardNavigationLabelProvider<T>>>,
}

/// `getVisibility`.
fn get_visibility<F>(filter_result: &TreeFilterResult<F>) -> TreeVisibility {
    match filter_result {
        TreeFilterResult::Bool(visible) => get_visible_state(VisibilityValue::Bool(*visible)),
        TreeFilterResult::Visibility(visibility) => get_visible_state(VisibilityValue::Visibility(*visibility)),
        TreeFilterResult::Data { visibility, .. } => get_visible_state(*visibility),
    }
}

impl<T, F: Clone + 'static> CompressibleBackend<T, F> {
    /// O `asTreeElement` do original: o elemento com o `incompressible` do delegate, em todos os níveis.
    fn as_compressed_element<TInput: Clone + 'static>(&self, element: IObjectTreeElement<AsyncNode<TInput, T>>) -> ICompressedTreeElement<AsyncNode<TInput, T>>
    where
        T: Clone + 'static,
    {
        let incompressible = self.compression_delegate.is_incompressible(&element.element.element_t());

        ICompressedTreeElement {
            children: element.children.map(|children| children.into_iter().map(|child| self.as_compressed_element(child)).collect()),
            element: element.element,
            collapsible: element.collapsible,
            collapsed: element.collapsed,
            incompressible,
        }
    }
}

type Model<TInput, T, F> = CompressibleObjectTreeModel<AsyncNode<TInput, T>, F>;

impl<TInput, T, F> AsyncDataTreeBackend<TInput, T, F> for CompressibleBackend<T, F>
where
    TInput: Clone + 'static,
    T: Clone + Eq + Hash + 'static,
    F: Clone + 'static,
{
    type Model = Model<TInput, T, F>;
    type Renderer = dyn ICompressibleAsyncDataTreeRenderer<TInput, T, F>;

    fn create_tree(
        &self,
        parts: TreeParts<TInput, T, F>,
        user: &str,
        container: NodeId,
        delegate: Rc<dyn IListVirtualDelegate<T>>,
        renderers: Vec<Rc<Self::Renderer>>,
        options: &AsyncDataTreeOptions<T, F>,
    ) -> Rc<AbstractTree<Self::Model>> {
        let tree_renderers: Vec<Rc<dyn ICompressibleTreeRenderer<AsyncNode<TInput, T>, F>>> = renderers
            .into_iter()
            .map(|renderer| {
                Rc::new(AsyncDataTreeRenderer::new(&parts.ui, renderer, parts.node_mapper.clone(), parts.on_did_change_node_slow_state.clone(), Rc::new(CompressibleNodeMapper::<TInput, T, F>::default())))
                    as Rc<dyn ICompressibleTreeRenderer<AsyncNode<TInput, T>, F>>
            })
            .collect();

        let compressible_options = CompressibleObjectTreeOptions {
            tree: as_object_tree_options::<TInput, T, F, Self::Model>(options),
            sorter: None,
            compression_enabled: self.compression_enabled,
            element_mapper: None,
            keyboard_navigation_label_provider: self.keyboard_navigation_label_provider.clone().map(|provider| Rc::new(NodeCompressibleLabelProvider(provider)) as Rc<dyn ICompressibleKeyboardNavigationLabelProvider<AsyncNode<TInput, T>>>),
        };

        CompressibleObjectTree::new(user, &parts.ui, container, Rc::new(crate::tree::async_data_tree_node::AsyncNodeDelegate(delegate)), tree_renderers, compressible_options).abstract_tree().clone()
    }

    fn set_children(&self, tree: &AbstractTree<Self::Model>, element: &Option<AsyncNode<TInput, T>>, children: Vec<IObjectTreeElement<AsyncNode<TInput, T>>>, options: ObjectTreeModelSetChildrenOptions<AsyncNode<TInput, T>, F>) {
        let children = children.into_iter().map(|child| self.as_compressed_element(child)).collect();
        tree.model.set_children(element, children, options);
    }

    fn resort(&self, tree: &AbstractTree<Self::Model>, element: &Option<AsyncNode<TInput, T>>, recursive: bool) {
        tree.model.resort(element, recursive);
    }

    fn should_auto_expand_single_child(&self, tree: &AbstractTree<Self::Model>, base: bool, node: &AsyncNode<TInput, T>) -> bool {
        // A single compressible child is rendered combined with its parent, which only reveals
        // the child's children if the child is expanded along with the parent
        base || (self.auto_expand_compressed_children && tree.model.is_compression_enabled() && !self.compression_delegate.is_incompressible(&node.element_t()))
    }

    // For compressed async data trees, `TreeVisibility.Recurse` doesn't currently work
    // and we have to filter everything beforehand
    // Related to #85193 and #85835
    fn filter_children(&self, children: Vec<T>) -> Vec<T> {
        let Some(filter) = &self.filter else { return children };

        children
            .into_iter()
            .filter(|element| {
                let visibility = get_visibility(&filter.filter(element, TreeVisibility::Visible));

                if visibility == TreeVisibility::Recurse {
                    throw("Recursive tree visibility not supported in async data compressed trees");
                }

                visibility == TreeVisibility::Visible
            })
            .collect()
    }

    fn expanded_ids(&self, tree: &AbstractTree<Self::Model>, get_id: &dyn Fn(&T) -> String) -> Vec<String> {
        let mut expanded = Vec::new();
        let root = tree.model.compressed_model().get_node(&None);
        let mut stack = vec![root.clone()];

        while let Some(node) = stack.pop() {
            if !Rc::ptr_eq(&node, &root)
                && node.collapsible.get()
                && !node.collapsed.get()
                && let Some(compressed) = &node.element
            {
                expanded.extend(compressed.elements.iter().map(|async_node| get_id(&async_node.element_t())));
            }

            stack.extend(node.children.borrow().iter().cloned());
        }

        expanded
    }

    fn render(&self, tree: &AsyncDataTree<TInput, T, F, Self>, node: &AsyncNode<TInput, T>, view_state_context: Option<&Rc<ViewStateContext<TInput, T>>>, options: Option<AsyncDataTreeUpdateChildrenOptions<T>>) {
        let Some(identity_provider) = tree.identity_provider.clone() else {
            // `super.render(node, viewStateContext)`: o original não repassa as opções neste caminho.
            return tree.render_base(node, view_state_context, None);
        };

        // Preserve traits across compressions. Hacky but does the trick.
        // This is hard to fix properly since it requires rewriting the traits
        // across trees and lists. Let's just keep it this way for now.
        let get_id = |element: &T| identity_provider.get_id(element);
        let compressed_tree_node = |node: &Option<AsyncNode<TInput, T>>| tree.tree.model.compressed_model().get_node(node);
        let get_uncompressed_ids = |nodes: Vec<Option<AsyncNode<TInput, T>>>| -> HashSet<String> {
            let mut result = HashSet::new();

            for node in nodes {
                let compressed_node = compressed_tree_node(&node.filter(|node| node != &tree.root));

                let Some(compressed) = &compressed_node.element else { continue };

                result.extend(compressed.elements.iter().map(|node| get_id(&node.element_t())));
            }

            result
        };

        let old_selection = get_uncompressed_ids(tree.tree.get_selection());
        let old_focus = get_uncompressed_ids(tree.tree.get_focus());

        tree.render_base(node, view_state_context, options);

        let mut selection = tree.get_selection();
        let mut did_change_selection = false;

        let mut focus = tree.get_focus();
        let mut did_change_focus = false;

        let mut stack = vec![compressed_tree_node(&if node == &tree.root { None } else { Some(node.clone()) })];

        // `visit` em pré-ordem (o `forEach` do original), com a pilha invertida para manter a ordem.
        while let Some(visited) = stack.pop() {
            if let Some(compressed) = &visited.element {
                for async_node in &compressed.elements {
                    let id = get_id(&async_node.element_t());
                    let element = compressed.elements.last().expect("um nó comprimido tem ao menos um elemento").element_t();

                    // github.com/microsoft/vscode/issues/85938
                    if old_selection.contains(&id) && !selection.contains(&element) {
                        selection.push(element.clone());
                        did_change_selection = true;
                    }

                    if old_focus.contains(&id) && !focus.contains(&element) {
                        focus.push(element);
                        did_change_focus = true;
                    }
                }
            }

            stack.extend(visited.children.borrow().iter().rev().cloned());
        }

        if did_change_selection {
            tree.set_selection(&selection, None);
        }

        if did_change_focus {
            tree.set_focus(&focus, None);
        }
    }
}

/// `CompressibleAsyncDataTree<TInput, T, TFilterData>`.
pub type CompressibleAsyncDataTree<TInput, T, F = ()> = AsyncDataTree<TInput, T, F, CompressibleBackend<T, F>>;

impl<TInput: Clone + 'static, T: Clone + Eq + Hash + 'static, F: Clone + 'static> AsyncDataTree<TInput, T, F, CompressibleBackend<T, F>> {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        user: &str,
        ui: &Ui,
        container: NodeId,
        virtual_delegate: Rc<dyn IListVirtualDelegate<T>>,
        compression_delegate: Rc<dyn ITreeCompressionDelegate<T>>,
        renderers: Vec<Rc<dyn ICompressibleAsyncDataTreeRenderer<TInput, T, F>>>,
        data_source: Rc<dyn IAsyncDataSource<TInput, T>>,
        options: CompressibleAsyncDataTreeOptions<T, F>,
    ) -> Rc<Self> {
        let backend = CompressibleBackend {
            compression_delegate,
            filter: options.base.filter.clone(),
            auto_expand_compressed_children: options.auto_expand_compressed_children,
            compression_enabled: options.compression_enabled,
            keyboard_navigation_label_provider: options.keyboard_navigation_label_provider,
        };

        Self::construct(user, ui, container, virtual_delegate, renderers, data_source, options.base, backend)
    }

    /// `getCompressedTreeNode`.
    pub fn get_compressed_tree_node(&self, element: &InputOrElement<TInput, T>) -> Rc<CompressedAsyncNode<TInput, T, F>> {
        let node = self.get_data_node(element);
        self.tree.model.compressed_model().get_node(&Some(node))
    }

    /// `updateOptions({ compressionEnabled })`: liga ou desliga a compressão das pastas de filho único.
    pub fn set_compression_enabled(&self, enabled: bool) {
        self.tree.model.set_compression_enabled(enabled);
    }

    /// `isCompressionEnabled`.
    pub fn is_compression_enabled(&self) -> bool {
        self.tree.model.is_compression_enabled()
    }
}
