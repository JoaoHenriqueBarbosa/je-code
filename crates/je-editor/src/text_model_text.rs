//! Porte de `src/vs/editor/common/model/textModelText.ts`: um `AbstractText` sobre um modelo de texto.
//!
//! Notas de tradução:
//! - `AbstractText` (classe abstrata) é o trait [`AbstractText`]; o `_transformer` da base é a célula de
//!   [`TextModelText::transformer_cache`];
//! - `ITextModel` é o [`TextModel`], guardado em `Rc` (o original guarda a referência ao mesmo objeto).

use std::cell::OnceCell;
use std::rc::Rc;

use je_base::JsString;

use crate::cursor_common::CursorSimpleModel;
use crate::position_to_offset::{PositionOffsetTransformer, TextLength};
use crate::range::Range;
use crate::text::abstract_text::AbstractText;
use crate::text_model::TextModel;

/// `TextModelText`: o texto vivo de um modelo (sem a checagem de versão do `TextModelValueReference`).
pub struct TextModelText {
    text_model: Rc<TextModel>,
    transformer: OnceCell<PositionOffsetTransformer>,
}

impl TextModelText {
    pub fn new(text_model: Rc<TextModel>) -> Self {
        Self { text_model, transformer: OnceCell::new() }
    }
}

impl AbstractText for TextModelText {
    fn get_value_of_range(&self, range: &Range) -> JsString {
        self.text_model.get_value_in_range(range)
    }

    fn get_line_length(&self, line_number: i32) -> i32 {
        self.text_model.get_line_length(line_number) as i32
    }

    fn length(&self) -> TextLength {
        let last_line_number = <TextModel as CursorSimpleModel>::get_line_count(&self.text_model);
        let last_line_len = self.text_model.get_line_length(last_line_number) as i32;
        TextLength::new(last_line_number - 1, last_line_len)
    }

    fn transformer_cache(&self) -> &OnceCell<PositionOffsetTransformer> {
        &self.transformer
    }
}
