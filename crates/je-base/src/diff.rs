//! Porte de `src/vs/base/common/diff/diff.ts` e `diff/diffChange.ts`: o `LcsDiff` (a diferença de Myers
//! "An O(ND) Difference Algorithm and its variations", portada do C# do Visual Studio), o diff de strings
//! e a distância de Levenshtein.
//!
//! Os índices são `i32`, como no `Int32Array` do original: o algoritmo passa por posições antes do início
//! da sequência (`originalStart - 1`) e as listas de diagonais (`Int32Array`) guardam esses valores.
//!
//! Diferenças de forma:
//!
//! - `ISequence#getElements` devolve `Int32Array | number[] | string[]`; aqui é [`SequenceElements`], com
//!   `Ints` (para as duas primeiras) e `Strings`. Uma lista de strings vazia conta como lista de números,
//!   como no original (`_isStringArray` exige `length > 0`);
//! - `getStrictElement` opcional é o método com resultado `None` por padrão;
//! - os parâmetros "de saída" (`quitEarlyArr`, `midOriginalArr`, `mergedChangeArr`) são `&mut` ou valores
//!   devolvidos;
//! - a tabela de igualdade dos caracteres (`precomputedEqualityArray`, um `Uint32Array` global) é uma tabela
//!   por thread, e as operações de 32 bits do `computeLevenshteinDistance` seguem as do JavaScript (deslocamento
//!   com contagem módulo 32, soma que dá a volta), inclusive onde o original usa `1 << k` com `k >= 32`.

use std::cell::RefCell;
use std::rc::Rc;

use crate::errors::{self, Error};
use crate::hash::string_hash;
use crate::js_string::{JsString, is_js_whitespace};

/// `Constants.MAX_SAFE_SMALL_INTEGER` e `Constants.MIN_SAFE_SMALL_INTEGER` de `uint.ts`.
const MAX_SAFE_SMALL_INTEGER: i32 = 1 << 30;
const MIN_SAFE_SMALL_INTEGER: i32 = -(1 << 30);

/// Represents information about a specific difference between two sequences (`DiffChange` e `IDiffChange`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DiffChange {
    /// The position of the first element in the original sequence which this change affects.
    pub original_start: i32,
    /// The number of elements from the original sequence which were affected.
    pub original_length: i32,
    /// The position of the first element in the modified sequence which this change affects.
    pub modified_start: i32,
    /// The number of elements from the modified sequence which were affected (added).
    pub modified_length: i32,
}

impl DiffChange {
    /// Constructs a new DiffChange with the given sequence information and content.
    pub fn new(original_start: i32, original_length: i32, modified_start: i32, modified_length: i32) -> Self {
        Self { original_start, original_length, modified_start, modified_length }
    }

    /// The end point (exclusive) of the change in the original sequence.
    pub fn get_original_end(&self) -> i32 {
        self.original_start + self.original_length
    }

    /// The end point (exclusive) of the change in the modified sequence.
    pub fn get_modified_end(&self) -> i32 {
        self.modified_start + self.modified_length
    }
}

/// O que `ISequence#getElements` devolve.
pub enum SequenceElements {
    /// `Int32Array` ou `number[]`.
    Ints(Vec<i32>),
    /// `string[]`.
    Strings(Vec<JsString>),
}

/// `ISequence`.
pub trait ISequence {
    fn get_elements(&self) -> SequenceElements;

    /// `getStrictElement?(index)`.
    fn get_strict_element(&self, _index: usize) -> Option<JsString> {
        None
    }
}

pub struct StringDiffSequence {
    source: JsString,
}

impl StringDiffSequence {
    pub fn new(source: JsString) -> Self {
        Self { source }
    }
}

impl ISequence for StringDiffSequence {
    fn get_elements(&self) -> SequenceElements {
        SequenceElements::Ints(self.source.units().iter().map(|&unit| i32::from(unit)).collect())
    }
}

pub fn string_diff(original: JsString, modified: JsString, pretty: bool) -> Vec<DiffChange> {
    LcsDiff::new(Rc::new(StringDiffSequence::new(original)), Rc::new(StringDiffSequence::new(modified)), None).compute_diff(pretty).changes
}

/// `IContinueProcessingPredicate`: `(furthestOriginalIndex, matchLengthOfLongest) => boolean`.
pub type ContinueProcessingPredicate = Box<dyn Fn(i32, f64) -> bool>;

/// `IDiffResult`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DiffResult {
    pub quit_early: bool,
    pub changes: Vec<DiffChange>,
}

//
// The code below has been ported from a C# implementation in VS
//

fn assert(condition: bool, message: &str) {
    if !condition {
        errors::throw(Error::new(message));
    }
}

//*****************************************************************************
// LcsDiff.cs
//
// An implementation of the difference algorithm described in
// "An O(ND) Difference Algorithm and its variations" by Eugene W. Myers
//
// Copyright (C) 2008 Microsoft Corporation @minifier_do_not_preserve
//*****************************************************************************

// Our total memory usage for storing history is (worst-case):
// 2 * [(MaxDifferencesHistory + 1) * (MaxDifferencesHistory + 1) - 1] * sizeof(int)
// 2 * [1448*1448 - 1] * 4 = 16773624 = 16MB
const MAX_DIFFERENCES_HISTORY: i32 = 1447;

/// A utility class which helps to create the set of DiffChanges from
/// a difference operation. This class accepts original DiffElements and
/// modified DiffElements that are involved in a particular change. The
/// MarkNextChange() method can be called to mark the separation between
/// distinct changes. At the end, the Changes property can be called to retrieve
/// the constructed changes.
struct DiffChangeHelper {
    changes: Vec<DiffChange>,
    original_start: i32,
    modified_start: i32,
    original_count: i32,
    modified_count: i32,
}

impl DiffChangeHelper {
    fn new() -> Self {
        Self {
            changes: Vec::new(),
            original_start: MAX_SAFE_SMALL_INTEGER,
            modified_start: MAX_SAFE_SMALL_INTEGER,
            original_count: 0,
            modified_count: 0,
        }
    }

    /// Marks the beginning of the next change in the set of differences.
    fn mark_next_change(&mut self) {
        // Only add to the list if there is something to add
        if self.original_count > 0 || self.modified_count > 0 {
            // Add the new change to our list
            self.changes.push(DiffChange::new(self.original_start, self.original_count, self.modified_start, self.modified_count));
        }

        // Reset for the next change
        self.original_count = 0;
        self.modified_count = 0;
        self.original_start = MAX_SAFE_SMALL_INTEGER;
        self.modified_start = MAX_SAFE_SMALL_INTEGER;
    }

    /// Adds the original element at the given position to the elements affected by the current change. The
    /// modified index gives context to the change position with respect to the original sequence.
    fn add_original_element(&mut self, original_index: i32, modified_index: i32) {
        // The 'true' start index is the smallest of the ones we've seen
        self.original_start = self.original_start.min(original_index);
        self.modified_start = self.modified_start.min(modified_index);

        self.original_count += 1;
    }

    /// Adds the modified element at the given position to the elements affected by the current change. The
    /// original index gives context to the change position with respect to the modified sequence.
    fn add_modified_element(&mut self, original_index: i32, modified_index: i32) {
        // The 'true' start index is the smallest of the ones we've seen
        self.original_start = self.original_start.min(original_index);
        self.modified_start = self.modified_start.min(modified_index);

        self.modified_count += 1;
    }

    /// Retrieves all of the changes marked by the class.
    fn get_changes(mut self) -> Vec<DiffChange> {
        if self.original_count > 0 || self.modified_count > 0 {
            // Finish up on whatever is left
            self.mark_next_change();
        }

        self.changes
    }

    /// Retrieves all of the changes marked by the class in the reverse order
    fn get_reverse_changes(mut self) -> Vec<DiffChange> {
        if self.original_count > 0 || self.modified_count > 0 {
            // Finish up on whatever is left
            self.mark_next_change();
        }

        self.changes.reverse();
        self.changes
    }
}

fn index(i: i32) -> usize {
    usize::try_from(i).expect("índice de diagonal negativo")
}

/// An implementation of the difference algorithm described in
/// "An O(ND) Difference Algorithm and its variations" by Eugene W. Myers
pub struct LcsDiff {
    continue_processing_predicate: Option<ContinueProcessingPredicate>,

    original_sequence: Rc<dyn ISequence>,
    modified_sequence: Rc<dyn ISequence>,
    has_strings: bool,
    original_string_elements: Vec<JsString>,
    original_elements_or_hash: Vec<i32>,
    modified_string_elements: Vec<JsString>,
    modified_elements_or_hash: Vec<i32>,

    forward_history: Vec<Vec<i32>>,
    reverse_history: Vec<Vec<i32>>,
}

impl LcsDiff {
    /// Constructs the DiffFinder
    pub fn new(original_sequence: Rc<dyn ISequence>, modified_sequence: Rc<dyn ISequence>, continue_processing_predicate: Option<ContinueProcessingPredicate>) -> Self {
        let (original_string_elements, original_elements_or_hash, original_has_strings) = Self::get_elements(&*original_sequence);
        let (modified_string_elements, modified_elements_or_hash, modified_has_strings) = Self::get_elements(&*modified_sequence);

        Self {
            continue_processing_predicate,
            original_sequence,
            modified_sequence,
            has_strings: original_has_strings && modified_has_strings,
            original_string_elements,
            original_elements_or_hash,
            modified_string_elements,
            modified_elements_or_hash,
            forward_history: Vec::new(),
            reverse_history: Vec::new(),
        }
    }

    fn get_elements(sequence: &dyn ISequence) -> (Vec<JsString>, Vec<i32>, bool) {
        match sequence.get_elements() {
            SequenceElements::Strings(elements) if !elements.is_empty() => {
                let hashes = elements.iter().map(|element| string_hash(element.units(), 0)).collect();
                (elements, hashes, true)
            }
            SequenceElements::Strings(_) => (Vec::new(), Vec::new(), false),
            SequenceElements::Ints(elements) => (Vec::new(), elements, false),
        }
    }

    fn elements_are_equal(&self, original_index: i32, new_index: i32) -> bool {
        let (o, n) = (index(original_index), index(new_index));
        if self.original_elements_or_hash[o] != self.modified_elements_or_hash[n] {
            return false;
        }
        if self.has_strings { self.original_string_elements[o] == self.modified_string_elements[n] } else { true }
    }

    fn elements_are_strict_equal(&self, original_index: i32, new_index: i32) -> bool {
        if !self.elements_are_equal(original_index, new_index) {
            return false;
        }
        let original_element = self.original_sequence.get_strict_element(index(original_index));
        let modified_element = self.modified_sequence.get_strict_element(index(new_index));
        original_element == modified_element
    }

    fn original_elements_are_equal(&self, index1: i32, index2: i32) -> bool {
        let (a, b) = (index(index1), index(index2));
        if self.original_elements_or_hash[a] != self.original_elements_or_hash[b] {
            return false;
        }
        if self.has_strings { self.original_string_elements[a] == self.original_string_elements[b] } else { true }
    }

    fn modified_elements_are_equal(&self, index1: i32, index2: i32) -> bool {
        let (a, b) = (index(index1), index(index2));
        if self.modified_elements_or_hash[a] != self.modified_elements_or_hash[b] {
            return false;
        }
        if self.has_strings { self.modified_string_elements[a] == self.modified_string_elements[b] } else { true }
    }

    pub fn compute_diff(&mut self, pretty: bool) -> DiffResult {
        let original_end = self.original_elements_or_hash.len() as i32 - 1;
        let modified_end = self.modified_elements_or_hash.len() as i32 - 1;
        self.compute_diff_range(0, original_end, 0, modified_end, pretty)
    }

    /// Computes the differences between the original and modified input sequences on the bounded range
    /// (all indexes inclusive).
    fn compute_diff_range(&mut self, original_start: i32, original_end: i32, modified_start: i32, modified_end: i32, pretty: bool) -> DiffResult {
        let mut quit_early = false;
        let mut changes = self.compute_diff_recursive(original_start, original_end, modified_start, modified_end, &mut quit_early);

        if pretty {
            // We have to clean up the computed diff to be more intuitive
            // but it turns out this cannot be done correctly until the entire set
            // of diffs have been computed
            changes = self.prettify_changes(changes);
        }

        DiffResult { quit_early, changes }
    }

    /// Private helper method which computes the differences on the bounded range recursively.
    fn compute_diff_recursive(&mut self, mut original_start: i32, mut original_end: i32, mut modified_start: i32, mut modified_end: i32, quit_early: &mut bool) -> Vec<DiffChange> {
        *quit_early = false;

        // Find the start of the differences
        while original_start <= original_end && modified_start <= modified_end && self.elements_are_equal(original_start, modified_start) {
            original_start += 1;
            modified_start += 1;
        }

        // Find the end of the differences
        while original_end >= original_start && modified_end >= modified_start && self.elements_are_equal(original_end, modified_end) {
            original_end -= 1;
            modified_end -= 1;
        }

        // In the special case where we either have all insertions or all deletions or the sequences are identical
        if original_start > original_end || modified_start > modified_end {
            let changes = if modified_start <= modified_end {
                assert(original_start == original_end + 1, "originalStart should only be one more than originalEnd");

                // All insertions
                vec![DiffChange::new(original_start, 0, modified_start, modified_end - modified_start + 1)]
            } else if original_start <= original_end {
                assert(modified_start == modified_end + 1, "modifiedStart should only be one more than modifiedEnd");

                // All deletions
                vec![DiffChange::new(original_start, original_end - original_start + 1, modified_start, 0)]
            } else {
                assert(original_start == original_end + 1, "originalStart should only be one more than originalEnd");
                assert(modified_start == modified_end + 1, "modifiedStart should only be one more than modifiedEnd");

                // Identical sequences - No differences
                Vec::new()
            };

            return changes;
        }

        // This problem can be solved using the Divide-And-Conquer technique.
        let (mut mid_original, mut mid_modified) = (0, 0);
        let result = self.compute_recursion_point(original_start, original_end, modified_start, modified_end, &mut mid_original, &mut mid_modified, quit_early);

        if let Some(result) = result {
            // Result is not-null when there was enough memory to compute the changes while
            // searching for the recursion point
            return result;
        } else if !*quit_early {
            // We can break the problem down recursively by finding the changes in the
            // First Half:   (originalStart, modifiedStart) to (midOriginal, midModified)
            // Second Half:  (midOriginal + 1, minModified + 1) to (originalEnd, modifiedEnd)
            // NOTE: ComputeDiff() is inclusive, therefore the second range starts on the next point

            let left_changes = self.compute_diff_recursive(original_start, mid_original, modified_start, mid_modified, quit_early);
            let right_changes = if !*quit_early {
                self.compute_diff_recursive(mid_original + 1, original_end, mid_modified + 1, modified_end, quit_early)
            } else {
                // We didn't have time to finish the first half, so we don't have time to compute this half.
                // Consider the entire rest of the sequence different.
                vec![DiffChange::new(mid_original + 1, original_end - (mid_original + 1) + 1, mid_modified + 1, modified_end - (mid_modified + 1) + 1)]
            };

            return self.concatenate_changes(left_changes, right_changes);
        }

        // If we hit here, we quit early, and so can't return anything meaningful
        vec![DiffChange::new(original_start, original_end - original_start + 1, modified_start, modified_end - modified_start + 1)]
    }

    #[allow(clippy::too_many_arguments)]
    fn walk_trace<'a>(
        &'a self,
        mut diagonal_forward_base: i32,
        diagonal_forward_start: i32,
        diagonal_forward_end: i32,
        diagonal_forward_offset: i32,
        mut diagonal_reverse_base: i32,
        diagonal_reverse_start: i32,
        diagonal_reverse_end: i32,
        diagonal_reverse_offset: i32,
        mut forward_points: &'a [i32],
        mut reverse_points: &'a [i32],
        original_end: i32,
        mid_original: i32,
        modified_end: i32,
        mid_modified: i32,
        delta_is_even: bool,
        quit_early: bool,
    ) -> Vec<DiffChange> {
        // First, walk backward through the forward diagonals history
        let mut change_helper = DiffChangeHelper::new();
        let mut diagonal_min = diagonal_forward_start;
        let mut diagonal_max = diagonal_forward_end;
        let mut diagonal_relative = (mid_original - mid_modified) - diagonal_forward_offset;
        let mut last_original_index = MIN_SAFE_SMALL_INTEGER;
        let mut history_index = self.forward_history.len() as i32 - 1;

        loop {
            // Get the diagonal index from the relative diagonal number
            let diagonal = diagonal_relative + diagonal_forward_base;

            // Figure out where we came from
            if diagonal == diagonal_min || (diagonal < diagonal_max && forward_points[index(diagonal - 1)] < forward_points[index(diagonal + 1)]) {
                // Vertical line (the element is an insert)
                let original_index = forward_points[index(diagonal + 1)];
                let modified_index = original_index - diagonal_relative - diagonal_forward_offset;
                if original_index < last_original_index {
                    change_helper.mark_next_change();
                }
                last_original_index = original_index;
                change_helper.add_modified_element(original_index + 1, modified_index);
                diagonal_relative = (diagonal + 1) - diagonal_forward_base; //Setup for the next iteration
            } else {
                // Horizontal line (the element is a deletion)
                let original_index = forward_points[index(diagonal - 1)] + 1;
                let modified_index = original_index - diagonal_relative - diagonal_forward_offset;
                if original_index < last_original_index {
                    change_helper.mark_next_change();
                }
                last_original_index = original_index - 1;
                change_helper.add_original_element(original_index, modified_index + 1);
                diagonal_relative = (diagonal - 1) - diagonal_forward_base; //Setup for the next iteration
            }

            if history_index >= 0 {
                forward_points = &self.forward_history[index(history_index)];
                diagonal_forward_base = forward_points[0]; //We stored this in the first spot
                diagonal_min = 1;
                diagonal_max = forward_points.len() as i32 - 1;
            }

            history_index -= 1;
            if history_index < -1 {
                break;
            }
        }

        // Ironically, we get the forward changes as the reverse of the
        // order we added them since we technically added them backwards
        let forward_changes = change_helper.get_reverse_changes();

        let reverse_changes = if quit_early {
            // TODO: Calculate a partial from the reverse diagonals.
            //       For now, just assume everything after the midOriginal/midModified point is a diff

            let mut original_start_point = mid_original + 1;
            let mut modified_start_point = mid_modified + 1;

            if let Some(last_forward_change) = forward_changes.last() {
                original_start_point = original_start_point.max(last_forward_change.get_original_end());
                modified_start_point = modified_start_point.max(last_forward_change.get_modified_end());
            }

            vec![DiffChange::new(
                original_start_point,
                original_end - original_start_point + 1,
                modified_start_point,
                modified_end - modified_start_point + 1,
            )]
        } else {
            // Now walk backward through the reverse diagonals history
            change_helper = DiffChangeHelper::new();
            diagonal_min = diagonal_reverse_start;
            diagonal_max = diagonal_reverse_end;
            diagonal_relative = (mid_original - mid_modified) - diagonal_reverse_offset;
            last_original_index = MAX_SAFE_SMALL_INTEGER;
            history_index = if delta_is_even { self.reverse_history.len() as i32 - 1 } else { self.reverse_history.len() as i32 - 2 };

            loop {
                // Get the diagonal index from the relative diagonal number
                let diagonal = diagonal_relative + diagonal_reverse_base;

                // Figure out where we came from
                if diagonal == diagonal_min || (diagonal < diagonal_max && reverse_points[index(diagonal - 1)] >= reverse_points[index(diagonal + 1)]) {
                    // Horizontal line (the element is a deletion))
                    let original_index = reverse_points[index(diagonal + 1)] - 1;
                    let modified_index = original_index - diagonal_relative - diagonal_reverse_offset;
                    if original_index > last_original_index {
                        change_helper.mark_next_change();
                    }
                    last_original_index = original_index + 1;
                    change_helper.add_original_element(original_index + 1, modified_index + 1);
                    diagonal_relative = (diagonal + 1) - diagonal_reverse_base; //Setup for the next iteration
                } else {
                    // Vertical line (the element is an insertion)
                    let original_index = reverse_points[index(diagonal - 1)];
                    let modified_index = original_index - diagonal_relative - diagonal_reverse_offset;
                    if original_index > last_original_index {
                        change_helper.mark_next_change();
                    }
                    last_original_index = original_index;
                    change_helper.add_modified_element(original_index + 1, modified_index + 1);
                    diagonal_relative = (diagonal - 1) - diagonal_reverse_base; //Setup for the next iteration
                }

                if history_index >= 0 {
                    reverse_points = &self.reverse_history[index(history_index)];
                    diagonal_reverse_base = reverse_points[0]; //We stored this in the first spot
                    diagonal_min = 1;
                    diagonal_max = reverse_points.len() as i32 - 1;
                }

                history_index -= 1;
                if history_index < -1 {
                    break;
                }
            }

            // There are cases where the reverse history will find diffs that
            // are correct, but not intuitive, so we need shift them.
            change_helper.get_changes()
        };

        self.concatenate_changes(forward_changes, reverse_changes)
    }

    /// Given the range to compute the diff on, this method finds the point:
    /// (midOriginal, midModified)
    /// that exists in the middle of the LCS of the two sequences and
    /// is the point at which the LCS problem may be broken down recursively.
    /// This method will try to keep the LCS trace in memory. If the LCS recursion
    /// point is calculated and the full trace is available in memory, then this method
    /// will return the change list.
    ///
    /// Devolve as mudanças, se disponíveis, ou `None`; o ponto vai em `mid_original` e `mid_modified`.
    #[allow(clippy::too_many_arguments)]
    fn compute_recursion_point(
        &mut self,
        mut original_start: i32,
        original_end: i32,
        mut modified_start: i32,
        modified_end: i32,
        mid_original: &mut i32,
        mid_modified: &mut i32,
        quit_early: &mut bool,
    ) -> Option<Vec<DiffChange>> {
        let (mut original_index, mut modified_index);
        let (mut diagonal_forward_start, mut diagonal_forward_end) = (0, 0);
        let (mut diagonal_reverse_start, mut diagonal_reverse_end) = (0, 0);

        // To traverse the edit graph and produce the proper LCS, our actual
        // start position is just outside the given boundary
        original_start -= 1;
        modified_start -= 1;

        // We set these up to make the compiler happy, but they will
        // be replaced before we return with the actual recursion point
        *mid_original = 0;
        *mid_modified = 0;

        // Clear out the history
        self.forward_history = Vec::new();
        self.reverse_history = Vec::new();

        // Each cell in the two arrays corresponds to a diagonal in the edit graph.
        // The integer value in the cell represents the originalIndex of the furthest
        // reaching point found so far that ends in that diagonal.
        // The modifiedIndex can be computed mathematically from the originalIndex and the diagonal number.
        let max_differences = (original_end - original_start) + (modified_end - modified_start);
        let num_diagonals = max_differences + 1;
        let mut forward_points = vec![0i32; index(num_diagonals)];
        let mut reverse_points = vec![0i32; index(num_diagonals)];
        // diagonalForwardBase: Index into forwardPoints of the diagonal which passes through (originalStart, modifiedStart)
        // diagonalReverseBase: Index into reversePoints of the diagonal which passes through (originalEnd, modifiedEnd)
        let diagonal_forward_base = modified_end - modified_start;
        let diagonal_reverse_base = original_end - original_start;
        // diagonalForwardOffset: Geometric offset which allows modifiedIndex to be computed from originalIndex and the
        //    diagonal number (relative to diagonalForwardBase)
        // diagonalReverseOffset: Geometric offset which allows modifiedIndex to be computed from originalIndex and the
        //    diagonal number (relative to diagonalReverseBase)
        let diagonal_forward_offset = original_start - modified_start;
        let diagonal_reverse_offset = original_end - modified_end;

        // delta: The difference between the end diagonal and the start diagonal. This is used to relate diagonal numbers
        //   relative to the start diagonal with diagonal numbers relative to the end diagonal.
        // The Even/Oddn-ness of this delta is important for determining when we should check for overlap
        let delta = diagonal_reverse_base - diagonal_forward_base;
        let delta_is_even = delta % 2 == 0;

        // Here we set up the start and end points as the furthest points found so far
        // in both the forward and reverse directions, respectively
        forward_points[index(diagonal_forward_base)] = original_start;
        reverse_points[index(diagonal_reverse_base)] = original_end;

        // Remember if we quit early, and thus need to do a best-effort result instead of a real result.
        *quit_early = false;

        // A couple of points:
        // --With this method, we iterate on the number of differences between the two sequences.
        //   The more differences there actually are, the longer this will take.
        // --Also, as the number of differences increases, we have to search on diagonals further
        //   away from the reference diagonal (which is diagonalForwardBase for forward, diagonalReverseBase for reverse).
        // --We extend on even diagonals (relative to the reference diagonal) only when numDifferences
        //   is even and odd diagonals only when numDifferences is odd.
        let mut num_differences = 1;
        while f64::from(num_differences) <= (f64::from(max_differences) / 2.0) + 1.0 {
            let mut furthest_original_index = 0;
            let mut furthest_modified_index = 0;

            // Run the algorithm in the forward direction
            diagonal_forward_start = Self::clip_diagonal_bound(diagonal_forward_base - num_differences, num_differences, diagonal_forward_base, num_diagonals);
            diagonal_forward_end = Self::clip_diagonal_bound(diagonal_forward_base + num_differences, num_differences, diagonal_forward_base, num_diagonals);
            let mut diagonal = diagonal_forward_start;
            while diagonal <= diagonal_forward_end {
                // STEP 1: We extend the furthest reaching point in the present diagonal
                // by looking at the diagonals above and below and picking the one whose point
                // is further away from the start point (originalStart, modifiedStart)
                if diagonal == diagonal_forward_start || (diagonal < diagonal_forward_end && forward_points[index(diagonal - 1)] < forward_points[index(diagonal + 1)]) {
                    original_index = forward_points[index(diagonal + 1)];
                } else {
                    original_index = forward_points[index(diagonal - 1)] + 1;
                }
                modified_index = original_index - (diagonal - diagonal_forward_base) - diagonal_forward_offset;

                // Save the current originalIndex so we can test for false overlap in step 3
                let temp_original_index = original_index;

                // STEP 2: We can continue to extend the furthest reaching point in the present diagonal
                // so long as the elements are equal.
                while original_index < original_end && modified_index < modified_end && self.elements_are_equal(original_index + 1, modified_index + 1) {
                    original_index += 1;
                    modified_index += 1;
                }
                forward_points[index(diagonal)] = original_index;

                if original_index + modified_index > furthest_original_index + furthest_modified_index {
                    furthest_original_index = original_index;
                    furthest_modified_index = modified_index;
                }

                // STEP 3: If delta is odd (overlap first happens on forward when delta is odd)
                // and diagonal is in the range of reverse diagonals computed for numDifferences-1
                // (the previous iteration; we haven't computed reverse diagonals for numDifferences yet)
                // then check for overlap.
                if !delta_is_even && (diagonal - diagonal_reverse_base).abs() <= (num_differences - 1) && original_index >= reverse_points[index(diagonal)] {
                    *mid_original = original_index;
                    *mid_modified = modified_index;

                    if temp_original_index <= reverse_points[index(diagonal)] && MAX_DIFFERENCES_HISTORY > 0 && num_differences <= (MAX_DIFFERENCES_HISTORY + 1) {
                        // BINGO! We overlapped, and we have the full trace in memory!
                        return Some(self.walk_trace(
                            diagonal_forward_base,
                            diagonal_forward_start,
                            diagonal_forward_end,
                            diagonal_forward_offset,
                            diagonal_reverse_base,
                            diagonal_reverse_start,
                            diagonal_reverse_end,
                            diagonal_reverse_offset,
                            &forward_points,
                            &reverse_points,
                            original_end,
                            *mid_original,
                            modified_end,
                            *mid_modified,
                            delta_is_even,
                            *quit_early,
                        ));
                    }
                    // Either false overlap, or we didn't have enough memory for the full trace
                    // Just return the recursion point
                    return None;
                }

                diagonal += 2;
            }

            // Check to see if we should be quitting early, before moving on to the next iteration.
            let match_length_of_longest = f64::from((furthest_original_index - original_start) + (furthest_modified_index - modified_start) - num_differences) / 2.0;

            if let Some(predicate) = &self.continue_processing_predicate
                && !predicate(furthest_original_index, match_length_of_longest)
            {
                // We can't finish, so skip ahead to generating a result from what we have.
                *quit_early = true;

                // Use the furthest distance we got in the forward direction.
                *mid_original = furthest_original_index;
                *mid_modified = furthest_modified_index;

                if match_length_of_longest > 0.0 && MAX_DIFFERENCES_HISTORY > 0 && num_differences <= (MAX_DIFFERENCES_HISTORY + 1) {
                    // Enough of the history is in memory to walk it backwards
                    return Some(self.walk_trace(
                        diagonal_forward_base,
                        diagonal_forward_start,
                        diagonal_forward_end,
                        diagonal_forward_offset,
                        diagonal_reverse_base,
                        diagonal_reverse_start,
                        diagonal_reverse_end,
                        diagonal_reverse_offset,
                        &forward_points,
                        &reverse_points,
                        original_end,
                        *mid_original,
                        modified_end,
                        *mid_modified,
                        delta_is_even,
                        *quit_early,
                    ));
                }
                // We didn't actually remember enough of the history.

                //Since we are quitting the diff early, we need to shift back the originalStart and modified start
                //back into the boundary limits since we decremented their value above beyond the boundary limit.
                original_start += 1;
                modified_start += 1;

                return Some(vec![DiffChange::new(
                    original_start,
                    original_end - original_start + 1,
                    modified_start,
                    modified_end - modified_start + 1,
                )]);
            }

            // Run the algorithm in the reverse direction
            diagonal_reverse_start = Self::clip_diagonal_bound(diagonal_reverse_base - num_differences, num_differences, diagonal_reverse_base, num_diagonals);
            diagonal_reverse_end = Self::clip_diagonal_bound(diagonal_reverse_base + num_differences, num_differences, diagonal_reverse_base, num_diagonals);
            let mut diagonal = diagonal_reverse_start;
            while diagonal <= diagonal_reverse_end {
                // STEP 1: We extend the furthest reaching point in the present diagonal
                // by looking at the diagonals above and below and picking the one whose point
                // is further away from the start point (originalEnd, modifiedEnd)
                if diagonal == diagonal_reverse_start || (diagonal < diagonal_reverse_end && reverse_points[index(diagonal - 1)] >= reverse_points[index(diagonal + 1)]) {
                    original_index = reverse_points[index(diagonal + 1)] - 1;
                } else {
                    original_index = reverse_points[index(diagonal - 1)];
                }
                modified_index = original_index - (diagonal - diagonal_reverse_base) - diagonal_reverse_offset;

                // Save the current originalIndex so we can test for false overlap
                let temp_original_index = original_index;

                // STEP 2: We can continue to extend the furthest reaching point in the present diagonal
                // as long as the elements are equal.
                while original_index > original_start && modified_index > modified_start && self.elements_are_equal(original_index, modified_index) {
                    original_index -= 1;
                    modified_index -= 1;
                }
                reverse_points[index(diagonal)] = original_index;

                // STEP 4: If delta is even (overlap first happens on reverse when delta is even)
                // and diagonal is in the range of forward diagonals computed for numDifferences
                // then check for overlap.
                if delta_is_even && (diagonal - diagonal_forward_base).abs() <= num_differences && original_index <= forward_points[index(diagonal)] {
                    *mid_original = original_index;
                    *mid_modified = modified_index;

                    if temp_original_index >= forward_points[index(diagonal)] && MAX_DIFFERENCES_HISTORY > 0 && num_differences <= (MAX_DIFFERENCES_HISTORY + 1) {
                        // BINGO! We overlapped, and we have the full trace in memory!
                        return Some(self.walk_trace(
                            diagonal_forward_base,
                            diagonal_forward_start,
                            diagonal_forward_end,
                            diagonal_forward_offset,
                            diagonal_reverse_base,
                            diagonal_reverse_start,
                            diagonal_reverse_end,
                            diagonal_reverse_offset,
                            &forward_points,
                            &reverse_points,
                            original_end,
                            *mid_original,
                            modified_end,
                            *mid_modified,
                            delta_is_even,
                            *quit_early,
                        ));
                    }
                    // Either false overlap, or we didn't have enough memory for the full trace
                    // Just return the recursion point
                    return None;
                }

                diagonal += 2;
            }

            // Save current vectors to history before the next iteration
            if num_differences <= MAX_DIFFERENCES_HISTORY {
                // We are allocating space for one extra int, which we fill with
                // the index of the diagonal base index
                let mut temp = vec![0i32; index(diagonal_forward_end - diagonal_forward_start + 2)];
                temp[0] = diagonal_forward_base - diagonal_forward_start + 1;
                temp[1..].copy_from_slice(&forward_points[index(diagonal_forward_start)..=index(diagonal_forward_end)]);
                self.forward_history.push(temp);

                let mut temp = vec![0i32; index(diagonal_reverse_end - diagonal_reverse_start + 2)];
                temp[0] = diagonal_reverse_base - diagonal_reverse_start + 1;
                temp[1..].copy_from_slice(&reverse_points[index(diagonal_reverse_start)..=index(diagonal_reverse_end)]);
                self.reverse_history.push(temp);
            }

            num_differences += 1;
        }

        // If we got here, then we have the full trace in history. We just have to convert it to a change list
        // NOTE: This part is a bit messy
        Some(self.walk_trace(
            diagonal_forward_base,
            diagonal_forward_start,
            diagonal_forward_end,
            diagonal_forward_offset,
            diagonal_reverse_base,
            diagonal_reverse_start,
            diagonal_reverse_end,
            diagonal_reverse_offset,
            &forward_points,
            &reverse_points,
            original_end,
            *mid_original,
            modified_end,
            *mid_modified,
            delta_is_even,
            *quit_early,
        ))
    }

    /// Shifts the given changes to provide a more intuitive diff.
    /// While the first element in a diff matches the first element after the diff,
    /// we shift the diff down.
    fn prettify_changes(&self, mut changes: Vec<DiffChange>) -> Vec<DiffChange> {
        // Shift all the changes down first
        let mut i: i64 = 0;
        while (i as usize) < changes.len() {
            let iu = i as usize;
            let mut change = changes[iu];
            let (original_stop, modified_stop) = if iu < changes.len() - 1 {
                (changes[iu + 1].original_start, changes[iu + 1].modified_start)
            } else {
                (self.original_elements_or_hash.len() as i32, self.modified_elements_or_hash.len() as i32)
            };
            let check_original = change.original_length > 0;
            let check_modified = change.modified_length > 0;

            while change.original_start + change.original_length < original_stop
                && change.modified_start + change.modified_length < modified_stop
                && (!check_original || self.original_elements_are_equal(change.original_start, change.original_start + change.original_length))
                && (!check_modified || self.modified_elements_are_equal(change.modified_start, change.modified_start + change.modified_length))
            {
                let start_strict_equal = self.elements_are_strict_equal(change.original_start, change.modified_start);
                let end_strict_equal = self.elements_are_strict_equal(change.original_start + change.original_length, change.modified_start + change.modified_length);
                if end_strict_equal && !start_strict_equal {
                    // moving the change down would create an equal change, but the elements are not strict equal
                    break;
                }
                change.original_start += 1;
                change.modified_start += 1;
            }
            changes[iu] = change;

            if iu < changes.len() - 1
                && let Some(merged) = Self::changes_overlap(&changes[iu], &changes[iu + 1])
            {
                changes[iu] = merged;
                changes.remove(iu + 1);
                // `i--` seguido do `i++` do laço: reavalia o mesmo índice.
                continue;
            }
            i += 1;
        }

        // Shift changes back up until we hit empty or whitespace-only lines
        let mut i = changes.len() as i64 - 1;
        while i >= 0 {
            let iu = i as usize;
            let mut change = changes[iu];

            let (mut original_stop, mut modified_stop) = (0, 0);
            if iu > 0 {
                let prev_change = changes[iu - 1];
                original_stop = prev_change.original_start + prev_change.original_length;
                modified_stop = prev_change.modified_start + prev_change.modified_length;
            }

            let check_original = change.original_length > 0;
            let check_modified = change.modified_length > 0;

            let mut best_delta = 0;
            let mut best_score = self.boundary_score(change.original_start, change.original_length, change.modified_start, change.modified_length);

            let mut delta = 1;
            loop {
                let original_start = change.original_start - delta;
                let modified_start = change.modified_start - delta;

                if original_start < original_stop || modified_start < modified_stop {
                    break;
                }

                if check_original && !self.original_elements_are_equal(original_start, original_start + change.original_length) {
                    break;
                }

                if check_modified && !self.modified_elements_are_equal(modified_start, modified_start + change.modified_length) {
                    break;
                }

                let touching_previous_change = original_start == original_stop && modified_start == modified_stop;
                let score = (if touching_previous_change { 5 } else { 0 })
                    + self.boundary_score(original_start, change.original_length, modified_start, change.modified_length);

                if score > best_score {
                    best_score = score;
                    best_delta = delta;
                }
                delta += 1;
            }

            change.original_start -= best_delta;
            change.modified_start -= best_delta;
            changes[iu] = change;

            if iu > 0
                && let Some(merged) = Self::changes_overlap(&changes[iu - 1], &changes[iu])
            {
                changes[iu - 1] = merged;
                changes.remove(iu);
                // `i++` seguido do `i--` do laço: reavalia o mesmo índice, que agora é o da fusão.
                continue;
            }
            i -= 1;
        }

        // There could be multiple longest common substrings.
        // Give preference to the ones containing longer lines
        if self.has_strings {
            for i in 1..changes.len() {
                let a_change = changes[i - 1];
                let b_change = changes[i];
                let matched_length = b_change.original_start - a_change.original_start - a_change.original_length;
                let a_original_start = a_change.original_start;
                let b_original_end = b_change.original_start + b_change.original_length;
                let ab_original_length = b_original_end - a_original_start;
                let a_modified_start = a_change.modified_start;
                let b_modified_end = b_change.modified_start + b_change.modified_length;
                let ab_modified_length = b_modified_end - a_modified_start;
                // Avoid wasting a lot of time with these searches
                if matched_length < 5
                    && ab_original_length < 20
                    && ab_modified_length < 20
                    && let Some((original_match_start, modified_match_start)) =
                        self.find_better_contiguous_sequence(a_original_start, ab_original_length, a_modified_start, ab_modified_length, matched_length)
                    && (original_match_start != a_change.original_start + a_change.original_length
                        || modified_match_start != a_change.modified_start + a_change.modified_length)
                {
                    // switch to another sequence that has a better score
                    let a = &mut changes[i - 1];
                    a.original_length = original_match_start - a.original_start;
                    a.modified_length = modified_match_start - a.modified_start;
                    let b = &mut changes[i];
                    b.original_start = original_match_start + matched_length;
                    b.modified_start = modified_match_start + matched_length;
                    b.original_length = b_original_end - b.original_start;
                    b.modified_length = b_modified_end - b.modified_start;
                }
            }
        }

        changes
    }

    fn find_better_contiguous_sequence(&self, original_start: i32, original_length: i32, modified_start: i32, modified_length: i32, desired_length: i32) -> Option<(i32, i32)> {
        if original_length < desired_length || modified_length < desired_length {
            return None;
        }
        let original_max = original_start + original_length - desired_length + 1;
        let modified_max = modified_start + modified_length - desired_length + 1;
        let mut best_score = 0;
        let mut best_original_start = 0;
        let mut best_modified_start = 0;
        for i in original_start..original_max {
            for j in modified_start..modified_max {
                let score = self.contiguous_sequence_score(i, j, desired_length);
                if score > 0 && score > best_score {
                    best_score = score;
                    best_original_start = i;
                    best_modified_start = j;
                }
            }
        }
        if best_score > 0 { Some((best_original_start, best_modified_start)) } else { None }
    }

    fn contiguous_sequence_score(&self, original_start: i32, modified_start: i32, length: i32) -> i32 {
        let mut score = 0;
        for l in 0..length {
            if !self.elements_are_equal(original_start + l, modified_start + l) {
                return 0;
            }
            score += self.original_string_elements[index(original_start + l)].len() as i32;
        }
        score
    }

    fn is_boundary(&self, elements_len: usize, strings: &[JsString], i: i32) -> bool {
        if i <= 0 || i as i64 >= elements_len as i64 - 1 {
            return true;
        }
        // `/^\s*$/.test(...)`
        self.has_strings && strings[index(i)].units().iter().all(|&unit| is_js_whitespace(unit))
    }

    fn original_is_boundary(&self, i: i32) -> bool {
        self.is_boundary(self.original_elements_or_hash.len(), &self.original_string_elements, i)
    }

    fn original_region_is_boundary(&self, original_start: i32, original_length: i32) -> bool {
        if self.original_is_boundary(original_start) || self.original_is_boundary(original_start - 1) {
            return true;
        }
        if original_length > 0 {
            let original_end = original_start + original_length;
            if self.original_is_boundary(original_end - 1) || self.original_is_boundary(original_end) {
                return true;
            }
        }
        false
    }

    fn modified_is_boundary(&self, i: i32) -> bool {
        self.is_boundary(self.modified_elements_or_hash.len(), &self.modified_string_elements, i)
    }

    fn modified_region_is_boundary(&self, modified_start: i32, modified_length: i32) -> bool {
        if self.modified_is_boundary(modified_start) || self.modified_is_boundary(modified_start - 1) {
            return true;
        }
        if modified_length > 0 {
            let modified_end = modified_start + modified_length;
            if self.modified_is_boundary(modified_end - 1) || self.modified_is_boundary(modified_end) {
                return true;
            }
        }
        false
    }

    fn boundary_score(&self, original_start: i32, original_length: i32, modified_start: i32, modified_length: i32) -> i32 {
        let original_score = i32::from(self.original_region_is_boundary(original_start, original_length));
        let modified_score = i32::from(self.modified_region_is_boundary(modified_start, modified_length));
        original_score + modified_score
    }

    /// Concatenates the two input DiffChange lists and returns the resulting list.
    fn concatenate_changes(&self, mut left: Vec<DiffChange>, right: Vec<DiffChange>) -> Vec<DiffChange> {
        if left.is_empty() || right.is_empty() {
            return if !right.is_empty() { right } else { left };
        }

        if let Some(merged) = Self::changes_overlap(left.last().expect("não vazia"), &right[0]) {
            // Since we break the problem down recursively, it is possible that we
            // might recurse in the middle of a change thereby splitting it into
            // two changes. Here in the combining stage, we detect and fuse those
            // changes back together
            let last = left.len() - 1;
            left[last] = merged;
            left.extend_from_slice(&right[1..]);
        } else {
            left.extend(right);
        }
        left
    }

    /// Returns the merged change if the two changes overlap and can be merged into a single change.
    fn changes_overlap(left: &DiffChange, right: &DiffChange) -> Option<DiffChange> {
        assert(left.original_start <= right.original_start, "Left change is not less than or equal to right change");
        assert(left.modified_start <= right.modified_start, "Left change is not less than or equal to right change");

        if left.original_start + left.original_length >= right.original_start || left.modified_start + left.modified_length >= right.modified_start {
            let original_start = left.original_start;
            let mut original_length = left.original_length;
            let modified_start = left.modified_start;
            let mut modified_length = left.modified_length;

            if left.original_start + left.original_length >= right.original_start {
                original_length = right.original_start + right.original_length - left.original_start;
            }
            if left.modified_start + left.modified_length >= right.modified_start {
                modified_length = right.modified_start + right.modified_length - left.modified_start;
            }

            Some(DiffChange::new(original_start, original_length, modified_start, modified_length))
        } else {
            None
        }
    }

    /// Helper method used to clip a diagonal index to the range of valid
    /// diagonals. This also decides whether or not the diagonal index,
    /// if it exceeds the boundary, should be clipped to the boundary or clipped
    /// one inside the boundary depending on the Even/Odd status of the boundary
    /// and numDifferences.
    fn clip_diagonal_bound(diagonal: i32, num_differences: i32, diagonal_base_index: i32, num_diagonals: i32) -> i32 {
        if diagonal >= 0 && diagonal < num_diagonals {
            // Nothing to clip, its in range
            return diagonal;
        }

        // diagonalsBelow: The number of diagonals below the reference diagonal
        // diagonalsAbove: The number of diagonals above the reference diagonal
        let diagonals_below = diagonal_base_index;
        let diagonals_above = num_diagonals - diagonal_base_index - 1;
        let diff_even = num_differences % 2 == 0;

        if diagonal < 0 {
            let lower_bound_even = diagonals_below % 2 == 0;
            if diff_even == lower_bound_even { 0 } else { 1 }
        } else {
            let upper_bound_even = diagonals_above % 2 == 0;
            if diff_even == upper_bound_even { num_diagonals - 1 } else { num_diagonals - 2 }
        }
    }
}

thread_local! {
    /// Precomputed equality array for character codes (`precomputedEqualityArray`, um `Uint32Array(0x10000)`).
    static PRECOMPUTED_EQUALITY_ARRAY: RefCell<Vec<u32>> = RefCell::new(vec![0; 0x10000]);
}

/// `a << n` do JavaScript: a contagem vale módulo 32.
fn shl(a: i32, n: i64) -> i32 {
    a.wrapping_shl((n & 31) as u32)
}

/// `a >>> n` do JavaScript, como número não negativo.
fn ushr(a: i32, n: i64) -> u32 {
    (a as u32) >> ((n & 31) as u32)
}

/// Computes the Levenshtein distance for strings of length <= 32.
fn compute_levenshtein_distance_for_short_strings(first_string: &[u16], second_string: &[u16]) -> i32 {
    let first_string_length = first_string.len() as i64;
    let second_string_length = second_string.len();
    let last_bit_mask = shl(1, first_string_length - 1);
    let mut positive_vector: i32 = -1;
    let mut negative_vector: i32 = 0;
    let mut distance = first_string_length as i32;

    PRECOMPUTED_EQUALITY_ARRAY.with(|table| {
        let mut table = table.borrow_mut();

        // Initialize precomputedEqualityArray for firstString
        for index in (0..first_string.len()).rev() {
            table[usize::from(first_string[index])] |= shl(1, index as i64) as u32;
        }

        // Process each character of secondString
        for index in 0..second_string_length {
            let mut equality_mask = table[usize::from(second_string[index])] as i32;
            let combined_vector = equality_mask | negative_vector;
            equality_mask |= (equality_mask & positive_vector).wrapping_add(positive_vector) ^ positive_vector;
            negative_vector |= !(equality_mask | positive_vector);
            positive_vector &= equality_mask;
            if negative_vector & last_bit_mask != 0 {
                distance += 1;
            }
            if positive_vector & last_bit_mask != 0 {
                distance -= 1;
            }
            negative_vector = shl(negative_vector, 1) | 1;
            positive_vector = shl(positive_vector, 1) | !(combined_vector | negative_vector);
            negative_vector &= combined_vector;
        }

        // Reset precomputedEqualityArray
        for index in (0..first_string.len()).rev() {
            table[usize::from(first_string[index])] = 0;
        }
    });

    distance
}

/// Computes the Levenshtein distance for strings of length > 32.
fn compute_levenshtein_distance_for_long_strings(first_string: &[u16], second_string: &[u16]) -> i32 {
    let first_string_length = first_string.len();
    let second_string_length = second_string.len();
    let horizontal_size = first_string_length.div_ceil(32);
    let vertical_size = second_string_length.div_ceil(32);

    // Initialize horizontal and vertical bit arrays
    let mut horizontal_bit_array = vec![-1i32; horizontal_size];
    let mut vertical_bit_array = vec![0i32; horizontal_size];

    PRECOMPUTED_EQUALITY_ARRAY.with(|table| {
        let mut table = table.borrow_mut();

        // Uma volta do laço interno, igual nos blocos intermediários e no último.
        struct Vectors {
            negative: i32,
            positive: i32,
        }
        let step = |i: usize, vectors: &mut Vectors, table: &[u32], horizontal: &mut [i32], vertical: &mut [i32]| -> (i32, i32) {
            let equality_mask = table[usize::from(first_string[i])] as i32;
            let previous_bit = ushr(horizontal[i / 32], i as i64) as i32 & 1;
            let match_bit = ushr(vertical[i / 32], i as i64) as i32 & 1;
            let combined_vector = equality_mask | vectors.negative;
            let combined_horizontal_vector =
                (((equality_mask | match_bit) & vectors.positive).wrapping_add(vectors.positive) ^ vectors.positive) | equality_mask | match_bit;
            let mut positive_horizontal_vector = vectors.negative | !(combined_horizontal_vector | vectors.positive);
            let mut negative_horizontal_vector = vectors.positive & combined_horizontal_vector;
            // Os dois vetores horizontais antes do deslocamento, para quem os lê (o `distance` do último bloco).
            let before_shift = (positive_horizontal_vector, negative_horizontal_vector);
            if (ushr(positive_horizontal_vector, 31) as i32 ^ previous_bit) != 0 {
                horizontal[i / 32] ^= shl(1, i as i64);
            }
            if (ushr(negative_horizontal_vector, 31) as i32 ^ match_bit) != 0 {
                vertical[i / 32] ^= shl(1, i as i64);
            }
            positive_horizontal_vector = shl(positive_horizontal_vector, 1) | previous_bit;
            negative_horizontal_vector = shl(negative_horizontal_vector, 1) | match_bit;
            vectors.positive = negative_horizontal_vector | !(combined_vector | positive_horizontal_vector);
            vectors.negative = positive_horizontal_vector & combined_vector;
            before_shift
        };

        let mut vertical_index = 0;
        while vertical_index + 1 < vertical_size {
            let mut vectors = Vectors { negative: 0, positive: -1 };
            let start = vertical_index * 32;
            let vertical_length = 32.min(second_string_length) + start;

            // Initialize precomputedEqualityArray for secondString
            for k in start..vertical_length {
                table[usize::from(second_string[k])] |= shl(1, k as i64) as u32;
            }

            // Process each character of firstString
            for i in 0..first_string_length {
                step(i, &mut vectors, &table, &mut horizontal_bit_array, &mut vertical_bit_array);
            }

            // Reset precomputedEqualityArray
            for k in start..vertical_length {
                table[usize::from(second_string[k])] = 0;
            }
            vertical_index += 1;
        }

        let mut vectors = Vectors { negative: 0, positive: -1 };
        let start = vertical_index * 32;
        let vertical_length = 32.min(second_string_length - start) + start;

        // Initialize precomputedEqualityArray for secondString
        for k in start..vertical_length {
            table[usize::from(second_string[k])] |= shl(1, k as i64) as u32;
        }

        let mut distance = second_string_length as i32;

        // Process each character of firstString
        for i in 0..first_string_length {
            let (positive_horizontal_vector, negative_horizontal_vector) =
                step(i, &mut vectors, &table, &mut horizontal_bit_array, &mut vertical_bit_array);
            distance += (ushr(positive_horizontal_vector, second_string_length as i64 - 1) & 1) as i32;
            distance -= (ushr(negative_horizontal_vector, second_string_length as i64 - 1) & 1) as i32;
        }

        // Reset precomputedEqualityArray
        for k in start..vertical_length {
            table[usize::from(second_string[k])] = 0;
        }

        distance
    })
}

/// Computes the Levenshtein distance between two strings.
pub fn compute_levenshtein_distance(first_string: &[u16], second_string: &[u16]) -> i32 {
    let (first_string, second_string) = if first_string.len() < second_string.len() { (second_string, first_string) } else { (first_string, second_string) };
    if second_string.is_empty() {
        return first_string.len() as i32;
    }
    if first_string.len() <= 32 {
        return compute_levenshtein_distance_for_short_strings(first_string, second_string);
    }
    compute_levenshtein_distance_for_long_strings(first_string, second_string)
}
