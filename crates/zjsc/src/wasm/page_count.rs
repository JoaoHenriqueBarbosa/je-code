//! Tradução de `runtime/PageCount.h`. Fica de fora o `static_assert` com `MAX_ARRAY_BUFFER_SIZE`,
//! que o porte do array buffer ainda não define.

use crate::wasm::wasm_limits::{MAX_MEMORY32_PAGES, MAX_MEMORY64_PAGES, PAGE_SIZE};

/// `PageCount::invalidPageCount`: o valor do `PageCount()` padrão, que é "sem valor".
const INVALID_PAGE_COUNT: u64 = u64::MAX;

/// `PageCount`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct PageCount {
    page_count: u64,
}

impl Default for PageCount {
    fn default() -> PageCount {
        PageCount { page_count: INVALID_PAGE_COUNT }
    }
}

impl PageCount {
    /// `PageCount::maxPageCount`.
    pub const MAX_PAGE_COUNT: u64 = MAX_MEMORY64_PAGES;
    /// `PageCount::maxMemory32PageCount`.
    pub const MAX_MEMORY32_PAGE_COUNT: u64 = MAX_MEMORY32_PAGES;
    /// `PageCount::maxMemory32Bytes`.
    pub const MAX_MEMORY32_BYTES: u64 = MAX_MEMORY32_PAGES * PAGE_SIZE;

    pub const fn new(page_count: u64) -> PageCount {
        PageCount { page_count }
    }

    /// `bytes()`: satura em vez de dar a volta, para todo limite em bytes continuar recusando.
    pub fn bytes(self) -> u64 {
        if self.page_count > u64::MAX / PAGE_SIZE {
            return u64::MAX;
        }
        self.page_count * PAGE_SIZE
    }

    pub fn page_count(self) -> u64 {
        self.page_count
    }

    /// `isValid(uint64_t)`.
    pub fn is_valid_count(page_count: u64) -> bool {
        page_count <= Self::MAX_PAGE_COUNT
    }

    /// `isValid()`.
    pub fn is_valid(self) -> bool {
        Self::is_valid_count(self.page_count)
    }

    /// `fromBytesUnchecked`.
    pub fn from_bytes_unchecked(bytes: u64) -> PageCount {
        assert!(bytes % PAGE_SIZE == 0);
        PageCount::new(bytes / PAGE_SIZE)
    }

    /// `fromBytes`.
    pub fn from_bytes(bytes: u64) -> PageCount {
        let count = PageCount::from_bytes_unchecked(bytes);
        assert!(count.is_valid());
        count
    }

    /// `fromBytesWithRoundUp`.
    pub fn from_bytes_with_round_up(bytes: u64) -> PageCount {
        PageCount::from_bytes(bytes.next_multiple_of(PAGE_SIZE))
    }

    /// `max()`.
    pub fn max() -> PageCount {
        PageCount::new(Self::MAX_PAGE_COUNT)
    }

    /// `explicit operator bool`: tem valor, não é o `PageCount()` padrão.
    pub fn has_value(self) -> bool {
        self.page_count != INVALID_PAGE_COUNT
    }
}

impl std::ops::Add for PageCount {
    type Output = PageCount;

    /// `operator+`: a soma que estoura ou passa de `maxPageCount` é o `PageCount()` padrão.
    fn add(self, other: PageCount) -> PageCount {
        let Some(new_count) = self.page_count.checked_add(other.page_count) else {
            return PageCount::default();
        };
        if !PageCount::is_valid_count(new_count) {
            return PageCount::default();
        }
        PageCount::new(new_count)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_has_no_value() {
        assert!(!PageCount::default().has_value());
        assert!(PageCount::new(0).has_value());
        // O padrão é o maior valor, então `maximum >= initial` vale quando não há máximo.
        assert!(PageCount::default() >= PageCount::new(7));
    }

    #[test]
    fn bytes_saturate() {
        assert_eq!(PageCount::new(2).bytes(), 2 * 65536);
        assert_eq!(PageCount::max().bytes(), u64::MAX);
        assert_eq!(PageCount::from_bytes_with_round_up(65537).page_count(), 2);
    }

    #[test]
    fn sum_past_the_maximum_is_invalid() {
        assert_eq!((PageCount::new(1) + PageCount::new(2)).page_count(), 3);
        assert!(!(PageCount::max() + PageCount::new(1)).has_value());
        assert!(!(PageCount::new(u64::MAX - 1) + PageCount::new(5)).has_value());
    }
}
