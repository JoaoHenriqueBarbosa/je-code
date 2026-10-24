//! Tradução de `runtime/IterationKind.h`.

/// `enum class IterationKind : uint32_t`.
#[repr(u32)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum IterationKind {
    Keys = 0,
    Values = 1,
    Entries = 2,
}
