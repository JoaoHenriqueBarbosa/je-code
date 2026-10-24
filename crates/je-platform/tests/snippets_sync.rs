//! Porte de `src/vs/platform/userDataSync/test/common/snippetsSync.test.ts` (suíte `SnippetsSync`), caso a caso e na mesma ordem.
//!
//! Divergências: o `setup` (o cliente, o sincronizador de snippets e o `client2`) roda no começo do corpo de cada
//! teste, dentro do `runWithFakedTimers` de [`async_test`], e o `teardown` (`clear` do servidor) no fim; o servidor é
//! um por teste (o `clear` do `teardown` o devolve ao estado inicial no original). `testObject.sync(manifest)` usa os
//! argumentos padrão do original (`preview` falso, configuração e cabeçalhos vazios). Os literais de snippet são
//! montados linha a linha com `\t` e `\n` explícitos, para a indentação ficar visível. O `JSON.parse` do conteúdo do
//! servidor e o `deepStrictEqual` de dicionários viram a comparação de mapas ordenados (a ordem das chaves não conta).

mod common;

use std::collections::BTreeMap;
use std::rc::Rc;

use common::files::async_test;
use common::user_data_sync_client::{RecordedRequest, UserDataSyncClient, UserDataSyncTestServer};
use common::{if_match_headers, read_text, sync_synchroniser, write_text};
use je_base::JsString;
use je_base::json::{Object, Value};
use je_base::lifecycle::DisposableStore;
use je_base::parts::request::IHeaders;
use je_base::resources::ExtUri;
use je_base::uri::URI;
use je_platform::resources::host_environment;
use je_platform::user_data_profile::{IUserDataProfilesService, UserDataProfile};
use je_platform::user_data_sync::abstract_synchronizer::{parse_json, stringify_json};
use je_platform::user_data_sync::snippets_sync::SnippetsSynchroniser;
use je_platform::user_data_sync::user_data_sync::{
    IResourcePreview, ISyncData, IUserDataSyncResourcePreview, IUserDataSyncStoreService, IUserDataSynchroniser, PREVIEW_DIR_NAME, SyncResource, SyncStatus,
};

/// As linhas de um literal de template do original, unidas por `\n`.
macro_rules! text_lines {
    ($first:literal $(, $rest:literal)* $(,)?) => {
        concat!($first $(, "\n", $rest)*)
    };
}

const TS_SNIPPET_1: &str = text_lines!(
    "{",
    "",
    "\t// Place your snippets for TypeScript here. Each snippet is defined under a snippet name and has a prefix, body and",
    "\t// description. The prefix is what is used to trigger the snippet and the body will be expanded and inserted. Possible variables are:",
    "\t// $1, $2 for tab stops, $0 for the final cursor position, Placeholders with the",
    "\t// same ids are connected.",
    "\t\"Print to console\": {",
    "\t// Example:",
    "\t\"prefix\": \"log\",",
    "\t\t\"body\": [",
    "\t\t\t\"console.log('$1');\",",
    "\t\t\t\"$2\"",
    "\t\t],",
    "\t\t\t\"description\": \"Log output to console\",",
    "\t}",
    "",
    "}"
);

const TS_SNIPPET_2: &str = text_lines!(
    "{",
    "",
    "\t// Place your snippets for TypeScript here. Each snippet is defined under a snippet name and has a prefix, body and",
    "\t// description. The prefix is what is used to trigger the snippet and the body will be expanded and inserted. Possible variables are:",
    "\t// $1, $2 for tab stops, $0 for the final cursor position, Placeholders with the",
    "\t// same ids are connected.",
    "\t\"Print to console\": {",
    "\t// Example:",
    "\t\"prefix\": \"log\",",
    "\t\t\"body\": [",
    "\t\t\t\"console.log('$1');\",",
    "\t\t\t\"$2\"",
    "\t\t],",
    "\t\t\t\"description\": \"Log output to console always\",",
    "\t}",
    "",
    "}"
);

const HTML_SNIPPET_1: &str = text_lines!(
    "{",
    "/*",
    "\t// Place your snippets for HTML here. Each snippet is defined under a snippet name and has a prefix, body and",
    "\t// description. The prefix is what is used to trigger the snippet and the body will be expanded and inserted.",
    "\t// Example:",
    "\t\"Print to console\": {",
    "\t\"prefix\": \"log\",",
    "\t\t\"body\": [",
    "\t\t\t\"console.log('$1');\",",
    "\t\t\t\"$2\"",
    "\t\t],",
    "\t\t\t\"description\": \"Log output to console\"",
    "\t}",
    "*/",
    "\"Div\": {",
    "\t\"prefix\": \"div\",",
    "\t\t\"body\": [",
    "\t\t\t\"<div>\",",
    "\t\t\t\"\",",
    "\t\t\t\"</div>\"",
    "\t\t],",
    "\t\t\t\"description\": \"New div\"",
    "\t}",
    "}"
);

const HTML_SNIPPET_2: &str = text_lines!(
    "{",
    "/*",
    "\t// Place your snippets for HTML here. Each snippet is defined under a snippet name and has a prefix, body and",
    "\t// description. The prefix is what is used to trigger the snippet and the body will be expanded and inserted.",
    "\t// Example:",
    "\t\"Print to console\": {",
    "\t\"prefix\": \"log\",",
    "\t\t\"body\": [",
    "\t\t\t\"console.log('$1');\",",
    "\t\t\t\"$2\"",
    "\t\t],",
    "\t\t\t\"description\": \"Log output to console\"",
    "\t}",
    "*/",
    "\"Div\": {",
    "\t\"prefix\": \"div\",",
    "\t\t\"body\": [",
    "\t\t\t\"<div>\",",
    "\t\t\t\"\",",
    "\t\t\t\"</div>\"",
    "\t\t],",
    "\t\t\t\"description\": \"New div changed\"",
    "\t}",
    "}"
);

const HTML_SNIPPET_3: &str = text_lines!(
    "{",
    "/*",
    "\t// Place your snippets for HTML here. Each snippet is defined under a snippet name and has a prefix, body and",
    "\t// description. The prefix is what is used to trigger the snippet and the body will be expanded and inserted.",
    "\t// Example:",
    "\t\"Print to console\": {",
    "\t\"prefix\": \"log\",",
    "\t\t\"body\": [",
    "\t\t\t\"console.log('$1');\",",
    "\t\t\t\"$2\"",
    "\t\t],",
    "\t\t\t\"description\": \"Log output to console\"",
    "\t}",
    "*/",
    "\"Div\": {",
    "\t\"prefix\": \"div\",",
    "\t\t\"body\": [",
    "\t\t\t\"<div>\",",
    "\t\t\t\"\",",
    "\t\t\t\"</div>\"",
    "\t\t],",
    "\t\t\t\"description\": \"New div changed again\"",
    "\t}",
    "}"
);

const GLOBAL_SNIPPET: &str = text_lines!(
    "{",
    "\t// Place your global snippets here. Each snippet is defined under a snippet name and has a scope, prefix, body and",
    "\t// description. Add comma separated ids of the languages where the snippet is applicable in the scope field. If scope",
    "\t// is left empty or omitted, the snippet gets applied to all languages. The prefix is what is",
    "\t// used to trigger the snippet and the body will be expanded and inserted. Possible variables are:",
    "\t// $1, $2 for tab stops, $0 for the final cursor position, and {1: label}, { 2: another } for placeholders.",
    "\t// Placeholders with the same ids are connected.",
    "\t// Example:",
    "\t// \"Print to console\": {",
    "\t// \t\"scope\": \"javascript,typescript\",",
    "\t// \t\"prefix\": \"log\",",
    "\t// \t\"body\": [",
    "\t// \t\t\"console.log('$1');\",",
    "\t// \t\t\"$2\"",
    "\t// \t],",
    "\t// \t\"description\": \"Log output to console\"",
    "\t// }",
    "}"
);

struct Fixture {
    server: Rc<UserDataSyncTestServer>,
    test_client: Rc<UserDataSyncClient>,
    client2: Rc<UserDataSyncClient>,
    test_object: Rc<SnippetsSynchroniser>,
}

/// O `setup` da suíte.
async fn setup(disposables: &Rc<DisposableStore>) -> Fixture {
    let server = Rc::new(UserDataSyncTestServer::new());
    let test_client = disposables.add_rc(UserDataSyncClient::set_up(server.clone(), true).await.unwrap());
    let test_object = test_client.get_synchronizer(SyncResource::Snippets).into_snippets().expect("o sincronizador de snippets está ativo");
    let client2 = disposables.add_rc(UserDataSyncClient::set_up(server.clone(), true).await.unwrap());
    Fixture { server, test_client, client2, test_object }
}

/// O `teardown` da suíte.
async fn teardown(fixture: &Fixture) {
    fixture.test_client.user_data_sync_store_service.clear().await.unwrap();
}

/// `testObject.sync(manifest)`.
async fn sync(fixture: &Fixture, manifest: Option<String>) {
    sync_synchroniser(&*fixture.test_object.base, manifest, false).await.unwrap();
}

/// `testObject.sync(await testClient.getLatestRef(SyncResource.Snippets))`.
async fn sync_latest(fixture: &Fixture) {
    sync(fixture, latest_ref(fixture).await).await;
}

/// `testObject.sync(await testClient.getLatestRef(SyncResource.Snippets), true)`.
async fn sync_latest_preview(fixture: &Fixture) -> Option<IUserDataSyncResourcePreview> {
    sync_synchroniser(&*fixture.test_object.base, latest_ref(fixture).await, true).await.unwrap()
}

/// `testClient.getLatestRef(SyncResource.Snippets)`.
async fn latest_ref(fixture: &Fixture) -> Option<String> {
    fixture.test_client.get_latest_ref(SyncResource::Snippets).await.unwrap()
}

fn ext_uri() -> ExtUri {
    ExtUri::unbiased(host_environment())
}

/// `assert.deepStrictEqual(testObject.conflicts.conflicts, [])`.
fn assert_no_conflicts(fixture: &Fixture) {
    assert_eq!(fixture.test_object.conflicts().conflicts, Vec::<IResourcePreview>::new());
}

/// `joinPath(environmentService.userDataSyncHome, testObject.resource, PREVIEW_DIR_NAME, name)`.
fn preview_resource(fixture: &Fixture, name: &str) -> URI {
    ext_uri().join_path(&fixture.test_client.environment_service.user_data_sync_home, &[fixture.test_object.resource.as_str(), PREVIEW_DIR_NAME, name])
}

/// `parseSnippets(content)`: o `JSON.parse` do conteúdo do servidor e o do `content` do `ISyncData`.
fn parse_snippets(content: &str) -> BTreeMap<String, String> {
    let sync_data = ISyncData::from_json(&parse_json(content).unwrap()).expect("o conteúdo do servidor é um ISyncData");
    let Value::Object(object) = parse_json(&sync_data.content).unwrap() else { panic!("os snippets sincronizados são um objeto") };
    object
        .iter()
        .map(|(key, value)| match value {
            Value::String(text) => (key.to_string_lossy(), text.to_string_lossy()),
            other => panic!("o snippet {} não é texto: {other:?}", key.to_string_lossy()),
        })
        .collect()
}

/// O dicionário esperado de um `assert.deepStrictEqual(actual, { ... })`.
fn dictionary(entries: &[(&str, &str)]) -> BTreeMap<String, String> {
    entries.iter().map(|(key, value)| (key.to_string(), value.to_string())).collect()
}

/// `JSON.stringify({ ... })` de um dicionário de snippets, na ordem dada.
fn stringify_snippets(entries: &[(&str, &str)]) -> String {
    let mut object = Object::new();
    for (key, value) in entries {
        object.insert(JsString::from(*key), Value::String(JsString::from(*value)));
    }
    stringify_json(&Value::Object(object))
}

/// `const { content } = await testClient.read(testObject.resource); parseSnippets(content)`.
async fn read_remote_snippets(fixture: &Fixture) -> BTreeMap<String, String> {
    let content = fixture.test_client.read(fixture.test_object.resource, None).await.unwrap().content.expect("o servidor tem o conteúdo");
    parse_snippets(&content)
}

/// `joinPath((profile ?? userDataProfilesService.defaultProfile).snippetsHome, name)`.
fn snippet_resource(client: &UserDataSyncClient, name: &str, profile: Option<&UserDataProfile>) -> URI {
    let snippets_home = match profile {
        Some(profile) => profile.snippets_home.clone(),
        None => client.user_data_profiles_service.default_profile().snippets_home,
    };
    ext_uri().join_path(&snippets_home, &[name])
}

async fn update_snippet(name: &str, content: &str, client: &UserDataSyncClient, profile: Option<&UserDataProfile>) {
    write_text(&client.file_service, &snippet_resource(client, name, profile), content).await.unwrap();
}

async fn remove_snippet(name: &str, client: &UserDataSyncClient) {
    client.file_service.del(&snippet_resource(client, name, None), None).await.unwrap();
}

async fn read_snippet(name: &str, client: &UserDataSyncClient, profile: Option<&UserDataProfile>) -> Option<String> {
    let resource = snippet_resource(client, name, profile);
    if client.file_service.exists(&resource).await.unwrap() { Some(read_text(&client.file_service, &resource).await) } else { None }
}

/// `assertPreviews(actual, expected)`.
fn assert_previews(actual: &[IResourcePreview], expected: &[URI]) {
    let actual: Vec<String> = actual.iter().map(|preview| preview.preview_resource.to_string(false).to_string_lossy()).collect();
    let expected: Vec<String> = expected.iter().map(|uri| uri.to_string(false).to_string_lossy()).collect();
    assert_eq!(actual, expected);
}

#[test]
fn when_snippets_does_not_exist() {
    async_test(|d| async move {
        let fixture = setup(&d).await;
        let (server, test_client, test_object) = (&fixture.server, &fixture.test_client, &fixture.test_object);
        let snippets_resource = test_client.user_data_profiles_service.default_profile().snippets_home;

        assert!(test_object.get_last_sync_user_data().await.unwrap().is_none());
        let mut manifest = latest_ref(&fixture).await;
        server.reset();
        sync(&fixture, manifest).await;

        assert_eq!(server.requests(), Vec::<RecordedRequest>::new());
        assert!(!test_client.file_service.exists(&snippets_resource).await.unwrap());

        let last_sync_user_data = test_object.get_last_sync_user_data().await.unwrap().expect("há dados da última sincronização");
        let remote_user_data = test_object.get_remote_user_data(None).await.unwrap();
        assert_eq!(last_sync_user_data.r#ref, remote_user_data.r#ref);
        assert_eq!(last_sync_user_data.sync_data, remote_user_data.sync_data);
        assert_eq!(last_sync_user_data.sync_data, None);

        manifest = latest_ref(&fixture).await;
        server.reset();
        sync(&fixture, manifest).await;
        assert_eq!(server.requests(), Vec::<RecordedRequest>::new());

        manifest = latest_ref(&fixture).await;
        server.reset();
        sync(&fixture, manifest).await;
        assert_eq!(server.requests(), Vec::<RecordedRequest>::new());
        teardown(&fixture).await;
    });
}

#[test]
fn when_snippet_is_created_after_first_sync() {
    async_test(|d| async move {
        let fixture = setup(&d).await;
        let (server, test_client, test_object) = (&fixture.server, &fixture.test_client, &fixture.test_object);
        sync_latest(&fixture).await;
        update_snippet("html.json", HTML_SNIPPET_1, test_client, None).await;

        let mut last_sync_user_data = test_object.get_last_sync_user_data().await.unwrap();
        let manifest = latest_ref(&fixture).await;
        server.reset();
        sync(&fixture, manifest).await;

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
        assert_eq!(last_sync_user_data.sync_data.as_ref().expect("há dados sincronizados").content, stringify_snippets(&[("html.json", HTML_SNIPPET_1)]));
        teardown(&fixture).await;
    });
}

#[test]
fn first_time_sync_outgoing_to_server_no_snippets() {
    async_test(|d| async move {
        let fixture = setup(&d).await;
        let (test_client, test_object) = (&fixture.test_client, &fixture.test_object);
        update_snippet("html.json", HTML_SNIPPET_1, test_client, None).await;
        update_snippet("typescript.json", TS_SNIPPET_1, test_client, None).await;

        sync_latest(&fixture).await;
        assert_eq!(test_object.status(), SyncStatus::Idle);
        assert_no_conflicts(&fixture);

        let actual = read_remote_snippets(&fixture).await;
        assert_eq!(actual, dictionary(&[("html.json", HTML_SNIPPET_1), ("typescript.json", TS_SNIPPET_1)]));
        teardown(&fixture).await;
    });
}

#[test]
fn first_time_sync_incoming_from_server_no_snippets() {
    async_test(|d| async move {
        let fixture = setup(&d).await;
        let (test_client, client2, test_object) = (&fixture.test_client, &fixture.client2, &fixture.test_object);
        update_snippet("html.json", HTML_SNIPPET_1, client2, None).await;
        update_snippet("typescript.json", TS_SNIPPET_1, client2, None).await;
        client2.sync().await.unwrap();

        sync_latest(&fixture).await;
        assert_eq!(test_object.status(), SyncStatus::Idle);
        assert_no_conflicts(&fixture);

        let actual1 = read_snippet("html.json", test_client, None).await;
        assert_eq!(actual1.as_deref(), Some(HTML_SNIPPET_1));
        let actual2 = read_snippet("typescript.json", test_client, None).await;
        assert_eq!(actual2.as_deref(), Some(TS_SNIPPET_1));
        teardown(&fixture).await;
    });
}

#[test]
fn first_time_sync_when_snippets_exists() {
    async_test(|d| async move {
        let fixture = setup(&d).await;
        let (test_client, client2, test_object) = (&fixture.test_client, &fixture.client2, &fixture.test_object);
        update_snippet("html.json", HTML_SNIPPET_1, client2, None).await;
        client2.sync().await.unwrap();

        update_snippet("typescript.json", TS_SNIPPET_1, test_client, None).await;
        sync_latest(&fixture).await;
        assert_eq!(test_object.status(), SyncStatus::Idle);
        assert_no_conflicts(&fixture);

        let actual1 = read_snippet("html.json", test_client, None).await;
        assert_eq!(actual1.as_deref(), Some(HTML_SNIPPET_1));
        let actual2 = read_snippet("typescript.json", test_client, None).await;
        assert_eq!(actual2.as_deref(), Some(TS_SNIPPET_1));

        let actual = read_remote_snippets(&fixture).await;
        assert_eq!(actual, dictionary(&[("html.json", HTML_SNIPPET_1), ("typescript.json", TS_SNIPPET_1)]));
        teardown(&fixture).await;
    });
}

#[test]
fn first_time_sync_when_snippets_exists_has_conflicts() {
    async_test(|d| async move {
        let fixture = setup(&d).await;
        let (test_client, client2, test_object) = (&fixture.test_client, &fixture.client2, &fixture.test_object);
        update_snippet("html.json", HTML_SNIPPET_1, client2, None).await;
        client2.sync().await.unwrap();

        update_snippet("html.json", HTML_SNIPPET_2, test_client, None).await;
        sync_latest(&fixture).await;

        assert_eq!(test_object.status(), SyncStatus::HasConflicts);
        let local = preview_resource(&fixture, "html.json");
        assert_previews(&test_object.conflicts().conflicts, &[local]);
        teardown(&fixture).await;
    });
}

#[test]
fn first_time_sync_when_snippets_exists_has_conflicts_and_accept_conflicts() {
    async_test(|d| async move {
        let fixture = setup(&d).await;
        let (test_client, client2, test_object) = (&fixture.test_client, &fixture.client2, &fixture.test_object);
        update_snippet("html.json", HTML_SNIPPET_1, client2, None).await;
        client2.sync().await.unwrap();

        update_snippet("html.json", HTML_SNIPPET_2, test_client, None).await;
        sync_latest(&fixture).await;
        let conflicts = test_object.conflicts().conflicts;
        test_object.accept(&conflicts[0].preview_resource, Some(Some(HTML_SNIPPET_1.to_string()))).await.unwrap();
        test_object.apply(false, IHeaders::new()).await.unwrap();

        assert_eq!(test_object.status(), SyncStatus::Idle);
        assert_no_conflicts(&fixture);

        let actual1 = read_snippet("html.json", test_client, None).await;
        assert_eq!(actual1.as_deref(), Some(HTML_SNIPPET_1));

        let actual = read_remote_snippets(&fixture).await;
        assert_eq!(actual, dictionary(&[("html.json", HTML_SNIPPET_1)]));
        teardown(&fixture).await;
    });
}

#[test]
fn first_time_sync_when_snippets_exists_has_multiple_conflicts() {
    async_test(|d| async move {
        let fixture = setup(&d).await;
        let (test_client, client2, test_object) = (&fixture.test_client, &fixture.client2, &fixture.test_object);
        update_snippet("html.json", HTML_SNIPPET_1, client2, None).await;
        update_snippet("typescript.json", TS_SNIPPET_1, client2, None).await;
        client2.sync().await.unwrap();

        update_snippet("html.json", HTML_SNIPPET_2, test_client, None).await;
        update_snippet("typescript.json", TS_SNIPPET_2, test_client, None).await;
        sync_latest(&fixture).await;

        assert_eq!(test_object.status(), SyncStatus::HasConflicts);
        let local1 = preview_resource(&fixture, "html.json");
        let local2 = preview_resource(&fixture, "typescript.json");
        assert_previews(&test_object.conflicts().conflicts, &[local1, local2]);
        teardown(&fixture).await;
    });
}

#[test]
fn first_time_sync_when_snippets_exists_has_multiple_conflicts_and_accept_one_conflict() {
    async_test(|d| async move {
        let fixture = setup(&d).await;
        let (test_client, client2, test_object) = (&fixture.test_client, &fixture.client2, &fixture.test_object);
        update_snippet("html.json", HTML_SNIPPET_1, client2, None).await;
        update_snippet("typescript.json", TS_SNIPPET_1, client2, None).await;
        client2.sync().await.unwrap();

        update_snippet("html.json", HTML_SNIPPET_2, test_client, None).await;
        update_snippet("typescript.json", TS_SNIPPET_2, test_client, None).await;
        sync_latest(&fixture).await;

        let mut conflicts = test_object.conflicts().conflicts;
        test_object.accept(&conflicts[0].preview_resource, Some(Some(HTML_SNIPPET_2.to_string()))).await.unwrap();

        conflicts = test_object.conflicts().conflicts;
        assert_eq!(test_object.status(), SyncStatus::HasConflicts);
        let local = preview_resource(&fixture, "typescript.json");
        assert_previews(&test_object.conflicts().conflicts, &[local]);
        let _ = conflicts;
        teardown(&fixture).await;
    });
}

#[test]
fn first_time_sync_when_snippets_exists_has_multiple_conflicts_and_accept_all_conflicts() {
    async_test(|d| async move {
        let fixture = setup(&d).await;
        let (test_client, client2, test_object) = (&fixture.test_client, &fixture.client2, &fixture.test_object);
        update_snippet("html.json", HTML_SNIPPET_1, client2, None).await;
        update_snippet("typescript.json", TS_SNIPPET_1, client2, None).await;
        client2.sync().await.unwrap();

        update_snippet("html.json", HTML_SNIPPET_2, test_client, None).await;
        update_snippet("typescript.json", TS_SNIPPET_2, test_client, None).await;
        sync_latest(&fixture).await;

        let conflicts = test_object.conflicts().conflicts;
        test_object.accept(&conflicts[0].preview_resource, Some(Some(HTML_SNIPPET_2.to_string()))).await.unwrap();
        test_object.accept(&conflicts[1].preview_resource, Some(Some(TS_SNIPPET_1.to_string()))).await.unwrap();
        test_object.apply(false, IHeaders::new()).await.unwrap();

        assert_eq!(test_object.status(), SyncStatus::Idle);
        assert_no_conflicts(&fixture);

        let actual1 = read_snippet("html.json", test_client, None).await;
        assert_eq!(actual1.as_deref(), Some(HTML_SNIPPET_2));
        let actual2 = read_snippet("typescript.json", test_client, None).await;
        assert_eq!(actual2.as_deref(), Some(TS_SNIPPET_1));

        let actual = read_remote_snippets(&fixture).await;
        assert_eq!(actual, dictionary(&[("html.json", HTML_SNIPPET_2), ("typescript.json", TS_SNIPPET_1)]));
        teardown(&fixture).await;
    });
}

#[test]
fn sync_adding_a_snippet() {
    async_test(|d| async move {
        let fixture = setup(&d).await;
        let (test_client, test_object) = (&fixture.test_client, &fixture.test_object);
        update_snippet("html.json", HTML_SNIPPET_1, test_client, None).await;
        sync_latest(&fixture).await;

        update_snippet("typescript.json", TS_SNIPPET_1, test_client, None).await;
        sync_latest(&fixture).await;
        assert_eq!(test_object.status(), SyncStatus::Idle);
        assert_no_conflicts(&fixture);

        let actual1 = read_snippet("html.json", test_client, None).await;
        assert_eq!(actual1.as_deref(), Some(HTML_SNIPPET_1));
        let actual2 = read_snippet("typescript.json", test_client, None).await;
        assert_eq!(actual2.as_deref(), Some(TS_SNIPPET_1));

        let actual = read_remote_snippets(&fixture).await;
        assert_eq!(actual, dictionary(&[("html.json", HTML_SNIPPET_1), ("typescript.json", TS_SNIPPET_1)]));
        teardown(&fixture).await;
    });
}

#[test]
fn sync_adding_a_snippet_accept() {
    async_test(|d| async move {
        let fixture = setup(&d).await;
        let (test_client, client2, test_object) = (&fixture.test_client, &fixture.client2, &fixture.test_object);
        update_snippet("html.json", HTML_SNIPPET_1, client2, None).await;
        client2.sync().await.unwrap();
        sync_latest(&fixture).await;

        update_snippet("typescript.json", TS_SNIPPET_1, client2, None).await;
        client2.sync().await.unwrap();

        sync_latest(&fixture).await;
        assert_eq!(test_object.status(), SyncStatus::Idle);
        assert_no_conflicts(&fixture);

        let actual1 = read_snippet("html.json", test_client, None).await;
        assert_eq!(actual1.as_deref(), Some(HTML_SNIPPET_1));
        let actual2 = read_snippet("typescript.json", test_client, None).await;
        assert_eq!(actual2.as_deref(), Some(TS_SNIPPET_1));
        teardown(&fixture).await;
    });
}

#[test]
fn sync_updating_a_snippet() {
    async_test(|d| async move {
        let fixture = setup(&d).await;
        let (test_client, test_object) = (&fixture.test_client, &fixture.test_object);
        update_snippet("html.json", HTML_SNIPPET_1, test_client, None).await;
        sync_latest(&fixture).await;

        update_snippet("html.json", HTML_SNIPPET_2, test_client, None).await;
        sync_latest(&fixture).await;
        assert_eq!(test_object.status(), SyncStatus::Idle);
        assert_no_conflicts(&fixture);

        let actual1 = read_snippet("html.json", test_client, None).await;
        assert_eq!(actual1.as_deref(), Some(HTML_SNIPPET_2));

        let actual = read_remote_snippets(&fixture).await;
        assert_eq!(actual, dictionary(&[("html.json", HTML_SNIPPET_2)]));
        teardown(&fixture).await;
    });
}

#[test]
fn sync_updating_a_snippet_accept() {
    async_test(|d| async move {
        let fixture = setup(&d).await;
        let (test_client, client2, test_object) = (&fixture.test_client, &fixture.client2, &fixture.test_object);
        update_snippet("html.json", HTML_SNIPPET_1, client2, None).await;
        client2.sync().await.unwrap();
        sync_latest(&fixture).await;

        update_snippet("html.json", HTML_SNIPPET_2, client2, None).await;
        client2.sync().await.unwrap();

        sync_latest(&fixture).await;
        assert_eq!(test_object.status(), SyncStatus::Idle);
        assert_no_conflicts(&fixture);

        let actual1 = read_snippet("html.json", test_client, None).await;
        assert_eq!(actual1.as_deref(), Some(HTML_SNIPPET_2));
        teardown(&fixture).await;
    });
}

#[test]
fn sync_updating_a_snippet_conflict() {
    async_test(|d| async move {
        let fixture = setup(&d).await;
        let (test_client, client2, test_object) = (&fixture.test_client, &fixture.client2, &fixture.test_object);
        update_snippet("html.json", HTML_SNIPPET_1, client2, None).await;
        client2.sync().await.unwrap();
        sync_latest(&fixture).await;

        update_snippet("html.json", HTML_SNIPPET_2, client2, None).await;
        client2.sync().await.unwrap();

        update_snippet("html.json", HTML_SNIPPET_3, test_client, None).await;
        sync_latest(&fixture).await;
        assert_eq!(test_object.status(), SyncStatus::HasConflicts);
        let local = preview_resource(&fixture, "html.json");
        assert_previews(&test_object.conflicts().conflicts, &[local]);
        teardown(&fixture).await;
    });
}

#[test]
fn sync_updating_a_snippet_resolve_conflict() {
    async_test(|d| async move {
        let fixture = setup(&d).await;
        let (test_client, client2, test_object) = (&fixture.test_client, &fixture.client2, &fixture.test_object);
        update_snippet("html.json", HTML_SNIPPET_1, client2, None).await;
        client2.sync().await.unwrap();
        sync_latest(&fixture).await;

        update_snippet("html.json", HTML_SNIPPET_2, client2, None).await;
        client2.sync().await.unwrap();

        update_snippet("html.json", HTML_SNIPPET_3, test_client, None).await;
        sync_latest(&fixture).await;
        let preview_resource = test_object.conflicts().conflicts[0].preview_resource.clone();
        test_object.accept(&preview_resource, Some(Some(HTML_SNIPPET_2.to_string()))).await.unwrap();
        test_object.apply(false, IHeaders::new()).await.unwrap();

        assert_eq!(test_object.status(), SyncStatus::Idle);
        assert_no_conflicts(&fixture);

        let actual1 = read_snippet("html.json", test_client, None).await;
        assert_eq!(actual1.as_deref(), Some(HTML_SNIPPET_2));

        let actual = read_remote_snippets(&fixture).await;
        assert_eq!(actual, dictionary(&[("html.json", HTML_SNIPPET_2)]));
        teardown(&fixture).await;
    });
}

#[test]
fn sync_removing_a_snippet() {
    async_test(|d| async move {
        let fixture = setup(&d).await;
        let (test_client, test_object) = (&fixture.test_client, &fixture.test_object);
        update_snippet("html.json", HTML_SNIPPET_1, test_client, None).await;
        update_snippet("typescript.json", TS_SNIPPET_1, test_client, None).await;
        sync_latest(&fixture).await;

        remove_snippet("html.json", test_client).await;
        sync_latest(&fixture).await;
        assert_eq!(test_object.status(), SyncStatus::Idle);
        assert_no_conflicts(&fixture);

        let actual1 = read_snippet("typescript.json", test_client, None).await;
        assert_eq!(actual1.as_deref(), Some(TS_SNIPPET_1));
        let actual2 = read_snippet("html.json", test_client, None).await;
        assert_eq!(actual2, None);

        let actual = read_remote_snippets(&fixture).await;
        assert_eq!(actual, dictionary(&[("typescript.json", TS_SNIPPET_1)]));
        teardown(&fixture).await;
    });
}

#[test]
fn sync_removing_a_snippet_accept() {
    async_test(|d| async move {
        let fixture = setup(&d).await;
        let (test_client, client2, test_object) = (&fixture.test_client, &fixture.client2, &fixture.test_object);
        update_snippet("html.json", HTML_SNIPPET_1, client2, None).await;
        update_snippet("typescript.json", TS_SNIPPET_1, client2, None).await;
        client2.sync().await.unwrap();
        sync_latest(&fixture).await;

        remove_snippet("html.json", client2).await;
        client2.sync().await.unwrap();

        sync_latest(&fixture).await;
        assert_eq!(test_object.status(), SyncStatus::Idle);
        assert_no_conflicts(&fixture);

        let actual1 = read_snippet("typescript.json", test_client, None).await;
        assert_eq!(actual1.as_deref(), Some(TS_SNIPPET_1));
        let actual2 = read_snippet("html.json", test_client, None).await;
        assert_eq!(actual2, None);
        teardown(&fixture).await;
    });
}

#[test]
fn sync_removing_a_snippet_locally_and_updating_it_remotely() {
    async_test(|d| async move {
        let fixture = setup(&d).await;
        let (test_client, client2, test_object) = (&fixture.test_client, &fixture.client2, &fixture.test_object);
        update_snippet("html.json", HTML_SNIPPET_1, client2, None).await;
        update_snippet("typescript.json", TS_SNIPPET_1, client2, None).await;
        client2.sync().await.unwrap();
        sync_latest(&fixture).await;

        update_snippet("html.json", HTML_SNIPPET_2, client2, None).await;
        client2.sync().await.unwrap();

        remove_snippet("html.json", test_client).await;
        sync_latest(&fixture).await;

        assert_eq!(test_object.status(), SyncStatus::Idle);
        assert_no_conflicts(&fixture);

        let actual1 = read_snippet("typescript.json", test_client, None).await;
        assert_eq!(actual1.as_deref(), Some(TS_SNIPPET_1));
        let actual2 = read_snippet("html.json", test_client, None).await;
        assert_eq!(actual2.as_deref(), Some(HTML_SNIPPET_2));
        teardown(&fixture).await;
    });
}

#[test]
fn sync_removing_a_snippet_conflict() {
    async_test(|d| async move {
        let fixture = setup(&d).await;
        let (test_client, client2, test_object) = (&fixture.test_client, &fixture.client2, &fixture.test_object);
        update_snippet("html.json", HTML_SNIPPET_1, client2, None).await;
        update_snippet("typescript.json", TS_SNIPPET_1, client2, None).await;
        client2.sync().await.unwrap();
        sync_latest(&fixture).await;

        remove_snippet("html.json", client2).await;
        client2.sync().await.unwrap();

        update_snippet("html.json", HTML_SNIPPET_2, test_client, None).await;
        sync_latest(&fixture).await;

        assert_eq!(test_object.status(), SyncStatus::HasConflicts);
        let local = preview_resource(&fixture, "html.json");
        assert_previews(&test_object.conflicts().conflicts, &[local]);
        teardown(&fixture).await;
    });
}

#[test]
fn sync_removing_a_snippet_resolve_conflict() {
    async_test(|d| async move {
        let fixture = setup(&d).await;
        let (test_client, client2, test_object) = (&fixture.test_client, &fixture.client2, &fixture.test_object);
        update_snippet("html.json", HTML_SNIPPET_1, client2, None).await;
        update_snippet("typescript.json", TS_SNIPPET_1, client2, None).await;
        client2.sync().await.unwrap();
        sync_latest(&fixture).await;

        remove_snippet("html.json", client2).await;
        client2.sync().await.unwrap();

        update_snippet("html.json", HTML_SNIPPET_2, test_client, None).await;
        sync_latest(&fixture).await;
        let preview_resource = test_object.conflicts().conflicts[0].preview_resource.clone();
        test_object.accept(&preview_resource, Some(Some(HTML_SNIPPET_3.to_string()))).await.unwrap();
        test_object.apply(false, IHeaders::new()).await.unwrap();

        assert_eq!(test_object.status(), SyncStatus::Idle);
        assert_no_conflicts(&fixture);

        let actual1 = read_snippet("typescript.json", test_client, None).await;
        assert_eq!(actual1.as_deref(), Some(TS_SNIPPET_1));
        let actual2 = read_snippet("html.json", test_client, None).await;
        assert_eq!(actual2.as_deref(), Some(HTML_SNIPPET_3));

        let actual = read_remote_snippets(&fixture).await;
        assert_eq!(actual, dictionary(&[("typescript.json", TS_SNIPPET_1), ("html.json", HTML_SNIPPET_3)]));
        teardown(&fixture).await;
    });
}

#[test]
fn sync_removing_a_snippet_resolve_conflict_by_removing() {
    async_test(|d| async move {
        let fixture = setup(&d).await;
        let (test_client, client2, test_object) = (&fixture.test_client, &fixture.client2, &fixture.test_object);
        update_snippet("html.json", HTML_SNIPPET_1, client2, None).await;
        update_snippet("typescript.json", TS_SNIPPET_1, client2, None).await;
        client2.sync().await.unwrap();
        sync_latest(&fixture).await;

        remove_snippet("html.json", client2).await;
        client2.sync().await.unwrap();

        update_snippet("html.json", HTML_SNIPPET_2, test_client, None).await;
        sync_latest(&fixture).await;
        let preview_resource = test_object.conflicts().conflicts[0].preview_resource.clone();
        test_object.accept(&preview_resource, Some(None)).await.unwrap();
        test_object.apply(false, IHeaders::new()).await.unwrap();

        assert_eq!(test_object.status(), SyncStatus::Idle);
        assert_no_conflicts(&fixture);

        let actual1 = read_snippet("typescript.json", test_client, None).await;
        assert_eq!(actual1.as_deref(), Some(TS_SNIPPET_1));
        let actual2 = read_snippet("html.json", test_client, None).await;
        assert_eq!(actual2, None);

        let actual = read_remote_snippets(&fixture).await;
        assert_eq!(actual, dictionary(&[("typescript.json", TS_SNIPPET_1)]));
        teardown(&fixture).await;
    });
}

#[test]
fn sync_global_and_language_snippet() {
    async_test(|d| async move {
        let fixture = setup(&d).await;
        let (test_client, client2, test_object) = (&fixture.test_client, &fixture.client2, &fixture.test_object);
        update_snippet("global.code-snippets", GLOBAL_SNIPPET, client2, None).await;
        update_snippet("html.json", HTML_SNIPPET_1, client2, None).await;
        client2.sync().await.unwrap();

        sync_latest(&fixture).await;
        assert_eq!(test_object.status(), SyncStatus::Idle);
        assert_no_conflicts(&fixture);

        let actual1 = read_snippet("html.json", test_client, None).await;
        assert_eq!(actual1.as_deref(), Some(HTML_SNIPPET_1));
        let actual2 = read_snippet("global.code-snippets", test_client, None).await;
        assert_eq!(actual2.as_deref(), Some(GLOBAL_SNIPPET));

        let actual = read_remote_snippets(&fixture).await;
        assert_eq!(actual, dictionary(&[("html.json", HTML_SNIPPET_1), ("global.code-snippets", GLOBAL_SNIPPET)]));
        teardown(&fixture).await;
    });
}

#[test]
fn sync_should_ignore_non_snippets() {
    async_test(|d| async move {
        let fixture = setup(&d).await;
        let (test_client, client2, test_object) = (&fixture.test_client, &fixture.client2, &fixture.test_object);
        update_snippet("global.code-snippets", GLOBAL_SNIPPET, client2, None).await;
        update_snippet("html.html", HTML_SNIPPET_1, client2, None).await;
        update_snippet("typescript.json", TS_SNIPPET_1, client2, None).await;
        client2.sync().await.unwrap();

        sync_latest(&fixture).await;
        assert_eq!(test_object.status(), SyncStatus::Idle);
        assert_no_conflicts(&fixture);

        let actual1 = read_snippet("typescript.json", test_client, None).await;
        assert_eq!(actual1.as_deref(), Some(TS_SNIPPET_1));
        let actual2 = read_snippet("global.code-snippets", test_client, None).await;
        assert_eq!(actual2.as_deref(), Some(GLOBAL_SNIPPET));
        let actual3 = read_snippet("html.html", test_client, None).await;
        assert_eq!(actual3, None);

        let actual = read_remote_snippets(&fixture).await;
        assert_eq!(actual, dictionary(&[("typescript.json", TS_SNIPPET_1), ("global.code-snippets", GLOBAL_SNIPPET)]));
        teardown(&fixture).await;
    });
}

#[test]
fn previews_are_reset_after_all_conflicts_resolved() {
    async_test(|d| async move {
        let fixture = setup(&d).await;
        let (test_client, client2, test_object) = (&fixture.test_client, &fixture.client2, &fixture.test_object);
        update_snippet("html.json", HTML_SNIPPET_1, client2, None).await;
        update_snippet("typescript.json", TS_SNIPPET_1, client2, None).await;
        client2.sync().await.unwrap();

        update_snippet("html.json", HTML_SNIPPET_2, test_client, None).await;
        sync_latest(&fixture).await;

        let conflicts = test_object.conflicts().conflicts;
        test_object.accept(&conflicts[0].preview_resource, Some(Some(HTML_SNIPPET_2.to_string()))).await.unwrap();
        test_object.apply(false, IHeaders::new()).await.unwrap();

        assert!(!test_client.file_service.exists(&ext_uri().dirname(&conflicts[0].preview_resource)).await.unwrap());
        teardown(&fixture).await;
    });
}

#[test]
fn merge_when_there_are_multiple_snippets_and_all_snippets_are_merged() {
    async_test(|d| async move {
        let fixture = setup(&d).await;
        let (test_client, test_object) = (&fixture.test_client, &fixture.test_object);

        update_snippet("html.json", HTML_SNIPPET_2, test_client, None).await;
        update_snippet("typescript.json", TS_SNIPPET_2, test_client, None).await;
        let preview = sync_latest_preview(&fixture).await;

        assert_eq!(test_object.status(), SyncStatus::Syncing);
        assert_previews(&preview.expect("há o preview").resource_previews, &[preview_resource(&fixture, "html.json"), preview_resource(&fixture, "typescript.json")]);
        assert_no_conflicts(&fixture);
        teardown(&fixture).await;
    });
}

#[test]
fn merge_when_there_are_multiple_snippets_and_all_snippets_are_merged_and_applied() {
    async_test(|d| async move {
        let fixture = setup(&d).await;
        let (test_client, test_object) = (&fixture.test_client, &fixture.test_object);

        update_snippet("html.json", HTML_SNIPPET_2, test_client, None).await;
        update_snippet("typescript.json", TS_SNIPPET_2, test_client, None).await;
        let _ = sync_latest_preview(&fixture).await;
        let preview = test_object.apply(false, IHeaders::new()).await.unwrap();

        assert_eq!(test_object.status(), SyncStatus::Idle);
        assert!(preview.is_none());
        assert_no_conflicts(&fixture);
        teardown(&fixture).await;
    });
}

#[test]
fn merge_when_there_are_multiple_snippets_and_one_snippet_has_no_changes_and_one_snippet_is_merged() {
    async_test(|d| async move {
        let fixture = setup(&d).await;
        let (test_client, client2, test_object) = (&fixture.test_client, &fixture.client2, &fixture.test_object);

        update_snippet("html.json", HTML_SNIPPET_1, client2, None).await;
        client2.sync().await.unwrap();

        update_snippet("html.json", HTML_SNIPPET_1, test_client, None).await;
        update_snippet("typescript.json", TS_SNIPPET_2, test_client, None).await;
        let preview = sync_latest_preview(&fixture).await;

        assert_eq!(test_object.status(), SyncStatus::Syncing);
        assert_previews(&preview.expect("há o preview").resource_previews, &[preview_resource(&fixture, "typescript.json"), preview_resource(&fixture, "html.json")]);
        assert_no_conflicts(&fixture);
        teardown(&fixture).await;
    });
}

#[test]
fn merge_when_there_are_multiple_snippets_and_one_snippet_has_no_changes_and_snippets_is_merged_and_applied() {
    async_test(|d| async move {
        let fixture = setup(&d).await;
        let (test_client, client2, test_object) = (&fixture.test_client, &fixture.client2, &fixture.test_object);

        update_snippet("html.json", HTML_SNIPPET_1, client2, None).await;
        client2.sync().await.unwrap();

        update_snippet("html.json", HTML_SNIPPET_1, test_client, None).await;
        update_snippet("typescript.json", TS_SNIPPET_2, test_client, None).await;
        let _ = sync_latest_preview(&fixture).await;

        let preview = test_object.apply(false, IHeaders::new()).await.unwrap();

        assert_eq!(test_object.status(), SyncStatus::Idle);
        assert!(preview.is_none());
        assert_no_conflicts(&fixture);
        teardown(&fixture).await;
    });
}

#[test]
fn merge_when_there_are_multiple_snippets_with_conflicts_and_all_snippets_are_merged() {
    async_test(|d| async move {
        let fixture = setup(&d).await;
        let (test_client, client2, test_object) = (&fixture.test_client, &fixture.client2, &fixture.test_object);

        update_snippet("html.json", HTML_SNIPPET_1, client2, None).await;
        update_snippet("typescript.json", TS_SNIPPET_1, client2, None).await;
        client2.sync().await.unwrap();

        update_snippet("html.json", HTML_SNIPPET_2, test_client, None).await;
        update_snippet("typescript.json", TS_SNIPPET_2, test_client, None).await;
        let preview = sync_latest_preview(&fixture).await;

        assert_eq!(test_object.status(), SyncStatus::HasConflicts);
        let expected = [preview_resource(&fixture, "html.json"), preview_resource(&fixture, "typescript.json")];
        assert_previews(&preview.expect("há o preview").resource_previews, &expected);
        assert_previews(&test_object.conflicts().conflicts, &expected);
        teardown(&fixture).await;
    });
}

#[test]
fn accept_when_there_are_multiple_snippets_with_conflicts_and_only_one_snippet_is_accepted() {
    async_test(|d| async move {
        let fixture = setup(&d).await;
        let (test_client, client2, test_object) = (&fixture.test_client, &fixture.client2, &fixture.test_object);

        update_snippet("html.json", HTML_SNIPPET_1, client2, None).await;
        update_snippet("typescript.json", TS_SNIPPET_1, client2, None).await;
        client2.sync().await.unwrap();

        update_snippet("html.json", HTML_SNIPPET_2, test_client, None).await;
        update_snippet("typescript.json", TS_SNIPPET_2, test_client, None).await;
        let mut preview = sync_latest_preview(&fixture).await;

        assert_eq!(test_object.status(), SyncStatus::HasConflicts);
        let both = [preview_resource(&fixture, "html.json"), preview_resource(&fixture, "typescript.json")];
        assert_previews(&preview.as_ref().expect("há o preview").resource_previews, &both);
        assert_previews(&test_object.conflicts().conflicts, &both);

        preview = test_object.accept(&preview.expect("há o preview").resource_previews[0].preview_resource, Some(Some(HTML_SNIPPET_2.to_string()))).await.unwrap();

        assert_eq!(test_object.status(), SyncStatus::HasConflicts);
        assert_previews(&preview.expect("há o preview").resource_previews, &both);
        assert_previews(&test_object.conflicts().conflicts, &[preview_resource(&fixture, "typescript.json")]);
        teardown(&fixture).await;
    });
}

#[test]
fn accept_when_there_are_multiple_snippets_with_conflicts_and_all_snippets_are_accepted() {
    async_test(|d| async move {
        let fixture = setup(&d).await;
        let (test_client, client2, test_object) = (&fixture.test_client, &fixture.client2, &fixture.test_object);

        update_snippet("html.json", HTML_SNIPPET_1, client2, None).await;
        update_snippet("typescript.json", TS_SNIPPET_1, client2, None).await;
        client2.sync().await.unwrap();

        update_snippet("html.json", HTML_SNIPPET_2, test_client, None).await;
        update_snippet("typescript.json", TS_SNIPPET_2, test_client, None).await;
        let mut preview = sync_latest_preview(&fixture).await;

        assert_eq!(test_object.status(), SyncStatus::HasConflicts);
        let both = [preview_resource(&fixture, "html.json"), preview_resource(&fixture, "typescript.json")];
        assert_previews(&preview.as_ref().expect("há o preview").resource_previews, &both);
        assert_previews(&test_object.conflicts().conflicts, &both);

        preview = test_object.accept(&preview.expect("há o preview").resource_previews[0].preview_resource, Some(Some(HTML_SNIPPET_2.to_string()))).await.unwrap();
        preview = test_object.accept(&preview.expect("há o preview").resource_previews[1].preview_resource, Some(Some(TS_SNIPPET_2.to_string()))).await.unwrap();

        assert_eq!(test_object.status(), SyncStatus::Syncing);
        assert_previews(&preview.expect("há o preview").resource_previews, &both);
        assert_no_conflicts(&fixture);
        teardown(&fixture).await;
    });
}

#[test]
fn accept_when_there_are_multiple_snippets_with_conflicts_and_all_snippets_are_accepted_and_applied() {
    async_test(|d| async move {
        let fixture = setup(&d).await;
        let (test_client, client2, test_object) = (&fixture.test_client, &fixture.client2, &fixture.test_object);
        update_snippet("html.json", HTML_SNIPPET_1, client2, None).await;
        update_snippet("typescript.json", TS_SNIPPET_1, client2, None).await;
        client2.sync().await.unwrap();

        update_snippet("html.json", HTML_SNIPPET_2, test_client, None).await;
        update_snippet("typescript.json", TS_SNIPPET_2, test_client, None).await;
        let mut preview = sync_latest_preview(&fixture).await;

        assert_eq!(test_object.status(), SyncStatus::HasConflicts);
        let both = [preview_resource(&fixture, "html.json"), preview_resource(&fixture, "typescript.json")];
        assert_previews(&preview.as_ref().expect("há o preview").resource_previews, &both);
        assert_previews(&test_object.conflicts().conflicts, &both);

        preview = test_object.accept(&preview.expect("há o preview").resource_previews[0].preview_resource, Some(Some(HTML_SNIPPET_2.to_string()))).await.unwrap();
        preview = test_object.accept(&preview.expect("há o preview").resource_previews[1].preview_resource, Some(Some(TS_SNIPPET_2.to_string()))).await.unwrap();
        preview = test_object.apply(false, IHeaders::new()).await.unwrap();

        assert_eq!(test_object.status(), SyncStatus::Idle);
        assert!(preview.is_none());
        assert_no_conflicts(&fixture);
        teardown(&fixture).await;
    });
}

#[test]
fn sync_profile_snippets() {
    async_test(|d| async move {
        let fixture = setup(&d).await;
        let test_client = &fixture.test_client;
        let client2 = d.add_rc(UserDataSyncClient::set_up(fixture.server.clone(), true).await.unwrap());
        let profile = client2.user_data_profiles_service.create_named_profile("profile1", None, None).await.unwrap();
        update_snippet("html.json", HTML_SNIPPET_1, &client2, Some(&profile)).await;
        client2.sync().await.unwrap();

        test_client.sync().await.unwrap();

        let synced_profile = test_client.user_data_profiles_service.profiles().into_iter().find(|p| p.id == profile.id).expect("o perfil foi sincronizado");
        let content = read_snippet("html.json", test_client, Some(&synced_profile)).await;
        assert_eq!(content.as_deref(), Some(HTML_SNIPPET_1));
        teardown(&fixture).await;
    });
}
