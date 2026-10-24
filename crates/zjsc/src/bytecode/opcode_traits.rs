//! Porte de `JSOpcodeTraits` (`bytecode/Instruction.h`) e do contrato `Traits::OpcodeTraits` que
//! `BytecodeGeneratorBase` e `OpcodeIDWidthBySize` (`OpcodeSize.h`) pedem.
//!
//! O `OpcodeID` do JS é `enum OpcodeID : unsigned`: o `Fits<OpcodeID, size>` é o do tipo
//! subjacente, e o `write_opcode` o chama com a largura do opcode (`opcode_id_size`), que no JS é
//! sempre `Narrow` (`maxJSOpcodeIDWidth`).

use crate::bytecode::fits::{Fits, FitsFrom, Fitted};
use crate::bytecode::instruction_stream::{
    BYTECODE_CHECKPOINT_COUNT_TABLE, NUMBER_OF_BYTECODE_WITH_CHECKPOINTS, NUMBER_OF_BYTECODE_WITH_METADATA,
};
use crate::bytecode::opcode::{OpcodeID, OPCODE_LENGTHS, OPCODE_NAMES};
use crate::bytecode::opcode_size::{opcode_id_width_by_size, OpcodeSize, MAX_JS_OPCODE_ID_WIDTH};

/// `Traits::OpcodeTraits`: os prefixos wide e a largura do opcode por tamanho
/// (`OpcodeIDWidthBySize<OpcodeTraits, size>::opcodeIDSize`).
pub trait OpcodeTraits {
    type OpcodeID: Fits + Copy;

    /// `Traits::maxOpcodeIDWidth`.
    const MAX_OPCODE_ID_WIDTH: OpcodeSize;

    /// `Traits::wide16`.
    fn wide16() -> Self::OpcodeID;
    /// `Traits::wide32`.
    fn wide32() -> Self::OpcodeID;

    /// `OpcodeIDWidthBySize<Traits, size>::opcodeIDSize`.
    fn opcode_id_size(size: OpcodeSize) -> OpcodeSize {
        opcode_id_width_by_size(size, Self::MAX_OPCODE_ID_WIDTH)
    }
}

/// `struct JSOpcodeTraits`.
#[derive(Debug)]
pub struct JSOpcodeTraits;

impl JSOpcodeTraits {
    /// `numberOfBytecodesWithCheckpoints` (o número do primeiro opcode sem checkpoints).
    pub const NUMBER_OF_BYTECODES_WITH_CHECKPOINTS: u16 = NUMBER_OF_BYTECODE_WITH_CHECKPOINTS;
    /// `numberOfBytecodesWithMetadata` (o número do primeiro opcode sem metadata).
    pub const NUMBER_OF_BYTECODES_WITH_METADATA: u16 = NUMBER_OF_BYTECODE_WITH_METADATA;

    /// `opcodeLengths`.
    pub fn opcode_lengths() -> &'static [u8] {
        &OPCODE_LENGTHS
    }

    /// `opcodeNames`.
    pub fn opcode_names() -> &'static [&'static str] {
        &OPCODE_NAMES
    }

    /// `checkpointCountTable`.
    pub fn checkpoint_count_table() -> &'static [u32] {
        &BYTECODE_CHECKPOINT_COUNT_TABLE
    }
}

// static_assert(numberOfBytecodesWithCheckpoints <= numberOfBytecodesWithMetadata)
const _: () = assert!(NUMBER_OF_BYTECODE_WITH_CHECKPOINTS <= NUMBER_OF_BYTECODE_WITH_METADATA);

impl OpcodeTraits for JSOpcodeTraits {
    type OpcodeID = OpcodeID;

    const MAX_OPCODE_ID_WIDTH: OpcodeSize = MAX_JS_OPCODE_ID_WIDTH;

    fn wide16() -> OpcodeID {
        OpcodeID::op_wide16
    }

    fn wide32() -> OpcodeID {
        OpcodeID::op_wide32
    }
}

/// `Fits<OpcodeID, size> : Fits<unsigned, size>`.
impl Fits for OpcodeID {
    fn check(&self, size: OpcodeSize) -> bool {
        (*self as u32).check(size)
    }

    fn convert(&self, size: OpcodeSize) -> Fitted {
        (*self as u32).convert(size)
    }
}

/// `Fits<OpcodeID, size>::convert(TargetType)`: o `static_cast<OpcodeID>` do valor sem sinal.
impl FitsFrom for OpcodeID {
    fn from_fitted(fitted: Fitted) -> Self {
        OpcodeID::from_u32(u32::from_fitted(fitted))
    }
}
