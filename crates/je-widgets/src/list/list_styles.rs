//! Os estilos da lista (`IListStyles`, `defaultListStyles` de `defaultStyles.ts`) e a resolução da cor de
//! uma linha (o efeito do `DefaultStyleController.style()` de `listWidget.ts`).
//!
//! Do TypeScript para o Rust:
//! - O `DefaultStyleController` gera regras de CSS com seletores (`.monaco-list:focus .monaco-list-row.focused`,
//!   `.selected:hover`...). Aqui a cascata é resolvida na mão por [`IListStyles::row_paint`]: para cada
//!   propriedade, a regra de maior especificidade que vale para o estado da linha, e no empate a emitida por
//!   último. O `:focus` da lista vira [`RowState::list_focused`].
//! - `outline: 1px solid|dotted|dashed` vira uma borda sólida de 1 px por dentro da linha
//!   (`outline-offset: -1px`); o `je-ui` não tem traço pontilhado nem tracejado.
//! - O `color` do CSS herda; os nós do `je-ui` não herdam. A lista aplica a cor da linha ao texto e aos
//!   codicons de todos os descendentes (ver `List::paint_row`).
//! - `listDropOverBackground` e `listDropBetweenBackground` pintam as linhas com a classe `drop-target*` (ver
//!   `List::paint_row`); as cores de tabela ainda não têm efeito.

use je_render::Rgba;
use je_theme::ColorTheme;

/// A cor do texto quando o tema não dá nenhuma.
pub const LIST_FALLBACK_FOREGROUND: Rgba = Rgba::new(204, 204, 204, 255);

/// `IListStyles`: `string | undefined` do original é `Option<Rgba>`. `foreground` e `row_radius` não estão
/// no original: o primeiro é a cor do texto herdada do workbench (`--vscode-foreground`), o segundo o
/// `border-radius` das linhas, que o tema 2026 dá pelo CSS do workbench (padrão 0).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct IListStyles {
    pub list_background: Option<Rgba>,
    pub list_inactive_focus_foreground: Option<Rgba>,
    pub list_focus_background: Option<Rgba>,
    pub list_focus_foreground: Option<Rgba>,
    pub list_focus_outline: Option<Rgba>,
    pub list_active_selection_background: Option<Rgba>,
    pub list_active_selection_foreground: Option<Rgba>,
    pub list_active_selection_icon_foreground: Option<Rgba>,
    pub list_focus_and_selection_outline: Option<Rgba>,
    pub list_focus_and_selection_background: Option<Rgba>,
    pub list_focus_and_selection_foreground: Option<Rgba>,
    pub list_inactive_selection_background: Option<Rgba>,
    pub list_inactive_selection_icon_foreground: Option<Rgba>,
    pub list_inactive_selection_foreground: Option<Rgba>,
    pub list_inactive_focus_background: Option<Rgba>,
    pub list_inactive_focus_outline: Option<Rgba>,
    pub list_hover_background: Option<Rgba>,
    pub list_hover_foreground: Option<Rgba>,
    pub list_drop_over_background: Option<Rgba>,
    pub list_drop_between_background: Option<Rgba>,
    pub list_selection_outline: Option<Rgba>,
    pub list_hover_outline: Option<Rgba>,
    pub tree_indent_guides_stroke: Option<Rgba>,
    pub tree_inactive_indent_guides_stroke: Option<Rgba>,
    pub tree_sticky_scroll_background: Option<Rgba>,
    pub tree_sticky_scroll_border: Option<Rgba>,
    pub tree_sticky_scroll_shadow: Option<Rgba>,
    pub table_columns_border: Option<Rgba>,
    pub table_odd_rows_background_color: Option<Rgba>,
    pub foreground: Option<Rgba>,
    pub row_radius: f64,
}

/// `unthemedListStyles`: nenhuma cor.
pub fn unthemed_list_styles() -> IListStyles {
    IListStyles::default()
}

/// `defaultListStyles` (`defaultStyles.ts`) com as cores resolvidas no tema.
pub fn default_list_styles(theme: &ColorTheme) -> IListStyles {
    let active_contrast_border = theme.get_color("contrastActiveBorder");
    IListStyles {
        list_background: None,
        list_inactive_focus_foreground: None,
        list_focus_background: theme.get_color("list.focusBackground"),
        list_focus_foreground: theme.get_color("list.focusForeground"),
        list_focus_outline: theme.get_color("list.focusOutline"),
        list_active_selection_background: theme.get_color("list.activeSelectionBackground"),
        list_active_selection_foreground: theme.get_color("list.activeSelectionForeground"),
        list_active_selection_icon_foreground: theme.get_color("list.activeSelectionIconForeground"),
        list_focus_and_selection_outline: theme.get_color("list.focusAndSelectionOutline"),
        list_focus_and_selection_background: theme.get_color("list.activeSelectionBackground"),
        list_focus_and_selection_foreground: theme.get_color("list.activeSelectionForeground"),
        list_inactive_selection_background: theme.get_color("list.inactiveSelectionBackground"),
        list_inactive_selection_icon_foreground: theme.get_color("list.inactiveSelectionIconForeground"),
        list_inactive_selection_foreground: theme.get_color("list.inactiveSelectionForeground"),
        list_inactive_focus_background: theme.get_color("list.inactiveFocusBackground"),
        list_inactive_focus_outline: theme.get_color("list.inactiveFocusOutline"),
        list_hover_background: theme.get_color("list.hoverBackground"),
        list_hover_foreground: theme.get_color("list.hoverForeground"),
        list_drop_over_background: theme.get_color("list.dropBackground"),
        list_drop_between_background: theme.get_color("list.dropBetweenBackground"),
        list_selection_outline: active_contrast_border,
        list_hover_outline: active_contrast_border,
        tree_indent_guides_stroke: theme.get_color("tree.indentGuidesStroke"),
        tree_inactive_indent_guides_stroke: theme.get_color("tree.inactiveIndentGuidesStroke"),
        tree_sticky_scroll_background: None,
        tree_sticky_scroll_border: None,
        tree_sticky_scroll_shadow: theme.get_color("scrollbar.shadow"),
        table_columns_border: theme.get_color("tree.tableColumnsBorder"),
        table_odd_rows_background_color: theme.get_color("tree.tableOddRowsBackground"),
        foreground: theme.get_color("foreground"),
        row_radius: 0.0,
    }
}

/// O estado de uma linha que as regras de `style()` olham.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct RowState {
    /// O `:focus` do `.monaco-list`.
    pub list_focused: bool,
    /// A classe `.focused` da linha.
    pub focused: bool,
    /// A classe `.selected` da linha.
    pub selected: bool,
    /// O `:hover` da linha.
    pub hovered: bool,
}

/// O que as regras de `style()` dão a uma linha.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct RowPaint {
    pub background: Option<Rgba>,
    pub foreground: Option<Rgba>,
    /// A cor dos codicons da linha (cai na `foreground` quando não há).
    pub icon_foreground: Option<Rgba>,
    /// A cor do `outline` da linha.
    pub outline: Option<Rgba>,
}

/// A primeira cor presente, na ordem de precedência.
fn first(candidates: &[(bool, Option<Rgba>)]) -> Option<Rgba> {
    candidates.iter().find_map(|(applies, color)| if *applies { *color } else { None })
}

impl IListStyles {
    /// A cascata de `DefaultStyleController.style()` para uma linha no estado dado. Cada lista de
    /// candidatos vai da regra mais forte à mais fraca: as com `:focus` (e `.selected.focused`) ganham das
    /// sem `:focus`, a seleção ganha do foco no empate, e o hover só vale em linha sem seleção nem foco.
    pub fn row_paint(&self, state: RowState) -> RowPaint {
        let RowState { list_focused: l, focused: f, selected: s, hovered: h } = state;
        let plain = !s && !f;

        let background = first(&[
            (plain && h, self.list_hover_background),
            (l && s && f, self.list_focus_and_selection_background),
            (l && s, self.list_active_selection_background),
            (l && f, self.list_focus_background),
            (s, self.list_inactive_selection_background),
            (f, self.list_inactive_focus_background),
        ]);

        let foreground = first(&[
            (plain && h, self.list_hover_foreground),
            (l && s && f, self.list_focus_and_selection_foreground),
            (l && s, self.list_active_selection_foreground),
            (l && f, self.list_focus_foreground),
            (s, self.list_inactive_selection_foreground),
            (f, self.list_inactive_focus_foreground),
        ]);

        let icon_foreground = first(&[(l && s, self.list_active_selection_icon_foreground), (f, self.list_inactive_selection_icon_foreground)]);

        let focus_and_selection_outline = self.list_focus_and_selection_outline.or(self.list_selection_outline).or(self.list_focus_outline);
        let inactive_focus_and_selection_outline = self.list_selection_outline.or(self.list_inactive_focus_outline);
        let outline = first(&[
            (l && f && s, focus_and_selection_outline),
            (l && f, self.list_focus_outline),
            (f && s, inactive_focus_and_selection_outline),
            (h, self.list_hover_outline),
            (f, self.list_inactive_focus_outline),
            (s, self.list_selection_outline),
        ]);

        RowPaint { background, foreground, icon_foreground, outline }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn color(v: u8) -> Option<Rgba> {
        Some(Rgba::new(v, v, v, 255))
    }

    fn styles() -> IListStyles {
        IListStyles {
            list_hover_background: color(1),
            list_focus_background: color(2),
            list_active_selection_background: color(3),
            list_focus_and_selection_background: color(4),
            list_inactive_selection_background: color(5),
            list_inactive_focus_background: color(6),
            list_focus_outline: color(7),
            ..IListStyles::default()
        }
    }

    #[test]
    fn hover_only_on_plain_rows() {
        let hovered = RowState { hovered: true, ..RowState::default() };
        assert_eq!(styles().row_paint(hovered).background, color(1));
        let selected = RowState { hovered: true, selected: true, ..RowState::default() };
        assert_eq!(styles().row_paint(selected).background, color(5));
    }

    #[test]
    fn focused_list_uses_active_colors() {
        let focused = RowState { list_focused: true, focused: true, ..RowState::default() };
        assert_eq!(styles().row_paint(focused).background, color(2));
        assert_eq!(styles().row_paint(focused).outline, color(7));
        let both = RowState { list_focused: true, focused: true, selected: true, hovered: false };
        assert_eq!(styles().row_paint(both).background, color(4));
        let blurred = RowState { list_focused: false, ..both };
        assert_eq!(styles().row_paint(blurred).background, color(5));
        assert_eq!(styles().row_paint(blurred).outline, None);
    }
}
