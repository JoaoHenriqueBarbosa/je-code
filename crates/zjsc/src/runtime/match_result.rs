//! Porte de `runtime/MatchResult.h`.
//!
//! DIVERGÊNCIA: o construtor a partir de `UGPRPair` (retorno do JIT do Yarr) não existe; o
//! `WTF::notFound` é `usize::MAX`, como no C++.

/// `WTF::notFound`.
pub const NOT_FOUND: usize = usize::MAX;

/// `struct MatchResult`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MatchResult {
    pub start: usize,
    pub end: usize,
}

impl Default for MatchResult {
    fn default() -> MatchResult {
        MatchResult::failed()
    }
}

impl MatchResult {
    /// `MatchResult(size_t start, size_t end)`.
    pub const fn new(start: usize, end: usize) -> MatchResult {
        MatchResult { start, end }
    }

    /// `failed()`.
    pub const fn failed() -> MatchResult {
        MatchResult { start: NOT_FOUND, end: 0 }
    }

    /// `explicit operator bool()`.
    pub const fn matched(&self) -> bool {
        self.start != NOT_FOUND
    }

    /// `empty()`.
    pub const fn empty(&self) -> bool {
        self.start == self.end
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn failed_is_not_a_match() {
        assert!(!MatchResult::failed().matched());
        assert!(MatchResult::new(2, 2).matched());
        assert!(MatchResult::new(2, 2).empty());
        assert!(!MatchResult::new(2, 3).empty());
    }
}
