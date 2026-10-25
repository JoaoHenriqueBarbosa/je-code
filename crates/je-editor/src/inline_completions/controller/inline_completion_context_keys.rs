//! Porte de `src/vs/editor/contrib/inlineCompletions/browser/controller/inlineCompletionContextKeys.ts`: as chaves de
//! contexto das sugestões em linha.
//!
//! Notas de tradução:
//! - `abstract class InlineCompletionContextKeys` com campos estáticos é a struct [`InlineCompletionContextKeys`], com as
//!   chaves num `thread_local` lido por [`inline_completion_context_keys`] (o mesmo padrão do `Context` do suggest);
//! - a `RawContextKey` do porte não é genérica: o `boolean | undefined` do original é o [`Dynamic`], e o valor padrão
//!   `undefined` de `suppressSuggestions` é [`Dynamic::Undefined`].

use je_base::dynamic::Dynamic;
use je_base::nls::localize;
use je_platform::contextkey::RawContextKey;

/// `InlineCompletionContextKeys.*`.
#[derive(Clone)]
pub struct InlineCompletionContextKeys {
    pub inline_suggestion_visible: RawContextKey,
    pub inline_suggestion_alternative_action_visible: RawContextKey,
    pub inline_suggestion_has_indentation: RawContextKey,
    pub inline_suggestion_has_indentation_less_than_tab_size: RawContextKey,
    pub suppress_suggestions: RawContextKey,

    pub cursor_before_ghost_text: RawContextKey,

    pub cursor_in_indentation: RawContextKey,
    pub has_selection: RawContextKey,
    pub cursor_at_inline_edit: RawContextKey,
    pub inline_edit_visible: RawContextKey,
    pub tab_should_jump_to_inline_edit: RawContextKey,
    pub tab_should_accept_inline_edit: RawContextKey,

    pub in_inline_edits_preview_editor: RawContextKey,
}

thread_local! {
    static KEYS: InlineCompletionContextKeys = InlineCompletionContextKeys {
        inline_suggestion_visible: RawContextKey::new(
            "inlineSuggestionVisible",
            false,
            localize("inlineSuggestionVisible", "Whether an inline suggestion is visible", &[]),
        ),
        inline_suggestion_alternative_action_visible: RawContextKey::new(
            "inlineSuggestionAlternativeActionVisible",
            false,
            localize(
                "inlineSuggestionAlternativeActionVisible",
                "Whether an alternative action for the inline suggestion is visible.",
                &[],
            ),
        ),
        inline_suggestion_has_indentation: RawContextKey::new(
            "inlineSuggestionHasIndentation",
            false,
            localize("inlineSuggestionHasIndentation", "Whether the inline suggestion starts with whitespace", &[]),
        ),
        inline_suggestion_has_indentation_less_than_tab_size: RawContextKey::new(
            "inlineSuggestionHasIndentationLessThanTabSize",
            true,
            localize(
                "inlineSuggestionHasIndentationLessThanTabSize",
                "Whether the inline suggestion starts with whitespace that is less than what would be inserted by tab",
                &[],
            ),
        ),
        suppress_suggestions: RawContextKey::new(
            "inlineSuggestionSuppressSuggestions",
            Dynamic::Undefined,
            localize("suppressSuggestions", "Whether suggestions should be suppressed for the current suggestion", &[]),
        ),

        cursor_before_ghost_text: RawContextKey::new(
            "cursorBeforeGhostText",
            false,
            localize("cursorBeforeGhostText", "Whether the cursor is at ghost text", &[]),
        ),

        cursor_in_indentation: RawContextKey::new(
            "cursorInIndentation",
            false,
            localize("cursorInIndentation", "Whether the cursor is in indentation", &[]),
        ),
        has_selection: RawContextKey::new(
            "editor.hasSelection",
            false,
            localize("editor.hasSelection", "Whether the editor has a selection", &[]),
        ),
        cursor_at_inline_edit: RawContextKey::new(
            "cursorAtInlineEdit",
            false,
            localize("cursorAtInlineEdit", "Whether the cursor is at an inline edit", &[]),
        ),
        inline_edit_visible: RawContextKey::new(
            "inlineEditIsVisible",
            false,
            localize("inlineEditVisible", "Whether an inline edit is visible", &[]),
        ),
        tab_should_jump_to_inline_edit: RawContextKey::new(
            "tabShouldJumpToInlineEdit",
            false,
            localize("tabShouldJumpToInlineEdit", "Whether tab should jump to an inline edit.", &[]),
        ),
        tab_should_accept_inline_edit: RawContextKey::new(
            "tabShouldAcceptInlineEdit",
            false,
            localize("tabShouldAcceptInlineEdit", "Whether tab should accept the inline edit.", &[]),
        ),

        in_inline_edits_preview_editor: RawContextKey::new(
            "inInlineEditsPreviewEditor",
            true,
            localize(
                "inInlineEditsPreviewEditor",
                "Whether the current code editor is showing an inline edits preview",
                &[],
            ),
        ),
    };
}

/// As chaves de contexto das sugestões em linha (as mesmas a cada chamada, no thread).
pub fn inline_completion_context_keys() -> InlineCompletionContextKeys {
    KEYS.with(Clone::clone)
}
