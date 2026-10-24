//! Porte de `bytecode/BytecodeOperandsForCheckpoint.h` e dos `enum Checkpoints : uint8_t` que o
//! gerador Ruby (`generator/Opcode.rb`) põe dentro de cada struct `Op*` com `checkpoints:` no
//! `BytecodeList.rb` (`BytecodeStructs.h`).
//!
//! Cada enumerador vira uma constante associada do `Op*` (`OpInstanceof::GET_HAS_INSTANCE`) com o
//! valor do C++ (a posição na lista do `.rb`, a partir de 0). O `numberOfCheckpoints` de cada op
//! bate com `BYTECODE_CHECKPOINT_COUNT_TABLE` (`instruction_stream.rs`).

use crate::bytecode::bytecode_index::Checkpoint;
use crate::bytecode::bytecode_ops::{
    OpAsyncIteratorNext, OpAsyncIteratorOpen, OpCallVarargs, OpConstructVarargs, OpInstanceof, OpIteratorNext,
    OpIteratorOpen, OpSuperConstructVarargs, OpTailCallVarargs,
};
use crate::bytecode::virtual_register::{virtual_register_for_argument_including_this, VirtualRegister};

/// `enum Checkpoints` de `op :tail_call_varargs`, `call_varargs`, `construct_varargs` e
/// `super_construct_varargs` (`determiningArgCount`, `makeCall`).
macro_rules! varargs_checkpoints {
    ($($name:ident),* $(,)?) => {
        $(impl $name {
            pub const DETERMINING_ARG_COUNT: Checkpoint = 0;
            pub const MAKE_CALL: Checkpoint = 1;
            pub const NUMBER_OF_CHECKPOINTS: Checkpoint = 2;
        })*
    };
}

varargs_checkpoints!(OpTailCallVarargs, OpCallVarargs, OpConstructVarargs, OpSuperConstructVarargs);

/// `enum Checkpoints` de `op :iterator_open` e `op :async_iterator_open` (`symbolCall`, `getNext`).
macro_rules! iterator_open_checkpoints {
    ($($name:ident),* $(,)?) => {
        $(impl $name {
            pub const SYMBOL_CALL: Checkpoint = 0;
            pub const GET_NEXT: Checkpoint = 1;
            pub const NUMBER_OF_CHECKPOINTS: Checkpoint = 2;
        })*
    };
}

iterator_open_checkpoints!(OpIteratorOpen, OpAsyncIteratorOpen);

impl OpIteratorNext {
    pub const COMPUTE_NEXT: Checkpoint = 0;
    pub const GET_DONE: Checkpoint = 1;
    pub const GET_VALUE: Checkpoint = 2;
    pub const NUMBER_OF_CHECKPOINTS: Checkpoint = 3;
}

impl OpInstanceof {
    pub const GET_HAS_INSTANCE: Checkpoint = 0;
    pub const GET_PROTOTYPE: Checkpoint = 1;
    pub const INSTANCEOF: Checkpoint = 2;
    pub const NUMBER_OF_CHECKPOINTS: Checkpoint = 3;
}

/// `resumeValueOperandFor(const OpAsyncIteratorNext&)`: o `op_async_iterator_next` não guarda um
/// `VirtualRegister` para o valor de retomada opcional; ele é o argumento de índice 1 da chamada,
/// endereçado por `m_stackOffset`, como o argc/argv do `op_call`. Só vale com `m_hasValue`.
pub fn resume_value_operand_for(bytecode: &OpAsyncIteratorNext) -> VirtualRegister {
    debug_assert!(bytecode.has_value);
    virtual_register_for_argument_including_this(1, (bytecode.stack_offset as i32).wrapping_neg())
}
