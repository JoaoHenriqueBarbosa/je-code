//! Tradução de `runtime/SymbolTableOrScopeDepth.h`.

use std::fmt;

use crate::bytecode::virtual_register::{VirtualRegister, FIRST_CONSTANT_REGISTER_INDEX};

/// `class SymbolTableOrScopeDepth`: um `unsigned` (`m_raw`) que guarda o índice da constante da
/// tabela de símbolos (relativo a `FirstConstantRegisterIndex`) ou a profundidade de escopo.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct SymbolTableOrScopeDepth {
    raw: u32,
}

impl SymbolTableOrScopeDepth {
    pub fn symbol_table(reg: VirtualRegister) -> SymbolTableOrScopeDepth {
        assert!(reg.is_constant());
        SymbolTableOrScopeDepth { raw: (reg.offset() - FIRST_CONSTANT_REGISTER_INDEX) as u32 }
    }

    pub const fn scope_depth(scope_depth: u32) -> SymbolTableOrScopeDepth {
        SymbolTableOrScopeDepth { raw: scope_depth }
    }

    pub const fn raw(value: u32) -> SymbolTableOrScopeDepth {
        SymbolTableOrScopeDepth { raw: value }
    }

    /// `symbolTable()` (o acessor): o `VirtualRegister` da constante.
    pub fn symbol_table_register(&self) -> VirtualRegister {
        VirtualRegister::new((self.raw as i32).wrapping_add(FIRST_CONSTANT_REGISTER_INDEX))
    }

    /// `scopeDepth()` (o acessor).
    pub const fn scope_depth_value(&self) -> u32 {
        self.raw
    }

    /// `raw()` (o acessor).
    pub const fn raw_value(&self) -> u32 {
        self.raw
    }

    pub fn dump(&self, out: &mut dyn fmt::Write) -> fmt::Result {
        write!(out, "{}", self.raw)
    }
}
