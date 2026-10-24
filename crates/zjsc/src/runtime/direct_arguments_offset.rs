//! Porte de `runtime/DirectArgumentsOffset.h`.
//!
//! Deslocamento dentro do objeto especial de argumentos que captura os argumentos de uma função.
//! `DirectArgumentsOffset::dump` só serve à depuração e não se porta.

use crate::runtime::generic_offset::GenericOffset;

/// Marca de tipo de `DirectArgumentsOffset`.
#[derive(Clone, Copy, Debug)]
pub struct DirectArgumentsOffsetKind;

pub type DirectArgumentsOffset = GenericOffset<DirectArgumentsOffsetKind>;
