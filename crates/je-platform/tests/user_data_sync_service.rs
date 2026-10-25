//! Porte de `src/vs/platform/userDataSync/test/common/userDataSyncService.test.ts` (suíte `UserDataSyncService`),
//! caso a caso e na mesma ordem.
//!
//! Notas de tradução:
//! - Os testes rodam sob [`async_test`] (o `ensureNoDisposablesAreLeakedInTestSuite` e o `runWithFakedTimers`), com
//!   o `disposableStore` entregue ao corpo. `client.instantiationService.get(IFoo)` é o campo do
//!   [`UserDataSyncClient`] (o `IUserDataSyncService` é `user_data_sync_service`, e assim por diante).
//! - `assert.deepStrictEqual(target.requests, [...])` é [`assert_requests`], sobre [`RecordedRequest`], com as
//!   tuplas `(tipo, caminho sob /v1/, cabeçalhos)`.
//! - As escritas de arquivos que os testes repetem (settings, keybindings, snippets, prompts, argv e, em dois
//!   casos, `tasks.json`) são [`write_local_changes`]; a ordem das escritas não altera o resultado.
//! - `joinPath(dirname(settingsResource), 'tasks.json')` usa o [`ExtUri`] sem viés, como o cliente de teste.
//! - `syncTask.run().then(null, () => null)` do teste de parada é a promessa guardada e aguardada (ignorando o
//!   erro) depois das asserções; ela só roda enquanto a parada acontece.
//! - O `assert.fail` do teste "can run sync taks only once" está dentro do `try`, que o captura: o teste não
//!   falha em nenhum dos dois desfechos da segunda execução, e aqui o resultado dela é ignorado.

mod common;

use std::cell::RefCell;
use std::rc::Rc;

use common::files::async_test;
use common::user_data_sync_client::{RecordedRequest, UserDataSyncClient, UserDataSyncTestServer};
use common::write_text;
use je_base::lifecycle::{DisposableStore, IDisposable};
use je_base::parts::request::{HeaderValue, IHeaders};
use je_base::path::ProcessEnvironment;
use je_base::resources::ExtUri;
use je_platform::user_data_profile::{IUserDataProfilesService, UseDefaultProfileFlags, UserDataProfileOptions};
use je_platform::user_data_sync::user_data_sync::{IUserDataSyncEnablementService, SyncResource, SyncStatus};

/// `{ type, url: `${target.url}/v1/<path>`, headers }`.
type Expected<'a> = (&'a str, &'a str, &'a [(&'a str, &'a str)]);

#[track_caller]
fn assert_requests(target: &UserDataSyncTestServer, expected: &[Expected]) {
    let expected: Vec<RecordedRequest> = expected
        .iter()
        .map(|(r#type, path, headers)| RecordedRequest {
            url: format!("{}/v1/{path}", target.url),
            r#type: r#type.to_string(),
            headers: headers.iter().map(|(name, value)| (name.to_string(), HeaderValue::One(value.to_string()))).collect::<IHeaders>(),
        })
        .collect();
    assert_eq!(target.requests(), expected);
}

async fn set_up(d: &Rc<DisposableStore>, target: &Rc<UserDataSyncTestServer>, empty: bool) -> Rc<UserDataSyncClient> {
    d.add_rc(UserDataSyncClient::set_up(target.clone(), empty).await.unwrap())
}

/// `await (await service.createSyncTask(null)).run()`.
async fn run_sync(client: &UserDataSyncClient) {
    client.sync().await.unwrap();
}

/// As escritas de arquivos locais dos testes: settings, keybindings, snippets (`html.json`), prompts, argv e,
/// quando pedido, `tasks.json`.
async fn write_local_changes(client: &UserDataSyncClient, snippets: &str, prompt_name: &str, prompt_text: &str, tasks: bool) {
    let ext_uri = ExtUri::unbiased(Rc::new(ProcessEnvironment));
    let profile = client.user_data_profiles_service.default_profile();
    let files = &client.file_service;
    write_text(files, &profile.settings_resource, r#"{"editor.fontSize":14}"#).await.unwrap();
    write_text(files, &profile.keybindings_resource, r#"[{"command":"abcd","key":"cmd+c"}]"#).await.unwrap();
    write_text(files, &client.environment_service.argv_resource, r#"{"locale":"de"}"#).await.unwrap();
    write_text(files, &ext_uri.join_path(&profile.snippets_home, &["html.json"]), snippets).await.unwrap();
    write_text(files, &ext_uri.join_path(&profile.prompts_home, &[prompt_name]), prompt_text).await.unwrap();
    if tasks {
        write_text(files, &ext_uri.join_path(&ext_uri.dirname(&profile.settings_resource), &["tasks.json"]), "{}").await.unwrap();
    }
}

/// `fileService.writeFile(settingsResource, { 'editor.fontSize': size })`.
async fn write_font_size(client: &UserDataSyncClient, size: u32) {
    let resource = client.user_data_profiles_service.default_profile().settings_resource;
    write_text(&client.file_service, &resource, &format!(r#"{{"editor.fontSize":{size}}}"#)).await.unwrap();
}

const FIRST_SYNC_ALL: &[Expected] = &[
    // Manifest
    ("GET", "manifest", &[]),
    // Settings
    ("POST", "resource/settings", &[("If-Match", "0")]),
    // Keybindings
    ("POST", "resource/keybindings", &[("If-Match", "0")]),
    // Snippets
    ("POST", "resource/snippets", &[("If-Match", "0")]),
    // Tasks
    ("POST", "resource/tasks", &[("If-Match", "0")]),
    // Global state
    ("POST", "resource/globalState", &[("If-Match", "0")]),
    // Prompts
    ("POST", "resource/prompts", &[("If-Match", "0")]),
];

#[test]
fn test_first_time_sync_ever() {
    async_test(|d| async move {
        // Setup the client
        let target = Rc::new(UserDataSyncTestServer::new());
        let client = set_up(&d, &target, false).await;

        // Sync for first time
        run_sync(&client).await;

        assert_requests(&target, FIRST_SYNC_ALL);
    });
}

#[test]
fn test_first_time_sync_ever_when_a_sync_resource_is_disabled() {
    async_test(|d| async move {
        // Setup the client
        let target = Rc::new(UserDataSyncTestServer::new());
        let client = set_up(&d, &target, false).await;
        client.user_data_sync_enablement_service.set_resource_enablement(SyncResource::Settings, false);

        // Sync for first time
        run_sync(&client).await;

        assert_requests(
            &target,
            &[
                // Manifest
                ("GET", "manifest", &[]),
                // Keybindings
                ("POST", "resource/keybindings", &[("If-Match", "0")]),
                // Snippets
                ("POST", "resource/snippets", &[("If-Match", "0")]),
                // Snippets
                ("POST", "resource/tasks", &[("If-Match", "0")]),
                // Global state
                ("POST", "resource/globalState", &[("If-Match", "0")]),
                // Prompts
                ("POST", "resource/prompts", &[("If-Match", "0")]),
            ],
        );
    });
}

#[test]
fn test_first_time_sync_ever_with_no_data() {
    async_test(|d| async move {
        // Setup the client
        let target = Rc::new(UserDataSyncTestServer::new());
        let client = set_up(&d, &target, true).await;

        // Sync for first time
        run_sync(&client).await;

        assert_requests(
            &target,
            &[
                // Manifest
                ("GET", "manifest", &[]),
            ],
        );
    });
}

const MERGE_NO_CHANGES: &[Expected] = &[
    ("GET", "manifest", &[]),
    ("GET", "resource/settings/latest", &[]),
    ("GET", "resource/keybindings/latest", &[]),
    ("GET", "resource/snippets/latest", &[]),
    ("GET", "resource/tasks/latest", &[]),
    ("GET", "resource/globalState/latest", &[]),
    ("GET", "resource/prompts/latest", &[]),
];

#[test]
fn test_first_time_sync_from_the_client_with_no_changes_merge() {
    async_test(|d| async move {
        let target = Rc::new(UserDataSyncTestServer::new());

        // Setup and sync from the first client
        let client = set_up(&d, &target, false).await;
        run_sync(&client).await;

        // Setup the test client
        let test_client = set_up(&d, &target, false).await;

        // Sync (merge) from the test client
        target.reset();
        run_sync(&test_client).await;

        assert_requests(&target, MERGE_NO_CHANGES);
    });
}

const MERGE_WITH_CHANGES: &[Expected] = &[
    ("GET", "manifest", &[]),
    ("GET", "resource/settings/latest", &[]),
    ("POST", "resource/settings", &[("If-Match", "1")]),
    ("GET", "resource/keybindings/latest", &[]),
    ("POST", "resource/keybindings", &[("If-Match", "1")]),
    ("GET", "resource/snippets/latest", &[]),
    ("POST", "resource/snippets", &[("If-Match", "1")]),
    ("GET", "resource/tasks/latest", &[]),
    ("GET", "resource/globalState/latest", &[]),
    ("GET", "resource/prompts/latest", &[]),
    ("POST", "resource/prompts", &[("If-Match", "1")]),
];

#[test]
fn test_first_time_sync_from_the_client_with_changes_merge() {
    async_test(|d| async move {
        let target = Rc::new(UserDataSyncTestServer::new());

        // Setup and sync from the first client
        let client = set_up(&d, &target, false).await;
        run_sync(&client).await;

        // Setup the test client with changes
        let test_client = set_up(&d, &target, false).await;
        write_local_changes(&test_client, "{}", "mine.prompt.md", "text", true).await;

        // Sync (merge) from the test client
        target.reset();
        run_sync(&test_client).await;

        assert_requests(&target, MERGE_WITH_CHANGES);
    });
}

#[test]
fn test_first_time_sync_from_the_client_with_changes_merge_with_profile() {
    async_test(|d| async move {
        let target = Rc::new(UserDataSyncTestServer::new());

        // Setup and sync from the first client
        let client = set_up(&d, &target, false).await;
        run_sync(&client).await;

        // Setup the test client with changes
        let test_client = set_up(&d, &target, false).await;
        test_client.user_data_profiles_service.create_named_profile("1", None, None).await.unwrap();
        write_local_changes(&test_client, "{}", "my.prompt.md", "some prompt text", true).await;

        // Sync (merge) from the test client
        target.reset();
        run_sync(&test_client).await;

        let mut expected = MERGE_WITH_CHANGES.to_vec();
        expected.push(("POST", "collection", &[]));
        expected.push(("POST", "resource/profiles", &[("If-Match", "0")]));
        assert_requests(&target, &expected);
    });
}

#[test]
fn test_sync_when_there_are_no_changes() {
    async_test(|d| async move {
        let target = Rc::new(UserDataSyncTestServer::new());

        // Setup and sync from the client
        let client = set_up(&d, &target, false).await;
        run_sync(&client).await;

        // sync from the client again
        target.reset();
        run_sync(&client).await;

        assert_requests(
            &target,
            &[
                // Manifest
                ("GET", "manifest", &[]),
            ],
        );
    });
}

const LOCAL_CHANGES: &[Expected] = &[
    // Manifest
    ("GET", "manifest", &[]),
    // Settings
    ("POST", "resource/settings", &[("If-Match", "1")]),
    // Keybindings
    ("POST", "resource/keybindings", &[("If-Match", "1")]),
    // Snippets
    ("POST", "resource/snippets", &[("If-Match", "1")]),
    // Global state
    ("POST", "resource/globalState", &[("If-Match", "1")]),
    // Prompts
    ("POST", "resource/prompts", &[("If-Match", "1")]),
];

#[test]
fn test_sync_when_there_are_local_changes() {
    async_test(|d| async move {
        let target = Rc::new(UserDataSyncTestServer::new());

        // Setup and sync from the client
        let client = set_up(&d, &target, false).await;
        run_sync(&client).await;
        target.reset();

        // Do changes in the client
        write_local_changes(&client, "{}", "shared.prompt.md", "prompt text", false).await;

        // Sync from the client
        run_sync(&client).await;

        assert_requests(&target, LOCAL_CHANGES);
    });
}

#[test]
fn test_sync_when_there_are_local_changes_with_profile() {
    async_test(|d| async move {
        let target = Rc::new(UserDataSyncTestServer::new());

        // Setup and sync from the client
        let client = set_up(&d, &target, false).await;
        run_sync(&client).await;
        target.reset();

        // Do changes in the client
        client.user_data_profiles_service.create_named_profile("1", None, None).await.unwrap();
        write_local_changes(&client, "{}", "default.prompt.md", "some prompt file contents", false).await;

        // Sync from the client
        run_sync(&client).await;

        let mut expected = LOCAL_CHANGES.to_vec();
        // Profiles
        expected.push(("POST", "collection", &[]));
        expected.push(("POST", "resource/profiles", &[("If-Match", "0")]));
        assert_requests(&target, &expected);
    });
}

#[test]
fn test_sync_when_there_are_local_changes_and_sync_resource_is_disabled() {
    async_test(|d| async move {
        let target = Rc::new(UserDataSyncTestServer::new());

        // Setup and sync from the client
        let client = set_up(&d, &target, false).await;
        run_sync(&client).await;
        target.reset();

        // Do changes in the client
        write_local_changes(&client, "{}", "1.prompt.md", "random prompt text", false).await;
        client.user_data_sync_enablement_service.set_resource_enablement(SyncResource::Snippets, false);
        client.user_data_sync_enablement_service.set_resource_enablement(SyncResource::Prompts, false);

        // Sync from the client
        run_sync(&client).await;

        assert_requests(
            &target,
            &[
                // Manifest
                ("GET", "manifest", &[]),
                // Settings
                ("POST", "resource/settings", &[("If-Match", "1")]),
                // Keybindings
                ("POST", "resource/keybindings", &[("If-Match", "1")]),
                // Global state
                ("POST", "resource/globalState", &[("If-Match", "1")]),
            ],
        );
    });
}

const REMOTE_CHANGES: &[Expected] = &[
    // Manifest
    ("GET", "manifest", &[]),
    // Settings
    ("GET", "resource/settings/latest", &[("If-None-Match", "1")]),
    // Keybindings
    ("GET", "resource/keybindings/latest", &[("If-None-Match", "1")]),
    // Snippets
    ("GET", "resource/snippets/latest", &[("If-None-Match", "1")]),
    // Global state
    ("GET", "resource/globalState/latest", &[("If-None-Match", "1")]),
    // Prompts
    ("GET", "resource/prompts/latest", &[("If-None-Match", "1")]),
];

#[test]
fn test_sync_when_there_are_remote_changes() {
    async_test(|d| async move {
        let target = Rc::new(UserDataSyncTestServer::new());

        // Sync from first client
        let client = set_up(&d, &target, false).await;
        run_sync(&client).await;

        // Sync from test client
        let test_client = set_up(&d, &target, false).await;
        run_sync(&test_client).await;

        // Do changes in first client and sync
        write_local_changes(&client, r#"{ "a": "changed" }"#, "unknown.prompt.md", "prompt text", false).await;
        run_sync(&client).await;

        // Sync from test client
        target.reset();
        run_sync(&test_client).await;

        assert_requests(&target, REMOTE_CHANGES);
    });
}

#[test]
fn test_sync_when_there_are_remote_changes_with_profile() {
    async_test(|d| async move {
        let target = Rc::new(UserDataSyncTestServer::new());

        // Sync from first client
        let client = set_up(&d, &target, false).await;
        run_sync(&client).await;

        // Sync from test client
        let test_client = set_up(&d, &target, false).await;
        run_sync(&test_client).await;

        // Do changes in first client and sync
        client.user_data_profiles_service.create_named_profile("1", None, None).await.unwrap();
        write_local_changes(&client, r#"{ "a": "changed" }"#, "global.prompt.md", "some text goes here", false).await;
        run_sync(&client).await;

        // Sync from test client
        target.reset();
        run_sync(&test_client).await;

        let mut expected = REMOTE_CHANGES.to_vec();
        // Profiles
        expected.push(("GET", "resource/profiles/latest", &[("If-None-Match", "0")]));
        assert_requests(&target, &expected);
    });
}

#[test]
fn test_delete() {
    async_test(|d| async move {
        let target = Rc::new(UserDataSyncTestServer::new());

        // Sync from the client
        let test_client = set_up(&d, &target, false).await;
        run_sync(&test_client).await;

        // Reset from the client
        target.reset();
        test_client.user_data_sync_service.reset().await.unwrap();

        assert_requests(
            &target,
            &[
                // Manifest
                ("DELETE", "collection", &[]),
                ("DELETE", "resource", &[]),
            ],
        );
    });
}

#[test]
fn test_delete_and_sync() {
    async_test(|d| async move {
        let target = Rc::new(UserDataSyncTestServer::new());

        // Sync from the client
        let test_client = set_up(&d, &target, false).await;
        run_sync(&test_client).await;

        // Reset from the client
        test_client.user_data_sync_service.reset().await.unwrap();

        // Sync again
        target.reset();
        run_sync(&test_client).await;

        assert_requests(&target, FIRST_SYNC_ALL);
    });
}

#[test]
fn test_sync_status() {
    async_test(|d| async move {
        let target = Rc::new(UserDataSyncTestServer::new());

        // Setup the client
        let client = set_up(&d, &target, false).await;

        // sync from the client
        let actual_statuses = Rc::new(RefCell::new(Vec::<SyncStatus>::new()));
        let recorded = actual_statuses.clone();
        let disposable = client.user_data_sync_service.on_did_change_status().listen(move |status| recorded.borrow_mut().push(*status));
        run_sync(&client).await;

        disposable.dispose();
        let syncing_then_idle = [SyncStatus::Syncing, SyncStatus::Idle];
        assert_eq!(*actual_statuses.borrow(), syncing_then_idle.repeat(9));
    });
}

#[test]
fn test_sync_conflicts_status() {
    async_test(|d| async move {
        let target = Rc::new(UserDataSyncTestServer::new());

        // Setup and sync from the first client
        let client = set_up(&d, &target, false).await;
        write_font_size(&client, 14).await;
        run_sync(&client).await;

        // Setup the test client
        let test_client = set_up(&d, &target, false).await;
        write_font_size(&test_client, 16).await;
        let test_object = &test_client.user_data_sync_service;

        // sync from the client
        run_sync(&test_client).await;

        assert_eq!(test_object.status(), SyncStatus::HasConflicts);
        assert_eq!(test_object.conflicts().into_iter().map(|conflicts| conflicts.sync_resource).collect::<Vec<_>>(), vec![SyncResource::Settings]);
    });
}

#[test]
fn test_sync_will_sync_other_non_conflicted_areas() {
    async_test(|d| async move {
        let target = Rc::new(UserDataSyncTestServer::new());

        // Setup and sync from the first client
        let client = set_up(&d, &target, false).await;
        write_font_size(&client, 14).await;
        run_sync(&client).await;

        // Setup the test client and get conflicts in settings
        let test_client = set_up(&d, &target, false).await;
        write_font_size(&test_client, 16).await;
        let test_object = &test_client.user_data_sync_service;
        run_sync(&test_client).await;

        // sync from the first client with changes in keybindings
        let keybindings_resource = test_client.user_data_profiles_service.default_profile().keybindings_resource;
        write_text(&client.file_service, &keybindings_resource, r#"[{"command":"abcd","key":"cmd+c"}]"#).await.unwrap();
        run_sync(&client).await;

        // sync from the test client
        target.reset();
        let actual_statuses = Rc::new(RefCell::new(Vec::<SyncStatus>::new()));
        let recorded = actual_statuses.clone();
        let disposable = test_object.on_did_change_status().listen(move |status| recorded.borrow_mut().push(*status));
        run_sync(&test_client).await;

        disposable.dispose();
        assert_eq!(*actual_statuses.borrow(), Vec::<SyncStatus>::new());
        assert_eq!(test_object.status(), SyncStatus::HasConflicts);

        assert_requests(
            &target,
            &[
                // Manifest
                ("GET", "manifest", &[]),
                // Keybindings
                ("GET", "resource/keybindings/latest", &[("If-None-Match", "1")]),
            ],
        );
    });
}

#[test]
fn test_stop_sync_reset_status() {
    async_test(|d| async move {
        let target = Rc::new(UserDataSyncTestServer::new());

        // Setup and sync from the first client
        let client = set_up(&d, &target, false).await;
        write_font_size(&client, 14).await;
        run_sync(&client).await;

        // Setup the test client
        let test_client = set_up(&d, &target, false).await;
        write_font_size(&test_client, 16).await;
        let test_object = &test_client.user_data_sync_service;

        let sync_task = test_object.create_sync_task(None, None).await.unwrap();
        let run = sync_task.run();
        sync_task.stop().await.unwrap();

        assert_eq!(test_object.status(), SyncStatus::Idle);
        assert!(test_object.conflicts().is_empty());

        // ignore error
        let _ = run.await;
    });
}

#[test]
fn test_sync_send_execution_id_header() {
    async_test(|d| async move {
        // Setup the client
        let target = Rc::new(UserDataSyncTestServer::new());
        let client = set_up(&d, &target, false).await;

        run_sync(&client).await;

        for request in target.requests_with_all_headers() {
            let has_execution_id_header = match request.headers.as_ref().and_then(|headers| headers.get("X-Execution-Id")) {
                Some(HeaderValue::One(execution_id)) => !execution_id.is_empty(),
                Some(_) => true,
                None => false,
            };
            assert!(has_execution_id_header, "Should have execution header: {}", request.url);
        }
    });
}

#[test]
fn test_can_run_sync_taks_only_once() {
    async_test(|d| async move {
        // Setup the client
        let target = Rc::new(UserDataSyncTestServer::new());
        let client = set_up(&d, &target, false).await;

        let sync_task = client.user_data_sync_service.create_sync_task(None, None).await.unwrap();
        sync_task.run().await.unwrap();

        // The second run may fail or not: the original catches the `assert.fail` it throws
        let _ = sync_task.run().await;
    });
}

#[test]
fn test_sync_when_there_are_local_profile_that_uses_default_profile() {
    async_test(|d| async move {
        let target = Rc::new(UserDataSyncTestServer::new());

        // Setup and sync from the client
        let client = set_up(&d, &target, false).await;
        run_sync(&client).await;
        target.reset();

        // Do changes in the client
        let options = UserDataProfileOptions { use_default_flags: Some(UseDefaultProfileFlags { settings: Some(true), ..Default::default() }), ..Default::default() };
        client.user_data_profiles_service.create_named_profile("1", Some(options), None).await.unwrap();
        write_local_changes(&client, "{}", "2.prompt.md", "file contents", false).await;

        // Sync from the client
        run_sync(&client).await;

        let mut expected = LOCAL_CHANGES.to_vec();
        // Profiles
        expected.push(("POST", "collection", &[]));
        expected.push(("POST", "resource/profiles", &[("If-Match", "0")]));
        assert_requests(&target, &expected);
    });
}

#[test]
fn test_sync_when_there_is_a_remote_profile_that_uses_default_profile() {
    async_test(|d| async move {
        let target = Rc::new(UserDataSyncTestServer::new());

        // Sync from first client
        let client = set_up(&d, &target, false).await;
        run_sync(&client).await;

        // Sync from test client
        let test_client = set_up(&d, &target, false).await;
        run_sync(&test_client).await;

        // Do changes in first client and sync
        let options = UserDataProfileOptions { use_default_flags: Some(UseDefaultProfileFlags { keybindings: Some(true), ..Default::default() }), ..Default::default() };
        client.user_data_profiles_service.create_named_profile("1", Some(options), None).await.unwrap();
        write_local_changes(&client, r#"{ "a": "changed" }"#, "best.prompt.md", "prompt prompt", false).await;
        run_sync(&client).await;

        // Sync from test client
        target.reset();
        run_sync(&test_client).await;

        let mut expected = REMOTE_CHANGES.to_vec();
        // Profiles
        expected.push(("GET", "resource/profiles/latest", &[("If-None-Match", "0")]));
        assert_requests(&target, &expected);
    });
}
