//! Tradução de `wasm/WasmMemory.h` e `.cpp`, só o modo `BoundsChecking`: sem Gigacage, sem
//! `mmap`, sem memória rápida com sinal. A memória é um `Vec<u8>` que só cresce; toda leitura e
//! escrita confere o limite. O `m_growSuccessCallback` e o `updateCachedMemories` do C++ existem
//! para a ponte com `ArrayBuffer`; aqui quem precisar observa o retorno de `grow`.
//!
//! Desvio deliberado: `maxAllocatableBytes` do C++ vem de `maxBufferByteLength` (ainda ausente em
//! `wasm_limits.rs`) e de `maxGrowableBufferReservationBytes`; aqui o teto é `MAX_ALLOCATABLE_BYTES`
//! (4 GiB), o do endereço de 32 bits. Memória compartilhada (`MemorySharingMode::Shared`) mantém o
//! mesmo comportamento: o interpretador é de uma thread só, então `growShared` e `grow` coincidem.

use crate::wasm::page_count::PageCount;
use crate::wasm::wasm_address_type::AddressType;
use crate::wasm::wasm_limits::{max_declarable_pages, PAGE_SIZE};
use std::cell::RefCell;
use std::rc::Rc;

/// O teto de bytes que uma memória recebe de fato (declarar é outra coisa).
pub const MAX_ALLOCATABLE_BYTES: u64 = 1 << 32;

/// `maxAllocatableBytes`.
pub fn max_allocatable_bytes(_address_type: AddressType) -> u64 {
    MAX_ALLOCATABLE_BYTES
}

/// `GrowFailReason`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GrowFailReason {
    InvalidDelta,
    InvalidGrowSize,
    WouldExceedMaximum,
    OutOfMemory,
}

/// `Memory`.
#[derive(Debug)]
pub struct Memory {
    /// Os bytes ficam num `Rc<RefCell<Vec<u8>>>` para o `ArrayBuffer` de `WebAssembly.Memory.buffer`
    /// (ver `runtime/js_web_assembly.rs`) enxergar o mesmo conteúdo (o `m_handle` do C++).
    bytes: Rc<RefCell<Vec<u8>>>,
    initial: PageCount,
    maximum: PageCount,
    shared: bool,
    address_type: AddressType,
}

impl Memory {
    /// `createZeroSized`.
    pub fn create_zero_sized(shared: bool, address_type: AddressType) -> Memory {
        Memory { bytes: Rc::default(), initial: PageCount::new(0), maximum: PageCount::new(0), shared, address_type }
    }

    /// Os bytes compartilhados com o `ArrayBuffer` exposto ao JS.
    pub fn bytes_handle(&self) -> Rc<RefCell<Vec<u8>>> {
        Rc::clone(&self.bytes)
    }

    /// `tryCreate`: `None` quando o cliente deve lançar `OutOfMemoryError`.
    pub fn try_create(initial: PageCount, maximum: PageCount, shared: bool, address_type: AddressType) -> Option<Memory> {
        assert!(initial.has_value());
        assert!(!maximum.has_value() || maximum >= initial);
        let initial_bytes = initial.bytes();
        if initial_bytes > max_allocatable_bytes(address_type) {
            return None;
        }
        if maximum.has_value() && maximum.bytes() == 0 {
            // User specified a zero maximum, initial size must also be zero.
            assert!(initial_bytes == 0);
            return Some(Memory::create_zero_sized(shared, address_type));
        }
        let mut bytes = Vec::new();
        bytes.try_reserve_exact(initial_bytes as usize).ok()?;
        bytes.resize(initial_bytes as usize, 0);
        Some(Memory { bytes: Rc::new(RefCell::new(bytes)), initial, maximum, shared, address_type })
    }

    /// `size`.
    pub fn size(&self) -> usize {
        self.bytes.borrow().len()
    }

    pub fn initial(&self) -> PageCount {
        self.initial
    }

    pub fn maximum(&self) -> PageCount {
        self.maximum
    }

    pub fn address_type(&self) -> AddressType {
        self.address_type
    }

    pub fn is_shared(&self) -> bool {
        self.shared
    }

    /// O tamanho atual em páginas.
    pub fn page_count(&self) -> PageCount {
        PageCount::from_bytes(self.size() as u64)
    }

    /// `grow`: devolve o tamanho anterior em páginas.
    pub fn grow(&mut self, delta: PageCount) -> Result<PageCount, GrowFailReason> {
        if !delta.is_valid() {
            return Err(GrowFailReason::InvalidDelta);
        }
        let old_page_count = self.page_count();
        let new_page_count = old_page_count + delta;
        let declarable = new_page_count.has_value()
            && new_page_count.is_valid()
            && new_page_count.page_count() <= max_declarable_pages(self.address_type);
        if !declarable {
            return Err(GrowFailReason::InvalidGrowSize);
        }
        if new_page_count.bytes() > max_allocatable_bytes(self.address_type) {
            return Err(GrowFailReason::OutOfMemory);
        }
        if delta.page_count() == 0 {
            return Ok(old_page_count);
        }
        if self.maximum.has_value() && new_page_count > self.maximum {
            return Err(GrowFailReason::WouldExceedMaximum);
        }
        let desired = new_page_count.bytes() as usize;
        let mut bytes = self.bytes.borrow_mut();
        let additional = desired - bytes.len();
        bytes.try_reserve_exact(additional).map_err(|_| GrowFailReason::OutOfMemory)?;
        bytes.resize(desired, 0);
        Ok(old_page_count)
    }

    /// `init`: copia `data` para `offset`, falha se sair da memória.
    pub fn init(&mut self, offset: u64, data: &[u8]) -> bool {
        let Some(end) = offset.checked_add(data.len() as u64) else {
            return false;
        };
        if end > self.size() as u64 {
            return false;
        }
        self.bytes.borrow_mut()[offset as usize..end as usize].copy_from_slice(data);
        true
    }

    /// Roda `body` sobre os `length` bytes a partir de `offset`, se estiverem dentro da memória.
    pub fn with_slice<R>(&self, offset: u64, length: u64, body: impl FnOnce(&[u8]) -> R) -> Option<R> {
        let end = offset.checked_add(length)?;
        let bytes = self.bytes.borrow();
        bytes.get(offset as usize..usize::try_from(end).ok()?).map(body)
    }

    /// A versão mutável de `with_slice`.
    pub fn with_slice_mut<R>(&mut self, offset: u64, length: u64, body: impl FnOnce(&mut [u8]) -> R) -> Option<R> {
        let end = offset.checked_add(length)?;
        let mut bytes = self.bytes.borrow_mut();
        bytes.get_mut(offset as usize..usize::try_from(end).ok()?).map(body)
    }

    /// Lê `width` (1 a 8) bytes little-endian.
    pub fn load(&self, offset: u64, width: usize) -> Option<u64> {
        self.with_slice(offset, width as u64, |slice| {
            let mut buffer = [0u8; 8];
            buffer[..width].copy_from_slice(slice);
            u64::from_le_bytes(buffer)
        })
    }

    /// Escreve os `width` bytes de baixo de `value`, little-endian.
    pub fn store(&mut self, offset: u64, width: usize, value: u64) -> bool {
        self.with_slice_mut(offset, width as u64, |slice| slice.copy_from_slice(&value.to_le_bytes()[..width])).is_some()
    }

    /// `memory.copy` (a região pode se sobrepor).
    pub fn copy_within(&mut self, destination: u64, source: u64, length: u64) -> bool {
        if self.with_slice(source, length, |_| ()).is_none() || self.with_slice(destination, length, |_| ()).is_none() {
            return false;
        }
        let source = source as usize;
        self.bytes.borrow_mut().copy_within(source..source + length as usize, destination as usize);
        true
    }

    /// `memory.fill`.
    pub fn fill(&mut self, offset: u64, value: u8, length: u64) -> bool {
        self.with_slice_mut(offset, length, |slice| slice.fill(value)).is_some()
    }
}

/// O tamanho de uma página, para quem só importa este módulo.
pub const WASM_PAGE_SIZE: u64 = PAGE_SIZE;

#[cfg(test)]
mod tests {
    use super::*;

    fn memory(initial: u64, maximum: Option<u64>) -> Memory {
        Memory::try_create(
            PageCount::new(initial),
            maximum.map(PageCount::new).unwrap_or_default(),
            false,
            AddressType::new(false),
        )
        .unwrap()
    }

    #[test]
    fn grow_returns_the_old_size_and_zero_fills() {
        let mut memory = memory(1, Some(3));
        assert!(memory.store(0, 4, 0xdead_beef));
        assert_eq!(memory.grow(PageCount::new(1)), Ok(PageCount::new(1)));
        assert_eq!(memory.size(), 2 * 65536);
        assert_eq!(memory.load(0, 4), Some(0xdead_beef));
        assert_eq!(memory.load(65536, 8), Some(0));
        assert_eq!(memory.grow(PageCount::new(0)), Ok(PageCount::new(2)));
    }

    #[test]
    fn grow_past_the_maximum_fails() {
        let mut memory = memory(1, Some(2));
        assert_eq!(memory.grow(PageCount::new(2)), Err(GrowFailReason::WouldExceedMaximum));
        assert_eq!(memory.grow(PageCount::new(1)), Ok(PageCount::new(1)));
        assert_eq!(memory.grow(PageCount::new(1)), Err(GrowFailReason::WouldExceedMaximum));
    }

    #[test]
    fn grow_past_the_address_space_fails() {
        let mut memory = memory(0, None);
        assert_eq!(memory.grow(PageCount::new(65537)), Err(GrowFailReason::InvalidGrowSize));
    }

    #[test]
    fn accesses_are_bounds_checked() {
        let mut memory = memory(1, None);
        assert!(memory.store(65532, 4, 1));
        assert!(!memory.store(65533, 4, 1));
        assert_eq!(memory.load(65536, 1), None);
        assert_eq!(memory.load(u64::MAX, 8), None);
        assert!(memory.init(10, &[1, 2, 3]));
        assert!(!memory.init(65535, &[1, 2]));
        assert!(memory.copy_within(11, 10, 3));
        assert_eq!(memory.with_slice(10, 4, <[u8]>::to_vec), Some(vec![1u8, 1, 2, 3]));
        assert!(!memory.fill(65535, 7, 2));
    }

    #[test]
    fn zero_maximum_gives_a_zero_sized_memory() {
        let memory = Memory::try_create(PageCount::new(0), PageCount::new(0), true, AddressType::new(false)).unwrap();
        assert_eq!(memory.size(), 0);
        assert!(memory.is_shared());
    }
}
