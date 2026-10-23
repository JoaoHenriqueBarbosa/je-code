//! Porte de `src/vs/base/browser/ui/selectBox/selectBox.ts` (e do `selectBox.css`): os contratos da caixa de
//! seleção (`ISelectBoxDelegate`, `ISelectOptionItem`, `ISelectBoxOptions`, `ISelectData`, `ISelectBoxStyles`
//! com `unthemedSelectBoxStyles` e `defaultSelectBoxStyles`) e o [`SelectBox`].
//!
//! # Uma implementação só
//!
//! No original `SelectBox` escolhe entre dois delegados: o `SelectBoxNative` (um `<select>` do sistema, o
//! padrão no macOS) e o `SelectBoxList` (a lista suspensa desenhada numa visão de contexto). O `<select>`
//! nativo não existe aqui (a UI é toda desenhada, não há controle do sistema), então os dois viram o mesmo: a
//! versão customizada, [`SelectBoxList`](crate::select_box_custom::SelectBoxList), com a API idêntica. Por
//! isso [`SelectBox`] é um sinônimo dela: um `SelectBox` cujo corpo só repassasse cada chamada ao delegado
//! seria função de repasse, e o `SelectBoxNative` duplicaria o que a customizada já faz. O que o nativo tinha
//! de diferente (`useCustomDrawn` só decidia entre os dois; `optionsAsChildren`, `anchorPosition` e
//! `maxVisibleOptions` eram ignorados por ele) é portanto sempre o comportamento da customizada.
//!
//! # Estilos
//!
//! `ISelectBoxStyles extends IListStyles`. A `IListStyles` inteira pertence ao `listWidget.ts` (o
//! `DefaultStyleController`, que gera CSS, não existe aqui), então [`ISelectBoxStyles`] carrega só os campos
//! dela que o `SelectBoxList` lê (`listFocusBackground`, `listFocusForeground`, `listFocusOutline`,
//! `listHoverBackground`, `listHoverForeground`, `listHoverOutline`). Os demais (`listBackground`,
//! `listActiveSelection*`, ...) nunca são lidos pela caixa de seleção. As regras do CSS que usam
//! `var(--vscode-...)` direto (`descriptionForeground` e `menu-separatorBackground` do separador, o raio
//! pequeno e o grande) entram como campos extras do mesmo tipo.

use je_base::event::Event;
use je_base::lifecycle::IDisposable;
use je_base::layout::AnchorPosition;
use je_render::Rgba;
use je_theme::ColorTheme;
use je_ui::NodeId;

pub use crate::select_box_custom::SelectBoxList;

/// `SelectBox`: ver o doc do módulo, é a única implementação (a customizada).
pub type SelectBox = SelectBoxList;

/// O `--vscode-cornerRadius-small` (`baseSizes.ts`: 4px em todos os temas): o raio do `.monaco-select-box`.
pub const CORNER_RADIUS_SMALL: f64 = 4.0;
/// O `--vscode-cornerRadius-large` (8px): o raio do `.monaco-select-box-dropdown-container`.
pub const CORNER_RADIUS_LARGE: f64 = 8.0;

/// `ISelectOptionItem`. `descriptionMarkdownActionHandler` não existe (não há renderizador de markdown, ver
/// [`SelectBoxList`]); `description_is_markdown` é mantido e a descrição aparece como texto.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ISelectOptionItem {
    pub text: String,
    pub detail: Option<String>,
    pub decorator_right: Option<String>,
    pub description: Option<String>,
    pub description_is_markdown: bool,
    pub is_disabled: bool,
    pub is_separator: bool,
}

impl ISelectOptionItem {
    /// Uma opção só com o texto.
    pub fn new(text: &str) -> Self {
        Self { text: text.to_owned(), ..Self::default() }
    }
}

/// `SeparatorSelectOption`.
pub fn separator_select_option() -> ISelectOptionItem {
    ISelectOptionItem { text: "\u{2500}".repeat(9), is_disabled: true, is_separator: true, ..ISelectOptionItem::default() }
}

/// `ISelectBoxOptions`. `ariaDescription` e `optionsAsChildren` ficam: o primeiro sem efeito (o `je-ui` não tem
/// `aria-description`), o segundo troca o contêiner da visão de contexto.
#[derive(Clone, Debug, Default)]
pub struct ISelectBoxOptions {
    /// Só decidia entre o nativo e o customizado: sem efeito aqui (ver o doc do módulo).
    pub use_custom_drawn: bool,
    pub aria_label: Option<String>,
    pub aria_description: Option<String>,
    pub min_bottom_margin: Option<f64>,
    /// A posição preferida da lista (por padrão abaixo; inverte quando falta espaço).
    pub anchor_position: Option<AnchorPosition>,
    /// O máximo de opções visíveis antes de rolar.
    pub max_visible_options: Option<usize>,
    pub options_as_children: bool,
    /// Visões de contexto de camada maior ficam por cima.
    pub context_view_layer: Option<i32>,
    /// Esconde as opções desabilitadas da lista suspensa.
    pub hide_disabled_options: bool,
    /// Mostra a descrição das opções numa dica à direita em vez do painel de detalhes.
    pub show_option_description_hovers: bool,
}

/// `ISelectData`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ISelectData {
    pub selected: String,
    pub index: usize,
}

/// `ISelectBoxStyles`: ver o doc do módulo. `None` é o `undefined` do original.
#[derive(Clone, Debug, PartialEq)]
pub struct ISelectBoxStyles {
    pub select_background: Option<Rgba>,
    pub select_list_background: Option<Rgba>,
    pub select_foreground: Option<Rgba>,
    pub decorator_right_foreground: Option<Rgba>,
    pub select_border: Option<Rgba>,
    pub select_list_border: Option<Rgba>,
    pub focus_border: Option<Rgba>,
    // Os campos de `IListStyles` que a caixa de seleção lê.
    pub list_focus_background: Option<Rgba>,
    pub list_focus_foreground: Option<Rgba>,
    pub list_focus_outline: Option<Rgba>,
    pub list_hover_background: Option<Rgba>,
    pub list_hover_foreground: Option<Rgba>,
    pub list_hover_outline: Option<Rgba>,
    // As variáveis de CSS que as regras do `selectBoxCustom.css` leem direto.
    /// `--vscode-descriptionForeground`: a cor do rótulo de um separador.
    pub description_foreground: Option<Rgba>,
    /// `--vscode-menu-separatorBackground`: a linha do separador.
    pub menu_separator_background: Option<Rgba>,
}

fn hex(value: u32) -> Rgba {
    Rgba::new((value >> 16) as u8, (value >> 8) as u8, value as u8, 255)
}

/// `unthemedSelectBoxStyles` (com os campos de `unthemedListStyles` que a caixa lê).
pub fn unthemed_select_box_styles() -> ISelectBoxStyles {
    ISelectBoxStyles {
        select_background: Some(hex(0x3C3C3C)),
        select_foreground: Some(hex(0xF0F0F0)),
        select_border: Some(hex(0x3C3C3C)),
        decorator_right_foreground: None,
        select_list_background: None,
        select_list_border: None,
        focus_border: None,
        list_focus_background: Some(hex(0x7FB0D0)),
        list_focus_foreground: None,
        list_focus_outline: None,
        list_hover_background: Some(hex(0x2A2D2E)),
        list_hover_foreground: None,
        list_hover_outline: None,
        description_foreground: None,
        menu_separator_background: None,
    }
}

/// `defaultSelectBoxStyles` de `defaultStyles.ts`, com as chaves de tema resolvidas.
pub fn default_select_box_styles(theme: &ColorTheme) -> ISelectBoxStyles {
    let color = |id: &str| theme.get_color(id);
    ISelectBoxStyles {
        select_background: color("dropdown.background"),
        select_list_background: color("dropdown.listBackground"),
        select_foreground: color("dropdown.foreground"),
        decorator_right_foreground: color("pickerGroup.foreground"),
        select_border: color("dropdown.border"),
        focus_border: color("focusBorder"),
        list_focus_background: color("quickInputList.focusBackground"),
        list_focus_foreground: color("quickInputList.focusForeground"),
        // `asCssVariableWithDefault(activeContrastBorder, Color.transparent)`.
        list_focus_outline: color("contrastActiveBorder").or(Some(Rgba::TRANSPARENT)),
        list_hover_background: color("list.hoverBackground"),
        list_hover_foreground: color("list.hoverForeground"),
        list_hover_outline: color("contrastActiveBorder"),
        select_list_border: color("editorWidget.border"),
        description_foreground: color("descriptionForeground"),
        menu_separator_background: color("menu.separatorBackground"),
    }
}

/// `ISelectBoxDelegate`.
pub trait ISelectBoxDelegate: IDisposable {
    fn on_did_select(&self) -> Event<ISelectData>;
    fn set_options(&self, options: Vec<ISelectOptionItem>, selected: Option<usize>);
    fn select(&self, index: usize);
    fn set_aria_label(&self, label: &str);
    fn focus(&self);
    fn blur(&self);
    fn set_focusable(&self, focus: bool);
    fn set_enabled(&self, enabled: bool);
    /// `render(container)` (o `Widget`).
    fn render(&self, container: NodeId);
}
