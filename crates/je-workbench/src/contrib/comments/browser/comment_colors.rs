//! Porte de `src/vs/workbench/contrib/comments/browser/commentColors.ts`: as cores dos comentários e as funções que dão a cor de uma
//! thread pelo estado dela.
//!
//! Divergências:
//! - **`registerColor`.** As sete cores (`commentsView.resolvedIcon`, `commentsView.unresolvedIcon`,
//!   `editorCommentsWidget.replyInputBackground`, `editorCommentsWidget.resolvedBorder`, `editorCommentsWidget.unresolvedBorder`,
//!   `editorCommentsWidget.rangeBackground` e `editorCommentsWidget.rangeActiveBackground`) já estão no registro do `je-theme`
//!   (`registered_colors.rs`, gerado das chamadas `registerColor` de `ref/vscode/src/vs`); aqui ficam os ids como constantes
//!   (`ColorIdentifier` é o texto do id). Só as duas que o original exporta são públicas.
//! - **`Map<CommentThreadState, string>`** (`commentThreadStateBorderColors` e `commentThreadStateIconColors`) são os `match` de
//!   [`comment_thread_state_border_color_id`] e [`comment_thread_state_icon_color_id`], exaustivos sobre os dois estados.
//! - **Variáveis de CSS.** `--comment-thread-state-color` e as duas irmãs não têm folha de estilo na árvore nativa; as constantes
//!   seguem exportadas com o nome do original para quem monta o estilo do nó.

use je_editor::languages::CommentThreadState;
use je_editor::services::theme_service::IColorTheme;
use je_theme::color::Color;

const RESOLVED_COMMENT_VIEW_ICON: &str = "commentsView.resolvedIcon";
const UNRESOLVED_COMMENT_VIEW_ICON: &str = "commentsView.unresolvedIcon";

const RESOLVED_COMMENT_BORDER: &str = "editorCommentsWidget.resolvedBorder";
const UNRESOLVED_COMMENT_BORDER: &str = "editorCommentsWidget.unresolvedBorder";

pub const COMMENT_THREAD_RANGE_BACKGROUND: &str = "editorCommentsWidget.rangeBackground";
pub const COMMENT_THREAD_RANGE_ACTIVE_BACKGROUND: &str = "editorCommentsWidget.rangeActiveBackground";

pub const COMMENT_THREAD_STATE_COLOR_VAR: &str = "--comment-thread-state-color";
pub const COMMENT_VIEW_THREAD_STATE_COLOR_VAR: &str = "--comment-view-thread-state-color";
pub const COMMENT_THREAD_STATE_BACKGROUND_COLOR_VAR: &str = "--comment-thread-state-background-color";

/// `commentThreadStateBorderColors.get(state)`.
fn comment_thread_state_border_color_id(state: CommentThreadState) -> &'static str {
    match state {
        CommentThreadState::Unresolved => UNRESOLVED_COMMENT_BORDER,
        CommentThreadState::Resolved => RESOLVED_COMMENT_BORDER,
    }
}

/// `commentThreadStateIconColors.get(state)`.
fn comment_thread_state_icon_color_id(state: CommentThreadState) -> &'static str {
    match state {
        CommentThreadState::Unresolved => UNRESOLVED_COMMENT_VIEW_ICON,
        CommentThreadState::Resolved => RESOLVED_COMMENT_VIEW_ICON,
    }
}

/// `getCommentThreadStateColor(state, theme, map)`: `map` é a função de `state` para o id da cor.
fn get_comment_thread_state_color(state: Option<CommentThreadState>, theme: &dyn IColorTheme, color_id: fn(CommentThreadState) -> &'static str) -> Option<Color> {
    state.and_then(|state| theme.get_color(color_id(state), None))
}

/// `getCommentThreadStateBorderColor(state, theme)`.
pub fn get_comment_thread_state_border_color(state: Option<CommentThreadState>, theme: &dyn IColorTheme) -> Option<Color> {
    get_comment_thread_state_color(state, theme, comment_thread_state_border_color_id)
}

/// `getCommentThreadStateIconColor(state, theme)`.
pub fn get_comment_thread_state_icon_color(state: Option<CommentThreadState>, theme: &dyn IColorTheme) -> Option<Color> {
    get_comment_thread_state_color(state, theme, comment_thread_state_icon_color_id)
}
