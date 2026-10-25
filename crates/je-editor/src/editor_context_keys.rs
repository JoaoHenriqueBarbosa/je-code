//! Porte parcial de `src/vs/editor/common/editorContextKeys.ts`: as chaves de contexto de provedor de formatação de
//! `EditorContextKeys` (`hasDocumentFormattingProvider`, `hasDocumentSelectionFormattingProvider`,
//! `hasMultipleDocumentFormattingProvider` e `hasMultipleDocumentSelectionFormattingProvider`), a
//! `multiDiffEditorItemAllUnchangedRegionsShown`, as do sticky scroll (`stickyScrollFocused` e `stickyScrollVisible`), as de
//! provedor de navegação (`hasDefinitionProvider`, `hasDeclarationProvider`, `hasImplementationProvider`,
//! `hasTypeDefinitionProvider` e `hasReferenceProvider`) e a `isInEmbeddedEditor`.
//!
//! As chaves são `thread_local` e as funções devolvem cópias (o mesmo padrão de `find_model`); as outras chaves do
//! namespace entram aqui quando a contribuição que as usa for portada.

use je_base::nls::localize;
use je_platform::contextkey::{ContextKeyMeta, RawContextKey};

thread_local! {
    static HAS_DEFINITION_PROVIDER: RawContextKey = RawContextKey::new(
        "editorHasDefinitionProvider",
        false,
        ContextKeyMeta::Description(localize("editorHasDefinitionProvider", "Whether the editor has a definition provider", &[])),
    );
    static HAS_DECLARATION_PROVIDER: RawContextKey = RawContextKey::new(
        "editorHasDeclarationProvider",
        false,
        ContextKeyMeta::Description(localize("editorHasDeclarationProvider", "Whether the editor has a declaration provider", &[])),
    );
    static HAS_IMPLEMENTATION_PROVIDER: RawContextKey = RawContextKey::new(
        "editorHasImplementationProvider",
        false,
        ContextKeyMeta::Description(localize("editorHasImplementationProvider", "Whether the editor has an implementation provider", &[])),
    );
    static HAS_TYPE_DEFINITION_PROVIDER: RawContextKey = RawContextKey::new(
        "editorHasTypeDefinitionProvider",
        false,
        ContextKeyMeta::Description(localize("editorHasTypeDefinitionProvider", "Whether the editor has a type definition provider", &[])),
    );
    static HAS_REFERENCE_PROVIDER: RawContextKey = RawContextKey::new(
        "editorHasReferenceProvider",
        false,
        ContextKeyMeta::Description(localize("editorHasReferenceProvider", "Whether the editor has a reference provider", &[])),
    );
    static IS_IN_EMBEDDED_EDITOR: RawContextKey = RawContextKey::new("isInEmbeddedEditor", false, ContextKeyMeta::Hide);
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

/// `EditorContextKeys.hasDefinitionProvider`.
pub fn has_definition_provider() -> RawContextKey {
    HAS_DEFINITION_PROVIDER.with(Clone::clone)
}

/// `EditorContextKeys.hasDeclarationProvider`.
pub fn has_declaration_provider() -> RawContextKey {
    HAS_DECLARATION_PROVIDER.with(Clone::clone)
}

/// `EditorContextKeys.hasImplementationProvider`.
pub fn has_implementation_provider() -> RawContextKey {
    HAS_IMPLEMENTATION_PROVIDER.with(Clone::clone)
}

/// `EditorContextKeys.hasTypeDefinitionProvider`.
pub fn has_type_definition_provider() -> RawContextKey {
    HAS_TYPE_DEFINITION_PROVIDER.with(Clone::clone)
}

/// `EditorContextKeys.hasReferenceProvider`.
pub fn has_reference_provider() -> RawContextKey {
    HAS_REFERENCE_PROVIDER.with(Clone::clone)
}

/// `EditorContextKeys.isInEmbeddedEditor`.
pub fn is_in_embedded_editor() -> RawContextKey {
    IS_IN_EMBEDDED_EDITOR.with(Clone::clone)
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
