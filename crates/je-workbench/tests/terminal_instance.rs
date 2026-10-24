//! Porte de `src/vs/workbench/contrib/terminal/test/browser/terminalInstance.test.ts` (suíte `Workbench - TerminalInstance`), caso a caso e na mesma ordem.
//!
//! Casos de fora (11 dos 24 do `TerminalInstance`, com os laços contados caso a caso), que dependem de DOM ou de peças do xterm.js
//! que o porte não tem:
//! - `removed/active horizontal scrollbars are released ...` (2), `repeated horizontal scrollbar toggles ...` e `horizontal
//!   scrollbar removal does not affect another terminal`: a barra de rolagem horizontal (`DomScrollableElement`), o
//!   `attachToElement`, o `layout` e o `toggleSizeToContentWidth` são DOM;
//! - `should only load the line data event addon once ...`: o `LineDataEventAddon` e o `loadAddon` do xterm;
//! - `runCommand should wait for prompt input ...`: o caso troca `instance.sendText` por atribuição (o Rust não troca um método
//!   de uma instância), e o `sendText` verdadeiro espera o `ptyProcessReady`, que o `onProcessReady = Event.None` do dublê nunca
//!   resolve;
//! - `should dispose contribution-owned xterm addons ...`: o `xterm.raw.loadAddon`;
//! - `should stop handling file drops ...` e `should not start handling file drops ...`: o `attachToElement` e o evento `drop` do DOM;
//! - os 2 `custom key event handler ...`: o `attachCustomKeyEventHandler`, o `KeyboardEvent` e o `softDispatch` do
//!   `IKeybindingService`.
//!
//! Portados (54): `TerminalInstance` (13), `DEFAULT_COMMANDS_TO_SKIP_SHELL` (1), `parseExitResult` (11), `TerminalLabelComputer` (21)
//! e `getCwdResource` (8).
//!
//! Divergências:
//! - **`createTerminalInstantiationService`.** O `workbenchInstantiationService` inteiro é o `InstantiationService` com os 15
//!   serviços que a `TerminalInstance` e o `TerminalProcessManager` do porte leem (os dublês de `terminal_instance_support.rs` e os
//!   de `workbenchTestServices`: `TestPathService`, `TestEnvironmentService`, `TestRemoteAgentService`, `TestContextService`,
//!   `TestFileService`). `IViewDescriptorService` e `IViewsService` ficam de fora: só a parte de DOM da instância os lê. O
//!   `IConfigurationResolverService` fica sem registro, como no `TestInstantiationService` não estrito do original.
//! - **`createTerminalInstance`.** A criação é síncrona ([`create_instance_with_config`]) e o `await instance.xtermReadyPromise` é o
//!   [`xterm_ready`], dentro do caso assíncrono. O construtor do xterm é o [`FakeXterm`].
//! - **`instance.dispose()` no fim dos casos assíncronos.** O `run_with_real_timers` espera todos os temporizadores (o de 10 s dos
//!   eventos iniciais, por exemplo); o original os deixa para o descarte da suíte.
//! - **`instance['_createProcess']()`, `_onTitleChange`, `_handleShellTypeChange` e os campos `_cwd`/`_userHome`.** Os membros
//!   privados que o original alcança por colchete são métodos públicos (`create_process`, `on_title_change`,
//!   `handle_shell_type_change`, `override_cwd_and_user_home`).
//! - **`await instance.rename(...)`.** O `rename` do porte é síncrono.
//! - **`for (const title of ['', undefined])`.** São dois casos, `..._with_empty_string_...` e `..._with_undefined_...`.
//! - **`writeP(instance.xterm!.raw, ...)`.** É o `write` do emulador, que o dublê interpreta de forma síncrona.
//! - **`ok(exitResult && typeof exitResult === 'object' && typeof exitResult.message === 'string')`.** É o `ExitCodeOrError::Error`
//!   (o `message` é sempre texto).

//! - **`getCwdResource`.** O original monta um objeto que reimplementa o método com um `mockFileService` e um
//!   `mockPathService`; aqui os dois mocks são o [`MockCwdServices`] (que implementa [`ICwdResourceServices`]) e o método
//!   chamado é o [`get_cwd_resource`] real, sobre o store de capacidades com o `{ getCwd }` do original.
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
mod support;

use std::cell::RefCell;
use std::rc::Rc;

use common::{LeakGuard, ensure_no_disposables_are_leaked_in_test_suite, run_with_real_timers};
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
use je_platform::terminal::capabilities::capabilities::{
    CwdDetection, ICwdDetectionCapability, INaiveCwdDetectionCapability, NaiveCwdDetection, TerminalCapability,
};
use je_platform::terminal::capabilities::terminal_capability_store::TerminalCapabilityStore;
use je_platform::terminal::terminal::{GeneralShellType, IShellLaunchConfig, ITerminalLaunchError, ShellType, TerminalType};
use je_platform::workspace::{I_WORKSPACE_CONTEXT_SERVICE, IWorkspaceContextService, WorkspaceFolder};
use je_workbench::contrib::terminal::browser::terminal::{I_TERMINAL_CONFIGURATION_SERVICE, ITerminalConfigurationService};
use je_workbench::contrib::terminal::browser::terminal_configuration_service::TerminalConfigurationService;
use je_workbench::contrib::terminal::browser::terminal_instance::{
    ExitCodeOrError, ExitResult, ICwdResourceServices, LabelInstance, TerminalLabelComputer, get_cwd_resource, parse_exit_result,
};
use je_workbench::contrib::terminal::common::terminal::{ProcessState, default_commands_to_skip_shell};
use je_workbench::test::common::workbench_test_services::{TestContextService, TestFileService};

use je_base::lifecycle::IDisposable;
use je_platform::contextkey::contextkey::IContextKey;
use je_platform::log::NullLogService;
use je_platform::path::path_service::{I_PATH_SERVICE, IPathService};
use je_platform::product::{I_PRODUCT_SERVICE, IProductService, ProductService};
use je_platform::terminal::terminal::{I_TERMINAL_LOG_SERVICE, ITerminalEnvironment, ITerminalLogService, PosixShellType, ShellCwd, TerminalEnvironmentValue, TitleEventSource};
use je_platform::workspace::{Workspace, new_test_workspace};
use je_platform::workspace_trust::{I_WORKSPACE_TRUST_REQUEST_SERVICE, IWorkspaceTrustRequestService};
use je_workbench::contrib::terminal::browser::terminal::{
    I_TERMINAL_INSTANCE_SERVICE, I_TERMINAL_SERVICE, ITerminalInstanceService, ITerminalService,
};
use je_workbench::contrib::terminal::browser::terminal_instance::{TerminalInstance, TerminalInstanceArgs};
use je_workbench::contrib::terminal::browser::xterm::xterm_terminal::{IXtermRaw, XtermOptions};
use je_workbench::contrib::terminal::common::environment_variable::{I_ENVIRONMENT_VARIABLE_SERVICE, IEnvironmentVariableService};
use je_workbench::contrib::terminal::common::terminal::{I_TERMINAL_PROFILE_RESOLVER_SERVICE, ITerminalProfileResolverService};
use je_workbench::services::environment::environment_service::{I_WORKBENCH_ENVIRONMENT_SERVICE, IWorkbenchEnvironmentService};
use je_workbench::services::history::history::{I_HISTORY_SERVICE, IHistoryService};
use je_workbench::services::remote::remote_agent_service::{I_REMOTE_AGENT_SERVICE, IRemoteAgentService};
use je_workbench::test::browser::workbench_test_services::{TestEnvironmentService, TestPathService, TestRemoteAgentService};
use support::fake_xterm::FakeXterm;
use support::terminal_instance_support::{
    TestEnvironmentVariableService, TestHistoryService, TestTerminalInstanceService, TestTerminalProfileResolverService, TestTerminalService,
    TestTerminalWorkspaceTrustRequestService,
};

/// `fixPath('/foo/root1')`.
const ROOT_1: &str = "/foo/root1";
/// `fixPath('/foo/root2')`.
const ROOT_2: &str = "/foo/root2";

fn is_windows() -> bool {
    Platform::detect().is_windows
}

// ---------------------------------------------------------------------------------------------------------------------
// suite('TerminalInstance')
// ---------------------------------------------------------------------------------------------------------------------

/// O `terminalShellTypeContextKey` do original (`set`, `reset` e `get` que não fazem nada).
struct NoopContextKey;

impl IContextKey for NoopContextKey {
    fn set(&self, _value: Dynamic) {}

    fn reset(&self) {}

    fn get(&self) -> Dynamic {
        Dynamic::Undefined
    }
}

/// `createTerminalInstantiationService(terminalInstanceService?, workspace?, requestWorkspaceTrust = async () => true, workspaceTrustRequestService?)`.
fn create_terminal_instantiation_service(
    store: &LeakGuard,
    terminal_instance_service: Option<Rc<TestTerminalInstanceService>>,
    workspace: Option<Rc<Workspace>>,
    request_workspace_trust: Option<Rc<dyn Fn() -> Promise<bool>>>,
    workspace_trust_request_service: Option<Rc<dyn IWorkspaceTrustRequestService>>,
) -> Rc<InstantiationService> {
    let configuration_service = TestConfigurationService::new(Some(Dynamic::object([
        ("files", Dynamic::object(Vec::<(&str, Dynamic)>::new())),
        (
            "terminal",
            Dynamic::object([(
                "integrated",
                Dynamic::object([
                    ("fontFamily", Dynamic::string("monospace")),
                    ("scrollback", Dynamic::Number(1000.0)),
                    ("fastScrollSensitivity", Dynamic::Number(2.0)),
                    ("mouseWheelScrollSensitivity", Dynamic::Number(1.0)),
                    ("unicodeVersion", Dynamic::string("6")),
                    ("commandsToSkipShell", Dynamic::array(Vec::<Dynamic>::new())),
                    ("shellIntegration", Dynamic::object([("enabled", Dynamic::Bool(true))])),
                ]),
            )]),
        ),
    ])));
    // O emissor do `TestConfigurationService` é descartado pelo store do caso (o original não o descarta).
    store.add_rc(configuration_service.on_did_change_configuration_emitter.clone());
    let configuration: Rc<dyn IConfigurationService> = configuration_service.clone();
    let configuration_instantiation = store.add_rc(InstantiationService::new(ServiceCollection::new().with(I_CONFIGURATION_SERVICE, configuration.clone())));
    let terminal_configuration_concrete = store.add_rc(configuration_instantiation.create_instance::<TerminalConfigurationService>(()));
    let terminal_configuration_service: Rc<dyn ITerminalConfigurationService> = terminal_configuration_concrete;

    let file_service_concrete = store.add_rc(TestFileService::new());
    let file_service: Rc<dyn IFileService> = file_service_concrete;
    // `if (workspace) instantiationService.stub(IWorkspaceContextService, new TestContextService(workspace))`
    let workspace_context_service: Rc<dyn IWorkspaceContextService> = TestContextService::new(workspace, None);
    let terminal_instance_service: Rc<dyn ITerminalInstanceService> = terminal_instance_service.unwrap_or_else(TestTerminalInstanceService::new);
    let workspace_trust_request_service: Rc<dyn IWorkspaceTrustRequestService> = workspace_trust_request_service.unwrap_or_else(|| {
        let request: Rc<dyn Fn() -> Promise<bool>> = request_workspace_trust.unwrap_or_else(|| Rc::new(|| Promise::resolved(true)));
        TestTerminalWorkspaceTrustRequestService::from_function(request)
    });
    let terminal_profile_resolver_service: Rc<dyn ITerminalProfileResolverService> = TestTerminalProfileResolverService::mock();
    let path_service: Rc<dyn IPathService> = TestPathService::new(None, None);
    let workbench_environment_service: Rc<dyn IWorkbenchEnvironmentService> = TestEnvironmentService::new();
    let remote_agent_service: Rc<dyn IRemoteAgentService> = Rc::new(TestRemoteAgentService);
    let product_service: Rc<dyn IProductService> = Rc::new(ProductService::default());
    let history_service: Rc<dyn IHistoryService> = TestHistoryService::new(None);
    let environment_variable_service: Rc<dyn IEnvironmentVariableService> = TestEnvironmentVariableService::new();
    let terminal_service: Rc<dyn ITerminalService> = Rc::new(TestTerminalService);
    let log_service: Rc<dyn ITerminalLogService> = Rc::new(NullLogService);
    store.add_rc(InstantiationService::new(
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
    ))
}

/// `store.add(instantiationService.createInstance(TerminalInstance, terminalShellTypeContextKey, shellLaunchConfig))`. O construtor
/// do emulador é o que cria o [`FakeXterm`] com as opções recebidas.
fn create_instance_with_config(store: &LeakGuard, instantiation_service: &InstantiationService, shell_launch_config: IShellLaunchConfig) -> Rc<TerminalInstance> {
    store.add_rc(instantiation_service.create_instance::<TerminalInstance>(TerminalInstanceArgs {
        terminal_shell_type_context_key: Rc::new(NoopContextKey),
        shell_launch_config,
        xterm_ctor: Box::new(|options: &XtermOptions| -> Rc<dyn IXtermRaw> { FakeXterm::from_options(options) }),
    }))
}

/// `await instance.xtermReadyPromise`: a parte assíncrona de `createTerminalInstance`.
async fn xterm_ready(instance: &TerminalInstance) {
    instance.xterm_ready_promise().await.unwrap();
}

/// `{ executable: '/usr/bin/zsh', cwd: URI.file('/home/test'), isTransient: true }`.
fn zsh_launch_config() -> IShellLaunchConfig {
    IShellLaunchConfig {
        executable: Some("/usr/bin/zsh".to_owned()),
        cwd: Some(ShellCwd::Uri(URI::file("/home/test", je_platform::resources::host_platform()))),
        is_transient: Some(true),
        ..IShellLaunchConfig::default()
    }
}

#[test]
fn terminal_instance_should_create_an_instance_of_terminal_instance_with_env_from_default_profile() {
    let store = ensure_no_disposables_are_leaked_in_test_suite();
    let instantiation_service = create_terminal_instantiation_service(&store, None, None, None, None);
    let instance = create_instance_with_config(&store, &instantiation_service, IShellLaunchConfig::default());
    run_with_real_timers(async move {
        xterm_ready(&instance).await;
        // Wait for the terminal instance to resolve shell launch config env.
        common::timeout(100).await.unwrap();
        let expected: ITerminalEnvironment = vec![("TEST".to_owned(), TerminalEnvironmentValue::Value("TEST".to_owned()))];
        assert_eq!(instance.shell_launch_config().borrow().env, Some(expected));
        // O `run_until_idle` espera os temporizadores da instância: o original os deixa para o descarte da suíte.
        instance.dispose();
    });
}

#[test]
fn terminal_instance_marked_remote_resolver_terminal_bypasses_workspace_trust_request() {
    let store = ensure_no_disposables_are_leaked_in_test_suite();
    let workspace_trust_request_service = TestTerminalWorkspaceTrustRequestService::new();
    let trust_service: Rc<dyn IWorkspaceTrustRequestService> = workspace_trust_request_service.clone();
    let instantiation_service = create_terminal_instantiation_service(&store, None, None, None, Some(trust_service));
    let instance = create_instance_with_config(
        &store,
        &instantiation_service,
        IShellLaunchConfig { remote_resolver_terminal: true, hide_from_user: Some(true), ..zsh_launch_config() },
    );
    run_with_real_timers(async move {
        xterm_ready(&instance).await;
        instance.create_process().await.unwrap();

        assert_eq!(
            (workspace_trust_request_service.request_count(), instance.shell_launch_config().borrow().remote_resolver_terminal),
            // `persistedResolverFlag: undefined`
            (0, false)
        );
        instance.dispose();
    });
}

#[test]
fn terminal_instance_unmarked_terminal_requests_workspace_trust() {
    let store = ensure_no_disposables_are_leaked_in_test_suite();
    let workspace_trust_request_service = TestTerminalWorkspaceTrustRequestService::new();
    let trust_service: Rc<dyn IWorkspaceTrustRequestService> = workspace_trust_request_service.clone();
    let instantiation_service = create_terminal_instantiation_service(&store, None, None, None, Some(trust_service));
    let instance = create_instance_with_config(&store, &instantiation_service, zsh_launch_config());
    run_with_real_timers(async move {
        xterm_ready(&instance).await;
        instance.create_process().await.unwrap();

        assert_eq!(workspace_trust_request_service.request_count(), 1);
        instance.dispose();
    });
}

#[test]
fn terminal_instance_reuse_terminal_requests_workspace_trust_and_does_not_relaunch_when_trust_is_denied() {
    let store = ensure_no_disposables_are_leaked_in_test_suite();
    let terminal_instance_service = TestTerminalInstanceService::new();
    let workspace_trust_request_service = TestTerminalWorkspaceTrustRequestService::new();
    let trust_service: Rc<dyn IWorkspaceTrustRequestService> = workspace_trust_request_service.clone();
    let instantiation_service = create_terminal_instantiation_service(&store, Some(terminal_instance_service.clone()), None, None, Some(trust_service));
    let instance = create_instance_with_config(&store, &instantiation_service, zsh_launch_config());
    run_with_real_timers(async move {
        xterm_ready(&instance).await;

        instance.reuse_terminal(IShellLaunchConfig { executable: Some("/usr/bin/zsh".to_owned()), ..IShellLaunchConfig::default() }, true).await.unwrap();

        assert_eq!((workspace_trust_request_service.request_count(), terminal_instance_service.create_process_count()), (1, 0));
        instance.dispose();
    });
}

#[test]
fn terminal_instance_should_not_create_a_process_when_workspace_trust_is_denied() {
    let store = ensure_no_disposables_are_leaked_in_test_suite();
    let terminal_instance_service = TestTerminalInstanceService::new();
    let (trust_request, resolve_trust) = Promise::new();
    let request_workspace_trust: Rc<dyn Fn() -> Promise<bool>> = Rc::new(move || trust_request.clone());
    let instantiation_service = create_terminal_instantiation_service(&store, Some(terminal_instance_service.clone()), None, Some(request_workspace_trust), None);
    let instance = create_instance_with_config(&store, &instantiation_service, IShellLaunchConfig::default());
    let exit_promise = instance.on_exit().to_promise(None);
    resolve_trust.resolve(false);
    run_with_real_timers(async move {
        exit_promise.await.unwrap();

        assert_eq!(terminal_instance_service.create_process_count(), 0);
    });
}

#[test]
fn terminal_instance_should_not_create_a_process_with_an_unexpected_cwd_in_an_empty_workspace() {
    let store = ensure_no_disposables_are_leaked_in_test_suite();
    let terminal_instance_service = TestTerminalInstanceService::new();
    let instantiation_service =
        create_terminal_instantiation_service(&store, Some(terminal_instance_service.clone()), Some(new_test_workspace("empty", Vec::new(), None, None)), None, None);
    let instance = create_instance_with_config(&store, &instantiation_service, IShellLaunchConfig::default());
    run_with_real_timers(async move {
        xterm_ready(&instance).await;
        terminal_instance_service.process_created_promise().await.unwrap();
        common::timeout(0).await.unwrap();
        instance.override_cwd_and_user_home(Some("/unexpected"), Some("/home"));
        let exit_promise = instance.on_exit().to_promise(None);

        instance.create_process().await.unwrap();
        let exit_result = exit_promise.await.unwrap();

        // `ok(exitResult && typeof exitResult === 'object' && typeof exitResult.message === 'string')`
        assert!(matches!(exit_result, Some(ExitCodeOrError::Error(_))));
        assert_eq!(terminal_instance_service.create_process_count(), 1);
    });
}

#[test]
fn terminal_instance_should_preserve_title_for_task_terminals() {
    let store = ensure_no_disposables_are_leaked_in_test_suite();
    let instantiation_service = create_terminal_instantiation_service(&store, None, None, None, None);
    let task_terminal = create_instance_with_config(
        &store,
        &instantiation_service,
        IShellLaunchConfig { type_: Some(TerminalType::Task), name: Some("Test Task Name".to_owned()), ..IShellLaunchConfig::default() },
    );

    // Simulate setting the title via API (as the task system would do)
    task_terminal.rename(Some("Test Task Name"), None);
    assert_eq!(task_terminal.title(), "Test Task Name");

    // Simulate a process title change (which happens when task completes)
    task_terminal.rename(Some("some-process-name"), Some(TitleEventSource::Process));

    // Verify that the task name is preserved
    assert_eq!(task_terminal.title(), "Test Task Name", "Task terminal should preserve API-set title");
}

/// O corpo de `for (const title of ['', undefined])`.
fn clearing_a_custom_title_restores_process_title_updates(title: Option<&'static str>) {
    let store = ensure_no_disposables_are_leaked_in_test_suite();
    let instantiation_service = create_terminal_instantiation_service(&store, None, None, None, None);
    let instance = create_instance_with_config(&store, &instantiation_service, IShellLaunchConfig::default());
    run_with_real_timers(async move {
        xterm_ready(&instance).await;
        instance.rename(Some("custom"), None);
        instance.rename(title, None);
        instance.rename(Some("next-process"), Some(TitleEventSource::Process));

        assert_eq!(
            (instance.static_title(), instance.process_name(), instance.title(), instance.title_source()),
            (None, "next-process".to_owned(), "next-process".to_owned(), TitleEventSource::Process)
        );
        instance.dispose();
    });
}

#[test]
fn terminal_instance_clearing_a_custom_title_with_empty_string_restores_process_title_updates() {
    clearing_a_custom_title_restores_process_title_updates(Some(""));
}

#[test]
fn terminal_instance_clearing_a_custom_title_with_undefined_restores_process_title_updates() {
    clearing_a_custom_title_restores_process_title_updates(None);
}

#[test]
fn terminal_instance_clearing_a_custom_title_restores_shell_title_sequence_updates() {
    let store = ensure_no_disposables_are_leaked_in_test_suite();
    let instantiation_service = create_terminal_instantiation_service(&store, None, None, None, None);
    let instance = create_instance_with_config(&store, &instantiation_service, IShellLaunchConfig::default());
    run_with_real_timers(async move {
        xterm_ready(&instance).await;
        instance.rename(Some("custom"), None);
        instance.rename(Some(""), None);
        // `writeP(instance.xterm!.raw, '\x1b]0;next-title\x07')`
        instance.xterm().unwrap().raw().write("\x1b]0;next-title\x07", None);

        assert_eq!(
            (instance.static_title(), instance.sequence(), instance.title(), instance.title_source()),
            (None, Some("next-title".to_owned()), "next-title".to_owned(), TitleEventSource::Sequence)
        );
        instance.dispose();
    });
}

#[test]
fn terminal_instance_should_preserve_agent_shell_type_detected_from_sequence_until_the_parent_shell_returns() {
    let store = ensure_no_disposables_are_leaked_in_test_suite();
    let instantiation_service = create_terminal_instantiation_service(&store, None, None, None, None);
    let instance = create_instance_with_config(&store, &instantiation_service, IShellLaunchConfig::default());
    run_with_real_timers(async move {
        xterm_ready(&instance).await;

        assert_eq!(instance.shell_type(), None);
        instance.on_title_change("Claude Code");
        assert_eq!(instance.shell_type(), Some(ShellType::General(GeneralShellType::Claude)));

        instance.handle_shell_type_change(Some(ShellType::General(GeneralShellType::Node)));
        assert_eq!(instance.shell_type(), Some(ShellType::General(GeneralShellType::Claude)));

        instance.handle_shell_type_change(None);
        assert_eq!(instance.shell_type(), Some(ShellType::General(GeneralShellType::Claude)));

        instance.handle_shell_type_change(Some(ShellType::Posix(PosixShellType::Zsh)));
        assert_eq!(instance.shell_type(), Some(ShellType::Posix(PosixShellType::Zsh)));
        instance.dispose();
    });
}

#[test]
fn terminal_instance_should_detect_command_code_agent_shell_type_from_its_osc_title() {
    let store = ensure_no_disposables_are_leaked_in_test_suite();
    let instantiation_service = create_terminal_instantiation_service(&store, None, None, None, None);
    let instance = create_instance_with_config(&store, &instantiation_service, IShellLaunchConfig::default());
    run_with_real_timers(async move {
        xterm_ready(&instance).await;

        assert_eq!(instance.shell_type(), None);
        instance.on_title_change("\u{2733} Command Code \u{00b7} my-project");
        assert_eq!(instance.shell_type(), Some(ShellType::General(GeneralShellType::CommandCode)));
        instance.dispose();
    });
}

#[test]
fn terminal_instance_should_fire_on_will_dispose_before_xterm_disposal_and_on_disposed_after_xterm_disposal() {
    let store = ensure_no_disposables_are_leaked_in_test_suite();
    let instantiation_service = create_terminal_instantiation_service(&store, None, None, None, None);
    let instance = create_instance_with_config(&store, &instantiation_service, IShellLaunchConfig::default());
    run_with_real_timers(async move {
        xterm_ready(&instance).await;
        let xterm = instance.xterm_ready_promise().await.unwrap().unwrap();
        let disposal_order: Rc<RefCell<Vec<&'static str>>> = Rc::default();

        let order = disposal_order.clone();
        let will_dispose = instance.on_will_dispose().listen(move |_| order.borrow_mut().push("onWillDispose"));
        let order = disposal_order.clone();
        let xterm_disposed = xterm.on_did_dispose().listen(move |_| order.borrow_mut().push("xterm"));
        let order = disposal_order.clone();
        let disposed = instance.on_disposed().listen(move |_| order.borrow_mut().push("onDisposed"));

        instance.dispose();

        assert_eq!(*disposal_order.borrow(), vec!["onWillDispose", "xterm", "onDisposed"]);
        will_dispose.dispose();
        xterm_disposed.dispose();
        disposed.dispose();
    });
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
    let terminal_configuration_concrete = store.add_rc(configuration_instantiation.create_instance::<TerminalConfigurationService>(()));
    let terminal_configuration_service: Rc<dyn ITerminalConfigurationService> = terminal_configuration_concrete;

    let file_service_concrete = store.add_rc(TestFileService::new());
    let file_service: Rc<dyn IFileService> = file_service_concrete;
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

// ---------------------------------------------------------------------------------------------------------------------
// suite('getCwdResource')
// ---------------------------------------------------------------------------------------------------------------------

/// O `mockFileService` (`canHandleResource` e `exists`) e o `mockPathService` (`fileURI`) do original.
struct MockCwdServices {
    remote_authority: Option<String>,
    file_exists: bool,
    file_service_can_handle: bool,
}

impl ICwdResourceServices for MockCwdServices {
    fn file_uri(&self, path: &str) -> Promise<URI> {
        match &self.remote_authority {
            Some(remote_authority) => Promise::resolved(URI::parse(format!("vscode-remote://{remote_authority}{path}").as_str(), false)),
            None => Promise::resolved(URI::file(path, je_platform::resources::host_platform())),
        }
    }

    fn can_handle_resource(&self, _resource: &URI) -> Promise<bool> {
        Promise::resolved(self.file_service_can_handle)
    }

    fn exists(&self, _resource: &URI) -> Promise<bool> {
        Promise::resolved(self.file_exists)
    }
}

/// A capacidade `{ getCwd: () => options.cwd }` do original.
struct MockCwdDetection {
    cwd: String,
}

impl ICwdDetectionCapability for MockCwdDetection {
    fn capability_type(&self) -> TerminalCapability {
        TerminalCapability::CwdDetection
    }

    fn on_did_change_cwd(&self) -> Event<String> {
        Event::none()
    }

    fn cwds(&self) -> Vec<String> {
        Vec::new()
    }

    fn is_trusted(&self) -> bool {
        false
    }

    fn get_cwd(&self) -> String {
        self.cwd.clone()
    }

    fn update_cwd(&self, _cwd: &str, _is_trusted: Option<bool>) {}
}

/// As `options` de `createMockTerminalInstance`.
#[derive(Default)]
struct CwdOptions {
    cwd: Option<&'static str>,
    remote_authority: Option<&'static str>,
    file_exists: Option<bool>,
    file_service_can_handle: Option<bool>,
}

/// O que `createMockTerminalInstance` devolve (`Pick<ITerminalInstance, 'getCwdResource' | 'capabilities' | 'remoteAuthority'>`).
struct MockTerminalInstance {
    capabilities: Rc<TerminalCapabilityStore>,
    remote_authority: Option<String>,
    services: Rc<MockCwdServices>,
}

impl MockTerminalInstance {
    /// `getCwdResource()`.
    fn get_cwd_resource(&self) -> Promise<Option<URI>> {
        get_cwd_resource(&*self.capabilities, self.remote_authority.as_deref(), self.services.clone())
    }
}

fn create_mock_terminal_instance(store: &LeakGuard, options: CwdOptions) -> MockTerminalInstance {
    let capabilities = store.add_rc(TerminalCapabilityStore::new());
    if let Some(cwd) = options.cwd.filter(|cwd| !cwd.is_empty()) {
        capabilities.add::<CwdDetection>(Rc::new(MockCwdDetection { cwd: cwd.to_owned() }));
    }
    let remote_authority = options.remote_authority.map(str::to_owned);
    MockTerminalInstance {
        capabilities,
        services: Rc::new(MockCwdServices {
            remote_authority: remote_authority.clone(),
            file_exists: options.file_exists != Some(false),
            file_service_can_handle: options.file_service_can_handle != Some(false),
        }),
        remote_authority,
    }
}

#[test]
fn get_cwd_resource_should_return_undefined_when_no_cwd_detection_capability() {
    let store = ensure_no_disposables_are_leaked_in_test_suite();
    let instance = create_mock_terminal_instance(&store, CwdOptions::default());
    run_with_real_timers(async move {
        let result = instance.get_cwd_resource().await.unwrap();
        assert_eq!(result, None);
    });
}

#[test]
fn get_cwd_resource_should_return_undefined_when_cwd_detection_capability_returns_no_cwd() {
    let store = ensure_no_disposables_are_leaked_in_test_suite();
    let instance = create_mock_terminal_instance(&store, CwdOptions { cwd: None, ..CwdOptions::default() });
    run_with_real_timers(async move {
        let result = instance.get_cwd_resource().await.unwrap();
        assert_eq!(result, None);
    });
}

#[test]
fn get_cwd_resource_should_return_uri_file_for_local_terminal_when_file_exists() {
    let store = ensure_no_disposables_are_leaked_in_test_suite();
    let test_cwd = "/test/path";
    let instance = create_mock_terminal_instance(&store, CwdOptions { cwd: Some(test_cwd), file_exists: Some(true), ..CwdOptions::default() });
    run_with_real_timers(async move {
        let result = instance.get_cwd_resource().await.unwrap().unwrap();
        assert_eq!(result.scheme().to_string_lossy(), "file");
        assert_eq!(result.path().to_string_lossy(), test_cwd);
    });
}

#[test]
fn get_cwd_resource_should_return_undefined_when_file_does_not_exist() {
    let store = ensure_no_disposables_are_leaked_in_test_suite();
    let test_cwd = "/test/nonexistent";
    let instance = create_mock_terminal_instance(&store, CwdOptions { cwd: Some(test_cwd), file_exists: Some(false), ..CwdOptions::default() });
    run_with_real_timers(async move {
        let result = instance.get_cwd_resource().await.unwrap();
        assert_eq!(result, None);
    });
}

#[test]
fn get_cwd_resource_should_use_path_service_file_uri_for_remote_terminal() {
    let store = ensure_no_disposables_are_leaked_in_test_suite();
    let test_cwd = "/test/remote/path";
    let instance =
        create_mock_terminal_instance(&store, CwdOptions { cwd: Some(test_cwd), remote_authority: Some("test-remote"), file_exists: Some(true), ..CwdOptions::default() });
    run_with_real_timers(async move {
        let result = instance.get_cwd_resource().await.unwrap().unwrap();
        assert_eq!(result.scheme().to_string_lossy(), "vscode-remote");
        assert_eq!(result.authority().to_string_lossy(), "test-remote");
        assert_eq!(result.path().to_string_lossy(), test_cwd);
    });
}

#[test]
fn get_cwd_resource_should_handle_windows_paths_correctly() {
    let store = ensure_no_disposables_are_leaked_in_test_suite();
    let test_cwd = if is_windows() { "C:\\test\\path" } else { "/test/path" };
    let instance = create_mock_terminal_instance(&store, CwdOptions { cwd: Some(test_cwd), file_exists: Some(true), ..CwdOptions::default() });
    run_with_real_timers(async move {
        let result = instance.get_cwd_resource().await.unwrap().unwrap();
        assert_eq!(result.scheme().to_string_lossy(), "file");
        if is_windows() {
            assert_eq!(result.path().to_string_lossy(), "/C:/test/path");
        } else {
            assert_eq!(result.path().to_string_lossy(), test_cwd);
        }
    });
}

#[test]
fn get_cwd_resource_should_handle_empty_cwd_string() {
    let store = ensure_no_disposables_are_leaked_in_test_suite();
    let instance = create_mock_terminal_instance(&store, CwdOptions { cwd: Some(""), ..CwdOptions::default() });
    run_with_real_timers(async move {
        let result = instance.get_cwd_resource().await.unwrap();
        assert_eq!(result, None);
    });
}

#[test]
fn get_cwd_resource_should_return_undefined_when_file_service_cannot_handle_the_resource_vs_code_web_enopro_scenario() {
    let store = ensure_no_disposables_are_leaked_in_test_suite();
    // Simulates server-linux-x64-web where remoteAuthority is falsy from the terminal's perspective, so URI.file() is
    // produced but the browser FileService has no file:// provider registered.
    let test_cwd = "/workspace/my-project";
    let instance = create_mock_terminal_instance(
        &store,
        CwdOptions { cwd: Some(test_cwd), file_exists: Some(true), file_service_can_handle: Some(false), ..CwdOptions::default() },
    );
    run_with_real_timers(async move {
        let result = instance.get_cwd_resource().await.unwrap();
        assert_eq!(result, None);
    });
}
