//! Porte de `bytecode/BytecodeGeneratorification.h` e `BytecodeGeneratorification.cpp`.
//!
//! O passo que transforma o corpo de uma função geradora em uma máquina de estados: calcula a
//! liveness dos locais em cada `op_yield`, insere o `switch` global sobre o estado do gerador no
//! começo, troca cada `op_yield` por uma sequência de salvar (`op_put_to_scope` no quadro do
//! gerador, seguido de `op_ret`) mais uma de retomar (`op_get_from_scope`), e troca
//! `op_create_generator_frame_environment` pelo ambiente que guarda os locais salvos.
//!
//! O nome do módulo segue o do `.cpp` (`bytecompiler/`, como o chamador espera), embora o arquivo
//! esteja em `bytecode/` no upstream.
//!
//! Diferenças de forma em relação ao C++, sem mudar o resultado:
//! - `performGeneratorification(BytecodeGenerator&, UnlinkedCodeBlockGenerator*,
//!   JSInstructionStreamWriter&, ...)` recebe só o gerador: o bloco (`m_codeBlock`) e o escritor
//!   (`m_writer`) são campos dele, e o C++ os passa como três ponteiros para o mesmo objeto.
//! - o `Strong<SymbolTable>` é o `Rc<RefCell<SymbolTable>>` que o gerador já guarda.
//! - `storageForGeneratorLocal` é função associada que recebe `m_storages` e o gerador, porque as
//!   lambdas dos fragmentos mexem nos dois ao mesmo tempo.
//! - o `Options::dumpBytecodesBeforeGeneratorification()` só imprime o dump de depuração do
//!   `BytecodeDumper` (ainda não portado) e o ramo de depuração não é portado.

use std::cell::RefCell;
use std::rc::Rc;

use crate::bytecode::bytecode_graph::BytecodeGraph;
use crate::bytecode::bytecode_index::BytecodeIndex;
use crate::bytecode::bytecode_liveness_analysis::BytecodeLivenessPropagation;
use crate::bytecode::bytecode_ops::{
    OpCreateGeneratorFrameEnvironment, OpCreateLexicalEnvironment, OpGetFromScope, OpMov, OpPutToScope, OpRet, OpSwitchImm,
    OpYield,
};
use crate::bytecode::bytecode_rewriter::BytecodeRewriter;
use crate::bytecode::opcode::OpcodeID;
use crate::bytecode::virtual_register::{virtual_register_for_argument_including_this, virtual_register_for_local, VirtualRegister};
use crate::bytecompiler::bytecode_generator::BytecodeGenerator;
use crate::runtime::get_put_info::{GetPutInfo, InitializationMode, ResolveMode, ResolveType};
use crate::runtime::identifier::Identifier;
use crate::runtime::js_generator::Argument as GeneratorArgument;
use crate::runtime::scope_offset::ScopeOffset;
use crate::runtime::symbol_table::{SymbolTable, SymbolTableEntry};
use crate::runtime::symbol_table_or_scope_depth::SymbolTableOrScopeDepth;
use crate::runtime::var_offset::VarOffset;
use crate::wtf::fast_bit_vector::FastBitVector;

/// `struct YieldData`.
#[derive(Clone, Debug)]
struct YieldData {
    point: u32,
    argument: VirtualRegister,
    liveness: FastBitVector,
}

impl Default for YieldData {
    fn default() -> YieldData {
        YieldData { point: 0, argument: VirtualRegister::new(0), liveness: FastBitVector::default() }
    }
}

/// `BytecodeGeneratorification::GeneratorFrameData`.
#[derive(Clone, Copy, Debug)]
struct GeneratorFrameData {
    point: u32,
    dst: VirtualRegister,
    scope: VirtualRegister,
    symbol_table: VirtualRegister,
    initial_value: VirtualRegister,
}

/// `BytecodeGeneratorification::Storage`.
#[derive(Clone, Debug)]
struct Storage {
    #[allow(dead_code)]
    identifier: Identifier,
    identifier_index: u32,
    scope_offset: ScopeOffset,
}

/// `class BytecodeGeneratorification`.
struct BytecodeGeneratorification {
    enter_point: u32,
    generator_frame_data: Option<GeneratorFrameData>,
    graph: BytecodeGraph,
    storages: Vec<Option<Storage>>,
    yields: Vec<YieldData>,
    generator_frame_symbol_table: Rc<RefCell<SymbolTable>>,
    generator_frame_symbol_table_index: i32,
}

impl BytecodeGeneratorification {
    /// O construtor.
    fn new(
        generator: &mut BytecodeGenerator,
        generator_frame_symbol_table: Rc<RefCell<SymbolTable>>,
        generator_frame_symbol_table_index: i32,
    ) -> BytecodeGeneratorification {
        let base = &mut **generator;
        let graph = BytecodeGraph::new(&mut base.code_block, &base.writer);
        let mut result = BytecodeGeneratorification {
            enter_point: 0,
            generator_frame_data: None,
            graph,
            storages: Vec::new(),
            yields: Vec::new(),
            generator_frame_symbol_table,
            generator_frame_symbol_table_index,
        };

        for instruction in generator.writer.iter() {
            match instruction.opcode_id_enum() {
                OpcodeID::op_enter => {
                    result.enter_point = instruction.offset();
                }

                OpcodeID::op_yield => {
                    let bytecode = instruction.as_op::<OpYield>();
                    let live_callee_locals_index = bytecode.yield_point as usize;
                    if live_callee_locals_index >= result.yields.len() {
                        result.yields.resize(live_callee_locals_index + 1, YieldData::default());
                    }
                    let data = &mut result.yields[live_callee_locals_index];
                    data.point = instruction.offset();
                    data.argument = bytecode.argument;
                }

                OpcodeID::op_create_generator_frame_environment => {
                    let bytecode = instruction.as_op::<OpCreateGeneratorFrameEnvironment>();
                    result.generator_frame_data = Some(GeneratorFrameData {
                        point: instruction.offset(),
                        dst: bytecode.dst,
                        scope: bytecode.scope,
                        symbol_table: bytecode.symbol_table,
                        initial_value: bytecode.initial_value,
                    });
                }

                _ => {}
            }
        }

        result
    }

    fn storage_for_generator_local(
        storages: &mut Vec<Option<Storage>>,
        generator_frame_symbol_table: &Rc<RefCell<SymbolTable>>,
        generator: &mut BytecodeGenerator,
        index: usize,
    ) -> Storage {
        // We assign a symbol to a register. There is one-on-one corresponding between a register and a symbol.
        // By doing so, we allocate the specific storage to save the given register.
        // This allow us not to save all the live registers even if the registers are not overwritten from the previous resuming time.
        // It means that, the register can be retrieved even if the immediate previous op_save does not save it.

        if storages.len() <= index {
            storages.resize(index + 1, None);
        }
        if let Some(storage) = &storages[index] {
            return storage.clone();
        }

        let identifier = Identifier::from_u32(generator.vm(), index as u32);
        let identifier_index = generator.code_block.number_of_identifiers();
        generator.code_block.add_identifier(identifier.clone());
        let scope_offset = {
            let mut symbol_table = generator_frame_symbol_table.borrow_mut();
            let scope_offset = symbol_table.take_next_scope_offset();
            let key = identifier.impl_().expect("identificador numérico sem impl");
            symbol_table.add(key, SymbolTableEntry::from_var_offset(VarOffset::from_scope_offset(scope_offset)));
            scope_offset
        };
        let storage = Storage { identifier, identifier_index, scope_offset };
        storages[index] = Some(storage.clone());
        storage
    }

    fn run(&mut self, generator: &mut BytecodeGenerator) {
        // We calculate the liveness at each merge point. This gives us the information which registers should be saved and resumed conservatively.

        {
            // GeneratorLivenessAnalysis::run: perform modified liveness analysis to determine which locals are live at the merge points.
            // This produces the conservative results for the question, "which variables should be saved and resumed?".
            BytecodeLivenessPropagation::run_liveness_fixpoint(&generator.code_block, &generator.writer, &mut self.graph);

            for data in &mut self.yields {
                data.liveness = BytecodeLivenessPropagation::get_liveness_info_at_instruction(
                    &generator.code_block,
                    &generator.writer,
                    &self.graph,
                    BytecodeIndex::from_offset(generator.writer.at(data.point).next().offset()),
                );
            }
        }

        let mut rewriter = BytecodeRewriter::default();

        // Setup the global switch for the generator.
        {
            let next_to_enter_point = generator.writer.at(self.enter_point).next();
            let switch_table_index = generator.code_block.number_of_unlinked_switch_jump_tables() as u32;
            let state = virtual_register_for_argument_including_this(GeneratorArgument::State as i32, 0);
            let jump_table = generator.code_block.add_unlinked_switch_jump_table();
            jump_table.min = 0;
            jump_table.branch_offsets = vec![0; self.yields.len() + 1];
            jump_table.add(0, next_to_enter_point.offset() as i32);
            for i in 0..self.yields.len() {
                jump_table.add(i as i32 + 1, self.yields[i].point as i32);
            }
            jump_table.default_offset = next_to_enter_point.offset() as i32;

            rewriter.insert_fragment_before(generator, &next_to_enter_point, |fragment| {
                fragment.append_instruction::<OpSwitchImm>(|g| OpSwitchImm::emit(g, switch_table_index, state));
            });
        }

        let table_index = self.generator_frame_symbol_table_index;
        for data in &self.yields {
            let scope = virtual_register_for_argument_including_this(GeneratorArgument::Frame as i32, 0);

            let instruction = generator.writer.at(data.point);
            // Emit save sequence.
            rewriter.insert_fragment_before(generator, &instruction, |fragment| {
                data.liveness.for_each_set_bit(|index| {
                    let operand = virtual_register_for_local(index as i32);
                    let storage = Self::storage_for_generator_local(
                        &mut self.storages,
                        &self.generator_frame_symbol_table,
                        fragment.generator(),
                        index,
                    );

                    fragment.append_instruction::<OpPutToScope>(|g| {
                        let ecma_mode = g.ecma_mode();
                        OpPutToScope::emit(
                            g,
                            scope, // scope
                            storage.identifier_index, // identifier
                            operand, // value
                            GetPutInfo::new(
                                ResolveMode::DoNotThrowIfNotFound,
                                ResolveType::ResolvedClosureVar,
                                InitializationMode::NotInitialization,
                                ecma_mode,
                            ), // info
                            SymbolTableOrScopeDepth::symbol_table(VirtualRegister::new(table_index)), // symbol table constant index
                            storage.scope_offset.offset(), // scope offset
                        );
                    });
                });

                // Insert op_ret just after save sequence.
                fragment.append_instruction::<OpRet>(|g| OpRet::emit(g, data.argument));
            });

            // Emit resume sequence.
            rewriter.replace_bytecode_with_fragment(generator, &instruction, |fragment| {
                data.liveness.for_each_set_bit(|index| {
                    let operand = virtual_register_for_local(index as i32);
                    let storage = Self::storage_for_generator_local(
                        &mut self.storages,
                        &self.generator_frame_symbol_table,
                        fragment.generator(),
                        index,
                    );

                    fragment.append_instruction::<OpGetFromScope>(|g| {
                        let ecma_mode = g.ecma_mode();
                        let value_profile = g.next_value_profile_index();
                        OpGetFromScope::emit(
                            g,
                            operand, // dst
                            scope, // scope
                            storage.identifier_index, // identifier
                            GetPutInfo::new(
                                ResolveMode::DoNotThrowIfNotFound,
                                ResolveType::ResolvedClosureVar,
                                InitializationMode::NotInitialization,
                                ecma_mode,
                            ), // info
                            0u32, // local scope depth
                            storage.scope_offset.offset(), // scope offset
                            value_profile,
                        );
                    });
                });
            });
        }

        if let Some(frame_data) = self.generator_frame_data {
            let instruction = generator.writer.at(frame_data.point);
            let scope_size = self.generator_frame_symbol_table.borrow().scope_size();
            rewriter.replace_bytecode_with_fragment(generator, &instruction, |fragment| {
                if scope_size == 0 {
                    // This will cause us to put jsUndefined() into the generator frame's scope value.
                    fragment.append_instruction::<OpMov>(|g| OpMov::emit(g, frame_data.dst, frame_data.initial_value));
                } else {
                    fragment.append_instruction::<OpCreateLexicalEnvironment>(|g| {
                        OpCreateLexicalEnvironment::emit(g, frame_data.dst, frame_data.scope, frame_data.symbol_table, frame_data.initial_value)
                    });
                }
            });
        }

        let base = &mut **generator;
        rewriter.execute(&mut base.code_block, &mut base.writer);
    }
}

/// `performGeneratorification(...)`: `generator_frame_symbol_table` é o `Strong<SymbolTable>` que o
/// gerador guarda (sempre presente quando `needsGeneratorification` vale).
pub fn perform_generatorification(
    generator: &mut BytecodeGenerator,
    generator_frame_symbol_table: Option<Rc<RefCell<SymbolTable>>>,
    generator_frame_symbol_table_index: i32,
) {
    let generator_frame_symbol_table =
        generator_frame_symbol_table.expect("needsGeneratorification sem a symbol table do quadro do gerador");
    let mut pass = BytecodeGeneratorification::new(generator, generator_frame_symbol_table, generator_frame_symbol_table_index);
    pass.run(generator);
}
