//! Tradução de `runtime/ArrayStorage.h`, `ArrayStorageInlines.h` e das definições de
//! `ArrayConventions.h` que só o `ArrayStorage` usa (`BASE_ARRAY_STORAGE_VECTOR_LEN`,
//! `FIRST_ARRAY_STORAGE_VECTOR_GROW`, `indexingHeaderForArrayStorage`). O resto de `ArrayConventions.h`
//! (`MAX_STORAGE_VECTOR_LENGTH`, `MIN_SPARSE_ARRAY_INDEX`, `isDenseEnoughForVector`...) já mora em
//! `js_object.rs` e é importado de lá, não repetido.
//!
//! DIVERGÊNCIAS (sem heap e sem GC, camada 3):
//!
//! - O `ArrayStorage` do C++ é o cabeçalho de um `Butterfly` (campos `m_sparseMap`, `m_indexBias`,
//!   `m_numValuesInVector` seguidos do vetor `m_vector`, com o `length` e o `vectorLength` roubados do
//!   `IndexingHeader`). Aqui é uma struct normal com os mesmos campos, e o `vectorLength` é o tamanho do
//!   `Vec`. `from(Butterfly*)`, `butterfly()`, `indexingHeader()`, os `*Offset()`, `sizeFor`, `totalSizeFor`
//!   e `totalSize` falam do layout de memória e não existem.
//! - `availableVectorLength` arredonda ao tamanho de classe do `MarkedSpace`; o comprimento do vetor não é
//!   observável, então é a identidade (mesma escolha de `available_contiguous_vector_length`), e
//!   `optimalVectorLength` é só o `max` com `BASE_ARRAY_STORAGE_VECTOR_LEN`.
//! - `m_sparseMap` é um `Option<SparseArrayValueMap>` (veja `sparse_array_value_map.rs`): o `WriteBarrier`
//!   e o `clear()` viram `Some`/`None`.
//! - Ligada ao `Butterfly` de `JSObject` pela variante `IndexedStorage::ArrayStorage` (`js_object.rs`),
//!   que o `js_object_array_storage.rs` conversa e consulta. O `length` daqui é o `publicLength` do
//!   `IndexingHeader` quando a forma é `ArrayStorage`/`SlowPutArrayStorage` (`JSObject::public_length`).

use crate::runtime::js_value::JSValue;
use crate::runtime::sparse_array_value_map::SparseArrayValueMap;

/// `BASE_ARRAY_STORAGE_VECTOR_LEN`.
pub const BASE_ARRAY_STORAGE_VECTOR_LEN: u32 = 4;

/// `FIRST_ARRAY_STORAGE_VECTOR_GROW`: o teto de crescimento de um array vazio quando o primeiro
/// elemento entra.
pub const FIRST_ARRAY_STORAGE_VECTOR_GROW: u32 = 4;

/// `struct ArrayStorage`.
#[derive(Debug)]
pub struct ArrayStorage {
    /// `IndexingHeader::publicLength` (o `length()` do `ArrayStorage`).
    length: u32,
    /// `m_indexBias`.
    index_bias: u32,
    /// `m_numValuesInVector`.
    num_values_in_vector: u32,
    /// `m_vector`: `vectorLength()` posições, os buracos são `JSValue::empty()`.
    vector: Vec<JSValue>,
    /// `m_sparseMap`.
    sparse_map: Option<SparseArrayValueMap>,
}

impl ArrayStorage {
    /// `visitChildren`: o vetor e o mapa esparso.
    pub(crate) fn visit_children(&self, marker: &mut crate::runtime::gc::Marker) {
        marker.values(&self.vector);
        if let Some(sparse_map) = &self.sparse_map {
            sparse_map.visit_children(marker);
        }
    }

    /// `tryCreateArrayButterfly(vm, owner, initialLength)` mais o `indexingHeaderForArrayStorage`:
    /// armazenamento novo com `length` e `vectorLength` dados, todos os buracos limpos. `None` é o `nullptr`
    /// do `tryAllocate` (falta de memória): quem chama lança `OutOfMemoryError`.
    pub fn try_new(length: u32, vector_length: u32) -> Option<ArrayStorage> {
        Some(ArrayStorage {
            length,
            index_bias: 0,
            num_values_in_vector: 0,
            vector: crate::runtime::fallible_alloc::try_filled_vec(JSValue::empty(), vector_length as usize)?,
            sparse_map: None,
        })
    }

    /// Armazenamento sem vetor (`length` e `vectorLength` zero): não aloca.
    pub fn empty() -> ArrayStorage {
        ArrayStorage { length: 0, index_bias: 0, num_values_in_vector: 0, vector: Vec::new(), sparse_map: None }
    }

    /// `baseIndexingHeaderForArrayStorage(length)`: o vetor tem `BASE_ARRAY_STORAGE_VECTOR_LEN` posições
    /// (quatro, uma constante: não vem de JS e não falha).
    pub fn new_base(length: u32) -> ArrayStorage {
        ArrayStorage {
            length,
            index_bias: 0,
            num_values_in_vector: 0,
            vector: vec![JSValue::empty(); BASE_ARRAY_STORAGE_VECTOR_LEN as usize],
            sparse_map: None,
        }
    }

    /// `length()`.
    pub fn length(&self) -> u32 {
        self.length
    }

    /// `setLength(length)`.
    pub fn set_length(&mut self, length: u32) {
        self.length = length;
    }

    /// `vectorLength()`.
    pub fn vector_length(&self) -> u32 {
        self.vector.len() as u32
    }

    /// `setVectorLength(length)`: cresce com buracos ou encolhe o vetor.
    pub fn set_vector_length(&mut self, length: u32) {
        self.vector.resize(length as usize, JSValue::empty());
    }

    /// `m_indexBias`.
    pub fn index_bias(&self) -> u32 {
        self.index_bias
    }

    /// `m_indexBias = bias`.
    pub fn set_index_bias(&mut self, bias: u32) {
        self.index_bias = bias;
    }

    /// `m_numValuesInVector`.
    pub fn num_values_in_vector(&self) -> u32 {
        self.num_values_in_vector
    }

    /// `m_numValuesInVector = count`.
    pub fn set_num_values_in_vector(&mut self, count: u32) {
        self.num_values_in_vector = count;
    }

    /// `hasHoles()`.
    pub fn has_holes(&self) -> bool {
        self.num_values_in_vector != self.length
    }

    /// `inSparseMode()`.
    pub fn in_sparse_mode(&self) -> bool {
        self.sparse_map.as_ref().is_some_and(|map| map.sparse_mode())
    }

    /// `m_sparseMap.get()`.
    pub fn sparse_map(&self) -> Option<&SparseArrayValueMap> {
        self.sparse_map.as_ref()
    }

    /// `m_sparseMap.get()` para escrita.
    pub fn sparse_map_mut(&mut self) -> Option<&mut SparseArrayValueMap> {
        self.sparse_map.as_mut()
    }

    /// `m_sparseMap.set(vm, this, map)` / `m_sparseMap.clear()`.
    pub fn set_sparse_map(&mut self, map: Option<SparseArrayValueMap>) {
        self.sparse_map = map;
    }

    /// `m_sparseMap.clear()` devolvendo o mapa que saiu.
    pub fn take_sparse_map(&mut self) -> Option<SparseArrayValueMap> {
        self.sparse_map.take()
    }

    /// `vector()`.
    pub fn vector(&self) -> &[JSValue] {
        &self.vector
    }

    /// `vector()` para escrita.
    pub fn vector_mut(&mut self) -> &mut [JSValue] {
        &mut self.vector
    }

    /// `availableVectorLength(indexBias, propertyCapacity, vectorLength)`: a identidade (veja o cabeçalho).
    pub fn available_vector_length(_index_bias: u32, _property_capacity: usize, vector_length: u32) -> u32 {
        vector_length
    }

    /// `optimalVectorLength(indexBias, propertyCapacity, vectorLength)`.
    pub fn optimal_vector_length(index_bias: u32, property_capacity: usize, vector_length: u32) -> u32 {
        ArrayStorage::available_vector_length(
            index_bias,
            property_capacity,
            BASE_ARRAY_STORAGE_VECTOR_LEN.max(vector_length),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn base_storage_has_four_holes() {
        let storage = ArrayStorage::new_base(0);
        assert_eq!(storage.vector_length(), 4);
        assert_eq!(storage.length(), 0);
        assert!(!storage.has_holes());
        assert!(storage.vector().iter().all(|value| value.is_empty()));
    }

    #[test]
    fn try_new_fails_instead_of_aborting_on_impossible_sizes() {
        // 2^32 - 1 posições de 8 bytes: o alocador (ou o limite de `isize`) recusa em máquina de 32 GiB ou menos.
        let storage = ArrayStorage::try_new(0, 16).expect("vetor pequeno");
        assert_eq!(storage.vector_length(), 16);
        assert!(storage.vector().iter().all(|value| value.is_empty()));
        assert!(crate::runtime::fallible_alloc::try_filled_vec(JSValue::empty(), usize::MAX / 4).is_none());
    }

    #[test]
    fn length_beyond_values_is_a_hole() {
        let mut storage = ArrayStorage::new_base(3);
        assert!(storage.has_holes());
        storage.set_num_values_in_vector(3);
        assert!(!storage.has_holes());
    }

    #[test]
    fn optimal_vector_length_is_at_least_the_base() {
        assert_eq!(ArrayStorage::optimal_vector_length(0, 0, 0), BASE_ARRAY_STORAGE_VECTOR_LEN);
        assert_eq!(ArrayStorage::optimal_vector_length(0, 0, 10), 10);
    }
}
