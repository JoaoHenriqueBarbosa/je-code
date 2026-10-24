//! Porte de `bytecode/PreciseJumpTargets.h`, `PreciseJumpTargets.cpp` e `PreciseJumpTargetsInlines.h`.
//!
//! O `Block` dos templates do C++ é o trait `JumpTargetBlock`, implementado pelo
//! `UnlinkedCodeBlockGenerator` e pelo `CodeBlock`; a reescrita de saltos (`update...`) só existe
//! sobre o gerador, como no C++. O `SWITCH_JMP` do C++ (uma macro
//! sobre a lista de opcodes de salto) é `jump_ops!`.
//!
//! O C++ lê a tabela de saltos fora de linha pelo `codeBlock` ou por um `HashMap` (o que o
//! `BytecodeRewriter::adjustJumpTargets` passa depois de trocar o mapa do bloco); aqui isso é
//! `OutOfLineJumpSource`. Como as tabelas de `switch` são lidas pelo acessor mutável do gerador, as
//! funções recebem `&mut UnlinkedCodeBlockGenerator`.

use crate::bytecode::bytecode_ops::{
    BoundLabel, OpJbelow, OpJbeloweq, OpJeq, OpJeqNull, OpJeqPtr, OpJfalse, OpJgreater, OpJgreatereq, OpJless, OpJlesseq, OpJmp,
    OpJneq, OpJneqNull, OpJneqPtr, OpJngreater, OpJngreatereq, OpJnless, OpJnlesseq, OpJnstricteq, OpJnundefinedOrNull,
    OpJstricteq, OpJtrue, OpJundefinedOrNull, OpSwitchChar, OpSwitchImm, OpSwitchString,
};
use crate::bytecode::instruction_stream::{InstructionStream, MutableRef, Ref};
use crate::bytecode::opcode::OpcodeID;
use crate::bytecode::handler_info::HandlerInfoBase;
use crate::bytecode::unlinked_code_block::{UnlinkedSimpleJumpTable, UnlinkedStringJumpTable};
use crate::bytecode::unlinked_code_block_generator::{OutOfLineJumpTargets, UnlinkedCodeBlockGenerator};
use crate::bytecompiler::label::LabelGenerator;

/// O que os templates `computePreciseJumpTargets<Block>`, `extractStoredJumpTargetsForInstruction<Block>`
/// e `BytecodeBasicBlock::computeImpl<Block>` leem do bloco.
pub trait JumpTargetBlock {
    /// `outOfLineJumpOffset(offset)`.
    fn jump_offset_out_of_line(&self, bytecode_offset: u32) -> i32;
    /// `unlinkedSwitchJumpTable(index)`, dado a `function`.
    fn with_switch_jump_table(&mut self, table_index: usize, function: &mut dyn FnMut(&mut UnlinkedSimpleJumpTable));
    /// `unlinkedStringSwitchJumpTable(index)`, dado a `function`.
    fn with_string_switch_jump_table(&mut self, table_index: usize, function: &mut dyn FnMut(&mut UnlinkedStringJumpTable));
    /// `numberOfExceptionHandlers()`.
    fn exception_handler_count(&self) -> usize;
    /// `exceptionHandler(index)`.
    fn exception_handler_base(&mut self, index: usize) -> HandlerInfoBase;
    /// `handlerForBytecodeIndex(BytecodeIndex(offset), RequiredHandler::AnyHandler)->target`.
    fn any_handler_target(&self, bytecode_offset: u32) -> Option<u32>;
}

/// Os rótulos do bytecode já resolvidos são sempre por deslocamento (`BoundType::Offset`), então o
/// gerador nunca é consultado.
struct OffsetOnly;

impl LabelGenerator for OffsetOnly {
    fn writer_position(&self) -> i32 {
        unreachable!("rótulo de bytecode já gravado é sempre por deslocamento")
    }
}

/// De onde vem o deslocamento de um salto cujo operando é zero (o salto fora de linha):
/// `jumpTargetForInstruction(Block*, ...)` lê do bloco e
/// `jumpTargetForInstruction(UncheckedKeyHashMap&, ...)` lê do mapa.
pub enum OutOfLineJumpSource<'a> {
    CodeBlock,
    Map(&'a OutOfLineJumpTargets),
}

impl OutOfLineJumpSource<'_> {
    /// `jumpTargetForInstruction(codeBlockOrMap, instruction, target)`.
    fn jump_target(&self, code_block: &impl JumpTargetBlock, bytecode_offset: u32, target: i32) -> i32 {
        if target != 0 {
            return target;
        }
        match self {
            OutOfLineJumpSource::CodeBlock => code_block.jump_offset_out_of_line(bytecode_offset),
            OutOfLineJumpSource::Map(map) => {
                debug_assert!(map.contains_key(&bytecode_offset));
                map.get(&bytecode_offset).copied().unwrap_or(0)
            }
        }
    }
}

/// O `SWITCH_JMP`: chama `$jump!(Op)` para cada opcode de salto simples (com `targetLabel`).
macro_rules! jump_ops {
    ($jump:ident, $opcode:expr, $fallback:block) => {
        match $opcode {
            OpcodeID::op_jmp => $jump!(OpJmp),
            OpcodeID::op_jtrue => $jump!(OpJtrue),
            OpcodeID::op_jfalse => $jump!(OpJfalse),
            OpcodeID::op_jeq_null => $jump!(OpJeqNull),
            OpcodeID::op_jneq_null => $jump!(OpJneqNull),
            OpcodeID::op_jundefined_or_null => $jump!(OpJundefinedOrNull),
            OpcodeID::op_jnundefined_or_null => $jump!(OpJnundefinedOrNull),
            OpcodeID::op_jeq_ptr => $jump!(OpJeqPtr),
            OpcodeID::op_jneq_ptr => $jump!(OpJneqPtr),
            OpcodeID::op_jless => $jump!(OpJless),
            OpcodeID::op_jlesseq => $jump!(OpJlesseq),
            OpcodeID::op_jgreater => $jump!(OpJgreater),
            OpcodeID::op_jgreatereq => $jump!(OpJgreatereq),
            OpcodeID::op_jnless => $jump!(OpJnless),
            OpcodeID::op_jnlesseq => $jump!(OpJnlesseq),
            OpcodeID::op_jngreater => $jump!(OpJngreater),
            OpcodeID::op_jngreatereq => $jump!(OpJngreatereq),
            OpcodeID::op_jeq => $jump!(OpJeq),
            OpcodeID::op_jneq => $jump!(OpJneq),
            OpcodeID::op_jstricteq => $jump!(OpJstricteq),
            OpcodeID::op_jnstricteq => $jump!(OpJnstricteq),
            OpcodeID::op_jbelow => $jump!(OpJbelow),
            OpcodeID::op_jbeloweq => $jump!(OpJbeloweq),
            _ => $fallback,
        }
    };
}

/// Os casos `op_switch_imm`, `op_switch_char` e `op_switch_string` do `SWITCH_JMP`: chama `function`
/// para cada deslocamento guardado nas tabelas (na ordem do C++), dando acesso mutável a ele.
/// Devolve `true` se o opcode era um `switch`.
fn for_each_switch_target(code_block: &mut impl JumpTargetBlock, instruction: &Ref, mut function: impl FnMut(&mut i32)) -> bool {
    match instruction.opcode_id_enum() {
        OpcodeID::op_switch_imm | OpcodeID::op_switch_char => {
            let table_index = if instruction.opcode_id_enum() == OpcodeID::op_switch_imm {
                instruction.as_op::<OpSwitchImm>().table_index
            } else {
                instruction.as_op::<OpSwitchChar>().table_index
            };
            code_block.with_switch_jump_table(table_index as usize, &mut |table| {
                if table.is_list() {
                    let mut i = 0;
                    while i < table.branch_offsets.len() {
                        function(&mut table.branch_offsets[i + 1]);
                        i += 2;
                    }
                } else {
                    for i in (0..table.branch_offsets.len()).rev() {
                        function(&mut table.branch_offsets[i]);
                    }
                }
                function(&mut table.default_offset);
            });
            true
        }
        OpcodeID::op_switch_string => {
            let bytecode = instruction.as_op::<OpSwitchString>();
            code_block.with_string_switch_jump_table(bytecode.table_index as usize, &mut |table| {
                for entry in table.offset_table.values_mut() {
                    function(&mut entry.branch_offset);
                }
                function(&mut table.default_offset);
            });
            true
        }
        _ => false,
    }
}

/// `jumpTargetForInstruction<Op>(codeBlock, instruction)`: o deslocamento relativo do salto simples.
fn jump_target_of<Op: crate::bytecode::bytecode_ops_decode::DecodeOp>(
    code_block: &impl JumpTargetBlock,
    source: &OutOfLineJumpSource<'_>,
    instruction: &Ref,
    target_label: impl FnOnce(Op) -> BoundLabel,
) -> i32 {
    let bytecode = instruction.as_op::<Op>();
    let target = target_label(bytecode).target(&OffsetOnly);
    source.jump_target(code_block, instruction.offset(), target)
}

/// `extractStoredJumpTargetsForInstruction(codeBlock, instruction, function)`.
pub fn extract_stored_jump_targets_for_instruction(
    code_block: &mut impl JumpTargetBlock,
    instruction: &Ref,
    mut function: impl FnMut(i32),
) {
    macro_rules! simple_jump {
        ($op:ident) => {{
            let target = jump_target_of::<$op>(code_block, &OutOfLineJumpSource::CodeBlock, instruction, |bytecode| bytecode.target_label);
            function(target);
        }};
    }
    jump_ops!(simple_jump, instruction.opcode_id_enum(), {
        for_each_switch_target(code_block, instruction, |target| function(*target));
    });
}

/// `updateStoredJumpTargetsForInstruction(codeBlock, finalOffset, instruction, function, codeBlockOrHashMap)`.
pub fn update_stored_jump_targets_for_instruction(
    code_block: &mut UnlinkedCodeBlockGenerator,
    final_offset: u32,
    instruction: &mut MutableRef,
    mut function: impl FnMut(i32) -> i32,
    source: &OutOfLineJumpSource<'_>,
) {
    let frozen = instruction.freeze();
    macro_rules! simple_jump {
        ($op:ident) => {{
            let target = jump_target_of::<$op>(code_block, source, &frozen, |bytecode| bytecode.target_label);
            let new_target = function(target);
            let bytecode_offset = final_offset.wrapping_add(instruction.offset());
            instruction.cast_mut::<$op>().set_target_label(BoundLabel::from_offset(new_target), &mut || {
                code_block.add_out_of_line_jump_target(bytecode_offset, new_target);
                BoundLabel::new()
            });
        }};
    }
    jump_ops!(simple_jump, frozen.opcode_id_enum(), {
        for_each_switch_target(code_block, &frozen, |target| {
            let old = *target;
            *target = function(old);
        });
    });
}

/// A sobrecarga de quatro argumentos: o mapa de saltos fora de linha é o do próprio bloco.
pub fn update_stored_jump_targets_for_instruction_in_block(
    code_block: &mut UnlinkedCodeBlockGenerator,
    final_offset: u32,
    instruction: &mut MutableRef,
    function: impl FnMut(i32) -> i32,
) {
    update_stored_jump_targets_for_instruction(code_block, final_offset, instruction, function, &OutOfLineJumpSource::CodeBlock);
}

/// `getJumpTargetsForInstruction`, que o C++ também expõe como `findJumpTargetsForInstruction`.
pub fn find_jump_targets_for_instruction(code_block: &mut impl JumpTargetBlock, instruction: &Ref, out: &mut Vec<u32>) {
    let offset = instruction.offset();
    extract_stored_jump_targets_for_instruction(code_block, instruction, |relative_offset| {
        out.push(offset.wrapping_add(relative_offset as u32));
    });
    // op_loop_hint does not have jump target stored in bytecode instructions.
    if instruction.opcode_id_enum() == OpcodeID::op_loop_hint {
        out.push(offset);
    }
}

/// `computePreciseJumpTargets(Block*, const JSInstructionStream&, out)`: a lista
/// ordenada, sem repetição, dos deslocamentos que são destino de salto.
pub fn compute_precise_jump_targets(code_block: &mut impl JumpTargetBlock, instructions: &InstructionStream) -> Vec<u32> {
    let mut out: Vec<u32> = Vec::new();

    for i in (0..code_block.exception_handler_count()).rev() {
        let handler = code_block.exception_handler_base(i);
        out.push(handler.target);
        out.push(handler.start);
        out.push(handler.end);
    }

    for instruction in instructions.iter() {
        find_jump_targets_for_instruction(code_block, &instruction.freeze(), &mut out);
    }

    out.sort();

    // We will have duplicates, and we must remove them.
    let mut to_index = 0;
    let mut from_index = 0;
    let mut last_value = u32::MAX;
    while from_index < out.len() {
        let value = out[from_index];
        from_index += 1;
        if value == last_value {
            continue;
        }
        out[to_index] = value;
        to_index += 1;
        last_value = value;
    }
    out.truncate(to_index);
    out
}

impl JumpTargetBlock for UnlinkedCodeBlockGenerator {
    fn jump_offset_out_of_line(&self, bytecode_offset: u32) -> i32 {
        self.out_of_line_jump_offset(bytecode_offset)
    }

    fn with_switch_jump_table(&mut self, table_index: usize, function: &mut dyn FnMut(&mut UnlinkedSimpleJumpTable)) {
        function(self.unlinked_switch_jump_table(table_index));
    }

    fn with_string_switch_jump_table(&mut self, table_index: usize, function: &mut dyn FnMut(&mut UnlinkedStringJumpTable)) {
        function(self.unlinked_string_switch_jump_table(table_index));
    }

    fn exception_handler_count(&self) -> usize {
        self.number_of_exception_handlers()
    }

    fn exception_handler_base(&mut self, index: usize) -> HandlerInfoBase {
        self.exception_handler(index).base
    }

    fn any_handler_target(&self, bytecode_offset: u32) -> Option<u32> {
        self.handler_for_bytecode_index(
            crate::bytecode::bytecode_index::BytecodeIndex::from_offset(bytecode_offset),
            crate::bytecode::handler_info::RequiredHandler::AnyHandler,
        )
        .map(|handler| handler.base.target)
    }
}
