//! Testes extras (sem equivalente no VS Code) do comportamento portado de `replaceInput.ts`: o botão de
//! preservar caixa, a largura reservada, os eventos, o desabilitar e o histórico. O `replaceInput` não tem
//! `.test.ts` na base do VS Code.

mod support;

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use je_base::key_codes::KeyCode;
use je_base::lifecycle::IDisposable;
use je_render::Rgba;
use je_render::kurbo::Rect;
use je_ui::{Event, EventKind, KeyData, Modifiers, NodeId, Ui};
use je_widgets::find_input::FindInputStyles;
use je_widgets::input_box::unthemed_input_box_styles;
use je_widgets::replace_input::{ReplaceInput, ReplaceInputOptions};
use je_widgets::toggle::unthemed_toggle_styles;
use support::{create_container, ensure_no_disposables_are_leaked_in_test_suite};

const DISABLED: Rgba = Rgba::opaque(0xE1, 0xE1, 0xE1);

fn options() -> ReplaceInputOptions {
    let styles = FindInputStyles { input_box_styles: unthemed_input_box_styles(), toggle_styles: unthemed_toggle_styles(), disabled_background: Some(DISABLED) };
    ReplaceInputOptions { caret_blink: false, ..ReplaceInputOptions::new("Replace", styles) }
}

fn setup(show_option_buttons: bool, opts: ReplaceInputOptions) -> (Ui, Rc<ReplaceInput>) {
    let ui = Ui::new();
    let parent = create_container(&ui, 300.0, 100.0);
    let root = ui.tree().root();
    ui.tree_mut().append_child(root, parent);
    let replace_input = ReplaceInput::new(&ui, Some(parent), None, show_option_buttons, opts);
    (ui, replace_input)
}

fn click(ui: &Ui, node: NodeId) {
    ui.dispatch(&mut Event::new(EventKind::Click, node));
}

fn key(ui: &Ui, node: NodeId, key_code: KeyCode) {
    ui.dispatch(&mut Event::new(EventKind::KeyDown, node).with_key(KeyData { key_code, modifiers: Modifiers::default() }));
}

#[test]
fn the_preserve_case_toggle_is_the_only_control_and_is_hidden_without_option_buttons() {
    let _store = ensure_no_disposables_are_leaked_in_test_suite();
    let (ui, replace_input) = setup(false, options());
    let nodes = ui.tree().children(replace_input.controls()).to_vec();
    assert_eq!(nodes, vec![replace_input.preserve_case().dom_node()]);
    assert!(!ui.tree().style(replace_input.controls()).visible);
    assert!(ui.tree().has_class(nodes[0], "codicon-preserve-case"));
    assert_eq!(ui.tree().label(nodes[0]), Some("Preserve Case"));
    replace_input.dispose();

    let (ui, replace_input) = setup(true, ReplaceInputOptions { append_preserve_case_label: " (Alt+P)".to_owned(), ..options() });
    assert!(ui.tree().style(replace_input.controls()).visible);
    assert_eq!(ui.tree().label(replace_input.preserve_case().dom_node()), Some("Preserve Case (Alt+P)"));
    // `top: 3px; right: 2px`, um botão de 22px.
    assert_eq!(ui.tree().bounds(replace_input.controls()), Rect::new(300.0 - 2.0 - 22.0, 3.0, 298.0, 23.0));
    replace_input.dispose();
}

#[test]
fn clicking_preserve_case_toggles_it_and_focuses_the_input_keyboard_keeps_the_focus() {
    let store = ensure_no_disposables_are_leaked_in_test_suite();
    let (ui, replace_input) = setup(true, options());
    let changes = Rc::new(RefCell::new(Vec::new()));
    let sink = changes.clone();
    store.add(replace_input.on_did_option_change().listen(move |via_keyboard| sink.borrow_mut().push(*via_keyboard)));
    let toggle = replace_input.preserve_case().dom_node();

    click(&ui, toggle);
    assert!(replace_input.get_preserve_case());
    assert_eq!(ui.focused(), Some(replace_input.input_box.input_element()));

    replace_input.focus_on_preserve();
    assert_eq!(ui.focused(), Some(toggle));
    key(&ui, toggle, KeyCode::Enter);
    assert!(!replace_input.get_preserve_case());
    assert_eq!(ui.focused(), Some(toggle));
    assert_eq!(*changes.borrow(), vec![false, true]);

    replace_input.set_preserve_case(true);
    assert!(replace_input.get_preserve_case());
    assert_eq!(changes.borrow().len(), 2, "o setter não dispara onDidOptionChange");
    replace_input.dispose();
}

#[test]
fn set_width_fixes_the_width_and_reserves_the_toggle_width_when_option_buttons_show() {
    let _store = ensure_no_disposables_are_leaked_in_test_suite();
    let (ui, replace_input) = setup(true, options());
    replace_input.set_width(180.0);
    assert_eq!(ui.tree().bounds(replace_input.dom_node()).width(), 180.0);
    assert_eq!(ui.tree().bounds(replace_input.controls()).x1, 178.0);
    let text_width = |ui: &Ui, replace_input: &ReplaceInput| ui.tree().bounds(ui.tree().children(replace_input.input_box.input_element())[0]).width();
    let with_buttons = text_width(&ui, &replace_input);
    replace_input.dispose();

    let (ui, hidden) = setup(false, options());
    hidden.set_width(180.0);
    assert_eq!(text_width(&ui, &hidden), with_buttons + 22.0);
    hidden.dispose();
}

#[test]
fn escape_and_arrows_navigate_the_single_toggle() {
    let _store = ensure_no_disposables_are_leaked_in_test_suite();
    let (ui, replace_input) = setup(true, options());
    let toggle = replace_input.preserve_case().dom_node();
    replace_input.focus_on_preserve();
    key(&ui, toggle, KeyCode::RightArrow);
    assert_eq!(ui.focused(), Some(toggle), "com um botão só, dá a volta nele mesmo");
    key(&ui, toggle, KeyCode::Escape);
    assert_eq!(ui.focused(), Some(replace_input.input_box.input_element()));
    replace_input.dispose();
}

#[test]
fn events_value_history_and_disabled_state() {
    let store = ensure_no_disposables_are_leaked_in_test_suite();
    let (ui, replace_input) = setup(true, options());
    let inputs = Rc::new(Cell::new(0));
    let counter = inputs.clone();
    store.add(replace_input.on_input().listen(move |()| counter.set(counter.get() + 1)));
    let toggle_keys = Rc::new(RefCell::new(Vec::new()));
    let sink = toggle_keys.clone();
    store.add(replace_input.on_preserve_case_key_down().listen(move |e| sink.borrow_mut().push(e.key_code)));

    replace_input.focus();
    ui.text_input("new");
    assert_eq!((replace_input.get_value(), inputs.get()), ("new".to_owned(), 1));
    replace_input.set_value("other");
    assert_eq!(inputs.get(), 1);
    replace_input.on_search_submit();
    assert_eq!(replace_input.input_box.history(), vec!["other".to_owned()]);
    key(&ui, replace_input.preserve_case().dom_node(), KeyCode::Tab);
    assert_eq!(*toggle_keys.borrow(), vec![KeyCode::Tab]);

    replace_input.set_enabled(false);
    assert!(ui.tree().has_class(replace_input.dom_node(), "disabled"));
    assert_eq!(ui.tree().style(replace_input.dom_node()).background, Some(DISABLED));
    assert!(!replace_input.preserve_case().enabled());
    replace_input.set_enabled(true);
    assert!(replace_input.preserve_case().enabled() && replace_input.input_box.is_enabled());
    replace_input.clear();
    assert_eq!(replace_input.get_value(), "");
    replace_input.dispose();
}
