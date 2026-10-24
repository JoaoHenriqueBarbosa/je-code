//! Tradução parcial de `runtime/PropertyName.h`: `class PropertyName`.
//!
//! O C++ guarda um `UniquedStringImpl*`; aqui é o `UniquedKey` (identidade do `StringImpl` internado)
//! mais a flag de nome privado, que no C++ vem de `SymbolImpl::isPrivate()` mas que o `StringImpl` do
//! porte não carrega (mesma razão do `m_private` de `Identifier`).
//!
//! Fora desta fatia: o construtor a partir de `CacheableIdentifier`, `dump` e as
//! `fastIsCanonicalNumericIndexString`/`isCanonicalNumericIndexString` (só os typed arrays usam).

use crate::runtime::identifier::{parse_index_impl, Identifier};
use crate::runtime::private_name::PrivateName;
use crate::wtf::text::string_impl::UniquedKey;

/// `class PropertyName`.
#[derive(Clone, Debug, Default)]
pub struct PropertyName {
    uid: Option<UniquedKey>,
    is_private: bool,
}

impl PropertyName {
    /// `PropertyName()`.
    pub fn null() -> PropertyName {
        PropertyName { uid: None, is_private: false }
    }

    /// `PropertyName(const Identifier&)`.
    pub fn from_identifier(identifier: &Identifier) -> PropertyName {
        PropertyName { uid: identifier.impl_(), is_private: identifier.is_private_name() }
    }

    /// `PropertyName(UniquedStringImpl*)`: o chamador diz se o símbolo é privado.
    pub fn from_uid(uid: Option<UniquedKey>, is_private: bool) -> PropertyName {
        PropertyName { uid, is_private }
    }

    /// `PropertyName(const PrivateName&)`.
    pub fn from_private_name(private_name: &PrivateName) -> PropertyName {
        PropertyName::from_identifier(&Identifier::from_private_name(private_name))
    }

    /// `isNull()`.
    pub fn is_null(&self) -> bool {
        self.uid.is_none()
    }

    /// `isSymbol()`.
    pub fn is_symbol(&self) -> bool {
        self.uid.as_ref().is_some_and(|uid| uid.0.is_symbol())
    }

    /// `isPrivateName()`.
    pub fn is_private_name(&self) -> bool {
        self.is_symbol() && self.is_private
    }

    /// `uid()`.
    pub fn uid(&self) -> Option<&UniquedKey> {
        self.uid.as_ref()
    }

    /// `publicName()`: o átomo, ou `None` para símbolo e nulo.
    pub fn public_name(&self) -> Option<&UniquedKey> {
        match &self.uid {
            Some(uid) if !uid.0.is_symbol() => Some(uid),
            _ => None,
        }
    }

    /// `parseIndex(PropertyName)`.
    pub fn parse_index(&self) -> Option<u32> {
        let uid = self.uid.as_ref()?;
        if uid.0.is_symbol() {
            return None;
        }
        parse_index_impl(&uid.0)
    }
}

/// `operator==(PropertyName, PropertyName)`: identidade do `uid`.
impl PartialEq for PropertyName {
    fn eq(&self, other: &PropertyName) -> bool {
        self.uid == other.uid
    }
}

/// `operator==(PropertyName, const Identifier&)`.
impl PartialEq<Identifier> for PropertyName {
    fn eq(&self, other: &Identifier) -> bool {
        self.uid == other.impl_()
    }
}
