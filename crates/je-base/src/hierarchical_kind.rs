//! Porte de `src/vs/base/common/hierarchicalKind.ts`: um tipo hierárquico separado por pontos
//! (`refactor.extract.function`), em que um pai contém os filhos.

/// `HierarchicalKind`.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct HierarchicalKind {
    pub value: String,
}

impl HierarchicalKind {
    pub const SEP: &'static str = ".";

    pub fn new(value: impl Into<String>) -> Self {
        Self { value: value.into() }
    }

    /// `HierarchicalKind.None`: tipo especial que não casa com nada.
    pub fn none() -> Self {
        Self::new("@@none@@")
    }

    /// `HierarchicalKind.Empty`.
    pub fn empty() -> Self {
        Self::new("")
    }

    pub fn equals(&self, other: &Self) -> bool {
        self.value == other.value
    }

    pub fn contains(&self, other: &Self) -> bool {
        self.equals(other) || self.value.is_empty() || other.value.starts_with(&format!("{}{}", self.value, Self::SEP))
    }

    pub fn intersects(&self, other: &Self) -> bool {
        self.contains(other) || other.contains(self)
    }

    /// `append(...parts)`.
    pub fn append(&self, parts: &[&str]) -> Self {
        let mut all: Vec<&str> = Vec::with_capacity(parts.len() + 1);
        if !self.value.is_empty() {
            all.push(&self.value);
        }
        all.extend_from_slice(parts);
        Self::new(all.join(Self::SEP))
    }
}
