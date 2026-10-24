//! Porte de `src/vs/platform/userDataSync/test/common/userDataProfilesManifestSync.test.ts` (suíte
//! `UserDataProfilesManifestSync`), caso a caso e na mesma ordem.
//!
//! Divergências: o `setup` (os dois clientes e o sincronizador de perfis) roda no começo do corpo de cada teste,
//! dentro do `runWithFakedTimers`, e o `teardown` (`clear` do servidor) no fim; o servidor é um por teste (o do
//! original é um por suíte, mas o `clear` do `teardown` o devolve ao estado inicial). O
//! `testObject.sync(manifest)` usa os argumentos padrão do original (`preview` falso, configuração e cabeçalhos
//! vazios). `JSON.parse` do conteúdo dos perfis passa por [`parse_user_data_profiles_manifest`], que lê só as
//! chaves de [`ISyncUserDataProfile`] e escreve na ordem `id`, `name`, `collection`; os `deepStrictEqual` de
//! `useDefaultFlags` comparam o `UseDefaultProfileFlags` com só as chaves informadas. `removeProfile` e
//! `updateProfile` sem `await` do original ficam sem `await` (a promessa roda até o primeiro `await`).

mod common;

use std::rc::Rc;

use common::files::async_test;
use common::user_data_sync_client::{RecordedRequest, UserDataSyncClient, UserDataSyncTestServer};
use je_base::collator::locale_compare;
use je_base::lifecycle::DisposableStore;
use je_base::parts::request::{HeaderValue, IHeaders};
use je_platform::user_data_profile::{IUserDataProfilesService, UseDefaultProfileFlags, UserDataProfileOptions, UserDataProfileUpdateOptions};
use je_platform::user_data_sync::abstract_synchronizer::parse_json;
use je_platform::user_data_sync::user_data_profiles_manifest_sync::{UserDataProfilesManifestSynchroniser, parse_user_data_profiles_manifest};
use je_platform::user_data_sync::user_data_sync::{ISyncData, ISyncUserDataProfile, IUserDataSyncConfiguration, IUserDataSyncStoreService, IUserDataSynchroniser, RefOrUserData, SyncResource, SyncStatus};

struct Fixture {
    server: Rc<UserDataSyncTestServer>,
    test_client: Rc<UserDataSyncClient>,
    client2: Rc<UserDataSyncClient>,
    test_object: Rc<UserDataProfilesManifestSynchroniser>,
}

/// O `setup` da suíte.
async fn setup(disposables: &Rc<DisposableStore>) -> Fixture {
    let server = Rc::new(UserDataSyncTestServer::new());
    let test_client = disposables.add_rc(UserDataSyncClient::set_up(server.clone(), true).await.unwrap());
    let test_object = test_client.get_synchronizer(SyncResource::Profiles).into_profiles_manifest().expect("o sincronizador de perfis está ativo");

    let client2 = disposables.add_rc(UserDataSyncClient::set_up(server.clone(), true).await.unwrap());
    Fixture { server, test_client, client2, test_object }
}

/// O `teardown` da suíte.
async fn teardown(fixture: &Fixture) {
    fixture.test_client.user_data_sync_store_service.clear().await.unwrap();
}

/// `testObject.sync(manifest)`.
async fn sync(test_object: &UserDataProfilesManifestSynchroniser, manifest: Option<String>) {
    test_object.sync(manifest.map(RefOrUserData::Ref), false, IUserDataSyncConfiguration::default(), IHeaders::new()).await.unwrap();
}

/// `{ 'If-Match': ref }`.
fn if_match(reference: &str) -> IHeaders {
    IHeaders::from([("If-Match".to_string(), HeaderValue::One(reference.to_string()))])
}

fn remote_profile(id: &str, name: &str, collection: &str, use_default_flags: Option<UseDefaultProfileFlags>) -> ISyncUserDataProfile {
    ISyncUserDataProfile { id: id.to_string(), collection: collection.to_string(), name: name.to_string(), icon: None, use_default_flags }
}

fn keybindings_flags() -> UseDefaultProfileFlags {
    UseDefaultProfileFlags { keybindings: Some(true), ..UseDefaultProfileFlags::default() }
}

/// `{ id, name, useDefaultFlags }` de `getLocalProfiles`.
#[derive(Debug, PartialEq)]
struct LocalProfile {
    id: String,
    name: String,
    use_default_flags: Option<UseDefaultProfileFlags>,
}

fn local_profile(id: &str, name: &str, use_default_flags: Option<UseDefaultProfileFlags>) -> LocalProfile {
    LocalProfile { id: id.to_string(), name: name.to_string(), use_default_flags }
}

/// `parseRemoteProfiles(content)`: `JSON.parse(JSON.parse(content).content)`.
fn parse_remote_profiles(content: &str) -> Vec<ISyncUserDataProfile> {
    let sync_data = ISyncData::from_json(&parse_json(content).unwrap()).expect("o conteúdo é um ISyncData");
    parse_user_data_profiles_manifest(&sync_data).unwrap()
}

/// `getLocalProfiles(client)`: os perfis menos o padrão, ordenados pelo nome.
fn get_local_profiles(client: &UserDataSyncClient) -> Vec<LocalProfile> {
    let mut profiles: Vec<_> = client.user_data_profiles_service.profiles().into_iter().skip(1).collect();
    profiles.sort_by(|a, b| locale_compare(&a.name, &b.name));
    profiles.into_iter().map(|profile| LocalProfile { id: profile.id, name: profile.name, use_default_flags: profile.use_default_flags }).collect()
}

#[test]
fn when_profiles_does_not_exist() {
    async_test(|d| async move {
        let fixture = setup(&d).await;
        let (server, test_client, test_object) = (&fixture.server, &fixture.test_client, &fixture.test_object);
        assert!(test_object.get_last_sync_user_data().await.unwrap().is_none());
        let mut manifest = test_client.get_latest_ref(SyncResource::Profiles).await.unwrap();
        server.reset();
        sync(test_object, manifest).await;

        assert_eq!(server.requests(), Vec::<RecordedRequest>::new());

        let last_sync_user_data = test_object.get_last_sync_user_data().await.unwrap().expect("há dados da última sincronização");
        let remote_user_data = test_object.get_remote_user_data(None).await.unwrap();
        assert_eq!(last_sync_user_data.r#ref, remote_user_data.r#ref);
        assert_eq!(last_sync_user_data.sync_data, remote_user_data.sync_data);
        assert_eq!(last_sync_user_data.sync_data, None);

        manifest = test_client.get_latest_ref(SyncResource::Profiles).await.unwrap();
        server.reset();
        sync(test_object, manifest).await;
        assert_eq!(server.requests(), Vec::<RecordedRequest>::new());

        manifest = test_client.get_latest_ref(SyncResource::Profiles).await.unwrap();
        server.reset();
        sync(test_object, manifest).await;
        assert_eq!(server.requests(), Vec::<RecordedRequest>::new());
        teardown(&fixture).await;
    });
}

#[test]
fn when_profile_is_created_after_first_sync() {
    async_test(|d| async move {
        let fixture = setup(&d).await;
        let (server, test_client, test_object) = (&fixture.server, &fixture.test_client, &fixture.test_object);
        sync(test_object, test_client.get_latest_ref(SyncResource::Profiles).await.unwrap()).await;
        test_client.user_data_profiles_service.create_profile("1", "1", None, None).await.unwrap();

        let mut last_sync_user_data = test_object.get_last_sync_user_data().await.unwrap();
        let manifest = test_client.get_latest_ref(SyncResource::Profiles).await.unwrap();
        server.reset();
        sync(test_object, manifest).await;

        assert_eq!(
            server.requests(),
            vec![
                RecordedRequest { r#type: "POST".to_string(), url: format!("{}/v1/collection", server.url), headers: IHeaders::new() },
                RecordedRequest {
                    r#type: "POST".to_string(),
                    url: format!("{}/v1/resource/{}", server.url, test_object.resource.as_str()),
                    headers: if_match(&last_sync_user_data.as_ref().expect("há dados da última sincronização").r#ref),
                },
            ]
        );

        last_sync_user_data = test_object.get_last_sync_user_data().await.unwrap();
        let remote_user_data = test_object.get_remote_user_data(None).await.unwrap();
        let last_sync_user_data = last_sync_user_data.expect("há dados da última sincronização");
        assert_eq!(last_sync_user_data.r#ref, remote_user_data.r#ref);
        assert_eq!(last_sync_user_data.sync_data, remote_user_data.sync_data);
        let sync_data = last_sync_user_data.sync_data.expect("há dados sincronizados");
        assert_eq!(parse_user_data_profiles_manifest(&sync_data).unwrap(), vec![remote_profile("1", "1", "1", None)]);
        teardown(&fixture).await;
    });
}

#[test]
fn first_time_sync_outgoing_to_server_no_state() {
    async_test(|d| async move {
        let fixture = setup(&d).await;
        let (test_client, test_object) = (&fixture.test_client, &fixture.test_object);
        test_client.user_data_profiles_service.create_profile("1", "1", None, None).await.unwrap();

        sync(test_object, test_client.get_latest_ref(SyncResource::Profiles).await.unwrap()).await;
        assert_eq!(test_object.status(), SyncStatus::Idle);
        assert!(test_object.conflicts().conflicts.is_empty());

        let content = test_client.read(test_object.resource, None).await.unwrap().content.expect("o servidor tem o conteúdo");
        assert_eq!(parse_remote_profiles(&content), vec![remote_profile("1", "1", "1", None)]);
        teardown(&fixture).await;
    });
}

#[test]
fn first_time_sync_incoming_from_server_no_state() {
    async_test(|d| async move {
        let fixture = setup(&d).await;
        let (test_client, client2, test_object) = (&fixture.test_client, &fixture.client2, &fixture.test_object);
        client2.user_data_profiles_service.create_profile("1", "name 1", None, None).await.unwrap();
        client2.sync().await.unwrap();

        sync(test_object, test_client.get_latest_ref(SyncResource::Profiles).await.unwrap()).await;
        assert_eq!(test_object.status(), SyncStatus::Idle);
        assert!(test_object.conflicts().conflicts.is_empty());

        assert_eq!(get_local_profiles(test_client), vec![local_profile("1", "name 1", None)]);
        teardown(&fixture).await;
    });
}

#[test]
fn first_time_sync_when_profiles_exists() {
    async_test(|d| async move {
        let fixture = setup(&d).await;
        let (test_client, client2, test_object) = (&fixture.test_client, &fixture.client2, &fixture.test_object);
        client2.user_data_profiles_service.create_profile("1", "name 1", None, None).await.unwrap();
        client2.sync().await.unwrap();

        test_client.user_data_profiles_service.create_profile("2", "name 2", None, None).await.unwrap();
        sync(test_object, test_client.get_latest_ref(SyncResource::Profiles).await.unwrap()).await;
        assert_eq!(test_object.status(), SyncStatus::Idle);
        assert!(test_object.conflicts().conflicts.is_empty());

        assert_eq!(get_local_profiles(test_client), vec![local_profile("1", "name 1", None), local_profile("2", "name 2", None)]);

        let content = test_client.read(test_object.resource, None).await.unwrap().content.expect("o servidor tem o conteúdo");
        assert_eq!(parse_remote_profiles(&content), vec![remote_profile("1", "name 1", "1", None), remote_profile("2", "name 2", "2", None)]);
        teardown(&fixture).await;
    });
}

#[test]
fn first_time_sync_when_storage_exists_has_conflicts() {
    async_test(|d| async move {
        let fixture = setup(&d).await;
        let (test_client, client2, test_object) = (&fixture.test_client, &fixture.client2, &fixture.test_object);
        client2.user_data_profiles_service.create_profile("1", "name 1", None, None).await.unwrap();
        client2.sync().await.unwrap();

        test_client.user_data_profiles_service.create_profile("1", "name 2", None, None).await.unwrap();
        sync(test_object, test_client.get_latest_ref(SyncResource::Profiles).await.unwrap()).await;

        assert_eq!(test_object.status(), SyncStatus::Idle);
        assert!(test_object.conflicts().conflicts.is_empty());

        assert_eq!(get_local_profiles(test_client), vec![local_profile("1", "name 1", None)]);

        let content = test_client.read(test_object.resource, None).await.unwrap().content.expect("o servidor tem o conteúdo");
        assert_eq!(parse_remote_profiles(&content), vec![remote_profile("1", "name 1", "1", None)]);
        teardown(&fixture).await;
    });
}

#[test]
fn sync_adding_a_profile() {
    async_test(|d| async move {
        let fixture = setup(&d).await;
        let (test_client, client2, test_object) = (&fixture.test_client, &fixture.client2, &fixture.test_object);
        test_client.user_data_profiles_service.create_profile("1", "name 1", None, None).await.unwrap();
        sync(test_object, test_client.get_latest_ref(SyncResource::Profiles).await.unwrap()).await;
        client2.sync().await.unwrap();

        test_client.user_data_profiles_service.create_profile("2", "name 2", None, None).await.unwrap();
        sync(test_object, test_client.get_latest_ref(SyncResource::Profiles).await.unwrap()).await;
        assert_eq!(test_object.status(), SyncStatus::Idle);
        assert!(test_object.conflicts().conflicts.is_empty());
        assert_eq!(get_local_profiles(test_client), vec![local_profile("1", "name 1", None), local_profile("2", "name 2", None)]);

        client2.sync().await.unwrap();
        assert_eq!(get_local_profiles(client2), vec![local_profile("1", "name 1", None), local_profile("2", "name 2", None)]);

        let content = test_client.read(test_object.resource, None).await.unwrap().content.expect("o servidor tem o conteúdo");
        assert_eq!(parse_remote_profiles(&content), vec![remote_profile("1", "name 1", "1", None), remote_profile("2", "name 2", "2", None)]);
        teardown(&fixture).await;
    });
}

#[test]
fn sync_updating_a_profile() {
    async_test(|d| async move {
        let fixture = setup(&d).await;
        let (test_client, client2, test_object) = (&fixture.test_client, &fixture.client2, &fixture.test_object);
        let profile = test_client.user_data_profiles_service.create_profile("1", "name 1", None, None).await.unwrap();
        sync(test_object, test_client.get_latest_ref(SyncResource::Profiles).await.unwrap()).await;
        client2.sync().await.unwrap();

        test_client.user_data_profiles_service.update_profile(&profile, UserDataProfileUpdateOptions { name: Some("name 2".to_string()), ..UserDataProfileUpdateOptions::default() }).await.unwrap();
        sync(test_object, test_client.get_latest_ref(SyncResource::Profiles).await.unwrap()).await;
        assert_eq!(test_object.status(), SyncStatus::Idle);
        assert!(test_object.conflicts().conflicts.is_empty());
        assert_eq!(get_local_profiles(test_client), vec![local_profile("1", "name 2", None)]);

        client2.sync().await.unwrap();
        assert_eq!(get_local_profiles(client2), vec![local_profile("1", "name 2", None)]);

        let content = test_client.read(test_object.resource, None).await.unwrap().content.expect("o servidor tem o conteúdo");
        assert_eq!(parse_remote_profiles(&content), vec![remote_profile("1", "name 2", "1", None)]);
        teardown(&fixture).await;
    });
}

#[test]
fn sync_removing_a_profile() {
    async_test(|d| async move {
        let fixture = setup(&d).await;
        let (test_client, client2, test_object) = (&fixture.test_client, &fixture.client2, &fixture.test_object);
        let profile = test_client.user_data_profiles_service.create_profile("1", "name 1", None, None).await.unwrap();
        test_client.user_data_profiles_service.create_profile("2", "name 2", None, None).await.unwrap();
        sync(test_object, test_client.get_latest_ref(SyncResource::Profiles).await.unwrap()).await;
        client2.sync().await.unwrap();

        let _removal = test_client.user_data_profiles_service.remove_profile(&profile);
        sync(test_object, test_client.get_latest_ref(SyncResource::Profiles).await.unwrap()).await;
        assert_eq!(test_object.status(), SyncStatus::Idle);
        assert!(test_object.conflicts().conflicts.is_empty());
        assert_eq!(get_local_profiles(test_client), vec![local_profile("2", "name 2", None)]);

        client2.sync().await.unwrap();
        assert_eq!(get_local_profiles(client2), vec![local_profile("2", "name 2", None)]);

        let content = test_client.read(test_object.resource, None).await.unwrap().content.expect("o servidor tem o conteúdo");
        assert_eq!(parse_remote_profiles(&content), vec![remote_profile("2", "name 2", "2", None)]);
        teardown(&fixture).await;
    });
}

#[test]
fn sync_profile_that_uses_default_profile() {
    async_test(|d| async move {
        let fixture = setup(&d).await;
        let (test_client, client2, test_object) = (&fixture.test_client, &fixture.client2, &fixture.test_object);
        client2
            .user_data_profiles_service
            .create_profile("1", "name 1", Some(UserDataProfileOptions { use_default_flags: Some(keybindings_flags()), ..UserDataProfileOptions::default() }), None)
            .await
            .unwrap();
        client2.sync().await.unwrap();

        sync(test_object, test_client.get_latest_ref(SyncResource::Profiles).await.unwrap()).await;
        assert_eq!(test_object.status(), SyncStatus::Idle);
        assert!(test_object.conflicts().conflicts.is_empty());

        let content = test_client.read(test_object.resource, None).await.unwrap().content.expect("o servidor tem o conteúdo");
        assert_eq!(parse_remote_profiles(&content), vec![remote_profile("1", "name 1", "1", Some(keybindings_flags()))]);

        assert_eq!(get_local_profiles(test_client), vec![local_profile("1", "name 1", Some(keybindings_flags()))]);
        teardown(&fixture).await;
    });
}

#[test]
fn sync_profile_when_the_profile_is_updated_to_use_default_profile_locally() {
    async_test(|d| async move {
        let fixture = setup(&d).await;
        let (test_client, client2, test_object) = (&fixture.test_client, &fixture.client2, &fixture.test_object);
        client2.user_data_profiles_service.create_profile("1", "name 1", None, None).await.unwrap();
        client2.sync().await.unwrap();

        sync(test_object, test_client.get_latest_ref(SyncResource::Profiles).await.unwrap()).await;

        let profile = test_client.user_data_profiles_service.profiles().into_iter().find(|profile| profile.id == "1").expect("o perfil 1 existe");
        let _update = test_client
            .user_data_profiles_service
            .update_profile(&profile, UserDataProfileUpdateOptions { use_default_flags: Some(keybindings_flags()), ..UserDataProfileUpdateOptions::default() });

        sync(test_object, test_client.get_latest_ref(SyncResource::Profiles).await.unwrap()).await;
        assert_eq!(test_object.status(), SyncStatus::Idle);
        assert!(test_object.conflicts().conflicts.is_empty());

        let content = test_client.read(test_object.resource, None).await.unwrap().content.expect("o servidor tem o conteúdo");
        assert_eq!(parse_remote_profiles(&content), vec![remote_profile("1", "name 1", "1", Some(keybindings_flags()))]);
        assert_eq!(get_local_profiles(test_client), vec![local_profile("1", "name 1", Some(keybindings_flags()))]);
        teardown(&fixture).await;
    });
}

#[test]
fn sync_profile_when_the_profile_is_updated_to_use_default_profile_remotely() {
    async_test(|d| async move {
        let fixture = setup(&d).await;
        let (test_client, client2, test_object) = (&fixture.test_client, &fixture.client2, &fixture.test_object);
        let profile = client2.user_data_profiles_service.create_profile("1", "name 1", None, None).await.unwrap();
        client2.sync().await.unwrap();

        sync(test_object, test_client.get_latest_ref(SyncResource::Profiles).await.unwrap()).await;

        let _update = client2.user_data_profiles_service.update_profile(&profile, UserDataProfileUpdateOptions { use_default_flags: Some(keybindings_flags()), ..UserDataProfileUpdateOptions::default() });
        client2.sync().await.unwrap();

        sync(test_object, test_client.get_latest_ref(SyncResource::Profiles).await.unwrap()).await;
        assert_eq!(test_object.status(), SyncStatus::Idle);
        assert!(test_object.conflicts().conflicts.is_empty());

        let content = test_client.read(test_object.resource, None).await.unwrap().content.expect("o servidor tem o conteúdo");
        assert_eq!(parse_remote_profiles(&content), vec![remote_profile("1", "name 1", "1", Some(keybindings_flags()))]);

        assert_eq!(get_local_profiles(test_client), vec![local_profile("1", "name 1", Some(keybindings_flags()))]);
        teardown(&fixture).await;
    });
}
