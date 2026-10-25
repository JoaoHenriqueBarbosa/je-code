//! Porte de `src/vs/workbench/services/themes/browser/modernTabColorCustomizations.ts`: as variáveis `--modern-ui-editor-tab-*`
//! que o `registerThemingParticipant` do original publica a partir das customizações de cor do usuário.
//!
//! Divergências:
//! - **Sem folha de estilo.** O participante do original escreve a regra
//!   `.modern-ui-tabs.monaco-workbench.monaco-workbench { ... }` no CSS do tema; aqui a função devolve o mesmo conjunto de
//!   declarações (nome da variável e cor) para o estilo resolvido (`ModernUiStyle`) consultar no lugar do `getComputedStyle`.
//!   O seletor (a raiz com `.modern-ui-tabs`) é decidido por quem consulta.
//! - A cor de cada variável é a `Color` (o `to_string()` dela é o texto que `${color}` escreveria).

use std::collections::BTreeMap;

use je_theme::ColorTheme;
use je_theme::color::Color;

const TAB_ACTIVE_BACKGROUND: &str = "tab.activeBackground";
const TAB_UNFOCUSED_ACTIVE_BACKGROUND: &str = "tab.unfocusedActiveBackground";
const TAB_INACTIVE_BACKGROUND: &str = "tab.inactiveBackground";
const TAB_UNFOCUSED_INACTIVE_BACKGROUND: &str = "tab.unfocusedInactiveBackground";
const TAB_HOVER_BACKGROUND: &str = "tab.hoverBackground";
const TAB_UNFOCUSED_HOVER_BACKGROUND: &str = "tab.unfocusedHoverBackground";
const TAB_ACTIVE_FOREGROUND: &str = "tab.activeForeground";
const TAB_UNFOCUSED_ACTIVE_FOREGROUND: &str = "tab.unfocusedActiveForeground";
const TAB_INACTIVE_FOREGROUND: &str = "tab.inactiveForeground";
const TAB_UNFOCUSED_INACTIVE_FOREGROUND: &str = "tab.unfocusedInactiveForeground";
const TAB_HOVER_FOREGROUND: &str = "tab.hoverForeground";
const TAB_UNFOCUSED_HOVER_FOREGROUND: &str = "tab.unfocusedHoverForeground";
const TAB_BORDER: &str = "tab.border";
const TAB_LAST_PINNED_BORDER: &str = "tab.lastPinnedBorder";
const TAB_ACTIVE_BORDER: &str = "tab.activeBorder";
const TAB_UNFOCUSED_ACTIVE_BORDER: &str = "tab.unfocusedActiveBorder";
const TAB_ACTIVE_BORDER_TOP: &str = "tab.activeBorderTop";
const TAB_UNFOCUSED_ACTIVE_BORDER_TOP: &str = "tab.unfocusedActiveBorderTop";
const TAB_SELECTED_BORDER_TOP: &str = "tab.selectedBorderTop";
const TAB_HOVER_BORDER: &str = "tab.hoverBorder";
const TAB_UNFOCUSED_HOVER_BORDER: &str = "tab.unfocusedHoverBorder";

const MODERN_TAB_ACTIVE_BACKGROUND: &str = "modernTab.activeBackground";
const MODERN_TAB_ACTIVE_FOREGROUND: &str = "modernTab.activeForeground";
const MODERN_TAB_HOVER_BACKGROUND: &str = "modernTab.hoverBackground";
const MODERN_TAB_HOVER_FOREGROUND: &str = "modernTab.hoverForeground";
const MODERN_EDITOR_TAB_ACTIVE_ACTION_BACKGROUND: &str = "modernEditorTab.activeActionBackground";
const MODERN_EDITOR_TAB_ACTIVE_BACKGROUND: &str = "modernEditorTab.activeBackground";
const MODERN_EDITOR_TAB_ACTIVE_FOREGROUND: &str = "modernEditorTab.activeForeground";
const MODERN_EDITOR_TAB_ACTIVE_HOVER_ACTION_BACKGROUND: &str = "modernEditorTab.activeHoverActionBackground";
const MODERN_EDITOR_TAB_ACTIVE_HOVER_BACKGROUND: &str = "modernEditorTab.activeHoverBackground";
const MODERN_EDITOR_TAB_HOVER_ACTION_BACKGROUND: &str = "modernEditorTab.hoverActionBackground";
const MODERN_EDITOR_TAB_HOVER_BACKGROUND: &str = "modernEditorTab.hoverBackground";
const MODERN_EDITOR_TAB_HOVER_FOREGROUND: &str = "modernEditorTab.hoverForeground";
const MODERN_EDITOR_TAB_INACTIVE_BACKGROUND: &str = "modernEditorTab.inactiveBackground";
const MODERN_EDITOR_TAB_SELECTED_ACTION_BACKGROUND: &str = "modernEditorTab.selectedActionBackground";

const EDITOR_BACKGROUND: &str = "editor.background";

/// `theme.getColorCustomization(id)` truthy.
fn is_customized(theme: &ColorTheme, id: &str) -> bool {
    theme.get_color_customization(id).is_some()
}

/// `resolveLegacyTabColor`: a cor a emitir como variável do Modern UI, ou `None` para deixar o padrão do CSS.
///
/// Com `related_legacy_color_ids`, o padrão de `legacy_color_id` no registro é assumido derivado deles (por exemplo,
/// `tab.unfocusedInactiveForeground` deriva de `tab.inactiveForeground` e `tab.activeForeground`), então customizar
/// qualquer um deles chega ao valor derivado pelo `getColor`.
fn resolve_legacy_tab_color(theme: &ColorTheme, legacy_color_id: &str, modern_color_ids: &[&str], related_legacy_color_ids: &[&str]) -> Option<Color> {
    if modern_color_ids.iter().any(|id| is_customized(theme, id)) {
        return None;
    }
    if !is_customized(theme, legacy_color_id) && !related_legacy_color_ids.iter().any(|id| is_customized(theme, id)) {
        return None;
    }
    theme.resolve_color(legacy_color_id, true)
}

/// `resolveExplicitTabColor`.
fn resolve_explicit_tab_color(
    theme: &ColorTheme,
    modern_color_id: &str,
    modern_color_ids: &[&str],
    legacy_color_id: &str,
    related_legacy_color_ids: &[&str],
) -> Option<Color> {
    if modern_color_ids.iter().any(|id| is_customized(theme, id)) {
        return theme.resolve_color(modern_color_id, true);
    }
    resolve_legacy_tab_color(theme, legacy_color_id, modern_color_ids, related_legacy_color_ids)
}

/// `flattenTabBackground`.
fn flatten_tab_background(color: &Color, editor_background: Option<&Color>) -> Color {
    editor_background.map_or_else(|| color.clone(), |background| color.make_opaque(background))
}

/// `collectModernTabColorCustomizations`: o nome da variável CSS e a cor de cada declaração que o participante emite.
pub fn collect_modern_tab_color_customizations(theme: &ColorTheme) -> BTreeMap<&'static str, Color> {
    let legacy = |legacy_color_id: &str, modern_color_ids: &[&str], related: &[&str]| resolve_legacy_tab_color(theme, legacy_color_id, modern_color_ids, related);
    let explicit = |modern_color_id: &str, modern_color_ids: &[&str], legacy_color_id: &str, related: &[&str]| {
        resolve_explicit_tab_color(theme, modern_color_id, modern_color_ids, legacy_color_id, related)
    };
    let get_color = |id: &str| theme.resolve_color(id, true);

    let active_modern = [MODERN_EDITOR_TAB_ACTIVE_BACKGROUND, MODERN_TAB_ACTIVE_BACKGROUND];
    let inactive_modern = [MODERN_EDITOR_TAB_INACTIVE_BACKGROUND];
    let hover_modern = [MODERN_EDITOR_TAB_HOVER_BACKGROUND, MODERN_TAB_HOVER_BACKGROUND];
    let active_hover_modern = [MODERN_EDITOR_TAB_ACTIVE_HOVER_BACKGROUND, MODERN_EDITOR_TAB_HOVER_BACKGROUND, MODERN_TAB_HOVER_BACKGROUND];

    let active_background = legacy(TAB_ACTIVE_BACKGROUND, &active_modern, &[]);
    let unfocused_active_background = legacy(TAB_UNFOCUSED_ACTIVE_BACKGROUND, &active_modern, &[TAB_ACTIVE_BACKGROUND]);
    let inactive_background = legacy(TAB_INACTIVE_BACKGROUND, &inactive_modern, &[]);
    let unfocused_inactive_background = legacy(TAB_UNFOCUSED_INACTIVE_BACKGROUND, &inactive_modern, &[TAB_INACTIVE_BACKGROUND]);
    let hover_background = legacy(TAB_HOVER_BACKGROUND, &hover_modern, &[]);
    let unfocused_hover_background = legacy(TAB_UNFOCUSED_HOVER_BACKGROUND, &hover_modern, &[TAB_HOVER_BACKGROUND]);
    let active_hover_background = legacy(TAB_HOVER_BACKGROUND, &active_hover_modern, &[]);
    let unfocused_active_hover_background = legacy(TAB_UNFOCUSED_HOVER_BACKGROUND, &active_hover_modern, &[TAB_HOVER_BACKGROUND]);
    let editor_background = get_color(EDITOR_BACKGROUND);
    let has_modern_active_action_background = theme.defines(MODERN_EDITOR_TAB_ACTIVE_ACTION_BACKGROUND);
    let has_modern_hover_action_background = theme.defines(MODERN_EDITOR_TAB_HOVER_ACTION_BACKGROUND);
    let has_modern_active_hover_action_background = theme.defines(MODERN_EDITOR_TAB_ACTIVE_HOVER_ACTION_BACKGROUND);
    let explicit_active_action_background = if has_modern_active_action_background { get_color(MODERN_EDITOR_TAB_ACTIVE_ACTION_BACKGROUND) } else { None };
    let explicit_hover_action_background = if has_modern_hover_action_background { get_color(MODERN_EDITOR_TAB_HOVER_ACTION_BACKGROUND) } else { None };
    let explicit_active_hover_action_background =
        if has_modern_active_hover_action_background { get_color(MODERN_EDITOR_TAB_ACTIVE_HOVER_ACTION_BACKGROUND) } else { None };
    let explicit_selected_action_background =
        if theme.defines(MODERN_EDITOR_TAB_SELECTED_ACTION_BACKGROUND) { get_color(MODERN_EDITOR_TAB_SELECTED_ACTION_BACKGROUND) } else { None };
    let explicit_active_background = explicit(MODERN_EDITOR_TAB_ACTIVE_BACKGROUND, &active_modern, TAB_ACTIVE_BACKGROUND, &[]);
    let explicit_unfocused_active_background = explicit(MODERN_EDITOR_TAB_ACTIVE_BACKGROUND, &active_modern, TAB_UNFOCUSED_ACTIVE_BACKGROUND, &[TAB_ACTIVE_BACKGROUND]);
    let explicit_inactive_background = explicit(MODERN_EDITOR_TAB_INACTIVE_BACKGROUND, &inactive_modern, TAB_INACTIVE_BACKGROUND, &[]);
    let explicit_unfocused_inactive_background =
        explicit(MODERN_EDITOR_TAB_INACTIVE_BACKGROUND, &inactive_modern, TAB_UNFOCUSED_INACTIVE_BACKGROUND, &[TAB_INACTIVE_BACKGROUND]);
    let explicit_hover_background = explicit(MODERN_EDITOR_TAB_HOVER_BACKGROUND, &hover_modern, TAB_HOVER_BACKGROUND, &[]);
    let explicit_unfocused_hover_background = explicit(MODERN_EDITOR_TAB_HOVER_BACKGROUND, &hover_modern, TAB_UNFOCUSED_HOVER_BACKGROUND, &[TAB_HOVER_BACKGROUND]);
    let explicit_active_hover_background = explicit(MODERN_EDITOR_TAB_ACTIVE_HOVER_BACKGROUND, &active_hover_modern, TAB_HOVER_BACKGROUND, &[]);
    let explicit_unfocused_active_hover_background =
        explicit(MODERN_EDITOR_TAB_ACTIVE_HOVER_BACKGROUND, &active_hover_modern, TAB_UNFOCUSED_HOVER_BACKGROUND, &[TAB_HOVER_BACKGROUND]);
    let tab_border = legacy(TAB_BORDER, &[], &[]);
    let active_border_top = legacy(TAB_ACTIVE_BORDER_TOP, &[], &[]);
    let unfocused_active_border_top = legacy(TAB_UNFOCUSED_ACTIVE_BORDER_TOP, &[], &[TAB_ACTIVE_BORDER_TOP]);
    let selected_border_top = legacy(TAB_SELECTED_BORDER_TOP, &[], &[]);
    let explicit_active_border_top = if get_color(TAB_ACTIVE_BORDER_TOP).is_some() { active_border_top.clone() } else { selected_border_top.clone() };
    let explicit_unfocused_active_border_top = if get_color(TAB_UNFOCUSED_ACTIVE_BORDER_TOP).is_some() { unfocused_active_border_top.clone() } else { selected_border_top };

    let mut variables: BTreeMap<&'static str, Color> = BTreeMap::new();
    let mut add = |name: &'static str, color: Option<Color>| {
        if let Some(color) = color {
            variables.insert(name, color);
        }
    };

    add("--modern-ui-editor-tab-active-background", active_background.clone());
    add("--modern-ui-editor-tab-unfocused-active-background", unfocused_active_background.clone());
    add("--modern-ui-editor-tab-inactive-background", inactive_background);
    add("--modern-ui-editor-tab-unfocused-inactive-background", unfocused_inactive_background);
    add("--modern-ui-editor-tab-hover-background", hover_background.clone());
    add("--modern-ui-editor-tab-unfocused-hover-background", unfocused_hover_background.clone());
    add("--modern-ui-editor-tab-active-hover-background", active_hover_background.clone());
    add("--modern-ui-editor-tab-unfocused-active-hover-background", unfocused_active_hover_background.clone());
    add("--modern-ui-editor-tab-active-foreground", legacy(TAB_ACTIVE_FOREGROUND, &[MODERN_EDITOR_TAB_ACTIVE_FOREGROUND, MODERN_TAB_ACTIVE_FOREGROUND], &[]));
    add(
        "--modern-ui-editor-tab-unfocused-active-foreground",
        legacy(TAB_UNFOCUSED_ACTIVE_FOREGROUND, &[MODERN_EDITOR_TAB_ACTIVE_FOREGROUND, MODERN_TAB_ACTIVE_FOREGROUND], &[TAB_ACTIVE_FOREGROUND]),
    );
    add("--modern-ui-editor-tab-inactive-foreground", legacy(TAB_INACTIVE_FOREGROUND, &[], &[TAB_ACTIVE_FOREGROUND]));
    add("--modern-ui-editor-tab-unfocused-inactive-foreground", legacy(TAB_UNFOCUSED_INACTIVE_FOREGROUND, &[], &[TAB_INACTIVE_FOREGROUND, TAB_ACTIVE_FOREGROUND]));
    add("--modern-ui-editor-tab-hover-foreground", legacy(TAB_HOVER_FOREGROUND, &[MODERN_EDITOR_TAB_HOVER_FOREGROUND, MODERN_TAB_HOVER_FOREGROUND], &[]));
    add(
        "--modern-ui-editor-tab-unfocused-hover-foreground",
        legacy(TAB_UNFOCUSED_HOVER_FOREGROUND, &[MODERN_EDITOR_TAB_HOVER_FOREGROUND, MODERN_TAB_HOVER_FOREGROUND], &[TAB_HOVER_FOREGROUND]),
    );
    add("--modern-ui-editor-tab-border", tab_border.clone());
    add("--modern-ui-editor-tab-custom-border", tab_border);
    add("--modern-ui-editor-tab-last-pinned-border", legacy(TAB_LAST_PINNED_BORDER, &[], &[]));
    add("--modern-ui-editor-tab-active-border", legacy(TAB_ACTIVE_BORDER, &[], &[]));
    add("--modern-ui-editor-tab-unfocused-active-border", legacy(TAB_UNFOCUSED_ACTIVE_BORDER, &[], &[TAB_ACTIVE_BORDER]));
    add("--modern-ui-editor-tab-active-border-top", active_border_top);
    add("--modern-ui-editor-tab-unfocused-active-border-top", unfocused_active_border_top);
    add("--modern-ui-editor-tab-custom-active-border-top", explicit_active_border_top);
    add("--modern-ui-editor-tab-custom-unfocused-active-border-top", explicit_unfocused_active_border_top);
    add("--modern-ui-editor-tab-hover-border", legacy(TAB_HOVER_BORDER, &[], &[]));
    add("--modern-ui-editor-tab-unfocused-hover-border", legacy(TAB_UNFOCUSED_HOVER_BORDER, &[], &[TAB_HOVER_BORDER]));
    add("--modern-ui-editor-tab-custom-active-background", explicit_active_background);
    add("--modern-ui-editor-tab-custom-unfocused-active-background", explicit_unfocused_active_background);
    add("--modern-ui-editor-tab-custom-inactive-background", explicit_inactive_background);
    add("--modern-ui-editor-tab-custom-unfocused-inactive-background", explicit_unfocused_inactive_background);
    add("--modern-ui-editor-tab-custom-hover-background", explicit_hover_background);
    add("--modern-ui-editor-tab-custom-unfocused-hover-background", explicit_unfocused_hover_background);
    add("--modern-ui-editor-tab-custom-active-hover-background", explicit_active_hover_background);
    add("--modern-ui-editor-tab-custom-unfocused-active-hover-background", explicit_unfocused_active_hover_background);
    add("--modern-ui-editor-tab-custom-action-active-background", explicit_active_action_background);
    add("--modern-ui-editor-tab-custom-action-hover-background", explicit_hover_action_background);
    add("--modern-ui-editor-tab-custom-action-active-hover-background", explicit_active_hover_action_background);
    add("--modern-ui-editor-tab-custom-action-selected-background", explicit_selected_action_background);

    let flatten = |color: &Option<Color>| color.as_ref().map(|color| flatten_tab_background(color, editor_background.as_ref()));
    if !has_modern_active_action_background {
        add("--modern-ui-editor-tab-action-active-background", flatten(&active_background));
        add("--modern-ui-editor-tab-action-unfocused-active-background", flatten(&unfocused_active_background));
    }
    if !has_modern_hover_action_background {
        add("--modern-ui-editor-tab-action-hover-background", flatten(&hover_background));
        add("--modern-ui-editor-tab-action-unfocused-hover-background", flatten(&unfocused_hover_background));
    }
    if !has_modern_active_hover_action_background {
        add("--modern-ui-editor-tab-action-active-hover-background", flatten(&active_hover_background));
        add("--modern-ui-editor-tab-action-unfocused-active-hover-background", flatten(&unfocused_active_hover_background));
    }
    variables
}
