//! Porte de `src/vs/platform/contextview/browser`: o menu de contexto (`ContextMenuHandler`, `ContextMenuService`) e a
//! ponte para os provedores de `je-widgets` ([`ContextMenuServiceProvider`]).

pub mod context_menu_handler;
pub mod context_menu_provider;
pub mod context_menu_service;

pub use context_menu_handler::{ContextMenuExtras, ContextMenuHandler, IContextMenuHandlerOptions};
pub use context_menu_provider::{ButtonContextMenuServiceProvider, ContextMenuServiceProvider};
pub use context_menu_service::{ContextMenuMenuDelegate, ContextMenuService};
