//! Porte de `src/vs/editor/common/core/ranges/lineRange.ts` (a parte usada pela tokenização).

use je_base::errors::{self, Error};

/// `LineRange`: intervalo de linhas `[start_line_number, end_line_number_exclusive)`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct LineRange {
    pub start_line_number: i32,
    pub end_line_number_exclusive: i32,
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

    /// `LineRange.joinMany`: a união normalizada de vários conjuntos de intervalos.
    pub fn join_many(line_ranges: &[Vec<LineRange>]) -> Vec<LineRange> {
        let mut all: Vec<LineRange> = line_ranges.iter().flatten().copied().filter(|r| r.length() != 0).collect();
        all.sort_by_key(|r| r.start_line_number);
        let mut result: Vec<LineRange> = Vec::new();
        for next in all {
            match result.last_mut() {
                Some(current) if current.end_line_number_exclusive >= next.start_line_number => {
                    current.end_line_number_exclusive = current.end_line_number_exclusive.max(next.end_line_number_exclusive);
                }
                _ => result.push(next),
            }
        }
        result
    }

    pub fn length(&self) -> i32 {
        self.end_line_number_exclusive - self.start_line_number
    }

    pub fn is_empty(&self) -> bool {
        self.start_line_number == self.end_line_number_exclusive
    }

    pub fn contains(&self, line_number: i32) -> bool {
        self.start_line_number <= line_number && line_number < self.end_line_number_exclusive
    }
}

impl std::fmt::Display for LineRange {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "[{},{})", self.start_line_number, self.end_line_number_exclusive)
    }
}
