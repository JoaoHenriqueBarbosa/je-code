//! Testes extras do `Dialog` (o original só tem os cinco de `dialog.test.ts`): resultado por botão, Enter em
//! campo, Escape, ordem dos botões, Tab, armadilha de foco, caixa de seleção e botão de fechar. Assumem Linux
//! (a ordem dos botões depende do sistema).

mod support;

use je_base::key_codes::KeyCode;
use je_base::lifecycle::IDisposable;
use je_base::promise::Promise;
use je_ui::{KeyData, Modifiers, NodeId, Role, Ui};
use je_widgets::button::unthemed_button_styles;
use je_widgets::dialog::{Dialog, DialogInputOptions, DialogOptions, DialogResult, DialogStyles};
use je_widgets::input_box::unthemed_input_box_styles;
use je_widgets::severity_icon::SeverityIcon;
use je_widgets::toggle::CheckboxStyles;
use je_base::severity::Severity;
use support::ensure_no_disposables_are_leaked_in_test_suite;

fn options() -> DialogOptions {
    DialogOptions::new(unthemed_button_styles(), CheckboxStyles::default(), unthemed_input_box_styles(), DialogStyles::default())
}

fn key(ui: &Ui, key_code: KeyCode) {
    let data = KeyData { key_code, modifiers: Modifiers::default() };
    ui.key_input(true, data);
    ui.key_input(false, data);
}

fn result_of(promise: &Promise<DialogResult>) -> DialogResult {
    promise.result().expect("resolvida").expect("sem erro")
}

fn labels(dialog: &Dialog) -> Vec<String> {
    let ui_buttons = dialog.buttons();
    ui_buttons.iter().map(|b| b.element()).map(|e| format!("{e:?}")).collect()
}

#[test]
fn enter_on_the_focused_primary_button_resolves_with_index_zero_and_the_checkbox() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let ui = Ui::new();
    let dialog = guard.add_rc(Dialog::new(&ui, ui.root(), "Msg", Some(vec!["Yes".into(), "No".into()]), DialogOptions { checkbox_label: Some("Again".into()), checkbox_checked: true, cancel_id: Some(1), ..options() }));
    let result = dialog.show();
    key(&ui, KeyCode::Enter);
    assert_eq!(result_of(&result), DialogResult { button: 0, checkbox_checked: Some(true), values: None });
}

#[test]
fn linux_order_puts_cancel_second_and_reverses() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let ui = Ui::new();
    let dialog = guard.add_rc(Dialog::new(&ui, ui.root(), "Msg", Some(vec!["Save".into(), "Don't".into(), "Cancel".into()]), DialogOptions { cancel_id: Some(2), ..options() }));
    let _result = dialog.show();
    let tree = ui.tree();
    let order: Vec<NodeId> = dialog.buttons().iter().map(|b| b.element()).collect();
    let x: Vec<f64> = order.iter().map(|n| tree.bounds(*n).x0).collect();
    assert!(x[0] < x[1] && x[1] < x[2], "{x:?}");
    // O primário (índice 0) é o último da fila, o do foco inicial.
    assert!(dialog.buttons()[2].has_focus());
    assert_eq!(labels(&dialog).len(), 3);
}

#[test]
fn escape_resolves_with_the_cancel_id_only_after_keydown_and_keyup() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let ui = Ui::new();
    let dialog = guard.add_rc(Dialog::new(&ui, ui.root(), "Msg", Some(vec!["Yes".into(), "No".into()]), DialogOptions { cancel_id: Some(1), ..options() }));
    let result = dialog.show();
    ui.key_input(false, KeyData { key_code: KeyCode::Escape, modifiers: Modifiers::default() });
    assert!(!result.is_settled());
    key(&ui, KeyCode::Escape);
    assert_eq!(result_of(&result).button, 1);
}

#[test]
fn disable_close_action_ignores_escape() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let ui = Ui::new();
    let dialog = guard.add_rc(Dialog::new(&ui, ui.root(), "Msg", None, DialogOptions { disable_close_action: true, ..options() }));
    let result = dialog.show();
    key(&ui, KeyCode::Escape);
    assert!(!result.is_settled());
    dialog.dispose();
    assert_eq!(result_of(&result).button, 0);
}

#[test]
fn enter_in_an_input_confirms_with_the_values() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let ui = Ui::new();
    let inputs = vec![DialogInputOptions { value: Some("abc".into()), ..Default::default() }, DialogInputOptions { value: Some("pw".into()), input_type: Some(je_widgets::input_box::InputType::Password), ..Default::default() }];
    let dialog = guard.add_rc(Dialog::new(&ui, ui.root(), "Msg", Some(vec!["Ok".into(), "Cancel".into()]), DialogOptions { cancel_id: Some(1), inputs: Some(inputs), ..options() }));
    let result = dialog.show();
    assert!(dialog.inputs()[0].has_focus(), "o primeiro campo recebe o foco");
    key(&ui, KeyCode::Enter);
    assert_eq!(result_of(&result), DialogResult { button: 0, checkbox_checked: None, values: Some(vec!["abc".into(), "pw".into()]) });
}

#[test]
fn tab_cycles_through_inputs_checkbox_and_buttons_with_wrapping() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let ui = Ui::new();
    let dialog = guard.add_rc(Dialog::new(&ui, ui.root(), "Msg", Some(vec!["Ok".into(), "Cancel".into()]), DialogOptions { cancel_id: Some(1), checkbox_label: Some("c".into()), inputs: Some(vec![DialogInputOptions::default()]), ..options() }));
    let _result = dialog.show();
    let checkbox = dialog.checkbox().expect("caixa").clone();
    let buttons = dialog.buttons();
    assert!(dialog.inputs()[0].has_focus());
    key(&ui, KeyCode::Tab);
    assert!(checkbox.has_focus());
    key(&ui, KeyCode::Tab);
    assert!(buttons[0].has_focus());
    key(&ui, KeyCode::Tab);
    assert!(buttons[1].has_focus());
    key(&ui, KeyCode::Tab);
    assert!(dialog.inputs()[0].has_focus(), "dá a volta");
    let shift = KeyData { key_code: KeyCode::Tab, modifiers: Modifiers { shift: true, ..Modifiers::default() } };
    ui.key_input(true, shift);
    assert!(buttons[1].has_focus(), "Shift+Tab volta para o último");
}

#[test]
fn focus_that_escapes_the_dialog_is_brought_back() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let ui = Ui::new();
    let outside = {
        let mut tree = ui.tree_mut();
        let root = tree.root();
        tree.create(root, Role::TextInput)
    };
    let dialog = guard.add_rc(Dialog::new(&ui, ui.root(), "Msg", Some(vec!["Ok".into()]), options()));
    let _result = dialog.show();
    let inside = ui.focused();
    ui.focus(outside);
    assert_eq!(ui.focused(), inside);
    dialog.dispose();
    ui.remove(outside);
}

#[test]
fn dispose_returns_the_focus_to_the_previous_element() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let ui = Ui::new();
    let before = {
        let mut tree = ui.tree_mut();
        let root = tree.root();
        tree.create(root, Role::TextInput)
    };
    ui.focus(before);
    let dialog = guard.add_rc(Dialog::new(&ui, ui.root(), "Msg", None, options()));
    let _result = dialog.show();
    assert_ne!(ui.focused(), Some(before));
    dialog.dispose();
    assert_eq!(ui.focused(), Some(before));
    assert_eq!(dialog.modal_element(), None);
}

#[test]
fn close_button_resolves_with_the_cancel_id_and_can_be_disabled() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let ui = Ui::new();
    let dialog = guard.add_rc(Dialog::new(&ui, ui.root(), "Msg", Some(vec!["Ok".into(), "No".into()]), DialogOptions { cancel_id: Some(1), ..options() }));
    let result = dialog.show();
    let toolbar = dialog.toolbar_container();
    assert!(!ui.tree().subtree(toolbar).iter().skip(1).collect::<Vec<_>>().is_empty());
    dialog.dispose();
    assert_eq!(result_of(&result).button, 1);

    let hidden = guard.add_rc(Dialog::new(&ui, ui.root(), "Msg", None, DialogOptions { disable_close_button: true, ..options() }));
    let _result = hidden.show();
    assert_eq!(ui.tree().subtree(hidden.toolbar_container()).len(), 1);
}

#[test]
fn box_is_centered_with_the_css_minimum_width() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let ui = Ui::new();
    let root = ui.root();
    ui.tree_mut().set_bounds(root, je_render::kurbo::Rect::new(0.0, 0.0, 1000.0, 700.0));
    let dialog = guard.add_rc(Dialog::new(&ui, ui.root(), "Msg", None, options()));
    let _result = dialog.show();
    let tree = ui.tree();
    let shadow = tree.bounds(dialog.shadow_element());
    assert_eq!(shadow.width(), 496.0);
    assert!(((shadow.x0 + shadow.x1) / 2.0 - 500.0).abs() <= 0.5);
    assert!(((shadow.y0 + shadow.y1) / 2.0 - 350.0).abs() <= 0.5);
    assert_eq!(tree.bounds(dialog.modal_element().expect("modal")).width(), 1000.0);
}

#[test]
fn severity_icon_class_names() {
    assert_eq!(SeverityIcon::class_name(Severity::Info), "codicon codicon-info");
    assert_eq!(SeverityIcon::class_name(Severity::Warning), "codicon codicon-warning");
    assert_eq!(SeverityIcon::class_name(Severity::Error), "codicon codicon-error");
    assert_eq!(SeverityIcon::class_name(Severity::Ignore), "severity-ignore codicon codicon-info");
}
