//! Porte de `.../bracketPairsTree/combineTextEditInfos.ts`: combina duas sequências de edições (a segunda
//! aplicada depois da primeira) numa só, relativa ao texto original.

use je_base::arrays::ArrayQueue;

use super::before_edit_position_mapper::TextEditInfo;
use super::length::{
    LENGTH_ZERO, Length, length_add, length_diff_non_negative, length_equals, length_is_zero,
    length_to_obj, sum_lengths,
};

/// `combineTextEditInfos(textEditInfoFirst, textEditInfoSecond)`.
pub fn combine_text_edit_infos(
    text_edit_info_first: &[TextEditInfo],
    text_edit_info_second: &[TextEditInfo],
) -> Vec<TextEditInfo> {
    if text_edit_info_first.is_empty() {
        return text_edit_info_second.to_vec();
    }
    if text_edit_info_second.is_empty() {
        return text_edit_info_first.to_vec();
    }

    // s0: o estado antes de qualquer edição
    let s0_to_s1_items = to_length_mapping(text_edit_info_first);
    let mut s0_to_s1_map = ArrayQueue::new(&s0_to_s1_items);
    // s1: o estado depois da primeira edição, mas antes da segunda
    let mut s1_to_s2_map: Vec<Option<LengthMapping>> = to_length_mapping(text_edit_info_second)
        .into_iter()
        .map(Some)
        .collect();
    s1_to_s2_map.push(None); // Copia tudo do antigo para o novo

    // s2: o estado depois das duas edições

    let mut cur_item: Option<LengthMapping> = s0_to_s1_map.dequeue();

    // `s1_length` `None` é o comprimento "infinito".
    let mut next_s0_to_s1_map_with_s1_length_of =
        |s1_length: Option<Length>, cur_item: &mut Option<LengthMapping>| -> Vec<LengthMapping> {
            let Some(mut s1_length) = s1_length else {
                let mut arr = s0_to_s1_map.take_while(|_| true).unwrap_or_default();
                if let Some(item) = cur_item.take() {
                    arr.insert(0, item);
                }
                return arr;
            };

            let mut result: Vec<LengthMapping> = Vec::new();
            while let Some(current) = *cur_item {
                if length_is_zero(s1_length) {
                    break;
                }
                let (item, remaining_item) = current.split_at(s1_length);
                result.push(item);
                s1_length = length_diff_non_negative(item.length_after, s1_length);
                *cur_item = remaining_item.or_else(|| s0_to_s1_map.dequeue());
            }
            if !length_is_zero(s1_length) {
                result.push(LengthMapping::new(false, s1_length, s1_length));
            }
            result
        };

    let mut result: Vec<TextEditInfo> = Vec::new();
    fn push_edit(
        result: &mut Vec<TextEditInfo>,
        start_offset: Length,
        end_offset: Length,
        new_length: Length,
    ) {
        if let Some(last) = result.last_mut()
            && length_equals(last.end_offset, start_offset)
        {
            *last = TextEditInfo::new(
                last.start_offset,
                end_offset,
                length_add(last.new_length, new_length),
            );
        } else {
            result.push(TextEditInfo::new(start_offset, end_offset, new_length));
        }
    }

    let mut s0_offset = LENGTH_ZERO;
    for s1_to_s2 in &s1_to_s2_map {
        let s0_to_s1 =
            next_s0_to_s1_map_with_s1_length_of(s1_to_s2.map(|m| m.length_before), &mut cur_item);

        match s1_to_s2 {
            Some(s1_to_s2) if s1_to_s2.modified => {
                let s0_length = sum_lengths(&s0_to_s1, |s| s.length_before);
                let s0_end_offset = length_add(s0_offset, s0_length);
                push_edit(&mut result, s0_offset, s0_end_offset, s1_to_s2.length_after);
                s0_offset = s0_end_offset;
            }
            _ => {
                for s1 in &s0_to_s1 {
                    let s0_start_offset = s0_offset;
                    s0_offset = length_add(s0_offset, s1.length_before);

                    if s1.modified {
                        push_edit(&mut result, s0_start_offset, s0_offset, s1.length_after);
                    }
                }
            }
        }
    }
    result
}

#[derive(Clone, Copy)]
struct LengthMapping {
    /// Se é falso, os comprimentos antes e depois são iguais.
    modified: bool,
    length_before: Length,
    length_after: Length,
}

impl LengthMapping {
    fn new(modified: bool, length_before: Length, length_after: Length) -> Self {
        Self {
            modified,
            length_before,
            length_after,
        }
    }

    fn split_at(self, length_after: Length) -> (LengthMapping, Option<LengthMapping>) {
        let remaining_length_after = length_diff_non_negative(length_after, self.length_after);
        if length_equals(remaining_length_after, LENGTH_ZERO) {
            (self, None)
        } else if self.modified {
            (
                LengthMapping::new(self.modified, self.length_before, length_after),
                Some(LengthMapping::new(
                    self.modified,
                    LENGTH_ZERO,
                    remaining_length_after,
                )),
            )
        } else {
            (
                LengthMapping::new(self.modified, length_after, length_after),
                Some(LengthMapping::new(
                    self.modified,
                    remaining_length_after,
                    remaining_length_after,
                )),
            )
        }
    }
}

impl std::fmt::Display for LengthMapping {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "{}:{} -> {}",
            if self.modified { 'M' } else { 'U' },
            length_to_obj(self.length_before),
            length_to_obj(self.length_after)
        )
    }
}

fn to_length_mapping(text_edit_infos: &[TextEditInfo]) -> Vec<LengthMapping> {
    let mut result = Vec::new();
    let mut last_offset = LENGTH_ZERO;
    for text_edit_info in text_edit_infos {
        let space_length = length_diff_non_negative(last_offset, text_edit_info.start_offset);
        if !length_is_zero(space_length) {
            result.push(LengthMapping::new(false, space_length, space_length));
        }

        let length_before =
            length_diff_non_negative(text_edit_info.start_offset, text_edit_info.end_offset);
        result.push(LengthMapping::new(
            true,
            length_before,
            text_edit_info.new_length,
        ));

        last_offset = text_edit_info.end_offset;
    }
    result
}
