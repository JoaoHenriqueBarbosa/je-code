//! Infraestrutura de teste de `src/vs/workbench/services/workingCopy/test/electron-browser/workingCopyHistoryService.test.ts`:
//! o `TestWorkingCopyHistoryService` e o `TestWillShutdownEvent`, que o teste do rastreador também usa.
//!
//! Divergências:
//! - **Herança.** `TestWorkingCopyHistoryService extends NativeWorkingCopyHistoryService` é o
//!   [`TestWorkingCopyHistoryService`], que guarda o [`NativeWorkingCopyHistoryService`] e o expõe por `Deref` (com os
//!   `_fileService`, `_configurationService` e `_lifecycleService` como campos públicos `file_service`,
//!   `configuration_service` e `lifecycle_service`).
//! - **`fileService?`.** `Option<Rc<FileService>>`: sem ele, o serviço cria um `FileService` com dois
//!   `InMemoryFileSystemProvider` (esquemas `inmemory` e `vscode-userdata`), como o original.
//! - **`@IFileService`.** O `FileService` concreto (o contrato do porte não tem `cloneFile`).
//! - **`TestWillShutdownEvent`.** O evento de desligamento em que cada `join` guarda a promessa (ou o resultado da função) em
//!   `value`. [`simulate_shutdown`] é o trio `new TestWillShutdownEvent()`, `fireWillShutdown(event)` e
//!   `Promise.allSettled(event.value)`.

use std::cell::RefCell;
use std::ops::Deref;
use std::rc::Rc;

use je_base::cancellation::CancellationToken;
use je_base::lifecycle::{DisposableStore, IDisposable};
use je_base::network::schemas;
use je_base::promise::Promise;
use je_base::uri::{Field, URI, UriChange};
use je_instantiation::Injectable;
use je_platform::configuration::service::IConfigurationService;
use je_platform::configuration::test_configuration_service::TestConfigurationService;
use je_platform::files::file_service::FileService;
use je_platform::files::i_file_service::IFileService;
use je_platform::files::in_memory::InMemoryFileSystemProvider;
use je_platform::label::label::ILabelService;
use je_platform::log::{LogServiceRef, NullLogService};
use je_platform::storage::StorageService;
use je_platform::uri_identity::uri_identity::IUriIdentityService;
use je_platform::uri_identity::uri_identity_service::UriIdentityService;
use je_workbench::services::label::label_service::LabelService;
use je_workbench::services::lifecycle::lifecycle::{ILifecycleService, ShutdownReason, WillShutdownEvent, WillShutdownJoin};
use je_workbench::services::remote::remote_agent_service::IRemoteAgentService;
use je_workbench::services::working_copy::working_copy_history::WorkingCopyHistoryEntryRef;
use je_workbench::services::working_copy::working_copy_history_service::NativeWorkingCopyHistoryService;
use je_workbench::test::browser::workbench_test_services::{TestEnvironmentService, TestPathService, TestRemoteAgentService};
use je_workbench::test::common::workbench_test_services::{TestContextService, TestLifecycleService, TestStorageService};

/// `TestWorkingCopyHistoryService`.
pub struct TestWorkingCopyHistoryService {
    native: Rc<NativeWorkingCopyHistoryService>,
    /// `_fileService`.
    pub file_service: Rc<FileService>,
    /// `_configurationService`.
    pub configuration_service: Rc<TestConfigurationService>,
    /// `_lifecycleService`.
    pub lifecycle_service: Rc<TestLifecycleService>,
}

impl TestWorkingCopyHistoryService {
    /// `constructor(disposables, fileService?)`.
    pub fn new(disposables: &Rc<DisposableStore>, file_service: Option<Rc<FileService>>) -> Rc<Self> {
        let environment_service = TestEnvironmentService::new();
        let log_service: LogServiceRef = NullLogService::shared();

        let file_service = file_service.unwrap_or_else(|| {
            let file_service = disposables.add_rc(FileService::new(log_service.clone()));
            let in_memory_provider = disposables.add_rc(InMemoryFileSystemProvider::new());
            disposables.add(file_service.register_provider(schemas::IN_MEMORY, in_memory_provider).unwrap());
            let user_data_provider = disposables.add_rc(InMemoryFileSystemProvider::new());
            disposables.add(file_service.register_provider(schemas::VSCODE_USER_DATA, user_data_provider).unwrap());

            file_service
        });

        let remote_agent_service: Rc<dyn IRemoteAgentService> = Rc::new(TestRemoteAgentService);
        let file_service_contract: Rc<dyn IFileService> = file_service.clone();
        let uri_identity_service = disposables.add_rc(UriIdentityService::create((), (file_service_contract,)));
        let lifecycle_service = disposables.add_rc(TestLifecycleService::new());
        let uri_identity_contract: Rc<dyn IUriIdentityService> = uri_identity_service;
        let storage = disposables.add_rc(TestStorageService::new());
        let storage_service: Rc<dyn StorageService> = Rc::new(storage);
        let label_service = disposables.add_rc(LabelService::create(
            (),
            (
                environment_service.clone(),
                TestContextService::new(None, None),
                TestPathService::new(None, None),
                Rc::new(TestRemoteAgentService),
                storage_service,
                lifecycle_service.clone(),
                uri_identity_contract.clone(),
            ),
        ));
        let label_contract: Rc<dyn ILabelService> = label_service;
        let configuration_service = TestConfigurationService::new(None);
        let configuration_contract: Rc<dyn IConfigurationService> = configuration_service.clone();
        let lifecycle_contract: Rc<dyn ILifecycleService> = lifecycle_service.clone();

        let native = NativeWorkingCopyHistoryService::new(
            file_service.clone(),
            remote_agent_service,
            environment_service,
            uri_identity_contract,
            label_contract,
            lifecycle_contract,
            log_service,
            configuration_contract,
        );

        Rc::new(Self { native, file_service, configuration_service, lifecycle_service })
    }
}

impl Deref for TestWorkingCopyHistoryService {
    type Target = NativeWorkingCopyHistoryService;

    fn deref(&self) -> &NativeWorkingCopyHistoryService {
        &self.native
    }
}

impl IDisposable for TestWorkingCopyHistoryService {
    fn dispose(&self) {
        self.native.dispose();
    }
}

/// `uri.toString()`.
pub fn text(resource: &URI) -> String {
    resource.to_string(false).to_string_lossy()
}

/// `uri.with({ scheme })`.
pub fn with_scheme(resource: &URI, scheme: &str) -> URI {
    resource.with(&UriChange { scheme: Field::from(scheme), ..UriChange::default() })
}

/// A entrada movida confere com a original: mesmo `id`, `timestamp` e `source`, outro local e a working copy de destino
/// (`assert.notStrictEqual(entries[n].location, entryN.location)` compara a identidade dos objetos `URI`, sempre diferentes no
/// original; aqui compara os valores, uma afirmação mais forte e verdadeira, porque a pasta da entrada depende do recurso da
/// working copy, que mudou).
pub fn assert_moved_entry(moved: &WorkingCopyHistoryEntryRef, original: &WorkingCopyHistoryEntryRef, target_resource: &URI) {
    assert_eq!(moved.id, original.id);
    assert_eq!(moved.timestamp(), original.timestamp());
    assert_eq!(moved.source(), original.source());
    assert_ne!(text(&moved.location), text(&original.location));
    assert_eq!(text(&moved.working_copy.resource), text(target_resource));
}

/// O trio `const event = new TestWillShutdownEvent(); service._lifecycleService.fireWillShutdown(event);
/// await Promise.allSettled(event.value);`: dispara o desligamento e devolve a promessa que assenta quando todas as promessas
/// juntadas (`event.value`) assentarem.
pub fn simulate_shutdown(lifecycle_service: &TestLifecycleService) -> Promise<()> {
    let value: Rc<RefCell<Vec<Promise<()>>>> = Rc::default();
    let joined = value.clone();
    let event = WillShutdownEvent::new(
        ShutdownReason::Close,
        CancellationToken::None,
        Rc::new(move |join, _joiner| {
            // `typeof promise === 'function' ? promise() : promise`
            let promise = match join {
                WillShutdownJoin::Promise(promise) => promise,
                WillShutdownJoin::Function(promise_fn) => promise_fn(),
            };
            joined.borrow_mut().push(promise);
        }),
        Rc::new(Vec::new),
        // No-Op in tests
        Rc::new(|| {}),
    );
    lifecycle_service.fire_will_shutdown(event);

    let promises = value.borrow().clone();

    Promise::all_settled(promises).then(|_| ())
}
