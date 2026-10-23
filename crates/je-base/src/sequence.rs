//! Porte de `src/vs/base/common/sequence.ts`: uma lista que avisa cada `splice`.

use std::cell::RefCell;
use std::rc::Rc;

use crate::event::{Emitter, Event};

/// `ISplice<T>`.
#[derive(Clone, Debug, PartialEq)]
pub struct Splice<T> {
    pub start: usize,
    pub delete_count: usize,
    pub to_insert: Vec<T>,
}

/// `ISpliceable<T>`.
pub trait Spliceable<T> {
    fn splice(&self, start: usize, delete_count: usize, to_insert: Vec<T>);
}

/// `Sequence<T>`.
pub struct Sequence<T: Clone + 'static> {
    pub elements: RefCell<Vec<T>>,
    on_did_splice_emitter: Rc<Emitter<Splice<T>>>,
}

impl<T: Clone + 'static> Default for Sequence<T> {
    fn default() -> Self {
        Self::new()
    }
}

impl<T: Clone + 'static> Sequence<T> {
    pub fn new() -> Self {
        Self { elements: RefCell::new(Vec::new()), on_did_splice_emitter: Emitter::new() }
    }

    /// `onDidSplice`.
    pub fn on_did_splice(&self) -> Event<Splice<T>> {
        self.on_did_splice_emitter.event()
    }
}

impl<T: Clone + 'static> Spliceable<T> for Sequence<T> {
    /// `splice`: como `Array#splice`, `start` e `delete_count` são limitados ao tamanho da lista.
    fn splice(&self, start: usize, delete_count: usize, to_insert: Vec<T>) {
        {
            let mut elements = self.elements.borrow_mut();
            let start = start.min(elements.len());
            let end = start.saturating_add(delete_count).min(elements.len());
            elements.splice(start..end, to_insert.iter().cloned());
        }
        self.on_did_splice_emitter.fire(Splice { start, delete_count, to_insert });
    }
}
