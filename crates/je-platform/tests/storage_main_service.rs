//! Porte de `src/vs/platform/storage/test/electron-main/storageMainService.test.ts`.
//!
//! Um `#[test]` por caso do original, na mesma ordem.
//!
//! Notas de tradução:
//! - O teste usa armazenamento só em memória (`useInMemoryStorage`) com o sistema de arquivos real para o ambiente, numa
//!   pasta temporária que nunca é criada: cada caso roda no laço de eventos real ([`run_on_real_loop`]). O
//!   `TestStorageMainService` (que sobrescreve `getStorageOptions`) é o [`StorageMainService::with_storage_options`].
//! - O `disposables` do original é um `DisposableStore` do escopo da suíte, criado antes do rastreio de vazamentos
//!   (`ensureNoDisposablesAreLeakedInTestSuite`, registrado no fim da suíte, depois do `teardown`): o rastreio começa
//!   com o caso e termina depois do `teardown`, que espera o aquecimento em segundo plano e limpa o `disposables`.
//! - O `NativeEnvironmentService` é o de `NativeEnvironmentService::native` com os argumentos vazios; o `FileService` não
//!   tem providers, como no original; o `StateService` (do `UserDataProfilesMainService`) não é iniciado, como no original.
//! - `inMemoryProfile` é um [`UserDataProfile`] com a raiz no esquema `inmemory`; o `{ id: generateUuid() }` do workspace
//!   é um identificador de janela vazia.
//! - `strictEqual(a, b)` e `notStrictEqual(a, b)` de instâncias são `Rc::ptr_eq`; `typeof x === 'string'` é `is_some()`.

mod common;

use std::cell::{Cell, RefCell};
use std::future::Future;
use std::rc::Rc;

use common::disk::run_on_real_loop;
use common::ensure_no_disposables_are_leaked_in_test_suite;
use common::lifecycle_main_service::TestLifecycleMainService;
use common::storage_main::{StorageMainServiceSetup, create_storage_main_service};
use je_base::lifecycle::DisposableStore;
use je_base::network::schemas;
use je_base::promise::Promise;
use je_base::uri::{Field, UriChange};
use je_base::uuid::generate_uuid;
use je_platform::environment::NativeParsedArgs;
use je_platform::environment::native::NativeEnvironmentService;
use je_platform::files::file_service::FileService;
use je_platform::log::NullLogService;
use je_platform::product::ProductService;
use je_platform::resources::{join_path, uri_file};
use je_platform::storage::{
    CompareAndSwapResult, IS_NEW_KEY, IStorageMain, IStorageMainService, StorageCallResult, StorageDatabaseChannel, StorageMainOptions, StorageMainService, StorageRequest, StorageScope,
};
use je_platform::telemetry::{CURRENT_SESSION_DATE_STORAGE_KEY, FIRST_SESSION_DATE_STORAGE_KEY};
use je_platform::user_data_profile::UserDataProfile;
use je_platform::workspace::{AnyWorkspaceIdentifier, EmptyWorkspaceIdentifier};
use je_storage::{StorageChangeEvent, StorageValue};

fn in_memory_profile() -> UserDataProfile {
    let in_memory_profile_root = uri_file("/location").with(&UriChange { scheme: Field::Set(schemas::IN_MEMORY.into()), ..Default::default() });
    let joined = |fragment: &str| join_path(&in_memory_profile_root, &[fragment]);
    UserDataProfile {
        id: "id".to_string(),
        name: "inMemory".to_string(),
        is_default: false,
        icon: None,
        location: in_memory_profile_root.clone(),
        global_storage_home: joined("globalStorageHome"),
        settings_resource: joined("settingsResource"),
        keybindings_resource: joined("keybindingsResource"),
        tasks_resource: joined("tasksResource"),
        mcp_resource: joined("mcp.json"),
        language_models_resource: joined("chatLanguageModels.json"),
        snippets_home: joined("snippetsHome"),
        prompts_home: joined("promptsHome"),
        extensions_resource: joined("extensionsResource"),
        cache_home: joined("cache"),
        agent_plugins_home: joined("agentPluginsHome"),
        use_default_flags: None,
        is_internal: None,
        is_transient: None,
        is_agents_window_profile: None,
        workspaces: None,
    }
}

fn workspace() -> AnyWorkspaceIdentifier {
    AnyWorkspaceIdentifier::Empty(EmptyWorkspaceIdentifier { id: generate_uuid() })
}

/// O estado da suíte: o `disposables` e os `storageMainServices` que o `teardown` aguarda.
struct Suite {
    disposables: Rc<DisposableStore>,
    storage_main_services: RefCell<Vec<Rc<StorageMainService>>>,
}

impl Suite {
    fn new() -> Rc<Self> {
        Rc::new(Self { disposables: DisposableStore::new(), storage_main_services: RefCell::new(Vec::new()) })
    }

    /// O `teardown`: o serviço aquece o armazém da aplicação em segundo plano assim que uma janela abre (o
    /// `TestLifecycleMainService` informa isso na hora). Espera esse aquecimento antes de descartar, senão ele pode
    /// terminar depois do descarte e vazar disposables para o caso seguinte.
    async fn teardown(&self) {
        let services: Vec<_> = self.storage_main_services.borrow_mut().drain(..).collect();
        let warm_ups: Vec<Promise<()>> = services.iter().flat_map(|service| [service.application_storage().init(), service.application_shared_storage().init()]).collect();
        Promise::all(warm_ups).await.unwrap();

        self.disposables.clear();
    }

    fn create_storage_service(&self, lifecycle_main_service: Rc<TestLifecycleMainService>) -> Rc<StorageMainService> {
        let log_service = NullLogService::shared();
        let environment_service = Rc::new(NativeEnvironmentService::native(NativeParsedArgs::default(), ProductService::default(), "/app").unwrap());
        let file_service = self.disposables.add_rc(FileService::new(log_service.clone()));

        // `getStorageOptions()` sobrescrito: useInMemoryStorage
        let StorageMainServiceSetup { storage_main_service: test_storage_service, .. } = create_storage_main_service(
            &self.disposables,
            Some(StorageMainOptions { use_in_memory_storage: true }),
            environment_service,
            file_service,
            log_service,
            lifecycle_main_service,
        );
        self.storage_main_services.borrow_mut().push(test_storage_service.clone());

        self.disposables.add(test_storage_service.application_storage());

        test_storage_service
    }

    async fn test_storage(&self, storage: Rc<dyn IStorageMain>, scope: StorageScope) {
        assert!(storage.is_in_memory());

        // Telemetry: added after init unless workspace/profile scoped
        if scope == StorageScope::Application {
            assert_eq!(storage.items().len(), 0);
            storage.init().await.unwrap();
            assert!(storage.get(FIRST_SESSION_DATE_STORAGE_KEY, None).is_some());
            assert!(storage.get(CURRENT_SESSION_DATE_STORAGE_KEY, None).is_some());
        } else {
            storage.init().await.unwrap();
        }

        let storage_change_event: Rc<RefCell<Option<StorageChangeEvent>>> = Rc::default();
        let last_event = storage_change_event.clone();
        self.disposables.add(storage.on_did_change_storage().listen(move |e| {
            *last_event.borrow_mut() = Some(e.clone());
        }));

        let storage_did_close = Rc::new(Cell::new(false));
        let did_close = storage_did_close.clone();
        self.disposables.add(storage.on_did_close_storage().listen(move |_| did_close.set(true)));

        // Basic store/get/remove
        let size = storage.items().len();

        let _ = storage.set("bar", StorageValue::from("foo"));
        assert_eq!(storage_change_event.borrow().as_ref().unwrap().key, "bar");
        let _ = storage.set("barNumber", StorageValue::Number(55.0));
        let _ = storage.set("barBoolean", StorageValue::Bool(true));

        assert_eq!(storage.get("bar", None).as_deref(), Some("foo"));
        assert_eq!(storage.get("barNumber", None).as_deref(), Some("55"));
        assert_eq!(storage.get("barBoolean", None).as_deref(), Some("true"));

        assert_eq!(storage.items().len(), size + 3);

        let _ = storage.delete("bar");
        assert_eq!(storage.get("bar", None), None);

        assert_eq!(storage.items().len(), size + 2);

        // IS_NEW
        assert_eq!(storage.get(IS_NEW_KEY, None).as_deref(), Some("true"));

        // Close
        storage.close().await.unwrap();

        assert!(storage_did_close.get());
    }
}

fn run<F>(body: impl FnOnce(Rc<Suite>) -> F + 'static)
where
    F: Future<Output = ()> + 'static,
{
    run_on_real_loop(async move {
        // O `disposables` é da suíte e nasce antes do rastreio; o rastreio (o `ensureNoDisposablesAreLeakedInTestSuite()`
        // do fim da suíte) vale para o caso e para o `teardown`, e a conferência vem depois dele.
        let suite = Suite::new();
        let _guard = ensure_no_disposables_are_leaked_in_test_suite();
        body(suite.clone()).await;
        suite.teardown().await;
    });
}

#[test]
fn basics_application() {
    run(|suite| async move {
        let storage_main_service = suite.create_storage_service(TestLifecycleMainService::new());

        suite.test_storage(storage_main_service.application_storage(), StorageScope::Application).await;
    });
}

#[test]
fn basics_profile() {
    run(|suite| async move {
        let storage_main_service = suite.create_storage_service(TestLifecycleMainService::new());
        let profile = in_memory_profile();

        suite.test_storage(storage_main_service.profile_storage(&profile), StorageScope::Profile).await;
    });
}

#[test]
fn basics_application_shared() {
    run(|suite| async move {
        let storage_main_service = suite.create_storage_service(TestLifecycleMainService::new());

        suite.test_storage(storage_main_service.application_shared_storage(), StorageScope::ApplicationShared).await;
    });
}

#[test]
fn basics_workspace() {
    run(|suite| async move {
        let workspace = workspace();
        let storage_main_service = suite.create_storage_service(TestLifecycleMainService::new());

        suite.test_storage(storage_main_service.workspace_storage(&workspace), StorageScope::Workspace).await;
    });
}

#[test]
fn storage_channel_compare_and_swap_isolates_keys_and_storage_scopes() {
    run(|suite| async move {
        struct Case {
            name: &'static str,
            request: StorageRequest,
            storage: Rc<dyn IStorageMain>,
        }

        let storage_main_service = suite.create_storage_service(TestLifecycleMainService::new());
        let channel = suite.disposables.add_rc(StorageDatabaseChannel::new(NullLogService::shared(), storage_main_service.clone()));
        let workspace = workspace();
        let application_shared_storage = storage_main_service.application_shared_storage();
        suite.disposables.add(application_shared_storage.clone());
        let profile_storage = storage_main_service.profile_storage(&in_memory_profile());
        suite.disposables.add(profile_storage.clone());
        let workspace_storage = storage_main_service.workspace_storage(&workspace);
        suite.disposables.add(workspace_storage.clone());
        let cases = [
            Case { name: "application", request: StorageRequest::default(), storage: storage_main_service.application_storage() },
            Case { name: "applicationShared", request: StorageRequest { application_shared: true, ..StorageRequest::default() }, storage: application_shared_storage.clone() },
            Case { name: "profile", request: StorageRequest { profile: Some(in_memory_profile()), ..StorageRequest::default() }, storage: profile_storage.clone() },
            Case { name: "workspace", request: StorageRequest { workspace: Some(workspace.clone()), ..StorageRequest::default() }, storage: workspace_storage.clone() },
        ];
        for test_case in &cases {
            let request = StorageRequest {
                insert: Some(vec![("key".to_string(), format!("{}-first", test_case.name)), ("unrelated".to_string(), format!("{}-sentinel", test_case.name))]),
                ..test_case.request.clone()
            };
            channel.call("updateItems", request).await.unwrap();
        }

        let compare_and_swap = |request: StorageRequest| {
            let channel = channel.clone();
            async move {
                match channel.call("compareAndSwap", request).await.unwrap() {
                    StorageCallResult::CompareAndSwap(result) => result,
                    other => panic!("esperava o resultado do compareAndSwap: {other:?}"),
                }
            }
        };
        let mut results: Vec<CompareAndSwapResult> = Vec::new();
        for test_case in &cases {
            let request = StorageRequest {
                key: "key".to_string(),
                expected_value: Some(format!("{}-first", test_case.name)),
                new_value: format!("{}-second", test_case.name),
                ..test_case.request.clone()
            };
            results.push(compare_and_swap(request).await);
        }
        let rejected_request = StorageRequest { key: "key".to_string(), expected_value: Some("stale".to_string()), new_value: "should-not-write".to_string(), ..cases[0].request.clone() };
        let rejected = compare_and_swap(rejected_request).await;
        let stored: Vec<(&str, Option<String>, Option<String>)> =
            cases.iter().map(|test_case| (test_case.name, test_case.storage.get("key", None), test_case.storage.get("unrelated", None))).collect();
        Promise::all(vec![application_shared_storage.close(), profile_storage.close(), workspace_storage.close()]).await.unwrap();

        assert_eq!(results, cases.iter().map(|test_case| CompareAndSwapResult { swapped: true, current_value: Some(format!("{}-second", test_case.name)) }).collect::<Vec<_>>());
        assert_eq!(rejected, CompareAndSwapResult { swapped: false, current_value: Some("application-second".to_string()) });
        assert_eq!(
            stored,
            cases.iter().map(|test_case| (test_case.name, Some(format!("{}-second", test_case.name)), Some(format!("{}-sentinel", test_case.name)))).collect::<Vec<_>>()
        );
    });
}

#[test]
fn storage_closed_on_will_shutdown() {
    run(|suite| async move {
        let lifecycle_main_service = TestLifecycleMainService::new();
        let storage_main_service = suite.create_storage_service(lifecycle_main_service.clone());

        let profile = in_memory_profile();
        let workspace = workspace();

        let workspace_storage = storage_main_service.workspace_storage(&workspace);
        let did_close_workspace_storage = Rc::new(Cell::new(false));
        let did_close = did_close_workspace_storage.clone();
        suite.disposables.add(workspace_storage.on_did_close_storage().listen(move |_| did_close.set(true)));

        let profile_storage = storage_main_service.profile_storage(&profile);
        let did_close_profile_storage = Rc::new(Cell::new(false));
        let did_close = did_close_profile_storage.clone();
        suite.disposables.add(profile_storage.on_did_close_storage().listen(move |_| did_close.set(true)));

        let application_storage = storage_main_service.application_storage();
        let did_close_application_storage = Rc::new(Cell::new(false));
        let did_close = did_close_application_storage.clone();
        suite.disposables.add(application_storage.on_did_close_storage().listen(move |_| did_close.set(true)));

        assert!(Rc::ptr_eq(&application_storage, &storage_main_service.application_storage())); // same instance as long as not closed
        assert!(Rc::ptr_eq(&profile_storage, &storage_main_service.profile_storage(&profile))); // same instance as long as not closed
        assert!(Rc::ptr_eq(&workspace_storage, &storage_main_service.workspace_storage(&workspace))); // same instance as long as not closed

        application_storage.init().await.unwrap();
        profile_storage.init().await.unwrap();
        workspace_storage.init().await.unwrap();

        lifecycle_main_service.fire_on_will_shutdown().await.unwrap();

        assert!(did_close_application_storage.get());
        assert!(did_close_profile_storage.get());
        assert!(did_close_workspace_storage.get());

        let profile_storage2 = storage_main_service.profile_storage(&profile);
        assert!(!Rc::ptr_eq(&profile_storage, &profile_storage2));

        let workspace_storage2 = storage_main_service.workspace_storage(&workspace);
        assert!(!Rc::ptr_eq(&workspace_storage, &workspace_storage2));

        profile_storage2.close().await.unwrap();
        workspace_storage2.close().await.unwrap();
    });
}

#[test]
fn storage_closed_before_init_works() {
    run(|suite| async move {
        let storage_main_service = suite.create_storage_service(TestLifecycleMainService::new());
        let profile = in_memory_profile();
        let workspace = workspace();

        let workspace_storage = storage_main_service.workspace_storage(&workspace);
        let did_close_workspace_storage = Rc::new(Cell::new(false));
        let did_close = did_close_workspace_storage.clone();
        suite.disposables.add(workspace_storage.on_did_close_storage().listen(move |_| did_close.set(true)));

        let profile_storage = storage_main_service.profile_storage(&profile);
        let did_close_profile_storage = Rc::new(Cell::new(false));
        let did_close = did_close_profile_storage.clone();
        suite.disposables.add(profile_storage.on_did_close_storage().listen(move |_| did_close.set(true)));

        let application_storage = storage_main_service.application_storage();
        let did_close_application_storage = Rc::new(Cell::new(false));
        let did_close = did_close_application_storage.clone();
        suite.disposables.add(application_storage.on_did_close_storage().listen(move |_| did_close.set(true)));

        application_storage.close().await.unwrap();
        profile_storage.close().await.unwrap();
        workspace_storage.close().await.unwrap();

        assert!(did_close_application_storage.get());
        assert!(did_close_profile_storage.get());
        assert!(did_close_workspace_storage.get());
    });
}

#[test]
fn storage_closed_before_init_awaits_works() {
    run(|suite| async move {
        let storage_main_service = suite.create_storage_service(TestLifecycleMainService::new());
        let profile = in_memory_profile();
        let workspace = workspace();

        let workspace_storage = storage_main_service.workspace_storage(&workspace);
        let did_close_workspace_storage = Rc::new(Cell::new(false));
        let did_close = did_close_workspace_storage.clone();
        suite.disposables.add(workspace_storage.on_did_close_storage().listen(move |_| did_close.set(true)));

        let profile_storage = storage_main_service.profile_storage(&profile);
        let did_close_profile_storage = Rc::new(Cell::new(false));
        let did_close = did_close_profile_storage.clone();
        suite.disposables.add(profile_storage.on_did_close_storage().listen(move |_| did_close.set(true)));

        let applicationtorage = storage_main_service.application_storage();
        let did_close_application_storage = Rc::new(Cell::new(false));
        let did_close = did_close_application_storage.clone();
        suite.disposables.add(applicationtorage.on_did_close_storage().listen(move |_| did_close.set(true)));

        let _ = applicationtorage.init();
        let _ = profile_storage.init();
        let _ = workspace_storage.init();

        applicationtorage.close().await.unwrap();
        profile_storage.close().await.unwrap();
        workspace_storage.close().await.unwrap();

        assert!(did_close_application_storage.get());
        assert!(did_close_profile_storage.get());
        assert!(did_close_workspace_storage.get());
    });
}

#[test]
fn application_shared_storage_closed_on_will_shutdown() {
    run(|suite| async move {
        let lifecycle_main_service = TestLifecycleMainService::new();
        let storage_main_service = suite.create_storage_service(lifecycle_main_service.clone());

        let application_shared_storage = storage_main_service.application_shared_storage();
        let did_close_application_shared_storage = Rc::new(Cell::new(false));
        let did_close = did_close_application_shared_storage.clone();
        suite.disposables.add(application_shared_storage.on_did_close_storage().listen(move |_| did_close.set(true)));

        application_shared_storage.init().await.unwrap();
        lifecycle_main_service.fire_on_will_shutdown().await.unwrap();

        assert!(did_close_application_shared_storage.get());
    });
}
