//! Porte de `src/vs/platform/quickinput/common/quickInput.ts`: só os tipos que `quickInputUtils` e o
//! `QuickInputTreeSorter` usam (`IQuickItemHighlights`, `IQuickItem`, `QuickInputButtonLocation`,
//! `IQuickInputButton` e `IQuickTreeItem`).
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
//! - Fora do porte, por não serem usados aqui: `IQuickPickItem` e as demais interfaces do arquivo.

use std::cell::Cell;
use std::rc::Rc;

use je_base::JsString;
use je_base::filters::Match;
use je_base::themables::ThemeColor;
use je_base::uri::URI;

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
// `IInputOptions`, `IPickOptions`, `IQuickPickItem` e `IQuickInputService`, reduzidos ao que os serviços do workbench
// que pedem entrada ao usuário leem (o `configurationResolverService`).
//
// Divergências:
// - `IQuickInputService` traz só `input` e `pick`; `createQuickPick`, `createInputBox`, `focus`, `navigate`... entram com a
//   UI do quick input.
// - `pick<T extends IQuickPickItem>` é genérico no original e o retorno é o próprio item `T`. Aqui o item é
//   [`IQuickPickItem`], com o [`IQuickItem`] em `base` e, em `extra`, os campos que o tipo derivado acrescenta (o
//   `PickStringItem.value`, por exemplo) como objeto dinâmico. `canPickMany` e o `CancellationToken` ficam de fora: nenhum
//   chamador portado os usa.
// - Em `IInputOptions` e `IPickOptions` entram só os campos lidos pelos chamadores portados.
// ---------------------------------------------------------------------------------------------

use je_base::dynamic::Dynamic;
use je_base::promise::Promise;

/// `IInputOptions`.
#[derive(Clone, Debug, Default)]
pub struct IInputOptions {
    /// The text to display underneath the input box.
    pub prompt: Option<String>,
    /// The value to prefill in the input box.
    pub value: Option<String>,
    /// Set to true to show a password prompt that will not show the typed value.
    pub password: Option<bool>,
    /// Set to true to keep the input box open when focus moves to another part of the editor or to another window.
    pub ignore_focus_lost: Option<bool>,
}

/// `IPickOptions`.
#[derive(Clone, Debug, Default)]
pub struct IPickOptions {
    /// An optional string to show as placeholder in the input box to guide the user what she picks on.
    pub place_holder: Option<String>,
    /// An optional flag to include the description when filtering the picks.
    pub match_on_detail: Option<bool>,
    /// Set to true to keep the picker open when focus moves to another part of the editor or to another window.
    pub ignore_focus_lost: Option<bool>,
}

/// `IQuickPickItem` e os campos que o tipo derivado (`T extends IQuickPickItem`) acrescenta, em `extra`.
#[derive(Clone, Debug)]
pub struct IQuickPickItem {
    pub base: IQuickItem,
    /// Os campos do tipo derivado, como objeto dinâmico (`{ value }` no `PickStringItem`).
    pub extra: Dynamic,
}

/// `IQuickInputService`.
pub trait IQuickInputService {
    /// Opens the quick input box for text input and returns a promise with the user typed value if it was confirmed or
    /// `None` if it was canceled.
    fn input(&self, options: Option<&IInputOptions>) -> Promise<Option<String>>;

    /// Opens the quick input box for selecting items and returns a promise with the user selected item or `None` if
    /// canceled.
    fn pick(&self, picks: Vec<IQuickPickItem>, options: Option<&IPickOptions>) -> Promise<Option<IQuickPickItem>>;
}

je_instantiation::service_id!(pub I_QUICK_INPUT_SERVICE: dyn IQuickInputService = "quickInputService");
