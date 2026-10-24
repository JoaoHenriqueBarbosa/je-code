//! Tradução de `wasm/WasmAddressType.h` e `.cpp`: o tipo de endereço de uma memória ou tabela, 32
//! ou 64 bits. Fica de fora o que converte de e para `B3::Type` (só o JIT usa).

use crate::wasm::wasm_format::{Type, TypeIndex, TypeKind};

/// `AddressType` (`Kind::I32` é o padrão).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum AddressType {
    #[default]
    I32,
    I64,
}

impl AddressType {
    /// `AddressType(bool is64Bit)`.
    pub const fn new(is_64_bit: bool) -> AddressType {
        if is_64_bit { AddressType::I64 } else { AddressType::I32 }
    }

    /// `AddressType(TypeKind)`: só `i32` e `i64` valem (`RELEASE_ASSERT_NOT_REACHED` no resto).
    pub fn from_type_kind(kind: TypeKind) -> AddressType {
        match kind {
            TypeKind::I32 => AddressType::I32,
            TypeKind::I64 => AddressType::I64,
            // Invariante: o parser só chama com i32 ou i64 (RELEASE_ASSERT_NOT_REACHED do C++).
            _ => panic!("Invalid Wasm Type to AddressType conversion"),
        }
    }

    pub const fn is_64_bit(self) -> bool {
        matches!(self, AddressType::I64)
    }

    /// `asWasmTypeKind`.
    pub fn as_wasm_type_kind(self) -> TypeKind {
        match self {
            AddressType::I32 => TypeKind::I32,
            AddressType::I64 => TypeKind::I64,
        }
    }

    /// `asWasmType`.
    pub fn as_wasm_type(self) -> Type {
        Type::new(self.as_wasm_type_kind(), TypeIndex::Invalid)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn address_type_round_trips_through_the_type_kind() {
        assert_eq!(AddressType::default(), AddressType::I32);
        assert!(!AddressType::new(false).is_64_bit());
        assert!(AddressType::new(true).is_64_bit());
        assert_eq!(AddressType::from_type_kind(TypeKind::I64), AddressType::I64);
        assert_eq!(AddressType::I32.as_wasm_type(), Type::new(TypeKind::I32, TypeIndex::Invalid));
        assert_eq!(AddressType::I64.as_wasm_type_kind(), TypeKind::I64);
    }
}
