//! Porte de `src/vs/editor/common/commands/replaceCommand.ts`.
//!
//! Notas de tradução:
//! - `text: string` é [`JsString`] (UTF-16), e `insertsAutoWhitespace: boolean = false` é `Option<bool>`, que
//!   vale `false` quando falta (como o `noop` de `CopyLinesCommand`).
//! - `inverseEditOperations[0]` entra por um helper só, [`first_inverse_range`], porque os seis comandos que
//!   calculam o cursor a partir da primeira operação inversa fazem o mesmo acesso.
//! - `throw new Error(...)` vira [`je_base::errors::throw`], o pânico que o resto do porte usa.

use je_base::JsString;
use je_base::errors::{self, Error};

use crate::editor_common::{Command, CursorStateComputerData, EditOperationBuilder};
use crate::position::Position;
use crate::range::Range;
use crate::selection::{Selection, SelectionDirection};
use crate::text_model::TextModel;

/// `Selection.fromPositions(position)`: a seleção vazia na posição.
fn selection_at(position: Position) -> Selection {
    Selection::from_positions(position, position)
}

/// `inverseEditOperations[0].range`.
fn first_inverse_range(helper: &dyn CursorStateComputerData) -> Range {
    helper.get_inverse_edit_operations()[0].range
}

/// `ReplaceCommand`.
pub struct ReplaceCommand {
    range: Range,
    text: JsString,
    inserts_auto_whitespace: bool,
}

impl ReplaceCommand {
    pub fn new(range: Range, text: JsString, inserts_auto_whitespace: Option<bool>) -> Self {
        Self { range, text, inserts_auto_whitespace: inserts_auto_whitespace.unwrap_or(false) }
    }
}

impl Command for ReplaceCommand {
    fn inserts_auto_whitespace(&self) -> bool {
        self.inserts_auto_whitespace
    }

    fn get_edit_operations(&mut self, _model: &TextModel, builder: &mut dyn EditOperationBuilder) {
        builder.add_tracked_edit_operation(self.range, Some(self.text.clone()), false);
    }

    fn compute_cursor_state(&mut self, _model: &TextModel, helper: &dyn CursorStateComputerData) -> Selection {
        selection_at(first_inverse_range(helper).get_end_position())
    }
}

/// `ReplaceOvertypeCommand`.
pub struct ReplaceOvertypeCommand {
    range: Range,
    text: JsString,
    inserts_auto_whitespace: bool,
}

impl ReplaceOvertypeCommand {
    pub fn new(range: Range, text: JsString, inserts_auto_whitespace: Option<bool>) -> Self {
        Self { range, text, inserts_auto_whitespace: inserts_auto_whitespace.unwrap_or(false) }
    }
}

impl Command for ReplaceOvertypeCommand {
    fn inserts_auto_whitespace(&self) -> bool {
        self.inserts_auto_whitespace
    }

    fn get_edit_operations(&mut self, model: &TextModel, builder: &mut dyn EditOperationBuilder) {
        let initial_start_position = self.range.get_start_position();
        let initial_end_position = self.range.get_end_position();
        let offset_delta = self.text.len() as i32 + if self.range.is_empty() { 0 } else { -1 };
        let end_position = overtype_end_position(model, initial_end_position, offset_delta);
        let replace_range = Range::from_positions(initial_start_position, end_position);
        builder.add_tracked_edit_operation(replace_range, Some(self.text.clone()), false);
    }

    fn compute_cursor_state(&mut self, _model: &TextModel, helper: &dyn CursorStateComputerData) -> Selection {
        selection_at(first_inverse_range(helper).get_end_position())
    }
}

/// `ReplaceCommandThatSelectsText`.
pub struct ReplaceCommandThatSelectsText {
    range: Range,
    text: JsString,
}

impl ReplaceCommandThatSelectsText {
    pub fn new(range: Range, text: JsString) -> Self {
        Self { range, text }
    }
}

impl Command for ReplaceCommandThatSelectsText {
    fn get_edit_operations(&mut self, _model: &TextModel, builder: &mut dyn EditOperationBuilder) {
        builder.add_tracked_edit_operation(self.range, Some(self.text.clone()), false);
    }

    fn compute_cursor_state(&mut self, _model: &TextModel, helper: &dyn CursorStateComputerData) -> Selection {
        Selection::from_range(&first_inverse_range(helper), SelectionDirection::Ltr)
    }
}

/// `ReplaceCommandWithoutChangingPosition`.
pub struct ReplaceCommandWithoutChangingPosition {
    range: Range,
    text: JsString,
    inserts_auto_whitespace: bool,
}

impl ReplaceCommandWithoutChangingPosition {
    pub fn new(range: Range, text: JsString, inserts_auto_whitespace: Option<bool>) -> Self {
        Self { range, text, inserts_auto_whitespace: inserts_auto_whitespace.unwrap_or(false) }
    }
}

impl Command for ReplaceCommandWithoutChangingPosition {
    fn inserts_auto_whitespace(&self) -> bool {
        self.inserts_auto_whitespace
    }

    fn get_edit_operations(&mut self, _model: &TextModel, builder: &mut dyn EditOperationBuilder) {
        builder.add_tracked_edit_operation(self.range, Some(self.text.clone()), false);
    }

    fn compute_cursor_state(&mut self, _model: &TextModel, helper: &dyn CursorStateComputerData) -> Selection {
        selection_at(first_inverse_range(helper).get_start_position())
    }
}

/// `ReplaceCommandWithOffsetCursorState`.
pub struct ReplaceCommandWithOffsetCursorState {
    range: Range,
    text: JsString,
    column_delta_offset: i32,
    line_number_delta_offset: i32,
    inserts_auto_whitespace: bool,
}

impl ReplaceCommandWithOffsetCursorState {
    pub fn new(
        range: Range,
        text: JsString,
        line_number_delta_offset: i32,
        column_delta_offset: i32,
        inserts_auto_whitespace: Option<bool>,
    ) -> Self {
        Self {
            range,
            text,
            column_delta_offset,
            line_number_delta_offset,
            inserts_auto_whitespace: inserts_auto_whitespace.unwrap_or(false),
        }
    }
}

impl Command for ReplaceCommandWithOffsetCursorState {
    fn inserts_auto_whitespace(&self) -> bool {
        self.inserts_auto_whitespace
    }

    fn get_edit_operations(&mut self, _model: &TextModel, builder: &mut dyn EditOperationBuilder) {
        builder.add_tracked_edit_operation(self.range, Some(self.text.clone()), false);
    }

    fn compute_cursor_state(&mut self, _model: &TextModel, helper: &dyn CursorStateComputerData) -> Selection {
        selection_at(
            first_inverse_range(helper).get_end_position().delta(self.line_number_delta_offset, self.column_delta_offset),
        )
    }
}

/// `ReplaceOvertypeCommandOnCompositionEnd`.
pub struct ReplaceOvertypeCommandOnCompositionEnd {
    range: Range,
}

impl ReplaceOvertypeCommandOnCompositionEnd {
    pub fn new(range: Range) -> Self {
        Self { range }
    }
}

impl Command for ReplaceOvertypeCommandOnCompositionEnd {
    fn get_edit_operations(&mut self, model: &TextModel, builder: &mut dyn EditOperationBuilder) {
        let text = model.get_value_in_range(&self.range);
        let initial_end_position = self.range.get_end_position();
        let end_position = overtype_end_position(model, initial_end_position, text.len() as i32);
        let replace_range = Range::from_positions(initial_end_position, end_position);
        builder.add_tracked_edit_operation(replace_range, Some(JsString::new()), false);
    }

    fn compute_cursor_state(&mut self, _model: &TextModel, helper: &dyn CursorStateComputerData) -> Selection {
        selection_at(first_inverse_range(helper).get_end_position())
    }
}

/// `ReplaceCommandThatPreservesSelection`.
pub struct ReplaceCommandThatPreservesSelection {
    range: Range,
    text: JsString,
    initial_selection: Selection,
    force_move_markers: bool,
    selection_id: Option<String>,
}

impl ReplaceCommandThatPreservesSelection {
    /// `forceMoveMarkers: boolean = false` é `Option<bool>`.
    pub fn new(edit_range: Range, text: JsString, initial_selection: Selection, force_move_markers: Option<bool>) -> Self {
        Self {
            range: edit_range,
            text,
            initial_selection,
            force_move_markers: force_move_markers.unwrap_or(false),
            selection_id: None,
        }
    }
}

impl Command for ReplaceCommandThatPreservesSelection {
    fn get_edit_operations(&mut self, _model: &TextModel, builder: &mut dyn EditOperationBuilder) {
        builder.add_tracked_edit_operation(self.range, Some(self.text.clone()), self.force_move_markers);
        self.selection_id = Some(builder.track_selection(self.initial_selection, None));
    }

    fn compute_cursor_state(&mut self, _model: &TextModel, helper: &dyn CursorStateComputerData) -> Selection {
        helper.get_tracked_selection(self.selection_id.as_deref().expect("getEditOperations roda antes de computeCursorState"))
    }
}

/// O fim de uma edição em modo de sobrescrita: `initial_end_position` mais `offset_delta` unidades, ou o
/// fim da linha se o deslocamento passa dela (os dois comandos de sobrescrita usam o mesmo cálculo).
fn overtype_end_position(model: &TextModel, initial_end_position: Position, offset_delta: i32) -> Position {
    let initial_end_line_number = initial_end_position.line_number;
    let end_position = add_positive_offset_to_model_position(model, initial_end_position, offset_delta);
    if end_position.line_number > initial_end_line_number {
        return Position::new(initial_end_line_number, model.get_line_max_column(initial_end_line_number));
    }
    end_position
}

/// `addPositiveOffsetToModelPosition`.
fn add_positive_offset_to_model_position(model: &TextModel, position: Position, offset: i32) -> Position {
    if offset < 0 {
        errors::throw(Error::new("Unexpected negative delta"));
    }
    let mut offset = offset;
    let line_count = model.get_line_count();
    let mut end_position = Position::new(line_count, model.get_line_max_column(line_count));
    for line_number in position.line_number..=line_count {
        if line_number == position.line_number {
            let future_offset = offset - model.get_line_max_column(position.line_number) + position.column;
            if future_offset <= 0 {
                end_position = Position::new(position.line_number, position.column + offset);
                break;
            }
            offset = future_offset;
        } else {
            let future_offset = offset - model.get_line_max_column(line_number);
            if future_offset <= 0 {
                end_position = Position::new(line_number, offset);
                break;
            }
            offset = future_offset;
        }
    }
    end_position
}
