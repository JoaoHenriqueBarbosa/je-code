//! Porte parcial de `src/vs/platform/terminal/common/terminal.ts` (continuação do [`super::terminal`]): o lado do pty
//! host e do pty service. `terminalProcess.ts` entra junto, porque as duas coisas se importam.
//!
//! Portado aqui: `TerminalIpcChannels`, `HeartbeatConstants`, `IHeartbeatService`, os tipos de layout
//! (`IRawTerminalInstanceLayoutInfo` e companhia, `IPtyHostAttachTarget`), `IProcessDetails`,
//! `ISetTerminalLayoutInfoArgs`, `IGetTerminalLayoutInfoArgs`, `IPtyService`, `IPtyServiceContribution`,
//! `IPtyHostController`, `IPtyHostService`, `ILocalPtyService`, `IPtyHostLatencyMeasurement`,
//! `ISerializedTerminalState`, `IPersistentTerminalProcessLaunchConfig` e `IRequestResolveVariablesEvent`.
//!
//! Representação:
//! - Um `Event<{ id: number; event: ... }>` do `IPtyService` é um [`Event`] da struct com `id` e `event`
//!   ([`PtyProcessData`], [`PtyProcessReady`]...), e cada membro `readonly onX` é o método `on_x()`.
//! - `unicodeVersion: '6' | '11'` é [`UnicodeVersion`]; o `'unix-to-win' | 'win-to-unix'` de `getWslPath` é
//!   [`WslPathDirection`]; `IProcessEnvironment` é o [`ProcessEnvironment`] do `je-base`.
//! - `refreshIgnoreProcessNames?` (membro opcional) devolve `Option<Promise<()>>`: `None` é o membro ausente, e o
//!   padrão do trait é ausente.
//! - O `IProcessProperty<T>` de `refreshProperty` e `updateProperty` é o enum [`IProcessProperty`], com a variante
//!   escolhida pelo [`ProcessPropertyType`].
//! - `IRawTerminalsLayoutInfo<T>.background: T[] | null` é `Option<Vec<T>>`.
//! - `ILocalPtyService` (`extends IPtyHostService`) é o trait [`ILocalPtyService`], que também é um
//!   [`IProxyService`]: é o que o `ProxyChannel.fromService` do `createLocalPtyChannel` precisa ler.

use std::rc::Rc;

use je_base::event::Event;
use je_base::lifecycle::IDisposable;
use je_base::parts::ipc::ProxyChannel::IProxyService;
use je_base::performance::PerformanceMark;
use je_base::platform::OperatingSystem;
use je_base::process::ProcessEnvironment;
use je_base::promise::Promise;
use je_instantiation::service_id;

use super::environment_variable::ISerializableEnvironmentVariableCollections;
use super::terminal::{
    IFixedTerminalDimensions, IFreedPortProcess, IProcessProperty, IProcessReadyEvent, IReconnectionProperties, IShellLaunchConfig, ITerminalChildProcess, ITerminalLaunchOutcome,
    ITerminalProcessOptions, ITerminalProfile, ITerminalTabAction, ProcessData, ProcessPropertyType, TerminalIcon, TerminalType, TitleEventSource, UnicodeVersion, WaitOnExitValue,
};
use super::terminal_recorder::IPtyHostProcessReplayEvent;

/// `TerminalIpcChannels`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum TerminalIpcChannels {
    /// Communicates between the renderer process and shared process.
    LocalPty,
    /// Communicates between the shared process and the pty host process.
    PtyHost,
    /// Communicates between the renderer process and the pty host process.
    PtyHostWindow,
    /// Deals with logging from the pty host process.
    Logger,
    /// Enables the detection of unresponsive pty hosts.
    Heartbeat,
}

impl TerminalIpcChannels {
    /// O valor do original (o nome do canal).
    pub fn as_str(self) -> &'static str {
        match self {
            Self::LocalPty => "localPty",
            Self::PtyHost => "ptyHost",
            Self::PtyHostWindow => "ptyHostWindow",
            Self::Logger => "logger",
            Self::Heartbeat => "heartbeat",
        }
    }
}

/// `HeartbeatConstants`.
pub struct HeartbeatConstants;

impl HeartbeatConstants {
    /// The duration between heartbeats
    pub const BEAT_INTERVAL: u64 = 5000;
    /// The duration of the first heartbeat while the pty host is starting up. This is much larger
    /// than the regular BeatInterval to accommodate slow machines, we still want to warn about the
    /// pty host's unresponsiveness eventually though.
    pub const CONNECTING_BEAT_INTERVAL: u64 = 20000;
    /// Defines a multiplier for BeatInterval for how long to wait before starting the second wait
    /// timer.
    pub const FIRST_WAIT_MULTIPLIER: f64 = 1.2;
    /// Defines a multiplier for BeatInterval for how long to wait before telling the user about
    /// non-responsiveness. The second timer is to avoid informing the user incorrectly when waking
    /// the computer up from sleep
    pub const SECOND_WAIT_MULTIPLIER: f64 = 1.0;
    /// How long to wait before telling the user about non-responsiveness when they try to create a
    /// process. This short circuits the standard wait timeouts to tell the user sooner and only
    /// create process is handled to avoid additional perf overhead.
    pub const CREATE_PROCESS_TIMEOUT: u64 = 5000;
}

/// `IHeartbeatService`.
pub trait IHeartbeatService {
    fn on_beat(&self) -> Event<()>;
}

/// `IRawTerminalInstanceLayoutInfo<T>`.
#[derive(Clone)]
pub struct IRawTerminalInstanceLayoutInfo<T> {
    pub relative_size: f64,
    pub terminal: T,
}

/// `IRawTerminalTabLayoutInfo<T>`.
#[derive(Clone)]
pub struct IRawTerminalTabLayoutInfo<T> {
    pub is_active: bool,
    pub active_persistent_process_id: Option<u32>,
    pub terminals: Vec<IRawTerminalInstanceLayoutInfo<T>>,
}

/// `ITerminalTabLayoutInfoById`.
pub type ITerminalTabLayoutInfoById = IRawTerminalTabLayoutInfo<u32>;

/// `IRawTerminalsLayoutInfo<T>`.
#[derive(Clone)]
pub struct IRawTerminalsLayoutInfo<T> {
    pub tabs: Vec<IRawTerminalTabLayoutInfo<T>>,
    pub background: Option<Vec<T>>,
}

/// `IPtyHostAttachTarget`.
#[derive(Clone)]
pub struct IPtyHostAttachTarget {
    pub id: u32,
    pub pid: i32,
    pub title: String,
    pub title_source: TitleEventSource,
    pub cwd: String,
    pub workspace_id: String,
    pub workspace_name: String,
    pub is_orphan: bool,
    pub icon: Option<TerminalIcon>,
    pub fixed_dimensions: Option<IFixedTerminalDimensions>,
    pub environment_variable_collections: Option<ISerializableEnvironmentVariableCollections>,
    pub reconnection_properties: Option<IReconnectionProperties>,
    pub wait_on_exit: Option<WaitOnExitValue>,
    pub hide_from_user: Option<bool>,
    pub is_feature_terminal: Option<bool>,
    pub type_: Option<TerminalType>,
    pub has_child_processes: bool,
    pub shell_integration_nonce: String,
    pub tab_actions: Option<Vec<ITerminalTabAction>>,
}

/// `ITerminalsLayoutInfo`: `IRawTerminalsLayoutInfo<IPtyHostAttachTarget | null>`.
pub type ITerminalsLayoutInfo = IRawTerminalsLayoutInfo<Option<IPtyHostAttachTarget>>;

/// `ITerminalsLayoutInfoById`.
pub type ITerminalsLayoutInfoById = IRawTerminalsLayoutInfo<u32>;

/// `ISetTerminalLayoutInfoArgs`.
#[derive(Clone)]
pub struct ISetTerminalLayoutInfoArgs {
    pub workspace_id: String,
    pub tabs: Vec<ITerminalTabLayoutInfoById>,
    pub background: Option<Vec<u32>>,
}

/// `IGetTerminalLayoutInfoArgs`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct IGetTerminalLayoutInfoArgs {
    pub workspace_id: String,
}

/// `IProcessDetails`.
#[derive(Clone)]
pub struct IProcessDetails {
    pub id: u32,
    pub pid: i32,
    pub title: String,
    pub title_source: TitleEventSource,
    pub cwd: String,
    pub workspace_id: String,
    pub workspace_name: String,
    pub is_orphan: bool,
    pub icon: Option<TerminalIcon>,
    pub color: Option<String>,
    pub fixed_dimensions: Option<IFixedTerminalDimensions>,
    pub environment_variable_collections: Option<ISerializableEnvironmentVariableCollections>,
    pub reconnection_properties: Option<IReconnectionProperties>,
    pub wait_on_exit: Option<WaitOnExitValue>,
    pub hide_from_user: Option<bool>,
    pub is_feature_terminal: Option<bool>,
    pub type_: Option<TerminalType>,
    pub has_child_processes: bool,
    pub shell_integration_nonce: String,
    pub tab_actions: Option<Vec<ITerminalTabAction>>,
}

/// `IPtyHostLatencyMeasurement`.
#[derive(Clone, Debug, PartialEq)]
pub struct IPtyHostLatencyMeasurement {
    pub label: String,
    pub latency: f64,
}

/// `IPersistentTerminalProcessLaunchConfig`.
#[derive(Clone)]
pub struct IPersistentTerminalProcessLaunchConfig {
    pub env: ProcessEnvironment,
    pub executable_env: ProcessEnvironment,
    pub options: ITerminalProcessOptions,
}

/// `ISerializedTerminalState`.
#[derive(Clone)]
pub struct ISerializedTerminalState {
    pub id: u32,
    pub shell_launch_config: IShellLaunchConfig,
    pub process_details: IProcessDetails,
    pub process_launch_config: IPersistentTerminalProcessLaunchConfig,
    pub unicode_version: UnicodeVersion,
    pub replay_event: IPtyHostProcessReplayEvent,
    pub timestamp: f64,
}

/// `IRequestResolveVariablesEvent`. O `requestId` é o do [`super::request_store::RequestEvent`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct IRequestResolveVariablesEvent {
    pub request_id: u64,
    pub workspace_id: String,
    pub original_text: Vec<String>,
}

/// O `'unix-to-win' | 'win-to-unix'` de `getWslPath`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WslPathDirection {
    UnixToWin,
    WinToUnix,
}

impl WslPathDirection {
    /// O texto do original.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::UnixToWin => "unix-to-win",
            Self::WinToUnix => "win-to-unix",
        }
    }
}

/// `{ id: number; event: IProcessDataEvent | string }` de `onProcessData`.
#[derive(Clone)]
pub struct PtyProcessData {
    pub id: u32,
    pub event: ProcessData,
}

/// `{ id: number; event: IProcessReadyEvent }` de `onProcessReady`.
#[derive(Clone)]
pub struct PtyProcessReady {
    pub id: u32,
    pub event: IProcessReadyEvent,
}

/// `{ id: number; event: IPtyHostProcessReplayEvent }` de `onProcessReplay`.
#[derive(Clone)]
pub struct PtyProcessReplay {
    pub id: u32,
    pub event: IPtyHostProcessReplayEvent,
}

/// `{ id: number }` de `onProcessOrphanQuestion`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PtyProcessOrphanQuestion {
    pub id: u32,
}

/// `{ requestId: number; workspaceId: string; instanceId: number }` de `onDidRequestDetach`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PtyDidRequestDetach {
    pub request_id: u32,
    pub workspace_id: String,
    pub instance_id: u32,
}

/// `{ id: number; property: IProcessProperty }` de `onDidChangeProperty`.
#[derive(Clone)]
pub struct PtyDidChangeProperty {
    pub id: u32,
    pub property: IProcessProperty,
}

/// `{ id: number; event: number | undefined }` de `onProcessExit`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PtyProcessExit {
    pub id: u32,
    pub event: Option<i32>,
}

/// A service that communicates with a pty host.
pub trait IPtyService {
    fn on_process_data(&self) -> Event<PtyProcessData>;
    fn on_process_ready(&self) -> Event<PtyProcessReady>;
    fn on_process_replay(&self) -> Event<PtyProcessReplay>;
    fn on_process_orphan_question(&self) -> Event<PtyProcessOrphanQuestion>;
    fn on_did_request_detach(&self) -> Event<PtyDidRequestDetach>;
    fn on_did_change_property(&self) -> Event<PtyDidChangeProperty>;
    fn on_process_exit(&self) -> Event<PtyProcessExit>;

    #[allow(clippy::too_many_arguments)]
    fn create_process(
        &self,
        shell_launch_config: &IShellLaunchConfig,
        cwd: &str,
        cols: i32,
        rows: i32,
        unicode_version: UnicodeVersion,
        env: &ProcessEnvironment,
        executable_env: &ProcessEnvironment,
        options: &ITerminalProcessOptions,
        should_persist: bool,
        workspace_id: &str,
        workspace_name: &str,
    ) -> Promise<u32>;
    /// Attaches to a persistent process, failing if `pid` is provided and differs from its process ID.
    /// Persistent process IDs restart with the pty host, so a saved ID can later belong to another process.
    fn attach_to_process(&self, id: u32, pid: Option<i32>) -> Promise<()>;
    fn detach_from_process(&self, id: u32, force_persist: Option<bool>) -> Promise<()>;
    fn shutdown_all(&self) -> Promise<()>;

    /// Lists all orphaned processes, ie. those without a connected frontend.
    fn list_processes(&self) -> Promise<Vec<IProcessDetails>>;
    fn get_performance_marks(&self) -> Promise<Vec<PerformanceMark>>;
    /// Measures and returns the latency of the current and all other processes to the pty host.
    fn get_latency(&self) -> Promise<Vec<IPtyHostLatencyMeasurement>>;

    fn start(&self, id: u32) -> Promise<Option<ITerminalLaunchOutcome>>;
    fn shutdown(&self, id: u32, immediate: bool) -> Promise<()>;
    fn input(&self, id: u32, data: &str) -> Promise<()>;
    fn send_signal(&self, id: u32, signal: &str) -> Promise<()>;
    fn resize(&self, id: u32, cols: i32, rows: i32, pixel_width: Option<i32>, pixel_height: Option<i32>) -> Promise<()>;
    fn clear_buffer(&self, id: u32) -> Promise<()>;
    fn get_initial_cwd(&self, id: u32) -> Promise<String>;
    fn get_cwd(&self, id: u32) -> Promise<String>;
    fn acknowledge_data_event(&self, id: u32, char_count: usize) -> Promise<()>;
    fn set_next_command_id(&self, id: u32, command_line: &str, command_id: &str) -> Promise<()>;
    fn set_unicode_version(&self, id: u32, version: UnicodeVersion) -> Promise<()>;
    fn process_binary(&self, id: u32, data: &str) -> Promise<()>;
    /// Confirm the process is _not_ an orphan.
    fn orphan_question_reply(&self, id: u32) -> Promise<()>;
    fn update_title(&self, id: u32, title: &str, title_source: TitleEventSource) -> Promise<()>;
    fn update_icon(&self, id: u32, user_initiated: bool, icon: &TerminalIcon, color: Option<&str>) -> Promise<()>;

    fn get_default_system_shell(&self, os_override: Option<OperatingSystem>) -> Promise<String>;
    fn get_environment(&self) -> Promise<ProcessEnvironment>;
    fn get_wsl_path(&self, original: &str, direction: WslPathDirection) -> Promise<String>;
    fn get_revived_pty_new_id(&self, workspace_id: &str, id: u32) -> Promise<Option<u32>>;
    fn set_terminal_layout_info(&self, args: &ISetTerminalLayoutInfoArgs) -> Promise<()>;
    fn get_terminal_layout_info(&self, args: &IGetTerminalLayoutInfoArgs) -> Promise<Option<ITerminalsLayoutInfo>>;
    fn reduce_connection_grace_time(&self) -> Promise<()>;
    fn request_detach_instance(&self, workspace_id: &str, instance_id: u32) -> Promise<Option<IProcessDetails>>;
    fn accept_detach_instance_reply(&self, request_id: u32, persistent_process_id: Option<u32>) -> Promise<()>;
    fn free_port_kill_process(&self, port: &str) -> Promise<IFreedPortProcess>;
    /// Serializes and returns terminal state.
    /// `ids`: the persistent terminal IDs to serialize.
    fn serialize_terminal_state(&self, ids: &[u32]) -> Promise<String>;
    /// Revives a workspaces terminal processes, these can then be reconnected to using the normal
    /// flow for restoring terminals after reloading.
    fn revive_terminal_processes(&self, workspace_id: &str, state: &[ISerializedTerminalState], date_time_format_locate: &str) -> Promise<()>;
    fn refresh_property(&self, id: u32, property: ProcessPropertyType) -> Promise<IProcessProperty>;
    fn update_property(&self, id: u32, property: &IProcessProperty) -> Promise<()>;

    /// `refreshIgnoreProcessNames?` (TODO do original: torná-lo obrigatório e tirar a implementação do pty host service).
    /// `None` é o membro ausente.
    fn refresh_ignore_process_names(&self, _names: &[String]) -> Option<Promise<()>> {
        None
    }

    // #region Pty service contribution RPC calls

    fn install_auto_reply(&self, match_: &str, reply: &str) -> Promise<()>;
    fn uninstall_all_auto_replies(&self) -> Promise<()>;

    // #endregion
}

service_id!(pub I_PTY_SERVICE: dyn IPtyService = "ptyService");

/// `IPtyServiceContribution`.
pub trait IPtyServiceContribution {
    fn handle_process_ready(&self, persistent_process_id: u32, process: Rc<dyn ITerminalChildProcess>);
    fn handle_process_dispose(&self, persistent_process_id: u32);
    fn handle_process_input(&self, persistent_process_id: u32, data: &str);
    fn handle_process_resize(&self, persistent_process_id: u32, cols: i32, rows: i32, pixel_width: Option<i32>, pixel_height: Option<i32>);
}

/// `IPtyHostController`.
pub trait IPtyHostController {
    fn on_pty_host_exit(&self) -> Event<i32>;
    fn on_pty_host_start(&self) -> Event<()>;
    fn on_pty_host_unresponsive(&self) -> Event<()>;
    fn on_pty_host_responsive(&self) -> Event<()>;
    fn on_pty_host_request_resolve_variables(&self) -> Event<IRequestResolveVariablesEvent>;

    fn restart_pty_host(&self) -> Promise<()>;
    fn accept_pty_host_resolved_variables(&self, request_id: u64, resolved: Vec<String>) -> Promise<()>;
    /// `profiles` e `defaultProfile` são `unknown` no original (o valor cru da configuração).
    fn get_profiles(
        &self,
        workspace_id: &str,
        profiles: &je_base::dynamic::Dynamic,
        default_profile: &je_base::dynamic::Dynamic,
        include_detected_profiles: bool,
    ) -> Promise<Vec<ITerminalProfile>>;
}

/// A service that communicates with a pty host controller (eg. main or server
/// process) and is able to launch and forward requests to the pty host.
pub trait IPtyHostService: IPtyService + IPtyHostController {}

impl<T: IPtyService + IPtyHostController + ?Sized> IPtyHostService for T {}

/// `ILocalPtyService`: a service responsible for communicating with the pty host process on Electron.
///
/// If the pty host is running locally, this is the service that routes the requests to it.
pub trait ILocalPtyService: IPtyHostService + IProxyService + IDisposable {}

impl<T: IPtyHostService + IProxyService + IDisposable + ?Sized> ILocalPtyService for T {}

service_id!(pub I_LOCAL_PTY_SERVICE: dyn ILocalPtyService = "localPtyService");
