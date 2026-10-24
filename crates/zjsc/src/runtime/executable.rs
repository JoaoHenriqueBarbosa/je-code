//! Tradução de `runtime/ExecutableBase.h`, `ExecutableBase.cpp` e `ExecutableBaseInlines.h`.
//!
//! DIVERGÊNCIAS (modelo de dados, CONVENTIONS itens 1 e 2, sem heap de células ainda):
//!
//! - `ExecutableBase` herda de `JSCell`. O `JSCell` ainda não existe, então a base guarda só o que
//!   o `type()` do C++ lê da `Structure` (`cell_type`, um `JSType`). A estrutura em si
//!   (`Structure*`) entra com o `JSCell`.
//! - Os ponteiros `ExecutableBase*` e `ScriptExecutable*` do C++, de onde se faz
//!   `uncheckedDowncast<Derivada>(this)`, viram os enums `ExecutableBaseRef` (aqui) e
//!   `ScriptExecutableRef` (`script_executable.rs`), um por família, como a convenção faz com os
//!   nós da árvore. Os métodos da base que desviam por `type()` vivem nesses enums; os que só
//!   leem estado da base vivem nas structs.
//! - `RefPtr<JITCode>` é `Option<Rc<dyn JITCode>>`, e `CodePtr<JSEntryPtrTag>` nulo é `None`.
//!   O `JITCode` aqui é o do LLInt (`LLIntJITCode`, de `jit/JITCode.h`): o interpretador não gera
//!   código de máquina, o `CodePtr` é o entrypoint dele.
//! - `friend class JIT`, `boundFunctionCallGenerator`, `offsetOf...` (offsets para o assembly do
//!   JIT/LLInt) não existem: o interpretador Rust lê os campos direto.
//! - `needsDestruction`, `destroy`, `subspaceFor`, `DECLARE_EXPORT_INFO` e `s_info` são
//!   maquinaria de GC e do `ClassInfo`; não há destruidor a registrar.

use std::fmt;
use std::rc::Rc;

use crate::bytecode::code_block_hash::CodeBlockHash;
use crate::runtime::arity_check_mode::ArityCheckMode;
use crate::runtime::code_specialization_kind::CodeSpecializationKind;
use crate::runtime::implementation_visibility::ImplementationVisibility;
use crate::runtime::inline_attribute::InlineAttribute;
use crate::runtime::intrinsic::Intrinsic;
use crate::runtime::js_type::JSType;
use crate::runtime::native_executable::NativeExecutableRef;
use crate::runtime::script_executable::ScriptExecutableRef;
use crate::wtf::code_ptr::CodePtr;
use crate::wtf::ptr_tag::JSEntryPtrTag;

/// A fatia de `JITCode` (`jit/JITCode.h`) que o `ExecutableBase` lê: `addressForCall`. DIVERGÊNCIA:
/// o módulo `jit` não existe (sem JIT, CONVENTIONS), então o `RefPtr<JITCode>` vira
/// `Rc<dyn JITCode>` e quem o implementa é o `LLIntJITCode` quando o `llint` for portado.
pub trait JITCode {
    /// `JITCode::addressForCall(ArityCheckMode)`.
    fn address_for_call(&self, arity_check: ArityCheckMode) -> CodePtr<JSEntryPtrTag>;
    /// `JITCode::intrinsic()` (`m_intrinsic { NoIntrinsic }`, só efetivo no `NativeExecutable`).
    fn intrinsic(&self) -> Intrinsic {
        Intrinsic::NoIntrinsic
    }
}

/// `enum class CompilationKind { FirstCompilation, OptimizingCompilation }`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CompilationKind {
    FirstCompilation,
    OptimizingCompilation,
}

/// `isCall(CodeSpecializationKind)`.
pub fn is_call(kind: CodeSpecializationKind) -> bool {
    kind == CodeSpecializationKind::CodeForCall
}

/// `class ExecutableBase`: o estado compartilhado por `NativeExecutable` e `ScriptExecutable`.
pub struct ExecutableBase {
    /// `JSCell::type()`, que o C++ lê do `TypeInfo` da `Structure` passada ao construtor.
    cell_type: JSType,
    pub(crate) jit_code_for_call: Option<Rc<dyn JITCode>>,
    pub(crate) jit_code_for_construct: Option<Rc<dyn JITCode>>,
    pub(crate) jit_code_for_call_with_arity_check: Option<CodePtr<JSEntryPtrTag>>,
    pub(crate) jit_code_for_construct_with_arity_check: Option<CodePtr<JSEntryPtrTag>>,
}

impl ExecutableBase {
    /// `ExecutableBase(VM&, Structure*)`. `cell_type` é o tipo que a `Structure` carrega.
    pub fn new(cell_type: JSType) -> ExecutableBase {
        ExecutableBase {
            cell_type,
            jit_code_for_call: None,
            jit_code_for_construct: None,
            jit_code_for_call_with_arity_check: None,
            jit_code_for_construct_with_arity_check: None,
        }
    }

    /// `JSCell::type()`.
    pub fn type_(&self) -> JSType {
        self.cell_type
    }

    pub fn is_eval_executable(&self) -> bool {
        self.type_() == JSType::EvalExecutableType
    }

    pub fn is_function_executable(&self) -> bool {
        self.type_() == JSType::FunctionExecutableType
    }

    pub fn is_program_executable(&self) -> bool {
        self.type_() == JSType::ProgramExecutableType
    }

    pub fn is_module_program_executable(&self) -> bool {
        self.type_() == JSType::ModuleProgramExecutableType
    }

    pub fn is_host_function(&self) -> bool {
        self.type_() == JSType::NativeExecutableType
    }

    /// `generatedJITCodeForCall()`: `ASSERT(m_jitCodeForCall)`.
    pub fn generated_jit_code_for_call(&self) -> Rc<dyn JITCode> {
        debug_assert!(self.jit_code_for_call.is_some());
        Rc::clone(self.jit_code_for_call.as_ref().expect("m_jitCodeForCall"))
    }

    /// `generatedJITCodeForConstruct()`: `ASSERT(m_jitCodeForConstruct)`.
    pub fn generated_jit_code_for_construct(&self) -> Rc<dyn JITCode> {
        debug_assert!(self.jit_code_for_construct.is_some());
        Rc::clone(self.jit_code_for_construct.as_ref().expect("m_jitCodeForConstruct"))
    }

    /// `generatedJITCodeAddressForCall()`.
    pub fn generated_jit_code_address_for_call(&self) -> CodePtr<JSEntryPtrTag> {
        debug_assert!(self.jit_code_for_call.is_some());
        self.jit_code_for_call.as_ref().expect("m_jitCodeForCall").address_for_call(ArityCheckMode::ArityCheckNotRequired)
    }

    /// `generatedJITCodeFor(CodeSpecializationKind)`.
    pub fn generated_jit_code_for(&self, kind: CodeSpecializationKind) -> Rc<dyn JITCode> {
        match kind {
            CodeSpecializationKind::CodeForCall => self.generated_jit_code_for_call(),
            CodeSpecializationKind::CodeForConstruct => self.generated_jit_code_for_construct(),
        }
    }

    /// `generatedJITCodeWithArityCheckForCall()`.
    pub fn generated_jit_code_with_arity_check_for_call(&self) -> Option<CodePtr<JSEntryPtrTag>> {
        self.jit_code_for_call_with_arity_check.clone()
    }

    /// `generatedJITCodeWithArityCheckForConstruct()`.
    pub fn generated_jit_code_with_arity_check_for_construct(&self) -> Option<CodePtr<JSEntryPtrTag>> {
        self.jit_code_for_construct_with_arity_check.clone()
    }

    /// `generatedJITCodeWithArityCheckFor(CodeSpecializationKind)`.
    pub fn generated_jit_code_with_arity_check_for(&self, kind: CodeSpecializationKind) -> Option<CodePtr<JSEntryPtrTag>> {
        match kind {
            CodeSpecializationKind::CodeForCall => self.generated_jit_code_with_arity_check_for_call(),
            CodeSpecializationKind::CodeForConstruct => self.generated_jit_code_with_arity_check_for_construct(),
        }
    }

    /// `entrypointFor(CodeSpecializationKind, ArityCheckMode)`.
    pub fn entrypoint_for(&mut self, kind: CodeSpecializationKind, arity: ArityCheckMode) -> CodePtr<JSEntryPtrTag> {
        // Check if we have a cached result. We only have it for arity check because we use the
        // no-arity entrypoint in non-virtual calls, which will "cache" this value directly in
        // machine code.
        if arity == ArityCheckMode::MustCheckArity {
            match kind {
                CodeSpecializationKind::CodeForCall => {
                    if let Some(result) = &self.jit_code_for_call_with_arity_check {
                        return result.clone();
                    }
                }
                CodeSpecializationKind::CodeForConstruct => {
                    if let Some(result) = &self.jit_code_for_construct_with_arity_check {
                        return result.clone();
                    }
                }
            }
        }
        let result = self.generated_jit_code_for(kind).address_for_call(arity);
        if arity == ArityCheckMode::MustCheckArity {
            // Cache the result; this is necessary for the JIT's virtual call optimizations.
            match kind {
                CodeSpecializationKind::CodeForCall => self.jit_code_for_call_with_arity_check = Some(result.clone()),
                CodeSpecializationKind::CodeForConstruct => {
                    self.jit_code_for_construct_with_arity_check = Some(result.clone())
                }
            }
        }
        result
    }

    /// `swapGeneratedJITCodeWithArityCheckForDebugger(kind, ...)`.
    pub fn swap_generated_jit_code_with_arity_check_for_debugger(
        &mut self,
        kind: CodeSpecializationKind,
        jit_code_with_arity_check: Option<CodePtr<JSEntryPtrTag>>,
    ) -> Option<CodePtr<JSEntryPtrTag>> {
        match kind {
            CodeSpecializationKind::CodeForCall => {
                self.swap_generated_jit_code_for_call_with_arity_check_for_debugger(jit_code_with_arity_check)
            }
            CodeSpecializationKind::CodeForConstruct => {
                self.swap_generated_jit_code_for_construct_with_arity_check_for_debugger(jit_code_with_arity_check)
            }
        }
    }

    /// `swapGeneratedJITCodeForCallWithArityCheckForDebugger`.
    pub fn swap_generated_jit_code_for_call_with_arity_check_for_debugger(
        &mut self,
        jit_code_for_call_with_arity_check: Option<CodePtr<JSEntryPtrTag>>,
    ) -> Option<CodePtr<JSEntryPtrTag>> {
        std::mem::replace(&mut self.jit_code_for_call_with_arity_check, jit_code_for_call_with_arity_check)
    }

    /// `swapGeneratedJITCodeForConstructWithArityCheckForDebugger`.
    pub fn swap_generated_jit_code_for_construct_with_arity_check_for_debugger(
        &mut self,
        jit_code_for_construct_with_arity_check: Option<CodePtr<JSEntryPtrTag>>,
    ) -> Option<CodePtr<JSEntryPtrTag>> {
        std::mem::replace(
            &mut self.jit_code_for_construct_with_arity_check,
            jit_code_for_construct_with_arity_check,
        )
    }

    /// `intrinsicFor(CodeSpecializationKind)` (ExecutableBaseInlines.h): `intrinsic()` só vale para
    /// chamada, o chamador passa o valor que o `intrinsic()` do `ExecutableBaseRef` devolveu.
    pub fn intrinsic_for(kind: CodeSpecializationKind, intrinsic: Intrinsic) -> Intrinsic {
        if is_call(kind) {
            return intrinsic;
        }
        Intrinsic::NoIntrinsic
    }
}

/// `ExecutableBase*` com o tipo dinâmico resolvido: o destino de `uncheckedDowncast` em
/// `ExecutableBase.cpp` e `ExecutableBaseInlines.h`.
#[derive(Clone)]
pub enum ExecutableBaseRef {
    /// `NativeExecutableType`.
    Native(NativeExecutableRef),
    /// Os quatro `ScriptExecutable` concretos.
    Script(ScriptExecutableRef),
}

impl ExecutableBaseRef {
    /// `jsDynamicCast<FunctionExecutable*>(executable)`.
    pub fn as_function_executable(&self) -> Option<crate::runtime::js_function::FunctionExecutableRef> {
        match self {
            ExecutableBaseRef::Script(ScriptExecutableRef::Function(function)) => Some(Rc::clone(function)),
            _ => None,
        }
    }

    /// `ExecutableBase::hashFor(CodeSpecializationKind)`.
    pub fn hash_for(&self, kind: CodeSpecializationKind) -> CodeBlockHash {
        match self {
            ExecutableBaseRef::Native(native) => native.borrow().hash_for(kind),
            ExecutableBaseRef::Script(script) => script.hash_for(kind),
        }
    }

    /// `ExecutableBase::intrinsic()`.
    pub fn intrinsic(&self) -> Intrinsic {
        match self {
            ExecutableBaseRef::Native(native) => native.borrow().intrinsic(),
            ExecutableBaseRef::Script(script) => script.intrinsic(),
        }
    }

    /// `ExecutableBase::intrinsicFor(CodeSpecializationKind)`.
    pub fn intrinsic_for(&self, kind: CodeSpecializationKind) -> Intrinsic {
        ExecutableBase::intrinsic_for(kind, self.intrinsic())
    }

    /// `ExecutableBase::implementationVisibility()`.
    pub fn implementation_visibility(&self) -> ImplementationVisibility {
        match self {
            ExecutableBaseRef::Native(native) => native.borrow().implementation_visibility(),
            ExecutableBaseRef::Script(ScriptExecutableRef::Function(function)) => {
                function.borrow().implementation_visibility()
            }
            ExecutableBaseRef::Script(_) => ImplementationVisibility::Public,
        }
    }

    /// `ExecutableBase::inlineAttribute()`.
    pub fn inline_attribute(&self) -> InlineAttribute {
        match self {
            ExecutableBaseRef::Script(ScriptExecutableRef::Function(function)) => function.borrow().inline_attribute(),
            _ => InlineAttribute::None,
        }
    }

    /// `ExecutableBase::hasJITCodeForCall()`.
    pub fn has_jit_code_for_call(&self) -> bool {
        match self {
            ExecutableBaseRef::Native(_) => true,
            ExecutableBaseRef::Script(script) => script.has_jit_code_for_call(),
        }
    }

    /// `ExecutableBase::hasJITCodeForConstruct()`.
    pub fn has_jit_code_for_construct(&self) -> bool {
        match self {
            ExecutableBaseRef::Native(_) => true,
            ExecutableBaseRef::Script(script) => script.has_jit_code_for_construct(),
        }
    }

    /// `ExecutableBase::hasJITCodeFor(CodeSpecializationKind)`.
    pub fn has_jit_code_for(&self, kind: CodeSpecializationKind) -> bool {
        match kind {
            CodeSpecializationKind::CodeForCall => self.has_jit_code_for_call(),
            CodeSpecializationKind::CodeForConstruct => self.has_jit_code_for_construct(),
        }
    }

    /// `ExecutableBase::dump(PrintStream&)`. O `PrintStream` vira `fmt::Write`.
    pub fn dump(&self, out: &mut dyn fmt::Write) -> fmt::Result {
        match self {
            ExecutableBaseRef::Native(native) => {
                let native = native.borrow();
                write!(out, "NativeExecutable:{:#x}/{:#x}", native.function().tagged_ptr(), native.constructor().tagged_ptr())
            }
            ExecutableBaseRef::Script(script) => script.dump(out),
        }
    }
}
