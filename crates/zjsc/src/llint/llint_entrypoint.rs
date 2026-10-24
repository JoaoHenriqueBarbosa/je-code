//! Porte de `llint/LLIntEntrypoint.h` e `.cpp`: escolhe o `JITCode` de entrada de um `CodeBlock`.
//!
//! `ENABLE(JIT)` é 0, então o ramo `Options::useJIT()` não existe; `ENABLE(C_LOOP)` é 0 e
//! `CPU(ARM64E)` é falso, então vale o ramo `getCodeRef<JSEntryPtrTag>(llint_*_prologue)`, que aqui
//! é o `LLIntEntry` correspondente (ver `llint_jit_code.rs`). O `std::call_once` por tipo de
//! entrada, que compartilha um único `JITCode` entre todos os `CodeBlock`s, vira o cache
//! `SHARED_CODE` (uma thread por `VM`, como o resto do porte).

use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

use crate::bytecode::code_block::CodeBlockRef;
use crate::bytecode::code_type::CodeType;
use crate::bytecode::opcode_size::OpcodeSize;
use crate::llint::llint_jit_code::{LLIntEntry, LLIntJITCode};
use crate::runtime::code_specialization_kind::CodeSpecializationKind;
use crate::runtime::executable::JITCode;
use crate::runtime::stack_alignment::{
    round_local_register_count_for_frame_pointer_offset, stack_alignment_registers,
};
use crate::wtf::code_ptr::CodePtr;
use crate::wtf::ptr_tag::{JSEntryPtrTag, NoPtrTag};

/// `maxFrameExtentForSlowPathCallInRegisters` (`assembler/MaxFrameExtentForSlowPathCall.h`): 0 em
/// x86_64, onde todos os argumentos vão em registradores.
pub const MAX_FRAME_EXTENT_FOR_SLOW_PATH_CALL_IN_REGISTERS: u32 = 0;

thread_local! {
    /// Os `static DirectJITCode*`/`static NativeJITCode*` de cada `setXEntrypoint`, indexados pela
    /// entrada principal.
    static SHARED_CODE: RefCell<HashMap<LLIntEntry, Rc<dyn JITCode>>> = RefCell::new(HashMap::new());
}

/// O `std::call_once` + `codeBlock->setJITCode(*jitCode)` comum aos quatro `setXEntrypoint`.
fn install_shared_code(code_block: &CodeBlockRef, code: LLIntJITCode, entry: LLIntEntry) {
    let shared = SHARED_CODE.with(|cache| {
        Rc::clone(cache.borrow_mut().entry(entry).or_insert_with(|| Rc::new(code) as Rc<dyn JITCode>))
    });
    code_block.borrow_mut().set_jit_code(shared);
}

/// `setFunctionEntrypoint`.
pub fn setup_function_entrypoint(code_block: &CodeBlockRef) {
    let kind = code_block.borrow().specialization_kind();
    let (entry, arity_check) = match kind {
        CodeSpecializationKind::CodeForCall => {
            (LLIntEntry::FunctionForCallPrologue, LLIntEntry::FunctionForCallArityCheck)
        }
        CodeSpecializationKind::CodeForConstruct => {
            (LLIntEntry::FunctionForConstructPrologue, LLIntEntry::FunctionForConstructArityCheck)
        }
    };
    install_shared_code(code_block, LLIntJITCode::direct(entry, arity_check), entry);
}

/// `setEvalEntrypoint`.
pub fn setup_eval_entrypoint(code_block: &CodeBlockRef) {
    install_shared_code(code_block, LLIntJITCode::native(LLIntEntry::EvalPrologue), LLIntEntry::EvalPrologue);
}

/// `setProgramEntrypoint`.
pub fn setup_program_entrypoint(code_block: &CodeBlockRef) {
    install_shared_code(code_block, LLIntJITCode::native(LLIntEntry::ProgramPrologue), LLIntEntry::ProgramPrologue);
}

/// `setModuleProgramEntrypoint`.
pub fn setup_module_program_entrypoint(code_block: &CodeBlockRef) {
    install_shared_code(
        code_block,
        LLIntJITCode::native(LLIntEntry::ModuleProgramPrologue),
        LLIntEntry::ModuleProgramPrologue,
    );
}

/// `arityFixup()`: sem JIT não há o thunk de arity fixup (`return nullptr`).
pub fn arity_fixup() -> CodePtr<NoPtrTag> {
    CodePtr::null()
}

/// `genericReturnPointEntrypoint(OpcodeSize)`.
pub fn generic_return_point_entrypoint(size: OpcodeSize) -> CodePtr<JSEntryPtrTag> {
    match size {
        OpcodeSize::Narrow => LLIntEntry::GenericReturnPointNarrow,
        OpcodeSize::Wide16 => LLIntEntry::GenericReturnPointWide16,
        OpcodeSize::Wide32 => LLIntEntry::GenericReturnPointWide32,
    }
    .code_ptr()
}

/// `LLInt::setEntrypoint(CodeBlock*)`.
pub fn set_entrypoint(code_block: &CodeBlockRef) {
    let code_type = code_block.borrow().code_type();
    match code_type {
        CodeType::GlobalCode => setup_program_entrypoint(code_block),
        CodeType::ModuleCode => setup_module_program_entrypoint(code_block),
        CodeType::EvalCode => setup_eval_entrypoint(code_block),
        CodeType::FunctionCode => setup_function_entrypoint(code_block),
    }
}

/// `LLInt::frameRegisterCountFor(CodeBlock*)`.
pub fn frame_register_count_for(code_block: &CodeBlockRef) -> u32 {
    let num_callee_locals = code_block.borrow().num_callee_locals();
    debug_assert!(num_callee_locals % stack_alignment_registers() == 0);
    round_local_register_count_for_frame_pointer_offset(
        num_callee_locals + MAX_FRAME_EXTENT_FOR_SLOW_PATH_CALL_IN_REGISTERS,
    )
}
