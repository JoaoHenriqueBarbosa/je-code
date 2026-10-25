//! Porte parcial de `src/vs/editor/common/core/edits/lineEdit.ts`: o `LineEdit` e o `LineReplacement` com o que o
//! `EditDeltaInfo.fromEdit` do `textModelEditSource` lê (`fromStringEdit`, `fromTextEdit`, `fromSingleTextEdit`, as
//! `replacements` com `lineRange` e `newLines`), mais `getNewLineRanges` e `toLineEdit`, que o `InlineEditWithChanges`
//! lê.
//!
//! As demais operações (`toEdit`, `mapLineNumber`, `rebase`, `humanReadablePatch`, `inverse`,
//! `removeCommonSuffixPrefixLines`, `toSingleTextEdit`, a serialização) entram junto de quem as usar.

use je_base::JsString;
use je_base::assert::{assert_fn, check_adjacent_items};
use je_base::strings::split_lines;

use super::string_edit::{BaseStringEdit, BaseStringReplacement};
use super::text_edit::{TextEdit, TextReplacement};
use crate::line_range::LineRange;
use crate::position::Position;
use crate::range::Range;
use crate::text::abstract_text::AbstractText;

/// `a + b` de strings.
fn concat(a: &JsString, b: &JsString) -> JsString {
    let mut result = a.clone();
    result.push_units(b.units());
    result
}

/// `LineEdit`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LineEdit {
    /// Have to be sorted by start line number and non-intersecting.
    pub replacements: Vec<LineReplacement>,
}

impl LineEdit {
    /// `LineEdit.fromStringEdit(edit, initialValue)`.
    pub fn from_string_edit<E: BaseStringEdit>(edit: &E, initial_value: &dyn AbstractText) -> LineEdit
    where
        E::Replacement: BaseStringReplacement,
    {
        let text_edit = TextEdit::from_string_edit(edit, initial_value);
        LineEdit::from_text_edit(&text_edit, initial_value)
    }

    /// `LineEdit.fromTextEdit(edit, initialValue)`.
    pub fn from_text_edit(edit: &TextEdit, initial_value: &dyn AbstractText) -> LineEdit {
        let edits = &edit.replacements;

        let mut result: Vec<LineReplacement> = Vec::new();

        let mut current_edits: Vec<TextReplacement> = Vec::new();
        for (i, edit) in edits.iter().enumerate() {
            let next_edit_range = edits.get(i + 1);
            current_edits.push(edit.clone());
            if let Some(next) = next_edit_range
                && next.range.start_line_number == edit.range.end_line_number
            {
                continue;
            }

            let single_edit = TextReplacement::join_replacements(&current_edits, initial_value);
            current_edits.clear();

            let single_line_edit = LineReplacement::from_single_text_edit(&single_edit, initial_value);
            result.push(single_line_edit);
        }

        LineEdit::new(result)
    }

    /// O construtor confere (como `assert`) que as substituições estão ordenadas e não se sobrepõem.
    pub fn new(replacements: Vec<LineReplacement>) -> Self {
        assert_fn(|| check_adjacent_items(&replacements, |i1, i2| i1.line_range.end_line_number_exclusive <= i2.line_range.start_line_number));
        Self { replacements }
    }

    pub fn is_empty(&self) -> bool {
        self.replacements.is_empty()
    }

    /// `getNewLineRanges()`: o intervalo de linhas que cada substituição ocupa depois de aplicada.
    pub fn get_new_line_ranges(&self) -> Vec<LineRange> {
        let mut ranges: Vec<LineRange> = Vec::new();
        let mut offset = 0;
        for e in &self.replacements {
            ranges.push(LineRange::of_length(e.line_range.start_line_number + offset, e.new_lines.len() as i32));
            offset += e.new_lines.len() as i32 - e.line_range.length();
        }
        ranges
    }
}

/// `LineReplacement`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LineReplacement {
    pub line_range: LineRange,
    pub new_lines: Vec<JsString>,
}

impl LineReplacement {
    pub fn new(line_range: LineRange, new_lines: Vec<JsString>) -> Self {
        Self { line_range, new_lines }
    }

    /// `LineReplacement.fromSingleTextEdit(edit, initialValue)`.
    pub fn from_single_text_edit(edit: &TextReplacement, initial_value: &dyn AbstractText) -> LineReplacement {
        // 1: ab[cde
        // 2: fghijk
        // 3: lmn]opq

        // replaced with

        // 1n: 123
        // 2n: 456
        // 3n: 789

        // simple solution: replace [1..4) with [1n..4n)

        let mut new_lines: Vec<JsString> = split_lines(edit.text.units()).into_iter().map(JsString::from).collect();
        let mut start_line_number = edit.range.start_line_number;
        let surviving_first_line_text = initial_value
            .get_value_of_range(&Range::from_positions(Position::new(edit.range.start_line_number, 1), edit.range.get_start_position()));
        let first = concat(&surviving_first_line_text, &new_lines[0]);
        new_lines[0] = first;

        let mut end_line_number_ex = edit.range.end_line_number + 1;
        let edit_end_line_number_max_column = initial_value.get_transformer().get_line_length(edit.range.end_line_number) + 1;
        let surviving_end_line_text = initial_value.get_value_of_range(&Range::from_positions(
            edit.range.get_end_position(),
            Position::new(edit.range.end_line_number, edit_end_line_number_max_column),
        ));
        let last_index = new_lines.len() - 1;
        let last = concat(&new_lines[last_index], &surviving_end_line_text);
        new_lines[last_index] = last;

        // Replacing [startLineNumber, endLineNumberEx) with newLines would be correct, however it might not be minimal.

        let start_before_new_line = edit.range.start_column == initial_value.get_transformer().get_line_length(edit.range.start_line_number) + 1;
        let end_after_new_line = edit.range.end_column == 1;

        if start_before_new_line && new_lines[0].len() == surviving_first_line_text.len() {
            // the replacement would not delete any text on the first line
            start_line_number += 1;
            new_lines.remove(0);
        }

        if !new_lines.is_empty()
            && start_line_number < end_line_number_ex
            && end_after_new_line
            && new_lines[new_lines.len() - 1].len() == surviving_end_line_text.len()
        {
            // the replacement would not delete any text on the last line
            end_line_number_ex -= 1;
            new_lines.pop();
        }

        LineReplacement::new(LineRange::new(start_line_number, end_line_number_ex), new_lines)
    }

    /// `toLineEdit()`.
    pub fn to_line_edit(&self) -> LineEdit {
        LineEdit::new(vec![self.clone()])
    }
}
