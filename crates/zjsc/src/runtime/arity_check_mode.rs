//! Tradução de `runtime/ArityCheckMode.h`.

/// `enum class ArityCheckMode : uint8_t { ArityCheckNotRequired, MustCheckArity }`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[repr(u8)]
pub enum ArityCheckMode {
    ArityCheckNotRequired,
    MustCheckArity,
}
