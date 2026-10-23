//! Porte de `src/vs/base/common/history.ts`: `HistoryNavigator` (histórico de um campo de entrada,
//! com limite) e `HistoryNavigator2` (lista ligada em que o último item é o texto ainda não confirmado).
//!
//! `IHistory` é o contrato do `Set` que o navegador usa; ele é compartilhado (a caixa de busca e o
//! serviço de armazenamento enxergam o mesmo conjunto), então os métodos recebem `&self` e quem
//! implementa guarda o estado com mutabilidade interior. [`HistorySet`] é o `new Set()` padrão.
//!
//! O `_currentPosition` do original testa `!currentElement`, o teste de falsidade do JavaScript (a
//! string vazia conta como "sem elemento"); por isso `T` de [`HistoryNavigator`] é [`Truthy`].

use std::cell::{Cell, RefCell};
use std::hash::Hash;
use std::rc::{Rc, Weak};

use crate::collections::SetWithKey;
use crate::dynamic::Truthy;
use crate::errors::{self, Error};
use crate::event::Event;
use crate::lifecycle::{DisposableRef, IDisposable};
use crate::map::OrderedMap;
use crate::navigator::{ArrayNavigator, INavigator};

/// `IHistory<T>`.
pub trait IHistory<T> {
    fn delete(&self, t: &T) -> bool;
    fn add(&self, t: T);
    fn has(&self, t: &T) -> bool;
    fn clear(&self);
    /// `forEach`: percorre os elementos na ordem de inserção.
    fn for_each(&self, callback: &mut dyn FnMut(&T));
    /// `replace?(t)`: devolve `false` se esta implementação não tem `replace` (o navegador então troca o
    /// conjunto por um novo).
    fn replace(&self, _t: Vec<T>) -> bool {
        false
    }
    /// `onDidChange?`.
    fn on_did_change(&self) -> Option<Event<Vec<String>>> {
        None
    }
}

/// O `Set` do JavaScript como [`IHistory`].
pub struct HistorySet<T>(RefCell<OrderedMap<T, ()>>);

impl<T: Hash + Eq + Clone> HistorySet<T> {
    pub fn new(values: impl IntoIterator<Item = T>) -> Self {
        let mut set = OrderedMap::new();
        for value in values {
            set.insert(value, ());
        }
        Self(RefCell::new(set))
    }
}

impl<T: Hash + Eq + Clone> IHistory<T> for HistorySet<T> {
    fn delete(&self, t: &T) -> bool {
        self.0.borrow_mut().remove(t).is_some()
    }

    fn add(&self, t: T) {
        // `Set#add` de um valor presente não muda a posição.
        let mut set = self.0.borrow_mut();
        if !set.contains_key(&t) {
            set.insert(t, ());
        }
    }

    fn has(&self, t: &T) -> bool {
        self.0.borrow().contains_key(t)
    }

    fn clear(&self) {
        self.0.borrow_mut().clear();
    }

    fn for_each(&self, callback: &mut dyn FnMut(&T)) {
        for key in self.0.borrow().keys() {
            callback(key);
        }
    }
}

struct NavigatorState<T> {
    history: Rc<dyn IHistory<T>>,
    navigator: ArrayNavigator<T>,
}

pub struct HistoryNavigator<T: Hash + Eq + Clone + Truthy + 'static> {
    limit: usize,
    state: RefCell<NavigatorState<T>>,
    disposable: RefCell<Option<DisposableRef>>,
}

impl<T: Hash + Eq + Clone + Truthy + 'static> HistoryNavigator<T> {
    /// `new HistoryNavigator(history = new Set(), limit = 10)`.
    pub fn new(history: Option<Rc<dyn IHistory<T>>>, limit: usize) -> Rc<Self> {
        let history = history.unwrap_or_else(|| Rc::new(HistorySet::new([])));
        let this = Rc::new(Self {
            limit,
            state: RefCell::new(NavigatorState { navigator: ArrayNavigator::new(Vec::new(), None, None, None), history: history.clone() }),
            disposable: RefCell::new(None),
        });
        this.on_change();
        if let Some(on_did_change) = history.on_did_change() {
            let weak: Weak<Self> = Rc::downgrade(&this);
            let listener = on_did_change.listen(move |_| {
                if let Some(this) = weak.upgrade() {
                    this.on_change();
                }
            });
            *this.disposable.borrow_mut() = Some(listener);
        }
        this
    }

    /// `new HistoryNavigator(new Set(values), limit)`.
    pub fn from_values(values: impl IntoIterator<Item = T>, limit: usize) -> Rc<Self> {
        Self::new(Some(Rc::new(HistorySet::new(values))), limit)
    }

    pub fn get_history(&self) -> Vec<T> {
        self.elements()
    }

    pub fn add(&self, t: T) {
        let history = self.history();
        history.delete(&t);
        history.add(t);
        self.on_change();
    }

    pub fn is_first(&self) -> bool {
        self.current_position() == 0
    }

    pub fn is_last(&self) -> bool {
        self.current_position() >= self.elements().len() as i64 - 1
    }

    pub fn is_nowhere(&self) -> bool {
        self.state.borrow().navigator.current().is_none()
    }

    pub fn has(&self, t: &T) -> bool {
        self.history().has(t)
    }

    pub fn clear(&self) {
        self.history().clear();
        self.on_change();
    }

    fn history(&self) -> Rc<dyn IHistory<T>> {
        self.state.borrow().history.clone()
    }

    fn on_change(&self) {
        self.reduce_to_limit();
        let elements = self.elements();
        let length = elements.len() as i64;
        self.state.borrow_mut().navigator = ArrayNavigator::new(elements, Some(0), Some(length), Some(length));
    }

    fn reduce_to_limit(&self) {
        let data = self.elements();
        if data.len() > self.limit {
            let replace_value = data[data.len() - self.limit..].to_vec();
            let history = self.history();
            if !history.replace(replace_value.clone()) {
                self.state.borrow_mut().history = Rc::new(HistorySet::new(replace_value));
            }
        }
    }

    fn current_position(&self) -> i64 {
        let current = self.state.borrow().navigator.current();
        match current {
            Some(element) if element.is_truthy() => {
                self.elements().iter().position(|e| *e == element).map_or(-1, |i| i as i64)
            }
            _ => -1,
        }
    }

    fn elements(&self) -> Vec<T> {
        let mut elements = Vec::new();
        self.history().for_each(&mut |e| elements.push(e.clone()));
        elements
    }
}

impl<T: Hash + Eq + Clone + Truthy + 'static> INavigator<T> for HistoryNavigator<T> {
    fn next(&self) -> Option<T> {
        // This will navigate past the end of the last element, and in that case the input should be cleared
        self.state.borrow().navigator.next()
    }

    fn previous(&self) -> Option<T> {
        if self.current_position() != 0 {
            return self.state.borrow().navigator.previous();
        }
        None
    }

    fn current(&self) -> Option<T> {
        self.state.borrow().navigator.current()
    }

    fn first(&self) -> Option<T> {
        self.state.borrow().navigator.first()
    }

    fn last(&self) -> Option<T> {
        self.state.borrow().navigator.last()
    }
}

impl<T: Hash + Eq + Clone + Truthy + 'static> IDisposable for HistoryNavigator<T> {
    fn dispose(&self) {
        if let Some(disposable) = self.disposable.borrow_mut().take() {
            disposable.dispose();
        }
    }
}

struct HistoryNode<T> {
    value: T,
    previous: Option<usize>,
    next: Option<usize>,
}

/// The right way to use HistoryNavigator2 is for the last item in the list to be the user's uncommitted
/// current text. eg empty string, or whatever has been typed. Then the user can navigate away from the
/// last item through the list, and back to it. When updating the last item, call replaceLast.
///
/// Os nós ficam num vetor, ligados por índice; um nó removido libera o espaço para o próximo `add`.
/// `K` é o tipo da chave de identidade (`identityFn`), que o original deixa `unknown` e compara com `===`.
pub struct HistoryNavigator2<T: Clone + 'static, K: Hash + Eq + Clone + 'static> {
    nodes: Vec<Option<HistoryNode<T>>>,
    free: Vec<usize>,
    value_set: SetWithKey<T, K>,
    head: usize,
    tail: usize,
    cursor: Cell<usize>,
    size: usize,
    capacity: usize,
    identity_fn: Rc<dyn Fn(&T) -> K>,
}

impl<T: Clone + Hash + Eq + 'static> HistoryNavigator2<T, T> {
    /// `new HistoryNavigator2(history)`: capacidade 10 e a identidade do próprio valor.
    pub fn with_defaults(history: &[T]) -> Self {
        Self::new(history, 10, |t| t.clone())
    }
}

impl<T: Clone + 'static, K: Hash + Eq + Clone + 'static> HistoryNavigator2<T, K> {
    pub fn new(history: &[T], capacity: usize, identity_fn: impl Fn(&T) -> K + 'static) -> Self {
        if history.is_empty() {
            errors::throw(Error::new("not supported"));
        }

        let identity_fn: Rc<dyn Fn(&T) -> K> = Rc::new(identity_fn);
        let key_fn = identity_fn.clone();
        let mut this = Self {
            nodes: vec![Some(HistoryNode { value: history[0].clone(), previous: None, next: None })],
            free: Vec::new(),
            value_set: SetWithKey::new(vec![history[0].clone()], move |t| key_fn(t)),
            head: 0,
            tail: 0,
            cursor: Cell::new(0),
            size: 1,
            capacity,
            identity_fn,
        };
        for value in &history[1..] {
            this.add(value.clone());
        }
        this
    }

    pub fn size(&self) -> usize {
        self.size
    }

    fn node(&self, index: usize) -> &HistoryNode<T> {
        self.nodes[index].as_ref().expect("índice de nó vivo")
    }

    fn node_mut(&mut self, index: usize) -> &mut HistoryNode<T> {
        self.nodes[index].as_mut().expect("índice de nó vivo")
    }

    fn alloc(&mut self, node: HistoryNode<T>) -> usize {
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

    pub fn add(&mut self, value: T) {
        let index = self.alloc(HistoryNode { value: value.clone(), previous: Some(self.tail), next: None });

        let tail = self.tail;
        self.node_mut(tail).next = Some(index);
        self.tail = index;
        self.cursor.set(self.tail);
        self.size += 1;

        if self.value_set.has(&value) {
            self.delete_from_list(&value);
        } else {
            self.value_set.add(value);
        }

        while self.size > self.capacity {
            let head_value = self.node(self.head).value.clone();
            self.value_set.delete(&head_value);

            let old_head = self.head;
            self.head = self.node(old_head).next.expect("a cabeça não é a cauda enquanto o tamanho excede a capacidade");
            let head = self.head;
            self.node_mut(head).previous = None;
            self.nodes[old_head] = None;
            self.free.push(old_head);
            self.size -= 1;
        }
    }

    /// Devolve o valor antigo do último item.
    pub fn replace_last(&mut self, value: T) -> T {
        if (self.identity_fn)(&self.node(self.tail).value) == (self.identity_fn)(&value) {
            return value;
        }

        let old_value = self.node(self.tail).value.clone();
        self.value_set.delete(&old_value);
        let tail = self.tail;
        self.node_mut(tail).value = value.clone();

        if self.value_set.has(&value) {
            self.delete_from_list(&value);
        } else {
            self.value_set.add(value);
        }

        old_value
    }

    pub fn prepend(&mut self, value: T) {
        if self.size == self.capacity || self.value_set.has(&value) {
            return;
        }

        let index = self.alloc(HistoryNode { value: value.clone(), previous: None, next: Some(self.head) });

        let head = self.head;
        self.node_mut(head).previous = Some(index);
        self.head = index;
        self.size += 1;

        self.value_set.add(value);
    }

    pub fn is_at_end(&self) -> bool {
        self.cursor.get() == self.tail
    }

    pub fn current(&self) -> T {
        self.node(self.cursor.get()).value.clone()
    }

    pub fn previous(&self) -> T {
        if let Some(previous) = self.node(self.cursor.get()).previous {
            self.cursor.set(previous);
        }
        self.current()
    }

    pub fn next(&self) -> T {
        if let Some(next) = self.node(self.cursor.get()).next {
            self.cursor.set(next);
        }
        self.current()
    }

    pub fn has(&self, t: &T) -> bool {
        self.value_set.has(t)
    }

    pub fn reset_cursor(&self) -> T {
        self.cursor.set(self.tail);
        self.current()
    }

    /// `[Symbol.iterator]`: da cabeça à cauda.
    pub fn iter(&self) -> impl Iterator<Item = &T> {
        let mut node = Some(self.head);
        std::iter::from_fn(move || {
            let current = self.node(node?);
            node = current.next;
            Some(&current.value)
        })
    }

    fn delete_from_list(&mut self, value: &T) {
        let mut temp = self.head;

        let value_key = (self.identity_fn)(value);
        while temp != self.tail {
            let next = self.node(temp).next.expect("antes da cauda sempre há próximo");
            if (self.identity_fn)(&self.node(temp).value) == value_key {
                if temp == self.head {
                    self.head = next;
                    let head = self.head;
                    self.node_mut(head).previous = None;
                } else {
                    let previous = self.node(temp).previous.expect("fora da cabeça sempre há anterior");
                    self.node_mut(previous).next = Some(next);
                    self.node_mut(next).previous = Some(previous);
                }

                self.size -= 1;
                // No original o cursor pode ficar num nó já fora da lista (só em `replace_last`, já que
                // `add` põe o cursor na cauda). Aqui o nó é liberado, então o cursor passa ao seguinte.
                if self.cursor.get() == temp {
                    self.cursor.set(next);
                }
                self.nodes[temp] = None;
                self.free.push(temp);
            }

            temp = next;
        }
    }
}
