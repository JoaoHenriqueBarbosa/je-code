//! Porte de `src/vs/editor/common/diff/defaultLinesDiffComputer/lineSequence.ts`: a sequência de linhas que o diff de
//! linhas usa (`LineSequence`).
//!
//! `string[]` é `Vec<JsString>` e o `trimmedHash` (o hash de cada linha sem as pontas em branco) é `Vec<i32>`.

use je_base::JsString;

use super::algorithms::diff_algorithm::ISequence;
use crate::offset_range::OffsetRange;

/// `LineSequence`.
pub struct LineSequence {
    trimmed_hash: Vec<i32>,
    lines: Vec<JsString>,
}

impl LineSequence {
    pub fn new(trimmed_hash: Vec<i32>, lines: Vec<JsString>) -> Self {
        Self { trimmed_hash, lines }
    }

    pub fn get_text(&self, range: OffsetRange) -> JsString {
        let start = (range.start.max(0) as usize).min(self.lines.len());
        let end = (range.end_exclusive.max(0) as usize).min(self.lines.len()).max(start);
        let mut text = JsString::new();
        for (index, line) in self.lines[start..end].iter().enumerate() {
            if index > 0 {
                text.push_unit(u16::from(b'\n'));
            }
            text.push_units(line.units());
        }
        text
    }
}

impl ISequence for LineSequence {
    fn get_element(&self, offset: i32) -> i32 {
        self.trimmed_hash[offset as usize]
    }

    fn length(&self) -> i32 {
        self.trimmed_hash.len() as i32
    }

    fn supports_boundary_score(&self) -> bool {
        true
    }

    fn get_boundary_score(&self, length: i32) -> i32 {
        let indentation_before = if length == 0 { 0 } else { get_indentation(&self.lines[length as usize - 1]) };
        let indentation_after = if length as usize == self.lines.len() { 0 } else { get_indentation(&self.lines[length as usize]) };
        1000 - (indentation_before + indentation_after)
    }

    fn is_strongly_equal(&self, offset1: i32, offset2: i32) -> bool {
        self.lines[offset1 as usize] == self.lines[offset2 as usize]
    }
}

fn get_indentation(str: &JsString) -> i32 {
    let mut i = 0;
    let units = str.units();
    while i < units.len() && (units[i] == u16::from(b' ') || units[i] == u16::from(b'\t')) {
        i += 1;
    }
    i as i32
}
