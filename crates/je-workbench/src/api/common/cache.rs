//! Porte de `src/vs/workbench/api/common/cache.ts`: o cache de objetos que o host de extensões entrega por
//! identificador à thread principal (`$undo(editId)`, `$resolveCodeAction(id)`...).
//!
//! Divergências:
//! - `add(item: readonly T[])` guarda a lista inteira sob um identificador novo e `get(pid, id)` lê o item `id` da lista
//!   `pid` (`undefined` é `None`). `Cache` usa mutabilidade interior (`&self`), porque o host a compartilha por `Rc`.
//! - `enableDebugLogging` é sempre `false` no original, e `logDebugInfo` não faz nada: não é portado.
//! - `T` precisa ser `Clone`: `get` devolve uma cópia do item (os itens do host são `Rc`).

use std::cell::{Cell, RefCell};
use std::collections::HashMap;

/// `Cache<T>`.
pub struct Cache<T> {
    data: RefCell<HashMap<u32, Vec<T>>>,
    id_pool: Cell<u32>,
}

impl<T: Clone> Cache<T> {
    /// `new Cache(id)`: o `id` do original só serve de rótulo do log de depuração, que está desligado.
    pub fn new(_id: &str) -> Self {
        Self { data: RefCell::new(HashMap::new()), id_pool: Cell::new(1) }
    }

    pub fn add(&self, item: Vec<T>) -> u32 {
        let id = self.id_pool.get();
        self.id_pool.set(id + 1);
        self.data.borrow_mut().insert(id, item);
        id
    }

    pub fn get(&self, pid: u32, id: usize) -> Option<T> {
        self.data.borrow().get(&pid).and_then(|items| items.get(id).cloned())
    }

    pub fn delete(&self, id: u32) {
        self.data.borrow_mut().remove(&id);
    }
}
