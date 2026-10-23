//! Porte de `src/vs/platform/quickinput/test/browser/quickInputUtils.test.ts` (suítes `QuickInputUtils`,
//! `quickInputButtonToAction` e `quickInputButtonsToActionArrays`), caso a caso e na mesma ordem.
//!
//! Divergências:
//! - O botão é um `IQuickInputButton` com `Default` (campos opcionais ausentes), e na lista de botões ele é
//!   compartilhado por identidade (`Rc`), porque o teste `should trigger callback with correct button` compara
//!   com `strictEqual` (aqui `Rc::ptr_eq`). O `toggle: { checked }` é o `Rc<QuickInputButtonToggle>` que o
//!   teste guarda e relê.
//! - Os `() => { }` e `() => { runCalled = true; }` do original, que devolvem `void`, devolvem
//!   `Dynamic::Undefined` (o `unknown` do `RunFn`). `runCalled` é um `Rc<Cell<bool>>`.
//! - `action.class?.includes(x)` é `class().is_some_and(|c| c.contains(x))`. `action.run()` devolve a promessa
//!   do porte e o teste a descarta, como o original descarta o retorno.
//! - Nenhum caso depende do DOM: o `createCSSRule` de `getIconClass`, que o primeiro caso exercita ao dar
//!   `iconPath`, vira o registro `registered_icon_classes`, e o teste só pede que a classe exista.

mod common;

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use common::ensure_no_disposables_are_leaked_in_test_suite;
use je_base::actions::IAction;
use je_base::dynamic::Dynamic;
use je_base::path::Platform;
use je_base::uri::URI;
use je_platform::quickinput::quick_input::{IQuickInputButton, QuickInputButtonRef, QuickInputButtonToggle, QuickInputIconPath};
use je_platform::quickinput::quick_input_utils::{
    OnTrigger, RunFn, quick_input_button_to_action, quick_input_buttons_to_action_arrays, registered_icon_classes,
};

fn noop() -> RunFn {
    Rc::new(|| Dynamic::Undefined.into())
}

fn noop_trigger() -> OnTrigger {
    Rc::new(|_: &QuickInputButtonRef| Dynamic::Undefined.into())
}

/// Um botão com classe de ícone e dica, o literal mais comum do original.
fn button(icon_class: &str, tooltip: &str) -> IQuickInputButton {
    IQuickInputButton { icon_class: Some(icon_class.to_owned()), tooltip: Some(tooltip.to_owned()), ..Default::default() }
}

fn shared(button: IQuickInputButton) -> QuickInputButtonRef {
    Rc::new(button)
}

// quickInputButtonToAction

#[test]
fn should_convert_simple_button_to_action() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    let button = IQuickInputButton {
        icon_path: Some(QuickInputIconPath { dark: URI::file("/path/to/icon.svg", Platform::Posix), light: None }),
        tooltip: Some("Test Tooltip".to_owned()),
        ..Default::default()
    };

    let run_called = Rc::new(Cell::new(false));
    let action = quick_input_button_to_action(&button, "test-id", {
        let run_called = run_called.clone();
        Rc::new(move || {
            run_called.set(true);
            Dynamic::Undefined.into()
        })
    });

    assert_eq!(action.id(), "test-id");
    assert_eq!(action.tooltip(), "Test Tooltip");
    assert!(action.enabled());
    assert!(action.class().is_some_and(|class_name| !class_name.is_empty()));
    assert_eq!(registered_icon_classes().len(), 1);

    let _ = action.run(&[]);
    assert!(run_called.get());
}

#[test]
fn should_handle_button_with_icon_class() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    let button = button("custom-icon-class", "Test");

    let action = quick_input_button_to_action(&button, "test-id", noop());

    assert!(action.class().is_some_and(|class_name| class_name.contains("custom-icon-class")));
}

#[test]
fn should_handle_always_visible_button() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    let button = IQuickInputButton { always_visible: Some(true), ..button("icon-class", "Test") };

    let action = quick_input_button_to_action(&button, "test-id", noop());

    assert!(action.class().is_some_and(|class_name| class_name.contains("always-visible")));
    assert!(action.class().is_some_and(|class_name| class_name.contains("icon-class")));
}

#[test]
fn should_handle_always_visible_without_icon_class() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    let button = IQuickInputButton { tooltip: Some("Test".to_owned()), always_visible: Some(true), ..Default::default() };

    let action = quick_input_button_to_action(&button, "test-id", noop());

    assert_eq!(action.class().as_deref(), Some("always-visible"));
}

#[test]
fn should_handle_toggle_button() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    let toggle = Rc::new(QuickInputButtonToggle::default());
    let button = IQuickInputButton { toggle: Some(toggle.clone()), ..button("toggle-icon", "Toggle Test") };

    let run_called = Rc::new(Cell::new(false));
    let action = quick_input_button_to_action(&button, "toggle-id", {
        let run_called = run_called.clone();
        Rc::new(move || {
            run_called.set(true);
            Dynamic::Undefined.into()
        })
    });

    assert_eq!(action.id(), "toggle-id");
    // For toggle buttons, tooltip is used as label
    assert_eq!(action.label(), "Toggle Test");
    assert_eq!(action.tooltip(), "");
    assert_ne!(action.checked(), None);

    // Initial state
    assert_eq!(action.checked(), Some(false));
    assert!(!toggle.checked.get());

    // Run the action
    let _ = action.run(&[]);
    assert!(run_called.get());

    // Toggle state should be flipped
    assert_eq!(action.checked(), Some(true));
    assert!(toggle.checked.get());
}

#[test]
fn should_handle_toggle_button_with_initial_checked_state() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    let toggle = Rc::new(QuickInputButtonToggle::default());
    toggle.checked.set(true);
    let button = IQuickInputButton { toggle: Some(toggle.clone()), ..button("toggle-icon", "Toggle Test") };

    let action = quick_input_button_to_action(&button, "toggle-id", noop());

    assert_eq!(action.checked(), Some(true));
    assert!(toggle.checked.get());

    // Run should flip the state
    let _ = action.run(&[]);

    assert_eq!(action.checked(), Some(false));
    assert!(!toggle.checked.get());
}

#[test]
fn should_use_empty_string_for_tooltip_when_not_provided() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    let button = IQuickInputButton { icon_class: Some("icon".to_owned()), ..Default::default() };

    let action = quick_input_button_to_action(&button, "test-id", noop());

    assert_eq!(action.tooltip(), "");
}

#[test]
fn should_handle_button_with_label() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    let button = IQuickInputButton { label: Some("Button Label".to_owned()), ..button("icon", "Test") };

    let action = quick_input_button_to_action(&button, "test-id", noop());

    // The label property exists on the button but the action's label is initially empty
    assert_eq!(action.label(), "");
}

// quickInputButtonsToActionArrays

#[test]
fn should_convert_empty_array() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    let buttons: Vec<QuickInputButtonRef> = Vec::new();

    let result = quick_input_buttons_to_action_arrays(&buttons, "prefix", noop_trigger());

    assert_eq!(result.primary.len(), 0);
    assert_eq!(result.secondary.len(), 0);
}

#[test]
fn should_convert_primary_buttons() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    let buttons = vec![shared(button("icon1", "Button 1")), shared(button("icon2", "Button 2"))];

    let result = quick_input_buttons_to_action_arrays(&buttons, "test", noop_trigger());

    assert_eq!(result.primary.len(), 2);
    assert_eq!(result.secondary.len(), 0);
    assert_eq!(result.primary[0].id(), "test-0");
    assert_eq!(result.primary[1].id(), "test-1");
}

#[test]
fn should_convert_secondary_buttons() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    let buttons = vec![
        shared(IQuickInputButton { secondary: Some(true), ..button("icon1", "Button 1") }),
        shared(IQuickInputButton { secondary: Some(true), ..button("icon2", "Button 2") }),
    ];

    let result = quick_input_buttons_to_action_arrays(&buttons, "test", noop_trigger());

    assert_eq!(result.primary.len(), 0);
    assert_eq!(result.secondary.len(), 2);
    assert_eq!(result.secondary[0].id(), "test-0");
    assert_eq!(result.secondary[1].id(), "test-1");
}

#[test]
fn should_convert_mixed_primary_and_secondary_buttons() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    let buttons = vec![
        shared(button("icon1", "Primary 1")),
        shared(IQuickInputButton { secondary: Some(true), ..button("icon2", "Secondary 1") }),
        shared(button("icon3", "Primary 2")),
        shared(IQuickInputButton { secondary: Some(true), ..button("icon4", "Secondary 2") }),
    ];

    let result = quick_input_buttons_to_action_arrays(&buttons, "test", noop_trigger());

    assert_eq!(result.primary.len(), 2);
    assert_eq!(result.secondary.len(), 2);
    assert_eq!(result.primary[0].id(), "test-0");
    assert_eq!(result.primary[1].id(), "test-2");
    assert_eq!(result.secondary[0].id(), "test-1");
    assert_eq!(result.secondary[1].id(), "test-3");
}

#[test]
fn should_apply_label_to_actions() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    let buttons = vec![
        shared(IQuickInputButton { label: Some("Label 1".to_owned()), ..button("icon1", "Button 1") }),
        shared(button("icon2", "Button 2")),
    ];

    let result = quick_input_buttons_to_action_arrays(&buttons, "test", noop_trigger());

    assert_eq!(result.primary[0].label(), "Label 1");
    assert_eq!(result.primary[1].label(), "");
}

#[test]
fn should_trigger_callback_with_correct_button() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    let button1 = shared(button("icon1", "Button 1"));
    let button2 = shared(button("icon2", "Button 2"));
    let buttons = vec![button1.clone(), button2.clone()];

    let triggered_buttons: Rc<RefCell<Vec<QuickInputButtonRef>>> = Rc::new(RefCell::new(Vec::new()));
    let result = quick_input_buttons_to_action_arrays(&buttons, "test", {
        let triggered_buttons = triggered_buttons.clone();
        Rc::new(move |button: &QuickInputButtonRef| {
            triggered_buttons.borrow_mut().push(button.clone());
            Dynamic::Undefined.into()
        })
    });

    let _ = result.primary[0].run(&[]);
    assert_eq!(triggered_buttons.borrow().len(), 1);
    assert!(Rc::ptr_eq(&triggered_buttons.borrow()[0], &button1));

    let _ = result.primary[1].run(&[]);
    assert_eq!(triggered_buttons.borrow().len(), 2);
    assert!(Rc::ptr_eq(&triggered_buttons.borrow()[1], &button2));
}

#[test]
fn should_handle_toggle_buttons_in_arrays() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    let toggle = Rc::new(QuickInputButtonToggle::default());
    let buttons = vec![
        shared(IQuickInputButton { toggle: Some(toggle.clone()), ..button("icon1", "Toggle") }),
        shared(button("icon2", "Regular")),
    ];

    let result = quick_input_buttons_to_action_arrays(&buttons, "test", noop_trigger());

    let toggle_action = &result.primary[0];
    assert_eq!(toggle_action.checked(), Some(false));
    let _ = toggle_action.run(&[]);
    assert_eq!(toggle_action.checked(), Some(true));
    assert!(toggle.checked.get());
}

#[test]
fn should_use_correct_id_prefix() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    let buttons = vec![shared(button("icon1", "Button 1"))];

    let result1 = quick_input_buttons_to_action_arrays(&buttons, "custom-prefix", noop_trigger());
    assert_eq!(result1.primary[0].id(), "custom-prefix-0");

    let result2 = quick_input_buttons_to_action_arrays(&buttons, "another", noop_trigger());
    assert_eq!(result2.primary[0].id(), "another-0");
}
