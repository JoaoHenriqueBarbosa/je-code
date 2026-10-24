//! Dublês de `src/vs/workbench/contrib/terminal/test/browser/terminalService.test.ts` (e os de `workbenchTestServices.ts` que ele
//! usa): o `TestDialogService`, o `ITerminalInstanceService` com `getBackend`, `getRegisteredBackends`, `convertProfileToShellLaunchConfig`
//! e `createInstance` trocáveis (os `instantiationService.stub`), os serviços de grupos e de editores de terminal sem nenhuma
//! instância, o `ITerminalInstance` parcial (`... satisfies Partial<ITerminalInstance> as unknown as ITerminalInstance`) e o
//! `TestPersistentTerminalBackend`.
//!
//! Divergências:
//! - **`upcast` parcial.** Os contratos do porte são traits completos: o membro que o literal original não define lança
//!   `<membro> is not a function` (o `unmocked` de `terminal_contrib_support.rs`), a falha de chamar um membro de um objeto
//!   parcial. As exceções são os membros que o original deixa `undefined` e o código sob teste lê sem falhar (`staticTitle`, `icon`,
//!   `color`, `persistentProcessId`), que valem `None`.
//! - **`TerminalGroupService` e `TerminalEditorService` reais.** O `workbenchInstantiationService` do original os registra vazios (sem
//!   instância nem evento nesses casos); os dublês devolvem um host de instâncias vazio.
//! - **`Emitter<number | undefined>` do `onExit`.** O `TerminalService` só espera o evento (`on_exit_signal`), então o emissor é
//!   `Emitter<()>`.

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use je_base::event::{Emitter, Event};
use je_base::platform::OperatingSystem;
use je_base::process::ProcessEnvironment;
use je_base::promise::Promise;
use je_platform::dialogs::{Confirmation, ConfirmationResult, IDialogService, Input, InputResult, Prompt, PromptResult};
use je_platform::terminal::capabilities::capabilities::ITerminalCapabilityStore;
use je_platform::terminal::terminal::{
    IShellLaunchConfig, ITerminalChildProcess, ITerminalProcessOptions, ShellCwd, TerminalExitReason, TerminalIcon, TerminalLocation, TerminalShellType,
    TitleEventSource, UnicodeVersion,
};
use je_workbench::contrib::terminal::browser::terminal::{
    ITerminalEditorService, ITerminalGroupService, ITerminalInstance, ITerminalInstanceHost, ITerminalInstanceService, ShellLaunchConfigOrProfile,
};
use je_workbench::contrib::terminal::common::terminal::SharedShellLaunchConfig;
use je_workbench::contrib::terminal::common::terminal_backend::{IDetachInstanceRequest, IPtyHostLatencyMeasurement, ITerminalBackend};
use je_workbench::contrib::terminal::common::terminal_environment::{ITerminalBackendWslPath, WslPathDirection};

use super::terminal_contrib_support::unmocked;

/// `TestDialogService`: o `confirm` devolve o resultado definido por [`TestDialogService::set_confirm_result`].
pub struct TestDialogService {
    confirm_result: Cell<ConfirmationResult>,
}

impl TestDialogService {
    pub fn new() -> Rc<Self> {
        Rc::new(Self { confirm_result: Cell::new(ConfirmationResult { checkbox_checked: None, confirmed: false }) })
    }

    /// `setConfirmResult(result)`.
    pub fn set_confirm_result(&self, result: ConfirmationResult) {
        self.confirm_result.set(result);
    }
}

impl IDialogService for TestDialogService {
    fn on_will_show_dialog(&self) -> Event<()> {
        Event::none()
    }

    fn on_did_show_dialog(&self) -> Event<()> {
        Event::none()
    }

    fn confirm(&self, _confirmation: Confirmation) -> Promise<ConfirmationResult> {
        Promise::resolved(self.confirm_result.get())
    }

    fn prompt(&self, _prompt: Prompt) -> Promise<PromptResult> {
        Promise::resolved(PromptResult { checkbox_checked: None, result: None })
    }

    fn input(&self, _input: Input) -> Promise<InputResult> {
        Promise::resolved(InputResult { confirmation: ConfirmationResult { checkbox_checked: None, confirmed: false }, values: None })
    }

    fn info(&self, _message: &str, _detail: Option<&str>) -> Promise<()> {
        Promise::resolved(())
    }

    fn warn(&self, _message: &str, _detail: Option<&str>) -> Promise<()> {
        Promise::resolved(())
    }

    fn error(&self, _message: &str, _detail: Option<&str>) -> Promise<()> {
        Promise::resolved(())
    }

    fn about(&self) -> Promise<()> {
        Promise::resolved(())
    }
}

/// O `convertProfileToShellLaunchConfig` trocável.
pub type ConvertStub = Rc<dyn Fn(Option<ShellLaunchConfigOrProfile>, Option<ShellCwd>) -> IShellLaunchConfig>;

/// O `createInstance` trocável.
pub type CreateInstanceStub = Rc<dyn Fn(IShellLaunchConfig, TerminalLocation) -> Rc<dyn ITerminalInstance>>;

/// O `ITerminalInstanceService` do `setup`: `getBackend` devolve `undefined`, `getRegisteredBackends` uma lista vazia e os outros
/// dois membros começam como no serviço sem `stub` (a conversão devolve o `IShellLaunchConfig` vazio e o `createInstance` lança).
pub struct StubTerminalInstanceService {
    convert: RefCell<ConvertStub>,
    create: RefCell<CreateInstanceStub>,
}

impl StubTerminalInstanceService {
    pub fn new() -> Rc<Self> {
        Rc::new(Self {
            convert: RefCell::new(Rc::new(|_, _| IShellLaunchConfig::default())),
            create: RefCell::new(Rc::new(|_, _| {
                panic!("TypeError: this._terminalInstanceService.createInstance is not a function (o dublê só define getBackend)")
            })),
        })
    }

    /// `instantiationService.stub(ITerminalInstanceService, 'convertProfileToShellLaunchConfig', fn)`.
    pub fn stub_convert_profile_to_shell_launch_config(&self, convert: ConvertStub) {
        *self.convert.borrow_mut() = convert;
    }

    /// `instantiationService.stub(ITerminalInstanceService, 'createInstance', fn)`.
    pub fn stub_create_instance(&self, create: CreateInstanceStub) {
        *self.create.borrow_mut() = create;
    }
}

impl ITerminalInstanceService for StubTerminalInstanceService {
    fn on_did_create_instance(&self) -> Event<Rc<dyn ITerminalInstance>> {
        Event::none()
    }

    fn on_did_register_backend(&self) -> Event<Rc<dyn ITerminalBackend>> {
        Event::none()
    }

    fn convert_profile_to_shell_launch_config(&self, shell_launch_config_or_profile: Option<ShellLaunchConfigOrProfile>, cwd: Option<ShellCwd>) -> IShellLaunchConfig {
        let convert = self.convert.borrow().clone();
        convert(shell_launch_config_or_profile, cwd)
    }

    fn create_instance(&self, launch_config: IShellLaunchConfig, target: TerminalLocation) -> Rc<dyn ITerminalInstance> {
        let create = self.create.borrow().clone();
        create(launch_config, target)
    }

    fn get_backend(&self, _remote_authority: Option<&str>) -> Promise<Option<Rc<dyn ITerminalBackend>>> {
        Promise::resolved(None)
    }

    fn get_registered_backends(&self) -> Vec<Rc<dyn ITerminalBackend>> {
        Vec::new()
    }

    fn did_register_backend(&self, _backend: Rc<dyn ITerminalBackend>) {}
}

/// O host de instâncias dos serviços de grupos e de editores do `setup`: sem instância e sem evento.
struct EmptyInstanceHost;

impl ITerminalInstanceHost for EmptyInstanceHost {
    fn active_instance(&self) -> Option<Rc<dyn ITerminalInstance>> {
        None
    }

    fn instances(&self) -> Vec<Rc<dyn ITerminalInstance>> {
        Vec::new()
    }

    fn on_did_change_active_instance(&self) -> Event<Option<Rc<dyn ITerminalInstance>>> {
        Event::none()
    }

    fn on_did_dispose_instance(&self) -> Event<Rc<dyn ITerminalInstance>> {
        Event::none()
    }

    fn on_did_focus_instance(&self) -> Event<Rc<dyn ITerminalInstance>> {
        Event::none()
    }

    fn on_did_change_instance_capability(&self) -> Event<Rc<dyn ITerminalInstance>> {
        Event::none()
    }

    fn on_did_change_instances(&self) -> Event<()> {
        Event::none()
    }
}

/// O `TerminalGroupService` sem grupos.
pub struct EmptyTerminalGroupService;

impl ITerminalGroupService for EmptyTerminalGroupService {
    fn show_panel(&self, _focus: bool) -> Promise<()> {
        Promise::resolved(())
    }

    fn hide_panel(&self) {}

    fn instance_host(&self) -> Rc<dyn ITerminalInstanceHost> {
        Rc::new(EmptyInstanceHost)
    }

    fn on_did_change_active_group(&self) -> Event<()> {
        Event::none()
    }
}

/// O `TerminalEditorService` sem editores.
pub struct EmptyTerminalEditorService;

impl ITerminalEditorService for EmptyTerminalEditorService {
    fn reveal_active_editor(&self, _preserve_focus: bool) -> Promise<()> {
        Promise::resolved(())
    }

    fn instance_host(&self) -> Rc<dyn ITerminalInstanceHost> {
        Rc::new(EmptyInstanceHost)
    }
}

/// `{ ... } satisfies Partial<ITerminalInstance> as unknown as ITerminalInstance`: o que o teste define vem de um campo.
#[derive(Default)]
pub struct PartialServiceInstance {
    pub instance_id: Option<u32>,
    pub target: Option<TerminalLocation>,
    pub has_child_processes: Option<bool>,
    pub on_disposed: Option<Rc<Emitter<Rc<dyn ITerminalInstance>>>>,
    pub on_exit: Option<Rc<Emitter<()>>>,
    pub dispose: Option<Rc<dyn Fn()>>,
    pub persistent_process_id: Option<u32>,
    pub title: Option<String>,
    pub title_source: Option<TitleEventSource>,
    pub static_title: Option<String>,
    pub icon: Option<TerminalIcon>,
    pub color: Option<String>,
    pub is_disposed: Option<bool>,
    pub shell_launch_config: Option<SharedShellLaunchConfig>,
}

impl ITerminalInstance for PartialServiceInstance {
    fn instance_id(&self) -> u32 {
        self.instance_id.unwrap_or_else(|| unmocked("instanceId"))
    }
    fn title(&self) -> String {
        self.title.clone().unwrap_or_else(|| unmocked("title"))
    }
    fn is_disposed(&self) -> bool {
        self.is_disposed.unwrap_or_else(|| unmocked("isDisposed"))
    }
    fn shell_launch_config(&self) -> SharedShellLaunchConfig {
        self.shell_launch_config.clone().unwrap_or_else(|| unmocked("shellLaunchConfig"))
    }
    fn on_will_dispose(&self) -> Event<Rc<dyn ITerminalInstance>> {
        unmocked("onWillDispose")
    }
    fn on_disposed(&self) -> Event<Rc<dyn ITerminalInstance>> {
        self.on_disposed.as_ref().unwrap_or_else(|| unmocked("onDisposed")).event()
    }
    fn relaunch(&self) {
        unmocked("relaunch")
    }
    fn capabilities(&self) -> Rc<dyn ITerminalCapabilityStore> {
        unmocked("capabilities")
    }
    fn shell_type(&self) -> TerminalShellType {
        None
    }
    fn target(&self) -> Option<TerminalLocation> {
        self.target
    }
    fn process_id(&self) -> Option<i32> {
        unmocked("processId")
    }
    fn process_ready(&self) -> Promise<()> {
        unmocked("processReady")
    }
    fn cols(&self) -> i32 {
        unmocked("cols")
    }
    fn rows(&self) -> i32 {
        unmocked("rows")
    }
    fn max_cols(&self) -> i32 {
        unmocked("maxCols")
    }
    fn max_rows(&self) -> i32 {
        unmocked("maxRows")
    }
    fn exit_code(&self) -> Option<i32> {
        unmocked("exitCode")
    }
    fn exit_reason(&self) -> Option<TerminalExitReason> {
        unmocked("exitReason")
    }
    fn initial_data_events(&self) -> Option<Vec<String>> {
        unmocked("initialDataEvents")
    }
    fn on_data(&self) -> Event<String> {
        unmocked("onData")
    }
    fn run_command(&self, _command_line: &str, _should_execute: bool) -> Promise<()> {
        unmocked("runCommand")
    }
    fn send_text(&self, _text: &str, _should_execute: bool) -> Promise<()> {
        unmocked("sendText")
    }
    fn rename(&self, _title: Option<&str>) {
        unmocked("rename")
    }
    fn dispose_with_reason(&self, _reason: Option<TerminalExitReason>) {
        match &self.dispose {
            Some(dispose) => dispose(),
            None => unmocked("dispose"),
        }
    }
    fn has_child_processes(&self) -> bool {
        self.has_child_processes.unwrap_or_else(|| unmocked("hasChildProcesses"))
    }
    fn persistent_process_id(&self) -> Option<u32> {
        self.persistent_process_id
    }
    fn title_source(&self) -> TitleEventSource {
        self.title_source.unwrap_or_else(|| unmocked("titleSource"))
    }
    fn static_title(&self) -> Option<String> {
        self.static_title.clone()
    }
    fn icon(&self) -> Option<TerminalIcon> {
        self.icon.clone()
    }
    fn color(&self) -> Option<String> {
        self.color.clone()
    }
    fn on_exit_signal(&self) -> Event<()> {
        self.on_exit.as_ref().unwrap_or_else(|| unmocked("onExit")).event()
    }
}

/// `TestPersistentTerminalBackend`: conta as atualizações de título e de ícone e guarda o último valor.
#[derive(Default)]
pub struct TestPersistentTerminalBackend {
    pub title_update_count: Cell<u32>,
    pub icon_update_count: Cell<u32>,
    pub last_title: RefCell<Option<String>>,
    pub last_icon_user_initiated: Cell<Option<bool>>,
}

impl ITerminalBackendWslPath for TestPersistentTerminalBackend {
    fn get_wsl_path(&self, _original: &str, _direction: WslPathDirection) -> Promise<String> {
        unmocked("getWslPath")
    }
}

impl ITerminalBackend for TestPersistentTerminalBackend {
    fn remote_authority(&self) -> Option<String> {
        unmocked("remoteAuthority")
    }

    fn is_responsive(&self) -> bool {
        unmocked("isResponsive")
    }

    fn when_ready(&self) -> Promise<()> {
        unmocked("whenReady")
    }

    fn set_ready(&self) {
        unmocked("setReady")
    }

    fn on_pty_host_unresponsive(&self) -> Event<()> {
        unmocked("onPtyHostUnresponsive")
    }

    fn on_pty_host_responsive(&self) -> Event<()> {
        unmocked("onPtyHostResponsive")
    }

    fn on_pty_host_restart(&self) -> Event<()> {
        unmocked("onPtyHostRestart")
    }

    fn on_did_request_detach(&self) -> Event<IDetachInstanceRequest> {
        unmocked("onDidRequestDetach")
    }

    fn attach_to_process(&self, _id: u32) -> Promise<Option<Rc<dyn ITerminalChildProcess>>> {
        unmocked("attachToProcess")
    }

    fn attach_to_revived_process(&self, _id: u32, _pid: u32) -> Promise<Option<Rc<dyn ITerminalChildProcess>>> {
        unmocked("attachToRevivedProcess")
    }

    fn get_latency(&self) -> Promise<Vec<IPtyHostLatencyMeasurement>> {
        unmocked("getLatency")
    }

    fn get_default_system_shell(&self, _os_override: Option<OperatingSystem>) -> Promise<String> {
        unmocked("getDefaultSystemShell")
    }

    fn get_environment(&self) -> Promise<ProcessEnvironment> {
        unmocked("getEnvironment")
    }

    fn get_shell_environment(&self) -> Promise<Option<ProcessEnvironment>> {
        unmocked("getShellEnvironment")
    }

    fn update_title(&self, _id: u32, title: &str, _title_source: TitleEventSource) -> Promise<()> {
        self.title_update_count.set(self.title_update_count.get() + 1);
        *self.last_title.borrow_mut() = Some(title.to_owned());
        Promise::resolved(())
    }

    fn update_icon(&self, _id: u32, user_initiated: bool, _icon: &TerminalIcon, _color: Option<&str>) -> Promise<()> {
        self.icon_update_count.set(self.icon_update_count.get() + 1);
        self.last_icon_user_initiated.set(Some(user_initiated));
        Promise::resolved(())
    }

    fn set_next_command_id(&self, _id: u32, _command_line: &str, _command_id: &str) -> Promise<()> {
        unmocked("setNextCommandId")
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
        _should_persist: bool,
    ) -> Promise<Rc<dyn ITerminalChildProcess>> {
        unmocked("createProcess")
    }

    fn restart_pty_host(&self) {
        unmocked("restartPtyHost")
    }
}
