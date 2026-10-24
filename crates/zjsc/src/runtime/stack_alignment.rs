//! Porte de `runtime/StackAlignment.h` para Linux x86_64.
//!
//! `sizeof(CallerFrameAndPC)` = 16, então `stackAdjustmentForAlignment()` = 0.

use crate::interpreter::call_frame::{CALLER_FRAME_AND_PC_SIZE_IN_REGISTERS, HEADER_SIZE_IN_REGISTERS};
use crate::wtf::math_extras::round_up_to_multiple_of_const;

/// `stackAlignmentBytes()`.
pub const fn stack_alignment_bytes() -> u32 {
    16
}

/// `stackAlignmentRegisters()`: `stackAlignmentBytes() / sizeof(EncodedJSValue)`.
pub const fn stack_alignment_registers() -> u32 {
    stack_alignment_bytes() / 8
}

/// `stackAdjustmentForAlignment()`: `sizeof(CallerFrameAndPC) % stackAlignmentBytes()` é 0.
pub const fn stack_adjustment_for_alignment() -> u32 {
    let size_of_caller_frame_and_pc = (CALLER_FRAME_AND_PC_SIZE_IN_REGISTERS as u32) * 8;
    let excess = size_of_caller_frame_and_pc % stack_alignment_bytes();
    if excess != 0 {
        return stack_alignment_bytes() - excess;
    }
    0
}

/// `roundArgumentCountToAlignFrame`.
pub fn round_argument_count_to_align_frame(argument_count: u32) -> u32 {
    let header = HEADER_SIZE_IN_REGISTERS as usize;
    (round_up_to_multiple_of_const::<2>(argument_count as usize + header) - header) as u32
}

/// `roundLocalRegisterCountForFramePointerOffset`.
pub fn round_local_register_count_for_frame_pointer_offset(local_register_count: u32) -> u32 {
    let size = CALLER_FRAME_AND_PC_SIZE_IN_REGISTERS as usize;
    (round_up_to_multiple_of_const::<2>(local_register_count as usize + size) - size) as u32
}

/// `argumentCountForStackSize`.
pub fn argument_count_for_stack_size(size_in_bytes: u32) -> u32 {
    let size_in_registers = size_in_bytes / 8;
    if size_in_registers <= HEADER_SIZE_IN_REGISTERS as u32 {
        return 0;
    }
    size_in_registers - HEADER_SIZE_IN_REGISTERS as u32
}

/// `logStackAlignmentRegisters()`: `fastLog2(stackAlignmentRegisters())`.
pub fn log_stack_alignment_registers() -> u32 {
    stack_alignment_registers().ilog2()
}

/// `isStackAligned`: aqui sobre o índice do registrador na pilha, já em unidades de registrador
/// (cada registrador tem 8 bytes, e a base da pilha é alinhada).
pub fn is_stack_aligned(register_index: usize) -> bool {
    (register_index * 8) & (stack_alignment_bytes() as usize - 1) == 0
}
