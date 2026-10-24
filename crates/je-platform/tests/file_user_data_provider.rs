//! Porte de `src/vs/platform/userData/test/browser/fileUserDataProvider.test.ts` (suítes `FileUserDataProvider` e
//! `FileUserDataProvider - Watching`), caso a caso e na mesma ordem.
//!
//! Notas de tradução:
//! - **`TestEnvironmentService`.** É o [`UserDataProfilesEnvironment`] (`userRoamingDataHome` com o esquema
//!   `vscode-userdata`, `cacheHome` igual). O `UriIdentityService` é o do porte, sobre o `FileService`, e o
//!   `UserDataProfilesService` recebe o `extUri` dele (o serviço do porte não toma o `IUriIdentityService` inteiro).
//! - **`setup`.** Na suíte `FileUserDataProvider` é `async` e roda no começo do corpo de cada teste, dentro do
//!   `runWithFakedTimers` (o `async_test`), com o `disposables` do `ensureNoDisposablesAreLeakedInTestSuite()` já
//!   criado. Na suíte `Watching` é síncrono. O `disposables.add(fileUserDataProvider)` duplicado do original é um só
//!   (o store ignora o repetido).
//! - **`try { ...; assert.fail(...) } catch (e) { }`.** O `catch` vazio do original também engole o `AssertionError`
//!   do `assert.fail`, então esses casos não reprovam em nenhum cenário: o porte executa a operação e descarta o
//!   resultado, como o original.
//! - **`done`.** Os casos `done => {...}` da suíte `Watching` esperam o evento, que o provider dispara de forma
//!   síncrona dentro do `fire`; o `done()` é uma flag conferida logo depois do `fire`.
//! - **`TestFileSystemProvider`.** Os métodos que o original lança `Not Supported` devolvem uma promessa rejeitada
//!   (nenhum é chamado pelos casos). `isEqual` é o `ext_uri().is_equal` sem viés, e `joinPath(resource, 'a/b')` é o
//!   `join_path` do `extUri` sem viés.

mod common;

use je_platform::files::i_file_service::IFileService;
use std::cell::Cell;
use std::rc::Rc;

use common::files::{async_test, join_path};
use common::{ensure_no_disposables_are_leaked_in_test_suite, uri_file};
use je_base::buffer::VSBuffer;
use je_base::errors::Error;
use je_base::event::{Emitter, Event};
use je_base::lifecycle::{self, DisposableRef, DisposableStore};
use je_base::network::schemas;
use je_base::promise::Promise;
use je_base::uri::{Field, URI, UriChange};
use je_instantiation::Injectable;
use je_platform::files::file_service::{FileContentSource, FileService};
use je_platform::files::files::{
    FileChange, FileChangeType, FileDeleteOptions, FileOpenOptions, FileOverwriteOptions, FileSystemProvider, FileSystemProviderCapabilities, FileType,
    FileWriteOptions, Stat, WatchOptions,
};
use je_platform::files::in_memory::InMemoryFileSystemProvider;
use je_platform::log::NullLogService;
use je_platform::resources::ext_uri;
use je_platform::uri_identity::uri_identity::IUriIdentityService;
use je_platform::uri_identity::uri_identity_service::UriIdentityService;
use je_platform::user_data::file_user_data_provider::FileUserDataProvider;
use je_platform::user_data_profile::{IUserDataProfilesService, InMemoryProfilesStore, UserDataProfilesEnvironment, UserDataProfilesService};

/// `ROOT`.
fn root() -> URI {
    uri_file("tests").with(&UriChange { scheme: Field::Set("vscode-tests".into()), ..UriChange::default() })
}

fn with_scheme(resource: &URI, scheme: &str) -> URI {
    resource.with(&UriChange { scheme: Field::Set(scheme.into()), ..UriChange::default() })
}

fn text(uri: &URI) -> String {
    uri.to_string(false).to_string_lossy()
}

fn buffer(content: &str) -> FileContentSource {
    VSBuffer::from_string(content).into()
}

/// O `TestEnvironmentService` sobre `app_settings_home`.
fn environment(app_settings_home: &URI) -> UserDataProfilesEnvironment {
    let user_roaming_data_home = with_scheme(app_settings_home, schemas::VSCODE_USER_DATA);
    UserDataProfilesEnvironment { user_roaming_data_home: user_roaming_data_home.clone(), cache_home: user_roaming_data_home, agent_sessions_workspace: None }
}

/// Os objetos do `setup` da suíte `FileUserDataProvider`.
struct Setup {
    test_object: Rc<dyn IFileService>,
    user_data_home_on_disk: URI,
    backup_workspace_home_on_disk: URI,
    environment: UserDataProfilesEnvironment,
    user_data_profiles_service: Rc<UserDataProfilesService>,
}

async fn setup(disposables: &Rc<DisposableStore>) -> Setup {
    let log_service = NullLogService::shared();
    let test_object = disposables.add_rc(FileService::new(log_service.clone()));
    let file_system_provider = disposables.add_rc(InMemoryFileSystemProvider::new());
    let root = root();
    disposables.add(test_object.register_provider(root.scheme().to_string_lossy().as_str(), file_system_provider.clone()).unwrap());

    let user_data_home_on_disk = join_path(&root, &["User"]);
    let backup_home = join_path(&root, &["Backups"]);
    let backup_workspace_home_on_disk = join_path(&backup_home, &["workspaceId"]);
    test_object.create_folder(&user_data_home_on_disk).await.unwrap();
    test_object.create_folder(&backup_workspace_home_on_disk).await.unwrap();

    let environment = environment(&user_data_home_on_disk);
    let uri_identity_service = disposables.add_rc(UriIdentityService::create((), (test_object.clone(),)));
    let user_data_profiles_service = disposables.add_rc(UserDataProfilesService::new(
        environment.clone(),
        test_object.clone(),
        uri_identity_service.ext_uri(),
        log_service.clone(),
        Rc::new(InMemoryProfilesStore::default()),
    ));

    let file_user_data_provider = disposables.add_rc(FileUserDataProvider::new(
        root.scheme().to_string_lossy().as_str(),
        file_system_provider,
        schemas::VSCODE_USER_DATA,
        user_data_profiles_service.clone(),
        uri_identity_service,
        log_service,
    ));
    disposables.add(test_object.register_provider(schemas::VSCODE_USER_DATA, file_user_data_provider).unwrap());

    Setup { test_object, user_data_home_on_disk, backup_workspace_home_on_disk, environment, user_data_profiles_service }
}

#[test]
fn exists_return_false_when_file_does_not_exist() {
    async_test(|disposables| async move {
        let Setup { test_object, user_data_profiles_service, .. } = setup(&disposables).await;
        let exists = test_object.exists(&user_data_profiles_service.default_profile().settings_resource).await.unwrap();
        assert!(!exists);
    });
}

#[test]
fn read_file_throws_error_if_not_exist() {
    async_test(|disposables| async move {
        let Setup { test_object, user_data_profiles_service, .. } = setup(&disposables).await;
        // try { ...; assert.fail('Should fail since file does not exist'); } catch (e) { }
        let _ = test_object.read_file(&user_data_profiles_service.default_profile().settings_resource, None, None).await;
    });
}

#[test]
fn read_existing_file() {
    async_test(|disposables| async move {
        let Setup { test_object, user_data_home_on_disk, user_data_profiles_service, .. } = setup(&disposables).await;
        test_object.write_file(&join_path(&user_data_home_on_disk, &["settings.json"]), buffer("{}"), None).await.unwrap();
        let result = test_object.read_file(&user_data_profiles_service.default_profile().settings_resource, None, None).await.unwrap();
        assert_eq!(result.value.to_string(), "{}");
    });
}

#[test]
fn create_file() {
    async_test(|disposables| async move {
        let Setup { test_object, user_data_home_on_disk, user_data_profiles_service, .. } = setup(&disposables).await;
        let resource = user_data_profiles_service.default_profile().settings_resource;
        let actual1 = test_object.create_file(&resource, Some(buffer("{}")), None).await.unwrap();
        assert_eq!(text(&actual1.resource), text(&resource));
        let actual2 = test_object.read_file(&join_path(&user_data_home_on_disk, &["settings.json"]), None, None).await.unwrap();
        assert_eq!(actual2.value.to_string(), "{}");
    });
}

#[test]
fn write_file_creates_the_file_if_not_exist() {
    async_test(|disposables| async move {
        let Setup { test_object, user_data_home_on_disk, user_data_profiles_service, .. } = setup(&disposables).await;
        let resource = user_data_profiles_service.default_profile().settings_resource;
        let actual1 = test_object.write_file(&resource, buffer("{}"), None).await.unwrap();
        assert_eq!(text(&actual1.resource), text(&resource));
        let actual2 = test_object.read_file(&join_path(&user_data_home_on_disk, &["settings.json"]), None, None).await.unwrap();
        assert_eq!(actual2.value.to_string(), "{}");
    });
}

#[test]
fn write_to_existing_file() {
    async_test(|disposables| async move {
        let Setup { test_object, user_data_home_on_disk, user_data_profiles_service, .. } = setup(&disposables).await;
        let resource = user_data_profiles_service.default_profile().settings_resource;
        test_object.write_file(&join_path(&user_data_home_on_disk, &["settings.json"]), buffer("{}"), None).await.unwrap();
        let actual1 = test_object.write_file(&resource, buffer("{a:1}"), None).await.unwrap();
        assert_eq!(text(&actual1.resource), text(&resource));
        let actual2 = test_object.read_file(&join_path(&user_data_home_on_disk, &["settings.json"]), None, None).await.unwrap();
        assert_eq!(actual2.value.to_string(), "{a:1}");
    });
}

#[test]
fn delete_file() {
    async_test(|disposables| async move {
        let Setup { test_object, user_data_home_on_disk, user_data_profiles_service, .. } = setup(&disposables).await;
        test_object.write_file(&join_path(&user_data_home_on_disk, &["settings.json"]), buffer(""), None).await.unwrap();
        test_object.del(&user_data_profiles_service.default_profile().settings_resource, None).await.unwrap();
        let result = test_object.exists(&join_path(&user_data_home_on_disk, &["settings.json"])).await.unwrap();
        assert!(!result);
    });
}

#[test]
fn resolve_file() {
    async_test(|disposables| async move {
        let Setup { test_object, user_data_home_on_disk, user_data_profiles_service, .. } = setup(&disposables).await;
        test_object.write_file(&join_path(&user_data_home_on_disk, &["settings.json"]), buffer(""), None).await.unwrap();
        let result = test_object.resolve(&user_data_profiles_service.default_profile().settings_resource, None).await.unwrap();
        assert!(!result.is_directory);
        assert!(result.children.is_none());
    });
}

#[test]
fn exists_return_false_for_folder_that_does_not_exist() {
    async_test(|disposables| async move {
        let Setup { test_object, user_data_profiles_service, .. } = setup(&disposables).await;
        let exists = test_object.exists(&user_data_profiles_service.default_profile().snippets_home).await.unwrap();
        assert!(!exists);
    });
}

#[test]
fn exists_return_true_for_folder_that_exists() {
    async_test(|disposables| async move {
        let Setup { test_object, user_data_home_on_disk, user_data_profiles_service, .. } = setup(&disposables).await;
        test_object.create_folder(&join_path(&user_data_home_on_disk, &["snippets"])).await.unwrap();
        let exists = test_object.exists(&user_data_profiles_service.default_profile().snippets_home).await.unwrap();
        assert!(exists);
    });
}

#[test]
fn read_file_throws_error_for_folder() {
    async_test(|disposables| async move {
        let Setup { test_object, user_data_home_on_disk, user_data_profiles_service, .. } = setup(&disposables).await;
        test_object.create_folder(&join_path(&user_data_home_on_disk, &["snippets"])).await.unwrap();
        // try { ...; assert.fail('Should fail since read file is not supported for folders'); } catch (e) { }
        let _ = test_object.read_file(&user_data_profiles_service.default_profile().snippets_home, None, None).await;
    });
}

#[test]
fn read_file_under_folder() {
    async_test(|disposables| async move {
        let Setup { test_object, user_data_home_on_disk, user_data_profiles_service, .. } = setup(&disposables).await;
        test_object.create_folder(&join_path(&user_data_home_on_disk, &["snippets"])).await.unwrap();
        test_object.write_file(&join_path(&user_data_home_on_disk, &["snippets", "settings.json"]), buffer("{}"), None).await.unwrap();
        let resource = join_path(&user_data_profiles_service.default_profile().snippets_home, &["settings.json"]);
        let actual = test_object.read_file(&resource, None, None).await.unwrap();
        assert_eq!(text(&actual.resource), text(&resource));
        assert_eq!(actual.value.to_string(), "{}");
    });
}

#[test]
fn read_file_under_sub_folder() {
    async_test(|disposables| async move {
        let Setup { test_object, user_data_home_on_disk, user_data_profiles_service, .. } = setup(&disposables).await;
        test_object.create_folder(&join_path(&user_data_home_on_disk, &["snippets", "java"])).await.unwrap();
        test_object.write_file(&join_path(&user_data_home_on_disk, &["snippets", "java", "settings.json"]), buffer("{}"), None).await.unwrap();
        let resource = join_path(&user_data_profiles_service.default_profile().snippets_home, &["java/settings.json"]);
        let actual = test_object.read_file(&resource, None, None).await.unwrap();
        assert_eq!(text(&actual.resource), text(&resource));
        assert_eq!(actual.value.to_string(), "{}");
    });
}

#[test]
fn create_file_under_folder_that_exists() {
    async_test(|disposables| async move {
        let Setup { test_object, user_data_home_on_disk, user_data_profiles_service, .. } = setup(&disposables).await;
        test_object.create_folder(&join_path(&user_data_home_on_disk, &["snippets"])).await.unwrap();
        let resource = join_path(&user_data_profiles_service.default_profile().snippets_home, &["settings.json"]);
        let actual1 = test_object.create_file(&resource, Some(buffer("{}")), None).await.unwrap();
        assert_eq!(text(&actual1.resource), text(&resource));
        let actual2 = test_object.read_file(&join_path(&user_data_home_on_disk, &["snippets", "settings.json"]), None, None).await.unwrap();
        assert_eq!(actual2.value.to_string(), "{}");
    });
}

#[test]
fn create_file_under_folder_that_does_not_exist() {
    async_test(|disposables| async move {
        let Setup { test_object, user_data_home_on_disk, user_data_profiles_service, .. } = setup(&disposables).await;
        let resource = join_path(&user_data_profiles_service.default_profile().snippets_home, &["settings.json"]);
        let actual1 = test_object.create_file(&resource, Some(buffer("{}")), None).await.unwrap();
        assert_eq!(text(&actual1.resource), text(&resource));
        let actual2 = test_object.read_file(&join_path(&user_data_home_on_disk, &["snippets", "settings.json"]), None, None).await.unwrap();
        assert_eq!(actual2.value.to_string(), "{}");
    });
}

#[test]
fn write_to_not_existing_file_under_container_that_exists() {
    async_test(|disposables| async move {
        let Setup { test_object, user_data_home_on_disk, user_data_profiles_service, .. } = setup(&disposables).await;
        test_object.create_folder(&join_path(&user_data_home_on_disk, &["snippets"])).await.unwrap();
        let resource = join_path(&user_data_profiles_service.default_profile().snippets_home, &["settings.json"]);
        let actual1 = test_object.write_file(&resource, buffer("{}"), None).await.unwrap();
        assert_eq!(text(&actual1.resource), text(&resource));
        let actual = test_object.read_file(&join_path(&user_data_home_on_disk, &["snippets", "settings.json"]), None, None).await.unwrap();
        assert_eq!(actual.value.to_string(), "{}");
    });
}

#[test]
fn write_to_not_existing_file_under_container_that_does_not_exists() {
    async_test(|disposables| async move {
        let Setup { test_object, user_data_home_on_disk, user_data_profiles_service, .. } = setup(&disposables).await;
        let resource = join_path(&user_data_profiles_service.default_profile().snippets_home, &["settings.json"]);
        let actual1 = test_object.write_file(&resource, buffer("{}"), None).await.unwrap();
        assert_eq!(text(&actual1.resource), text(&resource));
        let actual = test_object.read_file(&join_path(&user_data_home_on_disk, &["snippets", "settings.json"]), None, None).await.unwrap();
        assert_eq!(actual.value.to_string(), "{}");
    });
}

#[test]
fn write_to_existing_file_under_container() {
    async_test(|disposables| async move {
        let Setup { test_object, user_data_home_on_disk, user_data_profiles_service, .. } = setup(&disposables).await;
        test_object.create_folder(&join_path(&user_data_home_on_disk, &["snippets"])).await.unwrap();
        test_object.write_file(&join_path(&user_data_home_on_disk, &["snippets", "settings.json"]), buffer("{}"), None).await.unwrap();
        let resource = join_path(&user_data_profiles_service.default_profile().snippets_home, &["settings.json"]);
        let actual1 = test_object.write_file(&resource, buffer("{a:1}"), None).await.unwrap();
        assert_eq!(text(&actual1.resource), text(&resource));
        let actual = test_object.read_file(&join_path(&user_data_home_on_disk, &["snippets", "settings.json"]), None, None).await.unwrap();
        assert_eq!(actual.value.to_string(), "{a:1}");
    });
}

#[test]
fn write_file_under_sub_container() {
    async_test(|disposables| async move {
        let Setup { test_object, user_data_home_on_disk, user_data_profiles_service, .. } = setup(&disposables).await;
        let resource = join_path(&user_data_profiles_service.default_profile().snippets_home, &["java/settings.json"]);
        let actual1 = test_object.write_file(&resource, buffer("{}"), None).await.unwrap();
        assert_eq!(text(&actual1.resource), text(&resource));
        let actual = test_object.read_file(&join_path(&user_data_home_on_disk, &["snippets", "java", "settings.json"]), None, None).await.unwrap();
        assert_eq!(actual.value.to_string(), "{}");
    });
}

#[test]
fn delete_throws_error_for_folder_that_does_not_exist() {
    async_test(|disposables| async move {
        let Setup { test_object, user_data_profiles_service, .. } = setup(&disposables).await;
        // try { ...; assert.fail('Should fail the folder does not exist'); } catch (e) { }
        let _ = test_object.del(&user_data_profiles_service.default_profile().snippets_home, None).await;
    });
}

#[test]
fn delete_not_existing_file_under_container_that_exists() {
    async_test(|disposables| async move {
        let Setup { test_object, user_data_home_on_disk, user_data_profiles_service, .. } = setup(&disposables).await;
        test_object.create_folder(&join_path(&user_data_home_on_disk, &["snippets"])).await.unwrap();
        // try { ...; assert.fail('Should fail since file does not exist'); } catch (e) { }
        let _ = test_object.del(&join_path(&user_data_profiles_service.default_profile().snippets_home, &["settings.json"]), None).await;
    });
}

#[test]
fn delete_not_existing_file_under_container_that_does_not_exists() {
    async_test(|disposables| async move {
        let Setup { test_object, user_data_profiles_service, .. } = setup(&disposables).await;
        // try { ...; assert.fail('Should fail since file does not exist'); } catch (e) { }
        let _ = test_object.del(&join_path(&user_data_profiles_service.default_profile().snippets_home, &["settings.json"]), None).await;
    });
}

#[test]
fn delete_existing_file_under_folder() {
    async_test(|disposables| async move {
        let Setup { test_object, user_data_home_on_disk, user_data_profiles_service, .. } = setup(&disposables).await;
        test_object.create_folder(&join_path(&user_data_home_on_disk, &["snippets"])).await.unwrap();
        test_object.write_file(&join_path(&user_data_home_on_disk, &["snippets", "settings.json"]), buffer("{}"), None).await.unwrap();
        test_object.del(&join_path(&user_data_profiles_service.default_profile().snippets_home, &["settings.json"]), None).await.unwrap();
        let exists = test_object.exists(&join_path(&user_data_home_on_disk, &["snippets", "settings.json"])).await.unwrap();
        assert!(!exists);
    });
}

#[test]
fn resolve_folder() {
    async_test(|disposables| async move {
        let Setup { test_object, user_data_home_on_disk, user_data_profiles_service, .. } = setup(&disposables).await;
        test_object.create_folder(&join_path(&user_data_home_on_disk, &["snippets"])).await.unwrap();
        test_object.write_file(&join_path(&user_data_home_on_disk, &["snippets", "settings.json"]), buffer("{}"), None).await.unwrap();
        let result = test_object.resolve(&user_data_profiles_service.default_profile().snippets_home, None).await.unwrap();
        assert!(result.is_directory);
        let children = result.children.as_ref().expect("a pasta tem filhos");
        assert_eq!(children.len(), 1);
        assert_eq!(text(&children[0].resource), text(&join_path(&user_data_profiles_service.default_profile().snippets_home, &["settings.json"])));
    });
}

#[test]
fn read_backup_file() {
    async_test(|disposables| async move {
        let Setup { test_object, backup_workspace_home_on_disk, environment, .. } = setup(&disposables).await;
        test_object.write_file(&join_path(&backup_workspace_home_on_disk, &["backup.json"]), buffer("{}"), None).await.unwrap();
        let backup_home = with_scheme(&backup_workspace_home_on_disk, &environment.user_roaming_data_home.scheme().to_string_lossy());
        let result = test_object.read_file(&join_path(&backup_home, &["backup.json"]), None, None).await.unwrap();
        assert_eq!(result.value.to_string(), "{}");
    });
}

#[test]
fn create_backup_file() {
    async_test(|disposables| async move {
        let Setup { test_object, backup_workspace_home_on_disk, environment, .. } = setup(&disposables).await;
        let backup_home = with_scheme(&backup_workspace_home_on_disk, &environment.user_roaming_data_home.scheme().to_string_lossy());
        test_object.create_file(&join_path(&backup_home, &["backup.json"]), Some(buffer("{}")), None).await.unwrap();
        let result = test_object.read_file(&join_path(&backup_workspace_home_on_disk, &["backup.json"]), None, None).await.unwrap();
        assert_eq!(result.value.to_string(), "{}");
    });
}

#[test]
fn write_backup_file() {
    async_test(|disposables| async move {
        let Setup { test_object, backup_workspace_home_on_disk, environment, .. } = setup(&disposables).await;
        test_object.write_file(&join_path(&backup_workspace_home_on_disk, &["backup.json"]), buffer("{}"), None).await.unwrap();
        let backup_home = with_scheme(&backup_workspace_home_on_disk, &environment.user_roaming_data_home.scheme().to_string_lossy());
        test_object.write_file(&join_path(&backup_home, &["backup.json"]), buffer("{a:1}"), None).await.unwrap();
        let result = test_object.read_file(&join_path(&backup_workspace_home_on_disk, &["backup.json"]), None, None).await.unwrap();
        assert_eq!(result.value.to_string(), "{a:1}");
    });
}

#[test]
fn resolve_backups_folder() {
    async_test(|disposables| async move {
        let Setup { test_object, backup_workspace_home_on_disk, environment, .. } = setup(&disposables).await;
        test_object.write_file(&join_path(&backup_workspace_home_on_disk, &["backup.json"]), buffer("{}"), None).await.unwrap();
        let backup_home = with_scheme(&backup_workspace_home_on_disk, &environment.user_roaming_data_home.scheme().to_string_lossy());
        let result = test_object.resolve(&backup_home, None).await.unwrap();
        assert!(result.is_directory);
        let children = result.children.as_ref().expect("a pasta tem filhos");
        assert_eq!(children.len(), 1);
        assert_eq!(text(&children[0].resource), text(&join_path(&backup_home, &["backup.json"])));
    });
}

/// `TestFileSystemProvider`.
struct TestFileSystemProvider {
    on_did_change_file: Event<Vec<FileChange>>,
}

/// `throw new Error('Not Supported')`.
fn not_supported<T: Clone + 'static>() -> Promise<T> {
    Promise::rejected(Error::new("Not Supported"))
}

impl FileSystemProvider for TestFileSystemProvider {
    fn capabilities(&self) -> FileSystemProviderCapabilities {
        FileSystemProviderCapabilities::FILE_READ_WRITE
    }

    fn on_did_change_capabilities(&self) -> Event<()> {
        Event::none()
    }

    fn on_did_change_file(&self) -> Event<Vec<FileChange>> {
        self.on_did_change_file.clone()
    }

    fn watch(&self, _resource: &URI, _opts: &WatchOptions) -> DisposableRef {
        lifecycle::none()
    }

    fn stat(&self, _resource: &URI) -> Promise<Stat> {
        not_supported()
    }

    fn mkdir(&self, _resource: &URI) -> Promise<()> {
        not_supported()
    }

    fn readdir(&self, _resource: &URI) -> Promise<Vec<(String, FileType)>> {
        not_supported()
    }

    fn delete(&self, _resource: &URI, _opts: &FileDeleteOptions) -> Promise<()> {
        not_supported()
    }

    fn rename(&self, _from: &URI, _to: &URI, _opts: &FileOverwriteOptions) -> Promise<()> {
        not_supported()
    }

    fn read_file(&self, _resource: &URI, _atomic: bool) -> Promise<VSBuffer> {
        not_supported()
    }

    fn write_file(&self, _resource: &URI, _content: VSBuffer, _opts: &FileWriteOptions) -> Promise<()> {
        not_supported()
    }

    fn open(&self, _resource: &URI, _opts: &FileOpenOptions) -> Promise<i64> {
        not_supported()
    }

    fn close(&self, _fd: i64) -> Promise<()> {
        not_supported()
    }

    fn read(&self, _fd: i64, _pos: u64, _data: &VSBuffer, _offset: usize, _length: usize) -> Promise<usize> {
        not_supported()
    }

    fn write(&self, _fd: i64, _pos: u64, _data: &VSBuffer, _offset: usize, _length: usize) -> Promise<usize> {
        not_supported()
    }
}

/// Os objetos do `setup` da suíte `FileUserDataProvider - Watching`.
struct WatchingSetup {
    test_object: Rc<FileUserDataProvider>,
    file_event_emitter: Rc<Emitter<Vec<FileChange>>>,
    root_file_resource: URI,
    root_user_data_resource: URI,
}

fn watching_setup(disposables: &Rc<DisposableStore>) -> WatchingSetup {
    let root_file_resource = join_path(&root(), &["User"]);
    let root_user_data_resource = with_scheme(&root_file_resource, schemas::VSCODE_USER_DATA);

    let log_service = NullLogService::shared();
    let file_service = disposables.add_rc(FileService::new(log_service.clone()));
    let uri_identity_service = disposables.add_rc(UriIdentityService::create((), (file_service.clone(),)));
    let user_data_profiles_service = disposables.add_rc(UserDataProfilesService::new(
        environment(&root_file_resource),
        file_service,
        uri_identity_service.ext_uri(),
        log_service,
        Rc::new(InMemoryProfilesStore::default()),
    ));

    let file_event_emitter: Rc<Emitter<Vec<FileChange>>> = disposables.add_rc(Emitter::new());
    let test_object = disposables.add_rc(FileUserDataProvider::new(
        root_file_resource.scheme().to_string_lossy().as_str(),
        Rc::new(TestFileSystemProvider { on_did_change_file: file_event_emitter.event() }),
        schemas::VSCODE_USER_DATA,
        user_data_profiles_service,
        uri_identity_service,
        NullLogService::shared(),
    ));

    WatchingSetup { test_object, file_event_emitter, root_file_resource, root_user_data_resource }
}

fn watch_options() -> WatchOptions {
    WatchOptions { excludes: Vec::new(), recursive: false, ..WatchOptions::default() }
}

/// O corpo comum dos casos `done => {...}`: observa `segments` sob a pasta vigiada e dispara `change_type` no recurso
/// equivalente do sistema de arquivos; o `done()` é a flag conferida no fim.
fn assert_change_event_is_forwarded(segments: &[&str], change_type: FileChangeType) {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let disposables = guard.store();
    let WatchingSetup { test_object, file_event_emitter, root_file_resource, root_user_data_resource } = watching_setup(&disposables);

    disposables.add(test_object.watch(&root_user_data_resource, &watch_options()));
    let expected = join_path(&root_user_data_resource, segments);
    let target = join_path(&root_file_resource, segments);
    let done = Rc::new(Cell::new(false));
    let (listener_done, listener_expected) = (done.clone(), expected);
    disposables.add(test_object.on_did_change_file().listen(move |e| {
        if ext_uri().is_equal(Some(&e[0].resource), Some(&listener_expected), false) && e[0].change_type == change_type {
            listener_done.set(true);
        }
    }));
    file_event_emitter.fire(vec![FileChange::new(change_type, target)]);
    assert!(done.get(), "o evento esperado não chegou");
}

#[test]
fn watching_file_added_change_event() {
    assert_change_event_is_forwarded(&["settings.json"], FileChangeType::Added);
}

#[test]
fn watching_file_updated_change_event() {
    assert_change_event_is_forwarded(&["settings.json"], FileChangeType::Updated);
}

#[test]
fn watching_file_deleted_change_event() {
    assert_change_event_is_forwarded(&["settings.json"], FileChangeType::Deleted);
}

#[test]
fn watching_file_under_folder_created_change_event() {
    assert_change_event_is_forwarded(&["snippets", "settings.json"], FileChangeType::Added);
}

#[test]
fn watching_file_under_folder_updated_change_event() {
    assert_change_event_is_forwarded(&["snippets", "settings.json"], FileChangeType::Updated);
}

#[test]
fn watching_file_under_folder_deleted_change_event() {
    assert_change_event_is_forwarded(&["snippets", "settings.json"], FileChangeType::Deleted);
}

#[test]
fn watching_event_is_not_triggered_if_not_watched() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let disposables = guard.store();
    let WatchingSetup { test_object, file_event_emitter, root_file_resource, .. } = watching_setup(&disposables);

    let target = join_path(&root_file_resource, &["settings.json"]);
    let triggered = Rc::new(Cell::new(false));
    let listener_triggered = triggered.clone();
    disposables.add(test_object.on_did_change_file().listen(move |_| listener_triggered.set(true)));
    file_event_emitter.fire(vec![FileChange::new(FileChangeType::Deleted, target)]);
    assert!(!triggered.get(), "event should not be triggered");
}

#[test]
fn watching_event_is_not_triggered_if_not_watched_2() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let disposables = guard.store();
    let WatchingSetup { test_object, file_event_emitter, root_file_resource, root_user_data_resource } = watching_setup(&disposables);

    disposables.add(test_object.watch(&root_user_data_resource, &watch_options()));
    let target = join_path(&ext_uri().dirname(&root_file_resource), &["settings.json"]);
    let triggered = Rc::new(Cell::new(false));
    let listener_triggered = triggered.clone();
    disposables.add(test_object.on_did_change_file().listen(move |_| listener_triggered.set(true)));
    file_event_emitter.fire(vec![FileChange::new(FileChangeType::Deleted, target)]);
    assert!(!triggered.get(), "event should not be triggered");
}
