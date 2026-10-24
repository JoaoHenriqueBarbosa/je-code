//! Tradução de `bytecode/PutByIdFlags.h` e `.cpp`.

use std::fmt;

use crate::runtime::ecma_mode::ECMAMode;

/// `class PutByIdFlags`: `m_isDirect` e `m_ecmaMode`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct PutByIdFlags {
    is_direct: bool,
    ecma_mode: ECMAMode,
}

impl PutByIdFlags {
    pub const fn create(ecma_mode: ECMAMode) -> PutByIdFlags {
        PutByIdFlags { is_direct: false, ecma_mode }
    }

    /// Um `put_by_id` direto guarda a propriedade sem verificar se a cadeia de protótipos tem um setter.
    pub const fn create_direct(ecma_mode: ECMAMode) -> PutByIdFlags {
        PutByIdFlags { is_direct: true, ecma_mode }
    }

    pub const fn is_direct(&self) -> bool {
        self.is_direct
    }

    pub const fn ecma_mode(&self) -> ECMAMode {
        self.ecma_mode
    }

    /// `printInternal(PrintStream&, PutByIdFlags)`: o `CommaPrinter` com separador `|`.
    pub fn dump(&self, out: &mut dyn fmt::Write) -> fmt::Result {
        let mut first = true;
        if self.is_direct {
            out.write_str("IsDirect")?;
            first = false;
        }
        if self.ecma_mode.is_strict() {
            if !first {
                out.write_str("|")?;
            }
            out.write_str("Strict")?;
        }
        Ok(())
    }
}
