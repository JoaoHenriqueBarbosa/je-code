//! Dublês de `ITerminalInstance`, `ITerminalService`, `ITerminalProcessInfo`, `INotificationService` e dos serviços de hover e de
//! túnel dos testes das contribuições do terminal (`terminalLinkManager.test.ts`, `terminalLinkContribution.test.ts`,
//! `terminalClipboard.test.ts` e `terminalTelemetry.test.ts`).
//!
//! O original usa `upcastPartial<T>({ ... })` (um objeto literal cujos membros não definidos são `undefined`) e `mock<T>()`. Em
//! Rust um dublê parcial é uma struct que implementa o trait inteiro: o que o teste define vem de um campo, e o que ele não
//! define lança `panic!("<membro> is not a function")`, a mesma falha de chamar um membro de um `mock<T>()`. As exceções são os
//! membros que o literal original deixa `undefined` e que o código sob teste lê sem falhar (`ITerminalProcessInfo.os`,
//! `remoteAuthority`, `userHome` e `backend`), que valem `None`.

use std::cell::Cell;
use std::rc::Rc;

use je_base::event::{Emitter, Event};
use je_base::platform::OperatingSystem;
use je_base::promise::Promise;
use je_base::severity::Severity;
use je_base::uri::URI;
use je_platform::notification::{
    INotificationService, Notification, NotificationFilterSetting, NotificationHandle, NotificationMessage, NotificationSource, NotificationSourceFilter,
    NotificationsFilter, PromptChoiceKind, PromptOptions, StatusHandle, StatusMessageOptions,
};
use je_platform::terminal::capabilities::capabilities::ITerminalCapabilityStore;
use je_platform::terminal::terminal::{ITerminalLaunchError, TerminalExitReason, TerminalLocation, TerminalShellType};
use je_platform::terminal::environment_variable::IMergedEnvironmentVariableCollection;
use je_platform::tunnel::ITunnelService;
use je_workbench::contrib::terminal::browser::terminal::{ICreateTerminalOptions, ITerminalInstance, ITerminalService};
use je_workbench::contrib::terminal::browser::widgets::terminal_hover_widget::{ITerminalHoverService, TerminalHoverOptions};
use je_workbench::contrib::terminal::common::environment_variable::IEnvironmentVariableInfo;
use je_workbench::contrib::terminal::common::terminal::{
    IStartExtensionTerminalRequest, ITerminalProcessExtHostProxy, ITerminalProcessInfo, ProcessState, SharedShellLaunchConfig,
};
use je_workbench::contrib::terminal::common::terminal_backend::ITerminalBackend;
use je_base::lifecycle::DisposableRef;

/// `<membro> is not a function`: o membro de um `mock<T>()` que o teste não definiu.
pub fn unmocked(member: &str) -> ! {
    panic!("{member} is not a function")
}

/// `upcastPartial<ITerminalProcessInfo>({ get initialCwd() { return ''; } })`.
pub struct PartialProcessInfo {
    pub initial_cwd: String,
}

impl ITerminalProcessInfo for PartialProcessInfo {
    fn process_state(&self) -> ProcessState {
        unmocked("processState")
    }
    fn pty_process_ready(&self) -> Promise<()> {
        unmocked("ptyProcessReady")
    }
    fn shell_process_id(&self) -> Option<i32> {
        unmocked("shellProcessId")
    }
    fn remote_authority(&self) -> Option<String> {
        None
    }
    fn os(&self) -> Option<OperatingSystem> {
        None
    }
    fn user_home(&self) -> Option<String> {
        None
    }
    fn initial_cwd(&self) -> String {
        self.initial_cwd.clone()
    }
    fn environment_variable_info(&self) -> Option<Rc<dyn IEnvironmentVariableInfo>> {
        unmocked("environmentVariableInfo")
    }
    fn persistent_process_id(&self) -> Option<u32> {
        unmocked("persistentProcessId")
    }
    fn should_persist(&self) -> bool {
        unmocked("shouldPersist")
    }
    fn has_written_data(&self) -> bool {
        unmocked("hasWrittenData")
    }
    fn has_child_processes(&self) -> bool {
        unmocked("hasChildProcesses")
    }
    fn backend(&self) -> Option<Rc<dyn ITerminalBackend>> {
        None
    }
    fn capabilities(&self) -> Rc<dyn ITerminalCapabilityStore> {
        unmocked("capabilities")
    }
    fn shell_integration_nonce(&self) -> String {
        unmocked("shellIntegrationNonce")
    }
    fn ext_environment_variable_collection(&self) -> Option<Rc<dyn IMergedEnvironmentVariableCollection>> {
        unmocked("extEnvironmentVariableCollection")
    }
}

/// Um `INotificationService` cujos membros lançam.
pub struct PanickingNotificationService;

impl INotificationService for PanickingNotificationService {
    fn on_did_change_filter(&self) -> Event<()> {
        unmocked("onDidChangeFilter")
    }
    fn set_filter(&self, _filter: NotificationFilterSetting) {
        unmocked("setFilter")
    }
    fn get_filter(&self, _source: Option<&NotificationSource>) -> NotificationsFilter {
        unmocked("getFilter")
    }
    fn get_filters(&self) -> Vec<NotificationSourceFilter> {
        unmocked("getFilters")
    }
    fn remove_filter(&self, _source_id: &str) {
        unmocked("removeFilter")
    }
    fn notify(&self, _notification: Notification) -> Rc<dyn NotificationHandle> {
        unmocked("notify")
    }
    fn info(&self, _message: Vec<NotificationMessage>) {
        unmocked("info")
    }
    fn warn(&self, _message: Vec<NotificationMessage>) {
        unmocked("warn")
    }
    fn error(&self, _message: Vec<NotificationMessage>) {
        unmocked("error")
    }
    fn prompt(&self, _severity: Severity, _message: NotificationMessage, _choices: Vec<PromptChoiceKind>, _options: Option<PromptOptions>) -> Rc<dyn NotificationHandle> {
        unmocked("prompt")
    }
    fn status(&self, _message: NotificationMessage, _options: Option<StatusMessageOptions>) -> Rc<dyn StatusHandle> {
        unmocked("status")
    }
}

/// `ITunnelService` que não encaminha nada.
pub struct NoTunnelService;

impl ITunnelService for NoTunnelService {
    fn can_tunnel(&self, _uri: &URI) -> bool {
        false
    }
}

/// `upcastPartial<IHoverService>({})`: o hover nunca é mostrado.
pub struct NoHoverService;

impl ITerminalHoverService for NoHoverService {
    fn show_instant_hover(&self, _options: TerminalHoverOptions) -> Option<DisposableRef> {
        None
    }
}

/// `upcastPartial<ITerminalInstance>({ ... })` / `Object.assign(Object.create(null) as ITerminalInstance, { ... })`.
#[derive(Default)]
pub struct PartialTerminalInstance {
    pub instance_id: Option<u32>,
    pub capabilities: Option<Rc<dyn ITerminalCapabilityStore>>,
    pub resource: Option<URI>,
    pub target: Option<TerminalLocation>,
    pub process_ready: Option<Promise<()>>,
    pub on_disposed: Option<Rc<Emitter<Rc<dyn ITerminalInstance>>>>,
    pub shell_launch_config: Option<SharedShellLaunchConfig>,
    pub has_remote_authority: Option<bool>,
    pub used_shell_integration_injection: Option<bool>,
    pub session_id: Option<String>,
    /// As chamadas de `focus()`.
    pub focus_calls: Cell<u32>,
}

impl ITerminalInstance for PartialTerminalInstance {
    fn instance_id(&self) -> u32 {
        self.instance_id.unwrap_or_else(|| unmocked("instanceId"))
    }
    fn title(&self) -> String {
        unmocked("title")
    }
    fn is_disposed(&self) -> bool {
        unmocked("isDisposed")
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
        self.capabilities.clone().unwrap_or_else(|| unmocked("capabilities"))
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
        self.process_ready.clone().unwrap_or_else(|| unmocked("processReady"))
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
        unmocked("dispose")
    }
    fn resource(&self) -> URI {
        self.resource.clone().unwrap_or_else(|| unmocked("resource"))
    }
    fn has_remote_authority(&self) -> bool {
        self.has_remote_authority.unwrap_or_else(|| unmocked("hasRemoteAuthority"))
    }
    fn used_shell_integration_injection(&self) -> bool {
        self.used_shell_integration_injection.unwrap_or_else(|| unmocked("usedShellIntegrationInjection"))
    }
    fn shell_integration_injection_failure_reason(&self) -> Option<je_platform::terminal::terminal::ShellIntegrationInjectionFailureReason> {
        None
    }
    fn session_id(&self) -> String {
        self.session_id.clone().unwrap_or_else(|| unmocked("sessionId"))
    }
    fn focus(&self) {
        self.focus_calls.set(self.focus_calls.get() + 1);
    }
}

/// `upcastPartial<ITerminalService>({ onDidCreateInstance, onAnyInstanceShellTypeChanged })`.
pub struct PartialTerminalService {
    pub on_did_create_instance: Rc<Emitter<Rc<dyn ITerminalInstance>>>,
    pub on_any_instance_shell_type_changed: Rc<Emitter<Rc<dyn ITerminalInstance>>>,
}

impl ITerminalService for PartialTerminalService {
    fn instances(&self) -> Vec<Rc<dyn ITerminalInstance>> {
        unmocked("instances")
    }
    fn active_instance(&self) -> Option<Rc<dyn ITerminalInstance>> {
        unmocked("activeInstance")
    }
    fn on_did_create_instance(&self) -> Event<Rc<dyn ITerminalInstance>> {
        self.on_did_create_instance.event()
    }
    fn on_did_dispose_instance(&self) -> Event<Rc<dyn ITerminalInstance>> {
        unmocked("onDidDisposeInstance")
    }
    fn on_any_instance_process_id_ready(&self) -> Event<Rc<dyn ITerminalInstance>> {
        unmocked("onAnyInstanceProcessIdReady")
    }
    fn on_did_change_instance_dimensions(&self) -> Event<Rc<dyn ITerminalInstance>> {
        unmocked("onDidChangeInstanceDimensions")
    }
    fn on_any_instance_maximum_dimensions_change(&self) -> Event<Rc<dyn ITerminalInstance>> {
        unmocked("onAnyInstanceMaximumDimensionsChange")
    }
    fn on_did_request_start_extension_terminal(&self) -> Event<IStartExtensionTerminalRequest> {
        unmocked("onDidRequestStartExtensionTerminal")
    }
    fn on_did_change_active_instance(&self) -> Event<Option<Rc<dyn ITerminalInstance>>> {
        unmocked("onDidChangeActiveInstance")
    }
    fn on_any_instance_title_change(&self) -> Event<Rc<dyn ITerminalInstance>> {
        unmocked("onAnyInstanceTitleChange")
    }
    fn on_any_instance_data_input(&self) -> Event<Rc<dyn ITerminalInstance>> {
        unmocked("onAnyInstanceDataInput")
    }
    fn on_any_instance_shell_type_changed(&self) -> Event<Rc<dyn ITerminalInstance>> {
        self.on_any_instance_shell_type_changed.event()
    }
    fn register_process_support(&self, _is_supported: bool) {
        unmocked("registerProcessSupport")
    }
    fn set_active_instance(&self, _instance: Rc<dyn ITerminalInstance>) {
        unmocked("setActiveInstance")
    }
    fn create_terminal(&self, _options: Option<ICreateTerminalOptions>) -> Promise<Rc<dyn ITerminalInstance>> {
        unmocked("createTerminal")
    }
    fn request_start_extension_terminal(&self, _proxy: Rc<dyn ITerminalProcessExtHostProxy>, _cols: i32, _rows: i32) -> Promise<Option<ITerminalLaunchError>> {
        unmocked("requestStartExtensionTerminal")
    }
    fn get_instance_from_id(&self, _terminal_id: u32) -> Option<Rc<dyn ITerminalInstance>> {
        unmocked("getInstanceFromId")
    }
    fn set_next_command_id(&self, _id: u32, _command_line: &str, _command_id: &str) -> Promise<()> {
        unmocked("setNextCommandId")
    }
}
