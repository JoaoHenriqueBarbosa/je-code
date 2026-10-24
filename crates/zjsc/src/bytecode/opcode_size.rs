//! Porte de `bytecode/OpcodeSize.h`.
//!
//! `TypeBySize<size>` do C++ (tipo inteiro por largura) vira o método `bytes`: quem lê ou escreve
//! o operando escolhe `u8`/`u16`/`u32` a partir dele.

/// `enum OpcodeSize`: o valor numérico é a largura em bytes do operando.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[repr(u8)]
pub enum OpcodeSize {
    Narrow = 1,
    Wide16 = 2,
    Wide32 = 4,
}

impl OpcodeSize {
    /// `static_cast<size_t>(size)`: largura do operando em bytes.
    pub const fn bytes(self) -> usize {
        self as usize
    }

    /// `PaddingBySize<size>::value`: o prefixo `op_wide16`/`op_wide32` ocupa um byte.
    pub const fn padding(self) -> u8 {
        match self {
            OpcodeSize::Narrow => 0,
            OpcodeSize::Wide16 | OpcodeSize::Wide32 => 1,
        }
    }
}

/// `maxJSOpcodeIDWidth` (`Opcode.h`): todo `OpcodeID` do JS cabe em um byte.
pub const MAX_JS_OPCODE_ID_WIDTH: OpcodeSize = OpcodeSize::Narrow;

/// `OpcodeIDWidthBySize<Traits, size>::opcodeIDSize`: com `maxOpcodeIDWidth == Narrow` o opcode é
/// sempre de um byte; senão, `Wide16` e `Wide32` guardam o opcode em dois bytes.
pub const fn opcode_id_width_by_size(size: OpcodeSize, max_opcode_id_width: OpcodeSize) -> OpcodeSize {
    match size {
        OpcodeSize::Narrow => OpcodeSize::Narrow,
        OpcodeSize::Wide16 | OpcodeSize::Wide32 => match max_opcode_id_width {
            OpcodeSize::Narrow => OpcodeSize::Narrow,
            _ => OpcodeSize::Wide16,
        },
    }
}
