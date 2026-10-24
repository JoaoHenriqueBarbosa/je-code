//! Porte de `bytecode/DirectEvalCodeCache.h`, `DirectEvalCodeCache.cpp` e `DirectEvalCodeCacheInlines.h`:
//! o cache, por `CodeBlock` chamador, dos `DirectEvalExecutable` já compilados, indexado pelo texto do
//! programa e pelo `BytecodeIndex` do `op_call_direct_eval`.
//!
//! O `CacheLookupKey` do C++ existe para buscar sem tomar referência ao `StringImpl`; aqui a busca
//! empresta a `String` e a chave guardada é dona dela. O `Lock` (o compilador concorrente lê o mapa)
//! e o `visitAggregate` do GC não existem neste porte.

use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

use crate::bytecode::bytecode_index::BytecodeIndex;
use crate::runtime::eval_executable::EvalExecutable;
use crate::wtf::text::wtf_string::String as WtfString;

/// O `DirectEvalExecutable*` guardado.
pub type DirectEvalExecutableRef = Rc<RefCell<EvalExecutable>>;

/// `DirectEvalCodeCache::CacheKey`: o texto do programa e o `BytecodeIndex` da chamada.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
struct CacheKey {
    source: WtfString,
    bytecode_index_bits: u32,
}

/// `class DirectEvalCodeCache`.
#[derive(Default)]
pub struct DirectEvalCodeCache {
    /// `m_cacheMap`.
    cache_map: HashMap<CacheKey, DirectEvalExecutableRef>,
}

impl std::fmt::Debug for DirectEvalCodeCache {
    fn fmt(&self, out: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        out.debug_struct("DirectEvalCodeCache").field("entries", &self.cache_map.len()).finish()
    }
}

impl DirectEvalCodeCache {
    /// `maxCacheEntries`.
    const MAX_CACHE_ENTRIES: usize = 64;

    /// `get(const CacheLookupKey&)`.
    pub fn get(&self, source: &WtfString, bytecode_index: BytecodeIndex) -> Option<DirectEvalExecutableRef> {
        let key = CacheKey { source: source.clone(), bytecode_index_bits: bytecode_index.as_bits() };
        self.cache_map.get(&key).cloned()
    }

    /// `set(globalObject, owner, cacheKey, evalExecutable)` com o `setSlow`: só guarda enquanto há
    /// espaço e quando o executável permite (`allowDirectEvalCache`).
    pub fn set(&mut self, source: &WtfString, bytecode_index: BytecodeIndex, eval_executable: &DirectEvalExecutableRef) {
        if self.cache_map.len() >= Self::MAX_CACHE_ENTRIES {
            return;
        }
        if !eval_executable.borrow().allow_direct_eval_cache() {
            return;
        }
        let key = CacheKey { source: source.clone(), bytecode_index_bits: bytecode_index.as_bits() };
        self.cache_map.insert(key, Rc::clone(eval_executable));
    }

    /// `isEmpty()`.
    pub fn is_empty(&self) -> bool {
        self.cache_map.is_empty()
    }

    /// `clear()`.
    pub fn clear(&mut self) {
        self.cache_map.clear();
    }
}
