//! Tradução de `JavaScriptCore/builtins/BuiltinNames.h` e `BuiltinNames.cpp`.
//!
//! A parte que o C++ gera expandindo macros (campos, construtor, acessores `xxxPublicName()`,
//! `xxxPrivateName()`, `xxxSymbol()`) está em `builtin_names/generated.rs`, saída de
//! `scripts/gen-common-identifiers.py`. Aqui fica o que é escrito à mão: os dois mapas de busca,
//! `lookUpPrivateName`, `lookUpWellKnownSymbol` e `appendExternalName`.
//!
//! Os `ASSERT` de `checkPublicToPrivateMapConsistency` e de `appendExternalName` só existem com
//! `ASSERT_ENABLED` e não são portados (ver CONVENTIONS.md).
//!
//! Diferença de tipo em relação ao C++: o `MemoryCompactLookupOnlyRobinHoodHashSet<String>` guarda
//! `String`s cujo `impl()` é o `SymbolImpl`. O `Identifier` do porte carrega o `StringImpl` do símbolo
//! e a marca de privado, que é o que o `SymbolImpl*` do C++ leva adiante; por isso os mapas guardam o
//! `Identifier` e as consultas o devolvem no lugar do `PrivateSymbolImpl*` e do `SymbolImpl*`. A chave
//! é o conteúdo da string como unidades UTF-16: o `WTF::equal` do C++ compara conteúdo sem olhar a
//! largura (Latin-1 contra UTF-16), e a chave reproduz isso.

use std::collections::HashMap;

use crate::runtime::identifier::Identifier;
use crate::runtime::symbol_registry::content_key;
use crate::wtf::text::string_impl::CharType;

mod generated;

pub use generated::BuiltinNames;

/// `PrivateNameSet`: as strings dos private names, por conteúdo.
pub type PrivateNameSet = HashMap<Vec<u16>, Identifier>;

/// `WellKnownSymbolMap`: do nome público do símbolo conhecido (`iterator`) ao símbolo.
pub type WellKnownSymbolMap = HashMap<Vec<u16>, Identifier>;

/// A chave dos dois mapas para um span: o conteúdo em unidades UTF-16, como `content_key` faz com
/// um `StringImpl`.
fn span_key<T: CharType>(characters: &[T]) -> Vec<u16> {
    characters.iter().map(|&c| c.to_u16()).collect()
}

fn identifier_key(identifier: &Identifier) -> Option<Vec<u16>> {
    identifier.impl_().map(|uid| content_key(&uid.0))
}

impl BuiltinNames {
    /// `m_privateNameSet.add(symbol)` do construtor e de `appendExternalName`.
    pub(super) fn insert_private_name(&mut self, private_name: &Identifier) {
        if let Some(key) = identifier_key(private_name) {
            self.m_private_name_set.insert(key, private_name.clone());
        }
    }

    /// `m_wellKnownSymbolsMap.add(m_xxxSymbolPrivateIdentifier.impl(), symbol)`: `key` é o
    /// `m_xxxSymbolPrivateIdentifier` e `symbol` o `m_xxxSymbol`.
    pub(super) fn add_well_known_symbol(&mut self, key: &Identifier, symbol: &Identifier) {
        if let Some(key) = identifier_key(key) {
            self.m_well_known_symbols_map.insert(key, symbol.clone());
        }
    }

    /// `lookUpPrivateName(const Identifier&)`: o `impl()` nulo não casa com nada.
    pub fn look_up_private_name_identifier(&self, identifier: &Identifier) -> Option<Identifier> {
        self.m_private_name_set.get(&identifier_key(identifier)?).cloned()
    }

    /// `lookUpPrivateName(std::span<const Latin1Character|char16_t>)` e `lookUpPrivateName(const
    /// String&)` (o chamador passa o span da string).
    pub fn look_up_private_name<T: CharType>(&self, characters: &[T]) -> Option<Identifier> {
        self.m_private_name_set.get(&span_key(characters)).cloned()
    }

    /// `lookUpWellKnownSymbol(const Identifier&)`.
    pub fn look_up_well_known_symbol_identifier(&self, identifier: &Identifier) -> Option<Identifier> {
        self.m_well_known_symbols_map.get(&identifier_key(identifier)?).cloned()
    }

    /// `lookUpWellKnownSymbol(std::span<const Latin1Character|char16_t>)` e
    /// `lookUpWellKnownSymbol(const String&)`.
    pub fn look_up_well_known_symbol<T: CharType>(&self, characters: &[T]) -> Option<Identifier> {
        self.m_well_known_symbols_map.get(&span_key(characters)).cloned()
    }

    /// `appendExternalName(publicName, privateName)`: o `publicName` só entra no `ASSERT`.
    pub fn append_external_name(&mut self, _public_name: &Identifier, private_name: &Identifier) {
        self.insert_private_name(private_name);
    }
}
