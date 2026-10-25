//! Porte de `src/vs/platform/quickinput/common/quickInput.ts`: as interfaces dos itens, das opções, dos botões,
//! dos controladores (`IQuickInput`, `IQuickPick`, `IInputBox`, `IQuickTree`) e o `IQuickInputService`.
//!
//! Divergências do porte completo (as do começo do arquivo, abaixo, continuam valendo):
//! - **Sem genéricos nos itens.** `T extends IQuickPickItem` (e `T extends IQuickTreeItem`) não existe: um método
//!   genérico de um trait não é objeto de trait, e os controladores vivem atrás de `Rc<dyn ...>`. O item é sempre o
//!   [`IQuickPickItem`] (o [`IQuickTreeItem`] na árvore), e os campos do tipo derivado (o `handle` do
//!   `TransferQuickPickItem`, o `value` do `PickStringItem`) vão em `extra`, um objeto dinâmico. As sobrecargas de
//!   `pick` (`canPickMany: true` devolve `T[]`) são [`IQuickInputService::pick`] e [`IQuickInputService::pick_many`].
//! - **Propriedades** (`title: string | undefined`) são um par `title()` e `set_title(...)`, com `Cell`/`RefCell` na
//!   implementação. `readonly onX: Event<T>` é `on_x() -> Event<T>`.
//! - **`IQuickInputService.pick`** ganhou o `token` do original e as `picks` como [`QuickPicks`] (lista ou promessa de
//!   lista). A forma antiga do porte parcial (lista direta, sem `token`) ficou como [`IQuickInputService::pick_items`],
//!   e o `input` ganhou o `token`.
//! - **`anchor`** (`HTMLElement | { x, y }`) é só o `{ x, y }` ([`QuickInputAnchor`]): não há DOM.
//! - Fora do porte: `IQuickWidget` e `createQuickWidget` (o `widget` é um `HTMLElement`) e o setter de `currentQuickInput`.
//! - `quickAccess` é o método [`IQuickInputService::quick_access`], que lança `quickAccess is not defined` por padrão (o objeto
//!   parcial do original, sem a propriedade); o serviço completo o sobrescreve com o `QuickAccessController`.
//! - `withQuickInputAnchor<T>` apaga o `T` num [`Dynamic`] (método genérico não é objeto de trait).
//! - `IQuickInputButtonWithToggle` não é portado (nada o usa); `QuickInputType` é enum com [`QuickInputType::as_str`].
//!
//! Divergências:
//! - Campo opcional (`x?: T`) é `Option<T>`. As interfaces são structs com `Default` (tudo ausente), para o
//!   literal `{ label: 'a' }` do original virar `IQuickItem { label: "a".into(), ..Default::default() }`.
//! - `label` e `description` são [`JsString`]: o `QuickInputTreeSorter` compara com `<` e `>` de JavaScript,
//!   que ordena por unidade UTF-16, diferente da ordem de `str` do Rust (por byte UTF-8) para os caracteres
//!   fora do plano básico.
//! - `toggle?: { checked: boolean }` é um objeto compartilhado e mutável que o chamador mantém e o botão
//!   escreve ao ser acionado: [`QuickInputButtonToggle`] atrás de `Rc`, com `checked` em `Cell`.
//!   `IQuickInputButtonWithToggle` (a mesma interface com `toggle` obrigatório) não é portada: nada do que
//!   é portado a usa.
//! - O botão é compartilhado por identidade (o `onTrigger` recebe o mesmo objeto da lista): [`QuickInputButtonRef`].
//! - `checked?: boolean | 'mixed'` do `IQuickTreeItem` é [`QuickTreeItemChecked`].
//! - `IQuickTreeItem extends IQuickItem` é composição: o campo `base` guarda o `IQuickItem`.

use std::cell::Cell;
use std::rc::Rc;

use je_base::JsString;
use je_base::cancellation::CancellationToken;
use je_base::errors::{self, Error};
use je_base::event::Event;
use je_base::filters::Match;
use je_base::fuzzy_scorer::ItemAccessor;
use je_base::html_content::IMarkdownString;
use je_base::keybindings::ResolvedKeybinding;
use je_base::lifecycle::IDisposable;
use je_base::network::schemas;
use je_base::observable::base::IObservable;
use je_base::severity::Severity;
use je_base::themables::ThemeColor;
use je_base::uri::URI;

use super::quick_access::IQuickAccessController;

/// `IQuickItemHighlights`.
#[derive(Clone, Debug, Default)]
pub struct IQuickItemHighlights {
    pub label: Option<Vec<Match>>,
    pub description: Option<Vec<Match>>,
}

/// O `{ dark: URI; light?: URI }` de `iconPath`.
#[derive(Clone, Debug)]
pub struct QuickInputIconPath {
    pub dark: URI,
    pub light: Option<URI>,
}

/// Base properties for a quick pick and quick tree item.
#[derive(Clone, Debug, Default)]
pub struct IQuickItem {
    pub id: Option<String>,
    pub label: JsString,
    pub aria_label: Option<String>,
    pub description: Option<JsString>,
    /// Whether the item is displayed in italics.
    pub italic: Option<bool>,
    /// Whether the item is displayed with a strikethrough.
    pub strikethrough: Option<bool>,
    /// Icon classes to be passed on as `IIconLabelValueOptions`
    /// to the underlying `IconLabel` widget.
    pub icon_classes: Option<Vec<String>>,
    pub icon_path: Option<QuickInputIconPath>,
    /// Icon class to be assigned to the quick item container
    /// directly.
    pub icon_class: Option<String>,
    /// Theme color applied to the quick item's icon.
    pub icon_color: Option<ThemeColor>,
    pub highlights: Option<IQuickItemHighlights>,
    pub buttons: Option<Vec<QuickInputButtonRef>>,
    /// Used when we're in multi-select mode. Renders a disabled checkbox.
    pub disabled: Option<bool>,
}

/// `QuickInputButtonLocation`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum QuickInputButtonLocation {
    /// In the title bar.
    Title = 1,
    /// To the right of the input box.
    Inline = 2,
    /// At the far end inside the input box.
    /// Used by the public API to create toggles.
    Input = 3,
}

/// O `{ checked: boolean }` de `IQuickInputButton.toggle`.
#[derive(Debug, Default)]
pub struct QuickInputButtonToggle {
    pub checked: Cell<bool>,
}

/// Represents a button in the quick input UI.
#[derive(Clone, Debug, Default)]
pub struct IQuickInputButton {
    /// The path to the icon for the button.
    /// Either `iconPath` or `iconClass` is required.
    pub icon_path: Option<QuickInputIconPath>,
    /// The CSS class for the icon of the button.
    /// Either `iconPath` or `iconClass` is required.
    pub icon_class: Option<String>,
    /// The tooltip text for the button.
    pub tooltip: Option<String>,
    /// Whether to always show the button.
    /// By default, buttons are only visible when hovering over them with the mouse.
    pub always_visible: Option<bool>,
    /// Where the button should be rendered. The default is [`QuickInputButtonLocation::Title`].
    /// Note: this property is ignored if the button was added to a QuickPickItem.
    pub location: Option<QuickInputButtonLocation>,
    /// When present, indicates that the button is a toggle button that can be checked or unchecked.
    /// The `checked` property indicates the current state of the toggle and will be updated
    /// when the button is clicked.
    pub toggle: Option<Rc<QuickInputButtonToggle>>,
    /// Optional label for the button. When used with secondary actions, this label appears in the overflow menu.
    pub label: Option<String>,
    /// When true, the button will be rendered as a secondary action in the toolbar overflow menu.
    /// By default, buttons are rendered as primary actions.
    /// Note: this does not currently apply to buttons in the Input location
    pub secondary: Option<bool>,
}

/// O botão compartilhado por identidade (`Rc::ptr_eq` é o `===` do original).
pub type QuickInputButtonRef = Rc<IQuickInputButton>;

/// O `checked?: boolean | 'mixed'` de `IQuickTreeItem`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum QuickTreeItemChecked {
    Bool(bool),
    Mixed,
}

/// Represents a tree item in the quick tree.
#[derive(Clone, Debug, Default)]
pub struct IQuickTreeItem {
    pub base: IQuickItem,
    /// The checked state of the item. Can be true, false, or 'mixed' for tri-state.
    /// When canSelectMany is false, this is ignored and the item is treated as a single selection.
    /// When canSelectMany is true, this indicates the checkbox state of the item.
    /// If undefined, the item is unchecked by default.
    pub checked: Option<QuickTreeItemChecked>,
    /// The collapsible state of the tree item. Defaults to 'Expanded' if children are present.
    pub collapsed: Option<bool>,
    /// The children of this tree item.
    pub children: Option<Vec<IQuickTreeItem>>,
    /// Defaults to true, can be false to disable picks for a single item.
    /// When false, the item is not selectable and does not respond to mouse/keyboard activation.
    pub pickable: Option<bool>,
}

// ---------------------------------------------------------------------------------------------
// Itens, separadores, opções e eventos.
// ---------------------------------------------------------------------------------------------

use je_base::dynamic::Dynamic;
use je_base::promise::Promise;

/// O `ResolvedKeybinding` compartilhado de `IQuickPickItem.keybinding` (com `Debug` pelo rótulo).
#[derive(Clone)]
pub struct ResolvedKeybindingRef(pub Rc<dyn ResolvedKeybinding>);

impl std::fmt::Debug for ResolvedKeybindingRef {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "ResolvedKeybinding({:?})", self.0.get_label())
    }
}

/// `IQuickPickItemHighlights` (`extends IQuickItemHighlights`, com `detail`).
#[derive(Clone, Debug, Default)]
pub struct IQuickPickItemHighlights {
    pub label: Option<Vec<Match>>,
    pub description: Option<Vec<Match>>,
    pub detail: Option<Vec<Match>>,
}

/// O `string | IMarkdownString` de `tooltip`.
#[derive(Clone, Debug)]
pub enum QuickPickTooltip {
    Text(String),
    Markdown(IMarkdownString),
}

/// `IQuickPickItem`, a interface de um item do quick pick, e os campos que o tipo derivado (`T extends
/// IQuickPickItem`) acrescenta, em `extra`.
///
/// O `highlights` daqui (com `detail`) tem precedência sobre o `base.highlights` do [`IQuickItem`], como o
/// `highlights?: IQuickPickItemHighlights` que o original redeclara. O `type?: 'item'` é o ramo
/// [`QuickPickInput::Item`].
#[derive(Clone, Debug)]
pub struct IQuickPickItem {
    pub base: IQuickItem,
    /// Os campos do tipo derivado, como objeto dinâmico (`{ value }` no `PickStringItem`, `{ handle }` no
    /// `TransferQuickPickItem`).
    pub extra: Dynamic,
    /// The detail text of the quick pick item. Shown as the second line.
    pub detail: Option<String>,
    /// The tooltip for the quick pick item.
    pub tooltip: Option<QuickPickTooltip>,
    pub highlights: Option<IQuickPickItemHighlights>,
    /// Allows to show a keybinding next to the item to indicate how the item can be triggered outside of the picker
    /// using keyboard shortcut.
    pub keybinding: Option<ResolvedKeybindingRef>,
    /// Whether the item is picked by default when the Quick Pick is shown.
    pub picked: Option<bool>,
    /// Whether the item is always shown in the Quick Pick regardless of filtering.
    pub always_show: Option<bool>,
    /// Defaults to true with `IQuickPick.canSelectMany`, can be false to disable picks for a single item.
    pub pickable: Option<bool>,
}

impl Default for IQuickPickItem {
    fn default() -> Self {
        Self {
            base: IQuickItem::default(),
            extra: Dynamic::Undefined,
            detail: None,
            tooltip: None,
            highlights: None,
            keybinding: None,
            picked: None,
            always_show: None,
            pickable: None,
        }
    }
}

/// `IQuickPickSeparator`.
#[derive(Clone, Debug, Default)]
pub struct IQuickPickSeparator {
    pub id: Option<String>,
    pub label: Option<String>,
    pub description: Option<String>,
    pub aria_label: Option<String>,
    pub buttons: Option<Vec<QuickInputButtonRef>>,
    pub tooltip: Option<QuickPickTooltip>,
}

/// `QuickPickInput<T>` (`T | IQuickPickSeparator`). O `type: 'item' | 'separator'` do original é a variante.
#[derive(Clone, Debug)]
pub enum QuickPickInput<T = IQuickPickItem> {
    Item(T),
    Separator(IQuickPickSeparator),
}

/// `QuickPickItem` (`IQuickPickSeparator | IQuickPickItem`).
pub type QuickPickItem = QuickPickInput<IQuickPickItem>;

impl<T> QuickPickInput<T> {
    /// `item.type === 'separator'`.
    pub fn is_separator(&self) -> bool {
        matches!(self, Self::Separator(_))
    }

    /// O item, se não for um separador.
    pub fn as_item(&self) -> Option<&T> {
        match self {
            Self::Item(item) => Some(item),
            Self::Separator(_) => None,
        }
    }

    /// O item, se não for um separador (o filtro `item.type !== 'separator'`).
    pub fn into_item(self) -> Option<T> {
        match self {
            Self::Item(item) => Some(item),
            Self::Separator(_) => None,
        }
    }
}

/// `Promise<QuickPickInput<T>[]> | QuickPickInput<T>[]`, o primeiro argumento de `pick`.
#[derive(Clone)]
pub enum QuickPicks {
    Items(Vec<QuickPickInput>),
    Promised(Promise<Vec<QuickPickInput>>),
}

impl QuickPicks {
    /// `Promise.resolve(picks)`.
    pub fn into_promise(self) -> Promise<Vec<QuickPickInput>> {
        match self {
            Self::Items(items) => Promise::resolved(items),
            Self::Promised(promise) => promise,
        }
    }
}

impl From<Vec<QuickPickInput>> for QuickPicks {
    fn from(items: Vec<QuickPickInput>) -> Self {
        Self::Items(items)
    }
}

impl From<Promise<Vec<QuickPickInput>>> for QuickPicks {
    fn from(promise: Promise<Vec<QuickPickInput>>) -> Self {
        Self::Promised(promise)
    }
}

/// `IKeyMods`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct IKeyMods {
    pub ctrl_cmd: bool,
    pub alt: bool,
    pub shift: bool,
}

/// `isKeyModified`.
pub fn is_key_modified(key_mods: &IKeyMods) -> bool {
    key_mods.ctrl_cmd || key_mods.alt || key_mods.shift
}

/// `NO_KEY_MODS`.
pub const NO_KEY_MODS: IKeyMods = IKeyMods { ctrl_cmd: false, alt: false, shift: false };

/// `IQuickNavigateConfiguration`.
#[derive(Clone, Debug, Default)]
pub struct IQuickNavigateConfiguration {
    pub keybindings: Vec<ResolvedKeybindingRef>,
}

/// O `IAnchor` de `anchor` (`HTMLElement | { x, y, width?, height? }`): um `HTMLElement` entra como o retângulo dele (`x` e `y`
/// são `left` e `top`, com `width` e `height`). `width` e `height` ausentes valem o `1` e o `2` do `getAnchorRect`.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct QuickInputAnchor {
    pub x: f64,
    pub y: f64,
    pub width: Option<f64>,
    pub height: Option<f64>,
}

impl QuickInputAnchor {
    /// Um ponto (`{ x, y }`).
    pub fn point(x: f64, y: f64) -> Self {
        Self { x, y, width: None, height: None }
    }
}

/// `'above' | 'below' | 'overlay'` (o `anchorPosition`). `IInputOptions` aceita só `Above` e `Overlay`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum QuickInputAnchorPosition {
    Above,
    Below,
    Overlay,
}

/// O que `IInputOptions.validateInput` devolve: `string | null | undefined | { content, severity }`.
#[derive(Clone, Debug, PartialEq)]
pub enum InputValidationResult {
    /// `null` ou `undefined`: a entrada é válida.
    None,
    /// Uma `string`: a mensagem de erro.
    Message(String),
    /// `{ content, severity }`.
    Content { content: String, severity: Severity },
}

/// O `activeItem?: Promise<T> | T` de `IPickOptions`.
#[derive(Clone)]
pub enum PickActiveItem {
    Item(IQuickPickItem),
    Promised(Promise<IQuickPickItem>),
}

/// `IQuickPickItemButtonEvent<T>`: o botão acionado e o item dele.
#[derive(Clone, Debug)]
pub struct IQuickPickItemButtonEvent {
    pub button: QuickInputButtonRef,
    pub item: IQuickPickItem,
}

/// `IQuickPickSeparatorButtonEvent`.
#[derive(Clone, Debug)]
pub struct IQuickPickSeparatorButtonEvent {
    pub button: QuickInputButtonRef,
    pub separator: IQuickPickSeparator,
}

/// `IQuickPickItemButtonContext<T>` (`extends IQuickPickItemButtonEvent<T>`, com `removeItem`).
#[derive(Clone)]
pub struct IQuickPickItemButtonContext {
    pub event: IQuickPickItemButtonEvent,
    /// Removes the associated item from the quick pick.
    pub remove_item: Rc<dyn Fn()>,
}

/// `IPickOptions<T>`.
#[derive(Clone, Default)]
pub struct IPickOptions {
    /// an optional string to show as the title of the quick input
    pub title: Option<String>,
    /// the value to prefill in the input box
    pub value: Option<String>,
    /// An optional string to show as placeholder in the input box to guide the user what she picks on.
    pub place_holder: Option<String>,
    /// the text to display underneath the input box
    pub prompt: Option<String>,
    /// an optional flag to include the description when filtering the picks
    pub match_on_description: Option<bool>,
    /// An optional flag to include the description when filtering the picks.
    pub match_on_detail: Option<bool>,
    /// an optional flag to filter the picks based on label. Defaults to true.
    pub match_on_label: Option<bool>,
    /// an optional flag to sort the picks based by the label.
    pub sort_by_label: Option<bool>,
    /// Set to true to keep the picker open when focus moves to another part of the editor or to another window.
    pub ignore_focus_lost: Option<bool>,
    /// an optional flag to make this picker multi-select
    pub can_pick_many: Option<bool>,
    /// enables quick navigate in the picker to open an element without typing
    pub quick_navigate: Option<IQuickNavigateConfiguration>,
    /// Hides the input box from the picker UI. This is typically used in combination with quick-navigation where no
    /// search UI should be presented.
    pub hide_input: Option<bool>,
    /// a context key to set when this picker is active
    pub context_key: Option<String>,
    /// an optional property for the item to focus initially.
    pub active_item: Option<PickActiveItem>,
    /// an optional anchor for the picker
    pub anchor: Option<QuickInputAnchor>,
    /// Placement of the quick input relative to the anchor. Defaults to `Above`.
    pub anchor_position: Option<QuickInputAnchorPosition>,
    pub on_key_mods: Option<Rc<dyn Fn(IKeyMods)>>,
    pub on_did_focus: Option<Rc<dyn Fn(&IQuickPickItem)>>,
    pub on_did_trigger_item_button: Option<Rc<dyn Fn(IQuickPickItemButtonContext)>>,
    pub on_did_trigger_separator_button: Option<Rc<dyn Fn(IQuickPickSeparatorButtonEvent)>>,
}

impl std::fmt::Debug for IPickOptions {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("IPickOptions").field("title", &self.title).field("place_holder", &self.place_holder).finish_non_exhaustive()
    }
}

/// `IInputOptions`.
#[derive(Clone, Default)]
pub struct IInputOptions {
    /// an optional string to show as the title of the quick input
    pub title: Option<String>,
    /// The text to display underneath the input box.
    pub prompt: Option<String>,
    /// The value to prefill in the input box.
    pub value: Option<String>,
    /// the selection of value, default to the whole prefilled value
    pub value_selection: Option<(u32, u32)>,
    /// an optional string to show as placeholder in the input box to guide the user what to type
    pub place_holder: Option<String>,
    /// Set to true to show a password prompt that will not show the typed value.
    pub password: Option<bool>,
    /// Set to true to keep the input box open when focus moves to another part of the editor or to another window.
    pub ignore_focus_lost: Option<bool>,
    /// an optional function that is used to validate user input.
    pub validate_input: Option<Rc<dyn Fn(&str) -> Promise<InputValidationResult>>>,
    /// an optional anchor for the input.
    pub anchor: Option<QuickInputAnchor>,
    /// Placement of the quick input relative to the anchor (`Above` or `Overlay`). Defaults to `Above`.
    pub anchor_position: Option<QuickInputAnchorPosition>,
}

impl std::fmt::Debug for IInputOptions {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("IInputOptions").field("title", &self.title).field("prompt", &self.prompt).field("value", &self.value).finish_non_exhaustive()
    }
}

/// `QuickInputHideReason`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum QuickInputHideReason {
    /// Focus moved away from the quick input.
    Blur = 1,
    /// An explicit user gesture, e.g. pressing Escape key.
    Gesture = 2,
    /// Anything else.
    Other = 3,
}

/// `IQuickInputHideEvent`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct IQuickInputHideEvent {
    pub reason: QuickInputHideReason,
}

/// `QuickInputType`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum QuickInputType {
    QuickPick,
    InputBox,
    QuickWidget,
    QuickTree,
}

impl QuickInputType {
    /// O valor do `const enum` do original.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::QuickPick => "quickPick",
            Self::InputBox => "inputBox",
            Self::QuickWidget => "quickWidget",
            Self::QuickTree => "quickTree",
        }
    }
}

/// `IQuickPickWillAcceptEvent`: `veto()` desliga o tratamento padrão do aceite.
#[derive(Clone, Debug, Default)]
pub struct IQuickPickWillAcceptEvent {
    vetoed: Rc<Cell<bool>>,
}

impl IQuickPickWillAcceptEvent {
    pub fn new() -> Self {
        Self::default()
    }

    /// Allows to disable the default accept handling of the picker. If `veto` is called, the picker will not trigger
    /// the `onDidAccept` event.
    pub fn veto(&self) {
        self.vetoed.set(true);
    }

    /// Se algum ouvinte chamou `veto`.
    pub fn is_vetoed(&self) -> bool {
        self.vetoed.get()
    }
}

/// `IQuickPickDidAcceptEvent`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct IQuickPickDidAcceptEvent {
    /// Signals if the picker item is to be accepted in the background while keeping the picker open.
    pub in_background: bool,
}

/// `ItemActivation`: which item will be "active" (aka focused).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ItemActivation {
    /// No item will be active.
    None,
    /// First item will be active.
    First,
    /// Second item will be active.
    Second,
    /// Last item will be active.
    Last,
}

/// `QuickPickFocus`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum QuickPickFocus {
    First = 1,
    Second = 2,
    Last = 3,
    Next = 4,
    Previous = 5,
    NextPage = 6,
    PreviousPage = 7,
    NextSeparator = 8,
    PreviousSeparator = 9,
}

/// `'fuzzy' | 'contiguous'` (`matchOnLabelMode`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MatchOnLabelMode {
    Fuzzy,
    Contiguous,
}

/// O `ok: boolean | 'default'` do quick pick.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum QuickPickOk {
    Bool(bool),
    Default,
}

/// O `{ useSeparators: boolean }` de `createQuickPick`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct QuickPickCreateOptions {
    pub use_separators: bool,
}

/// `QuickInputAlignment`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum QuickInputAlignment {
    Top,
    Center,
    Custom,
}

/// O argumento de `setAlignment`: `'top' | 'center' | { top: number; left: number }`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum QuickInputAlignmentSetting {
    Top,
    Center,
    Custom { top: f64, left: f64 },
}

// ---------------------------------------------------------------------------------------------
// Controladores: `IQuickInput`, `IQuickPick`, `IInputBox` e `IQuickTree`.
// ---------------------------------------------------------------------------------------------

/// Declara o par `nome()` e `set_nome(valor)` de uma propriedade do original.
macro_rules! quick_input_properties {
    ($(
        $(#[$doc:meta])*
        $get:ident, $set:ident: $ty:ty;
    )*) => {
        $(
            $(#[$doc])*
            fn $get(&self) -> $ty;
            fn $set(&self, value: $ty);
        )*
    };
}

/// `IQuickInput`.
pub trait IQuickInput: IDisposable {
    /// The type of the quick input.
    fn quick_input_type(&self) -> QuickInputType;
    /// An event that is fired when the quick input is hidden.
    fn on_did_hide(&self) -> Event<IQuickInputHideEvent>;
    /// An event that is fired when the quick input will be hidden.
    fn on_will_hide(&self) -> Event<IQuickInputHideEvent>;
    /// An event that is fired when the quick input is disposed.
    fn on_dispose(&self) -> Event<()>;
    /// An event that is fired when a button in the quick input is triggered.
    fn on_did_trigger_button(&self) -> Event<QuickInputButtonRef>;

    quick_input_properties! {
        /// The title of the quick input.
        title, set_title: Option<String>;
        /// The description of the quick input. This is rendered right below the input box.
        description, set_description: Option<String>;
        /// The current step of the quick input rendered in the titlebar.
        step, set_step: Option<u32>;
        /// The total number of steps in the quick input rendered in the titlebar.
        total_steps, set_total_steps: Option<u32>;
        /// The buttons displayed in the quick input titlebar.
        buttons, set_buttons: Vec<QuickInputButtonRef>;
        /// Indicates whether the input is enabled.
        enabled, set_enabled: bool;
        /// The context key associated with the quick input.
        context_key, set_context_key: Option<String>;
        /// Indicates whether the quick input is busy. Renders a progress bar if true.
        busy, set_busy: bool;
        /// Indicates whether the quick input should be hidden when it loses focus.
        ignore_focus_out, set_ignore_focus_out: bool;
        /// An optional anchor for the quick input.
        anchor, set_anchor: Option<QuickInputAnchor>;
        /// Placement of the quick input relative to the anchor. Defaults to `Above`.
        anchor_position, set_anchor_position: Option<QuickInputAnchorPosition>;
    }

    /// Shows the quick input.
    fn show(&self);
    /// Hides the quick input.
    fn hide(&self);
    /// Notifies that the quick input has been hidden.
    fn did_hide(&self, reason: Option<QuickInputHideReason>);
    /// Notifies that the quick input will be hidden.
    fn will_hide(&self, reason: Option<QuickInputHideReason>);
}

/// `IQuickPick<T, O>`: o `O.useSeparators` só tipa `items`, então a lista aceita separadores sempre.
pub trait IQuickPick: IQuickInput {
    /// An event that is fired when the value of the quick pick input changes.
    fn on_did_change_value(&self) -> Event<String>;
    /// An event that is fired when the quick pick is about to accept the selected item.
    fn on_will_accept(&self) -> Event<IQuickPickWillAcceptEvent>;
    /// An event that is fired when the quick pick has accepted the selected item.
    fn on_did_accept(&self) -> Event<IQuickPickDidAcceptEvent>;
    /// An event that is fired when the custom button is triggered. The custom button is a button with text rendered
    /// to the right of the input.
    fn on_did_custom(&self) -> Event<()>;
    /// An event that is fired when an item button is triggered.
    fn on_did_trigger_item_button(&self) -> Event<IQuickPickItemButtonEvent>;
    /// An event that is fired when a separator button is triggered.
    fn on_did_trigger_separator_button(&self) -> Event<IQuickPickSeparatorButtonEvent>;
    /// An event that is fired when the active items change.
    fn on_did_change_active(&self) -> Event<Vec<IQuickPickItem>>;
    /// An event that is fired when the selected items change.
    fn on_did_change_selection(&self) -> Event<Vec<IQuickPickItem>>;
    /// The key modifiers.
    fn key_mods(&self) -> IKeyMods;

    quick_input_properties! {
        /// The current value of the quick pick input.
        value, set_value: String;
        /// A method that allows to massage the value used for filtering, e.g, to remove certain parts.
        filter_value, set_filter_value: Rc<dyn Fn(&str) -> String>;
        /// The ARIA label for the quick pick input.
        aria_label, set_aria_label: Option<String>;
        /// The placeholder text for the quick pick input.
        placeholder, set_placeholder: Option<String>;
        /// Text shown below the quick pick input.
        prompt, set_prompt: Option<String>;
        /// If enabled, the `onDidAccept` event will be fired when pressing the arrow-right key to accept the selected
        /// item without closing the picker.
        can_accept_in_background, set_can_accept_in_background: bool;
        /// The OK button state. It can be a boolean value or the string 'default'.
        ok, set_ok: QuickPickOk;
        /// The OK button label.
        ok_label, set_ok_label: Option<String>;
        /// Whether to show the custom button.
        custom_button, set_custom_button: bool;
        /// The label for the custom button.
        custom_label, set_custom_label: Option<String>;
        /// The hover text for the custom button.
        custom_hover, set_custom_hover: Option<String>;
        /// Whether the custom button should be rendered as a secondary button.
        custom_button_secondary, set_custom_button_secondary: Option<bool>;
        /// The items to be displayed in the quick pick.
        items, set_items: Vec<QuickPickInput>;
        /// Whether multiple items can be selected. If so, checkboxes will be rendered.
        can_select_many, set_can_select_many: bool;
        /// Whether to match on the description of the items.
        match_on_description, set_match_on_description: bool;
        /// Whether to match on the detail of the items.
        match_on_detail, set_match_on_detail: bool;
        /// Whether to match on the label of the items.
        match_on_label, set_match_on_label: bool;
        /// The mode to filter the label with. Defaults to fuzzy.
        match_on_label_mode, set_match_on_label_mode: MatchOnLabelMode;
        /// Whether to sort the items by label.
        sort_by_label, set_sort_by_label: bool;
        /// Whether to keep the scroll position when the quick pick input is updated.
        keep_scroll_position, set_keep_scroll_position: bool;
        /// The configuration for quick navigation.
        quick_navigate, set_quick_navigate: Option<IQuickNavigateConfiguration>;
        /// The currently active items.
        active_items, set_active_items: Vec<IQuickPickItem>;
        /// The item activation behavior for the next time `items` is set.
        item_activation, set_item_activation: ItemActivation;
        /// The currently selected items.
        selected_items, set_selected_items: Vec<IQuickPickItem>;
        /// The selection range for the value in the input.
        value_selection, set_value_selection: Option<(u32, u32)>;
        /// The validation message for the quick pick. This is rendered below the input.
        validation_message, set_validation_message: Option<String>;
        /// The severity of the validation message.
        severity, set_severity: Severity;
        /// Hides the input box from the picker UI.
        hide_input, set_hide_input: bool;
        /// Controls whether the count for the items should be shown.
        hide_count_badge, set_hide_count_badge: bool;
        /// Whether to hide the "Check All" checkbox.
        hide_check_all, set_hide_check_all: bool;
    }

    /// Checks if the quick pick input has focus.
    fn input_has_focus(&self) -> bool;
    /// Focuses on the quick pick input.
    fn focus_on_input(&self);
    /// Focus a particular item in the list. Used internally for keyboard navigation.
    fn focus(&self, focus: QuickPickFocus);
    /// Programmatically accepts an item. Used internally for keyboard navigation.
    fn accept(&self, in_background: Option<bool>);
}

/// `IInputBox`.
pub trait IInputBox: IQuickInput {
    /// Event called when the input value changes.
    fn on_did_change_value(&self) -> Event<String>;
    /// Event called when the user submits the input.
    fn on_did_accept(&self) -> Event<()>;

    quick_input_properties! {
        /// Value shown in the input box.
        value, set_value: String;
        /// Provide start and end values to be selected in the input box.
        value_selection, set_value_selection: Option<(u32, u32)>;
        /// Value shown as example for input.
        placeholder, set_placeholder: Option<String>;
        /// Determines if the input value should be hidden while typing.
        password, set_password: bool;
        /// Text show below the input box.
        prompt, set_prompt: Option<String>;
        /// An optional validation message indicating a problem with the current input value.
        validation_message, set_validation_message: Option<String>;
        /// Severity of the input validation message.
        severity, set_severity: Severity;
    }

    /// Programmatically accepts an item. Used internally for keyboard navigation.
    fn accept(&self);
}

/// `IQuickTreeCheckboxEvent<T>`.
#[derive(Clone, Debug)]
pub struct IQuickTreeCheckboxEvent {
    pub item: IQuickTreeItem,
    pub checked: QuickTreeItemChecked,
}

/// `IQuickTreeItemButtonEvent<T>`.
#[derive(Clone, Debug)]
pub struct IQuickTreeItemButtonEvent {
    pub button: QuickInputButtonRef,
    pub item: IQuickTreeItem,
}

/// `IQuickTree<T>`. Os itens são passados por valor; o controlador os identifica pelo `base.id` (o `Rc` do original
/// compara por referência).
pub trait IQuickTree: IQuickInput {
    /// An event that is fired when the filter value changes.
    fn on_did_change_value(&self) -> Event<String>;
    /// An event that is fired when the quick tree has accepted the selected items.
    fn on_did_accept(&self) -> Event<()>;
    /// An event that is fired when the active items change.
    fn on_did_change_active(&self) -> Event<Vec<IQuickTreeItem>>;
    /// An event that is fired when the selected items change.
    fn on_did_change_checked_leaf_items(&self) -> Event<Vec<IQuickTreeItem>>;
    /// An event that is fired when the checkbox state of an item changes.
    fn on_did_change_checkbox_state(&self) -> Event<IQuickTreeItem>;
    /// An event that is fired when an item button is triggered.
    fn on_did_trigger_item_button(&self) -> Event<IQuickTreeItemButtonEvent>;

    quick_input_properties! {
        /// The current value of the quick tree filter input.
        value, set_value: String;
        /// The ARIA label for the quick tree input.
        aria_label, set_aria_label: Option<String>;
        /// The placeholder text for the quick tree filter input.
        placeholder, set_placeholder: Option<String>;
        /// Whether to match on the description of the items.
        match_on_description, set_match_on_description: bool;
        /// Whether to match on the label of the items.
        match_on_label, set_match_on_label: bool;
        /// Whether to sort the items by label. Defaults to true.
        sort_by_label, set_sort_by_label: bool;
        /// The currently active items.
        active_items, set_active_items: Vec<IQuickTreeItem>;
        /// The validation message for the quick pick. This is rendered below the input.
        validation_message, set_validation_message: Option<String>;
        /// The severity of the validation message.
        severity, set_severity: Severity;
    }

    /// The items currently displayed in the quick tree. Modifications to the returned list do not cause updates.
    fn item_tree(&self) -> Vec<IQuickTreeItem>;
    /// The currently selected leaf items.
    fn checked_leaf_items(&self) -> Vec<IQuickTreeItem>;
    /// Get the parent element of the element passed in.
    fn get_parent(&self, element: &IQuickTreeItem) -> Option<IQuickTreeItem>;
    /// Sets the items to be displayed in the quick tree.
    fn set_item_tree(&self, item_tree: Vec<IQuickTreeItem>);
    /// Expands an item.
    fn expand(&self, element: &IQuickTreeItem);
    /// Collapses an item.
    fn collapse(&self, element: &IQuickTreeItem);
    /// Checks if an item is collapsed.
    fn is_collapsed(&self, element: &IQuickTreeItem) -> bool;
    /// Focuses on the tree input.
    fn focus_on_input(&self);
    /// Reveals and focuses a specific item in the tree.
    fn reveal(&self, element: &IQuickTreeItem);
    /// Focus a particular item in the list. Used internally for keyboard navigation.
    fn focus(&self, focus: QuickPickFocus);
    /// Programmatically accepts an item. Used internally for keyboard navigation.
    fn accept(&self, in_background: Option<bool>);
}

/// Um controlador qualquer (`IQuickInput`) com o tipo concreto à mão: o `currentQuickInput` e a sessão do
/// `MainThreadQuickOpen` precisam do `instanceof` que o original faz por `type`.
#[derive(Clone)]
pub enum QuickInputRef {
    QuickPick(Rc<dyn IQuickPick>),
    InputBox(Rc<dyn IInputBox>),
    QuickTree(Rc<dyn IQuickTree>),
}

// ---------------------------------------------------------------------------------------------
// Suporte ao fuzzy scorer.
// ---------------------------------------------------------------------------------------------

/// `IQuickPickItemWithResource` (`IQuickPickItem & { resource?: URI }`).
#[derive(Clone, Debug, Default)]
pub struct IQuickPickItemWithResource {
    pub item: IQuickPickItem,
    pub resource: Option<URI>,
}

/// `QuickPickItemScorerAccessor`.
#[derive(Clone, Copy, Debug, Default)]
pub struct QuickPickItemScorerAccessor {
    pub skip_description: bool,
    pub skip_path: bool,
}

impl QuickPickItemScorerAccessor {
    /// `new QuickPickItemScorerAccessor(options)`.
    pub fn new(skip_description: bool, skip_path: bool) -> Self {
        Self { skip_description, skip_path }
    }
}

impl ItemAccessor<IQuickPickItemWithResource> for QuickPickItemScorerAccessor {
    fn get_item_label(&self, entry: &IQuickPickItemWithResource) -> Option<JsString> {
        Some(entry.item.base.label.clone())
    }

    fn get_item_description(&self, entry: &IQuickPickItemWithResource) -> Option<JsString> {
        if self.skip_description {
            return None;
        }
        entry.item.base.description.clone()
    }

    fn get_item_path(&self, entry: &IQuickPickItemWithResource) -> Option<JsString> {
        if self.skip_path {
            return None;
        }
        let resource = entry.resource.as_ref()?;
        if resource.scheme().to_string_lossy() == schemas::FILE {
            return Some(resource.fs_path(crate::resources::host_platform()));
        }
        Some(resource.path().clone())
    }
}

/// `quickPickItemScorerAccessor` (o acessor sem opções).
pub fn quick_pick_item_scorer_accessor() -> QuickPickItemScorerAccessor {
    QuickPickItemScorerAccessor::default()
}

// ---------------------------------------------------------------------------------------------
// O serviço.
// ---------------------------------------------------------------------------------------------

/// `IQuickInputService`.
pub trait IQuickInputService {
    /// Provides access to the back button in quick input.
    fn back_button(&self) -> QuickInputButtonRef;

    /// Allows to register on the event that quick input is showing.
    fn on_show(&self) -> Event<()>;

    /// Allows to register on the event that quick input is hiding.
    fn on_hide(&self) -> Event<()>;

    /// The current alignment of the quick input widget.
    fn alignment(&self) -> IObservable<QuickInputAlignment>;

    /// Runs an operation so that the next quick pick it opens inherits the provided anchor.
    fn with_quick_input_anchor(
        &self,
        anchor: Option<QuickInputAnchor>,
        anchor_position: Option<QuickInputAnchorPosition>,
        operation: Box<dyn FnOnce() -> Promise<Dynamic>>,
    ) -> Promise<Dynamic>;

    /// Opens the quick input box for selecting items and returns a promise with the user selected item if any
    /// (`canPickMany` falso ou ausente).
    fn pick(&self, picks: QuickPicks, options: Option<&IPickOptions>, token: Option<&CancellationToken>) -> Promise<Option<IQuickPickItem>>;

    /// A sobrecarga de `pick` com `canPickMany: true`: a promessa traz os itens escolhidos, se houver.
    fn pick_many(&self, picks: QuickPicks, options: Option<&IPickOptions>, token: Option<&CancellationToken>) -> Promise<Option<Vec<IQuickPickItem>>>;

    /// A forma do porte parcial: lista direta e sem `token`. É o [`pick`](Self::pick) com [`QuickPicks::Items`].
    fn pick_items(&self, picks: Vec<IQuickPickItem>, options: Option<&IPickOptions>) -> Promise<Option<IQuickPickItem>> {
        self.pick(QuickPicks::Items(picks.into_iter().map(QuickPickInput::Item).collect()), options, None)
    }

    /// Opens the quick input box for text input and returns a promise with the user typed value if it was confirmed
    /// or `None` if it was canceled.
    fn input(&self, options: Option<&IInputOptions>, token: Option<&CancellationToken>) -> Promise<Option<String>>;

    /// Provides raw access to the quick pick controller.
    fn create_quick_pick(&self, options: Option<QuickPickCreateOptions>) -> Rc<dyn IQuickPick>;

    /// Provides raw access to the input box controller.
    fn create_input_box(&self) -> Rc<dyn IInputBox>;

    /// Provides raw access to the quick tree controller.
    fn create_quick_tree(&self) -> Rc<dyn IQuickTree>;

    /// Moves focus into quick input.
    fn focus(&self);

    /// Toggle the checked state of the selected item.
    fn toggle(&self);

    /// Navigate inside the opened quick input list.
    fn navigate(&self, next: bool, quick_navigate: Option<&IQuickNavigateConfiguration>);

    /// Navigate back in a multi-step quick input.
    fn back(&self) -> Promise<()>;

    /// Accept the selected item. `key_mods` allows to override the state of key modifiers that should be present
    /// when invoking.
    fn accept(&self, key_mods: Option<IKeyMods>) -> Promise<()>;

    /// Cancels quick input and closes it.
    fn cancel(&self, reason: Option<QuickInputHideReason>) -> Promise<()>;

    /// Toggles hover for the current quick input item.
    fn toggle_hover(&self);

    /// The current quick pick that is visible. `None` if none is open.
    fn current_quick_input(&self) -> Option<QuickInputRef>;

    /// Set the alignment of the quick input.
    fn set_alignment(&self, alignment: QuickInputAlignmentSetting);

    /// Provides access to the quick access providers.
    fn quick_access(&self) -> Rc<dyn IQuickAccessController> {
        errors::throw(Error::new("quickAccess is not defined"))
    }
}

je_instantiation::service_id!(pub I_QUICK_INPUT_SERVICE: dyn IQuickInputService = "quickInputService");
