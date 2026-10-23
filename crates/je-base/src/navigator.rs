//! Porte de `src/vs/base/common/navigator.ts`: `INavigator` e o `ArrayNavigator`.
//!
//! O navegador guarda uma posição, e os métodos recebem `&self` porque a posição fica numa `Cell`
//! (o mesmo que o objeto mutável do original; quem usa o navegador pelo `Rc` não precisa de `&mut`).

use std::cell::Cell;

/// `INavigator<T>`: `null` do original é `None`.
pub trait INavigator<T> {
    fn current(&self) -> Option<T>;
    fn previous(&self) -> Option<T>;
    fn first(&self) -> Option<T>;
    fn last(&self) -> Option<T>;
    fn next(&self) -> Option<T>;
}

pub struct ArrayNavigator<T> {
    items: Vec<T>,
    start: i64,
    end: i64,
    index: Cell<i64>,
}

impl<T: Clone> ArrayNavigator<T> {
    /// `new ArrayNavigator(items, start = 0, end = items.length, index = start - 1)`.
    pub fn new(items: Vec<T>, start: Option<i64>, end: Option<i64>, index: Option<i64>) -> Self {
        let start = start.unwrap_or(0);
        let end = end.unwrap_or(items.len() as i64);
        let index = index.unwrap_or(start - 1);
        Self { items, start, end, index: Cell::new(index) }
    }
}

impl<T: Clone> INavigator<T> for ArrayNavigator<T> {
    fn current(&self) -> Option<T> {
        let index = self.index.get();
        if index == self.start - 1 || index == self.end {
            return None;
        }
        usize::try_from(index).ok().and_then(|i| self.items.get(i)).cloned()
    }

    fn next(&self) -> Option<T> {
        self.index.set((self.index.get() + 1).min(self.end));
        self.current()
    }

    fn previous(&self) -> Option<T> {
        self.index.set((self.index.get() - 1).max(self.start - 1));
        self.current()
    }

    fn first(&self) -> Option<T> {
        self.index.set(self.start);
        self.current()
    }

    fn last(&self) -> Option<T> {
        self.index.set(self.end - 1);
        self.current()
    }
}
