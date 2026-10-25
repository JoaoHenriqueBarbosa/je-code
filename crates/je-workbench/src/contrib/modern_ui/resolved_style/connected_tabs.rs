//! As abas conectadas do editor (`modernUI/browser/media/connectedEditorTabs.css`, com as regras de `tabs.css` e de
//! `multieditortabscontrol.css` que ela cobre) e o participante de tema de `connectedEditorTabs.ts`.
//!
//! Divergências:
//! - **Sem cascata de CSS.** As variáveis `--modern-ui-*` que as regras declaram em cada nível (a raiz, o `.title.tabs`, a
//!   aba) são funções do nível: [`ModernUiStyle::tab_variable`] devolve o valor que o `getComputedStyle` leria no elemento
//!   descrito por um [`TabElement`] (as classes dele e o grupo `.editor-group-container.active`). A precedência (especificidade
//!   e ordem) já está resolvida na ordem dos ramos, comentada com a linha do `.css`.
//! - **Participantes de tema.** `--modern-ui-connected-tab-surface` e as variáveis `--modern-ui-connected-tab-strip-*` (o
//!   `registerThemingParticipant` de `connectedEditorTabs.ts`) e as `--modern-ui-editor-tab-custom-*` (o de
//!   `modernTabColorCustomizations.ts`, em `services::themes`) só existem quando o DOM recebeu o CSS do tema, ou seja, fora de
//!   [`ModernUiStyle::with_explicit_colors_only`]. O que o DOM escreve direto na raiz (`set_root_variable`) vence a folha.
//! - **Alto contraste.** O contorno fechado do `.tab-fill` em alto contraste (linhas 96 a 100, 746 a 748, 759 a 769 e 802 a
//!   805) é do `high_contrast_connected_tab_fill_stroke` (`multi_editor_tabs_control.rs`), que o desenho da aba usa;
//!   [`ModernUiStyle::connected_tab_fill_border_top_color`] devolve `None` nesse caso, menos na aba ativa fora da linha de
//!   cima, que a linha 330 deixa transparente em qualquer contraste.
//! - **Geometria nativa.** [`ModernUiStyle::tab_box_context`] monta o `TabBoxContext` de `multi_editor_tabs_control.rs` (a
//!   geometria que o desenho da aba usa: `tab_fill_rect`, `tab_action_geometry`) com as classes da raiz e da aba e os
//!   tamanhos do tema, sem repetir as fórmulas.

use je_theme::color::{Color, Srgba};
use je_theme::sizes::base_sizes::{
    CORNER_RADIUS_LARGE, CORNER_RADIUS_SMALL, SPACING_SIZE160, SPACING_SIZE20, SPACING_SIZE200, SPACING_SIZE240, SPACING_SIZE280, SPACING_SIZE40, SPACING_SIZE60, SPACING_SIZE80, STROKE_THICKNESS,
};

use super::{BoxEdges, ClassList, CornerRadii, EditorTabState, HorizontalEdges, ModernUiStyle, css_rgb};
use crate::browser::parts::editor::multi_editor_tabs_control::{
    ModernSpacing, StrokeSide, TabBoxContext, TabCssClasses, TabFillStrokeInput, TabStrokeColor, high_contrast_connected_tab_fill_stroke,
};
use crate::common::theme::{EDITOR_GROUP_HEADER_CONNECTED_TABS_BACKGROUND, EDITOR_GROUP_HEADER_TABS_BACKGROUND, EDITOR_GROUP_HEADER_TABS_BORDER};
use crate::services::themes::modern_tab_color_customizations::collect_modern_tab_color_customizations;

/// `z-index` do `.scrollbar` da barra de abas (`multieditortabscontrol.css`, linha 70, e `tabs.css`, linha 753): acima das
/// abas fixadas, que ficam em 8 e, a ativa, em 9.
pub const EDITOR_TABS_SCROLLBAR_Z_INDEX: i32 = 11;

/// Uma aba do editor (`.tab`) dentro de um grupo (`.editor-group-container`), com o que as regras testam nela e as
/// propriedades personalizadas que o controle das abas escreve no próprio elemento.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct TabElement {
    /// As classes da aba (`tab`, `active`, `selected`, `multi-selected`, `connected-tab-upper-row`, `tab-border-top`,
    /// `tab-border-bottom`, `dirty-border-top`...).
    pub classes: ClassList,
    /// O grupo da aba é o `.editor-group-container.active`.
    pub group_active: bool,
    /// `:hover` na aba.
    pub hovered: bool,
    /// `:focus` e `:focus-visible` na aba (o foco de teclado). Uma aba que não foi focada não os tem.
    pub focused: bool,
    pub focus_visible: bool,
    /// `--tab-border-top-color`, escrita na aba.
    pub border_top_color: Option<Color>,
    /// `--tab-border-bottom-color`, escrita na aba.
    pub border_bottom_color: Option<Color>,
    /// `--tab-dirty-border-top-color`, escrita na aba.
    pub dirty_border_top_color: Option<Color>,
    /// A aba está na primeira fileira de um `.title.two-tab-bars` (`.tabs-and-actions-container:first-child`).
    pub first_of_two_tab_bars: bool,
    /// `:focus-within` na aba (uma ação dela tem o foco).
    pub focus_within: bool,
    /// O `.tabs-container` da aba tem `.disable-sticky-tabs`.
    pub disable_sticky_tabs: bool,
    /// As classes do `.title` da aba (`tabs`, `show-file-icons`, `tab-actions-reserve-space`...).
    pub title: ClassList,
    /// A aba está em `.part.editor > .content .editor-group-container > .title`, o caminho que as regras de `tabs.css` e
    /// `connectedEditorTabs.css` exigem para as ações da aba e para o recuo dos ícones.
    pub in_editor_group: bool,
}

/// Onde a aba está na fileira e quanto ela mede: o que o contêiner (`.title.tabs` e `.tabs-and-actions-container`) e o layout
/// dão à geometria dela, além das classes da aba e da raiz.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct TabRowPlacement {
    /// `.title.tabs.compact-height` (a aba de 20px, não a de 24px; `tabs.css`, linhas 75 e 85).
    pub compact_height: bool,
    /// `.tabs-and-actions-container.wrapping` e `.connected-tab-upper-bar`.
    pub wrapping: bool,
    pub upper_bar: bool,
    /// A aba abre a fileira (`:first-child` ou depois de `.last-in-row`).
    pub starts_row: bool,
    /// `.title.two-tab-bars` e `.tabs-and-actions-container:first-child` (a barra das abas fixadas).
    pub two_tab_bars: bool,
    pub first_tab_bar: bool,
    /// `getBoundingClientRect().width` da aba.
    pub tab_width: f64,
}

impl TabElement {
    /// Uma aba com as classes de `class_name`, num grupo ativo ou não.
    pub fn new(class_name: &str, group_active: bool) -> Self {
        Self { classes: ClassList::new(class_name), group_active, ..Self::default() }
    }

    /// `--tab-border-top-color` e `--tab-border-bottom-color` da aba.
    pub fn with_border_colors(mut self, top: Option<Color>, bottom: Option<Color>) -> Self {
        self.border_top_color = top;
        self.border_bottom_color = bottom;
        self
    }

    /// O `.title` da aba, com as classes de `title_class_name`, e se ele está no caminho `.content .editor-group-container`.
    pub fn with_title(mut self, title_class_name: &str, in_editor_group: bool) -> Self {
        self.title = ClassList::new(title_class_name);
        self.in_editor_group = in_editor_group;
        self
    }

    fn has(&self, class_name: &str) -> bool {
        self.classes.contains(class_name)
    }
}

/// A cascata de uma propriedade: vence a declaração de maior especificidade e, no empate, a mais tardia na ordem de carga.
/// `None` quando nenhuma regra a declara.
fn cascade<T>(declarations: impl IntoIterator<Item = (u32, T)>) -> Option<T> {
    let mut winner: Option<(u32, T)> = None;
    for (specificity, value) in declarations {
        if winner.as_ref().is_none_or(|(best, _)| specificity >= *best) {
            winner = Some((specificity, value));
        }
    }
    winner.map(|(_, value)| value)
}

impl From<EditorTabState> for TabElement {
    fn from(state: EditorTabState) -> Self {
        let mut class_names = vec!["tab"];
        if state.active {
            class_names.push("active");
        }
        if state.selected {
            class_names.push("selected");
        }
        if state.close_action_off {
            class_names.push("close-action-off");
        }
        Self { classes: ClassList::new(&class_names.join(" ")), group_active: state.group_active, hovered: state.hovered, ..Self::default() }
    }
}

/// Qual dos dois indicadores de borda da aba (`.tab-border-top-container` ou `.tab-border-bottom-container`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TabBorderIndicator {
    Top,
    Bottom,
}

/// O que um indicador de borda da aba pinta: o `display` e o `background-color` calculados.
#[derive(Clone, Debug, PartialEq)]
pub struct TabBorderContainer {
    pub display: &'static str,
    pub background: Color,
}

/// `color-mix(in srgb, first first_share, second)`: a mistura com os canais pré-multiplicados pelo alfa (CSS Color 5).
fn color_mix_srgb(first: &Color, first_share: f64, second: &Color) -> Color {
    let (a, b) = (first.rgba(), second.rgba());
    let second_share = 1.0 - first_share;
    let alpha = a.a * first_share + b.a * second_share;
    if alpha <= 0.0 {
        return Color::transparent_color();
    }
    let channel = |x: u8, y: u8| ((f64::from(x) * a.a * first_share + f64::from(y) * b.a * second_share) / alpha).round();
    Color::from(Srgba::new(channel(a.r, b.r), channel(a.g, b.g), channel(a.b, b.b), alpha))
}

impl<'a> ModernUiStyle<'a> {
    /// `.monaco-workbench.modern-ui-tabs.modern-ui-connected-editor-tabs` (`CONNECTED_EDITOR_TABS_SELECTOR`).
    fn connected_tabs(&self) -> bool {
        self.has("monaco-workbench") && self.has("modern-ui-tabs") && self.has("modern-ui-connected-editor-tabs")
    }

    /// O valor de uma propriedade personalizada declarada na raiz: a que o DOM escreveu direto vence a da folha de tema.
    pub(super) fn declared(&self, name: &str) -> Option<Color> {
        self.root_variables.get(name).cloned().or_else(|| self.participant_variable(name))
    }

    /// As declarações dos participantes de tema na raiz, que só existem quando o DOM recebeu o CSS do tema.
    fn participant_variable(&self, name: &str) -> Option<Color> {
        if self.explicit_colors_only {
            return None;
        }
        // `.modern-ui-tabs.monaco-workbench.monaco-workbench { ... }` (modernTabColorCustomizations.ts, linha 144).
        if self.has("monaco-workbench") && self.has("modern-ui-tabs") {
            if let Some(color) = collect_modern_tab_color_customizations(self.theme).remove(name) {
                return Some(color);
            }
        }
        self.connected_participant_variable(name)
    }

    /// O `registerThemingParticipant` de `connectedEditorTabs.ts` (linhas 15 a 41).
    fn connected_participant_variable(&self, name: &str) -> Option<Color> {
        if !self.connected_tabs() {
            return None;
        }
        let background = self.color("editor.background");
        let opaque_over_editor = |color: &Color| background.as_ref().map_or_else(|| color.clone(), |background| color.make_opaque(background));
        let default_strip_color = self.color(EDITOR_GROUP_HEADER_TABS_BACKGROUND).or_else(|| background.clone());
        let default_strip = default_strip_color.as_ref().map(opaque_over_editor);
        let editor_strip = self.color(EDITOR_GROUP_HEADER_CONNECTED_TABS_BACKGROUND).or(default_strip_color).as_ref().map(opaque_over_editor);
        // A regra com `.modern-ui` (linhas 36 e 39) tem mais especificidade que a geral (linhas 24 e 30).
        let strip = if self.has("modern-ui") { editor_strip.or(default_strip) } else { default_strip };
        match name {
            "--modern-ui-connected-tab-surface" => background.clone(),
            "--modern-ui-connected-tab-strip-background" => strip,
            "--modern-ui-connected-tab-upper-hover-background" => {
                let hover = self.color("modernEditorTab.hoverBackground")?;
                // Achata contra a fileira, não contra o documento, para a máscara das ações e a pílula pintarem a mesma superfície.
                strip.map(|strip| hover.make_opaque(&strip))
            }
            _ => None,
        }
    }

    /// `--modern-ui-connected-tab-surface`: o fundo do editor, em que a aba ativa e a superfície se unem.
    pub fn connected_tab_surface(&self) -> Option<Color> {
        self.declared("--modern-ui-connected-tab-surface")
    }

    /// `--modern-ui-editor-tab-custom-<suffix>`.
    fn tab_custom(&self, suffix: &str) -> Option<Color> {
        self.declared(&format!("--modern-ui-editor-tab-custom-{suffix}"))
    }

    /// `var(--modern-ui-connected-tab-strip-background, var(--vscode-editorGroupHeader-tabsBackground))`.
    fn strip_background(&self) -> Option<Color> {
        self.declared("--modern-ui-connected-tab-strip-background").or_else(|| self.color(EDITOR_GROUP_HEADER_TABS_BACKGROUND))
    }

    /// `color-mix(in srgb, var(--vscode-foreground) 6%, <fileira>)` (linhas 108 e 109).
    fn strip_hover_mix(&self) -> Option<Color> {
        let foreground = self.color("foreground")?;
        let strip = self.strip_background()?;
        Some(color_mix_srgb(&foreground, 0.06, &strip))
    }

    /// O valor de uma propriedade personalizada `--modern-ui-*` no elemento da aba (ou no `.title.tabs`, que é uma aba sem
    /// classes), nas abas conectadas (`connectedEditorTabs.css`, linhas 20 a 33, 102 a 143, 364 a 370 e 694 a 700). Fora
    /// das abas conectadas, é a variável da raiz ([`ModernUiStyle::variable`]).
    pub fn tab_variable(&self, name: &str, tab: &TabElement) -> Option<Color> {
        if !self.connected_tabs() {
            return self.variable(name);
        }
        let upper_row = tab.has("connected-tab-upper-row");
        let group_active = tab.group_active;
        match name {
            "--modern-ui-connected-tab-surface" => self.connected_tab_surface(),
            // Linhas 104 e 131.
            "--modern-ui-connected-tab-active-background" => {
                let suffix = if group_active { "active-background" } else { "unfocused-active-background" };
                self.tab_custom(suffix).or_else(|| self.connected_tab_surface())
            }
            // Linhas 105 e 132.
            "--modern-ui-connected-tab-active-hover-background" => {
                let suffix = if group_active { "active-hover-background" } else { "unfocused-active-hover-background" };
                self.tab_custom(suffix).or_else(|| self.tab_variable("--modern-ui-connected-tab-active-background", tab))
            }
            // Linhas 106 e 107.
            "--modern-ui-editor-tab-inactive-background" => self.tab_custom("inactive-background").or_else(|| self.strip_background()),
            "--modern-ui-editor-tab-unfocused-inactive-background" => self.tab_custom("unfocused-inactive-background").or_else(|| self.strip_background()),
            // Linhas 108 e 696: a pílula de cima mistura o primeiro plano na fileira de cima.
            "--modern-ui-editor-tab-hover-background" => self.tab_custom("hover-background").or_else(|| {
                if upper_row { self.declared("--modern-ui-connected-tab-upper-hover-background") } else { self.strip_hover_mix() }
            }),
            // Linhas 109 e 697.
            "--modern-ui-editor-tab-unfocused-hover-background" => self.tab_custom("unfocused-hover-background").or_else(|| {
                if upper_row { self.tab_variable("--modern-ui-editor-tab-hover-background", tab) } else { self.strip_hover_mix() }
            }),
            // Linhas 110 e 698.
            "--modern-ui-editor-tab-action-hover-background" => {
                self.tab_custom("action-hover-background").or_else(|| self.tab_variable("--modern-ui-editor-tab-hover-background", tab))
            }
            // Linhas 111 e 699.
            "--modern-ui-editor-tab-action-unfocused-hover-background" => {
                self.tab_custom("action-hover-background").or_else(|| self.tab_variable("--modern-ui-editor-tab-unfocused-hover-background", tab))
            }
            "--modern-ui-connected-tab-border" => self.connected_tab_border(tab),
            // Declaradas só pelos participantes de tema de `connectedEditorTabs.ts` (linhas 24 a 40), na raiz.
            "--modern-ui-connected-tab-strip-background" | "--modern-ui-connected-tab-upper-hover-background" => self.declared(name),
            _ => self.variable(name),
        }
    }

    /// `--modern-ui-connected-tab-border` (linhas 20, 26, 32, 365 e 369): o traço que une a aba ativa à superfície.
    fn connected_tab_border(&self, tab: &TabElement) -> Option<Color> {
        let high_contrast = self.high_contrast();
        if tab.has("active") && tab.has("multi-selected") && !tab.has("connected-tab-upper-row") {
            let contrast_active_border = || self.color("contrastActiveBorder");
            return if high_contrast { contrast_active_border() } else { tab.border_top_color.clone().or_else(contrast_active_border) };
        }
        if high_contrast {
            return if tab.group_active { self.color("focusBorder") } else { self.color("contrastBorder") };
        }
        self.tab_custom("border")
            .or_else(|| self.color(EDITOR_GROUP_HEADER_TABS_BORDER))
            .or_else(|| self.color("tab.border"))
            .or_else(|| self.connected_tab_surface())
    }

    /// `background-color` do `.tab-fill` de uma aba conectada (`tabs.css`, linhas 377 a 455, e `connectedEditorTabs.css`,
    /// linhas 322 a 335): a aba ativa pinta `--modern-ui-connected-tab-current-background` em qualquer contraste; as demais
    /// usam as variáveis do grupo (focadas ou não) fora do alto contraste. `None` fora das abas conectadas.
    pub fn connected_tab_fill_background(&self, tab: &TabElement) -> Option<Color> {
        if !self.connected_tabs() {
            return None;
        }
        if tab.has("active") {
            let name = if tab.hovered { "--modern-ui-connected-tab-active-hover-background" } else { "--modern-ui-connected-tab-active-background" };
            return self.tab_variable(name, tab);
        }
        if self.high_contrast() {
            return None;
        }
        if tab.has("selected") {
            return self.color("tab.selectedBackground");
        }
        let name = match (tab.hovered, tab.group_active) {
            (true, true) => "--modern-ui-editor-tab-hover-background",
            (true, false) => "--modern-ui-editor-tab-unfocused-hover-background",
            (false, true) => "--modern-ui-editor-tab-inactive-background",
            (false, false) => "--modern-ui-editor-tab-unfocused-inactive-background",
        };
        self.tab_variable(name, tab)
    }

    /// `border-top-color` do `.tab-fill` de uma aba conectada fora do alto contraste. `None` fora das abas conectadas, em
    /// alto contraste (menos na aba ativa fora da linha de cima, abaixo) e quando a variável da regra não tem cor.
    pub fn connected_tab_fill_border_top_color(&self, tab: &TabElement) -> Option<Color> {
        if !self.connected_tabs() {
            return None;
        }
        if self.high_contrast() {
            // Linha 330: a aba ativa comum tem o contorno transparente em qualquer contraste (a aresta desenha o traço). O
            // contorno fechado das demais abas em alto contraste é do `high_contrast_connected_tab_fill_stroke`.
            return (tab.has("active") && !tab.has("connected-tab-upper-row")).then(Color::transparent_color);
        }
        self.connected_tab_fill_border_color(tab, StrokeSide::Top)
    }

    /// `--modern-ui-editor-tab-hover-border` e `--modern-ui-editor-tab-unfocused-hover-border` (`tabs.css`, linhas 47 e 48):
    /// `transparent` sem o participante de tema, e a do grupo sem foco é um alias da outra.
    fn tab_hover_border(&self, group_active: bool) -> Color {
        let hover = self.declared("--modern-ui-editor-tab-hover-border");
        let color = if group_active { hover } else { self.declared("--modern-ui-editor-tab-unfocused-hover-border").or(hover) };
        color.unwrap_or_else(Color::transparent_color)
    }

    /// `border-<lado>-color` do `.tab-fill` de uma aba conectada fora do alto contraste: a cascata de todas as regras que
    /// pintam a borda do preenchimento, aplicadas da menor para a maior especificidade (e, no empate, da ordem de carga: o
    /// `connectedEditorTabs.css` vem antes do `tabs.css`, na ordem dos `import`s de `modernUI.contribution.ts`). `None` é a
    /// cor `currentColor` (a variável da regra não existe). Sob `.hc-black` e `.hc-light` vale o
    /// `high_contrast_connected_tab_fill_stroke`.
    fn connected_tab_fill_border_color(&self, tab: &TabElement, side: StrokeSide) -> Option<Color> {
        let (active, selected, upper_row) = (tab.has("active"), tab.has("selected"), tab.has("connected-tab-upper-row"));
        let (tab_border_top, dirty_border_top, multi_selected) = (tab.has("tab-border-top"), tab.has("dirty-border-top"), tab.has("multi-selected"));
        let top = matches!(side, StrokeSide::Top);
        // `--modern-ui-connected-tab-top-accent` da aba (`connectedEditorTabs.css`, linha 755).
        let accent = if dirty_border_top { tab.dirty_border_top_color.clone() } else { None };
        // `tabs.css`, linha 156: o traço de `--modern-ui-editor-tab-border` nos quatro lados.
        let mut color = Some(self.editor_tab_border());
        // `tabs.css`, linhas 459 a 467: o traço da aba selecionada com `.tab-border-top` é `tab.selectedBorderTop` nos quatro
        // lados (`--tab-border-top-color` com `!important` no próprio preenchimento).
        if selected && !active && tab_border_top {
            color = self.color("tab.selectedBorderTop");
        }
        // `connectedEditorTabs.css`, linhas 330 e 710: a aba ativa e a pílula de cima têm o contorno transparente (a aresta
        // da aba ativa desenha o traço).
        if active || upper_row {
            color = Some(Color::transparent_color());
        }
        // `tabs.css`, linhas 387 a 397: o sublinhado de `:hover` da aba inativa sem seleção (o `tabs.css` carrega depois do
        // `connectedEditorTabs.css`, então vence a linha 710 no empate de especificidade).
        if tab.hovered && !active && !selected && matches!(side, StrokeSide::Bottom) {
            color = Some(self.tab_hover_border(tab.group_active));
        }
        // Linha 771: o acento sujo pinta a borda de cima da pílula de cima e das abas inativas.
        if dirty_border_top && (upper_row || !active) && top {
            color = accent.clone();
        }
        // Linhas 240 a 246: as abas inativas comuns (sem seleção e fora da pílula de cima) não têm contorno, e a regra tem mais
        // especificidade que a do acento sujo (o acento fica no gradiente de fundo).
        if !selected && !active && !upper_row {
            color = Some(Color::transparent_color());
        }
        // Linhas 338 a 344: só a pílula de cima ativa pinta o acento explícito de cima.
        if active && upper_row && tab_border_top && !dirty_border_top && top {
            let suffix = if tab.group_active { "active-border-top" } else { "unfocused-active-border-top" };
            color = Some(self.tab_custom(suffix).unwrap_or_else(Color::transparent_color));
        }
        // Linhas 797 a 800: a pílula de cima da aba ativa com várias seleções pinta os quatro lados.
        if active && upper_row && multi_selected {
            let border = tab.border_top_color.clone().or_else(|| self.color("contrastActiveBorder"));
            color = if top { accent.or(border) } else { border };
        }
        color
    }

    /// `--modern-ui-editor-tab-border` (`tabs.css`, linha 41, e o participante de `tab.border`).
    fn editor_tab_border(&self) -> Color {
        self.declared("--modern-ui-editor-tab-border").unwrap_or_else(Color::transparent_color)
    }

    /// `border-top-color` do `.tab-connected-edge` da aba ativa (linhas 346 a 354, 414 e 785 a 786). `None` quando o
    /// elemento não é desenhado: fora das abas conectadas, na aba que não é a ativa e na pílula de cima (linha 845).
    pub fn connected_edge_border_top_color(&self, tab: &TabElement) -> Option<Color> {
        if !self.connected_tabs() || !tab.has("active") || tab.has("connected-tab-upper-row") {
            return None;
        }
        if tab.has("dirty-border-top") {
            return tab.dirty_border_top_color.clone();
        }
        if tab.has("tab-border-top") && !tab.has("multi-selected") {
            let suffix = if tab.group_active { "active-border-top" } else { "unfocused-active-border-top" };
            if let Some(color) = self.tab_custom(suffix) {
                return Some(color);
            }
        }
        self.tab_variable("--modern-ui-connected-tab-border", tab)
    }

    /// `--editor-tab-border-indicator-display` fora das abas conectadas: `none` com `.modern-ui-tabs` (`tabs.css`, linha 56)
    /// e o `block` padrão de `multieditortabscontrol.css` sem ele.
    fn border_indicator_display(&self) -> &'static str {
        if self.has("modern-ui-tabs") { "none" } else { "block" }
    }

    /// O `display` e o `background-color` de `.tab-border-top-container` e `.tab-border-bottom-container`
    /// (`multieditortabscontrol.css`, linhas 326 a 362, e `connectedEditorTabs.css`, linhas 17 a 22, 356 a 358, 793 a 795 e
    /// 813 a 815). O topo some nas abas conectadas (a aresta desenha o acento); a base aparece na aba ativa com
    /// `.tab-border-bottom`, exceto na pílula de cima.
    pub fn tab_border_container(&self, tab: &TabElement, indicator: TabBorderIndicator) -> TabBorderContainer {
        let active = tab.has("active");
        let connected = self.connected_tabs();
        let (display, background) = match indicator {
            TabBorderIndicator::Top => {
                let colored = (active || tab.has("selected")) && tab.has("tab-border-top");
                let dirty = tab.has("dirty-border-top");
                // A regra de `.active`/`.selected` (linha 343) tem mais especificidade que a do acento sujo (linha 358).
                let background = if colored {
                    tab.border_top_color.clone()
                } else if dirty {
                    tab.dirty_border_top_color.clone()
                } else {
                    None
                };
                let display = if connected || !(colored || dirty) { "none" } else { self.border_indicator_display() };
                (display, background)
            }
            TabBorderIndicator::Bottom => {
                let shown = active && tab.has("tab-border-bottom") && !tab.first_of_two_tab_bars;
                let display = if !shown || (connected && tab.has("connected-tab-upper-row")) {
                    "none"
                } else if connected {
                    "block"
                } else {
                    self.border_indicator_display()
                };
                (display, if shown { tab.border_bottom_color.clone() } else { None })
            }
        };
        TabBorderContainer { display, background: background.unwrap_or_else(Color::transparent_color) }
    }

    /// `background-color` da fileira de abas (`.tabs-and-actions-container`, linhas 145 a 150), da máscara das abas fixas
    /// (`.sticky-tabs-background`, linhas 264 a 266) e do fundo do `::before` do botão de nova aba (linha 584): todos leem
    /// `var(--modern-ui-connected-tab-strip-background, var(--vscode-editorGroupHeader-tabsBackground))`, então a fileira,
    /// a máscara e o apoio do botão pintam a mesma superfície. `None` fora das abas conectadas e sem a cor da fileira.
    pub fn connected_tab_strip_background(&self) -> Option<Color> {
        if !self.connected_tabs() {
            return None;
        }
        self.strip_background()
    }

    /// `background-color` do `.tabs-bar-add-tab`: transparente nas abas conectadas (linhas 268 a 270; o apoio é o
    /// `::before`), o fundo do editor fora delas (`tabs.css`, linha 348).
    pub fn add_tab_background(&self) -> Color {
        if self.connected_tabs() {
            return Color::transparent_color();
        }
        self.chain(&["editor.background"])
    }

    /// A cor do traço que une a aba ativa ao resto da superfície nas pontas dos ombros (`.tab-fill::before` com
    /// `border-right` e `::after` com `border-left`, linhas 373 a 386, 391 e 481) e a da linha sob a fileira
    /// (`.tabs-and-actions-container::after`, linhas 286 a 289): `--modern-ui-connected-tab-border` do elemento.
    /// `None` fora das abas conectadas.
    pub fn connected_tab_separator_color(&self, tab: &TabElement) -> Option<Color> {
        if !self.connected_tabs() {
            return None;
        }
        self.connected_tab_border(tab)
    }

    /// `background-color` do `.tab-actions`. Só o alto contraste pinta o fundo, e só sob o ponteiro ou o foco
    /// (`tabs.css`, linhas 920 a 923, `!important`): a cadeia começa em `--modern-ui-editor-tab-action-overlay-background`,
    /// que as abas conectadas declaram `transparent` (linha 112) para a máscara não cobrir a pílula. As demais abas caem
    /// no fundo do cabeçalho.
    pub fn tab_actions_background(&self, tab: &TabElement, actions_focus_within: bool) -> Color {
        let masks = self.has("monaco-workbench") && self.has("modern-ui-tabs") && self.high_contrast() && (tab.hovered || actions_focus_within);
        if !masks || self.connected_tabs() {
            return Color::transparent_color();
        }
        self.chain(&[EDITOR_GROUP_HEADER_TABS_BACKGROUND, "editor.background"])
    }

    /// `pointer-events` do `.tab-actions` e, por herança, do `.action-label` dentro dele: a cascata de
    /// `multieditortabscontrol.css` (linha 547, 10 classes: `close-action-off`), de `tabs.css` (linha 518, 10 classes: a ranhura
    /// sobreposta não recebe o ponteiro; linha 583, 12: `auto` sob o ponteiro ou o foco; linha 721, 13: `auto` no título com
    /// `tab-actions-reserve-space`) e de `connectedEditorTabs.css` (linha 555, 17: a aba em repouso, sem `.active`, `.dirty`,
    /// `.sticky`, `:hover` nem `:focus-within`, deixa a ranhura fora do alvo; linha 564, 14: a ativa com a ação de fechar
    /// ligada). Sem nenhuma regra (fora do `.modern-ui-tabs` e da aba com a ação desligada) vale o padrão `auto`.
    pub fn tab_actions_pointer_events(&self, tab: &TabElement) -> &'static str {
        if !(self.has("monaco-workbench") && tab.in_editor_group) {
            return "auto";
        }
        let (active, close_action_off) = (tab.has("active"), tab.has("close-action-off"));
        let interacting = tab.hovered || tab.focus_within;
        let mut declarations: Vec<(u32, &'static str)> = Vec::new();
        if close_action_off {
            declarations.push((10, "none"));
        }
        if self.has("modern-ui-tabs") {
            declarations.push((10, "none"));
            if !close_action_off && interacting {
                declarations.push((12, "auto"));
            }
            if tab.title.contains("tab-actions-reserve-space") && !tab.has("sticky-compact") && !close_action_off {
                declarations.push((13, "auto"));
            }
            if self.connected_tabs() && tab.title.contains("tabs") {
                if active && !close_action_off {
                    declarations.push((14, "auto"));
                }
                if !active && !tab.has("dirty") && !tab.has("sticky") && !interacting {
                    declarations.push((17, "none"));
                }
            }
        }
        cascade(declarations).unwrap_or("auto")
    }

    /// `background-color` do `.tab-actions` sob o ponteiro ou o foco (`tabs.css`, linhas 580 a 606 e 645 a 649), fora do alto
    /// contraste: cada regra lê `var(--modern-ui-editor-tab-action-overlay-background, <variável do estado>)`, e o overlay
    /// (`transparent`) existe nas abas conectadas e no título com `tab-actions-reserve-space`. A variável do estado é a do
    /// grupo (ativo ou sem foco) e da aba (`active`, `:hover`, `selected`); uma variável indefinida invalida a declaração e o
    /// fundo volta a `transparent`. Em alto contraste vale a máscara `!important` de [`ModernUiStyle::tab_actions_background`].
    pub fn tab_actions_state_background(&self, tab: &TabElement) -> Color {
        let interacting = tab.hovered || tab.focus_within;
        if !(self.has("modern-ui-tabs") && tab.in_editor_group && interacting) {
            return Color::transparent_color();
        }
        if self.high_contrast() {
            return self.tab_actions_background(tab, tab.focus_within);
        }
        let (active, selected, group_active) = (tab.has("active"), tab.has("selected"), tab.group_active);
        if tab.has("close-action-off") {
            return Color::transparent_color();
        }
        let overlay = (self.connected_tabs() || tab.title.contains("tab-actions-reserve-space")).then(Color::transparent_color);
        let state = |color: Option<Color>| overlay.clone().or(color);
        let variable = |name: &str| state(self.tab_variable(name, tab));
        let mut declarations: Vec<(u32, Option<Color>)> = vec![(12, variable("--modern-ui-editor-tab-action-hover-background"))];
        if !group_active {
            declarations.push((13, variable("--modern-ui-editor-tab-action-unfocused-hover-background")));
        }
        if active {
            declarations.push((13, variable("--modern-ui-editor-tab-action-active-background")));
        }
        if active && !group_active {
            declarations.push((14, variable("--modern-ui-editor-tab-action-unfocused-active-background")));
        }
        if active && tab.hovered {
            declarations.push((13, variable("--modern-ui-editor-tab-action-active-hover-background")));
        }
        if active && tab.hovered && !group_active {
            declarations.push((14, variable("--modern-ui-editor-tab-action-unfocused-active-hover-background")));
        }
        if selected && !active {
            declarations.push((14, state(self.color("modernEditorTab.selectedActionBackground"))));
        }
        cascade(declarations).flatten().unwrap_or_else(Color::transparent_color)
    }

    /// `background-image` do `::before` do `.tab-actions` (`tabs.css`, linhas 554 a 643 e 676 a 684), fora do alto contraste e
    /// com a ação de fechar ligada: o degradê de `transparent` até a variável do estado, para a direita (`to right`) ou, com
    /// `.tab-actions-left`, para a esquerda. `none` sem regra ou com a variável indefinida.
    pub fn tab_actions_fade_gradient(&self, tab: &TabElement) -> String {
        let interacting = tab.hovered || tab.focus_within;
        if !(self.has("modern-ui-tabs") && tab.in_editor_group && interacting) || self.high_contrast() || tab.has("close-action-off") {
            return "none".to_owned();
        }
        let (active, selected, group_active) = (tab.has("active"), tab.has("selected"), tab.group_active);
        let variable = |name: &str| self.tab_variable(name, tab);
        let mut declarations: Vec<(u32, Option<Color>)> = vec![(14, variable("--modern-ui-editor-tab-action-hover-background"))];
        if !group_active {
            declarations.push((15, variable("--modern-ui-editor-tab-action-unfocused-hover-background")));
        }
        if active {
            declarations.push((15, variable("--modern-ui-editor-tab-action-active-background")));
        }
        if active && !group_active {
            declarations.push((16, variable("--modern-ui-editor-tab-action-unfocused-active-background")));
        }
        if active && tab.hovered {
            declarations.push((15, variable("--modern-ui-editor-tab-action-active-hover-background")));
        }
        if active && tab.hovered && !group_active {
            declarations.push((16, variable("--modern-ui-editor-tab-action-unfocused-active-hover-background")));
        }
        if selected && !active {
            declarations.push((16, self.color("modernEditorTab.selectedActionBackground")));
        }
        let direction = if tab.has("tab-actions-left") { "left" } else { "right" };
        match cascade(declarations).flatten() {
            Some(color) => format!("linear-gradient(to {direction}, {}, {})", css_rgb(&Color::transparent_color()), css_rgb(&color)),
            None => "none".to_owned(),
        }
    }

    /// `padding-left` e `padding-right` da `.tab` (`tabs.css`, linhas 99 a 110, 175 a 182 e 208 a 253, todas `!important`):
    /// `0 size80 0 <recuo>` com o recuo `--modern-ui-editor-tab-left-inset` (`size60`, ou `size80` sem ícones: a raiz sem
    /// `.file-icons-enabled` ou o `.title.tabs` sem `.show-file-icons`, nas abas que não são `sticky-compact` nem
    /// `tab-actions-left`); a coluna das ações (`--modern-ui-tab-action-padding`, `size280` ou a fórmula conectada) no lado da
    /// ação quando a aba é suja, fixada, ou no título com `tab-actions-reserve-space` e na aba `sizing-fit` sob o ponteiro, e
    /// zero na `sticky-compact`. `None` fora do `.modern-ui-tabs`.
    pub fn tab_padding(&self, tab: &TabElement, placement: TabRowPlacement) -> Option<HorizontalEdges> {
        if !self.has("modern-ui-tabs") {
            return None;
        }
        let (size60, size80) = (self.size(SPACING_SIZE60), self.size(SPACING_SIZE80));
        let title = &tab.title;
        let reserve = title.contains("tab-actions-reserve-space");
        let (sticky_compact, actions_left, close_action_off) = (tab.has("sticky-compact"), tab.has("tab-actions-left"), tab.has("close-action-off"));
        let (dirty, sticky, sizing_fit) = (tab.has("dirty"), tab.has("sticky"), tab.has("sizing-fit"));
        let rest = tab.has("tab-action-sizing-rest");
        // `:is(.active, :hover, :focus-within)` das regras de `.sizing-fit`, no título que não reserva o espaço das ações.
        let fit_interacting = !reserve && sizing_fit && (tab.has("active") || tab.hovered || tab.focus_within);
        let dirty_shown = dirty && !(close_action_off && tab.has("dirty-border-top"));
        let sticky_shown = sticky && !tab.has("pinned-action-off");
        let flat = !sticky_compact;
        // Linhas 107 e 108: o recuo sem ícones vale só no caminho `.content .editor-group-container > .title.tabs`.
        let iconless = tab.in_editor_group
            && title.contains("tabs")
            && ((self.has("monaco-workbench") && !self.has("file-icons-enabled")) || !title.contains("show-file-icons"));
        let inset = if iconless && flat && !actions_left { size80 } else { size60 };
        let classes = TabCssClasses { dirty, sticky, sticky_compact, ..TabCssClasses::default() };
        let action_padding = self.tab_box_context(tab, placement).action_padding(&classes);

        // As especificidades contam as classes e pseudoclasses de cada seletor, na ordem do arquivo.
        let mut left: Vec<(u32, f64)> = vec![(5, inset)];
        let mut right: Vec<(u32, f64)> = vec![(5, size80)];
        if sticky_compact {
            left.push((6, 0.0));
            right.push((6, 0.0));
        }
        if actions_left && flat {
            left.push((7, size60));
            right.push((7, size80));
        }
        if close_action_off && flat {
            left.push((7, inset));
            right.push((7, size80));
        }
        let ends_with_action = !actions_left && flat;
        if reserve && ends_with_action && !close_action_off {
            right.push((9, action_padding));
        }
        if fit_interacting && ends_with_action && !close_action_off {
            right.push((12, action_padding));
        }
        if dirty_shown && ends_with_action {
            right.push((10, action_padding));
        }
        if sticky_shown && ends_with_action {
            right.push((9, action_padding));
        }
        let starts_with_action = actions_left && flat;
        if reserve && starts_with_action && !close_action_off {
            left.push((9, action_padding));
            right.push((9, size80));
        }
        if fit_interacting && starts_with_action && !close_action_off {
            left.push((12, action_padding));
            right.push((12, size80));
        }
        if dirty_shown && starts_with_action {
            left.push((10, action_padding));
            right.push((10, size80));
        }
        if sticky_shown && starts_with_action {
            left.push((9, action_padding));
            right.push((9, size80));
        }
        if fit_interacting && rest && ends_with_action {
            right.push((12, size80));
        }
        if fit_interacting && rest && starts_with_action {
            left.push((12, inset));
        }
        Some(HorizontalEdges { left: cascade(left)?, right: cascade(right)? })
    }

    /// `z-index` de uma aba (`multieditortabscontrol.css`, linhas 241 a 255 e 277 a 291, e `connectedEditorTabs.css`, linhas
    /// 272 a 274): as abas fixadas compactas e `shrink` ficam acima das que rolam (8), e a ativa acima das demais fixadas (9). Com
    /// `.disable-sticky-tabs` o `z-index` volta a `auto` (`None`), que é também o das abas que não são fixadas assim.
    pub fn tab_z_index(&self, tab: &TabElement) -> Option<i32> {
        let sized = tab.has("sizing-fit") || tab.has("sizing-shrink") || tab.has("sizing-fixed");
        let pinned = sized && (tab.has("sticky-compact") || tab.has("sticky-shrink"));
        if !pinned || tab.disable_sticky_tabs {
            return None;
        }
        let raised = self.connected_tabs() && tab.has("active") && tab.has("sticky") && !tab.has("connected-tab-upper-row");
        Some(if raised { 9 } else { 8 })
    }

    /// `border-*-width` do `.tab-fill` (`tabs.css`, linha 156, e `connectedEditorTabs.css`, linhas 330 a 331): o traço de
    /// `strokeThickness` nos quatro lados, menos o de baixo na aba ativa conectada. `None` sem o `.modern-ui-tabs`.
    pub fn tab_fill_border_widths(&self, tab: &TabElement) -> Option<BoxEdges> {
        if !(self.has("monaco-workbench") && self.has("modern-ui-tabs")) {
            return None;
        }
        let stroke = self.size(STROKE_THICKNESS);
        let bottom = if self.connected_tabs() && tab.has("active") { 0.0 } else { stroke };
        Some(BoxEdges { top: stroke, right: stroke, bottom, left: stroke })
    }
}

/// A caixa do `.sticky-tabs-background` dentro do `.monaco-scrollable-element`: os deslocamentos de cima e de baixo
/// (`None` é `auto`), a altura calculada e a largura do traço de baixo.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct StickyTabsBackgroundBox {
    pub top: f64,
    pub bottom: Option<f64>,
    pub height: f64,
    pub border_bottom_width: f64,
}

impl<'a> ModernUiStyle<'a> {
    /// A caixa do `.sticky-tabs-background` (`tabs.css`, linhas 741 a 750, e `connectedEditorTabs.css`, linhas 277 a 282).
    /// `tabs.css` o posiciona em `top: 0` com a altura da aba (`editor_group_tab_height`); nas abas conectadas a máscara
    /// ocupa a altura toda do contêiner de rolagem (`scrollable_height`, `bottom: 0` e `height: auto`) e o traço de baixo
    /// (`border-bottom` com `box-sizing: border-box`) é o separador sob as abas fixadas, então a máscara cobre o
    /// preenchimento que rola por baixo sem deixar vão com o separador da fileira.
    pub fn sticky_tabs_background_box(&self, scrollable_height: f64, editor_group_tab_height: f64) -> StickyTabsBackgroundBox {
        if self.connected_tabs() {
            return StickyTabsBackgroundBox { top: 0.0, bottom: Some(0.0), height: scrollable_height, border_bottom_width: self.size(STROKE_THICKNESS) };
        }
        StickyTabsBackgroundBox { top: 0.0, bottom: None, height: editor_group_tab_height, border_bottom_width: 0.0 }
    }

    /// `border-radius` do `.tab-actions` (`tabs.css`, linhas 515 e 533, e `connectedEditorTabs.css`, linhas 492, 496 e 849 a
    /// 851): o canto do lado de fora da aba tem o raio pequeno; na aba ativa conectada o canto de cima do lado de fora usa
    /// `--modern-ui-connected-tab-radius` (o raio médio, ou o pequeno sem ele) e a pílula de cima ativa volta ao raio pequeno nos
    /// quatro cantos. `None` sem o `.modern-ui-tabs`.
    pub fn tab_actions_border_radii(&self, tab: &TabElement) -> Option<CornerRadii> {
        if !self.has("modern-ui-tabs") {
            return None;
        }
        let small = self.size(CORNER_RADIUS_SMALL);
        let left = tab.has("tab-actions-left");
        if !(self.connected_tabs() && tab.has("active")) {
            return Some(if left { CornerRadii(small, 0.0, 0.0, small) } else { CornerRadii(0.0, small, small, 0.0) });
        }
        if tab.has("connected-tab-upper-row") {
            return Some(CornerRadii::uniform(small));
        }
        // `var(--vscode-cornerRadius-medium, var(--vscode-cornerRadius-small))`.
        let radius = self.connected_tab_radius();
        Some(if left { CornerRadii(radius, 0.0, 0.0, 0.0) } else { CornerRadii(0.0, radius, 0.0, 0.0) })
    }

    /// `justify-content` do `.actions-container` do `.tab-actions` (`connectedEditorTabs.css`, linhas 499 a 501): `center` nas
    /// abas conectadas, que centra o botão na ranhura de 24px. `None` fora delas, em que nenhuma regra do Modern UI o declara.
    pub fn tab_actions_container_justify_content(&self) -> Option<&'static str> {
        self.connected_tabs().then_some("center")
    }

    /// As variáveis de tamanho que as regras da aba leem (`--vscode-spacing-*`, `--vscode-strokeThickness` e os raios), com
    /// os valores do tema.
    fn modern_spacing(&self) -> ModernSpacing {
        ModernSpacing {
            size20: self.size(SPACING_SIZE20),
            size40: self.size(SPACING_SIZE40),
            size60: self.size(SPACING_SIZE60),
            size160: self.size(SPACING_SIZE160),
            size200: self.size(SPACING_SIZE200),
            size240: self.size(SPACING_SIZE240),
            size280: self.size(SPACING_SIZE280),
            stroke: self.size(STROKE_THICKNESS),
            corner_small: self.size(CORNER_RADIUS_SMALL),
            corner_medium: self.connected_tab_radius(),
        }
    }

    /// O `TabBoxContext` de uma aba: a geometria nativa (`tab_fill_rect`, `tab_action_geometry`...) lê as classes da raiz
    /// (`.modern-ui-connected-editor-tabs`, `.modern-ui`), as da aba, os tamanhos do tema e a posição da aba na fileira. A
    /// altura da aba é a da linha conectada (`--modern-ui-connected-tab-row-height`) e, fora das abas conectadas, a do
    /// cabeçalho (`--editor-group-tab-height`: 24px, ou 20px com `.compact-height`).
    pub fn tab_box_context(&self, tab: &TabElement, placement: TabRowPlacement) -> TabBoxContext {
        let spacing = self.modern_spacing();
        let mut context = TabBoxContext {
            spacing,
            connected: self.connected_tabs(),
            modern_ui_root: self.has("modern-ui"),
            editor_group_tab_height: if placement.compact_height { spacing.size200 } else { spacing.size240 },
            wrapping: placement.wrapping,
            upper_bar: placement.upper_bar,
            upper_row: tab.has("connected-tab-upper-row"),
            active: tab.has("active"),
            hovered: tab.hovered,
            starts_row: placement.starts_row,
            group_active: tab.group_active,
            modern_ui_tabs: self.has("modern-ui-tabs"),
            two_tab_bars: placement.two_tab_bars,
            first_tab_bar: placement.first_tab_bar,
            tab_width: placement.tab_width,
            tab_height: 0.0,
        };
        context.tab_height = if context.connected { context.connected_row_height() } else { context.editor_group_tab_height };
        context
    }

    /// O `::after` do `.editor-group-container` em alto contraste (linhas 39 a 48, e 56 a 58 no editor da grade): a moldura
    /// contínua do grupo. `part` é o `.part.editor` e `in_grid_view` é o `.monaco-grid-view` entre a raiz e a parte. `None`
    /// quando o pseudoelemento não existe (`content: none`).
    pub fn connected_group_frame(&self, part: &ClassList, in_grid_view: bool) -> Option<ConnectedGroupFrame> {
        if !self.connected_tabs() || !self.high_contrast() || !part.contains("editor-tabs-multiple") {
            return None;
        }
        // Linha 56: o editor da grade do Modern UI deixa a moldura para a borda da parte.
        if in_grid_view && self.has("modern-ui") && !self.has("agent-sessions-workbench") && !part.contains("modal-editor-part") {
            return None;
        }
        let size = |id| self.size(id);
        Some(ConnectedGroupFrame {
            border_width: size(STROKE_THICKNESS),
            // `--modern-ui-connected-well-border` (linha 27); `None` é a variável sem cor, que o `border` resolve como `currentColor`.
            color: self.first(&["modernPanel.border", "contrastBorder"]),
            radius: size(CORNER_RADIUS_LARGE),
            pointer_events_none: true,
        })
    }
}

/// Qual dos dois ombros do `.tab-fill` da aba ativa: o `::before` (à esquerda) ou o `::after` (à direita).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FillShoulder {
    Left,
    Right,
}

/// A caixa de um ombro da aba ativa (`connectedEditorTabs.css`, linhas 373 a 393 e 478 a 484): um quadrado de
/// `--modern-ui-connected-tab-shoulder-radius` de lado, com o canto de baixo (o do lado de fora da aba) arredondado pelo mesmo raio.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct FillShoulderBox {
    pub width: f64,
    pub height: f64,
    pub bottom_radius: f64,
}

/// `border-top-right-radius` do `.tab-fill`: o espelho de [`TabBoxContext::fill_top_left_radius`] sem o canto quadrado do início
/// da fileira (a aresta cola só na esquerda, `connectedEditorTabs.css` L715 e L723). A aba ativa conectada tem a tampa de
/// `cap_radius` (L333); as demais têm o raio pequeno (`tabs.css` L154, `connectedEditorTabs.css` L233 e L709).
pub fn tab_fill_top_right_radius(ctx: &TabBoxContext) -> f64 {
    if !ctx.connected || !ctx.active || ctx.upper_row { ctx.spacing.corner_small } else { ctx.spacing.cap_radius(ctx.wrapping, ctx.upper_bar) }
}

/// `border-bottom-left-radius` do `.tab-fill`: a aba ativa conectada fora da linha de cima tem a base reta (L333 zera os dois
/// cantos de baixo, e L716 e L724 repetem o da esquerda no início da fileira); as demais têm o raio pequeno.
pub fn tab_fill_bottom_left_radius(ctx: &TabBoxContext) -> f64 {
    if !ctx.connected || !ctx.active || ctx.upper_row { ctx.spacing.corner_small } else { 0.0 }
}

/// `background-clip` do `.tab-fill`: nenhuma regra de `tabs.css`, `multieditortabscontrol.css` ou `connectedEditorTabs.css`
/// o declara no preenchimento (o `padding-box` de `tabs.css` L516 e L549 é do `.tab-actions` e do `::before` dele).
pub const TAB_FILL_BACKGROUND_CLIP: &str = "border-box";

/// `content` do `::before` e do `::after` do `.tab-connected-edge`: nenhuma regra os declara (as máscaras das pontas são do
/// `.tab-fill`), então valem `none`.
pub const TAB_CONNECTED_EDGE_PSEUDO_CONTENT: &str = "none";

impl<'a> ModernUiStyle<'a> {
    /// `outline-style` do `.tab-fill`. Só o contorno de foco de teclado da aba ativa em alto contraste o declara
    /// (`connectedEditorTabs.css`, linhas 96 a 100: `dashed`); a `.tab` em si tem `outline: none !important` (`tabs.css` L930 a
    /// L933) e o traço vive no preenchimento. As demais abas valem `none`.
    pub fn tab_fill_outline_style(&self, tab: &TabElement) -> &'static str {
        if self.connected_tabs() && self.high_contrast() && tab.has("active") && tab.focus_visible { "dashed" } else { "none" }
    }

    /// `border-*-width` do `.tab-border-top-container` e do `.tab-border-bottom-container`: nenhuma regra os declara (o
    /// `border: none` de `tabs.css` L764 é do estilo Pill, e o indicador é uma faixa de `background-color`), então valem `0`.
    pub fn tab_border_container_border_widths(&self) -> BoxEdges {
        BoxEdges { top: 0.0, right: 0.0, bottom: 0.0, left: 0.0 }
    }

    /// `border-<lado>-color` do `.tab-connected-edge` da aba ativa fora da linha de cima (`connectedEditorTabs.css`, linhas 406 a
    /// 421, 713 a 727 e 785 a 786). `None` quando o elemento não é desenhado (ver
    /// [`ModernUiStyle::connected_edge_border_top_color`]) e na base, que não tem borda (`border-bottom: 0`, a cor é a
    /// `currentColor`). O lado esquerdo da aba que abre a fileira (`starts_row`) é transparente (L718 e L726).
    pub fn connected_edge_border_color(&self, tab: &TabElement, side: StrokeSide, starts_row: bool) -> Option<Color> {
        if !self.connected_tabs() || !tab.has("active") || tab.has("connected-tab-upper-row") {
            return None;
        }
        match side {
            StrokeSide::Top => self.connected_edge_border_top_color(tab),
            StrokeSide::Bottom => None,
            StrokeSide::Left if starts_row => Some(Color::transparent_color()),
            StrokeSide::Left | StrokeSide::Right => self.tab_variable("--modern-ui-connected-tab-border", tab),
        }
    }

    /// `content` do `::before` e do `::after` do `.tab-fill` (`connectedEditorTabs.css`, linhas 373 a 404, 478 a 484, 750 a 753 e
    /// 817 a 819): `""` nos dois ombros da aba ativa fora da linha de cima, menos o da esquerda na aba que abre a fileira
    /// (`:first-child` ou depois de `.last-in-row`, `starts_row`) ou que tem `.connected-tab-left-edge`, em que a aresta cola.
    /// A pílula de cima não tem ombros (`none`), mas o `::after` dela vira o sublinhado de `tab-border-bottom` (aba ativa) ou o
    /// do `:hover` (L817 e L818), que não é um ombro ([`ModernUiStyle::fill_shoulder_box`] devolve `None`).
    pub fn fill_shoulder_content(&self, tab: &TabElement, starts_row: bool, shoulder: FillShoulder) -> &'static str {
        const PRESENT: &str = "\"\"";
        if !self.connected_tabs() {
            return "none";
        }
        let active = tab.has("active");
        if tab.has("connected-tab-upper-row") {
            let underline = (active && tab.has("tab-border-bottom")) || (tab.hovered && !tab.has("selected") && !active);
            return if shoulder == FillShoulder::Right && underline { PRESENT } else { "none" };
        }
        if !active || (shoulder == FillShoulder::Left && (starts_row || tab.has("connected-tab-left-edge"))) {
            return "none";
        }
        PRESENT
    }

    /// A caixa do ombro (`width`, `height` e o raio do canto de baixo) ou `None` quando ele não existe ou, na pílula de cima, é
    /// o sublinhado (`width: auto`, `height: strokeThickness`, `border-radius: 0`).
    pub fn fill_shoulder_box(&self, tab: &TabElement, starts_row: bool, shoulder: FillShoulder) -> Option<FillShoulderBox> {
        if self.fill_shoulder_content(tab, starts_row, shoulder) == "none" || tab.has("connected-tab-upper-row") {
            return None;
        }
        let radius = self.modern_spacing().shoulder_radius();
        Some(FillShoulderBox { width: radius, height: radius, bottom_radius: radius })
    }

    /// `border-<lado>-color` do `.tab-fill` de uma aba conectada, como o `getComputedStyle` devolve (`rgb(...)`, `rgba(...)`).
    /// Em alto contraste vale a cascata de `high_contrast_connected_tab_fill_stroke` (a aba ativa fora da linha de cima tem o
    /// contorno transparente, L330, e o `.tab-connected-edge` desenha o traço); fora dele valem todas as regras que pintam a
    /// borda do preenchimento, em cada lado (ver `connected_tab_fill_border_color`: o traço da aba, o da selecionada, o
    /// sublinhado do `:hover`, o acento sujo, a pílula de cima e a ativa com várias seleções). `None` fora das abas
    /// conectadas e quando a cor é `currentColor` (a variável do tema não existe).
    pub fn tab_fill_border_color(&self, tab: &TabElement, side: StrokeSide, placement: TabRowPlacement) -> Option<String> {
        if !self.connected_tabs() {
            return None;
        }
        let (active, selected, upper_row) = (tab.has("active"), tab.has("selected"), tab.has("connected-tab-upper-row"));
        if !self.high_contrast() {
            return self.connected_tab_fill_border_color(tab, side).as_ref().map(css_rgb);
        }
        if active && !upper_row {
            return Some(css_rgb(&Color::transparent_color()));
        }
        let css = |id: &str| self.color(id).as_ref().map(css_rgb);
        let (tab_border, contrast_active_border, focus_border) = (css("tab.border"), css("contrastActiveBorder"), css("focusBorder"));
        let dirty_border_top_color = tab.dirty_border_top_color.as_ref().map(css_rgb);
        let input = TabFillStrokeInput {
            wrapping: placement.wrapping,
            upper_bar: placement.upper_bar,
            active,
            selected,
            multi_selected: tab.has("multi-selected"),
            dirty_border_top: tab.has("dirty-border-top"),
            upper_row,
            pointer_hover: tab.hovered,
            hovered: false,
            focused: tab.focused,
            focus_within: tab.focus_within,
            tab_border: tab_border.as_deref(),
            contrast_active_border: contrast_active_border.as_deref(),
            focus_border: focus_border.as_deref(),
            dirty_border_top_color: dirty_border_top_color.as_deref(),
            spacing_size20: self.size(SPACING_SIZE20),
            regular_theme: false,
        };
        match high_contrast_connected_tab_fill_stroke(&input).color(side) {
            TabStrokeColor::Transparent => Some(css_rgb(&Color::transparent_color())),
            TabStrokeColor::CurrentColor => None,
            TabStrokeColor::EditorTabBorder => Some(css_rgb(&self.editor_tab_border())),
            TabStrokeColor::ConnectedTabBorder => self.connected_tab_border(tab).as_ref().map(css_rgb),
            TabStrokeColor::Color(color) => Some(color.to_owned()),
        }
    }

    /// `padding-top` do `.tabs-and-actions-container` (`tabs.css`, linhas 283, 305 a 307 e 318 a 320, e `connectedEditorTabs.css`,
    /// linhas 148 a 150 e 301 a 304): `size20` na quebra (sem duas barras) e na barra superior, `0` nas demais e sem o
    /// `.modern-ui-tabs`.
    pub fn tabs_and_actions_padding_top(&self, placement: TabRowPlacement) -> f64 {
        let reserved = self.has("modern-ui-tabs") && (placement.upper_bar || (placement.wrapping && !placement.two_tab_bars));
        if reserved { self.size(SPACING_SIZE20) } else { 0.0 }
    }

    /// `padding-bottom` do `.tabs-container` (`connectedEditorTabs.css`, linhas 182 a 189): o traço de `strokeThickness` que o
    /// separador ocupa, e `0` na barra superior. Fora das abas conectadas a regra não existe.
    pub fn tabs_container_padding_bottom(&self, placement: TabRowPlacement) -> f64 {
        if self.connected_tabs() && !placement.upper_bar { self.size(STROKE_THICKNESS) } else { 0.0 }
    }

    /// `bottom` do `::after` do `.tabs-and-actions-container`, o separador sob a fileira (`tabs.css`, linhas 286 a 297): `0`.
    pub fn tabs_row_separator_bottom(&self) -> f64 {
        0.0
    }
}

/// O que o `::after` de `.editor-group-container` pinta em alto contraste: a largura da borda, a cor, o raio e o
/// `pointer-events`.
#[derive(Clone, Debug, PartialEq)]
pub struct ConnectedGroupFrame {
    pub border_width: f64,
    pub color: Option<Color>,
    pub radius: f64,
    /// `pointer-events: none`.
    pub pointer_events_none: bool,
}

/// `display` do `> .title` e do `> .editor-container` de um `.editor-group-container` (`editorgroupview.css`, linhas 217 a
/// 219 e 253 a 255, a mesma regra para os dois): `none` no grupo `.empty`, `block` nos demais.
pub fn editor_group_child_display(group: &ClassList) -> &'static str {
    if group.contains("empty") { "none" } else { "block" }
}
