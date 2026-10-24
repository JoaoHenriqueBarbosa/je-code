//! Porte de `bytecode/BytecodeIndex.h`.
//!
//! `dump` é só depuração e `HashTableDeletedValueType` é detalhe das tabelas hash do WTF: o
//! valor "deletado" continua existindo como `deleted_value()`, com os mesmos bits.

/// `using Checkpoint = uint8_t`.
pub type Checkpoint = u8;

/// `static constexpr Checkpoint noCheckpoints = 0`.
pub const NO_CHECKPOINTS: Checkpoint = 0;

const INVALID_OFFSET: u32 = u32::MAX;

/// `class BytecodeIndex`: o deslocamento do bytecode e o checkpoint empacotados em 32 bits.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct BytecodeIndex {
    packed_bits: u32,
}

impl Default for BytecodeIndex {
    /// `BytecodeIndex() = default`: `m_packedBits { invalidOffset }`.
    fn default() -> BytecodeIndex {
        BytecodeIndex { packed_bits: INVALID_OFFSET }
    }
}

impl BytecodeIndex {
    pub const NUMBER_OF_CHECKPOINTS: u32 = 4;
    pub const CHECKPOINT_MASK: u32 = Self::NUMBER_OF_CHECKPOINTS - 1;
    /// `WTF::getMSBSet(numberOfCheckpoints)`.
    pub const CHECKPOINT_SHIFT: u32 = 2;

    /// `explicit BytecodeIndex(uint32_t bytecodeOffset, Checkpoint checkpoint = noCheckpoints)`.
    pub fn new(bytecode_offset: u32, checkpoint: Checkpoint) -> BytecodeIndex {
        BytecodeIndex { packed_bits: Self::pack(bytecode_offset, checkpoint) }
    }

    /// `BytecodeIndex(offset)` com o checkpoint padrão.
    pub fn from_offset(bytecode_offset: u32) -> BytecodeIndex {
        BytecodeIndex::new(bytecode_offset, NO_CHECKPOINTS)
    }

    pub fn offset(&self) -> u32 {
        self.packed_bits >> Self::CHECKPOINT_SHIFT
    }

    pub fn checkpoint(&self) -> Checkpoint {
        (self.packed_bits & Self::CHECKPOINT_MASK) as Checkpoint
    }

    pub fn as_bits(&self) -> u32 {
        self.packed_bits
    }

    /// `BytecodeIndex::deletedValue()`.
    pub fn deleted_value() -> BytecodeIndex {
        BytecodeIndex::from_bits(INVALID_OFFSET - 1)
    }

    pub fn is_hash_table_deleted_value(&self) -> bool {
        *self == Self::deleted_value()
    }

    pub fn from_bits(bits: u32) -> BytecodeIndex {
        BytecodeIndex { packed_bits: bits }
    }

    pub fn with_checkpoint(&self, checkpoint: Checkpoint) -> BytecodeIndex {
        BytecodeIndex::new(self.offset(), checkpoint)
    }

    /// `explicit operator bool()`: nem o inválido nem o "deletado". O C++ compara o "deletado"
    /// com `deletedValue().offset()` (os bits deslocados), e a comparação é preservada.
    pub fn is_valid(&self) -> bool {
        self.packed_bits != INVALID_OFFSET && self.packed_bits != Self::deleted_value().offset()
    }

    fn pack(bytecode_index: u32, checkpoint: Checkpoint) -> u32 {
        debug_assert!((checkpoint as u32) < Self::NUMBER_OF_CHECKPOINTS);
        debug_assert!((bytecode_index << Self::CHECKPOINT_SHIFT) >> Self::CHECKPOINT_SHIFT == bytecode_index);
        (bytecode_index << Self::CHECKPOINT_SHIFT) | checkpoint as u32
    }
}
