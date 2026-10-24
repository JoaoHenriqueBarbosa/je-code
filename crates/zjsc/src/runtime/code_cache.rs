//! Porte de `JavaScriptCore/runtime/CodeCache.h` e `CodeCache.cpp`: o cache em memória dos blocos de
//! código não ligados (programa, módulo, eval) e dos executáveis de `new Function`, mais as funções que
//! geram esses blocos (`parse<RootNode>` seguido de `BytecodeGenerator::generate`).
//!
//! Divergências (documentadas, nenhuma muda o que o programa JS observa):
//!
//! - Sem `Strong<JSCell>`: a célula do cache é o `Rc` do bloco ou do executável (`CodeCacheCell`).
//! - Sem cache de bytecode persistente: `fetchFromDisk` (que lê o `CachedBytecode` do `SourceProvider`
//!   e decodifica), `writeCodeBlock` (`commitCachedBytecode`), `CodeCache::write`, `encodeCodeBlock`,
//!   `serializeBytecode`, `sourceCodeKeyForSerializedProgram/Module` e o gancho
//!   `didGenerateUnlinkedCodeBlock` do Bun dependem da camada `CachedBytecode`/`BytecodeCacheError`,
//!   que não foi portada. Sem ela o `SourceProvider` nunca tem bytecode guardado, então `fetchFromDisk`
//!   devolve nulo e os outros são no-ops, exatamente como no C++ para um provider sem cache. Pelo mesmo
//!   motivo `CodeCache::updateCache` (que só repassa ao `SourceProvider::updateCache`, no-op no C++) não
//!   existe.
//! - `ApproximateTime` são os nanossegundos do `CLOCK_MONOTONIC` do pseudo-processo.
//! - `getUnlinkedGlobalCodeBlock` recebe o executável como `&dyn GlobalCodeExecutable` (o `ExecutableType`
//!   do template); o `IndirectEvalExecutable`/`DirectEvalExecutable` do C++ são o `EvalExecutable` do
//!   porte (o indireto tem `needsClassFieldInitializer = No` e `privateBrandRequirement = None`, os
//!   mesmos valores que o ramo `is_same_v<ExecutableType, DirectEvalExecutable>` deixaria).
//! - `VM` é `&Rc<VM>` onde o C++ gera o bytecode, porque o `parse<..>` do porte guarda o `Rc<VM>`.
//!   `VM::code_cache()` (um `CodeCache` por VM, com mutação interior) é do `vm.rs`.

use std::cell::RefCell;
use std::collections::HashMap;
use std::ops::DerefMut;
use std::rc::Rc;
use std::time::Duration;

use crate::bytecode::executable_info::{
    DerivedContextType, EvalContextType, ExecutableInfo, NeedsClassFieldInitializer,
};
use crate::bytecode::tdz_environment::{TDZEnvironment, TDZEnvironmentLink};
use crate::bytecode::unlinked_code_block::{
    UnlinkedEvalCodeBlock, UnlinkedGlobalCodeBlock, UnlinkedModuleProgramCodeBlock, UnlinkedProgramCodeBlock,
};
use crate::bytecode::unlinked_function_executable::{UnlinkedFunctionExecutable, UnlinkedFunctionExecutableRef, UnlinkedFunctionKind};
use crate::bytecompiler::bytecode_generator::{BytecodeGenerator, BytecodeGeneratorNode};
use crate::parser::nodes::{node, ProgramNode, EvalNode, ModuleProgramNode, Statement};
use crate::parser::parser_error::{ErrorType, ParserError, SyntaxErrorType};
use crate::parser::parser_modes::{
    construct_ability_for_parse_mode, CodeGenerationModeSet, FunctionMode, JSParserBuiltinMode, JSParserScriptMode,
    LexicallyScopedFeatures, PrivateBrandRequirement, SourceParseMode, SuperBinding,
};
use crate::parser::parser::{parse, parse_function_for_function_constructor, ParsedNode};
use crate::parser::parser_tokens::{JSTextPosition, JSToken};
use crate::parser::source_code::SourceCode;
use crate::parser::source_code_key::{SourceCodeKey, SourceCodeType};
use crate::parser::variable_environment::PrivateNameEnvironment;
use crate::runtime::code_specialization_kind::CodeSpecializationKind;
use crate::runtime::constructor_kind::ConstructorKind;
use crate::runtime::eval_executable::EvalExecutable;
use crate::runtime::identifier::Identifier;
use crate::runtime::implementation_visibility::ImplementationVisibility;
use crate::runtime::inline_attribute::InlineAttribute;
use crate::runtime::module_program_executable::ModuleProgramExecutable;
use crate::runtime::options_list::Options;
use crate::runtime::program_executable::ProgramExecutable;
use crate::runtime::script_executable::ScriptExecutableRef;
use crate::runtime::vm::VM;
use crate::wtf::text::wtf_string::String as WtfString;

/// `SourceCodeValue::cell` (`Strong<JSCell>`): o objeto guardado, de um dos quatro tipos que o cache aceita.
#[derive(Clone)]
pub enum CodeCacheCell {
    Program(Rc<RefCell<UnlinkedProgramCodeBlock>>),
    Eval(Rc<RefCell<UnlinkedEvalCodeBlock>>),
    ModuleProgram(Rc<RefCell<UnlinkedModuleProgramCodeBlock>>),
    Function(UnlinkedFunctionExecutableRef),
}

/// O `UnlinkedCodeBlockType` do `findCacheAndUpdateAge<..>` (o `uncheckedDowncast` do C++).
pub trait CodeCacheCellKind: Sized {
    fn from_cell(cell: &CodeCacheCell) -> Option<Rc<RefCell<Self>>>;
    fn into_cell(value: Rc<RefCell<Self>>) -> CodeCacheCell;
}

macro_rules! impl_cell_kind {
    ($type:ty, $variant:ident) => {
        impl CodeCacheCellKind for $type {
            fn from_cell(cell: &CodeCacheCell) -> Option<Rc<RefCell<Self>>> {
                match cell {
                    CodeCacheCell::$variant(value) => Some(Rc::clone(value)),
                    _ => None,
                }
            }

            fn into_cell(value: Rc<RefCell<Self>>) -> CodeCacheCell {
                CodeCacheCell::$variant(value)
            }
        }
    };
}

impl_cell_kind!(UnlinkedProgramCodeBlock, Program);
impl_cell_kind!(UnlinkedEvalCodeBlock, Eval);
impl_cell_kind!(UnlinkedModuleProgramCodeBlock, ModuleProgram);
impl_cell_kind!(UnlinkedFunctionExecutable, Function);

/// `struct SourceCodeValue`.
#[derive(Clone)]
pub struct SourceCodeValue {
    pub cell: CodeCacheCell,
    pub age: i64,
}

// This constant factor biases cache capacity toward allowing a minimum working set to enter the cache before
// it starts evicting.
const WORKING_SET_TIME: Duration = Duration::from_secs(10);
const WORKING_SET_MAX_BYTES: i64 = 16_000_000;
const WORKING_SET_MAX_ENTRIES: usize = 2000;

// This constant factor biases cache capacity toward recent activity. We want to adapt to changing workloads.
const RECENCY_BIAS: i64 = 4;

// This constant factor treats a sampled event for one old object as if it happened for many old objects. Most
// old objects are evicted before we can sample them, so we need to extrapolate from the ones we do sample.
const OLD_OBJECT_SAMPLING_MULTIPLIER: i64 = 32;

/// `class CodeCacheMap`.
pub struct CodeCacheMap {
    map: HashMap<SourceCodeKey, SourceCodeValue>,
    size: i64,
    size_at_last_prune: i64,
    time_at_last_prune: u64,
    min_capacity: i64,
    capacity: i64,
    age: i64,
}

impl Default for CodeCacheMap {
    fn default() -> CodeCacheMap {
        CodeCacheMap {
            map: HashMap::new(),
            size: 0,
            size_at_last_prune: 0,
            time_at_last_prune: ul_common::time::monotonic_nanos(),
            min_capacity: 0,
            capacity: 0,
            age: 0,
        }
    }
}

impl CodeCacheMap {
    /// `findCacheAndUpdateAge<UnlinkedCodeBlockType>(VM&, const SourceCodeKey&)`. O ramo `fetchFromDisk` é
    /// sempre nulo (ver o topo do módulo).
    pub fn find_cache_and_update_age<T: CodeCacheCellKind>(&mut self, key: &SourceCodeKey) -> Option<Rc<RefCell<T>>> {
        self.prune();

        let find_result = self.map.get_mut(key)?;

        let age = self.age - find_result.age;
        if age > self.capacity {
            // A requested object is older than the cache's capacity. We can infer that requested objects are
            // subject to high eviction probability, so we grow the cache to improve our hit rate.
            self.capacity += RECENCY_BIAS * OLD_OBJECT_SAMPLING_MULTIPLIER * key.length() as i64;
        } else if age < self.capacity / 2 {
            // A requested object is much younger than the cache's capacity. We can infer that requested objects
            // are subject to low eviction probability, so we shrink the cache to save memory.
            self.capacity -= RECENCY_BIAS * key.length() as i64;
            if self.capacity < self.min_capacity {
                self.capacity = self.min_capacity;
            }
        }

        find_result.age = self.age;
        self.age += key.length() as i64;

        T::from_cell(&find_result.cell)
    }

    /// `addCache(const SourceCodeKey&, const SourceCodeValue&)`.
    pub fn add_cache(&mut self, key: &SourceCodeKey, value: SourceCodeValue) {
        self.prune();

        let previous = self.map.insert(key.clone(), value);
        debug_assert!(previous.is_none());

        self.size += key.length() as i64;
        self.age += key.length() as i64;
    }

    /// `remove(iterator)`.
    pub fn remove(&mut self, key: &SourceCodeKey) {
        if self.map.remove(key).is_some() {
            self.size -= key.length() as i64;
        }
    }

    /// `clear()`.
    pub fn clear(&mut self) {
        self.size = 0;
        self.age = 0;
        self.map.clear();
    }

    /// `age()`.
    pub fn age(&self) -> i64 {
        self.age
    }

    fn number_of_entries(&self) -> usize {
        self.map.len()
    }

    fn can_prune_quickly(&self) -> bool {
        self.number_of_entries() < WORKING_SET_MAX_ENTRIES
    }

    fn prune(&mut self) {
        if self.size <= self.capacity && self.can_prune_quickly() {
            return;
        }

        if Duration::from_nanos(ul_common::time::monotonic_nanos().saturating_sub(self.time_at_last_prune)) < WORKING_SET_TIME
            && self.size - self.size_at_last_prune < WORKING_SET_MAX_BYTES
            && self.can_prune_quickly()
        {
            return;
        }

        self.prune_slow_case();
    }

    /// `CodeCacheMap::pruneSlowCase()`. O `writeCodeBlock` por entrada removida não existe (ver o topo).
    fn prune_slow_case(&mut self) {
        self.min_capacity = (self.size - self.size_at_last_prune).max(0);
        self.size_at_last_prune = self.size;
        self.time_at_last_prune = ul_common::time::monotonic_nanos();

        if self.capacity < self.min_capacity {
            self.capacity = self.min_capacity;
        }

        while self.size > self.capacity || !self.can_prune_quickly() {
            let Some(key) = self.map.keys().next().cloned() else {
                break;
            };
            self.size -= key.length() as i64;
            self.map.remove(&key);
        }
    }
}

/// O `ExecutableType` de `getUnlinkedGlobalCodeBlock`/`generateUnlinkedCodeBlock`: o que o cache lê do
/// `ProgramExecutable`, `ModuleProgramExecutable` e `EvalExecutable`.
pub trait GlobalCodeExecutable {
    /// A referência de `ScriptExecutable` do próprio executável (para `recordParse`).
    fn script_ref(&self) -> ScriptExecutableRef;
    fn lexically_scoped_features(&self) -> LexicallyScopedFeatures;
    fn derived_context_type(&self) -> DerivedContextType;
    fn is_arrow_function_context(&self) -> bool;
    fn is_inside_ordinary_function(&self) -> bool;
    /// Só o `DirectEvalExecutable` tem valor diferente do padrão.
    fn needs_class_field_initializer(&self) -> NeedsClassFieldInitializer {
        NeedsClassFieldInitializer::No
    }
    fn private_brand_requirement(&self) -> PrivateBrandRequirement {
        PrivateBrandRequirement::None
    }
}

macro_rules! impl_global_code_executable {
    ($type:ty, $variant:ident $(, $extra:item)*) => {
        impl GlobalCodeExecutable for Rc<RefCell<$type>> {
            fn script_ref(&self) -> ScriptExecutableRef {
                ScriptExecutableRef::$variant(Rc::clone(self))
            }

            fn lexically_scoped_features(&self) -> LexicallyScopedFeatures {
                self.borrow().lexically_scoped_features()
            }

            fn derived_context_type(&self) -> DerivedContextType {
                self.borrow().derived_context_type()
            }

            fn is_arrow_function_context(&self) -> bool {
                self.borrow().is_arrow_function_context()
            }

            fn is_inside_ordinary_function(&self) -> bool {
                self.borrow().is_inside_ordinary_function()
            }

            $($extra)*
        }
    };
}

impl_global_code_executable!(ProgramExecutable, Program);
impl_global_code_executable!(ModuleProgramExecutable, ModuleProgram);
impl_global_code_executable!(
    EvalExecutable,
    Eval,
    fn needs_class_field_initializer(&self) -> NeedsClassFieldInitializer {
        self.borrow().needs_class_field_initializer()
    },
    fn private_brand_requirement(&self) -> PrivateBrandRequirement {
        self.borrow().private_brand_requirement()
    }
);

/// O que `generateUnlinkedCodeBlockImpl` lê do nó raiz (`ProgramNode`, `EvalNode`, `ModuleProgramNode`).
pub trait CacheRootNode: ParsedNode {
    fn first_line(&self) -> i32;
    fn last_line(&self) -> u32;
    fn start_column(&self) -> u32;
    fn end_column(&self) -> u32;
    fn features(&self) -> crate::parser::parser_modes::CodeFeatures;
    fn lexically_scoped_features(&self) -> LexicallyScopedFeatures;
    fn has_captured_variables(&self) -> bool;
}

macro_rules! impl_cache_root_node {
    ($type:ty, $start_column:expr) => {
        impl CacheRootNode for $type {
            fn first_line(&self) -> i32 {
                self.base.base.base.position.line
            }

            fn last_line(&self) -> u32 {
                self.base.base.last_line()
            }

            fn start_column(&self) -> u32 {
                let this = self;
                ($start_column)(this)
            }

            fn end_column(&self) -> u32 {
                self.end_column
            }

            fn features(&self) -> crate::parser::parser_modes::CodeFeatures {
                self.base.features
            }

            fn lexically_scoped_features(&self) -> LexicallyScopedFeatures {
                self.base.lexically_scoped_features
            }

            fn has_captured_variables(&self) -> bool {
                self.base.var_declarations.has_captured_variables()
            }
        }
    };
}

impl_cache_root_node!(ProgramNode, |node: &ProgramNode| node.start_column);
impl_cache_root_node!(EvalNode, |_node: &EvalNode| 0);
impl_cache_root_node!(ModuleProgramNode, |node: &ModuleProgramNode| node.start_column);

/// `template <> struct CacheTypes<UnlinkedCodeBlockType>`.
pub trait CacheTypes: CodeCacheCellKind + DerefMut<Target = UnlinkedGlobalCodeBlock> {
    type RootNode: CacheRootNode + BytecodeGeneratorNode<Self>;
    const CODE_TYPE: SourceCodeType;
    const PARSE_MODE: SourceParseMode;

    /// `UnlinkedCodeBlockType::create(vm, executableInfo, codeGenerationMode)`.
    fn create_block(info: &ExecutableInfo, code_generation_mode: u8) -> Rc<RefCell<Self>>;
}

macro_rules! impl_cache_types {
    ($type:ty, $root:ty, $code_type:ident, $parse_mode:ident) => {
        impl CacheTypes for $type {
            type RootNode = $root;
            const CODE_TYPE: SourceCodeType = SourceCodeType::$code_type;
            const PARSE_MODE: SourceParseMode = SourceParseMode::$parse_mode;

            fn create_block(info: &ExecutableInfo, code_generation_mode: u8) -> Rc<RefCell<Self>> {
                <$type>::create(info, code_generation_mode)
            }
        }
    };
}

impl_cache_types!(UnlinkedProgramCodeBlock, ProgramNode, ProgramType, ProgramMode);
impl_cache_types!(UnlinkedEvalCodeBlock, EvalNode, EvalType, ProgramMode);
impl_cache_types!(UnlinkedModuleProgramCodeBlock, ModuleProgramNode, ModuleType, ModuleEvaluateMode);

/// `generateUnlinkedCodeBlockForFunctions(vm, unlinkedCodeBlock, parentSource, mode, error, depth)`.
/// `depth` `None` é o `std::numeric_limits<unsigned>::max()` do padrão do C++.
fn generate_unlinked_code_block_for_functions(
    vm: &Rc<VM>,
    function_decls: &[UnlinkedFunctionExecutableRef],
    function_exprs: &[UnlinkedFunctionExecutableRef],
    parent_source: &SourceCode,
    code_generation_mode: CodeGenerationModeSet,
    error: &mut ParserError,
    depth: u32,
) {
    if depth == 0 {
        return;
    }
    let mut generate = |unlinked_executable: &UnlinkedFunctionExecutableRef| {
        // FIXME: We should also generate CodeBlocks for CodeForConstruct of ordinary functions.
        // https://bugs.webkit.org/show_bug.cgi?id=193823
        let (kind, source, parse_mode) = {
            let executable = unlinked_executable.borrow();
            let kind = if executable.is_class_constructor_function() {
                CodeSpecializationKind::CodeForConstruct
            } else {
                CodeSpecializationKind::CodeForCall
            };
            (kind, executable.linked_source_code(parent_source), executable.parse_mode())
        };
        let unlinked_function_code_block = unlinked_executable.borrow_mut().unlinked_code_block_for(
            vm,
            &source,
            kind,
            code_generation_mode,
            error,
            parse_mode,
        );
        if let Some(unlinked_function_code_block) = unlinked_function_code_block {
            let base = unlinked_function_code_block.borrow().base_ref();
            let base = base.borrow();
            generate_unlinked_code_block_for_functions(
                vm,
                base.function_decls(),
                base.function_exprs(),
                &source,
                code_generation_mode,
                error,
                depth - 1,
            );
        }
    };

    for function_decl in function_decls {
        generate(function_decl);
    }
    for function_expr in function_exprs {
        generate(function_expr);
    }
}

/// `generateUnlinkedCodeBlockImpl<UnlinkedCodeBlockType, ExecutableType>(...)`. `executable` nulo é `None`.
#[allow(clippy::too_many_arguments)]
fn generate_unlinked_code_block_impl<T: CacheTypes>(
    vm: &Rc<VM>,
    source: &SourceCode,
    lexically_scoped_features: LexicallyScopedFeatures,
    script_mode: JSParserScriptMode,
    code_generation_mode: CodeGenerationModeSet,
    error: &mut ParserError,
    eval_context_type: EvalContextType,
    derived_context_type: DerivedContextType,
    is_arrow_function_context: bool,
    variables_under_tdz: Option<&TDZEnvironment>,
    private_name_environment: Option<&PrivateNameEnvironment>,
    executable: Option<&dyn GlobalCodeExecutable>,
) -> Option<Rc<RefCell<T>>> {
    let is_inside_ordinary_function = executable.is_some_and(|executable| executable.is_inside_ordinary_function());

    let root_node = parse::<T::RootNode>(
        vm,
        source,
        &Identifier::default(),
        ImplementationVisibility::Public,
        JSParserBuiltinMode::NotBuiltin,
        lexically_scoped_features,
        script_mode,
        T::PARSE_MODE,
        FunctionMode::None,
        SuperBinding::NotNeeded,
        error,
        ConstructorKind::None,
        derived_context_type,
        eval_context_type,
        private_name_environment,
        None,
        is_inside_ordinary_function,
    )?;

    let line_count = root_node.last_line().wrapping_sub(root_node.first_line() as u32);
    let start_column = root_node.start_column() + 1;
    let end_column_is_on_start_line = line_count == 0;
    let unlinked_end_column = root_node.end_column();
    let end_column = unlinked_end_column + if end_column_is_on_start_line { start_column } else { 1 };
    if let Some(executable) = executable {
        executable.script_ref().record_parse(
            root_node.features(),
            root_node.lexically_scoped_features(),
            root_node.has_captured_variables(),
            root_node.last_line() as i32,
            end_column,
        );
    }

    let mut needs_class_field_initializer = NeedsClassFieldInitializer::No;
    let mut private_brand_requirement = PrivateBrandRequirement::None;
    if T::CODE_TYPE == SourceCodeType::EvalType {
        if let Some(executable) = executable {
            needs_class_field_initializer = executable.needs_class_field_initializer();
            private_brand_requirement = executable.private_brand_requirement();
        }
    }
    let executable_info = ExecutableInfo::new(
        false,
        private_brand_requirement,
        false,
        ConstructorKind::None,
        script_mode,
        SuperBinding::NotNeeded,
        T::PARSE_MODE,
        derived_context_type,
        needs_class_field_initializer,
        is_arrow_function_context,
        false,
        eval_context_type,
        false,
    );

    let unlinked_code_block = T::create_block(&executable_info, code_generation_mode.to_raw());
    {
        let mut block = unlinked_code_block.borrow_mut();
        block.record_parse(
            root_node.features(),
            root_node.lexically_scoped_features(),
            root_node.has_captured_variables(),
            line_count,
            unlinked_end_column,
        );
        if let Some(provider) = source.provider() {
            let source_url_directive = provider.source_url_directive();
            if !source_url_directive.is_null() {
                block.set_source_url_directive(source_url_directive);
            }
            let source_mapping_url_directive = provider.source_mapping_url_directive();
            if !source_mapping_url_directive.is_null() {
                block.set_source_mapping_url_directive(source_mapping_url_directive);
            }
        }
    }

    let parent_variables_under_tdz: Option<Rc<TDZEnvironmentLink>> = variables_under_tdz.map(|variables_under_tdz| {
        TDZEnvironmentLink::create(vm.compact_variable_map().get(variables_under_tdz), None)
    });
    let root_node = node(*root_node);
    *error = BytecodeGenerator::generate_for::<T::RootNode, T>(
        vm,
        &root_node,
        source,
        &unlinked_code_block,
        code_generation_mode,
        &parent_variables_under_tdz,
        None,
        private_name_environment,
    );

    if error.is_valid() {
        return None;
    }

    Some(unlinked_code_block)
}

/// `generateUnlinkedCodeBlock<UnlinkedCodeBlockType, ExecutableType>(...)`.
#[allow(clippy::too_many_arguments)]
fn generate_unlinked_code_block<T: CacheTypes>(
    vm: &Rc<VM>,
    executable: &dyn GlobalCodeExecutable,
    source: &SourceCode,
    script_mode: JSParserScriptMode,
    code_generation_mode: CodeGenerationModeSet,
    error: &mut ParserError,
    eval_context_type: EvalContextType,
    variables_under_tdz: Option<&TDZEnvironment>,
    private_name_environment: Option<&PrivateNameEnvironment>,
) -> Option<Rc<RefCell<T>>> {
    generate_unlinked_code_block_impl::<T>(
        vm,
        source,
        executable.lexically_scoped_features(),
        script_mode,
        code_generation_mode,
        error,
        eval_context_type,
        executable.derived_context_type(),
        executable.is_arrow_function_context(),
        variables_under_tdz,
        private_name_environment,
        Some(executable),
    )
}

/// `generateUnlinkedCodeBlockForDirectEval(...)`.
#[allow(clippy::too_many_arguments)]
pub fn generate_unlinked_code_block_for_direct_eval(
    vm: &Rc<VM>,
    executable: &Rc<RefCell<EvalExecutable>>,
    source: &SourceCode,
    script_mode: JSParserScriptMode,
    code_generation_mode: CodeGenerationModeSet,
    error: &mut ParserError,
    eval_context_type: EvalContextType,
    variables_under_tdz: Option<&TDZEnvironment>,
    private_name_environment: Option<&PrivateNameEnvironment>,
) -> Option<Rc<RefCell<UnlinkedEvalCodeBlock>>> {
    generate_unlinked_code_block::<UnlinkedEvalCodeBlock>(
        vm,
        executable,
        source,
        script_mode,
        code_generation_mode,
        error,
        eval_context_type,
        variables_under_tdz,
        private_name_environment,
    )
}

/// `recursivelyGenerateUnlinkedCodeBlock<UnlinkedCodeBlockType>(...)` (nunca para o eval).
#[allow(clippy::too_many_arguments)]
fn recursively_generate_unlinked_code_block<T: CacheTypes>(
    vm: &Rc<VM>,
    source: &SourceCode,
    lexically_scoped_features: LexicallyScopedFeatures,
    script_mode: JSParserScriptMode,
    code_generation_mode: CodeGenerationModeSet,
    error: &mut ParserError,
    eval_context_type: EvalContextType,
    depth: u32,
) -> Option<Rc<RefCell<T>>> {
    debug_assert!(T::CODE_TYPE != SourceCodeType::EvalType);
    let is_arrow_function_context = false;
    let unlinked_code_block = generate_unlinked_code_block_impl::<T>(
        vm,
        source,
        lexically_scoped_features,
        script_mode,
        code_generation_mode,
        error,
        eval_context_type,
        DerivedContextType::None,
        is_arrow_function_context,
        None,
        None,
        None,
    )?;

    let base = unlinked_code_block.borrow().base_ref();
    {
        let base = base.borrow();
        generate_unlinked_code_block_for_functions(
            vm,
            base.function_decls(),
            base.function_exprs(),
            source,
            code_generation_mode,
            error,
            depth,
        );
    }
    Some(unlinked_code_block)
}

/// `recursivelyGenerateUnlinkedCodeBlocksForFunction(...)`.
pub fn recursively_generate_unlinked_code_blocks_for_function(
    vm: &Rc<VM>,
    executable: &UnlinkedFunctionExecutableRef,
    parent_source: &SourceCode,
    error: &mut ParserError,
    depth: u32,
) {
    let (kind, source, parse_mode) = {
        let executable = executable.borrow();
        let kind = if executable.is_class_constructor_function() {
            CodeSpecializationKind::CodeForConstruct
        } else {
            CodeSpecializationKind::CodeForCall
        };
        (kind, executable.linked_source_code(parent_source), executable.parse_mode())
    };
    let code_block = executable.borrow_mut().unlinked_code_block_for(
        vm,
        &source,
        kind,
        CodeGenerationModeSet::empty(),
        error,
        parse_mode,
    );
    if let Some(code_block) = code_block {
        let base = code_block.borrow().base_ref();
        let base = base.borrow();
        generate_unlinked_code_block_for_functions(
            vm,
            base.function_decls(),
            base.function_exprs(),
            &source,
            CodeGenerationModeSet::empty(),
            error,
            depth,
        );
    }
}

/// `recursivelyGenerateUnlinkedCodeBlockForProgram(...)`; `depth` padrão do C++ é `u32::MAX`.
#[allow(clippy::too_many_arguments)]
pub fn recursively_generate_unlinked_code_block_for_program(
    vm: &Rc<VM>,
    source: &SourceCode,
    lexically_scoped_features: LexicallyScopedFeatures,
    script_mode: JSParserScriptMode,
    code_generation_mode: CodeGenerationModeSet,
    error: &mut ParserError,
    eval_context_type: EvalContextType,
    depth: u32,
) -> Option<Rc<RefCell<UnlinkedProgramCodeBlock>>> {
    recursively_generate_unlinked_code_block::<UnlinkedProgramCodeBlock>(
        vm,
        source,
        lexically_scoped_features,
        script_mode,
        code_generation_mode,
        error,
        eval_context_type,
        depth,
    )
}

/// `recursivelyGenerateUnlinkedCodeBlockForModuleProgram(...)`.
#[allow(clippy::too_many_arguments)]
pub fn recursively_generate_unlinked_code_block_for_module_program(
    vm: &Rc<VM>,
    source: &SourceCode,
    lexically_scoped_features: LexicallyScopedFeatures,
    script_mode: JSParserScriptMode,
    code_generation_mode: CodeGenerationModeSet,
    error: &mut ParserError,
    eval_context_type: EvalContextType,
    depth: u32,
) -> Option<Rc<RefCell<UnlinkedModuleProgramCodeBlock>>> {
    recursively_generate_unlinked_code_block::<UnlinkedModuleProgramCodeBlock>(
        vm,
        source,
        lexically_scoped_features,
        script_mode,
        code_generation_mode,
        error,
        eval_context_type,
        depth,
    )
}

/// `recordParseFromUnlinkedCodeBlock(GlobalExecutable*, const SourceCode&, UnlinkedGlobalCodeBlock*)`
/// (`USE(BUN_JSC_ADDITIONS)`): o que um acerto do cache faz além de devolver o bloco.
pub fn record_parse_from_unlinked_code_block(
    executable: &ScriptExecutableRef,
    source: &SourceCode,
    unlinked_code_block: &UnlinkedGlobalCodeBlock,
) {
    let line_count = unlinked_code_block.line_count();
    let start_column = unlinked_code_block.start_column() + source.start_column().one_based_int() as u32;
    let end_column_is_on_start_line = line_count == 0;
    let end_column =
        unlinked_code_block.end_column() + if end_column_is_on_start_line { start_column } else { 1 };
    executable.record_parse(
        unlinked_code_block.code_features(),
        unlinked_code_block.lexically_scoped_features(),
        unlinked_code_block.has_captured_variables(),
        source.first_line().one_based_int() + line_count as i32,
        end_column,
    );
    if let Some(provider) = source.provider() {
        if !unlinked_code_block.source_url_directive().is_null() {
            provider.set_source_url_directive(unlinked_code_block.source_url_directive());
        }
        if !unlinked_code_block.source_mapping_url_directive().is_null() {
            provider.set_source_mapping_url_directive(unlinked_code_block.source_mapping_url_directive());
        }
    }
}

/// `class CodeCache`: caches top-level code such as <script>, window.eval(), new Function, and
/// JSEvaluateScript(). O `CodeCacheMap` fica atrás de `RefCell` porque o `VM::codeCache()` do C++ devolve
/// ponteiro mutável.
#[derive(Default)]
pub struct CodeCache {
    source_code: RefCell<CodeCacheMap>,
}

impl std::fmt::Debug for CodeCache {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("CodeCache").finish_non_exhaustive()
    }
}

impl CodeCache {
    /// `getUnlinkedGlobalCodeBlock<UnlinkedCodeBlockType, ExecutableType>(...)`.
    fn get_unlinked_global_code_block<T: CacheTypes>(
        &self,
        vm: &Rc<VM>,
        executable: &dyn GlobalCodeExecutable,
        source: &SourceCode,
        script_mode: JSParserScriptMode,
        code_generation_mode: CodeGenerationModeSet,
        error: &mut ParserError,
        eval_context_type: EvalContextType,
    ) -> Option<Rc<RefCell<T>>> {
        let derived_context_type = executable.derived_context_type();
        let is_arrow_function_context = executable.is_arrow_function_context();
        let key = SourceCodeKey::with(
            source,
            &WtfString::default(),
            T::CODE_TYPE,
            executable.lexically_scoped_features(),
            script_mode,
            derived_context_type,
            eval_context_type,
            is_arrow_function_context,
            code_generation_mode.to_raw(),
            None,
        );
        let unlinked_code_block = self.source_code.borrow_mut().find_cache_and_update_age::<T>(&key);
        if let Some(unlinked_code_block) = unlinked_code_block {
            if Options::use_code_cache() {
                record_parse_from_unlinked_code_block(&executable.script_ref(), source, &unlinked_code_block.borrow());
                return Some(unlinked_code_block);
            }
        }

        let unlinked_code_block = generate_unlinked_code_block::<T>(
            vm,
            executable,
            source,
            script_mode,
            code_generation_mode,
            error,
            eval_context_type,
            None,
            None,
        );

        if let Some(unlinked_code_block) = &unlinked_code_block {
            if Options::use_code_cache() {
                let mut source_code = self.source_code.borrow_mut();
                let age = source_code.age();
                source_code.add_cache(&key, SourceCodeValue { cell: T::into_cell(Rc::clone(unlinked_code_block)), age });
            }
        }

        unlinked_code_block
    }

    /// `getUnlinkedProgramCodeBlock`.
    pub fn get_unlinked_program_code_block(
        &self,
        vm: &Rc<VM>,
        executable: &Rc<RefCell<ProgramExecutable>>,
        source: &SourceCode,
        code_generation_mode: CodeGenerationModeSet,
        error: &mut ParserError,
    ) -> Option<Rc<RefCell<UnlinkedProgramCodeBlock>>> {
        self.get_unlinked_global_code_block::<UnlinkedProgramCodeBlock>(
            vm,
            executable,
            source,
            JSParserScriptMode::Classic,
            code_generation_mode,
            error,
            EvalContextType::None,
        )
    }

    /// `getUnlinkedEvalCodeBlock`.
    pub fn get_unlinked_eval_code_block(
        &self,
        vm: &Rc<VM>,
        executable: &Rc<RefCell<EvalExecutable>>,
        source: &SourceCode,
        code_generation_mode: CodeGenerationModeSet,
        error: &mut ParserError,
        eval_context_type: EvalContextType,
    ) -> Option<Rc<RefCell<UnlinkedEvalCodeBlock>>> {
        self.get_unlinked_global_code_block::<UnlinkedEvalCodeBlock>(
            vm,
            executable,
            source,
            JSParserScriptMode::Classic,
            code_generation_mode,
            error,
            eval_context_type,
        )
    }

    /// `getUnlinkedModuleProgramCodeBlock`.
    pub fn get_unlinked_module_program_code_block(
        &self,
        vm: &Rc<VM>,
        executable: &Rc<RefCell<ModuleProgramExecutable>>,
        source: &SourceCode,
        code_generation_mode: CodeGenerationModeSet,
        error: &mut ParserError,
    ) -> Option<Rc<RefCell<UnlinkedModuleProgramCodeBlock>>> {
        self.get_unlinked_global_code_block::<UnlinkedModuleProgramCodeBlock>(
            vm,
            executable,
            source,
            JSParserScriptMode::Module,
            code_generation_mode,
            error,
            EvalContextType::None,
        )
    }

    /// `getUnlinkedGlobalFunctionExecutable(...)`: o `UnlinkedFunctionExecutable` de `new Function(...)`.
    #[allow(clippy::too_many_arguments)]
    pub fn get_unlinked_global_function_executable(
        &self,
        vm: &Rc<VM>,
        name: &Identifier,
        source: &SourceCode,
        lexically_scoped_features: LexicallyScopedFeatures,
        code_generation_mode: CodeGenerationModeSet,
        function_constructor_parameters_end_position: Option<i32>,
        error: &mut ParserError,
    ) -> Option<UnlinkedFunctionExecutableRef> {
        let is_arrow_function_context = false;
        let key = SourceCodeKey::with(
            source,
            name.string().string(),
            SourceCodeType::FunctionType,
            lexically_scoped_features,
            JSParserScriptMode::Classic,
            DerivedContextType::None,
            EvalContextType::FunctionEvalContext,
            is_arrow_function_context,
            code_generation_mode.to_raw(),
            function_constructor_parameters_end_position,
        );
        let executable = self.source_code.borrow_mut().find_cache_and_update_age::<UnlinkedFunctionExecutable>(&key);
        if let Some(executable) = executable {
            if Options::use_code_cache() {
                if let Some(provider) = source.provider() {
                    let source_url_directive = executable.borrow().source_url_directive();
                    if !source_url_directive.is_null() {
                        provider.set_source_url_directive(&source_url_directive);
                    }
                    let source_mapping_url_directive = executable.borrow().source_mapping_url_directive();
                    if !source_mapping_url_directive.is_null() {
                        provider.set_source_mapping_url_directive(&source_mapping_url_directive);
                    }
                }
                return Some(executable);
            }
        }

        let mut position_before_last_newline = JSTextPosition::default();
        let program = parse_function_for_function_constructor(
            vm,
            source,
            lexically_scoped_features,
            error,
            Some(&mut position_before_last_newline),
            function_constructor_parameters_end_position,
        );
        let Some(program) = program else {
            assert!(error.is_valid());
            return None;
        };

        // This function assumes an input string that would result in a single function declaration.
        let func_decl = program.single_statement();
        let Some(Statement::FuncDecl(func_decl)) = func_decl else {
            // `if (!funcDecl) [[unlikely]]` (o `ASSERT(funcDecl->isFuncDeclNode())` garante o outro caso).
            let token = JSToken::default();
            *error = ParserError::with_message(
                ErrorType::SyntaxError,
                SyntaxErrorType::SyntaxErrorIrrecoverable,
                token,
                &WtfString::from_latin1(b"Parser error"),
                -1,
            );
            return None;
        };

        let metadata = Rc::clone(&func_decl.borrow().metadata);
        metadata.override_name(name);
        metadata.set_end_position(position_before_last_newline);
        // The Function constructor only has access to global variables, so no variables will be under TDZ unless
        // they're in the global lexical environment, which we always TDZ check accesses from.
        let construct_ability = construct_ability_for_parse_mode(metadata.parse_mode);
        let function_executable = UnlinkedFunctionExecutable::create(
            vm,
            source,
            &metadata,
            UnlinkedFunctionKind::UnlinkedNormalFunction,
            construct_ability,
            InlineAttribute::None,
            JSParserScriptMode::Classic,
            None,
            Vec::new(),
            None,
            DerivedContextType::None,
            EvalContextType::FunctionEvalContext,
            NeedsClassFieldInitializer::No,
            PrivateBrandRequirement::None,
        );

        if let Some(provider) = source.provider() {
            let source_url_directive = provider.source_url_directive();
            if !source_url_directive.is_null() {
                function_executable.borrow_mut().set_source_url_directive(source_url_directive);
            }
            let source_mapping_url_directive = provider.source_mapping_url_directive();
            if !source_mapping_url_directive.is_null() {
                function_executable.borrow_mut().set_source_mapping_url_directive(source_mapping_url_directive);
            }
        }

        // We initially start with hasCapturedVariables = false.
        function_executable.borrow_mut().record_parse(
            program.features,
            metadata.lexically_scoped_features,
            /* hasCapturedVariables */ false,
        );

        if Options::use_code_cache() {
            let mut source_code = self.source_code.borrow_mut();
            let age = source_code.age();
            source_code.add_cache(
                &key,
                SourceCodeValue { cell: CodeCacheCell::Function(Rc::clone(&function_executable)), age },
            );
        }
        Some(function_executable)
    }

    /// `clear()`: sem o `write()` (ver o topo do módulo).
    pub fn clear(&self) {
        self.source_code.borrow_mut().clear();
    }
}
