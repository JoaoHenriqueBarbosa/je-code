//! Porte de `.../bracketPairsTree/smallImmutableSet.ts`: um conjunto imutável de inteiros densos, que
//! funciona melhor com poucos elementos (menos de 32) e codifica a pertinência em bits.
//!
//! Divergências:
//! - O `SmallImmutableSet<T>` do original é genérico só para o `IDenseKeyProvider<T>` das operações; o
//!   conjunto em si guarda bits. Aqui o tipo não é genérico e `add`/`has` recebem o provedor de chaves.
//! - O cache de 129 instâncias (`cache`) existe no original para não alocar conjuntos de até 7 elementos
//!   densos. Aqui o conjunto é um valor barato de copiar (um `u32` e, raramente, um `Rc` de bits extras), e a
//!   igualdade é por valor, então o cache não tem o que otimizar.

use std::cell::RefCell;
use std::collections::HashMap;
use std::hash::Hash;
use std::rc::Rc;

/// `SmallImmutableSet`.
#[derive(Clone, Debug, Default)]
pub struct SmallImmutableSet {
    items: u32,
    /// Os bits dos elementos de chave 32 em diante, de 32 em 32. `None` é o vetor vazio do original.
    additional_items: Option<Rc<[u32]>>,
}

impl SmallImmutableSet {
    fn create(items: u32, additional_items: Vec<u32>) -> Self {
        Self {
            items,
            additional_items: if additional_items.is_empty() {
                None
            } else {
                Some(additional_items.into())
            },
        }
    }

    fn additional(&self) -> &[u32] {
        self.additional_items.as_deref().unwrap_or(&[])
    }

    /// `getEmpty()`.
    pub fn get_empty() -> Self {
        Self::default()
    }

    /// `add(value, keyProvider)`.
    pub fn add<T>(&self, value: &T, key_provider: &dyn IDenseKeyProvider<T>) -> Self {
        let key = key_provider.get_key(value);
        let mut idx = key >> 5; // dividido por 32
        if idx == 0 {
            // caminho rápido
            let new_item = (1u32 << key) | self.items;
            if new_item == self.items {
                return self.clone();
            }
            return Self {
                items: new_item,
                additional_items: self.additional_items.clone(),
            };
        }
        idx -= 1;
        let mut new_items = self.additional().to_vec();
        while new_items.len() <= idx {
            new_items.push(0);
        }
        new_items[idx] |= 1u32 << (key & 31);
        Self::create(self.items, new_items)
    }

    /// `has(value, keyProvider)`.
    pub fn has<T>(&self, value: &T, key_provider: &dyn IDenseKeyProvider<T>) -> bool {
        let key = key_provider.get_key(value);
        let mut idx = key >> 5; // dividido por 32
        if idx == 0 {
            // caminho rápido
            return (self.items & (1u32 << key)) != 0;
        }
        idx -= 1;
        (self.additional().get(idx).copied().unwrap_or(0) & (1u32 << (key & 31))) != 0
    }

    /// `merge(other)`.
    pub fn merge(&self, other: &SmallImmutableSet) -> Self {
        let merged = self.items | other.items;

        if self.additional_items.is_none() && other.additional_items.is_none() {
            // caminho rápido
            if merged == self.items {
                return self.clone();
            }
            if merged == other.items {
                return other.clone();
            }
            return Self {
                items: merged,
                additional_items: None,
            };
        }

        // Dá para otimizar, mas não é o caso comum
        let (a, b) = (self.additional(), other.additional());
        let new_items: Vec<u32> = (0..a.len().max(b.len()))
            .map(|i| a.get(i).copied().unwrap_or(0) | b.get(i).copied().unwrap_or(0))
            .collect();
        Self::create(merged, new_items)
    }

    /// `intersects(other)`.
    pub fn intersects(&self, other: &SmallImmutableSet) -> bool {
        if (self.items & other.items) != 0 {
            return true;
        }
        self.additional()
            .iter()
            .zip(other.additional())
            .any(|(a, b)| (a & b) != 0)
    }

    /// `equals(other)`.
    pub fn equals(&self, other: &SmallImmutableSet) -> bool {
        self.items == other.items && self.additional() == other.additional()
    }
}

/// `IDenseKeyProvider<T>`.
pub trait IDenseKeyProvider<T> {
    fn get_key(&self, value: &T) -> usize;
}

/// `identityKeyProvider`: o próprio número é a chave.
pub struct IdentityKeyProvider;

impl IDenseKeyProvider<usize> for IdentityKeyProvider {
    fn get_key(&self, value: &usize) -> usize {
        *value
    }
}

/// `identityKeyProvider`.
pub const IDENTITY_KEY_PROVIDER: IdentityKeyProvider = IdentityKeyProvider;

/// `DenseKeyProvider<T>`: dá a cada valor uma chave única crescente.
pub struct DenseKeyProvider<T: Hash + Eq + Clone> {
    items: RefCell<HashMap<T, usize>>,
    /// A ordem de inserção, que o `Map` do original preserva.
    order: RefCell<Vec<T>>,
}

impl<T: Hash + Eq + Clone> Default for DenseKeyProvider<T> {
    fn default() -> Self {
        Self {
            items: RefCell::new(HashMap::new()),
            order: RefCell::new(Vec::new()),
        }
    }
}

impl<T: Hash + Eq + Clone> DenseKeyProvider<T> {
    pub fn new() -> Self {
        Self::default()
    }

    /// `getKey(value)`.
    pub fn get_key(&self, value: &T) -> usize {
        if let Some(existing) = self.items.borrow().get(value) {
            return *existing;
        }
        let key = self.items.borrow().len();
        self.items.borrow_mut().insert(value.clone(), key);
        self.order.borrow_mut().push(value.clone());
        key
    }

    /// `reverseLookup(value)`.
    pub fn reverse_lookup(&self, value: usize) -> Option<T> {
        self.order.borrow().get(value).cloned()
    }

    /// `reverseLookupSet(set)`: os valores do conjunto, na ordem de inserção.
    pub fn reverse_lookup_set(&self, set: &SmallImmutableSet) -> Vec<T> {
        let order = self.order.borrow();
        order
            .iter()
            .enumerate()
            .filter(|(key, _)| set.has(key, &IDENTITY_KEY_PROVIDER))
            .map(|(_, value)| value.clone())
            .collect()
    }

    /// `keys()`: as chaves de valor, na ordem de inserção.
    pub fn keys(&self) -> Vec<T> {
        self.order.borrow().clone()
    }
}

impl<T: Hash + Eq + Clone> IDenseKeyProvider<T> for DenseKeyProvider<T> {
    fn get_key(&self, value: &T) -> usize {
        DenseKeyProvider::get_key(self, value)
    }
}
