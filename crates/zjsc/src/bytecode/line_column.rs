//! Porte de `bytecode/LineColumn.h`.

/// `struct LineColumn { unsigned line; unsigned column; }`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct LineColumn {
    pub line: u32,
    pub column: u32,
}
