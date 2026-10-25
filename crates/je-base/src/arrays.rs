//! Porte de `src/vs/base/common/arrays.ts`.
//!
//! Representação:
//! - `T[]` é `&[T]`/`Vec<T>`; `T | undefined` é `Option<T>`; os comparadores devolvem `f64`
//!   ([`CompareResult`], um `number` do JavaScript), com [`to_ordering`] para ordenar com o `sort_by`
//!   do Rust (`NaN` conta como empate, como no `Array.prototype.sort`). A ordenação ([`sort_by_compare`])
//!   não usa o `sort_by` do Rust (que pode entrar em pânico com comparador inconsistente): reproduz o
//!   TimSort do V8 com a mesma sequência de chamadas ao comparador.
//! - Onde o TypeScript devolve ou recebe elementos por referência (`T`), as funções que guardam o
//!   elemento em outra coleção pedem `T: Clone`.
//! - Chave de `distinct`/`uniqueFilter`/`withoutDuplicates` é um `K: Hash + Eq` (o `Set` do JS usa
//!   `SameValueZero`; `f64` não é `Hash`, quem precisar usa `to_bits` ou o texto do número).
//! - `T | T[]` é [`OneOrMany`]; `coalesce` e `coalesceInPlace` descartam o que é falso no sentido do
//!   JavaScript (`None`, e também `0`, `""`, `false`, `NaN` quando `T` tem esses valores), via
//!   [`Truthy`]; `coalesceInPlace` não pode mudar o tipo do vetor, então continua `Vec<Option<T>>`
//!   só com `Some`.
//! - `ISplice` (de `sequence.ts`, ainda não portado) é [`Splice`], definido aqui até `sequence.rs`
//!   existir.
//! - `index` devolve um `HashMap` (o objeto-dicionário do JS preservaria a ordem de inserção).
//! - `shuffle` com semente usa `f64::sin` do Rust, que pode diferir do `Math.sin` do V8 na última
//!   casa; só afeta qual permutação sai, não a validade dela.
//!
//! Pendente (dependem de módulos que ainda não existem): `topAsync` (precisa de `async.ts` e de
//! `cancellation.rs`) e `findAsync` (precisa de `Promise.all`).

use std::cell::RefCell;
use std::cmp::Ordering;
use std::collections::{HashMap, HashSet};
use std::hash::Hash;
use std::rc::Rc;

use crate::arrays_find::find_first_idx_monotonous_or_arr_len;
use crate::dynamic::Truthy;
use crate::errors::{self, Error};
use crate::js_string::JsString;
use crate::random::random_f64;

/// Um `T | T[]`.
#[derive(Clone, Debug, PartialEq)]
pub enum OneOrMany<T> {
    One(T),
    Many(Vec<T>),
}

/// Descreve uma troca num array (`ISplice`).
#[derive(Clone, Debug, PartialEq)]
pub struct Splice<T> {
    pub start: usize,
    pub delete_count: usize,
    pub to_insert: Vec<T>,
}

/// Menos que zero: o primeiro é menor; mais que zero: é maior; zero: nenhum dos dois.
pub type CompareResult = f64;

/// `CompareResult` (o `namespace` do original).
pub mod compare_result {
    use super::CompareResult;

    pub fn is_less_than(result: CompareResult) -> bool {
        result < 0.0
    }

    pub fn is_less_than_or_equal(result: CompareResult) -> bool {
        result <= 0.0
    }

    pub fn is_greater_than(result: CompareResult) -> bool {
        result > 0.0
    }

    pub fn is_neither_less_or_greater_than(result: CompareResult) -> bool {
        result == 0.0
    }

    pub const GREATER_THAN: CompareResult = 1.0;
    pub const LESS_THAN: CompareResult = -1.0;
    pub const NEITHER_LESS_OR_GREATER_THAN: CompareResult = 0.0;
}

/// `Comparator<T>` do original, em caixa (para guardar vários, como em [`tie_break_comparators`]).
pub type Comparator<T> = Box<dyn Fn(&T, &T) -> CompareResult>;

/// Converte o resultado de um comparador do JS no `Ordering` do Rust (`NaN` vira `Equal`).
pub fn to_ordering(result: CompareResult) -> Ordering {
    if result < 0.0 {
        Ordering::Less
    } else if result > 0.0 {
        Ordering::Greater
    } else {
        Ordering::Equal
    }
}

/// Tamanho a partir do qual o TimSort do V8 passa a calcular um `minrun` menor que o vetor e a mesclar corridas.
const V8_MIN_MERGE: usize = 64;

/// `kMinGallopWins` do V8 e valor inicial de `min_gallop`.
const V8_MIN_GALLOP_WINS: isize = 7;

/// Um comparador sobre posições do vetor de trabalho (as posições originais dos elementos).
type IndexCompare<'a> = &'a dyn Fn(usize, usize) -> CompareResult;

fn u(index: isize) -> usize {
    index as usize
}

/// `array.sort(compare)`, com a sequência de chamadas ao comparador do V8: o TimSort inteiro de
/// `third_party/v8/builtins/array-sort.tq` (`CountAndMakeRun`, `BinaryInsertionSort`, `MergeCollapse`,
/// `MergeAt`, `GallopLeft`/`GallopRight`, `MergeLow`/`MergeHigh` com `min_gallop` adaptativo), cada chamada
/// com os argumentos na mesma ordem. Isso importa quando o comparador não é uma ordem total: o resultado
/// (e a ausência de pânico) segue o do JavaScript.
///
/// Como `T` não é `Clone`, o TimSort ordena um vetor de posições (`usize`, que se copia de graça) e o
/// comparador sempre vê os elementos originais por referência; no fim a permutação é aplicada com `swap`.
pub fn sort_by_compare<T>(array: &mut [T], compare: impl Fn(&T, &T) -> CompareResult) {
    let length = array.len();
    if length < 2 {
        return;
    }
    let order = {
        let view: &[T] = array;
        let by_index = |a: usize, b: usize| compare(&view[a], &view[b]);
        v8_tim_sort(length, &by_index)
    };
    apply_permutation(array, order);
}

/// Põe em `array[i]` o elemento que estava em `source[i]`, seguindo os ciclos da permutação.
fn apply_permutation<T>(array: &mut [T], mut source: Vec<usize>) {
    for start in 0..source.len() {
        let mut current = start;
        loop {
            let next = source[current];
            source[current] = current;
            if next == start {
                break;
            }
            array.swap(current, next);
            current = next;
        }
    }
}

/// `ComputeMinRunLength`.
fn compute_min_run_length(length: usize) -> usize {
    let mut n = length;
    let mut r = 0;
    while n >= V8_MIN_MERGE {
        r |= n & 1;
        n >>= 1;
    }
    n + r
}

/// `ArrayTimSortImpl`: devolve as posições originais na ordem final.
fn v8_tim_sort(length: usize, cmp: IndexCompare) -> Vec<usize> {
    let mut work: Vec<usize> = (0..length).collect();
    let mut min_gallop = V8_MIN_GALLOP_WINS;
    let mut runs: Vec<(isize, isize)> = Vec::new();
    let min_run_length = compute_min_run_length(length);
    let mut low = 0;
    let mut remaining = length;
    while remaining != 0 {
        let mut current_run_length = count_and_make_run(&mut work, cmp, low, low + remaining);
        if current_run_length < min_run_length {
            let forced_run_length = min_run_length.min(remaining);
            binary_insertion_sort(&mut work, cmp, low, low + current_run_length, low + forced_run_length);
            current_run_length = forced_run_length;
        }
        runs.push((low as isize, current_run_length as isize));
        merge_collapse(&mut work, &mut runs, &mut min_gallop, cmp);
        low += current_run_length;
        remaining -= current_run_length;
    }
    merge_force_collapse(&mut work, &mut runs, &mut min_gallop, cmp);
    work
}

/// `CountAndMakeRun(lowArg, high)`.
fn count_and_make_run(work: &mut [usize], cmp: IndexCompare, low_arg: usize, high: usize) -> usize {
    let low = low_arg + 1;
    if low == high {
        return 1;
    }
    let mut run_length = 2;
    let order = cmp(work[low], work[low - 1]);
    let is_descending = order < 0.0;
    let mut previous = work[low];
    for index in (low + 1)..high {
        let current = work[index];
        let order = cmp(current, previous);
        if if is_descending { order >= 0.0 } else { order < 0.0 } {
            break;
        }
        previous = current;
        run_length += 1;
    }
    if is_descending {
        work[(low - 1)..(low - 1 + run_length)].reverse();
    }
    run_length
}

/// `BinaryInsertionSort(low, startArg, high)`.
fn binary_insertion_sort(work: &mut [usize], cmp: IndexCompare, low: usize, start_arg: usize, high: usize) {
    let start_from = if low == start_arg { start_arg + 1 } else { start_arg };
    for start in start_from..high {
        let (mut left, mut right) = (low, start);
        let pivot = work[start];
        while left < right {
            let mid = left + ((right - left) >> 1);
            if cmp(pivot, work[mid]) < 0.0 {
                right = mid;
            } else {
                left = mid + 1;
            }
        }
        work[left..=start].rotate_right(1);
    }
}

/// `RunInvariantEstablished(n)`.
fn run_invariant_established(runs: &[(isize, isize)], n: usize) -> bool {
    n < 2 || runs[n - 2].1 > runs[n - 1].1 + runs[n].1
}

/// `MergeCollapse`, com a correção de 2015 (olha `n - 2` e `n - 3`).
fn merge_collapse(work: &mut [usize], runs: &mut Vec<(isize, isize)>, min_gallop: &mut isize, cmp: IndexCompare) {
    while runs.len() > 1 {
        let mut n = runs.len() - 2;
        if !run_invariant_established(runs, n + 1) || !run_invariant_established(runs, n) {
            if n > 0 && runs[n - 1].1 < runs[n + 1].1 {
                n -= 1;
            }
            merge_at(work, runs, min_gallop, cmp, n);
        } else if runs[n].1 <= runs[n + 1].1 {
            merge_at(work, runs, min_gallop, cmp, n);
        } else {
            break;
        }
    }
}

/// `MergeForceCollapse`.
fn merge_force_collapse(work: &mut [usize], runs: &mut Vec<(isize, isize)>, min_gallop: &mut isize, cmp: IndexCompare) {
    while runs.len() > 1 {
        let mut n = runs.len() - 2;
        if n > 0 && runs[n - 1].1 < runs[n + 1].1 {
            n -= 1;
        }
        merge_at(work, runs, min_gallop, cmp, n);
    }
}

/// `MergeAt(i)`: mescla as corridas `i` e `i + 1`.
fn merge_at(work: &mut [usize], runs: &mut Vec<(isize, isize)>, min_gallop: &mut isize, cmp: IndexCompare, i: usize) {
    let (mut base_a, mut length_a) = runs[i];
    let (base_b, mut length_b) = runs[i + 1];
    runs[i].1 = length_a + length_b;
    if i + 3 == runs.len() {
        runs[i + 1] = runs[i + 2];
    }
    runs.pop();

    // Onde o primeiro elemento de B entra em A: os anteriores já estão no lugar.
    let key_right = work[u(base_b)];
    let k = gallop_right(work, cmp, key_right, base_a, length_a, 0);
    base_a += k;
    length_a -= k;
    if length_a == 0 {
        return;
    }
    // Onde o último elemento de A entra em B: os posteriores já estão no lugar.
    let key_left = work[u(base_a + length_a - 1)];
    length_b = gallop_left(work, cmp, key_left, base_b, length_b, length_b - 1);
    if length_b == 0 {
        return;
    }
    if length_a <= length_b {
        merge_low(work, min_gallop, cmp, base_a, length_a, base_b, length_b);
    } else {
        merge_high(work, min_gallop, cmp, base_a, length_a, base_b, length_b);
    }
}

/// `GallopLeft(array, key, base, length, hint)`.
fn gallop_left(array: &[usize], cmp: IndexCompare, key: usize, base: isize, length: isize, hint: isize) -> isize {
    let mut last_ofs: isize = 0;
    let mut offset: isize = 1;
    let order = cmp(array[u(base + hint)], key);
    if order < 0.0 {
        // a[hint] < key: galopa à direita até a[hint + last_ofs] < key <= a[hint + offset].
        let max_ofs = length - hint;
        while offset < max_ofs {
            let order = cmp(array[u(base + hint + offset)], key);
            if order >= 0.0 {
                break;
            }
            last_ofs = offset;
            offset = (offset << 1) + 1;
        }
        if offset > max_ofs {
            offset = max_ofs;
        }
        last_ofs += hint;
        offset += hint;
    } else {
        // key <= a[hint]: galopa à esquerda até a[hint - offset] < key <= a[hint - last_ofs].
        let max_ofs = hint + 1;
        while offset < max_ofs {
            let order = cmp(array[u(base + hint - offset)], key);
            if order < 0.0 {
                break;
            }
            last_ofs = offset;
            offset = (offset << 1) + 1;
        }
        if offset > max_ofs {
            offset = max_ofs;
        }
        let tmp = last_ofs;
        last_ofs = hint - offset;
        offset = hint - tmp;
    }
    // Agora a[base + last_ofs] < key <= a[base + offset]: busca binária.
    last_ofs += 1;
    while last_ofs < offset {
        let m = last_ofs + ((offset - last_ofs) >> 1);
        let order = cmp(array[u(base + m)], key);
        if order < 0.0 {
            last_ofs = m + 1;
        } else {
            offset = m;
        }
    }
    offset
}

/// `GallopRight(array, key, base, length, hint)`.
fn gallop_right(array: &[usize], cmp: IndexCompare, key: usize, base: isize, length: isize, hint: isize) -> isize {
    let mut last_ofs: isize = 0;
    let mut offset: isize = 1;
    let order = cmp(key, array[u(base + hint)]);
    if order < 0.0 {
        // key < a[hint]: galopa à esquerda até a[hint - offset] <= key < a[hint - last_ofs].
        let max_ofs = hint + 1;
        while offset < max_ofs {
            let order = cmp(key, array[u(base + hint - offset)]);
            if order >= 0.0 {
                break;
            }
            last_ofs = offset;
            offset = (offset << 1) + 1;
        }
        if offset > max_ofs {
            offset = max_ofs;
        }
        let tmp = last_ofs;
        last_ofs = hint - offset;
        offset = hint - tmp;
    } else {
        // a[hint] <= key: galopa à direita até a[hint + last_ofs] <= key < a[hint + offset].
        let max_ofs = length - hint;
        while offset < max_ofs {
            let order = cmp(key, array[u(base + hint + offset)]);
            if order < 0.0 {
                break;
            }
            last_ofs = offset;
            offset = (offset << 1) + 1;
        }
        if offset > max_ofs {
            offset = max_ofs;
        }
        last_ofs += hint;
        offset += hint;
    }
    // Agora a[base + last_ofs] <= key < a[base + offset]: busca binária.
    last_ofs += 1;
    while last_ofs < offset {
        let m = last_ofs + ((offset - last_ofs) >> 1);
        let order = cmp(key, array[u(base + m)]);
        if order < 0.0 {
            offset = m;
        } else {
            last_ofs = m + 1;
        }
    }
    offset
}

/// Como terminou o laço de `MergeLow`/`MergeHigh`: `Succeed` ou o rótulo `CopyB`/`CopyA`.
#[derive(Clone, Copy, PartialEq)]
enum MergeEnd {
    Succeed,
    CopyLast,
}

/// `MergeLow`: mescla A (copiada para um temporário) e B da esquerda para a direita; `length_a <= length_b`.
fn merge_low(
    work: &mut [usize],
    min_gallop_state: &mut isize,
    cmp: IndexCompare,
    base_a: isize,
    length_a_arg: isize,
    base_b: isize,
    length_b_arg: isize,
) {
    let mut length_a = length_a_arg;
    let mut length_b = length_b_arg;
    let temp: Vec<usize> = work[u(base_a)..u(base_a + length_a)].to_vec();
    let mut dest = base_a;
    let mut cursor_temp: isize = 0;
    let mut cursor_b = base_b;

    work[u(dest)] = work[u(cursor_b)];
    dest += 1;
    cursor_b += 1;

    let end = 'merge: {
        length_b -= 1;
        if length_b == 0 {
            break 'merge MergeEnd::Succeed;
        }
        if length_a == 1 {
            break 'merge MergeEnd::CopyLast;
        }
        let mut min_gallop = *min_gallop_state;
        loop {
            let mut wins_a: isize = 0;
            let mut wins_b: isize = 0;
            // Mescla um a um até uma das corridas parecer ganhar sempre.
            loop {
                let order = cmp(work[u(cursor_b)], temp[u(cursor_temp)]);
                if order < 0.0 {
                    work[u(dest)] = work[u(cursor_b)];
                    dest += 1;
                    cursor_b += 1;
                    wins_b += 1;
                    length_b -= 1;
                    wins_a = 0;
                    if length_b == 0 {
                        break 'merge MergeEnd::Succeed;
                    }
                    if wins_b >= min_gallop {
                        break;
                    }
                } else {
                    work[u(dest)] = temp[u(cursor_temp)];
                    dest += 1;
                    cursor_temp += 1;
                    wins_a += 1;
                    length_a -= 1;
                    wins_b = 0;
                    if length_a == 1 {
                        break 'merge MergeEnd::CopyLast;
                    }
                    if wins_a >= min_gallop {
                        break;
                    }
                }
            }

            // Modo galope.
            min_gallop += 1;
            let mut first_iteration = true;
            while wins_a >= V8_MIN_GALLOP_WINS || wins_b >= V8_MIN_GALLOP_WINS || first_iteration {
                first_iteration = false;
                min_gallop = (min_gallop - 1).max(1);
                *min_gallop_state = min_gallop;

                wins_a = gallop_right(&temp, cmp, work[u(cursor_b)], cursor_temp, length_a, 0);
                if wins_a > 0 {
                    work[u(dest)..u(dest + wins_a)].copy_from_slice(&temp[u(cursor_temp)..u(cursor_temp + wins_a)]);
                    dest += wins_a;
                    cursor_temp += wins_a;
                    length_a -= wins_a;
                    if length_a == 1 {
                        break 'merge MergeEnd::CopyLast;
                    }
                    // Com comparador consistente length_a não zera aqui, mas não se pode supor isso.
                    if length_a == 0 {
                        break 'merge MergeEnd::Succeed;
                    }
                }
                work[u(dest)] = work[u(cursor_b)];
                dest += 1;
                cursor_b += 1;
                length_b -= 1;
                if length_b == 0 {
                    break 'merge MergeEnd::Succeed;
                }

                wins_b = gallop_left(work, cmp, temp[u(cursor_temp)], cursor_b, length_b, 0);
                if wins_b > 0 {
                    work.copy_within(u(cursor_b)..u(cursor_b + wins_b), u(dest));
                    dest += wins_b;
                    cursor_b += wins_b;
                    length_b -= wins_b;
                    if length_b == 0 {
                        break 'merge MergeEnd::Succeed;
                    }
                }
                work[u(dest)] = temp[u(cursor_temp)];
                dest += 1;
                cursor_temp += 1;
                length_a -= 1;
                if length_a == 1 {
                    break 'merge MergeEnd::CopyLast;
                }
            }
            min_gallop += 1; // penaliza por sair do modo galope
            *min_gallop_state = min_gallop;
        }
    };

    match end {
        MergeEnd::Succeed => {
            if length_a > 0 {
                work[u(dest)..u(dest + length_a)].copy_from_slice(&temp[u(cursor_temp)..u(cursor_temp + length_a)]);
            }
        }
        MergeEnd::CopyLast => {
            // O último elemento de A pertence ao fim da mescla.
            work.copy_within(u(cursor_b)..u(cursor_b + length_b), u(dest));
            work[u(dest + length_b)] = temp[u(cursor_temp)];
        }
    }
}

/// `MergeHigh`: mescla A e B (copiada para um temporário) da direita para a esquerda; `length_a > length_b`.
fn merge_high(
    work: &mut [usize],
    min_gallop_state: &mut isize,
    cmp: IndexCompare,
    base_a: isize,
    length_a_arg: isize,
    base_b: isize,
    length_b_arg: isize,
) {
    let mut length_a = length_a_arg;
    let mut length_b = length_b_arg;
    let temp: Vec<usize> = work[u(base_b)..u(base_b + length_b)].to_vec();
    let mut dest = base_b + length_b - 1;
    let mut cursor_temp = length_b - 1;
    let mut cursor_a = base_a + length_a - 1;

    work[u(dest)] = work[u(cursor_a)];
    dest -= 1;
    cursor_a -= 1;

    let end = 'merge: {
        length_a -= 1;
        if length_a == 0 {
            break 'merge MergeEnd::Succeed;
        }
        if length_b == 1 {
            break 'merge MergeEnd::CopyLast;
        }
        let mut min_gallop = *min_gallop_state;
        loop {
            let mut wins_a: isize = 0;
            let mut wins_b: isize = 0;
            loop {
                let order = cmp(temp[u(cursor_temp)], work[u(cursor_a)]);
                if order < 0.0 {
                    work[u(dest)] = work[u(cursor_a)];
                    dest -= 1;
                    cursor_a -= 1;
                    wins_a += 1;
                    length_a -= 1;
                    wins_b = 0;
                    if length_a == 0 {
                        break 'merge MergeEnd::Succeed;
                    }
                    if wins_a >= min_gallop {
                        break;
                    }
                } else {
                    work[u(dest)] = temp[u(cursor_temp)];
                    dest -= 1;
                    cursor_temp -= 1;
                    wins_b += 1;
                    length_b -= 1;
                    wins_a = 0;
                    if length_b == 1 {
                        break 'merge MergeEnd::CopyLast;
                    }
                    if wins_b >= min_gallop {
                        break;
                    }
                }
            }

            // Modo galope.
            min_gallop += 1;
            let mut first_iteration = true;
            while wins_a >= V8_MIN_GALLOP_WINS || wins_b >= V8_MIN_GALLOP_WINS || first_iteration {
                first_iteration = false;
                min_gallop = (min_gallop - 1).max(1);
                *min_gallop_state = min_gallop;

                let k = gallop_right(work, cmp, temp[u(cursor_temp)], base_a, length_a, length_a - 1);
                wins_a = length_a - k;
                if wins_a > 0 {
                    dest -= wins_a;
                    cursor_a -= wins_a;
                    work.copy_within(u(cursor_a + 1)..u(cursor_a + 1 + wins_a), u(dest + 1));
                    length_a -= wins_a;
                    if length_a == 0 {
                        break 'merge MergeEnd::Succeed;
                    }
                }
                work[u(dest)] = temp[u(cursor_temp)];
                dest -= 1;
                cursor_temp -= 1;
                length_b -= 1;
                if length_b == 1 {
                    break 'merge MergeEnd::CopyLast;
                }

                let k = gallop_left(&temp, cmp, work[u(cursor_a)], 0, length_b, length_b - 1);
                wins_b = length_b - k;
                if wins_b > 0 {
                    dest -= wins_b;
                    cursor_temp -= wins_b;
                    work[u(dest + 1)..u(dest + 1 + wins_b)]
                        .copy_from_slice(&temp[u(cursor_temp + 1)..u(cursor_temp + 1 + wins_b)]);
                    length_b -= wins_b;
                    if length_b == 1 {
                        break 'merge MergeEnd::CopyLast;
                    }
                    // Com comparador consistente length_b não zera aqui, mas não se pode supor isso.
                    if length_b == 0 {
                        break 'merge MergeEnd::Succeed;
                    }
                }
                work[u(dest)] = work[u(cursor_a)];
                dest -= 1;
                cursor_a -= 1;
                length_a -= 1;
                if length_a == 0 {
                    break 'merge MergeEnd::Succeed;
                }
            }
            min_gallop += 1; // penaliza por sair do modo galope
            *min_gallop_state = min_gallop;
        }
    };

    match end {
        MergeEnd::Succeed => {
            if length_b > 0 {
                let from = dest - (length_b - 1);
                work[u(from)..u(from + length_b)].copy_from_slice(&temp[0..u(length_b)]);
            }
        }
        MergeEnd::CopyLast => {
            // O primeiro elemento de B pertence ao começo da mescla.
            dest -= length_a;
            cursor_a -= length_a;
            work.copy_within(u(cursor_a + 1)..u(cursor_a + 1 + length_a), u(dest + 1));
            work[u(dest)] = temp[u(cursor_temp)];
        }
    }
}

/// Os índices de `slice(start, end)` do JavaScript (negativos contam do fim), já limitados.
fn slice_bounds(len: usize, start: i64, end: Option<i64>) -> (usize, usize) {
    let len_i = len as i64;
    let resolve = |i: i64| if i < 0 { (len_i + i).max(0) } else { i.min(len_i) } as usize;
    let from = resolve(start);
    let to = end.map_or(len, resolve);
    (from, to.max(from))
}

/// Devolve o último elemento e os N-1 iniciais, `[rest, last]`. Lança se o array estiver vazio.
pub fn tail<T: Clone>(arr: &[T]) -> (Vec<T>, T) {
    match arr.split_last() {
        Some((last, rest)) => (rest.to_vec(), last.clone()),
        None => errors::throw(Error::new("Invalid tail call")),
    }
}

/// `equals(one, other, itemEquals)`: `None` é `undefined`; o mesmo slice (mesmo endereço e
/// tamanho) é igual sem olhar os itens, como `one === other`.
pub fn equals<T>(one: Option<&[T]>, other: Option<&[T]>, item_equals: impl Fn(&T, &T) -> bool) -> bool {
    match (one, other) {
        (Some(a), Some(b)) => {
            if std::ptr::eq(a, b) {
                return true;
            }
            a.len() == b.len() && a.iter().zip(b).all(|(x, y)| item_equals(x, y))
        }
        (None, None) => true,
        _ => false,
    }
}

/// Remove o elemento em `index` trocando-o pelo último: mais rápido que `splice`, mas muda a ordem.
pub fn remove_fast_without_keeping_order<T>(array: &mut Vec<T>, index: usize) {
    let last = array.len().wrapping_sub(1);
    if index < last {
        array.swap_remove(index);
    } else {
        array.pop();
    }
}

/// Busca binária num array ordenado. Devolve o índice (>= 0) se achou, senão `-(n + 1)` com `n`
/// a posição de inserção (o `~n` do original).
pub fn binary_search<T>(array: &[T], key: &T, comparator: impl Fn(&T, &T) -> CompareResult) -> i64 {
    binary_search2(array.len(), |i| comparator(&array[i], key))
}

/// Busca binária sobre uma coleção qualquer de `length` elementos; `compare_to_key(i)` diz se o
/// valor em `i` precede (< 0), iguala (0) ou sucede (> 0) a chave. Mesmo resultado de [`binary_search`].
pub fn binary_search2(length: usize, mut compare_to_key: impl FnMut(usize) -> CompareResult) -> i64 {
    let mut low: i64 = 0;
    let mut high: i64 = length as i64 - 1;
    while low <= high {
        let mid = (low + high) / 2;
        let comp = compare_to_key(mid as usize);
        if comp < 0.0 {
            low = mid + 1;
        } else if comp > 0.0 {
            high = mid - 1;
        } else {
            return mid;
        }
    }
    -(low + 1)
}

/// O `nth` menor elemento (começando em 0) pelo quickselect; `data` não precisa estar ordenado.
/// Lança `TypeError` se `nth >= data.len()`.
pub fn quick_select<T: Clone>(nth: usize, data: &[T], compare: impl Fn(&T, &T) -> CompareResult) -> T {
    quick_select_step(nth, data, &compare)
}

fn quick_select_step<T: Clone>(nth: usize, data: &[T], compare: &impl Fn(&T, &T) -> CompareResult) -> T {
    if nth >= data.len() {
        errors::throw(Error::type_error("invalid index"));
    }
    let pivot_value = &data[(data.len() as f64 * random_f64()).floor() as usize];
    let mut lower = Vec::new();
    let mut higher = Vec::new();
    let mut pivots = Vec::new();
    for value in data {
        let val = compare(value, pivot_value);
        if val < 0.0 {
            lower.push(value.clone());
        } else if val > 0.0 {
            higher.push(value.clone());
        } else {
            pivots.push(value.clone());
        }
    }
    if nth < lower.len() {
        quick_select_step(nth, &lower, compare)
    } else if nth < lower.len() + pivots.len() {
        pivots.swap_remove(0)
    } else {
        quick_select_step(nth - (lower.len() + pivots.len()), &higher, compare)
    }
}

/// Ordena uma cópia e agrupa os elementos que o comparador diz iguais.
pub fn group_by<T: Clone>(data: &[T], compare: impl Fn(&T, &T) -> CompareResult) -> Vec<Vec<T>> {
    let mut sorted = data.to_vec();
    sort_by_compare(&mut sorted, &compare);
    let mut result: Vec<Vec<T>> = Vec::new();
    for element in sorted {
        match result.last_mut() {
            Some(group) if compare(&group[0], &element) == 0.0 => group.push(element),
            _ => result.push(vec![element]),
        }
    }
    result
}

/// O gerador de [`group_adjacent_by`].
pub struct GroupAdjacentBy<I: Iterator, F> {
    iter: I,
    should_be_grouped: F,
    current: Option<Vec<I::Item>>,
    last: Option<I::Item>,
}

impl<I, F> Iterator for GroupAdjacentBy<I, F>
where
    I: Iterator,
    I::Item: Clone,
    F: FnMut(&I::Item, &I::Item) -> bool,
{
    type Item = Vec<I::Item>;

    fn next(&mut self) -> Option<Self::Item> {
        loop {
            match self.iter.next() {
                Some(item) => {
                    let same = self.last.as_ref().is_some_and(|last| (self.should_be_grouped)(last, &item));
                    self.last = Some(item.clone());
                    if same {
                        self.current.as_mut().expect("grupo aberto").push(item);
                    } else if let Some(done) = self.current.replace(vec![item]) {
                        return Some(done);
                    }
                }
                None => return self.current.take(),
            }
        }
    }
}

/// Divide os itens em grupos (não vazios); `should_be_grouped` decide se dois itens consecutivos
/// ficam no mesmo grupo. A ordem é preservada e a produção é preguiçosa.
pub fn group_adjacent_by<I, F>(items: I, should_be_grouped: F) -> GroupAdjacentBy<I::IntoIter, F>
where
    I: IntoIterator,
    I::Item: Clone,
    F: FnMut(&I::Item, &I::Item) -> bool,
{
    GroupAdjacentBy { iter: items.into_iter(), should_be_grouped, current: None, last: None }
}

/// Chama `f(anterior, atual)` para cada par adjacente, incluindo `(None, primeiro)` e `(último, None)`.
pub fn for_each_adjacent<T>(arr: &[T], mut f: impl FnMut(Option<&T>, Option<&T>)) {
    for i in 0..=arr.len() {
        f(if i == 0 { None } else { Some(&arr[i - 1]) }, arr.get(i));
    }
}

/// Chama `f(antes, elemento, depois)` para cada elemento.
pub fn for_each_with_neighbors<T>(arr: &[T], mut f: impl FnMut(Option<&T>, &T, Option<&T>)) {
    for i in 0..arr.len() {
        f(if i == 0 { None } else { Some(&arr[i - 1]) }, &arr[i], arr.get(i + 1));
    }
}

/// `[].concat(...arrays)` para arrays de elementos que não são eles mesmos arrays.
pub fn concat_arrays<T: Clone>(arrays: &[&[T]]) -> Vec<T> {
    arrays.iter().flat_map(|a| a.iter().cloned()).collect()
}

/// Diferença de dois arrays *ordenados*, como as trocas que aplicam o diff.
pub fn sorted_diff<T: Clone>(before: &[T], after: &[T], compare: impl Fn(&T, &T) -> CompareResult) -> Vec<Splice<T>> {
    let mut result: Vec<Splice<T>> = Vec::new();

    fn push_splice<T: Clone>(result: &mut Vec<Splice<T>>, start: usize, delete_count: usize, to_insert: &[T]) {
        if delete_count == 0 && to_insert.is_empty() {
            return;
        }
        match result.last_mut() {
            Some(latest) if latest.start + latest.delete_count == start => {
                latest.delete_count += delete_count;
                latest.to_insert.extend_from_slice(to_insert);
            }
            _ => result.push(Splice { start, delete_count, to_insert: to_insert.to_vec() }),
        }
    }

    let mut before_idx = 0;
    let mut after_idx = 0;
    loop {
        if before_idx == before.len() {
            push_splice(&mut result, before_idx, 0, &after[after_idx..]);
            break;
        }
        if after_idx == after.len() {
            push_splice(&mut result, before_idx, before.len() - before_idx, &[]);
            break;
        }
        let n = compare(&before[before_idx], &after[after_idx]);
        if n == 0.0 {
            before_idx += 1;
            after_idx += 1;
        } else if n < 0.0 {
            // O elemento de `before` é menor: foi removido.
            push_splice(&mut result, before_idx, 1, &[]);
            before_idx += 1;
        } else if n > 0.0 {
            // O elemento de `before` é maior: o de `after` foi adicionado.
            push_splice(&mut result, before_idx, 0, std::slice::from_ref(&after[after_idx]));
            after_idx += 1;
        }
    }
    result
}

/// O que saiu e o que entrou entre dois arrays *ordenados*.
#[derive(Clone, Debug, PartialEq)]
pub struct Delta<T> {
    pub removed: Vec<T>,
    pub added: Vec<T>,
}

/// Calcula o delta de dois arrays *ordenados* em no máximo `min(before.len(), after.len())` passos.
pub fn delta<T: Clone>(before: &[T], after: &[T], compare: impl Fn(&T, &T) -> CompareResult) -> Delta<T> {
    let mut removed = Vec::new();
    let mut added = Vec::new();
    for splice in sorted_diff(before, after, compare) {
        removed.extend_from_slice(&before[splice.start..splice.start + splice.delete_count]);
        added.extend(splice.to_insert);
    }
    Delta { removed, added }
}

/// Os `n` primeiros elementos do array quando ordenado com `compare`; mais rápido que ordenar tudo
/// quando o array é bem maior que `n`.
pub fn top<T: Clone>(array: &[T], compare: impl Fn(&T, &T) -> CompareResult, n: usize) -> Vec<T> {
    if n == 0 {
        return Vec::new();
    }
    let mut result = array[..n.min(array.len())].to_vec();
    sort_by_compare(&mut result, &compare);
    top_step(array, &compare, &mut result, n, array.len());
    result
}

fn top_step<T: Clone>(array: &[T], compare: &impl Fn(&T, &T) -> CompareResult, result: &mut Vec<T>, from: usize, to: usize) {
    let n = result.len();
    for element in array.iter().take(to).skip(from) {
        if compare(element, &result[n - 1]) < 0.0 {
            result.pop();
            let j = find_first_idx_monotonous_or_arr_len(result, |e| compare(element, e) < 0.0, 0, result.len());
            result.insert(j, element.clone());
        }
    }
}

/// `topAsync`: como [`top`], em lotes de `batch` elementos, cedendo ao laço de eventos entre um lote e outro (um
/// `setTimeout` de 0 ms, que não deixa a E/S passar fome) e rejeitando com [`Error::cancellation`](crate::errors::Error::cancellation)
/// se o `token` cancelar entre dois lotes. O array é recebido por valor porque a promessa o usa depois que a chamada volta.
pub fn top_async<T: Clone + 'static>(
    array: Vec<T>,
    compare: impl Fn(&T, &T) -> CompareResult + 'static,
    n: usize,
    batch: usize,
    token: Option<crate::cancellation::CancellationToken>,
) -> crate::promise::Promise<Vec<T>> {
    if n == 0 {
        return crate::promise::Promise::resolved(Vec::new());
    }

    crate::async_::async_fn(async move {
        let o = array.len();
        let mut result = array[..n.min(o)].to_vec();
        sort_by_compare(&mut result, &compare);
        let mut i = n;
        let mut m = (n + batch).min(o);
        while i < o {
            if i > n {
                // any other delay function would starve I/O
                crate::async_::timeout(0).await?;
            }
            if token.as_ref().is_some_and(|token| token.is_cancellation_requested()) {
                return Err(crate::errors::Error::cancellation());
            }
            top_step(&array, &compare, &mut result, i, m);
            i = m;
            m = (m + batch).min(o);
        }
        Ok(result)
    })
}

/// Novo array sem os valores falsos (`None`, e `0`, `""`, `false`, `NaN` quando `T` os tem).
pub fn coalesce<T: Truthy + Clone>(array: &[Option<T>]) -> Vec<T> {
    array.iter().filter(|e| e.is_truthy()).filter_map(Clone::clone).collect()
}

/// Remove os valores falsos de `array`, no lugar.
pub fn coalesce_in_place<T: Truthy>(array: &mut Vec<Option<T>>) {
    array.retain(Truthy::is_truthy);
}

/// `array.splice(to, 0, array.splice(from, 1)[0])` (obsoleto no original).
pub fn r#move<T>(array: &mut Vec<T>, from: usize, to: usize) {
    if from < array.len() {
        let item = array.remove(from);
        array.insert(to.min(array.len()), item);
    }
}

/// `!Array.isArray(obj) || obj.length === 0`, com `None` no lugar do que não é array.
pub fn is_falsy_or_empty<T>(obj: Option<&[T]>) -> bool {
    obj.is_none_or(<[T]>::is_empty)
}

/// Verdadeiro se for um array com pelo menos um elemento.
pub fn is_non_empty_array<T>(obj: Option<&[T]>) -> bool {
    obj.is_some_and(|a| !a.is_empty())
}

/// Tira os duplicados mantendo o primeiro de cada chave.
pub fn distinct<T: Clone, K: Hash + Eq>(array: &[T], key_fn: impl Fn(&T) -> K) -> Vec<T> {
    let mut filter = unique_filter(key_fn);
    array.iter().filter(|e| filter(e)).cloned().collect()
}

/// Um filtro que deixa passar só o primeiro elemento de cada chave.
pub fn unique_filter<T, K: Hash + Eq>(key_fn: impl Fn(&T) -> K) -> impl FnMut(&T) -> bool {
    let mut seen = HashSet::new();
    move |element| seen.insert(key_fn(element))
}

/// Tamanho do prefixo comum, com a igualdade dada.
pub fn common_prefix_length<T>(one: &[T], other: &[T], equals: impl Fn(&T, &T) -> bool) -> usize {
    one.iter().zip(other).take_while(|(a, b)| equals(a, b)).count()
}

/// `range(to)`: `0..to`, ou `0, -1, ...` descendo até (sem incluir) `to` se `to` é negativo.
pub fn range(to: i64) -> Vec<i64> {
    range_from(0, to)
}

/// `range(from, to)`: de `from` até `to` (exclusivo), subindo ou descendo.
pub fn range_from(from: i64, to: i64) -> Vec<i64> {
    if from <= to { (from..to).collect() } else { (to + 1..=from).rev().collect() }
}

/// `index(array, indexer, mapper)`: dicionário da chave de cada elemento para o valor mapeado;
/// chave repetida fica com o último.
pub fn index<T, R>(array: &[T], indexer: impl Fn(&T) -> JsString, mapper: impl Fn(&T) -> R) -> HashMap<JsString, R> {
    array.iter().map(|t| (indexer(t), mapper(t))).collect()
}

/// Insere `element` no fim. Devolve a função que o remove de novo (a primeira ocorrência igual,
/// se ainda houver). Obsoleto no original: na maioria dos casos use um `HashSet`.
pub fn insert<T: PartialEq + Clone + 'static>(array: &Rc<RefCell<Vec<T>>>, element: T) -> impl Fn() {
    array.borrow_mut().push(element.clone());
    let array = array.clone();
    move || {
        remove(&mut array.borrow_mut(), &element);
    }
}

/// Remove a primeira ocorrência de `element`, se existir, e a devolve.
pub fn remove<T: PartialEq>(array: &mut Vec<T>, element: &T) -> Option<T> {
    array.iter().position(|e| e == element).map(|i| array.remove(i))
}

/// Insere `insert_arr` em `target` na posição `insert_index` (índice como o de `slice`), num novo array.
pub fn array_insert<T: Clone>(target: &[T], insert_index: i64, insert_arr: &[T]) -> Vec<T> {
    let (split, _) = slice_bounds(target.len(), insert_index, None);
    let mut result = Vec::with_capacity(target.len() + insert_arr.len());
    result.extend_from_slice(&target[..split]);
    result.extend_from_slice(insert_arr);
    result.extend_from_slice(&target[split..]);
    result
}

/// Embaralha com Fisher-Yates. Com `seed` o resultado é determinístico (o gerador do original,
/// baseado em `sin`).
pub fn shuffle<T>(array: &mut [T], seed: Option<f64>) {
    let mut seed = seed;
    let mut rand = move || match seed.as_mut() {
        Some(s) => {
            let x = s.sin() * 179_426_549.0;
            *s += 1.0;
            x - x.floor()
        }
        None => random_f64(),
    };
    for i in (1..array.len()).rev() {
        let j = (rand() * (i + 1) as f64).floor() as usize;
        array.swap(i, j);
    }
}

/// Põe `value` no início do array, se estiver nele.
pub fn push_to_start<T: PartialEq>(arr: &mut Vec<T>, value: &T) {
    if let Some(index) = arr.iter().position(|e| e == value) {
        let item = arr.remove(index);
        arr.insert(0, item);
    }
}

/// Põe `value` no fim do array, se estiver nele.
pub fn push_to_end<T: PartialEq>(arr: &mut Vec<T>, value: &T) {
    if let Some(index) = arr.iter().position(|e| e == value) {
        let item = arr.remove(index);
        arr.push(item);
    }
}

pub fn push_many<T: Clone>(arr: &mut Vec<T>, items: &[T]) {
    arr.extend_from_slice(items);
}

/// Aplica `f` ao item ou a cada item.
pub fn map_array_or_not<T, U>(items: OneOrMany<T>, f: impl Fn(T) -> U) -> OneOrMany<U> {
    match items {
        OneOrMany::One(item) => OneOrMany::One(f(item)),
        OneOrMany::Many(items) => OneOrMany::Many(items.into_iter().map(f).collect()),
    }
}

/// `map` que descarta o que `f` devolve como `None` (o `undefined` do original).
pub fn map_filter<T, U>(array: &[T], f: impl Fn(&T) -> Option<U>) -> Vec<U> {
    array.iter().filter_map(f).collect()
}

/// Os elementos sem repetição, na ordem da primeira ocorrência (`Array.from(new Set(array))`).
pub fn without_duplicates<T: Clone + Hash + Eq>(array: &[T]) -> Vec<T> {
    distinct(array, T::clone)
}

/// `asArray(x)`: o item vira um array de um elemento.
pub fn as_array<T>(x: OneOrMany<T>) -> Vec<T> {
    match x {
        OneOrMany::One(item) => vec![item],
        OneOrMany::Many(items) => items,
    }
}

/// Um elemento aleatório, `None` se o array é vazio.
pub fn get_random_element<T>(arr: &[T]) -> Option<&T> {
    arr.get((random_f64() * arr.len() as f64).floor() as usize)
}

/// O índice inicial efetivo, com a regra do `splice`/`slice` nativos: acima do tamanho vale o
/// tamanho; negativo conta do fim, com piso em 0.
fn actual_start_index(len: usize, start: i64) -> usize {
    if start < 0 { (start + len as i64).max(0) as usize } else { (start as usize).min(len) }
}

/// Insere `new_items` a partir de `start`.
pub fn insert_into<T: Clone>(array: &mut Vec<T>, start: i64, new_items: &[T]) {
    let start_idx = actual_start_index(array.len(), start);
    array.splice(start_idx..start_idx, new_items.iter().cloned());
}

/// Remove `delete_count` elementos a partir de `start` e insere `new_items` no lugar, devolvendo
/// os removidos. Alternativa ao `splice` nativo que não estoura a pilha com muitos itens.
pub fn splice<T: Clone>(array: &mut Vec<T>, start: i64, delete_count: usize, new_items: &[T]) -> Vec<T> {
    let index = actual_start_index(array.len(), start);
    let end = index.saturating_add(delete_count).min(array.len());
    array.splice(index..end, new_items.iter().cloned()).collect()
}

/// Comparador que olha só o que `selector` extrai.
pub fn compare_by<A, B>(selector: impl Fn(&A) -> B, comparator: impl Fn(&B, &B) -> CompareResult) -> impl Fn(&A, &A) -> CompareResult {
    move |a, b| comparator(&selector(a), &selector(b))
}

/// Usa os comparadores em ordem até um deles desempatar.
pub fn tie_break_comparators<T>(comparators: Vec<Comparator<T>>) -> impl Fn(&T, &T) -> CompareResult {
    move |item1, item2| {
        for comparator in &comparators {
            let result = comparator(item1, item2);
            if !compare_result::is_neither_less_or_greater_than(result) {
                return result;
            }
        }
        compare_result::NEITHER_LESS_OR_GREATER_THAN
    }
}

/// A ordem natural dos números.
pub fn number_comparator(a: &f64, b: &f64) -> CompareResult {
    a - b
}

pub fn boolean_comparator(a: &bool, b: &bool) -> CompareResult {
    number_comparator(&f64::from(u8::from(*a)), &f64::from(u8::from(*b)))
}

pub fn reverse_order<T>(comparator: impl Fn(&T, &T) -> CompareResult) -> impl Fn(&T, &T) -> CompareResult {
    move |a, b| -comparator(a, b)
}

/// Comparador que trata `None` (o `undefined`) como o menor valor.
pub fn compare_undefined_smallest<T>(comparator: impl Fn(&T, &T) -> CompareResult) -> impl Fn(&Option<T>, &Option<T>) -> CompareResult {
    move |a, b| match (a, b) {
        (None, None) => compare_result::NEITHER_LESS_OR_GREATER_THAN,
        (None, Some(_)) => compare_result::LESS_THAN,
        (Some(_), None) => compare_result::GREATER_THAN,
        (Some(a), Some(b)) => comparator(a, b),
    }
}

/// Fila apoiada num slice, que se consome pelas duas pontas.
pub struct ArrayQueue<'a, T> {
    items: &'a [T],
    first_idx: i64,
    last_idx: i64,
}

impl<'a, T: Clone> ArrayQueue<'a, T> {
    /// Fila apoiada no slice dado, em O(1).
    pub fn new(items: &'a [T]) -> Self {
        Self { items, first_idx: 0, last_idx: items.len() as i64 - 1 }
    }

    pub fn length(&self) -> i64 {
        self.last_idx - self.first_idx + 1
    }

    fn at(&self, index: i64) -> Option<&T> {
        usize::try_from(index).ok().and_then(|i| self.items.get(i))
    }

    /// Consome do início enquanto `predicate` valer; `None` se nada foi consumido.
    pub fn take_while(&mut self, predicate: impl Fn(&T) -> bool) -> Option<Vec<T>> {
        let mut start_idx = self.first_idx;
        while start_idx <= self.last_idx && predicate(&self.items[start_idx as usize]) {
            start_idx += 1;
        }
        let result = (start_idx != self.first_idx).then(|| self.items[self.first_idx as usize..start_idx as usize].to_vec());
        self.first_idx = start_idx;
        result
    }

    /// Consome do fim enquanto `predicate` valer; `None` se nada foi consumido. O resultado tem a
    /// mesma ordem do array de base.
    pub fn take_from_end_while(&mut self, predicate: impl Fn(&T) -> bool) -> Option<Vec<T>> {
        let mut end_idx = self.last_idx;
        while end_idx >= self.first_idx && predicate(&self.items[end_idx as usize]) {
            end_idx -= 1;
        }
        let result = (end_idx != self.last_idx).then(|| self.items[(end_idx + 1) as usize..(self.last_idx + 1) as usize].to_vec());
        self.last_idx = end_idx;
        result
    }

    pub fn peek(&self) -> Option<T> {
        if self.length() == 0 { None } else { self.at(self.first_idx).cloned() }
    }

    pub fn peek_last(&self) -> Option<T> {
        if self.length() == 0 { None } else { self.at(self.last_idx).cloned() }
    }

    /// Como o original, não confere se a fila está vazia: avança o início de qualquer jeito.
    pub fn dequeue(&mut self) -> Option<T> {
        let result = self.at(self.first_idx).cloned();
        self.first_idx += 1;
        result
    }

    /// Como [`ArrayQueue::dequeue`], pela outra ponta.
    pub fn remove_last(&mut self) -> Option<T> {
        let result = self.at(self.last_idx).cloned();
        self.last_idx -= 1;
        result
    }

    pub fn take_count(&mut self, count: usize) -> Vec<T> {
        let (from, to) = slice_bounds(self.items.len(), self.first_idx, Some(self.first_idx.saturating_add(count as i64)));
        self.first_idx += count as i64;
        self.items[from..to].to_vec()
    }
}

/// Mais rápido que um iterador sobre array para dado calculado sob demanda.
pub struct CallbackIterable<T: 'static> {
    iterate: Iterate<T>,
}

type Iterate<T> = Rc<dyn Fn(&mut dyn FnMut(T) -> bool)>;

impl<T: 'static> Clone for CallbackIterable<T> {
    fn clone(&self) -> Self {
        Self { iterate: self.iterate.clone() }
    }
}

impl<T: 'static> CallbackIterable<T> {
    /// `iterate` chama o callback para cada item e para quando ele devolve `false`.
    pub fn new(iterate: impl Fn(&mut dyn FnMut(T) -> bool) + 'static) -> Self {
        Self { iterate: Rc::new(iterate) }
    }

    pub fn empty() -> Self {
        Self::new(|_| {})
    }

    pub fn iterate(&self, callback: &mut dyn FnMut(T) -> bool) {
        (self.iterate)(callback);
    }

    pub fn for_each(&self, mut handler: impl FnMut(T)) {
        self.iterate(&mut |item| {
            handler(item);
            true
        });
    }

    pub fn to_array(&self) -> Vec<T> {
        let mut result = Vec::new();
        self.for_each(|item| result.push(item));
        result
    }

    pub fn filter(&self, predicate: impl Fn(&T) -> bool + 'static) -> CallbackIterable<T> {
        let this = self.clone();
        CallbackIterable::new(move |cb| this.iterate(&mut |item| if predicate(&item) { cb(item) } else { true }))
    }

    pub fn map<R: 'static>(&self, map_fn: impl Fn(T) -> R + 'static) -> CallbackIterable<R> {
        let this = self.clone();
        CallbackIterable::new(move |cb| this.iterate(&mut |item| cb(map_fn(item))))
    }

    pub fn some(&self, predicate: impl Fn(&T) -> bool) -> bool {
        let mut result = false;
        self.iterate(&mut |item| {
            result = predicate(&item);
            !result
        });
        result
    }

    pub fn find_first(&self, predicate: impl Fn(&T) -> bool) -> Option<T> {
        let mut result = None;
        self.iterate(&mut |item| {
            if predicate(&item) {
                result = Some(item);
                return false;
            }
            true
        });
        result
    }

    pub fn find_last(&self, predicate: impl Fn(&T) -> bool) -> Option<T> {
        let mut result = None;
        self.iterate(&mut |item| {
            if predicate(&item) {
                result = Some(item);
            }
            true
        });
        result
    }

    /// O último item que é maior ou igual a todos os outros.
    pub fn find_last_max_by(&self, comparator: impl Fn(&T, &T) -> CompareResult) -> Option<T> {
        let mut result: Option<T> = None;
        self.iterate(&mut |item| {
            if result.as_ref().is_none_or(|current| compare_result::is_greater_than(comparator(&item, current))) {
                result = Some(item);
            }
            true
        });
        result
    }
}

/// Um rearranjo de itens de um array.
#[derive(Clone, Debug, PartialEq)]
pub struct Permutation {
    index_map: Vec<usize>,
}

impl Permutation {
    pub fn new(index_map: Vec<usize>) -> Self {
        Self { index_map }
    }

    /// A permutação que ordena `arr` com `compare_fn` (ordenação estável dos índices).
    pub fn create_sort_permutation<T>(arr: &[T], compare_fn: impl Fn(&T, &T) -> CompareResult) -> Self {
        let mut sort_indices: Vec<usize> = (0..arr.len()).collect();
        sort_indices.sort_by(|&a, &b| to_ordering(compare_fn(&arr[a], &arr[b])));
        Self::new(sort_indices)
    }

    /// Novo array com os elementos de `arr` rearranjados por esta permutação.
    pub fn apply<T: Clone>(&self, arr: &[T]) -> Vec<T> {
        (0..arr.len()).map(|index| arr[self.index_map[index]].clone()).collect()
    }

    /// A permutação que desfaz esta.
    pub fn inverse(&self) -> Permutation {
        let mut inverse_index_map = self.index_map.clone();
        for (i, &target) in self.index_map.iter().enumerate() {
            inverse_index_map[target] = i;
        }
        Permutation::new(inverse_index_map)
    }
}

pub fn sum(array: &[f64]) -> f64 {
    array.iter().fold(0.0, |acc, value| acc + value)
}

pub fn sum_by<T>(array: &[T], selector: impl Fn(&T) -> f64) -> f64 {
    array.iter().fold(0.0, |acc, value| acc + selector(value))
}
