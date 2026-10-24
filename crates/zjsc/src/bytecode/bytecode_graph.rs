//! Porte de `bytecode/BytecodeGraph.h`.
//!
//! O C++ devolve ponteiros para os blocos e itera com `IndexedContainerIterator`; em Rust, para
//! poder alterar um bloco enquanto lê os outros (a análise de liveness faz isso), a busca devolve o
//! índice do bloco e `basicBlocksInReverseOrder` devolve os índices em ordem inversa. `at(i)` e
//! `at_mut(i)` dão o bloco.

use crate::bytecode::bytecode_basic_block::{compute as compute_basic_blocks, BasicBlockVector, JSBytecodeBasicBlock};
use crate::bytecode::instruction_stream::InstructionStream;
use crate::bytecode::precise_jump_targets::JumpTargetBlock;
use crate::wtf::std_lib_extras::{approximate_binary_search, try_binary_search};

/// `class BytecodeGraph`.
#[derive(Debug)]
pub struct BytecodeGraph {
    basic_blocks: BasicBlockVector,
}

impl BytecodeGraph {
    /// `BytecodeGraph(CodeBlockType*, const InstructionStreamType&)`.
    pub fn new(code_block: &mut impl JumpTargetBlock, instructions: &InstructionStream) -> BytecodeGraph {
        let basic_blocks = compute_basic_blocks(code_block, instructions);
        debug_assert!(!basic_blocks.is_empty());
        BytecodeGraph { basic_blocks }
    }

    /// `basicBlocksInReverseOrder()`: os índices, do último bloco ao primeiro.
    pub fn basic_blocks_in_reverse_order(&self) -> impl Iterator<Item = usize> {
        (0..self.basic_blocks.len()).rev()
    }

    /// `blockContainsBytecodeOffset(block, bytecodeOffset)`.
    pub fn block_contains_bytecode_offset(block: &JSBytecodeBasicBlock, bytecode_offset: u32) -> bool {
        let leader_offset = block.leader_offset();
        bytecode_offset >= leader_offset && bytecode_offset < leader_offset.wrapping_add(block.total_length())
    }

    /// `findBasicBlockForBytecodeOffset(bytecodeOffset)`: o índice do bloco.
    pub fn find_basic_block_for_bytecode_offset(&self, bytecode_offset: u32) -> usize {
        let basic_block = approximate_binary_search(&self.basic_blocks, self.basic_blocks.len(), bytecode_offset, |block| {
            block.leader_offset()
        })
        .expect("grafo de bytecode sem blocos");
        // We found the block we were looking for.
        if Self::block_contains_bytecode_offset(&self.basic_blocks[basic_block], bytecode_offset) {
            return basic_block;
        }

        // Basic block is to the left of the returned block.
        if bytecode_offset < self.basic_blocks[basic_block].leader_offset() {
            debug_assert!(basic_block >= 1);
            debug_assert!(Self::block_contains_bytecode_offset(&self.basic_blocks[basic_block - 1], bytecode_offset));
            return basic_block - 1;
        }

        // Basic block is to the right of the returned block.
        debug_assert!(basic_block + 1 < self.basic_blocks.len());
        debug_assert!(Self::block_contains_bytecode_offset(&self.basic_blocks[basic_block + 1], bytecode_offset));
        basic_block + 1
    }

    /// `findBasicBlockWithLeaderOffset(leaderOffset)`: o índice do bloco, ou `None`.
    pub fn find_basic_block_with_leader_offset(&self, leader_offset: u32) -> Option<usize> {
        try_binary_search(&self.basic_blocks, self.basic_blocks.len(), leader_offset, |block| block.leader_offset())
    }

    /// `size()`.
    pub fn size(&self) -> usize {
        self.basic_blocks.len()
    }

    /// `at(index)`.
    pub fn at(&self, index: usize) -> &JSBytecodeBasicBlock {
        &self.basic_blocks[index]
    }

    /// `at(index)` para alterar o bloco.
    pub fn at_mut(&mut self, index: usize) -> &mut JSBytecodeBasicBlock {
        &mut self.basic_blocks[index]
    }

    /// `begin()`/`end()`: os blocos em ordem.
    pub fn iter(&self) -> std::slice::Iter<'_, JSBytecodeBasicBlock> {
        self.basic_blocks.iter()
    }

    /// `begin()`/`end()` para alterar os blocos.
    pub fn iter_mut(&mut self) -> std::slice::IterMut<'_, JSBytecodeBasicBlock> {
        self.basic_blocks.iter_mut()
    }

    /// `first()`.
    pub fn first(&self) -> &JSBytecodeBasicBlock {
        self.at(0)
    }

    /// `last()`.
    pub fn last(&self) -> &JSBytecodeBasicBlock {
        self.at(self.size() - 1)
    }

    /// `last()` para alterar o bloco.
    pub fn last_mut(&mut self) -> &mut JSBytecodeBasicBlock {
        let index = self.size() - 1;
        self.at_mut(index)
    }
}
