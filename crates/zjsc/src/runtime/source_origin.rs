//! Tradução de `runtime/SourceOrigin.h`.

use std::rc::Rc;

use crate::runtime::script_fetcher::ScriptFetcher;
use crate::wtf::text::wtf_string::String as WtfString;
use crate::wtf::url::URL;

/// `class SourceOrigin`. O `RefPtr<ScriptFetcher>` vira `Option<Rc<dyn ScriptFetcher>>`.
#[derive(Clone, Default)]
pub struct SourceOrigin {
    url: URL,
    fetcher: Option<Rc<dyn ScriptFetcher>>,
}

impl SourceOrigin {
    /// `explicit SourceOrigin(const URL&)`.
    pub fn new(url: URL) -> SourceOrigin {
        SourceOrigin { url, fetcher: None }
    }

    /// `explicit SourceOrigin(const URL&, Ref<ScriptFetcher>&&)`.
    pub fn with_fetcher(url: URL, fetcher: Rc<dyn ScriptFetcher>) -> SourceOrigin {
        SourceOrigin { url, fetcher: Some(fetcher) }
    }

    /// `url()`.
    pub fn url(&self) -> &URL {
        &self.url
    }

    /// `string()`.
    pub fn string(&self) -> &WtfString {
        self.url.string()
    }

    /// `isNull()`.
    pub fn is_null(&self) -> bool {
        self.url.is_null()
    }

    /// `fetcher()`.
    pub fn fetcher(&self) -> Option<&Rc<dyn ScriptFetcher>> {
        self.fetcher.as_ref()
    }
}

impl PartialEq for SourceOrigin {
    /// `operator==` padrão: a URL por valor e o `RefPtr` por ponteiro.
    fn eq(&self, other: &SourceOrigin) -> bool {
        if self.url != other.url {
            return false;
        }
        match (&self.fetcher, &other.fetcher) {
            (None, None) => true,
            (Some(a), Some(b)) => std::ptr::eq(Rc::as_ptr(a) as *const (), Rc::as_ptr(b) as *const ()),
            _ => false,
        }
    }
}

impl Eq for SourceOrigin {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn null_and_equality() {
        assert!(SourceOrigin::default().is_null());
        let a = SourceOrigin::new(URL::from_string(&WtfString::from_latin1(b"file:///a.js")));
        let b = SourceOrigin::new(URL::from_string(&WtfString::from_latin1(b"file:///a.js")));
        assert!(!a.is_null());
        assert!(a == b);
        assert!(a != SourceOrigin::default());
    }
}

impl std::fmt::Debug for SourceOrigin {
    /// O `ScriptFetcher` é opaco; só a URL e a presença dele são observáveis.
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SourceOrigin").field("url", &self.url).field("has_fetcher", &self.fetcher.is_some()).finish()
    }
}
