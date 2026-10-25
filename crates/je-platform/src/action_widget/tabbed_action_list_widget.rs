//! Porte de `src/vs/platform/actionWidget/browser/tabbedActionListWidget.ts` (e do `tabbedActionListWidget.css`): o popup
//! com uma barra de abas sobre um [`ActionList`], a página de detalhes, o corpo recolhível, o rodapé e a animação das abas.
//!
//! Divergências de tradução:
//! - **DOM.** O popup é uma árvore de [`ViewElement`] (`.action-widget`, `.tabbed-action-list-main`, `-body`, `-body-content`,
//!   `-tabbar`, `-tabstrip`, `-filter-slot`, `-footer`, `-empty`, `-details`) e as medidas que o motor de layout do DOM
//!   calcularia (`offsetHeight`, `getBoundingClientRect`) são a geometria que o widget calcula, com as métricas do CSS
//!   nas constantes deste módulo. A largura do texto de uma aba vem do renderizador nativo
//!   ([`TabbedActionListWidget::set_tab_text_measurer`]).
//! - **`Radio`, `ActionBar`, `Button` e `DomScrollableElement`.** São widgets do `je-widgets`, que desenham sobre a `Ui`
//!   nativa; aqui entram como estado ([`TabElement`], [`DetailsBackButton`], a rolagem da página de detalhes): o que o teste
//!   lê desses widgets (texto, `aria-label`, classes, caixa, animação) é campo deles.
//! - **Eventos do DOM.** `keydown` do popup é [`TabbedActionListWidget::dispatch_key_down`] (com o alvo em
//!   [`TabbedKeyTarget`]), o clique numa aba é [`TabbedActionListWidget::click_tab`], o clique num botão da barra é
//!   [`TabbedActionListWidget::click_tab_bar_action`], o `pointerdown` é [`TabbedActionListWidget::pointer_down`], o `blur`
//!   do `trackFocus` é [`TabbedActionListWidget::notify_blur`], o `ResizeObserver` do rodapé e da página de detalhes são
//!   [`TabbedActionListWidget::notify_footer_resized`] e [`TabbedActionListWidget::notify_details_resized`], o `resize`
//!   da janela é [`TabbedActionListWidget::notify_window_resized`].
//! - **Foco.** `document.activeElement` fora da lista (aba, botão da barra, rodapé, página de detalhes) é o [`PopupFocus`];
//!   o foco dentro da lista é o dela. O rodapé e o conteúdo de detalhes são do chamador, que avisa o foco com
//!   [`TabbedActionListWidget::set_focus_target`].
//! - **Animações.** `element.animate(...)` é um registro ([`TabAnimation`], [`BodyAnimation`]) que o teste lê e termina
//!   (`finish`); o quadro de animação do `dom.animate` é o temporizador de 16 ms do `runtime`.
//! - **Injeção.** `IInstantiationService` e `IHoverService` são as dependências diretas que o recorte usa; o hover gerenciado de um
//!   botão da barra é o texto em [`TabBarActionElement::managed_hover`].

use std::cell::{Cell, RefCell};
use std::collections::BTreeMap;
use std::rc::{Rc, Weak};

use je_base::event::{Emitter, Event};
use je_base::key_codes::{KeyCode, KeyMod};
use je_base::keybindings::decode_keybinding;
use je_base::layout::AnchorPosition;
use je_base::lifecycle::{DisposableRef, DisposableStore, IDisposable, to_disposable};
use je_base::platform::{OperatingSystem, Platform};
use je_base::runtime::{TimerId, clear_timeout, queue_microtask, set_timeout};
use je_base::uri::URI;

use super::action_list::{
    ActionList, ActionListItemData, FILTER_HEIGHT, IActionListDelegate, IActionListItem, IActionListOptions, IActionListUpdateOptions, KeyTarget,
};
use super::action_widget::ActiveElement;
use super::action_widget_motion::{ACTION_WIDGET_ANIMATED_CLASS, ACTION_WIDGET_DROPDOWN_MOTION_CLASS, finish_action_widget_opening_animation};
use crate::accessibility::IAccessibilityService;
use crate::contextview::context_view::{AnchorTarget, HideData, IContextViewDelegate, IContextViewService, get_anchor_rect};
use crate::contextview::view_element::{UiKeyEvent, ViewElement};
use crate::keybinding::keybinding::IKeybindingService;
use crate::layout::layout_service::ILayoutService;

/// Timing for the tab resize animation. Both tabs share it, or the strip bulges mid-way.
pub const TAB_RESIZE_ANIMATION_DURATION_MS: u64 = 300;
pub const BODY_COLLAPSE_ANIMATION_DURATION_MS: u64 = 200;
pub const ANIMATION_EASING: &str = "cubic-bezier(0.4, 0, 0.2, 1)";
/// A duração de um quadro de animação no relógio do `runtime`.
const ANIMATION_FRAME_MS: u64 = 16;

/// `.tabbed-action-list-tabbar { padding: 0 0 4px }`.
const TAB_BAR_PADDING_BOTTOM: f64 = 4.0;
/// A altura de um botão de aba e de um botão da barra (`.monaco-button` de linha única).
const TAB_BUTTON_HEIGHT: f64 = 22.0;
/// O `padding` horizontal de uma aba, a largura do ícone (`--vscode-codiconFontSize`) com a margem de `0 0.2em` e o `gap`.
const TAB_PADDING_HORIZONTAL: f64 = 6.0;
const TAB_ICON_WIDTH: f64 = 16.0;
const TAB_ICON_MARGIN: f64 = 4.0;
/// A largura de um caractere do rótulo de uma aba, enquanto o renderizador nativo não mede.
const DEFAULT_TAB_CHARACTER_WIDTH: f64 = 7.0;
/// `.action-widget { padding: 4px; border: 1px }` nos dois lados: o que o popup reserva em volta de `main`.
const POPUP_CHROME: f64 = 10.0;
/// `.tabbed-action-list-details-header { padding: var(--vscode-spacing-size40) }` e a altura do botão Voltar.
const DETAILS_HEADER_PADDING: f64 = 4.0;
const DETAILS_BACK_HEIGHT: f64 = 22.0;
/// A altura máxima da página de detalhes e a folga que ela deixa até a borda da janela.
const DETAILS_MAX_HEIGHT: f64 = 400.0;
const DETAILS_EDGE_MARGIN: f64 = 16.0;

/// `NEXT_TAB_KEYBINDING`, `PREVIOUS_TAB_KEYBINDING` e `DETAILS_BACK_KEYBINDING`: dependem do sistema operacional.
fn os() -> OperatingSystem {
    thread_local! {
        static OS: OperatingSystem = Platform::detect().os;
    }
    OS.with(|os| *os)
}

fn is_macintosh() -> bool {
    os() == OperatingSystem::Macintosh
}

fn is_windows() -> bool {
    os() == OperatingSystem::Windows
}

/// Switches tabs from anywhere in the popup, with the same keys as switching editor tabs.
pub fn next_tab_keybinding() -> u32 {
    if is_macintosh() { KeyMod::CTRL_CMD | KeyMod::ALT | KeyCode::RightArrow.bits() } else { KeyMod::CTRL_CMD | KeyCode::PageDown.bits() }
}

pub fn previous_tab_keybinding() -> u32 {
    if is_macintosh() { KeyMod::CTRL_CMD | KeyMod::ALT | KeyCode::LeftArrow.bits() } else { KeyMod::CTRL_CMD | KeyCode::PageUp.bits() }
}

/// Leaves a details page, with the same keys as Go Back and the Quick Pick Back button.
pub fn details_back_keybinding() -> u32 {
    if is_windows() {
        KeyMod::ALT | KeyCode::LeftArrow.bits()
    } else if is_macintosh() {
        KeyMod::WIN_CTRL | KeyCode::Minus.bits()
    } else {
        KeyMod::CTRL_CMD | KeyMod::ALT | KeyCode::Minus.bits()
    }
}

/// `StandardKeyboardEvent.equals(keybinding)`: o código da tecla com os modificadores na codificação do VS Code.
fn key_event_equals(e: &UiKeyEvent, keybinding: u32) -> bool {
    let mac = is_macintosh();
    let mut encoded = e.key_code.bits();
    if if mac { e.meta_key } else { e.ctrl_key } {
        encoded |= KeyMod::CTRL_CMD;
    }
    if e.shift_key {
        encoded |= KeyMod::SHIFT;
    }
    if e.alt_key {
        encoded |= KeyMod::ALT;
    }
    if if mac { e.ctrl_key } else { e.meta_key } {
        encoded |= KeyMod::WIN_CTRL;
    }
    encoded == keybinding
}

fn get_keybinding_label(keybinding_service: &dyn IKeybindingService, keybinding: u32) -> Option<String> {
    let decoded = decode_keybinding(keybinding, os()).ok().flatten()?;
    keybinding_service.resolve_keybinding(&decoded).first().and_then(|resolved| resolved.get_label())
}

/// Labels for the keys a [`TabbedActionListWidget`] handles itself, for help text.
pub struct TabbedActionListKeybindingLabels {
    pub next_tab: String,
    pub previous_tab: String,
    pub back: String,
}

/// `getTabbedActionListKeybindingLabels`.
pub fn get_tabbed_action_list_keybinding_labels(keybinding_service: &dyn IKeybindingService) -> TabbedActionListKeybindingLabels {
    TabbedActionListKeybindingLabels {
        next_tab: get_keybinding_label(keybinding_service, next_tab_keybinding()).unwrap_or_default(),
        previous_tab: get_keybinding_label(keybinding_service, previous_tab_keybinding()).unwrap_or_default(),
        back: get_keybinding_label(keybinding_service, details_back_keybinding()).unwrap_or_default(),
    }
}

/// The box a tab occupied, including the spacing that travels with its width.
#[derive(Clone, Debug, PartialEq)]
pub struct TabBox {
    pub width: f64,
    pub padding_left: String,
    pub padding_right: String,
    pub column_gap: String,
}

/// `ITabBuildResult`: result of [`ITabbedActionListShowOptions::create_action_list`]. The list options are recomputed on
/// every tab switch so callers can vary filter visibility, width, etc. by tab.
pub struct ITabbedActionListBuildResult<T: ActionListItemData> {
    pub items: Vec<Rc<IActionListItem<T>>>,
    pub list_options: Option<IActionListOptions>,
}

/// `ThemeIcon | { light: URI; dark: URI }`: o ícone de uma aba.
#[derive(Clone)]
pub enum TabIcon {
    /// O id do codicon.
    Theme(String),
    Image { light: URI, dark: URI },
}

/// Describes one tab in a [`TabbedActionListWidget`]. The `id` is the stable identity used everywhere the widget reasons
/// about a tab; `label`, `tooltip` and `icon` are presentation only.
#[derive(Clone)]
pub struct ITabDescriptor {
    pub id: String,
    /// Visible label. Defaults to `id`.
    pub label: Option<String>,
    /// Hover tooltip. Defaults to `label ?? id`.
    pub tooltip: Option<String>,
    pub icon: Option<TabIcon>,
}

impl ITabDescriptor {
    pub fn new(id: &str) -> Self {
        Self { id: id.to_owned(), label: None, tooltip: None, icon: None }
    }
}

/// An icon button rendered at the trailing edge of the tab bar. Unlike a tab, running it does not change the active tab.
#[derive(Clone)]
pub struct ITabBarAction {
    pub id: String,
    /// O id do codicon.
    pub icon: String,
    pub tooltip: String,
    /// When true, the button is pushed to the far end of the tab bar.
    pub align_end: bool,
    /// When true, use the workbench hover instead of the button's native title.
    pub use_managed_hover: bool,
    /// When true, the button renders in its pressed state. `None` is a momentary action.
    pub checked: Option<bool>,
    pub run: Rc<dyn Fn()>,
}

/// `tabLabels`: when tabs show their label beside their icon.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum TabLabels {
    #[default]
    Always,
    /// Labels only the active tab, and a tab with no icon always shows its label.
    Active,
    Never,
}

pub type CreateActionList<T> = Rc<dyn Fn(&str, bool) -> ITabbedActionListBuildResult<T>>;

/// Options for [`TabbedActionListWidget::show`].
#[derive(Clone)]
pub struct ITabbedActionListShowOptions<T: ActionListItemData> {
    /// Logical user / source identifier passed through to the [`ActionList`].
    pub user: String,
    /// Element the popup is anchored to.
    pub anchor: AnchorTarget,
    pub tabs: Vec<ITabDescriptor>,
    /// Initially active tab id. Must match an entry in `tabs`.
    pub initial_tab: String,
    /// Computes a tab's list, or its resting contents when `for_sizing` is true.
    pub create_action_list: CreateActionList<T>,
    pub delegate: Rc<dyn IActionListDelegate<T>>,
    pub width: Option<f64>,
    /// Context view layer used when the picker must render above another context view.
    pub context_view_layer: Option<i32>,
    /// Optional class name to add to the tab bar element. Must be a single class.
    pub tab_bar_class_name: Option<String>,
    /// Computes the class names on the popup's `.action-widget` element. Re-evaluated whenever the popup re-renders or its
    /// list is refreshed.
    pub widget_class_names: Option<Rc<dyn Fn(&str) -> Vec<String>>>,
    /// Tab whose contents fix the popup's height, so switching tabs scrolls instead of resizing.
    pub sizing_tab: Option<String>,
    /// Show the checked item's hover after the popup has been positioned.
    pub show_checked_item_hover: bool,
    /// Optional icon buttons rendered after the tabs.
    pub tab_bar_actions: Vec<ITabBarAction>,
    pub tab_labels: Option<TabLabels>,
    /// When true, the list's filter row is rendered inside the tab bar in place of the tabs.
    pub filter_in_tab_bar: bool,
    /// Whether the tabs and list are collapsed, leaving only the footer. Re-read on refresh.
    pub is_body_collapsed: Option<Rc<dyn Fn() -> bool>>,
    /// Renders content pinned below the list, e.g. a persistent option row.
    pub render_footer: Option<Rc<dyn Fn(&Rc<ViewElement>, &str) -> DisposableRef>>,
    /// Focuses a footer control when the body is collapsed.
    pub focus_footer: Option<Rc<dyn Fn()>>,
    /// Renders the body when the active tab has no items, e.g. a sign-in prompt. `None` shows the empty list.
    pub render_empty: Option<Rc<dyn Fn(&Rc<ViewElement>, &str) -> Option<DisposableRef>>>,
    /// Opens the focused item's details when Right Arrow is pressed in the list. Returns false when the item has no details.
    pub open_item_details: Option<Rc<dyn Fn(&Rc<IActionListItem<T>>) -> bool>>,
}

impl<T: ActionListItemData> ITabbedActionListShowOptions<T> {
    /// As opções obrigatórias; as demais começam ausentes.
    pub fn new(
        user: &str,
        anchor: AnchorTarget,
        tabs: Vec<ITabDescriptor>,
        initial_tab: &str,
        create_action_list: CreateActionList<T>,
        delegate: Rc<dyn IActionListDelegate<T>>,
    ) -> Self {
        Self {
            user: user.to_owned(),
            anchor,
            tabs,
            initial_tab: initial_tab.to_owned(),
            create_action_list,
            delegate,
            width: None,
            context_view_layer: None,
            tab_bar_class_name: None,
            widget_class_names: None,
            sizing_tab: None,
            show_checked_item_hover: false,
            tab_bar_actions: Vec::new(),
            tab_labels: None,
            filter_in_tab_bar: false,
            is_body_collapsed: None,
            render_footer: None,
            focus_footer: None,
            render_empty: None,
            open_item_details: None,
        }
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ITabbedActionListRefreshOptions {
    pub focus_item_id: Option<String>,
    pub preserve_hover: bool,
    pub preserve_scroll_position: bool,
    pub animate_item_move: bool,
    /// Keeps the popup's measured size, for refreshes that only change item state such as a checked row.
    pub preserve_size: bool,
}

/// `ITabbedActionListDetailsOptions`.
#[derive(Clone)]
pub struct ITabbedActionListDetailsOptions {
    pub label: String,
    pub back_label: String,
    /// Renders a fixed heading beside an icon-only Back button.
    pub render_header: Option<Rc<dyn Fn(&Rc<ViewElement>) -> DisposableRef>>,
    pub render: Rc<dyn Fn(&Rc<ViewElement>) -> DisposableRef>,
    pub focus: Option<Rc<dyn Fn(&Rc<ViewElement>)>>,
    pub restore_focus: Option<Rc<dyn Fn() -> bool>>,
    pub on_back: Option<Rc<dyn Fn()>>,
}

/// Um botão de aba (`Radio.optionElements[i]`).
#[derive(Clone)]
pub struct TabElement {
    pub id: String,
    /// O texto do botão, com o ícone na sintaxe `$(id)`.
    pub text: String,
    pub tooltip: String,
    pub aria_label: String,
    pub is_active: bool,
    pub classes: Vec<String>,
    /// `--tabbed-action-list-tab-icon-light` e `--tabbed-action-list-tab-icon-dark` do ícone de imagem.
    pub image_icon: Option<(URI, URI)>,
    /// `Radio.overrideOptionLabel`: o rótulo que a aba segura enquanto encolhe.
    pub label_override: Option<String>,
    pub box_: TabBox,
    pub animations: Vec<TabAnimation>,
}

impl TabElement {
    /// `textContent`: o texto sem o ícone `$(id)`, que é um `<span class="codicon">` vazio.
    pub fn text_content(&self) -> String {
        let text = self.label_override.as_deref().unwrap_or(&self.text);
        match text.strip_prefix("$(").and_then(|rest| rest.split_once(')')) {
            Some((_, rest)) => rest.trim_start().to_owned(),
            None => text.to_owned(),
        }
    }

    /// Whether the tab draws a codicon.
    pub fn has_codicon(&self) -> bool {
        self.text.starts_with("$(")
    }

    pub fn has_class(&self, class_name: &str) -> bool {
        self.classes.iter().any(|class| class == class_name)
    }
}

/// `element.animate([{ width, paddingLeft, ... }, { ... }], TAB_RESIZE_ANIMATION)` de uma aba.
#[derive(Clone)]
pub struct TabAnimation {
    pub from: TabBox,
    pub to: TabBox,
    pub duration: u64,
    pub easing: &'static str,
    pub finished: bool,
    /// `labelSpan.animate([{ maxWidth: scrollWidth }, { maxWidth: 0 }])` da aba que perde o rótulo.
    pub label_max_width: Option<(f64, f64)>,
}

/// Um botão da barra (`button.tabbed-action-list-tabbar-action`).
#[derive(Clone)]
pub struct TabBarActionElement {
    pub id: String,
    pub aria_label: String,
    /// `button.title`: ausente com o hover gerenciado.
    pub title: Option<String>,
    /// O conteúdo do hover gerenciado, quando `useManagedHover`.
    pub managed_hover: Option<String>,
    pub aria_pressed: Option<String>,
    pub classes: Vec<String>,
    /// A classe do codicon do `span`.
    pub icon_class: String,
    /// O botão é filho da barra (`alignEnd`) e não da faixa de abas.
    pub in_tab_bar: bool,
}

/// O botão Voltar da página de detalhes (`ActionBar` com ícone, ou `Button` de texto).
#[derive(Clone, Debug, PartialEq)]
pub struct DetailsBackButton {
    pub aria_label: String,
    pub title: String,
    /// O texto visível: vazio no botão de ícone.
    pub text: String,
    /// `back.closest('.monaco-action-bar')`.
    pub icon_action: bool,
    pub icon_class: String,
    pub text_button: bool,
}

/// A animação da altura do corpo (`body.animate([{ height: from }, { height: to }], BODY_COLLAPSE_ANIMATION)`).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct BodyAnimation {
    pub from_height: f64,
    pub to_height: f64,
    pub duration: u64,
    /// `animation.currentTime` (ms).
    pub current_time: f64,
    pub paused: bool,
}

impl BodyAnimation {
    /// A altura do corpo no instante `current_time`.
    pub fn current_height(&self) -> f64 {
        let progress = (self.current_time / self.duration as f64).clamp(0.0, 1.0);
        self.from_height + (self.to_height - self.from_height) * progress
    }
}

/// O `document.activeElement` fora da lista.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PopupFocus {
    /// O foco, se estiver no popup, é o da lista.
    List,
    Tab(usize),
    TabBarAction(usize),
    Footer,
    DetailsBack,
    DetailsPage,
    /// Um controle do conteúdo da página de detalhes.
    DetailsContent,
}

/// Onde um `keydown` aconteceu: as quatro perguntas de `target.closest(...)` do tratador do popup e o alvo dentro da lista.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct TabbedKeyTarget {
    pub on_tab_bar: bool,
    pub on_footer: bool,
    pub on_editable: bool,
    /// `.tabbed-action-list-empty`, `.action-list-submenu-panel`, `.tabbed-action-list-details`, a barra e o switch da linha.
    pub on_own_controls: bool,
    /// O alvo dentro do `ActionList` (o campo de filtro, a lista...), se ele está lá.
    pub list: Option<KeyTarget>,
}

impl TabbedKeyTarget {
    /// O `.monaco-list`.
    pub fn list() -> Self {
        Self { list: Some(KeyTarget::List), ..Self::default() }
    }

    /// O campo de filtro: editável, e dentro da barra com `filterInTabBar`.
    pub fn filter_input(in_tab_bar: bool) -> Self {
        Self { on_tab_bar: in_tab_bar, on_editable: true, list: Some(KeyTarget::FilterInput), ..Self::default() }
    }

    /// Um botão de aba ou da barra.
    pub fn tab_bar() -> Self {
        Self { on_tab_bar: true, ..Self::default() }
    }

    pub fn footer() -> Self {
        Self { on_footer: true, ..Self::default() }
    }

    /// O botão ou a página dentro de `.tabbed-action-list-details`.
    pub fn details() -> Self {
        Self { on_own_controls: true, ..Self::default() }
    }
}

/// O que o widget guarda do popup renderizado e a [`TabbedActionListWidget`] usa sem conhecer o tipo do item.
trait PopupControl {
    fn refresh(&self, options: Option<ITabbedActionListRefreshOptions>);
    fn show_details(&self, options: ITabbedActionListDetailsOptions);
    fn hide_details(&self);
    fn focus_item_action(&self, item_id: &str, action_id: &str) -> bool;
    fn focus_item(&self, item_id: &str) -> bool;
    fn dispatch_key_down(&self, e: &UiKeyEvent, target: TabbedKeyTarget);
    fn click_tab(&self, index: usize);
    fn click_tab_bar_action(&self, index: usize);
    fn pointer_down(&self);
    fn footer_resized(&self);
    fn details_resized(&self);
    fn window_resized(&self);
    fn set_focus(&self, focus: PopupFocus);
    fn focus(&self) -> PopupFocus;
    fn tabs(&self) -> Vec<TabElement>;
    fn tab_bar_actions(&self) -> Vec<TabBarActionElement>;
    fn active_tab(&self) -> String;
    fn elements(&self) -> PopupElements;
    fn body_state(&self) -> BodyState;
    fn body_animation(&self) -> Option<BodyAnimation>;
    fn set_body_animation_time(&self, time: f64, paused: bool);
    fn finish_body_animation(&self);
    fn finish_tab_animations(&self, index: usize);
    fn details_state(&self) -> Option<DetailsState>;
    fn details_scroll(&self, direction: f64);
    fn list_height(&self) -> f64;
    fn list_visible_labels(&self) -> Vec<Option<String>>;
    fn list_set_hover_enabled_state(&self) -> bool;
    fn list_toolbar_focus(&self) -> Option<(usize, String)>;
    fn list_has_focus(&self) -> bool;
    fn list_filter_focused(&self) -> bool;
    fn list_panel_state(&self) -> (bool, bool);
    fn list_focused_label(&self) -> Option<String>;
    fn list_filter_input(&self, value: &str);
    fn list_filter_composition(&self, start: bool);
}

/// Os elementos do popup, para o teste ler as classes, o estilo e o tamanho.
#[derive(Clone)]
pub struct PopupElements {
    pub widget: Rc<ViewElement>,
    pub block: Rc<ViewElement>,
    pub main: Rc<ViewElement>,
    pub body: Rc<ViewElement>,
    pub body_content: Rc<ViewElement>,
    pub tab_bar: Rc<ViewElement>,
    pub tab_strip: Rc<ViewElement>,
    pub filter_slot: Rc<ViewElement>,
    pub footer: Option<Rc<ViewElement>>,
    pub empty_body: Option<Rc<ViewElement>>,
    pub list: Rc<ViewElement>,
}

/// Os elementos que existem desde o começo do `render`.
struct BaseElements {
    widget: Rc<ViewElement>,
    block: Rc<ViewElement>,
    main: Rc<ViewElement>,
    body: Rc<ViewElement>,
    body_content: Rc<ViewElement>,
    tab_bar: Rc<ViewElement>,
    tab_strip: Rc<ViewElement>,
    filter_slot: Rc<ViewElement>,
}

/// O estado do corpo: `body.inert`, as classes e o `main`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BodyState {
    pub inert: bool,
    pub collapsed: bool,
    pub animating: bool,
    pub main_inert: bool,
    pub main_aria_hidden: bool,
}

/// A página de detalhes aberta.
#[derive(Clone)]
pub struct DetailsState {
    pub label: String,
    pub page: Rc<ViewElement>,
    pub header: Rc<ViewElement>,
    pub content: Rc<ViewElement>,
    pub back: DetailsBackButton,
    pub viewport_height: f64,
    pub scroll_top: f64,
    pub header_height: f64,
}

/// A fatia do widget que sobrevive ao popup: o que o `show` seguinte lê.
struct WidgetState {
    swapping_tab: Cell<bool>,
    details_visible: Cell<bool>,
    /// Boxes and labels from the last render, so the next one can animate from them.
    previous_tab_boxes: RefCell<Option<BTreeMap<String, TabBox>>>,
    previous_tab_texts: RefCell<Option<BTreeMap<String, String>>>,
    /// List height from `sizingTab`, recomputed when items change.
    fixed_list_height: Cell<Option<f64>>,
    fixed_popup_height: Cell<Option<f64>>,
    has_measured_sizing_tab: Cell<bool>,
    /// Side of the anchor the popup opened on, kept across tab swaps so it cannot jump sides.
    anchor_position: Cell<Option<AnchorPosition>>,
    active_popup: RefCell<Option<(u64, Rc<DisposableStore>)>>,
    control: RefCell<Option<Rc<dyn PopupControl>>>,
    next_popup_id: Cell<u64>,
}

type TextMeasurer = Rc<dyn Fn(&str) -> f64>;

/// A widget that shows a tabbed action list in a context view popup.
pub struct TabbedActionListWidget {
    this: Weak<TabbedActionListWidget>,
    context_view_service: Rc<dyn IContextViewService>,
    layout_service: Rc<dyn ILayoutService>,
    accessibility_service: Rc<dyn IAccessibilityService>,
    keybinding_service: Rc<dyn IKeybindingService>,
    on_did_change_tab: Rc<Emitter<String>>,
    on_did_hide: Rc<Emitter<()>>,
    state: WidgetState,
    text_measurer: RefCell<TextMeasurer>,
}

impl TabbedActionListWidget {
    pub fn new(
        context_view_service: Rc<dyn IContextViewService>,
        layout_service: Rc<dyn ILayoutService>,
        accessibility_service: Rc<dyn IAccessibilityService>,
        keybinding_service: Rc<dyn IKeybindingService>,
    ) -> Rc<Self> {
        Rc::new_cyclic(|this| Self {
            this: this.clone(),
            context_view_service,
            layout_service,
            accessibility_service,
            keybinding_service,
            on_did_change_tab: Emitter::new(),
            on_did_hide: Emitter::new(),
            state: WidgetState {
                swapping_tab: Cell::new(false),
                details_visible: Cell::new(false),
                previous_tab_boxes: RefCell::new(None),
                previous_tab_texts: RefCell::new(None),
                fixed_list_height: Cell::new(None),
                fixed_popup_height: Cell::new(None),
                has_measured_sizing_tab: Cell::new(false),
                anchor_position: Cell::new(None),
                active_popup: RefCell::new(None),
                control: RefCell::new(None),
                next_popup_id: Cell::new(1),
            },
            text_measurer: RefCell::new(Rc::new(|text: &str| text.chars().count() as f64 * DEFAULT_TAB_CHARACTER_WIDTH)),
        })
    }

    pub fn on_did_change_tab(&self) -> Event<String> {
        self.on_did_change_tab.event()
    }

    pub fn on_did_hide(&self) -> Event<()> {
        self.on_did_hide.event()
    }

    /// A medida do texto de um rótulo de aba pelo renderizador nativo.
    pub fn set_tab_text_measurer(&self, measurer: TextMeasurer) {
        *self.text_measurer.borrow_mut() = measurer;
    }

    pub fn is_visible(&self) -> bool {
        self.state.active_popup.borrow().is_some()
    }

    pub fn is_showing_details(&self) -> bool {
        self.state.details_visible.get()
    }

    /// `_activePopup.value = value`: o valor anterior é descartado depois de trocar.
    fn set_active_popup(&self, value: Option<(u64, Rc<DisposableStore>)>) {
        let previous = std::mem::replace(&mut *self.state.active_popup.borrow_mut(), value);
        if let Some((_, previous)) = previous {
            previous.dispose();
        }
    }

    fn control(&self) -> Option<Rc<dyn PopupControl>> {
        self.state.control.borrow().clone()
    }

    /// Shows the popup anchored to `options.anchor`. If a popup is already visible, it is replaced in place.
    pub fn show<T: ActionListItemData>(&self, options: ITabbedActionListShowOptions<T>) {
        let is_swap = self.is_visible();
        if is_swap {
            self.state.swapping_tab.set(true);
            self.set_active_popup(None);
        } else {
            // A fresh popup has nothing to animate from, and its models may have changed.
            *self.state.previous_tab_boxes.borrow_mut() = None;
            *self.state.previous_tab_texts.borrow_mut() = None;
            self.state.fixed_list_height.set(None);
            self.state.fixed_popup_height.set(None);
            self.state.has_measured_sizing_tab.set(false);
            self.state.anchor_position.set(None);
        }

        let popup_disposables = DisposableStore::new();
        let popup_id = self.state.next_popup_id.get();
        self.state.next_popup_id.set(popup_id + 1);

        // Reserve the disposable slot up-front so any synchronous hide triggered during render (e.g. an immediate selection)
        // finds the expected disposable to clear.
        *self.state.active_popup.borrow_mut() = Some((popup_id, popup_disposables.clone()));
        let context_view_service = self.context_view_service.clone();
        popup_disposables.add(to_disposable(move || context_view_service.hide_context_view(None)));

        let list_ref: Rc<RefCell<Option<Rc<ActionList<T>>>>> = Rc::new(RefCell::new(None));
        let layer = options.context_view_layer;
        let anchor = options.anchor.clone();
        let show_checked_item_hover = options.show_checked_item_hover;
        let is_body_collapsed = options.is_body_collapsed.clone();
        self.context_view_service.show_context_view(
            Rc::new(TabbedViewDelegate { owner: self.this.clone(), popup_id, options, is_swap, list_ref: list_ref.clone(), anchor, layer }),
            None,
            false,
        );

        if show_checked_item_hover && !is_body_collapsed.as_ref().is_some_and(|collapsed| collapsed()) {
            let list = list_ref.borrow().clone();
            if let Some(list) = list {
                list.widget().show_hover_for_checked_item();
            }
        }

        if is_swap {
            self.state.swapping_tab.set(false);
        }
    }

    /// `hide()`.
    pub fn hide(&self) {
        self.set_active_popup(None);
    }

    /// `hide()` do popup `popup_id`: só se ele ainda for o ativo.
    fn hide_popup(&self, popup_id: u64) {
        let is_current = self.state.active_popup.borrow().as_ref().is_some_and(|(id, _)| *id == popup_id);
        if is_current {
            self.set_active_popup(None);
        }
    }

    /// Rebuilds the active tab and remeasures its resting sizing contents without dismissing the popup. Focus is retained, and
    /// `preserve_hover` keeps the live detail panel.
    pub fn refresh_active_list(&self, options: Option<ITabbedActionListRefreshOptions>) {
        if let Some(control) = self.control() {
            control.refresh(options);
        }
    }

    /// Shows a drill-in page while retaining the list's filter, expansion, scroll, and focus state.
    pub fn show_details(&self, options: ITabbedActionListDetailsOptions) {
        if let Some(control) = self.control() {
            control.show_details(options);
        }
    }

    pub fn hide_details(&self) {
        if let Some(control) = self.control() {
            control.hide_details();
        }
    }

    pub fn focus_item_action(&self, item_id: &str, action_id: &str) -> bool {
        self.control().is_some_and(|control| control.focus_item_action(item_id, action_id))
    }

    /// Focuses a list row by its item's `id`. Returns whether the list took focus.
    pub fn focus_item(&self, item_id: &str) -> bool {
        self.control().is_some_and(|control| control.focus_item(item_id))
    }

    /// O `keydown` do popup (e do que está dentro dele). A lista trata primeiro; se o evento sobe, o popup trata em seguida.
    pub fn dispatch_key_down(&self, e: &UiKeyEvent, target: TabbedKeyTarget) {
        if let Some(control) = self.control() {
            control.dispatch_key_down(e, target);
        }
    }

    /// A seleção do `Radio`: o clique (precedido do `pointerdown`) na aba `index`.
    pub fn click_tab(&self, index: usize) {
        if let Some(control) = self.control() {
            control.click_tab(index);
        }
    }

    /// O clique no botão `index` da barra.
    pub fn click_tab_bar_action(&self, index: usize) {
        if let Some(control) = self.control() {
            control.click_tab_bar_action(index);
        }
    }

    /// O `pointerdown` no popup: a navegação deixa de ser por teclado.
    pub fn pointer_down(&self) {
        if let Some(control) = self.control() {
            control.pointer_down();
        }
    }

    /// O `ResizeObserver` do rodapé.
    pub fn notify_footer_resized(&self) {
        if let Some(control) = self.control() {
            control.footer_resized();
        }
    }

    /// O `ResizeObserver` do conteúdo e da página de detalhes.
    pub fn notify_details_resized(&self) {
        if let Some(control) = self.control() {
            control.details_resized();
        }
    }

    /// O `resize` da janela, que reposiciona a página de detalhes.
    pub fn notify_window_resized(&self) {
        if let Some(control) = self.control() {
            control.window_resized();
        }
    }

    /// O `onDidBlur` do `trackFocus(container)`: o foco saiu do popup. `active_element` é o que o recebeu.
    pub fn notify_blur(&self, active_element: ActiveElement) {
        if self.state.swapping_tab.get() {
            return;
        }
        if active_element == ActiveElement::HoverOrSubmenu {
            return;
        }
        let popup_id = self.state.active_popup.borrow().as_ref().map(|(id, _)| *id);
        if let Some(popup_id) = popup_id {
            self.hide_popup(popup_id);
        }
    }

    /// `element.focus()` num controle do popup que não é da lista (o rodapé, o conteúdo de detalhes).
    pub fn set_focus_target(&self, focus: PopupFocus) {
        if let Some(control) = self.control() {
            control.set_focus(focus);
        }
    }

    pub fn focus_target(&self) -> Option<PopupFocus> {
        self.control().map(|control| control.focus())
    }

    /// Os botões de aba do popup, na ordem.
    pub fn tabs(&self) -> Vec<TabElement> {
        self.control().map(|control| control.tabs()).unwrap_or_default()
    }

    pub fn tab_bar_actions(&self) -> Vec<TabBarActionElement> {
        self.control().map(|control| control.tab_bar_actions()).unwrap_or_default()
    }

    pub fn active_tab(&self) -> Option<String> {
        self.control().map(|control| control.active_tab())
    }

    pub fn elements(&self) -> Option<PopupElements> {
        self.control().map(|control| control.elements())
    }

    pub fn body_state(&self) -> Option<BodyState> {
        self.control().map(|control| control.body_state())
    }

    pub fn body_animation(&self) -> Option<BodyAnimation> {
        self.control().and_then(|control| control.body_animation())
    }

    /// `animation.currentTime = time` e `animation.pause()`.
    pub fn set_body_animation_time(&self, time: f64, paused: bool) {
        if let Some(control) = self.control() {
            control.set_body_animation_time(time, paused);
        }
    }

    /// `animation.finish()` da animação do corpo.
    pub fn finish_body_animation(&self) {
        if let Some(control) = self.control() {
            control.finish_body_animation();
        }
    }

    /// `animation.finish()` das animações da aba `index`, e o `finish` que devolve o rótulo que ela segurava.
    pub fn finish_tab_animations(&self, index: usize) {
        if let Some(control) = self.control() {
            control.finish_tab_animations(index);
        }
    }

    pub fn details_state(&self) -> Option<DetailsState> {
        self.control().and_then(|control| control.details_state())
    }

    /// `PageDown` (`1.0`) ou `PageUp` (`-1.0`) na página de detalhes.
    pub fn scroll_details_page(&self, direction: f64) {
        if let Some(control) = self.control() {
            control.details_scroll(direction);
        }
    }

    /// `parseFloat(list.style.height)`.
    pub fn list_height(&self) -> f64 {
        self.control().map_or(0.0, |control| control.list_height())
    }

    pub fn list_visible_labels(&self) -> Vec<Option<String>> {
        self.control().map(|control| control.list_visible_labels()).unwrap_or_default()
    }

    /// O botão da barra da linha focada que é o `document.activeElement`: o índice e o `aria-label`.
    pub fn list_focused_toolbar_button(&self) -> Option<(usize, String)> {
        self.control().and_then(|control| control.list_toolbar_focus())
    }

    /// O `document.activeElement` é a lista (`.monaco-list`).
    pub fn is_list_focused(&self) -> bool {
        self.control().is_some_and(|control| control.focus() == PopupFocus::List && control.list_has_focus())
    }

    pub fn is_filter_input_focused(&self) -> bool {
        self.control().is_some_and(|control| control.focus() == PopupFocus::List && control.list_filter_focused())
    }

    /// O painel de hover está visível e o foco está dentro dele.
    pub fn list_panel_state(&self) -> (bool, bool) {
        self.control().map_or((false, false), |control| control.list_panel_state())
    }

    /// O rótulo da linha focada da lista.
    pub fn list_focused_label(&self) -> Option<String> {
        self.control().and_then(|control| control.list_focused_label())
    }

    /// O `input` do usuário no campo de filtro da lista.
    pub fn dispatch_filter_input(&self, value: &str) {
        if let Some(control) = self.control() {
            control.list_filter_input(value);
        }
    }

    pub fn dispatch_filter_composition(&self, start: bool) {
        if let Some(control) = self.control() {
            control.list_filter_composition(start);
        }
    }

    pub fn keybinding_labels(&self) -> TabbedActionListKeybindingLabels {
        get_tabbed_action_list_keybinding_labels(self.keybinding_service.as_ref())
    }

    /// A largura natural de uma aba com o texto `text`.
    fn natural_tab_box(&self, text: &str, has_image_icon: bool) -> TabBox {
        let (has_codicon, label) = match text.strip_prefix("$(").and_then(|rest| rest.split_once(')')) {
            Some((_, rest)) => (true, rest.trim_start()),
            None => (false, text),
        };
        let measurer = self.text_measurer.borrow().clone();
        let label_width = if label.is_empty() { 0.0 } else { measurer(label) };
        let icon_width = if has_codicon || has_image_icon { TAB_ICON_WIDTH + TAB_ICON_MARGIN } else { 0.0 };
        TabBox {
            width: 2.0 * TAB_PADDING_HORIZONTAL + icon_width + label_width,
            padding_left: format!("{TAB_PADDING_HORIZONTAL}px"),
            padding_right: format!("{TAB_PADDING_HORIZONTAL}px"),
            column_gap: "0px".to_owned(),
        }
    }
}

impl IDisposable for TabbedActionListWidget {
    fn dispose(&self) {
        self.set_active_popup(None);
        self.on_did_change_tab.dispose();
        self.on_did_hide.dispose();
    }
}

/// O delegate da visão de contexto de um `show`.
struct TabbedViewDelegate<T: ActionListItemData> {
    owner: Weak<TabbedActionListWidget>,
    popup_id: u64,
    options: ITabbedActionListShowOptions<T>,
    is_swap: bool,
    list_ref: Rc<RefCell<Option<Rc<ActionList<T>>>>>,
    anchor: AnchorTarget,
    layer: Option<i32>,
}

impl<T: ActionListItemData> IContextViewDelegate for TabbedViewDelegate<T> {
    fn get_anchor(&self) -> AnchorTarget {
        self.anchor.clone()
    }

    fn render(&self, container: &Rc<ViewElement>) -> Option<DisposableRef> {
        let owner = self.owner.upgrade()?;
        let render_disposables = DisposableStore::new();
        let popup = RenderedPopup::create(
            owner.clone(),
            self.popup_id,
            self.options.clone(),
            self.is_swap,
            container,
            render_disposables.clone(),
            self.list_ref.clone(),
        );
        popup.build();
        Some(render_disposables)
    }

    fn on_hide(&self, _data: Option<HideData>) {
        *self.list_ref.borrow_mut() = None;
        let Some(owner) = self.owner.upgrade() else { return };
        // Skip consumer callbacks during a tab swap: we are about to re-show with the same anchor, so the consumer should not
        // e.g. refocus the trigger button between hide and show.
        if owner.state.swapping_tab.get() {
            return;
        }
        // External dismissal (Escape, click outside): clear our own tracker so `isVisible` reflects reality. Done before firing
        // consumer callbacks in case they re-show.
        let is_current = owner.state.active_popup.borrow().as_ref().is_some_and(|(id, _)| *id == self.popup_id);
        if is_current {
            owner.set_active_popup(None);
        }
        self.options.delegate.on_hide(None);
        owner.on_did_hide.fire(());
    }

    fn anchor_position(&self) -> Option<AnchorPosition> {
        self.list_ref.borrow().as_ref().and_then(|list| list.anchor_position())
    }

    fn layer(&self) -> Option<i32> {
        self.layer
    }
}

/// O estado de um popup renderizado (as variáveis locais do `render` do original).
struct RenderedPopup<T: ActionListItemData> {
    this: Weak<RenderedPopup<T>>,
    owner: Rc<TabbedActionListWidget>,
    popup_id: u64,
    options: ITabbedActionListShowOptions<T>,
    is_swap: bool,
    render_disposables: Rc<DisposableStore>,
    list_ref: Rc<RefCell<Option<Rc<ActionList<T>>>>>,
    elements: BaseElements,
    footer: RefCell<Option<Rc<ViewElement>>>,
    empty_body: RefCell<Option<Rc<ViewElement>>>,
    active_tab: RefCell<String>,
    widget_class_names: RefCell<Vec<String>>,
    has_rendered: Cell<bool>,
    tab_texts: RefCell<BTreeMap<String, String>>,
    tab_elements: RefCell<Vec<TabElement>>,
    action_elements: RefCell<Vec<TabBarActionElement>>,
    list: RefCell<Option<Rc<ActionList<T>>>>,
    focus: Cell<PopupFocus>,
    /// `body.inert` e `main.inert`.
    body_inert: Cell<bool>,
    main_inert: Cell<bool>,
    main_aria_hidden: Cell<bool>,
    body_animation: Cell<Option<BodyAnimation>>,
    body_animation_timer: Cell<Option<TimerId>>,
    pending_refresh: RefCell<Option<ITabbedActionListRefreshOptions>>,
    label_holds: RefCell<BTreeMap<usize, Rc<DisposableStore>>>,
    details: RefCell<Option<DetailsPage>>,
    details_disposables: RefCell<Option<Rc<DisposableStore>>>,
    sizing_tab: Option<String>,
}

/// A página de detalhes aberta e o que o `goBack` fecha em volta dela.
struct DetailsPage {
    options: ITabbedActionListDetailsOptions,
    page: Rc<ViewElement>,
    header: Rc<ViewElement>,
    content: Rc<ViewElement>,
    back: DetailsBackButton,
    previous_focus: PopupFocus,
    page_height: f64,
    show_above: bool,
    viewport_height: Cell<f64>,
    scroll_top: Cell<f64>,
}

impl<T: ActionListItemData> RenderedPopup<T> {
    fn create(
        owner: Rc<TabbedActionListWidget>,
        popup_id: u64,
        options: ITabbedActionListShowOptions<T>,
        is_swap: bool,
        container: &Rc<ViewElement>,
        render_disposables: Rc<DisposableStore>,
        list_ref: Rc<RefCell<Option<Rc<ActionList<T>>>>>,
    ) -> Rc<Self> {
        let widget = ViewElement::with_classes(&["action-widget"]);
        widget.set_parent(Some(container.clone()));
        // Intercept mouse-down events outside the picker so those clicks do not interact with controls behind it.
        let block = ViewElement::with_classes(&["context-view-block"]);
        block.set_parent(Some(container.clone()));
        let main = ViewElement::with_classes(&["tabbed-action-list-main"]);
        main.set_parent(Some(widget.clone()));
        let body = ViewElement::with_classes(&["tabbed-action-list-body"]);
        body.set_parent(Some(main.clone()));
        let body_content = ViewElement::with_classes(&["tabbed-action-list-body-content"]);
        body_content.set_parent(Some(body.clone()));
        let tab_bar = ViewElement::with_classes(&["tabbed-action-list-tabbar"]);
        tab_bar.set_parent(Some(body_content.clone()));
        if let Some(class_name) = &options.tab_bar_class_name {
            tab_bar.class_list_add(class_name);
        }
        // A consumer showing a filter hides the strip and takes its place, so the trailing actions never move.
        let tab_strip = ViewElement::with_classes(&["tabbed-action-list-tabstrip"]);
        tab_strip.set_parent(Some(tab_bar.clone()));
        let filter_slot = ViewElement::with_classes(&["tabbed-action-list-filter-slot"]);
        filter_slot.set_parent(Some(tab_bar.clone()));

        let sizing_tab = options.tabs.iter().find(|tab| Some(&tab.id) == options.sizing_tab.as_ref()).map(|tab| tab.id.clone());
        Rc::new_cyclic(|this| Self {
            this: this.clone(),
            owner,
            popup_id,
            active_tab: RefCell::new(options.initial_tab.clone()),
            options,
            is_swap,
            render_disposables,
            list_ref,
            elements: BaseElements { widget, block, main, body, body_content, tab_bar, tab_strip, filter_slot },
            footer: RefCell::new(None),
            empty_body: RefCell::new(None),
            widget_class_names: RefCell::new(Vec::new()),
            has_rendered: Cell::new(false),
            tab_texts: RefCell::new(BTreeMap::new()),
            tab_elements: RefCell::new(Vec::new()),
            action_elements: RefCell::new(Vec::new()),
            list: RefCell::new(None),
            focus: Cell::new(PopupFocus::List),
            body_inert: Cell::new(false),
            main_inert: Cell::new(false),
            main_aria_hidden: Cell::new(false),
            body_animation: Cell::new(None),
            body_animation_timer: Cell::new(None),
            pending_refresh: RefCell::new(None),
            label_holds: RefCell::new(BTreeMap::new()),
            details: RefCell::new(None),
            details_disposables: RefCell::new(None),
            sizing_tab,
        })
    }

    fn list(&self) -> Rc<ActionList<T>> {
        self.list.borrow().clone().expect("a lista é criada no começo do render")
    }

    fn is_body_collapsed(&self) -> bool {
        self.options.is_body_collapsed.as_ref().is_some_and(|collapsed| collapsed())
    }

    fn motion_reduced(&self) -> bool {
        self.owner.accessibility_service.is_motion_reduced()
    }

    fn layout_context_view(&self) {
        self.owner.context_view_service.layout();
    }

    /// `hide()` do popup: só tem efeito enquanto ele é o ativo.
    fn hide(&self) {
        self.owner.hide_popup(self.popup_id);
    }

    // #region foco

    /// `list.focus()` fora de um controle da barra, do rodapé ou da página.
    fn focus_list(&self) {
        self.focus.set(PopupFocus::List);
        self.list().focus();
    }

    fn list_has_focus_now(&self) -> bool {
        let widget = self.list();
        let widget = widget.widget();
        widget.is_list_focused() || widget.filter_input().is_some_and(|input| input.is_focused()) || widget.panel_contains_focus()
    }

    /// `dom.isAncestorOfActiveElement(body)`: a lista, as abas e os botões da barra ficam no corpo.
    fn focus_in_body(&self) -> bool {
        match self.focus.get() {
            PopupFocus::List => self.list_has_focus_now(),
            PopupFocus::Tab(_) | PopupFocus::TabBarAction(_) => true,
            PopupFocus::Footer | PopupFocus::DetailsBack | PopupFocus::DetailsPage | PopupFocus::DetailsContent => false,
        }
    }

    /// `dom.isAncestorOfActiveElement(main)`: o corpo e o rodapé.
    fn focus_in_main(&self) -> bool {
        self.focus_in_body() || self.focus.get() == PopupFocus::Footer
    }

    /// `dom.isAncestorOfActiveElement(widget)`.
    fn focus_in_widget(&self) -> bool {
        self.focus_in_main() || matches!(self.focus.get(), PopupFocus::DetailsBack | PopupFocus::DetailsPage | PopupFocus::DetailsContent)
    }

    fn focus_footer(&self) {
        if let Some(focus_footer) = &self.options.focus_footer {
            focus_footer();
        }
    }

    /// `radio.focusActiveItem()`.
    fn focus_active_tab(&self) {
        let active = self.active_tab.borrow().clone();
        let index = self.options.tabs.iter().position(|tab| tab.id == active).unwrap_or(0);
        if let Some(input) = self.list().filter_input() {
            input.blur();
        }
        self.focus.set(PopupFocus::Tab(index));
    }

    // #endregion

    // #region geometria

    fn footer_height(&self) -> f64 {
        self.footer.borrow().as_ref().map_or(0.0, |footer| footer.size().height)
    }

    /// `body.offsetHeight`: a animação em andamento, ou zero recolhido, ou a altura do conteúdo.
    fn body_height(&self) -> f64 {
        if let Some(animation) = self.body_animation.get() {
            return animation.current_height();
        }
        if self.body_inert.get() { 0.0 } else { self.body_content_height() }
    }

    /// `bodyContent.offsetHeight`: a barra de abas, e a lista com o que a rodeia ou o corpo vazio.
    fn body_content_height(&self) -> f64 {
        let list = self.list();
        let filter_in_bar = self.options.filter_in_tab_bar && list.filter_container().is_some() && self.empty_body.borrow().is_none();
        let tab_bar = TAB_BAR_PADDING_BOTTOM + if filter_in_bar { FILTER_HEIGHT } else { TAB_BUTTON_HEIGHT };
        let rest = match self.empty_body.borrow().as_ref() {
            Some(empty) => empty.size().height,
            None => list.chrome_height() - if filter_in_bar { FILTER_HEIGHT } else { 0.0 } + list.dom_node().size().height,
        };
        tab_bar + rest
    }

    /// `widget.offsetHeight - body.offsetHeight + bodyContent.offsetHeight`.
    fn expanded_popup_height(&self) -> f64 {
        self.elements.widget.size().height - self.elements.body.size().height + self.elements.body_content.size().height
    }

    /// Padding belongs to the popup's chrome, not the list's content height.
    fn chrome_height(&self, list_element: &Rc<ViewElement>) -> f64 {
        let content = match self.empty_body.borrow().as_ref() {
            Some(empty) => empty.size().height,
            None => list_element.size().height,
        };
        self.expanded_popup_height() - content
    }

    fn details_header_height(page: &DetailsPage) -> f64 {
        DETAILS_BACK_HEIGHT.max(page.header.size().height) + 2.0 * DETAILS_HEADER_PADDING
    }

    /// O refluxo do popup: o que o DOM mediria em `offsetHeight` e `offsetWidth` de cada elemento.
    fn install_measures(&self) {
        let weak = self.this.clone();
        let measure = move |select: fn(&RenderedPopup<T>) -> f64| -> Rc<dyn Fn() -> je_base::layout::ISize> {
            let weak = weak.clone();
            Rc::new(move || {
                let height = weak.upgrade().map_or(0.0, |popup| select(&popup));
                let width = weak.upgrade().map_or(0.0, |popup| popup.widget_width());
                je_base::layout::ISize { width, height }
            })
        };
        self.elements.widget.set_measure(measure(|popup| {
            if popup.owner.state.details_visible.get() {
                let details = popup.details.borrow();
                let page = details.as_ref().map_or(0.0, |page| Self::details_header_height(page) + page.viewport_height.get());
                return page + POPUP_CHROME;
            }
            popup.elements.main.size().height + POPUP_CHROME
        }));
        self.elements.main.set_measure(measure(|popup| popup.elements.body.size().height + popup.footer_height()));
        self.elements.body.set_measure(measure(RenderedPopup::body_height));
        self.elements.body_content.set_measure(measure(RenderedPopup::body_content_height));
    }

    /// `parseFloat(widget.style.width)`.
    fn widget_width(&self) -> f64 {
        parse_px(self.elements.widget.style_get_property("width"))
    }

    // #endregion

    fn apply_widget_class_names(&self) {
        let active = self.active_tab.borrow().clone();
        let next: Vec<String> = self
            .options
            .widget_class_names
            .as_ref()
            .map(|names| names(&active))
            .unwrap_or_default()
            .into_iter()
            .filter(|class_name| class_name != ACTION_WIDGET_DROPDOWN_MOTION_CLASS || (self.has_rendered.get() && !self.is_swap))
            .collect();
        let previous = self.widget_class_names.borrow().clone();
        for removed in previous.iter().filter(|name| !next.contains(name)) {
            self.elements.widget.class_list_remove(removed);
        }
        for added in next.iter().filter(|name| !previous.contains(name)) {
            self.elements.widget.class_list_add(added);
        }
        *self.widget_class_names.borrow_mut() = next;
    }

    fn activate_tab(&self, next: &str) {
        if *self.active_tab.borrow() == next {
            return;
        }
        *self.active_tab.borrow_mut() = next.to_owned();
        self.owner.on_did_change_tab.fire(next.to_owned());
        let mut options = self.options.clone();
        options.initial_tab = next.to_owned();
        self.owner.show(options);
    }

    fn switch_tab(&self, delta: isize) {
        let active = self.active_tab.borrow().clone();
        if let Some(current) = self.options.tabs.iter().position(|tab| tab.id == active) {
            let length = self.options.tabs.len() as isize;
            let next = ((current as isize + delta + length) % length) as usize;
            self.activate_tab(&self.options.tabs[next].id.clone());
        }
    }

    /// `list.computeHeightForItems(sizing.items, sizing.listOptions?.collapsedByDefault, sizing.listOptions) || undefined`.
    fn measure_sizing(&self, sizing: &ITabbedActionListBuildResult<T>) -> Option<f64> {
        let options = sizing.list_options.as_ref();
        let height = self.list().widget().compute_height_for_items(&sizing.items, options.and_then(|o| o.collapsed_by_default.as_ref()), options);
        if height == 0.0 { None } else { Some(height) }
    }

    fn layout(&self) {
        if self.owner.state.details_visible.get() {
            return;
        }
        let list = self.list();
        let state = &self.owner.state;
        let content_height = match state.fixed_popup_height.get() {
            None => state.fixed_list_height.get(),
            Some(popup_height) => Some((popup_height - self.chrome_height(&list.dom_node())).max(0.0)),
        };
        let width = list.layout(0.0, content_height);
        self.elements.widget.style_set_property("width", &format!("{}px", self.options.width.unwrap_or(width)));
        if let (Some(empty), Some(_)) = (self.empty_body.borrow().as_ref(), state.fixed_list_height.get()) {
            if let Some(height) = list.dom_node().style_get_property("height") {
                empty.style_set_property("min-height", &height);
            }
        }
        if state.fixed_list_height.get().is_some() && state.fixed_popup_height.get().is_none() {
            state.fixed_popup_height.set(Some(self.expanded_popup_height()));
        }
    }

    // #region corpo recolhível

    fn clear_body_animation(&self) {
        if let Some(timer) = self.body_animation_timer.take() {
            clear_timeout(timer);
        }
        if self.body_animation.take().is_some() {
            self.elements.body.class_list_remove("animating");
        }
    }

    fn finish_body_animation_event(&self) {
        self.clear_body_animation();
        self.layout_context_view();
        self.list().widget().set_hover_enabled(!self.body_inert.get());
    }

    /// `dom.animate(window, () => contextViewService.layout())`: um quadro por vez enquanto a animação roda.
    fn schedule_body_frame(&self) {
        let weak = self.this.clone();
        let timer = set_timeout(
            move || {
                let Some(popup) = weak.upgrade() else { return };
                popup.body_animation_timer.set(None);
                let Some(mut animation) = popup.body_animation.get() else { return };
                if !animation.paused {
                    animation.current_time += ANIMATION_FRAME_MS as f64;
                    popup.body_animation.set(Some(animation));
                }
                popup.layout_context_view();
                if !animation.paused && animation.current_time >= animation.duration as f64 {
                    popup.finish_body_animation_event();
                } else {
                    popup.schedule_body_frame();
                }
            },
            ANIMATION_FRAME_MS,
        );
        self.body_animation_timer.set(Some(timer));
    }

    fn update_body_collapsed(&self, from_height: Option<f64>) {
        let collapsed = self.is_body_collapsed();
        if self.body_inert.get() == collapsed {
            return;
        }
        let list = self.list();
        // `fromHeight` e a altura de partida lidos antes de trocar o estado: a animação em curso conta.
        self.clear_body_animation();
        list.widget().set_hover_enabled(false);
        self.body_inert.set(collapsed);
        if collapsed {
            self.elements.body.class_list_add("collapsed");
        } else {
            self.elements.body.class_list_remove("collapsed");
        }

        if let Some(from_height) = from_height {
            if !self.motion_reduced() {
                self.elements.body.class_list_add("animating");
                self.body_animation.set(Some(BodyAnimation {
                    from_height,
                    to_height: if collapsed { 0.0 } else { self.elements.body_content.size().height },
                    duration: BODY_COLLAPSE_ANIMATION_DURATION_MS,
                    current_time: 0.0,
                    paused: false,
                }));
                self.schedule_body_frame();
            }
            self.layout_context_view();
        }
        if self.body_animation.get().is_none() {
            list.widget().set_hover_enabled(!collapsed);
        }
    }

    // #endregion

    // #region atualização

    fn refresh_impl(&self, refresh: Option<ITabbedActionListRefreshOptions>) {
        let refresh = refresh.unwrap_or_default();
        if self.owner.state.details_visible.get() {
            // Keep the popup's size and scroll position afterwards only if every deferred request asked to, so a refresh that has
            // to re-measure still does.
            let mut pending = self.pending_refresh.borrow_mut();
            let keeps = |ask: bool, previous: Option<bool>| ask && previous.unwrap_or(true);
            *pending = Some(ITabbedActionListRefreshOptions {
                preserve_size: keeps(refresh.preserve_size, pending.as_ref().map(|p| p.preserve_size)),
                preserve_scroll_position: keeps(refresh.preserve_scroll_position, pending.as_ref().map(|p| p.preserve_scroll_position)),
                ..ITabbedActionListRefreshOptions::default()
            });
            return;
        }
        finish_action_widget_opening_animation(&self.elements.widget);
        let list = self.list();
        let had_focus = self.focus_in_widget();
        let body_height = self.elements.body.size().height;
        if self.is_body_collapsed() && self.focus_in_body() {
            self.focus_footer();
        }
        if self.body_inert.get() != self.is_body_collapsed() {
            // Refreshing must not anchor a hover against the body's old position.
            list.widget().set_hover_enabled(false);
        }
        self.apply_widget_class_names();
        let active = self.active_tab.borrow().clone();
        let sizing = match (&self.sizing_tab, refresh.preserve_size) {
            (Some(sizing_tab), false) => Some((self.options.create_action_list)(sizing_tab, true)),
            _ => None,
        };
        let refreshed = (self.options.create_action_list)(&active, false);
        if let Some(header) = list.header_container() {
            if refreshed.list_options.as_ref().and_then(|o| o.header_text.as_ref()).is_some() {
                header.show();
            } else {
                header.hide();
            }
        }
        let sizing_height = sizing.as_ref().and_then(|sizing| self.measure_sizing(sizing));
        let state = &self.owner.state;
        let sizing_changed = !refresh.preserve_size && sizing_height != state.fixed_list_height.get();
        if sizing_changed {
            state.fixed_list_height.set(sizing_height);
            state.fixed_popup_height.set(None);
        }
        list.update_items(
            &refreshed.items,
            refresh.focus_item_id.as_deref(),
            Some(IActionListUpdateOptions {
                preserve_hover: refresh.preserve_hover,
                preserve_scroll_position: refresh.preserve_scroll_position,
                animate_item_move: refresh.animate_item_move && !self.motion_reduced(),
            }),
        );
        if sizing_changed {
            self.layout();
            self.layout_context_view();
        }
        self.update_body_collapsed(Some(body_height));
        if had_focus && !self.focus_in_widget() {
            if self.body_inert.get() {
                self.focus_footer();
            } else if self.empty_body.borrow().is_some() {
                self.focus_active_tab();
            } else {
                self.focus_list();
            }
        }
    }

    // #endregion

    // #region detalhes

    fn show_details_impl(&self, details: ITabbedActionListDetailsOptions) {
        let list = self.list();
        let previous_focus = match self.focus.get() {
            PopupFocus::List if self.list_has_focus_now() => Some(PopupFocus::List),
            PopupFocus::List => None,
            other => Some(other),
        };
        let store = DisposableStore::new();
        let previous_store = self.details_disposables.borrow_mut().replace(store.clone());
        if let Some(previous_store) = previous_store {
            previous_store.dispose();
        }
        self.owner.state.details_visible.set(true);
        self.clear_body_animation();
        list.widget().set_hover_enabled(false);
        let main = &self.elements.main;
        main.style_set_property("width", &format!("{}px", self.widget_width()));
        let anchor = get_anchor_rect(&self.options.anchor);
        let show_above = list.anchor_position() == Some(AnchorPosition::Above)
            || (list.anchor_position().is_none() && self.elements.widget.rect().top + self.elements.widget.rect().height <= anchor.top);

        let page = ViewElement::with_classes(&["tabbed-action-list-details"]);
        page.set_parent(Some(self.elements.widget.clone()));
        let page_for_removal = page.clone();
        store.add(to_disposable(move || page_for_removal.set_parent(None)));
        let header = ViewElement::with_classes(&["tabbed-action-list-details-header"]);
        header.set_parent(Some(page.clone()));
        let back_keybinding = get_keybinding_label(self.owner.keybinding_service.as_ref(), details_back_keybinding());
        let back_title = match back_keybinding {
            Some(label) => format!("{} ({})", details.back_label, label),
            None => details.back_label.clone(),
        };
        let back = if let Some(render_header) = &details.render_header {
            let back = DetailsBackButton {
                aria_label: back_title.clone(),
                title: back_title.clone(),
                text: String::new(),
                icon_action: true,
                icon_class: "codicon-arrow-left".to_owned(),
                text_button: false,
            };
            store.add(render_header(&header));
            back
        } else {
            DetailsBackButton {
                aria_label: back_title.clone(),
                title: back_title,
                text: details.back_label.clone(),
                icon_action: false,
                icon_class: String::new(),
                text_button: true,
            }
        };

        let viewport = ViewElement::with_classes(&["tabbed-action-list-details-viewport"]);
        let content = ViewElement::with_classes(&["tabbed-action-list-details-content"]);
        content.set_parent(Some(viewport.clone()));
        viewport.set_parent(Some(page.clone()));
        store.add((details.render)(&content));
        let header_height = DETAILS_BACK_HEIGHT.max(header.size().height) + 2.0 * DETAILS_HEADER_PADDING;
        let page_height = DETAILS_MAX_HEIGHT.min(main.size().height.max(header_height + content.size().height));
        *self.details.borrow_mut() = Some(DetailsPage {
            options: details.clone(),
            page: page.clone(),
            header,
            content,
            back,
            previous_focus,
            page_height,
            show_above,
            viewport_height: Cell::new(0.0),
            scroll_top: Cell::new(0.0),
        });
        main.class_list_add("inert");
        self.main_inert.set(true);
        self.main_aria_hidden.set(true);
        self.elements.widget.class_list_add("showing-details");
        self.layout_details();
        // Focusing before layout can scroll the anchor out of place.
        match &details.focus {
            Some(focus) => focus(&page),
            None => self.focus.set(PopupFocus::DetailsBack),
        }
    }

    fn layout_details(&self) {
        let details = self.details.borrow();
        let Some(page) = details.as_ref() else { return };
        let anchor = get_anchor_rect(&self.options.anchor);
        let window = self.owner.layout_service.window_viewport();
        let available = if page.show_above { anchor.top } else { window.inner_height - (anchor.top + anchor.height) };
        let height = (page.page_height).min(available - DETAILS_EDGE_MARGIN).max(0.0);
        page.viewport_height.set((height - Self::details_header_height(page)).max(0.0));
        let max_scroll = (page.content.size().height - page.viewport_height.get()).max(0.0);
        page.scroll_top.set(page.scroll_top.get().min(max_scroll));
        drop(details);
        self.layout_context_view();
    }

    fn go_back(&self) {
        if self.details.borrow().is_none() {
            return;
        }
        let state = &self.owner.state;
        state.details_visible.set(false);
        self.elements.widget.class_list_remove("showing-details");
        self.elements.main.class_list_remove("inert");
        self.main_inert.set(false);
        self.main_aria_hidden.set(false);
        self.elements.main.style_remove_property("width");
        let store = self.details_disposables.borrow_mut().take();
        if let Some(store) = store {
            store.dispose();
        }
        let details = self.details.borrow_mut().take().expect("a página está aberta");
        let pending = self.pending_refresh.borrow_mut().take();
        if let Some(pending) = pending {
            self.refresh_impl(Some(pending));
        }
        self.layout();
        self.layout_context_view();
        let list = self.list();
        list.widget().set_hover_enabled(!self.body_inert.get());
        let restored = details.options.restore_focus.as_ref().is_some_and(|restore| restore());
        if !restored {
            // A focus that was inside `main` and is not under an inert subtree comes back to where it was.
            let returns = match details.previous_focus {
                Some(PopupFocus::List | PopupFocus::Tab(_) | PopupFocus::TabBarAction(_)) => !self.body_inert.get(),
                Some(PopupFocus::Footer) => true,
                _ => false,
            };
            if returns {
                self.focus.set(details.previous_focus.unwrap_or(PopupFocus::List));
            } else if !self.focus_in_main() {
                self.focus.set(PopupFocus::List);
            }
            if !self.focus_in_main() {
                if self.body_inert.get() {
                    self.focus_footer();
                } else {
                    self.focus_list();
                }
            }
        }
        if let Some(on_back) = &details.options.on_back {
            on_back();
        }
    }

    /// O tratador de teclas da página de detalhes (registrado em captura): devolve se tratou o evento.
    fn dispatch_details_key_down(&self, e: &UiKeyEvent) -> bool {
        if e.is_composing {
            return false;
        }
        if e.key_code == KeyCode::Escape {
            e.stop();
            self.hide();
        } else if key_event_equals(e, details_back_keybinding()) {
            e.stop();
            self.go_back();
        } else if e.key_code == KeyCode::PageDown || e.key_code == KeyCode::PageUp {
            e.stop();
            self.scroll_details(if e.key_code == KeyCode::PageDown { 1.0 } else { -1.0 });
        } else {
            return false;
        }
        true
    }

    fn scroll_details(&self, direction: f64) {
        let details = self.details.borrow();
        let Some(page) = details.as_ref() else { return };
        let max_scroll = (page.content.size().height - page.viewport_height.get()).max(0.0);
        page.scroll_top.set((page.scroll_top.get() + direction * page.viewport_height.get()).clamp(0.0, max_scroll));
    }

    // #endregion

    // #region abas

    fn build_tabs(&self) {
        let owner = &self.owner;
        let active = self.active_tab.borrow().clone();
        let label_mode = self.options.tab_labels.unwrap_or_default();
        let mut texts = BTreeMap::new();
        let mut elements = Vec::new();
        for tab in &self.options.tabs {
            let label = tab.label.clone().unwrap_or_else(|| tab.id.clone());
            // An image icon is drawn by the tab itself rather than through its text.
            let icon_prefix = match &tab.icon {
                Some(TabIcon::Theme(id)) => format!("$({id})"),
                _ => String::new(),
            };
            let shows_label = tab.icon.is_none() || label_mode == TabLabels::Always || (label_mode == TabLabels::Active && tab.id == active);
            let text = if shows_label {
                if icon_prefix.is_empty() { label.clone() } else { format!("{icon_prefix} {label}") }
            } else {
                icon_prefix
            };
            let is_active = tab.id == active;
            let mut classes = vec!["monaco-button".to_owned()];
            if !text.is_empty() {
                classes.push("monaco-text-button".to_owned());
            }
            if is_active {
                classes.push("active".to_owned());
            }
            let mut image_icon = None;
            if let Some(TabIcon::Image { light, dark }) = &tab.icon {
                // A button only styles itself as a text button once it has text, which a tab showing just its image never gets,
                // so it would lose the other tabs' styling.
                for class_name in ["monaco-text-button", "tabbed-action-list-tab-image-icon"] {
                    if !classes.iter().any(|class| class == class_name) {
                        classes.push(class_name.to_owned());
                    }
                }
                image_icon = Some((light.clone(), dark.clone()));
            }
            let box_ = owner.natural_tab_box(&text, image_icon.is_some());
            texts.insert(tab.id.clone(), text.clone());
            elements.push(TabElement {
                id: tab.id.clone(),
                text,
                tooltip: tab.tooltip.clone().unwrap_or_else(|| label.clone()),
                aria_label: label,
                is_active,
                classes,
                image_icon,
                label_override: None,
                box_,
                animations: Vec::new(),
            });
        }
        *self.tab_texts.borrow_mut() = texts;
        *self.tab_elements.borrow_mut() = elements;
    }

    fn build_tab_bar_actions(&self) {
        let mut elements = Vec::new();
        for tab_action in &self.options.tab_bar_actions {
            let mut classes = vec!["tabbed-action-list-tabbar-action".to_owned()];
            if tab_action.align_end {
                classes.push("align-end".to_owned());
            }
            if tab_action.checked == Some(true) {
                classes.push("checked".to_owned());
            }
            elements.push(TabBarActionElement {
                id: tab_action.id.clone(),
                aria_label: tab_action.tooltip.clone(),
                title: if tab_action.use_managed_hover { None } else { Some(tab_action.tooltip.clone()) },
                managed_hover: tab_action.use_managed_hover.then(|| tab_action.tooltip.clone()),
                // Only an action with a real on/off state announces as a toggle. A momentary one would otherwise read as a toggle
                // that is switched off.
                aria_pressed: tab_action.checked.map(|checked| checked.to_string()),
                classes,
                icon_class: format!("codicon-{}", tab_action.icon),
                in_tab_bar: tab_action.align_end,
            });
        }
        *self.action_elements.borrow_mut() = elements;
    }

    /// `_renderEmptyBody`: renders the caller's empty body, or nothing when it declines to handle the empty tab.
    fn render_empty_body(&self, active_tab: &str) -> Option<Rc<ViewElement>> {
        let render_empty = self.options.render_empty.as_ref()?;
        let body = ViewElement::with_classes(&["tabbed-action-list-empty"]);
        body.set_parent(Some(self.elements.body_content.clone()));
        match render_empty(&body, active_tab) {
            Some(rendered) => {
                self.render_disposables.add(rendered);
                Some(body)
            }
            None => {
                body.set_parent(None);
                None
            }
        }
    }

    /// `_measureTabBoxes`: boxes of the currently rendered tabs, keyed by tab id.
    fn measure_tab_boxes(&self) -> BTreeMap<String, TabBox> {
        self.tab_elements.borrow().iter().map(|tab| (tab.id.clone(), tab.box_.clone())).collect()
    }

    /// `_animateTabResize`: grows and shrinks the tabs from the boxes the previous render left them in.
    fn animate_tab_resize(&self, boxes: &BTreeMap<String, TabBox>) {
        let previous_boxes = self.owner.state.previous_tab_boxes.borrow().clone();
        let previous_texts = self.owner.state.previous_tab_texts.borrow().clone();
        let Some(previous_boxes) = previous_boxes else { return };
        if self.motion_reduced() {
            return;
        }
        let texts = self.tab_texts.borrow().clone();
        let tab_count = self.options.tabs.len();
        for index in 0..tab_count {
            let id = self.options.tabs[index].id.clone();
            let (Some(from), Some(to)) = (previous_boxes.get(&id), boxes.get(&id)) else { continue };
            // A tab that is not laid out yet measures 0, which would animate it away.
            if to.width <= 0.0 || (from.width - to.width).abs() < 1.0 {
                continue;
            }
            self.tab_elements.borrow_mut()[index].animations.push(TabAnimation {
                from: from.clone(),
                to: to.clone(),
                duration: TAB_RESIZE_ANIMATION_DURATION_MS,
                easing: ANIMATION_EASING,
                finished: false,
                label_max_width: None,
            });

            // Without its old label a closing tab drops the text at once and slides a lone icon across the space it filled.
            let previous_text = previous_texts.as_ref().and_then(|texts| texts.get(&id)).cloned();
            let current_text = texts.get(&id).cloned();
            if to.width < from.width {
                if let Some(previous_text) = previous_text {
                    if Some(&previous_text) != current_text.as_ref() {
                        self.hold_label_while_shrinking(index, &previous_text);
                    }
                }
            }
        }
    }

    /// `_holdLabelWhileShrinking`: keeps a closing tab's label on screen, collapsed to nothing by the time it settles.
    fn hold_label_while_shrinking(&self, index: usize, previous_text: &str) {
        let store = DisposableStore::new();
        {
            let mut tabs = self.tab_elements.borrow_mut();
            let tab = &mut tabs[index];
            tab.label_override = Some(previous_text.to_owned());
            if !tab.has_class("label-collapsing") {
                tab.classes.push("label-collapsing".to_owned());
            }
            // The label has to reach zero width, or it still displaces the resting icon.
            let label = TabElement { label_override: None, text: previous_text.to_owned(), ..tab.clone() }.text_content();
            let measurer = self.owner.text_measurer.borrow().clone();
            let scroll_width = if label.is_empty() { 0.0 } else { measurer(&label) };
            if let Some(animation) = tab.animations.last_mut() {
                animation.label_max_width = Some((scroll_width, 0.0));
            }
        }
        let weak = self.this.clone();
        store.add(to_disposable(move || {
            let Some(popup) = weak.upgrade() else { return };
            let mut tabs = popup.tab_elements.borrow_mut();
            if let Some(tab) = tabs.get_mut(index) {
                tab.label_override = None;
                tab.classes.retain(|class| class != "label-collapsing");
            }
        }));
        if let Some(previous) = self.label_holds.borrow_mut().insert(index, store) {
            previous.dispose();
        }
    }

    // #endregion

    // #region teclado

    fn dispatch_key_down_impl(&self, e: &UiKeyEvent, target: TabbedKeyTarget) {
        // Programmatic page focus can match :focus-visible even after a pointer click.
        self.elements.widget.class_list_add("keyboard-navigation");
        if self.owner.state.details_visible.get() && target.on_own_controls && self.dispatch_details_key_down(e) {
            return;
        }
        let list = self.list();
        if let Some(list_target) = target.list {
            list.widget().dispatch_key_down(e, list_target);
            if e.propagation_stopped() {
                return;
            }
        }
        let on_tab_bar = target.on_tab_bar;
        let on_footer = target.on_footer;
        let on_editable = target.on_editable;
        // The empty body and the hover panel carry controls of their own, e.g. a sign-in button or the detail card's pin, and so
        // do rows with a toolbar or switch. Keys pressed there belong to those controls rather than to the list sitting behind
        // them.
        let on_own_controls = target.on_own_controls;
        let list_navigation = !on_tab_bar && !on_footer && !on_own_controls;

        if e.key_code == KeyCode::Escape {
            e.stop();
            self.hide();
            return;
        }
        if e.key_code == KeyCode::Enter && list_navigation {
            e.stop();
            list.accept_selected(false);
            return;
        }
        if e.key_code == KeyCode::UpArrow && list_navigation {
            e.stop();
            list.focus_previous();
            return;
        }
        if e.key_code == KeyCode::DownArrow && list_navigation {
            e.stop();
            list.focus_next();
            return;
        }
        let tab_delta = if key_event_equals(e, next_tab_keybinding()) {
            1
        } else if key_event_equals(e, previous_tab_keybinding()) {
            -1
        } else {
            0
        };
        if tab_delta != 0 {
            // Text fields keep their own keys, and the tabs are covered by details or a collapsed body.
            if !on_editable && !self.owner.state.details_visible.get() && !self.body_inert.get() {
                e.stop();
                self.switch_tab(tab_delta);
            }
            return;
        }
        if e.key_code != KeyCode::LeftArrow && e.key_code != KeyCode::RightArrow {
            return;
        }
        if e.alt_key || e.ctrl_key || e.meta_key || e.shift_key || on_footer || on_own_controls || on_editable {
            return;
        }
        // Left and Right move between tabs only on the tab bar. In the list they belong to the rows: like a tree, Left collapses
        // the focused row's section and Right expands it from its toggle, while Right on any other row opens that item's details.
        if on_tab_bar {
            e.stop();
            self.switch_tab(if e.key_code == KeyCode::RightArrow { 1 } else { -1 });
            return;
        }
        let focused = if target.list.is_some_and(|list_target| list_target != KeyTarget::FilterInput) { list.get_focused_element() } else { None };
        let Some(focused) = focused else { return };
        let handled = if e.key_code == KeyCode::LeftArrow {
            list.collapse_focused_section()
        } else if focused.is_section_toggle {
            list.expand_focused_section()
        } else {
            self.options.open_item_details.as_ref().is_some_and(|open| open(&focused))
        };
        if handled {
            e.stop();
        }
    }

    // #endregion

    /// O `render` do original, na mesma ordem.
    fn build(&self) {
        let owner = self.owner.clone();
        let render_disposables = self.render_disposables.clone();
        let el = &self.elements;
        self.install_measures();
        if let Some(width) = self.options.width {
            el.widget.style_set_property("width", &format!("{width}px"));
        }
        self.apply_widget_class_names();

        self.build_tabs();
        self.build_tab_bar_actions();

        // Built before the active tab's list because a consumer may hold per-tab state while building, and the active tab has to
        // be the one that keeps it.
        let needs_sizing = !owner.state.has_measured_sizing_tab.get() && self.sizing_tab.is_some();
        let sizing_build = match (&self.sizing_tab, needs_sizing) {
            (Some(sizing_tab), true) => Some((self.options.create_action_list)(sizing_tab, true)),
            _ => None,
        };

        let active = self.active_tab.borrow().clone();
        let ITabbedActionListBuildResult { items, list_options } = (self.options.create_action_list)(&active, false);
        *self.empty_body.borrow_mut() = if items.is_empty() { self.render_empty_body(&active) } else { None };
        let weak = self.this.clone();
        let mut list_options_value = list_options.clone().unwrap_or_default();
        list_options_value.anchor_position = owner.state.anchor_position.get().or(list_options_value.anchor_position);
        // The tabs are part of the popup, so they have to fit beside the anchor too.
        list_options_value.get_popup_chrome_height = Some(Rc::new(move |list_element: &Rc<ViewElement>| {
            weak.upgrade().map_or(0.0, |popup| popup.chrome_height(list_element))
        }));
        let list = render_disposables.add_rc(ActionList::new(
            false,
            &items,
            self.options.delegate.clone(),
            Some(Rc::new(list_options_value)),
            self.options.anchor.clone(),
            owner.context_view_service.clone(),
            owner.layout_service.clone(),
        ));
        *self.list.borrow_mut() = Some(list.clone());
        *self.list_ref.borrow_mut() = Some(list.clone());

        // Rebuilding has to ask the consumer again, since what the popup shows can depend on state that changed while it stayed
        // open.
        let popup = self.this.upgrade().expect("o popup está vivo durante o render");
        *owner.state.control.borrow_mut() = Some(popup as Rc<dyn PopupControl>);
        let teardown_weak = self.this.clone();
        render_disposables.add(to_disposable(move || {
            let Some(popup) = teardown_weak.upgrade() else { return };
            let owner = &popup.owner;
            let is_current = owner.state.active_popup.borrow().as_ref().map_or(true, |(id, _)| *id == popup.popup_id);
            if is_current {
                *owner.state.control.borrow_mut() = None;
                owner.state.details_visible.set(false);
            }
            popup.clear_body_animation();
            let holds: Vec<_> = std::mem::take(&mut *popup.label_holds.borrow_mut()).into_values().collect();
            for hold in holds {
                hold.dispose();
            }
            let details = popup.details_disposables.borrow_mut().take();
            if let Some(details) = details {
                details.dispose();
            }
        }));

        if self.empty_body.borrow().is_none() {
            if let Some(header) = list.header_container() {
                header.set_parent(Some(el.body_content.clone()));
            }
            if let Some(filter) = list.filter_container() {
                // The filter takes the tabs' place inside the bar, so the trailing actions stay put and no extra row appears.
                filter.set_parent(Some(if self.options.filter_in_tab_bar { el.filter_slot.clone() } else { el.body_content.clone() }));
            }
            list.dom_node().set_parent(Some(el.body_content.clone()));
            if let Some(footer) = list.footer_container() {
                footer.set_parent(Some(el.body_content.clone()));
            }
        }

        if let Some(render_footer) = &self.options.render_footer {
            let footer = ViewElement::with_classes(&["tabbed-action-list-footer"]);
            footer.set_parent(Some(el.main.clone()));
            *self.footer.borrow_mut() = Some(footer.clone());
            render_disposables.add(render_footer(&footer, &active));
        }

        // Measured from the sizing tab's own items, so every tab lands on the same height however the popup opened.
        if needs_sizing {
            let sizing = sizing_build.unwrap_or(ITabbedActionListBuildResult { items: items.clone(), list_options });
            owner.state.fixed_list_height.set(self.measure_sizing(&sizing));
            owner.state.has_measured_sizing_tab.set(true);
        }

        self.layout();
        if owner.state.anchor_position.get().is_none() {
            owner.state.anchor_position.set(list.anchor_position());
        }

        self.update_body_collapsed(None);
        if self.options.is_body_collapsed.is_some() {
            let weak = self.this.clone();
            render_disposables.add(owner.accessibility_service.on_did_change_reduced_motion().listen(move |_| {
                if let Some(popup) = weak.upgrade() {
                    if popup.motion_reduced() {
                        popup.finish_body_animation_event();
                    }
                }
            }));
        }

        // Boxes are read before the animation starts, so they are the resting ones.
        let tab_boxes = self.measure_tab_boxes();
        self.animate_tab_resize(&tab_boxes);
        *owner.state.previous_tab_boxes.borrow_mut() = Some(tab_boxes);
        *owner.state.previous_tab_texts.borrow_mut() = Some(self.tab_texts.borrow().clone());
        if self.body_inert.get() {
            self.focus_footer();
        } else if self.empty_body.borrow().is_some() {
            // The list is not in the DOM at all, so focusing it would drop focus out of the popup. The active tab is the nearest
            // thing to act on, and it leads to the empty body's own action.
            self.focus_active_tab();
        } else {
            self.focus_list();
        }

        self.has_rendered.set(true);
        self.apply_widget_class_names();
        if !self.is_swap {
            el.widget.class_list_add(ACTION_WIDGET_ANIMATED_CLASS);
        }
    }
}

/// `parseFloat("123px")`, zero se faltar.
fn parse_px(value: Option<String>) -> f64 {
    value.and_then(|value| value.trim_end_matches("px").parse::<f64>().ok()).unwrap_or(0.0)
}

/// O contorno do foco da página de detalhes (`.action-widget.showing-details .tabbed-action-list-details:focus` e a regra com
/// `.keyboard-navigation`): `None` é `outline: none`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DetailsFocusOutline {
    pub outline: String,
    pub offset: String,
}

/// O contorno da página de detalhes focada: só a navegação por teclado desenha, por dentro, com os tokens do tema.
pub fn details_focus_outline(keyboard_navigation: bool) -> Option<DetailsFocusOutline> {
    keyboard_navigation.then(|| DetailsFocusOutline {
        outline: "var(--vscode-strokeThickness) solid var(--vscode-focusBorder)".to_owned(),
        offset: "calc(-1 * var(--vscode-strokeThickness))".to_owned(),
    })
}

impl BodyState {
    /// `getComputedStyle(body).visibility`: `.collapsed` esconde, `.animating` mostra de volta.
    pub fn visibility(&self) -> &'static str {
        if self.collapsed && !self.animating { "hidden" } else { "visible" }
    }
}

impl TabElement {
    /// `getComputedStyle(tab, '::before').opacity` do ícone de imagem: as abas inativas desbotam para 70%.
    pub fn image_icon_opacity(&self) -> Option<f64> {
        self.image_icon.as_ref().map(|_| if self.is_active { 1.0 } else { 0.7 })
    }

    /// A imagem que o `::before` desenha no tema: `dark` é a classe `vs-dark` ou `hc-black` do ancestral.
    pub fn image_icon_for_theme(&self, dark: bool) -> Option<&URI> {
        self.image_icon.as_ref().map(|(light, dark_image)| if dark { dark_image } else { light })
    }
}

impl<T: ActionListItemData> PopupControl for RenderedPopup<T> {
    fn refresh(&self, options: Option<ITabbedActionListRefreshOptions>) {
        self.refresh_impl(options);
    }

    fn show_details(&self, options: ITabbedActionListDetailsOptions) {
        self.show_details_impl(options);
    }

    fn hide_details(&self) {
        self.go_back();
    }

    fn focus_item_action(&self, item_id: &str, action_id: &str) -> bool {
        if self.body_inert.get() {
            return false;
        }
        self.focus.set(PopupFocus::List);
        self.list().focus_item_action(item_id, action_id)
    }

    fn focus_item(&self, item_id: &str) -> bool {
        if self.body_inert.get() {
            return false;
        }
        self.focus.set(PopupFocus::List);
        let list = self.list();
        list.focus_item_by_id(item_id);
        list.widget().is_list_focused()
    }

    fn dispatch_key_down(&self, e: &UiKeyEvent, target: TabbedKeyTarget) {
        self.dispatch_key_down_impl(e, target);
    }

    fn click_tab(&self, index: usize) {
        let next = self.options.tabs.get(index).map(|tab| tab.id.clone());
        if let Some(next) = next {
            self.activate_tab(&next);
        }
    }

    fn click_tab_bar_action(&self, index: usize) {
        if let Some(tab_action) = self.options.tab_bar_actions.get(index) {
            (tab_action.run)();
        }
    }

    fn pointer_down(&self) {
        self.elements.widget.class_list_remove("keyboard-navigation");
    }

    fn footer_resized(&self) {
        if self.owner.state.details_visible.get() {
            return;
        }
        self.layout();
        self.layout_context_view();
    }

    fn details_resized(&self) {
        self.layout_context_view();
    }

    fn window_resized(&self) {
        self.layout_details();
    }

    fn set_focus(&self, focus: PopupFocus) {
        self.focus.set(focus);
    }

    fn focus(&self) -> PopupFocus {
        self.focus.get()
    }

    fn tabs(&self) -> Vec<TabElement> {
        self.tab_elements.borrow().clone()
    }

    fn tab_bar_actions(&self) -> Vec<TabBarActionElement> {
        self.action_elements.borrow().clone()
    }

    fn active_tab(&self) -> String {
        self.active_tab.borrow().clone()
    }

    fn elements(&self) -> PopupElements {
        let el = &self.elements;
        PopupElements {
            widget: el.widget.clone(),
            block: el.block.clone(),
            main: el.main.clone(),
            body: el.body.clone(),
            body_content: el.body_content.clone(),
            tab_bar: el.tab_bar.clone(),
            tab_strip: el.tab_strip.clone(),
            filter_slot: el.filter_slot.clone(),
            footer: self.footer.borrow().clone(),
            empty_body: self.empty_body.borrow().clone(),
            list: self.list().dom_node(),
        }
    }

    fn body_state(&self) -> BodyState {
        BodyState {
            inert: self.body_inert.get(),
            collapsed: self.elements.body.class_list_contains("collapsed"),
            animating: self.elements.body.class_list_contains("animating"),
            main_inert: self.main_inert.get(),
            main_aria_hidden: self.main_aria_hidden.get(),
        }
    }

    fn body_animation(&self) -> Option<BodyAnimation> {
        self.body_animation.get()
    }

    fn set_body_animation_time(&self, time: f64, paused: bool) {
        if let Some(mut animation) = self.body_animation.get() {
            animation.current_time = time;
            animation.paused = paused;
            self.body_animation.set(Some(animation));
        }
    }

    fn finish_body_animation(&self) {
        if self.body_animation.get().is_some() {
            self.finish_body_animation_event();
        }
    }

    fn finish_tab_animations(&self, index: usize) {
        if let Some(tab) = self.tab_elements.borrow_mut().get_mut(index) {
            for animation in &mut tab.animations {
                animation.finished = true;
            }
        }
        // `animation.finished.then(() => store.dispose())` roda numa volta posterior do laço de eventos.
        let hold = self.label_holds.borrow_mut().remove(&index);
        if let Some(hold) = hold {
            queue_microtask(move || hold.dispose());
        }
    }

    fn details_state(&self) -> Option<DetailsState> {
        let details = self.details.borrow();
        let page = details.as_ref()?;
        Some(DetailsState {
            label: page.options.label.clone(),
            page: page.page.clone(),
            header: page.header.clone(),
            content: page.content.clone(),
            back: page.back.clone(),
            viewport_height: page.viewport_height.get(),
            scroll_top: page.scroll_top.get(),
            header_height: Self::details_header_height(page),
        })
    }

    fn details_scroll(&self, direction: f64) {
        self.scroll_details(direction);
    }

    fn list_height(&self) -> f64 {
        parse_px(self.list().dom_node().style_get_property("height"))
    }

    fn list_visible_labels(&self) -> Vec<Option<String>> {
        self.list().widget().visible_labels()
    }

    fn list_set_hover_enabled_state(&self) -> bool {
        !self.body_inert.get()
    }

    fn list_toolbar_focus(&self) -> Option<(usize, String)> {
        self.list().widget().focused_toolbar_button()
    }

    fn list_has_focus(&self) -> bool {
        self.list().widget().is_list_focused()
    }

    fn list_filter_focused(&self) -> bool {
        self.list().filter_input().is_some_and(|input| input.is_focused())
    }

    fn list_panel_state(&self) -> (bool, bool) {
        let list = self.list();
        (list.widget().panel_displayed(), list.widget().panel_contains_focus())
    }

    fn list_focused_label(&self) -> Option<String> {
        self.list().get_focused_element().and_then(|element| element.label.clone())
    }

    fn list_filter_input(&self, value: &str) {
        self.list().widget().filter_input_input(value);
    }

    fn list_filter_composition(&self, start: bool) {
        let list = self.list();
        if start {
            list.widget().filter_input_composition_start();
        } else {
            list.widget().filter_input_composition_end();
        }
    }
}
