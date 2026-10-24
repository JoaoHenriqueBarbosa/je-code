//! Tradução de `runtime/ScriptFetcher.h` (com `USE(BUN_JSC_ADDITIONS)` ligado pelo `cmakeconfig.h`).

/// `ScriptFetcher::Type`.
#[repr(u8)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ScriptFetcherType {
    Cached = 0,
    Worker = 1,
    NodeVM = 2,
}

/// `class ScriptFetcher`. O `RefCounted` vira `Rc<dyn ScriptFetcher>` em quem guarda.
pub trait ScriptFetcher {
    fn is_cached_script_fetcher(&self) -> bool {
        false
    }

    fn is_worker_script_fetcher(&self) -> bool {
        false
    }

    /// `fetcherType()`, virtual puro.
    fn fetcher_type(&self) -> ScriptFetcherType;
}
