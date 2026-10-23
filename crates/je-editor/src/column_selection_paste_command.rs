//! Porte de `src/vs/editor/common/commands/columnSelectionPasteCommand.ts`: colar um texto em bloco, uma
//! linha do texto por linha do documento, alinhado pela coluna visível do começo da seleção.
//!
//! Notas de tradução:
//! - `readonly string[]` é `Vec<JsString>` (UTF-16, como as linhas do modelo).
//! - `new ChosenReplaceCommand(range, lineText).getEditOperations(model, builder)` escolhe entre
//!   [`ReplaceOvertypeCommand`] e [`ReplaceCommand`] e chama o `get_edit_operations` do escolhido.

use crate::cursor_common::CursorSimpleModel;
use je_base::JsString;
use je_base::char_code as cc;

use crate::cursor_columns::{column_from_visible_column, visible_column_from_column};
use crate::editor_common::{Command, CursorStateComputerData, EditOperationBuilder};
use crate::range::Range;
use crate::replace_command::{ReplaceCommand, ReplaceOvertypeCommand};
use crate::selection::Selection;
use crate::text_model::TextModel;

/// `' '.repeat(count)`.
fn spaces(count: i32) -> JsString {
    JsString::from_units(vec![cc::Space; count.max(0) as usize])
}

/// `ColumnSelectionPasteCommand`.
pub struct ColumnSelectionPasteCommand {
    selection: Selection,
    text: Vec<JsString>,
    tab_size: i32,
    overtype: bool,
}

impl ColumnSelectionPasteCommand {
    pub fn new(selection: Selection, text: Vec<JsString>, tab_size: i32, overtype: bool) -> Self {
        Self { selection, text, tab_size, overtype }
    }

    fn pad_text_to_block_width(&self, visible_column: i32) -> Vec<JsString> {
        let prefix = spaces(visible_column);
        let mut end_visible_columns: Vec<i32> = Vec::new();
        let mut block_end_visible_column = visible_column;

        for text in &self.text {
            let mut text_with_prefix = prefix.clone();
            text_with_prefix.push_units(text.units());
            let end_visible_column =
                visible_column_from_column(text_with_prefix.units(), text_with_prefix.len() as i32 + 1, self.tab_size);
            end_visible_columns.push(end_visible_column);
            block_end_visible_column = block_end_visible_column.max(end_visible_column);
        }

        self.text
            .iter()
            .zip(&end_visible_columns)
            .map(|(text, &end_visible_column)| {
                let mut padded = text.clone();
                padded.push_units(spaces(block_end_visible_column - end_visible_column).units());
                padded
            })
            .collect()
    }
}

impl Command for ColumnSelectionPasteCommand {
    fn get_edit_operations(&mut self, model: &TextModel, builder: &mut dyn EditOperationBuilder) {
        let visible_column = visible_column_from_column(
            model.get_line_content(self.selection.start_line_number).units(),
            self.selection.start_column,
            self.tab_size,
        );
        let text = self.pad_text_to_block_width(visible_column);
        let line_count = model.get_line_count();

        for (i, line_text) in text.iter().enumerate() {
            let line_number = self.selection.end_line_number + i as i32;
            if line_number > line_count {
                let end_column = model.get_line_max_column(line_count);
                let padding = spaces(visible_column);
                // `'\n' + padding + text.slice(i).join('\n' + padding)`
                let mut appended_text = JsString::from("\n");
                appended_text.push_units(padding.units());
                for (j, rest) in text[i..].iter().enumerate() {
                    if j > 0 {
                        appended_text.push_unit(cc::LineFeed);
                        appended_text.push_units(padding.units());
                    }
                    appended_text.push_units(rest.units());
                }
                builder.add_tracked_edit_operation(Range::new(line_count, end_column, line_count, end_column), Some(appended_text), false);
                break;
            }

            let range: Range;
            let mut line_text = line_text.clone();
            if i == 0 {
                range = *self.selection;
            } else {
                let line_content = model.get_line_content(line_number);
                let column = column_from_visible_column(line_content.units(), visible_column, self.tab_size);
                range = Range::new(line_number, column, line_number, column);
                if column == model.get_line_max_column(line_number) {
                    let end_visible_column = visible_column_from_column(line_content.units(), column, self.tab_size);
                    let mut padded = spaces(visible_column - end_visible_column);
                    padded.push_units(line_text.units());
                    line_text = padded;
                }
            }
            let mut chosen_replace_command: Box<dyn Command> = if self.overtype {
                Box::new(ReplaceOvertypeCommand::new(range, line_text, Some(false)))
            } else {
                Box::new(ReplaceCommand::new(range, line_text, Some(false)))
            };
            chosen_replace_command.get_edit_operations(model, builder);
        }
    }

    fn compute_cursor_state(&mut self, _model: &TextModel, helper: &dyn CursorStateComputerData) -> Selection {
        let operations = helper.get_inverse_edit_operations();
        let end = operations[operations.len() - 1].range.get_end_position();
        Selection::from_positions(end, end)
    }
}
