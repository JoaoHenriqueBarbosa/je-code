//! Porte de `.../bracketPairsTree/beforeEditPositionMapper.ts`: o mapeamento de uma posição do texto
//! editado para a posição equivalente antes das edições, e a distância até a próxima edição.

use super::length::{
    Length, TextLength, length_add, length_diff_non_negative, length_less_than_equal,
    length_of_string, length_to_obj, position_to_length, to_length,
};
use crate::text_model_events::ModelContentChange;

/// `TextEditInfo`: uma edição, com o intervalo trocado (em comprimentos) e o comprimento do texto novo.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TextEditInfo {
    pub start_offset: Length,
    pub end_offset: Length,
    pub new_length: Length,
}

impl TextEditInfo {
    pub fn new(start_offset: Length, end_offset: Length, new_length: Length) -> Self {
        Self {
            start_offset,
            end_offset,
            new_length,
        }
    }

    /// `TextEditInfo.fromModelContentChanges(changes)`: as mudanças do evento vêm do fim para o começo, e o
    /// resultado fica em ordem crescente.
    pub fn from_model_content_changes(changes: &[ModelContentChange]) -> Vec<TextEditInfo> {
        // Tem de estar em ordem crescente
        let mut edits: Vec<TextEditInfo> = changes
            .iter()
            .map(|c| {
                TextEditInfo::new(
                    position_to_length(c.range.get_start_position()),
                    position_to_length(c.range.get_end_position()),
                    length_of_string(c.text.units()),
                )
            })
            .collect();
        edits.reverse();
        edits
    }
}

impl std::fmt::Display for TextEditInfo {
    /// `toString()`.
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "[{}...{}) -> {}",
            length_to_obj(self.start_offset),
            length_to_obj(self.end_offset),
            length_to_obj(self.new_length)
        )
    }
}

/// `BeforeEditPositionMapper`.
pub struct BeforeEditPositionMapper {
    next_edit_idx: usize,
    delta_old_to_new_line_count: i64,
    delta_old_to_new_column_count: i64,
    delta_line_idx_in_old: i64,
    edits: Vec<TextEditInfoCache>,
}

impl BeforeEditPositionMapper {
    /// `edits` tem de estar ordenado por deslocamento, em ordem crescente.
    pub fn new(edits: &[TextEditInfo]) -> Self {
        Self {
            next_edit_idx: 0,
            delta_old_to_new_line_count: 0,
            delta_old_to_new_column_count: 0,
            delta_line_idx_in_old: -1,
            edits: edits.iter().map(TextEditInfoCache::from).collect(),
        }
    }

    /// `offset` tem de ser igual ou maior que o último deslocamento com que este método foi chamado.
    pub fn get_offset_before_change(&mut self, offset: Length) -> Length {
        self.adjust_next_edit(offset);
        self.translate_cur_to_old(offset)
    }

    /// `offset` tem de ser igual ou maior que o último deslocamento com que este método foi chamado. Devolve
    /// `None` se não há mais edição.
    pub fn get_distance_to_next_change(&mut self, offset: Length) -> Option<Length> {
        self.adjust_next_edit(offset);

        let next_change_offset = self
            .edits
            .get(self.next_edit_idx)
            .map(|next_edit| self.translate_old_to_cur(next_edit.offset_obj))?;
        Some(length_diff_non_negative(offset, next_change_offset))
    }

    fn translate_old_to_cur(&self, old_offset_obj: TextLength) -> Length {
        if old_offset_obj.line_count == self.delta_line_idx_in_old {
            to_length(
                old_offset_obj.line_count + self.delta_old_to_new_line_count,
                old_offset_obj.column_count + self.delta_old_to_new_column_count,
            )
        } else {
            to_length(
                old_offset_obj.line_count + self.delta_old_to_new_line_count,
                old_offset_obj.column_count,
            )
        }
    }

    fn translate_cur_to_old(&self, new_offset: Length) -> Length {
        let offset_obj = length_to_obj(new_offset);
        if offset_obj.line_count - self.delta_old_to_new_line_count == self.delta_line_idx_in_old {
            to_length(
                offset_obj.line_count - self.delta_old_to_new_line_count,
                offset_obj.column_count - self.delta_old_to_new_column_count,
            )
        } else {
            to_length(
                offset_obj.line_count - self.delta_old_to_new_line_count,
                offset_obj.column_count,
            )
        }
    }

    fn adjust_next_edit(&mut self, offset: Length) {
        while self.next_edit_idx < self.edits.len() {
            let next_edit = self.edits[self.next_edit_idx];

            // Depois de aplicar a edição, qual é o deslocamento do fim dela (considerando as edições anteriores)?
            let next_edit_end_offset_in_cur =
                self.translate_old_to_cur(next_edit.end_offset_after_obj);

            if length_less_than_equal(next_edit_end_offset_in_cur, offset) {
                // Estamos depois da edição, pule-a
                self.next_edit_idx += 1;

                let next_edit_end_offset_in_cur_obj = length_to_obj(next_edit_end_offset_in_cur);

                // Antes de aplicar a edição, qual é o deslocamento do fim dela (considerando as edições anteriores)?
                let next_edit_end_offset_before_in_cur_obj =
                    length_to_obj(self.translate_old_to_cur(next_edit.end_offset_before_obj));

                let line_delta = next_edit_end_offset_in_cur_obj.line_count
                    - next_edit_end_offset_before_in_cur_obj.line_count;
                self.delta_old_to_new_line_count += line_delta;

                let previous_column_delta =
                    if self.delta_line_idx_in_old == next_edit.end_offset_before_obj.line_count {
                        self.delta_old_to_new_column_count
                    } else {
                        0
                    };
                let column_delta = next_edit_end_offset_in_cur_obj.column_count
                    - next_edit_end_offset_before_in_cur_obj.column_count;
                self.delta_old_to_new_column_count = previous_column_delta + column_delta;
                self.delta_line_idx_in_old = next_edit.end_offset_before_obj.line_count;
            } else {
                // Estamos dentro ou antes da edição.
                break;
            }
        }
    }
}

#[derive(Clone, Copy)]
struct TextEditInfoCache {
    end_offset_before_obj: TextLength,
    end_offset_after_obj: TextLength,
    offset_obj: TextLength,
}

impl From<&TextEditInfo> for TextEditInfoCache {
    fn from(edit: &TextEditInfo) -> Self {
        Self {
            end_offset_before_obj: length_to_obj(edit.end_offset),
            end_offset_after_obj: length_to_obj(length_add(edit.start_offset, edit.new_length)),
            offset_obj: length_to_obj(edit.start_offset),
        }
    }
}
