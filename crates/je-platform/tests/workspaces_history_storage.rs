//! Porte de `src/vs/platform/workspaces/test/node/workspacesHistoryStorage.test.ts`.
//!
//! Um `#[test]` por caso do original, na mesma ordem. A fonte que ele importa (`restoreRecentlyOpened`, `toStoreData` e os
//! tipos de recentes de `workspaces.ts`) é [`je_platform::workspaces`].
//!
//! Notas de tradução:
//! - `toStoreData` devolve o valor guardado ([`Dynamic`]) e `restoreRecentlyOpened` o lê; `JSON.parse` é o parser de
//!   `je_base::json` mais `from_json`.
//! - `isRecentFolder(actualRecent)` sobre `IRecentWorkspace | IRecentFolder` é o `match` sobre
//!   [`RecentWorkspaceOrFolder`]; o `(<IRecentFolder>expectedRecent).folderUri` de um esperado de outro tipo é `undefined` no
//!   original (e a comparação falha), então um par de tipos diferentes reprova.
//! - O original compara `actualRecent.remoteAuthority` com ele mesmo (uma tautologia, sem efeito); aqui a comparação é com o
//!   `remoteAuthority` esperado, que é a intenção.
//! - `assert.deepStrictEqual` do `toStoreData` é o `assert_deep_strict_equal` sobre valores dinâmicos, onde `label` e
//!   `remoteAuthority` ausentes são as chaves com `undefined`.
//! - `tmpdir()` é `std::env::temp_dir()`, só para contas de caminho (nenhum arquivo é tocado).
//! - O `ensureNoDisposablesAreLeakedInTestSuite()` fica no fim da suíte no original; o [`LeakGuard`] é criado no começo do
//!   caso e descartado no fim.

mod common;

use common::workspace_asserts::{assert_equal_uri, assert_equal_workspace, temp_path, to_workspace};
use common::{assert_deep_strict_equal, ensure_no_disposables_are_leaked_in_test_suite};
use je_base::dynamic::Dynamic;
use je_base::json;
use je_base::uri::URI;
use je_platform::js::from_json;
use je_platform::log::NullLogService;
use je_platform::resources::uri_file;
use je_platform::workspace::WorkspaceIdentifier;
use je_platform::workspaces::{RecentFile, RecentFolder, RecentWorkspace, RecentWorkspaceOrFolder, RecentlyOpened, restore_recently_opened, to_store_data};

/// `label` e `remoteAuthority` de um recente de `IRecentlyOpened.workspaces`.
fn label_and_authority(recent: &RecentWorkspaceOrFolder) -> (&Option<String>, &Option<String>) {
    match recent {
        RecentWorkspaceOrFolder::Workspace(workspace) => (&workspace.label, &workspace.remote_authority),
        RecentWorkspaceOrFolder::Folder(folder) => (&folder.label, &folder.remote_authority),
    }
}

fn assert_equal_recently_opened(actual: &RecentlyOpened, expected: &RecentlyOpened, message: &str) {
    assert_eq!(actual.files.len(), expected.files.len(), "{message}");
    for (actual, expected) in actual.files.iter().zip(&expected.files) {
        assert_equal_uri(Some(&actual.file_uri), Some(&expected.file_uri), message);
        assert_eq!(actual.label, expected.label);
        assert_eq!(actual.remote_authority, expected.remote_authority);
    }
    assert_eq!(actual.workspaces.len(), expected.workspaces.len(), "{message}");
    for (actual_recent, expected_recent) in actual.workspaces.iter().zip(&expected.workspaces) {
        match (actual_recent, expected_recent) {
            (RecentWorkspaceOrFolder::Folder(actual), RecentWorkspaceOrFolder::Folder(expected)) => {
                assert_equal_uri(Some(&actual.folder_uri), Some(&expected.folder_uri), message);
            }
            (RecentWorkspaceOrFolder::Workspace(actual), RecentWorkspaceOrFolder::Workspace(expected)) => {
                assert_equal_workspace(Some(&actual.workspace), Some(&expected.workspace), message);
            }
            _ => panic!("{message}: o recente restaurado e o esperado são de tipos diferentes"),
        }
        let ((actual_label, actual_authority), (expected_label, expected_authority)) = (label_and_authority(actual_recent), label_and_authority(expected_recent));
        assert_eq!(actual_label, expected_label);
        assert_eq!(actual_authority, expected_authority);
    }
}

fn assert_restoring(state: &RecentlyOpened, message: &str) {
    let stored = to_store_data(state);
    let restored = restore_recently_opened(Some(&stored), &NullLogService::shared());
    assert_equal_recently_opened(state, &restored, message);
}

/// `JSON.parse(text)`.
fn json_parse(text: &str) -> Dynamic {
    let mut errors = Vec::new();
    let value = json::parse_str(text, &mut errors).expect("JSON inválido");
    assert!(errors.is_empty(), "JSON inválido");
    from_json(&value)
}

/// `{ workspace }`.
fn workspace(workspace: WorkspaceIdentifier) -> RecentWorkspaceOrFolder {
    RecentWorkspaceOrFolder::Workspace(RecentWorkspace { workspace, label: None, remote_authority: None })
}

/// `{ folderUri }`.
fn folder(folder_uri: URI) -> RecentWorkspaceOrFolder {
    RecentWorkspaceOrFolder::Folder(RecentFolder { folder_uri, label: None, remote_authority: None })
}

/// `{ fileUri }`.
fn file(file_uri: URI) -> RecentFile {
    RecentFile { file_uri, label: None, remote_authority: None }
}

#[test]
fn storing_and_restoring() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();

    let test_ws_path = uri_file(&temp_path(&["windowStateTest", "test.code-workspace"]));
    let test_file_uri = uri_file(&temp_path(&["windowStateTest", "testFile.txt"]));
    let test_folder_uri = uri_file(&temp_path(&["windowStateTest", "testFolder"]));

    let test_remote_folder_uri = URI::parse("foo://bar/c/e", false);
    let test_remote_file_uri = URI::parse("foo://bar/c/d.txt", false);
    let test_remote_ws_uri = URI::parse("foo://bar/c/test.code-workspace", false);

    let mut ro = RecentlyOpened { files: vec![], workspaces: vec![] };
    assert_restoring(&ro, "empty");
    ro = RecentlyOpened { files: vec![file(test_file_uri.clone())], workspaces: vec![] };
    assert_restoring(&ro, "file");
    ro = RecentlyOpened { files: vec![], workspaces: vec![folder(test_folder_uri.clone())] };
    assert_restoring(&ro, "folder");
    ro = RecentlyOpened { files: vec![], workspaces: vec![workspace(to_workspace(test_ws_path.clone())), folder(test_folder_uri)] };
    assert_restoring(&ro, "workspaces and folders");

    ro = RecentlyOpened { files: vec![file(test_remote_file_uri.clone())], workspaces: vec![workspace(to_workspace(test_remote_ws_uri)), folder(test_remote_folder_uri.clone())] };
    assert_restoring(&ro, "remote workspaces and folders");
    ro = RecentlyOpened {
        files: vec![RecentFile { label: Some("abc".to_string()), ..file(test_file_uri) }],
        workspaces: vec![
            RecentWorkspaceOrFolder::Workspace(RecentWorkspace { label: Some("def".to_string()), workspace: to_workspace(test_ws_path.clone()), remote_authority: None }),
            folder(test_remote_folder_uri.clone()),
        ],
    };
    assert_restoring(&ro, "labels");
    ro = RecentlyOpened {
        files: vec![RecentFile { label: Some("abc".to_string()), remote_authority: Some("test".to_string()), ..file(test_remote_file_uri) }],
        workspaces: vec![
            RecentWorkspaceOrFolder::Workspace(RecentWorkspace { label: Some("def".to_string()), remote_authority: Some("test".to_string()), workspace: to_workspace(test_ws_path) }),
            RecentWorkspaceOrFolder::Folder(RecentFolder { folder_uri: test_remote_folder_uri, label: None, remote_authority: Some("test".to_string()) }),
        ],
    };
    assert_restoring(&ro, "authority");
}

#[test]
fn open_1_55() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();

    let v1_55 = r#"{
			"entries": [
				{
					"folderUri": "foo://bar/23/43",
					"remoteAuthority": "test+test"
				},
				{
					"workspace": {
						"id": "53b714b46ef1a2d4346568b4f591028c",
						"configPath": "file:///home/user/workspaces/testing/custom.code-workspace"
					}
				},
				{
					"folderUri": "file:///home/user/workspaces/testing/folding",
					"label": "abc"
				},
				{
					"fileUri": "file:///home/user/.config/code-oss-dev/storage.json",
					"label": "def"
				}
			]
		}"#;

    let windows_state = restore_recently_opened(Some(&json_parse(v1_55)), &NullLogService::shared());
    let expected = RecentlyOpened {
        files: vec![RecentFile { label: Some("def".to_string()), ..file(URI::parse("file:///home/user/.config/code-oss-dev/storage.json", false)) }],
        workspaces: vec![
            RecentWorkspaceOrFolder::Folder(RecentFolder { folder_uri: URI::parse("foo://bar/23/43", false), label: None, remote_authority: Some("test+test".to_string()) }),
            workspace(WorkspaceIdentifier {
                id: "53b714b46ef1a2d4346568b4f591028c".to_string(),
                config_path: URI::parse("file:///home/user/workspaces/testing/custom.code-workspace", false),
            }),
            RecentWorkspaceOrFolder::Folder(RecentFolder {
                folder_uri: URI::parse("file:///home/user/workspaces/testing/folding", false),
                label: Some("abc".to_string()),
                remote_authority: None,
            }),
        ],
    };

    assert_equal_recently_opened(&windows_state, &expected, "v1_33");
}

#[test]
fn to_store_data_drops_label_if_it_matches_path() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();

    let actual = to_store_data(&RecentlyOpened {
        workspaces: vec![],
        files: vec![RecentFile { file_uri: URI::parse("file:///foo/bar/test.txt", false), label: Some("/foo/bar/test.txt".to_string()), remote_authority: None }],
    });
    let expected = Dynamic::object([(
        "entries",
        Dynamic::array([Dynamic::object([("fileUri", Dynamic::string("file:///foo/bar/test.txt")), ("label", Dynamic::Undefined), ("remoteAuthority", Dynamic::Undefined)])]),
    )]);
    assert_deep_strict_equal(&actual, &expected);
}
