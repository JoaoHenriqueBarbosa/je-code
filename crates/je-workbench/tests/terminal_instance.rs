//! Porte de `src/vs/workbench/contrib/terminal/test/browser/terminalInstance.test.ts` (suíte `Workbench - TerminalInstance`), caso a caso e na mesma ordem.
//!
//! Casos de fora (os 23 do `TerminalInstance` e os 7 do `getCwdResource`), que dependem do `TerminalInstance` real (o
//! emulador de terminal da Fase 6, o DOM, o processo e o `workbenchInstantiationService` inteiro), ainda não portados:
//! - `TerminalInstance`: `removed`/`active horizontal scrollbars are released ...` (2), `repeated horizontal scrollbar toggles
//!   ...`, `horizontal scrollbar removal does not affect another terminal`, `should create an instance of TerminalInstance
//!   with env from default profile`, `marked remote resolver terminal bypasses workspace trust request`, `unmarked terminal
//!   requests workspace trust`, `reuseTerminal requests workspace trust ...`, `should not create a process when workspace
//!   trust is denied`, `should not create a process with an unexpected cwd in an empty workspace`, `should preserve title for
//!   task terminals`, `should only load the line data event addon once ...`, `clearing a custom title with ... restores
//!   process title updates` (variações do laço) e `... restores shell title sequence updates`, `should preserve agent shell
//!   type ...`, `should detect Command Code agent shell type ...`, `runCommand should wait for prompt input ...`, `should fire
//!   onWillDispose ...`, `should dispose contribution-owned xterm addons ...`, `should stop handling file drops ...`,
//!   `should not start handling file drops ...` e os 2 `custom key event handler ...`;
//! - `getCwdResource` (7 casos): é um método do `TerminalInstance`.
//!
//! Portados: `DEFAULT_COMMANDS_TO_SKIP_SHELL` (1), `parseExitResult` (11) e `TerminalLabelComputer` (21).
//!
//! Divergências:
//! - **`workbenchInstantiationService(undefined, store)`.** O `TerminalLabelComputer` só lê o `IFileService`, o
//!   `ITerminalConfigurationService` e o `IWorkspaceContextService`; o `InstantiationService` do caso tem esses três (o
//!   `TestFileService`, o `TerminalConfigurationService` sobre o `TestConfigurationService(configuration)` e o
//!   `TestContextService` com o `TestWorkspace`), como o `createLabelComputer` do original.
//! - **`capabilities.add(TerminalCapability.NaiveCwdDetection, null!)`.** É uma implementação nula
//!   ([`NullNaiveCwdDetection`]) da capacidade, porque a união tipada não aceita `null`.
//! - **`createInstance(partial)`.** É [`create_instance`], que recebe uma função que altera os campos do [`LabelInstance`].
//! - **`ITerminalLaunchError | number`.** É [`ExitCodeOrError`] em um `Option`, e `deepStrictEqual` sobre
//!   `{ code, message }` é `assert_eq!` sobre o [`ExitResult`].
//! - **`workspaceFolder: { uri } as IWorkspaceFolder`.** O `WorkspaceFolder` do porte tem `name` e `index`; o nome fica
//!   vazio, que vale como ausente (o nome base do caminho é o que o `${workspaceFolder}` resolve).
//! - **`fixPath`.** No Linux é a identidade (`isWindows` falso), e os ramos `isWindows` mantêm o `if` em tempo de execução.
//! - **`ensureNoDisposablesAreLeakedInTestSuite()`.** É o guarda criado no começo de cada caso.

#[path = "../../je-base/tests/common/mod.rs"]
mod common;

use std::rc::Rc;

use common::{LeakGuard, ensure_no_disposables_are_leaked_in_test_suite};
use je_base::arrays::OneOrMany;
use je_base::dynamic::Dynamic;
use je_base::event::Event;
use je_base::platform::{OperatingSystem, Platform};
use je_base::promise::Promise;
use je_base::uri::{URI, UriComponents};
use je_instantiation::{InstantiationService, ServiceCollection};
use je_platform::configuration::service::{I_CONFIGURATION_SERVICE, IConfigurationService};
use je_platform::configuration::test_configuration_service::TestConfigurationService;
use je_platform::files::i_file_service::{I_FILE_SERVICE, IFileService};
use je_platform::terminal::capabilities::capabilities::{INaiveCwdDetectionCapability, NaiveCwdDetection, TerminalCapability};
use je_platform::terminal::capabilities::terminal_capability_store::TerminalCapabilityStore;
use je_platform::terminal::terminal::{GeneralShellType, IShellLaunchConfig, ITerminalLaunchError, ShellType, TerminalType};
use je_platform::workspace::{I_WORKSPACE_CONTEXT_SERVICE, IWorkspaceContextService, WorkspaceFolder};
use je_workbench::contrib::terminal::browser::terminal::{I_TERMINAL_CONFIGURATION_SERVICE, ITerminalConfigurationService};
use je_workbench::contrib::terminal::browser::terminal_configuration_service::TerminalConfigurationService;
use je_workbench::contrib::terminal::browser::terminal_instance::{ExitCodeOrError, ExitResult, LabelInstance, TerminalLabelComputer, parse_exit_result};
use je_workbench::contrib::terminal::common::terminal::{ProcessState, default_commands_to_skip_shell};
use je_workbench::test::common::workbench_test_services::{TestContextService, TestFileService};

/// `fixPath('/foo/root1')`.
const ROOT_1: &str = "/foo/root1";
/// `fixPath('/foo/root2')`.
const ROOT_2: &str = "/foo/root2";

fn is_windows() -> bool {
    Platform::detect().is_windows
}

// ---------------------------------------------------------------------------------------------------------------------
// suite('DEFAULT_COMMANDS_TO_SKIP_SHELL')
// ---------------------------------------------------------------------------------------------------------------------

#[test]
fn default_commands_to_skip_shell_should_include_zoom_commands_so_they_are_not_consumed_by_kitty_keyboard_protocol() {
    let commands = default_commands_to_skip_shell();
    assert!(["workbench.action.zoomIn", "workbench.action.zoomOut", "workbench.action.zoomReset"].iter().all(|command| commands.contains(command)));
}

// ---------------------------------------------------------------------------------------------------------------------
// suite('parseExitResult')
// ---------------------------------------------------------------------------------------------------------------------

/// `{ code, message }`.
fn result(code: Option<i32>, message: Option<&str>) -> ExitResult {
    ExitResult { code, message: message.map(str::to_owned) }
}

/// `{ executable, args }`.
fn launch_config(executable: Option<&str>, args: Option<OneOrMany<String>>) -> IShellLaunchConfig {
    IShellLaunchConfig { executable: executable.map(str::to_owned), args, ..IShellLaunchConfig::default() }
}

fn code(value: i32) -> ExitCodeOrError {
    ExitCodeOrError::Code(value)
}

/// `{ code?, message }`.
fn error(code: Option<i32>, message: &str) -> ExitCodeOrError {
    ExitCodeOrError::Error(ITerminalLaunchError { message: message.to_owned(), code })
}

#[test]
fn parse_exit_result_should_return_no_message_for_exit_code_undefined() {
    let _store = ensure_no_disposables_are_leaked_in_test_suite();
    let config = launch_config(None, None);
    assert_eq!(parse_exit_result(None, &config, ProcessState::KilledDuringLaunch, None), result(None, None));
    assert_eq!(parse_exit_result(None, &config, ProcessState::KilledByUser, None), result(None, None));
    assert_eq!(parse_exit_result(None, &config, ProcessState::KilledByProcess, None), result(None, None));
}

#[test]
fn parse_exit_result_should_return_no_message_for_exit_code_0() {
    let _store = ensure_no_disposables_are_leaked_in_test_suite();
    let config = launch_config(None, None);
    assert_eq!(parse_exit_result(Some(&code(0)), &config, ProcessState::KilledDuringLaunch, None), result(Some(0), None));
    assert_eq!(parse_exit_result(Some(&code(0)), &config, ProcessState::KilledByUser, None), result(Some(0), None));
    assert_eq!(parse_exit_result(Some(&code(0)), &config, ProcessState::KilledDuringLaunch, None), result(Some(0), None));
}

#[test]
fn parse_exit_result_should_return_friendly_message_when_executable_is_specified_for_non_zero_exit_codes() {
    let _store = ensure_no_disposables_are_leaked_in_test_suite();
    let config = launch_config(Some("foo"), None);
    assert_eq!(
        parse_exit_result(Some(&code(1)), &config, ProcessState::KilledDuringLaunch, None),
        result(Some(1), Some("The terminal process \"foo\" failed to launch (exit code: 1)."))
    );
    assert_eq!(
        parse_exit_result(Some(&code(1)), &config, ProcessState::KilledByUser, None),
        result(Some(1), Some("The terminal process \"foo\" terminated with exit code: 1."))
    );
    assert_eq!(
        parse_exit_result(Some(&code(1)), &config, ProcessState::KilledByProcess, None),
        result(Some(1), Some("The terminal process \"foo\" terminated with exit code: 1."))
    );
}

#[test]
fn parse_exit_result_should_return_friendly_message_when_executable_and_args_are_specified_for_non_zero_exit_codes() {
    let _store = ensure_no_disposables_are_leaked_in_test_suite();
    let config = launch_config(Some("foo"), Some(OneOrMany::Many(vec!["bar".to_owned(), "baz".to_owned()])));
    assert_eq!(
        parse_exit_result(Some(&code(1)), &config, ProcessState::KilledDuringLaunch, None),
        result(Some(1), Some("The terminal process \"foo 'bar', 'baz'\" failed to launch (exit code: 1)."))
    );
    assert_eq!(
        parse_exit_result(Some(&code(1)), &config, ProcessState::KilledByUser, None),
        result(Some(1), Some("The terminal process \"foo 'bar', 'baz'\" terminated with exit code: 1."))
    );
    assert_eq!(
        parse_exit_result(Some(&code(1)), &config, ProcessState::KilledByProcess, None),
        result(Some(1), Some("The terminal process \"foo 'bar', 'baz'\" terminated with exit code: 1."))
    );
}

#[test]
fn parse_exit_result_should_return_friendly_message_when_executable_and_arguments_are_omitted_for_non_zero_exit_codes() {
    let _store = ensure_no_disposables_are_leaked_in_test_suite();
    let config = launch_config(None, None);
    assert_eq!(
        parse_exit_result(Some(&code(1)), &config, ProcessState::KilledDuringLaunch, None),
        result(Some(1), Some("The terminal process failed to launch (exit code: 1)."))
    );
    assert_eq!(
        parse_exit_result(Some(&code(1)), &config, ProcessState::KilledByUser, None),
        result(Some(1), Some("The terminal process terminated with exit code: 1."))
    );
    assert_eq!(
        parse_exit_result(Some(&code(1)), &config, ProcessState::KilledByProcess, None),
        result(Some(1), Some("The terminal process terminated with exit code: 1."))
    );
}

#[test]
fn parse_exit_result_should_ignore_pty_host_related_errors() {
    let _store = ensure_no_disposables_are_leaked_in_test_suite();
    assert_eq!(
        parse_exit_result(Some(&error(None, "Could not find pty with id 16")), &launch_config(None, None), ProcessState::KilledDuringLaunch, None),
        result(None, None)
    );
}

#[test]
fn parse_exit_result_should_format_conpty_failure_code_5() {
    let _store = ensure_no_disposables_are_leaked_in_test_suite();
    assert_eq!(
        parse_exit_result(
            Some(&error(Some(5), "A native exception occurred during launch (Cannot create process, error code: 5)")),
            &launch_config(Some("foo"), None),
            ProcessState::KilledDuringLaunch,
            None
        ),
        result(
            Some(5),
            Some("The terminal process failed to launch: Access was denied to the path containing your executable \"foo\". Manage and change your permissions to get this to work.")
        )
    );
}

#[test]
fn parse_exit_result_should_format_conpty_failure_code_267() {
    let _store = ensure_no_disposables_are_leaked_in_test_suite();
    assert_eq!(
        parse_exit_result(
            Some(&error(Some(267), "A native exception occurred during launch (Cannot create process, error code: 267)")),
            &launch_config(None, None),
            ProcessState::KilledDuringLaunch,
            Some("/foo")
        ),
        result(Some(267), Some("The terminal process failed to launch: Invalid starting directory \"/foo\", review your terminal.integrated.cwd setting."))
    );
}

#[test]
fn parse_exit_result_should_format_conpty_failure_code_1260() {
    let _store = ensure_no_disposables_are_leaked_in_test_suite();
    assert_eq!(
        parse_exit_result(
            Some(&error(Some(1260), "A native exception occurred during launch (Cannot create process, error code: 1260)")),
            &launch_config(Some("foo"), None),
            ProcessState::KilledDuringLaunch,
            None
        ),
        result(
            Some(1260),
            Some("The terminal process failed to launch: Windows cannot open this program because it has been prevented by a software restriction policy. For more information, open Event Viewer or contact your system Administrator.")
        )
    );
}

#[test]
fn parse_exit_result_should_format_conpty_launch_failure() {
    let _store = ensure_no_disposables_are_leaked_in_test_suite();
    assert_eq!(
        parse_exit_result(
            Some(&error(None, "A native exception occurred during launch (Cannot launch conpty). Winpty has been removed, see https://code.visualstudio.com/updates/v1_109#_removal-of-winpty-support for more details. You can also try enabling the `terminal.integrated.windowsUseConptyDll` setting.")),
            &launch_config(None, None),
            ProcessState::KilledDuringLaunch,
            None
        ),
        result(
            None,
            Some("The terminal process failed to launch: A native exception occurred during launch (Cannot launch conpty). Winpty has been removed, see https://code.visualstudio.com/updates/v1_109#_removal-of-winpty-support for more details. You can also try enabling the `terminal.integrated.windowsUseConptyDll` setting..")
        )
    );
}

#[test]
fn parse_exit_result_should_format_generic_failures() {
    let _store = ensure_no_disposables_are_leaked_in_test_suite();
    assert_eq!(
        parse_exit_result(
            Some(&error(Some(123), "A native exception occurred during launch (Cannot create process, error code: 123)")),
            &launch_config(None, None),
            ProcessState::KilledDuringLaunch,
            None
        ),
        result(Some(123), Some("The terminal process failed to launch: A native exception occurred during launch (Cannot create process, error code: 123)."))
    );
    assert_eq!(
        parse_exit_result(Some(&error(Some(123), "foo")), &launch_config(None, None), ProcessState::KilledDuringLaunch, None),
        result(Some(123), Some("The terminal process failed to launch: foo."))
    );
}

// ---------------------------------------------------------------------------------------------------------------------
// suite('TerminalLabelComputer')
// ---------------------------------------------------------------------------------------------------------------------

/// A implementação nula de `capabilities.add(TerminalCapability.NaiveCwdDetection, null!)`.
struct NullNaiveCwdDetection;

impl INaiveCwdDetectionCapability for NullNaiveCwdDetection {
    fn capability_type(&self) -> TerminalCapability {
        TerminalCapability::NaiveCwdDetection
    }

    fn on_did_change_cwd(&self) -> Event<String> {
        Event::none()
    }

    fn get_cwd(&self) -> Promise<String> {
        Promise::resolved(String::new())
    }
}

/// O `TerminalCapabilityStore` do `setup`/`createInstance`, com a detecção ingênua de cwd fora do Windows.
fn create_capabilities(store: &LeakGuard) -> Rc<TerminalCapabilityStore> {
    let capabilities = store.add_rc(TerminalCapabilityStore::new());
    if !is_windows() {
        capabilities.add::<NaiveCwdDetection>(Rc::new(NullNaiveCwdDetection));
    }
    capabilities
}

/// `createInstance(partial)`.
fn create_instance(store: &LeakGuard, partial: impl FnOnce(&mut LabelInstance)) -> LabelInstance {
    let mut instance = LabelInstance {
        shell_launch_config: IShellLaunchConfig::default(),
        shell_type: Some(ShellType::General(GeneralShellType::PowerShell)),
        cwd: Some("cwd".to_owned()),
        fixed_cols: None,
        fixed_rows: None,
        initial_cwd: None,
        process_name: String::new(),
        sequence: None,
        user_home: Some("/home/user".to_owned()),
        workspace_folder: None,
        static_title: None,
        capabilities: create_capabilities(store),
        title: String::new(),
        description: String::new(),
        progress_state: None,
        os: Some(OperatingSystem::Linux),
    };
    partial(&mut instance);
    instance
}

/// `{ terminal: { integrated: { tabs: { separator, title, description, allowAgentCliTitle? } } } }`.
fn tabs_configuration(separator: &str, title: &str, description: &str, allow_agent_cli_title: Option<bool>) -> Dynamic {
    let mut tabs = vec![("separator", Dynamic::string(separator)), ("title", Dynamic::string(title)), ("description", Dynamic::string(description))];
    if let Some(allow) = allow_agent_cli_title {
        tabs.push(("allowAgentCliTitle", Dynamic::Bool(allow)));
    }
    Dynamic::object([("terminal", Dynamic::object([("integrated", Dynamic::object([("tabs", Dynamic::object(tabs))]))]))])
}

/// `createLabelComputer(configuration)`.
fn create_label_computer(store: &LeakGuard, configuration: Dynamic) -> Rc<TerminalLabelComputer> {
    let configuration_service = TestConfigurationService::new(Some(configuration));
    // O emissor do `TestConfigurationService` é descartado pelo store do caso (o original não o descarta).
    store.add_rc(configuration_service.on_did_change_configuration_emitter.clone());
    let configuration: Rc<dyn IConfigurationService> = configuration_service.clone();
    let configuration_instantiation = store.add_rc(InstantiationService::new(ServiceCollection::new().with(I_CONFIGURATION_SERVICE, configuration.clone())));
    let terminal_configuration_service: Rc<dyn ITerminalConfigurationService> =
        store.add_rc(configuration_instantiation.create_instance::<TerminalConfigurationService>(()));

    let file_service: Rc<dyn IFileService> = store.add_rc(TestFileService::new());
    let workspace_context_service: Rc<dyn IWorkspaceContextService> = TestContextService::new(None, None);
    let instantiation_service = store.add_rc(InstantiationService::new(
        ServiceCollection::new()
            .with(I_CONFIGURATION_SERVICE, configuration)
            .with(I_FILE_SERVICE, file_service)
            .with(I_WORKSPACE_CONTEXT_SERVICE, workspace_context_service)
            .with(I_TERMINAL_CONFIGURATION_SERVICE, terminal_configuration_service),
    ));
    store.add_rc(instantiation_service.create_instance::<TerminalLabelComputer>(()))
}

/// `{ uri: URI.from({ scheme: Schemas.file, path }) } as IWorkspaceFolder`.
fn workspace_folder(path: &str) -> WorkspaceFolder {
    let uri = URI::from(&UriComponents { scheme: Some("file".into()), path: Some(path.into()), ..UriComponents::default() }, false);
    WorkspaceFolder::new(uri, "", 0, None)
}

fn local() -> IShellLaunchConfig {
    IShellLaunchConfig { type_: Some(TerminalType::Local), ..IShellLaunchConfig::default() }
}

fn task() -> IShellLaunchConfig {
    IShellLaunchConfig { type_: Some(TerminalType::Task), ..IShellLaunchConfig::default() }
}

fn title_template(template: &str) -> IShellLaunchConfig {
    IShellLaunchConfig { title_template: Some(template.to_owned()), ..IShellLaunchConfig::default() }
}

#[test]
fn terminal_label_computer_should_resolve_to_empty_when_the_template_variables_are_empty() {
    let store = ensure_no_disposables_are_leaked_in_test_suite();
    let computer = create_label_computer(&store, tabs_configuration(" - ", "", "", None));
    computer.refresh_label(&create_instance(&store, |instance| instance.process_name = String::new()), false);
    assert_eq!(computer.title(), "");
    assert_eq!(computer.description(), "");
}

#[test]
fn terminal_label_computer_should_resolve_cwd_when_outside_of_user_home() {
    let store = ensure_no_disposables_are_leaked_in_test_suite();
    let computer = create_label_computer(&store, tabs_configuration(" - ", "${cwd}", "${cwd}", None));
    computer.refresh_label(&create_instance(&store, |instance| instance.cwd = Some(ROOT_1.to_owned())), false);
    assert_eq!(computer.title(), ROOT_1);
    assert_eq!(computer.description(), ROOT_1);
}

#[test]
fn terminal_label_computer_should_resolve_cwd_when_under_user_home() {
    let store = ensure_no_disposables_are_leaked_in_test_suite();
    let computer = create_label_computer(&store, tabs_configuration(" - ", "${cwd}", "${cwd}", None));
    computer.refresh_label(&create_instance(&store, |instance| instance.cwd = Some("/home/user/foo/bar".to_owned())), false);
    assert_eq!(computer.title(), "~/foo/bar");
    assert_eq!(computer.description(), "~/foo/bar");
}

#[test]
fn terminal_label_computer_should_resolve_cwd_when_exactly_at_user_home() {
    let store = ensure_no_disposables_are_leaked_in_test_suite();
    let computer = create_label_computer(&store, tabs_configuration(" - ", "${cwd}", "${cwd}", None));
    computer.refresh_label(&create_instance(&store, |instance| instance.cwd = Some("/home/user".to_owned())), false);
    assert_eq!(computer.title(), "~");
    assert_eq!(computer.description(), "~");
}

#[test]
fn terminal_label_computer_should_not_shorten_cwd_on_windows() {
    let store = ensure_no_disposables_are_leaked_in_test_suite();
    let computer = create_label_computer(&store, tabs_configuration(" - ", "${cwd}", "${cwd}", None));
    computer.refresh_label(
        &create_instance(&store, |instance| {
            instance.cwd = Some("C:\\Users\\user".to_owned());
            instance.user_home = Some("C:\\Users\\user".to_owned());
            instance.os = Some(OperatingSystem::Windows);
        }),
        false,
    );
    assert_eq!(computer.title(), "C:\\Users\\user");
    assert_eq!(computer.description(), "C:\\Users\\user");
}

#[test]
fn terminal_label_computer_should_resolve_workspace_folder() {
    let store = ensure_no_disposables_are_leaked_in_test_suite();
    let computer = create_label_computer(&store, tabs_configuration(" - ", "${workspaceFolder}", "${workspaceFolder}", None));
    computer.refresh_label(
        &create_instance(&store, |instance| {
            instance.process_name = "zsh".to_owned();
            instance.workspace_folder = Some(workspace_folder("folder"));
        }),
        false,
    );
    assert_eq!(computer.title(), "folder");
    assert_eq!(computer.description(), "folder");
}

#[test]
fn terminal_label_computer_should_resolve_local() {
    let store = ensure_no_disposables_are_leaked_in_test_suite();
    let computer = create_label_computer(&store, tabs_configuration(" - ", "${local}", "${local}", None));
    computer.refresh_label(
        &create_instance(&store, |instance| {
            instance.process_name = "zsh".to_owned();
            instance.shell_launch_config = local();
        }),
        false,
    );
    assert_eq!(computer.title(), "Local");
    assert_eq!(computer.description(), "Local");
}

#[test]
fn terminal_label_computer_should_resolve_process() {
    let store = ensure_no_disposables_are_leaked_in_test_suite();
    let computer = create_label_computer(&store, tabs_configuration(" - ", "${process}", "${process}", None));
    computer.refresh_label(&create_instance(&store, |instance| instance.process_name = "zsh".to_owned()), false);
    assert_eq!(computer.title(), "zsh");
    assert_eq!(computer.description(), "zsh");
}

#[test]
fn terminal_label_computer_should_resolve_sequence() {
    let store = ensure_no_disposables_are_leaked_in_test_suite();
    let computer = create_label_computer(&store, tabs_configuration(" - ", "${sequence}", "${sequence}", None));
    computer.refresh_label(&create_instance(&store, |instance| instance.sequence = Some("sequence".to_owned())), false);
    assert_eq!(computer.title(), "sequence");
    assert_eq!(computer.description(), "sequence");
}

#[test]
fn terminal_label_computer_should_resolve_empty_sequence_to_process_name() {
    let store = ensure_no_disposables_are_leaked_in_test_suite();
    let computer = create_label_computer(&store, tabs_configuration(" - ", "${sequence}${separator}${process}", "${sequence}", None));
    computer.refresh_label(
        &create_instance(&store, |instance| {
            instance.process_name = "zsh".to_owned();
            instance.sequence = Some(String::new());
        }),
        false,
    );
    assert_eq!(computer.title(), "zsh");
    assert_eq!(computer.description(), "");
}

#[test]
fn terminal_label_computer_should_resolve_task() {
    let store = ensure_no_disposables_are_leaked_in_test_suite();
    let computer = create_label_computer(&store, tabs_configuration(" ~ ", "${process}${separator}${task}", "${task}", None));
    computer.refresh_label(
        &create_instance(&store, |instance| {
            instance.process_name = "zsh".to_owned();
            instance.shell_launch_config = task();
        }),
        false,
    );
    assert_eq!(computer.title(), "zsh ~ Task");
    assert_eq!(computer.description(), "Task");
}

#[test]
fn terminal_label_computer_should_resolve_separator() {
    let store = ensure_no_disposables_are_leaked_in_test_suite();
    let computer = create_label_computer(&store, tabs_configuration(" ~ ", "${separator}", "${separator}", None));
    computer.refresh_label(
        &create_instance(&store, |instance| {
            instance.process_name = "zsh".to_owned();
            instance.shell_launch_config = task();
        }),
        false,
    );
    assert_eq!(computer.title(), "zsh");
    assert_eq!(computer.description(), "");
}

#[test]
fn terminal_label_computer_should_always_return_static_title_when_specified() {
    let store = ensure_no_disposables_are_leaked_in_test_suite();
    let computer = create_label_computer(&store, tabs_configuration(" ~ ", "${process}", "${workspaceFolder}", None));
    computer.refresh_label(
        &create_instance(&store, |instance| {
            instance.process_name = "process".to_owned();
            instance.workspace_folder = Some(workspace_folder("folder"));
            instance.static_title = Some("my-title".to_owned());
        }),
        false,
    );
    assert_eq!(computer.title(), "my-title");
    assert_eq!(computer.description(), "folder");
}

#[test]
fn terminal_label_computer_should_use_shell_launch_config_title_template_as_template_when_set() {
    let store = ensure_no_disposables_are_leaked_in_test_suite();
    let computer = create_label_computer(&store, tabs_configuration(" - ", "${process}", "${cwd}", None));
    computer.refresh_label(
        &create_instance(&store, |instance| {
            instance.sequence = Some("my-sequence".to_owned());
            instance.process_name = "zsh".to_owned();
            instance.shell_launch_config = title_template("${sequence}");
        }),
        false,
    );
    assert_eq!(computer.title(), "my-sequence");
    assert_eq!(computer.description(), "cwd");
}

#[test]
fn terminal_label_computer_should_use_sequence_for_agent_cli_shell_types() {
    let store = ensure_no_disposables_are_leaked_in_test_suite();
    let computer = create_label_computer(&store, tabs_configuration(" - ", "${process}", "${cwd}", Some(true)));
    computer.refresh_label(
        &create_instance(&store, |instance| {
            instance.shell_type = Some(ShellType::General(GeneralShellType::Copilot));
            instance.sequence = Some("Copilot Agent".to_owned());
            instance.process_name = "copilot".to_owned();
        }),
        false,
    );
    assert_eq!(computer.title(), "Copilot Agent");
}

#[test]
fn terminal_label_computer_should_use_sequence_for_gemini_agent_cli_shell_type() {
    let store = ensure_no_disposables_are_leaked_in_test_suite();
    let computer = create_label_computer(&store, tabs_configuration(" - ", "${process}", "${cwd}", Some(true)));
    computer.refresh_label(
        &create_instance(&store, |instance| {
            instance.shell_type = Some(ShellType::General(GeneralShellType::Gemini));
            instance.sequence = Some("Gemini - my-project".to_owned());
            instance.process_name = "node".to_owned();
        }),
        false,
    );
    assert_eq!(computer.title(), "Gemini - my-project");
}

#[test]
fn terminal_label_computer_should_use_sequence_for_command_code_agent_cli_shell_type() {
    let store = ensure_no_disposables_are_leaked_in_test_suite();
    let computer = create_label_computer(&store, tabs_configuration(" - ", "${process}", "${cwd}", Some(true)));
    computer.refresh_label(
        &create_instance(&store, |instance| {
            instance.shell_type = Some(ShellType::General(GeneralShellType::CommandCode));
            instance.sequence = Some("Fix Parser Bug".to_owned());
            instance.process_name = "node".to_owned();
        }),
        false,
    );
    assert_eq!(computer.title(), "Fix Parser Bug");
}

#[test]
fn terminal_label_computer_should_prefer_shell_launch_config_title_template_over_agent_cli_shell_type_override() {
    let store = ensure_no_disposables_are_leaked_in_test_suite();
    let computer = create_label_computer(&store, tabs_configuration(" - ", "${process}", "${cwd}", Some(true)));
    computer.refresh_label(
        &create_instance(&store, |instance| {
            instance.shell_type = Some(ShellType::General(GeneralShellType::Copilot));
            instance.sequence = Some("Copilot Agent".to_owned());
            instance.process_name = "copilot".to_owned();
            instance.shell_launch_config = title_template("${process}");
        }),
        false,
    );
    assert_eq!(computer.title(), "copilot");
}

#[test]
fn terminal_label_computer_should_fall_back_to_configured_title_when_allow_agent_cli_title_is_disabled() {
    let store = ensure_no_disposables_are_leaked_in_test_suite();
    let computer = create_label_computer(&store, tabs_configuration(" - ", "${process}", "${cwd}", Some(false)));
    computer.refresh_label(
        &create_instance(&store, |instance| {
            instance.shell_type = Some(ShellType::General(GeneralShellType::Copilot));
            instance.sequence = Some("Copilot Agent".to_owned());
            instance.process_name = "copilot".to_owned();
        }),
        false,
    );
    assert_eq!(computer.title(), "copilot");
}

#[test]
fn terminal_label_computer_should_provide_cwd_folder_for_all_cwds_only_when_in_multi_root() {
    let store = ensure_no_disposables_are_leaked_in_test_suite();
    let computer = create_label_computer(&store, tabs_configuration(" ~ ", "${process}${separator}${cwdFolder}", "${cwdFolder}", None));
    computer.refresh_label(
        &create_instance(&store, |instance| {
            instance.process_name = "process".to_owned();
            instance.workspace_folder = Some(workspace_folder(ROOT_1));
            instance.cwd = Some(ROOT_1.to_owned());
        }),
        false,
    );
    // single-root, cwd is same as root
    assert_eq!(computer.title(), "process");
    assert_eq!(computer.description(), "");
    // multi-root
    computer.refresh_label(
        &create_instance(&store, |instance| {
            instance.process_name = "process".to_owned();
            instance.workspace_folder = Some(workspace_folder(ROOT_1));
            instance.cwd = Some(ROOT_2.to_owned());
        }),
        false,
    );
    if is_windows() {
        assert_eq!(computer.title(), "process");
        assert_eq!(computer.description(), "");
    } else {
        assert_eq!(computer.title(), "process ~ root2");
        assert_eq!(computer.description(), "root2");
    }
}

#[test]
fn terminal_label_computer_should_hide_cwd_folder_in_single_folder_workspaces_when_cwd_matches_the_workspaces_default_cwd_even_when_slashes_differ() {
    let store = ensure_no_disposables_are_leaked_in_test_suite();
    let configuration = || tabs_configuration(" ~ ", "${process}${separator}${cwdFolder}", "${cwdFolder}", None);
    let computer = create_label_computer(&store, configuration());
    computer.refresh_label(
        &create_instance(&store, |instance| {
            instance.process_name = "process".to_owned();
            instance.workspace_folder = Some(workspace_folder(ROOT_1));
            instance.cwd = Some(ROOT_1.to_owned());
        }),
        false,
    );
    assert_eq!(computer.title(), "process");
    assert_eq!(computer.description(), "");
    if !is_windows() {
        let computer = create_label_computer(&store, configuration());
        computer.refresh_label(
            &create_instance(&store, |instance| {
                instance.process_name = "process".to_owned();
                instance.workspace_folder = Some(workspace_folder(ROOT_1));
                instance.cwd = Some(ROOT_2.to_owned());
            }),
            false,
        );
        assert_eq!(computer.title(), "process ~ root2");
        assert_eq!(computer.description(), "root2");
    }
}
