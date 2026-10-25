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

use std::collections::BTreeSet;

use je_theme::ColorTheme;
use je_theme::color::Color;
use je_theme::color::format::css::format_rgb;
use je_theme::registry::{as_css_variable_name, color_registry};
use je_theme::size_utils::get_size_registry;
use je_theme::sizes::base_sizes::{CORNER_RADIUS_LARGE, SPACING_NONE, SPACING_SIZE20, SPACING_SIZE320, SPACING_SIZE360, SPACING_SIZE40, SPACING_SIZE60, SPACING_SIZE80};

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
}

impl<'a> ModernUiStyle<'a> {
    pub fn new(theme: &'a ColorTheme, root_class_name: &str) -> Self {
        Self { theme, root: ClassList::new(root_class_name), notification_insets: NotificationInsets::default() }
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
        self.theme.resolve_color(id, true)
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
        // `--modern-ui-editor-border-color` (floatingPanels.css linha 36; connectedEditorTabs.css linhas 13 e 51) vence a
        // cadeia de `editorBorder.css` (linha 49, `!important`).
        if self.connected_high_contrast_editor(part) {
            return Some(self.floating_card_border_color().unwrap_or_else(|| self.first(&["modernPanel.border", "contrastBorder"]).unwrap_or_else(Color::transparent_color)));
        }
        if self.compact() {
            return Some(Color::transparent_color());
        }
        Some(self.chain(&["editor.border", "surface.border", "widget.border"]))
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
    /// fora do alto contraste (linhas 37 e 61).
    pub fn editor_tab_foreground(&self, tab: EditorTabState) -> Option<Color> {
        if !self.has("modern-ui-tabs") {
            return None;
        }
        if tab.active {
            return self.color("modernEditorTab.activeForeground");
        }
        if tab.selected {
            return self.color("tab.selectedForeground");
        }
        if tab.hovered {
            return self.color("modernEditorTab.hoverForeground");
        }
        if self.has("modern-ui-connected-editor-tabs") && !self.high_contrast() {
            return self.color("tab.inactiveForeground");
        }
        self.color("foreground").map(|foreground| foreground.transparent(0.5))
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
