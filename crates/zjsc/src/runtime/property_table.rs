//! Tradução de `runtime/PropertyTable.{h,cpp}` e `PropertyTableInlines.h`.
//!
//! DIVERGÊNCIA (hash aberto compacto do C++): o `PropertyTable` do C++ é uma célula do GC com um vetor
//! de índices de endereçamento aberto (8 ou 16 bits, modo "compact") mais um vetor de entradas em ordem
//! de inserção. Nada disso é observável: a ordem de iteração é a das entradas, e é isso que o porte
//! guarda (`Vec<Option<PropertyTableEntry>>`, `None` é a `PROPERTY_MAP_DELETED_ENTRY_KEY`), com um
//! `HashMap` por identidade do `UniquedKey` no lugar do vetor de índices. `rehash`/`canInsert`/
//! `sizeForCapacity`/`isCompact`/`dataSize` e os contadores de `DUMP_PROPERTYMAP_STATS` somem; a
//! compactação das entradas apagadas acontece com a mesma condição de `remove` (`deleted * 4 >= size`).
//! `copy(vm, newCapacity)` e `clone` são o `Clone`. A tabela não é célula: o `Structure` a possui.
//!
//! Fora desta fatia: `visitChildren` e `dump`. `renumberPropertyOffsets` recebe, no lugar do `JSObject*`,
//! a função que lê o valor de um offset (o `object->getDirect(entry.offset())`), e devolve os valores.

use std::collections::HashMap;

use crate::runtime::js_value::JSValue;
use crate::runtime::property_attribute::{ACCESSOR, DONT_DELETE, READ_ONLY};
use crate::runtime::property_offset::{offset_for_property_number, PropertyOffset, INVALID_OFFSET};
use crate::wtf::text::string_impl::UniquedKey;

/// `PropertyMapEntry` (`PropertyTableEntry`): chave, offset e atributos (cabem em `uint8_t`).
///
/// `is_private` é o `isPrivate()` do `SymbolImpl` da chave, que o `StringImpl` do porte não carrega.
#[derive(Clone, Debug)]
pub struct PropertyTableEntry {
    key: UniquedKey,
    offset: PropertyOffset,
    attributes: u8,
    is_private: bool,
}

impl PropertyTableEntry {
    /// `PropertyMapEntry(UniquedStringImpl*, PropertyOffset, unsigned attributes)`.
    pub fn new(key: UniquedKey, offset: PropertyOffset, attributes: u32, is_private: bool) -> PropertyTableEntry {
        debug_assert!(attributes <= u8::MAX as u32);
        PropertyTableEntry { key, offset, attributes: attributes as u8, is_private }
    }

    pub fn key(&self) -> &UniquedKey {
        &self.key
    }

    pub fn offset(&self) -> PropertyOffset {
        self.offset
    }

    pub fn attributes(&self) -> u32 {
        self.attributes as u32
    }

    pub fn is_private(&self) -> bool {
        self.is_private
    }

    fn set_attributes(&mut self, attributes: u32) {
        debug_assert!(attributes <= u8::MAX as u32);
        self.attributes = attributes as u8;
    }
}

/// `class PropertyTable`.
#[derive(Clone, Debug, Default)]
pub struct PropertyTable {
    /// Entradas em ordem de inserção; `None` é a entrada apagada.
    entries: Vec<Option<PropertyTableEntry>>,
    /// Chave para a posição em `entries`.
    index: HashMap<UniquedKey, usize>,
    /// `m_deletedOffsets`.
    deleted_offsets: Vec<PropertyOffset>,
    /// `m_deletedCount`.
    deleted_count: usize,
}

impl PropertyTable {
    /// `find(key)`: a entrada, se existir.
    pub fn find(&self, key: &UniquedKey) -> Option<&PropertyTableEntry> {
        let position = *self.index.get(key)?;
        self.entries[position].as_ref()
    }

    /// `get(key)`: `(offset, attributes)`, com `invalidOffset` quando não há a chave.
    pub fn get(&self, key: &UniquedKey) -> (PropertyOffset, u32) {
        match self.find(key) {
            Some(entry) => (entry.offset, entry.attributes()),
            None => (INVALID_OFFSET, 0),
        }
    }

    /// `add(vm, entry)`: `(offset, attributes, added)`. Se a chave já existe, devolve a entrada
    /// existente e `false`, sem alterar a tabela.
    pub fn add(&mut self, entry: PropertyTableEntry) -> (PropertyOffset, u32, bool) {
        debug_assert!(!self.deleted_offsets.contains(&entry.offset));

        // Look for a value with a matching key already in the array.
        if let Some(existing) = self.find(&entry.key) {
            return (existing.offset, existing.attributes(), false);
        }

        let result = (entry.offset, entry.attributes(), true);
        self.index.insert(entry.key.clone(), self.entries.len());
        self.entries.push(Some(entry));
        result
    }

    /// `remove(vm, key, entryIndex, index)` por chave: apaga a entrada e conta uma apagada.
    fn remove_at(&mut self, position: usize) {
        if let Some(entry) = self.entries[position].take() {
            self.index.remove(&entry.key);
        }
        self.deleted_count += 1;
        debug_assert!(self.index.len() == self.size() as usize);
        if self.deleted_count * 4 >= self.entries.len() {
            self.compact();
        }
    }

    /// O `rehash(vm, m_keyCount, true)` de `remove`: tira as entradas apagadas, mantendo a ordem.
    fn compact(&mut self) {
        self.entries.retain(|entry| entry.is_some());
        self.index.clear();
        for (position, entry) in self.entries.iter().enumerate() {
            if let Some(entry) = entry {
                self.index.insert(entry.key.clone(), position);
            }
        }
        self.deleted_count = 0;
    }

    /// `take(vm, key)`: remove a chave e devolve `(offset, attributes)` que ela tinha.
    pub fn take(&mut self, key: &UniquedKey) -> (PropertyOffset, u32) {
        let Some(&position) = self.index.get(key) else {
            return (INVALID_OFFSET, 0);
        };
        let result = match &self.entries[position] {
            Some(entry) => (entry.offset, entry.attributes()),
            None => return (INVALID_OFFSET, 0),
        };
        self.remove_at(position);
        result
    }

    /// `updateAttributeIfExists(key, attributes)`: o offset, ou `invalidOffset` se a chave não existe.
    pub fn update_attribute_if_exists(&mut self, key: &UniquedKey, attributes: u32) -> PropertyOffset {
        let Some(&position) = self.index.get(key) else {
            return INVALID_OFFSET;
        };
        match &mut self.entries[position] {
            Some(entry) => {
                entry.set_attributes(attributes);
                entry.offset
            }
            None => INVALID_OFFSET,
        }
    }

    /// `size()`: o número de chaves.
    pub fn size(&self) -> u32 {
        (self.entries.len() - self.deleted_count) as u32
    }

    /// `isEmpty()`.
    pub fn is_empty(&self) -> bool {
        self.size() == 0
    }

    /// `propertyStorageSize()`.
    pub fn property_storage_size(&self) -> u32 {
        self.size() + self.deleted_offsets.len() as u32
    }

    /// `clearDeletedOffsets()`.
    pub fn clear_deleted_offsets(&mut self) {
        self.deleted_offsets.clear();
    }

    /// `hasDeletedOffset()`.
    pub fn has_deleted_offset(&self) -> bool {
        !self.deleted_offsets.is_empty()
    }

    /// `takeDeletedOffset()`.
    pub fn take_deleted_offset(&mut self) -> PropertyOffset {
        self.deleted_offsets.pop().expect("takeDeletedOffset sem offset apagado (ASSERT do C++)")
    }

    /// `addDeletedOffset(offset)`.
    pub fn add_deleted_offset(&mut self, offset: PropertyOffset) {
        debug_assert!(!self.deleted_offsets.contains(&offset));
        self.deleted_offsets.push(offset);
    }

    /// `nextOffset(inlineCapacity)`.
    pub fn next_offset(&mut self, inline_capacity: PropertyOffset) -> PropertyOffset {
        if self.has_deleted_offset() {
            return self.take_deleted_offset();
        }
        offset_for_property_number(self.size() as i32, inline_capacity)
    }

    /// `forEachProperty`/`begin()..end()`: as entradas em ordem de inserção.
    pub fn iter(&self) -> impl Iterator<Item = &PropertyTableEntry> {
        self.entries.iter().flatten()
    }

    fn for_each_property_mutable(&mut self, mut apply: impl FnMut(&mut PropertyTableEntry)) {
        for entry in self.entries.iter_mut().flatten() {
            apply(entry);
        }
    }

    /// `renumberPropertyOffsets(object, inlineCapacity, values)`: renumera as entradas em ordem de
    /// inserção (o offset da i-ésima é `offsetForPropertyNumber(i, inlineCapacity)`), lê o valor de cada
    /// offset antigo com `value_at` e zera a lista de offsets apagados. Devolve `(último offset, valores)`;
    /// o último é `invalidOffset` se a tabela está vazia.
    pub fn renumber_property_offsets(
        &mut self,
        inline_capacity: i32,
        mut value_at: impl FnMut(PropertyOffset) -> JSValue,
    ) -> (PropertyOffset, Vec<JSValue>) {
        let mut values = Vec::with_capacity(self.size() as usize);
        let mut offset = INVALID_OFFSET;
        self.for_each_property_mutable(|entry| {
            values.push(value_at(entry.offset));
            offset = offset_for_property_number(values.len() as i32 - 1, inline_capacity);
            entry.offset = offset;
        });
        self.clear_deleted_offsets();
        (offset, values)
    }

    /// `seal()`.
    pub fn seal(&mut self) {
        self.for_each_property_mutable(|entry| {
            if !(entry.is_private && entry.key.0.is_symbol()) {
                entry.set_attributes(entry.attributes() | DONT_DELETE);
            }
        });
    }

    /// `freeze()`.
    pub fn freeze(&mut self) {
        self.for_each_property_mutable(|entry| {
            if !(entry.is_private && entry.key.0.is_symbol()) {
                if entry.attributes() & ACCESSOR == 0 {
                    entry.set_attributes(entry.attributes() | DONT_DELETE | READ_ONLY);
                } else {
                    entry.set_attributes(entry.attributes() | DONT_DELETE);
                }
            }
        });
    }

    /// `isSealed()`.
    pub fn is_sealed(&self) -> bool {
        self.iter().all(|entry| (entry.is_private && entry.key.0.is_symbol()) || entry.attributes() & DONT_DELETE == DONT_DELETE)
    }

    /// `isFrozen()`.
    pub fn is_frozen(&self) -> bool {
        self.iter().all(|entry| {
            if entry.is_private && entry.key.0.is_symbol() {
                return true;
            }
            if entry.attributes() & DONT_DELETE != DONT_DELETE {
                return false;
            }
            if entry.attributes() & ACCESSOR == 0 && entry.attributes() & READ_ONLY != READ_ONLY {
                return false;
            }
            true
        })
    }
}
