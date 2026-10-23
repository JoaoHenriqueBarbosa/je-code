//! Porte de `src/vs/platform/workspaces/test/common/workspaces.test.ts` (suíte `Workspaces`), caso a caso e na mesma ordem.
//!
//! Divergências: o `URI.file('foo').toJSON()` do original é o objeto serializado equivalente (`$mid`, `fsPath`, `external`,
//! `path`, `scheme`), montado à mão; `toWorkspaceIdentifier` recebe um `WorkspaceSnapshot` no lugar do literal.

mod common;

use common::*;
use je_platform::resources::uri_file;
use je_platform::workspace::{
    AnyWorkspaceIdentifier, WorkspaceFolder, WorkspaceSnapshot, has_workspace_file_extension, revive_identifier, to_workspace_identifier,
};

fn serialized_file_uri(path: &str) -> String {
    format!("{{'$mid': 1, 'path': '/{path}', 'scheme': 'file'}}")
}

#[test]
fn revive_identifier_test() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    let serialized_workspace_identifier = j(&format!("{{'id': 'id', 'configPath': {}}}", serialized_file_uri("foo")));
    assert!(revive_identifier(&serialized_workspace_identifier).is_some_and(|i| i.is_workspace_identifier()));

    let serialized_single_folder_workspace_identifier = j(&format!("{{'id': 'id', 'uri': {}}}", serialized_file_uri("foo")));
    assert!(revive_identifier(&serialized_single_folder_workspace_identifier).is_some_and(|i| i.is_single_folder_workspace_identifier()));

    let serialized_empty_workspace_identifier = j("{'id': 'id'}");
    assert_eq!(revive_identifier(&serialized_empty_workspace_identifier).unwrap().id(), "id");
    // isWorkspaceIdentifier(serializedEmptyWorkspaceIdentifier) e isSingleFolderWorkspaceIdentifier(...) são falsos:
    // o objeto não tem `configPath` nem `uri`.
    assert!(!revive_identifier(&serialized_empty_workspace_identifier).unwrap().is_workspace_identifier());
    assert!(!revive_identifier(&serialized_empty_workspace_identifier).unwrap().is_single_folder_workspace_identifier());

    assert!(revive_identifier(&je_base::dynamic::Dynamic::Undefined).is_none());
}

#[test]
fn has_workspace_file_extension_test() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    assert!(!has_workspace_file_extension("something"));
    assert!(has_workspace_file_extension("something.code-workspace"));
}

#[test]
fn to_workspace_identifier_test() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    let snapshot = |folders: Vec<WorkspaceFolder>, configuration| WorkspaceSnapshot { id: "id".into(), folders, transient: false, configuration, name: None };

    let identifier = to_workspace_identifier(&snapshot(vec![], None));
    assert!(identifier.is_empty_workspace_identifier());
    assert!(!identifier.is_workspace_identifier());

    let identifier = to_workspace_identifier(&snapshot(vec![WorkspaceFolder::new(uri_file("test"), "test", 0, None)], None));
    assert!(identifier.is_single_folder_workspace_identifier());
    assert!(!identifier.is_workspace_identifier());

    let identifier = to_workspace_identifier(&snapshot(vec![], Some(uri_file("test.code-workspace"))));
    assert!(!identifier.is_single_folder_workspace_identifier());
    assert!(identifier.is_workspace_identifier());
    assert!(matches!(identifier, AnyWorkspaceIdentifier::Workspace(_)));
}
