//! Porte de `src/vs/editor/common/core/editOperation.ts` e do `IIdentifiedSingleEditOperation` de `model.ts`.
//!
//! `ISingleEditOperation` e `IIdentifiedSingleEditOperation` do original são a mesma estrutura com campos
//! opcionais a mais; aqui há um tipo só, [`SingleEditOperation`], com os opcionais no valor neutro
//! (`identifier: None`, `force_move_markers: false`, ...).

use je_base::JsString;

use crate::model::SingleEditOperationIdentifier;
use crate::position::Position;
use crate::range::Range;

/// Uma operação de edição simples: troca o texto em `range` por `text`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SingleEditOperation {
    /// O intervalo a trocar; vazio emula uma inserção.
    pub range: Range,
    /// O texto novo; `None` emula uma remoção.
    pub text: Option<JsString>,
    /// Semântica de inserção: com `range` colapsado, todos os marcadores na posição se movem.
    pub force_move_markers: bool,
    /// `identifier` do `IIdentifiedSingleEditOperation`.
    pub identifier: Option<SingleEditOperationIdentifier>,
    /// `isAutoWhitespaceEdit`.
    pub is_auto_whitespace_edit: bool,
    /// `_isTracked`.
    pub is_tracked: bool,
}

/// `IIdentifiedSingleEditOperation`.
pub type IdentifiedSingleEditOperation = SingleEditOperation;

impl SingleEditOperation {
    pub fn new(range: Range, text: Option<JsString>, force_move_markers: bool) -> Self {
        Self {
            range,
            text,
            force_move_markers,
            identifier: None,
            is_auto_whitespace_edit: false,
            is_tracked: false,
        }
    }
}

/// `EditOperation`.
pub struct EditOperation;

impl EditOperation {
    pub fn insert(position: Position, text: impl Into<JsString>) -> SingleEditOperation {
        SingleEditOperation::new(
            Range::new(position.line_number, position.column, position.line_number, position.column),
            Some(text.into()),
            true,
        )
    }

    pub fn delete(range: Range) -> SingleEditOperation {
        SingleEditOperation::new(range, None, false)
    }

    pub fn replace(range: Range, text: Option<JsString>) -> SingleEditOperation {
        SingleEditOperation::new(range, text, false)
    }

    pub fn replace_move(range: Range, text: Option<JsString>) -> SingleEditOperation {
        SingleEditOperation::new(range, text, true)
    }
}
