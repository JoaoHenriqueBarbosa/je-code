//! Tradução de `parser/UnlinkedSourceCode.{h,cpp}`.
//!
//! `UnlinkedSourceCode(WTF::HashTableDeletedValueType)` e `isHashTableDeletedValue()` não existem: o
//! `HashMap` do Rust não usa o marcador de valor deletado do WTF. O `friend` dos `Cached*` some, os
//! campos são `pub(crate)`.

use std::rc::Rc;

use crate::parser::source_provider::SourceProvider;
use crate::wtf::text::conversion_mode::ConversionMode;
use crate::wtf::text::wtf_string::String as WtfString;

/// `class UnlinkedSourceCode`. O `RefPtr<SourceProvider>` vira `Option<Rc<dyn SourceProvider>>`.
#[derive(Clone, Default)]
pub struct UnlinkedSourceCode {
    pub(crate) provider: Option<Rc<dyn SourceProvider>>,
    pub(crate) start_offset: i32,
    pub(crate) end_offset: i32,
}

impl UnlinkedSourceCode {
    /// `UnlinkedSourceCode(Ref<SourceProvider>&&)`.
    pub fn from_provider(provider: Rc<dyn SourceProvider>) -> UnlinkedSourceCode {
        let end_offset = provider.source().length() as i32;
        UnlinkedSourceCode { provider: Some(provider), start_offset: 0, end_offset }
    }

    /// `UnlinkedSourceCode(Ref<SourceProvider>&&, int startOffset, int endOffset)` e a variante com
    /// `RefPtr`.
    pub fn with_offsets(provider: Option<Rc<dyn SourceProvider>>, start_offset: i32, end_offset: i32) -> UnlinkedSourceCode {
        UnlinkedSourceCode { provider, start_offset, end_offset }
    }

    /// `provider()`: o C++ desreferencia o ponteiro (nulo é erro de quem chama).
    pub fn provider(&self) -> &Rc<dyn SourceProvider> {
        self.provider.as_ref().expect("UnlinkedSourceCode::provider() com provedor nulo")
    }

    /// `hash()`.
    pub fn hash(&self) -> u32 {
        debug_assert!(self.provider.is_some());
        self.provider().hash()
    }

    /// `view()`: o `StringView` vira `String`; sem provedor, a `StringView` nula vira a `String` nula.
    pub fn view(&self) -> WtfString {
        match &self.provider {
            None => WtfString::default(),
            Some(provider) => provider.get_range(self.start_offset, self.end_offset),
        }
    }

    /// `toUTF8()`: o `CString` vira bytes.
    pub fn to_utf8(&self) -> Vec<u8> {
        match &self.provider {
            None => Vec::new(),
            Some(provider) => provider
                .source()
                .substring(self.start_offset as u32, self.end_offset.wrapping_sub(self.start_offset) as u32)
                .utf8(ConversionMode::LenientConversion),
        }
    }

    /// `isNull()`.
    pub fn is_null(&self) -> bool {
        self.provider.is_none()
    }

    /// `startOffset()`.
    pub fn start_offset(&self) -> i32 {
        self.start_offset
    }

    /// `endOffset()`.
    pub fn end_offset(&self) -> i32 {
        self.end_offset
    }

    /// `length()`.
    pub fn length(&self) -> i32 {
        self.end_offset - self.start_offset
    }
}

/// Igualdade do `RefPtr<SourceProvider>`: por ponteiro (só a parte de dados do ponteiro gordo).
pub fn same_provider(a: &Option<Rc<dyn SourceProvider>>, b: &Option<Rc<dyn SourceProvider>>) -> bool {
    match (a, b) {
        (None, None) => true,
        (Some(a), Some(b)) => std::ptr::eq(Rc::as_ptr(a) as *const (), Rc::as_ptr(b) as *const ()),
        _ => false,
    }
}

impl PartialEq for UnlinkedSourceCode {
    /// `operator==` padrão: provedor por ponteiro e os dois offsets.
    fn eq(&self, other: &UnlinkedSourceCode) -> bool {
        same_provider(&self.provider, &other.provider)
            && self.start_offset == other.start_offset
            && self.end_offset == other.end_offset
    }
}

impl Eq for UnlinkedSourceCode {}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parser::source_provider::{SourceProviderSourceType, StringSourceProvider};
    use crate::parser::source_tainted_origin::SourceTaintedOrigin;
    use crate::runtime::source_origin::SourceOrigin;
    use crate::wtf::text::text_position::TextPosition;

    fn provider(source: &str) -> Rc<dyn SourceProvider> {
        StringSourceProvider::create(
            &WtfString::from_latin1(source.as_bytes()),
            &SourceOrigin::default(),
            WtfString::default(),
            SourceTaintedOrigin::Untainted,
            TextPosition::default(),
            SourceProviderSourceType::Program,
        )
    }

    #[test]
    fn view_and_equality() {
        let p = provider("hello world");
        let whole = UnlinkedSourceCode::from_provider(p.clone());
        assert_eq!(whole.length(), 11);
        let part = UnlinkedSourceCode::with_offsets(Some(p.clone()), 6, 11);
        assert_eq!(part.view(), WtfString::from_latin1(b"world"));
        assert_eq!(part.to_utf8(), b"world".to_vec());
        assert!(part == UnlinkedSourceCode::with_offsets(Some(p.clone()), 6, 11));
        assert!(part != whole);
        assert!(UnlinkedSourceCode::default().is_null());
        assert!(UnlinkedSourceCode::default().view().is_null());
    }
}
