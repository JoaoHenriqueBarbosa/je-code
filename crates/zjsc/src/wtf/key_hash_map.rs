//! Reprodução fiel, para chaves `UniquedKey`, de `WTF/wtf/HashTable.h` (`UncheckedKeyHashMap` e
//! `UncheckedKeyHashSet`) e de `WTF/wtf/InlineMap.h`, com o `IdentifierRepHash` do JSC.
//!
//! O que importa aqui é a ORDEM DE ITERAÇÃO, que o `BytecodeGenerator` torna observável (a ordem em
//! que `pushLexicalScopeInternal` aloca os registradores dos `let`): a posição de cada entrada na
//! tabela depende do hash, da capacidade, do sondeamento e do momento em que cada rehash acontece.
//!
//! Reproduzido:
//! - sondeamento quadrático triangular, `i = (i + ++probe) & mask`;
//! - `HashTable`: tamanho mínimo 8, expansão depois da inserção (`shouldExpand`: carga 3/4 até 1024
//!   buckets e 1/2 acima), rehash no mesmo tamanho quando `keyCount * 6 < tableSize * 2`, encolhe
//!   pela metade quando `keyCount * 6 < tableSize` e `tableSize > 8`;
//! - `InlineMap`: até `INLINE` entradas em ordem de inserção (remoção move a última para o buraco),
//!   depois tabela com capacidade `roundUpToPowerOfTwo(size * 2 * 4 / 3)`, expansão ANTES da
//!   inserção (`(size + deleted) * 4 >= capacity * 3`), volta para inline quando encolhe a
//!   `size <= INLINE`;
//! - `IdentifierRepHash::hash`: `existingSymbolAwareHash()`.
//!
//! O hash de um símbolo é o `SymbolImpl::hashForSymbol` (contador de criação), guardado no
//! `StringImpl` do símbolo; a ordem de símbolos numa tabela é a do C++.

use crate::wtf::text::string_impl::UniquedKey;

/// `IdentifierRepHash::hash`: `key->existingSymbolAwareHash()`.
pub fn identifier_rep_hash(key: &UniquedKey) -> u32 {
    // Atoms do porte nem sempre têm o hash pré-calculado como no C++; `symbol_aware_hash` o calcula
    // (mesmo valor quando já existe), sem mudar a tabela depois da inserção.
    key.0.symbol_aware_hash()
}

/// `AddResult`: `iterator->key`, `iterator->value` e `isNewEntry`.
pub struct AddResult<'a, V> {
    pub key: &'a UniquedKey,
    pub value: &'a mut V,
    pub is_new_entry: bool,
}

#[derive(Clone, Debug)]
enum Slot<V> {
    Empty,
    Deleted,
    Full((UniquedKey, V)),
}

/// A tabela aberta: `m_table` de `HashTable` e `hashedData` de `InlineMap`.
#[derive(Clone, Debug)]
struct Buckets<V> {
    slots: Vec<Slot<V>>,
}

impl<V> Buckets<V> {
    fn with_capacity(capacity: usize) -> Buckets<V> {
        let mut slots = Vec::with_capacity(capacity);
        slots.resize_with(capacity, || Slot::Empty);
        Buckets { slots }
    }

    fn mask(&self) -> usize {
        self.slots.len() - 1
    }

    /// `lookup`: o índice da chave, se existe.
    fn lookup(&self, key: &UniquedKey) -> Option<usize> {
        if self.slots.is_empty() {
            return None;
        }
        let mask = self.mask();
        let mut i = identifier_rep_hash(key) as usize & mask;
        let mut probe = 0;
        loop {
            match &self.slots[i] {
                Slot::Empty => return None,
                Slot::Full((entry_key, _)) if entry_key == key => return Some(i),
                _ => {}
            }
            probe += 1;
            i = (i + probe) & mask;
        }
    }

    /// O laço de `add`: `Ok(índice)` se a chave existe; `Err(índice)` do bucket onde inserir (o
    /// último deletado visto, senão o vazio em que a busca parou).
    fn find_for_add(&self, key: &UniquedKey) -> Result<usize, usize> {
        let mask = self.mask();
        let mut i = identifier_rep_hash(key) as usize & mask;
        let mut probe = 0;
        let mut deleted = None;
        loop {
            match &self.slots[i] {
                Slot::Empty => return Err(deleted.unwrap_or(i)),
                Slot::Deleted => deleted = Some(i),
                Slot::Full((entry_key, _)) if entry_key == key => return Ok(i),
                Slot::Full(_) => {}
            }
            probe += 1;
            i = (i + probe) & mask;
        }
    }

    /// `findKeyOrEmptyOrDeleted` do `InlineMap`: devolve o PRIMEIRO deletado visto.
    fn find_for_add_first_deleted(&self, key: &UniquedKey) -> Result<usize, usize> {
        let mask = self.mask();
        let mut i = identifier_rep_hash(key) as usize & mask;
        let mut probe = 0;
        let mut deleted = None;
        loop {
            match &self.slots[i] {
                Slot::Empty => return Err(deleted.unwrap_or(i)),
                Slot::Deleted => {
                    if deleted.is_none() {
                        deleted = Some(i);
                    }
                }
                Slot::Full((entry_key, _)) if entry_key == key => return Ok(i),
                Slot::Full(_) => {}
            }
            probe += 1;
            i = (i + probe) & mask;
        }
    }

    /// `reinsert` / `findKeyOrEmptyInStorage` para chave nova: o primeiro vazio da sequência.
    fn reinsert(&mut self, entry: (UniquedKey, V)) -> usize {
        let mask = self.mask();
        let mut i = identifier_rep_hash(&entry.0) as usize & mask;
        let mut probe = 0;
        while !matches!(self.slots[i], Slot::Empty) {
            probe += 1;
            i = (i + probe) & mask;
        }
        self.slots[i] = Slot::Full(entry);
        i
    }

    /// `rehash`: percorre a tabela antiga em ordem de índice. Devolve o novo índice de `track`.
    fn rehash(&mut self, new_size: usize, track: Option<usize>) -> Option<usize> {
        let old = std::mem::replace(self, Buckets::with_capacity(new_size));
        let mut tracked = None;
        for (index, slot) in old.slots.into_iter().enumerate() {
            if let Slot::Full(entry) = slot {
                let new_index = self.reinsert(entry);
                if track == Some(index) {
                    tracked = Some(new_index);
                }
            }
        }
        tracked
    }

    fn into_entries(self) -> impl Iterator<Item = (UniquedKey, V)> {
        self.slots.into_iter().filter_map(|slot| match slot {
            Slot::Full(entry) => Some(entry),
            _ => None,
        })
    }
}

#[derive(Clone, Debug)]
enum Repr<V> {
    /// Entradas em ordem de inserção (só com `INLINE > 0`).
    Inline(Vec<(UniquedKey, V)>),
    Hashed(Buckets<V>),
}

/// `UncheckedKeyHashMap`/`UncheckedKeyHashSet` (`INLINE == 0`) ou `InlineMap<..., INLINE>`.
#[derive(Clone, Debug)]
pub struct KeyHashMap<V, const INLINE: usize = 0> {
    repr: Repr<V>,
    key_count: usize,
    deleted_count: usize,
}

impl<V, const INLINE: usize> Default for KeyHashMap<V, INLINE> {
    fn default() -> Self {
        let repr = if INLINE > 0 { Repr::Inline(Vec::new()) } else { Repr::Hashed(Buckets { slots: Vec::new() }) };
        KeyHashMap { repr, key_count: 0, deleted_count: 0 }
    }
}

const MIN_TABLE_SIZE: usize = 8;
const MAX_SMALL_TABLE_CAPACITY: usize = 1024;
const MIN_LOAD: usize = 6;

/// `HashTableCapacityForSize::shouldExpand`.
fn hash_table_should_expand(key_and_delete_count: usize, table_size: usize) -> bool {
    if table_size <= MAX_SMALL_TABLE_CAPACITY {
        key_and_delete_count * 4 >= table_size * 3
    } else {
        key_and_delete_count * 2 >= table_size
    }
}

impl<V, const INLINE: usize> KeyHashMap<V, INLINE> {
    pub fn len(&self) -> u32 {
        self.key_count as u32
    }

    pub fn is_empty(&self) -> bool {
        self.key_count == 0
    }

    pub fn contains(&self, key: &UniquedKey) -> bool {
        self.position(key).is_some()
    }

    /// Posição da chave: índice no vetor inline ou no bucket.
    fn position(&self, key: &UniquedKey) -> Option<usize> {
        match &self.repr {
            Repr::Inline(entries) => entries.iter().position(|(entry_key, _)| entry_key == key),
            Repr::Hashed(buckets) => buckets.lookup(key),
        }
    }

    fn entry_at(&self, index: usize) -> &(UniquedKey, V) {
        match &self.repr {
            Repr::Inline(entries) => &entries[index],
            Repr::Hashed(buckets) => match &buckets.slots[index] {
                Slot::Full(entry) => entry,
                _ => unreachable!("índice de bucket sem entrada"),
            },
        }
    }

    fn entry_at_mut(&mut self, index: usize) -> &mut (UniquedKey, V) {
        match &mut self.repr {
            Repr::Inline(entries) => &mut entries[index],
            Repr::Hashed(buckets) => match &mut buckets.slots[index] {
                Slot::Full(entry) => entry,
                _ => unreachable!("índice de bucket sem entrada"),
            },
        }
    }

    pub fn find(&self, key: &UniquedKey) -> Option<&V> {
        self.position(key).map(|index| &self.entry_at(index).1)
    }

    pub fn find_mut(&mut self, key: &UniquedKey) -> Option<&mut V> {
        let index = self.position(key)?;
        Some(&mut self.entry_at_mut(index).1)
    }

    fn result_at(&mut self, index: usize, is_new_entry: bool) -> AddResult<'_, V> {
        let (key, value) = self.entry_at_mut(index);
        AddResult { key, value, is_new_entry }
    }

    /// `add`: não sobrescreve uma entrada existente.
    pub fn add(&mut self, key: &UniquedKey, value: V) -> AddResult<'_, V> {
        let (index, unused) = self.insert(key, value);
        self.result_at(index, unused.is_none())
    }

    /// `set`: sobrescreve o valor de uma entrada existente.
    pub fn set(&mut self, key: &UniquedKey, value: V) -> AddResult<'_, V> {
        let (index, unused) = self.insert(key, value);
        let is_new_entry = unused.is_none();
        if let Some(value) = unused {
            self.entry_at_mut(index).1 = value;
        }
        self.result_at(index, is_new_entry)
    }

    /// Devolve o índice final da entrada e, se a chave já existia, o valor que não foi usado.
    fn insert(&mut self, key: &UniquedKey, value: V) -> (usize, Option<V>) {
        if INLINE > 0 {
            self.insert_inline_map(key, value)
        } else {
            self.insert_hash_table(key, value)
        }
    }

    /// `HashTable::add`: a tabela nasce com 8 buckets; expande DEPOIS de inserir.
    fn insert_hash_table(&mut self, key: &UniquedKey, value: V) -> (usize, Option<V>) {
        let Repr::Hashed(buckets) = &mut self.repr else { unreachable!() };
        if buckets.slots.is_empty() {
            *buckets = Buckets::with_capacity(MIN_TABLE_SIZE);
        }
        let slot = match buckets.find_for_add(key) {
            Ok(index) => return (index, Some(value)),
            Err(slot) => slot,
        };
        if matches!(buckets.slots[slot], Slot::Deleted) {
            self.deleted_count -= 1;
        }
        buckets.slots[slot] = Slot::Full((key.clone(), value));
        self.key_count += 1;
        let table_size = buckets.slots.len();
        if hash_table_should_expand(self.key_count + self.deleted_count, table_size) {
            // `expand`: no mesmo tamanho se `keyCount * minLoad < tableSize * 2`.
            let new_size = if self.key_count * MIN_LOAD < table_size * 2 { table_size } else { table_size * 2 };
            let moved = buckets.rehash(new_size, Some(slot));
            self.deleted_count = 0;
            return (moved.expect("entrada recém-inserida"), None);
        }
        (slot, None)
    }

    /// `InlineMap::add`: inline até `INLINE`, depois tabela; expande ANTES de inserir.
    fn insert_inline_map(&mut self, key: &UniquedKey, value: V) -> (usize, Option<V>) {
        if let Repr::Inline(entries) = &mut self.repr {
            if let Some(index) = entries.iter().position(|(entry_key, _)| entry_key == key) {
                return (index, Some(value));
            }
            if entries.len() < INLINE {
                entries.push((key.clone(), value));
                self.key_count += 1;
                return (entries.len() - 1, None);
            }
            // `transitionToHashed`.
            let capacity = (self.key_count * 2 * 4 / 3).next_power_of_two();
            let mut buckets = Buckets::with_capacity(capacity);
            for entry in std::mem::take(entries) {
                buckets.reinsert(entry);
            }
            self.repr = Repr::Hashed(buckets);
            self.deleted_count = 0;
        }
        let Repr::Hashed(buckets) = &mut self.repr else { unreachable!() };
        let capacity = buckets.slots.len();
        if (self.key_count + self.deleted_count) * 4 >= capacity * 3 {
            // `expand`: `shouldCompactOnly` rehasha no mesmo tamanho.
            let new_size = if self.key_count * MIN_LOAD < capacity * 2 { capacity } else { capacity * 2 };
            buckets.rehash(new_size, None);
            self.deleted_count = 0;
        }
        let slot = match buckets.find_for_add_first_deleted(key) {
            Ok(index) => return (index, Some(value)),
            Err(slot) => slot,
        };
        if matches!(buckets.slots[slot], Slot::Deleted) {
            self.deleted_count -= 1;
        }
        buckets.slots[slot] = Slot::Full((key.clone(), value));
        self.key_count += 1;
        (slot, None)
    }

    pub fn remove(&mut self, key: &UniquedKey) -> bool {
        let Some(index) = self.position(key) else {
            return false;
        };
        match &mut self.repr {
            Repr::Inline(entries) => {
                // Move a última entrada para o buraco.
                entries.swap_remove(index);
                self.key_count -= 1;
            }
            Repr::Hashed(buckets) => {
                buckets.slots[index] = Slot::Deleted;
                self.deleted_count += 1;
                self.key_count -= 1;
                let table_size = buckets.slots.len();
                if INLINE > 0 {
                    if self.key_count * MIN_LOAD < table_size {
                        self.shrink_inline_map();
                    }
                } else if self.key_count * MIN_LOAD < table_size && table_size > MIN_TABLE_SIZE {
                    buckets.rehash(table_size / 2, None);
                    self.deleted_count = 0;
                }
            }
        }
        true
    }

    /// `InlineMap::shrink`: volta para inline se couber, senão rehash pela metade.
    fn shrink_inline_map(&mut self) {
        let Repr::Hashed(buckets) = &mut self.repr else { unreachable!() };
        if self.key_count <= INLINE {
            let entries: Vec<_> = std::mem::replace(buckets, Buckets { slots: Vec::new() }).into_entries().collect();
            self.repr = Repr::Inline(entries);
        } else {
            let half = buckets.slots.len() / 2;
            buckets.rehash(half, None);
        }
        self.deleted_count = 0;
    }

    /// `begin()`/`end()`: ordem de tabela (ou de inserção, no trecho inline).
    pub fn iter(&self) -> impl Iterator<Item = &(UniquedKey, V)> + '_ {
        let (inline, hashed) = match &self.repr {
            Repr::Inline(entries) => (Some(entries.iter()), None),
            Repr::Hashed(buckets) => (None, Some(buckets.slots.iter())),
        };
        inline.into_iter().flatten().chain(hashed.into_iter().flatten().filter_map(|slot| match slot {
            Slot::Full(entry) => Some(entry),
            _ => None,
        }))
    }

    pub fn iter_mut(&mut self) -> impl Iterator<Item = &mut (UniquedKey, V)> + '_ {
        let (inline, hashed) = match &mut self.repr {
            Repr::Inline(entries) => (Some(entries.iter_mut()), None),
            Repr::Hashed(buckets) => (None, Some(buckets.slots.iter_mut())),
        };
        inline.into_iter().flatten().chain(hashed.into_iter().flatten().filter_map(|slot| match slot {
            Slot::Full(entry) => Some(entry),
            _ => None,
        }))
    }

    pub fn values(&self) -> impl Iterator<Item = &V> + '_ {
        self.iter().map(|(_, value)| value)
    }

    pub fn values_mut(&mut self) -> impl Iterator<Item = &mut V> + '_ {
        self.iter_mut().map(|(_, value)| value)
    }

    /// Igualdade como conjunto de chaves (o `operator==` do `HashSet`: não depende da ordem).
    pub fn has_same_keys<W, const OTHER: usize>(&self, other: &KeyHashMap<W, OTHER>) -> bool {
        self.key_count == other.key_count && self.iter().all(|(key, _)| other.contains(key))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::wtf::text::string_impl::StringImpl;

    fn key(text: &str) -> UniquedKey {
        UniquedKey(StringImpl::create(text.as_bytes()))
    }

    fn names<V, const N: usize>(map: &KeyHashMap<V, N>) -> Vec<String> {
        map.iter().map(|(key, _)| String::from_utf8(key.0.span8().to_vec()).unwrap()).collect()
    }

    /// Posição esperada de cada chave: bucket calculado à mão a partir do hash do `StringImpl`.
    fn expected_order(texts: &[&str], capacity: usize) -> Vec<String> {
        let mut slots: Vec<Option<&str>> = vec![None; capacity];
        for text in texts {
            let mut i = key(text).0.hash() as usize & (capacity - 1);
            let mut probe = 0;
            while slots[i].is_some() {
                probe += 1;
                i = (i + probe) & (capacity - 1);
            }
            slots[i] = Some(text);
        }
        slots.into_iter().flatten().map(String::from).collect()
    }

    #[test]
    fn inline_map_keeps_insertion_order_up_to_nine() {
        let mut map = KeyHashMap::<u32, 9>::default();
        let texts = ["a", "b", "c", "d", "e", "f", "g", "h", "i"];
        for (n, text) in texts.iter().enumerate() {
            map.add(&key(text), n as u32);
        }
        assert_eq!(names(&map), texts);
    }

    #[test]
    fn inline_map_goes_hashed_at_ten_with_capacity_32() {
        let mut map = KeyHashMap::<u32, 9>::default();
        let texts = ["a", "b", "c", "d", "e", "f", "g", "h", "i", "j", "k"];
        for (n, text) in texts.iter().enumerate() {
            map.add(&key(text), n as u32);
        }
        // 9 -> 32 de capacidade; 11 entradas ficam bem abaixo de 3/4.
        assert_eq!(names(&map), expected_order(&texts, 32));
    }

    #[test]
    fn hash_table_starts_at_eight_and_doubles_after_insert() {
        let mut map = KeyHashMap::<()>::default();
        let texts = ["x", "y", "z", "w", "v", "u", "t"];
        for text in texts {
            map.add(&key(text), ());
        }
        // A 6ª chave em 8 buckets dispara a expansão (6 * 4 >= 8 * 3): o rehash reinsere pela
        // ordem de índice da tabela de 8, e só então entra a 7ª.
        let in_eight = expected_order(&texts[..6], 8);
        let mut rehash_order: Vec<&str> = in_eight.iter().map(String::as_str).collect();
        rehash_order.push(texts[6]);
        assert_eq!(names(&map), expected_order(&rehash_order, 16));
    }

    #[test]
    fn add_does_not_overwrite_and_set_does() {
        let mut map = KeyHashMap::<u32, 9>::default();
        let k = key("a");
        assert!(map.add(&k, 1).is_new_entry);
        assert!(!map.add(&k, 2).is_new_entry);
        assert_eq!(map.find(&k), Some(&1));
        assert!(!map.set(&k, 3).is_new_entry);
        assert_eq!(map.find(&k), Some(&3));
    }

    #[test]
    fn inline_remove_moves_last_entry_into_the_hole() {
        let mut map = KeyHashMap::<u32, 9>::default();
        let keys: Vec<_> = ["a", "b", "c", "d"].iter().map(|text| key(text)).collect();
        for (n, k) in keys.iter().enumerate() {
            map.add(k, n as u32);
        }
        assert!(map.remove(&keys[1]));
        assert_eq!(names(&map), ["a", "d", "c"]);
    }

    #[test]
    fn hash_table_shrinks_and_keeps_lookup() {
        let mut map = KeyHashMap::<u32>::default();
        let keys: Vec<_> = (0..40).map(|n| key(&format!("name{n}"))).collect();
        for (n, k) in keys.iter().enumerate() {
            map.add(k, n as u32);
        }
        for k in &keys[..38] {
            assert!(map.remove(k));
        }
        assert_eq!(map.len(), 2);
        assert_eq!(map.find(&keys[38]), Some(&38));
        assert_eq!(map.find(&keys[39]), Some(&39));
    }
}

