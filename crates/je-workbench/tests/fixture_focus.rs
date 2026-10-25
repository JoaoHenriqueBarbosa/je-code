//! Porte de `src/vs/workbench/test/browser/componentFixtures/fixtureFocus.test.ts`.
//!
//! Suíte `Component fixture focus`, caso a caso e na mesma ordem.
//!
//! Notas de tradução:
//! - **`ensureNoDisposablesAreLeakedInTestSuite()`.** É o [`LeakGuard`] criado no começo de cada caso; `disposables.add`
//!   vira `guard.add` e `guard.add_rc`.
//! - **DOM.** `$('button')` seguido de `mainWindow.document.body.append(...)` é um nó `Button` filho da raiz da `Ui`
//!   (a raiz faz o papel de `body`, `document` e `window`), `element.focus()` é o foco da `Ui` e
//!   `mainWindow.document.activeElement` é `ui.focused()`.
//! - **`await new Promise(resolve => mainWindow.setTimeout(resolve, 0))`.** É uma volta do laço de `je_base::runtime`
//!   (`turn`), que dispara os timers vencidos, entre eles o adiamento da perda de foco do `FocusTracker`.
//! - **`deepStrictEqual` de objetos.** São tuplas comparadas campo a campo, na ordem do literal do original.

#[path = "../../je-base/tests/common/mod.rs"]
mod common;

use common::ensure_no_disposables_are_leaked_in_test_suite;
use je_base::lifecycle::{DisposableStore, IDisposable, to_disposable};
use je_base::observable::base::Observable;
use je_ui::{Role, Ui};
use je_workbench::platform::user_interaction::user_interaction_service::UiElement;
use je_workbench::test::browser::component_fixtures::fixture_focus::{Focusable, apply_fixture_focus, create_fixture_user_interaction_service};

/// `$('button')` anexado ao `body`.
fn append_button(ui: &Ui) -> UiElement {
    let root = ui.root();
    let node = ui.tree_mut().create(root, Role::Button);
    UiElement::new(ui, node)
}

#[test]
fn defaults_to_deterministic_focus_and_leaves_focus_natural_when_disabled() {
    let disposables = ensure_no_disposables_are_leaked_in_test_suite();
    let ui = Ui::new();
    let first = append_button(&ui);
    let second = append_button(&ui);
    {
        let (ui, first, second) = (ui.clone(), first.node(), second.node());
        disposables.add(to_disposable(move || {
            ui.remove(first);
            ui.remove(second);
        }));
    }

    apply_fixture_focus(true, &first);
    let default_active_element = ui.focused();
    apply_fixture_focus(false, &second);

    // defaultFocused, focusUnchangedWhenDisabled
    assert_eq!(
        (default_active_element == Some(first.node()), ui.focused() == Some(first.node())),
        (true, true),
    );
}

#[test]
fn uses_fake_focus_by_default_and_tracks_real_dom_focus_for_the_fixture_lifetime() {
    let disposables = ensure_no_disposables_are_leaked_in_test_suite();
    let ui = Ui::new();
    let first = append_button(&ui);
    let second = append_button(&ui);
    {
        let (ui, first, second) = (ui.clone(), first.node(), second.node());
        disposables.add(to_disposable(move || {
            ui.remove(first);
            ui.remove(second);
        }));
    }

    let fake_store = disposables.add_rc(DisposableStore::new());
    let fake_focus = create_fixture_user_interaction_service(true, false).create_focus_tracker(&first, &fake_store);
    let real_store = disposables.add_rc(DisposableStore::new());
    let real_service = create_fixture_user_interaction_service(false, true);
    first.focus();
    let real_focused = real_service.create_focus_tracker(&first, &real_store);
    let real_unfocused = real_service.create_focus_tracker(&second, &real_store);
    let simulated_hover = real_service.create_hover_tracker(&first, &real_store);
    let during_lifetime = (real_focused.get(), real_unfocused.get());
    real_store.dispose();
    second.focus();
    je_base::runtime::turn();

    // fakeFocus, duringLifetime { focused, unfocused }, afterLifetime { focused, unfocused }, simulatedHover
    assert_eq!(
        (
            fake_focus.get(),
            during_lifetime,
            (real_focused.get(), real_unfocused.get()),
            simulated_hover.get(),
        ),
        (true, (true, false), (true, false), true),
    );
}
