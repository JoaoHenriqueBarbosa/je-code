//! Porte de `src/vs/workbench/contrib/terminal/test/browser/terminalService.test.ts` (suíte `Workbench - TerminalService`), caso a
//! caso e na mesma ordem.
//!
//! Notas de tradução:
//! - **`ensureNoDisposablesAreLeakedInTestSuite()` e `setup(async () => ...)`.** O [`Fixture`] cria o guarda de vazamentos antes de
//!   tudo, e o serviço e os serviços que ele lê passam pelo store do guarda. O fixture fica vivo até depois do
//!   `run_with_faked_timers`: o corpo `async` do caso recebe só clones.
//! - **`workbenchInstantiationService({ configurationService }, store)` e os `stub`.** São os dez serviços que o `TerminalService` do
//!   porte lê, de `support/terminal_service_support.rs` (ver os recortes no cabeçalho de `terminal_service.rs`): o
//!   `TestConfigurationService` com `files`, `confirmOnKill` e `enablePersistentSessions`, o `TerminalConfigurationService` real, o
//!   `TestDialogService`, o `ITerminalInstanceService` com `getBackend` indefinido, `getRegisteredBackends` vazio e os `stub` de
//!   `convertProfileToShellLaunchConfig` e `createInstance`, e o `IRemoteAgentService` sem conexão. O
//!   `instantiationService.stub(ITerminalService, terminalService)` não existe: o `TerminalService` do porte não lê o `ITerminalService`.
//! - **`setup` assíncrono e `_initializePrimaryBackend`.** A inicialização do backend primário termina numa microtarefa antes de o
//!   `setup` interno atribuir `_primaryBackend`; o caso espera um turno (`timeout(0)`) antes de [`TerminalService::set_primary_backend`]
//!   (a atribuição por `Reflect`), na mesma ordem.
//! - **`Reflect.get(terminalService, '_backgroundedTerminalDisposables').size`.** É
//!   `backgrounded_terminal_disposables_size_for_testing`. `_updateTitle` e `_updateIcon` são `update_title` e `update_icon`.
//! - **`configurationService.onDidChangeConfigurationEmitter.fire({ affectsConfiguration: () => true, ... })`.** É o evento real da chave
//!   `terminal.integrated` (`fire_configuration_change`), cuja `affectsConfiguration` para a seção do terminal é verdadeira, e
//!   `setUserConfiguration(TERMINAL_CONFIG_SECTION, { confirmOnKill })` grava `terminal.integrated` como objeto.
//! - **Chamadas sem `await`.** O `safeDisposeTerminal` sem `await` do original termina nas microtarefas que o mocha deixa drenar; o
//!   caso espera um turno (`timeout(0)`) no fim, o mesmo efeito.
//! - **`fail()`.** É um `panic!` no `dispose` do dublê.
//! - **`Emitter<number | undefined>` do `onExit`.** É o `Emitter<()>` (ver `terminal_service_support.rs`).

#[path = "../../je-base/tests/common/mod.rs"]
mod common;
#[path = "configuration_event_support/mod.rs"]
mod configuration_event_support;
mod support;

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use common::{LeakGuard, ensure_no_disposables_are_leaked_in_test_suite, run_with_faked_timers, timeout};
use configuration_event_support::fire_configuration_change;
use je_base::dynamic::Dynamic;
use je_base::event::Emitter;
use je_base::themables::ThemeIcon;
use je_instantiation::{InstantiationService, ServiceCollection};
use je_platform::configuration::service::{I_CONFIGURATION_SERVICE, IConfigurationService};
use je_platform::configuration::test_configuration_service::TestConfigurationService;
use je_platform::contextkey::contextkey::{I_CONTEXT_KEY_SERVICE, IContextKeyService};
use je_platform::contextkey::service::ContextKeyService;
use je_platform::dialogs::{ConfirmationResult, I_DIALOG_SERVICE, IDialogService};
use je_platform::log::NullLogService;
use je_platform::terminal::terminal::{
    I_TERMINAL_LOG_SERVICE, IShellLaunchConfig, ITerminalChildProcess, ITerminalLogService, TerminalIcon, TerminalLocation, TitleEventSource,
};
use je_workbench::contrib::terminal::browser::terminal::{
    I_TERMINAL_CONFIGURATION_SERVICE, I_TERMINAL_EDITOR_SERVICE, I_TERMINAL_GROUP_SERVICE, I_TERMINAL_INSTANCE_SERVICE, ICreateTerminalOptions,
    ITerminalConfigurationService, ITerminalEditorService, ITerminalGroupService, ITerminalInstance, ITerminalInstanceService, ITerminalService,
    ShellLaunchConfigOrProfile,
};
use je_workbench::contrib::terminal::browser::terminal_configuration_service::TerminalConfigurationService;
use je_workbench::contrib::terminal::browser::terminal_service::TerminalService;
use je_workbench::contrib::terminal::common::terminal_backend::ITerminalBackend;
use je_workbench::services::environment::environment_service::{I_WORKBENCH_ENVIRONMENT_SERVICE, IWorkbenchEnvironmentService};
use je_workbench::services::extensions::extensions::{I_EXTENSION_SERVICE, IExtensionService};
use je_workbench::services::remote::remote_agent_service::{I_REMOTE_AGENT_SERVICE, IRemoteAgentService};
use je_workbench::test::browser::workbench_test_services::{TestEnvironmentService, TestRemoteAgentService};
use support::extensions_workbench_service_support::StubExtensionService;
use support::terminal_service_support::{
    EmptyTerminalEditorService, EmptyTerminalGroupService, PartialServiceInstance, StubTerminalInstanceService, TestDialogService, TestPersistentTerminalBackend,
};

struct Fixture {
    terminal_service: Rc<TerminalService>,
    configuration_service: Rc<TestConfigurationService>,
    dialog_service: Rc<TestDialogService>,
    terminal_instance_service: Rc<StubTerminalInstanceService>,
    // Declarado por último: o `Drop` dele descarta o que o `Fixture` registrou e confere os vazamentos.
    store: LeakGuard,
}

impl Fixture {
    /// O `setup` da suíte.
    fn new() -> Self {
        let store = ensure_no_disposables_are_leaked_in_test_suite();
        let dialog_service = TestDialogService::new();
        let configuration_service = TestConfigurationService::new(Some(Dynamic::object([
            ("files", Dynamic::object(Vec::<(&str, Dynamic)>::new())),
            (
                "terminal",
                Dynamic::object([(
                    "integrated",
                    Dynamic::object([("confirmOnKill", Dynamic::string("never")), ("enablePersistentSessions", Dynamic::Bool(true))]),
                )]),
            ),
        ])));
        // O emissor do `TestConfigurationService` é descartado pelo store do caso (o original não o descarta).
        store.add_rc(configuration_service.on_did_change_configuration_emitter.clone());
        let configuration: Rc<dyn IConfigurationService> = configuration_service.clone();

        let configuration_instantiation = store.add_rc(InstantiationService::new(ServiceCollection::new().with(I_CONFIGURATION_SERVICE, configuration.clone())));
        let terminal_configuration_concrete = store.add_rc(configuration_instantiation.create_instance::<TerminalConfigurationService>(()));
        let terminal_configuration_service: Rc<dyn ITerminalConfigurationService> = terminal_configuration_concrete;
        let context_key_concrete = store.add_rc(ContextKeyService::new(configuration));
        let context_key_service: Rc<dyn IContextKeyService> = context_key_concrete;

        let terminal_instance_service = StubTerminalInstanceService::new();
        let instance_service: Rc<dyn ITerminalInstanceService> = terminal_instance_service.clone();
        let dialog: Rc<dyn IDialogService> = dialog_service.clone();
        let remote_agent_service: Rc<dyn IRemoteAgentService> = Rc::new(TestRemoteAgentService);
        let environment_service: Rc<dyn IWorkbenchEnvironmentService> = TestEnvironmentService::new();
        let extension_service: Rc<dyn IExtensionService> = StubExtensionService::new();
        let log_service: Rc<dyn ITerminalLogService> = Rc::new(NullLogService);
        let group_service: Rc<dyn ITerminalGroupService> = Rc::new(EmptyTerminalGroupService);
        let editor_service: Rc<dyn ITerminalEditorService> = Rc::new(EmptyTerminalEditorService);

        let instantiation_service = store.add_rc(InstantiationService::new(
            ServiceCollection::new()
                .with(I_CONTEXT_KEY_SERVICE, context_key_service)
                .with(I_TERMINAL_LOG_SERVICE, log_service)
                .with(I_DIALOG_SERVICE, dialog)
                .with(I_REMOTE_AGENT_SERVICE, remote_agent_service)
                .with(I_WORKBENCH_ENVIRONMENT_SERVICE, environment_service)
                .with(I_TERMINAL_CONFIGURATION_SERVICE, terminal_configuration_service)
                .with(I_TERMINAL_EDITOR_SERVICE, editor_service)
                .with(I_TERMINAL_GROUP_SERVICE, group_service)
                .with(I_TERMINAL_INSTANCE_SERVICE, instance_service)
                .with(I_EXTENSION_SERVICE, extension_service),
        ));
        let terminal_service = store.add_rc(instantiation_service.create_instance::<TerminalService>(()));
        Self { terminal_service, configuration_service, dialog_service, terminal_instance_service, store }
    }
}

/// `setConfirmOnKill(configurationService, value)`.
async fn set_confirm_on_kill(configuration_service: &TestConfigurationService, value: &str) {
    configuration_service
        .set_user_configuration("terminal", Dynamic::object([("integrated", Dynamic::object([("confirmOnKill", Dynamic::string(value))]))]), None)
        .await
        .unwrap();
    fire_configuration_change(configuration_service, &["terminal.integrated"]);
}

/// `{ target, hasChildProcesses, onExit: onExitEmitter.event, dispose: () => onExitEmitter.fire(undefined) }`.
fn disposable_instance(target: TerminalLocation, has_child_processes: bool, on_exit_emitter: &Rc<Emitter<()>>) -> Rc<dyn ITerminalInstance> {
    let emitter = on_exit_emitter.clone();
    Rc::new(PartialServiceInstance {
        target: Some(target),
        has_child_processes: Some(has_child_processes),
        on_exit: Some(on_exit_emitter.clone()),
        dispose: Some(Rc::new(move || emitter.fire(()))),
        ..PartialServiceInstance::default()
    })
}

/// `{ target, hasChildProcesses, dispose: () => fail() }`.
fn failing_instance(target: TerminalLocation, has_child_processes: bool) -> Rc<dyn ITerminalInstance> {
    Rc::new(PartialServiceInstance {
        target: Some(target),
        has_child_processes: Some(has_child_processes),
        dispose: Some(Rc::new(|| panic!("fail()"))),
        ..PartialServiceInstance::default()
    })
}

fn confirm_result(confirmed: bool) -> ConfirmationResult {
    ConfirmationResult { checkbox_checked: None, confirmed }
}

/// `createTerminalInstance(options)`.
fn create_terminal_instance(custom_pty_implementation: bool) -> Rc<dyn ITerminalInstance> {
    let shell_launch_config = if custom_pty_implementation {
        IShellLaunchConfig {
            custom_pty_implementation: Some(Rc::new(|_, _, _| -> Rc<dyn ITerminalChildProcess> { panic!("should not be called") })),
            ..IShellLaunchConfig::default()
        }
    } else {
        IShellLaunchConfig::default()
    };
    Rc::new(PartialServiceInstance {
        persistent_process_id: Some(13),
        title: Some("terminal title".to_owned()),
        title_source: Some(TitleEventSource::Process),
        static_title: None,
        icon: Some(TerminalIcon::Theme(ThemeIcon::from_id("remote"))),
        color: None,
        is_disposed: Some(false),
        shell_launch_config: Some(Rc::new(RefCell::new(shell_launch_config))),
        ..PartialServiceInstance::default()
    })
}

#[test]
fn background_terminals_should_remove_disposed_hidden_terminals_and_their_listeners() {
    let fixture = Fixture::new();
    let service = fixture.terminal_service.clone();
    let disposal_emitters: Vec<Rc<Emitter<Rc<dyn ITerminalInstance>>>> = (0..3).map(|_| fixture.store.add_rc(Emitter::new())).collect();
    let instances: Vec<Rc<dyn ITerminalInstance>> = disposal_emitters
        .iter()
        .enumerate()
        .map(|(index, emitter)| {
            let instance: Rc<dyn ITerminalInstance> = Rc::new(PartialServiceInstance {
                instance_id: Some(index as u32 + 1),
                on_disposed: Some(emitter.clone()),
                ..PartialServiceInstance::default()
            });
            instance
        })
        .collect();
    let instance_index = Rc::new(Cell::new(0usize));
    fixture
        .terminal_instance_service
        .stub_convert_profile_to_shell_launch_config(Rc::new(|_, _| IShellLaunchConfig { hide_from_user: Some(true), ..IShellLaunchConfig::default() }));
    fixture.terminal_instance_service.stub_create_instance({
        let (instances, instance_index) = (instances.clone(), instance_index.clone());
        Rc::new(move |_, _| {
            let index = instance_index.get();
            instance_index.set(index + 1);
            instances[index].clone()
        })
    });
    service.register_process_support(true);

    run_with_faked_timers(async move {
        for index in 0..instances.len() {
            let instance = service
                .create_terminal(Some(ICreateTerminalOptions {
                    config: Some(ShellLaunchConfigOrProfile::LaunchConfig(IShellLaunchConfig { hide_from_user: Some(true), ..IShellLaunchConfig::default() })),
                    skip_contributed_profile_check: Some(true),
                    ..ICreateTerminalOptions::default()
                }))
                .await
                .unwrap();

            assert!(service.instances().iter().any(|candidate| Rc::ptr_eq(candidate, &instance)));
            assert_eq!(service.backgrounded_terminal_disposables_size_for_testing(), 1);
            assert!(disposal_emitters[index].has_listeners());

            disposal_emitters[index].fire(instance.clone());

            assert!(!service.instances().iter().any(|candidate| Rc::ptr_eq(candidate, &instance)));
            assert_eq!(service.backgrounded_terminal_disposables_size_for_testing(), 0);
            assert!(!disposal_emitters[index].has_listeners());
        }
    });
}

#[test]
fn safe_dispose_terminal_should_not_show_prompt_when_confirm_on_kill_is_never() {
    let fixture = Fixture::new();
    let (service, configuration_service) = (fixture.terminal_service.clone(), fixture.configuration_service.clone());
    let on_exit_emitter: Rc<Emitter<()>> = fixture.store.add_rc(Emitter::new());

    run_with_faked_timers(async move {
        set_confirm_on_kill(&configuration_service, "never").await;
        service.safe_dispose_terminal(disposable_instance(TerminalLocation::Editor, true, &on_exit_emitter)).await.unwrap();
        service.safe_dispose_terminal(disposable_instance(TerminalLocation::Panel, true, &on_exit_emitter)).await.unwrap();
    });
}

#[test]
fn safe_dispose_terminal_should_not_show_prompt_when_any_terminal_editor_is_closed_handled_by_editor_itself() {
    let fixture = Fixture::new();
    let (service, configuration_service) = (fixture.terminal_service.clone(), fixture.configuration_service.clone());
    let on_exit_emitter: Rc<Emitter<()>> = fixture.store.add_rc(Emitter::new());

    run_with_faked_timers(async move {
        set_confirm_on_kill(&configuration_service, "editor").await;
        let _ = service.safe_dispose_terminal(disposable_instance(TerminalLocation::Editor, true, &on_exit_emitter));
        set_confirm_on_kill(&configuration_service, "always").await;
        let _ = service.safe_dispose_terminal(disposable_instance(TerminalLocation::Editor, true, &on_exit_emitter));
        timeout(0).await.unwrap();
    });
}

#[test]
fn safe_dispose_terminal_should_not_show_prompt_when_confirm_on_kill_is_editor_and_panel_terminal_is_closed() {
    let fixture = Fixture::new();
    let (service, configuration_service) = (fixture.terminal_service.clone(), fixture.configuration_service.clone());
    let on_exit_emitter: Rc<Emitter<()>> = fixture.store.add_rc(Emitter::new());

    run_with_faked_timers(async move {
        set_confirm_on_kill(&configuration_service, "editor").await;
        let _ = service.safe_dispose_terminal(disposable_instance(TerminalLocation::Panel, true, &on_exit_emitter));
        timeout(0).await.unwrap();
    });
}

#[test]
fn safe_dispose_terminal_should_show_prompt_when_confirm_on_kill_is_panel_and_panel_terminal_is_closed() {
    let fixture = Fixture::new();
    let (service, configuration_service, dialog_service) = (fixture.terminal_service.clone(), fixture.configuration_service.clone(), fixture.dialog_service.clone());
    let on_exit_emitter: Rc<Emitter<()>> = fixture.store.add_rc(Emitter::new());

    run_with_faked_timers(async move {
        set_confirm_on_kill(&configuration_service, "panel").await;
        // No child process cases
        dialog_service.set_confirm_result(confirm_result(false));
        let _ = service.safe_dispose_terminal(disposable_instance(TerminalLocation::Panel, false, &on_exit_emitter));
        dialog_service.set_confirm_result(confirm_result(true));
        let _ = service.safe_dispose_terminal(disposable_instance(TerminalLocation::Panel, false, &on_exit_emitter));
        // Child process cases
        dialog_service.set_confirm_result(confirm_result(false));
        service.safe_dispose_terminal(failing_instance(TerminalLocation::Panel, true)).await.unwrap();
        dialog_service.set_confirm_result(confirm_result(true));
        let _ = service.safe_dispose_terminal(disposable_instance(TerminalLocation::Panel, true, &on_exit_emitter));
        timeout(0).await.unwrap();
    });
}

#[test]
fn safe_dispose_terminal_should_show_prompt_when_confirm_on_kill_is_always_and_panel_terminal_is_closed() {
    let fixture = Fixture::new();
    let (service, configuration_service, dialog_service) = (fixture.terminal_service.clone(), fixture.configuration_service.clone(), fixture.dialog_service.clone());
    let on_exit_emitter: Rc<Emitter<()>> = fixture.store.add_rc(Emitter::new());

    run_with_faked_timers(async move {
        set_confirm_on_kill(&configuration_service, "always").await;
        // No child process cases
        dialog_service.set_confirm_result(confirm_result(false));
        let _ = service.safe_dispose_terminal(disposable_instance(TerminalLocation::Panel, false, &on_exit_emitter));
        dialog_service.set_confirm_result(confirm_result(true));
        let _ = service.safe_dispose_terminal(disposable_instance(TerminalLocation::Panel, false, &on_exit_emitter));
        // Child process cases
        dialog_service.set_confirm_result(confirm_result(false));
        service.safe_dispose_terminal(failing_instance(TerminalLocation::Panel, true)).await.unwrap();
        dialog_service.set_confirm_result(confirm_result(true));
        let _ = service.safe_dispose_terminal(disposable_instance(TerminalLocation::Panel, true, &on_exit_emitter));
        timeout(0).await.unwrap();
    });
}

#[test]
fn persistent_title_and_icon_updates_should_not_update_pty_host_metadata_for_custom_pty_terminals() {
    let fixture = Fixture::new();
    let service = fixture.terminal_service.clone();
    let backend = Rc::new(TestPersistentTerminalBackend::default());

    run_with_faked_timers(async move {
        timeout(0).await.unwrap();
        let primary_backend: Rc<dyn ITerminalBackend> = backend.clone();
        service.set_primary_backend(Some(primary_backend));
        let instance = create_terminal_instance(true);

        service.update_title(Some(instance.clone()));
        service.update_icon(instance, false);
        timeout(500).await.unwrap();

        assert_eq!(backend.title_update_count.get(), 0);
        assert_eq!(backend.icon_update_count.get(), 0);
    });
}

#[test]
fn persistent_title_and_icon_updates_should_update_pty_host_metadata_for_regular_pty_terminals() {
    let fixture = Fixture::new();
    let service = fixture.terminal_service.clone();
    let backend = Rc::new(TestPersistentTerminalBackend::default());

    run_with_faked_timers(async move {
        timeout(0).await.unwrap();
        let primary_backend: Rc<dyn ITerminalBackend> = backend.clone();
        service.set_primary_backend(Some(primary_backend));
        let instance = create_terminal_instance(false);

        service.update_title(Some(instance.clone()));
        service.update_icon(instance, true);
        timeout(500).await.unwrap();

        assert_eq!(backend.title_update_count.get(), 1);
        assert_eq!(backend.icon_update_count.get(), 1);
        assert_eq!(backend.last_title.borrow().as_deref(), Some("terminal title"));
        assert_eq!(backend.last_icon_user_initiated.get(), Some(true));
    });
}
