//! Porte de `src/vs/base/browser/ui/dialog/dialog.ts` e `dialog.css`: o [`Dialog`], a caixa modal com
//! ícone por tipo, mensagem, detalhe, campos de texto e de senha, caixa de seleção, botões, rodapé e
//! corpo personalizados, o botão de fechar e a navegação por teclado; mais [`DialogStyles`] (o
//! `IDialogStyles`), [`default_dialog_styles`] e [`unthemed_dialog_styles`].
//!
//! Do original ficam: as opções (`cancelId`, `detail`, `detailElement`, `alignment`, `checkboxLabel`,
//! `checkboxChecked`, `type`, `extraClasses`, `modalBlockExtraClasses`, `inputs`, `keyEventProcessor`,
//! `renderBody`, `renderFooter`, `icon`, `buttonOptions` (com `sublabel` e `styleButton`),
//! `primaryButtonDropdown`, `disableCloseAction`, `disableCloseButton`, `disableDefaultAction`,
//! `isExternalFocusAllowed`, `onVisibilityChange`), `show()` (devolve o [`DialogResult`] numa `Promise` do
//! `je-base`), `updateMessage`, o arranjo dos botões por sistema (`rearrangeButtons`: Windows põe o botão de
//! cancelar por último; Linux e macOS põem o de cancelar em segundo e invertem a ordem), o Enter dentro de um
//! campo de texto (confirma), o Cmd+D do macOS (o botão "não"), o Escape no `keyup` (só se o `keydown`
//! também foi visto), Tab, Shift+Tab e as setas entre links, campos, caixa de seleção, botões e links do
//! rodapé (dando a volta), a armadilha de foco (o foco que escapa da caixa volta para dentro) e o retorno do
//! foco ao fechar.
//!
//! Como o DOM e o CSS viram [`je_ui`]:
//!
//! - Os elementos são nós com as mesmas classes (`monaco-dialog-modal-block dimmed`, `dialog-shadow`,
//!   `monaco-dialog-box` com `align-vertical`, `dialog-footer-row`, `dialog-buttons-row`, `dialog-buttons`,
//!   `dialog-message-row`, `dialog-icon`, `dialog-message-container`, `dialog-message`,
//!   `dialog-message-text`, `dialog-message-detail`, `dialog-message-body`, `dialog-message-input`,
//!   `dialog-checkbox-row`, `dialog-checkbox-message`, `dialog-toolbar-row`, `dialog-toolbar`), na mesma
//!   ordem do DOM. Os `aria-*` ficam em [`Dialog::attribute`] e o `aria-label` do ícone no rótulo do nó.
//! - Não há motor de layout: [`Dialog::layout`] resolve o `display: flex` do CSS (caixa em `column-reverse`
//!   com `min-width: 480px`, `max-width: 90vw`, `max-height: 90vh`, `min-height: 75px`, `padding: 8px`; a
//!   linha da mensagem com o ícone de 24px (64px em `align-vertical`), `padding: 0 8px 0 12px`; os botões
//!   com `margin: 4px` encostados à direita, com a margem de 67px à esquerda; o `justify-content:
//!   flex-end` da barra do botão de fechar) e é chamado por `show()` (chame de novo ao redimensionar a
//!   janela). O `line-height: 1.4em` do workbench é herdado já em pixels (18,2), o detalhe tem `line-height:
//!   20px` e a mensagem `14px/600`.
//! - O texto (`innerText`, que quebra em `\n`) é medido com [`crate::text::measure_text`] e quebrado por
//!   largura em nós-filhos `dialog-line`, um por linha; o texto inteiro fica no rótulo do nó (o
//!   `textContent`). `white-space: pre-wrap` do detalhe preserva os espaços; `word-wrap: break-word` corta
//!   a palavra que não cabe sozinha na linha.
//! - O bloco modal (`position: fixed`, `z-index: 2575`) cobre a janela inteira (`dom::window_size`) e o
//!   fundo é `rgba(0, 0, 0, 0.5)`. O `box-shadow` (`0 0 8px <dialogShadow>` no `.dialog-shadow` e
//!   `--vscode-shadow-xl`, `0 0 20px rgba(0, 0, 0, 0.15)`, na caixa) não existe na árvore: vira camadas de
//!   retângulos arredondados translúcidos atrás da caixa, com a queda gaussiana da sombra.
//! - O `window.addEventListener('keydown', ..., true)` é um ouvinte de captura na raiz. O `focusout` com
//!   `relatedTarget` é um ouvinte de captura de `Focus` na raiz: se o foco foi para fora da caixa, volta
//!   para o último elemento de dentro.
//! - O `<a>` do original é qualquer nó com papel [`Role::Link`] ou com a classe `monaco-button` (o `Button`
//!   é um `<a>`). O `tabIndex = 0` que o original dá a eles fica em [`Dialog::tab_index`]; a cor do link e o
//!   `text-decoration: underline` de `applyStyles` ficam em [`Dialog::link_style`] (a cor vai para o texto
//!   do link e o sublinhado é um nó filho de 1px).
//!
//! Fica de fora, por enquanto:
//!
//! - **`renderFormattedText` e o Markdown do detalhe.** O detalhe é texto simples quebrado por largura.
//!   `TODO(integração)`: o formatado entra pelo `detailElement` (um nó já pronto) quando o renderizador
//!   existir.
//! - **A rolagem da mensagem** (`overflow-y: auto` do `.dialog-message-container`): o conteúdo que passa de
//!   `90vh` é cortado, sem barra. `TODO(integração)`: pôr um `ScrollableElement`.
//! - **A animação `codicon-modifier-spin`** do tipo `pending`: o ícone é estático.
//! - **O contorno de foco dos links e do `.monaco-button-dropdown:focus-within`**, e o encolhimento das
//!   linhas do rodapé quando a janela é baixa (só a linha da mensagem encolhe).
//! - O `Window` de `onVisibilityChange(window, visible)`: só o `visible`.
//!
//! Suposição: a largura do `.dialog-message-container` é a disponível na linha (os campos de texto ocupam a
//! largura inteira); o CSS deixa a largura do contêiner a cargo do conteúdo e isso só se confirma com o
//! oráculo.

use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::rc::{Rc, Weak};

use je_base::actions::{Action, ActionRef, ToActionProps, to_action};
use je_base::dynamic::Dynamic;
use je_base::key_codes::{KeyCode, KeyMod};
use je_base::labels::mnemonic_button_label_force_disabled;
use je_base::lifecycle::{Disposable, IDisposable, to_disposable};
use je_base::nls::localize;
use je_base::promise::{MaybePromise, Promise, Resolver};
use je_base::themables::ThemeIcon;
use je_render::Rgba;
use je_render::kurbo::Rect;
use je_text::TextStyle;
use je_theme::ColorTheme;
use je_ui::{Border, Content, Event, EventKind, NodeId, PointerEvents, Role, Ui};

use crate::action_bar::{ActionBar, IActionBarOptions, IActionOptions};
use crate::action_view_items::{ActionBarStyles, IActionViewItemOptions, default_action_bar_styles};
use crate::button::{
    Attributes, Button, ButtonBar, ButtonBarAlignment, ButtonDropdownActions, ButtonLabel, ButtonOptions, ButtonStyles, ButtonWithDescription, ButtonWithDropdown, ButtonWithDropdownOptions, IButton,
    IButtonWithDescription, INHERITED_FOREGROUND,
};
use crate::button::flex::{FlexItem, resolve_flexible_lengths};
use crate::common::is_windows;
use crate::dom::{StandardKeyboardEvent, clear_node, create_el, hide, show, window_size};
use crate::input_box::{InputBox, InputBoxOptions, InputBoxStyles, InputType};
use crate::sash::is_macintosh;
use crate::text::{measure_text, ui_font};
use crate::toggle::{Checkbox, CheckboxStyles};

/// O `line-height: 1.4em` do workbench com `font-size: 13px`, já em pixels como o `line-height` herdado,
/// truncado para 1/64 como o `LayoutUnit` do Chromium.
const BASE_LINE_HEIGHT: f64 = 18.1875;
/// O `line-height: 20px` do `.dialog-message-detail`.
const DETAIL_LINE_HEIGHT: f64 = 20.0;
/// O `min-height: 22px` do `.dialog-message`.
const MESSAGE_MIN_HEIGHT: f64 = 22.0;
/// O `padding: 8px` da caixa.
const BOX_PADDING: f64 = 8.0;
/// A altura da linha da barra de ações (`height: 20px` mais `padding-bottom: 4px`).
const TOOLBAR_ROW_HEIGHT: f64 = 24.0;
/// O `padding: 0 8px 0 12px` da linha da mensagem.
const ROW_PADDING_LEFT: f64 = 12.0;
const ROW_PADDING_RIGHT: f64 = 8.0;
/// O `padding: 15px 0 0` das linhas de campo e de caixa de seleção.
const FIELD_PADDING_TOP: f64 = 15.0;
/// O `--vscode-cornerRadius-xLarge`.
const CORNER_RADIUS_X_LARGE: f64 = 12.0;
/// O `z-index` do bloco modal (acima das visões de contexto, abaixo do hover do workbench).
const MODAL_Z_INDEX: i32 = 2575;
/// A largura que o `.monaco-checkbox` reserva na linha: `18px` mais `margin-right: 9px`.
const CHECKBOX_COLUMN: f64 = 27.0;

/// `IDialogInputOptions`.
#[derive(Clone, Debug, Default)]
pub struct DialogInputOptions {
    pub placeholder: Option<String>,
    /// `type`: sem valor, texto.
    pub input_type: Option<InputType>,
    pub value: Option<String>,
}

/// `DialogContentsAlignment`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum DialogContentsAlignment {
    /// Da esquerda para a direita (ícone, mensagem, botões numa linha à parte): o padrão.
    #[default]
    Horizontal,
    /// De cima para baixo (ícone, mensagem e botões empilhados).
    Vertical,
}

/// O `type` de `IDialogOptions`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DialogType {
    None,
    Info,
    Error,
    Question,
    Warning,
    Pending,
}

/// Um item de `buttonOptions`: o `sublabel` e o `styleButton`.
#[derive(Clone, Default)]
pub struct DialogButtonOptions {
    pub sublabel: Option<String>,
    pub style_button: Option<StyleButton>,
}

/// `IDialogResult`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DialogResult {
    pub button: i32,
    pub checkbox_checked: Option<bool>,
    pub values: Option<Vec<String>>,
}

/// `IDialogStyles`, com os campos a mais que o CSS lê por variável: `focus_border` (`--vscode-focusBorder`) e
/// `action_bar` (as cores do botão de fechar).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct DialogStyles {
    pub dialog_foreground: Option<Rgba>,
    pub dialog_background: Option<Rgba>,
    pub dialog_shadow: Option<Rgba>,
    pub dialog_border: Option<Rgba>,
    pub error_icon_foreground: Option<Rgba>,
    pub warning_icon_foreground: Option<Rgba>,
    pub info_icon_foreground: Option<Rgba>,
    pub text_link_foreground: Option<Rgba>,
    pub focus_border: Option<Rgba>,
    pub action_bar: ActionBarStyles,
}

/// Os estilos sem tema: nenhuma cor.
pub fn unthemed_dialog_styles() -> DialogStyles {
    DialogStyles::default()
}

/// `defaultDialogStyles` de `defaultStyles.ts`, com as chaves de tema resolvidas.
pub fn default_dialog_styles(theme: &ColorTheme) -> DialogStyles {
    let color = |id: &str| theme.get_color(id);
    DialogStyles {
        dialog_background: color("editorWidget.background"),
        dialog_foreground: color("editorWidget.foreground"),
        dialog_shadow: color("widget.shadow"),
        dialog_border: color("widget.border"),
        error_icon_foreground: color("problemsErrorIcon.foreground"),
        warning_icon_foreground: color("problemsWarningIcon.foreground"),
        info_icon_foreground: color("problemsInfoIcon.foreground"),
        text_link_foreground: color("textLink.foreground"),
        focus_border: color("focusBorder"),
        action_bar: default_action_bar_styles(theme),
    }
}

/// `IDialogOptions`. Os quatro estilos são obrigatórios como no original; o resto vem de
/// [`DialogOptions::new`] e se troca com a sintaxe `..`.
#[derive(Clone)]
pub struct DialogOptions {
    pub cancel_id: Option<i32>,
    pub detail: Option<String>,
    /// Um nó já pronto no lugar do texto do `detail` (tem precedência sobre ele). Os `<a>` dentro dele entram
    /// na ordem do Tab.
    pub detail_element: Option<NodeId>,
    pub alignment: DialogContentsAlignment,
    pub checkbox_label: Option<String>,
    pub checkbox_checked: bool,
    pub r#type: Option<DialogType>,
    pub extra_classes: Vec<String>,
    /// Classes do bloco modal de janela inteira.
    pub modal_block_extra_classes: Vec<String>,
    pub inputs: Option<Vec<DialogInputOptions>>,
    pub key_event_processor: Option<KeyEventProcessor>,
    /// `renderBody(container)`: o corpo personalizado. Sem motor de layout, quem renderiza posiciona os nós
    /// que cria (`set_bounds`); a altura do corpo é a do que ele ocupa.
    pub render_body: Option<RenderContainer>,
    /// `renderFooter(container)`: idem para o rodapé.
    pub render_footer: Option<RenderContainer>,
    pub icon: Option<ThemeIcon>,
    pub button_options: Vec<Option<DialogButtonOptions>>,
    pub primary_button_dropdown: Option<ButtonWithDropdownOptions>,
    pub disable_close_action: bool,
    pub disable_close_button: bool,
    pub disable_default_action: bool,
    /// Escapatória temporária para widgets de dentro do diálogo cujos popups ficam na raiz da janela.
    pub is_external_focus_allowed: Option<Rc<dyn Fn(NodeId) -> bool>>,
    pub on_visibility_change: Option<Rc<dyn Fn(bool)>>,
    pub button_styles: ButtonStyles,
    pub checkbox_styles: CheckboxStyles,
    pub input_box_styles: InputBoxStyles,
    pub dialog_styles: DialogStyles,
}

impl DialogOptions {
    pub fn new(button_styles: ButtonStyles, checkbox_styles: CheckboxStyles, input_box_styles: InputBoxStyles, dialog_styles: DialogStyles) -> Self {
        Self {
            cancel_id: None,
            detail: None,
            detail_element: None,
            alignment: DialogContentsAlignment::Horizontal,
            checkbox_label: None,
            checkbox_checked: false,
            r#type: None,
            extra_classes: Vec::new(),
            modal_block_extra_classes: Vec::new(),
            inputs: None,
            key_event_processor: None,
            render_body: None,
            render_footer: None,
            icon: None,
            button_options: Vec::new(),
            primary_button_dropdown: None,
            disable_close_action: false,
            disable_close_button: false,
            disable_default_action: false,
            is_external_focus_allowed: None,
            on_visibility_change: None,
            button_styles,
            checkbox_styles,
            input_box_styles,
            dialog_styles,
        }
    }
}

/// O que `applyStyles` põe num link: a cor e o `text-decoration: underline`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct LinkStyle {
    pub color: Rgba,
    pub underline: bool,
}

/// O `ButtonMapEntry`.
#[derive(Clone, Debug, PartialEq, Eq)]
struct ButtonMapEntry {
    label: String,
    index: usize,
}

/// Um botão da barra do diálogo, com o tipo concreto que o layout e o Tab precisam.
#[derive(Clone)]
enum DialogButton {
    Text(Rc<Button>),
    Description(Rc<ButtonWithDescription>),
    Dropdown(Rc<ButtonWithDropdown>),
}

impl DialogButton {
    fn as_button(&self) -> Rc<dyn IButton> {
        match self {
            Self::Text(b) => b.clone(),
            Self::Description(b) => b.clone(),
            Self::Dropdown(b) => b.clone(),
        }
    }

    /// Os botões que recebem foco, na ordem do Tab (o botão com menu tem dois).
    fn focusables(&self) -> Vec<Rc<Button>> {
        match self {
            Self::Text(b) => vec![b.clone()],
            Self::Description(b) => vec![b.button().clone()],
            Self::Dropdown(b) => vec![b.primary_button.clone(), b.dropdown_button.clone()],
        }
    }
}

/// O tamanho de um botão na linha de botões: a caixa de margem.
struct ButtonSlot {
    button: DialogButton,
    /// Largura natural da caixa de borda (o `fit-content`).
    width: f64,
    /// A margem horizontal de cada lado e a vertical.
    margin_x: f64,
    margin_y: f64,
    height: f64,
}

impl ButtonSlot {
    fn outer_width(&self) -> f64 {
        self.width + 2.0 * self.margin_x
    }

    fn outer_height(&self) -> f64 {
        self.height + 2.0 * self.margin_y
    }
}

/// Um nó com a classe, para o que o `querySelectorAll('a')` acha: o `Link` e o `Button` (um `<a>`).
fn is_anchor(tree: &je_ui::Tree, node: NodeId) -> bool {
    tree.role(node) == Role::Link || tree.has_class(node, "monaco-button")
}

/// Os `<a>` abaixo de `root`, na ordem do DOM.
fn anchors_under(ui: &Ui, root: NodeId) -> Vec<NodeId> {
    let tree = ui.tree();
    tree.subtree(root).into_iter().skip(1).filter(|n| is_anchor(&tree, *n)).collect()
}

/// A altura que o conteúdo posicionado à mão de `node` ocupa: o maior fundo entre a caixa do nó e os filhos.
fn content_extent(ui: &Ui, node: NodeId) -> f64 {
    let tree = ui.tree();
    let own = tree.bounds(node).height();
    tree.children(node).iter().map(|c| tree.bounds(*c).y1).fold(own, f64::max)
}

/// A palavra mais larga do texto: o `min-content` do texto que quebra.
fn longest_word(text: &str, style: &TextStyle) -> f64 {
    text.split_whitespace().map(|w| measure_text(w, style)).fold(0.0, f64::max)
}

/// Quebra `text` em linhas de até `width` px: `\n` quebra sempre; `pre_wrap` preserva os espaços (o
/// `white-space: pre-wrap`), senão eles colapsam (`normal`); a palavra mais larga que a linha é cortada em
/// pedaços (`word-wrap: break-word`). O espaço no fim da linha fica pendurado.
fn wrap_lines(text: &str, style: &TextStyle, width: f64, pre_wrap: bool) -> Vec<String> {
    let mut lines = Vec::new();
    for paragraph in text.split('\n') {
        let words: Vec<&str> = if pre_wrap { paragraph.split(' ').collect() } else { paragraph.split_whitespace().collect() };
        let mut current = String::new();
        let mut started = false;
        for word in words {
            let candidate = if started { format!("{current} {word}") } else { word.to_owned() };
            if measure_text(candidate.trim_end(), style) <= width {
                current = candidate;
                started = true;
                continue;
            }
            if started {
                lines.push(std::mem::take(&mut current));
            }
            // A palavra sozinha: cortada em pedaços que cabem.
            let mut piece = String::new();
            for ch in word.chars() {
                let mut next = piece.clone();
                next.push(ch);
                if !piece.is_empty() && measure_text(&next, style) > width {
                    lines.push(std::mem::take(&mut piece));
                    piece.push(ch);
                } else {
                    piece = next;
                }
            }
            current = piece;
            started = true;
        }
        lines.push(current.trim_end().to_owned());
    }
    lines
}

/// A função erro de Abramowitz e Stegun (7.1.26), para a queda gaussiana da sombra.
fn erf(x: f64) -> f64 {
    let t = 1.0 / (1.0 + 0.327_591_1 * x.abs());
    let poly = t * (0.254_829_592 + t * (-0.284_496_736 + t * (1.421_413_741 + t * (-1.453_152_027 + t * 1.061_405_429))));
    let y = 1.0 - poly * (-x * x).exp();
    if x >= 0.0 { y } else { -y }
}

/// As camadas (expansão, alfa) que imitam um `box-shadow: 0 0 <blur>px <cor com alfa>`: a sombra é a caixa
/// borrada por uma gaussiana de desvio `blur / 2`, com metade da intensidade na borda; cada camada cobre uma
/// faixa de largura `step` e o alfa dela é escolhido para que a composição das camadas dê a queda exata no
/// meio da faixa.
fn shadow_layers(blur: f64, alpha: f64) -> Vec<(f64, f64)> {
    const LAYERS: usize = 8;
    let sigma = blur / 2.0;
    let step = blur / LAYERS as f64;
    let intensity = |j: usize| if j > LAYERS { 0.0 } else { alpha * (1.0 - 0.5 * (1.0 + erf(((j as f64 - 0.5) * step / sigma) / std::f64::consts::SQRT_2))) };
    (1..=LAYERS).map(|j| (j as f64 * step, 1.0 - (1.0 - intensity(j)) / (1.0 - intensity(j + 1)))).collect()
}

/// Como um bloco de texto é quebrado e pintado.
struct LineBlock<'a> {
    style: &'a TextStyle,
    color: Rgba,
    line_height: f64,
    width: f64,
    pre_wrap: bool,
    centered: bool,
}

/// Um callback sem argumentos que dá o foco a um elemento.
type FocusFn = Rc<dyn Fn()>;
/// `styleButton`.
pub type StyleButton = Rc<dyn Fn(&dyn IButton)>;
/// `keyEventProcessor`.
pub type KeyEventProcessor = Rc<dyn Fn(&StandardKeyboardEvent)>;
/// `renderBody` e `renderFooter`: recebem a árvore e o contêiner.
pub type RenderContainer = Rc<dyn Fn(&Ui, NodeId)>;

/// Um nó de camada de sombra e a expansão dela.
struct ShadowLayer {
    node: NodeId,
    spread: f64,
    /// Sombra do `dialogShadow` (`true`) ou o `--vscode-shadow-xl` da caixa.
    from_style: bool,
    alpha: f64,
}

/// O diálogo modal.
pub struct Dialog {
    base: Rc<Disposable>,
    weak_self: Weak<Dialog>,
    ui: Ui,
    container: NodeId,
    message: String,
    options: DialogOptions,
    vertical: bool,
    modal_element: Cell<Option<NodeId>>,
    shadow_element: NodeId,
    shadow_layers: Vec<ShadowLayer>,
    element: NodeId,
    toolbar_row: NodeId,
    toolbar_container: NodeId,
    message_row: NodeId,
    buttons_row: NodeId,
    buttons_container: NodeId,
    footer_row: Option<NodeId>,
    footer_content: Option<NodeId>,
    footer_action_to_focus: Cell<Option<NodeId>>,
    icon_element: NodeId,
    message_container: NodeId,
    message_element: Option<NodeId>,
    message_text_element: Option<NodeId>,
    message_detail_element: NodeId,
    detail_text: RefCell<Option<String>>,
    message_body_element: Option<NodeId>,
    input_rows: Vec<NodeId>,
    inputs: Vec<Rc<InputBox>>,
    checkbox_row: Option<NodeId>,
    checkbox_message_element: Option<NodeId>,
    checkbox: Option<Rc<Checkbox>>,
    buttons: Vec<String>,
    button_styles: ButtonStyles,
    dialog_buttons: RefCell<Vec<DialogButton>>,
    button_bar: RefCell<Option<Rc<ButtonBar>>>,
    action_bar: RefCell<Option<Rc<ActionBar>>>,
    focus_to_return: Cell<Option<NodeId>>,
    last_inner_focus: Cell<Option<NodeId>>,
    tab_indexes: RefCell<HashMap<NodeId, i32>>,
    link_styles: RefCell<HashMap<NodeId, LinkStyle>>,
    attributes: Attributes,
    shown: Cell<bool>,
    disposed: Cell<bool>,
}

impl Dialog {
    /// `new Dialog(container, message, buttons, options)`: monta a caixa (escondida até o `show()`).
    pub fn new(ui: &Ui, container: NodeId, message: &str, buttons: Option<Vec<String>>, options: DialogOptions) -> Rc<Dialog> {
        let base = Disposable::new();
        let vertical = options.alignment == DialogContentsAlignment::Vertical;
        let has_detail = options.detail.as_deref().is_some_and(|d| !d.is_empty()) || options.detail_element.is_some();
        let has_body = options.render_body.is_some();

        // Modal background blocker, Dialog Box.
        let (modal, shadow_element) = {
            let mut tree = ui.tree_mut();
            let modal = create_el(&mut tree, container, Role::Generic, "monaco-dialog-modal-block dimmed");
            for class in &options.modal_block_extra_classes {
                tree.add_class(modal, class);
            }
            let style = tree.style_mut(modal);
            style.background = Some(Rgba::new(0, 0, 0, 128));
            style.z_index = MODAL_Z_INDEX;
            let shadow_element = create_el(&mut tree, modal, Role::Generic, "dialog-shadow");
            tree.style_mut(shadow_element).radius = CORNER_RADIUS_X_LARGE;
            (modal, shadow_element)
        };
        let shadow_layers = Self::create_shadow_layers(ui, shadow_element, &options.dialog_styles);
        let element = {
            let mut tree = ui.tree_mut();
            let element = create_el(&mut tree, shadow_element, Role::Dialog, "monaco-dialog-box");
            if vertical {
                tree.add_class(element, "align-vertical");
            }
            for class in &options.extra_classes {
                tree.add_class(element, class);
            }
            tree.style_mut(element).radius = CORNER_RADIUS_X_LARGE;
            element
        };
        let attributes = Attributes::default();

        // Footer
        let (footer_row, footer_content) = match options.render_footer.clone() {
            Some(render_footer) => {
                let (row, content) = {
                    let mut tree = ui.tree_mut();
                    let row = create_el(&mut tree, element, Role::Generic, "dialog-footer-row");
                    let content = create_el(&mut tree, row, Role::Generic, "dialog-footer");
                    (row, content)
                };
                render_footer(ui, content);
                (Some(row), Some(content))
            }
            None => (None, None),
        };
        let footer_action_to_focus = Cell::new(None);
        let tab_indexes = RefCell::new(HashMap::new());
        if let Some(row) = footer_row {
            for anchor in anchors_under(ui, row) {
                tab_indexes.borrow_mut().insert(anchor, 0);
                if footer_action_to_focus.get().is_none() {
                    footer_action_to_focus.set(Some(anchor));
                }
            }
        }

        // Buttons
        let button_styles = options.button_styles;
        let buttons = match buttons {
            Some(buttons) if !buttons.is_empty() => buttons,
            _ if !options.disable_default_action => vec![localize("ok", "OK", &[])],
            _ => Vec::new(),
        };
        let (buttons_row, buttons_container) = {
            let mut tree = ui.tree_mut();
            let row = create_el(&mut tree, element, Role::Generic, "dialog-buttons-row");
            tree.style_mut(row).clip = true;
            let container = create_el(&mut tree, row, Role::Generic, "dialog-buttons");
            (row, container)
        };

        // Message
        let (message_row, icon_element, message_container) = {
            let mut tree = ui.tree_mut();
            let row = create_el(&mut tree, element, Role::Generic, "dialog-message-row");
            tree.style_mut(row).clip = true;
            let icon = create_el(&mut tree, row, Role::Image, "dialog-icon");
            let container = create_el(&mut tree, row, Role::Generic, "dialog-message-container");
            tree.style_mut(container).clip = true;
            (row, icon, container)
        };
        ui.tree_mut().set_label(icon_element, Self::icon_aria_label(options.r#type));

        let (message_element, message_text_element) = if has_detail || has_body {
            let mut tree = ui.tree_mut();
            let message_element = create_el(&mut tree, message_container, Role::Generic, "dialog-message");
            let text = create_el(&mut tree, message_element, Role::Generic, "dialog-message-text");
            tree.set_label(text, message);
            (Some(message_element), Some(text))
        } else {
            (None, None)
        };

        let message_detail_element = create_el(&mut ui.tree_mut(), message_container, Role::Generic, "dialog-message-detail");
        let detail_text = if let Some(detail_element) = options.detail_element {
            ui.tree_mut().append_child(message_detail_element, detail_element);
            None
        } else if has_detail || !has_body {
            let text = options.detail.clone().filter(|d| !d.is_empty()).unwrap_or_else(|| message.to_owned());
            ui.tree_mut().set_label(message_detail_element, text.clone());
            Some(text)
        } else {
            ui.tree_mut().style_mut(message_detail_element).visible = false;
            None
        };

        let message_body_element = options.render_body.clone().map(|render_body| {
            let body = create_el(&mut ui.tree_mut(), message_container, Role::Generic, "dialog-message-body");
            render_body(ui, body);
            body
        });

        if has_body || options.detail_element.is_some() {
            for anchor in anchors_under(ui, message_container) {
                tab_indexes.borrow_mut().insert(anchor, 0);
            }
        }

        // Inputs
        let mut input_rows = Vec::new();
        let mut inputs = Vec::new();
        for input in options.inputs.iter().flatten() {
            let row = create_el(&mut ui.tree_mut(), message_container, Role::Generic, "dialog-message-input");
            let input_box = InputBox::new(
                ui,
                row,
                None,
                InputBoxOptions { placeholder: input.placeholder.clone(), input_type: input.input_type.unwrap_or(InputType::Text), ..InputBoxOptions::new(options.input_box_styles.clone()) },
            );
            base.register(input_box.clone());
            if let Some(value) = input.value.as_deref().filter(|v| !v.is_empty()) {
                input_box.set_value(value);
            }
            input_rows.push(row);
            inputs.push(input_box);
        }

        // Checkbox
        let (checkbox_row, checkbox_message_element, checkbox) = match options.checkbox_label.as_deref().filter(|l| !l.is_empty()) {
            Some(label) => {
                let row = create_el(&mut ui.tree_mut(), message_container, Role::Generic, "dialog-checkbox-row");
                let checkbox = Checkbox::new(ui, label, options.checkbox_checked, options.checkbox_styles.clone());
                base.register(checkbox.clone());
                let message_node = {
                    let mut tree = ui.tree_mut();
                    tree.append_child(row, checkbox.dom_node());
                    let message_node = create_el(&mut tree, row, Role::Generic, "dialog-checkbox-message");
                    tree.set_label(message_node, label);
                    tree.style_mut(message_node).cursor = Some(je_ui::Cursor::Pointer);
                    message_node
                };
                let toggled = checkbox.clone();
                base.register(ui.listen(message_node, EventKind::Click, false, move |_| toggled.set_checked(!toggled.checked())));
                (Some(row), Some(message_node), Some(checkbox))
            }
            None => (None, None, None),
        };

        // Toolbar
        let (toolbar_row, toolbar_container) = {
            let mut tree = ui.tree_mut();
            let row = create_el(&mut tree, element, Role::Generic, "dialog-toolbar-row");
            let container = create_el(&mut tree, row, Role::Generic, "dialog-toolbar");
            (row, container)
        };

        hide(&mut ui.tree_mut(), element);
        for layer in &shadow_layers {
            hide(&mut ui.tree_mut(), layer.node);
        }

        let this = Rc::new_cyclic(|weak| Dialog {
            base,
            weak_self: weak.clone(),
            ui: ui.clone(),
            container,
            message: message.to_owned(),
            options,
            vertical,
            modal_element: Cell::new(Some(modal)),
            shadow_element,
            shadow_layers,
            element,
            toolbar_row,
            toolbar_container,
            message_row,
            buttons_row,
            buttons_container,
            footer_row,
            footer_content,
            footer_action_to_focus,
            icon_element,
            message_container,
            message_element,
            message_text_element,
            message_detail_element,
            detail_text: RefCell::new(detail_text),
            message_body_element,
            input_rows,
            inputs,
            checkbox_row,
            checkbox_message_element,
            checkbox,
            buttons,
            button_styles,
            dialog_buttons: RefCell::new(Vec::new()),
            button_bar: RefCell::new(None),
            action_bar: RefCell::new(None),
            focus_to_return: Cell::new(None),
            last_inner_focus: Cell::new(None),
            tab_indexes,
            link_styles: RefCell::new(HashMap::new()),
            attributes,
            shown: Cell::new(false),
            disposed: Cell::new(false),
        });

        // guide users back into the dialog if clicked elsewhere
        let weak = this.weak_self.clone();
        this.base.register(ui.listen(modal, EventKind::Click, false, move |e| {
            if let Some(dialog) = weak.upgrade() && Some(e.target) == dialog.modal_element.get() {
                dialog.ui.focus(dialog.element);
            }
        }));
        this.element_attribute_defaults();
        this.apply_styles();
        this
    }

    /// As camadas da sombra: a do `dialogShadow` (`0 0 8px`) e a de `--vscode-shadow-xl` (`0 0 20px
    /// rgba(0, 0, 0, 0.15)`), atrás da caixa.
    fn create_shadow_layers(ui: &Ui, shadow_element: NodeId, styles: &DialogStyles) -> Vec<ShadowLayer> {
        let mut specs: Vec<(f64, f64, bool)> = Vec::new();
        if let Some(color) = styles.dialog_shadow {
            specs.extend(shadow_layers(8.0, f64::from(color.a) / 255.0).into_iter().rev().map(|(spread, alpha)| (spread, alpha, true)));
        }
        specs.extend(shadow_layers(20.0, 0.15).into_iter().rev().map(|(spread, alpha)| (spread, alpha, false)));
        let mut tree = ui.tree_mut();
        specs
            .into_iter()
            .map(|(spread, alpha, from_style)| {
                let node = create_el(&mut tree, shadow_element, Role::Presentation, "dialog-shadow-layer");
                tree.style_mut(node).pointer_events = PointerEvents::None;
                ShadowLayer { node, spread, from_style, alpha }
            })
            .collect()
    }

    /// `getIconAriaLabel`.
    fn icon_aria_label(r#type: Option<DialogType>) -> String {
        match r#type {
            Some(DialogType::Error) => localize("dialogErrorMessage", "Error", &[]),
            Some(DialogType::Warning) => localize("dialogWarningMessage", "Warning", &[]),
            Some(DialogType::Pending) => localize("dialogPendingMessage", "In Progress", &[]),
            _ => localize("dialogInfoMessage", "Info", &[]),
        }
    }

    /// Os `setAttribute` do `show()` que valem desde já, e o `role` do nó.
    fn element_attribute_defaults(&self) {
        self.attributes.set("role", "dialog");
        self.attributes.set("tabindex", "-1");
    }

    // ----- acessores -----

    /// O bloco modal (`.monaco-dialog-modal-block`); `None` depois de descartado.
    pub fn modal_element(&self) -> Option<NodeId> {
        self.modal_element.get()
    }

    /// O `.monaco-dialog-box`.
    pub fn element(&self) -> NodeId {
        self.element
    }

    /// O `.dialog-shadow`, que envolve a caixa.
    pub fn shadow_element(&self) -> NodeId {
        self.shadow_element
    }

    pub fn message_container(&self) -> NodeId {
        self.message_container
    }

    /// O `.dialog-message-detail`.
    pub fn message_detail_element(&self) -> NodeId {
        self.message_detail_element
    }

    pub fn buttons_container(&self) -> NodeId {
        self.buttons_container
    }

    pub fn footer_container(&self) -> Option<NodeId> {
        self.footer_row
    }

    pub fn icon_element(&self) -> NodeId {
        self.icon_element
    }

    pub fn toolbar_container(&self) -> NodeId {
        self.toolbar_container
    }

    /// Os botões (só depois do `show()`).
    pub fn buttons(&self) -> Vec<Rc<dyn IButton>> {
        self.dialog_buttons.borrow().iter().map(DialogButton::as_button).collect()
    }

    pub fn inputs(&self) -> &[Rc<InputBox>] {
        &self.inputs
    }

    pub fn checkbox(&self) -> Option<&Rc<Checkbox>> {
        self.checkbox.as_ref()
    }

    /// Os `aria-*` e o `role` da caixa.
    pub fn attribute(&self, name: &str) -> Option<String> {
        self.attributes.get(name)
    }

    /// O `tabIndex` que o diálogo deu a um `<a>` (0), ou `None` se não mexeu nele.
    pub fn tab_index(&self, node: NodeId) -> Option<i32> {
        self.tab_indexes.borrow().get(&node).copied()
    }

    /// A cor e o sublinhado que o diálogo deu a um link, ou `None` se não mexeu nele.
    pub fn link_style(&self, node: NodeId) -> Option<LinkStyle> {
        self.link_styles.borrow().get(&node).copied()
    }

    /// `updateMessage(message)`.
    pub fn update_message(&self, message: &str) {
        *self.detail_text.borrow_mut() = Some(message.to_owned());
        self.ui.tree_mut().set_label(self.message_detail_element, message);
        if self.shown.get() {
            self.layout();
        }
    }

    // ----- show -----

    /// `show()`: mostra a caixa e devolve o resultado quando um botão (ou o fechar, ou o Escape) resolver.
    pub fn show(&self) -> Promise<DialogResult> {
        let this = self.weak_self.upgrade().expect("o diálogo está vivo");
        this.focus_to_return.set(this.ui.focused());
        Promise::with_executor(move |resolver| this.run_show(resolver))
    }

    fn result(&self, button: i32, with_values: bool) -> DialogResult {
        DialogResult {
            button,
            checkbox_checked: self.checkbox.as_ref().map(|c| c.checked()),
            values: (with_values && !self.inputs.is_empty()).then(|| self.inputs.iter().map(|i| i.value()).collect()),
        }
    }

    fn run_show(self: &Rc<Self>, resolver: Resolver<DialogResult>) {
        clear_node(&mut self.ui.tree_mut(), self.buttons_container);

        let cancel = self.options.cancel_id.unwrap_or(0);
        {
            let (weak, resolver) = (self.weak_self.clone(), resolver.clone());
            self.base.register(to_disposable(move || {
                if let Some(dialog) = weak.upgrade() {
                    resolver.resolve(dialog.result(cancel, false));
                }
            }));
        }

        let alignment = if self.vertical { ButtonBarAlignment::Vertical } else { ButtonBarAlignment::Horizontal };
        let button_bar = Rc::new(ButtonBar::new(&self.ui, self.buttons_container, Some(alignment)));
        self.base.register(button_bar.clone());
        *self.button_bar.borrow_mut() = Some(button_bar.clone());
        let button_map = Rc::new(self.rearrange_buttons(self.options.cancel_id));

        // Buttons
        for index in 0..button_map.len() {
            self.add_button(&button_bar, &button_map, index, &resolver);
        }

        // Handle keyboard events globally: Tab, Arrow-Left/Right
        let saw_escape_key_down = Rc::new(Cell::new(false));
        {
            let (weak, resolver, button_map, saw_escape) = (self.weak_self.clone(), resolver.clone(), button_map.clone(), saw_escape_key_down.clone());
            self.base.register(self.ui.listen(self.ui.root(), EventKind::KeyDown, true, move |e| {
                if let Some(dialog) = weak.upgrade() {
                    dialog.on_key_down(e, &resolver, &button_map, &saw_escape);
                }
            }));
        }
        {
            let (weak, resolver) = (self.weak_self.clone(), resolver.clone());
            self.base.register(self.ui.listen(self.ui.root(), EventKind::KeyUp, true, move |e| {
                e.stop(true);
                let Some(dialog) = weak.upgrade() else { return };
                let Some(event) = StandardKeyboardEvent::from_event(e) else { return };
                if !dialog.options.disable_close_action && event.equals(KeyCode::Escape.bits()) && saw_escape_key_down.get() {
                    resolver.resolve(dialog.result(cancel, false));
                }
            }));
        }

        // Detect focus out
        {
            let weak = self.weak_self.clone();
            self.base.register(self.ui.listen(self.element, EventKind::Focus, true, move |e| {
                if let Some(dialog) = weak.upgrade() {
                    dialog.last_inner_focus.set(Some(e.target));
                }
            }));
            let weak = self.weak_self.clone();
            self.base.register(self.ui.listen(self.ui.root(), EventKind::Focus, true, move |e| {
                if let Some(dialog) = weak.upgrade() {
                    dialog.on_focus_moved(e);
                }
            }));
        }

        self.refresh_icon();

        if !self.options.disable_close_action && !self.options.disable_close_button {
            self.add_close_button(&resolver);
        }

        self.apply_styles();

        self.attributes.set("aria-modal", "true");
        self.attributes.set("aria-labelledby", "monaco-dialog-icon monaco-dialog-message-text");
        self.attributes.set("aria-describedby", "monaco-dialog-icon monaco-dialog-message-text monaco-dialog-message-detail monaco-dialog-message-body monaco-dialog-footer");
        self.shown.set(true);
        self.layout();
        {
            let mut tree = self.ui.tree_mut();
            show(&mut tree, self.element);
            for layer in &self.shadow_layers {
                show(&mut tree, layer.node);
            }
        }

        // Notify visibility change
        if let Some(on_visibility_change) = self.options.on_visibility_change.clone() {
            on_visibility_change(true);
            self.base.register(to_disposable(move || on_visibility_change(false)));
        }

        // Focus first element (input or button)
        if let Some(first) = self.inputs.first() {
            first.focus();
            first.select(None);
        } else {
            let mut focused_button = false;
            let buttons = self.dialog_buttons.borrow().clone();
            for (index, entry) in button_map.iter().enumerate() {
                if entry.index == 0 {
                    buttons[index].as_button().focus();
                    focused_button = true;
                }
            }
            if !focused_button {
                self.ui.focus(self.footer_action_to_focus.get().unwrap_or(self.element));
            }
        }
    }

    /// Um botão da barra: o principal com menu, com descrição ou comum, com o rótulo e o clique.
    fn add_button(&self, button_bar: &Rc<ButtonBar>, button_map: &Rc<Vec<ButtonMapEntry>>, index: usize, resolver: &Resolver<DialogResult>) {
        let entry = &button_map[index];
        let primary = entry.index == 0;
        let button_options = self.options.button_options.get(entry.index).cloned().flatten();
        let on_click = {
            let (weak, resolver, button_map) = (self.weak_self.clone(), resolver.clone(), button_map.clone());
            Rc::new(move || {
                if let Some(dialog) = weak.upgrade() {
                    resolver.resolve(dialog.result(button_map[index].index as i32, true));
                }
            })
        };
        let button_base_options = ButtonOptions { secondary: !primary, styles: self.button_styles, ..ButtonOptions::default() };
        let button = if let (true, Some(dropdown)) = (primary, self.options.primary_button_dropdown.clone()) {
            let actions = match &dropdown.actions {
                ButtonDropdownActions::Provider(provider) => provider.get_actions(),
                ButtonDropdownActions::Actions(actions) => actions.clone(),
            };
            let actions = actions.into_iter().map(|action| Self::run_then_click(action, on_click.clone())).collect();
            let button = button_bar.add_button_with_dropdown(ButtonWithDropdownOptions {
                button: ButtonOptions { styles: self.button_styles, ..dropdown.button.clone() },
                actions: ButtonDropdownActions::Actions(actions),
                dropdown_layer: Some(2600), // ensure the dropdown is above the dialog
                ..dropdown
            });
            DialogButton::Dropdown(button)
        } else if button_options.as_ref().is_some_and(|o| o.sublabel.is_some()) {
            DialogButton::Description(button_bar.add_button_with_description(button_base_options))
        } else {
            DialogButton::Text(button_bar.add_button(button_base_options))
        };

        let generic = button.as_button();
        if let Some(style_button) = button_options.as_ref().and_then(|o| o.style_button.clone()) {
            style_button(&*generic);
        }
        generic.set_label(ButtonLabel::Text(mnemonic_button_label_force_disabled(&entry.label)));
        if let (DialogButton::Description(description), Some(sublabel)) = (&button, button_options.as_ref().and_then(|o| o.sublabel.clone())) {
            description.set_description(&sublabel);
        }
        self.base.register(generic.on_did_click().listen(move |_| on_click()));
        self.dialog_buttons.borrow_mut().push(button);
    }

    /// `toAction({ ...action, run: async () => { await action.run(); onButtonClick(index); } })`.
    fn run_then_click(action: ActionRef, on_click: Rc<dyn Fn()>) -> ActionRef {
        let inner = action.clone();
        to_action(ToActionProps {
            id: action.id(),
            label: action.label(),
            tooltip: Some(action.tooltip()),
            enabled: Some(action.enabled()),
            checked: action.checked(),
            class: action.class(),
            alt: action.alt(),
            run: Rc::new(move |args: &[Dynamic]| {
                let (ran, on_click) = (inner.run(args), on_click.clone());
                MaybePromise::Promise(ran.then(move |value| {
                    on_click();
                    value
                }))
            }),
        })
    }

    /// O botão de fechar: um `ActionBar` com a ação `dialog.close` (só o ícone).
    fn add_close_button(&self, resolver: &Resolver<DialogResult>) {
        let styles = {
            let mut styles = self.options.dialog_styles.action_bar.clone();
            styles.foreground = self.options.dialog_styles.dialog_foreground.or(styles.foreground);
            Rc::new(styles)
        };
        let action_bar = ActionBar::new(&self.ui, self.toolbar_container, IActionBarOptions { styles: Some(styles), ..Default::default() });
        self.base.register(action_bar.clone());
        let (weak, resolver) = (self.weak_self.clone(), resolver.clone());
        let action = Action::new(
            "dialog.close",
            &localize("dialogClose", "Close Dialog", &[]),
            &ThemeIcon::from_id("dialog-close").as_class_name(),
            true,
            Some(Rc::new(move |_| {
                if let Some(dialog) = weak.upgrade() {
                    resolver.resolve(dialog.result(dialog.options.cancel_id.unwrap_or(0), false));
                }
                MaybePromise::Value(Dynamic::Undefined)
            })),
        );
        self.base.register(action.clone());
        action_bar.push_action(action, IActionOptions { item: IActionViewItemOptions { icon: Some(true), label: Some(false), ..Default::default() }, index: None });
        *self.action_bar.borrow_mut() = Some(action_bar);
    }

    // ----- teclado e foco -----

    /// Os elementos focáveis na ordem visual (links da mensagem, campos, caixa de seleção, botões, links do
    /// rodapé) e o índice do que tem o foco, se algum.
    fn focus_order(&self) -> (Vec<FocusFn>, Option<usize>) {
        let mut elements: Vec<FocusFn> = Vec::new();
        let mut focused: Option<usize> = None;
        let active = self.ui.focused();
        let mut push = |focus: FocusFn, has_focus: bool| {
            elements.push(focus);
            if has_focus {
                focused = Some(elements.len() - 1);
            }
        };
        let node_focus = |node: NodeId| -> FocusFn {
            let ui = self.ui.clone();
            Rc::new(move || ui.focus(node))
        };
        for link in anchors_under(&self.ui, self.message_container) {
            push(node_focus(link), active == Some(link));
        }
        for input in &self.inputs {
            let (target, has_focus) = (input.clone(), input.has_focus());
            push(Rc::new(move || target.focus()), has_focus);
        }
        if let Some(checkbox) = &self.checkbox {
            let (target, has_focus) = (checkbox.clone(), checkbox.has_focus());
            push(Rc::new(move || target.focus()), has_focus);
        }
        for button in self.dialog_buttons.borrow().iter() {
            for target in button.focusables() {
                let has_focus = target.has_focus();
                push(Rc::new(move || target.focus()), has_focus);
            }
        }
        if let Some(footer) = self.footer_row {
            for link in anchors_under(&self.ui, footer) {
                push(node_focus(link), active == Some(link));
            }
        }
        (elements, focused)
    }

    /// O ouvinte global de `keydown`.
    fn on_key_down(&self, e: &mut Event, resolver: &Resolver<DialogResult>, button_map: &[ButtonMapEntry], saw_escape_key_down: &Cell<bool>) {
        let Some(evt) = StandardKeyboardEvent::from_event(e) else { return };
        let cancel_id = self.options.cancel_id;

        if evt.equals(KeyCode::Escape.bits()) {
            saw_escape_key_down.set(true);
        }

        if evt.equals(KeyMod::ALT) {
            e.prevent_default();
        }

        if evt.equals(KeyCode::Enter.bits()) {
            // Enter in input field should OK the dialog
            if self.inputs.iter().any(|input| input.has_focus()) {
                e.stop(false);
                let button = button_map.iter().find(|b| Some(b.index as i32) != cancel_id).map_or(0, |b| b.index as i32);
                resolver.resolve(self.result(button, true));
            }
            return; // leave default handling
        }

        // Cmd+D (trigger the "no"/"do not save"-button) (macOS only)
        if is_macintosh() && evt.equals(KeyMod::CTRL_CMD | KeyCode::KeyD.bits()) {
            e.stop(false);
            if let Some(no_button) = button_map.iter().find(|b| b.index == 1 && Some(b.index as i32) != cancel_id) {
                resolver.resolve(self.result(no_button.index as i32, true));
            }
            return; // leave default handling
        }

        if evt.equals(KeyCode::Space.bits()) {
            return; // leave default handling
        }

        let mut event_handled = false;

        // Focus: Next / Previous
        let is_arrow_navigation = evt.equals(KeyCode::RightArrow.bits()) || evt.equals(KeyCode::LeftArrow.bits());
        let is_editable_target = self.ui.tree().role(e.target) == Role::TextInput;
        if evt.equals(KeyCode::Tab.bits()) || evt.equals(KeyMod::SHIFT | KeyCode::Tab.bits()) || (is_arrow_navigation && !is_editable_target) {
            // Build a list of focusable elements in their visual order
            let (elements, focused) = self.focus_order();
            if !elements.is_empty() {
                let count = elements.len();
                // Focus next element (with wrapping)
                let target = if evt.equals(KeyCode::Tab.bits()) || evt.equals(KeyCode::RightArrow.bits()) {
                    focused.map_or(0, |i| (i + 1) % count)
                } else {
                    // Focus previous element (with wrapping); default to focus last element if none have focus
                    let from = focused.unwrap_or(count);
                    if from == 0 { count - 1 } else { from - 1 }
                };
                elements[target]();
            }
            event_handled = true;
        }

        if event_handled {
            e.stop(true);
        } else if let Some(processor) = &self.options.key_event_processor {
            processor(&evt);
        }
    }

    /// O `focusout` da caixa: o foco que foi parar fora dela volta para o último elemento de dentro.
    fn on_focus_moved(&self, e: &mut Event) {
        let target = e.target;
        let (inside, root) = {
            let tree = self.ui.tree();
            (tree.is_ancestor(self.element, target), tree.root())
        };
        if inside || target == root {
            return;
        }
        // Temporary: let focus escape for body-level popups.
        if self.options.is_external_focus_allowed.as_ref().is_some_and(|allowed| allowed(target)) {
            return;
        }
        self.focus_to_return.set(Some(target));
        let back = self.last_inner_focus.get().filter(|n| self.ui.tree().is_alive(*n)).unwrap_or(self.element);
        self.ui.focus(back);
        e.stop(true);
    }

    /// `rearrangeButtons`.
    fn rearrange_buttons(&self, cancel_id: Option<i32>) -> Vec<ButtonMapEntry> {
        // Maps each button to its current label and old index so that when we move them around it's not a problem
        let mut button_map: Vec<ButtonMapEntry> = self.buttons.iter().enumerate().map(|(index, label)| ButtonMapEntry { label: label.clone(), index }).collect();
        if self.buttons.len() < 2 || self.vertical {
            return button_map; // only need to rearrange if there are 2+ buttons and the alignment is left-to-right
        }
        let cancel = cancel_id.and_then(|id| usize::try_from(id).ok()).filter(|id| *id < button_map.len());
        if is_windows() {
            // Windows: "[Do it]/Cancel": o cancelar por último.
            if let Some(cancel) = cancel {
                let cancel_button = button_map.remove(cancel);
                button_map.push(cancel_button);
            }
        } else {
            // Linux (GNOME HIG) e macOS (HIG): o cancelar vem antes do afirmativo, e a ordem se inverte.
            if let Some(cancel) = cancel {
                let cancel_button = button_map.remove(cancel);
                button_map.insert(1, cancel_button);
            }
            button_map.reverse();
        }
        button_map
    }

    // ----- ícone e estilos -----

    /// As classes do ícone e o glifo, pelo `type` (ou pelo `icon`).
    fn icon_classes(&self) -> Vec<String> {
        let mut classes = vec!["dialog-icon".to_owned()];
        if let Some(icon) = &self.options.icon {
            classes.extend(icon.as_class_name_array());
            return classes;
        }
        match self.options.r#type {
            Some(DialogType::Error) => classes.extend(ThemeIcon::from_id("dialog-error").as_class_name_array()),
            Some(DialogType::Warning) => classes.extend(ThemeIcon::from_id("dialog-warning").as_class_name_array()),
            Some(DialogType::Pending) => {
                classes.extend(ThemeIcon::from_id("loading").as_class_name_array());
                classes.push("codicon-modifier-spin".to_owned());
            }
            Some(DialogType::None) => classes.push("no-codicon".to_owned()),
            Some(DialogType::Info | DialogType::Question) | None => classes.extend(ThemeIcon::from_id("dialog-info").as_class_name_array()),
        }
        classes
    }

    /// O glifo do ícone, ou `None` quando não há ícone (`no-codicon`).
    fn icon_glyph(&self) -> Option<String> {
        self.icon_classes().iter().find_map(|c| c.strip_prefix("codicon-").filter(|id| !id.starts_with("modifier-")).map(str::to_owned))
    }

    /// Troca as classes do ícone (o `classList.remove` e `add` do `show()`).
    fn refresh_icon(&self) {
        let classes = self.icon_classes().join(" ");
        self.ui.tree_mut().set_class_name(self.icon_element, &classes);
        self.refresh_icon_content();
    }

    /// O conteúdo do ícone: o glifo no tamanho do CSS (24px, ou 64px no `align-vertical`) e a cor do tipo.
    fn refresh_icon_content(&self) {
        let styles = &self.options.dialog_styles;
        let color = match self.options.r#type {
            Some(DialogType::None) => None,
            Some(DialogType::Error) => styles.error_icon_foreground,
            Some(DialogType::Warning) => styles.warning_icon_foreground,
            _ => styles.info_icon_foreground,
        };
        let color = color.or(styles.dialog_foreground).unwrap_or(INHERITED_FOREGROUND);
        let size = if self.vertical { 64.0 } else { 24.0 };
        let content = match self.icon_glyph() {
            Some(id) if self.ui.tree().has_class(self.icon_element, "codicon") => Content::Codicon { id, size: size as f32, color },
            _ => Content::None,
        };
        self.ui.tree_mut().style_mut(self.icon_element).content = content;
    }

    /// `applyStyles`.
    fn apply_styles(&self) {
        let style = &self.options.dialog_styles;
        {
            let mut tree = self.ui.tree_mut();
            for layer in &self.shadow_layers {
                let color = if layer.from_style { style.dialog_shadow.unwrap_or(Rgba::TRANSPARENT) } else { Rgba::new(0, 0, 0, 0) };
                tree.style_mut(layer.node).background = Some(Rgba { a: (layer.alpha * 255.0).round() as u8, ..color });
            }
            let element = tree.style_mut(self.element);
            element.background = style.dialog_background;
            element.border = style.dialog_border.map(|color| Border { width: 1.0, color });
        }
        if let Some(color) = style.text_link_foreground {
            let mut links = anchors_under(&self.ui, self.message_container);
            if let Some(footer) = self.footer_row {
                links.extend(anchors_under(&self.ui, footer));
            }
            for link in links {
                if self.ui.tree().has_class(link, "monaco-button") {
                    continue;
                }
                self.style_link(link, color);
            }
        }
        self.refresh_icon_content();
    }

    /// `el.style.color = linkFgColor; el.style.textDecoration = 'underline'`.
    fn style_link(&self, link: NodeId, color: Rgba) {
        let text_nodes: Vec<NodeId> = {
            let tree = self.ui.tree();
            tree.subtree(link).into_iter().filter(|n| matches!(tree.style(*n).content, Content::Text { .. })).collect()
        };
        for node in text_nodes {
            let (text, style, line_height) = {
                let mut tree = self.ui.tree_mut();
                let Content::Text { text, style, line_height, .. } = tree.style(node).content.clone() else { continue };
                tree.style_mut(node).content = Content::Text { text: text.clone(), style: style.clone(), color, line_height };
                (text, style, line_height)
            };
            let underline = self.ui.tree().children_with_class(node, "text-decoration-underline").first().copied();
            let underline = underline.unwrap_or_else(|| {
                let mut tree = self.ui.tree_mut();
                let node = create_el(&mut tree, node, Role::Presentation, "text-decoration-underline");
                tree.style_mut(node).pointer_events = PointerEvents::None;
                node
            });
            let shaped = je_text::with_text_system(|t| t.line(&text, &style));
            let baseline = (line_height - f64::from(shaped.line_height)) / 2.0 + f64::from(shaped.baseline);
            let y = (baseline + 1.0).round();
            let mut tree = self.ui.tree_mut();
            tree.set_bounds(underline, Rect::new(0.0, y, f64::from(shaped.width), y + 1.0));
            tree.style_mut(underline).background = Some(color);
        }
        self.link_styles.borrow_mut().insert(link, LinkStyle { color, underline: true });
    }

    // ----- layout -----

    /// Escreve `text` como linhas filhas de `node` e devolve a altura (`linhas * line_height`).
    fn render_lines(&self, node: NodeId, text: &str, block: LineBlock<'_>) -> f64 {
        let LineBlock { style, color, line_height, width, pre_wrap, centered } = block;
        let old = self.ui.tree().children_with_class(node, "dialog-line");
        for line in old {
            self.ui.remove(line);
        }
        let lines = wrap_lines(text, style, width, pre_wrap);
        let mut tree = self.ui.tree_mut();
        for (index, line) in lines.iter().enumerate() {
            let line_width = measure_text(line, style);
            let x = if centered { (width - line_width) / 2.0 } else { 0.0 };
            let y = index as f64 * line_height;
            let line_node = create_el(&mut tree, node, Role::Presentation, "dialog-line");
            tree.set_bounds(line_node, Rect::new(x, y, x + line_width, y + line_height));
            tree.style_mut(line_node).content = Content::Text { text: line.clone(), style: style.clone(), color, line_height };
        }
        lines.len() as f64 * line_height
    }

    /// Os botões e o tamanho natural de cada um (o `fit-content` e as margens do `dialog.css`).
    fn measure_buttons(&self) -> Vec<ButtonSlot> {
        let vertical = self.vertical;
        self.dialog_buttons
            .borrow()
            .iter()
            .map(|button| match button {
                DialogButton::Text(b) => {
                    b.set_height(None);
                    let width = b.min_content_width();
                    b.set_width(Some(width));
                    ButtonSlot { button: button.clone(), width, margin_x: if vertical { 0.0 } else { 4.0 }, margin_y: 4.0, height: b.size().1 }
                }
                DialogButton::Description(b) => {
                    let description = measure_text(&b.description_text(), &ui_font(11.0, 400)) + 40.0;
                    let width = b.button().min_content_width().max(description);
                    b.layout(Some((0.0, 0.0, width + 10.0)));
                    let height = self.ui.tree().bounds(b.element()).height();
                    ButtonSlot { button: button.clone(), width, margin_x: 5.0, margin_y: 4.0, height }
                }
                DialogButton::Dropdown(b) => {
                    b.set_width(None);
                    let r = self.ui.tree().bounds(b.element());
                    let (margin_x, margin_y) = if vertical { (0.0, 0.0) } else { (5.0, 4.0) };
                    ButtonSlot { button: button.clone(), width: r.width(), margin_x, margin_y, height: r.height() }
                }
            })
            .collect()
    }

    /// Posiciona os botões na `.dialog-buttons` de `container_width`, com a altura de linha `line_height`.
    fn place_buttons(&self, slots: &[ButtonSlot], container_width: f64, line_height: f64) {
        let vertical = self.vertical;
        let mut cursor_y = 0.0;
        // `flex-end`: os botões encostam à direita; se não cabem, encolhem (`flex-shrink: 1`, `min-width: 0`).
        let widths: Vec<f64> = if vertical {
            slots.iter().map(|s| container_width - 2.0 * s.margin_x).collect()
        } else {
            let items: Vec<FlexItem> = slots.iter().map(|s| FlexItem { base: s.outer_width(), pad_border: 0.0, grow: 0.0, shrink: 1.0, min: 0.0 }).collect();
            resolve_flexible_lengths(&items, container_width).iter().zip(slots).map(|(outer, s)| outer - 2.0 * s.margin_x).collect()
        };
        let total: f64 = widths.iter().zip(slots).map(|(w, s)| w + 2.0 * s.margin_x).sum();
        let mut cursor_x = if vertical { 0.0 } else { container_width - total };
        for (slot, width) in slots.iter().zip(widths) {
            let (x, y) = (cursor_x + slot.margin_x, if vertical { cursor_y + slot.margin_y } else { slot.margin_y });
            match &slot.button {
                DialogButton::Text(b) => {
                    b.set_width(Some(width));
                    b.set_height((!vertical).then_some(line_height - 2.0 * slot.margin_y));
                    b.set_position(x, y);
                }
                DialogButton::Description(b) => b.layout(Some((x - slot.margin_x, y - slot.margin_y, width + 2.0 * slot.margin_x))),
                DialogButton::Dropdown(b) => {
                    if vertical {
                        b.set_width(Some(width));
                    }
                    b.set_position(x, y);
                }
            }
            if vertical {
                cursor_y += slot.outer_height();
            } else {
                cursor_x += width + 2.0 * slot.margin_x;
            }
        }
    }

    /// Resolve o `display: flex` do CSS: tamanho e posição de cada nó da caixa. Chame de novo quando a
    /// janela mudar de tamanho ou o conteúdo mudar.
    pub fn layout(&self) {
        if self.disposed.get() {
            return;
        }
        let styles = &self.options.dialog_styles;
        let (vw, vh) = window_size(&self.ui);
        let vertical = self.vertical;
        let border = if styles.dialog_border.is_some() { 1.0 } else { 0.0 };
        let fg = styles.dialog_foreground.unwrap_or(INHERITED_FOREGROUND);
        let (base_style, message_style) = (ui_font(13.0, 400), ui_font(14.0, 600));
        let detail_text = self.detail_text.borrow().clone();
        let has_icon = self.icon_glyph().is_some() && self.ui.tree().has_class(self.icon_element, "codicon");
        let icon_size = if !has_icon { 0.0 } else if vertical { 64.0 } else { 24.0 };

        // A largura: `width: min-content` entre `min-width` e `max-width`.
        let slots = self.measure_buttons();
        let buttons_min = match (slots.is_empty(), vertical) {
            (true, _) => 0.0,
            (false, true) => slots.iter().map(ButtonSlot::outer_width).fold(0.0, f64::max) + 10.0,
            (false, false) => 67.0 + slots.iter().map(ButtonSlot::outer_width).sum::<f64>(),
        };
        let longest = longest_word(&self.message, &message_style)
            .max(detail_text.as_deref().map_or(0.0, |d| longest_word(d, &base_style)))
            .max(self.options.checkbox_label.as_deref().map_or(0.0, |l| longest_word(l, &base_style) + CHECKBOX_COLUMN));
        let message_min = ROW_PADDING_LEFT + ROW_PADDING_RIGHT + longest + if vertical { 0.0 } else { icon_size + 12.0 };
        let css_min: f64 = if vertical { 350.0 } else { 480.0 };
        let width = css_min.max(buttons_min.max(message_min).min((0.9 * vw).floor()));
        let text_width = if vertical { width - ROW_PADDING_LEFT - ROW_PADDING_RIGHT } else { width - ROW_PADDING_LEFT - ROW_PADDING_RIGHT - icon_size - 12.0 };
        let text_x = if vertical { 0.0 } else { 12.0 };

        // O contêiner da mensagem: mensagem, detalhe, corpo, campos e caixa de seleção.
        let mut y = 0.0;
        if let (Some(message_element), Some(text_element)) = (self.message_element, self.message_text_element) {
            let lines = self.render_lines(text_element, &self.message, LineBlock { style: &message_style, color: fg, line_height: BASE_LINE_HEIGHT, width: text_width, pre_wrap: false, centered: vertical });
            let block = lines.max(MESSAGE_MIN_HEIGHT);
            let mut tree = self.ui.tree_mut();
            tree.set_bounds(message_element, Rect::new(text_x, y, text_x + text_width, y + block));
            tree.set_bounds(text_element, Rect::new(0.0, (block - lines) / 2.0, text_width, (block - lines) / 2.0 + lines));
            y += block + 4.0;
        }
        if self.ui.tree().style(self.message_detail_element).visible {
            let height = match (&detail_text, self.options.detail_element) {
                (Some(text), _) => self.render_lines(self.message_detail_element, text, LineBlock { style: &base_style, color: fg, line_height: DETAIL_LINE_HEIGHT, width: text_width, pre_wrap: true, centered: vertical }),
                (None, Some(detail_element)) => content_extent(&self.ui, detail_element),
                (None, None) => 0.0,
            };
            self.ui.tree_mut().set_bounds(self.message_detail_element, Rect::new(text_x, y, text_x + text_width, y + height));
            y += height;
        }
        if let Some(body) = self.message_body_element {
            let height = content_extent(&self.ui, body);
            self.ui.tree_mut().set_bounds(body, Rect::new(text_x, y, text_x + text_width, y + height));
            y += height;
        }
        for (row, input) in self.input_rows.iter().zip(&self.inputs) {
            self.ui.tree_mut().set_bounds(*row, Rect::new(text_x, y, text_x + text_width, y + FIELD_PADDING_TOP));
            self.ui.tree_mut().set_bounds(input.element(), Rect::new(0.0, FIELD_PADDING_TOP, text_width, FIELD_PADDING_TOP + 24.0));
            input.layout();
            let height = FIELD_PADDING_TOP + input.height();
            self.ui.tree_mut().set_bounds(*row, Rect::new(text_x, y, text_x + text_width, y + height));
            y += height;
        }
        if let (Some(row), Some(message_node), Some(checkbox), Some(label)) = (self.checkbox_row, self.checkbox_message_element, &self.checkbox, &self.options.checkbox_label) {
            let lines = self.render_lines(message_node, label, LineBlock { style: &base_style, color: fg, line_height: BASE_LINE_HEIGHT, width: text_width - CHECKBOX_COLUMN, pre_wrap: false, centered: false });
            let height = FIELD_PADDING_TOP + lines.max(18.0);
            checkbox.set_position(0.0, FIELD_PADDING_TOP);
            let mut tree = self.ui.tree_mut();
            tree.set_bounds(row, Rect::new(text_x, y, text_x + text_width, y + height));
            tree.set_bounds(message_node, Rect::new(CHECKBOX_COLUMN, FIELD_PADDING_TOP, text_width, height));
            y += height;
        }
        let container_height = y;

        // As linhas da caixa (`column-reverse`: barra de ações, mensagem, botões, rodapé).
        let message_row_natural = if vertical { icon_size + container_height } else { icon_size.max(container_height) };
        let line_height = slots.iter().map(ButtonSlot::outer_height).fold(0.0, f64::max);
        let buttons_row_height = 20.0 + line_height;
        let footer_content_height = self.footer_content.map_or(0.0, |f| content_extent(&self.ui, f));
        let footer_row_height = if self.footer_row.is_some() { 20.0 + footer_content_height } else { 0.0 };
        let max_content = (0.9 * vh).floor();
        let mut message_row_height = message_row_natural;
        let mut footer_extra = 0.0;
        let content = |message: f64, footer: f64| TOOLBAR_ROW_HEIGHT + message + buttons_row_height + footer;
        if vh > 0.0 && content(message_row_height, footer_row_height) > max_content {
            message_row_height = (max_content - TOOLBAR_ROW_HEIGHT - buttons_row_height - footer_row_height).max(0.0);
        }
        let total = content(message_row_height, footer_row_height);
        if total < 75.0 {
            // `min-height: 75px`: a folga vai para as linhas que crescem (`flex-grow: 1`).
            let extra = 75.0 - total;
            let share = if self.footer_row.is_some() { extra / 2.0 } else { extra };
            message_row_height += share;
            footer_extra = if self.footer_row.is_some() { share } else { 0.0 };
        }
        let footer_row_height = if self.footer_row.is_some() { footer_row_height + footer_extra } else { 0.0 };
        let content_height = content(message_row_height, footer_row_height);

        // A caixa, centrada na janela (`justify-content` e `align-items: center` do bloco modal).
        let box_width = width + 2.0 * BOX_PADDING + 2.0 * border;
        let box_height = content_height + 2.0 * BOX_PADDING + 2.0 * border;
        let (box_x, box_y) = (((vw - box_width) / 2.0).round(), ((vh - box_height) / 2.0).round());
        let origin = self.ui.tree().absolute_bounds(self.container);
        let (ox, oy) = (border + BOX_PADDING, border + BOX_PADDING);
        let mut tree = self.ui.tree_mut();
        if let Some(modal) = self.modal_element.get() {
            tree.set_bounds(modal, Rect::new(-origin.x0, -origin.y0, vw - origin.x0, vh - origin.y0));
        }
        tree.set_bounds(self.shadow_element, Rect::new(box_x, box_y, box_x + box_width, box_y + box_height));
        tree.set_bounds(self.element, Rect::new(0.0, 0.0, box_width, box_height));
        for layer in &self.shadow_layers {
            let s = layer.spread;
            tree.set_bounds(layer.node, Rect::new(-s, -s, box_width + s, box_height + s));
            tree.style_mut(layer.node).radius = CORNER_RADIUS_X_LARGE + s;
        }
        tree.set_bounds(self.toolbar_row, Rect::new(ox, oy, ox + width, oy + TOOLBAR_ROW_HEIGHT));
        tree.set_bounds(self.toolbar_container, Rect::new(0.0, 0.0, width, 20.0));
        let message_y = oy + TOOLBAR_ROW_HEIGHT;
        tree.set_bounds(self.message_row, Rect::new(ox, message_y, ox + width, message_y + message_row_height));
        if vertical {
            let icon_x = ROW_PADDING_LEFT + (width - ROW_PADDING_LEFT - ROW_PADDING_RIGHT - icon_size) / 2.0;
            tree.set_bounds(self.icon_element, Rect::new(icon_x, 0.0, icon_x + icon_size, icon_size));
            tree.set_bounds(self.message_container, Rect::new(ROW_PADDING_LEFT, icon_size, width - ROW_PADDING_RIGHT, (message_row_height).max(icon_size)));
        } else {
            tree.set_bounds(self.icon_element, Rect::new(ROW_PADDING_LEFT, 0.0, ROW_PADDING_LEFT + icon_size, icon_size));
            tree.set_bounds(self.message_container, Rect::new(ROW_PADDING_LEFT + icon_size, 0.0, width - ROW_PADDING_RIGHT, message_row_height));
        }
        let buttons_y = message_y + message_row_height;
        tree.set_bounds(self.buttons_row, Rect::new(ox, buttons_y, ox + width, buttons_y + buttons_row_height));
        let (buttons_x, buttons_width) = if vertical { (5.0, width - 10.0) } else { (67.0, width - 67.0) };
        tree.set_bounds(self.buttons_container, Rect::new(buttons_x, 20.0, buttons_x + buttons_width, 20.0 + line_height));
        tree.style_mut(self.buttons_container).clip = !vertical;
        if let (Some(row), Some(content)) = (self.footer_row, self.footer_content) {
            let footer_y = buttons_y + buttons_row_height + 20.0;
            tree.set_bounds(row, Rect::new(ox, footer_y, ox + width, footer_y + footer_row_height - 20.0));
            let footer_bounds = tree.bounds(content);
            tree.set_bounds(content, Rect::new(ROW_PADDING_LEFT, 0.0, width - ROW_PADDING_RIGHT, footer_bounds.height().max(footer_content_height)));
        }
        drop(tree);
        self.place_buttons(&slots, buttons_width, line_height);
        self.layout_toolbar(width);
    }

    /// `.dialog-toolbar-row .actions-container { justify-content: flex-end }`.
    fn layout_toolbar(&self, width: f64) {
        let Some(action_bar) = self.action_bar.borrow().clone() else { return };
        let total: f64 = (0..action_bar.length()).map(|i| action_bar.get_width(i)).sum();
        let list = action_bar.actions_list();
        let mut tree = self.ui.tree_mut();
        tree.set_fill(list, false, false);
        tree.set_bounds(list, Rect::new(width - total, 0.0, width, 20.0));
    }
}

impl IDisposable for Dialog {
    fn dispose(&self) {
        if self.disposed.replace(true) {
            return;
        }
        self.base.dispose();
        if let Some(modal) = self.modal_element.take() {
            self.ui.remove(modal);
        }
        if let Some(node) = self.focus_to_return.take() {
            let alive_in_body = {
                let tree = self.ui.tree();
                tree.is_alive(node) && tree.is_ancestor(tree.root(), node)
            };
            if alive_in_body {
                self.ui.focus(node);
            }
        }
    }

    fn constructor_name(&self) -> &'static str {
        "Dialog"
    }
}
