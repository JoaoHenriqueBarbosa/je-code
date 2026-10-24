//! Porte de `bytecode/UnlinkedCodeBlock.h` e `UnlinkedCodeBlock.cpp`: o `UnlinkedCodeBlock` base e as
//! tabelas de salto (`UnlinkedStringJumpTable`, `UnlinkedSimpleJumpTable`) que ele guarda.
//!
//! Divergências (documentadas, nenhuma muda o que o programa JS observa):
//!
//! - Sem `JSCell`: o `UnlinkedCodeBlock` é um struct comum (o bytecompiler o guarda em
//!   `Rc<RefCell<..>>`). `visitChildren`, `estimatedSize`, `subspaceFor`, `DECLARE_INFO`, o
//!   `ConcurrentJSLock`, `WriteBarrier` e a destruição por heap não existem; o `WriteBarrier<Unknown>`
//!   das constantes é o próprio `JSValue`, e o `WriteBarrier<UnlinkedFunctionExecutable>` é a
//!   referência compartilhada `UnlinkedFunctionExecutableRef`.
//! - `FixedVector<T>` é `Vec<T>` (ficam imutáveis em tamanho depois do `finalize`).
//! - Os bitfields (`m_superBinding : 1`, `m_constructorKind : 2` ...) guardam o enum, que tem o mesmo
//!   conjunto de valores. `OptionSet<CodeGenerationMode>` chega como os bits crus (`u8`), como no
//!   `parser::source_code_key`.
//! - Camadas de JIT e de tiering (`m_unlinkedBaselineCode`, `m_llintExecuteCounter`, `m_quickDFGTierUp`
//!   com o `DFG::ExitProfile`, `thresholdForJIT`, `BytecodeLivenessAnalysis`) e depuração
//!   (`dump`, `dumpExpressionInfo`, `hasIdentifier`) não existem; `m_quickDFGTierUp`/`m_quickFTLTierUp`
//!   também somem. `initializeLoopHintExecutionCounter` e o destrutor só agem com a opção de fuzzing
//!   `returnEarlyFromInfiniteLoopsForFuzzing`, que fica desligada.
//! - Os perfis compartilhados (`UnlinkedValueProfile`, `UnlinkedArrayProfile`, `BinaryArithProfile`,
//!   `UnaryArithProfile`) ainda não têm porte: `allocateSharedProfiles` guarda as contagens que o C++
//!   usa para dimensioná-los (`number_of_*_profiles`).
//! - `instructionAt`/`bytecodeOffset` por ponteiro: `instruction_at` devolve o `Ref` do fluxo (que
//!   já carrega o deslocamento).
//! - `typeProfilerExpressionInfoForBytecodeOffset` devolve `Option` no lugar do `bool` com os dois
//!   parâmetros de saída em `UINT_MAX`.

use std::collections::HashMap;
use std::rc::Rc;

use crate::bytecode::bytecode_index::BytecodeIndex;
use crate::bytecode::code_type::CodeType;
use crate::bytecode::executable_info::{
    DerivedContextType, EvalContextType, ExecutableInfo, NeedsClassFieldInitializer,
};
use crate::bytecode::expression_info::{Entry as ExpressionInfoEntry, ExpressionInfo};
use crate::bytecode::handler_info::{HandlerInfoBase, RequiredHandler, UnlinkedHandlerInfo};
use crate::bytecode::instruction_stream::{JSInstructionStream, Ref};
use crate::bytecode::line_column::LineColumn;
use crate::bytecode::opcode::OpcodeID;
use crate::bytecode::unlinked_metadata_table::UnlinkedMetadataTable;
use crate::bytecode::virtual_register::VirtualRegister;
use crate::parser::parser::IdentifierSet;
use crate::parser::parser_modes::{
    is_arrow_function_parse_mode, CodeGenerationMode, JSParserScriptMode, PrivateBrandRequirement, SourceParseMode,
    SuperBinding,
};
use crate::runtime::constructor_kind::ConstructorKind;
use crate::runtime::identifier::Identifier;
use crate::runtime::js_cjs_value_types::SourceCodeRepresentation;
use crate::runtime::js_value::JSValue;
use crate::runtime::unlinked_function_executable::UnlinkedFunctionExecutableRef;
use crate::wtf::text::string_impl::{equal, StringImpl, MAX_LENGTH};
use crate::wtf::tri_state::TriState;

/// `typedef unsigned UnlinkedArrayAllocationProfile`.
pub type UnlinkedArrayAllocationProfile = u32;

/// `typedef unsigned UnlinkedObjectAllocationProfile`.
pub type UnlinkedObjectAllocationProfile = u32;

/// `UnlinkedStringJumpTable::OffsetLocation`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct OffsetLocation {
    pub branch_offset: i32,
    pub index_in_table: u32,
}

/// Chave de `StringOffsetTable` (`RefPtr<StringImpl>` com o `DefaultHash` do WTF, o `StringHash`):
/// igualdade e hash pelo conteúdo, não pela identidade, porque `offsetForValue` consulta com uma
/// string que não precisa ser a mesma instância (nem átomo).
#[derive(Clone, Debug)]
pub struct StringHashKey(pub Rc<StringImpl>);

impl PartialEq for StringHashKey {
    fn eq(&self, other: &StringHashKey) -> bool {
        equal(&self.0, &other.0)
    }
}

impl Eq for StringHashKey {}

impl std::hash::Hash for StringHashKey {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        self.0.hash().hash(state);
    }
}

/// `UnlinkedStringJumpTable`.
#[derive(Clone, Debug)]
pub struct UnlinkedStringJumpTable {
    /// `m_offsetTable` (`MemoryCompactLookupOnlyRobinHoodHashMap`): só se consulta por chave.
    pub offset_table: HashMap<StringHashKey, OffsetLocation>,
    pub min_length: u32,
    pub max_length: u32,
    pub default_offset: i32,
}

impl Default for UnlinkedStringJumpTable {
    fn default() -> UnlinkedStringJumpTable {
        UnlinkedStringJumpTable {
            offset_table: HashMap::new(),
            min_length: MAX_LENGTH,
            max_length: 0,
            default_offset: 0,
        }
    }
}

impl UnlinkedStringJumpTable {
    /// `offsetForValue(StringImpl*)`.
    pub fn offset_for_value(&self, value: &Rc<StringImpl>) -> i32 {
        match self.offset_table.get(&StringHashKey(Rc::clone(value))) {
            None => self.default_offset,
            Some(location) => location.branch_offset,
        }
    }

    /// `indexForValue(StringImpl*, unsigned defaultIndex)`.
    pub fn index_for_value(&self, value: &Rc<StringImpl>, default_index: u32) -> u32 {
        match self.offset_table.get(&StringHashKey(Rc::clone(value))) {
            None => default_index,
            Some(location) => location.index_in_table,
        }
    }

    /// `m_offsetTable.add(key, location).isNewEntry`: só grava se a chave ainda não existe.
    pub fn add_offset(&mut self, key: Rc<StringImpl>, location: OffsetLocation) -> bool {
        match self.offset_table.entry(StringHashKey(key)) {
            std::collections::hash_map::Entry::Occupied(_) => false,
            std::collections::hash_map::Entry::Vacant(vacant) => {
                vacant.insert(location);
                true
            }
        }
    }

    /// `m_offsetTable.size()`.
    pub fn offset_table_size(&self) -> usize {
        self.offset_table.len()
    }

    /// `m_offsetTable.isEmpty()`.
    pub fn offset_table_is_empty(&self) -> bool {
        self.offset_table.is_empty()
    }

    /// `m_offsetTable.find(key)->value.m_indexInTable = index`.
    pub fn set_index_in_table(&mut self, key: &Rc<StringImpl>, index: u32) {
        if let Some(location) = self.offset_table.get_mut(&StringHashKey(Rc::clone(key))) {
            location.index_in_table = index;
        }
    }

    pub fn min_length(&self) -> u32 {
        self.min_length
    }

    pub fn max_length(&self) -> u32 {
        self.max_length
    }

    pub fn default_offset(&self) -> i32 {
        self.default_offset
    }
}

/// `UnlinkedSimpleJumpTable`.
#[derive(Clone, Debug, Default)]
pub struct UnlinkedSimpleJumpTable {
    pub branch_offsets: Vec<i32>,
    pub min: i32,
    pub default_offset: i32,
    pub is_list: i32,
}

impl UnlinkedSimpleJumpTable {
    /// `offsetForValue(int32_t)`. A tabela em lista (switch esparso) guarda pares chave/deslocamento.
    pub fn offset_for_value(&self, value: i32) -> i32 {
        if self.is_list() {
            return self
                .branch_offsets
                .chunks_exact(2)
                .find(|pair| pair[0] == value)
                .map_or(self.default_offset, |pair| pair[1]);
        }
        if value >= self.min && (value.wrapping_sub(self.min) as u32 as usize) < self.branch_offsets.len() {
            let offset = self.branch_offsets[value.wrapping_sub(self.min) as usize];
            if offset != 0 {
                return offset;
            }
        }
        self.default_offset
    }

    /// `add(key, offset)`: o primeiro deslocamento de cada chave vale.
    pub fn add(&mut self, key: i32, offset: i32) {
        if self.branch_offsets[key as usize] == 0 {
            self.branch_offsets[key as usize] = offset;
        }
    }

    pub fn default_offset(&self) -> i32 {
        self.default_offset
    }

    /// `isList()`: tabela em pares chave/deslocamento, usada nos switches esparsos.
    pub fn is_list(&self) -> bool {
        self.is_list != 0
    }
}

/// `RareData::TypeProfilerExpressionRange`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TypeProfilerExpressionRange {
    pub start_divot: u32,
    pub end_divot: u32,
}

/// `UnlinkedCodeBlock::RareData`.
#[derive(Clone, Debug)]
pub struct RareData {
    pub exception_handlers: Vec<UnlinkedHandlerInfo>,

    // Jump Tables
    pub unlinked_switch_jump_tables: Vec<UnlinkedSimpleJumpTable>,
    pub unlinked_string_switch_jump_tables: Vec<UnlinkedStringJumpTable>,

    pub type_profiler_info_map: HashMap<u32, TypeProfilerExpressionRange>,
    pub op_profile_control_flow_bytecode_offsets: Vec<u32>,
    pub bit_vectors: Vec<crate::wtf::bit_vector::BitVector>,
    pub constant_identifier_sets: Vec<IdentifierSet>,

    pub needs_class_field_initializer: NeedsClassFieldInitializer,
    pub private_brand_requirement: PrivateBrandRequirement,
}

impl Default for RareData {
    /// `makeUnique<RareData>()`: os dois bitfields começam em zero (`No` e `None`).
    fn default() -> RareData {
        RareData {
            exception_handlers: Vec::new(),
            unlinked_switch_jump_tables: Vec::new(),
            unlinked_string_switch_jump_tables: Vec::new(),
            type_profiler_info_map: HashMap::new(),
            op_profile_control_flow_bytecode_offsets: Vec::new(),
            bit_vectors: Vec::new(),
            constant_identifier_sets: Vec::new(),
            needs_class_field_initializer: NeedsClassFieldInitializer::No,
            private_brand_requirement: PrivateBrandRequirement::None,
        }
    }
}

/// `FOR_EACH_OPCODE_WITH_SIMPLE_ARRAY_PROFILE` (`Opcode.h`).
pub const OPCODES_WITH_SIMPLE_ARRAY_PROFILE: [OpcodeID; 15] = [
    OpcodeID::op_get_length,
    OpcodeID::op_get_by_val,
    OpcodeID::op_in_by_val,
    OpcodeID::op_put_by_val,
    OpcodeID::op_put_by_val_direct,
    OpcodeID::op_enumerator_next,
    OpcodeID::op_enumerator_get_by_val,
    OpcodeID::op_enumerator_in_by_val,
    OpcodeID::op_enumerator_put_by_val,
    OpcodeID::op_enumerator_has_own_property,
    OpcodeID::op_new_array_with_species,
    OpcodeID::op_call,
    OpcodeID::op_call_ignore_result,
    OpcodeID::op_tail_call,
    OpcodeID::op_iterator_open,
];

/// `class UnlinkedCodeBlock` (a base de `UnlinkedGlobalCodeBlock` e `UnlinkedFunctionCodeBlock`).
pub struct UnlinkedCodeBlock {
    this_register: VirtualRegister,
    scope_register: VirtualRegister,

    pub(crate) num_vars: u32,
    pub(crate) num_callee_locals: u32,
    is_constructor: bool,
    num_parameters: u32,

    is_builtin_function: bool,
    is_builtin_default_class_constructor: bool,
    super_binding: SuperBinding,
    script_mode: JSParserScriptMode,
    is_arrow_function_context: bool,
    is_class_context: bool,
    has_tail_calls: bool,
    constructor_kind: ConstructorKind,
    derived_context_type: DerivedContextType,
    eval_context_type: EvalContextType,
    code_type: CodeType,
    age: u32,
    has_checkpoints: bool,

    parse_mode: SourceParseMode,
    /// `OptionSet<CodeGenerationMode>::toRaw()`.
    code_generation_mode: u8,

    metadata: UnlinkedMetadataTable,
    pub(crate) instructions: Option<Box<JSInstructionStream>>,

    // Constant Pools
    pub(crate) identifiers: Vec<Identifier>,
    pub(crate) constant_registers: Vec<JSValue>,
    pub(crate) constants_source_code_representation: Vec<SourceCodeRepresentation>,
    pub(crate) function_decls: Vec<UnlinkedFunctionExecutableRef>,
    pub(crate) function_exprs: Vec<UnlinkedFunctionExecutableRef>,

    pub(crate) out_of_line_jump_targets: HashMap<u32, i32>,
    pub(crate) rare_data: Option<Box<RareData>>,
    pub(crate) expression_info: Option<Box<ExpressionInfo>>,
    number_of_value_profiles: u32,
    number_of_array_profiles: u32,
    number_of_binary_arith_profiles: u32,
    number_of_unary_arith_profiles: u32,
}

impl UnlinkedCodeBlock {
    /// `static constexpr unsigned maxAge = 7`.
    pub const MAX_AGE: u32 = 7;

    /// A base compartilhada que as filhas guardam, já conhecida do coletor: as constantes do bloco são raízes
    /// enquanto ele viver (o `visitChildren` do `UnlinkedCodeBlock`).
    fn new_shared(code_type: CodeType, info: &ExecutableInfo, code_generation_mode: u8) -> Rc<RefCell<UnlinkedCodeBlock>> {
        let base = Rc::new(RefCell::new(UnlinkedCodeBlock::new(code_type, info, code_generation_mode)));
        crate::runtime::gc::register_unlinked_code_block(&base);
        base
    }

    /// `UnlinkedCodeBlock(VM&, Structure*, CodeType, const ExecutableInfo&, OptionSet<CodeGenerationMode>)`.
    pub fn new(code_type: CodeType, info: &ExecutableInfo, code_generation_mode: u8) -> UnlinkedCodeBlock {
        let mut code_block = UnlinkedCodeBlock {
            this_register: VirtualRegister::default(),
            scope_register: VirtualRegister::default(),
            num_vars: 0,
            num_callee_locals: 0,
            is_constructor: info.is_constructor(),
            num_parameters: 0,
            is_builtin_function: info.is_builtin_function(),
            is_builtin_default_class_constructor: info.is_builtin_default_class_constructor(),
            super_binding: info.super_binding(),
            script_mode: info.script_mode(),
            is_arrow_function_context: info.is_arrow_function_context(),
            is_class_context: info.is_class_context(),
            has_tail_calls: false,
            constructor_kind: info.constructor_kind(),
            derived_context_type: info.derived_context_type(),
            eval_context_type: info.eval_context_type(),
            code_type,
            age: 0,
            has_checkpoints: false,
            parse_mode: info.parse_mode(),
            code_generation_mode,
            metadata: UnlinkedMetadataTable::create(),
            instructions: None,
            identifiers: Vec::new(),
            constant_registers: Vec::new(),
            constants_source_code_representation: Vec::new(),
            function_decls: Vec::new(),
            function_exprs: Vec::new(),
            out_of_line_jump_targets: HashMap::new(),
            rare_data: None,
            expression_info: None,
            number_of_value_profiles: 0,
            number_of_array_profiles: 0,
            number_of_binary_arith_profiles: 0,
            number_of_unary_arith_profiles: 0,
        };
        if info.needs_class_field_initializer() == NeedsClassFieldInitializer::Yes {
            code_block.create_rare_data_if_necessary();
            if let Some(rare_data) = code_block.rare_data.as_mut() {
                rare_data.needs_class_field_initializer = NeedsClassFieldInitializer::Yes;
            }
        }
        if info.private_brand_requirement() == PrivateBrandRequirement::Needed {
            code_block.create_rare_data_if_necessary();
            if let Some(rare_data) = code_block.rare_data.as_mut() {
                rare_data.private_brand_requirement = PrivateBrandRequirement::Needed;
            }
        }
        code_block
    }

    /// `createRareDataIfNecessary(const AbstractLocker&)`.
    pub(crate) fn create_rare_data_if_necessary(&mut self) {
        if self.rare_data.is_none() {
            self.rare_data = Some(Box::default());
        }
    }

    pub fn is_constructor(&self) -> bool {
        self.is_constructor
    }

    pub fn parse_mode(&self) -> SourceParseMode {
        self.parse_mode
    }

    pub fn is_arrow_function(&self) -> bool {
        is_arrow_function_parse_mode(self.parse_mode())
    }

    pub fn derived_context_type(&self) -> DerivedContextType {
        self.derived_context_type
    }

    pub fn eval_context_type(&self) -> EvalContextType {
        self.eval_context_type
    }

    pub fn is_arrow_function_context(&self) -> bool {
        self.is_arrow_function_context
    }

    pub fn is_class_context(&self) -> bool {
        self.is_class_context
    }

    pub fn has_tail_calls(&self) -> bool {
        self.has_tail_calls
    }

    pub fn set_has_tail_calls(&mut self) {
        self.has_tail_calls = true;
    }

    pub fn is_builtin_default_class_constructor(&self) -> bool {
        self.is_builtin_default_class_constructor
    }

    pub fn has_expression_info(&self) -> bool {
        self.expression_info.as_ref().is_some_and(|info| !info.is_empty())
    }

    pub fn has_checkpoints(&self) -> bool {
        self.has_checkpoints
    }

    pub fn set_has_checkpoints(&mut self) {
        self.has_checkpoints = true;
    }

    // Special registers
    pub fn set_this_register(&mut self, this_register: VirtualRegister) {
        self.this_register = this_register;
    }

    pub fn set_scope_register(&mut self, scope_register: VirtualRegister) {
        self.scope_register = scope_register;
    }

    // Parameter information
    pub fn set_num_parameters(&mut self, new_value: u32) {
        self.num_parameters = new_value;
    }

    pub fn num_parameters(&self) -> u32 {
        self.num_parameters
    }

    // Constant Pools

    pub fn number_of_identifiers(&self) -> usize {
        self.identifiers.len()
    }

    pub fn identifier(&self, index: usize) -> &Identifier {
        &self.identifiers[index]
    }

    pub fn identifiers(&self) -> &[Identifier] {
        &self.identifiers
    }

    pub fn bit_vector(&mut self, i: usize) -> &mut crate::wtf::bit_vector::BitVector {
        debug_assert!(self.rare_data.is_some());
        &mut self.rare_data.as_mut().expect("rare data").bit_vectors[i]
    }

    pub fn constant_registers(&self) -> &[JSValue] {
        &self.constant_registers
    }

    /// `constantRegister(VirtualRegister)` e `getConstant(VirtualRegister)`: o `WriteBarrier<Unknown>`
    /// é o próprio `JSValue`, então `get()` é a identidade.
    pub fn constant_register(&self, reg: VirtualRegister) -> JSValue {
        self.constant_registers[reg.to_constant_index() as usize]
    }

    pub fn constants_source_code_representation(&self) -> &[SourceCodeRepresentation] {
        &self.constants_source_code_representation
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

    pub fn number_of_constant_identifier_sets(&self) -> usize {
        self.rare_data.as_ref().map_or(0, |rare_data| rare_data.constant_identifier_sets.len())
    }

    pub fn constant_identifier_sets(&self) -> &[IdentifierSet] {
        debug_assert!(self.rare_data.is_some());
        &self.rare_data.as_ref().expect("rare data").constant_identifier_sets
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
        let rare_data = self.rare_data.as_ref()?;
        HandlerInfoBase::handler_for_index::<UnlinkedHandlerInfo, _>(rare_data.exception_handlers.iter(), index, required_handler)
    }

    pub fn is_builtin_function(&self) -> bool {
        self.is_builtin_function
    }

    pub fn constructor_kind(&self) -> ConstructorKind {
        self.constructor_kind
    }

    pub fn super_binding(&self) -> SuperBinding {
        self.super_binding
    }

    pub fn script_mode(&self) -> JSParserScriptMode {
        self.script_mode
    }

    /// `instructions()`.
    pub fn instructions(&self) -> &JSInstructionStream {
        debug_assert!(self.instructions.is_some());
        self.instructions.as_ref().expect("UnlinkedCodeBlock sem instruções antes do finalize")
    }

    /// `instructionAt(BytecodeIndex)`.
    pub fn instruction_at(&self, index: BytecodeIndex) -> Ref {
        self.instructions().at(index.offset())
    }

    pub fn instructions_size(&self) -> usize {
        self.instructions().size_in_bytes()
    }

    pub fn num_callee_locals(&self) -> u32 {
        self.num_callee_locals
    }

    pub fn num_vars(&self) -> u32 {
        self.num_vars
    }

    // Jump Tables

    pub fn number_of_unlinked_switch_jump_tables(&self) -> usize {
        self.rare_data.as_ref().map_or(0, |rare_data| rare_data.unlinked_switch_jump_tables.len())
    }

    pub fn unlinked_switch_jump_table(&self, table_index: usize) -> &UnlinkedSimpleJumpTable {
        debug_assert!(self.rare_data.is_some());
        &self.rare_data.as_ref().expect("rare data").unlinked_switch_jump_tables[table_index]
    }

    pub fn number_of_unlinked_string_switch_jump_tables(&self) -> usize {
        self.rare_data.as_ref().map_or(0, |rare_data| rare_data.unlinked_string_switch_jump_tables.len())
    }

    pub fn unlinked_string_switch_jump_table(&self, table_index: usize) -> &UnlinkedStringJumpTable {
        debug_assert!(self.rare_data.is_some());
        &self.rare_data.as_ref().expect("rare data").unlinked_string_switch_jump_tables[table_index]
    }

    pub fn function_decl(&self, index: usize) -> &UnlinkedFunctionExecutableRef {
        &self.function_decls[index]
    }

    pub fn number_of_function_decls(&self) -> usize {
        self.function_decls.len()
    }

    pub fn function_decls(&self) -> &[UnlinkedFunctionExecutableRef] {
        &self.function_decls
    }

    pub fn function_expr(&self, index: usize) -> &UnlinkedFunctionExecutableRef {
        &self.function_exprs[index]
    }

    pub fn number_of_function_exprs(&self) -> usize {
        self.function_exprs.len()
    }

    pub fn function_exprs(&self) -> &[UnlinkedFunctionExecutableRef] {
        &self.function_exprs
    }

    // Exception handling support
    pub fn number_of_exception_handlers(&self) -> usize {
        self.rare_data.as_ref().map_or(0, |rare_data| rare_data.exception_handlers.len())
    }

    pub fn exception_handler(&mut self, index: usize) -> &mut UnlinkedHandlerInfo {
        debug_assert!(self.rare_data.is_some());
        &mut self.rare_data.as_mut().expect("rare data").exception_handlers[index]
    }

    pub fn code_type(&self) -> CodeType {
        self.code_type
    }

    pub fn this_register(&self) -> VirtualRegister {
        self.this_register
    }

    pub fn scope_register(&self) -> VirtualRegister {
        self.scope_register
    }

    pub fn has_rare_data(&self) -> bool {
        self.rare_data.is_some()
    }

    /// `expressionInfoForBytecodeIndex`.
    pub fn expression_info_for_bytecode_index(&self, bytecode_index: BytecodeIndex) -> ExpressionInfoEntry {
        self.expression_info().entry_for_inst_pc(bytecode_index.offset())
    }

    /// `lineColumnForBytecodeIndex`.
    pub fn line_column_for_bytecode_index(&mut self, bytecode_index: BytecodeIndex) -> LineColumn {
        self.expression_info
            .as_mut()
            .expect("UnlinkedCodeBlock sem ExpressionInfo antes do finalize")
            .line_column_for_inst_pc(bytecode_index.offset())
    }

    /// O `ExpressionInfo` que o `finalize` instalou (`*m_expressionInfo`).
    pub fn expression_info(&self) -> &ExpressionInfo {
        self.expression_info.as_ref().expect("UnlinkedCodeBlock sem ExpressionInfo antes do finalize")
    }

    /// `typeProfilerExpressionInfoForBytecodeOffset`: `None` onde o C++ devolve `false` com
    /// `startDivot` e `endDivot` em `UINT_MAX`.
    pub fn type_profiler_expression_info_for_bytecode_offset(&self, bytecode_offset: u32) -> Option<TypeProfilerExpressionRange> {
        let rare_data = self.rare_data.as_ref()?;
        rare_data.type_profiler_info_map.get(&bytecode_offset).copied()
    }

    pub fn op_profile_control_flow_bytecode_offsets(&self) -> &[u32] {
        debug_assert!(self.rare_data.is_some());
        &self.rare_data.as_ref().expect("rare data").op_profile_control_flow_bytecode_offsets
    }

    pub fn has_op_profile_control_flow_bytecode_offsets(&self) -> bool {
        self.rare_data.as_ref().is_some_and(|rare_data| !rare_data.op_profile_control_flow_bytecode_offsets.is_empty())
    }

    pub fn was_compiled_with_debugging_opcodes(&self) -> bool {
        self.code_generation_mode & CodeGenerationMode::Debugger as u8 != 0
    }

    pub fn was_compiled_with_type_profiler_opcodes(&self) -> bool {
        self.code_generation_mode & CodeGenerationMode::TypeProfiler as u8 != 0
    }

    pub fn was_compiled_with_control_flow_profiler_opcodes(&self) -> bool {
        self.code_generation_mode & CodeGenerationMode::ControlFlowProfiler as u8 != 0
    }

    /// `codeGenerationMode()`: os bits crus do `OptionSet`.
    pub fn code_generation_mode(&self) -> u8 {
        self.code_generation_mode
    }

    pub fn did_optimize(&self) -> TriState {
        self.metadata.did_optimize()
    }

    pub fn set_did_optimize(&mut self, did_optimize: TriState) {
        self.metadata.set_did_optimize(did_optimize);
    }

    pub fn age(&self) -> u32 {
        self.age
    }

    pub fn reset_age(&mut self) {
        self.age = 0;
    }

    pub fn needs_class_field_initializer(&self) -> NeedsClassFieldInitializer {
        match &self.rare_data {
            Some(rare_data) => rare_data.needs_class_field_initializer,
            None => NeedsClassFieldInitializer::No,
        }
    }

    pub fn private_brand_requirement(&self) -> PrivateBrandRequirement {
        match &self.rare_data {
            Some(rare_data) => rare_data.private_brand_requirement,
            None => PrivateBrandRequirement::None,
        }
    }

    pub fn metadata(&mut self) -> &mut UnlinkedMetadataTable {
        &mut self.metadata
    }

    pub fn loop_hints_are_eligible_for_fuzzing_early_return(&self) -> bool {
        // Some builtins are required to always complete the loops they run.
        !self.is_builtin_function()
    }

    /// `allocateSharedProfiles`: calcula quantos perfis de cada tipo o código precisa.
    pub fn allocate_shared_profiles(&mut self, num_binary_arith_profiles: u32, num_unary_arith_profiles: u32) {
        assert!(!self.metadata.is_finalized());

        {
            let mut number_of_value_profiles = self.num_parameters();
            if self.metadata.has_metadata() {
                number_of_value_profiles += self.metadata.num_value_profiles();
            }

            self.number_of_value_profiles = number_of_value_profiles;
        }

        if self.metadata.has_metadata() {
            let mut number_of_array_profiles = 0;

            for opcode_id in OPCODES_WITH_SIMPLE_ARRAY_PROFILE {
                number_of_array_profiles += self.metadata.num_entries(opcode_id);
            }
            number_of_array_profiles += self.metadata.num_entries(OpcodeID::op_iterator_next);
            self.number_of_array_profiles = number_of_array_profiles;
        }

        self.number_of_binary_arith_profiles = num_binary_arith_profiles;
        self.number_of_unary_arith_profiles = num_unary_arith_profiles;
    }

    pub fn number_of_value_profiles(&self) -> u32 {
        self.number_of_value_profiles
    }

    pub fn number_of_array_profiles(&self) -> u32 {
        self.number_of_array_profiles
    }

    pub fn number_of_binary_arith_profiles(&self) -> u32 {
        self.number_of_binary_arith_profiles
    }

    pub fn number_of_unary_arith_profiles(&self) -> u32 {
        self.number_of_unary_arith_profiles
    }

    /// `outOfLineJumpOffset(JSInstructionStream::Offset)`.
    pub fn out_of_line_jump_offset(&self, bytecode_offset: u32) -> i32 {
        debug_assert!(self.out_of_line_jump_targets.contains_key(&bytecode_offset));
        self.out_of_line_jump_targets[&bytecode_offset]
    }
}

// ---------------------------------------------------------------------------------------------
// Subtipos: `UnlinkedGlobalCodeBlock` (UnlinkedGlobalCodeBlock.h), `UnlinkedProgramCodeBlock`,
// `UnlinkedModuleProgramCodeBlock`, `UnlinkedEvalCodeBlock` e `UnlinkedFunctionCodeBlock`
// (um `.h`/`.cpp` cada).
//
// Design da herança: no C++ o `BytecodeGenerator` recebe `UnlinkedCodeBlock*` apontando para o
// subtipo, e o `UnlinkedCodeBlockGenerator` guarda esse mesmo objeto. Aqui a base compartilhada é
// `Rc<RefCell<UnlinkedCodeBlock>>` (o campo `base` de `UnlinkedGlobalCodeBlock` e de
// `UnlinkedFunctionCodeBlock`), e `base_ref()` devolve um clone do `Rc` para o gerador. Como a base
// vive atrás de um `RefCell`, ela não tem `Deref`; os métodos dela se chamam por
// `subtipo.base_ref().borrow()` / `.borrow_mut()`. Entre os subtipos, `Program`, `ModuleProgram` e
// `Eval` herdam de `UnlinkedGlobalCodeBlock` por valor com o `inherit!` (`Deref`/`DerefMut` para o
// Global, que dá `base_ref()` e os acessores do Global). Os `create(VM&, ...)` perdem o `VM` (sem
// heap de células), e `destroy`,
// `subspaceFor`, `createStructure`, `DECLARE_INFO` e os construtores com `Decoder&` (cache de
// bytecode) não existem. `PackedRefPtr<StringImpl>` das diretivas é o `String` da WTF.
// ---------------------------------------------------------------------------------------------

use std::ops::{Deref, DerefMut};
use std::cell::RefCell;

use crate::parser::nodes::inherit;
use crate::parser::parser_modes::{
    CodeFeatures, LexicallyScopedFeatures, AWAIT_FEATURE, NO_EVAL_CACHE_FEATURE, NO_FEATURES,
    NO_LEXICALLY_SCOPED_FEATURES,
};
use crate::parser::variable_environment::VariableEnvironment;
use crate::wtf::text::wtf_string::String as WtfString;

/// `class UnlinkedGlobalCodeBlock`.
pub struct UnlinkedGlobalCodeBlock {
    base: Rc<RefCell<UnlinkedCodeBlock>>,
    features: CodeFeatures,
    lexically_scoped_features: LexicallyScopedFeatures,
    has_captured_variables: bool,
    line_count: u32,
    end_column: u32,
    source_url_directive: WtfString,
    source_mapping_url_directive: WtfString,
}

impl UnlinkedGlobalCodeBlock {
    /// `UnlinkedGlobalCodeBlock(VM&, Structure*, CodeType, const ExecutableInfo&, OptionSet<CodeGenerationMode>)`.
    fn new(code_type: CodeType, info: &ExecutableInfo, code_generation_mode: u8) -> UnlinkedGlobalCodeBlock {
        UnlinkedGlobalCodeBlock {
            base: UnlinkedCodeBlock::new_shared(code_type, info, code_generation_mode),
            features: NO_FEATURES,
            lexically_scoped_features: NO_LEXICALLY_SCOPED_FEATURES,
            has_captured_variables: false,
            line_count: 0,
            end_column: u32::MAX,
            source_url_directive: WtfString::default(),
            source_mapping_url_directive: WtfString::default(),
        }
    }

    /// A conversão implícita para `UnlinkedCodeBlock*` do C++: a base compartilhada com o gerador.
    pub fn base_ref(&self) -> Rc<RefCell<UnlinkedCodeBlock>> {
        Rc::clone(&self.base)
    }

    pub fn record_parse(
        &mut self,
        features: CodeFeatures,
        lexically_scoped_features: LexicallyScopedFeatures,
        has_captured_variables: bool,
        line_count: u32,
        end_column: u32,
    ) {
        self.features = features;
        self.lexically_scoped_features = lexically_scoped_features;
        self.has_captured_variables = has_captured_variables;
        self.line_count = line_count;
        // For the UnlinkedCodeBlock, startColumn is always 0.
        self.end_column = end_column;
    }

    pub fn source_url_directive(&self) -> &WtfString {
        &self.source_url_directive
    }

    pub fn source_mapping_url_directive(&self) -> &WtfString {
        &self.source_mapping_url_directive
    }

    pub fn set_source_url_directive(&mut self, source_url: WtfString) {
        self.source_url_directive = source_url;
    }

    pub fn set_source_mapping_url_directive(&mut self, source_mapping_url: WtfString) {
        self.source_mapping_url_directive = source_mapping_url;
    }

    pub fn code_features(&self) -> CodeFeatures {
        self.features
    }

    pub fn allow_direct_eval_cache(&self) -> bool {
        self.features & NO_EVAL_CACHE_FEATURE == 0
    }

    pub fn lexically_scoped_features(&self) -> LexicallyScopedFeatures {
        self.lexically_scoped_features
    }

    pub fn has_captured_variables(&self) -> bool {
        self.has_captured_variables
    }

    pub fn line_count(&self) -> u32 {
        self.line_count
    }

    pub fn start_column(&self) -> u32 {
        0
    }

    pub fn end_column(&self) -> u32 {
        self.end_column
    }
}

/// `class UnlinkedProgramCodeBlock`.
pub struct UnlinkedProgramCodeBlock {
    base: UnlinkedGlobalCodeBlock,
    var_declarations: VariableEnvironment,
    lexical_declarations: VariableEnvironment,
}

inherit!(UnlinkedProgramCodeBlock => UnlinkedGlobalCodeBlock);

impl UnlinkedProgramCodeBlock {
    /// `create(VM&, const ExecutableInfo&, OptionSet<CodeGenerationMode>)`.
    pub fn create(info: &ExecutableInfo, code_generation_mode: u8) -> Rc<RefCell<UnlinkedProgramCodeBlock>> {
        Rc::new(RefCell::new(UnlinkedProgramCodeBlock {
            base: UnlinkedGlobalCodeBlock::new(CodeType::GlobalCode, info, code_generation_mode),
            var_declarations: VariableEnvironment::default(),
            lexical_declarations: VariableEnvironment::default(),
        }))
    }

    pub fn set_variable_declarations(&mut self, environment: VariableEnvironment) {
        self.var_declarations = environment;
    }

    pub fn variable_declarations(&self) -> &VariableEnvironment {
        &self.var_declarations
    }

    pub fn set_lexical_declarations(&mut self, environment: VariableEnvironment) {
        self.lexical_declarations = environment;
    }

    pub fn lexical_declarations(&self) -> &VariableEnvironment {
        &self.lexical_declarations
    }
}

/// `class UnlinkedModuleProgramCodeBlock`.
pub struct UnlinkedModuleProgramCodeBlock {
    base: UnlinkedGlobalCodeBlock,
    var_declarations: VariableEnvironment,
    module_environment_symbol_table_constant_register_offset: i32,
}

inherit!(UnlinkedModuleProgramCodeBlock => UnlinkedGlobalCodeBlock);

impl UnlinkedModuleProgramCodeBlock {
    /// `create(VM&, const ExecutableInfo&, OptionSet<CodeGenerationMode>)`.
    pub fn create(info: &ExecutableInfo, code_generation_mode: u8) -> Rc<RefCell<UnlinkedModuleProgramCodeBlock>> {
        Rc::new(RefCell::new(UnlinkedModuleProgramCodeBlock {
            base: UnlinkedGlobalCodeBlock::new(CodeType::ModuleCode, info, code_generation_mode),
            var_declarations: VariableEnvironment::default(),
            module_environment_symbol_table_constant_register_offset: 0,
        }))
    }

    /// O deslocamento do registrador de constante que guarda a tabela de símbolos do ambiente do
    /// módulo (ver o comentário longo do `.h`).
    pub fn module_environment_symbol_table_constant_register_offset(&self) -> i32 {
        self.module_environment_symbol_table_constant_register_offset
    }

    pub fn set_module_environment_symbol_table_constant_register_offset(&mut self, offset: i32) {
        self.module_environment_symbol_table_constant_register_offset = offset;
    }

    pub fn is_async(&self) -> bool {
        self.code_features() & AWAIT_FEATURE != 0
    }

    pub fn set_variable_declarations(&mut self, environment: VariableEnvironment) {
        self.var_declarations = environment;
    }

    pub fn variable_declarations(&self) -> &VariableEnvironment {
        &self.var_declarations
    }
}

/// `class UnlinkedEvalCodeBlock`.
pub struct UnlinkedEvalCodeBlock {
    base: UnlinkedGlobalCodeBlock,
    variables: Vec<Identifier>,
    function_hoisting_candidates: Vec<Identifier>,
}

inherit!(UnlinkedEvalCodeBlock => UnlinkedGlobalCodeBlock);

impl UnlinkedEvalCodeBlock {
    /// `create(VM&, const ExecutableInfo&, OptionSet<CodeGenerationMode>)`.
    pub fn create(info: &ExecutableInfo, code_generation_mode: u8) -> Rc<RefCell<UnlinkedEvalCodeBlock>> {
        Rc::new(RefCell::new(UnlinkedEvalCodeBlock {
            base: UnlinkedGlobalCodeBlock::new(CodeType::EvalCode, info, code_generation_mode),
            variables: Vec::new(),
            function_hoisting_candidates: Vec::new(),
        }))
    }

    pub fn variable(&self, index: usize) -> &Identifier {
        &self.variables[index]
    }

    pub fn num_variables(&self) -> usize {
        self.variables.len()
    }

    pub fn variables(&self) -> &[Identifier] {
        &self.variables
    }

    pub fn adopt_variables(&mut self, variables: Vec<Identifier>) {
        debug_assert!(self.variables.is_empty());
        self.variables = variables;
    }

    pub fn function_hoisting_candidate(&self, index: usize) -> &Identifier {
        &self.function_hoisting_candidates[index]
    }

    pub fn num_function_hoisting_candidates(&self) -> usize {
        self.function_hoisting_candidates.len()
    }

    pub fn function_hoisting_candidates(&self) -> &[Identifier] {
        &self.function_hoisting_candidates
    }

    pub fn adopt_function_hoisting_candidates(&mut self, function_hoisting_candidates: Vec<Identifier>) {
        debug_assert!(self.function_hoisting_candidates.is_empty());
        self.function_hoisting_candidates = function_hoisting_candidates;
    }
}

/// `class UnlinkedFunctionCodeBlock`.
pub struct UnlinkedFunctionCodeBlock {
    base: Rc<RefCell<UnlinkedCodeBlock>>,
}

impl UnlinkedFunctionCodeBlock {
    /// `create(VM&, CodeType, const ExecutableInfo&, OptionSet<CodeGenerationMode>)`.
    pub fn create(
        code_type: CodeType,
        info: &ExecutableInfo,
        code_generation_mode: u8,
    ) -> Rc<RefCell<UnlinkedFunctionCodeBlock>> {
        Rc::new(RefCell::new(UnlinkedFunctionCodeBlock {
            base: UnlinkedCodeBlock::new_shared(code_type, info, code_generation_mode),
        }))
    }

    /// A conversão implícita para `UnlinkedCodeBlock*` do C++: a base compartilhada com o gerador.
    pub fn base_ref(&self) -> Rc<RefCell<UnlinkedCodeBlock>> {
        Rc::clone(&self.base)
    }
}
