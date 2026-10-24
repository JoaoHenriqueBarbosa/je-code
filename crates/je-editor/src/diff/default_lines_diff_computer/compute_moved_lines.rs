//! Porte de `src/vs/editor/common/diff/defaultLinesDiffComputer/computeMovedLines.ts`: a detecção de blocos de linhas que
//! mudaram de lugar (`computeMovedLines`).
//!
//! Representação:
//! - `Set<DetailedLineRangeMapping>` (`excludedChanges`, comparado por identidade) é o conjunto de índices da lista
//!   `changes` (ver [`LineRangeFragment::source_index`]); o `diffBeforeEndOfMove...` de `removeMovesInSameDiff` também
//!   é comparado por índice.
//! - `SetMap<string, { range }>` do `original3LineHashes` é `HashMap` de `(hash, hash, hash)` para a lista de intervalos
//!   (na ordem de inserção, a ordem do `Set` do original), e o `PossibleMapping` mutável que o original compartilha entre
//!   `possibleMappings` e `nextMappings` é o índice na lista `possible_mappings`.
//! - `Set<LineRangeFragment>` das inserções é a lista com `Option` (um `delete` deixa o buraco, e a iteração mantém a ordem).
//! - `MonotonousArray.findLastMonotonous` e `findLastMonotonous` são a busca binária `find_last_monotonous` (o resultado
//!   é o mesmo, o `MonotonousArray` só guarda o último índice para acelerar).
//! - `line1.charCodeAt(idx)` fora da linha (`NaN` no original, que não é um espaço) é `-1`.

use std::collections::{HashMap, HashSet};

use je_base::JsString;
use je_base::arrays::push_many;
use je_base::arrays_find::{find_last_idx_monotonous, find_last_monotonous};
use je_base::js_string::trim;

use super::algorithms::diff_algorithm::{ITimeout, IDiffAlgorithm, SequenceDiff};
use super::algorithms::myers_diff_algorithm::MyersDiffAlgorithm;
use super::lines_slice_char_sequence::LinesSliceCharSequence;
use super::utils::{LineRangeFragment, is_space};
use crate::diff::range_mapping::{DetailedLineRangeMapping, LineRangeMapping};
use crate::line_range::{LineRange, LineRangeSet};
use crate::range::Range;

pub fn compute_moved_lines(
    changes: &[DetailedLineRangeMapping],
    original_lines: &[JsString],
    modified_lines: &[JsString],
    hashed_original_lines: &[i32],
    hashed_modified_lines: &[i32],
    timeout: &dyn ITimeout,
) -> Vec<LineRangeMapping> {
    let (mut moves, excluded_changes) = compute_moves_from_simple_deletions_to_simple_insertions(changes, original_lines, modified_lines, timeout);

    if !timeout.is_valid() {
        return Vec::new();
    }

    let filtered_changes: Vec<DetailedLineRangeMapping> =
        changes.iter().enumerate().filter(|(index, _)| !excluded_changes.contains(index)).map(|(_, change)| change.clone()).collect();
    let unchanged_moves =
        compute_unchanged_moves(filtered_changes, hashed_original_lines, hashed_modified_lines, original_lines, modified_lines, timeout);
    push_many(&mut moves, &unchanged_moves);

    moves = join_close_consecutive_moves(moves);
    // Ignore too short moves
    moves.retain(|current| {
        let lines: Vec<JsString> =
            current.original.to_offset_range().slice(original_lines).iter().map(|line| JsString::from(trim(line.units()))).collect();
        let original_text_length: usize = lines.iter().map(JsString::len).sum::<usize>() + lines.len().saturating_sub(1);
        original_text_length >= 15 && lines.iter().filter(|line| line.len() >= 2).count() >= 2
    });
    moves = remove_moves_in_same_diff(changes, moves);

    moves
}

fn compute_moves_from_simple_deletions_to_simple_insertions(
    changes: &[DetailedLineRangeMapping],
    original_lines: &[JsString],
    modified_lines: &[JsString],
    timeout: &dyn ITimeout,
) -> (Vec<LineRangeMapping>, HashSet<usize>) {
    let mut moves: Vec<LineRangeMapping> = Vec::new();

    let deletions: Vec<LineRangeFragment> = changes
        .iter()
        .enumerate()
        .filter(|(_, c)| c.modified.is_empty() && c.original.length() >= 3)
        .map(|(index, d)| LineRangeFragment::new(d.original, original_lines, index))
        .collect();
    let mut insertions: Vec<Option<LineRangeFragment>> = changes
        .iter()
        .enumerate()
        .filter(|(_, c)| c.original.is_empty() && c.modified.length() >= 3)
        .map(|(index, d)| Some(LineRangeFragment::new(d.modified, modified_lines, index)))
        .collect();

    let mut excluded_changes: HashSet<usize> = HashSet::new();

    for deletion in &deletions {
        let mut highest_similarity = -1.0;
        let mut best: Option<usize> = None;
        for (index, insertion) in insertions.iter().enumerate() {
            let Some(insertion) = insertion else {
                continue;
            };
            let similarity = deletion.compute_similarity(insertion);
            if similarity > highest_similarity {
                highest_similarity = similarity;
                best = Some(index);
            }
        }

        if highest_similarity > 0.90
            && let Some(best) = best
            && let Some(best) = insertions[best].take()
        {
            moves.push(LineRangeMapping::new(deletion.range, best.range));
            excluded_changes.insert(deletion.source_index);
            excluded_changes.insert(best.source_index);
        }

        if !timeout.is_valid() {
            return (moves, excluded_changes);
        }
    }

    (moves, excluded_changes)
}

/// `PossibleMapping`.
struct PossibleMapping {
    modified_line_range: LineRange,
    original_line_range: LineRange,
}

fn compute_unchanged_moves(
    changes: Vec<DetailedLineRangeMapping>,
    hashed_original_lines: &[i32],
    hashed_modified_lines: &[i32],
    original_lines: &[JsString],
    modified_lines: &[JsString],
    timeout: &dyn ITimeout,
) -> Vec<LineRangeMapping> {
    let mut changes = changes;
    let mut moves: Vec<LineRangeMapping> = Vec::new();

    let mut original_3_line_hashes: HashMap<(i32, i32, i32), Vec<LineRange>> = HashMap::new();

    for change in &changes {
        for i in change.original.start_line_number..change.original.end_line_number_exclusive - 2 {
            let key = (
                hashed_original_lines[(i - 1) as usize],
                hashed_original_lines[(i + 1 - 1) as usize],
                hashed_original_lines[(i + 2 - 1) as usize],
            );
            original_3_line_hashes.entry(key).or_default().push(LineRange::new(i, i + 3));
        }
    }

    let mut possible_mappings: Vec<PossibleMapping> = Vec::new();

    changes.sort_by_key(|c| c.modified.start_line_number);

    for change in &changes {
        let mut last_mappings: Vec<usize> = Vec::new();
        for i in change.modified.start_line_number..change.modified.end_line_number_exclusive - 2 {
            let key = (
                hashed_modified_lines[(i - 1) as usize],
                hashed_modified_lines[(i + 1 - 1) as usize],
                hashed_modified_lines[(i + 2 - 1) as usize],
            );
            let current_modified_range = LineRange::new(i, i + 3);

            let mut next_mappings: Vec<usize> = Vec::new();
            for range in original_3_line_hashes.get(&key).map(Vec::as_slice).unwrap_or_default() {
                // does this match extend some last match?
                let extended = last_mappings.iter().copied().find(|&last_mapping| {
                    possible_mappings[last_mapping].original_line_range.end_line_number_exclusive + 1 == range.end_line_number_exclusive
                        && possible_mappings[last_mapping].modified_line_range.end_line_number_exclusive + 1
                            == current_modified_range.end_line_number_exclusive
                });
                if let Some(last_mapping) = extended {
                    let last = &mut possible_mappings[last_mapping];
                    last.original_line_range = LineRange::new(last.original_line_range.start_line_number, range.end_line_number_exclusive);
                    last.modified_line_range =
                        LineRange::new(last.modified_line_range.start_line_number, current_modified_range.end_line_number_exclusive);
                    next_mappings.push(last_mapping);
                    continue;
                }

                possible_mappings.push(PossibleMapping { modified_line_range: current_modified_range, original_line_range: *range });
                next_mappings.push(possible_mappings.len() - 1);
            }
            last_mappings = next_mappings;
        }

        if !timeout.is_valid() {
            return Vec::new();
        }
    }

    possible_mappings.sort_by(|a, b| b.modified_line_range.length().cmp(&a.modified_line_range.length()));

    let mut modified_set = LineRangeSet::default();
    let mut original_set = LineRangeSet::default();

    for mapping in &possible_mappings {
        let diff_orig_to_mod = mapping.modified_line_range.start_line_number - mapping.original_line_range.start_line_number;
        let modified_sections = modified_set.subtract_from(&mapping.modified_line_range);
        let original_translated_sections = original_set.subtract_from(&mapping.original_line_range).get_with_delta(diff_orig_to_mod);

        let modified_intersected_sections = modified_sections.get_intersection(&original_translated_sections);

        for s in modified_intersected_sections.ranges() {
            if s.length() < 3 {
                continue;
            }
            let modified_line_range = *s;
            let original_line_range = s.delta(-diff_orig_to_mod);

            moves.push(LineRangeMapping::new(original_line_range, modified_line_range));

            modified_set.add_range(modified_line_range);
            original_set.add_range(original_line_range);
        }
    }

    moves.sort_by_key(|m| m.original.start_line_number);

    for i in 0..moves.len() {
        let move_ = moves[i];
        let first_touching_change_orig = find_last_monotonous(&changes, |c| c.original.start_line_number <= move_.original.start_line_number)
            .expect("há uma mudança que começa antes do movimento");
        let first_touching_change_mod = find_last_monotonous(&changes, |c| c.modified.start_line_number <= move_.modified.start_line_number)
            .expect("há uma mudança que começa antes do movimento");
        let lines_above = (move_.original.start_line_number - first_touching_change_orig.original.start_line_number)
            .max(move_.modified.start_line_number - first_touching_change_mod.modified.start_line_number);

        let last_touching_change_orig = find_last_monotonous(&changes, |c| c.original.start_line_number < move_.original.end_line_number_exclusive)
            .expect("há uma mudança que começa antes do fim do movimento");
        let last_touching_change_mod = find_last_monotonous(&changes, |c| c.modified.start_line_number < move_.modified.end_line_number_exclusive)
            .expect("há uma mudança que começa antes do fim do movimento");
        let lines_below = (last_touching_change_orig.original.end_line_number_exclusive - move_.original.end_line_number_exclusive)
            .max(last_touching_change_mod.modified.end_line_number_exclusive - move_.modified.end_line_number_exclusive);

        let mut extend_to_top = 0;
        while extend_to_top < lines_above {
            let orig_line = move_.original.start_line_number - extend_to_top - 1;
            let mod_line = move_.modified.start_line_number - extend_to_top - 1;
            if orig_line as usize > original_lines.len() || mod_line as usize > modified_lines.len() {
                break;
            }
            if modified_set.contains(mod_line) || original_set.contains(orig_line) {
                break;
            }
            if !are_lines_similar(&original_lines[(orig_line - 1) as usize], &modified_lines[(mod_line - 1) as usize], timeout) {
                break;
            }
            extend_to_top += 1;
        }

        if extend_to_top > 0 {
            original_set.add_range(LineRange::new(move_.original.start_line_number - extend_to_top, move_.original.start_line_number));
            modified_set.add_range(LineRange::new(move_.modified.start_line_number - extend_to_top, move_.modified.start_line_number));
        }

        let mut extend_to_bottom = 0;
        while extend_to_bottom < lines_below {
            let orig_line = move_.original.end_line_number_exclusive + extend_to_bottom;
            let mod_line = move_.modified.end_line_number_exclusive + extend_to_bottom;
            if orig_line as usize > original_lines.len() || mod_line as usize > modified_lines.len() {
                break;
            }
            if modified_set.contains(mod_line) || original_set.contains(orig_line) {
                break;
            }
            if !are_lines_similar(&original_lines[(orig_line - 1) as usize], &modified_lines[(mod_line - 1) as usize], timeout) {
                break;
            }
            extend_to_bottom += 1;
        }

        if extend_to_bottom > 0 {
            original_set.add_range(LineRange::new(move_.original.end_line_number_exclusive, move_.original.end_line_number_exclusive + extend_to_bottom));
            modified_set.add_range(LineRange::new(move_.modified.end_line_number_exclusive, move_.modified.end_line_number_exclusive + extend_to_bottom));
        }

        if extend_to_top > 0 || extend_to_bottom > 0 {
            moves[i] = LineRangeMapping::new(
                LineRange::new(move_.original.start_line_number - extend_to_top, move_.original.end_line_number_exclusive + extend_to_bottom),
                LineRange::new(move_.modified.start_line_number - extend_to_top, move_.modified.end_line_number_exclusive + extend_to_bottom),
            );
        }
    }

    moves
}

fn are_lines_similar(line1: &JsString, line2: &JsString, timeout: &dyn ITimeout) -> bool {
    if trim(line1.units()) == trim(line2.units()) {
        return true;
    }
    if line1.len() > 300 && line2.len() > 300 {
        return false;
    }

    let myers_diffing_algorithm = MyersDiffAlgorithm;
    let result = myers_diffing_algorithm.compute(
        &LinesSliceCharSequence::new(std::slice::from_ref(line1), Range::new(1, 1, 1, line1.len() as i32), false),
        &LinesSliceCharSequence::new(std::slice::from_ref(line2), Range::new(1, 1, 1, line2.len() as i32), false),
        Some(timeout),
    );
    let mut common_non_space_char_count = 0;
    let inverted = SequenceDiff::invert(&result.diffs, line1.len() as i32);
    for seq in inverted {
        seq.seq1_range.for_each(|idx| {
            let char_code = line1.units().get(idx as usize).map_or(-1, |&unit| i32::from(unit));
            if !is_space(char_code) {
                common_non_space_char_count += 1;
            }
        });
    }

    // `countNonWsChars` lê `line1.length` posições de `str`: uma posição além de `str` (`NaN`) não é um espaço.
    let count_non_ws_chars = |str: &JsString| -> i32 {
        let mut count = 0;
        for i in 0..line1.len() {
            let char_code = str.units().get(i).map_or(-1, |&unit| i32::from(unit));
            if !is_space(char_code) {
                count += 1;
            }
        }
        count
    };

    let longer_line_length = count_non_ws_chars(if line1.len() > line2.len() { line1 } else { line2 });
    f64::from(common_non_space_char_count) / f64::from(longer_line_length) > 0.6 && longer_line_length > 10
}

fn join_close_consecutive_moves(moves: Vec<LineRangeMapping>) -> Vec<LineRangeMapping> {
    if moves.is_empty() {
        return moves;
    }

    let mut moves = moves;
    moves.sort_by_key(|m| m.original.start_line_number);

    let mut result = vec![moves[0]];
    for &current in &moves[1..] {
        let last = result[result.len() - 1];

        let original_dist = current.original.start_line_number - last.original.end_line_number_exclusive;
        let modified_dist = current.modified.start_line_number - last.modified.end_line_number_exclusive;
        let current_move_after_last = original_dist >= 0 && modified_dist >= 0;

        if current_move_after_last && original_dist + modified_dist <= 2 {
            let last_index = result.len() - 1;
            result[last_index] = last.join(&current);
            continue;
        }

        result.push(current);
    }
    result
}

fn remove_moves_in_same_diff(changes: &[DetailedLineRangeMapping], moves: Vec<LineRangeMapping>) -> Vec<LineRangeMapping> {
    moves
        .into_iter()
        .filter(|m| {
            let diff_before_end_of_move_original =
                find_last_idx_monotonous(changes, |c| c.original.start_line_number < m.original.end_line_number_exclusive, 0, changes.len());
            let diff_before_end_of_move_modified =
                find_last_idx_monotonous(changes, |c| c.modified.start_line_number < m.modified.end_line_number_exclusive, 0, changes.len());

            // Sem mudança antes do fim do movimento, o original usa um `LineRangeMapping` novo, que nunca é igual a outro.
            diff_before_end_of_move_original < 0 || diff_before_end_of_move_original != diff_before_end_of_move_modified
        })
        .collect()
}
