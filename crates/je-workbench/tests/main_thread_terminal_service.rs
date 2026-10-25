//! Porte de `src/vs/workbench/api/test/browser/mainThreadTerminalService.test.ts` (suíte `MainThreadTerminalService`), caso a caso e na
//! mesma ordem.
//!
//! Notas de tradução:
//! - **`ensureNoDisposablesAreLeakedInTestSuite()` e `setup(async () => ...)`.** O [`Fixture`] cria o guarda de vazamentos antes de
//!   tudo e monta os serviços do `setup`; o `await timeout(0)` do fim do `setup` (o `TerminalService` inicializa os estilos do
//!   editor de forma assíncrona) é a primeira linha do corpo de cada caso. O fixture fica vivo até depois do
//!   `run_with_faked_timers`: o corpo `async` do caso recebe só clones.
//! - **`workbenchInstantiationService({ configurationService }, store)`.** É a coleção de serviços viva do fixture, com os serviços
//!   que o `TerminalService`, o `TerminalInstanceService`, o `TerminalGroupService` e a `TerminalInstance` do porte leem (os
//!   dublês de `terminal_instance_support.rs`, `terminal_service_support.rs` e `workbench_test_services`). A configuração é a do
//!   original (`terminal.integrated` com `allowInUntrustedWorkspace`, `fontFamily`, `scrollback`, `fastScrollSensitivity`,
//!   `mouseWheelScrollSensitivity`, `unicodeVersion`, `commandsToSkipShell`, `shellIntegration.enabled = false` e `tabs`).
//!   `IViewDescriptorService` e `IViewsService` (`TestViewDescriptorService` e `{ isViewVisible: () => false }`) não entram: o
//!   `TerminalGroupService` do porte não os lê (o frontend entrega o painel por `set_panel_host`, que o fixture não define, como
//!   o original não define a view).
//! - **`instantiationService.set(ITerminalProfileService, new class extends TestTerminalProfileService { ... })`.** É o
//!   [`TestTerminalProfileService`]. O `ITerminalProfileService` do porte tem só `onDidChangeAvailableProfiles` e
//!   `registerTerminalProfileProvider`; `getContributedDefaultProfile`, `refreshAvailableProfiles` e `getDefaultProfileName` (que o
//!   original sobrescreve) são do contrato completo que o `MainThreadTerminalService` do porte não lê.
//! - **`ITerminalInstanceService`, `ITerminalGroupService` e `ITerminalService` reais.** Os três são criados pelo serviço de
//!   instanciação do fixture e registrados na coleção depois de criados (`instantiationService.set`). O construtor do xterm da
//!   `TerminalInstance` é o [`FakeXterm`]. `ITerminalEditorService` é o serviço sem editores de `terminal_service_support.rs`.
//! - **`instantiationService.stub(ITerminalLinkProviderService, {})` e os outros dois.** São os dublês [`UnusedLinkProviderService`],
//!   [`UnusedQuickFixService`] e [`UnusedCompletionService`]: o membro chamado falha, como num `{}`.
//! - **`AnyCallRPCProtocol<Pick<ExtHostTerminalServiceShape, '$startExtensionTerminal' | '$acceptProcessInput'>>({...})`.** É o
//!   [`AnyCallExtHostTerminalService`]: os dois métodos que o caso define fazem o que o original faz
//!   (`$sendProcessReady(id, -1, '', undefined)` e o registro da entrada), e os outros devolvem uma promessa resolvida sem valor
//!   (o `() => Promise.resolve(undefined)` do original; onde a forma tem um valor, o valor neutro). O contexto é o
//!   `single_proxy_rpc_protocol`.
//! - **`$createTerminal`, `$dispose`, `$show`, `$sendText`, `$sendProcessReady` e `$sendProcessExit`.** São `create_terminal`,
//!   `dispose_terminal`, `show`, `send_text` (métodos próprios, que devolvem a promessa que o original aguarda) e
//!   `rpc_send_process_ready` e `rpc_send_process_exit` (da forma `MainThreadTerminalServiceShape`). O id do host de extensões é o
//!   [`ExtHostTerminalIdentifier::Uuid`].
//! - **`terminal.xtermReadyPromise`.** A promessa é da classe concreta, fora do contrato `ITerminalInstance` do porte; o caso 1
//!   espera um turno (`timeout(0)`), que basta para o emulador falso ficar pronto.
//! - **Caso 1 (`WeakRef`, `globalThis.gc` e `this.skip()`).** O original só roda com `--expose-gc` e confere, depois de uma coleta,
//!   que o pedido de criação do terminal fechado não ficou retido. Em Rust a coleta é o descarte determinístico do último `Rc`,
//!   então o caso roda sempre: o `WeakRef` do pedido é o `Weak` da configuração de lançamento compartilhada da instância (o
//!   objeto que o pedido vira), e o `requestIdleCallback` mais o `timeout(0)` são dois turnos (`timeout(0)`).
//! - **`try { ... } finally { ... }`.** O `finally` roda depois das afirmações; se uma afirmação falha o caso já reprovou, e o
//!   guarda de vazamentos não confere nada quando o teste está em pânico.
//! - **`store.add(terminals.onDidChangeInstances(() => terminals.instances[0]?.dispose()))`.** O ouvinte guarda o serviço por
//!   referência fraca, para o fixture não formar um ciclo.

#[path = "../../je-base/tests/common/mod.rs"]
mod common;
mod support;

use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::{Rc, Weak};

use common::{LeakGuard, ensure_no_disposables_are_leaked_in_test_suite, run_with_faked_timers, timeout};
use je_base::arrays::OneOrMany;
use je_base::cancellation::CancellationToken;
use je_base::dynamic::Dynamic;
use je_base::event::Event;
use je_base::lifecycle::DisposableRef;
use je_base::promise::Promise;
use je_instantiation::{InstantiationService, ServiceCollection};
use je_platform::configuration::service::{I_CONFIGURATION_SERVICE, IConfigurationService};
use je_platform::configuration::test_configuration_service::TestConfigurationService;
use je_platform::contextkey::contextkey::{I_CONTEXT_KEY_SERVICE, IContextKeyService};
use je_platform::contextkey::service::ContextKeyService;
use je_platform::dialogs::{I_DIALOG_SERVICE, IDialogService};
use je_platform::files::i_file_service::{I_FILE_SERVICE, IFileService};
use je_platform::log::NullLogService;
use je_platform::path::path_service::{I_PATH_SERVICE, IPathService};
use je_platform::product::{I_PRODUCT_SERVICE, IProductService, ProductService};
use je_platform::terminal::capabilities::capabilities::ITerminalCapabilityStore;
use je_platform::terminal::environment_variable::ISerializableEnvironmentVariableCollection;
use je_platform::terminal::terminal::{
    I_TERMINAL_LOG_SERVICE, ICreateContributedTerminalProfileOptions, ITerminalLaunchError, ITerminalLogService, ITerminalProfile, TerminalExitReason, TerminalShellType,
};
use je_platform::workspace::{I_WORKSPACE_CONTEXT_SERVICE, IWorkspaceContextService};
use je_platform::workspace_trust::{I_WORKSPACE_TRUST_REQUEST_SERVICE, IWorkspaceTrustRequestService};
use je_workbench::api::browser::main_thread_terminal_service::{MainThreadTerminalService, MainThreadTerminalServiceServices};
use je_workbench::api::common::ext_host_protocol::{
    ExtHostTerminalIdentifier, ExtHostTerminalServiceShape, ITerminalCommandDto, ITerminalCompletionContextDto, ITerminalDimensionsDto, ITerminalLinkDto, IShellLaunchConfigDto,
    MainThreadTerminalServiceShape, TerminalCommandMatchResultDto, TerminalCompletionListDto, TerminalLaunchConfig, TerminalQuickFix,
};
use je_workbench::contrib::terminal::browser::terminal::{
    I_TERMINAL_CONFIGURATION_SERVICE, I_TERMINAL_EDITOR_SERVICE, I_TERMINAL_GROUP_SERVICE, I_TERMINAL_INSTANCE_SERVICE, I_TERMINAL_SERVICE, ITerminalConfigurationService,
    ITerminalEditorService, ITerminalExternalLinkProvider, ITerminalGroupService, ITerminalInstance, ITerminalInstanceService, ITerminalService,
};
use je_workbench::contrib::terminal::browser::terminal_configuration_service::TerminalConfigurationService;
use je_workbench::contrib::terminal::browser::terminal_group_service::TerminalGroupService;
use je_workbench::contrib::terminal::browser::terminal_instance_service::{TerminalInstanceService, XtermCtorFactory};
use je_workbench::contrib::terminal::browser::terminal_service::TerminalService;
use je_workbench::contrib::terminal::browser::xterm::xterm_terminal::{IXtermRaw, XtermCtor, XtermOptions};
use je_workbench::contrib::terminal::common::environment_variable::{I_ENVIRONMENT_VARIABLE_SERVICE, IEnvironmentVariableService};
use je_workbench::contrib::terminal::common::terminal::{
    I_TERMINAL_PROFILE_RESOLVER_SERVICE, ITerminalProfileProvider, ITerminalProfileResolverService, ITerminalProfileService,
};
use je_workbench::contrib::terminal_contrib::links::links::ITerminalLinkProviderService;
use je_workbench::contrib::terminal_contrib::quick_fix::quick_fix::{
    ITerminalCommandSelector, ITerminalQuickFixProvider, ITerminalQuickFixProviderSelector, ITerminalQuickFixService,
};
use je_workbench::contrib::terminal_contrib::suggest::terminal_completion_item::ITerminalCompletion;
use je_workbench::contrib::terminal_contrib::suggest::terminal_completion_service::{ITerminalCompletionProvider, ITerminalCompletionService};
use je_workbench::services::environment::environment_service::{I_WORKBENCH_ENVIRONMENT_SERVICE, IWorkbenchEnvironmentService};
use je_workbench::services::extensions::extensions::{I_EXTENSION_SERVICE, IExtensionService};
use je_workbench::services::history::history::{I_HISTORY_SERVICE, IHistoryService};
use je_workbench::services::remote::remote_agent_service::{I_REMOTE_AGENT_SERVICE, IRemoteAgentService};
use je_workbench::test::browser::workbench_test_services::{TestEnvironmentService, TestPathService, TestRemoteAgentService};
use je_workbench::test::common::workbench_test_services::{TestContextService, TestFileService};
use support::extensions_workbench_service_support::StubExtensionService;
use support::fake_xterm::FakeXterm;
use support::terminal_contrib_support::unmocked;
use support::terminal_instance_support::{TestEnvironmentVariableService, TestHistoryService, TestTerminalProfileResolverService, TestTerminalWorkspaceTrustRequestService};
use support::terminal_service_support::{EmptyTerminalEditorService, TestDialogService};
use support::test_rpc_protocol::single_proxy_rpc_protocol;

/// `AnyCallRPCProtocol<Pick<ExtHostTerminalServiceShape, '$startExtensionTerminal' | '$acceptProcessInput'>>({...})`.
#[derive(Default)]
struct AnyCallExtHostTerminalService {
    /// O `service` do `setup`, que `$startExtensionTerminal` usa (atribuído depois da criação do serviço).
    service: RefCell<Weak<MainThreadTerminalService>>,
    /// O `input` do `setup`.
    input: Rc<RefCell<Vec<String>>>,
}

impl ExtHostTerminalServiceShape for AnyCallExtHostTerminalService {
    fn rpc_accept_terminal_closed(&self, _id: u32, _exit_code: Option<i32>, _exit_reason: TerminalExitReason) -> Promise<()> {
        Promise::resolved(())
    }

    fn rpc_accept_terminal_opened(&self, _id: u32, _ext_host_terminal_id: Option<&str>, _name: &str, _shell_launch_config: IShellLaunchConfigDto) {}

    fn rpc_accept_active_terminal_changed(&self, _id: Option<u32>) -> Promise<()> {
        Promise::resolved(())
    }

    fn rpc_accept_terminal_process_id(&self, _id: u32, _process_id: i32) -> Promise<()> {
        Promise::resolved(())
    }

    fn rpc_accept_terminal_process_data(&self, _id: u32, _data: &str) -> Promise<()> {
        Promise::resolved(())
    }

    fn rpc_accept_did_execute_command(&self, _id: u32, _command: ITerminalCommandDto) -> Promise<()> {
        Promise::resolved(())
    }

    fn rpc_accept_terminal_title_change(&self, _id: u32, _name: &str) -> Promise<()> {
        Promise::resolved(())
    }

    fn rpc_accept_terminal_dimensions(&self, _id: u32, _cols: i32, _rows: i32) -> Promise<()> {
        Promise::resolved(())
    }

    fn rpc_accept_terminal_maximum_dimensions(&self, _id: u32, _cols: i32, _rows: i32) -> Promise<()> {
        Promise::resolved(())
    }

    fn rpc_accept_terminal_interaction(&self, _id: u32) {}

    fn rpc_accept_terminal_selection(&self, _id: u32, _selection: Option<&str>) {}

    fn rpc_accept_terminal_shell_type(&self, _id: u32, _shell_type: TerminalShellType) {}

    /// `$startExtensionTerminal: async id => { service.$sendProcessReady(id, -1, '', undefined); return undefined; }`.
    fn rpc_start_extension_terminal(&self, id: u32, _initial_dimensions: Option<ITerminalDimensionsDto>) -> Promise<Option<ITerminalLaunchError>> {
        let service = self.service.borrow().upgrade();
        if let Some(service) = service {
            service.rpc_send_process_ready(id, -1, "", None);
        }
        Promise::resolved(None)
    }

    fn rpc_accept_process_ack_data_event(&self, _id: u32, _char_count: usize) {}

    /// `$acceptProcessInput: (_id, data) => input.push(data)`.
    fn rpc_accept_process_input(&self, _id: u32, data: &str) {
        self.input.borrow_mut().push(data.to_owned());
    }

    fn rpc_accept_process_resize(&self, _id: u32, _cols: i32, _rows: i32) {}

    fn rpc_accept_process_shutdown(&self, _id: u32, _immediate: bool) {}

    fn rpc_accept_process_request_initial_cwd(&self, _id: u32) {}

    fn rpc_accept_process_request_cwd(&self, _id: u32) {}

    fn rpc_accept_process_request_latency(&self, _id: u32) -> Promise<u32> {
        Promise::resolved(0)
    }

    fn rpc_provide_links(&self, _id: u32, _line: &str) -> Promise<Vec<ITerminalLinkDto>> {
        Promise::resolved(Vec::new())
    }

    fn rpc_activate_link(&self, _id: u32, _link_id: u32) {}

    fn rpc_init_environment_variable_collections(&self, _collections: Vec<(String, ISerializableEnvironmentVariableCollection)>) {}

    fn rpc_accept_default_profile(&self, _profile: ITerminalProfile, _automation_profile: ITerminalProfile) {}

    fn rpc_create_contributed_profile_terminal(&self, _id: &str, _options: ICreateContributedTerminalProfileOptions) -> Promise<()> {
        Promise::resolved(())
    }

    fn rpc_provide_terminal_quick_fixes(&self, _id: &str, _match_result: TerminalCommandMatchResultDto, _token: CancellationToken) -> Promise<Option<OneOrMany<TerminalQuickFix>>> {
        Promise::resolved(None)
    }

    fn rpc_provide_terminal_completions(&self, _id: &str, _options: ITerminalCompletionContextDto, _token: CancellationToken) -> Promise<Option<TerminalCompletionListDto>> {
        Promise::resolved(None)
    }
}

/// `new class extends TestTerminalProfileService { ... }`: os membros do contrato reduzido do porte (ver o cabeçalho).
struct TestTerminalProfileService;

impl ITerminalProfileService for TestTerminalProfileService {
    fn on_did_change_available_profiles(&self) -> Event<Vec<ITerminalProfile>> {
        Event::none()
    }

    fn register_terminal_profile_provider(&self, _extension_identifier: &str, _id: &str, _profile_provider: Rc<dyn ITerminalProfileProvider>) -> DisposableRef {
        panic!("Method not implemented.")
    }
}

/// `instantiationService.stub(ITerminalLinkProviderService, {})`.
struct UnusedLinkProviderService;

impl ITerminalLinkProviderService for UnusedLinkProviderService {
    fn link_providers(&self) -> Vec<Rc<dyn ITerminalExternalLinkProvider>> {
        unmocked("linkProviders")
    }

    fn on_did_add_link_provider(&self) -> Event<Rc<dyn ITerminalExternalLinkProvider>> {
        unmocked("onDidAddLinkProvider")
    }

    fn on_did_remove_link_provider(&self) -> Event<Rc<dyn ITerminalExternalLinkProvider>> {
        unmocked("onDidRemoveLinkProvider")
    }

    fn register_link_provider(&self, _provider: Rc<dyn ITerminalExternalLinkProvider>) -> DisposableRef {
        unmocked("registerLinkProvider")
    }
}

/// `instantiationService.stub(ITerminalQuickFixService, {})`.
struct UnusedQuickFixService;

impl ITerminalQuickFixService for UnusedQuickFixService {
    fn on_did_register_provider(&self) -> Event<ITerminalQuickFixProviderSelector> {
        unmocked("onDidRegisterProvider")
    }

    fn on_did_register_command_selector(&self) -> Event<ITerminalCommandSelector> {
        unmocked("onDidRegisterCommandSelector")
    }

    fn on_did_unregister_provider(&self) -> Event<String> {
        unmocked("onDidUnregisterProvider")
    }

    fn extension_quick_fixes(&self) -> Promise<Vec<ITerminalCommandSelector>> {
        unmocked("extensionQuickFixes")
    }

    fn providers(&self) -> Rc<RefCell<HashMap<String, Rc<dyn ITerminalQuickFixProvider>>>> {
        unmocked("providers")
    }

    fn register_quick_fix_provider(&self, _id: &str, _provider: Rc<dyn ITerminalQuickFixProvider>) -> DisposableRef {
        unmocked("registerQuickFixProvider")
    }

    fn register_command_selector(&self, _selector: ITerminalCommandSelector) {
        unmocked("registerCommandSelector")
    }
}

/// `instantiationService.stub(ITerminalCompletionService, {})`.
struct UnusedCompletionService;

impl ITerminalCompletionService for UnusedCompletionService {
    fn providers(&self) -> Vec<Rc<dyn ITerminalCompletionProvider>> {
        unmocked("providers")
    }

    fn on_did_change_providers(&self) -> Event<()> {
        unmocked("onDidChangeProviders")
    }

    fn register_terminal_completion_provider(
        &self,
        _extension_identifier: &str,
        _id: &str,
        _provider: Rc<dyn ITerminalCompletionProvider>,
        _trigger_characters: &[String],
    ) -> DisposableRef {
        unmocked("registerTerminalCompletionProvider")
    }

    fn provide_completions(
        &self,
        _prompt_value: &str,
        _cursor_position: i64,
        _allow_fallback_completions: bool,
        _shell_type: TerminalShellType,
        _capabilities: Rc<dyn ITerminalCapabilityStore>,
        _token: CancellationToken,
        _trigger_character: Option<bool>,
        _skip_extension_completions: Option<bool>,
        _explicitly_invoked: Option<bool>,
    ) -> Promise<Option<Vec<ITerminalCompletion>>> {
        unmocked("provideCompletions")
    }
}

/// O construtor do xterm de cada instância: o emulador falso.
fn fake_xterm_ctor_factory() -> XtermCtorFactory {
    Rc::new(|| -> XtermCtor { Box::new(|options: &XtermOptions| -> Rc<dyn IXtermRaw> { FakeXterm::from_options(options) }) })
}

struct Fixture {
    service: Rc<MainThreadTerminalService>,
    terminals: Rc<TerminalService>,
    groups: Rc<TerminalGroupService>,
    input: Rc<RefCell<Vec<String>>>,
    // Declarado por último: o `Drop` dele descarta o que o `Fixture` registrou e confere os vazamentos.
    store: LeakGuard,
}

impl Fixture {
    /// O `setup` da suíte (sem o `await timeout(0)` do fim, que é a primeira linha de cada caso).
    fn new() -> Self {
        let store = ensure_no_disposables_are_leaked_in_test_suite();
        let configuration_service = TestConfigurationService::new(Some(Dynamic::object([
            ("files", Dynamic::object(Vec::<(&str, Dynamic)>::new())),
            (
                "terminal",
                Dynamic::object([(
                    "integrated",
                    Dynamic::object([
                        ("allowInUntrustedWorkspace", Dynamic::Bool(true)),
                        ("fontFamily", Dynamic::string("monospace")),
                        ("scrollback", Dynamic::Number(1000.0)),
                        ("fastScrollSensitivity", Dynamic::Number(2.0)),
                        ("mouseWheelScrollSensitivity", Dynamic::Number(1.0)),
                        ("unicodeVersion", Dynamic::string("6")),
                        ("commandsToSkipShell", Dynamic::array(Vec::<Dynamic>::new())),
                        ("shellIntegration", Dynamic::object([("enabled", Dynamic::Bool(false))])),
                        ("tabs", Dynamic::object([("title", Dynamic::string("${process}")), ("description", Dynamic::string(""))])),
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
        let context_key_concrete = store.add_rc(ContextKeyService::new(configuration.clone()));
        let context_key_service: Rc<dyn IContextKeyService> = context_key_concrete;

        let file_service_concrete = store.add_rc(TestFileService::new());
        let file_service: Rc<dyn IFileService> = file_service_concrete;
        let workspace_context_service: Rc<dyn IWorkspaceContextService> = TestContextService::new(None, None);
        let workspace_trust_request_service: Rc<dyn IWorkspaceTrustRequestService> =
            TestTerminalWorkspaceTrustRequestService::from_function(Rc::new(|| Promise::resolved(true)));
        let terminal_profile_resolver_service: Rc<dyn ITerminalProfileResolverService> = TestTerminalProfileResolverService::new();
        let path_service: Rc<dyn IPathService> = TestPathService::new(None, None);
        let environment_service: Rc<dyn IWorkbenchEnvironmentService> = TestEnvironmentService::new();
        let remote_agent_service: Rc<dyn IRemoteAgentService> = Rc::new(TestRemoteAgentService);
        let product_service: Rc<dyn IProductService> = Rc::new(ProductService::default());
        let history_service: Rc<dyn IHistoryService> = TestHistoryService::new(None);
        let environment_variable_service: Rc<dyn IEnvironmentVariableService> = TestEnvironmentVariableService::new();
        let log_service: Rc<dyn ITerminalLogService> = Rc::new(NullLogService);
        let dialog_service: Rc<dyn IDialogService> = TestDialogService::new();
        let extension_service: Rc<dyn IExtensionService> = StubExtensionService::new();
        let editor_service: Rc<dyn ITerminalEditorService> = Rc::new(EmptyTerminalEditorService);

        let collection = ServiceCollection::new()
            .with(I_CONFIGURATION_SERVICE, configuration)
            .with(I_TERMINAL_CONFIGURATION_SERVICE, terminal_configuration_service)
            .with(I_CONTEXT_KEY_SERVICE, context_key_service)
            .with(I_FILE_SERVICE, file_service)
            .with(I_WORKSPACE_CONTEXT_SERVICE, workspace_context_service)
            .with(I_WORKSPACE_TRUST_REQUEST_SERVICE, workspace_trust_request_service)
            .with(I_TERMINAL_PROFILE_RESOLVER_SERVICE, terminal_profile_resolver_service.clone())
            .with(I_PATH_SERVICE, path_service)
            .with(I_WORKBENCH_ENVIRONMENT_SERVICE, environment_service.clone())
            .with(I_REMOTE_AGENT_SERVICE, remote_agent_service.clone())
            .with(I_PRODUCT_SERVICE, product_service)
            .with(I_HISTORY_SERVICE, history_service)
            .with(I_ENVIRONMENT_VARIABLE_SERVICE, environment_variable_service.clone())
            .with(I_TERMINAL_LOG_SERVICE, log_service)
            .with(I_DIALOG_SERVICE, dialog_service)
            .with(I_EXTENSION_SERVICE, extension_service)
            .with(I_TERMINAL_EDITOR_SERVICE, editor_service.clone());
        let instantiation_service = store.add_rc(InstantiationService::new(collection.clone()));

        // `instantiationService.set(ITerminalInstanceService, store.add(instantiationService.createInstance(TerminalInstanceService)))`
        let terminal_instance_service = store.add_rc(instantiation_service.create_instance::<TerminalInstanceService>(()));
        terminal_instance_service.set_xterm_ctor_factory(fake_xterm_ctor_factory());
        let terminal_instance_service_ref: Rc<dyn ITerminalInstanceService> = terminal_instance_service;
        collection.set(I_TERMINAL_INSTANCE_SERVICE, terminal_instance_service_ref);

        // `groups = store.add(instantiationService.createInstance(TerminalGroupService))`
        let groups = store.add_rc(instantiation_service.create_instance::<TerminalGroupService>(()));
        let groups_ref: Rc<dyn ITerminalGroupService> = groups.clone();
        collection.set(I_TERMINAL_GROUP_SERVICE, groups_ref.clone());

        // `terminals = store.add(instantiationService.createInstance(TerminalService))`
        let terminals = store.add_rc(instantiation_service.create_instance::<TerminalService>(()));
        let terminals_ref: Rc<dyn ITerminalService> = terminals.clone();
        terminals_ref.register_process_support(true);
        collection.set(I_TERMINAL_SERVICE, terminals_ref.clone());

        // These provider registries are not used by terminal creation or disposal.
        let ext_host = Rc::new(AnyCallExtHostTerminalService::default());
        let input = ext_host.input.clone();
        let context = single_proxy_rpc_protocol::<dyn ExtHostTerminalServiceShape>(ext_host.clone());
        let service = store.add_rc(MainThreadTerminalService::new(
            &*context,
            MainThreadTerminalServiceServices {
                terminal_service: terminals_ref,
                terminal_link_provider_service: Rc::new(UnusedLinkProviderService),
                terminal_quick_fix_service: Rc::new(UnusedQuickFixService),
                environment_variable_service,
                log_service: NullLogService::shared(),
                terminal_profile_resolver_service,
                remote_agent_service,
                terminal_group_service: groups_ref,
                terminal_editor_service: editor_service,
                terminal_profile_service: Rc::new(TestTerminalProfileService),
                terminal_completion_service: Rc::new(UnusedCompletionService),
                environment_service,
            },
        ));
        *ext_host.service.borrow_mut() = Rc::downgrade(&service);

        Self { service, terminals, groups, input, store }
    }
}

/// `{ name, shellPath, hideFromUser }` e os campos que o caso define.
fn launch_config(name: &str, shell_path: Option<&str>, hide_from_user: bool, is_extension_custom_pty_terminal: bool) -> TerminalLaunchConfig {
    TerminalLaunchConfig {
        name: Some(name.to_owned()),
        shell_path: shell_path.map(str::to_owned),
        hide_from_user: hide_from_user.then_some(true),
        is_extension_custom_pty_terminal: is_extension_custom_pty_terminal.then_some(true),
        ..TerminalLaunchConfig::default()
    }
}

/// O id (`string`) que o host de extensões gera até a thread principal atribuir o número.
fn uuid(id: &str) -> ExtHostTerminalIdentifier {
    ExtHostTerminalIdentifier::Uuid(id.to_owned())
}

#[test]
fn releases_the_launch_request_after_an_api_terminal_closes() {
    let fixture = Fixture::new();
    let (service, terminals) = (fixture.service.clone(), fixture.terminals.clone());

    run_with_faked_timers(async move {
        timeout(0).await.unwrap();
        // `async function createAndClose()`
        let request = {
            let request = launch_config("Lifecycle test", Some("/test-shell"), true, false);
            service.create_terminal("closed-terminal", request).await.unwrap();
            let terminal = terminals.instances()[0].clone();
            // `await terminal.xtermReadyPromise`
            timeout(0).await.unwrap();
            service.dispose_terminal(&uuid("closed-terminal")).await.unwrap();
            assert!(terminal.is_disposed());
            Rc::downgrade(&terminal.shell_launch_config())
        };
        // Process creation queues idle work. Let that complete before checking lifetime ownership.
        timeout(0).await.unwrap();
        timeout(0).await.unwrap();
        assert!(request.upgrade().is_none(), "The closed terminal launch request is still retained");
    });
}

#[test]
fn closing_one_api_terminal_preserves_a_live_terminal() {
    let fixture = Fixture::new();
    let (service, terminals, input) = (fixture.service.clone(), fixture.terminals.clone(), fixture.input.clone());

    run_with_faked_timers(async move {
        timeout(0).await.unwrap();
        service.create_terminal("closed-terminal", launch_config("Closed", None, true, true)).await.unwrap();
        service.create_terminal("live-terminal", launch_config("Live", None, true, true)).await.unwrap();
        let instances: Vec<Rc<dyn ITerminalInstance>> = terminals.instances();
        Promise::all(instances.iter().map(|instance| instance.process_ready()).collect()).await.unwrap();
        service.dispose_terminal(&uuid("closed-terminal")).await.unwrap();
        service.dispose_terminal(&uuid("closed-terminal")).await.unwrap();
        service.show(&uuid("closed-terminal"), false).await.unwrap();
        service.send_text(&uuid("closed-terminal"), "discarded", false).await.unwrap();
        service.send_text(&uuid("live-terminal"), "delivered", false).await.unwrap();
        assert_eq!(
            (input.borrow().clone(), instances.iter().map(|instance| instance.is_disposed()).collect::<Vec<_>>()),
            (vec!["delivered".to_owned()], vec![true, false])
        );
        // `finally`
        for instance in &instances {
            instance.dispose_with_reason(None);
        }
    });
}

#[test]
fn a_terminal_closed_outside_the_api_no_longer_resolves_its_extension_identifier() {
    let fixture = Fixture::new();
    let (service, terminals) = (fixture.service.clone(), fixture.terminals.clone());

    run_with_faked_timers(async move {
        timeout(0).await.unwrap();
        service.create_terminal("user-closed-terminal", launch_config("Closed by user", Some("/test-shell"), true, false)).await.unwrap();
        let terminal = terminals.instances()[0].clone();
        terminal.dispose_with_reason(None);
        service.show(&uuid("user-closed-terminal"), false).await.unwrap();
        assert_eq!(terminals.instances().len(), 0);
    });
}

#[test]
fn a_process_exit_removes_the_extension_identifier() {
    let fixture = Fixture::new();
    let (service, terminals) = (fixture.service.clone(), fixture.terminals.clone());

    run_with_faked_timers(async move {
        timeout(0).await.unwrap();
        service.create_terminal("exited-terminal", launch_config("Exited", None, true, true)).await.unwrap();
        let terminal = terminals.instances()[0].clone();
        terminal.process_ready().await.unwrap();
        let disposed = terminal.on_disposed().to_promise(None);
        service.rpc_send_process_exit(terminal.instance_id(), Some(0));
        disposed.await.unwrap();
        service.show(&uuid("exited-terminal"), false).await.unwrap();
        assert_eq!(terminals.instances().len(), 0);
        // `finally`
        terminal.dispose_with_reason(None);
    });
}

#[test]
fn a_terminal_removed_from_its_group_still_releases_its_extension_identifier() {
    let fixture = Fixture::new();
    let (service, terminals, groups) = (fixture.service.clone(), fixture.terminals.clone(), fixture.groups.clone());

    run_with_faked_timers(async move {
        timeout(0).await.unwrap();
        service.create_terminal("detached-terminal", launch_config("Detached", Some("/test-shell"), false, false)).await.unwrap();
        let terminal = terminals.instances()[0].clone();
        groups.get_group_for_instance(&terminal).unwrap().remove_instance(&terminal);
        terminal.dispose_with_reason(None);
        service.show(&uuid("detached-terminal"), false).await.unwrap();
        assert_eq!(terminals.instances().len(), 0);
        // `finally`
        terminal.dispose_with_reason(None);
    });
}

#[test]
fn a_terminal_closed_during_creation_releases_its_extension_identifier() {
    let fixture = Fixture::new();
    let (service, terminals) = (fixture.service.clone(), fixture.terminals.clone());
    let weak_terminals = Rc::downgrade(&fixture.terminals);
    fixture.store.add(fixture.terminals.on_did_change_instances().listen(move |_| {
        if let Some(terminals) = weak_terminals.upgrade()
            && let Some(first) = terminals.instances().first()
        {
            first.dispose_with_reason(None);
        }
    }));

    run_with_faked_timers(async move {
        timeout(0).await.unwrap();
        service.create_terminal("early-closed-terminal", launch_config("Early close", Some("/test-shell"), true, false)).await.unwrap();
        service.show(&uuid("early-closed-terminal"), false).await.unwrap();
        assert_eq!(terminals.instances().len(), 0);
    });
}
