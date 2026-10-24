//! Porte de `bytecompiler/BytecodeGenerator.h` e `BytecodeGenerator.cpp`.
//!
//! Esta primeira parte traz as classes auxiliares (`CallArguments`, `Variable`, `FinallyContext`,
//! `ForInContext`, `TryData` e afins) e os métodos inline do `BytecodeGenerator` até `emitNode`.
//! O `struct BytecodeGenerator` com os campos fica em `bytecode_generator_part2.rs`; aqui os métodos
//! usam os campos pelos nomes em snake_case do `m_` correspondente.

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use crate::bytecode::handler_info::HandlerType;
use crate::bytecompiler::bytecode_generator_base::{BytecodeGeneratorBase, BytecodeGeneratorTraits};
use crate::bytecompiler::label::{GenericLabelRef, LabelRef};
use crate::bytecompiler::label_scope::{LabelScope, LabelScopeRef, LabelScopeType};
use crate::bytecompiler::register_id::RegisterID;
use crate::parser::nodes::{ArgumentsNode, NodeRef, Statement};
use crate::runtime::identifier::Identifier;
use crate::runtime::property_attribute::READ_ONLY;
use crate::runtime::var_offset::VarOffset;

// Imports dos fragmentos juntados por `include!` (part2, part3, cpp1..cpp6), que não têm `use` próprio.
use crate::bytecode::bytecode_ops::*;
use crate::bytecode::code_type::CodeType;
use crate::bytecode::executable_info::{DerivedContextType, EvalContextType, NeedsClassFieldInitializer};
use crate::bytecode::link_time_constant::LinkTimeConstant;
use crate::bytecode::opcode::OpcodeID;
use crate::bytecode::virtual_register::{virtual_register_for_argument_including_this, VirtualRegister};
use crate::parser::parser_modes::{
    function_name_is_in_scope, function_name_scope_is_dynamic, is_async_function_body_parse_mode,
    is_async_function_wrapper_parse_mode, is_async_generator_wrapper_parse_mode,
    is_generator_or_async_function_body_parse_mode, is_generator_or_async_function_wrapper_parse_mode,
    is_generator_wrapper_parse_mode,
};
use crate::bytecompiler::existing_variable_mode::ExistingVariableMode;
use crate::interpreter::call_frame::CallFrameSlot;
use crate::parser::nodes::SwitchType;
use crate::parser::parser_error::ParserError;
pub use crate::parser::parser_modes::{PrivateBrandRequirement, SuperBinding};
pub use crate::parser::parser_tokens::JSTextPosition;
use crate::parser::parser_modes::{SourceParseMode, SourceParseModeSet};
use crate::parser::variable_environment::{PrivateNameEnvironment, TDZEnvironmentLink};
use crate::runtime::construct_ability::ConstructAbility;
use crate::runtime::constructor_kind::ConstructorKind;
use crate::runtime::direct_arguments_offset::DirectArgumentsOffset;
use crate::runtime::ecma_mode::ECMAMode;
pub use crate::runtime::get_put_info::ResolveMode;
use crate::runtime::get_put_info::{GetPutInfo, InitializationMode, ResolveType};
use crate::runtime::js_async_generator::AsyncGeneratorSuspendReason;
use crate::runtime::js_cjs_value_types::SourceCodeRepresentation;
use crate::runtime::js_generator::ResumeMode;
use crate::runtime::js_type::JSType;
use crate::runtime::symbol_table::{SymbolTable, SymbolTableEntry, NO_LOCKING_NECESSARY};
use crate::runtime::var_offset::VarKind;
use crate::wtf::text::atom_string_impl::UniquedStringImpl;

pub use crate::bytecompiler::identifier_map::IdentifierMap;

/// `NoExpectedFunction` como valor de topo, para quem passa o enum sem qualificá-lo.
pub const NO_EXPECTED_FUNCTION: ExpectedFunction = ExpectedFunction::NoExpectedFunction;

/// `RefPtr<RegisterID>` do C++: a contagem intrusiva vive em `register_id.rs`.
pub use crate::bytecompiler::register_id::RegisterRef;

/// `enum ExpectedFunction`.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ExpectedFunction {
    NoExpectedFunction,
    ExpectObjectConstructor,
    ExpectArrayConstructor,
}

/// `enum class EmitAwait : bool`.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum EmitAwait {
    No,
    Yes,
}

/// `enum class DebuggableCall : bool`.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum DebuggableCall {
    No,
    Yes,
}

/// `enum class ThisResolutionType`.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ThisResolutionType {
    Local,
    Scoped,
}

/// `enum class InvalidPrototypeMode : uint8_t`.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
#[repr(u8)]
pub enum InvalidPrototypeMode {
    Throw,
    Ignore,
}

/// `class CallArguments`. `m_argv` é um `std::span` sobre `m_allocatedRegisters`; aqui os registros
/// alocados são o próprio vetor e `argv` guarda o mesmo conteúdo na ordem (this primeiro).
pub struct CallArguments {
    pub arguments_node: Option<NodeRef<ArgumentsNode>>,
    pub argv: Vec<Option<RegisterRef>>,
    pub allocated_registers: Vec<Option<RegisterRef>>,
}

impl CallArguments {
    pub fn this_register(&self) -> Option<RegisterRef> {
        self.argv[0].clone()
    }

    pub fn argument_register(&self, i: usize) -> Option<RegisterRef> {
        self.argv[i + 1].clone()
    }

    pub fn stack_offset(&self) -> u32 {
        let index = match &self.argv[0] {
            Some(register) => register.borrow().index(),
            None => 0,
        };
        (-index + crate::interpreter::call_frame::HEADER_SIZE_IN_REGISTERS) as u32
    }

    pub fn argument_count_including_this(&self) -> u32 {
        self.argv.len() as u32
    }

    pub fn arguments_node(&self) -> Option<NodeRef<ArgumentsNode>> {
        self.arguments_node.clone()
    }
}

/// `Variable::VariableKind`.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum VariableKind {
    NormalVariable,
    SpecialVariable,
}

/// `class Variable`. `m_local` é `RegisterID*` (identidade por ponteiro), logo `Option<RegisterRef>`.
#[derive(Clone)]
pub struct Variable {
    pub ident: Identifier,
    pub offset: VarOffset,
    pub local: Option<RegisterRef>,
    pub attributes: u32,
    pub kind: VariableKind,
    pub symbol_table_constant_index: i32,
    pub is_lexically_scoped: bool,
}

impl Default for Variable {
    fn default() -> Self {
        Variable {
            ident: Identifier::empty_identifier(),
            offset: VarOffset::default(),
            local: None,
            attributes: 0,
            kind: VariableKind::NormalVariable,
            symbol_table_constant_index: 0,
            is_lexically_scoped: false,
        }
    }
}

impl Variable {
    pub fn from_ident(ident: &Identifier) -> Variable {
        Variable {
            ident: ident.clone(),
            offset: VarOffset::default(),
            local: None,
            attributes: 0,
            kind: VariableKind::NormalVariable, // Sem significado para este tipo de Variable.
            symbol_table_constant_index: 0, // Sem significado para este tipo de Variable.
            is_lexically_scoped: false,
        }
    }

    pub fn new(
        ident: &Identifier,
        offset: VarOffset,
        local: Option<RegisterRef>,
        attributes: u32,
        kind: VariableKind,
        symbol_table_constant_index: i32,
        is_lexically_scoped: bool,
    ) -> Variable {
        Variable {
            ident: ident.clone(),
            offset,
            local,
            attributes,
            kind,
            symbol_table_constant_index,
            is_lexically_scoped,
        }
    }

    // Se não definido, é uma variável sem escopo local. Se definido, pode ser uma variável de pilha,
    // uma variável com escopo num escopo local, ou uma variável capturada no objeto de arguments direto.
    pub fn is_resolved(&self) -> bool {
        self.offset.is_valid()
    }

    pub fn symbol_table_constant_index(&self) -> i32 {
        debug_assert!(self.is_resolved() && !self.is_special());
        self.symbol_table_constant_index
    }

    pub fn ident(&self) -> &Identifier {
        &self.ident
    }

    pub fn offset(&self) -> VarOffset {
        self.offset
    }

    pub fn is_local(&self) -> bool {
        self.offset.is_stack()
    }

    pub fn local(&self) -> Option<RegisterRef> {
        self.local.clone()
    }

    pub fn is_read_only(&self) -> bool {
        self.attributes & READ_ONLY != 0
    }

    pub fn is_special(&self) -> bool {
        self.kind != VariableKind::NormalVariable
    }

    pub fn is_const(&self) -> bool {
        self.is_read_only() && self.is_lexically_scoped
    }

    pub fn set_is_read_only(&mut self) {
        self.attributes |= READ_ONLY;
    }
}

/// `operator==(const Variable&, const Variable&) = default`: compara membro a membro, com o
/// `RegisterID*` por ponteiro.
impl PartialEq for Variable {
    fn eq(&self, other: &Variable) -> bool {
        let same_local = match (&self.local, &other.local) {
            (None, None) => true,
            (Some(a), Some(b)) => a.is_same_register(b),
            _ => false,
        };
        self.ident == other.ident
            && self.offset == other.offset
            && same_local
            && self.attributes == other.attributes
            && self.kind == other.kind
            && self.symbol_table_constant_index == other.symbol_table_constant_index
            && self.is_lexically_scoped == other.is_lexically_scoped
    }
}

// https://tc39.github.io/ecma262/#sec-completion-record-specification-type
//
// Nos casos Break e Continue, em vez dos valores Break e Continue do enum abaixo, o jumpID único do
// break/continue serve de codificação do CompletionType. O emitFinallyCompletion() usa esse jumpID depois
// para achar o alvo do salto após executar os blocos finally. O jumpID é:
//     jumpID = bytecodeOffset (do nó break/continue) + CompletionType::NumberOfTypes.
// Logo não há colisão entre jumpIDs e os valores do enum.
/// `enum class CompletionType : int`. O valor é um `i32` porque carrega jumpIDs além das três variantes.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct CompletionType(pub i32);

impl CompletionType {
    pub const NORMAL: CompletionType = CompletionType(0);
    pub const THROW: CompletionType = CompletionType(1);
    pub const RETURN: CompletionType = CompletionType(2);
    pub const NUMBER_OF_TYPES: CompletionType = CompletionType(3);
}

pub fn bytecode_offset_to_jump_id(offset: u32) -> CompletionType {
    let jump_id_as_int = offset as i32 + CompletionType::NUMBER_OF_TYPES.0;
    debug_assert!(jump_id_as_int >= CompletionType::NUMBER_OF_TYPES.0);
    CompletionType(jump_id_as_int)
}

/// `struct FinallyJump`. `Ref<Label>` vira `LabelRef`.
pub struct FinallyJump {
    pub jump_id: CompletionType,
    pub target_lexical_scope_index: i32,
    pub target_label: LabelRef,
}

impl FinallyJump {
    pub fn new(jump_id: CompletionType, target_lexical_scope_index: i32, target_label: LabelRef) -> FinallyJump {
        FinallyJump { jump_id, target_lexical_scope_index, target_label }
    }
}

/// O `struct { typeRegister; valueRegister; } m_completionRecord` de `FinallyContext`.
#[derive(Default)]
pub struct CompletionRecord {
    pub type_register: Option<RegisterRef>,
    pub value_register: Option<RegisterRef>,
}

/// `class FinallyContext`. O construtor `FinallyContext(BytecodeGenerator&, Label&)` está no `.cpp`.
/// `m_outerContext` é um ponteiro para o contexto externo da pilha de escopos de controle; vira
/// `Option<Rc<RefCell<FinallyContext>>>`.
#[derive(Default)]
pub struct FinallyContext {
    pub outer_context: Option<Rc<RefCell<FinallyContext>>>,
    pub finally_label: Option<LabelRef>,
    pub number_of_breaks_or_continues: u32,
    pub handles_returns: bool,
    pub jumps: Vec<FinallyJump>,
    pub completion_record: CompletionRecord,
}

impl FinallyContext {
    pub fn outer_context(&self) -> Option<Rc<RefCell<FinallyContext>>> {
        self.outer_context.clone()
    }

    pub fn finally_label(&self) -> Option<LabelRef> {
        self.finally_label.clone()
    }

    pub fn completion_type_register(&self) -> Option<RegisterRef> {
        self.completion_record.type_register.clone()
    }

    pub fn completion_value_register(&self) -> Option<RegisterRef> {
        self.completion_record.value_register.clone()
    }

    pub fn number_of_breaks_or_continues(&self) -> u32 {
        self.number_of_breaks_or_continues
    }

    /// `Checked<uint32_t, CrashOnOverflow>::operator++`: estouro derruba o processo.
    /// Nenhum fonte JS dispara o estouro: seriam mais de 2^32 `break`/`continue` num só programa, e o
    /// `Checked<uint32_t, CrashOnOverflow>` do C++ também derruba o processo.
    pub fn inc_number_of_breaks_or_continues(&mut self) {
        self.number_of_breaks_or_continues = self
            .number_of_breaks_or_continues
            .checked_add(1)
            .expect("CheckedArithmetic: estouro em numberOfBreaksOrContinues");
    }

    pub fn handles_returns(&self) -> bool {
        self.handles_returns
    }

    pub fn set_handles_returns(&mut self) {
        self.handles_returns = true;
    }

    pub fn register_jump(&mut self, jump_id: CompletionType, lexical_scope_index: i32, target_label: LabelRef) {
        self.jumps.push(FinallyJump::new(jump_id, lexical_scope_index, target_label));
    }

    pub fn number_of_jumps(&self) -> usize {
        self.jumps.len()
    }

    pub fn jumps(&mut self, i: usize) -> &mut FinallyJump {
        &mut self.jumps[i]
    }
}

/// `ControlFlowScope::Type` (`uint8_t`) e as constantes anônimas `Label`, `Finally`.
pub type ControlFlowScopeType = u8;
pub const CONTROL_FLOW_SCOPE_LABEL: ControlFlowScopeType = 0;
pub const CONTROL_FLOW_SCOPE_FINALLY: ControlFlowScopeType = 1;

/// `struct ControlFlowScope`.
#[derive(Clone)]
pub struct ControlFlowScope {
    pub type_: ControlFlowScopeType,
    pub lexical_scope_index: i32,
    pub finally_context: Option<Rc<RefCell<FinallyContext>>>,
}

impl ControlFlowScope {
    pub fn new(type_: ControlFlowScopeType, lexical_scope_index: i32, finally_context: Option<Rc<RefCell<FinallyContext>>>) -> ControlFlowScope {
        ControlFlowScope { type_, lexical_scope_index, finally_context }
    }

    pub fn is_label_scope(&self) -> bool {
        self.type_ == CONTROL_FLOW_SCOPE_LABEL
    }

    pub fn is_finally_scope(&self) -> bool {
        self.type_ == CONTROL_FLOW_SCOPE_FINALLY
    }
}

/// `ForInContext::GetInst` (e `PutInst`, `InInst`): índice da instrução e índice do registro da propriedade.
pub type ForInGetInst = (u32, i32);
pub type ForInPutInst = ForInGetInst;
pub type ForInInInst = ForInGetInst;
/// `ForInContext::HasOwnPropertyJumpInst`: índice do desvio e alvo do caminho genérico.
pub type ForInHasOwnPropertyJumpInst = (u32, u32);

/// `class ForInContext : public RefCounted<ForInContext>`. Quem compartilha usa `Rc<RefCell<ForInContext>>`.
/// O método `finalize(BytecodeGenerator&, UnlinkedCodeBlockGenerator*, unsigned)` está no `.cpp`.
pub struct ForInContext {
    pub local_register: Option<RegisterRef>,
    pub property_name: Option<RegisterRef>,
    pub property_offset: Option<RegisterRef>,
    pub enumerator: Option<RegisterRef>,
    pub mode: Option<RegisterRef>,
    pub base_variable: Option<Variable>,
    pub is_valid: bool,
    pub body_bytecode_start_offset: u32,
    pub in_insts: Vec<ForInInInst>,
    pub get_insts: Vec<ForInGetInst>,
    pub put_insts: Vec<ForInPutInst>,
    pub has_own_property_jump_insts: Vec<ForInHasOwnPropertyJumpInst>,
}

impl ForInContext {
    pub fn new(
        local_register: Option<RegisterRef>,
        property_name: Option<RegisterRef>,
        property_offset: Option<RegisterRef>,
        enumerator: Option<RegisterRef>,
        mode: Option<RegisterRef>,
        base_variable: Option<Variable>,
        body_bytecode_start_offset: u32,
    ) -> ForInContext {
        ForInContext {
            local_register,
            property_name,
            property_offset,
            enumerator,
            mode,
            base_variable,
            is_valid: true,
            body_bytecode_start_offset,
            in_insts: Vec::new(),
            get_insts: Vec::new(),
            put_insts: Vec::new(),
            has_own_property_jump_insts: Vec::new(),
        }
    }

    pub fn is_valid(&self) -> bool {
        self.is_valid
    }

    pub fn invalidate(&mut self) {
        self.is_valid = false;
    }

    pub fn local(&self) -> Option<RegisterRef> {
        self.local_register.clone()
    }

    pub fn property_name(&self) -> Option<RegisterRef> {
        self.property_name.clone()
    }

    pub fn property_offset(&self) -> Option<RegisterRef> {
        self.property_offset.clone()
    }

    pub fn enumerator(&self) -> Option<RegisterRef> {
        self.enumerator.clone()
    }

    pub fn mode(&self) -> Option<RegisterRef> {
        self.mode.clone()
    }

    pub fn base_variable(&self) -> &Option<Variable> {
        &self.base_variable
    }

    pub fn add_get_inst(&mut self, inst_index: u32, property_reg_index: i32) {
        self.get_insts.push((inst_index, property_reg_index));
    }

    pub fn add_put_inst(&mut self, inst_index: u32, property_reg_index: i32) {
        self.put_insts.push((inst_index, property_reg_index));
    }

    pub fn add_in_inst(&mut self, inst_index: u32, property_reg_index: i32) {
        self.in_insts.push((inst_index, property_reg_index));
    }

    pub fn add_has_own_property_jump(&mut self, branch_inst_index: u32, generic_path_target: u32) {
        self.has_own_property_jump_insts.push((branch_inst_index, generic_path_target));
    }

    pub fn body_bytecode_start_offset(&self) -> u32 {
        self.body_bytecode_start_offset
    }
}

/// `struct TryData`. O `TryData*` do C++ aponta para um objeto que vive num `SegmentedVector` do
/// gerador e é alterado depois; por isso o compartilhamento é `Rc<RefCell<TryData>>`.
pub struct TryData {
    pub target: LabelRef,
    pub handler_type: HandlerType,
}

/// `struct TryContext`.
pub struct TryContext {
    pub start: LabelRef,
    pub try_data: Rc<RefCell<TryData>>,
}

/// `struct TryRange`.
#[derive(Clone)]
pub struct TryRange {
    pub start: LabelRef,
    pub end: LabelRef,
    pub try_data: Rc<RefCell<TryData>>,
}

/// `struct UsingSlot`.
#[derive(Default)]
pub struct UsingSlot {
    pub value: Option<RegisterRef>,
    pub method: Option<RegisterRef>,
    pub reached: Option<RegisterRef>,
    pub is_async: bool,
}

/// `struct UsingScope`.
#[derive(Default)]
pub struct UsingScope {
    pub slots: Vec<UsingSlot>,
    pub next_slot: u32,
    pub has_await_using: bool,
}

/// `struct JSGeneratorTraits` (`BytecodeGenerator.h:347`): a marca do tipo vive em `label.rs`
/// (base de `Label`); aqui os tipos associados, `opcodeForDisablingOptimizations` e a
/// especialização `GenericLabel<JSGeneratorTraits>::setLocation(BytecodeGenerator&, unsigned)`.
pub use crate::bytecompiler::label::JSGeneratorTraits;

impl BytecodeGeneratorTraits for JSGeneratorTraits {
    type OpcodeID = crate::bytecode::opcode::OpcodeID;
    type OpcodeTraits = crate::bytecode::opcode_traits::JSOpcodeTraits;
    type CodeBlock = crate::bytecode::unlinked_code_block_generator::UnlinkedCodeBlockGenerator;

    const OPCODE_FOR_DISABLING_OPTIMIZATIONS: crate::bytecode::opcode::OpcodeID = crate::bytecode::opcode::OpcodeID::op_debug;

    fn opcode_id_value(opcode_id: crate::bytecode::opcode::OpcodeID) -> u16 {
        opcode_id as u16
    }

    fn set_label_location(
        generator: &mut BytecodeGeneratorBase<Self>,
        label: &GenericLabelRef<Self>,
        location: u32,
    ) {
        label.borrow_mut().set_location_raw(location);
        let unresolved: Vec<i32> = label.borrow().unresolved_jumps().clone();

        for offset in unresolved {
            let mut instruction = generator.writer.ref_at(offset as u32);
            let target = location as i32 - offset;

            macro_rules! case {
                ($op:ident) => {
                    if instruction.opcode_id() == crate::bytecode::bytecode_ops::$op::OPCODE_ID as u16 {
                        let instruction_offset = instruction.offset();
                        let code_block = &mut generator.code_block;
                        instruction.cast_mut::<crate::bytecode::bytecode_ops::$op>().set_target_label(
                            crate::bytecompiler::label::BoundLabel::from_offset(target),
                            &mut || {
                                code_block.add_out_of_line_jump_target(instruction_offset, target);
                                crate::bytecompiler::label::BoundLabel::new()
                            },
                        );
                        continue;
                    }
                };
            }

            case!(OpJmp);
            case!(OpJtrue);
            case!(OpJfalse);
            case!(OpJeqNull);
            case!(OpJneqNull);
            case!(OpJundefinedOrNull);
            case!(OpJnundefinedOrNull);
            case!(OpJeq);
            case!(OpJstricteq);
            case!(OpJneq);
            case!(OpJeqPtr);
            case!(OpJneqPtr);
            case!(OpJnstricteq);
            case!(OpJless);
            case!(OpJlesseq);
            case!(OpJgreater);
            case!(OpJgreatereq);
            case!(OpJnless);
            case!(OpJnlesseq);
            case!(OpJngreater);
            case!(OpJngreatereq);
            case!(OpJbelow);
            case!(OpJbeloweq);
            // default: ASSERT_NOT_REACHED()
        }
    }
}

/// `m_writer.position()`: o `BoundLabel` lê a posição do escritor do gerador (membro da base,
/// ver `bytecode_generator_part3.rs`).
impl crate::bytecompiler::label::LabelGenerator for BytecodeGenerator {
    fn writer_position(&self) -> i32 {
        self.writer.position() as i32
    }
}

/// O que o `emit` gerado de cada `Op*` pede ao gerador (`template<typename BytecodeGenerator>`):
/// `recordOpcode` e `writeOpcode<size>` da `BytecodeGeneratorBase` (o campo `base`), `addMetadataFor` (`BytecodeGenerator.h:515`) e `setUsesCheckpoints`
/// (`BytecodeGenerator.h:1110`).
impl crate::bytecode::bytecode_ops::OpWriter for BytecodeGenerator {
    fn record_opcode(&mut self, opcode_id: crate::bytecode::opcode::OpcodeID) {
        self.base.record_opcode(opcode_id);
    }

    fn write_opcode(
        &mut self,
        size: crate::bytecompiler::bytecode_generator_base::OpcodeSize,
        opcode_id: crate::bytecode::opcode::OpcodeID,
        ops: &[&dyn crate::bytecompiler::bytecode_generator_base::Fits],
    ) {
        self.base.write_opcode(size, opcode_id, ops);
    }

    fn add_metadata_for(&mut self, opcode_id: crate::bytecode::opcode::OpcodeID) -> u32 {
        self.code_block.metadata().add_entry(opcode_id)
    }

    fn set_uses_checkpoints(&mut self) {
        self.code_block.set_has_checkpoints();
    }
}

/// Contrato dos quatro construtores do `BytecodeGenerator` (Program, Function, Eval, ModuleProgram),
/// usado pelo `generate` genérico. A implementação por nó vive junto do `.cpp`.
pub trait BytecodeGeneratorNode<UnlinkedCodeBlock> {
    #[allow(clippy::too_many_arguments)]
    fn new_generator(
        vm: &Rc<crate::runtime::vm::VM>,
        node: &NodeRef<Self>,
        unlinked_code_block: &Rc<RefCell<UnlinkedCodeBlock>>,
        code_generation_mode: crate::parser::parser_modes::CodeGenerationModeSet,
        parent_scope_tdz_variables: &Option<Rc<crate::bytecode::tdz_environment::TDZEnvironmentLink>>,
        generator_or_async_wrapper_function_parameter_names: Option<&[Identifier]>,
        private_name_environment: Option<&crate::parser::variable_environment::PrivateNameEnvironment>,
    ) -> BytecodeGenerator
    where
        Self: Sized;
}

/// `BytecodeGenerator(VM&, ProgramNode*, UnlinkedProgramCodeBlock*, ...)`.
impl BytecodeGeneratorNode<crate::bytecode::unlinked_code_block::UnlinkedProgramCodeBlock>
    for crate::parser::nodes::ProgramNode
{
    fn new_generator(
        vm: &Rc<crate::runtime::vm::VM>,
        node: &NodeRef<Self>,
        unlinked_code_block: &Rc<RefCell<crate::bytecode::unlinked_code_block::UnlinkedProgramCodeBlock>>,
        code_generation_mode: crate::parser::parser_modes::CodeGenerationModeSet,
        parent_scope_tdz_variables: &Option<Rc<crate::bytecode::tdz_environment::TDZEnvironmentLink>>,
        _generator_or_async_wrapper_function_parameter_names: Option<&[Identifier]>,
        _private_name_environment: Option<&crate::parser::variable_environment::PrivateNameEnvironment>,
    ) -> BytecodeGenerator {
        BytecodeGenerator::new_program(
            vm,
            node.clone(),
            unlinked_code_block,
            code_generation_mode,
            parent_scope_tdz_variables,
            None,
            None,
        )
    }
}

/// `BytecodeGenerator(VM&, EvalNode*, UnlinkedEvalCodeBlock*, ...)`.
impl BytecodeGeneratorNode<crate::bytecode::unlinked_code_block::UnlinkedEvalCodeBlock>
    for crate::parser::nodes::EvalNode
{
    fn new_generator(
        vm: &Rc<crate::runtime::vm::VM>,
        node: &NodeRef<Self>,
        unlinked_code_block: &Rc<RefCell<crate::bytecode::unlinked_code_block::UnlinkedEvalCodeBlock>>,
        code_generation_mode: crate::parser::parser_modes::CodeGenerationModeSet,
        parent_scope_tdz_variables: &Option<Rc<crate::bytecode::tdz_environment::TDZEnvironmentLink>>,
        _generator_or_async_wrapper_function_parameter_names: Option<&[Identifier]>,
        private_name_environment: Option<&crate::parser::variable_environment::PrivateNameEnvironment>,
    ) -> BytecodeGenerator {
        BytecodeGenerator::new_eval(
            vm,
            node.clone(),
            unlinked_code_block,
            code_generation_mode,
            parent_scope_tdz_variables,
            None,
            private_name_environment,
        )
    }
}

/// `BytecodeGenerator(VM&, ModuleProgramNode*, UnlinkedModuleProgramCodeBlock*, ...)`.
impl BytecodeGeneratorNode<crate::bytecode::unlinked_code_block::UnlinkedModuleProgramCodeBlock>
    for crate::parser::nodes::ModuleProgramNode
{
    fn new_generator(
        vm: &Rc<crate::runtime::vm::VM>,
        node: &NodeRef<Self>,
        unlinked_code_block: &Rc<RefCell<crate::bytecode::unlinked_code_block::UnlinkedModuleProgramCodeBlock>>,
        code_generation_mode: crate::parser::parser_modes::CodeGenerationModeSet,
        parent_scope_tdz_variables: &Option<Rc<crate::bytecode::tdz_environment::TDZEnvironmentLink>>,
        _generator_or_async_wrapper_function_parameter_names: Option<&[Identifier]>,
        _private_name_environment: Option<&crate::parser::variable_environment::PrivateNameEnvironment>,
    ) -> BytecodeGenerator {
        BytecodeGenerator::new_for_module_program(
            vm,
            node.clone(),
            unlinked_code_block,
            code_generation_mode,
            parent_scope_tdz_variables,
        )
    }
}

/// `BytecodeGenerator(VM&, FunctionNode*, UnlinkedFunctionCodeBlock*, ...)`.
impl BytecodeGeneratorNode<crate::bytecode::unlinked_code_block::UnlinkedFunctionCodeBlock>
    for crate::parser::nodes::FunctionNode
{
    fn new_generator(
        vm: &Rc<crate::runtime::vm::VM>,
        node: &NodeRef<Self>,
        unlinked_code_block: &Rc<RefCell<crate::bytecode::unlinked_code_block::UnlinkedFunctionCodeBlock>>,
        code_generation_mode: crate::parser::parser_modes::CodeGenerationModeSet,
        parent_scope_tdz_variables: &Option<Rc<crate::bytecode::tdz_environment::TDZEnvironmentLink>>,
        generator_or_async_wrapper_function_parameter_names: Option<&[Identifier]>,
        private_name_environment: Option<&crate::parser::variable_environment::PrivateNameEnvironment>,
    ) -> BytecodeGenerator {
        // O construtor guarda os nomes como `Vec`; o parâmetro do `generate` do C++ é um ponteiro para o vetor.
        let names: Option<Vec<Identifier>> = generator_or_async_wrapper_function_parameter_names.map(|names| names.to_vec());
        BytecodeGenerator::new_function(
            vm,
            node.clone(),
            &unlinked_code_block.borrow(),
            code_generation_mode,
            parent_scope_tdz_variables,
            names.as_ref(),
            private_name_environment,
        )
    }
}

// Dados que o `.cpp` inclui no gerador e que o cabeçalho só declara.
thread_local! {
    /// Conta os `TryData` criados, para o `SegmentedVector<TryData, 8>` do `.cpp` manter índices estáveis.
    pub static TRY_DATA_COUNTER: Cell<usize> = const { Cell::new(0) };
}

impl BytecodeGenerator {
    pub fn vm(&self) -> &crate::runtime::vm::VM {
        &self.vm
    }

    pub fn property_names(&self) -> &crate::runtime::common_identifiers::CommonIdentifiers {
        &self.vm.property_names
    }

    pub fn is_constructor(&self) -> bool {
        self.code_block.is_constructor()
    }

    pub fn derived_context_type(&self) -> crate::bytecode::executable_info::DerivedContextType {
        self.derived_context_type
    }

    pub fn uses_arrow_function(&self) -> bool {
        self.scope_node.borrow().uses_arrow_function()
    }

    pub fn needs_to_update_arrow_function_context(&self) -> bool {
        self.needs_to_update_arrow_function_context
    }

    pub fn uses_eval(&self) -> bool {
        self.scope_node.borrow().uses_eval()
    }

    pub fn uses_this(&self) -> bool {
        self.scope_node.borrow().uses_this()
    }

    pub fn is_function_node(&self) -> bool {
        self.scope_node.is_function_node()
    }

    pub fn has_shadows_arguments_code_feature(&self) -> bool {
        self.scope_node.borrow().has_shadows_arguments_feature()
    }

    pub fn is_async_function_without_await(&self) -> bool {
        self.scope_node.borrow().is_async_function_without_await()
    }

    pub fn lexically_scoped_features(&self) -> crate::parser::parser_modes::LexicallyScopedFeatures {
        self.scope_node.borrow().lexically_scoped_features
    }

    pub fn private_brand_requirement(&self) -> crate::parser::parser_modes::PrivateBrandRequirement {
        self.code_block.private_brand_requirement()
    }

    pub fn constructor_kind(&self) -> crate::runtime::constructor_kind::ConstructorKind {
        self.code_block.constructor_kind()
    }

    pub fn super_binding(&self) -> crate::parser::parser_modes::SuperBinding {
        self.code_block.super_binding()
    }

    pub fn script_mode(&self) -> crate::parser::parser_modes::JSParserScriptMode {
        self.code_block.script_mode()
    }

    pub fn needs_class_field_initializer(&self) -> crate::bytecode::executable_info::NeedsClassFieldInitializer {
        self.code_block.needs_class_field_initializer()
    }

    /// `static ParserError generate(...)`. O ramo `Options::reportBytecodeCompileTimes()` só imprime
    /// diagnóstico com `dataLogLn` (saída de depuração do processo, sem efeito observável pelo programa
    /// JS), logo some. O `DeferGC deferGC(vm)` também some: não há `Heap` portado (o GC é o `Rc` do
    /// Rust), então a guarda não tem efeito observável.
    #[allow(clippy::too_many_arguments)]
    pub fn generate_for<Node: BytecodeGeneratorNode<UnlinkedCodeBlock>, UnlinkedCodeBlock>(
        vm: &Rc<crate::runtime::vm::VM>,
        node: &NodeRef<Node>,
        _source_code: &crate::parser::source_code::SourceCode,
        unlinked_code_block: &Rc<RefCell<UnlinkedCodeBlock>>,
        code_generation_mode: crate::parser::parser_modes::CodeGenerationModeSet,
        parent_scope_tdz_variables: &Option<Rc<crate::bytecode::tdz_environment::TDZEnvironmentLink>>,
        generator_or_async_wrapper_function_parameter_names: Option<&[Identifier]>,
        private_name_environment: Option<&crate::parser::variable_environment::PrivateNameEnvironment>,
    ) -> crate::parser::parser_error::ParserError {
        let mut bytecode_generator = Node::new_generator(
            vm,
            node,
            unlinked_code_block,
            code_generation_mode,
            parent_scope_tdz_variables,
            generator_or_async_wrapper_function_parameter_names,
            private_name_environment,
        );
        let (error, _size) = bytecode_generator.generate();
        error
    }

    // Retorna o registro que guarda "this".
    pub fn this_register(&self) -> RegisterRef {
        // `&m_thisRegister`: o campo é RegisterID por valor e os callees pedem RegisterRef; o handle
        // compartilha só o índice virtual (igual a emit_to_this_this_register).
        crate::bytecompiler::register_id::RegisterRef::new(&std::rc::Rc::new(std::cell::RefCell::new(
            crate::bytecompiler::register_id::RegisterID::from_virtual_register(self.this_register.virtual_register()),
        )))
    }

    pub fn arguments_register(&self) -> Option<RegisterRef> {
        self.arguments_register.clone()
    }

    pub fn new_target(&self) -> RegisterRef {
        debug_assert!(self.new_target_register.is_some());
        self.new_target_register.clone().expect("newTargetRegister ausente")
    }

    pub fn scope_register(&self) -> Option<RegisterRef> {
        self.scope_register.clone()
    }

    pub fn generator_register(&self) -> Option<RegisterRef> {
        self.generator_register.clone()
    }

    pub fn promise_register(&self) -> Option<RegisterRef> {
        self.promise_register.clone()
    }

    // Igual a newTemporary(), mas devolve "suggestion" se ela for temporária. Serve quando você pôs
    // "suggestion" num RefPtr, mas quer permitir que a próxima instrução a sobrescreva mesmo assim.
    pub fn new_temporary_or(&mut self, suggestion: &RegisterRef) -> RegisterRef {
        if suggestion.borrow().is_temporary() {
            suggestion.clone()
        } else {
            self.new_temporary()
        }
    }

    // Funções para tratar o registro dst

    pub fn ignored_result(&self) -> RegisterRef {
        // `&m_ignoredResultRegister`: o campo é RegisterID por valor; o handle compartilha o índice
        // virtual (inválido, sem `set_index`), que é o que `is_ignored_result` compara.
        crate::bytecompiler::register_id::RegisterRef::new(&Rc::new(RefCell::new(
            crate::bytecompiler::register_id::RegisterID::from_virtual_register(
                self.ignored_result_register.raw_virtual_register(),
            ),
        )))
    }

    // `dst == ignoredResult()` do C++ (comparação do `RegisterID*`). O campo `m_ignoredResultRegister`
    // é único e é o único registrador sem índice virtual válido, então o índice virtual identifica o
    // objeto mesmo quando o handle foi copiado.
    pub fn is_ignored_result(&self, register: &RegisterRef) -> bool {
        register.borrow().raw_virtual_register() == self.ignored_result_register.raw_virtual_register()
    }

    // Retorna um lugar para escrever valores intermediários de uma operação, reaproveitando dst se for
    // seguro.
    pub fn temp_destination(&mut self, dst: Option<&RegisterRef>) -> RegisterRef {
        match dst {
            Some(d) if !self.is_ignored_result(d) && d.borrow().is_temporary() => d.clone(),
            _ => self.new_temporary(),
        }
    }

    // Retorna o lugar onde escrever a saída final de uma operação.
    pub fn final_destination(&mut self, original_dst: Option<&RegisterRef>, temp_dst: Option<&RegisterRef>) -> RegisterRef {
        if let Some(d) = original_dst {
            if !self.is_ignored_result(d) {
                return d.clone();
            }
        }
        if let Some(t) = temp_dst {
            debug_assert!(!self.is_ignored_result(t));
            if t.borrow().is_temporary() {
                return t.clone();
            }
        }
        self.new_temporary()
    }

    pub fn destination_for_assign_result(&mut self, dst: Option<&RegisterRef>) -> Option<RegisterRef> {
        if let Some(d) = dst {
            if !self.is_ignored_result(d) {
                return Some(if d.borrow().is_temporary() { d.clone() } else { self.new_temporary() });
            }
        }
        None
    }

    // Move src para dst se dst não for nulo e for diferente de src, senão só devolve src.
    pub fn move_register(&mut self, dst: Option<&RegisterRef>, src: &RegisterRef) -> Option<RegisterRef> {
        match dst {
            Some(d) if self.is_ignored_result(d) => None,
            // `dst != src`: ponteiros do C++; aqui, o mesmo `Rc` ou o mesmo índice virtual (handles de
            // `this_register()` são cópias).
            Some(d) if !d.is_same_register(src) => self.emit_move(d, src),
            _ => Some(src.clone()),
        }
    }

    pub fn emit_node(&mut self, dst: Option<&RegisterRef>, n: &Statement) {
        let tail_position_poisoner = self.allow_tail_call_optimization;
        self.allow_tail_call_optimization = false;
        let call_ignore_result_position_poisoner = self.allow_call_ignore_result_optimization;
        self.allow_call_ignore_result_optimization = false;
        self.emit_node_in_tail_position_statement(dst.cloned(), n);
        // `SetForScope`: restaura os valores ao sair do escopo.
        self.allow_call_ignore_result_optimization = call_ignore_result_position_poisoner;
        self.allow_tail_call_optimization = tail_position_poisoner;
    }
}

// Os fragmentos abaixo seguem a ordem do .h e do .cpp; todos compartilham o escopo deste módulo
// (e, por isso, os `use` do topo), então `NestedScopeType`, `TDZCheckOptimization`,
// `PreservedTDZStack` (part3) e `PROPERTY_*` (part2) ficam acessíveis por
// `crate::bytecompiler::bytecode_generator::...` sem reexportação.

// BytecodeGenerator.h, linha 502 em diante: o struct e os métodos.
include!("bytecode_generator_part2.rs");
include!("bytecode_generator_part3.rs");
// BytecodeGenerator.cpp.
include!("bytecode_generator_cpp1.rs");
include!("bytecode_generator_cpp1c.rs");
include!("bytecode_generator_cpp2.rs");
include!("bytecode_generator_cpp3.rs");
include!("bytecode_generator_cpp4.rs");
include!("bytecode_generator_cpp5.rs");
include!("bytecode_generator_cpp6.rs");
include!("bytecode_generator_cpp7.rs");
