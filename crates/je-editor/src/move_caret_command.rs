//! Porte de `src/vs/editor/contrib/caretOperations/browser/moveCaretCommand.ts`.

use crate::editor_common::{Command, CursorStateComputerData, EditOperationBuilder};
use crate::range::Range;
use crate::selection::Selection;
use crate::text_model::TextModel;

/// `MoveCaretCommand`.
pub struct MoveCaretCommand {
    selection: Selection,
    is_moving_left: bool,
}

impl MoveCaretCommand {
    pub fn new(selection: Selection, is_moving_left: bool) -> Self {
        Self { selection, is_moving_left }
    }
}

impl Command for MoveCaretCommand {
    fn get_edit_operations(&mut self, model: &TextModel, builder: &mut dyn EditOperationBuilder) {
        if self.selection.start_line_number != self.selection.end_line_number || self.selection.is_empty() {
            return;
        }
        let line_number = self.selection.start_line_number;
        let start_column = self.selection.start_column;
        let end_column = self.selection.end_column;
        if self.is_moving_left && start_column == 1 {
            return;
        }
        if !self.is_moving_left && end_column == model.get_line_max_column(line_number) {
            return;
        }

        if self.is_moving_left {
            let range_before = Range::new(line_number, start_column - 1, line_number, start_column);
            let char_before = model.get_value_in_range(&range_before);
            builder.add_edit_operation(range_before, None, false);
            builder.add_edit_operation(Range::new(line_number, end_column, line_number, end_column), Some(char_before), false);
        } else {
            let range_after = Range::new(line_number, end_column, line_number, end_column + 1);
            let char_after = model.get_value_in_range(&range_after);
            builder.add_edit_operation(range_after, None, false);
            builder.add_edit_operation(Range::new(line_number, start_column, line_number, start_column), Some(char_after), false);
        }
    }

    fn compute_cursor_state(&mut self, _model: &TextModel, _helper: &dyn CursorStateComputerData) -> Selection {
        let s = &self.selection;
        if self.is_moving_left {
            Selection::new(s.start_line_number, s.start_column - 1, s.end_line_number, s.end_column - 1)
        } else {
            Selection::new(s.start_line_number, s.start_column + 1, s.end_line_number, s.end_column + 1)
        }
    }
}
