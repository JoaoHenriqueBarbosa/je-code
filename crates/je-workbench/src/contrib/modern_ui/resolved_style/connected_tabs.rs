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
//!   [`ModernUiStyle::connected_tab_fill_border_top_color`] devolve `None` nesse caso.

use je_theme::color::{Color, Srgba};

use super::{ClassList, EditorTabState, ModernUiStyle};
use crate::common::theme::{EDITOR_GROUP_HEADER_CONNECTED_TABS_BACKGROUND, EDITOR_GROUP_HEADER_TABS_BACKGROUND, EDITOR_GROUP_HEADER_TABS_BORDER};
use crate::services::themes::modern_tab_color_customizations::collect_modern_tab_color_customizations;

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
    /// alto contraste e quando a variável da regra não tem cor.
    pub fn connected_tab_fill_border_top_color(&self, tab: &TabElement) -> Option<Color> {
        if !self.connected_tabs() || self.high_contrast() {
            return None;
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
}
