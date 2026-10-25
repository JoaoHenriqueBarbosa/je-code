//! Porte de `src/vs/platform/userDataSync/test/common/mcpSync.test.ts` (suíte `McpSync`), caso a caso e na
//! mesma ordem.
//!
//! Divergências: o `setup` (o cliente e o sincronizador de mcp) roda no começo do corpo de cada teste, dentro do
//! `runWithFakedTimers` de [`async_test`], e o `teardown` (`clear` do servidor) no fim; o servidor é um por teste
//! (o `clear` do `teardown` o devolve ao estado inicial no original). `testObject.sync(manifest)` usa os argumentos
//! padrão do original (`preview` falso, configuração e cabeçalhos vazios). Os `fileService.writeFile` e
//! `fileService.del` sem `await` do original ficam sem `await` (a promessa roda até o primeiro `await`). Os
//! `JSON.stringify` dos literais são os textos compactos equivalentes.

mod common;

use std::rc::Rc;

use common::files::async_test;
use common::user_data_sync_client::{RecordedRequest, UserDataSyncClient, UserDataSyncTestServer};
use common::{create_text, if_match_headers, read_text, sync_synchroniser, write_text};
use je_base::lifecycle::DisposableStore;
use je_base::parts::request::IHeaders;
use je_base::uri::URI;
use je_platform::log::NullLogService;
use je_platform::user_data_profile::IUserDataProfilesService;
use je_platform::user_data_sync::mcp_sync::{McpSynchroniser, get_mcp_content_from_sync_content};
use je_platform::user_data_sync::user_data_sync::{Change, ISyncData, IUserDataSyncStoreService, IUserDataSynchroniser, MergeState, SyncResource, SyncStatus};

struct Fixture {
    server: Rc<UserDataSyncTestServer>,
    client: Rc<UserDataSyncClient>,
    test_object: Rc<McpSynchroniser>,
}

/// O `setup` da suíte.
async fn setup(disposables: &Rc<DisposableStore>) -> Fixture {
    let server = Rc::new(UserDataSyncTestServer::new());
    let client = disposables.add_rc(UserDataSyncClient::set_up(server.clone(), true).await.unwrap());
    let test_object = client.get_synchronizer(SyncResource::Mcp).into_mcp().expect("o sincronizador de mcp está ativo");
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
async fn sync(test_object: &McpSynchroniser, manifest: Option<String>) {
    sync_synchroniser(&*test_object.base(), manifest, false).await.unwrap();
}

/// `defaultProfile.mcpResource`.
fn mcp_resource_of(client: &UserDataSyncClient) -> URI {
    client.user_data_profiles_service.default_profile().mcp_resource
}

/// `getMcpContentFromSyncContent(syncData!.content, logService)`.
fn mcp_content(sync_data: &Option<ISyncData>) -> Option<String> {
    get_mcp_content_from_sync_content(&sync_data.as_ref().expect("há dados sincronizados").content, &NullLogService::shared())
}

/// `{ mcpServers: {} }`.
const EMPTY_MCP: &str = r#"{"mcpServers":{}}"#;
/// `{ mcpServers: { 'test-server': { command: 'node', args: ['./server.js'] } } }`.
const SERVER_MCP: &str = r#"{"mcpServers":{"test-server":{"command":"node","args":["./server.js"]}}}"#;
/// `{ mcpServers: { server1: { command: 'node', args: ['./server1.js'] } } }`.
const SERVER_1_MCP: &str = r#"{"mcpServers":{"server1":{"command":"node","args":["./server1.js"]}}}"#;
/// `{ mcpServers: { server2: { command: 'node', args: ['./server2.js'] } } }`.
const SERVER_2_MCP: &str = r#"{"mcpServers":{"server2":{"command":"node","args":["./server2.js"]}}}"#;
/// `server1` e `server2` juntos.
const BOTH_SERVERS_MCP: &str = r#"{"mcpServers":{"server1":{"command":"node","args":["./server1.js"]},"server2":{"command":"node","args":["./server2.js"]}}}"#;

#[test]
fn when_mcp_file_does_not_exist() {
    async_test(|d| async move {
        let fixture = setup(&d).await;
        let (server, client, test_object) = (&fixture.server, &fixture.client, &fixture.test_object);
        let mcp_resource = mcp_resource_of(client);

        assert!(test_object.get_last_sync_user_data().await.unwrap().is_none());
        let mut manifest = client.get_latest_ref(SyncResource::Mcp).await.unwrap();
        server.reset();
        sync(test_object, manifest).await;

        assert_eq!(server.requests(), Vec::<RecordedRequest>::new());
        assert!(!client.file_service.exists(&mcp_resource).await.unwrap());

        let last_sync_user_data = test_object.get_last_sync_user_data().await.unwrap().expect("há dados da última sincronização");
        let remote_user_data = test_object.get_remote_user_data(None).await.unwrap();
        assert_eq!(last_sync_user_data.r#ref, remote_user_data.r#ref);
        assert_eq!(last_sync_user_data.sync_data, remote_user_data.sync_data);
        assert_eq!(last_sync_user_data.sync_data, None);

        manifest = client.get_latest_ref(SyncResource::Mcp).await.unwrap();
        server.reset();
        sync(test_object, manifest).await;
        assert_eq!(server.requests(), Vec::<RecordedRequest>::new());

        manifest = client.get_latest_ref(SyncResource::Mcp).await.unwrap();
        server.reset();
        sync(test_object, manifest).await;
        assert_eq!(server.requests(), Vec::<RecordedRequest>::new());
        teardown(&fixture).await;
    });
}

#[test]
fn when_mcp_file_does_not_exist_and_remote_has_changes() {
    async_test(|d| async move {
        let fixture = setup(&d).await;
        let (server, client, test_object) = (&fixture.server, &fixture.client, &fixture.test_object);
        let client2 = set_up_client2(&d, server).await;
        let content = SERVER_MCP;
        write_text(&client2.file_service, &mcp_resource_of(&client2), content).await.unwrap();
        client2.sync().await.unwrap();

        let mcp_resource = mcp_resource_of(client);

        sync(test_object, client.get_latest_ref(SyncResource::Mcp).await.unwrap()).await;

        assert_eq!(test_object.status(), SyncStatus::Idle);
        let last_sync_user_data = test_object.get_last_sync_user_data().await.unwrap().expect("há dados da última sincronização");
        let remote_user_data = test_object.get_remote_user_data(None).await.unwrap();
        assert_eq!(mcp_content(&last_sync_user_data.sync_data), Some(content.to_string()));
        assert_eq!(mcp_content(&remote_user_data.sync_data), Some(content.to_string()));
        assert_eq!(read_text(&client.file_service, &mcp_resource).await, content);
        teardown(&fixture).await;
    });
}

#[test]
fn when_mcp_file_exists_locally_and_remote_has_no_mcp() {
    async_test(|d| async move {
        let fixture = setup(&d).await;
        let (client, test_object) = (&fixture.client, &fixture.test_object);
        let mcp_resource = mcp_resource_of(client);
        let content = SERVER_MCP;
        let _ = write_text(&client.file_service, &mcp_resource, content);

        sync(test_object, client.get_latest_ref(SyncResource::Mcp).await.unwrap()).await;

        assert_eq!(test_object.status(), SyncStatus::Idle);
        let last_sync_user_data = test_object.get_last_sync_user_data().await.unwrap().expect("há dados da última sincronização");
        let remote_user_data = test_object.get_remote_user_data(None).await.unwrap();
        assert_eq!(mcp_content(&last_sync_user_data.sync_data), Some(content.to_string()));
        assert_eq!(mcp_content(&remote_user_data.sync_data), Some(content.to_string()));
        teardown(&fixture).await;
    });
}

#[test]
fn first_time_sync_when_mcp_file_exists_locally_with_same_content_as_remote() {
    async_test(|d| async move {
        let fixture = setup(&d).await;
        let (server, client, test_object) = (&fixture.server, &fixture.client, &fixture.test_object);
        let client2 = set_up_client2(&d, server).await;
        let content = SERVER_MCP;
        write_text(&client2.file_service, &mcp_resource_of(&client2), content).await.unwrap();
        client2.sync().await.unwrap();

        let mcp_resource = mcp_resource_of(client);
        write_text(&client.file_service, &mcp_resource, content).await.unwrap();

        sync(test_object, client.get_latest_ref(SyncResource::Mcp).await.unwrap()).await;

        assert_eq!(test_object.status(), SyncStatus::Idle);
        let last_sync_user_data = test_object.get_last_sync_user_data().await.unwrap().expect("há dados da última sincronização");
        let remote_user_data = test_object.get_remote_user_data(None).await.unwrap();
        assert_eq!(mcp_content(&last_sync_user_data.sync_data), Some(content.to_string()));
        assert_eq!(mcp_content(&remote_user_data.sync_data), Some(content.to_string()));
        assert_eq!(read_text(&client.file_service, &mcp_resource).await, content);
        teardown(&fixture).await;
    });
}

#[test]
fn when_mcp_file_locally_has_moved_forward() {
    async_test(|d| async move {
        let fixture = setup(&d).await;
        let (client, test_object) = (&fixture.client, &fixture.test_object);
        let mcp_resource = mcp_resource_of(client);
        let _ = write_text(&client.file_service, &mcp_resource, EMPTY_MCP);

        sync(test_object, client.get_latest_ref(SyncResource::Mcp).await.unwrap()).await;

        let content = SERVER_MCP;
        let _ = write_text(&client.file_service, &mcp_resource, content);

        sync(test_object, client.get_latest_ref(SyncResource::Mcp).await.unwrap()).await;

        assert_eq!(test_object.status(), SyncStatus::Idle);
        let last_sync_user_data = test_object.get_last_sync_user_data().await.unwrap().expect("há dados da última sincronização");
        let remote_user_data = test_object.get_remote_user_data(None).await.unwrap();
        assert_eq!(mcp_content(&last_sync_user_data.sync_data), Some(content.to_string()));
        assert_eq!(mcp_content(&remote_user_data.sync_data), Some(content.to_string()));
        teardown(&fixture).await;
    });
}

#[test]
fn when_mcp_file_remotely_has_moved_forward() {
    async_test(|d| async move {
        let fixture = setup(&d).await;
        let (server, client, test_object) = (&fixture.server, &fixture.client, &fixture.test_object);
        let client2 = set_up_client2(&d, server).await;
        let mcp_resource2 = mcp_resource_of(&client2);
        write_text(&client2.file_service, &mcp_resource2, EMPTY_MCP).await.unwrap();

        let mcp_resource = mcp_resource_of(client);

        client2.sync().await.unwrap();
        sync(test_object, client.get_latest_ref(SyncResource::Mcp).await.unwrap()).await;

        let content = SERVER_MCP;
        let _ = write_text(&client2.file_service, &mcp_resource2, content);

        client2.sync().await.unwrap();
        sync(test_object, client.get_latest_ref(SyncResource::Mcp).await.unwrap()).await;

        assert_eq!(test_object.status(), SyncStatus::Idle);
        let last_sync_user_data = test_object.get_last_sync_user_data().await.unwrap().expect("há dados da última sincronização");
        let remote_user_data = test_object.get_remote_user_data(None).await.unwrap();
        assert_eq!(mcp_content(&last_sync_user_data.sync_data), Some(content.to_string()));
        assert_eq!(mcp_content(&remote_user_data.sync_data), Some(content.to_string()));
        assert_eq!(read_text(&client.file_service, &mcp_resource).await, content);
        teardown(&fixture).await;
    });
}

#[test]
fn when_mcp_file_has_moved_forward_locally_and_remotely_with_same_changes() {
    async_test(|d| async move {
        let fixture = setup(&d).await;
        let (server, client, test_object) = (&fixture.server, &fixture.client, &fixture.test_object);
        let client2 = set_up_client2(&d, server).await;
        let mcp_resource2 = mcp_resource_of(&client2);
        write_text(&client2.file_service, &mcp_resource2, EMPTY_MCP).await.unwrap();

        let mcp_resource = mcp_resource_of(client);

        client2.sync().await.unwrap();
        sync(test_object, client.get_latest_ref(SyncResource::Mcp).await.unwrap()).await;

        let content = SERVER_MCP;
        let _ = write_text(&client2.file_service, &mcp_resource2, content);
        client2.sync().await.unwrap();

        let _ = write_text(&client.file_service, &mcp_resource, content);
        sync(test_object, client.get_latest_ref(SyncResource::Mcp).await.unwrap()).await;

        assert_eq!(test_object.status(), SyncStatus::Idle);
        let last_sync_user_data = test_object.get_last_sync_user_data().await.unwrap().expect("há dados da última sincronização");
        let remote_user_data = test_object.get_remote_user_data(None).await.unwrap();
        assert_eq!(mcp_content(&last_sync_user_data.sync_data), Some(content.to_string()));
        assert_eq!(mcp_content(&remote_user_data.sync_data), Some(content.to_string()));
        assert_eq!(read_text(&client.file_service, &mcp_resource).await, content);
        teardown(&fixture).await;
    });
}

#[test]
fn when_mcp_file_has_moved_forward_locally_and_remotely_accept_preview() {
    async_test(|d| async move {
        let fixture = setup(&d).await;
        let (server, client, test_object) = (&fixture.server, &fixture.client, &fixture.test_object);
        let client2 = set_up_client2(&d, server).await;
        let mcp_resource2 = mcp_resource_of(&client2);
        write_text(&client2.file_service, &mcp_resource2, EMPTY_MCP).await.unwrap();

        let mcp_resource = mcp_resource_of(client);

        client2.sync().await.unwrap();
        sync(test_object, client.get_latest_ref(SyncResource::Mcp).await.unwrap()).await;

        let _ = write_text(&client2.file_service, &mcp_resource2, SERVER_1_MCP);
        client2.sync().await.unwrap();

        let content = SERVER_2_MCP;
        let _ = write_text(&client.file_service, &mcp_resource, content);
        sync(test_object, client.get_latest_ref(SyncResource::Mcp).await.unwrap()).await;

        let conflicts = test_object.conflicts().conflicts;
        let preview_content = read_text(&client.file_service, &conflicts[0].preview_resource).await;
        assert_eq!(test_object.status(), SyncStatus::HasConflicts);
        assert_eq!(conflicts.len(), 1);
        assert_eq!(conflicts[0].merge_state, MergeState::Conflict);
        assert_eq!(conflicts[0].local_change, Change::Modified);
        assert_eq!(conflicts[0].remote_change, Change::Modified);

        test_object.accept(&conflicts[0].preview_resource, None).await.unwrap();
        test_object.apply(false, IHeaders::new()).await.unwrap();
        assert_eq!(test_object.status(), SyncStatus::Idle);
        let last_sync_user_data = test_object.get_last_sync_user_data().await.unwrap().expect("há dados da última sincronização");
        let remote_user_data = test_object.get_remote_user_data(None).await.unwrap();
        assert_eq!(mcp_content(&last_sync_user_data.sync_data), Some(preview_content.clone()));
        assert_eq!(mcp_content(&remote_user_data.sync_data), Some(preview_content.clone()));
        assert_eq!(read_text(&client.file_service, &mcp_resource).await, preview_content);
        teardown(&fixture).await;
    });
}

#[test]
fn when_mcp_file_has_moved_forward_locally_and_remotely_accept_modified_preview() {
    async_test(|d| async move {
        let fixture = setup(&d).await;
        let (server, client, test_object) = (&fixture.server, &fixture.client, &fixture.test_object);
        let client2 = set_up_client2(&d, server).await;
        let mcp_resource2 = mcp_resource_of(&client2);
        write_text(&client2.file_service, &mcp_resource2, EMPTY_MCP).await.unwrap();

        let mcp_resource = mcp_resource_of(client);

        client2.sync().await.unwrap();
        sync(test_object, client.get_latest_ref(SyncResource::Mcp).await.unwrap()).await;

        let _ = write_text(&client2.file_service, &mcp_resource2, SERVER_1_MCP);
        client2.sync().await.unwrap();

        let _ = write_text(&client.file_service, &mcp_resource, SERVER_2_MCP);
        sync(test_object, client.get_latest_ref(SyncResource::Mcp).await.unwrap()).await;

        let content = BOTH_SERVERS_MCP;
        let preview_resource = test_object.conflicts().conflicts[0].preview_resource.clone();
        test_object.accept(&preview_resource, Some(Some(content.to_string()))).await.unwrap();
        test_object.apply(false, IHeaders::new()).await.unwrap();
        assert_eq!(test_object.status(), SyncStatus::Idle);
        let last_sync_user_data = test_object.get_last_sync_user_data().await.unwrap().expect("há dados da última sincronização");
        let remote_user_data = test_object.get_remote_user_data(None).await.unwrap();
        assert_eq!(mcp_content(&last_sync_user_data.sync_data), Some(content.to_string()));
        assert_eq!(mcp_content(&remote_user_data.sync_data), Some(content.to_string()));
        assert_eq!(read_text(&client.file_service, &mcp_resource).await, content);
        teardown(&fixture).await;
    });
}

#[test]
fn when_mcp_file_has_moved_forward_locally_and_remotely_accept_remote() {
    async_test(|d| async move {
        let fixture = setup(&d).await;
        let (server, client, test_object) = (&fixture.server, &fixture.client, &fixture.test_object);
        let client2 = set_up_client2(&d, server).await;
        let mcp_resource2 = mcp_resource_of(&client2);
        write_text(&client2.file_service, &mcp_resource2, EMPTY_MCP).await.unwrap();

        let mcp_resource = mcp_resource_of(client);

        client2.sync().await.unwrap();
        sync(test_object, client.get_latest_ref(SyncResource::Mcp).await.unwrap()).await;

        let content = SERVER_1_MCP;
        let _ = write_text(&client2.file_service, &mcp_resource2, content);
        client2.sync().await.unwrap();

        let _ = write_text(&client.file_service, &mcp_resource, SERVER_2_MCP);
        sync(test_object, client.get_latest_ref(SyncResource::Mcp).await.unwrap()).await;
        assert_eq!(test_object.status(), SyncStatus::HasConflicts);

        let remote_resource = test_object.conflicts().conflicts[0].remote_resource.clone();
        test_object.accept(&remote_resource, None).await.unwrap();
        test_object.apply(false, IHeaders::new()).await.unwrap();
        assert_eq!(test_object.status(), SyncStatus::Idle);
        let last_sync_user_data = test_object.get_last_sync_user_data().await.unwrap().expect("há dados da última sincronização");
        let remote_user_data = test_object.get_remote_user_data(None).await.unwrap();
        assert_eq!(mcp_content(&last_sync_user_data.sync_data), Some(content.to_string()));
        assert_eq!(mcp_content(&remote_user_data.sync_data), Some(content.to_string()));
        assert_eq!(read_text(&client.file_service, &mcp_resource).await, content);
        teardown(&fixture).await;
    });
}

#[test]
fn when_mcp_file_has_moved_forward_locally_and_remotely_accept_local() {
    async_test(|d| async move {
        let fixture = setup(&d).await;
        let (server, client, test_object) = (&fixture.server, &fixture.client, &fixture.test_object);
        let client2 = set_up_client2(&d, server).await;
        let mcp_resource2 = mcp_resource_of(&client2);
        write_text(&client2.file_service, &mcp_resource2, EMPTY_MCP).await.unwrap();

        let mcp_resource = mcp_resource_of(client);

        client2.sync().await.unwrap();
        sync(test_object, client.get_latest_ref(SyncResource::Mcp).await.unwrap()).await;

        let _ = write_text(&client2.file_service, &mcp_resource2, SERVER_1_MCP);
        client2.sync().await.unwrap();

        let content = SERVER_2_MCP;
        let _ = write_text(&client.file_service, &mcp_resource, content);
        sync(test_object, client.get_latest_ref(SyncResource::Mcp).await.unwrap()).await;
        assert_eq!(test_object.status(), SyncStatus::HasConflicts);

        let local_resource = test_object.conflicts().conflicts[0].local_resource.clone();
        test_object.accept(&local_resource, None).await.unwrap();
        test_object.apply(false, IHeaders::new()).await.unwrap();
        assert_eq!(test_object.status(), SyncStatus::Idle);
        let last_sync_user_data = test_object.get_last_sync_user_data().await.unwrap().expect("há dados da última sincronização");
        let remote_user_data = test_object.get_remote_user_data(None).await.unwrap();
        assert_eq!(mcp_content(&last_sync_user_data.sync_data), Some(content.to_string()));
        assert_eq!(mcp_content(&remote_user_data.sync_data), Some(content.to_string()));
        assert_eq!(read_text(&client.file_service, &mcp_resource).await, content);
        teardown(&fixture).await;
    });
}

#[test]
fn when_mcp_file_was_removed_in_one_client() {
    async_test(|d| async move {
        let fixture = setup(&d).await;
        let (server, client, test_object) = (&fixture.server, &fixture.client, &fixture.test_object);
        let mcp_resource = mcp_resource_of(client);
        write_text(&client.file_service, &mcp_resource, EMPTY_MCP).await.unwrap();
        sync(test_object, client.get_latest_ref(SyncResource::Mcp).await.unwrap()).await;

        let client2 = set_up_client2(&d, server).await;
        client2.sync().await.unwrap();

        let mcp_resource2 = mcp_resource_of(&client2);
        let _ = client2.file_service.del(&mcp_resource2, None);
        client2.sync().await.unwrap();

        sync(test_object, client.get_latest_ref(SyncResource::Mcp).await.unwrap()).await;

        assert_eq!(test_object.status(), SyncStatus::Idle);
        let last_sync_user_data = test_object.get_last_sync_user_data().await.unwrap().expect("há dados da última sincronização");
        let remote_user_data = test_object.get_remote_user_data(None).await.unwrap();
        assert_eq!(mcp_content(&last_sync_user_data.sync_data), None);
        assert_eq!(mcp_content(&remote_user_data.sync_data), None);
        assert!(!client.file_service.exists(&mcp_resource).await.unwrap());
        teardown(&fixture).await;
    });
}

#[test]
fn when_mcp_file_is_created_after_first_sync() {
    async_test(|d| async move {
        let fixture = setup(&d).await;
        let (server, client, test_object) = (&fixture.server, &fixture.client, &fixture.test_object);
        let mcp_resource = mcp_resource_of(client);
        sync(test_object, client.get_latest_ref(SyncResource::Mcp).await.unwrap()).await;

        create_text(&client.file_service, &mcp_resource, SERVER_MCP).await.unwrap();

        let mut last_sync_user_data = test_object.get_last_sync_user_data().await.unwrap();
        let manifest = client.get_latest_ref(SyncResource::Mcp).await.unwrap();
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
        assert_eq!(mcp_content(&last_sync_user_data.sync_data), Some(SERVER_MCP.to_string()));
        teardown(&fixture).await;
    });
}

#[test]
fn apply_remote_when_mcp_file_does_not_exist() {
    async_test(|d| async move {
        let fixture = setup(&d).await;
        let (server, client, test_object) = (&fixture.server, &fixture.client, &fixture.test_object);
        let mcp_resource = mcp_resource_of(client);
        if client.file_service.exists(&mcp_resource).await.unwrap() {
            client.file_service.del(&mcp_resource, None).await.unwrap();
        }

        let preview = sync_synchroniser(&*test_object.base(), client.get_latest_ref(SyncResource::Mcp).await.unwrap(), true).await.unwrap().expect("há o preview");

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
fn sync_profile_mcp() {
    async_test(|d| async move {
        let fixture = setup(&d).await;
        let (server, client) = (&fixture.server, &fixture.client);
        let client2 = set_up_client2(&d, server).await;
        let profile = client2.user_data_profiles_service.create_named_profile("profile1", None, None).await.unwrap();
        let expected = SERVER_MCP;
        create_text(&client2.file_service, &profile.mcp_resource, expected).await.unwrap();
        client2.sync().await.unwrap();

        client.sync().await.unwrap();

        let synced_profile = client.user_data_profiles_service.profiles().into_iter().find(|p| p.id == profile.id).expect("o perfil foi sincronizado");
        let actual = read_text(&client.file_service, &synced_profile.mcp_resource).await;
        assert_eq!(actual, expected);
        teardown(&fixture).await;
    });
}
