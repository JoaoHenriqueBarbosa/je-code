//! Porte de `ITerminalBackend` (`src/vs/platform/terminal/common/terminal.ts`): o backend que cria e conecta os processos
//! do terminal (local, remoto ou o do pty host).
//!
//! Fica no workbench, e não em `je_platform::terminal::terminal`, porque herda o [`ITerminalBackendWslPath`] (o
//! `Pick<ITerminalBackend, 'getWslPath'>` que o `terminalEnvironment.ts` lê), que vive aqui.
//!
//! Divergências:
//! - Ficam de fora os membros que dependem de tipos de layout e de detalhes de processo ainda não portados:
//!   `listProcesses`, `getProfiles`, `setTerminalLayoutInfo`, `getTerminalLayoutInfo`, `getPerformanceMarks`,
//!   `reduceConnectionGraceTime`, `requestDetachInstance`, `acceptDetachInstanceReply`, `persistTerminalState` e o
//!   `ITerminalBackendPtyServiceContributions` (`installAutoReply` e `uninstallAllAutoReplies`).
//! - `IProcessEnvironment` é o [`ProcessEnvironment`] do `je-base`; `getShellEnvironment(): Promise<IProcessEnvironment |
//!   undefined>` é uma promessa de `Option`.
//! - `createProcess` recebe o `IShellLaunchConfig` por referência (o backend o lê; o objeto continua dono da instância).

use std::rc::Rc;

use je_base::event::Event;
use je_base::platform::OperatingSystem;
use je_base::process::ProcessEnvironment;
use je_base::promise::Promise;
use je_platform::terminal::terminal::{
    IShellLaunchConfig, ITerminalChildProcess, ITerminalProcessOptions, TerminalIcon, TitleEventSource, UnicodeVersion,
};

use super::terminal_environment::ITerminalBackendWslPath;

/// `IPtyHostLatencyMeasurement`.
#[derive(Clone, Debug, PartialEq)]
pub struct IPtyHostLatencyMeasurement {
    pub label: String,
    pub latency: f64,
}

/// O `{ requestId, workspaceId, instanceId }` de `onDidRequestDetach`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct IDetachInstanceRequest {
    pub request_id: u32,
    pub workspace_id: String,
    pub instance_id: u32,
}

/// `ITerminalBackend`.
pub trait ITerminalBackend: ITerminalBackendWslPath {
    fn remote_authority(&self) -> Option<String>;

    fn is_responsive(&self) -> bool;

    /// A promise that resolves when the backend is ready to be used, ie. after terminal persistence has been actioned.
    fn when_ready(&self) -> Promise<()>;

    /// Signal to the backend that persistence has been actioned and is ready for use.
    fn set_ready(&self);

    /// Fired when the ptyHost process becomes non-responsive, this should disable stdin for all terminals using this pty
    /// host connection and mark them as disconnected.
    fn on_pty_host_unresponsive(&self) -> Event<()>;

    /// Fired when the ptyHost process becomes responsive after being non-responsive. Allowing previously disconnected
    /// terminals to reconnect.
    fn on_pty_host_responsive(&self) -> Event<()>;

    /// Fired when the ptyHost has been restarted, this is used as a signal for listening terminals that its pty has been
    /// lost and will remain disconnected.
    fn on_pty_host_restart(&self) -> Event<()>;

    fn on_did_request_detach(&self) -> Event<IDetachInstanceRequest>;

    fn attach_to_process(&self, id: u32) -> Promise<Option<Rc<dyn ITerminalChildProcess>>>;

    /// Attaches to the process revived for a restored terminal, otherwise to its original process. The local backend
    /// refuses an original process whose pid differs from `pid`, unless `pid` is 0 (unknown).
    fn attach_to_revived_process(&self, id: u32, pid: u32) -> Promise<Option<Rc<dyn ITerminalChildProcess>>>;

    fn get_latency(&self) -> Promise<Vec<IPtyHostLatencyMeasurement>>;
    fn get_default_system_shell(&self, os_override: Option<OperatingSystem>) -> Promise<String>;
    fn get_environment(&self) -> Promise<ProcessEnvironment>;
    fn get_shell_environment(&self) -> Promise<Option<ProcessEnvironment>>;
    fn update_title(&self, id: u32, title: &str, title_source: TitleEventSource) -> Promise<()>;
    fn update_icon(&self, id: u32, user_initiated: bool, icon: &TerminalIcon, color: Option<&str>) -> Promise<()>;
    fn set_next_command_id(&self, id: u32, command_line: &str, command_id: &str) -> Promise<()>;

    #[allow(clippy::too_many_arguments)] // os parâmetros são os do original
    fn create_process(
        &self,
        shell_launch_config: &IShellLaunchConfig,
        cwd: &str,
        cols: i32,
        rows: i32,
        unicode_version: UnicodeVersion,
        env: &ProcessEnvironment,
        options: &ITerminalProcessOptions,
        should_persist: bool,
    ) -> Promise<Rc<dyn ITerminalChildProcess>>;

    fn restart_pty_host(&self);
}
