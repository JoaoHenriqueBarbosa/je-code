//! Porte de `src/vs/base/browser/ui/tree/tree.ts` (a parte de modelo): visibilidade, filtro, elemento,
//! eventos do modelo e o erro. Os tipos de renderer, mouse e arrastar ficam com os widgets.

use std::hash::{Hash, Hasher};
use std::rc::Rc;

use je_base::event::Event as BaseEvent;

/// `TreeVisibility`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TreeVisibility {
    /// O nó fica escondido.
    Hidden,
    /// O nó fica visível.
    Visible,
    /// O nó fica visível se algum descendente estiver.
    Recurse,
}

/// O `boolean | TreeVisibility` de `ITreeFilterDataResult.visibility`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum VisibilityValue {
    Bool(bool),
    Visibility(TreeVisibility),
}

/// `TreeFilterResult<TFilterData>`: `boolean | TreeVisibility | ITreeFilterDataResult<TFilterData>`.
#[derive(Clone, Debug)]
pub enum TreeFilterResult<F> {
    Bool(bool),
    Visibility(TreeVisibility),
    /// `ITreeFilterDataResult`.
    Data { visibility: VisibilityValue, data: F },
}

/// `ITreeFilter<T, TFilterData>`.
pub trait ITreeFilter<T, F = ()> {
    fn filter(&self, element: &T, parent_visibility: TreeVisibility) -> TreeFilterResult<F>;
}

/// `ITreeSorter<T>`.
pub trait ITreeSorter<T> {
    fn compare(&self, element: &T, other_element: &T) -> i32;
}

/// `ITreeElement<T>`.
#[derive(Clone, Debug)]
pub struct ITreeElement<T> {
    pub element: T,
    pub children: Option<Vec<ITreeElement<T>>>,
    pub collapsible: Option<bool>,
    pub collapsed: Option<bool>,
}

impl<T> ITreeElement<T> {
    /// `{ element }`.
    pub fn new(element: T) -> Self {
        Self { element, children: None, collapsible: None, collapsed: None }
    }

    /// `{ element, children }`.
    pub fn with_children(element: T, children: Vec<ITreeElement<T>>) -> Self {
        Self { element, children: Some(children), collapsible: None, collapsed: None }
    }

    /// `{ element, collapsed }`.
    pub fn collapsed(mut self, collapsed: bool) -> Self {
        self.collapsed = Some(collapsed);
        self
    }

    /// `{ element, collapsible }`.
    pub fn collapsible(mut self, collapsible: bool) -> Self {
        self.collapsible = Some(collapsible);
        self
    }
}

/// `ObjectTreeElementCollapseState`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ObjectTreeElementCollapseState {
    Expanded,
    Collapsed,
    PreserveOrExpanded,
    PreserveOrCollapsed,
}

/// `TreeError`.
pub fn tree_error(user: &str, message: &str) -> je_base::errors::Error {
    je_base::errors::Error::new(format!("TreeError [{user}] {message}"))
}

/// `ITreeModelSpliceEvent<T, TFilterData>`.
pub struct TreeModelSpliceEvent<N> {
    pub inserted_nodes: Vec<Rc<N>>,
    pub deleted_nodes: Vec<Rc<N>>,
}

impl<N> Clone for TreeModelSpliceEvent<N> {
    fn clone(&self) -> Self {
        Self { inserted_nodes: self.inserted_nodes.clone(), deleted_nodes: self.deleted_nodes.clone() }
    }
}

/// `ITreeListSpliceData<T, TFilterData>`.
pub struct TreeListSpliceData<N> {
    pub start: usize,
    pub delete_count: usize,
    pub elements: Vec<Rc<N>>,
}

impl<N> Clone for TreeListSpliceData<N> {
    fn clone(&self) -> Self {
        Self { start: self.start, delete_count: self.delete_count, elements: self.elements.clone() }
    }
}

/// `ICollapseStateChangeEvent<T, TFilterData>`.
pub struct CollapseStateChangeEvent<N> {
    pub node: Rc<N>,
    pub deep: bool,
}

impl<N> Clone for CollapseStateChangeEvent<N> {
    fn clone(&self) -> Self {
        Self { node: self.node.clone(), deep: self.deep }
    }
}

/// `IObjectTreeElement.collapsed`: `boolean | ObjectTreeElementCollapseState`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CollapsedValue {
    Bool(bool),
    State(ObjectTreeElementCollapseState),
}

/// `IObjectTreeElement<T>`. O `Iterable` de filhos do original é um `Vec` (avaliado de uma vez, não sob
/// demanda).
#[derive(Clone, Debug)]
pub struct IObjectTreeElement<T> {
    pub element: T,
    pub children: Option<Vec<IObjectTreeElement<T>>>,
    pub collapsible: Option<bool>,
    pub collapsed: Option<CollapsedValue>,
}

impl<T> IObjectTreeElement<T> {
    /// `{ element }`.
    pub fn new(element: T) -> Self {
        Self { element, children: None, collapsible: None, collapsed: None }
    }

    /// `{ element, children }`.
    pub fn with_children(element: T, children: Vec<IObjectTreeElement<T>>) -> Self {
        Self { element, children: Some(children), collapsible: None, collapsed: None }
    }

    /// `{ ..., collapsed }` com um booleano.
    pub fn collapsed(mut self, collapsed: bool) -> Self {
        self.collapsed = Some(CollapsedValue::Bool(collapsed));
        self
    }

    /// `{ ..., collapsed }` com um `ObjectTreeElementCollapseState`.
    pub fn collapse_state(mut self, state: ObjectTreeElementCollapseState) -> Self {
        self.collapsed = Some(CollapsedValue::State(state));
        self
    }

    /// `{ ..., collapsible }`.
    pub fn collapsible(mut self, collapsible: bool) -> Self {
        self.collapsible = Some(collapsible);
        self
    }
}

/// Um `Rc<T>` com identidade por referência (o `===` e o `Map` por objeto do JavaScript): igual e hash
/// pelo endereço. É o tipo de elemento das árvores cujos elementos são objetos.
pub struct ByRef<T>(pub Rc<T>);

impl<T> ByRef<T> {
    pub fn new(value: T) -> Self {
        Self(Rc::new(value))
    }
}

impl<T> Clone for ByRef<T> {
    fn clone(&self) -> Self {
        Self(self.0.clone())
    }
}

impl<T> PartialEq for ByRef<T> {
    fn eq(&self, other: &Self) -> bool {
        Rc::ptr_eq(&self.0, &other.0)
    }
}

impl<T> Eq for ByRef<T> {}

impl<T> Hash for ByRef<T> {
    fn hash<H: Hasher>(&self, state: &mut H) {
        Rc::as_ptr(&self.0).hash(state);
    }
}

impl<T> std::ops::Deref for ByRef<T> {
    type Target = T;

    fn deref(&self) -> &T {
        &self.0
    }
}

impl<T: std::fmt::Debug> std::fmt::Debug for ByRef<T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.0.fmt(f)
    }
}

/// `ITreeNode<T, TFilterData>`: o que a árvore lê de um nó. O `TreeNode` do `IndexTreeModel` implementa
/// direto; o nó da `CompressibleObjectTreeModel` é um envoltório que lê o nó comprimido por baixo (o
/// `CompressedTreeNodeWrapper`). Dois nós são o mesmo nó quando `Rc::ptr_eq`.
pub trait TreeNodeApi<T, F>: Sized + 'static {
    fn element(&self) -> T;
    fn children(&self) -> Vec<Rc<Self>>;
    fn depth(&self) -> usize;
    fn visible_children_count(&self) -> usize;
    fn visible_child_index(&self) -> i64;
    fn collapsible(&self) -> bool;
    fn collapsed(&self) -> bool;
    fn visible(&self) -> bool;
    fn filter_data(&self) -> Option<F>;
}

/// `ITreeModel<T, TFilterData, TRef>`. Os três parâmetros e o tipo do nó são tipos associados.
/// `T` é o `Element` (para a `ObjectTreeModel`, `Option<X>`: o `null` é a raiz), `TRef` é o `Ref`.
///
/// Divergências do original: `getNode(location?)` recebe sempre a referência (`root_ref()` no lugar do
/// `undefined`); o `if (!parentRef)` do `abstractTree.ts` (referência nula) é [`ITreeModel::ref_is_falsy`],
/// já que `Option<X>` e `Vec` não têm "falsy" (as referências `0` e `''` do JavaScript, que o original
/// também trata como nulas, aqui são referências válidas).
pub trait ITreeModel: 'static {
    type Element: Clone + 'static;
    type FilterData: Clone + 'static;
    type Ref: Clone + 'static;
    type Node: TreeNodeApi<Self::Element, Self::FilterData>;

    fn root_ref(&self) -> Self::Ref;

    /// A referência é a raiz/nula (`!ref` no original)?
    fn ref_is_falsy(&self, _location: &Self::Ref) -> bool {
        false
    }

    fn on_did_splice_model(&self) -> BaseEvent<TreeModelSpliceEvent<Self::Node>>;
    fn on_did_splice_rendered_nodes(&self) -> BaseEvent<TreeListSpliceData<Self::Node>>;
    fn on_did_change_collapse_state(&self) -> BaseEvent<CollapseStateChangeEvent<Self::Node>>;
    fn on_did_change_render_node_count(&self) -> BaseEvent<Rc<Self::Node>>;

    fn has(&self, location: &Self::Ref) -> bool;
    fn get_list_index(&self, location: &Self::Ref) -> i64;
    fn get_list_render_count(&self, location: &Self::Ref) -> usize;
    fn get_node(&self, location: &Self::Ref) -> Rc<Self::Node>;
    fn get_node_location(&self, node: &Rc<Self::Node>) -> Self::Ref;
    fn get_parent_node_location(&self, location: &Self::Ref) -> Option<Self::Ref>;

    fn get_first_element_child(&self, location: &Self::Ref) -> Option<Self::Element>;
    fn get_last_element_ancestor(&self, location: &Self::Ref) -> Option<Self::Element>;

    fn is_collapsible(&self, location: &Self::Ref) -> bool;
    fn set_collapsible(&self, location: &Self::Ref, collapsible: Option<bool>) -> bool;
    fn is_collapsed(&self, location: &Self::Ref) -> bool;
    fn set_collapsed(&self, location: &Self::Ref, collapsed: Option<bool>, recursive: Option<bool>) -> bool;
    fn expand_to(&self, location: &Self::Ref);

    fn rerender(&self, location: &Self::Ref);
    fn refilter(&self);
}

/// `ITreeElementRenderDetails`: os detalhes de render da lista mais o recuo.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct TreeElementRenderDetails {
    pub height: Option<f64>,
    pub on_scroll: Option<bool>,
    pub indent: f64,
}

/// `ITreeRenderer<T, TFilterData, TTemplateData>`: o renderer de um nó da árvore. O `TTemplateData` é o
/// [`TemplateData`](crate::list::list_types::TemplateData) da lista (o renderer converte de volta com
/// `downcast_ref`).
pub trait ITreeRenderer<M: ITreeModel> {
    fn template_id(&self) -> String;
    /// `rowClassName`: a classe que a árvore põe nas linhas deste renderer.
    fn row_class_name(&self) -> Option<String> {
        None
    }
    fn render_template(&self, container: je_ui::NodeId) -> crate::list::list_types::TemplateData;
    fn render_element(&self, node: &Rc<M::Node>, index: usize, template_data: &crate::list::list_types::TemplateData, details: Option<&TreeElementRenderDetails>);
    fn dispose_element(&self, _node: &Rc<M::Node>, _index: i64, _template_data: &crate::list::list_types::TemplateData, _details: Option<&TreeElementRenderDetails>) {}
    /// `renderTwistie?`: devolve se o renderer desenhou o twistie.
    fn render_twistie(&self, _element: &M::Element, _twistie: je_ui::NodeId) -> bool {
        false
    }
    /// `onDidChangeTwistieState?`.
    fn on_did_change_twistie_state(&self) -> Option<BaseEvent<M::Element>> {
        None
    }
    fn dispose_template(&self, template_data: &crate::list::list_types::TemplateData);
}

/// `ITreeEvent<T>`: os elementos já materializados (o getter preguiçoso do original não tem ganho aqui).
#[derive(Clone, Debug)]
pub struct TreeEvent<E> {
    pub elements: Vec<E>,
    pub browser_event: Option<crate::list::list_types::BrowserEvent>,
}

/// `TreeMouseEventTarget`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TreeMouseEventTarget {
    Unknown,
    Twistie,
    Element,
    Filter,
}

/// `ITreeMouseEvent<T>`: `element` é o `T | null` do original. `is_handled_by_list` é o
/// `browserEvent.isHandledByList`.
#[derive(Clone, Debug)]
pub struct TreeMouseEvent<E> {
    pub browser_event: crate::list::list_types::BrowserEvent,
    pub is_handled_by_list: Rc<std::cell::Cell<bool>>,
    pub element: Option<E>,
    pub target: TreeMouseEventTarget,
}

/// `ITreeNavigator<T>`.
pub trait ITreeNavigator<E> {
    fn current(&mut self) -> Option<E>;
    fn previous(&mut self) -> Option<E>;
    fn first(&mut self) -> Option<E>;
    fn last(&mut self) -> Option<E>;
    fn next(&mut self) -> Option<E>;
}

/// O `TInput | T` do original: o elemento de uma árvore de dados é a entrada (a raiz) ou um elemento
/// comum. O `element === this.input` do TypeScript (identidade) vira a escolha da variante.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum InputOrElement<TInput, T> {
    /// A entrada (a raiz da árvore).
    Input(TInput),
    /// Um elemento comum.
    Element(T),
}

impl<TInput, T> InputOrElement<TInput, T> {
    /// O elemento, se não for a entrada.
    pub fn as_element(&self) -> Option<&T> {
        match self {
            Self::Element(element) => Some(element),
            Self::Input(_) => None,
        }
    }

    /// A entrada, se não for um elemento comum.
    pub fn as_input(&self) -> Option<&TInput> {
        match self {
            Self::Input(input) => Some(input),
            Self::Element(_) => None,
        }
    }
}

/// `IDataSource<TInput, T>`.
pub trait IDataSource<TInput, T> {
    /// `hasChildren?`: `None` quando a fonte não o define.
    fn has_children(&self, _element: &InputOrElement<TInput, T>) -> Option<bool> {
        None
    }

    /// `getChildren`. O `Iterable` do original é um `Vec`.
    fn get_children(&self, element: &InputOrElement<TInput, T>) -> Vec<T>;
}

/// `IAsyncDataSource<TInput, T>`.
pub trait IAsyncDataSource<TInput, T: Clone + 'static> {
    fn has_children(&self, element: &InputOrElement<TInput, T>) -> bool;

    /// `getChildren`: `Iterable<T> | Promise<Iterable<T>>`, com o `Iterable` como `Vec`
    /// (`MaybePromise::Value` é o `isIterable(children)` do original).
    fn get_children(&self, element: &InputOrElement<TInput, T>) -> je_base::promise::MaybePromise<Vec<T>>;

    /// `getParent?`: `None` quando a fonte não o define (o `expandTo` falha nesse caso).
    fn get_parent(&self, _element: &T) -> Option<InputOrElement<TInput, T>> {
        None
    }
}
