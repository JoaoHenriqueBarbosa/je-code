//! Porte de `src/vs/base/test/browser/actionbar.test.ts` (suíte `Actionbar`), caso a caso e na mesma ordem.
//!
//! Divergências de tradução (nenhuma enfraquece uma asserção):
//!
//! - `document.createElement('div')` é um nó solto da `Ui` ([`create_container`]).
//! - `assert(x instanceof ToggleActionViewItem)` é `constructor_name() == "ToggleActionViewItem"` e
//!   `x instanceof ActionViewItem` é o nome do tipo do [`ActionViewItem`] (o trait `IActionViewItem` não tem
//!   `as_any`; o nome é o `constructor.name` que o rastreador de vazamento já usa).
//! - `createToggleActionViewItemProvider(unthemedToggleStyles)` recebe a `Ui` (ver o módulo `toggle`).
//! - `action.checked = true` é `set_checked(Some(true))`; `checked === undefined` é `None`.
//! - `await import('quickInputUtils.js')` é um `use` do `je-platform` (dependência de desenvolvimento, só para
//!   este caso): o `quickInputButtonToAction` é do `je-platform`.
//! - O `Separator` e o `Action` viram `ActionRef`; a identidade (`===`) é o endereço do `Rc`.

mod support;

use std::rc::Rc;

use je_base::actions::{Action, ActionRef, IAction, Separator};
use je_base::lifecycle::IDisposable;
use je_platform::quickinput::quick_input::{IQuickInputButton, QuickInputButtonToggle};
use je_platform::quickinput::quick_input_utils::{RunFn, quick_input_button_to_action};
use je_ui::Ui;
use je_widgets::action_bar::{ActionBar, IActionBarOptions, IActionOptions, prepare_actions};
use je_widgets::action_view_items::{ActionViewItem, IActionViewItem};
use je_widgets::toggle::{create_toggle_action_view_item_provider, unthemed_toggle_styles};
use support::{LeakGuard, create_container, ensure_no_disposables_are_leaked_in_test_suite};

/// `store.add(new Action(id, label))`.
fn action(store: &LeakGuard, id: &str, label: &str) -> ActionRef {
    let action = store.add_rc(Action::new(id, label, "", true, None));
    action
}

fn same(a: &ActionRef, b: &ActionRef) -> bool {
    std::ptr::addr_eq(Rc::as_ptr(a), Rc::as_ptr(b))
}

/// `item instanceof ToggleActionViewItem`.
fn is_toggle(item: &Rc<dyn IActionViewItem>) -> bool {
    item.constructor_name() == "ToggleActionViewItem"
}

/// `item instanceof ActionViewItem`.
fn is_action_view_item(item: &Rc<dyn IActionViewItem>) -> bool {
    item.constructor_name() == std::any::type_name::<ActionViewItem>()
}

/// Uma barra com o provedor de toggles, num contêiner novo.
fn toggle_bar(store: &LeakGuard) -> (Ui, Rc<ActionBar>) {
    let ui = Ui::new();
    let container = create_container(&ui, 200.0, 30.0);
    let provider = create_toggle_action_view_item_provider(&ui, Some(unthemed_toggle_styles()));
    let actionbar = store.add_rc(ActionBar::new(&ui, container, IActionBarOptions { action_view_item_provider: Some(provider), ..Default::default() }));
    (ui, actionbar)
}

#[test]
fn prepare_actions_removes_duplicate_separators() {
    let store = ensure_no_disposables_are_leaked_in_test_suite();
    let a1: ActionRef = Separator::new();
    let a2: ActionRef = Separator::new();
    let a3 = action(&store, "a3", "");
    let a4: ActionRef = Separator::new();
    let a5: ActionRef = Separator::new();
    let a6 = action(&store, "a6", "");
    let a7: ActionRef = Separator::new();

    let actions = prepare_actions(vec![a1, a2, a3.clone(), a4, a5.clone(), a6.clone(), a7]);
    assert_eq!(actions.len(), 3); // duplicate separators get removed
    assert!(same(&actions[0], &a3));
    assert!(same(&actions[1], &a5));
    assert!(same(&actions[2], &a6));
}

#[test]
fn has_action() {
    let store = ensure_no_disposables_are_leaked_in_test_suite();
    let ui = Ui::new();
    let container = create_container(&ui, 200.0, 30.0);
    let actionbar = store.add_rc(ActionBar::new(&ui, container, IActionBarOptions::default()));

    let a1 = action(&store, "a1", "");
    let a2 = action(&store, "a2", "");

    actionbar.push_action(a1.clone(), IActionOptions::default());
    assert!(actionbar.has_action(&a1));
    assert!(!actionbar.has_action(&a2));

    actionbar.pull(0);
    assert!(!actionbar.has_action(&a1));

    actionbar.push_action(a1.clone(), IActionOptions { index: Some(1), ..Default::default() });
    actionbar.push_action(a2.clone(), IActionOptions { index: Some(0), ..Default::default() });
    assert!(actionbar.has_action(&a1));
    assert!(actionbar.has_action(&a2));

    actionbar.pull(0);
    assert!(actionbar.has_action(&a1));
    assert!(!actionbar.has_action(&a2));

    actionbar.pull(0);
    assert!(!actionbar.has_action(&a1));
    assert!(!actionbar.has_action(&a2));

    actionbar.push_action(a1.clone(), IActionOptions::default());
    assert!(actionbar.has_action(&a1));
    actionbar.clear();
    assert!(!actionbar.has_action(&a1));
}

#[test]
fn toggle_action_view_item_provider_renders_toggle_for_actions_with_checked_state() {
    let store = ensure_no_disposables_are_leaked_in_test_suite();
    let (_ui, actionbar) = toggle_bar(&store);

    let toggle_action = action(&store, "toggle", "Toggle");
    toggle_action.set_checked(Some(true));

    actionbar.push_action(toggle_action, IActionOptions::default());

    // Verify that the action was rendered as a toggle
    let items = actionbar.view_items();
    assert_eq!(items.len(), 1);
    assert!(is_toggle(&items[0]), "Action with checked state should render as ToggleActionViewItem");
}

#[test]
fn toggle_action_view_item_provider_renders_button_for_actions_without_checked_state() {
    let store = ensure_no_disposables_are_leaked_in_test_suite();
    let (_ui, actionbar) = toggle_bar(&store);

    let button_action = action(&store, "button", "Button");

    actionbar.push_action(button_action, IActionOptions::default());

    // Verify that the action was rendered as a regular button (ActionViewItem)
    let items = actionbar.view_items();
    assert_eq!(items.len(), 1);
    assert!(is_action_view_item(&items[0]), "Action without checked state should render as ActionViewItem");
    assert!(!is_toggle(&items[0]), "Action without checked state should not render as ToggleActionViewItem");
}

#[test]
fn toggle_action_view_item_provider_handles_mixed_actions_toggles_and_buttons() {
    let store = ensure_no_disposables_are_leaked_in_test_suite();
    let (_ui, actionbar) = toggle_bar(&store);

    let toggle_action = action(&store, "toggle", "Toggle");
    toggle_action.set_checked(Some(false));
    let button_action = action(&store, "button", "Button");

    actionbar.push(&[toggle_action, button_action], IActionOptions::default());

    // Verify that we have both types of items
    let items = actionbar.view_items();
    assert_eq!(items.len(), 2);
    assert!(is_toggle(&items[0]), "First action should be a toggle");
    assert!(is_action_view_item(&items[1]), "Second action should be a button");
    assert!(!is_toggle(&items[1]), "Second action should not be a toggle");
}

#[test]
fn toggle_action_view_item_provider_toggle_state_changes_when_action_checked_changes() {
    let store = ensure_no_disposables_are_leaked_in_test_suite();
    let (_ui, actionbar) = toggle_bar(&store);

    let toggle_action = action(&store, "toggle", "Toggle");
    toggle_action.set_checked(Some(false));

    actionbar.push_action(toggle_action.clone(), IActionOptions::default());

    // Verify the toggle view item was created
    let toggle_view_item = actionbar.view_items()[0].clone();
    assert!(is_toggle(&toggle_view_item), "Toggle view item should exist");

    // Change the action's checked state
    toggle_action.set_checked(Some(true));
    // The view item should reflect the updated checked state
    assert_eq!(toggle_action.checked(), Some(true), "Toggle action should update checked state");
}

#[test]
fn toggle_action_view_item_provider_quick_input_button_with_toggle_property_creates_action_with_checked_state() {
    let _store = ensure_no_disposables_are_leaked_in_test_suite();
    let noop = || -> RunFn { Rc::new(|| je_base::dynamic::Dynamic::Undefined.into()) };

    // Create a button with toggle property
    let toggle = Rc::new(QuickInputButtonToggle::default());
    toggle.checked.set(true);
    let toggle_button = IQuickInputButton { icon_class: Some("test-icon".to_owned()), tooltip: Some("Toggle Button".to_owned()), toggle: Some(toggle), ..Default::default() };

    let action = quick_input_button_to_action(&toggle_button, "test-id", noop());

    // Verify the action has checked property set
    assert_eq!(action.checked(), Some(true), "Action should have checked property set to true");

    // Create a button without toggle property
    let regular_button = IQuickInputButton { icon_class: Some("test-icon".to_owned()), tooltip: Some("Regular Button".to_owned()), ..Default::default() };

    let regular_action = quick_input_button_to_action(&regular_button, "test-id-2", noop());

    // Verify the action doesn't have checked property
    assert_eq!(regular_action.checked(), None, "Regular action should not have checked property");
}
