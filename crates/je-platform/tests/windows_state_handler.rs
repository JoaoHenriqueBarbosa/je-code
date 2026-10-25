//! Porte de `src/vs/platform/windows/test/electron-main/windowsStateHandler.test.ts`.
//!
//! Um `#[test]` por caso do original, na mesma ordem.
//!
//! Notas de tradução:
//! - `getWindowsStateStoreData` devolve o valor guardado ([`Dynamic`]) e `restoreWindowsState` o lê, como o
//!   `IStateService` o entrega; `JSON.parse` é o parser de `je_base::json` mais `from_json`.
//! - `mode: 0` do original é `WindowMode::Maximized`, que vale 0.
//! - `assert.deepStrictEqual(expected, actual)` sobre `undefined` ou o estado de UI é a igualdade de valor dos tipos
//!   ([`PartialEq`]); `assertEqualURI` compara o `toString()`.
//! - `tmpdir()` é `std::env::temp_dir()`, só para contas de caminho (nenhum arquivo é tocado).
//! - O `ensureNoDisposablesAreLeakedInTestSuite()` fica no fim da suíte no original; o [`LeakGuard`] é criado no começo do
//!   caso e descartado no fim.

mod common;

use common::workspace_asserts::{assert_equal_uri, assert_equal_workspace, temp_path, to_workspace};
use common::ensure_no_disposables_are_leaked_in_test_suite;
use je_base::dynamic::Dynamic;
use je_base::json;
use je_base::uri::URI;
use je_platform::js::from_json;
use je_platform::resources::uri_file;
use je_platform::window::electron_main::{WindowMode, WindowUiState};
use je_platform::windows::windows_state_handler::{WindowState, WindowsState, get_windows_state_store_data, restore_windows_state};
use je_platform::workspace::WorkspaceIdentifier;

fn get_ui_state() -> WindowUiState {
    WindowUiState {
        x: Some(0.0),
        y: Some(10.0),
        width: Some(100.0),
        height: Some(200.0),
        mode: Some(WindowMode::Maximized), // 0
        ..Default::default()
    }
}

fn assert_equal_window_state(expected: Option<&WindowState>, actual: Option<&WindowState>, message: &str) {
    let (Some(expected), Some(actual)) = (expected, actual) else {
        assert_eq!(expected, actual, "{message}");
        return;
    };
    assert_eq!(expected.backup_path, actual.backup_path, "{message}");
    assert_equal_uri(expected.folder_uri.as_ref(), actual.folder_uri.as_ref(), message);
    assert_eq!(expected.remote_authority, actual.remote_authority, "{message}");
    assert_equal_workspace(expected.workspace.as_ref(), actual.workspace.as_ref(), message);
    assert_eq!(expected.ui_state, actual.ui_state, "{message}");
}

fn assert_equal_windows_state(expected: &WindowsState, actual: &WindowsState, message: &str) {
    assert_equal_window_state(expected.last_plugin_development_host_window.as_ref(), actual.last_plugin_development_host_window.as_ref(), message);
    assert_equal_window_state(expected.last_active_window.as_ref(), actual.last_active_window.as_ref(), message);
    assert_eq!(expected.opened_windows.len(), actual.opened_windows.len(), "{message}");
    for (expected, actual) in expected.opened_windows.iter().zip(&actual.opened_windows) {
        assert_equal_window_state(Some(expected), Some(actual), message);
    }
}

fn assert_restoring(state: &WindowsState, message: &str) {
    let stored = get_windows_state_store_data(state);
    let restored = restore_windows_state(&stored);
    assert_equal_windows_state(state, &restored, message);
}

/// `JSON.parse(text)`.
fn json_parse(text: &str) -> Dynamic {
    let mut errors = Vec::new();
    let value = json::parse_str(text, &mut errors).expect("JSON inválido");
    assert!(errors.is_empty(), "JSON inválido");
    from_json(&value)
}

#[test]
fn storing_and_restoring() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();

    let test_backup_path1 = temp_path(&["windowStateTest", "backupFolder1"]);
    let test_backup_path2 = temp_path(&["windowStateTest", "backupFolder2"]);

    let test_ws_path = uri_file(&temp_path(&["windowStateTest", "test.code-workspace"]));
    let test_folder_uri = uri_file(&temp_path(&["windowStateTest", "testFolder"]));

    let test_remote_folder_uri = URI::parse("foo://bar/c/d", false);

    let mut window_state = WindowsState { opened_windows: vec![], ..Default::default() };
    assert_restoring(&window_state, "no windows");
    window_state = WindowsState { opened_windows: vec![WindowState { backup_path: Some(test_backup_path1.clone()), ui_state: get_ui_state(), ..Default::default() }], ..Default::default() };
    assert_restoring(&window_state, "empty workspace");

    window_state = WindowsState {
        opened_windows: vec![WindowState { backup_path: Some(test_backup_path1.clone()), ui_state: get_ui_state(), workspace: Some(to_workspace(test_ws_path)), ..Default::default() }],
        ..Default::default()
    };
    assert_restoring(&window_state, "workspace");

    window_state = WindowsState {
        opened_windows: vec![WindowState { backup_path: Some(test_backup_path2.clone()), ui_state: get_ui_state(), folder_uri: Some(test_folder_uri.clone()), ..Default::default() }],
        ..Default::default()
    };
    assert_restoring(&window_state, "folder");

    window_state = WindowsState {
        opened_windows: vec![
            WindowState { backup_path: Some(test_backup_path1.clone()), ui_state: get_ui_state(), folder_uri: Some(test_folder_uri.clone()), ..Default::default() },
            WindowState {
                backup_path: Some(test_backup_path1),
                ui_state: get_ui_state(),
                folder_uri: Some(test_remote_folder_uri),
                remote_authority: Some("bar".to_string()),
                ..Default::default()
            },
        ],
        ..Default::default()
    };
    assert_restoring(&window_state, "multiple windows");

    window_state = WindowsState {
        last_active_window: Some(WindowState { backup_path: Some(test_backup_path2.clone()), ui_state: get_ui_state(), folder_uri: Some(test_folder_uri.clone()), ..Default::default() }),
        opened_windows: vec![],
        ..Default::default()
    };
    assert_restoring(&window_state, "lastActiveWindow");

    window_state = WindowsState {
        last_plugin_development_host_window: Some(WindowState { backup_path: Some(test_backup_path2), ui_state: get_ui_state(), folder_uri: Some(test_folder_uri), ..Default::default() }),
        opened_windows: vec![],
        ..Default::default()
    };
    assert_restoring(&window_state, "lastPluginDevelopmentHostWindow");
}

#[test]
fn open_1_32() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();

    let v1_32_workspace = r#"{
			"openedWindows": [],
			"lastActiveWindow": {
				"workspaceIdentifier": {
					"id": "53b714b46ef1a2d4346568b4f591028c",
					"configURIPath": "file:///home/user/workspaces/testing/custom.code-workspace"
				},
				"backupPath": "/home/user/.config/code-oss-dev/Backups/53b714b46ef1a2d4346568b4f591028c",
				"uiState": {
					"mode": 0,
					"x": 0,
					"y": 27,
					"width": 2560,
					"height": 1364
				}
			}
		}"#;

    let mut windows_state = restore_windows_state(&json_parse(v1_32_workspace));
    let mut expected = WindowsState {
        opened_windows: vec![],
        last_active_window: Some(WindowState {
            backup_path: Some("/home/user/.config/code-oss-dev/Backups/53b714b46ef1a2d4346568b4f591028c".to_string()),
            ui_state: WindowUiState { mode: Some(WindowMode::Maximized), x: Some(0.0), y: Some(27.0), width: Some(2560.0), height: Some(1364.0), ..Default::default() },
            workspace: Some(WorkspaceIdentifier {
                id: "53b714b46ef1a2d4346568b4f591028c".to_string(),
                config_path: URI::parse("file:///home/user/workspaces/testing/custom.code-workspace", false),
            }),
            ..Default::default()
        }),
        ..Default::default()
    };

    assert_equal_windows_state(&expected, &windows_state, "v1_32_workspace");

    let v1_32_folder = r#"{
			"openedWindows": [],
			"lastActiveWindow": {
				"folder": "file:///home/user/workspaces/testing/folding",
				"backupPath": "/home/user/.config/code-oss-dev/Backups/1daac1621c6c06f9e916ac8062e5a1b5",
				"uiState": {
					"mode": 1,
					"x": 625,
					"y": 263,
					"width": 1718,
					"height": 953
				}
			}
		}"#;

    windows_state = restore_windows_state(&json_parse(v1_32_folder));
    expected = WindowsState {
        opened_windows: vec![],
        last_active_window: Some(WindowState {
            backup_path: Some("/home/user/.config/code-oss-dev/Backups/1daac1621c6c06f9e916ac8062e5a1b5".to_string()),
            ui_state: WindowUiState { mode: Some(WindowMode::Normal), x: Some(625.0), y: Some(263.0), width: Some(1718.0), height: Some(953.0), ..Default::default() },
            folder_uri: Some(URI::parse("file:///home/user/workspaces/testing/folding", false)),
            ..Default::default()
        }),
        ..Default::default()
    };
    assert_equal_windows_state(&expected, &windows_state, "v1_32_folder");

    let v1_32_empty_window = r#" {
			"openedWindows": [
			],
			"lastActiveWindow": {
				"backupPath": "/home/user/.config/code-oss-dev/Backups/1549539668998",
				"uiState": {
					"mode": 1,
					"x": 768,
					"y": 336,
					"width": 1200,
					"height": 800
				}
			}
		}"#;

    windows_state = restore_windows_state(&json_parse(v1_32_empty_window));
    expected = WindowsState {
        opened_windows: vec![],
        last_active_window: Some(WindowState {
            backup_path: Some("/home/user/.config/code-oss-dev/Backups/1549539668998".to_string()),
            ui_state: WindowUiState { mode: Some(WindowMode::Normal), x: Some(768.0), y: Some(336.0), width: Some(1200.0), height: Some(800.0), ..Default::default() },
            ..Default::default()
        }),
        ..Default::default()
    };
    assert_equal_windows_state(&expected, &windows_state, "v1_32_empty_window");
}
