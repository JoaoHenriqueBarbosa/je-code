//! Porte de `src/vs/editor/common/core/ranges/offsetRange.ts` (a parte usada pela tokenização).

use je_base::errors::{self, Error};

/// `OffsetRange`: intervalo semiaberto `[start, end_exclusive)` de inteiros.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct OffsetRange {
    pub start: i32,
    pub end_exclusive: i32,
}

impl OffsetRange {
    /// O construtor: `start` depois de `end_exclusive` é erro de lógica.
    pub fn new(start: i32, end_exclusive: i32) -> Self {
        if start > end_exclusive {
            errors::throw(Error::bug_indicating(Some(&format!("Invalid range: [{start}, {end_exclusive})"))));
        }
        Self { start, end_exclusive }
    }

    /// `OffsetRange.addRange(range, sortedRanges)`: insere juntando os que tocam.
    pub fn add_range(range: OffsetRange, sorted_ranges: &mut Vec<OffsetRange>) {
        let mut i = 0;
        while i < sorted_ranges.len() && sorted_ranges[i].end_exclusive < range.start {
            i += 1;
        }
        let mut j = i;
        while j < sorted_ranges.len() && sorted_ranges[j].start <= range.end_exclusive {
            j += 1;
        }
        if i == j {
            sorted_ranges.insert(i, range);
        } else {
            let start = range.start.min(sorted_ranges[i].start);
            let end = range.end_exclusive.max(sorted_ranges[j - 1].end_exclusive);
            sorted_ranges.splice(i..j, [OffsetRange::new(start, end)]);
        }
    }

    pub fn is_empty(&self) -> bool {
        self.start == self.end_exclusive
    }

    pub fn delta(&self, offset: i32) -> OffsetRange {
        OffsetRange::new(self.start + offset, self.end_exclusive + offset)
    }

    pub fn length(&self) -> i32 {
        self.end_exclusive - self.start
    }

    pub fn contains(&self, offset: i32) -> bool {
        self.start <= offset && offset < self.end_exclusive
    }

    pub fn intersection_length(&self, range: &OffsetRange) -> i32 {
        let start = self.start.max(range.start);
        let end = self.end_exclusive.min(range.end_exclusive);
        (end - start).max(0)
    }
}

impl std::fmt::Display for OffsetRange {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "[{}, {})", self.start, self.end_exclusive)
    }
}
