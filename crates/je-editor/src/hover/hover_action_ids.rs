//! Porte de `src/vs/editor/contrib/hover/browser/hoverActionIds.ts`: os identificadores dos comandos do hover.
//!
//! `INCREASE_HOVER_VERBOSITY_ACTION_LABEL` e `DECREASE_HOVER_VERBOSITY_ACTION_LABEL` são `nls.localize(...)` avaliados na
//! carga do módulo; aqui são funções, porque o texto localizado depende do idioma ativo na hora da chamada. O
//! `comment` do tradutor (`Label for action that will increase the hover verbosity level.`) não tem lugar no `localize`
//! de `je-base`.

use je_base::nls::localize;

pub const SHOW_OR_FOCUS_HOVER_ACTION_ID: &str = "editor.action.showHover";
pub const SHOW_DEFINITION_PREVIEW_HOVER_ACTION_ID: &str = "editor.action.showDefinitionPreviewHover";
pub const HIDE_HOVER_ACTION_ID: &str = "editor.action.hideHover";
pub const SCROLL_UP_HOVER_ACTION_ID: &str = "editor.action.scrollUpHover";
pub const SCROLL_DOWN_HOVER_ACTION_ID: &str = "editor.action.scrollDownHover";
pub const SCROLL_LEFT_HOVER_ACTION_ID: &str = "editor.action.scrollLeftHover";
pub const SCROLL_RIGHT_HOVER_ACTION_ID: &str = "editor.action.scrollRightHover";
pub const PAGE_UP_HOVER_ACTION_ID: &str = "editor.action.pageUpHover";
pub const PAGE_DOWN_HOVER_ACTION_ID: &str = "editor.action.pageDownHover";
pub const GO_TO_TOP_HOVER_ACTION_ID: &str = "editor.action.goToTopHover";
pub const GO_TO_BOTTOM_HOVER_ACTION_ID: &str = "editor.action.goToBottomHover";
pub const INCREASE_HOVER_VERBOSITY_ACTION_ID: &str = "editor.action.increaseHoverVerbosityLevel";
pub const INCREASE_HOVER_VERBOSITY_ACCESSIBLE_ACTION_ID: &str = "editor.action.increaseHoverVerbosityLevelFromAccessibleView";
pub const DECREASE_HOVER_VERBOSITY_ACTION_ID: &str = "editor.action.decreaseHoverVerbosityLevel";
pub const DECREASE_HOVER_VERBOSITY_ACCESSIBLE_ACTION_ID: &str = "editor.action.decreaseHoverVerbosityLevelFromAccessibleView";
pub const HIDE_LONG_LINE_WARNING_HOVER_ACTION_ID: &str = "editor.action.hideLongLineWarningHover";

/// `INCREASE_HOVER_VERBOSITY_ACTION_LABEL`.
pub fn increase_hover_verbosity_action_label() -> String {
    localize("increaseHoverVerbosityLevel", "Increase Hover Verbosity Level", &[])
}

/// `DECREASE_HOVER_VERBOSITY_ACTION_LABEL`.
pub fn decrease_hover_verbosity_action_label() -> String {
    localize("decreaseHoverVerbosityLevel", "Decrease Hover Verbosity Level", &[])
}
