//! Porte de `src/vs/editor/contrib/find/browser/replaceAllCommand.ts`: o comando que troca todas as ocorrências da
//! busca de uma vez (`ReplaceAllCommand`).
//!
//! Representação: `IEditOperation { range, text }` é a struct privada [`EditOperation`], e `string` é [`JsString`].
//! O id da seleção rastreada (`_trackedEditorSelectionId`) fica num `Option<String>`, preenchido por
//! `get_edit_operations` e lido por `compute_cursor_state`.

use je_base::JsString;

use crate::editor_common::{Command, CursorStateComputerData, EditOperationBuilder};
use crate::range::Range;
use crate::selection::Selection;
use crate::text_model::TextModel;

/// `IEditOperation`.
struct EditOperation {
    range: Range,
    text: JsString,
}

/// `ReplaceAllCommand`.
pub struct ReplaceAllCommand {
    editor_selection: Selection,
    tracked_editor_selection_id: Option<String>,
    ranges: Vec<Range>,
    replace_strings: Vec<JsString>,
}

impl ReplaceAllCommand {
    pub fn new(editor_selection: Selection, ranges: Vec<Range>, replace_strings: Vec<JsString>) -> Self {
        Self { editor_selection, tracked_editor_selection_id: None, ranges, replace_strings }
    }
}

impl Command for ReplaceAllCommand {
    fn get_edit_operations(&mut self, _model: &TextModel, builder: &mut dyn EditOperationBuilder) {
        if !self.ranges.is_empty() {
            // Collect all edit operations
            let mut ops: Vec<EditOperation> = self
                .ranges
                .iter()
                .zip(&self.replace_strings)
                .map(|(range, text)| EditOperation { range: *range, text: text.clone() })
                .collect();

            // Sort them in ascending order by range starts
            ops.sort_by(|o1, o2| Range::compare_ranges_using_starts(Some(&o1.range), Some(&o2.range)).cmp(&0));

            // Merge operations that touch each other
            let mut result_ops: Vec<EditOperation> = Vec::new();
            let mut ops = ops.into_iter();
            let mut previous_op = ops.next().expect("há ao menos uma operação");
            for op in ops {
                if previous_op.range.end_line_number == op.range.start_line_number && previous_op.range.end_column == op.range.start_column {
                    // These operations are one after another and can be merged
                    previous_op.range = previous_op.range.plus_range(&op.range);
                    previous_op.text.push_units(op.text.units());
                } else {
                    result_ops.push(previous_op);
                    previous_op = op;
                }
            }
            result_ops.push(previous_op);

            for op in result_ops {
                builder.add_edit_operation(op.range, Some(op.text), false);
            }
        }

        self.tracked_editor_selection_id = Some(builder.track_selection(self.editor_selection, None));
    }

    fn compute_cursor_state(&mut self, _model: &TextModel, helper: &dyn CursorStateComputerData) -> Selection {
        helper.get_tracked_selection(
            self.tracked_editor_selection_id.as_deref().expect("getEditOperations roda antes de computeCursorState"),
        )
    }
}
