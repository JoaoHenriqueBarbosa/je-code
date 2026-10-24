//! Tradução de `runtime/FunctionHasExecutedCache.h` e `.cpp`.
//!
//! O `VM` entrega `&FunctionHasExecutedCache` (`functionHasExecutedCache()` devolve `this`), e os
//! chamadores inserem faixas por ele, então as tabelas ficam atrás de `RefCell`.
//!
//! DIVERGÊNCIA: o `UncheckedKeyHashMap` do C++ itera em ordem de tabela hash; aqui `getFunctionRanges`
//! devolve as faixas ordenadas por `(start, end)`, que é determinística e não é observável pela
//! inspeção (o inspector reordena por início).

use std::cell::RefCell;
use std::collections::HashMap;

use crate::parser::source_provider::SourceID;

/// `FunctionRange`: `m_start` e `m_end`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct FunctionRange {
    pub start: u32,
    pub end: u32,
}

/// `class FunctionHasExecutedCache`.
#[derive(Debug, Default)]
pub struct FunctionHasExecutedCache {
    /// `m_rangeMap`: `SourceID` para (faixa para "já executou").
    range_map: RefCell<HashMap<SourceID, HashMap<FunctionRange, bool>>>,
}

impl FunctionHasExecutedCache {
    /// `hasExecutedAtOffset(SourceID, unsigned)`: vale a menor faixa que contém o deslocamento.
    pub fn has_executed_at_offset(&self, id: SourceID, offset: u32) -> bool {
        let range_map = self.range_map.borrow();
        let Some(map) = range_map.get(&id) else {
            return false;
        };
        let mut distance = u32::MAX;
        let mut has_executed = false;
        for (range, executed) in map {
            if range.start <= offset && offset <= range.end && range.end - range.start < distance {
                has_executed = *executed;
                distance = range.end - range.start;
            }
        }
        has_executed
    }

    /// `insertUnexecutedRange(SourceID, unsigned, unsigned)`: só insere uma vez por faixa (`HashMap::add`).
    pub fn insert_unexecuted_range(&self, id: SourceID, start: u32, end: u32) {
        self.range_map.borrow_mut().entry(id).or_default().entry(FunctionRange { start, end }).or_insert(false);
    }

    /// `removeUnexecutedRange(SourceID, unsigned, unsigned)`: marca a faixa como executada (`HashMap::set`).
    pub fn remove_unexecuted_range(&self, id: SourceID, start: u32, end: u32) {
        // FIXME do C++: nunca deveria retornar aqui, mas hoje retorna em algumas situações.
        if let Some(map) = self.range_map.borrow_mut().get_mut(&id) {
            map.insert(FunctionRange { start, end }, true);
        }
    }

    /// `getFunctionRanges(SourceID)`: tuplas `(hasExecuted, start, end)`.
    pub fn get_function_ranges(&self, id: SourceID) -> Vec<(bool, u32, u32)> {
        let range_map = self.range_map.borrow();
        let mut ranges: Vec<(bool, u32, u32)> = match range_map.get(&id) {
            Some(map) => map.iter().map(|(range, executed)| (*executed, range.start, range.end)).collect(),
            None => Vec::new(),
        };
        ranges.sort_by_key(|&(_, start, end)| (start, end));
        ranges
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn smallest_enclosing_range_decides_and_insert_is_idempotent() {
        let cache = FunctionHasExecutedCache::default();
        assert!(!cache.has_executed_at_offset(1, 5));
        cache.insert_unexecuted_range(1, 0, 100);
        cache.insert_unexecuted_range(1, 10, 20);
        cache.remove_unexecuted_range(1, 10, 20);
        assert!(cache.has_executed_at_offset(1, 15));
        assert!(!cache.has_executed_at_offset(1, 50));
        cache.insert_unexecuted_range(1, 10, 20);
        assert!(cache.has_executed_at_offset(1, 15));
        assert_eq!(cache.get_function_ranges(1), vec![(false, 0, 100), (true, 10, 20)]);
    }
}
