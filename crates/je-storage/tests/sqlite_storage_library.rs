//! Porte de `src/vs/base/parts/storage/test/node/storage.integrationTest.ts` (suíte `SQLite Storage Library`), caso a caso e na mesma ordem.

mod common;

use std::cell::{Cell, RefCell};
use std::path::Path;
use std::rc::Rc;

use common::{TestDir, run_with_faked_timers, run_with_real_timers, timeout};
use je_base::map::OrderedMap;
use je_base::uuid::generate_uuid;
use je_storage::{
    IStorage, KeySet, LogFn, Recovery, SqliteStorageDatabase, SqliteStorageDatabaseLoggingOptions,
    SqliteStorageDatabaseOptions, Storage, StorageDatabase, StorageOptions, UpdateRequest,
};

fn to_set(elements: &[&str]) -> KeySet {
    let mut set = KeySet::new();
    for element in elements {
        set.insert((*element).to_string(), ());
    }
    set
}

fn to_map(entries: &[(&str, &str)]) -> OrderedMap<String, String> {
    let mut map = OrderedMap::new();
    for (key, value) in entries {
        map.insert((*key).to_string(), (*value).to_string());
    }
    map
}

fn insert(items: &OrderedMap<String, String>) -> UpdateRequest {
    UpdateRequest {
        insert: Some(items.clone()),
        delete: None,
    }
}

fn delete(keys: KeySet) -> UpdateRequest {
    UpdateRequest {
        insert: None,
        delete: Some(keys),
    }
}

fn database(path: &str) -> Rc<SqliteStorageDatabase> {
    SqliteStorageDatabase::new(path, SqliteStorageDatabaseOptions::default())
}

/// O `recovery` que registra se foi chamado e devolve `items`.
fn recovery_returning(called: &Rc<Cell<bool>>, items: OrderedMap<String, String>) -> Recovery {
    let called = called.clone();
    Rc::new(move || {
        called.set(true);
        items.clone()
    })
}

/// `{ foo: 'bar' }` e `{ bar: 'foo' }` como texto JSON.
fn json_key() -> String {
    r#"{"foo":"bar"}"#.to_string()
}

fn json_value() -> String {
    r#"{"bar":"foo"}"#.to_string()
}

fn basic_items() -> OrderedMap<String, String> {
    to_map(&[
        ("foo", "bar"),
        ("some/foo/path", "some/bar/path"),
        (&json_key(), &json_value()),
    ])
}

async fn test_db_basics(path: String, log_error: Option<LogFn>) {
    let options = SqliteStorageDatabaseOptions {
        logging: log_error.map(|log_error| SqliteStorageDatabaseLoggingOptions {
            log_error: Some(log_error),
            log_trace: None,
        }),
        ..SqliteStorageDatabaseOptions::default()
    };
    let storage = SqliteStorageDatabase::new(&path, options);

    let items = basic_items();

    let mut stored_items = storage.get_items().await.unwrap();
    assert_eq!(stored_items.len(), 0);

    storage.update_items(insert(&items)).await.unwrap();

    stored_items = storage.get_items().await.unwrap();
    assert_eq!(stored_items.len(), items.len());
    assert_eq!(stored_items.get("foo").as_deref(), Some("bar"));
    assert_eq!(
        stored_items.get("some/foo/path").as_deref(),
        Some("some/bar/path")
    );
    assert_eq!(stored_items.get(&json_key()), Some(json_value()));

    storage
        .update_items(delete(to_set(&["foo"])))
        .await
        .unwrap();
    stored_items = storage.get_items().await.unwrap();
    assert_eq!(stored_items.len(), items.len() - 1);
    assert!(!stored_items.has("foo"));
    assert_eq!(
        stored_items.get("some/foo/path").as_deref(),
        Some("some/bar/path")
    );
    assert_eq!(stored_items.get(&json_key()), Some(json_value()));

    storage.update_items(insert(&items)).await.unwrap();
    stored_items = storage.get_items().await.unwrap();
    assert_eq!(stored_items.len(), items.len());
    assert_eq!(stored_items.get("foo").as_deref(), Some("bar"));
    assert_eq!(
        stored_items.get("some/foo/path").as_deref(),
        Some("some/bar/path")
    );
    assert_eq!(stored_items.get(&json_key()), Some(json_value()));

    let items_change = to_map(&[("foo", "otherbar")]);
    storage.update_items(insert(&items_change)).await.unwrap();

    stored_items = storage.get_items().await.unwrap();
    assert_eq!(stored_items.get("foo").as_deref(), Some("otherbar"));

    storage
        .update_items(delete(to_set(&[
            "foo",
            "bar",
            "some/foo/path",
            &json_key(),
        ])))
        .await
        .unwrap();
    stored_items = storage.get_items().await.unwrap();
    assert_eq!(stored_items.len(), 0);

    storage
        .update_items(UpdateRequest {
            insert: Some(items.clone()),
            delete: Some(to_set(&["foo", "some/foo/path", "other"])),
        })
        .await
        .unwrap();
    stored_items = storage.get_items().await.unwrap();
    assert_eq!(stored_items.len(), 1);
    assert_eq!(stored_items.get(&json_key()), Some(json_value()));

    storage
        .update_items(delete(to_set(&[&json_key()])))
        .await
        .unwrap();
    stored_items = storage.get_items().await.unwrap();
    assert_eq!(stored_items.len(), 0);

    let recovery_called = Rc::new(Cell::new(false));
    storage
        .close(Some(recovery_returning(
            &recovery_called,
            OrderedMap::new(),
        )))
        .await
        .unwrap();

    assert!(!recovery_called.get());
}

#[test]
fn basics() {
    let testdir = TestDir::new("storagelibrary");
    let path = testdir.join("storage.db");
    run_with_real_timers(async move {
        test_db_basics(path, None).await;
    });
}

#[test]
fn basics_open_multiple_times() {
    let testdir = TestDir::new("storagelibrary");
    let path = testdir.join("storage.db");
    run_with_real_timers(async move {
        test_db_basics(path.clone(), None).await;
        test_db_basics(path, None).await;
    });
}

#[test]
fn basics_corrupt_db_falls_back_to_empty_db() {
    let testdir = TestDir::new("storagelibrary");
    let corrupt_db_path = testdir.join("broken.db");
    run_with_real_timers(async move {
        std::fs::write(&corrupt_db_path, "This is a broken DB").unwrap();

        let expected_error: Rc<RefCell<Option<String>>> = Rc::default();
        let sink = expected_error.clone();
        test_db_basics(
            corrupt_db_path,
            Some(Rc::new(move |error: &str| {
                *sink.borrow_mut() = Some(error.to_string());
            })),
        )
        .await;

        assert!(expected_error.borrow().is_some());
    });
}

#[test]
fn basics_corrupt_db_restores_from_previous_backup() {
    let testdir = TestDir::new("storagelibrary");
    let storage_path = testdir.join("storage.db");
    run_with_real_timers(async move {
        let mut storage = database(&storage_path);

        let items = basic_items();

        storage.update_items(insert(&items)).await.unwrap();
        storage.close(None).await.unwrap();

        std::fs::write(&storage_path, "This is now a broken DB").unwrap();

        storage = database(&storage_path);

        let stored_items = storage.get_items().await.unwrap();
        assert_eq!(stored_items.len(), items.len());
        assert_eq!(stored_items.get("foo").as_deref(), Some("bar"));
        assert_eq!(
            stored_items.get("some/foo/path").as_deref(),
            Some("some/bar/path")
        );
        assert_eq!(stored_items.get(&json_key()), Some(json_value()));

        let recovery_called = Rc::new(Cell::new(false));
        storage
            .close(Some(recovery_returning(
                &recovery_called,
                OrderedMap::new(),
            )))
            .await
            .unwrap();

        assert!(!recovery_called.get());
    });
}

#[test]
fn basics_corrupt_db_falls_back_to_empty_db_if_backup_is_corrupt() {
    let testdir = TestDir::new("storagelibrary");
    let storage_path = testdir.join("storage.db");
    run_with_real_timers(async move {
        let mut storage = database(&storage_path);

        let items = basic_items();

        storage.update_items(insert(&items)).await.unwrap();
        storage.close(None).await.unwrap();

        std::fs::write(&storage_path, "This is now a broken DB").unwrap();
        std::fs::write(
            format!("{storage_path}.backup"),
            "This is now also a broken DB",
        )
        .unwrap();

        storage = database(&storage_path);

        let stored_items = storage.get_items().await.unwrap();
        assert_eq!(stored_items.len(), 0);

        test_db_basics(storage_path, None).await;
    });
}

// O original pula este caso no Windows ("Windows will fail to write to open DB due to locking").
#[cfg_attr(
    windows,
    ignore = "Windows will fail to write to open DB due to locking"
)]
#[test]
fn basics_db_that_becomes_corrupt_during_runtime_stores_all_state_from_cache_on_close() {
    let testdir = TestDir::new("storagelibrary");
    let storage_path = testdir.join("storage.db");
    run_with_real_timers(async move {
        let mut storage = database(&storage_path);

        let items = basic_items();

        storage.update_items(insert(&items)).await.unwrap();
        storage.close(None).await.unwrap();

        let backup_path = format!("{storage_path}.backup");
        assert!(Path::new(&backup_path).exists());

        storage = database(&storage_path);
        storage.get_items().await.unwrap();

        std::fs::write(&storage_path, "This is now a broken DB").unwrap();

        // we still need to trigger a check to the DB so that we get to know that
        // the DB is corrupt. We have no extra code on shutdown that checks for the
        // health of the DB. This is an optimization to not perform too many tasks
        // on shutdown.
        let _ = storage.check_integrity(true).await; // error is expected here but we do not want to fail

        std::fs::remove_file(&backup_path).unwrap(); // also test that the recovery DB is backed up properly

        let recovery_called = Rc::new(Cell::new(false));
        storage
            .close(Some(recovery_returning(&recovery_called, items.clone())))
            .await
            .unwrap();

        assert!(recovery_called.get());
        assert!(Path::new(&backup_path).exists());

        storage = database(&storage_path);

        let stored_items = storage.get_items().await.unwrap();
        assert_eq!(stored_items.len(), items.len());
        assert_eq!(stored_items.get("foo").as_deref(), Some("bar"));
        assert_eq!(
            stored_items.get("some/foo/path").as_deref(),
            Some("some/bar/path")
        );
        assert_eq!(stored_items.get(&json_key()), Some(json_value()));

        recovery_called.set(false);
        storage
            .close(Some(recovery_returning(
                &recovery_called,
                OrderedMap::new(),
            )))
            .await
            .unwrap();

        assert!(!recovery_called.get());
    });
}

#[test]
fn real_world_example() {
    let testdir = TestDir::new("storagelibrary");
    let path = testdir.join("storage.db");
    run_with_real_timers(async move {
        let mut storage = database(&path);

        let items1 = to_map(&[
            ("colorthemedata", COLOR_THEME_DATA),
            ("commandpalette.mru.cache", COMMAND_PALETTE_MRU_CACHE),
            ("cpp.1.lastsessiondate", "Fri Oct 05 2018"),
            ("debug.actionswidgetposition", "0.6880952380952381"),
        ]);

        let items2 = to_map(&[(
            "workbench.editors.files.textfileeditor",
            TEXT_FILE_EDITOR_STATE,
        )]);

        let items3 = to_map(&[
            ("nps/iscandidate", "false"),
            (
                "telemetry.instanceid",
                "d52bfcd4-4be6-476b-a38f-d44c717c41d6",
            ),
            ("workbench.activity.pinnedviewlets", PINNED_VIEWLETS),
            ("workbench.panel.height", "419"),
            (VERY_LONG_KEY, "is long"),
        ]);

        let mut stored_items = storage.get_items().await.unwrap();
        assert_eq!(stored_items.len(), 0);

        // `await Promise.all([await a, await b, await c])`: cada `await` interno já espera a operação.
        storage.update_items(insert(&items1)).await.unwrap();
        storage.update_items(insert(&items2)).await.unwrap();
        storage.update_items(insert(&items3)).await.unwrap();

        assert_eq!(storage.check_integrity(true).await.unwrap(), "ok");
        assert_eq!(storage.check_integrity(false).await.unwrap(), "ok");

        stored_items = storage.get_items().await.unwrap();
        assert_eq!(
            stored_items.len(),
            items1.len() + items2.len() + items3.len()
        );

        let mut items1_keys = Vec::new();
        for (key, value) in items1.iter() {
            items1_keys.push(key.clone());
            assert_eq!(stored_items.get(key).as_ref(), Some(value));
        }

        let mut items2_keys = Vec::new();
        for (key, value) in items2.iter() {
            items2_keys.push(key.clone());
            assert_eq!(stored_items.get(key).as_ref(), Some(value));
        }

        let mut items3_keys = Vec::new();
        for (key, value) in items3.iter() {
            items3_keys.push(key.clone());
            assert_eq!(stored_items.get(key).as_ref(), Some(value));
        }

        let key_set =
            |keys: &[String]| to_set(&keys.iter().map(String::as_str).collect::<Vec<_>>());
        storage
            .update_items(delete(key_set(&items1_keys)))
            .await
            .unwrap();
        storage
            .update_items(delete(key_set(&items2_keys)))
            .await
            .unwrap();
        storage
            .update_items(delete(key_set(&items3_keys)))
            .await
            .unwrap();

        stored_items = storage.get_items().await.unwrap();
        assert_eq!(stored_items.len(), 0);

        storage.update_items(insert(&items1)).await.unwrap();
        storage.get_items().await.unwrap();
        storage.update_items(insert(&items2)).await.unwrap();
        storage.get_items().await.unwrap();
        storage.update_items(insert(&items3)).await.unwrap();
        storage.get_items().await.unwrap();

        stored_items = storage.get_items().await.unwrap();
        assert_eq!(
            stored_items.len(),
            items1.len() + items2.len() + items3.len()
        );

        storage.close(None).await.unwrap();

        storage = database(&path);

        stored_items = storage.get_items().await.unwrap();
        assert_eq!(
            stored_items.len(),
            items1.len() + items2.len() + items3.len()
        );

        storage.close(None).await.unwrap();
    });
}

#[test]
fn very_large_item_value() {
    let testdir = TestDir::new("storagelibrary");
    let path = testdir.join("storage.db");
    run_with_real_timers(async move {
        let storage = database(&path);

        let mut random_data = create_large_random_data(); // 3.6MB

        storage.update_items(insert(&random_data)).await.unwrap();

        let mut stored_items = storage.get_items().await.unwrap();
        assert_eq!(
            random_data.get(&"colorthemedata".to_string()).cloned(),
            stored_items.get("colorthemedata")
        );
        assert_eq!(
            random_data
                .get(&"commandpalette.mru.cache".to_string())
                .cloned(),
            stored_items.get("commandpalette.mru.cache")
        );
        assert_eq!(
            random_data.get(&"super.large.string".to_string()).cloned(),
            stored_items.get("super.large.string")
        );

        random_data = create_large_random_data();

        storage.update_items(insert(&random_data)).await.unwrap();

        stored_items = storage.get_items().await.unwrap();
        assert_eq!(
            random_data.get(&"colorthemedata".to_string()).cloned(),
            stored_items.get("colorthemedata")
        );
        assert_eq!(
            random_data
                .get(&"commandpalette.mru.cache".to_string())
                .cloned(),
            stored_items.get("commandpalette.mru.cache")
        );
        assert_eq!(
            random_data.get(&"super.large.string".to_string()).cloned(),
            stored_items.get("super.large.string")
        );

        let mut to_delete = KeySet::new();
        to_delete.insert("super.large.string".to_string(), ());
        storage.update_items(delete(to_delete)).await.unwrap();

        stored_items = storage.get_items().await.unwrap();
        assert_eq!(
            random_data.get(&"colorthemedata".to_string()).cloned(),
            stored_items.get("colorthemedata")
        );
        assert_eq!(
            random_data
                .get(&"commandpalette.mru.cache".to_string())
                .cloned(),
            stored_items.get("commandpalette.mru.cache")
        );
        assert!(stored_items.get("super.large.string").is_none());

        storage.close(None).await.unwrap();
    });
}

#[test]
fn multiple_concurrent_writes_execute_in_sequence() {
    let testdir = TestDir::new("storagelibrary");
    let path = testdir.join("storage.db");
    run_with_faked_timers(async move {
        let storage = Storage::new(database(&path), StorageOptions::default());

        storage.init().await.unwrap();

        drop(storage.set("foo", "bar".into(), false));
        drop(storage.set("some/foo/path", "some/bar/path".into(), false));

        timeout(2).await.unwrap();

        drop(storage.set("foo1", "bar".into(), false));
        drop(storage.set("some/foo1/path", "some/bar/path".into(), false));

        timeout(2).await.unwrap();

        drop(storage.set("foo2", "bar".into(), false));
        drop(storage.set("some/foo2/path", "some/bar/path".into(), false));

        timeout(2).await.unwrap();

        drop(storage.delete("foo1", false));
        drop(storage.delete("some/foo1/path", false));

        timeout(2).await.unwrap();

        drop(storage.delete("foo4", false));
        drop(storage.delete("some/foo4/path", false));

        timeout(5).await.unwrap();

        drop(storage.set("foo3", "bar".into(), false));
        storage
            .set("some/foo3/path", "some/bar/path".into(), false)
            .await
            .unwrap();

        let items = storage.database.get_items().await.unwrap();
        assert_eq!(items.get("foo").as_deref(), Some("bar"));
        assert_eq!(items.get("some/foo/path").as_deref(), Some("some/bar/path"));
        assert!(!items.has("foo1"));
        assert!(!items.has("some/foo1/path"));
        assert_eq!(items.get("foo2").as_deref(), Some("bar"));
        assert_eq!(
            items.get("some/foo2/path").as_deref(),
            Some("some/bar/path")
        );
        assert_eq!(items.get("foo3").as_deref(), Some("bar"));
        assert_eq!(
            items.get("some/foo3/path").as_deref(),
            Some("some/bar/path")
        );

        storage.close().await.unwrap();
    });
}

#[test]
fn lots_of_insert_and_delete_below_inline_max() {
    let testdir = TestDir::new("storagelibrary");
    let path = testdir.join("storage.db");
    run_with_real_timers(async move {
        let storage = database(&path);

        let (items, keys) = create_many_random_data(200, false);

        storage.update_items(insert(&items)).await.unwrap();

        let mut stored_items = storage.get_items().await.unwrap();
        assert_eq!(stored_items.len(), items.len());

        storage.update_items(delete(keys)).await.unwrap();

        stored_items = storage.get_items().await.unwrap();
        assert_eq!(stored_items.len(), 0);

        storage.close(None).await.unwrap();
    });
}

#[test]
fn lots_of_insert_and_delete_above_inline_max() {
    let testdir = TestDir::new("storagelibrary");
    let path = testdir.join("storage.db");
    run_with_real_timers(async move {
        let storage = database(&path);

        let (items, keys) = create_many_random_data(400, false);

        storage.update_items(insert(&items)).await.unwrap();

        let mut stored_items = storage.get_items().await.unwrap();
        assert_eq!(stored_items.len(), items.len());

        storage.update_items(delete(keys)).await.unwrap();

        stored_items = storage.get_items().await.unwrap();
        assert_eq!(stored_items.len(), 0);

        storage.close(None).await.unwrap();
    });
}

#[test]
fn invalid_path_does_not_hang() {
    let testdir = TestDir::new("storagelibrary");
    let path = testdir
        .path()
        .join("nonexist")
        .join("storage.db")
        .to_string_lossy()
        .into_owned();
    run_with_real_timers(async move {
        let storage = database(&path);

        let error = async {
            storage.get_items().await?;
            storage.close(None).await
        }
        .await
        .err();

        assert!(error.is_some());
    });
}

#[test]
fn optimize() {
    let testdir = TestDir::new("storagelibrary");
    let db_path = testdir.join("storage.db");
    run_with_real_timers(async move {
        let mut storage = database(&db_path);

        let (items, keys) = create_many_random_data(400, true);

        storage.update_items(insert(&items)).await.unwrap();

        let mut stored_items = storage.get_items().await.unwrap();
        assert_eq!(stored_items.len(), items.len());

        storage.optimize().await.unwrap();
        storage.close(None).await.unwrap();

        let size_before_delete_and_optimize = std::fs::metadata(&db_path).unwrap().len();

        storage = database(&db_path);

        stored_items = storage.get_items().await.unwrap();
        assert_eq!(stored_items.len(), items.len());

        storage.update_items(delete(keys)).await.unwrap();

        stored_items = storage.get_items().await.unwrap();
        assert_eq!(stored_items.len(), 0);

        storage.optimize().await.unwrap();
        storage.close(None).await.unwrap();

        storage = database(&db_path);

        stored_items = storage.get_items().await.unwrap();
        assert_eq!(stored_items.len(), 0);

        storage.close(None).await.unwrap();

        let size_after_delete_and_optimize = std::fs::metadata(&db_path).unwrap().len();

        assert!(size_after_delete_and_optimize < size_before_delete_and_optimize);
    });
}

fn create_many_random_data(
    length: usize,
    include_very_large: bool,
) -> (OrderedMap<String, String>, KeySet) {
    let mut items = OrderedMap::new();
    let mut keys = KeySet::new();

    for _ in 0..length {
        let uuid = generate_uuid();
        let key = format!("key: {uuid}");

        items.insert(key.clone(), format!("value: {uuid}"));
        keys.insert(key, ());
    }

    if include_very_large {
        let large_data = create_large_random_data();
        for (key, value) in large_data.iter() {
            items.insert(key.clone(), value.clone());
            keys.insert(key.clone(), ());
        }
    }

    (items, keys)
}

fn create_large_random_data() -> OrderedMap<String, String> {
    let mut items = OrderedMap::new();
    items.insert("colorthemedata".to_string(), COLOR_THEME_DATA.to_string());
    items.insert(
        "commandpalette.mru.cache".to_string(),
        COMMAND_PALETTE_MRU_CACHE.to_string(),
    );

    let uuid = generate_uuid();
    let value = vec![uuid.as_str(); 100_000];

    items.insert("super.large.string".to_string(), value.join(",")); // 3.6MB

    items
}

// ---- Dados do teste (copiados do original) -------------------------------------------------------

const COLOR_THEME_DATA: &str = r##"{"id":"vs vscode-theme-defaults-themes-light_plus-json","label":"Light+ (default light)","settingsId":"Default Light+","selector":"vs.vscode-theme-defaults-themes-light_plus-json","themeTokenColors":[{"settings":{"foreground":"#000000ff","background":"#ffffffff"}},{"scope":["meta.embedded","source.groovy.embedded"],"settings":{"foreground":"#000000ff"}},{"scope":"emphasis","settings":{"fontStyle":"italic"}},{"scope":"strong","settings":{"fontStyle":"bold"}},{"scope":"meta.diff.header","settings":{"foreground":"#000080"}},{"scope":"comment","settings":{"foreground":"#008000"}},{"scope":"constant.language","settings":{"foreground":"#0000ff"}},{"scope":["constant.numeric"],"settings":{"foreground":"#098658"}},{"scope":"constant.regexp","settings":{"foreground":"#811f3f"}},{"name":"css tags in selectors, xml tags","scope":"entity.name.tag","settings":{"foreground":"#800000"}},{"scope":"entity.name.selector","settings":{"foreground":"#800000"}},{"scope":"entity.other.attribute-name","settings":{"foreground":"#ff0000"}},{"scope":["entity.other.attribute-name.class.css","entity.other.attribute-name.class.mixin.css","entity.other.attribute-name.id.css","entity.other.attribute-name.parent-selector.css","entity.other.attribute-name.pseudo-class.css","entity.other.attribute-name.pseudo-element.css","source.css.less entity.other.attribute-name.id","entity.other.attribute-name.attribute.scss","entity.other.attribute-name.scss"],"settings":{"foreground":"#800000"}},{"scope":"invalid","settings":{"foreground":"#cd3131"}},{"scope":"markup.underline","settings":{"fontStyle":"underline"}},{"scope":"markup.bold","settings":{"fontStyle":"bold","foreground":"#000080"}},{"scope":"markup.heading","settings":{"fontStyle":"bold","foreground":"#800000"}},{"scope":"markup.italic","settings":{"fontStyle":"italic"}},{"scope":"markup.inserted","settings":{"foreground":"#098658"}},{"scope":"markup.deleted","settings":{"foreground":"#a31515"}},{"scope":"markup.changed","settings":{"foreground":"#0451a5"}},{"scope":["punctuation.definition.quote.begin.markdown","punctuation.definition.list.begin.markdown"],"settings":{"foreground":"#0451a5"}},{"scope":"markup.inline.raw","settings":{"foreground":"#800000"}},{"name":"brackets of XML/HTML tags","scope":"punctuation.definition.tag","settings":{"foreground":"#800000"}},{"scope":"meta.preprocessor","settings":{"foreground":"#0000ff"}},{"scope":"meta.preprocessor.string","settings":{"foreground":"#a31515"}},{"scope":"meta.preprocessor.numeric","settings":{"foreground":"#098658"}},{"scope":"meta.structure.dictionary.key.python","settings":{"foreground":"#0451a5"}},{"scope":"storage","settings":{"foreground":"#0000ff"}},{"scope":"storage.type","settings":{"foreground":"#0000ff"}},{"scope":"storage.modifier","settings":{"foreground":"#0000ff"}},{"scope":"string","settings":{"foreground":"#a31515"}},{"scope":["string.comment.buffered.block.pug","string.quoted.pug","string.interpolated.pug","string.unquoted.plain.in.yaml","string.unquoted.plain.out.yaml","string.unquoted.block.yaml","string.quoted.single.yaml","string.quoted.double.xml","string.quoted.single.xml","string.unquoted.cdata.xml","string.quoted.double.html","string.quoted.single.html","string.unquoted.html","string.quoted.single.handlebars","string.quoted.double.handlebars"],"settings":{"foreground":"#0000ff"}},{"scope":"string.regexp","settings":{"foreground":"#811f3f"}},{"name":"String interpolation","scope":["punctuation.definition.template-expression.begin","punctuation.definition.template-expression.end","punctuation.section.embedded"],"settings":{"foreground":"#0000ff"}},{"name":"Reset JavaScript string interpolation expression","scope":["meta.template.expression"],"settings":{"foreground":"#000000"}},{"scope":["support.constant.property-value","support.constant.font-name","support.constant.media-type","support.constant.media","constant.other.color.rgb-value","constant.other.rgb-value","support.constant.color"],"settings":{"foreground":"#0451a5"}},{"scope":["support.type.vendored.property-name","support.type.property-name","variable.css","variable.scss","variable.other.less","source.coffee.embedded"],"settings":{"foreground":"#ff0000"}},{"scope":["support.type.property-name.json"],"settings":{"foreground":"#0451a5"}},{"scope":"keyword","settings":{"foreground":"#0000ff"}},{"scope":"keyword.control","settings":{"foreground":"#0000ff"}},{"scope":"keyword.operator","settings":{"foreground":"#000000"}},{"scope":["keyword.operator.new","keyword.operator.expression","keyword.operator.cast","keyword.operator.sizeof","keyword.operator.instanceof","keyword.operator.logical.python"],"settings":{"foreground":"#0000ff"}},{"scope":"keyword.other.unit","settings":{"foreground":"#098658"}},{"scope":["punctuation.section.embedded.begin.php","punctuation.section.embedded.end.php"],"settings":{"foreground":"#800000"}},{"scope":"support.function.git-rebase","settings":{"foreground":"#0451a5"}},{"scope":"constant.sha.git-rebase","settings":{"foreground":"#098658"}},{"name":"coloring of the Java import and package identifiers","scope":["storage.modifier.import.java","variable.language.wildcard.java","storage.modifier.package.java"],"settings":{"foreground":"#000000"}},{"name":"this.self","scope":"variable.language","settings":{"foreground":"#0000ff"}},{"name":"Function declarations","scope":["entity.name.function","support.function","support.constant.handlebars"],"settings":{"foreground":"#795E26"}},{"name":"Types declaration and references","scope":["meta.return-type","support.class","support.type","entity.name.type","entity.name.class","storage.type.numeric.go","storage.type.byte.go","storage.type.boolean.go","storage.type.string.go","storage.type.uintptr.go","storage.type.error.go","storage.type.rune.go","storage.type.cs","storage.type.generic.cs","storage.type.modifier.cs","storage.type.variable.cs","storage.type.annotation.java","storage.type.generic.java","storage.type.java","storage.type.object.array.java","storage.type.primitive.array.java","storage.type.primitive.java","storage.type.token.java","storage.type.groovy","storage.type.annotation.groovy","storage.type.parameters.groovy","storage.type.generic.groovy","storage.type.object.array.groovy","storage.type.primitive.array.groovy","storage.type.primitive.groovy"],"settings":{"foreground":"#267f99"}},{"name":"Types declaration and references, TS grammar specific","scope":["meta.type.cast.expr","meta.type.new.expr","support.constant.math","support.constant.dom","support.constant.json","entity.other.inherited-class"],"settings":{"foreground":"#267f99"}},{"name":"Control flow keywords","scope":"keyword.control","settings":{"foreground":"#AF00DB"}},{"name":"Variable and parameter name","scope":["variable","meta.definition.variable.name","support.variable","entity.name.variable"],"settings":{"foreground":"#001080"}},{"name":"Object keys, TS grammar specific","scope":["meta.object-literal.key"],"settings":{"foreground":"#001080"}},{"name":"CSS property value","scope":["support.constant.property-value","support.constant.font-name","support.constant.media-type","support.constant.media","constant.other.color.rgb-value","constant.other.rgb-value","support.constant.color"],"settings":{"foreground":"#0451a5"}},{"name":"Regular expression groups","scope":["punctuation.definition.group.regexp","punctuation.definition.group.assertion.regexp","punctuation.definition.character-class.regexp","punctuation.character.set.begin.regexp","punctuation.character.set.end.regexp","keyword.operator.negation.regexp","support.other.parenthesis.regexp"],"settings":{"foreground":"#d16969"}},{"scope":["constant.character.character-class.regexp","constant.other.character-class.set.regexp","constant.other.character-class.regexp","constant.character.set.regexp"],"settings":{"foreground":"#811f3f"}},{"scope":"keyword.operator.quantifier.regexp","settings":{"foreground":"#000000"}},{"scope":["keyword.operator.or.regexp","keyword.control.anchor.regexp"],"settings":{"foreground":"#ff0000"}},{"scope":"constant.character","settings":{"foreground":"#0000ff"}},{"scope":"constant.character.escape","settings":{"foreground":"#ff0000"}},{"scope":"token.info-token","settings":{"foreground":"#316bcd"}},{"scope":"token.warn-token","settings":{"foreground":"#cd9731"}},{"scope":"token.error-token","settings":{"foreground":"#cd3131"}},{"scope":"token.debug-token","settings":{"foreground":"#800080"}}],"extensionData":{"extensionId":"vscode.theme-defaults","extensionPublisher":"vscode","extensionName":"theme-defaults","extensionIsBuiltin":true},"colorMap":{"editor.background":"#ffffff","editor.foreground":"#000000","editor.inactiveSelectionBackground":"#e5ebf1","editorIndentGuide.background":"#d3d3d3","editorIndentGuide.activeBackground":"#939393","editor.selectionHighlightBackground":"#add6ff4d","editorSuggestWidget.background":"#f3f3f3","activityBarBadge.background":"#007acc","sideBarTitle.foreground":"#6f6f6f","list.hoverBackground":"#e8e8e8","input.placeholderForeground":"#767676","settings.textInputBorder":"#cecece","settings.numberInputBorder":"#cecece"}}"##;

const COMMAND_PALETTE_MRU_CACHE: &str = r##"{"usesLRU":true,"entries":[{"key":"revealFileInOS","value":3},{"key":"extension.openInGitHub","value":4},{"key":"workbench.extensions.action.openExtensionsFolder","value":11},{"key":"workbench.action.showRuntimeExtensions","value":14},{"key":"workbench.action.toggleTabsVisibility","value":15},{"key":"extension.liveServerPreview.open","value":16},{"key":"workbench.action.openIssueReporter","value":18},{"key":"workbench.action.openProcessExplorer","value":19},{"key":"workbench.action.toggleSharedProcess","value":20},{"key":"workbench.action.configureLocale","value":21},{"key":"workbench.action.appPerf","value":22},{"key":"workbench.action.reportPerformanceIssueUsingReporter","value":23},{"key":"workbench.action.openGlobalKeybindings","value":25},{"key":"workbench.action.output.toggleOutput","value":27},{"key":"extension.sayHello","value":29}]}"##;

const TEXT_FILE_EDITOR_STATE: &str = r##"{"textEditorViewState":[["file:///Users/dummy/Documents/ticino-playground/play.htm",{"0":{"cursorState":[{"inSelectionMode":false,"selectionStart":{"lineNumber":6,"column":16},"position":{"lineNumber":6,"column":16}}],"viewState":{"scrollLeft":0,"firstPosition":{"lineNumber":1,"column":1},"firstPositionDeltaTop":0},"contributionsState":{"editor.contrib.folding":{},"editor.contrib.wordHighlighter":false}}}],["file:///Users/dummy/Documents/ticino-playground/nakefile.js",{"0":{"cursorState":[{"inSelectionMode":false,"selectionStart":{"lineNumber":7,"column":81},"position":{"lineNumber":7,"column":81}}],"viewState":{"scrollLeft":0,"firstPosition":{"lineNumber":1,"column":1},"firstPositionDeltaTop":20},"contributionsState":{"editor.contrib.folding":{},"editor.contrib.wordHighlighter":false}}}],["file:///Users/dummy/Desktop/vscode2/.gitattributes",{"0":{"cursorState":[{"inSelectionMode":false,"selectionStart":{"lineNumber":9,"column":12},"position":{"lineNumber":9,"column":12}}],"viewState":{"scrollLeft":0,"firstPosition":{"lineNumber":1,"column":1},"firstPositionDeltaTop":20},"contributionsState":{"editor.contrib.folding":{},"editor.contrib.wordHighlighter":false}}}],["file:///Users/dummy/Desktop/vscode2/src/vs/workbench/contrib/search/browser/openAnythingHandler.ts",{"0":{"cursorState":[{"inSelectionMode":false,"selectionStart":{"lineNumber":1,"column":1},"position":{"lineNumber":1,"column":1}}],"viewState":{"scrollLeft":0,"firstPosition":{"lineNumber":1,"column":1},"firstPositionDeltaTop":0},"contributionsState":{"editor.contrib.folding":{},"editor.contrib.wordHighlighter":false}}}]]}"##;

const PINNED_VIEWLETS: &str = r##"[{"id":"workbench.view.explorer","pinned":true,"order":0,"visible":true},{"id":"workbench.view.search","pinned":true,"order":1,"visible":true},{"id":"workbench.view.scm","pinned":true,"order":2,"visible":true},{"id":"workbench.view.debug","pinned":true,"order":3,"visible":true},{"id":"workbench.view.extensions","pinned":true,"order":4,"visible":true},{"id":"workbench.view.extension.gitlens","pinned":true,"order":7,"visible":true},{"id":"workbench.view.extension.test","pinned":false,"visible":false}]"##;

const VERY_LONG_KEY: &str = "very.long.key.very.long.key.very.long.key.very.long.key.very.long.key.very.long.key.very.long.key.very.long.key.very.long.key.very.long.key.very.long.key.very.long.key.very.long.key.very.long.key.very.long.key.very.long.key.very.long.key.very.long.key.very.long.key.very.long.key.very.long.key.very.long.key.very.long.key.very.long.key.very.long.key.very.long.key.very.long.key.very.long.key.very.long.key.very.long.key.very.long.key.very.long.key.very.long.key.very.long.key.very.long.key.";
