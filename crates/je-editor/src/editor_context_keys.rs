//! Porte parcial de `src/vs/editor/common/editorContextKeys.ts`: as chaves de contexto de provedor de formatação de
//! `EditorContextKeys` (`hasDocumentFormattingProvider`, `hasDocumentSelectionFormattingProvider`,
//! `hasMultipleDocumentFormattingProvider` e `hasMultipleDocumentSelectionFormattingProvider`).
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
