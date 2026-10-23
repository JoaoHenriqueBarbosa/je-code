//! Porte de `src/vs/editor/common/core/text/positionToOffset.ts` e `positionToOffsetImpl.ts`.
//!
//! `positionToOffset.ts` só registra as dependências (`_setPositionOffsetTransformerDependencies`) para
//! quebrar um ciclo de módulos do TypeScript e reexporta `PositionOffsetTransformerBase` e
//! `PositionOffsetTransformer`; em Rust não há ciclo, então o registro não existe e `ensureDependenciesAreSet`
//! (um no-op marcado como dívida no original) também não.
//!
//! Texto segue a semântica do JavaScript: deslocamentos e colunas contam unidades UTF-16. `getStringEdit`,
//! `getStringReplacement`, `getTextReplacement` e `getTextEdit` da base vêm de `StringEdit`/`TextEdit`
//! (`core/edits`), que o original recebe por injeção de dependência; aqui são importados direto.

use std::cell::OnceCell;

use crate::edits::edit::BaseEdit;
use crate::edits::string_edit::{StringEdit, StringReplacement};
use crate::edits::text_edit::{TextEdit, TextReplacement};
use crate::line_range::LineRange;
use crate::offset_range::OffsetRange;
use crate::position::Position;
use crate::range::Range;

// TODO(integração): mover para o módulo próprio (`core/text/textLength.ts`). Fica de fora `ofSubstr`
// (o porte de texto é `&str`/`&[u16]`; use `of_text_units` com `OffsetRange::substring`).
/// `TextLength`: comprimento não negativo de texto em contagem de linhas e colunas.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TextLength {
    pub line_count: i32,
    pub column_count: i32,
}

impl TextLength {
    pub const ZERO: TextLength = TextLength { line_count: 0, column_count: 0 };

    pub const fn new(line_count: i32, column_count: i32) -> Self {
        Self { line_count, column_count }
    }

    /// `lengthDiffNonNegative`.
    pub fn length_diff_non_negative(start: TextLength, end: TextLength) -> TextLength {
        if end.is_less_than(&start) {
            return TextLength::ZERO;
        }
        if start.line_count == end.line_count {
            TextLength::new(0, end.column_count - start.column_count)
        } else {
            TextLength::new(end.line_count - start.line_count, end.column_count)
        }
    }

    /// `betweenPositions`.
    pub fn between_positions(position1: Position, position2: Position) -> TextLength {
        if position1.line_number == position2.line_number {
            TextLength::new(0, position2.column - position1.column)
        } else {
            TextLength::new(position2.line_number - position1.line_number, position2.column - 1)
        }
    }

    /// `fromPosition`.
    pub fn from_position(pos: Position) -> TextLength {
        TextLength::new(pos.line_number - 1, pos.column - 1)
    }

    /// `ofRange`.
    pub fn of_range(range: &Range) -> TextLength {
        TextLength::between_positions(range.get_start_position(), range.get_end_position())
    }

    /// `ofText`: o `for (const c of text)` do original itera pontos de código, não unidades UTF-16.
    pub fn of_text(text: &str) -> TextLength {
        let mut line = 0;
        let mut column = 0;
        for c in text.chars() {
            if c == '\n' {
                line += 1;
                column = 0;
            } else {
                column += 1;
            }
        }
        TextLength::new(line, column)
    }

    /// `ofText` sobre unidades UTF-16 (um surrogate isolado conta como um ponto de código, como no `for...of`).
    pub fn of_text_units(text: &[u16]) -> TextLength {
        TextLength::of_text(&String::from_utf16_lossy(text))
    }

    /// `sum`.
    pub fn sum<T>(fragments: &[T], get_length: impl Fn(&T) -> TextLength) -> TextLength {
        fragments.iter().fold(TextLength::ZERO, |acc, f| acc.add(get_length(f)))
    }

    pub fn is_zero(&self) -> bool {
        self.line_count == 0 && self.column_count == 0
    }

    pub fn is_less_than(&self, other: &TextLength) -> bool {
        if self.line_count != other.line_count {
            return self.line_count < other.line_count;
        }
        self.column_count < other.column_count
    }

    pub fn is_greater_than(&self, other: &TextLength) -> bool {
        if self.line_count != other.line_count {
            return self.line_count > other.line_count;
        }
        self.column_count > other.column_count
    }

    pub fn is_greater_than_or_equal_to(&self, other: &TextLength) -> bool {
        if self.line_count != other.line_count {
            return self.line_count > other.line_count;
        }
        self.column_count >= other.column_count
    }

    /// `compare`: negativo, zero ou positivo.
    pub fn compare(&self, other: &TextLength) -> i32 {
        if self.line_count != other.line_count {
            return self.line_count - other.line_count;
        }
        self.column_count - other.column_count
    }

    pub fn add(&self, other: TextLength) -> TextLength {
        if other.line_count == 0 {
            TextLength::new(self.line_count, self.column_count + other.column_count)
        } else {
            TextLength::new(self.line_count + other.line_count, other.column_count)
        }
    }

    pub fn create_range(&self, start_position: Position) -> Range {
        if self.line_count == 0 {
            Range::new(
                start_position.line_number,
                start_position.column,
                start_position.line_number,
                start_position.column + self.column_count,
            )
        } else {
            Range::new(
                start_position.line_number,
                start_position.column,
                start_position.line_number + self.line_count,
                self.column_count + 1,
            )
        }
    }

    pub fn to_range(&self) -> Range {
        Range::new(1, 1, self.line_count + 1, self.column_count + 1)
    }

    pub fn to_line_range(&self) -> LineRange {
        LineRange::of_length(1, self.line_count + 1)
    }

    pub fn add_to_position(&self, position: Position) -> Position {
        if self.line_count == 0 {
            Position::new(position.line_number, position.column + self.column_count)
        } else {
            Position::new(position.line_number + self.line_count, self.column_count + 1)
        }
    }

    pub fn add_to_range(&self, range: &Range) -> Range {
        Range::from_positions(
            self.add_to_position(range.get_start_position()),
            self.add_to_position(range.get_end_position()),
        )
    }
}

impl std::fmt::Display for TextLength {
    /// `toString()`: `linhas,colunas`.
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{},{}", self.line_count, self.column_count)
    }
}

/// `PositionOffsetTransformerBase`: converte entre posição (linha, coluna) e deslocamento.
pub trait PositionOffsetTransformerBase {
    fn get_offset(&self, position: Position) -> i32;

    fn get_offset_range(&self, range: &Range) -> OffsetRange {
        OffsetRange::new(self.get_offset(range.get_start_position()), self.get_offset(range.get_end_position()))
    }

    fn get_position(&self, offset: i32) -> Position;

    fn get_range(&self, offset_range: OffsetRange) -> Range {
        Range::from_positions(self.get_position(offset_range.start), self.get_position(offset_range.end_exclusive))
    }

    fn get_string_edit(&self, edit: &TextEdit) -> StringEdit {
        StringEdit::new(edit.replacements.iter().map(|e| self.get_string_replacement(e)).collect())
    }

    fn get_string_replacement(&self, edit: &TextReplacement) -> StringReplacement {
        StringReplacement::new(self.get_offset_range(&edit.range), edit.text.clone())
    }

    fn get_text_replacement(&self, edit: &StringReplacement) -> TextReplacement {
        TextReplacement::new(self.get_range(edit.replace_range), edit.new_text.clone())
    }

    fn get_text_edit(&self, edit: &StringEdit) -> TextEdit {
        TextEdit::new(edit.replacements().iter().map(|e| self.get_text_replacement(e)).collect())
    }
}

/// `PositionOffsetTransformer`: a implementação sobre um texto, com os deslocamentos de linha calculados
/// sob demanda.
pub struct PositionOffsetTransformer {
    /// O texto.
    pub text: String,
    /// O texto em unidades UTF-16, a base de todos os deslocamentos.
    units: Vec<u16>,
    line_start_offset_by_line_idx: OnceCell<Vec<i32>>,
    line_end_offset_by_line_idx: OnceCell<Vec<i32>>,
}

impl PositionOffsetTransformer {
    pub fn new(text: &str) -> Self {
        Self {
            text: text.to_string(),
            units: text.encode_utf16().collect(),
            line_start_offset_by_line_idx: OnceCell::new(),
            line_end_offset_by_line_idx: OnceCell::new(),
        }
    }

    fn compute_line_offsets(&self) -> (Vec<i32>, Vec<i32>) {
        let mut starts = vec![0];
        let mut ends = Vec::new();
        for (i, &unit) in self.units.iter().enumerate() {
            if unit == u16::from(b'\n') {
                starts.push(i as i32 + 1);
                if i > 0 && self.units[i - 1] == u16::from(b'\r') {
                    ends.push(i as i32 - 1);
                } else {
                    ends.push(i as i32);
                }
            }
        }
        ends.push(self.units.len() as i32);
        (starts, ends)
    }

    fn ensure_line_offsets(&self) {
        if self.line_start_offset_by_line_idx.get().is_none() {
            let (starts, ends) = self.compute_line_offsets();
            let _ = self.line_start_offset_by_line_idx.set(starts);
            let _ = self.line_end_offset_by_line_idx.set(ends);
        }
    }

    fn line_start_offset_by_line_idx(&self) -> &[i32] {
        self.ensure_line_offsets();
        self.line_start_offset_by_line_idx.get().expect("preenchido por ensure_line_offsets")
    }

    fn line_end_offset_by_line_idx(&self) -> &[i32] {
        self.ensure_line_offsets();
        self.line_end_offset_by_line_idx.get().expect("preenchido por ensure_line_offsets")
    }

    fn validate_position(&self, position: Position) -> Position {
        if position.line_number < 1 {
            return Position::new(1, 1);
        }
        let line_count = self.text_length().line_count + 1;
        if position.line_number > line_count {
            let line_length = self.get_line_length(line_count);
            return Position::new(line_count, line_length + 1);
        }
        if position.column < 1 {
            return Position::new(position.line_number, 1);
        }
        let line_length = self.get_line_length(position.line_number);
        if position.column - 1 > line_length {
            return Position::new(position.line_number, line_length + 1);
        }
        position
    }

    /// `getTextLength(offsetRange)`.
    pub fn get_text_length(&self, offset_range: OffsetRange) -> TextLength {
        TextLength::of_range(&self.get_range(offset_range))
    }

    /// `textLength`.
    pub fn text_length(&self) -> TextLength {
        let starts = self.line_start_offset_by_line_idx();
        let line_idx = starts.len() - 1;
        TextLength::new(line_idx as i32, self.units.len() as i32 - starts[line_idx])
    }

    /// `getLineLength(lineNumber)`: o comprimento da linha sem o fim de linha.
    pub fn get_line_length(&self, line_number: i32) -> i32 {
        let idx = (line_number - 1) as usize;
        self.line_end_offset_by_line_idx()[idx] - self.line_start_offset_by_line_idx()[idx]
    }
}

impl PositionOffsetTransformerBase for PositionOffsetTransformer {
    fn get_offset(&self, position: Position) -> i32 {
        let val_pos = self.validate_position(position);
        self.line_start_offset_by_line_idx()[(val_pos.line_number - 1) as usize] + val_pos.column - 1
    }

    /// `findLastIdxMonotonous(starts, i => i <= offset)`. Um `offset` negativo (fora do contrato, o original
    /// devolve coluna `NaN`) cai na primeira linha.
    fn get_position(&self, offset: i32) -> Position {
        let starts = self.line_start_offset_by_line_idx();
        let idx = starts.partition_point(|&i| i <= offset).saturating_sub(1);
        Position::new(idx as i32 + 1, offset - starts[idx] + 1)
    }
}
