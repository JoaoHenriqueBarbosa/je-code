//! Tradução de `runtime/ECMAMode.h` e `.cpp`.

use std::fmt;

/// `struct ECMAMode`: um byte, `StrictMode = 0` e `SloppyMode = 1`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct ECMAMode {
    value: u8,
}

impl ECMAMode {
    pub const STRICT_MODE: u8 = 0;
    pub const SLOPPY_MODE: u8 = 1;

    /// `fromByte`. O `ASSERT` do construtor é invariante: o byte vem do bytecode.
    pub const fn from_byte(byte: u8) -> ECMAMode {
        assert!(byte == Self::STRICT_MODE || byte == Self::SLOPPY_MODE);
        ECMAMode { value: byte }
    }

    /// `fromBool`.
    pub const fn from_bool(is_strict: bool) -> ECMAMode {
        if is_strict {
            Self::strict()
        } else {
            Self::sloppy()
        }
    }

    pub const fn strict() -> ECMAMode {
        ECMAMode { value: Self::STRICT_MODE }
    }

    pub const fn sloppy() -> ECMAMode {
        ECMAMode { value: Self::SLOPPY_MODE }
    }

    pub const fn is_strict(&self) -> bool {
        self.value == Self::STRICT_MODE
    }

    pub const fn value(&self) -> u8 {
        self.value
    }

    /// `ECMAMode::dump(PrintStream&)`: o `PrintStream` vira `fmt::Write`.
    pub fn dump(&self, out: &mut dyn fmt::Write) -> fmt::Result {
        if self.is_strict() {
            out.write_str("StrictMode")
        } else {
            out.write_str("NotStrictMode")
        }
    }
}
