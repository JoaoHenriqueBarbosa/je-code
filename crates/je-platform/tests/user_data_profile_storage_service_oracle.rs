//! Porte de `src/vs/platform/userDataProfile/test/common/userDataProfileStorageService.test.ts` (suíte
//! `ProfileStorageService`, 5 casos, na ordem do original).
//!
//! Divergências: o `TestStorageDatabase` e o `TestUserDataProfileStorageService` (o backend com um banco por perfil)
//! são os de `common/user_data_sync_client.rs`. O `setup` roda no começo do corpo de cada teste. `new Map<string,
//! string | undefined>()` é um `IndexMap<String, Option<String>>`. `storage.items.size` é `storage.size()`, e o
//! `deepStrictEqual` do `loadKeyTargets` compara o dicionário chave a chave (o objeto do original não olha a ordem).

mod common;

use std::rc::Rc;

use common::files::async_test;
use common::user_data_sync_client::{TestStorageDatabase, TestUserDataProfileStorageBackend};
use indexmap::IndexMap;
use je_base::path::{FixedEnvironment, Platform};
use je_base::resources::ExtUri;
use je_base::uri::URI;
use je_platform::storage::{InMemoryStorageService, StorageScope, StorageService, StorageTarget, TARGET_KEY, load_key_targets};
use je_platform::user_data_profile::user_data_profile_storage_service::{AbstractUserDataProfileStorageService, IUserDataProfileStorageService, ProfileStorageValue};
use je_platform::user_data_profile::{UserDataProfile, to_user_data_profile};
use je_storage::{IStorage, Storage, StorageOptions, StorageValue};

struct Fixture {
    test_object: Rc<AbstractUserDataProfileStorageService>,
    profile: UserDataProfile,
    storage: Rc<Storage>,
}

fn file(path: &str) -> URI {
    URI::file(path, Platform::Posix)
}

/// O `setup` da suíte.
async fn setup(disposables: &Rc<je_base::lifecycle::DisposableStore>) -> Fixture {
    let ext_uri = ExtUri::unbiased(Rc::new(FixedEnvironment::new(Platform::Posix, "/")));
    let profile = to_user_data_profile("test", "test", &file("foo"), &file("cache"), None, None, &ext_uri);

    let backend = Rc::new(TestUserDataProfileStorageBackend::default());
    let in_memory = disposables.add_rc(InMemoryStorageService::new());
    let storage_service: Rc<dyn StorageService> = Rc::new(in_memory);
    let test_object = disposables.add_rc(AbstractUserDataProfileStorageService::new(false, storage_service, backend.clone()));

    let database: Rc<TestStorageDatabase> = backend.setup_storage_database(&profile);
    let storage = disposables.add_rc(Storage::new(database, StorageOptions::default()));
    storage.init().await.unwrap();
    Fixture { test_object, profile, storage }
}

/// `JSON.stringify({ chave: StorageTarget, ... })`.
fn targets_json(entries: &[(&str, StorageTarget)]) -> String {
    let parts: Vec<String> = entries.iter().map(|(key, target)| format!("\"{key}\":{}", *target as i32)).collect();
    format!("{{{}}}", parts.join(","))
}

/// `assert.deepStrictEqual(loadKeyTargets(storage), { ... })`.
#[track_caller]
fn assert_key_targets(storage: &Storage, expected: &[(&str, StorageTarget)]) {
    let actual = load_key_targets(storage);
    assert_eq!(actual.len(), expected.len(), "alvos diferentes");
    for (key, target) in expected {
        assert_eq!(actual.get(&key.to_string()), Some(target), "chave {key}");
    }
}

#[test]
fn read_empty_storage() {
    async_test(|disposables| async move {
        let fixture = setup(&disposables).await;

        let actual = fixture.test_object.read_storage_data(&fixture.profile).await.unwrap();

        assert_eq!(actual.len(), 0);
    });
}

#[test]
fn read_storage_with_data() {
    async_test(|disposables| async move {
        let fixture = setup(&disposables).await;
        fixture.storage.set("foo", StorageValue::from("bar"), false);
        fixture.storage.set(TARGET_KEY, StorageValue::from(targets_json(&[("foo", StorageTarget::User)]).as_str()), false);
        fixture.storage.flush(None).await.unwrap();

        let actual = fixture.test_object.read_storage_data(&fixture.profile).await.unwrap();

        assert_eq!(actual.len(), 1);
        assert_eq!(actual.get("foo"), Some(&ProfileStorageValue { value: Some("bar".to_string()), target: StorageTarget::User, scope: Some(StorageScope::Profile) }));
    });
}

#[test]
fn write_in_empty_storage() {
    async_test(|disposables| async move {
        let fixture = setup(&disposables).await;
        let mut data = IndexMap::new();
        data.insert("foo".to_string(), Some("bar".to_string()));
        fixture.test_object.update_storage_data(&fixture.profile, data, StorageTarget::User, None).await.unwrap();

        assert_eq!(fixture.storage.size(), 2);
        assert_key_targets(&fixture.storage, &[("foo", StorageTarget::User)]);
        assert_eq!(fixture.storage.get("foo", None), Some("bar".to_string()));
    });
}

#[test]
fn write_in_storage_with_data() {
    async_test(|disposables| async move {
        let fixture = setup(&disposables).await;
        fixture.storage.set("foo", StorageValue::from("bar"), false);
        fixture.storage.set(TARGET_KEY, StorageValue::from(targets_json(&[("foo", StorageTarget::User)]).as_str()), false);
        fixture.storage.flush(None).await.unwrap();

        let mut data = IndexMap::new();
        data.insert("abc".to_string(), Some("xyz".to_string()));
        fixture.test_object.update_storage_data(&fixture.profile, data, StorageTarget::Machine, None).await.unwrap();

        assert_eq!(fixture.storage.size(), 3);
        assert_key_targets(&fixture.storage, &[("foo", StorageTarget::User), ("abc", StorageTarget::Machine)]);
        assert_eq!(fixture.storage.get("foo", None), Some("bar".to_string()));
        assert_eq!(fixture.storage.get("abc", None), Some("xyz".to_string()));
    });
}

#[test]
fn write_in_storage_with_data_insert_update_remove() {
    async_test(|disposables| async move {
        let fixture = setup(&disposables).await;
        fixture.storage.set("foo", StorageValue::from("bar"), false);
        fixture.storage.set("abc", StorageValue::from("xyz"), false);
        fixture.storage.set(TARGET_KEY, StorageValue::from(targets_json(&[("foo", StorageTarget::User), ("abc", StorageTarget::Machine)]).as_str()), false);
        fixture.storage.flush(None).await.unwrap();

        let mut data: IndexMap<String, Option<String>> = IndexMap::new();
        data.insert("foo".to_string(), None);
        data.insert("abc".to_string(), Some("def".to_string()));
        data.insert("var".to_string(), Some("const".to_string()));
        fixture.test_object.update_storage_data(&fixture.profile, data, StorageTarget::User, None).await.unwrap();

        assert_eq!(fixture.storage.size(), 3);
        assert_key_targets(&fixture.storage, &[("abc", StorageTarget::User), ("var", StorageTarget::User)]);
        assert_eq!(fixture.storage.get("abc", None), Some("def".to_string()));
        assert_eq!(fixture.storage.get("var", None), Some("const".to_string()));
    });
}
