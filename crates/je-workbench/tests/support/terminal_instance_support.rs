//! Dublês de `src/vs/workbench/contrib/terminal/test/browser/terminalInstance.test.ts` (e os de `workbenchTestServices.ts` que ele
//! usa): `TestTerminalChildProcess`, `TestTerminalInstanceService` (com o backend que ele devolve), `MockTerminalProfileResolverService`
//! (sobre o `TestTerminalProfileResolverService`), `TestTerminalWorkspaceTrustRequestService`, o `ITerminalService` mínimo do caso
//! (`{ setNextCommandId: async () => { } }`), o `TestHistoryService` e um `EnvironmentVariableService` sem persistência.
//!
//! Divergências:
//! - **`Partial<ITerminalInstanceService>`, o objeto do backend e `mock<IWorkspaceTrustRequestService>()`.** O original deixa os
//!   outros membros indefinidos (chamá-los lança `TypeError`). Os contratos do porte são traits completos, então os membros que o
//!   original não define devolvem o valor neutro (eventos vazios, promessas resolvidas, listas vazias). A única exceção é
//!   `ITerminalInstanceService.createInstance`, que não tem valor neutro e falha com a mensagem do `TypeError`.
//! - **`MockTerminalProfileResolverService extends TestTerminalProfileResolverService`.** A subclasse só troca o perfil padrão; é o
//!   [`TestTerminalProfileResolverService::mock`] (o mesmo tipo, com o perfil do original) em vez de uma segunda struct de repasse.
//! - **`EnvironmentVariableService`.** O original guarda as coleções no `IStorageService` e as persiste; o dublê guarda só em memória
//!   (`collections`, `mergedCollection`, `onDidChangeCollections`, `set` e `delete` são os do original sem a persistência).
//! - **`{ requestWorkspaceTrust } as Partial<IWorkspaceTrustRequestService>`.** É o [`TestTerminalWorkspaceTrustRequestService::from_function`].
//! - **`Disposable` do `TestTerminalChildProcess`.** Não tem recurso a descartar; o `this._register(...)` do original não existe.
//! - **`ITerminalLogService`.** É o `NullLogService` (o `TerminalLogService` real escreve num logger que o teste não lê).

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use je_base::event::{Emitter, Event};
use je_base::errors::Error;
use je_base::map::OrderedMap;
use je_base::platform::OperatingSystem;
use je_base::process::{ProcessEnvironment, SafeProcess};
use je_base::promise::{Promise, Resolver};
use je_base::themables::ThemeIcon;
use je_base::uri::URI;
use je_platform::terminal::environment_variable::{IEnvironmentVariableCollection, IMergedEnvironmentVariableCollection};
use je_platform::terminal::environment_variable_collection::MergedEnvironmentVariableCollection;
use je_platform::terminal::terminal::{
    IProcessDataEvent, IProcessProperty, IProcessReadyEvent, IShellLaunchConfig, ITerminalChildProcess, ITerminalEnvironment, ITerminalLaunchOutcome,
    ITerminalProcessOptions, ITerminalProfile, ProcessData, ProcessPropertyType, ShellCwd, TerminalEnvironmentValue, TerminalIcon, TerminalLocation, TitleEventSource,
    UnicodeVersion,
};
use je_platform::workspace_trust::{
    IWorkspaceTrustRequestService, ResourceTrustRequestOptions, WorkspaceTrustRequestOptions, WorkspaceTrustUriResponse,
};
use je_workbench::common::editor::{EditorInputOrUntyped, GroupIdentifier};
use je_workbench::contrib::terminal::browser::terminal::{ITerminalInstance, ITerminalInstanceService, ITerminalService, ShellLaunchConfigOrProfile};
use je_workbench::contrib::terminal::common::environment_variable::IEnvironmentVariableService;
use je_workbench::contrib::terminal::common::terminal::{
    IShellLaunchConfigResolveOptions, ITerminalProfileResolverService, SharedShellLaunchConfig,
};
use je_workbench::contrib::terminal::common::terminal_backend::{IDetachInstanceRequest, IPtyHostLatencyMeasurement, ITerminalBackend};
use je_workbench::contrib::terminal::common::terminal_environment::{ITerminalBackendWslPath, WslPathDirection};
use je_workbench::services::history::history::{GoFilter, IHistoryService};

/// `TestTerminalChildProcess`.
pub struct TestTerminalChildProcess {
    should_persist: bool,
    /// O que `input(data)` recebeu (o original o descarta; o dublê o guarda para quem quiser conferir).
    pub inputs: RefCell<Vec<String>>,
}

impl TestTerminalChildProcess {
    pub fn new(should_persist: bool) -> Rc<Self> {
        Rc::new(Self { should_persist, inputs: RefCell::new(Vec::new()) })
    }
}

impl ITerminalChildProcess for TestTerminalChildProcess {
    fn id(&self) -> u32 {
        0
    }

    fn should_persist(&self) -> bool {
        self.should_persist
    }

    fn on_process_data(&self) -> Event<ProcessData> {
        Event::none()
    }

    fn on_process_ready(&self) -> Event<IProcessReadyEvent> {
        Event::none()
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

    fn input(&self, data: &str) {
        self.inputs.borrow_mut().push(data.to_owned());
    }

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
        Promise::resolved(())
    }
}

/// O estado que o `TestTerminalInstanceService` e os backends dele compartilham.
struct ProcessCreation {
    create_process_count: Cell<u32>,
    resolver: Resolver<()>,
    processes: RefCell<Vec<Rc<TestTerminalChildProcess>>>,
}

/// O objeto que `TestTerminalInstanceService.getBackend()` devolve.
struct TestTerminalBackend {
    creation: Rc<ProcessCreation>,
}

impl ITerminalBackendWslPath for TestTerminalBackend {
    fn get_wsl_path(&self, original: &str, _direction: WslPathDirection) -> Promise<String> {
        Promise::resolved(original.to_owned())
    }
}

impl ITerminalBackend for TestTerminalBackend {
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
        Event::none()
    }

    fn on_did_request_detach(&self) -> Event<IDetachInstanceRequest> {
        Event::none()
    }

    fn attach_to_process(&self, _id: u32) -> Promise<Option<Rc<dyn ITerminalChildProcess>>> {
        Promise::resolved(None)
    }

    fn attach_to_revived_process(&self, _id: u32, _pid: u32) -> Promise<Option<Rc<dyn ITerminalChildProcess>>> {
        Promise::resolved(None)
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
        self.creation.create_process_count.set(self.creation.create_process_count.get() + 1);
        self.creation.resolver.resolve(());
        let process = TestTerminalChildProcess::new(should_persist);
        self.creation.processes.borrow_mut().push(process.clone());
        let process: Rc<dyn ITerminalChildProcess> = process;
        Promise::resolved(process)
    }

    fn restart_pty_host(&self) {}
}

/// `TestTerminalInstanceService`.
pub struct TestTerminalInstanceService {
    creation: Rc<ProcessCreation>,
    process_created_promise: Promise<()>,
}

impl TestTerminalInstanceService {
    pub fn new() -> Rc<Self> {
        let (process_created_promise, resolver) = Promise::new();
        Rc::new(Self {
            creation: Rc::new(ProcessCreation { create_process_count: Cell::new(0), resolver, processes: RefCell::new(Vec::new()) }),
            process_created_promise,
        })
    }

    /// `createProcessCount`.
    pub fn create_process_count(&self) -> u32 {
        self.creation.create_process_count.get()
    }

    /// `processCreatedPromise`.
    pub fn process_created_promise(&self) -> Promise<()> {
        self.process_created_promise.clone()
    }
}

impl ITerminalInstanceService for TestTerminalInstanceService {
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
        let backend: Rc<dyn ITerminalBackend> = Rc::new(TestTerminalBackend { creation: self.creation.clone() });
        Promise::resolved(Some(backend))
    }

    fn get_registered_backends(&self) -> Vec<Rc<dyn ITerminalBackend>> {
        Vec::new()
    }

    fn did_register_backend(&self, _backend: Rc<dyn ITerminalBackend>) {}
}

/// `TestTerminalProfileResolverService` (e, com [`Self::mock`], o `MockTerminalProfileResolverService` do teste).
pub struct TestTerminalProfileResolverService {
    default_profile: ITerminalProfile,
}

impl TestTerminalProfileResolverService {
    /// `new TestTerminalProfileResolverService()`: o perfil `Default` de `/default`.
    pub fn new() -> Rc<Self> {
        Rc::new(Self { default_profile: ITerminalProfile { path: "/default".to_owned(), profile_name: "Default".to_owned(), is_default: true, ..ITerminalProfile::default() } })
    }

    /// `new MockTerminalProfileResolverService()`: o perfil `my-sh` do teste.
    pub fn mock() -> Rc<Self> {
        let env: ITerminalEnvironment = vec![("TEST".to_owned(), TerminalEnvironmentValue::Value("TEST".to_owned()))];
        Rc::new(Self {
            default_profile: ITerminalProfile {
                profile_name: "my-sh".to_owned(),
                path: "/usr/bin/zsh".to_owned(),
                env: Some(env),
                is_default: true,
                is_unsafe_path: Some(false),
                is_from_path: Some(true),
                icon: Some(TerminalIcon::Theme(ThemeIcon::from_id("terminal-linux"))),
                color: Some("terminal.ansiYellow".to_owned()),
                ..ITerminalProfile::default()
            },
        })
    }
}

impl ITerminalProfileResolverService for TestTerminalProfileResolverService {
    fn default_profile_name(&self) -> Option<String> {
        Some(String::new())
    }

    fn resolve_icon(&self, _shell_launch_config: &mut IShellLaunchConfig, _os: OperatingSystem) {}

    fn resolve_shell_launch_config(&self, _shell_launch_config: SharedShellLaunchConfig, _options: IShellLaunchConfigResolveOptions<'_>) -> Promise<()> {
        Promise::resolved(())
    }

    fn get_default_profile(&self, _options: IShellLaunchConfigResolveOptions<'_>) -> Promise<ITerminalProfile> {
        Promise::resolved(self.default_profile.clone())
    }

    fn get_default_shell(&self, _options: IShellLaunchConfigResolveOptions<'_>) -> Promise<String> {
        Promise::resolved("/default".to_owned())
    }

    fn get_default_shell_args(&self, _options: IShellLaunchConfigResolveOptions<'_>) -> Promise<je_base::arrays::OneOrMany<String>> {
        Promise::resolved(je_base::arrays::OneOrMany::Many(Vec::new()))
    }

    fn get_default_icon(&self) -> ThemeIcon {
        ThemeIcon::from_id("terminal")
    }

    fn get_environment(&self, _remote_authority: Option<&str>) -> Promise<ProcessEnvironment> {
        Promise::resolved(SafeProcess::native().env)
    }
}

/// `TestTerminalWorkspaceTrustRequestService`: conta os pedidos de confiança, e responde `false`; ou, com
/// [`Self::from_function`], responde o que a função devolve.
pub struct TestTerminalWorkspaceTrustRequestService {
    request_count: Cell<u32>,
    request: Rc<dyn Fn() -> Promise<bool>>,
}

impl TestTerminalWorkspaceTrustRequestService {
    /// `new TestTerminalWorkspaceTrustRequestService()`.
    pub fn new() -> Rc<Self> {
        Self::from_function(Rc::new(|| Promise::resolved(false)))
    }

    /// `{ requestWorkspaceTrust } as Partial<IWorkspaceTrustRequestService>`.
    pub fn from_function(request: Rc<dyn Fn() -> Promise<bool>>) -> Rc<Self> {
        Rc::new(Self { request_count: Cell::new(0), request })
    }

    /// `requestCount`.
    pub fn request_count(&self) -> u32 {
        self.request_count.get()
    }
}

impl IWorkspaceTrustRequestService for TestTerminalWorkspaceTrustRequestService {
    fn on_did_initiate_open_files_trust_request(&self) -> Event<()> {
        Event::none()
    }

    fn on_did_initiate_workspace_trust_request(&self) -> Event<Option<WorkspaceTrustRequestOptions>> {
        Event::none()
    }

    fn on_did_initiate_workspace_trust_request_on_startup(&self) -> Event<()> {
        Event::none()
    }

    fn on_did_initiate_resources_trust_request(&self) -> Event<ResourceTrustRequestOptions> {
        Event::none()
    }

    fn complete_open_files_trust_request(&self, _result: WorkspaceTrustUriResponse, _save_response: Option<bool>) -> Promise<()> {
        Promise::resolved(())
    }

    fn request_open_files_trust(&self, _open_files: &[URI]) -> Promise<WorkspaceTrustUriResponse> {
        Promise::resolved(WorkspaceTrustUriResponse::Cancel)
    }

    fn complete_resources_trust_request(&self, _uri: &URI, _result: WorkspaceTrustUriResponse) -> Promise<()> {
        Promise::resolved(())
    }

    fn request_resources_trust(&self, _options: ResourceTrustRequestOptions) -> Promise<Option<bool>> {
        Promise::resolved(None)
    }

    fn cancel_workspace_trust_request(&self) {}

    fn complete_workspace_trust_request(&self, _trusted: Option<bool>) -> Promise<()> {
        Promise::resolved(())
    }

    fn request_workspace_trust(&self, _options: Option<WorkspaceTrustRequestOptions>) -> Promise<Option<bool>> {
        self.request_count.set(self.request_count.get() + 1);
        (self.request)().then(Some)
    }

    fn request_workspace_trust_on_startup(&self) {}
}

/// `{ setNextCommandId: async () => { } } as Partial<ITerminalService>`.
pub struct TestTerminalService;

impl ITerminalService for TestTerminalService {
    fn get_instance_from_id(&self, _terminal_id: u32) -> Option<Rc<dyn ITerminalInstance>> {
        None
    }

    fn set_next_command_id(&self, _id: u32, _command_line: &str, _command_id: &str) -> Promise<()> {
        Promise::resolved(())
    }
}

/// `TestHistoryService`.
pub struct TestHistoryService {
    root: Option<URI>,
}

impl TestHistoryService {
    pub fn new(root: Option<URI>) -> Rc<Self> {
        Rc::new(Self { root })
    }
}

impl IHistoryService for TestHistoryService {
    fn go_forward(&self, _filter: Option<GoFilter>) -> Promise<()> {
        Promise::resolved(())
    }

    fn go_back(&self, _filter: Option<GoFilter>) -> Promise<()> {
        Promise::resolved(())
    }

    fn go_previous(&self, _filter: Option<GoFilter>) -> Promise<()> {
        Promise::resolved(())
    }

    fn go_last(&self, _filter: Option<GoFilter>) -> Promise<()> {
        Promise::resolved(())
    }

    fn reopen_last_closed_editor(&self) -> Promise<()> {
        Promise::resolved(())
    }

    fn get_history(&self) -> Vec<EditorInputOrUntyped> {
        Vec::new()
    }

    fn remove_from_history(&self, _input: &EditorInputOrUntyped) {}

    fn get_last_active_workspace_root(&self, _scheme_filter: Option<&str>, _authority_filter: Option<&str>) -> Option<URI> {
        self.root.clone()
    }

    fn get_last_active_file(&self, _scheme_filter: &str, _authority_filter: Option<&str>) -> Option<URI> {
        None
    }

    fn open_next_recently_used_editor(&self, _group: Option<GroupIdentifier>) -> Promise<()> {
        Promise::resolved(())
    }

    fn open_previously_used_editor(&self, _group: Option<GroupIdentifier>) -> Promise<()> {
        Promise::resolved(())
    }

    fn clear(&self) {}

    fn clear_recently_opened(&self) {}
}

/// O `EnvironmentVariableService` sem persistência (ver o cabeçalho).
pub struct TestEnvironmentVariableService {
    collections: RefCell<OrderedMap<String, IEnvironmentVariableCollection>>,
    merged_collection: RefCell<Rc<dyn IMergedEnvironmentVariableCollection>>,
    on_did_change_collections: Rc<Emitter<Rc<dyn IMergedEnvironmentVariableCollection>>>,
}

impl TestEnvironmentVariableService {
    pub fn new() -> Rc<Self> {
        let collections: OrderedMap<String, IEnvironmentVariableCollection> = OrderedMap::new();
        let merged_collection: Rc<dyn IMergedEnvironmentVariableCollection> = Rc::new(MergedEnvironmentVariableCollection::new(collections.clone()));
        Rc::new(Self { collections: RefCell::new(collections), merged_collection: RefCell::new(merged_collection), on_did_change_collections: Emitter::new() })
    }

    /// `updateCollections`: recria a coleção mesclada e avisa.
    fn update_collections(&self) {
        let merged: Rc<dyn IMergedEnvironmentVariableCollection> = Rc::new(MergedEnvironmentVariableCollection::new(self.collections.borrow().clone()));
        *self.merged_collection.borrow_mut() = merged.clone();
        self.on_did_change_collections.fire(merged);
    }
}

impl IEnvironmentVariableService for TestEnvironmentVariableService {
    fn collections(&self) -> OrderedMap<String, IEnvironmentVariableCollection> {
        self.collections.borrow().clone()
    }

    fn merged_collection(&self) -> Rc<dyn IMergedEnvironmentVariableCollection> {
        self.merged_collection.borrow().clone()
    }

    fn on_did_change_collections(&self) -> Event<Rc<dyn IMergedEnvironmentVariableCollection>> {
        self.on_did_change_collections.event()
    }

    fn set(&self, extension_identifier: &str, collection: IEnvironmentVariableCollection) {
        self.collections.borrow_mut().insert(extension_identifier.to_owned(), collection);
        self.update_collections();
    }

    fn delete(&self, extension_identifier: &str) {
        self.collections.borrow_mut().remove(&extension_identifier.to_owned());
        self.update_collections();
    }
}

/// Os pedidos de `IProcessDataEvent` que o teste cria ao simular a saída do processo.
pub fn process_data_event(data: &str, track_commit: bool) -> Rc<IProcessDataEvent> {
    IProcessDataEvent::new(data, track_commit)
}
