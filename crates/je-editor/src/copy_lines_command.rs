//! Porte de `src/vs/editor/contrib/linesOperations/browser/copyLinesCommand.ts`.

use je_base::JsString;

use crate::editor_common::{Command, CursorStateComputerData, EditOperationBuilder};
use crate::range::Range;
use crate::selection::{Selection, SelectionDirection};
use crate::text_model::TextModel;

/// `CopyLinesCommand`.
pub struct CopyLinesCommand {
    selection: Selection,
    is_copying_down: bool,
    noop: bool,

    selection_direction: SelectionDirection,
    selection_id: Option<String>,
    start_line_number_delta: i32,
    end_line_number_delta: i32,
}

impl CopyLinesCommand {
    /// `new CopyLinesCommand(selection, isCopyingDown, noop)`: `noop` é opcional no original e vale `false`
    /// quando falta.
    pub fn new(selection: Selection, is_copying_down: bool, noop: Option<bool>) -> Self {
        Self {
            selection,
            is_copying_down,
            noop: noop.unwrap_or(false),
            selection_direction: SelectionDirection::Ltr,
            selection_id: None,
            start_line_number_delta: 0,
            end_line_number_delta: 0,
        }
    }
}

impl Command for CopyLinesCommand {
    fn get_edit_operations(&mut self, model: &TextModel, builder: &mut dyn EditOperationBuilder) {
        let mut s = self.selection;

        self.start_line_number_delta = 0;
        self.end_line_number_delta = 0;
        if s.start_line_number < s.end_line_number && s.end_column == 1 {
            self.end_line_number_delta = 1;
            s = s.set_end_position(s.end_line_number - 1, model.get_line_max_column(s.end_line_number - 1));
        }

        // `sourceLines.join('\n')`
        let mut source_text = JsString::new();
        for (i, line_number) in (s.start_line_number..=s.end_line_number).enumerate() {
            if i > 0 {
                source_text.push_unit(10);
            }
            source_text.push_units(model.get_line_content(line_number).units());
        }

        if source_text.is_empty() {
            // Duplicando uma linha vazia
            if self.is_copying_down {
                self.start_line_number_delta += 1;
                self.end_line_number_delta += 1;
            }
        }

        if self.noop {
            let text = if s.end_line_number == model.get_line_count() { JsString::new() } else { JsString::from("\n") };
            builder.add_edit_operation(
                Range::new(s.end_line_number, model.get_line_max_column(s.end_line_number), s.end_line_number + 1, 1),
                Some(text),
                false,
            );
        } else if !self.is_copying_down {
            // `'\n' + sourceText`
            let mut text = JsString::from("\n");
            text.push_units(source_text.units());
            let end_column = model.get_line_max_column(s.end_line_number);
            builder.add_edit_operation(Range::new(s.end_line_number, end_column, s.end_line_number, end_column), Some(text), false);
        } else {
            // `sourceText + '\n'`
            let mut text = source_text;
            text.push_unit(10);
            builder.add_edit_operation(Range::new(s.start_line_number, 1, s.start_line_number, 1), Some(text), false);
        }

        self.selection_id = Some(builder.track_selection(s, None));
        self.selection_direction = self.selection.get_direction();
    }

    fn compute_cursor_state(&mut self, _model: &TextModel, helper: &dyn CursorStateComputerData) -> Selection {
        let mut result = helper
            .get_tracked_selection(self.selection_id.as_deref().expect("getEditOperations roda antes de computeCursorState"));

        if self.start_line_number_delta != 0 || self.end_line_number_delta != 0 {
            let mut start_line_number = result.start_line_number;
            let mut start_column = result.start_column;
            let mut end_line_number = result.end_line_number;
            let mut end_column = result.end_column;

            if self.start_line_number_delta != 0 {
                start_line_number += self.start_line_number_delta;
                start_column = 1;
            }

            if self.end_line_number_delta != 0 {
                end_line_number += self.end_line_number_delta;
                end_column = 1;
            }

            result = Selection::create_with_direction(
                start_line_number,
                start_column,
                end_line_number,
                end_column,
                self.selection_direction,
            );
        }

        result
    }
}
