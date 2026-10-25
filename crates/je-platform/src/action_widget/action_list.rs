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
//! Entram nesta fatia: as linhas como estado ([`RowState`], de `ActionItemRenderer`, `SeparatorRenderer` e do
//! `accessibilityProvider`), o contorno da linha focada, o painel de hover e de submenu (`_showSubmenuForElement`,
//! `_layoutSubmenu` do painel de conteúdo, os temporizadores de mostrar e esconder), os eventos de ponteiro
//! (`onListHover`, `initialMouseMove`, `mousedown`), o `Tab` pelos controles do painel (`tabThroughPanel`) e
//! `_measureItemWidths` (a medida do texto vem do renderizador nativo: [`ActionListWidget::set_item_measurer`]).
//!
//! Fica para a fatia seguinte: o `onHover` assíncrono do delegate (`canPreview`), o triângulo seguro do ponteiro com
//! intenção, as ações do rodapé do hover (`hover.actions`), os links de `detail`, o foco das barras de ferramentas por linha
//! (`focusItemAction`, `tabThroughItemActions`), a animação de movimento de item, o `markdown` das descrições e os atalhos
//! resolvidos (`keybinding`, o título com `{0} to Apply`).
//!
//! Entram na fatia do submenu com lista: o `softDispatch` do campo de filtro ([`KeybindingResolver`]), o posicionamento do
//! painel de lista (`preserveVerticalPosition`, `alignToAnchorTop`, a altura filtrada perto da borda do viewport), o
//! `hoverContent` das ações filhas ([`RemovableAction::with_hover_content`]) e o clique na barra de remoção do submenu.

use std::any::Any;
use std::cell::{Cell, RefCell};
use std::collections::{BTreeMap, BTreeSet};
use std::rc::{Rc, Weak};

use je_base::actions::{ActionRef, IAction, SubmenuAction};
use je_base::dynamic::Dynamic;
use je_base::errors::Error;
use je_base::event::{Emitter, Event};
use je_base::key_codes::KeyCode;
use je_base::layout::{AnchorPosition, ILayoutAnchor, IRect, LayoutAnchorPosition, LayoutResult, layout as layout_along_axis};
use je_base::lifecycle::{DisposableRef, DisposableStore, IDisposable};
use je_base::promise::Promise;
use je_base::runtime::{TimerId, clear_timeout, queue_microtask, set_timeout};
use je_base::uri::URI;

use crate::contextview::context_view::{AnchorTarget, IContextViewCloseAnimation, IContextViewService, get_anchor_rect};
use crate::contextview::view_element::{UiKeyEvent, ViewElement};
use crate::layout::layout_service::ILayoutService;

pub const ACCEPT_SELECTED_ACTION_COMMAND: &str = "acceptSelectedCodeAction";
pub const PREVIEW_SELECTED_ACTION_COMMAND: &str = "previewSelectedCodeAction";

/// Action ID of the auto-appended toolbar action created from [`IActionListItem::on_remove`].
pub const REMOVE_TOOLBAR_ACTION_ID: &str = "actionList.remove";

/// A altura (px) de uma linha de ação, de cabeçalho e de separador sem rótulo (`_actionLineHeight`, `_headerLineHeight` e
/// `_separatorLineHeight`).
/// A duração de um quadro de animação (`requestAnimationFrame`) no relógio do `runtime`.
const ANIMATION_FRAME_MS: u64 = 16;
const ACTION_LINE_HEIGHT: f64 = 24.0;
const HEADER_LINE_HEIGHT: f64 = 24.0;
const SEPARATOR_LINE_HEIGHT: f64 = 8.0;

/// A altura do filtro e do rodapé que o `ActionList.computeHeight` desconta, e a do cabeçalho sem altura própria.
pub const FILTER_HEIGHT: f64 = 36.0;
const FOOTER_HEIGHT: f64 = 32.0;
const DEFAULT_HEADER_HEIGHT: f64 = 36.0;

/// O espaço vertical que o `.action-widget` reserva em volta do conteúdo: `padding: 4px` e `border: 1px` nos dois lados
/// (`actionWidget.css`). É o que `computeActionWidgetVerticalChromeHeight` lê do estilo calculado.
pub const ACTION_WIDGET_VERTICAL_CHROME_HEIGHT: f64 = 10.0;

/// A largura da barra de rolagem vertical da lista (`verticalScrollbarSize` do `List`).
pub const LIST_VERTICAL_SCROLLBAR_SIZE: f64 = 10.0;

/// O `padding-right` da linha (`padding: 0 12px 0 6px`) e o da linha com detalhe (`padding-right: 6px`), e a margem direita
/// que a barra de ferramentas da linha com detalhe ganha (`var(--vscode-spacing-size60, 6px)`).
const ROW_PADDING_RIGHT: f64 = 12.0;
const ROW_DETAIL_PADDING_RIGHT: f64 = 6.0;
const ROW_DETAIL_TOOLBAR_MARGIN_RIGHT: f64 = 6.0;
/// A altura da linha do título com detalhe (`.title { line-height: 14px }`), a do detalhe (`line-height: 14px`) e a da barra
/// de ferramentas (ícone de 16px mais o enchimento de 3px de cada lado do `.action-label`).
const DETAIL_TITLE_LINE_HEIGHT: f64 = 14.0;
const DETAIL_LINE_HEIGHT: f64 = 14.0;
const TOOLBAR_HEIGHT: f64 = 22.0;

/// A geometria calculada da barra de ferramentas de uma linha: o `getComputedStyle` e os retângulos que o teste confere.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct RowToolbarGeometry {
    pub row_height: f64,
    /// `detail.getBoundingClientRect().top` (zero para a linha sem detalhe: o topo da linha).
    pub detail_top: f64,
    /// `getComputedStyle(toolbar).display` e `.visibility`.
    pub display: &'static str,
    pub visibility: &'static str,
    /// `getComputedStyle(toolbar).marginRight`, em px.
    pub margin_right: f64,
    /// `row.getBoundingClientRect().right - toolbar.getBoundingClientRect().right`.
    pub right_inset: f64,
}

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
    /// `icon.color.id` do `ThemeIcon`: o id do token de cor do tema.
    pub icon_color: Option<String>,
    pub title: String,
}

/// Um selo de `additionalBadges`: `{ label, className?, tooltip? }`.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ActionListBadge {
    pub label: String,
    pub class_name: Option<String>,
    pub tooltip: Option<String>,
}

/// Um elemento de conteúdo de hover (o `HTMLElement` que o chamador cria): texto, tamanho e identidade (`Rc`).
pub struct HoverElement {
    text: RefCell<String>,
    aria_label: RefCell<Option<String>>,
    size: Cell<(f64, f64)>,
    /// Os ouvintes do `ResizeObserver` do elemento (referências fracas: quem observa guarda o ouvinte).
    resize_listeners: RefCell<Vec<Weak<dyn Fn()>>>,
}

impl HoverElement {
    pub fn new(text: &str) -> Rc<Self> {
        Rc::new(Self {
            text: RefCell::new(text.to_owned()),
            aria_label: RefCell::new(None),
            size: Cell::new((0.0, 0.0)),
            resize_listeners: RefCell::new(Vec::new()),
        })
    }

    /// `ResizeObserver.observe(element)`: o ouvinte roda quando o tamanho do elemento muda, enquanto quem observa o mantiver vivo.
    pub fn observe_resize(&self, listener: &Rc<dyn Fn()>) {
        self.resize_listeners.borrow_mut().push(Rc::downgrade(listener));
    }

    /// `textContent`.
    pub fn text(&self) -> String {
        self.text.borrow().clone()
    }

    /// `setAttribute('aria-label', label)`.
    pub fn set_aria_label(&self, label: &str) {
        *self.aria_label.borrow_mut() = Some(label.to_owned());
    }

    /// `getAttribute('aria-label') ?? textContent`: o nome que o controle tem para quem o foca.
    pub fn accessible_label(&self) -> String {
        self.aria_label.borrow().clone().unwrap_or_else(|| self.text())
    }

    pub fn set_text(&self, text: &str) {
        *self.text.borrow_mut() = text.to_owned();
    }

    /// `style.width` e `style.height`: avisa os observadores quando o tamanho muda.
    pub fn set_size(&self, width: f64, height: f64) {
        if self.size.replace((width, height)) == (width, height) {
            return;
        }
        self.resize_listeners.borrow_mut().retain(|listener| listener.strong_count() > 0);
        let listeners: Vec<Rc<dyn Fn()>> = self.resize_listeners.borrow().iter().filter_map(Weak::upgrade).collect();
        for listener in listeners {
            listener();
        }
    }

    pub fn size(&self) -> (f64, f64) {
        self.size.get()
    }
}

/// `IActionListItemHover.content`: `string | IMarkdownString | HTMLElement | (() => HTMLElement)`.
#[derive(Clone)]
pub enum HoverContent {
    Text(String),
    Element(Rc<HoverElement>),
    Lazy(Rc<dyn Fn() -> Rc<HoverElement>>),
    /// `new MarkdownString().appendText(text).appendMarkdown(markdown)`: o texto entra literal (com os caracteres do
    /// markdown escapados) e o resto é renderizado como markdown.
    Markdown { text: String, markdown: String },
}

/// O texto e a quantidade de links que o markdown de `appendMarkdown` renderiza: `**negrito**`, `__negrito__`, `` `código` ``
/// e `[texto](destino)` (o link vira o texto, e conta como um `<a>`).
fn render_markdown(markdown: &str) -> (String, usize) {
    let mut text = String::new();
    let mut links = 0;
    let chars: Vec<char> = markdown.chars().collect();
    let mut index = 0;
    while index < chars.len() {
        let rest: String = chars[index..].iter().collect();
        if rest.starts_with("**") || rest.starts_with("__") {
            index += 2;
            continue;
        }
        if chars[index] == '`' {
            index += 1;
            continue;
        }
        if chars[index] == '[' {
            if let Some(close) = rest.find("](") {
                if let Some(end) = rest[close..].find(')') {
                    text.push_str(&rest[1..close]);
                    links += 1;
                    index += rest[..close + end + 1].chars().count();
                    continue;
                }
            }
        }
        text.push(chars[index]);
        index += 1;
    }
    (text, links)
}

/// `IActionListItemHover`: os campos que mudam estado ou geometria do painel.
#[derive(Clone, Default)]
pub struct IActionListItemHover {
    pub content: Option<HoverContent>,
    /// Enables keyboard access and a clickable row chevron for the hover panel.
    pub expandable: bool,
    /// Whether to show the expandable hover's row chevron. Defaults to true.
    pub show_indicator: Option<bool>,
    /// Includes the focused row's toolbar actions and hover-panel controls in one Tab sequence.
    pub tab_through_panel: bool,
    /// Interactive elements owned by the hover content, in forward Tab order.
    pub get_tabbable_elements: Option<Rc<dyn Fn() -> Vec<Rc<HoverElement>>>>,
    /// The hover content provides its own padding, so the header adds none.
    pub content_owns_padding: bool,
    /// Align the panel's bottom with the parent popup, keeping content above its trigger.
    pub align_to_parent_bottom: bool,
    /// Align the panel with the outer border of the parent popup instead of the row's own edge.
    pub align_to_parent: bool,
    /// Keep the panel where it opened as its content changes size.
    pub preserve_vertical_position: bool,
    /// Align the panel's top with the trigger row (with `preserve_vertical_position`, shifting up only to fit the viewport).
    pub align_to_anchor_top: bool,
    /// Actions rendered in the standard hover footer (`.hover-row.status-bar`).
    pub actions: Vec<IHoverAction>,
    /// Releases a content element built by a `HoverContent::Lazy` that the panel did not keep.
    pub dispose_content: Option<Rc<dyn Fn(&Rc<HoverElement>)>>,
    /// Owned by the hover descriptor; registered for the widget's lifetime when its content is shown.
    pub disposable: Option<DisposableRef>,
}

/// `IHoverAction`: uma ação do rodapé padrão do hover.
#[derive(Clone)]
pub struct IHoverAction {
    pub command_id: String,
    pub label: String,
    pub icon_class: Option<String>,
    pub run: Rc<dyn Fn()>,
}

/// O rodapé do painel (`.hover-row.status-bar`): a classe, o rótulo de cada ação e o retângulo do `.actions`.
#[derive(Clone)]
pub struct PanelFooter {
    pub class_name: String,
    pub actions: Vec<String>,
    pub rect: IRect,
}

/// O estilo do painel (`.action-list-submenu-panel`): `padding`, `border` e `overflow: hidden`.
pub const SUBMENU_PANEL_PADDING: f64 = 4.0;
pub const SUBMENU_PANEL_BORDER: f64 = 1.0;
/// `.action-list-submenu-panel { overflow: hidden }`.
pub const SUBMENU_PANEL_CLIPS_OVERFLOW: bool = true;
/// A altura da linha do rodapé (`.hover-row.status-bar { line-height: 22px }`).
const HOVER_FOOTER_HEIGHT: f64 = 22.0;
/// A duração e a curva da animação que acompanha a linha que mudou de lugar (`_animateItemMove`).
const ITEM_MOVE_DURATION_MS: u64 = 160;
const ITEM_MOVE_EASING: &str = "cubic-bezier(0.2, 0.8, 0.2, 1)";

/// `IActionListHeaderLink`: o link do cabeçalho e da linha com detalhe.
#[derive(Clone)]
pub struct IActionListHeaderLink {
    /// Visible link text (e.g. "Learn more"). Should be localized.
    pub label: String,
    /// Target opened via the link handler when the link is activated.
    pub uri: URI,
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
/// `badgeBeforeLabel`, `additionalBadges`, `ariaDescription`, `opensDialog`, `toolbarDialogActionIds`,
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
    /// Optional link rendered at the end of the detail line.
    pub detail_link: Option<IActionListHeaderLink>,
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
    /// Badges displayed alongside `badge`, each with an optional single CSS class and hover.
    pub additional_badges: Vec<ActionListBadge>,
    /// Displays the badge before the label instead of after it.
    pub badge_before_label: bool,
    /// Optional accessible description used in place of `description` for screen reader labels.
    pub aria_description: Option<String>,
    pub hover: Option<IActionListItemHover>,
    pub hide_icon: bool,
    /// Chamado uma vez quando a linha entra na área visível da lista (depois do primeiro quadro, ao rolar).
    pub on_did_become_visible: Option<Rc<dyn Fn()>>,
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
            detail_link: None,
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
            additional_badges: Vec::new(),
            badge_before_label: false,
            aria_description: None,
            hover: None,
            hide_icon: false,
            on_did_become_visible: None,
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

/// `reserveSubmenuSpace?: boolean | 'always'`: `Never` é `false`, `WhenAnyHasSubmenu` é `true` (o padrão) e `Always` é `'always'`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ReserveSubmenuSpace {
    Never,
    WhenAnyHasSubmenu,
    Always,
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
    /// Opens links of the list (the detail link of a row) instead of the opener service. O original também recebe o item; a
    /// opção não é genérica no item, então só a `URI` chega.
    pub link_handler: Option<Rc<dyn Fn(&URI)>>,
    /// Optional dismiss ("x") button on the header banner; invoked on click, and the banner is removed.
    pub header_dismiss: Option<Rc<dyn Fn()>>,
    /// Reserve a gutter for the row chevron: `Always`, never, or only when some item of the list opens a panel (padrão).
    pub reserve_submenu_space: Option<ReserveSubmenuSpace>,
    /// Keeps a hover panel open while the pointer travels toward it.
    pub submenu_pointer_intent: bool,
    /// Delay (ms) before a hovered row opens its panel. Defaults to 500.
    pub submenu_hover_delay: Option<u64>,
    /// Stops pointer events on the row toolbar from reaching the row.
    pub stop_toolbar_pointer_propagation: bool,
    pub secondary_heading: Option<String>,
    /// Optional text shown below the action list as a footer.
    pub footer_text: Option<String>,
    /// Optional text shown above the action list as a header banner.
    pub header_text: Option<String>,
    /// Optional link appended to the header banner text (e.g. "Learn more"); activated through the link handler.
    pub header_link: Option<IActionListHeaderLink>,
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

/// `IAction & { onRemove?, hoverContent? }`: a ação embrulhada com o que o `Object.assign` do original acrescenta.
pub struct RemovableAction {
    action: ActionRef,
    on_remove: Option<OnRemove>,
    hover_content: Option<String>,
}

impl RemovableAction {
    /// `Object.assign(action, { onRemove })`.
    pub fn new(action: ActionRef, on_remove: OnRemove) -> Rc<Self> {
        Rc::new(Self { action, on_remove: Some(on_remove), hover_content: None })
    }

    /// `Object.assign(action, { hoverContent })`: o markdown que o hover da linha do submenu mostra depois do rótulo.
    pub fn with_hover_content(action: ActionRef, hover_content: &str) -> Rc<Self> {
        Rc::new(Self { action, on_remove: None, hover_content: Some(hover_content.to_owned()) })
    }

    pub fn on_remove(&self) -> Option<OnRemove> {
        self.on_remove.clone()
    }

    pub fn hover_content(&self) -> Option<String> {
        self.hover_content.clone()
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
    action.as_any().downcast_ref::<RemovableAction>().and_then(RemovableAction::on_remove)
}

/// `(action as IAction & { hoverContent?: string }).hoverContent`.
fn hover_content_of(action: &ActionRef) -> Option<String> {
    action.as_any().downcast_ref::<RemovableAction>().and_then(RemovableAction::hover_content)
}

// #endregion

// #region foco, filtro e requisição de filtro

/// `document.activeElement` dentro de uma lista: o campo de filtro, a lista, ou nenhum dos dois.
#[derive(Default)]
pub struct FocusState {
    filter_focused: Cell<bool>,
    list_focused: Cell<bool>,
    /// O painel do submenu ou do hover (`_submenuContainer`) é o `document.activeElement`.
    panel_focused: Cell<bool>,
    /// Um controle do conteúdo do hover é o `document.activeElement`.
    control_focused: RefCell<Option<Rc<HoverElement>>>,
    /// Um botão da barra de ferramentas da linha focada é o `document.activeElement` (o índice dele na barra).
    toolbar_focused: Cell<Option<usize>>,
}

impl FocusState {
    /// O foco do documento sai do painel, dos controles dele e da barra da linha.
    fn leave_panel(&self) {
        self.panel_focused.set(false);
        *self.control_focused.borrow_mut() = None;
        self.toolbar_focused.set(None);
    }
}

/// O `<input>` do filtro: o texto, o foco e os eventos `focus` e `blur`.
pub struct FilterInput {
    value: RefCell<String>,
    /// `selectionStart` e `selectionEnd`, em caracteres.
    selection: Cell<(usize, usize)>,
    focus_state: Rc<FocusState>,
    on_did_focus: Rc<Emitter<()>>,
    on_did_blur: Rc<Emitter<()>>,
}

impl FilterInput {
    fn new(value: String, focus_state: Rc<FocusState>) -> Rc<Self> {
        let end = value.chars().count();
        Rc::new(Self { value: RefCell::new(value), selection: Cell::new((end, end)), focus_state, on_did_focus: Emitter::new(), on_did_blur: Emitter::new() })
    }

    /// `value`.
    pub fn value(&self) -> String {
        self.value.borrow().clone()
    }

    /// `input.value = value`: o cursor vai para o fim do texto, como no DOM.
    pub fn set_value(&self, value: &str) {
        *self.value.borrow_mut() = value.to_owned();
        let end = value.chars().count();
        self.selection.set((end, end));
    }

    /// `input.setSelectionRange(start, end)`.
    pub fn set_selection_range(&self, start: usize, end: usize) {
        let length = self.value.borrow().chars().count();
        self.selection.set((start.min(length), end.min(length)));
    }

    /// `[input.selectionStart, input.selectionEnd]`.
    pub fn selection(&self) -> (usize, usize) {
        self.selection.get()
    }

    /// `dom.isActiveElement(input)`.
    pub fn is_focused(&self) -> bool {
        self.focus_state.filter_focused.get()
    }

    /// `input.focus()`: tira o foco da lista e dispara `focus` se ele ainda não estava no campo.
    pub fn focus(&self) {
        self.focus_state.list_focused.set(false);
        self.focus_state.leave_panel();
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

/// O `CancellationTokenSource` de uma requisição de `onFilter`. Clonar devolve outra referência ao mesmo token, como guardar
/// o `CancellationToken` e lê-lo depois.
#[derive(Clone, Default)]
pub struct FilterRequest {
    cancelled: Rc<Cell<bool>>,
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

/// O alvo de um evento de teclado, de dentro para fora: o campo de filtro, a lista, o painel do submenu ou o `domNode`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum KeyTarget {
    FilterInput,
    List,
    SubmenuPanel,
    /// Um controle embutido na lista: `input`, `textarea`, `button`, `a` ou um elemento `contenteditable`.
    EmbeddedControl,
    /// O link `.monaco-link` do detalhe da linha focada (`Link` trata o `Enter` e abre o destino).
    DetailLink,
    /// O próprio `domNode` (`widget.domNode.dispatchEvent(...)`), fora da lista.
    DomNode,
}

/// Um evento de ponteiro sobre uma linha: `clientX`, `clientY`, `movementX`, `movementY` e o alvo dentro da linha.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PointerEvent {
    pub client_x: f64,
    pub client_y: f64,
    pub movement_x: f64,
    pub movement_y: f64,
    pub target: PointerTarget,
}

impl Default for PointerEvent {
    fn default() -> Self {
        Self { client_x: 0.0, client_y: 0.0, movement_x: 0.0, movement_y: 0.0, target: PointerTarget::Row }
    }
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
    /// `_itemDetailLinks`: o link `.monaco-link` renderizado no detalhe de cada linha com `detailLink`.
    detail_links: Vec<(Rc<IActionListItem<T>>, Rc<HoverElement>)>,
    /// `_list.scrollTop`.
    scroll_top: f64,
    /// `_submenuContainer.style.display === 'none'`.
    panel_hidden: bool,
    /// O que o painel mostra como cabeçalho (`hoverHeader`).
    panel_header: Option<PanelHeader>,
    /// `ACTION_WIDGET_ANIMATED_CLASS` no painel (só com lista de submenu).
    panel_animated: bool,
    ignore_initial_hover: bool,
    /// O ouvinte `initialMouseMove` ainda está registrado.
    initial_mouse_move_listening: bool,
    suppress_hover: bool,
    submenu_show_timer: Option<TimerId>,
    submenu_hide_timer: Option<TimerId>,
    /// A linha que o painel aberto reporta como `aria-expanded` (`_expandedTrigger`).
    expanded_trigger: Option<Rc<IActionListItem<T>>>,
    submenu_pointer_origin: Option<(f64, f64)>,
    submenu_pointer_grace_deadline: Option<u64>,
    /// `getDomNodeZoomLevel(domNode)` e o tamanho da janela (`innerWidth`, `innerHeight`).
    zoom: f64,
    window_size: (f64, f64),
    /// O retângulo do painel depois do último `_layoutSubmenu` (`top`, `left`, largura, altura).
    panel_rect: Option<(f64, f64, f64, f64)>,
    /// `openingPanelTop` e `openingPanelHeight` do `_showSubmenuForElement` (já divididos pelo zoom): onde o painel que preserva a
    /// posição vertical abriu.
    opening_panel_top: Option<f64>,
    opening_panel_height: Option<f64>,
    /// `maxWidth` do `_showSubmenuForElement`: a largura da lista do submenu aberto.
    submenu_max_width: f64,
    /// `_submenuContainer.style.top` e `style.left` (px, já divididos pelo zoom), `style.width` e a altura do
    /// `.action-list-submenu-viewport` quando o painel tem um.
    panel_style_top: Option<f64>,
    panel_style_left: Option<f64>,
    panel_style_width: Option<f64>,
    panel_viewport_height: Option<f64>,
    /// Quantas vezes o `_layoutSubmenu` do painel aberto reposicionou o painel.
    layout_submenu_count: usize,
    /// A rolagem do `.action-list-submenu-viewport` do hover, a altura do conteúdo dele e o zoom que o painel usa.
    panel_scroll_top: f64,
    panel_content_height: f64,
    panel_zoom: f64,
    /// O ouvinte do `ResizeObserver` do conteúdo e o quadro agendado que reposiciona o painel.
    panel_resize_listener: Option<Rc<dyn Fn()>>,
    panel_layout_timer: Option<TimerId>,
    /// As ações do rodapé do hover (`_submenuHoverActionElements`) e a ação de cada uma.
    panel_footer: Vec<(Rc<HoverElement>, IHoverAction)>,
    /// `_submenuSide`: o lado em que o painel persistente abriu (`true` é a direita).
    submenu_side: Option<bool>,
    /// A animação de movimento da linha (duração e curva) e `prefers-reduced-motion`.
    item_move_animation: Option<(u64, &'static str)>,
    reduce_motion: bool,
    /// `canPrefetchVisibleItems`: verdadeiro a partir do primeiro quadro depois da criação.
    can_prefetch_visible_items: bool,
    /// `visibleItems`: a identidade das linhas que já foram avisadas.
    visible_items: BTreeSet<String>,
    /// `visibleItemsUpdate`: o quadro agendado que avisa as linhas que entraram na área visível.
    visible_items_update: Option<TimerId>,
    prefetch_timer: Option<TimerId>,
}

/// O cabeçalho do painel: o texto do markdown renderizado ou o elemento do chamador.
#[derive(Clone)]
enum PanelHeader {
    Text(String),
    /// O markdown renderizado: o `textContent` e quantos `<a>` ele tem.
    Rendered { text: String, links: usize },
    Element(Rc<HoverElement>),
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
    /// `_headerContainer`: some até o botão de dispensa remover o banner.
    header_container: RefCell<Option<Rc<ViewElement>>>,
    /// O `span.action-list-header-dismiss` do banner.
    header_dismiss_button: Option<Rc<ViewElement>>,
    on_did_request_layout: Rc<Emitter<()>>,
    state: RefCell<WidgetState<T>>,
    measurer: RefCell<ItemMeasurer<T>>,
    is_submenu_panel: Cell<bool>,
    disposed: Cell<bool>,
    panel_subscriptions: Rc<DisposableStore>,
    /// `_list.domId`: o prefixo do id de cada linha (`getElementID`).
    list_dom_id: String,
    /// `IKeybindingService.softDispatch`: o comando que a tecla resolve, se algum.
    keybinding_resolver: RefCell<Option<KeybindingResolver>>,
}

/// O que o `softDispatch` do `IKeybindingService` devolve para uma tecla: o `commandId` quando há uma regra
/// (`ResultKind.KbFound`) e `None` quando não há (`ResultKind.NoMatchingKb`).
pub type KeybindingResolver = Rc<dyn Fn(&UiKeyEvent) -> Option<String>>;

/// `actionWidgetKeybindingCommands`.
fn is_action_widget_keybinding_command(command_id: &str) -> bool {
    command_id == ACCEPT_SELECTED_ACTION_COMMAND || command_id == PREVIEW_SELECTED_ACTION_COMMAND || command_id == "toggleSectionCodeAction"
}

/// Os atributos de acessibilidade do campo de filtro (`role`, `aria-*`).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct FilterAria {
    pub role: Option<String>,
    pub label: String,
    pub autocomplete: Option<String>,
    pub expanded: Option<String>,
    pub controls: Option<String>,
    pub active_descendant: Option<String>,
}

std::thread_local! {
    static NEXT_LIST_DOM_ID: Cell<usize> = const { Cell::new(0) };
}

/// `List.domId`: um id único por lista.
fn next_list_dom_id() -> String {
    NEXT_LIST_DOM_ID.with(|next| {
        next.set(next.get() + 1);
        format!("list_id_{}", next.get())
    })
}

fn has_submenu_actions<T: ActionListItemData>(item: &IActionListItem<T>) -> bool {
    item.submenu_actions.as_ref().is_some_and(|actions| !actions.is_empty())
}

/// Whether the row exposes a submenu or keyboard-accessible hover panel.
fn has_expandable_panel<T: ActionListItemData>(item: &IActionListItem<T>) -> bool {
    has_submenu_actions(item) || item.hover.as_ref().is_some_and(|hover| hover.expandable)
}

fn has_submenu_indicator<T: ActionListItemData>(item: &IActionListItem<T>) -> bool {
    has_expandable_panel(item) && item.hover.as_ref().and_then(|hover| hover.show_indicator) != Some(false)
}

/// `${value}px` de uma propriedade de `style`: o zero negativo sai como `0px`.
fn css_px(value: f64) -> String {
    format!("{}px", value + 0.0)
}

/// `stripNewlines(str)`.
fn strip_newlines(text: &str) -> String {
    text.replace("\r\n", " ").replace(['\r', '\n'], " ")
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
        let header_dismiss_button = if header_container.is_some() && options.as_deref().is_some_and(|o| o.header_dismiss.is_some()) {
            Some(ViewElement::with_classes(&["action-list-header-dismiss"]))
        } else {
            None
        };

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
            header_container: RefCell::new(header_container),
            header_dismiss_button,
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
                detail_links: Vec::new(),
                scroll_top: 0.0,
                panel_hidden: true,
                panel_header: None,
                panel_animated: false,
                ignore_initial_hover: true,
                initial_mouse_move_listening: true,
                suppress_hover: false,
                submenu_show_timer: None,
                submenu_hide_timer: None,
                expanded_trigger: None,
                submenu_pointer_origin: None,
                submenu_pointer_grace_deadline: None,
                zoom: 1.0,
                window_size: (1280.0, 800.0),
                panel_rect: None,
                opening_panel_top: None,
                opening_panel_height: None,
                submenu_max_width: 0.0,
                panel_style_top: None,
                panel_style_left: None,
                panel_style_width: None,
                panel_viewport_height: None,
                layout_submenu_count: 0,
                panel_scroll_top: 0.0,
                panel_content_height: 0.0,
                panel_zoom: 1.0,
                panel_resize_listener: None,
                panel_layout_timer: None,
                panel_footer: Vec::new(),
                submenu_side: None,
                item_move_animation: None,
                reduce_motion: false,
                can_prefetch_visible_items: false,
                visible_items: BTreeSet::new(),
                visible_items_update: None,
                prefetch_timer: None,
            }),
            measurer: RefCell::new(Rc::new(|_| 0.0)),
            is_submenu_panel: Cell::new(false),
            disposed: Cell::new(false),
            panel_subscriptions: DisposableStore::new(),
            list_dom_id: next_list_dom_id(),
            keybinding_resolver: RefCell::new(None),
        });

        widget.apply_filter(false, true, None);
        // O quadro que libera `canPrefetchVisibleItems` só é agendado quando alguma linha quer saber que ficou visível.
        if items.iter().any(|item| item.on_did_become_visible.is_some()) {
            let weak = widget.this.clone();
            let prefetch_timer = set_timeout(
                move || {
                    if let Some(widget) = weak.upgrade() {
                        let mut state = widget.state.borrow_mut();
                        state.prefetch_timer = None;
                        state.can_prefetch_visible_items = true;
                    }
                },
                ANIMATION_FRAME_MS,
            );
            widget.state.borrow_mut().prefetch_timer = Some(prefetch_timer);
        }

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
        self.header_container.borrow().clone()
    }

    /// `headerContainer.querySelector('.action-list-header-dismiss')`.
    pub fn header_dismiss_button(&self) -> Option<Rc<ViewElement>> {
        self.header_dismiss_button.clone()
    }

    /// O `mouseup` do botão de dispensa (`addDisposableGenericMouseUpListener`).
    pub fn header_dismiss_mouse_up(&self) {
        self.dismiss_header();
    }

    /// O `keydown` do botão de dispensa: `Enter` e espaço dispensam o banner.
    pub fn header_dismiss_key_down(&self, e: &UiKeyEvent) {
        if e.key == "Enter" || e.key == " " {
            e.prevent_default();
            self.dismiss_header();
        }
    }

    /// `headerContainer.querySelector('a.monaco-link')`: o texto do link do banner e o `href` dele.
    pub fn header_link(&self) -> Option<(String, String)> {
        self.header_container.borrow().as_ref()?;
        let link = self.opts()?.header_link.as_ref()?;
        Some((link.label.clone(), link.uri.to_string(true).to_string_lossy()))
    }

    fn dismiss_header(&self) {
        let Some(on_dismiss) = self.opts().and_then(|o| o.header_dismiss.clone()) else {
            return;
        };
        on_dismiss();
        // Refocus the widget first so removing the focused button doesn't trip close-on-blur.
        self.focus();
        let header = self.header_container.borrow_mut().take();
        if let Some(header) = header {
            header.set_parent(None);
        }
        // Drop the reference so the banner no longer reserves header height, then request a re-layout so the popup shrinks to
        // fit the remaining content.
        self.on_did_request_layout.fire(());
    }

    /// `filterInput`.
    pub fn filter_input(&self) -> Option<Rc<FilterInput>> {
        self.filter_input.clone()
    }

    /// `closeAnimation`.
    pub fn close_animation(&self) -> Option<IContextViewCloseAnimation> {
        self.opts().and_then(|o| o.close_animation.clone())
    }

    /// Os atributos `role` e `aria-*` do campo de filtro. `aria-activedescendant` segue o foco da lista enquanto o campo é
    /// o elemento ativo (`_updateFilterActiveDescendant`).
    pub fn filter_aria(&self) -> Option<FilterAria> {
        let input = self.filter_input.as_ref()?;
        let placeholder = self.opts().and_then(|o| o.filter_placeholder.clone()).unwrap_or_else(|| "Search...".to_owned());
        if !self.opts().is_some_and(|o| o.filter_as_combobox) {
            return Some(FilterAria { label: "Filter items".to_owned(), ..FilterAria::default() });
        }
        let focused = self.state.borrow().focus;
        let active_descendant = match focused {
            Some(index) if input.is_focused() => Some(self.row_element_id(index)),
            _ => None,
        };
        Some(FilterAria {
            role: Some("combobox".to_owned()),
            label: placeholder,
            autocomplete: Some("list".to_owned()),
            expanded: Some("true".to_owned()),
            controls: Some(self.list_dom_id.clone()),
            active_descendant,
        })
    }

    /// O `id` da linha focada (`document.querySelector('.monaco-list-row.focused').id`).
    pub fn focused_row_id(&self) -> Option<String> {
        self.focused_index().map(|index| self.row_element_id(index))
    }

    /// `_list.getElementID(index)`.
    fn row_element_id(&self, index: usize) -> String {
        format!("{}_{index}", self.list_dom_id)
    }

    /// `document.getElementById(id)` de uma linha: o estado dela.
    pub fn row_by_element_id(&self, id: &str) -> Option<RowState> {
        let index = id.strip_prefix(self.list_dom_id.as_str())?.strip_prefix('_')?.parse::<usize>().ok()?;
        self.row_state(index)
    }

    /// `document.getElementById(id)?.getAttribute('role')` da lista (`getWidgetRole`).
    pub fn list_role_by_id(&self, id: &str) -> Option<String> {
        (id == self.list_dom_id).then(|| "listbox".to_owned())
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

    /// O índice da linha focada (`_list.getFocus()[0]`).
    pub fn focused_index(&self) -> Option<usize> {
        self.state.borrow().focus
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

    /// O serviço de teclas (`IKeybindingService.softDispatch`) que o campo de filtro de um submenu consulta; o submenu que este
    /// widget abre herda o mesmo, como o `createInstance` do original.
    pub fn set_keybinding_resolver(&self, resolver: KeybindingResolver) {
        *self.keybinding_resolver.borrow_mut() = Some(resolver);
    }

    /// `_submenuContainer.style.top`.
    pub fn panel_style_top(&self) -> Option<String> {
        self.state.borrow().panel_style_top.map(css_px)
    }

    /// `_submenuContainer.style.left`.
    pub fn panel_style_left(&self) -> Option<String> {
        self.state.borrow().panel_style_left.map(css_px)
    }

    /// `_submenuContainer.style.width`.
    pub fn panel_style_width(&self) -> Option<String> {
        self.state.borrow().panel_style_width.map(css_px)
    }

    /// `.action-list-submenu-viewport`.style.height em px, quando o painel tem esse viewport.
    pub fn panel_viewport_height(&self) -> Option<f64> {
        self.state.borrow().panel_viewport_height
    }

    /// O texto do rodapé (`footerContainer.textContent`).
    pub fn footer_text(&self) -> Option<String> {
        self.footer_container.as_ref()?;
        self.opts().and_then(|o| o.footer_text.clone())
    }

    /// Quantas vezes o `_layoutSubmenu` do painel aberto deste widget reposicionou o painel.
    pub fn submenu_layout_count(&self) -> usize {
        self.state.borrow().layout_submenu_count
    }

    /// `_list.scrollTop = value`: o `scroll` da lista reposiciona o painel aberto (`_layoutSubmenu`). O limite da rolagem é do
    /// `List` (ver `reveal`); aqui o valor é o que o elemento recebeu.
    pub fn set_scroll_top(&self, scroll_top: f64) {
        let changed = {
            let mut state = self.state.borrow_mut();
            let changed = (state.scroll_top - scroll_top).abs() > f64::EPSILON;
            state.scroll_top = scroll_top;
            changed
        };
        if changed {
            self.layout_submenu();
            self.schedule_visible_items_update();
        }
    }

    /// O `onDidScroll` da lista com `scrollTopChanged`: no próximo quadro, as linhas que entraram na área visível avisam
    /// `onDidBecomeVisible` uma vez (a identidade é o `id` do item, ou o próprio item).
    fn schedule_visible_items_update(&self) {
        {
            let state = self.state.borrow();
            if !state.can_prefetch_visible_items || state.visible_items_update.is_some() {
                return;
            }
        }
        let weak = self.this.clone();
        let timer = set_timeout(
            move || {
                if let Some(widget) = weak.upgrade() {
                    widget.state.borrow_mut().visible_items_update = None;
                    widget.notify_visible_items();
                }
            },
            ANIMATION_FRAME_MS,
        );
        self.state.borrow_mut().visible_items_update = Some(timer);
    }

    /// `getVisibleItems()`: as linhas entre `firstVisibleIndex` e `lastVisibleIndex` da área visível da lista.
    fn visible_range_items(&self) -> Vec<Rc<IActionListItem<T>>> {
        let viewport = self.dom_node.size().height;
        let state = self.state.borrow();
        let scroll_top = state.scroll_top;
        let mut top = 0.0;
        let mut items = Vec::new();
        for item in &state.visible_menu_items {
            let height = Self::item_height_with(item, self.opts());
            if top + height > scroll_top && top < scroll_top + viewport {
                items.push(item.clone());
            }
            top += height;
        }
        items
    }

    fn notify_visible_items(&self) {
        let identity = |item: &Rc<IActionListItem<T>>| item.item_id().map_or_else(|| format!("ptr:{:p}", Rc::as_ptr(item)), |id| format!("id:{id}"));
        let items = self.visible_range_items();
        let current: BTreeSet<String> = items.iter().map(identity).collect();
        let previous = std::mem::replace(&mut self.state.borrow_mut().visible_items, current);
        for item in items {
            if !previous.contains(&identity(&item)) {
                if let Some(notify) = &item.on_did_become_visible {
                    notify();
                }
            }
        }
    }

    /// O evento `resize` da janela: o painel aberto cancela o que estava agendado e se reposiciona.
    pub fn window_resized(&self) {
        let open = self.state.borrow().current_submenu_element.is_some();
        if open {
            self.cancel_submenu_show();
            self.reset_submenu_pointer();
            self.layout_submenu();
        }
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
        self.focus_state.leave_panel();
        let child = self.state.borrow().current_submenu_widget.clone();
        if let Some(child) = child {
            child.release_focus();
        }
        self.focus_state.list_focused.set(true);
    }

    /// O foco do documento saiu deste widget e dos painéis dele.
    fn release_focus(&self) {
        self.focus_state.list_focused.set(false);
        self.focus_state.leave_panel();
        if let Some(input) = &self.filter_input {
            input.blur();
        }
        let child = self.state.borrow().current_submenu_widget.clone();
        if let Some(child) = child {
            child.release_focus();
        }
    }

    /// `_submenuContainer.focus()`.
    fn focus_panel(&self) {
        if let Some(input) = &self.filter_input {
            input.blur();
        }
        self.focus_state.list_focused.set(false);
        *self.focus_state.control_focused.borrow_mut() = None;
        self.focus_state.toolbar_focused.set(None);
        self.focus_state.panel_focused.set(true);
    }

    /// `control.focus()` num controle do conteúdo do hover.
    pub fn focus_control(&self, control: Rc<HoverElement>) {
        if let Some(input) = &self.filter_input {
            input.blur();
        }
        self.focus_state.list_focused.set(false);
        self.focus_state.panel_focused.set(false);
        self.focus_state.toolbar_focused.set(None);
        *self.focus_state.control_focused.borrow_mut() = Some(control);
    }

    /// `toolbar.focus(index)`: o foco vai para o botão `index` da barra da linha focada.
    pub fn focus_toolbar_button(&self, index: usize) {
        if let Some(input) = &self.filter_input {
            input.blur();
        }
        self.focus_state.list_focused.set(false);
        self.focus_state.panel_focused.set(false);
        *self.focus_state.control_focused.borrow_mut() = None;
        self.focus_state.toolbar_focused.set(Some(index));
    }

    /// O botão da barra da linha focada que é o `document.activeElement`: o índice e o `aria-label`.
    pub fn focused_toolbar_button(&self) -> Option<(usize, String)> {
        let position = self.focus_state.toolbar_focused.get()?;
        let row = self.row_state(self.focused_index()?)?;
        row.toolbar.get(position).map(|button| (position, button.aria_label.clone()))
    }

    /// Quantos botões a barra de `element` tem (as ações e a de remover que `onRemove` acrescenta).
    fn toolbar_length(element: &IActionListItem<T>) -> usize {
        element.toolbar_actions.as_ref().map_or(0, Vec::len) + usize::from(element.on_remove.is_some())
    }

    /// `document.activeElement === this._list.getHTMLElement()`.
    pub fn is_list_focused(&self) -> bool {
        self.focus_state.list_focused.get()
    }

    /// `document.activeElement === panel`.
    pub fn is_panel_focused(&self) -> bool {
        self.focus_state.panel_focused.get()
    }

    /// `panel.contains(document.activeElement)`.
    pub fn panel_contains_focus(&self) -> bool {
        if self.focus_state.list_focused.get() || self.focus_state.filter_focused.get() {
            return false;
        }
        if self.focus_state.panel_focused.get() || self.focus_state.control_focused.borrow().is_some() {
            return true;
        }
        let child = self.state.borrow().current_submenu_widget.clone();
        child.is_some_and(|child| child.has_focus())
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
        let list_has_focus = self.focus_state.list_focused.get() || self.focus_state.toolbar_focused.get().is_some();

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
        self.with_hover_suppressed(true, |widget| widget.focus_checked_or_first_inner());
    }

    /// Roda `body` com `_suppressHover` ligado (`value`), restaurando o valor anterior.
    fn with_hover_suppressed<R>(&self, value: bool, body: impl FnOnce(&Self) -> R) -> R {
        let previous = self.state.borrow().suppress_hover;
        self.state.borrow_mut().suppress_hover = previous || value;
        let result = body(self);
        self.state.borrow_mut().suppress_hover = previous;
        result
    }

    fn focus_checked_or_first_inner(&self) {
        let initial_focus_item_id = self.state.borrow_mut().initial_focus_item_id.take();
        if let Some(initial_focus_item_id) = initial_focus_item_id {
            for index in 0..self.list_length() {
                let Some(element) = self.element(index) else { continue };
                if Self::focus_condition(&element) && element.item_id().as_deref() == Some(initial_focus_item_id.as_str()) {
                    self.set_focus(Some(index));
                    self.reveal(index);
                    return;
                }
            }
        }
        let focused_index = self.state.borrow().focus;
        if let Some(focused_index) = focused_index {
            if self.element(focused_index).is_some_and(|element| Self::focus_condition(&element)) {
                self.reveal(focused_index);
                return;
            }
        }
        // Try to focus the checked item first
        for index in 0..self.list_length() {
            let Some(element) = self.element(index) else { continue };
            if Self::focus_condition(&element) && element.item.as_ref().is_some_and(ActionListItemData::list_item_checked) {
                self.set_focus(Some(index));
                self.reveal(index);
                return;
            }
        }
        // Set focus on the first focusable item without moving DOM focus
        self.list_focus_first();
        let focus = self.state.borrow().focus;
        if let Some(index) = focus {
            self.reveal(index);
        }
    }

    /// `_list.reveal(index)`: rola o mínimo para a linha ficar inteira na área visível da lista (a altura do `domNode`).
    fn reveal(&self, index: usize) {
        let viewport = self.dom_node.size().height;
        let (top, height, content) = {
            let state = self.state.borrow();
            let height_of = |item: &Rc<IActionListItem<T>>| Self::item_height_with(item, self.opts());
            let top: f64 = state.visible_menu_items.iter().take(index).map(height_of).sum();
            let height = state.visible_menu_items.get(index).map_or(0.0, height_of);
            (top, height, state.visible_menu_items.iter().map(height_of).sum::<f64>())
        };
        let mut state = self.state.borrow_mut();
        let mut scroll_top = state.scroll_top;
        if top < scroll_top {
            scroll_top = top;
        } else if top + height > scroll_top + viewport {
            scroll_top = top + height - viewport;
        }
        let clamped = scroll_top.min((content - viewport).max(0.0)).max(0.0);
        let changed = (state.scroll_top - clamped).abs() > f64::EPSILON;
        state.scroll_top = clamped;
        drop(state);
        if changed {
            self.schedule_visible_items_update();
        }
    }

    /// `rows.style.top` do `.monaco-list-rows`: o deslocamento negativo da rolagem.
    pub fn rows_top(&self) -> f64 {
        -self.state.borrow().scroll_top
    }

    /// `rows.clientHeight` do `.monaco-list-rows`: a altura de todas as linhas visíveis.
    pub fn rows_height(&self) -> f64 {
        self.compute_list_height()
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

    /// `focusItemAction(itemId, actionId)`: restores focus to an item action without scheduling a later list focus.
    pub fn focus_item_action(&self, item_id: &str, action_id: &str) -> bool {
        let item = self.state.borrow().all_menu_items.iter().find(|item| item.item_id().as_deref() == Some(item_id)).cloned();
        let Some(item) = item else {
            return false;
        };
        if let Some(section) = item.section.clone() {
            if self.state.borrow().collapsed_sections.contains(&section) {
                self.toggle_section(&section);
            }
        }
        let Some(index) = self.list_index_of(&item) else {
            return false;
        };
        self.set_focus(Some(index));
        self.reveal(index);
        let mut actions: Vec<String> = item.toolbar_actions.iter().flatten().map(|action| IAction::id(action.as_ref())).collect();
        if item.on_remove.is_some() {
            actions.push(REMOVE_TOOLBAR_ACTION_ID.to_owned());
        }
        let Some(action_index) = actions.iter().position(|id| id == action_id) else {
            return false;
        };
        self.focus_toolbar_button(action_index);
        self.focused_toolbar_button().is_some_and(|(focused, _)| focused == action_index)
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

    /// Um clique ou toque na linha `index`, no `target` dela: a lista foca a linha, seleciona e o `onListClick` tira o foco.
    pub fn click_row(&self, index: usize, target: PointerTarget) {
        self.set_focus(Some(index));
        self.set_selection(Some(index), SelectionEvent::Pointer(target));
        if self.element(index).is_some_and(|element| Self::focus_condition(&element)) {
            self.set_focus(None);
        }
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
        if element.open_submenu_on_click && has_expandable_panel(&element) && (is_pointer_activation || event == SelectionEvent::Accept) {
            self.set_selection(None, SelectionEvent::Accept);
            self.show_submenu_for_item(&element);
            if !is_pointer_activation {
                if self.state.borrow().current_submenu_widget.is_some() {
                    self.focus_submenu_widget();
                } else {
                    self.focus_panel();
                }
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

        // Show hover on focus change (suppress during programmatic initial focus)
        let suppress_hover = self.state.borrow().suppress_hover;
        if !suppress_hover {
            if self.opts().is_some_and(|o| o.persistent_hover) {
                self.cancel_submenu_show();
                self.reset_submenu_pointer();
            }
            self.show_hover_for_element(&element);
        }
    }

    // #endregion

    // #region teclado e filtro

    /// Um `keydown` em `target`: os tratadores do campo de filtro (nesta ordem, no próprio campo), depois os do `domNode`
    /// (ArrowRight que abre o submenu e a digitação que revela o filtro), subindo enquanto ninguém parar a propagação.
    pub fn dispatch_key_down(&self, e: &UiKeyEvent, target: KeyTarget) {
        // `_list.onKeyDown`: a tecla dentro da lista liga a navegação por teclado.
        if target == KeyTarget::List {
            self.set_keyboard_navigation(Some(true));
        }
        // O ouvinte de captura do `domNode` roda antes dos tratadores do alvo.
        self.handle_tab_through_panel_key_down(e);
        if e.propagation_stopped() {
            return;
        }
        match target {
            KeyTarget::FilterInput => self.filter_input_key_down(e),
            KeyTarget::SubmenuPanel => self.submenu_panel_key_down(e),
            KeyTarget::DetailLink => self.detail_link_key_down(e),
            KeyTarget::List | KeyTarget::EmbeddedControl | KeyTarget::DomNode => {}
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
            // Typing in a submenu filter must not reach the parent list, nor the keybindings the action widget owns.
            let resolver = self.keybinding_resolver.borrow().clone();
            let command_id = resolver.and_then(|resolve| resolve(e));
            let is_action_widget_keybinding = command_id.as_deref().is_some_and(is_action_widget_keybinding_command);
            if is_action_widget_keybinding || (e.key.chars().count() == 1 && !e.ctrl_key && !e.meta_key && !e.alt_key) {
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
                    if has_expandable_panel(&element) {
                        e.stop();
                        self.show_submenu_for_element(&element);
                        if element.hover.as_ref().is_some_and(|hover| hover.tab_through_panel) {
                            self.focus_first_tab_through_panel_control(&element);
                        } else if self.state.borrow().current_submenu_widget.is_some() {
                            self.focus_submenu_widget();
                        } else {
                            self.focus_panel();
                        }
                    }
                }
            }
        }

        if self.filter_input.is_some() || self.opts().is_some_and(|o| o.on_type.is_some()) {
            let on_type = self.opts().is_some_and(|o| o.on_type.is_some());
            let combobox = self.opts().is_some_and(|o| o.filter_as_combobox);
            let is_editable_target = matches!(target, KeyTarget::FilterInput | KeyTarget::EmbeddedControl | KeyTarget::DetailLink);
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
        let preserved_scroll_top = options.preserve_scroll_position.then(|| self.state.borrow().scroll_top);
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
        // O conteúdo `() => HTMLElement` do item preservado é construído aqui, como no original, para saber se o painel o
        // contém ainda.
        let preserved_content = preserved_item.as_ref().and_then(|item| match item.hover.as_ref().and_then(|hover| hover.content.clone()) {
            Some(HoverContent::Lazy(build)) => Some(build()),
            Some(HoverContent::Element(element)) => Some(element),
            _ => None,
        });
        let preserve_hover = !has_submenu_widget
            && preserved_content.as_ref().is_some_and(|content| {
                matches!(&self.state.borrow().panel_header, Some(PanelHeader::Element(shown)) if Rc::ptr_eq(shown, content))
            });
        let preserve_submenu = has_submenu_widget && preserved_item.as_ref().is_some_and(|item| has_submenu_actions(item));
        let preserve_panel = preserve_hover || preserve_submenu;
        // A content element built by a function that the panel did not keep is released by the item that built it.
        if !preserve_hover {
            let built = preserved_item.as_ref().is_some_and(|item| matches!(item.hover.as_ref().and_then(|hover| hover.content.as_ref()), Some(HoverContent::Lazy(_))));
            if let (true, Some(item), Some(content)) = (built, &preserved_item, &preserved_content) {
                if let Some(dispose_content) = item.hover.as_ref().and_then(|hover| hover.dispose_content.clone()) {
                    dispose_content(content);
                }
            }
        }
        let previous_row = if options.animate_item_move && preserve_panel {
            let current = self.state.borrow().current_submenu_element.clone();
            current.and_then(|current| self.list_index_of(&current)).map(|index| self.row_rect(index))
        } else {
            None
        };

        self.state.borrow_mut().all_menu_items = items.to_vec();
        if let (true, Some(preserved_item)) = (preserve_panel, &preserved_item) {
            let mut state = self.state.borrow_mut();
            state.current_submenu_element = Some(preserved_item.clone());
            state.expanded_trigger = Some(preserved_item.clone());
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
        // Restoring focus after a passive refresh must not open an unrequested hover.
        let suppress = preserve_panel || (self.state.borrow().current_submenu_element.is_none() && !self.opts().is_some_and(|o| o.persistent_hover));
        self.with_hover_suppressed(suppress, |widget| widget.apply_filter(false, false, focus_for_filter));
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
            if let (Some(previous_row), Some(preserved_item)) = (previous_row, &preserved_item) {
                self.animate_item_move(preserved_item, previous_row);
            }
        } else if let Some(focus_item_id) = focus_item_id {
            self.focus_item_by_id(focus_item_id);
        }
        if let Some(scroll_top) = preserved_scroll_top {
            self.set_scroll_top(scroll_top);
        }
    }

    /// `_animateItemMove`: the row that kept the open panel and changed place slides from where it was, unless motion is reduced,
    /// it did not move, or either position lies outside the list.
    fn animate_item_move(&self, item: &Rc<IActionListItem<T>>, previous: IRect) {
        if self.state.borrow().reduce_motion {
            return;
        }
        let Some(index) = self.list_index_of(item) else {
            return;
        };
        let current = self.row_rect(index);
        let bounds = self.dom_node.rect();
        let bounds_bottom = bounds.top + bounds.height;
        if previous.top < bounds.top
            || previous.top + previous.height > bounds_bottom
            || current.top < bounds.top
            || current.top + current.height > bounds_bottom
            || (previous.top - current.top).abs() < 1.0
        {
            return;
        }
        self.state.borrow_mut().item_move_animation = Some((ITEM_MOVE_DURATION_MS, ITEM_MOVE_EASING));
    }

    /// `row.getAnimations()`: the duration (ms) and easing of the running move animation, if any.
    pub fn item_move_animations(&self) -> Vec<(u64, String)> {
        self.state.borrow().item_move_animation.map(|(duration, easing)| (duration, easing.to_owned())).into_iter().collect()
    }

    /// `animation.finish()`.
    pub fn finish_item_move_animations(&self) {
        self.state.borrow_mut().item_move_animation = None;
    }

    /// `prefers-reduced-motion: reduce`.
    pub fn set_reduce_motion(&self, reduce: bool) {
        self.state.borrow_mut().reduce_motion = reduce;
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
            self.focus_state.leave_panel();
            widget.focus();
        }
    }

    fn show_submenu_for_element(&self, element: &Rc<IActionListItem<T>>) {
        let already_current = self.state.borrow().current_submenu_element.as_ref().is_some_and(|current| Rc::ptr_eq(current, element));
        if !self.state.borrow().hover_enabled || already_current {
            return;
        }

        if let Some(parent_widget) = self.dom_node.parent_element().and_then(|parent| parent.closest("action-widget")) {
            parent_widget.finish_opening_animation();
        }
        self.state.borrow_mut().current_submenu_element = Some(element.clone());
        self.clear_submenu_container();

        // Marks the row the panel belongs to as open, so a screen reader can tell what ArrowRight opened.
        self.state.borrow_mut().expanded_trigger = Some(element.clone());

        let submenu_actions = element.submenu_actions.clone().filter(|actions| !actions.is_empty());

        // When the item has hover content, render it as a header.
        let hover_content = element.hover.as_ref().and_then(|hover| hover.content.clone());
        let header = hover_content.map(|content| match content {
            HoverContent::Lazy(build) => PanelHeader::Element(build()),
            HoverContent::Element(element) => PanelHeader::Element(element),
            HoverContent::Text(text) => PanelHeader::Text(text),
            HoverContent::Markdown { text, markdown } => {
                // `appendText` is literal and the line break it adds renders as `<br>`, so only the markdown part is parsed.
                let (rendered, links) = render_markdown(&markdown);
                PanelHeader::Rendered { text: format!("{text}{rendered}"), links }
            }
        });
        if let (Some(PanelHeader::Element(_)), Some(disposable)) = (&header, element.hover.as_ref().and_then(|hover| hover.disposable.clone())) {
            // The hover element is owned by the caller and reused across shows, so its disposable lives as long as the widget.
            self.panel_subscriptions.add(disposable);
        }
        let footer: Vec<(Rc<HoverElement>, IHoverAction)> = element
            .hover
            .as_ref()
            .map(|hover| hover.actions.iter().map(|action| (HoverElement::new(&action.label), action.clone())).collect())
            .unwrap_or_default();
        {
            let mut state = self.state.borrow_mut();
            // `hoverHeader.offsetWidth`: the width the panel keeps while the content is shown.
            if let Some(PanelHeader::Element(content)) = &header {
                state.submenu_max_width = content.size().0;
            }
            state.panel_header = header;
            state.panel_footer = footer;
            // Show container before creating the widget so the list can measure during construction.
            state.panel_hidden = false;
            state.panel_animated = false;
        }

        if let Some(submenu_actions) = submenu_actions {
            let submenu_items = Self::create_submenu_items(&submenu_actions);
            let delegate: Rc<dyn IActionListDelegate<ActionRef>> = Rc::new(SubmenuDelegate::<T> { parent: self.this.clone() });
            let widget = ActionListWidget::<ActionRef>::new(false, &submenu_items, delegate, element.submenu_options.clone());
            widget.is_submenu_panel.set(true);
            let resolver = self.keybinding_resolver.borrow().clone();
            if let Some(resolver) = resolver {
                widget.set_keybinding_resolver(resolver);
            }
            self.state.borrow_mut().current_submenu_widget = Some(widget.clone());

            // The submenu widget's constructor focuses its first item by default; clear that until the user actually navigates
            // into the submenu (via ArrowRight) so it doesn't render as if selected while the parent list still has focus.
            widget.clear_focus();

            let header_width = match &self.state.borrow().panel_header {
                Some(PanelHeader::Element(content)) => content.size().0,
                _ => 0.0,
            };
            let total_height = widget.compute_list_height();
            widget.layout(total_height, None);
            let max_width = header_width.max(widget.compute_max_width(0.0));
            widget.layout(total_height, Some(max_width));
            widget.dom_node.set_size(max_width, total_height);
            widget.dom_node.style_set_property("width", &css_px(max_width));
            {
                let mut state = self.state.borrow_mut();
                state.submenu_max_width = max_width;
                if element.submenu_options.is_some() {
                    state.panel_style_width = Some(max_width + 10.0);
                }
            }

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
        }
        {
            let mut state = self.state.borrow_mut();
            state.has_layout_submenu = true;
            state.panel_animated = state.current_submenu_widget.is_some();
        }
        self.layout_submenu();
        self.observe_panel_content(element);
    }

    /// The `ResizeObserver` of `_showSubmenuForElement`: content that grows or shrinks (for instance after focus reveals
    /// bounded text) repositions the panel, not just the row that measured it before the content changed.
    fn observe_panel_content(&self, element: &Rc<IActionListItem<T>>) {
        let hover = element.hover.clone().unwrap_or_default();
        let persistent = self.opts().is_some_and(|o| o.persistent_hover);
        let observes = persistent || hover.align_to_parent || hover.tab_through_panel || hover.preserve_vertical_position || hover.align_to_parent_bottom;
        let (current, has_submenu_widget, header) = {
            let state = self.state.borrow();
            (state.current_submenu_element.clone(), state.current_submenu_widget.is_some(), state.panel_header.clone())
        };
        let is_current = current.is_some_and(|current| Rc::ptr_eq(&current, element));
        let (true, true, false, Some(PanelHeader::Element(content))) = (observes, is_current, has_submenu_widget, header) else {
            return;
        };
        let weak = self.this.clone();
        let listener: Rc<dyn Fn()> = Rc::new(move || {
            if let Some(widget) = weak.upgrade() {
                widget.schedule_panel_layout();
            }
        });
        content.observe_resize(&listener);
        self.state.borrow_mut().panel_resize_listener = Some(listener);
    }

    /// Layout can resize the observed panel, so it runs at the next animation frame, outside the resize observation.
    fn schedule_panel_layout(&self) {
        if self.state.borrow().panel_layout_timer.is_some() {
            return;
        }
        let weak = self.this.clone();
        let timer = set_timeout(
            move || {
                if let Some(widget) = weak.upgrade() {
                    widget.state.borrow_mut().panel_layout_timer = None;
                    widget.layout_submenu();
                }
            },
            ANIMATION_FRAME_MS,
        );
        self.state.borrow_mut().panel_layout_timer = Some(timer);
    }

    /// A host that writes the panel's `style.width` (a resize of the panel) is undone at the next frame: the panel goes back to
    /// the width its content asked for when it opened.
    pub fn set_panel_width(&self, width: f64) {
        {
            let mut state = self.state.borrow_mut();
            if let Some(rect) = state.panel_rect.as_mut() {
                rect.2 = width;
            }
            state.panel_style_width = Some(width);
        }
        self.schedule_panel_layout();
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
            return;
        }
        let trigger_differs = !self.state.borrow().expanded_trigger.as_ref().is_some_and(|trigger| Rc::ptr_eq(trigger, &current));
        if trigger_differs {
            self.state.borrow_mut().expanded_trigger = if row_exists { Some(current.clone()) } else { None };
        }
        self.state.borrow_mut().layout_submenu_count += 1;
        self.layout_hover_panel(&current, self.list_index_of(&current));
    }

    /// `getBoundingClientRect()` da linha `index`: as alturas acima dela, com o zoom do widget.
    fn row_rect(&self, index: usize) -> IRect {
        let (zoom, scroll_top) = {
            let state = self.state.borrow();
            (state.zoom, state.scroll_top)
        };
        let parent = self.dom_node.rect();
        let items = self.state.borrow().visible_menu_items.clone();
        let above: f64 = items.iter().take(index).map(|item| Self::item_height_with(item, self.opts())).sum();
        let height = items.get(index).map_or(0.0, |item| Self::item_height_with(item, self.opts()));
        IRect { top: parent.top + (above - scroll_top) * zoom, left: parent.left, width: parent.width, height: height * zoom }
    }

    /// O posicionamento do painel de hover de conteúdo (o ramo sem lista de submenu do `layout` de `_showSubmenuForElement`).
    fn layout_hover_panel(&self, current: &Rc<IActionListItem<T>>, index: Option<usize>) {
        let (zoom_level, (window_width, window_height), header, has_submenu_widget) = {
            let state = self.state.borrow();
            (state.zoom, state.window_size, state.panel_header.clone(), state.current_submenu_widget.is_some())
        };
        if has_submenu_widget {
            self.layout_submenu_list_panel(current, index);
            return;
        }
        let Some(PanelHeader::Element(content)) = header else {
            self.state.borrow_mut().panel_rect = None;
            return;
        };
        let (max_width, opening_top, opening_height, previous_viewport, previous_panel, previous_scroll, previous_side, has_footer) = {
            let state = self.state.borrow();
            (
                state.submenu_max_width,
                state.opening_panel_top,
                state.opening_panel_height,
                state.panel_viewport_height,
                state.panel_rect,
                state.panel_scroll_top,
                state.submenu_side,
                !state.panel_footer.is_empty(),
            )
        };
        let hover = current.hover.clone().unwrap_or_default();
        let persistent = self.opts().is_some_and(|o| o.persistent_hover);
        let preserve = hover.preserve_vertical_position;
        let align_to_parent = persistent || hover.align_to_parent || hover.align_to_parent_bottom;
        let zoom = if align_to_parent || preserve { zoom_level } else { 1.0 };
        let parent_rect = self.dom_node.rect();
        let edge_rect = if align_to_parent {
            self.dom_node.parent_element().and_then(|parent| parent.closest("action-widget")).map_or(parent_rect, |widget| widget.rect())
        } else {
            parent_rect
        };
        let (_, content_height) = content.size();
        // The footer row sits flush with the panel's inner edges (negative margin), so it adds its height minus the padding.
        let footer_extra = if has_footer { HOVER_FOOTER_HEIGHT - SUBMENU_PANEL_PADDING } else { 0.0 };
        let chrome = ACTION_WIDGET_VERTICAL_CHROME_HEIGHT + footer_extra;
        let scrollable = preserve || hover.align_to_parent_bottom || (!persistent && !hover.align_to_parent);

        let mut panel_width = if persistent {
            edge_rect.width
        } else if align_to_parent {
            (max_width + ACTION_WIDGET_VERTICAL_CHROME_HEIGHT) * zoom
        } else {
            max_width + ACTION_WIDGET_VERTICAL_CHROME_HEIGHT
        };
        let gap = if align_to_parent { 0.0 } else { 4.0 };
        let viewport_margin = 4.0;
        let edge_right = edge_rect.left + edge_rect.width;
        let space_right = window_width - viewport_margin - edge_right - gap;
        let space_left = edge_rect.left - viewport_margin - gap;
        let mut style_width = if persistent { Some(edge_rect.width / zoom) } else { None };
        if !align_to_parent {
            // Keep previews beside their row while retaining a readable content width.
            let minimum_panel_width = panel_width.min(240.0);
            let available_side_width = space_right.max(space_left);
            if available_side_width >= minimum_panel_width {
                panel_width = panel_width.min(available_side_width);
            }
            // On a narrow viewport neither side may have room for the panel next to its anchor: clamp it to what fits on screen.
            let available_width = window_width - 2.0 * viewport_margin;
            if panel_width > available_width {
                panel_width = available_width.max(0.0);
            }
            style_width = Some(panel_width);
        }
        let mut show_right = space_right >= panel_width || space_right >= space_left;
        if persistent {
            if let Some(side_is_right) = previous_side {
                if (if side_is_right { space_right } else { space_left }) >= panel_width {
                    show_right = side_is_right;
                }
            }
        }
        let mut left = if show_right { edge_right - parent_rect.left + gap } else { edge_rect.left - parent_rect.left - panel_width - gap };
        let page_left = parent_rect.left + left;
        if page_left < viewport_margin {
            left += viewport_margin - page_left;
        } else if page_left + panel_width > window_width - viewport_margin {
            left -= (page_left + panel_width) - (window_width - viewport_margin);
        }

        // The scroll viewport still has its previous height when content changes, so a panel that keeps its position
        // reads the height it has now; otherwise it follows the content.
        let shown_viewport = if preserve { previous_viewport.unwrap_or(content_height) } else { content_height };
        let panel_height = (shown_viewport + chrome) * zoom;
        let opening_height = if preserve && opening_height.is_none() { Some(panel_height / zoom) } else { opening_height };
        let anchor_height = opening_height.map_or(panel_height, |height| height * zoom);
        let anchor_rect = index.map(|index| self.row_rect(index));
        let edge_bottom = edge_rect.top + edge_rect.height;
        let bottom_boundary = if hover.align_to_parent_bottom { edge_bottom.min(window_height) } else { window_height };
        let mut top = if hover.align_to_parent_bottom {
            bottom_boundary - parent_rect.top - anchor_height
        } else if let Some(opening_top) = opening_top {
            opening_top * zoom
        } else if let Some(row) = anchor_rect {
            if hover.align_to_anchor_top { row.top - parent_rect.top } else { row.top - parent_rect.top + (row.height - anchor_height) / 2.0 }
        } else {
            previous_panel.map_or(parent_rect.top, |(panel_top, _, _, _)| panel_top) - parent_rect.top
        };
        let keeps_anchor_top = preserve && hover.align_to_anchor_top;
        let panel_bottom = parent_rect.top + top + anchor_height;
        if panel_bottom > bottom_boundary && !keeps_anchor_top {
            top -= panel_bottom - bottom_boundary + 8.0;
        }
        if parent_rect.top + top < 0.0 {
            top = -parent_rect.top;
        }
        let new_opening_top = if preserve && opening_top.is_none() { Some(top / zoom) } else { opening_top };
        let available_panel_height = if hover.align_to_parent_bottom { bottom_boundary - 8.0 } else { window_height - parent_rect.top - top - 8.0 };
        let (viewport_height, height) = if scrollable {
            let available_height = (available_panel_height / zoom - chrome).max(0.0);
            let viewport_height = content_height.min(available_height);
            (Some(viewport_height), (viewport_height + chrome) * zoom)
        } else {
            (None, panel_height)
        };
        if hover.align_to_parent_bottom {
            top = (bottom_boundary - height).max(0.0) - parent_rect.top;
        }
        let scroll_top = viewport_height.map_or(0.0, |viewport| previous_scroll.min((content_height - viewport).max(0.0)).max(0.0));

        let mut state = self.state.borrow_mut();
        state.opening_panel_height = opening_height;
        state.opening_panel_top = new_opening_top;
        state.panel_viewport_height = viewport_height;
        state.panel_content_height = content_height;
        state.panel_scroll_top = scroll_top;
        state.panel_zoom = zoom;
        if persistent {
            state.submenu_side = Some(show_right);
        }
        state.panel_style_top = Some(top / zoom);
        state.panel_style_left = Some(left / zoom);
        if style_width.is_some() {
            state.panel_style_width = style_width;
        }
        state.panel_rect = Some((parent_rect.top + top, parent_rect.left + left, panel_width, height));
    }


    /// O ramo com lista de submenu do `layout` de `_showSubmenuForElement`: largura, lado, topo e a altura da lista dentro do
    /// painel. O painel só tem o viewport que rola quando o item preserva a posição vertical ou alinha pelo rodapé do pai.
    fn layout_submenu_list_panel(&self, current: &Rc<IActionListItem<T>>, index: Option<usize>) {
        let (zoom_level, (window_width, window_height), header, submenu, max_width, opening_top, opening_height, previous_viewport, previous_panel) = {
            let state = self.state.borrow();
            (
                state.zoom,
                state.window_size,
                state.panel_header.clone(),
                state.current_submenu_widget.clone(),
                state.submenu_max_width,
                state.opening_panel_top,
                state.opening_panel_height,
                state.panel_viewport_height,
                state.panel_rect,
            )
        };
        let Some(submenu) = submenu else {
            return;
        };
        let hover = current.hover.clone().unwrap_or_default();
        let persistent = self.opts().is_some_and(|o| o.persistent_hover);
        let preserve = hover.preserve_vertical_position;
        let has_viewport = preserve || hover.align_to_parent_bottom;
        let align_to_parent = persistent || hover.align_to_parent || hover.align_to_parent_bottom;
        let parent_rect = self.dom_node.rect();
        let edge_rect = if align_to_parent {
            self.dom_node.parent_element().and_then(|parent| parent.closest("action-widget")).map_or(parent_rect, |widget| widget.rect())
        } else {
            parent_rect
        };
        let anchor_rect = index.map(|index| self.row_rect(index));
        let zoom = if align_to_parent || preserve { zoom_level } else { 1.0 };
        let chrome = ACTION_WIDGET_VERTICAL_CHROME_HEIGHT;

        let header_height = match &header {
            Some(PanelHeader::Element(content)) => content.size().1,
            _ => 0.0,
        };
        let offset_height = |element: Option<Rc<ViewElement>>| element.map_or(0.0, |element| element.size().height);
        let submenu_chrome = offset_height(submenu.header_container()) + offset_height(submenu.filter_container()) + offset_height(submenu.footer_container());
        let total_height = submenu.compute_list_height();
        // The scroll viewport still has its previous height when content changes.
        let viewport_height = previous_viewport.unwrap_or(header_height + submenu_chrome + total_height);

        let mut panel_width = if persistent {
            edge_rect.width
        } else if align_to_parent {
            (max_width + chrome) * zoom
        } else {
            max_width + chrome
        };
        let gap = if align_to_parent { 0.0 } else { 4.0 };
        let viewport_margin = 4.0;
        let edge_right = edge_rect.left + edge_rect.width;
        let space_right = window_width - viewport_margin - edge_right - gap;
        let space_left = edge_rect.left - viewport_margin - gap;

        // On a narrow viewport neither side may have room for the panel next to its anchor: clamp it to what fits on screen.
        let mut style_width = if persistent { Some(edge_rect.width / zoom) } else { None };
        if !align_to_parent {
            let available_width = window_width - 2.0 * viewport_margin;
            if panel_width > available_width {
                panel_width = available_width.max(0.0);
            }
            style_width = Some(panel_width);
        }

        let show_right = space_right >= panel_width || space_right >= space_left;
        let mut left = if show_right { edge_right - parent_rect.left + gap } else { edge_rect.left - parent_rect.left - panel_width - gap };
        let page_left = parent_rect.left + left;
        if page_left < viewport_margin {
            left += viewport_margin - page_left;
        } else if page_left + panel_width > window_width - viewport_margin {
            left -= (page_left + panel_width) - (window_width - viewport_margin);
        }

        let panel_height = (chrome + viewport_height) * zoom;
        let mut state_opening_height = opening_height;
        if preserve && state_opening_height.is_none() {
            state_opening_height = Some(panel_height / zoom);
        }
        let anchor_height = state_opening_height.map_or(panel_height, |height| height * zoom);
        let edge_bottom = edge_rect.top + edge_rect.height;
        let bottom_boundary = if hover.align_to_parent_bottom { edge_bottom.min(window_height) } else { window_height };
        let mut top = if hover.align_to_parent_bottom {
            bottom_boundary - parent_rect.top - anchor_height
        } else if let Some(opening_top) = opening_top {
            opening_top * zoom
        } else if let (Some(anchor), true) = (anchor_rect, hover.align_to_anchor_top) {
            anchor.top - parent_rect.top
        } else if let Some(anchor) = anchor_rect {
            anchor.top - parent_rect.top + (anchor.height - anchor_height) / 2.0
        } else {
            previous_panel.map_or(parent_rect.top, |(panel_top, _, _, _)| panel_top) - parent_rect.top
        };
        let keeps_anchor_top = preserve && hover.align_to_anchor_top;
        if keeps_anchor_top && has_viewport {
            let desired_panel_height = chrome * zoom + (submenu_chrome + total_height) * zoom;
            top = top.min(window_height - parent_rect.top - desired_panel_height - 8.0);
        }
        let panel_bottom = parent_rect.top + top + anchor_height;
        if panel_bottom > bottom_boundary && !keeps_anchor_top {
            top -= panel_bottom - bottom_boundary + 8.0;
        }
        if parent_rect.top + top < 0.0 {
            top = -parent_rect.top;
        }
        let state_opening_top = if preserve && opening_top.is_none() { Some(top / zoom) } else { opening_top };
        let available_panel_height = if hover.align_to_parent_bottom { bottom_boundary - 8.0 } else { window_height - parent_rect.top - top - 8.0 };
        let mut new_viewport = previous_viewport;
        let mut new_panel_height = panel_height;
        if has_viewport {
            let available_height = (available_panel_height / zoom - chrome).max(0.0);
            let submenu_height = if total_height == 0.0 { 0.0 } else { ACTION_LINE_HEIGHT.max(total_height.min(available_height - submenu_chrome)) };
            submenu.layout(submenu_height, Some(max_width));
            new_viewport = Some(submenu_chrome + submenu_height);
            new_panel_height = (chrome + submenu_chrome + submenu_height) * zoom;
        }
        if hover.align_to_parent_bottom {
            top = (bottom_boundary - new_panel_height).max(0.0) - parent_rect.top;
        }

        let mut state = self.state.borrow_mut();
        state.opening_panel_height = state_opening_height;
        state.opening_panel_top = state_opening_top;
        state.panel_viewport_height = new_viewport;
        state.panel_style_top = Some(top / zoom);
        state.panel_style_left = Some(left / zoom);
        if style_width.is_some() {
            state.panel_style_width = style_width;
        }
        state.panel_rect = Some((parent_rect.top + top, parent_rect.left + left, panel_width, new_panel_height));
    }
    pub fn panel_rect(&self) -> Option<IRect> {
        self.state.borrow().panel_rect.map(|(top, left, width, height)| IRect { top, left, width, height })
    }

    /// `getDomNodeZoomLevel(domNode)`: o zoom CSS do ancestral que a lista lê.
    pub fn set_zoom_level(&self, zoom: f64) {
        self.state.borrow_mut().zoom = zoom;
    }

    /// `window.innerWidth` e `window.innerHeight` da janela da lista.
    pub fn set_window_size(&self, width: f64, height: f64) {
        self.state.borrow_mut().window_size = (width, height);
    }

    /// `row.getBoundingClientRect()` de uma linha visível, para o teste conferir a geometria.
    pub fn visible_row_rect(&self, index: usize) -> IRect {
        self.row_rect(index)
    }

    /// A geometria que o motor de layout do DOM calcularia para a barra de ferramentas da linha `index` (`actionWidget.css`):
    /// `focused_or_hovered` é a classe `focused` (ou o `:hover`) da linha. `None` se a linha não tem barra.
    pub fn row_toolbar_geometry(&self, index: usize, focused_or_hovered: bool) -> Option<RowToolbarGeometry> {
        let element = self.element(index)?;
        let mut action_count = element.toolbar_actions.as_ref().map_or(0, Vec::len);
        if element.on_remove.is_some() {
            action_count += 1;
        }
        if element.kind != ActionListItemKind::Action || action_count == 0 {
            return None;
        }
        let zoom = self.state.borrow().zoom;
        let row = self.row_rect(index);
        let has_detail = element.detail.as_deref().is_some_and(|detail| !detail.is_empty());
        // `.has-detail.has-toolbar` mantém a barra no fluxo (`display: flex`) e só esconde com `visibility`, para a linha não
        // mudar de geometria quando a barra aparece; nas outras linhas a barra só entra no fluxo com o foco ou o ponteiro.
        let (display, visibility) = match (has_detail, focused_or_hovered) {
            (_, true) => ("flex", "visible"),
            (true, false) => ("flex", "hidden"),
            (false, false) => ("none", "visible"),
        };
        let margin_right = if has_detail { ROW_DETAIL_TOOLBAR_MARGIN_RIGHT } else { 0.0 };
        let padding_right = if has_detail { ROW_DETAIL_PADDING_RIGHT } else { ROW_PADDING_RIGHT };
        // Com detalhe a linha quebra em duas: a primeira tem a maior das caixas (título e barra), a segunda tem o detalhe, e o
        // conteúdo fica centrado na altura da linha (`align-content: center`).
        let detail_top = if has_detail {
            let first_line = DETAIL_TITLE_LINE_HEIGHT.max(TOOLBAR_HEIGHT);
            let content_height = first_line + DETAIL_LINE_HEIGHT;
            row.top + ((row.height / zoom - content_height) / 2.0 + first_line) * zoom
        } else {
            row.top
        };
        Some(RowToolbarGeometry {
            row_height: row.height / zoom,
            detail_top,
            display,
            visibility,
            margin_right,
            right_inset: padding_right + margin_right,
        })
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
                icon_color: None,
            });
            item.tooltip = Some(if tooltip.is_empty() { label.clone() } else { tooltip });
            item.hover = hover_content_of(action)
                .map(|markdown| IActionListItemHover { content: Some(HoverContent::Markdown { text: label, markdown }), ..IActionListItemHover::default() });
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
                header.group = Some(ActionListGroup { icon: None, icon_color: None, title: group_label.clone() });
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
        self.cancel_submenu_hide();
        self.cancel_submenu_show();
        self.state.borrow_mut().current_submenu_element = None;
        self.clear_submenu_container();
        self.state.borrow_mut().panel_hidden = true;
    }

    /// `_clearSubmenuContainer()`: if focus currently lives inside the panel, it is first moved back to the list.
    fn clear_submenu_container(&self) {
        self.cancel_submenu_hide();
        self.cancel_submenu_show();
        self.reset_submenu_pointer();
        if self.panel_contains_focus() {
            self.dom_focus_list();
        }
        let (widget, layout_timer) = {
            let mut state = self.state.borrow_mut();
            state.has_layout_submenu = false;
            state.panel_animated = false;
            state.panel_header = None;
            state.panel_footer = Vec::new();
            state.panel_resize_listener = None;
            state.panel_scroll_top = 0.0;
            state.panel_content_height = 0.0;
            state.panel_rect = None;
            state.opening_panel_top = None;
            state.opening_panel_height = None;
            state.submenu_max_width = 0.0;
            state.panel_style_top = None;
            state.panel_style_left = None;
            state.panel_style_width = None;
            state.panel_viewport_height = None;
            // The row that opened the panel is no longer expanded.
            state.expanded_trigger = None;
            (state.current_submenu_widget.take(), state.panel_layout_timer.take())
        };
        if let Some(timer) = layout_timer {
            clear_timeout(timer);
        }
        if let Some(widget) = widget {
            widget.dispose();
        }
    }

    fn cancel_submenu_show(&self) {
        let timer = {
            let mut state = self.state.borrow_mut();
            state.pending_submenu_element = None;
            state.submenu_show_timer.take()
        };
        if let Some(timer) = timer {
            clear_timeout(timer);
        }
    }

    fn cancel_submenu_hide(&self) {
        let timer = self.state.borrow_mut().submenu_hide_timer.take();
        if let Some(timer) = timer {
            clear_timeout(timer);
        }
    }

    fn schedule_submenu_hide(&self) {
        self.cancel_submenu_hide();
        if self.opts().is_some_and(|o| o.persistent_hover) {
            return;
        }
        let weak = self.this.clone();
        let timer = set_timeout(
            move || {
                if let Some(widget) = weak.upgrade() {
                    widget.state.borrow_mut().submenu_hide_timer = None;
                    widget.hide_submenu();
                }
            },
            300,
        );
        self.state.borrow_mut().submenu_hide_timer = Some(timer);
    }

    fn uses_submenu_pointer_intent(&self) -> bool {
        self.opts().is_some_and(|o| o.persistent_hover || o.submenu_pointer_intent)
    }

    fn update_submenu_pointer(&self, event: &PointerEvent) {
        let mut state = self.state.borrow_mut();
        state.submenu_pointer_origin = Some((event.client_x, event.client_y));
        state.submenu_pointer_grace_deadline = None;
    }

    fn reset_submenu_pointer(&self) {
        let mut state = self.state.borrow_mut();
        state.submenu_pointer_origin = None;
        state.submenu_pointer_grace_deadline = None;
    }

    /// `new dom.SafeTriangle(origin.x, origin.y, panel).contains(x, y)`: os quatro triângulos da origem para as arestas
    /// do retângulo do painel (os pontos são `Int16Array`, então truncam).
    fn safe_triangle_contains(&self, origin: (f64, f64), x: f64, y: f64) -> bool {
        let Some(panel) = self.panel_rect() else {
            return false;
        };
        let to_i16 = |value: f64| f64::from(value as i16);
        let (left, top, right, bottom) = (to_i16(panel.left), to_i16(panel.top), to_i16(panel.left + panel.width), to_i16(panel.top + panel.height));
        let points = [(left, top), (right, top), (left, bottom), (right, bottom)];
        (0..4).any(|i| {
            let (p1, p2) = (points[i], points[(i + 1) % 4]);
            je_base::numbers::is_point_within_triangle(x, y, origin.0, origin.1, p1.0, p1.1, p2.0, p2.1)
        })
    }

    fn schedule_submenu_show(&self, element: &Rc<IActionListItem<T>>, pointer: PointerEvent) {
        self.cancel_submenu_show();
        self.state.borrow_mut().pending_submenu_element = Some(element.clone());
        let mut delay = self.opts().and_then(|o| o.submenu_hover_delay).unwrap_or(500);
        if self.uses_submenu_pointer_intent() {
            delay = 0;
            let origin = self.state.borrow().submenu_pointer_origin;
            let has_current = self.state.borrow().current_submenu_element.is_some();
            if let Some(origin) = origin {
                if pointer.client_x != origin.0 && has_current && self.safe_triangle_contains(origin, pointer.client_x, pointer.client_y) {
                    // Give a diagonal path into the current card a bounded grace period.
                    let now = je_base::runtime::date_now();
                    let deadline = *self.state.borrow_mut().submenu_pointer_grace_deadline.get_or_insert(now + 200);
                    delay = deadline.saturating_sub(now);
                }
            }
        }
        if delay == 0 {
            self.run_scheduled_submenu_show(pointer);
        } else {
            let weak = self.this.clone();
            let timer = set_timeout(
                move || {
                    if let Some(widget) = weak.upgrade() {
                        widget.state.borrow_mut().submenu_show_timer = None;
                        widget.run_scheduled_submenu_show(pointer);
                    }
                },
                delay,
            );
            self.state.borrow_mut().submenu_show_timer = Some(timer);
        }
    }

    /// O `show` do `_scheduleSubmenuShow`.
    fn run_scheduled_submenu_show(&self, pointer: PointerEvent) {
        let pending = self.state.borrow_mut().pending_submenu_element.take();
        let Some(pending) = pending else {
            return;
        };
        if self.list_index_of(&pending).is_some() {
            self.show_submenu_for_element(&pending);
            if self.uses_submenu_pointer_intent() {
                self.update_submenu_pointer(&pointer);
            }
        }
    }

    /// `_showHoverForElement(element, index)`.
    fn show_hover_for_element(&self, element: &Rc<IActionListItem<T>>) {
        let is_current = self.state.borrow().current_submenu_element.as_ref().is_some_and(|current| Rc::ptr_eq(current, element));
        if is_current {
            return;
        }
        let has_hover_content = element.hover.as_ref().is_some_and(|hover| hover.content.is_some());
        if has_hover_content || has_submenu_actions(element) {
            if self.list_index_of(element).is_some() {
                self.show_submenu_for_element(element);
            }
            return;
        }
        if !self.opts().is_some_and(|o| o.persistent_hover) {
            self.hide_submenu();
        }
    }

    // #region ponteiro

    /// O `mouseover` na linha `index` (`_list.onMouseOver`): ignorado enquanto o ponteiro estacionário não se moveu.
    pub fn pointer_over(&self, index: usize, event: &PointerEvent) {
        if !self.state.borrow().ignore_initial_hover {
            self.on_list_hover(Some(index), event);
        }
    }

    /// O `mousemove` na linha `index`: o ouvinte da lista liga a navegação por mouse; o `initialMouseMove` abre o hover.
    pub fn pointer_move(&self, index: usize, event: &PointerEvent) {
        if event.movement_x != 0.0 || event.movement_y != 0.0 {
            self.set_keyboard_navigation(Some(false));
        }
        let listening = self.state.borrow().initial_mouse_move_listening;
        if listening && (event.movement_x != 0.0 || event.movement_y != 0.0) {
            {
                let mut state = self.state.borrow_mut();
                state.initial_mouse_move_listening = false;
                state.ignore_initial_hover = false;
            }
            self.on_list_hover(Some(index), event);
        }
    }

    /// O `mousedown` na linha: tira o `initialMouseMove`, liga a navegação por mouse e a lista pega o foco do documento.
    pub fn pointer_down(&self) {
        {
            let mut state = self.state.borrow_mut();
            state.initial_mouse_move_listening = false;
            state.ignore_initial_hover = false;
        }
        self.set_keyboard_navigation(Some(false));
        self.dom_focus_list();
    }

    /// O ouvinte `initialMouseMove` (que mapeia o evento para a linha por `data-index`) ainda está registrado.
    pub fn is_mapping_pointer_moves(&self) -> bool {
        self.state.borrow().initial_mouse_move_listening
    }

    /// O ponteiro saiu do `domNode` (`mouseleave`): um painel agendado não pode abrir depois.
    pub fn pointer_leave(&self) {
        self.cancel_submenu_show();
        self.reset_submenu_pointer();
    }

    /// O `mouseenter` no painel (`_submenuContainer`): o painel não esconde mais, e com a intenção do ponteiro o que estava
    /// agendado para abrir é cancelado.
    pub fn panel_pointer_enter(&self) {
        self.cancel_submenu_hide();
        if self.uses_submenu_pointer_intent() {
            self.cancel_submenu_show();
            self.reset_submenu_pointer();
        }
    }

    /// O `mouseleave` do painel (`_submenuContainer`).
    pub fn panel_pointer_leave(&self) {
        self.schedule_submenu_hide();
    }

    /// O `mouseenter` no banner do cabeçalho: ele é cromo, não uma linha, e apontá-lo dispensa o painel da linha (o preview
    /// persistente só cancela o que estava agendado).
    pub fn header_pointer_enter(&self) {
        if self.opts().is_some_and(|o| o.persistent_hover) {
            self.cancel_submenu_show();
            self.reset_submenu_pointer();
        } else {
            self.hide_submenu();
        }
    }

    /// `showHoverForCheckedItem()`: o hover do item marcado, ou do focado quando o preview é persistente.
    pub fn show_hover_for_checked_item(&self) {
        let checked = self
            .state
            .borrow()
            .all_menu_items
            .iter()
            .find(|item| item.kind == ActionListItemKind::Action && item.item.as_ref().is_some_and(ActionListItemData::list_item_checked))
            .cloned();
        let element = checked.or_else(|| if self.opts().is_some_and(|o| o.persistent_hover) { self.get_focused_element() } else { None });
        let Some(element) = element else {
            return;
        };
        if let Some(section) = element.section.clone() {
            if self.state.borrow().collapsed_sections.contains(&section) {
                self.toggle_section(&section);
            }
        }
        if let Some(index) = self.list_index_of(&element) {
            self.set_focus(Some(index));
            self.reveal(index);
            self.show_hover_for_element(&element);
        }
    }

    /// O `mousedown` no botão da barra da linha `index`: a barra sempre cancela o padrão (o foco fica onde estava), e com
    /// `stopToolbarPointerPropagation` o evento também não chega aos tratadores da lista. Devolve `defaultPrevented`.
    pub fn toolbar_pointer_down(&self, index: usize) -> bool {
        let Some(element) = self.element(index) else {
            return false;
        };
        let has_toolbar = element.toolbar_actions.as_ref().is_some_and(|actions| !actions.is_empty()) || element.on_remove.is_some();
        if has_toolbar && !self.opts().is_some_and(|o| o.stop_toolbar_pointer_propagation) {
            self.set_keyboard_navigation(Some(false));
        }
        has_toolbar
    }

    /// O `click` no botão `action_id` da barra da linha `index`: com `stopToolbarPointerPropagation` o clique para na barra;
    /// sem ele sobe até a lista, que não seleciona a linha por um clique na barra.
    pub fn click_toolbar_action(&self, index: usize, action_id: &str) -> Option<Promise<()>> {
        let label = self.element(index)?.label.clone()?;
        if !self.opts().is_some_and(|o| o.stop_toolbar_pointer_propagation) {
            self.click_row(index, PointerTarget::Toolbar);
        }
        self.run_item_toolbar_action(&label, action_id)
    }

    /// O clique no chevron da linha `index`: abre o painel dela sem selecionar a linha.
    pub fn click_submenu_indicator(&self, index: usize) {
        if let Some(element) = self.element(index) {
            if has_submenu_indicator(&element) {
                self.show_submenu_for_item(&element);
            }
        }
    }

    /// `onListHover`.
    fn on_list_hover(&self, index: Option<usize>, event: &PointerEvent) {
        self.set_keyboard_navigation(Some(false));
        let element = index.and_then(|index| self.element(index));
        let Some(element) = element else {
            return;
        };
        let has_hover_content = element.hover.as_ref().is_some_and(|hover| hover.content.is_some());
        let persistent = self.opts().is_some_and(|o| o.persistent_hover);
        if element.item.is_some() && Self::focus_condition(&element) {
            // Hovering inside a toolbar skips the re-render that would destroy the element mid-hover.
            if event.target == PointerTarget::Toolbar {
                if !has_submenu_actions(&element) {
                    self.cancel_submenu_show();
                }
                self.set_focus(None);
                return;
            }

            // Set focus immediately for responsive hover feedback
            let has_panel = has_submenu_actions(&element) || has_hover_content;
            let suppress = has_panel || self.uses_submenu_pointer_intent();
            self.with_hover_suppressed(suppress, |widget| widget.set_focus(index));

            if has_panel {
                let is_current = self.state.borrow().current_submenu_element.as_ref().is_some_and(|current| Rc::ptr_eq(current, &element));
                if is_current {
                    self.cancel_submenu_hide();
                    self.cancel_submenu_show();
                    if self.uses_submenu_pointer_intent() {
                        self.update_submenu_pointer(event);
                    }
                } else {
                    if !self.uses_submenu_pointer_intent() {
                        self.hide_submenu();
                    }
                    self.schedule_submenu_show(&element, *event);
                }
                return;
            }

            let is_current = self.state.borrow().current_submenu_element.as_ref().is_some_and(|current| Rc::ptr_eq(current, &element));
            if is_current {
                self.cancel_submenu_hide();
            } else {
                self.cancel_submenu_show();
                if self.opts().is_some_and(|o| o.submenu_pointer_intent) {
                    self.schedule_submenu_hide();
                } else if !persistent {
                    self.hide_submenu();
                }
            }
        } else if has_hover_content && index.is_some() {
            let is_current = self.state.borrow().current_submenu_element.as_ref().is_some_and(|current| Rc::ptr_eq(current, &element));
            if is_current {
                self.cancel_submenu_hide();
                self.cancel_submenu_show();
            } else {
                if !persistent {
                    self.hide_submenu();
                }
                self.schedule_submenu_show(&element, *event);
            }
        }
    }

    // #endregion

    fn has_focus(&self) -> bool {
        self.focus_state.list_focused.get() || self.filter_input.as_ref().is_some_and(|input| input.is_focused())
    }

    /// Os controles do conteúdo do hover do item aberto (`getTabbableElements`).
    fn panel_tabbable_controls(&self, element: &IActionListItem<T>) -> Vec<Rc<HoverElement>> {
        let mut controls = element.hover.as_ref().and_then(|hover| hover.get_tabbable_elements.clone()).map(|get| get()).unwrap_or_default();
        controls.extend(self.hover_action_elements());
        controls
    }

    /// `_submenuHoverActionElements`: the footer actions of the open panel, in Tab order.
    pub fn hover_action_elements(&self) -> Vec<Rc<HoverElement>> {
        self.state.borrow().panel_footer.iter().map(|(control, _)| control.clone()).collect()
    }

    /// The click on the footer action `index`.
    pub fn click_hover_action(&self, index: usize) {
        let run = self.state.borrow().panel_footer.get(index).map(|(_, action)| action.run.clone());
        if let Some(run) = run {
            run();
        }
    }

    /// `_itemDetailLinks.get(element) ?? []`: o link do detalhe da linha, criado na primeira vez que a linha o renderiza.
    fn item_detail_links(&self, element: &Rc<IActionListItem<T>>) -> Vec<Rc<HoverElement>> {
        let renders_link = element.detail.as_deref().is_some_and(|detail| !detail.is_empty()) && element.detail_link.is_some();
        if !renders_link {
            return Vec::new();
        }
        let mut state = self.state.borrow_mut();
        if let Some((_, link)) = state.detail_links.iter().find(|(item, _)| Rc::ptr_eq(item, element)) {
            return vec![link.clone()];
        }
        let label = element.detail_link.as_ref().map(|link| link.label.clone()).unwrap_or_default();
        let link = HoverElement::new(&label);
        state.detail_links.push((element.clone(), link.clone()));
        vec![link]
    }

    /// `row.querySelector('.detail .monaco-link')` da linha visível `index`.
    pub fn detail_link_element(&self, index: usize) -> Option<Rc<HoverElement>> {
        let element = self.element(index)?;
        self.item_detail_links(&element).into_iter().next()
    }

    /// O controle do conteúdo do hover que é o `document.activeElement`.
    pub fn focused_control(&self) -> Option<Rc<HoverElement>> {
        self.focus_state.control_focused.borrow().clone()
    }

    /// `document.activeElement === control`.
    pub fn is_control_focused(&self, control: &Rc<HoverElement>) -> bool {
        self.focus_state.control_focused.borrow().as_ref().is_some_and(|focused| Rc::ptr_eq(focused, control))
    }

    /// O abridor do `Link` do detalhe: `opener: linkHandler && (() => linkHandler(uri, element))`. Ativar o link nunca seleciona
    /// a linha. Sem `linkHandler`, o original usa o serviço de abertura, que esta lista não recebe: nada é aberto.
    fn open_detail_link(&self, element: &IActionListItem<T>) {
        let Some(link) = &element.detail_link else {
            return;
        };
        if let Some(handler) = self.opts().and_then(|o| o.link_handler.clone()) {
            handler(&link.uri);
        }
    }

    /// `link.click()` no link do detalhe da linha visível `index`.
    pub fn click_detail_link(&self, index: usize) {
        if let Some(element) = self.element(index) {
            self.open_detail_link(&element);
        }
    }

    /// O `keydown` do `Link`: `Enter` abre o destino e não deixa o evento chegar à lista (que selecionaria a linha).
    fn detail_link_key_down(&self, e: &UiKeyEvent) {
        if e.key_code != KeyCode::Enter {
            return;
        }
        let Some(element) = self.state.borrow().focus.and_then(|index| self.element(index)) else {
            return;
        };
        e.stop();
        self.open_detail_link(&element);
    }

    /// `_getTabThroughPanelControls`: abre o painel do item se preciso e devolve os controles dele, com o link do detalhe
    /// na frente.
    fn get_tab_through_panel_controls(&self, element: &Rc<IActionListItem<T>>) -> Vec<Rc<HoverElement>> {
        let is_current = self.state.borrow().current_submenu_element.as_ref().is_some_and(|current| Rc::ptr_eq(current, element));
        if element.hover.as_ref().is_some_and(|hover| hover.tab_through_panel) && !is_current {
            self.show_submenu_for_element(element);
        }
        let mut controls = self.item_detail_links(element);
        controls.extend(self.panel_tabbable_controls(element));
        controls
    }

    /// `_focusFirstTabThroughPanelControl`.
    fn focus_first_tab_through_panel_control(&self, element: &Rc<IActionListItem<T>>) {
        let controls = self.get_tab_through_panel_controls(element);
        if Self::toolbar_length(element) > 0 {
            self.focus_toolbar_button(0);
            return;
        }
        match controls.first() {
            Some(control) => self.focus_control(control.clone()),
            None => self.dom_focus_list(),
        }
    }

    /// `_handleTabThroughPanelKeyDown`: Tab e Shift+Tab percorrem a lista e os controles do painel, e as setas voltam à lista.
    fn handle_tab_through_panel_key_down(&self, e: &UiKeyEvent) {
        if e.is_composing {
            return;
        }
        let Some(index) = self.state.borrow().focus else {
            return;
        };
        let Some(element) = self.element(index) else {
            return;
        };
        if !element.hover.as_ref().is_some_and(|hover| hover.tab_through_panel) && self.item_detail_links(&element).is_empty() {
            return;
        }
        let active_control = self.focus_state.control_focused.borrow().clone();
        let toolbar_position = self.focus_state.toolbar_focused.get();
        let in_panel = self.focus_state.panel_focused.get() || active_control.is_some();

        if (e.key == "ArrowUp" || e.key == "ArrowDown") && (toolbar_position.is_some() || in_panel) {
            e.stop();
            self.dom_focus_list();
            if e.key == "ArrowUp" {
                self.focus_previous();
            } else {
                self.focus_next();
            }
            return;
        }

        // Ctrl/Meta/Alt+Tab are editor- and OS-level shortcuts; only plain Tab and Shift+Tab drive panel traversal.
        if e.key != "Tab" || e.ctrl_key || e.meta_key || e.alt_key {
            return;
        }

        let controls = self.get_tab_through_panel_controls(&element);
        let toolbar_length = Self::toolbar_length(&element);
        let active_position = active_control.as_ref().and_then(|active| controls.iter().position(|control| Rc::ptr_eq(control, active)));
        let list_or_filter_active = self.focus_state.list_focused.get() || self.focus_state.filter_focused.get();
        let list_target = if self.opts().is_some_and(|o| o.filter_as_combobox) && self.filter_input.is_some() { TabTarget::Filter } else { TabTarget::List };
        let target: Option<TabTarget> = if e.shift_key {
            if in_panel {
                match active_position {
                    Some(position) if position > 0 => Some(TabTarget::Control(controls[position - 1].clone())),
                    _ if toolbar_length > 0 => Some(TabTarget::Toolbar(toolbar_length - 1)),
                    _ => Some(TabTarget::List),
                }
            } else if let Some(position) = toolbar_position {
                if position > 0 { Some(TabTarget::Toolbar(position - 1)) } else { Some(list_target) }
            } else {
                None
            }
        } else if list_or_filter_active {
            if toolbar_length > 0 { Some(TabTarget::Toolbar(0)) } else { controls.first().map(|control| TabTarget::Control(control.clone())) }
        } else if let Some(position) = toolbar_position {
            if position + 1 < toolbar_length { Some(TabTarget::Toolbar(position + 1)) } else { controls.first().map(|control| TabTarget::Control(control.clone())) }
        } else {
            active_position.map(|position| controls.get(position + 1).cloned().map_or(TabTarget::List, TabTarget::Control))
        };
        if let Some(target) = target {
            e.stop();
            match target {
                TabTarget::Toolbar(position) => self.focus_toolbar_button(position),
                TabTarget::Control(control) => self.focus_control(control),
                TabTarget::List => self.dom_focus_list(),
                TabTarget::Filter => {
                    if let Some(input) = &self.filter_input {
                        input.focus();
                    }
                }
            }
        }
    }

    /// Os tratadores do painel (`keydown` do `_submenuContainer`): ArrowLeft e Escape voltam à lista; Enter, Espaço e as
    /// setas agem sobre o submenu.
    fn submenu_panel_key_down(&self, e: &UiKeyEvent) {
        let plain = !e.ctrl_key && !e.meta_key && !e.alt_key && !e.shift_key;
        if plain && (e.key_code == KeyCode::PageDown || e.key_code == KeyCode::PageUp) {
            // The scrollable hover pages through its content by one viewport.
            let (viewport, content, has_submenu_widget) = {
                let state = self.state.borrow();
                (state.panel_viewport_height, state.panel_content_height, state.current_submenu_widget.is_some())
            };
            if let (Some(viewport), false) = (viewport, has_submenu_widget) {
                e.stop();
                let delta = if e.key_code == KeyCode::PageDown { viewport } else { -viewport };
                let mut state = self.state.borrow_mut();
                state.panel_scroll_top = (state.panel_scroll_top + delta).clamp(0.0, (content - viewport).max(0.0));
                return;
            }
        }
        if e.key == "Enter" || e.key == " " {
            let current = self.state.borrow().current_submenu_element.clone();
            let active = self.focus_state.control_focused.borrow().clone();
            if let Some(active) = &active {
                let run = self.state.borrow().panel_footer.iter().find(|(control, _)| Rc::ptr_eq(control, active)).map(|(_, action)| action.run.clone());
                if let Some(run) = run {
                    e.stop();
                    run();
                    return;
                }
            }
            if let (Some(current), Some(active)) = (current, active) {
                let owns_control = current.hover.as_ref().is_some_and(|hover| hover.tab_through_panel)
                    && self.panel_tabbable_controls(&current).iter().any(|control| Rc::ptr_eq(control, &active));
                if owns_control {
                    e.stop_propagation();
                    return;
                }
            }
        }
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
        } else if self.opts().is_some_and(|o| o.persistent_hover) {
            let focused = self.get_focused_element();
            if let Some(element) = focused {
                self.show_hover_for_element(&element);
            }
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
        clamp(self.measure_item_widths(measured).into_iter().fold(f64::NEG_INFINITY, f64::max))
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
        self.cancel_submenu_show();
        self.cancel_submenu_hide();
        let timers = {
            let mut state = self.state.borrow_mut();
            [state.prefetch_timer.take(), state.visible_items_update.take()]
        };
        for timer in timers.into_iter().flatten() {
            clear_timeout(timer);
        }
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

// #region linhas renderizadas como estado

/// Uma cor CSS resolvida (`getComputedStyle(...).outlineColor`).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct CssColor {
    pub red: u8,
    pub green: u8,
    pub blue: u8,
    pub alpha: f32,
}

impl CssColor {
    pub const TRANSPARENT: CssColor = CssColor { red: 0, green: 0, blue: 0, alpha: 0.0 };

    pub fn rgb(red: u8, green: u8, blue: u8) -> Self {
        Self { red, green, blue, alpha: 1.0 }
    }
}

/// As métricas do `.action-item-badge` (`actionWidget.css`): sem os tokens de tamanho do workbench valem os valores de
/// reserva do `var(...)`; o selo não tem borda (nem com `--vscode-contrastBorder`) e a altura é o `line-height`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct BadgeMetrics {
    pub padding_vertical: f64,
    pub padding_horizontal: f64,
    pub border_width: f64,
    pub border_radius: f64,
    pub font_size: f64,
    pub height: f64,
}

/// `getComputedStyle(badge)` e a altura do selo, com o `line-height` de 18px do CSS ou o que o chamador sobrescreve.
pub fn action_item_badge_metrics(line_height: Option<f64>) -> BadgeMetrics {
    BadgeMetrics {
        padding_vertical: 0.0,
        padding_horizontal: 6.0,
        border_width: 0.0,
        border_radius: 9999.0,
        font_size: 11.0,
        height: line_height.unwrap_or(18.0),
    }
}

/// A altura (px) do ícone de informação do rótulo do toggle: `font-size: 12px` do `.action-list-item-toggle-info`.
const TOGGLE_INFO_ICON_SIZE: f64 = 12.0;

/// O deslocamento vertical entre o centro do ícone de informação e o centro do texto do rótulo, com `line-height`
/// `line_height` no rótulo: o `.title` que tem o ícone é `display: flex; align-items: center`, então os dois ficam
/// centrados na altura da maior caixa.
pub fn toggle_info_icon_center_offset(line_height: f64) -> f64 {
    let box_height = line_height.max(TOGGLE_INFO_ICON_SIZE);
    let text_center = (box_height - line_height) / 2.0 + line_height / 2.0;
    let icon_center = (box_height - TOGGLE_INFO_ICON_SIZE) / 2.0 + TOGGLE_INFO_ICON_SIZE / 2.0;
    icon_center - text_center
}

/// A calha do chevron da linha: com chevron (`Shown`), só reservada para alinhar as outras linhas (`visibility: hidden`) ou
/// sem espaço (`display: none`).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum SubmenuGutter {
    Shown,
    Hidden,
    #[default]
    None,
}

/// Um selo renderizado: o texto e o `className` do elemento.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct RowBadge {
    pub text: String,
    pub class_name: String,
}

/// Um botão da barra da linha: o `textContent`, o `aria-label` e se mostra o ícone da ação (a classe `codicon`).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct RowToolbarButton {
    pub action_id: String,
    pub text: String,
    pub aria_label: String,
    pub icon: bool,
}

/// O `Switch` do toggle da linha.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct RowSwitch {
    pub checked: bool,
    pub disabled: bool,
    pub aria_label: String,
}

/// O ícone da linha: a classe do codicon, a cor inline (`style.color`) e se está visível.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct RowIcon {
    pub class_name: String,
    pub inline_color: String,
    pub hidden: bool,
}

/// O ícone de informação depois do rótulo do toggle.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct RowInfoIcon {
    pub aria_hidden: String,
    pub is_last_child_of_title: bool,
}

/// O que o `ActionItemRenderer` (e o `SeparatorRenderer`) deixa no DOM de uma linha, como estado: o `renderElement` reescreve
/// tudo a cada reuso da linha, então o estado de cada linha vem só do item e do estado do widget.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct RowState {
    /// `getAriaLabel(element)`.
    pub aria_label: Option<String>,
    /// `container.title`.
    pub title_attribute: String,
    pub icon: RowIcon,
    /// `.title`.textContent.
    pub title_text: String,
    /// O texto do primeiro filho do `.title` (o `action-list-item-toggle-label-text` quando há ícone de informação).
    pub title_label_text: String,
    pub info_icon: Option<RowInfoIcon>,
    /// O `.title` é flex centrado (a regra `:has(> .action-list-item-toggle-info)`).
    pub title_centers_info_icon: bool,
    /// O `.action-item-badge` principal: o texto, `display: none` e `aria-hidden`.
    pub badge_text: String,
    pub badge_hidden: bool,
    pub badge_aria_hidden: String,
    /// O selo vem logo depois do `.title` (`previousElementSibling`).
    pub badge_after_title: bool,
    /// `textContent` dos filhos `.title` e `.action-item-badge`, na ordem visual.
    pub title_and_badge_order: Vec<String>,
    pub additional_badges: Vec<RowBadge>,
    pub toolbar: Vec<RowToolbarButton>,
    pub classes: Vec<String>,
    pub aria_expanded: Option<String>,
    pub aria_haspopup: Option<String>,
    pub switch: Option<RowSwitch>,
    /// Quantos `.action-list-item-inline-toggle-label` a linha tem.
    pub inline_toggle_label_count: usize,
    /// `.action-list-submenu-indicator.has-submenu`.
    pub submenu_indicator: bool,
    /// O estado da calha do chevron (`style.display` e `style.visibility` do `.action-list-submenu-indicator`).
    pub submenu_gutter: SubmenuGutter,
    /// `role` da linha (`getRole`): `option` para ações, `separator` para o resto.
    pub role: String,
    /// `aria-setsize` e `aria-posinset` (`getSetSize` e `getPosInSet`): só as ações contam.
    pub aria_set_size: usize,
    pub aria_pos_in_set: usize,
    /// `.description`.textContent da linha.
    pub description_text: String,
}

fn row_badges(badges: &[ActionListBadge]) -> Vec<RowBadge> {
    badges
        .iter()
        .map(|badge| RowBadge {
            text: badge.label.clone(),
            class_name: match &badge.class_name {
                Some(class_name) if !class_name.is_empty() => format!("action-item-badge {class_name}"),
                _ => "action-item-badge".to_owned(),
            },
        })
        .collect()
}

/// `asCssVariable(id)`.
pub(crate) fn as_css_variable(id: &str) -> String {
    format!("var(--vscode-{})", id.replace('.', "-"))
}

impl<T: ActionListItemData> ActionListWidget<T> {
    /// `getAriaLabel(element)` do `accessibilityProvider`.
    fn aria_label_for(&self, element: &IActionListItem<T>) -> Option<String> {
        if element.kind != ActionListItemKind::Action {
            return None;
        }
        let mut label = element.label.as_deref().map(strip_newlines).unwrap_or_default();
        if let Some(badge) = element.badge.as_deref().filter(|badge| !badge.is_empty()) {
            label = if element.badge_before_label { format!("{}, {label}", strip_newlines(badge)) } else { format!("{label}, {}", strip_newlines(badge)) };
        }
        for badge in &element.additional_badges {
            label = format!("{label}, {}", strip_newlines(&badge.label));
        }
        if let Some(detail) = element.detail.as_deref().filter(|detail| !detail.is_empty()) {
            label = format!("{label}, {}", strip_newlines(detail));
        }
        let has_description = element.description.as_deref().is_some_and(|description| !description.is_empty());
        if let Some(aria_description) = element.aria_description.as_deref().filter(|text| !text.is_empty()) {
            label = format!("{label}, {}", strip_newlines(aria_description));
        } else if has_description {
            label = format!("{label}, {}", strip_newlines(element.description.as_deref().unwrap_or_default()));
        }
        let hover_content = element.hover.as_ref().and_then(|hover| hover.content.clone());
        if let (Some(content), false, false) = (hover_content, element.aria_description.as_deref().is_some_and(|text| !text.is_empty()), has_description) {
            let hover_text = match content {
                HoverContent::Text(text) => Some(text),
                HoverContent::Element(element) => Some(element.text()),
                HoverContent::Markdown { text, markdown } => Some(format!("{text}\n{markdown}")),
                HoverContent::Lazy(_) => None,
            };
            if let Some(hover_text) = hover_text.filter(|text| !text.is_empty()) {
                let detail_matches = element.detail.as_deref().is_some_and(|detail| !detail.is_empty() && strip_newlines(detail) == strip_newlines(&hover_text));
                if !detail_matches {
                    label = format!("{label}, {}", strip_newlines(&hover_text));
                }
            }
        }
        if let Some(group) = element.group.as_ref().filter(|group| !group.title.is_empty()) {
            label = format!("{label}, {}", group.title);
        }
        if let Some(toggle) = element.standalone_toggle.as_ref().or(element.inline_toggle.as_ref()) {
            let checked = self.toggle_checked(element, toggle);
            let state = if checked { "on" } else { "off" };
            label = if element.standalone_toggle.is_some() { format!("{}, {state}", toggle.label) } else { format!("{label}, {}, {state}", toggle.label) };
        }
        if element.disabled {
            label = format!("{label}, Disabled Reason: true");
        }
        if has_submenu_actions(element) {
            label = format!("{label}, use right arrow to access options");
        }
        Some(label)
    }

    /// O estado marcado do `Switch` do toggle independente da linha (o `toggle.checked` depois de ativado).
    fn toggle_checked(&self, element: &IActionListItem<T>, toggle: &IActionListItemInlineToggle) -> bool {
        if element.standalone_toggle.is_none() {
            return toggle.checked;
        }
        let state = self.state.borrow();
        state.standalone_toggles.iter().find(|(item, _)| std::ptr::eq(Rc::as_ptr(item), element)).map_or(toggle.checked, |(_, checked)| *checked)
    }

    /// O estado renderizado da linha visível `index` (`renderElement` e o `aria-label` do `List`).
    pub fn row_state(&self, index: usize) -> Option<RowState> {
        let element = self.element(index)?;
        let (aria_set_size, aria_pos_in_set) = {
            let state = self.state.borrow();
            let is_action = |item: &Rc<IActionListItem<T>>| item.kind == ActionListItemKind::Action;
            let set_size = state.visible_menu_items.iter().filter(|item| is_action(item)).count();
            let pos_in_set = state.visible_menu_items.iter().take(index + 1).filter(|item| is_action(item)).count().max(1);
            (set_size, pos_in_set)
        };
        let role = if element.kind == ActionListItemKind::Action { "option" } else { "separator" };
        let mut row = RowState {
            aria_label: self.aria_label_for(&element),
            role: role.to_owned(),
            aria_set_size,
            aria_pos_in_set,
            ..RowState::default()
        };

        match element.kind {
            ActionListItemKind::Separator => {
                row.title_text = element.label.clone().unwrap_or_default();
                row.additional_badges = row_badges(&element.additional_badges);
                return Some(row);
            }
            ActionListItemKind::Header => {
                row.title_text = element.group.as_ref().map(|group| group.title.clone()).or_else(|| element.label.clone()).unwrap_or_default();
                return Some(row);
            }
            ActionListItemKind::Action => {}
        }

        if let Some(icon) = element.group.as_ref().and_then(|group| group.icon.clone().map(|icon| (icon, group.icon_color.clone()))) {
            row.icon.class_name = icon.0;
            row.icon.inline_color = icon.1.as_deref().map(as_css_variable).unwrap_or_default();
        } else {
            row.icon.class_name = "lightBulb".to_owned();
            row.icon.inline_color = "var(--vscode-editorLightBulb-foreground)".to_owned();
        }

        let label = element.label.clone().unwrap_or_default();
        if (element.item.is_none() && element.standalone_toggle.is_none()) || label.is_empty() {
            return Some(row);
        }
        row.icon.hidden = element.hide_icon;

        let mut classes = vec!["action".to_owned()];
        if let Some(class_name) = &element.class_name {
            let names: Vec<&str> = class_name.split_whitespace().collect();
            if !names.is_empty() {
                classes.push("action-list-custom".to_owned());
                classes.extend(names.into_iter().map(str::to_owned));
            }
        }

        let section_expanded = element.group.as_ref().and_then(|group| group.icon.as_deref()) == Some("chevronDown");
        row.title_text = strip_newlines(&label);
        row.title_label_text = row.title_text.clone();
        row.badge_text = element.badge.clone().unwrap_or_default();
        row.badge_hidden = row.badge_text.is_empty();
        row.badge_aria_hidden = "true".to_owned();
        row.description_text = element.description.clone().unwrap_or_default();
        row.badge_after_title = !element.badge_before_label;
        row.title_and_badge_order =
            if element.badge_before_label { vec![row.badge_text.clone(), row.title_text.clone()] } else { vec![row.title_text.clone(), row.badge_text.clone()] };
        row.additional_badges = row_badges(&element.additional_badges);

        if element.detail.as_deref().is_some_and(|detail| !detail.is_empty()) {
            classes.push("has-detail".to_owned());
        }

        let toggle = element.standalone_toggle.as_ref().or(element.inline_toggle.as_ref());
        if let Some(toggle) = toggle {
            if toggle.show_info_icon {
                if element.standalone_toggle.is_some() {
                    row.info_icon = Some(RowInfoIcon { aria_hidden: "true".to_owned(), is_last_child_of_title: true });
                    row.title_centers_info_icon = true;
                }
            }
            if element.standalone_toggle.is_none() {
                row.inline_toggle_label_count = 1;
                classes.push("has-inline-toggle".to_owned());
            } else {
                classes.push("has-standalone-toggle".to_owned());
            }
            row.switch = Some(RowSwitch {
                // Callers use `title` to say why a switch is unavailable, so it names the control for a screen reader too.
                aria_label: toggle.title.clone().unwrap_or_else(|| toggle.label.clone()),
                checked: self.toggle_checked(&element, toggle),
                disabled: toggle.disabled,
            });
        }

        if element.disabled {
            classes.push("option-disabled".to_owned());
        }
        row.title_attribute = if element.hover.is_some() {
            // Don't show tooltip when hover content is configured - the rich hover will show instead
            String::new()
        } else if let Some(tooltip) = element.tooltip.clone().filter(|tooltip| !tooltip.is_empty()) {
            tooltip
        } else if element.disabled {
            label.clone()
        } else {
            // O resto da cadeia lê atalhos do serviço de teclas, que esta lista ainda não recebe: sem atalho, o título é vazio.
            String::new()
        };

        let mut toolbar_actions: Vec<(String, String, String)> = element
            .toolbar_actions
            .iter()
            .flatten()
            .map(|action| (action.id(), action.label(), action.tooltip()))
            .collect();
        if element.on_remove.is_some() {
            toolbar_actions.push((REMOVE_TOOLBAR_ACTION_ID.to_owned(), "Remove".to_owned(), "Remove".to_owned()));
        }
        if !toolbar_actions.is_empty() {
            classes.push("has-toolbar".to_owned());
        }
        row.toolbar = toolbar_actions
            .into_iter()
            .map(|(action_id, action_label, tooltip)| RowToolbarButton {
                action_id,
                text: if element.toolbar_labels { action_label.clone() } else { String::new() },
                aria_label: if tooltip.is_empty() { action_label } else { tooltip },
                icon: !element.toolbar_labels,
            })
            .collect();

        row.submenu_indicator = has_submenu_indicator(&element);
        row.submenu_gutter = if row.submenu_indicator {
            SubmenuGutter::Shown
        } else if self.reserves_submenu_space() {
            // Reserve space for alignment when other items have submenus
            SubmenuGutter::Hidden
        } else {
            SubmenuGutter::None
        };
        // `_layoutSubmenu` marca a linha do painel aberto com `aria-expanded="true"` mesmo sem painel expansível.
        let expanded = self.state.borrow().expanded_trigger.as_ref().is_some_and(|trigger| Rc::ptr_eq(trigger, &element));
        if has_expandable_panel(&element) {
            row.aria_haspopup = Some(if element.hover.as_ref().is_some_and(|hover| hover.expandable) { "dialog" } else { "menu" }.to_owned());
            row.aria_expanded = Some(expanded.to_string());
        } else if expanded {
            row.aria_expanded = Some("true".to_owned());
        } else if element.is_section_toggle {
            row.aria_expanded = Some(section_expanded.to_string());
        }
        row.classes = classes;
        Some(row)
    }

    /// O `outlineColor` calculado da linha `index`: só a linha focada e habilitada tem contorno; o contorno é o
    /// `menu.selectionBorder`, e o `contrastActiveBorder` quando o foco veio do ponteiro (`mouse-navigation`) ou a lista
    /// não está com o foco visível por teclado.
    pub fn row_outline_color(&self, index: usize, selection_border: CssColor, contrast_active_border: CssColor) -> CssColor {
        let (focus, keyboard_navigation) = {
            let state = self.state.borrow();
            (state.focus, state.keyboard_navigation)
        };
        let Some(element) = self.element(index) else {
            return CssColor::TRANSPARENT;
        };
        if focus != Some(index) || element.disabled {
            return CssColor::TRANSPARENT;
        }
        let pointer_driven = keyboard_navigation == Some(false) || (keyboard_navigation != Some(true) && !self.focus_state.list_focused.get());
        if pointer_driven { contrast_active_border } else { selection_border }
    }

    /// `_computeToolbarWidth(item)`.
    fn compute_toolbar_width(item: &IActionListItem<T>) -> f64 {
        let mut action_count = item.toolbar_actions.as_ref().map_or(0, Vec::len);
        if item.on_remove.is_some() {
            action_count += 1;
        }
        if action_count == 0 {
            return 0.0;
        }
        // Each toolbar action button is ~22px (16px icon + padding), plus a 6px row gap and 10px trailing margin.
        let action_button_width = 22.0;
        action_count as f64 * action_button_width + 6.0 + 10.0
    }

    /// `_measureItemWidths(items)`: a largura da linha medida pelo renderizador nativo, mais a da barra de ferramentas
    /// (que fica fora da medida quando a linha tem `detail`).
    pub fn measure_item_widths(&self, items: &[Rc<IActionListItem<T>>]) -> Vec<f64> {
        let measurer = self.measurer.borrow().clone();
        items
            .iter()
            .map(|item| measurer(item) + if item.detail.as_deref().is_some_and(|detail| !detail.is_empty()) { 0.0 } else { Self::compute_toolbar_width(item) })
            .collect()
    }

    /// O painel de hover ou de submenu está visível (`style.display !== 'none'`).
    pub fn panel_displayed(&self) -> bool {
        !self.state.borrow().panel_hidden
    }

    /// `reservesSubmenuSpace()`, lido a cada renderização: se algum item abre um painel pode mudar quando os itens são
    /// reconstruídos no lugar, e uma resposta velha desloca toda linha pela largura de um chevron.
    fn reserves_submenu_space(&self) -> bool {
        match self.opts().and_then(|o| o.reserve_submenu_space).unwrap_or(ReserveSubmenuSpace::WhenAnyHasSubmenu) {
            ReserveSubmenuSpace::Always => true,
            ReserveSubmenuSpace::Never => false,
            ReserveSubmenuSpace::WhenAnyHasSubmenu => self.state.borrow().all_menu_items.iter().any(|item| has_submenu_indicator(item)),
        }
    }

    /// O `role` do painel de um hover expansível (`dialog`): o painel que carrega uma lista de submenu deixa a semântica
    /// para a lista. `None` com o painel fechado.
    pub fn panel_role(&self) -> Option<String> {
        self.expandable_panel_element().map(|_| "dialog".to_owned())
    }

    /// O `aria-label` do painel de um hover expansível: o rótulo da linha que o abriu.
    pub fn panel_aria_label(&self) -> Option<String> {
        self.expandable_panel_element().and_then(|element| element.label.clone().filter(|label| !label.is_empty()))
    }

    fn expandable_panel_element(&self) -> Option<Rc<IActionListItem<T>>> {
        let state = self.state.borrow();
        if state.panel_hidden {
            return None;
        }
        state.current_submenu_element.clone().filter(|element| element.hover.as_ref().is_some_and(|hover| hover.expandable))
    }

    /// `panel.querySelector('.action-list-submenu-hover-header')?.classList.contains('content-owns-padding')`.
    pub fn panel_header_owns_padding(&self) -> Option<bool> {
        let state = self.state.borrow();
        state.panel_header.as_ref()?;
        Some(state.current_submenu_element.as_ref().is_some_and(|element| element.hover.as_ref().is_some_and(|hover| hover.content_owns_padding)))
    }

    /// `panel.querySelector('.action-list-submenu-hover-header')?.textContent`.
    pub fn panel_hover_text(&self) -> Option<String> {
        match self.state.borrow().panel_header.clone()? {
            PanelHeader::Text(text) => Some(text),
            PanelHeader::Rendered { text, .. } => Some(text),
            PanelHeader::Element(element) => Some(element.text()),
        }
    }

    /// `panel.querySelectorAll('.action-list-submenu-hover-header a').length`.
    pub fn panel_hover_links(&self) -> usize {
        match self.state.borrow().panel_header {
            Some(PanelHeader::Rendered { links, .. }) => links,
            _ => 0,
        }
    }

    /// `panel.textContent`: o cabeçalho de hover e os rótulos das linhas do submenu.
    pub fn panel_text(&self) -> String {
        let (header, submenu) = {
            let state = self.state.borrow();
            (state.panel_header.clone(), state.current_submenu_widget.clone())
        };
        let mut text = match header {
            Some(PanelHeader::Text(text)) => text,
            Some(PanelHeader::Rendered { text, .. }) => text,
            Some(PanelHeader::Element(element)) => element.text(),
            None => String::new(),
        };
        if let Some(submenu) = submenu {
            for label in submenu.visible_labels().into_iter().flatten() {
                text.push_str(&label);
            }
        }
        text
    }

    /// `panel.contains(content)`.
    pub fn panel_contains(&self, content: &Rc<HoverElement>) -> bool {
        matches!(&self.state.borrow().panel_header, Some(PanelHeader::Element(shown)) if Rc::ptr_eq(shown, content))
    }

    /// O painel tem a classe `ACTION_WIDGET_ANIMATED_CLASS` (só o painel que mostra uma lista de submenu).
    pub fn panel_animated(&self) -> bool {
        self.state.borrow().panel_animated
    }

    /// `viewport.scrollHeight > viewport.clientHeight`: o conteúdo do hover é maior que o viewport do painel.
    pub fn panel_scrolls(&self) -> bool {
        let state = self.state.borrow();
        state.panel_viewport_height.is_some_and(|viewport| state.panel_content_height > viewport)
    }

    /// `viewport.scrollTop`.
    pub fn panel_scroll_top(&self) -> f64 {
        self.state.borrow().panel_scroll_top
    }

    /// `content.getBoundingClientRect()`: o conteúdo do hover dentro do painel, deslocado pela rolagem do viewport.
    pub fn panel_content_rect(&self) -> Option<IRect> {
        let state = self.state.borrow();
        let (top, left, _, _) = state.panel_rect?;
        let zoom = state.panel_zoom;
        let inset = (SUBMENU_PANEL_PADDING + SUBMENU_PANEL_BORDER) * zoom;
        let width = match &state.panel_header {
            Some(PanelHeader::Element(content)) => content.size().0,
            _ => 0.0,
        };
        Some(IRect { top: top + inset - state.panel_scroll_top * zoom, left: left + inset, width: width * zoom, height: state.panel_content_height * zoom })
    }

    /// O rodapé `.hover-row.status-bar` do painel: encostado nas bordas internas (as margens negativas anulam o `padding`).
    pub fn panel_footer(&self) -> Option<PanelFooter> {
        let state = self.state.borrow();
        if state.panel_footer.is_empty() {
            return None;
        }
        let (top, left, width, height) = state.panel_rect?;
        let zoom = state.panel_zoom;
        let border = SUBMENU_PANEL_BORDER * zoom;
        let bottom = top + height - border;
        Some(PanelFooter {
            class_name: "hover-row status-bar".to_owned(),
            actions: state.panel_footer.iter().map(|(_, action)| action.label.clone()).collect(),
            rect: IRect { top: bottom - HOVER_FOOTER_HEIGHT * zoom, left: left + border, width: width - 2.0 * border, height: HOVER_FOOTER_HEIGHT * zoom },
        })
    }
}

// #endregion

/// Para onde o Tab leva o foco: um botão da barra da linha, um controle do painel, a lista ou o campo de filtro.
enum TabTarget {
    Toolbar(usize),
    Control(Rc<HoverElement>),
    List,
    Filter,
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

    pub fn focus_item_action(&self, item_id: &str, action_id: &str) -> bool {
        self.widget.focus_item_action(item_id, action_id)
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
