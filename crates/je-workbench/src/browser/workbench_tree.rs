//! Porte das árvores do workbench de `src/vs/platform/list/browser/listService.ts`: `WorkbenchObjectTree`,
//! `WorkbenchAsyncDataTree` e `WorkbenchCompressibleAsyncDataTree`, com o que elas compartilham (o `workbenchTreeDataPreamble`,
//! o `WorkbenchTreeInternals`, o registro no `ListService`, as chaves de contexto de foco e seleção, o `onDidOpen` do
//! `TreeResourceNavigator` e a configuração `workbench.list.*` e `workbench.tree.*`).
//!
//! O contrato do serviço, o [`ListService`](je_platform::list::list_service::ListService) e as chaves de contexto (`listFocus`,
//! `listHasSelectionOrFocus`...) ficam em `je-platform/src/list/list_service.rs`, que não depende de widgets. Estas árvores
//! precisam de `je-widgets` (a `AsyncDataTree`, a `ObjectTree`) e do `ResourceNavigator`, que já mora em `je-workbench`
//! ([`crate::browser::resource_navigator`]); por isso o código que toca widgets fica aqui, ao lado dele.
//!
//! Divergências:
//! - **Herança.** Cada `WorkbenchXTree extends XTree` do original é um [`WorkbenchTree`] sobre o widget (`Deref` para ele), com os
//!   três tipos como alias. O `internals` e o `navigator` são campos do envoltório. O `updateOptions` sobrescrito é
//!   [`WorkbenchTree::update_options`].
//! - **Injeção.** Os cinco serviços do `@IInstantiationService`, `@IContextKeyService`, `@IListService`, `@IConfigurationService` e o
//!   `defaultListStyles` são os campos de [`WorkbenchTreeServices`]. O `IListService` é o [`ListService`] concreto, porque o original
//!   o converte (`listService as ListService`) para chamar `register`. O `defaultListStyles` é um fornecedor, porque o tema é um
//!   serviço.
//! - **`overrideStyles`.** O `IStyleOverride<IListStyles>` do original (nomes de cor) já chega resolvido como [`IListStyles`] (quem
//!   conhece o tema resolve); `tree.style(overrideStyles ? getListStyles(overrideStyles) : defaultListStyles)` fica igual.
//! - **O que o widget ainda não tem.** O `typeNavigationMode`, o `horizontalScrolling`, o `keyboardNavigationDelegate`, o
//!   `keyboardNavigationEventFilter`, o `multipleSelectionController`, o `scrollToActiveElement`, o `contextViewProvider` e o
//!   `findWidgetStyles` não existem nas opções das árvores do `je-widgets`; o `useAltAsMultipleSelectionModifier` é lido e mantido
//!   atualizado por [`WorkbenchTree::use_alt_as_multiple_selection_modifier`] para quando o controlador existir. A árvore abstrata
//!   não emite `onDidChangeFindOpenState`, então a chave `treeFindOpen` é criada (com o padrão `false`) mas ainda não é guiada.
//! - **Serviço de chaves escopado.** O `createScoped(widget.getHTMLElement())` usa o [`ContextKeyTarget`] avulso do porte (como o
//!   editor), porque o alvo do serviço não é o nó do `je-ui`.

use std::any::Any;
use std::cell::Cell;
use std::hash::Hash;
use std::ops::Deref;
use std::rc::Rc;

use je_base::dynamic::{Dynamic, Truthy};
use je_base::event::Event;
use je_base::lifecycle::{Disposable, DisposableRef, IDisposable};
use je_base::nls::localize;
use je_platform::configuration::registry::{ConfigurationNode, PropertySchema, configuration_registry};
use je_platform::configuration::service::IConfigurationService;
use je_platform::contextkey::contextkey::{IContextKey, IContextKeyService, IScopedContextKeyService};
use je_platform::list::list_service::{
    IListWidget, ListService, raw_workbench_list_focus_context_key, raw_workbench_list_scroll_at_boundary_context_key, workbench_list_double_selection, workbench_list_has_selection_or_focus,
    workbench_list_multi_selection, workbench_list_selection_navigation, workbench_list_supports_find, workbench_list_supports_multi_select_context_key, workbench_tree_element_can_collapse,
    workbench_tree_element_can_expand, workbench_tree_element_has_child, workbench_tree_element_has_parent, workbench_tree_find_open, workbench_tree_sticky_scroll_focused,
};
use je_ui::{EventKind, NodeId, Ui};
use je_widgets::list::list_styles::IListStyles;
use je_widgets::list::list_types::IListVirtualDelegate;
use je_widgets::tree::abstract_tree::{AbstractTree, AbstractTreeOptionsUpdate, ExpandOnlyOnTwistieClick, TreeFindMatchType, TreeFindMode};
use je_widgets::tree::async_data_tree::{AsyncDataTree, AsyncDataTreeBackend, ObjectBackend};
use je_widgets::tree::async_data_tree_node::{AsyncDataTreeOptions, AsyncNode, IAsyncDataTreeRenderer};
use je_widgets::tree::compressible_async_data_tree::{CompressibleAsyncDataTreeOptions, CompressibleBackend, ICompressibleAsyncDataTreeRenderer, ITreeCompressionDelegate};
use je_widgets::tree::object_tree::{ObjectTree, ObjectTreeOptions};
use je_widgets::tree::object_tree_model::ObjectTreeModel;
use je_widgets::tree::tree_renderer::RenderIndentGuides;
use je_widgets::tree::tree_types::{IAsyncDataSource, ITreeModel, ITreeRenderer, TreeEvent, TreeMouseEvent, TreeNodeApi};

use crate::browser::parts::editor::editor_group_view::ContextKeyTarget;
use crate::browser::resource_navigator::{IOpenEvent, IResourceNavigatorOptions, ResourceNavigator};
use crate::services::editor::editor_resolver_service::workbench_configuration_node_base;

pub const MULTI_SELECT_MODIFIER_SETTING_KEY: &str = "workbench.list.multiSelectModifier";
pub const OPEN_MODE_SETTING_KEY: &str = "workbench.list.openMode";
pub const HORIZONTAL_SCROLLING_KEY: &str = "workbench.list.horizontalScrolling";
pub const DEFAULT_FIND_MODE_SETTING_KEY: &str = "workbench.list.defaultFindMode";
pub const TYPE_NAVIGATION_MODE_SETTING_KEY: &str = "workbench.list.typeNavigationMode";
/// Obsoleta em favor de `workbench.list.defaultFindMode` e `workbench.list.typeNavigationMode`.
pub const KEYBOARD_NAVIGATION_SETTING_KEY: &str = "workbench.list.keyboardNavigation";
pub const SCROLL_BY_PAGE_KEY: &str = "workbench.list.scrollByPage";
pub const DEFAULT_FIND_MATCH_TYPE_SETTING_KEY: &str = "workbench.list.defaultFindMatchType";
pub const TREE_INDENT_KEY: &str = "workbench.tree.indent";
pub const TREE_RENDER_INDENT_GUIDES_KEY: &str = "workbench.tree.renderIndentGuides";
pub const LIST_SMOOTH_SCROLLING: &str = "workbench.list.smoothScrolling";
pub const MOUSE_WHEEL_SCROLL_SENSITIVITY_KEY: &str = "workbench.list.mouseWheelScrollSensitivity";
pub const FAST_SCROLL_SENSITIVITY_KEY: &str = "workbench.list.fastScrollSensitivity";
pub const TREE_EXPAND_MODE: &str = "workbench.tree.expandMode";
pub const TREE_STICKY_SCROLL: &str = "workbench.tree.enableStickyScroll";
pub const TREE_STICKY_SCROLL_MAX_ELEMENTS: &str = "workbench.tree.stickyScrollMaxItemCount";

fn config_number(service: &dyn IConfigurationService, key: &str) -> Option<f64> {
    match service.get_value(Some(key), None) {
        Dynamic::Number(number) => Some(number),
        _ => None,
    }
}

fn config_string(service: &dyn IConfigurationService, key: &str) -> Option<String> {
    match service.get_value(Some(key), None) {
        Dynamic::String(text) => Some(text.to_string_lossy().to_string()),
        _ => None,
    }
}

/// `Boolean(configurationService.getValue(key))`.
fn config_bool(service: &dyn IConfigurationService, key: &str) -> bool {
    service.get_value(Some(key), None).is_truthy()
}

/// `useAltAsMultipleSelectionModifier(configurationService)`.
pub fn use_alt_as_multiple_selection_modifier(service: &dyn IConfigurationService) -> bool {
    config_string(service, MULTI_SELECT_MODIFIER_SETTING_KEY).as_deref() == Some("alt")
}

/// `getDefaultTreeFindMode(configurationService)`.
pub fn get_default_tree_find_mode(service: &dyn IConfigurationService) -> Option<TreeFindMode> {
    match config_string(service, DEFAULT_FIND_MODE_SETTING_KEY).as_deref() {
        Some("highlight") => return Some(TreeFindMode::Highlight),
        Some("filter") => return Some(TreeFindMode::Filter),
        _ => {}
    }

    match config_string(service, KEYBOARD_NAVIGATION_SETTING_KEY).as_deref() {
        Some("simple" | "highlight") => Some(TreeFindMode::Highlight),
        Some("filter") => Some(TreeFindMode::Filter),
        _ => None,
    }
}

/// `getDefaultTreeFindMatchType(configurationService)`.
pub fn get_default_tree_find_match_type(service: &dyn IConfigurationService) -> Option<TreeFindMatchType> {
    match config_string(service, DEFAULT_FIND_MATCH_TYPE_SETTING_KEY).as_deref() {
        Some("fuzzy") => Some(TreeFindMatchType::Fuzzy),
        Some("contiguous") => Some(TreeFindMatchType::Contiguous),
        _ => None,
    }
}

fn config_render_indent_guides(service: &dyn IConfigurationService) -> Option<RenderIndentGuides> {
    match config_string(service, TREE_RENDER_INDENT_GUIDES_KEY).as_deref() {
        Some("none") => Some(RenderIndentGuides::None),
        Some("onHover") => Some(RenderIndentGuides::OnHover),
        Some("always") => Some(RenderIndentGuides::Always),
        _ => None,
    }
}

/// `Number(getValue(...))` para a contagem de elementos do sticky scroll.
fn config_count(service: &dyn IConfigurationService, key: &str) -> Option<usize> {
    config_number(service, key).map(|count| if count.is_nan() { 0 } else { count.max(0.0) as usize })
}

/// Os campos de `IAbstractTreeOptionsUpdate` que a configuração muda depois da criação (o `newOptions` do
/// `onDidChangeConfiguration` do `WorkbenchTreeInternals`), sem o tipo do elemento, para a árvore ser apagada pelo
/// [`WorkbenchTreeHost`].
#[derive(Clone, Default, PartialEq)]
pub struct TreeOptionsDelta {
    pub indent: Option<f64>,
    pub render_indent_guides: Option<RenderIndentGuides>,
    pub smooth_scrolling: Option<bool>,
    pub default_find_mode: Option<TreeFindMode>,
    pub default_find_match_type: Option<TreeFindMatchType>,
    pub scroll_by_page: Option<bool>,
    pub expand_only_on_twistie_click: Option<bool>,
    pub enable_sticky_scroll: Option<bool>,
    pub sticky_scroll_max_item_count: Option<usize>,
    pub mouse_wheel_scroll_sensitivity: Option<f64>,
    pub fast_scroll_sensitivity: Option<f64>,
}

impl TreeOptionsDelta {
    /// `Object.keys(newOptions).length === 0`.
    pub fn is_empty(&self) -> bool {
        *self == Self::default()
    }

    /// A atualização no tipo do elemento da árvore.
    pub fn to_update<E>(&self) -> AbstractTreeOptionsUpdate<E> {
        AbstractTreeOptionsUpdate {
            indent: self.indent,
            render_indent_guides: self.render_indent_guides,
            smooth_scrolling: self.smooth_scrolling,
            default_find_mode: self.default_find_mode,
            default_find_match_type: self.default_find_match_type,
            scroll_by_page: self.scroll_by_page,
            expand_only_on_twistie_click: self.expand_only_on_twistie_click.map(ExpandOnlyOnTwistieClick::Always),
            enable_sticky_scroll: self.enable_sticky_scroll,
            sticky_scroll_max_item_count: self.sticky_scroll_max_item_count,
            mouse_wheel_scroll_sensitivity: self.mouse_wheel_scroll_sensitivity,
            fast_scroll_sensitivity: self.fast_scroll_sensitivity,
            ..AbstractTreeOptionsUpdate::default()
        }
    }
}

/// Quais opções o chamador definiu explicitamente (o `options.x === undefined` do `onDidChangeConfiguration`): as que ele definiu
/// não seguem a configuração.
#[derive(Clone, Copy, Default)]
pub struct TreeOptionOverrides {
    pub render_indent_guides: bool,
    pub expand_only_on_twistie_click: bool,
    pub enable_sticky_scroll: bool,
    pub sticky_scroll_max_item_count: bool,
}

impl TreeOptionOverrides {
    pub fn of<E>(update: &AbstractTreeOptionsUpdate<E>) -> Self {
        Self {
            render_indent_guides: update.render_indent_guides.is_some(),
            expand_only_on_twistie_click: update.expand_only_on_twistie_click.is_some(),
            enable_sticky_scroll: update.enable_sticky_scroll.is_some(),
            sticky_scroll_max_item_count: update.sticky_scroll_max_item_count.is_some(),
        }
    }
}

/// `workbenchTreeDataPreamble` (com o `toWorkbenchListOptions`): as opções da árvore do workbench a partir das do chamador e da
/// configuração. Os campos que o original sobrescreve sem olhar o chamador são sobrescritos aqui também.
pub fn workbench_tree_data_preamble<E>(service: &dyn IConfigurationService, update: &mut AbstractTreeOptionsUpdate<E>, keyboard_support: &mut Option<bool>) {
    *keyboard_support = Some(false);

    // toWorkbenchListOptions
    update.smooth_scrolling = Some(config_bool(service, LIST_SMOOTH_SCROLLING));
    update.mouse_wheel_scroll_sensitivity = config_number(service, MOUSE_WHEEL_SCROLL_SENSITIVITY_KEY);
    update.fast_scroll_sensitivity = config_number(service, FAST_SCROLL_SENSITIVITY_KEY);
    update.scroll_by_page = Some(config_bool(service, SCROLL_BY_PAGE_KEY));

    update.indent = config_number(service, TREE_INDENT_KEY);
    update.render_indent_guides = update.render_indent_guides.or_else(|| config_render_indent_guides(service));
    update.default_find_mode = update.default_find_mode.or_else(|| get_default_tree_find_mode(service));
    update.default_find_match_type = update.default_find_match_type.or_else(|| get_default_tree_find_match_type(service));
    if update.expand_only_on_twistie_click.is_none() {
        update.expand_only_on_twistie_click = Some(ExpandOnlyOnTwistieClick::Always(config_string(service, TREE_EXPAND_MODE).as_deref() == Some("doubleClick")));
    }
    if update.enable_sticky_scroll.is_none() {
        update.enable_sticky_scroll = Some(config_bool(service, TREE_STICKY_SCROLL));
    }
    if update.sticky_scroll_max_item_count.is_none() {
        update.sticky_scroll_max_item_count = config_count(service, TREE_STICKY_SCROLL_MAX_ELEMENTS);
    }
}

/// O que o `WorkbenchTreeInternals` lê do estado do nó em foco (o `updateCollapseContextKeys`).
#[derive(Clone, Copy)]
pub struct FocusedNodeState {
    pub collapsible: bool,
    pub collapsed: bool,
    pub has_parent: bool,
    pub has_child: bool,
}

/// O que o [`WorkbenchTreeInternals`] e o [`ListService`] precisam de uma árvore, sem o tipo do modelo nem do elemento. O original
/// recebe qualquer uma das cinco árvores do workbench; aqui o elemento vira `Rc<dyn Any>` e as opções viram o [`TreeOptionsDelta`].
pub trait WorkbenchTreeHost {
    fn html_element(&self) -> NodeId;
    fn is_dom_focused(&self) -> bool;
    fn on_did_change_selection(&self) -> Event<()>;
    fn on_did_change_focus(&self) -> Event<()>;
    fn on_did_change_collapse_state(&self) -> Event<()>;
    fn on_did_change_model(&self) -> Event<()>;
    fn on_did_change_sticky_scroll_focused(&self) -> Event<bool>;
    fn on_did_scroll(&self) -> Event<()>;
    fn on_did_dispose(&self) -> Event<()>;
    fn selection_count(&self) -> usize;
    fn focus_count(&self) -> usize;
    /// O primeiro elemento em foco, como o usuário da árvore o vê (`T`, não o nó interno).
    fn focused_element(&self) -> Option<Rc<dyn Any>>;
    fn focused_node_state(&self) -> Option<FocusedNodeState>;
    fn scroll_top(&self) -> f64;
    fn scroll_height(&self) -> f64;
    fn render_height(&self) -> f64;
    fn update_options(&self, delta: &TreeOptionsDelta);
    fn style(&self, styles: &IListStyles);
    /// `tree instanceof WorkbenchDataTree || tree instanceof WorkbenchAsyncDataTree` seguido de `tree.getInput()`.
    fn data_tree_input(&self) -> Option<Option<Rc<dyn Any>>>;
}

/// O [`WorkbenchTreeHost`] de qualquer [`AbstractTree`] cujos elementos e referências são `Option<X>` (o `null` é a raiz): a
/// `ObjectTree`, a `CompressibleObjectTree` e a árvore por baixo das de dados assíncronos.
struct AbstractTreeHost<M, X>
where
    M: ITreeModel<Element = Option<X>, Ref = Option<X>>,
    X: Clone + Eq + Hash + 'static,
{
    tree: Rc<AbstractTree<M>>,
    /// O elemento como o usuário o vê.
    to_user: Rc<dyn Fn(&X) -> Rc<dyn Any>>,
    /// `getInput()` das árvores de dados.
    input: Option<Rc<dyn Fn() -> Option<Rc<dyn Any>>>>,
}

impl<M, X> WorkbenchTreeHost for AbstractTreeHost<M, X>
where
    M: ITreeModel<Element = Option<X>, Ref = Option<X>>,
    X: Clone + Eq + Hash + 'static,
{
    fn html_element(&self) -> NodeId {
        self.tree.get_html_element()
    }

    fn is_dom_focused(&self) -> bool {
        self.tree.is_dom_focused()
    }

    fn on_did_change_selection(&self) -> Event<()> {
        self.tree.on_did_change_selection().map(|_| (), None)
    }

    fn on_did_change_focus(&self) -> Event<()> {
        self.tree.on_did_change_focus().map(|_| (), None)
    }

    fn on_did_change_collapse_state(&self) -> Event<()> {
        self.tree.on_did_change_collapse_state().map(|_| (), None)
    }

    fn on_did_change_model(&self) -> Event<()> {
        self.tree.on_did_change_model()
    }

    fn on_did_change_sticky_scroll_focused(&self) -> Event<bool> {
        self.tree.on_did_change_sticky_scroll_focused()
    }

    fn on_did_scroll(&self) -> Event<()> {
        self.tree.on_did_scroll().map(|_| (), None)
    }

    fn on_did_dispose(&self) -> Event<()> {
        self.tree.on_did_dispose()
    }

    fn selection_count(&self) -> usize {
        self.tree.get_selection().len()
    }

    fn focus_count(&self) -> usize {
        self.tree.get_focus().len()
    }

    fn focused_element(&self) -> Option<Rc<dyn Any>> {
        self.tree.get_focus().into_iter().next().flatten().map(|element| (self.to_user)(&element))
    }

    fn focused_node_state(&self) -> Option<FocusedNodeState> {
        let focused = self.tree.get_focus().into_iter().next().flatten()?;
        let location = Some(focused);
        let node = self.tree.get_node(&location);

        Some(FocusedNodeState {
            collapsible: node.collapsible(),
            collapsed: node.collapsed(),
            has_parent: self.tree.get_parent_element(&location).is_some(),
            has_child: self.tree.get_first_element_child(&location).flatten().is_some(),
        })
    }

    fn scroll_top(&self) -> f64 {
        self.tree.scroll_top()
    }

    fn scroll_height(&self) -> f64 {
        self.tree.scroll_height()
    }

    fn render_height(&self) -> f64 {
        self.tree.render_height()
    }

    fn update_options(&self, delta: &TreeOptionsDelta) {
        self.tree.update_options(&delta.to_update());
    }

    fn style(&self, styles: &IListStyles) {
        self.tree.style(styles);
    }

    fn data_tree_input(&self) -> Option<Option<Rc<dyn Any>>> {
        self.input.as_ref().map(|input| input())
    }
}

/// A árvore como o [`ListService`] a guarda (`WorkbenchListWidget`): o `getHTMLElement()`, o `onDidFocus` e o `onDidDispose`.
struct WorkbenchTreeListWidget {
    ui: Ui,
    host: Rc<dyn WorkbenchTreeHost>,
}

impl IListWidget for WorkbenchTreeListWidget {
    fn is_active_element(&self) -> bool {
        self.host.is_dom_focused()
    }

    fn get_focused_element(&self) -> Option<Rc<dyn Any>> {
        self.host.focused_element()
    }

    fn workbench_data_tree_input(&self) -> Option<Option<Rc<dyn Any>>> {
        self.host.data_tree_input()
    }

    fn on_did_focus(&self) -> Event<()> {
        let (ui, node) = (self.ui.clone(), self.host.html_element());
        Event::new(move |listener| ui.listen(node, EventKind::Focus, false, move |_| listener(&())))
    }

    fn on_did_dispose(&self) -> Event<()> {
        self.host.on_did_dispose()
    }

    fn set_last_focused(&self, last_focused: bool) {
        self.ui.tree_mut().toggle_class(self.host.html_element(), "last-focused", last_focused);
    }
}

/// Os serviços que o original injeta nas árvores do workbench (ver as divergências do módulo).
#[derive(Clone)]
pub struct WorkbenchTreeServices {
    pub context_key_service: Rc<dyn IContextKeyService>,
    pub list_service: Rc<ListService>,
    pub configuration_service: Rc<dyn IConfigurationService>,
    /// `defaultListStyles`.
    pub default_list_styles: Rc<dyn Fn() -> IListStyles>,
}

/// O que as `IWorkbench*TreeOptions` acrescentam às opções do widget: `IResourceNavigatorOptions`, `selectionNavigation` e
/// `overrideStyles`.
#[derive(Clone, Default)]
pub struct IWorkbenchTreeOptions {
    pub resource_navigator: IResourceNavigatorOptions,
    pub selection_navigation: bool,
    pub override_styles: Option<IListStyles>,
}

/// As partes das opções do widget que o `WorkbenchTreeInternals` lê (lidas antes do preâmbulo, que as sobrescreve).
#[derive(Clone, Copy, Default)]
pub struct WorkbenchTreeInternalsOptions {
    pub multiple_selection_support: Option<bool>,
    pub find_widget_enabled: Option<bool>,
    pub overrides: TreeOptionOverrides,
}

/// `WorkbenchTreeInternals`.
pub struct WorkbenchTreeInternals {
    base: Rc<Disposable>,
    host: Rc<dyn WorkbenchTreeHost>,
    default_list_styles: Rc<dyn Fn() -> IListStyles>,
    pub context_key_service: Rc<dyn IScopedContextKeyService>,
    list_supports_multi_select: Rc<dyn IContextKey>,
    use_alt_as_multiple_selection_modifier: Rc<Cell<bool>>,
}

impl WorkbenchTreeInternals {
    pub fn new(ui: &Ui, host: Rc<dyn WorkbenchTreeHost>, workbench: &IWorkbenchTreeOptions, options: WorkbenchTreeInternalsOptions, services: &WorkbenchTreeServices) -> Rc<Self> {
        let base = Disposable::new();

        // createScopedContextKeyService
        let context_key_service = services.context_key_service.create_scoped(ContextKeyTarget::new());
        let keys: &dyn IContextKeyService = &*context_key_service;
        raw_workbench_list_focus_context_key().bind_to(keys);
        base.register(context_key_service.clone());

        // createScrollObserver
        let list_scroll_at = raw_workbench_list_scroll_at_boundary_context_key().bind_to(keys);
        let update_scroll = {
            let host = host.clone();
            move || {
                let at_top = host.scroll_top() == 0.0;

                // We need a threshold `1` since scrollHeight is rounded.
                // https://developer.mozilla.org/en-US/docs/Web/API/Element/scrollHeight#determine_if_an_element_has_been_totally_scrolled
                let at_bottom = host.scroll_height() - host.render_height() - host.scroll_top() < 1.0;
                let position = match (at_top, at_bottom) {
                    (true, true) => "both",
                    (true, false) => "top",
                    (false, true) => "bottom",
                    (false, false) => "none",
                };
                list_scroll_at.set(Dynamic::string(position));
            }
        };
        update_scroll();
        base.register(host.on_did_scroll().listen(move |_| update_scroll()));

        let list_supports_multi_select = workbench_list_supports_multi_select_context_key().bind_to(keys);
        list_supports_multi_select.set(Dynamic::Bool(options.multiple_selection_support != Some(false)));

        workbench_list_selection_navigation().bind_to(keys).set(Dynamic::Bool(workbench.selection_navigation));

        let list_support_find_widget = workbench_list_supports_find().bind_to(keys);
        list_support_find_widget.set(Dynamic::Bool(options.find_widget_enabled.unwrap_or(true)));

        let has_selection_or_focus = workbench_list_has_selection_or_focus().bind_to(keys);
        let has_double_selection = workbench_list_double_selection().bind_to(keys);
        let has_multi_selection = workbench_list_multi_selection().bind_to(keys);

        let tree_element_can_collapse = workbench_tree_element_can_collapse().bind_to(keys);
        let tree_element_has_parent = workbench_tree_element_has_parent().bind_to(keys);
        let tree_element_can_expand = workbench_tree_element_can_expand().bind_to(keys);
        let tree_element_has_child = workbench_tree_element_has_child().bind_to(keys);

        // Criada com o padrão `false`; ver as divergências do módulo.
        workbench_tree_find_open().bind_to(keys);
        let tree_sticky_scroll_focused = workbench_tree_sticky_scroll_focused().bind_to(keys);

        let use_alt = Rc::new(Cell::new(use_alt_as_multiple_selection_modifier(services.configuration_service.as_ref())));

        let this = Rc::new(Self { base, host: host.clone(), default_list_styles: services.default_list_styles.clone(), context_key_service: context_key_service.clone(), list_supports_multi_select, use_alt_as_multiple_selection_modifier: use_alt.clone() });
        this.update_style_overrides(workbench.override_styles.as_ref());

        let update_collapse_context_keys = {
            let host = host.clone();
            Rc::new(move || {
                let Some(state) = host.focused_node_state() else { return };
                tree_element_can_collapse.set(Dynamic::Bool(state.collapsible && !state.collapsed));
                tree_element_has_parent.set(Dynamic::Bool(state.has_parent));
                tree_element_can_expand.set(Dynamic::Bool(state.collapsible && state.collapsed));
                tree_element_has_child.set(Dynamic::Bool(state.has_child));
            })
        };

        let list_widget: Rc<dyn IListWidget> = Rc::new(WorkbenchTreeListWidget { ui: ui.clone(), host: host.clone() });
        this.base.register(services.list_service.register(list_widget, Vec::new()));

        {
            let (host, keys_service, has_selection_or_focus, has_multi_selection, has_double_selection) =
                (host.clone(), context_key_service.clone(), has_selection_or_focus.clone(), has_multi_selection, has_double_selection);
            this.base.register(host.clone().on_did_change_selection().listen(move |_| {
                let selection = host.selection_count();
                let focus = host.focus_count();

                keys_service.buffer_change_events(&|| {
                    has_selection_or_focus.set(Dynamic::Bool(selection > 0 || focus > 0));
                    has_multi_selection.set(Dynamic::Bool(selection > 1));
                    has_double_selection.set(Dynamic::Bool(selection == 2));
                });
            }));
        }
        {
            let (host, update) = (host.clone(), update_collapse_context_keys.clone());
            this.base.register(host.clone().on_did_change_focus().listen(move |_| {
                has_selection_or_focus.set(Dynamic::Bool(host.selection_count() > 0 || host.focus_count() > 0));
                update();
            }));
        }
        {
            let update = update_collapse_context_keys.clone();
            this.base.register(host.on_did_change_collapse_state().listen(move |_| update()));
        }
        {
            let update = update_collapse_context_keys;
            this.base.register(host.on_did_change_model().listen(move |_| update()));
        }
        this.base.register(host.on_did_change_sticky_scroll_focused().listen(move |focused| tree_sticky_scroll_focused.set(Dynamic::Bool(*focused))));

        {
            let (service, host, overrides) = (services.configuration_service.clone(), host.clone(), options.overrides);
            this.base.register(services.configuration_service.on_did_change_configuration().listen(move |e| {
                let service = service.as_ref();
                let mut delta = TreeOptionsDelta::default();
                if e.affects_configuration(MULTI_SELECT_MODIFIER_SETTING_KEY, None) {
                    use_alt.set(use_alt_as_multiple_selection_modifier(service));
                }
                if e.affects_configuration(TREE_INDENT_KEY, None) {
                    delta.indent = config_number(service, TREE_INDENT_KEY);
                }
                if e.affects_configuration(TREE_RENDER_INDENT_GUIDES_KEY, None) && !overrides.render_indent_guides {
                    delta.render_indent_guides = config_render_indent_guides(service);
                }
                if e.affects_configuration(LIST_SMOOTH_SCROLLING, None) {
                    delta.smooth_scrolling = Some(config_bool(service, LIST_SMOOTH_SCROLLING));
                }
                if e.affects_configuration(DEFAULT_FIND_MODE_SETTING_KEY, None) || e.affects_configuration(KEYBOARD_NAVIGATION_SETTING_KEY, None) {
                    delta.default_find_mode = get_default_tree_find_mode(service);
                }
                if e.affects_configuration(DEFAULT_FIND_MATCH_TYPE_SETTING_KEY, None) {
                    delta.default_find_match_type = get_default_tree_find_match_type(service);
                }
                if e.affects_configuration(SCROLL_BY_PAGE_KEY, None) {
                    delta.scroll_by_page = Some(config_bool(service, SCROLL_BY_PAGE_KEY));
                }
                if e.affects_configuration(TREE_EXPAND_MODE, None) && !overrides.expand_only_on_twistie_click {
                    delta.expand_only_on_twistie_click = Some(config_string(service, TREE_EXPAND_MODE).as_deref() == Some("doubleClick"));
                }
                if e.affects_configuration(TREE_STICKY_SCROLL, None) && !overrides.enable_sticky_scroll {
                    delta.enable_sticky_scroll = Some(config_bool(service, TREE_STICKY_SCROLL));
                }
                if e.affects_configuration(TREE_STICKY_SCROLL_MAX_ELEMENTS, None) && !overrides.sticky_scroll_max_item_count {
                    delta.sticky_scroll_max_item_count = config_count(service, TREE_STICKY_SCROLL_MAX_ELEMENTS).map(|count| count.max(1));
                }
                if e.affects_configuration(MOUSE_WHEEL_SCROLL_SENSITIVITY_KEY, None) {
                    delta.mouse_wheel_scroll_sensitivity = config_number(service, MOUSE_WHEEL_SCROLL_SENSITIVITY_KEY);
                }
                if e.affects_configuration(FAST_SCROLL_SENSITIVITY_KEY, None) {
                    delta.fast_scroll_sensitivity = config_number(service, FAST_SCROLL_SENSITIVITY_KEY);
                }
                if !delta.is_empty() {
                    host.update_options(&delta);
                }
            }));
        }

        this
    }

    /// `useAltAsMultipleSelectionModifier`.
    pub fn use_alt_as_multiple_selection_modifier(&self) -> bool {
        self.use_alt_as_multiple_selection_modifier.get()
    }

    /// `updateOptions(options)`: só o `multipleSelectionSupport` interessa.
    pub fn update_options(&self, multiple_selection_support: Option<bool>) {
        if let Some(support) = multiple_selection_support {
            self.list_supports_multi_select.set(Dynamic::Bool(support));
        }
    }

    /// `updateStyleOverrides(overrideStyles)`.
    pub fn update_style_overrides(&self, override_styles: Option<&IListStyles>) {
        self.host.style(&override_styles.cloned().unwrap_or_else(|| (self.default_list_styles)()));
    }

    /// Entrega ao `dispose` do `internals` um recurso que o original guarda no mesmo array (o navegador de recursos).
    pub fn register(&self, disposable: DisposableRef) {
        self.base.register(disposable);
    }
}

impl IDisposable for WorkbenchTreeInternals {
    fn dispose(&self) {
        self.base.dispose();
    }

    fn constructor_name(&self) -> &'static str {
        "WorkbenchTreeInternals"
    }
}

/// Uma árvore do workbench: o widget `W` (acessível por `Deref`) com o `internals` e o `onDidOpen`.
pub struct WorkbenchTree<W, T: Clone + 'static> {
    tree: Rc<W>,
    internals: Rc<WorkbenchTreeInternals>,
    navigator: Rc<ResourceNavigator<T>>,
}

impl<W, T: Clone + 'static> Deref for WorkbenchTree<W, T> {
    type Target = W;

    fn deref(&self) -> &W {
        &self.tree
    }
}

impl<W: IDisposable, T: Clone + 'static> IDisposable for WorkbenchTree<W, T> {
    fn dispose(&self) {
        self.internals.dispose();
        self.tree.dispose();
    }

    fn constructor_name(&self) -> &'static str {
        "WorkbenchTree"
    }
}

impl<W, T: Clone + 'static> WorkbenchTree<W, T> {
    /// O widget por baixo, para quem precisa do `Rc`.
    pub fn widget(&self) -> &Rc<W> {
        &self.tree
    }

    /// `contextKeyService`.
    pub fn context_key_service(&self) -> Rc<dyn IScopedContextKeyService> {
        self.internals.context_key_service.clone()
    }

    /// `useAltAsMultipleSelectionModifier`.
    pub fn use_alt_as_multiple_selection_modifier(&self) -> bool {
        self.internals.use_alt_as_multiple_selection_modifier()
    }

    /// `onDidOpen`.
    pub fn on_did_open(&self) -> Event<IOpenEvent<T>> {
        self.navigator.on_did_open()
    }

    fn assemble(
        ui: &Ui,
        tree: Rc<W>,
        host: Rc<dyn WorkbenchTreeHost>,
        navigator: Rc<ResourceNavigator<T>>,
        workbench: &IWorkbenchTreeOptions,
        options: WorkbenchTreeInternalsOptions,
        services: &WorkbenchTreeServices,
    ) -> Rc<Self> {
        let internals = WorkbenchTreeInternals::new(ui, host, workbench, options, services);
        internals.register(navigator.clone());
        Rc::new(Self { tree, internals, navigator })
    }
}

impl<W, T, M> WorkbenchTree<W, T>
where
    T: Clone + 'static,
    W: Deref<Target = AbstractTree<M>>,
    M: ITreeModel,
    M::Element: Eq + Hash,
{
    /// `updateOptions(options)` sobrescrito: a atualização do widget, depois os estilos e o `internals`.
    pub fn update_options(&self, update: &AbstractTreeOptionsUpdate<M::Element>, override_styles: Option<&IListStyles>) {
        self.tree.update_options(update);

        if let Some(override_styles) = override_styles {
            self.internals.update_style_overrides(Some(override_styles));
        }

        self.internals.update_options(update.multiple_selection_support);
    }
}

/// O `ResourceNavigator` das opções e da configuração (`{ configurationService, ...options }`).
fn navigator_options(workbench: &IWorkbenchTreeOptions, services: &WorkbenchTreeServices) -> IResourceNavigatorOptions {
    IResourceNavigatorOptions {
        configuration_service: workbench.resource_navigator.configuration_service.clone().or_else(|| Some(services.configuration_service.clone())),
        open_on_single_click: workbench.resource_navigator.open_on_single_click,
    }
}

/// `WorkbenchObjectTree<T, TFilterData>`.
pub type WorkbenchObjectTree<T, F = ()> = WorkbenchTree<ObjectTree<T, F>, T>;

impl<T: Clone + Eq + Hash + 'static, F: Clone + 'static> WorkbenchTree<ObjectTree<T, F>, T> {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        user: &str,
        ui: &Ui,
        container: NodeId,
        delegate: Rc<dyn IListVirtualDelegate<T>>,
        renderers: Vec<Rc<dyn ITreeRenderer<ObjectTreeModel<T, F>>>>,
        mut options: ObjectTreeOptions<T, F>,
        workbench: IWorkbenchTreeOptions,
        services: &WorkbenchTreeServices,
    ) -> Rc<Self> {
        let internals_options = WorkbenchTreeInternalsOptions {
            multiple_selection_support: options.tree.update.multiple_selection_support,
            find_widget_enabled: options.tree.find_widget_enabled,
            overrides: TreeOptionOverrides::of(&options.tree.update),
        };
        workbench_tree_data_preamble(services.configuration_service.as_ref(), &mut options.tree.update, &mut options.tree.keyboard_support);

        let tree = ObjectTree::new(user, ui, container, delegate, renderers, options);

        let host: Rc<dyn WorkbenchTreeHost> = Rc::new(AbstractTreeHost { tree: tree.abstract_tree().clone(), to_user: Rc::new(|element: &T| Rc::new(element.clone()) as Rc<dyn Any>), input: None });

        // O `TreeResourceNavigator`: os eventos da árvore de objetos são sobre `T | null`.
        let on_did_change_selection = tree.on_did_change_selection().map(
            |event: &TreeEvent<Option<T>>| TreeEvent { elements: event.elements.iter().flatten().cloned().collect(), browser_event: event.browser_event },
            None,
        );
        let flatten_mouse_event = |event: &TreeMouseEvent<Option<T>>| TreeMouseEvent {
            browser_event: event.browser_event,
            is_handled_by_list: event.is_handled_by_list.clone(),
            element: event.element.clone().flatten(),
            target: event.target,
        };
        let weak = Rc::downgrade(&tree);
        let navigator = ResourceNavigator::new(
            on_did_change_selection,
            tree.on_pointer().map(flatten_mouse_event, None),
            tree.on_mouse_dbl_click().map(flatten_mouse_event, None),
            move || weak.upgrade().and_then(|tree| tree.get_selection().into_iter().next().flatten()),
            Some(navigator_options(&workbench, services)),
        );

        Self::assemble(ui, tree, host, navigator, &workbench, internals_options, services)
    }
}

/// O que as duas árvores de dados assíncronos compartilham depois de criar o widget.
fn async_data_tree_parts<TInput, T, F, B>(
    ui: &Ui,
    tree: Rc<AsyncDataTree<TInput, T, F, B>>,
    workbench: &IWorkbenchTreeOptions,
    internals_options: WorkbenchTreeInternalsOptions,
    services: &WorkbenchTreeServices,
) -> Rc<WorkbenchTree<AsyncDataTree<TInput, T, F, B>, T>>
where
    TInput: Clone + 'static,
    T: Clone + Eq + Hash + 'static,
    F: Clone + 'static,
    B: AsyncDataTreeBackend<TInput, T, F>,
{
    let weak_for_input = Rc::downgrade(&tree);
    let host: Rc<dyn WorkbenchTreeHost> = Rc::new(AbstractTreeHost {
        tree: tree.abstract_tree().clone(),
        to_user: Rc::new(|node: &AsyncNode<TInput, T>| Rc::new(node.element_t()) as Rc<dyn Any>),
        input: Some(Rc::new(move || weak_for_input.upgrade().and_then(|tree| tree.get_input()).map(|input| Rc::new(input) as Rc<dyn Any>))),
    });

    let weak = Rc::downgrade(&tree);
    let navigator = ResourceNavigator::new(
        tree.on_did_change_selection(),
        tree.on_pointer(),
        tree.on_mouse_dbl_click(),
        move || weak.upgrade().and_then(|tree| tree.get_selection().into_iter().next()),
        Some(navigator_options(workbench, services)),
    );

    WorkbenchTree::assemble(ui, tree, host, navigator, workbench, internals_options, services)
}

/// `WorkbenchAsyncDataTree<TInput, T, TFilterData>`.
pub type WorkbenchAsyncDataTree<TInput, T, F = ()> = WorkbenchTree<AsyncDataTree<TInput, T, F, ObjectBackend>, T>;

impl<TInput: Clone + 'static, T: Clone + Eq + Hash + 'static, F: Clone + 'static> WorkbenchTree<AsyncDataTree<TInput, T, F, ObjectBackend>, T> {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        user: &str,
        ui: &Ui,
        container: NodeId,
        delegate: Rc<dyn IListVirtualDelegate<T>>,
        renderers: Vec<Rc<dyn IAsyncDataTreeRenderer<TInput, T, F>>>,
        data_source: Rc<dyn IAsyncDataSource<TInput, T>>,
        mut options: AsyncDataTreeOptions<T, F>,
        workbench: IWorkbenchTreeOptions,
        services: &WorkbenchTreeServices,
    ) -> Rc<Self> {
        let internals_options = WorkbenchTreeInternalsOptions {
            multiple_selection_support: options.update.multiple_selection_support,
            find_widget_enabled: options.find_widget_enabled,
            overrides: TreeOptionOverrides::of(&options.update),
        };
        workbench_tree_data_preamble(services.configuration_service.as_ref(), &mut options.update, &mut options.keyboard_support);

        let tree = AsyncDataTree::<TInput, T, F, ObjectBackend>::new(user, ui, container, delegate, renderers, data_source, options);
        async_data_tree_parts(ui, tree, &workbench, internals_options, services)
    }
}

/// `WorkbenchCompressibleAsyncDataTree<TInput, T, TFilterData>`.
pub type WorkbenchCompressibleAsyncDataTree<TInput, T, F = ()> = WorkbenchTree<AsyncDataTree<TInput, T, F, CompressibleBackend<T, F>>, T>;

impl<TInput: Clone + 'static, T: Clone + Eq + Hash + 'static, F: Clone + 'static> WorkbenchTree<AsyncDataTree<TInput, T, F, CompressibleBackend<T, F>>, T> {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        user: &str,
        ui: &Ui,
        container: NodeId,
        virtual_delegate: Rc<dyn IListVirtualDelegate<T>>,
        compression_delegate: Rc<dyn ITreeCompressionDelegate<T>>,
        renderers: Vec<Rc<dyn ICompressibleAsyncDataTreeRenderer<TInput, T, F>>>,
        data_source: Rc<dyn IAsyncDataSource<TInput, T>>,
        mut options: CompressibleAsyncDataTreeOptions<T, F>,
        workbench: IWorkbenchTreeOptions,
        services: &WorkbenchTreeServices,
    ) -> Rc<Self> {
        let internals_options = WorkbenchTreeInternalsOptions {
            multiple_selection_support: options.base.update.multiple_selection_support,
            find_widget_enabled: options.base.find_widget_enabled,
            overrides: TreeOptionOverrides::of(&options.base.update),
        };
        workbench_tree_data_preamble(services.configuration_service.as_ref(), &mut options.base.update, &mut options.base.keyboard_support);

        let tree = AsyncDataTree::<TInput, T, F, CompressibleBackend<T, F>>::new(user, ui, container, virtual_delegate, compression_delegate, renderers, data_source, options);
        async_data_tree_parts(ui, tree, &workbench, internals_options, services)
    }
}

/// O registro da configuração `workbench.list.*` e `workbench.tree.*` (o corpo do módulo do original roda no `import`).
pub fn register_list_configuration() {
    let strings = |values: &[&str]| Dynamic::array(values.iter().map(|value| Dynamic::string(value)));
    let described = |entries: Vec<(&'static str, Dynamic)>| PropertySchema::from_entries(entries);

    let properties = vec![
        (
            MULTI_SELECT_MODIFIER_SETTING_KEY,
            described(vec![
                ("type", Dynamic::string("string")),
                ("enum", strings(&["ctrlCmd", "alt"])),
                (
                    "markdownEnumDescriptions",
                    Dynamic::array([
                        Dynamic::string(&localize("multiSelectModifier.ctrlCmd", "Maps to `Control` on Windows and Linux and to `Command` on macOS.", &[])),
                        Dynamic::string(&localize("multiSelectModifier.alt", "Maps to `Alt` on Windows and Linux and to `Option` on macOS.", &[])),
                    ]),
                ),
                ("default", Dynamic::string("ctrlCmd")),
                (
                    "description",
                    Dynamic::string(&localize(
                        "multiSelectModifier",
                        "The modifier to be used to add an item in trees and lists to a multi-selection with the mouse (for example in the explorer, open editors and scm view). The 'Open to Side' mouse gestures - if supported - will adapt such that they do not conflict with the multiselect modifier.",
                        &[],
                    )),
                ),
            ]),
        ),
        (
            OPEN_MODE_SETTING_KEY,
            described(vec![
                ("type", Dynamic::string("string")),
                ("enum", strings(&["singleClick", "doubleClick"])),
                ("default", Dynamic::string("singleClick")),
                (
                    "description",
                    Dynamic::string(&localize(
                        "openModeModifier",
                        "Controls how to open items in trees and lists using the mouse (if supported). Note that some trees and lists might choose to ignore this setting if it is not applicable.",
                        &[],
                    )),
                ),
            ]),
        ),
        (
            HORIZONTAL_SCROLLING_KEY,
            described(vec![
                ("type", Dynamic::string("boolean")),
                ("default", Dynamic::Bool(false)),
                (
                    "description",
                    Dynamic::string(&localize(
                        "horizontalScrolling setting",
                        "Controls whether lists and trees support horizontal scrolling in the workbench. Warning: turning on this setting has a performance implication.",
                        &[],
                    )),
                ),
            ]),
        ),
        (
            SCROLL_BY_PAGE_KEY,
            described(vec![
                ("type", Dynamic::string("boolean")),
                ("default", Dynamic::Bool(false)),
                ("description", Dynamic::string(&localize("list.scrollByPage", "Controls whether clicks in the scrollbar scroll page by page.", &[]))),
            ]),
        ),
        (
            TREE_INDENT_KEY,
            described(vec![
                ("type", Dynamic::string("number")),
                ("default", Dynamic::Number(8.0)),
                ("minimum", Dynamic::Number(4.0)),
                ("maximum", Dynamic::Number(40.0)),
                ("description", Dynamic::string(&localize("tree indent setting", "Controls tree indentation in pixels.", &[]))),
            ]),
        ),
        (
            TREE_RENDER_INDENT_GUIDES_KEY,
            described(vec![
                ("type", Dynamic::string("string")),
                ("enum", strings(&["none", "onHover", "always"])),
                ("default", Dynamic::string("onHover")),
                ("description", Dynamic::string(&localize("render tree indent guides", "Controls whether the tree should render indent guides.", &[]))),
            ]),
        ),
        (
            LIST_SMOOTH_SCROLLING,
            described(vec![
                ("type", Dynamic::string("boolean")),
                ("default", Dynamic::Bool(false)),
                ("description", Dynamic::string(&localize("list smoothScrolling setting", "Controls whether lists and trees have smooth scrolling.", &[]))),
            ]),
        ),
        (
            MOUSE_WHEEL_SCROLL_SENSITIVITY_KEY,
            described(vec![
                ("type", Dynamic::string("number")),
                ("default", Dynamic::Number(1.0)),
                (
                    "markdownDescription",
                    Dynamic::string(&localize("Mouse Wheel Scroll Sensitivity", "A multiplier to be used on the `deltaX` and `deltaY` of mouse wheel scroll events.", &[])),
                ),
            ]),
        ),
        (
            FAST_SCROLL_SENSITIVITY_KEY,
            described(vec![
                ("type", Dynamic::string("number")),
                ("default", Dynamic::Number(5.0)),
                ("markdownDescription", Dynamic::string(&localize("Fast Scroll Sensitivity", "Scrolling speed multiplier when pressing `Alt`.", &[]))),
            ]),
        ),
        (
            DEFAULT_FIND_MODE_SETTING_KEY,
            described(vec![
                ("type", Dynamic::string("string")),
                ("enum", strings(&["highlight", "filter"])),
                (
                    "enumDescriptions",
                    Dynamic::array([
                        Dynamic::string(&localize(
                            "defaultFindModeSettingKey.highlight",
                            "Highlight elements when searching. Further up and down navigation will traverse only the highlighted elements.",
                            &[],
                        )),
                        Dynamic::string(&localize("defaultFindModeSettingKey.filter", "Filter elements when searching.", &[])),
                    ]),
                ),
                ("default", Dynamic::string("highlight")),
                ("description", Dynamic::string(&localize("defaultFindModeSettingKey", "Controls the default find mode for lists and trees in the workbench.", &[]))),
            ]),
        ),
        (
            KEYBOARD_NAVIGATION_SETTING_KEY,
            described(vec![
                ("type", Dynamic::string("string")),
                ("enum", strings(&["simple", "highlight", "filter"])),
                (
                    "enumDescriptions",
                    Dynamic::array([
                        Dynamic::string(&localize(
                            "keyboardNavigationSettingKey.simple",
                            "Simple keyboard navigation focuses elements which match the keyboard input. Matching is done only on prefixes.",
                            &[],
                        )),
                        Dynamic::string(&localize(
                            "keyboardNavigationSettingKey.highlight",
                            "Highlight keyboard navigation highlights elements which match the keyboard input. Further up and down navigation will traverse only the highlighted elements.",
                            &[],
                        )),
                        Dynamic::string(&localize(
                            "keyboardNavigationSettingKey.filter",
                            "Filter keyboard navigation will filter out and hide all the elements which do not match the keyboard input.",
                            &[],
                        )),
                    ]),
                ),
                ("default", Dynamic::string("highlight")),
                (
                    "description",
                    Dynamic::string(&localize(
                        "keyboardNavigationSettingKey",
                        "Controls the keyboard navigation style for lists and trees in the workbench. Can be simple, highlight and filter.",
                        &[],
                    )),
                ),
                ("deprecated", Dynamic::Bool(true)),
                (
                    "deprecationMessage",
                    Dynamic::string(&localize(
                        "keyboardNavigationSettingKeyDeprecated",
                        "Please use 'workbench.list.defaultFindMode' and	'workbench.list.typeNavigationMode' instead.",
                        &[],
                    )),
                ),
            ]),
        ),
        (
            DEFAULT_FIND_MATCH_TYPE_SETTING_KEY,
            described(vec![
                ("type", Dynamic::string("string")),
                ("enum", strings(&["fuzzy", "contiguous"])),
                (
                    "enumDescriptions",
                    Dynamic::array([
                        Dynamic::string(&localize("defaultFindMatchTypeSettingKey.fuzzy", "Use fuzzy matching when searching.", &[])),
                        Dynamic::string(&localize("defaultFindMatchTypeSettingKey.contiguous", "Use contiguous matching when searching.", &[])),
                    ]),
                ),
                ("default", Dynamic::string("fuzzy")),
                (
                    "description",
                    Dynamic::string(&localize("defaultFindMatchTypeSettingKey", "Controls the type of matching used when searching lists and trees in the workbench.", &[])),
                ),
            ]),
        ),
        (
            TREE_EXPAND_MODE,
            described(vec![
                ("type", Dynamic::string("string")),
                ("enum", strings(&["singleClick", "doubleClick"])),
                ("default", Dynamic::string("singleClick")),
                (
                    "description",
                    Dynamic::string(&localize(
                        "expand mode",
                        "Controls how tree folders are expanded when clicking the folder names. Note that some trees and lists might choose to ignore this setting if it is not applicable.",
                        &[],
                    )),
                ),
            ]),
        ),
        (
            TREE_STICKY_SCROLL,
            described(vec![
                ("type", Dynamic::string("boolean")),
                ("default", Dynamic::Bool(true)),
                ("description", Dynamic::string(&localize("sticky scroll", "Controls whether sticky scrolling is enabled in trees.", &[]))),
            ]),
        ),
        (
            TREE_STICKY_SCROLL_MAX_ELEMENTS,
            described(vec![
                ("type", Dynamic::string("number")),
                ("minimum", Dynamic::Number(1.0)),
                ("default", Dynamic::Number(7.0)),
                (
                    "markdownDescription",
                    Dynamic::string(&localize(
                        "sticky scroll maximum items",
                        "Controls the number of sticky elements displayed in the tree when {0} is enabled.",
                        &["`#workbench.tree.enableStickyScroll#`"],
                    )),
                ),
            ]),
        ),
        (
            TYPE_NAVIGATION_MODE_SETTING_KEY,
            described(vec![
                ("type", Dynamic::string("string")),
                ("enum", strings(&["automatic", "trigger"])),
                ("default", Dynamic::string("automatic")),
                (
                    "markdownDescription",
                    Dynamic::string(&localize(
                        "typeNavigationMode2",
                        "Controls how type navigation works in lists and trees in the workbench. When set to `trigger`, type navigation begins once the `list.triggerTypeNavigation` command is run.",
                        &[],
                    )),
                ),
            ]),
        ),
    ];

    configuration_registry().register_configuration(ConfigurationNode::new(workbench_configuration_node_base().with_properties(properties)));
}
