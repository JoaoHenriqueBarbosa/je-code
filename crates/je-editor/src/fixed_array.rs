//! Porte de `src/vs/editor/common/model/fixedArray.ts`: um array que devolve o padrão além do fim.

/// `FixedArray<T>`.
pub struct FixedArray<T: Clone> {
    store: Vec<T>,
    default: T,
}

impl<T: Clone> FixedArray<T> {
    pub fn new(default: T) -> Self {
        Self { store: Vec::new(), default }
    }

    pub fn get(&self, index: usize) -> T {
        self.store.get(index).cloned().unwrap_or_else(|| self.default.clone())
    }

    pub fn set(&mut self, index: usize, value: T) {
        if index >= self.store.len() {
            self.store.resize(index + 1, self.default.clone());
        }
        self.store[index] = value;
    }

    pub fn replace(&mut self, index: usize, old_length: usize, new_length: usize) {
        if index >= self.store.len() {
            return;
        }
        if old_length == 0 {
            self.insert(index, new_length);
            return;
        } else if new_length == 0 {
            self.delete(index, old_length);
            return;
        }
        let end = (index + old_length).min(self.store.len());
        self.store.splice(index..end, std::iter::repeat_n(self.default.clone(), new_length));
    }

    pub fn delete(&mut self, delete_index: usize, delete_count: usize) {
        if delete_count == 0 || delete_index >= self.store.len() {
            return;
        }
        let end = (delete_index + delete_count).min(self.store.len());
        self.store.drain(delete_index..end);
    }

    pub fn insert(&mut self, insert_index: usize, insert_count: usize) {
        if insert_count == 0 || insert_index >= self.store.len() {
            return;
        }
        self.store.splice(insert_index..insert_index, std::iter::repeat_n(self.default.clone(), insert_count));
    }
}
