//! Tradução de `wasm/WasmTable.h` e `.cpp`, os dados puros (sem `JSWebAssemblyTable`, sem barreira
//! de escrita, sem a tabela inline de tamanho fixo, que só existe por causa do JIT).
//!
//! Cada elemento é um `u64` com a codificação de referência do interpretador (ver
//! `wasm_instance::NULL_REF` e `func_ref`): o C++ separa `FuncRefTable` (funções importáveis) de
//! `ExternOrAnyRefTable` (`JSValue`), e aqui os dois cabem no mesmo vetor, o que `Table::get` e
//! `Table::set` já faziam por cima (`funcTable->get(index)` ou `jsNull`).

use crate::wasm::wasm_address_type::AddressType;
use crate::wasm::wasm_format::{TableElementType, Type};
use crate::wasm::wasm_limits::MAX_TABLE_ENTRIES;

/// `Table`.
#[derive(Debug)]
pub struct Table {
    elements: Vec<u64>,
    maximum: Option<u64>,
    element_type: TableElementType,
    wasm_type: Type,
    address_type: AddressType,
}

impl Table {
    /// `isValidLength`.
    pub fn is_valid_length(length: u64) -> bool {
        length <= MAX_TABLE_ENTRIES as u64
    }

    /// `tryCreate`: `None` quando o tamanho inicial passa de `maxTableEntries`. Os elementos nascem
    /// com `null` (o valor de inicialização explícito é aplicado por quem instancia).
    pub fn try_create(
        declared_initial: u64,
        maximum: Option<u64>,
        element_type: TableElementType,
        wasm_type: Type,
        address_type: AddressType,
        null: u64,
    ) -> Option<Table> {
        if !Table::is_valid_length(declared_initial) {
            return None;
        }
        Some(Table { elements: vec![null; declared_initial as usize], maximum, element_type, wasm_type, address_type })
    }

    /// `length`.
    pub fn length(&self) -> u32 {
        self.elements.len() as u32
    }

    pub fn maximum(&self) -> Option<u64> {
        self.maximum
    }

    pub fn element_type(&self) -> TableElementType {
        self.element_type
    }

    pub fn wasm_type(&self) -> Type {
        self.wasm_type
    }

    pub fn address_type(&self) -> AddressType {
        self.address_type
    }

    /// `grow`: o tamanho anterior, ou `None` quando passa do máximo ou do limite de entradas.
    pub fn grow(&mut self, delta: u64, default_value: u64) -> Option<u32> {
        let length = u64::from(self.length());
        if delta == 0 {
            return Some(length as u32);
        }
        let new_length = length.checked_add(delta)?;
        if self.maximum.is_some_and(|maximum| new_length > maximum) {
            return None;
        }
        if !Table::is_valid_length(new_length) {
            return None;
        }
        self.elements.try_reserve_exact((new_length - length) as usize).ok()?;
        self.elements.resize(new_length as usize, default_value);
        Some(length as u32)
    }

    /// `get`.
    pub fn get(&self, index: u32) -> u64 {
        self.elements[index as usize]
    }

    /// `set`.
    pub fn set(&mut self, index: u32, value: u64) {
        self.elements[index as usize] = value;
    }

    /// `fill` de um intervalo (a operação `table.fill`).
    pub fn fill_range(&mut self, start: u32, value: u64, count: u32) {
        self.elements[start as usize..start as usize + count as usize].fill(value);
    }

    /// Os elementos, para `table.copy` entre tabelas.
    pub fn elements(&self) -> &[u64] {
        &self.elements
    }

    /// `table.copy` dentro da mesma tabela (a região pode se sobrepor).
    pub fn copy_within(&mut self, destination: u32, source: u32, count: u32) {
        let source = source as usize;
        self.elements.copy_within(source..source + count as usize, destination as usize);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::wasm::wasm_format::funcref_type;

    fn table(initial: u64, maximum: Option<u64>) -> Table {
        Table::try_create(initial, maximum, TableElementType::Funcref, funcref_type(), AddressType::new(false), 0).unwrap()
    }

    #[test]
    fn grow_respects_the_maximum() {
        let mut table = table(2, Some(4));
        assert_eq!(table.grow(0, 9), Some(2));
        assert_eq!(table.grow(2, 9), Some(2));
        assert_eq!(table.get(3), 9);
        assert_eq!(table.grow(1, 9), None);
        assert_eq!(table.length(), 4);
    }

    #[test]
    fn oversized_tables_are_refused() {
        let created = Table::try_create(
            MAX_TABLE_ENTRIES as u64 + 1,
            None,
            TableElementType::Funcref,
            funcref_type(),
            AddressType::new(false),
            0,
        );
        assert!(created.is_none());
    }

    #[test]
    fn copy_within_overlaps_like_memmove() {
        let mut table = table(4, None);
        for index in 0..4 {
            table.set(index, u64::from(index) + 1);
        }
        table.copy_within(1, 0, 3);
        assert_eq!(table.elements(), &[1, 1, 2, 3]);
    }
}
