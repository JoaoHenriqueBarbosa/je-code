//! Porte de `src/vs/base/common/idGenerator.ts`.

use std::cell::Cell;

/// `IdGenerator`: ids sequenciais com um prefixo (`id#1`, `id#2`...).
pub struct IdGenerator {
    prefix: String,
    last_id: Cell<u64>,
}

impl IdGenerator {
    pub fn new(prefix: impl Into<String>) -> Self {
        Self { prefix: prefix.into(), last_id: Cell::new(0) }
    }

    /// `nextId`.
    pub fn next_id(&self) -> String {
        let id = self.last_id.get() + 1;
        self.last_id.set(id);
        format!("{}{id}", self.prefix)
    }
}

thread_local! {
    static DEFAULT_GENERATOR: IdGenerator = IdGenerator::new("id#");
}

/// `defaultGenerator.nextId()`: o gerador padrão (prefixo `id#`), um por thread.
pub fn default_generator_next_id() -> String {
    DEFAULT_GENERATOR.with(IdGenerator::next_id)
}
