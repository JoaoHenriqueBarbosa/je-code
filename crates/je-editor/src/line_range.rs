//! Porte de `src/vs/editor/common/core/ranges/lineRange.ts`: `LineRange` e `LineRangeSet`.
//!
//! `IRange` e `Range` são o mesmo tipo (veja `range.rs`). Os números de linha são `i32`, como no resto do
//! crate; `Number.MAX_SAFE_INTEGER` de `toInclusiveRange` vira `i32::MAX`, a maior coluna que o `Range`
//! representa.

use std::cmp::Ordering;

use je_base::arrays_find::{find_first_idx_monotonous_or_arr_len, find_last_idx_monotonous, find_last_monotonous};
use je_base::errors::{self, Error};
use je_base::observable::deps::StrictEquals;

use crate::offset_range::OffsetRange;
use crate::range::Range;

/// `ISerializedLineRange`: `[startLineNumber, endLineNumberExclusive]`.
pub type SerializedLineRange = [i32; 2];

/// `LineRange`: intervalo de linhas (base 1) `[start_line_number, end_line_number_exclusive)`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct LineRange {
    /// A linha inicial.
    pub start_line_number: i32,
    /// A linha final (exclusiva).
    pub end_line_number_exclusive: i32,
}

/// O `===` de um observável que carrega um `LineRange` compara por valor: o observável só avisa quando o intervalo
/// mudou de fato (o mesmo critério do `Rect` e do `Point`).
impl StrictEquals for LineRange {
    fn strict_equals(&self, other: &Self) -> bool {
        self == other
    }
}

impl LineRange {
    pub fn new(start_line_number: i32, end_line_number_exclusive: i32) -> Self {
        if start_line_number > end_line_number_exclusive {
            errors::throw(Error::bug_indicating(Some(&format!(
                "startLineNumber {start_line_number} cannot be after endLineNumberExclusive {end_line_number_exclusive}"
            ))));
        }
        Self { start_line_number, end_line_number_exclusive }
    }

    pub fn of_length(start_line_number: i32, length: i32) -> Self {
        Self::new(start_line_number, start_line_number + length)
    }

    pub fn from_range(range: &Range) -> Self {
        Self::new(range.start_line_number, range.end_line_number)
    }

    pub fn from_range_inclusive(range: &Range) -> Self {
        Self::new(range.start_line_number, range.end_line_number + 1)
    }

    /// `LineRange.compareByStart`: o comparador por linha inicial.
    pub fn compare_by_start(a: &LineRange, b: &LineRange) -> Ordering {
        a.start_line_number.cmp(&b.start_line_number)
    }

    pub fn subtract(a: LineRange, b: Option<LineRange>) -> Vec<LineRange> {
        let Some(b) = b else {
            return vec![a];
        };
        if a.start_line_number < b.start_line_number && b.end_line_number_exclusive < a.end_line_number_exclusive {
            vec![
                LineRange::new(a.start_line_number, b.start_line_number),
                LineRange::new(b.end_line_number_exclusive, a.end_line_number_exclusive),
            ]
        } else if b.start_line_number <= a.start_line_number && a.end_line_number_exclusive <= b.end_line_number_exclusive {
            Vec::new()
        } else if b.end_line_number_exclusive < a.end_line_number_exclusive {
            vec![LineRange::new(b.end_line_number_exclusive.max(a.start_line_number), a.end_line_number_exclusive)]
        } else {
            vec![LineRange::new(a.start_line_number, b.start_line_number.min(a.end_line_number_exclusive))]
        }
    }

    /// `LineRange.joinMany`: a união normalizada de vários conjuntos de intervalos, cada um já ordenado.
    pub fn join_many(line_ranges: &[Vec<LineRange>]) -> Vec<LineRange> {
        let Some((first, rest)) = line_ranges.split_first() else {
            return Vec::new();
        };
        let mut result = LineRangeSet::new(first.clone());
        for ranges in rest {
            result = result.get_union(&LineRangeSet::new(ranges.clone()));
        }
        result.normalized_ranges
    }

    /// `LineRange.join(lineRanges)`: o menor intervalo que cobre todos; lista vazia é erro de lógica.
    pub fn join_all(line_ranges: &[LineRange]) -> LineRange {
        if line_ranges.is_empty() {
            errors::throw(Error::bug_indicating(Some("lineRanges cannot be empty")));
        }
        let mut start_line_number = line_ranges[0].start_line_number;
        let mut end_line_number_exclusive = line_ranges[0].end_line_number_exclusive;
        for r in &line_ranges[1..] {
            start_line_number = start_line_number.min(r.start_line_number);
            end_line_number_exclusive = end_line_number_exclusive.max(r.end_line_number_exclusive);
        }
        LineRange::new(start_line_number, end_line_number_exclusive)
    }

    pub fn deserialize(line_range: SerializedLineRange) -> LineRange {
        LineRange::new(line_range[0], line_range[1])
    }

    /// Indica se este intervalo contém a linha dada.
    pub fn contains(&self, line_number: i32) -> bool {
        self.start_line_number <= line_number && line_number < self.end_line_number_exclusive
    }

    pub fn contains_range(&self, range: &LineRange) -> bool {
        self.start_line_number <= range.start_line_number && range.end_line_number_exclusive <= self.end_line_number_exclusive
    }

    pub fn is_empty(&self) -> bool {
        self.start_line_number == self.end_line_number_exclusive
    }

    /// Move este intervalo pelo deslocamento de linhas.
    pub fn delta(&self, offset: i32) -> LineRange {
        LineRange::new(self.start_line_number + offset, self.end_line_number_exclusive + offset)
    }

    pub fn delta_length(&self, offset: i32) -> LineRange {
        LineRange::new(self.start_line_number, self.end_line_number_exclusive + offset)
    }

    /// O número de linhas que o intervalo cobre.
    pub fn length(&self) -> i32 {
        self.end_line_number_exclusive - self.start_line_number
    }

    /// `join(other)`: o intervalo que combina este e o dado.
    pub fn join(&self, other: &LineRange) -> LineRange {
        LineRange::new(
            self.start_line_number.min(other.start_line_number),
            self.end_line_number_exclusive.max(other.end_line_number_exclusive),
        )
    }

    /// O resultado é vazio se os intervalos apenas se tocam; se nem se tocam, é `None`.
    pub fn intersect(&self, other: &LineRange) -> Option<LineRange> {
        let start_line_number = self.start_line_number.max(other.start_line_number);
        let end_line_number_exclusive = self.end_line_number_exclusive.min(other.end_line_number_exclusive);
        if start_line_number <= end_line_number_exclusive {
            return Some(LineRange::new(start_line_number, end_line_number_exclusive));
        }
        None
    }

    pub fn intersects_strict(&self, other: &LineRange) -> bool {
        self.start_line_number < other.end_line_number_exclusive && other.start_line_number < self.end_line_number_exclusive
    }

    pub fn intersects_or_touches(&self, other: &LineRange) -> bool {
        self.start_line_number <= other.end_line_number_exclusive && other.start_line_number <= self.end_line_number_exclusive
    }

    /// `toInclusiveRange`: `None` quando o intervalo é vazio.
    pub fn to_inclusive_range(&self) -> Option<Range> {
        if self.is_empty() {
            return None;
        }
        Some(Range::new(self.start_line_number, 1, self.end_line_number_exclusive - 1, i32::MAX))
    }

    /// `toExclusiveRange` (deprecado no original): a posição final não é garantidamente válida.
    pub fn to_exclusive_range(&self) -> Range {
        Range::new(self.start_line_number, 1, self.end_line_number_exclusive, 1)
    }

    pub fn map_to_line_array<T>(&self, mut f: impl FnMut(i32) -> T) -> Vec<T> {
        let mut result = Vec::new();
        for line_number in self.start_line_number..self.end_line_number_exclusive {
            result.push(f(line_number));
        }
        result
    }

    pub fn for_each(&self, mut f: impl FnMut(i32)) {
        for line_number in self.start_line_number..self.end_line_number_exclusive {
            f(line_number);
        }
    }

    pub fn serialize(&self) -> SerializedLineRange {
        [self.start_line_number, self.end_line_number_exclusive]
    }

    /// Converte este intervalo de linhas (base 1) em intervalo de deslocamentos (base 0, subtrai 1).
    pub fn to_offset_range(&self) -> OffsetRange {
        OffsetRange::new(self.start_line_number - 1, self.end_line_number_exclusive - 1)
    }

    pub fn distance_to_range(&self, other: &LineRange) -> i32 {
        if self.end_line_number_exclusive <= other.start_line_number {
            return other.start_line_number - self.end_line_number_exclusive;
        }
        if other.end_line_number_exclusive <= self.start_line_number {
            return self.start_line_number - other.end_line_number_exclusive;
        }
        0
    }

    pub fn distance_to_line(&self, line_number: i32) -> i32 {
        if self.contains(line_number) {
            return 0;
        }
        if line_number < self.start_line_number {
            return self.start_line_number - line_number;
        }
        line_number - self.end_line_number_exclusive
    }

    pub fn add_margin(&self, margin_top: i32, margin_bottom: i32) -> LineRange {
        LineRange::new(self.start_line_number - margin_top, self.end_line_number_exclusive + margin_bottom)
    }
}

impl std::fmt::Display for LineRange {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "[{},{})", self.start_line_number, self.end_line_number_exclusive)
    }
}

/// `LineRangeSet`: intervalos ordenados pela linha inicial, sem dois que se toquem ou se cruzem.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct LineRangeSet {
    normalized_ranges: Vec<LineRange>,
}

impl LineRangeSet {
    /// O construtor confia que `normalized_ranges` já está ordenada e normalizada.
    pub fn new(normalized_ranges: Vec<LineRange>) -> Self {
        Self { normalized_ranges }
    }

    pub fn ranges(&self) -> &[LineRange] {
        &self.normalized_ranges
    }

    pub fn add_range(&mut self, range: LineRange) {
        if range.length() == 0 {
            return;
        }

        // O índice do primeiro elemento que toca `range` ou que vem depois dele.
        let ranges = &self.normalized_ranges;
        let join_range_start_idx =
            find_first_idx_monotonous_or_arr_len(ranges, |r| r.end_line_number_exclusive >= range.start_line_number, 0, ranges.len());
        // O índice depois do último elemento que toca `range` ou que vem antes dele.
        let join_range_end_idx_exclusive =
            (find_last_idx_monotonous(ranges, |r| r.start_line_number <= range.end_line_number_exclusive, 0, ranges.len()) + 1) as usize;

        if join_range_start_idx == join_range_end_idx_exclusive {
            // Nenhum elemento toca `range`: o índice é o do elemento depois dele.
            self.normalized_ranges.insert(join_range_start_idx, range);
        } else if join_range_start_idx == join_range_end_idx_exclusive - 1 {
            // Um só elemento toca `range`: ele é o primeiro e o último, então se substitui.
            let join_range = self.normalized_ranges[join_range_start_idx];
            self.normalized_ranges[join_range_start_idx] = join_range.join(&range);
        } else {
            // O primeiro e o último diferem: substitui-se a faixa inteira.
            let join_range = self.normalized_ranges[join_range_start_idx]
                .join(&self.normalized_ranges[join_range_end_idx_exclusive - 1])
                .join(&range);
            self.normalized_ranges.splice(join_range_start_idx..join_range_end_idx_exclusive, [join_range]);
        }
    }

    pub fn contains(&self, line_number: i32) -> bool {
        let range_that_starts_before_end = find_last_monotonous(&self.normalized_ranges, |r| r.start_line_number <= line_number);
        range_that_starts_before_end.is_some_and(|r| r.end_line_number_exclusive > line_number)
    }

    pub fn intersects(&self, range: &LineRange) -> bool {
        let range_that_starts_before_end = find_last_monotonous(&self.normalized_ranges, |r| r.start_line_number < range.end_line_number_exclusive);
        range_that_starts_before_end.is_some_and(|r| r.end_line_number_exclusive > range.start_line_number)
    }

    pub fn get_union(&self, other: &LineRangeSet) -> LineRangeSet {
        if self.normalized_ranges.is_empty() {
            return other.clone();
        }
        if other.normalized_ranges.is_empty() {
            return self.clone();
        }

        let mut result: Vec<LineRange> = Vec::new();
        let mut i1 = 0;
        let mut i2 = 0;
        let mut current: Option<LineRange> = None;
        while i1 < self.normalized_ranges.len() || i2 < other.normalized_ranges.len() {
            let next;
            if i1 < self.normalized_ranges.len() && i2 < other.normalized_ranges.len() {
                let line_range1 = self.normalized_ranges[i1];
                let line_range2 = other.normalized_ranges[i2];
                if line_range1.start_line_number < line_range2.start_line_number {
                    next = line_range1;
                    i1 += 1;
                } else {
                    next = line_range2;
                    i2 += 1;
                }
            } else if i1 < self.normalized_ranges.len() {
                next = self.normalized_ranges[i1];
                i1 += 1;
            } else {
                next = other.normalized_ranges[i2];
                i2 += 1;
            }

            match current {
                None => current = Some(next),
                Some(c) => {
                    if c.end_line_number_exclusive >= next.start_line_number {
                        // Funde.
                        current = Some(LineRange::new(c.start_line_number, c.end_line_number_exclusive.max(next.end_line_number_exclusive)));
                    } else {
                        // Empilha.
                        result.push(c);
                        current = Some(next);
                    }
                }
            }
        }
        if let Some(c) = current {
            result.push(c);
        }
        LineRangeSet::new(result)
    }

    /// Subtrai todos os intervalos deste conjunto de `range` e devolve o resultado.
    pub fn subtract_from(&self, range: &LineRange) -> LineRangeSet {
        let ranges = &self.normalized_ranges;
        // O índice do primeiro elemento que toca `range` ou que vem depois dele.
        let join_range_start_idx =
            find_first_idx_monotonous_or_arr_len(ranges, |r| r.end_line_number_exclusive >= range.start_line_number, 0, ranges.len());
        // O índice depois do último elemento que toca `range` ou que vem antes dele.
        let join_range_end_idx_exclusive =
            (find_last_idx_monotonous(ranges, |r| r.start_line_number <= range.end_line_number_exclusive, 0, ranges.len()) + 1) as usize;

        if join_range_start_idx == join_range_end_idx_exclusive {
            return LineRangeSet::new(vec![*range]);
        }

        let mut result: Vec<LineRange> = Vec::new();
        let mut start_line_number = range.start_line_number;
        for r in &ranges[join_range_start_idx..join_range_end_idx_exclusive] {
            if r.start_line_number > start_line_number {
                result.push(LineRange::new(start_line_number, r.start_line_number));
            }
            start_line_number = r.end_line_number_exclusive;
        }
        if start_line_number < range.end_line_number_exclusive {
            result.push(LineRange::new(start_line_number, range.end_line_number_exclusive));
        }

        LineRangeSet::new(result)
    }

    pub fn get_intersection(&self, other: &LineRangeSet) -> LineRangeSet {
        let mut result: Vec<LineRange> = Vec::new();

        let mut i1 = 0;
        let mut i2 = 0;
        while i1 < self.normalized_ranges.len() && i2 < other.normalized_ranges.len() {
            let r1 = self.normalized_ranges[i1];
            let r2 = other.normalized_ranges[i2];

            if let Some(i) = r1.intersect(&r2)
                && !i.is_empty()
            {
                result.push(i);
            }

            if r1.end_line_number_exclusive < r2.end_line_number_exclusive {
                i1 += 1;
            } else {
                i2 += 1;
            }
        }

        LineRangeSet::new(result)
    }

    pub fn get_with_delta(&self, value: i32) -> LineRangeSet {
        LineRangeSet::new(self.normalized_ranges.iter().map(|r| r.delta(value)).collect())
    }
}

impl std::fmt::Display for LineRangeSet {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let parts: Vec<String> = self.normalized_ranges.iter().map(|r| r.to_string()).collect();
        write!(f, "{}", parts.join(", "))
    }
}
