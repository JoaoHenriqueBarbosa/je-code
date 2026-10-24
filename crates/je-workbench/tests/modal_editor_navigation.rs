//! Porte de `src/vs/workbench/test/browser/parts/editor/modalEditorNavigation.test.ts` (suíte `Modal Editor Navigation`),
//! caso a caso e na mesma ordem.
//!
//! Notas de tradução:
//! - `TestModalEditorNavigationHost` do original é um dublê que "imita" o comportamento de navegação do
//!   `ModalEditorPartImpl` (`updateOptions` guarda a navegação e dispara `onDidChangeNavigation`). Aqui o hospedeiro é o
//!   próprio [`ModalEditorPartState`], que guarda o mesmo estado e dispara o mesmo evento, então o caso testa o código de
//!   produção em vez de uma cópia. `host.updateOptions({ navigation })` é `update_options` com as opções que só têm a
//!   navegação (o `maximized` ausente não mexe no estado, como no original).
//! - `disposables.add({ dispose: () => host.dispose() })` e `disposables.add(host.onDidChangeNavigation(...))` são o
//!   `add_rc` e o `add` do guarda de vazamentos.
//! - **Identidade do contexto.** `assert.strictEqual(host.navigation, nav)` e `deepStrictEqual(events, [nav])` comparam o
//!   objeto (e a função `navigate` por identidade): [`same_navigation`] compara `total`, `current` e o `Rc` de `navigate`.
//! - **`navigate` que se refere a si mesmo.** No original o `const navigate = (index) => { ...; host.updateOptions({
//!   navigation: { total, current: index, navigate } }); }` guarda a si mesmo na navegação. Aqui cada chamada monta o
//!   próximo contexto com um `navigate` equivalente ([`navigate_fn`]); `current` e `total` são os mesmos. O ciclo entre
//!   a parte e o fecho é desfeito por `ModalEditorPartState::dispose`, como no original.

#[path = "../../je-base/tests/common/mod.rs"]
mod common;

use std::cell::RefCell;
use std::rc::Rc;

use common::{LeakGuard, ensure_no_disposables_are_leaked_in_test_suite};
use je_platform::opener::editor_types::{ModalEditorNavigation, ModalEditorPartOptions};
use je_workbench::browser::parts::editor::modal_editor_part::ModalEditorPartState;

/// `{ total, current, navigate }`.
fn navigation(total: f64, current: f64, navigate: Rc<dyn Fn(f64)>) -> ModalEditorNavigation {
    ModalEditorNavigation { total, current, navigate }
}

/// `host.updateOptions({ navigation })` e `host.updateOptions({ navigation: undefined })`.
fn options(navigation: Option<ModalEditorNavigation>) -> ModalEditorPartOptions {
    ModalEditorPartOptions { navigation, ..ModalEditorPartOptions::default() }
}

/// `() => { }`.
fn no_navigate() -> Rc<dyn Fn(f64)> {
    Rc::new(|_| {})
}

/// A identidade de um contexto de navegação: os números e a função `navigate`.
fn same_navigation(a: Option<&ModalEditorNavigation>, b: Option<&ModalEditorNavigation>) -> bool {
    match (a, b) {
        (None, None) => true,
        (Some(a), Some(b)) => a.total == b.total && a.current == b.current && Rc::ptr_eq(&a.navigate, &b.navigate),
        _ => false,
    }
}

/// O `navigate` do original que, a cada índice, registra (se `log` existir), valida (`valid`) e atualiza a navegação do
/// hospedeiro com `{ total, current: index, navigate }`.
fn navigate_fn(host: &Rc<ModalEditorPartState>, total: f64, log: Option<Rc<RefCell<Vec<f64>>>>, valid: fn(f64) -> bool) -> Rc<dyn Fn(f64)> {
    let host = host.clone();
    Rc::new(move |index| {
        if let Some(log) = &log {
            log.borrow_mut().push(index);
        }
        if valid(index) {
            let next = navigation(total, index, navigate_fn(&host, total, log.clone(), valid));
            host.update_options(Some(&options(Some(next))));
        }
    })
}

/// `disposables.add({ dispose: () => host.dispose() })`.
fn new_host(guard: &LeakGuard) -> Rc<ModalEditorPartState> {
    guard.add_rc(ModalEditorPartState::new(None))
}

/// `disposables.add(host.onDidChangeNavigation(ctx => events.push(ctx)))`.
fn record_navigation_events(guard: &LeakGuard, host: &ModalEditorPartState) -> Rc<RefCell<Vec<Option<ModalEditorNavigation>>>> {
    let events = Rc::new(RefCell::new(Vec::new()));
    let recorded = events.clone();
    guard.add(host.on_did_change_navigation().listen(move |context| recorded.borrow_mut().push(context.clone())));
    events
}

#[test]
fn update_options_sets_navigation_and_fires_event() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let host = new_host(&guard);

    let events = record_navigation_events(&guard, &host);

    let nav = navigation(10.0, 3.0, no_navigate());

    host.update_options(Some(&options(Some(nav.clone()))));

    assert!(same_navigation(host.navigation().as_ref(), Some(&nav)));
    let events = events.borrow();
    assert_eq!(events.len(), 1);
    assert!(same_navigation(events[0].as_ref(), Some(&nav)));
}

#[test]
fn update_options_with_undefined_navigation_clears_navigation() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let host = new_host(&guard);

    let events = record_navigation_events(&guard, &host);

    let nav = navigation(5.0, 0.0, no_navigate());

    host.update_options(Some(&options(Some(nav.clone()))));
    host.update_options(Some(&options(None)));

    assert!(same_navigation(host.navigation().as_ref(), None));
    let events = events.borrow();
    assert_eq!(events.len(), 2);
    assert!(same_navigation(events[0].as_ref(), Some(&nav)));
    assert!(same_navigation(events[1].as_ref(), None));
}

#[test]
fn navigate_callback_updates_context() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let host = new_host(&guard);

    let navigated_indices = Rc::new(RefCell::new(Vec::new()));

    // Simulates what real navigation does: update the context with new index
    let navigate = navigate_fn(&host, 10.0, Some(navigated_indices.clone()), |_| true);

    host.update_options(Some(&options(Some(navigation(10.0, 0.0, navigate)))));

    // Navigate forward
    (host.navigation().expect("a navegação foi definida").navigate)(1.0);
    assert_eq!(host.navigation().expect("a navegação foi definida").current, 1.0);

    (host.navigation().expect("a navegação foi definida").navigate)(5.0);
    assert_eq!(host.navigation().expect("a navegação foi definida").current, 5.0);

    assert_eq!(*navigated_indices.borrow(), vec![1.0, 5.0]);
}

#[test]
fn navigation_boundary_conditions() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let host = new_host(&guard);

    let navigate = navigate_fn(&host, 3.0, None, |index| (0.0..3.0).contains(&index));

    host.update_options(Some(&options(Some(navigation(3.0, 0.0, navigate)))));

    // At first item
    let current = || host.navigation().expect("a navegação foi definida");
    assert_eq!(current().current, 0.0);
    assert!(current().current <= 0.0); // previous disabled

    // Navigate to last
    (current().navigate)(2.0);
    assert_eq!(current().current, 2.0);
    assert!(current().current >= current().total - 1.0); // next disabled

    // Navigate back to middle
    (current().navigate)(1.0);
    assert_eq!(current().current, 1.0);
}

#[test]
fn navigation_context_fires_multiple_events() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let host = new_host(&guard);

    let events = record_navigation_events(&guard, &host);

    let navigate = navigate_fn(&host, 5.0, None, |_| true);

    host.update_options(Some(&options(Some(navigation(5.0, 0.0, navigate)))));
    (host.navigation().expect("a navegação foi definida").navigate)(1.0);
    (host.navigation().expect("a navegação foi definida").navigate)(2.0);
    host.update_options(Some(&options(None)));

    assert_eq!(events.borrow().len(), 4); // initial + 2 navigates + clear
}
