//! Tradução de `wtf/FixedVector.h` (a parte de leitura e construção que o `Parser` usa).
//!
//! O C++ guarda um `unique_ptr<EmbeddedFixedVector>` nulo quando vazio; `Box<[T]>` vazio tem o mesmo
//! papel, sem alocação.

use std::ops::{Index, IndexMut};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FixedVector<T> {
    storage: Box<[T]>,
}

impl<T> Default for FixedVector<T> {
    fn default() -> Self {
        FixedVector { storage: Box::new([]) }
    }
}

impl<T> FixedVector<T> {
    /// `FixedVector(Vector&&)`.
    pub fn from_vec(vector: Vec<T>) -> FixedVector<T> {
        FixedVector { storage: vector.into_boxed_slice() }
    }

    /// `createWithSizeFromGenerator`.
    pub fn create_with_size_from_generator(size: usize, mut generator: impl FnMut(usize) -> T) -> FixedVector<T> {
        FixedVector { storage: (0..size).map(&mut generator).collect() }
    }

    pub fn size(&self) -> usize {
        self.storage.len()
    }

    pub fn is_empty(&self) -> bool {
        self.storage.is_empty()
    }

    pub fn iter(&self) -> std::slice::Iter<'_, T> {
        self.storage.iter()
    }

    pub fn iter_mut(&mut self) -> std::slice::IterMut<'_, T> {
        self.storage.iter_mut()
    }

    pub fn first(&self) -> &T {
        &self.storage[0]
    }

    pub fn last(&self) -> &T {
        &self.storage[self.storage.len() - 1]
    }

    pub fn clear(&mut self) {
        self.storage = Box::new([]);
    }

    pub fn as_slice(&self) -> &[T] {
        &self.storage
    }
}

impl<T: Clone> FixedVector<T> {
    /// `FixedVector(FillWith, size, value)`.
    pub fn filled(size: usize, value: &T) -> FixedVector<T> {
        FixedVector { storage: vec![value.clone(); size].into_boxed_slice() }
    }

    pub fn fill(&mut self, value: &T) {
        for slot in self.storage.iter_mut() {
            *slot = value.clone();
        }
    }
}

impl<T> Index<usize> for FixedVector<T> {
    type Output = T;

    fn index(&self, index: usize) -> &T {
        &self.storage[index]
    }
}

impl<T> IndexMut<usize> for FixedVector<T> {
    fn index_mut(&mut self, index: usize) -> &mut T {
        &mut self.storage[index]
    }
}

impl<'a, T> IntoIterator for &'a FixedVector<T> {
    type Item = &'a T;
    type IntoIter = std::slice::Iter<'a, T>;

    fn into_iter(self) -> Self::IntoIter {
        self.storage.iter()
    }
}
