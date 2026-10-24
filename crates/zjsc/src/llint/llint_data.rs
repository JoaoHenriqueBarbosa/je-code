//! Porte do que o interpretador por `match` consome de `llint/LLIntData.{h,cpp}` e
//! `LLIntOffsetsExtractor.cpp`.
//!
//! O `LLInt::initialize` do C++ preenche as tabelas `g_opcodeMap`/`g_opcodeMapWide16`/
//! `g_opcodeMapWide32` com o endereço de cada `llint_op_*` do assembly gerado e confere os
//! deslocamentos do `LLIntOffsetsExtractor`. Aqui o despacho é um `match` sobre o `OpcodeID`
//! (`CONVENTIONS.md`, item 4), então essas tabelas e as conferências de offset não têm
//! correspondente: o que sobra são as constantes que o `.asm` lê do `LLIntDesiredOffsets.h` e a
//! classificação dos pontos de entrada (`LLIntEntry`, em `llint_jit_code.rs`, é o rótulo que o
//! `LLIntEntrypoint.cpp` instala no `JITCode`).

use crate::bytecode::code_block::llint_baseline_callee_save_space_as_virtual_registers;
use crate::interpreter::call_frame::HEADER_SIZE_IN_REGISTERS;
use crate::llint::llint_entrypoint::MAX_FRAME_EXTENT_FOR_SLOW_PATH_CALL_IN_REGISTERS;
use crate::llint::llint_jit_code::LLIntEntry;
use crate::runtime::stack_alignment::stack_alignment_registers;

/// `CallFrameHeaderSlots` do `.asm`: `CallFrame::headerSizeInRegisters`.
pub const CALL_FRAME_HEADER_SLOTS: usize = HEADER_SIZE_IN_REGISTERS as usize;

/// `CalleeSaveSpaceAsVirtualRegisters` do `.asm`.
pub fn callee_save_space_as_virtual_registers() -> usize {
    llint_baseline_callee_save_space_as_virtual_registers() as usize
}

/// `StackAlignmentSlots` do `.asm` (`stackAlignmentRegisters()`).
pub fn stack_alignment_slots() -> usize {
    stack_alignment_registers() as usize
}

/// `getFrameRegisterSizeForCodeBlock` (LowLevelInterpreter.asm), em registradores:
/// `numCalleeLocals + maxFrameExtentForSlowPathCall`.
pub fn frame_register_size_for_code_block(num_callee_locals: u32) -> usize {
    num_callee_locals as usize + MAX_FRAME_EXTENT_FOR_SLOW_PATH_CALL_IN_REGISTERS as usize
}

/// Verdadeiro nos rótulos `llint_function_for_*_arity_check`, que rodam o `functionArityCheck`
/// antes do corpo.
pub fn entry_checks_arity(entry: LLIntEntry) -> bool {
    matches!(entry, LLIntEntry::FunctionForCallArityCheck | LLIntEntry::FunctionForConstructArityCheck)
}

/// Verdadeiro nos rótulos de função (`llint_function_for_*`), que rodam o `functionInitialization`
/// (perfil dos argumentos) antes do primeiro opcode. Os de programa, módulo e eval só rodam o
/// `prologue`. `None` para os rótulos que não são pontos de entrada de `CodeBlock`.
pub fn entry_is_function(entry: LLIntEntry) -> Option<bool> {
    match entry {
        LLIntEntry::ProgramPrologue | LLIntEntry::ModuleProgramPrologue | LLIntEntry::EvalPrologue => Some(false),
        LLIntEntry::FunctionForCallPrologue
        | LLIntEntry::FunctionForConstructPrologue
        | LLIntEntry::FunctionForCallArityCheck
        | LLIntEntry::FunctionForConstructArityCheck => Some(true),
        _ => None,
    }
}
