//! Porte de `src/vs/workbench/services/userDataProfile/test/browser/snippetsResource.test.ts` (suíte `SnippetsResource`),
//! caso a caso e na mesma ordem.
//!
//! Divergências:
//! - **`TestInstantiationService`.** `new TestInstantiationService()` com `stub(...)` é um `InstantiationService` sobre
//!   um `ServiceCollection` com os serviços (`fileService`, `logService`, `uriIdentityService`); o `stub` posterior de
//!   `IUserDataProfileService` (casos `initialize`) é um segundo `InstantiationService` com o serviço a mais, feito por
//!   [`Setup::instantiation_service`]. O `UserDataProfileService` é o real, com o `currentProfile` do `{ currentProfile:
//!   targetProfile }` do original.
//! - **`sinon.spy(fileService, 'writeFile')`.** É o [`SpyFileService`], que repassa todos os métodos ao `FileService`
//!   real e guarda o recurso de cada `write_file`. O `teardown` (`sinon.restore()`) não tem o que restaurar: o espião não
//!   altera o serviço original.
//! - **`{ snippetsHome } as IUserDataProfile`.** O `UserDataProfile` do porte não tem campos ausentes: os demais
//!   recursos recebem o próprio `snippetsHome`, que os casos não leem.
//! - **`UriIdentityService`.** É o real, criado sobre o espião (o original o cria sobre o `FileService`).
//! - **`await` e `ensureNoDisposablesAreLeakedInTestSuite()`.** Os casos rodam em `run_with_faked_timers`, com o guarda e o
//!   `setup` dentro do corpo; o `teardown` e a conferência de vazamentos rodam no `Drop` do guarda.
//! - **Laço `for (const scheme ...) for (const operation ...)`.** Cada combinação é um `#[test]`, na ordem do laço
//!   (`vscode-tests` com `apply` e `initialize`, depois `vscode-userdata` com `apply` e `initialize`).

#[path = "../../je-base/tests/common/mod.rs"]
mod common;

use std::cell::RefCell;
use std::future::Future;
use std::rc::Rc;

use common::{ensure_no_disposables_are_leaked_in_test_suite, run_with_faked_timers};
use je_base::cancellation::CancellationToken;
use je_base::errors::Error;
use je_base::event::Event;
use je_base::lifecycle::{DisposableRef, DisposableStore};
use je_base::network::schemas;
use je_base::promise::Promise;
use je_base::uri::{Field, URI, UriChange};
use je_instantiation::{InstantiationService, Injectable, ServiceCollection};
use je_platform::files::file_service::{FileContentSource, FileService};
use je_platform::files::files::{
    CreateFileOptions, FileChangesEvent, FileContent, FileOperationEvent, FileStat, FileStatWithMetadata, FileStreamContent, FileSystemProviderActivationEvent,
    FileSystemProviderCapabilities, FileSystemProviderCapabilitiesChangeEvent, FileSystemProviderRef, FileSystemProviderRegistrationEvent,
    PartialFileDeleteOptions, ReadFileOptions, ResolveFileOptions, WatchOptions, WriteFileOptions,
};
use je_platform::files::i_file_service::{I_FILE_SERVICE, IFileService};
use je_platform::files::in_memory::InMemoryFileSystemProvider;
use je_platform::log::{I_LOG_SERVICE, LogService, NullLogService};
use je_platform::resources::{join_path, uri_file};
use je_platform::uri_identity::uri_identity::{I_URI_IDENTITY_SERVICE, IUriIdentityService};
use je_platform::uri_identity::uri_identity_service::UriIdentityService;
use je_platform::user_data_profile::user_data_profile::UserDataProfile;
use je_workbench::services::user_data_profile::snippets_resource::{SnippetsResource, SnippetsResourceInitializer};
use je_workbench::services::user_data_profile::user_data_profile::{
    I_USER_DATA_PROFILE_SERVICE, IProfileResource, IProfileResourceInitializer, IUserDataProfileService,
};
use je_workbench::services::user_data_profile::user_data_profile_service::UserDataProfileService;
use serde_json::json;

/// Um teste `async` da suíte: o `ensureNoDisposablesAreLeakedInTestSuite` e o `runWithFakedTimers`, com o store do
/// `disposables` entregue ao corpo.
fn async_test<F: Future<Output = ()> + 'static>(body: impl FnOnce(Rc<DisposableStore>) -> F) {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    run_with_faked_timers(body(guard.store()));
}

fn root() -> URI {
    uri_file("tests").with(&UriChange { scheme: Field::Set("vscode-tests".into()), ..Default::default() })
}

fn text(uri: &URI) -> String {
    uri.to_string(false).to_string_lossy()
}

/// `sinon.spy(fileService, 'writeFile')`: repassa tudo ao `FileService` e guarda o recurso de cada `write_file`.
struct SpyFileService {
    inner: Rc<FileService>,
    written: RefCell<Vec<URI>>,
}

impl IFileService for SpyFileService {
    fn on_did_files_change(&self) -> Event<Rc<FileChangesEvent>> {
        IFileService::on_did_files_change(&*self.inner)
    }

    fn get_provider(&self, scheme: &str) -> Option<FileSystemProviderRef> {
        IFileService::get_provider(&*self.inner, scheme)
    }

    fn resolve_with_metadata(&self, resource: &URI, options: Option<ResolveFileOptions>) -> Promise<FileStatWithMetadata> {
        IFileService::resolve_with_metadata(&*self.inner, resource, options)
    }

    fn exists(&self, resource: &URI) -> Promise<bool> {
        IFileService::exists(&*self.inner, resource)
    }

    fn on_did_change_file_system_provider_registrations(&self) -> Event<FileSystemProviderRegistrationEvent> {
        IFileService::on_did_change_file_system_provider_registrations(&*self.inner)
    }

    fn on_did_change_file_system_provider_capabilities(&self) -> Event<FileSystemProviderCapabilitiesChangeEvent> {
        IFileService::on_did_change_file_system_provider_capabilities(&*self.inner)
    }

    fn has_provider(&self, resource: &URI) -> bool {
        IFileService::has_provider(&*self.inner, resource)
    }

    fn has_capability(&self, resource: &URI, capability: FileSystemProviderCapabilities) -> bool {
        IFileService::has_capability(&*self.inner, resource, capability)
    }

    fn read_file(&self, resource: &URI, options: Option<ReadFileOptions>, token: Option<CancellationToken>) -> Promise<FileContent> {
        IFileService::read_file(&*self.inner, resource, options, token)
    }

    fn resolve(&self, resource: &URI, options: Option<ResolveFileOptions>) -> Promise<FileStat> {
        IFileService::resolve(&*self.inner, resource, options)
    }

    fn create_folder(&self, resource: &URI) -> Promise<FileStatWithMetadata> {
        IFileService::create_folder(&*self.inner, resource)
    }

    fn watch(&self, resource: &URI, options: Option<WatchOptions>) -> DisposableRef {
        IFileService::watch(&*self.inner, resource, options)
    }

    fn read_file_stream(&self, resource: &URI, options: Option<ReadFileOptions>, token: Option<CancellationToken>) -> Promise<FileStreamContent> {
        IFileService::read_file_stream(&*self.inner, resource, options, token)
    }

    fn write_file(&self, resource: &URI, content: FileContentSource, options: Option<WriteFileOptions>) -> Promise<FileStatWithMetadata> {
        self.written.borrow_mut().push(resource.clone());
        IFileService::write_file(&*self.inner, resource, content, options)
    }

    fn del(&self, resource: &URI, options: Option<PartialFileDeleteOptions>) -> Promise<()> {
        IFileService::del(&*self.inner, resource, options)
    }

    fn on_did_run_operation(&self) -> Event<FileOperationEvent> {
        IFileService::on_did_run_operation(&*self.inner)
    }

    fn create_file(&self, resource: &URI, content: Option<FileContentSource>, options: Option<CreateFileOptions>) -> Promise<FileStatWithMetadata> {
        IFileService::create_file(&*self.inner, resource, content, options)
    }

    fn on_will_activate_file_system_provider(&self) -> Event<FileSystemProviderActivationEvent> {
        IFileService::on_will_activate_file_system_provider(&*self.inner)
    }

    fn register_provider(&self, scheme: &str, provider: FileSystemProviderRef) -> Result<DisposableRef, Error> {
        IFileService::register_provider(&*self.inner, scheme, provider)
    }
}

struct Setup {
    disposables: Rc<DisposableStore>,
    file_service: Rc<SpyFileService>,
    log_service: Rc<dyn LogService>,
    uri_identity_service: Rc<dyn IUriIdentityService>,
    snippets_home: URI,
    profile: UserDataProfile,
}

/// Um perfil em que todo recurso é o `snippetsHome` (o original usa `{ snippetsHome } as IUserDataProfile`).
fn profile_at(snippets_home: &URI) -> UserDataProfile {
    UserDataProfile {
        id: "test".to_owned(),
        is_default: false,
        name: "test".to_owned(),
        icon: None,
        location: snippets_home.clone(),
        global_storage_home: snippets_home.clone(),
        settings_resource: snippets_home.clone(),
        keybindings_resource: snippets_home.clone(),
        tasks_resource: snippets_home.clone(),
        snippets_home: snippets_home.clone(),
        prompts_home: snippets_home.clone(),
        extensions_resource: snippets_home.clone(),
        mcp_resource: snippets_home.clone(),
        language_models_resource: snippets_home.clone(),
        agent_plugins_home: snippets_home.clone(),
        cache_home: snippets_home.clone(),
        use_default_flags: None,
        is_internal: None,
        is_transient: None,
        is_agents_window_profile: None,
        workspaces: None,
    }
}

fn setup(disposables: &Rc<DisposableStore>) -> Setup {
    let log_service = NullLogService::shared();
    let inner = disposables.add_rc(FileService::new(log_service.clone()));
    let file_system_provider = disposables.add_rc(InMemoryFileSystemProvider::new());
    let root = root();
    disposables.add(inner.register_provider(&root.scheme().to_string_lossy(), file_system_provider).unwrap());

    let file_service = Rc::new(SpyFileService { inner, written: RefCell::new(Vec::new()) });
    let file_service_dyn: Rc<dyn IFileService> = file_service.clone();
    let uri_identity_service = disposables.add_rc(UriIdentityService::create((), (file_service_dyn,)));

    let snippets_home = join_path(&root, &["User", "profiles", "test", "snippets"]);
    let profile = profile_at(&snippets_home);
    Setup { disposables: disposables.clone(), file_service, log_service, uri_identity_service, snippets_home, profile }
}

impl Setup {
    /// O `instantiationService` com os três `stub` do `setup`, mais o `IUserDataProfileService` quando o caso o dá.
    fn instantiation_service(&self, current_profile: Option<&UserDataProfile>) -> Rc<InstantiationService> {
        let file_service: Rc<dyn IFileService> = self.file_service.clone();
        let mut services = ServiceCollection::new()
            .with(I_FILE_SERVICE, file_service)
            .with(I_LOG_SERVICE, self.log_service.clone())
            .with(I_URI_IDENTITY_SERVICE, self.uri_identity_service.clone());
        if let Some(profile) = current_profile {
            let concrete = self.disposables.add_rc(UserDataProfileService::create(profile.clone(), ()));
            let profile_service: Rc<dyn IUserDataProfileService> = concrete;
            services = services.with(I_USER_DATA_PROFILE_SERVICE, profile_service);
        }
        self.disposables.add_rc(InstantiationService::new(services))
    }

    async fn read_text(&self, resource: &URI) -> String {
        IFileService::read_file(&*self.file_service, resource, None, None).await.unwrap().value.to_string()
    }

    async fn exists(&self, resource: &URI) -> bool {
        IFileService::exists(&*self.file_service, resource).await.unwrap()
    }

    fn written(&self) -> Vec<String> {
        self.file_service.written.borrow().iter().map(text).collect()
    }
}

#[test]
fn apply_writes_in_bounds_snippet_keys() {
    async_test(|disposables| async move {
        let setup = setup(&disposables);
        let test_object = setup.instantiation_service(None).create_instance::<SnippetsResource>(());

        test_object.apply(&json!({ "snippets": { "javascript.json": "{}" } }).to_string(), &setup.profile).await.unwrap();

        assert_eq!(setup.read_text(&join_path(&setup.snippets_home, &["javascript.json"])).await, "{}");
    });
}

#[test]
fn apply_ignores_path_traversal_keys_posix() {
    async_test(|disposables| async move {
        let setup = setup(&disposables);
        let test_object = setup.instantiation_service(None).create_instance::<SnippetsResource>(());
        let escaped = join_path(&setup.snippets_home, &["..", "..", "..", "..", "tmp", "PROBE_snippets"]);

        test_object.apply(&json!({ "snippets": { "../../../../tmp/PROBE_snippets": "ok" } }).to_string(), &setup.profile).await.unwrap();

        assert!(!setup.exists(&escaped).await);
    });
}

/// O corpo do laço `for (const scheme ...) for (const operation ...)`: `operation` é `apply` ou `initialize`.
fn windows_style_traversal_keys_are_ignored_before_writing(scheme: &'static str, operation: &'static str) {
    async_test(|disposables| async move {
        let setup = setup(&disposables);
        if scheme != root().scheme().to_string_lossy() {
            let provider = disposables.add_rc(InMemoryFileSystemProvider::new());
            disposables.add(setup.file_service.register_provider(scheme, provider).unwrap());
        }
        let target_profile = UserDataProfile {
            snippets_home: setup.snippets_home.with(&UriChange { scheme: Field::Set(scheme.into()), ..Default::default() }),
            ..setup.profile.clone()
        };
        let content = json!({
            "snippets": {
                "javascript.json": "{}",
                "../../../../outside.json": "ignored",
                "..\\..\\..\\..\\outside.json": "ignored",
                "..\\..\\..\\..\\..\\nested\\outside.json": "ignored",
                "..\\\\..\\\\..\\\\..\\\\outside.json": "ignored",
                "..\\..\\..\\..\\tmp\\PROBE_snippets": "ignored",
                ("..\\".repeat(12) + "outside.json"): "ignored",
            }
        })
        .to_string();

        if operation == "apply" {
            setup.instantiation_service(None).create_instance::<SnippetsResource>(()).apply(&content, &target_profile).await.unwrap();
        } else {
            let instantiation_service = setup.instantiation_service(Some(&target_profile));
            instantiation_service.create_instance::<SnippetsResourceInitializer>(()).initialize(&content).await.unwrap();
        }

        assert_eq!(setup.written(), vec![text(&join_path(&target_profile.snippets_home, &["javascript.json"]))]);
    });
}

#[test]
fn apply_ignores_windows_style_path_traversal_keys_before_writing_vscode_tests() {
    windows_style_traversal_keys_are_ignored_before_writing("vscode-tests", "apply");
}

#[test]
fn initialize_ignores_windows_style_path_traversal_keys_before_writing_vscode_tests() {
    windows_style_traversal_keys_are_ignored_before_writing("vscode-tests", "initialize");
}

#[test]
fn apply_ignores_windows_style_path_traversal_keys_before_writing_vscode_userdata() {
    windows_style_traversal_keys_are_ignored_before_writing(schemas::VSCODE_USER_DATA, "apply");
}

#[test]
fn initialize_ignores_windows_style_path_traversal_keys_before_writing_vscode_userdata() {
    windows_style_traversal_keys_are_ignored_before_writing(schemas::VSCODE_USER_DATA, "initialize");
}

#[test]
fn apply_writes_in_bounds_key_even_alongside_a_traversal_key() {
    async_test(|disposables| async move {
        let setup = setup(&disposables);
        let test_object = setup.instantiation_service(None).create_instance::<SnippetsResource>(());
        let escaped = join_path(&setup.snippets_home, &["..", "..", "tmp", "PROBE_snippets"]);

        test_object
            .apply(&json!({ "snippets": { "javascript.json": "{}", "../../tmp/PROBE_snippets": "ok" } }).to_string(), &setup.profile)
            .await
            .unwrap();

        assert_eq!(setup.read_text(&join_path(&setup.snippets_home, &["javascript.json"])).await, "{}");
        assert!(!setup.exists(&escaped).await);
    });
}
