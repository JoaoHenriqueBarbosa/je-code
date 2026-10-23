//! Porte de `src/vs/editor/common/model/bracketPairsTextModelPart/bracketPairsTree/length.ts`: um
//! comprimento não negativo em linhas e colunas, empacotado num único inteiro (sem alocação).
//!
//! No original o `Length` é um `number` de 52 bits: 26 para a linha e 26 para a coluna. Aqui é um `i64` com
//! a mesma codificação, então `lengthAdd`, as comparações e o `lengthHash` coincidem bit a bit. O piso
//! (`Math.floor`) sobre valores negativos vira `div_euclid`.

use std::fmt;

use je_base::strings::split_lines;

use crate::position::Position;
use crate::range::Range;

/// `2 ** 26`.
const FACTOR: i64 = 1 << 26;

/// `Length`: `lineCount * 2^26 + columnCount`. A ordem dos inteiros é a ordem dos comprimentos.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Default)]
pub struct Length(i64);

/// `lengthZero`.
pub const LENGTH_ZERO: Length = Length(0);

/// `TextLength` (`core/text/textLength.ts`), a forma com campos nomeados de um [`Length`].
// TODO(integração): mover para `core/text/text_length.rs` quando o `core/text` for portado.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TextLength {
    pub line_count: i64,
    pub column_count: i64,
}

impl TextLength {
    pub fn new(line_count: i64, column_count: i64) -> Self {
        Self {
            line_count,
            column_count,
        }
    }
}

impl fmt::Display for TextLength {
    /// `toString()`: `linhas,colunas`.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{},{}", self.line_count, self.column_count)
    }
}

/// `lengthDiff(startLineCount, startColumnCount, endLineCount, endColumnCount)`: o fim é maior ou igual ao
/// início.
pub fn length_diff(
    start_line_count: i64,
    start_column_count: i64,
    end_line_count: i64,
    end_column_count: i64,
) -> Length {
    if start_line_count != end_line_count {
        to_length(end_line_count - start_line_count, end_column_count)
    } else {
        to_length(0, end_column_count - start_column_count)
    }
}

pub fn length_is_zero(length: Length) -> bool {
    length.0 == 0
}

/// `toLength(lineCount, columnCount)`. Sem estouro (tudo abaixo de `2^26`), vale
/// `toLength(l1, c1) + toLength(l2, c2) = toLength(l1 + l2, c1 + c2)`.
pub fn to_length(line_count: i64, column_count: i64) -> Length {
    Length(line_count * FACTOR + column_count)
}

pub fn length_to_obj(length: Length) -> TextLength {
    let line_count = length.0.div_euclid(FACTOR);
    let column_count = length.0 - line_count * FACTOR;
    TextLength::new(line_count, column_count)
}

pub fn length_get_line_count(length: Length) -> i64 {
    length.0.div_euclid(FACTOR)
}

/// A quantidade de colunas, supondo que o comprimento não atravessa nenhuma linha.
pub fn length_get_column_count_if_zero_line_count(length: Length) -> i64 {
    length.0
}

/// `[10 linhas, 5 col] + [0 linhas, 3 col] = [10 linhas, 8 col]` e
/// `[10 linhas, 5 col] + [20 linhas, 3 col] = [30 linhas, 3 col]`.
pub fn length_add(l1: Length, l2: Length) -> Length {
    let mut r = l1.0 + l2.0;
    if l2.0 >= FACTOR {
        r -= l1.0 % FACTOR;
    }
    Length(r)
}

/// `sumLengths(items, lengthFn)`.
pub fn sum_lengths<T>(items: &[T], length_fn: impl Fn(&T) -> Length) -> Length {
    items
        .iter()
        .fold(LENGTH_ZERO, |a, b| length_add(a, length_fn(b)))
}

pub fn length_equals(length1: Length, length2: Length) -> bool {
    length1 == length2
}

/// O `result` não negativo tal que `lengthAdd(length1, result) = length2`, ou zero se não existir.
pub fn length_diff_non_negative(length1: Length, length2: Length) -> Length {
    let (l1, l2) = (length1.0, length2.0);
    let diff = l2 - l1;
    if diff <= 0 {
        // A contagem de linhas de length1 é maior que a de length2, ou são iguais e a coluna de length1 é maior.
        return LENGTH_ZERO;
    }

    let line_count1 = l1.div_euclid(FACTOR);
    let line_count2 = l2.div_euclid(FACTOR);
    let col_count2 = l2 - line_count2 * FACTOR;

    if line_count1 == line_count2 {
        let col_count1 = l1 - line_count1 * FACTOR;
        to_length(0, col_count2 - col_count1)
    } else {
        to_length(line_count2 - line_count1, col_count2)
    }
}

pub fn length_less_than(length1: Length, length2: Length) -> bool {
    length1.0 < length2.0
}

pub fn length_less_than_equal(length1: Length, length2: Length) -> bool {
    length1.0 <= length2.0
}

pub fn length_greater_than_equal(length1: Length, length2: Length) -> bool {
    length1.0 >= length2.0
}

pub fn length_to_position(length: Length) -> Position {
    let text_length = length_to_obj(length);
    Position::new(
        text_length.line_count as i32 + 1,
        text_length.column_count as i32 + 1,
    )
}

pub fn position_to_length(position: Position) -> Length {
    to_length(
        i64::from(position.line_number) - 1,
        i64::from(position.column) - 1,
    )
}

pub fn lengths_to_range(length_start: Length, length_end: Length) -> Range {
    let start = length_to_obj(length_start);
    let end = length_to_obj(length_end);
    Range::new(
        start.line_count as i32 + 1,
        start.column_count as i32 + 1,
        end.line_count as i32 + 1,
        end.column_count as i32 + 1,
    )
}

pub fn length_of_range(range: &Range) -> TextLength {
    if range.start_line_number == range.end_line_number {
        TextLength::new(0, i64::from(range.end_column - range.start_column))
    } else {
        TextLength::new(
            i64::from(range.end_line_number - range.start_line_number),
            i64::from(range.end_column - 1),
        )
    }
}

pub fn length_compare(length1: Length, length2: Length) -> i64 {
    length1.0 - length2.0
}

/// `lengthOfString(str)`.
pub fn length_of_string(str: &[u16]) -> Length {
    let lines = split_lines(str);
    to_length(lines.len() as i64 - 1, lines[lines.len() - 1].len() as i64)
}

pub fn length_of_string_obj(str: &[u16]) -> TextLength {
    let lines = split_lines(str);
    TextLength::new(lines.len() as i64 - 1, lines[lines.len() - 1].len() as i64)
}

/// Um hash numérico do comprimento.
pub fn length_hash(length: Length) -> i64 {
    length.0
}

pub fn length_max(length1: Length, length2: Length) -> Length {
    if length1 > length2 { length1 } else { length2 }
}
