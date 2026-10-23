//! Porte de `src/vs/platform/workspaces/test/node/workspaces.test.ts` (suíte `Workspaces`), caso a caso e na mesma ordem.
//!
//! Divergências: o `fs.Stats` do `getSingleFolderWorkspaceIdentifier` é o `FolderStat` com `ino` e `birthtimeMs` (os
//! campos que o original lê); `fs.statSync` vira `std::fs::metadata` (`ino` do `MetadataExt` no Unix).

mod common;

use std::path::PathBuf;

use common::disk::get_random_test_path;
use je_base::platform::Platform;
use je_base::uri::URI;
use je_platform::workspaces::node::workspaces::{FolderStat, get_single_folder_workspace_identifier, get_workspace_identifier};

struct TestDir {
    path: PathBuf,
}

impl TestDir {
    fn new() -> Self {
        let path = get_random_test_path(&["vsctests", "workspacesmanagementmainservice"]);
        std::fs::create_dir_all(&path).unwrap();
        Self { path }
    }
}

impl Drop for TestDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.path);
    }
}

#[cfg(unix)]
fn stat_of(path: &std::path::Path) -> FolderStat {
    use std::os::unix::fs::MetadataExt;
    let metadata = std::fs::metadata(path).unwrap();
    FolderStat { ino: metadata.ino() as f64, birthtime_ms: metadata.ctime() as f64 * 1000.0 }
}

#[cfg(not(unix))]
fn stat_of(path: &std::path::Path) -> FolderStat {
    let metadata = std::fs::metadata(path).unwrap();
    let created = metadata.created().ok().and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok()).map_or(0.0, |d| d.as_millis() as f64);
    FolderStat { ino: 0.0, birthtime_ms: created }
}

#[test]
fn get_single_workspace_identifier() {
    let platform = Platform::detect();
    let test_dir = TestDir::new();

    let non_local_uri = URI::parse("myscheme://server/work/p/f1", false);
    let non_local_uri_id = get_single_folder_workspace_identifier(&non_local_uri, None, &platform);
    assert!(non_local_uri_id.is_some_and(|i| !i.id.is_empty()));

    let f1 = test_dir.path.join("f1");
    let local_non_existing_uri = URI::file(f1.to_str().unwrap(), platform.path_flavor());
    let local_non_existing_uri_id = get_single_folder_workspace_identifier(&local_non_existing_uri, None, &platform);
    assert!(local_non_existing_uri_id.is_none());

    std::fs::create_dir(&f1).unwrap();
    let local_existing_uri = URI::file(f1.to_str().unwrap(), platform.path_flavor());
    let local_existing_uri_id = get_single_folder_workspace_identifier(&local_existing_uri, Some(&stat_of(&f1)), &platform);
    assert!(local_existing_uri_id.is_some_and(|i| !i.id.is_empty()));
}

#[test]
fn workspace_identifiers_are_stable() {
    let platform = Platform::detect();
    let windows = platform.is_windows;

    // workspace identifier (local)
    assert_eq!(
        get_workspace_identifier(&URI::file("/hello/test", platform.path_flavor()), &platform).id,
        if windows { "9f3efb614e2cd7924e4b8076e6c72233" } else { "e36736311be12ff6d695feefe415b3e8" }
    );

    // single folder identifier (local)
    let fake_stat = FolderStat { ino: 1_611_312_115_129.0, birthtime_ms: 1_611_312_115_129.0 };
    assert_eq!(
        get_single_folder_workspace_identifier(&URI::file("/hello/test", platform.path_flavor()), Some(&fake_stat), &platform).map(|i| i.id).as_deref(),
        Some(if windows { "9a8441e897e5174fa388bc7ef8f7a710" } else { "1d726b3d516dc2a6d343abf4797eaaef" })
    );

    // workspace identifier (remote)
    assert_eq!(get_workspace_identifier(&URI::parse("vscode-remote:/hello/test", false), &platform).id, "786de4f224d57691f218dc7f31ee2ee3");

    // single folder identifier (remote)
    assert_eq!(
        get_single_folder_workspace_identifier(&URI::parse("vscode-remote:/hello/test", false), None, &platform).map(|i| i.id).as_deref(),
        Some("786de4f224d57691f218dc7f31ee2ee3")
    );
}
