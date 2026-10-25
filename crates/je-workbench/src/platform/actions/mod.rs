//! Porte de `src/vs/platform/actions/browser`: o que estende widgets (`WorkbenchToolBar`, `WorkbenchButtonBar`,
//! `MenuWorkbenchButtonBar`) e por isso não cabe no `je-platform`, que não depende de `je-widgets`. A lógica de modelo
//! de `menuEntryActionViewItem.ts` está em `je_platform::actions::menu_entry_action_view_item`.

pub mod buttonbar;
pub mod toolbar;

pub use buttonbar::{IButtonConfig, IButtonConfigProvider, IMenuWorkbenchButtonBarOptions, IWorkbenchButtonBarOptions, MenuWorkbenchButtonBar, WorkbenchButtonBar};
pub use toolbar::{HiddenItemStrategy, IMenuWorkbenchToolBarOptions, IToolBarRenderOptions, IWorkbenchToolBarOptions, MenuWorkbenchToolBar, OverflowBehavior, PrimaryGroup, WorkbenchToolBar};
