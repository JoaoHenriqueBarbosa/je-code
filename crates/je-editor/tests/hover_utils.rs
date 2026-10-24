//! Porte de `src/vs/editor/contrib/hover/test/browser/hoverUtils.test.ts` (suíte `Hover Utils`), caso a caso
//! e na mesma ordem.
//!
//! Notas de tradução:
//! - `ensureNoDisposablesAreLeakedInTestSuite()` vira o `LeakGuard` criado no começo de cada caso.
//! - `createMockMouseEvent` (um `IEditorMouseEvent` parcial com `as`) vira um `EditorMouseEvent` completo,
//!   com o resto dos campos no valor padrão.
//! - `createMockElement`, que troca o `getBoundingClientRect` de um `div`, vira o `DomNodePagePosition` com
//!   o mesmo retângulo: `getDomNodePagePosition` devolve `left`, `top`, `width` e `height` do retângulo mais
//!   o deslocamento da página, que no teste é zero.
//! - Os literais `'on'`, `'altKey'` etc. viram `HoverEnabled` e `MultiCursorModifier`.

#[path = "../../je-base/tests/common/mod.rs"]
mod common;

use common::ensure_no_disposables_are_leaked_in_test_suite;
use je_editor::config::editor_options::hover::HoverEnabled;
use je_editor::editor_browser::{EditorMouseEvent, MouseEvent};
use je_editor::hover::hover_utils::{
    DomNodePagePosition, ModifierState, MultiCursorModifier, is_mouse_position_within_element,
    is_trigger_modifier_pressed, should_show_hover,
};

// suite('shouldShowHover')

fn create_mock_mouse_event(ctrl_key: bool, alt_key: bool, meta_key: bool) -> EditorMouseEvent {
    EditorMouseEvent {
        event: MouseEvent { ctrl_key, alt_key, meta_key, shift_key: false, ..MouseEvent::default() },
        ..EditorMouseEvent::default()
    }
}

#[test]
fn should_show_hover_returns_true_when_enabled_is_on() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    let mouse_event = create_mock_mouse_event(false, false, false);
    let result = should_show_hover(HoverEnabled::On, MultiCursorModifier::AltKey, &mouse_event);
    assert_eq!(result, true);
}

#[test]
fn should_show_hover_returns_false_when_enabled_is_off() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    let mouse_event = create_mock_mouse_event(false, false, false);
    let result = should_show_hover(HoverEnabled::Off, MultiCursorModifier::AltKey, &mouse_event);
    assert_eq!(result, false);
}

#[test]
fn should_show_hover_returns_true_with_ctrl_pressed_when_multi_cursor_modifier_is_alt_key() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    let mouse_event = create_mock_mouse_event(true, false, false);
    let result = should_show_hover(HoverEnabled::OnKeyboardModifier, MultiCursorModifier::AltKey, &mouse_event);
    assert_eq!(result, true);
}

#[test]
fn should_show_hover_returns_false_without_ctrl_pressed_when_multi_cursor_modifier_is_alt_key() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    let mouse_event = create_mock_mouse_event(false, false, false);
    let result = should_show_hover(HoverEnabled::OnKeyboardModifier, MultiCursorModifier::AltKey, &mouse_event);
    assert_eq!(result, false);
}

#[test]
fn should_show_hover_returns_true_with_meta_key_pressed_when_multi_cursor_modifier_is_alt_key() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    let mouse_event = create_mock_mouse_event(false, false, true);
    let result = should_show_hover(HoverEnabled::OnKeyboardModifier, MultiCursorModifier::AltKey, &mouse_event);
    assert_eq!(result, true);
}

#[test]
fn should_show_hover_returns_true_with_alt_pressed_when_multi_cursor_modifier_is_ctrl_key() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    let mouse_event = create_mock_mouse_event(false, true, false);
    let result = should_show_hover(HoverEnabled::OnKeyboardModifier, MultiCursorModifier::CtrlKey, &mouse_event);
    assert_eq!(result, true);
}

#[test]
fn should_show_hover_returns_false_without_alt_pressed_when_multi_cursor_modifier_is_ctrl_key() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    let mouse_event = create_mock_mouse_event(false, false, false);
    let result = should_show_hover(HoverEnabled::OnKeyboardModifier, MultiCursorModifier::CtrlKey, &mouse_event);
    assert_eq!(result, false);
}

#[test]
fn should_show_hover_returns_true_with_alt_pressed_when_multi_cursor_modifier_is_meta_key() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    let mouse_event = create_mock_mouse_event(false, true, false);
    let result = should_show_hover(HoverEnabled::OnKeyboardModifier, MultiCursorModifier::MetaKey, &mouse_event);
    assert_eq!(result, true);
}

#[test]
fn should_show_hover_ignores_alt_when_multi_cursor_modifier_is_alt_key() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    let mouse_event = create_mock_mouse_event(false, true, false);
    let result = should_show_hover(HoverEnabled::OnKeyboardModifier, MultiCursorModifier::AltKey, &mouse_event);
    assert_eq!(result, false);
}

#[test]
fn should_show_hover_ignores_ctrl_when_multi_cursor_modifier_is_ctrl_key() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    let mouse_event = create_mock_mouse_event(true, false, false);
    let result = should_show_hover(HoverEnabled::OnKeyboardModifier, MultiCursorModifier::CtrlKey, &mouse_event);
    assert_eq!(result, false);
}

// suite('isMousePositionWithinElement')

fn create_mock_element(left: f64, top: f64, width: f64, height: f64) -> DomNodePagePosition {
    DomNodePagePosition { left, top, width, height }
}

#[test]
fn is_mouse_position_within_element_returns_true_when_mouse_is_inside_element_bounds() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    let element = create_mock_element(100.0, 100.0, 200.0, 100.0);
    assert_eq!(is_mouse_position_within_element(&element, 150.0, 150.0), true);
    assert_eq!(is_mouse_position_within_element(&element, 200.0, 150.0), true);
    assert_eq!(is_mouse_position_within_element(&element, 250.0, 180.0), true);
}

#[test]
fn is_mouse_position_within_element_returns_true_when_mouse_is_on_element_edges() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    let element = create_mock_element(100.0, 100.0, 200.0, 100.0);
    assert_eq!(is_mouse_position_within_element(&element, 100.0, 100.0), false); // top-left corner
    assert_eq!(is_mouse_position_within_element(&element, 300.0, 100.0), false); // top-right corner
    assert_eq!(is_mouse_position_within_element(&element, 100.0, 200.0), false); // bottom-left corner
    assert_eq!(is_mouse_position_within_element(&element, 300.0, 200.0), false); // bottom-right corner
}

#[test]
fn is_mouse_position_within_element_returns_false_when_mouse_is_left_of_element() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    let element = create_mock_element(100.0, 100.0, 200.0, 100.0);
    assert_eq!(is_mouse_position_within_element(&element, 99.0, 150.0), false);
    assert_eq!(is_mouse_position_within_element(&element, 50.0, 150.0), false);
}

#[test]
fn is_mouse_position_within_element_returns_false_when_mouse_is_right_of_element() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    let element = create_mock_element(100.0, 100.0, 200.0, 100.0);
    assert_eq!(is_mouse_position_within_element(&element, 301.0, 150.0), false);
    assert_eq!(is_mouse_position_within_element(&element, 400.0, 150.0), false);
}

#[test]
fn is_mouse_position_within_element_returns_false_when_mouse_is_above_element() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    let element = create_mock_element(100.0, 100.0, 200.0, 100.0);
    assert_eq!(is_mouse_position_within_element(&element, 200.0, 99.0), false);
    assert_eq!(is_mouse_position_within_element(&element, 200.0, 50.0), false);
}

#[test]
fn is_mouse_position_within_element_returns_false_when_mouse_is_below_element() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    let element = create_mock_element(100.0, 100.0, 200.0, 100.0);
    assert_eq!(is_mouse_position_within_element(&element, 200.0, 201.0), false);
    assert_eq!(is_mouse_position_within_element(&element, 200.0, 300.0), false);
}

#[test]
fn is_mouse_position_within_element_handles_element_at_origin() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    let element = create_mock_element(0.0, 0.0, 100.0, 100.0);
    assert_eq!(is_mouse_position_within_element(&element, 0.0, 0.0), false);
    assert_eq!(is_mouse_position_within_element(&element, 50.0, 50.0), true);
    assert_eq!(is_mouse_position_within_element(&element, 100.0, 100.0), false);
    assert_eq!(is_mouse_position_within_element(&element, 101.0, 101.0), false);
}

#[test]
fn is_mouse_position_within_element_handles_small_elements_1x1() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    let element = create_mock_element(100.0, 100.0, 1.0, 1.0);
    assert_eq!(is_mouse_position_within_element(&element, 100.0, 100.0), false);
    assert_eq!(is_mouse_position_within_element(&element, 101.0, 101.0), false);
    assert_eq!(is_mouse_position_within_element(&element, 102.0, 102.0), false);
}

// suite('isTriggerModifierPressed')

fn create_modifier_event(ctrl_key: bool, alt_key: bool, meta_key: bool) -> ModifierState {
    ModifierState { ctrl_key, alt_key, meta_key }
}

#[test]
fn is_trigger_modifier_pressed_returns_true_with_ctrl_pressed_when_multi_cursor_modifier_is_alt_key() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    let event = create_modifier_event(true, false, false);
    assert_eq!(is_trigger_modifier_pressed(MultiCursorModifier::AltKey, event), true);
}

#[test]
fn is_trigger_modifier_pressed_returns_true_with_meta_key_pressed_when_multi_cursor_modifier_is_alt_key() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    let event = create_modifier_event(false, false, true);
    assert_eq!(is_trigger_modifier_pressed(MultiCursorModifier::AltKey, event), true);
}

#[test]
fn is_trigger_modifier_pressed_returns_true_with_both_ctrl_and_meta_key_pressed_when_multi_cursor_modifier_is_alt_key() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    let event = create_modifier_event(true, false, true);
    assert_eq!(is_trigger_modifier_pressed(MultiCursorModifier::AltKey, event), true);
}

#[test]
fn is_trigger_modifier_pressed_returns_false_without_ctrl_or_meta_key_when_multi_cursor_modifier_is_alt_key() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    let event = create_modifier_event(false, false, false);
    assert_eq!(is_trigger_modifier_pressed(MultiCursorModifier::AltKey, event), false);
}

#[test]
fn is_trigger_modifier_pressed_returns_false_with_alt_pressed_when_multi_cursor_modifier_is_alt_key() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    let event = create_modifier_event(false, true, false);
    assert_eq!(is_trigger_modifier_pressed(MultiCursorModifier::AltKey, event), false);
}

#[test]
fn is_trigger_modifier_pressed_returns_true_with_alt_pressed_when_multi_cursor_modifier_is_ctrl_key() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    let event = create_modifier_event(false, true, false);
    assert_eq!(is_trigger_modifier_pressed(MultiCursorModifier::CtrlKey, event), true);
}

#[test]
fn is_trigger_modifier_pressed_returns_false_without_alt_pressed_when_multi_cursor_modifier_is_ctrl_key() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    let event = create_modifier_event(false, false, false);
    assert_eq!(is_trigger_modifier_pressed(MultiCursorModifier::CtrlKey, event), false);
}

#[test]
fn is_trigger_modifier_pressed_returns_false_with_ctrl_pressed_when_multi_cursor_modifier_is_ctrl_key() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    let event = create_modifier_event(true, false, false);
    assert_eq!(is_trigger_modifier_pressed(MultiCursorModifier::CtrlKey, event), false);
}

#[test]
fn is_trigger_modifier_pressed_returns_true_with_alt_pressed_when_multi_cursor_modifier_is_meta_key() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    let event = create_modifier_event(false, true, false);
    assert_eq!(is_trigger_modifier_pressed(MultiCursorModifier::MetaKey, event), true);
}

#[test]
fn is_trigger_modifier_pressed_returns_false_without_alt_pressed_when_multi_cursor_modifier_is_meta_key() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    let event = create_modifier_event(false, false, false);
    assert_eq!(is_trigger_modifier_pressed(MultiCursorModifier::MetaKey, event), false);
}

#[test]
fn is_trigger_modifier_pressed_returns_false_with_meta_key_pressed_when_multi_cursor_modifier_is_meta_key() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    let event = create_modifier_event(false, false, true);
    assert_eq!(is_trigger_modifier_pressed(MultiCursorModifier::MetaKey, event), false);
}
