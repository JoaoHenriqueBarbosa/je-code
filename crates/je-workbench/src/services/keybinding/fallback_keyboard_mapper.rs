//! Porte de `src/vs/workbench/services/keybinding/common/fallbackKeyboardMapper.ts`.

use std::rc::Rc;

use je_base::keybindings::{KeyCodeChord, Keybinding, ResolvedKeybinding};
use je_base::platform::OperatingSystem;
use je_platform::keybinding::keybinding::KeyboardEvent;
use je_platform::keybinding::us_layout_resolved_keybinding::USLayoutResolvedKeybinding;
use je_platform::keyboard_layout::IKeyboardMapper;

/// Um mapeador de teclado para usar quando a leitura do mapa de teclas do sistema falha.
pub struct FallbackKeyboardMapper {
    map_alt_gr_to_ctrl_alt: bool,
    os: OperatingSystem,
}

impl FallbackKeyboardMapper {
    pub fn new(map_alt_gr_to_ctrl_alt: bool, os: OperatingSystem) -> Self {
        Self { map_alt_gr_to_ctrl_alt, os }
    }
}

impl IKeyboardMapper for FallbackKeyboardMapper {
    fn dump_debug_info(&self) -> String {
        "FallbackKeyboardMapper dispatching on keyCode".to_string()
    }

    fn resolve_keyboard_event(&self, keyboard_event: &KeyboardEvent) -> Rc<dyn ResolvedKeybinding> {
        let alt_gr = self.map_alt_gr_to_ctrl_alt && keyboard_event.alt_graph_key;
        let ctrl_key = keyboard_event.ctrl_key || alt_gr;
        let alt_key = keyboard_event.alt_key || alt_gr;
        let chord = KeyCodeChord::new(
            ctrl_key,
            keyboard_event.shift_key,
            alt_key,
            keyboard_event.meta_key,
            keyboard_event.key_code,
        );
        let mut result = self.resolve_keybinding(&chord.to_keybinding());
        result.swap_remove(0)
    }

    fn resolve_keybinding(&self, keybinding: &Keybinding) -> Vec<Rc<dyn ResolvedKeybinding>> {
        USLayoutResolvedKeybinding::resolve_keybinding(keybinding, self.os)
    }
}
