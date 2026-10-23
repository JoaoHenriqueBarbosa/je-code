//! Peças da [`AsyncDataTree`](crate::tree::async_data_tree::AsyncDataTree) de
//! `src/vs/base/browser/ui/tree/asyncDataTree.ts`: o nó interno (`IAsyncDataTreeNode`), o envoltório que o
//! renderer do usuário enxerga (`AsyncDataTreeNodeWrapper`), o renderer (`AsyncDataTreeRenderer`), as
//! opções (`IAsyncDataTreeOptions` e o `asObjectTreeOptions`) e o estado de visão.
//!
//! Do TypeScript para o Rust:
//! - `IAsyncDataTreeNode` é uma struct com `Cell`/`RefCell` nos campos que o original muta; o elemento
//!   interno do `ObjectTree` é o [`AsyncNode`] (`ByRef`: identidade por referência, o `===` do original). O
//!   `parent` é fraco (o pai já guarda o filho em `children`).
//! - O `element: TInput | T` é um [`InputOrElement`]; o `element` da raiz é `None` até o `setInput` (o
//!   `undefined` do original).
//! - O `id` da raiz é sempre `None` (o `null` do original só existe para marcar a raiz); o `node.id &&` do
//!   `asTreeElement` trata o id vazio como ausente, como o JavaScript.
//! - O `ITreeNode<TInput | T, TFilterData>` do renderer do usuário é o [`AsyncDataTreeNodeWrapper`]: lê o
//!   nó da árvore por baixo ao vivo, qualquer que seja o modelo (`ObjectTreeModel` ou o comprimível), por
//!   um objeto de leitura sem o tipo do nó. O `.element` da raiz lança, como o `node.element!.element` do
//!   original.
//! - O `WeakMapper` do original (um `WeakMap` do nó para o envoltório) é o [`WeakMapper`]: o envoltório
//!   guarda o nó por baixo, então uma entrada com `Weak` nunca morreria; o cache é por endereço do nó e
//!   descarta as entradas cujo nó só o próprio cache e o envoltório ainda referenciam.
//! - O `ITreeRenderer` que o usuário implementa é o [`IAsyncDataTreeRenderer`] (sobre o envoltório). O
//!   `renderTwistie` e o `rowClassName` do renderer do usuário não são repassados (o
//!   `AsyncDataTreeRenderer` do original também não os repassa).
//! - As opções são sobre `T` (o original as declara sobre `T` e as converte para `IAsyncDataTreeNode`). Ficam
//!   de fora: `findProvider` (e a busca assíncrona), `dnd`, `multipleSelectionController`,
//!   `accessibilityProvider` e `stickyScrollDelegate` (a árvore abstrata do `je-widgets` ainda não tem os
//!   três primeiros, e o último é sobre o modelo). // TODO(integração): depende do controlador de busca e
//!   da arrastar e soltar.
//! - No original o `collapseByDefault` é função (`(e: T) => boolean`); aqui é `Option<Rc<dyn Fn(&T) -> bool>>`.

use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::rc::{Rc, Weak};

use je_base::event::Event as BaseEvent;
use je_base::promise::CancelablePromise;
use je_base::themables::ThemeIcon;
use je_ui::{NodeId, Ui};

use crate::list::list_types::{GroupId, IIdentityProvider, IKeyboardNavigationLabelProvider, IListVirtualDelegate, TemplateData};
use crate::list::list_view::ListViewOptions;
use crate::tree::abstract_tree::{AbstractTreeOptions, AbstractTreeOptionsUpdate, ExpandOnlyOnTwistieClick};
use crate::tree::sticky_scroll::StickyScrollNodeSourceRange;
use crate::tree::tree_types::{ByRef, ITreeFilter, ITreeModel, ITreeRenderer, ITreeSorter, InputOrElement, ObjectTreeElementCollapseState, TreeElementRenderDetails, TreeFilterResult, TreeNodeApi, TreeVisibility};

/// `IAsyncDataTreeNode<TInput, T>`.
pub struct IAsyncDataTreeNode<TInput, T> {
    pub element: RefCell<Option<InputOrElement<TInput, T>>>,
    parent: Option<Weak<IAsyncDataTreeNode<TInput, T>>>,
    pub children: RefCell<Vec<AsyncNode<TInput, T>>>,
    pub id: Option<String>,
    pub refresh_promise: RefCell<Option<CancelablePromise<()>>>,
    pub has_children: Cell<bool>,
    pub stale: Cell<bool>,
    pub slow: Cell<bool>,
    /// `PreserveOrCollapsed`, `PreserveOrExpanded` ou nenhum.
    pub default_collapse_state: Option<ObjectTreeElementCollapseState>,
    pub force_expanded: Cell<bool>,
}

/// O nó interno com identidade por referência: o elemento do `ObjectTree` por baixo.
pub type AsyncNode<TInput, T> = ByRef<IAsyncDataTreeNode<TInput, T>>;

impl<TInput: Clone, T: Clone> IAsyncDataTreeNode<TInput, T> {
    /// `parent`.
    pub fn parent(&self) -> Option<AsyncNode<TInput, T>> {
        self.parent.as_ref()?.upgrade().map(ByRef)
    }

    /// `node.element` (que só falta na raiz antes do `setInput`).
    pub fn element_value(&self) -> InputOrElement<TInput, T> {
        self.element.borrow().clone().expect("o elemento do nó não foi definido")
    }

    /// `node.element as T`.
    pub fn element_t(&self) -> T {
        match self.element_value() {
            InputOrElement::Element(element) => element,
            InputOrElement::Input(_) => panic!("o nó raiz não tem elemento comum"),
        }
    }
}

/// `createAsyncDataTreeNode`.
pub fn create_async_data_tree_node<TInput, T>(
    element: Option<InputOrElement<TInput, T>>,
    parent: Option<&AsyncNode<TInput, T>>,
    id: Option<String>,
    has_children: bool,
    default_collapse_state: Option<ObjectTreeElementCollapseState>,
) -> AsyncNode<TInput, T> {
    ByRef::new(IAsyncDataTreeNode {
        element: RefCell::new(element),
        parent: parent.map(|parent| Rc::downgrade(&parent.0)),
        children: RefCell::new(Vec::new()),
        id,
        refresh_promise: RefCell::new(None),
        has_children: Cell::new(has_children),
        stale: Cell::new(true),
        slow: Cell::new(false),
        default_collapse_state,
        force_expanded: Cell::new(false),
    })
}

/// `isAncestor`.
fn is_ancestor<TInput: Clone, T: Clone>(ancestor: &AsyncNode<TInput, T>, descendant: &AsyncNode<TInput, T>) -> bool {
    let mut current = descendant.parent();

    while let Some(parent) = current {
        if &parent == ancestor {
            return true;
        }

        current = parent.parent();
    }

    false
}

/// `intersects`.
pub fn intersects<TInput: Clone, T: Clone>(node: &AsyncNode<TInput, T>, other: &AsyncNode<TInput, T>) -> bool {
    node == other || is_ancestor(node, other) || is_ancestor(other, node)
}

/// `dfs`.
pub fn dfs<TInput: Clone, T: Clone>(node: &AsyncNode<TInput, T>, f: &mut impl FnMut(&AsyncNode<TInput, T>)) {
    f(node);
    let children = node.children.borrow().clone();

    for child in &children {
        dfs(child, f);
    }
}

// Envoltório do nó

/// O nó da árvore por baixo, sem o tipo dele: o que o [`AsyncDataTreeNodeWrapper`] lê.
pub trait NodeView<TInput, T, F> {
    fn element(&self) -> Option<AsyncNode<TInput, T>>;
    fn children(&self) -> Vec<Rc<dyn NodeView<TInput, T, F>>>;
    fn depth(&self) -> usize;
    fn visible_children_count(&self) -> usize;
    fn visible_child_index(&self) -> i64;
    fn collapsible(&self) -> bool;
    fn collapsed(&self) -> bool;
    fn visible(&self) -> bool;
    fn filter_data(&self) -> Option<F>;
    /// Quantas referências o nó por baixo tem (para o cache saber se ainda interessa).
    fn node_strong_count(&self) -> usize;
}

/// O [`NodeView`] de um nó concreto.
struct ViewOf<N>(Rc<N>);

impl<TInput: 'static, T: 'static, F: 'static, N: TreeNodeApi<Option<AsyncNode<TInput, T>>, F>> NodeView<TInput, T, F> for ViewOf<N> {
    fn element(&self) -> Option<AsyncNode<TInput, T>> {
        self.0.element()
    }

    fn children(&self) -> Vec<Rc<dyn NodeView<TInput, T, F>>> {
        self.0.children().into_iter().map(|node| Rc::new(ViewOf(node)) as Rc<dyn NodeView<TInput, T, F>>).collect()
    }

    fn depth(&self) -> usize {
        self.0.depth()
    }

    fn visible_children_count(&self) -> usize {
        self.0.visible_children_count()
    }

    fn visible_child_index(&self) -> i64 {
        self.0.visible_child_index()
    }

    fn collapsible(&self) -> bool {
        self.0.collapsible()
    }

    fn collapsed(&self) -> bool {
        self.0.collapsed()
    }

    fn visible(&self) -> bool {
        self.0.visible()
    }

    fn filter_data(&self) -> Option<F> {
        self.0.filter_data()
    }

    fn node_strong_count(&self) -> usize {
        Rc::strong_count(&self.0)
    }
}

/// O que um cache de envoltórios precisa saber de um envoltório: se só o cache ainda o referencia.
pub trait Orphanable {
    fn is_orphan(&self) -> bool;
}

/// O `WeakMapper` (ver o `//!`): o mesmo envoltório para o mesmo nó.
pub struct WeakMapper<W: Orphanable> {
    entries: RefCell<HashMap<usize, Rc<W>>>,
    /// O tamanho do mapa a partir do qual a próxima inserção varre as entradas órfãs.
    prune_at: Cell<usize>,
}

impl<W: Orphanable> Default for WeakMapper<W> {
    fn default() -> Self {
        Self { entries: RefCell::default(), prune_at: Cell::new(Self::MIN_PRUNE_SIZE) }
    }
}

impl<W: Orphanable> WeakMapper<W> {
    const MIN_PRUNE_SIZE: usize = 64;

    /// `map(node)`: o envoltório do nó, criado por `make` na primeira vez.
    pub(crate) fn map<N: ?Sized>(&self, node: &Rc<N>, make: impl FnOnce() -> W) -> Rc<W> {
        let key = Rc::as_ptr(node).cast::<()>() as usize;
        let mut entries = self.entries.borrow_mut();

        if let Some(wrapper) = entries.get(&key) {
            return wrapper.clone();
        }

        if entries.len() >= self.prune_at.get() {
            entries.retain(|_, wrapper| !wrapper.is_orphan());
            self.prune_at.set((entries.len() * 2).max(Self::MIN_PRUNE_SIZE));
        }

        let wrapper = Rc::new(make());
        entries.insert(key, wrapper.clone());
        wrapper
    }
}

/// `AsyncDataTreeNodeWrapper<TInput, T, TFilterData>`: o `ITreeNode<TInput | T, TFilterData>` do renderer.
pub struct AsyncDataTreeNodeWrapper<TInput, T, F> {
    node: Rc<dyn NodeView<TInput, T, F>>,
}

impl<TInput: 'static, T: 'static, F: 'static> AsyncDataTreeNodeWrapper<TInput, T, F> {
    /// O envoltório do nó da árvore, pelo cache.
    pub(crate) fn mapped<N: TreeNodeApi<Option<AsyncNode<TInput, T>>, F>>(mapper: &WeakMapper<Self>, node: &Rc<N>) -> Rc<Self> {
        mapper.map(node, || Self { node: Rc::new(ViewOf(node.clone())) })
    }
}

impl<TInput, T, F> Orphanable for AsyncDataTreeNodeWrapper<TInput, T, F> {
    fn is_orphan(&self) -> bool {
        self.node.node_strong_count() <= 1
    }
}

impl<TInput: Clone + 'static, T: Clone + 'static, F: Clone + 'static> TreeNodeApi<InputOrElement<TInput, T>, F> for AsyncDataTreeNodeWrapper<TInput, T, F> {
    fn element(&self) -> InputOrElement<TInput, T> {
        self.node.element().expect("o nó raiz não tem elemento").element_value()
    }

    fn children(&self) -> Vec<Rc<Self>> {
        self.node.children().into_iter().map(|node| Rc::new(Self { node })).collect()
    }

    fn depth(&self) -> usize {
        self.node.depth()
    }

    fn visible_children_count(&self) -> usize {
        self.node.visible_children_count()
    }

    fn visible_child_index(&self) -> i64 {
        self.node.visible_child_index()
    }

    fn collapsible(&self) -> bool {
        self.node.collapsible()
    }

    fn collapsed(&self) -> bool {
        self.node.collapsed()
    }

    fn visible(&self) -> bool {
        self.node.visible()
    }

    fn filter_data(&self) -> Option<F> {
        self.node.filter_data()
    }
}

// Renderer

/// O `ITreeRenderer<T, TFilterData, TTemplateData>` que o usuário implementa para a árvore de dados
/// assíncrona: o nó é o [`AsyncDataTreeNodeWrapper`].
pub trait IAsyncDataTreeRenderer<TInput: Clone + 'static, T: Clone + 'static, F: Clone + 'static> {
    fn template_id(&self) -> String;
    fn render_template(&self, container: NodeId) -> TemplateData;
    fn render_element(&self, node: &Rc<AsyncDataTreeNodeWrapper<TInput, T, F>>, index: usize, template_data: &TemplateData, details: Option<&TreeElementRenderDetails>);
    fn dispose_element(&self, _node: &Rc<AsyncDataTreeNodeWrapper<TInput, T, F>>, _index: i64, _template_data: &TemplateData, _details: Option<&TreeElementRenderDetails>) {}
    fn dispose_template(&self, template_data: &TemplateData);
}

/// `IDataTreeListTemplateData<T>`.
pub(crate) struct DataTreeListTemplateData {
    pub(crate) template_data: TemplateData,
}

/// O `templateData` do usuário dentro do [`DataTreeListTemplateData`].
pub(crate) fn user_template_data(template_data: &TemplateData) -> TemplateData {
    Rc::downcast::<DataTreeListTemplateData>(template_data.clone()).unwrap_or_else(|_| panic!("o template não é da árvore de dados assíncrona")).template_data.clone()
}

/// O cache de envoltórios dos nós da árvore (`AsyncDataTreeNodeMapper`).
pub type AsyncNodeMapper<TInput, T, F> = WeakMapper<AsyncDataTreeNodeWrapper<TInput, T, F>>;

/// `AsyncDataTreeRenderer<TInput, T, TFilterData, TTemplateData>`: vale para qualquer modelo cujo elemento
/// seja o nó interno (o `ObjectTreeModel` e o comprimível). `R` é o renderer do usuário e `X` o que a
/// variante comprimível precisa a mais (o mapa dos envoltórios dos nós comprimidos; `()` na comum).
pub struct AsyncDataTreeRenderer<TInput: Clone + 'static, T: Clone + 'static, F: Clone + 'static, R: ?Sized, X = ()> {
    pub(crate) renderer: Rc<R>,
    pub(crate) node_mapper: Rc<AsyncNodeMapper<TInput, T, F>>,
    pub(crate) extra: X,
    ui: Ui,
    on_did_change_twistie_state: BaseEvent<AsyncNode<TInput, T>>,
}

impl<TInput: Clone + 'static, T: Clone + 'static, F: Clone + 'static, R: ?Sized, X> AsyncDataTreeRenderer<TInput, T, F, R, X> {
    pub fn new(ui: &Ui, renderer: Rc<R>, node_mapper: Rc<AsyncNodeMapper<TInput, T, F>>, on_did_change_twistie_state: BaseEvent<AsyncNode<TInput, T>>, extra: X) -> Self {
        Self { renderer, node_mapper, extra, ui: ui.clone(), on_did_change_twistie_state }
    }
}

impl<TInput, T, F, R, X, M> ITreeRenderer<M> for AsyncDataTreeRenderer<TInput, T, F, R, X>
where
    TInput: Clone + 'static,
    T: Clone + 'static,
    F: Clone + 'static,
    R: IAsyncDataTreeRenderer<TInput, T, F> + ?Sized,
    M: ITreeModel<Element = Option<AsyncNode<TInput, T>>, FilterData = F>,
{
    fn template_id(&self) -> String {
        self.renderer.template_id()
    }

    fn render_template(&self, container: NodeId) -> TemplateData {
        Rc::new(DataTreeListTemplateData { template_data: self.renderer.render_template(container) })
    }

    fn render_element(&self, node: &Rc<M::Node>, index: usize, template_data: &TemplateData, details: Option<&TreeElementRenderDetails>) {
        self.renderer.render_element(&AsyncDataTreeNodeWrapper::mapped(&self.node_mapper, node), index, &user_template_data(template_data), details);
    }

    fn render_twistie(&self, element: &M::Element, twistie: NodeId) -> bool {
        let loading = ThemeIcon::from_id("tree-item-loading").as_class_name_array();
        let mut tree = self.ui.tree_mut();

        if element.as_ref().is_some_and(|element| element.slow.get()) {
            for class in &loading {
                tree.add_class(twistie, class);
            }

            true
        } else {
            for class in &loading {
                tree.remove_class(twistie, class);
            }

            false
        }
    }

    fn on_did_change_twistie_state(&self) -> Option<BaseEvent<M::Element>> {
        Some(self.on_did_change_twistie_state.map(|node| Some(node.clone()), None))
    }

    fn dispose_element(&self, node: &Rc<M::Node>, index: i64, template_data: &TemplateData, details: Option<&TreeElementRenderDetails>) {
        self.renderer.dispose_element(&AsyncDataTreeNodeWrapper::mapped(&self.node_mapper, node), index, &user_template_data(template_data), details);
    }

    fn dispose_template(&self, template_data: &TemplateData) {
        self.renderer.dispose_template(&user_template_data(template_data));
    }
}

// Opções

/// O `defaultFindVisibility` das opções: `TreeVisibility` fixa ou função do elemento.
pub enum DefaultFindVisibility<T> {
    Fixed(TreeVisibility),
    With(Rc<dyn Fn(&T) -> TreeVisibility>),
}

impl<T> Clone for DefaultFindVisibility<T> {
    fn clone(&self) -> Self {
        match self {
            Self::Fixed(visibility) => Self::Fixed(*visibility),
            Self::With(f) => Self::With(f.clone()),
        }
    }
}

/// O `collapseByDefault` das opções.
pub type CollapseByDefault<T> = Rc<dyn Fn(&T) -> bool>;

/// O `stickyScrollNodeSourceRangeProvider` das opções.
pub type SourceRangeProvider<T> = Rc<dyn Fn(&T, StickyScrollNodeSourceRange) -> Option<StickyScrollNodeSourceRange>>;

/// `IAsyncDataTreeOptions<T, TFilterData>` (ver o `//!` para o que fica de fora).
pub struct AsyncDataTreeOptions<T, F = ()> {
    pub update: AbstractTreeOptionsUpdate<T>,
    /// As opções da `ListView`.
    pub view: ListViewOptions,
    pub identity_provider: Option<Rc<dyn IIdentityProvider<T>>>,
    pub keyboard_navigation_label_provider: Option<Rc<dyn IKeyboardNavigationLabelProvider<T>>>,
    pub keyboard_support: Option<bool>,
    pub collapse_by_default: Option<CollapseByDefault<T>>,
    pub allow_non_collapsible_parents: bool,
    pub filter: Option<Rc<dyn ITreeFilter<T, F>>>,
    pub default_find_visibility: Option<DefaultFindVisibility<T>>,
    pub sticky_scroll_node_source_range_provider: Option<SourceRangeProvider<T>>,
    pub disable_expand_on_spacebar: bool,
    pub find_widget_enabled: Option<bool>,
    pub sorter: Option<Rc<dyn ITreeSorter<T>>>,
    pub auto_expand_single_children: bool,
}

impl<T, F> Default for AsyncDataTreeOptions<T, F> {
    fn default() -> Self {
        Self {
            update: AbstractTreeOptionsUpdate::default(),
            view: ListViewOptions::default(),
            identity_provider: None,
            keyboard_navigation_label_provider: None,
            keyboard_support: None,
            collapse_by_default: None,
            allow_non_collapsible_parents: false,
            filter: None,
            default_find_visibility: None,
            sticky_scroll_node_source_range_provider: None,
            disable_expand_on_spacebar: false,
            find_widget_enabled: None,
            sorter: None,
            auto_expand_single_children: false,
        }
    }
}

/// `IAsyncDataTreeUpdateChildrenOptions<T>`: as opções do `setChildren` do `ObjectTree` que a árvore
/// repassa. Os `onDidCreateNode`/`onDidDeleteNode` do original receberiam os nós internos (a tipagem do
/// original mente), e por isso não existem aqui.
pub struct AsyncDataTreeUpdateChildrenOptions<T> {
    pub diff_depth: Option<usize>,
    pub diff_identity_provider: Option<Rc<dyn IIdentityProvider<T>>>,
}

impl<T> Default for AsyncDataTreeUpdateChildrenOptions<T> {
    fn default() -> Self {
        Self { diff_depth: None, diff_identity_provider: None }
    }
}

/// `IAsyncDataTreeViewState`.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct IAsyncDataTreeViewState {
    pub focus: Option<Vec<String>>,
    pub selection: Option<Vec<String>>,
    pub expanded: Option<Vec<String>>,
    pub scroll_top: Option<f64>,
}

/// `IAsyncDataTreeViewStateContext<TInput, T>`.
pub struct ViewStateContext<TInput, T> {
    pub view_state: IAsyncDataTreeViewState,
    pub selection: RefCell<Vec<AsyncNode<TInput, T>>>,
    pub focus: RefCell<Vec<AsyncNode<TInput, T>>>,
}

impl<TInput, T> ViewStateContext<TInput, T> {
    pub fn new(view_state: IAsyncDataTreeViewState) -> Rc<Self> {
        Rc::new(Self { view_state, selection: RefCell::default(), focus: RefCell::default() })
    }
}

// Adaptadores das opções (o `asObjectTreeOptions`)

/// O `IListVirtualDelegate<T>` do usuário visto como o dos nós internos (o `ComposedTreeDelegate`).
pub(crate) struct AsyncNodeDelegate<T>(pub(crate) Rc<dyn IListVirtualDelegate<T>>);

impl<TInput: Clone, T: Clone> IListVirtualDelegate<AsyncNode<TInput, T>> for AsyncNodeDelegate<T> {
    fn get_height(&self, element: &AsyncNode<TInput, T>) -> f64 {
        self.0.get_height(&element.element_t())
    }

    fn get_template_id(&self, element: &AsyncNode<TInput, T>) -> String {
        self.0.get_template_id(&element.element_t())
    }

    fn has_dynamic_height(&self, element: &AsyncNode<TInput, T>) -> Option<bool> {
        self.0.has_dynamic_height(&element.element_t())
    }

    fn get_dynamic_height(&self, element: &AsyncNode<TInput, T>) -> Option<f64> {
        self.0.get_dynamic_height(&element.element_t())
    }

    fn set_dynamic_height(&self, element: &AsyncNode<TInput, T>, height: f64) {
        self.0.set_dynamic_height(&element.element_t(), height);
    }
}

/// O provedor de identidade do usuário (sobre `T`) visto como o dos nós internos.
pub(crate) struct NodeIdentityProvider<T>(pub(crate) Rc<dyn IIdentityProvider<T>>);

impl<TInput: Clone, T: Clone> IIdentityProvider<AsyncNode<TInput, T>> for NodeIdentityProvider<T> {
    fn get_id(&self, element: &AsyncNode<TInput, T>) -> String {
        self.0.get_id(&element.element_t())
    }

    fn get_group_id(&self, element: &AsyncNode<TInput, T>) -> Option<GroupId> {
        self.0.get_group_id(&element.element_t())
    }
}

impl<TInput: Clone, T: Clone> IIdentityProvider<Option<AsyncNode<TInput, T>>> for NodeIdentityProvider<T> {
    fn get_id(&self, element: &Option<AsyncNode<TInput, T>>) -> String {
        self.0.get_id(&element.as_ref().expect("a raiz não tem identidade").element_t())
    }

    fn get_group_id(&self, element: &Option<AsyncNode<TInput, T>>) -> Option<GroupId> {
        self.0.get_group_id(&element.as_ref().expect("a raiz não tem identidade").element_t())
    }
}

struct NodeFilter<T, F>(Rc<dyn ITreeFilter<T, F>>);

impl<TInput: Clone, T: Clone, F> ITreeFilter<Option<AsyncNode<TInput, T>>, F> for NodeFilter<T, F> {
    fn filter(&self, element: &Option<AsyncNode<TInput, T>>, parent_visibility: TreeVisibility) -> TreeFilterResult<F> {
        self.0.filter(&element.as_ref().expect("a raiz não é filtrada").element_t(), parent_visibility)
    }
}

struct NodeLabelProvider<T>(Rc<dyn IKeyboardNavigationLabelProvider<T>>);

impl<TInput: Clone, T: Clone> IKeyboardNavigationLabelProvider<Option<AsyncNode<TInput, T>>> for NodeLabelProvider<T> {
    fn get_keyboard_navigation_label(&self, element: &Option<AsyncNode<TInput, T>>) -> Vec<Option<String>> {
        self.0.get_keyboard_navigation_label(&element.as_ref().expect("a raiz não tem rótulo").element_t())
    }
}

/// O `IAbstractTreeOptionsUpdate<T>` sobre os nós internos.
fn update_as_node_options<TInput: Clone + 'static, T: Clone + 'static>(update: &AbstractTreeOptionsUpdate<T>) -> AbstractTreeOptionsUpdate<Option<AsyncNode<TInput, T>>> {
    AbstractTreeOptionsUpdate {
        indent: update.indent,
        default_indent: update.default_indent,
        render_indent_guides: update.render_indent_guides,
        hide_twisties_of_childless_elements: update.hide_twisties_of_childless_elements,
        twistie_additional_css_class: update.twistie_additional_css_class.clone().map(|f| {
            Rc::new(move |element: &Option<AsyncNode<TInput, T>>| f(&element.as_ref().expect("a raiz não tem classe").element_t())) as Rc<dyn Fn(&Option<AsyncNode<TInput, T>>) -> Option<String>>
        }),
        padding_bottom: update.padding_bottom,
        padding_top: update.padding_top,
        multiple_selection_support: update.multiple_selection_support,
        default_find_mode: update.default_find_mode,
        default_find_match_type: update.default_find_match_type,
        show_not_found_message: update.show_not_found_message,
        smooth_scrolling: update.smooth_scrolling,
        scroll_by_page: update.scroll_by_page,
        mouse_wheel_scroll_sensitivity: update.mouse_wheel_scroll_sensitivity,
        fast_scroll_sensitivity: update.fast_scroll_sensitivity,
        expand_on_double_click: update.expand_on_double_click,
        expand_only_on_twistie_click: update.expand_only_on_twistie_click.clone().map(|value| match value {
            ExpandOnlyOnTwistieClick::Always(always) => ExpandOnlyOnTwistieClick::Always(always),
            ExpandOnlyOnTwistieClick::When(f) => {
                ExpandOnlyOnTwistieClick::When(Rc::new(move |element: &Option<AsyncNode<TInput, T>>| f(&element.as_ref().expect("a raiz não tem clique de twistie").element_t())))
            }
        }),
        enable_sticky_scroll: update.enable_sticky_scroll,
        sticky_scroll_max_item_count: update.sticky_scroll_max_item_count,
        sticky_scroll_max_node_height: update.sticky_scroll_max_node_height,
    }
}

/// `asObjectTreeOptions`: as opções da árvore de dados como as da árvore abstrata do modelo por baixo.
/// O `collapseByDefault` é sempre ligado (o padrão por elemento vai no `defaultCollapseState`), o
/// `sorter` fica de fora (a ordenação é da árvore de dados) e o `findWidgetEnabled` é ligado (o original
/// o sobrescreve com `!asyncFindEnabled`).
pub(crate) fn as_object_tree_options<TInput, T, F, M>(options: &AsyncDataTreeOptions<T, F>) -> AbstractTreeOptions<M>
where
    TInput: Clone + 'static,
    T: Clone + 'static,
    F: Clone + 'static,
    M: ITreeModel<Element = Option<AsyncNode<TInput, T>>, FilterData = F>,
{
    let default_find_visibility = options.default_find_visibility.clone();

    AbstractTreeOptions {
        update: update_as_node_options(&options.update),
        view: options.view.clone(),
        identity_provider: options.identity_provider.clone().map(|provider| Rc::new(NodeIdentityProvider(provider)) as Rc<dyn IIdentityProvider<M::Element>>),
        keyboard_navigation_label_provider: options.keyboard_navigation_label_provider.clone().map(|provider| Rc::new(NodeLabelProvider(provider)) as Rc<dyn IKeyboardNavigationLabelProvider<M::Element>>),
        keyboard_support: options.keyboard_support,
        collapse_by_default: true,
        allow_non_collapsible_parents: options.allow_non_collapsible_parents,
        filter: options.filter.clone().map(|filter| Rc::new(NodeFilter(filter)) as Rc<dyn ITreeFilter<M::Element, F>>),
        default_find_visibility: Some(Rc::new(move |element: &M::Element| {
            let element = element.as_ref().expect("a raiz não tem visibilidade de busca");

            if element.has_children.get() && element.stale.get() {
                TreeVisibility::Visible
            } else {
                match &default_find_visibility {
                    Some(DefaultFindVisibility::Fixed(visibility)) => *visibility,
                    None => TreeVisibility::Recurse,
                    Some(DefaultFindVisibility::With(f)) => f(&element.element_t()),
                }
            }
        })),
        sticky_scroll_delegate: None,
        sticky_scroll_node_source_range_provider: options.sticky_scroll_node_source_range_provider.clone().map(|provider| {
            Rc::new(move |element: &M::Element, default_range: StickyScrollNodeSourceRange| provider(&element.as_ref().expect("a raiz não tem faixa").element_t(), default_range))
                as Rc<dyn Fn(&M::Element, StickyScrollNodeSourceRange) -> Option<StickyScrollNodeSourceRange>>
        }),
        disable_expand_on_spacebar: options.disable_expand_on_spacebar,
        find_widget_enabled: Some(true),
    }
}
