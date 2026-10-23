//! Porte de `src/vs/editor/common/commands/surroundSelectionCommand.ts`: envolver a seleção com um par de
//! textos.
//!
//! Notas de tradução:
//! - Os textos são [`JsString`] (UTF-16): `computeCursorState` subtrai o `length` do texto de depois, que no
//!   original conta unidades UTF-16.
//! - `this._charAfterSelection || null`: o texto vazio vira `None`, porque o construtor de operações ignora
//!   a edição de texto vazio e isso quebraria o `computeCursorState` em cadeia (comentário do original).

use je_base::JsString;

use crate::editor_common::{Command, CursorStateComputerData, EditOperationBuilder};
use crate::position::Position;
use crate::range::Range;
use crate::selection::Selection;
use crate::text_model::TextModel;

/// `SurroundSelectionCommand`.
pub struct SurroundSelectionCommand {
    range: Selection,
    char_before_selection: JsString,
    char_after_selection: JsString,
}

impl SurroundSelectionCommand {
    pub fn new(range: Selection, char_before_selection: impl Into<JsString>, char_after_selection: impl Into<JsString>) -> Self {
        Self { range, char_before_selection: char_before_selection.into(), char_after_selection: char_after_selection.into() }
    }
}

impl Command for SurroundSelectionCommand {
    fn get_edit_operations(&mut self, _model: &TextModel, builder: &mut dyn EditOperationBuilder) {
        builder.add_tracked_edit_operation(
            Range::new(self.range.start_line_number, self.range.start_column, self.range.start_line_number, self.range.start_column),
            Some(self.char_before_selection.clone()),
            false,
        );

        // `addTrackedEditOperation()` ignora o texto vazio, o que causaria uma cadeia de erros em
        // `computeCursorState()`.
        let char_after_selection = (!self.char_after_selection.is_empty()).then(|| self.char_after_selection.clone());
        builder.add_tracked_edit_operation(
            Range::new(self.range.end_line_number, self.range.end_column, self.range.end_line_number, self.range.end_column),
            char_after_selection,
            false,
        );
    }

    fn compute_cursor_state(&mut self, _model: &TextModel, helper: &dyn CursorStateComputerData) -> Selection {
        let inverse_edit_operations = helper.get_inverse_edit_operations();
        let first_operation_range = inverse_edit_operations[0].range;
        let second_operation_range = inverse_edit_operations[1].range;

        Selection::new(
            first_operation_range.end_line_number,
            first_operation_range.end_column,
            second_operation_range.end_line_number,
            second_operation_range.end_column - self.char_after_selection.len() as i32,
        )
    }
}

/// `CompositionSurroundSelectionCommand`: um comando de envolver a seleção que roda depois que a composição
/// terminou.
pub struct CompositionSurroundSelectionCommand {
    position: Position,
    text: JsString,
    char_after: JsString,
}

impl CompositionSurroundSelectionCommand {
    pub fn new(position: Position, text: impl Into<JsString>, char_after: impl Into<JsString>) -> Self {
        Self { position, text: text.into(), char_after: char_after.into() }
    }
}

impl Command for CompositionSurroundSelectionCommand {
    fn get_edit_operations(&mut self, _model: &TextModel, builder: &mut dyn EditOperationBuilder) {
        // `this._text + this._charAfter`
        let mut text = self.text.clone();
        text.push_units(self.char_after.units());
        builder.add_tracked_edit_operation(
            Range::new(self.position.line_number, self.position.column, self.position.line_number, self.position.column),
            Some(text),
            false,
        );
    }

    fn compute_cursor_state(&mut self, _model: &TextModel, helper: &dyn CursorStateComputerData) -> Selection {
        let inverse_edit_operations = helper.get_inverse_edit_operations();
        let op_range = inverse_edit_operations[0].range;

        Selection::new(
            op_range.end_line_number,
            op_range.start_column,
            op_range.end_line_number,
            op_range.end_column - self.char_after.len() as i32,
        )
    }
}
