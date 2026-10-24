//! Porte de `src/vs/platform/quickinput`.
//!
//! Portado: `common/quickInput.ts` completo ([`quick_input`], menos o que o topo dele lista como fora do porte), a lógica
//! pura de `browser/quickInputUtils.ts` ([`quick_input_utils`]), o ordenador da árvore ([`tree`]), o contrato dos provedores
//! de `common/quickAccess.ts` ([`quick_access`]) e o provedor de seletor de `browser/pickerQuickAccess.ts`
//! ([`picker_quick_access`]).

pub mod picker_quick_access;
pub mod quick_access;
pub mod quick_input;
pub mod quick_input_utils;
pub mod tree;
