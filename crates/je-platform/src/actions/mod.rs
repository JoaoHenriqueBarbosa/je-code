//! Porte de `src/vs/platform/actions/common`: `MenuId`, `MenuRegistry`, `MenuItemAction`, `SubmenuItemAction`, `Action2` e
//! `registerAction2`.
//!
//! `menu_service` é o porte de `menuService.ts` (a implementação de `IMenuService`, com o estado de itens escondidos no
//! armazenamento e o debounce de eventos).
//!
//! Fora do porte (e por quê): `menuResetAction.ts` (uma `Action2` com diálogo de confirmação, que depende do serviço de
//! diálogo da interface) e todo o `browser/` (toolbars, botões, view items e `actionViewItemService`: a UI é desenhada
//! pela árvore de widgets nativa, não por HTML; `menuEntryActionViewItem.ts` e `toolbar.ts` dependem de widgets). Do
//! `browser/` não sobra lógica pura de modelo além do que já está em `actions.rs`.

pub mod menu_entry_action_view_item;
pub mod menu_ids;
pub mod menu_service;
#[allow(clippy::module_inception)] // `actions.rs` é o porte de `actions.ts`, como no original
pub mod actions;

pub use actions::*;
pub use menu_service::{MenuService, PersistedMenuHideState, create_configure_keybinding_action};
