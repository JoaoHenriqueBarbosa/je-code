//! Porte de `bytecode/UnlinkedMetadataTable.h`, `.cpp` e `UnlinkedMetadataTableInlines.h`: a parte
//! que o gerador de bytecode e o `UnlinkedCodeBlock` usam (contagem de entradas por opcode e de
//! value profiles, `finalize`, `didOptimize`).
//!
//! Divergências:
//!
//! - O C++ guarda, antes do `finalize`, os contadores por opcode num buffer de `Offset32` e, no
//!   `finalize`, os troca por deslocamentos em bytes calculados com `sizeof`/`alignof` do
//!   `Op::Metadata` de cada opcode (`metadataSize`/`metadataAlignment`, vindos de
//!   `crate::bytecode::op_metadata`), escolhendo tabela de 16 ou 32 bits e recusando overflow. O
//!   porte guarda os deslocamentos num array próprio (`offsets`, sem o viés de 32 bits, como o
//!   `buffer` de pré-processamento do C++) e mantém as contagens depois do `finalize`, que é o que
//!   o `link()` do interpretador precisa; o buffer cru de bytes não existe.
//! - `link`, `unlink`, `LinkingData`, `createFromPersistentSteps`/`expandSteps` (cache de bytecode),
//!   `sizeInBytesForGC` e as estatísticas (`ENABLE(METADATA_STATISTICS)`) não existem: dependem do
//!   `MetadataTable`, do cache e do coletor.
//! - `ThreadSafeRefCounted`: o `UnlinkedCodeBlock` guarda a tabela por valor; o compartilhamento com
//!   o `MetadataTable` entra junto com o `link`.

use crate::bytecode::instruction_stream::NUMBER_OF_BYTECODE_WITH_METADATA;
use crate::bytecode::op_metadata::{metadata_alignment, metadata_size, VALUE_PROFILE_SIZE};
use crate::bytecode::opcode::OpcodeID;
use crate::wtf::tri_state::TriState;

/// `UnlinkedMetadataTable::s_offsetTableEntries - 1`: um contador por opcode com metadata.
const METADATA_OPCODE_COUNT: usize = NUMBER_OF_BYTECODE_WITH_METADATA as usize;

/// `s_offsetTableEntries`: uma entrada a mais para o deslocamento final.
const OFFSET_TABLE_ENTRIES: usize = METADATA_OPCODE_COUNT + 1;

/// `s_offset16TableSize`: `roundUpToMultipleOf<sizeof(Offset32)>(s_offsetTableEntries * sizeof(Offset16))`.
const OFFSET16_TABLE_SIZE: u32 = (OFFSET_TABLE_ENTRIES as u32 * 2).div_ceil(4) * 4;

/// `s_offset32TableSize`: `roundUpToMultipleOf<s_maxMetadataAlignment>(s_offsetTableEntries * sizeof(Offset32))`.
const OFFSET32_TABLE_SIZE: u32 = (OFFSET_TABLE_ENTRIES as u32 * 4).div_ceil(8) * 8;

#[derive(Clone, Debug)]
pub struct UnlinkedMetadataTable {
    has_metadata: bool,
    is_finalized: bool,
    did_optimize: TriState,
    num_value_profiles: u32,
    /// O buffer de pré-processamento do C++: quantas entradas cada opcode já pediu.
    entry_counts: [u32; METADATA_OPCODE_COUNT],
    /// O `Offset32` de cada opcode e o final, preenchidos pelo `finalize` (sem o viés de
    /// `s_offset32TableSize`, que `offset_for`/`total_size` aplicam).
    offsets: [u32; OFFSET_TABLE_ENTRIES],
    is_32_bit: bool,
}

impl Default for UnlinkedMetadataTable {
    fn default() -> UnlinkedMetadataTable {
        UnlinkedMetadataTable::create()
    }
}

impl UnlinkedMetadataTable {
    pub const MAX_METADATA_ALIGNMENT: u32 = 8;

    /// `UnlinkedMetadataTable()` / `create()`.
    pub fn create() -> UnlinkedMetadataTable {
        UnlinkedMetadataTable {
            has_metadata: false,
            is_finalized: false,
            did_optimize: TriState::Indeterminate,
            num_value_profiles: 0,
            entry_counts: [0; METADATA_OPCODE_COUNT],
            offsets: [0; OFFSET_TABLE_ENTRIES],
            is_32_bit: false,
        }
    }

    /// `addEntry(OpcodeID)`: devolve o índice da nova entrada daquele opcode.
    pub fn add_entry(&mut self, opcode_id: OpcodeID) -> u32 {
        debug_assert!(!self.is_finalized && (opcode_id as usize) < METADATA_OPCODE_COUNT);
        self.has_metadata = true;
        let count = &mut self.entry_counts[opcode_id as usize];
        let index = *count;
        *count += 1;
        index
    }

    /// `addValueProfile()`.
    pub fn add_value_profile(&mut self) -> u32 {
        debug_assert!(!self.is_finalized);
        self.has_metadata = true;
        // Preinecrement because we want the first value profile's offset to be 1, since it's negative indexed.
        self.num_value_profiles += 1;
        self.num_value_profiles
    }

    /// `numEntries<Bytecode>()`.
    pub fn num_entries(&self, opcode_id: OpcodeID) -> u32 {
        debug_assert!((opcode_id as usize) < METADATA_OPCODE_COUNT);
        self.entry_counts[opcode_id as usize]
    }

    /// `finalize()`: calcula os deslocamentos em bytes de cada opcode (alinhados pelo `alignof` do
    /// `Metadata`), escolhe a tabela de 16 ou 32 bits e devolve `false` se a conta estoura 32 bits.
    #[must_use]
    pub fn finalize(&mut self) -> bool {
        debug_assert!(!self.is_finalized);
        self.is_finalized = true;
        if !self.has_metadata {
            return true;
        }

        // `CheckedUint32`: `None` é o `hasOverflowed()`.
        let mut checked_offset = Some(OFFSET16_TABLE_SIZE);
        for index in 0..METADATA_OPCODE_COUNT {
            let Some(current) = checked_offset else { break };
            let number_of_entries = self.entry_counts[index];
            self.offsets[index] = current; // We align when we access this.
            if number_of_entries == 0 {
                continue;
            }
            let opcode_id = OpcodeID::from_u32(index as u32);
            let alignment = metadata_alignment(opcode_id);
            debug_assert!(alignment <= Self::MAX_METADATA_ALIGNMENT);
            let aligned_offset = current.wrapping_add(alignment - 1) & !(alignment - 1);
            if aligned_offset < current {
                checked_offset = None;
                break;
            }
            checked_offset = number_of_entries
                .checked_mul(metadata_size(opcode_id))
                .and_then(|size| aligned_offset.checked_add(size));
        }

        // Each computed offset is stored as Offset32 (with an additional s_offset32TableSize bias
        // in the 32-bit layout) and totalSize() sums the value profile size with that biased offset.
        let end_offset = checked_offset.and_then(|offset| {
            let value_profile_size = self.num_value_profiles.checked_mul(VALUE_PROFILE_SIZE)?;
            offset.checked_add(OFFSET32_TABLE_SIZE)?.checked_add(value_profile_size)?;
            Some(offset)
        });
        let Some(end_offset) = end_offset else {
            self.has_metadata = false;
            self.is_32_bit = false;
            self.num_value_profiles = 0;
            self.entry_counts = [0; METADATA_OPCODE_COUNT];
            self.offsets = [0; OFFSET_TABLE_ENTRIES];
            return false; // Failure.
        };
        self.offsets[METADATA_OPCODE_COUNT] = end_offset;
        self.is_32_bit = end_offset > u16::MAX as u32;
        true
    }

    /// `m_is32Bit`.
    pub fn is_32_bit(&self) -> bool {
        self.is_32_bit
    }

    /// `offsetTableSize()`.
    pub fn offset_table_size(&self) -> u32 {
        debug_assert!(self.is_finalized);
        if self.is_32_bit {
            OFFSET16_TABLE_SIZE + OFFSET32_TABLE_SIZE
        } else {
            OFFSET16_TABLE_SIZE
        }
    }

    /// `totalSize()`: value profiles mais o deslocamento final.
    pub fn total_size(&self) -> u32 {
        debug_assert!(self.is_finalized);
        if !self.has_metadata {
            return 0;
        }
        self.num_value_profiles * VALUE_PROFILE_SIZE + self.offsets[METADATA_OPCODE_COUNT] + self.offset_bias()
    }

    /// O deslocamento (em bytes, desde o início do buffer de offsets) do primeiro metadata do
    /// opcode, como `MetadataTable::getOffset` o lê (com o viés da tabela de 32 bits).
    pub fn offset_for(&self, opcode_id: OpcodeID) -> u32 {
        debug_assert!(self.is_finalized && (opcode_id as usize) < METADATA_OPCODE_COUNT);
        self.offsets[opcode_id as usize] + self.offset_bias()
    }

    fn offset_bias(&self) -> u32 {
        if self.is_32_bit { OFFSET32_TABLE_SIZE } else { 0 }
    }

    pub fn is_finalized(&self) -> bool {
        self.is_finalized
    }

    pub fn has_metadata(&self) -> bool {
        self.has_metadata
    }

    pub fn num_value_profiles(&self) -> u32 {
        self.num_value_profiles
    }

    pub fn did_optimize(&self) -> TriState {
        self.did_optimize
    }

    pub fn set_did_optimize(&mut self, did_optimize: TriState) {
        self.did_optimize = did_optimize;
    }
}
