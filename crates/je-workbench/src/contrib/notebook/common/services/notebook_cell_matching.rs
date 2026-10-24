//! Porte de `src/vs/workbench/contrib/notebook/common/services/notebookCellMatching.ts`: o casamento das células de um
//! notebook modificado com as de um original por similaridade (distância de Levenshtein do texto).
//!
//! Divergências:
//! - `type ICell` é o trait [`ICell`]; `internalMetadata?.internalId` é [`ICell::internal_id`], e as células chegam como
//!   `Rc<dyn ICell>` (o worker de notebook as compartilha com os documentos espelho).
//! - Os índices de célula são `i64` (o `-1` de "sem par" é um valor); `editCount` e `percentage` são `f64`, porque o
//!   original usa `Infinity` e `Number.MAX_SAFE_INTEGER` ([`MAX_SAFE_INTEGER`]).
//! - Os `Map` do cache guardam a ordem de inserção (o `Array.from(matchesForThisOriginalIndex).find(...)` do original
//!   devolve o primeiro par que atende), por isso são `IndexMap`. Os dois mapas ficam em `RefCell`, porque o predicado
//!   do RULE 1 lê `cache.originalToModified` enquanto `computeClosestCell` ainda o preenche. O `{ editCount }` guardado nos
//!   mapas é imutável, e os dois mapas guardam o mesmo valor; aqui é o próprio `f64`.
//! - `computeLevenshteinDistance` recebe as unidades UTF-16 do texto (`JsString::units`).
//! - O resultado devolve só `{ modified, original, percentage }` ([`CellMatch`]), que é o tipo declarado do original
//!   (os campos `dist` e `possibleOriginal` existem em tempo de execução, mas nenhum chamador os lê).

use std::cell::RefCell;
use std::collections::{HashMap, HashSet};
use std::rc::Rc;

use indexmap::IndexMap;
use je_base::JsString;
use je_base::diff::compute_levenshtein_distance;
use je_base::js_string::index_of;

use crate::contrib::notebook::common::notebook_common::CellKind;

/// `Number.MAX_SAFE_INTEGER`.
pub const MAX_SAFE_INTEGER: f64 = 9_007_199_254_740_991.0;

/// `ICell`.
pub trait ICell {
    /// `internalMetadata?.internalId`.
    fn internal_id(&self) -> Option<String>;
    fn get_value(&self) -> JsString;
    fn get_lines_content(&self) -> Vec<JsString>;
    fn cell_kind(&self) -> CellKind;
}

/// O elemento do resultado de [`match_cell_based_on_similarties`].
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct CellMatch {
    pub modified: i64,
    pub original: i64,
    pub percentage: f64,
}

struct CellEditCountCache {
    modified_to_original: RefCell<IndexMap<i64, IndexMap<i64, f64>>>,
    original_to_modified: RefCell<IndexMap<i64, IndexMap<i64, f64>>>,
}

struct MatchResult {
    modified: i64,
    original: i64,
    dist: f64,
    percentage: f64,
    possible_original: i64,
}

struct ClosestCell {
    index: i64,
    edit_count: f64,
    percentage: f64,
}

fn includes(haystack: &JsString, needle: &JsString) -> bool {
    index_of(haystack.units(), needle.units(), 0).is_some()
}

/// Given a set of modified cells and original cells, this function will attempt to match the modified cells with the original cells.
/// E.g. Assume you have (original on left and modified on right):
/// ```text
/// =================
/// Cell A  | Cell a
/// Cell B  | Cell b
/// Cell C  | Cell d
/// Cell D  | Cell e
/// =================
/// ```
/// Here we know that `Cell C` has been removed and `Cell e` has been added.
/// The mapping from modified to original will be as follows:
/// Cell a => Cell A
/// Cell b => Cell B
/// Cell d => Cell D
/// Cell e => <Does not match anything in original, hence a new Cell>
/// Cell C in original was not matched, hence it was deleted.
///
/// Thus the return value is as follows:
/// ```text
/// [
/// { modified: 0, original: 0 },
/// { modified: 1, original: 1 },
/// { modified: 2, original: 3 },
/// { modified: 3, original: -1 },
/// ]
/// ```
pub fn match_cell_based_on_similarties(modified_cells: &[Rc<dyn ICell>], original_cells: &[Rc<dyn ICell>]) -> Vec<CellMatch> {
    let cache = CellEditCountCache { modified_to_original: RefCell::new(IndexMap::new()), original_to_modified: RefCell::new(IndexMap::new()) };
    let mut results: Vec<MatchResult> = Vec::new();
    let mut mapped_original_cell_to_modified_cell: HashMap<i64, i64> = HashMap::new();
    let mut mapped_modified_indexes: HashSet<i64> = HashSet::new();
    // `{ dist, modifiedIndex }`: só o `dist` é lido.
    let mut original_index_with_most_edits: HashMap<i64, f64> = HashMap::new();

    for (i, modified_cell) in modified_cells.iter().enumerate() {
        let i = i as i64;
        let ClosestCell { index, edit_count: dist, percentage } = compute_closest_cell(
            modified_cell.as_ref(),
            i,
            original_cells,
            true,
            &cache,
            &|original_index, edit_count| can_original_index_be_mapped_to_modified_index(&mapped_original_cell_to_modified_cell, &original_index_with_most_edits, original_index, edit_count),
        );
        if index >= 0 && dist == 0.0 {
            track_mapped_indexes(&mut mapped_original_cell_to_modified_cell, &mut mapped_modified_indexes, i, index);
            results.push(MatchResult { modified: i, original: index, dist, percentage, possible_original: index });
        } else {
            original_index_with_most_edits.insert(index, dist);
            results.push(MatchResult { modified: i, original: -1, dist, percentage, possible_original: index });
        }
    }

    for i in 0..results.len() {
        if results[i].original >= 0 {
            continue;
        }
        let i_signed = i as i64;

        // I.e. Assume you have the following
        // =================
        // A a (this has ben matched)
        // B b <not matched>
        // C c <not matched>
        // D d (these two have been matched)
        // e e
        // f f
        // =================
        // Just match A => a, B => b, C => c

        // Find the next cell that has been matched.
        let previous_matched_cell = if i > 0 { results[..i].iter().rev().find(|r| r.original >= 0) } else { None };
        let previous_matched_original_index = previous_matched_cell.map_or(-1, |r| r.original);
        let previous_matched_modified_index = previous_matched_cell.map_or(-1, |r| r.modified);
        let matched_cell = results[i + 1..].iter().find(|r| r.original >= 0);
        let mut unavailable_indexes: HashSet<i64> = HashSet::new();
        let next_matched_modified_index = results.iter().enumerate().position(|(idx, item)| idx > i && item.original >= 0).map_or(-1, |idx| idx as i64);
        let next_matched_original_index = if next_matched_modified_index >= 0 { results[next_matched_modified_index as usize].original } else { -1 };
        // Find the available indexes that we can match with.
        // We are only interested in b and c (anything after d is of no use).
        for original_index in 0..original_cells.len() as i64 {
            if mapped_original_cell_to_modified_cell.contains_key(&original_index) {
                unavailable_indexes.insert(original_index);
                continue;
            }
            if let Some(matched_cell) = matched_cell
                && original_index >= matched_cell.original
            {
                unavailable_indexes.insert(original_index);
            }
            if next_matched_original_index >= 0 && original_index > next_matched_original_index {
                unavailable_indexes.insert(original_index);
            }
        }

        let modified_cell = &modified_cells[i];

        // I.e. Assume you have the following
        // =================
        // A a (this has ben matched)
        // B b <not matched because the % of change is too high, but we do have a probable match>
        // C c <not matched>
        // D d (these two have been matched)
        // e e
        // f f
        // =================
        // Given that we have a probable match for B => b, we can match it.
        let possible_original = results[i].possible_original;
        if results[i].original == -1
            && possible_original >= 0
            && !unavailable_indexes.contains(&possible_original)
            && can_original_index_be_mapped_to_modified_index(&mapped_original_cell_to_modified_cell, &original_index_with_most_edits, possible_original, results[i].dist)
        {
            track_mapped_indexes(&mut mapped_original_cell_to_modified_cell, &mut mapped_modified_indexes, i_signed, possible_original);
            results[i].original = possible_original;
            continue;
        }

        // I.e. Assume you have the following
        // =================
        // A a (this has ben matched)
        // B b <not matched>
        // C c <not matched>
        // D d (these two have been matched)
        // =================
        // Its possible that B matches better with c and C matches better with b.
        // However given the fact that we have matched A => a and D => d.
        // & if the indexes are an exact match.
        // I.e. index of D in Modified === index of d in Original, and index of A in Modified === index of a in Original.
        // Then this means there are absolutely no modifications.
        // Hence we can just assign the indexes as is.
        //
        // NOTE: For this, we must ensure we have exactly the same number of items on either side.
        // I.e. we have B, C remaining in Modified, and b, c remaining in Original.
        // Thats 2 Modified items === 2 Original Items.
        // If its not the same, then this means something has been deleted/inserted, and we cannot blindly map the indexes.
        if previous_matched_original_index > 0 && previous_matched_modified_index > 0 && previous_matched_original_index == previous_matched_modified_index {
            let next_modified = if next_matched_modified_index >= 0 { next_matched_modified_index } else { modified_cells.len() as i64 - 1 };
            let next_original = if next_matched_original_index >= 0 { next_matched_original_index } else { original_cells.len() as i64 - 1 };
            if next_modified == next_original && !unavailable_indexes.contains(&i_signed) && i < original_cells.len() {
                let remaining_modified_items =
                    (if next_matched_modified_index >= 0 { next_matched_modified_index } else { modified_cells.len() as i64 }) - previous_matched_modified_index;
                let remaining_original_items =
                    (if next_matched_original_index >= 0 { next_matched_original_index } else { original_cells.len() as i64 }) - previous_matched_original_index;
                if remaining_modified_items == remaining_original_items && modified_cell.cell_kind() == original_cells[i].cell_kind() {
                    track_mapped_indexes(&mut mapped_original_cell_to_modified_cell, &mut mapped_modified_indexes, i_signed, i_signed);
                    results[i].original = i_signed;
                    continue;
                }
            }
        }

        // I.e. Assume you have the following
        // =================
        // A a (this has ben matched)
        // B b <not matched>
        // C c <not matched>
        // D d (these two have been matched)
        // e e
        // f f
        // =================
        // We can now try to match B with b and c and figure out which is best.
        // RULE 1. Its possible that B will match best with c, howevber C matches better with c, meaning we should match B with b.
        // To do this, we need to see if c has a better match with something else.

        // RULE 1
        // Try to find the next best match, but exclucde items that have a better match.
        let ClosestCell { index, edit_count: _, percentage } = compute_closest_cell(modified_cell.as_ref(), i_signed, original_cells, false, &cache, &|original_index, original_edit_count| {
            if unavailable_indexes.contains(&original_index) {
                return false;
            }

            if next_matched_modified_index > 0 || previous_matched_original_index > 0 {
                // See if we have a beter match for this.
                let original_to_modified = cache.original_to_modified.borrow();
                let matches_for_this_original_index = original_to_modified.get(&original_index);
                if let Some(matches_for_this_original_index) = matches_for_this_original_index
                    && previous_matched_original_index < original_index
                {
                    let better_match = matches_for_this_original_index.iter().find(|(modified_index, edit_count)| {
                        if **modified_index == i_signed {
                            // This is the same modifeid entry.
                            return false;
                        }
                        if **modified_index >= next_matched_modified_index {
                            // We're only interested in matches that are before the next matched index.
                            return false;
                        }
                        if mapped_modified_indexes.contains(&i_signed) {
                            // This has already been matched.
                            return false;
                        }
                        **edit_count < original_edit_count
                    });
                    if better_match.is_some() {
                        // We do have a better match for this, hence do not use this.
                        return false;
                    }
                }
            }
            !unavailable_indexes.contains(&original_index)
        });

        // I.e. Assume you have the following
        // =================
        // A a (this has ben matched)
        // B bbbbbbbbbbbbbb <not matched>
        // C cccccccccccccc <not matched>
        // D d (these two have been matched)
        // e e
        // f f
        // =================
        // RULE 1 . Now when attempting to match `bbbbbbbbbbbb` with B, the number of edits is very high and the percentage is also very high.
        // Basically majority of the text needs to be changed.
        // However if the indexes line up perfectly well, and this is the best match, then use it.
        //
        // Similarly its possible we're trying to match b with `BBBBBBBBBBBB` and the number of edits is very high, but the indexes line up perfectly well.
        //
        // RULE 2. However it is also possible that there's a better match with another cell
        // Assume we have
        // =================
        // AAAA     a (this has been matched)
        // bbbbbbbb b <not matched>
        // bbbb     c <not matched>
        // dddd     d (these two have been matched)
        // =================
        // In this case if we use the algorithm of (1) above, we'll end up matching bbbb with b, and bbbbbbbb with c.
        // But we're not really sure if this is the best match.
        // In such cases try to match with the same cell index.

        // RULE 1 (got a match and the indexes line up perfectly well, use it regardless of the number of edits).
        if index >= 0 && i > 0 && results[i - 1].original == index - 1 {
            track_mapped_indexes(&mut mapped_original_cell_to_modified_cell, &mut mapped_modified_indexes, i_signed, index);
            results[i].original = index;
            continue;
        }

        // RULE 2
        // Here we know that `AAAA => a`
        // Check if the previous cell has been matched.
        // And if the next modified and next original cells are a match.
        let next_original_cell = if i > 0 && original_cells.len() as i64 > results[i - 1].original { results[i - 1].original + 1 } else { -1 };
        let next_original_cell_value = if i > 0 && next_original_cell >= 0 && next_original_cell < original_cells.len() as i64 {
            Some(original_cells[next_original_cell as usize].get_value())
        } else {
            None
        };
        if index >= 0
            && i > 0
            && let Some(next_original_cell_value) = &next_original_cell_value
            && !mapped_original_cell_to_modified_cell.contains_key(&next_original_cell)
        {
            let modified_value = modified_cell.get_value();
            if includes(&modified_value, next_original_cell_value) || includes(next_original_cell_value, &modified_value) {
                track_mapped_indexes(&mut mapped_original_cell_to_modified_cell, &mut mapped_modified_indexes, i_signed, next_original_cell);
                results[i].original = next_original_cell;
                continue;
            }
        }

        if percentage < 90.0 || (i == 0 && results.len() == 1) {
            track_mapped_indexes(&mut mapped_original_cell_to_modified_cell, &mut mapped_modified_indexes, i_signed, index);
            results[i].original = index;
        }
    }

    results.into_iter().map(|r| CellMatch { modified: r.modified, original: r.original, percentage: r.percentage }).collect()
}

/// `canOriginalIndexBeMappedToModifiedIndex`.
fn can_original_index_be_mapped_to_modified_index(
    mapped_original_cell_to_modified_cell: &HashMap<i64, i64>,
    original_index_with_most_edits: &HashMap<i64, f64>,
    original_index: i64,
    edit_count: f64,
) -> bool {
    if mapped_original_cell_to_modified_cell.contains_key(&original_index) {
        return false;
    }
    let existing_edits = original_index_with_most_edits.get(&original_index).copied().unwrap_or(MAX_SAFE_INTEGER);
    edit_count < existing_edits
}

/// `trackMappedIndexes`.
fn track_mapped_indexes(
    mapped_original_cell_to_modified_cell: &mut HashMap<i64, i64>,
    mapped_modified_indexes: &mut HashSet<i64>,
    modified_index: i64,
    original_index: i64,
) {
    mapped_original_cell_to_modified_cell.insert(original_index, modified_index);
    mapped_modified_indexes.insert(modified_index);
}

fn compute_closest_cell(
    cell: &dyn ICell,
    cell_index: i64,
    arr: &[Rc<dyn ICell>],
    ignore_empty_cells: bool,
    cache: &CellEditCountCache,
    can_original_index_be_mapped_to_modified_index: &dyn Fn(i64, f64) -> bool,
) -> ClosestCell {
    let mut min_edits = f64::INFINITY;
    let mut min_index: i64 = -1;

    // Always give preference to internal Cell Id if found.
    if let Some(internal_id) = cell.internal_id().filter(|id| !id.is_empty())
        && let Some(internal_id_index) = arr.iter().position(|cell| cell.internal_id().as_ref() == Some(&internal_id))
    {
        return ClosestCell { index: internal_id_index as i64, edit_count: 0.0, percentage: MAX_SAFE_INTEGER };
    }

    let cell_value = cell.get_value();
    for (i, original_cell) in arr.iter().enumerate() {
        let i = i as i64;
        // Skip cells that are not of the same kind.
        if original_cell.cell_kind() != cell.cell_kind() {
            continue;
        }
        let str = original_cell.get_value();
        let existing = cache.modified_to_original.borrow().get(&cell_index).and_then(|entry| entry.get(&i)).copied();
        let edit_count = existing.unwrap_or_else(|| compute_number_of_edits(cell, original_cell.as_ref()));
        cache.modified_to_original.borrow_mut().entry(cell_index).or_default().insert(i, edit_count);
        cache.original_to_modified.borrow_mut().entry(i).or_default().insert(cell_index, edit_count);

        if !can_original_index_be_mapped_to_modified_index(i, edit_count) {
            continue;
        }
        if str.is_empty() && ignore_empty_cells {
            continue;
        }
        if str == cell_value && !cell_value.is_empty() {
            return ClosestCell { index: i, edit_count: 0.0, percentage: 0.0 };
        }

        if edit_count < min_edits {
            min_edits = edit_count;
            min_index = i;
        }
    }

    if min_index == -1 {
        return ClosestCell { index: -1, edit_count: MAX_SAFE_INTEGER, percentage: MAX_SAFE_INTEGER };
    }
    let percentage = if cell_value.is_empty() && arr[min_index as usize].get_value().is_empty() {
        0.0
    } else if !cell_value.is_empty() {
        min_edits * 100.0 / cell_value.len() as f64
    } else {
        MAX_SAFE_INTEGER
    };
    ClosestCell { index: min_index, edit_count: min_edits, percentage }
}

fn compute_number_of_edits(modified: &dyn ICell, original: &dyn ICell) -> f64 {
    let (modified, original) = (modified.get_value(), original.get_value());
    if modified == original {
        return 0.0;
    }

    f64::from(compute_levenshtein_distance(modified.units(), original.units()))
}
