//! Porte de `src/vs/workbench/services/storage/test/browser/storageService.test.ts` (suítes `StorageService (browser)`,
//! `StorageService (browser specific)` e `IndexDBStorageDatabase (browser)`), caso a caso e na mesma ordem.
//!
//! Notas de tradução:
//! - **O navegador.** O original roda sobre o IndexedDB e o `BroadcastChannel` reais do navegador de teste. Aqui o ambiente
//!   hospedeiro é o [`TestBrowserHost`]: um [`BrowserStorageHost`] com bancos nomeados que vivem na thread do teste (reabrir
//!   o mesmo nome vê os dados gravados, como o IndexedDB), operações atômicas por conexão (a transação de leitura e escrita do
//!   `compareAndSwap` roda inteira), erro de banco fechado depois do `close` e canais de broadcast que entregam aos outros
//!   canais do mesmo nome (nunca ao remetente). O `isSafari` é falso e o documento tem foco.
//! - **`createStorageService`.** O `FileService` e o `InMemoryFileSystemProvider` que o original cria e registra no
//!   `disposables` não são entregues a nada (o `BrowserStorageService` não usa o serviço de arquivos), então não existem aqui.
//!   O `UserDataProfileService` é o real, com o perfil `inMemory`; o `NullLogService` é o `NullLogger`.
//! - **`createSuite<BrowserStorageService>`** (de `platform/storage/test/common/storageService.test.ts`) é o conjunto de
//!   funções `store_data`, `remove_data` e as demais, que recebem o serviço (`AbstractStorageService`) do
//!   `BrowserStorageService`, como no teste do `InMemoryStorageService` do `je-platform`. O `setup` é `createStorageService`
//!   mais a criação do `disposables` da suíte, o `teardown` é `disposables.clear()` seguido de `await clear()` e do
//!   `disposables.clear()` externo, nessa ordem. Os valores são os do `je-storage` (`StorageValue`, e `undefined` é
//!   `StorageValue::Undefined`); o argumento `fallbackValue` ausente é `None`.
//! - **`flakySuite`** é só o `suite` com tentativas repetidas no mocha: o caso roda uma vez.
//! - **`ensureNoDisposablesAreLeakedInTestSuite()`** é o `LeakGuard`, criado depois do `disposables` da suíte (o que o original
//!   cria na definição da suíte, antes do rastreador, não é rastreado) e solto por último.
//! - **`test.skip('clear', ...)`** é `#[ignore]`, com o corpo do original.
//! - **Os literais `{ swapped, currentValue }`** são o [`CompareAndSwapResult`]; as comparações `deepStrictEqual` de objetos
//!   literais são `assert_eq!` de tuplas com os mesmos campos, na mesma ordem.
//! - **Relógio.** O original não usa timers falsos (o `runWithFakedTimers` só existe no caso ignorado): os casos rodam em
//!   `run_with_real_timers`.

#[path = "../../je-base/tests/common/mod.rs"]
mod common;
mod support;

use std::cell::{Cell, RefCell};
use std::collections::{BTreeMap, HashMap};
use std::rc::Rc;

use common::{ensure_no_disposables_are_leaked_in_test_suite, run_with_real_timers};
use je_base::event::{Emitter, Event};
use je_base::json::{parse_str, Value};
use je_base::lifecycle::{Disposable, DisposableStore, IDisposable};
use je_base::map::OrderedMap;
use je_base::promise::Promise;
use je_base::runtime;
use je_base::uri::{URI, UriComponents};
use je_instantiation::Injectable;
use je_platform::log::{LogServiceRef, NullLogger};
use je_platform::storage::{
    AbstractStorageService, CompareAndSwapResult, StorageEntry, StorageScope, StorageService, StorageTarget, StorageTargetChangeEvent, StorageValueChangeEvent, StorageWorkspace,
};
use je_platform::user_data_profile::user_data_profile::UserDataProfile;
use je_storage::{IStorage, Storage, StorageChangeEvent, StorageDatabase, StorageItemsChangeEvent, StorageOptions, StorageValue};
use je_workbench::services::storage::browser_storage_service::{
    db_closed_error, BrowserStorageHost, BrowserStorageService, IIndexedDbStorageDatabase, IndexedDb, IndexedDbStorageDatabase, IndexedDbStorageDatabaseOptions,
    StorageBroadcastChannel,
};
use je_workbench::services::user_data_profile::user_data_profile::IUserDataProfileService;
use je_workbench::services::user_data_profile::user_data_profile_service::UserDataProfileService;

//#region O navegador de teste

type Table = Rc<RefCell<BTreeMap<String, String>>>;

type ChannelEntry = (usize, String, Rc<Emitter<StorageItemsChangeEvent>>);

thread_local! {
    /// Os bancos por nome (o `indexedDB` do navegador).
    static DATABASES: RefCell<HashMap<String, Table>> = RefCell::new(HashMap::new());
    /// Os canais de broadcast abertos (`BroadcastChannel`).
    static CHANNELS: RefCell<Vec<ChannelEntry>> = const { RefCell::new(Vec::new()) };
    static NEXT_CHANNEL_ID: Cell<usize> = const { Cell::new(0) };
}

fn table_of(name: &str) -> Table {
    DATABASES.with(|databases| databases.borrow_mut().entry(name.to_string()).or_default().clone())
}

/// `IndexedDB`: uma conexão com um banco nomeado.
struct TestIndexedDb {
    name: String,
    table: Table,
    closed: Cell<bool>,
}

impl TestIndexedDb {
    /// Roda a transação, ou rejeita com o erro de banco fechado.
    fn transaction<T: Clone + 'static>(&self, run: impl FnOnce(&mut BTreeMap<String, String>) -> T) -> Promise<T> {
        if self.closed.get() {
            return Promise::rejected(db_closed_error(&self.name));
        }
        Promise::resolved(run(&mut *self.table.borrow_mut()))
    }
}

impl IndexedDb for TestIndexedDb {
    fn close(&self) {
        self.closed.set(true);
    }

    fn get_key_values(&self, _store: &str) -> Promise<OrderedMap<String, String>> {
        self.transaction(|table| {
            let mut items = OrderedMap::new();
            for (key, value) in table.iter() {
                items.insert(key.clone(), value.clone());
            }
            items
        })
    }

    fn get_value(&self, _store: &str, key: &str) -> Promise<Option<String>> {
        let key = key.to_string();
        self.transaction(move |table| table.get(&key).cloned())
    }

    fn update_items(&self, _store: &str, inserts: Vec<(String, String)>, deletes: Vec<String>) -> Promise<()> {
        self.transaction(move |table| {
            for (key, value) in inserts {
                table.insert(key, value);
            }
            for key in deletes {
                table.remove(&key);
            }
        })
    }

    fn clear(&self, _store: &str) -> Promise<()> {
        self.transaction(|table| table.clear())
    }

    fn compare_and_swap(&self, _store: &str, key: &str, expected_value: Option<&str>, new_value: &str) -> Promise<CompareAndSwapResult> {
        let (key, expected_value, new_value) = (key.to_string(), expected_value.map(str::to_string), new_value.to_string());
        self.transaction(move |table| {
            let current_value = table.get(&key).cloned();
            if current_value == expected_value {
                table.insert(key, new_value.clone());
                CompareAndSwapResult { swapped: true, current_value: Some(new_value) }
            } else {
                CompareAndSwapResult { swapped: false, current_value }
            }
        })
    }
}

/// `BroadcastDataChannel`: entrega aos outros canais do mesmo nome, depois de uma microtarefa (a mensagem do
/// `BroadcastChannel` é assíncrona).
struct TestBroadcastChannel {
    id: usize,
    name: String,
    base: Rc<Disposable>,
    emitter: Rc<Emitter<StorageItemsChangeEvent>>,
}

impl StorageBroadcastChannel for TestBroadcastChannel {
    fn on_did_receive_data(&self) -> Event<StorageItemsChangeEvent> {
        self.emitter.event()
    }

    fn post_data(&self, data: StorageItemsChangeEvent) {
        let receivers: Vec<Rc<Emitter<StorageItemsChangeEvent>>> = CHANNELS.with(|channels| {
            channels.borrow().iter().filter(|(id, name, _)| *id != self.id && *name == self.name).map(|(_, _, emitter)| emitter.clone()).collect()
        });
        for receiver in receivers {
            let data = data.clone();
            runtime::queue_microtask(move || receiver.fire(data));
        }
    }
}

impl IDisposable for TestBroadcastChannel {
    fn dispose(&self) {
        CHANNELS.with(|channels| channels.borrow_mut().retain(|(id, _, _)| *id != self.id));
        self.base.dispose();
    }
}

struct TestBrowserHost;

impl BrowserStorageHost for TestBrowserHost {
    fn create_indexed_db(&self, name: &str, _version: Option<u32>, _stores: &[&str]) -> Promise<Rc<dyn IndexedDb>> {
        let database: Rc<dyn IndexedDb> = Rc::new(TestIndexedDb { name: name.to_string(), table: table_of(name), closed: Cell::new(false) });
        Promise::resolved(database)
    }

    fn create_broadcast_channel(&self, name: &str) -> Rc<dyn StorageBroadcastChannel> {
        let id = NEXT_CHANNEL_ID.with(|next| next.replace(next.get() + 1));
        let base = Disposable::new();
        let emitter = Emitter::<StorageItemsChangeEvent>::new();
        base.register(emitter.clone());
        CHANNELS.with(|channels| channels.borrow_mut().push((id, name.to_string(), emitter.clone())));
        Rc::new(TestBroadcastChannel { id, name: name.to_string(), base, emitter })
    }

    fn is_safari(&self) -> bool {
        false
    }

    fn has_focus(&self) -> bool {
        true
    }
}

fn host() -> Rc<dyn BrowserStorageHost> {
    Rc::new(TestBrowserHost)
}

fn log_service() -> LogServiceRef {
    Rc::new(NullLogger::new())
}

//#endregion

/// Um URI do esquema `inmemory` com o caminho dado (`URI.file('/profiles').with({ scheme: Schemas.inMemory })` e `joinPath`).
fn in_memory_uri(path: &str) -> URI {
    URI::from(UriComponents { scheme: Some("inmemory".into()), path: Some(path.into()), ..Default::default() })
}

/// `createStorageService`.
async fn create_storage_service() -> (Rc<DisposableStore>, Rc<BrowserStorageService>) {
    let disposables = DisposableStore::new();

    let extra = "/profiles/extra";
    let in_memory_extra_profile = UserDataProfile {
        id: "id".to_string(),
        name: "inMemory".to_string(),
        is_default: false,
        icon: None,
        location: in_memory_uri(extra),
        global_storage_home: in_memory_uri(&format!("{extra}/globalStorageHome")),
        settings_resource: in_memory_uri(&format!("{extra}/settingsResource")),
        keybindings_resource: in_memory_uri(&format!("{extra}/keybindingsResource")),
        tasks_resource: in_memory_uri(&format!("{extra}/tasksResource")),
        mcp_resource: in_memory_uri(&format!("{extra}/mcp.json")),
        language_models_resource: in_memory_uri(&format!("{extra}/chatLanguageModels.json")),
        snippets_home: in_memory_uri(&format!("{extra}/snippetsHome")),
        prompts_home: in_memory_uri(&format!("{extra}/promptsHome")),
        extensions_resource: in_memory_uri(&format!("{extra}/extensionsResource")),
        cache_home: in_memory_uri(&format!("{extra}/cache")),
        agent_plugins_home: in_memory_uri(&format!("{extra}/agentPluginsHome")),
        use_default_flags: None,
        is_internal: None,
        is_transient: None,
        is_agents_window_profile: None,
        workspaces: None,
    };

    let user_data_profile_service = disposables.add_rc(UserDataProfileService::create(in_memory_extra_profile, ()));
    let user_data_profile_service: Rc<dyn IUserDataProfileService> = user_data_profile_service;
    let storage_service = disposables.add_rc(BrowserStorageService::new(StorageWorkspace { id: "workspace-storage-test".to_string() }, user_data_profile_service, log_service(), host()));

    storage_service.initialize().await.unwrap();

    (disposables, storage_service)
}

//#region createSuite (platform/storage/test/common/storageService.test.ts)

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

/// O `setup` e o `teardown` de `createSuite` com os de `StorageService (browser)`: o `disposables` da suíte, o serviço, o corpo
/// do caso, e depois `disposables.clear()`, `await storageService.clear()` e o `disposables.clear()` externo.
fn run_suite(body: impl FnOnce(Rc<AbstractStorageService>, Rc<DisposableStore>) + 'static) {
    run_with_real_timers(async move {
        let outer_disposables = DisposableStore::new();
        let disposables = DisposableStore::new();
        let _guard = ensure_no_disposables_are_leaked_in_test_suite();

        let (service_disposables, storage_service) = create_storage_service().await;
        outer_disposables.add(service_disposables);
        body(storage_service.service(), disposables.clone());

        disposables.clear();
        storage_service.clear().await.unwrap();
        outer_disposables.clear();
    });
}

#[test]
fn get_data_integer_boolean_application() {
    run_suite(|service, disposables| store_data(&service, StorageScope::Application, &disposables));
}

#[test]
fn get_data_integer_boolean_profile() {
    run_suite(|service, disposables| store_data(&service, StorageScope::Profile, &disposables));
}

#[test]
fn get_data_integer_boolean_object_workspace() {
    run_suite(|service, disposables| store_data(&service, StorageScope::Workspace, &disposables));
}

#[test]
fn storage_change_source() {
    run_suite(|service, disposables| {
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
    });
}

#[test]
fn storage_change_event_scope_all_keys() {
    run_suite(|service, disposables| {
        let events = collect_events(&service, StorageScope::Workspace, None, &disposables);

        service.store("testChange", s("foobar"), StorageScope::Workspace, StorageTarget::Machine);
        service.store("testChange2", s("foobar"), StorageScope::Workspace, StorageTarget::Machine);
        service.store("testChange", s("foobar"), StorageScope::Application, StorageTarget::Machine);
        service.store("testChange", s("foobar"), StorageScope::Profile, StorageTarget::Machine);
        service.store("testChange2", s("foobar"), StorageScope::Profile, StorageTarget::Machine);
        assert_eq!(events.borrow().len(), 2);
    });
}

#[test]
fn storage_change_event_scope_specific_key() {
    run_suite(|service, disposables| {
        let events = collect_events(&service, StorageScope::Workspace, Some("testChange"), &disposables);

        service.store("testChange", s("foobar"), StorageScope::Workspace, StorageTarget::Machine);
        service.store("testChange", s("foobar"), StorageScope::Profile, StorageTarget::User);
        service.store("testChange", s("foobar"), StorageScope::Application, StorageTarget::Machine);
        service.store("testChange2", s("foobar"), StorageScope::Workspace, StorageTarget::Machine);
        assert!(find(&events, "testChange").is_some());
        assert_eq!(events.borrow().len(), 1);
    });
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
    run_suite(|service, disposables| remove_data(&service, StorageScope::Application, &disposables));
}

#[test]
fn remove_data_profile() {
    run_suite(|service, disposables| remove_data(&service, StorageScope::Profile, &disposables));
}

#[test]
fn remove_data_workspace() {
    run_suite(|service, disposables| remove_data(&service, StorageScope::Workspace, &disposables));
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
fn keys_browser() {
    run_suite(|service, disposables| {
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
    });
}

//#endregion

//#region StorageService (browser specific)

/// `setup` e `teardown` de `StorageService (browser specific)`: o serviço, o corpo e `await clear()` seguido de
/// `disposables.clear()`.
fn run_browser_specific<F, Fut>(body: F)
where
    F: FnOnce(Rc<BrowserStorageService>) -> Fut + 'static,
    Fut: std::future::Future<Output = ()> + 'static,
{
    run_with_real_timers(async move {
        let disposables = DisposableStore::new();
        let _guard = ensure_no_disposables_are_leaked_in_test_suite();

        let (service_disposables, storage_service) = create_storage_service().await;
        disposables.add(service_disposables);

        body(storage_service.clone()).await;

        storage_service.clear().await.unwrap();
        disposables.clear();
    });
}

#[test]
#[ignore = "slow test and also only ever being used as a developer action"]
fn clear() {
    run_browser_specific(|storage_service| async move {
        let service = storage_service.service();
        service.store("bar", s("foo"), StorageScope::Application, StorageTarget::Machine);
        service.store("bar", StorageValue::Number(3.0), StorageScope::Application, StorageTarget::User);
        service.store("bar", s("foo"), StorageScope::Profile, StorageTarget::Machine);
        service.store("bar", StorageValue::Number(3.0), StorageScope::Profile, StorageTarget::User);
        service.store("bar", s("foo"), StorageScope::Workspace, StorageTarget::Machine);
        service.store("bar", StorageValue::Number(3.0), StorageScope::Workspace, StorageTarget::User);

        storage_service.clear().await.unwrap();

        for scope in [StorageScope::Application, StorageScope::Profile, StorageScope::Workspace] {
            for target in [StorageTarget::User, StorageTarget::Machine] {
                assert_eq!(service.get("bar", scope, None), None);
                assert_eq!(service.keys(scope, target).len(), 0);
            }
        }
    });
}

#[test]
fn application_database_access_shares_storage_state_and_fallback() {
    run_browser_specific(|storage_service| async move {
        let service = storage_service.service();
        service.store("key", s("first"), StorageScope::Application, StorageTarget::Machine);
        storage_service.flush().await.unwrap();
        let before = storage_service.get_application_storage_value("key").await.unwrap();
        let result = storage_service.compare_and_swap_application_storage("key", Some("first"), "second").await.unwrap();

        assert_eq!(
            (before, result, service.get("key", StorageScope::Application, None)),
            (Some("first".to_owned()), CompareAndSwapResult { swapped: true, current_value: Some("second".to_owned()) }, Some("second".to_owned())),
        );
    });
}

#[test]
fn application_shared_compare_and_swap_stays_machine_local_and_publishes_the_new_value() {
    run_browser_specific(|storage_service| async move {
        let service = storage_service.service();
        let result = service.compare_and_swap_application_shared_value("shared-claim", None, "owner").await.unwrap();
        let rejected = service.compare_and_swap_application_shared_value("shared-claim", None, "other").await.unwrap();
        let value = service.read_application_shared_value("shared-claim").await.unwrap();
        let cached = service.get("shared-claim", StorageScope::ApplicationShared, None);
        let machine = service.keys(StorageScope::ApplicationShared, StorageTarget::Machine).contains(&"shared-claim".to_owned());
        let synced = service.keys(StorageScope::ApplicationShared, StorageTarget::User).contains(&"shared-claim".to_owned());

        assert_eq!(
            (result, rejected, value, cached, machine, synced),
            (
                CompareAndSwapResult { swapped: true, current_value: Some("owner".to_owned()) },
                CompareAndSwapResult { swapped: false, current_value: Some("owner".to_owned()) },
                Some("owner".to_owned()),
                Some("owner".to_owned()),
                true,
                false,
            ),
        );
    });
}

//#endregion

//#region IndexDBStorageDatabase (browser)

const DATABASE_ID: &str = "workspace-storage-db-test";

fn options() -> IndexedDbStorageDatabaseOptions {
    IndexedDbStorageDatabaseOptions { id: DATABASE_ID.to_string(), broadcast_changes: false }
}

/// `IndexedDBStorageDatabase.create({ id }, logService)`.
async fn create_database() -> Rc<dyn IIndexedDbStorageDatabase> {
    IndexedDbStorageDatabase::create(options(), log_service(), host()).await.unwrap()
}

/// `disposables.add(new Storage(disposables.add(await IndexedDBStorageDatabase.create({ id }, logService))))`.
async fn create_storage(disposables: &Rc<DisposableStore>) -> Rc<Storage> {
    let database = create_database().await;
    disposables.add(database.clone());
    let storage = Storage::new(database, StorageOptions::default());
    disposables.add_rc(storage.clone())
}

/// A suíte: o `disposables` (criado antes do guarda), o corpo e o `teardown` (`create` + `clear` + `disposables.clear()`).
fn run_indexed_db<F, Fut>(body: F)
where
    F: FnOnce(Rc<DisposableStore>) -> Fut + 'static,
    Fut: std::future::Future<Output = ()> + 'static,
{
    run_with_real_timers(async move {
        let disposables = DisposableStore::new();
        let _guard = ensure_no_disposables_are_leaked_in_test_suite();

        body(disposables.clone()).await;

        let storage = create_database().await;
        disposables.add(storage.clone());
        storage.clear().await.unwrap();

        disposables.clear();
    });
}

#[test]
fn indexed_db_basics() {
    run_indexed_db(|disposables| async move {
        let mut storage = create_storage(&disposables).await;

        storage.init().await.unwrap();

        // Insert initial data
        let _ = storage.set("bar", s("foo"), false);
        let _ = storage.set("barNumber", StorageValue::Number(55.0), false);
        let _ = storage.set("barBoolean", StorageValue::Bool(true), false);
        let _ = storage.set("barUndefined", StorageValue::Undefined, false);
        let _ = storage.set("barNull", StorageValue::Null, false);

        assert_eq!(storage.get("bar", None), Some("foo".to_owned()));
        assert_eq!(storage.get("barNumber", None), Some("55".to_owned()));
        assert_eq!(storage.get("barBoolean", None), Some("true".to_owned()));
        assert_eq!(storage.get("barUndefined", None), None);
        assert_eq!(storage.get("barNull", None), None);

        assert_eq!(storage.size(), 3);
        assert_eq!(storage.items().len(), 3);

        storage.close().await.unwrap();

        storage = create_storage(&disposables).await;

        storage.init().await.unwrap();

        // Check initial data still there
        assert_eq!(storage.get("bar", None), Some("foo".to_owned()));
        assert_eq!(storage.get("barNumber", None), Some("55".to_owned()));
        assert_eq!(storage.get("barBoolean", None), Some("true".to_owned()));
        assert_eq!(storage.get("barUndefined", None), None);
        assert_eq!(storage.get("barNull", None), None);

        assert_eq!(storage.size(), 3);
        assert_eq!(storage.items().len(), 3);

        // Update data
        let _ = storage.set("bar", s("foo2"), false);
        let _ = storage.set("barNumber", StorageValue::Number(552.0), false);

        assert_eq!(storage.get("bar", None), Some("foo2".to_owned()));
        assert_eq!(storage.get("barNumber", None), Some("552".to_owned()));

        storage.close().await.unwrap();

        storage = create_storage(&disposables).await;

        storage.init().await.unwrap();

        // Check initial data still there
        assert_eq!(storage.get("bar", None), Some("foo2".to_owned()));
        assert_eq!(storage.get("barNumber", None), Some("552".to_owned()));
        assert_eq!(storage.get("barBoolean", None), Some("true".to_owned()));
        assert_eq!(storage.get("barUndefined", None), None);
        assert_eq!(storage.get("barNull", None), None);

        assert_eq!(storage.size(), 3);
        assert_eq!(storage.items().len(), 3);

        // Delete data
        let _ = storage.delete("bar", false);
        let _ = storage.delete("barNumber", false);
        let _ = storage.delete("barBoolean", false);

        assert_eq!(storage.get("bar", Some("undefined")), Some("undefined".to_owned()));
        assert_eq!(storage.get("barNumber", Some("undefinedNumber")), Some("undefinedNumber".to_owned()));
        assert_eq!(storage.get("barBoolean", Some("undefinedBoolean")), Some("undefinedBoolean".to_owned()));

        assert_eq!(storage.size(), 0);
        assert_eq!(storage.items().len(), 0);

        storage.close().await.unwrap();

        storage = create_storage(&disposables).await;

        storage.init().await.unwrap();

        assert_eq!(storage.get("bar", Some("undefined")), Some("undefined".to_owned()));
        assert_eq!(storage.get("barNumber", Some("undefinedNumber")), Some("undefinedNumber".to_owned()));
        assert_eq!(storage.get("barBoolean", Some("undefinedBoolean")), Some("undefinedBoolean".to_owned()));

        assert_eq!(storage.size(), 0);
        assert_eq!(storage.items().len(), 0);
    });
}

/// `new Map([[key, value], ...])` como pedido de atualização de inserção.
fn insert_request(entries: &[(&str, &str)]) -> je_storage::UpdateRequest {
    let mut insert = OrderedMap::new();
    for (key, value) in entries {
        insert.insert((*key).to_string(), (*value).to_string());
    }
    je_storage::UpdateRequest { insert: Some(insert), delete: None }
}

#[test]
fn indexed_db_compare_and_swap() {
    run_indexed_db(|disposables| async move {
        let database = create_database().await;
        disposables.add(database.clone());
        database.update_items(insert_request(&[("key", "first"), ("unrelated", "sentinel")])).await.unwrap();

        let rejected = database.compare_and_swap("key", Some("stale"), "second").await.unwrap();
        let accepted = database.compare_and_swap("key", Some("first"), "second").await.unwrap();
        let items = database.get_items().await.unwrap();
        let value = database.get_value("key").await.unwrap();

        assert_eq!(
            (rejected, accepted, value, items.get("unrelated")),
            (
                CompareAndSwapResult { swapped: false, current_value: Some("first".to_owned()) },
                CompareAndSwapResult { swapped: true, current_value: Some("second".to_owned()) },
                Some("second".to_owned()),
                Some("sentinel".to_owned()),
            ),
        );
    });
}

#[test]
fn indexed_db_compare_and_swap_rejects_after_close_without_modifying_stored_values() {
    run_indexed_db(|disposables| async move {
        let database = create_database().await;
        disposables.add(database.clone());
        database.update_items(insert_request(&[("key", "first"), ("unrelated", "sentinel")])).await.unwrap();
        database.close(None).await.unwrap();

        assert!(database.compare_and_swap("key", Some("first"), "second").await.is_err());

        let reopened = create_database().await;
        disposables.add(reopened.clone());
        let items = reopened.get_items().await.unwrap();
        assert_eq!((items.get("key"), items.get("unrelated")), (Some("first".to_owned()), Some("sentinel".to_owned())));
    });
}

#[test]
fn indexed_db_compare_and_swap_is_atomic_across_database_connections() {
    run_indexed_db(|disposables| async move {
        let database_a = create_database().await;
        disposables.add(database_a.clone());
        let database_b = create_database().await;
        disposables.add(database_b.clone());
        database_a.update_items(insert_request(&[("key", "first")])).await.unwrap();

        let (promise_a, promise_b) = (database_a.compare_and_swap("key", Some("first"), "second"), database_b.compare_and_swap("key", Some("first"), "third"));
        let results = vec![promise_a.await.unwrap(), promise_b.await.unwrap()];
        let final_value = database_a.get_items().await.unwrap().get("key");
        let winner = results.iter().find(|result| result.swapped);
        let loser = results.iter().find(|result| !result.swapped);

        assert_eq!(
            (results.iter().filter(|result| result.swapped).count(), final_value, winner.map(|w| w.current_value.clone()), loser.map(|l| l.current_value.clone())),
            (1, winner.and_then(|w| w.current_value.clone()), winner.map(|w| w.current_value.clone()), winner.map(|w| w.current_value.clone())),
        );
    });
}

#[test]
fn indexed_db_clear() {
    run_indexed_db(|disposables| async move {
        let mut storage = create_storage(&disposables).await;

        storage.init().await.unwrap();

        let _ = storage.set("bar", s("foo"), false);
        let _ = storage.set("barNumber", StorageValue::Number(55.0), false);
        let _ = storage.set("barBoolean", StorageValue::Bool(true), false);

        storage.close().await.unwrap();

        let db = create_database().await;
        disposables.add(db.clone());
        let storage_database: Rc<dyn StorageDatabase> = db.clone();
        storage = disposables.add_rc(Storage::new(storage_database, StorageOptions::default()));

        storage.init().await.unwrap();
        db.clear().await.unwrap();

        storage = create_storage(&disposables).await;

        storage.init().await.unwrap();

        assert_eq!(storage.get("bar", None), None);
        assert_eq!(storage.get("barNumber", None), None);
        assert_eq!(storage.get("barBoolean", None), None);

        assert_eq!(storage.size(), 0);
        assert_eq!(storage.items().len(), 0);
    });
}

#[test]
fn indexed_db_inserts_and_deletes_at_the_same_time() {
    run_indexed_db(|disposables| async move {
        let mut storage = create_storage(&disposables).await;

        storage.init().await.unwrap();

        let _ = storage.set("bar", s("foo"), false);
        let _ = storage.set("barNumber", StorageValue::Number(55.0), false);
        let _ = storage.set("barBoolean", StorageValue::Bool(true), false);

        storage.close().await.unwrap();

        storage = create_storage(&disposables).await;

        storage.init().await.unwrap();

        let _ = storage.set("bar", s("foobar"), false);
        let large_item = format!("{{\"largeItem\":\"{}\"}}", "Hello World".repeat(1000));
        let _ = storage.set("largeItem", s(&large_item), false);
        let _ = storage.delete("barNumber", false);
        let _ = storage.delete("barBoolean", false);

        storage.close().await.unwrap();

        storage = create_storage(&disposables).await;

        storage.init().await.unwrap();

        assert_eq!(storage.get("bar", None), Some("foobar".to_owned()));
        assert_eq!(storage.get("largeItem", None), Some(large_item));
        assert_eq!(storage.get("barNumber", None), None);
        assert_eq!(storage.get("barBoolean", None), None);
    });
}

#[test]
fn indexed_db_storage_change_event() {
    run_indexed_db(|disposables| async move {
        let storage = create_storage(&disposables).await;
        let storage_change_events: Rc<RefCell<Vec<StorageChangeEvent>>> = Rc::default();
        let sink = storage_change_events.clone();
        storage.on_did_change_storage().listen_in(move |e| sink.borrow_mut().push(e.clone()), &disposables);

        storage.init().await.unwrap();

        let external_of = |key: &str| storage_change_events.borrow().iter().find(|e| e.key == key).map(|e| e.external);

        let _ = storage.set("notExternal", StorageValue::Number(42.0), false);
        assert_eq!(external_of("notExternal"), Some(false));
        storage_change_events.borrow_mut().clear();

        let _ = storage.set("isExternal", StorageValue::Number(42.0), true);
        assert_eq!(external_of("isExternal"), Some(true));

        let _ = storage.delete("notExternal", false);
        assert_eq!(external_of("notExternal"), Some(false));

        let _ = storage.delete("isExternal", true);
        assert_eq!(external_of("isExternal"), Some(true));
    });
}

//#endregion
