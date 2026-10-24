//! Porte de `bytecode/ExpressionInfo.h`, `ExpressionInfo.cpp` e `ExpressionInfoInlines.h`.
//!
//! Divergências (nenhuma é observável):
//!
//! - O C++ aloca o `ExpressionInfo` numa laje contígua (cabeçalho, capítulos, palavras) e o
//!   `Decoder` anda por ponteiros. Aqui o objeto guarda `Vec<Chapter>` e `Vec<EncodedInfo>` (as
//!   palavras normais seguidas das de extensão, como na laje) e o `Decoder` anda por índices; os
//!   métodos que leem palavras recebem a fatia. `recacheInfo` trata a realocação do vetor do
//!   `Encoder`, que com índices só precisa mover o fim das extensões.
//! - `createBorrowed`/`m_borrowedPayload` (carga que mora no cache de bytecode mapeado), `byteSize` e
//!   `byteSizeForGCPacing` (contabilidade de memória do coletor) e `dumpEncodedInfo`/`print`
//!   (depuração) não existem: o cache de bytecode e o ritmo do coletor ficam fora do porte.
//! - `Encoder::remap` recebe `Vec<u32>` e uma closure `FnMut(u32) -> u32`, o `template<RemapFunc>`.

use std::collections::HashMap;

use crate::bytecode::line_column::LineColumn;
use crate::parser::parser::IterationStatus;

/// `using InstPC = unsigned`.
pub type InstPC = u32;

/// `static constexpr InstPC maxInstPC`.
pub const MAX_INST_PC: InstPC = u32::MAX;

/// `enum class FieldID : uint8_t { InstPC, Divot, Start, End, Line, Column }`.
#[repr(u8)]
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum FieldID {
    InstPC = 0,
    Divot = 1,
    Start = 2,
    End = 3,
    Line = 4,
    Column = 5,
}

impl FieldID {
    /// `static_cast<FieldID>(bits)`: só os seis valores existem em palavras bem formadas.
    fn from_bits(bits: u32) -> FieldID {
        match bits {
            0 => FieldID::InstPC,
            1 => FieldID::Divot,
            2 => FieldID::Start,
            3 => FieldID::End,
            4 => FieldID::Line,
            5 => FieldID::Column,
            _ => unreachable!("FieldID inválido em palavra do ExpressionInfo"),
        }
    }
}

/// `ExpressionInfo::Chapter`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Chapter {
    pub start_inst_pc: InstPC,
    pub start_encoded_info_index: u32,
}

/// `ExpressionInfo::Entry`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Entry {
    pub inst_pc: InstPC,
    pub line_column: LineColumn,
    pub divot: u32,
    /// Relativo ao `divot`.
    pub start_offset: u32,
    /// Relativo ao `divot`.
    pub end_offset: u32,
}

impl Entry {
    /// `Entry::reset()`.
    pub fn reset(&mut self) {
        *self = Entry::default();
    }
}

/// `ExpressionInfo::EncodedInfo`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct EncodedInfo {
    pub value: u32,
}

impl EncodedInfo {
    pub fn is_abs_inst_pc(&self) -> bool {
        if self.value < (SPECIAL_HEADER << HEADER_SHIFT) {
            return false;
        }
        let is_multi = (self.value >> MULTI_BIT_SHIFT) & 1 != 0;
        !is_multi
    }

    pub fn is_extension(&self) -> bool {
        if self.value < (SPECIAL_HEADER << HEADER_SHIFT) {
            return false;
        }
        let is_multi = (self.value >> MULTI_BIT_SHIFT) & 1 != 0;
        is_multi
    }
}

const BITS_PER_WORD: u32 = 32;

// Número de bits de cada campo na codificação Basic.
const INST_PC_BITS: u32 = 5;
const DIVOT_BITS: u32 = 7;
const START_BITS: u32 = 6;
const END_BITS: u32 = 6;
const LINE_BITS: u32 = 3;
const COLUMN_BITS: u32 = 5;
const _: () = assert!(INST_PC_BITS + DIVOT_BITS + START_BITS + END_BITS + LINE_BITS + COLUMN_BITS == BITS_PER_WORD);

// Vieses dos valores com sinal.
const DIVOT_BIAS: u32 = (1 << DIVOT_BITS) / 2;
const LINE_BIAS: u32 = (1 << LINE_BITS) / 2;
const COLUMN_BIAS: u32 = (1 << COLUMN_BITS) / 2;

const INST_PC_SHIFT: u32 = BITS_PER_WORD - INST_PC_BITS;
const DIVOT_SHIFT: u32 = INST_PC_SHIFT - DIVOT_BITS;
const START_SHIFT: u32 = DIVOT_SHIFT - START_BITS;
const END_SHIFT: u32 = START_SHIFT - END_BITS;
const LINE_SHIFT: u32 = END_SHIFT - LINE_BITS;
const COLUMN_SHIFT: u32 = LINE_SHIFT - COLUMN_BITS;

const SPECIAL_HEADER: u32 = (1 << INST_PC_BITS) - 1;
const WIDE_HEADER: u32 = SPECIAL_HEADER - 1;

const MAX_INST_PC_VALUE: u32 = WIDE_HEADER - 1;
const MAX_BIASED_DIVOT_VALUE: u32 = (1 << DIVOT_BITS) - 1;
const MAX_START_VALUE: u32 = (1 << START_BITS) - 1;
const MAX_END_VALUE: u32 = (1 << END_BITS) - 1;
const MAX_BIASED_LINE_VALUE: u32 = (1 << LINE_BITS) - 1;

const SAME_AS_DIVOT_VALUE: u32 = (1 << COLUMN_BITS) - 1;
const MAX_BIASED_COLUMN_VALUE: u32 = SAME_AS_DIVOT_VALUE - 1;

// Número de bits nas codificações Wide e Special.
const SPECIAL_VALUE_BITS: u32 = 26;
const SINGLE_VALUE_BITS: u32 = 23;
const DUO_VALUE_BITS: u32 = 10;
const FULL_VALUE_BITS: u32 = 32;
const MULTI_SIZE_BITS: u32 = 5;
const FIELD_ID_BITS: u32 = 3;

const MAX_SPECIAL_VALUE: u32 = (1 << SPECIAL_VALUE_BITS) - 1;
const MAX_SINGLE_VALUE: u32 = (1 << SINGLE_VALUE_BITS) - 1;
const MAX_DUO_VALUE: u32 = (1 << DUO_VALUE_BITS) - 1;
const INVALID_FIELD_ID: u32 = (1 << FIELD_ID_BITS) - 1;

const MULTI_SIZE_MASK: u32 = (1 << MULTI_SIZE_BITS) - 1;
const FIELD_ID_MASK: u32 = (1 << FIELD_ID_BITS) - 1;

const HEADER_SHIFT: u32 = BITS_PER_WORD - INST_PC_BITS;
const MULTI_BIT_SHIFT: u32 = HEADER_SHIFT - 1;
const _: () = assert!(HEADER_SHIFT == 27);
const _: () = assert!(MULTI_BIT_SHIFT == 26);

const SPECIAL_VALUE_SHIFT: u32 = MULTI_BIT_SHIFT - SPECIAL_VALUE_BITS;
const _: () = assert!(SPECIAL_VALUE_SHIFT == 0);

const FIRST_FIELD_ID_SHIFT: u32 = MULTI_BIT_SHIFT - FIELD_ID_BITS;
const SINGLE_VALUE_SHIFT: u32 = FIRST_FIELD_ID_SHIFT - SINGLE_VALUE_BITS;
const _: () = assert!(SINGLE_VALUE_SHIFT == 0);

const DUO_FIRST_VALUE_SHIFT: u32 = FIRST_FIELD_ID_SHIFT - DUO_VALUE_BITS;
const DUO_SECOND_FIELD_ID_SHIFT: u32 = DUO_FIRST_VALUE_SHIFT - FIELD_ID_BITS;
const DUO_SECOND_VALUE_SHIFT: u32 = DUO_SECOND_FIELD_ID_SHIFT - DUO_VALUE_BITS;
const _: () = assert!(DUO_SECOND_VALUE_SHIFT == 0);

const MULTI_SIZE_SHIFT: u32 = FIRST_FIELD_ID_SHIFT - MULTI_SIZE_BITS;
const MULTI_FIRST_FIELD_SHIFT: u32 = MULTI_SIZE_SHIFT - FIELD_ID_BITS;

const NUMBER_OF_WORDS_BETWEEN_CHAPTERS: u32 = 10000;

/// `ExpressionInfo::cast<unsigned, bitCount>`: o campo de bits `unsigned x : bitCount`.
fn cast_unsigned(value: u32, bit_count: u32) -> u32 {
    if bit_count >= 32 {
        return value;
    }
    value & ((1u32 << bit_count) - 1)
}

/// `ExpressionInfo::cast<int, bitCount>`: o campo de bits `int x : bitCount` (com sinal).
fn cast_signed(value: u32, bit_count: u32) -> i32 {
    if bit_count >= 32 {
        return value as i32;
    }
    ((value << (32 - bit_count)) as i32) >> (32 - bit_count)
}

/// `Encoder::fits<unsigned, bitCount>`.
fn fits_unsigned(value: u32, bit_count: u32) -> bool {
    cast_unsigned(value, bit_count) == value
}

/// `Encoder::fits<int, bitCount>`.
fn fits_signed(value: i32, bit_count: u32) -> bool {
    cast_signed(value as u32, bit_count) == value
}

fn is_special(value: u32) -> bool {
    value >= (SPECIAL_HEADER << HEADER_SHIFT)
}

fn is_wide_or_special(value: u32) -> bool {
    value >= (WIDE_HEADER << HEADER_SHIFT)
}

/// `ExpressionInfo::Diff`.
#[derive(Clone, Copy, Debug, Default)]
struct Diff {
    inst_pc: u32,
    divot: i32,
    start: u32,
    end: u32,
    line: i32,
    column: i32,
}

impl Diff {
    /// `Diff::set<bitCount>`.
    fn set(&mut self, field_id: FieldID, value: u32, bit_count: u32) {
        match field_id {
            FieldID::InstPC => self.inst_pc = self.inst_pc.wrapping_add(cast_unsigned(value, bit_count)),
            FieldID::Divot => self.divot = self.divot.wrapping_add(cast_signed(value, bit_count)),
            FieldID::Start => self.start = self.start.wrapping_add(cast_unsigned(value, bit_count)),
            FieldID::End => self.end = self.end.wrapping_add(cast_unsigned(value, bit_count)),
            FieldID::Line => self.line = self.line.wrapping_add(cast_signed(value, bit_count)),
            FieldID::Column => self.column = self.column.wrapping_add(cast_signed(value, bit_count)),
        }
    }

    fn reset(&mut self) {
        *self = Diff::default();
    }
}

/// `Encoder::Wide::SortOrder`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
enum SortOrder {
    Single,
    Duo,
    Multi,
}

/// `Encoder::Wide`.
#[derive(Clone, Copy, Debug)]
struct Wide {
    value: u32,
    field_id: FieldID,
    order: SortOrder,
}

/// `ExpressionInfo::Encoder`.
#[derive(Debug, Default)]
pub struct Encoder {
    entry: Entry,
    current_chapter_start_index: u32,
    number_of_encoded_info_extensions: u32,
    expression_info_chapters: Vec<Chapter>,
    expression_info_encoded_info: Vec<EncodedInfo>,
}

impl Encoder {
    pub fn entry(&self) -> Entry {
        self.entry
    }

    fn encode_abs_inst_pc(abs_inst_pc: InstPC) -> EncodedInfo {
        let word = (SPECIAL_HEADER << HEADER_SHIFT) | ((abs_inst_pc & MAX_SPECIAL_VALUE) << SPECIAL_VALUE_SHIFT);
        EncodedInfo { value: word }
    }

    fn encode_extension(offset: u32) -> EncodedInfo {
        assert!((offset & MAX_SPECIAL_VALUE) == offset);
        let word = (SPECIAL_HEADER << HEADER_SHIFT) | (1 << MULTI_BIT_SHIFT) | ((offset & MAX_SPECIAL_VALUE) << SPECIAL_VALUE_SHIFT);
        EncodedInfo { value: word }
    }

    const fn encode_extension_end() -> EncodedInfo {
        let word = (WIDE_HEADER << HEADER_SHIFT) | (INVALID_FIELD_ID << FIRST_FIELD_ID_SHIFT);
        EncodedInfo { value: word }
    }

    fn encode_single(field_id: FieldID, value: u32) -> EncodedInfo {
        let word = (WIDE_HEADER << HEADER_SHIFT)
            | ((field_id as u32) << FIRST_FIELD_ID_SHIFT)
            | ((value & MAX_SINGLE_VALUE) << SINGLE_VALUE_SHIFT);
        EncodedInfo { value: word }
    }

    fn encode_duo(field_id1: FieldID, value1: u32, field_id2: FieldID, value2: u32) -> EncodedInfo {
        let word = (WIDE_HEADER << HEADER_SHIFT)
            | (1 << MULTI_BIT_SHIFT)
            | ((field_id1 as u32) << FIRST_FIELD_ID_SHIFT)
            | ((value1 & MAX_DUO_VALUE) << DUO_FIRST_VALUE_SHIFT)
            | ((field_id2 as u32) << DUO_SECOND_FIELD_ID_SHIFT)
            | ((value2 & MAX_DUO_VALUE) << DUO_SECOND_VALUE_SHIFT);
        EncodedInfo { value: word }
    }

    fn encode_multi_header(num_wides: u32, wides: &[Wide]) -> EncodedInfo {
        let mut word = (WIDE_HEADER << HEADER_SHIFT)
            | (1 << MULTI_BIT_SHIFT)
            | (INVALID_FIELD_ID << FIRST_FIELD_ID_SHIFT)
            | (num_wides << MULTI_SIZE_SHIFT);
        let mut field_shift = MULTI_FIRST_FIELD_SHIFT;
        for wide in wides.iter().take(num_wides as usize) {
            word |= (wide.field_id as u32) << field_shift;
            // Depois do último campo o C++ subtrai abaixo de zero (sem uso): módulo 2^32.
            field_shift = field_shift.wrapping_sub(FIELD_ID_BITS);
        }
        EncodedInfo { value: word }
    }

    fn encode_basic(diff: &Diff) -> EncodedInfo {
        debug_assert!(diff.inst_pc <= MAX_INST_PC_VALUE);
        debug_assert!(diff.start <= MAX_START_VALUE);
        debug_assert!(diff.end <= MAX_END_VALUE);
        let biased_divot = (diff.divot as u32).wrapping_add(DIVOT_BIAS);
        let biased_line = (diff.line as u32).wrapping_add(LINE_BIAS);
        let biased_column = if diff.column == i32::MAX {
            SAME_AS_DIVOT_VALUE
        } else {
            (diff.column as u32).wrapping_add(COLUMN_BIAS)
        };

        debug_assert!(biased_divot <= MAX_BIASED_DIVOT_VALUE);
        debug_assert!(biased_line <= MAX_BIASED_LINE_VALUE);
        debug_assert!(biased_column <= MAX_BIASED_COLUMN_VALUE || (diff.column == i32::MAX && biased_column == SAME_AS_DIVOT_VALUE));

        let word = (diff.inst_pc << INST_PC_SHIFT)
            | (biased_divot << DIVOT_SHIFT)
            | (diff.start << START_SHIFT)
            | (diff.end << END_SHIFT)
            | (biased_line << LINE_SHIFT)
            | (biased_column << COLUMN_SHIFT);
        EncodedInfo { value: word }
    }

    /// `Encoder::fits<bitCount>(Wide)`.
    fn wide_fits(wide: &Wide, bit_count: u32) -> bool {
        match wide.field_id {
            FieldID::InstPC | FieldID::Start | FieldID::End => fits_unsigned(wide.value, bit_count),
            FieldID::Divot | FieldID::Line | FieldID::Column => fits_signed(wide.value as i32, bit_count),
        }
    }

    /// `Encoder::adjustInstPC`.
    fn adjust_inst_pc(&mut self, info_index: usize, inst_pc_delta: u32) {
        /// Os dois rótulos do `goto` do C++.
        enum Next {
            /// `emitExtension`: troca a primeira palavra por uma Extension e cai na ilha.
            EmitExtension,
            /// `emitExtensionIsland`: a Extension já foi escrita.
            EmitExtensionIsland,
        }

        let mut first_value = self.expression_info_encoded_info[info_index].value;

        let header_bits = first_value >> HEADER_SHIFT;
        let is_multi = (first_value >> MULTI_BIT_SHIFT) & 1 != 0;
        let first_field_id_bits = (first_value >> FIRST_FIELD_ID_SHIFT) & FIELD_ID_MASK;

        let mut is_basic = false;

        let next = 'find: {
            if header_bits == SPECIAL_HEADER {
                // Handle AbsInstPC.
                let inst_pc = cast_unsigned(first_value, SPECIAL_VALUE_BITS);
                let updated_inst_pc = inst_pc.wrapping_add(inst_pc_delta);
                if fits_unsigned(updated_inst_pc, SPECIAL_VALUE_BITS) {
                    self.expression_info_encoded_info[info_index] = Self::encode_abs_inst_pc(updated_inst_pc);
                    return;
                }
                break 'find Next::EmitExtension;
            }

            if header_bits == WIDE_HEADER {
                if !is_multi {
                    // Handle SingleWide.
                    let field_id = FieldID::from_bits(first_field_id_bits);
                    let candidate_inst_pc = cast_unsigned(first_value, SINGLE_VALUE_BITS);
                    let updated_inst_pc = candidate_inst_pc.wrapping_add(inst_pc_delta);
                    if field_id == FieldID::InstPC && fits_unsigned(updated_inst_pc, SINGLE_VALUE_BITS) {
                        self.expression_info_encoded_info[info_index] = Self::encode_single(FieldID::InstPC, updated_inst_pc);
                        return;
                    }
                    break 'find Next::EmitExtension;
                }

                if first_field_id_bits != INVALID_FIELD_ID {
                    // Handle DuoWide.
                    let field_id = FieldID::from_bits(first_field_id_bits);
                    let candidate_inst_pc = cast_unsigned(first_value >> DUO_FIRST_VALUE_SHIFT, DUO_VALUE_BITS);
                    let updated_inst_pc = candidate_inst_pc.wrapping_add(inst_pc_delta);
                    if field_id == FieldID::InstPC && fits_unsigned(updated_inst_pc, DUO_VALUE_BITS) {
                        let field_id2 = FieldID::from_bits((first_value >> DUO_SECOND_FIELD_ID_SHIFT) & FIELD_ID_MASK);
                        let value2 = cast_unsigned(first_value >> DUO_SECOND_VALUE_SHIFT, DUO_VALUE_BITS);
                        self.expression_info_encoded_info[info_index] =
                            Self::encode_duo(FieldID::InstPC, updated_inst_pc, field_id2, value2);
                        return;
                    }
                    break 'find Next::EmitExtension;
                }

                // Handle MultiWide.
                let first_multi_field_id = FieldID::from_bits((first_value >> MULTI_FIRST_FIELD_SHIFT) & FIELD_ID_MASK);
                if first_multi_field_id == FieldID::InstPC {
                    let inst_pc = self.expression_info_encoded_info[info_index + 1].value;
                    let updated_inst_pc = inst_pc.wrapping_add(inst_pc_delta);
                    self.expression_info_encoded_info[info_index + 1].value = updated_inst_pc;
                    return;
                }

                // We can't just move the MultiWide header to the extension: we have to move the
                // whole MultiWide record (i.e. multiple words). The Decoder relies on them to
                // being contiguous.
                let number_of_fields = ((first_value >> MULTI_SIZE_SHIFT) & MULTI_SIZE_MASK) as usize;

                let location_of_extension_island = self.expression_info_encoded_info.len();
                self.expression_info_encoded_info.push(EncodedInfo { value: first_value }); // MultiWide header.
                for i in 1..number_of_fields {
                    let field_value = self.expression_info_encoded_info[info_index + i];
                    self.expression_info_encoded_info.push(field_value);
                    self.expression_info_encoded_info[info_index + i] = Self::encode_single(FieldID::InstPC, 0); // Replace with a no-op.
                }
                // Save the last field in firstValue, and let the extension emitter below append it.
                first_value = self.expression_info_encoded_info[info_index + number_of_fields].value;
                self.expression_info_encoded_info[info_index + number_of_fields] = Self::encode_single(FieldID::InstPC, 0); // Replace with a no-op.

                let extension_offset = (location_of_extension_island - info_index) as u32;
                self.expression_info_encoded_info[info_index] = Self::encode_extension(extension_offset);
                break 'find Next::EmitExtensionIsland;
            }

            // Handle Basic.
            let inst_pc = cast_unsigned(first_value >> INST_PC_SHIFT, INST_PC_BITS);
            let updated_inst_pc = inst_pc.wrapping_add(inst_pc_delta);
            if updated_inst_pc < MAX_INST_PC_VALUE {
                let mut replacement = first_value & ((1u32 << INST_PC_SHIFT) - 1);
                replacement |= updated_inst_pc << INST_PC_SHIFT;
                self.expression_info_encoded_info[info_index] = EncodedInfo { value: replacement };
                return;
            }
            is_basic = true;
            Next::EmitExtension
        };

        if let Next::EmitExtension = next {
            let extension_offset = (self.expression_info_encoded_info.len() - info_index) as u32;
            self.expression_info_encoded_info[info_index] = Self::encode_extension(extension_offset);
        }

        // Because the Basic word is used as a terminator for the current Entry,
        // if the firstValue is a Basic word, it needs to come last. Otherwise, we should
        // just emit firstValue first. AbsInstPC and MultiWide relies on this for correctness.
        if !is_basic {
            self.expression_info_encoded_info.push(EncodedInfo { value: first_value });
        }

        if fits_unsigned(inst_pc_delta, SINGLE_VALUE_BITS) {
            self.expression_info_encoded_info.push(Self::encode_single(FieldID::InstPC, inst_pc_delta));
        } else {
            // The wides array is really only to enable us to use encodeMultiHeader. Hence,
            // we don't really need to store instPCDelta as the value here. It can be any value
            // since it's not used. However, to avoid confusion, we'll just populate it consistently.
            let wides = [Wide { value: inst_pc_delta, field_id: FieldID::InstPC, order: SortOrder::Multi }];
            self.expression_info_encoded_info.push(Self::encode_multi_header(1, &wides));
            self.expression_info_encoded_info.push(EncodedInfo { value: inst_pc_delta });
        }

        if is_basic {
            // If we're terminating with the Basic word, then we don't need the
            // ExtensionEnd because the Basic word is an implied end.
            self.expression_info_encoded_info.push(EncodedInfo { value: first_value });
        } else {
            self.expression_info_encoded_info.push(Self::encode_extension_end());
        }
    }

    /// `Encoder::encode`.
    pub fn encode(&mut self, inst_pc: InstPC, divot: u32, start_offset: u32, end_offset: u32, line_column: LineColumn) {
        let mut num_wides: usize = 0;
        let mut wides = [Wide { value: 0, field_id: FieldID::InstPC, order: SortOrder::Multi }; 6];

        let mut append_wide = |id: FieldID, value: u32, wides: &mut [Wide; 6]| {
            wides[num_wides] = Wide { value, field_id: id, order: SortOrder::Multi };
            num_wides += 1;
        };

        let current_encoded_info_index = self.expression_info_encoded_info.len() as u32;
        let chapter_size = current_encoded_info_index - self.current_chapter_start_index;
        if chapter_size >= NUMBER_OF_WORDS_BETWEEN_CHAPTERS {
            self.expression_info_chapters.push(Chapter {
                start_inst_pc: inst_pc,
                start_encoded_info_index: current_encoded_info_index,
            });
            self.current_chapter_start_index = current_encoded_info_index;
            let abs_inst_pc = std::cmp::min(inst_pc, MAX_SINGLE_VALUE);
            self.expression_info_encoded_info.push(Self::encode_abs_inst_pc(abs_inst_pc));
            self.entry.reset();
            self.entry.inst_pc = abs_inst_pc;
        }

        let mut diff = Diff {
            inst_pc: inst_pc.wrapping_sub(self.entry.inst_pc),
            divot: divot.wrapping_sub(self.entry.divot) as i32,
            start: start_offset,
            end: end_offset,
            ..Diff::default()
        };

        diff.line = line_column.line.wrapping_sub(self.entry.line_column.line) as i32;
        if diff.line != 0 {
            self.entry.line_column.column = 0;
        }

        diff.column = line_column.column.wrapping_sub(self.entry.line_column.column) as i32;

        let same_divot_and_column_diff = diff.column == diff.divot;

        // Divot, line, and column diffs can negative values. To maximize the chance that they fit
        // in a Basic word, we apply a bias to these values. InstPC is always monotonically increasing
        // i.e. it's diff is always positive and unsigned. Start and end are already relative to divot
        // i.e. their diffs are always positive and unsigned. Hence, instPC, start, and end do not
        // require a bias.

        // Encode header:
        if diff.inst_pc > MAX_INST_PC_VALUE {
            append_wide(FieldID::InstPC, diff.inst_pc, &mut wides);
            diff.inst_pc = 0;
        }

        // Encode divot:
        if (diff.divot as u32).wrapping_add(DIVOT_BIAS) > MAX_BIASED_DIVOT_VALUE {
            append_wide(FieldID::Divot, diff.divot as u32, &mut wides);
            diff.divot = 0;
        }

        // Encode start:
        if diff.start > MAX_START_VALUE {
            append_wide(FieldID::Start, diff.start, &mut wides);
            diff.start = 0;
        }

        // Encode end:
        if diff.end > MAX_END_VALUE {
            append_wide(FieldID::End, diff.end, &mut wides);
            diff.end = 0;
        }

        // Encode line:
        if (diff.line as u32).wrapping_add(LINE_BIAS) > MAX_BIASED_LINE_VALUE {
            append_wide(FieldID::Line, diff.line as u32, &mut wides);
            diff.line = 0;
        }

        // Encode column:
        if same_divot_and_column_diff {
            diff.column = i32::MAX;
        } else if (diff.column as u32).wrapping_add(COLUMN_BIAS) > MAX_BIASED_COLUMN_VALUE {
            append_wide(FieldID::Column, diff.column as u32, &mut wides);
            diff.column = 0;
        }

        self.entry.inst_pc = inst_pc;
        self.entry.divot = divot;
        self.entry.line_column = line_column;

        // Canonicalize the wide EncodedInfo.
        {
            let mut last_duo_index = num_wides;
            let mut num_duo_wides = 0;

            // We want to process the InstPC wide (if present) last. This enables an InstPC wide to be emitted
            // first (if possible) to simplify the remap logic in adjustInst(). adjustInst() assumes that
            // the InstPC wide (if present) will likely be in the first word.
            for i in (0..num_wides).rev() {
                let wide = wides[i];
                if Self::wide_fits(&wide, DUO_VALUE_BITS) {
                    wides[i].order = SortOrder::Duo;
                    num_duo_wides += 1;
                    last_duo_index = i;
                } else if Self::wide_fits(&wide, SINGLE_VALUE_BITS) {
                    wides[i].order = SortOrder::Single;
                } else {
                    wides[i].order = SortOrder::Multi;
                }
            }

            if num_duo_wides & 1 != 0 {
                wides[last_duo_index].order = SortOrder::Single;
            }

            wides[..num_wides].sort_by(|a, b| a.order.cmp(&b.order).then(a.field_id.cmp(&b.field_id)));
        }

        // Emit the wide EncodedInfo.
        let mut i = 0;
        while i < num_wides {
            let wide = wides[i];

            if wide.order == SortOrder::Single {
                self.expression_info_encoded_info.push(Self::encode_single(wide.field_id, wide.value));
                i += 1;
                continue;
            }

            if wide.order == SortOrder::Duo {
                i += 1;
                let wide2 = wides[i];
                debug_assert!(Self::wide_fits(&wide, DUO_VALUE_BITS));
                debug_assert!(Self::wide_fits(&wide2, DUO_VALUE_BITS));
                self.expression_info_encoded_info
                    .push(Self::encode_duo(wide.field_id, wide.value, wide2.field_id, wide2.value));
                i += 1;
                continue;
            }

            debug_assert!(wide.order == SortOrder::Multi);
            let remaining_wides = num_wides - i;
            self.expression_info_encoded_info
                .push(Self::encode_multi_header(remaining_wides as u32, &wides[i..num_wides]));
            while i < num_wides {
                self.expression_info_encoded_info.push(EncodedInfo { value: wides[i].value });
                i += 1;
            }
        }

        self.expression_info_encoded_info.push(Self::encode_basic(&diff));
    }

    /// `Encoder::remap` (de `ExpressionInfoInlines.h`).
    pub fn remap(&mut self, mut adjustment_label_points: Vec<u32>, mut remap_func: impl FnMut(u32) -> u32) {
        if adjustment_label_points.is_empty() {
            return; // Nothing to adjust.
        }

        // Pad the end with a value that exceeds all other bytecodeIndexes.
        // This way, currentLabel below will always has a meaningful value
        // to compare instPC against.
        adjustment_label_points.push(u32::MAX);

        let mut decoder = Decoder::for_encoded_info(&self.expression_info_encoded_info);
        let num_encoded_info = self.expression_info_encoded_info.len();

        // These are the types of adjustments that we need to handle:
        // 1. bytecode got inserted before a LabelPoint.
        // 2. bytecode got inserted after the LabelPoint.
        // 3. bytecode got deleted after the LabelPoint.
        //
        // This means that we only need to do a remap of InstPC for the following:
        //
        // a. the EncodedInfo Entry at a LabelPoint InstPC (due to (1) above).
        // b. the EncodedInfo Entry right after the LabelPoint InstPC (due to (2) and (3) above).
        // c. the EncodedInfo Entry that start with an AbsInstPC.
        //
        // Os comentários detalhados de cada caso estão em `ExpressionInfoInlines.h`: como as palavras
        // são deltas, uma correção aplicada vale para as seguintes; só o AbsInstPC, que é absoluto,
        // precisa ser refeito sempre.

        let mut adjustment_index = 0;
        let mut current_label: InstPC = adjustment_label_points[adjustment_index];
        let mut need_to_adjust_label_after = false;
        let mut cummulative_delta: u32 = 0;

        while decoder.decode(&self.expression_info_encoded_info, None) != IterationStatus::Done {
            let current_info_index = decoder.current_info();
            let is_abs_inst_pc = self.expression_info_encoded_info[current_info_index].is_abs_inst_pc();
            let mut need_remap = is_abs_inst_pc;

            let inst_pc = decoder.inst_pc();
            if inst_pc >= current_label {
                need_to_adjust_label_after = true;
                need_remap = true;
                adjustment_index += 1;
                current_label = adjustment_label_points[adjustment_index];
            } else if need_to_adjust_label_after {
                need_to_adjust_label_after = false;
                need_remap = true;
            }

            if need_remap {
                if is_abs_inst_pc {
                    cummulative_delta = 0;
                }
                let inst_pc_delta = remap_func(inst_pc).wrapping_sub(inst_pc).wrapping_sub(cummulative_delta);
                if inst_pc_delta != 0 || is_abs_inst_pc {
                    self.adjust_inst_pc(current_info_index, inst_pc_delta);

                    // adjustInstPC() may have resized and reallocated m_expressionInfoEncodedInfo.
                    // So, we need to re-compute endInfo. info will be re-computed at the top of the loop.
                    decoder.recache_info(&self.expression_info_encoded_info);
                    cummulative_delta = cummulative_delta.wrapping_add(inst_pc_delta);
                }
            }
        }
        self.number_of_encoded_info_extensions = (self.expression_info_encoded_info.len() - num_encoded_info) as u32;

        // Now, let's remap the Chapter startInstPCs. Their startEncodedInfoIndex will not change because
        // the above remap algorithm does in place remapping.
        for chapter in &mut self.expression_info_chapters {
            chapter.start_inst_pc = remap_func(chapter.start_inst_pc);
        }
    }

    /// `Encoder::createExpressionInfo`: consome os vetores do codificador (`WTF::move`).
    pub fn create_expression_info(&mut self) -> Box<ExpressionInfo> {
        let chapters = std::mem::take(&mut self.expression_info_chapters);
        let encoded_info = std::mem::take(&mut self.expression_info_encoded_info);
        Box::new(ExpressionInfo::new(chapters, encoded_info, self.number_of_encoded_info_extensions))
    }
}

/// `ExpressionInfo::Decoder`.
#[derive(Debug, Default)]
pub struct Decoder {
    entry: Entry,
    /// Fim das palavras normais (índice).
    end_info: usize,
    /// Fim das palavras de extensão (índice).
    end_extension_info: usize,
    current_info: usize,
    next_info: usize,
    has_decoded_first_entry: bool,
}

impl Decoder {
    /// `Decoder(const ExpressionInfo&)`.
    pub fn new(expression_info: &ExpressionInfo) -> Decoder {
        Decoder {
            entry: Entry::default(),
            end_info: expression_info.number_of_encoded_info as usize,
            end_extension_info: (expression_info.number_of_encoded_info + expression_info.number_of_encoded_info_extensions) as usize,
            current_info: 0,
            next_info: 0,
            has_decoded_first_entry: false,
        }
    }

    /// `Decoder(Vector<EncodedInfo>&)`: só o `Encoder::remap` usa.
    pub fn for_encoded_info(encoded_info_vector: &[EncodedInfo]) -> Decoder {
        Decoder {
            entry: Entry::default(),
            end_info: encoded_info_vector.len(),
            end_extension_info: encoded_info_vector.len(),
            current_info: 0,
            next_info: 0,
            has_decoded_first_entry: false,
        }
    }

    /// `recacheInfo`: com índices só o fim das extensões se move quando o vetor cresce.
    pub fn recache_info(&mut self, encoded_info_vector: &[EncodedInfo]) {
        if self.end_info == encoded_info_vector.len() {
            return; // Did not resize i.e nothing changed.
        }
        self.end_extension_info = encoded_info_vector.len();
    }

    /// `currentInfo()`: o índice da primeira palavra da entrada decodificada.
    pub fn current_info(&self) -> usize {
        self.current_info
    }

    /// `setNextInfo`: pula para o começo de um capítulo.
    pub fn set_next_info(&mut self, info: usize) {
        self.next_info = info;
    }

    pub fn entry(&self) -> Entry {
        self.entry
    }

    pub fn set_entry(&mut self, entry: Entry) {
        self.entry = entry;
    }

    pub fn inst_pc(&self) -> InstPC {
        self.entry.inst_pc
    }

    pub fn divot(&self) -> u32 {
        self.entry.divot
    }

    pub fn start_offset(&self) -> u32 {
        self.entry.start_offset
    }

    pub fn end_offset(&self) -> u32 {
        self.entry.end_offset
    }

    pub fn line_column(&self) -> LineColumn {
        self.entry.line_column
    }

    /// `Decoder::decode`. `infos` são as palavras normais seguidas das de extensão.
    pub fn decode(&mut self, infos: &[EncodedInfo], target_inst_pc: Option<InstPC>) -> IterationStatus {
        self.current_info = self.next_info; // Go decode the next Entry.

        debug_assert!(self.current_info <= self.end_info);
        debug_assert!(self.end_info <= self.end_extension_info);
        let mut current_info = self.current_info;
        if current_info == self.end_info {
            return IterationStatus::Done;
        }

        let mut diff = Diff::default();

        let mut value = infos[current_info].value;

        let mut saved_info: Option<usize> = None;
        let mut has_abs_inst_pc = false;
        let mut current_inst_pc = self.entry.inst_pc;

        // Decode wide words.
        while is_wide_or_special(value) {
            let special = is_special(value);
            let is_multi = (value >> MULTI_BIT_SHIFT) & 1 != 0;
            let first_field_id_bits = (value >> FIRST_FIELD_ID_SHIFT) & FIELD_ID_MASK;

            if special {
                if is_multi {
                    // Decode Extension word.
                    let extension_offset = cast_unsigned(value, SPECIAL_VALUE_BITS) as usize;
                    saved_info = Some(current_info);
                    current_info = current_info + extension_offset - 1; // -1 to compensate for the increment below.
                    debug_assert!(current_info + 1 >= self.end_info);
                    debug_assert!(current_info < self.end_extension_info);
                } else {
                    // Decode AbsInstPC word.
                    debug_assert!(current_info == self.current_info);

                    // We can't call m_entry.reset() here because we always scan up to the entry
                    // above the one that we're looking for before declaring Done. Hence, we have
                    // to defer any changes to m_entry until we know that the current entry does
                    // not exceed what we're looking for, and that we can commit it.
                    has_abs_inst_pc = true;
                    current_inst_pc = 0;
                    diff.reset();
                    diff.set(FieldID::InstPC, value, SPECIAL_VALUE_BITS);
                }
            } else if first_field_id_bits == INVALID_FIELD_ID && !is_multi {
                // Decode ExtensionEnd word.
                debug_assert!(saved_info.is_some());
                if let Some(saved) = saved_info {
                    current_info = saved;
                }
                // We need to clear savedInfo to indicate that we terminated the Extension with
                // ExtensionEnd. Otherwise, we need to restore currentInfo after we decode the Basic word
                // terminator.
                saved_info = None;
            } else if first_field_id_bits == INVALID_FIELD_ID {
                // Decode MultiWide word.
                let number_of_fields = (value >> MULTI_SIZE_SHIFT) & MULTI_SIZE_MASK;
                let mut field_shift = MULTI_FIRST_FIELD_SHIFT;
                for _ in 0..number_of_fields {
                    current_info += 1;
                    let field_id = FieldID::from_bits((value >> field_shift) & FIELD_ID_MASK);
                    diff.set(field_id, infos[current_info].value, FULL_VALUE_BITS);
                    field_shift = field_shift.wrapping_sub(FIELD_ID_BITS);
                }
            } else if is_multi {
                // Decode DuoWide word.
                let field_id1 = FieldID::from_bits(first_field_id_bits);
                let field_id2 = FieldID::from_bits((value >> DUO_SECOND_FIELD_ID_SHIFT) & FIELD_ID_MASK);
                diff.set(field_id1, value >> DUO_FIRST_VALUE_SHIFT, DUO_VALUE_BITS);
                diff.set(field_id2, value >> DUO_SECOND_VALUE_SHIFT, DUO_VALUE_BITS);
            } else {
                // Decode SingleWide word.
                diff.set(FieldID::from_bits(first_field_id_bits), value, SINGLE_VALUE_BITS);
            }

            current_info += 1;
            value = infos[current_info].value;
        }

        // Decode Basic word.
        // We check the bounds against m_endExtensionInfo here because the Basic word may be in
        // the extensions section.
        debug_assert!(current_info < self.end_extension_info);

        diff.inst_pc = diff.inst_pc.wrapping_add(value >> INST_PC_SHIFT);
        current_inst_pc = current_inst_pc.wrapping_add(diff.inst_pc);

        let mut status = IterationStatus::Continue;

        // We want to find the entry whose InstPC is below the targetInstPC but not to exceed it.
        // This means by necessity, we must always decode the next one above it before we
        // know that we're done. If the current decode exceeds the target, then we need to
        // abort immediately and not commit any changes to m_entry.
        //
        // The only exception to this is that we need to at least decode 1 entry before
        // calling it quits. This only applies to opcodes at the start of the function before
        // the first ExpressionInfo entry. Our historical convention is to map those to the
        // first entry. The m_hasDecodedFirstEntry flag helps us achieve this.

        if target_inst_pc.is_some_and(|target| self.has_decoded_first_entry && current_inst_pc > target) {
            // We're done because we have reached our targetInstPC.
            status = IterationStatus::Done;
        } else {
            self.has_decoded_first_entry = true;

            if has_abs_inst_pc {
                self.entry.reset();
            }
            self.entry.inst_pc = current_inst_pc;

            diff.divot = diff.divot.wrapping_add(cast_signed((value >> DIVOT_SHIFT).wrapping_sub(DIVOT_BIAS), DIVOT_BITS));
            self.entry.divot = self.entry.divot.wrapping_add(diff.divot as u32);

            // Unlike other values, startOffset and endOffset are always relative
            // to the divot. Hence, they are never cummulative relative to the last expression
            // info entry.
            const START_MASK: u32 = (1 << START_BITS) - 1;
            diff.start = diff.start.wrapping_add((value >> START_SHIFT) & START_MASK);
            self.entry.start_offset = diff.start; // Not cummulative.

            const END_MASK: u32 = (1 << END_BITS) - 1;
            diff.end = diff.end.wrapping_add((value >> END_SHIFT) & END_MASK);
            self.entry.end_offset = diff.end; // Not cummulative.

            diff.line = diff.line.wrapping_add(cast_signed((value >> LINE_SHIFT).wrapping_sub(LINE_BIAS), LINE_BITS));
            if diff.line != 0 {
                self.entry.line_column.column = 0;
            }
            self.entry.line_column.line = self.entry.line_column.line.wrapping_add(diff.line as u32);

            const COLUMN_MASK: u32 = (1 << COLUMN_BITS) - 1;

            let column_field = (value >> COLUMN_SHIFT) & COLUMN_MASK;
            diff.column = diff.column.wrapping_add(if column_field == SAME_AS_DIVOT_VALUE {
                diff.divot
            } else {
                cast_signed(column_field.wrapping_sub(COLUMN_BIAS), COLUMN_BITS)
            });
            self.entry.line_column.column = self.entry.line_column.column.wrapping_add(diff.column as u32);
        }

        if let Some(saved) = saved_info {
            // We got here because we are terminating an Extension with a Basic word.
            // So, we have to restore currentInfo for the next Entry.
            current_info = saved;
        }

        current_info += 1;
        self.next_info = current_info; // This is where the next Entry to decode will start.
        status
    }
}

/// `class ExpressionInfo`.
#[derive(Debug)]
pub struct ExpressionInfo {
    cached_line_columns: HashMap<InstPC, LineColumn>,
    chapters: Vec<Chapter>,
    /// `encodedInfo[numberOfEncodedInfo + numberOfEncodedInfoExtensions]`.
    encoded_info: Vec<EncodedInfo>,
    number_of_encoded_info: u32,
    number_of_encoded_info_extensions: u32,
}

impl ExpressionInfo {
    /// `ExpressionInfo(Vector<Chapter>&&, Vector<EncodedInfo>&&, unsigned numberOfEncodedInfoExtensions)`.
    fn new(chapters: Vec<Chapter>, encoded_info: Vec<EncodedInfo>, number_of_encoded_info_extensions: u32) -> ExpressionInfo {
        ExpressionInfo {
            cached_line_columns: HashMap::new(),
            number_of_encoded_info: encoded_info.len() as u32 - number_of_encoded_info_extensions,
            number_of_encoded_info_extensions,
            chapters,
            encoded_info,
        }
    }

    pub fn is_empty(&self) -> bool {
        self.number_of_encoded_info == 0
    }

    pub fn chapters(&self) -> &[Chapter] {
        &self.chapters
    }

    pub fn encoded_info(&self) -> &[EncodedInfo] {
        &self.encoded_info
    }

    /// `ExpressionInfo::Decoder` sobre este objeto: o `UnlinkedCodeBlock::dumpExpressionInfo` e
    /// quem percorre todas as entradas usam o par decodificador e `encoded_info()`.
    pub fn decoder(&self) -> Decoder {
        Decoder::new(self)
    }

    /// `lineColumnForInstPC`.
    pub fn line_column_for_inst_pc(&mut self, inst_pc: InstPC) -> LineColumn {
        if let Some(line_column) = self.cached_line_columns.get(&inst_pc) {
            return *line_column;
        }

        let entry = self.entry_for_inst_pc(inst_pc);
        self.cached_line_columns.entry(inst_pc).or_insert(entry.line_column);
        entry.line_column
    }

    /// `findChapterEncodedInfoJustBelow`: o índice da primeira palavra do capítulo.
    fn find_chapter_encoded_info_just_below(&self, inst_pc: InstPC) -> usize {
        let mut low: usize = 0;
        let mut high: usize = self.chapters.len();
        while low < high {
            let mid = low + (high - low) / 2; // std::midpoint
            if self.chapters[mid].start_inst_pc <= inst_pc {
                low = mid + 1;
            } else {
                high = mid;
            }
        }

        let mut start_index = 0;
        if low != 0 {
            let chapter = &self.chapters[low - 1];
            start_index = chapter.start_encoded_info_index as usize;
        }
        start_index
    }

    /// `entryForInstPC`.
    pub fn entry_for_inst_pc(&self, inst_pc: InstPC) -> Entry {
        let mut decoder = Decoder::new(self);

        let chapter_start = self.find_chapter_encoded_info_just_below(inst_pc);
        decoder.set_next_info(chapter_start);
        while decoder.decode(&self.encoded_info, Some(inst_pc)) != IterationStatus::Done {}
        decoder.entry()
    }
}
