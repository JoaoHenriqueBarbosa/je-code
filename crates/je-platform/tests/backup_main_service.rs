//! Porte de `src/vs/platform/backup/test/electron-main/backupMainService.test.ts`.
//!
//! Um `#[test]` por caso do original, na mesma ordem.
//!
//! Notas de tradução:
//! - O teste usa o sistema de arquivos real numa pasta temporária (`getRandomTestPath`): cada caso roda no laço de
//!   eventos real ([`run_on_real_loop`]) com um [`TestDir`], apagado no fim (o `teardown` do original).
//! - A subclasse `TestBackupMainService` não existe: o `backupHome` entra no construtor e `getFolderHash`,
//!   `getWorkspaceBackups` e `getFolderBackups` são públicos (`testGetFolderHash`, `testGetWorkspaceBackups` e
//!   `testGetFolderBackups` do original).
//! - O `InMemoryTestStateMainService` de `workbenchTestServices.ts` está aqui, como suporte do teste.
//! - O `new EnvironmentMainService(...)` do original só fornecia o `backupHome`, que a subclasse sobrescrevia: não é
//!   preciso criá-lo.
//! - `JSON.parse` e `JSON.stringify` são o parser do `je_base::json` e o `json_stringify` de `Dynamic`.
//! - `test.skip` no Linux (`getWorkspaceHash`) é `#[ignore]` no Linux, e os casos que dependem de `platform.isLinux`
//!   ou `isWindows` usam `cfg!`.

mod common;

use std::cell::RefCell;
use std::collections::HashMap;
use std::future::Future;
use std::path::Path;
use std::rc::Rc;

use common::disk::{TestDir, run_on_real_loop};
use common::{ensure_no_disposables_are_leaked_in_test_suite, join};
use je_base::dynamic::{Dynamic, json_stringify};
use je_base::json;
use je_base::network::schemas;
use je_base::promise::Promise;
use je_base::uri::URI;
use je_platform::backup::backup_main_service::{BackupMainService, IBackupMainService};
use je_platform::files::files::HotExitConfiguration;
use je_platform::backup::{BackupInfo, FolderBackupInfo, WorkspaceBackupInfo};
use je_platform::configuration::test_configuration_service::TestConfigurationService;
use je_platform::js::from_json;
use je_platform::log::{ConsoleMainLogger, DEFAULT_LOG_LEVEL, DefaultLogService, LogServiceRef};
use je_platform::resources::{ext_uri, fs_path, uri_file};
use je_platform::state::{StateReadService, StateService};
use je_platform::workspace::WorkspaceIdentifier;
use je_platform::workspaces::node::md5::md5_hex;

/// `InMemoryTestStateMainService` de `workbenchTestServices.ts`.
#[derive(Default)]
struct InMemoryTestStateMainService {
    data: RefCell<HashMap<String, Dynamic>>,
}

impl StateReadService for InMemoryTestStateMainService {
    fn get_item(&self, key: &str, _default_value: Dynamic) -> Dynamic {
        self.data.borrow().get(key).cloned().unwrap_or(Dynamic::Undefined)
    }
}

impl StateService for InMemoryTestStateMainService {
    fn set_item(&self, key: &str, data: Dynamic) {
        self.data.borrow_mut().insert(key.to_string(), data);
    }

    fn set_items(&self, items: &[(String, Dynamic)]) {
        for (key, data) in items {
            self.data.borrow_mut().insert(key.clone(), data.clone());
        }
    }

    fn remove_item(&self, key: &str) {
        self.data.borrow_mut().remove(key);
    }

    fn close(&self) -> Promise<()> {
        Promise::resolved(())
    }
}

fn uri_text(uri: &URI) -> String {
    uri.to_string(false).to_string_lossy()
}

fn text_of(value: &Dynamic) -> String {
    match value {
        Dynamic::String(text) => text.to_string_lossy(),
        other => panic!("esperava uma string: {other:?}"),
    }
}

fn assert_deep(actual: &Dynamic, expected: &Dynamic) {
    assert!(actual.deep_strict_equals(expected), "{actual:?} != {expected:?}");
}

fn assert_equal_folder_infos(actual: &[FolderBackupInfo], expected: &[FolderBackupInfo]) {
    let with_uri_as_string = |f: &FolderBackupInfo| (uri_text(&f.folder_uri), f.remote_authority.clone());
    assert_eq!(actual.iter().map(with_uri_as_string).collect::<Vec<_>>(), expected.iter().map(with_uri_as_string).collect::<Vec<_>>());
}

fn sanitize_path(path: &str) -> String {
    if cfg!(target_os = "linux") { path.to_string() } else { path.to_lowercase() }
}

fn to_workspace(path: &str) -> WorkspaceIdentifier {
    WorkspaceIdentifier { id: md5_hex(sanitize_path(path).as_bytes()), config_path: uri_file(path) }
}

fn to_workspace_backup_info(path: &str, remote_authority: Option<&str>) -> WorkspaceBackupInfo {
    WorkspaceBackupInfo { workspace: to_workspace(path), remote_authority: remote_authority.map(str::to_string) }
}

fn to_folder_backup_info(uri: &URI, remote_authority: Option<&str>) -> FolderBackupInfo {
    FolderBackupInfo { folder_uri: uri.clone(), remote_authority: remote_authority.map(str::to_string) }
}

/// `toSerializedWorkspace`.
fn to_serialized_workspace(ws: &WorkspaceIdentifier) -> Dynamic {
    Dynamic::object([("id", Dynamic::string(&ws.id)), ("configURIPath", Dynamic::string(&uri_text(&ws.config_path)))])
}

fn serialized_folder(folder_uri: &str) -> Dynamic {
    Dynamic::object([("folderUri", Dynamic::string(folder_uri))])
}

/// Um `ISerializedBackupWorkspaces` sem janelas vazias.
fn serialized_backup_workspaces(workspaces: Vec<Dynamic>, folders: Vec<Dynamic>) -> Dynamic {
    Dynamic::object([("workspaces", Dynamic::array(workspaces)), ("folders", Dynamic::array(folders)), ("emptyWindows", Dynamic::array([]))])
}

fn items_of(value: &Dynamic) -> Vec<Dynamic> {
    match value {
        Dynamic::Array(items) => items.to_vec(),
        other => panic!("esperava um array: {other:?}"),
    }
}

fn mkdir(path: &str) {
    std::fs::create_dir(path).unwrap();
}

fn exists(path: &str) -> bool {
    Path::new(path).exists()
}

fn foo_file() -> URI {
    uri_file(if cfg!(windows) { "C:\\foo" } else { "/foo" })
}

fn bar_file() -> URI {
    uri_file(if cfg!(windows) { "C:\\bar" } else { "/bar" })
}

struct Fixture {
    test_dir: TestDir,
    backup_home: String,
    existing_test_folder1: URI,
    service: Rc<BackupMainService>,
    config_service: Rc<TestConfigurationService>,
    state_main_service: Rc<InMemoryTestStateMainService>,
}

impl Fixture {
    /// O `setup` do original.
    async fn new() -> Rc<Self> {
        let test_dir = TestDir::new(&["vsctests", "backupmainservice"]);
        let backup_home = join(&[&test_dir.path().to_string_lossy(), "Backups"]);
        let existing_test_folder1 = uri_file(&join(&[&test_dir.path().to_string_lossy(), "folder1"]));

        std::fs::create_dir_all(&backup_home).unwrap();

        let config_service = TestConfigurationService::new(None);
        let state_main_service = Rc::new(InMemoryTestStateMainService::default());
        let log_service: LogServiceRef = DefaultLogService::new(ConsoleMainLogger::new(DEFAULT_LOG_LEVEL), Vec::new());

        let service = BackupMainService::new(backup_home.clone(), config_service.clone(), log_service, state_main_service.clone());
        service.initialize().await.unwrap();

        Rc::new(Self { test_dir, backup_home, existing_test_folder1, service, config_service, state_main_service })
    }

    /// `toBackupPath(uri)`.
    fn to_backup_path(&self, uri: &URI) -> String {
        join(&[&self.backup_home, &BackupMainService::get_folder_hash(&to_folder_backup_info(uri, None))])
    }

    /// `toBackupPath(id)`.
    fn to_backup_path_of_id(&self, id: &str) -> String {
        join(&[&self.backup_home, id])
    }

    async fn ensure_folder_exists(&self, uri: &URI) {
        if !exists(&fs_path(uri)) {
            mkdir(&fs_path(uri));
        }

        let backup_folder = self.to_backup_path(uri);
        create_backup_folder(&backup_folder);
    }

    async fn ensure_workspace_exists(&self, workspace: WorkspaceIdentifier) -> WorkspaceIdentifier {
        if !exists(&fs_path(&workspace.config_path)) {
            std::fs::write(fs_path(&workspace.config_path), "Hello").unwrap();
        }

        let backup_folder = self.to_backup_path_of_id(&workspace.id);
        create_backup_folder(&backup_folder);

        workspace
    }

    fn read_workspaces_metadata(&self) -> Dynamic {
        self.state_main_service.get_item("backupWorkspaces", Dynamic::Undefined)
    }

    fn write_workspaces_metadata(&self, data: &str) {
        if data.is_empty() {
            self.state_main_service.remove_item("backupWorkspaces");
        } else {
            let mut errors = Vec::new();
            let parsed = json::parse_str(data, &mut errors).expect("JSON válido");
            self.state_main_service.set_item("backupWorkspaces", from_json(&parsed));
        }
    }

    fn write_workspaces_metadata_value(&self, value: &Dynamic) {
        self.write_workspaces_metadata(&json_stringify(value).expect("JSON.stringify"));
    }

    fn set_hot_exit(&self, value: &str) {
        drop(self.config_service.set_user_configuration("files.hotExit", Dynamic::string(value), None));
    }
}

fn create_backup_folder(backup_folder: &str) {
    if !exists(backup_folder) {
        mkdir(backup_folder);
        mkdir(&join(&[backup_folder, schemas::FILE]));
        std::fs::write(join(&[backup_folder, schemas::FILE, "foo.txt"]), "Hello").unwrap();
    }
}

fn run<F>(body: impl FnOnce(Rc<Fixture>) -> F + 'static)
where
    F: Future<Output = ()> + 'static,
{
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    run_on_real_loop(async move {
        let fixture = Fixture::new().await;
        body(fixture).await;
    });
}

#[test]
fn service_validates_backup_workspaces_on_startup_and_cleans_up_folder_workspaces() {
    run(|f| async move {
        let (foo, bar) = (foo_file(), bar_file());

        // 1) backup workspace path does not exist
        f.service.register_folder_backup(to_folder_backup_info(&foo, None));
        f.service.register_folder_backup(to_folder_backup_info(&bar, None));
        f.service.initialize().await.unwrap();
        assert_equal_folder_infos(&f.service.get_folder_backups(), &[]);

        // 2) backup workspace path exists with empty contents within
        mkdir(&f.to_backup_path(&foo));
        mkdir(&f.to_backup_path(&bar));
        f.service.register_folder_backup(to_folder_backup_info(&foo, None));
        f.service.register_folder_backup(to_folder_backup_info(&bar, None));
        f.service.initialize().await.unwrap();
        assert_equal_folder_infos(&f.service.get_folder_backups(), &[]);
        assert!(!exists(&f.to_backup_path(&foo)));
        assert!(!exists(&f.to_backup_path(&bar)));

        // 3) backup workspace path exists with empty folders within
        mkdir(&f.to_backup_path(&foo));
        mkdir(&f.to_backup_path(&bar));
        mkdir(&join(&[&f.to_backup_path(&foo), schemas::FILE]));
        mkdir(&join(&[&f.to_backup_path(&bar), schemas::UNTITLED]));
        f.service.register_folder_backup(to_folder_backup_info(&foo, None));
        f.service.register_folder_backup(to_folder_backup_info(&bar, None));
        f.service.initialize().await.unwrap();
        assert_equal_folder_infos(&f.service.get_folder_backups(), &[]);
        assert!(!exists(&f.to_backup_path(&foo)));
        assert!(!exists(&f.to_backup_path(&bar)));

        // 4) backup workspace path points to a workspace that no longer exists
        // so it should convert the backup worspace to an empty workspace backup
        let file_backups = join(&[&f.to_backup_path(&foo), schemas::FILE]);
        mkdir(&f.to_backup_path(&foo));
        mkdir(&f.to_backup_path(&bar));
        mkdir(&file_backups);
        f.service.register_folder_backup(to_folder_backup_info(&foo, None));
        assert_eq!(f.service.get_folder_backups().len(), 1);
        assert_eq!(f.service.get_empty_window_backups().len(), 0);
        std::fs::write(join(&[&file_backups, "backup.txt"]), "").unwrap();
        f.service.initialize().await.unwrap();
        assert_eq!(f.service.get_folder_backups().len(), 0);
        assert_eq!(f.service.get_empty_window_backups().len(), 1);
    });
}

#[test]
fn service_validates_backup_workspaces_on_startup_and_cleans_up_root_workspaces() {
    run(|f| async move {
        let (foo, bar) = (foo_file(), bar_file());
        let (foo_path, bar_path) = (fs_path(&foo), fs_path(&bar));

        // 1) backup workspace path does not exist
        f.service.register_workspace_backup(to_workspace_backup_info(&foo_path, None));
        f.service.register_workspace_backup(to_workspace_backup_info(&bar_path, None));
        f.service.initialize().await.unwrap();
        assert!(f.service.get_workspace_backups().is_empty());

        // 2) backup workspace path exists with empty contents within
        mkdir(&f.to_backup_path(&foo));
        mkdir(&f.to_backup_path(&bar));
        f.service.register_workspace_backup(to_workspace_backup_info(&foo_path, None));
        f.service.register_workspace_backup(to_workspace_backup_info(&bar_path, None));
        f.service.initialize().await.unwrap();
        assert!(f.service.get_workspace_backups().is_empty());
        assert!(!exists(&f.to_backup_path(&foo)));
        assert!(!exists(&f.to_backup_path(&bar)));

        // 3) backup workspace path exists with empty folders within
        mkdir(&f.to_backup_path(&foo));
        mkdir(&f.to_backup_path(&bar));
        mkdir(&join(&[&f.to_backup_path(&foo), schemas::FILE]));
        mkdir(&join(&[&f.to_backup_path(&bar), schemas::UNTITLED]));
        f.service.register_workspace_backup(to_workspace_backup_info(&foo_path, None));
        f.service.register_workspace_backup(to_workspace_backup_info(&bar_path, None));
        f.service.initialize().await.unwrap();
        assert!(f.service.get_workspace_backups().is_empty());
        assert!(!exists(&f.to_backup_path(&foo)));
        assert!(!exists(&f.to_backup_path(&bar)));

        // 4) backup workspace path points to a workspace that no longer exists
        // so it should convert the backup worspace to an empty workspace backup
        let file_backups = join(&[&f.to_backup_path(&foo), schemas::FILE]);
        mkdir(&f.to_backup_path(&foo));
        mkdir(&f.to_backup_path(&bar));
        mkdir(&file_backups);
        f.service.register_workspace_backup(to_workspace_backup_info(&foo_path, None));
        assert_eq!(f.service.get_workspace_backups().len(), 1);
        assert_eq!(f.service.get_empty_window_backups().len(), 0);
        std::fs::write(join(&[&file_backups, "backup.txt"]), "").unwrap();
        f.service.initialize().await.unwrap();
        assert_eq!(f.service.get_workspace_backups().len(), 0);
        assert_eq!(f.service.get_empty_window_backups().len(), 1);
    });
}

#[test]
fn service_supports_to_migrate_backup_data_from_another_location() {
    run(|f| async move {
        let (foo, bar) = (foo_file(), bar_file());
        let backup_path_to_migrate = f.to_backup_path(&foo);
        mkdir(&backup_path_to_migrate);
        std::fs::write(join(&[&backup_path_to_migrate, "backup.txt"]), "Some Data").unwrap();
        f.service.register_folder_backup(to_folder_backup_info(&uri_file(&backup_path_to_migrate), None));

        let workspace_backup_path =
            f.service.register_workspace_backup_migrating(to_workspace_backup_info(&fs_path(&bar), None), &backup_path_to_migrate).await.unwrap();

        assert!(exists(&workspace_backup_path));
        assert!(exists(&join(&[&workspace_backup_path, "backup.txt"])));
        assert!(!exists(&backup_path_to_migrate));

        let empty_backups = f.service.get_empty_window_backups();
        assert_eq!(0, empty_backups.len());
    });
}

#[test]
fn service_backup_migration_makes_sure_to_preserve_existing_backups() {
    run(|f| async move {
        let (foo, bar) = (foo_file(), bar_file());
        let backup_path_to_migrate = f.to_backup_path(&foo);
        mkdir(&backup_path_to_migrate);
        std::fs::write(join(&[&backup_path_to_migrate, "backup.txt"]), "Some Data").unwrap();
        f.service.register_folder_backup(to_folder_backup_info(&uri_file(&backup_path_to_migrate), None));

        let backup_path_to_preserve = f.to_backup_path(&bar);
        mkdir(&backup_path_to_preserve);
        std::fs::write(join(&[&backup_path_to_preserve, "backup.txt"]), "Some Data").unwrap();
        f.service.register_folder_backup(to_folder_backup_info(&uri_file(&backup_path_to_preserve), None));

        let workspace_backup_path =
            f.service.register_workspace_backup_migrating(to_workspace_backup_info(&fs_path(&bar), None), &backup_path_to_migrate).await.unwrap();

        assert!(exists(&workspace_backup_path));
        assert!(exists(&join(&[&workspace_backup_path, "backup.txt"])));
        assert!(!exists(&backup_path_to_migrate));

        let empty_backups = f.service.get_empty_window_backups();
        assert_eq!(1, empty_backups.len());
        assert_eq!(1, std::fs::read_dir(join(&[&f.backup_home, &empty_backups[0].backup_folder])).unwrap().count());
    });
}

// suite('loadSync')

#[test]
fn get_folder_backup_paths_should_return_empty_when_workspaces_json_does_not_exist() {
    run(|f| async move {
        assert_equal_folder_infos(&f.service.get_folder_backups(), &[]);
    });
}

#[test]
fn get_folder_backup_paths_should_return_empty_when_folders_in_workspaces_json_is_absent() {
    run(|f| async move {
        f.write_workspaces_metadata("{}");
        f.service.initialize().await.unwrap();
        assert_equal_folder_infos(&f.service.get_folder_backups(), &[]);
    });
}

#[test]
fn get_folder_backup_paths_should_return_empty_when_folders_in_workspaces_json_is_not_a_string_array() {
    run(|f| async move {
        for metadata in [
            r#"{"folders":{}}"#,
            r#"{"folders":{"foo": ["bar"]}}"#,
            r#"{"folders":{"foo": []}}"#,
            r#"{"folders":{"foo": "bar"}}"#,
            r#"{"folders":"foo"}"#,
            r#"{"folders":1}"#,
        ] {
            f.write_workspaces_metadata(metadata);
            f.service.initialize().await.unwrap();
            assert_equal_folder_infos(&f.service.get_folder_backups(), &[]);
        }
    });
}

#[test]
fn get_folder_backup_paths_should_return_empty_when_files_hot_exit_is_on_exit_and_window_close() {
    run(|f| async move {
        let fi = to_folder_backup_info(&uri_file(&fs_path(&foo_file()).to_uppercase()), None);
        f.service.register_folder_backup(fi.clone());
        assert_equal_folder_infos(&f.service.get_folder_backups(), &[fi]);
        f.set_hot_exit(HotExitConfiguration::ON_EXIT_AND_WINDOW_CLOSE);
        f.service.initialize().await.unwrap();
        assert_equal_folder_infos(&f.service.get_folder_backups(), &[]);
    });
}

#[test]
fn get_workspace_backups_should_return_empty_when_workspaces_json_does_not_exist() {
    run(|f| async move {
        assert!(f.service.get_workspace_backups().is_empty());
    });
}

#[test]
fn get_workspace_backups_should_return_empty_when_folder_workspaces_in_workspaces_json_is_absent() {
    run(|f| async move {
        f.write_workspaces_metadata("{}");
        f.service.initialize().await.unwrap();
        assert!(f.service.get_workspace_backups().is_empty());
    });
}

#[test]
fn get_workspace_backups_should_return_empty_when_root_workspaces_in_workspaces_json_is_not_an_object_array() {
    run(|f| async move {
        for metadata in [
            r#"{"rootWorkspaces":{}}"#,
            r#"{"rootWorkspaces":{"foo": ["bar"]}}"#,
            r#"{"rootWorkspaces":{"foo": []}}"#,
            r#"{"rootWorkspaces":{"foo": "bar"}}"#,
            r#"{"rootWorkspaces":"foo"}"#,
            r#"{"rootWorkspaces":1}"#,
        ] {
            f.write_workspaces_metadata(metadata);
            f.service.initialize().await.unwrap();
            assert!(f.service.get_workspace_backups().is_empty());
        }
    });
}

#[test]
fn get_workspace_backups_should_return_empty_when_workspaces_in_workspaces_json_is_not_an_object_array() {
    run(|f| async move {
        for metadata in [
            r#"{"workspaces":{}}"#,
            r#"{"workspaces":{"foo": ["bar"]}}"#,
            r#"{"workspaces":{"foo": []}}"#,
            r#"{"workspaces":{"foo": "bar"}}"#,
            r#"{"workspaces":"foo"}"#,
            r#"{"workspaces":1}"#,
        ] {
            f.write_workspaces_metadata(metadata);
            f.service.initialize().await.unwrap();
            assert!(f.service.get_workspace_backups().is_empty());
        }
    });
}

#[test]
fn get_workspace_backups_should_return_empty_when_files_hot_exit_is_on_exit_and_window_close() {
    run(|f| async move {
        let upper_foo_path = fs_path(&foo_file()).to_uppercase();
        f.service.register_workspace_backup(to_workspace_backup_info(&upper_foo_path, None));
        assert_eq!(f.service.get_workspace_backups().len(), 1);
        assert_eq!(
            f.service.get_workspace_backups().iter().map(|r| uri_text(&r.workspace.config_path)).collect::<Vec<_>>(),
            vec![uri_text(&uri_file(&upper_foo_path))]
        );
        f.set_hot_exit(HotExitConfiguration::ON_EXIT_AND_WINDOW_CLOSE);
        f.service.initialize().await.unwrap();
        assert!(f.service.get_workspace_backups().is_empty());
    });
}

#[test]
fn get_empty_workspace_backup_paths_should_return_empty_when_workspaces_json_does_not_exist() {
    run(|f| async move {
        assert!(f.service.get_empty_window_backups().is_empty());
    });
}

#[test]
fn get_empty_workspace_backup_paths_should_return_empty_when_folder_workspaces_in_workspaces_json_is_absent() {
    run(|f| async move {
        f.write_workspaces_metadata("{}");
        f.service.initialize().await.unwrap();
        assert!(f.service.get_empty_window_backups().is_empty());
    });
}

#[test]
fn get_empty_workspace_backup_paths_should_return_empty_when_folder_workspaces_in_workspaces_json_is_not_a_string_array() {
    run(|f| async move {
        for metadata in [
            r#"{"emptyWorkspaces":{}}"#,
            r#"{"emptyWorkspaces":{"foo": ["bar"]}}"#,
            r#"{"emptyWorkspaces":{"foo": []}}"#,
            r#"{"emptyWorkspaces":{"foo": "bar"}}"#,
            r#"{"emptyWorkspaces":"foo"}"#,
            r#"{"emptyWorkspaces":1}"#,
        ] {
            f.write_workspaces_metadata(metadata);
            f.service.initialize().await.unwrap();
            assert!(f.service.get_empty_window_backups().is_empty());
        }
    });
}

// suite('dedupeFolderWorkspaces')

#[test]
fn should_ignore_duplicates_folder_workspace() {
    run(|f| async move {
        f.ensure_folder_exists(&f.existing_test_folder1).await;

        let existing = uri_text(&f.existing_test_folder1);
        f.write_workspaces_metadata_value(&serialized_backup_workspaces(Vec::new(), vec![serialized_folder(&existing), serialized_folder(&existing)]));
        f.service.initialize().await.unwrap();

        let json = f.read_workspaces_metadata();
        assert_deep(&json.get_str("folders"), &Dynamic::array([serialized_folder(&existing)]));
    });
}

#[test]
fn should_ignore_duplicates_on_windows_and_mac_folder_workspace() {
    run(|f| async move {
        f.ensure_folder_exists(&f.existing_test_folder1).await;

        let existing = uri_text(&f.existing_test_folder1);
        f.write_workspaces_metadata_value(&serialized_backup_workspaces(Vec::new(), vec![serialized_folder(&existing), serialized_folder(&existing.to_lowercase())]));
        f.service.initialize().await.unwrap();
        let json = f.read_workspaces_metadata();
        assert_deep(&json.get_str("folders"), &Dynamic::array([serialized_folder(&existing)]));
    });
}

#[test]
fn should_ignore_duplicates_on_windows_and_mac_root_workspace() {
    run(|f| async move {
        let test_dir_path = f.test_dir.path().to_string_lossy().into_owned();
        let workspace_path = join(&[&test_dir_path, "Foo.code-workspace"]);
        let workspace_path1 = join(&[&test_dir_path, "FOO.code-workspace"]);
        let workspace_path2 = join(&[&test_dir_path, "foo.code-workspace"]);

        let workspace1 = f.ensure_workspace_exists(to_workspace(&workspace_path)).await;
        let workspace2 = f.ensure_workspace_exists(to_workspace(&workspace_path1)).await;
        let workspace3 = f.ensure_workspace_exists(to_workspace(&workspace_path2)).await;

        f.write_workspaces_metadata_value(&serialized_backup_workspaces(
            [&workspace1, &workspace2, &workspace3].into_iter().map(to_serialized_workspace).collect(),
            Vec::new(),
        ));
        f.service.initialize().await.unwrap();

        let json = f.read_workspaces_metadata();
        let workspaces = items_of(&json.get_str("workspaces"));
        assert_eq!(workspaces.len(), if cfg!(target_os = "linux") { 3 } else { 1 });
        let config_uri_paths: Vec<String> = workspaces.iter().map(|r| text_of(&r.get_str("configURIPath"))).collect();
        if cfg!(target_os = "linux") {
            assert_eq!(
                config_uri_paths,
                vec![uri_text(&uri_file(&workspace_path)), uri_text(&uri_file(&workspace_path1)), uri_text(&uri_file(&workspace_path2))]
            );
        } else {
            assert_eq!(config_uri_paths, vec![uri_text(&uri_file(&workspace_path))], "should return the first duplicated entry");
        }
    });
}

// suite('registerWindowForBackups')

#[test]
fn should_persist_paths_to_workspaces_json_folder_workspace() {
    run(|f| async move {
        let (foo, bar) = (foo_file(), bar_file());
        f.service.register_folder_backup(to_folder_backup_info(&foo, None));
        f.service.register_folder_backup(to_folder_backup_info(&bar, None));
        assert_equal_folder_infos(&f.service.get_folder_backups(), &[to_folder_backup_info(&foo, None), to_folder_backup_info(&bar, None)]);

        let json = f.read_workspaces_metadata();
        assert_deep(&json.get_str("folders"), &Dynamic::array([serialized_folder(&uri_text(&foo)), serialized_folder(&uri_text(&bar))]));
    });
}

#[test]
fn should_persist_paths_to_workspaces_json_root_workspace() {
    run(|f| async move {
        let (foo, bar) = (foo_file(), bar_file());
        let ws1 = to_workspace_backup_info(&fs_path(&foo), None);
        f.service.register_workspace_backup(ws1.clone());
        let ws2 = to_workspace_backup_info(&fs_path(&bar), None);
        f.service.register_workspace_backup(ws2.clone());

        let backups = f.service.get_workspace_backups();
        assert_eq!(backups.iter().map(|b| uri_text(&b.workspace.config_path)).collect::<Vec<_>>(), vec![uri_text(&foo), uri_text(&bar)]);
        assert_eq!(ws1.workspace.id, backups[0].workspace.id);
        assert_eq!(ws2.workspace.id, backups[1].workspace.id);

        let json = f.read_workspaces_metadata();
        let workspaces = items_of(&json.get_str("workspaces"));
        assert_eq!(workspaces.iter().map(|b| text_of(&b.get_str("configURIPath"))).collect::<Vec<_>>(), vec![uri_text(&foo), uri_text(&bar)]);
        assert_eq!(ws1.workspace.id, text_of(&workspaces[0].get_str("id")));
        assert_eq!(ws2.workspace.id, text_of(&workspaces[1].get_str("id")));
    });
}

#[test]
fn should_always_store_the_workspace_path_in_workspaces_json_using_the_case_given_regardless_of_whether_the_file_system_is_case_sensitive_folder_workspace() {
    run(|f| async move {
        let upper_foo = uri_file(&fs_path(&foo_file()).to_uppercase());
        f.service.register_folder_backup(to_folder_backup_info(&upper_foo, None));
        assert_equal_folder_infos(&f.service.get_folder_backups(), &[to_folder_backup_info(&upper_foo, None)]);

        let json = f.read_workspaces_metadata();
        assert_deep(&json.get_str("folders"), &Dynamic::array([serialized_folder(&uri_text(&upper_foo))]));
    });
}

#[test]
fn should_always_store_the_workspace_path_in_workspaces_json_using_the_case_given_regardless_of_whether_the_file_system_is_case_sensitive_root_workspace() {
    run(|f| async move {
        let upper_foo_path = fs_path(&foo_file()).to_uppercase();
        f.service.register_workspace_backup(to_workspace_backup_info(&upper_foo_path, None));
        assert_eq!(
            f.service.get_workspace_backups().iter().map(|b| uri_text(&b.workspace.config_path)).collect::<Vec<_>>(),
            vec![uri_text(&uri_file(&upper_foo_path))]
        );

        let json = f.read_workspaces_metadata();
        assert_eq!(
            items_of(&json.get_str("workspaces")).iter().map(|b| text_of(&b.get_str("configURIPath"))).collect::<Vec<_>>(),
            vec![uri_text(&uri_file(&upper_foo_path))]
        );
    });
}

// suite('getWorkspaceHash')

#[test]
#[cfg_attr(target_os = "linux", ignore = "test.skip no Linux, como no original")]
fn should_ignore_case_on_windows_and_mac() {
    run(|_f| async move {
        let assert_folder_hash = |uri1: URI, uri2: URI| {
            assert_eq!(BackupMainService::get_folder_hash(&to_folder_backup_info(&uri1, None)), BackupMainService::get_folder_hash(&to_folder_backup_info(&uri2, None)));
        };

        if cfg!(target_os = "macos") {
            assert_folder_hash(uri_file("/foo"), uri_file("/FOO"));
        }

        if cfg!(windows) {
            assert_folder_hash(uri_file("c:\\foo"), uri_file("C:\\FOO"));
        }
    });
}

// suite('mixed path casing')

#[test]
fn should_handle_case_insensitive_paths_properly_register_window_for_backups_sync_folder_workspace() {
    run(|f| async move {
        f.service.register_folder_backup(to_folder_backup_info(&foo_file(), None));
        f.service.register_folder_backup(to_folder_backup_info(&uri_file(&fs_path(&foo_file()).to_uppercase()), None));

        if cfg!(target_os = "linux") {
            assert_eq!(f.service.get_folder_backups().len(), 2);
        } else {
            assert_eq!(f.service.get_folder_backups().len(), 1);
        }
    });
}

#[test]
fn should_handle_case_insensitive_paths_properly_register_window_for_backups_sync_root_workspace() {
    run(|f| async move {
        let foo_path = fs_path(&foo_file());
        f.service.register_workspace_backup(to_workspace_backup_info(&foo_path, None));
        f.service.register_workspace_backup(to_workspace_backup_info(&foo_path.to_uppercase(), None));

        if cfg!(target_os = "linux") {
            assert_eq!(f.service.get_workspace_backups().len(), 2);
        } else {
            assert_eq!(f.service.get_workspace_backups().len(), 1);
        }
    });
}

// suite('getDirtyWorkspaces')

#[test]
fn should_report_if_a_workspace_or_folder_has_backups() {
    run(|f| async move {
        let foo = foo_file();
        let folder_backup_path = f.service.register_folder_backup(to_folder_backup_info(&foo, None));

        let backup_workspace_info = to_workspace_backup_info(&fs_path(&foo), None);
        let workspace_backup_path = f.service.register_workspace_backup(backup_workspace_info.clone());

        assert_eq!(f.service.get_dirty_workspaces().await.unwrap().len(), 0);

        // ignore - folder might exist already
        let _ = std::fs::create_dir_all(join(&[&folder_backup_path, schemas::FILE]));
        let _ = std::fs::create_dir_all(join(&[&workspace_backup_path, schemas::UNTITLED]));

        assert_eq!(f.service.get_dirty_workspaces().await.unwrap().len(), 0);

        std::fs::write(join(&[&folder_backup_path, schemas::FILE, "594a4a9d82a277a899d4713a5b08f504"]), "").unwrap();
        std::fs::write(join(&[&workspace_backup_path, schemas::UNTITLED, "594a4a9d82a277a899d4713a5b08f504"]), "").unwrap();

        let dirty_workspaces = f.service.get_dirty_workspaces().await.unwrap();
        assert_eq!(dirty_workspaces.len(), 2);

        let mut found = 0;
        for dirty_workpspace in &dirty_workspaces {
            match dirty_workpspace {
                BackupInfo::Folder(folder) => {
                    if ext_uri().is_equal(Some(&foo), Some(&folder.folder_uri), false) {
                        found += 1;
                    }
                }
                BackupInfo::Workspace(workspace) => {
                    if ext_uri().is_equal(Some(&backup_workspace_info.workspace.config_path), Some(&workspace.workspace.config_path), false) {
                        found += 1;
                    }
                }
            }
        }

        assert_eq!(found, 2);
    });
}
