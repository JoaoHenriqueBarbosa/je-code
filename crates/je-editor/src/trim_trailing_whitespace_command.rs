//! Porte de `src/vs/editor/common/commands/trimTrailingWhitespaceCommand.ts`: o comando que remove os
//! espaços no fim das linhas (`files.trimTrailingWhitespace`), preservando strings e expressões regulares
//! conforme os tokens da linha.

use crate::cursor_common::CursorSimpleModel;
use crate::edit_operation::{EditOperation, SingleEditOperation};
use crate::editor_common::{Command, CursorStateComputerData, EditOperationBuilder};
use crate::encoded_token_attributes::standard_token_type;
use crate::line_tokens::IViewLineTokens;
use crate::position::Position;
use crate::range::Range;
use crate::selection::Selection;
use crate::text_model::TextModel;
use je_base::strings;

/// `TrimTrailingWhitespaceCommand`.
pub struct TrimTrailingWhitespaceCommand {
    selection: Selection,
    selection_id: Option<String>,
    cursors: Vec<Position>,
    trim_in_regexes_and_strings: bool,
}

impl TrimTrailingWhitespaceCommand {
    /// `new TrimTrailingWhitespaceCommand(selection, cursors, trimInRegexesAndStrings)`.
    pub fn new(selection: Selection, cursors: Vec<Position>, trim_in_regexes_and_strings: bool) -> Self {
        Self { selection, selection_id: None, cursors, trim_in_regexes_and_strings }
    }
}

impl Command for TrimTrailingWhitespaceCommand {
    fn get_edit_operations(&mut self, model: &TextModel, builder: &mut dyn EditOperationBuilder) {
        let ops = trim_trailing_whitespace(model, &mut self.cursors, self.trim_in_regexes_and_strings);
        for op in ops {
            builder.add_edit_operation(op.range, op.text, false);
        }

        self.selection_id = Some(builder.track_selection(self.selection, None));
    }

    fn compute_cursor_state(&mut self, _model: &TextModel, helper: &dyn CursorStateComputerData) -> Selection {
        helper.get_tracked_selection(self.selection_id.as_deref().expect("getEditOperations roda antes de computeCursorState"))
    }
}

/// `trimTrailingWhitespace(model, cursors, trimInRegexesAndStrings)`: gera as edições que removem o espaço no
/// fim das linhas do modelo, ignorando o trecho das linhas em que há cursores. Ordena e reduz `cursors` no
/// lugar, como o original.
pub fn trim_trailing_whitespace(model: &TextModel, cursors: &mut Vec<Position>, trim_in_regexes_and_strings: bool) -> Vec<SingleEditOperation> {
    // Ordena os cursores de forma crescente
    cursors.sort_by(|a, b| a.line_number.cmp(&b.line_number).then(a.column.cmp(&b.column)));

    // Reduz vários cursores na mesma linha e mantém só o último da linha
    for i in (0..cursors.len().saturating_sub(1)).rev() {
        if cursors[i].line_number == cursors[i + 1].line_number {
            // Remove o cursor em `i`
            cursors.remove(i);
        }
    }

    let mut r: Vec<SingleEditOperation> = Vec::new();
    let mut cursor_index = 0;
    let cursor_len = cursors.len();

    for line_number in 1..=model.get_line_count() {
        let line_content = model.get_line_content(line_number);
        let units = line_content.units();
        let max_line_column = units.len() as i32 + 1;
        let mut min_edit_column = 0;

        if cursor_index < cursor_len && cursors[cursor_index].line_number == line_number {
            min_edit_column = cursors[cursor_index].column;
            cursor_index += 1;
            if min_edit_column == max_line_column {
                // O cursor está no fim da linha: com certeza não há edição nesta linha
                continue;
            }
        }

        if units.is_empty() {
            continue;
        }

        let last_non_whitespace_index = strings::last_non_whitespace_index(units, units.len() as i64 - 1);

        let mut from_column = match last_non_whitespace_index {
            // A linha inteira é espaço em branco
            None => 1,
            // Há espaço em branco no fim
            Some(index) if index != units.len() - 1 => index as i32 + 2,
            // Não há espaço em branco no fim
            Some(_) => continue,
        };

        if !trim_in_regexes_and_strings {
            if !model.tokenization().has_accurate_tokens_for_line(line_number) {
                // Não queremos forçar a tokenização da linha, que pode ser cara, mas também não queremos aparar
                // o espaço de linhas ainda não tokenizadas, pois isso pode estar errado e remover espaço de
                // linhas que o usuário pediu para preservar. Então desistimos se os tokens não são precisos.
                continue;
            }

            let line_tokens = model.tokenization().get_line_tokens(line_number);
            let from_column_type = line_tokens.get_standard_token_type(line_tokens.find_token_index_at_offset(from_column));

            if from_column_type == standard_token_type::STRING || from_column_type == standard_token_type::REG_EX {
                continue;
            }
        }

        from_column = from_column.max(min_edit_column);
        r.push(EditOperation::delete(Range::new(line_number, from_column, line_number, max_line_column)));
    }

    r
}
