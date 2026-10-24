//! Dublês de `src/vs/workbench/contrib/terminal/test/browser/terminalProcessManager.test.ts` (e os de `workbenchTestServices.ts` que
//! ele usa): o `TestTerminalChildProcess` (com `fireReady`), o `TestTerminalInstanceService` (com o `ptyHostRestartEmitter`, os
//! argumentos do reanexo e os processos criados) e o `workbenchInstantiationService` reduzido aos serviços que o
//! `TerminalProcessManager` lê, com o `EnvironmentVariableService` real (sobre o `IStorageService` em memória e o `IExtensionService`
//! sem extensões).
//!
//! Divergências:
//! - **`as unknown as ITerminalBackend` e `Partial<ITerminalInstanceService>`.** Os contratos do porte são traits completos: os
//!   membros que o original não define devolvem o valor neutro (eventos vazios, promessas resolvidas), como em
//!   `terminal_instance_support.rs`. A exceção é `ITerminalInstanceService.createInstance`, que falha com a mensagem do `TypeError`.
//! - **`Disposable` dos emissores.** O `_onProcessReady` do original nunca é descartado; o emissor do porte é rastreado pelo guarda
//!   de vazamentos, então o dublê o registra no store do caso (o `LeakGuard::store`).
//! - **`instantiationService.stub(ITerminalService, { setNextCommandId: async () => { } })`.** É o `TestTerminalService` de
//!   `terminal_instance_support.rs`, que devolve `setNextCommandId` resolvido.
//! - **`workbenchInstantiationService`.** Os 15 serviços da tupla do gerenciador, como os de `terminalInstance.test.ts`; o
//!   `IConfigurationResolverService` fica sem registro (o `TestInstantiationService` não estrito entrega indefinido).

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use je_base::dynamic::Dynamic;
use je_base::errors::{self, Error};
use je_base::event::{Emitter, Event};
use je_base::lifecycle::DisposableStore;
use je_base::platform::OperatingSystem;
use je_base::process::ProcessEnvironment;
use je_base::promise::Promise;
use je_instantiation::{InstantiationService, ServiceCollection};
use je_platform::configuration::service::{I_CONFIGURATION_SERVICE, IConfigurationService};
use je_platform::configuration::test_configuration_service::TestConfigurationService;
use je_platform::files::i_file_service::{I_FILE_SERVICE, IFileService};
use je_platform::log::NullLogService;
use je_platform::path::path_service::{I_PATH_SERVICE, IPathService};
use je_platform::product::{I_PRODUCT_SERVICE, IProductService, ProductService};
use je_platform::storage::{I_STORAGE_SERVICE, InMemoryStorageService, StorageService};
use je_platform::terminal::terminal::{
    I_TERMINAL_LOG_SERVICE, IProcessProperty, IProcessReadyEvent, IShellLaunchConfig, ITerminalChildProcess, ITerminalLaunchOutcome, ITerminalLogService,
    ITerminalProcessOptions, ProcessData, ProcessPropertyType, ShellCwd, TerminalIcon, TerminalLocation, TitleEventSource, UnicodeVersion,
};
use je_platform::workspace::{I_WORKSPACE_CONTEXT_SERVICE, IWorkspaceContextService};
use je_platform::workspace_trust::{I_WORKSPACE_TRUST_REQUEST_SERVICE, IWorkspaceTrustRequestService};
use je_workbench::contrib::terminal::browser::terminal::{
    I_TERMINAL_CONFIGURATION_SERVICE, I_TERMINAL_INSTANCE_SERVICE, I_TERMINAL_SERVICE, ITerminalConfigurationService, ITerminalInstance, ITerminalInstanceService,
    ITerminalService, ShellLaunchConfigOrProfile,
};
use je_workbench::contrib::terminal::browser::terminal_configuration_service::TerminalConfigurationService;
use je_workbench::contrib::terminal::common::environment_variable::{I_ENVIRONMENT_VARIABLE_SERVICE, IEnvironmentVariableService};
use je_workbench::contrib::terminal::common::environment_variable_service::EnvironmentVariableService;
use je_workbench::contrib::terminal::common::terminal::{I_TERMINAL_PROFILE_RESOLVER_SERVICE, ITerminalProfileResolverService};
use je_workbench::contrib::terminal::common::terminal_backend::{IDetachInstanceRequest, IPtyHostLatencyMeasurement, ITerminalBackend};
use je_workbench::contrib::terminal::common::terminal_environment::{ITerminalBackendWslPath, WslPathDirection};
use je_workbench::services::environment::environment_service::{I_WORKBENCH_ENVIRONMENT_SERVICE, IWorkbenchEnvironmentService};
use je_workbench::services::extensions::extensions::{I_EXTENSION_SERVICE, IExtensionService};
use je_workbench::services::history::history::{I_HISTORY_SERVICE, IHistoryService};
use je_workbench::services::remote::remote_agent_service::{I_REMOTE_AGENT_SERVICE, IRemoteAgentService};
use je_workbench::test::common::workbench_test_services::{TestContextService, TestFileService};
use je_workbench::test::browser::workbench_test_services::{TestEnvironmentService, TestPathService, TestRemoteAgentService};

use super::extensions_workbench_service_support::StubExtensionService;
use super::terminal_instance_support::{TestHistoryService, TestTerminalProfileResolverService, TestTerminalService, TestTerminalWorkspaceTrustRequestService};

/// `TestTerminalChildProcess`.
pub struct ProcessManagerChildProcess {
    id: Cell<u32>,
    should_persist: bool,
    on_process_ready: Rc<Emitter<IProcessReadyEvent>>,
}

impl ProcessManagerChildProcess {
    pub fn new(should_persist: bool, store: &DisposableStore) -> Rc<Self> {
        let on_process_ready = store.add_rc(Emitter::new());
        Rc::new(Self { id: Cell::new(0), should_persist, on_process_ready })
    }

    /// `fireReady(pid)`.
    pub fn fire_ready(&self, pid: i32) {
        self.on_process_ready.fire(IProcessReadyEvent { pid, cwd: String::new(), windows_pty: None });
    }
}

impl ITerminalChildProcess for ProcessManagerChildProcess {
    fn id(&self) -> u32 {
        self.id.get()
    }

    fn should_persist(&self) -> bool {
        self.should_persist
    }

    fn on_process_data(&self) -> Event<ProcessData> {
        Event::none()
    }

    fn on_process_ready(&self) -> Event<IProcessReadyEvent> {
        self.on_process_ready.event()
    }

    fn on_did_change_property(&self) -> Event<IProcessProperty> {
        Event::none()
    }

    fn on_process_exit(&self) -> Event<Option<i32>> {
        Event::none()
    }

    fn start(&self) -> Promise<Option<ITerminalLaunchOutcome>> {
        Promise::resolved(None)
    }

    fn shutdown(&self, _immediate: bool) {}

    fn input(&self, _data: &str) {}

    fn send_signal(&self, _signal: &str) {}

    fn process_binary(&self, _data: &str) -> Promise<()> {
        Promise::resolved(())
    }

    fn resize(&self, _cols: i32, _rows: i32, _pixel_width: Option<i32>, _pixel_height: Option<i32>) -> Result<(), Error> {
        Ok(())
    }

    fn clear_buffer(&self) -> Promise<()> {
        Promise::resolved(())
    }

    fn acknowledge_data_event(&self, _char_count: usize) {}

    fn set_unicode_version(&self, _version: UnicodeVersion) -> Promise<()> {
        Promise::resolved(())
    }

    fn get_initial_cwd(&self) -> Promise<String> {
        Promise::resolved(String::new())
    }

    fn get_cwd(&self) -> Promise<String> {
        Promise::resolved(String::new())
    }

    fn refresh_property(&self, _property: ProcessPropertyType) -> Promise<IProcessProperty> {
        Promise::resolved(IProcessProperty::Cwd(String::new()))
    }

    fn update_property(&self, _property: IProcessProperty) -> Promise<()> {
        errors::throw(Error::new("Method not implemented."))
    }
}

/// O estado que o `TestTerminalInstanceService` e o backend dele compartilham.
struct SharedState {
    store: Rc<DisposableStore>,
    pty_host_restart_emitter: Rc<Emitter<()>>,
    revived_attach_args: RefCell<Vec<(u32, u32)>>,
    created_processes: RefCell<Vec<Rc<ProcessManagerChildProcess>>>,
    refuse_revived_attach: Cell<bool>,
}

/// O objeto que `TestTerminalInstanceService.getBackend()` devolve.
struct TestBackend {
    state: Rc<SharedState>,
}

impl ITerminalBackendWslPath for TestBackend {
    fn get_wsl_path(&self, original: &str, _direction: WslPathDirection) -> Promise<String> {
        Promise::resolved(original.to_owned())
    }
}

impl ITerminalBackend for TestBackend {
    fn remote_authority(&self) -> Option<String> {
        None
    }

    fn is_responsive(&self) -> bool {
        false
    }

    fn when_ready(&self) -> Promise<()> {
        Promise::resolved(())
    }

    fn set_ready(&self) {}

    fn on_pty_host_unresponsive(&self) -> Event<()> {
        Event::none()
    }

    fn on_pty_host_responsive(&self) -> Event<()> {
        Event::none()
    }

    fn on_pty_host_restart(&self) -> Event<()> {
        self.state.pty_host_restart_emitter.event()
    }

    fn on_did_request_detach(&self) -> Event<IDetachInstanceRequest> {
        Event::none()
    }

    fn attach_to_process(&self, _id: u32) -> Promise<Option<Rc<dyn ITerminalChildProcess>>> {
        Promise::resolved(None)
    }

    fn attach_to_revived_process(&self, id: u32, pid: u32) -> Promise<Option<Rc<dyn ITerminalChildProcess>>> {
        self.state.revived_attach_args.borrow_mut().push((id, pid));
        if self.state.refuse_revived_attach.get() {
            return Promise::resolved(None);
        }
        let process: Rc<dyn ITerminalChildProcess> = ProcessManagerChildProcess::new(true, &self.state.store);
        Promise::resolved(Some(process))
    }

    fn get_latency(&self) -> Promise<Vec<IPtyHostLatencyMeasurement>> {
        Promise::resolved(Vec::new())
    }

    fn get_default_system_shell(&self, _os_override: Option<OperatingSystem>) -> Promise<String> {
        Promise::resolved(String::new())
    }

    fn get_environment(&self) -> Promise<ProcessEnvironment> {
        Promise::resolved(ProcessEnvironment::new())
    }

    fn get_shell_environment(&self) -> Promise<Option<ProcessEnvironment>> {
        Promise::resolved(Some(ProcessEnvironment::new()))
    }

    fn update_title(&self, _id: u32, _title: &str, _title_source: TitleEventSource) -> Promise<()> {
        Promise::resolved(())
    }

    fn update_icon(&self, _id: u32, _user_initiated: bool, _icon: &TerminalIcon, _color: Option<&str>) -> Promise<()> {
        Promise::resolved(())
    }

    fn set_next_command_id(&self, _id: u32, _command_line: &str, _command_id: &str) -> Promise<()> {
        Promise::resolved(())
    }

    fn create_process(
        &self,
        _shell_launch_config: &IShellLaunchConfig,
        _cwd: &str,
        _cols: i32,
        _rows: i32,
        _unicode_version: UnicodeVersion,
        _env: &ProcessEnvironment,
        _options: &ITerminalProcessOptions,
        should_persist: bool,
    ) -> Promise<Rc<dyn ITerminalChildProcess>> {
        let child_process = ProcessManagerChildProcess::new(should_persist, &self.state.store);
        // `childProcess.id = this.createdProcesses.push(childProcess)`
        let length = {
            let mut created_processes = self.state.created_processes.borrow_mut();
            created_processes.push(child_process.clone());
            created_processes.len()
        };
        child_process.id.set(length as u32);
        let process: Rc<dyn ITerminalChildProcess> = child_process;
        Promise::resolved(process)
    }

    fn restart_pty_host(&self) {}
}

/// `TestTerminalInstanceService`.
pub struct ProcessManagerInstanceService {
    state: Rc<SharedState>,
}

impl ProcessManagerInstanceService {
    /// `new TestTerminalInstanceService()` mais o `store.add(terminalInstanceService.ptyHostRestartEmitter)` do `setup`.
    pub fn new(store: Rc<DisposableStore>) -> Rc<Self> {
        let pty_host_restart_emitter = store.add_rc(Emitter::new());
        Rc::new(Self {
            state: Rc::new(SharedState {
                store,
                pty_host_restart_emitter,
                revived_attach_args: RefCell::new(Vec::new()),
                created_processes: RefCell::new(Vec::new()),
                refuse_revived_attach: Cell::new(false),
            }),
        })
    }

    /// `ptyHostRestartEmitter.fire()`.
    pub fn fire_pty_host_restart(&self) {
        self.state.pty_host_restart_emitter.fire(());
    }

    /// `revivedAttachArgs`.
    pub fn revived_attach_args(&self) -> Vec<(u32, u32)> {
        self.state.revived_attach_args.borrow().clone()
    }

    /// `createdProcesses[index]`.
    pub fn created_process(&self, index: usize) -> Rc<ProcessManagerChildProcess> {
        self.state.created_processes.borrow()[index].clone()
    }

    /// `refuseRevivedAttach = value`.
    pub fn set_refuse_revived_attach(&self, value: bool) {
        self.state.refuse_revived_attach.set(value);
    }
}

impl ITerminalInstanceService for ProcessManagerInstanceService {
    fn on_did_create_instance(&self) -> Event<Rc<dyn ITerminalInstance>> {
        Event::none()
    }

    fn on_did_register_backend(&self) -> Event<Rc<dyn ITerminalBackend>> {
        Event::none()
    }

    fn convert_profile_to_shell_launch_config(&self, _shell_launch_config_or_profile: Option<ShellLaunchConfigOrProfile>, _cwd: Option<ShellCwd>) -> IShellLaunchConfig {
        IShellLaunchConfig::default()
    }

    fn create_instance(&self, _launch_config: IShellLaunchConfig, _target: TerminalLocation) -> Rc<dyn ITerminalInstance> {
        panic!("TypeError: this.terminalInstanceService.createInstance is not a function (o dublê Partial do original só define getBackend)")
    }

    fn get_backend(&self, _remote_authority: Option<&str>) -> Promise<Option<Rc<dyn ITerminalBackend>>> {
        let backend: Rc<dyn ITerminalBackend> = Rc::new(TestBackend { state: self.state.clone() });
        Promise::resolved(Some(backend))
    }

    fn get_registered_backends(&self) -> Vec<Rc<dyn ITerminalBackend>> {
        Vec::new()
    }

    fn did_register_backend(&self, _backend: Rc<dyn ITerminalBackend>) {}
}

/// O que o `setup` do `terminalProcessManager.test.ts` tira do `workbenchInstantiationService`.
pub struct ProcessManagerServices {
    pub instantiation_service: Rc<InstantiationService>,
    pub terminal_instance_service: Rc<ProcessManagerInstanceService>,
    pub environment_variable_service: Rc<EnvironmentVariableService>,
    pub configuration_service: Rc<TestConfigurationService>,
}

/// `workbenchInstantiationService(undefined, store)` mais os `stub` do `setup` (`ITerminalInstanceService` e `ITerminalService`).
pub fn create_process_manager_services(store: &Rc<DisposableStore>) -> ProcessManagerServices {
    let configuration_service = TestConfigurationService::new(Some(Dynamic::object([("files", Dynamic::object(Vec::<(&str, Dynamic)>::new()))])));
    // O emissor do `TestConfigurationService` é descartado pelo store do caso (o original não o descarta).
    store.add_rc(configuration_service.on_did_change_configuration_emitter.clone());
    let configuration: Rc<dyn IConfigurationService> = configuration_service.clone();
    let configuration_instantiation = store.add_rc(InstantiationService::new(ServiceCollection::new().with(I_CONFIGURATION_SERVICE, configuration.clone())));
    let terminal_configuration_concrete = store.add_rc(configuration_instantiation.create_instance::<TerminalConfigurationService>(()));
    let terminal_configuration_service: Rc<dyn ITerminalConfigurationService> = terminal_configuration_concrete;

    // O `EnvironmentVariableService` real do `workbenchInstantiationService`, sobre o armazenamento e o serviço de extensões.
    let extension_service: Rc<dyn IExtensionService> = StubExtensionService::new();
    let storage = store.add_rc(InMemoryStorageService::new());
    let storage_service: Rc<dyn StorageService> = Rc::new(storage);
    let environment_variable_instantiation =
        store.add_rc(InstantiationService::new(ServiceCollection::new().with(I_EXTENSION_SERVICE, extension_service).with(I_STORAGE_SERVICE, storage_service)));
    let environment_variable_concrete = store.add_rc(environment_variable_instantiation.create_instance::<EnvironmentVariableService>(()));
    let environment_variable_service: Rc<dyn IEnvironmentVariableService> = environment_variable_concrete.clone();

    let file_service_concrete = store.add_rc(TestFileService::new());
    let file_service: Rc<dyn IFileService> = file_service_concrete;
    let workspace_context_service: Rc<dyn IWorkspaceContextService> = TestContextService::new(None, None);
    let terminal_instance_concrete = ProcessManagerInstanceService::new(store.clone());
    let terminal_instance_service: Rc<dyn ITerminalInstanceService> = terminal_instance_concrete.clone();
    let workspace_trust_request_service: Rc<dyn IWorkspaceTrustRequestService> = TestTerminalWorkspaceTrustRequestService::new();
    let terminal_profile_resolver_service: Rc<dyn ITerminalProfileResolverService> = TestTerminalProfileResolverService::mock();
    let path_service: Rc<dyn IPathService> = TestPathService::new(None, None);
    let workbench_environment_service: Rc<dyn IWorkbenchEnvironmentService> = TestEnvironmentService::new();
    let remote_agent_service: Rc<dyn IRemoteAgentService> = Rc::new(TestRemoteAgentService);
    let product_service: Rc<dyn IProductService> = Rc::new(ProductService::default());
    let history_service: Rc<dyn IHistoryService> = TestHistoryService::new(None);
    let terminal_service: Rc<dyn ITerminalService> = Rc::new(TestTerminalService);
    let log_service: Rc<dyn ITerminalLogService> = Rc::new(NullLogService);
    let instantiation_service = store.add_rc(InstantiationService::new(
        ServiceCollection::new()
            .with(I_CONFIGURATION_SERVICE, configuration)
            .with(I_TERMINAL_CONFIGURATION_SERVICE, terminal_configuration_service)
            .with(I_FILE_SERVICE, file_service)
            .with(I_WORKSPACE_CONTEXT_SERVICE, workspace_context_service)
            .with(I_TERMINAL_INSTANCE_SERVICE, terminal_instance_service)
            .with(I_WORKSPACE_TRUST_REQUEST_SERVICE, workspace_trust_request_service)
            .with(I_TERMINAL_PROFILE_RESOLVER_SERVICE, terminal_profile_resolver_service)
            .with(I_PATH_SERVICE, path_service)
            .with(I_WORKBENCH_ENVIRONMENT_SERVICE, workbench_environment_service)
            .with(I_REMOTE_AGENT_SERVICE, remote_agent_service)
            .with(I_PRODUCT_SERVICE, product_service)
            .with(I_HISTORY_SERVICE, history_service)
            .with(I_ENVIRONMENT_VARIABLE_SERVICE, environment_variable_service)
            .with(I_TERMINAL_SERVICE, terminal_service)
            .with(I_TERMINAL_LOG_SERVICE, log_service),
    ));
    ProcessManagerServices {
        instantiation_service,
        terminal_instance_service: terminal_instance_concrete,
        environment_variable_service: environment_variable_concrete,
        configuration_service,
    }
}
