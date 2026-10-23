//! Porte de `src/vs/editor/common/diff/legacyLinesDiffComputer.ts`: o computador de diff de linhas legado
//! (`DiffComputer`), com o diff de caracteres dentro de cada bloco de linhas, e o adaptador
//! `LegacyLinesDiffComputer` para `ILinesDiffComputer`.
//!
//! Notas de tradução:
//! - o texto é UTF-16 ([`JsString`]) e as colunas contam unidades UTF-16, como no original;
//! - `IChange`, `ICharChange` e `ILineChange` são o trait [`IChange`] mais as estruturas [`CharChange`] e
//!   [`LineChange`] (as interfaces não têm outro uso além de descrever essas duas classes);
//! - o predicado de continuação do original é `() => boolean`; aqui é [`ContinuePredicate`], embrulhado no
//!   `ContinueProcessingPredicate` do `LcsDiff` (que recebe dois argumentos e os ignora);
//! - `DiffComputer` não guarda `originalLines` e `modifiedLines` à parte: as linhas ficam nas `LineSequence`
//!   (`lines`), que é onde o original também as lê;
//! - o corpo de `computeDiff` de `LegacyLinesDiffComputer` repete a conversão de `IChange` em intervalos de
//!   linhas de `lineRangeMappingFromChange`; aqui as duas usam [`line_ranges_from_change`].

use std::fmt;
use std::rc::Rc;
use std::time::Instant;

use je_base::JsString;
use je_base::assert::{assert_fn, check_adjacent_items};
use je_base::char_code as cc;
use je_base::diff::{ContinueProcessingPredicate, DiffChange, DiffResult, ISequence, LcsDiff, SequenceElements};
use je_base::errors::{self, Error};
use je_base::js_string::char_code_at;
use je_base::strings;

use crate::diff::lines_diff_computer::{ILinesDiffComputer, LinesDiff, LinesDiffComputerOptions};
use crate::diff::range_mapping::{DetailedLineRangeMapping, RangeMapping, line_ranges_from_change};
use crate::range::Range;

const MINIMUM_MATCHING_CHARACTER_LENGTH: i32 = 3;

pub struct LegacyLinesDiffComputer;

impl ILinesDiffComputer for LegacyLinesDiffComputer {
    fn compute_diff(&self, original_lines: &[JsString], modified_lines: &[JsString], options: &LinesDiffComputerOptions) -> LinesDiff {
        let diff_computer = DiffComputer::new(
            original_lines.to_vec(),
            modified_lines.to_vec(),
            DiffComputerOpts {
                max_computation_time: options.max_computation_time_ms,
                should_ignore_trim_whitespace: options.ignore_trim_whitespace,
                should_compute_char_changes: true,
                should_make_pretty_diff: true,
                should_post_process_char_changes: true,
            },
        );
        let result = diff_computer.compute_diff();
        let mut changes: Vec<DetailedLineRangeMapping> = Vec::new();

        for c in &result.changes {
            let (original_range, modified_range) = line_ranges_from_change(c);

            let mut change = DetailedLineRangeMapping::new(
                original_range,
                modified_range,
                c.char_changes.as_ref().map(|char_changes| {
                    char_changes.iter().map(|c| RangeMapping::new(c.original_range(), c.modified_range())).collect()
                }),
            );
            let joined = changes.last().filter(|last_change| {
                last_change.modified.end_line_number_exclusive == change.modified.start_line_number
                    || last_change.original.end_line_number_exclusive == change.original.start_line_number
            });
            if let Some(last_change) = joined {
                // join touching diffs. Probably moving diffs up/down in the algorithm causes touching diffs.
                change = DetailedLineRangeMapping::new(
                    last_change.original.join(&change.original),
                    last_change.modified.join(&change.modified),
                    last_change
                        .inner_changes
                        .as_ref()
                        .zip(change.inner_changes.as_ref())
                        .map(|(last, current)| last.iter().chain(current).copied().collect()),
                );
                changes.pop();
            }

            changes.push(change);
        }

        assert_fn(|| {
            check_adjacent_items(&changes, |m1, m2| {
                m2.original.start_line_number - m1.original.end_line_number_exclusive
                    == m2.modified.start_line_number - m1.modified.end_line_number_exclusive
                    // There has to be an unchanged line in between (otherwise both diffs should have been joined)
                    && m1.original.end_line_number_exclusive < m2.original.start_line_number
                    && m1.modified.end_line_number_exclusive < m2.modified.start_line_number
            })
        });

        LinesDiff::new(changes, Vec::new(), result.quit_early)
    }
}

/// `IDiffComputationResult`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DiffComputationResult {
    pub quit_early: bool,
    pub identical: bool,

    /// The changes as (legacy) line change array. Deprecado no original: use `changes2`.
    pub changes: Vec<LineChange>,

    /// The changes as (modern) line range mapping array.
    pub changes2: Vec<DetailedLineRangeMapping>,
}

/// `IChange`: uma mudança de linhas. O lado com o número final igual a zero é uma inserção ou remoção.
pub trait IChange {
    fn original_start_line_number(&self) -> i32;
    fn original_end_line_number(&self) -> i32;
    fn modified_start_line_number(&self) -> i32;
    fn modified_end_line_number(&self) -> i32;
}

macro_rules! impl_change {
    ($type:ty) => {
        impl IChange for $type {
            fn original_start_line_number(&self) -> i32 {
                self.original_start_line_number
            }

            fn original_end_line_number(&self) -> i32 {
                self.original_end_line_number
            }

            fn modified_start_line_number(&self) -> i32 {
                self.modified_start_line_number
            }

            fn modified_end_line_number(&self) -> i32 {
                self.modified_end_line_number
            }
        }
    };
}

/// `IDiffComputerResult`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DiffComputerResult {
    pub quit_early: bool,
    pub changes: Vec<LineChange>,
}

/// `() => boolean`: continua o cálculo enquanto devolve `true`.
pub type ContinuePredicate = Rc<dyn Fn() -> bool>;

fn compute_diff(
    original_sequence: Rc<dyn ISequence>,
    modified_sequence: Rc<dyn ISequence>,
    continue_processing_predicate: &ContinuePredicate,
    pretty: bool,
) -> DiffResult {
    let continue_processing_predicate = Rc::clone(continue_processing_predicate);
    let predicate: ContinueProcessingPredicate = Box::new(move |_, _| continue_processing_predicate());
    let mut diff_algo = LcsDiff::new(original_sequence, modified_sequence, Some(predicate));
    diff_algo.compute_diff(pretty)
}

struct LineSequence {
    lines: Vec<JsString>,
    start_columns: Vec<i32>,
    end_columns: Vec<i32>,
}

impl LineSequence {
    fn new(lines: Vec<JsString>) -> Self {
        let start_columns = lines.iter().map(|line| get_first_non_blank_column(line, 1)).collect();
        let end_columns = lines.iter().map(|line| get_last_non_blank_column(line, 1)).collect();
        Self { lines, start_columns, end_columns }
    }

    fn get_start_line_number(&self, i: i32) -> i32 {
        i + 1
    }

    fn get_end_line_number(&self, i: i32) -> i32 {
        i + 1
    }

    fn create_char_sequence(&self, should_ignore_trim_whitespace: bool, start_index: usize, end_index: usize) -> CharSequence {
        let mut char_codes: Vec<i32> = Vec::new();
        let mut line_numbers: Vec<i32> = Vec::new();
        let mut columns: Vec<i32> = Vec::new();
        for index in start_index..=end_index {
            let line_content = &self.lines[index];
            let start_column = if should_ignore_trim_whitespace { self.start_columns[index] } else { 1 };
            let end_column = if should_ignore_trim_whitespace { self.end_columns[index] } else { line_content.len() as i32 + 1 };
            for col in start_column..end_column {
                char_codes.push(i32::from(line_content.units()[(col - 1) as usize]));
                line_numbers.push(index as i32 + 1);
                columns.push(col);
            }
            if !should_ignore_trim_whitespace && index < end_index {
                // Add \n if trim whitespace is not ignored
                char_codes.push(i32::from(cc::LineFeed));
                line_numbers.push(index as i32 + 1);
                columns.push(line_content.len() as i32 + 1);
            }
        }
        CharSequence { char_codes, line_numbers, columns }
    }
}

impl ISequence for LineSequence {
    fn get_elements(&self) -> SequenceElements {
        SequenceElements::Strings(
            self.lines
                .iter()
                .enumerate()
                .map(|(i, line)| line.slice((self.start_columns[i] - 1) as usize, (self.end_columns[i] - 1) as usize))
                .collect(),
        )
    }

    fn get_strict_element(&self, index: usize) -> Option<JsString> {
        self.lines.get(index).cloned()
    }
}

struct CharSequence {
    char_codes: Vec<i32>,
    line_numbers: Vec<i32>,
    columns: Vec<i32>,
}

fn assert_index(index: i32, arr: &[i32]) {
    if index < 0 || index as usize >= arr.len() {
        errors::throw(Error::new("Illegal index"));
    }
}

impl CharSequence {
    fn len(&self) -> usize {
        self.char_codes.len()
    }

    fn get_start_line_number(&self, i: i32) -> i32 {
        if i > 0 && i as usize == self.line_numbers.len() {
            // the start line number of the element after the last element
            // is the end line number of the last element
            return self.get_end_line_number(i - 1);
        }
        assert_index(i, &self.line_numbers);

        self.line_numbers[i as usize]
    }

    fn get_end_line_number(&self, i: i32) -> i32 {
        if i == -1 {
            // the end line number of the element before the first element
            // is the start line number of the first element
            return self.get_start_line_number(i + 1);
        }
        assert_index(i, &self.line_numbers);

        if self.char_codes[i as usize] == i32::from(cc::LineFeed) {
            return self.line_numbers[i as usize] + 1;
        }
        self.line_numbers[i as usize]
    }

    fn get_start_column(&self, i: i32) -> i32 {
        if i > 0 && i as usize == self.columns.len() {
            // the start column of the element after the last element
            // is the end column of the last element
            return self.get_end_column(i - 1);
        }
        assert_index(i, &self.columns);
        self.columns[i as usize]
    }

    fn get_end_column(&self, i: i32) -> i32 {
        if i == -1 {
            // the end column of the element before the first element
            // is the start column of the first element
            return self.get_start_column(i + 1);
        }
        assert_index(i, &self.columns);

        if self.char_codes[i as usize] == i32::from(cc::LineFeed) {
            return 1;
        }
        self.columns[i as usize] + 1
    }
}

impl ISequence for CharSequence {
    fn get_elements(&self) -> SequenceElements {
        SequenceElements::Ints(self.char_codes.clone())
    }
}

impl fmt::Display for CharSequence {
    /// `toString()`: `[a-(linha,coluna), \n-(linha,coluna), ...]`.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let parts: Vec<String> = self
            .char_codes
            .iter()
            .enumerate()
            .map(|(idx, &s)| {
                let text = if s == i32::from(cc::LineFeed) { "\\n".to_string() } else { String::from_utf16_lossy(&[s as u16]) };
                format!("{text}-({},{})", self.line_numbers[idx], self.columns[idx])
            })
            .collect();
        write!(f, "[{}]", parts.join(", "))
    }
}

/// A linha e o trecho de colunas de um lado (original ou modificado) de uma mudança numa linha só.
#[derive(Clone, Copy, Debug)]
struct LineColumns {
    line_number: i32,
    start_column: i32,
    end_column: i32,
}

/// `ICharChange`: uma mudança no nível de caractere.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CharChange {
    pub original_start_line_number: i32,
    pub original_start_column: i32,
    pub original_end_line_number: i32,
    pub original_end_column: i32,

    pub modified_start_line_number: i32,
    pub modified_start_column: i32,
    pub modified_end_line_number: i32,
    pub modified_end_column: i32,
}

impl_change!(CharChange);

impl CharChange {
    fn create_from_diff_change(diff_change: &DiffChange, original_char_sequence: &CharSequence, modified_char_sequence: &CharSequence) -> CharChange {
        let original_start_line_number = original_char_sequence.get_start_line_number(diff_change.original_start);
        let original_start_column = original_char_sequence.get_start_column(diff_change.original_start);
        let original_end_line_number = original_char_sequence.get_end_line_number(diff_change.original_start + diff_change.original_length - 1);
        let original_end_column = original_char_sequence.get_end_column(diff_change.original_start + diff_change.original_length - 1);

        let modified_start_line_number = modified_char_sequence.get_start_line_number(diff_change.modified_start);
        let modified_start_column = modified_char_sequence.get_start_column(diff_change.modified_start);
        let modified_end_line_number = modified_char_sequence.get_end_line_number(diff_change.modified_start + diff_change.modified_length - 1);
        let modified_end_column = modified_char_sequence.get_end_column(diff_change.modified_start + diff_change.modified_length - 1);

        CharChange {
            original_start_line_number,
            original_start_column,
            original_end_line_number,
            original_end_column,
            modified_start_line_number,
            modified_start_column,
            modified_end_line_number,
            modified_end_column,
        }
    }

    /// Uma mudança que cabe numa linha de cada lado.
    fn on_lines(original: LineColumns, modified: LineColumns) -> CharChange {
        CharChange {
            original_start_line_number: original.line_number,
            original_start_column: original.start_column,
            original_end_line_number: original.line_number,
            original_end_column: original.end_column,
            modified_start_line_number: modified.line_number,
            modified_start_column: modified.start_column,
            modified_end_line_number: modified.line_number,
            modified_end_column: modified.end_column,
        }
    }

    /// O intervalo do lado original.
    pub fn original_range(&self) -> Range {
        Range::new(self.original_start_line_number, self.original_start_column, self.original_end_line_number, self.original_end_column)
    }

    /// O intervalo do lado modificado.
    pub fn modified_range(&self) -> Range {
        Range::new(self.modified_start_line_number, self.modified_start_column, self.modified_end_line_number, self.modified_end_column)
    }
}

fn post_process_char_changes(raw_changes: Vec<DiffChange>) -> Vec<DiffChange> {
    if raw_changes.len() <= 1 {
        return raw_changes;
    }

    let mut result = vec![raw_changes[0]];

    for curr_change in &raw_changes[1..] {
        let prev_change = result.last_mut().expect("o resultado começa com um elemento");

        let original_matching_length = curr_change.original_start - (prev_change.original_start + prev_change.original_length);
        let modified_matching_length = curr_change.modified_start - (prev_change.modified_start + prev_change.modified_length);
        // Both of the above should be equal, but the continueProcessingPredicate may prevent this from being true
        let matching_length = original_matching_length.min(modified_matching_length);

        if matching_length < MINIMUM_MATCHING_CHARACTER_LENGTH {
            // Merge the current change into the previous one
            prev_change.original_length = (curr_change.original_start + curr_change.original_length) - prev_change.original_start;
            prev_change.modified_length = (curr_change.modified_start + curr_change.modified_length) - prev_change.modified_start;
        } else {
            // Add the current change
            result.push(*curr_change);
        }
    }

    result
}

/// `ILineChange`: uma mudança de linhas, com as mudanças de caractere dentro dela quando calculadas.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LineChange {
    pub original_start_line_number: i32,
    pub original_end_line_number: i32,
    pub modified_start_line_number: i32,
    pub modified_end_line_number: i32,
    pub char_changes: Option<Vec<CharChange>>,
}

impl_change!(LineChange);

impl LineChange {
    fn create_from_diff_result(
        should_ignore_trim_whitespace: bool,
        diff_change: &DiffChange,
        original_line_sequence: &LineSequence,
        modified_line_sequence: &LineSequence,
        continue_char_diff: &ContinuePredicate,
        should_compute_char_changes: bool,
        should_post_process_char_changes: bool,
    ) -> LineChange {
        let (original_start_line_number, original_end_line_number) = if diff_change.original_length == 0 {
            (original_line_sequence.get_start_line_number(diff_change.original_start) - 1, 0)
        } else {
            (
                original_line_sequence.get_start_line_number(diff_change.original_start),
                original_line_sequence.get_end_line_number(diff_change.original_start + diff_change.original_length - 1),
            )
        };

        let (modified_start_line_number, modified_end_line_number) = if diff_change.modified_length == 0 {
            (modified_line_sequence.get_start_line_number(diff_change.modified_start) - 1, 0)
        } else {
            (
                modified_line_sequence.get_start_line_number(diff_change.modified_start),
                modified_line_sequence.get_end_line_number(diff_change.modified_start + diff_change.modified_length - 1),
            )
        };

        let mut char_changes: Option<Vec<CharChange>> = None;

        if should_compute_char_changes
            && diff_change.original_length > 0
            && diff_change.original_length < 20
            && diff_change.modified_length > 0
            && diff_change.modified_length < 20
            && continue_char_diff()
        {
            // Compute character changes for diff chunks of at most 20 lines...
            let original_char_sequence = Rc::new(original_line_sequence.create_char_sequence(
                should_ignore_trim_whitespace,
                diff_change.original_start as usize,
                (diff_change.original_start + diff_change.original_length - 1) as usize,
            ));
            let modified_char_sequence = Rc::new(modified_line_sequence.create_char_sequence(
                should_ignore_trim_whitespace,
                diff_change.modified_start as usize,
                (diff_change.modified_start + diff_change.modified_length - 1) as usize,
            ));

            if original_char_sequence.len() > 0 && modified_char_sequence.len() > 0 {
                let mut raw_changes = compute_diff(original_char_sequence.clone(), modified_char_sequence.clone(), continue_char_diff, true).changes;

                if should_post_process_char_changes {
                    raw_changes = post_process_char_changes(raw_changes);
                }

                char_changes = Some(
                    raw_changes
                        .iter()
                        .map(|raw_change| CharChange::create_from_diff_change(raw_change, &original_char_sequence, &modified_char_sequence))
                        .collect(),
                );
            }
        }

        LineChange { original_start_line_number, original_end_line_number, modified_start_line_number, modified_end_line_number, char_changes }
    }
}

/// `IDiffComputerOpts`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct DiffComputerOpts {
    pub should_compute_char_changes: bool,
    pub should_post_process_char_changes: bool,
    pub should_ignore_trim_whitespace: bool,
    pub should_make_pretty_diff: bool,
    /// O `number` do original, em milissegundos; `0` desliga o limite.
    pub max_computation_time: f64,
}

pub struct DiffComputer {
    should_compute_char_changes: bool,
    should_post_process_char_changes: bool,
    should_ignore_trim_whitespace: bool,
    should_make_pretty_diff: bool,
    original: Rc<LineSequence>,
    modified: Rc<LineSequence>,
    continue_line_diff: ContinuePredicate,
    continue_char_diff: ContinuePredicate,
}

impl DiffComputer {
    pub fn new(original_lines: Vec<JsString>, modified_lines: Vec<JsString>, opts: DiffComputerOpts) -> Self {
        Self {
            should_compute_char_changes: opts.should_compute_char_changes,
            should_post_process_char_changes: opts.should_post_process_char_changes,
            should_ignore_trim_whitespace: opts.should_ignore_trim_whitespace,
            should_make_pretty_diff: opts.should_make_pretty_diff,
            original: Rc::new(LineSequence::new(original_lines)),
            modified: Rc::new(LineSequence::new(modified_lines)),

            continue_line_diff: create_continue_processing_predicate(opts.max_computation_time),
            // never run after 5s for character changes...
            continue_char_diff: create_continue_processing_predicate(if opts.max_computation_time == 0.0 {
                0.0
            } else {
                opts.max_computation_time.min(5000.0)
            }),
        }
    }

    fn line_change_from(&self, diff_change: &DiffChange) -> LineChange {
        LineChange::create_from_diff_result(
            self.should_ignore_trim_whitespace,
            diff_change,
            &self.original,
            &self.modified,
            &self.continue_char_diff,
            self.should_compute_char_changes,
            self.should_post_process_char_changes,
        )
    }

    pub fn compute_diff(&self) -> DiffComputerResult {
        if self.original.lines.len() == 1 && self.original.lines[0].is_empty() {
            // empty original => fast path
            if self.modified.lines.len() == 1 && self.modified.lines[0].is_empty() {
                return DiffComputerResult { quit_early: false, changes: Vec::new() };
            }

            return DiffComputerResult {
                quit_early: false,
                changes: vec![LineChange {
                    original_start_line_number: 1,
                    original_end_line_number: 1,
                    modified_start_line_number: 1,
                    modified_end_line_number: self.modified.lines.len() as i32,
                    char_changes: None,
                }],
            };
        }

        if self.modified.lines.len() == 1 && self.modified.lines[0].is_empty() {
            // empty modified => fast path
            return DiffComputerResult {
                quit_early: false,
                changes: vec![LineChange {
                    original_start_line_number: 1,
                    original_end_line_number: self.original.lines.len() as i32,
                    modified_start_line_number: 1,
                    modified_end_line_number: 1,
                    char_changes: None,
                }],
            };
        }

        let diff_result = compute_diff(self.original.clone(), self.modified.clone(), &self.continue_line_diff, self.should_make_pretty_diff);
        let raw_changes = diff_result.changes;
        let quit_early = diff_result.quit_early;

        // The diff is always computed with ignoring trim whitespace
        // This ensures we get the prettiest diff

        if self.should_ignore_trim_whitespace {
            let line_changes: Vec<LineChange> = raw_changes.iter().map(|raw_change| self.line_change_from(raw_change)).collect();
            return DiffComputerResult { quit_early, changes: line_changes };
        }

        // Need to post-process and introduce changes where the trim whitespace is different
        // Note that we are looping starting at -1 to also cover the lines before the first change
        let mut result: Vec<LineChange> = Vec::new();

        let mut original_line_index: i32 = 0;
        let mut modified_line_index: i32 = 0;
        let len = raw_changes.len() as i32;
        let mut i: i32 = -1; /* !!!! */
        while i < len {
            let next_change = if i + 1 < len { Some(raw_changes[(i + 1) as usize]) } else { None };
            let original_stop = next_change.map_or(self.original.lines.len() as i32, |change| change.original_start);
            let modified_stop = next_change.map_or(self.modified.lines.len() as i32, |change| change.modified_start);

            while original_line_index < original_stop && modified_line_index < modified_stop {
                let original_line = &self.original.lines[original_line_index as usize];
                let modified_line = &self.modified.lines[modified_line_index as usize];

                if original_line != modified_line {
                    // These lines differ only in trim whitespace

                    // Check the leading whitespace
                    {
                        let mut original_start_column = get_first_non_blank_column(original_line, 1);
                        let mut modified_start_column = get_first_non_blank_column(modified_line, 1);
                        while original_start_column > 1 && modified_start_column > 1 {
                            let original_char = char_code_at(original_line.units(), i64::from(original_start_column) - 2);
                            let modified_char = char_code_at(modified_line.units(), i64::from(modified_start_column) - 2);
                            if char_codes_differ(original_char, modified_char) {
                                break;
                            }
                            original_start_column -= 1;
                            modified_start_column -= 1;
                        }

                        if original_start_column > 1 || modified_start_column > 1 {
                            self.push_trim_whitespace_char_change(
                                &mut result,
                                LineColumns { line_number: original_line_index + 1, start_column: 1, end_column: original_start_column },
                                LineColumns { line_number: modified_line_index + 1, start_column: 1, end_column: modified_start_column },
                            );
                        }
                    }

                    // Check the trailing whitespace
                    {
                        let mut original_end_column = get_last_non_blank_column(original_line, 1);
                        let mut modified_end_column = get_last_non_blank_column(modified_line, 1);
                        let original_max_column = original_line.len() as i32 + 1;
                        let modified_max_column = modified_line.len() as i32 + 1;
                        while original_end_column < original_max_column && modified_end_column < modified_max_column {
                            let original_char = char_code_at(original_line.units(), i64::from(original_end_column) - 1);
                            // O original lê as duas unidades de `originalLine` (não de `modifiedLine`); mantido.
                            let modified_char = char_code_at(original_line.units(), i64::from(modified_end_column) - 1);
                            if char_codes_differ(original_char, modified_char) {
                                break;
                            }
                            original_end_column += 1;
                            modified_end_column += 1;
                        }

                        if original_end_column < original_max_column || modified_end_column < modified_max_column {
                            self.push_trim_whitespace_char_change(
                                &mut result,
                                LineColumns { line_number: original_line_index + 1, start_column: original_end_column, end_column: original_max_column },
                                LineColumns { line_number: modified_line_index + 1, start_column: modified_end_column, end_column: modified_max_column },
                            );
                        }
                    }
                }
                original_line_index += 1;
                modified_line_index += 1;
            }

            if let Some(next_change) = next_change {
                // Emit the actual change
                result.push(self.line_change_from(&next_change));

                original_line_index += next_change.original_length;
                modified_line_index += next_change.modified_length;
            }
            i += 1;
        }

        DiffComputerResult { quit_early, changes: result }
    }

    fn push_trim_whitespace_char_change(&self, result: &mut Vec<LineChange>, original: LineColumns, modified: LineColumns) {
        if self.merge_trim_whitespace_char_change(result, original, modified) {
            // Merged into previous
            return;
        }

        let char_changes = if self.should_compute_char_changes { Some(vec![CharChange::on_lines(original, modified)]) } else { None };
        result.push(LineChange {
            original_start_line_number: original.line_number,
            original_end_line_number: original.line_number,
            modified_start_line_number: modified.line_number,
            modified_end_line_number: modified.line_number,
            char_changes,
        });
    }

    fn merge_trim_whitespace_char_change(&self, result: &mut [LineChange], original: LineColumns, modified: LineColumns) -> bool {
        let Some(prev_change) = result.last_mut() else {
            return false;
        };

        if prev_change.original_end_line_number == 0 || prev_change.modified_end_line_number == 0 {
            // Don't merge with inserts/deletes
            return false;
        }

        if prev_change.original_end_line_number == original.line_number && prev_change.modified_end_line_number == modified.line_number {
            if self.should_compute_char_changes
                && let Some(char_changes) = &mut prev_change.char_changes
            {
                char_changes.push(CharChange::on_lines(original, modified));
            }
            return true;
        }

        if prev_change.original_end_line_number + 1 == original.line_number && prev_change.modified_end_line_number + 1 == modified.line_number {
            prev_change.original_end_line_number = original.line_number;
            prev_change.modified_end_line_number = modified.line_number;
            if self.should_compute_char_changes
                && let Some(char_changes) = &mut prev_change.char_changes
            {
                char_changes.push(CharChange::on_lines(original, modified));
            }
            return true;
        }

        false
    }
}

/// `a !== b` entre dois `charCodeAt`: o `NaN` (fora do texto) nunca é igual a nada.
fn char_codes_differ(a: Option<u16>, b: Option<u16>) -> bool {
    match (a, b) {
        (Some(a), Some(b)) => a != b,
        _ => true,
    }
}

fn get_first_non_blank_column(txt: &JsString, default_value: i32) -> i32 {
    match strings::first_non_whitespace_index(txt.units()) {
        None => default_value,
        Some(r) => r as i32 + 1,
    }
}

fn get_last_non_blank_column(txt: &JsString, default_value: i32) -> i32 {
    match strings::last_non_whitespace_index(txt.units(), txt.len() as i64 - 1) {
        None => default_value,
        Some(r) => r as i32 + 2,
    }
}

fn create_continue_processing_predicate(maximum_runtime: f64) -> ContinuePredicate {
    if maximum_runtime == 0.0 {
        return Rc::new(|| true);
    }

    let start_time = Instant::now();
    Rc::new(move || start_time.elapsed().as_secs_f64() * 1000.0 < maximum_runtime)
}
