//! O trecho de `src/vs/editor/common/model.ts` que a API do buffer de texto usa: preferências de fim de
//! linha, snapshot, operações de edição, resultado de `applyEdits` e dados de busca.
//!
//! Textos são UTF-16 ([`JsString`]); o `text: string | null` das operações é `Option<JsString>`.

use std::rc::Rc;

use je_base::JsString;

use crate::js_reg_exp::JsRegExp;
use crate::range::Range;
use crate::text_change::TextChange;
use crate::word_character_classifier::WordCharacterClassifier;

/// `EndOfLinePreference`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EndOfLinePreference {
    /// O fim de linha do próprio buffer.
    TextDefined = 0,
    /// `\n`.
    Lf = 1,
    /// `\r\n`.
    CrLf = 2,
}

/// `DefaultEndOfLine`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DefaultEndOfLine {
    Lf = 1,
    CrLf = 2,
}

/// O fim de linha de um buffer: `'\r\n' | '\n'` do original.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Eol {
    Lf,
    CrLf,
}

impl Eol {
    /// O texto do fim de linha.
    pub fn as_units(self) -> &'static [u16] {
        match self {
            Eol::Lf => &[10],
            Eol::CrLf => &[13, 10],
        }
    }

    pub fn len(self) -> usize {
        self.as_units().len()
    }

    pub fn is_empty(self) -> bool {
        false
    }
}

/// `ISingleEditOperationIdentifier`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SingleEditOperationIdentifier {
    pub major: i32,
    pub minor: i32,
}

/// `FindMatch`: o intervalo e, se pedido, os grupos (`None` para o grupo que não participou).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FindMatch {
    pub range: Range,
    pub matches: Option<Vec<Option<JsString>>>,
}

/// `ITextSnapshot`: um iterador de pedaços (em torno de 64 KB) que devolve `None` ao terminar.
pub trait TextSnapshot {
    fn read(&mut self) -> Option<JsString>;
}

/// `ValidAnnotatedEditOperation`.
#[derive(Clone, Debug)]
pub struct ValidAnnotatedEditOperation {
    pub identifier: Option<SingleEditOperationIdentifier>,
    pub range: Range,
    pub text: Option<JsString>,
    pub force_move_markers: bool,
    pub is_auto_whitespace_edit: bool,
    pub is_tracked: bool,
}

/// `IValidEditOperation`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ValidEditOperation {
    pub identifier: Option<SingleEditOperationIdentifier>,
    /// O intervalo a trocar; vazio emula uma inserção.
    pub range: Range,
    /// O texto novo; vazio emula uma remoção.
    pub text: JsString,
    pub text_change: TextChange,
}

/// `IInternalModelContentChange`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct InternalModelContentChange {
    pub range: Range,
    pub range_offset: usize,
    pub range_length: usize,
    pub text: JsString,
    pub force_move_markers: bool,
}

/// `ApplyEditsResult`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ApplyEditsResult {
    pub reverse_edits: Option<Vec<ValidEditOperation>>,
    pub changes: Vec<InternalModelContentChange>,
    pub trim_auto_whitespace_line_numbers: Option<Vec<i32>>,
}

/// `SearchData`.
#[derive(Debug)]
pub struct SearchData {
    /// A regex a procurar.
    pub regex: JsRegExp,
    /// O classificador de separadores de palavra, para buscar por palavra inteira.
    pub word_separators: Option<Rc<WordCharacterClassifier>>,
    /// O texto simples a procurar, quando dá para evitar a regex.
    pub simple_search: Option<JsString>,
}

impl SearchData {
    pub fn new(regex: JsRegExp, word_separators: Option<Rc<WordCharacterClassifier>>, simple_search: Option<JsString>) -> Self {
        Self {
            regex,
            word_separators,
            simple_search,
        }
    }
}
