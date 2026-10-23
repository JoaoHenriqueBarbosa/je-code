//! Porte de `src/vs/editor/common/coordinatesConverter.ts`: `ICoordinatesConverter` e
//! `IdentityCoordinatesConverter`.
//!
//! Representação: os parâmetros opcionais do original (`affinity?`, `allowZeroLineNumber?`,
//! `belowHiddenRanges?`) são `Option`. O `ITextModel` do conversor é o `Rc<TextModel>`.

use std::rc::Rc;

use crate::model::PositionAffinity;
use crate::position::Position;
use crate::range::Range;
use crate::text_model::TextModel;

pub trait ICoordinatesConverter {
    // View -> Model conversion and related methods
    fn convert_view_position_to_model_position(&self, view_position: Position) -> Position;
    fn convert_view_range_to_model_range(&self, view_range: Range) -> Range;
    fn validate_view_position(&self, view_position: Position, expected_model_position: Position) -> Position;
    fn validate_view_range(&self, view_range: Range, expected_model_range: Range) -> Range;

    // Model -> View conversion and related methods

    /// `allow_zero_line_number`: devolve 0 quando há linhas escondidas no topo e a posição está na área escondida?
    /// `below_hidden_ranges`: quando a posição do modelo está numa área escondida, devolve a primeira posição
    /// da visão depois ou antes dela?
    fn convert_model_position_to_view_position(
        &self,
        model_position: Position,
        affinity: Option<PositionAffinity>,
        allow_zero_line_number: Option<bool>,
        below_hidden_ranges: Option<bool>,
    ) -> Position;
    /// `affinity`: só tem efeito se o intervalo é vazio.
    fn convert_model_range_to_view_range(&self, model_range: Range, affinity: Option<PositionAffinity>) -> Range;
    fn model_position_is_visible(&self, model_position: Position) -> bool;
    fn get_model_line_view_line_count(&self, model_line_number: i32) -> i32;
    fn get_view_line_number_of_model_position(&self, model_line_number: i32, model_column: i32) -> i32;
}

pub struct IdentityCoordinatesConverter {
    model: Rc<TextModel>,
}

impl IdentityCoordinatesConverter {
    pub fn new(model: Rc<TextModel>) -> Self {
        Self { model }
    }

    fn valid_position(&self, pos: Position) -> Position {
        self.model.validate_position(pos)
    }

    fn valid_range(&self, range: Range) -> Range {
        self.model.validate_range(&range)
    }

    /// `modelRangeIsVisible(modelRange)`: não faz parte da interface.
    pub fn model_range_is_visible(&self, model_range: Range) -> bool {
        let line_count = self.model.get_line_count();
        if model_range.start_line_number < 1 || model_range.start_line_number > line_count {
            // invalid arguments
            return false;
        }
        if model_range.end_line_number < 1 || model_range.end_line_number > line_count {
            // invalid arguments
            return false;
        }
        true
    }
}

impl ICoordinatesConverter for IdentityCoordinatesConverter {
    // View -> Model conversion and related methods

    fn convert_view_position_to_model_position(&self, view_position: Position) -> Position {
        self.valid_position(view_position)
    }

    fn convert_view_range_to_model_range(&self, view_range: Range) -> Range {
        self.valid_range(view_range)
    }

    fn validate_view_position(&self, _view_position: Position, expected_model_position: Position) -> Position {
        self.valid_position(expected_model_position)
    }

    fn validate_view_range(&self, _view_range: Range, expected_model_range: Range) -> Range {
        self.valid_range(expected_model_range)
    }

    // Model -> View conversion and related methods

    fn convert_model_position_to_view_position(
        &self,
        model_position: Position,
        _affinity: Option<PositionAffinity>,
        _allow_zero_line_number: Option<bool>,
        _below_hidden_ranges: Option<bool>,
    ) -> Position {
        self.valid_position(model_position)
    }

    fn convert_model_range_to_view_range(&self, model_range: Range, _affinity: Option<PositionAffinity>) -> Range {
        self.valid_range(model_range)
    }

    fn model_position_is_visible(&self, model_position: Position) -> bool {
        let line_count = self.model.get_line_count();
        if model_position.line_number < 1 || model_position.line_number > line_count {
            // invalid arguments
            return false;
        }
        true
    }

    fn get_model_line_view_line_count(&self, _model_line_number: i32) -> i32 {
        1
    }

    fn get_view_line_number_of_model_position(&self, model_line_number: i32, _model_column: i32) -> i32 {
        model_line_number
    }
}
