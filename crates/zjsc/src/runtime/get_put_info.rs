//! Tradução de `runtime/GetPutInfo.h` e `.cpp`.
//!
//! Caminhos que passam a valer (conferidos por grep em `src/bytecompiler`):
//! - `crate::runtime::get_put_info::{ResolveMode, ResolveType, InitializationMode, GetPutInfo}`:
//!   já citados assim em `nodes_codegen_cpp3b`, `nodes_codegen_cpp5b`, `bytecode_generator_cpp1/3/4`.
//! - `crate::runtime::ecma_mode::ECMAMode` (`ECMAMode.h` é outro módulo, criado junto): é o caminho de
//!   `bytecode_generator_cpp2`. Divergem e precisam ser acertados pelos donos: `bytecode_generator_part3`,
//!   `bytecode_generator_cpp4` e `nodes_codegen_cpp2` citam `crate::parser::parser_modes::ECMAMode`
//!   (o tipo não existe lá), e `bytecode_generator_cpp1`/`cpp1c` usam `ECMAMode` sem caminho confirmado.
//! - Nenhum `use` cita `code_type` nem `resolve_type` como módulo: `ResolveType` mora aqui.
//!
//! Fora deste módulo: `friend class LLIntOffsetsExtractor`.

use std::fmt;

use crate::runtime::ecma_mode::ECMAMode;

/// `enum ResolveMode`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[repr(u32)]
pub enum ResolveMode {
    ThrowIfNotFound = 0,
    DoNotThrowIfNotFound = 1,
}

impl ResolveMode {
    pub(crate) fn from_u32(value: u32) -> ResolveMode {
        match value {
            0 => ResolveMode::ThrowIfNotFound,
            1 => ResolveMode::DoNotThrowIfNotFound,
            _ => panic!("ResolveMode inválido: {value}"),
        }
    }
}

/// `enum ResolveType : unsigned`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[repr(u32)]
pub enum ResolveType {
    // Lexical scope guaranteed a certain type of variable access.
    GlobalProperty = 0,
    GlobalVar = 1,
    GlobalLexicalVar = 2,
    ClosureVar = 3,
    ResolvedClosureVar = 4,
    ModuleVar = 5,

    // Ditto, but at least one intervening scope used non-strict eval, which
    // can inject an intercepting var declaration at runtime.
    GlobalPropertyWithVarInjectionChecks = 6,
    GlobalVarWithVarInjectionChecks = 7,
    GlobalLexicalVarWithVarInjectionChecks = 8,
    ClosureVarWithVarInjectionChecks = 9,

    // We haven't found which scope this belongs to, and we also
    // haven't ruled out the possibility of it being cached.
    UnresolvedProperty = 10,
    UnresolvedPropertyWithVarInjectionChecks = 11,

    // Lexical scope didn't prove anything, probably because of a 'with' scope.
    Dynamic = 12,
}

/// `FOR_EACH_RESOLVE_TYPE`, na ordem do C++.
pub const FOR_EACH_RESOLVE_TYPE: [ResolveType; 13] = [
    ResolveType::GlobalProperty,
    ResolveType::GlobalVar,
    ResolveType::GlobalLexicalVar,
    ResolveType::ClosureVar,
    ResolveType::ResolvedClosureVar,
    ResolveType::ModuleVar,
    ResolveType::GlobalPropertyWithVarInjectionChecks,
    ResolveType::GlobalVarWithVarInjectionChecks,
    ResolveType::GlobalLexicalVarWithVarInjectionChecks,
    ResolveType::ClosureVarWithVarInjectionChecks,
    ResolveType::UnresolvedProperty,
    ResolveType::UnresolvedPropertyWithVarInjectionChecks,
    ResolveType::Dynamic,
];

impl ResolveType {
    pub(crate) fn from_u32(value: u32) -> ResolveType {
        match FOR_EACH_RESOLVE_TYPE.get(value as usize) {
            Some(resolve_type) => *resolve_type,
            None => panic!("ResolveType inválido: {value}"),
        }
    }
}

/// `enum class InitializationMode : unsigned`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[repr(u32)]
pub enum InitializationMode {
    Initialization = 0,                // "let x = 20;"
    ConstInitialization = 1,           // "const x = 20;"
    NotInitialization = 2,             // "x = 20;"
    ScopedArgumentInitialization = 3,  // Assign to scoped argument, com semântica de NotInitialization.
}

impl InitializationMode {
    pub(crate) fn from_u32(value: u32) -> InitializationMode {
        match value {
            0 => InitializationMode::Initialization,
            1 => InitializationMode::ConstInitialization,
            2 => InitializationMode::NotInitialization,
            3 => InitializationMode::ScopedArgumentInitialization,
            _ => panic!("InitializationMode inválido: {value}"),
        }
    }
}

/// `resolveModeName`.
pub fn resolve_mode_name(resolve_mode: ResolveMode) -> &'static str {
    match resolve_mode {
        ResolveMode::ThrowIfNotFound => "ThrowIfNotFound",
        ResolveMode::DoNotThrowIfNotFound => "DoNotThrowIfNotFound",
    }
}

/// `resolveTypeName`.
pub fn resolve_type_name(resolve_type: ResolveType) -> &'static str {
    match resolve_type {
        ResolveType::GlobalProperty => "GlobalProperty",
        ResolveType::GlobalVar => "GlobalVar",
        ResolveType::GlobalLexicalVar => "GlobalLexicalVar",
        ResolveType::ClosureVar => "ClosureVar",
        ResolveType::ResolvedClosureVar => "ResolvedClosureVar",
        ResolveType::ModuleVar => "ModuleVar",
        ResolveType::GlobalPropertyWithVarInjectionChecks => "GlobalPropertyWithVarInjectionChecks",
        ResolveType::GlobalVarWithVarInjectionChecks => "GlobalVarWithVarInjectionChecks",
        ResolveType::GlobalLexicalVarWithVarInjectionChecks => "GlobalLexicalVarWithVarInjectionChecks",
        ResolveType::ClosureVarWithVarInjectionChecks => "ClosureVarWithVarInjectionChecks",
        ResolveType::UnresolvedProperty => "UnresolvedProperty",
        ResolveType::UnresolvedPropertyWithVarInjectionChecks => "UnresolvedPropertyWithVarInjectionChecks",
        ResolveType::Dynamic => "Dynamic",
    }
}

/// `initializationModeName`.
pub fn initialization_mode_name(initialization_mode: InitializationMode) -> &'static str {
    match initialization_mode {
        InitializationMode::Initialization => "Initialization",
        InitializationMode::ConstInitialization => "ConstInitialization",
        InitializationMode::NotInitialization => "NotInitialization",
        InitializationMode::ScopedArgumentInitialization => "ScopedArgumentInitialization",
    }
}

/// `isInitialization`.
pub fn is_initialization(initialization_mode: InitializationMode) -> bool {
    match initialization_mode {
        InitializationMode::Initialization | InitializationMode::ConstInitialization => true,
        InitializationMode::NotInitialization | InitializationMode::ScopedArgumentInitialization => false,
    }
}

/// `makeType`.
pub fn make_type(resolve_type: ResolveType, needs_var_injection_checks: bool) -> ResolveType {
    if !needs_var_injection_checks {
        return resolve_type;
    }

    match resolve_type {
        ResolveType::GlobalProperty => ResolveType::GlobalPropertyWithVarInjectionChecks,
        ResolveType::GlobalVar => ResolveType::GlobalVarWithVarInjectionChecks,
        ResolveType::GlobalLexicalVar => ResolveType::GlobalLexicalVarWithVarInjectionChecks,
        ResolveType::ClosureVar | ResolveType::ResolvedClosureVar => ResolveType::ClosureVarWithVarInjectionChecks,
        ResolveType::UnresolvedProperty => ResolveType::UnresolvedPropertyWithVarInjectionChecks,
        ResolveType::ModuleVar
        | ResolveType::GlobalPropertyWithVarInjectionChecks
        | ResolveType::GlobalVarWithVarInjectionChecks
        | ResolveType::GlobalLexicalVarWithVarInjectionChecks
        | ResolveType::ClosureVarWithVarInjectionChecks
        | ResolveType::UnresolvedPropertyWithVarInjectionChecks
        | ResolveType::Dynamic => resolve_type,
    }
}

/// `needsVarInjectionChecks`. O `default:` do C++ é inalcançável com o enum fechado.
pub fn needs_var_injection_checks(resolve_type: ResolveType) -> bool {
    match resolve_type {
        ResolveType::GlobalProperty
        | ResolveType::GlobalVar
        | ResolveType::GlobalLexicalVar
        | ResolveType::ClosureVar
        | ResolveType::ResolvedClosureVar
        | ResolveType::ModuleVar
        | ResolveType::UnresolvedProperty => false,
        ResolveType::GlobalPropertyWithVarInjectionChecks
        | ResolveType::GlobalVarWithVarInjectionChecks
        | ResolveType::GlobalLexicalVarWithVarInjectionChecks
        | ResolveType::ClosureVarWithVarInjectionChecks
        | ResolveType::UnresolvedPropertyWithVarInjectionChecks
        | ResolveType::Dynamic => true,
    }
}

/// `class GetPutInfo`: operando de 31 bits com quatro campos de 10 bits.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Default)]
pub struct GetPutInfo {
    operand: u32,
}

impl GetPutInfo {
    // Give each field 10 bits for simplicity.
    pub const IS_STRICT_SHIFT: u32 = 30;
    pub const MODE_SHIFT: u32 = 20;
    pub const INITIALIZATION_SHIFT: u32 = 10;
    pub const TYPE_BITS: u32 = (1 << Self::INITIALIZATION_SHIFT) - 1;
    pub const INITIALIZATION_BITS: u32 = ((1 << Self::MODE_SHIFT) - 1) & !Self::TYPE_BITS;
    pub const MODE_BITS: u32 = ((1 << 30) - 1) & !Self::INITIALIZATION_BITS & !Self::TYPE_BITS;
    pub const IS_STRICT_BIT: u32 = 1 << 30;

    pub fn new(
        resolve_mode: ResolveMode,
        resolve_type: ResolveType,
        initialization_mode: InitializationMode,
        ecma_mode: ECMAMode,
    ) -> GetPutInfo {
        GetPutInfo {
            operand: ((ecma_mode.is_strict() as u32) << Self::IS_STRICT_SHIFT)
                | ((resolve_mode as u32) << Self::MODE_SHIFT)
                | ((initialization_mode as u32) << Self::INITIALIZATION_SHIFT)
                | resolve_type as u32,
        }
    }

    /// `explicit GetPutInfo(unsigned operand)`.
    pub fn from_operand(operand: u32) -> GetPutInfo {
        GetPutInfo { operand }
    }

    pub fn resolve_type(&self) -> ResolveType {
        ResolveType::from_u32(self.operand & Self::TYPE_BITS)
    }

    pub fn initialization_mode(&self) -> InitializationMode {
        InitializationMode::from_u32((self.operand & Self::INITIALIZATION_BITS) >> Self::INITIALIZATION_SHIFT)
    }

    pub fn resolve_mode(&self) -> ResolveMode {
        ResolveMode::from_u32((self.operand & Self::MODE_BITS) >> Self::MODE_SHIFT)
    }

    pub fn ecma_mode(&self) -> ECMAMode {
        if self.operand & Self::IS_STRICT_BIT != 0 {
            ECMAMode::strict()
        } else {
            ECMAMode::sloppy()
        }
    }

    pub fn operand(&self) -> u32 {
        self.operand
    }

    /// `GetPutInfo::dump(PrintStream&)`.
    pub fn dump(&self, out: &mut dyn fmt::Write) -> fmt::Result {
        write!(out, "{}<", self.operand())?;
        print_resolve_mode(out, self.resolve_mode())?;
        out.write_str("|")?;
        print_resolve_type(out, self.resolve_type())?;
        out.write_str("|")?;
        print_initialization_mode(out, self.initialization_mode())?;
        out.write_str("|")?;
        self.ecma_mode().dump(out)?;
        out.write_str(">")
    }
}

/// `enum GetOrPut { Get, Put }`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum GetOrPut {
    Get,
    Put,
}

/// O `InlineWatchpointSet*` de `ResolveOp`: o conjunto de watchpoints da `SymbolTableEntry`. A entrada do
/// porte nunca o tem (`watchpointSet()` é sempre nulo), então hoje o campo é sempre `None`.
#[derive(Clone, Debug)]
pub struct ResolveWatchpointSet(pub crate::bytecode::watchpoint::WatchpointSetRef);

impl ResolveWatchpointSet {
    /// O "endereço" do conjunto, o que o metadata guarda.
    pub fn identity(&self) -> usize {
        std::rc::Rc::as_ptr(&self.0) as *const () as usize
    }

    /// `watchpointSet->invalidate(vm, PutToScopeFireDetail(codeBlock, ident))`.
    pub fn invalidate_for_put_to_scope(
        &self,
        vm: &crate::runtime::vm::VM,
        _code_block: &crate::bytecode::code_block::CodeBlockRef,
        _ident: &crate::runtime::identifier::Identifier,
    ) {
        self.0.borrow_mut().invalidate_with_reason(vm, "Executed op_put_to_scope");
    }
}

/// `struct ResolveOp`.
#[derive(Clone)]
pub struct ResolveOp {
    pub type_: ResolveType,
    pub depth: u32,
    pub structure: Option<crate::runtime::structure::StructureRef>,
    /// `JSLexicalEnvironment*`: o escopo léxico ou de módulo (`JSScopeRef::LexicalEnvironment` ou `ModuleEnvironment`).
    pub lexical_environment: Option<crate::runtime::js_scope::JSScopeRef>,
    pub watchpoint_set: Option<ResolveWatchpointSet>,
    pub operand: usize,
    pub imported_name: Option<crate::wtf::text::string_impl::UniquedKey>,
}

impl ResolveOp {
    /// `ResolveOp(type, depth, structure, lexicalEnvironment, watchpointSet, operand, importedName)`.
    pub fn new(
        type_: ResolveType,
        depth: u32,
        structure: Option<crate::runtime::structure::StructureRef>,
        lexical_environment: Option<crate::runtime::js_scope::JSScopeRef>,
        watchpoint_set: Option<ResolveWatchpointSet>,
        operand: usize,
    ) -> ResolveOp {
        ResolveOp { type_, depth, structure, lexical_environment, watchpoint_set, operand, imported_name: None }
    }

    /// `ResolveOp(Dynamic, 0, nullptr, nullptr, nullptr, 0)`.
    pub fn dynamic() -> ResolveOp {
        ResolveOp::new(ResolveType::Dynamic, 0, None, None, None, 0)
    }
}

/// `WTF::printInternal(PrintStream&, JSC::ResolveMode)`.
pub fn print_resolve_mode(out: &mut dyn fmt::Write, mode: ResolveMode) -> fmt::Result {
    out.write_str(resolve_mode_name(mode))
}

/// `WTF::printInternal(PrintStream&, JSC::ResolveType)`.
pub fn print_resolve_type(out: &mut dyn fmt::Write, resolve_type: ResolveType) -> fmt::Result {
    out.write_str(resolve_type_name(resolve_type))
}

/// `WTF::printInternal(PrintStream&, JSC::InitializationMode)`.
pub fn print_initialization_mode(out: &mut dyn fmt::Write, mode: InitializationMode) -> fmt::Result {
    out.write_str(initialization_mode_name(mode))
}
