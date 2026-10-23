//! Porte de `src/vs/base/browser/ui/tree/objectTree.ts`: a [`ObjectTree`], a árvore endereçada pelo
//! próprio elemento. A `CompressibleObjectTree` fica em [`crate::tree::compressible_object_tree`].
//!
//! Do TypeScript para o Rust:
//! - `ObjectTree<T, TFilterData> extends AbstractTree<T | null, TFilterData, T | null>` é uma struct que faz
//!   `Deref` para a [`AbstractTree`] do [`ObjectTreeModel`]: o elemento e a referência são `Option<T>` (o
//!   `null` é a raiz), como no original. Quem implementa o renderer vê `node.element: Option<T>`.
//! - O delegate de alturas e o filtro e o provedor de identidade do usuário são sobre `T`, como no
//!   original; os adaptadores para `Option<T>` ficam aqui.
//! - O `Iterable` de filhos é um `Vec`.

use std::hash::Hash;
use std::ops::Deref;
use std::rc::Rc;

use je_base::lifecycle::IDisposable;
use je_ui::{NodeId, Ui};

use crate::list::list_types::{GroupId, IIdentityProvider, IListVirtualDelegate};
use crate::tree::abstract_tree::{AbstractTree, AbstractTreeOptions};
use crate::tree::index_tree_model::{IndexTreeModel, IndexTreeModelOptions, IndexTreeModelSpliceOptions};
use crate::tree::object_tree_model::{ObjectTreeModel, ObjectTreeModelOptions, ObjectTreeModelSetChildrenOptions};
use crate::tree::tree_types::{IObjectTreeElement, ITreeElement, ITreeFilter, ITreeModel, ITreeRenderer, ITreeSorter, TreeFilterResult, TreeVisibility, tree_error};

/// O `IListVirtualDelegate<T>` visto como `IListVirtualDelegate<Option<T>>` (a raiz não tem altura).
pub struct OptionDelegate<T>(pub Rc<dyn IListVirtualDelegate<T>>);

impl<T> IListVirtualDelegate<Option<T>> for OptionDelegate<T> {
    fn get_height(&self, element: &Option<T>) -> f64 {
        element.as_ref().map_or(0.0, |e| self.0.get_height(e))
    }

    fn get_template_id(&self, element: &Option<T>) -> String {
        element.as_ref().map_or_else(String::new, |e| self.0.get_template_id(e))
    }

    fn has_dynamic_height(&self, element: &Option<T>) -> Option<bool> {
        element.as_ref().and_then(|e| self.0.has_dynamic_height(e))
    }

    fn get_dynamic_height(&self, element: &Option<T>) -> Option<f64> {
        element.as_ref().and_then(|e| self.0.get_dynamic_height(e))
    }

    fn set_dynamic_height(&self, element: &Option<T>, height: f64) {
        if let Some(e) = element {
            self.0.set_dynamic_height(e, height);
        }
    }
}

/// O `ITreeFilter<Option<T>, F>` das opções da árvore visto como `ITreeFilter<T, F>` do modelo.
struct UnwrapFilter<T, F>(Rc<dyn ITreeFilter<Option<T>, F>>);

impl<T: Clone, F> ITreeFilter<T, F> for UnwrapFilter<T, F> {
    fn filter(&self, element: &T, parent_visibility: TreeVisibility) -> TreeFilterResult<F> {
        self.0.filter(&Some(element.clone()), parent_visibility)
    }
}

/// O `IIdentityProvider<Option<T>>` das opções da árvore visto como `IIdentityProvider<T>` do modelo.
struct UnwrapIdentityProvider<T>(Rc<dyn IIdentityProvider<Option<T>>>);

impl<T: Clone> IIdentityProvider<T> for UnwrapIdentityProvider<T> {
    fn get_id(&self, element: &T) -> String {
        self.0.get_id(&Some(element.clone()))
    }

    fn get_group_id(&self, element: &T) -> Option<GroupId> {
        self.0.get_group_id(&Some(element.clone()))
    }
}

/// Os métodos do `ObjectTree` que valem para qualquer árvore de elementos `T | null`: o `ObjectTree` e a
/// `CompressibleObjectTree` (que herda dele no original).
pub trait ObjectTreeMethods<T> {
    /// `rerender(element?)`: sem elemento, re-renderiza a lista inteira.
    fn rerender_element(&self, element: Option<&T>);
    /// `updateElementHeight`.
    fn update_element_height(&self, element: &T, height: Option<f64>);
    /// `getListRenderCount`.
    fn get_list_render_count(&self, element: &Option<T>) -> usize;
    /// `hasElement`.
    fn has_element(&self, element: &T) -> bool;
}

impl<T: Clone + 'static, M: ITreeModel<Element = Option<T>, Ref = Option<T>>> ObjectTreeMethods<T> for AbstractTree<M>
where
    M::Element: Eq + Hash,
{
    fn rerender_element(&self, element: Option<&T>) {
        match element {
            None => self.view.rerender(),
            Some(element) => self.model.rerender(&Some(element.clone())),
        }
    }

    fn update_element_height(&self, element: &T, height: Option<f64>) {
        let index = self.model.get_list_index(&Some(element.clone()));

        if index == -1 {
            return;
        }

        self.view.update_element_height(index as usize, height);
    }

    fn get_list_render_count(&self, element: &Option<T>) -> usize {
        self.model.get_list_render_count(element)
    }

    fn has_element(&self, element: &T) -> bool {
        self.model.has(&Some(element.clone()))
    }
}

/// As opções do `ObjectTreeModel` a partir das da árvore (`options as IObjectTreeOptions<T | null, ...>`
/// do original): o filtro e o provedor de identidade da árvore são sobre `Option<T>`, os do modelo sobre `T`.
pub fn object_tree_model_options<T: Clone + 'static, F: 'static, M: ITreeModel<Element = Option<T>, FilterData = F>>(
    tree_options: &AbstractTreeOptions<M>,
    sorter: Option<Rc<dyn ITreeSorter<T>>>,
) -> ObjectTreeModelOptions<T, F> {
    ObjectTreeModelOptions {
        sorter,
        identity_provider: tree_options.identity_provider.clone().map(|p| Rc::new(UnwrapIdentityProvider(p)) as Rc<dyn IIdentityProvider<T>>),
        collapse_by_default: tree_options.collapse_by_default,
        allow_non_collapsible_parents: tree_options.allow_non_collapsible_parents,
        filter: tree_options.filter.clone().map(|f| Rc::new(UnwrapFilter(f)) as Rc<dyn ITreeFilter<T, F>>),
        auto_expand_single_children: false,
    }
}

/// `IObjectTreeOptions<T, TFilterData>`.
pub struct ObjectTreeOptions<T: Clone + Eq + Hash + 'static, F: Clone + 'static = ()> {
    pub tree: AbstractTreeOptions<ObjectTreeModel<T, F>>,
    pub sorter: Option<Rc<dyn ITreeSorter<T>>>,
}

impl<T: Clone + Eq + Hash + 'static, F: Clone + 'static> Default for ObjectTreeOptions<T, F> {
    fn default() -> Self {
        Self { tree: AbstractTreeOptions::default(), sorter: None }
    }
}

/// `ObjectTree<T, TFilterData>`.
pub struct ObjectTree<T: Clone + Eq + Hash + 'static, F: Clone + 'static = ()> {
    tree: Rc<AbstractTree<ObjectTreeModel<T, F>>>,
}

impl<T: Clone + Eq + Hash + 'static, F: Clone + 'static> Deref for ObjectTree<T, F> {
    type Target = AbstractTree<ObjectTreeModel<T, F>>;

    fn deref(&self) -> &Self::Target {
        &self.tree
    }
}

impl<T: Clone + Eq + Hash + 'static, F: Clone + 'static> ObjectTree<T, F> {
    pub fn new(
        user: &str,
        ui: &Ui,
        container: NodeId,
        delegate: Rc<dyn IListVirtualDelegate<T>>,
        renderers: Vec<Rc<dyn ITreeRenderer<ObjectTreeModel<T, F>>>>,
        options: ObjectTreeOptions<T, F>,
    ) -> Rc<Self> {
        let sorter = options.sorter;
        let create_model = |tree_options: &AbstractTreeOptions<ObjectTreeModel<T, F>>| Rc::new(ObjectTreeModel::new(user, object_tree_model_options(tree_options, sorter)));

        Rc::new(Self { tree: AbstractTree::new(user, ui, container, Rc::new(OptionDelegate(delegate)), renderers, options.tree, create_model) })
    }

    /// A árvore abstrata por baixo (para quem precisa do `Rc`).
    pub fn abstract_tree(&self) -> &Rc<AbstractTree<ObjectTreeModel<T, F>>> {
        &self.tree
    }

    /// `setChildren`.
    pub fn set_children(&self, element: &Option<T>, children: Vec<IObjectTreeElement<T>>, options: ObjectTreeModelSetChildrenOptions<T, F>) {
        self.tree.model.set_children(element, children, options);
    }

    /// `resort`.
    pub fn resort(&self, element: &Option<T>, recursive: bool) {
        self.tree.model.resort(element, recursive);
    }
}

impl<T: Clone + Eq + Hash + 'static, F: Clone + 'static> IDisposable for ObjectTree<T, F> {
    fn dispose(&self) {
        self.tree.dispose();
    }

    fn constructor_name(&self) -> &'static str {
        "ObjectTree"
    }
}

/// `IndexTree<T, TFilterData>`.
pub struct IndexTree<T: Clone + Eq + Hash + 'static, F: Clone + 'static = ()> {
    user: String,
    tree: Rc<AbstractTree<IndexTreeModel<T, F>>>,
}

impl<T: Clone + Eq + Hash + 'static, F: Clone + 'static> Deref for IndexTree<T, F> {
    type Target = AbstractTree<IndexTreeModel<T, F>>;

    fn deref(&self) -> &Self::Target {
        &self.tree
    }
}

impl<T: Clone + Eq + Hash + 'static, F: Clone + 'static> IndexTree<T, F> {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        user: &str,
        ui: &Ui,
        container: NodeId,
        delegate: Rc<dyn IListVirtualDelegate<T>>,
        renderers: Vec<Rc<dyn ITreeRenderer<IndexTreeModel<T, F>>>>,
        root_element: T,
        options: AbstractTreeOptions<IndexTreeModel<T, F>>,
    ) -> Rc<Self> {
        let create_model = |tree_options: &AbstractTreeOptions<IndexTreeModel<T, F>>| {
            IndexTreeModel::new(
                user,
                root_element,
                IndexTreeModelOptions {
                    collapse_by_default: tree_options.collapse_by_default,
                    allow_non_collapsible_parents: tree_options.allow_non_collapsible_parents,
                    filter: tree_options.filter.clone(),
                    auto_expand_single_children: false,
                },
            )
        };

        Rc::new(Self { user: user.to_owned(), tree: AbstractTree::new(user, ui, container, delegate, renderers, options, create_model) })
    }

    /// `splice`.
    pub fn splice(&self, location: &[usize], delete_count: usize, to_insert: Vec<ITreeElement<T>>) {
        self.tree.model.splice(location, delete_count, to_insert, &IndexTreeModelSpliceOptions::default());
    }

    /// `rerender(location?)`.
    pub fn rerender(&self, location: Option<&Vec<usize>>) {
        match location {
            None => self.tree.view.rerender(),
            Some(location) => self.tree.model.rerender(location),
        }
    }

    /// `updateElementHeight`.
    pub fn update_element_height(&self, location: &Vec<usize>, height: f64) {
        if location.is_empty() {
            crate::common::throw(&tree_error(&self.user, "Update element height failed: invalid location").to_string());
        }

        let index = self.tree.model.get_list_index(location);

        if index == -1 {
            return;
        }

        self.tree.view.update_element_height(index as usize, Some(height));
    }
}

impl<T: Clone + Eq + Hash + 'static, F: Clone + 'static> IDisposable for IndexTree<T, F> {
    fn dispose(&self) {
        self.tree.dispose();
    }

    fn constructor_name(&self) -> &'static str {
        "IndexTree"
    }
}
