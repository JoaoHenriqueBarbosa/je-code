//! Porte lógico de `src/vs/workbench/contrib/terminal/browser/terminalProcessManager.ts`: o `TerminalProcessManager`, que guarda
//! o estado da criação e do ciclo de vida do processo de um terminal (a criação pelo backend, o ambiente, os eventos de dados
//! filtrados pelo `SeamlessRelaunchDataFilter`, o reconhecimento de dados (`ack`), o relançamento, o encerramento, o sistema
//! operacional e as capacidades), mais o `AckDataBufferer` e o `SeamlessRelaunchDataFilter`.
//!
//! Terminologia do original: _Process_ é o processo lançado com o `terminalProcess.ts`, ou o pty como um todo; _Pty Process_
//! é o processo pai do pseudoterminal; _Shell Process_ é o filho (o shell).
//!
//! Divergências:
//! - **Async.** Os métodos `async` do original devolvem [`Promise`] e rodam como tarefas locais do runtime ([`run_async`]):
//!   cada `await` cede para uma microtarefa como no JavaScript, e o `throw` é o `Err`. A tarefa mantém o gerenciador vivo (um
//!   `Rc`) enquanto roda.
//! - **Injeção.** Os parâmetros decorados do construtor são a tupla `Deps` do [`Injectable`] (o `Deps` aceita até 15), na
//!   ordem do original. O `IConfigurationResolverService` é opcional (`createVariableResolver` aceita o serviço indefinido, que
//!   o `TestInstantiationService` não estrito entrega aos testes). `IAccessibilityService`, `ITelemetryService` e `INotificationService` não entram na tupla: são lidos do
//!   serviço de instanciação quando usados (`IInstantiationService.invokeFunction`), e a ausência de um deles vale como o
//!   serviço indefinido do original (`this._telemetryService?.`). Os parâmetros estáticos (`_instanceId`, `cwd`,
//!   `environmentVariableCollections` e `shellIntegrationNonce`) são [`TerminalProcessManagerArgs`].
//! - **`IShellLaunchConfig` compartilhado.** O gerenciador guarda o [`SharedShellLaunchConfig`] da instância e o altera no
//!   lugar, como o original (`attachPersistentProcess = undefined`, o `env` que o `createTerminalEnvironment` reescreve...). O
//!   empréstimo nunca atravessa um `await`: o `createTerminalEnvironment` roda sobre uma cópia, e o `env` volta para o objeto.
//! - **Serviços do `terminalEnvironment`.** `getWorkspaceForTerminal` e `createVariableResolver` leem os contratos reduzidos
//!   `IHistoryService` e `IConfigurationResolverService` de `terminal_environment`; o gerenciador os alimenta com os serviços
//!   reais por dois adaptadores ([`HistoryServiceAdapter`] e [`ConfigurationResolverAdapter`]).
//! - **`_dimensions`.** É um [`Cell`] de [`ITerminalDimensions`].
//! - **`ptyProcessReady`.** Depois do descarte é um `Option` vazio (o `undefined!` do original).
//! - **`runWhenWindowIdle`.** Não há janela: o relato de latência roda num `set_timeout(0)`.
//! - **`customPtyImplementation`.** É a função de [`IShellLaunchConfig`], que devolve o processo.
//! - **`SeamlessRelaunchDataFilter`.** Os dois gravadores são `Rc<RefCell<TerminalRecorder>>`, e as `MutableDisposable` guardam
//!   a inscrição que grava os dados. O `firstDisposable.value = secondDisposable.value` do original passa a inscrição de um
//!   para o outro (o segundo fica vazio, que é o efeito observável: o original só a descarta uma vez).
//! - **`mainWindow.setTimeout`** é o `runtime::set_timeout`.

use std::cell::{Cell, RefCell};
use std::future::Future;
use std::rc::{Rc, Weak};

use je_base::dynamic::{Dynamic, Truthy};
use je_base::errors::Error;
use je_base::event::{Emitter, Event};
use je_base::js_string::JsString;
use je_base::lifecycle::{Disposable, DisposableRef, DisposableStore, IDisposable, MutableDisposable, to_disposable};
use je_base::map::OrderedMap;
use je_base::nls::localize;
use je_base::platform::{OperatingSystem, Platform};
use je_base::process::ProcessEnvironment;
use je_base::promise::Promise;
use je_base::runtime::{self, TimerId};
use je_base::severity::Severity;
use je_base::uri::URI;
use je_base::uuid::generate_uuid;
use je_instantiation::{Injectable, InstantiationService, ServiceId};
use je_platform::accessibility::I_ACCESSIBILITY_SERVICE;
use je_platform::configuration::service::IConfigurationService;
use je_platform::log::Logger;
use je_platform::notification::notification::{I_NOTIFICATION_SERVICE, Notification};
use je_platform::path::path_service::IPathService;
use je_platform::product::IProductService;
use je_platform::remote::remote_hosts::get_remote_authority;
use je_platform::resources::fs_path;
use je_platform::telemetry::telemetry::I_TELEMETRY_SERVICE;
use je_platform::terminal::capabilities::capabilities::{ISerializedCommandDetectionCapability, ITerminalCapabilityStore, NaiveCwdDetection};
use je_platform::terminal::capabilities::naive_cwd_detection_capability::NaiveCwdDetectionCapability;
use je_platform::terminal::capabilities::terminal_capability_store::TerminalCapabilityStore;
use je_platform::terminal::environment_variable::{
    EnvironmentVariableScope, IEnvironmentVariableCollection, IMergedEnvironmentVariableCollection, VariableResolver as PlatformVariableResolver,
};
use je_platform::terminal::environment_variable_collection::MergedEnvironmentVariableCollection;
use je_platform::terminal::environment_variable_shared::serialize_environment_variable_collections;
use je_platform::terminal::terminal::{
    FlowControlConstants, IProcessDataEvent, IProcessProperty, IProcessReadyEvent, IShellLaunchConfig, ITerminalChildProcess, ITerminalDimensions,
    ITerminalEnvironment, ITerminalLaunchOutcome, ITerminalLogService, ITerminalProcessOptions, ITerminalProcessShellIntegrationOptions, ProcessData,
    ProcessPropertyType, ShellCwd, TerminalEnvironmentValue, UnicodeVersion,
};
use je_platform::terminal::terminal_environment::should_use_environment_variable_collection;
use je_platform::terminal::terminal_recorder::TerminalRecorder;
use je_platform::terminal::terminal_settings::TerminalSettingId;
use je_platform::terminal::terminal_strings::{ITerminalFormatMessageOptions, format_message_for_terminal};
use je_platform::workspace::{IWorkspaceContextService, WorkspaceFolder};

use super::environment_variable_info::{EnvironmentVariableInfoChangesActive, EnvironmentVariableInfoStale};
use super::terminal::{ITerminalConfigurationService, ITerminalInstanceService, ITerminalService};
use crate::contrib::terminal::common::environment_variable::{IEnvironmentVariableInfo, IEnvironmentVariableService};
use crate::contrib::terminal::common::terminal::{
    IBeforeProcessDataEvent, IShellLaunchConfigResolveOptions, ITerminalProcessInfo, ITerminalProcessManager, ITerminalProfileResolverService, ProcessState,
    SharedShellLaunchConfig,
};
use crate::contrib::terminal::common::terminal_backend::ITerminalBackend;
use crate::contrib::terminal::common::terminal_environment::{
    DetectLocale, IConfigurationResolverService as ResolverContract, IHistoryService as HistoryContract, ShellResource, create_terminal_environment,
    create_variable_resolver, get_cwd, get_workspace_for_terminal, merge_environments,
};
use crate::services::configuration_resolver::common::configuration_resolver::IConfigurationResolverService;
use crate::services::environment::environment_service::IWorkbenchEnvironmentService;
use crate::services::history::history::IHistoryService;
use crate::services::remote::remote_agent_service::IRemoteAgentService;

/// `ProcessConstants.ErrorLaunchThresholdDuration`: the amount of time to consider terminal errors to be related to the launch.
const ERROR_LAUNCH_THRESHOLD_DURATION: u64 = 500;

/// `SeamlessRelaunchConstants.SwapWaitMaximumDuration`: the maximum duration after a relaunch occurs to trigger a swap.
const SWAP_WAIT_MAXIMUM_DURATION: u64 = 3000;

/// `TaskSettingId.Reconnection`.
const TASK_RECONNECTION_SETTING: &str = "task.reconnection";

/// `TerminalContribSettingId.SuggestEnabled`.
const SUGGEST_ENABLED_SETTING: &str = "terminal.integrated.suggest.enabled";

/// `ProcessType`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ProcessType {
    Process,
    PseudoTerminal,
}

/// Roda uma tarefa `async` como a função `async` do JavaScript: a promessa assenta com o valor, ou rejeita com o erro.
pub(crate) fn run_async<T: Clone + 'static>(future: impl Future<Output = Result<T, Error>> + 'static) -> Promise<T> {
    let (promise, resolver) = Promise::new();
    runtime::spawn_local(async move {
        match future.await {
            Ok(value) => resolver.resolve(value),
            Err(error) => resolver.reject(error),
        }
    });
    promise
}

/// O `IHistoryService` real como o contrato reduzido do `terminalEnvironment`.
struct HistoryServiceAdapter(Rc<dyn IHistoryService>);

impl HistoryContract for HistoryServiceAdapter {
    fn get_last_active_workspace_root(&self, scheme_filter: Option<&str>, authority_filter: Option<&str>) -> Option<URI> {
        self.0.get_last_active_workspace_root(scheme_filter, authority_filter)
    }
}

/// O `IConfigurationResolverService` real como o contrato reduzido do `terminalEnvironment`.
struct ConfigurationResolverAdapter(Rc<dyn IConfigurationResolverService>);

impl ResolverContract for ConfigurationResolverAdapter {
    fn resolve_with_environment(&self, environment: &ProcessEnvironment, folder: Option<&WorkspaceFolder>, value: &str) -> Promise<String> {
        self.0.resolve_with_environment(environment, folder, value)
    }
}

/// O `ProcessEnvironment` como `ITerminalEnvironment` (todo valor é texto), para o `mergeEnvironments`.
fn to_terminal_environment(environment: &ProcessEnvironment) -> ITerminalEnvironment {
    environment.iter().map(|(key, value)| (key.clone(), TerminalEnvironmentValue::Value(value.clone()))).collect()
}

/// O objeto `{ [key]: string | null | undefined }` de uma configuração como `ITerminalEnvironment`; `None` quando o valor não é
/// um objeto.
fn terminal_environment_from_dynamic(value: &Dynamic) -> Option<ITerminalEnvironment> {
    if !matches!(value, Dynamic::Object(_)) {
        return None;
    }
    Some(
        value
            .own_entries()
            .into_iter()
            .map(|(key, entry)| {
                let entry = match entry {
                    Dynamic::String(text) => TerminalEnvironmentValue::Value(text.to_string_lossy()),
                    Dynamic::Null => TerminalEnvironmentValue::Null,
                    _ => TerminalEnvironmentValue::Undefined,
                };
                (key.to_string_lossy(), entry)
            })
            .collect(),
    )
}

/// O `terminal.integrated.detectLocale` como [`DetectLocale`] (o padrão é `auto`).
fn detect_locale_from_config(value: &Dynamic) -> DetectLocale {
    match je_platform::js::to_js_string(value).to_string_lossy().as_str() {
        "off" => DetectLocale::Off,
        "on" => DetectLocale::On,
        _ => DetectLocale::Auto,
    }
}

/// Os parâmetros estáticos do construtor do [`TerminalProcessManager`].
pub struct TerminalProcessManagerArgs {
    pub instance_id: u32,
    pub cwd: Option<ShellCwd>,
    pub environment_variable_collections: Option<OrderedMap<String, IEnvironmentVariableCollection>>,
    pub shell_integration_nonce: Option<String>,
}

/// Holds all state related to the creation and management of terminal processes.
pub struct TerminalProcessManager {
    this: Weak<Self>,
    /// O `_store` do `Disposable` do original.
    store: Rc<DisposableStore>,
    instance_id: u32,

    process_state: Cell<ProcessState>,
    pty_process_ready: RefCell<Option<Promise<()>>>,
    shell_process_id: Cell<Option<i32>>,
    remote_authority: Option<String>,
    os: Cell<Option<OperatingSystem>>,
    user_home: RefCell<Option<String>>,
    environment_variable_info: RefCell<Option<Rc<dyn IEnvironmentVariableInfo>>>,
    backend: RefCell<Option<Rc<dyn ITerminalBackend>>>,
    capabilities: Rc<TerminalCapabilityStore>,
    shell_integration_nonce: String,
    process_ready_timestamp: Cell<f64>,

    is_disposed: Cell<bool>,
    process: RefCell<Option<Rc<dyn ITerminalChildProcess>>>,
    process_type: Cell<ProcessType>,
    pre_launch_input_queue: RefCell<Vec<String>>,
    initial_cwd: RefCell<Option<String>>,
    ext_environment_variable_collection: RefCell<Option<Rc<dyn IMergedEnvironmentVariableCollection>>>,
    environment_variable_collection_listener: Rc<MutableDisposable<dyn IDisposable>>,
    ack_data_bufferer: AckDataBufferer,
    has_written_data: Cell<bool>,
    has_child_processes: Cell<bool>,
    pty_responsive_listener: RefCell<Option<DisposableRef>>,
    pty_listeners_attached: Cell<bool>,
    data_filter: Rc<SeamlessRelaunchDataFilter>,
    process_listeners: RefCell<Option<Vec<DisposableRef>>>,
    is_disconnected: Cell<bool>,

    process_traits: RefCell<Option<IProcessReadyEvent>>,
    shell_launch_config: RefCell<Option<SharedShellLaunchConfig>>,
    dimensions: Cell<ITerminalDimensions>,

    on_pty_disconnect: Rc<Emitter<()>>,
    on_pty_reconnect: Rc<Emitter<()>>,
    on_process_ready: Rc<Emitter<IProcessReadyEvent>>,
    on_process_state_change: Rc<Emitter<()>>,
    on_before_process_data: Rc<Emitter<Rc<IBeforeProcessDataEvent>>>,
    on_process_data: Rc<Emitter<Rc<IProcessDataEvent>>>,
    on_process_replay_complete: Rc<Emitter<()>>,
    on_did_change_property: Rc<Emitter<IProcessProperty>>,
    on_environment_variable_info_change: Rc<Emitter<Rc<dyn IEnvironmentVariableInfo>>>,
    on_process_exit: Rc<Emitter<Option<i32>>>,
    on_restore_commands: Rc<Emitter<ISerializedCommandDetectionCapability>>,
    cwd_workspace_folder: Option<WorkspaceFolder>,

    history_service: Rc<dyn IHistoryService>,
    instantiation_service: Rc<InstantiationService>,
    log_service: Rc<dyn ITerminalLogService>,
    workspace_context_service: Rc<dyn IWorkspaceContextService>,
    configuration_resolver_service: Option<Rc<dyn IConfigurationResolverService>>,
    workbench_environment_service: Rc<dyn IWorkbenchEnvironmentService>,
    product_service: Rc<dyn IProductService>,
    remote_agent_service: Rc<dyn IRemoteAgentService>,
    path_service: Rc<dyn IPathService>,
    environment_variable_service: Rc<dyn IEnvironmentVariableService>,
    terminal_configuration_service: Rc<dyn ITerminalConfigurationService>,
    terminal_profile_resolver_service: Rc<dyn ITerminalProfileResolverService>,
    configuration_service: Rc<dyn IConfigurationService>,
    terminal_instance_service: Rc<dyn ITerminalInstanceService>,
    terminal_service: Rc<dyn ITerminalService>,
}

impl TerminalProcessManager {
    fn rc(&self) -> Rc<Self> {
        self.this.upgrade().expect("o gerenciador de processo foi descartado da memória")
    }

    /// `os` é um campo público do original (`os: OperatingSystem | undefined`), que o teste do relançamento do pty host troca.
    pub fn set_os(&self, os: Option<OperatingSystem>) {
        self.os.set(os);
    }

    /// Um serviço lido do serviço de instanciação quando usado (ver "Divergências").
    fn optional_service<T: ?Sized + 'static>(&self, id: ServiceId<T>) -> Option<Rc<T>> {
        self.instantiation_service.invoke_function(|accessor| accessor.try_get(id))
    }

    /// `this._configurationService.getValue(key)` com a verdade de JavaScript.
    fn config_flag(&self, key: &str) -> bool {
        self.configuration_service.get_value(Some(key), None).is_truthy()
    }

    /// `TerminalSettingId.ShellIntegrationEnabled`, `TerminalContribSettingId.SuggestEnabled` e o nonce.
    fn process_options(&self) -> ITerminalProcessOptions {
        ITerminalProcessOptions {
            shell_integration: ITerminalProcessShellIntegrationOptions {
                enabled: self.config_flag(TerminalSettingId::ShellIntegrationEnabled.as_str()),
                suggest_enabled: self.config_flag(SUGGEST_ENABLED_SETTING),
                nonce: self.shell_integration_nonce.clone(),
            },
            windows_use_conpty_dll: self.terminal_configuration_service.config().get_str("windowsUseConptyDll").is_truthy(),
            environment_variable_collections: None,
            workspace_folder: self.cwd_workspace_folder.clone(),
            is_screen_reader_optimized: self.optional_service(I_ACCESSIBILITY_SERVICE).is_some_and(|service| service.is_screen_reader_optimized()),
        }
    }

    /// `shouldPersist` do `createProcess` e do `_launchLocalProcess`.
    fn should_persist_new_process(&self, shell_launch_config: &SharedShellLaunchConfig) -> bool {
        let slc = shell_launch_config.borrow();
        let config = self.terminal_configuration_service.config();
        ((self.config_flag(TASK_RECONNECTION_SETTING) && slc.reconnection_properties.is_some()) || !slc.is_feature_terminal.unwrap_or(false))
            && config.get_str("enablePersistentSessions").is_truthy()
            && !slc.is_transient.unwrap_or(false)
    }

    fn unicode_version(&self) -> UnicodeVersion {
        UnicodeVersion::from_config_text(&je_platform::js::to_js_string(&self.terminal_configuration_service.config().get_str("unicodeVersion")).to_string_lossy())
    }

    /// `ShellResource` do `cwd` de um `IShellLaunchConfig`.
    fn workspace_for_cwd(&self, cwd: Option<&ShellCwd>) -> Option<WorkspaceFolder> {
        let resource = cwd.map(|cwd| match cwd {
            ShellCwd::Path(path) => ShellResource::Path(path.as_str()),
            ShellCwd::Uri(uri) => ShellResource::Uri(uri),
        });
        get_workspace_for_terminal(resource, &*self.workspace_context_service, &HistoryServiceAdapter(self.history_service.clone()))
    }

    pub fn free_port_kill_process_async(&self, port: &str) -> Promise<()> {
        let this = self.rc();
        let port = port.to_owned();
        run_async(async move {
            let process = this.process.borrow().clone();
            if let Some(process) = process
                && let Some(killed) = process.free_port_kill_process(&port)
                && let Err(error) = killed.await
            {
                if let Some(notification_service) = this.optional_service(I_NOTIFICATION_SERVICE) {
                    notification_service.notify(Notification::new(
                        Severity::Warning,
                        localize(
                            "killportfailure",
                            "Could not kill process listening on port {0}, command exited with error {1}",
                            &[port.as_str(), error.message.as_str()],
                        ),
                    ));
                }
            }
            Ok(())
        })
    }

    fn create_pty_process_ready_promise(&self) -> Promise<()> {
        let (promise, resolver) = Promise::new();
        let slot: Rc<RefCell<Option<DisposableRef>>> = Rc::new(RefCell::new(None));
        let listener_slot = slot.clone();
        let weak = self.this.clone();
        let listener = self.on_process_ready.event().once().listen(move |_| {
            if let Some(this) = weak.upgrade() {
                this.log_service.debug(&format!("Terminal process ready (shellProcessId: {})", this.shell_process_id.get().map_or_else(|| "undefined".to_owned(), |pid| pid.to_string())));
                let listener = listener_slot.borrow_mut().take();
                if let Some(listener) = listener {
                    this.store.delete(&listener);
                }
            }
            resolver.resolve(());
        });
        *slot.borrow_mut() = Some(listener.clone());
        self.store.add(listener);
        promise
    }

    /// `detachFromProcess`.
    fn detach_from_process_async(&self, force_persist: Option<bool>) -> Promise<()> {
        let this = self.rc();
        run_async(async move {
            let process = this.process.borrow().clone();
            if let Some(process) = process
                && let Some(detached) = process.detach(force_persist)
            {
                detached.await?;
            }
            *this.process.borrow_mut() = None;
            Ok(())
        })
    }

    /// `createProcess(shellLaunchConfig, cols, rows, reset = true)`.
    pub fn create_process_with_reset(&self, shell_launch_config: SharedShellLaunchConfig, cols: i32, rows: i32, reset: bool) -> Promise<Option<ITerminalLaunchOutcome>> {
        let this = self.rc();
        run_async(async move { this.create_process_task(shell_launch_config, cols, rows, reset).await })
    }

    async fn create_process_task(
        self: Rc<Self>,
        shell_launch_config: SharedShellLaunchConfig,
        cols: i32,
        rows: i32,
        reset: bool,
    ) -> Result<Option<ITerminalLaunchOutcome>, Error> {
        *self.shell_launch_config.borrow_mut() = Some(shell_launch_config.clone());
        self.dimensions.set(ITerminalDimensions { cols, rows });

        let mut new_process: Option<Rc<dyn ITerminalChildProcess>> = None;

        let custom_pty_implementation = shell_launch_config.borrow().custom_pty_implementation.clone();
        if let Some(custom_pty_implementation) = custom_pty_implementation {
            self.process_type.set(ProcessType::PseudoTerminal);
            new_process = Some(custom_pty_implementation(self.instance_id, cols, rows));
        } else {
            let Some(backend) = self.terminal_instance_service.get_backend(self.remote_authority.as_deref()).await? else {
                return Err(Error::new(format!(
                    "No terminal backend registered for remote authority '{}'",
                    self.remote_authority.as_deref().unwrap_or("undefined")
                )));
            };
            *self.backend.borrow_mut() = Some(backend.clone());

            // Create variable resolver
            // Start with the full base environment so that all standard variables (e.g. PATH) are available, then overlay the
            // shell environment on top so that launch configuration variables and shell-profile modifications take precedence.
            let mut env_for_resolver = self.terminal_profile_resolver_service.get_environment(self.remote_authority.as_deref()).await?;
            let shell_environment = backend.get_shell_environment().await?;
            merge_environments(&mut env_for_resolver, shell_environment.as_ref().map(to_terminal_environment).as_ref());
            let variable_resolver = create_variable_resolver(
                self.cwd_workspace_folder.as_ref(),
                &env_for_resolver,
                self.configuration_resolver_service.clone().map(|service| Rc::new(ConfigurationResolverAdapter(service)) as Rc<dyn ResolverContract>),
            );

            // resolvedUserHome is needed here as remote resolvers can launch local terminals before they're connected to the remote.
            *self.user_home.borrow_mut() = self.path_service.resolved_user_home().map(|home| fs_path(&home));
            self.os.set(Some(Platform::detect().os));
            if self.remote_authority.as_deref().is_some_and(|authority| !authority.is_empty()) {
                let user_home_uri = self.path_service.user_home().await?;
                *self.user_home.borrow_mut() = Some(user_home_uri.path().to_string_lossy());
                let Some(remote_env) = self.remote_agent_service.get_environment().await? else {
                    return Err(Error::new(format!(
                        "Failed to get remote environment for remote authority \"{}\"",
                        self.remote_authority.as_deref().unwrap_or("undefined")
                    )));
                };
                *self.user_home.borrow_mut() = Some(remote_env.user_home.path().to_string_lossy());
                self.os.set(Some(remote_env.os));

                // this is a copy of what the merged environment collection is on the remote side
                let env = self.resolve_environment(&backend, variable_resolver.as_ref(), &shell_launch_config).await?;
                let should_persist = self.should_persist_new_process(&shell_launch_config);
                let attach_persistent_process_id = shell_launch_config.borrow().attach_persistent_process.as_ref().map(|process| process.id);
                if let Some(attach_persistent_process_id) = attach_persistent_process_id {
                    match backend.attach_to_process(attach_persistent_process_id).await? {
                        Some(result) => new_process = Some(result),
                        None => {
                            // Warn and just create a new terminal if attach failed for some reason
                            self.log_service.warn("Attach to process failed for terminal");
                            shell_launch_config.borrow_mut().attach_persistent_process = None;
                        }
                    }
                }
                if new_process.is_none() {
                    self.terminal_profile_resolver_service
                        .resolve_shell_launch_config(
                            shell_launch_config.clone(),
                            IShellLaunchConfigResolveOptions {
                                remote_authority: self.remote_authority.as_deref(),
                                os: self.os.get().unwrap_or(Platform::detect().os),
                                allow_automation_shell: None,
                                allow_agent_host_shell: None,
                            },
                        )
                        .await?;
                    let mut options = self.process_options();
                    options.environment_variable_collections = self
                        .ext_environment_variable_collection
                        .borrow()
                        .as_ref()
                        .map(|collection| serialize_environment_variable_collections(collection.collections()));
                    let slc = shell_launch_config.borrow().clone();
                    match backend.create_process(&slc, "", cols, rows, self.unicode_version(), &env, &options, should_persist).await {
                        Ok(process) => new_process = Some(process),
                        Err(error) => {
                            if error.message == "Could not fetch remote environment" {
                                self.log_service.trace("Could not fetch remote environment, silently failing");
                                return Ok(None);
                            }
                            return Err(error);
                        }
                    }
                }
                if !self.is_disposed.get() {
                    self.setup_pty_host_listeners(&backend);
                }
            } else {
                let attach = shell_launch_config.borrow().attach_persistent_process.as_ref().map(|process| (process.id, process.pid, process.find_revived_id));
                if let Some((id, pid, find_revived_id)) = attach {
                    let result = if find_revived_id.unwrap_or(false) { backend.attach_to_revived_process(id, pid).await? } else { backend.attach_to_process(id).await? };
                    match result {
                        Some(result) => new_process = Some(result),
                        None => {
                            // Warn and just create a new terminal if attach failed for some reason
                            self.log_service.warn("Attach to process failed for terminal");
                            shell_launch_config.borrow_mut().attach_persistent_process = None;
                        }
                    }
                }
                if new_process.is_none() {
                    let user_home = self.user_home.borrow().clone();
                    new_process = Some(self.launch_local_process(&backend, &shell_launch_config, cols, rows, user_home, variable_resolver.as_ref()).await?);
                }
                if !self.is_disposed.get() {
                    self.setup_pty_host_listeners(&backend);
                }
            }
        }
        let new_process = new_process.expect("o processo foi criado por um dos ramos");

        // If the process was disposed during its creation, shut it down and return failure
        if self.is_disposed.get() {
            new_process.shutdown(false);
            return Ok(None);
        }

        *self.process.borrow_mut() = Some(new_process.clone());
        // Don't report the previous shell's pid for the new process before it's ready
        self.shell_process_id.set(None);
        self.set_process_state(ProcessState::Launching);

        // Add any capabilities inherent to the backend
        if matches!(self.os.get(), Some(OperatingSystem::Linux | OperatingSystem::Macintosh)) {
            self.capabilities.add::<NaiveCwdDetection>(NaiveCwdDetectionCapability::new(new_process.clone()));
        }

        self.data_filter.new_process(new_process.clone(), reset);

        let previous_listeners = self.process_listeners.borrow_mut().take();
        if let Some(previous_listeners) = previous_listeners {
            for listener in &previous_listeners {
                listener.dispose();
            }
        }
        let mut listeners: Vec<DisposableRef> = Vec::new();
        let weak = Rc::downgrade(&self);
        let ready_process = new_process.clone();
        listeners.push(new_process.on_process_ready().listen({
            let weak = weak.clone();
            move |event| {
                let Some(this) = weak.upgrade() else { return };
                this.log_service.debug(&format!("onProcessReady {} {}", event.pid, event.cwd));
                *this.process_traits.borrow_mut() = Some(event.clone());
                this.shell_process_id.set(Some(event.pid));
                *this.initial_cwd.borrow_mut() = Some(event.cwd.clone());
                this.process_ready_timestamp.set(runtime::date_now() as f64);
                this.on_did_change_property.fire(IProcessProperty::InitialCwd(event.cwd.clone()));
                this.on_process_ready.fire(event.clone());

                let queued = if this.process.borrow().is_some() && !this.pre_launch_input_queue.borrow().is_empty() {
                    Some(this.pre_launch_input_queue.borrow().clone())
                } else {
                    None
                };
                if let Some(queued) = queued {
                    // Send any queued data that's waiting
                    this.log_service.debug(&format!("sending prelaunch input queue {queued:?}"));
                    ready_process.input(&queued.join(""));
                    this.pre_launch_input_queue.borrow_mut().clear();
                }
            }
        }));
        listeners.push(new_process.on_process_exit().listen({
            let weak = weak.clone();
            move |exit_code| {
                if let Some(this) = weak.upgrade() {
                    this.on_exit(*exit_code);
                }
            }
        }));
        listeners.push(new_process.on_did_change_property().listen({
            let weak = weak.clone();
            move |property| {
                let Some(this) = weak.upgrade() else { return };
                match property {
                    IProcessProperty::HasChildProcesses(value) => this.has_child_processes.set(*value),
                    IProcessProperty::FailedShellIntegrationActivation(_) => {
                        if let Some(telemetry_service) = this.optional_service(I_TELEMETRY_SERVICE) {
                            telemetry_service.public_log2("terminal/shellIntegrationActivationFailureCustomArgs", None);
                        }
                    }
                    _ => {}
                }
                this.on_did_change_property.fire(property.clone());
            }
        }));
        if let Some(on_process_replay_complete) = new_process.on_process_replay_complete() {
            let weak = weak.clone();
            listeners.push(on_process_replay_complete.listen(move |_| {
                if let Some(this) = weak.upgrade() {
                    this.on_process_replay_complete.fire(());
                }
            }));
        }
        if let Some(on_restore_commands) = new_process.on_restore_commands() {
            let weak = weak.clone();
            listeners.push(on_restore_commands.listen(move |commands| {
                if let Some(this) = weak.upgrade() {
                    this.on_restore_commands.fire(commands.clone());
                }
            }));
        }
        *self.process_listeners.borrow_mut() = Some(listeners);
        {
            let weak = weak.clone();
            runtime::set_timeout(
                move || {
                    if let Some(this) = weak.upgrade()
                        && this.process_state.get() == ProcessState::Launching
                    {
                        this.set_process_state(ProcessState::Running);
                    }
                },
                ERROR_LAUNCH_THRESHOLD_DURATION,
            );
        }

        let result = new_process.start().await?;
        if result.is_some() {
            // Error
            return Ok(result);
        }

        // Report the latency to the pty host when idle
        let weak = Rc::downgrade(&self);
        runtime::set_timeout(
            move || {
                let Some(this) = weak.upgrade() else { return };
                let backend = this.backend.borrow().clone();
                if let Some(backend) = backend {
                    let weak = Rc::downgrade(&this);
                    // O resultado (ou a rejeição) do relato não tem consumidor, como o `.then` sem `catch` do original.
                    let _ = backend.get_latency().then(move |measurements| {
                        if let Some(this) = weak.upgrade() {
                            let lines: Vec<String> = measurements.iter().map(|measurement| format!("{}: {:.2}ms", measurement.label, measurement.latency)).collect();
                            this.log_service.info(&format!(
                                "Latency measurements for {} backend\n{}",
                                this.remote_authority.as_deref().unwrap_or("local"),
                                lines.join("\n")
                            ));
                        }
                    });
                }
            },
            0,
        );

        Ok(None)
    }

    /// `relaunch`.
    pub fn relaunch_process(&self, shell_launch_config: SharedShellLaunchConfig, cols: i32, rows: i32, reset: bool) -> Promise<Option<ITerminalLaunchOutcome>> {
        *self.pty_process_ready.borrow_mut() = Some(self.create_pty_process_ready_promise());
        self.log_service.trace(&format!("Relaunching terminal instance {}", self.instance_id));

        // Fire reconnect if needed to ensure the terminal is usable again
        if self.is_disconnected.get() {
            self.is_disconnected.set(false);
            self.on_pty_reconnect.fire(());
        }

        // Clear data written flag to re-enable seamless relaunch if this relaunch was manually triggered
        self.has_written_data.set(false);

        self.create_process_with_reset(shell_launch_config, cols, rows, reset)
    }

    /// Fetch any extension environment additions and apply them (`_resolveEnvironment`).
    async fn resolve_environment(
        &self,
        backend: &Rc<dyn ITerminalBackend>,
        variable_resolver: Option<&crate::contrib::terminal::common::terminal_environment::VariableResolver>,
        shell_launch_config: &SharedShellLaunchConfig,
    ) -> Result<ProcessEnvironment, Error> {
        let cwd = shell_launch_config.borrow().cwd.clone();
        let workspace_folder = self.workspace_for_cwd(cwd.as_ref());
        let platform = Platform::detect();
        let platform_key = if platform.is_windows {
            "windows"
        } else if platform.is_macintosh {
            "osx"
        } else {
            "linux"
        };
        let env_from_config_value = self.configuration_service.get_value(Some(&format!("terminal.integrated.env.{platform_key}")), None);
        let env_from_config = terminal_environment_from_dynamic(&env_from_config_value);
        let use_shell_environment = shell_launch_config.borrow().use_shell_environment.unwrap_or(false);
        self.log_service.debug(&format!(
            "Resolving environment (useShellEnvironment={use_shell_environment}, platformKey={platform_key}, envFromConfig={})",
            match &env_from_config {
                Some(env) => env.iter().map(|(key, _)| key.as_str()).collect::<Vec<_>>().join(","),
                None => "none".to_owned(),
            }
        ));

        let base_env: ProcessEnvironment;
        if use_shell_environment {
            let Some(shell_env) = backend.get_shell_environment().await? else {
                return Err(Error::named("BugIndicatingError", "Cannot fetch shell environment to use"));
            };
            let mut keys: Vec<&str> = shell_env.keys().map(String::as_str).collect();
            keys.sort_unstable();
            self.log_service.debug(&format!("Shell environment resolved with {} variables: {}", shell_env.len(), keys.join(", ")));
            base_env = shell_env;
        } else {
            base_env = self.terminal_profile_resolver_service.get_environment(self.remote_authority.as_deref()).await?;
            self.log_service.debug(&format!("Profile environment resolved with {} variables", base_env.len()));
        }
        let detect_locale = detect_locale_from_config(&self.terminal_configuration_service.config().get_str("detectLocale"));
        let mut slc = shell_launch_config.borrow().clone();
        let mut env =
            create_terminal_environment(&mut slc, env_from_config.as_ref(), variable_resolver, Some(self.product_service.version()), detect_locale, &base_env).await;
        shell_launch_config.borrow_mut().env = slc.env.clone();
        {
            let mut keys: Vec<&str> = env.keys().map(String::as_str).collect();
            keys.sort_unstable();
            self.log_service.debug(&format!("Terminal environment created with {} variables: {}", env.len(), keys.join(", ")));
        }
        self.environment_variable_collection_listener.clear();
        if !self.is_disposed.get() && should_use_environment_variable_collection(&slc) {
            let collection = self.environment_variable_service.merged_collection();
            *self.ext_environment_variable_collection.borrow_mut() = Some(collection.clone());

            let weak = self.this.clone();
            self.environment_variable_collection_listener.set(Some(self.environment_variable_service.on_did_change_collections().listen(move |new_collection| {
                if let Some(this) = weak.upgrade() {
                    this.on_environment_variable_collection_change(new_collection.clone());
                }
            })));
            // For remote terminals, this is a copy of the mergedEnvironmentCollection created on the remote side. Since the
            // environment collection is synced between the remote and local sides immediately this is a fairly safe way of
            // enabling the env var diffing and info widget. While technically these could differ due to the slight change of
            // a race condition, the chance is minimal plus the impact on the user is also not that great if it happens - it's
            // not worth adding plumbing to sync back the resolved collection.
            let scope = EnvironmentVariableScope { workspace_folder };
            let resolver_ref: Option<PlatformVariableResolver<'_>> = variable_resolver.map(|resolver| &**resolver);
            collection.apply_to_process_environment(&mut env, Some(&scope), resolver_ref).await?;
            if !collection.get_variable_map(Some(&scope)).is_empty() {
                let info: Rc<dyn IEnvironmentVariableInfo> = self.instantiation_service.create_instance::<EnvironmentVariableInfoChangesActive>((collection.clone(),));
                *self.environment_variable_info.borrow_mut() = Some(info.clone());
                self.on_environment_variable_info_change.fire(info);
            }
        }
        Ok(env)
    }

    async fn launch_local_process(
        &self,
        backend: &Rc<dyn ITerminalBackend>,
        shell_launch_config: &SharedShellLaunchConfig,
        cols: i32,
        rows: i32,
        user_home: Option<String>,
        variable_resolver: Option<&crate::contrib::terminal::common::terminal_environment::VariableResolver>,
    ) -> Result<Rc<dyn ITerminalChildProcess>, Error> {
        self.terminal_profile_resolver_service
            .resolve_shell_launch_config(
                shell_launch_config.clone(),
                IShellLaunchConfigResolveOptions { remote_authority: None, os: Platform::detect().os, allow_automation_shell: None, allow_agent_host_shell: None },
            )
            .await?;
        let active_workspace_root_uri = self.history_service.get_last_active_workspace_root(Some("file"), None);

        let custom_cwd = {
            let cwd = self.terminal_configuration_service.config().get_str("cwd");
            if cwd.is_truthy() { Some(je_platform::js::to_js_string(&cwd).to_string_lossy()) } else { None }
        };
        let slc_for_cwd = shell_launch_config.borrow().clone();
        let initial_cwd = get_cwd(&slc_for_cwd, user_home.as_deref(), variable_resolver, active_workspace_root_uri.as_ref(), custom_cwd.as_deref(), Some(&*self.log_service)).await;

        let env = self.resolve_environment(backend, variable_resolver, shell_launch_config).await?;

        let mut options = self.process_options();
        options.environment_variable_collections = self
            .ext_environment_variable_collection
            .borrow()
            .as_ref()
            .map(|collection| serialize_environment_variable_collections(collection.collections()));
        let should_persist = self.should_persist_new_process(shell_launch_config);
        let slc = shell_launch_config.borrow().clone();
        backend.create_process(&slc, &initial_cwd, cols, rows, self.unicode_version(), &env, &options, should_persist).await
    }

    fn setup_pty_host_listeners(&self, backend: &Rc<dyn ITerminalBackend>) {
        if self.pty_listeners_attached.get() {
            return;
        }
        self.pty_listeners_attached.set(true);

        // Mark the process as disconnected is the pty host is unresponsive, the responsive event will fire only when the pty
        // host was already unresponsive
        let weak = self.this.clone();
        self.store.add(backend.on_pty_host_unresponsive().listen({
            let weak = weak.clone();
            move |_| {
                if let Some(this) = weak.upgrade() {
                    this.is_disconnected.set(true);
                    this.on_pty_disconnect.fire(());
                }
            }
        }));
        *self.pty_responsive_listener.borrow_mut() = Some(backend.on_pty_host_responsive().listen({
            let weak = weak.clone();
            move |_| {
                if let Some(this) = weak.upgrade() {
                    this.is_disconnected.set(false);
                    this.on_pty_reconnect.fire(());
                }
            }
        }));
        self.store.add(to_disposable({
            let weak = weak.clone();
            move || {
                if let Some(this) = weak.upgrade() {
                    let listener = this.pty_responsive_listener.borrow().clone();
                    if let Some(listener) = listener {
                        listener.dispose();
                    }
                }
            }
        }));

        // When the pty host restarts, reconnect is no longer possible so dispose the responsive listener
        self.store.add(backend.on_pty_host_restart().listen({
            let weak = weak.clone();
            move |_| {
                let Some(task) = weak.upgrade() else { return };
                // O corpo da função `async` do original roda de forma síncrona até o primeiro `await` (o do `relaunch`), por isso
                // a mensagem de reinício sai antes de o `fire` do evento voltar.
                {
                    // When the pty host restarts, reconnect is no longer possible
                    if !task.is_disconnected.get() {
                        task.is_disconnected.set(true);
                        task.on_pty_disconnect.fire(());
                    }
                    let listener = task.pty_responsive_listener.borrow_mut().take();
                    if let Some(listener) = listener {
                        listener.dispose();
                    }
                    let shell_launch_config = task.shell_launch_config.borrow().clone();
                    if let Some(shell_launch_config) = shell_launch_config {
                        let (is_feature_terminal, has_reconnection_properties) = {
                            let slc = shell_launch_config.borrow();
                            (slc.is_feature_terminal.unwrap_or(false), task.reconnection_properties_present(&slc))
                        };
                        if is_feature_terminal && !has_reconnection_properties {
                            // Indicate the process is exited (and gone forever) only for feature terminals so they can react
                            // to the exit, this is particularly important for tasks so that it knows that the process is not
                            // still active. Note that this is not done for regular terminals because otherwise the terminal
                            // instance would be disposed.
                            task.on_exit(Some(-1));
                        } else {
                            // For normal terminals write a message indicating what happened and relaunch using the previous
                            // shellLaunchConfig
                            let message = localize("ptyHostRelaunch", "Restarting the terminal because the connection to the shell process was lost...", &[]);
                            // Align with the pty service's revive logic (_reviveTerminalProcess in
                            // src/vs/platform/terminal/node/ptyService.ts) to hedge against PSReadLine `GetConsoleCursorInfo`
                            // and cursor handling from conpty.
                            let dimensions = task.dimensions.get();
                            let mut post_restart_message = String::new();
                            if task.os.get() == Some(OperatingSystem::Windows) && dimensions.rows > 0 {
                                post_restart_message = "\r\n".repeat((dimensions.rows - 1) as usize) + "\x1b[H";
                            }
                            task.on_process_data.fire(IProcessDataEvent::new(
                                format!(
                                    "{}{post_restart_message}",
                                    format_message_for_terminal(&message, ITerminalFormatMessageOptions { exclude_leading_new_line: false, loud_formatting: true })
                                ),
                                false,
                            ));
                            // O `await` do `relaunch` não tem consumidor: o resultado (ou a rejeição) é descartado.
                            let _ = task.relaunch_process(shell_launch_config, dimensions.cols, dimensions.rows, false);
                        }
                    }
                }
            }
        }));
        self.store.add(to_disposable({
            let weak = weak.clone();
            move || {
                if let Some(this) = weak.upgrade() {
                    *this.pty_process_ready.borrow_mut() = None;
                }
            }
        }));
    }

    /// `reconnectionProperties` com o `IShellLaunchConfig` já emprestado.
    fn reconnection_properties_present(&self, slc: &IShellLaunchConfig) -> bool {
        slc.attach_persistent_process.as_ref().and_then(|process| process.reconnection_properties.as_ref()).is_some() || slc.reconnection_properties.is_some()
    }

    fn resize(&self, cols: i32, rows: i32, pixel_width: Option<i32>, pixel_height: Option<i32>) -> Result<(), Error> {
        let process = self.process.borrow().clone();
        let Some(process) = process else { return Ok(()) };
        // The child process could already be terminated
        if let Err(error) = process.resize(cols, rows, pixel_width, pixel_height) {
            // We tried to write to a closed pipe / channel.
            let is_closed_channel = error.props.iter().any(|(key, value)| key == "code" && (value == "EPIPE" || value == "ERR_IPC_CHANNEL_CLOSED"));
            if !is_closed_channel {
                return Err(error);
            }
        }
        self.dimensions.set(ITerminalDimensions { cols, rows });
        Ok(())
    }

    fn on_exit(&self, exit_code: Option<i32>) {
        *self.process.borrow_mut() = None;
        // If the process is marked as launching then mark the process as killed during launch. This typically means that there
        // is a problem with the shell and args.
        if self.process_state.get() == ProcessState::Launching {
            self.set_process_state(ProcessState::KilledDuringLaunch);
        }

        // If TerminalInstance did not know about the process exit then it was triggered by the process, not on VS Code's side.
        if self.process_state.get() == ProcessState::Running {
            self.set_process_state(ProcessState::KilledByProcess);
        }

        self.on_process_exit.fire(exit_code);
    }

    fn set_process_state(&self, state: ProcessState) {
        self.process_state.set(state);
        self.on_process_state_change.fire(());
    }

    fn on_environment_variable_collection_change(&self, new_collection: Rc<dyn IMergedEnvironmentVariableCollection>) {
        let Some(ext_collection) = self.ext_environment_variable_collection.borrow().clone() else { return };
        let scope = EnvironmentVariableScope { workspace_folder: self.cwd_workspace_folder.clone() };
        let diff = ext_collection.diff(&*new_collection, Some(&scope));
        let Some(diff) = diff else {
            // If there are no longer differences, remove the stale info indicator
            let is_stale = self.environment_variable_info.borrow().as_ref().is_some_and(|info| info.requires_action());
            if is_stale {
                let info: Rc<dyn IEnvironmentVariableInfo> = self.instantiation_service.create_instance::<EnvironmentVariableInfoChangesActive>((ext_collection,));
                *self.environment_variable_info.borrow_mut() = Some(info.clone());
                self.on_environment_variable_info_change.fire(info);
            }
            return;
        };
        let info: Rc<dyn IEnvironmentVariableInfo> = self.instantiation_service.create_instance::<EnvironmentVariableInfoStale>((diff, self.instance_id, new_collection));
        *self.environment_variable_info.borrow_mut() = Some(info.clone());
        self.on_environment_variable_info_change.fire(info);
    }
}

impl ITerminalProcessInfo for TerminalProcessManager {
    fn process_state(&self) -> ProcessState {
        self.process_state.get()
    }

    fn pty_process_ready(&self) -> Promise<()> {
        self.pty_process_ready.borrow().clone().unwrap_or_else(|| Promise::resolved(()))
    }

    fn shell_process_id(&self) -> Option<i32> {
        self.shell_process_id.get()
    }

    fn remote_authority(&self) -> Option<String> {
        self.remote_authority.clone()
    }

    fn os(&self) -> Option<OperatingSystem> {
        self.os.get()
    }

    fn user_home(&self) -> Option<String> {
        self.user_home.borrow().clone()
    }

    fn initial_cwd(&self) -> String {
        self.initial_cwd.borrow().clone().unwrap_or_default()
    }

    fn environment_variable_info(&self) -> Option<Rc<dyn IEnvironmentVariableInfo>> {
        self.environment_variable_info.borrow().clone()
    }

    fn persistent_process_id(&self) -> Option<u32> {
        self.process.borrow().as_ref().map(|process| process.id())
    }

    fn should_persist(&self) -> bool {
        let has_reconnection_properties = self
            .shell_launch_config
            .borrow()
            .as_ref()
            .is_some_and(|slc| self.reconnection_properties_present(&slc.borrow()));
        has_reconnection_properties || self.process.borrow().as_ref().is_some_and(|process| process.should_persist())
    }

    fn has_written_data(&self) -> bool {
        self.has_written_data.get()
    }

    fn has_child_processes(&self) -> bool {
        self.has_child_processes.get()
    }

    fn backend(&self) -> Option<Rc<dyn ITerminalBackend>> {
        self.backend.borrow().clone()
    }

    fn capabilities(&self) -> Rc<dyn ITerminalCapabilityStore> {
        self.capabilities.clone()
    }

    fn shell_integration_nonce(&self) -> String {
        self.shell_integration_nonce.clone()
    }

    fn ext_environment_variable_collection(&self) -> Option<Rc<dyn IMergedEnvironmentVariableCollection>> {
        self.ext_environment_variable_collection.borrow().clone()
    }
}

impl ITerminalProcessManager for TerminalProcessManager {
    fn process_traits(&self) -> Option<IProcessReadyEvent> {
        self.process_traits.borrow().clone()
    }

    fn process_ready_timestamp(&self) -> f64 {
        self.process_ready_timestamp.get()
    }

    fn on_pty_disconnect(&self) -> Event<()> {
        self.on_pty_disconnect.event()
    }

    fn on_pty_reconnect(&self) -> Event<()> {
        self.on_pty_reconnect.event()
    }

    fn on_process_ready(&self) -> Event<IProcessReadyEvent> {
        self.on_process_ready.event()
    }

    fn on_before_process_data(&self) -> Event<Rc<IBeforeProcessDataEvent>> {
        self.on_before_process_data.event()
    }

    fn on_process_data(&self) -> Event<Rc<IProcessDataEvent>> {
        self.on_process_data.event()
    }

    fn on_process_replay_complete(&self) -> Event<()> {
        self.on_process_replay_complete.event()
    }

    fn on_environment_variable_info_changed(&self) -> Event<Rc<dyn IEnvironmentVariableInfo>> {
        self.on_environment_variable_info_change.event()
    }

    fn on_did_change_property(&self) -> Event<IProcessProperty> {
        self.on_did_change_property.event()
    }

    fn on_process_exit(&self) -> Event<Option<i32>> {
        self.on_process_exit.event()
    }

    fn on_restore_commands(&self) -> Event<ISerializedCommandDetectionCapability> {
        self.on_restore_commands.event()
    }

    fn dispose_immediate(&self, immediate: bool) {
        self.is_disposed.set(true);
        let process = self.process.borrow().clone();
        if let Some(process) = process {
            // If the process was still connected this dispose came from within VS Code, not the process, so mark the process as
            // killed by the user.
            self.set_process_state(ProcessState::KilledByUser);
            process.shutdown(immediate);
            *self.process.borrow_mut() = None;
        }
        let listeners = self.process_listeners.borrow_mut().take();
        if let Some(listeners) = listeners {
            for listener in &listeners {
                listener.dispose();
            }
        }
        self.store.dispose();
    }

    fn detach_from_process(&self, force_persist: Option<bool>) -> Promise<()> {
        self.detach_from_process_async(force_persist)
    }

    fn create_process(&self, shell_launch_config: SharedShellLaunchConfig, cols: i32, rows: i32) -> Promise<Option<ITerminalLaunchOutcome>> {
        self.create_process_with_reset(shell_launch_config, cols, rows, true)
    }

    fn relaunch(&self, shell_launch_config: SharedShellLaunchConfig, cols: i32, rows: i32, reset: bool) -> Promise<Option<ITerminalLaunchOutcome>> {
        self.relaunch_process(shell_launch_config, cols, rows, reset)
    }

    fn write(&self, data: &str) -> Promise<()> {
        let this = self.rc();
        let data = data.to_owned();
        run_async(async move {
            this.pty_process_ready().await?;
            this.data_filter.disable_seamless_relaunch();
            this.has_written_data.set(true);
            if this.shell_process_id.get().is_some_and(|pid| pid != 0) || this.process_type.get() == ProcessType::PseudoTerminal {
                let process = this.process.borrow().clone();
                if let Some(process) = process {
                    // Send data if the pty is ready
                    process.input(&data);
                }
            } else {
                // If the pty is not ready, queue the data received to send later
                this.log_service.debug(&format!("queueing data in prelaunch input queue {data:?}"));
                this.pre_launch_input_queue.borrow_mut().push(data);
            }
            Ok(())
        })
    }

    fn send_signal(&self, signal: &str) -> Promise<()> {
        let this = self.rc();
        let signal = signal.to_owned();
        run_async(async move {
            this.pty_process_ready().await?;
            let process = this.process.borrow().clone();
            if let Some(process) = process {
                process.send_signal(&signal);
            }
            Ok(())
        })
    }

    fn set_dimensions(&self, cols: i32, rows: i32, pixel_width: Option<i32>, pixel_height: Option<i32>) -> Promise<()> {
        // Resizing a disposed pty is a contractual no-op so re-entrant resizes during the synchronous teardown stack (#315282)
        // are silently dropped.
        if self.store.is_disposed() {
            return Promise::resolved(());
        }
        let Some(pty_process_ready) = self.pty_process_ready.borrow().clone() else {
            return Promise::rejected(Error::new("TerminalProcessManager.setDimensions called before initialization"));
        };
        let this = self.rc();
        pty_process_ready.then_result(move |ready| {
            ready?;
            this.resize(cols, rows, pixel_width, pixel_height)
        })
    }

    fn set_dimensions_sync(&self, cols: i32, rows: i32, pixel_width: Option<i32>, pixel_height: Option<i32>) {
        // O `_resize` síncrono do original lança fora do `try` quando o erro não é de pipe fechado; o `Result` é o `throw`.
        if let Err(error) = self.resize(cols, rows, pixel_width, pixel_height) {
            je_base::errors::throw(error);
        }
    }

    fn clear_buffer(&self) -> Promise<()> {
        let process = self.process.borrow().clone();
        match process {
            Some(process) => process.clear_buffer(),
            None => Promise::resolved(()),
        }
    }

    fn set_unicode_version(&self, version: UnicodeVersion) -> Promise<()> {
        let process = self.process.borrow().clone();
        match process {
            Some(process) => process.set_unicode_version(version),
            None => Promise::resolved(()),
        }
    }

    fn set_next_command_id(&self, command_line: &str, command_id: &str) -> Promise<()> {
        let this = self.rc();
        let command_line = command_line.to_owned();
        let command_id = command_id.to_owned();
        run_async(async move {
            this.pty_process_ready().await?;
            let process = this.process.borrow().clone();
            let Some(process) = process else { return Ok(()) };
            let id = process.id();
            if id == 0 {
                return Ok(());
            }
            this.terminal_service.set_next_command_id(id, &command_line, &command_id).await
        })
    }

    fn acknowledge_data_event(&self, char_count: usize) {
        self.ack_data_bufferer.ack(char_count);
    }

    fn process_binary(&self, data: &str) -> Promise<()> {
        let this = self.rc();
        let data = data.to_owned();
        run_async(async move {
            this.pty_process_ready().await?;
            this.data_filter.disable_seamless_relaunch();
            this.has_written_data.set(true);
            let process = this.process.borrow().clone();
            if let Some(process) = process {
                // O `processBinary` do original não é aguardado.
                let _ = process.process_binary(&data);
            }
            Ok(())
        })
    }

    fn refresh_property(&self, property_type: ProcessPropertyType) -> Promise<IProcessProperty> {
        let process = self.process.borrow().clone();
        match process {
            Some(process) => process.refresh_property(property_type),
            None => Promise::rejected(Error::new("Cannot refresh property when process is not set")),
        }
    }

    fn update_property(&self, property: IProcessProperty) -> Promise<()> {
        let process = self.process.borrow().clone();
        match process {
            Some(process) => process.update_property(property),
            None => Promise::resolved(()),
        }
    }

    fn get_backend_os(&self) -> Promise<OperatingSystem> {
        let this = self.rc();
        run_async(async move {
            let mut os = Platform::detect().os;
            if this.remote_authority.as_deref().is_some_and(|authority| !authority.is_empty()) {
                let Some(remote_env) = this.remote_agent_service.get_environment().await? else {
                    return Err(Error::new(format!(
                        "Failed to get remote environment for remote authority \"{}\"",
                        this.remote_authority.as_deref().unwrap_or("undefined")
                    )));
                };
                os = remote_env.os;
            }
            Ok(os)
        })
    }

    fn free_port_kill_process(&self, port: &str) -> Promise<()> {
        self.free_port_kill_process_async(port)
    }
}

impl IDisposable for TerminalProcessManager {
    fn dispose(&self) {
        ITerminalProcessManager::dispose_immediate(self, false);
    }
}

impl Injectable for TerminalProcessManager {
    type Args = TerminalProcessManagerArgs;
    type Deps = (
        Rc<dyn IHistoryService>,
        Rc<InstantiationService>,
        Rc<dyn ITerminalLogService>,
        Rc<dyn IWorkspaceContextService>,
        Option<Rc<dyn IConfigurationResolverService>>,
        Rc<dyn IWorkbenchEnvironmentService>,
        Rc<dyn IProductService>,
        Rc<dyn IRemoteAgentService>,
        Rc<dyn IPathService>,
        Rc<dyn IEnvironmentVariableService>,
        Rc<dyn ITerminalConfigurationService>,
        Rc<dyn ITerminalProfileResolverService>,
        Rc<dyn IConfigurationService>,
        Rc<dyn ITerminalInstanceService>,
        Rc<dyn ITerminalService>,
    );

    fn create(
        args: TerminalProcessManagerArgs,
        (
            history_service,
            instantiation_service,
            log_service,
            workspace_context_service,
            configuration_resolver_service,
            workbench_environment_service,
            product_service,
            remote_agent_service,
            path_service,
            environment_variable_service,
            terminal_configuration_service,
            terminal_profile_resolver_service,
            configuration_service,
            terminal_instance_service,
            terminal_service,
        ): Self::Deps,
    ) -> Rc<Self> {
        let TerminalProcessManagerArgs { instance_id, cwd, environment_variable_collections, shell_integration_nonce } = args;
        let store = DisposableStore::new();
        let capabilities = TerminalCapabilityStore::new();
        store.add_rc(capabilities.clone());

        let cwd_workspace_folder = {
            let resource = cwd.as_ref().map(|cwd| match cwd {
                ShellCwd::Path(path) => ShellResource::Path(path.as_str()),
                ShellCwd::Uri(uri) => ShellResource::Uri(uri),
            });
            get_workspace_for_terminal(resource, &*workspace_context_service, &HistoryServiceAdapter(history_service.clone()))
        };

        let remote_authority = match &cwd {
            Some(ShellCwd::Uri(uri)) => get_remote_authority(uri).map(|authority| authority.to_string_lossy()),
            _ => workbench_environment_service.remote_authority(),
        };

        let environment_variable_collection_listener: Rc<MutableDisposable<dyn IDisposable>> = MutableDisposable::new();
        store.add_rc(environment_variable_collection_listener.clone());
        let data_filter = SeamlessRelaunchDataFilter::create((), (log_service.clone(),));
        store.add_rc(data_filter.clone());

        let on_pty_disconnect = store.add_rc(Emitter::new());
        let on_pty_reconnect = store.add_rc(Emitter::new());
        let on_process_ready = store.add_rc(Emitter::new());
        let on_process_state_change = store.add_rc(Emitter::new());
        let on_before_process_data = store.add_rc(Emitter::new());
        let on_process_data = store.add_rc(Emitter::new());
        let on_process_replay_complete = store.add_rc(Emitter::new());
        let on_did_change_property = store.add_rc(Emitter::new());
        let on_environment_variable_info_change = store.add_rc(Emitter::new());
        let on_process_exit = store.add_rc(Emitter::new());
        let on_restore_commands = store.add_rc(Emitter::new());

        let this = Rc::new_cyclic(|weak: &Weak<Self>| {
            let ack_weak = weak.clone();
            Self {
                this: weak.clone(),
                store,
                instance_id,
                process_state: Cell::new(ProcessState::Uninitialized),
                pty_process_ready: RefCell::new(None),
                shell_process_id: Cell::new(None),
                remote_authority,
                os: Cell::new(None),
                user_home: RefCell::new(None),
                environment_variable_info: RefCell::new(None),
                backend: RefCell::new(None),
                capabilities,
                shell_integration_nonce: shell_integration_nonce.unwrap_or_else(generate_uuid),
                process_ready_timestamp: Cell::new(0.0),
                is_disposed: Cell::new(false),
                process: RefCell::new(None),
                process_type: Cell::new(ProcessType::Process),
                pre_launch_input_queue: RefCell::new(Vec::new()),
                initial_cwd: RefCell::new(None),
                ext_environment_variable_collection: RefCell::new(None),
                environment_variable_collection_listener,
                ack_data_bufferer: AckDataBufferer::new(Rc::new(move |char_count| {
                    if let Some(this) = ack_weak.upgrade() {
                        let process = this.process.borrow().clone();
                        if let Some(process) = process {
                            process.acknowledge_data_event(char_count);
                        }
                    }
                })),
                has_written_data: Cell::new(false),
                has_child_processes: Cell::new(false),
                pty_responsive_listener: RefCell::new(None),
                pty_listeners_attached: Cell::new(false),
                data_filter,
                process_listeners: RefCell::new(None),
                is_disconnected: Cell::new(false),
                process_traits: RefCell::new(None),
                shell_launch_config: RefCell::new(None),
                dimensions: Cell::new(ITerminalDimensions { cols: 0, rows: 0 }),
                on_pty_disconnect,
                on_pty_reconnect,
                on_process_ready,
                on_process_state_change,
                on_before_process_data,
                on_process_data,
                on_process_replay_complete,
                on_did_change_property,
                on_environment_variable_info_change,
                on_process_exit,
                on_restore_commands,
                cwd_workspace_folder,
                history_service,
                instantiation_service,
                log_service,
                workspace_context_service,
                configuration_resolver_service,
                workbench_environment_service,
                product_service,
                remote_agent_service,
                path_service,
                environment_variable_service,
                terminal_configuration_service,
                terminal_profile_resolver_service,
                configuration_service,
                terminal_instance_service,
                terminal_service,
            }
        });

        *this.pty_process_ready.borrow_mut() = Some(this.create_pty_process_ready_promise());

        let weak = Rc::downgrade(&this);
        this.store.add(this.data_filter.on_process_data().listen(move |event| {
            let Some(this) = weak.upgrade() else { return };
            let (data, shared_event) = match event {
                ProcessData::Text(text) => (text.clone(), None),
                ProcessData::Event(event) => (event.data.borrow().clone(), Some(event.clone())),
            };
            let before_process_data_event = Rc::new(IBeforeProcessDataEvent { data: RefCell::new(data) });
            this.on_before_process_data.fire(before_process_data_event.clone());
            let new_data = before_process_data_event.data.borrow().clone();
            if !new_data.is_empty() {
                match shared_event {
                    // This event is used by the caller so the object must be reused
                    Some(shared_event) => {
                        *shared_event.data.borrow_mut() = new_data;
                        this.on_process_data.fire(shared_event);
                    }
                    None => this.on_process_data.fire(IProcessDataEvent::new(new_data, false)),
                }
            }
        }));

        if let Some(collections) = environment_variable_collections {
            let collection: Rc<dyn IMergedEnvironmentVariableCollection> = Rc::new(MergedEnvironmentVariableCollection::new(collections));
            *this.ext_environment_variable_collection.borrow_mut() = Some(collection.clone());
            let weak = Rc::downgrade(&this);
            this.environment_variable_collection_listener.set(Some(this.environment_variable_service.on_did_change_collections().listen(move |new_collection| {
                if let Some(this) = weak.upgrade() {
                    this.on_environment_variable_collection_change(new_collection.clone());
                }
            })));
            let info: Rc<dyn IEnvironmentVariableInfo> = this.instantiation_service.create_instance::<EnvironmentVariableInfoChangesActive>((collection,));
            *this.environment_variable_info.borrow_mut() = Some(info.clone());
            this.on_environment_variable_info_change.fire(info);
        }

        this
    }
}

/// `AckDataBufferer`.
struct AckDataBufferer {
    unsent_char_count: Cell<usize>,
    callback: Rc<dyn Fn(usize)>,
}

impl AckDataBufferer {
    fn new(callback: Rc<dyn Fn(usize)>) -> Self {
        Self { unsent_char_count: Cell::new(0), callback }
    }

    fn ack(&self, char_count: usize) {
        self.unsent_char_count.set(self.unsent_char_count.get() + char_count);
        while self.unsent_char_count.get() > FlowControlConstants::CHAR_COUNT_ACK_SIZE {
            self.unsent_char_count.set(self.unsent_char_count.get() - FlowControlConstants::CHAR_COUNT_ACK_SIZE);
            (self.callback)(FlowControlConstants::CHAR_COUNT_ACK_SIZE);
        }
    }
}

/// Filters data events from the process and supports seamlessly restarting swapping out the process with another, delaying the
/// swap in output in order to minimize flickering/clearing of the terminal.
pub struct SeamlessRelaunchDataFilter {
    base: Rc<Disposable>,
    this: Weak<Self>,
    first_recorder: RefCell<Option<Rc<RefCell<TerminalRecorder>>>>,
    second_recorder: RefCell<Option<Rc<RefCell<TerminalRecorder>>>>,
    first_disposable: Rc<MutableDisposable<dyn IDisposable>>,
    second_disposable: Rc<MutableDisposable<dyn IDisposable>>,
    data_listener: Rc<MutableDisposable<dyn IDisposable>>,
    active_process: RefCell<Option<Rc<dyn ITerminalChildProcess>>>,
    disable_seamless_relaunch: Cell<bool>,
    swap_timeout: Cell<Option<TimerId>>,
    on_process_data: Rc<Emitter<ProcessData>>,

    log_service: Rc<dyn ITerminalLogService>,
}

impl SeamlessRelaunchDataFilter {
    /// `onProcessData`.
    pub fn on_process_data(&self) -> Event<ProcessData> {
        self.on_process_data.event()
    }

    /// A inscrição que repassa os dados do processo ao emissor do filtro (`process.onProcessData(e => this._onProcessData.fire(e))`).
    fn forward_process_data(&self, process: &Rc<dyn ITerminalChildProcess>) -> DisposableRef {
        let emitter = self.on_process_data.clone();
        process.on_process_data().listen(move |data| emitter.fire(data.clone()))
    }

    pub fn new_process(&self, process: Rc<dyn ITerminalChildProcess>, reset: bool) {
        // Stop listening to the old process and trigger delayed shutdown (for hang issue #71966)
        self.data_listener.clear();
        let previous_process = self.active_process.borrow().clone();
        if let Some(previous_process) = previous_process {
            previous_process.shutdown(false);
        }

        *self.active_process.borrow_mut() = Some(process.clone());

        // Start firing events immediately if:
        // - there's no recorder, which means it's a new terminal
        // - this is not a reset, so seamless relaunch isn't necessary
        // - seamless relaunch is disabled because the terminal has accepted input
        if self.first_recorder.borrow().is_none() || !reset || self.disable_seamless_relaunch.get() {
            let (recorder, disposable) = Self::create_recorder(&process);
            *self.first_recorder.borrow_mut() = Some(recorder);
            self.first_disposable.set(Some(disposable));
            if self.disable_seamless_relaunch.get() && reset {
                self.on_process_data.fire(ProcessData::Text("\x1bc".to_owned()));
            }
            self.data_listener.set(Some(self.forward_process_data(&process)));
            self.disable_seamless_relaunch.set(false);
            return;
        }

        // Trigger a swap if there was a recent relaunch
        if self.second_recorder.borrow().is_some() {
            self.trigger_swap();
        }

        let weak = self.this.clone();
        self.swap_timeout.set(Some(runtime::set_timeout(
            move || {
                if let Some(this) = weak.upgrade() {
                    this.trigger_swap();
                }
            },
            SWAP_WAIT_MAXIMUM_DURATION,
        )));

        // Pause all outgoing data events
        self.data_listener.clear();

        self.first_disposable.clear();
        let (recorder, disposable) = Self::create_recorder(&process);
        *self.second_recorder.borrow_mut() = Some(recorder);
        self.second_disposable.set(Some(disposable));
    }

    /// Disables seamless relaunch for the active process.
    pub fn disable_seamless_relaunch(&self) {
        self.disable_seamless_relaunch.set(true);
        self.stop_recording();
        self.trigger_swap();
    }

    /// Trigger the swap of the processes if needed (eg. timeout, input).
    pub fn trigger_swap(&self) {
        // Clear the swap timeout if it exists
        if let Some(swap_timeout) = self.swap_timeout.take() {
            runtime::clear_timeout(swap_timeout);
        }

        // Do nothing if there's nothing being recorder
        let Some(first_recorder) = self.first_recorder.borrow().clone() else { return };
        // Clear the first recorder if no second process was attached before the swap trigger
        let Some(second_recorder) = self.second_recorder.borrow().clone() else {
            *self.first_recorder.borrow_mut() = None;
            self.first_disposable.clear();
            return;
        };

        // Generate data for each recorder
        let first_data = Self::get_data_from_recorder(&first_recorder);
        let second_data = Self::get_data_from_recorder(&second_recorder);

        // Re-write the terminal if the data differs
        if first_data == second_data {
            self.log_service.trace("Seamless terminal relaunch - identical content");
        } else {
            self.log_service.trace("Seamless terminal relaunch - resetting content");
            // Fire full reset (RIS) followed by the new data so the update happens in the same frame
            self.on_process_data.fire(ProcessData::Event(IProcessDataEvent::new(format!("\x1bc{second_data}"), false)));
        }

        // Set up the new data listener
        let active_process = self.active_process.borrow().clone();
        if let Some(active_process) = active_process {
            self.data_listener.set(Some(self.forward_process_data(&active_process)));
        }

        // Replace first recorder with second
        *self.first_recorder.borrow_mut() = Some(second_recorder);
        self.first_disposable.set(self.second_disposable.clear_and_leak());
        *self.second_recorder.borrow_mut() = None;
    }

    fn stop_recording(&self) {
        // Continue recording if a swap is coming
        if self.swap_timeout.get().is_some() {
            return;
        }
        // Stop recording
        *self.first_recorder.borrow_mut() = None;
        self.first_disposable.clear();
        *self.second_recorder.borrow_mut() = None;
        self.second_disposable.clear();
    }

    fn create_recorder(process: &Rc<dyn ITerminalChildProcess>) -> (Rc<RefCell<TerminalRecorder>>, DisposableRef) {
        let recorder = Rc::new(RefCell::new(TerminalRecorder::new(0.0, 0.0)));
        let recording = recorder.clone();
        let disposable = process.on_process_data().listen(move |event| recording.borrow_mut().handle_data(JsString::from(event.data())));
        (recorder, disposable)
    }

    fn get_data_from_recorder(recorder: &Rc<RefCell<TerminalRecorder>>) -> String {
        recorder
            .borrow_mut()
            .generate_replay_event_sync()
            .events
            .iter()
            .filter(|event| !event.data.is_empty())
            .map(|event| event.data.to_string_lossy())
            .collect::<Vec<_>>()
            .join("")
    }
}

impl Injectable for SeamlessRelaunchDataFilter {
    type Args = ();
    type Deps = (Rc<dyn ITerminalLogService>,);

    fn create(_: (), (log_service,): Self::Deps) -> Rc<Self> {
        let base = Disposable::new();
        let first_disposable: Rc<MutableDisposable<dyn IDisposable>> = MutableDisposable::new();
        base.register(first_disposable.clone());
        let second_disposable: Rc<MutableDisposable<dyn IDisposable>> = MutableDisposable::new();
        base.register(second_disposable.clone());
        let data_listener: Rc<MutableDisposable<dyn IDisposable>> = MutableDisposable::new();
        base.register(data_listener.clone());
        let on_process_data = Emitter::new();
        base.register(on_process_data.clone());
        Rc::new_cyclic(|this| Self {
            base,
            this: this.clone(),
            first_recorder: RefCell::new(None),
            second_recorder: RefCell::new(None),
            first_disposable,
            second_disposable,
            data_listener,
            active_process: RefCell::new(None),
            disable_seamless_relaunch: Cell::new(false),
            swap_timeout: Cell::new(None),
            on_process_data,
            log_service,
        })
    }
}

impl IDisposable for SeamlessRelaunchDataFilter {
    fn dispose(&self) {
        self.base.dispose();
    }
}
