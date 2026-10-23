//! Porte de `src/vs/platform/actions/common`: `MenuId`, `MenuRegistry`, `MenuItemAction`, `SubmenuItemAction`, `Action2` e
//! `registerAction2`.
//!
//! Fora do porte (e por quê): `menuService.ts` (a implementação de `IMenuService`, com o estado de itens escondidos no
//! armazenamento e o debounce de eventos; o contrato `IMenuService` está aqui), `menuResetAction.ts` e todo o `browser/`
//! (toolbars, botões e view items: a UI é desenhada pela árvore de widgets nativa, não por HTML).

pub mod menu_ids;
#[allow(clippy::module_inception)] // `actions.rs` é o porte de `actions.ts`, como no original
pub mod actions;

pub use actions::*;
