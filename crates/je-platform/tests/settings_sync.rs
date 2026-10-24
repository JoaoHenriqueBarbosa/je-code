//! Porte de `src/vs/platform/userDataSync/test/common/settingsSync.test.ts` (as suítes `SettingsSync - Auto` e
//! `SettingsSync - Manual`), caso a caso e na mesma ordem.
//!
//! Divergências: o `setup` (o registro das settings de máquina, o cliente e o sincronizador de settings) roda no
//! começo do corpo de cada teste e o `teardown` (`clear` do servidor) no fim; o servidor é um por teste (o `clear` do
//! `teardown` o devolve ao estado inicial no original). Cada teste roda numa thread própria, com o registro de
//! configuração dela: o registro das settings de máquina da suíte `Auto` não vaza para a `Manual`, como no original
//! (que só o registra no `setup` da primeira). `testObject.sync(manifest)` usa os argumentos padrão do original
//! (`preview` falso, configuração e cabeçalhos vazios). `JSON.parse` do conteúdo vira [`parse_json`] e a comparação de
//! valores JSON; `JSON.stringify` de um literal é o texto compacto escrito na mão. Os literais de várias linhas são
//! listas de linhas com `\t`, para o recuo ficar explícito.

mod common;

use std::rc::Rc;

use common::files::async_test;
use common::user_data_sync_client::{RecordedRequest, UserDataSyncClient, UserDataSyncTestServer};
use common::watcher::to_promise;
use common::{create_text, if_match_headers, node, prop_with, read_text, sync_synchroniser, write_text};
use je_base::json::Value;
use je_base::lifecycle::DisposableStore;
use je_base::parts::request::IHeaders;
use je_platform::configuration::registry::{ConfigurationScope, configuration_registry};
use je_platform::user_data_profile::{IUserDataProfilesService, UserDataProfile};
use je_platform::user_data_sync::abstract_synchronizer::parse_json;
use je_platform::user_data_sync::settings_sync::{SettingsSynchroniser, parse_settings_sync_content};
use je_platform::user_data_sync::user_data_sync::{ISyncData, IUserDataSyncStoreService, IUserDataSynchroniser, SyncResource, SyncStatus, UserDataSyncError, UserDataSyncErrorCode};

struct Fixture {
    server: Rc<UserDataSyncTestServer>,
    client: Rc<UserDataSyncClient>,
    test_object: Rc<SettingsSynchroniser>,
}

/// `Registry.as<IConfigurationRegistry>(Extensions.Configuration).registerConfiguration({ id: 'settingsSync', ... })`
/// do `setup` da suíte `Auto`.
fn register_settings_sync_configuration() {
    configuration_registry().register_configuration(node(
        "settingsSync",
        vec![
            ("settingsSync.machine", prop_with("{ 'type': 'string' }", vec![("scope", ConfigurationScope::Machine.to_dynamic())])),
            ("settingsSync.machineOverridable", prop_with("{ 'type': 'string' }", vec![("scope", ConfigurationScope::MachineOverridable.to_dynamic())])),
        ],
    ));
}

/// O `setup` da suíte.
async fn setup(disposables: &Rc<DisposableStore>) -> Fixture {
    let server = Rc::new(UserDataSyncTestServer::new());
    let client = disposables.add_rc(UserDataSyncClient::set_up(server.clone(), true).await.unwrap());
    let test_object = client.get_synchronizer(SyncResource::Settings).into_settings().expect("o sincronizador de settings está ativo");
    Fixture { server, client, test_object }
}

/// O `setup` da suíte `Auto`: registra as settings de máquina antes do cliente.
async fn setup_auto(disposables: &Rc<DisposableStore>) -> Fixture {
    register_settings_sync_configuration();
    setup(disposables).await
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
async fn sync(test_object: &SettingsSynchroniser, manifest: Option<String>) {
    sync_synchroniser(&*test_object.base(), manifest, false).await.unwrap();
}

/// `testObject.sync(await client.getLatestRef(SyncResource.Settings))`.
async fn sync_latest(fixture: &Fixture) {
    sync(&fixture.test_object, fixture.client.get_latest_ref(SyncResource::Settings).await.unwrap()).await;
}

fn settings_resource(client: &UserDataSyncClient) -> je_base::uri::URI {
    client.user_data_profiles_service.default_profile().settings_resource
}

/// `parseSettingsSyncContent(syncData!.content)?.settings`.
fn settings_of(sync_data: &Option<ISyncData>) -> String {
    parse_settings_sync_content(&sync_data.as_ref().expect("há dados sincronizados").content).unwrap().settings
}

/// `updateSettings(content, client, profile?)`.
async fn update_settings(content: &str, client: &UserDataSyncClient, profile: Option<&UserDataProfile>) {
    let resource = match profile {
        Some(profile) => profile.settings_resource.clone(),
        None => settings_resource(client),
    };
    write_text(&client.file_service, &resource, content).await.unwrap();
    client.configuration_service.reload_configuration().await.unwrap();
}

fn string_property(value: &Value, name: &str) -> String {
    let Value::Object(object) = value else { panic!("não é um objeto: {value:?}") };
    match object.get(name) {
        Some(Value::String(text)) => text.to_string_lossy(),
        other => panic!("{name} não é um texto: {other:?}"),
    }
}

/// `parseSettings(content)`: o `settings` do `syncData.content` do conteúdo lido do servidor.
fn parse_settings(content: &str) -> String {
    let sync_data = parse_json(content).unwrap();
    let settings_sync_content = parse_json(&string_property(&sync_data, "content")).unwrap();
    string_property(&settings_sync_content, "settings")
}

/// `client.read(testObject.resource)` e `parseSettings` do `content`.
async fn read_remote_settings(fixture: &Fixture) -> String {
    let user_data = fixture.client.read(fixture.test_object.resource, None).await.unwrap();
    parse_settings(&user_data.content.expect("o conteúdo existe"))
}

/// Um literal de várias linhas.
fn lines(lines: &[&str]) -> String {
    lines.join("\n")
}

/// O conteúdo de `when settings file is empty and remote has changes`, `sync for first time to the server` e (com o
/// fim de `workbench.colorCustomizations` dado) `sync throws invalid content error`.
fn workbench_settings(color_customizations_end: &str) -> String {
    lines(&[
        "{",
        "\t// Always",
        "\t\"files.autoSave\": \"afterDelay\",",
        "\t\"files.simpleDialog.enable\": true,",
        "",
        "\t// Workbench",
        "\t\"workbench.colorTheme\": \"GitHub Sharp\",",
        "\t\"workbench.tree.indent\": 20,",
        "\t\"workbench.colorCustomizations\": {",
        "\t\t\"editorLineNumber.activeForeground\": \"#ff0000\",",
        "\t\t\"[GitHub Sharp]\": {",
        "\t\t\t\"statusBarItem.remoteBackground\": \"#24292E\",",
        "\t\t\t\"editorPane.background\": \"#f3f1f11a\"",
        "\t\t}",
        color_customizations_end,
        "",
        "\t\"gitBranch.base\": \"remote-repo/master\",",
        "",
        "\t// Experimental",
        "\t\"workbench.view.experimental.allowMovingToNewContainer\": true,",
        "}",
    ])
}

/// O que sobra de `machine_settings_content` e de `machine_settings_content_spread` sem as settings de máquina.
fn settings_without_machine_settings() -> String {
    lines(&["{", "\t// Always", "\t\"files.autoSave\": \"afterDelay\",", "\t\"files.simpleDialog.enable\": true,", "", "\t// Workbench", "\t\"workbench.colorTheme\": \"GitHub Sharp\"", "}"])
}

/// `do not sync ignored settings` (as duas suítes): o conteúdo com `settingsSync.ignoredSettings`.
fn ignored_settings_content() -> String {
    lines(&[
        "{",
        "\t// Always",
        "\t\"files.autoSave\": \"afterDelay\",",
        "\t\"files.simpleDialog.enable\": true,",
        "",
        "\t// Editor",
        "\t\"editor.fontFamily\": \"Fira Code\",",
        "",
        "\t// Terminal",
        "\t\"terminal.integrated.shell.osx\": \"some path\",",
        "",
        "\t// Workbench",
        "\t\"workbench.colorTheme\": \"GitHub Sharp\",",
        "",
        "\t// Ignored",
        "\t\"settingsSync.ignoredSettings\": [",
        "\t\t\"editor.fontFamily\",",
        "\t\t\"terminal.integrated.shell.osx\"",
        "\t]",
        "}",
    ])
}

/// O que sobra de `ignored_settings_content` sem as settings ignoradas.
fn ignored_settings_expected() -> String {
    lines(&[
        "{",
        "\t// Always",
        "\t\"files.autoSave\": \"afterDelay\",",
        "\t\"files.simpleDialog.enable\": true,",
        "",
        "\t// Workbench",
        "\t\"workbench.colorTheme\": \"GitHub Sharp\",",
        "",
        "\t// Ignored",
        "\t\"settingsSync.ignoredSettings\": [",
        "\t\t\"editor.fontFamily\",",
        "\t\t\"terminal.integrated.shell.osx\"",
        "\t]",
        "}",
    ])
}

// region: SettingsSync - Auto

#[test]
fn auto_when_settings_file_does_not_exist() {
    async_test(|d| async move {
        let fixture = setup_auto(&d).await;
        let (server, client, test_object) = (&fixture.server, &fixture.client, &fixture.test_object);
        let settings_resource = settings_resource(client);

        assert!(test_object.get_last_sync_user_data().await.unwrap().is_none());
        let mut manifest = client.get_latest_ref(SyncResource::Settings).await.unwrap();
        server.reset();
        sync(test_object, manifest).await;

        assert_eq!(server.requests(), Vec::<RecordedRequest>::new());
        assert!(!client.file_service.exists(&settings_resource).await.unwrap());

        let last_sync_user_data = test_object.get_last_sync_user_data().await.unwrap().expect("há dados da última sincronização");
        let remote_user_data = test_object.get_remote_user_data(None).await.unwrap();
        assert_eq!(last_sync_user_data.r#ref, remote_user_data.r#ref);
        assert_eq!(last_sync_user_data.sync_data, remote_user_data.sync_data);
        assert_eq!(last_sync_user_data.sync_data, None);

        manifest = client.get_latest_ref(SyncResource::Settings).await.unwrap();
        server.reset();
        sync(test_object, manifest).await;
        assert_eq!(server.requests(), Vec::<RecordedRequest>::new());

        manifest = client.get_latest_ref(SyncResource::Settings).await.unwrap();
        server.reset();
        sync(test_object, manifest).await;
        assert_eq!(server.requests(), Vec::<RecordedRequest>::new());
        teardown(&fixture).await;
    });
}

#[test]
fn auto_when_settings_file_is_empty_and_remote_has_no_changes() {
    async_test(|d| async move {
        let fixture = setup_auto(&d).await;
        let (client, test_object) = (&fixture.client, &fixture.test_object);
        let settings_resource = settings_resource(client);
        write_text(&client.file_service, &settings_resource, "").await.unwrap();

        sync_latest(&fixture).await;

        let last_sync_user_data = test_object.get_last_sync_user_data().await.unwrap().expect("há dados da última sincronização");
        let remote_user_data = test_object.get_remote_user_data(None).await.unwrap();
        assert_eq!(settings_of(&last_sync_user_data.sync_data), "{}");
        assert_eq!(settings_of(&remote_user_data.sync_data), "{}");
        assert_eq!(read_text(&client.file_service, &settings_resource).await, "");
        teardown(&fixture).await;
    });
}

#[test]
fn auto_when_settings_file_is_empty_and_remote_has_changes() {
    async_test(|d| async move {
        let fixture = setup_auto(&d).await;
        let (server, client, test_object) = (&fixture.server, &fixture.client, &fixture.test_object);
        let client2 = set_up_client2(&d, server).await;
        let content = workbench_settings("\t},");
        write_text(&client2.file_service, &settings_resource(&client2), &content).await.unwrap();
        client2.sync().await.unwrap();

        let settings_resource = settings_resource(client);
        write_text(&client.file_service, &settings_resource, "").await.unwrap();

        sync_latest(&fixture).await;

        let last_sync_user_data = test_object.get_last_sync_user_data().await.unwrap().expect("há dados da última sincronização");
        let remote_user_data = test_object.get_remote_user_data(None).await.unwrap();
        assert_eq!(settings_of(&last_sync_user_data.sync_data), content);
        assert_eq!(settings_of(&remote_user_data.sync_data), content);
        assert_eq!(read_text(&client.file_service, &settings_resource).await, content);
        teardown(&fixture).await;
    });
}

#[test]
fn auto_when_settings_file_is_created_after_first_sync() {
    async_test(|d| async move {
        let fixture = setup_auto(&d).await;
        let (server, client, test_object) = (&fixture.server, &fixture.client, &fixture.test_object);

        let settings_resource = settings_resource(client);
        sync_latest(&fixture).await;
        create_text(&client.file_service, &settings_resource, "{}").await.unwrap();

        let mut last_sync_user_data = test_object.get_last_sync_user_data().await.unwrap();
        let manifest = client.get_latest_ref(SyncResource::Settings).await.unwrap();
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
        assert_eq!(settings_of(&last_sync_user_data.sync_data), "{}");
        teardown(&fixture).await;
    });
}

#[test]
fn auto_sync_for_first_time_to_the_server() {
    async_test(|d| async move {
        let fixture = setup_auto(&d).await;
        let expected = workbench_settings("\t},");

        update_settings(&expected, &fixture.client, None).await;
        sync_latest(&fixture).await;

        assert_eq!(read_remote_settings(&fixture).await, expected);
        teardown(&fixture).await;
    });
}

#[test]
fn auto_do_not_sync_machine_settings() {
    async_test(|d| async move {
        let fixture = setup_auto(&d).await;
        let settings_content = lines(&[
            "{",
            "\t// Always",
            "\t\"files.autoSave\": \"afterDelay\",",
            "\t\"files.simpleDialog.enable\": true,",
            "",
            "\t// Workbench",
            "\t\"workbench.colorTheme\": \"GitHub Sharp\",",
            "",
            "\t// Machine",
            "\t\"settingsSync.machine\": \"someValue\",",
            "\t\"settingsSync.machineOverridable\": \"someValue\"",
            "}",
        ]);
        update_settings(&settings_content, &fixture.client, None).await;

        sync_latest(&fixture).await;

        assert_eq!(read_remote_settings(&fixture).await, settings_without_machine_settings());
        teardown(&fixture).await;
    });
}

#[test]
fn auto_do_not_sync_machine_settings_when_spread_across_file() {
    async_test(|d| async move {
        let fixture = setup_auto(&d).await;
        let settings_content = lines(&[
            "{",
            "\t// Always",
            "\t\"files.autoSave\": \"afterDelay\",",
            "\t\"settingsSync.machine\": \"someValue\",",
            "\t\"files.simpleDialog.enable\": true,",
            "",
            "\t// Workbench",
            "\t\"workbench.colorTheme\": \"GitHub Sharp\",",
            "",
            "\t// Machine",
            "\t\"settingsSync.machineOverridable\": \"someValue\"",
            "}",
        ]);
        update_settings(&settings_content, &fixture.client, None).await;

        sync_latest(&fixture).await;

        assert_eq!(read_remote_settings(&fixture).await, settings_without_machine_settings());
        teardown(&fixture).await;
    });
}

#[test]
fn auto_do_not_sync_machine_settings_when_spread_across_file_2() {
    async_test(|d| async move {
        let fixture = setup_auto(&d).await;
        let settings_content = lines(&[
            "{",
            "\t// Always",
            "\t\"files.autoSave\": \"afterDelay\",",
            "\t\"settingsSync.machine\": \"someValue\",",
            "",
            "\t// Workbench",
            "\t\"workbench.colorTheme\": \"GitHub Sharp\",",
            "",
            "\t// Machine",
            "\t\"settingsSync.machineOverridable\": \"someValue\",",
            "\t\"files.simpleDialog.enable\": true,",
            "}",
        ]);
        update_settings(&settings_content, &fixture.client, None).await;

        sync_latest(&fixture).await;

        let expected = lines(&[
            "{",
            "\t// Always",
            "\t\"files.autoSave\": \"afterDelay\",",
            "",
            "\t// Workbench",
            "\t\"workbench.colorTheme\": \"GitHub Sharp\",",
            "\t\"files.simpleDialog.enable\": true,",
            "}",
        ]);
        assert_eq!(read_remote_settings(&fixture).await, expected);
        teardown(&fixture).await;
    });
}

#[test]
fn auto_sync_when_all_settings_are_machine_settings() {
    async_test(|d| async move {
        let fixture = setup_auto(&d).await;
        let settings_content = lines(&["{", "\t// Machine", "\t\"settingsSync.machine\": \"someValue\",", "\t\"settingsSync.machineOverridable\": \"someValue\"", "}"]);
        update_settings(&settings_content, &fixture.client, None).await;

        sync_latest(&fixture).await;

        assert_eq!(read_remote_settings(&fixture).await, lines(&["{", "}"]));
        teardown(&fixture).await;
    });
}

#[test]
fn auto_sync_when_all_settings_are_machine_settings_with_trailing_comma() {
    async_test(|d| async move {
        let fixture = setup_auto(&d).await;
        let settings_content = lines(&["{", "\t// Machine", "\t\"settingsSync.machine\": \"someValue\",", "\t\"settingsSync.machineOverridable\": \"someValue\",", "}"]);
        update_settings(&settings_content, &fixture.client, None).await;

        sync_latest(&fixture).await;

        assert_eq!(read_remote_settings(&fixture).await, lines(&["{", "}"]));
        teardown(&fixture).await;
    });
}

#[test]
fn auto_local_change_event_is_triggered_when_settings_are_changed() {
    async_test(|d| async move {
        let fixture = setup_auto(&d).await;
        let content = lines(&["{", "\t\"files.autoSave\": \"afterDelay\",", "\t\"files.simpleDialog.enable\": true,", "}"]);

        update_settings(&content, &fixture.client, None).await;
        sync_latest(&fixture).await;

        let promise = to_promise(&fixture.test_object.base().on_did_change_local());
        update_settings(&lines(&["{", "\t\"files.autoSave\": \"off\",", "\t\"files.simpleDialog.enable\": true,", "}"]), &fixture.client, None).await;
        promise.await.unwrap();
        teardown(&fixture).await;
    });
}

#[test]
fn auto_do_not_sync_ignored_settings() {
    async_test(|d| async move {
        let fixture = setup_auto(&d).await;
        update_settings(&ignored_settings_content(), &fixture.client, None).await;

        sync_latest(&fixture).await;

        assert_eq!(read_remote_settings(&fixture).await, ignored_settings_expected());
        teardown(&fixture).await;
    });
}

#[test]
fn auto_do_not_sync_ignored_and_machine_settings() {
    async_test(|d| async move {
        let fixture = setup_auto(&d).await;
        let settings_content = lines(&[
            "{",
            "\t// Always",
            "\t\"files.autoSave\": \"afterDelay\",",
            "\t\"files.simpleDialog.enable\": true,",
            "",
            "\t// Editor",
            "\t\"editor.fontFamily\": \"Fira Code\",",
            "",
            "\t// Terminal",
            "\t\"terminal.integrated.shell.osx\": \"some path\",",
            "",
            "\t// Workbench",
            "\t\"workbench.colorTheme\": \"GitHub Sharp\",",
            "",
            "\t// Ignored",
            "\t\"settingsSync.ignoredSettings\": [",
            "\t\t\"editor.fontFamily\",",
            "\t\t\"terminal.integrated.shell.osx\"",
            "\t],",
            "",
            "\t// Machine",
            "\t\"settingsSync.machine\": \"someValue\",",
            "}",
        ]);
        update_settings(&settings_content, &fixture.client, None).await;

        sync_latest(&fixture).await;

        let expected = lines(&[
            "{",
            "\t// Always",
            "\t\"files.autoSave\": \"afterDelay\",",
            "\t\"files.simpleDialog.enable\": true,",
            "",
            "\t// Workbench",
            "\t\"workbench.colorTheme\": \"GitHub Sharp\",",
            "",
            "\t// Ignored",
            "\t\"settingsSync.ignoredSettings\": [",
            "\t\t\"editor.fontFamily\",",
            "\t\t\"terminal.integrated.shell.osx\"",
            "\t],",
            "}",
        ]);
        assert_eq!(read_remote_settings(&fixture).await, expected);
        teardown(&fixture).await;
    });
}

/// `assert.ok(e instanceof UserDataSyncError)` e `e.code === LocalInvalidContent`.
async fn assert_sync_fails_with_invalid_content(fixture: &Fixture) {
    let result = sync_synchroniser(&*fixture.test_object.base(), fixture.client.get_latest_ref(SyncResource::Settings).await.unwrap(), false).await;
    let error = result.expect_err("should fail with invalid content error");
    let sync_error = UserDataSyncError::from_error(&error).expect("é um UserDataSyncError");
    assert_eq!(sync_error.code, UserDataSyncErrorCode::LocalInvalidContent);
}

#[test]
fn auto_sync_throws_invalid_content_error() {
    async_test(|d| async move {
        let fixture = setup_auto(&d).await;
        update_settings(&workbench_settings("\t}"), &fixture.client, None).await;

        assert_sync_fails_with_invalid_content(&fixture).await;
        teardown(&fixture).await;
    });
}

#[test]
fn auto_sync_throws_invalid_content_error_content_is_an_array() {
    async_test(|d| async move {
        let fixture = setup_auto(&d).await;
        update_settings("[]", &fixture.client, None).await;

        assert_sync_fails_with_invalid_content(&fixture).await;
        teardown(&fixture).await;
    });
}

#[test]
fn auto_sync_when_there_are_conflicts() {
    async_test(|d| async move {
        let fixture = setup_auto(&d).await;
        let (server, client, test_object) = (&fixture.server, &fixture.client, &fixture.test_object);
        let client2 = set_up_client2(&d, server).await;
        update_settings(r#"{"a":1,"b":2,"settingsSync.ignoredSettings":["a"]}"#, &client2, None).await;
        client2.sync().await.unwrap();

        update_settings(r#"{"a":2,"b":1,"settingsSync.ignoredSettings":["a"]}"#, client, None).await;
        sync_latest(&fixture).await;

        assert_eq!(test_object.base().status(), SyncStatus::HasConflicts);
        let conflicts = test_object.base().conflicts().conflicts;
        assert_eq!(conflicts[0].local_resource.to_string(false), test_object.resources.local.to_string(false));

        let merge_content = read_text(&client.file_service, &conflicts[0].preview_resource).await;
        assert_eq!(merge_content, "");
        teardown(&fixture).await;
    });
}

#[test]
fn auto_sync_profile_settings() {
    async_test(|d| async move {
        let fixture = setup_auto(&d).await;
        let (server, client, test_object) = (&fixture.server, &fixture.client, &fixture.test_object);
        let client2 = set_up_client2(&d, server).await;
        let profile = client2.user_data_profiles_service.create_named_profile("profile1", None, None).await.unwrap();
        update_settings(r#"{"a":1,"b":2}"#, &client2, Some(&profile)).await;
        client2.sync().await.unwrap();

        client.sync().await.unwrap();

        assert_eq!(test_object.base().status(), SyncStatus::Idle);

        let synced_profile = client.user_data_profiles_service.profiles().into_iter().find(|p| p.id == profile.id).expect("o perfil foi sincronizado");
        let content = read_text(&client.file_service, &synced_profile.settings_resource).await;
        assert_eq!(parse_json(&content).unwrap(), parse_json(r#"{"a":1,"b":2}"#).unwrap());
        teardown(&fixture).await;
    });
}

// endregion

// region: SettingsSync - Manual

#[test]
fn manual_do_not_sync_ignored_settings() {
    async_test(|d| async move {
        let fixture = setup(&d).await;
        let test_object = &fixture.test_object;
        update_settings(&ignored_settings_content(), &fixture.client, None).await;

        let preview = sync_synchroniser(&*test_object.base(), fixture.client.get_latest_ref(SyncResource::Settings).await.unwrap(), true).await.unwrap();
        assert_eq!(test_object.base().status(), SyncStatus::Syncing);
        let preview = test_object.base().accept(&preview.expect("há o preview").resource_previews[0].preview_resource, None).await.unwrap();
        assert!(preview.is_some());
        test_object.base().apply(false, IHeaders::new()).await.unwrap();

        assert_eq!(read_remote_settings(&fixture).await, ignored_settings_expected());
        teardown(&fixture).await;
    });
}

// endregion
