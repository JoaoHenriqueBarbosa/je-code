//! Porte de `src/vs/editor/contrib/inPlaceReplace/browser/inPlaceReplaceCommand.ts`: o comando que troca o texto sob o
//! cursor pelo valor seguinte ou anterior e reposiciona a seleção.

use je_base::JsString;

use crate::editor_common::{Command, CursorStateComputerData, EditOperationBuilder};
use crate::range::Range;
use crate::selection::Selection;
use crate::text_model::TextModel;

/// `InPlaceReplaceCommand`.
pub struct InPlaceReplaceCommand {
    edit_range: Range,
    original_selection: Selection,
    text: JsString,
}

impl InPlaceReplaceCommand {
    pub fn new(edit_range: Range, original_selection: Selection, text: JsString) -> Self {
        Self { edit_range, original_selection, text }
    }
}

impl Command for InPlaceReplaceCommand {
    fn get_edit_operations(&mut self, _model: &TextModel, builder: &mut dyn EditOperationBuilder) {
        builder.add_tracked_edit_operation(self.edit_range, Some(self.text.clone()), false);
    }

    fn compute_cursor_state(&mut self, _model: &TextModel, helper: &dyn CursorStateComputerData) -> Selection {
        let inverse_edit_operations = helper.get_inverse_edit_operations();
        let src_range = inverse_edit_operations[0].range;

        if !self.original_selection.is_empty() {
            // Preserve selection and extends to typed text
            return Selection::new(
                src_range.end_line_number,
                src_range.end_column - self.text.len() as i32,
                src_range.end_line_number,
                src_range.end_column,
            );
        }

        let column = self.original_selection.position_column.min(src_range.end_column);
        Selection::new(src_range.end_line_number, column, src_range.end_line_number, column)
    }
}
