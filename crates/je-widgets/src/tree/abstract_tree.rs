//! Porte de `src/vs/base/browser/ui/tree/abstractTree.ts`: a [`AbstractTree`], a árvore sobre a lista
//! virtualizada. O renderer com recuo e guias está em [`crate::tree::tree_renderer`], o `Trait` de
//! foco/seleção/âncora em [`crate::tree::tree_trait`], o sticky scroll em [`crate::tree::sticky_scroll`] e
//! o filtro e o controlador de busca em [`crate::tree::find`].
//!
//! Do TypeScript para o Rust:
//! - `AbstractTree<T, TFilterData, TRef>` é `AbstractTree<M: ITreeModel>`: os três parâmetros e o tipo do
//!   nó são os tipos associados do modelo. O `createModel` abstrato (herança) é o parâmetro `create_model`
//!   do construtor, que roda depois de o `FindFilter` trocar o filtro, como no original. As subclasses
//!   (`ObjectTree`, `IndexTree`) são structs que fazem `Deref` para esta.
//! - A `TreeNodeList` (subclasse da `List`) e o `TreeNodeListMouseController` são o
//!   [`ListOverrides`](crate::list::list_widget::ListOverrides) que a árvore passa à lista.
//! - Os eventos do DOM são os do `je-ui`. `isButton`, `isEditableElement`, `isMonacoEditor`,
//!   `isMonacoCustomToggle` e `isActionItem` (classes de widgets que o `je-ui` ainda não tem) não são
//!   conferidos. A arrastar e soltar (`TreeNodeListDragAndDrop`), o tipo-para-navegar (a `List` do
//!   `je-widgets` não o tem), o `aria-*`, o `createStyleSheet` e o `style()` (CSS gerado) ficam de fora.
//! - O `Relay` de eventos do modelo é um `Emitter` que repassa (o modelo não é trocado depois do
//!   construtor).

use std::cell::{Cell, RefCell};
use std::collections::{HashMap, HashSet};
use std::hash::Hash;
use std::rc::{Rc, Weak};

use je_base::async_::Delayer;
use je_base::event::{Emitter, Event as BaseEvent, EventBufferer};
use je_base::key_codes::KeyCode;
use je_base::lifecycle::{ById, DisposableRef, DisposableStore, IDisposable};
use je_base::map::SetMap;
use je_base::promise::MaybePromise;
use je_base::symbols::Delay;
use je_ui::{EventKind, KeyData, NodeId, Ui};

use crate::common::{store_emitter, throw};
use crate::list::list_types::{BrowserEvent, ElementFilter, GroupId, IIdentityProvider, IKeyboardNavigationLabelProvider, IListRenderer, IListVirtualDelegate, ListMouseEvent};
use crate::list::list_view::{ListViewOptions, ListViewOptionsUpdate};
use crate::list::list_widget::{List, ListOptions, ListOverrides};
use crate::tree::find::{DefaultFindVisibility, FindHandle};
use crate::tree::sticky_scroll::{IStickyScrollDelegate, StickyScrollController, StickyScrollNodeSourceRangeProvider};
use crate::list::list_styles::IListStyles;
use crate::tree::tree_renderer::{Collection, EventCollection, RenderIndentGuides, RenderedIndentGuides, TreeRenderer, TreeRendererOptions, TwistieClassProvider};
use crate::tree::tree_trait::TreeTrait;
use crate::tree::tree_types::{
    CollapseStateChangeEvent, ITreeFilter, ITreeModel, ITreeNavigator, ITreeRenderer, TreeEvent, TreeMouseEvent, TreeMouseEventTarget, TreeNodeApi, tree_error,
};

/// `TreeFindMode`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TreeFindMode {
    Highlight,
    Filter,
}

/// `TreeFindMatchType`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TreeFindMatchType {
    Fuzzy,
    Contiguous,
}

/// `ComposedTreeDelegate<T, N>`.
pub struct ComposedTreeDelegate<M: ITreeModel> {
    delegate: Rc<dyn IListVirtualDelegate<M::Element>>,
}

impl<M: ITreeModel> ComposedTreeDelegate<M> {
    pub fn new(delegate: Rc<dyn IListVirtualDelegate<M::Element>>) -> Self {
        Self { delegate }
    }
}

impl<M: ITreeModel> IListVirtualDelegate<Rc<M::Node>> for ComposedTreeDelegate<M> {
    fn get_height(&self, element: &Rc<M::Node>) -> f64 {
        self.delegate.get_height(&element.element())
    }

    fn get_template_id(&self, element: &Rc<M::Node>) -> String {
        self.delegate.get_template_id(&element.element())
    }

    fn has_dynamic_height(&self, element: &Rc<M::Node>) -> Option<bool> {
        Some(self.delegate.has_dynamic_height(&element.element()).unwrap_or(false))
    }

    fn set_dynamic_height(&self, element: &Rc<M::Node>, height: f64) {
        self.delegate.set_dynamic_height(&element.element(), height);
    }
}

/// `IAbstractTreeViewState`/`AbstractTreeViewState`.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct AbstractTreeViewState {
    pub focus: Vec<String>,
    pub selection: Vec<String>,
    /// `{ [id: string]: 1 | 0 }`.
    pub expanded: HashMap<String, bool>,
    pub scroll_top: f64,
}

impl AbstractTreeViewState {
    /// `AbstractTreeViewState.empty`.
    pub fn empty(scroll_top: f64) -> Self {
        Self { scroll_top, ..Self::default() }
    }
}

/// `expandOnlyOnTwistieClick?: boolean | ((e: T) => boolean)`.
pub enum ExpandOnlyOnTwistieClick<E> {
    Always(bool),
    When(Rc<dyn Fn(&E) -> bool>),
}

impl<E> Clone for ExpandOnlyOnTwistieClick<E> {
    fn clone(&self) -> Self {
        match self {
            Self::Always(b) => Self::Always(*b),
            Self::When(f) => Self::When(f.clone()),
        }
    }
}

/// `IAbstractTreeOptionsUpdate<T>`.
pub struct AbstractTreeOptionsUpdate<E> {
    pub indent: Option<f64>,
    pub default_indent: Option<f64>,
    pub render_indent_guides: Option<RenderIndentGuides>,
    pub hide_twisties_of_childless_elements: Option<bool>,
    pub twistie_additional_css_class: Option<TwistieClassProvider<E>>,
    pub padding_bottom: Option<f64>,
    pub padding_top: Option<f64>,
    pub multiple_selection_support: Option<bool>,
    pub default_find_mode: Option<TreeFindMode>,
    pub default_find_match_type: Option<TreeFindMatchType>,
    pub show_not_found_message: Option<bool>,
    pub smooth_scrolling: Option<bool>,
    pub scroll_by_page: Option<bool>,
    pub mouse_wheel_scroll_sensitivity: Option<f64>,
    pub fast_scroll_sensitivity: Option<f64>,
    pub expand_on_double_click: Option<bool>,
    pub expand_only_on_twistie_click: Option<ExpandOnlyOnTwistieClick<E>>,
    pub enable_sticky_scroll: Option<bool>,
    pub sticky_scroll_max_item_count: Option<usize>,
    pub sticky_scroll_max_node_height: Option<f64>,
}

impl<E> Default for AbstractTreeOptionsUpdate<E> {
    fn default() -> Self {
        Self {
            indent: None,
            default_indent: None,
            render_indent_guides: None,
            hide_twisties_of_childless_elements: None,
            twistie_additional_css_class: None,
            padding_bottom: None,
            padding_top: None,
            multiple_selection_support: None,
            default_find_mode: None,
            default_find_match_type: None,
            show_not_found_message: None,
            smooth_scrolling: None,
            scroll_by_page: None,
            mouse_wheel_scroll_sensitivity: None,
            fast_scroll_sensitivity: None,
            expand_on_double_click: None,
            expand_only_on_twistie_click: None,
            enable_sticky_scroll: None,
            sticky_scroll_max_item_count: None,
            sticky_scroll_max_node_height: None,
        }
    }
}

impl<E> Clone for AbstractTreeOptionsUpdate<E> {
    fn clone(&self) -> Self {
        Self {
            indent: self.indent,
            default_indent: self.default_indent,
            render_indent_guides: self.render_indent_guides,
            hide_twisties_of_childless_elements: self.hide_twisties_of_childless_elements,
            twistie_additional_css_class: self.twistie_additional_css_class.clone(),
            padding_bottom: self.padding_bottom,
            padding_top: self.padding_top,
            multiple_selection_support: self.multiple_selection_support,
            default_find_mode: self.default_find_mode,
            default_find_match_type: self.default_find_match_type,
            show_not_found_message: self.show_not_found_message,
            smooth_scrolling: self.smooth_scrolling,
            scroll_by_page: self.scroll_by_page,
            mouse_wheel_scroll_sensitivity: self.mouse_wheel_scroll_sensitivity,
            fast_scroll_sensitivity: self.fast_scroll_sensitivity,
            expand_on_double_click: self.expand_on_double_click,
            expand_only_on_twistie_click: self.expand_only_on_twistie_click.clone(),
            enable_sticky_scroll: self.enable_sticky_scroll,
            sticky_scroll_max_item_count: self.sticky_scroll_max_item_count,
            sticky_scroll_max_node_height: self.sticky_scroll_max_node_height,
        }
    }
}

impl<E> AbstractTreeOptionsUpdate<E> {
    /// `{ ...this._options, ...optionsUpdate }`: os campos presentes na atualização vencem.
    pub fn merge(&mut self, update: &AbstractTreeOptionsUpdate<E>) {
        macro_rules! merge_fields {
            ($($field:ident),*) => { $(if update.$field.is_some() { self.$field = update.$field.clone(); })* };
        }
        merge_fields!(
            indent,
            default_indent,
            render_indent_guides,
            hide_twisties_of_childless_elements,
            twistie_additional_css_class,
            padding_bottom,
            padding_top,
            multiple_selection_support,
            default_find_mode,
            default_find_match_type,
            show_not_found_message,
            smooth_scrolling,
            scroll_by_page,
            mouse_wheel_scroll_sensitivity,
            fast_scroll_sensitivity,
            expand_on_double_click,
            expand_only_on_twistie_click,
            enable_sticky_scroll,
            sticky_scroll_max_item_count,
            sticky_scroll_max_node_height
        );
    }

    fn renderer_options(&self) -> TreeRendererOptions<E> {
        TreeRendererOptions {
            indent: self.indent,
            default_indent: self.default_indent,
            render_indent_guides: self.render_indent_guides,
            hide_twisties_of_childless_elements: self.hide_twisties_of_childless_elements,
            twistie_additional_css_class: self.twistie_additional_css_class.clone(),
        }
    }

    fn view_options_update(&self) -> ListViewOptionsUpdate {
        ListViewOptionsUpdate {
            smooth_scrolling: self.smooth_scrolling,
            scroll_by_page: self.scroll_by_page,
            mouse_wheel_scroll_sensitivity: self.mouse_wheel_scroll_sensitivity,
            fast_scroll_sensitivity: self.fast_scroll_sensitivity,
            padding_top: self.padding_top,
            padding_bottom: self.padding_bottom,
        }
    }
}

/// `IAbstractTreeOptions<T, TFilterData>`. O `contextViewProvider` e o `findWidgetStyles` do original
/// (widgets de busca que o `je-ui` ainda não tem) ficam de fora; ver [`crate::tree::find`].
pub struct AbstractTreeOptions<M: ITreeModel> {
    pub update: AbstractTreeOptionsUpdate<M::Element>,
    /// As opções da `ListView` (`setRowHeight`, `supportDynamicHeights`, `mouseSupport`, ...).
    pub view: ListViewOptions,
    pub identity_provider: Option<Rc<dyn IIdentityProvider<M::Element>>>,
    pub keyboard_navigation_label_provider: Option<Rc<dyn IKeyboardNavigationLabelProvider<M::Element>>>,
    pub keyboard_support: Option<bool>,
    pub collapse_by_default: bool,
    pub allow_non_collapsible_parents: bool,
    pub filter: Option<Rc<dyn ITreeFilter<M::Element, M::FilterData>>>,
    pub default_find_visibility: Option<DefaultFindVisibility<M::Element>>,
    pub sticky_scroll_delegate: Option<Rc<dyn IStickyScrollDelegate<M>>>,
    pub sticky_scroll_node_source_range_provider: Option<StickyScrollNodeSourceRangeProvider<M::Element>>,
    pub disable_expand_on_spacebar: bool,
    /// `findWidgetEnabled` (padrão: ligado, se houver `keyboard_navigation_label_provider`).
    pub find_widget_enabled: Option<bool>,
}

impl<M: ITreeModel> Default for AbstractTreeOptions<M> {
    fn default() -> Self {
        Self {
            update: AbstractTreeOptionsUpdate::default(),
            view: ListViewOptions::default(),
            identity_provider: None,
            keyboard_navigation_label_provider: None,
            keyboard_support: None,
            collapse_by_default: false,
            allow_non_collapsible_parents: false,
            filter: None,
            default_find_visibility: None,
            sticky_scroll_delegate: None,
            sticky_scroll_node_source_range_provider: None,
            disable_expand_on_spacebar: false,
            find_widget_enabled: None,
        }
    }
}

impl<M: ITreeModel> Clone for AbstractTreeOptions<M> {
    fn clone(&self) -> Self {
        Self {
            update: self.update.clone(),
            view: self.view.clone(),
            identity_provider: self.identity_provider.clone(),
            keyboard_navigation_label_provider: self.keyboard_navigation_label_provider.clone(),
            keyboard_support: self.keyboard_support,
            collapse_by_default: self.collapse_by_default,
            allow_non_collapsible_parents: self.allow_non_collapsible_parents,
            filter: self.filter.clone(),
            default_find_visibility: self.default_find_visibility.clone(),
            sticky_scroll_delegate: self.sticky_scroll_delegate.clone(),
            sticky_scroll_node_source_range_provider: self.sticky_scroll_node_source_range_provider.clone(),
            disable_expand_on_spacebar: self.disable_expand_on_spacebar,
            find_widget_enabled: self.find_widget_enabled,
        }
    }
}

/// O `IIdentityProvider<ITreeNode<T>>` que o `asListOptions` monta sobre o do usuário.
struct NodeIdentityProvider<M: ITreeModel> {
    provider: Rc<dyn IIdentityProvider<M::Element>>,
}

impl<M: ITreeModel> IIdentityProvider<Rc<M::Node>> for NodeIdentityProvider<M> {
    fn get_id(&self, node: &Rc<M::Node>) -> String {
        self.provider.get_id(&node.element())
    }

    fn get_group_id(&self, node: &Rc<M::Node>) -> Option<GroupId> {
        self.provider.get_group_id(&node.element())
    }
}

/// `isStickyScrollContainer`.
fn is_sticky_scroll_container(ui: &Ui, node: Option<NodeId>) -> bool {
    node.is_some_and(|n| ui.tree().has_class(n, "monaco-tree-sticky-container"))
}

/// `isStickyScrollElement`.
fn is_sticky_scroll_element(ui: &Ui, node: Option<NodeId>) -> bool {
    node.is_some_and(|n| ui.tree().has_class(n, "monaco-tree-sticky-row"))
}

/// `hasParentWithClass(node, clazz, stopClazz)`.
fn has_parent_with_class(ui: &Ui, node: Option<NodeId>, class: &str, stop_class: &str) -> bool {
    let tree = ui.tree();
    let mut current = node;

    while let Some(n) = current {
        if tree.has_class(n, class) {
            return true;
        }

        if tree.has_class(n, stop_class) {
            return false;
        }

        current = tree.parent(n);
    }

    false
}

/// `asTreeMouseEvent`.
fn as_tree_mouse_event<M: ITreeModel>(ui: &Ui, event: &ListMouseEvent<Rc<M::Node>>) -> TreeMouseEvent<M::Element> {
    let target = event.browser_event.target;
    let kind = if has_parent_with_class(ui, target, "monaco-tl-twistie", "monaco-tl-row") {
        TreeMouseEventTarget::Twistie
    } else if has_parent_with_class(ui, target, "monaco-tl-contents", "monaco-tl-row") {
        TreeMouseEventTarget::Element
    } else if has_parent_with_class(ui, target, "monaco-tree-type-filter", "monaco-list") {
        TreeMouseEventTarget::Filter
    } else {
        TreeMouseEventTarget::Unknown
    };

    TreeMouseEvent { browser_event: event.browser_event, is_handled_by_list: event.is_handled_by_list.clone(), element: event.element.as_ref().map(|n| n.element()), target: kind }
}

/// A `TreeNodeList` e o `TreeNodeListMouseController`.
struct TreeListOverrides<M: ITreeModel>
where
    M::Element: Eq + Hash,
{
    tree: Weak<AbstractTree<M>>,
}

impl<M: ITreeModel> ListOverrides<Rc<M::Node>> for TreeListOverrides<M>
where
    M::Element: Eq + Hash,
{
    fn after_splice(&self, list: &List<Rc<M::Node>>, start: usize, elements: &[Rc<M::Node>]) {
        let Some(tree) = self.tree.upgrade() else { return };

        if elements.is_empty() {
            return;
        }

        let mut additional_focus: Vec<usize> = Vec::new();
        let mut additional_selection: Vec<usize> = Vec::new();
        let mut anchor: Option<usize> = None;

        for (index, node) in elements.iter().enumerate() {
            if tree.focus.has(node) {
                additional_focus.push(start + index);
            }

            if tree.selection.has(node) {
                additional_selection.push(start + index);
            }

            if tree.anchor.has(node) {
                anchor = Some(start + index);
            }
        }

        if !additional_focus.is_empty() {
            let mut focus = list.get_focus();
            focus.extend(additional_focus);
            list.set_focus_base(distinct(focus), None);
        }

        if !additional_selection.is_empty() {
            let mut selection = list.get_selection();
            selection.extend(additional_selection);
            list.set_selection_base(distinct(selection), None);
        }

        if let Some(anchor) = anchor {
            list.set_anchor_base(Some(anchor));
        }
    }

    fn after_set_focus(&self, list: &List<Rc<M::Node>>, indexes: &[usize], browser_event: Option<BrowserEvent>) {
        if let Some(tree) = self.tree.upgrade() {
            tree.focus.set(indexes.iter().map(|i| list.element(*i)).collect(), browser_event);
        }
    }

    fn after_set_selection(&self, list: &List<Rc<M::Node>>, indexes: &[usize], browser_event: Option<BrowserEvent>) {
        if let Some(tree) = self.tree.upgrade() {
            tree.selection.set(indexes.iter().map(|i| list.element(*i)).collect(), browser_event);
        }
    }

    fn after_set_anchor(&self, list: &List<Rc<M::Node>>, index: Option<usize>) {
        if let Some(tree) = self.tree.upgrade() {
            match index {
                None => tree.anchor.set(Vec::new(), None),
                Some(index) => tree.anchor.set(vec![list.element(index)], None),
            }
        }
    }

    fn view_pointer(&self, list: &List<Rc<M::Node>>, e: &ListMouseEvent<Rc<M::Node>>) {
        let Some(tree) = self.tree.upgrade() else { return list.default_view_pointer(e) };

        // `isButton`, `isEditableElement` e `isMonacoEditor` do original: widgets que o `je-ui` não tem.
        if e.is_handled_by_list.get() {
            return;
        }

        let Some(node) = &e.element else { return list.default_view_pointer(e) };

        if List::<Rc<M::Node>>::is_selection_range_change_event(e) || List::<Rc<M::Node>>::is_selection_single_change_event(e) {
            return list.default_view_pointer(e);
        }

        let target = e.browser_event.target;
        let on_twistie = target.is_some_and(|t| tree.ui.tree().has_class(t, "monaco-tl-twistie"));
        let is_sticky_element = is_sticky_scroll_element(&tree.ui, target);

        let expand_only_on_twistie_click = if is_sticky_element {
            true
        } else {
            match tree.expand_only_on_twistie_click() {
                ExpandOnlyOnTwistieClick::When(f) => f(&node.element()),
                ExpandOnlyOnTwistieClick::Always(b) => b,
            }
        };

        if !is_sticky_element {
            if expand_only_on_twistie_click && !on_twistie && e.browser_event.click_count != 2 {
                return list.default_view_pointer(e);
            }

            if !tree.expand_on_double_click() && e.browser_event.click_count == 2 {
                return list.default_view_pointer(e);
            }
        } else {
            tree.handle_sticky_scroll_mouse_event(list, node);
        }

        if node.collapsible() && (!is_sticky_element || on_twistie) {
            let location = tree.model.get_node_location(node);
            let recursive = e.browser_event.modifiers.alt;

            tree.set_focus(std::slice::from_ref(&location), None);
            tree.toggle_collapsed(&location, recursive);

            if on_twistie {
                // Não marcar antes de chamar o `default_view_pointer`: ele rejeitaria o evento como tratado.
                e.is_handled_by_list.set(true);
                return;
            }
        }

        if !is_sticky_element {
            list.default_view_pointer(e);
        }
    }

    fn double_click(&self, list: &List<Rc<M::Node>>, e: &ListMouseEvent<Rc<M::Node>>) {
        let Some(tree) = self.tree.upgrade() else { return list.default_double_click(e) };

        let on_twistie = e.browser_event.target.is_some_and(|t| tree.ui.tree().has_class(t, "monaco-tl-twistie"));

        if on_twistie || !tree.expand_on_double_click() {
            return;
        }

        if e.is_handled_by_list.get() {
            return;
        }

        list.default_double_click(e);
    }

    /// Para o foco do DOM não ser roubado (por exemplo, com o menu de contexto).
    fn mouse_down(&self, list: &List<Rc<M::Node>>, e: &ListMouseEvent<Rc<M::Node>>) {
        let Some(tree) = self.tree.upgrade() else { return list.default_mouse_down(e) };
        let target = e.browser_event.target;

        if !is_sticky_scroll_container(&tree.ui, target) && !is_sticky_scroll_element(&tree.ui, target) {
            list.default_mouse_down(e);
        }
    }
}

/// `distinct` de índices, mantendo a primeira ocorrência.
fn distinct(indexes: Vec<usize>) -> Vec<usize> {
    let mut seen = HashSet::new();
    indexes.into_iter().filter(|i| seen.insert(*i)).collect()
}

/// `AbstractTreePart`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AbstractTreePart {
    Tree,
    StickyScroll,
}

type ViewReader<M> = fn(&List<Rc<<M as ITreeModel>::Node>>) -> Option<Rc<<M as ITreeModel>::Node>>;
type WeakList<M> = Weak<List<Rc<<M as ITreeModel>::Node>>>;
type NodeFilter<M> = Rc<dyn Fn(&Rc<<M as ITreeModel>::Node>) -> bool>;

/// `AbstractTree<T, TFilterData, TRef>`.
pub struct AbstractTree<M: ITreeModel>
where
    M::Element: Eq + Hash,
{
    weak_self: Weak<AbstractTree<M>>,
    pub(crate) ui: Ui,
    user: String,
    pub(crate) model: Rc<M>,
    pub(crate) view: Rc<List<Rc<M::Node>>>,
    pub(crate) renderers: Vec<Rc<TreeRenderer<M>>>,
    pub(crate) tree_delegate: Rc<ComposedTreeDelegate<M>>,
    pub(crate) focus: Rc<TreeTrait<M>>,
    pub(crate) selection: Rc<TreeTrait<M>>,
    pub(crate) anchor: Rc<TreeTrait<M>>,
    event_bufferer: Rc<EventBufferer>,
    options: RefCell<AbstractTreeOptions<M>>,
    additional_focus_navigation_filter: RefCell<Option<NodeFilter<M>>>,
    sticky_scroll_controller: RefCell<Option<Rc<StickyScrollController<M>>>>,
    find_handle: RefCell<Option<Rc<dyn FindHandle<M>>>>,
    on_did_change_focus: BaseEvent<TreeEvent<M::Element>>,
    on_did_change_selection: BaseEvent<TreeEvent<M::Element>>,
    on_did_change_sticky_scroll_dom_node: Rc<Emitter<Option<NodeId>>>,
    on_did_change_collapse_state: Rc<Emitter<CollapseStateChangeEvent<M::Node>>>,
    on_did_change_render_node_count: Rc<Emitter<Rc<M::Node>>>,
    on_did_change_model: Rc<Emitter<()>>,
    on_did_change_active_nodes: Rc<Emitter<Vec<Rc<M::Node>>>>,
    on_will_refilter: Rc<Emitter<()>>,
    on_did_update_options: Rc<Emitter<()>>,
    active_nodes_debounce: Delayer<()>,
    disposables: Rc<DisposableStore>,
    model_disposables: Rc<DisposableStore>,
    disposed: Cell<bool>,
}

impl<M: ITreeModel> AbstractTree<M>
where
    M::Element: Eq + Hash,
{
    /// O construtor. `create_model` é o `createModel(user, options)` do original: recebe as opções (depois
    /// de qualquer troca de filtro) e devolve o modelo.
    pub fn new(
        user: &str,
        ui: &Ui,
        container: NodeId,
        delegate: Rc<dyn IListVirtualDelegate<M::Element>>,
        renderers: Vec<Rc<dyn ITreeRenderer<M>>>,
        options: AbstractTreeOptions<M>,
        create_model: impl FnOnce(&AbstractTreeOptions<M>) -> Rc<M>,
    ) -> Rc<Self> {
        let model = create_model(&options);
        let disposables = DisposableStore::new();

        let tree_delegate = Rc::new(ComposedTreeDelegate::<M>::new(delegate));
        let on_did_change_active_nodes = store_emitter(&disposables);
        let on_did_change_collapse_state = store_emitter(&disposables);
        let active_nodes_collection = EventCollection::new(on_did_change_active_nodes.event(), Vec::new());
        disposables.add_rc(active_nodes_collection.clone());
        let active_nodes: Rc<dyn Collection<M::Node>> = active_nodes_collection;
        let rendered_indent_guides: RenderedIndentGuides<M::Node> = Rc::new(RefCell::new(SetMap::new()));

        let renderer_options = options.update.renderer_options();
        let tree_renderers: Vec<Rc<TreeRenderer<M>>> = renderers
            .into_iter()
            .map(|r| TreeRenderer::new(ui, r, model.clone(), on_did_change_collapse_state.event(), active_nodes.clone(), rendered_indent_guides.clone(), &renderer_options))
            .collect();
        for renderer in &tree_renderers {
            disposables.add_rc(renderer.clone());
        }

        // O `view` ainda não existe quando as traits são criadas: elas o leem pela referência fraca.
        let view_cell: Rc<RefCell<Option<WeakList<M>>>> = Rc::default();
        let first_with = |f: ViewReader<M>| {
            let view_cell = view_cell.clone();
            move || view_cell.borrow().as_ref().and_then(Weak::upgrade).and_then(|v| f(&v))
        };
        let focus = TreeTrait::<M>::new(first_with(|v| v.get_focused_elements().into_iter().next()), options.identity_provider.clone());
        let selection = TreeTrait::<M>::new(first_with(|v| v.get_selected_elements().into_iter().next()), options.identity_provider.clone());
        let anchor = TreeTrait::<M>::new(first_with(|v| v.get_anchor_element()), options.identity_provider.clone());
        let event_bufferer = EventBufferer::new();

        let this = Rc::new_cyclic(|weak: &Weak<AbstractTree<M>>| {
            let list_options = ListOptions {
                view: options.view.clone(),
                identity_provider: options.identity_provider.clone().map(|provider| Rc::new(NodeIdentityProvider::<M> { provider }) as Rc<dyn IIdentityProvider<Rc<M::Node>>>),
                multiple_selection_support: options.update.multiple_selection_support,
                keyboard_support: Some(false),
                overrides: Some(Rc::new(TreeListOverrides::<M> { tree: weak.clone() })),
            };

            let list_renderers: Vec<Rc<dyn IListRenderer<Rc<M::Node>>>> = tree_renderers.iter().map(|r| r.clone() as Rc<dyn IListRenderer<Rc<M::Node>>>).collect();
            let view = List::new(user, ui, container, tree_delegate.clone(), list_renderers, list_options);
            *view_cell.borrow_mut() = Some(Rc::downgrade(&view));

            Self {
                weak_self: weak.clone(),
                ui: ui.clone(),
                user: user.to_owned(),
                model,
                view,
                renderers: tree_renderers,
                tree_delegate,
                on_did_change_focus: event_bufferer.wrap_event(&focus.on_did_change()),
                on_did_change_selection: event_bufferer.wrap_event(&selection.on_did_change()),
                focus,
                selection,
                anchor,
                event_bufferer,
                options: RefCell::new(options),
                additional_focus_navigation_filter: RefCell::new(None),
                sticky_scroll_controller: RefCell::new(None),
                find_handle: RefCell::new(None),
                on_did_change_sticky_scroll_dom_node: store_emitter(&disposables),
                on_did_change_collapse_state,
                on_did_change_render_node_count: store_emitter(&disposables),
                on_did_change_model: store_emitter(&disposables),
                on_did_change_active_nodes,
                on_will_refilter: store_emitter(&disposables),
                on_did_update_options: store_emitter(&disposables),
                active_nodes_debounce: Delayer::new(Delay::Millis(0)),
                disposables: disposables.clone(),
                model_disposables: DisposableStore::new(),
                disposed: Cell::new(false),
            }
        });

        this.setup_model();
        this.install_keyboard_support();

        if this.options.borrow().update.enable_sticky_scroll == Some(true) {
            *this.sticky_scroll_controller.borrow_mut() = Some(StickyScrollController::new(&this));
        }

        this.ui.tree_mut().toggle_class(this.view.get_html_element(), "always", this.options.borrow().update.render_indent_guides == Some(RenderIndentGuides::Always));

        this
    }

    // -------------------- eventos --------------------

    pub fn on_did_change_focus(&self) -> BaseEvent<TreeEvent<M::Element>> {
        self.on_did_change_focus.clone()
    }

    pub fn on_did_change_selection(&self) -> BaseEvent<TreeEvent<M::Element>> {
        self.on_did_change_selection.clone()
    }

    fn map_mouse_event(&self, event: BaseEvent<ListMouseEvent<Rc<M::Node>>>) -> BaseEvent<TreeMouseEvent<M::Element>> {
        let ui = self.ui.clone();
        event.map(move |e| as_tree_mouse_event::<M>(&ui, e), None)
    }

    pub fn on_mouse_click(&self) -> BaseEvent<TreeMouseEvent<M::Element>> {
        self.map_mouse_event(self.view.view.on_mouse_click())
    }

    pub fn on_mouse_dbl_click(&self) -> BaseEvent<TreeMouseEvent<M::Element>> {
        self.map_mouse_event(self.view.view.on_mouse_dbl_click()).filter(|e| e.target != TreeMouseEventTarget::Filter, None)
    }

    pub fn on_pointer(&self) -> BaseEvent<TreeMouseEvent<M::Element>> {
        self.map_mouse_event(self.view.on_pointer())
    }

    pub fn on_did_scroll(&self) -> BaseEvent<je_base::scrollable::ScrollEvent> {
        self.view.view.on_did_scroll()
    }

    pub fn on_did_change_content_height(&self) -> BaseEvent<f64> {
        self.view.view.on_did_change_content_height()
    }

    pub fn on_did_dispose(&self) -> BaseEvent<()> {
        self.view.on_did_dispose()
    }

    pub fn on_did_change_model(&self) -> BaseEvent<()> {
        self.on_did_change_model.event()
    }

    pub fn on_did_change_collapse_state(&self) -> BaseEvent<CollapseStateChangeEvent<M::Node>> {
        self.on_did_change_collapse_state.event()
    }

    pub fn on_did_change_render_node_count(&self) -> BaseEvent<Rc<M::Node>> {
        self.on_did_change_render_node_count.event()
    }

    pub fn on_will_refilter(&self) -> BaseEvent<()> {
        self.on_will_refilter.event()
    }

    pub fn on_did_update_options(&self) -> BaseEvent<()> {
        self.on_did_update_options.event()
    }

    pub fn on_did_change_sticky_scroll_dom_node(&self) -> BaseEvent<Option<NodeId>> {
        self.on_did_change_sticky_scroll_dom_node.event()
    }

    pub fn on_did_change_sticky_scroll_focused(&self) -> BaseEvent<bool> {
        self.sticky_scroll_controller.borrow().as_ref().map_or_else(BaseEvent::none, |c| c.on_did_change_has_focus())
    }

    // -------------------- opções --------------------

    pub fn expand_on_double_click(&self) -> bool {
        self.options.borrow().update.expand_on_double_click.unwrap_or(true)
    }

    pub fn expand_only_on_twistie_click(&self) -> ExpandOnlyOnTwistieClick<M::Element> {
        self.options.borrow().update.expand_only_on_twistie_click.clone().unwrap_or(ExpandOnlyOnTwistieClick::Always(true))
    }

    /// `get options()`: um instantâneo.
    pub fn options(&self) -> AbstractTreeOptions<M> {
        self.options.borrow().clone()
    }

    /// `style(styles)`: repassa os estilos à lista (cores das linhas) e ao renderer (cor do twistie), e
    /// renderiza de novo as linhas para o twistie tomar a cor.
    pub fn style(&self, styles: &IListStyles) {
        for renderer in &self.renderers {
            renderer.style(styles);
        }
        self.view.style(styles);
        self.view.rerender();
    }

    pub fn update_options(&self, update: &AbstractTreeOptionsUpdate<M::Element>) {
        self.options.borrow_mut().update.merge(update);

        for renderer in &self.renderers {
            renderer.update_options(&update.renderer_options());
        }

        self.view.view.update_options(&update.view_options_update());
        if let Some(find) = self.find_handle() {
            find.update_options(update);
        }

        self.update_sticky_scroll(update);
        self.on_did_update_options.fire(());

        let always = self.options.borrow().update.render_indent_guides == Some(RenderIndentGuides::Always);
        self.ui.tree_mut().toggle_class(self.get_html_element(), "always", always);
    }

    fn update_sticky_scroll(&self, update: &AbstractTreeOptionsUpdate<M::Element>) {
        let enabled = self.options.borrow().update.enable_sticky_scroll == Some(true);
        let has_controller = self.sticky_scroll_controller.borrow().is_some();

        if !has_controller && enabled {
            let this = self.weak_self.upgrade().expect("a árvore foi descartada");
            let controller = StickyScrollController::new(&this);
            let dom_node = controller.dom_node();
            *self.sticky_scroll_controller.borrow_mut() = Some(controller);
            self.on_did_change_sticky_scroll_dom_node.fire(Some(dom_node));
        } else if has_controller && !enabled {
            let controller = self.sticky_scroll_controller.borrow_mut().take();

            if let Some(controller) = controller {
                controller.dispose();
            }

            self.on_did_change_sticky_scroll_dom_node.fire(None);
        }

        let controller = self.sticky_scroll_controller.borrow().clone();

        if let Some(controller) = controller {
            controller.update_options(update);
        }
    }

    pub fn update_width(&self, element: &M::Ref) {
        // TODO(integração): `List.updateWidth(index)` (rolagem horizontal) ainda não existe na lista.
        let _ = self.model.get_list_index(element);
    }

    // -------------------- widget --------------------

    pub fn get_html_element(&self) -> NodeId {
        self.view.get_html_element()
    }

    pub fn content_height(&self) -> f64 {
        self.view.content_height()
    }

    pub fn content_width(&self) -> f64 {
        self.view.view.content_width()
    }

    pub fn scroll_top(&self) -> f64 {
        self.view.scroll_top()
    }

    pub fn set_scroll_top(&self, scroll_top: f64) {
        self.view.set_scroll_top(scroll_top);
    }

    pub fn scroll_left(&self) -> f64 {
        self.view.view.get_scroll_left()
    }

    pub fn set_scroll_left(&self, scroll_left: f64) {
        self.view.view.set_scroll_left(scroll_left);
    }

    pub fn scroll_height(&self) -> f64 {
        self.view.scroll_height()
    }

    pub fn render_height(&self) -> f64 {
        self.view.render_height()
    }

    pub fn refresh_sticky_scroll(&self) {
        let controller = self.sticky_scroll_controller.borrow().clone();

        if let Some(controller) = controller {
            controller.refresh();
        }
    }

    pub fn rerender_sticky_scroll(&self) {
        let controller = self.sticky_scroll_controller.borrow().clone();

        if let Some(controller) = controller {
            controller.rerender();
        }
    }

    pub fn sticky_scroll_dom_node(&self) -> Option<NodeId> {
        self.sticky_scroll_controller.borrow().as_ref().map(|c| c.dom_node())
    }

    pub(crate) fn sticky_scroll_controller(&self) -> Option<Rc<StickyScrollController<M>>> {
        self.sticky_scroll_controller.borrow().clone()
    }

    pub fn first_visible_element(&self) -> Option<M::Element> {
        let mut index = self.view.first_visible_index();

        if let Some(controller) = self.sticky_scroll_controller() {
            index += controller.count() as i64;
        }

        if index < 0 || index >= self.view.length() as i64 {
            return None;
        }

        Some(self.view.element(index as usize).element())
    }

    pub fn last_visible_element(&self) -> M::Element {
        let index = self.view.last_visible_index();
        self.view.element(index as usize).element()
    }

    pub fn selection_size(&self) -> usize {
        self.selection.get_nodes().len()
    }

    pub fn dom_focus(&self) {
        let controller = self.sticky_scroll_controller();

        match controller {
            Some(controller) if controller.focused_last() => controller.dom_focus(),
            _ => self.view.dom_focus(),
        }
    }

    pub fn is_dom_focused(&self) -> bool {
        self.view.is_dom_focused()
    }

    pub fn layout(&self, height: Option<f64>, width: Option<f64>) {
        self.view.layout(height, width);
    }

    // -------------------- navegação na árvore --------------------

    pub fn get_parent_element(&self, location: &M::Ref) -> M::Element {
        let parent_ref = self.model.get_parent_node_location(location).expect("o nó não tem pai");
        self.model.get_node(&parent_ref).element()
    }

    pub fn get_first_element_child(&self, location: &M::Ref) -> Option<M::Element> {
        self.model.get_first_element_child(location)
    }

    pub fn get_node(&self, location: &M::Ref) -> Rc<M::Node> {
        self.model.get_node(location)
    }

    pub fn get_root_node(&self) -> Rc<M::Node> {
        self.model.get_node(&self.model.root_ref())
    }

    pub fn get_node_location(&self, node: &Rc<M::Node>) -> M::Ref {
        self.model.get_node_location(node)
    }

    pub fn collapse(&self, location: &M::Ref, recursive: bool) -> bool {
        self.model.set_collapsed(location, Some(true), Some(recursive))
    }

    pub fn expand(&self, location: &M::Ref, recursive: bool) -> bool {
        self.model.set_collapsed(location, Some(false), Some(recursive))
    }

    pub fn toggle_collapsed(&self, location: &M::Ref, recursive: bool) -> bool {
        self.model.set_collapsed(location, None, Some(recursive))
    }

    pub fn expand_all(&self) {
        self.model.set_collapsed(&self.model.root_ref(), Some(false), Some(true));
    }

    pub fn collapse_all(&self) {
        self.model.set_collapsed(&self.model.root_ref(), Some(true), Some(true));
    }

    pub fn is_collapsible(&self, location: &M::Ref) -> bool {
        self.model.is_collapsible(location)
    }

    pub fn set_collapsible(&self, location: &M::Ref, collapsible: Option<bool>) -> bool {
        self.model.set_collapsible(location, collapsible)
    }

    pub fn is_collapsed(&self, location: &M::Ref) -> bool {
        self.model.is_collapsed(location)
    }

    pub fn expand_to(&self, location: &M::Ref) {
        self.model.expand_to(location);
    }

    /// O controlador de busca, quando há (ver [`crate::tree::find::attach_find_controller`]).
    pub fn find_handle(&self) -> Option<Rc<dyn FindHandle<M>>> {
        self.find_handle.borrow().clone()
    }

    pub(crate) fn set_find_handle(&self, handle: Option<Rc<dyn FindHandle<M>>>) {
        *self.find_handle.borrow_mut() = handle;
    }

    /// `openFind`.
    pub fn open_find(&self) {
        if let Some(find) = self.find_handle() {
            find.open();
        }
    }

    /// `closeFind`.
    pub fn close_find(&self) {
        if let Some(find) = self.find_handle() {
            find.close();
        }
    }

    /// `findMode`.
    pub fn find_mode(&self) -> TreeFindMode {
        self.find_handle().map_or(TreeFindMode::Highlight, |f| f.mode())
    }

    /// `findMatchType`.
    pub fn find_match_type(&self) -> TreeFindMatchType {
        self.find_handle().map_or(TreeFindMatchType::Fuzzy, |f| f.match_type())
    }

    pub fn refilter(&self) {
        self.on_will_refilter.fire(());
        self.model.refilter();
    }

    pub fn set_anchor(&self, element: Option<&M::Ref>) {
        let Some(element) = element else { return self.view.set_anchor_base(None) };

        self.event_bufferer.buffer_events(|| {
            let node = self.model.get_node(element);
            self.anchor.set(vec![node], None);

            let index = self.model.get_list_index(element);

            if index > -1 {
                self.view.set_anchor_base(Some(index as usize));
            }
        });
    }

    pub fn get_anchor(&self) -> Option<M::Element> {
        self.anchor.get().into_iter().next()
    }

    pub fn set_selection(&self, elements: &[M::Ref], browser_event: Option<BrowserEvent>) {
        self.event_bufferer.buffer_events(|| {
            let nodes = elements.iter().map(|e| self.model.get_node(e)).collect();
            self.selection.set(nodes, browser_event);

            let indexes = elements.iter().map(|e| self.model.get_list_index(e)).filter(|i| *i > -1).map(|i| i as usize).collect();
            self.view.set_selection_base(indexes, browser_event);
        });
    }

    pub fn get_selection(&self) -> Vec<M::Element> {
        self.selection.get()
    }

    pub fn set_focus(&self, elements: &[M::Ref], browser_event: Option<BrowserEvent>) {
        self.event_bufferer.buffer_events(|| {
            let nodes = elements.iter().map(|e| self.model.get_node(e)).collect();
            self.focus.set(nodes, browser_event);

            let indexes = elements.iter().map(|e| self.model.get_list_index(e)).filter(|i| *i > -1).map(|i| i as usize).collect();
            self.view.set_focus_base(indexes, browser_event);
        });
    }

    /// O filtro de navegação padrão (`focusNavigationFilter`): o do controlador de busca, que só deixa
    /// o foco parar nos nós que casam com o padrão.
    fn default_focus_navigation_filter(&self) -> Option<NodeFilter<M>> {
        let find = self.find_handle()?;
        Some(Rc::new(move |node: &Rc<M::Node>| find.should_allow_focus(node)))
    }

    fn get_focus_navigation_filter(&self, filter: Option<NodeFilter<M>>) -> Option<ElementFilter<Rc<M::Node>>> {
        let additional = self.additional_focus_navigation_filter.borrow().clone();

        match additional {
            None => filter,
            Some(additional) => Some(Rc::new(move |node: &Rc<M::Node>| filter.as_ref().is_none_or(|f| f(node)) && additional(node))),
        }
    }

    fn resolve_filter(&self, browser_event: Option<BrowserEvent>, filter: Option<NodeFilter<M>>) -> Option<ElementFilter<Rc<M::Node>>> {
        let filter = filter.or_else(|| if browser_event.is_some_and(|e| e.is_keyboard && e.modifiers.alt) { None } else { self.default_focus_navigation_filter() });
        self.get_focus_navigation_filter(filter)
    }

    pub fn focus_next(&self, n: usize, do_loop: bool, browser_event: Option<BrowserEvent>, filter: Option<NodeFilter<M>>) {
        let filter = self.resolve_filter(browser_event, filter);
        self.view.focus_next(n, do_loop, browser_event, filter.as_deref());
    }

    pub fn focus_previous(&self, n: usize, do_loop: bool, browser_event: Option<BrowserEvent>, filter: Option<NodeFilter<M>>) {
        let filter = self.resolve_filter(browser_event, filter);
        self.view.focus_previous(n, do_loop, browser_event, filter.as_deref());
    }

    pub fn focus_next_page(&self, browser_event: Option<BrowserEvent>, filter: Option<NodeFilter<M>>) {
        let filter = self.resolve_filter(browser_event, filter);
        self.view.focus_next_page(browser_event, filter);
    }

    pub fn focus_previous_page(&self, browser_event: Option<BrowserEvent>, filter: Option<NodeFilter<M>>) {
        let filter = self.resolve_filter(browser_event, filter);
        let weak = self.weak_self.clone();
        self.view.focus_previous_page(browser_event, filter, Rc::new(move || weak.upgrade().and_then(|t| t.sticky_scroll_controller()).map_or(0.0, |c| c.height())));
    }

    pub fn focus_last(&self, browser_event: Option<BrowserEvent>, filter: Option<NodeFilter<M>>) {
        let filter = self.resolve_filter(browser_event, filter);
        self.view.focus_last(browser_event, filter.as_deref());
    }

    pub fn focus_first(&self, browser_event: Option<BrowserEvent>, filter: Option<NodeFilter<M>>) {
        let filter = self.resolve_filter(browser_event, filter);
        self.view.focus_first(browser_event, filter.as_deref());
    }

    pub fn set_focus_navigation_filter(&self, filter: Option<NodeFilter<M>>) {
        *self.additional_focus_navigation_filter.borrow_mut() = filter;
    }

    pub fn get_focus(&self) -> Vec<M::Element> {
        self.focus.get()
    }

    pub fn get_sticky_scroll_focus(&self) -> Vec<M::Element> {
        self.sticky_scroll_controller().and_then(|c| c.get_focus()).into_iter().collect()
    }

    pub fn get_focused_part(&self) -> AbstractTreePart {
        if self.sticky_scroll_controller().is_some_and(|c| c.focused_last()) { AbstractTreePart::StickyScroll } else { AbstractTreePart::Tree }
    }

    pub fn reveal(&self, location: &M::Ref, relative_top: Option<f64>) {
        self.model.expand_to(location);

        let index = self.model.get_list_index(location);

        if index == -1 {
            return;
        }

        match self.sticky_scroll_controller() {
            None => self.view.reveal(index as usize, relative_top, 0.0),
            Some(controller) => {
                let padding_top = controller.node_position_top_below_widget(&self.get_node(location));
                self.view.reveal(index as usize, relative_top, padding_top);
            }
        }
    }

    /// `getRelativeTop`: `None` quando o elemento não está *inteiro* na janela visível.
    pub fn get_relative_top(&self, location: &M::Ref) -> Option<f64> {
        let index = self.model.get_list_index(location);

        if index == -1 {
            return None;
        }

        let controller = self.sticky_scroll_controller();
        let sticky_scroll_node = controller.as_ref().and_then(|c| c.get_node(&self.get_node(location)));
        let padding_top = sticky_scroll_node.map(|n| n.position).or_else(|| controller.map(|c| c.height())).unwrap_or(0.0);

        self.view.get_relative_top(index as usize, padding_top)
    }

    /// `getElementTop`: o topo absoluto do elemento, ou `None` se ele não está na árvore.
    pub fn get_element_top(&self, location: &M::Ref) -> Option<f64> {
        let index = self.model.get_list_index(location);

        if index == -1 {
            return None;
        }

        Some(self.view.get_element_top(index as usize))
    }

    pub fn get_view_state(&self, identity_provider: Option<Rc<dyn IIdentityProvider<M::Element>>>) -> AbstractTreeViewState {
        let identity_provider = identity_provider.or_else(|| self.options.borrow().identity_provider.clone());

        let Some(identity_provider) = identity_provider else {
            throw(&tree_error(&self.user, "Can't get tree view state without an identity provider").to_string());
        };

        let mut state = AbstractTreeViewState::empty(self.scroll_top());

        for focus in self.get_focus() {
            let id = identity_provider.get_id(&focus);

            if !state.focus.contains(&id) {
                state.focus.push(id);
            }
        }

        for selection in self.get_selection() {
            let id = identity_provider.get_id(&selection);

            if !state.selection.contains(&id) {
                state.selection.push(id);
            }
        }

        let root = self.get_root_node();
        let mut stack = vec![root.clone()];

        while let Some(node) = stack.pop() {
            if !Rc::ptr_eq(&node, &root) && node.collapsible() {
                state.expanded.insert(identity_provider.get_id(&node.element()), !node.collapsed());
            }

            stack.extend(node.children());
        }

        state
    }

    // -------------------- teclado --------------------

    fn install_keyboard_support(&self) {
        if self.options.borrow().keyboard_support == Some(false) {
            return;
        }

        let weak = self.weak_self.clone();
        self.disposables.add(self.ui.listen(self.view.get_html_element(), EventKind::KeyDown, false, move |e| {
            let (Some(tree), Some(KeyData { key_code, modifiers })) = (weak.upgrade(), e.key) else { return };

            match key_code {
                KeyCode::LeftArrow => {
                    e.stop(true);
                    tree.on_left_arrow();
                }
                KeyCode::RightArrow => {
                    e.stop(true);
                    tree.on_right_arrow();
                }
                KeyCode::Space => {
                    e.stop(true);
                    tree.on_space(modifiers.alt);
                }
                _ => {}
            }
        }));

        // O `List` do `je-widgets` instala o teclado (setas, Enter, Esc) sozinho, mas o `ObjectTree` do
        // original passa `keyboardSupport` adiante; aqui o teclado da lista fica ligado como no original.
        // TODO(integração): a lista desliga o próprio teclado quando `keyboardSupport` é falso.
    }

    fn on_left_arrow(&self) {
        let nodes = self.view.get_focused_elements();

        let Some(node) = nodes.first() else { return };
        let location = self.model.get_node_location(node);
        let did_change = self.model.set_collapsed(&location, Some(true), None);

        if !did_change {
            let Some(parent_location) = self.model.get_parent_node_location(&location) else { return };

            if self.model.ref_is_falsy(&parent_location) {
                return;
            }

            let parent_list_index = self.model.get_list_index(&parent_location);
            self.view.reveal(parent_list_index as usize, None, 0.0);
            self.view.set_focus(vec![parent_list_index as usize], None);
        }
    }

    fn on_right_arrow(&self) {
        let nodes = self.view.get_focused_elements();

        let Some(node) = nodes.first() else { return };
        let location = self.model.get_node_location(node);
        let did_change = self.model.set_collapsed(&location, Some(false), None);

        if !did_change {
            if !node.children().iter().any(|child| child.visible()) {
                return;
            }

            let Some(focused_index) = self.view.get_focus().first().copied() else { return };
            let first_child_index = focused_index + 1;
            self.view.reveal(first_child_index, None, 0.0);
            self.view.set_focus(vec![first_child_index], None);
        }
    }

    fn on_space(&self, alt: bool) {
        let nodes = self.view.get_focused_elements();

        let Some(node) = nodes.first() else { return };
        let location = self.model.get_node_location(node);
        self.model.set_collapsed(&location, None, Some(alt));
    }

    // -------------------- modelo --------------------

    /// `setupModel`.
    fn setup_model(&self) {
        let model = self.model.clone();

        let weak = self.weak_self.clone();
        self.model_disposables.add(model.on_did_splice_rendered_nodes().listen(move |e| {
            if let Some(tree) = weak.upgrade() {
                tree.view.splice(e.start, e.delete_count, &e.elements);
            }
        }));

        let weak = self.weak_self.clone();
        self.model_disposables.add(model.on_did_splice_model().listen(move |e| {
            let Some(tree) = weak.upgrade() else { return };

            tree.event_bufferer.buffer_events(|| {
                tree.focus.on_did_model_splice(e);
                tree.selection.on_did_model_splice(e);
            });

            tree.on_did_change_model.fire(());
            tree.schedule_active_nodes();
        }));

        for event in [self.focus.on_did_change(), self.selection.on_did_change()] {
            let weak = self.weak_self.clone();
            self.model_disposables.add(event.listen(move |_| {
                if let Some(tree) = weak.upgrade() {
                    tree.schedule_active_nodes();
                }
            }));
        }

        let weak = self.weak_self.clone();
        self.model_disposables.add(model.on_did_change_collapse_state().listen(move |e| {
            if let Some(tree) = weak.upgrade() {
                tree.on_did_change_collapse_state.fire(e.clone());
            }
        }));

        let weak = self.weak_self.clone();
        self.model_disposables.add(model.on_did_change_render_node_count().listen(move |e| {
            if let Some(tree) = weak.upgrade() {
                tree.on_did_change_render_node_count.fire(e.clone());
            }
        }));
    }

    /// As folhas ativas (foco e seleção) mudam quando o modelo, o foco ou a seleção mudam. O `Delayer(0)`
    /// do original junta várias mudanças na mesma pilha numa só.
    fn schedule_active_nodes(&self) {
        let weak = self.weak_self.clone();

        self.active_nodes_debounce.trigger(
            move || {
                if let Some(tree) = weak.upgrade() {
                    let mut seen: HashSet<ById<M::Node>> = HashSet::new();
                    let mut nodes: Vec<Rc<M::Node>> = Vec::new();

                    for node in tree.focus.get_nodes().into_iter().chain(tree.selection.get_nodes()) {
                        if seen.insert(ById(node.clone())) {
                            nodes.push(node);
                        }
                    }

                    tree.on_did_change_active_nodes.fire(nodes);
                }

                MaybePromise::Value(())
            },
            None,
        );
    }

    pub fn navigate(&self, start: Option<&M::Ref>) -> TreeNavigator<M> {
        TreeNavigator::new(self.view.clone(), self.model.clone(), start)
    }

    /// `StickyScrollController` chama isto quando o usuário clica num nó fixado: rola o nó para baixo do
    /// widget e o foca/seleciona (`TreeNodeListMouseController.handleStickyScrollMouseEvent`).
    fn handle_sticky_scroll_mouse_event(&self, list: &List<Rc<M::Node>>, node: &Rc<M::Node>) {
        let Some(controller) = self.sticky_scroll_controller() else {
            throw("Sticky scroll controller not found");
        };

        let Some(node_index) = list.view.index_of(|n| Rc::ptr_eq(n, node)) else { return };
        let element_scroll_top = list.get_element_top(node_index);
        let element_target_view_top = controller.node_position_top_below_widget(node);
        self.set_scroll_top(element_scroll_top - element_target_view_top);
        list.dom_focus();
        list.set_focus(vec![node_index], None);
        list.set_selection(vec![node_index], None);
    }

    pub fn dispose(&self) {
        if self.disposed.replace(true) {
            return;
        }

        self.active_nodes_debounce.dispose();
        self.disposables.dispose();

        if let Some(controller) = self.sticky_scroll_controller.borrow_mut().take() {
            controller.dispose();
        }

        self.view.dispose();
        self.model_disposables.dispose();
        self.focus.dispose();
        self.selection.dispose();
        self.anchor.dispose();
    }

    /// O `DisposableRef` que descarta a árvore (para registrar em um `DisposableStore`).
    pub fn as_disposable(self: &Rc<Self>) -> DisposableRef {
        self.clone()
    }
}

impl<M: ITreeModel> IDisposable for AbstractTree<M>
where
    M::Element: Eq + Hash,
{
    fn dispose(&self) {
        AbstractTree::dispose(self);
    }

    fn constructor_name(&self) -> &'static str {
        "AbstractTree"
    }
}

/// `TreeNavigator<T, TFilterData, TRef>`.
pub struct TreeNavigator<M: ITreeModel> {
    view: Rc<List<Rc<M::Node>>>,
    index: i64,
}

impl<M: ITreeModel> TreeNavigator<M> {
    pub fn new(view: Rc<List<Rc<M::Node>>>, model: Rc<M>, start: Option<&M::Ref>) -> Self {
        let index = start.map_or(-1, |s| model.get_list_index(s));
        Self { view, index }
    }
}

impl<M: ITreeModel> ITreeNavigator<M::Element> for TreeNavigator<M> {
    fn current(&mut self) -> Option<M::Element> {
        if self.index < 0 || self.index >= self.view.length() as i64 {
            return None;
        }

        Some(self.view.element(self.index as usize).element())
    }

    fn previous(&mut self) -> Option<M::Element> {
        self.index -= 1;
        self.current()
    }

    fn next(&mut self) -> Option<M::Element> {
        self.index += 1;
        self.current()
    }

    fn first(&mut self) -> Option<M::Element> {
        self.index = 0;
        self.current()
    }

    fn last(&mut self) -> Option<M::Element> {
        self.index = self.view.length() as i64 - 1;
        self.current()
    }
}
