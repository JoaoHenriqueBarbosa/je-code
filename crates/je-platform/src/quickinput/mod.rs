//! Porte de `src/vs/platform/quickinput`.
//!
//! Portado: `common/quickInput.ts` completo ([`quick_input`], menos o que o topo dele lista como fora do porte), a lógica
//! pura de `browser/quickInputUtils.ts` ([`quick_input_utils`]), o ordenador da árvore ([`tree`]), `common/quickAccess.ts`
//! ([`quick_access`]: contrato dos provedores, opções, registro), o controlador de `browser/quickAccess.ts`
//! ([`quick_access_controller`]), o provedor de seletor de `browser/pickerQuickAccess.ts` ([`picker_quick_access`]), os
//! controladores de `browser/quickInput.ts` ([`quick_input_browser`]: `QuickPick`, `InputBox`, `QuickWidget`), a caixa de texto
//! de `browser/quickInputBox.ts` ([`quick_input_box`]), a lista de `browser/quickInputList.ts` ([`quick_input_list`]) e o
//! `QuickInputController` de `browser/quickInputController.ts` ([`quick_input_controller`]), o `QuickInputHoverDelegate` de
//! `browser/quickInput.ts` ([`quick_input_hover`]) e a árvore e o `QuickTree` de `browser/tree` ([`tree`]).

pub mod picker_quick_access;
pub mod quick_access;
pub mod quick_access_controller;
pub mod quick_input;
pub mod quick_input_box;
pub mod quick_input_browser;
pub mod quick_input_controller;
pub mod quick_input_hover;
pub mod quick_input_list;
pub mod quick_input_utils;
pub mod tree;
