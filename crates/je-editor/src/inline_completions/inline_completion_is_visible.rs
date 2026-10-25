//! Porte de `src/vs/editor/contrib/inlineCompletions/browser/model/inlineCompletionIsVisible.ts`.
//!
//! Notas de tradução:
//! - texto é UTF-16 ([`JsString`]); `substring` é `je_base::js_string::substring` e `trimStart` é
//!   `je_base::js_string::trim_start`;
//! - o `!editRange` do original (o `range` de um `TextReplacement` é sempre presente) não tem ramo em Rust.

use je_base::filters::matches_sub_string;
use je_base::js_string::{substring, trim_start};

use crate::cursor_common::CursorSimpleModel;
use crate::edits::text_edit::TextReplacement;
use crate::inline_completions::compute_ghost_text::single_text_remove_common_prefix;
use crate::model::EndOfLinePreference;
use crate::position::Position;
use crate::range::Range;
use crate::text_model::TextModel;

/// `inlineCompletionIsVisible(singleTextEdit, originalRange, model, cursorPosition)`.
pub fn inline_completion_is_visible(
    single_text_edit: &TextReplacement,
    original_range: Option<&Range>,
    model: &TextModel,
    cursor_position: Position,
) -> bool {
    let minimized_replacement = single_text_remove_common_prefix(single_text_edit, model, None);
    let edit_range = single_text_edit.range;
    if original_range.is_some_and(|original| original.get_start_position() != edit_range.get_start_position())
        || cursor_position.line_number != minimized_replacement.range.start_line_number
        // Se a sugestão fica vazia depois de tirar o prefixo comum com o modelo, o item não seria visível.
        || minimized_replacement.is_empty()
    {
        return false;
    }

    // We might consider comparing by .toLowText, but this requires GhostTextReplacement
    let original_value = model.get_value_in_range_with(&minimized_replacement.range, EndOfLinePreference::Lf);
    let filter_text = &minimized_replacement.text;

    let cursor_pos_index = i64::from((cursor_position.column - minimized_replacement.range.start_column).max(0));

    let mut filter_text_before = substring(filter_text.units(), 0, cursor_pos_index);
    let mut filter_text_after = substring(filter_text.units(), cursor_pos_index, i64::MAX);

    let mut original_value_before = substring(original_value.units(), 0, cursor_pos_index);
    let mut original_value_after = substring(original_value.units(), cursor_pos_index, i64::MAX);

    let original_value_indent =
        <TextModel as CursorSimpleModel>::get_line_indent_column(model, minimized_replacement.range.start_line_number);
    if minimized_replacement.range.start_column <= original_value_indent {
        // Remove indentation
        original_value_before = trim_start(original_value_before);
        if original_value_before.is_empty() {
            original_value_after = trim_start(original_value_after);
        }
        filter_text_before = trim_start(filter_text_before);
        if filter_text_before.is_empty() {
            filter_text_after = trim_start(filter_text_after);
        }
    }

    filter_text_before.starts_with(original_value_before) && matches_sub_string(original_value_after, filter_text_after).is_some()
}
