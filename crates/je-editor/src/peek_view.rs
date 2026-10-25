//! Porte parcial de `src/vs/editor/contrib/peekView/browser/peekView.ts`: o `PeekContext`, as chaves de contexto que
//! dizem se o editor atual vive dentro de um peek.
//!
//! Pendentes (a fatia do peek): o `PeekViewWidget`, o `PeekContextController` (a contribuição que liga a chave
//! `inReferenceSearchEditor` ao serviço de chaves do editor), `peekViewTitleBackground` e as demais cores, e a
//! `IPeekViewService`. O `PeekContext` entra antes porque as ações de `goToCommands.ts` filtram por ele.
//!
//! As chaves são `thread_local` e as funções devolvem cópias (o mesmo padrão de `editor_context_keys`).

use je_base::nls::localize;
use je_platform::contextkey::{ContextKeyExpression, RawContextKey};

thread_local! {
    static IN_PEEK_EDITOR: RawContextKey = RawContextKey::new(
        "inReferenceSearchEditor",
        true,
        localize("inReferenceSearchEditor", "Whether the current code editor is embedded inside peek", &[]),
    );
}

/// `PeekContext.inPeekEditor`.
pub fn in_peek_editor() -> RawContextKey {
    IN_PEEK_EDITOR.with(Clone::clone)
}

/// `PeekContext.notInPeekEditor`: `inPeekEditor.toNegated()`.
pub fn not_in_peek_editor() -> ContextKeyExpression {
    in_peek_editor().to_negated()
}
