//! O host de extensões: o programa Node (`NodeRunner`), os serviços `ExtHost*` e o objeto `vscode` que eles alimentam.

use std::rc::Rc;

use je_base::event::Event;
use je_base::lifecycle::IDisposable;
use je_base::promise::Promise;
use je_base::uri::URI;
use je_platform::log::{LogServiceRef, NullLogService};
use je_platform::request::request::{AuthInfo, Credentials};
use je_workbench::api::browser::main_thread_commands::MainThreadCommands;
use je_workbench::api::common::ext_host_commands::ExtHostCommands;
use je_workbench::api::common::ext_host_configuration::ExtHostConfiguration;
use je_workbench::api::common::ext_host_file_system_info::ExtHostFileSystemInfo;
use je_workbench::api::common::ext_host_output::ExtHostOutputService;
use je_workbench::api::common::ext_host_progress::ExtHostProgress;
use je_workbench::api::common::ext_host_protocol::{
    ExtHostCommandsShape, ExtHostConfigurationShape, ExtHostContext, ExtHostOutputServiceShape, ExtHostProgressShape, ExtHostQuickOpenShape, ExtHostStatusBarShape,
    ExtHostWorkspaceShape, ICommandMetadataDto, IExtHostTelemetry, IMainContext, MainContext, MainThreadTelemetryShape,
};
use je_workbench::api::common::ext_host_quick_open::ExtHostQuickOpen;
use je_workbench::api::common::ext_host_status_bar::ExtHostStatusBar;
use je_workbench::api::common::ext_host_workspace::{ExtHostWorkspace, IExtHostInitDataService, IExtHostWorkspaceProvider, IURITransformerService, WorkspaceFolder};
use je_workbench::services::extensions::extensions::IExtensionService;
use zjsc::api::embed_node::{JsData, NodeRunner};

use crate::api::events::EventTables;
use crate::api::window::{WindowServices, WindowState};
use crate::api::workspace::WorkspaceServices;
use crate::api::{commands, disposable, enums, events, l10n, types, uri, window, workspace};
use crate::in_process::{BridgedContext, IMainThreadBinder, LateForwarders};
use crate::local_context::LocalContext;

/// O `ExtHostCommandsShape` que ainda não existe quando o lado do workbench é criado: encaminha para o real depois de
/// ligado (os dois lados do RPC nascem um dentro do outro).
#[derive(Default)]
struct LateExtHostCommands(std::cell::RefCell<Option<Rc<dyn ExtHostCommandsShape>>>);

impl ExtHostCommandsShape for LateExtHostCommands {
    fn rpc_execute_contributed_command(&self, id: &str, args: Vec<je_base::dynamic::Dynamic>) -> Promise<je_base::dynamic::Dynamic> {
        self.0.borrow().as_ref().expect("ExtHostCommands ainda não ligado").rpc_execute_contributed_command(id, args)
    }

    fn rpc_get_contributed_command_metadata(&self) -> Promise<Vec<(String, ICommandMetadataDto)>> {
        self.0.borrow().as_ref().expect("ExtHostCommands ainda não ligado").rpc_get_contributed_command_metadata()
    }
}

/// A telemetria que descarta tudo (o lado do workbench real entra quando a telemetria for montada).
struct NullTelemetry;

impl IDisposable for NullTelemetry {
    fn dispose(&self) {}
}

impl MainThreadTelemetryShape for NullTelemetry {
    fn rpc_public_log(&self, _event_name: &str, _data: Option<je_base::dynamic::Dynamic>) {}
    fn rpc_public_log2(&self, _event_name: &str, _data: Option<je_base::dynamic::Dynamic>) {}
}

impl IExtHostTelemetry for NullTelemetry {
    fn on_extension_error(&self, _extension: &je_extensions::manifest::ExtensionIdentifier, _error: &je_base::errors::Error) -> bool {
        false
    }
}

/// Os dados de início do host, sem workspace: ele chega pelo `$initializeWorkspace` que o `MainThreadWorkspace` envia.
struct NoInitialWorkspace;

impl IExtHostInitDataService for NoInitialWorkspace {
    fn workspace(&self) -> Option<je_workbench::api::common::ext_host_protocol::IWorkspaceData> {
        None
    }
}

/// O `IURITransformerService` de um host em processo: não há transformação de URIs (os dois lados enxergam o mesmo sistema de
/// arquivos e os mesmos esquemas).
struct IdentityUriTransformer;

impl IURITransformerService for IdentityUriTransformer {
    fn transform_outgoing_scheme(&self, scheme: &str) -> String {
        scheme.to_owned()
    }
}

/// O `IExtHostWorkspaceProvider` de um host sem os clientes de workspace: não há pastas abertas nem proxy ou certificados para
/// consultar. O `ExtHostQuickOpen` só o usa em `showWorkspaceFolderPick`, que o `vscode` do host não expõe.
struct NoWorkspaceProvider;

impl IExtHostWorkspaceProvider for NoWorkspaceProvider {
    fn get_workspace_folder2(&self, _uri: &URI, _resolve_parent: Option<bool>) -> Promise<Option<Rc<WorkspaceFolder>>> {
        Promise::resolved(None)
    }

    fn resolve_workspace_folder(&self, _uri: &URI) -> Promise<Option<Rc<WorkspaceFolder>>> {
        Promise::resolved(None)
    }

    fn get_workspace_folders2(&self) -> Promise<Option<Vec<Rc<WorkspaceFolder>>>> {
        Promise::resolved(None)
    }

    fn resolve_proxy(&self, _url: &str) -> Promise<Option<String>> {
        Promise::resolved(None)
    }

    fn lookup_authorization(&self, _auth_info: AuthInfo) -> Promise<Option<Credentials>> {
        Promise::resolved(None)
    }

    fn lookup_kerberos_authorization(&self, _url: &str) -> Promise<Option<String>> {
        Promise::resolved(None)
    }

    fn load_certificates(&self) -> Promise<Vec<String>> {
        Promise::resolved(Vec::new())
    }
}

/// Os serviços do workbench de que o host depende e que o chamador monta.
pub struct WorkbenchServices {
    pub command_service: Rc<dyn je_platform::commands::commands::ICommandService>,
    pub extension_service: Rc<dyn IExtensionService>,
}

/// Os serviços `ExtHost*` dos namespaces que dependem de um cliente `MainThread*` em processo, e os clientes que o host é dono.
#[derive(Default)]
struct BridgedServices {
    workspace: Option<Rc<ExtHostWorkspace>>,
    configuration: Option<Rc<ExtHostConfiguration>>,
    status_bar: Option<Rc<ExtHostStatusBar>>,
    quick_open: Option<Rc<ExtHostQuickOpen>>,
    output: Option<Rc<ExtHostOutputService>>,
    progress: Option<Rc<ExtHostProgress>>,
    main_thread: Vec<Rc<dyn IDisposable>>,
}

/// O host de extensões.
pub struct ExtensionHost {
    runner: NodeRunner,
    commands: Rc<ExtHostCommands>,
    vscode: JsData,
    table: Rc<disposable::DisposableTable>,
    bridged: BridgedServices,
    _main_thread_commands: Option<Rc<MainThreadCommands>>,
}

impl ExtensionHost {
    /// Monta os serviços e registra `require('vscode')` em `runner`, com o `MainThreadCommands` do workbench no mesmo
    /// [`LocalContext`] (sem serialização: os dois lados são Rust no mesmo processo).
    pub fn new(runner: &NodeRunner, services: WorkbenchServices) -> ExtensionHost {
        let context = LocalContext::new();
        let context_ref: &dyn IMainContext = &*context;
        let late = Rc::new(LateExtHostCommands::default());
        let late_shape: Rc<dyn ExtHostCommandsShape> = late.clone();
        context_ref.set_erased(ExtHostContext::EXT_HOST_COMMANDS.sid, Rc::new(late_shape));
        let telemetry: Rc<dyn MainThreadTelemetryShape> = Rc::new(NullTelemetry);
        context_ref.set_erased(MainContext::MAIN_THREAD_TELEMETRY.sid, Rc::new(telemetry));
        let main_thread_commands = MainThreadCommands::new(context_ref, services.command_service, services.extension_service);
        let main_shape: Rc<dyn je_workbench::api::common::ext_host_protocol::MainThreadCommandsShape> = main_thread_commands.clone();
        context_ref.set_erased(MainContext::MAIN_THREAD_COMMANDS.sid, Rc::new(main_shape));
        let mut host = Self::with_context(runner, context_ref);
        let shape: Rc<dyn ExtHostCommandsShape> = host.commands.clone();
        *late.0.borrow_mut() = Some(shape);
        host._main_thread_commands = Some(main_thread_commands);
        host
    }

    /// Monta o host sobre `context`, que já entrega o proxy de `MainThreadCommands` e `MainThreadTelemetry` (o `RPCProtocol` do
    /// lado do host, quando o workbench está do outro lado de um protocolo de mensagens), registra o `ExtHostCommands` nele
    /// (para o workbench executar os comandos que as extensões registram) e registra `require('vscode')` em `runner`. O objeto
    /// `vscode` só tem `commands`, `l10n` e as classes de valor: os namespaces que pedem clientes do workbench entram por
    /// [`ExtensionHost::with_bridge`].
    pub fn with_context(runner: &NodeRunner, context: &dyn IMainContext) -> ExtensionHost {
        Self::with_bridge(runner, context, None, NullLogService::shared())
    }

    /// Como [`ExtensionHost::with_context`], com o `binder` que monta, em processo, os clientes `MainThread*` do workbench
    /// (ver `crate::in_process`). Cada namespace entra em `vscode` se o `binder` registrou os clientes de que ele depende:
    /// `workspace` pede `MainThreadWorkspace`, `MainThreadConfiguration` e `MainThreadMessageService`; `window` existe se houver ao
    /// menos um destes: `MainThreadMessageService` (as mensagens), `MainThreadStatusBar` (a barra de status), `MainThreadQuickOpen`
    /// (`showQuickPick` e `showInputBox`), `MainThreadOutputService` com [`IMainThreadBinder::output_environment`]
    /// (`createOutputChannel`) e `MainThreadProgress` (`withProgress`).
    pub fn with_bridge(runner: &NodeRunner, context: &dyn IMainContext, binder: Option<&dyn IMainThreadBinder>, log_service: LogServiceRef) -> ExtensionHost {
        let commands = ExtHostCommands::new(context, NullLogService::shared(), Rc::new(NullTelemetry));
        let shape: Rc<dyn ExtHostCommandsShape> = commands.clone();
        context.set(&ExtHostContext::EXT_HOST_COMMANDS, shape);

        let table = disposable::DisposableTable::new();
        let event_tables = EventTables::new();
        let window_state = WindowState::default();
        let disposable_class = disposable::define(runner, &table);
        let mut members = types::classes(runner);
        members.push(("Disposable".to_owned(), disposable_class));
        members.extend(events::classes(runner, &table, &event_tables));
        members.push(uri::class(runner));
        members.extend(enums::enums());
        members.extend(workspace::classes(runner));
        members.extend(window::classes(runner, &window_state, &table));

        let mut bridged = BridgedServices::default();
        if let Some(binder) = binder {
            let local = LocalContext::new();
            let late = LateForwarders::register(&local);
            let local_context: Rc<dyn IMainContext> = local.clone();
            bridged.main_thread = binder.bind(local_context);
            let bridged_context = BridgedContext::new(context, local.clone());
            let bridged_ref: &dyn IMainContext = &bridged_context;
            let provided = |sid: &'static str| local.contains(sid);

            if provided(MainContext::MAIN_THREAD_WORKSPACE.sid) && provided(MainContext::MAIN_THREAD_CONFIGURATION.sid) && provided(MainContext::MAIN_THREAD_MESSAGE_SERVICE.sid) {
                let workspace_service = ExtHostWorkspace::new(
                    bridged_ref,
                    &NoInitialWorkspace,
                    Rc::new(ExtHostFileSystemInfo::new()),
                    log_service.clone(),
                    Rc::new(IdentityUriTransformer),
                );
                let configuration = ExtHostConfiguration::new(bridged_ref, workspace_service.clone(), log_service.clone());
                let workspace_shape: Rc<dyn ExtHostWorkspaceShape> = workspace_service.clone();
                late.workspace.0.bind(workspace_shape);
                let configuration_shape: Rc<dyn ExtHostConfigurationShape> = configuration.clone();
                late.configuration.0.bind(configuration_shape);
                members.push((
                    "workspace".to_owned(),
                    workspace::namespace(runner, &WorkspaceServices { workspace: workspace_service.clone(), configuration: configuration.clone() }, &table, &event_tables),
                ));
                bridged.workspace = Some(workspace_service);
                bridged.configuration = Some(configuration);
            }

            if provided(MainContext::MAIN_THREAD_STATUS_BAR.sid) {
                let status_bar = ExtHostStatusBar::new(bridged_ref, commands.converter());
                let status_bar_shape: Rc<dyn ExtHostStatusBarShape> = status_bar.clone();
                late.status_bar.0.bind(status_bar_shape);
                bridged.status_bar = Some(status_bar);
            }

            if provided(MainContext::MAIN_THREAD_QUICK_OPEN.sid) {
                let workspace_provider: Rc<dyn IExtHostWorkspaceProvider> = match &bridged.workspace {
                    Some(workspace_service) => workspace_service.clone(),
                    None => Rc::new(NoWorkspaceProvider),
                };
                let quick_open = ExtHostQuickOpen::new(bridged_ref, workspace_provider, commands.clone());
                let quick_open_shape: Rc<dyn ExtHostQuickOpenShape> = quick_open.clone();
                late.quick_open.0.bind(quick_open_shape);
                bridged.quick_open = Some(quick_open);
            }

            if provided(MainContext::MAIN_THREAD_OUTPUT_SERVICE.sid)
                && let Some(environment) = binder.output_environment()
            {
                let output = ExtHostOutputService::new(
                    bridged_ref,
                    environment.logs_location,
                    environment.extension_log_levels,
                    environment.directory_creator,
                    environment.logger_service,
                    log_service.clone(),
                );
                let output_shape: Rc<dyn ExtHostOutputServiceShape> = output.clone();
                late.output.0.bind(output_shape);
                bridged.output = Some(output);
            }

            if provided(MainContext::MAIN_THREAD_PROGRESS.sid) {
                let progress = ExtHostProgress::new(bridged_ref);
                let progress_shape: Rc<dyn ExtHostProgressShape> = progress.clone();
                late.progress.0.bind(progress_shape);
                bridged.progress = Some(progress);
            }

            let message_service = provided(MainContext::MAIN_THREAD_MESSAGE_SERVICE.sid).then(|| bridged_ref.get_proxy(&MainContext::MAIN_THREAD_MESSAGE_SERVICE));
            if message_service.is_some() || bridged.status_bar.is_some() || bridged.quick_open.is_some() || bridged.output.is_some() || bridged.progress.is_some() {
                let window_services = Rc::new(WindowServices {
                    message_service,
                    log_service: log_service.clone(),
                    status_bar: bridged.status_bar.clone(),
                    quick_open: bridged.quick_open.clone(),
                    output: bridged.output.clone(),
                    progress: bridged.progress.clone(),
                });
                members.push(("window".to_owned(), window::namespace(runner, window_services, &table, &event_tables, &window_state)));
            }
        }

        members.extend([
            ("commands".to_owned(), commands::namespace(runner, &commands, &table)),
            ("l10n".to_owned(), l10n::namespace(runner)),
        ]);
        let vscode = JsData::Object(members);
        runner.register_module("vscode", &vscode);
        ExtensionHost { runner: runner.clone(), commands, vscode, table, bridged, _main_thread_commands: None }
    }

    pub fn commands(&self) -> &Rc<ExtHostCommands> {
        &self.commands
    }

    pub fn vscode(&self) -> &JsData {
        &self.vscode
    }

    /// O `ExtHostWorkspace` do host, se o host foi montado com os clientes de workspace.
    pub fn workspace(&self) -> Option<&Rc<ExtHostWorkspace>> {
        self.bridged.workspace.as_ref()
    }

    /// O `ExtHostConfiguration` do host, se o host foi montado com os clientes de configuração.
    pub fn configuration(&self) -> Option<&Rc<ExtHostConfiguration>> {
        self.bridged.configuration.as_ref()
    }

    /// A tabela dos descartáveis do Rust que o JavaScript segura (`registerCommand` devolve um deles).
    pub fn disposables(&self) -> &Rc<disposable::DisposableTable> {
        &self.table
    }

    /// Descarta os clientes `MainThread*` que o `binder` criou e a mensagem de status do host (a parada do host: o workbench deixa de
    /// enxergar o que as extensões registraram).
    pub fn dispose_main_thread(&self) {
        if let Some(status_bar) = &self.bridged.status_bar {
            status_bar.dispose_status_message();
        }
        for client in &self.bridged.main_thread {
            client.dispose();
        }
    }

    /// Esvazia as microtarefas dos dois mundos (as do motor e as das `Promise` do Rust) e os timers vencidos até estabilizar.
    pub fn pump(&self) {
        for _ in 0..64 {
            self.runner.drain_microtasks();
            je_base::runtime::turn();
        }
    }

    /// `Event::none()` serve de marcador para o laço do host quando não há evento externo.
    pub fn idle_event() -> Event<()> {
        Event::none()
    }
}
