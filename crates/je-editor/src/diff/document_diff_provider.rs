//! Porte de `src/vs/editor/common/diff/documentDiffProvider.ts`: o provedor de diferenças entre dois modelos
//! (`IDocumentDiffProvider`), as opções do cálculo (`IDocumentDiffProviderOptions`), o resultado dele (`IDocumentDiff`) e o
//! `nullDocumentDiff`.
//!
//! Divergências:
//! - `IDocumentDiffProviderOptions` tem os mesmos quatro campos de `ILinesDiffComputerOptions`
//!   (`ignoreTrimWhitespace`, `maxComputationTimeMs`, `computeMoves`, `extendToSubwords`) e o original passa uma à outra
//!   por tipagem estrutural: é um alias para [`LinesDiffComputerOptions`] (DRY).
//! - `ITextModel` é o [`TextModel`] concreto, recebido por `&Rc<TextModel>`; `Promise<IDocumentDiff>` é `Promise<DocumentDiff>`.
//! - `nullDocumentDiff` (um objeto congelado e compartilhado) é a função [`null_document_diff`], que devolve um valor novo e
//!   igual a cada chamada.

use std::rc::Rc;

use je_base::cancellation::CancellationToken;
use je_base::event::Event;
use je_base::promise::Promise;

use super::lines_diff_computer::{LinesDiffComputerOptions, MovedText};
use super::range_mapping::DetailedLineRangeMapping;
use crate::text_model::TextModel;

/// `IDocumentDiffProviderOptions`.
pub type DocumentDiffProviderOptions = LinesDiffComputerOptions;

/// `IDocumentDiff`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DocumentDiff {
    /// If true, both text models are identical (byte-by-byte).
    pub identical: bool,
    /// If true, the diff computation hit the timeout and the result might be an approximation.
    pub quit_early: bool,
    /// Maps all modified line ranges in the original to the corresponding line ranges in the modified text model.
    pub changes: Vec<DetailedLineRangeMapping>,
    /// Sorted by original line ranges. The original line ranges and the modified line ranges must be disjoint (but can be
    /// touching).
    pub moves: Vec<MovedText>,
}

/// `nullDocumentDiff`: idêntico, sem mudanças e sem movimentos.
pub fn null_document_diff() -> DocumentDiff {
    DocumentDiff { identical: true, quit_early: false, changes: Vec::new(), moves: Vec::new() }
}

/// `IDocumentDiffProvider`: um provedor calcula o diff entre dois modelos de texto.
pub trait IDocumentDiffProvider {
    /// Computes the diff between the text models `original` and `modified`.
    fn compute_diff(
        &self,
        original: &Rc<TextModel>,
        modified: &Rc<TextModel>,
        options: &DocumentDiffProviderOptions,
        cancellation_token: &CancellationToken,
    ) -> Promise<DocumentDiff>;

    /// Is fired when settings of the diff algorithm change that could alter the result of the diffing computation.
    /// Any user of this provider should recompute the diff when this event is fired.
    fn on_did_change(&self) -> Event<()>;
}
