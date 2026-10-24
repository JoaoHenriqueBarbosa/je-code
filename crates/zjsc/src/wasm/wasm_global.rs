//! Tradução de `wasm/WasmGlobal.h` e `.cpp`: o valor de uma global, compartilhável entre instâncias
//! (uma global mutável importada é o mesmo `Global` nas duas). Sem o `JSWebAssemblyGlobal` nem os
//! `visitAggregate` do GC. O valor é um `u64` (i32 com os 32 bits altos zerados, f32 e f64 em bits,
//! referência codificada); `v128` guarda os 16 bytes a parte.

use crate::wasm::wasm_format::{Mutability, Type, V128};

/// `Global`.
#[derive(Debug)]
pub struct Global {
    ty: Type,
    mutability: Mutability,
    bits: u64,
    vector: V128,
}

impl Global {
    pub fn new(ty: Type, mutability: Mutability, bits: u64) -> Global {
        Global { ty, mutability, bits, vector: [0; 16] }
    }

    pub fn new_vector(ty: Type, mutability: Mutability, vector: V128) -> Global {
        Global { ty, mutability, bits: 0, vector }
    }

    pub fn ty(&self) -> Type {
        self.ty
    }

    pub fn mutability(&self) -> Mutability {
        self.mutability
    }

    /// `get`.
    pub fn get(&self) -> u64 {
        self.bits
    }

    /// `set`.
    pub fn set(&mut self, bits: u64) {
        self.bits = bits;
    }

    pub fn get_vector(&self) -> V128 {
        self.vector
    }

    pub fn set_vector(&mut self, vector: V128) {
        self.vector = vector;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::wasm::wasm_format::TYPE_I32;

    #[test]
    fn set_and_get() {
        let mut global = Global::new(TYPE_I32, Mutability::Mutable, 5);
        assert_eq!(global.get(), 5);
        global.set(9);
        assert_eq!(global.get(), 9);
        assert_eq!(global.mutability(), Mutability::Mutable);
    }
}
