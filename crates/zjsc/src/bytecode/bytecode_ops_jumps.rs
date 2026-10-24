//! Traits que o `BytecodeGenerator::fuseCompareAndJump`/`fuseTestAndJmp` (templates no C++, sobre
//! `JmpOp`) usam como limite genérico para o salto fundido: `JmpOp::emit(this, lhs, rhs, label)` e
//! `JmpOp::emit(this, operand, label)`. Cada impl delega ao `emit` que `bytecode_op!` já gerou para a
//! struct (grupos `BinaryJmp` e `UnaryJmp` do `.rb`). Reexportados por `bytecode_ops.rs`
//! (`pub use bytecode_ops_jumps::{CompareJumpOp, TestJumpOp}`).

use crate::bytecode::bytecode_ops::{BoundLabel, OpWriter};
use crate::bytecode::virtual_register::VirtualRegister;

/// `JmpOp` de `fuseCompareAndJump`: `emit(generator, lhs, rhs, targetLabel)`.
pub trait CompareJumpOp {
    fn emit<G: OpWriter>(generator: &mut G, lhs: VirtualRegister, rhs: VirtualRegister, target_label: BoundLabel);
}

/// `JmpOp` de `fuseTestAndJmp`: `emit(generator, operand, targetLabel)`.
pub trait TestJumpOp {
    fn emit<G: OpWriter>(generator: &mut G, operand: VirtualRegister, target_label: BoundLabel);
}

macro_rules! impl_compare_jump_op {
    ($($name:ident),* $(,)?) => {
        $(impl CompareJumpOp for crate::bytecode::bytecode_ops::$name {
            fn emit<G: OpWriter>(generator: &mut G, lhs: VirtualRegister, rhs: VirtualRegister, target_label: BoundLabel) {
                crate::bytecode::bytecode_ops::$name::emit(generator, lhs, rhs, target_label)
            }
        })*
    };
}

macro_rules! impl_test_jump_op {
    ($($name:ident),* $(,)?) => {
        $(impl TestJumpOp for crate::bytecode::bytecode_ops::$name {
            fn emit<G: OpWriter>(generator: &mut G, operand: VirtualRegister, target_label: BoundLabel) {
                crate::bytecode::bytecode_ops::$name::emit(generator, operand, target_label)
            }
        })*
    };
}

impl_compare_jump_op!(
    OpJless, OpJnless, OpJlesseq, OpJnlesseq, OpJgreater, OpJngreater, OpJgreatereq, OpJngreatereq, OpJeq, OpJneq,
    OpJstricteq, OpJnstricteq, OpJbelow, OpJbeloweq,
);

impl_test_jump_op!(
    OpJeqNull, OpJneqNull, OpJundefinedOrNull, OpJnundefinedOrNull, OpJtrue, OpJfalse,
);
