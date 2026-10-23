//! Porte de `src/vs/base/common/arraysFind.ts`.
//!
//! Os parâmetros opcionais do original (`fromIndex`, `startIdx`, `endIdxEx`) viram argumentos
//! explícitos: o padrão do `fromIndex` é `Option<i64>` (`None` = do fim, ou do início para os
//! `findFirst*`), e `start_idx`/`end_idx_ex` são sempre passados (`0` e `array.len()` são os
//! padrões do original). Índices que o TypeScript devolve como `-1` continuam `-1` (`i64`).
//! `MonotonousArray.assertInvariants` (estático no original) é [`set_assert_invariants`].

use std::rc::Rc;
use std::sync::atomic::{AtomicBool, Ordering};

use crate::arrays::{CompareResult, compare_result};
use crate::errors::{self, Error};

/// O último item em que o predicado vale, procurando de `from_index` para trás.
pub fn find_last<T>(array: &[T], predicate: impl Fn(&T, usize) -> bool, from_index: Option<i64>) -> Option<&T> {
    usize::try_from(find_last_idx(array, predicate, from_index)).ok().map(|i| &array[i])
}

/// Índice do último item em que o predicado vale, ou `-1`.
pub fn find_last_idx<T>(array: &[T], predicate: impl Fn(&T, usize) -> bool, from_index: Option<i64>) -> i64 {
    let mut i = from_index.unwrap_or(array.len() as i64 - 1);
    while i >= 0 {
        if let Some(element) = array.get(i as usize)
            && predicate(element, i as usize)
        {
            return i;
        }
        i -= 1;
    }
    -1
}

/// O primeiro item em que o predicado vale, procurando de `from_index` (padrão 0) para frente.
pub fn find_first<T>(array: &[T], predicate: impl Fn(&T, usize) -> bool, from_index: Option<usize>) -> Option<&T> {
    usize::try_from(find_first_idx(array, predicate, from_index)).ok().map(|i| &array[i])
}

/// Índice do primeiro item em que o predicado vale, ou `-1`.
pub fn find_first_idx<T>(array: &[T], predicate: impl Fn(&T, usize) -> bool, from_index: Option<usize>) -> i64 {
    for (i, element) in array.iter().enumerate().skip(from_index.unwrap_or(0)) {
        if predicate(element, i) {
            return i as i64;
        }
    }
    -1
}

/// Acha o último item em que o predicado vale por busca binária. `predicate` precisa ser monótono:
/// `arr.map(predicate)` tem a forma `[true, ..., true, false, ..., false]`.
pub fn find_last_monotonous<T>(array: &[T], predicate: impl Fn(&T) -> bool) -> Option<&T> {
    usize::try_from(find_last_idx_monotonous(array, predicate, 0, array.len())).ok().map(|i| &array[i])
}

/// Índice do último item em que o predicado monótono vale, ou `start_idx - 1` se não vale em nenhum.
pub fn find_last_idx_monotonous<T>(array: &[T], predicate: impl Fn(&T) -> bool, start_idx: usize, end_idx_ex: usize) -> i64 {
    let mut i = start_idx;
    let mut j = end_idx_ex;
    while i < j {
        let k = (i + j) / 2;
        if predicate(&array[k]) {
            i = k + 1;
        } else {
            j = k;
        }
    }
    i as i64 - 1
}

/// Acha o primeiro item em que o predicado vale por busca binária. `predicate` precisa ser monótono:
/// `[false, ..., false, true, ..., true]`.
pub fn find_first_monotonous<T>(array: &[T], predicate: impl Fn(&T) -> bool) -> Option<&T> {
    let idx = find_first_idx_monotonous_or_arr_len(array, predicate, 0, array.len());
    array.get(idx)
}

/// Índice do primeiro item em que o predicado monótono vale, ou `end_idx_ex` se não vale em nenhum.
pub fn find_first_idx_monotonous_or_arr_len<T>(array: &[T], predicate: impl Fn(&T) -> bool, start_idx: usize, end_idx_ex: usize) -> usize {
    let mut i = start_idx;
    let mut j = end_idx_ex;
    while i < j {
        let k = (i + j) / 2;
        if predicate(&array[k]) {
            j = k;
        } else {
            i = k + 1;
        }
    }
    i
}

/// Como [`find_first_idx_monotonous_or_arr_len`], mas `-1` quando o resultado é `array.len()`.
pub fn find_first_idx_monotonous<T>(array: &[T], predicate: impl Fn(&T) -> bool, start_idx: usize, end_idx_ex: usize) -> i64 {
    let idx = find_first_idx_monotonous_or_arr_len(array, predicate, start_idx, end_idx_ex);
    if idx == array.len() { -1 } else { idx as i64 }
}

type Predicate<T> = Rc<dyn Fn(&T) -> bool>;

static ASSERT_INVARIANTS: AtomicBool = AtomicBool::new(false);

/// `MonotonousArray.assertInvariants = value`.
pub fn set_assert_invariants(value: bool) {
    ASSERT_INVARIANTS.store(value, Ordering::Relaxed);
}

/// Use quando há um array ordenado consultado várias vezes com predicados monótonos cada vez mais
/// fracos, para achar o último item com certa propriedade.
pub struct MonotonousArray<'a, T> {
    array: &'a [T],
    find_last_monotonous_last_idx: usize,
    prev_find_last_predicate: Option<Predicate<T>>,
}

impl<'a, T> MonotonousArray<'a, T> {
    pub fn new(array: &'a [T]) -> Self {
        Self { array, find_last_monotonous_last_idx: 0, prev_find_last_predicate: None }
    }

    /// O predicado precisa ser monótono; a cada chamada seguinte precisa ser mais fraco (ou igual)
    /// que o anterior, isto é, ter mais entradas `true`.
    pub fn find_last_monotonous(&mut self, predicate: impl Fn(&T) -> bool + 'static) -> Option<&'a T> {
        let predicate: Predicate<T> = Rc::new(predicate);
        if ASSERT_INVARIANTS.load(Ordering::Relaxed) {
            if let Some(prev) = &self.prev_find_last_predicate
                && self.array.iter().any(|item| prev(item) && !predicate(item))
            {
                errors::throw(Error::new("MonotonousArray: current predicate must be weaker than (or equal to) the previous predicate."));
            }
            self.prev_find_last_predicate = Some(predicate.clone());
        }
        let idx = find_last_idx_monotonous(self.array, |item| predicate(item), self.find_last_monotonous_last_idx, self.array.len());
        self.find_last_monotonous_last_idx = (idx + 1) as usize;
        usize::try_from(idx).ok().map(|i| &self.array[i])
    }
}

/// O primeiro item que é maior ou igual a todos os outros.
pub fn find_first_max<T>(array: &[T], comparator: impl Fn(&T, &T) -> CompareResult) -> Option<&T> {
    let mut max = array.first()?;
    for item in &array[1..] {
        if comparator(item, max) > 0.0 {
            max = item;
        }
    }
    Some(max)
}

/// O último item que é maior ou igual a todos os outros.
pub fn find_last_max<T>(array: &[T], comparator: impl Fn(&T, &T) -> CompareResult) -> Option<&T> {
    let mut max = array.first()?;
    for item in &array[1..] {
        if comparator(item, max) >= 0.0 {
            max = item;
        }
    }
    Some(max)
}

/// O primeiro item que é menor ou igual a todos os outros.
pub fn find_first_min<T>(array: &[T], comparator: impl Fn(&T, &T) -> CompareResult) -> Option<&T> {
    find_first_max(array, |a, b| -comparator(a, b))
}

/// Índice do primeiro maior elemento, ou `-1` num array vazio.
pub fn find_max_idx<T>(array: &[T], comparator: impl Fn(&T, &T) -> CompareResult) -> i64 {
    if array.is_empty() {
        return -1;
    }
    let mut max_idx = 0;
    for (i, item) in array.iter().enumerate().skip(1) {
        if compare_result::is_greater_than(comparator(item, &array[max_idx])) {
            max_idx = i;
        }
    }
    max_idx as i64
}

/// O primeiro valor mapeado que não é `None`.
pub fn map_find_first<T, R>(items: impl IntoIterator<Item = T>, map_fn: impl Fn(T) -> Option<R>) -> Option<R> {
    items.into_iter().find_map(map_fn)
}
