//! Tradução de `runtime/SparseArrayValueMap.h` e `.cpp`: o mapa de índices esparsos (ou com atributos)
//! do `ArrayStorage`, e `PutDirectIndexMode` (`JSObject.h`).
//!
//! DIVERGÊNCIAS (sem heap e sem GC, camada 3):
//!
//! - O C++ é um `JSCell` (`StructureIsImmortal`) com um `HashSet<SparseArrayEntry>`; aqui é um valor
//!   guardado dentro do `ArrayStorage` (`Option<SparseArrayValueMap>`), com um `BTreeMap` por índice. A
//!   ordem de iteração do `HashSet` não é observável (toda enumeração ordena os índices), então a
//!   ordenada do `BTreeMap` é equivalente. `m_reportedCapacity`, `getConcurrently`, `visitChildren`,
//!   `create`/`destroy` e o `cellLock` são do coletor e do compilador concorrente e não existem.
//! - As operações que dependem do objeto dono (`putEntry`, `putDirect`, e `SparseArrayEntry::put`, que
//!   chama o setter) ficam em `js_object_array_storage.rs`: o mapa mora dentro do butterfly do dono,
//!   que o `RefCell` não deixa emprestado durante a chamada de usuário. `SparseArrayEntry` é `Copy`
//!   por isso: quem chama copia a entrada, solta o empréstimo e só então age.

use std::collections::BTreeMap;

use crate::runtime::js_getter_setter::GetterSetter;
use crate::runtime::js_object::JSObject;
use crate::runtime::js_value::JSValue;
use crate::runtime::property_attribute::ACCESSOR;
use crate::runtime::property_descriptor::PropertyDescriptor;
use crate::runtime::property_slot::PropertySlot;

/// `enum PutDirectIndexMode`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PutDirectIndexMode {
    PutDirectIndexLikePutDirect,
    PutDirectIndexShouldThrow,
    PutDirectIndexShouldNotThrow,
}

/// `class SparseArrayEntry`.
#[derive(Clone, Copy, Debug)]
pub struct SparseArrayEntry {
    /// `m_index`.
    index: u32,
    /// `m_attributes`.
    attributes: u32,
    /// `m_value` (nasce `undefined`, como `UndefinedWriteBarrierTag`).
    value: JSValue,
}

impl SparseArrayEntry {
    /// `SparseArrayEntry(unsigned index)`.
    fn new(index: u32) -> SparseArrayEntry {
        SparseArrayEntry { index, attributes: 0, value: JSValue::undefined() }
    }

    /// `index()`.
    pub fn index(&self) -> u32 {
        self.index
    }

    /// `attributes()`.
    pub fn attributes(&self) -> u32 {
        self.attributes
    }

    /// `get()`.
    pub fn value(&self) -> JSValue {
        self.value
    }

    /// `getNonSparseMode()`.
    pub fn get_non_sparse_mode(&self) -> JSValue {
        debug_assert!(self.attributes == 0);
        self.value
    }

    /// `get(JSObject*, PropertySlot&)`.
    pub fn get_slot(&self, this_object: &JSObject, slot: &mut PropertySlot) {
        let value = self.value;
        debug_assert!(!value.is_empty());

        match GetterSetter::from_value(&value) {
            None => slot.set_value(this_object, self.attributes, value),
            Some(getter_setter) => slot.set_getter_slot(this_object, self.attributes, getter_setter),
        }
    }

    /// `get(PropertyDescriptor&)`.
    pub fn get_descriptor(&self, descriptor: &mut PropertyDescriptor) {
        descriptor.set_descriptor(self.value, self.attributes);
    }
}

/// `class SparseArrayValueMap`.
#[derive(Debug, Default)]
pub struct SparseArrayValueMap {
    /// `m_set`.
    entries: BTreeMap<u32, SparseArrayEntry>,
    /// `Flags::SparseMode`.
    sparse_mode: bool,
    /// `Flags::LengthIsReadOnly`.
    length_is_read_only: bool,
    /// `Flags::HasAnyKindOfGetterSetterProperties`.
    has_any_kind_of_getter_setter_properties: bool,
}

impl SparseArrayValueMap {
    /// `SparseArrayValueMap::visitChildren`: os valores (e os `GetterSetter` dos acessores).
    pub(crate) fn visit_children(&self, marker: &mut crate::runtime::gc::Marker) {
        for entry in self.entries.values() {
            marker.value(entry.value);
        }
    }

    /// `sparseMode()`.
    pub fn sparse_mode(&self) -> bool {
        self.sparse_mode
    }

    /// `setSparseMode()`.
    pub fn set_sparse_mode(&mut self) {
        self.sparse_mode = true;
    }

    /// `lengthIsReadOnly()`.
    pub fn length_is_read_only(&self) -> bool {
        self.length_is_read_only
    }

    /// `setLengthIsReadOnly()`.
    pub fn set_length_is_read_only(&mut self) {
        self.length_is_read_only = true;
    }

    /// `hasAnyKindOfGetterSetterProperties()`.
    pub fn has_any_kind_of_getter_setter_properties(&self) -> bool {
        self.has_any_kind_of_getter_setter_properties
    }

    /// `setHasAnyKindOfGetterSetterProperties()`.
    pub fn set_has_any_kind_of_getter_setter_properties(&mut self) {
        self.has_any_kind_of_getter_setter_properties = true;
    }

    /// `add(array, i)`: `true` quando a entrada é nova (`AddResult::isNewEntry`); a entrada nova nasce
    /// com `undefined` e sem atributos.
    pub fn add(&mut self, i: u32) -> bool {
        match self.entries.entry(i) {
            std::collections::btree_map::Entry::Occupied(_) => false,
            std::collections::btree_map::Entry::Vacant(vacant) => {
                vacant.insert(SparseArrayEntry::new(i));
                true
            }
        }
    }

    /// `find(i)`: a cópia da entrada, ou `None` no `notFound()`.
    pub fn find(&self, i: u32) -> Option<SparseArrayEntry> {
        self.entries.get(&i).copied()
    }

    /// A menor chave `>= i` do mapa (`lower_bound(i)`), ou `None` quando não há.
    pub fn first_index_at_or_after(&self, i: u32) -> Option<u32> {
        self.entries.range(i..).next().map(|(&key, _)| key)
    }

    /// `contains(i)`.
    pub fn contains(&self, i: u32) -> bool {
        self.entries.contains_key(&i)
    }

    /// `remove(i)`.
    pub fn remove(&mut self, i: u32) {
        self.entries.remove(&i);
    }

    /// `size()`.
    pub fn size(&self) -> usize {
        self.entries.len()
    }

    /// `isEmpty()`.
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// `begin()`..`end()`, por índice crescente.
    pub fn iter(&self) -> impl Iterator<Item = &SparseArrayEntry> {
        self.entries.values()
    }

    /// `SparseArrayEntry::forceSet(vm, map, value, attributes)` (com `value`) e
    /// `forceSet(map, attributes)` (sem): a entrada de `i` tem de existir.
    pub fn force_set(&mut self, i: u32, value: Option<JSValue>, attributes: u32) {
        let entry = self.entries.get_mut(&i).expect("forceSet em entrada inexistente");
        if let Some(value) = value {
            entry.value = value;
        }
        if attributes & ACCESSOR != 0 {
            self.has_any_kind_of_getter_setter_properties = true;
        }
        entry.attributes = attributes;
    }

    /// `m_value.set(vm, map, value)` de `SparseArrayEntry::put`, sem mexer nos atributos.
    pub fn set_value(&mut self, i: u32, value: JSValue) {
        self.entries.get_mut(&i).expect("put em entrada inexistente").value = value;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::runtime::js_value::js_number_i32;
    use crate::runtime::property_attribute::{DONT_ENUM, READ_ONLY};

    #[test]
    fn add_reports_new_entries_and_find_returns_copies() {
        let mut map = SparseArrayValueMap::default();
        assert!(map.add(7));
        assert!(!map.add(7));
        let entry = map.find(7).unwrap();
        assert_eq!(entry.index(), 7);
        assert_eq!(entry.attributes(), 0);
        assert!(entry.value().is_undefined());
        assert!(map.find(8).is_none());
    }

    #[test]
    fn force_set_updates_value_and_attributes() {
        let mut map = SparseArrayValueMap::default();
        map.add(3);
        map.force_set(3, Some(js_number_i32(9)), READ_ONLY | DONT_ENUM);
        let entry = map.find(3).unwrap();
        assert_eq!(entry.attributes(), READ_ONLY | DONT_ENUM);
        assert_eq!(entry.value().as_int32(), 9);
        map.force_set(3, None, 0);
        assert_eq!(map.find(3).unwrap().value().as_int32(), 9);
        assert!(!map.has_any_kind_of_getter_setter_properties());
    }

    #[test]
    fn iteration_is_ordered_by_index() {
        let mut map = SparseArrayValueMap::default();
        for index in [30, 2, 11] {
            map.add(index);
        }
        let indices: Vec<u32> = map.iter().map(|entry| entry.index()).collect();
        assert_eq!(indices, vec![2, 11, 30]);
        map.remove(11);
        assert_eq!(map.size(), 2);
    }
}
