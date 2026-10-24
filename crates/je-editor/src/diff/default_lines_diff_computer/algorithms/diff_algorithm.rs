//! Porte de `src/vs/editor/common/diff/defaultLinesDiffComputer/algorithms/diffAlgorithm.ts`: o contrato dos algoritmos de
//! diff (`IDiffAlgorithm`, `ISequence`, `ITimeout`), o resultado (`DiffAlgorithmResult`), a diferença entre duas
//! sequências (`SequenceDiff`, `OffsetPair`) e os prazos (`InfiniteTimeout`, `DateTimeout`).
//!
//! Representação:
//! - `ISequence` é o trait [`ISequence`]: `getBoundaryScore?` (opcional no original) é o par
//!   [`ISequence::supports_boundary_score`] e [`ISequence::get_boundary_score`], porque quem usa testa se o método existe
//!   (`!sequence1.getBoundaryScore`). Os elementos são `number` no original e `i32` aqui.
//! - `IDiffAlgorithm.compute(sequence1, sequence2, timeout?)` recebe o prazo como `Option<&dyn ITimeout>` (`None` é o
//!   `InfiniteTimeout.instance` do parâmetro padrão).
//! - `Number.MAX_SAFE_INTEGER` de [`OffsetPair::MAX`] é `i32::MAX`, o maior valor que o [`OffsetRange`] representa
//!   (a convenção de `range_mapping`).
//! - `DateTimeout.disable()` troca o `isValid` do objeto por um que sempre vale; aqui o `timeout` vira `u64::MAX` e o
//!   estado volta a válido, o mesmo efeito observável.

use std::cell::Cell;

use je_base::arrays::for_each_adjacent;
use je_base::errors::{self, Error};
use je_base::runtime::date_now;

use crate::offset_range::OffsetRange;

/// `IDiffAlgorithm`: um algoritmo de diff síncrono (deve rodar num worker).
pub trait IDiffAlgorithm {
    /// `compute(sequence1, sequence2, timeout?)`.
    fn compute(&self, sequence1: &dyn ISequence, sequence2: &dyn ISequence, timeout: Option<&dyn ITimeout>) -> DiffAlgorithmResult;
}

/// `DiffAlgorithmResult`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DiffAlgorithmResult {
    pub diffs: Vec<SequenceDiff>,
    /// Indicates if the time out was reached.
    /// In that case, the diffs might be an approximation and the user should be asked to rerun the diff with more time.
    pub hit_timeout: bool,
}

impl DiffAlgorithmResult {
    pub fn trivial(seq1: &dyn ISequence, seq2: &dyn ISequence) -> DiffAlgorithmResult {
        DiffAlgorithmResult::new(
            vec![SequenceDiff::new(OffsetRange::of_length(seq1.length()), OffsetRange::of_length(seq2.length()))],
            false,
        )
    }

    pub fn trivial_timed_out(seq1: &dyn ISequence, seq2: &dyn ISequence) -> DiffAlgorithmResult {
        DiffAlgorithmResult::new(
            vec![SequenceDiff::new(OffsetRange::of_length(seq1.length()), OffsetRange::of_length(seq2.length()))],
            true,
        )
    }

    pub fn new(diffs: Vec<SequenceDiff>, hit_timeout: bool) -> Self {
        Self { diffs, hit_timeout }
    }
}

/// `SequenceDiff`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SequenceDiff {
    pub seq1_range: OffsetRange,
    pub seq2_range: OffsetRange,
}

impl SequenceDiff {
    pub fn invert(sequence_diffs: &[SequenceDiff], doc1_length: i32) -> Vec<SequenceDiff> {
        let mut result: Vec<SequenceDiff> = Vec::new();
        for_each_adjacent(sequence_diffs, |a, b| {
            result.push(SequenceDiff::from_offset_pairs(
                a.map_or(OffsetPair::ZERO, SequenceDiff::get_end_exclusives),
                b.map_or_else(
                    || {
                        OffsetPair::new(
                            doc1_length,
                            a.map_or(0, |a| a.seq2_range.end_exclusive - a.seq1_range.end_exclusive) + doc1_length,
                        )
                    },
                    SequenceDiff::get_starts,
                ),
            ));
        });
        result
    }

    pub fn from_offset_pairs(start: OffsetPair, end_exclusive: OffsetPair) -> SequenceDiff {
        SequenceDiff::new(OffsetRange::new(start.offset1, end_exclusive.offset1), OffsetRange::new(start.offset2, end_exclusive.offset2))
    }

    pub fn assert_sorted(sequence_diffs: &[SequenceDiff]) {
        let mut last: Option<&SequenceDiff> = None;
        for cur in sequence_diffs {
            if let Some(last) = last
                && !(last.seq1_range.end_exclusive <= cur.seq1_range.start && last.seq2_range.end_exclusive <= cur.seq2_range.start)
            {
                errors::throw(Error::bug_indicating(Some("Sequence diffs must be sorted")));
            }
            last = Some(cur);
        }
    }

    pub fn new(seq1_range: OffsetRange, seq2_range: OffsetRange) -> Self {
        Self { seq1_range, seq2_range }
    }

    pub fn swap(&self) -> SequenceDiff {
        SequenceDiff::new(self.seq2_range, self.seq1_range)
    }

    pub fn join(&self, other: &SequenceDiff) -> SequenceDiff {
        SequenceDiff::new(self.seq1_range.join(other.seq1_range), self.seq2_range.join(other.seq2_range))
    }

    pub fn delta(&self, offset: i32) -> SequenceDiff {
        if offset == 0 {
            return *self;
        }
        SequenceDiff::new(self.seq1_range.delta(offset), self.seq2_range.delta(offset))
    }

    pub fn delta_start(&self, offset: i32) -> SequenceDiff {
        if offset == 0 {
            return *self;
        }
        SequenceDiff::new(self.seq1_range.delta_start(offset), self.seq2_range.delta_start(offset))
    }

    pub fn delta_end(&self, offset: i32) -> SequenceDiff {
        if offset == 0 {
            return *self;
        }
        SequenceDiff::new(self.seq1_range.delta_end(offset), self.seq2_range.delta_end(offset))
    }

    pub fn intersects_or_touches(&self, other: &SequenceDiff) -> bool {
        self.seq1_range.intersects_or_touches(other.seq1_range) || self.seq2_range.intersects_or_touches(other.seq2_range)
    }

    pub fn intersect(&self, other: &SequenceDiff) -> Option<SequenceDiff> {
        let i1 = self.seq1_range.intersect(other.seq1_range)?;
        let i2 = self.seq2_range.intersect(other.seq2_range)?;
        Some(SequenceDiff::new(i1, i2))
    }

    pub fn get_starts(&self) -> OffsetPair {
        OffsetPair::new(self.seq1_range.start, self.seq2_range.start)
    }

    pub fn get_end_exclusives(&self) -> OffsetPair {
        OffsetPair::new(self.seq1_range.end_exclusive, self.seq2_range.end_exclusive)
    }
}

impl std::fmt::Display for SequenceDiff {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{} <-> {}", self.seq1_range, self.seq2_range)
    }
}

/// `OffsetPair`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct OffsetPair {
    pub offset1: i32,
    pub offset2: i32,
}

impl OffsetPair {
    pub const ZERO: OffsetPair = OffsetPair { offset1: 0, offset2: 0 };
    pub const MAX: OffsetPair = OffsetPair { offset1: i32::MAX, offset2: i32::MAX };

    pub fn new(offset1: i32, offset2: i32) -> Self {
        Self { offset1, offset2 }
    }

    pub fn delta(&self, offset: i32) -> OffsetPair {
        if offset == 0 {
            return *self;
        }
        OffsetPair::new(self.offset1 + offset, self.offset2 + offset)
    }

    pub fn equals(&self, other: &OffsetPair) -> bool {
        self.offset1 == other.offset1 && self.offset2 == other.offset2
    }
}

impl std::fmt::Display for OffsetPair {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{} <-> {}", self.offset1, self.offset2)
    }
}

/// `ISequence`.
pub trait ISequence {
    fn get_element(&self, offset: i32) -> i32;

    /// `get length()`.
    fn length(&self) -> i32;

    /// `getBoundaryScore` existe (o método é opcional no original).
    fn supports_boundary_score(&self) -> bool {
        false
    }

    /// The higher the score, the better that offset can be used to split the sequence.
    /// Is used to optimize insertions.
    /// Must not be negative.
    fn get_boundary_score(&self, _length: i32) -> i32 {
        0
    }

    /// For line sequences, getElement returns a number representing trimmed lines.
    /// This however checks equality for the original lines.
    /// It prevents shifting to less matching lines.
    fn is_strongly_equal(&self, offset1: i32, offset2: i32) -> bool;
}

/// `ITimeout`.
pub trait ITimeout {
    fn is_valid(&self) -> bool;
}

/// `InfiniteTimeout`.
pub struct InfiniteTimeout;

impl InfiniteTimeout {
    /// `InfiniteTimeout.instance`.
    pub const INSTANCE: InfiniteTimeout = InfiniteTimeout;
}

impl ITimeout for InfiniteTimeout {
    fn is_valid(&self) -> bool {
        true
    }
}

/// `DateTimeout`.
pub struct DateTimeout {
    start_time: u64,
    valid: Cell<bool>,
    timeout: Cell<f64>,
}

impl DateTimeout {
    pub fn new(timeout: f64) -> Self {
        if timeout <= 0.0 {
            errors::throw(Error::bug_indicating(Some("timeout must be positive")));
        }
        Self { start_time: date_now(), valid: Cell::new(true), timeout: Cell::new(timeout) }
    }

    /// Recommendation: Set a log-point `{this.disable()}` in the body
    pub fn disable(&self) {
        self.timeout.set(u64::MAX as f64);
        self.valid.set(true);
    }
}

impl ITimeout for DateTimeout {
    fn is_valid(&self) -> bool {
        let valid = ((date_now() - self.start_time) as f64) < self.timeout.get();
        if !valid && self.valid.get() {
            self.valid.set(false); // timeout reached
        }
        self.valid.get()
    }
}
