//! Porte de `src/vs/workbench/services/keybinding`.

pub mod fallback_keyboard_mapper;
pub mod mac_linux_keyboard_mapper;
pub mod keybinding_editing;
pub mod keybinding_io;
pub mod unbound_commands;
pub mod windows_keyboard_mapper;

// `KeymapInfo`, os layouts do macOS e o `BrowserKeyboardMapperFactoryBase` (`keyboardLayoutService.ts`).
pub mod keymap_info;
pub mod keyboard_layouts;
pub mod keyboard_layout_service;
