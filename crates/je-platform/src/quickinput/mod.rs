//! Porte de `src/vs/platform/quickinput`.
//!
//! Portado: `common/quickInput.ts` completo ([`quick_input`], menos o que o topo dele lista como fora do porte), a lógica
//! pura de `browser/quickInputUtils.ts` ([`quick_input_utils`]) e o ordenador da árvore ([`tree`]).

pub mod quick_input;
pub mod quick_input_utils;
pub mod tree;
