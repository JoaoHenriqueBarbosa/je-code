//! Porte de `bytecode/BytecodeLivenessAnalysis.h` e `BytecodeLivenessAnalysisInlines.h`: o
//! `BytecodeLivenessPropagation` (o passo sobre as instruções e o ponto fixo), usado pela
//! `GeneratorLivenessAnalysis` da generatorification.
//!
//! Não portados, por dependerem do `CodeBlock` do heap (que ainda não existe; sem JIT ele só entra
//! com o interpretador): a classe `BytecodeLivenessAnalysis` e o `.cpp` dela (`computeFullLiveness`,
//! `dumpResults`), o `FullBytecodeLiveness.h` e o `tmpLivenessForCheckpoint`. As funções daqui
//! recebem o `UnlinkedCodeBlockGenerator`, que é o `CodeBlockType` da generatorification.
//!
//! Diferenças de forma em relação ao C++, sem mudar o cálculo:
//! - o bloco vem por índice no grafo (`block_index`) e não por referência, porque o C++ lê outros
//!   blocos do grafo (`handlerBlock->in()`) enquanto escreve o `in()` do bloco corrente;
//! - `computeLocalLivenessForBlock` calcula o vetor em temporário e grava no `in()` do bloco com
//!   `setAndCheck`, que é o que `computeLocalLivenessForInstruction(..., block.in())` faz;
//! - os dois functores (`use` e `def`) de `stepOverInstruction` mexem no mesmo `out`, então o vetor
//!   vai num `RefCell`.

use std::cell::RefCell;

use crate::bytecode::bytecode_graph::BytecodeGraph;
use crate::bytecode::bytecode_index::BytecodeIndex;
use crate::bytecode::bytecode_use_def::{compute_defs_for_bytecode_index, compute_uses_for_bytecode_index};
use crate::bytecode::handler_info::RequiredHandler;
use crate::bytecode::instruction_stream::InstructionStream;
use crate::bytecode::unlinked_code_block_generator::UnlinkedCodeBlockGenerator;
use crate::bytecode::virtual_register::VirtualRegister;
use crate::bytecompiler::bytecode_generator_base::GeneratorCodeBlock;
use crate::wtf::fast_bit_vector::FastBitVector;

// We model our bytecode effects like the following and insert the liveness calculation points.
//
// <- BeforeUse
//     Use
// <- AfterUse
//     Use by exception handlers
//     Def
/// `enum class LivenessCalculationPoint`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum LivenessCalculationPoint {
    BeforeUse,
    AfterUse,
}

/// `virtualRegisterIsAlwaysLive(reg)`.
pub fn virtual_register_is_always_live(reg: VirtualRegister) -> bool {
    !reg.is_local()
}

/// `virtualRegisterThatIsNotAlwaysLiveIsLive(out, reg)`.
pub fn virtual_register_that_is_not_always_live_is_live(out: &FastBitVector, reg: VirtualRegister) -> bool {
    let local = reg.to_local() as u32 as usize;
    if local >= out.num_bits() {
        return false;
    }
    out.at(local)
}

/// `virtualRegisterIsLive(out, operand)`.
pub fn virtual_register_is_live(out: &FastBitVector, operand: VirtualRegister) -> bool {
    virtual_register_is_always_live(operand) || virtual_register_that_is_not_always_live_is_live(out, operand)
}

/// `isValidRegisterForLiveness(operand)`.
pub fn is_valid_register_for_liveness(operand: VirtualRegister) -> bool {
    if operand.is_constant() {
        return false;
    }
    operand.is_local()
}

/// `class BytecodeLivenessPropagation`: só funções estáticas.
pub struct BytecodeLivenessPropagation;

impl BytecodeLivenessPropagation {
    /// `stepOverBytecodeIndexDef`.
    pub fn step_over_bytecode_index_def(
        code_block: &UnlinkedCodeBlockGenerator,
        instructions: &InstructionStream,
        _graph: &BytecodeGraph,
        bytecode_index: BytecodeIndex,
        def: &mut dyn FnMut(usize),
    ) {
        let instruction = instructions.at(bytecode_index.offset());
        compute_defs_for_bytecode_index(code_block, &instruction, bytecode_index.checkpoint() as u32, |operand: VirtualRegister| {
            if is_valid_register_for_liveness(operand) {
                def(operand.to_local() as usize);
            }
        });
    }

    /// `stepOverBytecodeIndexUse`.
    pub fn step_over_bytecode_index_use(
        code_block: &UnlinkedCodeBlockGenerator,
        instructions: &InstructionStream,
        _graph: &BytecodeGraph,
        bytecode_index: BytecodeIndex,
        use_: &mut dyn FnMut(usize),
    ) {
        let instruction = instructions.at(bytecode_index.offset());
        compute_uses_for_bytecode_index(code_block, &instruction, bytecode_index.checkpoint() as u32, |operand: VirtualRegister| {
            if is_valid_register_for_liveness(operand) {
                use_(operand.to_local() as usize);
            }
        });
    }

    /// `stepOverBytecodeIndexUseInExceptionHandler`.
    pub fn step_over_bytecode_index_use_in_exception_handler(
        code_block: &UnlinkedCodeBlockGenerator,
        _instructions: &InstructionStream,
        graph: &BytecodeGraph,
        bytecode_index: BytecodeIndex,
        use_: &mut dyn FnMut(usize),
    ) {
        // If we have an exception handler, we want the live-in variables of the
        // exception handler block to be included in the live-in of this particular BytecodeIndex.
        if let Some(handler) = code_block.handler_for_bytecode_index(bytecode_index, RequiredHandler::AnyHandler) {
            let handler_block = graph.find_basic_block_with_leader_offset(handler.base.target);
            debug_assert!(handler_block.is_some());
            if let Some(handler_block) = handler_block {
                graph.at(handler_block).in_().for_each_set_bit(|bit| use_(bit));
            }
        }
    }

    // Simplified interface to bytecode use/def, which determines defs first and then uses, and includes
    // exception handlers in the uses.
    /// `stepOverBytecodeIndex`.
    pub fn step_over_bytecode_index(
        code_block: &UnlinkedCodeBlockGenerator,
        instructions: &InstructionStream,
        graph: &BytecodeGraph,
        bytecode_index: BytecodeIndex,
        use_: &mut dyn FnMut(usize),
        def: &mut dyn FnMut(usize),
    ) {
        // This abstractly executes the BytecodeIndex in reverse. Instructions logically first use operands and
        // then define operands. This logical ordering is necessary for operations that use and def the same
        // operand, like:
        //
        //     op_add loc1, loc1, loc2
        //
        // The use of loc1 happens before the def of loc1. That's a semantic requirement since the add
        // operation cannot travel forward in time to read the value that it will produce after reading that
        // value. Since we are executing in reverse, this means that we must do defs before uses (reverse of
        // uses before defs).
        //
        // Since this is a liveness analysis, this ordering ends up being particularly important: if we did
        // uses before defs, then the add operation above would appear to not have loc1 live, since we'd
        // first add it to the out set (the use), and then we'd remove it (the def).

        Self::step_over_bytecode_index_def(code_block, instructions, graph, bytecode_index, def);
        Self::step_over_bytecode_index_use_in_exception_handler(code_block, instructions, graph, bytecode_index, use_);
        Self::step_over_bytecode_index_use(code_block, instructions, graph, bytecode_index, use_);
    }

    /// `stepOverInstruction`.
    pub fn step_over_instruction(
        code_block: &UnlinkedCodeBlockGenerator,
        instructions: &InstructionStream,
        graph: &BytecodeGraph,
        bytecode_index: BytecodeIndex,
        out: &mut FastBitVector,
    ) {
        let number_of_checkpoints = instructions.at(bytecode_index.offset()).with_instruction(|instruction| instruction.number_of_checkpoints());
        let shared = RefCell::new(std::mem::take(out));
        let mut checkpoint = number_of_checkpoints;
        while checkpoint > 0 {
            checkpoint -= 1;
            Self::step_over_bytecode_index(
                code_block,
                instructions,
                graph,
                bytecode_index.with_checkpoint(checkpoint as u8),
                &mut |bit_index| {
                    // This is the use functor, so we set the bit.
                    shared.borrow_mut().set(bit_index, true);
                },
                &mut |bit_index| {
                    // This is the def functor, so we clear the bit.
                    shared.borrow_mut().set(bit_index, false);
                },
            );
        }
        *out = shared.into_inner();
    }

    /// O miolo de `computeLocalLivenessForInstruction`: o vetor de liveness em `target_index`, a
    /// partir do `out()` do bloco.
    fn local_liveness_at(
        code_block: &UnlinkedCodeBlockGenerator,
        instructions: &InstructionStream,
        graph: &BytecodeGraph,
        block_index: usize,
        target_index: BytecodeIndex,
    ) -> FastBitVector {
        let block = graph.at(block_index);
        debug_assert!(!block.is_exit_block());
        debug_assert!(!block.is_entry_block());
        debug_assert!(target_index.checkpoint() == 0, "computeLocalLivenessForInstruction can't be used to ask questions about checkpoints");

        let mut out = block.out().clone();

        let mut cursor = block.total_length();
        for i in (0..block.delta().len()).rev() {
            cursor -= block.delta()[i] as u32;
            let bytecode_index = BytecodeIndex::from_offset(block.leader_offset() + cursor);
            if target_index.offset() > bytecode_index.offset() {
                break;
            }
            Self::step_over_instruction(code_block, instructions, graph, bytecode_index, &mut out);
        }

        out
    }

    /// `computeLocalLivenessForInstruction`: grava em `result` e diz se mudou.
    pub fn compute_local_liveness_for_instruction(
        code_block: &UnlinkedCodeBlockGenerator,
        instructions: &InstructionStream,
        graph: &BytecodeGraph,
        block_index: usize,
        target_index: BytecodeIndex,
        result: &mut FastBitVector,
    ) -> bool {
        let out = Self::local_liveness_at(code_block, instructions, graph, block_index, target_index);
        result.set_and_check(&out)
    }

    /// `computeLocalLivenessForBlock`: o `result` é o `in()` do próprio bloco.
    pub fn compute_local_liveness_for_block(
        code_block: &UnlinkedCodeBlockGenerator,
        instructions: &InstructionStream,
        graph: &mut BytecodeGraph,
        block_index: usize,
    ) -> bool {
        if graph.at(block_index).is_exit_block() || graph.at(block_index).is_entry_block() {
            return false;
        }
        let leader_offset = graph.at(block_index).leader_offset();
        let out = Self::local_liveness_at(code_block, instructions, graph, block_index, BytecodeIndex::from_offset(leader_offset));
        graph.at_mut(block_index).in_mut().set_and_check(&out)
    }

    /// `getLivenessInfoAtInstruction`.
    pub fn get_liveness_info_at_instruction(
        code_block: &UnlinkedCodeBlockGenerator,
        instructions: &InstructionStream,
        graph: &BytecodeGraph,
        bytecode_index: BytecodeIndex,
    ) -> FastBitVector {
        debug_assert!(bytecode_index.checkpoint() == 0, "getLivenessInfoAtInstruction can't be used to ask questions about checkpoints");
        let block_index = graph.find_basic_block_for_bytecode_offset(bytecode_index.offset());
        debug_assert!(!graph.at(block_index).is_entry_block());
        debug_assert!(!graph.at(block_index).is_exit_block());
        let mut out = FastBitVector::default();
        out.resize(graph.at(block_index).out().num_bits());
        Self::compute_local_liveness_for_instruction(code_block, instructions, graph, block_index, bytecode_index, &mut out);
        out
    }

    /// `runLivenessFixpoint`.
    pub fn run_liveness_fixpoint(
        code_block: &UnlinkedCodeBlockGenerator,
        instructions: &InstructionStream,
        graph: &mut BytecodeGraph,
    ) {
        let number_of_variables = GeneratorCodeBlock::num_callee_locals(code_block) as usize;
        for block in graph.iter_mut() {
            block.in_mut().resize(number_of_variables);
            block.out_mut().resize(number_of_variables);
            block.in_mut().clear_all();
            block.out_mut().clear_all();
        }

        let mut changed;
        {
            let last_block = graph.last_mut();
            last_block.in_mut().clear_all();
            last_block.out_mut().clear_all();
        }
        let mut new_out = FastBitVector::default();
        new_out.resize(graph.last().out().num_bits());
        loop {
            changed = false;
            for block_index in graph.basic_blocks_in_reverse_order().collect::<Vec<_>>() {
                new_out.clear_all();
                for &successor_index in graph.at(block_index).successors() {
                    new_out.or_assign(graph.at(successor_index as usize).in_());
                }
                graph.at_mut(block_index).out_mut().assign(&new_out);
                changed |= Self::compute_local_liveness_for_block(code_block, instructions, graph, block_index);
            }
            if !changed {
                break;
            }
        }
    }
}
