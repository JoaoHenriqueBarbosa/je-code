//! Porte de `src/vs/workbench/contrib/comments/common/commentContextKeys.ts`: as chaves de contexto de comentários
//! (`CommentContextKeys`).
//!
//! Notas de tradução:
//! - **`namespace CommentContextKeys`** são funções que devolvem o [`RawContextKey`] do `thread_local` (o `RawContextKey`
//!   registra a chave na lista central ao nascer, então cada uma nasce uma vez por thread). Os nomes são os do original em
//!   `snake_case`; `commentContext` virou [`comment_context`] e `commentThreadContext`, [`comment_thread_context`].
//! - As chaves sem valor padrão (`undefined`) usam [`Dynamic::Undefined`].

use je_base::dynamic::Dynamic;
use je_base::nls::localize;
use je_platform::contextkey::{ContextKeyMeta, RawContextKey};

/// Uma chave de `CommentContextKeys`: o nome, o valor padrão, o tipo e a descrição localizada (`nls_id` e `message`).
macro_rules! comment_context_key {
    ($(#[$doc:meta])* $name:ident, $key:literal, $default:expr, $ty:literal, $nls_id:literal, $message:literal) => {
        $(#[$doc])*
        pub fn $name() -> RawContextKey {
            thread_local! {
                static KEY: RawContextKey = RawContextKey::new(
                    $key,
                    $default,
                    ContextKeyMeta::Info { ty: $ty.to_owned(), description: localize($nls_id, $message, &[]) },
                );
            }
            KEY.with(Clone::clone)
        }
    };
}

comment_context_key!(
    /// A context key that is set when the active cursor is in a commenting range.
    active_cursor_has_commenting_range,
    "activeCursorHasCommentingRange",
    false,
    "boolean",
    "hasCommentingRange",
    "Whether the position at the active cursor has a commenting range"
);

comment_context_key!(
    /// A context key that is set when the active cursor is in the range of an existing comment.
    active_cursor_has_comment,
    "activeCursorHasComment",
    false,
    "boolean",
    "hasComment",
    "Whether the position at the active cursor has a comment"
);

comment_context_key!(
    /// A context key that is set when the active editor has commenting ranges.
    active_editor_has_commenting_range,
    "activeEditorHasCommentingRange",
    false,
    "boolean",
    "editorHasCommentingRange",
    "Whether the active editor has a commenting range"
);

comment_context_key!(
    /// A context key that is set when the workspace has either comments or commenting ranges.
    workspace_has_commenting,
    "workspaceHasCommenting",
    false,
    "boolean",
    "hasCommentingProvider",
    "Whether the open workspace has either comments or commenting ranges."
);

comment_context_key!(
    /// A context key that is set when the comment thread has no comments.
    comment_thread_is_empty,
    "commentThreadIsEmpty",
    false,
    "boolean",
    "commentThreadIsEmpty",
    "Set when the comment thread has no comments"
);

comment_context_key!(
    /// A context key that is set when the comment has no input.
    comment_is_empty,
    "commentIsEmpty",
    false,
    "boolean",
    "commentIsEmpty",
    "Set when the comment has no input"
);

comment_context_key!(
    /// The context value of the comment.
    comment_context,
    "comment",
    Dynamic::Undefined,
    "string",
    "comment",
    "The context value of the comment"
);

comment_context_key!(
    /// The context value of the comment thread.
    comment_thread_context,
    "commentThread",
    Dynamic::Undefined,
    "string",
    "commentThread",
    "The context value of the comment thread"
);

comment_context_key!(
    /// The comment controller id associated with a comment thread.
    comment_controller_context,
    "commentController",
    Dynamic::Undefined,
    "string",
    "commentController",
    "The comment controller id associated with a comment thread"
);

comment_context_key!(
    /// The comment widget is focused.
    comment_focused,
    "commentFocused",
    false,
    "boolean",
    "commentFocused",
    "Set when the comment is focused"
);

comment_context_key!(
    /// A context key that is set when a comment widget is visible in the editor.
    comment_widget_visible,
    "commentWidgetVisible",
    false,
    "boolean",
    "commentWidgetVisible",
    "Set when a comment widget is visible in the editor"
);

comment_context_key!(
    /// A context key that is set when commenting is enabled.
    commenting_enabled,
    "commentingEnabled",
    true,
    "boolean",
    "commentingEnabled",
    "Whether commenting functionality is enabled"
);
