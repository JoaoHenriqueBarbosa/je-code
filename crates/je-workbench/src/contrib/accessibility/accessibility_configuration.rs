//! Porte do recorte de `src/vs/workbench/contrib/accessibility/browser/accessibilityConfiguration.ts` e de
//! `common/accessibilityCommands.ts` que a visão acessível lê: as chaves de contexto, os ids de configuração e os ids de
//! comando.
//!
//! Notas de tradução:
//! - **`new RawContextKey<boolean>(id, false, true)`** é [`RawContextKey::new`] com [`ContextKeyMeta::Hide`]; as chaves
//!   com `undefined` por padrão usam [`Dynamic::Undefined`]. Cada chave é um `thread_local` para registrar a informação
//!   da chave uma única vez, como a constante do módulo do original.

use je_base::dynamic::Dynamic;
use je_platform::contextkey::contextkey::{ContextKeyMeta, RawContextKey};

macro_rules! context_keys {
    ($($(#[$doc:meta])* $function:ident, $static_name:ident: $id:literal, $default:expr, $meta:expr;)*) => {
        thread_local! {
            $(static $static_name: RawContextKey = RawContextKey::new($id, $default, $meta);)*
        }

        $(
            $(#[$doc])*
            pub fn $function() -> RawContextKey {
                $static_name.with(RawContextKey::clone)
            }
        )*
    };
}

context_keys! {
    /// `accessibilityHelpIsShown`.
    accessibility_help_is_shown, ACCESSIBILITY_HELP_IS_SHOWN: "accessibilityHelpIsShown", false, ContextKeyMeta::Hide;
    /// `accessibleViewIsShown`.
    accessible_view_is_shown, ACCESSIBLE_VIEW_IS_SHOWN: "accessibleViewIsShown", false, ContextKeyMeta::Hide;
    /// `accessibleViewSupportsNavigation`.
    accessible_view_supports_navigation, ACCESSIBLE_VIEW_SUPPORTS_NAVIGATION: "accessibleViewSupportsNavigation", false, ContextKeyMeta::Hide;
    /// `accessibleViewVerbosityEnabled`.
    accessible_view_verbosity_enabled, ACCESSIBLE_VIEW_VERBOSITY_ENABLED: "accessibleViewVerbosityEnabled", false, ContextKeyMeta::Hide;
    /// `accessibleViewGoToSymbolSupported`.
    accessible_view_go_to_symbol_supported, ACCESSIBLE_VIEW_GO_TO_SYMBOL_SUPPORTED: "accessibleViewGoToSymbolSupported", false, ContextKeyMeta::Hide;
    /// `accessibleViewOnLastLine`.
    accessible_view_on_last_line, ACCESSIBLE_VIEW_ON_LAST_LINE: "accessibleViewOnLastLine", false, ContextKeyMeta::Hide;
    /// `accessibleViewCurrentProviderId`.
    accessible_view_current_provider_id, ACCESSIBLE_VIEW_CURRENT_PROVIDER_ID: "accessibleViewCurrentProviderId", Dynamic::Undefined, ContextKeyMeta::None;
    /// `accessibleViewInCodeBlock`.
    accessible_view_in_code_block, ACCESSIBLE_VIEW_IN_CODE_BLOCK: "accessibleViewInCodeBlock", Dynamic::Undefined, ContextKeyMeta::None;
    /// `accessibleViewContainsCodeBlocks`.
    accessible_view_contains_code_blocks, ACCESSIBLE_VIEW_CONTAINS_CODE_BLOCKS: "accessibleViewContainsCodeBlocks", Dynamic::Undefined, ContextKeyMeta::None;
    /// `accessibleViewHasUnassignedKeybindings`.
    accessible_view_has_unassigned_keybindings, ACCESSIBLE_VIEW_HAS_UNASSIGNED_KEYBINDINGS: "accessibleViewHasUnassignedKeybindings", Dynamic::Undefined, ContextKeyMeta::None;
    /// `accessibleViewHasAssignedKeybindings`.
    accessible_view_has_assigned_keybindings, ACCESSIBLE_VIEW_HAS_ASSIGNED_KEYBINDINGS: "accessibleViewHasAssignedKeybindings", Dynamic::Undefined, ContextKeyMeta::None;
}

/// `AccessibilityWorkbenchSettingId.HideAccessibleView`.
pub const HIDE_ACCESSIBLE_VIEW_SETTING: &str = "accessibility.hideAccessibleView";
/// `AccessibilityWorkbenchSettingId.AccessibleViewCloseOnKeyPress`.
pub const ACCESSIBLE_VIEW_CLOSE_ON_KEY_PRESS_SETTING: &str = "accessibility.accessibleView.closeOnKeyPress";

/// `AccessibilityCommandId`, só os ids que a visão acessível cita.
pub mod accessibility_command_id {
    pub const OPEN_ACCESSIBLE_VIEW: &str = "editor.action.accessibleView";
    pub const DISABLE_VERBOSITY_HINT: &str = "editor.action.accessibleViewDisableHint";
    pub const GO_TO_SYMBOL: &str = "editor.action.accessibleViewGoToSymbol";
    pub const SHOW_NEXT: &str = "editor.action.accessibleViewNext";
    pub const SHOW_PREVIOUS: &str = "editor.action.accessibleViewPrevious";
    pub const ACCESSIBILITY_HELP_CONFIGURE_KEYBINDINGS: &str = "editor.action.accessibilityHelpConfigureKeybindings";
    pub const ACCESSIBILITY_HELP_CONFIGURE_ASSIGNED_KEYBINDINGS: &str = "editor.action.accessibilityHelpConfigureAssignedKeybindings";
    pub const ACCESSIBILITY_HELP_OPEN_HELP_LINK: &str = "editor.action.accessibilityHelpOpenHelpLink";
}

/// `ACCESSIBLE_VIEW_SHOWN_STORAGE_PREFIX` (`platform/accessibility/common/accessibility.ts`).
pub const ACCESSIBLE_VIEW_SHOWN_STORAGE_PREFIX: &str = "ACCESSIBLE_VIEW_SHOWN_";
