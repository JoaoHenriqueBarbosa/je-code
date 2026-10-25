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
use je_theme::size_utils::get_size_registry;
use je_theme::sizes::base_sizes::{
    CORNER_RADIUS_LARGE, CORNER_RADIUS_MEDIUM, CORNER_RADIUS_SMALL, SPACING_SIZE160, SPACING_SIZE20, SPACING_SIZE200, SPACING_SIZE240, SPACING_SIZE280, SPACING_SIZE40, SPACING_SIZE60, STROKE_THICKNESS,
};

use super::{BoxEdges, ClassList, EditorTabState, ModernUiStyle};
use crate::browser::parts::editor::multi_editor_tabs_control::{ModernSpacing, TabBoxContext};
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

    fn has(&self, class_name: &str) -> bool {
        self.classes.contains(class_name)
    }
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
    fn declared(&self, name: &str) -> Option<Color> {
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
        let (active, selected, upper_row) = (tab.has("active"), tab.has("selected"), tab.has("connected-tab-upper-row"));
        let (tab_border_top, dirty_border_top, multi_selected) = (tab.has("tab-border-top"), tab.has("dirty-border-top"), tab.has("multi-selected"));
        // `--modern-ui-connected-tab-top-accent` da aba (linha 755).
        let accent = if dirty_border_top { tab.dirty_border_top_color.clone() } else { None };
        // Linhas 797 a 800: a pílula de cima da aba ativa com várias seleções.
        if active && upper_row && multi_selected {
            return accent.or_else(|| tab.border_top_color.clone()).or_else(|| self.color("contrastActiveBorder"));
        }
        // Linha 771: o acento sujo pinta a borda de cima da pílula de cima e das abas inativas.
        if dirty_border_top && (upper_row || !active) {
            return accent;
        }
        if active {
            // Linhas 338 a 344: só a pílula de cima pinta o acento explícito; a aba ativa comum tem a borda transparente
            // (linha 330) e o `.tab-connected-edge` desenha o topo.
            if upper_row && tab_border_top {
                let suffix = if tab.group_active { "active-border-top" } else { "unfocused-active-border-top" };
                return Some(self.tab_custom(suffix).unwrap_or_else(Color::transparent_color));
            }
            return Some(Color::transparent_color());
        }
        // Linha 710: a pílula de cima tem o contorno transparente.
        if upper_row {
            return Some(Color::transparent_color());
        }
        if selected {
            // `tabs.css`, linhas 459 a 467: o traço de cima da aba selecionada é `tab.selectedBorderTop`.
            return if tab_border_top { self.color("tab.selectedBorderTop") } else { Some(self.editor_tab_border()) };
        }
        // Linhas 240 a 246: as abas inativas não têm contorno.
        Some(Color::transparent_color())
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

    /// `pointer-events` do `.tab-actions` (linhas 554 a 565) e, por herança, do `.action-label` dentro dele: a aba em repouso
    /// (sem `.active`, `.dirty`, `.sticky`, `:hover` nem `:focus-within`) deixa a ranhura da ação fora do alvo do ponteiro,
    /// para ela não interceptar o clique da aba vizinha; nos demais estados vale `auto`. Fora das abas conectadas a regra não
    /// existe e vale o padrão `auto`.
    pub fn tab_actions_pointer_events(&self, tab: &TabElement) -> &'static str {
        let idle = !(tab.has("active") || tab.has("dirty") || tab.has("sticky") || tab.hovered || tab.focus_within);
        if self.connected_tabs() && idle { "none" } else { "auto" }
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
            corner_medium: self.size(CORNER_RADIUS_MEDIUM),
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
        let size = |id| get_size_registry().resolve_default_size(id, self.theme).map_or(0.0, |size| size.value);
        Some(ConnectedGroupFrame {
            border_width: size(STROKE_THICKNESS),
            // `--modern-ui-connected-well-border` (linha 27); `None` é a variável sem cor, que o `border` resolve como `currentColor`.
            color: self.first(&["modernPanel.border", "contrastBorder"]),
            radius: size(CORNER_RADIUS_LARGE),
            pointer_events_none: true,
        })
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
