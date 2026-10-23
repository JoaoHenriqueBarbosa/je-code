//! Porte de `src/vs/base/browser/ui/tree/tree.ts` (a parte de modelo): visibilidade, filtro, elemento,
//! eventos do modelo e o erro. Os tipos de renderer, mouse e arrastar ficam com os widgets.

use std::rc::Rc;

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
