//! O `JITCode` do LLInt (`DirectJITCode`/`NativeJITCode` de `jit/JITCode.h` com
//! `JITType::InterpreterThunk`), na única forma que existe sem JIT.
//!
//! DIVERGÊNCIA: o C++ guarda ponteiros para os rótulos do assembly do LLInt
//! (`llint_program_prologue` etc., via `getCodeRef`). Aqui não há assembly: cada rótulo é um
//! `LLIntEntry`, e o `CodePtr<JSEntryPtrTag>` carrega o número dele (ver `wtf/code_ptr.rs`). O
//! laço de despacho do interpretador decodifica o `CodePtr` com `LLIntEntry::from_code_ptr`.

use crate::runtime::arity_check_mode::ArityCheckMode;
use crate::runtime::executable::JITCode;
use crate::wtf::code_ptr::CodePtr;
use crate::wtf::ptr_tag::JSEntryPtrTag;

/// Os rótulos de entrada do `LowLevelInterpreter*.asm` que o `LLIntEntrypoint.cpp` referencia.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[repr(usize)]
pub enum LLIntEntry {
    /// `llint_program_prologue`.
    ProgramPrologue = 1,
    /// `llint_module_program_prologue`.
    ModuleProgramPrologue = 2,
    /// `llint_eval_prologue`.
    EvalPrologue = 3,
    /// `llint_function_for_call_prologue`.
    FunctionForCallPrologue = 4,
    /// `llint_function_for_construct_prologue`.
    FunctionForConstructPrologue = 5,
    /// `llint_function_for_call_arity_check`.
    FunctionForCallArityCheck = 6,
    /// `llint_function_for_construct_arity_check`.
    FunctionForConstructArityCheck = 7,
    /// `llint_default_call_trampoline`.
    DefaultCallTrampoline = 8,
    /// `llint_get_host_call_return_value`.
    GetHostCallReturnValue = 9,
    /// `fuzzer_return_early_from_loop_hint`.
    FuzzerReturnEarlyFromLoopHint = 10,
    /// `llint_generic_return_point`, narrow.
    GenericReturnPointNarrow = 11,
    /// `llint_generic_return_point`, wide16.
    GenericReturnPointWide16 = 12,
    /// `llint_generic_return_point`, wide32.
    GenericReturnPointWide32 = 13,
}

const ALL_ENTRIES: [LLIntEntry; 13] = [
    LLIntEntry::ProgramPrologue,
    LLIntEntry::ModuleProgramPrologue,
    LLIntEntry::EvalPrologue,
    LLIntEntry::FunctionForCallPrologue,
    LLIntEntry::FunctionForConstructPrologue,
    LLIntEntry::FunctionForCallArityCheck,
    LLIntEntry::FunctionForConstructArityCheck,
    LLIntEntry::DefaultCallTrampoline,
    LLIntEntry::GetHostCallReturnValue,
    LLIntEntry::FuzzerReturnEarlyFromLoopHint,
    LLIntEntry::GenericReturnPointNarrow,
    LLIntEntry::GenericReturnPointWide16,
    LLIntEntry::GenericReturnPointWide32,
];

impl LLIntEntry {
    /// O `getCodePtr<JSEntryPtrTag>(label)`: o número do rótulo como `CodePtr`.
    pub fn code_ptr(self) -> CodePtr<JSEntryPtrTag> {
        CodePtr::new(self as usize)
    }

    /// O caminho inverso, usado pelo laço de despacho. `None` para nulo ou valor desconhecido.
    pub fn from_code_ptr(ptr: CodePtr<JSEntryPtrTag>) -> Option<LLIntEntry> {
        let value = ptr.tagged_ptr();
        ALL_ENTRIES.iter().copied().find(|entry| *entry as usize == value)
    }
}

/// `DirectJITCode` (função, com a entrada de verificação de aridade) e `NativeJITCode`
/// (programa, módulo e eval, sem ela), ambos `JITType::InterpreterThunk` e
/// `ShareAttribute::Shared`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct LLIntJITCode {
    entry: LLIntEntry,
    entry_with_arity_check: Option<LLIntEntry>,
}

impl LLIntJITCode {
    /// `NativeJITCode(codeRef, JITType::InterpreterThunk, Intrinsic::NoIntrinsic, Shared)`.
    pub fn native(entry: LLIntEntry) -> LLIntJITCode {
        LLIntJITCode { entry, entry_with_arity_check: None }
    }

    /// `DirectJITCode(ref, withArityCheck, JITType::InterpreterThunk, Shared)`.
    pub fn direct(entry: LLIntEntry, entry_with_arity_check: LLIntEntry) -> LLIntJITCode {
        LLIntJITCode { entry, entry_with_arity_check: Some(entry_with_arity_check) }
    }

}

/// `DirectJITCode::addressForCall` / `NativeJITCode::addressForCall`: o `NativeJITCode` ignora o
/// modo de aridade (`UNUSED_PARAM`); o `DirectJITCode` devolve a entrada com verificação quando
/// `MustCheckArity`.
impl JITCode for LLIntJITCode {
    fn address_for_call(&self, arity_check: ArityCheckMode) -> CodePtr<JSEntryPtrTag> {
        match (arity_check, self.entry_with_arity_check) {
            (ArityCheckMode::MustCheckArity, Some(with_arity_check)) => with_arity_check.code_ptr(),
            _ => self.entry.code_ptr(),
        }
    }
}
