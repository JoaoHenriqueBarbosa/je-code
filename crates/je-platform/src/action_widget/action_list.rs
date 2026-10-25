//! Porte parcial de `src/vs/platform/actionWidget/browser/actionList.ts` (3664 linhas): o contrato (`IActionListItem`,
//! `IActionListDelegate`, `IActionListOptions`, `IActionListUpdateOptions`, `ActionListItemKind`), o estado do
//! `ActionListWidget` que o `actionWidget.ts` e o seu teste usam (itens, filtro, foco, seções, aceitar, a fila de
//! submenu) e o `ActionList` com o posicionamento por âncora, sem DOM: a lista é o estado nativo, o pintor a desenha.
//!
//! Dentro do recorte, o corpo é o do original, na mesma ordem: `_applyFilter`, `updateItems`, `focusNext/Previous`,
//! `_focusCheckedOrFirst`, `acceptSelected`, `onListSelection`, o filtro com combobox e composição de IME, a abertura do
//! submenu por ArrowRight com a lista de ações dele, o `ActionList.computeHeight` e a escolha do lado (`_showAbove`).
//!
//! Divergências de tradução:
//! - **`item`.** `IActionListItem.item` (`T`) é `Option<T>`, com `T: ActionListItemData` (`item?.id` e `item?.checked` do
//!   original, que lê `(item as { id?: string }).id`). Os itens da lista são `Rc<IActionListItem<T>>`, porque o original
//!   compara itens por identidade (`includes`, `indexOf`).
//! - **`List`.** O `List` do VS Code (`splice`, `focusNext(n, loop, _, filter)`, `setFocus`, `setSelection`) é o estado
//!   `focus`/`selection` sobre `visible_menu_items`: `setFocus` sempre dispara `onDidChangeFocus` (como o `Trait.set`) e o
//!   `splice` total limpa foco e seleção sem disparar (como o `Trait.splice`).
//! - **Foco e eventos do DOM.** `document.activeElement` é o [`FocusState`] da lista (o campo de filtro ou a lista) e os
//!   eventos entram por [`ActionListWidget::dispatch_key_down`] e pelos métodos `filter_input_*`, com o alvo explícito.
//! - **`onRemove` de uma `IAction`.** `Object.assign(action, { onRemove })` é o [`RemovableAction`], que embrulha a ação.
//! - **`_instantiationService.createInstance(ActionListWidget, ...)`** do submenu é [`ActionListWidget::new`].
//!
//! Fica para a fatia do `ActionList` (o resto das 3664 linhas): a renderização das linhas (`ActionItemRenderer`, ícones,
//! badges, atalhos, toggles pintados), o hover (`hover`, `onHover`, `persistentHover`, `_showHoverForElement`, o painel de
//! conteúdo de `IActionListItemHover` e o ponteiro com intenção), as barras de ferramentas por linha e
//! `focusItemAction`, o `Tab` entre linha e painel (`tabThroughItemActions`), o `softDispatch` de atalhos no filtro do
//! submenu, os atributos de acessibilidade (`aria-*`), a animação de movimento de item, o `markdown` das descrições e
//! `_measureItemWidths` (a medida de texto vem do renderizador nativo: [`ActionListWidget::set_item_measurer`]).

use std::any::Any;
use std::cell::{Cell, RefCell};
use std::collections::{BTreeMap, BTreeSet};
use std::rc::{Rc, Weak};

use je_base::actions::{ActionRef, IAction, SubmenuAction};
use je_base::dynamic::Dynamic;
use je_base::errors::Error;
use je_base::event::{Emitter, Event};
use je_base::key_codes::KeyCode;
use je_base::layout::{AnchorPosition, ILayoutAnchor, LayoutAnchorPosition, LayoutResult, layout as layout_along_axis};
use je_base::lifecycle::{DisposableRef, DisposableStore, IDisposable};
use je_base::promise::Promise;
use je_base::runtime::queue_microtask;

use crate::contextview::context_view::{AnchorTarget, IContextViewCloseAnimation, IContextViewService, get_anchor_rect};
use crate::contextview::view_element::{UiKeyEvent, ViewElement};
use crate::layout::layout_service::ILayoutService;

pub const ACCEPT_SELECTED_ACTION_COMMAND: &str = "acceptSelectedCodeAction";
pub const PREVIEW_SELECTED_ACTION_COMMAND: &str = "previewSelectedCodeAction";

/// Action ID of the auto-appended toolbar action created from [`IActionListItem::on_remove`].
pub const REMOVE_TOOLBAR_ACTION_ID: &str = "actionList.remove";

/// A altura (px) de uma linha de ação, de cabeçalho e de separador sem rótulo (`_actionLineHeight`, `_headerLineHeight` e
/// `_separatorLineHeight`).
const ACTION_LINE_HEIGHT: f64 = 24.0;
const HEADER_LINE_HEIGHT: f64 = 24.0;
const SEPARATOR_LINE_HEIGHT: f64 = 8.0;

/// A altura do filtro e do rodapé que o `ActionList.computeHeight` desconta, e a do cabeçalho sem altura própria.
const FILTER_HEIGHT: f64 = 36.0;
const FOOTER_HEIGHT: f64 = 32.0;
const DEFAULT_HEADER_HEIGHT: f64 = 36.0;

/// O espaço vertical que o `.action-widget` reserva em volta do conteúdo: `padding: 4px` e `border: 1px` nos dois lados
/// (`actionWidget.css`). É o que `computeActionWidgetVerticalChromeHeight` lê do estilo calculado.
pub const ACTION_WIDGET_VERTICAL_CHROME_HEIGHT: f64 = 10.0;

// #region contrato

/// O que o `ActionList` lê de um item: `(item as { id?: string }).id` e `(item as { checked?: boolean }).checked`.
pub trait ActionListItemData: Clone + 'static {
    /// `(item as { id?: string }).id`.
    fn list_item_id(&self) -> Option<String> {
        None
    }

    /// `(item as { checked?: boolean }).checked`.
    fn list_item_checked(&self) -> bool {
        false
    }
}

impl ActionListItemData for ActionRef {
    fn list_item_id(&self) -> Option<String> {
        Some(IAction::id(self.as_ref()))
    }

    fn list_item_checked(&self) -> bool {
        IAction::checked(self.as_ref()).unwrap_or(false)
    }
}

impl ActionListItemData for String {}

impl ActionListItemData for &'static str {}

/// `ActionListItemKind`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ActionListItemKind {
    Action,
    Header,
    Separator,
}

/// `{ kind?, icon?: ThemeIcon, title }`: o ícone é o id do codicon.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ActionListGroup {
    pub icon: Option<String>,
    pub title: String,
}

/// `IActionListItemInlineToggle`.
#[derive(Clone)]
pub struct IActionListItemInlineToggle {
    pub label: String,
    pub checked: bool,
    pub on_change: Rc<dyn Fn(bool)>,
    pub title: Option<String>,
    pub show_info_icon: bool,
    pub disabled: bool,
}

/// `onRemove?: () => void | Promise<void>`.
pub type OnRemove = Rc<dyn Fn() -> Promise<()>>;

/// `IActionListItem<T>`: os campos de dados. O hover, o atalho resolvido, os ícones por classe, `onDidBecomeVisible`,
/// `badgeBeforeLabel`, `additionalBadges`, `ariaDescription`, `opensDialog`, `toolbarDialogActionIds`, `detailLink`,
/// `hideIcon` e `tooltip` por linha pertencem à renderização e ao hover (ver a nota do módulo).
#[derive(Clone)]
pub struct IActionListItem<T: ActionListItemData> {
    pub item: Option<T>,
    pub kind: ActionListItemKind,
    pub group: Option<ActionListGroup>,
    pub disabled: bool,
    pub label: Option<String>,
    /// Optional detail text displayed as a second line below the label.
    pub detail: Option<String>,
    pub inline_toggle: Option<IActionListItemInlineToggle>,
    pub standalone_toggle: Option<IActionListItemInlineToggle>,
    pub description: Option<String>,
    /// Optional actions shown in a nested submenu panel.
    pub submenu_actions: Option<Vec<ActionRef>>,
    /// Open a submenu or expandable hover on row activation instead of selecting the item.
    pub open_submenu_on_click: bool,
    /// Options for the action list rendered in the nested submenu panel.
    pub submenu_options: Option<Rc<IActionListOptions>>,
    /// Items that replace this item when the parent list is filtered.
    pub filter_items: Option<Vec<Rc<IActionListItem<T>>>>,
    pub can_preview: Option<bool>,
    pub tooltip: Option<String>,
    pub toolbar_actions: Option<Vec<ActionRef>>,
    pub toolbar_labels: bool,
    pub section: Option<String>,
    pub is_section_toggle: bool,
    pub class_name: Option<String>,
    pub badge: Option<String>,
    pub show_always: bool,
    pub on_remove: Option<OnRemove>,
}

impl<T: ActionListItemData> IActionListItem<T> {
    /// Um item só com o `kind`, todo o resto ausente.
    pub fn new(kind: ActionListItemKind) -> Self {
        Self {
            item: None,
            kind,
            group: None,
            disabled: false,
            label: None,
            detail: None,
            inline_toggle: None,
            standalone_toggle: None,
            description: None,
            submenu_actions: None,
            open_submenu_on_click: false,
            submenu_options: None,
            filter_items: None,
            can_preview: None,
            tooltip: None,
            toolbar_actions: None,
            toolbar_labels: false,
            section: None,
            is_section_toggle: false,
            class_name: None,
            badge: None,
            show_always: false,
            on_remove: None,
        }
    }

    /// `{ kind: Action, label, item }`.
    pub fn action(label: &str, item: T) -> Self {
        Self { label: Some(label.to_owned()), item: Some(item), ..Self::new(ActionListItemKind::Action) }
    }

    /// `(item.item as { id?: string } | undefined)?.id`.
    pub fn item_id(&self) -> Option<String> {
        self.item.as_ref().and_then(ActionListItemData::list_item_id)
    }
}

/// `IActionListDelegate<T>`. O `onHover` fica com a fatia do hover (ver a nota do módulo).
pub trait IActionListDelegate<T: ActionListItemData> {
    fn on_hide(&self, did_cancel: Option<bool>);
    fn on_select(&self, action: &T, preview: bool);
    /// O `onFilter` está definido? Sem ele, o filtro é local.
    fn handles_filter(&self) -> bool {
        false
    }
    /// `onFilter(filter, cancellationToken)`: o `token` é o [`FilterRequest`], e quem o resolve confere
    /// [`FilterRequest::is_cancellation_requested`].
    fn on_filter(&self, _filter: &str, _request: &FilterRequest) -> Promise<Vec<Rc<IActionListItem<T>>>> {
        Promise::rejected(Error::new("onFilter is not defined".to_owned()))
    }
    /// `onFocus?(action)`.
    fn on_focus(&self, _action: Option<&T>) {}
}

/// `IActionListUpdateOptions`. `animateItemMove` é da animação de movimento (renderização).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct IActionListUpdateOptions {
    /// Retain the open hover or submenu when its replacement item has the same id.
    pub preserve_hover: bool,
    /// Keep the current viewport when items are refreshed in place.
    pub preserve_scroll_position: bool,
    /// Animate a visible focused item's move.
    pub animate_item_move: bool,
}

/// `IActionListOptions`: os campos que mudam o estado ou a altura. As opções de renderização, de hover e de ponteiro
/// (`linkHandler`, `inlineDescription`, `persistentHover` e o painel de hover, `submenuPointerIntent`,
/// `submenuHoverDelay`, `reserveSubmenuSpace`, `hideDefaultKeybindingTooltip`, `stopToolbarPointerPropagation`,
/// `tabThroughItemActions`, `headerIcon`, `headerLink`, `headerDismiss`) entram com a fatia da renderização.
#[derive(Clone, Default)]
pub struct IActionListOptions {
    /// When true, shows a filter input.
    pub show_filter: bool,
    pub filter_placeholder: Option<String>,
    /// Initial text for the filter input.
    pub initial_filter_value: Option<String>,
    /// Keep focus in the filter while navigating and accepting results, using combobox semantics.
    pub filter_as_combobox: bool,
    /// Reveals a filter when printable text is typed in a list without a filter input.
    pub on_type: Option<Rc<dyn Fn(&str)>>,
    /// Called when the user changes the filter text, but not when it is cleared programmatically.
    pub on_did_change_filter: Option<Rc<dyn Fn(&str)>>,
    pub filter_actions: Option<Vec<ActionRef>>,
    /// Section IDs that should be collapsed by default.
    pub collapsed_by_default: Option<BTreeSet<String>>,
    pub min_width: Option<f64>,
    pub max_width: Option<f64>,
    /// Maximum number of action rows shown before the list scrolls.
    pub max_visible_items: Option<usize>,
    /// Optional callback fired when a section's collapsed state changes.
    pub on_did_toggle_section: Option<Rc<dyn Fn(&str, bool)>>,
    /// Height (px) used for action items that have a `detail` line. Defaults to 48.
    pub detail_item_height: Option<f64>,
    /// Height (px) used for action items that have an `inlineToggle`. Defaults to 70.
    pub inline_toggle_item_height: Option<f64>,
    /// When true, the group title is shown on the first item of each group.
    pub show_group_title_on_first_item: bool,
    /// When true and filtering is enabled, focuses the filter input when the list opens.
    pub focus_filter_on_open: bool,
    /// Optional action item id to focus when the list opens.
    pub initial_focus_item_id: Option<String>,
    pub persistent_hover: bool,
    pub secondary_heading: Option<String>,
    /// Optional text shown below the action list as a footer.
    pub footer_text: Option<String>,
    /// Optional text shown above the action list as a header banner.
    pub header_text: Option<String>,
    /// Optional CSS class name added to the action list container, for scoped styling.
    pub class_name: Option<String>,
    /// Optional CSS class name added to the containing action widget.
    pub widget_class_name: Option<String>,
    /// Optional CSS class and duration used to animate the containing action widget before the context view is hidden.
    pub close_animation: Option<IContextViewCloseAnimation>,
    /// Optional fixed side of the anchor where the action list should render.
    pub anchor_position: Option<AnchorPosition>,
    /// Preferred side, falling back when needed and locking the resolved direction on first layout.
    pub preferred_anchor_position: Option<AnchorPosition>,
    /// Uses the available height instead of the usual fractional viewport cap.
    pub use_full_height: bool,
    /// Measures everything the containing popup shows besides the list's rows.
    pub get_popup_chrome_height: Option<Rc<dyn Fn(&Rc<ViewElement>) -> f64>>,
}

// #endregion

// #region ação com remoção

/// `IAction & { onRemove }`: a ação embrulhada com o `onRemove` que o `Object.assign` do original acrescenta.
pub struct RemovableAction {
    action: ActionRef,
    on_remove: OnRemove,
}

impl RemovableAction {
    pub fn new(action: ActionRef, on_remove: OnRemove) -> Rc<Self> {
        Rc::new(Self { action, on_remove })
    }

    pub fn on_remove(&self) -> OnRemove {
        self.on_remove.clone()
    }
}

impl IAction for RemovableAction {
    fn id(&self) -> String {
        self.action.id()
    }
    fn label(&self) -> String {
        self.action.label()
    }
    fn set_label(&self, value: String) {
        self.action.set_label(value);
    }
    fn tooltip(&self) -> String {
        self.action.tooltip()
    }
    fn set_tooltip(&self, value: String) {
        self.action.set_tooltip(value);
    }
    fn class(&self) -> Option<String> {
        self.action.class()
    }
    fn set_class(&self, value: Option<String>) {
        self.action.set_class(value);
    }
    fn enabled(&self) -> bool {
        self.action.enabled()
    }
    fn set_enabled(&self, value: bool) {
        self.action.set_enabled(value);
    }
    fn checked(&self) -> Option<bool> {
        self.action.checked()
    }
    fn set_checked(&self, value: Option<bool>) {
        self.action.set_checked(value);
    }
    fn alt(&self) -> Option<ActionRef> {
        self.action.alt()
    }
    fn run(&self, args: &[Dynamic]) -> Promise<Dynamic> {
        self.action.run(args)
    }
    fn as_any(&self) -> &dyn Any {
        self
    }
}

/// `(action as IAction & { onRemove?: () => void }).onRemove`.
fn on_remove_of(action: &ActionRef) -> Option<OnRemove> {
    action.as_any().downcast_ref::<RemovableAction>().map(RemovableAction::on_remove)
}

// #endregion

// #region foco, filtro e requisição de filtro

/// `document.activeElement` dentro de uma lista: o campo de filtro, a lista, ou nenhum dos dois.
#[derive(Default)]
pub struct FocusState {
    filter_focused: Cell<bool>,
    list_focused: Cell<bool>,
}

/// O `<input>` do filtro: o texto, o foco e os eventos `focus` e `blur`.
pub struct FilterInput {
    value: RefCell<String>,
    focus_state: Rc<FocusState>,
    on_did_focus: Rc<Emitter<()>>,
    on_did_blur: Rc<Emitter<()>>,
}

impl FilterInput {
    fn new(value: String, focus_state: Rc<FocusState>) -> Rc<Self> {
        Rc::new(Self { value: RefCell::new(value), focus_state, on_did_focus: Emitter::new(), on_did_blur: Emitter::new() })
    }

    /// `value`.
    pub fn value(&self) -> String {
        self.value.borrow().clone()
    }

    pub fn set_value(&self, value: &str) {
        *self.value.borrow_mut() = value.to_owned();
    }

    /// `dom.isActiveElement(input)`.
    pub fn is_focused(&self) -> bool {
        self.focus_state.filter_focused.get()
    }

    /// `input.focus()`: tira o foco da lista e dispara `focus` se ele ainda não estava no campo.
    pub fn focus(&self) {
        self.focus_state.list_focused.set(false);
        if !self.focus_state.filter_focused.replace(true) {
            self.on_did_focus.fire(());
        }
    }

    /// `input.blur()`.
    pub fn blur(&self) {
        if self.focus_state.filter_focused.replace(false) {
            self.on_did_blur.fire(());
        }
    }

    /// O evento `focus` do campo.
    pub fn on_did_focus(&self) -> Event<()> {
        self.on_did_focus.event()
    }

    /// O evento `blur` do campo.
    pub fn on_did_blur(&self) -> Event<()> {
        self.on_did_blur.event()
    }

    fn dispose(&self) {
        self.on_did_focus.dispose();
        self.on_did_blur.dispose();
    }
}

/// O `CancellationTokenSource` de uma requisição de `onFilter`.
#[derive(Default)]
pub struct FilterRequest {
    cancelled: Cell<bool>,
}

impl FilterRequest {
    /// `token.isCancellationRequested`.
    pub fn is_cancellation_requested(&self) -> bool {
        self.cancelled.get()
    }

    fn cancel(&self) {
        self.cancelled.set(true);
    }
}

/// O alvo de um evento de teclado, de dentro para fora: o campo de filtro, a lista, ou o painel do submenu.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum KeyTarget {
    FilterInput,
    List,
    SubmenuPanel,
}

/// O que ativou uma linha (`e.browserEvent` do `onDidChangeSelection`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SelectionEvent {
    /// `AcceptSelectedEvent`.
    Accept,
    /// `PreviewSelectedEvent`.
    Preview,
    /// Um clique ou toque, e a parte da linha que o recebeu.
    Pointer(PointerTarget),
}

/// `target.closest(...)` de um clique numa linha.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PointerTarget {
    Row,
    Toolbar,
    SubmenuIndicator,
    InlineToggle,
}

// #endregion

/// O estado do `ActionListWidget`.
struct WidgetState<T: ActionListItemData> {
    all_menu_items: Vec<Rc<IActionListItem<T>>>,
    visible_menu_items: Vec<Rc<IActionListItem<T>>>,
    focus: Option<usize>,
    selection: Option<usize>,
    collapsed_sections: BTreeSet<String>,
    filter_text: String,
    ime_session_in_progress: bool,
    initial_focus_item_id: Option<String>,
    has_laid_out: bool,
    keyboard_navigation: Option<bool>,
    pending_submenu_element: Option<Rc<IActionListItem<T>>>,
    current_submenu_element: Option<Rc<IActionListItem<T>>>,
    current_submenu_widget: Option<Rc<ActionListWidget<ActionRef>>>,
    has_layout_submenu: bool,
    hover_enabled: bool,
    filter_request: Option<Rc<FilterRequest>>,
    group_title_by_index: BTreeMap<usize, String>,
    standalone_toggles: Vec<(Rc<IActionListItem<T>>, bool)>,
}

/// A medida de largura de um item (o `_measureItemWidths` do original lê o DOM renderizado).
pub type ItemMeasurer<T> = Rc<dyn Fn(&IActionListItem<T>) -> f64>;

/// `ActionListWidget<T>`: a lista autônoma (filtro, seções, submenu), sem depender do serviço de visão de contexto.
pub struct ActionListWidget<T: ActionListItemData> {
    this: Weak<ActionListWidget<T>>,
    supports_preview: bool,
    delegate: Rc<dyn IActionListDelegate<T>>,
    options: Option<Rc<IActionListOptions>>,
    dom_node: Rc<ViewElement>,
    focus_state: Rc<FocusState>,
    filter_input: Option<Rc<FilterInput>>,
    filter_container: Option<Rc<ViewElement>>,
    footer_container: Option<Rc<ViewElement>>,
    header_container: Option<Rc<ViewElement>>,
    on_did_request_layout: Rc<Emitter<()>>,
    state: RefCell<WidgetState<T>>,
    measurer: RefCell<ItemMeasurer<T>>,
    is_submenu_panel: Cell<bool>,
    disposed: Cell<bool>,
    panel_subscriptions: Rc<DisposableStore>,
}

fn has_submenu_actions<T: ActionListItemData>(item: &IActionListItem<T>) -> bool {
    item.submenu_actions.as_ref().is_some_and(|actions| !actions.is_empty())
}

impl<T: ActionListItemData> ActionListWidget<T> {
    /// O construtor: cria o filtro (se pedido), aplica o filtro inicial e foca o item marcado ou o primeiro.
    pub fn new(
        supports_preview: bool,
        items: &[Rc<IActionListItem<T>>],
        delegate: Rc<dyn IActionListDelegate<T>>,
        options: Option<Rc<IActionListOptions>>,
    ) -> Rc<Self> {
        let option = |read: &dyn Fn(&IActionListOptions) -> Option<String>| options.as_deref().and_then(read);
        let filter_text = match options.as_deref() {
            Some(options) if options.show_filter => options.initial_filter_value.clone().unwrap_or_default(),
            _ => String::new(),
        };
        let dom_node = ViewElement::with_classes(&["actionList"]);
        if let Some(class_name) = option(&|o| o.class_name.clone()) {
            for class_name in class_name.split_whitespace() {
                dom_node.class_list_add(class_name);
            }
        }

        let collapsed_sections = options.as_deref().and_then(|o| o.collapsed_by_default.clone()).unwrap_or_default();
        let focus_state = Rc::new(FocusState::default());

        let show_filter = options.as_deref().is_some_and(|o| o.show_filter);
        let secondary_heading = option(&|o| o.secondary_heading.clone());
        let filter_input = if show_filter { Some(FilterInput::new(filter_text.clone(), focus_state.clone())) } else { None };
        let filter_container =
            if show_filter || secondary_heading.is_some() { Some(ViewElement::with_classes(&["action-list-filter"])) } else { None };
        let footer_container = option(&|o| o.footer_text.clone()).map(|_| ViewElement::with_classes(&["action-list-footer"]));
        let header_container = option(&|o| o.header_text.clone()).map(|_| ViewElement::with_classes(&["action-list-header"]));

        let widget = Rc::new_cyclic(|this| Self {
            this: this.clone(),
            supports_preview,
            delegate,
            options: options.clone(),
            dom_node,
            focus_state,
            filter_input,
            filter_container,
            footer_container,
            header_container,
            on_did_request_layout: Emitter::new(),
            state: RefCell::new(WidgetState {
                all_menu_items: items.to_vec(),
                visible_menu_items: Vec::new(),
                focus: None,
                selection: None,
                collapsed_sections,
                filter_text,
                ime_session_in_progress: false,
                initial_focus_item_id: options.as_deref().and_then(|o| o.initial_focus_item_id.clone()),
                has_laid_out: false,
                keyboard_navigation: None,
                pending_submenu_element: None,
                current_submenu_element: None,
                current_submenu_widget: None,
                has_layout_submenu: false,
                hover_enabled: true,
                filter_request: None,
                group_title_by_index: BTreeMap::new(),
                standalone_toggles: Vec::new(),
            }),
            measurer: RefCell::new(Rc::new(|_| 0.0)),
            is_submenu_panel: Cell::new(false),
            disposed: Cell::new(false),
            panel_subscriptions: DisposableStore::new(),
        });

        widget.apply_filter(false, true, None);

        if !widget.state.borrow().visible_menu_items.is_empty() {
            widget.focus_checked_or_first();
        }
        widget
    }

    fn rc(&self) -> Rc<Self> {
        self.this.upgrade().expect("o widget está vivo enquanto um método dele roda")
    }

    fn opts(&self) -> Option<&IActionListOptions> {
        self.options.as_deref()
    }

    // #region acessores

    /// `domNode`.
    pub fn dom_node(&self) -> Rc<ViewElement> {
        self.dom_node.clone()
    }

    /// `filterContainer`.
    pub fn filter_container(&self) -> Option<Rc<ViewElement>> {
        self.filter_container.clone()
    }

    /// `footerContainer`.
    pub fn footer_container(&self) -> Option<Rc<ViewElement>> {
        self.footer_container.clone()
    }

    /// `headerContainer`.
    pub fn header_container(&self) -> Option<Rc<ViewElement>> {
        self.header_container.clone()
    }

    /// `filterInput`.
    pub fn filter_input(&self) -> Option<Rc<FilterInput>> {
        self.filter_input.clone()
    }

    /// `closeAnimation`.
    pub fn close_animation(&self) -> Option<IContextViewCloseAnimation> {
        self.opts().and_then(|o| o.close_animation.clone())
    }

    /// `onDidRequestLayout`.
    pub fn on_did_request_layout(&self) -> Event<()> {
        self.on_did_request_layout.event()
    }

    /// Define como o renderizador nativo mede a largura de um item.
    pub fn set_item_measurer(&self, measurer: ItemMeasurer<T>) {
        *self.measurer.borrow_mut() = measurer;
    }

    /// As linhas visíveis (os rótulos de `_list.element(i).label`), na ordem.
    pub fn visible_labels(&self) -> Vec<Option<String>> {
        self.state.borrow().visible_menu_items.iter().map(|item| item.label.clone()).collect()
    }

    /// `_keyboardNavigation`.
    pub fn keyboard_navigation(&self) -> Option<bool> {
        self.state.borrow().keyboard_navigation
    }

    /// O índice da linha selecionada (`_list.getSelection()[0]`).
    pub fn selection(&self) -> Option<usize> {
        self.state.borrow().selection
    }

    /// O texto do filtro (`_filterText`).
    pub fn filter_text(&self) -> String {
        self.state.borrow().filter_text.clone()
    }

    /// O painel do submenu está aberto, e com quais rótulos de linha (`.action-list-submenu-panel .monaco-list-row.action`).
    pub fn submenu_rows(&self) -> Option<Vec<Option<String>>> {
        let widget = self.state.borrow().current_submenu_widget.clone()?;
        Some(widget.visible_labels())
    }

    /// O widget do submenu aberto.
    pub fn submenu_widget(&self) -> Option<Rc<ActionListWidget<ActionRef>>> {
        self.state.borrow().current_submenu_widget.clone()
    }

    /// `this.domNode.isConnected`: a lista já está dentro de um widget.
    fn is_connected(&self) -> bool {
        self.dom_node.parent_element().is_some()
    }

    // #endregion

    // #region lista (o List do VS Code)

    fn element(&self, index: usize) -> Option<Rc<IActionListItem<T>>> {
        self.state.borrow().visible_menu_items.get(index).cloned()
    }

    fn list_length(&self) -> usize {
        self.state.borrow().visible_menu_items.len()
    }

    fn list_index_of(&self, item: &Rc<IActionListItem<T>>) -> Option<usize> {
        self.state.borrow().visible_menu_items.iter().position(|candidate| Rc::ptr_eq(candidate, item))
    }

    /// `_list.setFocus([index])` e `setFocus([])`: dispara `onDidChangeFocus` sempre, como o `Trait.set`.
    fn set_focus(&self, index: Option<usize>) {
        self.state.borrow_mut().focus = index;
        self.on_focus();
    }

    /// `_list.domFocus()`.
    fn dom_focus_list(&self) {
        if let Some(input) = &self.filter_input {
            input.blur();
        }
        self.focus_state.list_focused.set(true);
    }

    /// `findNextIndex(index, loop, filter)`.
    fn find_next_index(&self, mut index: usize, loop_: bool, filter: &dyn Fn(&IActionListItem<T>) -> bool) -> Option<usize> {
        let length = self.list_length();
        for _ in 0..length {
            if index >= length && !loop_ {
                return None;
            }
            index %= length;
            if self.element(index).is_some_and(|element| filter(&element)) {
                return Some(index);
            }
            index += 1;
        }
        None
    }

    /// `findPreviousIndex(index, loop, filter)`.
    fn find_previous_index(&self, mut index: i64, loop_: bool, filter: &dyn Fn(&IActionListItem<T>) -> bool) -> Option<usize> {
        let length = self.list_length() as i64;
        for _ in 0..length {
            if index < 0 && !loop_ {
                return None;
            }
            index = (length + (index % length)) % length;
            if self.element(index as usize).is_some_and(|element| filter(&element)) {
                return Some(index as usize);
            }
            index -= 1;
        }
        None
    }

    fn list_focus_next(&self, n: usize, loop_: bool) {
        if self.list_length() == 0 {
            return;
        }
        let start = self.state.borrow().focus.map_or(0, |focus| focus + n);
        if let Some(index) = self.find_next_index(start, loop_, &Self::focus_condition) {
            self.set_focus(Some(index));
        }
    }

    fn list_focus_previous(&self, n: usize, loop_: bool) {
        if self.list_length() == 0 {
            return;
        }
        let start = self.state.borrow().focus.map_or(0, |focus| focus as i64 - n as i64);
        if let Some(index) = self.find_previous_index(start, loop_, &Self::focus_condition) {
            self.set_focus(Some(index));
        }
    }

    fn list_focus_first(&self) {
        if self.list_length() == 0 {
            return;
        }
        if let Some(index) = self.find_next_index(0, false, &Self::focus_condition) {
            self.set_focus(Some(index));
        }
    }

    fn list_focus_last(&self) {
        let length = self.list_length();
        if length == 0 {
            return;
        }
        if let Some(index) = self.find_next_index(length - 1, false, &Self::focus_condition) {
            self.set_focus(Some(index));
        }
    }

    fn focus_condition(element: &IActionListItem<T>) -> bool {
        !element.disabled && element.kind == ActionListItemKind::Action
    }

    // #endregion

    fn set_keyboard_navigation(&self, keyboard_navigation: Option<bool>) {
        self.state.borrow_mut().keyboard_navigation = keyboard_navigation;
    }

    // #region filtro

    fn apply_or_update_filter(&self) {
        if !self.delegate.handles_filter() {
            self.apply_filter(false, true, None);
            return;
        }

        let filter_text = self.state.borrow().filter_text.clone();
        let previous = self.state.borrow_mut().filter_request.take();
        if let Some(previous) = previous {
            previous.cancel();
        }
        let request = Rc::new(FilterRequest::default());
        self.state.borrow_mut().filter_request = Some(request.clone());
        let weak = self.this.clone();
        let for_result = request.clone();
        let for_finally = request.clone();
        let promise = self.delegate.on_filter(&filter_text, &request);
        promise
            .then_result(move |result| {
                if for_result.is_cancellation_requested() {
                    return Ok(());
                }
                if let (Some(widget), Ok(items)) = (weak.upgrade(), result) {
                    widget.state.borrow_mut().all_menu_items = items;
                    widget.apply_filter(true, true, None);
                }
                Ok(())
            })
            .finally({
                let weak = self.this.clone();
                move || {
                    // Retain cancelled requests so an interrupted IME search can restart with unchanged text.
                    let Some(widget) = weak.upgrade() else { return };
                    let is_current = widget.state.borrow().filter_request.as_ref().is_some_and(|current| Rc::ptr_eq(current, &for_finally));
                    if is_current && !for_finally.is_cancellation_requested() {
                        widget.state.borrow_mut().filter_request = None;
                    }
                }
            });
    }

    fn matches_filter(item: &IActionListItem<T>, filter_lower: &str) -> bool {
        let label = item.label.clone().unwrap_or_default().to_lowercase();
        let description = item.description.clone().unwrap_or_default().to_lowercase();
        label.contains(filter_lower)
            || description.contains(filter_lower)
            || (item.toolbar_labels && item.toolbar_actions.as_ref().is_some_and(|actions| actions.iter().any(|action| action.label().to_lowercase().contains(filter_lower))))
    }

    fn apply_filter(&self, skip_text_filter: bool, fire_layout: bool, focus_item_id: Option<String>) {
        let (all_menu_items, collapsed_sections, filter_text, focused_item) = {
            let state = self.state.borrow();
            let focused_item = state.focus.and_then(|index| state.visible_menu_items.get(index).cloned());
            (state.all_menu_items.clone(), state.collapsed_sections.clone(), state.filter_text.clone(), focused_item)
        };
        let filter_lower = if skip_text_filter { String::new() } else { filter_text.to_lowercase() };
        let is_filtering = !skip_text_filter && !filter_lower.is_empty();
        let mut visible: Vec<Rc<IActionListItem<T>>> = Vec::new();

        if is_filtering {
            let mut pending_separator: Option<Rc<IActionListItem<T>>> = None;
            let mut filtered_section_items: Vec<Rc<IActionListItem<T>>> = Vec::new();
            let mut has_matching_action_in_section = false;

            let flush = |visible: &mut Vec<Rc<IActionListItem<T>>>,
                         pending_separator: &mut Option<Rc<IActionListItem<T>>>,
                         filtered_section_items: &mut Vec<Rc<IActionListItem<T>>>,
                         has_matching_action_in_section: &mut bool| {
                if let Some(separator) = pending_separator.take() {
                    if *has_matching_action_in_section {
                        visible.push(separator);
                    }
                }
                visible.append(filtered_section_items);
                *has_matching_action_in_section = false;
            };

            for item in &all_menu_items {
                if item.kind == ActionListItemKind::Header {
                    continue;
                }

                if item.kind == ActionListItemKind::Separator {
                    flush(&mut visible, &mut pending_separator, &mut filtered_section_items, &mut has_matching_action_in_section);
                    pending_separator = if item.label.as_deref().is_some_and(|label| !label.is_empty()) { Some(item.clone()) } else { None };
                    continue;
                }

                if item.show_always {
                    filtered_section_items.push(item.clone());
                    continue;
                }

                if item.is_section_toggle {
                    continue;
                }

                if let Some(filter_items) = &item.filter_items {
                    let matching: Vec<Rc<IActionListItem<T>>> = if Self::matches_filter(item, &filter_lower) {
                        filter_items.clone()
                    } else {
                        filter_items.iter().filter(|candidate| Self::matches_filter(candidate, &filter_lower)).cloned().collect()
                    };
                    if !matching.is_empty() {
                        has_matching_action_in_section = true;
                        filtered_section_items.extend(matching);
                    }
                    continue;
                }

                if Self::matches_filter(item, &filter_lower) {
                    has_matching_action_in_section = true;
                    filtered_section_items.push(item.clone());
                }
            }

            flush(&mut visible, &mut pending_separator, &mut filtered_section_items, &mut has_matching_action_in_section);
        } else {
            for item in &all_menu_items {
                if item.kind == ActionListItemKind::Header {
                    visible.push(item.clone());
                    continue;
                }

                if item.kind == ActionListItemKind::Separator {
                    if item.section.as_ref().is_some_and(|section| collapsed_sections.contains(section)) {
                        continue;
                    }
                    visible.push(item.clone());
                    continue;
                }

                // Update icon for section toggle items based on collapsed state
                if item.is_section_toggle {
                    if let Some(section) = &item.section {
                        let collapsed = collapsed_sections.contains(section);
                        let mut toggle = (**item).clone();
                        let mut group = toggle.group.clone().unwrap_or_default();
                        group.icon = Some(if collapsed { "chevronRight" } else { "chevronDown" }.to_owned());
                        toggle.group = Some(group);
                        visible.push(Rc::new(toggle));
                        continue;
                    }
                }
                // Not filtering - check collapsed sections
                if item.section.as_ref().is_some_and(|section| collapsed_sections.contains(section)) {
                    continue;
                }
                visible.push(item.clone());
            }
        }

        // Remove orphaned separators while keeping labeled separators that act as section headers above their following
        // action items.
        let mut has_action_before = vec![false; visible.len()];
        let mut seen_action = false;
        for (index, item) in visible.iter().enumerate() {
            has_action_before[index] = seen_action;
            if item.kind == ActionListItemKind::Action {
                seen_action = true;
            }
        }

        let mut has_action_before_next_separator = vec![false; visible.len()];
        let mut seen_action_in_section = false;
        for index in (0..visible.len()).rev() {
            if visible[index].kind == ActionListItemKind::Action {
                seen_action_in_section = true;
                continue;
            }
            if visible[index].kind != ActionListItemKind::Separator {
                continue;
            }
            has_action_before_next_separator[index] = seen_action_in_section;
            seen_action_in_section = false;
        }

        for index in (0..visible.len()).rev() {
            let item = visible[index].clone();
            if item.kind != ActionListItemKind::Separator {
                continue;
            }
            let has_following_action_in_section = has_action_before_next_separator[index];
            let is_leading_unlabeled_divider = item.label.as_deref().is_none_or(str::is_empty) && !has_action_before[index];
            if !has_following_action_in_section || is_leading_unlabeled_divider {
                visible.remove(index);
            }
        }

        // Recompute group title positions based on visible items
        if self.opts().is_some_and(|o| o.show_group_title_on_first_item) {
            self.recompute_group_titles(&visible);
        }

        // Capture whether the filter input currently has focus before the list is replaced.
        let filter_input_has_focus = self.filter_input.as_ref().is_some_and(|input| input.is_focused());
        // Focus is only ours to restore if the list had it.
        let list_has_focus = self.focus_state.list_focused.get();

        {
            let mut state = self.state.borrow_mut();
            state.visible_menu_items = visible;
            // `_list.splice(0, length, visible)`: the traits of the replaced rows are gone.
            state.focus = None;
            state.selection = None;
        }

        // Notify the parent that a re-layout is needed
        if fire_layout {
            self.on_did_request_layout.fire(());
        }

        let has_laid_out = self.state.borrow().has_laid_out;
        // Restore focus after the rows were replaced, otherwise the blur handler in ActionWidgetService closes the widget.
        // Keep focus on the filter input if the user is typing a filter.
        if filter_input_has_focus {
            if let Some(input) = &self.filter_input {
                input.focus();
            }
            // Keep a highlighted item in the list so Enter works without pressing DownArrow first
            if !self.restore_item_focus(&focus_item_id, &focused_item) {
                self.focus_checked_or_first();
            }
        } else if has_laid_out && (focused_item.is_some() || focus_item_id.is_some()) {
            // Restore focus to the previously focused item
            let restored = self.restore_item_focus(&focus_item_id, &focused_item);
            if list_has_focus {
                if !restored {
                    // The focused row or its toolbar may have been removed by the update.
                    self.focus_checked_or_first();
                }
                self.dom_focus_list();
            }
        }
    }

    fn restore_item_focus(&self, focus_item_id: &Option<String>, focused_item: &Option<Rc<IActionListItem<T>>>) -> bool {
        let item_id = focus_item_id.clone().or_else(|| focused_item.as_ref().and_then(|item| item.item_id()));
        let Some(item_id) = item_id else {
            return false;
        };
        for index in 0..self.list_length() {
            if self.element(index).is_some_and(|element| element.item_id().as_deref() == Some(item_id.as_str())) {
                self.set_focus(Some(index));
                return true;
            }
        }
        false
    }

    fn recompute_group_titles(&self, items: &[Rc<IActionListItem<T>>]) {
        let mut titles = BTreeMap::new();
        let mut seen_titles = BTreeSet::new();
        for (index, item) in items.iter().enumerate() {
            if item.kind != ActionListItemKind::Action {
                continue;
            }
            if let Some(group) = &item.group {
                if !group.title.is_empty() && seen_titles.insert(group.title.clone()) {
                    titles.insert(index, group.title.clone());
                }
            }
        }
        self.state.borrow_mut().group_title_by_index = titles;
    }

    /// `_groupTitleByIndex`.
    pub fn group_title_by_index(&self) -> BTreeMap<usize, String> {
        self.state.borrow().group_title_by_index.clone()
    }

    fn toggle_section(&self, section: &str) {
        let expanding = self.state.borrow().collapsed_sections.contains(section);
        if expanding {
            self.state.borrow_mut().collapsed_sections.remove(section);
        } else {
            self.state.borrow_mut().collapsed_sections.insert(section.to_owned());
            let focused_item = self.get_focused_element();
            if let Some(focused_item) = focused_item {
                if focused_item.section.as_deref() == Some(section) && !focused_item.is_section_toggle {
                    let toggle_index = self
                        .state
                        .borrow()
                        .visible_menu_items
                        .iter()
                        .position(|item| item.section.as_deref() == Some(section) && item.is_section_toggle);
                    if let Some(toggle_index) = toggle_index {
                        self.set_focus(Some(toggle_index));
                    }
                }
            }
        }
        let collapsed = self.state.borrow().collapsed_sections.contains(section);
        if let Some(on_did_toggle_section) = self.opts().and_then(|o| o.on_did_toggle_section.clone()) {
            on_did_toggle_section(section, collapsed);
        }
        self.apply_filter(false, true, None);
    }

    // #endregion

    // #region foco

    fn focus_checked_or_first(&self) {
        let initial_focus_item_id = self.state.borrow_mut().initial_focus_item_id.take();
        if let Some(initial_focus_item_id) = initial_focus_item_id {
            for index in 0..self.list_length() {
                let Some(element) = self.element(index) else { continue };
                if Self::focus_condition(&element) && element.item_id().as_deref() == Some(initial_focus_item_id.as_str()) {
                    self.set_focus(Some(index));
                    return;
                }
            }
        }
        let focused_index = self.state.borrow().focus;
        if let Some(focused_index) = focused_index {
            if self.element(focused_index).is_some_and(|element| Self::focus_condition(&element)) {
                return;
            }
        }
        // Try to focus the checked item first
        for index in 0..self.list_length() {
            let Some(element) = self.element(index) else { continue };
            if Self::focus_condition(&element) && element.item.as_ref().is_some_and(ActionListItemData::list_item_checked) {
                self.set_focus(Some(index));
                return;
            }
        }
        // Set focus on the first focusable item without moving DOM focus
        self.list_focus_first();
    }

    /// `focus()`.
    pub fn focus(&self) {
        self.set_keyboard_navigation(None);
        if let Some(input) = &self.filter_input {
            if self.opts().is_some_and(|o| o.focus_filter_on_open) {
                input.focus();
                // Highlight the first item so Enter works immediately
                self.focus_checked_or_first();
                return;
            }
        }
        self.dom_focus_list();
        self.focus_checked_or_first();
    }

    /// `clearFocus()`.
    pub fn clear_focus(&self) {
        self.set_focus(None);
    }

    /// `getFocusedElement()`.
    pub fn get_focused_element(&self) -> Option<Rc<IActionListItem<T>>> {
        let state = self.state.borrow();
        state.focus.and_then(|index| state.visible_menu_items.get(index).cloned())
    }

    /// `focusItemById(itemId)`: foca já e de novo depois do evento corrente, para o tratamento de ponteiro da lista não
    /// desfazer o foco.
    pub fn focus_item_by_id(&self, item_id: &str) {
        self.focus_item(item_id);
        let weak = self.this.clone();
        let item_id = item_id.to_owned();
        queue_microtask(move || {
            if let Some(widget) = weak.upgrade() {
                if widget.is_connected() {
                    widget.focus_item(&item_id);
                }
            }
        });
    }

    fn focus_item(&self, item_id: &str) {
        for index in 0..self.list_length() {
            if self.element(index).is_some_and(|element| element.item_id().as_deref() == Some(item_id)) {
                self.set_focus(Some(index));
                self.dom_focus_list();
                break;
            }
        }
    }

    /// `focusPrevious()`.
    pub fn focus_previous(&self) {
        self.set_keyboard_navigation(Some(true));
        if self.focus_filter_result(false) {
            return;
        }
        if let Some(input) = self.filter_input.clone() {
            if input.is_focused() {
                self.dom_focus_list();
                // An item is already highlighted; advance from it instead of jumping to last
                let current = self.state.borrow().focus;
                if let Some(current) = current {
                    self.list_focus_previous(1, false);
                    let focused = self.state.borrow().focus;
                    // If we couldn't move (already at first), go to filter
                    if focused.is_some_and(|focused| focused >= current) {
                        input.focus();
                    }
                } else {
                    self.list_focus_last();
                }
                return;
            }
        }
        let previous_focus = self.state.borrow().focus;
        self.list_focus_previous(1, true);
        let focused = self.state.borrow().focus;
        if let Some(focused) = focused {
            // If focus wrapped (was at first focusable, now at last), move to filter instead
            if let (Some(input), Some(previous_focus)) = (&self.filter_input, previous_focus) {
                if focused > previous_focus {
                    self.set_focus(None);
                    input.focus();
                }
            }
        }
    }

    /// `focusNext()`.
    pub fn focus_next(&self) {
        self.set_keyboard_navigation(Some(true));
        if self.focus_filter_result(true) {
            return;
        }
        if let Some(input) = self.filter_input.clone() {
            if input.is_focused() {
                self.dom_focus_list();
                // An item is already highlighted; advance from it instead of jumping to first
                let current = self.state.borrow().focus;
                if current.is_some() {
                    self.list_focus_next(1, false);
                } else {
                    self.list_focus_first();
                }
                return;
            }
        }
        let previous_focus = self.state.borrow().focus;
        self.list_focus_next(1, true);
        let focused = self.state.borrow().focus;
        if let Some(focused) = focused {
            // If focus wrapped (was at last focusable, now at first), move to filter instead
            if let (Some(input), Some(previous_focus)) = (&self.filter_input, previous_focus) {
                if focused < previous_focus {
                    self.set_focus(None);
                    input.focus();
                }
            }
        }
    }

    fn focus_filter_result(&self, next: bool) -> bool {
        let Some(input) = &self.filter_input else {
            return false;
        };
        if !self.opts().is_some_and(|o| o.filter_as_combobox) || !input.is_focused() {
            return false;
        }
        if next {
            self.list_focus_next(1, true);
        } else {
            self.list_focus_previous(1, true);
        }
        true
    }

    /// The focused row's section, if that section has a toggle that can expand it again.
    fn get_focused_section(&self) -> Option<String> {
        let section = self.get_focused_element()?.section.clone()?;
        let has_toggle = self.state.borrow().all_menu_items.iter().any(|item| item.section.as_deref() == Some(section.as_str()) && item.is_section_toggle);
        if has_toggle { Some(section) } else { None }
    }

    /// `collapseFocusedSection()`: collapses the section containing the focused row, returning whether it was expanded.
    pub fn collapse_focused_section(&self) -> bool {
        if let Some(section) = self.get_focused_section() {
            if !self.state.borrow().collapsed_sections.contains(&section) {
                self.toggle_section(&section);
                return true;
            }
        }
        false
    }

    /// `expandFocusedSection()`: expands the section containing the focused row, returning whether it was collapsed.
    pub fn expand_focused_section(&self) -> bool {
        if let Some(section) = self.get_focused_section() {
            if self.state.borrow().collapsed_sections.contains(&section) {
                self.toggle_section(&section);
                return true;
            }
        }
        false
    }

    /// `toggleFocusedSection()`.
    pub fn toggle_focused_section(&self) -> bool {
        let Some(element) = self.get_focused_element() else {
            return false;
        };
        if element.is_section_toggle {
            if let Some(section) = &element.section {
                self.toggle_section(section);
                return true;
            }
        }
        false
    }

    // #endregion

    // #region aceitar e seleção

    /// `acceptSelected(preview)`.
    pub fn accept_selected(&self, preview: bool) {
        let Some(focus_index) = self.state.borrow().focus else {
            return;
        };
        let Some(element) = self.element(focus_index) else {
            return;
        };
        if !Self::focus_condition(&element) {
            return;
        }

        self.set_selection(Some(focus_index), if preview { SelectionEvent::Preview } else { SelectionEvent::Accept });
    }

    /// `_list.setSelection([index], event)` e `setSelection([])`: dispara `onDidChangeSelection`.
    fn set_selection(&self, index: Option<usize>, event: SelectionEvent) {
        self.state.borrow_mut().selection = index;
        if let Some(element) = index.and_then(|index| self.element(index)) {
            self.on_list_selection(element, event);
        }
    }

    /// Um clique ou toque na linha `index`, no `target` dela.
    pub fn click_row(&self, index: usize, target: PointerTarget) {
        self.set_selection(Some(index), SelectionEvent::Pointer(target));
    }

    fn on_list_selection(&self, element: Rc<IActionListItem<T>>, event: SelectionEvent) {
        if let Some(toggle) = &element.standalone_toggle {
            self.set_selection(None, SelectionEvent::Accept);
            let checked = {
                let mut state = self.state.borrow_mut();
                let position = state.standalone_toggles.iter().position(|(item, _)| Rc::ptr_eq(item, &element));
                let position = position.unwrap_or_else(|| {
                    state.standalone_toggles.push((element.clone(), toggle.checked));
                    state.standalone_toggles.len() - 1
                });
                if toggle.disabled {
                    None
                } else {
                    let checked = !state.standalone_toggles[position].1;
                    state.standalone_toggles[position].1 = checked;
                    Some(checked)
                }
            };
            if let Some(checked) = checked {
                (toggle.on_change)(checked);
            }
            return;
        }
        if element.is_section_toggle {
            if let Some(section) = element.section.clone() {
                self.set_selection(None, SelectionEvent::Accept);
                let weak = self.this.clone();
                queue_microtask(move || {
                    if let Some(widget) = weak.upgrade() {
                        widget.toggle_section(&section);
                    }
                });
                return;
            }
        }
        let is_pointer_activation = matches!(event, SelectionEvent::Pointer(_));
        if let SelectionEvent::Pointer(target) = event {
            if target != PointerTarget::Row {
                self.set_selection(None, SelectionEvent::Accept);
                return;
            }
        }
        if element.open_submenu_on_click && has_submenu_actions(&element) && (is_pointer_activation || event == SelectionEvent::Accept) {
            self.set_selection(None, SelectionEvent::Accept);
            self.show_submenu_for_item(&element);
            if !is_pointer_activation {
                self.focus_submenu_widget();
            }
            return;
        }
        if let (Some(item), true) = (&element.item, Self::focus_condition(&element)) {
            let is_preview_event = event == SelectionEvent::Preview;
            self.delegate.on_select(item, is_preview_event && self.supports_preview);
        } else {
            self.set_selection(None, SelectionEvent::Accept);
        }
    }

    fn on_focus(&self) {
        let Some(focus_index) = self.state.borrow().focus else {
            return;
        };
        let Some(element) = self.element(focus_index) else {
            return;
        };
        self.delegate.on_focus(element.item.as_ref());
    }

    // #endregion

    // #region teclado e filtro

    /// Um `keydown` em `target`: os tratadores do campo de filtro (nesta ordem, no próprio campo), depois os do `domNode`
    /// (ArrowRight que abre o submenu e a digitação que revela o filtro), subindo enquanto ninguém parar a propagação.
    pub fn dispatch_key_down(&self, e: &UiKeyEvent, target: KeyTarget) {
        match target {
            KeyTarget::FilterInput => self.filter_input_key_down(e),
            KeyTarget::SubmenuPanel => self.submenu_panel_key_down(e),
            KeyTarget::List => {}
        }
        // `stopPropagation()` no campo ou no painel impede o evento de subir até o `domNode`.
        if e.propagation_stopped() {
            return;
        }
        self.dom_node_key_down(e, target);
    }

    fn filter_input_key_down(&self, e: &UiKeyEvent) {
        if self.filter_input.is_none() {
            return;
        }
        self.set_keyboard_navigation(Some(true));
        if !e.is_composing && self.is_submenu_panel.get() {
            // Typing in a submenu filter must not reach the parent list (the keybinding branch of softDispatch is part of
            // the ActionList slice).
            if e.key.chars().count() == 1 && !e.ctrl_key && !e.meta_key && !e.alt_key {
                e.stop_propagation();
            }
        }
        if !self.opts().is_some_and(|o| o.filter_as_combobox) {
            return;
        }
        let ime_session_in_progress = self.state.borrow().ime_session_in_progress;
        let is_composing = ime_session_in_progress || e.is_composing || e.key_code == KeyCode::KeyInComposition;
        if is_composing || e.key_code == KeyCode::LeftArrow || e.key_code == KeyCode::RightArrow || e.key_code == KeyCode::Home || e.key_code == KeyCode::End {
            e.stop_propagation();
            return;
        }
        let is_navigation = e.key_code == KeyCode::UpArrow || e.key_code == KeyCode::DownArrow;
        if !is_navigation && e.key_code != KeyCode::Enter {
            return;
        }
        if e.ctrl_key || e.meta_key || e.alt_key || e.shift_key {
            if is_navigation {
                e.stop_propagation();
            }
            return;
        }
        e.stop();
        if e.key_code == KeyCode::UpArrow {
            self.focus_previous();
        } else if e.key_code == KeyCode::DownArrow {
            self.focus_next();
        } else {
            self.accept_selected(false);
        }
    }

    fn dom_node_key_down(&self, e: &UiKeyEvent, target: KeyTarget) {
        // ArrowRight opens submenu for the focused item and moves focus into it
        if e.key == "ArrowRight" && !e.is_composing && !e.alt_key && !e.ctrl_key && !e.meta_key && !e.shift_key {
            let focused = self.state.borrow().focus;
            if let Some(focus_index) = focused {
                if let Some(element) = self.element(focus_index) {
                    if has_submenu_actions(&element) {
                        e.stop();
                        self.show_submenu_for_element(&element);
                        self.focus_submenu_widget();
                    }
                }
            }
        }

        if self.filter_input.is_some() || self.opts().is_some_and(|o| o.on_type.is_some()) {
            let on_type = self.opts().is_some_and(|o| o.on_type.is_some());
            let combobox = self.opts().is_some_and(|o| o.filter_as_combobox);
            let is_editable_target = target == KeyTarget::FilterInput;
            if (on_type || combobox) && (is_editable_target || target == KeyTarget::SubmenuPanel) {
                return;
            }
            let filter_active = self.filter_input.as_ref().is_some_and(|input| input.is_focused());
            if !filter_active && !e.is_composing && e.key.chars().count() == 1 && e.key != " " && !e.ctrl_key && !e.meta_key && !e.alt_key {
                e.prevent_default();
                e.stop_propagation();
                if let Some(input) = &self.filter_input {
                    input.focus();
                    input.set_value(&e.key);
                    self.state.borrow_mut().filter_text = e.key.clone();
                    self.apply_or_update_filter();
                    if let Some(on_did_change_filter) = self.opts().and_then(|o| o.on_did_change_filter.clone()) {
                        on_did_change_filter(&e.key);
                    }
                } else if let Some(on_type) = self.opts().and_then(|o| o.on_type.clone()) {
                    on_type(&e.key);
                }
            }
        }
    }

    /// `onFilterValueChanged`: while an IME composition is running the input holds intermediate text which must not drive
    /// the filter; filter once the composition commits instead.
    fn on_filter_value_changed(&self) {
        let Some(input) = &self.filter_input else {
            return;
        };
        let value = input.value();
        let (ime_session_in_progress, same_text, current_cancelled) = {
            let state = self.state.borrow();
            (
                state.ime_session_in_progress,
                value == state.filter_text,
                state.filter_request.as_ref().is_some_and(|request| request.is_cancellation_requested()),
            )
        };
        // Restart cancelled compositions without duplicating the trailing input event's live request.
        if ime_session_in_progress || (same_text && !current_cancelled) {
            return;
        }
        self.state.borrow_mut().filter_text = value.clone();
        self.apply_or_update_filter();
        if let Some(on_did_change_filter) = self.opts().and_then(|o| o.on_did_change_filter.clone()) {
            on_did_change_filter(&value);
        }
    }

    /// O usuário digitou no campo: o evento `input`.
    pub fn filter_input_input(&self, value: &str) {
        if let Some(input) = &self.filter_input {
            input.set_value(value);
        }
        self.on_filter_value_changed();
    }

    /// O evento `compositionstart` do campo.
    pub fn filter_input_composition_start(&self) {
        let request = {
            let mut state = self.state.borrow_mut();
            state.ime_session_in_progress = true;
            state.filter_request.clone()
        };
        // A dynamic filter request issued for the previous value can still be in flight; the committed value starts a fresh
        // request from `compositionend`.
        if let Some(request) = request {
            request.cancel();
        }
    }

    /// O evento `compositionend` do campo.
    pub fn filter_input_composition_end(&self) {
        self.state.borrow_mut().ime_session_in_progress = false;
        self.on_filter_value_changed();
    }

    /// `setFilter(value, focusItemId)`: update an open searchable list without closing its owning picker.
    pub fn set_filter(&self, value: &str, focus_item_id: Option<&str>) {
        let Some(input) = self.filter_input.clone() else {
            return;
        };
        input.set_value(value);
        self.state.borrow_mut().filter_text = value.to_owned();
        self.apply_or_update_filter();
        if let Some(focus_item_id) = focus_item_id.filter(|id| !id.is_empty()) {
            self.focus_item_by_id(focus_item_id);
        }
        input.focus();
    }

    /// `clearFilter()`.
    pub fn clear_filter(&self) -> bool {
        if let Some(input) = &self.filter_input {
            if !self.state.borrow().filter_text.is_empty() {
                input.set_value("");
                self.state.borrow_mut().filter_text = String::new();
                self.apply_or_update_filter();
                return true;
            }
        }
        false
    }

    // #endregion

    // #region itens

    /// `updateItems(items, focusItemId, options)`: replaces the items in place, preserving the current filter, without
    /// closing the widget.
    pub fn update_items(&self, items: &[Rc<IActionListItem<T>>], focus_item_id: Option<&str>, options: Option<IActionListUpdateOptions>) {
        let options = options.unwrap_or_default();
        let pending = self.state.borrow().pending_submenu_element.clone();
        if let Some(pending) = pending {
            let pending_id = pending.item_id();
            let pending_item = match (&pending_id, options.preserve_hover) {
                (Some(pending_id), true) => items.iter().find(|item| item.item_id().as_deref() == Some(pending_id.as_str())).cloned(),
                _ => None,
            };
            match pending_item {
                Some(pending_item) => self.state.borrow_mut().pending_submenu_element = Some(pending_item),
                None => self.cancel_submenu_show(),
            }
        }
        let expanded_item_id = self.state.borrow().current_submenu_element.as_ref().and_then(|item| item.item_id());
        let preserved_item = match (&expanded_item_id, options.preserve_hover) {
            (Some(expanded_item_id), true) => {
                let wanted = focus_item_id.map_or_else(|| expanded_item_id.clone(), str::to_owned);
                items.iter().find(|item| item.item_id().as_deref() == Some(wanted.as_str())).cloned()
            }
            _ => None,
        };
        let has_submenu_widget = self.state.borrow().current_submenu_widget.is_some();
        let preserve_submenu = has_submenu_widget && preserved_item.as_ref().is_some_and(|item| has_submenu_actions(item));
        // Sem o painel de hover (ver a nota do módulo), só o submenu se preserva.
        let preserve_panel = preserve_submenu;

        self.state.borrow_mut().all_menu_items = items.to_vec();
        if let (true, Some(preserved_item)) = (preserve_panel, &preserved_item) {
            let mut state = self.state.borrow_mut();
            state.current_submenu_element = Some(preserved_item.clone());
            if let Some(section) = &preserved_item.section {
                state.collapsed_sections.remove(section);
            }
        } else {
            let persistent_hover = self.opts().is_some_and(|o| o.persistent_hover);
            let (has_current, current_still_listed) = {
                let state = self.state.borrow();
                (
                    state.current_submenu_element.is_some(),
                    state.current_submenu_element.as_ref().is_some_and(|current| state.all_menu_items.iter().any(|item| Rc::ptr_eq(item, current))),
                )
            };
            if (options.preserve_hover || persistent_hover) && has_current && !current_still_listed {
                self.hide_submenu();
            }
        }
        let previous_visible_count = self.list_length();
        // An explicitly requested focusItemId (e.g. refocusing a row a caller just toggled) is honored unconditionally; only
        // the fallback to the previously expanded submenu's item is gated on actually preserving that panel.
        let focus_for_filter = focus_item_id.map(str::to_owned).or(if preserve_panel { expanded_item_id } else { None });
        self.apply_filter(false, false, focus_for_filter);
        if let (true, Some(preserved_item)) = (preserve_submenu, &preserved_item) {
            let widget = self.state.borrow().current_submenu_widget.clone();
            if let (Some(widget), Some(actions)) = (widget, &preserved_item.submenu_actions) {
                widget.update_items(&Self::create_submenu_items(actions), None, None);
            }
        }
        if self.list_length() != previous_visible_count {
            self.on_did_request_layout.fire(());
        }
        if preserve_panel {
            self.layout_submenu();
        } else if let Some(focus_item_id) = focus_item_id {
            self.focus_item_by_id(focus_item_id);
        }
    }

    fn remove_item(&self, item: &Rc<IActionListItem<T>>) {
        let index = self.state.borrow().all_menu_items.iter().position(|candidate| Rc::ptr_eq(candidate, item));
        if let Some(index) = index {
            self.state.borrow_mut().all_menu_items.remove(index);
            self.apply_filter(false, true, None);
        }
    }

    /// O clique no botão de uma ação da barra da linha `row_label`: a ação `actionList.remove` espera o `onRemove` da linha e
    /// depois a tira da lista. Devolve a promessa dessa ação.
    pub fn run_item_toolbar_action(&self, row_label: &str, action_id: &str) -> Option<Promise<()>> {
        let element = self.state.borrow().visible_menu_items.iter().find(|item| item.label.as_deref() == Some(row_label)).cloned()?;
        if action_id == REMOVE_TOOLBAR_ACTION_ID {
            let on_remove = element.on_remove.clone()?;
            let weak = self.this.clone();
            let removed = element.clone();
            return Some(on_remove().then(move |_| {
                if let Some(widget) = weak.upgrade() {
                    widget.remove_item(&removed);
                }
            }));
        }
        let action = element.toolbar_actions.as_ref()?.iter().find(|action| action.id() == action_id)?.clone();
        Some(action.run(&[]).then(|_| ()))
    }

    // #endregion

    // #region submenu

    fn show_submenu_for_item(&self, item: &Rc<IActionListItem<T>>) {
        if self.list_index_of(item).is_some() {
            self.show_submenu_for_element(item);
        }
    }

    fn focus_submenu_widget(&self) {
        let widget = self.state.borrow().current_submenu_widget.clone();
        if let Some(widget) = widget {
            // O foco do documento passa para o painel.
            if let Some(input) = &self.filter_input {
                input.blur();
            }
            self.focus_state.list_focused.set(false);
            widget.focus();
        }
    }

    fn show_submenu_for_element(&self, element: &Rc<IActionListItem<T>>) {
        let already_current = self.state.borrow().current_submenu_element.as_ref().is_some_and(|current| Rc::ptr_eq(current, element));
        if !self.state.borrow().hover_enabled || already_current {
            return;
        }
        // O painel só existe aqui para as ações de submenu; o painel de hover é da fatia do hover.
        let Some(submenu_actions) = element.submenu_actions.clone().filter(|actions| !actions.is_empty()) else {
            return;
        };

        if let Some(parent_widget) = self.dom_node.parent_element().and_then(|parent| parent.closest("action-widget")) {
            parent_widget.finish_opening_animation();
        }
        self.state.borrow_mut().current_submenu_element = Some(element.clone());
        self.clear_submenu_container();

        let submenu_items = Self::create_submenu_items(&submenu_actions);
        let delegate: Rc<dyn IActionListDelegate<ActionRef>> = Rc::new(SubmenuDelegate::<T> { parent: self.this.clone() });
        let widget = ActionListWidget::<ActionRef>::new(false, &submenu_items, delegate, element.submenu_options.clone());
        widget.is_submenu_panel.set(true);
        self.state.borrow_mut().current_submenu_widget = Some(widget.clone());

        // The submenu widget's constructor focuses its first item by default; clear that until the user actually navigates
        // into the submenu (via ArrowRight) so it doesn't render as if selected while the parent list still has focus.
        widget.clear_focus();

        let total_height = widget.compute_list_height();
        widget.layout(total_height, None);
        let max_width = widget.compute_max_width(0.0);
        widget.layout(total_height, Some(max_width));
        widget.dom_node.set_size(max_width, total_height);

        let parent = self.this.clone();
        let submenu = Rc::downgrade(&widget);
        let subscription = widget.on_did_request_layout().listen(move |_| {
            let Some(submenu) = submenu.upgrade() else { return };
            let total_height = submenu.compute_list_height();
            submenu.layout(total_height, Some(max_width));
            if let Some(parent) = parent.upgrade() {
                parent.layout_submenu();
            }
        });
        widget.attach_subscription(subscription);
        self.state.borrow_mut().has_layout_submenu = true;
    }

    /// Um objeto descartado junto com o widget (as assinaturas de eventos do painel).
    fn attach_subscription(&self, subscription: DisposableRef) {
        self.panel_subscriptions.add(subscription);
    }

    /// `_layoutSubmenu`: reposiciona o painel; some com ele se a linha que o abriu deixou a lista.
    fn layout_submenu(&self) {
        let (has_layout, current) = {
            let state = self.state.borrow();
            (state.has_layout_submenu, state.current_submenu_element.clone())
        };
        if !has_layout {
            return;
        }
        let Some(current) = current else {
            return;
        };
        // Width measurement and virtualization can replace or recycle the original row.
        let row_exists = self.list_index_of(&current).is_some();
        let persistent = self.opts().is_some_and(|o| o.persistent_hover);
        let still_listed = self.state.borrow().all_menu_items.iter().any(|item| Rc::ptr_eq(item, &current));
        if !row_exists && (!persistent || !still_listed) {
            self.hide_submenu();
        }
    }

    /// `_createSubmenuItems(submenuActions)`.
    fn create_submenu_items(submenu_actions: &[ActionRef]) -> Vec<Rc<IActionListItem<ActionRef>>> {
        let submenu_item = |action: &ActionRef, submenu: Option<Vec<ActionRef>>| {
            let tooltip = action.tooltip();
            let label = action.label();
            let mut item = IActionListItem::<ActionRef>::action(&label, action.clone());
            item.description = if !tooltip.is_empty() && tooltip != label { Some(tooltip.clone()) } else { None };
            item.group = Some(ActionListGroup {
                title: String::new(),
                icon: Some(if action.checked().unwrap_or(false) { "check" } else { "blank" }.to_owned()),
            });
            item.tooltip = Some(if tooltip.is_empty() { label } else { tooltip });
            item.on_remove = on_remove_of(action);
            item.submenu_actions = submenu;
            Rc::new(item)
        };

        let mut submenu_items = Vec::new();
        let submenu_groups: Vec<&SubmenuAction> = submenu_actions.iter().filter_map(|action| action.as_any().downcast_ref::<SubmenuAction>()).collect();
        let groups_with_actions: Vec<&SubmenuAction> = submenu_groups.into_iter().filter(|group| !group.actions().is_empty()).collect();
        for (group_index, group) in groups_with_actions.iter().enumerate() {
            let group_label = IAction::label(*group);
            if !group_label.is_empty() {
                let mut header = IActionListItem::<ActionRef>::new(ActionListItemKind::Header);
                header.group = Some(ActionListGroup { icon: None, title: group_label.clone() });
                header.label = Some(group_label);
                submenu_items.push(Rc::new(header));
            }
            for child in group.actions() {
                let nested = child
                    .as_any()
                    .downcast_ref::<SubmenuAction>()
                    .map(|nested| vec![SubmenuAction::new(&nested.id(), "", nested.actions().to_vec(), None) as ActionRef]);
                submenu_items.push(submenu_item(child, nested));
            }
            if group_index < groups_with_actions.len() - 1 {
                let mut separator = IActionListItem::<ActionRef>::new(ActionListItemKind::Separator);
                separator.label = Some(String::new());
                submenu_items.push(Rc::new(separator));
            }
        }
        for action in submenu_actions {
            if action.as_any().downcast_ref::<SubmenuAction>().is_none() {
                submenu_items.push(submenu_item(action, None));
            }
        }
        submenu_items
    }

    fn hide_submenu(&self) {
        self.cancel_submenu_show();
        self.state.borrow_mut().current_submenu_element = None;
        self.clear_submenu_container();
    }

    /// `_clearSubmenuContainer()`: if focus currently lives inside the panel, it is first moved back to the list.
    fn clear_submenu_container(&self) {
        self.cancel_submenu_show();
        let widget = {
            let mut state = self.state.borrow_mut();
            state.has_layout_submenu = false;
            state.current_submenu_widget.take()
        };
        if let Some(widget) = widget {
            if widget.has_focus() {
                self.dom_focus_list();
            }
            widget.dispose();
        }
    }

    fn cancel_submenu_show(&self) {
        self.state.borrow_mut().pending_submenu_element = None;
    }

    fn has_focus(&self) -> bool {
        self.focus_state.list_focused.get() || self.filter_input.as_ref().is_some_and(|input| input.is_focused())
    }

    /// Os tratadores do painel (`keydown` do `_submenuContainer`): ArrowLeft e Escape voltam à lista; Enter, Espaço e as
    /// setas agem sobre o submenu.
    fn submenu_panel_key_down(&self, e: &UiKeyEvent) {
        if e.key == "ArrowLeft" || e.key == "Escape" {
            e.stop();
            let dismiss_widget = e.key == "Escape" && self.state.borrow().current_submenu_widget.is_some();
            self.hide_submenu();
            if dismiss_widget {
                self.hide(None);
                return;
            }
            self.set_keyboard_navigation(Some(true));
            self.dom_focus_list();
            return;
        }
        let widget = self.state.borrow().current_submenu_widget.clone();
        let Some(widget) = widget else {
            return;
        };
        match e.key.as_str() {
            "Enter" | " " => {
                e.stop();
                widget.accept_selected(false);
            }
            "ArrowDown" => {
                e.stop();
                widget.focus_next();
            }
            "ArrowUp" => {
                e.stop();
                widget.focus_previous();
            }
            _ => {}
        }
    }

    /// `onSelect` do delegate do submenu depois que a ação rodou.
    fn on_submenu_selected(&self) {
        let parent_item = self.state.borrow().current_submenu_element.as_ref().and_then(|element| element.item.clone());
        self.hide_submenu();
        if let Some(parent_item) = parent_item {
            self.delegate.on_select(&parent_item, false);
        }
        self.hide(None);
    }

    /// `setHoverEnabled(enabled)`: com o hover desligado o painel some.
    pub fn set_hover_enabled(&self, enabled: bool) {
        if self.state.borrow().hover_enabled == enabled {
            return;
        }
        self.state.borrow_mut().hover_enabled = enabled;
        if !enabled {
            self.hide_submenu();
        }
    }

    // #endregion

    /// `hide(didCancel)`.
    pub fn hide(&self, did_cancel: Option<bool>) {
        self.delegate.on_hide(did_cancel);
        let request = self.state.borrow_mut().filter_request.take();
        if let Some(request) = request {
            request.cancel();
        }
        self.hide_submenu();
    }

    // #region altura e largura

    /// Whether this widget uses dynamic height (has filter or collapsible sections).
    pub fn has_dynamic_height(&self) -> bool {
        if self.opts().is_some_and(|o| o.show_filter) {
            return true;
        }
        self.state.borrow().all_menu_items.iter().any(|item| item.is_section_toggle)
    }

    /// The height of a single action row in pixels.
    pub fn line_height(&self) -> f64 {
        ACTION_LINE_HEIGHT
    }

    /// Row heights depend on the options of the tab the items belong to, not the shown one.
    fn item_height_with(item: &IActionListItem<T>, options: Option<&IActionListOptions>) -> f64 {
        let detail_height = options.and_then(|o| o.detail_item_height).unwrap_or(48.0);
        match item.kind {
            ActionListItemKind::Header => HEADER_LINE_HEIGHT,
            ActionListItemKind::Separator => {
                if item.detail.as_deref().is_some_and(|detail| !detail.is_empty()) {
                    detail_height
                } else if item.label.as_deref().is_some_and(|label| !label.is_empty()) {
                    ACTION_LINE_HEIGHT
                } else {
                    SEPARATOR_LINE_HEIGHT
                }
            }
            ActionListItemKind::Action => {
                if item.inline_toggle.is_some() {
                    return options.and_then(|o| o.inline_toggle_item_height).unwrap_or(70.0);
                }
                if item.detail.as_deref().is_some_and(|detail| !detail.is_empty()) { detail_height } else { ACTION_LINE_HEIGHT }
            }
        }
    }

    /// Computes the total height of all items (including collapsed/filtered items).
    pub fn compute_full_height(&self) -> f64 {
        let items = self.state.borrow().all_menu_items.clone();
        self.compute_height_for_items(&items, None, self.opts())
    }

    /// Height the list would need for `items`, leaving out anything in a collapsed section.
    pub fn compute_height_for_items(
        &self,
        items: &[Rc<IActionListItem<T>>],
        collapsed_sections: Option<&BTreeSet<String>>,
        options: Option<&IActionListOptions>,
    ) -> f64 {
        let mut height = 0.0;
        for item in items {
            // The toggle is the row that reopens its own section, so it stays on screen.
            if let (Some(collapsed_sections), Some(section)) = (collapsed_sections, &item.section) {
                if !item.is_section_toggle && collapsed_sections.contains(section) {
                    continue;
                }
            }
            height += Self::item_height_with(item, options);
        }
        height
    }

    /// Computes the total height of visible items in the list.
    pub fn compute_list_height(&self) -> f64 {
        self.state.borrow().visible_menu_items.iter().map(|item| Self::item_height_with(item, self.opts())).sum()
    }

    fn compute_max_visible_items_height_for(&self, items: &[Rc<IActionListItem<T>>]) -> f64 {
        let Some(max_visible_items) = self.opts().and_then(|o| o.max_visible_items) else {
            return f64::INFINITY;
        };
        let mut height = 0.0;
        let mut action_count = 0;
        for item in items {
            if action_count >= max_visible_items {
                break;
            }
            height += Self::item_height_with(item, self.opts());
            if item.kind == ActionListItemKind::Action {
                action_count += 1;
            }
        }
        height
    }

    /// Computes the height of the visible rows through the `maxVisibleItems`th action row, or infinity when the list is not
    /// capped.
    pub fn compute_max_visible_items_height(&self) -> f64 {
        let items = self.state.borrow().visible_menu_items.clone();
        self.compute_max_visible_items_height_for(&items)
    }

    /// Computes the full list height capped through the configured maximum number of visible action rows.
    pub fn compute_max_full_height(&self) -> f64 {
        let items = self.state.borrow().all_menu_items.clone();
        self.compute_full_height().min(self.compute_max_visible_items_height_for(&items))
    }

    /// `layout(height, width)`: lays out the list widget with the given explicit dimensions.
    pub fn layout(&self, height: f64, width: Option<f64>) {
        {
            let mut state = self.state.borrow_mut();
            state.has_laid_out = true;
        }
        let width = width.unwrap_or_else(|| self.dom_node.size().width);
        self.dom_node.set_size(width, height);
        self.dom_node.style_set_property("height", &format!("{height}px"));
        self.layout_submenu();
    }

    /// `computeMaxWidth(minWidth)`.
    pub fn compute_max_width(&self, min_width: f64) -> f64 {
        let effective_min_width = min_width.max(self.opts().and_then(|o| o.min_width).unwrap_or(0.0));
        let raw_max_width_cap = self.opts().and_then(|o| o.max_width).unwrap_or(f64::INFINITY);
        let max_width_cap = raw_max_width_cap.max(effective_min_width);
        let clamp = |width: f64| width.max(effective_min_width).min(max_width_cap);
        let (all, visible) = {
            let state = self.state.borrow();
            (state.all_menu_items.clone(), state.visible_menu_items.clone())
        };
        let total_item_count = all.len();
        if total_item_count >= 50 {
            return clamp(380.0);
        }

        // With hidden items, measure all of them to prevent width jumps when expanding/collapsing sections.
        let measured = if total_item_count > visible.len() { &all } else { &visible };
        let measurer = self.measurer.borrow().clone();
        clamp(measured.iter().map(|item| measurer(item)).fold(f64::NEG_INFINITY, f64::max))
    }

    // #endregion

    /// `dispose()`.
    pub fn dispose(&self) {
        if self.disposed.replace(true) {
            return;
        }
        let request = self.state.borrow_mut().filter_request.take();
        if let Some(request) = request {
            request.cancel();
        }
        self.state.borrow_mut().pending_submenu_element = None;
        let widget = self.state.borrow_mut().current_submenu_widget.take();
        if let Some(widget) = widget {
            widget.dispose();
        }
        self.panel_subscriptions.dispose();
        self.on_did_request_layout.dispose();
        if let Some(input) = &self.filter_input {
            input.dispose();
        }
    }
}

/// O delegate do widget do submenu: roda a ação, esconde o painel, avisa o delegate do pai e esconde o pai.
struct SubmenuDelegate<T: ActionListItemData> {
    parent: Weak<ActionListWidget<T>>,
}

impl<T: ActionListItemData> IActionListDelegate<ActionRef> for SubmenuDelegate<T> {
    fn on_hide(&self, _did_cancel: Option<bool>) {}

    fn on_select(&self, action: &ActionRef, _preview: bool) {
        let _ = action.run(&[]);
        if let Some(parent) = self.parent.upgrade() {
            parent.on_submenu_selected();
        }
    }
}

/// `ActionList<T>`: o `ActionListWidget` com o posicionamento por âncora e a conta da altura disponível.
pub struct ActionList<T: ActionListItemData> {
    widget: Rc<ActionListWidget<T>>,
    anchor: AnchorTarget,
    context_view_service: Rc<dyn IContextViewService>,
    layout_service: Rc<dyn ILayoutService>,
    subscriptions: Rc<DisposableStore>,
    state: RefCell<ActionListState>,
    anchor_position: Option<AnchorPosition>,
    preferred_anchor_position: Option<AnchorPosition>,
    use_full_height: bool,
    get_popup_chrome_height: Option<Rc<dyn Fn(&Rc<ViewElement>) -> f64>>,
    widget_class_name: Option<String>,
}

#[derive(Default)]
struct ActionListState {
    last_min_width: f64,
    has_laid_out: bool,
    show_above: Option<bool>,
    /// Height to size against instead of the current contents. Survives re-layouts.
    fixed_content_height: Option<f64>,
}

impl<T: ActionListItemData> ActionList<T> {
    pub fn new(
        supports_preview: bool,
        items: &[Rc<IActionListItem<T>>],
        delegate: Rc<dyn IActionListDelegate<T>>,
        options: Option<Rc<IActionListOptions>>,
        anchor: AnchorTarget,
        context_view_service: Rc<dyn IContextViewService>,
        layout_service: Rc<dyn ILayoutService>,
    ) -> Rc<Self> {
        let widget = ActionListWidget::new(supports_preview, items, delegate, options.clone());
        let list = Rc::new(Self {
            widget: widget.clone(),
            anchor,
            context_view_service,
            layout_service,
            subscriptions: DisposableStore::new(),
            state: RefCell::new(ActionListState::default()),
            anchor_position: options.as_deref().and_then(|o| o.anchor_position),
            preferred_anchor_position: options.as_deref().and_then(|o| o.preferred_anchor_position),
            use_full_height: options.as_deref().is_some_and(|o| o.use_full_height),
            get_popup_chrome_height: options.as_deref().and_then(|o| o.get_popup_chrome_height.clone()),
            widget_class_name: options.as_deref().and_then(|o| o.widget_class_name.clone()),
        });
        let weak = Rc::downgrade(&list);
        list.subscriptions.add(widget.on_did_request_layout().listen(move |_| {
            let Some(list) = weak.upgrade() else { return };
            let (has_laid_out, last_min_width) = {
                let state = list.state.borrow();
                (state.has_laid_out, state.last_min_width)
            };
            if has_laid_out {
                list.layout(last_min_width, None);
                list.context_view_service.layout();
            }
        }));
        list
    }

    /// O widget interno.
    pub fn widget(&self) -> &Rc<ActionListWidget<T>> {
        &self.widget
    }

    pub fn dom_node(&self) -> Rc<ViewElement> {
        self.widget.dom_node()
    }

    pub fn filter_container(&self) -> Option<Rc<ViewElement>> {
        self.widget.filter_container()
    }

    pub fn footer_container(&self) -> Option<Rc<ViewElement>> {
        self.widget.footer_container()
    }

    pub fn header_container(&self) -> Option<Rc<ViewElement>> {
        self.widget.header_container()
    }

    pub fn filter_input(&self) -> Option<Rc<FilterInput>> {
        self.widget.filter_input()
    }

    pub fn close_animation(&self) -> Option<IContextViewCloseAnimation> {
        self.widget.close_animation()
    }

    pub fn widget_class_name(&self) -> Option<String> {
        self.widget_class_name.clone()
    }

    /// Returns the resolved anchor position after the first layout. Used by the context view delegate to lock the dropdown
    /// direction.
    pub fn anchor_position(&self) -> Option<AnchorPosition> {
        if self.anchor_position.is_some() {
            return self.anchor_position;
        }
        let show_above = self.state.borrow().show_above?;
        Some(if show_above { AnchorPosition::Above } else { AnchorPosition::Below })
    }

    pub fn focus(&self) {
        self.widget.focus();
    }

    /// `hide(didCancel, hideContextView)`.
    pub fn hide(&self, did_cancel: Option<bool>, hide_context_view: bool) {
        self.widget.hide(did_cancel);
        if hide_context_view {
            self.context_view_service.hide_context_view(None);
        }
    }

    pub fn set_filter(&self, value: &str, focus_item_id: Option<&str>) {
        self.widget.set_filter(value, focus_item_id);
    }

    pub fn clear_filter(&self) -> bool {
        self.widget.clear_filter()
    }

    pub fn focus_previous(&self) {
        self.widget.focus_previous();
    }

    pub fn focus_next(&self) {
        self.widget.focus_next();
    }

    pub fn collapse_focused_section(&self) -> bool {
        self.widget.collapse_focused_section()
    }

    pub fn expand_focused_section(&self) -> bool {
        self.widget.expand_focused_section()
    }

    pub fn toggle_focused_section(&self) -> bool {
        self.widget.toggle_focused_section()
    }

    pub fn accept_selected(&self, preview: bool) {
        self.widget.accept_selected(preview);
    }

    pub fn update_items(&self, items: &[Rc<IActionListItem<T>>], focus_item_id: Option<&str>, options: Option<IActionListUpdateOptions>) {
        self.widget.update_items(items, focus_item_id, options);
    }

    pub fn get_focused_element(&self) -> Option<Rc<IActionListItem<T>>> {
        self.widget.get_focused_element()
    }

    pub fn focus_item_by_id(&self, item_id: &str) {
        self.widget.focus_item_by_id(item_id);
    }

    fn compute_action_widget_vertical_chrome_height(&self) -> f64 {
        match self.dom_node().parent_element().and_then(|parent| parent.closest("action-widget")) {
            Some(_) => ACTION_WIDGET_VERTICAL_CHROME_HEIGHT,
            None => 0.0,
        }
    }

    /// A altura de tudo que o popup mostra além das linhas: o filtro, o rodapé e o cabeçalho da própria lista.
    pub fn chrome_height(&self) -> f64 {
        let filter_height = if self.widget.filter_container().is_some() { FILTER_HEIGHT } else { 0.0 };
        let footer_height = if self.widget.footer_container().is_some() { FOOTER_HEIGHT } else { 0.0 };
        let header_height = match self.widget.header_container() {
            Some(header) if !header.is_hidden() => {
                let height = header.size().height;
                if height != 0.0 { height } else { DEFAULT_HEADER_HEIGHT }
            }
            _ => 0.0,
        };
        filter_height + footer_height + header_height
    }

    fn compute_height(&self) -> f64 {
        let max_visible_items_height = self.widget.compute_max_visible_items_height();
        let fixed_content_height = self.state.borrow().fixed_content_height;
        let list_height = fixed_content_height.unwrap_or_else(|| self.widget.compute_list_height().min(max_visible_items_height));

        let chrome_height = self.chrome_height();
        // Space the popup takes up around the list and its own chrome, e.g. its padding, border or tabs. It counts toward
        // fitting beside the anchor but not toward the viewport cap.
        let popup_chrome_height = self.get_popup_chrome_height.as_ref().map(|measure| measure(&self.dom_node()));
        let outer_chrome_height = match popup_chrome_height {
            None => self.compute_action_widget_vertical_chrome_height(),
            Some(popup_chrome_height) => (popup_chrome_height - chrome_height).max(0.0),
        };
        let window = self.layout_service.window_viewport();
        let available_height;

        if self.widget.has_dynamic_height() || self.anchor_position.is_some() || self.preferred_anchor_position.is_some() {
            let viewport_height = window.inner_height;
            let anchor_rect = get_anchor_rect(&self.anchor);
            let anchor_top_in_viewport = anchor_rect.top - window.page_y_offset;
            let bottom_gap = 30.0;
            let space_below = viewport_height - anchor_top_in_viewport - anchor_rect.height - bottom_gap;
            let space_above = anchor_top_in_viewport;

            // Keep the resolved direction stable when filtering changes the visible item count.
            if self.state.borrow().show_above.is_none() {
                // A pinned height decides the direction too, so a later tab cannot flip it.
                let full_height = fixed_content_height.unwrap_or_else(|| self.widget.compute_max_full_height());
                let show_above = if let Some(anchor_position) = self.anchor_position {
                    anchor_position == AnchorPosition::Above
                } else if let Some(preferred) = self.preferred_anchor_position {
                    let prefer_above = preferred == AnchorPosition::Above;
                    let viewport_max_height = if self.use_full_height { viewport_height } else { (viewport_height * 0.6).floor() };
                    let desired_height = (chrome_height + full_height).min(viewport_max_height) + outer_chrome_height;
                    // Reflect above preferences so both directions use the same flip policy.
                    let placement = layout_along_axis(
                        viewport_height - bottom_gap,
                        desired_height,
                        ILayoutAnchor {
                            offset: if prefer_above { viewport_height - bottom_gap - anchor_top_in_viewport - anchor_rect.height } else { anchor_top_in_viewport },
                            size: anchor_rect.height,
                            mode: None,
                            position: LayoutAnchorPosition::Before,
                        },
                    );
                    match placement.result {
                        LayoutResult::Overlap => space_above > space_below,
                        LayoutResult::Ok => prefer_above,
                        LayoutResult::Flipped => !prefer_above,
                    }
                } else {
                    chrome_height + full_height > space_below && space_above > space_below
                };
                self.state.borrow_mut().show_above = Some(show_above);
            }
            let show_above = self.state.borrow().show_above.unwrap_or(false);
            available_height = ((if show_above { space_above } else { space_below }) - outer_chrome_height).max(0.0);
        } else {
            let padding = 10.0;
            let window_height = self.layout_service.get_container().client_height();
            let widget_top = self.dom_node().rect().top;
            available_height = if widget_top > 0.0 { window_height - widget_top - padding } else { window_height * 0.7 };
        }

        let viewport_max_height = if self.use_full_height { window.inner_height } else { (window.inner_height * 0.6).floor() };
        let action_line_height = self.widget.line_height();
        if self.anchor_position.is_some() || self.preferred_anchor_position.is_some() {
            let max_height = available_height.min(viewport_max_height);
            let height = (list_height + chrome_height).min(max_height.max(0.0));
            return (height - chrome_height).max(0.0);
        }
        let max_height = available_height.max(action_line_height * 3.0 + chrome_height).min(viewport_max_height);
        let height = (list_height + chrome_height).min(max_height);
        height - chrome_height
    }

    /// `layout(minWidth, fixedContentHeight)`: devolve a largura.
    pub fn layout(&self, min_width: f64, fixed_content_height: Option<f64>) -> f64 {
        {
            let mut state = self.state.borrow_mut();
            state.has_laid_out = true;
            state.last_min_width = min_width;
            if fixed_content_height.is_some() {
                state.fixed_content_height = fixed_content_height;
            }
        }

        let list_height = self.compute_height();
        self.widget.layout(list_height, None);

        let computed_width = self.widget.compute_max_width(min_width);
        self.widget.layout(list_height, Some(computed_width));

        computed_width
    }
}

impl<T: ActionListItemData> IDisposable for ActionList<T> {
    fn dispose(&self) {
        self.subscriptions.dispose();
        self.widget.dispose();
    }
}
