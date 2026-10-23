//! Porte de `src/vs/platform/storage/test/common/storageService.test.ts` (suíte `StorageService (in-memory)`), caso a caso e na mesma ordem.
//!
//! Divergências: o `createSuite<T extends IStorageService>({ setup, teardown })` (reutilizado por outras suítes
//! no original) é o conjunto de funções que recebem o serviço (`store_data`, `remove_data`, `keys_suite`), e cada
//! `test(...)` chama a função com o `InMemoryStorageService`. Os valores são os do `je-storage`
//! (`StorageValue`, e objetos como `Value` JSON); `undefined` é `StorageValue::Undefined`, e o argumento
//! `fallbackValue` ausente é `None`. `storageTargetEvent = Object.create(null)` (um objeto sem `scope`, para
//! reiniciar a captura) é `None`. `disposables` (o store de listeners, limpo no `teardown`) é o store da
//! guarda de vazamento.

mod common;

use std::cell::RefCell;
use std::rc::Rc;

use common::{LeakGuard, ensure_no_disposables_are_leaked_in_test_suite};
use je_base::json::{Value, parse_str};
use je_base::lifecycle::DisposableStore;
use je_platform::storage::{
    AbstractStorageService, InMemoryStorageService, StorageEntry, StorageScope, StorageService, StorageTarget, StorageTargetChangeEvent, StorageValueChangeEvent,
};
use je_storage::StorageValue;

fn setup() -> (LeakGuard, Rc<AbstractStorageService>) {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let service = guard.add_rc(InMemoryStorageService::new());
    (guard, service)
}

fn v(literal: &str) -> Value {
    let mut errors = Vec::new();
    parse_str(&literal.replace('\'', "\""), &mut errors).expect("literal JSON")
}

fn s(text: &str) -> StorageValue {
    StorageValue::String(text.to_owned())
}

type Events = Rc<RefCell<Vec<StorageValueChangeEvent>>>;

/// `storageService.onDidChangeValue(scope, key, disposables)(e => events.push(e), undefined, disposables)`.
fn collect_events(service: &dyn StorageService, scope: StorageScope, key: Option<&str>, disposables: &Rc<DisposableStore>) -> Events {
    let events: Events = Rc::default();
    let sink = events.clone();
    service.on_did_change_value(scope, key, disposables).listen_in(move |e| sink.borrow_mut().push(e.clone()), disposables);
    events
}

fn find(events: &Events, key: &str) -> Option<StorageValueChangeEvent> {
    events.borrow().iter().find(|e| e.key == key).cloned()
}

#[test]
fn get_data_integer_boolean_application() {
    let (guard, service) = setup();
    store_data(&service, StorageScope::Application, &guard.store());
}

#[test]
fn get_data_integer_boolean_profile() {
    let (guard, service) = setup();
    store_data(&service, StorageScope::Profile, &guard.store());
}

#[test]
fn get_data_integer_boolean_object_workspace() {
    let (guard, service) = setup();
    store_data(&service, StorageScope::Workspace, &guard.store());
}

#[test]
fn storage_change_source() {
    let (guard, service) = setup();
    let disposables = guard.store();
    let events = collect_events(&service, StorageScope::Workspace, None, &disposables);

    let entry = |key: &str, value: &str| StorageEntry { key: key.to_owned(), value: s(value), scope: StorageScope::Workspace, target: StorageTarget::Machine };

    // Explicit external source
    service.store_all(&[entry("testExternalChange", "foobar")], true);
    assert_eq!(find(&events, "testExternalChange").map(|e| e.external), Some(true));

    // Default source
    service.store_all(&[entry("testChange", "barfoo")], false);
    assert_eq!(find(&events, "testChange").map(|e| e.external), Some(false));

    service.store("testChange", s("foobar"), StorageScope::Workspace, StorageTarget::Machine);
    assert_eq!(find(&events, "testChange").map(|e| e.external), Some(false));
}

#[test]
fn storage_change_event_scope_all_keys() {
    let (guard, service) = setup();
    let events = collect_events(&service, StorageScope::Workspace, None, &guard.store());

    service.store("testChange", s("foobar"), StorageScope::Workspace, StorageTarget::Machine);
    service.store("testChange2", s("foobar"), StorageScope::Workspace, StorageTarget::Machine);
    service.store("testChange", s("foobar"), StorageScope::Application, StorageTarget::Machine);
    service.store("testChange", s("foobar"), StorageScope::Profile, StorageTarget::Machine);
    service.store("testChange2", s("foobar"), StorageScope::Profile, StorageTarget::Machine);
    assert_eq!(events.borrow().len(), 2);
}

#[test]
fn storage_change_event_scope_specific_key() {
    let (guard, service) = setup();
    let events = collect_events(&service, StorageScope::Workspace, Some("testChange"), &guard.store());

    service.store("testChange", s("foobar"), StorageScope::Workspace, StorageTarget::Machine);
    service.store("testChange", s("foobar"), StorageScope::Profile, StorageTarget::User);
    service.store("testChange", s("foobar"), StorageScope::Application, StorageTarget::Machine);
    service.store("testChange2", s("foobar"), StorageScope::Workspace, StorageTarget::Machine);
    assert!(find(&events, "testChange").is_some());
    assert_eq!(events.borrow().len(), 1);
}

fn store_data(service: &Rc<AbstractStorageService>, scope: StorageScope, disposables: &Rc<DisposableStore>) {
    let events = collect_events(service, scope, None, disposables);

    assert_eq!(service.get("test.get", scope, Some("foobar")), Some("foobar".to_owned()));
    assert_eq!(service.get("test.get", scope, Some("")), Some(String::new()));
    assert_eq!(service.get_number("test.getNumber", scope, Some(5.0)), Some(5.0));
    assert_eq!(service.get_number("test.getNumber", scope, Some(0.0)), Some(0.0));
    assert_eq!(service.get_boolean("test.getBoolean", scope, Some(true)), Some(true));
    assert_eq!(service.get_boolean("test.getBoolean", scope, Some(false)), Some(false));
    assert_eq!(service.get_object("test.getObject", scope, Some(v("{ 'foo': 'bar' }"))), Some(v("{ 'foo': 'bar' }")));
    assert_eq!(service.get_object("test.getObject", scope, Some(v("{}"))), Some(v("{}")));
    assert_eq!(service.get_object("test.getObject", scope, Some(v("[]"))), Some(v("[]")));

    service.store("test.get", s("foobar"), scope, StorageTarget::Machine);
    assert_eq!(service.get("test.get", scope, None), Some("foobar".to_owned()));
    let event = find(&events, "test.get");
    assert_eq!(event.as_ref().map(|e| e.scope), Some(scope));
    assert_eq!(event.as_ref().map(|e| e.key.as_str()), Some("test.get"));
    events.borrow_mut().clear();

    service.store("test.get", s(""), scope, StorageTarget::Machine);
    assert_eq!(service.get("test.get", scope, None), Some(String::new()));
    let event = find(&events, "test.get").unwrap();
    assert_eq!(event.scope, scope);
    assert_eq!(event.key, "test.get");

    service.store("test.getNumber", StorageValue::Number(5.0), scope, StorageTarget::Machine);
    assert_eq!(service.get_number("test.getNumber", scope, None), Some(5.0));

    service.store("test.getNumber", StorageValue::Number(0.0), scope, StorageTarget::Machine);
    assert_eq!(service.get_number("test.getNumber", scope, None), Some(0.0));

    service.store("test.getBoolean", StorageValue::Bool(true), scope, StorageTarget::Machine);
    assert_eq!(service.get_boolean("test.getBoolean", scope, None), Some(true));

    service.store("test.getBoolean", StorageValue::Bool(false), scope, StorageTarget::Machine);
    assert_eq!(service.get_boolean("test.getBoolean", scope, None), Some(false));

    service.store("test.getObject", StorageValue::Object(v("{}")), scope, StorageTarget::Machine);
    assert_eq!(service.get_object("test.getObject", scope, None), Some(v("{}")));

    service.store("test.getObject", StorageValue::Object(v("[42]")), scope, StorageTarget::Machine);
    assert_eq!(service.get_object("test.getObject", scope, None), Some(v("[42]")));

    service.store("test.getObject", StorageValue::Object(v("{ 'foo': {} }")), scope, StorageTarget::Machine);
    assert_eq!(service.get_object("test.getObject", scope, None), Some(v("{ 'foo': {} }")));

    assert_eq!(service.get("test.getDefault", scope, Some("getDefault")), Some("getDefault".to_owned()));
    assert_eq!(service.get_number("test.getNumberDefault", scope, Some(5.0)), Some(5.0));
    assert_eq!(service.get_boolean("test.getBooleanDefault", scope, Some(true)), Some(true));
    assert_eq!(service.get_object("test.getObjectDefault", scope, Some(v("{ 'foo': 42 }"))), Some(v("{ 'foo': 42 }")));

    service.store_all(
        &[
            StorageEntry { key: "test.storeAll1".to_owned(), value: s("foobar"), scope, target: StorageTarget::Machine },
            StorageEntry { key: "test.storeAll2".to_owned(), value: StorageValue::Number(4.0), scope, target: StorageTarget::Machine },
            StorageEntry { key: "test.storeAll3".to_owned(), value: StorageValue::Null, scope, target: StorageTarget::Machine },
        ],
        false,
    );

    assert_eq!(service.get("test.storeAll1", scope, Some("foobar")), Some("foobar".to_owned()));
    assert_eq!(service.get("test.storeAll2", scope, Some("4")), Some("4".to_owned()));
    assert_eq!(service.get("test.storeAll3", scope, Some("null")), Some("null".to_owned()));
}

#[test]
fn remove_data_application() {
    let (guard, service) = setup();
    remove_data(&service, StorageScope::Application, &guard.store());
}

#[test]
fn remove_data_profile() {
    let (guard, service) = setup();
    remove_data(&service, StorageScope::Profile, &guard.store());
}

#[test]
fn remove_data_workspace() {
    let (guard, service) = setup();
    remove_data(&service, StorageScope::Workspace, &guard.store());
}

fn remove_data(service: &Rc<AbstractStorageService>, scope: StorageScope, disposables: &Rc<DisposableStore>) {
    let events = collect_events(service, scope, None, disposables);

    service.store("test.remove", s("foobar"), scope, StorageTarget::Machine);
    assert_eq!(Some("foobar".to_owned()), service.get("test.remove", scope, None));

    service.remove("test.remove", scope);
    assert!(service.get("test.remove", scope, None).is_none_or(|v| v.is_empty()));
    let event = find(&events, "test.remove");
    assert_eq!(event.as_ref().map(|e| e.scope), Some(scope));
    assert_eq!(event.as_ref().map(|e| e.key.as_str()), Some("test.remove"));
}

#[test]
fn keys_in_memory() {
    let (guard, service) = setup();
    let disposables = guard.store();
    let scopes = [StorageScope::Workspace, StorageScope::Profile, StorageScope::Application];
    let targets = [StorageTarget::Machine, StorageTarget::User];

    let storage_target_event: Rc<RefCell<Option<StorageTargetChangeEvent>>> = Rc::default();
    let sink = storage_target_event.clone();
    service.on_did_change_target().listen_in(move |e| *sink.borrow_mut() = Some(*e), &disposables);

    // Empty
    for scope in scopes {
        for target in targets {
            assert_eq!(service.keys(scope, target).len(), 0);
        }
    }

    let storage_value_change_event: Rc<RefCell<Option<StorageValueChangeEvent>>> = Rc::default();

    // Add values
    for scope in scopes {
        let sink = storage_value_change_event.clone();
        service.on_did_change_value(scope, None, &disposables).listen_in(move |e| *sink.borrow_mut() = Some(e.clone()), &disposables);

        for target in targets {
            *storage_target_event.borrow_mut() = None;
            *storage_value_change_event.borrow_mut() = None;

            service.store("test.target1", s("value1"), scope, target);
            assert_eq!(service.keys(scope, target).len(), 1);
            assert_eq!(storage_target_event.borrow().map(|e| e.scope), Some(scope));
            let event = storage_value_change_event.borrow().clone();
            assert_eq!(event.as_ref().map(|e| e.key.as_str()), Some("test.target1"));
            assert_eq!(event.as_ref().map(|e| e.scope), Some(scope));
            assert_eq!(event.as_ref().and_then(|e| e.target), Some(target));

            *storage_target_event.borrow_mut() = None;
            *storage_value_change_event.borrow_mut() = None;

            service.store("test.target1", s("otherValue1"), scope, target);
            assert_eq!(service.keys(scope, target).len(), 1);
            assert_eq!(*storage_target_event.borrow(), None);
            let event = storage_value_change_event.borrow().clone();
            assert_eq!(event.as_ref().map(|e| e.key.as_str()), Some("test.target1"));
            assert_eq!(event.as_ref().map(|e| e.scope), Some(scope));
            assert_eq!(event.as_ref().and_then(|e| e.target), Some(target));

            service.store("test.target2", s("value2"), scope, target);
            service.store("test.target3", s("value3"), scope, target);

            assert_eq!(service.keys(scope, target).len(), 3);
        }
    }

    // Remove values
    for scope in scopes {
        for target in targets {
            let keys_length = service.keys(scope, target).len();

            service.store("test.target4", s("value1"), scope, target);
            assert_eq!(service.keys(scope, target).len(), keys_length + 1);

            *storage_target_event.borrow_mut() = None;
            *storage_value_change_event.borrow_mut() = None;

            service.remove("test.target4", scope);
            assert_eq!(service.keys(scope, target).len(), keys_length);
            assert_eq!(storage_target_event.borrow().map(|e| e.scope), Some(scope));
            let event = storage_value_change_event.borrow().clone();
            assert_eq!(event.as_ref().map(|e| e.key.as_str()), Some("test.target4"));
            assert_eq!(event.as_ref().map(|e| e.scope), Some(scope));
        }
    }

    // Remove all
    for scope in scopes {
        for target in targets {
            for key in service.keys(scope, target) {
                service.remove(&key, scope);
            }

            assert_eq!(service.keys(scope, target).len(), 0);
        }
    }

    // Adding undefined or null removes value
    for scope in scopes {
        for target in targets {
            service.store("test.target1", s("value1"), scope, target);
            assert_eq!(service.keys(scope, target).len(), 1);

            *storage_target_event.borrow_mut() = None;

            service.store("test.target1", StorageValue::Undefined, scope, target);
            assert_eq!(service.keys(scope, target).len(), 0);
            assert_eq!(storage_target_event.borrow().map(|e| e.scope), Some(scope));

            service.store("test.target1", s(""), scope, target);
            assert_eq!(service.keys(scope, target).len(), 1);

            service.store("test.target1", StorageValue::Null, scope, target);
            assert_eq!(service.keys(scope, target).len(), 0);
        }
    }

    // Target change
    for scope in scopes {
        *storage_target_event.borrow_mut() = None;
        service.store("test.target5", s("value1"), scope, StorageTarget::Machine);
        assert!(storage_target_event.borrow().is_some());
        *storage_target_event.borrow_mut() = None;
        service.store("test.target5", s("value1"), scope, StorageTarget::User);
        assert!(storage_target_event.borrow().is_some());
        *storage_target_event.borrow_mut() = None;
        service.store("test.target5", s("value1"), scope, StorageTarget::Machine);
        assert!(storage_target_event.borrow().is_some());
        *storage_target_event.borrow_mut() = None;
        service.store("test.target5", s("value1"), scope, StorageTarget::Machine);
        assert!(storage_target_event.borrow().is_none()); // no change in target
    }
}
