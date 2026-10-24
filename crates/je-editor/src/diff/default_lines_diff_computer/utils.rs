//! Porte de `src/vs/editor/common/diff/defaultLinesDiffComputer/utils.ts`: `Array2D`, `isSpace` e `LineRangeFragment`.
//!
//! Representação:
//! - `Array2D<T>` é o [`Array2D`] com o valor padrão do tipo nas posições ainda não escritas (o `T[]` do original as
//!   deixa `undefined`, e o algoritmo só lê o que já escreveu).
//! - `isSpace(charCode)` recebe `i32`, porque o original chama com `-1` (fim da sequência) e com o `NaN` de um índice fora
//!   da linha.
//! - `LineRangeFragment.chrKeys` (um `static Map<string, number>` do carregamento do módulo) é um `thread_local` indexado
//!   pela unidade de código UTF-16 (o `line[j]` do original é uma unidade).
//! - `LineRangeFragment.source` (o `DetailedLineRangeMapping` de origem, comparado por identidade em
//!   `computeMovedLines`) é o índice da mudança na lista `changes`: [`LineRangeFragment::source_index`]. O fragmento não
//!   guarda `lines` (só o construtor as lê).

use std::cell::RefCell;
use std::collections::HashMap;

use je_base::JsString;

use crate::line_range::LineRange;

/// `Array2D<T>`.
pub struct Array2D<T: Copy + Default> {
    array: Vec<T>,
    pub width: i32,
    pub height: i32,
}

impl<T: Copy + Default> Array2D<T> {
    pub fn new(width: i32, height: i32) -> Self {
        Self { array: vec![T::default(); (width as usize) * (height as usize)], width, height }
    }

    pub fn get(&self, x: i32, y: i32) -> T {
        self.array[(x + y * self.width) as usize]
    }

    pub fn set(&mut self, x: i32, y: i32, value: T) {
        let index = (x + y * self.width) as usize;
        self.array[index] = value;
    }
}

/// `isSpace(charCode)`.
pub fn is_space(char_code: i32) -> bool {
    char_code == i32::from(b' ') || char_code == i32::from(b'\t')
}

thread_local! {
    /// `LineRangeFragment.chrKeys`.
    static CHR_KEYS: RefCell<HashMap<u16, usize>> = RefCell::new(HashMap::new());
}

/// `LineRangeFragment`.
pub struct LineRangeFragment {
    pub range: LineRange,
    /// O índice, na lista de mudanças, do `DetailedLineRangeMapping` de que o fragmento vem (`source`).
    pub source_index: usize,
    total_count: i32,
    histogram: Vec<i32>,
}

impl LineRangeFragment {
    fn get_key(chr: u16) -> usize {
        CHR_KEYS.with(|chr_keys| {
            let mut chr_keys = chr_keys.borrow_mut();
            let next = chr_keys.len();
            *chr_keys.entry(chr).or_insert(next)
        })
    }

    pub fn new(range: LineRange, lines: &[JsString], source_index: usize) -> Self {
        let mut histogram: Vec<i32> = Vec::new();
        let mut counter = 0;
        let count_key = |histogram: &mut Vec<i32>, key: usize| {
            if key >= histogram.len() {
                histogram.resize(key + 1, 0);
            }
            histogram[key] += 1;
        };
        for i in (range.start_line_number - 1)..(range.end_line_number_exclusive - 1) {
            let line = &lines[i as usize];
            for &chr in line.units() {
                counter += 1;
                count_key(&mut histogram, Self::get_key(chr));
            }
            counter += 1;
            count_key(&mut histogram, Self::get_key(u16::from(b'\n')));
        }

        Self { range, source_index, total_count: counter, histogram }
    }

    pub fn compute_similarity(&self, other: &LineRangeFragment) -> f64 {
        let mut sum_differences = 0;
        let max_length = self.histogram.len().max(other.histogram.len());
        for i in 0..max_length {
            sum_differences += (self.histogram.get(i).copied().unwrap_or(0) - other.histogram.get(i).copied().unwrap_or(0)).abs();
        }
        1.0 - (f64::from(sum_differences) / f64::from(self.total_count + other.total_count))
    }
}
