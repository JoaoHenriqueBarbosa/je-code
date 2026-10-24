//! Porte de `src/vs/platform/keybinding/common`: o contrato do serviço de atalhos, o resolvedor (que mapeia
//! sequências de acordes a comandos), o registro de atalhos padrão, o `USLayoutResolvedKeybinding` e a lógica de
//! acordes do `AbstractKeybindingService`.

pub mod abstract_keybinding_service;
pub mod base_resolved_keybinding;
#[allow(clippy::module_inception)] // `keybinding.rs` é o porte de `keybinding.ts`, como no original
pub mod keybinding;
pub mod keybinding_resolver;
pub mod keybindings_registry;
pub mod mock_keybinding_service;
pub mod resolved_keybinding_item;
pub mod us_layout_resolved_keybinding;

pub use keybinding::*;
