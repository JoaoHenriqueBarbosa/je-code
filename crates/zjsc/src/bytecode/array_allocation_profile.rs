//! Porte de `bytecode/ArrayAllocationProfile.h` e `ArrayAllocationProfile.cpp`: o comportamento do
//! `ArrayAllocationProfile`. O struct (`ArrayAllocationProfile`, com o `CompactPointerTuple<JSArray*,
//! uint16_t>` aberto em `last_array` e `indexing_type_and_vector_length`) e os construtores
//! (`ArrayAllocationProfile()`, `ArrayAllocationProfile(IndexingType)`) vivem em
//! `bytecode/op_metadata.rs`, que o metadata do bytecode precisa; este módulo acrescenta os
//! métodos no mesmo tipo.
//!
//! Divergências:
//!
//! - `JSArray*` é o `HeapRef` (índice da célula; 0 é o `nullptr`). O que o C++ pergunta ao
//!   `JSArray` (`indexingType()` e `getVectorLength()`) é o trait `LastArray`, que `HeapArrays`
//!   implementa sobre o `cell_registry` (o `HeapRef` é o `cell_id`).
//! - `isCompilationThread()` e as variantes `...Concurrently` (que só mudam por thread de compilação)
//!   ficam: sem JIT elas valem o mesmo que as de thread principal, mas existem porque o C++ as chama.
//! - `BASE_CONTIGUOUS_VECTOR_LEN_MAX` (`runtime/ArrayConventions.h`, `25U`) mora aqui até o módulo
//!   de `ArrayConventions` ser portado.

use crate::bytecode::op_metadata::{ArrayAllocationProfile, HeapRef};
use crate::runtime::indexing_type::{
    is_copy_on_write, least_upper_bound_of_indexing_types, IndexingType, ARRAY_WITH_CONTIGUOUS,
    ARRAY_WITH_UNDECIDED, COPY_ON_WRITE, INDEXING_TYPE_MASK,
};
use crate::runtime::js_array::JSArray;
use crate::runtime::options::Options;

/// `BASE_CONTIGUOUS_VECTOR_LEN_MAX`.
pub const BASE_CONTIGUOUS_VECTOR_LEN_MAX: u32 = 25;

/// `MIN_ARRAY_STORAGE_CONSTRUCTION_LENGTH` (`ArrayConventions.h`).
pub const MIN_ARRAY_STORAGE_CONSTRUCTION_LENGTH: u32 = 1024 * 1024 * 1024 / 8;

/// O que o perfil lê do `JSArray` da última alocação.
pub trait LastArray {
    /// `JSArray::indexingType()`.
    fn indexing_type(&self, array: HeapRef) -> IndexingType;
    /// `JSArray::getVectorLength()`.
    fn get_vector_length(&self, array: HeapRef) -> u32;
}

/// O `LastArray` do porte: o `HeapRef` é o `cell_id` do `JSArray` no `cell_registry`.
pub struct HeapArrays;

impl LastArray for HeapArrays {
    fn indexing_type(&self, array: HeapRef) -> IndexingType {
        JSArray::from_cell_id(array as usize).expect("ArrayAllocationProfile::m_lastArray não é um JSArray").cell().indexing_type()
    }

    fn get_vector_length(&self, array: HeapRef) -> u32 {
        JSArray::from_cell_id(array as usize).expect("ArrayAllocationProfile::m_lastArray não é um JSArray").vector_length()
    }
}

/// `ArrayAllocationProfile::IndexingTypeAndVectorLength`: `(IndexingType << 8) | VectorLength`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct IndexingTypeAndVectorLength {
    bits: u16,
}

impl IndexingTypeAndVectorLength {
    const VECTOR_LENGTH_MASK: u16 = u8::MAX as u16;
    const INDEXING_TYPE_SHIFT: u16 = 8;

    fn new(indexing_type: IndexingType, vector_length: u32) -> IndexingTypeAndVectorLength {
        debug_assert!(vector_length <= BASE_CONTIGUOUS_VECTOR_LEN_MAX);
        IndexingTypeAndVectorLength {
            bits: ((indexing_type as u16) << Self::INDEXING_TYPE_SHIFT) | vector_length as u16,
        }
    }

    fn indexing_type(self) -> IndexingType {
        (self.bits >> Self::INDEXING_TYPE_SHIFT) as IndexingType
    }

    fn vector_length(self) -> u32 {
        (self.bits & Self::VECTOR_LENGTH_MASK) as u32
    }

    fn with_indexing_type(self, indexing_type: IndexingType) -> IndexingTypeAndVectorLength {
        IndexingTypeAndVectorLength::new(indexing_type, self.vector_length())
    }
}

impl ArrayAllocationProfile {
    /// `current()`.
    fn current(&self) -> IndexingTypeAndVectorLength {
        IndexingTypeAndVectorLength { bits: self.indexing_type_and_vector_length }
    }

    /// `selectIndexingTypeConcurrently`.
    pub fn select_indexing_type_concurrently(&self) -> IndexingType {
        self.current().indexing_type()
    }

    /// `selectIndexingType`.
    pub fn select_indexing_type(&mut self, arrays: &dyn LastArray) -> IndexingType {
        if self.last_array != 0 && arrays.indexing_type(self.last_array) != self.current().indexing_type() {
            self.update_profile(arrays);
        }
        self.current().indexing_type()
    }

    /// `vectorLengthHintConcurrently`: o hint fica em `[0, BASE_CONTIGUOUS_VECTOR_LEN_MAX]`.
    pub fn vector_length_hint_concurrently(&self) -> u32 {
        self.current().vector_length()
    }

    /// `vectorLengthHint`.
    pub fn vector_length_hint(&mut self, arrays: &dyn LastArray) -> u32 {
        let largest_seen_vector_length = self.current().vector_length();
        if self.last_array != 0
            && largest_seen_vector_length != BASE_CONTIGUOUS_VECTOR_LEN_MAX
            && arrays.get_vector_length(self.last_array) > largest_seen_vector_length
        {
            self.update_profile(arrays);
        }
        self.current().vector_length()
    }

    /// `updateLastAllocation`.
    pub fn update_last_allocation(&mut self, last_array: HeapRef) -> HeapRef {
        self.last_array = last_array;
        last_array
    }

    /// `updateProfile`.
    pub fn update_profile(&mut self, arrays: &dyn LastArray) {
        // `std::exchange(m_storage, Storage(nullptr, m_storage.type()))`.
        let last_array = std::mem::replace(&mut self.last_array, 0);
        let current = self.current();
        if last_array == 0 {
            return;
        }
        if Options::with(|options| options.use_array_allocation_profiling) {
            // O modelo: sobe para a versão CoW do `lastArray`, exceto ArrayStorage, que não tem CoW.
            let mut indexing_type = least_upper_bound_of_indexing_types(
                current.indexing_type() & INDEXING_TYPE_MASK,
                arrays.indexing_type(last_array),
            );
            if is_copy_on_write(current.indexing_type()) {
                if indexing_type > ARRAY_WITH_CONTIGUOUS {
                    indexing_type = ARRAY_WITH_CONTIGUOUS;
                }
                indexing_type |= COPY_ON_WRITE;
            }
            let largest_seen_vector_length = current
                .vector_length()
                .max(arrays.get_vector_length(last_array))
                .min(BASE_CONTIGUOUS_VECTOR_LEN_MAX);
            self.indexing_type_and_vector_length =
                IndexingTypeAndVectorLength::new(indexing_type, largest_seen_vector_length).bits;
        }
    }

    /// `selectIndexingTypeFor`.
    pub fn select_indexing_type_for(
        profile: Option<&mut ArrayAllocationProfile>,
        arrays: &dyn LastArray,
    ) -> IndexingType {
        match profile {
            None => ARRAY_WITH_UNDECIDED,
            Some(profile) => profile.select_indexing_type(arrays),
        }
    }

    /// `updateLastAllocationFor`.
    pub fn update_last_allocation_for(profile: Option<&mut ArrayAllocationProfile>, last_array: HeapRef) -> HeapRef {
        if let Some(profile) = profile {
            profile.update_last_allocation(last_array);
        }
        last_array
    }

    /// `initializeIndexingMode`.
    pub fn initialize_indexing_mode(&mut self, recommended_indexing_mode: IndexingType) {
        self.indexing_type_and_vector_length =
            self.current().with_indexing_type(recommended_indexing_mode).bits;
    }
}
