//! Tradução de `runtime/ScriptExecutable.h`, `ScriptExecutable.cpp` e `ScriptExecutableInlines.h`,
//! mais o `prepareForExecution<ExecutableType>` que o C++ define em `bytecode/CodeBlock.h`.
//!
//! DIVERGÊNCIAS (ver também `executable.rs`):
//!
//! - `ScriptExecutable*` é o enum `ScriptExecutableRef` (as quatro classes concretas); o que o C++
//!   faz por `type()`/`classInfo()` + `uncheckedDowncast` vira `match`. A struct `ScriptExecutable`
//!   guarda só o estado da base. Os métodos que a base não consegue resolver sozinha
//!   (`installCode`, `newCodeBlockFor`, `prepareForExecution`, `clearCode`, `hasClearableCode`,
//!   `recordParse` de cinco argumentos, `lastLine`, `endColumn`, `topLevelExecutable`,
//!   `ensureTemplateObjectMap`, `createTemplateObject`, `overrideLineNumber`,
//!   `typeProfiling{Start,End}Offset`, `dump`) são do enum.
//! - Sem heap, sem GC e sem JIT: `runConstraint`, `visitCodeBlockEdge`, `jettisonCodeBlockEdgeIfDead`
//!   (coleta), os conjuntos `Heap::ScriptExecutableSpaceAndSets` (o `IsoCellSet& clearableCodeSet`
//!   de `clearCode`), `DeferGCForAWhile`, `CODEBLOCK_LOG_EVENT` e o `Profiler::JettisonReason` de
//!   `installCode` (só serve ao ramo `isMarked` do GC) não existem. `installCode(VM&, CodeBlock*,
//!   CodeType, CodeSpecializationKind, JettisonReason)` é `install_code_for_kind` sem o último
//!   argumento. `vm.m_perBytecodeProfiler` só existe com `useProfiler` (desligada, sem efeito
//!   observável) e o ramo some. `ENABLE(JIT)` vale o `#else`: não há `m_unlinkedBaselineCode` e
//!   `setupJIT` é `UNREACHABLE_FOR_PLATFORM()`.
//! - `TemplateObjectMap` é `HashMap<u64, Option<JSArrayRef>>` (o valor nulo é o `WriteBarrier`
//!   vazio que `add` insere antes de criar o array); o `cellLock()` some (uma thread).
//! - Bitfields viram campos do tipo próprio, com o mesmo conjunto de valores.

use std::cell::RefCell;
use std::collections::HashMap;
use std::fmt;
use std::ops::{Deref, DerefMut};
use std::rc::Rc;

use crate::bytecode::code_block::{CodeBlock, CodeBlockRef};
use crate::bytecode::code_block_hash::CodeBlockHash;
use crate::bytecode::code_type::CodeType;
use crate::bytecode::eval_code_block::EvalCodeBlock;
use crate::bytecode::executable_info::{DerivedContextType, EvalContextType};
use crate::bytecode::function_code_block::FunctionCodeBlock;
use crate::bytecode::module_program_code_block::ModuleProgramCodeBlock;
use crate::bytecode::program_code_block::ProgramCodeBlock;
use crate::llint::llint_entrypoint;
use crate::parser::parser_error::ParserError;
use crate::parser::parser_modes::{
    is_generator_or_async_function_body_parse_mode, CodeFeatures, CodeGenerationMode, LexicallyScopedFeatures,
    ARGUMENTS_FEATURE, NON_SIMPLE_PARAMETER_LIST_FEATURE, NO_FEATURES, STRICT_MODE_LEXICALLY_SCOPED_FEATURE,
    TAINTED_BY_WITH_SCOPE_LEXICALLY_SCOPED_FEATURE,
};
use crate::parser::source_code::SourceCode;
use crate::parser::source_provider::{SourceID, SourceProvider};
use crate::runtime::code_specialization_kind::CodeSpecializationKind;
use crate::runtime::eval_executable::EvalExecutable;
use crate::runtime::executable::ExecutableBase;
use crate::runtime::function_executable::FunctionExecutable;
use crate::runtime::intrinsic::Intrinsic;
use crate::runtime::js_object::JSObjectRef;
use crate::runtime::js_function::JSFunction;
use crate::runtime::js_global_object::JSGlobalObject;
use crate::runtime::js_scope::JSScopeRef;
use crate::runtime::js_template_object_descriptor::JSTemplateObjectDescriptor;
use crate::runtime::js_type::JSType;
use crate::runtime::module_program_executable::ModuleProgramExecutable;
use crate::runtime::options::Options;
use crate::runtime::program_executable::ProgramExecutable;
use crate::runtime::source_origin::SourceOrigin;
use crate::runtime::symbol_table::SymbolTableRef;
use crate::runtime::throw_scope::{throw_exception, ThrowScope};
use crate::runtime::vm::VM;
use crate::wtf::option_set::OptionSet;
use crate::wtf::text::wtf_string::String as WtfString;

/// `JSArray*`. DIVERGÊNCIA: o `JSArray` (butterfly com `IndexingType`, `ArrayStorage`) ainda não foi
/// portado; o array da template é um `JSObject` com estrutura de array, então o `JSArrayRef` é o
/// `JSObjectRef` até a `JSArray` ter tipo próprio.
pub type JSArrayRef = crate::runtime::js_array::JSArray;

/// `ScriptExecutable::TemplateObjectMap`.
pub type TemplateObjectMap = HashMap<u64, Option<JSArrayRef>>;

/// `class ScriptExecutable`: o estado compartilhado por Program, Eval, ModuleProgram e Function.
pub struct ScriptExecutable {
    base: ExecutableBase,
    pub(crate) source: SourceCode,
    pub(crate) intrinsic: Intrinsic,
    pub(crate) did_try_to_enter_in_loop: bool,
    pub(crate) features: CodeFeatures,
    pub(crate) lexically_scoped_features: LexicallyScopedFeatures,
    pub(crate) code_generation_mode_for_generator_body: OptionSet<CodeGenerationMode>,
    pub(crate) has_captured_variables: bool,
    pub(crate) never_inline: bool,
    pub(crate) never_optimize: bool,
    pub(crate) never_ftl_optimize: bool,
    pub(crate) is_arrow_function_context: bool,
    pub(crate) can_use_osr_exit_fuzzing: bool,
    pub(crate) code_for_generator_body_was_generated: bool,
    pub(crate) is_inside_ordinary_function: bool,
    pub(crate) derived_context_type: DerivedContextType,
    pub(crate) eval_context_type: EvalContextType,
}

crate::parser::nodes::inherit!(ScriptExecutable => ExecutableBase);

impl ScriptExecutable {
    /// `ScriptExecutable(Structure*, VM&, const SourceCode&, LexicallyScopedFeatures, DerivedContextType,
    /// bool isInArrowFunctionContext, bool isInsideOrdinaryFunction, EvalContextType, Intrinsic)`.
    /// `cell_type` é o tipo que a `Structure` passada ao construtor carrega.
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        cell_type: JSType,
        source: &SourceCode,
        lexically_scoped_features: LexicallyScopedFeatures,
        derived_context_type: DerivedContextType,
        is_in_arrow_function_context: bool,
        is_inside_ordinary_function: bool,
        eval_context_type: EvalContextType,
        intrinsic: Intrinsic,
    ) -> ScriptExecutable {
        ScriptExecutable {
            base: ExecutableBase::new(cell_type),
            source: source.clone(),
            intrinsic,
            did_try_to_enter_in_loop: false,
            features: NO_FEATURES,
            lexically_scoped_features,
            code_generation_mode_for_generator_body: OptionSet::default(),
            has_captured_variables: false,
            never_inline: false,
            never_optimize: false,
            never_ftl_optimize: false,
            is_arrow_function_context: is_in_arrow_function_context,
            can_use_osr_exit_fuzzing: true,
            code_for_generator_body_was_generated: false,
            is_inside_ordinary_function,
            derived_context_type,
            eval_context_type,
        }
    }

    /// `m_source.provider()`: o C++ a desreferencia sem checar.
    fn provider(&self) -> &Rc<dyn SourceProvider> {
        self.source.provider().expect("ScriptExecutable sem SourceProvider")
    }

    /// `source()`.
    pub fn source(&self) -> &SourceCode {
        &self.source
    }

    /// `sourceID()`.
    pub fn source_id(&self) -> SourceID {
        self.source.provider_id()
    }

    /// `sourceOrigin()`.
    pub fn source_origin(&self) -> &SourceOrigin {
        self.provider().source_origin()
    }

    /// `sourceURL()`. This is NOT the path that should be used for computing relative paths from a
    /// script. Use SourceOrigin's URL for that, the values may or may not be the same... This should
    /// only be used for `error.sourceURL` and stack traces.
    pub fn source_url(&self) -> &WtfString {
        self.provider().source_url()
    }

    /// `sourceURLStripped()`.
    pub fn source_url_stripped(&self) -> WtfString {
        self.provider().source_url_stripped()
    }

    /// `preRedirectURL()`.
    pub fn pre_redirect_url(&self) -> &WtfString {
        self.provider().pre_redirect_url()
    }

    /// `firstLine()`.
    pub fn first_line(&self) -> i32 {
        self.source.first_line().one_based_int()
    }

    /// `startColumn()`.
    pub fn start_column(&self) -> u32 {
        self.source.start_column().one_based_int() as u32
    }

    pub fn uses_arguments(&self) -> bool {
        self.features & ARGUMENTS_FEATURE != 0
    }

    pub fn is_arrow_function_context(&self) -> bool {
        self.is_arrow_function_context
    }

    pub fn derived_context_type(&self) -> DerivedContextType {
        self.derived_context_type
    }

    pub fn eval_context_type(&self) -> EvalContextType {
        self.eval_context_type
    }

    pub fn is_in_strict_context(&self) -> bool {
        self.lexically_scoped_features & STRICT_MODE_LEXICALLY_SCOPED_FEATURE != 0
    }

    pub fn uses_non_simple_parameter_list(&self) -> bool {
        self.features & NON_SIMPLE_PARAMETER_LIST_FEATURE != 0
    }

    pub fn set_never_inline(&mut self, value: bool) {
        self.never_inline = value;
    }

    pub fn set_never_optimize(&mut self, value: bool) {
        self.never_optimize = value;
    }

    pub fn set_never_ftl_optimize(&mut self, value: bool) {
        self.never_ftl_optimize = value;
    }

    pub fn set_did_try_to_enter_in_loop(&mut self, value: bool) {
        self.did_try_to_enter_in_loop = value;
    }

    pub fn set_can_use_osr_exit_fuzzing(&mut self, value: bool) {
        self.can_use_osr_exit_fuzzing = value;
    }

    pub fn never_inline(&self) -> bool {
        self.never_inline
    }

    pub fn never_optimize(&self) -> bool {
        self.never_optimize
    }

    pub fn never_ftl_optimize(&self) -> bool {
        self.never_ftl_optimize
    }

    pub fn did_try_to_enter_in_loop(&self) -> bool {
        self.did_try_to_enter_in_loop
    }

    pub fn is_inlining_candidate(&self) -> bool {
        !self.never_inline()
    }

    pub fn is_ok_to_optimize(&self) -> bool {
        !self.never_optimize()
    }

    pub fn can_use_osr_exit_fuzzing(&self) -> bool {
        self.can_use_osr_exit_fuzzing
    }

    pub fn is_inside_ordinary_function(&self) -> bool {
        self.is_inside_ordinary_function
    }

    /// `features()`.
    pub fn features(&self) -> CodeFeatures {
        self.features
    }

    /// `lexicallyScopedFeatures()`.
    pub fn lexically_scoped_features(&self) -> LexicallyScopedFeatures {
        self.lexically_scoped_features
    }

    pub fn set_tainted_by_with_scope(&mut self) {
        self.lexically_scoped_features |= TAINTED_BY_WITH_SCOPE_LEXICALLY_SCOPED_FEATURE;
    }

    /// `intrinsic()`.
    pub fn intrinsic(&self) -> Intrinsic {
        self.intrinsic
    }

    /// `hasJITCodeForCall()`.
    pub fn has_jit_code_for_call(&self) -> bool {
        self.jit_code_for_call.is_some()
    }

    /// `hasJITCodeForConstruct()`.
    pub fn has_jit_code_for_construct(&self) -> bool {
        self.jit_code_for_construct.is_some()
    }

    /// `hasJITCodeFor(CodeSpecializationKind)`.
    pub fn has_jit_code_for(&self, kind: CodeSpecializationKind) -> bool {
        match kind {
            CodeSpecializationKind::CodeForCall => self.has_jit_code_for_call(),
            CodeSpecializationKind::CodeForConstruct => self.has_jit_code_for_construct(),
        }
    }

    /// `hashFor(CodeSpecializationKind)`.
    pub fn hash_for(&self, kind: CodeSpecializationKind) -> CodeBlockHash {
        CodeBlockHash::from_source_code(self.source(), kind)
    }

    /// `recordParse(CodeFeatures, LexicallyScopedFeatures, bool hasCapturedVariables)` (protegido).
    pub(crate) fn record_parse(
        &mut self,
        features: CodeFeatures,
        lexically_scoped_features: LexicallyScopedFeatures,
        has_captured_variables: bool,
    ) {
        self.features = features;
        self.lexically_scoped_features = lexically_scoped_features;
        self.has_captured_variables = has_captured_variables;
    }

    /// `ensureTemplateObjectMapImpl(std::unique_ptr<TemplateObjectMap>& dest)`.
    pub(crate) fn ensure_template_object_map_impl(dest: &mut Option<Box<TemplateObjectMap>>) -> &mut TemplateObjectMap {
        dest.get_or_insert_with(|| Box::new(TemplateObjectMap::new()))
    }

    /// Zera os ponteiros de código gerado, o começo de `clearCode(IsoCellSet&)`.
    pub(crate) fn clear_jit_code(&mut self) {
        self.jit_code_for_call = None;
        self.jit_code_for_construct = None;
        self.jit_code_for_call_with_arity_check = None;
        self.jit_code_for_construct_with_arity_check = None;
    }
}

/// `static void setupJIT(VM&, CodeBlock*)`: `ENABLE(JIT)` vale o ramo `#else`.
fn setup_jit(_vm: &VM, _code_block: &CodeBlockRef) {
    unreachable!("UNREACHABLE_FOR_PLATFORM");
}

/// `ScriptExecutable*` com o tipo dinâmico resolvido.
#[derive(Clone)]
pub enum ScriptExecutableRef {
    Eval(Rc<RefCell<EvalExecutable>>),
    Function(Rc<RefCell<FunctionExecutable>>),
    Program(Rc<RefCell<ProgramExecutable>>),
    ModuleProgram(Rc<RefCell<ModuleProgramExecutable>>),
}

/// Aplica `$body` ao `ScriptExecutable` (por `Deref`) de qualquer variante.
macro_rules! with_script {
    ($self:expr, $e:ident => $body:expr) => {
        match $self {
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

/// Igual a `with_script!`, com empréstimo mutável.
macro_rules! with_script_mut {
    ($self:expr, $e:ident => $body:expr) => {
        match $self {
            ScriptExecutableRef::Eval(rc) => {
                let mut $e = rc.borrow_mut();
                $body
            }
            ScriptExecutableRef::Function(rc) => {
                let mut $e = rc.borrow_mut();
                $body
            }
            ScriptExecutableRef::Program(rc) => {
                let mut $e = rc.borrow_mut();
                $body
            }
            ScriptExecutableRef::ModuleProgram(rc) => {
                let mut $e = rc.borrow_mut();
                $body
            }
        }
    };
}

impl ScriptExecutableRef {
    /// Igualdade de ponteiro (`this == executable` do C++).
    pub fn ptr_eq(&self, other: &ScriptExecutableRef) -> bool {
        match (self, other) {
            (ScriptExecutableRef::Eval(a), ScriptExecutableRef::Eval(b)) => Rc::ptr_eq(a, b),
            (ScriptExecutableRef::Function(a), ScriptExecutableRef::Function(b)) => Rc::ptr_eq(a, b),
            (ScriptExecutableRef::Program(a), ScriptExecutableRef::Program(b)) => Rc::ptr_eq(a, b),
            (ScriptExecutableRef::ModuleProgram(a), ScriptExecutableRef::ModuleProgram(b)) => Rc::ptr_eq(a, b),
            _ => false,
        }
    }

    /// `JSCell::type()`.
    pub fn type_(&self) -> JSType {
        with_script!(self, e => e.type_())
    }

    /// `isInStrictContext()`.
    pub fn is_in_strict_context(&self) -> bool {
        with_script!(self, e => e.is_in_strict_context())
    }

    /// `source()`.
    pub fn source(&self) -> SourceCode {
        with_script!(self, e => e.source().clone())
    }

    /// `hashFor(CodeSpecializationKind)`.
    pub fn hash_for(&self, kind: CodeSpecializationKind) -> CodeBlockHash {
        with_script!(self, e => e.hash_for(kind))
    }

    /// `intrinsic()`.
    pub fn intrinsic(&self) -> Intrinsic {
        with_script!(self, e => e.intrinsic())
    }

    /// `hasJITCodeForCall()`.
    pub fn has_jit_code_for_call(&self) -> bool {
        with_script!(self, e => e.has_jit_code_for_call())
    }

    /// `hasJITCodeForConstruct()`.
    pub fn has_jit_code_for_construct(&self) -> bool {
        with_script!(self, e => e.has_jit_code_for_construct())
    }

    /// `hasJITCodeFor(CodeSpecializationKind)`.
    pub fn has_jit_code_for(&self, kind: CodeSpecializationKind) -> bool {
        with_script!(self, e => e.has_jit_code_for(kind))
    }

    /// `ScriptExecutable::lastLine() const`.
    pub fn last_line(&self) -> i32 {
        match self {
            ScriptExecutableRef::Function(function) => function.borrow().last_line(),
            ScriptExecutableRef::Eval(e) => e.borrow().last_line() as i32,
            ScriptExecutableRef::Program(e) => e.borrow().last_line() as i32,
            ScriptExecutableRef::ModuleProgram(e) => e.borrow().last_line() as i32,
        }
    }

    /// `ScriptExecutable::endColumn() const`.
    pub fn end_column(&self) -> u32 {
        match self {
            ScriptExecutableRef::Function(function) => function.borrow().end_column() as u32,
            ScriptExecutableRef::Eval(e) => e.borrow().end_column(),
            ScriptExecutableRef::Program(e) => e.borrow().end_column(),
            ScriptExecutableRef::ModuleProgram(e) => e.borrow().end_column(),
        }
    }

    /// `recordParse(CodeFeatures, LexicallyScopedFeatures, bool hasCapturedVariables, int lastLine, unsigned endColumn)`.
    pub fn record_parse(
        &self,
        features: CodeFeatures,
        lexically_scoped_features: LexicallyScopedFeatures,
        has_captured_variables: bool,
        last_line: i32,
        end_column: u32,
    ) {
        match self {
            ScriptExecutableRef::Function(function) => {
                // Since UnlinkedFunctionExecutable holds the information to calculate lastLine and endColumn,
                // we do not need to remember them in ScriptExecutable's fields.
                function.borrow_mut().record_parse(features, lexically_scoped_features, has_captured_variables);
            }
            ScriptExecutableRef::Eval(e) => {
                e.borrow_mut().record_parse_global(features, lexically_scoped_features, has_captured_variables, last_line, end_column)
            }
            ScriptExecutableRef::Program(e) => {
                e.borrow_mut().record_parse_global(features, lexically_scoped_features, has_captured_variables, last_line, end_column)
            }
            ScriptExecutableRef::ModuleProgram(e) => {
                e.borrow_mut().record_parse_global(features, lexically_scoped_features, has_captured_variables, last_line, end_column)
            }
        }
    }

    /// `overrideLineNumber(VM&)`.
    pub fn override_line_number(&self, _vm: &VM) -> Option<i32> {
        match self {
            ScriptExecutableRef::Function(function) => function.borrow().override_line_number(),
            _ => None,
        }
    }

    /// `typeProfilingStartOffset()`.
    pub fn type_profiling_start_offset(&self) -> u32 {
        match self {
            ScriptExecutableRef::Function(function) => function.borrow().function_start(),
            ScriptExecutableRef::Eval(_) => u32::MAX,
            _ => 0,
        }
    }

    /// `typeProfilingEndOffset()`.
    pub fn type_profiling_end_offset(&self) -> u32 {
        match self {
            ScriptExecutableRef::Function(function) => function.borrow().function_end(),
            ScriptExecutableRef::Eval(_) => u32::MAX,
            _ => (self.source().length() as u32).wrapping_sub(1),
        }
    }

    /// `moduleEnvironmentSymbolTable()` (`ASSERT(m_moduleEnvironmentSymbolTable)`, só do `ModuleProgramExecutable`).
    pub fn module_environment_symbol_table(&self) -> SymbolTableRef {
        let ScriptExecutableRef::ModuleProgram(executable) = self else {
            panic!("moduleEnvironmentSymbolTable em executável que não é ModuleProgramExecutable");
        };
        executable.borrow().module_environment_symbol_table().expect("m_moduleEnvironmentSymbolTable")
    }

    /// `isInsideOrdinaryFunction()`.
    pub fn is_inside_ordinary_function(&self) -> bool {
        with_script!(self, e => e.is_inside_ordinary_function())
    }

    /// `startColumn()`.
    pub fn start_column(&self) -> u32 {
        with_script!(self, e => e.start_column())
    }

    /// `topLevelExecutable()`.
    pub fn top_level_executable(&self) -> ScriptExecutableRef {
        match self {
            ScriptExecutableRef::Function(function) => {
                function.borrow().top_level_executable().unwrap_or_else(|| self.clone())
            }
            _ => self.clone(),
        }
    }

    /// `hasClearableCode()` (privado).
    fn has_clearable_code(&self) -> bool {
        let has_jit_code = with_script!(self, e => {
            e.jit_code_for_call.is_some()
                || e.jit_code_for_construct.is_some()
                || e.jit_code_for_call_with_arity_check.is_some()
                || e.jit_code_for_construct_with_arity_check.is_some()
        });
        if has_jit_code {
            return true;
        }
        match self {
            ScriptExecutableRef::Function(function) => function.borrow().either_code_block().is_some(),
            ScriptExecutableRef::Eval(e) => {
                let e = e.borrow();
                e.code_block().is_some() || e.unlinked_code_block().is_some()
            }
            ScriptExecutableRef::Program(e) => {
                let e = e.borrow();
                e.code_block().is_some() || e.unlinked_code_block().is_some()
            }
            ScriptExecutableRef::ModuleProgram(e) => {
                let e = e.borrow();
                e.code_block().is_some() || e.unlinked_code_block().is_some() || e.module_environment_symbol_table().is_some()
            }
        }
    }

    /// `clearCode(IsoCellSet&)`: o conjunto do heap não existe (ver o topo do módulo).
    pub fn clear_code(&self) {
        with_script_mut!(self, e => e.clear_jit_code());
        match self {
            ScriptExecutableRef::Function(function) => {
                let mut function = function.borrow_mut();
                function.clear_code_block_for_call();
                function.clear_code_block_for_construct();
            }
            ScriptExecutableRef::Eval(e) => {
                let mut e = e.borrow_mut();
                e.clear_code_block();
                e.clear_unlinked_code_block();
            }
            ScriptExecutableRef::Program(e) => {
                let mut e = e.borrow_mut();
                e.clear_code_block();
                e.clear_unlinked_code_block();
            }
            ScriptExecutableRef::ModuleProgram(e) => {
                let mut e = e.borrow_mut();
                e.clear_code_block();
                e.clear_unlinked_code_block();
                e.clear_module_environment_symbol_table();
            }
        }
    }

    /// `installCode(CodeBlock*)`.
    pub fn install_code(&self, code_block: &CodeBlockRef) {
        let (vm, code_type, kind) = {
            let code_block = code_block.borrow();
            // `codeBlock->vm()`: o `VM` é o do realm do bloco.
            (code_block.global_object().vm_rc(), code_block.code_type(), code_block.specialization_kind())
        };
        self.install_code_for_kind(&vm, Some(code_block), code_type, kind);
    }

    /// `installCode(VM&, CodeBlock*, CodeType, CodeSpecializationKind, Profiler::JettisonReason)`, sem o
    /// `JettisonReason` (ver o topo do módulo).
    pub fn install_code_for_kind(
        &self,
        vm: &VM,
        generic_code_block: Option<&CodeBlockRef>,
        code_type: CodeType,
        kind: CodeSpecializationKind,
    ) {
        let old_code_block: Option<CodeBlockRef> = match code_type {
            CodeType::GlobalCode => {
                let ScriptExecutableRef::Program(executable) = self else {
                    panic!("installCode: GlobalCode em executável que não é ProgramExecutable");
                };
                debug_assert!(kind == CodeSpecializationKind::CodeForCall);
                executable.borrow_mut().replace_code_block_with(vm, generic_code_block.cloned())
            }
            CodeType::ModuleCode => {
                let ScriptExecutableRef::ModuleProgram(executable) = self else {
                    panic!("installCode: ModuleCode em executável que não é ModuleProgramExecutable");
                };
                debug_assert!(kind == CodeSpecializationKind::CodeForCall);
                executable.borrow_mut().replace_code_block_with(vm, generic_code_block.cloned())
            }
            CodeType::EvalCode => {
                let ScriptExecutableRef::Eval(executable) = self else {
                    panic!("installCode: EvalCode em executável que não é EvalExecutable");
                };
                debug_assert!(kind == CodeSpecializationKind::CodeForCall);
                executable.borrow_mut().replace_code_block_with(vm, generic_code_block.cloned())
            }
            CodeType::FunctionCode => {
                let ScriptExecutableRef::Function(executable) = self else {
                    panic!("installCode: FunctionCode em executável que não é FunctionExecutable");
                };
                executable.borrow_mut().replace_code_block_with(vm, kind, generic_code_block.cloned())
            }
        };

        with_script_mut!(self, e => match kind {
            CodeSpecializationKind::CodeForCall => {
                e.jit_code_for_call = generic_code_block.map(|code_block| code_block.borrow().jit_code());
                e.jit_code_for_call_with_arity_check = None;
            }
            CodeSpecializationKind::CodeForConstruct => {
                e.jit_code_for_construct = generic_code_block.map(|code_block| code_block.borrow().jit_code());
                e.jit_code_for_construct_with_arity_check = None;
            }
        });

        if let Some(generic_code_block) = generic_code_block {
            // DIVERGÊNCIA: `ASSERT(isExecutableScript(codeBlock->jitType()))` e `setIsJettisoned(false)`
            // não existem: o porte só tem o LLInt (todo bloco é `InterpreterThunk`) e o `CodeBlock`
            // não guarda `m_isJettisoned`.
            assert!(generic_code_block.borrow().owner_executable().ptr_eq(self));

            let global_object = Rc::clone(generic_code_block.borrow().global_object());
            if global_object.has_debugger() {
                global_object.debugger().register_code_block(generic_code_block);
            }
        }

        // DIVERGÊNCIA: `oldCodeBlock->unlinkOrUpgradeIncomingCalls(vm, genericCodeBlock)` só age sobre
        // chamadas incoming de JIT, que não existem; o bloco antigo apenas deixa de ser referenciado.
        drop(old_code_block);
    }

    /// `newCodeBlockFor(CodeSpecializationKind, JSFunction*, JSScope*)`: `nullptr` com exceção pendente é `None`.
    pub fn new_code_block_for(
        &self,
        kind: CodeSpecializationKind,
        function: Option<&JSFunction>,
        scope: &JSScopeRef,
    ) -> Option<CodeBlockRef> {
        let vm = scope.scope().realm().vm_rc();
        let mut throw_scope = ThrowScope::new(&vm);

        debug_assert!(self.end_column() != u32::MAX);

        let global_object = scope.scope().realm();

        match self {
            ScriptExecutableRef::Eval(executable_ref) => {
                assert!(kind == CodeSpecializationKind::CodeForCall);
                assert!(executable_ref.borrow().code_block().is_none());
                assert!(function.is_none());

                // FIXME: There might be a case that executable->unlinkedCodeBlock() will be a nullptr
                // since ScriptExecutable::clearCode might be triggered due to limited memory usage.
                // We should regenerate unlinkedCodeBlock if necessary for both EvalExecutable and ProgramExecutable.
                // See similar problem for ModuleProgramExecutable in https://bugs.webkit.org/show_bug.cgi?id=255044.
                let unlinked_code_block = executable_ref.borrow().unlinked_code_block().cloned()?;
                EvalCodeBlock::create(&vm, executable_ref, &unlinked_code_block, scope)
            }
            ScriptExecutableRef::Program(executable_ref) => {
                assert!(kind == CodeSpecializationKind::CodeForCall);
                assert!(executable_ref.borrow().code_block().is_none());
                assert!(function.is_none());
                let unlinked_code_block = executable_ref.borrow().unlinked_code_block().cloned()?;
                ProgramCodeBlock::create(&vm, executable_ref, &unlinked_code_block, scope)
            }
            ScriptExecutableRef::ModuleProgram(executable_ref) => {
                assert!(kind == CodeSpecializationKind::CodeForCall);
                assert!(executable_ref.borrow().code_block().is_none());
                assert!(function.is_none());

                let unlinked_code_block = ModuleProgramExecutable::get_unlinked_code_block(executable_ref, &global_object);
                if throw_scope.exception().is_some() {
                    return None;
                }
                debug_assert!(executable_ref.borrow().unlinked_code_block().is_some());
                ModuleProgramCodeBlock::create(&vm, executable_ref, &unlinked_code_block?, scope)
            }
            ScriptExecutableRef::Function(executable_ref) => {
                assert!(function.is_some());
                assert!(executable_ref.borrow().code_block_for(kind).is_none());
                let mut error = ParserError::new();
                let mut code_generation_mode = global_object.default_code_generation_mode();
                // We continue using the same CodeGenerationMode for Generators because live generator objects can
                // keep the state which is only valid with the CodeBlock compiled with the same CodeGenerationMode.
                let parse_mode = executable_ref.borrow().parse_mode();
                if is_generator_or_async_function_body_parse_mode(parse_mode) {
                    let mut executable = executable_ref.borrow_mut();
                    if !executable.code_for_generator_body_was_generated {
                        executable.code_generation_mode_for_generator_body = code_generation_mode;
                        executable.code_for_generator_body_was_generated = true;
                    } else {
                        code_generation_mode = executable.code_generation_mode_for_generator_body;
                    }
                }
                let unlinked_executable = Rc::clone(executable_ref.borrow().unlinked_executable());
                let source = executable_ref.borrow().source().clone();
                let unlinked_code_block = unlinked_executable.borrow_mut().unlinked_code_block_for(
                    &vm,
                    &source,
                    kind,
                    code_generation_mode,
                    &mut error,
                    parse_mode,
                );
                {
                    let unlinked = unlinked_executable.borrow();
                    self.record_parse(
                        unlinked.features(),
                        unlinked.lexically_scoped_features(),
                        unlinked.has_captured_variables(),
                        self.last_line(),
                        self.end_column(),
                    );
                }
                let Some(unlinked_code_block) = unlinked_code_block else {
                    throw_exception(&global_object, &mut throw_scope, error.to_error_object(&global_object, &source).expect("ParserError válido"));
                    return None;
                };
                FunctionCodeBlock::create(&vm, executable_ref, &unlinked_code_block, scope)
            }
        }
    }

    /// `newReplacementCodeBlockFor(CodeSpecializationKind)`.
    ///
    /// DIVERGÊNCIA: o `VM& vm = this->vm()` do C++ vem do `JSCell`; aqui vem do `CodeBlock` de linha de base,
    /// que guarda o mesmo `VM`.
    pub fn new_replacement_code_block_for(&self, kind: CodeSpecializationKind) -> CodeBlockRef {
        let (current, create): (Option<CodeBlockRef>, fn(&VM, &CodeBlockRef) -> CodeBlockRef) = match self {
            ScriptExecutableRef::Eval(executable) => {
                assert!(kind == CodeSpecializationKind::CodeForCall);
                (executable.borrow().code_block(), CodeBlock::create_copy_parsed_block)
            }
            ScriptExecutableRef::Program(executable) => {
                assert!(kind == CodeSpecializationKind::CodeForCall);
                (executable.borrow().code_block(), CodeBlock::create_copy_parsed_block)
            }
            ScriptExecutableRef::ModuleProgram(executable) => {
                assert!(kind == CodeSpecializationKind::CodeForCall);
                (executable.borrow().code_block(), CodeBlock::create_copy_parsed_block)
            }
            ScriptExecutableRef::Function(executable) => {
                (executable.borrow().code_block_for(kind), CodeBlock::create_copy_parsed_block)
            }
        };
        let baseline = Rc::clone(&current.expect("newReplacementCodeBlockFor sem CodeBlock"));
        let vm = baseline.borrow().global_object().vm_rc();
        // DIVERGÊNCIA: sem `m_alternative` (só LLInt), `baselineVersion()` é o próprio bloco e
        // `setAlternative(vm, baseline)` não tem onde guardar.
        create(&vm, &baseline)
    }

    /// `prepareForExecutionImpl(VM&, JSFunction*, JSScope*, CodeSpecializationKind, CodeBlock*&)` (privado).
    fn prepare_for_execution_impl(
        &self,
        vm: &VM,
        function: Option<&JSFunction>,
        scope: &JSScopeRef,
        kind: CodeSpecializationKind,
        result_code_block: &mut Option<CodeBlockRef>,
    ) {
        let mut throw_scope = ThrowScope::new(vm);

        if vm.get_and_clear_fail_next_new_code_block() {
            let global_object = scope.scope().realm();
            let error = crate::runtime::error::create_error(&global_object, &WtfString::from_latin1(b"Forced Failure"));
            throw_exception(&global_object, &mut throw_scope, error);
            return;
        }

        let code_block = self.new_code_block_for(kind, function, scope);
        if throw_scope.exception().is_some() {
            return;
        }

        let code_block = code_block.expect("newCodeBlockFor sem exceção devolveu nulo");
        *result_code_block = Some(Rc::clone(&code_block));

        // DIVERGÊNCIA: `Options::validateBytecode()` chama `CodeBlock::validate()` (BytecodeValidator),
        // que não foi portado; a opção é só de depuração e não muda o resultado.

        // `ENABLE(JIT)` vale o ramo `#else`: não há `m_unlinkedBaselineCode`, então
        // `installedUnlinkedBaselineCode` é sempre falso.
        if Options::use_ll_int() {
            // `setupLLInt(codeBlock)`. PENDÊNCIA: `crate::llint::llint_entrypoint` (o `LLInt::setEntrypoint`,
            // que instala o `LLIntJITCode`) ainda não foi portado.
            llint_entrypoint::set_entrypoint(&code_block);
        } else {
            setup_jit(vm, &code_block);
        }

        let (code_type, specialization_kind) = {
            let code_block = code_block.borrow();
            (code_block.code_type(), code_block.specialization_kind())
        };
        self.install_code_for_kind(vm, Some(&code_block), code_type, specialization_kind);
    }

    /// `prepareForExecution<ExecutableType>(VM&, JSFunction*, JSScope*, CodeSpecializationKind, CodeBlock*&)`
    /// (`bytecode/CodeBlock.h`): o `ExecutableType` do template é a variante de `self`.
    pub fn prepare_for_execution(
        &self,
        vm: &VM,
        function: Option<&JSFunction>,
        scope: &JSScopeRef,
        kind: CodeSpecializationKind,
        result_code_block: &mut Option<CodeBlockRef>,
    ) {
        if self.has_jit_code_for(kind) {
            *result_code_block = match self {
                ScriptExecutableRef::Eval(executable) => executable.borrow().code_block(),
                ScriptExecutableRef::Program(executable) => executable.borrow().code_block(),
                ScriptExecutableRef::ModuleProgram(executable) => executable.borrow().code_block(),
                ScriptExecutableRef::Function(executable) => executable.borrow().code_block_for(kind),
            };
            return;
        }

        self.prepare_for_execution_impl(vm, function, scope, kind, result_code_block);
    }

    /// `ensureTemplateObjectMap(VM&)` (privado), entregue por chamada de retorno porque o mapa vive
    /// dentro do `RefCell` do executável concreto.
    fn with_template_object_map<R>(&self, vm: &VM, f: impl FnOnce(&mut TemplateObjectMap) -> R) -> R {
        match self {
            ScriptExecutableRef::Function(e) => f(e.borrow_mut().ensure_template_object_map(vm)),
            ScriptExecutableRef::Eval(e) => f(e.borrow_mut().ensure_template_object_map(vm)),
            ScriptExecutableRef::Program(e) => f(e.borrow_mut().ensure_template_object_map(vm)),
            ScriptExecutableRef::ModuleProgram(e) => f(e.borrow_mut().ensure_template_object_map(vm)),
        }
    }

    /// `createTemplateObject(JSGlobalObject*, JSTemplateObjectDescriptor*)`.
    pub fn create_template_object(
        &self,
        global_object: &JSGlobalObject,
        descriptor: &JSTemplateObjectDescriptor,
    ) -> Option<JSArrayRef> {
        let vm = global_object.vm();
        let throw_scope = ThrowScope::new(&vm);

        let end_offset = descriptor.end_offset() as u64;
        let existing = self.with_template_object_map(&vm, |map| map.entry(end_offset).or_insert(None).clone());
        if let Some(array) = existing {
            return Some(array);
        }
        let template_object = descriptor.create_template_object(global_object);
        if throw_scope.exception().is_some() {
            return None;
        }
        self.with_template_object_map(&vm, |map| {
            map.insert(end_offset, template_object.clone());
        });
        template_object
    }

    /// `ExecutableBase::dump(PrintStream&)` para as quatro classes concretas.
    pub fn dump(&self, out: &mut dyn fmt::Write) -> fmt::Result {
        match self {
            ScriptExecutableRef::Eval(eval) => match eval.borrow().code_block() {
                Some(code_block) => code_block.borrow().dump(out),
                None => out.write_str("EvalExecutable w/o CodeBlock"),
            },
            ScriptExecutableRef::Program(program) => match program.borrow().code_block() {
                Some(code_block) => code_block.borrow().dump(out),
                None => out.write_str("ProgramExecutable w/o CodeBlock"),
            },
            ScriptExecutableRef::ModuleProgram(executable) => match executable.borrow().code_block() {
                Some(code_block) => code_block.borrow().dump(out),
                None => out.write_str("ModuleProgramExecutable w/o CodeBlock"),
            },
            ScriptExecutableRef::Function(function) => {
                let function = function.borrow();
                if function.either_code_block().is_none() {
                    out.write_str("FunctionExecutable w/o CodeBlock")
                } else {
                    // `CommaPrinter comma("/"_s)`: o separador só sai a partir do segundo item.
                    let mut first = true;
                    if let Some(code_block) = function.code_block_for_call() {
                        code_block.borrow().dump(out)?;
                        first = false;
                    }
                    if let Some(code_block) = function.code_block_for_construct() {
                        if !first {
                            out.write_str("/")?;
                        }
                        code_block.borrow().dump(out)?;
                    }
                    Ok(())
                }
            }
        }
    }
}
