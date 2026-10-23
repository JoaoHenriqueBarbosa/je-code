//! Porte de `src/vs/platform/workspaces/test/common/workspaceIdentifier.test.ts` (suíte `Workspaces`), caso a caso e na mesma ordem.

mod common;

use common::*;
use je_base::uri::URI;
use je_platform::workspaces::workspace_identifier::{get_single_folder_workspace_identifier, get_workspace_identifier};

#[test]
fn workspace_identifiers_are_stable() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    // workspace identifier
    assert_eq!(get_workspace_identifier(&URI::parse("vscode-remote:/hello/test", false)).id, "474434e4");

    // single folder identifier
    assert_eq!(get_single_folder_workspace_identifier(&URI::parse("vscode-remote:/hello/test", false)).id, "474434e4");
}
