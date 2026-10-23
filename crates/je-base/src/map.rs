//! Porte de `src/vs/base/common/map.ts`: `ResourceMap`, `ResourceSet`, `LinkedMap`, os caches
//! `LRUCache` e `MRUCache`, `CounterSet`, `BidirectionalMap`, `SetMap`, `NKeyMap` e utilitários de `Map`.
//!
//! O `Map` e o `Set` do JavaScript iteram na ordem de inserção. O porte tem o [`OrderedMap`] para isso
//! (índice por hash mais vetor de entradas), usado onde a ordem é observável.
//!
//! O `LinkedMap` do original detecta modificação durante a iteração (`_state`) e lança. Para isso ser
//! observável em Rust, o `LinkedMap` tem mutabilidade interior (`RefCell`): os métodos recebem `&self`
//! e um iterador vivo vê a mudança e lança [`errors::throw`] no próximo `next`.

use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::fmt::{self, Display};
use std::hash::{Hash, Hasher};
use std::ops::Deref;
use std::rc::Rc;

use crate::errors::{self, Error};
use crate::uri::URI;
use crate::JsString;

fn throw_message(message: &str) -> ! {
    errors::throw(Error::new(message))
}

// ---- OrderedMap: o Map do JavaScript ---------------------------------------------------------------

/// Mapa com a ordem de inserção do `Map` do JavaScript: sobrescrever uma chave mantém a posição, remover
/// e reinserir manda para o fim.
#[derive(Clone, Debug)]
pub struct OrderedMap<K, V> {
    index: HashMap<K, usize>,
    entries: Vec<Option<(K, V)>>,
}

impl<K, V> Default for OrderedMap<K, V> {
    fn default() -> Self {
        Self { index: HashMap::new(), entries: Vec::new() }
    }
}

impl<K: Hash + Eq + Clone, V> OrderedMap<K, V> {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn len(&self) -> usize {
        self.index.len()
    }

    pub fn is_empty(&self) -> bool {
        self.index.is_empty()
    }

    pub fn clear(&mut self) {
        self.index.clear();
        self.entries.clear();
    }

    /// `Map#set`: devolve o valor anterior, se havia.
    pub fn insert(&mut self, key: K, value: V) -> Option<V> {
        if let Some(&slot) = self.index.get(&key) {
            let entry = self.entries[slot].as_mut().expect("índice aponta para entrada viva");
            return Some(std::mem::replace(&mut entry.1, value));
        }
        self.index.insert(key.clone(), self.entries.len());
        self.entries.push(Some((key, value)));
        None
    }

    pub fn get(&self, key: &K) -> Option<&V> {
        let slot = *self.index.get(key)?;
        self.entries[slot].as_ref().map(|(_, v)| v)
    }

    pub fn get_mut(&mut self, key: &K) -> Option<&mut V> {
        let slot = *self.index.get(key)?;
        self.entries[slot].as_mut().map(|(_, v)| v)
    }

    pub fn contains_key(&self, key: &K) -> bool {
        self.index.contains_key(key)
    }

    /// `Map#delete`, devolvendo o valor removido.
    pub fn remove(&mut self, key: &K) -> Option<V> {
        let slot = self.index.remove(key)?;
        let (_, value) = self.entries[slot].take().expect("índice aponta para entrada viva");
        // Compacta quando os buracos passam da metade, para a iteração não degradar.
        if self.entries.len() > 32 && self.index.len() * 2 < self.entries.len() {
            self.entries.retain(Option::is_some);
            for (position, entry) in self.entries.iter().enumerate() {
                if let Some((k, _)) = entry {
                    self.index.insert(k.clone(), position);
                }
            }
        }
        Some(value)
    }

    pub fn iter(&self) -> impl Iterator<Item = (&K, &V)> {
        self.entries.iter().flatten().map(|(k, v)| (k, v))
    }

    pub fn keys(&self) -> impl Iterator<Item = &K> {
        self.iter().map(|(k, _)| k)
    }

    pub fn values(&self) -> impl Iterator<Item = &V> {
        self.iter().map(|(_, v)| v)
    }

    pub fn values_mut(&mut self) -> impl Iterator<Item = &mut V> {
        self.entries.iter_mut().flatten().map(|(_, v)| v)
    }
}

/// `getOrSet`: devolve o valor da chave, inserindo `value` antes se ela não existe.
pub fn get_or_set<K: Hash + Eq + Clone, V>(map: &mut OrderedMap<K, V>, key: K, value: V) -> &V {
    if !map.contains_key(&key) {
        map.insert(key.clone(), value);
    }
    map.get(&key).expect("acabou de inserir")
}

/// `mapToString`.
pub fn map_to_string<K: Hash + Eq + Clone + Display, V: Display>(map: &OrderedMap<K, V>) -> String {
    let entries: Vec<String> = map.iter().map(|(key, value)| format!("{key} => {value}")).collect();
    format!("Map({}) {{{}}}", map.len(), entries.join(", "))
}

/// `setToString`. O `Set` do JavaScript é um [`OrderedMap`] sem valor.
pub fn set_to_string<K: Hash + Eq + Clone + Display>(set: &OrderedMap<K, ()>) -> String {
    let entries: Vec<String> = set.keys().map(ToString::to_string).collect();
    format!("Set({}) {{{}}}", set.len(), entries.join(", "))
}

// ---- ResourceMap e ResourceSet ---------------------------------------------------------------------

/// `ResourceMapKeyFn`: a identidade de um URI no mapa (por exemplo `IExtUri#getComparisonKey`).
pub type ResourceMapKeyFn = Rc<dyn Fn(&URI) -> JsString>;

fn default_to_key() -> ResourceMapKeyFn {
    Rc::new(|resource| resource.to_string(false))
}

pub struct ResourceMap<T> {
    map: OrderedMap<JsString, (URI, T)>,
    to_key: ResourceMapKeyFn,
}

impl<T> ResourceMap<T> {
    /// `new ResourceMap()`.
    pub fn new() -> Self {
        Self { map: OrderedMap::new(), to_key: default_to_key() }
    }

    /// `new ResourceMap(toKey)`.
    pub fn with_key_fn(to_key: ResourceMapKeyFn) -> Self {
        Self { map: OrderedMap::new(), to_key }
    }

    /// `new ResourceMap(entries, toKey?)`.
    pub fn from_entries(entries: impl IntoIterator<Item = (URI, T)>, to_key: Option<ResourceMapKeyFn>) -> Self {
        let mut result = Self { map: OrderedMap::new(), to_key: to_key.unwrap_or_else(default_to_key) };
        for (resource, value) in entries {
            result.set(resource, value);
        }
        result
    }

    pub fn set(&mut self, resource: URI, value: T) -> &mut Self {
        self.map.insert((self.to_key)(&resource), (resource, value));
        self
    }

    pub fn get(&self, resource: &URI) -> Option<&T> {
        self.map.get(&(self.to_key)(resource)).map(|(_, value)| value)
    }

    pub fn has(&self, resource: &URI) -> bool {
        self.map.contains_key(&(self.to_key)(resource))
    }

    pub fn size(&self) -> usize {
        self.map.len()
    }

    pub fn clear(&mut self) {
        self.map.clear();
    }

    pub fn delete(&mut self, resource: &URI) -> bool {
        self.map.remove(&(self.to_key)(resource)).is_some()
    }

    pub fn for_each(&self, mut callback: impl FnMut(&T, &URI, &ResourceMap<T>)) {
        for (resource, value) in self.map.values() {
            callback(value, resource, self);
        }
    }

    pub fn values(&self) -> impl Iterator<Item = &T> {
        self.map.values().map(|(_, value)| value)
    }

    pub fn keys(&self) -> impl Iterator<Item = &URI> {
        self.map.values().map(|(resource, _)| resource)
    }

    /// `entries()` e `[Symbol.iterator]`.
    pub fn entries(&self) -> impl Iterator<Item = (&URI, &T)> {
        self.map.values().map(|(resource, value)| (resource, value))
    }
}

impl<T: Clone> ResourceMap<T> {
    /// `new ResourceMap(other, toKey?)`: copia as entradas de `other` (as chaves são as de `other`).
    pub fn from_other(other: &ResourceMap<T>, to_key: Option<ResourceMapKeyFn>) -> Self {
        Self { map: other.map.clone(), to_key: to_key.unwrap_or_else(default_to_key) }
    }
}

impl<T> Default for ResourceMap<T> {
    fn default() -> Self {
        Self::new()
    }
}

pub struct ResourceSet {
    map: ResourceMap<URI>,
}

impl ResourceSet {
    pub fn new(to_key: Option<ResourceMapKeyFn>) -> Self {
        Self { map: ResourceMap::from_entries(std::iter::empty(), to_key) }
    }

    pub fn from_entries(entries: impl IntoIterator<Item = URI>, to_key: Option<ResourceMapKeyFn>) -> Self {
        let mut set = Self::new(to_key);
        for entry in entries {
            set.add(entry);
        }
        set
    }

    pub fn size(&self) -> usize {
        self.map.size()
    }

    pub fn add(&mut self, value: URI) -> &mut Self {
        self.map.set(value.clone(), value);
        self
    }

    pub fn clear(&mut self) {
        self.map.clear();
    }

    pub fn delete(&mut self, value: &URI) -> bool {
        self.map.delete(value)
    }

    pub fn has(&self, value: &URI) -> bool {
        self.map.has(value)
    }

    pub fn for_each(&self, mut callback: impl FnMut(&URI, &URI, &ResourceSet)) {
        for resource in self.map.keys() {
            callback(resource, resource, self);
        }
    }

    /// `keys()`, `values()` e `[Symbol.iterator]`.
    pub fn iter(&self) -> impl Iterator<Item = &URI> {
        self.map.keys()
    }

    pub fn entries(&self) -> impl Iterator<Item = (&URI, &URI)> {
        self.map.keys().map(|resource| (resource, resource))
    }
}

// ---- LinkedMap -------------------------------------------------------------------------------------

struct Node<K, V> {
    key: K,
    value: V,
    previous: Option<usize>,
    next: Option<usize>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Touch {
    None = 0,
    AsOld = 1,
    AsNew = 2,
}

struct LinkedInner<K, V> {
    map: HashMap<K, usize>,
    nodes: Vec<Option<Node<K, V>>>,
    free: Vec<usize>,
    head: Option<usize>,
    tail: Option<usize>,
    size: usize,
    state: u64,
}

impl<K, V> LinkedInner<K, V> {
    fn node(&self, index: usize) -> &Node<K, V> {
        self.nodes[index].as_ref().expect("nó vivo")
    }

    fn node_mut(&mut self, index: usize) -> &mut Node<K, V> {
        self.nodes[index].as_mut().expect("nó vivo")
    }

    fn allocate(&mut self, node: Node<K, V>) -> usize {
        match self.free.pop() {
            Some(index) => {
                self.nodes[index] = Some(node);
                index
            }
            None => {
                self.nodes.push(Some(node));
                self.nodes.len() - 1
            }
        }
    }

    fn add_item_first(&mut self, item: usize) {
        // First time Insert
        if self.head.is_none() && self.tail.is_none() {
            self.tail = Some(item);
        } else if let Some(head) = self.head {
            self.node_mut(item).next = Some(head);
            self.node_mut(head).previous = Some(item);
        } else {
            throw_message("Invalid list");
        }
        self.head = Some(item);
        self.state += 1;
    }

    fn add_item_last(&mut self, item: usize) {
        // First time Insert
        if self.head.is_none() && self.tail.is_none() {
            self.head = Some(item);
        } else if let Some(tail) = self.tail {
            self.node_mut(item).previous = Some(tail);
            self.node_mut(tail).next = Some(item);
        } else {
            throw_message("Invalid list");
        }
        self.tail = Some(item);
        self.state += 1;
    }

    /// `removeItem`: desencadeia o nó (que continua alocado, o chamador o libera).
    fn remove_item(&mut self, item: usize) {
        let (previous, next) = (self.node(item).previous, self.node(item).next);
        if Some(item) == self.head && Some(item) == self.tail {
            self.head = None;
            self.tail = None;
        } else if Some(item) == self.head {
            // This can only happen if size === 1 which is handled by the case above.
            let Some(next) = next else { throw_message("Invalid list") };
            self.node_mut(next).previous = None;
            self.head = Some(next);
        } else if Some(item) == self.tail {
            // This can only happen if size === 1 which is handled by the case above.
            let Some(previous) = previous else { throw_message("Invalid list") };
            self.node_mut(previous).next = None;
            self.tail = Some(previous);
        } else {
            let (Some(next), Some(previous)) = (next, previous) else { throw_message("Invalid list") };
            self.node_mut(next).previous = Some(previous);
            self.node_mut(previous).next = Some(next);
        }
        let node = self.node_mut(item);
        node.next = None;
        node.previous = None;
        self.state += 1;
    }

    fn touch(&mut self, item: usize, touch: Touch) {
        let (Some(head), Some(tail)) = (self.head, self.tail) else { throw_message("Invalid list") };
        if touch != Touch::AsOld && touch != Touch::AsNew {
            return;
        }
        let (next, previous) = (self.node(item).next, self.node(item).previous);

        if touch == Touch::AsOld {
            if item == head {
                return;
            }

            // Unlink the item
            if item == tail {
                // previous must be defined since item was not head but is tail
                // So there are more than on item in the map
                let previous = previous.expect("previous definido");
                self.node_mut(previous).next = None;
                self.tail = Some(previous);
            } else {
                // Both next and previous are not undefined since item was neither head nor tail.
                let (next, previous) = (next.expect("next definido"), previous.expect("previous definido"));
                self.node_mut(next).previous = Some(previous);
                self.node_mut(previous).next = Some(next);
            }

            // Insert the node at head
            self.node_mut(item).previous = None;
            self.node_mut(item).next = Some(head);
            self.node_mut(head).previous = Some(item);
            self.head = Some(item);
            self.state += 1;
        } else {
            if item == tail {
                return;
            }

            // Unlink the item.
            if item == head {
                // next must be defined since item was not tail but is head
                // So there are more than on item in the map
                let next = next.expect("next definido");
                self.node_mut(next).previous = None;
                self.head = Some(next);
            } else {
                let (next, previous) = (next.expect("next definido"), previous.expect("previous definido"));
                self.node_mut(next).previous = Some(previous);
                self.node_mut(previous).next = Some(next);
            }
            self.node_mut(item).next = None;
            self.node_mut(item).previous = Some(tail);
            self.node_mut(tail).next = Some(item);
            self.tail = Some(item);
            self.state += 1;
        }
    }
}

/// Mapa encadeado: ordem de inserção com `Touch` para reordenar, base dos caches LRU e MRU.
pub struct LinkedMap<K, V> {
    inner: RefCell<LinkedInner<K, V>>,
}

impl<K, V> Default for LinkedMap<K, V> {
    fn default() -> Self {
        Self {
            inner: RefCell::new(LinkedInner {
                map: HashMap::new(),
                nodes: Vec::new(),
                free: Vec::new(),
                head: None,
                tail: None,
                size: 0,
                state: 0,
            }),
        }
    }
}

impl<K: Hash + Eq + Clone, V: Clone> LinkedMap<K, V> {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn clear(&self) {
        let mut inner = self.inner.borrow_mut();
        inner.map.clear();
        inner.nodes.clear();
        inner.free.clear();
        inner.head = None;
        inner.tail = None;
        inner.size = 0;
        inner.state += 1;
    }

    pub fn is_empty(&self) -> bool {
        let inner = self.inner.borrow();
        inner.head.is_none() && inner.tail.is_none()
    }

    pub fn size(&self) -> usize {
        self.inner.borrow().size
    }

    pub fn first(&self) -> Option<V> {
        let inner = self.inner.borrow();
        inner.head.map(|head| inner.node(head).value.clone())
    }

    pub fn last(&self) -> Option<V> {
        let inner = self.inner.borrow();
        inner.tail.map(|tail| inner.node(tail).value.clone())
    }

    pub fn has(&self, key: &K) -> bool {
        self.inner.borrow().map.contains_key(key)
    }

    pub fn get(&self, key: &K, touch: Touch) -> Option<V> {
        let mut inner = self.inner.borrow_mut();
        let item = *inner.map.get(key)?;
        if touch != Touch::None {
            inner.touch(item, touch);
        }
        Some(inner.node(item).value.clone())
    }

    pub fn set(&self, key: K, value: V, touch: Touch) -> &Self {
        let mut inner = self.inner.borrow_mut();
        if let Some(&item) = inner.map.get(&key) {
            inner.node_mut(item).value = value;
            if touch != Touch::None {
                inner.touch(item, touch);
            }
        } else {
            let item = inner.allocate(Node { key: key.clone(), value, previous: None, next: None });
            match touch {
                Touch::AsOld => inner.add_item_first(item),
                Touch::None | Touch::AsNew => inner.add_item_last(item),
            }
            inner.map.insert(key, item);
            inner.size += 1;
        }
        drop(inner);
        self
    }

    pub fn delete(&self, key: &K) -> bool {
        self.remove(key).is_some()
    }

    pub fn remove(&self, key: &K) -> Option<V> {
        let mut inner = self.inner.borrow_mut();
        let item = inner.map.remove(key)?;
        inner.remove_item(item);
        inner.size -= 1;
        inner.free.push(item);
        inner.nodes[item].take().map(|node| node.value)
    }

    pub fn shift(&self) -> Option<V> {
        let mut inner = self.inner.borrow_mut();
        match (inner.head, inner.tail) {
            (None, None) => return None,
            (Some(_), Some(_)) => {}
            _ => throw_message("Invalid list"),
        }
        let item = inner.head.expect("cabeça definida");
        let key = inner.node(item).key.clone();
        inner.map.remove(&key);
        inner.remove_item(item);
        inner.size -= 1;
        inner.free.push(item);
        inner.nodes[item].take().map(|node| node.value)
    }

    /// `forEach`: lança se o callback modificar o mapa.
    pub fn for_each(&self, mut callback: impl FnMut(&V, &K, &LinkedMap<K, V>)) {
        let state = self.inner.borrow().state;
        let mut current = self.inner.borrow().head;
        while let Some(item) = current {
            let (key, value, next) = {
                let inner = self.inner.borrow();
                let node = inner.node(item);
                (node.key.clone(), node.value.clone(), node.next)
            };
            callback(&value, &key, self);
            if self.inner.borrow().state != state {
                throw_message("LinkedMap got modified during iteration.");
            }
            current = next;
        }
    }

    fn iter_with<T>(&self, project: fn(&K, &V) -> T) -> LinkedIter<'_, K, V, T> {
        let inner = self.inner.borrow();
        LinkedIter { map: self, state: inner.state, current: inner.head, project }
    }

    /// `keys()`: o iterador lança no `next` se o mapa mudou depois de criado.
    pub fn keys(&self) -> LinkedIter<'_, K, V, K> {
        self.iter_with(|key, _| key.clone())
    }

    /// `values()`.
    pub fn values(&self) -> LinkedIter<'_, K, V, V> {
        self.iter_with(|_, value| value.clone())
    }

    /// `entries()` e `[Symbol.iterator]`.
    pub fn entries(&self) -> LinkedIter<'_, K, V, (K, V)> {
        self.iter_with(|key, value| (key.clone(), value.clone()))
    }

    /// `trimOld`: remove os mais antigos até sobrar `new_size`.
    pub(crate) fn trim_old(&self, new_size: i64) {
        self.trim(new_size, true);
    }

    /// `trimNew`: remove os mais novos até sobrar `new_size`.
    pub(crate) fn trim_new(&self, new_size: i64) {
        self.trim(new_size, false);
    }

    fn trim(&self, new_size: i64, from_old: bool) {
        if new_size >= self.size() as i64 {
            return;
        }
        if new_size == 0 {
            self.clear();
            return;
        }
        let mut inner = self.inner.borrow_mut();
        let mut current = if from_old { inner.head } else { inner.tail };
        let mut current_size = inner.size as i64;
        while let Some(item) = current {
            if current_size <= new_size {
                break;
            }
            let node = inner.nodes[item].take().expect("nó vivo");
            inner.map.remove(&node.key);
            inner.free.push(item);
            current = if from_old { node.next } else { node.previous };
            current_size -= 1;
        }
        // Se `new_size` for negativo o laço esvazia a lista; o original deixava a outra ponta apontando
        // para um nó removido, aqui as duas pontas ficam vazias.
        if from_old {
            inner.head = current;
            if current.is_none() {
                inner.tail = None;
            }
        } else {
            inner.tail = current;
            if current.is_none() {
                inner.head = None;
            }
        }
        inner.size = current_size.max(0) as usize;
        if let Some(item) = current {
            let node = inner.node_mut(item);
            if from_old {
                node.previous = None;
            } else {
                node.next = None;
            }
        }
        inner.state += 1;
    }

    pub fn to_json(&self) -> Vec<(K, V)> {
        let mut data = Vec::new();
        self.for_each(|value, key, _| data.push((key.clone(), value.clone())));
        data
    }

    pub fn from_json(&self, data: Vec<(K, V)>) {
        self.clear();
        for (key, value) in data {
            self.set(key, value, Touch::None);
        }
    }
}

/// Iterador de `keys()`/`values()`/`entries()` do [`LinkedMap`].
pub struct LinkedIter<'a, K, V, T> {
    map: &'a LinkedMap<K, V>,
    state: u64,
    current: Option<usize>,
    project: fn(&K, &V) -> T,
}

impl<K, V, T> Iterator for LinkedIter<'_, K, V, T> {
    type Item = T;

    fn next(&mut self) -> Option<T> {
        let inner = self.map.inner.borrow();
        if inner.state != self.state {
            drop(inner);
            throw_message("LinkedMap got modified during iteration.");
        }
        let item = self.current?;
        let node = inner.node(item);
        self.current = node.next;
        Some((self.project)(&node.key, &node.value))
    }
}

// ---- caches LRU e MRU ------------------------------------------------------------------------------

/// `Math.round` do JavaScript (arredonda a metade para cima).
fn js_round(value: f64) -> i64 {
    (value + 0.5).floor() as i64
}

/// A classe abstrata `Cache` do original, com a política de remoção fixada em tempo de compilação:
/// `OLD = true` é o [`LRUCache`] (remove os mais antigos), `OLD = false` o [`MRUCache`] (remove os mais novos).
pub struct Cache<K, V, const OLD: bool> {
    map: LinkedMap<K, V>,
    limit: Cell<usize>,
    ratio: Cell<f64>,
}

pub type LRUCache<K, V> = Cache<K, V, true>;
pub type MRUCache<K, V> = Cache<K, V, false>;

impl<K, V, const OLD: bool> Deref for Cache<K, V, OLD> {
    type Target = LinkedMap<K, V>;

    fn deref(&self) -> &LinkedMap<K, V> {
        &self.map
    }
}

fn clamp_ratio(ratio: f64) -> f64 {
    ratio.clamp(0.0, 1.0)
}

impl<K: Hash + Eq + Clone, V: Clone, const OLD: bool> Cache<K, V, OLD> {
    pub fn new(limit: usize, ratio: f64) -> Self {
        Self { map: LinkedMap::new(), limit: Cell::new(limit), ratio: Cell::new(clamp_ratio(ratio)) }
    }

    pub fn limit(&self) -> usize {
        self.limit.get()
    }

    pub fn set_limit(&self, limit: usize) {
        self.limit.set(limit);
        self.check_trim();
    }

    pub fn ratio(&self) -> f64 {
        self.ratio.get()
    }

    pub fn set_ratio(&self, ratio: f64) {
        self.ratio.set(clamp_ratio(ratio));
        self.check_trim();
    }

    /// `get(key)`: o padrão de um cache é `Touch.AsNew`.
    pub fn get(&self, key: &K) -> Option<V> {
        self.map.get(key, Touch::AsNew)
    }

    /// `peek`: lê sem reordenar.
    pub fn peek(&self, key: &K) -> Option<V> {
        self.map.get(key, Touch::None)
    }

    pub fn set(&self, key: K, value: V) -> &Self {
        if !OLD && self.limit.get() <= self.map.size() && !self.map.has(&key) {
            self.trim(js_round(self.limit.get() as f64 * self.ratio.get()) - 1);
        }

        self.map.set(key, value, Touch::AsNew);
        if OLD {
            self.check_trim();
        }
        self
    }

    fn check_trim(&self) {
        if self.map.size() > self.limit.get() {
            self.trim(js_round(self.limit.get() as f64 * self.ratio.get()));
        }
    }

    fn trim(&self, new_size: i64) {
        if OLD {
            self.map.trim_old(new_size);
        } else {
            self.map.trim_new(new_size);
        }
    }
}

// ---- CounterSet, BidirectionalMap, SetMap ----------------------------------------------------------

pub struct CounterSet<T> {
    map: HashMap<T, usize>,
}

impl<T: Hash + Eq> CounterSet<T> {
    pub fn new() -> Self {
        Self { map: HashMap::new() }
    }

    pub fn add(&mut self, value: T) -> &mut Self {
        *self.map.entry(value).or_insert(0) += 1;
        self
    }

    pub fn delete(&mut self, value: &T) -> bool {
        let Some(counter) = self.map.get_mut(value) else {
            return false;
        };
        *counter -= 1;
        if *counter == 0 {
            self.map.remove(value);
        }
        true
    }

    pub fn has(&self, value: &T) -> bool {
        self.map.contains_key(value)
    }
}

impl<T: Hash + Eq> Default for CounterSet<T> {
    fn default() -> Self {
        Self::new()
    }
}

/// A map that allows access both by keys and values.
/// **NOTE**: values need to be unique.
pub struct BidirectionalMap<K, V> {
    m1: OrderedMap<K, V>,
    m2: HashMap<V, K>,
}

impl<K: Hash + Eq + Clone, V: Hash + Eq + Clone> BidirectionalMap<K, V> {
    pub fn new() -> Self {
        Self { m1: OrderedMap::new(), m2: HashMap::new() }
    }

    pub fn from_entries(entries: impl IntoIterator<Item = (K, V)>) -> Self {
        let mut map = Self::new();
        for (key, value) in entries {
            map.set(key, value);
        }
        map
    }

    pub fn clear(&mut self) {
        self.m1.clear();
        self.m2.clear();
    }

    pub fn set(&mut self, key: K, value: V) {
        if let Some(previous_value) = self.m1.get(&key) {
            self.m2.remove(previous_value);
        }
        self.m1.insert(key.clone(), value.clone());
        self.m2.insert(value, key);
    }

    pub fn get(&self, key: &K) -> Option<&V> {
        self.m1.get(key)
    }

    pub fn get_key(&self, value: &V) -> Option<&K> {
        self.m2.get(value)
    }

    pub fn delete(&mut self, key: &K) -> bool {
        let Some(value) = self.m1.remove(key) else {
            return false;
        };
        self.m2.remove(&value);
        true
    }

    pub fn for_each(&self, mut callback: impl FnMut(&V, &K, &BidirectionalMap<K, V>)) {
        for (key, value) in self.m1.iter() {
            callback(value, key, self);
        }
    }

    pub fn keys(&self) -> impl Iterator<Item = &K> {
        self.m1.keys()
    }

    pub fn values(&self) -> impl Iterator<Item = &V> {
        self.m1.values()
    }
}

impl<K: Hash + Eq + Clone, V: Hash + Eq + Clone> Default for BidirectionalMap<K, V> {
    fn default() -> Self {
        Self::new()
    }
}

pub struct SetMap<K, V> {
    map: OrderedMap<K, OrderedMap<V, ()>>,
}

impl<K: Hash + Eq + Clone, V: Hash + Eq + Clone> SetMap<K, V> {
    pub fn new() -> Self {
        Self { map: OrderedMap::new() }
    }

    pub fn add(&mut self, key: K, value: V) {
        if !self.map.contains_key(&key) {
            self.map.insert(key.clone(), OrderedMap::new());
        }
        self.map.get_mut(&key).expect("acabou de inserir").insert(value, ());
    }

    pub fn delete(&mut self, key: &K, value: &V) {
        let Some(values) = self.map.get_mut(key) else {
            return;
        };

        values.remove(value);

        if values.is_empty() {
            self.map.remove(key);
        }
    }

    pub fn for_each(&self, key: &K, mut callback: impl FnMut(&V)) {
        if let Some(values) = self.map.get(key) {
            values.keys().for_each(&mut callback);
        }
    }

    /// `get`: os valores da chave, na ordem de inserção (vazio se a chave não existe).
    pub fn get(&self, key: &K) -> Vec<&V> {
        self.map.get(key).map(|values| values.keys().collect()).unwrap_or_default()
    }
}

impl<K: Hash + Eq + Clone, V: Hash + Eq + Clone> Default for SetMap<K, V> {
    fn default() -> Self {
        Self::new()
    }
}

/// `mapsStrictEqualIgnoreOrder`.
pub fn maps_strict_equal_ignore_order<K: Hash + Eq + Clone, V: PartialEq>(a: &OrderedMap<K, V>, b: &OrderedMap<K, V>) -> bool {
    if std::ptr::eq(a, b) {
        return true;
    }

    if a.len() != b.len() {
        return false;
    }

    for (key, value) in a.iter() {
        if b.get(key) != Some(value) {
            return false;
        }
    }

    for key in b.keys() {
        if !a.contains_key(key) {
            return false;
        }
    }

    true
}

// ---- NKeyMap ---------------------------------------------------------------------------------------

/// Uma chave de `NKeyMap`: `string | boolean | number`. O número compara como no `Map` do JavaScript
/// (`NaN` igual a `NaN`, `-0` igual a `0`).
#[derive(Clone, Debug)]
pub enum NKey {
    Str(String),
    Bool(bool),
    Num(f64),
}

impl NKey {
    fn number_bits(value: f64) -> u64 {
        if value.is_nan() {
            f64::NAN.to_bits()
        } else if value == 0.0 {
            0
        } else {
            value.to_bits()
        }
    }
}

impl PartialEq for NKey {
    fn eq(&self, other: &Self) -> bool {
        match (self, other) {
            (NKey::Str(a), NKey::Str(b)) => a == b,
            (NKey::Bool(a), NKey::Bool(b)) => a == b,
            (NKey::Num(a), NKey::Num(b)) => Self::number_bits(*a) == Self::number_bits(*b),
            _ => false,
        }
    }
}

impl Eq for NKey {}

impl Hash for NKey {
    fn hash<H: Hasher>(&self, state: &mut H) {
        match self {
            NKey::Str(value) => (0u8, value).hash(state),
            NKey::Bool(value) => (1u8, value).hash(state),
            NKey::Num(value) => (2u8, Self::number_bits(*value)).hash(state),
        }
    }
}

impl From<&str> for NKey {
    fn from(value: &str) -> Self {
        NKey::Str(value.to_string())
    }
}

impl From<bool> for NKey {
    fn from(value: bool) -> Self {
        NKey::Bool(value)
    }
}

impl From<f64> for NKey {
    fn from(value: f64) -> Self {
        NKey::Num(value)
    }
}

impl Display for NKey {
    /// O `${key}` do original.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            NKey::Str(value) => f.write_str(value),
            NKey::Bool(value) => write!(f, "{value}"),
            NKey::Num(value) if value.fract() == 0.0 && value.abs() < 1e21 => write!(f, "{}", *value as i128),
            NKey::Num(value) => write!(f, "{value}"),
        }
    }
}

enum NKeyNode<V> {
    Map(OrderedMap<NKey, NKeyNode<V>>),
    Value(V),
}

/// A map that is addressable with an arbitrary number of keys. This is useful in high performance
/// scenarios where creating a composite key whenever the data is accessed is too expensive.
pub struct NKeyMap<V> {
    data: OrderedMap<NKey, NKeyNode<V>>,
}

impl<V> NKeyMap<V> {
    pub fn new() -> Self {
        Self { data: OrderedMap::new() }
    }

    /// Sets a value on the map. Note that unlike a standard `Map`, the first argument is the value.
    pub fn set(&mut self, value: V, keys: &[NKey]) {
        let Some((last, prefix)) = keys.split_last() else {
            throw_message("NKeyMap#set needs at least one key");
        };
        let mut current = &mut self.data;
        for key in prefix {
            if !current.contains_key(key) {
                current.insert(key.clone(), NKeyNode::Map(OrderedMap::new()));
            }
            let Some(NKeyNode::Map(next)) = current.get_mut(key) else {
                throw_message("NKeyMap: a chave tem menos níveis que as já gravadas");
            };
            current = next;
        }
        current.insert(last.clone(), NKeyNode::Value(value));
    }

    pub fn get(&self, keys: &[NKey]) -> Option<&V> {
        let (last, prefix) = keys.split_last()?;
        let mut current = &self.data;
        for key in prefix {
            let NKeyNode::Map(next) = current.get(key)? else {
                return None;
            };
            current = next;
        }
        match current.get(last)? {
            NKeyNode::Value(value) => Some(value),
            NKeyNode::Map(_) => None,
        }
    }

    /// Remove o item (ou o ramo) no fim de `keys` e poda os mapas pai que ficarem vazios.
    fn delete_path(map: &mut OrderedMap<NKey, NKeyNode<V>>, keys: &[NKey]) -> bool {
        let Some((first, rest)) = keys.split_first() else {
            return false;
        };
        if rest.is_empty() {
            return map.remove(first).is_some();
        }
        let Some(NKeyNode::Map(child)) = map.get_mut(first) else {
            return false;
        };
        let deleted = Self::delete_path(child, rest);
        if deleted && child.is_empty() {
            map.remove(first);
        }
        deleted
    }

    pub fn delete(&mut self, keys: &[NKey]) -> bool {
        Self::delete_path(&mut self.data, keys)
    }

    /// Remove tudo sob o prefixo `keys` (sem chaves, limpa o mapa).
    pub fn delete_all(&mut self, keys: &[NKey]) -> bool {
        if keys.is_empty() {
            let had_data = !self.data.is_empty();
            self.data.clear();
            return had_data;
        }
        Self::delete_path(&mut self.data, keys)
    }

    pub fn clear(&mut self) {
        self.data.clear();
    }

    fn collect_values<'a>(map: &'a OrderedMap<NKey, NKeyNode<V>>, out: &mut Vec<&'a V>) {
        for node in map.values() {
            match node {
                NKeyNode::Map(child) => Self::collect_values(child, out),
                NKeyNode::Value(value) => out.push(value),
            }
        }
    }

    /// Os valores sob o prefixo `keys`, na ordem de inserção.
    pub fn get_all(&self, keys: &[NKey]) -> Vec<&V> {
        let mut current = &self.data;
        for (position, key) in keys.iter().enumerate() {
            match current.get(key) {
                Some(NKeyNode::Map(next)) => current = next,
                Some(NKeyNode::Value(value)) if position == keys.len() - 1 => return vec![value],
                _ => return Vec::new(),
            }
        }
        let mut out = Vec::new();
        Self::collect_values(current, &mut out);
        out
    }

    pub fn values(&self) -> Vec<&V> {
        self.get_all(&[])
    }
}

/// Get a textual representation of the map for debugging purposes (`toString`).
impl<V: Display> Display for NKeyMap<V> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fn print_map<V: Display>(map: &OrderedMap<NKey, NKeyNode<V>>, depth: usize) -> String {
            let mut result = String::new();
            for (key, value) in map.iter() {
                result.push_str(&format!("{}{}: ", "  ".repeat(depth), key));
                match value {
                    NKeyNode::Map(child) => {
                        result.push('\n');
                        result.push_str(&print_map(child, depth + 1));
                    }
                    NKeyNode::Value(value) => result.push_str(&format!("{value}\n")),
                }
            }
            result
        }

        f.write_str(&print_map(&self.data, 0))
    }
}

impl<V> Default for NKeyMap<V> {
    fn default() -> Self {
        Self::new()
    }
}
