//! Porte de `src/vs/platform/quickinput`.
//!
//! Portado até agora: os tipos de `common/quickInput.ts` que os utilitários usam ([`quick_input`]), a lógica
//! pura de `browser/quickInputUtils.ts` ([`quick_input_utils`]) e o ordenador da árvore ([`tree`]).

pub mod quick_input;
pub mod quick_input_utils;
pub mod tree;
