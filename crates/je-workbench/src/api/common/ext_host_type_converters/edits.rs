//! `extHostTypeConverters.ts`: `EndOfLine` e `TextEdit`, entre as classes da API e o `languages.TextEdit`.
//!
//! Divergências:
//! - O `languages.TextEdit` é o do editor (`je_editor::code_action::languages::TextEdit`), que também carrega
//!   `insertAsSnippet` e `keepWhitespace`; `TextEdit.from` os deixa ausentes, como o original.
//! - `EndOfLine.from` do original devolve `undefined` para um valor que não é `LF` nem `CRLF`; o enum da API
//!   só tem os dois, então a conversão é total.

use je_base::errors::Error;
use je_editor::code_action::languages::TextEdit as EditorTextEdit;
use je_editor::model::EndOfLineSequence;

use super::geometry::Range;
use crate::api::common::ext_host_types as types;

/// `typeConverters.EndOfLine`.
pub struct EndOfLine;

impl EndOfLine {
    pub fn from(eol: types::EndOfLine) -> EndOfLineSequence {
        match eol {
            types::EndOfLine::LF => EndOfLineSequence::Lf,
            types::EndOfLine::CRLF => EndOfLineSequence::CrLf,
        }
    }

    pub fn to(eol: EndOfLineSequence) -> types::EndOfLine {
        match eol {
            EndOfLineSequence::Lf => types::EndOfLine::LF,
            EndOfLineSequence::CrLf => types::EndOfLine::CRLF,
        }
    }
}

/// `typeConverters.TextEdit`.
pub struct TextEdit;

impl TextEdit {
    pub fn from(edit: &types::TextEdit) -> EditorTextEdit {
        EditorTextEdit {
            range: Range::from(&edit.range()),
            text: edit.new_text().to_owned(),
            eol: edit.new_eol().map(EndOfLine::from),
            insert_as_snippet: None,
            keep_whitespace: None,
        }
    }

    pub fn to(edit: &EditorTextEdit) -> Result<types::TextEdit, Error> {
        let mut result = types::TextEdit::new(Range::to(&edit.range)?, Some(&edit.text));
        result.set_new_eol(edit.eol.map(EndOfLine::to));
        Ok(result)
    }
}
