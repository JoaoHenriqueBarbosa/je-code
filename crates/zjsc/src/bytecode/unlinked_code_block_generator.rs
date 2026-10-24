//! Porte de `bytecode/UnlinkedCodeBlockGenerator.h` e `UnlinkedCodeBlockGenerator.cpp`: o
//! acumulador de dados de bytecode que o `BytecodeGenerator` preenche (constantes, identificadores,
//! tabelas de salto, handlers de exceção, informação de expressão) e que o `finalize` transfere ao
//! `UnlinkedCodeBlock`.
//!
//! Divergências:
//!
//! - O `Strong<UnlinkedCodeBlock>` é o `Rc<RefCell<UnlinkedCodeBlock>>` que o bytecompiler já usa. Sem
//!   heap, `VM& m_vm`, `vm()` e os `ASSERT(m_vm.heap.isDeferred())` não existem; o construtor recebe
//!   o `&VM` só para manter a assinatura do C++. O C++ passa o `UnlinkedCodeBlock*` do subtipo
//!   (Program, Function, Eval, ModuleProgram); aqui o chamador passa `subtipo.base_ref()`, o `Rc`
//!   da base que o subtipo compartilha (ver o design em `unlinked_code_block.rs`).
//! - `WriteBarrier<Unknown>` é `JSValue` e `WriteBarrier<UnlinkedFunctionExecutable>` é
//!   `UnlinkedFunctionExecutableRef`; `Vector` é `Vec`.
//! - `constantRegister(reg)` e `getConstant(reg)` são o mesmo acesso (a barreira desaparece), então só
//!   existe `constant_register`.
//! - `handlerForBytecodeIndex`/`handlerForIndex` devolvem referência compartilhada (o C++ devolve
//!   ponteiro mutável; quem altera um handler usa `exception_handler(index)`).
//! - `applyModification(BytecodeRewriter&)` e `dump` não estão aqui: o `BytecodeRewriter` ainda não
//!   tem porte. As peças que ele usa (`Encoder::remap`, as listas de handlers, offsets de profile e
//!   o mapa do type profiler) são `pub(crate)` ou têm acessor.
//! - `numVars`, `numCalleeLocals` e os setters correspondentes vêm do trait `GeneratorCodeBlock`
//!   (o contrato `Traits::CodeBlock` do `BytecodeGeneratorBase`), não de métodos próprios.

use std::cell::{RefCell, RefMut};
use std::collections::HashMap;
use std::rc::Rc;

use crate::bytecode::bytecode_index::BytecodeIndex;
use crate::bytecode::expression_info::Encoder as ExpressionInfoEncoder;
use crate::bytecode::handler_info::{HandlerInfoBase, RequiredHandler, UnlinkedHandlerInfo};
use crate::bytecode::instruction_stream::JSInstructionStream;
use crate::bytecode::line_column::LineColumn;
use crate::bytecode::link_time_constant::LinkTimeConstant;
use crate::bytecode::unlinked_code_block::{
    RareData, TypeProfilerExpressionRange, UnlinkedCodeBlock, UnlinkedSimpleJumpTable, UnlinkedStringJumpTable,
};
use crate::bytecode::unlinked_metadata_table::UnlinkedMetadataTable;
use crate::bytecode::virtual_register::VirtualRegister;
use crate::bytecompiler::bytecode_generator_base::GeneratorCodeBlock;
use crate::parser::parser::IdentifierSet;
use crate::runtime::identifier::Identifier;
use crate::runtime::js_cjs_value_types::SourceCodeRepresentation;
use crate::runtime::js_value::JSValue;
use crate::runtime::unlinked_function_executable::UnlinkedFunctionExecutableRef;
use crate::runtime::vm::VM;
use crate::wtf::bit_vector::BitVector;

/// `using OutOfLineJumpTargets = UncheckedKeyHashMap<JSInstructionStream::Offset, int>`.
pub type OutOfLineJumpTargets = HashMap<u32, i32>;

/// Os acessos de leitura que o C++ escreve como `m_codeBlock->nome()` um por um.
macro_rules! code_block_getters {
    ($($name:ident: $ty:ty),* $(,)?) => {
        $(
            pub fn $name(&self) -> $ty {
                self.code_block.borrow().$name()
            }
        )*
    };
}

/// As escritas no `UnlinkedCodeBlock` que o gerador expõe (`// Updating UnlinkedCodeBlock.`).
macro_rules! code_block_setters {
    ($($name:ident($($arg:ident: $ty:ty)?)),* $(,)?) => {
        $(
            pub fn $name(&mut self $(, $arg: $ty)?) {
                self.code_block.borrow_mut().$name($($arg)?);
            }
        )*
    };
}

/// `class UnlinkedCodeBlockGenerator`.
pub struct UnlinkedCodeBlockGenerator {
    code_block: Rc<RefCell<UnlinkedCodeBlock>>,
    // In non-RareData.
    identifiers: Vec<Identifier>,
    constant_registers: Vec<JSValue>,
    constants_source_code_representation: Vec<SourceCodeRepresentation>,
    function_decls: Vec<UnlinkedFunctionExecutableRef>,
    function_exprs: Vec<UnlinkedFunctionExecutableRef>,
    expression_info_encoder: ExpressionInfoEncoder,
    out_of_line_jump_targets: OutOfLineJumpTargets,
    // In RareData.
    exception_handlers: Vec<UnlinkedHandlerInfo>,
    unlinked_switch_jump_tables: Vec<UnlinkedSimpleJumpTable>,
    unlinked_string_switch_jump_tables: Vec<UnlinkedStringJumpTable>,
    type_profiler_info_map: HashMap<u32, TypeProfilerExpressionRange>,
    op_profile_control_flow_bytecode_offsets: Vec<u32>,
    bit_vectors: Vec<BitVector>,
    constant_identifier_sets: Vec<IdentifierSet>,
    num_binary_arith_profiles: u32,
    num_unary_arith_profiles: u32,
}

impl UnlinkedCodeBlockGenerator {
    /// `UnlinkedCodeBlockGenerator(VM&, UnlinkedCodeBlock*)`.
    pub fn new(_vm: &VM, code_block: Rc<RefCell<UnlinkedCodeBlock>>) -> UnlinkedCodeBlockGenerator {
        UnlinkedCodeBlockGenerator {
            code_block,
            identifiers: Vec::new(),
            constant_registers: Vec::new(),
            constants_source_code_representation: Vec::new(),
            function_decls: Vec::new(),
            function_exprs: Vec::new(),
            expression_info_encoder: ExpressionInfoEncoder::default(),
            out_of_line_jump_targets: HashMap::new(),
            exception_handlers: Vec::new(),
            unlinked_switch_jump_tables: Vec::new(),
            unlinked_string_switch_jump_tables: Vec::new(),
            type_profiler_info_map: HashMap::new(),
            op_profile_control_flow_bytecode_offsets: Vec::new(),
            bit_vectors: Vec::new(),
            constant_identifier_sets: Vec::new(),
            num_binary_arith_profiles: 0,
            num_unary_arith_profiles: 0,
        }
    }

    /// O `m_codeBlock` (`Strong<UnlinkedCodeBlock>`).
    pub fn code_block(&self) -> &Rc<RefCell<UnlinkedCodeBlock>> {
        &self.code_block
    }

    code_block_getters! {
        is_constructor: bool,
        constructor_kind: crate::runtime::constructor_kind::ConstructorKind,
        super_binding: crate::parser::parser_modes::SuperBinding,
        script_mode: crate::parser::parser_modes::JSParserScriptMode,
        needs_class_field_initializer: crate::bytecode::executable_info::NeedsClassFieldInitializer,
        private_brand_requirement: crate::parser::parser_modes::PrivateBrandRequirement,
        parse_mode: crate::parser::parser_modes::SourceParseMode,
        is_arrow_function: bool,
        derived_context_type: crate::bytecode::executable_info::DerivedContextType,
        eval_context_type: crate::bytecode::executable_info::EvalContextType,
        is_arrow_function_context: bool,
        is_class_context: bool,
        num_parameters: u32,
        this_register: VirtualRegister,
        scope_register: VirtualRegister,
        was_compiled_with_debugging_opcodes: bool,
        has_checkpoints: bool,
        has_tail_calls: bool,
        is_builtin_function: bool,
        is_builtin_default_class_constructor: bool,
    }

    // Updating UnlinkedCodeBlock.
    code_block_setters! {
        set_has_checkpoints(),
        set_has_tail_calls(),
        set_this_register(this_register: VirtualRegister),
        set_scope_register(scope_register: VirtualRegister),
        set_num_parameters(new_value: u32),
    }

    /// `metadata()`.
    pub fn metadata(&self) -> RefMut<'_, UnlinkedMetadataTable> {
        RefMut::map(self.code_block.borrow_mut(), |code_block| code_block.metadata())
    }

    /// `addExpressionInfo`.
    pub fn add_expression_info(
        &mut self,
        instruction_offset: u32,
        divot: u32,
        start_offset: u32,
        end_offset: u32,
        line_column: LineColumn,
    ) {
        self.expression_info_encoder.encode(instruction_offset, divot, start_offset, end_offset, line_column);
    }

    /// `addTypeProfilerExpressionInfo`.
    pub fn add_type_profiler_expression_info(&mut self, instruction_offset: u32, start_divot: u32, end_divot: u32) {
        let range = TypeProfilerExpressionRange { start_divot, end_divot };
        self.type_profiler_info_map.insert(instruction_offset, range);
    }

    pub fn add_op_profile_control_flow_bytecode_offset(&mut self, offset: u32) {
        self.op_profile_control_flow_bytecode_offsets.push(offset);
    }

    pub fn number_of_unlinked_switch_jump_tables(&self) -> usize {
        self.unlinked_switch_jump_tables.len()
    }

    pub fn add_unlinked_switch_jump_table(&mut self) -> &mut UnlinkedSimpleJumpTable {
        self.unlinked_switch_jump_tables.push(UnlinkedSimpleJumpTable::default());
        self.unlinked_switch_jump_tables.last_mut().expect("tabela recém-inserida")
    }

    pub fn unlinked_switch_jump_table(&mut self, table_index: usize) -> &mut UnlinkedSimpleJumpTable {
        &mut self.unlinked_switch_jump_tables[table_index]
    }

    pub fn number_of_unlinked_string_switch_jump_tables(&self) -> usize {
        self.unlinked_string_switch_jump_tables.len()
    }

    pub fn add_unlinked_string_switch_jump_table(&mut self) -> &mut UnlinkedStringJumpTable {
        self.unlinked_string_switch_jump_tables.push(UnlinkedStringJumpTable::default());
        self.unlinked_string_switch_jump_tables.last_mut().expect("tabela recém-inserida")
    }

    pub fn unlinked_string_switch_jump_table(&mut self, table_index: usize) -> &mut UnlinkedStringJumpTable {
        &mut self.unlinked_string_switch_jump_tables[table_index]
    }

    pub fn number_of_exception_handlers(&self) -> usize {
        self.exception_handlers.len()
    }

    pub fn exception_handler(&mut self, index: usize) -> &mut UnlinkedHandlerInfo {
        &mut self.exception_handlers[index]
    }

    pub fn add_exception_handler(&mut self, handler: UnlinkedHandlerInfo) {
        self.exception_handlers.push(handler);
    }

    /// `handlerForBytecodeIndex`.
    pub fn handler_for_bytecode_index(
        &self,
        bytecode_index: BytecodeIndex,
        required_handler: RequiredHandler,
    ) -> Option<&UnlinkedHandlerInfo> {
        self.handler_for_index(bytecode_index.offset(), required_handler)
    }

    /// `handlerForIndex`.
    pub fn handler_for_index(&self, index: u32, required_handler: RequiredHandler) -> Option<&UnlinkedHandlerInfo> {
        HandlerInfoBase::handler_for_index::<UnlinkedHandlerInfo, _>(self.exception_handlers.iter(), index, required_handler)
    }

    pub fn bit_vector(&mut self, i: usize) -> &mut BitVector {
        &mut self.bit_vectors[i]
    }

    pub fn add_bit_vector(&mut self, bit_vector: BitVector) -> u32 {
        self.bit_vectors.push(bit_vector);
        (self.bit_vectors.len() - 1) as u32
    }

    pub fn number_of_constant_identifier_sets(&self) -> u32 {
        self.constant_identifier_sets.len() as u32
    }

    pub fn constant_identifier_sets(&self) -> &[IdentifierSet] {
        &self.constant_identifier_sets
    }

    pub fn add_set_constant(&mut self, set: IdentifierSet) -> u32 {
        let result = self.constant_identifier_sets.len() as u32;
        self.constant_identifier_sets.push(set);
        result
    }

    /// `constantRegister(VirtualRegister)` e `getConstant(VirtualRegister)`.
    pub fn constant_register(&self, reg: VirtualRegister) -> JSValue {
        self.constant_registers[reg.to_constant_index() as usize]
    }

    pub fn constant_registers(&self) -> &[JSValue] {
        &self.constant_registers
    }

    pub fn constant_source_code_representation(&self, reg: VirtualRegister) -> SourceCodeRepresentation {
        self.constant_source_code_representation_at(reg.to_constant_index() as usize)
    }

    /// `constantSourceCodeRepresentation(unsigned index)`.
    pub fn constant_source_code_representation_at(&self, index: usize) -> SourceCodeRepresentation {
        if index < self.constants_source_code_representation.len() {
            return self.constants_source_code_representation[index];
        }
        SourceCodeRepresentation::Other
    }

    /// `addConstant(JSValue, SourceCodeRepresentation)`; o argumento padrão do C++ (`Other`) é
    /// `add_constant`.
    pub fn add_constant_with_representation(&mut self, value: JSValue, source_code_representation: SourceCodeRepresentation) -> u32 {
        let result = self.constant_registers.len() as u32;
        self.constant_registers.push(value);
        self.constants_source_code_representation.push(source_code_representation);
        result
    }

    /// `addConstant(JSValue)`.
    pub fn add_constant(&mut self, value: JSValue) -> u32 {
        self.add_constant_with_representation(value, SourceCodeRepresentation::Other)
    }

    /// `addConstant(LinkTimeConstant)`: o valor guardado é `jsNumber(int32_t)` da constante.
    pub fn add_link_time_constant(&mut self, link_time_constant: LinkTimeConstant) -> u32 {
        self.add_constant_with_representation(
            JSValue::Int32(link_time_constant as i32),
            SourceCodeRepresentation::LinkTimeConstant,
        )
    }

    pub fn add_function_decl(&mut self, executable: UnlinkedFunctionExecutableRef) -> u32 {
        let size = self.function_decls.len() as u32;
        self.function_decls.push(executable);
        size
    }

    pub fn add_function_expr(&mut self, executable: UnlinkedFunctionExecutableRef) -> u32 {
        let size = self.function_exprs.len() as u32;
        self.function_exprs.push(executable);
        size
    }

    pub fn number_of_identifiers(&self) -> u32 {
        self.identifiers.len() as u32
    }

    pub fn identifier(&self, index: usize) -> &Identifier {
        &self.identifiers[index]
    }

    pub fn add_identifier(&mut self, identifier: Identifier) {
        self.identifiers.push(identifier);
    }

    /// `addOutOfLineJumpTarget`.
    pub fn add_out_of_line_jump_target(&mut self, bytecode_offset: u32, target: i32) {
        assert!(target != 0);
        self.out_of_line_jump_targets.insert(bytecode_offset, target);
    }

    /// `outOfLineJumpOffset(JSInstructionStream::Offset)`.
    pub fn out_of_line_jump_offset(&self, bytecode_offset: u32) -> i32 {
        debug_assert!(self.out_of_line_jump_targets.contains_key(&bytecode_offset));
        self.out_of_line_jump_targets[&bytecode_offset]
    }

    /// `replaceOutOfLineJumpTargets()`: devolve o mapa e deixa um vazio no lugar (`std::swap`).
    pub fn replace_out_of_line_jump_targets(&mut self) -> OutOfLineJumpTargets {
        std::mem::take(&mut self.out_of_line_jump_targets)
    }

    /// `applyModification(BytecodeRewriter&)`: o `writer` é o `m_writer` do rewriter no C++.
    pub fn apply_modification(
        &mut self,
        rewriter: &mut crate::bytecode::bytecode_rewriter::BytecodeRewriter,
        writer: &mut crate::bytecode::instruction_stream::InstructionStreamWriter,
    ) {
        // Before applying the changes, we adjust the jumps based on the original bytecode offset, the offset to the jump target, and
        // the insertion information.

        rewriter.adjust_jump_targets(self, writer);

        // Then, exception handlers should be adjusted.
        for handler in &mut self.exception_handlers {
            handler.base.target = rewriter.adjust_absolute_offset(handler.base.target) as u32;
            handler.base.start = rewriter.adjust_absolute_offset(handler.base.start) as u32;
            handler.base.end = rewriter.adjust_absolute_offset(handler.base.end) as u32;
        }

        for offset in &mut self.op_profile_control_flow_bytecode_offsets {
            *offset = rewriter.adjust_absolute_offset(*offset) as u32;
        }

        if !self.type_profiler_info_map.is_empty() {
            let mut adjusted_type_profiler_info_map: HashMap<u32, TypeProfilerExpressionRange> = HashMap::new();
            for (key, value) in &self.type_profiler_info_map {
                adjusted_type_profiler_info_map.insert(rewriter.adjust_absolute_offset(*key) as u32, *value);
            }
            std::mem::swap(&mut self.type_profiler_info_map, &mut adjusted_type_profiler_info_map);
        }

        let mut bytecode_offset_adjustments: Vec<u32> = Vec::new();
        rewriter.for_each_label_point(|bytecode_offset| {
            bytecode_offset_adjustments.push(bytecode_offset as u32);
        });
        self.expression_info_encoder.remap(bytecode_offset_adjustments, |bytecode_offset| {
            rewriter.adjust_absolute_offset(bytecode_offset) as u32
        });

        // Then, modify the unlinked instructions.
        rewriter.apply_modification(self, writer);
    }

    pub fn add_binary_arith_profile(&mut self) -> u32 {
        let index = self.num_binary_arith_profiles;
        self.num_binary_arith_profiles += 1;
        index
    }

    pub fn add_unary_arith_profile(&mut self) -> u32 {
        let index = self.num_unary_arith_profiles;
        self.num_unary_arith_profiles += 1;
        index
    }

    /// `finalize(std::unique_ptr<JSInstructionStream>)`: instala no `UnlinkedCodeBlock` tudo o que o
    /// gerador acumulou. Devolve `false` quando a metadata não cabe (ver `UnlinkedMetadataTable`).
    #[must_use]
    pub fn finalize(&mut self, instructions: Box<JSInstructionStream>) -> bool {
        let mut code_block = self.code_block.borrow_mut();
        code_block.instructions = Some(instructions);
        code_block.allocate_shared_profiles(self.num_binary_arith_profiles, self.num_unary_arith_profiles);
        let metadata_ok = code_block.metadata().finalize();

        code_block.identifiers = std::mem::take(&mut self.identifiers);
        code_block.constant_registers = std::mem::take(&mut self.constant_registers);
        code_block.constants_source_code_representation = std::mem::take(&mut self.constants_source_code_representation);
        code_block.function_decls = std::mem::take(&mut self.function_decls);
        code_block.function_exprs = std::mem::take(&mut self.function_exprs);
        code_block.expression_info = Some(self.expression_info_encoder.create_expression_info());

        code_block.out_of_line_jump_targets = std::mem::take(&mut self.out_of_line_jump_targets);

        if code_block.rare_data.is_none()
            && (!self.exception_handlers.is_empty()
                || !self.unlinked_switch_jump_tables.is_empty()
                || !self.unlinked_string_switch_jump_tables.is_empty()
                || !self.type_profiler_info_map.is_empty()
                || !self.op_profile_control_flow_bytecode_offsets.is_empty()
                || !self.bit_vectors.is_empty()
                || !self.constant_identifier_sets.is_empty())
        {
            code_block.create_rare_data_if_necessary();
        }
        if let Some(rare_data) = code_block.rare_data.as_mut() {
            let RareData {
                exception_handlers,
                unlinked_switch_jump_tables,
                unlinked_string_switch_jump_tables,
                type_profiler_info_map,
                op_profile_control_flow_bytecode_offsets,
                bit_vectors,
                constant_identifier_sets,
                ..
            } = &mut **rare_data;
            *exception_handlers = std::mem::take(&mut self.exception_handlers);
            *unlinked_switch_jump_tables = std::mem::take(&mut self.unlinked_switch_jump_tables);
            *unlinked_string_switch_jump_tables = std::mem::take(&mut self.unlinked_string_switch_jump_tables);
            *type_profiler_info_map = std::mem::take(&mut self.type_profiler_info_map);
            *op_profile_control_flow_bytecode_offsets = std::mem::take(&mut self.op_profile_control_flow_bytecode_offsets);
            *bit_vectors = std::mem::take(&mut self.bit_vectors);
            *constant_identifier_sets = std::mem::take(&mut self.constant_identifier_sets);
        }

        metadata_ok
    }
}

/// O que o `BytecodeGeneratorBase` usa de `Traits::CodeBlock`.
impl GeneratorCodeBlock for UnlinkedCodeBlockGenerator {
    fn num_callee_locals(&self) -> u32 {
        self.code_block.borrow().num_callee_locals()
    }

    fn set_num_callee_locals(&mut self, count: u32) {
        self.code_block.borrow_mut().num_callee_locals = count;
    }

    fn num_vars(&self) -> i32 {
        self.code_block.borrow().num_vars() as i32
    }

    fn set_num_vars(&mut self, count: i32) {
        self.code_block.borrow_mut().num_vars = count as u32;
    }
}
