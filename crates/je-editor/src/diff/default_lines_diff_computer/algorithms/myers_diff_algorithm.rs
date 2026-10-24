//! Porte de `src/vs/editor/common/diff/defaultLinesDiffComputer/algorithms/myersDiffAlgorithm.ts`: o algoritmo O(ND) de Myers,
//! que tem complexidade quadrática de espaço no pior caso (`MyersDiffAlgorithm`).
//!
//! Representação:
//! - `FastInt32Array` e `FastArrayNegativeIndices<T>` (arrays com índice negativo) são as structs privadas
//!   [`FastInt32Array`] e [`FastArrayNegativeIndices`]. O `Int32Array` nasce zerado com 10 posições e dobra de tamanho na
//!   escrita; uma leitura além do que foi alocado devolve zero (o original devolveria `undefined`, o que o algoritmo nunca
//!   lê, porque só lê diagonais que o passo anterior escreveu). O `T[]` de `FastArrayNegativeIndices` com buracos devolve
//!   `undefined` para o que não foi escrito, e `undefined` e `null` valem o mesmo no algoritmo (`!path`): o `None`.
//! - `SnakePath` é encadeado por `Rc` (o `prev` compartilhado), como as referências do original.

use std::rc::Rc;

use super::diff_algorithm::{DiffAlgorithmResult, IDiffAlgorithm, ISequence, ITimeout, InfiniteTimeout, SequenceDiff};
use crate::offset_range::OffsetRange;

/// An O(ND) diff algorithm that has a quadratic space worst-case complexity.
pub struct MyersDiffAlgorithm;

impl IDiffAlgorithm for MyersDiffAlgorithm {
    fn compute(&self, seq1: &dyn ISequence, seq2: &dyn ISequence, timeout: Option<&dyn ITimeout>) -> DiffAlgorithmResult {
        let timeout = timeout.unwrap_or(&InfiniteTimeout::INSTANCE);

        // These are common special cases.
        // The early return improves performance dramatically.
        if seq1.length() == 0 || seq2.length() == 0 {
            return DiffAlgorithmResult::trivial(seq1, seq2);
        }

        let seq_x = seq1; // Text on the x axis
        let seq_y = seq2; // Text on the y axis

        let get_x_after_snake = |mut x: i32, mut y: i32| -> i32 {
            while x < seq_x.length() && y < seq_y.length() && seq_x.get_element(x) == seq_y.get_element(y) {
                x += 1;
                y += 1;
            }
            x
        };

        let mut d = 0;
        // V[k]: X value of longest d-line that ends in diagonal k.
        // d-line: path from (0,0) to (x,y) that uses exactly d non-diagonals.
        // diagonal k: Set of points (x,y) with x-y = k.
        // k=1 -> (1,0),(2,1)
        let mut v = FastInt32Array::new();
        v.set(0, get_x_after_snake(0, 0));

        let mut paths: FastArrayNegativeIndices<Option<Rc<SnakePath>>> = FastArrayNegativeIndices::new();
        paths.set(0, if v.get(0) == 0 { None } else { Some(Rc::new(SnakePath { prev: None, x: 0, y: 0, length: v.get(0) })) });

        let mut k;

        'search: loop {
            d += 1;
            if !timeout.is_valid() {
                return DiffAlgorithmResult::trivial_timed_out(seq_x, seq_y);
            }
            // The paper has `for (k = -d; k <= d; k += 2)`, but we can ignore diagonals that cannot influence the result.
            let lower_bound = -d.min(seq_y.length() + (d % 2));
            let upper_bound = d.min(seq_x.length() + (d % 2));
            k = lower_bound;
            while k <= upper_bound {
                // We can use the X values of (d-1)-lines to compute X value of the longest d-lines.
                let max_x_of_d_line_top = if k == upper_bound { -1 } else { v.get(k + 1) }; // We take a vertical non-diagonal (add a symbol in seqX)
                let max_x_of_d_line_left = if k == lower_bound { -1 } else { v.get(k - 1) + 1 }; // We take a horizontal non-diagonal (+1 x) (delete a symbol in seqX)
                let x = max_x_of_d_line_top.max(max_x_of_d_line_left).min(seq_x.length());
                let y = x - k;
                if x > seq_x.length() || y > seq_y.length() {
                    // This diagonal is irrelevant for the result.
                    // TODO: Don't pay the cost for this in the next iteration.
                    k += 2;
                    continue;
                }
                let new_max_x = get_x_after_snake(x, y);
                v.set(k, new_max_x);
                let last_path = if x == max_x_of_d_line_top { paths.get(k + 1) } else { paths.get(k - 1) };
                paths.set(
                    k,
                    if new_max_x != x { Some(Rc::new(SnakePath { prev: last_path, x, y, length: new_max_x - x })) } else { last_path },
                );

                if v.get(k) == seq_x.length() && v.get(k) - k == seq_y.length() {
                    break 'search;
                }
                k += 2;
            }
        }

        let mut path = paths.get(k);
        let mut result: Vec<SequenceDiff> = Vec::new();
        let mut last_aligning_pos_s1 = seq_x.length();
        let mut last_aligning_pos_s2 = seq_y.length();

        loop {
            let end_x = path.as_ref().map_or(0, |path| path.x + path.length);
            let end_y = path.as_ref().map_or(0, |path| path.y + path.length);

            if end_x != last_aligning_pos_s1 || end_y != last_aligning_pos_s2 {
                result.push(SequenceDiff::new(OffsetRange::new(end_x, last_aligning_pos_s1), OffsetRange::new(end_y, last_aligning_pos_s2)));
            }
            let Some(current) = path else {
                break;
            };
            last_aligning_pos_s1 = current.x;
            last_aligning_pos_s2 = current.y;

            path = current.prev.clone();
        }

        result.reverse();
        DiffAlgorithmResult::new(result, false)
    }
}

struct SnakePath {
    prev: Option<Rc<SnakePath>>,
    x: i32,
    y: i32,
    length: i32,
}

/// An array that supports fast negative indices.
struct FastInt32Array {
    positive_arr: Vec<i32>,
    negative_arr: Vec<i32>,
}

impl FastInt32Array {
    fn new() -> Self {
        Self { positive_arr: vec![0; 10], negative_arr: vec![0; 10] }
    }

    fn get(&self, idx: i32) -> i32 {
        let (array, index) = if idx < 0 { (&self.negative_arr, (-idx - 1) as usize) } else { (&self.positive_arr, idx as usize) };
        array.get(index).copied().unwrap_or(0)
    }

    fn set(&mut self, idx: i32, value: i32) {
        let (array, index) = if idx < 0 { (&mut self.negative_arr, (-idx - 1) as usize) } else { (&mut self.positive_arr, idx as usize) };
        if index >= array.len() {
            array.resize(array.len() * 2, 0);
        }
        if let Some(slot) = array.get_mut(index) {
            *slot = value;
        }
    }
}

/// An array that supports fast negative indices.
struct FastArrayNegativeIndices<T: Clone + Default> {
    positive_arr: Vec<T>,
    negative_arr: Vec<T>,
}

impl<T: Clone + Default> FastArrayNegativeIndices<T> {
    fn new() -> Self {
        Self { positive_arr: Vec::new(), negative_arr: Vec::new() }
    }

    fn get(&self, idx: i32) -> T {
        let (array, index) = if idx < 0 { (&self.negative_arr, (-idx - 1) as usize) } else { (&self.positive_arr, idx as usize) };
        array.get(index).cloned().unwrap_or_default()
    }

    fn set(&mut self, idx: i32, value: T) {
        let (array, index) = if idx < 0 { (&mut self.negative_arr, (-idx - 1) as usize) } else { (&mut self.positive_arr, idx as usize) };
        if index >= array.len() {
            array.resize(index + 1, T::default());
        }
        array[index] = value;
    }
}
