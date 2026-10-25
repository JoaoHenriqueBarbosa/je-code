//! Porte de `src/vs/editor/contrib/inlineCompletions/browser/model/textModelValueReference.ts`: uma visão imutável
//! de um modelo de texto numa versão específica.
//!
//! Notas de tradução:
//! - `AbstractText` (classe abstrata) é o trait [`AbstractText`]; o `_transformer` da base é a célula de
//!   [`TextModelValueReference::transformer_cache`];
//! - `ITextModel` é o [`TextModel`], guardado em `Rc` (o original guarda a referência ao mesmo objeto);
//! - o `TODO: throw here!` do `_assertValid` é do original: ele só reporta o erro e segue lendo o modelo;
//! - `getEOL` devolve o [`Eol`] do modelo e `getPositionAt` recebe o deslocamento como `i64`, como o `TextModel`.

use std::cell::OnceCell;
use std::rc::Rc;

use je_base::JsString;
use je_base::errors::{self, Error};
use je_base::uri::URI;

use crate::cursor_common::CursorSimpleModel;
use crate::model::Eol;
use crate::position::Position;
use crate::position_to_offset::{PositionOffsetTransformer, TextLength};
use crate::range::Range;
use crate::text::abstract_text::AbstractText;
use crate::text_model::TextModel;

/// `TextModelValueReference`: como o `TextModelText`, mas reporta erro se o modelo mudou depois da criação.
/// Isso garante que o dado lido é consistente com a versão do momento da construção.
pub struct TextModelValueReference {
    version: i64,
    text_model: Rc<TextModel>,
    transformer: OnceCell<PositionOffsetTransformer>,
}

impl TextModelValueReference {
    /// `snapshot(textModel)`.
    pub fn snapshot(text_model: &Rc<TextModel>) -> Rc<TextModelValueReference> {
        Rc::new(Self::new(Rc::clone(text_model)))
    }

    fn new(text_model: Rc<TextModel>) -> Self {
        let version = text_model.get_version_id();
        Self { version, text_model, transformer: OnceCell::new() }
    }

    /// `uri`.
    pub fn uri(&self) -> URI {
        self.text_model.uri()
    }

    /// `version`.
    pub fn version(&self) -> i64 {
        self.version
    }

    fn assert_valid(&self) {
        let current = self.text_model.get_version_id();
        if current != self.version {
            errors::on_unexpected_error(&Error::new(format!(
                "TextModel has changed: expected version {}, got {}",
                self.version, current
            )));
            // TODO: throw here! (do original)
        }
    }

    /// `targets(textModel)`: o modelo alvo é o mesmo recurso.
    pub fn targets(&self, text_model: &TextModel) -> bool {
        self.text_model.uri().to_string(false) == text_model.uri().to_string(false)
    }

    /// `getEOL()`.
    pub fn get_eol(&self) -> Eol {
        self.assert_valid();
        self.text_model.get_eol()
    }

    /// `getPositionAt(offset)`.
    pub fn get_position_at(&self, offset: i64) -> Position {
        self.assert_valid();
        self.text_model.get_position_at(offset)
    }

    /// `getValueInRange(range)`.
    pub fn get_value_in_range(&self, range: &Range) -> JsString {
        self.assert_valid();
        self.text_model.get_value_in_range(range)
    }

    /// `getVersionId()`.
    pub fn get_version_id(&self) -> i64 {
        self.version
    }

    /// `dangerouslyGetUnderlyingModel()`.
    pub fn dangerously_get_underlying_model(&self) -> &Rc<TextModel> {
        &self.text_model
    }
}

impl AbstractText for TextModelValueReference {
    fn get_value_of_range(&self, range: &Range) -> JsString {
        self.assert_valid();
        self.text_model.get_value_in_range(range)
    }

    fn get_line_length(&self, line_number: i32) -> i32 {
        self.assert_valid();
        self.text_model.get_line_length(line_number) as i32
    }

    fn length(&self) -> TextLength {
        self.assert_valid();
        let last_line_number = <TextModel as CursorSimpleModel>::get_line_count(&self.text_model);
        let last_line_len = self.text_model.get_line_length(last_line_number) as i32;
        TextLength::new(last_line_number - 1, last_line_len)
    }

    fn transformer_cache(&self) -> &OnceCell<PositionOffsetTransformer> {
        &self.transformer
    }
}
