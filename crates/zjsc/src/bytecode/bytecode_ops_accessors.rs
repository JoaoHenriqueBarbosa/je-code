//! Accessors dos `Op*` decodificados: o `BytecodeStructs.h` gerado dá a cada struct um método por
//! operando (`dst()`, `lhs()`, `base()`, `value()`...) que devolve o `m_campo` convertido para o tipo
//! do operando. Em Rust o campo é `pub` e o método de mesmo nome o devolve por valor (todos os tipos
//! de operando são `Copy`, exceto `BoundLabel`, cujo `targetLabel` é lido pelo `OpMut` e não por
//! accessor). `impl_op_accessors!` é chamado de dentro de `impl_op_decode!`, ou seja, uma vez por
//! struct, com a lista de campos de `bytecode_op!`.
//!
//! Também define os traits que o `BytecodeGenerator::fuseCompareAndJump`/`fuseTestAndJmp` (templates
//! no C++, sobre `BinOp`/`UnaryOp`) usam como limite genérico: `CompareOp` (`dst`, `lhs`, `rhs`,
//! `swapOperands`) e `TestOp` (`dst`, `operand`). Ficam aqui, e não em `bytecode_ops.rs`, e são
//! reexportados por lá (`pub use bytecode_ops_accessors::{CompareOp, TestOp}`).

use crate::bytecode::bytecode_ops_decode::DecodeOp;
use crate::bytecode::virtual_register::VirtualRegister;

/// Um método por campo, exceto `target_label` (`BoundLabel` não é `Copy`).
macro_rules! impl_op_accessors {
    ($name:ident { $($field:ident: $ty:ty),* $(,)? }) => {
        impl $name {
            $($crate::bytecode::bytecode_ops_accessors::impl_op_accessor!($field, $ty);)*
        }
    };
}

macro_rules! impl_op_accessor {
    (target_label, $ty:ty) => {};
    ($field:ident, $ty:ty) => {
        #[inline]
        #[allow(dead_code)]
        pub fn $field(&self) -> $ty {
            self.$field
        }
    };
}

pub(crate) use impl_op_accessor;
pub(crate) use impl_op_accessors;

/// `BinOp` de `fuseCompareAndJump`: `dst`, `lhs`, `rhs`.
pub trait CompareOp: DecodeOp {
    fn dst(&self) -> VirtualRegister;
    fn lhs(&self) -> VirtualRegister;
    fn rhs(&self) -> VirtualRegister;
    /// `std::swap(binop.m_lhs, binop.m_rhs)`.
    fn swap_operands(&mut self);
}

/// `UnaryOp` de `fuseTestAndJmp`: `dst`, `operand`.
pub trait TestOp: DecodeOp {
    fn dst(&self) -> VirtualRegister;
    fn operand(&self) -> VirtualRegister;
}

macro_rules! impl_compare_op {
    ($($name:ident),* $(,)?) => {
        $(impl CompareOp for crate::bytecode::bytecode_ops::$name {
            fn dst(&self) -> VirtualRegister { self.dst }
            fn lhs(&self) -> VirtualRegister { self.lhs }
            fn rhs(&self) -> VirtualRegister { self.rhs }
            fn swap_operands(&mut self) { std::mem::swap(&mut self.lhs, &mut self.rhs); }
        })*
    };
}

macro_rules! impl_test_op {
    ($($name:ident),* $(,)?) => {
        $(impl TestOp for crate::bytecode::bytecode_ops::$name {
            fn dst(&self) -> VirtualRegister { self.dst }
            fn operand(&self) -> VirtualRegister { self.operand }
        })*
    };
}

impl_compare_op!(
    OpBelow, OpBeloweq, OpEq, OpNeq, OpGreater, OpGreatereq, OpLess, OpLesseq, OpStricteq, OpNstricteq,
);

impl_test_op!(OpEqNull, OpNeqNull, OpIsUndefinedOrNull, OpNot);
