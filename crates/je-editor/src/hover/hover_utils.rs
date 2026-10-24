//! Porte de `src/vs/editor/contrib/hover/browser/hoverUtils.ts`.
//!
//! Divergências:
//! - `isMousePositionWithinElement(element: HTMLElement, ...)` lê o retângulo da página com
//!   `dom.getDomNodePagePosition(element)`. Não há DOM: a função recebe o próprio retângulo
//!   (`DomNodePagePosition`), que o chamador obtém do widget (já somado ao deslocamento da página).
//! - Os literais `'altKey' | 'ctrlKey' | 'metaKey'` viram `MultiCursorModifier`; o `hoverEnabled` é o
//!   `HoverEnabled` da opção `hover`.
//! - `isTriggerModifierPressed` recebe `ModifierState`, o tipo estrutural `{ ctrlKey, metaKey, altKey }`;
//!   o `MouseEvent` do `editor_browser` converte nele com `From`.

use crate::config::editor_options::hover::HoverEnabled;
use crate::editor_browser::{EditorMouseEvent, MouseEvent};

/// `PADDING.VALUE`.
const PADDING: f64 = 3.0;

/// `dom.IDomNodePagePosition`: o retângulo de um nó em coordenadas da página.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct DomNodePagePosition {
    pub left: f64,
    pub top: f64,
    pub width: f64,
    pub height: f64,
}

/// `'altKey' | 'ctrlKey' | 'metaKey'`: o modificador do multi-cursor.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum MultiCursorModifier {
    AltKey,
    CtrlKey,
    MetaKey,
}

/// `{ ctrlKey: boolean; metaKey: boolean; altKey: boolean }`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ModifierState {
    pub ctrl_key: bool,
    pub meta_key: bool,
    pub alt_key: bool,
}

impl From<&MouseEvent> for ModifierState {
    fn from(event: &MouseEvent) -> Self {
        ModifierState { ctrl_key: event.ctrl_key, meta_key: event.meta_key, alt_key: event.alt_key }
    }
}

pub fn is_mouse_position_within_element(element: &DomNodePagePosition, posx: f64, posy: f64) -> bool {
    !(posx < element.left + PADDING
        || posx > element.left + element.width - PADDING
        || posy < element.top + PADDING
        || posy > element.top + element.height - PADDING)
}

/// Determines whether hover should be shown based on the hover setting and current keyboard modifiers.
/// When `hover_enabled` is `OnKeyboardModifier`, hover is shown when the user presses the opposite
/// modifier key from the multi-cursor modifier (e.g., if multi-cursor uses Alt, hover shows on Ctrl/Cmd).
pub fn should_show_hover(
    hover_enabled: HoverEnabled,
    multi_cursor_modifier: MultiCursorModifier,
    mouse_event: &EditorMouseEvent,
) -> bool {
    match hover_enabled {
        HoverEnabled::On => true,
        HoverEnabled::Off => false,
        HoverEnabled::OnKeyboardModifier => {
            is_trigger_modifier_pressed(multi_cursor_modifier, ModifierState::from(&mouse_event.event))
        }
    }
}

/// Returns true if the trigger modifier (inverse of multi-cursor modifier) is pressed.
/// This works with both mouse and keyboard events by relying only on the modifier flags.
pub fn is_trigger_modifier_pressed(multi_cursor_modifier: MultiCursorModifier, event: ModifierState) -> bool {
    if multi_cursor_modifier == MultiCursorModifier::AltKey {
        return event.ctrl_key || event.meta_key;
    }
    event.alt_key // multiCursorModifier is ctrlKey or metaKey
}
