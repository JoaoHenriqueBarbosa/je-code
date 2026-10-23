//! Porte de `src/vs/editor/common/model/prefixSumComputer.ts`.
//!
//! As duas estruturas calculam as somas de prefixo sob demanda, e por isso as leituras (`getTotalSum`,
//! `getPrefixSum`, `getIndexOf`) mexem no cache: aqui elas recebem `&self` e o cache fica em `Cell` e
//! `RefCell`, como o `prefixSumValidIndex` em `Int32Array` do original, que existe para ser alterado por
//! um método de leitura.
//!
//! O `PrefixSumComputer` guarda `u32` como o `Uint32Array` do original, inclusive no transbordamento das
//! somas (que dão a volta em 2^32). O `ConstantTimePrefixSumComputer` guarda `number[]` no original; aqui
//! os valores são `u32` e as somas `usize`, porque `_indexBySum` tem uma entrada por unidade da soma.
//!
//! Divergências nas bordas onde o JavaScript não falha:
//! - `PrefixSumComputer::get_prefix_sum` num computador vazio devolve 0 (o original devolve `undefined`).
//! - `ConstantTimePrefixSumComputer::set_value` e `get_prefix_sum` com índice além do fim entram em
//!   pânico; o original cria um array esparso ou devolve `undefined`.

use std::cell::{Cell, RefCell};

use je_base::arrays::array_insert;

/// `PrefixSumComputer`: somas de prefixo com invalidação a partir do índice alterado e busca binária.
#[derive(Clone, Debug)]
pub struct PrefixSumComputer {
    /// `values[i]` é o valor no índice `i`.
    values: Vec<u32>,
    /// `prefix_sum[i] = SUM(values[j]), 0 <= j <= i`.
    prefix_sum: RefCell<Vec<u32>>,
    /// `prefix_sum[i]`, `0 <= i <= prefix_sum_valid_index`, é confiável.
    prefix_sum_valid_index: Cell<isize>,
}

impl PrefixSumComputer {
    pub fn new(values: Vec<u32>) -> Self {
        let len = values.len();
        Self {
            values,
            prefix_sum: RefCell::new(vec![0; len]),
            prefix_sum_valid_index: Cell::new(-1),
        }
    }

    pub fn get_count(&self) -> usize {
        self.values.len()
    }

    /// Baixa o índice confiável para `index - 1` quando ele está acima.
    fn invalidate_from(&self, index: usize) {
        let last_valid = index as isize - 1;
        if last_valid < self.prefix_sum_valid_index.get() {
            self.prefix_sum_valid_index.set(last_valid);
        }
    }

    /// Recria o cache com o tamanho de `values`, preservando a parte confiável.
    fn rebuild_prefix_sum(&mut self) {
        let mut prefix_sum = vec![0; self.values.len()];
        let valid = self.prefix_sum_valid_index.get();
        if valid >= 0 {
            let keep = valid as usize + 1;
            prefix_sum[..keep].copy_from_slice(&self.prefix_sum.get_mut()[..keep]);
        }
        *self.prefix_sum.get_mut() = prefix_sum;
    }

    pub fn insert_values(&mut self, insert_index: usize, insert_values: &[u32]) -> bool {
        if insert_values.is_empty() {
            return false;
        }
        self.values
            .splice(insert_index..insert_index, insert_values.iter().copied());
        self.invalidate_from(insert_index);
        self.rebuild_prefix_sum();
        true
    }

    pub fn set_value(&mut self, index: usize, value: u32) -> bool {
        if self.values.get(index) == Some(&value) {
            return false;
        }
        // O `Uint32Array` ignora a escrita fora do intervalo, mas a invalidação acontece do mesmo jeito.
        if let Some(slot) = self.values.get_mut(index) {
            *slot = value;
        }
        self.invalidate_from(index);
        true
    }

    pub fn remove_values(&mut self, start_index: usize, count: usize) -> bool {
        let old_len = self.values.len();
        if start_index >= old_len {
            return false;
        }
        let count = count.min(old_len - start_index);
        if count == 0 {
            return false;
        }
        self.values.drain(start_index..start_index + count);
        self.invalidate_from(start_index);
        self.rebuild_prefix_sum();
        true
    }

    pub fn get_total_sum(&self) -> u32 {
        match self.values.len() {
            0 => 0,
            len => self.prefix_sum_at(len - 1),
        }
    }

    /// A soma dos primeiros `index + 1` itens: `SUM(0 <= j <= index, values[j])`. Índice negativo dá 0
    /// e índice além do fim vale o último.
    pub fn get_prefix_sum(&self, index: i64) -> u32 {
        if index < 0 || self.values.is_empty() {
            return 0;
        }
        self.prefix_sum_at(usize::try_from(index).unwrap_or(usize::MAX))
    }

    /// `_getPrefixSum`. Só é chamado com `values` não vazio.
    fn prefix_sum_at(&self, index: usize) -> u32 {
        let valid = self.prefix_sum_valid_index.get();
        let mut prefix_sum = self.prefix_sum.borrow_mut();
        if (index as isize) <= valid {
            return prefix_sum[index];
        }

        let mut start_index = (valid + 1) as usize;
        if start_index == 0 {
            prefix_sum[0] = self.values[0];
            start_index += 1;
        }

        let index = index.min(self.values.len() - 1);
        for i in start_index..=index {
            prefix_sum[i] = prefix_sum[i - 1].wrapping_add(self.values[i]);
        }
        self.prefix_sum_valid_index.set(valid.max(index as isize));
        prefix_sum[index]
    }

    pub fn get_index_of(&self, sum: i64) -> PrefixSumIndexOfResult {
        // Calcula todas as somas (deixa o cache inteiro confiável).
        self.get_total_sum();
        let prefix_sum = self.prefix_sum.borrow();

        let mut low: i64 = 0;
        let mut high: i64 = self.values.len() as i64 - 1;
        let mut mid: i64 = 0;
        let mut mid_start: i64 = 0;

        while low <= high {
            mid = low + (high - low) / 2;

            let mid_stop = i64::from(prefix_sum[mid as usize]);
            mid_start = mid_stop - i64::from(self.values[mid as usize]);

            if sum < mid_start {
                high = mid - 1;
            } else if sum >= mid_stop {
                low = mid + 1;
            } else {
                break;
            }
        }

        PrefixSumIndexOfResult::new(mid as usize, sum - mid_start)
    }
}

/// `ConstantTimePrefixSumComputer`: [`get_index_of`](Self::get_index_of) em O(1) amortizado, à custa de
/// uma entrada por unidade da soma total (`PrefixSumComputer::get_index_of` é O(log n)).
#[derive(Clone, Debug)]
pub struct ConstantTimePrefixSumComputer {
    values: Vec<u32>,
    is_valid: Cell<bool>,
    valid_end_index: Cell<isize>,
    /// `prefix_sum[i] = SUM(values[j]), 0 <= j <= i`.
    prefix_sum: RefCell<Vec<usize>>,
    /// `index_by_sum[sum] = idx` quando `prefix_sum[idx - 1] <= sum < prefix_sum[idx]`.
    index_by_sum: RefCell<Vec<usize>>,
}

impl ConstantTimePrefixSumComputer {
    pub fn new(values: Vec<u32>) -> Self {
        Self {
            values,
            is_valid: Cell::new(false),
            valid_end_index: Cell::new(-1),
            prefix_sum: RefCell::new(Vec::new()),
            index_by_sum: RefCell::new(Vec::new()),
        }
    }

    /// `SUM(0 <= j < values.len(), values[j])`.
    pub fn get_total_sum(&self) -> usize {
        self.ensure_valid();
        self.index_by_sum.borrow().len()
    }

    /// A soma dos primeiros `count` itens: `SUM(0 <= j < count, values[j])`.
    pub fn get_prefix_sum(&self, count: usize) -> usize {
        self.ensure_valid();
        if count == 0 {
            return 0;
        }
        self.prefix_sum.borrow()[count - 1]
    }

    /// O `result` tal que `get_prefix_sum(result.index) + result.remainder == sum`.
    pub fn get_index_of(&self, sum: i64) -> PrefixSumIndexOfResult {
        self.ensure_valid();
        let prefix_sum = self.prefix_sum.borrow();
        let sum_above = |idx: usize| {
            if idx > 0 {
                prefix_sum[idx - 1] as i64
            } else {
                0
            }
        };
        let idx = usize::try_from(sum)
            .ok()
            .and_then(|sum| self.index_by_sum.borrow().get(sum).copied());
        match idx {
            Some(idx) => PrefixSumIndexOfResult::new(idx, sum - sum_above(idx)),
            None => {
                // A soma não tem entrada direta (soma >= total, array vazio ou só zeros).
                let last_idx = self.values.len().saturating_sub(1);
                PrefixSumIndexOfResult::new(last_idx, sum - sum_above(last_idx))
            }
        }
    }

    /// `values.splice(start, deleteCount)`.
    pub fn remove_values(&mut self, start: usize, delete_count: usize) {
        let from = start.min(self.values.len());
        let to = from.saturating_add(delete_count).min(self.values.len());
        self.values.drain(from..to);
        self.invalidate(start);
    }

    pub fn insert_values(&mut self, insert_index: usize, insert_arr: &[u32]) {
        self.values = array_insert(&self.values, insert_index as i64, insert_arr);
        self.invalidate(insert_index);
    }

    fn invalidate(&self, index: usize) {
        self.is_valid.set(false);
        self.valid_end_index
            .set(self.valid_end_index.get().min(index as isize - 1));
    }

    fn ensure_valid(&self) {
        if self.is_valid.get() {
            return;
        }

        let mut prefix_sum = self.prefix_sum.borrow_mut();
        let mut index_by_sum = self.index_by_sum.borrow_mut();
        let len = self.values.len();
        prefix_sum.resize(len, 0);
        let first = (self.valid_end_index.get() + 1) as usize;
        for i in first..len {
            let value = self.values[i] as usize;
            let sum_above = if i > 0 { prefix_sum[i - 1] } else { 0 };

            prefix_sum[i] = sum_above + value;
            if index_by_sum.len() < sum_above + value {
                index_by_sum.resize(sum_above + value, 0);
            }
            index_by_sum[sum_above..sum_above + value].fill(i);
        }

        // Apara o que sobrou do estado anterior.
        index_by_sum.truncate(if len > 0 { prefix_sum[len - 1] } else { 0 });

        self.is_valid.set(true);
        self.valid_end_index.set(len as isize - 1);
    }

    pub fn set_value(&mut self, index: usize, value: u32) {
        if self.values[index] == value {
            // Nada mudou.
            return;
        }
        self.values[index] = value;
        self.invalidate(index);
    }
}

/// `PrefixSumIndexOfResult`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PrefixSumIndexOfResult {
    pub index: usize,
    pub remainder: i64,
}

impl PrefixSumIndexOfResult {
    pub fn new(index: usize, remainder: i64) -> Self {
        Self { index, remainder }
    }
}
