//! Porte de `src/vs/editor/test/common/core/random.ts`: o `Random` com semente (`MersenneTwister`) e os
//! geradores que os testes usam (`nextIntRange`, `nextString`, `nextMultiLineString`, `nextConsecutiveOffsets`,
//! `nextConsecutivePositions`, `nextTextEdit`).

// Cada teste inclui este arquivo e usa só parte dos geradores.
#![allow(dead_code)]

use je_editor::edits::text_edit::{TextEdit, TextReplacement};
use je_editor::offset_range::OffsetRange;
use je_editor::position::Position;
use je_editor::position_to_offset::{PositionOffsetTransformer, PositionOffsetTransformerBase};
use je_editor::range::Range;
use je_editor::text::abstract_text::AbstractText;


/// `Random.create(seed)`: um Mersenne Twister (MT19937) com a mesma semeadura do original.
pub struct Random {
    mt: [u32; 624],
    index: usize,
}

impl Random {
    pub fn create(seed: u32) -> Self {
        let mut mt = [0u32; 624];
        mt[0] = seed;
        for i in 1..624 {
            let s = mt[i - 1] ^ (mt[i - 1] >> 30);
            mt[i] = (((s & 0xffff_0000) >> 16).wrapping_mul(0x6c07_8965) << 16)
                .wrapping_add((s & 0x0000_ffff).wrapping_mul(0x6c07_8965))
                .wrapping_add(i as u32);
        }
        Self { mt, index: 0 }
    }

    fn next_int(&mut self) -> u32 {
        if self.index == 0 {
            self.generate_numbers();
        }

        let mut y = self.mt[self.index];
        y ^= y >> 11;
        y ^= (y << 7) & 0x9d2c_5680;
        y ^= (y << 15) & 0xefc6_0000;
        y ^= y >> 18;

        self.index = (self.index + 1) % 624;

        y
    }

    /// `nextIntRange(start, endExclusive)`.
    pub fn next_int_range(&mut self, start: i64, end_exclusive: i64) -> i64 {
        let range = (end_exclusive - start) as f64;
        (f64::from(self.next_int()) / (4_294_967_296.0 / range)).floor() as i64 + start
    }

    fn generate_numbers(&mut self) {
        for i in 0..624 {
            let y = (self.mt[i] & 0x8000_0000) + (self.mt[(i + 1) % 624] & 0x7fff_ffff);
            self.mt[i] = self.mt[(i + 397) % 624] ^ (y >> 1);
            if !y.is_multiple_of(2) {
                self.mt[i] ^= 0x9908_b0df;
            }
        }
    }
}

/// `Random.basicAlphabet` e `basicAlphabetMultiline`.
pub const BASIC_ALPHABET: &str = "      abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789";
pub const BASIC_ALPHABET_MULTILINE: &str = "      \n\n\nabcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789";

impl Random {
    pub fn next_char(&mut self, alphabet: &str) -> char {
        let character_index = self.next_int_range(0, alphabet.len() as i64) as i32;
        alphabet.as_bytes()[character_index as usize] as char
    }

    pub fn next_string(&mut self, length: i32, alphabet: &str) -> String {
        let mut random_text = String::new();
        for _ in 0..length {
            random_text.push(self.next_char(alphabet));
        }
        random_text
    }

    pub fn next_multi_line_string(&mut self, line_count: i32, line_length_range: OffsetRange, alphabet: &str) -> String {
        let mut lines: Vec<String> = Vec::new();
        for _ in 0..line_count {
            let line_length = self.next_int_range(i64::from(line_length_range.start), i64::from(line_length_range.end_exclusive)) as i32;
            lines.push(self.next_string(line_length, alphabet));
        }
        lines.join("\n")
    }

    pub fn next_consecutive_offsets(&mut self, range: OffsetRange, count: i32) -> Vec<i32> {
        let mut offsets: Vec<i32> = (0..count).map(|_| self.next_int_range(i64::from(range.start), i64::from(range.end_exclusive)) as i32).collect();
        offsets.sort_unstable();
        offsets
    }

    pub fn next_consecutive_positions(&mut self, source: &dyn AbstractText, count: i32) -> Vec<Position> {
        let t = PositionOffsetTransformer::new(&source.get_value().to_string_lossy());
        let offsets = self.next_consecutive_offsets(OffsetRange::new(0, t.text.len() as i32), count);
        offsets.into_iter().map(|offset| t.get_position(offset)).collect()
    }

    pub fn next_text_edit(&mut self, target: &dyn AbstractText, single_text_edit_count: i64) -> TextEdit {
        let single_text_edit_count = i32::try_from(single_text_edit_count).expect("contagem pequena");
        let mut single_text_edits: Vec<TextReplacement> = Vec::new();

        let positions = self.next_consecutive_positions(target, single_text_edit_count * 2);

        for i in 0..single_text_edit_count as usize {
            let start = positions[i * 2];
            let end = positions[i * 2 + 1];
            let new_text = self.next_string(end.column - start.column, BASIC_ALPHABET_MULTILINE);
            single_text_edits.push(TextReplacement::new(Range::from_positions(start, end), new_text));
        }

        TextEdit::new(single_text_edits).normalize()
    }
}
