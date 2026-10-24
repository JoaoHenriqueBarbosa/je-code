//! Porte de `runtime/GenericOffset.h`.
//!
//! O mixin `GenericOffset<T>` do C++ (CRTP) vira um struct genérico sobre uma marca de tipo:
//! `ScopeOffset` e `DirectArgumentsOffset` são aliases com marcas distintas, então os dois
//! continuam sendo tipos diferentes, como no C++.

use std::cmp::Ordering;
use std::hash::{Hash, Hasher};
use std::marker::PhantomData;
use std::ops::{Add, AddAssign, Sub, SubAssign};

/// `GenericOffset::invalidOffset`.
pub const INVALID_OFFSET: u32 = u32::MAX;

pub struct GenericOffset<T> {
    offset: u32,
    marker: PhantomData<fn() -> T>,
}

impl<T> GenericOffset<T> {
    pub const INVALID_OFFSET: u32 = INVALID_OFFSET;

    pub const fn new(offset: u32) -> Self {
        GenericOffset { offset, marker: PhantomData }
    }

    /// `operator!`: verdadeiro quando o deslocamento é inválido.
    pub fn is_invalid(&self) -> bool {
        self.offset == INVALID_OFFSET
    }

    /// `explicit operator bool` implícito do C++ (`!!offset`).
    pub fn is_valid(&self) -> bool {
        !self.is_invalid()
    }

    pub fn offset_unchecked(&self) -> u32 {
        self.offset
    }

    pub fn offset(&self) -> u32 {
        debug_assert!(self.offset != INVALID_OFFSET);
        self.offset
    }
}

impl<T> Default for GenericOffset<T> {
    fn default() -> Self {
        GenericOffset::new(INVALID_OFFSET)
    }
}

impl<T> Clone for GenericOffset<T> {
    fn clone(&self) -> Self {
        *self
    }
}

impl<T> Copy for GenericOffset<T> {}

impl<T> std::fmt::Debug for GenericOffset<T> {
    fn fmt(&self, out: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        out.debug_tuple("GenericOffset").field(&self.offset).finish()
    }
}

impl<T> PartialEq for GenericOffset<T> {
    fn eq(&self, other: &Self) -> bool {
        self.offset == other.offset
    }
}

impl<T> Eq for GenericOffset<T> {}

impl<T> PartialOrd for GenericOffset<T> {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl<T> Ord for GenericOffset<T> {
    fn cmp(&self, other: &Self) -> Ordering {
        self.offset.cmp(&other.offset)
    }
}

impl<T> Hash for GenericOffset<T> {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.offset.hash(state);
    }
}

impl<T> Add<i32> for GenericOffset<T> {
    type Output = GenericOffset<T>;
    fn add(self, value: i32) -> GenericOffset<T> {
        GenericOffset::new(self.offset().wrapping_add(value as u32))
    }
}

impl<T> Sub<i32> for GenericOffset<T> {
    type Output = GenericOffset<T>;
    fn sub(self, value: i32) -> GenericOffset<T> {
        GenericOffset::new(self.offset().wrapping_sub(value as u32))
    }
}

impl<T> AddAssign<i32> for GenericOffset<T> {
    fn add_assign(&mut self, value: i32) {
        *self = *self + value;
    }
}

impl<T> SubAssign<i32> for GenericOffset<T> {
    fn sub_assign(&mut self, value: i32) {
        *self = *self - value;
    }
}
