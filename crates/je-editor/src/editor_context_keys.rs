//! Porte parcial de `src/vs/editor/common/editorContextKeys.ts`: as chaves de contexto de provedor de formatação de
//! `EditorContextKeys` (`hasDocumentFormattingProvider`, `hasDocumentSelectionFormattingProvider`,
//! `hasMultipleDocumentFormattingProvider` e `hasMultipleDocumentSelectionFormattingProvider`), a
//! `multiDiffEditorItemAllUnchangedRegionsShown` e as do sticky scroll (`stickyScrollFocused` e `stickyScrollVisible`).
//!
//! As chaves são `thread_local` e as funções devolvem cópias (o mesmo padrão de `find_model`); as outras chaves do
//! namespace entram aqui quando a contribuição que as usa for portada.

use je_platform::contextkey::{ContextKeyMeta, RawContextKey};

thread_local! {
    static HAS_DOCUMENT_FORMATTING_PROVIDER: RawContextKey = RawContextKey::new(
        "editorHasDocumentFormattingProvider",
        false,
        ContextKeyMeta::Description("Whether the editor has a document formatting provider".to_owned()),
    );
    static HAS_DOCUMENT_SELECTION_FORMATTING_PROVIDER: RawContextKey = RawContextKey::new(
        "editorHasDocumentSelectionFormattingProvider",
        false,
        ContextKeyMeta::Description("Whether the editor has a document selection formatting provider".to_owned()),
    );
    static HAS_MULTIPLE_DOCUMENT_FORMATTING_PROVIDER: RawContextKey = RawContextKey::new(
        "editorHasMultipleDocumentFormattingProvider",
        false,
        ContextKeyMeta::Description("Whether the editor has multiple document formatting providers".to_owned()),
    );
    static HAS_MULTIPLE_DOCUMENT_SELECTION_FORMATTING_PROVIDER: RawContextKey = RawContextKey::new(
        "editorHasMultipleDocumentSelectionFormattingProvider",
        false,
        ContextKeyMeta::Description("Whether the editor has multiple document selection formatting providers".to_owned()),
    );
    static MULTI_DIFF_EDITOR_ITEM_ALL_UNCHANGED_REGIONS_SHOWN: RawContextKey = RawContextKey::new(
        "multiDiffEditorItemAllUnchangedRegionsShown",
        false,
        ContextKeyMeta::Description("Whether all unchanged regions of a multi diff editor file are revealed".to_owned()),
    );
    static STICKY_SCROLL_FOCUSED: RawContextKey = RawContextKey::new(
        "stickyScrollFocused",
        false,
        ContextKeyMeta::Description("Whether the sticky scroll is focused".to_owned()),
    );
    static STICKY_SCROLL_VISIBLE: RawContextKey = RawContextKey::new(
        "stickyScrollVisible",
        false,
        ContextKeyMeta::Description("Whether the sticky scroll is visible".to_owned()),
    );
}

/// `EditorContextKeys.stickyScrollFocused`.
pub fn sticky_scroll_focused() -> RawContextKey {
    STICKY_SCROLL_FOCUSED.with(Clone::clone)
}

/// `EditorContextKeys.stickyScrollVisible`.
pub fn sticky_scroll_visible() -> RawContextKey {
    STICKY_SCROLL_VISIBLE.with(Clone::clone)
}

/// `EditorContextKeys.multiDiffEditorItemAllUnchangedRegionsShown`.
pub fn multi_diff_editor_item_all_unchanged_regions_shown() -> RawContextKey {
    MULTI_DIFF_EDITOR_ITEM_ALL_UNCHANGED_REGIONS_SHOWN.with(Clone::clone)
}

/// `EditorContextKeys.hasDocumentFormattingProvider`.
pub fn has_document_formatting_provider() -> RawContextKey {
    HAS_DOCUMENT_FORMATTING_PROVIDER.with(Clone::clone)
}

/// `EditorContextKeys.hasDocumentSelectionFormattingProvider`.
pub fn has_document_selection_formatting_provider() -> RawContextKey {
    HAS_DOCUMENT_SELECTION_FORMATTING_PROVIDER.with(Clone::clone)
}

/// `EditorContextKeys.hasMultipleDocumentFormattingProvider`.
pub fn has_multiple_document_formatting_provider() -> RawContextKey {
    HAS_MULTIPLE_DOCUMENT_FORMATTING_PROVIDER.with(Clone::clone)
}

/// `EditorContextKeys.hasMultipleDocumentSelectionFormattingProvider`.
pub fn has_multiple_document_selection_formatting_provider() -> RawContextKey {
    HAS_MULTIPLE_DOCUMENT_SELECTION_FORMATTING_PROVIDER.with(Clone::clone)
}

/// Declara, de uma lista só, as chaves de contexto do editor de diff (`EditorContextKeys.isEmbeddedDiffEditor` e as demais que o
/// `DiffEditorWidget` escreve): a chave `thread_local` e a função que devolve uma cópia.
macro_rules! diff_editor_context_keys {
    ($($(#[$doc:meta])* $static_name:ident, $getter:ident, $key:literal, $default:expr, $description:literal;)*) => {
        thread_local! {
            $(static $static_name: RawContextKey = RawContextKey::new($key, $default, ContextKeyMeta::Description($description.to_owned()));)*
        }

        $(
            $(#[$doc])*
            pub fn $getter() -> RawContextKey {
                $static_name.with(Clone::clone)
            }
        )*
    };
}

diff_editor_context_keys! {
    /// `EditorContextKeys.isEmbeddedDiffEditor`.
    IS_EMBEDDED_DIFF_EDITOR, is_embedded_diff_editor, "isEmbeddedDiffEditor", false, "Whether the context is an embedded diff editor";
    /// `EditorContextKeys.hasChanges`.
    HAS_CHANGES, has_changes, "diffEditorHasChanges", false, "Whether the diff editor has changes";
    /// `EditorContextKeys.comparingMovedCode`.
    COMPARING_MOVED_CODE, comparing_moved_code, "comparingMovedCode", false, "Whether a moved code block is selected for comparison";
    /// `EditorContextKeys.diffEditorRenderSideBySideInlineBreakpointReached`.
    DIFF_EDITOR_RENDER_SIDE_BY_SIDE_INLINE_BREAKPOINT_REACHED, diff_editor_render_side_by_side_inline_breakpoint_reached,
        "diffEditorRenderSideBySideInlineBreakpointReached", false, "Whether the diff editor render side by side inline breakpoint is reached";
    /// `EditorContextKeys.diffEditorInlineMode`.
    DIFF_EDITOR_INLINE_MODE, diff_editor_inline_mode, "diffEditorInlineMode", false, "Whether inline mode is active";
    /// `EditorContextKeys.diffEditorTemporaryInlineMode`.
    DIFF_EDITOR_TEMPORARY_INLINE_MODE, diff_editor_temporary_inline_mode, "diffEditorTemporaryInlineMode", false,
        "Whether inline mode is temporarily active after manually resizing the diff editor";
    /// `EditorContextKeys.diffEditorAutomaticRenderSideBySide`.
    DIFF_EDITOR_AUTOMATIC_RENDER_SIDE_BY_SIDE, diff_editor_automatic_render_side_by_side, "diffEditorAutomaticRenderSideBySide", true,
        "Whether automatic diff editor layout would render side by side";
    /// `EditorContextKeys.diffEditorOriginalWritable`.
    DIFF_EDITOR_ORIGINAL_WRITABLE, diff_editor_original_writable, "diffEditorOriginalWritable", false, "Whether modified is writable in the diff editor";
    /// `EditorContextKeys.diffEditorModifiedWritable`.
    DIFF_EDITOR_MODIFIED_WRITABLE, diff_editor_modified_writable, "diffEditorModifiedWritable", false, "Whether modified is writable in the diff editor";
    /// `EditorContextKeys.diffEditorOriginalUri`.
    DIFF_EDITOR_ORIGINAL_URI, diff_editor_original_uri, "diffEditorOriginalUri", String::new(), "The uri of the original document";
    /// `EditorContextKeys.diffEditorModifiedUri`.
    DIFF_EDITOR_MODIFIED_URI, diff_editor_modified_uri, "diffEditorModifiedUri", String::new(), "The uri of the modified document";
}
