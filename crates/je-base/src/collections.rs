//! Porte de `src/vs/base/common/collections.ts`.
//!
//! Representação:
//! - Um `Set` do JavaScript chega como o slice dos seus elementos distintos na ordem de inserção
//!   (`&[T]`), e um `Map` como o slice dos seus pares (`&[(K, V)]`). É a ordem de inserção que
//!   `diffSets`/`diffMaps`/`intersection` expõem no resultado, e `HashSet`/`HashMap` não a têm.
//! - `groupBy` e `groupByMap` são a mesma função aqui: o `Record` do original só difere do `Map`
//!   na ordem de chaves que parecem inteiros, e uma chave genérica `K` não tem esse caso. O resultado
//!   é a lista de `(chave, grupo)` na ordem em que cada chave apareceu.
//! - `SetWithKey` guarda as entradas na ordem de inserção (como o `Map` por baixo dele) e uma tabela
//!   chave -> posição; trocar o valor de uma chave existente mantém a posição, como `Map.set`.
//!   `Symbol.toStringTag` é [`SetWithKey::to_string_tag`].

use std::collections::{HashMap, HashSet};
use std::hash::Hash;

/// Agrupa os elementos pela chave de `group_fn`, na ordem em que cada chave apareceu.
pub fn group_by<K: Hash + Eq + Clone, V: Clone>(data: &[V], group_fn: impl Fn(&V) -> K) -> Vec<(K, Vec<V>)> {
    let mut result: Vec<(K, Vec<V>)> = Vec::new();
    let mut positions: HashMap<K, usize> = HashMap::new();
    for element in data {
        let key = group_fn(element);
        match positions.get(&key) {
            Some(&position) => result[position].1.push(element.clone()),
            None => {
                positions.insert(key.clone(), result.len());
                result.push((key, vec![element.clone()]));
            }
        }
    }
    result
}

/// O que saiu e o que entrou entre dois conjuntos (ou, em [`diff_maps`], dois mapas).
#[derive(Clone, Debug, PartialEq)]
pub struct Diff<T> {
    pub removed: Vec<T>,
    pub added: Vec<T>,
}

pub fn diff_sets<T: Hash + Eq + Clone>(before: &[T], after: &[T]) -> Diff<T> {
    let before_set: HashSet<&T> = before.iter().collect();
    let after_set: HashSet<&T> = after.iter().collect();
    Diff {
        removed: before.iter().filter(|e| !after_set.contains(e)).cloned().collect(),
        added: after.iter().filter(|e| !before_set.contains(e)).cloned().collect(),
    }
}

/// Dois conjuntos têm exatamente os mesmos elementos? O mesmo slice (mesmo endereço e tamanho) é
/// igual sem olhar os elementos, como `a === b`.
pub fn equal_sets<T: Hash + Eq>(a: &[T], b: &[T]) -> bool {
    if std::ptr::eq(a, b) {
        return true;
    }
    if a.len() != b.len() {
        return false;
    }
    let b_set: HashSet<&T> = b.iter().collect();
    a.iter().all(|e| b_set.contains(e))
}

/// Os valores que saíram e os que entraram entre dois mapas, comparando só as chaves.
pub fn diff_maps<K: Hash + Eq, V: Clone>(before: &[(K, V)], after: &[(K, V)]) -> Diff<V> {
    let before_keys: HashSet<&K> = before.iter().map(|(k, _)| k).collect();
    let after_keys: HashSet<&K> = after.iter().map(|(k, _)| k).collect();
    Diff {
        removed: before.iter().filter(|(k, _)| !after_keys.contains(k)).map(|(_, v)| v.clone()).collect(),
        added: after.iter().filter(|(k, _)| !before_keys.contains(k)).map(|(_, v)| v.clone()).collect(),
    }
}

/// Interseção de `set_a` com `set_b`, na ordem em que os elementos aparecem em `set_b`, sem repetição.
pub fn intersection<T: Hash + Eq + Clone>(set_a: &[T], set_b: impl IntoIterator<Item = T>) -> Vec<T> {
    let a: HashSet<&T> = set_a.iter().collect();
    let mut seen: HashSet<T> = HashSet::new();
    let mut result = Vec::new();
    for elem in set_b {
        if a.contains(&elem) && seen.insert(elem.clone()) {
            result.push(elem);
        }
    }
    result
}

/// Um conjunto cujos elementos são identificados por uma chave derivada (`toKey`), não por
/// igualdade do valor. Guarda o último valor inserido para cada chave.
pub struct SetWithKey<T, K> {
    entries: Vec<(K, T)>,
    positions: HashMap<K, usize>,
    to_key: Box<dyn Fn(&T) -> K>,
}

impl<T, K: Hash + Eq + Clone> SetWithKey<T, K> {
    pub fn new(values: Vec<T>, to_key: impl Fn(&T) -> K + 'static) -> Self {
        let mut set = Self { entries: Vec::new(), positions: HashMap::new(), to_key: Box::new(to_key) };
        for value in values {
            set.add(value);
        }
        set
    }

    pub fn size(&self) -> usize {
        self.entries.len()
    }

    pub fn add(&mut self, value: T) -> &mut Self {
        let key = (self.to_key)(&value);
        match self.positions.get(&key) {
            Some(&position) => self.entries[position].1 = value,
            None => {
                self.positions.insert(key.clone(), self.entries.len());
                self.entries.push((key, value));
            }
        }
        self
    }

    pub fn delete(&mut self, value: &T) -> bool {
        let key = (self.to_key)(value);
        let Some(position) = self.positions.remove(&key) else {
            return false;
        };
        self.entries.remove(position);
        for (k, _) in &self.entries[position..] {
            if let Some(p) = self.positions.get_mut(k) {
                *p -= 1;
            }
        }
        true
    }

    pub fn has(&self, value: &T) -> bool {
        self.positions.contains_key(&(self.to_key)(value))
    }

    /// `(valor, valor)` de cada entrada, na ordem de inserção.
    pub fn entries(&self) -> impl Iterator<Item = (&T, &T)> {
        self.entries.iter().map(|(_, v)| (v, v))
    }

    pub fn keys(&self) -> impl Iterator<Item = &T> {
        self.values()
    }

    pub fn values(&self) -> impl Iterator<Item = &T> {
        self.entries.iter().map(|(_, v)| v)
    }

    pub fn clear(&mut self) {
        self.entries.clear();
        self.positions.clear();
    }

    pub fn for_each(&self, mut callback: impl FnMut(&T, &T)) {
        for (_, value) in &self.entries {
            callback(value, value);
        }
    }

    pub fn iter(&self) -> impl Iterator<Item = &T> {
        self.values()
    }

    /// `[Symbol.toStringTag]`.
    pub fn to_string_tag(&self) -> &'static str {
        "SetWithKey"
    }
}
