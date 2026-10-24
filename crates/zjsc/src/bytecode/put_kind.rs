//! Tradução de `runtime/PrivateFieldPutKind.h` e `.cpp` (o mapa de nomes do bytecompiler o põe em
//! `crate::bytecode::put_kind`).

use std::fmt;

/// `struct PrivateFieldPutKind`: um byte, `None = 0`, `Set = 1`, `Define = 2`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct PrivateFieldPutKind {
    value: u8,
}

impl PrivateFieldPutKind {
    pub const NONE: u8 = 0;
    pub const SET: u8 = 1;
    pub const DEFINE: u8 = 2;

    /// O `ASSERT` do construtor é invariante: o byte vem do bytecode.
    const fn new(value: u8) -> PrivateFieldPutKind {
        assert!(value == Self::NONE || value == Self::SET || value == Self::DEFINE);
        PrivateFieldPutKind { value }
    }

    pub const fn from_byte(byte: u8) -> PrivateFieldPutKind {
        Self::new(byte)
    }

    pub const fn none() -> PrivateFieldPutKind {
        Self::new(Self::NONE)
    }

    pub const fn set() -> PrivateFieldPutKind {
        Self::new(Self::SET)
    }

    pub const fn define() -> PrivateFieldPutKind {
        Self::new(Self::DEFINE)
    }

    pub const fn is_none(&self) -> bool {
        self.value == Self::NONE
    }

    pub const fn is_set(&self) -> bool {
        self.value == Self::SET
    }

    pub const fn is_define(&self) -> bool {
        self.value == Self::DEFINE
    }

    pub const fn value(&self) -> u8 {
        self.value
    }

    /// `PrivateFieldPutKind::dump(PrintStream&)`.
    pub fn dump(&self, out: &mut dyn fmt::Write) -> fmt::Result {
        if self.is_set() {
            out.write_str("Set")
        } else if self.is_define() {
            out.write_str("Define")
        } else {
            out.write_str("None")
        }
    }
}
