//! Porte de `src/vs/platform/userDataSync/test/common/globalStateSync.test.ts` (suíte `GlobalStateSync`), caso a
//! caso e na mesma ordem.
//!
//! Divergências: o `setup` (os dois clientes e o sincronizador de estado global) roda no começo do corpo de cada
//! teste e o `teardown` (`clear` do servidor) no fim; o servidor é um por teste (o `clear` do `teardown` o devolve
//! ao estado inicial no original). `testObject.sync(manifest)` usa os argumentos padrão do original (`preview`
//! falso, configuração e cabeçalhos vazios). `JSON.parse(...).storage` e o `assert.deepStrictEqual` sobre ele são
//! [`assert_storage`], que compara o dicionário sem olhar a ordem das chaves (a do `deepStrictEqual`). O
//! `IUserDataProfileStorageService` e o `IStorageService` do cliente são os campos
//! `user_data_profile_storage_service` e `storage_service`. O `profile` do último teste é, como no original, o perfil
//! criado no `client2` e gravado no armazenamento do `testClient` pelo id.

mod common;

use std::rc::Rc;

use common::files::async_test;
use common::user_data_sync_client::{RecordedRequest, UserDataSyncClient, UserDataSyncTestServer};
use common::{if_match_headers, read_text, sync_synchroniser, write_text};
use indexmap::IndexMap;
use je_base::JsString;
use je_base::lifecycle::DisposableStore;
use je_platform::storage::{StorageScope, StorageTarget};
use je_platform::user_data_profile::user_data_profile_storage_service::IUserDataProfileStorageService;
use je_platform::user_data_profile::{IUserDataProfilesService, UserDataProfile};
use je_platform::user_data_sync::abstract_synchronizer::parse_json;
use je_platform::user_data_sync::global_state_sync::{GlobalStateSynchroniser, parse_global_state};
use je_platform::user_data_sync::user_data_sync::{IGlobalState, IStorageValue, ISyncData, IUserDataSyncStoreService, IUserDataSynchroniser, SyncResource, SyncStatus};
use je_storage::StorageValue;

struct Fixture {
    server: Rc<UserDataSyncTestServer>,
    test_client: Rc<UserDataSyncClient>,
    client2: Rc<UserDataSyncClient>,
    test_object: Rc<GlobalStateSynchroniser>,
}

/// O `setup` da suíte.
async fn setup(disposables: &Rc<DisposableStore>) -> Fixture {
    let server = Rc::new(UserDataSyncTestServer::new());
    let test_client = disposables.add_rc(UserDataSyncClient::set_up(server.clone(), true).await.unwrap());
    let test_object = test_client.get_synchronizer(SyncResource::GlobalState).into_global_state().expect("o sincronizador de estado global está ativo");

    let client2 = disposables.add_rc(UserDataSyncClient::set_up(server.clone(), true).await.unwrap());
    Fixture { server, test_client, client2, test_object }
}

/// O `teardown` da suíte.
async fn teardown(fixture: &Fixture) {
    fixture.test_client.user_data_sync_store_service.clear().await.unwrap();
}

/// `testObject.sync(await testClient.getLatestRef(SyncResource.GlobalState))`.
async fn sync_latest(fixture: &Fixture) {
    let manifest = fixture.test_client.get_latest_ref(SyncResource::GlobalState).await.unwrap();
    sync_synchroniser(&*fixture.test_object.base.clone(), manifest, false).await.unwrap();
}

/// `parseGlobalState(content)`: o estado global do `syncData.content` do conteúdo lido do servidor.
fn parse_global_state_of(content: &str) -> IGlobalState {
    let sync_data = ISyncData::from_json(&parse_json(content).unwrap()).expect("é um ISyncData");
    parse_global_state(&sync_data.content).unwrap()
}

/// `testClient.read(testObject.resource, collection?)` e `parseGlobalState` do `content`.
async fn read_global_state(client: &UserDataSyncClient, collection: Option<&str>) -> IGlobalState {
    let user_data = client.read(SyncResource::GlobalState, collection).await.unwrap();
    parse_global_state_of(&user_data.content.expect("o conteúdo existe"))
}

/// `assert.deepStrictEqual(actual.storage, { chave: { version: 1, value, scope? } })`, sem olhar a ordem das chaves.
#[track_caller]
fn assert_storage(actual: &IGlobalState, expected: &[(&str, &str, Option<StorageScope>)]) {
    assert_eq!(actual.storage.len(), expected.len(), "chaves de armazenamento diferentes");
    for (key, value, scope) in expected {
        assert_eq!(actual.storage.get(&JsString::from(*key)), Some(&IStorageValue { version: 1.0, value: JsString::from(*value), scope: *scope }), "chave {key}");
    }
}

/// `updateLocale(client)`.
async fn update_locale(client: &UserDataSyncClient) {
    write_text(&client.file_service, &client.environment_service.argv_resource, r#"{"locale":"en"}"#).await.unwrap();
}

/// `updateUserStorage(key, value, client)`.
fn update_user_storage(key: &str, value: &str, client: &UserDataSyncClient) {
    client.storage_service.store(key, StorageValue::from(value), StorageScope::Profile, StorageTarget::User);
}

/// `updateUserStorageForProfile(key, value, profile, client)`.
async fn update_user_storage_for_profile(key: &str, value: &str, profile: &UserDataProfile, client: &UserDataSyncClient) {
    let data: IndexMap<String, Option<String>> = IndexMap::from([(key.to_string(), Some(value.to_string()))]);
    client.user_data_profile_storage_service.update_storage_data(profile, data, StorageTarget::User, None).await.unwrap();
}

/// `updateMachineStorage(key, value, client)`.
fn update_machine_storage(key: &str, value: &str, client: &UserDataSyncClient) {
    client.storage_service.store(key, StorageValue::from(value), StorageScope::Profile, StorageTarget::Machine);
}

/// `removeStorage(key, client)`.
fn remove_storage(key: &str, client: &UserDataSyncClient) {
    client.storage_service.remove(key, StorageScope::Profile);
}

/// `readStorage(key, client)`.
fn read_storage(key: &str, client: &UserDataSyncClient) -> Option<String> {
    client.storage_service.get(key, StorageScope::Profile, None)
}

/// `readLocale(client)`.
async fn read_locale(client: &UserDataSyncClient) -> Option<String> {
    let content = read_text(&client.file_service, &client.environment_service.argv_resource).await;
    match parse_json(&content).unwrap() {
        je_base::json::Value::Object(object) => match object.get("locale") {
            Some(je_base::json::Value::String(locale)) => Some(locale.to_string_lossy()),
            _ => None,
        },
        other => panic!("o argv não é um objeto: {other:?}"),
    }
}

#[test]
fn when_global_state_does_not_exist() {
    async_test(|d| async move {
        let fixture = setup(&d).await;
        let (server, test_client, test_object) = (&fixture.server, &fixture.test_client, &fixture.test_object);

        assert!(test_object.get_last_sync_user_data().await.unwrap().is_none());
        let mut manifest = test_client.get_latest_ref(SyncResource::GlobalState).await.unwrap();
        server.reset();
        sync_synchroniser(&*test_object.base.clone(), manifest, false).await.unwrap();

        assert_eq!(server.requests(), Vec::<RecordedRequest>::new());

        let last_sync_user_data = test_object.get_last_sync_user_data().await.unwrap().expect("há dados da última sincronização");
        let remote_user_data = test_object.get_remote_user_data(None).await.unwrap();
        assert_eq!(last_sync_user_data.r#ref, remote_user_data.r#ref);
        assert_eq!(last_sync_user_data.sync_data, remote_user_data.sync_data);
        assert_eq!(last_sync_user_data.sync_data, None);

        manifest = test_client.get_latest_ref(SyncResource::GlobalState).await.unwrap();
        server.reset();
        sync_synchroniser(&*test_object.base.clone(), manifest, false).await.unwrap();
        assert_eq!(server.requests(), Vec::<RecordedRequest>::new());

        manifest = test_client.get_latest_ref(SyncResource::GlobalState).await.unwrap();
        server.reset();
        sync_synchroniser(&*test_object.base.clone(), manifest, false).await.unwrap();
        assert_eq!(server.requests(), Vec::<RecordedRequest>::new());
        teardown(&fixture).await;
    });
}

#[test]
fn when_global_state_is_created_after_first_sync() {
    async_test(|d| async move {
        let fixture = setup(&d).await;
        let (server, test_client, test_object) = (&fixture.server, &fixture.test_client, &fixture.test_object);
        sync_latest(&fixture).await;
        update_user_storage("a", "value1", test_client);

        let mut last_sync_user_data = test_object.get_last_sync_user_data().await.unwrap();
        let manifest = test_client.get_latest_ref(SyncResource::GlobalState).await.unwrap();
        server.reset();
        sync_synchroniser(&*test_object.base.clone(), manifest, false).await.unwrap();

        assert_eq!(
            server.requests(),
            vec![RecordedRequest {
                r#type: "POST".to_string(),
                url: format!("{}/v1/resource/{}", server.url, test_object.resource.as_str()),
                headers: if_match_headers(&last_sync_user_data.as_ref().expect("há dados da última sincronização").r#ref),
            }]
        );

        last_sync_user_data = test_object.get_last_sync_user_data().await.unwrap();
        let remote_user_data = test_object.get_remote_user_data(None).await.unwrap();
        let last_sync_user_data = last_sync_user_data.expect("há dados da última sincronização");
        assert_eq!(last_sync_user_data.r#ref, remote_user_data.r#ref);
        assert_eq!(last_sync_user_data.sync_data, remote_user_data.sync_data);
        let last_sync_global_state = parse_global_state(&last_sync_user_data.sync_data.as_ref().expect("há dados sincronizados").content).unwrap();
        assert_storage(&last_sync_global_state, &[("a", "value1", Some(StorageScope::Profile))]);
        teardown(&fixture).await;
    });
}

#[test]
fn first_time_sync_outgoing_to_server_no_state() {
    async_test(|d| async move {
        let fixture = setup(&d).await;
        let (test_client, test_object) = (&fixture.test_client, &fixture.test_object);
        update_user_storage("a", "value1", test_client);
        update_machine_storage("b", "value1", test_client);
        update_locale(test_client).await;

        sync_latest(&fixture).await;
        assert_eq!(test_object.base.status(), SyncStatus::Idle);
        assert!(test_object.base.conflicts().conflicts.is_empty());

        let actual = read_global_state(test_client, None).await;
        assert_storage(&actual, &[("globalState.argv.locale", "en", None), ("a", "value1", Some(StorageScope::Profile))]);
        teardown(&fixture).await;
    });
}

#[test]
fn first_time_sync_incoming_from_server_no_state() {
    async_test(|d| async move {
        let fixture = setup(&d).await;
        let (test_client, client2, test_object) = (&fixture.test_client, &fixture.client2, &fixture.test_object);
        update_user_storage("a", "value1", client2);
        update_locale(client2).await;
        client2.sync().await.unwrap();

        sync_latest(&fixture).await;
        assert_eq!(test_object.base.status(), SyncStatus::Idle);
        assert!(test_object.base.conflicts().conflicts.is_empty());

        assert_eq!(read_storage("a", test_client), Some("value1".to_string()));
        assert_eq!(read_locale(test_client).await, Some("en".to_string()));
        teardown(&fixture).await;
    });
}

#[test]
fn first_time_sync_when_storage_exists() {
    async_test(|d| async move {
        let fixture = setup(&d).await;
        let (test_client, client2, test_object) = (&fixture.test_client, &fixture.client2, &fixture.test_object);
        update_user_storage("a", "value1", client2);
        client2.sync().await.unwrap();

        update_user_storage("b", "value2", test_client);
        sync_latest(&fixture).await;
        assert_eq!(test_object.base.status(), SyncStatus::Idle);
        assert!(test_object.base.conflicts().conflicts.is_empty());

        assert_eq!(read_storage("a", test_client), Some("value1".to_string()));
        assert_eq!(read_storage("b", test_client), Some("value2".to_string()));

        let actual = read_global_state(test_client, None).await;
        assert_storage(&actual, &[("a", "value1", Some(StorageScope::Profile)), ("b", "value2", Some(StorageScope::Profile))]);
        teardown(&fixture).await;
    });
}

#[test]
fn first_time_sync_when_storage_exists_has_conflicts() {
    async_test(|d| async move {
        let fixture = setup(&d).await;
        let (test_client, client2, test_object) = (&fixture.test_client, &fixture.client2, &fixture.test_object);
        update_user_storage("a", "value1", client2);
        client2.sync().await.unwrap();

        update_user_storage("a", "value2", client2);
        sync_latest(&fixture).await;

        assert_eq!(test_object.base.status(), SyncStatus::Idle);
        assert!(test_object.base.conflicts().conflicts.is_empty());

        assert_eq!(read_storage("a", test_client), Some("value1".to_string()));

        let actual = read_global_state(test_client, None).await;
        assert_storage(&actual, &[("a", "value1", Some(StorageScope::Profile))]);
        teardown(&fixture).await;
    });
}

#[test]
fn sync_adding_a_storage_value() {
    async_test(|d| async move {
        let fixture = setup(&d).await;
        let (test_client, test_object) = (&fixture.test_client, &fixture.test_object);
        update_user_storage("a", "value1", test_client);
        sync_latest(&fixture).await;

        update_user_storage("b", "value2", test_client);
        sync_latest(&fixture).await;
        assert_eq!(test_object.base.status(), SyncStatus::Idle);
        assert!(test_object.base.conflicts().conflicts.is_empty());

        assert_eq!(read_storage("a", test_client), Some("value1".to_string()));
        assert_eq!(read_storage("b", test_client), Some("value2".to_string()));

        let actual = read_global_state(test_client, None).await;
        assert_storage(&actual, &[("a", "value1", Some(StorageScope::Profile)), ("b", "value2", Some(StorageScope::Profile))]);
        teardown(&fixture).await;
    });
}

#[test]
fn sync_updating_a_storage_value() {
    async_test(|d| async move {
        let fixture = setup(&d).await;
        let (test_client, test_object) = (&fixture.test_client, &fixture.test_object);
        update_user_storage("a", "value1", test_client);
        sync_latest(&fixture).await;

        update_user_storage("a", "value2", test_client);
        sync_latest(&fixture).await;
        assert_eq!(test_object.base.status(), SyncStatus::Idle);
        assert!(test_object.base.conflicts().conflicts.is_empty());

        assert_eq!(read_storage("a", test_client), Some("value2".to_string()));

        let actual = read_global_state(test_client, None).await;
        assert_storage(&actual, &[("a", "value2", Some(StorageScope::Profile))]);
        teardown(&fixture).await;
    });
}

#[test]
fn sync_removing_a_storage_value() {
    async_test(|d| async move {
        let fixture = setup(&d).await;
        let (test_client, test_object) = (&fixture.test_client, &fixture.test_object);
        update_user_storage("a", "value1", test_client);
        update_user_storage("b", "value2", test_client);
        sync_latest(&fixture).await;

        remove_storage("b", test_client);
        sync_latest(&fixture).await;
        assert_eq!(test_object.base.status(), SyncStatus::Idle);
        assert!(test_object.base.conflicts().conflicts.is_empty());

        assert_eq!(read_storage("a", test_client), Some("value1".to_string()));
        assert_eq!(read_storage("b", test_client), None);

        let actual = read_global_state(test_client, None).await;
        assert_storage(&actual, &[("a", "value1", Some(StorageScope::Profile))]);
        teardown(&fixture).await;
    });
}

#[test]
fn sync_profile_state() {
    async_test(|d| async move {
        let fixture = setup(&d).await;
        let test_client = &fixture.test_client;
        let client2 = disposables_client(&d, &fixture.server).await;
        let profile = client2.user_data_profiles_service.create_named_profile("profile1", None, None).await.unwrap();
        update_locale(&client2).await;
        update_user_storage_for_profile("a", "value1", &profile, test_client).await;
        client2.sync().await.unwrap();

        test_client.sync().await.unwrap();

        let synced_profile = test_client.user_data_profiles_service.profiles().into_iter().find(|p| p.id == profile.id).expect("o perfil foi sincronizado");
        let profile_storage = test_client.user_data_profile_storage_service.read_storage_data(&synced_profile).await.unwrap();
        assert_eq!(profile_storage.get("a").and_then(|value| value.value.clone()), Some("value1".to_string()));
        assert_eq!(read_locale(test_client).await, Some("en".to_string()));

        let actual = read_global_state(test_client, Some("1")).await;
        assert_storage(&actual, &[("a", "value1", Some(StorageScope::Profile))]);
        teardown(&fixture).await;
    });
}

/// `const client2 = disposableStore.add(new UserDataSyncClient(server)); await client2.setUp(true)`.
async fn disposables_client(disposables: &Rc<DisposableStore>, server: &Rc<UserDataSyncTestServer>) -> Rc<UserDataSyncClient> {
    disposables.add_rc(UserDataSyncClient::set_up(server.clone(), true).await.unwrap())
}
