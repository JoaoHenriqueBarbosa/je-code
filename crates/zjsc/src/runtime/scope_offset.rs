//! Porte de `runtime/ScopeOffset.h`.
//!
//! Deslocamento dentro de um escopo (ativação ou objeto global). `ScopeOffset::dump` só serve à
//! depuração e não se porta.

use crate::runtime::generic_offset::GenericOffset;

/// Marca de tipo de `ScopeOffset`.
#[derive(Clone, Copy, Debug)]
pub struct ScopeOffsetKind;

pub type ScopeOffset = GenericOffset<ScopeOffsetKind>;
