//! Porte de `bytecode/LinkTimeConstant.h`. O enum, a contagem e a tabela de nomes são gerados
//! por `scripts/gen-bytecode-intrinsics.py` e vivem em `bytecode_intrinsics_table`; este módulo é o
//! nome que o `LinkTimeConstant.h` tem no C++.

pub use super::bytecode_intrinsics_table::{
    LinkTimeConstant, LINK_TIME_CONSTANT_TABLE, NUMBER_OF_LINK_TIME_CONSTANTS,
};
