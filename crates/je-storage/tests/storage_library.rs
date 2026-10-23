//! Porte de `src/vs/base/parts/storage/test/node/storage.integrationTest.ts` (suíte `Storage Library`), caso a caso e na mesma ordem.

mod common;

use std::any::Any;
use std::cell::{Cell, RefCell};
use std::rc::Rc;

use common::{TestDir, all, run_with_faked_timers, run_with_real_timers, track};
use je_base::event::{Emitter, Event};
use je_base::json::{Object, Value};
use je_base::map::OrderedMap;
use je_base::path::Platform;
use je_base::promise::Promise;
use je_base::uri::URI;
use je_storage::marshalling::uri_to_value;
use je_storage::{
    IStorage, ItemMap, KeySet, Recovery, SqliteStorageDatabase, SqliteStorageDatabaseOptions,
    Storage, StorageChangeEvent, StorageDatabase, StorageItemsChangeEvent, StorageOptions,
    StorageValue, UpdateRequest, is_storage_items_change_event,
};

fn new_storage(test_dir: &TestDir) -> Rc<Storage> {
    Storage::new(
        SqliteStorageDatabase::new(
            &test_dir.join("storage.db"),
            SqliteStorageDatabaseOptions::default(),
        ),
        StorageOptions::default(),
    )
}

/// `{ bar: 'baz' }`.
fn bar_baz() -> Value {
    Value::Object(
        [("bar", Value::from("baz"))]
            .into_iter()
            .collect::<Object>(),
    )
}

/// Os `changes` do teste: o `Set<string>` das chaves dos eventos de mudança.
fn record_changes(storage: &Storage) -> Rc<RefCell<OrderedMap<String, ()>>> {
    let changes: Rc<RefCell<OrderedMap<String, ()>>> = Rc::default();
    let sink = changes.clone();
    drop(
        storage
            .on_did_change_storage()
            .listen(move |e: &StorageChangeEvent| {
                sink.borrow_mut().insert(e.key.clone(), ());
            }),
    );
    changes
}

fn has(changes: &Rc<RefCell<OrderedMap<String, ()>>>, key: &str) -> bool {
    changes.borrow().contains_key(&key.to_string())
}

#[test]
fn objects() {
    let test_dir = TestDir::new("storagelibrary");
    let storage = new_storage(&test_dir);
    run_with_faked_timers(async move {
        storage.init().await.unwrap();

        assert!(storage.get_object("foo", None).is_none());
        let uri = URI::file("path/to/folder", Platform::Posix);
        let value = Value::Object(
            [("bar", uri_to_value(&uri))]
                .into_iter()
                .collect::<Object>(),
        );
        drop(storage.set("foo", StorageValue::Object(value.clone()), false));
        assert_eq!(storage.get_object("foo", None), Some(value));

        storage.close().await.unwrap();
    });
}

#[test]
fn basics() {
    let test_dir = TestDir::new("storagelibrary");
    let storage = new_storage(&test_dir);
    run_with_faked_timers(async move {
        storage.init().await.unwrap();

        // Empty fallbacks
        assert_eq!(storage.get("foo", Some("bar")).as_deref(), Some("bar"));
        assert_eq!(storage.get_number("foo", Some(55.0)), Some(55.0));
        assert_eq!(storage.get_boolean("foo", Some(true)), Some(true));
        assert_eq!(storage.get_object("foo", Some(bar_baz())), Some(bar_baz()));

        let changes = record_changes(&storage);

        storage.when_flushed().await.unwrap(); // returns immediately when no pending updates

        // Simple updates
        let set1_promise = storage.set("bar", "foo".into(), false);
        let set2_promise = storage.set("barNumber", 55.0.into(), false);
        let set3_promise = storage.set("barBoolean", true.into(), false);
        let set4_promise = storage.set("barObject", bar_baz().into(), false);

        let flush_promise_resolved = track(&storage.when_flushed());

        assert_eq!(storage.get("bar", None).as_deref(), Some("foo"));
        assert_eq!(storage.get_number("barNumber", None), Some(55.0));
        assert_eq!(storage.get_boolean("barBoolean", None), Some(true));
        assert_eq!(storage.get_object("barObject", None), Some(bar_baz()));

        assert_eq!(changes.borrow().len(), 4);
        assert!(has(&changes, "bar"));
        assert!(has(&changes, "barNumber"));
        assert!(has(&changes, "barBoolean"));
        assert!(has(&changes, "barObject"));

        let set_promise_resolved = Rc::new(Cell::new(false));
        all(vec![set1_promise, set2_promise, set3_promise, set4_promise])
            .await
            .unwrap();
        set_promise_resolved.set(true);
        assert!(set_promise_resolved.get());
        assert!(flush_promise_resolved.get());

        changes.borrow_mut().clear();

        // Does not trigger events for same update values
        drop(storage.set("bar", "foo".into(), false));
        drop(storage.set("barNumber", 55.0.into(), false));
        drop(storage.set("barBoolean", true.into(), false));
        drop(storage.set("barObject", bar_baz().into(), false));
        assert_eq!(changes.borrow().len(), 0);

        // Simple deletes
        let delete1_promise = storage.delete("bar", false);
        let delete2_promise = storage.delete("barNumber", false);
        let delete3_promise = storage.delete("barBoolean", false);
        let delete4_promise = storage.delete("barObject", false);

        assert!(storage.get("bar", None).is_none());
        assert!(storage.get_number("barNumber", None).is_none());
        assert!(storage.get_boolean("barBoolean", None).is_none());
        assert!(storage.get_object("barObject", None).is_none());

        assert_eq!(changes.borrow().len(), 4);
        assert!(has(&changes, "bar"));
        assert!(has(&changes, "barNumber"));
        assert!(has(&changes, "barBoolean"));
        assert!(has(&changes, "barObject"));

        changes.borrow_mut().clear();

        // Does not trigger events for same delete values
        drop(storage.delete("bar", false));
        drop(storage.delete("barNumber", false));
        drop(storage.delete("barBoolean", false));
        drop(storage.delete("barObject", false));
        assert_eq!(changes.borrow().len(), 0);

        let delete_promise_resolved = Rc::new(Cell::new(false));
        all(vec![
            delete1_promise,
            delete2_promise,
            delete3_promise,
            delete4_promise,
        ])
        .await
        .unwrap();
        delete_promise_resolved.set(true);
        assert!(delete_promise_resolved.get());

        storage.close().await.unwrap();
        storage.close().await.unwrap(); // it is ok to call this multiple times
    });
}

/// `class TestSQLiteStorageDatabase extends SQLiteStorageDatabase`: o evento externo é do teste e o
/// resto é do banco herdado.
struct TestSqliteStorageDatabase {
    inner: Rc<SqliteStorageDatabase>,
    on_did_change_items_external: Rc<Emitter<StorageItemsChangeEvent>>,
}

impl TestSqliteStorageDatabase {
    fn new(path: &str) -> Rc<Self> {
        Rc::new(Self {
            inner: SqliteStorageDatabase::new(path, SqliteStorageDatabaseOptions::default()),
            on_did_change_items_external: Emitter::new(),
        })
    }

    fn fire_did_change_items_external(&self, event: StorageItemsChangeEvent) {
        self.on_did_change_items_external.fire(event);
    }
}

impl StorageDatabase for TestSqliteStorageDatabase {
    fn on_did_change_items_external(&self) -> Event<StorageItemsChangeEvent> {
        self.on_did_change_items_external.event()
    }

    fn get_items(&self) -> Promise<ItemMap> {
        self.inner.get_items()
    }

    fn update_items(&self, request: UpdateRequest) -> Promise<()> {
        self.inner.update_items(request)
    }

    fn optimize(&self) -> Promise<()> {
        self.inner.optimize()
    }

    fn close(&self, recovery: Option<Recovery>) -> Promise<()> {
        self.inner.close(recovery)
    }
}

fn single(key: &str, value: &str) -> OrderedMap<String, String> {
    let mut map = OrderedMap::new();
    map.insert(key.to_string(), value.to_string());
    map
}

#[test]
fn external_changes() {
    let test_dir = TestDir::new("storagelibrary");
    let database = TestSqliteStorageDatabase::new(&test_dir.join("storage.db"));
    let storage = Storage::new(database.clone(), StorageOptions::default());
    run_with_faked_timers(async move {
        let changes = record_changes(&storage);

        storage.init().await.unwrap();

        storage.set("foo", "bar".into(), false).await.unwrap();
        assert!(has(&changes, "foo"));
        changes.borrow_mut().clear();

        // Nothing happens if changing to same value
        let mut changed = single("foo", "bar");
        database.fire_did_change_items_external(StorageItemsChangeEvent {
            changed: Some(changed.clone()),
            deleted: None,
        });
        assert_eq!(changes.borrow().len(), 0);

        // Change is accepted if valid
        changed.insert("foo".to_string(), "bar1".to_string());
        database.fire_did_change_items_external(StorageItemsChangeEvent {
            changed: Some(changed.clone()),
            deleted: None,
        });
        assert!(has(&changes, "foo"));
        assert_eq!(storage.get("foo", None).as_deref(), Some("bar1"));
        changes.borrow_mut().clear();

        // Delete is accepted
        let mut deleted = KeySet::new();
        deleted.insert("foo".to_string(), ());
        database.fire_did_change_items_external(StorageItemsChangeEvent {
            changed: None,
            deleted: Some(deleted.clone()),
        });
        assert!(has(&changes, "foo"));
        assert_eq!(storage.get("foo", None), None);
        changes.borrow_mut().clear();

        // Nothing happens if changing to same value
        database.fire_did_change_items_external(StorageItemsChangeEvent {
            changed: None,
            deleted: Some(deleted.clone()),
        });
        assert_eq!(changes.borrow().len(), 0);

        let event = |changed: &Option<OrderedMap<String, String>>, deleted: &Option<KeySet>| {
            StorageItemsChangeEvent {
                changed: changed.clone(),
                deleted: deleted.clone(),
            }
        };
        let (changed, deleted) = (Some(changed), Some(deleted));
        assert!(is_storage_items_change_event(Some(
            &event(&changed, &None) as &dyn Any
        )));
        assert!(is_storage_items_change_event(Some(
            &event(&None, &deleted) as &dyn Any
        )));
        assert!(is_storage_items_change_event(Some(
            &event(&changed, &deleted) as &dyn Any
        )));
        assert!(!is_storage_items_change_event(None));
        // `{ changed: 'yes', deleted: false }`: campos que não são `Map` nem `Set` não têm tipo aqui, e
        // qualquer valor de outro tipo é um objeto que não é um evento de mudança.
        assert!(!is_storage_items_change_event(Some(&"yes" as &dyn Any)));

        storage.close().await.unwrap();
    });
}

#[test]
fn close_flushes_data() {
    let test_dir = TestDir::new("storagelibrary");
    run_with_real_timers(async move {
        let mut storage = new_storage(&test_dir);
        storage.init().await.unwrap();

        let set1_promise = storage.set("foo", "bar".into(), false);
        let set2_promise = storage.set("bar", "foo".into(), false);

        let flush_promise_resolved = track(&storage.when_flushed());

        assert_eq!(storage.get("foo", None).as_deref(), Some("bar"));
        assert_eq!(storage.get("bar", None).as_deref(), Some("foo"));

        let set_promise_resolved = Rc::new(Cell::new(false));
        let flag = set_promise_resolved.clone();
        drop(Promise::all_settled(vec![set1_promise, set2_promise]).then(move |_| flag.set(true)));

        storage.close().await.unwrap();

        assert!(set_promise_resolved.get());
        assert!(flush_promise_resolved.get());

        storage = new_storage(&test_dir);
        storage.init().await.unwrap();

        assert_eq!(storage.get("foo", None).as_deref(), Some("bar"));
        assert_eq!(storage.get("bar", None).as_deref(), Some("foo"));

        storage.close().await.unwrap();

        storage = new_storage(&test_dir);
        storage.init().await.unwrap();

        let delete1_promise = storage.delete("foo", false);
        let delete2_promise = storage.delete("bar", false);

        assert!(storage.get("foo", None).is_none());
        assert!(storage.get("bar", None).is_none());

        let delete_promise_resolved = Rc::new(Cell::new(false));
        let flag = delete_promise_resolved.clone();
        drop(
            Promise::all_settled(vec![delete1_promise, delete2_promise])
                .then(move |_| flag.set(true)),
        );

        storage.close().await.unwrap();

        assert!(delete_promise_resolved.get());

        storage = new_storage(&test_dir);
        storage.init().await.unwrap();

        assert!(storage.get("foo", None).is_none());
        assert!(storage.get("bar", None).is_none());

        storage.close().await.unwrap();
    });
}

#[test]
fn explicit_flush() {
    let test_dir = TestDir::new("storagelibrary");
    run_with_real_timers(async move {
        let storage = new_storage(&test_dir);
        storage.init().await.unwrap();

        drop(storage.set("foo", "bar".into(), false));
        drop(storage.set("bar", "foo".into(), false));

        let flush_promise_resolved = track(&storage.when_flushed());

        assert!(!flush_promise_resolved.get());

        storage.flush(Some(0)).await.unwrap();

        assert!(flush_promise_resolved.get());

        storage.close().await.unwrap();
    });
}

#[test]
fn conflicting_updates() {
    let test_dir = TestDir::new("storagelibrary");
    let storage = new_storage(&test_dir);
    run_with_faked_timers(async move {
        storage.init().await.unwrap();

        let changes = record_changes(&storage);

        let set1_promise = storage.set("foo", "bar1".into(), false);
        let set2_promise = storage.set("foo", "bar2".into(), false);
        let set3_promise = storage.set("foo", "bar3".into(), false);

        let flush_promise_resolved = track(&storage.when_flushed());

        assert_eq!(storage.get("foo", None).as_deref(), Some("bar3"));
        assert_eq!(changes.borrow().len(), 1);
        assert!(has(&changes, "foo"));

        all(vec![set1_promise, set2_promise, set3_promise])
            .await
            .unwrap();
        assert!(flush_promise_resolved.get());

        changes.borrow_mut().clear();

        let set4_promise = storage.set("bar", "foo".into(), false);
        let delete1_promise = storage.delete("bar", false);

        assert!(storage.get("bar", None).is_none());

        assert_eq!(changes.borrow().len(), 1);
        assert!(has(&changes, "bar"));

        all(vec![set4_promise, delete1_promise]).await.unwrap();

        storage.close().await.unwrap();
    });
}

#[test]
fn corrupt_db_recovers() {
    let test_dir = TestDir::new("storagelibrary");
    run_with_faked_timers(async move {
        let storage_file = test_dir.join("storage.db");

        let mut storage = Storage::new(
            SqliteStorageDatabase::new(&storage_file, SqliteStorageDatabaseOptions::default()),
            StorageOptions::default(),
        );
        storage.init().await.unwrap();

        storage.set("bar", "foo".into(), false).await.unwrap();

        std::fs::write(&storage_file, "This is a broken DB").unwrap();

        storage.set("foo", "bar".into(), false).await.unwrap();

        assert_eq!(storage.get("bar", None).as_deref(), Some("foo"));
        assert_eq!(storage.get("foo", None).as_deref(), Some("bar"));

        storage.close().await.unwrap();

        storage = Storage::new(
            SqliteStorageDatabase::new(&storage_file, SqliteStorageDatabaseOptions::default()),
            StorageOptions::default(),
        );
        storage.init().await.unwrap();

        assert_eq!(storage.get("bar", None).as_deref(), Some("foo"));
        assert_eq!(storage.get("foo", None).as_deref(), Some("bar"));

        storage.close().await.unwrap();
    });
}
