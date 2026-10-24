//! Tradução de `runtime/PropertyNameArray.h`.
//!
//! DIVERGÊNCIAS:
//!
//! - `PropertyNameArray` é `RefCounted` no C++ só para o `releaseData()` entregar a lista ao
//!   `JSPropertyNameEnumerator` e ao cache de nomes; aqui a lista é do builder e `release_data`
//!   a move para fora.
//! - O `UniquedStringImpl*` não carrega a marca de nome privado (ver `Identifier::from_uid`), então
//!   `add_uid`/`add_unchecked_uid` recebem `is_private` de quem enumera (`PropertyTableEntry::isPrivate`)
//!   no lugar do `static_cast<SymbolImpl*>(identifier)->isPrivate()`.
//! - O limiar `setThreshold = 20` (busca linear antes, `HashSet` depois) é só desempenho: a unicidade é
//!   sempre conferida no `HashSet` de chaves.

use std::collections::HashSet;

use crate::runtime::enumeration_mode::{PrivateSymbolMode, PropertyNameMode};
use crate::runtime::identifier::Identifier;
use crate::runtime::vm::VM;
use crate::wtf::text::string_impl::UniquedKey;

/// `class PropertyNameArray`.
#[derive(Clone, Debug, Default)]
pub struct PropertyNameArray {
    property_name_vector: Vec<Identifier>,
}

impl PropertyNameArray {
    /// `propertyNameVector()`.
    pub fn property_name_vector(&self) -> &Vec<Identifier> {
        &self.property_name_vector
    }

    /// `propertyNameVector()` mutável.
    pub fn property_name_vector_mut(&mut self) -> &mut Vec<Identifier> {
        &mut self.property_name_vector
    }
}

/// `class PropertyNameArrayBuilder`.
pub struct PropertyNameArrayBuilder<'vm> {
    data: PropertyNameArray,
    set: HashSet<UniquedKey>,
    vm: &'vm VM,
    property_name_mode: PropertyNameMode,
    private_symbol_mode: PrivateSymbolMode,
}

impl<'vm> PropertyNameArrayBuilder<'vm> {
    /// `PropertyNameArrayBuilder(VM&, PropertyNameMode, PrivateSymbolMode)`.
    pub fn new(
        vm: &'vm VM,
        property_name_mode: PropertyNameMode,
        private_symbol_mode: PrivateSymbolMode,
    ) -> PropertyNameArrayBuilder<'vm> {
        PropertyNameArrayBuilder {
            data: PropertyNameArray::default(),
            set: HashSet::new(),
            vm,
            property_name_mode,
            private_symbol_mode,
        }
    }

    /// `vm()`.
    pub fn vm(&self) -> &'vm VM {
        self.vm
    }

    /// `add(uint32_t index)`: `add(Identifier::from(m_vm, index))`.
    pub fn add_index(&mut self, index: u32) {
        self.add(&Identifier::from_u32(self.vm, index));
    }

    /// `add(const Identifier&)`.
    pub fn add(&mut self, identifier: &Identifier) {
        let Some(key) = identifier.impl_() else { return };
        self.add_checked(key, identifier.is_private_name(), || identifier.clone());
    }

    /// `add(UniquedStringImpl*)`.
    pub fn add_uid(&mut self, key: &UniquedKey, is_private: bool) {
        let vm = self.vm;
        self.add_checked(key.clone(), is_private, || Identifier::from_uid(vm, Some(key)));
    }

    /// `addUnchecked(UniquedStringImpl*)`: sem conferir duplicata.
    pub fn add_unchecked_uid(&mut self, key: &UniquedKey, is_private: bool) {
        if !self.is_uid_matched_to_type_mode(key, is_private) {
            return;
        }
        self.set.insert(key.clone());
        self.data.property_name_vector.push(Identifier::from_uid(self.vm, Some(key)));
    }

    fn add_checked(&mut self, key: UniquedKey, is_private: bool, identifier: impl FnOnce() -> Identifier) {
        if !self.is_uid_matched_to_type_mode(&key, is_private) {
            return;
        }
        if !self.set.insert(key) {
            return;
        }
        self.data.property_name_vector.push(identifier());
    }

    /// `isUidMatchedToTypeMode(identifier)`.
    fn is_uid_matched_to_type_mode(&self, key: &UniquedKey, is_private: bool) -> bool {
        if key.0.is_symbol() {
            if !self.include_symbol_properties() {
                return false;
            }
            if self.private_symbol_mode == PrivateSymbolMode::Include {
                return true;
            }
            return !is_private;
        }
        self.include_string_properties()
    }

    /// `data()`.
    pub fn data(&self) -> &PropertyNameArray {
        &self.data
    }

    /// `releaseData()`.
    pub fn release_data(self) -> PropertyNameArray {
        self.data
    }

    /// `canAddKnownUniqueForStructure()`.
    pub fn can_add_known_unique_for_structure(&self) -> bool {
        self.data.property_name_vector.is_empty()
    }

    /// `size()`.
    pub fn len(&self) -> usize {
        self.data.property_name_vector.len()
    }

    /// `size() == 0`.
    pub fn is_empty(&self) -> bool {
        self.data.property_name_vector.is_empty()
    }

    /// `begin()`/`end()`.
    pub fn iter(&self) -> std::slice::Iter<'_, Identifier> {
        self.data.property_name_vector.iter()
    }

    /// `operator[](unsigned)`.
    pub fn get(&self, index: usize) -> &Identifier {
        &self.data.property_name_vector[index]
    }

    /// `includeSymbolProperties()`.
    pub fn include_symbol_properties(&self) -> bool {
        self.property_name_mode.includes_symbols()
    }

    /// `includeStringProperties()`.
    pub fn include_string_properties(&self) -> bool {
        self.property_name_mode.includes_strings()
    }

    /// `propertyNameMode()`.
    pub fn property_name_mode(&self) -> PropertyNameMode {
        self.property_name_mode
    }

    /// `privateSymbolMode()`.
    pub fn private_symbol_mode(&self) -> PrivateSymbolMode {
        self.private_symbol_mode
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn deduplicates_and_filters_by_mode() {
        let vm = VM::new();
        let mut names = PropertyNameArrayBuilder::new(&vm, PropertyNameMode::Strings, PrivateSymbolMode::Exclude);
        let a = Identifier::from_span(&vm, b"a");
        names.add(&a);
        names.add(&a);
        names.add_index(1);
        assert_eq!(names.len(), 2);
        assert_eq!(names.get(1), &Identifier::from_u32(&vm, 1));
    }
}
