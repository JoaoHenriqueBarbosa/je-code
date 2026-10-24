//! Porte de `bytecode/BytecodeBasicBlock.h` e `BytecodeBasicBlock.cpp`.
//!
//! O `template<typename OpcodeTraits>` só tem a instância `JSOpcodeTraits` (`JSBytecodeBasicBlock`),
//! então a struct não é genérica. As sobrecargas de `compute` e o `computeImpl<Block>` sobre
//! `CodeBlock` e `UnlinkedCodeBlockGenerator` são genéricas no `JumpTargetBlock`.
//! O `friend class BytecodeGraph` é `pub(crate)` nos campos e métodos que só o grafo usa.

use crate::bytecode::instruction_stream::{InstructionStream, Ref};
use crate::bytecode::opcode::OpcodeID;
use crate::bytecode::opcode_inlines::{is_branch, is_terminal, is_throw, is_unconditional_branch};
use crate::bytecode::precise_jump_targets::{compute_precise_jump_targets, find_jump_targets_for_instruction, JumpTargetBlock};
use crate::wtf::fast_bit_vector::FastBitVector;

/// `enum SpecialBlockType { EntryBlock, ExitBlock }`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SpecialBlockType {
    EntryBlock,
    ExitBlock,
}

/// `class BytecodeBasicBlock` (a instância `JSBytecodeBasicBlock`).
#[derive(Debug)]
pub struct BytecodeBasicBlock {
    leader_offset: u32,
    total_length: u32,
    index: u32,

    delta: Vec<u8>,
    successors: Vec<u32>,

    in_: FastBitVector,
    out: FastBitVector,
}

pub type JSBytecodeBasicBlock = BytecodeBasicBlock;
pub type BasicBlockVector = Vec<BytecodeBasicBlock>;

impl BytecodeBasicBlock {
    /// `BytecodeBasicBlock(const InstructionStreamType::Ref&, unsigned blockIndex)`.
    pub fn new(instruction: &Ref, block_index: u32) -> BytecodeBasicBlock {
        let mut block = BytecodeBasicBlock {
            leader_offset: instruction.offset(),
            total_length: 0,
            index: block_index,
            delta: Vec::new(),
            successors: Vec::new(),
            in_: FastBitVector::default(),
            out: FastBitVector::default(),
        };
        block.add_length(instruction.size() as u32);
        block
    }

    /// `BytecodeBasicBlock(SpecialBlockType, unsigned blockIndex)`.
    pub fn special(block_type: SpecialBlockType, block_index: u32) -> BytecodeBasicBlock {
        BytecodeBasicBlock {
            leader_offset: if block_type == SpecialBlockType::EntryBlock { 0 } else { u32::MAX },
            total_length: if block_type == SpecialBlockType::EntryBlock { 0 } else { u32::MAX },
            index: block_index,
            delta: Vec::new(),
            successors: Vec::new(),
            in_: FastBitVector::default(),
            out: FastBitVector::default(),
        }
    }

    /// `isEntryBlock()`.
    pub fn is_entry_block(&self) -> bool {
        self.leader_offset == 0 && self.total_length == 0
    }

    /// `isExitBlock()`.
    pub fn is_exit_block(&self) -> bool {
        self.leader_offset == u32::MAX && self.total_length == u32::MAX
    }

    /// `leaderOffset()`.
    pub fn leader_offset(&self) -> u32 {
        self.leader_offset
    }

    /// `totalLength()`.
    pub fn total_length(&self) -> u32 {
        self.total_length
    }

    /// `delta()`.
    pub fn delta(&self) -> &Vec<u8> {
        &self.delta
    }

    /// `successors()`.
    pub fn successors(&self) -> &Vec<u32> {
        &self.successors
    }

    /// `in()`.
    pub fn in_(&self) -> &FastBitVector {
        &self.in_
    }

    /// `in()` não constante.
    pub fn in_mut(&mut self) -> &mut FastBitVector {
        &mut self.in_
    }

    /// `out()`.
    pub fn out(&self) -> &FastBitVector {
        &self.out
    }

    /// `out()` não constante.
    pub fn out_mut(&mut self) -> &mut FastBitVector {
        &mut self.out
    }

    /// `index()`.
    pub fn index(&self) -> u32 {
        self.index
    }

    /// `shrinkToFit()`.
    fn shrink_to_fit(&mut self) {
        self.delta.shrink_to_fit();
        self.successors.shrink_to_fit();
    }

    /// `addSuccessor(block)`: o `block` é só o índice dele.
    fn add_successor(&mut self, block_index: u32) {
        if !self.successors.contains(&block_index) {
            self.successors.push(block_index);
        }
    }

    /// `addLength(unsigned)`.
    fn add_length(&mut self, bytecode_length: u32) {
        self.delta.push(bytecode_length as u8);
        self.total_length += bytecode_length;
    }
}

/// `isJumpTarget(opcodeID, jumpTargets, bytecodeOffset)`.
fn is_jump_target(opcode_id: OpcodeID, jump_targets: &[u32], bytecode_offset: u32) -> bool {
    if opcode_id == OpcodeID::op_catch {
        return true;
    }

    jump_targets.binary_search(&bytecode_offset).is_ok()
}

/// `linkBlocks(from, to)`.
fn link_blocks(basic_blocks: &mut [BytecodeBasicBlock], from: usize, to: usize) {
    let to_index = basic_blocks[to].index();
    basic_blocks[from].add_successor(to_index);
}

/// `BytecodeBasicBlock<OpcodeTraits>::compute(Block*, const JSInstructionStream&)`, que no C++ só
/// encaminha para `computeImpl`; aqui os dois são esta função.
pub(crate) fn compute(code_block: &mut impl JumpTargetBlock, instructions: &InstructionStream) -> BasicBlockVector {
    let mut basic_blocks: BasicBlockVector = Vec::new();
    let jump_targets = compute_precise_jump_targets(code_block, instructions);

    {
        // Create the entry and exit basic blocks.
        basic_blocks.reserve(jump_targets.len() + 2);
        {
            // Entry block.
            basic_blocks.push(BytecodeBasicBlock::special(SpecialBlockType::EntryBlock, basic_blocks.len() as u32));
            // First block.
            basic_blocks.push(BytecodeBasicBlock::special(SpecialBlockType::EntryBlock, basic_blocks.len() as u32));
            link_blocks(&mut basic_blocks, 0, 1);
        }

        let mut current = basic_blocks.len() - 1;
        let mut next_instruction_is_leader = false;
        for instruction in instructions.iter() {
            let instruction = instruction.freeze();
            let bytecode_offset = instruction.offset();
            let opcode_id = instruction.opcode_id_enum();

            let mut created_block = false;
            // If the current bytecode is a jump target, then it's the leader of its own basic block.
            if next_instruction_is_leader || is_jump_target(opcode_id, &jump_targets, bytecode_offset) {
                basic_blocks.push(BytecodeBasicBlock::new(&instruction, basic_blocks.len() as u32));
                current = basic_blocks.len() - 1;
                created_block = true;
                next_instruction_is_leader = false;
            }

            // If the current bytecode is a branch or a return, then the next instruction is the leader of its own basic block.
            if is_branch(opcode_id) || is_terminal(opcode_id) || is_throw(opcode_id) {
                next_instruction_is_leader = true;
            }

            if created_block {
                continue;
            }

            // Otherwise, just add to the length of the current block.
            basic_blocks[current].add_length(instruction.size() as u32);
        }
        // Exit block.
        basic_blocks.push(BytecodeBasicBlock::special(SpecialBlockType::ExitBlock, basic_blocks.len() as u32));
        basic_blocks.shrink_to_fit();
        debug_assert!(basic_blocks.last().is_some_and(|block| block.is_exit_block()));
    }
    // After this point, we never change basicBlocks.

    let last_index = basic_blocks.len() - 1;

    // Link basic blocks together.
    for i in 0..basic_blocks.len() {
        if basic_blocks[i].is_entry_block() || basic_blocks[i].is_exit_block() {
            continue;
        }

        let leader_offset = basic_blocks[i].leader_offset();
        let total_length = basic_blocks[i].total_length();

        let mut falls_through = true;
        let mut visited_length = 0;
        while visited_length < total_length {
            let instruction = instructions.at(leader_offset + visited_length);
            let opcode_id = instruction.opcode_id_enum();

            visited_length += instruction.size() as u32;

            // If we found a terminal bytecode, link to the exit block.
            if is_terminal(opcode_id) {
                debug_assert!(instruction.offset() + instruction.size() as u32 == leader_offset + total_length);
                link_blocks(&mut basic_blocks, i, last_index);
                falls_through = false;
                break;
            }

            // If we found a throw, get the HandlerInfo for this instruction to see where we will jump.
            // If there isn't one, treat this throw as a terminal. This is true even if we have a finally
            // block because the finally block will create its own catch, which will generate a HandlerInfo.
            if is_throw(opcode_id) {
                debug_assert!(instruction.offset() + instruction.size() as u32 == leader_offset + total_length);
                let handler_target = code_block.any_handler_target(instruction.offset());
                falls_through = false;
                let Some(handler_target) = handler_target else {
                    link_blocks(&mut basic_blocks, i, last_index);
                    break;
                };
                for other in 0..basic_blocks.len() {
                    if handler_target == basic_blocks[other].leader_offset() {
                        link_blocks(&mut basic_blocks, i, other);
                        break;
                    }
                }
                break;
            }

            // If we found a branch, link to the block(s) that we jump to.
            if is_branch(opcode_id) {
                debug_assert!(instruction.offset() + instruction.size() as u32 == leader_offset + total_length);
                let mut bytecode_offsets_jumped_to: Vec<u32> = Vec::new();
                find_jump_targets_for_instruction(code_block, &instruction, &mut bytecode_offsets_jumped_to);

                let mut number_of_jump_targets = bytecode_offsets_jumped_to.len();
                debug_assert!(number_of_jump_targets != 0);
                for other in 0..basic_blocks.len() {
                    if bytecode_offsets_jumped_to.contains(&basic_blocks[other].leader_offset()) {
                        link_blocks(&mut basic_blocks, i, other);
                        number_of_jump_targets -= 1;
                        if number_of_jump_targets == 0 {
                            break;
                        }
                    }
                }
                // numberOfJumpTargets may not be 0 here if there are multiple jumps targeting the same
                // basic blocks (e.g. in a switch type opcode). Since we only decrement numberOfJumpTargets
                // once per basic block, the duplicates are not accounted for. For our purpose here,
                // that doesn't matter because we only need to link to the target block once regardless
                // of how many ways this block can jump there.

                if is_unconditional_branch(opcode_id) {
                    falls_through = false;
                }

                break;
            }
        }

        // If we fall through then link to the next block in program order.
        if falls_through {
            debug_assert!(i + 1 < basic_blocks.len());
            link_blocks(&mut basic_blocks, i, i + 1);
        }
    }

    for (index, basic_block) in basic_blocks.iter_mut().enumerate() {
        basic_block.shrink_to_fit();
        debug_assert!(basic_block.index() as usize == index);
    }

    basic_blocks
}
