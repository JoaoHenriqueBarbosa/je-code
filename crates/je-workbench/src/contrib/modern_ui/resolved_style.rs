//! O estilo resolvido do Modern UI: o que o `getComputedStyle` devolveria ao original depois de aplicar as regras de
//! `workbench/browser/media/floatingPanels.css` e de `workbench/contrib/modernUI/browser/media/*.css` sobre as variáveis
//! `--vscode-*` que `generateColorThemeCSS` publica a partir do tema.
//!
//! Divergências:
//! - **Sem cascata de CSS.** Cada propriedade é uma função das classes que os seletores testam (as da raiz do workbench e
//!   as da parte) e do tema. A precedência (especificidade, `!important`, ordem) já está resolvida na ordem dos ramos,
//!   comentada com o `.css` e a linha de onde vem. O widget nativo pinta o valor devolvido; o teste o consulta onde o
//!   original lê `getComputedStyle`.
//! - **As variáveis `--vscode-*` são o tema.** `var(--vscode-surface-border)` é `theme.resolve_color("surface.border")`,
//!   que já inclui `workbench.colorCustomizations` e o padrão do registro, como o `generateColorThemeCSS` emite
//!   (`theme.getColor(id, true)`). Uma variável sem cor (id não registrado) cai no próximo `var()` da cadeia.
//! - **Cores.** Saem como [`Color`]; o texto `rgb(...)` do computado é [`css_rgb`], e o valor de uma propriedade
//!   personalizada (`getPropertyValue`) é o `to_string()` da cor, que é o texto que o `generateColorThemeCSS` escreve.

use std::collections::{BTreeMap, BTreeSet};

use je_theme::ColorTheme;
use je_theme::color::Color;
use je_theme::color::format::css::format_rgb;
use je_theme::registry::{as_css_variable_name, color_registry};
use je_theme::size_utils::get_size_registry;
use je_theme::sizes::base_sizes::{CODICON_FONT_SIZE, CORNER_RADIUS_LARGE, CORNER_RADIUS_SMALL, FONT_WEIGHT_REGULAR, SPACING_NONE, STROKE_THICKNESS, SPACING_SIZE20, SPACING_SIZE240, SPACING_SIZE320, SPACING_SIZE360, SPACING_SIZE40, SPACING_SIZE60, SPACING_SIZE80};

// Os traços de borda dos cartões compactos (`floatingPanels.css`, linhas 508 a 522).
mod card_border_layers;
// As abas conectadas do editor (`connectedEditorTabs.css` e as regras de `tabs.css` que ela cobre).
mod connected_tabs;

pub use card_border_layers::{BorderLayer, CardEdge, GradientKind};
pub use connected_tabs::{
    ConnectedGroupFrame, EDITOR_TABS_SCROLLBAR_Z_INDEX, FillShoulder, FillShoulderBox, StickyTabsBackgroundBox, TAB_CONNECTED_EDGE_PSEUDO_CONTENT, TAB_FILL_BACKGROUND_CLIP, TabBorderContainer, TabBorderIndicator, TabElement,
    TabRowPlacement, editor_group_child_display, tab_fill_bottom_left_radius, tab_fill_top_right_radius,
};

/// O `classList` de um elemento: só o que os seletores consultam.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ClassList(BTreeSet<String>);

impl ClassList {
    /// O `className` com as classes separadas por espaço.
    pub fn new(class_name: &str) -> Self {
        Self(class_name.split_whitespace().map(str::to_owned).collect())
    }

    pub fn contains(&self, class_name: &str) -> bool {
        self.0.contains(class_name)
    }

    fn any(&self, class_names: &[&str]) -> bool {
        class_names.iter().any(|class_name| self.contains(class_name))
    }
}

/// O texto `rgb(r, g, b)` (ou `rgba(...)`) que o `getComputedStyle` devolve para uma cor.
pub fn css_rgb(color: &Color) -> String {
    format_rgb(color)
}

/// As quatro bordas de uma caixa (`border-top-color` e as outras), na ordem do shorthand `border-color`.
#[derive(Clone, Debug, PartialEq)]
pub struct BorderColors {
    pub top: Color,
    pub right: Color,
    pub bottom: Color,
    pub left: Color,
}

impl BorderColors {
    fn uniform(color: Color) -> Self {
        Self { top: color.clone(), right: color.clone(), bottom: color.clone(), left: color }
    }

    /// O `border-color` calculado: o shorthand na forma mais curta (uma, duas, três ou quatro cores).
    pub fn to_css(&self) -> String {
        let (top, right, bottom, left) = (css_rgb(&self.top), css_rgb(&self.right), css_rgb(&self.bottom), css_rgb(&self.left));
        if top == right && right == bottom && bottom == left {
            top
        } else if top == bottom && right == left {
            format!("{top} {right}")
        } else if right == left {
            format!("{top} {right} {bottom}")
        } else {
            format!("{top} {right} {bottom} {left}")
        }
    }
}

/// Onde uma linha de lista (`.monaco-list-row`) está, para o recuo horizontal de `padding.css`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ListRowHost {
    /// `.monaco-pane-view .pane-body`; `chat_view` é `.chat-viewpane`, que fica de fora da regra.
    PaneBody { chat_view: bool },
    /// `.multi-diff-editor-with-file-list-files`.
    MultiDiffFileList,
    /// Qualquer outra lista: sem recuo.
    Other,
}

/// O estado de uma aba do editor que as regras de `tabs.css` testam.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct EditorTabState {
    /// `.tab.active`.
    pub active: bool,
    /// `.tab.selected`.
    pub selected: bool,
    /// `:hover` na aba.
    pub hovered: bool,
    /// `.editor-group-container.active` (o grupo com o foco).
    pub group_active: bool,
    /// `:focus-within` nas ações da aba.
    pub actions_focus_within: bool,
    /// `.tab.close-action-off`.
    pub close_action_off: bool,
    /// `.title.tab-actions-reserve-space`.
    pub actions_reserve_space: bool,
}

/// Uma ação (`.action-item`) da barra de abas de um painel composto ou das configurações.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ActionTabState {
    /// `.checked`.
    pub checked: bool,
    /// `.icon` (a barra de ícones do painel).
    pub icon: bool,
    /// `:hover`.
    pub hovered: bool,
}

/// Resolve o estilo do Modern UI para um tema e as classes da raiz do workbench (`.monaco-workbench`).
pub struct ModernUiStyle<'a> {
    theme: &'a ColorTheme,
    root: ClassList,
    notification_insets: NotificationInsets,
    /// `--activity-bar-width` e `--activity-bar-action-height`, que o shell define na raiz.
    activity_bar_metrics: ActivityBarMetrics,
    /// Só as cores que o tema define de fato existem como `--vscode-*`: o DOM de um teste sem o CSS gerado pelo tema
    /// (`generateColorThemeCSS`) não carrega os padrões registrados.
    explicit_colors_only: bool,
    /// `--activity-bar-action-gap`, que `activitybarPart.ts` publica na raiz (`None` é a variável indefinida, `0px`).
    activity_bar_action_gap: Option<f64>,
    /// `--window-zoom-factor`, que `window.ts` publica na raiz (`var(--window-zoom-factor, 1)`: indefinida vale `1`).
    window_zoom_factor: f64,
    /// As propriedades personalizadas de cor que o DOM escreve direto na raiz (`root.style.setProperty('--modern-ui-...')`),
    /// que vencem as que os participantes de tema declaram na folha de estilo.
    root_variables: BTreeMap<String, Color>,
}

impl<'a> ModernUiStyle<'a> {
    pub fn new(theme: &'a ColorTheme, root_class_name: &str) -> Self {
        Self {
            theme,
            root: ClassList::new(root_class_name),
            notification_insets: NotificationInsets::default(),
            activity_bar_metrics: ActivityBarMetrics::default(),
            explicit_colors_only: false,
            activity_bar_action_gap: None,
            window_zoom_factor: 1.0,
            root_variables: BTreeMap::new(),
        }
    }

    /// Define uma propriedade personalizada de cor na raiz (`root.style.setProperty(name, color)`), por exemplo
    /// `--modern-ui-editor-tab-custom-active-border-top`.
    pub fn set_root_variable(&mut self, name: &str, color: Color) {
        self.root_variables.insert(name.to_owned(), color);
    }

    /// Remove uma propriedade personalizada da raiz (`root.style.removeProperty(name)`).
    pub fn remove_root_variable(&mut self, name: &str) {
        self.root_variables.remove(name);
    }

    /// Define `--window-zoom-factor` (`root.style.setProperty`).
    pub fn set_window_zoom_factor(&mut self, zoom_factor: f64) {
        self.window_zoom_factor = zoom_factor;
    }

    /// Um DOM em que só as variáveis `--vscode-*` definidas pelo tema existem, sem os padrões registrados.
    pub fn with_explicit_colors_only(mut self) -> Self {
        self.explicit_colors_only = true;
        self
    }

    /// Troca as classes da raiz (`root.className = ...`).
    pub fn set_root_class_name(&mut self, root_class_name: &str) {
        self.root = ClassList::new(root_class_name);
    }

    fn has(&self, class_name: &str) -> bool {
        self.root.contains(class_name)
    }

    fn high_contrast(&self) -> bool {
        self.root.any(&["hc-black", "hc-light"])
    }

    /// `.floating-panels.modern-ui-compact`: os cartões se encontram de borda a borda.
    fn compact(&self) -> bool {
        self.has("floating-panels") && self.has("modern-ui-compact")
    }

    /// `var(--vscode-<id>)`.
    fn color(&self, id: &str) -> Option<Color> {
        self.theme.resolve_color(id, !self.explicit_colors_only)
    }

    /// `var(--vscode-a, var(--vscode-b, ...))`: a primeira cor definida da cadeia.
    fn first(&self, ids: &[&str]) -> Option<Color> {
        ids.iter().find_map(|id| self.color(id))
    }

    /// A mesma cadeia com `transparent` como último recurso.
    fn chain(&self, ids: &[&str]) -> Color {
        self.first(ids).unwrap_or_else(Color::transparent_color)
    }

    /// `--modern-ui-floating-card-border-color`: definida só no compacto (`transparent`).
    fn floating_card_border_color(&self) -> Option<Color> {
        self.compact().then(Color::transparent_color)
    }

    /// `var(--modern-ui-floating-card-border-color, var(--vscode-a, ...))`.
    fn card_border_or(&self, ids: &[&str]) -> Color {
        self.floating_card_border_color().unwrap_or_else(|| self.chain(ids))
    }

    /// O valor de uma variável CSS, `--vscode-*` (a cor do tema) ou `--modern-ui-*` das abas (`tabs.css`, linhas 24 a 54).
    /// `None` é a variável indefinida.
    pub fn variable(&self, name: &str) -> Option<Color> {
        if let Some(color) = self.root_variables.get(name) {
            return Some(color.clone());
        }
        if let Some(id) = modern_ui_tab_variable_color_id(name) {
            return self.has("modern-ui-tabs").then(|| self.color(id)).flatten();
        }
        color_registry().get_colors().iter().find(|contribution| as_css_variable_name(contribution.id) == name).and_then(|contribution| self.color(contribution.id))
    }

    /// `--modern-ui-floating-card-stroke-color` de uma parte (`getPropertyValue`): o traço do cartão compacto, que o
    /// `background-image` de `floatingPanels.css` (linhas 508 a 521) pinta nas bordas. `None` fora do compacto.
    pub fn floating_card_stroke_color(&self, part: &ClassList) -> Option<Color> {
        if !self.compact() {
            return None;
        }
        // connectedEditorTabs.css, linhas 51 a 53: o editor com várias abas em alto contraste.
        if part.contains("editor") && self.connected_high_contrast_editor(part) {
            return self.first(&["modernPanel.border", "contrastBorder"]);
        }
        Some(if part.contains("activitybar") {
            // floatingPanels.css, linha 486.
            self.chain(&["modernActivityBar.border", "surface.border", "widget.border"])
        } else if part.contains("panel") {
            // floatingPanels.css, linha 482.
            self.chain(&["modernPanel.border", "surface.border", "widget.border"])
        } else if part.any(&["sidebar", "auxiliarybar"]) {
            // floatingPanels.css, linha 476.
            self.chain(&["surface.border", "widget.border"])
        } else {
            // editorBorder.css, linha 53.
            self.chain(&["editor.border", "surface.border", "widget.border"])
        })
    }

    /// `.modern-ui-tabs.modern-ui-connected-editor-tabs.modern-ui.monaco-workbench:is(.hc-black, .hc-light)
    /// .part.editor.editor-tabs-multiple:not(.modal-editor-part)` (connectedEditorTabs.css, linha 50).
    fn connected_high_contrast_editor(&self, part: &ClassList) -> bool {
        self.has("modern-ui-tabs")
            && self.has("modern-ui-connected-editor-tabs")
            && self.has("modern-ui")
            && self.high_contrast()
            && part.contains("editor-tabs-multiple")
            && !part.contains("modal-editor-part")
    }

    /// `border-color` do `.part.editor`. `None` quando nenhuma regra de borda do Modern UI alcança a parte.
    pub fn editor_border_color(&self, part: &ClassList) -> Option<Color> {
        if !self.has("modern-ui") {
            return None;
        }
        // `--modern-ui-editor-border-color` do editor com várias abas em alto contraste (connectedEditorTabs.css, linhas 50 a
        // 53, com a especificidade acima de `editorBorder.css`, linha 26) e, com `.floating-panels`, a `!important` da linha
        // 49 do `editorBorder.css` lê a mesma variável: a moldura do grupo é quem desenha o traço, não a borda da parte.
        if self.connected_high_contrast_editor(part) {
            return Some(self.floating_card_border_color().unwrap_or_else(|| self.first(&["modernPanel.border", "contrastBorder"]).unwrap_or_else(Color::transparent_color)));
        }
        if !self.has("floating-panels") {
            // editorBorder.css, linha 27: só o editor principal; `color-mix(in srgb, foreground 12%, transparent)` é o
            // primeiro plano com 12% do seu alfa.
            if part.contains("modal-editor-part") {
                return None;
            }
            return Some(self.first(&["editorGroup.border"]).unwrap_or_else(|| {
                let foreground = self.color("foreground").unwrap_or_else(Color::transparent_color);
                foreground.transparent(0.12)
            }));
        }
        // `--modern-ui-editor-border-color` (floatingPanels.css linha 36) vence a cadeia de `editorBorder.css` (linha 49,
        // `!important`).
        if self.compact() {
            return Some(Color::transparent_color());
        }
        Some(self.chain(&["editor.border", "surface.border", "widget.border"]))
    }

    /// `border-width` do `.part.editor`: o traço de `strokeThickness` do editor principal com o Modern UI (editorBorder.css,
    /// linha 27) e o `1px` fixo da parte modal (modalEditorPart.css, linha 40). Sem nenhuma das duas regras, sem borda.
    pub fn editor_border_width(&self, part: &ClassList) -> f64 {
        if part.contains("modal-editor-part") {
            1.0
        } else if self.has("modern-ui") {
            self.size(STROKE_THICKNESS)
        } else {
            0.0
        }
    }

    /// `border-color` das partes flutuantes (barra de atividade, barra lateral, painel). `part` leva `part` e o tipo
    /// (`activitybar`, `sidebar`, `auxiliarybar`, `panel`) e o lado (`left` ou `right`). `None` fora do `.floating-panels`
    /// ou para outro tipo de parte.
    pub fn floating_part_border_colors(&self, part: &ClassList) -> Option<BorderColors> {
        if !self.has("floating-panels") {
            return None;
        }
        if part.contains("activitybar") {
            // floatingPanels.css, linha 233: os quatro lados; o lado voltado à barra lateral é a emenda.
            return Some(BorderColors::uniform(self.card_border_or(&["modernActivityBar.border", "surface.border", "widget.border"])));
        }
        if part.contains("panel") {
            // floatingPanels.css, linha 78.
            return Some(BorderColors::uniform(self.card_border_or(&["modernPanel.border", "surface.border", "widget.border"])));
        }
        if part.any(&["sidebar", "auxiliarybar"]) {
            // floatingPanels.css, linhas 41 a 48: a borda cedida à emenda (linhas 62 a 73) fica transparente.
            let edge = self.card_border_or(&["surface.border", "widget.border"]);
            let seam = Color::transparent_color();
            let mut colors = BorderColors::uniform(edge);
            if part.contains("sidebar") && !self.has("noactivitybar") {
                if part.contains("left") {
                    colors.left = seam;
                } else if part.contains("right") {
                    colors.right = seam;
                }
            }
            return Some(colors);
        }
        None
    }

    /// `background-color` da raiz do workbench e da grade (`.monaco-grid-view`) com `.floating-panels`: o fundo do shell
    /// que aparece pelos vãos dos cartões (floatingPanels.css, linha 207). `--modern-ui-shell-background` não é uma cor de
    /// tema, então a cadeia começa em `modernUI.shellBackground`.
    pub fn shell_background(&self) -> Option<Color> {
        self.has("floating-panels").then(|| self.first(&["modernUI.shellBackground", "titleBar.activeBackground"])).flatten()
    }

    /// `left` e `right` calculados de uma linha de lista, em px (padding.css, linhas 40 a 46 e 72 a 76): com `.modern-ui` a
    /// linha é recuada de `spacing.size40` (`spacing.size20` no compacto) e a barra de rolagem fica na borda do painel; sem
    /// ele, `left: 0` e `right: 0` de `list.css`.
    pub fn list_row_horizontal_inset(&self, host: ListRowHost) -> f64 {
        let inset = match host {
            ListRowHost::PaneBody { chat_view: false } | ListRowHost::MultiDiffFileList if self.has("modern-ui") => {
                Some(if self.has("modern-ui-compact") { SPACING_SIZE20 } else { SPACING_SIZE40 })
            }
            _ => None,
        };
        inset.and_then(|id| get_size_registry().resolve_default_size(id, self.theme)).map_or(0.0, |size| size.value)
    }

    /// `background-color` do indicador (`.active-item-indicator`) de uma ação das abas de um painel composto
    /// (`tabs.css`, linhas 863 a 897 e 1136 a 1145). `None` é o fundo que nenhuma regra do Modern UI define.
    pub fn pane_tab_indicator_background(&self, action: ActionTabState) -> Option<Color> {
        if !self.has("modern-ui-tabs") {
            return None;
        }
        match (action.checked, action.hovered) {
            // `.action-item:not(.icon).checked` e, sem alto contraste, `.action-item.icon.checked`.
            (true, _) if !action.icon || !self.high_contrast() => self.color("modernTab.activeBackground"),
            (false, true) => self.color("modernTab.hoverBackground"),
            _ => None,
        }
    }

    /// `color` do `.action-label` de uma ação das abas de um painel composto ou das configurações (`tabs.css`, linhas
    /// 867 a 897 e 1136 a 1145).
    pub fn action_tab_foreground(&self, action: ActionTabState) -> Option<Color> {
        if !self.has("modern-ui-tabs") {
            return None;
        }
        if action.checked {
            self.color("modernTab.activeForeground")
        } else if action.hovered {
            self.color("modernTab.hoverForeground")
        } else {
            None
        }
    }

    /// `background-color` do `.action-label` das abas das configurações (`tabs.css`, linhas 1136 a 1145).
    pub fn settings_tab_background(&self, action: ActionTabState) -> Option<Color> {
        self.pane_tab_indicator_background(ActionTabState { icon: false, ..action })
    }

    /// `background-color` do `.tab-fill` de uma aba do editor fora do alto contraste (`tabs.css`, linhas 370 a 458). Os
    /// grupos sem foco usam as variáveis `unfocused-*`, que são aliases das focadas, então o grupo não muda o valor.
    pub fn editor_tab_fill_background(&self, tab: EditorTabState) -> Option<Color> {
        if self.has("modern-ui-tabs") && self.has("modern-ui-connected-editor-tabs") {
            // `connectedEditorTabs.css` (linhas 102 a 143 e 322 a 335) refaz as variáveis de fundo das abas.
            return self.connected_tab_fill_background(&TabElement::from(tab));
        }
        if !self.has("modern-ui-tabs") || self.high_contrast() {
            return None;
        }
        let id = match (tab.active, tab.selected, tab.hovered) {
            (true, _, false) => "modernEditorTab.activeBackground",
            (true, _, true) => "modernEditorTab.activeHoverBackground",
            (false, true, _) => "tab.selectedBackground",
            (false, false, true) => "modernEditorTab.hoverBackground",
            (false, false, false) => "modernEditorTab.inactiveBackground",
        };
        self.color(id)
    }

    /// `color` do `.tab-label` de uma aba do editor (`tabs.css`, linhas 1060 a 1111). A aba inativa usa
    /// `--modern-ui-editor-tab-inactive-foreground`: 50% do primeiro plano, ou `tab.inactiveForeground` nas abas conectadas
    /// fora do alto contraste (linhas 37 e 61). Cada variável `--modern-ui-editor-tab-*-foreground` que o DOM escreveu na raiz
    /// vence o padrão, e a do grupo sem foco (`unfocused-*`) é um alias da do grupo com foco (linhas 36, 38 e 40).
    pub fn editor_tab_foreground(&self, tab: EditorTabState) -> Option<Color> {
        if !self.has("modern-ui-tabs") {
            return None;
        }
        if tab.active {
            return self.tab_foreground_variable(tab.group_active, "active", || self.color("modernEditorTab.activeForeground"));
        }
        if tab.selected {
            return self.color("tab.selectedForeground");
        }
        if tab.hovered {
            return self.tab_foreground_variable(tab.group_active, "hover", || self.color("modernEditorTab.hoverForeground"));
        }
        self.tab_foreground_variable(tab.group_active, "inactive", || {
            if self.has("modern-ui-connected-editor-tabs") && !self.high_contrast() {
                self.color("tab.inactiveForeground")
            } else {
                self.color("foreground").map(|foreground| foreground.transparent(0.5))
            }
        })
    }

    /// `--modern-ui-editor-tab-<state>-foreground` (ou `unfocused-<state>-foreground`, que sem valor próprio é um alias da
    /// primeira): o que o DOM escreveu na raiz e, sem isso, o padrão da folha.
    fn tab_foreground_variable(&self, group_active: bool, state: &str, default: impl FnOnce() -> Option<Color>) -> Option<Color> {
        let own = self.root_variables.get(&format!("--modern-ui-editor-tab-{state}-foreground")).cloned();
        if group_active {
            return own.or_else(default);
        }
        self.root_variables.get(&format!("--modern-ui-editor-tab-unfocused-{state}-foreground")).cloned().or(own).or_else(default)
    }

    /// `color` do ícone (`::before`), do nome e da descrição de um rótulo de aba: o do `.tab-label`. O rótulo com a decoração
    /// de arquivo (`.monaco-decoration-itemColor`, `decoration_color`) fica de fora das regras de `tabs.css` (linhas 1067 a
    /// 1111) e o ícone, o nome e a descrição herdam a cor dela (linhas 1115 a 1119).
    pub fn editor_tab_label_color(&self, tab: EditorTabState, decoration_color: Option<&Color>) -> Option<Color> {
        decoration_color.cloned().or_else(|| self.editor_tab_foreground(tab))
    }

    /// `background-color` do `.tab-actions` de uma aba (`tabs.css`, linhas 580 a 650). O fundo só existe com o ponteiro
    /// na aba ou o foco dentro das ações e com a ação de fechar ligada; `.tab-actions-reserve-space` zera a sobreposição.
    pub fn editor_tab_actions_background(&self, tab: EditorTabState) -> Option<Color> {
        if !self.has("modern-ui-tabs") || tab.close_action_off || !(tab.hovered || tab.actions_focus_within) {
            return None;
        }
        if tab.actions_reserve_space {
            return Some(Color::transparent_color());
        }
        let id = if tab.selected && !tab.active {
            "modernEditorTab.selectedActionBackground"
        } else if tab.active && tab.hovered {
            "modernEditorTab.activeHoverActionBackground"
        } else if tab.active {
            "modernEditorTab.activeActionBackground"
        } else {
            "modernEditorTab.hoverActionBackground"
        };
        self.color(id)
    }
}

/// Onde uma lista de notificações ou os toasts ficam (`.notifications-center` e `.notifications-toasts`): a classe de
/// posição do elemento.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum NotificationsPlacement {
    /// Sem classe de posição: canto inferior direito.
    #[default]
    BottomRight,
    /// `.bottom-left`.
    BottomLeft,
    /// `.top-right`.
    TopRight,
}

/// As propriedades `--modern-ui-notifications-*-inset` que o shell define na raiz (`None` é a variável indefinida e
/// vale o espaçamento padrão da cadeia `var()`).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct NotificationInsets {
    /// `--modern-ui-notifications-inline-inset`.
    pub inline: Option<f64>,
    /// `--modern-ui-notifications-block-start-inset`.
    pub block_start: Option<f64>,
    /// `--modern-ui-notifications-block-end-inset`.
    pub block_end: Option<f64>,
}

/// Os deslocamentos `top`, `right`, `bottom` e `left` calculados, em px. `None` é `auto` (ou o valor que nenhuma regra do
/// Modern UI define).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct BoxOffsets {
    pub top: Option<f64>,
    pub right: Option<f64>,
    pub bottom: Option<f64>,
    pub left: Option<f64>,
}

/// O `padding` calculado de uma caixa, em px (vertical e horizontal, como o shorthand de dois valores).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct BoxPadding {
    pub vertical: f64,
    pub horizontal: f64,
}

/// O `border-radius` calculado de uma caixa, em px, na ordem do shorthand: canto superior esquerdo, superior direito,
/// inferior direito e inferior esquerdo.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct CornerRadii(pub f64, pub f64, pub f64, pub f64);

impl CornerRadii {
    fn uniform(radius: f64) -> Self {
        Self(radius, radius, radius, radius)
    }
}

impl<'a> ModernUiStyle<'a> {
    /// `.modern-ui-notifications-dialogs.monaco-workbench` (notificationsDialogs.css): o módulo está ligado.
    fn notifications_dialogs(&self) -> bool {
        self.has("modern-ui-notifications-dialogs")
    }

    /// `var(--vscode-<size>)`, em px; `0` quando o tamanho não existe para o tema.
    fn size(&self, id: &str) -> f64 {
        get_size_registry().resolve_default_size(id, self.theme).map_or(0.0, |size| size.value)
    }

    /// Define as variáveis `--modern-ui-notifications-*-inset` (`root.style.setProperty`).
    pub fn set_notification_insets(&mut self, insets: NotificationInsets) {
        self.notification_insets = insets;
    }

    /// `padding` de `.notification-list-item` (linhas 17 a 19).
    pub fn notification_item_padding(&self) -> Option<BoxPadding> {
        self.notifications_dialogs().then(|| BoxPadding { vertical: self.size(SPACING_SIZE60), horizontal: self.size(SPACING_SIZE20) })
    }

    /// `var(--modern-ui-notifications-inline-inset, var(--vscode-spacing-size80))`.
    fn inline_inset(&self) -> f64 {
        self.notification_insets.inline.unwrap_or_else(|| self.size(SPACING_SIZE80))
    }

    /// `var(--modern-ui-notifications-block-end-inset, <fallback>)`: a barra de status (size360) ou, sem ela, size320.
    fn block_end_inset(&self, placement: NotificationsPlacement) -> f64 {
        let fallback = if self.has("nostatusbar") && placement != NotificationsPlacement::TopRight { SPACING_SIZE320 } else { SPACING_SIZE360 };
        self.notification_insets.block_end.unwrap_or_else(|| self.size(fallback))
    }

    /// `var(--modern-ui-notifications-block-start-inset, var(--vscode-spacing-size360))`.
    fn block_start_inset(&self) -> f64 {
        self.notification_insets.block_start.unwrap_or_else(|| self.size(SPACING_SIZE360))
    }

    /// `right`, `bottom`, `left` e `top` de `.notifications-center` (linhas 47 a 66).
    pub fn notifications_center_offsets(&self, placement: NotificationsPlacement) -> Option<BoxOffsets> {
        if !self.notifications_dialogs() {
            return None;
        }
        let inline = self.inline_inset();
        let bottom = self.block_end_inset(placement);
        Some(match placement {
            NotificationsPlacement::BottomRight => BoxOffsets { right: Some(inline), bottom: Some(bottom), ..BoxOffsets::default() },
            NotificationsPlacement::BottomLeft => BoxOffsets { left: Some(inline), bottom: Some(bottom), ..BoxOffsets::default() },
            NotificationsPlacement::TopRight => BoxOffsets { top: Some(self.block_start_inset()), right: Some(inline), ..BoxOffsets::default() },
        })
    }

    /// `right`, `bottom`, `left` e `top` de `.notifications-toasts` (linhas 73 a 95): cada deslocamento perde a margem de
    /// `spacing.size40` do toast.
    pub fn notifications_toasts_offsets(&self, placement: NotificationsPlacement) -> Option<BoxOffsets> {
        let center = self.notifications_center_offsets(placement)?;
        let margin = self.size(SPACING_SIZE40);
        let reduce = |offset: Option<f64>| offset.map(|value| value - margin);
        Some(BoxOffsets { top: reduce(center.top), right: reduce(center.right), bottom: reduce(center.bottom), left: reduce(center.left) })
    }

    /// `border-radius` de `.notifications-center` (linhas 47 a 51).
    pub fn notifications_center_radius(&self) -> Option<f64> {
        self.notifications_dialogs().then(|| self.size(CORNER_RADIUS_LARGE))
    }

    /// `border-radius` de um `.monaco-list-row` da central de notificações (linhas 101 a 103): só a última linha arredonda
    /// os cantos de baixo.
    pub fn notifications_center_row_radius(&self, last_child: bool) -> Option<CornerRadii> {
        (self.notifications_dialogs() && last_child).then(|| {
            let radius = self.size(CORNER_RADIUS_LARGE);
            CornerRadii(0.0, 0.0, radius, radius)
        })
    }

    /// `border-radius` do `.notifications-toasts`, da lista, do `.notification-toast`, da rolagem e das linhas dos toasts
    /// (linhas 105 a 112).
    pub fn notifications_toast_radius(&self) -> Option<CornerRadii> {
        self.notifications_dialogs().then(|| CornerRadii::uniform(self.size(CORNER_RADIUS_LARGE)))
    }

    /// `margin-bottom` de `.notification-toast`: a margem de 4px de `notificationsToasts.css` (linha 39), zerada nos
    /// toasts que não são o último (linhas 78 a 80).
    pub fn notification_toast_margin_bottom(&self, last_child: bool) -> f64 {
        if self.notifications_dialogs() && !last_child {
            self.size(SPACING_NONE)
        } else {
            4.0
        }
    }

    /// `padding` de `.monaco-dialog-box` (linhas 114 a 117).
    pub fn dialog_padding(&self) -> Option<f64> {
        self.notifications_dialogs().then(|| self.size(SPACING_SIZE40))
    }

    /// `min-width` de `.monaco-dialog-box`, em px (linhas 114 a 117).
    pub fn dialog_min_width(&self) -> Option<f64> {
        self.notifications_dialogs().then_some(440.0)
    }
}

/// Em que parte um `.pane` está (`.part.sidebar`, `.part.auxiliarybar`, `.part.panel`), para `paneHeaders.css`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PaneHost {
    SideBar,
    AuxiliaryBar,
    Panel,
    /// Qualquer outra parte: nenhuma regra de cor do Modern UI alcança o pane.
    Other,
}

/// O estado de um `.pane` que as regras de `paneHeaders.css` testam.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PaneState {
    pub host: PaneHost,
    /// `.split-view-view:first-of-type`: o primeiro pane da pilha vertical.
    pub first_in_stack: bool,
    /// `.pane.horizontal`.
    pub horizontal: bool,
    /// `:hover` no cabeçalho.
    pub header_hovered: bool,
}

impl<'a> ModernUiStyle<'a> {
    /// `background-color` do `.pane` (paneHeaders.css, linhas 65 a 72, `!important`).
    pub fn pane_background(&self, host: PaneHost) -> Option<Color> {
        if !self.has("modern-ui") {
            return None;
        }
        match host {
            PaneHost::SideBar | PaneHost::AuxiliaryBar => self.color("sideBar.background"),
            PaneHost::Panel => self.color("panel.background"),
            PaneHost::Other => None,
        }
    }

    /// `background-color` do `.pane-header` (paneHeaders.css, linhas 82 a 88 e 99 a 101): a cor da superfície em repouso e
    /// a de `list.hoverBackground` com o ponteiro em cima.
    pub fn pane_header_background(&self, pane: PaneState) -> Option<Color> {
        if !self.has("modern-ui") {
            return None;
        }
        if pane.header_hovered {
            return self.color("list.hoverBackground");
        }
        self.pane_background(pane.host)
    }

    /// `background-color` do `::before` do `.pane-header`, o separador recuado (paneHeaders.css, linhas 47 a 53).
    pub fn pane_header_separator_color(&self, host: PaneHost) -> Option<Color> {
        if !self.has("modern-ui") {
            return None;
        }
        match host {
            PaneHost::SideBar | PaneHost::AuxiliaryBar => self.first(&["sideBarSectionHeader.border", "surface.border"]),
            PaneHost::Panel => self.first(&["panelSectionHeader.border", "panel.border", "surface.border"]),
            PaneHost::Other => None,
        }
    }

    /// `display` do `::before` do `.pane-header` é diferente de `none` (paneHeaders.css, linhas 60 a 63): o primeiro pane da
    /// pilha vertical e os panes horizontais não têm o separador.
    pub fn pane_header_separator_visible(&self, pane: PaneState) -> bool {
        self.has("modern-ui") && !pane.first_in_stack && !pane.horizontal
    }

    /// `border-top-width` do `.pane-header`: o Modern UI zera a borda que o `PaneView` aplica inline (paneHeaders.css,
    /// linhas 29 a 31, `!important`). `None` sem `.modern-ui`: vale o valor inline.
    pub fn pane_header_border_top_width(&self) -> Option<f64> {
        self.has("modern-ui").then_some(0.0)
    }
}

/// `--activity-bar-width` e `--activity-bar-action-height` (`None` é a variável indefinida e vale o padrão da cadeia
/// `var()` de cada regra).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct ActivityBarMetrics {
    pub width: Option<f64>,
    pub action_height: Option<f64>,
}

/// Largura e altura calculadas de uma caixa, em px.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct BoxSize {
    pub width: f64,
    pub height: f64,
}

/// Um `.action-item` da barra de atividade que as regras de `activityBar.css` testam.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ActivityItemState {
    /// `.checked`.
    pub checked: bool,
    /// `:hover`.
    pub hovered: bool,
}

/// O `.badge-content` de um item da barra de atividade.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ActivityBadgeBox {
    pub top: f64,
    pub right: f64,
    pub width: Option<f64>,
    pub height: f64,
}

/// A geometria de um `.action-item` das abas de um painel composto (`tabs.css`, linhas 784 a 850): a caixa do item e o
/// indicador `.active-item-indicator`, em px.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PaneCompositeActionBox {
    /// `height` calculado do `.action-item`.
    pub item_height: f64,
    /// `border-top-width` do `.action-item` (`border-block`).
    pub item_border_block: f64,
    /// `height` calculado do indicador.
    pub indicator_height: f64,
    /// Distância do topo da caixa de borda do item ao topo do indicador (`top: 50%` com `translateY(-50%)`).
    pub indicator_top_inset: f64,
    /// Distância da base do indicador à base da caixa de borda do item.
    pub indicator_bottom_inset: f64,
}

impl<'a> ModernUiStyle<'a> {
    /// Define `--activity-bar-width` e `--activity-bar-action-height` (`root.style.setProperty`).
    pub fn set_activity_bar_metrics(&mut self, metrics: ActivityBarMetrics) {
        self.activity_bar_metrics = metrics;
    }

    fn activity_bar_action_height(&self, compact: bool) -> f64 {
        self.activity_bar_metrics.action_height.unwrap_or(if compact { 28.0 } else { 36.0 })
    }

    /// `background-color` do `.active-item-indicator` da barra de atividade (activityBar.css, linhas 27 a 41 e 94 a 110):
    /// o item marcado e o item com o ponteiro em cima têm a caixa preenchida; no alto contraste só o contorno.
    pub fn activity_bar_indicator_background(&self, item: ActivityItemState) -> Option<Color> {
        if !self.has("modern-ui") {
            return None;
        }
        if self.high_contrast() && (item.checked || item.hovered) {
            return Some(Color::transparent_color());
        }
        match (item.checked, item.hovered) {
            (true, _) => self.color("modernActivityBarItem.activeBackground"),
            (false, true) => self.color("modernActivityBarItem.hoverBackground"),
            _ => None,
        }
    }

    /// `color` do `.action-label.codicon` da barra de atividade (activityBar.css, linhas 31 a 45, `!important`).
    pub fn activity_bar_label_foreground(&self, item: ActivityItemState) -> Option<Color> {
        if !self.has("modern-ui") {
            return None;
        }
        match (item.checked, item.hovered) {
            (true, _) => self.color("modernActivityBarItem.activeForeground"),
            (false, true) => self.color("modernActivityBarItem.hoverForeground"),
            _ => None,
        }
    }

    /// `width`, `height` e `border-radius` do indicador de um item marcado ou com o ponteiro em cima (activityBar.css, linhas
    /// 17 a 25 e 51 a 56). `part` leva `activitybar` e, no compacto, `compact`.
    pub fn activity_bar_indicator_box(&self, part: &ClassList) -> Option<(BoxSize, f64)> {
        if !self.has("modern-ui") || !part.contains("activitybar") {
            return None;
        }
        let side = self.activity_bar_action_height(part.contains("compact")) - 4.0;
        Some((BoxSize { width: side, height: side }, self.size(CORNER_RADIUS_SMALL)))
    }

    /// O `.badge-content` da barra de atividade (activityaction.css, linhas 158 a 181; activityBar.css, linhas 248 a 268):
    /// `empty` é o `:empty`, o selo redondo sem texto.
    pub fn activity_bar_badge_content(&self, part: &ClassList, empty: bool) -> Option<ActivityBadgeBox> {
        if !self.has("modern-ui") || !part.contains("activitybar") {
            return None;
        }
        let compact = part.contains("compact");
        let (top, side) = if compact { (13.0, 13.0) } else { (18.0, 16.0) };
        Some(ActivityBadgeBox { top, right: 3.0, width: empty.then_some(side), height: side })
    }

    /// `.pane-composite-part > :is(.title, .header-or-footer)`: com `.modern-ui-tabs` o item ocupa a altura toda do
    /// cabeçalho, e com `.modern-ui` ela é fixa em `spacing.size320`. `title_height` é a altura do `.title`.
    pub fn pane_composite_action_box(&self, title_height: f64) -> Option<PaneCompositeActionBox> {
        if !self.has("modern-ui-tabs") {
            return None;
        }
        let item_height = if self.has("modern-ui") { self.size(SPACING_SIZE320) } else { title_height };
        let item_border_block = self.size(SPACING_SIZE40);
        let indicator_height = self.size(SPACING_SIZE240);
        // `top: 50%` do bloco de conteúdo mais `translateY(-50%)`, com a borda do item de fora.
        let inset = item_border_block + ((item_height - 2.0 * item_border_block) - indicator_height) / 2.0;
        Some(PaneCompositeActionBox { item_height, item_border_block, indicator_height, indicator_top_inset: inset, indicator_bottom_inset: inset })
    }

    /// `top` e `right` do `.badge-content` de um `.badge.compact` num item de ícone (tabs.css, linhas 847 a 850).
    pub fn pane_composite_badge_content_offsets(&self) -> Option<(f64, f64)> {
        self.has("modern-ui-tabs").then(|| (13.0, self.size(SPACING_SIZE20)))
    }

    /// `font-weight` do `.action-label` de uma aba de painel composto (tabs.css, linha 859 para o item sem ícone;
    /// activityBar.css, linha 193 para o ícone codicon). `None` é o peso que nenhuma regra do Modern UI define.
    pub fn pane_composite_label_font_weight(&self, icon: bool) -> Option<f64> {
        let applies = if icon { self.has("modern-ui") } else { self.has("modern-ui-tabs") };
        applies.then(|| self.size(FONT_WEIGHT_REGULAR))
    }

    /// `border-top-width` do `.composite.title` do painel inferior (padding.css, linhas 274 a 277): o Modern UI zera a
    /// borda de 1px que descentraliza as pílulas. `None` mantém a borda clássica.
    pub fn bottom_panel_title_border_top_width(&self, part: &ClassList) -> Option<f64> {
        (self.has("modern-ui") && self.has("monaco-workbench") && part.contains("panel") && part.contains("bottom")).then_some(0.0)
    }

    /// `color` do `.action-label.codicon-more` das abas de um painel composto (activityBar.css, linhas 201 a 206,
    /// `!important`).
    pub fn pane_composite_overflow_foreground(&self) -> Option<Color> {
        (self.has("modern-ui") && self.has("monaco-workbench")).then(|| self.color("icon.foreground")).flatten()
    }

    /// `border-bottom-width` do `.header` e `border-top-width` do `.footer` de um `.pane-composite-part` que não está vazio
    /// (activityBar.css, linhas 225 a 232). `None` mantém a borda clássica.
    pub fn pane_composite_header_footer_border_width(&self, part: &ClassList) -> Option<f64> {
        (self.has("modern-ui") && self.has("monaco-workbench") && !part.contains("empty")).then_some(0.0)
    }

    /// `overflow` do `.header` é `visible` (activityBar.css, linhas 225 a 228).
    pub fn pane_composite_header_overflow_visible(&self, part: &ClassList) -> bool {
        self.pane_composite_header_footer_border_width(part).is_some()
    }
}

/// O `margin` calculado de uma parte flutuante, em px.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PartMargins {
    pub top: f64,
    pub right: f64,
    pub bottom: f64,
    pub left: f64,
}

/// A geometria da trilha da barra de atividade (`.part.activitybar`) e do seu primeiro item marcado, em px, relativa à
/// caixa de borda da trilha.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ActivityRailGeometry {
    /// `width` da trilha: a largura do item mais a faixa (`--modern-ui-activitybar-lane`).
    pub width: f64,
    /// `width` do `.action-item`.
    pub action_width: f64,
    /// Centro do item menos o centro da trilha.
    pub action_center_offset: f64,
    /// Borda esquerda do indicador do item marcado.
    pub indicator_left: f64,
    /// Borda direita do indicador do item marcado.
    pub indicator_right: f64,
}

impl<'a> ModernUiStyle<'a> {
    /// `--modern-ui-floating-card-margin` (`floatingPanels.css`, linhas 24 a 34).
    fn floating_card_margin(&self) -> f64 {
        if self.compact() { self.size(SPACING_NONE) } else { self.size(SPACING_SIZE40) }
    }

    /// `--modern-ui-floating-card-inner-margin`.
    fn floating_card_inner_margin(&self) -> f64 {
        self.size(SPACING_NONE)
    }

    /// `--modern-ui-floating-card-outer-margin`: igual ao `--modern-ui-floating-card-margin`.
    fn floating_card_outer_margin(&self) -> f64 {
        self.floating_card_margin()
    }

    /// `--modern-ui-activitybar-lane` (linhas 227 e 294).
    fn activity_bar_lane(&self) -> f64 {
        self.size(if self.compact() { SPACING_SIZE40 } else { SPACING_SIZE80 })
    }

    /// `border-radius` do `.part.activitybar` (floatingPanels.css, linhas 239 a 274 e 293 a 298): arredondada do lado da
    /// janela, quadrada onde encontra a barra lateral, e com o canto exposto por um painel horizontal arredondado.
    /// `None` fora do `.floating-panels`. A ordem é a do shorthand.
    pub fn activity_bar_corner_radii(&self, part: &ClassList) -> Option<CornerRadii> {
        if !self.has("floating-panels") || !part.contains("activitybar") {
            return None;
        }
        if self.compact() {
            return Some(CornerRadii::uniform(0.0));
        }
        let large = self.size(CORNER_RADIUS_LARGE);
        if self.has("nosidebar") {
            return Some(CornerRadii::uniform(large));
        }
        let (left, right) = (part.contains("left"), part.contains("right"));
        let has_panel = !self.has("nopanel");
        let alignment = |names: &[&str]| names.iter().any(|name| self.has(&format!("panel-alignment-{name}")));
        let (mut top_left, mut top_right, mut bottom_right, mut bottom_left) = if left {
            (large, 0.0, 0.0, large)
        } else {
            (0.0, large, large, 0.0)
        };
        // Linhas 253 a 267: o canto exposto além da barra lateral encurtada pelo painel horizontal.
        if has_panel && self.has("panel-position-top") {
            if left && alignment(&["justify", "left"]) {
                top_right = large;
            }
            if right && alignment(&["justify", "right"]) {
                top_left = large;
            }
        }
        if has_panel && self.has("panel-position-bottom") {
            if left && alignment(&["justify", "left"]) {
                bottom_right = large;
            }
            if right && alignment(&["justify", "right"]) {
                bottom_left = large;
            }
        }
        Some(CornerRadii(top_left, top_right, bottom_right, bottom_left))
    }

    /// `margin` do `.part.activitybar` e do `.part.panel` flutuantes (floatingPanels.css, linhas 76 a 102, 222 a 298 e 532 a
    /// 535), da barra lateral e do editor principal. `None` fora do `.floating-panels` ou para outro tipo de parte.
    pub fn floating_part_margins(&self, part: &ClassList) -> Option<PartMargins> {
        if !self.has("floating-panels") {
            return None;
        }
        let (card, inner, outer) = (self.floating_card_margin(), self.floating_card_inner_margin(), self.floating_card_outer_margin());
        if part.contains("activitybar") {
            let mut margins = PartMargins { top: inner, right: inner, bottom: card, left: inner };
            if part.contains("left") {
                margins.left = outer;
            } else if part.contains("right") {
                margins.right = outer;
                if self.has("nosidebar") && !self.compact() {
                    margins.left = card;
                }
            }
            if self.has("top-window-edge") {
                margins.top = outer;
            }
            if self.has("nostatusbar") || self.compact() {
                margins.bottom = outer;
            }
            return Some(margins);
        }
        if part.any(&["sidebar", "auxiliarybar"]) {
            // Linhas 39 a 52 e 62 a 67: a barra lateral esquerda encontra a trilha sem vão.
            let mut margins = PartMargins { top: inner, right: inner, bottom: card, left: card };
            if part.contains("sidebar") && part.contains("left") && !self.has("noactivitybar") {
                margins.left = inner;
            }
            return Some(margins);
        }
        if part.contains("panel") {
            let mut margins = PartMargins { top: inner, right: inner, bottom: card, left: card };
            let horizontal = part.any(&["top", "bottom"]);
            if !self.has("nosidebar")
                && (self.has("panel-alignment-justify") || self.has("panel-alignment-right"))
                && horizontal
                && !part.contains("floating-part-outer-right")
            {
                margins.right = card;
            }
            if part.contains("top") {
                margins.bottom = if !self.compact() && part.contains("floating-part-outer-bottom") { outer } else { inner };
            }
            if self.has("panel-position-bottom") && !self.has("nopanel") && !self.has("nomaineditorarea") && part.contains("bottom") {
                margins.top = card;
            }
            if self.compact() && part.contains("floating-part-outer-bottom") {
                margins.bottom = outer;
            }
            return Some(margins);
        }
        if part.contains("editor") {
            // Linhas 176 a 202, 441 a 443, 461 a 463 e 532 a 535 (o editor principal, filho da grade): a precedência é a da
            // especificidade, que põe o painel em cima (linhas 187 e 461) acima da margem da janela (linhas 180 e 441).
            let mut margins = PartMargins { top: inner, right: inner, bottom: card, left: card };
            if self.has("top-window-edge") {
                margins.top = outer;
            }
            if self.has("panel-position-top") && !self.has("nopanel") {
                margins.top = card;
            }
            if part.contains("floating-editor-outer-left") {
                margins.left = outer;
            }
            if part.contains("floating-editor-outer-right") {
                margins.right = outer;
            }
            if self.has("nostatusbar") {
                margins.bottom = outer;
            }
            if self.has("panel-position-bottom") && !self.has("nopanel") {
                margins.bottom = inner;
            }
            if self.compact() && part.contains("floating-editor-outer-bottom") {
                margins.bottom = outer;
            }
            return Some(margins);
        }
        None
    }

    /// A trilha da barra de atividade flutuante e o indicador do primeiro item marcado (floatingPanels.css, linhas 222 a
    /// 249, 293 a 325 e 347 a 349; activityBar.css, linhas 17 a 25 e 51 a 56). `None` fora do `.floating-panels`.
    pub fn activity_rail_geometry(&self, part: &ClassList) -> Option<ActivityRailGeometry> {
        if !self.has("floating-panels") || !part.contains("activitybar") {
            return None;
        }
        let stroke = self.size(STROKE_THICKNESS);
        let lane = self.activity_bar_lane();
        let action_width = self.activity_bar_metrics.width.unwrap_or(48.0);
        let width = action_width + lane;
        // `.content` desloca `(lane - 2px) / 2` e começa dentro da borda de um traço.
        let action_left = stroke + (lane - 2.0 * stroke) / 2.0;
        let action_height = self.activity_bar_action_height(part.contains("compact"));
        let indicator_left = action_left + (action_width - action_height + 4.0) / 2.0;
        Some(ActivityRailGeometry {
            width,
            action_width,
            action_center_offset: action_left + action_width / 2.0 - width / 2.0,
            indicator_left,
            indicator_right: indicator_left + (action_height - 4.0),
        })
    }
}

/// Os quatro lados de uma caixa, em px (`padding-top` e os outros).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct BoxEdges {
    pub top: f64,
    pub right: f64,
    pub bottom: f64,
    pub left: f64,
}

/// Os lados esquerdo e direito de uma propriedade horizontal, em px.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct HorizontalEdges {
    pub left: f64,
    pub right: f64,
}

/// As classes de um `.statusbar-item` que as regras da barra de status testam.
struct StatusBarItemFlags {
    /// `.left.first-visible-item`.
    edge_left: bool,
    /// `.right.last-visible-item`.
    edge_right: bool,
    /// `.has-background-color`.
    colored: bool,
    /// `.compact-left`.
    compact_left: bool,
    /// `.compact-right`.
    compact_right: bool,
}

impl StatusBarItemFlags {
    fn new(item: &ClassList) -> Self {
        Self {
            edge_left: item.contains("left") && item.contains("first-visible-item"),
            edge_right: item.contains("right") && item.contains("last-visible-item"),
            colored: item.contains("has-background-color"),
            compact_left: item.contains("compact-left"),
            compact_right: item.contains("compact-right"),
        }
    }

    fn edge(&self) -> bool {
        self.edge_left || self.edge_right
    }
}

impl<'a> ModernUiStyle<'a> {
    /// `.modern-ui.modern-ui-compact` (statusBar.css): diferente de [`Self::compact`], que exige o `.floating-panels`.
    fn modern_compact(&self) -> bool {
        self.has("modern-ui") && self.has("modern-ui-compact")
    }

    /// `padding` do `.part.statusbar` (floatingPanels.css, linhas 352 a 385). O compacto (linhas 382 a 385) tem a própria
    /// folga e exclui as duas regras que a tomam do cartão (linhas 368 a 379), então as densidades não competem. Fora do
    /// `.floating-panels` a barra de status não tem `padding`.
    pub fn status_bar_padding(&self) -> BoxEdges {
        if !self.has("floating-panels") {
            return BoxEdges { top: 0.0, right: 0.0, bottom: 0.0, left: 0.0 };
        }
        let horizontal = if self.has("modern-ui-compact") {
            self.size(SPACING_SIZE40)
        } else if self.has("noactivitybar") || self.has("activitybar-compact") {
            self.floating_card_margin()
        } else {
            self.size(SPACING_SIZE60)
        };
        let vertical = self.size(SPACING_SIZE20);
        BoxEdges { top: vertical, right: horizontal, bottom: vertical, left: horizontal }
    }

    /// `height` calculado de um `.statusbar-item` (statusbarpart.css, linha 60: `100%` do contêiner de itens, que preenche a
    /// caixa de conteúdo da barra) e, por tabela, do `.statusbar-item-label` (linha 100). `status_bar_height` é a `height`
    /// da barra, de caixa de borda.
    pub fn status_bar_item_height(&self, status_bar_height: f64) -> f64 {
        let padding = self.status_bar_padding();
        status_bar_height - padding.top - padding.bottom
    }

    /// `padding-left` e `padding-right` de um `.statusbar-item` (statusbarpart.css, linhas 87 a 95; floatingPanels.css, linhas
    /// 387 a 408). A folga do cartão no item da ponta esquerda cai para zero quando o item tem fundo próprio: o rótulo
    /// assume o recuo e o fundo encosta na borda.
    pub fn status_bar_item_padding(&self, item: &ClassList) -> HorizontalEdges {
        let flags = StatusBarItemFlags::new(item);
        let floating = self.has("floating-panels");
        let left = if floating && flags.edge_left && flags.colored {
            self.size(SPACING_NONE)
        } else if floating && flags.edge_left {
            self.floating_card_margin()
        } else if flags.edge_left {
            2.0
        } else {
            0.0
        };
        let right = if floating && flags.edge_right && flags.colored {
            self.size(SPACING_NONE)
        } else if flags.edge_right {
            2.0
        } else {
            0.0
        };
        HorizontalEdges { left, right }
    }

    /// `margin-left` e `margin-right` do `.statusbar-item-label` (statusbarpart.css, linhas 97 a 135; statusBar.css, linhas
    /// 44 a 57). Em empate de especificidade vale a regra de `statusbarpart.css`, que o caso do oráculo exige (o
    /// `.compact-left` do compacto mantém os 5px da direita). Dentro de uma mesma especificidade vale a regra mais adiante.
    pub fn status_bar_label_margin(&self, item: &ClassList) -> HorizontalEdges {
        let flags = StatusBarItemFlags::new(item);
        let compact = self.modern_compact();
        // Especificidade 8: as pontas, `.compact-left.compact-right` (linha 123) e as exceções do compacto. Depois, a 7 e
        // na ordem do arquivo: `.compact-left` (111), `.compact-right` (117), `.has-background-color` (130).
        let left = if flags.edge()
            || (flags.compact_left && flags.compact_right)
            || (compact && (flags.compact_left || flags.colored))
            || flags.colored
            || flags.compact_left
        {
            0.0
        } else if flags.compact_right {
            5.0
        } else if compact {
            self.size(SPACING_SIZE20)
        } else {
            3.0
        };
        let right = if flags.edge()
            || (flags.compact_left && flags.compact_right)
            || (compact && (flags.compact_right || flags.colored))
            || flags.colored
            || flags.compact_right
        {
            0.0
        } else if flags.compact_left {
            5.0
        } else if compact {
            self.size(SPACING_SIZE20)
        } else {
            3.0
        };
        HorizontalEdges { left, right }
    }

    /// `padding-left` e `padding-right` do `.statusbar-item-label`, da regra de maior especificidade para a menor
    /// (statusbarpart.css, linhas 97 a 145; statusBar.css, linhas 44 a 100; floatingPanels.css, linhas 387 a 423). Vale a
    /// mesma regra de desempate de [`Self::status_bar_label_margin`]; o `.floating-panels` fica antes do Modern UI.
    pub fn status_bar_label_padding(&self, item: &ClassList) -> HorizontalEdges {
        let flags = StatusBarItemFlags::new(item);
        let floating = self.has("floating-panels");
        let compact = self.modern_compact();
        let floating_compact = floating && self.has("modern-ui-compact");
        let size40 = self.size(SPACING_SIZE40);
        let size60 = self.size(SPACING_SIZE60);
        let size80 = self.size(SPACING_SIZE80);
        let colored_edge = floating && flags.colored && flags.edge();
        // Especificidade 11 (floatingPanels.css, linhas 415 a 423) e 10 (linhas 401 a 413).
        if colored_edge && floating_compact {
            return HorizontalEdges { left: size40, right: if flags.edge_left { size40 } else { size60 } };
        }
        if colored_edge {
            return if flags.edge_left { HorizontalEdges { left: size80, right: size60 } } else { HorizontalEdges { left: size60, right: size80 } };
        }
        // Especificidade 9: a ponta do compacto (statusBar.css, linhas 96 a 100) vence o `padding-left` da folga do cartão
        // (floatingPanels.css, linhas 387 a 390), que só toca a esquerda e só na ponta esquerda.
        if compact && flags.edge() {
            return HorizontalEdges { left: size40, right: size40 };
        }
        let floating_left = (floating && flags.edge_left).then(|| self.floating_card_margin());
        // Especificidade 8 e 7 do arquivo base, na ordem do arquivo (a regra mais adiante vence).
        let base = if flags.compact_right && flags.colored {
            HorizontalEdges { left: 10.0, right: 3.0 }
        } else if flags.compact_left && flags.colored {
            HorizontalEdges { left: 3.0, right: 10.0 }
        } else if flags.edge() || flags.colored {
            HorizontalEdges { left: 8.0, right: 8.0 }
        } else if flags.compact_right || flags.compact_left {
            HorizontalEdges { left: 3.0, right: 3.0 }
        } else if compact {
            HorizontalEdges { left: size40, right: size40 }
        } else {
            HorizontalEdges { left: 5.0, right: 5.0 }
        };
        HorizontalEdges { left: floating_left.unwrap_or(base.left), right: base.right }
    }
}

/// O id da cor que cada variável `--modern-ui-*` das abas repassa (`tabs.css`, linhas 24 a 54). As variáveis
/// `unfocused-*` são aliases das focadas.
fn modern_ui_tab_variable_color_id(name: &str) -> Option<&'static str> {
    let name = name.strip_prefix("--modern-ui-")?.replace("-unfocused-", "-");
    Some(match name.as_str() {
        "tab-active-background" => "modernTab.activeBackground",
        "tab-hover-background" => "modernTab.hoverBackground",
        "editor-tab-active-background" => "modernEditorTab.activeBackground",
        "editor-tab-inactive-background" => "modernEditorTab.inactiveBackground",
        "editor-tab-hover-background" => "modernEditorTab.hoverBackground",
        "editor-tab-active-hover-background" => "modernEditorTab.activeHoverBackground",
        "editor-tab-active-foreground" => "modernEditorTab.activeForeground",
        "editor-tab-hover-foreground" => "modernEditorTab.hoverForeground",
        "editor-tab-action-active-background" => "modernEditorTab.activeActionBackground",
        "editor-tab-action-hover-background" => "modernEditorTab.hoverActionBackground",
        "editor-tab-action-active-hover-background" => "modernEditorTab.activeHoverActionBackground",
        _ => return None,
    })
}

/// `position` do `.menubar-menu-items-holder` (menubar.css, linhas 66 a 76): o menu do aplicativo é `fixed`, ancorado na
/// janela e não na trilha da barra de atividade (por isso `floatingPanels.css`, linhas 316 a 325, desloca o `.content` com
/// `left` e não com `transform`, que viraria o bloco contêiner do menu e o recortaria).
pub const MENUBAR_MENU_HOLDER_POSITION: &str = "fixed";

/// `z-index` de uma barra de rolagem visível (`.monaco-scrollable-element > .scrollbar.visible`, scrollbars.css).
pub const VISIBLE_SCROLLBAR_Z_INDEX: i32 = 11;

/// A caixa de um `.menubar.compact` da barra de atividade, em px.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct MenubarBox {
    /// `display` do `.menubar` (menubar.css, linha 8).
    pub display: &'static str,
    /// `height` calculado.
    pub height: f64,
    /// `padding-top` calculado.
    pub padding_top: f64,
}

/// O estado de um `.menubar-menu-button` da barra compacta que as regras de `menubarControl.css` e de `activityBar.css`
/// testam.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct MenubarButtonState {
    /// `:focus`.
    pub focused: bool,
    /// `.open`.
    pub open: bool,
    /// `:hover`.
    pub hovered: bool,
    /// `:focus-within` na `.menubar`.
    pub focus_within: bool,
}

/// O `outline` calculado de um título de menu.
#[derive(Clone, Debug, PartialEq)]
pub struct OutlineStyle {
    /// `None` é `currentcolor`.
    pub color: Option<Color>,
    pub style: &'static str,
    pub width: f64,
}

/// O que as regras pintam no botão e no `.toolbar-toggle-more` do menu compacto da barra de atividade.
#[derive(Clone, Debug, PartialEq)]
pub struct CompactMenuToggleStyle {
    /// `background-color` do `.menubar-menu-button` (`None` é transparente, o padrão).
    pub button_background: Option<Color>,
    /// `background-color` do `.toolbar-toggle-more` (`None` é transparente, o padrão).
    pub background: Option<Color>,
    /// `color` do `.toolbar-toggle-more`.
    pub color: Option<Color>,
    /// `outline` do `.menubar-menu-title` (`None` é `outline-style: none`).
    pub outline: Option<OutlineStyle>,
    /// `font-size` do `.toolbar-toggle-more` em px (`None` quando nenhuma regra do Modern UI o define).
    pub font_size: Option<f64>,
}

impl<'a> ModernUiStyle<'a> {
    /// Define `--activity-bar-action-gap` (`root.style.setProperty`).
    pub fn set_activity_bar_action_gap(&mut self, gap: f64) {
        self.activity_bar_action_gap = Some(gap);
    }

    /// `.modern-ui.monaco-workbench`: as regras de `activityBar.css` que miram o menu compacto.
    fn modern_workbench(&self) -> bool {
        self.has("modern-ui") && self.has("monaco-workbench")
    }

    /// A caixa do `.menubar.compact` de uma `.part.activitybar` (activityBar.css, linhas 58 a 61; floatingPanels.css, linhas
    /// 334 a 337). `None` quando nenhuma regra do Modern UI nem do `.floating-panels` o alcança.
    pub fn compact_menubar_box(&self, part: &ClassList) -> Option<MenubarBox> {
        if !part.contains("activitybar") {
            return None;
        }
        // `var(--activity-bar-action-height, 36px)`: nas duas regras o padrão é 36px, também no compacto.
        let action_height = self.activity_bar_metrics.action_height.unwrap_or(36.0);
        let floating_rail = self.has("floating-panels") && !part.contains("top") && !part.contains("bottom");
        if floating_rail {
            // `(var(--modern-ui-activitybar-lane) - 2px) / 2`: o `2px` são as duas bordas de 1px do cartão.
            let inset = (self.activity_bar_lane() - 2.0) / 2.0;
            return Some(MenubarBox { display: "flex", height: action_height + inset, padding_top: inset });
        }
        self.modern_workbench().then_some(MenubarBox { display: "flex", height: action_height, padding_top: 0.0 })
    }

    /// `margin-top` de um `.action-item` da barra de atividade (floatingPanels.css, linhas 308 a 314): o vão entre os itens
    /// vale só para o item que segue outro (`.action-item + .action-item`) fora de um `.monaco-menu`, que guarda o
    /// espaçamento próprio das linhas e dos separadores.
    pub fn activity_bar_action_margin_top(&self, inside_menu: bool, follows_sibling: bool) -> f64 {
        if self.has("floating-panels") && !inside_menu && follows_sibling {
            self.activity_bar_action_gap.unwrap_or(0.0)
        } else {
            0.0
        }
    }

    /// `z-index` da barra de rolagem vertical do menu do aplicativo (activityBar.css, linhas 67 a 69): o Modern UI a leva ao
    /// fundo, e a do submenu (que não é filha direta do `.menubar-menu-items-holder`) segue [`VISIBLE_SCROLLBAR_Z_INDEX`].
    pub fn application_menu_scrollbar_z_index(&self) -> i32 {
        if self.modern_workbench() { 0 } else { VISIBLE_SCROLLBAR_Z_INDEX }
    }

    /// O botão e o `.toolbar-toggle-more` do menu compacto da barra de atividade (menubarControl.css, linhas 11 a 27 e 49 a
    /// 63; menubar.css, linhas 89 a 97; activityBar.css, linhas 71 a 92). Com o Modern UI o botão não pinta fundo e o
    /// alvo assume as cores de item da barra de atividade.
    pub fn compact_menu_toggle_style(&self, state: MenubarButtonState) -> CompactMenuToggleStyle {
        let active = state.open || state.focused;
        // `.menubar.compact:not(:focus-within) > .menubar-menu-button:hover`.
        let hovered = state.hovered && !state.focus_within;
        let modern = self.modern_workbench();
        let button_background = if modern && active {
            None
        } else if state.focused {
            self.color("menubar.selectionBackground")
        } else {
            None
        };
        let (background, color) = if modern && hovered {
            (self.color("modernActivityBarItem.hoverBackground"), self.color("modernActivityBarItem.hoverForeground"))
        } else if modern && active {
            (self.color("modernActivityBarItem.activeBackground"), self.color("modernActivityBarItem.activeForeground"))
        } else if hovered || active {
            (None, self.color("activityBar.foreground"))
        } else {
            (None, self.color("activityBar.inactiveForeground"))
        };
        // `.open` e `:focus` (sólido) vêm depois de `:hover` (tracejado) no arquivo, com a mesma especificidade.
        let outline_style = if active {
            Some("solid")
        } else if state.hovered {
            Some("dashed")
        } else {
            None
        };
        CompactMenuToggleStyle {
            button_background,
            background,
            color,
            outline: outline_style.map(|style| OutlineStyle { color: self.color("menubar.selectionBorder"), style, width: 1.0 }),
            font_size: modern.then(|| self.size(CODICON_FONT_SIZE)),
        }
    }
}

/// Onde um `.monaco-sash` fica na árvore: os seletores de descendente de `sashHandles.css` e os `:not(.part .monaco-sash)`
/// de `floatingPanels.css`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct SashPlacement {
    /// Dentro de um `.part`: os sashes internos de uma parte (divisões do editor, redimensionadores de árvore e de view).
    pub inside_part: bool,
    /// Dentro de um `.monaco-modal-editor-block`.
    pub inside_modal_editor_block: bool,
}

/// O `content` calculado do `::after` de um sash que desenha a alça (`content: ''`).
pub const SASH_GRIP_CONTENT_EMPTY: &str = "\"\"";

/// O `content` calculado do `::after` que as regras de alça suprimem (`content: none`).
pub const SASH_GRIP_CONTENT_NONE: &str = "none";

/// O `content` calculado de um pseudo-elemento que nenhuma regra define (`normal`).
pub const SASH_GRIP_CONTENT_NORMAL: &str = "normal";

/// A alça de três pontos de um sash (`.monaco-sash::after`, `sashHandles.css`, linhas 30 a 60): um ponto de 2px e dois
/// pontos de `box-shadow`, que lê `currentColor`.
#[derive(Clone, Debug, PartialEq)]
pub struct SashGrip {
    /// `width` e `height` do ponto central.
    pub dot_size: f64,
    /// `background-color` do ponto central.
    pub dot_color: Color,
    /// `color` do pseudo-elemento, que é a cor dos dois pontos de `box-shadow` (`currentColor`).
    pub shadow_color: Color,
    /// Os deslocamentos `(x, y)` dos pontos de `box-shadow`.
    pub shadow_offsets: Vec<(f64, f64)>,
    /// `opacity`: a alça cede ao destaque do sash enquanto ele recebe o ponteiro ou arrasta (linhas 62 a 66).
    pub opacity: f64,
}

impl<'a> ModernUiStyle<'a> {
    /// `--modern-ui-floating-card-statusbar-margin` (floatingPanels.css, linhas 24 a 34).
    fn floating_card_statusbar_margin(&self) -> f64 {
        if self.compact() { self.size(SPACING_NONE) } else { self.size(SPACING_SIZE40) }
    }

    /// `content` calculado do `::after` de um sash (sashHandles.css, linhas 30 a 87). As regras que escondem a alça
    /// (`content: none`) vencem a que a desenha por vir depois com a mesma especificidade ou por ser mais específica; a
    /// regra do compacto e a que desenha exigem o sash habilitado, as demais valem também para o sash desabilitado.
    pub fn sash_grip_content(&self, sash: &ClassList, placement: SashPlacement) -> &'static str {
        if !self.has("modern-ui") {
            return SASH_GRIP_CONTENT_NORMAL;
        }
        let disabled = sash.contains("disabled");
        let suppressed = placement.inside_part
            || placement.inside_modal_editor_block
            || (!disabled && self.has("modern-ui-compact"))
            || (self.has("nopanel") && sash.contains("horizontal"))
            || (self.has("nosidebar") && self.has("noactivitybar") && sash.contains("vertical") && sash.contains("primary-sidebar-sash"));
        if suppressed {
            SASH_GRIP_CONTENT_NONE
        } else if disabled {
            SASH_GRIP_CONTENT_NORMAL
        } else {
            SASH_GRIP_CONTENT_EMPTY
        }
    }

    /// A alça de um sash (sashHandles.css, linhas 30 a 66). `None` quando o `::after` não desenha nada. A cor vem de
    /// `modernSash.gripForeground`; uma variável sem cor deixa o fundo e a cor no padrão (transparente).
    pub fn sash_grip(&self, sash: &ClassList, placement: SashPlacement) -> Option<SashGrip> {
        if self.sash_grip_content(sash, placement) != SASH_GRIP_CONTENT_EMPTY {
            return None;
        }
        let color = self.chain(&["modernSash.gripForeground"]);
        let shadow_offsets = if sash.contains("vertical") {
            vec![(0.0, -5.0), (0.0, 5.0)]
        } else if sash.contains("horizontal") {
            vec![(-5.0, 0.0), (5.0, 0.0)]
        } else {
            Vec::new()
        };
        Some(SashGrip {
            dot_size: 2.0,
            dot_color: color.clone(),
            shadow_color: color,
            shadow_offsets,
            opacity: if sash.contains("hover") || sash.contains("active") { 0.0 } else { 1.0 },
        })
    }

    /// `top` e `bottom` calculados do `::before` (o destaque) de um sash vertical da grade (floatingPanels.css, linhas 601
    /// a 622): o destaque acompanha as margens dos cartões que o sash divide. `None` fora do `.floating-panels`, nos sashes
    /// internos de uma parte e nos sashes horizontais.
    pub fn vertical_sash_highlight_insets(&self, sash: &ClassList, placement: SashPlacement) -> Option<(f64, f64)> {
        if !self.has("floating-panels") || placement.inside_part || !sash.contains("vertical") {
            return None;
        }
        let top = if self.has("top-window-edge") { self.floating_card_outer_margin() } else { self.floating_card_inner_margin() };
        // Sem barra de status e sem painel inferior, os cartões encostam na borda de baixo da janela e levam a margem externa.
        let bottom = if self.has("nostatusbar") && (!self.has("panel-position-bottom") || self.has("nopanel")) {
            self.floating_card_outer_margin()
        } else {
            self.floating_card_statusbar_margin()
        };
        Some((top, bottom))
    }

    /// `--window-corner-radius` (style.css, linhas 101 a 108): o raio da moldura nativa, fixo em coordenadas de tela, então
    /// o zoom o divide.
    fn window_corner_radius(&self) -> f64 {
        let native = if self.has("macos-tahoe") { 16.0 } else { 10.0 };
        native / self.window_zoom_factor
    }

    /// `--modern-ui-window-corner-radius` (floatingPanels.css, linhas 537 a 539): o raio concêntrico com a moldura nativa
    /// do macOS, descontados a borda da janela (`.border:not(.fullscreen)`, style.css linhas 95 a 99) e a margem externa
    /// do cartão. `None` fora dos casos em que a regra vale: macOS com cartões flutuantes e sem barra de status, nem na
    /// web, nem em tela cheia, nem no compacto.
    fn exposed_window_corner_radius(&self) -> Option<f64> {
        let applies = self.has("monaco-workbench")
            && self.has("mac")
            && self.has("floating-panels")
            && self.has("nostatusbar")
            && !self.has("web")
            && !self.has("fullscreen")
            && !self.has("modern-ui-compact");
        applies.then(|| {
            let border_width = if self.has("border") { 1.0 } else { 0.0 };
            (self.window_corner_radius() - border_width - self.floating_card_outer_margin()).max(0.0)
        })
    }

    /// Os cantos de baixo voltados à moldura do macOS (floatingPanels.css, linhas 541 a 561): `inset` é o que o elemento
    /// desconta do raio da janela (o traço do cartão, nos elementos de dentro dele).
    fn with_exposed_bottom_corners(&self, mut radii: CornerRadii, bottom_left: bool, bottom_right: bool, inset: f64) -> CornerRadii {
        if let Some(window_radius) = self.exposed_window_corner_radius() {
            let radius = (window_radius - inset).max(0.0);
            if bottom_left {
                radii.3 = radius;
            }
            if bottom_right {
                radii.2 = radius;
            }
        }
        radii
    }

    /// `border-radius` de uma barra lateral, painel ou barra lateral secundária flutuante (floatingPanels.css, linhas 49,
    /// 65 a 73, 79 e 123 a 127): a barra lateral primária encontra a trilha da barra de atividade sem canto do lado dela.
    fn floating_card_corner_radii(&self, part: &ClassList) -> Option<CornerRadii> {
        if !self.has("floating-panels") {
            return None;
        }
        if self.has("modern-ui-compact") {
            return Some(CornerRadii::uniform(0.0));
        }
        let mut radii = CornerRadii::uniform(self.size(CORNER_RADIUS_LARGE));
        if part.contains("sidebar") && !self.has("noactivitybar") {
            if part.contains("left") {
                radii.0 = 0.0;
                radii.3 = 0.0;
            } else if part.contains("right") {
                radii.1 = 0.0;
                radii.2 = 0.0;
            }
        }
        Some(radii)
    }

    /// `border-radius` do `.part.editor` (editorBorder.css, linhas 26 a 36). `None` sem o Modern UI e no editor modal.
    fn editor_part_corner_radii(&self, part: &ClassList) -> Option<CornerRadii> {
        if !self.has("modern-ui") || part.contains("modal-editor-part") {
            return None;
        }
        Some(CornerRadii::uniform(if self.compact() { 0.0 } else { self.size(CORNER_RADIUS_LARGE) }))
    }

    /// `border-radius` de uma parte flutuante (barra de atividade, barra lateral, painel, barra lateral secundária ou
    /// editor), com os cantos de baixo que ficam à vista da moldura nativa do macOS acompanhando a curva dela
    /// (floatingPanels.css, linhas 537 a 562). `None` quando nenhuma regra de raio do Modern UI alcança a parte.
    pub fn part_corner_radii(&self, part: &ClassList) -> Option<CornerRadii> {
        let radii = if part.contains("activitybar") {
            self.activity_bar_corner_radii(part)?
        } else if part.any(&["sidebar", "auxiliarybar", "panel"]) {
            self.floating_card_corner_radii(part)?
        } else if part.contains("editor") {
            self.editor_part_corner_radii(part)?
        } else {
            return None;
        };
        let rail = part.contains("activitybar");
        let outer_bottom = part.contains("floating-part-outer-bottom");
        let editor_outer_bottom = part.contains("editor") && part.contains("floating-editor-outer-bottom");
        let bottom_left = (rail && part.contains("left"))
            || (outer_bottom && part.contains("floating-part-outer-left"))
            || (editor_outer_bottom && part.contains("floating-editor-outer-left"));
        let bottom_right = (rail && part.contains("right"))
            || (outer_bottom && part.contains("floating-part-outer-right"))
            || (editor_outer_bottom && part.contains("floating-editor-outer-right"));
        Some(self.with_exposed_bottom_corners(radii, bottom_left, bottom_right, 0.0))
    }

    /// `border-radius` do `> .content` de um `.part.editor` (floatingPanels.css, linhas 104 a 106, 123 a 127 e 553 a 561): o
    /// conteúdo fica um traço de cartão para dentro, então o canto exposto perde esse traço. `None` fora do
    /// `.floating-panels`.
    pub fn editor_content_corner_radii(&self, editor: &ClassList) -> Option<CornerRadii> {
        if !self.has("floating-panels") || !editor.contains("editor") {
            return None;
        }
        let radii = CornerRadii::uniform(if self.has("modern-ui-compact") { 0.0 } else { self.size(CORNER_RADIUS_LARGE) });
        let outer_bottom = editor.contains("floating-editor-outer-bottom");
        Some(self.with_exposed_bottom_corners(
            radii,
            outer_bottom && editor.contains("floating-editor-outer-left"),
            outer_bottom && editor.contains("floating-editor-outer-right"),
            self.size(STROKE_THICKNESS),
        ))
    }

    /// `border-radius` de um `.webview-overlay-content` (floatingPanels.css, linhas 118 a 120, 123 a 127 e 553 a 561): o
    /// webview modal mantém o raio do cartão no compacto e não acompanha a moldura do macOS. `None` fora do
    /// `.floating-panels`.
    pub fn webview_overlay_corner_radii(&self, overlay: &ClassList) -> Option<CornerRadii> {
        if !self.has("floating-panels") || !overlay.contains("webview-overlay-content") {
            return None;
        }
        let modal = overlay.contains("webview-overlay-modal");
        let radii = CornerRadii::uniform(if self.has("modern-ui-compact") && !modal { 0.0 } else { self.size(CORNER_RADIUS_LARGE) });
        let outer_bottom = !modal && overlay.contains("webview-overlay-outer-bottom");
        Some(self.with_exposed_bottom_corners(
            radii,
            outer_bottom && overlay.contains("webview-overlay-outer-left"),
            outer_bottom && overlay.contains("webview-overlay-outer-right"),
            self.size(STROKE_THICKNESS),
        ))
    }
}
