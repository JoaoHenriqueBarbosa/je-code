//! Porte de `WTF/wtf/text/SymbolRegistry.h` e `SymbolRegistry.cpp`.
//!
//! Mora em `runtime/` porque só o `VM` a possui (`m_symbolRegistry`, `m_privateSymbolRegistry`).

use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

use crate::wtf::text::string_impl::StringImpl;
use crate::wtf::text::symbol_impl::{RegisteredSymbolImpl, SymbolRegistryId};

/// `SymbolRegistry::Type`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SymbolRegistryType {
    PublicSymbol,
    PrivateSymbol,
}

/// Conteúdo do `StringImpl` como chave: 8 e 16 bits com o mesmo texto são a mesma chave, como na
/// igualdade de `StringImpl` do C++.
pub(crate) fn content_key(rep: &StringImpl) -> Vec<u16> {
    if rep.is_8bit() {
        rep.span8().iter().map(|&unit| u16::from(unit)).collect()
    } else {
        rep.span16().to_vec()
    }
}

/// `class SymbolRegistry`. O `m_table` do C++ guarda o próprio símbolo registrado (que é um
/// `StringImpl`); aqui o mapa leva o conteúdo ao `RegisteredSymbolImpl`.
#[derive(Debug)]
pub struct SymbolRegistry {
    table: RefCell<HashMap<Vec<u16>, Rc<RegisteredSymbolImpl>>>,
    symbol_type: SymbolRegistryType,
}

impl SymbolRegistry {
    /// `SymbolRegistry(Type)`.
    pub fn new(symbol_type: SymbolRegistryType) -> SymbolRegistry {
        SymbolRegistry { table: RefCell::new(HashMap::new()), symbol_type }
    }

    /// O `CheckedPtr<SymbolRegistry>` do C++: a identidade é o endereço, estável enquanto o
    /// registro não for movido (o `VM` o guarda em `Box`).
    pub fn id(&self) -> SymbolRegistryId {
        SymbolRegistryId(self as *const SymbolRegistry as usize)
    }

    /// `symbolForKey(const String&)`.
    pub fn symbol_for_key(&self, rep: &Rc<StringImpl>) -> Rc<RegisteredSymbolImpl> {
        let key = content_key(rep);
        if let Some(existing) = self.table.borrow().get(&key) {
            return Rc::clone(existing);
        }
        let symbol = match self.symbol_type {
            SymbolRegistryType::PrivateSymbol => RegisteredSymbolImpl::create_private(rep, self.id()),
            SymbolRegistryType::PublicSymbol => RegisteredSymbolImpl::create(rep, self.id()),
        };
        self.table.borrow_mut().insert(key, Rc::clone(&symbol));
        symbol
    }

    /// `remove(RegisteredSymbolImpl&)`.
    pub fn remove(&self, uid: &RegisteredSymbolImpl) {
        debug_assert!(uid.symbol_registry() == Some(self.id()));
        let key = content_key(uid.string_impl());
        let removed = self.table.borrow_mut().remove(&key);
        debug_assert!(removed.is_some(), "The string being removed is registered in the string table of another thread!");
    }
}

impl Drop for SymbolRegistry {
    /// `~SymbolRegistry()`.
    fn drop(&mut self) {
        for symbol in self.table.get_mut().values() {
            symbol.clear_symbol_registry();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn same_key_gives_same_symbol() {
        let registry = SymbolRegistry::new(SymbolRegistryType::PrivateSymbol);
        let a = registry.symbol_for_key(&StringImpl::create(b"@x1"));
        let b = registry.symbol_for_key(&StringImpl::create(b"@x1"));
        let c = registry.symbol_for_key(&StringImpl::create(b"@x2"));
        assert!(Rc::ptr_eq(&a, &b));
        assert!(!Rc::ptr_eq(&a, &c));
        assert!(a.is_private() && a.is_registered());
    }
}
