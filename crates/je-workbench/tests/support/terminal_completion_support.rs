//! Infraestrutura de teste de `src/vs/workbench/contrib/terminalContrib/suggest/test/browser/terminalCompletionService.test.ts`:
//! o `setup` da suíte (o `TerminalCompletionService` com o `IFileService` em memória, o `TestConfigurationService`, o
//! `ILabelService` do `workbenchInstantiationService` e o `TerminalCapabilityStore`) e o provedor de completions de mentira
//! da suíte `Provider Configuration`.
//!
//! Divergências:
//! - **`workbenchInstantiationService` e `instantiationService.stub(...)`.** O contêiner de injeção não entra. O
//!   [`TerminalCompletionFixture`] cria os serviços que o `TerminalCompletionService` lê: o `ILabelService` é o
//!   `LabelService` real, montado como o do original (com o `TestPathService(URI.file(homeDir))`, o `TestFileService` e o
//!   `UriIdentityService` real sobre ele), e o `IFileService` do serviço é o dublê [`TerminalCompletionFileService`], que no
//!   original é o objeto `{ stat, resolve, realpath }` posto por `instantiationService.stub(IFileService, ...)` depois que
//!   o `LabelService` já nasceu com o `TestFileService`.
//! - **`{ stat, resolve, realpath } as IFileService`.** Os demais métodos do contrato lançam `TypeError`
//!   (`fileService.x is not a function`), como no objeto parcial do original. `resolve(resource, options)` do original
//!   cobre os dois métodos do contrato do porte que o serviço usa (`resolve_with_metadata` com `resolveMetadata: true` e
//!   `resolve` sem ele): os dois devolvem o `createFileStat` com os filhos.
//! - **`validResources` e `childResources`.** São listas compartilhadas (`Rc<RefCell<..>>`) que o caso reatribui, como as
//!   variáveis `let` da suíte.
//! - **`createInstance(TestTerminalCompletionService)`.** A subclasse de teste só expõe o método protegido, que no porte é
//!   público; [`TerminalCompletionFixture::create_service`] cria outro serviço sobre os mesmos dublês.

use std::cell::RefCell;
use std::collections::BTreeMap;
use std::rc::Rc;

use je_base::cancellation::CancellationToken;
use je_base::errors::{self, Error};
use je_base::event::Event;
use je_base::lifecycle::{DisposableRef, DisposableStore};
use je_base::promise::Promise;
use je_base::uri::{Field, URI, UriChange};
use je_instantiation::Injectable;
use je_platform::configuration::service::IConfigurationService;
use je_platform::configuration::test_configuration_service::TestConfigurationService;
use je_platform::files::file_service::FileContentSource;
use je_platform::files::files::{
    CreateFileOptions, FileChangesEvent, FileContent, FileOperationEvent, FileStat, FileStatWithMetadata, FileStreamContent, FileSystemProviderActivationEvent,
    FileSystemProviderCapabilities, FileSystemProviderCapabilitiesChangeEvent, FileSystemProviderRef, FileSystemProviderRegistrationEvent, PartialFileDeleteOptions,
    ReadFileOptions, ResolveFileOptions, WatchOptions, WriteFileOptions,
};
use je_platform::files::i_file_service::IFileService;
use je_platform::label::label::ILabelService;
use je_platform::log::{LogService, NullLogService};
use je_platform::resources::uri_file;
use je_platform::storage::StorageService;
use je_platform::terminal::capabilities::terminal_capability_store::TerminalCapabilityStore;
use je_platform::terminal::terminal::{ShellType, TerminalShellType};
use je_platform::uri_identity::uri_identity::IUriIdentityService;
use je_platform::uri_identity::uri_identity_service::UriIdentityService;
use je_workbench::contrib::terminal_contrib::suggest::terminal_completion_item::{ITerminalCompletion, TerminalCompletionItemKind};
use je_workbench::contrib::terminal_contrib::suggest::terminal_completion_service::{
    ITerminalCompletionProvider, TerminalCompletionProviderResult, TerminalCompletionService,
};
use je_workbench::services::label::label_service::LabelService;
use je_workbench::test::browser::workbench_test_services::{TestEnvironmentService, TestPathService, TestRemoteAgentService};
use je_workbench::test::common::workbench_test_services::{
    FileStatChild, FileStatOptions, TestContextService, TestFileService, TestLifecycleService, TestStorageService, create_file_stat,
};

/// `testEnv`.
pub fn test_env() -> BTreeMap<String, String> {
    BTreeMap::from([("HOME".to_string(), "/home/user".to_string()), ("USERPROFILE".to_string(), "/home/user".to_string())])
}

/// `homeDir`: o diretório de casa de `testEnv` com a barra final.
pub fn home_dir() -> String {
    let home = if cfg!(windows) { test_env()["USERPROFILE"].clone() } else { test_env()["HOME"].clone() };
    if home.ends_with('/') { home } else { format!("{home}/") }
}

/// `normalizePath`.
fn normalize_path(path: &str) -> String {
    if path == "/" { path.to_string() } else { path.trim_end_matches('/').to_string() }
}

/// `fileService.x is not a function`.
fn not_mocked(method: &str) -> ! {
    errors::throw(Error::type_error(format!("fileService.{method} is not a function")))
}

/// O objeto `{ stat, resolve, realpath }` que a suíte põe no lugar do `IFileService`.
pub struct TerminalCompletionFileService {
    valid_resources: Rc<RefCell<Vec<URI>>>,
    child_resources: Rc<RefCell<Vec<FileStatChild>>>,
}

impl TerminalCompletionFileService {
    /// `doesResourceExist`.
    fn does_resource_exist(&self, resource: &URI) -> bool {
        let target = normalize_path(&resource.path().to_string_lossy());
        self.valid_resources.borrow().iter().any(|valid| normalize_path(&valid.path().to_string_lossy()) == target)
            || self.child_resources.borrow().iter().any(|child| normalize_path(&child.resource.path().to_string_lossy()) == target)
    }

    /// `throw new Error('Doesn\'t exist')` quando o recurso não existe.
    fn check_exists(&self, resource: &URI) -> Result<(), Error> {
        if self.does_resource_exist(resource) { Ok(()) } else { Err(Error::new("Doesn't exist")) }
    }

    /// O `resolve` do original: o stat com os filhos diretos do recurso.
    fn resolve_with_children(&self, resource: &URI) -> Promise<FileStatWithMetadata> {
        if let Err(error) = self.check_exists(resource) {
            return Promise::rejected(error);
        }
        let parent_path = resource.path().to_string_lossy();
        let parent_path = parent_path.strip_suffix('/').unwrap_or(&parent_path).to_string();
        let children: Vec<FileStatChild> = self
            .child_resources
            .borrow()
            .iter()
            .filter(|child| {
                let child_path = child.resource.path().to_string_lossy();
                let child_path = child_path.strip_suffix('/').unwrap_or(&child_path);
                child_path.starts_with(&parent_path) && child_path.matches('/').count() == parent_path.matches('/').count() + 1
            })
            .cloned()
            .collect();
        Promise::resolved(create_file_stat(resource, &FileStatOptions { children: Some(children), ..FileStatOptions::default() }))
    }
}

impl IFileService for TerminalCompletionFileService {
    fn on_did_files_change(&self) -> Event<Rc<FileChangesEvent>> {
        not_mocked("onDidFilesChange")
    }

    fn get_provider(&self, _scheme: &str) -> Option<FileSystemProviderRef> {
        not_mocked("getProvider")
    }

    fn resolve_with_metadata(&self, resource: &URI, _options: Option<ResolveFileOptions>) -> Promise<FileStatWithMetadata> {
        self.resolve_with_children(resource)
    }

    /// `async stat(resource)`.
    fn stat(&self, resource: &URI) -> Promise<FileStatWithMetadata> {
        match self.check_exists(resource) {
            Ok(()) => Promise::resolved(create_file_stat(resource, &FileStatOptions::default())),
            Err(error) => Promise::rejected(error),
        }
    }

    fn exists(&self, _resource: &URI) -> Promise<bool> {
        not_mocked("exists")
    }

    fn on_did_change_file_system_provider_registrations(&self) -> Event<FileSystemProviderRegistrationEvent> {
        not_mocked("onDidChangeFileSystemProviderRegistrations")
    }

    fn on_did_change_file_system_provider_capabilities(&self) -> Event<FileSystemProviderCapabilitiesChangeEvent> {
        not_mocked("onDidChangeFileSystemProviderCapabilities")
    }

    fn has_provider(&self, _resource: &URI) -> bool {
        not_mocked("hasProvider")
    }

    fn has_capability(&self, _resource: &URI, _capability: FileSystemProviderCapabilities) -> bool {
        not_mocked("hasCapability")
    }

    fn read_file(&self, _resource: &URI, _options: Option<ReadFileOptions>, _token: Option<CancellationToken>) -> Promise<FileContent> {
        not_mocked("readFile")
    }

    fn resolve(&self, resource: &URI, _options: Option<ResolveFileOptions>) -> Promise<FileStat> {
        self.resolve_with_children(resource).then(FileStatWithMetadata::into_stat)
    }

    fn create_folder(&self, _resource: &URI) -> Promise<FileStatWithMetadata> {
        not_mocked("createFolder")
    }

    fn watch(&self, _resource: &URI, _options: Option<WatchOptions>) -> DisposableRef {
        not_mocked("watch")
    }

    fn read_file_stream(&self, _resource: &URI, _options: Option<ReadFileOptions>, _token: Option<CancellationToken>) -> Promise<FileStreamContent> {
        not_mocked("readFileStream")
    }

    fn write_file(&self, _resource: &URI, _content: FileContentSource, _options: Option<WriteFileOptions>) -> Promise<FileStatWithMetadata> {
        not_mocked("writeFile")
    }

    fn del(&self, _resource: &URI, _options: Option<PartialFileDeleteOptions>) -> Promise<()> {
        not_mocked("del")
    }

    fn on_did_run_operation(&self) -> Event<FileOperationEvent> {
        not_mocked("onDidRunOperation")
    }

    fn create_file(&self, _resource: &URI, _content: Option<FileContentSource>, _options: Option<CreateFileOptions>) -> Promise<FileStatWithMetadata> {
        not_mocked("createFile")
    }

    fn on_will_activate_file_system_provider(&self) -> Event<FileSystemProviderActivationEvent> {
        not_mocked("onWillActivateFileSystemProvider")
    }

    fn register_provider(&self, _scheme: &str, _provider: FileSystemProviderRef) -> Result<DisposableRef, Error> {
        not_mocked("registerProvider")
    }

    /// `async realpath(resource)`.
    fn realpath(&self, resource: &URI) -> Promise<Option<URI>> {
        let path = resource.path().to_string_lossy();
        let target = if path.contains("symlink-file") {
            Some("/target/actual-file.txt")
        } else if path.contains("symlink-folder") {
            Some("/target/actual-folder")
        } else {
            None
        };
        Promise::resolved(target.map(|target| resource.with(&UriChange { path: Field::from(target), ..UriChange::default() })))
    }
}

/// O que o `setup` da suíte `TerminalCompletionService` monta.
pub struct TerminalCompletionFixture {
    pub configuration_service: Rc<TestConfigurationService>,
    pub capabilities: Rc<TerminalCapabilityStore>,
    /// `validResources`.
    pub valid_resources: Rc<RefCell<Vec<URI>>>,
    /// `childResources`.
    pub child_resources: Rc<RefCell<Vec<FileStatChild>>>,
    pub terminal_completion_service: Rc<TerminalCompletionService>,
    store: Rc<DisposableStore>,
    file_service: Rc<TerminalCompletionFileService>,
    label_service: Rc<LabelService>,
    log_service: Rc<dyn LogService>,
}

impl TerminalCompletionFixture {
    /// O `setup`: o `workbenchInstantiationService` com o `TestPathService(URI.file(homeDir))`, o `stub` do `IFileService` e do
    /// `IConfigurationService` e o `store.add(createInstance(TerminalCompletionService))` com o `processEnv` de teste.
    pub fn new(store: &Rc<DisposableStore>) -> Self {
        let test_file_service = store.add_rc(TestFileService::new());
        let test_file_service_contract: Rc<dyn IFileService> = test_file_service;
        let uri_identity_service = store.add_rc(UriIdentityService::create((), (test_file_service_contract,)));
        let storage = store.add_rc(TestStorageService::new());
        let storage_service: Rc<dyn StorageService> = Rc::new(storage);
        let lifecycle_service = store.add_rc(TestLifecycleService::new());
        let uri_identity_contract: Rc<dyn IUriIdentityService> = uri_identity_service;
        let label_service = store.add_rc(LabelService::create(
            (),
            (
                TestEnvironmentService::new(),
                TestContextService::new(None, None),
                TestPathService::new(Some(uri_file(&home_dir())), None),
                Rc::new(TestRemoteAgentService),
                storage_service,
                lifecycle_service,
                uri_identity_contract,
            ),
        ));

        let configuration_service = TestConfigurationService::new(None);
        store.add_rc(configuration_service.on_did_change_configuration_emitter.clone());
        let valid_resources: Rc<RefCell<Vec<URI>>> = Rc::default();
        let child_resources: Rc<RefCell<Vec<FileStatChild>>> = Rc::default();
        let file_service = Rc::new(TerminalCompletionFileService { valid_resources: valid_resources.clone(), child_resources: child_resources.clone() });

        let log_service = NullLogService::shared();
        let terminal_completion_service = create_terminal_completion_service(store, &configuration_service, &file_service, &label_service, &log_service);
        terminal_completion_service.set_process_env(test_env());

        Self {
            configuration_service,
            capabilities: store.add_rc(TerminalCapabilityStore::new()),
            valid_resources,
            child_resources,
            terminal_completion_service,
            store: store.clone(),
            file_service,
            label_service,
            log_service,
        }
    }

    /// `store.add(instantiationService.createInstance(TerminalCompletionService))` sobre os mesmos dublês.
    pub fn create_service(&self) -> Rc<TerminalCompletionService> {
        create_terminal_completion_service(&self.store, &self.configuration_service, &self.file_service, &self.label_service, &self.log_service)
    }
}

fn create_terminal_completion_service(
    store: &Rc<DisposableStore>,
    configuration_service: &Rc<TestConfigurationService>,
    file_service: &Rc<TerminalCompletionFileService>,
    label_service: &Rc<LabelService>,
    log_service: &Rc<dyn LogService>,
) -> Rc<TerminalCompletionService> {
    let configuration_service: Rc<dyn IConfigurationService> = configuration_service.clone();
    let file_service: Rc<dyn IFileService> = file_service.clone();
    let label_service: Rc<dyn ILabelService> = label_service.clone();
    store.add_rc(TerminalCompletionService::create((), (configuration_service, file_service, label_service, log_service.clone())))
}

/// `createMockProvider(id)`.
pub struct MockCompletionProvider {
    id: RefCell<String>,
    trigger_characters: RefCell<Option<Vec<String>>>,
}

impl MockCompletionProvider {
    pub fn new(id: &str) -> Rc<Self> {
        Rc::new(Self { id: RefCell::new(id.to_string()), trigger_characters: RefCell::new(None) })
    }
}

impl ITerminalCompletionProvider for MockCompletionProvider {
    fn id(&self) -> String {
        self.id.borrow().clone()
    }

    fn set_id(&self, id: &str) {
        *self.id.borrow_mut() = id.to_string();
    }

    fn provide_completions(&self, _value: &str, _cursor_position: i64, _token: &CancellationToken) -> Promise<Option<TerminalCompletionProviderResult>> {
        let id = self.id();
        Promise::resolved(Some(TerminalCompletionProviderResult::Items(vec![ITerminalCompletion {
            label: format!("completion-from-{id}").as_str().into(),
            kind: Some(TerminalCompletionItemKind::Method),
            replacement_range: Some([0, 0]),
            provider: id,
            ..ITerminalCompletion::default()
        }])))
    }

    fn trigger_characters(&self) -> Option<Vec<String>> {
        self.trigger_characters.borrow().clone()
    }

    fn set_trigger_characters(&self, trigger_characters: Vec<String>) {
        *self.trigger_characters.borrow_mut() = Some(trigger_characters);
    }
}

/// `TerminalShellType` de um tipo de shell.
pub fn shell(shell_type: impl Into<ShellType>) -> TerminalShellType {
    Some(shell_type.into())
}
