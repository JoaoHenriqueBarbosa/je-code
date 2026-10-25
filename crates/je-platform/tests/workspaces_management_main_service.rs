//! Porte de `src/vs/platform/workspaces/test/electron-main/workspacesManagementMainService.test.ts`.
//!
//! Um `#[test]` por caso do original, na mesma ordem.
//!
//! Notas de tradução:
//! - O teste usa o sistema de arquivos real numa pasta temporária (`getRandomTestPath`): cada caso roda no laço de
//!   eventos real ([`run_on_real_loop`]) com um [`TestDir`], apagado no fim (o `teardown` do original).
//! - O `TestEnvironmentService` (que sobrescreve `untitledWorkspacesHome`) não existe: o `untitledWorkspacesHome` e o
//!   `workspaceStorageHome` entram no construtor do serviço, e o `UserDataProfilesEnvironment` aponta para a mesma pasta.
//! - O `StateService` e o `UriIdentityService` do original são o [`InMemoryProfilesStore`] e o `ExtUri` enviesado pela caixa
//!   do caminho; o `FileService` é o real, sem provider registrado, como no original.
//! - `TestDialogMainService` e `TestBackupMainService` são as classes de apoio do original: todo método lança, menos
//!   `getDirtyWorkspaces`.
//! - O `ensureNoDisposablesAreLeakedInTestSuite()` fica no fim da suíte no original, então o hook dele roda depois do
//!   `setup`: o [`LeakGuard`] é criado depois de montar o serviço.
//! - `JSON.parse` e `JSON.stringify` são o parser e o formatador do `je_base`.
//! - `(!isWindows ? test.skip : test)` é `#[ignore]` fora do Windows, e `isWindows` do `assertPathEquals` usa `cfg!`.

mod common;

use std::future::Future;
use std::rc::Rc;

use common::disk::{TestDir, run_on_real_loop};
use common::{ensure_no_disposables_are_leaked_in_test_suite, join};
use je_base::JsString;
use je_base::extpath;
use je_base::json::{self, Object, Value};
use je_base::json_formatter;
use je_base::labels::normalize_drive_letter;
use je_base::lifecycle::IDisposable;
use je_base::path::{PathModule, ProcessEnvironment};
use je_base::promise::Promise;
use je_base::uri::URI;
use je_platform::backup::backup_main_service::IBackupMainService;
use je_platform::backup::{BackupInfo, EmptyWindowBackupInfo, FolderBackupInfo, WorkspaceBackupInfo};
use je_platform::dialogs::dialog_main_service::{
    BrowserWindow, IDialogMainService, MessageBoxOptions, MessageBoxReturnValue, OpenDialogOptions, OpenDialogReturnValue, SaveDialogOptions, SaveDialogReturnValue,
};
use je_platform::dialogs::NativeOpenDialogOptions;
use je_platform::files::file_service::FileService;
use je_platform::log::NullLogService;
use je_platform::resources::{ext_uri_biased_ignore_path_case, host_environment, uri_file};
use je_platform::user_data_profile::{InMemoryProfilesStore, UserDataProfilesEnvironment, UserDataProfilesService};
use je_platform::workspace::{WORKSPACE_EXTENSION, WorkspaceIdentifier};
use je_platform::workspaces::workspaces_management_main_service::{IWorkspacesManagementMainService, WorkspacesManagementMainService};
use je_platform::workspaces::{WorkspaceFolderCreationData, rewrite_workspace_file_for_new_location};

struct TestDialogMainService;

impl IDialogMainService for TestDialogMainService {
    fn pick_file_folder(&self, _options: NativeOpenDialogOptions, _window: Option<BrowserWindow>) -> Promise<Option<Vec<String>>> {
        panic!("Method not implemented.")
    }

    fn pick_folder(&self, _options: NativeOpenDialogOptions, _window: Option<BrowserWindow>) -> Promise<Option<Vec<String>>> {
        panic!("Method not implemented.")
    }

    fn pick_file(&self, _options: NativeOpenDialogOptions, _window: Option<BrowserWindow>) -> Promise<Option<Vec<String>>> {
        panic!("Method not implemented.")
    }

    fn pick_workspace(&self, _options: NativeOpenDialogOptions, _window: Option<BrowserWindow>) -> Promise<Option<Vec<String>>> {
        panic!("Method not implemented.")
    }

    fn show_message_box(&self, _options: MessageBoxOptions, _window: Option<BrowserWindow>) -> Promise<MessageBoxReturnValue> {
        panic!("Method not implemented.")
    }

    fn show_save_dialog(&self, _options: SaveDialogOptions, _window: Option<BrowserWindow>) -> Promise<SaveDialogReturnValue> {
        panic!("Method not implemented.")
    }

    fn show_open_dialog(&self, _options: OpenDialogOptions, _window: Option<BrowserWindow>) -> Promise<OpenDialogReturnValue> {
        panic!("Method not implemented.")
    }
}

struct TestBackupMainService;

impl IBackupMainService for TestBackupMainService {
    fn is_hot_exit_enabled(&self) -> bool {
        panic!("Method not implemented.")
    }

    fn get_empty_window_backups(&self) -> Vec<EmptyWindowBackupInfo> {
        panic!("Method not implemented.")
    }

    fn register_workspace_backup(&self, _workspace_info: WorkspaceBackupInfo) -> String {
        panic!("Method not implemented.")
    }

    fn register_workspace_backup_migrating(&self, _workspace_info: WorkspaceBackupInfo, _migrate_from: &str) -> Promise<String> {
        panic!("Method not implemented.")
    }

    fn register_folder_backup(&self, _folder_info: FolderBackupInfo) -> String {
        panic!("Method not implemented.")
    }

    fn register_empty_window_backup(&self, _empty_window_info: EmptyWindowBackupInfo) -> String {
        panic!("Method not implemented.")
    }

    fn get_dirty_workspaces(&self) -> Promise<Vec<BackupInfo>> {
        Promise::resolved(Vec::new())
    }
}

struct Fixture {
    _test_dir: TestDir,
    service: Rc<WorkspacesManagementMainService>,
}

impl Fixture {
    /// O `setup` do original.
    fn new() -> Rc<Self> {
        let test_dir = TestDir::new(&["vsctests", "workspacesmanagementmainservice"]);
        let root = test_dir.path().to_string_lossy().into_owned();
        let untitled_workspaces_home_path = join(&[&root, "Workspaces"]);

        let log_service = NullLogService::shared();
        let file_service = FileService::new(log_service.clone());
        let user_data_profiles_service = UserDataProfilesService::new(
            UserDataProfilesEnvironment {
                user_roaming_data_home: uri_file(&join(&[&root, "User"])),
                cache_home: uri_file(&join(&[&root, "CachedData"])),
                agent_sessions_workspace: None,
            },
            file_service,
            ext_uri_biased_ignore_path_case(),
            log_service.clone(),
            Rc::new(InMemoryProfilesStore::default()),
        );
        let service = WorkspacesManagementMainService::new(
            uri_file(&untitled_workspaces_home_path),
            uri_file(&join(&[&root, "workspaceStorage"])),
            log_service,
            user_data_profiles_service,
            Rc::new(TestBackupMainService),
            Rc::new(TestDialogMainService),
        );

        std::fs::create_dir_all(&untitled_workspaces_home_path).unwrap();

        Rc::new(Self { _test_dir: test_dir, service })
    }
}

fn run<F>(body: impl FnOnce(Rc<Fixture>) -> F + 'static)
where
    F: Future<Output = ()> + 'static,
{
    run_on_real_loop(async move {
        let fixture = Fixture::new();
        let guard = ensure_no_disposables_are_leaked_in_test_suite();
        body(fixture.clone()).await;
        fixture.service.dispose(); // teardown
        drop(guard);
    });
}

fn cwd() -> String {
    std::env::current_dir().unwrap().to_string_lossy().into_owned()
}

fn tmp_dir() -> String {
    std::env::temp_dir().to_string_lossy().into_owned()
}

fn path_module() -> PathModule<'static> {
    PathModule::native(&ProcessEnvironment)
}

fn dirname(path: &str) -> String {
    path_module().dirname(path)
}

fn uri_text(uri: &URI) -> String {
    uri.to_string(false).to_string_lossy()
}

fn uri_text_unencoded(uri: &URI) -> String {
    uri.to_string(true).to_string_lossy()
}

async fn create_untitled_workspace(service: &Rc<WorkspacesManagementMainService>, folders: &[&str], names: Option<&[&str]>) -> WorkspaceIdentifier {
    let data = folders
        .iter()
        .enumerate()
        .map(|(index, folder)| WorkspaceFolderCreationData { uri: uri_file(folder), name: names.map(|names| names[index].to_string()) })
        .collect();
    service.create_untitled_workspace(data, None).await.unwrap()
}

fn js(text: &str) -> JsString {
    JsString::from(text)
}

fn string_value(text: &str) -> Value {
    Value::String(js(text))
}

/// `fs.writeFileSync(path, JSON.stringify(value))`.
fn write_json(path: &str, value: &Value) {
    std::fs::write(path, json_formatter::json_stringify(value, None)).unwrap();
}

fn object_of(entries: Vec<(&str, Value)>) -> Value {
    let mut object = Object::new();
    for (key, value) in entries {
        object.insert(js(key), value);
    }
    Value::Object(object)
}

fn create_workspace(workspace_config_path: &str, folders: &[&str]) {
    let folders = folders.iter().map(|folder| object_of(vec![("path", string_value(folder))])).collect();
    write_json(workspace_config_path, &object_of(vec![("folders", Value::Array(folders))]));
}

/// `JSON.parse(text) as IStoredWorkspace`.
fn parse_stored(text: &str) -> Object {
    let mut errors = Vec::new();
    match json::parse_str(text, &mut errors) {
        Some(Value::Object(object)) => object,
        other => panic!("esperava um objeto JSON: {other:?}"),
    }
}

fn read_stored(path: &str) -> Object {
    parse_stored(&std::fs::read_to_string(path).unwrap())
}

fn stored_folders(workspace: &Object) -> Vec<Object> {
    match workspace.get("folders") {
        Some(Value::Array(folders)) => folders
            .iter()
            .map(|folder| match folder {
                Value::Object(folder) => folder.clone(),
                other => panic!("esperava um objeto: {other:?}"),
            })
            .collect(),
        other => panic!("esperava um array: {other:?}"),
    }
}

fn field(folder: &Object, key: &str) -> Option<String> {
    match folder.get(key) {
        Some(Value::String(text)) => Some(text.to_string_lossy()),
        _ => None,
    }
}

/// `(<IRawFileWorkspaceFolder>folder).path`.
fn path_of(folder: &Object) -> String {
    field(folder, "path").expect("esperava o campo path")
}

fn assert_path_equals(path_in_workspace_file: &str, path_on_disk: &str) {
    let (mut path_in_workspace_file, mut path_on_disk) = (path_in_workspace_file.to_string(), path_on_disk.to_string());
    if cfg!(windows) {
        path_in_workspace_file = normalize_drive_letter(&path_in_workspace_file, true);
        path_on_disk = normalize_drive_letter(&path_on_disk, true);
        if !extpath::is_unc(&path_on_disk, host_environment().as_ref()) {
            path_on_disk = extpath::to_slashes(&path_on_disk); // workspace file is using slashes for all paths except where mandatory
        }
    }

    assert_eq!(path_in_workspace_file, path_on_disk);
}

fn assert_equal_uri(u1: &URI, u2: &URI) {
    assert_eq!(uri_text(u1), uri_text(u2));
}

fn workspace_file_name() -> String {
    format!("workspace.{WORKSPACE_EXTENSION}")
}

fn date_now() -> u128 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_millis()
}

#[test]
fn create_workspace_folders() {
    run(|f| async move {
        let (cwd, tmp_dir) = (cwd(), tmp_dir());
        let workspace = create_untitled_workspace(&f.service, &[&cwd, &tmp_dir], None).await;
        assert!(std::path::Path::new(&je_platform::resources::fs_path(&workspace.config_path)).exists());
        assert!(f.service.is_untitled_workspace(&workspace));

        let ws = read_stored(&je_platform::resources::fs_path(&workspace.config_path));
        let folders = stored_folders(&ws);
        assert_eq!(folders.len(), 2);
        assert_path_equals(&path_of(&folders[0]), &cwd);
        assert_path_equals(&path_of(&folders[1]), &tmp_dir);
        assert!(field(&folders[0], "name").is_none_or(|name| name.is_empty()));
        assert!(field(&folders[1], "name").is_none_or(|name| name.is_empty()));
    });
}

#[test]
fn create_workspace_folders_with_name() {
    run(|f| async move {
        let (cwd, tmp_dir) = (cwd(), tmp_dir());
        let workspace = create_untitled_workspace(&f.service, &[&cwd, &tmp_dir], Some(&["currentworkingdirectory", "tempdir"])).await;
        assert!(std::path::Path::new(&je_platform::resources::fs_path(&workspace.config_path)).exists());
        assert!(f.service.is_untitled_workspace(&workspace));

        let ws = read_stored(&je_platform::resources::fs_path(&workspace.config_path));
        let folders = stored_folders(&ws);
        assert_eq!(folders.len(), 2);
        assert_path_equals(&path_of(&folders[0]), &cwd);
        assert_path_equals(&path_of(&folders[1]), &tmp_dir);
        assert_eq!(field(&folders[0], "name").as_deref(), Some("currentworkingdirectory"));
        assert_eq!(field(&folders[1], "name").as_deref(), Some("tempdir"));
    });
}

#[test]
fn create_untitled_workspace_folders_as_other_resource_uris() {
    run(|f| async move {
        let folder1_uri = URI::parse("myscheme://server/work/p/f1", false);
        let folder2_uri = URI::parse("myscheme://server/work/o/f3", false);

        let workspace = f
            .service
            .create_untitled_workspace(
                vec![WorkspaceFolderCreationData { uri: folder1_uri.clone(), name: None }, WorkspaceFolderCreationData { uri: folder2_uri.clone(), name: None }],
                Some("server".to_string()),
            )
            .await
            .unwrap();
        assert!(std::path::Path::new(&je_platform::resources::fs_path(&workspace.config_path)).exists());
        assert!(f.service.is_untitled_workspace(&workspace));

        let ws = read_stored(&je_platform::resources::fs_path(&workspace.config_path));
        let folders = stored_folders(&ws);
        assert_eq!(folders.len(), 2);
        assert_eq!(field(&folders[0], "uri").unwrap(), uri_text_unencoded(&folder1_uri));
        assert_eq!(field(&folders[1], "uri").unwrap(), uri_text_unencoded(&folder2_uri));
        assert!(field(&folders[0], "name").is_none_or(|name| name.is_empty()));
        assert!(field(&folders[1], "name").is_none_or(|name| name.is_empty()));
        assert_eq!(field(&ws, "remoteAuthority").as_deref(), Some("server"));
    });
}

#[test]
fn resolve_workspace() {
    run(|f| async move {
        let (cwd, tmp_dir) = (cwd(), tmp_dir());
        let mut workspace = create_untitled_workspace(&f.service, &[&cwd, &tmp_dir], None).await;
        assert!(f.service.resolve_local_workspace(&workspace.config_path).await.unwrap().is_some());

        // make it a valid workspace path
        let config_fs_path = je_platform::resources::fs_path(&workspace.config_path);
        let new_path = join(&[&dirname(&config_fs_path), &workspace_file_name()]);
        std::fs::rename(&config_fs_path, &new_path).unwrap();
        workspace.config_path = uri_file(&new_path);

        let resolved = f.service.resolve_local_workspace(&workspace.config_path).await.unwrap().unwrap();
        assert_eq!(2, resolved.folders.len());
        assert_equal_uri(&resolved.identifier.config_path, &workspace.config_path);
        assert!(!resolved.identifier.id.is_empty());
        write_json(&new_path, &object_of(vec![("something", string_value("something"))])); // invalid workspace

        let resolved_invalid = f.service.resolve_local_workspace(&workspace.config_path).await.unwrap();
        assert!(resolved_invalid.is_none());

        write_json(&new_path, &object_of(vec![("transient", Value::Bool(true)), ("folders", Value::Array(Vec::new()))])); // transient worksapce
        let resolved_transient = f.service.resolve_local_workspace(&workspace.config_path).await.unwrap();
        assert_eq!(resolved_transient.and_then(|resolved| resolved.transient), Some(true));
    });
}

/// Os três casos `resolveWorkspace (support relative paths)`: o arquivo do workspace tem uma pasta relativa e a pasta
/// resolvida é `expected` sob a pasta do arquivo.
async fn assert_resolves_relative_folder(f: &Fixture, relative_path: &str, expected: &[&str]) {
    let (cwd, tmp_dir) = (cwd(), tmp_dir());
    let workspace = create_untitled_workspace(&f.service, &[&cwd, &tmp_dir], None).await;
    let config_fs_path = je_platform::resources::fs_path(&workspace.config_path);
    write_json(&config_fs_path, &object_of(vec![("folders", Value::Array(vec![object_of(vec![("path", string_value(relative_path))])]))]));

    let resolved = f.service.resolve_local_workspace(&workspace.config_path).await.unwrap().unwrap();
    let mut segments = vec![dirname(&config_fs_path)];
    segments.extend(expected.iter().map(|segment| segment.to_string()));
    let segments: Vec<&str> = segments.iter().map(String::as_str).collect();
    assert_equal_uri(&resolved.folders[0].uri, &uri_file(&join(&segments)));
}

#[test]
fn resolve_workspace_support_relative_paths() {
    run(|f| async move {
        assert_resolves_relative_folder(&f, "./ticino-playground/lib", &["ticino-playground", "lib"]).await;
    });
}

#[test]
fn resolve_workspace_support_relative_paths_2() {
    run(|f| async move {
        assert_resolves_relative_folder(&f, "./ticino-playground/lib/../other", &["ticino-playground", "other"]).await;
    });
}

#[test]
fn resolve_workspace_support_relative_paths_3() {
    run(|f| async move {
        assert_resolves_relative_folder(&f, "ticino-playground/lib", &["ticino-playground", "lib"]).await;
    });
}

#[test]
fn resolve_workspace_support_invalid_json_via_fault_tolerant_parsing() {
    run(|f| async move {
        let (cwd, tmp_dir) = (cwd(), tmp_dir());
        let workspace = create_untitled_workspace(&f.service, &[&cwd, &tmp_dir], None).await;
        let config_fs_path = je_platform::resources::fs_path(&workspace.config_path);
        std::fs::write(&config_fs_path, "{ \"folders\": [ { \"path\": \"./ticino-playground/lib\" } , ] }").unwrap(); // trailing comma

        let resolved = f.service.resolve_local_workspace(&workspace.config_path).await.unwrap().unwrap();
        assert_equal_uri(&resolved.folders[0].uri, &uri_file(&join(&[&dirname(&config_fs_path), "ticino-playground", "lib"])));
    });
}

#[test]
fn rewrite_workspace_file_for_new_location_moves_relative_paths() {
    let (cwd, tmp_dir) = (cwd(), tmp_dir());
    let ext_uri = ext_uri_biased_ignore_path_case();
    let folder1 = cwd; // absolute path because outside of tmpDir
    let tmp_inside_dir = join(&[&tmp_dir, "inside"]);

    let first_config_path = join(&[&tmp_dir, &format!("myworkspace0-{}.code-workspace", date_now())]);
    create_workspace(&first_config_path, &[&folder1, "inside", &join(&["inside", "somefolder"])]);
    let orig_content = std::fs::read_to_string(&first_config_path).unwrap();

    let mut orig_config_path = uri_file(&first_config_path);
    let mut workspace_config_path = uri_file(&join(&[&tmp_dir, "inside", "myworkspace1.code-workspace"]));
    let mut new_content = rewrite_workspace_file_for_new_location(&orig_content, &orig_config_path, false, &workspace_config_path, &ext_uri).unwrap();
    let mut folders = stored_folders(&parse_stored(&new_content));
    assert_eq!(folders.len(), 3);
    assert_path_equals(&path_of(&folders[0]), &folder1); // absolute path because outside of tmpdir
    assert_path_equals(&path_of(&folders[1]), ".");
    assert_path_equals(&path_of(&folders[2]), "somefolder");

    orig_config_path = workspace_config_path;
    workspace_config_path = uri_file(&join(&[&tmp_dir, "myworkspace2.code-workspace"]));
    new_content = rewrite_workspace_file_for_new_location(&new_content, &orig_config_path, false, &workspace_config_path, &ext_uri).unwrap();
    folders = stored_folders(&parse_stored(&new_content));
    assert_eq!(folders.len(), 3);
    assert_path_equals(&path_of(&folders[0]), &folder1);
    assert_path_equals(&path_of(&folders[1]), "inside");
    assert_path_equals(&path_of(&folders[2]), "inside/somefolder");

    orig_config_path = workspace_config_path;
    workspace_config_path = uri_file(&join(&[&tmp_dir, "other", "myworkspace2.code-workspace"]));
    new_content = rewrite_workspace_file_for_new_location(&new_content, &orig_config_path, false, &workspace_config_path, &ext_uri).unwrap();
    folders = stored_folders(&parse_stored(&new_content));
    assert_eq!(folders.len(), 3);
    assert_path_equals(&path_of(&folders[0]), &folder1);
    assert_path_equals(&path_of(&folders[1]), "../inside");
    assert_path_equals(&path_of(&folders[2]), "../inside/somefolder");

    orig_config_path = workspace_config_path;
    workspace_config_path = URI::parse("foo://foo/bar/myworkspace2.code-workspace", false);
    new_content = rewrite_workspace_file_for_new_location(&new_content, &orig_config_path, false, &workspace_config_path, &ext_uri).unwrap();
    folders = stored_folders(&parse_stored(&new_content));
    assert_eq!(folders.len(), 3);
    assert_eq!(field(&folders[0], "uri").unwrap(), uri_text_unencoded(&uri_file(&folder1)));
    assert_eq!(field(&folders[1], "uri").unwrap(), uri_text_unencoded(&uri_file(&tmp_inside_dir)));
    assert_eq!(field(&folders[2], "uri").unwrap(), uri_text_unencoded(&uri_file(&join(&[&tmp_inside_dir, "somefolder"]))));

    std::fs::remove_file(&first_config_path).unwrap();
}

#[test]
fn rewrite_workspace_file_for_new_location_preserves_comments() {
    run(|f| async move {
        let (cwd, tmp_dir) = (cwd(), tmp_dir());
        let workspace = create_untitled_workspace(&f.service, &[&cwd, &tmp_dir, &join(&[&tmp_dir, "somefolder"])], None).await;
        let workspace_config_path = uri_file(&join(&[&tmp_dir, &format!("myworkspace.{}.{WORKSPACE_EXTENSION}", date_now())]));

        let mut orig_content = std::fs::read_to_string(je_platform::resources::fs_path(&workspace.config_path)).unwrap();
        orig_content = format!("// this is a comment\n{orig_content}");

        let new_content = rewrite_workspace_file_for_new_location(&orig_content, &workspace.config_path, false, &workspace_config_path, &ext_uri_biased_ignore_path_case()).unwrap();
        assert_eq!(Some(0), new_content.find("// this is a comment"));
        f.service.delete_untitled_workspace(&workspace).await.unwrap();
    });
}

#[test]
fn rewrite_workspace_file_for_new_location_preserves_forward_slashes() {
    run(|f| async move {
        let (cwd, tmp_dir) = (cwd(), tmp_dir());
        let workspace = create_untitled_workspace(&f.service, &[&cwd, &tmp_dir, &join(&[&tmp_dir, "somefolder"])], None).await;
        let workspace_config_path = uri_file(&join(&[&tmp_dir, &format!("myworkspace.{}.{WORKSPACE_EXTENSION}", date_now())]));

        let mut orig_content = std::fs::read_to_string(je_platform::resources::fs_path(&workspace.config_path)).unwrap();
        orig_content = orig_content.replace('\\', "/"); // convert backslash to slash

        let new_content = rewrite_workspace_file_for_new_location(&orig_content, &workspace.config_path, false, &workspace_config_path, &ext_uri_biased_ignore_path_case()).unwrap();
        assert!(stored_folders(&parse_stored(&new_content)).iter().all(|folder| !path_of(folder).contains('\\')));
        f.service.delete_untitled_workspace(&workspace).await.unwrap();
    });
}

#[test]
#[cfg_attr(not(windows), ignore = "test.skip fora do Windows, como no original")]
fn rewrite_workspace_file_for_new_location_unc_paths() {
    run(|f| async move {
        let tmp_dir = tmp_dir();
        let workspace_location = join(&[&tmp_dir, "wsloc"]);
        let folder1_location = "x:\\foo";
        let folder2_location = "\\\\server\\share2\\some\\path";
        let folder3_location = join(&[&workspace_location, "inner", "more"]);

        let workspace = create_untitled_workspace(&f.service, &[folder1_location, folder2_location, &folder3_location], None).await;
        let workspace_config_path = uri_file(&join(&[&workspace_location, &format!("myworkspace.{}.{WORKSPACE_EXTENSION}", date_now())]));
        let orig_content = std::fs::read_to_string(je_platform::resources::fs_path(&workspace.config_path)).unwrap();
        let new_content = rewrite_workspace_file_for_new_location(&orig_content, &workspace.config_path, true, &workspace_config_path, &ext_uri_biased_ignore_path_case()).unwrap();
        let folders = stored_folders(&parse_stored(&new_content));
        assert_path_equals(&path_of(&folders[0]), folder1_location);
        assert_path_equals(&path_of(&folders[1]), folder2_location);
        assert_path_equals(&path_of(&folders[2]), "inner/more");

        f.service.delete_untitled_workspace(&workspace).await.unwrap();
    });
}

#[test]
fn delete_untitled_workspace_untitled() {
    run(|f| async move {
        let (cwd, tmp_dir) = (cwd(), tmp_dir());
        let workspace = create_untitled_workspace(&f.service, &[&cwd, &tmp_dir], None).await;
        let config_fs_path = je_platform::resources::fs_path(&workspace.config_path);
        assert!(std::path::Path::new(&config_fs_path).exists());
        f.service.delete_untitled_workspace(&workspace).await.unwrap();
        assert!(!std::path::Path::new(&config_fs_path).exists());
    });
}

#[test]
fn delete_untitled_workspace_saved() {
    run(|f| async move {
        let (cwd, tmp_dir) = (cwd(), tmp_dir());
        let workspace = create_untitled_workspace(&f.service, &[&cwd, &tmp_dir], None).await;
        f.service.delete_untitled_workspace(&workspace).await.unwrap();
    });
}

#[test]
fn get_untitled_workspace() {
    run(|f| async move {
        f.service.initialize().await;
        let mut untitled = f.service.get_untitled_workspaces();
        assert_eq!(untitled.len(), 0);

        let (cwd, tmp_dir) = (cwd(), tmp_dir());
        let untitled_one = create_untitled_workspace(&f.service, &[&cwd, &tmp_dir], None).await;
        assert!(std::path::Path::new(&je_platform::resources::fs_path(&untitled_one.config_path)).exists());

        f.service.initialize().await;
        untitled = f.service.get_untitled_workspaces();
        assert_eq!(1, untitled.len());
        assert_eq!(untitled_one.id, untitled[0].workspace.id);

        f.service.delete_untitled_workspace(&untitled_one).await.unwrap();
        f.service.initialize().await;
        untitled = f.service.get_untitled_workspaces();
        assert_eq!(0, untitled.len());
    });
}
