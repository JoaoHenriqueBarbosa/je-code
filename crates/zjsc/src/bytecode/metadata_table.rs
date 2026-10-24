//! Porte de `bytecode/MetadataTable.h` e `MetadataTable.cpp` (mais o `link()`/`unlink()` de
//! `UnlinkedMetadataTableInlines.h`): a tabela de metadata linkada de um `CodeBlock`.
//!
//! No C++ o `MetadataTable` é um ponteiro para o meio de um buffer de bytes, com a disposição
//! `[ValueProfile][LinkingData][MetadataTableOffsets][MetadataContent]`, e `get<Metadata>()` faz
//! aritmética de ponteiro com `alignof(Metadata)`. Rust seguro não reinterpreta bytes em structs,
//! então a tabela guarda os dados com tipo: um `Vec<Op*Metadata>` por opcode com metadata
//! (`MetadataStore`, gerado pelo macro abaixo na mesma ordem do `OpcodeID`) e um
//! `Vec<ValueProfile>` para os perfis de valor. Cada vetor tem exatamente as
//! `UnlinkedMetadataTable::numEntries` entradas que o `finalize()` contou; a identidade
//! (opcode, `m_metadataID`) escolhe a entrada, como no C++.
//!
//! Divergências:
//!
//! - O buffer cru, o `LinkingData` e o `refCount` não existem: o compartilhamento é um
//!   `Rc<RefCell<MetadataTable>>` (`MetadataTableRef`), e a tabela guarda uma cópia do
//!   `UnlinkedMetadataTable` finalizado (o C++ guarda um `Ref` para o mesmo objeto; depois do
//!   `finalize()` ele só é lido, exceto `didOptimize`, que nenhum leitor do porte consulta pela
//!   tabela linkada).
//! - `getOffset`/`offsetTable16`/`offsetTable32`/`is32Bit` viram consultas ao
//!   `UnlinkedMetadataTable` (`offset_for`, `is_32_bit`), que guarda os mesmos deslocamentos em
//!   bytes; `offset_in_metadata_table` devolve o valor que o C++ calcula.
//! - O `~MetadataTable` (que destrói cada `Metadata`) e o `destroy`/`unlink` são o `Drop` do Rust.
//! - `validate()` só tem `ASSERT`: vira `debug_assert!` no `link`.
//! - Uma entrada de metadata nasce com o `Default` do struct (o `fastZeroedMalloc` do C++ mais o
//!   construtor `Metadata(const Op&)` que o `CodeBlock::finishCreation` roda por instrução).

use std::cell::RefCell;
use std::rc::Rc;

use crate::bytecode::op_metadata::*;
use crate::bytecode::opcode::OpcodeID;
use crate::bytecode::unlinked_metadata_table::UnlinkedMetadataTable;
use crate::bytecode::value_profile::ValueProfile;

/// `RefPtr<MetadataTable>`.
pub type MetadataTableRef = Rc<RefCell<MetadataTable>>;

/// Liga o tipo `Op*Metadata` ao seu `OpcodeID` e ao vetor dele no `MetadataStore`: o
/// `Metadata::opcodeID` que `MetadataTable::get<Metadata>()` usa.
pub trait MetadataFor: Sized + Default {
    const OPCODE_ID: OpcodeID;
    fn entries(store: &MetadataStore) -> &Vec<Self>;
    fn entries_mut(store: &mut MetadataStore) -> &mut Vec<Self>;
}

/// Declara o `MetadataStore` e os `impl MetadataFor` a partir da lista `opcode, Tipo;` na ordem do
/// `OpcodeID` (a mesma de `define_op_metadata!` em `op_metadata.rs`).
macro_rules! define_metadata_store {
    ($( $opcode:ident, $ty:ident; )*) => {
        /// Uma lista de entradas por opcode com metadata.
        #[derive(Debug, Default)]
        pub struct MetadataStore {
            $( pub $opcode: Vec<$ty>, )*
        }

        $(
            impl MetadataFor for $ty {
                const OPCODE_ID: OpcodeID = OpcodeID::$opcode;
                fn entries(store: &MetadataStore) -> &Vec<$ty> {
                    &store.$opcode
                }
                fn entries_mut(store: &mut MetadataStore) -> &mut Vec<$ty> {
                    &mut store.$opcode
                }
            }
        )*

        impl MetadataStore {
            /// Uma entrada por `addEntry` que o gerador pediu, cada uma no valor inicial.
            fn with_entry_counts(unlinked: &UnlinkedMetadataTable) -> MetadataStore {
                MetadataStore {
                    $( $opcode: vec_of_defaults(unlinked.num_entries(OpcodeID::$opcode)), )*
                }
            }
        }
    };
}

fn vec_of_defaults<T: Default>(count: u32) -> Vec<T> {
    let mut entries = Vec::with_capacity(count as usize);
    entries.resize_with(count as usize, T::default);
    entries
}

define_metadata_store! {
    op_tail_call_varargs, OpTailCallVarargsMetadata;
    op_call_varargs, OpCallVarargsMetadata;
    op_iterator_next, OpIteratorNextMetadata;
    op_construct_varargs, OpConstructVarargsMetadata;
    op_super_construct_varargs, OpSuperConstructVarargsMetadata;
    op_iterator_open, OpIteratorOpenMetadata;
    op_async_iterator_open, OpAsyncIteratorOpenMetadata;
    op_instanceof, OpInstanceofMetadata;
    op_set_private_brand, OpSetPrivateBrandMetadata;
    op_check_private_brand, OpCheckPrivateBrandMetadata;
    op_put_by_id, OpPutByIdMetadata;
    op_construct, OpConstructMetadata;
    op_super_construct, OpSuperConstructMetadata;
    op_tail_call, OpTailCallMetadata;
    op_call_direct_eval, OpCallDirectEvalMetadata;
    op_create_generator, OpCreateGeneratorMetadata;
    op_create_async_generator, OpCreateAsyncGeneratorMetadata;
    op_create_promise, OpCreatePromiseMetadata;
    op_catch, OpCatchMetadata;
    op_new_array_with_size, OpNewArrayWithSizeMetadata;
    op_new_array_buffer, OpNewArrayBufferMetadata;
    op_get_by_id, OpGetByIdMetadata;
    op_get_length, OpGetLengthMetadata;
    op_profile_type, OpProfileTypeMetadata;
    op_profile_control_flow, OpProfileControlFlowMetadata;
    op_new_array_with_species, OpNewArrayWithSpeciesMetadata;
    op_call, OpCallMetadata;
    op_call_ignore_result, OpCallIgnoreResultMetadata;
    op_async_iterator_next, OpAsyncIteratorNextMetadata;
    op_resolve_scope, OpResolveScopeMetadata;
    op_get_from_scope, OpGetFromScopeMetadata;
    op_put_to_scope, OpPutToScopeMetadata;
    op_create_this, OpCreateThisMetadata;
    op_new_object, OpNewObjectMetadata;
    op_new_array, OpNewArrayMetadata;
    op_put_private_name, OpPutPrivateNameMetadata;
    op_get_private_name, OpGetPrivateNameMetadata;
    op_get_by_val_with_this, OpGetByValWithThisMetadata;
    op_get_by_val, OpGetByValMetadata;
    op_put_by_val, OpPutByValMetadata;
    op_put_by_val_direct, OpPutByValDirectMetadata;
    op_in_by_val, OpInByValMetadata;
    op_enumerator_next, OpEnumeratorNextMetadata;
    op_enumerator_in_by_val, OpEnumeratorInByValMetadata;
    op_enumerator_has_own_property, OpEnumeratorHasOwnPropertyMetadata;
    op_enumerator_put_by_val, OpEnumeratorPutByValMetadata;
    op_to_this, OpToThisMetadata;
    op_enumerator_get_by_val, OpEnumeratorGetByValMetadata;
    op_get_by_id_direct, OpGetByIdDirectMetadata;
    op_jneq_ptr, OpJneqPtrMetadata;
}

/// `class MetadataTable`.
#[derive(Debug)]
pub struct MetadataTable {
    /// `LinkingData::unlinkedMetadata`.
    unlinked_metadata: UnlinkedMetadataTable,
    /// Os `m_numValueProfiles` perfis que ficam antes do `LinkingData`: o `valueProfilesEnd()[-n]`
    /// do C++ (n começa em 1) é o índice `n - 1` daqui.
    value_profiles: Vec<ValueProfile>,
    store: MetadataStore,
}

impl MetadataTable {
    /// `UnlinkedMetadataTable::link()`: `nullptr` (aqui `None`) quando não há metadata. A tabela
    /// precisa estar finalizada.
    pub fn link(unlinked_metadata: &UnlinkedMetadataTable) -> Option<MetadataTableRef> {
        debug_assert!(unlinked_metadata.is_finalized());
        if !unlinked_metadata.has_metadata() {
            return None;
        }
        let table = MetadataTable {
            unlinked_metadata: unlinked_metadata.clone(),
            value_profiles: vec_of_defaults(unlinked_metadata.num_value_profiles()),
            store: MetadataStore::with_entry_counts(unlinked_metadata),
        };
        table.validate();
        Some(Rc::new(RefCell::new(table)))
    }

    /// `get<Metadata>()`: as entradas de um opcode (`get<Metadata>()[metadataID]`).
    pub fn get<M: MetadataFor>(&self) -> &[M] {
        debug_assert!((M::OPCODE_ID as u16) < crate::bytecode::instruction_stream::NUMBER_OF_BYTECODE_WITH_METADATA);
        M::entries(&self.store)
    }

    /// `get<Metadata>()` para escrita.
    pub fn get_mut<M: MetadataFor>(&mut self) -> &mut [M] {
        debug_assert!((M::OPCODE_ID as u16) < crate::bytecode::instruction_stream::NUMBER_OF_BYTECODE_WITH_METADATA);
        M::entries_mut(&mut self.store)
    }

    /// `forEach<Op>(func)`: percorre as entradas do opcode, da primeira à última.
    pub fn for_each<M: MetadataFor>(&mut self, mut func: impl FnMut(&mut M)) {
        for entry in self.get_mut::<M>() {
            func(entry);
        }
    }

    /// `forEachValueProfile(func)`: do perfil de offset 1 ao de offset `m_numValueProfiles`.
    pub fn for_each_value_profile(&mut self, mut func: impl FnMut(&mut ValueProfile)) {
        for profile in self.value_profiles.iter_mut() {
            func(profile);
        }
    }

    /// `valueProfileForOffset(profileOffset)`: `profileOffset` é o valor de `addValueProfile()`
    /// (começa em 1, porque o C++ indexa para trás a partir de `valueProfilesEnd()`).
    pub fn value_profile_for_offset(&mut self, profile_offset: u32) -> &mut ValueProfile {
        debug_assert!(profile_offset >= 1 && profile_offset <= self.unlinked_metadata.num_value_profiles());
        &mut self.value_profiles[profile_offset as usize - 1]
    }

    /// `offsetInMetadataTable(opcode)`: o deslocamento em bytes do metadata `metadata_id` dentro
    /// do buffer do C++ (deslocamento do opcode alinhado ao `alignof(Metadata)`, mais
    /// `sizeof(Metadata) * m_metadataID`).
    pub fn offset_in_metadata_table(&self, opcode_id: OpcodeID, metadata_id: u32) -> usize {
        let alignment = metadata_alignment(opcode_id) as usize;
        let base_type_offset = (self.unlinked_metadata.offset_for(opcode_id) as usize).div_ceil(alignment) * alignment;
        base_type_offset + metadata_size(opcode_id) as usize * metadata_id as usize
    }

    /// `sizeInBytesForGC()` (`UnlinkedMetadataTable::sizeInBytesForGC(MetadataTable&)`): o tamanho
    /// da tabela sem a tabela de deslocamentos, que o `UnlinkedCodeBlock` já contou.
    pub fn size_in_bytes_for_gc(&self) -> usize {
        self.total_size() as usize - self.unlinked_metadata.offset_table_size() as usize
    }

    /// `unlinkedMetadata()`.
    pub fn unlinked_metadata(&self) -> &UnlinkedMetadataTable {
        &self.unlinked_metadata
    }

    /// `is32Bit()`.
    pub fn is_32_bit(&self) -> bool {
        self.unlinked_metadata.is_32_bit()
    }

    /// `totalSize()` (sem o `LinkingData`, que o porte não tem).
    fn total_size(&self) -> u32 {
        self.unlinked_metadata.total_size()
    }

    /// `validate()`: a tabela de valores de perfil e as entradas batem com o que o `finalize()` contou.
    fn validate(&self) {
        debug_assert!(self.value_profiles.len() as u32 == self.unlinked_metadata.num_value_profiles());
        debug_assert!(self.unlinked_metadata.offset_for(OpcodeID::op_tail_call_varargs) >= self.unlinked_metadata.offset_table_size());
    }
}
