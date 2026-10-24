//! Porte de `src/vs/editor/common/diff/defaultLinesDiffComputer/heuristicSequenceOptimizations.ts`: as heurísticas que
//! melhoram o resultado dos algoritmos de diff (`optimizeSequenceDiffs`, `removeShortMatches`,
//! `extendDiffsToEntireWordIfAppropriate`, `removeVeryShortMatchingLinesBetweenDiffs` e
//! `removeVeryShortMatchingTextBetweenLongDiffs`).
//!
//! Representação:
//! - As listas de [`SequenceDiff`] entram por valor (o original muda alguns arrays no lugar e quem chama sempre reatribui
//!   o resultado, o que o retorno por valor reproduz).
//! - O `findParent` de `extendDiffsToEntireWordIfAppropriate` é uma função `&dyn Fn(&LinesSliceCharSequence, i32)`.
//!   O `scanWord` interno (um fechamento sobre `equalMappings`, `additional` e `lastPoint`) é a função [`scan_word`] com o
//!   estado em [`WordScan`], e `equalMappings.shift()` é o `pop_front` de um `VecDeque`.
//! - Os `!` do original (asserções de não nulo) são `expect`.
//! - `/\s/g` é `is_js_whitespace` de `je_base::js_string`, e `split(/\r\n|\r|\n/).length > 1` é a presença de `\r` ou `\n`.
//! - `Math.pow` é `powf`, e o `*` e o `/` do limite de palavra (`* 2 / 3`) são aritmética de ponto flutuante.

use std::collections::VecDeque;

use je_base::arrays::for_each_with_neighbors;
use je_base::js_string::{is_js_whitespace, trim};

use super::algorithms::diff_algorithm::{ISequence, OffsetPair, SequenceDiff};
use super::line_sequence::LineSequence;
use super::lines_slice_char_sequence::LinesSliceCharSequence;
use crate::offset_range::OffsetRange;

pub fn optimize_sequence_diffs(sequence1: &dyn ISequence, sequence2: &dyn ISequence, sequence_diffs: Vec<SequenceDiff>) -> Vec<SequenceDiff> {
    let mut result = sequence_diffs;
    result = join_sequence_diffs_by_shifting(sequence1, sequence2, result);
    // Sometimes, calling this function twice improves the result.
    // Uncomment the second invocation and run the tests to see the difference.
    result = join_sequence_diffs_by_shifting(sequence1, sequence2, result);
    result = shift_sequence_diffs(sequence1, sequence2, result);
    result
}

/// This function fixes issues like this:
/// ```txt
/// import { Baz, Bar } from "foo";
/// ```
/// <->
/// ```txt
/// import { Baz, Bar, Foo } from "foo";
/// ```
/// Computed diff: [ {Add "," after Bar}, {Add "Foo " after space} }
/// Improved diff: [{Add ", Foo" after Bar}]
fn join_sequence_diffs_by_shifting(sequence1: &dyn ISequence, sequence2: &dyn ISequence, sequence_diffs: Vec<SequenceDiff>) -> Vec<SequenceDiff> {
    if sequence_diffs.is_empty() {
        return sequence_diffs;
    }

    let mut result: Vec<SequenceDiff> = vec![sequence_diffs[0]];

    // First move them all to the left as much as possible and join them if possible
    for &diff in &sequence_diffs[1..] {
        let prev_result = result[result.len() - 1];
        let mut cur = diff;

        if cur.seq1_range.is_empty() || cur.seq2_range.is_empty() {
            let length = cur.seq1_range.start - prev_result.seq1_range.end_exclusive;
            let mut d = 1;
            while d <= length {
                if sequence1.get_element(cur.seq1_range.start - d) != sequence1.get_element(cur.seq1_range.end_exclusive - d)
                    || sequence2.get_element(cur.seq2_range.start - d) != sequence2.get_element(cur.seq2_range.end_exclusive - d)
                {
                    break;
                }
                d += 1;
            }
            d -= 1;

            if d == length {
                // Merge previous and current diff
                let last = result.len() - 1;
                result[last] = SequenceDiff::new(
                    OffsetRange::new(prev_result.seq1_range.start, cur.seq1_range.end_exclusive - length),
                    OffsetRange::new(prev_result.seq2_range.start, cur.seq2_range.end_exclusive - length),
                );
                continue;
            }

            cur = cur.delta(-d);
        }

        result.push(cur);
    }

    let mut result2: Vec<SequenceDiff> = Vec::new();
    // Then move them all to the right and join them again if possible
    for i in 0..result.len() - 1 {
        let next_result = result[i + 1];
        let mut cur = result[i];

        if cur.seq1_range.is_empty() || cur.seq2_range.is_empty() {
            let length = next_result.seq1_range.start - cur.seq1_range.end_exclusive;
            let mut d = 0;
            while d < length {
                if !sequence1.is_strongly_equal(cur.seq1_range.start + d, cur.seq1_range.end_exclusive + d)
                    || !sequence2.is_strongly_equal(cur.seq2_range.start + d, cur.seq2_range.end_exclusive + d)
                {
                    break;
                }
                d += 1;
            }

            if d == length {
                // Merge previous and current diff, write to result!
                result[i + 1] = SequenceDiff::new(
                    OffsetRange::new(cur.seq1_range.start + length, next_result.seq1_range.end_exclusive),
                    OffsetRange::new(cur.seq2_range.start + length, next_result.seq2_range.end_exclusive),
                );
                continue;
            }

            if d > 0 {
                cur = cur.delta(d);
            }
        }

        result2.push(cur);
    }

    if !result.is_empty() {
        result2.push(result[result.len() - 1]);
    }

    result2
}

// align character level diffs at whitespace characters
// import { IBar } from "foo";
// import { I[Arr, I]Bar } from "foo";
// ->
// import { [IArr, ]IBar } from "foo";

// import { ITransaction, observableValue, transaction } from 'vs/base/common/observable';
// import { ITransaction, observable[FromEvent, observable]Value, transaction } from 'vs/base/common/observable';
// ->
// import { ITransaction, [observableFromEvent, ]observableValue, transaction } from 'vs/base/common/observable';

// collectBrackets(level + 1, levelPerBracketType);
// collectBrackets(level + 1, levelPerBracket[ + 1, levelPerBracket]Type);
// ->
// collectBrackets(level + 1, [levelPerBracket + 1, ]levelPerBracketType);

fn shift_sequence_diffs(sequence1: &dyn ISequence, sequence2: &dyn ISequence, sequence_diffs: Vec<SequenceDiff>) -> Vec<SequenceDiff> {
    if !sequence1.supports_boundary_score() || !sequence2.supports_boundary_score() {
        return sequence_diffs;
    }

    let mut sequence_diffs = sequence_diffs;
    for i in 0..sequence_diffs.len() {
        let prev_diff = if i > 0 { Some(sequence_diffs[i - 1]) } else { None };
        let diff = sequence_diffs[i];
        let next_diff = if i + 1 < sequence_diffs.len() { Some(sequence_diffs[i + 1]) } else { None };

        let seq1_valid_range = OffsetRange::new(
            prev_diff.map_or(0, |prev| prev.seq1_range.end_exclusive + 1),
            next_diff.map_or(sequence1.length(), |next| next.seq1_range.start - 1),
        );
        let seq2_valid_range = OffsetRange::new(
            prev_diff.map_or(0, |prev| prev.seq2_range.end_exclusive + 1),
            next_diff.map_or(sequence2.length(), |next| next.seq2_range.start - 1),
        );

        if diff.seq1_range.is_empty() {
            sequence_diffs[i] = shift_diff_to_better_position(diff, sequence1, sequence2, seq1_valid_range, seq2_valid_range);
        } else if diff.seq2_range.is_empty() {
            sequence_diffs[i] =
                shift_diff_to_better_position(diff.swap(), sequence2, sequence1, seq2_valid_range, seq1_valid_range).swap();
        }
    }

    sequence_diffs
}

fn shift_diff_to_better_position(
    diff: SequenceDiff,
    sequence1: &dyn ISequence,
    sequence2: &dyn ISequence,
    seq1_valid_range: OffsetRange,
    seq2_valid_range: OffsetRange,
) -> SequenceDiff {
    let max_shift_limit = 100; // To prevent performance issues

    // don't touch previous or next!
    let mut delta_before = 1;
    while diff.seq1_range.start - delta_before >= seq1_valid_range.start
        && diff.seq2_range.start - delta_before >= seq2_valid_range.start
        && sequence2.is_strongly_equal(diff.seq2_range.start - delta_before, diff.seq2_range.end_exclusive - delta_before)
        && delta_before < max_shift_limit
    {
        delta_before += 1;
    }
    delta_before -= 1;

    let mut delta_after = 0;
    while diff.seq1_range.start + delta_after < seq1_valid_range.end_exclusive
        && diff.seq2_range.end_exclusive + delta_after < seq2_valid_range.end_exclusive
        && sequence2.is_strongly_equal(diff.seq2_range.start + delta_after, diff.seq2_range.end_exclusive + delta_after)
        && delta_after < max_shift_limit
    {
        delta_after += 1;
    }

    if delta_before == 0 && delta_after == 0 {
        return diff;
    }

    // Visualize `[sequence1.text, diff.seq1Range.start + deltaAfter]`
    // and `[sequence2.text, diff.seq2Range.start + deltaAfter, diff.seq2Range.endExclusive + deltaAfter]`

    let mut best_delta = 0;
    let mut best_score = -1;
    // find best scored delta
    for delta in -delta_before..=delta_after {
        let seq2_offset_start = diff.seq2_range.start + delta;
        let seq2_offset_end_exclusive = diff.seq2_range.end_exclusive + delta;
        let seq1_offset = diff.seq1_range.start + delta;

        let score = sequence1.get_boundary_score(seq1_offset)
            + sequence2.get_boundary_score(seq2_offset_start)
            + sequence2.get_boundary_score(seq2_offset_end_exclusive);
        if score > best_score {
            best_score = score;
            best_delta = delta;
        }
    }

    diff.delta(best_delta)
}

pub fn remove_short_matches(_sequence1: &dyn ISequence, _sequence2: &dyn ISequence, sequence_diffs: Vec<SequenceDiff>) -> Vec<SequenceDiff> {
    let mut result: Vec<SequenceDiff> = Vec::new();
    for s in sequence_diffs {
        let Some(&last) = result.last() else {
            result.push(s);
            continue;
        };

        if s.seq1_range.start - last.seq1_range.end_exclusive <= 2 || s.seq2_range.start - last.seq2_range.end_exclusive <= 2 {
            let last_index = result.len() - 1;
            result[last_index] = SequenceDiff::new(last.seq1_range.join(s.seq1_range), last.seq2_range.join(s.seq2_range));
        } else {
            result.push(s);
        }
    }

    result
}

/// O estado que o `scanWord` de `extendDiffsToEntireWordIfAppropriate` compartilha (`equalMappings`, `additional`,
/// `lastPoint`).
struct WordScan {
    equal_mappings: VecDeque<SequenceDiff>,
    additional: Vec<SequenceDiff>,
    last_point: OffsetPair,
}

type FindParent<'a> = &'a dyn Fn(&LinesSliceCharSequence, i32) -> Option<OffsetRange>;

fn scan_word(
    scan: &mut WordScan,
    sequence1: &LinesSliceCharSequence,
    sequence2: &LinesSliceCharSequence,
    find_parent: FindParent<'_>,
    force: bool,
    pair: OffsetPair,
    equal_mapping: &SequenceDiff,
) {
    if pair.offset1 < scan.last_point.offset1 || pair.offset2 < scan.last_point.offset2 {
        return;
    }

    let (Some(w1), Some(w2)) = (find_parent(sequence1, pair.offset1), find_parent(sequence2, pair.offset2)) else {
        return;
    };
    let mut w = SequenceDiff::new(w1, w2);
    let equal_part = w.intersect(equal_mapping).expect("a palavra toca o trecho igual de onde partiu");

    let mut equal_chars1 = equal_part.seq1_range.length();
    let mut equal_chars2 = equal_part.seq2_range.length();

    // The words do not touch previous equals mappings, as we would have processed them already.
    // But they might touch the next ones.

    while let Some(&next) = scan.equal_mappings.front() {
        let intersects = next.seq1_range.intersects(w.seq1_range) || next.seq2_range.intersects(w.seq2_range);
        if !intersects {
            break;
        }

        let v1 = find_parent(sequence1, next.seq1_range.start);
        let v2 = find_parent(sequence2, next.seq2_range.start);
        // Because there is an intersection, we know that the words are not empty.
        let v = SequenceDiff::new(v1.expect("a palavra não é vazia"), v2.expect("a palavra não é vazia"));
        let equal_part = v.intersect(&next).expect("a palavra toca o trecho igual");

        equal_chars1 += equal_part.seq1_range.length();
        equal_chars2 += equal_part.seq2_range.length();

        w = w.join(&v);

        if w.seq1_range.end_exclusive >= next.seq1_range.end_exclusive {
            // The word extends beyond the next equal mapping.
            scan.equal_mappings.pop_front();
        } else {
            break;
        }
    }

    if (force && equal_chars1 + equal_chars2 < w.seq1_range.length() + w.seq2_range.length())
        || f64::from(equal_chars1 + equal_chars2) < f64::from((w.seq1_range.length() + w.seq2_range.length()) * 2) / 3.0
    {
        scan.additional.push(w);
    }

    scan.last_point = w.get_end_exclusives();
}

pub fn extend_diffs_to_entire_word_if_appropriate(
    sequence1: &LinesSliceCharSequence,
    sequence2: &LinesSliceCharSequence,
    sequence_diffs: Vec<SequenceDiff>,
    find_parent: FindParent<'_>,
    force: bool,
) -> Vec<SequenceDiff> {
    let equal_mappings = SequenceDiff::invert(&sequence_diffs, sequence1.length());

    let mut scan = WordScan { equal_mappings: equal_mappings.into(), additional: Vec::new(), last_point: OffsetPair::new(0, 0) };

    while let Some(next) = scan.equal_mappings.pop_front() {
        if next.seq1_range.is_empty() {
            continue;
        }
        scan_word(&mut scan, sequence1, sequence2, find_parent, force, next.get_starts(), &next);
        // The equal parts are not empty, so -1 gives us a character that is equal in both parts.
        scan_word(&mut scan, sequence1, sequence2, find_parent, force, next.get_end_exclusives().delta(-1), &next);
    }

    merge_sequence_diffs(sequence_diffs, scan.additional)
}

fn merge_sequence_diffs(sequence_diffs1: Vec<SequenceDiff>, sequence_diffs2: Vec<SequenceDiff>) -> Vec<SequenceDiff> {
    let mut sequence_diffs1: VecDeque<SequenceDiff> = sequence_diffs1.into();
    let mut sequence_diffs2: VecDeque<SequenceDiff> = sequence_diffs2.into();
    let mut result: Vec<SequenceDiff> = Vec::new();

    while !sequence_diffs1.is_empty() || !sequence_diffs2.is_empty() {
        let sd1 = sequence_diffs1.front().copied();
        let sd2 = sequence_diffs2.front().copied();

        let next = match (sd1, sd2) {
            (Some(sd1), sd2) if sd2.is_none_or(|sd2| sd1.seq1_range.start < sd2.seq1_range.start) => {
                sequence_diffs1.pop_front();
                sd1
            }
            _ => {
                sequence_diffs2.pop_front();
                sd2.expect("alguma das duas listas não está vazia")
            }
        };

        match result.last().copied() {
            Some(last) if last.seq1_range.end_exclusive >= next.seq1_range.start => {
                let last_index = result.len() - 1;
                result[last_index] = last.join(&next);
            }
            _ => result.push(next),
        }
    }

    result
}

pub fn remove_very_short_matching_lines_between_diffs(
    sequence1: &LineSequence,
    _sequence2: &LineSequence,
    sequence_diffs: Vec<SequenceDiff>,
) -> Vec<SequenceDiff> {
    let mut diffs = sequence_diffs;
    if diffs.is_empty() {
        return diffs;
    }

    let mut counter = 0;
    loop {
        let mut should_repeat = false;

        let mut result: Vec<SequenceDiff> = vec![diffs[0]];

        for &cur in &diffs[1..] {
            let last_result = result[result.len() - 1];

            let should_join = {
                let unchanged_range = OffsetRange::new(last_result.seq1_range.end_exclusive, cur.seq1_range.start);

                let unchanged_text = sequence1.get_text(unchanged_range);
                let unchanged_text_without_ws_length = unchanged_text.units().iter().filter(|&&unit| !is_js_whitespace(unit)).count();
                unchanged_text_without_ws_length <= 4
                    && (last_result.seq1_range.length() + last_result.seq2_range.length() > 5
                        || cur.seq1_range.length() + cur.seq2_range.length() > 5)
            };
            if should_join {
                should_repeat = true;
                let last_index = result.len() - 1;
                result[last_index] = result[last_index].join(&cur);
            } else {
                result.push(cur);
            }
        }

        diffs = result;

        let repeat = counter < 10 && should_repeat;
        counter += 1;
        if !repeat {
            break;
        }
    }

    diffs
}

pub fn remove_very_short_matching_text_between_long_diffs(
    sequence1: &LinesSliceCharSequence,
    sequence2: &LinesSliceCharSequence,
    sequence_diffs: Vec<SequenceDiff>,
) -> Vec<SequenceDiff> {
    let mut diffs = sequence_diffs;
    if diffs.is_empty() {
        return diffs;
    }

    let mut counter = 0;
    loop {
        let mut should_repeat = false;

        let mut result: Vec<SequenceDiff> = vec![diffs[0]];

        for &cur in &diffs[1..] {
            let last_result = result[result.len() - 1];

            if should_join_text_diffs(sequence1, sequence2, &last_result, &cur) {
                should_repeat = true;
                let last_index = result.len() - 1;
                result[last_index] = result[last_index].join(&cur);
            } else {
                result.push(cur);
            }
        }

        diffs = result;

        let repeat = counter < 10 && should_repeat;
        counter += 1;
        if !repeat {
            break;
        }
    }

    let mut new_diffs: Vec<SequenceDiff> = Vec::new();

    // Remove short suffixes/prefixes
    for_each_with_neighbors(&diffs, |prev, cur, next| {
        let mut new_diff = *cur;

        let should_mark_as_changed =
            |text: &je_base::JsString| !text.is_empty() && trim(text.units()).len() <= 3 && cur.seq1_range.length() + cur.seq2_range.length() > 100;

        let full_range1 = sequence1.extend_to_full_lines(cur.seq1_range);
        let prefix = sequence1.get_text(OffsetRange::new(full_range1.start, cur.seq1_range.start));
        if should_mark_as_changed(&prefix) {
            new_diff = new_diff.delta_start(-(prefix.len() as i32));
        }
        let suffix = sequence1.get_text(OffsetRange::new(cur.seq1_range.end_exclusive, full_range1.end_exclusive));
        if should_mark_as_changed(&suffix) {
            new_diff = new_diff.delta_end(suffix.len() as i32);
        }

        let available_space = SequenceDiff::from_offset_pairs(
            prev.map_or(OffsetPair::ZERO, SequenceDiff::get_end_exclusives),
            next.map_or(OffsetPair::MAX, SequenceDiff::get_starts),
        );
        let result = new_diff.intersect(&available_space).expect("o diff cabe no espaço disponível");
        match new_diffs.last().copied() {
            Some(last) if result.get_starts().equals(&last.get_end_exclusives()) => {
                let last_index = new_diffs.len() - 1;
                new_diffs[last_index] = last.join(&result);
            }
            _ => new_diffs.push(result),
        }
    });

    new_diffs
}

/// O `shouldJoinDiffs(before, after)` de `removeVeryShortMatchingTextBetweenLongDiffs`.
fn should_join_text_diffs(sequence1: &LinesSliceCharSequence, sequence2: &LinesSliceCharSequence, before: &SequenceDiff, after: &SequenceDiff) -> bool {
    let unchanged_range = OffsetRange::new(before.seq1_range.end_exclusive, after.seq1_range.start);

    let unchanged_line_count = sequence1.count_lines_in(unchanged_range);
    if unchanged_line_count > 5 || unchanged_range.length() > 500 {
        return false;
    }

    let unchanged_text = sequence1.get_text(unchanged_range);
    let unchanged_text = trim(unchanged_text.units());
    if unchanged_text.len() > 20 || unchanged_text.iter().any(|&unit| unit == u16::from(b'\r') || unit == u16::from(b'\n')) {
        return false;
    }

    let before_line_count1 = sequence1.count_lines_in(before.seq1_range);
    let before_seq1_length = before.seq1_range.length();
    let before_line_count2 = sequence2.count_lines_in(before.seq2_range);
    let before_seq2_length = before.seq2_range.length();

    let after_line_count1 = sequence1.count_lines_in(after.seq1_range);
    let after_seq1_length = after.seq1_range.length();
    let after_line_count2 = sequence2.count_lines_in(after.seq2_range);
    let after_seq2_length = after.seq2_range.length();

    // TODO: Maybe a neural net can be used to derive the result from these numbers

    let max = f64::from(2 * 40 + 50);
    let cap = |v: i32| f64::from(v).min(max);

    ((cap(before_line_count1 * 40 + before_seq1_length).powf(1.5) + cap(before_line_count2 * 40 + before_seq2_length).powf(1.5)).powf(1.5))
        + ((cap(after_line_count1 * 40 + after_seq1_length).powf(1.5) + cap(after_line_count2 * 40 + after_seq2_length).powf(1.5)).powf(1.5))
        > (max.powf(1.5).powf(1.5)) * 1.3
}
