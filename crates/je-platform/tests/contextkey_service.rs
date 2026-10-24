//! Porte de `src/vs/platform/contextkey/test/browser/contextkey.test.ts` (suíte `ContextKeyService`), caso a caso e na mesma ordem.
//!
//! Divergências: o `document.createElement('div')` é o [`TestTarget`] (só pai e atributos); o
//! `TestConfigurationService` é o de `je_platform::configuration::test_configuration_service`; os `DeferredPromise` do
//! original viram o registro do que o ouvinte viu, porque os eventos são síncronos; o `setContext` do teste
//! `issue #147732` é chamado direto sobre o serviço (o `invokeFunction` do `TestInstantiationService` só
//! entrega o acessor).

mod common;

use std::cell::{Cell, RefCell};
use std::collections::HashSet;
use std::rc::Rc;

use common::contextkey::TestTarget;
use common::*;
use je_base::dynamic::Dynamic;
use je_base::lifecycle::{DisposableRef, IDisposable};
use je_platform::configuration::test_configuration_service::TestConfigurationService;
use je_platform::contextkey::service::{ContextKeyService, set_context};
use je_platform::contextkey::{ContextKeyExpr, IContextKeyService};

fn new_root() -> Rc<ContextKeyService> {
    ContextKeyService::new(TestConfigurationService::new(None))
}

fn set(names: &[&str]) -> HashSet<String> {
    names.iter().map(|n| (*n).to_string()).collect()
}

#[test]
fn update_parent() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let root = guard.add_rc(new_root());
    let parent1 = root.create_scoped(TestTarget::new());
    guard.add(as_disposable_dyn(&parent1));
    let parent2 = root.create_scoped(TestTarget::new());
    guard.add(as_disposable_dyn(&parent2));

    let child = parent1.create_scoped(TestTarget::new());
    guard.add(as_disposable_dyn(&child));
    parent1.create_key("testA", n(1.0));
    parent1.create_key("testB", n(2.0));
    parent1.create_key("testD", n(0.0));

    parent2.create_key("testA", n(3.0));
    parent2.create_key("testC", n(4.0));
    parent2.create_key("testD", n(0.0));

    let failures: Rc<RefCell<Vec<String>>> = Rc::new(RefCell::new(Vec::new()));
    let completed = Rc::new(Cell::new(false));
    let observed_child = child.clone();
    let (observed_failures, observed_completed) = (failures.clone(), completed.clone());
    guard.add(child.on_did_change_context().listen(move |e| {
        let check = |ok: bool, message: &str| {
            if !ok {
                observed_failures.borrow_mut().push(message.to_string());
            }
        };
        check(e.affects_some(&set(&["testA"])), "testA changed");
        check(e.affects_some(&set(&["testB"])), "testB changed");
        check(e.affects_some(&set(&["testC"])), "testC changed");
        check(!e.affects_some(&set(&["testD"])), "testD did not change");

        check(observed_child.get_context_key_value("testA") == n(3.0), "testA == 3");
        check(observed_child.get_context_key_value("testB").is_undefined(), "testB undefined");
        check(observed_child.get_context_key_value("testC") == n(4.0), "testC == 4");
        check(observed_child.get_context_key_value("testD") == n(0.0), "testD == 0");
        observed_completed.set(true);
    }));

    child.update_parent(parent2.clone());

    assert!(completed.get(), "o evento de mudança deveria ter disparado");
    assert_eq!(*failures.borrow(), Vec::<String>::new());
}

#[test]
fn update_parent_to_same_service() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let root = guard.add_rc(new_root());
    let parent1 = root.create_scoped(TestTarget::new());
    guard.add(as_disposable_dyn(&parent1));

    let child = parent1.create_scoped(TestTarget::new());
    guard.add(as_disposable_dyn(&child));
    parent1.create_key("testA", n(1.0));
    parent1.create_key("testB", n(2.0));
    parent1.create_key("testD", n(0.0));

    let event_fired = Rc::new(Cell::new(false));
    let fired = event_fired.clone();
    guard.add(child.on_did_change_context().listen(move |_| fired.set(true)));

    child.update_parent(parent1.clone());

    assert!(!event_fired.get());
}

#[test]
fn issue_147732_uris_as_context_values() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let context_key_service = guard.add_rc(new_root());

    // `URI.parse('test://abc')`, e `JSON.parse(JSON.stringify([uri]))`
    let uri = je_base::uri::URI::parse("test://abc", false);
    context_key_service.create_key("notebookCellResource", Dynamic::Undefined).set(Dynamic::String(uri.to_string(false)));
    let marshalled = j("[{'$mid': 1, 'path': '', 'scheme': 'test', 'authority': 'abc'}]");
    set_context(&*context_key_service, &s("jupyter.runByLineCells"), &marshalled);

    let expr = ContextKeyExpr::in_("notebookCellResource", "jupyter.runByLineCells");
    assert!(context_key_service.context_matches_rules(Some(&expr)));
}

#[test]
fn suppress_update_event_from_parent_when_one_key_is_overridden_by_child() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let root = guard.add_rc(new_root());
    let child = root.create_scoped(TestTarget::new());
    guard.add(as_disposable_dyn(&child));

    root.create_key("testA", n(1.0));
    child.create_key("testA", n(4.0));

    let fired = Rc::new(Cell::new(false));
    let observed = fired.clone();
    let event = child.on_did_change_context().listen(move |_| observed.set(true));
    root.set_context("testA", n(10.0));
    assert!(!fired.get(), "Should not fire event when overridden key is updated in parent");
    event.dispose();
}

#[test]
fn suppress_update_event_from_parent_when_all_keys_are_overridden_by_child() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let root = guard.add_rc(new_root());
    let child = root.create_scoped(TestTarget::new());
    guard.add(as_disposable_dyn(&child));

    root.create_key("testA", n(1.0));
    root.create_key("testB", n(2.0));
    root.create_key("testC", n(3.0));

    child.create_key("testA", n(4.0));
    child.create_key("testB", n(5.0));
    child.create_key("testD", n(6.0));

    let fired = Rc::new(Cell::new(false));
    let observed = fired.clone();
    let event = child.on_did_change_context().listen(move |_| observed.set(true));
    let inner = root.clone();
    root.buffer_change_events(&move || {
        inner.set_context("testA", n(10.0));
        inner.set_context("testB", n(20.0));
        inner.set_context("testD", n(30.0));
    });

    assert!(!fired.get(), "Should not fire event when overridden key is updated in parent");
    event.dispose();
}

#[test]
fn pass_through_update_event_from_parent_when_one_key_is_not_overridden_by_child() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let root = guard.add_rc(new_root());
    let child = root.create_scoped(TestTarget::new());
    guard.add(as_disposable_dyn(&child));

    root.create_key("testA", n(1.0));
    root.create_key("testB", n(2.0));
    root.create_key("testC", n(3.0));

    child.create_key("testA", n(4.0));
    child.create_key("testB", n(5.0));
    child.create_key("testD", n(6.0));

    let failures: Rc<RefCell<Vec<String>>> = Rc::new(RefCell::new(Vec::new()));
    let completed = Rc::new(Cell::new(false));
    let (observed_failures, observed_completed) = (failures.clone(), completed.clone());
    guard.add(child.on_did_change_context().listen(move |e| {
        for key in ["testA", "testB", "testC"] {
            if !e.affects_some(&set(&[key])) {
                observed_failures.borrow_mut().push(format!("{key} changed"));
            }
        }
        observed_completed.set(true);
    }));

    let inner = root.clone();
    root.buffer_change_events(&move || {
        inner.set_context("testA", n(10.0));
        inner.set_context("testB", n(20.0));
        inner.set_context("testC", n(30.0));
    });

    assert!(completed.get(), "o evento do pai deveria ter chegado ao filho");
    assert_eq!(*failures.borrow(), Vec::<String>::new());
}

/// Cria a chave com o valor inicial, conta os eventos, atribui o novo valor e devolve se algum evento disparou.
fn set_and_check_fired(initial: Dynamic, value: Dynamic) -> bool {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let root = guard.add_rc(new_root());
    let key = root.create_key("testKey", initial);

    let event_fired = Rc::new(Cell::new(false));
    let fired = event_fired.clone();
    guard.add(root.on_did_change_context().listen(move |_| fired.set(true)));

    key.set(value);
    event_fired.get()
}

#[test]
fn setting_identical_array_values_should_not_fire_change_event() {
    // Set the same array content (different reference)
    assert!(!set_and_check_fired(j("['a', 'b', 'c']"), j("['a', 'b', 'c']")), "Should not fire event when setting identical array");
}

#[test]
fn setting_different_array_values_should_fire_change_event() {
    assert!(set_and_check_fired(j("['a', 'b', 'c']"), j("['a', 'b', 'd']")), "Should fire event when setting different array");
}

#[test]
fn setting_identical_complex_object_should_not_fire_change_event() {
    assert!(!set_and_check_fired(j("{'foo': 'bar', 'count': 42}"), j("{'foo': 'bar', 'count': 42}")), "Should not fire event when setting identical object");
}

#[test]
fn setting_different_complex_object_should_fire_change_event() {
    assert!(set_and_check_fired(j("{'foo': 'bar', 'count': 42}"), j("{'foo': 'bar', 'count': 43}")), "Should fire event when setting different object");
}

#[test]
fn setting_empty_arrays_should_not_fire_change_event_when_identical() {
    assert!(!set_and_check_fired(j("[]"), j("[]")), "Should not fire event when setting identical empty array");
}

#[test]
fn setting_nested_arrays_should_handle_deep_equality() {
    assert!(
        !set_and_check_fired(j("['a:b', 'c:d']"), j("['a:b', 'c:d']")),
        "Should not fire event when setting identical array with complex values"
    );
}

#[test]
fn setting_same_primitive_values_should_not_fire_change_event() {
    assert!(!set_and_check_fired(s("hello"), s("hello")), "Should not fire event when setting identical string");
}

#[test]
fn setting_different_primitive_values_should_fire_change_event() {
    assert!(set_and_check_fired(n(42.0), n(43.0)), "Should fire event when setting different number");
}

#[test]
fn dispose_context_forwards_through_disposed_scoped_service() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let root = guard.add_rc(new_root());
    let scoped = root.create_scoped(TestTarget::new());
    let child = scoped.create_scoped(TestTarget::new());

    // Set a value on the child so we can observe it
    child.create_key("testKey", s("value"));
    assert_eq!(child.get_context_key_value("testKey"), s("value"));

    // Dispose the intermediate scoped service first
    scoped.dispose();

    // Now dispose the child: this should still forward to root and clean up the child's context entry.
    // Before the fix, ScopedContextKeyService.disposeContext bailed out when _isDisposed was true,
    // leaking Context objects.
    child.dispose();

    // The child's context should no longer be accessible through root
    assert!(
        root.get_context_key_value("testKey").is_undefined(),
        "Child context should be cleaned up even when parent scoped service was disposed first"
    );
}

fn as_disposable_dyn(service: &Rc<dyn je_platform::contextkey::IScopedContextKeyService>) -> DisposableRef {
    service.clone()
}
