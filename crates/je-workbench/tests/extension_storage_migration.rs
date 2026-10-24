//! Porte de `src/vs/workbench/services/extensions/test/browser/extensionStorageMigration.test.ts` (suíte `ExtensionStorageMigration`), caso a caso e na mesma ordem.
//!
//! Notas de tradução:
//! - **`setup` e `teardown`.** O `setup` é [`setup`]; o `teardown` (`disposables.clear()`) é o `drop` do `LeakGuard` ao fim do
//!   corpo, que também faz o `ensureNoDisposablesAreLeakedInTestSuite()`. O fixture fica vivo até o fim do corpo assíncrono.
//! - **`workbenchInstantiationService(undefined, disposables)`.** Os serviços que o teste não troca e que
//!   `migrateExtensionStorage` e o `ExtensionStorageService` não leem (editor, texto, ...) não entram: o [`setup`] monta uma
//!   coleção só com os serviços que o original deixa o `workbenchInstantiationService` fornecer e que algum código do caso lê
//!   (`IStorageService` em memória, `IProductService`, `ILogService`, `IWorkspaceContextService` com o `TestWorkspace`,
//!   `IUriIdentityService`) mais os que o próprio `setup` troca (`IFileService` real com o provider em memória,
//!   `IEnvironmentService`, `IUserDataProfilesService`, `IUserDataProfileService` e `IExtensionStorageService`).
//! - **`instantiationService.stub(IEnvironmentService, { userRoamingDataHome, workspaceStorageHome, cacheHome })`.** É o
//!   [`PartialEnvironmentService`]: os três campos dados e, nos demais membros do contrato, o `throw` de um objeto parcial
//!   que não os tem.
//! - **`URI.file('tests').with({ scheme: 'vscode-tests' })`.** É o `URI` `vscode-tests:///tests`.
//! - **`new UserDataProfilesService(environmentService, fileService, uriIdentityService, logService)`.** É o serviço com o
//!   armazenamento em memória dos perfis (`InMemoryProfilesStore`), que é o que a classe `common` do original faz.
//! - **`createInstance(ExtensionStorageService)`.** Os serviços injetados são o armazenamento, o produto e o log do fixture.
//! - **`assert.deepStrictEqual(x, undefined)`.** É `assert_eq!` com `None`; o estado é o `serde_json::Value` do `JSON.parse`.
//! - **Relógio.** O original não usa timers falsos: os casos rodam em `run_with_real_timers`.

#[path = "../../je-base/tests/common/mod.rs"]
mod common;

use std::rc::Rc;

use common::{LeakGuard, ensure_no_disposables_are_leaked_in_test_suite, run_with_real_timers};
use je_base::buffer::VSBuffer;
use je_base::uri::URI;
use je_instantiation::{Injectable, InstantiationService, ServiceCollection};
use je_platform::environment::environment::{
    DisableExtensions, EnvironmentService, ExtensionHostDebugParams, ExtensionKind, I_ENVIRONMENT_SERVICE, SyncMode,
};
use je_platform::files::file_service::{FileContentSource, FileService};
use je_platform::files::files::FileSystemProviderRef;
use je_platform::files::i_file_service::{IFileService, I_FILE_SERVICE};
use je_platform::files::in_memory::InMemoryFileSystemProvider;
use je_platform::log::{I_LOG_SERVICE, LogServiceRef, NullLogService};
use je_platform::product::{I_PRODUCT_SERVICE, IProductService, ProductService};
use je_platform::resources::join_path;
use je_platform::storage::{I_STORAGE_SERVICE, InMemoryStorageService, StorageScope, StorageService};
use je_platform::uri_identity::uri_identity::{I_URI_IDENTITY_SERVICE, IUriIdentityService};
use je_platform::uri_identity::uri_identity_service::UriIdentityService;
use je_platform::user_data_profile::{
    I_USER_DATA_PROFILES_SERVICE, IUserDataProfilesService, InMemoryProfilesStore, UserDataProfilesEnvironment, UserDataProfilesService,
};
use je_platform::workspace::{IWorkspaceContextService, I_WORKSPACE_CONTEXT_SERVICE, default_test_workspace};
use je_workbench::services::extension_management::extension_storage::{
    ExtensionStorageService, ExtensionStorageTarget, I_EXTENSION_STORAGE_SERVICE, IExtensionStorageService,
};
use je_workbench::services::extensions::extension_storage_migration::migrate_extension_storage;
use je_workbench::services::user_data_profile::user_data_profile::{I_USER_DATA_PROFILE_SERVICE, IUserDataProfileService};
use je_workbench::services::user_data_profile::user_data_profile_service::UserDataProfileService;
use je_workbench::test::common::workbench_test_services::TestContextService;
use serde_json::{Value, json};

//#region Utilitários dos casos

/// Gera os membros do contrato `IEnvironmentService` que o objeto parcial do original não tem: o `throw` de quem os lê.
macro_rules! unavailable_members {
    ($(fn $name:ident() -> $ret:ty;)*) => {
        $(
            fn $name(&self) -> $ret {
                panic!(concat!("o objeto parcial não tem ", stringify!($name)))
            }
        )*
    };
}

/// `{ userRoamingDataHome, workspaceStorageHome, cacheHome }` como `IEnvironmentService`.
struct PartialEnvironmentService {
    user_roaming_data_home: URI,
    workspace_storage_home: URI,
    cache_home: URI,
}

impl EnvironmentService for PartialEnvironmentService {
    fn user_roaming_data_home(&self) -> URI {
        self.user_roaming_data_home.clone()
    }

    fn workspace_storage_home(&self) -> URI {
        self.workspace_storage_home.clone()
    }

    fn cache_home(&self) -> URI {
        self.cache_home.clone()
    }

    unavailable_members! {
        fn state_resource() -> URI;
        fn keyboard_layout_resource() -> URI;
        fn argv_resource() -> URI;
        fn untitled_workspaces_home() -> URI;
        fn local_history_home() -> URI;
        fn app_shared_data_home() -> URI;
        fn user_data_sync_home() -> URI;
        fn sync() -> Option<SyncMode>;
        fn continue_on() -> Option<String>;
        fn edit_session_id() -> Option<String>;
        fn debug_extension_host() -> ExtensionHostDebugParams;
        fn is_extension_development() -> bool;
        fn disable_extensions() -> DisableExtensions;
        fn skip_builtin_extensions() -> Vec<String>;
        fn extension_development_location_uri() -> Option<Vec<URI>>;
        fn extension_development_kind() -> Option<Vec<ExtensionKind>>;
        fn extension_tests_location_uri() -> Option<URI>;
        fn logs_home() -> URI;
        fn log_level() -> Option<String>;
        fn extension_log_level() -> Option<Vec<(String, String)>>;
        fn verbose() -> bool;
        fn is_built() -> bool;
        fn disable_telemetry() -> bool;
        fn disable_experiments() -> bool;
        fn service_machine_id_resource() -> URI;
        fn agent_sessions_workspace() -> URI;
        fn policy_file() -> Option<URI>;
    }
}

/// `ROOT`: `URI.file('tests').with({ scheme: 'vscode-tests' })`.
fn root() -> URI {
    URI::parse("vscode-tests:///tests", false)
}

/// `workspaceStorageHome`.
fn workspace_storage_home() -> URI {
    join_path(&root(), &["workspaceStorageHome"])
}

/// O que o `setup` da suíte deixa montado.
struct Fixture {
    instantiation_service: Rc<InstantiationService>,
    extension_storage_service: Rc<ExtensionStorageService>,
    file_service: Rc<dyn IFileService>,
    storage_service: Rc<dyn StorageService>,
    user_data_profiles_service: Rc<dyn IUserDataProfilesService>,
}

/// O `setup` da suíte.
fn setup(disposables: &LeakGuard) -> Fixture {
    let collection = ServiceCollection::new();
    let instantiation_service = disposables.add_rc(InstantiationService::new(collection.clone()));

    // Os serviços do `workbenchInstantiationService` que algum código do caso lê.
    let storage = InMemoryStorageService::new();
    let storage_service: Rc<dyn StorageService> = Rc::new(storage.clone());
    disposables.add_rc(storage);
    collection.set(I_STORAGE_SERVICE, storage_service.clone());
    collection.set(I_PRODUCT_SERVICE, Rc::new(ProductService::default()) as Rc<dyn IProductService>);
    let log_service: LogServiceRef = NullLogService::shared();
    collection.set(I_LOG_SERVICE, log_service.clone());
    collection.set(I_WORKSPACE_CONTEXT_SERVICE, TestContextService::new(None, None) as Rc<dyn IWorkspaceContextService>);

    let file_service = disposables.add_rc(FileService::new(log_service.clone()));
    let provider = disposables.add_rc(InMemoryFileSystemProvider::new());
    let provider_ref: FileSystemProviderRef = provider;
    disposables.add(file_service.register_provider("vscode-tests", provider_ref).expect("o esquema ainda não tem provider"));
    let file_service_contract: Rc<dyn IFileService> = file_service;
    collection.set(I_FILE_SERVICE, file_service_contract.clone());

    let uri_identity_service = disposables.add_rc(UriIdentityService::create((), (file_service_contract.clone(),)));
    collection.set(I_URI_IDENTITY_SERVICE, uri_identity_service.clone() as Rc<dyn IUriIdentityService>);

    let environment_service: Rc<dyn EnvironmentService> =
        Rc::new(PartialEnvironmentService { user_roaming_data_home: root(), workspace_storage_home: workspace_storage_home(), cache_home: root() });
    collection.set(I_ENVIRONMENT_SERVICE, environment_service);
    let user_data_profiles_service = disposables.add_rc(UserDataProfilesService::new(
        UserDataProfilesEnvironment { user_roaming_data_home: root(), cache_home: root(), agent_sessions_workspace: None },
        file_service_contract.clone(),
        uri_identity_service.ext_uri(),
        log_service,
        Rc::new(InMemoryProfilesStore::default()),
    ));
    let user_data_profiles_service: Rc<dyn IUserDataProfilesService> = user_data_profiles_service;
    collection.set(I_USER_DATA_PROFILES_SERVICE, user_data_profiles_service.clone());
    let user_data_profile_service = disposables.add_rc(UserDataProfileService::create(user_data_profiles_service.default_profile(), ()));
    collection.set(I_USER_DATA_PROFILE_SERVICE, user_data_profile_service as Rc<dyn IUserDataProfileService>);

    let extension_storage_service = disposables.add_rc(instantiation_service.create_instance::<ExtensionStorageService>(()));
    collection.set(I_EXTENSION_STORAGE_SERVICE, extension_storage_service.clone() as Rc<dyn IExtensionStorageService>);

    Fixture { instantiation_service, extension_storage_service, file_service: file_service_contract, storage_service, user_data_profiles_service }
}

/// `id` como o alvo do estado da extensão.
fn target(extension_id: &str) -> ExtensionStorageTarget {
    ExtensionStorageTarget::from(extension_id)
}

//#endregion

#[test]
fn migrate_extension_storage_case() {
    run_with_real_timers(async {
        let disposables = ensure_no_disposables_are_leaked_in_test_suite();
        let fixture = setup(&disposables);

        let (from_extension_id, to_extension_id) = ("pub.from", "pub.to");
        let storage_migrated_key = format!("extensionStorage.migrate.{from_extension_id}-{to_extension_id}");
        let extension_storage_service = fixture.extension_storage_service.clone();
        let file_service = fixture.file_service.clone();
        let storage_service = fixture.storage_service.clone();
        let global_storage_home = fixture.user_data_profiles_service.default_profile().global_storage_home;
        let test_workspace_id = default_test_workspace().id();

        extension_storage_service.set_extension_state(&target(from_extension_id), Some(&json!({ "globalKey": "hello global state" })), true);
        extension_storage_service.set_extension_state(&target(from_extension_id), Some(&json!({ "workspaceKey": "hello workspace state" })), false);
        file_service
            .write_file(&join_path(&global_storage_home, &[from_extension_id]), FileContentSource::Buffer(VSBuffer::from_string("hello global storage")), None)
            .await
            .unwrap();
        file_service
            .write_file(
                &join_path(&workspace_storage_home(), &[test_workspace_id.as_str(), from_extension_id]),
                FileContentSource::Buffer(VSBuffer::from_string("hello workspace storage")),
                None,
            )
            .await
            .unwrap();

        migrate_extension_storage(from_extension_id, to_extension_id, true, &fixture.instantiation_service).await.unwrap();
        migrate_extension_storage(from_extension_id, to_extension_id, false, &fixture.instantiation_service).await.unwrap();

        assert_eq!(extension_storage_service.get_extension_state(&target(from_extension_id), true), None::<Value>);
        assert_eq!(extension_storage_service.get_extension_state(&target(from_extension_id), false), None::<Value>);
        assert!(!file_service.exists(&join_path(&global_storage_home, &[from_extension_id])).await.unwrap());
        assert!(!file_service.exists(&join_path(&workspace_storage_home(), &[test_workspace_id.as_str(), from_extension_id])).await.unwrap());

        assert_eq!(extension_storage_service.get_extension_state(&target(to_extension_id), true), Some(json!({ "globalKey": "hello global state" })));
        assert_eq!(extension_storage_service.get_extension_state(&target(to_extension_id), false), Some(json!({ "workspaceKey": "hello workspace state" })));
        assert_eq!(
            file_service.read_file(&join_path(&global_storage_home, &[to_extension_id]), None, None).await.unwrap().value.to_string(),
            "hello global storage"
        );
        assert_eq!(
            file_service.read_file(&join_path(&workspace_storage_home(), &[test_workspace_id.as_str(), to_extension_id]), None, None).await.unwrap().value.to_string(),
            "hello workspace storage"
        );

        assert_eq!(storage_service.get(&storage_migrated_key, StorageScope::Profile, None), Some("true".to_string()));
        assert_eq!(storage_service.get(&storage_migrated_key, StorageScope::Workspace, None), Some("true".to_string()));

        drop(fixture);
        drop(disposables);
    });
}

#[test]
fn migrate_extension_storage_when_does_not_exist() {
    run_with_real_timers(async {
        let disposables = ensure_no_disposables_are_leaked_in_test_suite();
        let fixture = setup(&disposables);

        let (from_extension_id, to_extension_id) = ("pub.from", "pub.to");
        let storage_migrated_key = format!("extensionStorage.migrate.{from_extension_id}-{to_extension_id}");
        let extension_storage_service = fixture.extension_storage_service.clone();
        let file_service = fixture.file_service.clone();
        let storage_service = fixture.storage_service.clone();
        let global_storage_home = fixture.user_data_profiles_service.default_profile().global_storage_home;
        let test_workspace_id = default_test_workspace().id();

        migrate_extension_storage(from_extension_id, to_extension_id, true, &fixture.instantiation_service).await.unwrap();
        migrate_extension_storage(from_extension_id, to_extension_id, false, &fixture.instantiation_service).await.unwrap();

        assert_eq!(extension_storage_service.get_extension_state(&target(from_extension_id), true), None::<Value>);
        assert_eq!(extension_storage_service.get_extension_state(&target(from_extension_id), false), None::<Value>);
        assert!(!file_service.exists(&join_path(&global_storage_home, &[from_extension_id])).await.unwrap());
        assert!(!file_service.exists(&join_path(&workspace_storage_home(), &[test_workspace_id.as_str(), from_extension_id])).await.unwrap());

        assert_eq!(extension_storage_service.get_extension_state(&target(to_extension_id), true), None::<Value>);
        assert_eq!(extension_storage_service.get_extension_state(&target(to_extension_id), false), None::<Value>);
        assert!(!file_service.exists(&join_path(&global_storage_home, &[to_extension_id])).await.unwrap());
        assert!(!file_service.exists(&join_path(&workspace_storage_home(), &[test_workspace_id.as_str(), to_extension_id])).await.unwrap());

        assert_eq!(storage_service.get(&storage_migrated_key, StorageScope::Profile, None), Some("true".to_string()));
        assert_eq!(storage_service.get(&storage_migrated_key, StorageScope::Workspace, None), Some("true".to_string()));

        drop(fixture);
        drop(disposables);
    });
}
