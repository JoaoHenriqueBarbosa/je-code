//! Tradução de `JavaScriptCore/runtime/PrivateName.h`.

use std::rc::Rc;

use crate::wtf::text::string_impl::StringImpl;
use crate::wtf::text::symbol_impl::{PrivateSymbolImpl, RegisteredSymbolImpl, SymbolImpl};

/// O `const Ref<SymbolImpl> m_uid` do C++. O `PrivateSymbolImpl` é um `SymbolImpl` no C++ (herança);
/// no porte é um embrulho com `Deref`, então a posse guarda qual dos dois é.
#[derive(Clone, Debug)]
enum Uid {
    Symbol(Rc<SymbolImpl>),
    Private(Rc<PrivateSymbolImpl>),
    /// O `RegisteredSymbolImpl` de `Symbol.for` (também um `SymbolImpl` no C++).
    Registered(Rc<RegisteredSymbolImpl>),
}

/// `class PrivateName`.
#[derive(Clone, Debug)]
pub struct PrivateName {
    m_uid: Uid,
}

impl PrivateName {
    /// `explicit PrivateName(SymbolImpl& uid)`.
    pub fn new(uid: Rc<SymbolImpl>) -> PrivateName {
        PrivateName { m_uid: Uid::Symbol(uid) }
    }

    /// `PrivateName(DescriptionTag, const String& description)`: `SymbolImpl::create(*description.impl())`.
    pub fn with_description(description: &Rc<StringImpl>) -> PrivateName {
        PrivateName::new(SymbolImpl::create(description))
    }

    /// `PrivateName(PrivateSymbolTag, const String& description)`: `PrivateSymbolImpl::create(*description.impl())`.
    pub fn with_private_symbol(description: &Rc<StringImpl>) -> PrivateName {
        PrivateName { m_uid: Uid::Private(PrivateSymbolImpl::create(description)) }
    }

    /// `PrivateName(SymbolImpl& uid)` para o `RegisteredSymbolImpl` que `SymbolRegistry::symbolForKey` devolve.
    pub fn with_registered_symbol(uid: Rc<RegisteredSymbolImpl>) -> PrivateName {
        PrivateName { m_uid: Uid::Registered(uid) }
    }

    /// `uid()`.
    pub fn uid(&self) -> &SymbolImpl {
        match &self.m_uid {
            Uid::Symbol(symbol) => symbol,
            Uid::Private(symbol) => symbol,
            Uid::Registered(symbol) => symbol,
        }
    }
}

/// `operator==`: `&uid() == &other.uid()`. A identidade do símbolo é a do `Rc<StringImpl>` dele.
impl PartialEq for PrivateName {
    fn eq(&self, other: &Self) -> bool {
        Rc::ptr_eq(self.uid().string_impl(), other.uid().string_impl())
    }
}

impl Eq for PrivateName {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn identity_is_by_symbol() {
        let description = StringImpl::create(b"x");
        let a = PrivateName::with_description(&description);
        let b = PrivateName::with_description(&description);
        assert_eq!(a, a.clone());
        assert_ne!(a, b);
        let p = PrivateName::with_private_symbol(&description);
        assert!(p.uid().is_private());
        assert!(!a.uid().is_private());
    }
}
