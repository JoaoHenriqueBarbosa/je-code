//! Tradução de `wasm/WasmLimits.h`. Os limites batem com os dos outros navegadores
//! (https://www.w3.org/TR/wasm-js-api-2/#limits).
//!
//! Fica de fora, até o porte de `MAX_ARRAY_BUFFER_SIZE` (o limite do array buffer), o grupo
//! `maxBufferByteLength` e `maxPageAlignedArrayBufferBytes`.

use crate::wasm::wasm_address_type::AddressType;

pub const MAX_TYPES: usize = 1_000_000;
pub const MAX_FUNCTIONS: usize = 1_000_000;
pub const MAX_IMPORTS: usize = 1_000_000;
pub const MAX_EXPORTS: usize = 1_000_000;
pub const MAX_EXCEPTIONS: usize = 1_000_000;
pub const MAX_GLOBALS: usize = 1_000_000;
pub const MAX_DATA_SEGMENTS: usize = 100_000;
pub const MAX_MEMORIES: usize = 100;
pub const MAX_STRUCT_FIELD_COUNT: usize = 10_000;
pub const MAX_ARRAY_NEW_FIXED_ARGS: usize = 10_000;
pub const MAX_RECURSION_GROUP_COUNT: usize = 1_000_000;
pub const MAX_NUMBER_OF_RECURSION_GROUPS: usize = 1_000_000;
pub const MAX_SUBTYPE_SUPERTYPE_COUNT: usize = 1;
pub const MAX_SUBTYPE_DEPTH: usize = 63;

pub const MAX_MODULE_SIZE: usize = 1024 * 1024 * 1024;
pub const MAX_FUNCTION_SIZE: usize = 7_654_321;
pub const MAX_FUNCTION_LOCALS: usize = 50_000;
pub const MAX_FUNCTION_PARAMS: usize = 1000;
pub const MAX_FUNCTION_RETURNS: usize = 1000;

pub const MAX_TABLE_ENTRIES: usize = 10_000_000;
pub const MAX_TABLES: u32 = 100_000;

/// `PageCount::pageSize`.
pub const PAGE_SIZE: u64 = 64 * 1024;
/// `PageCount::maxMemory32PageCount`.
pub const MAX_MEMORY32_PAGES: u64 = 64 * 1024;
/// `PageCount::maxPageCount`: `numeric_limits<uint64_t>::max() / pageSize + 1`.
pub const MAX_MEMORY64_PAGES: u64 = u64::MAX / PAGE_SIZE + 1;

/// `maxDeclarablePages`.
pub fn max_declarable_pages(address_type: AddressType) -> u64 {
    if address_type.is_64_bit() { MAX_MEMORY64_PAGES } else { MAX_MEMORY32_PAGES }
}

/// Limite de bytes de um array GC; fora da especificação da API JS, existe para evitar condições
/// de contorno complicadas.
pub const MAX_ARRAY_SIZE_IN_BYTES: usize = 1 << 30;
