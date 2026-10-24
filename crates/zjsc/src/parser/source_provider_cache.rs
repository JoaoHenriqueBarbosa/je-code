//! Tradução de `parser/SourceProviderCache.h` e `SourceProviderCache.cpp`.
//!
//! O `m_map` é `UncheckedKeyHashMap<int, std::unique_ptr<Item>, IntHash<int>, UnsignedWithZeroKeyHashTraits<int>>`;
//! a ordem de iteração do mapa não é observável (só `get`, `add` e `clear`), então um `HashMap<i32, _>`
//! equivale, e `UnsignedWithZeroKeyHashTraits` (aceitar a chave 0) é natural nele.

use std::collections::HashMap;
use std::rc::Rc;

use crate::parser::source_provider_cache_item::SourceProviderCacheItem;

/// `class SourceProviderCache`.
#[derive(Debug)]
pub struct SourceProviderCache {
    map: HashMap<i32, Rc<SourceProviderCacheItem>>,
}

impl SourceProviderCache {
    /// `SourceProviderCache::create(unsigned sourceLength)` junto com o construtor privado.
    pub fn create(source_length: u32) -> SourceProviderCache {
        const CONSERVATIVE_SOURCE_BYTES_PER_ENTRY: u32 = 512;
        const MAXIMUM_ENTRIES_TO_RESERVE: u32 = 64 * 1024;

        let estimated_entries = source_length / CONSERVATIVE_SOURCE_BYTES_PER_ENTRY;
        SourceProviderCache {
            map: HashMap::with_capacity(estimated_entries.min(MAXIMUM_ENTRIES_TO_RESERVE) as usize),
        }
    }

    pub fn clear(&mut self) {
        self.map.clear();
    }

    /// `m_map.add(...)`: o `add` do WTF não sobrescreve chave existente.
    pub fn add(&mut self, source_position: i32, item: Rc<SourceProviderCacheItem>) {
        self.map.entry(source_position).or_insert(item);
    }

    pub fn get(&self, source_position: i32) -> Option<Rc<SourceProviderCacheItem>> {
        self.map.get(&source_position).cloned()
    }
}
