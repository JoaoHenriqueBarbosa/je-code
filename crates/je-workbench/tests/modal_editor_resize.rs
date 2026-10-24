//! Porte de `src/vs/workbench/test/browser/parts/editor/modalEditorResize.test.ts` (suíte `Modal Editor Resize`), caso a
//! caso e na mesma ordem.
//!
//! Notas de tradução:
//! - `TestModalEditorResizeHost` do original é um dublê que "imita" o comportamento de redimensionamento do
//!   `ModalEditorPartImpl` (`toggleMaximized`, `handleHeaderDoubleClick`, `size` e `position`). Aqui o hospedeiro é o
//!   próprio [`ModalEditorPartState`], que guarda o mesmo estado e dispara os mesmos eventos, então o caso testa o código de
//!   produção em vez de uma cópia. `host.size = value` e `host.position = value` são `set_size` e `set_position`.
//! - `disposables.add(new TestModalEditorResizeHost())` é o `add_rc` do guarda de vazamentos, e
//!   `disposables.add(host.onDidChangeMaximized(...))` é o `add`.
//! - `assert.deepStrictEqual` de objetos é a comparação de tuplas (os tipos `EditorSize` e `EditorPosition` comparam por valor).

#[path = "../../je-base/tests/common/mod.rs"]
mod common;

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use common::{LeakGuard, ensure_no_disposables_are_leaked_in_test_suite};
use je_platform::opener::editor_types::{EditorPosition, EditorSize};
use je_workbench::browser::parts::editor::modal_editor_part::ModalEditorPartState;

/// `{ width, height }`.
fn size(width: f64, height: f64) -> EditorSize {
    EditorSize { width, height }
}

/// `{ left, top }`.
fn position(left: f64, top: f64) -> EditorPosition {
    EditorPosition { left, top }
}

/// `disposables.add(new TestModalEditorResizeHost())`.
fn new_host(guard: &LeakGuard) -> Rc<ModalEditorPartState> {
    guard.add_rc(ModalEditorPartState::new(None))
}

/// `disposables.add(host.onDidChangeMaximized(v => events.push(v)))`.
fn record_maximized_events(guard: &LeakGuard, host: &ModalEditorPartState) -> Rc<RefCell<Vec<bool>>> {
    let events = Rc::new(RefCell::new(Vec::new()));
    let recorded = events.clone();
    guard.add(host.on_did_change_maximized().listen(move |maximized| recorded.borrow_mut().push(*maximized)));
    events
}

#[test]
fn double_click_from_default_size_maximizes() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let host = new_host(&guard);

    let events = record_maximized_events(&guard, &host);

    host.handle_header_double_click();

    assert_eq!((host.maximized(), host.size(), events.borrow().clone()), (true, None, vec![true]));
}

#[test]
fn double_click_from_maximized_restores_default() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let host = new_host(&guard);

    host.handle_header_double_click(); // maximize

    let events = record_maximized_events(&guard, &host);

    host.handle_header_double_click(); // restore

    assert_eq!((host.maximized(), host.size(), events.borrow().clone()), (false, None, vec![false]));
}

#[test]
fn double_click_from_custom_size_restores_default_without_firing_maximized_event() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let host = new_host(&guard);

    host.set_size(Some(size(800.0, 600.0)));

    let maximized_events = record_maximized_events(&guard, &host);
    let layout_requested = Rc::new(Cell::new(false));
    let requested = layout_requested.clone();
    guard.add(host.on_did_request_layout().listen(move |_| requested.set(true)));

    host.handle_header_double_click();

    assert_eq!((host.maximized(), host.size(), maximized_events.borrow().clone(), layout_requested.get()), (false, None, Vec::<bool>::new(), true));
}

#[test]
fn double_click_cycle_custom_to_default_to_maximized_to_default() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let host = new_host(&guard);

    let events = record_maximized_events(&guard, &host);

    // Start with custom size
    host.set_size(Some(size(800.0, 600.0)));

    // First double-click: custom → default (fires layout, not maximized)
    host.handle_header_double_click();
    assert!(!host.maximized());
    assert_eq!(host.size(), None);

    // Second double-click: default → maximized
    host.handle_header_double_click();
    assert!(host.maximized());
    assert_eq!(host.size(), None);

    // Third double-click: maximized → default
    host.handle_header_double_click();
    assert!(!host.maximized());
    assert_eq!(host.size(), None);

    assert_eq!(*events.borrow(), vec![true, false]);
}

#[test]
fn toggle_maximized_preserves_custom_state_through_maximize_un_maximize_cycle() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let host = new_host(&guard);

    host.set_size(Some(size(800.0, 600.0)));
    host.set_position(Some(position(100.0, 50.0)));

    host.toggle_maximized();
    assert!(host.maximized());

    host.toggle_maximized();
    assert_eq!((host.maximized(), host.size(), host.position()), (false, Some(size(800.0, 600.0)), Some(position(100.0, 50.0))));
}

#[test]
fn double_click_from_maximized_clears_saved_custom_state() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let host = new_host(&guard);

    // Set custom size then maximize via toggleMaximized (saves state)
    host.set_size(Some(size(800.0, 600.0)));
    host.toggle_maximized();
    assert!(host.maximized());

    // Double-click to un-maximize: should go to default, not restore custom
    host.handle_header_double_click();
    assert_eq!((host.maximized(), host.size()), (false, None));
}

#[test]
fn double_click_clears_custom_position_along_with_size() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let host = new_host(&guard);

    host.set_size(Some(size(800.0, 600.0)));
    host.set_position(Some(position(100.0, 50.0)));

    host.handle_header_double_click();

    assert_eq!((host.size(), host.position(), host.maximized()), (None, None, false));
}

#[test]
fn session_persistence_state_can_be_saved_and_restored_across_instances() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let host1 = new_host(&guard);

    host1.set_size(Some(size(900.0, 700.0)));
    host1.set_position(Some(position(200.0, 100.0)));

    // Simulate saving state on close
    let saved_state = (host1.size(), host1.position(), host1.maximized());

    // Simulate restoring state on new modal
    let host2 = new_host(&guard);
    host2.set_size(saved_state.0);
    host2.set_position(saved_state.1);
    if saved_state.2 {
        host2.toggle_maximized();
    }

    assert_eq!((host2.size(), host2.position(), host2.maximized()), (Some(size(900.0, 700.0)), Some(position(200.0, 100.0)), false));
}
