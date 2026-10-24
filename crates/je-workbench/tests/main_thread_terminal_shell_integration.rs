//! Porte de `src/vs/workbench/api/test/browser/mainThreadTerminalShellIntegration.test.ts` (suíte
//! `MainThreadTerminalShellIntegration`), caso a caso e na mesma ordem.
//!
//! De fora: o caso `finished commands flush data and release their resources while the service stays alive`. Ele observa, com
//! `sinon.spy(DisposableStore.prototype, 'add')`, os `Emitter` que o código registra num `DisposableStore` durante a execução
//! e confere que cada um recebe um único `dispose` no fim do comando; em Rust não há protótipo para espionar nem `dispose` de
//! `Emitter` observável por fora, e o efeito visível (os dados em lote, o evento de fim) já é conferido pelos demais casos.
//!
//! Notas de tradução:
//! - `ensureNoDisposablesAreLeakedInTestSuite()` vira o guarda criado no começo de cada caso (`store`); o corpo é síncrono.
//! - `new class extends mock<ICommandDetectionCapability>() { onCommandExecuted; onCommandFinished }`, o
//!   `mock<ITerminalInstance>()`, o `mock<ITerminalService>()` e o `mock<ITerminalCommand>()` são os dublês deste arquivo: só os
//!   membros que o caso sobrescreve existem, e qualquer outro método falha (o `TypeError` de método ausente do `mock`). O
//!   `shellType`, propriedade que o código lê e o `mock` deixa `undefined`, é `None`.
//! - `createOnInstanceEvent` e `createOnInstanceCapabilityEvent` que o `mock<ITerminalService>` sobrescreve são os do
//!   [`ITerminalServiceEvents`] (ver `terminal_events.rs`), que usam `instances`, `onDidCreateInstance` (o `Event.None` das
//!   duas sobrescritas) e `onDidDisposeInstance` do dublê, como o original passa ao multiplexador.
//! - `SingleProxyRPCProtocol(proxy)` é o `single_proxy_rpc_protocol` de `support`, `overrideExtensionService` é o
//!   `TestExtensionService` (cujo `activateByEvent` resolve já), e o `mock<IWorkbenchEnvironmentService>` não existe (o construtor
//!   do porte não o recebe).
//! - `shellLaunchConfig = {}` é o `SharedShellLaunchConfig` vazio.

#[path = "../../je-base/tests/common/mod.rs"]
mod common;
mod support;

use std::cell::RefCell;
use std::rc::Rc;

use common::ensure_no_disposables_are_leaked_in_test_suite;
use je_base::event::{Emitter, Event};
use je_base::lifecycle::IDisposable;
use je_base::promise::Promise;
use je_platform::terminal::capabilities::capabilities::{
    CommandDetection, IBaseTerminalCommand, ICommandDetectionCapability, ICommandInvalidationRequest, ICurrentPartialCommand, IHandleCommandOptions, IMarkProperties,
    IMarker, IPromptInputModel, ISerializedCommandDetectionCapability, ITerminalCapabilityStore, ITerminalCommand, TerminalCapability, TerminalCommandConfidence,
    TerminalCommandForLine,
};
use je_platform::terminal::capabilities::terminal_capability_store::TerminalCapabilityStore;
use je_platform::terminal::terminal::{
    ITerminalLaunchError, ITerminalOutputMatch, ITerminalOutputMatcher, TerminalExitReason, TerminalLocation, TerminalShellType,
};
use je_workbench::api::browser::main_thread_terminal_shell_integration::MainThreadTerminalShellIntegration;
use je_workbench::api::common::ext_host_protocol::ExtHostTerminalShellIntegrationShape;
use je_workbench::api::common::ext_host_types::TerminalShellExecutionCommandLineConfidence;
use je_workbench::contrib::terminal::browser::terminal::{ICreateTerminalOptions, ITerminalInstance, ITerminalService};
use je_workbench::contrib::terminal::common::terminal::{IStartExtensionTerminalRequest, ITerminalProcessExtHostProxy, SharedShellLaunchConfig};
use je_workbench::test::browser::workbench_test_services::TestExtensionService;
use support::test_rpc_protocol::single_proxy_rpc_protocol;

/// O que um membro de `mock<T>()` que ninguém sobrescreveu faz: lançar.
fn unmocked(name: &str) -> ! {
    panic!("{name} is not a function")
}

/// `new class extends mock<ICommandDetectionCapability>() { override onCommandExecuted = executed.event; override onCommandFinished = finished.event; }`.
struct MockCommandDetection {
    executed: Event<Rc<dyn ITerminalCommand>>,
    finished: Event<Rc<dyn ITerminalCommand>>,
}

impl ICommandDetectionCapability for MockCommandDetection {
    fn capability_type(&self) -> TerminalCapability {
        unmocked("type")
    }
    fn prompt_input_model(&self) -> Rc<dyn IPromptInputModel> {
        unmocked("promptInputModel")
    }
    fn commands(&self) -> Vec<Rc<dyn ITerminalCommand>> {
        unmocked("commands")
    }
    fn executing_command(&self) -> Option<String> {
        unmocked("executingCommand")
    }
    fn executing_command_object(&self) -> Option<Rc<dyn ITerminalCommand>> {
        unmocked("executingCommandObject")
    }
    fn executing_command_confidence(&self) -> Option<TerminalCommandConfidence> {
        unmocked("executingCommandConfidence")
    }
    fn cwd(&self) -> Option<String> {
        unmocked("cwd")
    }
    fn has_rich_command_detection(&self) -> bool {
        unmocked("hasRichCommandDetection")
    }
    fn current_command(&self) -> Option<Rc<dyn ICurrentPartialCommand>> {
        unmocked("currentCommand")
    }
    fn on_command_started(&self) -> Event<Rc<dyn ITerminalCommand>> {
        unmocked("onCommandStarted")
    }
    fn on_command_finished(&self) -> Event<Rc<dyn ITerminalCommand>> {
        self.finished.clone()
    }
    fn on_command_executed(&self) -> Event<Rc<dyn ITerminalCommand>> {
        self.executed.clone()
    }
    fn on_command_invalidated(&self) -> Event<Vec<Rc<dyn ITerminalCommand>>> {
        unmocked("onCommandInvalidated")
    }
    fn on_current_command_invalidated(&self) -> Event<ICommandInvalidationRequest> {
        unmocked("onCurrentCommandInvalidated")
    }
    fn on_set_rich_command_detection(&self) -> Event<bool> {
        unmocked("onSetRichCommandDetection")
    }
    fn set_continuation_prompt(&self, _value: &str) {
        unmocked("setContinuationPrompt")
    }
    fn set_prompt_terminator(&self, _value: &str, _last_prompt_line: &str) {
        unmocked("setPromptTerminator")
    }
    fn set_cwd(&self, _value: &str) {
        unmocked("setCwd")
    }
    fn set_is_windows_pty(&self, _value: bool) {
        unmocked("setIsWindowsPty")
    }
    fn set_is_command_storage_disabled(&self) {
        unmocked("setIsCommandStorageDisabled")
    }
    fn get_cwd_for_line(&self, _line: f64) -> Option<String> {
        unmocked("getCwdForLine")
    }
    fn get_command_for_line(&self, _line: f64) -> Option<TerminalCommandForLine> {
        unmocked("getCommandForLine")
    }
    fn clear_commands(&self) {
        unmocked("clearCommands")
    }
    fn handle_prompt_start(&self, _options: Option<&IHandleCommandOptions>) {
        unmocked("handlePromptStart")
    }
    fn handle_continuation_start(&self) {
        unmocked("handleContinuationStart")
    }
    fn handle_continuation_end(&self) {
        unmocked("handleContinuationEnd")
    }
    fn handle_right_prompt_start(&self) {
        unmocked("handleRightPromptStart")
    }
    fn handle_right_prompt_end(&self) {
        unmocked("handleRightPromptEnd")
    }
    fn handle_command_start(&self, _options: Option<&IHandleCommandOptions>) {
        unmocked("handleCommandStart")
    }
    fn handle_command_executed(&self, _options: Option<&IHandleCommandOptions>) {
        unmocked("handleCommandExecuted")
    }
    fn handle_command_finished(&self, _exit_code: Option<f64>, _options: Option<&IHandleCommandOptions>) {
        unmocked("handleCommandFinished")
    }
    fn set_has_rich_command_detection(&self, _value: bool) {
        unmocked("setHasRichCommandDetection")
    }
    fn set_command_line(&self, _command_line: &str, _is_trusted: bool) {
        unmocked("setCommandLine")
    }
    fn set_next_command_id(&self, _command: &str, _command_id: &str) {
        unmocked("setNextCommandId")
    }
    fn serialize(&self) -> ISerializedCommandDetectionCapability {
        unmocked("serialize")
    }
    fn deserialize(&self, _serialized: &ISerializedCommandDetectionCapability) {
        unmocked("deserialize")
    }
}

/// `new class extends mock<ITerminalInstance>() { instanceId; capabilities; onData; shellLaunchConfig = {} }`.
struct MockInstance {
    instance_id: u32,
    capabilities: Rc<TerminalCapabilityStore>,
    data: Rc<Emitter<String>>,
    shell_launch_config: SharedShellLaunchConfig,
}

impl ITerminalInstance for MockInstance {
    fn instance_id(&self) -> u32 {
        self.instance_id
    }
    fn title(&self) -> String {
        unmocked("title")
    }
    fn is_disposed(&self) -> bool {
        unmocked("isDisposed")
    }
    fn shell_launch_config(&self) -> SharedShellLaunchConfig {
        self.shell_launch_config.clone()
    }
    fn on_will_dispose(&self) -> Event<Rc<dyn ITerminalInstance>> {
        unmocked("onWillDispose")
    }
    fn on_disposed(&self) -> Event<Rc<dyn ITerminalInstance>> {
        unmocked("onDisposed")
    }
    fn relaunch(&self) {
        unmocked("relaunch")
    }
    fn capabilities(&self) -> Rc<dyn ITerminalCapabilityStore> {
        self.capabilities.clone()
    }
    fn shell_type(&self) -> TerminalShellType {
        None
    }
    fn target(&self) -> Option<TerminalLocation> {
        unmocked("target")
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
        self.data.event()
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
}

/// `new class extends mock<ITerminalService>() { instances; onDidDisposeInstance }`.
struct MockTerminalService {
    instances: Vec<Rc<dyn ITerminalInstance>>,
    disposed: Event<Rc<dyn ITerminalInstance>>,
}

impl ITerminalService for MockTerminalService {
    fn instances(&self) -> Vec<Rc<dyn ITerminalInstance>> {
        self.instances.clone()
    }
    fn active_instance(&self) -> Option<Rc<dyn ITerminalInstance>> {
        unmocked("activeInstance")
    }
    /// O `Event.None` que as sobrescritas de `createOnInstanceEvent` e `createOnInstanceCapabilityEvent` passam.
    fn on_did_create_instance(&self) -> Event<Rc<dyn ITerminalInstance>> {
        Event::none()
    }
    fn on_did_dispose_instance(&self) -> Event<Rc<dyn ITerminalInstance>> {
        self.disposed.clone()
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
        unmocked("onAnyInstanceShellTypeChanged")
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

/// `new class extends mock<ExtHostTerminalShellIntegrationShape>() { ... }`: registra os eventos que o caso observa.
struct MockExtHost {
    events: Rc<RefCell<Vec<String>>>,
}

impl ExtHostTerminalShellIntegrationShape for MockExtHost {
    fn rpc_shell_integration_change(&self, _instance_id: u32, _supports_execute_command_api: bool) {}

    fn rpc_shell_execution_start(
        &self,
        instance_id: u32,
        _supports_execute_command_api: bool,
        _command_line_value: &str,
        _command_line_confidence: TerminalShellExecutionCommandLineConfidence,
        _is_trusted: bool,
        _cwd: Option<&str>,
    ) {
        self.events.borrow_mut().push(format!("{instance_id}:start"));
    }

    fn rpc_shell_execution_end(
        &self,
        instance_id: u32,
        _command_line_value: &str,
        _command_line_confidence: TerminalShellExecutionCommandLineConfidence,
        _is_trusted: bool,
        _exit_code: Option<i32>,
    ) {
        self.events.borrow_mut().push(format!("{instance_id}:end"));
    }

    fn rpc_shell_execution_data(&self, instance_id: u32, data: &str) {
        self.events.borrow_mut().push(format!("{instance_id}:data:{data}"));
    }

    fn rpc_shell_env_change(&self, _instance_id: u32, _shell_env_keys: &[String], _shell_env_values: &[Option<String>], _is_trusted: bool) {
        unmocked("$shellEnvChange")
    }

    fn rpc_cwd_change(&self, _instance_id: u32, _cwd: Option<&str>) {
        unmocked("$cwdChange")
    }

    fn rpc_close_terminal(&self, instance_id: u32) {
        self.events.borrow_mut().push(format!("{instance_id}:close"));
    }
}

/// `command()`: `new class extends mock<ITerminalCommand>() { command = 'echo test'; commandLineConfidence = 'high'; isTrusted = true; exitCode = 0; }`.
struct MockCommand;

impl IBaseTerminalCommand for MockCommand {
    fn command(&self) -> String {
        "echo test".to_owned()
    }
    fn set_command(&self, _command: &str) {
        unmocked("command")
    }
    fn command_line_confidence(&self) -> TerminalCommandConfidence {
        TerminalCommandConfidence::High
    }
    fn set_command_line_confidence(&self, _confidence: TerminalCommandConfidence) {
        unmocked("commandLineConfidence")
    }
    fn is_trusted(&self) -> bool {
        true
    }
    fn timestamp(&self) -> f64 {
        unmocked("timestamp")
    }
    fn duration(&self) -> f64 {
        unmocked("duration")
    }
    fn id(&self) -> Option<String> {
        unmocked("id")
    }
    /// `cwd` não é definido pelo `mock`: `undefined`.
    fn cwd(&self) -> Option<String> {
        None
    }
    fn exit_code(&self) -> Option<f64> {
        Some(0.0)
    }
    fn command_start_line_content(&self) -> Option<String> {
        unmocked("commandStartLineContent")
    }
    fn mark_properties(&self) -> Option<IMarkProperties> {
        unmocked("markProperties")
    }
    fn executed_x(&self) -> Option<f64> {
        unmocked("executedX")
    }
    fn start_x(&self) -> Option<f64> {
        unmocked("startX")
    }
}

impl ITerminalCommand for MockCommand {
    fn prompt_start_marker(&self) -> Option<Rc<dyn IMarker>> {
        unmocked("promptStartMarker")
    }
    fn marker(&self) -> Option<Rc<dyn IMarker>> {
        unmocked("marker")
    }
    fn end_marker(&self) -> Option<Rc<dyn IMarker>> {
        unmocked("endMarker")
    }
    fn set_end_marker(&self, _marker: Option<Rc<dyn IMarker>>) {
        unmocked("endMarker")
    }
    fn executed_marker(&self) -> Option<Rc<dyn IMarker>> {
        unmocked("executedMarker")
    }
    fn aliases(&self) -> Option<Vec<Vec<String>>> {
        unmocked("aliases")
    }
    fn was_replayed(&self) -> Option<bool> {
        unmocked("wasReplayed")
    }
    fn extract_command_line(&self) -> String {
        unmocked("extractCommandLine")
    }
    fn get_output(&self) -> Option<String> {
        unmocked("getOutput")
    }
    fn get_output_match(&self, _output_matcher: &ITerminalOutputMatcher) -> Option<ITerminalOutputMatch> {
        unmocked("getOutputMatch")
    }
    fn has_output(&self) -> bool {
        unmocked("hasOutput")
    }
    fn get_prompt_row_count(&self) -> f64 {
        unmocked("getPromptRowCount")
    }
    fn get_command_row_count(&self) -> f64 {
        unmocked("getCommandRowCount")
    }
}

fn command() -> Rc<dyn ITerminalCommand> {
    Rc::new(MockCommand)
}

/// `createTerminal(instanceId)`.
struct Terminal {
    instance: Rc<dyn ITerminalInstance>,
    data: Rc<Emitter<String>>,
    executed: Rc<Emitter<Rc<dyn ITerminalCommand>>>,
    finished: Rc<Emitter<Rc<dyn ITerminalCommand>>>,
}

fn create_terminal(store: &common::LeakGuard, instance_id: u32) -> Terminal {
    let data = store.add_rc(Emitter::<String>::new());
    let executed = store.add_rc(Emitter::<Rc<dyn ITerminalCommand>>::new());
    let finished = store.add_rc(Emitter::<Rc<dyn ITerminalCommand>>::new());
    let capabilities = store.add_rc(TerminalCapabilityStore::new());
    capabilities.add::<CommandDetection>(Rc::new(MockCommandDetection { executed: executed.event(), finished: finished.event() }) as Rc<dyn ICommandDetectionCapability>);
    let instance: Rc<dyn ITerminalInstance> = Rc::new(MockInstance { instance_id, capabilities, data: data.clone(), shell_launch_config: Rc::default() });
    Terminal { instance, data, executed, finished }
}

/// `createIntegration(instances)`.
struct Integration {
    integration: Rc<MainThreadTerminalShellIntegration>,
    disposed: Rc<Emitter<Rc<dyn ITerminalInstance>>>,
    events: Rc<RefCell<Vec<String>>>,
}

fn create_integration(store: &common::LeakGuard, instances: Vec<Rc<dyn ITerminalInstance>>) -> Integration {
    let disposed = store.add_rc(Emitter::<Rc<dyn ITerminalInstance>>::new());
    let events: Rc<RefCell<Vec<String>>> = Rc::default();
    let terminal_service: Rc<dyn ITerminalService> = Rc::new(MockTerminalService { instances, disposed: disposed.event() });
    let proxy: Rc<dyn ExtHostTerminalShellIntegrationShape> = Rc::new(MockExtHost { events: events.clone() });
    let integration = store.add_rc(MainThreadTerminalShellIntegration::new(&*single_proxy_rpc_protocol(proxy), terminal_service, TestExtensionService::new()));
    Integration { integration, disposed, events }
}

#[test]
fn terminal_disposal_flushes_pending_data_before_closing_the_extension_stream() {
    let store = ensure_no_disposables_are_leaked_in_test_suite();
    let terminal = create_terminal(&store, 1);
    let Integration { disposed, events, .. } = create_integration(&store, vec![terminal.instance.clone()]);
    terminal.executed.fire(command());
    terminal.data.fire("pending".to_owned());
    disposed.fire(terminal.instance.clone());
    assert_eq!(*events.borrow(), vec!["1:start", "1:data:pending", "1:close"]);
    assert!(!terminal.data.has_listeners());
}

#[test]
fn finishing_one_terminal_keeps_another_terminal_stream_active() {
    let store = ensure_no_disposables_are_leaked_in_test_suite();
    let first = create_terminal(&store, 1);
    let second = create_terminal(&store, 2);
    let Integration { events, .. } = create_integration(&store, vec![first.instance.clone(), second.instance.clone()]);
    let first_command = command();
    let second_command = command();
    first.executed.fire(first_command.clone());
    second.executed.fire(second_command.clone());
    first.data.fire("first".to_owned());
    second.data.fire("second".to_owned());
    first.finished.fire(first_command);
    second.data.fire(" still running".to_owned());
    second.finished.fire(second_command);
    assert_eq!(*events.borrow(), vec!["1:start", "2:start", "1:data:first", "1:end", "2:data:second still running", "2:end"]);
}

#[test]
fn replacing_a_command_flushes_its_data_and_removes_the_old_data_listener() {
    let store = ensure_no_disposables_are_leaked_in_test_suite();
    let terminal = create_terminal(&store, 1);
    let Integration { events, .. } = create_integration(&store, vec![terminal.instance.clone()]);
    terminal.executed.fire(command());
    terminal.data.fire("old".to_owned());
    let current = command();
    terminal.executed.fire(current.clone());
    terminal.data.fire("new".to_owned());
    terminal.finished.fire(current);
    let data_events: Vec<String> = events.borrow().iter().filter(|event| event.starts_with("1:data:")).cloned().collect();
    assert_eq!(data_events, vec!["1:data:old", "1:data:new"]);
    assert!(!terminal.data.has_listeners());
}

#[test]
fn duplicate_start_notifications_do_not_duplicate_output() {
    let store = ensure_no_disposables_are_leaked_in_test_suite();
    let terminal = create_terminal(&store, 1);
    let Integration { events, .. } = create_integration(&store, vec![terminal.instance.clone()]);
    let current = command();
    terminal.executed.fire(current.clone());
    terminal.executed.fire(current.clone());
    terminal.data.fire("once".to_owned());
    terminal.finished.fire(current);
    assert_eq!(*events.borrow(), vec!["1:start", "1:data:once", "1:end"]);
}

#[test]
fn service_disposal_releases_active_terminal_streams_and_flushes_pending_output() {
    let store = ensure_no_disposables_are_leaked_in_test_suite();
    let first = create_terminal(&store, 1);
    let second = create_terminal(&store, 2);
    let Integration { integration, events, .. } = create_integration(&store, vec![first.instance.clone(), second.instance.clone()]);
    first.executed.fire(command());
    second.executed.fire(command());
    first.data.fire("first".to_owned());
    second.data.fire("second".to_owned());
    integration.dispose();
    assert_eq!(*events.borrow(), vec!["1:start", "2:start", "1:data:first", "2:data:second"]);
    assert_eq!((first.data.has_listeners(), second.data.has_listeners()), (false, false));
}
