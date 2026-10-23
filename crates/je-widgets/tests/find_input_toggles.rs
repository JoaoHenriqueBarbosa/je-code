//! Porte de `src/vs/base/test/browser/ui/findinput/findInputToggles.test.ts` (suíte `navigateToggles`), caso a caso e na mesma ordem.
//!
//! Divergências: o `document.activeElement` é o nó com foco do [`Ui`]; o `StandardKeyboardEvent` do teste
//! original (um `KeyboardEvent` com `keyCode`) é um evento `KeyDown` do `je-ui` com a tecla; `blur` e `focus`
//! de um elemento são `Ui::blur` e `Ui::focus` (que dão `Blur` e `Focus` como o navegador).

mod support;

use std::cell::Cell;
use std::rc::Rc;

use je_base::key_codes::KeyCode;
use je_ui::{Event, EventKind, KeyData, Modifiers, NodeId, Role, Ui};
use je_widgets::find_input_toggles::navigate_toggles;
use support::{create_container, ensure_no_disposables_are_leaked_in_test_suite};

struct Fixture {
    ui: Ui,
    container: NodeId,
    toggles: Vec<NodeId>,
}

fn setup() -> Fixture {
    let ui = Ui::new();
    let container = create_container(&ui, 100.0, 20.0);
    let toggles = {
        let mut tree = ui.tree_mut();
        let root = tree.root();
        tree.append_child(root, container);
        (0..3).map(|_| tree.create(container, Role::Generic)).collect()
    };
    Fixture { ui, container, toggles }
}

fn keydown(fixture: &Fixture, key_code: KeyCode) -> Event {
    Event::new(EventKind::KeyDown, fixture.container).with_key(KeyData { key_code, modifiers: Modifiers::default() })
}

fn navigate(fixture: &Fixture, key_code: KeyCode, focus_input: impl FnOnce()) {
    navigate_toggles(&fixture.ui, &mut keydown(fixture, key_code), || fixture.toggles.clone(), focus_input);
}

#[test]
fn right_arrow_moves_to_the_next_toggle_and_wraps_around() {
    let _store = ensure_no_disposables_are_leaked_in_test_suite();
    let f = setup();
    f.ui.focus(f.toggles[0]);
    navigate(&f, KeyCode::RightArrow, || {});
    assert_eq!(f.ui.focused(), Some(f.toggles[1]));

    f.ui.focus(f.toggles[2]);
    navigate(&f, KeyCode::RightArrow, || {});
    assert_eq!(f.ui.focused(), Some(f.toggles[0]));
}

#[test]
fn left_arrow_moves_to_the_previous_toggle_and_wraps_around() {
    let _store = ensure_no_disposables_are_leaked_in_test_suite();
    let f = setup();
    f.ui.focus(f.toggles[2]);
    navigate(&f, KeyCode::LeftArrow, || {});
    assert_eq!(f.ui.focused(), Some(f.toggles[1]));

    f.ui.focus(f.toggles[0]);
    navigate(&f, KeyCode::LeftArrow, || {});
    assert_eq!(f.ui.focused(), Some(f.toggles[2]));
}

#[test]
fn escape_gives_the_focus_back_to_the_input() {
    let _store = ensure_no_disposables_are_leaked_in_test_suite();
    let f = setup();
    f.ui.focus(f.toggles[1]);

    let input_focused = Cell::new(false);
    navigate(&f, KeyCode::Escape, || input_focused.set(true));

    assert!(input_focused.get());
    assert_ne!(f.ui.focused(), Some(f.toggles[1]));
}

#[test]
fn does_nothing_when_the_focus_is_not_on_a_toggle() {
    let _store = ensure_no_disposables_are_leaked_in_test_suite();
    let f = setup();
    let outsider = f.ui.tree_mut().create(f.container, Role::Generic);
    f.ui.focus(outsider);

    navigate(&f, KeyCode::RightArrow, || {});

    assert_eq!(f.ui.focused(), Some(outsider));
}

#[test]
fn the_toggles_are_only_resolved_for_the_keys_that_navigate() {
    let _store = ensure_no_disposables_are_leaked_in_test_suite();
    let f = setup();
    f.ui.focus(f.toggles[0]);

    let resolved = Rc::new(Cell::new(0));
    let get_toggles = || {
        resolved.set(resolved.get() + 1);
        f.toggles.clone()
    };

    navigate_toggles(&f.ui, &mut keydown(&f, KeyCode::KeyA), get_toggles, || {});
    assert_eq!(resolved.get(), 0, "typing should not resolve the toggles");

    navigate_toggles(&f.ui, &mut keydown(&f, KeyCode::RightArrow), get_toggles, || {});
    assert_eq!(resolved.get(), 1);
}
