//! Porte de `src/vs/base/test/browser/ui/toggle/toggle.test.ts` (suíte `Toggle`), caso a caso e na mesma ordem.

mod support;

use je_base::lifecycle::IDisposable;
use je_ui::Ui;
use je_widgets::toggle::{Toggle, ToggleOpts, unthemed_toggle_styles};
use support::ensure_no_disposables_are_leaked_in_test_suite;

#[test]
fn focusable_toggle_is_removed_from_the_tab_order_when_disabled() {
    let _store = ensure_no_disposables_are_leaked_in_test_suite();
    let ui = Ui::new();
    let toggle = Toggle::new(&ui, ToggleOpts::new("toggle", false, unthemed_toggle_styles()));

    // Focusable toggles are tabbable by default.
    assert_eq!(toggle.tab_index(), 0);

    toggle.disable();
    assert_eq!(toggle.tab_index(), -1, "disabled toggle should not be tabbable");

    toggle.enable();
    assert_eq!(toggle.tab_index(), 0, "enabled toggle should be tabbable again");

    toggle.dispose();
}

#[test]
fn not_focusable_toggle_keeps_externally_managed_tab_index_when_enabled_disabled() {
    let _store = ensure_no_disposables_are_leaked_in_test_suite();
    let ui = Ui::new();
    let toggle = Toggle::new(&ui, ToggleOpts { not_focusable: true, ..ToggleOpts::new("toggle", false, unthemed_toggle_styles()) });

    // notFocusable toggles are never tabbable on their own; their tabIndex is managed by the owner.
    assert_eq!(toggle.tab_index(), -1);

    // The owner (e.g. a toolbar using roving tabindex) may make it focusable.
    toggle.set_tab_index(0);

    toggle.disable();
    assert_eq!(toggle.tab_index(), 0, "notFocusable toggle tabIndex should be left untouched on disable");

    toggle.enable();
    assert_eq!(toggle.tab_index(), 0, "notFocusable toggle tabIndex should be left untouched on enable");

    toggle.dispose();
}
