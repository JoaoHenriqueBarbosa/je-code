//! Porte de `src/vs/platform/userDataSync/test/common/keybindingsSync.test.ts` (suíte `KeybindingsSync`), caso a
//! caso e na mesma ordem.
//!
//! Divergências: o `setup` (o cliente e o sincronizador de keybindings) roda no começo do corpo de cada teste e o
//! `teardown` (`clear` do servidor) no fim; o servidor é um por teste (o `clear` do `teardown` o devolve ao estado
//! inicial no original). `testObject.sync(manifest)` usa os argumentos padrão do original (`preview` falso,
//! configuração e cabeçalhos vazios). `JSON.parse` do conteúdo vira [`parse_json`] e a comparação de valores JSON.

mod common;

use std::rc::Rc;

use common::files::async_test;
use common::user_data_sync_client::{RecordedRequest, UserDataSyncClient, UserDataSyncTestServer};
use common::{if_match_headers, read_text, sync_synchroniser, write_text};
use je_base::lifecycle::DisposableStore;
use je_base::parts::request::IHeaders;
use je_base::uri::URI;
use je_platform::log::NullLogService;
use je_platform::user_data_profile::IUserDataProfilesService;
use je_platform::user_data_sync::abstract_synchronizer::parse_json;
use je_platform::user_data_sync::keybindings_sync::{KeybindingsSynchroniser, get_keybindings_content_from_sync_content};
use je_platform::user_data_sync::user_data_sync::{ISyncData, IUserDataSyncStoreService, IUserDataSynchroniser, SyncResource, UserDataSyncError, UserDataSyncErrorCode};

struct Fixture {
    server: Rc<UserDataSyncTestServer>,
    client: Rc<UserDataSyncClient>,
    test_object: Rc<KeybindingsSynchroniser>,
}

/// O `setup` da suíte.
async fn setup(disposables: &Rc<DisposableStore>) -> Fixture {
    let server = Rc::new(UserDataSyncTestServer::new());
    let client = disposables.add_rc(UserDataSyncClient::set_up(server.clone(), true).await.unwrap());
    let test_object = client.get_synchronizer(SyncResource::Keybindings).into_keybindings().expect("o sincronizador de keybindings está ativo");
    Fixture { server, client, test_object }
}

/// `client2 = disposableStore.add(new UserDataSyncClient(server)); await client2.setUp(true)`.
async fn set_up_client2(disposables: &Rc<DisposableStore>, server: &Rc<UserDataSyncTestServer>) -> Rc<UserDataSyncClient> {
    disposables.add_rc(UserDataSyncClient::set_up(server.clone(), true).await.unwrap())
}

/// O `teardown` da suíte.
async fn teardown(fixture: &Fixture) {
    fixture.client.user_data_sync_store_service.clear().await.unwrap();
}

/// `testObject.sync(manifest)`.
async fn sync(test_object: &KeybindingsSynchroniser, manifest: Option<String>) {
    sync_synchroniser(&*test_object.base(), manifest, false).await.unwrap();
}

fn keybindings_resource(client: &UserDataSyncClient) -> URI {
    client.user_data_profiles_service.default_profile().keybindings_resource
}

/// `getKeybindingsContentFromSyncContent(syncData!.content, true, logService)`.
fn keybindings_content(sync_data: &Option<ISyncData>) -> Option<String> {
    get_keybindings_content_from_sync_content(&sync_data.as_ref().expect("há dados sincronizados").content, true, &NullLogService::shared())
}

/// `JSON.stringify([{ key: 'shift+cmd+w', command: 'workbench.action.closeAllEditors' }])`.
const CLOSE_ALL_EDITORS_CONTENT: &str = r#"[{"key":"shift+cmd+w","command":"workbench.action.closeAllEditors"}]"#;

#[test]
fn when_keybindings_file_does_not_exist() {
    async_test(|d| async move {
        let fixture = setup(&d).await;
        let (server, client, test_object) = (&fixture.server, &fixture.client, &fixture.test_object);
        let keybindings_resource = keybindings_resource(client);

        assert!(test_object.get_last_sync_user_data().await.unwrap().is_none());
        let mut manifest = client.get_latest_ref(SyncResource::Keybindings).await.unwrap();
        server.reset();
        sync(test_object, manifest).await;

        assert_eq!(server.requests(), Vec::<RecordedRequest>::new());
        assert!(!client.file_service.exists(&keybindings_resource).await.unwrap());

        let last_sync_user_data = test_object.get_last_sync_user_data().await.unwrap().expect("há dados da última sincronização");
        let remote_user_data = test_object.get_remote_user_data(None).await.unwrap();
        assert_eq!(last_sync_user_data.r#ref, remote_user_data.r#ref);
        assert_eq!(last_sync_user_data.sync_data, remote_user_data.sync_data);
        assert_eq!(last_sync_user_data.sync_data, None);

        manifest = client.get_latest_ref(SyncResource::Keybindings).await.unwrap();
        server.reset();
        sync(test_object, manifest).await;
        assert_eq!(server.requests(), Vec::<RecordedRequest>::new());

        manifest = client.get_latest_ref(SyncResource::Keybindings).await.unwrap();
        server.reset();
        sync(test_object, manifest).await;
        assert_eq!(server.requests(), Vec::<RecordedRequest>::new());
        teardown(&fixture).await;
    });
}

#[test]
fn when_keybindings_file_is_empty_and_remote_has_no_changes() {
    async_test(|d| async move {
        let fixture = setup(&d).await;
        let (client, test_object) = (&fixture.client, &fixture.test_object);
        let keybindings_resource = keybindings_resource(client);
        write_text(&client.file_service, &keybindings_resource, "").await.unwrap();

        sync(test_object, client.get_latest_ref(SyncResource::Keybindings).await.unwrap()).await;

        let last_sync_user_data = test_object.get_last_sync_user_data().await.unwrap().expect("há dados da última sincronização");
        let remote_user_data = test_object.get_remote_user_data(None).await.unwrap();
        assert_eq!(keybindings_content(&last_sync_user_data.sync_data), Some("[]".to_string()));
        assert_eq!(keybindings_content(&remote_user_data.sync_data), Some("[]".to_string()));
        assert_eq!(read_text(&client.file_service, &keybindings_resource).await, "");
        teardown(&fixture).await;
    });
}

#[test]
fn when_keybindings_file_is_empty_and_remote_has_changes() {
    async_test(|d| async move {
        let fixture = setup(&d).await;
        let (server, client, test_object) = (&fixture.server, &fixture.client, &fixture.test_object);
        let client2 = set_up_client2(&d, server).await;
        let content = CLOSE_ALL_EDITORS_CONTENT;
        write_text(&client2.file_service, &keybindings_resource(&client2), content).await.unwrap();
        client2.sync().await.unwrap();

        let keybindings_resource = keybindings_resource(client);
        write_text(&client.file_service, &keybindings_resource, "").await.unwrap();

        sync(test_object, client.get_latest_ref(SyncResource::Keybindings).await.unwrap()).await;

        let last_sync_user_data = test_object.get_last_sync_user_data().await.unwrap().expect("há dados da última sincronização");
        let remote_user_data = test_object.get_remote_user_data(None).await.unwrap();
        assert_eq!(keybindings_content(&last_sync_user_data.sync_data), Some(content.to_string()));
        assert_eq!(keybindings_content(&remote_user_data.sync_data), Some(content.to_string()));
        assert_eq!(read_text(&client.file_service, &keybindings_resource).await, content);
        teardown(&fixture).await;
    });
}

#[test]
fn when_keybindings_file_is_empty_with_comment_and_remote_has_no_changes() {
    async_test(|d| async move {
        let fixture = setup(&d).await;
        let (client, test_object) = (&fixture.client, &fixture.test_object);
        let keybindings_resource = keybindings_resource(client);
        let expected_content = "// Empty Keybindings";
        write_text(&client.file_service, &keybindings_resource, expected_content).await.unwrap();

        sync(test_object, client.get_latest_ref(SyncResource::Keybindings).await.unwrap()).await;

        let last_sync_user_data = test_object.get_last_sync_user_data().await.unwrap().expect("há dados da última sincronização");
        let remote_user_data = test_object.get_remote_user_data(None).await.unwrap();
        assert_eq!(keybindings_content(&last_sync_user_data.sync_data), Some(expected_content.to_string()));
        assert_eq!(keybindings_content(&remote_user_data.sync_data), Some(expected_content.to_string()));
        assert_eq!(read_text(&client.file_service, &keybindings_resource).await, expected_content);
        teardown(&fixture).await;
    });
}

#[test]
fn when_keybindings_file_is_empty_and_remote_has_keybindings() {
    async_test(|d| async move {
        let fixture = setup(&d).await;
        let (server, client, test_object) = (&fixture.server, &fixture.client, &fixture.test_object);
        let client2 = set_up_client2(&d, server).await;
        let content = CLOSE_ALL_EDITORS_CONTENT;
        write_text(&client2.file_service, &keybindings_resource(&client2), content).await.unwrap();
        client2.sync().await.unwrap();

        let keybindings_resource = keybindings_resource(client);
        write_text(&client.file_service, &keybindings_resource, "// Empty Keybindings").await.unwrap();

        sync(test_object, client.get_latest_ref(SyncResource::Keybindings).await.unwrap()).await;

        let last_sync_user_data = test_object.get_last_sync_user_data().await.unwrap().expect("há dados da última sincronização");
        let remote_user_data = test_object.get_remote_user_data(None).await.unwrap();
        assert_eq!(keybindings_content(&last_sync_user_data.sync_data), Some(content.to_string()));
        assert_eq!(keybindings_content(&remote_user_data.sync_data), Some(content.to_string()));
        assert_eq!(read_text(&client.file_service, &keybindings_resource).await, content);
        teardown(&fixture).await;
    });
}

#[test]
fn when_keybindings_file_is_empty_and_remote_has_empty_array() {
    async_test(|d| async move {
        let fixture = setup(&d).await;
        let (server, client, test_object) = (&fixture.server, &fixture.client, &fixture.test_object);
        let client2 = set_up_client2(&d, server).await;
        let content = "// Place your key bindings in this file to override the defaults\n[\n]";
        write_text(&client2.file_service, &keybindings_resource(&client2), content).await.unwrap();
        client2.sync().await.unwrap();

        let keybindings_resource = keybindings_resource(client);
        let expected_local_content = "// Empty Keybindings";
        write_text(&client.file_service, &keybindings_resource, expected_local_content).await.unwrap();

        sync(test_object, client.get_latest_ref(SyncResource::Keybindings).await.unwrap()).await;

        let last_sync_user_data = test_object.get_last_sync_user_data().await.unwrap().expect("há dados da última sincronização");
        let remote_user_data = test_object.get_remote_user_data(None).await.unwrap();
        assert_eq!(keybindings_content(&last_sync_user_data.sync_data), Some(content.to_string()));
        assert_eq!(keybindings_content(&remote_user_data.sync_data), Some(content.to_string()));
        assert_eq!(read_text(&client.file_service, &keybindings_resource).await, expected_local_content);
        teardown(&fixture).await;
    });
}

#[test]
fn when_keybindings_file_is_created_after_first_sync() {
    async_test(|d| async move {
        let fixture = setup(&d).await;
        let (server, client, test_object) = (&fixture.server, &fixture.client, &fixture.test_object);
        let keybindings_resource = keybindings_resource(client);
        sync(test_object, client.get_latest_ref(SyncResource::Keybindings).await.unwrap()).await;
        common::create_text(&client.file_service, &keybindings_resource, "[]").await.unwrap();

        let mut last_sync_user_data = test_object.get_last_sync_user_data().await.unwrap();
        let manifest = client.get_latest_ref(SyncResource::Keybindings).await.unwrap();
        server.reset();
        sync(test_object, manifest).await;

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
        assert_eq!(keybindings_content(&last_sync_user_data.sync_data), Some("[]".to_string()));
        teardown(&fixture).await;
    });
}

#[test]
fn test_apply_remote_when_keybindings_file_does_not_exist() {
    async_test(|d| async move {
        let fixture = setup(&d).await;
        let (server, client, test_object) = (&fixture.server, &fixture.client, &fixture.test_object);
        let keybindings_resource = keybindings_resource(client);
        if client.file_service.exists(&keybindings_resource).await.unwrap() {
            client.file_service.del(&keybindings_resource, None).await.unwrap();
        }

        let preview = sync_synchroniser(&*test_object.base(), client.get_latest_ref(SyncResource::Keybindings).await.unwrap(), true).await.unwrap().expect("há o preview");

        server.reset();
        let remote_resource = &preview.resource_previews[0].remote_resource;
        let content = test_object.resolve_content(remote_resource).await.unwrap();
        test_object.accept(remote_resource, Some(content)).await.unwrap();
        test_object.apply(false, IHeaders::new()).await.unwrap();
        assert_eq!(server.requests(), Vec::<RecordedRequest>::new());
        teardown(&fixture).await;
    });
}

#[test]
fn sync_throws_invalid_content_error_content_is_an_object() {
    async_test(|d| async move {
        let fixture = setup(&d).await;
        let (client, test_object) = (&fixture.client, &fixture.test_object);
        write_text(&client.file_service, &keybindings_resource(client), "{}").await.unwrap();
        let result = sync_synchroniser(&*test_object.base(), client.get_latest_ref(SyncResource::Keybindings).await.unwrap(), false).await;
        let error = result.expect_err("should fail with invalid content error");
        let sync_error = UserDataSyncError::from_error(&error).expect("é um UserDataSyncError");
        assert_eq!(sync_error.code, UserDataSyncErrorCode::LocalInvalidContent);
        teardown(&fixture).await;
    });
}

#[test]
fn sync_profile_keybindings() {
    async_test(|d| async move {
        let fixture = setup(&d).await;
        let (server, client) = (&fixture.server, &fixture.client);
        let client2 = set_up_client2(&d, server).await;
        let profile = client2.user_data_profiles_service.create_named_profile("profile1", None, None).await.unwrap();
        write_text(&client2.file_service, &profile.keybindings_resource, CLOSE_ALL_EDITORS_CONTENT).await.unwrap();
        client2.sync().await.unwrap();

        client.sync().await.unwrap();

        let synced_profile = client.user_data_profiles_service.profiles().into_iter().find(|p| p.id == profile.id).expect("o perfil foi sincronizado");
        let content = read_text(&client.file_service, &synced_profile.keybindings_resource).await;
        assert_eq!(parse_json(&content).unwrap(), parse_json(r#"[{"key":"shift+cmd+w","command":"workbench.action.closeAllEditors"}]"#).unwrap());
        teardown(&fixture).await;
    });
}
