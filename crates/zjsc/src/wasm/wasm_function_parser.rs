//! Tradução de `wasm/WasmFunctionParser.h`: a validação de instruções com a pilha de controle e a
//! pilha de tipos, sem o gerador de código.
//!
//! O C++ é um template sobre `Context` (o gerador de código: IPInt, BBQ, OMG, ou o
//! `ConstExprGenerator`), que recebe uma chamada `add*` por instrução. Aqui `Context` é um trait
//! com as mesmas garantias de ordem: o parser lê os imediatos, confere a pilha e os tipos, e só
//! então avisa o contexto, que pode recusar (é assim que uma expressão constante rejeita
//! `i32.div_s`). Diferenças deliberadas:
//!
//! - `ExpressionType` é vazio nos dois contextos que existem (validador e expressão constante), então
//!   a pilha de expressões é uma pilha de `Type`.
//! - As centenas de métodos `add*` viram um único `Context::add` que recebe um `Hook` (o opcode, ou o
//!   índice no caso de `get_global` e `ref.func`). O validador aceita tudo; o gerador de expressão
//!   constante tem a lista branca do C++.
//! - `FOR_EACH_WASM_*_OP` viram as tabelas de `wasm_ops.rs` (`types()`).
//! - `addArguments`, `addLocal`, `didFinishParsingLocals`, `didPopValueFromStack`, `dump` e os
//!   contadores de perfil de chamada só servem a geradores de código e não existem aqui.
//! - `shouldFuseBranchCompare` é falso nos dois contextos, então a fusão de comparação com `br_if` e
//!   `if` não foi portada.
//! - `m_usesLegacyExceptions` e `m_usesModernExceptions` do `ModuleInformation` ficam no parser
//!   (`uses_legacy_exceptions`, `uses_modern_exceptions`), porque o módulo é emprestado como `&`.
//! - SIMD (`ExtSIMD`) está em `wasm_function_parser_simd.rs`, com a tabela de `wasm_simd_opcodes.rs`.

use crate::runtime::options::Options;
use crate::wasm::wasm_format::{
    StorageType, TableElementType, Type, TypeIndex, TypeKind, is_defaultable_type, is_ref_type, is_valid_heap_type_kind,
    is_valid_type_kind, is_value_type,
};
use crate::wasm::wasm_limits::MAX_FUNCTION_LOCALS;
use crate::wasm::wasm_module_information::{ModuleInformation, RttKind, StructuralType, TypeDefinition, rtt_to_string};
use crate::wasm::wasm_ops::{
    BinaryOpType, Ext1OpType, ExtAtomicOpType, ExtGCOpType, LoadOpType, OpType, StoreOpType, UnaryOpType, is_valid_op_type,
    memory_log2_alignment,
};
use crate::wasm::wasm_parser::ParserBase;
use crate::wasm::wasm_simd_opcodes::ExtSimdOpType;
use crate::wtf::bit_vector::BitVector;

/// `PartialResult`: o texto do erro, já com o prefixo que a API JS mostra.
pub type PartialResult = Result<(), String>;

/// `parseXxx` falhou: `WASM_PARSER_FAIL_IF`.
macro_rules! pfail_if {
    ($self:expr, $cond:expr, $($arg:tt)*) => {
        if $cond {
            return $self.pfail(format!($($arg)*));
        }
    };
}

/// `WASM_VALIDATOR_FAIL_IF`.
macro_rules! vfail_if {
    ($self:expr, $cond:expr, $($arg:tt)*) => {
        if $cond {
            return $self.vfail(format!($($arg)*));
        }
    };
}

/// Lê um imediato (`Option`), falhando como `WASM_PARSER_FAIL_IF(!parseXxx(x), ...)`.
macro_rules! parse_or_fail {
    ($self:expr, $e:expr, $($arg:tt)*) => {
        match $e {
            Some(value) => value,
            None => return $self.pfail(format!($($arg)*)),
        }
    };
}

/// Desempilha, falhando como `WASM_TRY_POP_EXPRESSION_STACK_INTO`.
macro_rules! pop_or_fail {
    ($self:expr, $what:expr) => {
        $self.pop($what)?
    };
}

pub(crate) use {parse_or_fail, pfail_if, pop_or_fail, vfail_if};

/// `BlockType`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BlockType {
    If,
    Else,
    Block,
    Loop,
    TopLevel,
    Try,
    TryTable,
    Catch,
}

/// `CatchKind`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CatchKind {
    Catch = 0,
    CatchRef = 1,
    CatchAll = 2,
    CatchAllRef = 3,
}

/// `BlockSignature`: um tipo de resultado (ou `void`) ou uma assinatura de função da seção de tipos.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum BlockSignature {
    Type(Type),
    Function { position: u32, arguments: Vec<Type>, returns: Vec<Type> },
}

impl BlockSignature {
    /// `BlockSignature(const RTT&)`: a posição precisa ser de um tipo função.
    pub fn from_position(info: &ModuleInformation, position: u32) -> Option<BlockSignature> {
        match &info.rtt(position as usize).structural {
            StructuralType::Function { arguments, returns } => {
                Some(BlockSignature::Function { position, arguments: arguments.clone(), returns: returns.clone() })
            }
            _ => None,
        }
    }

    pub fn argument_count(&self) -> usize {
        match self {
            BlockSignature::Type(_) => 0,
            BlockSignature::Function { arguments, .. } => arguments.len(),
        }
    }

    pub fn return_count(&self) -> usize {
        match self {
            BlockSignature::Type(ty) => usize::from(ty.kind != TypeKind::Void),
            BlockSignature::Function { returns, .. } => returns.len(),
        }
    }

    pub fn argument_type(&self, index: usize) -> Type {
        match self {
            BlockSignature::Type(_) => unreachable!("BlockSignature simples não tem argumentos"),
            BlockSignature::Function { arguments, .. } => arguments[index],
        }
    }

    pub fn return_type(&self, index: usize) -> Type {
        match self {
            BlockSignature::Type(ty) => *ty,
            BlockSignature::Function { returns, .. } => returns[index],
        }
    }

    pub fn has_returned_v128(&self) -> bool {
        match self {
            BlockSignature::Type(ty) => ty.is_v128(),
            BlockSignature::Function { returns, .. } => returns.iter().any(|ty| ty.is_v128()),
        }
    }

    pub fn holds_rtt(&self) -> bool {
        matches!(self, BlockSignature::Function { .. })
    }

    /// `BlockSignature::dump`: `(I32, I64) -> [I32]`.
    pub fn dump(&self) -> String {
        let arguments: Vec<&str> = (0..self.argument_count()).map(|i| self.argument_type(i).kind.name()).collect();
        let returns: Vec<&str> = (0..self.return_count()).map(|i| self.return_type(i).kind.name()).collect();
        format!("({}) -> [{}]", arguments.join(", "), returns.join(", "))
    }
}

/// Os métodos de `ControlType` que o parser usa (`isIf`, `isTry`, `signature`,
/// `branchTargetArity`...).
pub trait ControlData {
    fn block_type(&self) -> BlockType;
    /// O `addElse`/`addCatch` do contexto troca o tipo do bloco.
    fn set_block_type(&mut self, block_type: BlockType);
    fn signature(&self) -> &BlockSignature;
    fn branch_target_arity(&self) -> usize;
    fn branch_target_type(&self, index: usize) -> Type;

    fn is_if(&self) -> bool {
        self.block_type() == BlockType::If
    }
    fn is_else(&self) -> bool {
        self.block_type() == BlockType::Else
    }
    fn is_try(&self) -> bool {
        self.block_type() == BlockType::Try
    }
    fn is_catch(&self) -> bool {
        self.block_type() == BlockType::Catch
    }
    fn is_any_catch(&self) -> bool {
        self.block_type() == BlockType::Catch
    }
    fn is_top_level(&self) -> bool {
        self.block_type() == BlockType::TopLevel
    }
    fn is_loop(&self) -> bool {
        self.block_type() == BlockType::Loop
    }
    fn is_block(&self) -> bool {
        self.block_type() == BlockType::Block
    }
}

/// O `ControlData` dos geradores de verdade: o desvio para um `loop` leva os argumentos, para os
/// demais leva os resultados.
#[derive(Clone, Debug)]
pub struct StandardControl {
    block_type: BlockType,
    signature: BlockSignature,
}

impl StandardControl {
    pub fn new(block_type: BlockType, signature: BlockSignature) -> StandardControl {
        StandardControl { block_type, signature }
    }
}

impl ControlData for StandardControl {
    fn block_type(&self) -> BlockType {
        self.block_type
    }
    fn set_block_type(&mut self, block_type: BlockType) {
        self.block_type = block_type;
    }
    fn signature(&self) -> &BlockSignature {
        &self.signature
    }
    fn branch_target_arity(&self) -> usize {
        if self.block_type == BlockType::Loop { self.signature.argument_count() } else { self.signature.return_count() }
    }
    fn branch_target_type(&self, index: usize) -> Type {
        if self.block_type == BlockType::Loop { self.signature.argument_type(index) } else { self.signature.return_type(index) }
    }
}

/// O que o parser mostra ao contexto em cada chamada.
pub struct Env<'a> {
    pub info: &'a ModuleInformation,
    /// `m_parser->offset()`.
    pub offset: usize,
    /// O tipo do operando que `drop` e `select` consumiram (a largura, para quem executa).
    pub operand: Option<Type>,
}

/// A chamada `add*` do C++ que o parser faz depois de validar a instrução.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Hook {
    /// Um opcode de um byte (aritmética, memória, tabela, referência, chamada, controle, locals).
    Op(OpType),
    /// `get_global` com o índice (o contexto confere de novo o que o C++ conferia).
    GetGlobal(u32),
    /// `ref.func` com o índice no espaço de funções.
    RefFunc(u32),
    Ext1(Ext1OpType),
    ExtGC(ExtGCOpType),
    Atomic(ExtAtomicOpType),
    /// O `addSIMD*` da instrução `0xFD`, pelo opcode estendido (a operação, a lane e o modo de sinal
    /// saem de `ExtSimdOpType::info`).
    Simd(ExtSimdOpType),
    /// `addElseToUnreachable`, `addCatchToUnreachable`, `addCatchAllToUnreachable`,
    /// `addDelegateToUnreachable` e `addEndToUnreachable`, pelo opcode que os chama.
    ToUnreachable(OpType),
    /// `endBlock`.
    EndBlock,
}

/// O `Context` do C++.
pub trait Context {
    type Control: ControlData;

    /// `Context::validateFunctionBodySize`.
    const VALIDATE_FUNCTION_BODY_SIZE: bool;
    /// `!std::is_same<Context, ConstExprGenerator>()`: `ref.func` exige função declarada.
    const REF_FUNC_NEEDS_DECLARATION: bool;

    /// `addTopLevel`, `addBlock`, `addLoop`, `addIf`, `addTry`, `addTryTable`: o dado de controle de um
    /// bloco novo (o parser já chamou `add` com o opcode).
    fn make_control(&mut self, block_type: BlockType, signature: BlockSignature) -> Self::Control;
    fn add(&mut self, env: &Env<'_>, hook: Hook) -> PartialResult;
    fn notify_function_uses_simd(&mut self) {}
    fn did_parse_opcode(&mut self, _opcode: OpType) {}
    fn end_top_level(&mut self, _env: &Env<'_>) -> PartialResult {
        Ok(())
    }
}

/// `ControlEntry`.
struct ControlEntry<T> {
    else_block_stack: Vec<Type>,
    enclosed_stack_begin: usize,
    local_init_stack_height: usize,
    control_data: T,
}

/// `FunctionParser<Context>`.
pub struct FunctionParser<'s, 'i, C: Context> {
    pub(super) parser: ParserBase<'s>,
    pub(super) context: &'i mut C,
    pub(super) info: &'i ModuleInformation,
    pub(super) signature: BlockSignature,
    pub(super) expression_stack: Vec<Type>,
    pub(super) current_stack_begin: usize,
    control_stack: Vec<ControlEntry<C::Control>>,
    pub(super) locals: Vec<Type>,
    local_init_stack: Vec<u32>,
    local_init_flags: BitVector,
    pub(super) current_opcode: OpType,
    pub(super) current_ext_op: u32,
    current_opcode_starting_offset: usize,
    pub(super) unreachable_blocks: u32,
    loop_index: u32,
    pub uses_legacy_exceptions: bool,
    pub uses_modern_exceptions: bool,
    /// O operando que o `drop`/`select` em curso consumiu, entregue ao contexto em `Env::operand`.
    hook_operand: Option<Type>,
}

pub(super) fn simple_type(kind: TypeKind) -> Type {
    Type::new(kind, TypeIndex::Invalid)
}

/// `StorageType::unpacked`.
pub(super) fn unpacked(storage: StorageType) -> Type {
    match storage {
        StorageType::Type(ty) => ty,
        StorageType::Packed(_) => simple_type(TypeKind::I32),
    }
}

/// `heapTypeKindAsString`.
fn heap_type_kind_as_string(kind: TypeKind) -> &'static str {
    match kind {
        TypeKind::Funcref => "func",
        TypeKind::Externref => "extern",
        TypeKind::I31ref => "i31",
        TypeKind::Arrayref => "array",
        TypeKind::Structref => "struct",
        TypeKind::Eqref => "eq",
        TypeKind::Anyref => "any",
        TypeKind::Noneref => "none",
        TypeKind::Nofuncref => "nofunc",
        TypeKind::Noexternref => "noextern",
        TypeKind::Exnref => "exnref",
        TypeKind::Noexnref => "noexnref",
        other => unreachable!("não é um tipo heap: {}", other.name()),
    }
}

/// Os argumentos e retornos de um tipo função (os tipos de exceção também são funções).
pub(super) fn function_signature(definition: &TypeDefinition) -> (&[Type], &[Type]) {
    match &definition.structural {
        StructuralType::Function { arguments, returns } => (arguments, returns),
        _ => unreachable!("esperava um tipo função"),
    }
}

impl<'s, 'i, C: Context> FunctionParser<'s, 'i, C> {
    pub fn new(context: &'i mut C, function: &'s [u8], signature: BlockSignature, info: &'i ModuleInformation) -> Self {
        let use_wasm_simd = Options::with(|options| options.use_wasm_simd);
        FunctionParser {
            parser: ParserBase::new(function, use_wasm_simd),
            context,
            info,
            signature,
            expression_stack: Vec::new(),
            current_stack_begin: 0,
            control_stack: Vec::new(),
            locals: Vec::new(),
            local_init_stack: Vec::new(),
            local_init_flags: BitVector::new(),
            current_opcode: OpType::Unreachable,
            current_ext_op: 0,
            current_opcode_starting_offset: 0,
            unreachable_blocks: 0,
            loop_index: 0,
            uses_legacy_exceptions: false,
            uses_modern_exceptions: false,
            hook_operand: None,
        }
    }

    pub fn offset(&self) -> usize {
        self.parser.offset()
    }

    pub fn current_opcode(&self) -> OpType {
        self.current_opcode
    }

    pub fn current_opcode_starting_offset(&self) -> usize {
        self.current_opcode_starting_offset
    }

    pub fn type_of_local(&self, index: usize) -> Type {
        self.locals[index]
    }

    pub fn unreachable_blocks(&self) -> u32 {
        self.unreachable_blocks
    }

    // --- Falhas -------------------------------------------------------------------------------

    /// `ParserBase::fail`.
    pub(super) fn pfail<T>(&self, message: String) -> Result<T, String> {
        Err(self.parser.fail(&message))
    }

    /// `validationFail`.
    pub(super) fn vfail<T>(&self, message: String) -> Result<T, String> {
        Err(format!("WebAssembly.Module doesn't validate: {}", message))
    }

    /// `typeToStringModuleRelative`.
    pub(super) fn ty(&self, ty: Type) -> String {
        if !is_ref_type(ty) {
            return self.info.type_to_string(ty);
        }
        let mut out = String::from("(ref ");
        if ty.is_nullable() {
            out.push_str("null ");
        }
        match ty.index {
            TypeIndex::Abstract(kind) => out.push_str(heap_type_kind_as_string(kind)),
            index => {
                let id = match index {
                    TypeIndex::Concrete(id) => Some(id),
                    _ => None,
                };
                let kind = id.map(|id| self.info.canonical_rtt(id).structural.kind());
                out.push_str(match kind {
                    Some(RttKind::Function) => "<func:",
                    Some(RttKind::Array) => "<array:",
                    _ => "<struct:",
                });
                for position in 0..self.info.type_count() {
                    if Some(self.info.canonical_type_id(position)) == id {
                        out.push_str(&position.to_string());
                        break;
                    }
                }
                out.push('>');
            }
        }
        out.push(')');
        out
    }

    // --- Contexto -----------------------------------------------------------------------------

    pub(super) fn hook(&mut self, hook: Hook) -> PartialResult {
        let env = Env { info: self.info, offset: self.parser.offset(), operand: self.hook_operand.take() };
        self.context.add(&env, hook)
    }

    // --- Pilha de expressões ------------------------------------------------------------------

    /// `WASM_TRY_POP_EXPRESSION_STACK_INTO`.
    pub(super) fn pop(&mut self, what: &str) -> Result<Type, String> {
        if self.expression_stack.len() == self.current_stack_begin {
            return self.pfail(format!("can't pop empty stack in {}", what));
        }
        Ok(self.expression_stack.pop().unwrap())
    }

    pub(super) fn slice_size(&self) -> usize {
        self.expression_stack.len() - self.current_stack_begin
    }

    fn parent_entry_begin(&self) -> usize {
        self.control_stack.last().map_or(0, |entry| entry.enclosed_stack_begin)
    }

    fn push_local_initialized(&mut self, index: u32) {
        if !is_defaultable_type(self.locals[index as usize]) && !self.local_is_initialized(index) {
            self.local_init_stack.push(index);
            self.local_init_flags.quick_set(index as usize);
        }
    }

    fn local_init_stack_height(&self) -> usize {
        self.local_init_stack.len()
    }

    fn reset_local_init_stack_to_height(&mut self, height: usize) {
        while self.local_init_stack.len() > height {
            let index = self.local_init_stack.pop().unwrap();
            self.local_init_flags.quick_clear(index as usize);
        }
    }

    fn local_is_initialized(&self, index: u32) -> bool {
        self.local_init_flags.quick_get(index as usize)
    }

    // --- parse --------------------------------------------------------------------------------

    /// `parse`.
    pub fn parse(&mut self) -> PartialResult {
        let (arguments, returns) = match &self.signature {
            BlockSignature::Function { arguments, returns, .. } => (arguments.clone(), returns.clone()),
            _ => return self.pfail("type signature was not a function signature".to_string()),
        };
        if arguments.iter().any(|ty| ty.is_v128()) || returns.iter().any(|ty| ty.is_v128()) {
            self.context.notify_function_uses_simd();
        }

        let local_groups_count = parse_or_fail!(self, self.parser.parse_var_uint32(), "can't get local groups count");
        self.locals.extend(arguments.iter().copied());

        let mut total_number_of_locals = arguments.len() as u64;
        for group in 0..local_groups_count {
            let number_of_locals = parse_or_fail!(
                self,
                self.parser.parse_var_uint32(),
                "can't get Function's number of locals in group {}",
                group
            );
            total_number_of_locals += u64::from(number_of_locals);
            pfail_if!(
                self,
                total_number_of_locals > MAX_FUNCTION_LOCALS as u64,
                "Function's number of locals is too big {} maximum {}",
                total_number_of_locals,
                MAX_FUNCTION_LOCALS
            );
            let type_of_local = parse_or_fail!(
                self,
                self.parser.parse_value_type(self.info),
                "can't get Function local's type in group {}",
                group
            );
            if type_of_local.is_v128() {
                self.context.notify_function_uses_simd();
            }
            for _ in 0..number_of_locals {
                self.locals.push(type_of_local);
            }
        }

        self.local_init_flags.ensure_size(total_number_of_locals as usize);
        // Param locals are always considered initialized, so we need to pre-set them.
        for (index, argument) in arguments.iter().enumerate() {
            if !is_defaultable_type(*argument) {
                self.local_init_flags.quick_set(index);
            }
        }

        self.parse_body()
    }

    /// `parseConstantExpression`.
    pub fn parse_constant_expression(&mut self) -> PartialResult {
        if self.signature.has_returned_v128() {
            self.context.notify_function_uses_simd();
        }
        debug_assert_eq!(self.signature.argument_count(), 0);
        self.parse_body()
    }

    fn parse_body(&mut self) -> PartialResult {
        let top_level = self.context.make_control(BlockType::TopLevel, self.signature.clone());
        self.control_stack.push(ControlEntry {
            else_block_stack: Vec::new(),
            enclosed_stack_begin: 0,
            local_init_stack_height: 0,
            control_data: top_level,
        });
        while !self.control_stack.is_empty() {
            self.current_opcode_starting_offset = self.parser.offset();
            let op = parse_or_fail!(self, self.parser.parse_uint8(), "can't decode opcode");
            pfail_if!(self, !is_valid_op_type(i64::from(op)), "invalid opcode {}", op);
            self.current_opcode = OpType::from_value(op).expect("opcode válido");

            if self.unreachable_blocks != 0 {
                self.parse_unreachable_expression()?;
            } else {
                self.parse_expression()?;
            }
            let opcode = self.current_opcode;
            self.context.did_parse_opcode(opcode);
        }
        let env = Env { info: self.info, offset: self.parser.offset(), operand: None };
        self.context.end_top_level(&env)?;
        if C::VALIDATE_FUNCTION_BODY_SIZE {
            pfail_if!(
                self,
                self.parser.offset() != self.parser.source().len(),
                "function body size doesn't match the expected size"
            );
        }
        Ok(())
    }

    // --- Imediatos ----------------------------------------------------------------------------

    pub(super) fn parse_table_index(&mut self) -> Result<u32, String> {
        let table_index = parse_or_fail!(self, self.parser.parse_var_uint32(), "can't parse table index");
        vfail_if!(
            self,
            table_index as usize >= self.info.table_count(),
            "table index {} is invalid, limit is {}",
            table_index,
            self.info.table_count()
        );
        Ok(table_index)
    }

    fn parse_index_for_local(&mut self) -> Result<u32, String> {
        let index = parse_or_fail!(self, self.parser.parse_var_uint32(), "can't get index for local");
        vfail_if!(
            self,
            index as usize >= self.locals.len(),
            "attempt to use unknown local {}, the number of locals is {}",
            index,
            self.locals.len()
        );
        Ok(index)
    }

    fn parse_index_for_global(&mut self) -> Result<u32, String> {
        let index = parse_or_fail!(self, self.parser.parse_var_uint32(), "can't get global's index");
        vfail_if!(
            self,
            index as usize >= self.info.globals.len(),
            "{} of unknown global, limit is {}",
            index,
            self.info.globals.len()
        );
        Ok(index)
    }

    pub(super) fn parse_function_index(&mut self) -> Result<u32, String> {
        let function_index = parse_or_fail!(self, self.parser.parse_var_uint32(), "can't parse function index");
        pfail_if!(
            self,
            function_index as usize >= self.info.function_index_space_size(),
            "function index {} exceeds function index space {}",
            function_index,
            self.info.function_index_space_size()
        );
        Ok(function_index)
    }

    pub(super) fn parse_exception_index(&mut self) -> Result<u32, String> {
        let exception_index = parse_or_fail!(self, self.parser.parse_var_uint32(), "can't parse exception index");
        vfail_if!(
            self,
            exception_index as usize >= self.info.exception_index_space_size(),
            "exception index {} is invalid, limit is {}",
            exception_index,
            self.info.exception_index_space_size()
        );
        Ok(exception_index)
    }

    pub(super) fn parse_branch_target(&mut self, unreachable_blocks: u32) -> Result<u32, String> {
        let target = parse_or_fail!(self, self.parser.parse_var_uint32(), "can't get br / br_if's target");
        let mut control_stack_size = self.control_stack.len() as u64;
        // Take into account the unreachable blocks in the control stack that were not added because they were unrechable
        if unreachable_blocks != 0 {
            control_stack_size += u64::from(unreachable_blocks - 1);
        }
        pfail_if!(
            self,
            u64::from(target) >= control_stack_size,
            "br / br_if's target {} exceeds control stack size {}",
            target,
            control_stack_size
        );
        Ok(target)
    }

    fn parse_delegate_target(&mut self, unreachable_blocks: u32) -> Result<u32, String> {
        // Right now, control stack includes try-delegate block, and delegate needs to specify outer scope.
        let target = parse_or_fail!(self, self.parser.parse_var_uint32(), "can't get delegate target");
        let mut control_stack_size = self.control_stack.len() as i64;
        if unreachable_blocks != 0 {
            control_stack_size += i64::from(unreachable_blocks - 1); // The first block is in the control stack already.
        }
        control_stack_size -= 1; // delegate target does not include the current block.
        pfail_if!(self, control_stack_size < 0, "invalid control stack size");
        pfail_if!(
            self,
            i64::from(target) >= control_stack_size,
            "delegate target {} exceeds control stack size {}",
            target,
            control_stack_size
        );
        Ok(target)
    }

    pub(super) fn parse_element_index(&mut self) -> Result<u32, String> {
        let element_index = parse_or_fail!(self, self.parser.parse_var_uint32(), "can't parse element index");
        vfail_if!(
            self,
            element_index as usize >= self.info.element_count(),
            "element index {} is invalid, limit is {}",
            element_index,
            self.info.element_count()
        );
        Ok(element_index)
    }

    pub(super) fn parse_data_segment_index(&mut self) -> Result<u32, String> {
        let index = parse_or_fail!(self, self.parser.parse_var_uint32(), "can't parse data segment index");
        vfail_if!(
            self,
            index >= self.info.data_segments_count(),
            "data segment index {} is invalid, limit is {}",
            index,
            self.info.data_segments_count()
        );
        Ok(index)
    }

    pub(super) fn parse_memory_index(&mut self) -> Result<u8, String> {
        let memory_index = parse_or_fail!(self, self.parser.parse_var_uint32(), "can't get memory index");
        vfail_if!(self, memory_index as usize >= self.info.memory_count(), "memory index {} is out of range", memory_index);
        Ok(memory_index as u8)
    }

    /// `parseMemoryIndexAndFixupAlignment`: devolve o alinhamento sem os bits de memória e o índice.
    pub(super) fn parse_memory_index_and_fixup_alignment(&mut self, alignment: u32) -> Result<(u32, u8), String> {
        // memarg ::= a:u32 o:u64 (if a < 2^6) | a:u32 x:memidx o:u64 (if 2^6 <= a < 2^7)
        pfail_if!(self, alignment >= (1 << 7), "byte alignment immediate {} is too large", alignment);
        let has_memory_index = alignment & (1 << 6) != 0;
        let alignment = alignment & 0b111111;
        let memory_index = if has_memory_index { self.parse_memory_index()? } else { 0 };
        Ok((alignment, memory_index))
    }

    pub(super) fn parse_memory_offset(&mut self, memory_index: u8) -> Result<u64, String> {
        // The offset is encoded as a u64 whatever the memory's address type is; only its value is
        // restricted to the range the address type can hold.
        let result = parse_or_fail!(self, self.parser.parse_var_uint64(), "can't get memory offset");
        vfail_if!(
            self,
            !self.info.memories[memory_index as usize].is_memory64() && result > u64::from(u32::MAX),
            "memory offset {} is out of range for an i32 memory",
            result
        );
        Ok(result)
    }

    fn parse_table_init_immediates(&mut self) -> Result<(u32, u32), String> {
        let element_index = self.parse_element_index()?;
        let table_index = self.parse_table_index()?;
        Ok((element_index, table_index))
    }

    /// Devolve (destino, origem).
    fn parse_table_copy_immediates(&mut self) -> Result<(u32, u32), String> {
        let dst = parse_or_fail!(self, self.parser.parse_var_uint32(), "can't parse destination table index");
        vfail_if!(
            self,
            dst as usize >= self.info.table_count(),
            "table index {} is invalid, limit is {}",
            dst,
            self.info.table_count()
        );
        let src = parse_or_fail!(self, self.parser.parse_var_uint32(), "can't parse source table index");
        vfail_if!(
            self,
            src as usize >= self.info.table_count(),
            "table index {} is invalid, limit is {}",
            src,
            self.info.table_count()
        );
        Ok((dst, src))
    }

    fn parse_call_indirect_immediates(&mut self) -> Result<(u32, u32), String> {
        pfail_if!(self, self.info.table_count() == 0, "call_indirect is only valid when a table is defined or imported");
        let signature_index = parse_or_fail!(self, self.parser.parse_var_uint32(), "can't get call_indirect's signature index");
        let table_index = parse_or_fail!(self, self.parser.parse_var_uint32(), "can't get call_indirect's table index");
        pfail_if!(
            self,
            table_index as usize >= self.info.table_count(),
            "call_indirect's table index {} invalid, limit is {}",
            table_index,
            self.info.table_count()
        );
        pfail_if!(
            self,
            self.info.type_count() <= signature_index as usize,
            "call_indirect's signature index {} exceeds known signatures {}",
            signature_index,
            self.info.type_count()
        );
        pfail_if!(
            self,
            self.info.tables[table_index as usize].element_type != TableElementType::Funcref,
            "call_indirect is only valid when a table has type funcref"
        );
        vfail_if!(
            self,
            self.info.rtt(signature_index as usize).structural.kind() != RttKind::Function,
            "invalid type index (not a function signature) for call_indirect, got {}",
            signature_index
        );
        Ok((signature_index, table_index))
    }

    fn parse_annotated_select_immediates(&mut self) -> Result<Type, String> {
        let size = parse_or_fail!(self, self.parser.parse_var_uint32(), "select can't parse the size of annotation vector");
        pfail_if!(self, size != 1, "select invalid result arity for");
        let target_type = parse_or_fail!(self, self.parser.parse_value_type(self.info), "select can't parse annotations");
        Ok(target_type)
    }

    // --- Verificações -------------------------------------------------------------------------

    /// `checkBranchTarget`; `index` é a posição na pilha de controle.
    pub(super) fn check_branch_target(&mut self, index: usize, conditional: bool) -> PartialResult {
        let (arity, is_top_level, signature) = {
            let data = &self.control_stack[index].control_data;
            (data.branch_target_arity(), data.is_top_level(), data.signature().dump())
        };
        if arity == 0 {
            return Ok(());
        }
        let slice_size = self.slice_size();
        vfail_if!(
            self,
            slice_size < arity,
            "{} on expression stack of size {}, but block, {} expects {} values",
            if is_top_level { "branch out of function" } else { "branch to block" },
            slice_size,
            signature,
            arity
        );
        let offset = self.expression_stack.len() - arity;
        for i in 0..arity {
            let target_type = self.control_stack[index].control_data.branch_target_type(i);
            let stack_type = self.expression_stack[offset + i];
            vfail_if!(
                self,
                !self.info.is_subtype(stack_type, target_type),
                "branch's stack type is not a subtype of block's type branch target type. Stack value has type {} but branch target expects a value of {} at index {}",
                self.ty(stack_type),
                self.ty(target_type),
                i
            );
            if conditional {
                // Types must widen to the branch target type via subtyping. See https://github.com/WebAssembly/gc/issues/516.
                self.expression_stack[offset + i] = target_type;
            }
        }
        Ok(())
    }

    fn check_local_initialized(&self, index: u32) -> PartialResult {
        // If typed funcrefs are off, non-defaultable locals fail earlier.
        if is_defaultable_type(self.locals[index as usize]) {
            return Ok(());
        }
        vfail_if!(
            self,
            !self.local_is_initialized(index),
            "non-defaultable function local {} is accessed before initialization",
            index
        );
        Ok(())
    }

    fn check_arguments_and_widen(&mut self, signature: &BlockSignature) -> PartialResult {
        let argument_count = signature.argument_count();
        let slice_size = self.slice_size();
        vfail_if!(
            self,
            slice_size < argument_count,
            "Too few values on stack for block. Block expects {}, but only {} were present. Block has signature: {}",
            argument_count,
            slice_size,
            signature.dump()
        );
        let offset = self.expression_stack.len() - argument_count;
        for i in 0..argument_count {
            let slot = self.expression_stack[offset + i];
            let expected = signature.argument_type(i);
            vfail_if!(
                self,
                !self.info.is_subtype(slot, expected),
                "Block expects the argument at index {} to be {} but argument has type {}",
                i,
                self.ty(expected),
                self.ty(slot)
            );
            // Widen the operand to the block's declared parameter type.
            self.expression_stack[offset + i] = expected;
        }
        Ok(())
    }

    fn check_results_and_widen(&mut self, signature: &BlockSignature) -> PartialResult {
        let slice_size = self.slice_size();
        vfail_if!(
            self,
            signature.return_count() != slice_size,
            " block with type: {} returns: {} but stack has: {} values",
            signature.dump(),
            signature.return_count(),
            slice_size
        );
        for i in 0..signature.return_count() {
            let slot = self.expression_stack[self.current_stack_begin + i];
            let expected = signature.return_type(i);
            vfail_if!(
                self,
                !self.info.is_subtype(slot, expected),
                "control flow returns with unexpected type. {} is not a {}",
                self.ty(slot),
                self.ty(expected)
            );
            // Widen the operand to the block's declared result type.
            self.expression_stack[self.current_stack_begin + i] = expected;
        }
        Ok(())
    }

    fn end_block_and_check_result_types(&mut self, entry: &ControlEntry<C::Control>) -> PartialResult {
        let signature = entry.control_data.signature().clone();
        self.check_results_and_widen(&signature)?;
        let parent_begin = self.parent_entry_begin();
        // We should avoid adding other callsites of endBlock: a new block is a merge point.
        self.hook(Hook::EndBlock)?;
        self.current_stack_begin = parent_begin;
        Ok(())
    }

    // --- Assinatura de bloco ------------------------------------------------------------------

    fn parse_block_signature(&mut self) -> Result<BlockSignature, String> {
        if let Some(kind_byte) = self.parser.peek_int7() {
            if is_valid_type_kind(kind_byte) {
                let type_kind = TypeKind::from_i8(kind_byte).expect("kind válido");
                if is_valid_heap_type_kind(i64::from(kind_byte)) || type_kind == TypeKind::Ref || type_kind == TypeKind::RefNull {
                    return self.parse_reftype_signature();
                }
                let ty = simple_type(type_kind);
                pfail_if!(
                    self,
                    !(is_value_type(ty, self.parser.use_wasm_simd) || ty.kind == TypeKind::Void),
                    "result type of block: {} is not a value type or Void",
                    type_kind.name()
                );
                self.parser.offset += 1;
                return Ok(BlockSignature::Type(ty));
            }
        }

        let index = parse_or_fail!(
            self,
            self.parser.parse_var_int64(),
            "Block-like instruction doesn't return value type but can't decode type section index"
        );
        pfail_if!(self, index < 0, "Block-like instruction signature index is negative");
        pfail_if!(
            self,
            index as u64 >= self.info.type_count() as u64,
            "Block-like instruction signature index is out of bounds. Index: {} type index space: {}",
            index,
            self.info.type_count()
        );
        match BlockSignature::from_position(self.info, index as u32) {
            Some(signature) => Ok(signature),
            None => self.pfail("Block-like instruction signature index does not refer to a function type definition".to_string()),
        }
    }

    fn parse_reftype_signature(&mut self) -> Result<BlockSignature, String> {
        let result_type = parse_or_fail!(self, self.parser.parse_value_type(self.info), "result type of block is not a valid ref type");
        Ok(BlockSignature::Type(result_type))
    }

    /// `parseBlockSignatureAndNotifySIMDUseIfNeeded`; a falha leva `message` (o texto de quem chama).
    fn parse_block_signature_and_notify(&mut self, message: String) -> Result<BlockSignature, String> {
        match self.parse_block_signature() {
            Ok(signature) => {
                if signature.has_returned_v128() {
                    self.context.notify_function_uses_simd();
                }
                Ok(signature)
            }
            // O C++ descarta a mensagem de dentro e usa a de quem chama.
            Err(_) => self.pfail(message),
        }
    }

    fn switch_to_block(&mut self, block: C::Control, argument_count: usize) {
        debug_assert_eq!(self.current_stack_begin, self.parent_entry_begin());
        let new_begin = self.expression_stack.len() - argument_count;
        let height = self.local_init_stack_height();
        self.control_stack.push(ControlEntry {
            else_block_stack: Vec::new(),
            enclosed_stack_begin: new_begin,
            local_init_stack_height: height,
            control_data: block,
        });
        self.current_stack_begin = new_begin;
    }

    /// `parseNestedBlocksEagerly`: devolve `shouldContinue`.
    fn parse_nested_blocks_eagerly(&mut self) -> Result<bool, String> {
        loop {
            // Only attempt to parse the most optimistic case of a single non-ref or void return signature.
            let kind_byte = match self.parser.peek_int7() {
                Some(kind_byte) if is_valid_type_kind(kind_byte) => kind_byte,
                _ => return Ok(true),
            };
            let ty = simple_type(TypeKind::from_i8(kind_byte).expect("kind válido"));
            if !(ty.kind == TypeKind::Void || is_value_type(ty, self.parser.use_wasm_simd)) {
                return Ok(true);
            }
            self.parser.offset += 1;

            self.hook(Hook::Op(OpType::Block))?;
            let block = self.context.make_control(BlockType::Block, BlockSignature::Type(ty));
            self.switch_to_block(block, 0);

            let source = self.parser.source();
            if self.parser.offset() >= source.len() {
                return Ok(false);
            }
            let next = source[self.parser.offset()];
            if !is_valid_op_type(i64::from(next)) {
                return Ok(false);
            }
            self.current_opcode = OpType::from_value(next).expect("opcode válido");
            if self.current_opcode == OpType::Block {
                self.parser.offset += 1;
                continue;
            }
            return Ok(false);
        }
    }

    // --- Casos numéricos ----------------------------------------------------------------------

    fn binary_case(&mut self, op: OpType, types: &[TypeKind]) -> PartialResult {
        let right = pop_or_fail!(self, "binary right");
        let left = pop_or_fail!(self, "binary left");
        vfail_if!(self, left != simple_type(types[0]), "{} left value type mismatch", op.name());
        vfail_if!(self, right != simple_type(types[1]), "{} right value type mismatch", op.name());
        self.hook(Hook::Op(op))?;
        self.expression_stack.push(simple_type(types[2]));
        Ok(())
    }

    fn unary_case(&mut self, op: OpType, types: &[TypeKind]) -> PartialResult {
        let value = pop_or_fail!(self, "unary");
        vfail_if!(self, value != simple_type(types[0]), "{} value type mismatch", op.name());
        self.hook(Hook::Op(op))?;
        self.expression_stack.push(simple_type(types[1]));
        Ok(())
    }

    fn load(&mut self, memory_type: Type) -> PartialResult {
        vfail_if!(self, self.info.memory_count() == 0, "load instruction without memory");
        let alignment = parse_or_fail!(self, self.parser.parse_var_uint32(), "can't get load alignment");
        let (alignment, memory_index) = self.parse_memory_index_and_fixup_alignment(alignment)?;
        let natural = memory_log2_alignment(self.current_opcode);
        pfail_if!(
            self,
            alignment > natural,
            "byte alignment {} exceeds load's natural alignment {}",
            1u64 << alignment,
            1u64 << natural
        );
        let _offset = self.parse_memory_offset(memory_index)?;
        let pointer = pop_or_fail!(self, "load pointer");
        let address_kind = self.info.memories[memory_index as usize].address_type.as_wasm_type_kind();
        vfail_if!(self, pointer.kind != address_kind, "{} pointer type mismatch", self.current_opcode.name());
        self.hook(Hook::Op(self.current_opcode))?;
        self.expression_stack.push(memory_type);
        Ok(())
    }

    fn store(&mut self, memory_type: Type) -> PartialResult {
        vfail_if!(self, self.info.memory_count() == 0, "store instruction without memory");
        let alignment = parse_or_fail!(self, self.parser.parse_var_uint32(), "can't get store alignment");
        let (alignment, memory_index) = self.parse_memory_index_and_fixup_alignment(alignment)?;
        let natural = memory_log2_alignment(self.current_opcode);
        pfail_if!(
            self,
            alignment > natural,
            "byte alignment {} exceeds store's natural alignment {}",
            1u64 << alignment,
            1u64 << natural
        );
        let _offset = self.parse_memory_offset(memory_index)?;
        let value = pop_or_fail!(self, "store value");
        let pointer = pop_or_fail!(self, "store pointer");
        let address_kind = self.info.memories[memory_index as usize].address_type.as_wasm_type_kind();
        vfail_if!(self, pointer.kind != address_kind, "{} pointer type mismatch", self.current_opcode.name());
        vfail_if!(self, value != memory_type, "{} value type mismatch", self.current_opcode.name());
        self.hook(Hook::Op(self.current_opcode))?;
        Ok(())
    }

    fn trunc_saturated(&mut self, op: Ext1OpType, types: &[TypeKind]) -> PartialResult {
        let value = pop_or_fail!(self, "unary");
        let operand = simple_type(types[0]);
        vfail_if!(
            self,
            value != operand,
            "trunc-saturated value type mismatch. Expected: {} but expression stack has {}",
            self.ty(operand),
            self.ty(value)
        );
        self.hook(Hook::Ext1(op))?;
        self.expression_stack.push(simple_type(types[1]));
        Ok(())
    }

    // --- Pilha de controle --------------------------------------------------------------------

    pub(super) fn control_index(&self, target: u32) -> usize {
        self.control_stack.len() - 1 - target as usize
    }

    pub(super) fn heap_type_index(&self, heap_type: i32) -> TypeIndex {
        if heap_type >= 0 {
            self.info.type_index_of(heap_type as usize)
        } else {
            TypeIndex::Abstract(TypeKind::from_i8(heap_type as i8).expect("tipo heap já validado"))
        }
    }

    /// `parseExpression`: o ramo de código alcançável.
    pub(super) fn parse_expression(&mut self) -> PartialResult {
        let op = self.current_opcode;
        match op {
            OpType::Select => {
                let condition = pop_or_fail!(self, "select condition");
                let zero = pop_or_fail!(self, "select zero");
                let non_zero = pop_or_fail!(self, "select non-zero");
                pfail_if!(self, is_ref_type(non_zero), "can't use ref-types with unannotated select");
                vfail_if!(self, !condition.is_i32(), "select condition must be i32, got {}", self.ty(condition));
                vfail_if!(
                    self,
                    non_zero != zero,
                    "select result types must match, got {} and {}",
                    self.ty(non_zero),
                    self.ty(zero)
                );
                self.hook_operand = Some(zero);
                self.hook(Hook::Op(op))?;
                self.expression_stack.push(zero);
                Ok(())
            }
            OpType::AnnotatedSelect => {
                let target_type = self.parse_annotated_select_immediates()?;
                let condition = pop_or_fail!(self, "select condition");
                let zero = pop_or_fail!(self, "select zero");
                let non_zero = pop_or_fail!(self, "select non-zero");
                vfail_if!(self, !condition.is_i32(), "select condition must be i32, got {}", self.ty(condition));
                vfail_if!(
                    self,
                    !self.info.is_subtype(non_zero, target_type),
                    "select result types must match, got {} and {}",
                    self.ty(non_zero),
                    self.ty(target_type)
                );
                vfail_if!(
                    self,
                    !self.info.is_subtype(zero, target_type),
                    "select result types must match, got {} and {}",
                    self.ty(zero),
                    self.ty(target_type)
                );
                self.hook_operand = Some(target_type);
                self.hook(Hook::Op(OpType::Select))?;
                self.expression_stack.push(target_type);
                Ok(())
            }
            OpType::F32Const => {
                parse_or_fail!(self, self.parser.parse_uint32(), "can't parse 32-bit floating-point constant");
                self.expression_stack.push(simple_type(TypeKind::F32));
                Ok(())
            }
            OpType::I32Const => {
                parse_or_fail!(self, self.parser.parse_var_int32(), "can't parse 32-bit constant");
                self.expression_stack.push(simple_type(TypeKind::I32));
                Ok(())
            }
            OpType::F64Const => {
                parse_or_fail!(self, self.parser.parse_uint64(), "can't parse 64-bit floating-point constant");
                self.expression_stack.push(simple_type(TypeKind::F64));
                Ok(())
            }
            OpType::I64Const => {
                parse_or_fail!(self, self.parser.parse_var_int64(), "can't parse 64-bit constant");
                self.expression_stack.push(simple_type(TypeKind::I64));
                Ok(())
            }
            OpType::TableGet => {
                let table_index = parse_or_fail!(self, self.parser.parse_var_uint32(), "can't parse table index");
                vfail_if!(
                    self,
                    table_index as usize >= self.info.table_count(),
                    "table index {} is invalid, limit is {}",
                    table_index,
                    self.info.table_count()
                );
                let index = pop_or_fail!(self, "table.get");
                let address_kind = self.info.tables[table_index as usize].address_type.as_wasm_type_kind();
                vfail_if!(
                    self,
                    address_kind != index.kind,
                    "table.get index to type {} expected {}",
                    self.ty(index),
                    address_kind.name()
                );
                self.hook(Hook::Op(op))?;
                self.expression_stack.push(self.info.tables[table_index as usize].wasm_type);
                Ok(())
            }
            OpType::TableSet => {
                let table_index = parse_or_fail!(self, self.parser.parse_var_uint32(), "can't parse table index");
                vfail_if!(
                    self,
                    table_index as usize >= self.info.table_count(),
                    "table index {} is invalid, limit is {}",
                    table_index,
                    self.info.table_count()
                );
                let value = pop_or_fail!(self, "table.set");
                let index = pop_or_fail!(self, "table.set");
                let address_kind = self.info.tables[table_index as usize].address_type.as_wasm_type_kind();
                vfail_if!(
                    self,
                    address_kind != index.kind,
                    "table.set index to type {} expected {}",
                    self.ty(index),
                    address_kind.name()
                );
                let table_type = self.info.tables[table_index as usize].wasm_type;
                vfail_if!(
                    self,
                    !self.info.is_subtype(value, table_type),
                    "table.set value to type {} expected {}",
                    self.ty(value),
                    self.ty(table_type)
                );
                self.hook(Hook::Op(op))
            }
            OpType::Ext1 => self.parse_ext1(),
            OpType::ExtGC => self.parse_ext_gc(),
            OpType::ExtAtomic => self.parse_ext_atomic(),
            OpType::ExtSIMD => self.parse_ext_simd(true),
            OpType::RefNull => {
                let heap_type = parse_or_fail!(
                    self,
                    self.parser.parse_heap_type(self.info),
                    "ref.null heaptype must be funcref, externref or type_idx"
                );
                let type_of_null = Type::new(TypeKind::RefNull, self.heap_type_index(heap_type));
                self.expression_stack.push(type_of_null);
                Ok(())
            }
            OpType::RefIsNull => {
                let value = pop_or_fail!(self, "ref.is_null");
                vfail_if!(self, !is_ref_type(value), "ref.is_null to type {} expected a reference type", self.ty(value));
                self.hook(Hook::Op(op))?;
                self.expression_stack.push(simple_type(TypeKind::I32));
                Ok(())
            }
            OpType::RefFunc => {
                let index = parse_or_fail!(self, self.parse_function_index().ok(), "can't get index for ref.func");
                // Function references don't need to be declared in constant expression contexts.
                if C::REF_FUNC_NEEDS_DECLARATION {
                    vfail_if!(self, !self.info.is_declared_function(index as usize), "ref.func index {} isn't declared", index);
                }
                self.hook(Hook::RefFunc(index))?;
                let signature_index = self.info.type_signature_index_from_function_index_space(index as usize);
                self.expression_stack.push(Type::new(TypeKind::Ref, self.info.type_index_of(signature_index as usize)));
                Ok(())
            }
            OpType::RefAsNonNull => {
                let reference = pop_or_fail!(self, "ref.as_non_null");
                vfail_if!(
                    self,
                    !is_ref_type(reference),
                    "ref.as_non_null ref to type {} expected a reference type",
                    self.ty(reference)
                );
                self.hook(Hook::Op(op))?;
                self.expression_stack.push(Type::new(TypeKind::Ref, reference.index));
                Ok(())
            }
            OpType::BrOnNull => {
                let target = self.parse_branch_target(0)?;
                let reference = pop_or_fail!(self, "br_on_null");
                vfail_if!(
                    self,
                    !is_ref_type(reference),
                    "br_on_null ref to type {} expected a reference type",
                    self.ty(reference)
                );
                let index = self.control_index(target);
                self.check_branch_target(index, true)?;
                self.hook(Hook::Op(op))?;
                self.expression_stack.push(Type::new(TypeKind::Ref, reference.index));
                Ok(())
            }
            OpType::BrOnNonNull => {
                let target = self.parse_branch_target(0)?;
                // Pop the stack manually to avoid changing the stack size, because the branch needs the value with a different type.
                pfail_if!(
                    self,
                    self.expression_stack.len() == self.current_stack_begin,
                    "can't pop empty stack in br_on_non_null"
                );
                let reference = self.expression_stack.pop().unwrap();
                self.expression_stack.push(Type::new(TypeKind::Ref, reference.index));
                vfail_if!(
                    self,
                    !is_ref_type(reference),
                    "br_on_non_null ref to type {} expected a reference type",
                    self.ty(reference)
                );
                let index = self.control_index(target);
                self.check_branch_target(index, true)?;
                self.hook(Hook::Op(op))?;
                // On a non-taken branch, the value is null so it's not needed on the stack.
                pop_or_fail!(self, "br_on_non_null");
                self.hook(Hook::Op(OpType::Drop))
            }
            OpType::RefEq => {
                let ref0 = pop_or_fail!(self, "ref.eq");
                let ref1 = pop_or_fail!(self, "ref.eq");
                let eqref = Type::new(TypeKind::RefNull, TypeIndex::Abstract(TypeKind::Eqref));
                vfail_if!(
                    self,
                    !self.info.is_subtype(ref0, eqref),
                    "ref.eq ref0 to type {} expected {}",
                    ref0.kind.name(),
                    TypeKind::Eqref.name()
                );
                vfail_if!(
                    self,
                    !self.info.is_subtype(ref1, eqref),
                    "ref.eq ref1 to type {} expected {}",
                    ref1.kind.name(),
                    TypeKind::Eqref.name()
                );
                self.hook(Hook::Op(op))?;
                self.expression_stack.push(simple_type(TypeKind::I32));
                Ok(())
            }
            OpType::GetLocal => {
                let index = self.parse_index_for_local()?;
                self.check_local_initialized(index)?;
                self.hook(Hook::Op(op))?;
                self.expression_stack.push(self.locals[index as usize]);
                Ok(())
            }
            OpType::SetLocal => {
                let index = self.parse_index_for_local()?;
                self.push_local_initialized(index);
                let value = pop_or_fail!(self, "set_local");
                let local = self.locals[index as usize];
                vfail_if!(
                    self,
                    !self.info.is_subtype(value, local),
                    "set_local to type {} expected {}",
                    self.ty(value),
                    self.ty(local)
                );
                self.hook(Hook::Op(op))
            }
            OpType::TeeLocal => {
                let index = self.parse_index_for_local()?;
                self.push_local_initialized(index);
                pfail_if!(
                    self,
                    self.expression_stack.len() == self.current_stack_begin,
                    "can't tee_local on empty expression stack"
                );
                let value = pop_or_fail!(self, "tee_local");
                let local = self.locals[index as usize];
                vfail_if!(
                    self,
                    !self.info.is_subtype(value, local),
                    "set_local to type {} expected {}",
                    self.ty(value),
                    self.ty(local)
                );
                self.hook(Hook::Op(op))?;
                self.expression_stack.push(local);
                Ok(())
            }
            OpType::GetGlobal => {
                let index = self.parse_index_for_global()?;
                let result_type = self.info.globals[index as usize].ty;
                if result_type.is_v128() {
                    self.context.notify_function_uses_simd();
                }
                self.hook(Hook::GetGlobal(index))?;
                self.expression_stack.push(result_type);
                Ok(())
            }
            OpType::SetGlobal => {
                let index = self.parse_index_for_global()?;
                vfail_if!(
                    self,
                    index as usize >= self.info.globals.len(),
                    "set_global {} of unknown global, limit is {}",
                    index,
                    self.info.globals.len()
                );
                vfail_if!(
                    self,
                    self.info.globals[index as usize].mutability == crate::wasm::wasm_format::Mutability::Immutable,
                    "set_global {} is immutable",
                    index
                );
                let value = pop_or_fail!(self, "set_global value");
                let global_type = self.info.globals[index as usize].ty;
                vfail_if!(
                    self,
                    !self.info.is_subtype(value, global_type),
                    "set_global {} with type {} with a variable of type {}",
                    index,
                    global_type.kind.name(),
                    value.kind.name()
                );
                if global_type.is_v128() {
                    self.context.notify_function_uses_simd();
                }
                self.hook(Hook::Op(op))
            }
            OpType::TailCall | OpType::Call => self.parse_call(),
            OpType::TailCallIndirect | OpType::CallIndirect => self.parse_call_indirect(),
            OpType::TailCallRef | OpType::CallRef => self.parse_call_ref(),
            OpType::Block => {
                // First try parsing the simple cases with potentially repeated block instructions.
                if !self.parse_nested_blocks_eagerly()? {
                    return Ok(());
                }
                let signature = self.parse_block_signature_and_notify("can't get block's signature".to_string())?;
                let argument_count = signature.argument_count();
                self.check_arguments_and_widen(&signature)?;
                self.hook(Hook::Op(op))?;
                let block = self.context.make_control(BlockType::Block, signature);
                self.switch_to_block(block, argument_count);
                Ok(())
            }
            OpType::Loop => {
                let signature = self.parse_block_signature_and_notify("can't get loop's signature".to_string())?;
                let argument_count = signature.argument_count();
                self.check_arguments_and_widen(&signature)?;
                self.hook(Hook::Op(op))?;
                self.loop_index += 1;
                let block = self.context.make_control(BlockType::Loop, signature);
                self.switch_to_block(block, argument_count);
                Ok(())
            }
            OpType::If => {
                let signature = self.parse_block_signature_and_notify("can't get if's signature".to_string())?;
                let condition = pop_or_fail!(self, "if condition");
                vfail_if!(self, !condition.is_i32(), "if condition must be i32, got {}", self.ty(condition));
                let argument_count = signature.argument_count();
                self.check_arguments_and_widen(&signature)?;
                let parent_stack_height = self.expression_stack.len() - argument_count;
                self.hook(Hook::Op(op))?;
                let else_save = self.expression_stack[parent_stack_height..].to_vec();
                let control = self.context.make_control(BlockType::If, signature);
                debug_assert_eq!(self.current_stack_begin, self.parent_entry_begin());
                let height = self.local_init_stack_height();
                self.control_stack.push(ControlEntry {
                    else_block_stack: else_save,
                    enclosed_stack_begin: parent_stack_height,
                    local_init_stack_height: height,
                    control_data: control,
                });
                self.current_stack_begin = parent_stack_height;
                Ok(())
            }
            OpType::Else => {
                pfail_if!(self, self.control_stack.len() == 1, "can't use else block at the top-level of a function");
                let last = self.control_stack.len() - 1;
                vfail_if!(self, !self.control_stack[last].control_data.is_if(), "else block isn't associated to an if");
                let signature = self.control_stack[last].control_data.signature().clone();
                self.check_results_and_widen(&signature)?;
                self.hook(Hook::Op(op))?;
                self.control_stack[last].control_data.set_block_type(BlockType::Else);
                self.expression_stack.truncate(self.current_stack_begin);
                let else_stack = self.control_stack[last].else_block_stack.clone();
                self.expression_stack.extend(else_stack);
                let height = self.control_stack[last].local_init_stack_height;
                self.reset_local_init_stack_to_height(height);
                Ok(())
            }
            OpType::Try => {
                self.uses_legacy_exceptions = true;
                let signature = self.parse_block_signature_and_notify("can't get try's signature".to_string())?;
                let argument_count = signature.argument_count();
                self.check_arguments_and_widen(&signature)?;
                let parent_stack_height = self.expression_stack.len() - argument_count;
                self.hook(Hook::Op(op))?;
                let control = self.context.make_control(BlockType::Try, signature);
                debug_assert_eq!(self.current_stack_begin, self.parent_entry_begin());
                let height = self.local_init_stack_height();
                self.control_stack.push(ControlEntry {
                    else_block_stack: Vec::new(),
                    enclosed_stack_begin: parent_stack_height,
                    local_init_stack_height: height,
                    control_data: control,
                });
                self.current_stack_begin = parent_stack_height;
                Ok(())
            }
            OpType::Catch => {
                pfail_if!(self, self.control_stack.len() == 1, "can't use catch block at the top-level of a function");
                let exception_index = self.parse_exception_index()?;
                let signature_index = self.info.type_signature_index_from_exception_index_space(exception_index as usize);
                let exception_arguments = function_signature(self.info.rtt(signature_index as usize)).0.to_vec();
                let last = self.control_stack.len() - 1;
                let data = &self.control_stack[last].control_data;
                vfail_if!(self, !(data.is_try() || data.is_catch()), "catch block isn't associated to a try");
                let signature = data.signature().clone();
                self.check_results_and_widen(&signature)?;
                self.hook(Hook::Op(op))?;
                self.control_stack[last].control_data.set_block_type(BlockType::Catch);
                self.expression_stack.truncate(self.current_stack_begin);
                for argument in exception_arguments {
                    if argument.is_v128() {
                        self.context.notify_function_uses_simd();
                    }
                    self.expression_stack.push(argument);
                }
                let height = self.control_stack[last].local_init_stack_height;
                self.reset_local_init_stack_to_height(height);
                Ok(())
            }
            OpType::CatchAll => {
                pfail_if!(self, self.control_stack.len() == 1, "can't use catch block at the top-level of a function");
                let last = self.control_stack.len() - 1;
                let data = &self.control_stack[last].control_data;
                vfail_if!(self, !(data.is_try() || data.is_catch()), "catch block isn't associated to a try");
                let signature = data.signature().clone();
                self.check_results_and_widen(&signature)?;
                self.hook(Hook::Op(op))?;
                self.control_stack[last].control_data.set_block_type(BlockType::Catch);
                self.expression_stack.truncate(self.current_stack_begin);
                let height = self.control_stack[last].local_init_stack_height;
                self.reset_local_init_stack_to_height(height);
                Ok(())
            }
            OpType::TryTable => self.parse_try_table(),
            OpType::Delegate => {
                pfail_if!(self, self.control_stack.len() == 1, "can't use delegate at the top-level of a function");
                let target = self.parse_delegate_target(0)?;
                let entry = self.control_stack.pop().unwrap();
                vfail_if!(self, !entry.control_data.is_try(), "delegate isn't associated to a try");
                let target_data = &self.control_stack[self.control_index(target)].control_data;
                vfail_if!(
                    self,
                    !target_data.is_try() && !target_data.is_top_level(),
                    "delegate target isn't a try or the top level block"
                );
                self.hook(Hook::Op(op))?;
                // Unlike the sibling catch/catch_all arms, delegate ends the try block, so it widens results.
                self.end_block_and_check_result_types(&entry)?;
                self.reset_local_init_stack_to_height(entry.local_init_stack_height);
                Ok(())
            }
            OpType::Throw => {
                let exception_index = self.parse_exception_index()?;
                let signature_index = self.info.type_signature_index_from_exception_index_space(exception_index as usize);
                let definition = self.info.rtt(signature_index as usize);
                let exception_arguments = function_signature(definition).0.to_vec();
                let slice_size = self.slice_size();
                vfail_if!(
                    self,
                    slice_size < exception_arguments.len(),
                    "Too few arguments on stack for the exception being thrown. The exception expects {}, but only {} were present. Exception has signature: {}",
                    exception_arguments.len(),
                    slice_size,
                    rtt_to_string(definition)
                );
                let count = exception_arguments.len();
                for i in 0..count {
                    let arg = self.expression_stack[self.expression_stack.len() - i - 1];
                    let expected = exception_arguments[count - i - 1];
                    vfail_if!(
                        self,
                        !self.info.is_subtype(arg, expected),
                        "The exception being thrown expects the argument at index {} to be {} but argument has type {}",
                        i,
                        self.ty(expected),
                        self.ty(arg)
                    );
                }
                let new_len = self.expression_stack.len() - count;
                self.expression_stack.truncate(new_len);
                self.hook(Hook::Op(op))?;
                self.unreachable_blocks = 1;
                Ok(())
            }
            OpType::ThrowRef => {
                let exnref = pop_or_fail!(self, "exception reference");
                let exnref_type = Type::new(TypeKind::RefNull, TypeIndex::Abstract(TypeKind::Exnref));
                vfail_if!(self, !self.info.is_subtype(exnref, exnref_type), "throw_ref expected an exception reference");
                self.hook(Hook::Op(op))?;
                self.unreachable_blocks = 1;
                Ok(())
            }
            OpType::Rethrow => {
                let target = self.parse_branch_target(0)?;
                let index = self.control_index(target);
                vfail_if!(
                    self,
                    !self.control_stack[index].control_data.is_any_catch(),
                    "rethrow doesn't refer to a catch block"
                );
                self.hook(Hook::Op(op))?;
                self.unreachable_blocks = 1;
                Ok(())
            }
            OpType::Br | OpType::BrIf => {
                let target = self.parse_branch_target(0)?;
                if op == OpType::BrIf {
                    let condition = pop_or_fail!(self, "br / br_if condition");
                    vfail_if!(self, !condition.is_i32(), "conditional branch with non-i32 condition {}", self.ty(condition));
                } else {
                    self.unreachable_blocks = 1;
                }
                let index = self.control_index(target);
                self.check_branch_target(index, op == OpType::BrIf)?;
                self.hook(Hook::Op(op))
            }
            OpType::BrTable => {
                let number_of_targets =
                    parse_or_fail!(self, self.parser.parse_var_uint32(), "can't get the number of targets for br_table");
                pfail_if!(
                    self,
                    number_of_targets == u32::MAX,
                    "br_table's number of targets is too big {}",
                    number_of_targets
                );
                let mut targets: Vec<usize> = Vec::new();
                let mut error_message: Option<String> = None;
                for i in 0..number_of_targets {
                    let Some(target) = self.parser.parse_var_uint32() else {
                        error_message.get_or_insert_with(|| format!("can't get {}th target for br_table", i));
                        break;
                    };
                    if target as usize >= self.control_stack.len() {
                        error_message.get_or_insert_with(|| {
                            format!("br_table's {}th target {} exceeds control stack size {}", i, target, self.control_stack.len())
                        });
                        break;
                    }
                    targets.push(self.control_index(target));
                }
                if let Some(message) = error_message {
                    return self.pfail(message);
                }
                let default_target_index =
                    parse_or_fail!(self, self.parser.parse_var_uint32(), "can't get default target for br_table");
                pfail_if!(
                    self,
                    default_target_index as usize >= self.control_stack.len(),
                    "br_table's default target {} exceeds control stack size {}",
                    default_target_index,
                    self.control_stack.len()
                );
                let default_target = self.control_index(default_target_index);
                let condition = pop_or_fail!(self, "br_table condition");
                vfail_if!(self, !condition.is_i32(), "br_table with non-i32 condition {}", self.ty(condition));
                let default_arity = self.control_stack[default_target].control_data.branch_target_arity();
                for (i, target) in targets.iter().enumerate() {
                    let arity = self.control_stack[*target].control_data.branch_target_arity();
                    vfail_if!(
                        self,
                        default_arity != arity,
                        "br_table target type size mismatch. Default has size: {}but target: {} has size: {}",
                        default_arity,
                        i,
                        arity
                    );
                    // In the presence of subtyping, we need to check each branch target.
                    self.check_branch_target(*target, false)?;
                }
                self.check_branch_target(default_target, false)?;
                self.hook(Hook::Op(op))?;
                self.unreachable_blocks = 1;
                Ok(())
            }
            OpType::Return => {
                self.check_branch_target(0, false)?;
                self.hook(Hook::Op(op))?;
                self.unreachable_blocks = 1;
                Ok(())
            }
            OpType::End => {
                let entry = self.control_stack.pop().unwrap();
                if entry.control_data.is_if() {
                    let signature = entry.control_data.signature().clone();
                    self.check_results_and_widen(&signature)?;
                    self.hook(Hook::Op(OpType::Else))?;
                    self.expression_stack.truncate(self.current_stack_begin);
                    self.expression_stack.extend(entry.else_block_stack.iter().copied());
                }
                self.end_block_and_check_result_types(&entry)?;
                if !entry.control_data.is_top_level() {
                    self.reset_local_init_stack_to_height(entry.local_init_stack_height);
                }
                Ok(())
            }
            OpType::Unreachable => {
                self.hook(Hook::Op(op))?;
                self.unreachable_blocks = 1;
                Ok(())
            }
            OpType::Drop => {
                self.hook_operand = Some(pop_or_fail!(self, "can't drop on empty stack"));
                self.hook(Hook::Op(op))
            }
            OpType::Nop => Ok(()),
            OpType::GrowMemory => {
                pfail_if!(
                    self,
                    self.info.memory_count() == 0,
                    "grow_memory is only valid if a memory is defined or imported"
                );
                let memory_index = self.parse_memory_index()?;
                let is_memory64 = self.info.memories[memory_index as usize].is_memory64();
                let delta = if is_memory64 {
                    let delta = pop_or_fail!(self, "expect an i64 argument to grow_memory on the stack");
                    vfail_if!(self, !delta.is_i64(), "grow_memory with non-i64 delta argument has type: {}", self.ty(delta));
                    delta
                } else {
                    let delta = pop_or_fail!(self, "expect an i32 argument to grow_memory on the stack");
                    vfail_if!(self, !delta.is_i32(), "grow_memory with non-i32 delta argument has type: {}", self.ty(delta));
                    delta
                };
                let _ = delta;
                self.hook(Hook::Op(op))?;
                self.expression_stack.push(simple_type(if is_memory64 { TypeKind::I64 } else { TypeKind::I32 }));
                Ok(())
            }
            OpType::CurrentMemory => {
                pfail_if!(
                    self,
                    self.info.memory_count() == 0,
                    "current_memory is only valid if a memory is defined or imported"
                );
                let memory_index = self.parse_memory_index()?;
                self.hook(Hook::Op(op))?;
                let is_memory64 = self.info.memories[memory_index as usize].is_memory64();
                self.expression_stack.push(simple_type(if is_memory64 { TypeKind::I64 } else { TypeKind::I32 }));
                Ok(())
            }
            _ => {
                let byte = op.value();
                if let Some(binary) = BinaryOpType::from_value(byte) {
                    return self.binary_case(op, binary.types());
                }
                if let Some(unary) = UnaryOpType::from_value(byte) {
                    return self.unary_case(op, unary.types());
                }
                if let Some(load) = LoadOpType::from_value(byte) {
                    return self.load(simple_type(load.types()[0]));
                }
                if let Some(store) = StoreOpType::from_value(byte) {
                    return self.store(simple_type(store.types()[0]));
                }
                unreachable!("opcode sem tratamento: {}", op.name())
            }
        }
    }

    fn parse_call(&mut self) -> PartialResult {
        let op = self.current_opcode;
        if op == OpType::TailCall {
            pfail_if!(self, !Options::with(|o| o.use_wasm_tail_calls), "wasm tail calls are not enabled");
        }
        let function_index = self.parse_function_index()?;
        let signature_index = self.info.type_signature_index_from_function_index_space(function_index as usize);
        let (arguments, returns) = {
            let (arguments, returns) = function_signature(self.info.rtt(signature_index as usize));
            (arguments.to_vec(), returns.to_vec())
        };
        let slice_size = self.slice_size();
        pfail_if!(
            self,
            arguments.len() > slice_size,
            "call function index {} has {} arguments, but the expression stack currently holds {} values",
            function_index,
            arguments.len(),
            slice_size
        );
        self.check_call_arguments(&arguments, "call", 0)?;
        if op == OpType::TailCall {
            self.check_tail_call_returns(&returns, &format!("tail call function index {}", function_index))?;
            self.hook(Hook::Op(op))?;
            self.unreachable_blocks = 1;
            return Ok(());
        }
        self.hook(Hook::Op(op))?;
        self.push_call_results(&returns);
        Ok(())
    }

    /// Confere e desempilha `arguments` (o topo da pilha, na ordem). `skip` é a quantidade de valores
    /// no topo que não são argumentos (o índice do `call_indirect`, a referência do `call_ref`).
    fn check_call_arguments(&mut self, arguments: &[Type], what: &str, skip: usize) -> PartialResult {
        let count = arguments.len() + skip;
        for i in 0..count {
            let arg = self.expression_stack[self.expression_stack.len() - i - 1];
            if i >= skip {
                let expected = arguments[count - i - 1];
                vfail_if!(
                    self,
                    !self.info.is_subtype(arg, expected),
                    "argument type mismatch in {}, got {}, expected {}",
                    what,
                    self.ty(arg),
                    self.ty(expected)
                );
            }
        }
        let new_len = self.expression_stack.len() - count;
        self.expression_stack.truncate(new_len);
        Ok(())
    }

    fn check_tail_call_returns(&mut self, returns: &[Type], prefix: &str) -> PartialResult {
        let caller_count = self.signature.return_count();
        pfail_if!(
            self,
            returns.len() != caller_count,
            "{} with return count {}, but the caller's signature has {} return values",
            prefix,
            returns.len(),
            caller_count
        );
        for (i, ret) in returns.iter().enumerate() {
            let expected = self.signature.return_type(i);
            vfail_if!(
                self,
                !self.info.is_subtype(*ret, expected),
                "{} return type mismatch: expected {}, got {}",
                prefix,
                self.ty(expected),
                self.ty(*ret)
            );
        }
        Ok(())
    }

    fn push_call_results(&mut self, returns: &[Type]) {
        for ret in returns {
            if ret.is_v128() {
                // We care SIMD only when it is not a tail-call: in tail-call case, return values are not visible to this function.
                self.context.notify_function_uses_simd();
            }
            self.expression_stack.push(*ret);
        }
    }

    fn parse_call_indirect(&mut self) -> PartialResult {
        let op = self.current_opcode;
        if op == OpType::TailCallIndirect {
            pfail_if!(self, !Options::with(|o| o.use_wasm_tail_calls), "wasm tail calls are not enabled");
        }
        let (signature_index, table_index) = self.parse_call_indirect_immediates()?;
        let (arguments, returns) = {
            let (arguments, returns) = function_signature(self.info.rtt(signature_index as usize));
            (arguments.to_vec(), returns.to_vec())
        };
        let argument_count = arguments.len() + 1; // Add the callee's index.
        let slice_size = self.slice_size();
        pfail_if!(
            self,
            argument_count > slice_size,
            "call_indirect expects {} arguments, but the expression stack currently holds {} values",
            argument_count,
            slice_size
        );
        let table_address_type = self.info.tables[table_index as usize].address_type.as_wasm_type();
        let last = *self.expression_stack.last().unwrap();
        vfail_if!(
            self,
            table_address_type != last,
            "call_indirect index to type {} expected {}",
            last.kind.name(),
            table_address_type.kind.name()
        );
        self.check_call_arguments(&arguments, "call_indirect", 1)?;
        if op == OpType::TailCallIndirect {
            let message = "tail call indirect";
            let caller_count = self.signature.return_count();
            pfail_if!(
                self,
                returns.len() != caller_count,
                // O texto do C++ tem um `"_s, but ...` colado dentro do literal; a mensagem o herda.
                "{} function with return count {}_s, but the caller's signature has {} return values",
                message,
                returns.len(),
                caller_count
            );
            for (i, ret) in returns.iter().enumerate() {
                let expected = self.signature.return_type(i);
                vfail_if!(
                    self,
                    !self.info.is_subtype(*ret, expected),
                    "tail call indirect return type mismatch: expected {}, got {}",
                    self.ty(expected),
                    self.ty(*ret)
                );
            }
            self.hook(Hook::Op(op))?;
            self.unreachable_blocks = 1;
            return Ok(());
        }
        self.hook(Hook::Op(op))?;
        self.push_call_results(&returns);
        Ok(())
    }

    fn parse_call_ref(&mut self) -> PartialResult {
        let op = self.current_opcode;
        if op == OpType::TailCallRef {
            pfail_if!(self, !Options::with(|o| o.use_wasm_tail_calls), "wasm tail calls are not enabled");
        }
        let raw_type_index = parse_or_fail!(self, self.parser.parse_var_uint32(), "can't get call_ref's signature index");
        vfail_if!(self, raw_type_index as usize >= self.info.type_count(), "call_ref index {} is out of bounds", raw_type_index);
        pfail_if!(
            self,
            self.expression_stack.len() == self.current_stack_begin,
            "can't call_ref on empty expression stack"
        );
        let definition = self.info.rtt(raw_type_index as usize);
        vfail_if!(
            self,
            definition.structural.kind() != RttKind::Function,
            "invalid type index (not a function signature) for call_ref, got {}",
            raw_type_index
        );
        let (arguments, returns) = {
            let (arguments, returns) = function_signature(definition);
            (arguments.to_vec(), returns.to_vec())
        };
        let callee_type = Type::new(TypeKind::RefNull, self.info.type_index_of(raw_type_index as usize));
        let last = *self.expression_stack.last().unwrap();
        vfail_if!(
            self,
            !self.info.is_subtype(last, callee_type),
            "invalid type for call_ref value, expected {} got {}",
            self.ty(callee_type),
            self.ty(last)
        );
        let argument_count = arguments.len() + 1; // Add the callee's value.
        let slice_size = self.slice_size();
        pfail_if!(
            self,
            argument_count > slice_size,
            "call_ref expects {} arguments, but the expression stack currently holds {} values",
            argument_count,
            slice_size
        );
        self.check_call_arguments(&arguments, "call_ref", 1)?;
        if op == OpType::TailCallRef {
            let caller_count = self.signature.return_count();
            pfail_if!(
                self,
                returns.len() != caller_count,
                "tail call indirect function with return count {}_s, but the caller's signature has {} return values",
                returns.len(),
                caller_count
            );
            for (i, ret) in returns.iter().enumerate() {
                let expected = self.signature.return_type(i);
                vfail_if!(
                    self,
                    !self.info.is_subtype(*ret, expected),
                    "tail call ref return type mismatch: expected {}, got {}",
                    self.ty(expected),
                    self.ty(*ret)
                );
            }
            self.hook(Hook::Op(op))?;
            self.unreachable_blocks = 1;
            return Ok(());
        }
        self.hook(Hook::Op(op))?;
        self.push_call_results(&returns);
        Ok(())
    }

    fn parse_try_table(&mut self) -> PartialResult {
        self.uses_modern_exceptions = true;
        let signature = self.parse_block_signature_and_notify("can't get try_table's signature".to_string())?;
        let argument_count = signature.argument_count();
        self.check_arguments_and_widen(&signature)?;

        let number_of_catches =
            parse_or_fail!(self, self.parser.parse_var_uint32(), "can't get the number of catch statements for try_table");
        pfail_if!(
            self,
            number_of_catches == u32::MAX,
            "try_table's number of catch targets is too big {}",
            number_of_catches
        );

        // (tipo do catch, argumentos da exceção, posição do alvo na pilha de controle)
        let mut targets: Vec<(CatchKind, Vec<Type>, usize)> = Vec::new();
        for i in 0..number_of_catches {
            // catch = (opcode), (tag?), (label)
            let catch_opcode = parse_or_fail!(self, self.parser.parse_uint8(), "can't read opcode of try_table catch at index {}", i);
            pfail_if!(
                self,
                catch_opcode > CatchKind::CatchAllRef as u8,
                "invalid opcode of try_table catch at index {},  opcode {} is invalid",
                i,
                catch_opcode
            );
            let kind = match catch_opcode {
                0 => CatchKind::Catch,
                1 => CatchKind::CatchRef,
                2 => CatchKind::CatchAll,
                _ => CatchKind::CatchAllRef,
            };
            let mut exception_arguments = Vec::new();
            if (catch_opcode) < CatchKind::CatchAll as u8 {
                let tag = parse_or_fail!(
                    self,
                    self.parse_exception_index().ok(),
                    "can't read tag of try_table catch at index {}",
                    i
                );
                let signature_index = self.info.type_signature_index_from_exception_index_space(tag as usize);
                exception_arguments = function_signature(self.info.rtt(signature_index as usize)).0.to_vec();
                for argument in &exception_arguments {
                    if argument.is_v128() {
                        self.context.notify_function_uses_simd();
                    }
                }
            }
            let label = parse_or_fail!(self, self.parser.parse_var_uint32(), "can't read label of try_table catch at index {}", i);
            pfail_if!(
                self,
                label as usize >= self.control_stack.len(),
                "try_table's catch target {} exceeds control stack size {}",
                label,
                self.control_stack.len()
            );
            targets.push((kind, exception_arguments, self.control_index(label)));
        }

        for (kind, exception_arguments, target) in &targets {
            let mut results: Vec<Type> = Vec::new();
            if *kind == CatchKind::Catch || *kind == CatchKind::CatchRef {
                results.extend(exception_arguments.iter().copied());
            }
            if *kind == CatchKind::CatchRef || *kind == CatchKind::CatchAllRef {
                results.push(Type::new(TypeKind::Ref, TypeIndex::Abstract(TypeKind::Exnref)));
            }
            let data = &self.control_stack[*target].control_data;
            vfail_if!(self, results.len() != data.branch_target_arity(), "");
            for (i, result) in results.iter().enumerate() {
                vfail_if!(
                    self,
                    !self.info.is_subtype(*result, data.branch_target_type(i)),
                    "try_table target type mismatch"
                );
            }
        }

        self.hook(Hook::Op(OpType::TryTable))?;
        let block = self.context.make_control(BlockType::TryTable, signature);
        self.switch_to_block(block, argument_count);
        Ok(())
    }

    fn parse_ext1(&mut self) -> PartialResult {
        self.current_ext_op = parse_or_fail!(self, self.parser.parse_var_uint32(), "can't parse 0xfc extended opcode");
        let ext = self.current_ext_op;
        let Some(op) = Ext1OpType::from_value(ext) else {
            return self.pfail(format!("invalid 0xfc extended op {}", ext));
        };
        let i32_kind = TypeKind::I32;
        match op {
            Ext1OpType::TableInit => {
                let (element_index, table_index) = self.parse_table_init_immediates()?;
                let element_type = self.info.elements[element_index as usize].element_type;
                let table_type = self.info.tables[table_index as usize].wasm_type;
                vfail_if!(
                    self,
                    !self.info.is_subtype(element_type, table_type),
                    "table.init requires table's type \"{}\" and element's type \"{}\" are the same",
                    self.ty(table_type),
                    self.ty(element_type)
                );
                let length = pop_or_fail!(self, "table.init");
                let src_offset = pop_or_fail!(self, "table.init");
                let dst_offset = pop_or_fail!(self, "table.init");
                let address_kind = self.info.tables[table_index as usize].address_type.as_wasm_type_kind();
                vfail_if!(
                    self,
                    dst_offset.kind != address_kind,
                    "table.init dst_offset to type {} expected {}",
                    self.ty(dst_offset),
                    address_kind.name()
                );
                vfail_if!(
                    self,
                    i32_kind != src_offset.kind,
                    "table.init src_offset to type {} expected {}",
                    self.ty(src_offset),
                    i32_kind.name()
                );
                vfail_if!(
                    self,
                    i32_kind != length.kind,
                    "table.init length to type {} expected {}",
                    self.ty(length),
                    i32_kind.name()
                );
                self.hook(Hook::Ext1(op))
            }
            Ext1OpType::ElemDrop => {
                self.parse_element_index()?;
                self.hook(Hook::Ext1(op))
            }
            Ext1OpType::TableSize => {
                let table_index = self.parse_table_index()?;
                self.hook(Hook::Ext1(op))?;
                self.expression_stack.push(self.info.tables[table_index as usize].address_type.as_wasm_type());
                Ok(())
            }
            Ext1OpType::TableGrow => {
                let table_index = self.parse_table_index()?;
                let delta = pop_or_fail!(self, "table.grow");
                let fill = pop_or_fail!(self, "table.grow");
                let table_type = self.info.tables[table_index as usize].wasm_type;
                vfail_if!(
                    self,
                    !self.info.is_subtype(fill, table_type),
                    "table.grow expects fill value of type {} got {}",
                    self.ty(table_type),
                    self.ty(fill)
                );
                let address_type = self.info.tables[table_index as usize].address_type;
                vfail_if!(
                    self,
                    delta.kind != address_type.as_wasm_type_kind(),
                    "table.grow expects an {} delta value, got {}",
                    if address_type.is_64_bit() { "i64" } else { "i32" },
                    self.ty(delta)
                );
                self.hook(Hook::Ext1(op))?;
                self.expression_stack.push(address_type.as_wasm_type());
                Ok(())
            }
            Ext1OpType::TableFill => {
                let table_index = self.parse_table_index()?;
                let count = pop_or_fail!(self, "table.fill");
                let fill = pop_or_fail!(self, "table.fill");
                let offset = pop_or_fail!(self, "table.fill");
                let table_type = self.info.tables[table_index as usize].wasm_type;
                let address_kind = self.info.tables[table_index as usize].address_type.as_wasm_type_kind();
                vfail_if!(
                    self,
                    !self.info.is_subtype(fill, table_type),
                    "table.fill expects fill value of type {} got {}",
                    self.ty(table_type),
                    self.ty(fill)
                );
                vfail_if!(
                    self,
                    offset.kind != address_kind,
                    "table.fill expects an {} offset value, got {}",
                    address_kind.name(),
                    self.ty(offset)
                );
                vfail_if!(
                    self,
                    count.kind != address_kind,
                    "table.fill expects an {} count value, got {}",
                    address_kind.name(),
                    self.ty(count)
                );
                self.hook(Hook::Ext1(op))
            }
            Ext1OpType::TableCopy => {
                let (dst_table, src_table) = self.parse_table_copy_immediates()?;
                let src_type = self.info.tables[src_table as usize].wasm_type;
                let dst_type = self.info.tables[dst_table as usize].wasm_type;
                vfail_if!(
                    self,
                    !self.info.is_subtype(src_type, dst_type),
                    "type mismatch at table.copy. got {} and {}",
                    self.ty(src_type),
                    self.ty(dst_type)
                );
                let length = pop_or_fail!(self, "table.copy");
                let src_offset = pop_or_fail!(self, "table.copy");
                let dst_offset = pop_or_fail!(self, "table.copy");
                let dst_address = self.info.tables[dst_table as usize].address_type;
                let src_address = self.info.tables[src_table as usize].address_type;
                vfail_if!(
                    self,
                    dst_offset.kind != dst_address.as_wasm_type_kind(),
                    "table.copy dst_offset to type {} expected {}",
                    self.ty(dst_offset),
                    dst_address.as_wasm_type_kind().name()
                );
                vfail_if!(
                    self,
                    src_offset.kind != src_address.as_wasm_type_kind(),
                    "table.copy src_offset to type {} expected {}",
                    self.ty(src_offset),
                    src_address.as_wasm_type_kind().name()
                );
                let length_kind = if dst_address.is_64_bit() && src_address.is_64_bit() { TypeKind::I64 } else { TypeKind::I32 };
                vfail_if!(
                    self,
                    length.kind != length_kind,
                    "table.copy length to type {} expected {}",
                    self.ty(length),
                    length_kind.name()
                );
                self.hook(Hook::Ext1(op))
            }
            Ext1OpType::MemoryFill => {
                vfail_if!(self, self.info.memory_count() == 0, "memory must be present");
                let memory_index = self.parse_memory_index()?;
                let count = pop_or_fail!(self, "memory.fill");
                let target_value = pop_or_fail!(self, "memory.fill");
                let dst = pop_or_fail!(self, "memory.fill");
                let address_kind = self.info.memories[memory_index as usize].address_type.as_wasm_type_kind();
                vfail_if!(
                    self,
                    address_kind != dst.kind,
                    "memory.fill dstAddress to type {} expected {}",
                    self.ty(dst),
                    address_kind.name()
                );
                vfail_if!(
                    self,
                    i32_kind != target_value.kind,
                    "memory.fill targetValue to type {} expected {}",
                    self.ty(target_value),
                    i32_kind.name()
                );
                vfail_if!(
                    self,
                    address_kind != count.kind,
                    "memory.fill size to type {} expected {}",
                    self.ty(count),
                    address_kind.name()
                );
                self.hook(Hook::Ext1(op))
            }
            Ext1OpType::MemoryCopy => {
                let dst_memory = self.parse_memory_index()?;
                let src_memory = self.parse_memory_index()?;
                vfail_if!(self, self.info.memory_count() == 0, "memory must be present");
                let count = pop_or_fail!(self, "memory.copy");
                let src = pop_or_fail!(self, "memory.copy");
                let dst = pop_or_fail!(self, "memory.copy");
                let dst_kind = self.info.memories[dst_memory as usize].address_type.as_wasm_type_kind();
                let src_kind = self.info.memories[src_memory as usize].address_type.as_wasm_type_kind();
                vfail_if!(
                    self,
                    dst_kind != dst.kind,
                    "memory.copy dstAddress to type {} expected {}",
                    self.ty(dst),
                    dst_kind.name()
                );
                vfail_if!(
                    self,
                    src_kind != src.kind,
                    "memory.copy targetValue to type {} expected {}",
                    self.ty(src),
                    src_kind.name()
                );
                let count_kind = if dst_kind == TypeKind::I64 && src_kind == TypeKind::I64 { TypeKind::I64 } else { TypeKind::I32 };
                vfail_if!(
                    self,
                    count_kind != count.kind,
                    "memory.copy size to type {} expected {}",
                    self.ty(count),
                    count_kind.name()
                );
                self.hook(Hook::Ext1(op))
            }
            Ext1OpType::MemoryInit => {
                let _data_segment = self.parse_data_segment_index()?;
                let memory_index = self.parse_memory_index()?;
                vfail_if!(self, self.info.memory_count() == 0, "memory must be present");
                let length = pop_or_fail!(self, "memory.init");
                let src = pop_or_fail!(self, "memory.init");
                let dst = pop_or_fail!(self, "memory.init");
                let dst_kind = self.info.memories[memory_index as usize].address_type.as_wasm_type_kind();
                vfail_if!(
                    self,
                    dst_kind != dst.kind,
                    "memory.init dst address to type {} expected {}",
                    self.ty(dst),
                    dst_kind.name()
                );
                vfail_if!(
                    self,
                    i32_kind != src.kind,
                    "memory.init src address to type {} expected {}",
                    self.ty(src),
                    i32_kind.name()
                );
                vfail_if!(
                    self,
                    i32_kind != length.kind,
                    "memory.init length to type {} expected {}",
                    self.ty(length),
                    i32_kind.name()
                );
                self.hook(Hook::Ext1(op))
            }
            Ext1OpType::DataDrop => {
                self.parse_data_segment_index()?;
                self.hook(Hook::Ext1(op))
            }
            Ext1OpType::I32TruncSatF32S
            | Ext1OpType::I32TruncSatF32U
            | Ext1OpType::I32TruncSatF64S
            | Ext1OpType::I32TruncSatF64U
            | Ext1OpType::I64TruncSatF32S
            | Ext1OpType::I64TruncSatF32U
            | Ext1OpType::I64TruncSatF64S
            | Ext1OpType::I64TruncSatF64U => self.trunc_saturated(op, op.types()),
            Ext1OpType::I64Add128 | Ext1OpType::I64Sub128 => {
                pfail_if!(self, !Options::with(|o| o.use_wasm_wide_arithmetic), "wasm wide arithmetic is not enabled");
                let rhs_hi = pop_or_fail!(self, "i64.add128/sub128");
                let rhs_lo = pop_or_fail!(self, "i64.add128/sub128");
                let lhs_hi = pop_or_fail!(self, "i64.add128/sub128");
                let lhs_lo = pop_or_fail!(self, "i64.add128/sub128");
                for (name, value) in [("lhs_lo", lhs_lo), ("lhs_hi", lhs_hi), ("rhs_lo", rhs_lo), ("rhs_hi", rhs_hi)] {
                    vfail_if!(
                        self,
                        TypeKind::I64 != value.kind,
                        "i64.add128/sub128 {} to type {} expected {}",
                        name,
                        self.ty(value),
                        TypeKind::I64.name()
                    );
                }
                self.hook(Hook::Ext1(op))?;
                self.expression_stack.push(simple_type(TypeKind::I64));
                self.expression_stack.push(simple_type(TypeKind::I64));
                Ok(())
            }
            Ext1OpType::I64MulWideS | Ext1OpType::I64MulWideU => {
                pfail_if!(self, !Options::with(|o| o.use_wasm_wide_arithmetic), "wasm wide arithmetic is not enabled");
                let rhs = pop_or_fail!(self, "i64.mul_wide");
                let lhs = pop_or_fail!(self, "i64.mul_wide");
                vfail_if!(
                    self,
                    TypeKind::I64 != lhs.kind,
                    "i64.mul_wide lhs to type {} expected {}",
                    self.ty(lhs),
                    TypeKind::I64.name()
                );
                vfail_if!(
                    self,
                    TypeKind::I64 != rhs.kind,
                    "i64.mul_wide rhs to type {} expected {}",
                    self.ty(rhs),
                    TypeKind::I64.name()
                );
                self.hook(Hook::Ext1(op))?;
                self.expression_stack.push(simple_type(TypeKind::I64));
                self.expression_stack.push(simple_type(TypeKind::I64));
                Ok(())
            }
        }
    }

    // --- Código inalcançável ------------------------------------------------------------------

    /// `parseUnreachableExpression`: só lê os imediatos (e confere o que o C++ confere).
    pub(super) fn parse_unreachable_expression(&mut self) -> PartialResult {
        debug_assert!(self.unreachable_blocks != 0);
        let op = self.current_opcode;
        match op {
            OpType::Else => {
                if self.unreachable_blocks > 1 {
                    return Ok(());
                }
                let last = self.control_stack.len() - 1;
                self.unreachable_blocks = 0;
                vfail_if!(self, !self.control_stack[last].control_data.is_if(), "else block isn't associated to an if");
                self.hook(Hook::ToUnreachable(op))?;
                self.control_stack[last].control_data.set_block_type(BlockType::Else);
                self.expression_stack.truncate(self.current_stack_begin);
                let else_stack = self.control_stack[last].else_block_stack.clone();
                self.expression_stack.extend(else_stack);
                let height = self.control_stack[last].local_init_stack_height;
                self.reset_local_init_stack_to_height(height);
                Ok(())
            }
            OpType::Catch => {
                let exception_index = self.parse_exception_index()?;
                let signature_index = self.info.type_signature_index_from_exception_index_space(exception_index as usize);
                let exception_arguments = function_signature(self.info.rtt(signature_index as usize)).0.to_vec();
                if self.unreachable_blocks > 1 {
                    return Ok(());
                }
                let last = self.control_stack.len() - 1;
                let data = &self.control_stack[last].control_data;
                vfail_if!(self, !(data.is_try() || data.is_catch()), "catch block isn't associated to a try");
                self.unreachable_blocks = 0;
                self.expression_stack.truncate(self.current_stack_begin);
                self.hook(Hook::ToUnreachable(op))?;
                self.control_stack[last].control_data.set_block_type(BlockType::Catch);
                for argument in exception_arguments {
                    if argument.is_v128() {
                        self.context.notify_function_uses_simd();
                    }
                    self.expression_stack.push(argument);
                }
                let height = self.control_stack[last].local_init_stack_height;
                self.reset_local_init_stack_to_height(height);
                Ok(())
            }
            OpType::CatchAll => {
                if self.unreachable_blocks > 1 {
                    return Ok(());
                }
                let last = self.control_stack.len() - 1;
                self.unreachable_blocks = 0;
                self.expression_stack.truncate(self.current_stack_begin);
                let data = &self.control_stack[last].control_data;
                vfail_if!(self, !(data.is_try() || data.is_catch()), "catch block isn't associated to a try");
                self.hook(Hook::ToUnreachable(op))?;
                self.control_stack[last].control_data.set_block_type(BlockType::Catch);
                let height = self.control_stack[last].local_init_stack_height;
                self.reset_local_init_stack_to_height(height);
                Ok(())
            }
            OpType::Delegate => {
                pfail_if!(self, self.control_stack.len() == 1, "can't use delegate at the top-level of a function");
                let target = self.parse_delegate_target(self.unreachable_blocks)?;
                if self.unreachable_blocks == 1 {
                    let entry = self.control_stack.pop().unwrap();
                    vfail_if!(self, !entry.control_data.is_try(), "delegate isn't associated to a try");
                    let data = &self.control_stack[self.control_index(target)].control_data;
                    vfail_if!(
                        self,
                        !data.is_try() && !data.is_top_level(),
                        "delegate target isn't a try block"
                    );
                    self.hook(Hook::ToUnreachable(op))?;
                    // Drop child's slice and pre-allocate result placeholder slots.
                    self.expression_stack.truncate(self.current_stack_begin);
                    let signature = entry.control_data.signature().clone();
                    for i in 0..signature.return_count() {
                        self.expression_stack.push(signature.return_type(i));
                    }
                    let parent_begin = self.parent_entry_begin();
                    self.hook(Hook::ToUnreachable(OpType::End))?;
                    self.current_stack_begin = parent_begin;
                    self.reset_local_init_stack_to_height(entry.local_init_stack_height);
                }
                self.unreachable_blocks -= 1;
                Ok(())
            }
            OpType::End => {
                if self.unreachable_blocks == 1 {
                    let entry = self.control_stack.pop().unwrap();
                    let parent_begin = self.parent_entry_begin();
                    if entry.control_data.is_if() {
                        self.hook(Hook::ToUnreachable(OpType::Else))?;
                        self.expression_stack.truncate(self.current_stack_begin);
                        self.expression_stack.extend(entry.else_block_stack.iter().copied());
                        self.end_block_and_check_result_types(&entry)?;
                    } else {
                        self.expression_stack.truncate(self.current_stack_begin);
                        let signature = entry.control_data.signature().clone();
                        for i in 0..signature.return_count() {
                            self.expression_stack.push(signature.return_type(i));
                        }
                        self.hook(Hook::ToUnreachable(OpType::End))?;
                    }
                    self.current_stack_begin = parent_begin;
                    if !entry.control_data.is_top_level() {
                        self.reset_local_init_stack_to_height(entry.local_init_stack_height);
                    }
                }
                self.unreachable_blocks -= 1;
                Ok(())
            }
            OpType::Try | OpType::Loop | OpType::If | OpType::Block => {
                self.unreachable_blocks += 1;
                parse_or_fail_block(self, op)
            }
            OpType::BrTable => {
                let number_of_targets = parse_or_fail!(
                    self,
                    self.parser.parse_var_uint32(),
                    "can't get the number of targets for br_table in unreachable context"
                );
                pfail_if!(
                    self,
                    number_of_targets == u32::MAX,
                    "br_table's number of targets is too big {}",
                    number_of_targets
                );
                for i in 0..number_of_targets {
                    parse_or_fail!(
                        self,
                        self.parser.parse_var_uint32(),
                        "can't get {}th target for br_table in unreachable context",
                        i
                    );
                }
                parse_or_fail!(
                    self,
                    self.parser.parse_var_uint32(),
                    "can't get default target for br_table in unreachable context"
                );
                Ok(())
            }
            OpType::TryTable => {
                self.unreachable_blocks += 1;
                let result = self.parse_block_signature_and_notify("can't get try_table's signature in unreachable context".to_string());
                result?;
                let number_of_catches = parse_or_fail!(
                    self,
                    self.parser.parse_var_uint32(),
                    "can't get the number of catch statements for try_table in unreachable context"
                );
                for _ in 0..number_of_catches {
                    let catch_opcode = parse_or_fail!(
                        self,
                        self.parser.parse_uint8(),
                        "can't get catch opcode for try_table in unreachable context"
                    );
                    pfail_if!(self, catch_opcode > 0x03, "invalid catch opcode for try_table in unreachable context");
                    if catch_opcode < 2 {
                        parse_or_fail!(
                            self,
                            self.parse_exception_index().ok(),
                            "invalid exception tag for try_table in unreachable context"
                        );
                    }
                    parse_or_fail!(
                        self,
                        self.parser.parse_var_uint32(),
                        "invalid destination label for try_table in unreachable context"
                    );
                }
                Ok(())
            }
            OpType::TailCallIndirect | OpType::CallIndirect => {
                if op == OpType::TailCallIndirect {
                    pfail_if!(self, !Options::with(|o| o.use_wasm_tail_calls), "wasm tail calls are not enabled");
                }
                self.parse_call_indirect_immediates()?;
                Ok(())
            }
            OpType::TailCallRef | OpType::CallRef => {
                if op == OpType::TailCallRef {
                    pfail_if!(self, !Options::with(|o| o.use_wasm_tail_calls), "wasm tail calls are not enabled");
                }
                parse_or_fail!(self, self.parser.parse_var_uint32(), "can't call_ref's signature index in unreachable context");
                Ok(())
            }
            OpType::F32Const => {
                parse_or_fail!(self, self.parser.parse_uint32(), "can't parse 32-bit floating-point constant");
                Ok(())
            }
            OpType::F64Const => {
                parse_or_fail!(self, self.parser.parse_uint64(), "can't parse 64-bit floating-point constant");
                Ok(())
            }
            OpType::GetLocal => {
                let index = self.parse_index_for_local()?;
                self.check_local_initialized(index)
            }
            OpType::SetLocal | OpType::TeeLocal => {
                let index = self.parse_index_for_local()?;
                self.push_local_initialized(index);
                Ok(())
            }
            OpType::GetGlobal | OpType::SetGlobal => {
                self.parse_index_for_global()?;
                Ok(())
            }
            OpType::TailCall | OpType::Call => {
                if op == OpType::TailCall {
                    pfail_if!(self, !Options::with(|o| o.use_wasm_tail_calls), "wasm tail calls are not enabled");
                }
                self.parse_function_index()?;
                Ok(())
            }
            OpType::Rethrow => {
                let target = self.parse_branch_target(0)?;
                let index = self.control_index(target);
                vfail_if!(
                    self,
                    !self.control_stack[index].control_data.is_any_catch(),
                    "rethrow doesn't refer to a catch block"
                );
                Ok(())
            }
            OpType::Br | OpType::BrIf => {
                self.parse_branch_target(self.unreachable_blocks)?;
                Ok(())
            }
            OpType::Throw => {
                self.parse_exception_index()?;
                Ok(())
            }
            OpType::I32Const => {
                parse_or_fail!(
                    self,
                    self.parser.parse_var_int32(),
                    "can't get immediate for {} in unreachable context",
                    op.name()
                );
                Ok(())
            }
            OpType::I64Const => {
                parse_or_fail!(
                    self,
                    self.parser.parse_var_int64(),
                    "can't get immediate for {} in unreachable context",
                    op.name()
                );
                Ok(())
            }
            OpType::Ext1 => {
                self.current_ext_op = parse_or_fail!(self, self.parser.parse_var_uint32(), "can't parse extended 0xfc opcode");
                let ext = self.current_ext_op;
                let Some(ext_op) = Ext1OpType::from_value(ext) else {
                    return self.pfail(format!("invalid extended 0xfc op {}", ext));
                };
                match ext_op {
                    Ext1OpType::TableInit => {
                        self.parse_table_init_immediates()?;
                    }
                    Ext1OpType::ElemDrop => {
                        self.parse_element_index()?;
                    }
                    Ext1OpType::TableSize | Ext1OpType::TableGrow | Ext1OpType::TableFill => {
                        self.parse_table_index()?;
                    }
                    Ext1OpType::TableCopy => {
                        self.parse_table_copy_immediates()?;
                    }
                    Ext1OpType::MemoryFill => {
                        self.parse_memory_index()?;
                    }
                    Ext1OpType::MemoryCopy => {
                        self.parse_memory_index()?;
                        self.parse_memory_index()?;
                    }
                    Ext1OpType::MemoryInit => {
                        self.parse_data_segment_index()?;
                        self.parse_memory_index()?;
                    }
                    Ext1OpType::DataDrop => {
                        self.parse_data_segment_index()?;
                    }
                    _ => {}
                }
                Ok(())
            }
            OpType::AnnotatedSelect => {
                self.parse_annotated_select_immediates()?;
                Ok(())
            }
            OpType::TableGet | OpType::TableSet => {
                self.parse_table_index()?;
                Ok(())
            }
            OpType::RefNull => {
                parse_or_fail!(
                    self,
                    self.parser.parse_heap_type(self.info),
                    "can't get heap type for {} in unreachable context",
                    op.name()
                );
                Ok(())
            }
            OpType::RefFunc => {
                let index = self.parse_function_index()?;
                // Function references don't need to be declared in constant expression contexts.
                if C::REF_FUNC_NEEDS_DECLARATION {
                    vfail_if!(self, !self.info.is_declared_function(index as usize), "ref.func index {} isn't declared", index);
                }
                Ok(())
            }
            OpType::BrOnNull | OpType::BrOnNonNull => {
                self.parse_branch_target(0)?;
                Ok(())
            }
            OpType::ExtGC => self.parse_unreachable_ext_gc(),
            OpType::GrowMemory | OpType::CurrentMemory => {
                pfail_if!(
                    self,
                    self.info.memory_count() == 0,
                    "grow_memory/current_memory is only valid if a memory is defined or imported"
                );
                self.parse_memory_index()?;
                Ok(())
            }
            OpType::ExtAtomic => self.parse_unreachable_ext_atomic(),
            OpType::ExtSIMD => self.parse_ext_simd(false),
            _ => {
                let byte = op.value();
                if LoadOpType::from_value(byte).is_some() || StoreOpType::from_value(byte).is_some() {
                    // two immediate cases
                    pfail_if!(self, self.info.memory_count() == 0, "load/store instruction without memory");
                    let alignment = parse_or_fail!(
                        self,
                        self.parser.parse_var_uint32(),
                        "can't get first immediate for {} in unreachable context",
                        op.name()
                    );
                    let (alignment, memory_index) = self.parse_memory_index_and_fixup_alignment(alignment)?;
                    let natural = memory_log2_alignment(op);
                    pfail_if!(
                        self,
                        alignment > natural,
                        "byte alignment {} exceeds {}'s natural alignment {}",
                        1u64 << alignment,
                        op.name(),
                        1u64 << natural
                    );
                    self.parse_memory_offset(memory_index)?;
                }
                // no immediate cases
                Ok(())
            }
        }
    }
}

/// O ramo `Try`/`Loop`/`If`/`Block` do código inalcançável.
fn parse_or_fail_block<C: Context>(parser: &mut FunctionParser<'_, '_, C>, op: OpType) -> PartialResult {
    parser.parse_block_signature_and_notify(format!("can't get inline type for {} in unreachable context", op.name())).map(|_| ())
}
