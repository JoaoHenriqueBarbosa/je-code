//! Porte de `src/vs/base/browser/ui/list/splice.ts`: o contrato de quem recebe um `splice` e o
//! combinador que repassa o mesmo `splice` a vários.

/// `ISpliceable<T>`.
pub trait ISpliceable<T> {
    fn splice(&self, start: usize, delete_count: usize, elements: &[T]);
}

/// `CombinedSpliceable<T>`.
pub struct CombinedSpliceable<T> {
    spliceables: Vec<std::rc::Rc<dyn ISpliceable<T>>>,
}

impl<T> CombinedSpliceable<T> {
    pub fn new(spliceables: Vec<std::rc::Rc<dyn ISpliceable<T>>>) -> Self {
        Self { spliceables }
    }
}

impl<T> ISpliceable<T> for CombinedSpliceable<T> {
    fn splice(&self, start: usize, delete_count: usize, elements: &[T]) {
        for s in &self.spliceables {
            s.splice(start, delete_count, elements);
        }
    }
}
