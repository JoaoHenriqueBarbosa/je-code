//! Porte de `src/vs/platform/webContentExtractor/electron-main/webContentCache.ts`: o cache dos resultados
//! de extração de conteúdo web.
//!
//! Divergências: o `Date.now()` do original é `je_base::runtime::date_now()` (o relógio que os testes
//! controlam, como o `Date.now` trocado do teste original). O `extUriIgnorePathCase` é uma instância de
//! [`ExtUri`] sobre o ambiente do processo, guardada no cache. O campo `options` da entrada do cache,
//! que o original guarda e nunca lê, não é guardado. As constantes da classe estática são constantes do
//! módulo. `add`, `try_get` e `clear` recebem `&self`: o [`LRUCache`] já tem mutabilidade interior.

use std::rc::Rc;

use je_base::JsString;
use je_base::map::LRUCache;
use je_base::path::ProcessEnvironment;
use je_base::resources::ExtUri;
use je_base::runtime;
use je_base::uri::URI;

use super::web_content_extractor::{WebContentExtractResult, WebContentExtractorOptions};

const MAX_CACHE_SIZE: usize = 1000;
const SUCCESS_CACHE_DURATION: u64 = 1000 * 60 * 60 * 24; // 24 hours
const ERROR_CACHE_DURATION: u64 = 1000 * 60 * 5; // 5 minutes

#[derive(Clone)]
struct CacheEntry {
    result: WebContentExtractResult,
    expiration: u64,
}

/// A cache for web content extraction results.
pub struct WebContentCache {
    cache: LRUCache<JsString, CacheEntry>,
    ext_uri: ExtUri,
}

impl Default for WebContentCache {
    fn default() -> Self {
        Self::new()
    }
}

impl WebContentCache {
    pub fn new() -> Self {
        Self { cache: LRUCache::new(MAX_CACHE_SIZE, 1.0), ext_uri: ExtUri::ignore_path_case(Rc::new(ProcessEnvironment)) }
    }

    /// Add a web content extraction result to the cache.
    pub fn add(&self, uri: &URI, options: Option<&WebContentExtractorOptions>, result: WebContentExtractResult) {
        let duration = match result {
            WebContentExtractResult::Ok { .. } | WebContentExtractResult::Redirect { .. } => SUCCESS_CACHE_DURATION,
            WebContentExtractResult::Error { .. } => ERROR_CACHE_DURATION,
        };
        let expiration = runtime::date_now() + duration;

        let key = self.key(uri, options);
        self.cache.set(key, CacheEntry { result, expiration });
    }

    /// Try to get a cached web content extraction result for the given URI and options.
    pub fn try_get(&self, uri: &URI, options: Option<&WebContentExtractorOptions>) -> Option<WebContentExtractResult> {
        let key = self.key(uri, options);
        let entry = self.cache.get(&key)?;

        if entry.expiration < runtime::date_now() {
            self.cache.delete(&key);
            return None;
        }

        Some(entry.result)
    }

    /// Clear all cached entries.
    pub fn clear(&self) {
        self.cache.clear();
    }

    fn key(&self, uri: &URI, options: Option<&WebContentExtractorOptions>) -> JsString {
        let follow_redirects = options.and_then(|o| o.follow_redirects).unwrap_or(false);
        let mut key = JsString::from(if follow_redirects { "true" } else { "false" });
        key.push_units(self.ext_uri.get_comparison_key(uri, false).units());
        key
    }
}
