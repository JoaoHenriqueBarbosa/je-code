//! Porte de `bytecode/CodeBlock.h` e `CodeBlock.cpp`: o `CodeBlock` (o `UnlinkedCodeBlock`
//! linkado a um `JSGlobalObject` e a uma cadeia de escopo), com o necessário para o interpretador:
//! o construtor e o `finishCreation` (constantes, `FunctionExecutable`s de declarações e
//! expressões, handlers de exceção, `MetadataTable` e a passagem de linkagem do metadata por
//! instrução), `setConstantRegisters`, `initializeTemplateObjects`, `setNumParameters` e os
//! acessores que o LLInt e os caminhos lentos usam.
//!
//! `ENABLE(C_LOOP)` é 0 (`derived/cmakeconfig.h`), logo vale o ramo `#if !ENABLE(C_LOOP)` e o
//! número de registradores salvos pelo callee do LLInt vem do `RegisterSet::llintBaselineCalleeSaveRegisters()`
//! de x86_64 (`jit/RegisterSet.cpp`): `regCS1` (r12), `regCS2` (`jitDataRegister`), `regCS3`
//! (`numberTagRegister`) e `regCS4` (`notCellMaskRegister`).
//!
//! Modelo (ver `CONVENTIONS.md`): o `CodeBlock` é uma célula do C++; aqui é um struct comum que o
//! `Heap` do porte guarda numa arena, com o índice `CodeBlockId` (o `CodeBlock*` do
//! `interpreter/register.rs`) passado no `new`. As referências a outras células
//! (`WriteBarrier<T>`) são `Rc<RefCell<T>>` (`JSGlobalObjectRef`, `ScriptExecutableRef`,
//! `FunctionExecutableRef`, `UnlinkedCodeBlockRef`), como o resto do porte faz com `SymbolTableRef`.
//! O `VM` entra como parâmetro dos métodos que o usam (o `m_vm` do C++ é um ponteiro de volta).
//!
//! Divergências (tudo o que o C++ tem e este módulo não tem, com o motivo):
//!
//! - `m_instructionsRawPointer`, `m_hash` (`CodeBlockHash` calculado no uso), `m_creationTime`,
//!   `CrashChecker`, `m_magic`, `m_osrExitCounter`, `m_optimizationDelayCounter`,
//!   `m_reoptimizationRetryCounter`, `m_previousCounter`, `m_incomingCalls`,
//!   `m_llintGetByIdWatchpointMap`, `m_jitCode`, `m_jitData`, `m_lazyValueProfiles`,
//!   `m_alternative` e tudo de `ENABLE(JIT)`/`ENABLE(DFG_JIT)`/`ENABLE(FTL_JIT)` (tiers, OSR,
//!   jettison, `setupWithUnlinkedBaselineCode`, `handleCatch` do LLInt para o `HandlerInfo::nativeCode`):
//!   camadas de JIT não se portam; vale o ramo `#else` (`handler.initialize(unlinkedHandler)`).
//! - `ProgramCodeBlock`, `EvalCodeBlock`, `FunctionCodeBlock` e `ModuleProgramCodeBlock` são o mesmo
//!   `CodeBlock` (`CodeBlockRef`): só diferem no `Structure` da célula. Cada um tem um módulo fino
//!   com o `create` e o `create_copy_parsed_block` (`CopyParsedBlockTag`); o
//!   `ModuleProgramCodeBlock` passa ao `create` o
//!   `module_environment_symbol_table_constant_register_offset` (o `dynamicDowncast` do C++).
//!   O índice `CodeBlockId` da arena é reservado dentro do `create`.
//! - O profiler de tipos e o de fluxo de controle (`op_profile_type`, `op_profile_control_flow`,
//!   `functionHasExecutedCache`, `insertBasicBlockBoundariesForControlFlowProfiler`,
//!   `TypeLocation`, `wasCompiledWithTypeProfilerOpcodes`) existem só com o inspetor remoto, que
//!   não se porta (`CONVENTIONS.md`); o gerador nunca os emite sem ele.
//! - `valueProfileForBytecodeIndex`, `getArrayProfile`, `arithProfile*`: consumidores do DFG e do
//!   `$vm`, não do interpretador; entram com o `BytecodeDumper`/`JSDollarVM`.
//! - `m_codeOrigin` do `DataOnlyCallLinkInfo` (o `CodeOrigin { instruction.index() }` que o
//!   `initialize` grava): `DataOnlyCallLinkInfo` do porte não tem o campo; só o DFG/`retrieveCaller`
//!   o lê, e o LLInt o reconstrói a partir do `BytecodeIndex` do frame.

use std::cell::{Ref, RefCell};
use std::collections::HashSet;
use std::rc::Rc;

use crate::runtime::executable::JITCode;
use crate::bytecode::bytecode_index::BytecodeIndex;
use crate::bytecode::bytecode_ops::{
    OpCreateRest, OpGetFromScope, OpNewArrayBuffer, OpNewObject, OpPutToScope, OpResolveScope,
};
use crate::bytecode::code_type::CodeType;
use crate::bytecode::handler_info::{HandlerInfo, RequiredHandler};
use crate::bytecode::instruction_stream::{JSInstructionStream, Ref as InstructionRef, NUMBER_OF_BYTECODE_WITH_METADATA};
use crate::bytecode::metadata_table::{MetadataFor, MetadataTable, MetadataTableRef};
use crate::bytecode::op_metadata::*;
use crate::bytecode::opcode::{OpcodeID, OPCODE_LENGTHS};
use crate::bytecode::unlinked_code_block::{
    UnlinkedCodeBlock, UnlinkedSimpleJumpTable, UnlinkedStringJumpTable,
};
use crate::bytecode::value_profile::{ArgumentValueProfile, ValueProfile};
use crate::bytecode::virtual_register::VirtualRegister;
use crate::parser::source_provider::SourceProvider;
use crate::interpreter::register::CodeBlockId;
use crate::parser::parser_modes::JSParserScriptMode;
use crate::runtime::code_specialization_kind::{specialization_from_is_construct, CodeSpecializationKind};
use crate::runtime::js_function::FunctionExecutableRef;
use crate::runtime::js_symbol_table_object::SymbolTableObjectVariables;
use crate::runtime::script_executable::ScriptExecutableRef;
use crate::runtime::get_put_info::{
    is_initialization, GetOrPut, GetPutInfo, InitializationMode, ResolveType,
};
use crate::runtime::identifier::Identifier;
use crate::runtime::intrinsic::Intrinsic;
use crate::runtime::js_cjs_value_types::SourceCodeRepresentation;
use crate::runtime::js_global_object::JSGlobalObjectRef;
use crate::runtime::js_scope::{JSScope, JSScopeRef};
use crate::runtime::js_template_object_descriptor::JSTemplateObjectDescriptor;
use crate::runtime::js_value::JSValue;
use crate::runtime::options::Options;
use crate::runtime::symbol_table::{PropagateCloneInvalidationToOriginal, SymbolTable, SymbolTableRef};
use crate::runtime::unlinked_function_executable::UnlinkedFunctionExecutable;
use crate::runtime::vm::VM;
use crate::bytecode::bytecode_intrinsics_table::LINK_TIME_CONSTANT_TABLE;
use crate::parser::source_code::SourceCode;

/// `RefPtr<UnlinkedCodeBlock>`/`WriteBarrier<UnlinkedCodeBlock>`.
pub type UnlinkedCodeBlockRef = Rc<RefCell<UnlinkedCodeBlock>>;

/// `CodeBlock*`/`WriteBarrier<CodeBlock>`. `ProgramCodeBlock`, `EvalCodeBlock`, `FunctionCodeBlock`
/// e `ModuleProgramCodeBlock` têm o mesmo layout (o `static_assert(sizeof(...) == sizeof(CodeBlock))`
/// do C++) e só diferem no `Structure` da célula e no `create`, então o porte usa o mesmo tipo.
pub type CodeBlockRef = Rc<RefCell<CodeBlock>>;

/// O `CodeBlock` é grande demais para o `Debug` derivado; quem o guarda em estruturas com `Debug` (o `VM`)
/// só precisa que o tipo o implemente.
impl std::fmt::Debug for CodeBlock {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("CodeBlock")
    }
}

thread_local! {
    /// Próximo índice de `CodeBlock` na arena (0 é o `nullptr`).
    static NEXT_CODE_BLOCK_ID: std::cell::Cell<CodeBlockId> = const { std::cell::Cell::new(1) };
}

/// Reserva o índice do `CodeBlock*` (o endereço da célula no C++).
fn allocate_code_block_id() -> CodeBlockId {
    NEXT_CODE_BLOCK_ID.with(|next| {
        let id = next.get();
        next.set(id + 1);
        id
    })
}

/// `CallLinkInfo::CallType` (`CallLinkInfoBase.h`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum CallType {
    None = 0,
    Call = 1,
    CallVarargs = 2,
    Construct = 3,
    ConstructVarargs = 4,
    TailCall = 5,
    TailCallVarargs = 6,
    DirectCall = 7,
    DirectConstruct = 8,
    DirectTailCall = 9,
}

/// `CallLinkInfo::Mode` (`CallLinkInfo.h`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum CallLinkInfoMode {
    Init = 0,
    Monomorphic = 1,
    Polymorphic = 2,
    Virtual = 3,
}

impl CallLinkInfoMode {
    /// O `mode` gravado no `DataOnlyCallLinkInfo` (um byte, como o bitfield do C++).
    pub fn from_u8(mode: u8) -> CallLinkInfoMode {
        match mode {
            0 => CallLinkInfoMode::Init,
            1 => CallLinkInfoMode::Monomorphic,
            2 => CallLinkInfoMode::Polymorphic,
            3 => CallLinkInfoMode::Virtual,
            other => unreachable!("CallLinkInfo::Mode inválido: {other}"),
        }
    }
}

/// `CallLinkInfo::callTypeFor(OpcodeID)`.
pub fn call_type_for(opcode_id: OpcodeID) -> CallType {
    match opcode_id {
        OpcodeID::op_tail_call_varargs => CallType::TailCallVarargs,
        OpcodeID::op_call
        | OpcodeID::op_call_ignore_result
        | OpcodeID::op_call_direct_eval
        | OpcodeID::op_iterator_open
        | OpcodeID::op_iterator_next
        | OpcodeID::op_async_iterator_open
        | OpcodeID::op_async_iterator_next => CallType::Call,
        OpcodeID::op_call_varargs => CallType::CallVarargs,
        OpcodeID::op_construct | OpcodeID::op_super_construct => CallType::Construct,
        OpcodeID::op_construct_varargs | OpcodeID::op_super_construct_varargs => CallType::ConstructVarargs,
        OpcodeID::op_tail_call => CallType::TailCall,
        _ => unreachable!("RELEASE_ASSERT_NOT_REACHED: callTypeFor({opcode_id:?})"),
    }
}

/// `DataOnlyCallLinkInfo::initialize(vm, owner, callType, codeOrigin)` (`CallLinkInfo.cpp`), sem o
/// `m_codeOrigin` (ver o cabeçalho do módulo). `m_type` é `Type::DataOnly` por construção.
fn initialize_data_only_call_link_info(info: &mut DataOnlyCallLinkInfo, owner: CodeBlockId, call_type: CallType) {
    info.owner = owner as u32;
    info.call_type = call_type as u8;
    info.mode = CallLinkInfoMode::Init as u8;
    if !Options::use_ll_int_i_cs() {
        // `setVirtualCall(vm)`: `reset`, callee limpo, `setClearedByVirtual` e `Mode::Virtual`.
        info.callee = 0;
        info.code_block = 0;
        info.cleared_by_virtual = true;
        info.mode = CallLinkInfoMode::Virtual as u8;
    }
}

/// `CodeBlock::RareData`.
#[derive(Debug, Default)]
pub struct RareData {
    pub exception_handlers: Vec<HandlerInfo>,
    /// `m_directEvalCodeCache`.
    pub direct_eval_code_cache: crate::bytecode::direct_eval_code_cache::DirectEvalCodeCache,
}

/// `class CodeBlock`.
pub struct CodeBlock {
    /// O `CodeBlock*` na arena do heap (0 é nulo).
    id: CodeBlockId,
    /// `m_globalObject`.
    global_object: JSGlobalObjectRef,
    /// `m_numCalleeLocals`.
    num_callee_locals: u32,
    /// `m_numVars`.
    num_vars: u32,
    /// `m_numParameters`.
    num_parameters: u32,
    /// `m_numberOfArgumentsToSkip` (31 bits do `m_numberOfArgumentsToSkipAndCouldBeTainted`).
    number_of_arguments_to_skip: u32,
    /// `m_couldBeTainted` (o bit de sinal da mesma palavra).
    could_be_tainted: bool,
    /// `m_hasDebuggerStatement`.
    has_debugger_statement: bool,
    /// `m_steppingMode` (`SteppingModeDisabled`).
    stepping_mode_enabled: bool,
    /// `m_numBreakpoints`.
    num_breakpoints: u32,
    /// `m_bytecodeCost`.
    bytecode_cost: u32,
    /// `m_scopeRegister`.
    scope_register: VirtualRegister,
    /// `m_unlinkedCode`.
    unlinked_code: UnlinkedCodeBlockRef,
    /// `m_ownerExecutable`.
    owner_executable: ScriptExecutableRef,
    /// `m_metadata`: nulo quando o bytecode não tem metadata.
    metadata: Option<MetadataTableRef>,
    /// `m_argumentValueProfiles`.
    argument_value_profiles: Vec<ArgumentValueProfile>,
    /// `m_constantRegisters`.
    constant_registers: Vec<JSValue>,
    /// `m_functionDecls`.
    function_decls: Vec<FunctionExecutableRef>,
    /// `m_functionExprs`.
    function_exprs: Vec<FunctionExecutableRef>,
    /// `m_jitCode`: o `LLIntJITCode` que `LLInt::setEntrypoint` instala (nulo até lá).
    jit_code: Option<Rc<dyn JITCode>>,
    /// `m_rareData`.
    rare_data: Option<Box<RareData>>,
}

impl CodeBlock {
    /// `CodeBlock::setJITCode(Ref<JITCode>&&)`.
    pub fn set_jit_code(&mut self, code: Rc<dyn JITCode>) {
        self.jit_code = Some(code);
    }

    /// `CodeBlock::jitCode()`: o `ASSERT` do chamador é que o `setEntrypoint` já rodou.
    pub fn jit_code(&self) -> Rc<dyn JITCode> {
        Rc::clone(self.jit_code.as_ref().expect("m_jitCode"))
    }

    /// `CodeBlock::inferredName() const` (`utf8()` do C++, aqui já como `String`).
    pub fn inferred_name(&self) -> String {
        match self.code_type() {
            CodeType::GlobalCode => "<global>".to_string(),
            CodeType::EvalCode => "<eval>".to_string(),
            CodeType::FunctionCode => match &self.owner_executable {
                ScriptExecutableRef::Function(executable) => {
                    String::from_utf8_lossy(&executable.borrow().ecma_name().utf8()).into_owned()
                }
                _ => unreachable!("CodeBlock de função com owner que não é FunctionExecutable"),
            },
            // DIVERGÊNCIA: o ramo `USE(BUN_JSC_ADDITIONS)` (`sourceURL` do provider) não foi portado.
            CodeType::ModuleCode => "<module>".to_string(),
        }
    }

    /// `CodeBlock::inferredNameWithHash() const`: `makeString(inferredName(), "#", hash())`.
    /// DIVERGÊNCIA: `m_hash` não é guardado no bloco; o hash é recalculado a partir do fonte do
    /// executable (mesmo valor que o `hash()` do C++ produz no ramo da thread principal).
    pub fn inferred_name_with_hash(&self) -> String {
        format!("{}#{}", self.inferred_name(), self.owner_executable.hash_for(self.specialization_kind()))
    }

    /// `CodeBlock::dump(PrintStream&) const` (`dumpAssumingJITType(out, jitType())`).
    ///
    /// DIVERGÊNCIAS: sem `m_alternative` e sem JIT, o `jitType()` é `None` até o `m_jitCode` ser
    /// instalado e `InterpreterThunk` depois (impresso `LLInt`); os campos `m_shouldAlwaysBeInlined`, `m_didFailJITCompilation`,
    /// `m_didFailFTLCompilation` e `m_hasBeenCompiledWithFTL` só existem com o JIT e ficam de fora.
    /// O dump de bytecode é `dump_bytecode`; o C++ também não o chama daqui.
    pub fn dump(&self, out: &mut dyn std::fmt::Write) -> std::fmt::Result {
        macro_rules! owner {
            ($e:ident => $body:expr) => {
                match &self.owner_executable {
                    ScriptExecutableRef::Eval(rc) => {
                        let $e = rc.borrow();
                        $body
                    }
                    ScriptExecutableRef::Function(rc) => {
                        let $e = rc.borrow();
                        $body
                    }
                    ScriptExecutableRef::Program(rc) => {
                        let $e = rc.borrow();
                        $body
                    }
                    ScriptExecutableRef::ModuleProgram(rc) => {
                        let $e = rc.borrow();
                        $body
                    }
                }
            };
        }
        let owner_pointer: *const () = match &self.owner_executable {
            ScriptExecutableRef::Eval(rc) => Rc::as_ptr(rc) as *const (),
            ScriptExecutableRef::Function(rc) => Rc::as_ptr(rc) as *const (),
            ScriptExecutableRef::Program(rc) => Rc::as_ptr(rc) as *const (),
            ScriptExecutableRef::ModuleProgram(rc) => Rc::as_ptr(rc) as *const (),
        };
        // O `jitType()` é `None` até o `LLInt::setEntrypoint` instalar o `m_jitCode`; o dump de
        // `dumpGeneratedBytecodes` roda no fim do `finishCreation`, antes disso.
        let jit_type = if self.jit_code.is_some() { "LLInt" } else { "None" };
        write!(
            out,
            "{}:[{:p}->{:p}, {jit_type}",
            self.inferred_name_with_hash(),
            self as *const CodeBlock,
            owner_pointer
        )?;
        out.write_str(match self.code_type() {
            CodeType::GlobalCode => "Global",
            CodeType::EvalCode => "Eval",
            CodeType::FunctionCode => "Function",
            CodeType::ModuleCode => "Module",
        })?;
        if self.code_type() == CodeType::FunctionCode {
            out.write_str(match self.specialization_kind() {
                CodeSpecializationKind::CodeForCall => "Call",
                CodeSpecializationKind::CodeForConstruct => "Construct",
            })?;
        }
        write!(out, ", {}", self.instructions_size())?;
        if owner!(e => e.never_inline()) {
            out.write_str(" (NeverInline)")?;
        }
        if owner!(e => e.never_optimize()) {
            out.write_str(" (NeverOptimize)")?;
        } else if owner!(e => e.never_ftl_optimize()) {
            out.write_str(" (NeverFTLOptimize)")?;
        }
        if owner!(e => e.did_try_to_enter_in_loop()) {
            out.write_str(" (DidTryToEnterInLoop)")?;
        }
        if owner!(e => e.is_in_strict_context()) {
            out.write_str(" (StrictMode)")?;
        }
        out.write_str("]")
    }

    /// `CodeBlock::dumpBytecode()`: o dump de bytecode em `WTF::dataFile()` (stderr).
    pub fn dump_bytecode(&mut self) {
        let mut text = String::new();
        if self.dump_bytecode_to(&mut text).is_ok() {
            crate::wtf::data_log::data_log(&text);
        }
    }

    /// `CodeBlock::dumpBytecode(PrintStream&)`: `BytecodeGraph graph(this, instructions())` e
    /// `CodeBlockBytecodeDumper<CodeBlock>::dumpGraph`.
    pub fn dump_bytecode_to(&mut self, out: &mut dyn std::fmt::Write) -> std::fmt::Result {
        let unlinked_code = Rc::clone(&self.unlinked_code);
        let graph = {
            let unlinked = unlinked_code.borrow();
            crate::bytecode::bytecode_graph::BytecodeGraph::new(self, unlinked.instructions())
        };
        crate::bytecode::bytecode_dumper::dump_graph(self, &graph, out)
    }

    /// `CodeBlock::create`: o construtor `CodeBlock(VM&, Structure*, ScriptExecutable*,
    /// UnlinkedCodeBlock*, JSScope*)` seguido do `finishCreation`. Devolve `None` quando o
    /// `finishCreation` devolve `false` (a criação dos objetos de template lançou).
    /// `module_environment_symbol_table_constant_register_offset` é `Some` quando o
    /// `unlinkedCodeBlock` é um `UnlinkedModuleProgramCodeBlock`
    /// (`moduleEnvironmentSymbolTableConstantRegisterOffset()`).
    pub fn create(
        vm: &VM,
        owner_executable: &ScriptExecutableRef,
        unlinked_code_block: &UnlinkedCodeBlockRef,
        scope: &JSScopeRef,
        module_environment_symbol_table_constant_register_offset: Option<i32>,
    ) -> Option<CodeBlockRef> {
        let id = allocate_code_block_id();
        let code_block = Rc::new(RefCell::new(CodeBlock::new(vm, id, owner_executable, unlinked_code_block, scope)));
        let created = CodeBlock::finish_creation(
            &code_block,
            vm,
            owner_executable,
            unlinked_code_block,
            scope,
            module_environment_symbol_table_constant_register_offset,
        );
        if !created {
            return None;
        }
        Some(code_block)
    }

    /// `CodeBlock(VM&, Structure*, CopyParsedBlockTag, CodeBlock& other)` seguido do
    /// `finishCreation(VM&, CopyParsedBlockTag, CodeBlock& other)` (o `create(vm, CopyParsedBlock,
    /// other)` das subclasses). Compartilha a `MetadataTable` e copia as constantes e os
    /// `FunctionExecutable`s; `m_hasDebuggerStatement`, `m_steppingMode` e `m_numBreakpoints`
    /// recomeçam zerados e o `couldBeTainted` vem do original.
    pub fn create_copy_parsed_block(_vm: &VM, other: &CodeBlockRef) -> CodeBlockRef {
        let other = other.borrow();
        let mut code_block = CodeBlock {
            id: allocate_code_block_id(),
            global_object: Rc::clone(&other.global_object),
            num_callee_locals: other.num_callee_locals,
            num_vars: other.num_vars,
            num_parameters: 0,
            number_of_arguments_to_skip: other.number_of_arguments_to_skip,
            could_be_tainted: other.could_be_tainted,
            has_debugger_statement: false,
            stepping_mode_enabled: false,
            num_breakpoints: 0,
            bytecode_cost: other.bytecode_cost,
            scope_register: other.scope_register,
            unlinked_code: Rc::clone(&other.unlinked_code),
            owner_executable: other.owner_executable.clone(),
            metadata: other.metadata.clone(),
            argument_value_profiles: Vec::new(),
            constant_registers: other.constant_registers.clone(),
            function_decls: other.function_decls.clone(),
            function_exprs: other.function_exprs.clone(),
            jit_code: None,
            rare_data: None,
        };
        debug_assert!(code_block.scope_register.is_local());
        let allocate_argument_value_profiles = false;
        code_block.set_num_parameters(other.num_parameters, allocate_argument_value_profiles);
        if let Some(other_rare_data) = &other.rare_data {
            code_block.create_rare_data_if_necessary();
            if let Some(rare_data) = code_block.rare_data.as_mut() {
                rare_data.exception_handlers = other_rare_data.exception_handlers.clone();
            }
        }
        Rc::new(RefCell::new(code_block))
    }

    /// `CodeBlock(VM&, Structure*, ScriptExecutable* ownerExecutable, UnlinkedCodeBlock*, JSScope*)`.
    fn new(
        _vm: &VM,
        id: CodeBlockId,
        owner_executable: &ScriptExecutableRef,
        unlinked_code_block: &UnlinkedCodeBlockRef,
        scope: &JSScopeRef,
    ) -> CodeBlock {
        let (num_callee_locals, num_vars, scope_register, num_parameters, metadata) = {
            let mut unlinked = unlinked_code_block.borrow_mut();
            (
                unlinked.num_callee_locals(),
                unlinked.num_vars(),
                unlinked.scope_register(),
                unlinked.num_parameters(),
                // `unlinkedCodeBlock->metadata().link()`
                MetadataTable::link(unlinked.metadata()),
            )
        };
        debug_assert!(scope_register.is_local());
        // `ASSERT(source().provider())`, `source().provider()->couldBeTainted()`.
        let could_be_tainted = owner_executable
            .source()
            .provider()
            .is_some_and(|provider| provider.could_be_tainted());
        let mut code_block = CodeBlock {
            id,
            global_object: scope.realm(),
            num_callee_locals,
            num_vars,
            num_parameters: 0,
            number_of_arguments_to_skip: 0,
            could_be_tainted,
            has_debugger_statement: false,
            stepping_mode_enabled: false,
            num_breakpoints: 0,
            bytecode_cost: 0,
            scope_register,
            unlinked_code: Rc::clone(unlinked_code_block),
            owner_executable: owner_executable.clone(),
            metadata,
            argument_value_profiles: Vec::new(),
            constant_registers: Vec::new(),
            function_decls: Vec::new(),
            function_exprs: Vec::new(),
            jit_code: None,
            rare_data: None,
        };
        let allocate_argument_value_profiles = true;
        code_block.set_num_parameters(num_parameters, allocate_argument_value_profiles);
        code_block
    }

    /// `bool CodeBlock::finishCreation(VM&, ScriptExecutable*, UnlinkedCodeBlock*, JSScope*)`.
    ///
    /// The main purpose of this function is to generate linked bytecode from unlinked bytecode. The
    /// process of linking is taking an abstract representation of bytecode and tying it to a
    /// GlobalObject and scope chain. This process is not allowed to generate control flow or
    /// introduce new locals: the liveness analysis is cached in the `UnlinkedCodeBlock`.
    ///
    /// Recebe o `Rc` porque os caminhos de linkagem consultam o `CodeBlock` já criado
    /// (`JSScope::constantScopeForCodeBlock(type, this)`) enquanto o preenchem.
    fn finish_creation(
        this: &Rc<RefCell<CodeBlock>>,
        vm: &VM,
        owner_executable: &ScriptExecutableRef,
        unlinked_code_block: &UnlinkedCodeBlockRef,
        scope: &JSScopeRef,
        module_environment_symbol_table_constant_register_offset: Option<i32>,
    ) -> bool {
        let top_level_executable = owner_executable.top_level_executable();
        // We wait to initialize template objects until the end of finishCreation because it can
        // throw. We rely on linking to put the CodeBlock into a coherent state, so we can't throw
        // until we're all done linking.
        let template_object_indices = {
            let (constants, representations) = {
                let unlinked = unlinked_code_block.borrow();
                (unlinked.constant_registers().to_vec(), unlinked.constants_source_code_representation().to_vec())
            };
            this.borrow_mut().set_constant_registers(vm, &constants, &representations)
        };

        // We already have the cloned symbol table for the module environment since we need to
        // instantiate the module environments before linking the code block. We replace the stored
        // symbol table with the already cloned one.
        let mut module_environment = None;
        if let Some(offset) = module_environment_symbol_table_constant_register_offset {
            let environment = JSScope::as_module_environment(scope);
            let cloned_symbol_table = owner_executable.module_environment_symbol_table();
            this.borrow_mut()
                .replace_constant(VirtualRegister::new(offset), JSValue::from_cell(cloned_symbol_table.borrow().cell_id()));
            module_environment = Some(environment);
        }

        let source = owner_executable.source().clone();
        let is_inside_ordinary_function = owner_executable.is_inside_ordinary_function();

        let function_decls: Vec<_> = unlinked_code_block.borrow().function_decls().to_vec();
        let mut linked_function_decls = Vec::with_capacity(function_decls.len());
        for unlinked_executable in &function_decls {
            let mut executable = module_environment.as_ref().and_then(|environment| {
                instantiated_module_function_executable(environment, &top_level_executable, unlinked_executable)
            });
            if executable.is_none() {
                executable = Some(UnlinkedFunctionExecutable::link(
                    unlinked_executable,
                    vm,
                    Some(top_level_executable.clone()),
                    &source,
                    None,
                    Intrinsic::NoIntrinsic,
                    is_inside_ordinary_function,
                ));
            }
            linked_function_decls.extend(executable);
        }
        this.borrow_mut().function_decls = linked_function_decls;

        let function_exprs: Vec<_> = unlinked_code_block.borrow().function_exprs().to_vec();
        let mut linked_function_exprs = Vec::with_capacity(function_exprs.len());
        for unlinked_executable in &function_exprs {
            linked_function_exprs.push(UnlinkedFunctionExecutable::link(
                unlinked_executable,
                vm,
                Some(top_level_executable.clone()),
                &source,
                None,
                Intrinsic::NoIntrinsic,
                is_inside_ordinary_function,
            ));
        }
        this.borrow_mut().function_exprs = linked_function_exprs;

        let number_of_exception_handlers = unlinked_code_block.borrow().number_of_exception_handlers();
        if number_of_exception_handlers != 0 {
            let mut code_block = this.borrow_mut();
            code_block.create_rare_data_if_necessary();
            let mut handlers = Vec::with_capacity(number_of_exception_handlers);
            for i in 0..number_of_exception_handlers {
                let mut handler = HandlerInfo::default();
                handler.initialize(unlinked_code_block.borrow_mut().exception_handler(i));
                handlers.push(handler);
            }
            if let Some(rare_data) = code_block.rare_data.as_mut() {
                rare_data.exception_handlers = handlers;
            }
        }

        // Bookkeep the strongly referenced module environments.
        let mut strongly_referenced_module_environments: HashSet<usize> = HashSet::new();

        let instruction_iterator = unlinked_code_block.borrow().instructions().iter();
        // `m_metadataID` de cada instrução: o `addEntry` do gerador numerou as entradas de cada
        // opcode na ordem de emissão, e a passagem de linkagem as visita na mesma ordem.
        let mut next_metadata_id = [0u32; NUMBER_OF_BYTECODE_WITH_METADATA as usize];
        for instruction in instruction_iterator {
            let opcode_id = instruction.opcode_id_enum();
            this.borrow_mut().bytecode_cost += OPCODE_LENGTHS[opcode_id as usize] as u32 + 1;
            let metadata_id = if (opcode_id as u16) < NUMBER_OF_BYTECODE_WITH_METADATA {
                let counter = &mut next_metadata_id[opcode_id as usize];
                let id = *counter;
                *counter += 1;
                id
            } else {
                0
            };
            let instruction = instruction.freeze();
            CodeBlock::link_instruction(
                this,
                vm,
                scope,
                unlinked_code_block,
                &mut strongly_referenced_module_environments,
                &instruction,
                opcode_id,
                metadata_id,
            );
        }

        if Options::dump_generated_bytecodes() {
            this.borrow_mut().dump_bytecode();
        }

        CodeBlock::initialize_template_objects(this, vm, &top_level_executable, &template_object_indices)
    }

    /// Um passo do `switch (opcodeID)` do `finishCreation`: o `LINK(...)` e os `case` explícitos.
    /// As entradas que o C++ só constrói com `Metadata { bytecode }` (sem campo a ligar) já nascem
    /// no valor inicial na `MetadataTable`, então não têm ramo aqui.
    #[allow(clippy::too_many_arguments)]
    fn link_instruction(
        this: &Rc<RefCell<CodeBlock>>,
        vm: &VM,
        scope: &JSScopeRef,
        unlinked_code_block: &UnlinkedCodeBlockRef,
        strongly_referenced_module_environments: &mut HashSet<usize>,
        instruction: &InstructionRef,
        opcode_id: OpcodeID,
        metadata_id: u32,
    ) {
        let id = metadata_id as usize;
        let code_block_id = this.borrow().id;
        let global_object = Rc::clone(&this.borrow().global_object);

        // `link_callLinkInfo`
        macro_rules! link_call_link_info {
            ($metadata_type:ty) => {{
                let table = this.borrow().metadata.clone();
                if let Some(table) = table {
                    let mut table = table.borrow_mut();
                    let metadata = &mut table.get_mut::<$metadata_type>()[id];
                    initialize_data_only_call_link_info(&mut metadata.call_link_info, code_block_id, call_type_for(opcode_id));
                }
            }};
        }

        match opcode_id {
            // `link_arrayAllocationProfile`
            OpcodeID::op_new_array_buffer => {
                let bytecode = instruction.as_op::<OpNewArrayBuffer>();
                let table = this.borrow().metadata.clone();
                if let Some(table) = table {
                    table.borrow_mut().get_mut::<OpNewArrayBufferMetadata>()[id].array_allocation_profile =
                        ArrayAllocationProfile::with_indexing_type(bytecode.recommended_indexing_type);
                }
            }

            // `link_objectAllocationProfile`
            OpcodeID::op_new_object => {
                let bytecode = instruction.as_op::<OpNewObject>();
                let table = this.borrow().metadata.clone();
                if let Some(table) = table {
                    let object_prototype = global_object.object_prototype();
                    let mut table = table.borrow_mut();
                    let profile = &mut table.get_mut::<OpNewObjectMetadata>()[id].object_allocation_profile;
                    crate::bytecode::object_allocation_profile::initialize_profile(
                        profile,
                        vm,
                        &global_object,
                        code_block_id as crate::bytecode::op_metadata::HeapRef,
                        &object_prototype,
                        bytecode.inline_capacity,
                    );
                }
            }

            OpcodeID::op_call => link_call_link_info!(OpCallMetadata),
            OpcodeID::op_tail_call => link_call_link_info!(OpTailCallMetadata),
            OpcodeID::op_call_direct_eval => link_call_link_info!(OpCallDirectEvalMetadata),
            OpcodeID::op_construct => link_call_link_info!(OpConstructMetadata),
            OpcodeID::op_super_construct => link_call_link_info!(OpSuperConstructMetadata),
            OpcodeID::op_iterator_open => link_call_link_info!(OpIteratorOpenMetadata),
            OpcodeID::op_iterator_next => link_call_link_info!(OpIteratorNextMetadata),
            OpcodeID::op_async_iterator_open => link_call_link_info!(OpAsyncIteratorOpenMetadata),
            OpcodeID::op_async_iterator_next => link_call_link_info!(OpAsyncIteratorNextMetadata),
            OpcodeID::op_call_varargs => link_call_link_info!(OpCallVarargsMetadata),
            OpcodeID::op_tail_call_varargs => link_call_link_info!(OpTailCallVarargsMetadata),
            OpcodeID::op_construct_varargs => link_call_link_info!(OpConstructVarargsMetadata),
            OpcodeID::op_super_construct_varargs => link_call_link_info!(OpSuperConstructVarargsMetadata),
            OpcodeID::op_call_ignore_result => link_call_link_info!(OpCallIgnoreResultMetadata),

            OpcodeID::op_resolve_scope => {
                let bytecode = instruction.as_op::<OpResolveScope>();
                let ident = this.borrow().identifier(bytecode.var as usize).clone();
                assert!(bytecode.resolve_type != ResolveType::ResolvedClosureVar);

                let op = JSScope::abstract_resolve(
                    &global_object,
                    bytecode.local_scope_depth,
                    scope,
                    &ident,
                    GetOrPut::Get,
                    bytecode.resolve_type,
                    InitializationMode::NotInitialization,
                );

                let mut metadata = OpResolveScopeMetadata::default();
                metadata.resolve_type = ResolveTypeField(op.type_);
                metadata.local_scope_depth_or_global_lexical_binding_epoch = op.depth;
                if let Some(lexical_environment) = &op.lexical_environment {
                    if op.type_ == ResolveType::ModuleVar {
                        // Keep the linked module environment strongly referenced.
                        let cell_id = lexical_environment.cell_id();
                        if strongly_referenced_module_environments.insert(cell_id) {
                            this.borrow_mut().add_constant(JSValue::from_cell(cell_id));
                        }
                        metadata.lexical_environment_or_symbol_table_or_constant_scope_or_global_object = cell_id as u32;
                    } else {
                        metadata.lexical_environment_or_symbol_table_or_constant_scope_or_global_object =
                            lexical_environment.symbol_table_object().symbol_table().borrow().cell_id() as u32;
                    }
                } else if let Some(constant_scope) = JSScope::constant_scope_for_code_block(op.type_, &global_object) {
                    metadata.lexical_environment_or_symbol_table_or_constant_scope_or_global_object =
                        constant_scope.cell_id() as u32;
                    if op.type_ == ResolveType::GlobalProperty
                        || op.type_ == ResolveType::GlobalPropertyWithVarInjectionChecks
                    {
                        metadata.local_scope_depth_or_global_lexical_binding_epoch =
                            global_object.global_lexical_binding_epoch();
                    }
                } else {
                    // `metadata.m_globalObject.clear()`
                    metadata.lexical_environment_or_symbol_table_or_constant_scope_or_global_object = 0;
                }
                let table = this.borrow().metadata.clone();
                if let Some(table) = table {
                    table.borrow_mut().get_mut::<OpResolveScopeMetadata>()[id] = metadata;
                }
            }

            OpcodeID::op_get_from_scope => {
                let bytecode = instruction.as_op::<OpGetFromScope>();
                let mut metadata = OpGetFromScopeMetadata::new(bytecode.get_put_info, bytecode.offset);
                let table = this.borrow().metadata.clone();

                // `metadata.m_watchpointSet = nullptr;`
                metadata.watchpoint_set_or_structure_id = 0;

                debug_assert!(!is_initialization(bytecode.get_put_info.initialization_mode()));
                if bytecode.get_put_info.resolve_type() == ResolveType::ResolvedClosureVar {
                    metadata.get_put_info = GetPutInfo::new(
                        bytecode.get_put_info.resolve_mode(),
                        ResolveType::ClosureVar,
                        bytecode.get_put_info.initialization_mode(),
                        bytecode.get_put_info.ecma_mode(),
                    );
                } else {
                    let ident = this.borrow().identifier(bytecode.var as usize).clone();
                    let op = JSScope::abstract_resolve(
                        &global_object,
                        bytecode.local_scope_depth,
                        scope,
                        &ident,
                        GetOrPut::Get,
                        bytecode.get_put_info.resolve_type(),
                        InitializationMode::NotInitialization,
                    );

                    metadata.get_put_info = GetPutInfo::new(
                        bytecode.get_put_info.resolve_mode(),
                        op.type_,
                        bytecode.get_put_info.initialization_mode(),
                        bytecode.get_put_info.ecma_mode(),
                    );
                    if op.type_ == ResolveType::ModuleVar {
                        metadata.get_put_info = GetPutInfo::new(
                            bytecode.get_put_info.resolve_mode(),
                            ResolveType::ClosureVar,
                            bytecode.get_put_info.initialization_mode(),
                            bytecode.get_put_info.ecma_mode(),
                        );
                    }
                    if is_global_var_resolve_type(op.type_) {
                        metadata.watchpoint_set_or_structure_id =
                            op.watchpoint_set.as_ref().map_or(0, |watchpoint_set| watchpoint_set.identity() as u64);
                    } else if let Some(structure) = &op.structure {
                        metadata.watchpoint_set_or_structure_id = structure.id() as u64;
                    }
                    metadata.operand = op.operand as u64;
                }
                if let Some(table) = table {
                    table.borrow_mut().get_mut::<OpGetFromScopeMetadata>()[id] = metadata;
                }
            }

            OpcodeID::op_put_to_scope => {
                let bytecode = instruction.as_op::<OpPutToScope>();
                let mut metadata = OpPutToScopeMetadata::new(bytecode.get_put_info, bytecode.offset);
                let table = this.borrow().metadata.clone();

                if bytecode.get_put_info.resolve_type() == ResolveType::ResolvedClosureVar {
                    // Only do watching if the property we're putting to is not anonymous.
                    if bytecode.var != u32::MAX {
                        let symbol_table_register = bytecode.symbol_table_or_scope_depth.symbol_table_register();
                        let symbol_table = SymbolTable::from_cell_id(this.borrow().get_constant(symbol_table_register).as_cell())
                            .expect("op_put_to_scope: a constante não é um SymbolTable");
                        let ident = this.borrow().identifier(bytecode.var as usize);
                        let key = ident.impl_().expect("op_put_to_scope: identificador nulo");
                        // `ASSERT(iter != symbolTable->end(locker))`
                        debug_assert!(symbol_table.borrow().find(&key).is_some());
                        if bytecode.get_put_info.initialization_mode() == InitializationMode::ScopedArgumentInitialization {
                            debug_assert!(bytecode.value.is_argument());
                            // `symbolTable->prepareToWatchScopedArgument(iter->value, argumentIndex)` e
                            // `iter->value.prepareToWatch()`: o `SymbolTableEntry::prepareToWatch` do
                            // porte só infla entrada watchable, e nenhuma é (`is_watchable`).
                        }
                        // `metadata.m_watchpointSet = iter->value.watchpointSet()`: a entrada sem
                        // conjunto de watchpoints devolve nulo.
                        metadata.watchpoint_set_or_structure_id = 0;
                    } else {
                        metadata.watchpoint_set_or_structure_id = 0;
                    }
                } else {
                    let ident = this.borrow().identifier(bytecode.var as usize).clone();
                    metadata.watchpoint_set_or_structure_id = 0;
                    let op = JSScope::abstract_resolve(
                        &global_object,
                        bytecode.symbol_table_or_scope_depth.scope_depth_value(),
                        scope,
                        &ident,
                        GetOrPut::Put,
                        bytecode.get_put_info.resolve_type(),
                        bytecode.get_put_info.initialization_mode(),
                    );

                    metadata.get_put_info = GetPutInfo::new(
                        bytecode.get_put_info.resolve_mode(),
                        op.type_,
                        bytecode.get_put_info.initialization_mode(),
                        bytecode.get_put_info.ecma_mode(),
                    );
                    if is_global_var_resolve_type(op.type_) {
                        metadata.watchpoint_set_or_structure_id =
                            op.watchpoint_set.as_ref().map_or(0, |watchpoint_set| watchpoint_set.identity() as u64);
                    } else if op.type_ == ResolveType::ClosureVar || op.type_ == ResolveType::ClosureVarWithVarInjectionChecks {
                        if let Some(watchpoint_set) = &op.watchpoint_set {
                            watchpoint_set.invalidate_for_put_to_scope(vm, this, &ident);
                        }
                    } else if let Some(structure) = &op.structure {
                        metadata.watchpoint_set_or_structure_id = structure.id() as u64;
                    }
                    metadata.operand = op.operand as u64;
                }
                if let Some(table) = table {
                    table.borrow_mut().get_mut::<OpPutToScopeMetadata>()[id] = metadata;
                }
            }

            OpcodeID::op_debug => {
                use crate::bytecode::bytecode_ops::OpDebug;
                use crate::interpreter::interpreter::DebugHookType;
                if instruction.as_op::<OpDebug>().debug_hook_type == DebugHookType::DidReachDebuggerStatement {
                    this.borrow_mut().has_debugger_statement = true;
                }
            }

            OpcodeID::op_create_rest => {
                let number_of_arguments_to_skip = instruction.as_op::<OpCreateRest>().num_parameters_to_skip;
                // This is used when rematerializing the rest parameter during OSR exit in the FTL JIT.
                this.borrow_mut().number_of_arguments_to_skip = number_of_arguments_to_skip;
            }

            _ => {}
        }
        let _ = unlinked_code_block;
    }

    /// `setConstantRegisters(constants, constantsSourceCodeRepresentation)`: devolve os índices
    /// das constantes que são `JSTemplateObjectDescriptor`.
    fn set_constant_registers(
        &mut self,
        vm: &VM,
        constants: &[JSValue],
        constants_source_code_representation: &[SourceCodeRepresentation],
    ) -> Vec<u32> {
        let global_object = Rc::clone(&self.global_object);
        let mut template_object_indices = Vec::new();

        debug_assert!(constants.len() == constants_source_code_representation.len());
        let count = constants.len();
        self.constant_registers = Vec::with_capacity(count);
        for i in 0..count {
            let mut constant = constants[i];
            let representation = constants_source_code_representation[i];
            match representation {
                SourceCodeRepresentation::LinkTimeConstant => {
                    constant = global_object
                        .link_time_constant(LINK_TIME_CONSTANT_TABLE[constant.as_int32() as usize].1);
                    debug_assert!(constant.is_cell()); // Unlinked Baseline JIT requires this.
                }
                SourceCodeRepresentation::Other | SourceCodeRepresentation::Integer | SourceCodeRepresentation::Double => {
                    if !constant.is_empty() && constant.is_cell() {
                        let cell_id = constant.as_cell();
                        if let Some(symbol_table) = SymbolTable::from_cell_id(cell_id) {
                            // We have to make sure to use a single code block for constant watchpointing.
                            // If we didn't then we could jettison a compilation because that constant changed
                            // but invalidate the clone. Then the next compilation would see the original
                            // watchpoint intact and assume the value is still the original constant.
                            let cached = global_object.symbol_table_cache_get(&symbol_table);
                            let clone = match cached {
                                Some(clone) => clone,
                                None => {
                                    // For non-builtin code, link the clone's singleton watchpoint to the master
                                    // SymbolTable held inside the UnlinkedCodeBlock so that all per-realm clones
                                    // share one InferredValue.
                                    let propagate = if self.unlinked_code.borrow().is_builtin_function() {
                                        PropagateCloneInvalidationToOriginal::No
                                    } else {
                                        PropagateCloneInvalidationToOriginal::Yes
                                    };
                                    let clone = symbol_table.borrow().clone_scope_part(vm, propagate);
                                    global_object.symbol_table_cache_set(&symbol_table, &clone);
                                    clone
                                }
                            };
                            let clone: SymbolTableRef = clone;
                            constant = JSValue::from_cell(clone.borrow().cell_id());
                        } else if JSTemplateObjectDescriptor::from_cell_id(cell_id).is_some() {
                            template_object_indices.push(i as u32);
                        }
                    }
                }
            }
            self.constant_registers.push(constant);
        }

        template_object_indices
    }

    /// `initializeTemplateObjects(topLevelExecutable, templateObjectIndices)`: `false` quando
    /// `createTemplateObject` lança (a exceção fica pendente no `VM`).
    fn initialize_template_objects(
        this: &Rc<RefCell<CodeBlock>>,
        vm: &VM,
        top_level_executable: &ScriptExecutableRef,
        template_object_indices: &[u32],
    ) -> bool {
        let global_object = Rc::clone(&this.borrow().global_object);
        for &i in template_object_indices {
            let descriptor_cell = this.borrow().constant_registers[i as usize].as_cell();
            let Some(descriptor) = JSTemplateObjectDescriptor::from_cell_id(descriptor_cell) else {
                unreachable!("uncheckedDowncast<JSTemplateObjectDescriptor>");
            };
            let template_object = top_level_executable.create_template_object(&global_object, &descriptor);
            // `RETURN_IF_EXCEPTION(scope, void())` e `RETURN_IF_EXCEPTION(throwScope, false)`.
            let Some(template_object) = template_object else {
                return false;
            };
            this.borrow_mut().constant_registers[i as usize] = JSValue::from_cell(template_object.cell_id());
        }
        true
    }

    /// `setNumParameters(newValue, allocateArgumentValueProfiles)`.
    pub fn set_num_parameters(&mut self, new_value: u32, allocate_argument_value_profiles: bool) {
        self.num_parameters = new_value;
        let count = if Options::use_jit() && allocate_argument_value_profiles { new_value } else { 0 };
        self.argument_value_profiles = vec![ArgumentValueProfile::default(); count as usize];
    }

    /// `isConstantOwnedByUnlinkedCodeBlock(reg)`.
    pub fn is_constant_owned_by_unlinked_code_block(&self, reg: VirtualRegister) -> bool {
        // This needs to correspond to what we do inside setConstantRegisters.
        let unlinked = self.unlinked_code.borrow();
        match unlinked.constant_source_code_representation(reg) {
            SourceCodeRepresentation::Integer | SourceCodeRepresentation::Double => true,
            SourceCodeRepresentation::Other => {
                let value = unlinked.constant_register(reg);
                if value.is_empty() || !value.is_cell() {
                    return true;
                }
                let cell_id = value.as_cell();
                if SymbolTable::from_cell_id(cell_id).is_some() || JSTemplateObjectDescriptor::from_cell_id(cell_id).is_some() {
                    return false;
                }
                true
            }
            SourceCodeRepresentation::LinkTimeConstant => false,
        }
    }

    /// `createRareDataIfNecessary()`.
    /// `directEvalCodeCache()`.
    pub fn direct_eval_code_cache(&mut self) -> &mut crate::bytecode::direct_eval_code_cache::DirectEvalCodeCache {
        self.create_rare_data_if_necessary();
        &mut self.rare_data.as_mut().expect("rare data recém-criada").direct_eval_code_cache
    }

    fn create_rare_data_if_necessary(&mut self) {
        if self.rare_data.is_none() {
            self.rare_data = Some(Box::default());
        }
    }

    /// `unlinkedCodeBlock()`.
    pub fn unlinked_code_block(&self) -> &UnlinkedCodeBlockRef {
        &self.unlinked_code
    }

    /// `id` na arena (o `this` do C++ como `CodeBlock*`).
    pub fn id(&self) -> CodeBlockId {
        self.id
    }

    /// `metadataTable()`.
    pub fn metadata_table(&self) -> Option<&MetadataTableRef> {
        self.metadata.as_ref()
    }

    /// `metadata<Metadata>(opcodeID, metadataID)`: roda `f` sobre a entrada.
    pub fn with_metadata<M: MetadataFor, R>(&self, metadata_id: u32, f: impl FnOnce(&mut M) -> R) -> R {
        let table = self.metadata.as_ref().expect("ASSERT(m_metadata)");
        let mut table = table.borrow_mut();
        f(&mut table.get_mut::<M>()[metadata_id as usize])
    }

    /// `CodeBlock::finalizeUnconditionally`, que o coletor chama depois da marcação e antes da varredura:
    /// `updateAllArrayAllocationProfilePredictions` (o perfil dobra o array que viu por último e solta a
    /// referência fraca a ele) e `finalizeLLIntInlineCaches` (o id de célula que vai ser varrida sai do cache:
    /// o callee ligado de um `CallLinkInfo`, que volta a `Init` com `clearedByGC`, e o escopo ou a tabela de
    /// símbolos do `op_resolve_scope`). `doomed` diz se o id é de célula que a varredura vai soltar. Devolve
    /// `false` se a tabela estiver emprestada.
    pub(crate) fn finalize_unconditionally(&self, doomed: &dyn Fn(usize) -> bool) -> bool {
        use crate::bytecode::array_allocation_profile::HeapArrays;
        let Some(table) = self.metadata.as_ref() else {
            return true;
        };
        let Ok(mut table) = table.try_borrow_mut() else {
            return false;
        };
        table.for_each::<OpNewArrayMetadata>(|m| m.array_allocation_profile.update_profile(&HeapArrays));
        table.for_each::<OpNewArrayWithSizeMetadata>(|m| m.array_allocation_profile.update_profile(&HeapArrays));
        table.for_each::<OpNewArrayBufferMetadata>(|m| m.array_allocation_profile.update_profile(&HeapArrays));
        table.for_each::<OpNewArrayWithSpeciesMetadata>(|m| m.array_allocation_profile.update_profile(&HeapArrays));
        let unlink_dead_callee = |info: &mut DataOnlyCallLinkInfo| {
            if info.callee != 0 && doomed(info.callee as usize) {
                info.callee = 0;
                info.cleared_by_gc = true;
                info.mode = CallLinkInfoMode::Init as u8;
            }
        };
        table.for_each::<OpTailCallMetadata>(|m| unlink_dead_callee(&mut m.call_link_info));
        table.for_each::<OpTailCallVarargsMetadata>(|m| unlink_dead_callee(&mut m.call_link_info));
        table.for_each::<OpResolveScopeMetadata>(|m| {
            let scope = &mut m.lexical_environment_or_symbol_table_or_constant_scope_or_global_object;
            if *scope != 0 && doomed(*scope as usize) {
                *scope = 0;
            }
        });
        true
    }

    /// `offsetInMetadataTable(Metadata*)`, dado o opcode e o `metadataID`.
    pub fn offset_in_metadata_table(&self, opcode_id: OpcodeID, metadata_id: u32) -> usize {
        self.metadata.as_ref().expect("ASSERT(m_metadata)").borrow().offset_in_metadata_table(opcode_id, metadata_id)
    }

    /// `numParameters()`.
    pub fn num_parameters(&self) -> u32 {
        self.num_parameters
    }

    /// `numberOfArgumentsToSkip()`.
    pub fn number_of_arguments_to_skip(&self) -> u32 {
        self.number_of_arguments_to_skip
    }

    /// `couldBeTainted()`.
    pub fn could_be_tainted(&self) -> bool {
        self.could_be_tainted
    }

    /// `numCalleeLocals()`.
    pub fn num_callee_locals(&self) -> u32 {
        self.num_callee_locals
    }

    /// `numVars()`.
    pub fn num_vars(&self) -> u32 {
        self.num_vars
    }

    /// `isTemporaryRegister(reg)`.
    pub fn is_temporary_register(&self, reg: VirtualRegister) -> bool {
        reg.offset() >= self.num_vars as i32
    }

    /// `hasDebuggerStatement`.
    pub fn has_debugger_statement(&self) -> bool {
        self.has_debugger_statement
    }

    /// `hasDebuggerRequests()`: `m_debuggerRequests` junta os três campos.
    pub fn has_debugger_requests(&self) -> bool {
        self.has_debugger_statement || self.stepping_mode_enabled || self.num_breakpoints != 0
    }

    /// `isConstructor()`.
    pub fn is_constructor(&self) -> bool {
        self.unlinked_code.borrow().is_constructor()
    }

    /// `specializationKind()`.
    pub fn specialization_kind(&self) -> CodeSpecializationKind {
        specialization_from_is_construct(self.is_constructor())
    }

    /// `codeType()`.
    pub fn code_type(&self) -> CodeType {
        self.unlinked_code.borrow().code_type()
    }

    /// `scriptMode()`.
    pub fn script_mode(&self) -> JSParserScriptMode {
        self.unlinked_code.borrow().script_mode()
    }

    /// `thisRegister()`.
    pub fn this_register(&self) -> VirtualRegister {
        self.unlinked_code.borrow().this_register()
    }

    /// `setScopeRegister(scopeRegister)`.
    pub fn set_scope_register(&mut self, scope_register: VirtualRegister) {
        debug_assert!(scope_register.is_local() || !scope_register.is_valid());
        self.scope_register = scope_register;
    }

    /// `scopeRegister()`.
    pub fn scope_register(&self) -> VirtualRegister {
        self.scope_register
    }

    /// `instructions()`: o fluxo é o do `UnlinkedCodeBlock`.
    pub fn instructions(&self) -> Ref<'_, JSInstructionStream> {
        Ref::map(self.unlinked_code.borrow(), |unlinked| unlinked.instructions())
    }

    /// `instructionAt(BytecodeIndex)`.
    pub fn instruction_at(&self, index: BytecodeIndex) -> InstructionRef {
        self.instructions().at(index.offset())
    }

    /// `instructionsSize()`.
    pub fn instructions_size(&self) -> u32 {
        self.instructions().size_in_bytes() as u32
    }

    /// `bytecodeCost()`.
    pub fn bytecode_cost(&self) -> u32 {
        self.bytecode_cost
    }

    /// `ownerExecutable()`.
    pub fn owner_executable(&self) -> &ScriptExecutableRef {
        &self.owner_executable
    }

    /// `source()`.
    pub fn source(&self) -> SourceCode {
        self.owner_executable.source().clone()
    }

    /// `CodeBlock::expressionInfoForBytecodeIndex`: a entrada do `UnlinkedCodeBlock` tem o divot relativo ao
    /// início do fonte do bloco, e aqui ele volta a ser absoluto no provider (`divot += sourceOffset()`).
    pub fn expression_info_for_bytecode_index(
        &self,
        bytecode_index: BytecodeIndex,
    ) -> crate::bytecode::expression_info::Entry {
        let mut entry = self.unlinked_code_block().borrow().expression_info_for_bytecode_index(bytecode_index);
        entry.divot += self.source_offset();
        entry
    }

    /// `sourceOffset()`.
    pub fn source_offset(&self) -> u32 {
        self.owner_executable.source().start_offset() as u32
    }

    /// `firstLineColumnOffset()`.
    pub fn first_line_column_offset(&self) -> u32 {
        self.owner_executable.start_column()
    }

    /// `globalObject()`.
    pub fn global_object(&self) -> &JSGlobalObjectRef {
        &self.global_object
    }

    /// `numberOfArgumentValueProfiles()`.
    pub fn number_of_argument_value_profiles(&self) -> usize {
        self.argument_value_profiles.len()
    }

    /// `valueProfileForArgument(argumentIndex)`.
    pub fn value_profile_for_argument(&mut self, argument_index: usize) -> &mut ArgumentValueProfile {
        &mut self.argument_value_profiles[argument_index]
    }

    /// `argumentValueProfiles()`.
    pub fn argument_value_profiles(&mut self) -> &mut Vec<ArgumentValueProfile> {
        &mut self.argument_value_profiles
    }

    /// `valueProfileForOffset(profileOffset)`: roda `f` sobre o perfil (`m_metadata->valueProfileForOffset`).
    pub fn with_value_profile_for_offset<R>(&self, profile_offset: u32, f: impl FnOnce(&mut ValueProfile) -> R) -> R {
        let table = self.metadata.as_ref().expect("ASSERT(m_metadata)");
        f(table.borrow_mut().value_profile_for_offset(profile_offset))
    }

    /// `totalNumberOfValueProfiles()`.
    pub fn total_number_of_value_profiles(&self) -> u32 {
        self.unlinked_code.borrow().number_of_value_profiles()
    }

    /// `handlerForBytecodeIndex(BytecodeIndex, RequiredHandler)`.
    pub fn handler_for_bytecode_index(&self, bytecode_index: BytecodeIndex, required_handler: RequiredHandler) -> Option<&HandlerInfo> {
        assert!(bytecode_index.offset() < self.instructions().size_in_bytes() as u32);
        self.handler_for_index(bytecode_index.offset(), required_handler)
    }

    /// `handlerForIndex(unsigned, RequiredHandler)`.
    pub fn handler_for_index(&self, index: u32, required_handler: RequiredHandler) -> Option<&HandlerInfo> {
        let rare_data = self.rare_data.as_ref()?;
        crate::bytecode::handler_info::HandlerInfoBase::handler_for_index(
            rare_data.exception_handlers.iter(),
            index,
            required_handler,
        )
    }

    /// `numberOfExceptionHandlers()`.
    pub fn number_of_exception_handlers(&self) -> usize {
        self.rare_data.as_ref().map_or(0, |rare_data| rare_data.exception_handlers.len())
    }

    /// `exceptionHandler(index)`.
    pub fn exception_handler(&mut self, index: usize) -> &mut HandlerInfo {
        let rare_data = self.rare_data.as_mut().expect("RELEASE_ASSERT(m_rareData)");
        &mut rare_data.exception_handlers[index]
    }

    /// `clearExceptionHandlers()`.
    pub fn clear_exception_handlers(&mut self) {
        if let Some(rare_data) = self.rare_data.as_mut() {
            rare_data.exception_handlers.clear();
        }
    }

    /// `appendExceptionHandler(handler)`.
    pub fn append_exception_handler(&mut self, handler: HandlerInfo) {
        self.create_rare_data_if_necessary(); // We may be handling the exception of an inlined call frame.
        if let Some(rare_data) = self.rare_data.as_mut() {
            rare_data.exception_handlers.push(handler);
        }
    }

    /// `hasTailCalls()`.
    pub fn has_tail_calls(&self) -> bool {
        self.unlinked_code.borrow().has_tail_calls()
    }

    /// `numberOfIdentifiers()`.
    pub fn number_of_identifiers(&self) -> usize {
        self.unlinked_code.borrow().number_of_identifiers()
    }

    /// `identifier(index)`: o `Identifier` é um valor pequeno (átomo), então a cópia equivale à
    /// referência do C++.
    pub fn identifier(&self, index: usize) -> Identifier {
        self.unlinked_code.borrow().identifier(index).clone()
    }

    /// `constants()`/`constantRegisters()`.
    pub fn constant_registers(&self) -> &[JSValue] {
        &self.constant_registers
    }

    /// `constantRegister(reg)` e `getConstant(reg)`.
    pub fn get_constant(&self, reg: VirtualRegister) -> JSValue {
        self.constant_registers[reg.to_constant_index() as usize]
    }

    /// `addConstant(locker, value)`.
    pub fn add_constant(&mut self, value: JSValue) -> u32 {
        let result = self.constant_registers.len() as u32;
        self.constant_registers.push(value);
        result
    }

    /// `addConstantLazily(locker)`.
    pub fn add_constant_lazily(&mut self) -> u32 {
        let result = self.constant_registers.len() as u32;
        self.constant_registers.push(JSValue::empty());
        result
    }

    /// `replaceConstant(reg, value)`.
    pub fn replace_constant(&mut self, reg: VirtualRegister, value: JSValue) {
        debug_assert!(reg.is_constant() && (reg.to_constant_index() as usize) < self.constant_registers.len());
        self.constant_registers[reg.to_constant_index() as usize] = value;
    }

    /// `functionDecl(index)`.
    pub fn function_decl(&self, index: usize) -> &FunctionExecutableRef {
        &self.function_decls[index]
    }

    /// `numberOfFunctionDecls()`.
    pub fn number_of_function_decls(&self) -> usize {
        self.function_decls.len()
    }

    /// `functionDecls()`.
    pub fn function_decls(&self) -> &[FunctionExecutableRef] {
        &self.function_decls
    }

    /// `functionExpr(index)`.
    pub fn function_expr(&self, index: usize) -> &FunctionExecutableRef {
        &self.function_exprs[index]
    }

    /// `numberOfFunctionExprs()`.
    pub fn number_of_function_exprs(&self) -> usize {
        self.function_exprs.len()
    }

    /// `unlinkedSwitchJumpTable(tableIndex)`: roda `f` sobre a tabela do `UnlinkedCodeBlock`.
    pub fn with_unlinked_switch_jump_table<R>(&self, table_index: usize, f: impl FnOnce(&UnlinkedSimpleJumpTable) -> R) -> R {
        f(self.unlinked_code.borrow().unlinked_switch_jump_table(table_index))
    }

    /// `unlinkedStringSwitchJumpTable(tableIndex)`.
    pub fn with_unlinked_string_switch_jump_table<R>(&self, table_index: usize, f: impl FnOnce(&UnlinkedStringJumpTable) -> R) -> R {
        f(self.unlinked_code.borrow().unlinked_string_switch_jump_table(table_index))
    }
}

/// `ResolveType` global com `InlineWatchpointSet`: o teste que `op_get_from_scope` e
/// `op_put_to_scope` repetem (`GlobalVar`, `GlobalVarWithVarInjectionChecks`, `GlobalLexicalVar`,
/// `GlobalLexicalVarWithVarInjectionChecks`).
fn is_global_var_resolve_type(resolve_type: ResolveType) -> bool {
    matches!(
        resolve_type,
        ResolveType::GlobalVar
            | ResolveType::GlobalVarWithVarInjectionChecks
            | ResolveType::GlobalLexicalVar
            | ResolveType::GlobalLexicalVarWithVarInjectionChecks
    )
}

/// `instantiatedModuleFunctionExecutable(moduleEnvironment, topLevelExecutable, unlinkedExecutable)`.
fn instantiated_module_function_executable(
    module_environment: &crate::runtime::js_module_environment::JSModuleEnvironmentRef,
    top_level_executable: &ScriptExecutableRef,
    unlinked_executable: &crate::bytecode::unlinked_function_executable::UnlinkedFunctionExecutableRef,
) -> Option<FunctionExecutableRef> {
    let key = unlinked_executable.borrow().name().impl_()?;
    let entry = module_environment.symbol_table().borrow().get(&key);
    if entry.is_null() {
        return None;
    }
    let function = module_environment.variable_at(entry.scope_offset()).as_js_function()?;
    let executable = function.executable().as_function_executable()?;
    if !Rc::ptr_eq(executable.borrow().unlinked_executable(), unlinked_executable)
        || !ScriptExecutableRef::Function(Rc::clone(&executable)).top_level_executable().ptr_eq(top_level_executable)
    {
        return None;
    }
    Some(executable)
}

/// `sizeof(Register)`.
const SIZEOF_REGISTER: usize = 8;

/// `sizeof(CPURegister)` em x86_64.
const SIZEOF_CPU_REGISTER: usize = 8;

/// `RegisterSet::llintBaselineCalleeSaveRegisters().numberOfSetRegisters()` em x86_64.
const LLINT_BASELINE_CALLEE_SAVE_REGISTER_COUNT: usize = 4;

/// `CodeBlock::numberOfLLIntBaselineCalleeSaveRegisters()`.
pub fn number_of_llint_baseline_callee_save_registers() -> u32 {
    LLINT_BASELINE_CALLEE_SAVE_REGISTER_COUNT as u32
}

/// `WTF::roundUpToMultipleOf<divisor>(x)`.
fn round_up_to_multiple_of(divisor: usize, x: usize) -> usize {
    x.div_ceil(divisor) * divisor
}

/// `CodeBlock::llintBaselineCalleeSaveSpaceAsVirtualRegisters()`. O C++ devolve `size_t`; o
/// `BytecodeGeneratorBase` recebe `uint32_t virtualRegisterCountForCalleeSaves`, por isso `u32`.
/// Em x86_64 vale 4 (`AssertInvariants.cpp`).
pub fn llint_baseline_callee_save_space_as_virtual_registers() -> u32 {
    let bytes = number_of_llint_baseline_callee_save_registers() as usize * SIZEOF_CPU_REGISTER;
    (round_up_to_multiple_of(SIZEOF_REGISTER, bytes) / SIZEOF_REGISTER) as u32
}

impl crate::bytecode::precise_jump_targets::JumpTargetBlock for CodeBlock {
    fn jump_offset_out_of_line(&self, bytecode_offset: u32) -> i32 {
        self.unlinked_code.borrow().out_of_line_jump_offset(bytecode_offset)
    }

    /// As tabelas são do `UnlinkedCodeBlock`, que o fluxo de instruções mantém emprestado enquanto o
    /// grafo é montado: a `function` recebe uma cópia, e só a leitura (o único uso sobre `CodeBlock`)
    /// é observável.
    fn with_switch_jump_table(
        &mut self,
        table_index: usize,
        function: &mut dyn FnMut(&mut crate::bytecode::unlinked_code_block::UnlinkedSimpleJumpTable),
    ) {
        let mut table = self.unlinked_code.borrow().unlinked_switch_jump_table(table_index).clone();
        function(&mut table);
    }

    fn with_string_switch_jump_table(
        &mut self,
        table_index: usize,
        function: &mut dyn FnMut(&mut crate::bytecode::unlinked_code_block::UnlinkedStringJumpTable),
    ) {
        let mut table = self.unlinked_code.borrow().unlinked_string_switch_jump_table(table_index).clone();
        function(&mut table);
    }

    fn exception_handler_count(&self) -> usize {
        self.number_of_exception_handlers()
    }

    fn exception_handler_base(&mut self, index: usize) -> crate::bytecode::handler_info::HandlerInfoBase {
        self.exception_handler(index).base
    }

    fn any_handler_target(&self, bytecode_offset: u32) -> Option<u32> {
        self.handler_for_bytecode_index(BytecodeIndex::from_offset(bytecode_offset), RequiredHandler::AnyHandler)
            .map(|handler| handler.base.target)
    }
}
