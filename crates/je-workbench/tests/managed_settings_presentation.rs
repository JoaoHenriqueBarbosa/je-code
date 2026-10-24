//! Porte de `src/vs/workbench/services/configuration/test/common/managedSettingsPresentation.test.ts` (suíte `ManagedSettingsPresentationService`), caso a caso e na mesma ordem.
//!
//! Notas de tradução:
//! - `ensureNoDisposablesAreLeakedInTestSuite()` vira o [`LeakGuard`] criado no começo de cada caso.
//! - `mock<IManagedSettingsService>()` com `onDidChangeManagedSettings` e `getManagedSettingValue` é o
//!   [`StubManagedSettingsService`]; o método que o original não sobrescreve falha com `panic!` (no original seria um
//!   `TypeError` por chamar `undefined`).
//! - `Record<string, ManagedSettingValue | undefined>` é um `HashMap<String, PolicyValue>` compartilhado entre o teste e
//!   o dublê (a chave ausente é o `undefined`); `values.x = v` é [`Setup::set`] e `delete values.x` é [`Setup::delete`].
//! - A função `managedSettingsPresentation` do esquema é uma função dinâmica nativa ([`presentation`]) que recebe
//!   `(read, localValue)`; `read('x')` é [`read_managed_setting`].
//! - `registry.registerConfiguration(node)` e o `toDisposable` que o desregistra são [`register`].
//! - `assert.deepStrictEqual` sobre objetos e listas é a comparação de [`Dynamic`] (`undefined` dentro de lista é
//!   [`Dynamic::Undefined`]).

#[path = "../../je-platform/tests/common/mod.rs"]
mod common;

use std::cell::RefCell;
use std::collections::{HashMap, HashSet};
use std::rc::Rc;

use common::{LeakGuard, arr, assert_deep_strict_equal, ensure_no_disposables_are_leaked_in_test_suite, n, node, obj, prop_with, s};
use je_base::dynamic::{Dynamic, FunctionRef};
use je_base::event::{Emitter, Event};
use je_base::lifecycle::{DisposableRef, IDisposable, to_disposable};
use je_base::policy::{ManagedSettingValue, ManagedSettingsData, PolicyValue};
use je_platform::configuration::registry::{PropertySchema, configuration_registry};
use je_platform::policy::copilot_managed_settings::IManagedSettingsService;
use je_workbench::services::configuration::common::managed_settings_presentation::{IManagedSettingsPresentationService, ManagedSettingsPresentationService, read_managed_setting};

type Values = Rc<RefCell<HashMap<String, PolicyValue>>>;

/// `new class extends mock<IManagedSettingsService>() { ... }`.
struct StubManagedSettingsService {
    changed: Event<()>,
    values: Values,
}

impl IManagedSettingsService for StubManagedSettingsService {
    fn on_did_change_managed_settings(&self) -> Event<()> {
        self.changed.clone()
    }

    fn get_managed_settings(&self) -> ManagedSettingsData {
        panic!("getManagedSettings não é sobrescrito pelo dublê")
    }

    fn get_managed_setting_value(&self, key: &str) -> Option<ManagedSettingValue> {
        self.values.borrow().get(key).cloned()
    }
}

/// `registerConfiguration({ id: 'managedPresentationTest', properties })` e o `toDisposable` que o desregistra.
fn register(guard: &LeakGuard, properties: Vec<(&str, PropertySchema)>) -> DisposableRef {
    let configuration = node("managedPresentationTest", properties);
    configuration_registry().register_configuration(configuration.clone());
    guard.add(to_disposable(move || configuration_registry().deregister_configurations(std::slice::from_ref(&configuration))))
}

/// A função `managedSettingsPresentation: (read, localValue) => ...` do esquema.
fn presentation(body: impl Fn(&Dynamic, &Dynamic) -> Dynamic + 'static) -> Dynamic {
    Dynamic::Function(FunctionRef::native("managedSettingsPresentation", 2, move |args| {
        body(args.first().unwrap_or(&Dynamic::Undefined), args.get(1).unwrap_or(&Dynamic::Undefined))
    }))
}

/// Uma propriedade `{ type, default?, managedSettingsPresentation }`.
fn managed_property(literal: &str, body: impl Fn(&Dynamic, &Dynamic) -> Dynamic + 'static) -> PropertySchema {
    prop_with(literal, vec![("managedSettingsPresentation", presentation(body))])
}

/// `read('key') === true`.
fn is_true(read: &Dynamic, key: &str) -> bool {
    read_managed_setting(read, key).strict_equals(&Dynamic::Bool(true))
}

struct Setup {
    service: Rc<ManagedSettingsPresentationService>,
    values: Values,
    changed: Rc<Emitter<()>>,
    events: Rc<RefCell<Vec<Vec<String>>>>,
}

impl Setup {
    /// `values.key = value`.
    fn set(&self, key: &str, value: PolicyValue) {
        self.values.borrow_mut().insert(key.to_owned(), value);
    }

    /// `delete values.key`.
    fn delete(&self, key: &str) {
        self.values.borrow_mut().remove(key);
    }

    fn fire_changed(&self) {
        self.changed.fire(());
    }

    /// `service.getValue(key)`, com `undefined` para a ausência.
    fn value(&self, key: &str) -> Dynamic {
        self.service.get_value(key, None).unwrap_or(Dynamic::Undefined)
    }

    /// `service.getValue(key, localValue)`.
    fn value_with_local(&self, key: &str, local_value: &Dynamic) -> Dynamic {
        self.service.get_value(key, Some(local_value)).unwrap_or(Dynamic::Undefined)
    }

    /// Os eventos recebidos, como lista de listas de textos.
    fn events_value(&self) -> Dynamic {
        arr(self.events.borrow().iter().map(|keys| arr(keys.iter().map(|key| s(key)).collect())).collect())
    }
}

/// `setup(values)`.
fn setup(guard: &LeakGuard, initial: Vec<(&str, PolicyValue)>) -> Setup {
    let changed = guard.add_rc(Emitter::<()>::new());
    let values: Values = Rc::new(RefCell::new(initial.into_iter().map(|(key, value)| (key.to_owned(), value)).collect()));
    let service = guard.add_rc(ManagedSettingsPresentationService::new(Rc::new(StubManagedSettingsService { changed: changed.event(), values: values.clone() })));
    let events: Rc<RefCell<Vec<Vec<String>>>> = Rc::new(RefCell::new(Vec::new()));
    guard.add(service.on_did_change().listen({
        let events = events.clone();
        move |keys: &Rc<HashSet<String>>| {
            let mut sorted: Vec<String> = keys.iter().cloned().collect();
            sorted.sort();
            events.borrow_mut().push(sorted);
        }
    }));
    Setup { service, values, changed, events }
}

/// `registry.getConfigurationProperties()[key].default`.
fn registered_default(key: &str) -> Dynamic {
    configuration_registry().get_configuration_properties().get(key).expect("propriedade registrada").get("default")
}

fn texts(items: &[&str]) -> Dynamic {
    arr(items.iter().map(|item| s(item)).collect())
}

#[test]
fn projects_only_declared_restrictions_including_false_zero_and_empty_strings() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    register(
        &guard,
        vec![
            ("test.managedBoolean", managed_property("{ 'type': 'boolean', 'default': true }", |read, _| read_managed_setting(read, "boolean"))),
            ("test.managedNumber", managed_property("{ 'type': 'number', 'default': 1 }", |read, _| read_managed_setting(read, "number"))),
            ("test.managedString", managed_property("{ 'type': 'string', 'default': 'default' }", |read, _| read_managed_setting(read, "string"))),
            ("test.unmanaged", prop_with("{ 'type': 'boolean', 'default': true }", vec![])),
        ],
    );
    let t = setup(
        &guard,
        vec![
            ("boolean", PolicyValue::Boolean(false)),
            ("number", PolicyValue::Number(0.0)),
            ("string", PolicyValue::String(String::new())),
            ("test.unmanaged", PolicyValue::Boolean(false)),
        ],
    );
    assert_deep_strict_equal(
        &obj(vec![
            (
                "values",
                arr(["test.managedBoolean", "test.managedNumber", "test.managedString", "test.unmanaged", "test.missing"].iter().map(|key| t.value(key)).collect()),
            ),
            ("default", registered_default("test.managedBoolean")),
        ]),
        &obj(vec![("values", arr(vec![Dynamic::Bool(false), n(0.0), s(""), Dynamic::Undefined, Dynamic::Undefined])), ("default", Dynamic::Bool(true))]),
    );
}

#[test]
fn notifies_only_affected_settings_and_clears_restrictions_without_changing_defaults() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    register(
        &guard,
        vec![
            (
                "test.managedFirst",
                managed_property("{ 'type': 'boolean', 'default': true }", |read, _| if is_true(read, "deny") { Dynamic::Bool(false) } else { Dynamic::Undefined }),
            ),
            (
                "test.managedSecond",
                managed_property("{ 'type': 'boolean', 'default': true }", |read, _| {
                    if is_true(read, "deny") && !is_true(read, "exception") { Dynamic::Bool(false) } else { Dynamic::Undefined }
                }),
            ),
        ],
    );
    let t = setup(&guard, vec![]);
    t.set("unrelated", PolicyValue::Boolean(true));
    t.fire_changed();
    t.set("deny", PolicyValue::Boolean(true));
    t.fire_changed();
    t.fire_changed();
    t.set("exception", PolicyValue::Boolean(true));
    t.fire_changed();
    t.delete("deny");
    t.fire_changed();
    t.set("deny", PolicyValue::Boolean(false));
    t.fire_changed();
    assert_deep_strict_equal(
        &obj(vec![("events", t.events_value()), ("values", arr(["test.managedFirst", "test.managedSecond"].iter().map(|key| t.value(key)).collect()))]),
        &obj(vec![
            ("events", arr(vec![texts(&["test.managedFirst", "test.managedSecond"]), texts(&["test.managedSecond"]), texts(&["test.managedFirst"])])),
            ("values", arr(vec![Dynamic::Undefined, Dynamic::Undefined])),
        ]),
    );
}

#[test]
fn observes_configuration_registration_and_removal() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let t = setup(&guard, vec![("enforced", PolicyValue::Boolean(true))]);
    let registration = register(
        &guard,
        vec![("test.managedDynamic", managed_property("{ 'type': 'boolean' }", |read, _| read_managed_setting(read, "enforced")))],
    );
    let registered = t.value("test.managedDynamic");
    registration.dispose();
    assert_deep_strict_equal(
        &obj(vec![("registered", registered), ("removed", t.value("test.managedDynamic")), ("events", t.events_value())]),
        &obj(vec![
            ("registered", Dynamic::Bool(true)),
            ("removed", Dynamic::Undefined),
            ("events", arr(vec![texts(&["test.managedDynamic"]), texts(&["test.managedDynamic"])])),
        ]),
    );
}

#[test]
fn compares_array_values_structurally_and_notifies_on_changes_empty_lists_and_removal() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    register(
        &guard,
        vec![(
            "test.managedArray",
            managed_property("{ 'type': 'array', 'items': { 'type': 'string' }, 'default': [] }", |read, _| match read_managed_setting(read, "hosts") {
                Dynamic::String(hosts) => {
                    let hosts = hosts.to_string_lossy();
                    if hosts.is_empty() { arr(vec![]) } else { arr(hosts.split(',').map(s).collect()) }
                }
                _ => Dynamic::Undefined,
            }),
        )],
    );
    let t = setup(&guard, vec![]);
    t.set("hosts", PolicyValue::String("managed.example".to_owned()));
    t.fire_changed();
    t.fire_changed();
    t.set("unrelated", PolicyValue::Boolean(true));
    t.fire_changed();
    t.set("hosts", PolicyValue::String("updated.example".to_owned()));
    t.fire_changed();
    let updated = t.value("test.managedArray");
    t.set("hosts", PolicyValue::String(String::new()));
    t.fire_changed();
    let empty = t.value("test.managedArray");
    t.delete("hosts");
    t.fire_changed();
    assert_deep_strict_equal(
        &obj(vec![("updated", updated), ("empty", empty), ("removed", t.value("test.managedArray")), ("events", t.events_value())]),
        &obj(vec![
            ("updated", texts(&["updated.example"])),
            ("empty", texts(&[])),
            ("removed", Dynamic::Undefined),
            ("events", arr((0..4).map(|_| texts(&["test.managedArray"])).collect())),
        ]),
    );
}

#[test]
fn stops_notifications_when_disposed() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    register(&guard, vec![("test.managedDisposable", managed_property("{ 'type': 'boolean' }", |read, _| read_managed_setting(read, "value")))]);
    let t = setup(&guard, vec![]);
    t.service.dispose();
    t.set("value", PolicyValue::Boolean(true));
    t.fire_changed();
    assert_deep_strict_equal(&t.events_value(), &arr(vec![]));
}

#[test]
fn passes_the_inspected_local_value_to_presentation_callbacks_without_persisting_it() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    register(
        &guard,
        vec![(
            "test.managedLocal",
            managed_property("{ 'type': 'string', 'default': 'default' }", |read, local_value| {
                if is_true(read, "restrict") && matches!(local_value, Dynamic::String(_)) { local_value.clone() } else { Dynamic::Undefined }
            }),
        )],
    );
    let t = setup(&guard, vec![("restrict", PolicyValue::Boolean(true))]);
    assert_deep_strict_equal(
        &obj(vec![
            ("first", t.value_with_local("test.managedLocal", &s("first"))),
            ("second", t.value_with_local("test.managedLocal", &s("second"))),
            ("withoutLocal", t.value("test.managedLocal")),
            ("default", registered_default("test.managedLocal")),
        ]),
        &obj(vec![("first", s("first")), ("second", s("second")), ("withoutLocal", Dynamic::Undefined), ("default", s("default"))]),
    );
}
