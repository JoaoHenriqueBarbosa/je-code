//! Tradução de `parser/SourceTaintedOrigin.{h,cpp}`.
//!
//! Fora desta fatia, e por quê: `sourceTaintedOriginFromStack` e `computeNewSourceTaintedOriginFromStack`
//! dependem de `VM`, `CallFrame`, `StackVisitor`, `CodeBlock` e `JSWebAssemblyInstance`, das camadas
//! seguintes. `TriState` do WTF vira o `enum TriState` abaixo, com os mesmos valores.

use crate::wtf::text::wtf_string::String as WtfString;

/// `enum class SourceTaintedOrigin`. A ordem importa: o C++ compara com `std::max` e `>=`.
#[repr(u8)]
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum SourceTaintedOrigin {
    Untainted = 0,
    /// Means the VM saw some tainted code this event loop turn but no such code was on the stack when
    /// this source was created.
    IndirectlyTaintedByHistory = 1,
    IndirectlyTainted = 2,
    KnownTainted = 3,
}

/// `WTF::TriState` (definido em `crate::wtf::tri_state`; reexportado para os chamadores atuais).
pub use crate::wtf::tri_state::TriState;

/// `taintednessToTriState(SourceTaintedOrigin)`.
pub fn taintedness_to_tri_state(origin: SourceTaintedOrigin) -> TriState {
    if origin == SourceTaintedOrigin::Untainted {
        return TriState::False;
    }
    if origin == SourceTaintedOrigin::KnownTainted {
        return TriState::True;
    }
    TriState::Indeterminate
}

/// `sourceTaintedOriginToString(SourceTaintedOrigin)`.
pub fn source_tainted_origin_to_string(taintedness: SourceTaintedOrigin) -> WtfString {
    let name: &[u8] = match taintedness {
        SourceTaintedOrigin::Untainted => b"Untainted",
        SourceTaintedOrigin::KnownTainted => b"KnownTainted",
        SourceTaintedOrigin::IndirectlyTainted => b"IndirectlyTainted",
        SourceTaintedOrigin::IndirectlyTaintedByHistory => b"IndirectlyTaintedByHistory",
    };
    WtfString::from_latin1(name)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tri_state() {
        assert_eq!(taintedness_to_tri_state(SourceTaintedOrigin::Untainted), TriState::False);
        assert_eq!(taintedness_to_tri_state(SourceTaintedOrigin::KnownTainted), TriState::True);
        assert_eq!(taintedness_to_tri_state(SourceTaintedOrigin::IndirectlyTainted), TriState::Indeterminate);
        assert!(SourceTaintedOrigin::IndirectlyTainted >= SourceTaintedOrigin::IndirectlyTaintedByHistory);
    }

    #[test]
    fn to_string() {
        assert_eq!(source_tainted_origin_to_string(SourceTaintedOrigin::KnownTainted), WtfString::from_latin1(b"KnownTainted"));
    }
}
