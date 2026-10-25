//! Porte de `src/vs/editor/common/core/text/getPositionOffsetTransformerFromTextModel.ts`: o
//! `PositionOffsetTransformerBase` que delega ao modelo de texto (sem copiar o texto, ao contrário do
//! `PositionOffsetTransformer`).

use std::rc::Rc;

use crate::position::Position;
use crate::position_to_offset::PositionOffsetTransformerBase;
use crate::text_model::TextModel;

/// `PositionOffsetTransformerWithTextModel`.
pub struct PositionOffsetTransformerWithTextModel {
    text_model: Rc<TextModel>,
}

/// `getPositionOffsetTransformerFromTextModel(textModel)`.
pub fn get_position_offset_transformer_from_text_model(text_model: &Rc<TextModel>) -> PositionOffsetTransformerWithTextModel {
    PositionOffsetTransformerWithTextModel { text_model: Rc::clone(text_model) }
}

impl PositionOffsetTransformerBase for PositionOffsetTransformerWithTextModel {
    fn get_offset(&self, position: Position) -> i32 {
        self.text_model.get_offset_at(position) as i32
    }

    fn get_position(&self, offset: i32) -> Position {
        self.text_model.get_position_at(i64::from(offset))
    }
}
