//! Tradução de `wasm/WasmMemoryInformation.h` e `.cpp` (sem o `PinnedSizeRegisterInfo`, que é do
//! JIT): o que o módulo declara de uma memória.

use crate::wasm::page_count::PageCount;
use crate::wasm::wasm_address_type::AddressType;

/// `MemoryInformation`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MemoryInformation {
    pub initial: PageCount,
    /// `PageCount()` (sem valor) quando o módulo não declara máximo.
    pub maximum: PageCount,
    pub is_shared: bool,
    pub is_import: bool,
    pub address_type: AddressType,
}

impl MemoryInformation {
    pub fn new(initial: PageCount, maximum: PageCount, is_shared: bool, is_import: bool, is_memory64: bool) -> MemoryInformation {
        assert!(initial.has_value());
        assert!(!maximum.has_value() || maximum >= initial);
        MemoryInformation { initial, maximum, is_shared, is_import, address_type: AddressType::new(is_memory64) }
    }

    pub fn is_memory64(&self) -> bool {
        self.address_type.is_64_bit()
    }

    /// `doesAccessOverflow`: a soma `first + second` estoura o tipo do endereço. Num endereço de 32
    /// bits, `sumOverflows<uint32_t>` também recusa um operando que não cabe em 32 bits.
    pub fn does_access_overflow(&self, first: u64, second: u64) -> bool {
        if self.is_memory64() {
            return first.checked_add(second).is_none();
        }
        let sum = u32::try_from(first).ok().and_then(|first| first.checked_add(u32::try_from(second).ok()?));
        sum.is_none()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn access_overflow_depends_on_the_address_width() {
        let memory32 = MemoryInformation::new(PageCount::new(1), PageCount::default(), false, false, false);
        assert!(!memory32.does_access_overflow(1, 2));
        assert!(memory32.does_access_overflow(u64::from(u32::MAX), 1));
        assert!(memory32.does_access_overflow(1 << 32, 0));
        let memory64 = MemoryInformation::new(PageCount::new(1), PageCount::new(4), true, false, true);
        assert!(!memory64.does_access_overflow(1 << 32, 1));
        assert!(memory64.does_access_overflow(u64::MAX, 1));
        assert!(memory64.is_memory64());
    }
}
