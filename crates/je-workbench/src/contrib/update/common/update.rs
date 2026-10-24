//! Porte de `src/vs/workbench/contrib/update/common/update.ts`: os identificadores das ações de notas de versão e as
//! chaves e expressões de contexto do indicador de atualização.
//!
//! Divergência: no original as constantes são criadas quando o módulo carrega. Aqui cada chave é criada na primeira
//! chamada da sua função (o mesmo modelo de [`crate::common::contextkeys`]), e cada expressão é montada a cada chamada
//! a partir delas.

use je_platform::contextkey::contextkey::{ContextKeyExpr, ContextKeyExpression, ContextKeyMeta, RawContextKey};

use crate::common::contextkeys::{in_editor_zen_mode_context, menu_bar_visible_context};

/// `ShowCurrentReleaseNotesActionId`.
pub const SHOW_CURRENT_RELEASE_NOTES_ACTION_ID: &str = "update.showCurrentReleaseNotes";
/// `ShowCurrentReleaseNotesFromCurrentFileActionId`.
pub const SHOW_CURRENT_RELEASE_NOTES_FROM_CURRENT_FILE_ACTION_ID: &str = "developer.showCurrentFileAsReleaseNotes";

/// `UpdateTitleBarContext`.
pub fn update_title_bar_context() -> RawContextKey {
    thread_local! { static KEY: RawContextKey = RawContextKey::new("updateTitleBar", false, ContextKeyMeta::None); }
    KEY.with(Clone::clone)
}

/// `UpdateTitleBarChatInProgressContext`.
pub fn update_title_bar_chat_in_progress_context() -> RawContextKey {
    thread_local! { static KEY: RawContextKey = RawContextKey::new("updateTitleBarChatRequestInProgress", false, ContextKeyMeta::None); }
    KEY.with(Clone::clone)
}

/// `UpdateTitleBarEditorVisibleContext`.
pub fn update_title_bar_editor_visible_context() -> ContextKeyExpression {
    ContextKeyExpr::and([
        update_title_bar_context().expr(),
        in_editor_zen_mode_context().to_negated(),
        ContextKeyExpr::not("inDebugMode"),
        update_title_bar_chat_in_progress_context().to_negated(),
    ])
    .expect("a conjunção tem termos")
}

/// `UpdateGlobalActivityBadgeVisibleContext`.
pub fn update_global_activity_badge_visible_context() -> ContextKeyExpression {
    ContextKeyExpr::or([
        update_title_bar_editor_visible_context().negate(),
        menu_bar_visible_context().negate(),
        ContextKeyExpr::not_equals("config.workbench.activityBar.location", "top"),
    ])
    .expect("a disjunção tem termos")
}
