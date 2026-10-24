//! Porte de `src/vs/platform/userDataSync/test/common/userDataAutoSyncService.test.ts` (suíte
//! `UserDataAutoSyncService`), caso a caso e na mesma ordem.
//!
//! Divergências: `TestUserDataAutoSyncService` (a subclasse que desliga o início automático e fixa o atraso em
//! 50 ms) é o [`UserDataAutoSyncService`] com [`UserDataAutoSyncOverrides`], e o `sync()` dela é [`sync`]. O
//! `IMeteredConnectionService` do cliente de teste (`isConnectionMetered: false` e um evento mudo) é
//! [`NotMeteredConnection`]. O `setUp()` do original é `set_up(.., false)` (arquivos locais criados). As
//! comparações de `deepStrictEqual` das requisições são sobre [`RecordedRequest`]. Os testes rodam sob
//! [`async_test`] (`runWithFakedTimers({})`).

mod common;

use std::rc::Rc;

use common::files::async_test;
use common::user_data_sync_client::{RecordedRequest, UserDataSyncClient, UserDataSyncTestServer};
use common::write_text;
use je_base::event::Event;
use je_base::lifecycle::DisposableStore;
use je_base::parts::request::{HeaderValue, IHeaders};
use je_base::promise::Promise;
use je_platform::log::NullLogService;
use je_platform::metered_connection::IMeteredConnectionService;
use je_platform::resources::join_path;
use je_platform::user_data_profile::IUserDataProfilesService;
use je_platform::user_data_sync::user_data_auto_sync_service::{UserDataAutoSyncOverrides, UserDataAutoSyncService, UserDataAutoSyncServices};
use je_platform::user_data_sync::user_data_sync::{SyncOptions, SyncResource, UserDataAutoSyncError, UserDataSyncErrorCode, UserDataSyncStoreError};
use je_platform::user_data_sync::user_data_sync_machines::IUserDataSyncMachinesService;

/// `{ isConnectionMetered: false, onDidChangeIsConnectionMetered: new Emitter<boolean>().event }`.
struct NotMeteredConnection;

impl IMeteredConnectionService for NotMeteredConnection {
    fn is_connection_metered(&self) -> bool {
        false
    }

    fn on_did_change_is_connection_metered(&self) -> Event<bool> {
        Event::none()
    }
}

/// `client.instantiationService.createInstance(TestUserDataAutoSyncService)`.
fn create_test_object(client: &UserDataSyncClient) -> Rc<UserDataAutoSyncService> {
    UserDataAutoSyncService::new(
        UserDataAutoSyncServices {
            product_service: client.product_service.clone(),
            user_data_sync_store_management_service: client.user_data_sync_store_management_service.clone(),
            user_data_sync_store_service: client.user_data_sync_store_service.clone(),
            user_data_sync_enablement_service: client.user_data_sync_enablement_service.clone(),
            user_data_sync_service: client.user_data_sync_service.clone(),
            log_service: NullLogService::shared(),
            user_data_sync_account_service: client.user_data_sync_account_service.clone(),
            telemetry_service: client.telemetry_service.clone(),
            user_data_sync_machines_service: client.user_data_sync_machines_service.clone(),
            storage_service: client.storage_service.clone(),
            metered_connection_service: Rc::new(NotMeteredConnection),
        },
        UserDataAutoSyncOverrides { start_auto_sync: false, sync_trigger_delay_time: Some(50) },
    )
}

/// `testObject.sync()`: `this.triggerSync(['sync'])`.
fn sync(test_object: &UserDataAutoSyncService) -> Promise<()> {
    test_object.trigger_sync(&["sync".to_string()], None)
}

/// `{ type, url, headers }`.
fn request(r#type: &str, url: String, headers: &[(&str, &str)]) -> RecordedRequest {
    RecordedRequest {
        url,
        r#type: r#type.to_string(),
        headers: headers.iter().map(|(name, value)| (name.to_string(), HeaderValue::One(value.to_string()))).collect::<IHeaders>(),
    }
}

/// `${target.url}/v1/<path>`.
fn url(target: &UserDataSyncTestServer, path: &str) -> String {
    format!("{}/v1/{path}", target.url)
}

/// `target.requests.filter(request => !request.url.startsWith(`${target.url}/v1/resource/machines`))`.
fn requests_without_machines(target: &UserDataSyncTestServer) -> Vec<RecordedRequest> {
    let machines = url(target, "resource/machines");
    target.requests().into_iter().filter(|request| !request.url.starts_with(&machines)).collect()
}

async fn set_up(d: &Rc<DisposableStore>, target: &Rc<UserDataSyncTestServer>) -> Rc<UserDataSyncClient> {
    d.add_rc(UserDataSyncClient::set_up(target.clone(), false).await.unwrap())
}

/// `await (await client.instantiationService.get(IUserDataSyncService).createSyncTask(null)).run()`.
async fn sync_once(client: &UserDataSyncClient) {
    client.sync().await.unwrap();
}

#[test]
fn auto_sync_with_sync_resource_change_triggers_sync() {
    async_test(|d| async move {
        // Setup the client
        let target = Rc::new(UserDataSyncTestServer::new());
        let client = set_up(&d, &target).await;

        // Sync once and reset requests
        sync_once(&client).await;
        target.reset();

        let test_object = d.add_rc(create_test_object(&client));

        // Trigger auto sync with settings change
        test_object.trigger_sync(&[SyncResource::Settings.as_str().to_string()], None).await.unwrap();

        // Filter out machine requests
        let actual = requests_without_machines(&target);

        // Make sure only one manifest request is made
        assert_eq!(actual, vec![request("GET", url(&target, "manifest"), &[])]);
    });
}

#[test]
fn auto_sync_with_sync_resource_change_triggers_sync_for_every_change() {
    async_test(|d| async move {
        // Setup the client
        let target = Rc::new(UserDataSyncTestServer::new());
        let client = set_up(&d, &target).await;

        // Sync once and reset requests
        sync_once(&client).await;
        target.reset();

        let test_object = d.add_rc(create_test_object(&client));

        // Trigger auto sync with settings change multiple times
        for _counter in 0..2 {
            test_object.trigger_sync(&[SyncResource::Settings.as_str().to_string()], None).await.unwrap();
        }

        // Filter out machine requests
        let actual = requests_without_machines(&target);

        assert_eq!(actual, vec![request("GET", url(&target, "manifest"), &[]), request("GET", url(&target, "manifest"), &[("If-None-Match", "1")])]);
    });
}

#[test]
fn auto_sync_with_non_sync_resource_change_triggers_sync() {
    async_test(|d| async move {
        // Setup the client
        let target = Rc::new(UserDataSyncTestServer::new());
        let client = set_up(&d, &target).await;

        // Sync once and reset requests
        sync_once(&client).await;
        target.reset();

        let test_object = d.add_rc(create_test_object(&client));

        // Trigger auto sync with window focus once
        test_object.trigger_sync(&["windowFocus".to_string()], None).await.unwrap();

        // Filter out machine requests
        let actual = requests_without_machines(&target);

        // Make sure only one manifest request is made
        assert_eq!(actual, vec![request("GET", url(&target, "manifest"), &[])]);
    });
}

#[test]
fn auto_sync_with_non_sync_resource_change_does_not_trigger_continuous_syncs() {
    async_test(|d| async move {
        // Setup the client
        let target = Rc::new(UserDataSyncTestServer::new());
        let client = set_up(&d, &target).await;

        // Sync once and reset requests
        sync_once(&client).await;
        target.reset();

        let test_object = d.add_rc(create_test_object(&client));

        // Trigger auto sync with window focus multiple times
        for _counter in 0..2 {
            test_object.trigger_sync(&["windowFocus".to_string()], Some(SyncOptions { skip_if_synced_recently: Some(true), ..SyncOptions::default() })).await.unwrap();
        }

        // Filter out machine requests
        let actual = requests_without_machines(&target);

        // Make sure only one manifest request is made
        assert_eq!(actual, vec![request("GET", url(&target, "manifest"), &[])]);
    });
}

#[test]
fn first_auto_sync_requests() {
    async_test(|d| async move {
        // Setup the client
        let target = Rc::new(UserDataSyncTestServer::new());
        let client = set_up(&d, &target).await;
        let test_object = d.add_rc(create_test_object(&client));

        sync(&test_object).await.unwrap();

        assert_eq!(
            target.requests(),
            vec![
                // Manifest
                request("GET", url(&target, "manifest"), &[]),
                // Machines
                request("GET", url(&target, "resource/machines/latest"), &[]),
                // Settings
                request("POST", url(&target, "resource/settings"), &[("If-Match", "0")]),
                // Keybindings
                request("POST", url(&target, "resource/keybindings"), &[("If-Match", "0")]),
                // Snippets
                request("POST", url(&target, "resource/snippets"), &[("If-Match", "0")]),
                // Tasks
                request("POST", url(&target, "resource/tasks"), &[("If-Match", "0")]),
                // Global state
                request("POST", url(&target, "resource/globalState"), &[("If-Match", "0")]),
                // Prompts
                request("POST", url(&target, "resource/prompts"), &[("If-Match", "0")]),
                // Manifest
                request("GET", url(&target, "manifest"), &[]),
                // Machines
                request("POST", url(&target, "resource/machines"), &[("If-Match", "0")]),
            ]
        );
    });
}

#[test]
fn further_auto_sync_requests_without_changes() {
    async_test(|d| async move {
        // Setup the client
        let target = Rc::new(UserDataSyncTestServer::new());
        let client = set_up(&d, &target).await;
        let test_object = d.add_rc(create_test_object(&client));

        // Sync once and reset requests
        sync(&test_object).await.unwrap();
        target.reset();

        sync(&test_object).await.unwrap();

        assert_eq!(
            target.requests(),
            vec![
                // Manifest
                request("GET", url(&target, "manifest"), &[("If-None-Match", "1")]),
            ]
        );
    });
}

#[test]
fn further_auto_sync_requests_with_changes() {
    async_test(|d| async move {
        // Setup the client
        let target = Rc::new(UserDataSyncTestServer::new());
        let client = set_up(&d, &target).await;
        let test_object = d.add_rc(create_test_object(&client));

        // Sync once and reset requests
        sync(&test_object).await.unwrap();
        target.reset();

        // Do changes in the client
        let file_service = &client.file_service;
        let default_profile = client.user_data_profiles_service.default_profile();
        write_text(file_service, &default_profile.settings_resource, r#"{"editor.fontSize":14}"#).await.unwrap();
        write_text(file_service, &default_profile.keybindings_resource, r#"[{"command":"abcd","key":"cmd+c"}]"#).await.unwrap();
        write_text(file_service, &join_path(&default_profile.snippets_home, &["html.json"]), "{}").await.unwrap();
        write_text(file_service, &join_path(&default_profile.prompts_home, &["h1.prompt.md"]), " ").await.unwrap();
        write_text(file_service, &client.environment_service.argv_resource, r#"{"locale":"de"}"#).await.unwrap();
        sync(&test_object).await.unwrap();

        assert_eq!(
            target.requests(),
            vec![
                // Manifest
                request("GET", url(&target, "manifest"), &[("If-None-Match", "1")]),
                // Settings
                request("POST", url(&target, "resource/settings"), &[("If-Match", "1")]),
                // Keybindings
                request("POST", url(&target, "resource/keybindings"), &[("If-Match", "1")]),
                // Snippets
                request("POST", url(&target, "resource/snippets"), &[("If-Match", "1")]),
                // Global state
                request("POST", url(&target, "resource/globalState"), &[("If-Match", "1")]),
                // Prompts
                request("POST", url(&target, "resource/prompts"), &[("If-Match", "1")]),
            ]
        );
    });
}

#[test]
fn auto_sync_send_execution_id_header() {
    async_test(|d| async move {
        // Setup the client
        let target = Rc::new(UserDataSyncTestServer::new());
        let client = set_up(&d, &target).await;
        let test_object = d.add_rc(create_test_object(&client));

        // Sync once and reset requests
        sync(&test_object).await.unwrap();
        target.reset();

        sync(&test_object).await.unwrap();

        let machines = url(&target, "resource/machines");
        for request in target.requests_with_all_headers() {
            let has_execution_id_header = request.headers.as_ref().and_then(|headers| headers.get("X-Execution-Id")).is_some_and(|value| match value {
                HeaderValue::One(text) => !text.is_empty(),
                HeaderValue::Many(items) => !items.is_empty(),
            });
            if request.url.starts_with(&machines) {
                assert!(!has_execution_id_header, "Should not have execution header: {}", request.url);
            } else {
                assert!(has_execution_id_header, "Should have execution header: {}", request.url);
            }
        }
    });
}

#[test]
fn delete_on_one_client_throws_turned_off_error_on_other_client_while_syncing() {
    async_test(|d| async move {
        let target = Rc::new(UserDataSyncTestServer::new());

        // Set up and sync from the client
        let client = set_up(&d, &target).await;
        sync_once(&client).await;

        // Set up and sync from the test client
        let test_client = set_up(&d, &target).await;
        let test_object = d.add_rc(create_test_object(&test_client));
        sync(&test_object).await.unwrap();

        // Reset from the first client
        client.user_data_sync_service.reset().await.unwrap();

        // Sync from the test client
        target.reset();

        let error_promise = test_object.on_error().to_promise(None);
        sync(&test_object).await.unwrap();

        let e = error_promise.await.unwrap();
        let auto_sync_error = UserDataAutoSyncError::from_error(&e).expect("é um UserDataAutoSyncError");
        assert_eq!(auto_sync_error.error.code, UserDataSyncErrorCode::TurnedOff);
        assert_eq!(
            target.requests(),
            vec![
                // Manifest
                request("GET", url(&target, "manifest"), &[("If-None-Match", "1")]),
                // Machine
                request("GET", url(&target, "resource/machines/latest"), &[("If-None-Match", "1")]),
            ]
        );
    });
}

#[test]
fn disabling_the_machine_turns_off_sync() {
    async_test(|d| async move {
        let target = Rc::new(UserDataSyncTestServer::new());

        // Set up and sync from the test client
        let test_client = set_up(&d, &target).await;
        let test_object = d.add_rc(create_test_object(&test_client));
        sync(&test_object).await.unwrap();

        // Disable current machine
        let user_data_sync_machines_service = &test_client.user_data_sync_machines_service;
        let machines = user_data_sync_machines_service.get_machines(None).await.unwrap();
        let current_machine = machines.into_iter().find(|machine| machine.is_current).expect("a máquina atual existe");
        user_data_sync_machines_service.set_enablements(&[(current_machine.id, false)]).await.unwrap();

        target.reset();

        let error_promise = test_object.on_error().to_promise(None);
        sync(&test_object).await.unwrap();

        let e = error_promise.await.unwrap();
        let auto_sync_error = UserDataAutoSyncError::from_error(&e).expect("é um UserDataAutoSyncError");
        assert_eq!(auto_sync_error.error.code, UserDataSyncErrorCode::TurnedOff);
        assert_eq!(
            target.requests(),
            vec![
                // Manifest
                request("GET", url(&target, "manifest"), &[("If-None-Match", "1")]),
                // Machine
                request("GET", url(&target, "resource/machines/latest"), &[("If-None-Match", "2")]),
                request("POST", url(&target, "resource/machines"), &[("If-Match", "2")]),
            ]
        );
    });
}

#[test]
fn removing_the_machine_adds_machine_back() {
    async_test(|d| async move {
        let target = Rc::new(UserDataSyncTestServer::new());

        // Set up and sync from the test client
        let test_client = set_up(&d, &target).await;
        let test_object = d.add_rc(create_test_object(&test_client));
        sync(&test_object).await.unwrap();

        // Remove current machine
        test_client.user_data_sync_machines_service.remove_current_machine(None).await.unwrap();

        target.reset();

        sync(&test_object).await.unwrap();
        assert_eq!(
            target.requests(),
            vec![
                // Manifest
                request("GET", url(&target, "manifest"), &[("If-None-Match", "1")]),
                // Machine
                request("POST", url(&target, "resource/machines"), &[("If-Match", "2")]),
            ]
        );
    });
}

#[test]
fn creating_new_session_from_one_client_throws_session_expired_error_on_another_client_while_syncing() {
    async_test(|d| async move {
        let target = Rc::new(UserDataSyncTestServer::new());

        // Set up and sync from the client
        let client = set_up(&d, &target).await;
        sync_once(&client).await;

        // Set up and sync from the test client
        let test_client = set_up(&d, &target).await;
        let test_object = d.add_rc(create_test_object(&test_client));
        sync(&test_object).await.unwrap();

        // Reset from the first client
        client.user_data_sync_service.reset().await.unwrap();

        // Sync again from the first client to create new session
        sync_once(&client).await;

        // Sync from the test client
        target.reset();

        let error_promise = test_object.on_error().to_promise(None);
        sync(&test_object).await.unwrap();

        let e = error_promise.await.unwrap();
        let auto_sync_error = UserDataAutoSyncError::from_error(&e).expect("é um UserDataAutoSyncError");
        assert_eq!(auto_sync_error.error.code, UserDataSyncErrorCode::SessionExpired);
        assert_eq!(
            target.requests(),
            vec![
                // Manifest
                request("GET", url(&target, "manifest"), &[("If-None-Match", "1")]),
                // Machine
                request("GET", url(&target, "resource/machines/latest"), &[("If-None-Match", "1")]),
            ]
        );
    });
}

#[test]
fn rate_limit_on_server() {
    async_test(|d| async move {
        let target = Rc::new(UserDataSyncTestServer::with_rate_limit(5, None));

        // Set up and sync from the test client
        let test_client = set_up(&d, &target).await;
        let test_object = d.add_rc(create_test_object(&test_client));

        let error_promise = test_object.on_error().to_promise(None);
        while target.requests().len() < 5 {
            sync(&test_object).await.unwrap();
        }

        let e = error_promise.await.unwrap();
        let store_error = UserDataSyncStoreError::from_error(&e).expect("é um UserDataSyncStoreError");
        assert_eq!(store_error.error.code, UserDataSyncErrorCode::TooManyRequests);
    });
}

#[test]
fn auto_sync_is_suspended_when_server_donot_accepts_requests() {
    async_test(|d| async move {
        let target = Rc::new(UserDataSyncTestServer::with_rate_limit(5, Some(1)));

        // Set up and sync from the test client
        let test_client = set_up(&d, &target).await;
        let test_object = d.add_rc(create_test_object(&test_client));

        while target.requests().len() < 5 {
            sync(&test_object).await.unwrap();
        }

        target.reset();
        sync(&test_object).await.unwrap();

        assert_eq!(target.requests(), Vec::<RecordedRequest>::new());
    });
}

#[test]
fn cache_control_header_with_no_cache_is_sent_when_triggered_with_disable_cache_option() {
    async_test(|d| async move {
        let target = Rc::new(UserDataSyncTestServer::with_rate_limit(5, Some(1)));

        // Set up and sync from the test client
        let test_client = set_up(&d, &target).await;
        let test_object = d.add_rc(create_test_object(&test_client));

        test_object.trigger_sync(&["some reason".to_string()], Some(SyncOptions { disable_cache: Some(true), ..SyncOptions::default() })).await.unwrap();
        let first = target.requests_with_all_headers().into_iter().next().expect("há uma requisição");
        assert_eq!(first.headers.as_ref().and_then(|headers| headers.get("Cache-Control")), Some(&HeaderValue::One("no-cache".to_string())));
    });
}

#[test]
fn cache_control_header_is_not_sent_when_triggered_without_disable_cache_option() {
    async_test(|d| async move {
        let target = Rc::new(UserDataSyncTestServer::with_rate_limit(5, Some(1)));

        // Set up and sync from the test client
        let test_client = set_up(&d, &target).await;
        let test_object = d.add_rc(create_test_object(&test_client));

        test_object.trigger_sync(&["some reason".to_string()], None).await.unwrap();
        let first = target.requests_with_all_headers().into_iter().next().expect("há uma requisição");
        assert_eq!(first.headers.as_ref().and_then(|headers| headers.get("Cache-Control")), None);
    });
}
