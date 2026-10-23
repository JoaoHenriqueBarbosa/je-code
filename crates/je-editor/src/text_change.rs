//! Porte de `src/vs/editor/common/core/textChange.ts`.
//!
//! `write` e `read` usam os mesmos `UInt32BE` e `UInt16LE` do original (`buffer.writeUInt32BE` e afins,
//! inlinados aqui porque o `je-base` ainda não tem `base/common/buffer.ts`); a leitura não exige o
//! alinhamento de 2 bytes que o `Uint16Array` do JavaScript exige.
// TODO(integração): mover os helpers de UInt32BE/UInt16LE para je_base::buffer.

use std::fmt;

use je_base::JsString;

use crate::string_builder::decode_utf16le;

fn escape_new_line(str: &JsString) -> String {
    str.to_string_lossy().replace('\n', "\\n").replace('\r', "\\r")
}

/// Uma troca de texto: o texto antigo em `old_position` virou o novo em `new_position`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TextChange {
    pub old_position: usize,
    pub old_text: JsString,
    pub new_position: usize,
    pub new_text: JsString,
}

impl TextChange {
    pub fn new(old_position: usize, old_text: JsString, new_position: usize, new_text: JsString) -> Self {
        Self {
            old_position,
            old_text,
            new_position,
            new_text,
        }
    }

    pub fn old_length(&self) -> usize {
        self.old_text.len()
    }

    pub fn old_end(&self) -> usize {
        self.old_position + self.old_text.len()
    }

    pub fn new_length(&self) -> usize {
        self.new_text.len()
    }

    pub fn new_end(&self) -> usize {
        self.new_position + self.new_text.len()
    }

    fn write_string_size(str: &JsString) -> usize {
        4 + 2 * str.len()
    }

    fn write_string(b: &mut [u8], str: &JsString, mut offset: usize) -> usize {
        b[offset..offset + 4].copy_from_slice(&(str.len() as u32).to_be_bytes());
        offset += 4;
        for &unit in str.units() {
            b[offset..offset + 2].copy_from_slice(&unit.to_le_bytes());
            offset += 2;
        }
        offset
    }

    fn read_string(b: &[u8], mut offset: usize) -> JsString {
        let len = u32::from_be_bytes([b[offset], b[offset + 1], b[offset + 2], b[offset + 3]]) as usize;
        offset += 4;
        decode_utf16le(b, offset, len)
    }

    pub fn write_size(&self) -> usize {
        4 + 4 + Self::write_string_size(&self.old_text) + Self::write_string_size(&self.new_text)
    }

    pub fn write(&self, b: &mut [u8], mut offset: usize) -> usize {
        b[offset..offset + 4].copy_from_slice(&(self.old_position as u32).to_be_bytes());
        offset += 4;
        b[offset..offset + 4].copy_from_slice(&(self.new_position as u32).to_be_bytes());
        offset += 4;
        offset = Self::write_string(b, &self.old_text, offset);
        Self::write_string(b, &self.new_text, offset)
    }

    pub fn read(b: &[u8], mut offset: usize, dest: &mut Vec<TextChange>) -> usize {
        let read_u32 = |offset: usize| u32::from_be_bytes([b[offset], b[offset + 1], b[offset + 2], b[offset + 3]]) as usize;
        let old_position = read_u32(offset);
        offset += 4;
        let new_position = read_u32(offset);
        offset += 4;
        let old_text = Self::read_string(b, offset);
        offset += Self::write_string_size(&old_text);
        let new_text = Self::read_string(b, offset);
        offset += Self::write_string_size(&new_text);
        dest.push(TextChange::new(old_position, old_text, new_position, new_text));
        offset
    }
}

impl fmt::Display for TextChange {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.old_text.is_empty() {
            return write!(f, "(insert@{} \"{}\")", self.old_position, escape_new_line(&self.new_text));
        }
        if self.new_text.is_empty() {
            return write!(f, "(delete@{} \"{}\")", self.old_position, escape_new_line(&self.old_text));
        }
        write!(
            f,
            "(replace@{} \"{}\" with \"{}\")",
            self.old_position,
            escape_new_line(&self.old_text),
            escape_new_line(&self.new_text)
        )
    }
}

/// `compressConsecutiveTextChanges(prevEdits, currEdits)`.
pub fn compress_consecutive_text_changes(prev_edits: Option<Vec<TextChange>>, curr_edits: Vec<TextChange>) -> Vec<TextChange> {
    match prev_edits {
        Some(prev) if !prev.is_empty() => TextChangeCompressor::new(prev, curr_edits).compress(),
        _ => curr_edits,
    }
}

/// `substr(start)` e `substr(0, n)` de um texto UTF-16, com `n` e `start` já dentro do texto.
fn split_at_unit(text: &JsString, offset: usize) -> (JsString, JsString) {
    (text.slice(0, offset), text.slice(offset, text.len()))
}

struct TextChangeCompressor {
    prev_edits: Vec<TextChange>,
    curr_edits: Vec<TextChange>,
    result: Vec<TextChange>,
    prev_delta_offset: isize,
    curr_delta_offset: isize,
}

impl TextChangeCompressor {
    fn new(prev_edits: Vec<TextChange>, curr_edits: Vec<TextChange>) -> Self {
        Self {
            prev_edits,
            curr_edits,
            result: Vec::new(),
            prev_delta_offset: 0,
            curr_delta_offset: 0,
        }
    }

    fn compress(mut self) -> Vec<TextChange> {
        let mut prev_index = 0;
        let mut curr_index = 0;

        let mut prev_edit = self.get_prev(prev_index);
        let mut curr_edit = self.get_curr(curr_index);

        while prev_index < self.prev_edits.len() || curr_index < self.curr_edits.len() {
            let Some(prev) = prev_edit.clone() else {
                self.accept_curr(&curr_edit.expect("curr_edit existe enquanto prev_edit acabou"));
                curr_index += 1;
                curr_edit = self.get_curr(curr_index);
                continue;
            };
            let Some(curr) = curr_edit.clone() else {
                self.accept_prev(&prev);
                prev_index += 1;
                prev_edit = self.get_prev(prev_index);
                continue;
            };

            if curr.old_end() <= prev.new_position {
                self.accept_curr(&curr);
                curr_index += 1;
                curr_edit = self.get_curr(curr_index);
                continue;
            }

            if prev.new_end() <= curr.old_position {
                self.accept_prev(&prev);
                prev_index += 1;
                prev_edit = self.get_prev(prev_index);
                continue;
            }

            if curr.old_position < prev.new_position {
                let (e1, e2) = Self::split_curr(&curr, prev.new_position - curr.old_position);
                self.accept_curr(&e1);
                curr_edit = Some(e2);
                continue;
            }

            if prev.new_position < curr.old_position {
                let (e1, e2) = Self::split_prev(&prev, curr.old_position - prev.new_position);
                self.accept_prev(&e1);
                prev_edit = Some(e2);
                continue;
            }

            // Aqui `curr.old_position == prev.new_position`.
            let (merge_prev, merge_curr);
            if curr.old_end() == prev.new_end() {
                merge_prev = prev;
                merge_curr = curr;
                prev_index += 1;
                prev_edit = self.get_prev(prev_index);
                curr_index += 1;
                curr_edit = self.get_curr(curr_index);
            } else if curr.old_end() < prev.new_end() {
                let (e1, e2) = Self::split_prev(&prev, curr.old_length());
                merge_prev = e1;
                merge_curr = curr;
                prev_edit = Some(e2);
                curr_index += 1;
                curr_edit = self.get_curr(curr_index);
            } else {
                let (e1, e2) = Self::split_curr(&curr, prev.new_length());
                merge_prev = prev;
                merge_curr = e1;
                prev_index += 1;
                prev_edit = self.get_prev(prev_index);
                curr_edit = Some(e2);
            }

            self.prev_delta_offset += merge_prev.new_length() as isize - merge_prev.old_length() as isize;
            self.curr_delta_offset += merge_curr.new_length() as isize - merge_curr.old_length() as isize;
            self.result.push(TextChange::new(
                merge_prev.old_position,
                merge_prev.old_text,
                merge_curr.new_position,
                merge_curr.new_text,
            ));
        }

        let merged = Self::merge(self.result);
        Self::remove_no_ops(merged)
    }

    fn accept_curr(&mut self, curr_edit: &TextChange) {
        self.result.push(Self::rebase_curr(self.prev_delta_offset, curr_edit));
        self.curr_delta_offset += curr_edit.new_length() as isize - curr_edit.old_length() as isize;
    }

    fn get_curr(&self, curr_index: usize) -> Option<TextChange> {
        self.curr_edits.get(curr_index).cloned()
    }

    fn accept_prev(&mut self, prev_edit: &TextChange) {
        self.result.push(Self::rebase_prev(self.curr_delta_offset, prev_edit));
        self.prev_delta_offset += prev_edit.new_length() as isize - prev_edit.old_length() as isize;
    }

    fn get_prev(&self, prev_index: usize) -> Option<TextChange> {
        self.prev_edits.get(prev_index).cloned()
    }

    fn rebase_curr(prev_delta_offset: isize, curr_edit: &TextChange) -> TextChange {
        TextChange::new(
            (curr_edit.old_position as isize - prev_delta_offset) as usize,
            curr_edit.old_text.clone(),
            curr_edit.new_position,
            curr_edit.new_text.clone(),
        )
    }

    fn rebase_prev(curr_delta_offset: isize, prev_edit: &TextChange) -> TextChange {
        TextChange::new(
            prev_edit.old_position,
            prev_edit.old_text.clone(),
            (prev_edit.new_position as isize + curr_delta_offset) as usize,
            prev_edit.new_text.clone(),
        )
    }

    fn split_prev(edit: &TextChange, offset: usize) -> (TextChange, TextChange) {
        let (pre_text, post_text) = split_at_unit(&edit.new_text, offset);
        (
            TextChange::new(edit.old_position, edit.old_text.clone(), edit.new_position, pre_text),
            TextChange::new(edit.old_end(), JsString::new(), edit.new_position + offset, post_text),
        )
    }

    fn split_curr(edit: &TextChange, offset: usize) -> (TextChange, TextChange) {
        let (pre_text, post_text) = split_at_unit(&edit.old_text, offset);
        (
            TextChange::new(edit.old_position, pre_text, edit.new_position, edit.new_text.clone()),
            TextChange::new(edit.old_position + offset, post_text, edit.new_end(), JsString::new()),
        )
    }

    fn merge(edits: Vec<TextChange>) -> Vec<TextChange> {
        let mut iter = edits.into_iter();
        let Some(mut prev) = iter.next() else {
            return Vec::new();
        };
        let mut result = Vec::new();
        for curr in iter {
            if prev.old_end() == curr.old_position {
                // Funde em `prev`.
                let mut old_text = prev.old_text;
                old_text.push_units(curr.old_text.units());
                let mut new_text = prev.new_text;
                new_text.push_units(curr.new_text.units());
                prev = TextChange::new(prev.old_position, old_text, prev.new_position, new_text);
            } else {
                result.push(prev);
                prev = curr;
            }
        }
        result.push(prev);
        result
    }

    fn remove_no_ops(edits: Vec<TextChange>) -> Vec<TextChange> {
        edits.into_iter().filter(|edit| edit.old_text != edit.new_text).collect()
    }
}
