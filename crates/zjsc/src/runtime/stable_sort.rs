//! Porte de `runtime/StableSort.h`: a ordenação estável por Powersort (`arrayStableSort` nas duas
//! `MergeStrategy`: `Simple` do `%TypedArray%.prototype.sort` e `toSorted`, `Galloping` do
//! `Array.prototype.sort` e `toSorted`) com `arrayInsertionSort`, `extendAndNormalizeRun`,
//! `mergeRunsSimple`, `gallopLeft`, `gallopRight`, `mergePowersortRuns` e
//! `coerceComparatorResultToBoolean`.
//!
//! DIVERGÊNCIAS:
//!
//! - O `Functor` do C++ devolve `bool` e deixa a exceção pendente no `VM` (`RETURN_IF_EXCEPTION`); aqui o
//!   comparador devolve `Result<bool, Thrown>` e o `Err` interrompe a ordenação, que deixa o vetor num
//!   estado parcial como o C++ (quem chama descarta o vetor).
//! - `WTF::Vector<PowersortStackEntry, 64>` é um `Vec`; `UInt128` é `u128`.

use crate::runtime::host_call::Thrown;
use crate::runtime::js_global_object::JSGlobalObject;
use crate::runtime::js_value::JSValue;
use crate::runtime::string_regexp_support::check_exception;

/// `coerceComparatorResultToBoolean(globalObject, comparatorResult)`: o resultado do comparador como
/// "a vem antes de b".
pub fn coerce_comparator_result_to_boolean(global_object: &JSGlobalObject, comparator_result: JSValue) -> Result<bool, Thrown> {
    if let JSValue::Int32(integer) = comparator_result {
        return Ok(integer < 0);
    }

    // See https://bugs.webkit.org/show_bug.cgi?id=47825 on boolean special-casing
    if comparator_result.is_boolean() {
        return Ok(!comparator_result.as_boolean());
    }

    let number = comparator_result.to_number();
    check_exception(global_object)?;
    Ok(number < 0.0)
}

/// `arrayInsertionSort(vm, span, comparator, sortedHeader)`.
fn array_insertion_sort<T: Copy>(
    span: &mut [T],
    comparator: &mut impl FnMut(T, T) -> Result<bool, Thrown>,
    sorted_header: usize,
) -> Result<(), Thrown> {
    let length = span.len();
    for i in (sorted_header + 1)..length {
        let value = span[i];

        // [l, r)
        let mut left = 0;
        let mut right = i;
        while left < right {
            let m = left + (right - left) / 2;
            let target = span[m];
            if !comparator(value, target)? {
                left = m + 1;
            } else {
                right = m;
            }
        }
        let mut t = value;
        for j in left..i {
            std::mem::swap(&mut span[j], &mut t);
        }
        span[i] = t;
    }
    Ok(())
}

/// `extendAndNormalizeRun(vm, span, begin, comparator)`: detecta a sequência crescente ou estritamente
/// decrescente que começa em `begin` (e a inverte, se decrescente). Devolve o índice final (inclusivo).
fn extend_and_normalize_run<T: Copy>(
    span: &mut [T],
    begin: usize,
    comparator: &mut impl FnMut(T, T) -> Result<bool, Thrown>,
) -> Result<usize, Thrown> {
    let mut end = begin;
    let num_elements = span.len();
    if end + 1 >= num_elements {
        return Ok(end);
    }

    // Check if the run starts descending: a[begin+1] < a[begin] (strict for stability).
    let descending = comparator(span[end + 1], span[end])?;

    if descending {
        // Extend strictly descending run.
        end += 1;
        while end + 1 < num_elements {
            if !comparator(span[end + 1], span[end])? {
                break;
            }
            end += 1;
        }
        span[begin..=end].reverse();
    } else {
        // Extend ascending run (non-strictly: a[i+1] >= a[i]).
        end += 1;
        while end + 1 < num_elements {
            if comparator(span[end + 1], span[end])? {
                break;
            }
            end += 1;
        }
    }

    Ok(end)
}

/// `mergeRunsSimple(vm, dst, src, srcIndex1, srcEnd1, srcIndex2, srcEnd2, comparator)`.
fn merge_runs_simple<T: Copy>(
    dst: &mut [T],
    src: &[T],
    src_index1: usize,
    src_end1: usize,
    src_index2: usize,
    src_end2: usize,
    comparator: &mut impl FnMut(T, T) -> Result<bool, Thrown>,
) -> Result<(), Thrown> {
    let mut left = src_index1;
    let left_end = src_end1;
    let mut right = src_index2;
    let right_end = src_end2;

    debug_assert!(left_end <= right);
    debug_assert!(right_end <= src.len());

    for dst_index in left..right_end {
        if right < right_end {
            if left >= left_end {
                dst[dst_index] = src[right];
                right += 1;
                continue;
            }
            if comparator(src[right], src[left])? {
                dst[dst_index] = src[right];
                right += 1;
                continue;
            }
        }
        dst[dst_index] = src[left];
        left += 1;
    }
    Ok(())
}

/// `nextOffset = (offset << 1) + 1; offset = nextOffset > offset ? min(nextOffset, maxOffset) : maxOffset`.
fn next_gallop_offset(offset: usize, max_offset: usize) -> usize {
    let next_offset = (offset << 1).wrapping_add(1);
    if next_offset > offset { next_offset.min(max_offset) } else { max_offset }
}

/// `gallopLeft(vm, key, base, length, hint, comparator)`: a posição mais à esquerda onde `key` entra
/// (antes dos iguais), `k` em `[0, length]` com `base[k-1] < key <= base[k]`. A aritmética de `size_t`
/// dá a volta como no C++ (`lastOffset = hint - offset` com `offset == hint + 1`, depois `++lastOffset`).
fn gallop_left<T: Copy>(
    key: T,
    base: &[T],
    hint: usize,
    comparator: &mut impl FnMut(T, T) -> Result<bool, Thrown>,
) -> Result<usize, Thrown> {
    let length = base.len();
    debug_assert!(hint < length);

    let mut last_offset: usize = 0;
    let mut offset: usize = 1;

    // base[hint] < key => gallop right
    let hint_less_than_key = comparator(base[hint], key)?;

    if hint_less_than_key {
        // Gallop right: a[hint] < key, so search in (hint, length).
        let max_offset = length - hint;
        while offset < max_offset {
            if !comparator(base[hint + offset], key)? {
                break;
            }
            last_offset = offset;
            offset = next_gallop_offset(offset, max_offset);
        }
        last_offset += hint;
        offset += hint;
    } else {
        // Gallop left: a[hint] >= key, so search in [0, hint).
        let max_offset = hint + 1;
        while offset < max_offset {
            if comparator(base[hint - offset], key)? {
                break;
            }
            last_offset = offset;
            offset = next_gallop_offset(offset, max_offset);
        }
        let tmp = last_offset;
        last_offset = hint.wrapping_sub(offset);
        offset = hint - tmp;
    }

    // Now base[lastOffset] < key <= base[offset], binary search in (lastOffset, offset].
    last_offset = last_offset.wrapping_add(1);
    while last_offset < offset {
        let m = last_offset + ((offset - last_offset) >> 1);
        if comparator(base[m], key)? {
            last_offset = m + 1;
        } else {
            offset = m;
        }
    }

    Ok(offset)
}

/// `gallopRight(vm, key, base, length, hint, comparator)`: a posição mais à direita onde `key` entra
/// (depois dos iguais), `k` em `[0, length]` com `base[k-1] <= key < base[k]`.
fn gallop_right<T: Copy>(
    key: T,
    base: &[T],
    hint: usize,
    comparator: &mut impl FnMut(T, T) -> Result<bool, Thrown>,
) -> Result<usize, Thrown> {
    let length = base.len();
    debug_assert!(hint < length);

    let mut last_offset: usize = 0;
    let mut offset: usize = 1;

    // key < base[hint] => gallop left
    let key_less_than_hint = comparator(key, base[hint])?;

    if key_less_than_hint {
        // Gallop left: key < a[hint], so search in [0, hint).
        let max_offset = hint + 1;
        while offset < max_offset {
            if !comparator(key, base[hint - offset])? {
                break;
            }
            last_offset = offset;
            offset = next_gallop_offset(offset, max_offset);
        }
        // Translate back to positive offsets from base.
        let tmp = last_offset;
        last_offset = hint.wrapping_sub(offset);
        offset = hint - tmp;
    } else {
        // Gallop right: key >= a[hint], so search in (hint, length).
        let max_offset = length - hint;
        while offset < max_offset {
            if comparator(key, base[hint + offset])? {
                break;
            }
            last_offset = offset;
            offset = next_gallop_offset(offset, max_offset);
        }
        // Translate to absolute offsets.
        last_offset += hint;
        offset += hint;
    }

    // Now base[lastOffset] <= key < base[offset], binary search in (lastOffset, offset].
    last_offset = last_offset.wrapping_add(1);
    while last_offset < offset {
        let m = last_offset + ((offset - last_offset) >> 1);
        if comparator(key, base[m])? {
            offset = m;
        } else {
            last_offset = m + 1;
        }
    }

    Ok(offset)
}

/// `minGallopThreshold`.
const MIN_GALLOP_THRESHOLD: usize = 7;

/// `mergePowersortRuns(vm, dst, src, srcIndex1, srcEnd1, srcIndex2, srcEnd2, comparator, minGallop)`.
#[allow(clippy::too_many_arguments)]
fn merge_powersort_runs<T: Copy>(
    dst: &mut [T],
    src: &[T],
    src_index1: usize,
    src_end1: usize,
    src_index2: usize,
    src_end2: usize,
    comparator: &mut impl FnMut(T, T) -> Result<bool, Thrown>,
    min_gallop: &mut usize,
) -> Result<(), Thrown> {
    debug_assert!(src_end1 <= src_index2);
    debug_assert!(src_end2 <= src.len());

    let mut left_length = src_end1 - src_index1;
    let mut right_length = src_end2 - src_index2;

    // Either run is empty. Just copy entire two consecutive runs into destination.
    if left_length == 0 || right_length == 0 {
        dst[src_index1..src_end2].copy_from_slice(&src[src_index1..src_end2]);
        return Ok(());
    }

    // Pre-merge trim: skip leading elements of left that are already in place.
    let skip_left = gallop_right(src[src_index2], &src[src_index1..src_index1 + left_length], 0, comparator)?;

    // Copy the already-in-place leading elements.
    if skip_left != 0 {
        dst[src_index1..src_index1 + skip_left].copy_from_slice(&src[src_index1..src_index1 + skip_left]);
    }

    let mut left = src_index1 + skip_left;
    left_length -= skip_left;

    if left_length == 0 {
        // All of left is <= first element of right; just copy right.
        dst[src_index2..src_index2 + right_length].copy_from_slice(&src[src_index2..src_index2 + right_length]);
        return Ok(());
    }

    // Pre-merge trim: skip trailing elements of right that are already in place.
    let skip_right = right_length
        - gallop_left(src[src_end1 - 1], &src[src_index2..src_index2 + right_length], right_length - 1, comparator)?;

    // Copy the already-in-place trailing elements.
    if skip_right != 0 {
        dst[src_end2 - skip_right..src_end2].copy_from_slice(&src[src_end2 - skip_right..src_end2]);
    }

    let right_end = src_end2 - skip_right;
    right_length -= skip_right;

    if right_length == 0 {
        // All of right is >= last element of left; just copy left.
        dst[left..left + left_length].copy_from_slice(&src[left..left + left_length]);
        return Ok(());
    }

    debug_assert!(left_length != 0 && right_length != 0);

    // Merge with galloping mode. After pre-merge trim, new range is [left, leftEnd) and [right, rightEnd).
    let left_end = src_end1;
    let mut right = src_index2;
    let mut dst_index = left;

    'merge_finish: loop {
        // Linear merge until one side wins minGallop times consecutively.
        let mut left_wins = 0usize;
        let mut right_wins = 0usize;

        while left_wins < *min_gallop && right_wins < *min_gallop {
            if right < right_end && left < left_end {
                if comparator(src[right], src[left])? {
                    dst[dst_index] = src[right];
                    dst_index += 1;
                    right += 1;
                    right_wins += 1;
                    left_wins = 0;
                } else {
                    dst[dst_index] = src[left];
                    dst_index += 1;
                    left += 1;
                    left_wins += 1;
                    right_wins = 0;
                }
            } else {
                // One side of run gets exhausted. Let's just copy the rest.
                break 'merge_finish;
            }
        }

        // Galloping mode: one side is winning consistently.
        // Increase minGallop to make it harder to re-enter galloping (penalize leaving).
        *min_gallop += 1;

        loop {
            // Decrease minGallop while galloping is productive.
            if *min_gallop > 1 {
                *min_gallop -= 1;
            }

            if left >= left_end || right >= right_end {
                break 'merge_finish;
            }

            // Gallop in left run for right's current element.
            {
                let k = gallop_right(src[right], &src[left..left_end], 0, comparator)?;
                left_wins = k;
                if k != 0 {
                    // k elements in the left are lower than right elements. Just copy them.
                    dst[dst_index..dst_index + k].copy_from_slice(&src[left..left + k]);
                    dst_index += k;
                    left += k;
                }
                dst[dst_index] = src[right];
                dst_index += 1;
                right += 1;

                if left >= left_end || right >= right_end {
                    break 'merge_finish;
                }
            }

            // Gallop in right run for left's current element.
            {
                let k = gallop_left(src[left], &src[right..right_end], 0, comparator)?;
                right_wins = k;
                if k != 0 {
                    // k elements in the right are lower than left elements. Just copy them.
                    dst[dst_index..dst_index + k].copy_from_slice(&src[right..right + k]);
                    dst_index += k;
                    right += k;
                }
                dst[dst_index] = src[left];
                dst_index += 1;
                left += 1;

                if left >= left_end || right >= right_end {
                    break 'merge_finish;
                }
            }

            if left_wins < MIN_GALLOP_THRESHOLD && right_wins < MIN_GALLOP_THRESHOLD {
                break;
            }
        }

        // Leaving galloping mode; penalize.
        *min_gallop += 1;
    }

    // Copy remaining elements.
    while left < left_end {
        dst[dst_index] = src[left];
        dst_index += 1;
        left += 1;
    }
    while right < right_end {
        dst[dst_index] = src[right];
        dst_index += 1;
        right += 1;
    }
    Ok(())
}

/// `enum class MergeStrategy { Galloping, Simple }`: o `Array.prototype.sort` usa `Galloping`, o
/// `%TypedArray%.prototype.sort` usa `Simple`. A ordem das chamadas ao comparador é observável.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum MergeStrategy {
    Galloping,
    Simple,
}

/// A sequência ordenada `[m_begin, m_end]` do Powersort.
#[derive(Clone, Copy)]
struct SortedRun {
    begin: usize,
    end: usize,
}

/// A `power` do Powersort para as sequências `[left, middle - 1]` e `[middle, right]`.
fn power(left: usize, middle: usize, right: usize, n: usize) -> u32 {
    let n1 = (middle - left) as u128;
    let n2 = (right - middle + 1) as u128;
    // a and b are 2*midpoints of the two ranges, so always within [0, 2n)
    let mut a = left as u128 * 2 + n1;
    let mut b = middle as u128 * 2 + n2;
    a <<= 62;
    b <<= 62;

    let n = n as u128;
    let differing_bits = (a / n) ^ (b / n);
    debug_assert!(differing_bits >> 64 == 0);
    (differing_bits as u64).leading_zeros()
}

/// Estende a sequência que acaba em `run.end` enquanto o próximo elemento não for menor que o último, e
/// completa as curtas por inserção (`forceRunLength` 64, `extendRunCutoff` 8).
fn extend_run<T: Copy>(
    src: &mut [T],
    begin: usize,
    comparator: &mut impl FnMut(T, T) -> Result<bool, Thrown>,
) -> Result<SortedRun, Thrown> {
    const EXTEND_RUN_CUTOFF: usize = 8;
    const FORCE_RUN_LENGTH: usize = 64;

    let num_elements = src.len();
    let mut run = SortedRun { begin, end: extend_and_normalize_run(src, begin, comparator)? };

    if run.end - run.begin < EXTEND_RUN_CUTOFF {
        // If the run is too short, insertion sort a bit
        let size = FORCE_RUN_LENGTH.min(num_elements - run.begin);
        array_insertion_sort(&mut src[run.begin..run.begin + size], comparator, run.end - run.begin)?;
        run.end = run.begin + size - 1;
    }

    // See if we can extend the run any more.
    while run.end + 1 < num_elements {
        if comparator(src[run.end + 1], src[run.end])? {
            break;
        }
        run.end += 1;
    }
    Ok(run)
}

/// Funde `[range.begin, range.end]` com `[run1.begin, run1.end]` por `working_set` e copia de volta.
fn merge_into_source<T: Copy>(
    merge_strategy: MergeStrategy,
    src: &mut [T],
    working_set: &mut [T],
    range_to_merge: SortedRun,
    run1: SortedRun,
    comparator: &mut impl FnMut(T, T) -> Result<bool, Thrown>,
    min_gallop: &mut usize,
) -> Result<(), Thrown> {
    match merge_strategy {
        MergeStrategy::Galloping => merge_powersort_runs(
            working_set,
            src,
            range_to_merge.begin,
            range_to_merge.end + 1,
            run1.begin,
            run1.end + 1,
            comparator,
            min_gallop,
        )?,
        MergeStrategy::Simple => merge_runs_simple(
            working_set,
            src,
            range_to_merge.begin,
            range_to_merge.end + 1,
            run1.begin,
            run1.end + 1,
            comparator,
        )?,
    }
    let span = range_to_merge.begin..run1.end + 1;
    src[span.clone()].copy_from_slice(&working_set[span]);
    Ok(())
}

/// `arrayStableSort<MergeStrategy::Simple>(vm, src, workingSet, comparator)`: o do `%TypedArray%`.
pub fn array_stable_sort_simple<T: Copy>(
    src: &mut [T],
    working_set: &mut [T],
    comparator: impl FnMut(T, T) -> Result<bool, Thrown>,
) -> Result<(), Thrown> {
    array_stable_sort(MergeStrategy::Simple, src, working_set, comparator)
}

/// `arrayStableSort<mergeStrategy>(vm, src, workingSet, comparator)`: ordena `src` no lugar com
/// `working_set` (do mesmo comprimento) de rascunho. O `comparator(a, b)` responde "a vem antes de b".
pub fn array_stable_sort<T: Copy>(
    merge_strategy: MergeStrategy,
    src: &mut [T],
    working_set: &mut [T],
    mut comparator: impl FnMut(T, T) -> Result<bool, Thrown>,
) -> Result<(), Thrown> {
    const EXTEND_RUN_CUTOFF: usize = 8;

    let comparator = &mut comparator;
    let num_elements = src.len();

    if num_elements == 0 {
        return Ok(());
    }

    // If the array is small, Powersort probably isn't worth it. Just insertion sort.
    if num_elements < EXTEND_RUN_CUTOFF {
        return array_insertion_sort(src, comparator, 0);
    }

    // floor(lg(n)) + 1
    let mut powerstack: Vec<(SortedRun, u32)> = Vec::with_capacity((usize::BITS - num_elements.leading_zeros()) as usize);

    let mut min_gallop = MIN_GALLOP_THRESHOLD;

    // Detect ascending or descending run, normalize to ascending.
    let mut run1 = extend_run(src, 0, comparator)?;

    while run1.end + 1 < num_elements {
        let run2 = extend_run(src, run1.end + 1, comparator)?;

        let p = power(run1.begin, run2.begin, run2.end, num_elements);
        while powerstack.last().is_some_and(|&(_, last_power)| last_power > p) {
            let (range_to_merge, _) = powerstack.pop().expect("a pilha não está vazia");
            debug_assert!(range_to_merge.end + 1 == run1.begin);
            merge_into_source(merge_strategy, src, working_set, range_to_merge, run1, comparator, &mut min_gallop)?;
            run1.begin = range_to_merge.begin;
        }

        powerstack.push((run1, p));
        run1 = run2;
    }

    while let Some((range_to_merge, _)) = powerstack.pop() {
        debug_assert!(range_to_merge.end + 1 == run1.begin);
        merge_into_source(merge_strategy, src, working_set, range_to_merge, run1, comparator, &mut min_gallop)?;
        run1.begin = range_to_merge.begin;
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sort(values: &mut Vec<(u32, usize)>) {
        let mut working_set = values.clone();
        array_stable_sort_simple(values, &mut working_set, |left, right| Ok(left.0 < right.0)).unwrap();
    }

    fn sort_galloping(values: &mut Vec<(u32, usize)>) {
        let mut working_set = values.clone();
        array_stable_sort(MergeStrategy::Galloping, values, &mut working_set, |left, right| Ok(left.0 < right.0)).unwrap();
    }

    fn assert_stably_sorted(values: &[(u32, usize)]) {
        for pair in values.windows(2) {
            assert!(pair[0].0 < pair[1].0 || (pair[0].0 == pair[1].0 && pair[0].1 < pair[1].1), "{pair:?}");
        }
    }

    #[test]
    fn galloping_sorts_stably_across_sizes_and_shapes() {
        for length in [0usize, 1, 2, 7, 8, 9, 63, 64, 65, 200, 1000, 5000] {
            // Chaves repetidas (estabilidade), sequências já ordenadas, invertidas e em dentes de serra.
            let shapes: [fn(usize) -> u32; 5] = [
                |i| ((i * 7919) % 13) as u32,
                |i| (i / 3) as u32,
                |i| 10_000 - (i as u32),
                |i| (i % 50) as u32,
                |i| if i < 600 { i as u32 } else { (i * 31 % 700) as u32 },
            ];
            for shape in shapes {
                let mut values: Vec<(u32, usize)> = (0..length).map(|i| (shape(i), i)).collect();
                sort_galloping(&mut values);
                assert_stably_sorted(&values);
            }
        }
    }

    #[test]
    fn galloping_and_simple_agree() {
        let mut galloping: Vec<(u32, usize)> = (0..3000).map(|i| (((i * 2654435761usize) >> 7) as u32 % 40, i)).collect();
        let mut simple = galloping.clone();
        sort_galloping(&mut galloping);
        sort(&mut simple);
        assert_eq!(galloping, simple);
    }

    #[test]
    fn galloping_comparator_error_stops_the_sort() {
        let mut values: Vec<u32> = (0..500).map(|i| (i * 37 % 101) as u32).collect();
        let mut working_set = values.clone();
        let mut calls = 0;
        let result = array_stable_sort(MergeStrategy::Galloping, &mut values, &mut working_set, |left, right| {
            calls += 1;
            if calls == 100 { Err(Thrown::Pending) } else { Ok(left < right) }
        });
        assert_eq!(result, Err(Thrown::Pending));
        assert_eq!(calls, 100);
    }

    #[test]
    fn gallop_positions_follow_the_documented_bounds() {
        let base = [1u32, 2, 2, 2, 3, 5, 5, 8];
        let mut less = |a: u32, b: u32| Ok(a < b);
        for hint in 0..base.len() {
            for key in 0..10u32 {
                let left = gallop_left(key, &base, hint, &mut less).unwrap();
                let right = gallop_right(key, &base, hint, &mut less).unwrap();
                assert_eq!(left, base.iter().filter(|&&x| x < key).count(), "left key={key} hint={hint}");
                assert_eq!(right, base.iter().filter(|&&x| x <= key).count(), "right key={key} hint={hint}");
            }
        }
    }

    #[test]
    fn sorts_stably_across_sizes() {
        for length in [0usize, 1, 2, 7, 8, 9, 63, 64, 65, 200, 1000] {
            let mut values: Vec<(u32, usize)> = (0..length).map(|i| (((i * 7919) % 13) as u32, i)).collect();
            sort(&mut values);
            for pair in values.windows(2) {
                assert!(pair[0].0 < pair[1].0 || (pair[0].0 == pair[1].0 && pair[0].1 < pair[1].1), "{pair:?}");
            }
        }
    }

    #[test]
    fn descending_input_is_reversed_without_breaking_stability() {
        let mut values: Vec<(u32, usize)> = (0..100).map(|i| (100 - i as u32, i)).collect();
        sort(&mut values);
        assert!(values.windows(2).all(|pair| pair[0].0 < pair[1].0));
    }

    #[test]
    fn comparator_error_stops_the_sort() {
        let mut values: Vec<u32> = (0..50).rev().collect();
        let mut working_set = values.clone();
        let result = array_stable_sort_simple(&mut values, &mut working_set, |_, _| Err(Thrown::Pending));
        assert_eq!(result, Err(Thrown::Pending));
    }
}
