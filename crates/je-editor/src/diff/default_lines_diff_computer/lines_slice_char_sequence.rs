//! Porte de `src/vs/editor/common/diff/defaultLinesDiffComputer/linesSliceCharSequence.ts`: a sequência de caracteres de um
//! pedaço de linhas, que o diff de caracteres usa (`LinesSliceCharSequence`).
//!
//! Representação:
//! - `string[]` é `&[JsString]` e os caracteres (`elements`) são as unidades de código UTF-16 em `Vec<u16>`. O campo
//!   `lines` do original (só o construtor o lê) não é guardado.
//! - `preference: 'left' | 'right' = 'right'` de `translateOffset` é o enum [`OffsetPreference`] com o padrão passado
//!   explícito por quem chama.
//! - `CharBoundaryCategory` é o enum privado [`CharBoundaryCategory`] e `score: Record<...>` a função
//!   [`CharBoundaryCategory::boundary_score`].

use je_base::arrays_find::{find_first_monotonous, find_last_idx_monotonous, find_last_monotonous};
use je_base::JsString;
use je_base::js_string::{substring, trim_end, trim_start};

use super::algorithms::diff_algorithm::ISequence;
use super::utils::is_space;
use crate::offset_range::OffsetRange;
use crate::position::Position;
use crate::range::Range;

/// `'left' | 'right'`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum OffsetPreference {
    Left,
    Right,
}

/// `LinesSliceCharSequence`.
pub struct LinesSliceCharSequence {
    elements: Vec<u16>,
    first_element_offset_by_line_idx: Vec<i32>,
    line_start_offsets: Vec<i32>,
    trimmed_ws_lengths_by_line_idx: Vec<i32>,
    range: Range,
    pub consider_whitespace_changes: bool,
}

impl LinesSliceCharSequence {
    pub fn new(lines: &[JsString], range: Range, consider_whitespace_changes: bool) -> Self {
        let mut elements: Vec<u16> = Vec::new();
        let mut first_element_offset_by_line_idx: Vec<i32> = vec![0];
        let mut line_start_offsets: Vec<i32> = Vec::new();
        let mut trimmed_ws_lengths_by_line_idx: Vec<i32> = Vec::new();

        for line_number in range.start_line_number..=range.end_line_number {
            let mut line: &[u16] = lines[(line_number - 1) as usize].units();
            let mut line_start_offset = 0;
            if line_number == range.start_line_number && range.start_column > 1 {
                line_start_offset = range.start_column - 1;
                line = substring(line, i64::from(line_start_offset), line.len() as i64);
            }
            line_start_offsets.push(line_start_offset);

            let mut trimmed_ws_length = 0;
            if !consider_whitespace_changes {
                let trimmed_start_line = trim_start(line);
                trimmed_ws_length = (line.len() - trimmed_start_line.len()) as i32;
                line = trim_end(trimmed_start_line);
            }
            trimmed_ws_lengths_by_line_idx.push(trimmed_ws_length);

            let line_length = if line_number == range.end_line_number {
                (range.end_column - 1 - line_start_offset - trimmed_ws_length).min(line.len() as i32)
            } else {
                line.len() as i32
            };
            elements.extend_from_slice(&line[..line_length.max(0) as usize]);

            if line_number < range.end_line_number {
                elements.push(u16::from(b'\n'));
                first_element_offset_by_line_idx.push(elements.len() as i32);
            }
        }

        Self { elements, first_element_offset_by_line_idx, line_start_offsets, trimmed_ws_lengths_by_line_idx, range, consider_whitespace_changes }
    }

    /// `get text()`.
    pub fn text(&self) -> JsString {
        self.get_text(OffsetRange::new(0, self.length()))
    }

    pub fn get_text(&self, range: OffsetRange) -> JsString {
        let start = (range.start.max(0) as usize).min(self.elements.len());
        let end = (range.end_exclusive.max(0) as usize).min(self.elements.len()).max(start);
        JsString::from(&self.elements[start..end])
    }

    pub fn translate_offset(&self, offset: i32, preference: OffsetPreference) -> Position {
        // find smallest i, so that lineBreakOffsets[i] <= offset using binary search
        let i = find_last_idx_monotonous(
            &self.first_element_offset_by_line_idx,
            |value| *value <= offset,
            0,
            self.first_element_offset_by_line_idx.len(),
        ) as usize;
        let line_offset = offset - self.first_element_offset_by_line_idx[i];
        Position::new(
            self.range.start_line_number + i as i32,
            1 + self.line_start_offsets[i]
                + line_offset
                + if line_offset == 0 && preference == OffsetPreference::Left { 0 } else { self.trimmed_ws_lengths_by_line_idx[i] },
        )
    }

    pub fn translate_range(&self, range: OffsetRange) -> Range {
        let pos1 = self.translate_offset(range.start, OffsetPreference::Right);
        let pos2 = self.translate_offset(range.end_exclusive, OffsetPreference::Left);
        if pos2.is_before(&pos1) {
            return Range::from_positions(pos2, pos2);
        }
        Range::from_positions(pos1, pos2)
    }

    /// Finds the word that contains the character at the given offset
    pub fn find_word_containing(&self, offset: i32) -> Option<OffsetRange> {
        if offset < 0 || offset as usize >= self.elements.len() {
            return None;
        }

        if !is_word_char(self.elements[offset as usize]) {
            return None;
        }

        // find start
        let mut start = offset as usize;
        while start > 0 && is_word_char(self.elements[start - 1]) {
            start -= 1;
        }

        // find end
        let mut end = offset as usize;
        while end < self.elements.len() && is_word_char(self.elements[end]) {
            end += 1;
        }

        Some(OffsetRange::new(start as i32, end as i32))
    }

    /// fooBar has the two sub-words foo and bar
    pub fn find_sub_word_containing(&self, offset: i32) -> Option<OffsetRange> {
        if offset < 0 || offset as usize >= self.elements.len() {
            return None;
        }

        if !is_word_char(self.elements[offset as usize]) {
            return None;
        }

        // find start
        let mut start = offset as usize;
        while start > 0 && is_word_char(self.elements[start - 1]) && !is_upper_case(self.elements[start]) {
            start -= 1;
        }

        // find end
        let mut end = offset as usize;
        while end < self.elements.len() && is_word_char(self.elements[end]) && !is_upper_case(self.elements[end]) {
            end += 1;
        }

        Some(OffsetRange::new(start as i32, end as i32))
    }

    pub fn count_lines_in(&self, range: OffsetRange) -> i32 {
        self.translate_offset(range.end_exclusive, OffsetPreference::Right).line_number
            - self.translate_offset(range.start, OffsetPreference::Right).line_number
    }

    pub fn extend_to_full_lines(&self, range: OffsetRange) -> OffsetRange {
        let start = find_last_monotonous(&self.first_element_offset_by_line_idx, |x| *x <= range.start).copied().unwrap_or(0);
        let end = find_first_monotonous(&self.first_element_offset_by_line_idx, |x| range.end_exclusive <= *x)
            .copied()
            .unwrap_or(self.elements.len() as i32);
        OffsetRange::new(start, end)
    }
}

impl ISequence for LinesSliceCharSequence {
    fn get_element(&self, offset: i32) -> i32 {
        i32::from(self.elements[offset as usize])
    }

    fn length(&self) -> i32 {
        self.elements.len() as i32
    }

    fn supports_boundary_score(&self) -> bool {
        true
    }

    fn get_boundary_score(&self, length: i32) -> i32 {
        //   a   b   c   ,           d   e   f
        // 11  0   0   12  15  6   13  0   0   11

        let prev_category = get_category(if length > 0 { i32::from(self.elements[length as usize - 1]) } else { -1 });
        let next_category = get_category(if (length as usize) < self.elements.len() { i32::from(self.elements[length as usize]) } else { -1 });

        if prev_category == CharBoundaryCategory::LineBreakCR && next_category == CharBoundaryCategory::LineBreakLF {
            // don't break between \r and \n
            return 0;
        }
        if prev_category == CharBoundaryCategory::LineBreakLF {
            // prefer the linebreak before the change
            return 150;
        }

        let mut score = 0;
        if prev_category != next_category {
            score += 10;
            if prev_category == CharBoundaryCategory::WordLower && next_category == CharBoundaryCategory::WordUpper {
                score += 1;
            }
        }

        score += prev_category.boundary_score();
        score += next_category.boundary_score();

        score
    }

    fn is_strongly_equal(&self, offset1: i32, offset2: i32) -> bool {
        self.elements[offset1 as usize] == self.elements[offset2 as usize]
    }
}

impl std::fmt::Display for LinesSliceCharSequence {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Slice: \"{}\"", self.text())
    }
}

fn is_word_char(char_code: u16) -> bool {
    (u16::from(b'a')..=u16::from(b'z')).contains(&char_code)
        || (u16::from(b'A')..=u16::from(b'Z')).contains(&char_code)
        || (u16::from(b'0')..=u16::from(b'9')).contains(&char_code)
}

fn is_upper_case(char_code: u16) -> bool {
    (u16::from(b'A')..=u16::from(b'Z')).contains(&char_code)
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum CharBoundaryCategory {
    WordLower,
    WordUpper,
    WordNumber,
    End,
    Other,
    Separator,
    Space,
    LineBreakCR,
    LineBreakLF,
}

impl CharBoundaryCategory {
    /// `score[category]`.
    fn boundary_score(self) -> i32 {
        match self {
            CharBoundaryCategory::WordLower | CharBoundaryCategory::WordUpper | CharBoundaryCategory::WordNumber => 0,
            CharBoundaryCategory::End => 10,
            CharBoundaryCategory::Other => 2,
            CharBoundaryCategory::Separator => 30,
            CharBoundaryCategory::Space => 3,
            CharBoundaryCategory::LineBreakCR | CharBoundaryCategory::LineBreakLF => 10,
        }
    }
}

fn get_category(char_code: i32) -> CharBoundaryCategory {
    let is_in = |from: u8, to: u8| (i32::from(from)..=i32::from(to)).contains(&char_code);
    if char_code == i32::from(b'\n') {
        CharBoundaryCategory::LineBreakLF
    } else if char_code == i32::from(b'\r') {
        CharBoundaryCategory::LineBreakCR
    } else if is_space(char_code) {
        CharBoundaryCategory::Space
    } else if is_in(b'a', b'z') {
        CharBoundaryCategory::WordLower
    } else if is_in(b'A', b'Z') {
        CharBoundaryCategory::WordUpper
    } else if is_in(b'0', b'9') {
        CharBoundaryCategory::WordNumber
    } else if char_code == -1 {
        CharBoundaryCategory::End
    } else if char_code == i32::from(b',') || char_code == i32::from(b';') {
        CharBoundaryCategory::Separator
    } else {
        CharBoundaryCategory::Other
    }
}
