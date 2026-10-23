//! Porte de `src/vs/editor/common/core/edits/arrayEdit.ts`: `ArrayEdit` e `ArrayReplacement`.
//!
//! Os elementos são comparados com `PartialEq`, no lugar do `===` do original.

use std::fmt;

use je_base::errors::{self, Error};

use super::edit::{BaseEdit, BaseReplacement, validate_replacements};
use crate::offset_range::{OffsetRange, js_array_slice};

/// Um conjunto de substituições de um array. Todas são aplicadas de uma vez.
#[derive(Clone, Debug)]
pub struct ArrayEdit<T: Clone + PartialEq + fmt::Debug> {
    replacements: Vec<ArrayReplacement<T>>,
}

impl<T: Clone + PartialEq + fmt::Debug> ArrayEdit<T> {
    pub fn new(replacements: Vec<ArrayReplacement<T>>) -> Self {
        validate_replacements(&replacements);
        Self { replacements }
    }

    /// `ArrayEdit.empty`.
    pub fn empty() -> Self {
        Self::new(Vec::new())
    }

    pub fn create(replacements: Vec<ArrayReplacement<T>>) -> Self {
        Self::new(replacements)
    }

    pub fn single(replacement: ArrayReplacement<T>) -> Self {
        Self::new(vec![replacement])
    }

    pub fn replace(range: OffsetRange, replacement: Vec<T>) -> Self {
        Self::new(vec![ArrayReplacement::new(range, replacement)])
    }

    pub fn insert(offset: i32, replacement: Vec<T>) -> Self {
        Self::new(vec![ArrayReplacement::new(OffsetRange::empty_at(offset), replacement)])
    }

    pub fn delete(range: OffsetRange) -> Self {
        Self::new(vec![ArrayReplacement::new(range, Vec::new())])
    }

    pub fn apply(&self, data: &[T]) -> Vec<T> {
        let mut result_data: Vec<T> = Vec::new();
        let mut pos = 0;
        for edit in &self.replacements {
            result_data.extend(js_array_slice(data, pos, Some(edit.replace_range.start)));
            result_data.extend(edit.new_value.iter().cloned());
            pos = edit.replace_range.end_exclusive;
        }
        result_data.extend(js_array_slice(data, pos, None));
        result_data
    }

    /// Cria uma edição que desfaz esta.
    pub fn inverse(&self, base_val: &[T]) -> ArrayEdit<T> {
        let mut edits: Vec<ArrayReplacement<T>> = Vec::new();
        let mut offset = 0;
        for e in &self.replacements {
            edits.push(ArrayReplacement::new(
                OffsetRange::of_start_and_length(e.replace_range.start + offset, e.new_value.len() as i32),
                js_array_slice(base_val, e.replace_range.start, Some(e.replace_range.end_exclusive)),
            ));
            offset += e.new_value.len() as i32 - e.replace_range.length();
        }
        ArrayEdit::new(edits)
    }
}

impl<T: Clone + PartialEq + fmt::Debug> BaseEdit for ArrayEdit<T> {
    type Replacement = ArrayReplacement<T>;

    fn replacements(&self) -> &[ArrayReplacement<T>] {
        &self.replacements
    }

    fn create_new(replacements: Vec<ArrayReplacement<T>>) -> Self {
        Self::new(replacements)
    }
}

#[derive(Clone, Debug)]
pub struct ArrayReplacement<T: Clone + PartialEq + fmt::Debug> {
    pub replace_range: OffsetRange,
    pub new_value: Vec<T>,
}

impl<T: Clone + PartialEq + fmt::Debug> ArrayReplacement<T> {
    pub fn new(range: OffsetRange, new_value: Vec<T>) -> Self {
        Self { replace_range: range, new_value }
    }
}

impl<T: Clone + PartialEq + fmt::Debug> BaseReplacement for ArrayReplacement<T> {
    fn replace_range(&self) -> OffsetRange {
        self.replace_range
    }

    fn equals(&self, other: &Self) -> bool {
        self.replace_range.equals(other.replace_range) && self.new_value == other.new_value
    }

    fn get_new_length(&self) -> i32 {
        self.new_value.len() as i32
    }

    fn try_join_touching(&self, other: &Self) -> Option<Self> {
        let mut joined = self.new_value.clone();
        joined.extend(other.new_value.iter().cloned());
        Some(Self::new(self.replace_range.join_right_touching(other.replace_range), joined))
    }

    fn slice(&self, range: OffsetRange, range_in_replacement: Option<OffsetRange>) -> Self {
        // No original `rangeInReplacement` é obrigatório aqui: sem ele a leitura de `undefined` lança.
        let Some(range_in_replacement) = range_in_replacement else {
            errors::throw(Error::named("TypeError", "Cannot read properties of undefined (reading 'slice')"));
        };
        Self::new(range, range_in_replacement.slice(&self.new_value))
    }
}
