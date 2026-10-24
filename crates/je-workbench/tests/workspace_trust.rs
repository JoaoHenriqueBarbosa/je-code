//! Porte de `src/vs/workbench/services/workspaces/test/common/workspaceTrust.test.ts` (suíte `Workspace Trust`), caso a caso e na mesma ordem.
//!
//! Notas de tradução:
//! - `ensureNoDisposablesAreLeakedInTestSuite()`, `setup` e `teardown`. O guarda de vazamentos é criado antes do
//!   [`Fixture`] (o `setup`); o `Drop` do [`Fixture`] descarta o `store` e faz o `Memento.clear(StorageScope.WORKSPACE)`
//!   do `teardown` da suíte `Management` (os mementos são por thread, e cada caso roda na sua). Todo caso roda dentro de
//!   `run_with_faked_timers`.
//! - `TestInstantiationService` (`stub`, `createInstance`) é o [`Fixture`]: cada caso monta os serviços com as partes que o
//!   `setup` registra. O `environmentService` (`{} as IWorkbenchEnvironmentService`, com `disableWorkspaceTrust`,
//!   `remoteAuthority` e `filesToOpenOrCreate` atribuídos pelos casos) é o `TestEnvironmentService` com
//!   [`EnvironmentOverrides`], montado inteiro antes de o serviço ser criado (o `stub` com `{ ...environmentService }`).
//! - `IRemoteAuthorityResolverService` (`new class extends mock<...>() { }`) é o [`MockResolver`], que não implementa
//!   nenhuma operação (chamar uma é falha, como o `mock` do original); o resolvedor do caso remoto é o [`FakeResolver`].
//! - `TestStorageService` é o `InMemoryStorageService`; `TestContextService` e `TestWorkspaceTrustEnablementService` são os
//!   dublês de `test/common/workbench_test_services.rs`; `new Workspace(id, folders)` é `new_test_workspace`.
//! - `JSON.stringify(trustInfo)` do caso `empty workspace - trusted, open trusted file` é o texto que o `toJSON` do URI
//!   produz para `URI.parse('file:///Folder')` (`$mid`, `path` e `scheme`).
//! - `assert.deepStrictEqual({ ... }, { ... })` entre objetos é a comparação das tuplas, na ordem dos campos.

#[path = "../../je-platform/tests/common/mod.rs"]
mod common;

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use common::{ensure_no_disposables_are_leaked_in_test_suite, j, null_log, run_with_faked_timers};
use je_base::JsString;
use je_base::async_::async_fn;
use je_base::lifecycle::{DisposableStore, IDisposable};
use je_base::network::schemas;
use je_base::promise::Promise;
use je_base::uri::{Field, URI, UriChange, UriComponents};
use je_platform::agent_host::common::agent_host_uri::{AGENT_HOST_SCHEME, to_agent_host_uri};
use je_platform::configuration::service::IConfigurationService;
use je_platform::configuration::test_configuration_service::TestConfigurationService;
use je_platform::files::file_service::FileService;
use je_platform::files::i_file_service::IFileService;
use je_platform::files::in_memory::InMemoryFileSystemProvider;
use je_platform::remote::remote_authority_resolver::{
    IRemoteAuthorityResolverService, RemoteConnection, ResolvedAuthority, ResolverResult, WebSocketRemoteConnection,
};
use je_platform::resources::{ext_uri, uri_file};
use je_platform::storage::{InMemoryStorageService, StorageScope, StorageService, StorageTarget};
use je_platform::uri_identity::uri_identity::IUriIdentityService;
use je_platform::uri_identity::uri_identity_service::UriIdentityService;
use je_platform::window::IPath;
use je_platform::workspace::{IWorkspaceContextService, new_test_workspace, to_workspace_folder};
use je_platform::workspace_trust::{
    IWorkspaceTrustEnablementService, IWorkspaceTrustManagementService, IWorkspaceTrustRequestService, IWorkspaceTrustTransitionParticipant, ResourceTrustRequestOptions,
};
use je_instantiation::Injectable;
use je_storage::StorageValue;
use je_workbench::common::memento::Memento;
use je_workbench::services::workspaces::common::workspace_trust::{
    WORKSPACE_TRUST_STORAGE_KEY, WorkspaceTrustEnablementService, WorkspaceTrustEnablementServiceParts, WorkspaceTrustManagementService,
    WorkspaceTrustManagementServiceParts, WorkspaceTrustRequestService,
};
use je_workbench::test::browser::workbench_test_services::{EnvironmentOverrides, TestEnvironmentService};
use je_workbench::test::common::workbench_test_services::{TestContextService, TestWorkspaceTrustEnablementService};

/// `new class extends mock<IRemoteAuthorityResolverService>() { }`: nenhuma operação implementada.
struct MockResolver;

impl IRemoteAuthorityResolverService for MockResolver {
    fn resolve_authority(&self, _authority: &str) -> Promise<ResolverResult> {
        panic!("IRemoteAuthorityResolverService.resolveAuthority não implementado (mock)");
    }

    fn get_canonical_uri(&self, _uri: &URI) -> Promise<URI> {
        panic!("IRemoteAuthorityResolverService.getCanonicalURI não implementado (mock)");
    }
}

/// O resolvedor do caso remoto: `getCanonicalURI: async uri => uri` e `resolveAuthority` com `WebSocketRemoteConnection('localhost', 0)`.
struct FakeResolver;

impl IRemoteAuthorityResolverService for FakeResolver {
    fn resolve_authority(&self, authority: &str) -> Promise<ResolverResult> {
        Promise::resolved(ResolverResult {
            authority: ResolvedAuthority {
                authority: authority.to_owned(),
                connect_to: RemoteConnection::WebSocket(WebSocketRemoteConnection::new("localhost", 0)),
                connection_token: None,
            },
            options: None,
        })
    }

    fn get_canonical_uri(&self, uri: &URI) -> Promise<URI> {
        Promise::resolved(uri.clone())
    }
}

/// `getUserSettings(enabled, emptyWindow)`: `{ workspace: { trust: { emptyWindow, enabled } } }`.
fn user_settings(enabled: bool, empty_window: bool) -> je_base::dynamic::Dynamic {
    j(&format!("{{ 'workspace': {{ 'trust': {{ 'emptyWindow': {empty_window}, 'enabled': {enabled} }} }} }}"))
}

/// O `setup` da suíte e o que os casos usam dele.
struct Fixture {
    store: Rc<DisposableStore>,
    configuration_service: Rc<TestConfigurationService>,
    file_service: Rc<FileService>,
    uri_identity_service: Rc<dyn IUriIdentityService>,
    storage_service: Rc<dyn StorageService>,
    workspace_service: Rc<TestContextService>,
}

impl Drop for Fixture {
    /// O `teardown`: `Memento.clear(StorageScope.WORKSPACE)` e o descarte do `store`.
    fn drop(&mut self) {
        Memento::clear(StorageScope::Workspace);
        self.store.dispose();
    }
}

impl Fixture {
    fn new() -> Fixture {
        let store = DisposableStore::new();
        let configuration_service = TestConfigurationService::new(None);
        let file_service = store.add_rc(FileService::new(null_log()));
        let file_service_contract: Rc<dyn IFileService> = file_service.clone();
        let uri_identity_concrete = store.add_rc(UriIdentityService::create((), (file_service_contract,)));
        let uri_identity_service: Rc<dyn IUriIdentityService> = uri_identity_concrete;
        let storage = store.add_rc(InMemoryStorageService::new());
        let storage_service: Rc<dyn StorageService> = Rc::new(storage);
        let workspace_service = TestContextService::new(None, None);
        Fixture { store, configuration_service, file_service, uri_identity_service, storage_service, workspace_service }
    }

    /// `await configurationService.setUserConfiguration('security', getUserSettings(enabled, emptyWindow))`.
    async fn set_security(&self, enabled: bool, empty_window: bool) {
        self.configuration_service.set_user_configuration("security", user_settings(enabled, empty_window), None).await.unwrap();
    }

    fn environment(&self, overrides: EnvironmentOverrides) -> Rc<TestEnvironmentService> {
        TestEnvironmentService::with_overrides(overrides)
    }

    fn enablement_service(&self, overrides: EnvironmentOverrides) -> Rc<WorkspaceTrustEnablementService> {
        let configuration_service: Rc<dyn IConfigurationService> = self.configuration_service.clone();
        self.store.add_rc(WorkspaceTrustEnablementService::new(WorkspaceTrustEnablementServiceParts { configuration_service, environment_service: self.environment(overrides) }))
    }

    /// `store.add(instantiationService.createInstance(WorkspaceTrustManagementService))`.
    fn management_service(&self, overrides: EnvironmentOverrides, resolver: Rc<dyn IRemoteAuthorityResolverService>) -> Rc<WorkspaceTrustManagementService> {
        let configuration_service: Rc<dyn IConfigurationService> = self.configuration_service.clone();
        let workspace_service: Rc<dyn IWorkspaceContextService> = self.workspace_service.clone();
        let enablement: Rc<dyn IWorkspaceTrustEnablementService> = TestWorkspaceTrustEnablementService::new(true);
        let file_service: Rc<dyn IFileService> = self.file_service.clone();
        self.store.add_rc(WorkspaceTrustManagementService::new(WorkspaceTrustManagementServiceParts {
            configuration_service,
            remote_authority_resolver_service: resolver,
            storage_service: self.storage_service.clone(),
            uri_identity_service: self.uri_identity_service.clone(),
            environment_service: self.environment(overrides),
            workspace_service,
            workspace_trust_enablement_service: enablement,
            file_service,
        }))
    }

    /// `initializeTestObject()`.
    async fn initialize_test_object(&self) -> Rc<WorkspaceTrustManagementService> {
        let service = self.management_service(EnvironmentOverrides::default(), Rc::new(MockResolver));
        service.workspace_trust_initialized().await.unwrap();
        service
    }

    /// `workspaceService.setWorkspace(new Workspace(id, folders))`.
    fn set_workspace(&self, id: &str, folders: Vec<URI>) {
        let folders = folders.iter().map(|folder| to_workspace_folder(folder, ext_uri().basename_or_authority(folder))).collect();
        self.workspace_service.set_workspace(new_test_workspace(id, folders, None, None));
    }

    /// `workspaceService.setWorkspace(new Workspace('empty-workspace'))`.
    fn set_empty_workspace(&self) {
        self.set_workspace("empty-workspace", Vec::new());
    }
}

/// O corpo comum dos casos: o guarda de vazamentos, o `setup` e o caso, no relógio virtual.
fn run<F: std::future::Future<Output = ()> + 'static>(case: impl FnOnce(Fixture) -> F + 'static) {
    run_with_faked_timers(async move {
        let _leaks = ensure_no_disposables_are_leaked_in_test_suite();
        case(Fixture::new()).await;
    });
}

/// `URI.parse(value)`.
fn parse_uri(value: &str) -> URI {
    URI::parse(value, false)
}

/// `URI.from({ scheme, authority, path, query? })`.
fn uri_from(scheme: &str, authority: &str, path: &str, query: Option<&str>) -> URI {
    URI::from(
        &UriComponents {
            scheme: Some(JsString::from(scheme)),
            authority: Some(JsString::from(authority)),
            path: Some(JsString::from(path)),
            query: query.map(JsString::from),
            fragment: None,
        },
        false,
    )
}

mod enablement {
    use super::*;

    #[test]
    fn workspace_trust_enabled() {
        run(|f| async move {
            f.set_security(true, true).await;
            let test_object = f.enablement_service(EnvironmentOverrides { disable_workspace_trust: Some(false), ..EnvironmentOverrides::default() });

            assert!(test_object.is_workspace_trust_enabled());
        });
    }

    #[test]
    fn workspace_trust_disabled_user_setting() {
        run(|f| async move {
            f.set_security(false, true).await;
            let test_object = f.enablement_service(EnvironmentOverrides { disable_workspace_trust: Some(false), ..EnvironmentOverrides::default() });

            assert!(!test_object.is_workspace_trust_enabled());
        });
    }

    #[test]
    fn workspace_trust_disabled_disable_workspace_trust_flag() {
        run(|f| async move {
            let test_object = f.enablement_service(EnvironmentOverrides { disable_workspace_trust: Some(true), ..EnvironmentOverrides::default() });

            assert!(!test_object.is_workspace_trust_enabled());
        });
    }
}

mod management {
    use super::*;

    fn defaults() -> EnvironmentOverrides {
        EnvironmentOverrides { disable_workspace_trust: Some(false), ..EnvironmentOverrides::default() }
    }

    async fn initialize(f: &Fixture) -> Rc<WorkspaceTrustManagementService> {
        let service = f.management_service(defaults(), Rc::new(MockResolver));
        service.workspace_trust_initialized().await.unwrap();
        service
    }

    #[test]
    fn empty_workspace_trusted() {
        run(|f| async move {
            f.set_security(true, true).await;
            f.set_empty_workspace();
            let test_object = initialize(&f).await;

            assert!(test_object.is_workspace_trusted());
        });
    }

    #[test]
    fn empty_workspace_untrusted() {
        run(|f| async move {
            f.set_security(true, false).await;
            f.set_empty_workspace();
            let test_object = initialize(&f).await;

            assert!(!test_object.is_workspace_trusted());
        });
    }

    #[test]
    fn empty_workspace_trusted_open_trusted_file() {
        run(|f| async move {
            f.set_security(true, true).await;
            let trust_info = "{\"uriTrustInfo\":[{\"uri\":{\"$mid\":1,\"path\":\"/Folder\",\"scheme\":\"file\"},\"trusted\":true}]}";
            f.storage_service.store(WORKSPACE_TRUST_STORAGE_KEY, StorageValue::String(trust_info.to_owned()), StorageScope::ApplicationShared, StorageTarget::Machine);

            let overrides = EnvironmentOverrides {
                files_to_open_or_create: Some(vec![IPath { file_uri: Some(parse_uri("file:///Folder/file.txt")) }]),
                ..defaults()
            };

            f.set_empty_workspace();
            let test_object = f.management_service(overrides, Rc::new(MockResolver));
            test_object.workspace_trust_initialized().await.unwrap();

            assert!(test_object.is_workspace_trusted());
        });
    }

    #[test]
    fn empty_workspace_trusted_open_untrusted_file() {
        run(|f| async move {
            f.set_security(true, true).await;

            let overrides = EnvironmentOverrides {
                files_to_open_or_create: Some(vec![IPath { file_uri: Some(parse_uri("file:///Folder/foo.txt")) }]),
                ..defaults()
            };

            f.set_empty_workspace();
            let test_object = f.management_service(overrides, Rc::new(MockResolver));
            test_object.workspace_trust_initialized().await.unwrap();

            assert!(!test_object.is_workspace_trusted());
        });
    }

    #[test]
    fn remote_workspace_does_not_inherit_trust_through_backslash_traversal() {
        run(|f| async move {
            f.set_security(true, true).await;
            let root = uri_from(schemas::VSCODE_REMOTE, "host", "/c:/lab/ws", None);
            let workspace_uri = parse_uri(&format!("{}/..%5Cevilfolder", root.to_string(false)));
            let overrides = EnvironmentOverrides { remote_authority: Some(root.authority().to_string_lossy()), ..defaults() };
            let provider = f.store.add_rc(InMemoryFileSystemProvider::new());
            f.store.add(f.file_service.register_provider(schemas::VSCODE_REMOTE, provider).unwrap());
            f.set_workspace("remote-workspace", vec![workspace_uri.clone()]);
            let test_object = f.management_service(overrides, Rc::new(FakeResolver));
            test_object.workspace_resolved().await.unwrap();
            test_object.set_uris_trust(std::slice::from_ref(&root), true).await.unwrap();

            let uris = [
                workspace_uri,
                parse_uri(&format!("{}/../evilfolder", root.to_string(false))),
                root.with(&UriChange { path: Field::from("/c:/lab/evilfolder"), ..UriChange::default() }),
                parse_uri(&format!("{}/child", root.to_string(false))),
            ];
            let mut trust_info = Vec::new();
            for uri in &uris {
                trust_info.push(test_object.get_uri_trust_info(uri).await.unwrap().trusted);
            }
            assert_eq!((test_object.is_workspace_trusted(), trust_info), (false, vec![false, false, false, true]));
        });
    }

    #[test]
    fn agent_host_folder_is_not_auto_trusted_as_a_virtual_resource() {
        run(|f| async move {
            f.set_security(true, true).await;
            f.set_empty_workspace();
            let test_object = initialize(&f).await;

            // A regular virtual resource (e.g. github1s) is auto-trusted...
            let virtual_uri = parse_uri("vscode-test-virtual://authority/folder");
            assert!(test_object.get_uri_trust_info(&virtual_uri).await.unwrap().trusted);

            // ...but an agent host folder is not, even though it is a virtual scheme.
            let agent_host_uri = uri_from(AGENT_HOST_SCHEME, "my-server", "/Users/me/code", Some("_ah=meta"));
            assert!(!test_object.get_uri_trust_info(&agent_host_uri).await.unwrap().trusted);
        });
    }

    #[test]
    fn agent_host_folder_trust_persists_and_ignores_the_ah_query() {
        run(|f| async move {
            f.set_security(true, true).await;
            f.set_empty_workspace();
            let test_object = initialize(&f).await;

            let agent_host_uri = uri_from(AGENT_HOST_SCHEME, "my-server", "/Users/me/code", Some("_ah=meta"));
            test_object.set_uris_trust(std::slice::from_ref(&agent_host_uri), true).await.unwrap();

            // The same folder with a different _ah payload resolves to the same trust entry.
            let same_folder_different_meta = uri_from(AGENT_HOST_SCHEME, "my-server", "/Users/me/code", Some("_ah=other"));
            assert!(test_object.get_uri_trust_info(&same_folder_different_meta).await.unwrap().trusted);
        });
    }

    /// `for (const authority of ['managed-host', 'host_with_underscore', 'Host42'])`.
    fn registered_authorities_are_automatically_trusted_without_persistence(authority: &'static str) {
        run(move |f| async move {
            f.set_security(true, true).await;
            let folder = to_agent_host_uri(&uri_file("/workspaces/repo"), authority);
            f.set_workspace("managed-workspace", vec![folder.clone()]);
            let test_object = f.management_service(defaults(), Rc::new(MockResolver));
            f.store.add(test_object.register_trusted_authority(AGENT_HOST_SCHEME, authority));
            test_object.workspace_trust_initialized().await.unwrap();
            let management: Rc<dyn IWorkspaceTrustManagementService> = test_object.clone();
            let configuration_service: Rc<dyn IConfigurationService> = f.configuration_service.clone();
            let requests = f.store.add_rc(WorkspaceTrustRequestService::new(configuration_service, management));

            test_object.set_uris_trust(std::slice::from_ref(&folder), true).await.unwrap();
            test_object.set_uris_trust(std::slice::from_ref(&folder), false).await.unwrap();
            let other_repository = to_agent_host_uri(&uri_file("/workspaces/other-repo"), authority);
            let actual = (
                test_object.is_workspace_trusted(),
                test_object.is_workspace_trust_forced(),
                test_object.can_set_workspace_trust(),
                test_object.get_uri_trust_info(&folder).await.unwrap().trusted,
                test_object.get_uri_trust_info(&other_repository).await.unwrap().trusted,
                requests.request_resources_trust(ResourceTrustRequestOptions { uri: other_repository.clone(), message: None }).await.unwrap(),
                requests.request_workspace_trust(None).await.unwrap(),
                test_object.get_trusted_uris(),
                f.storage_service.get(WORKSPACE_TRUST_STORAGE_KEY, StorageScope::ApplicationShared, None),
            );
            assert_eq!(actual, (true, true, false, true, true, Some(true), Some(true), Vec::new(), None));
        });
    }

    #[test]
    fn registered_authorities_are_automatically_trusted_without_persistence_managed_host() {
        registered_authorities_are_automatically_trusted_without_persistence("managed-host");
    }

    #[test]
    fn registered_authorities_are_automatically_trusted_without_persistence_host_with_underscore() {
        registered_authorities_are_automatically_trusted_without_persistence("host_with_underscore");
    }

    #[test]
    fn registered_authorities_are_automatically_trusted_without_persistence_host42() {
        registered_authorities_are_automatically_trusted_without_persistence("Host42");
    }

    #[test]
    fn registered_authority_trust_does_not_extend_to_other_schemes_or_authorities() {
        run(|f| async move {
            f.set_security(true, true).await;
            let managed = to_agent_host_uri(&uri_file("/workspaces/repo"), "managed-host");
            let local = uri_file("/workspaces/repo");
            f.set_workspace("mixed-workspace", vec![managed.clone(), local.clone()]);
            let test_object = f.management_service(defaults(), Rc::new(MockResolver));
            f.store.add(test_object.register_trusted_authority(AGENT_HOST_SCHEME, &managed.authority().to_string_lossy()));
            test_object.workspace_trust_initialized().await.unwrap();
            let authorities = ["managed-host-other", "managed", "unmanaged-host", "local"];
            let mut other_hosts = Vec::new();
            for authority in authorities {
                other_hosts.push(test_object.get_uri_trust_info(&to_agent_host_uri(&local, authority)).await.unwrap().trusted);
            }
            let wrong_scheme = test_object.get_uri_trust_info(&managed.with(&UriChange { scheme: Field::from("file"), ..UriChange::default() })).await.unwrap();

            let actual = (
                test_object.is_workspace_trusted(),
                test_object.is_workspace_trust_forced(),
                test_object.can_set_workspace_trust(),
                test_object.get_uri_trust_info(&managed).await.unwrap().trusted,
                other_hosts,
                wrong_scheme.trusted,
            );
            assert_eq!(actual, (false, false, true, true, vec![false; authorities.len()], false));
        });
    }

    #[test]
    fn authority_registration_updates_workspace_trust_and_lasts_until_the_last_registration_is_disposed() {
        run(|f| async move {
            f.set_security(true, true).await;
            let folder = to_agent_host_uri(&uri_file("/workspaces/repo"), "managed-host");
            f.set_workspace("managed-workspace", vec![folder.clone()]);
            let test_object = initialize(&f).await;
            let changes = Rc::new(RefCell::new(Vec::<bool>::new()));
            let sink = changes.clone();
            test_object.on_did_change_trust().listen_in(move |trusted| sink.borrow_mut().push(*trusted), &f.store);
            let trusted = test_object.on_did_change_trust().to_promise(None);
            let first = f.store.add(test_object.register_trusted_authority(AGENT_HOST_SCHEME, &folder.authority().to_string_lossy()));
            trusted.await.unwrap();
            let second = f.store.add(test_object.register_trusted_authority(AGENT_HOST_SCHEME, &folder.authority().to_string_lossy().to_uppercase()));
            first.dispose();
            let still_trusted = test_object.get_uri_trust_info(&folder).await.unwrap().trusted;
            let untrusted = test_object.on_did_change_trust().to_promise(None);
            second.dispose();
            untrusted.await.unwrap();

            let actual = (
                changes.borrow().clone(),
                still_trusted,
                test_object.get_uri_trust_info(&folder).await.unwrap().trusted,
                test_object.is_workspace_trusted(),
                test_object.is_workspace_trust_forced(),
                test_object.can_set_workspace_trust(),
                test_object.get_trusted_uris(),
            );
            assert_eq!(actual, (vec![true, false], true, false, false, false, true, Vec::new()));
        });
    }

    /// `ParticipantState`: o que o participante do caso `setWorkspaceTrust waits ...` registra.
    struct Participant {
        started: Rc<Cell<bool>>,
        completed: Rc<Cell<bool>>,
        started_resolve: je_base::promise::Resolver<()>,
        can_complete: Promise<()>,
        completed_resolve: je_base::promise::Resolver<()>,
    }

    impl IWorkspaceTrustTransitionParticipant for Participant {
        fn participate(&self, trusted: bool) -> Promise<()> {
            let (started, completed) = (self.started.clone(), self.completed.clone());
            let (started_resolve, can_complete, completed_resolve) = (self.started_resolve.clone(), self.can_complete.clone(), self.completed_resolve.clone());
            async_fn(async move {
                if trusted {
                    started.set(true);
                    started_resolve.resolve(());
                    can_complete.await?;
                    completed.set(true);
                    completed_resolve.resolve(());
                }
                Ok(())
            })
        }
    }

    #[test]
    fn set_workspace_trust_waits_for_trust_transition_participants_before_resolving() {
        run(|f| async move {
            f.set_security(true, true).await;
            f.set_workspace("folder-workspace", vec![parse_uri("file:///Folder")]);
            let test_object = initialize(&f).await;

            let (participant_can_complete, release_participant) = Promise::<()>::new();
            let (participant_started, participant_started_resolve) = Promise::<()>::new();
            let (participant_completed_promise, participant_completed_resolve) = Promise::<()>::new();

            let participant_started_flag = Rc::new(Cell::new(false));
            let participant_completed = Rc::new(Cell::new(false));
            let trust_change_event_fired = Rc::new(Cell::new(false));

            f.store.add(test_object.add_workspace_trust_transition_participant(Rc::new(Participant {
                started: participant_started_flag.clone(),
                completed: participant_completed.clone(),
                started_resolve: participant_started_resolve,
                can_complete: participant_can_complete,
                completed_resolve: participant_completed_resolve,
            })));

            let fired = trust_change_event_fired.clone();
            test_object.on_did_change_trust().listen_in(
                move |trusted| {
                    if *trusted {
                        fired.set(true);
                    }
                },
                &f.store,
            );

            test_object.set_workspace_trust(false).await.unwrap();
            assert_eq!(
                (test_object.is_workspace_trusted(), participant_started_flag.get(), participant_completed.get(), trust_change_event_fired.get()),
                (false, false, false, false)
            );

            let set_workspace_trust_promise = test_object.set_workspace_trust(true);
            let set_workspace_trust_resolved = Rc::new(Cell::new(false));
            let resolved = set_workspace_trust_resolved.clone();
            drop(set_workspace_trust_promise.then(move |()| resolved.set(true)));

            participant_started.await.unwrap();
            Promise::resolved(()).await.unwrap();

            assert_eq!(
                (
                    set_workspace_trust_resolved.get(),
                    test_object.is_workspace_trusted(),
                    participant_started_flag.get(),
                    participant_completed.get(),
                    trust_change_event_fired.get()
                ),
                (false, true, true, false, false)
            );

            release_participant.resolve(());
            participant_completed_promise.await.unwrap();

            set_workspace_trust_promise.await.unwrap();
            Promise::resolved(()).await.unwrap();

            assert_eq!(
                (set_workspace_trust_resolved.get(), test_object.is_workspace_trusted(), participant_completed.get(), trust_change_event_fired.get()),
                (true, true, true, true)
            );
        });
    }
}
