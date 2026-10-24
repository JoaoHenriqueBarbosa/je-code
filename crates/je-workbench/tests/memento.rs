//! Porte de `src/vs/workbench/test/common/memento.test.ts` (suíte `Memento`), caso a caso e na mesma ordem.
//!
//! Notas de tradução:
//! - `setup` (um `TestStorageService` novo e `Memento.clear` dos três escopos) roda antes de
//!   `ensureNoDisposablesAreLeakedInTestSuite()`, como no original, onde o `setup` da suíte é declarado antes dele e
//!   o que ele cria não é rastreado. O `disposables.add(...)` do original é `add_rc`, e o `teardown` é o `Drop` do
//!   guarda (que descarta o store).
//! - `TestStorageService` é o `InMemoryStorageService`; os mementos (`Memento<T>`) são `ObjectRef` (o objeto
//!   compartilhado que o chamador edita), `memento.foo = x` é `set` e `delete memento.foo` é `delete`.
//! - `assert.deepStrictEqual(memento, { ... })` compara o JSON do objeto, com as chaves ordenadas (a ordem de
//!   propriedades não conta no `deepStrictEqual`). `assert(memento)` (o objeto existe) não tem equivalente: um
//!   `ObjectRef` sempre existe.
//! - `JSON.parse(storage.get(...)!)` é `json_parse`, e `storage.get(key, scope, null!)` (que dá `null` sem o valor) é
//!   `get(key, scope, None)` igual a `None`.

#[path = "../../je-base/tests/common/mod.rs"]
mod common;

use std::rc::Rc;

use common::ensure_no_disposables_are_leaked_in_test_suite;
use je_base::JsString;
use je_base::dynamic::{Dynamic, ObjectRef};
use je_base::json::Value;
use je_base::jsonc::json_parse;
use je_platform::js::to_json;
use je_platform::storage::{AbstractStorageService, InMemoryStorageService, StorageScope, StorageService, StorageTarget};
use je_workbench::common::memento::Memento;

const MEMENTO_KEY: &str = "memento/memento.test";

/// O `setup`: um serviço de armazenamento novo e os mementos dos três escopos limpos.
fn setup() -> Rc<AbstractStorageService> {
    let storage = InMemoryStorageService::new();
    Memento::clear(StorageScope::Application);
    Memento::clear(StorageScope::Profile);
    Memento::clear(StorageScope::Workspace);
    storage
}

fn service(storage: &Rc<AbstractStorageService>) -> Rc<dyn StorageService> {
    Rc::new(storage.clone())
}

fn key(name: &str) -> JsString {
    JsString::from(name)
}

fn numbers(values: &[f64]) -> Dynamic {
    Dynamic::array(values.iter().map(|value| Dynamic::Number(*value)))
}

fn text(value: &str) -> Dynamic {
    Dynamic::string(value)
}

/// O texto canônico de um valor JSON, com as chaves dos objetos ordenadas.
fn canonical(value: &Value) -> String {
    match value {
        Value::Null => "null".to_string(),
        Value::Bool(flag) => flag.to_string(),
        Value::Number(number) => number.to_string(),
        Value::String(text) => format!("{:?}", text.to_string_lossy()),
        Value::Array(items) => format!("[{}]", items.iter().map(canonical).collect::<Vec<_>>().join(",")),
        Value::Object(object) => {
            let mut entries: Vec<(String, String)> = object.iter().map(|(name, item)| (name.to_string_lossy(), canonical(item))).collect();
            entries.sort();
            format!("{{{}}}", entries.iter().map(|(name, item)| format!("{name:?}:{item}")).collect::<Vec<_>>().join(","))
        }
    }
}

fn memento_json(memento: &ObjectRef) -> String {
    canonical(&to_json(&Dynamic::Object(memento.clone())).expect("o memento é um objeto"))
}

/// `assert.deepStrictEqual(memento, { ...expected })`.
fn assert_memento(memento: &ObjectRef, expected: &[(&str, Dynamic)]) {
    let expected = Dynamic::object(expected.iter().map(|(name, value)| (*name, value.clone())));
    assert_eq!(memento_json(memento), canonical(&to_json(&expected).expect("o esperado é um objeto")));
}

/// `assert.deepStrictEqual(a, b)` entre dois mementos.
fn assert_same_content(a: &ObjectRef, b: &ObjectRef) {
    assert_eq!(memento_json(a), memento_json(b));
}

/// `JSON.parse(storage.get('memento/memento.test', scope)!)`.
fn stored_json(storage: &Rc<AbstractStorageService>, scope: StorageScope) -> String {
    let stored = storage.get(MEMENTO_KEY, scope, None).expect("o memento deveria estar gravado");
    canonical(&json_parse(&stored.encode_utf16().collect::<Vec<u16>>()).expect("JSON válido"))
}

fn assert_stored(storage: &Rc<AbstractStorageService>, scope: StorageScope, expected: &[(&str, Dynamic)]) {
    let expected = Dynamic::object(expected.iter().map(|(name, value)| (*name, value.clone())));
    assert_eq!(stored_json(storage, scope), canonical(&to_json(&expected).expect("o esperado é um objeto")));
}

#[test]
fn loading_and_saving_memento_with_scopes() {
    let storage = setup();
    let disposables = ensure_no_disposables_are_leaked_in_test_suite();
    disposables.add_rc(storage.clone());
    let my_memento = Memento::new("memento.test", service(&storage));

    // Application
    let mut memento = my_memento.get_memento(StorageScope::Application, StorageTarget::Machine);
    memento.set(&key("foo"), numbers(&[1.0, 2.0, 3.0]));
    let mut application_memento = my_memento.get_memento(StorageScope::Application, StorageTarget::Machine);
    assert_same_content(&application_memento, &memento);

    // Profile
    memento = my_memento.get_memento(StorageScope::Profile, StorageTarget::Machine);
    memento.set(&key("foo"), numbers(&[4.0, 5.0, 6.0]));
    let mut profile_memento = my_memento.get_memento(StorageScope::Profile, StorageTarget::Machine);
    assert_same_content(&profile_memento, &memento);

    // Workspace
    memento = my_memento.get_memento(StorageScope::Workspace, StorageTarget::Machine);
    memento.set(&key("foo"), text("Hello World"));

    my_memento.save_memento();

    // Application
    memento = my_memento.get_memento(StorageScope::Application, StorageTarget::Machine);
    assert_memento(&memento, &[("foo", numbers(&[1.0, 2.0, 3.0]))]);
    application_memento = my_memento.get_memento(StorageScope::Application, StorageTarget::Machine);
    assert_same_content(&application_memento, &memento);

    // Profile
    memento = my_memento.get_memento(StorageScope::Profile, StorageTarget::Machine);
    assert_memento(&memento, &[("foo", numbers(&[4.0, 5.0, 6.0]))]);
    profile_memento = my_memento.get_memento(StorageScope::Profile, StorageTarget::Machine);
    assert_same_content(&profile_memento, &memento);

    // Workspace
    memento = my_memento.get_memento(StorageScope::Workspace, StorageTarget::Machine);
    assert_memento(&memento, &[("foo", text("Hello World"))]);

    // Assert the Mementos are stored properly in storage
    assert_stored(&storage, StorageScope::Application, &[("foo", numbers(&[1.0, 2.0, 3.0]))]);
    assert_stored(&storage, StorageScope::Profile, &[("foo", numbers(&[4.0, 5.0, 6.0]))]);
    assert_stored(&storage, StorageScope::Workspace, &[("foo", text("Hello World"))]);

    // Delete Application
    memento = my_memento.get_memento(StorageScope::Application, StorageTarget::Machine);
    memento.delete(&key("foo"));

    // Delete Profile
    memento = my_memento.get_memento(StorageScope::Profile, StorageTarget::Machine);
    memento.delete(&key("foo"));

    // Delete Workspace
    memento = my_memento.get_memento(StorageScope::Workspace, StorageTarget::Machine);
    memento.delete(&key("foo"));

    my_memento.save_memento();

    // Application
    memento = my_memento.get_memento(StorageScope::Application, StorageTarget::Machine);
    assert_memento(&memento, &[]);

    // Profile
    memento = my_memento.get_memento(StorageScope::Profile, StorageTarget::Machine);
    assert_memento(&memento, &[]);

    // Workspace
    memento = my_memento.get_memento(StorageScope::Workspace, StorageTarget::Machine);
    assert_memento(&memento, &[]);

    // Assert the Mementos are also removed from storage
    assert_eq!(storage.get(MEMENTO_KEY, StorageScope::Application, None), None);
    assert_eq!(storage.get(MEMENTO_KEY, StorageScope::Profile, None), None);
    assert_eq!(storage.get(MEMENTO_KEY, StorageScope::Workspace, None), None);
}

#[test]
fn save_and_load() {
    let storage = setup();
    let disposables = ensure_no_disposables_are_leaked_in_test_suite();
    disposables.add_rc(storage.clone());
    let my_memento = Memento::new("memento.test", service(&storage));

    // Profile
    let mut memento = my_memento.get_memento(StorageScope::Profile, StorageTarget::Machine);
    memento.set(&key("foo"), numbers(&[1.0, 2.0, 3.0]));

    // Workspace
    memento = my_memento.get_memento(StorageScope::Workspace, StorageTarget::Machine);
    memento.set(&key("foo"), text("Hello World"));

    my_memento.save_memento();

    // Profile
    memento = my_memento.get_memento(StorageScope::Profile, StorageTarget::Machine);
    assert_memento(&memento, &[("foo", numbers(&[1.0, 2.0, 3.0]))]);
    let mut profile_memento = my_memento.get_memento(StorageScope::Profile, StorageTarget::Machine);
    assert_same_content(&profile_memento, &memento);

    // Workspace
    memento = my_memento.get_memento(StorageScope::Workspace, StorageTarget::Machine);
    assert_memento(&memento, &[("foo", text("Hello World"))]);

    // Profile
    memento = my_memento.get_memento(StorageScope::Profile, StorageTarget::Machine);
    memento.set(&key("foo"), numbers(&[4.0, 5.0, 6.0]));

    // Workspace
    memento = my_memento.get_memento(StorageScope::Workspace, StorageTarget::Machine);
    memento.set(&key("foo"), text("World Hello"));

    my_memento.save_memento();

    // Profile
    memento = my_memento.get_memento(StorageScope::Profile, StorageTarget::Machine);
    assert_memento(&memento, &[("foo", numbers(&[4.0, 5.0, 6.0]))]);
    profile_memento = my_memento.get_memento(StorageScope::Profile, StorageTarget::Machine);
    assert_same_content(&profile_memento, &memento);

    // Workspace
    memento = my_memento.get_memento(StorageScope::Workspace, StorageTarget::Machine);
    assert_memento(&memento, &[("foo", text("World Hello"))]);

    // Delete Profile
    memento = my_memento.get_memento(StorageScope::Profile, StorageTarget::Machine);
    memento.delete(&key("foo"));

    // Delete Workspace
    memento = my_memento.get_memento(StorageScope::Workspace, StorageTarget::Machine);
    memento.delete(&key("foo"));

    my_memento.save_memento();

    // Profile
    memento = my_memento.get_memento(StorageScope::Profile, StorageTarget::Machine);
    assert_memento(&memento, &[]);

    // Workspace
    memento = my_memento.get_memento(StorageScope::Workspace, StorageTarget::Machine);
    assert_memento(&memento, &[]);
}

#[test]
fn save_and_load_2_components_with_same_id() {
    let storage = setup();
    let disposables = ensure_no_disposables_are_leaked_in_test_suite();
    disposables.add_rc(storage.clone());
    let my_memento = Memento::new("memento.test", service(&storage));
    let my_memento2 = Memento::new("memento.test", service(&storage));

    // Profile
    let mut memento = my_memento.get_memento(StorageScope::Profile, StorageTarget::Machine);
    memento.set(&key("foo"), numbers(&[1.0, 2.0, 3.0]));

    memento = my_memento2.get_memento(StorageScope::Profile, StorageTarget::Machine);
    memento.set(&key("bar"), numbers(&[1.0, 2.0, 3.0]));

    // Workspace
    memento = my_memento.get_memento(StorageScope::Workspace, StorageTarget::Machine);
    memento.set(&key("foo"), text("Hello World"));

    memento = my_memento2.get_memento(StorageScope::Workspace, StorageTarget::Machine);
    memento.set(&key("bar"), text("Hello World"));

    my_memento.save_memento();
    my_memento2.save_memento();

    // Profile
    memento = my_memento.get_memento(StorageScope::Profile, StorageTarget::Machine);
    assert_memento(&memento, &[("foo", numbers(&[1.0, 2.0, 3.0])), ("bar", numbers(&[1.0, 2.0, 3.0]))]);
    let mut profile_memento = my_memento.get_memento(StorageScope::Profile, StorageTarget::Machine);
    assert_same_content(&profile_memento, &memento);

    memento = my_memento2.get_memento(StorageScope::Profile, StorageTarget::Machine);
    assert_memento(&memento, &[("foo", numbers(&[1.0, 2.0, 3.0])), ("bar", numbers(&[1.0, 2.0, 3.0]))]);
    profile_memento = my_memento2.get_memento(StorageScope::Profile, StorageTarget::Machine);
    assert_same_content(&profile_memento, &memento);

    // Workspace
    memento = my_memento.get_memento(StorageScope::Workspace, StorageTarget::Machine);
    assert_memento(&memento, &[("foo", text("Hello World")), ("bar", text("Hello World"))]);

    memento = my_memento2.get_memento(StorageScope::Workspace, StorageTarget::Machine);
    assert_memento(&memento, &[("foo", text("Hello World")), ("bar", text("Hello World"))]);
}

#[test]
fn clear_memento() {
    let storage = setup();
    let disposables = ensure_no_disposables_are_leaked_in_test_suite();
    disposables.add_rc(storage.clone());
    let mut my_memento = Memento::new("memento.test", service(&storage));

    // Profile
    let mut profile_memento = my_memento.get_memento(StorageScope::Profile, StorageTarget::Machine);
    profile_memento.set(&key("foo"), text("Hello World"));

    // Workspace
    let mut workspace_memento = my_memento.get_memento(StorageScope::Workspace, StorageTarget::Machine);
    workspace_memento.set(&key("bar"), text("Hello World"));

    my_memento.save_memento();

    // Clear
    let storage = disposables.add_rc(InMemoryStorageService::new());
    Memento::clear(StorageScope::Profile);
    Memento::clear(StorageScope::Workspace);

    my_memento = Memento::new("memento.test", service(&storage));
    profile_memento = my_memento.get_memento(StorageScope::Profile, StorageTarget::Machine);
    workspace_memento = my_memento.get_memento(StorageScope::Workspace, StorageTarget::Machine);

    assert_memento(&profile_memento, &[]);
    assert_memento(&workspace_memento, &[]);
}
