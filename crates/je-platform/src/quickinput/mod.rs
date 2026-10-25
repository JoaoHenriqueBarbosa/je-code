//! Porte de `src/vs/platform/quickinput`.
//!
//! Portado: `common/quickInput.ts` completo ([`quick_input`], menos o que o topo dele lista como fora do porte), a lógica
//! pura de `browser/quickInputUtils.ts` ([`quick_input_utils`]), o ordenador da árvore ([`tree`]), `common/quickAccess.ts`
//! ([`quick_access`]: contrato dos provedores, opções, registro), o controlador de `browser/quickAccess.ts`
//! ([`quick_access_controller`]), o provedor de seletor de `browser/pickerQuickAccess.ts` ([`picker_quick_access`]) e os
//! controladores de `browser/quickInput.ts` ([`quick_input_browser`]: `QuickPick`, `InputBox`, `QuickWidget`).

pub mod picker_quick_access;
pub mod quick_access;
pub mod quick_access_controller;
pub mod quick_input;
pub mod quick_input_browser;
pub mod quick_input_utils;
pub mod tree;
