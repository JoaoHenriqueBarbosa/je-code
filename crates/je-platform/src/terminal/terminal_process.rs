//! Porte de `src/vs/platform/terminal/node/terminalProcess.ts`: o processo de um terminal local, um pty que roda o shell,
//! com validação do diretório e do executável, injeção da integração de shell, controle de fluxo, título do processo,
//! encerramento e propriedades do processo.
//!
//! Divergências:
//! - O `node-pty` é o [`RawPty`] do `je-terminal` (o pty do Alacritty). Não há callbacks vindos de outra thread: o laço de
//!   eventos consulta o pty a cada [`PTY_POLL_INTERVAL_MS`] ms e entrega os dados, o fim do processo e o resto na ordem em
//!   que a thread de leitura os enfileirou. `ptyProcess.pid > 0` vale sempre no Unix, então o ramo do `node-pty` que espera
//!   o primeiro dado para ter o pid (ConPTY adiado) não existe.
//! - `class TerminalProcess extends Disposable` é um [`Disposable`] por composição, e o `TerminalProcess` é um `Rc` (o
//!   estado mutável do original, `this._x`, é `Cell` ou `RefCell`; nenhum empréstimo fica aberto quando um evento é
//!   disparado ou uma promessa é aguardada). O `protected _throttleKillSpawn()` que um teste sobrescreve numa subclasse é
//!   [`TerminalProcess::override_throttle_kill_spawn`].
//! - `Reflect.get(terminal, '_ptyProcess')` e `'_titleInterval'` dos testes são [`TerminalProcess::observe`].
//! - No Windows o pty cru é o ConPTY do Alacritty (ver [`je_terminal::pty`]), `kill` com sinal falha e `process` é o nome do
//!   executável, como no `node-pty`. O `WindowsShellHelper` ([`WindowsShellHelper`]) e o `DelayedResizer` ([`DelayedResizer`],
//!   só do `Git\bin\bash.exe`, com `cols` e `rows` zero) são os do original.
//! - `exitMessage` nunca é escrito no original e o getter não existe aqui. `ptyProcess.process` é o nome do grupo de
//!   processos em primeiro plano (ver [`RawPty::process`]).
//! - Os timers de encerramento (`_closeTimeout` e o de `MaximumShutdownTime`) são cancelados no `dispose`; o original
//!   deixa o timer disparar e ele retorna ao ver o store descartado, o efeito é o mesmo.
//! - `input` e `sendSignal` sem pty são no-op (o original lança `TypeError` ao desreferenciar `undefined`).
//! - O ambiente do filho é o dado mais o do processo pai (ver [`je_terminal::pty`]).

use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::path::PathBuf;
use std::rc::{Rc, Weak};

use je_base::async_::{IntervalHandle, async_fn, clear_interval, set_interval, timeout};
use je_base::errors::Error;
use je_base::event::{Emitter, Event};
use je_base::lifecycle::{Disposable, IDisposable, to_disposable};
use je_base::network::FileAccess;
use je_base::nls::localize;
use je_base::node::processes::find_executable;
use je_base::node::windows_version::get_windows_build_number_sync;
use je_base::path::{PathModule, ProcessEnvironment as HostEnvironment};
use je_base::platform::Platform;
use je_base::process::ProcessEnvironment;
use je_base::promise::{DeferredPromise, Promise};
use je_base::runtime::{self, TimerId};
use je_terminal::pty::{PtyEvent, PtySpawn, RawPty};

use super::child_process_monitor::ChildProcessMonitor;
use super::delayed_resizer::DelayedResizer;
use super::windows_shell_helper::WindowsShellHelper;
use super::node_terminal_environment::{
    OptionalProcessEnvironment, ShellIntegrationInjection, get_shell_integration_injection, sanitize_env_for_logging,
};
use super::terminal::{
    FlowControlConstants, GeneralShellType, IFixedTerminalDimensions, IProcessProperty, IProcessReadyEvent, IProcessReadyWindowsPty, IShellLaunchConfig, ITerminalChildProcess,
    ITerminalLaunchError, ITerminalLaunchOutcome, ITerminalLaunchResult, ITerminalProcessOptions, PosixShellType, ProcessData, ProcessPropertyType, ShellCwd, ShellType,
    TerminalEnvironmentValue, TerminalShellType, UnicodeVersion, WindowsPtyBackend,
};
use crate::log::{LogArg, LogLevel, LogServiceRef, Logger};
use crate::product::IProductService;

/// De quanto em quanto tempo o laço de eventos consulta o pty (ver a nota de divergência no topo).
pub const PTY_POLL_INTERVAL_MS: u64 = 4;

/// `ShutdownConstants.DataFlushTimeout`: the amount of ms that must pass between data events after exit is queued before
/// the actual kill call is triggered.
const DATA_FLUSH_TIMEOUT: u64 = 250;
/// `ShutdownConstants.MaximumShutdownTime`: the maximum ms to allow after dispose is called because forcefully killing the
/// process.
const MAXIMUM_SHUTDOWN_TIME: u64 = 5000;
/// `Constants.KillSpawnThrottleInterval`: the minimum duration between kill and spawn calls on Windows/conpty.
const KILL_SPAWN_THROTTLE_INTERVAL: u64 = 250;
/// `Constants.KillSpawnSpacingDuration`: the amount of time to wait when a call is throttled beyond the exact amount.
const KILL_SPAWN_SPACING_DURATION: u64 = 50;
/// O intervalo do sorteio do título do processo (`setInterval(..., 200)`).
const TITLE_POLL_INTERVAL_MS: u64 = 200;

thread_local! {
    /// `TerminalProcess._lastKillOrStart`.
    static LAST_KILL_OR_START: Cell<u64> = const { Cell::new(0) };
}

fn posix_shell_type(name: &str) -> Option<ShellType> {
    let shell = match name {
        "bash" => PosixShellType::Bash,
        "csh" => PosixShellType::Csh,
        "fish" => PosixShellType::Fish,
        "ksh" => PosixShellType::Ksh,
        "sh" => PosixShellType::Sh,
        "zsh" => PosixShellType::Zsh,
        _ => return None,
    };
    Some(ShellType::from(shell))
}

fn general_shell_type(name: &str) -> Option<ShellType> {
    let shell = match name {
        "claude" => GeneralShellType::Claude,
        "codex" => GeneralShellType::Codex,
        "commandcode" => GeneralShellType::CommandCode,
        "copilot" => GeneralShellType::Copilot,
        "gemini" => GeneralShellType::Gemini,
        "pwsh" | "powershell" => GeneralShellType::PowerShell,
        "python" => GeneralShellType::Python,
        "julia" => GeneralShellType::Julia,
        "nu" => GeneralShellType::NuShell,
        "node" => GeneralShellType::Node,
        "xonsh" => GeneralShellType::Xonsh,
        _ => return None,
    };
    Some(ShellType::from(shell))
}

/// `posixShellTypeMap.get(name) || generalShellTypeMap.get(name)`.
fn known_shell_type(name: &str) -> TerminalShellType {
    posix_shell_type(name).or_else(|| general_shell_type(name))
}

/// O que `TerminalProcess._properties` guarda e o `refreshProperty` compara.
struct ProcessProperties {
    cwd: String,
    initial_cwd: String,
    fixed_dimensions: IFixedTerminalDimensions,
}

/// O que os testes leem por `Reflect.get` (ver [`TerminalProcess::observe`]).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TerminalProcessObservation {
    /// `_ptyProcess !== undefined`.
    pub native_pty_present: bool,
    /// `_titleInterval !== undefined`.
    pub title_polling: bool,
}

/// O processo de um terminal local.
pub struct TerminalProcess {
    me: Weak<TerminalProcess>,
    base: Rc<Disposable>,
    shell_launch_config: RefCell<IShellLaunchConfig>,
    /// Environment used for `findExecutable`.
    executable_env: ProcessEnvironment,
    options: ITerminalProcessOptions,
    log_service: LogServiceRef,
    product_service: Rc<dyn IProductService>,
    file_access: Rc<FileAccess>,
    initial_cwd: String,
    /// `_ptyOptions.env`.
    pty_env: RefCell<ProcessEnvironment>,
    /// `_ptyOptions.name`.
    pty_name: String,
    cols: i32,
    rows: i32,
    /// `_ptyOptions.useConpty` e `useConptyDll`.
    use_conpty: bool,
    use_conpty_dll: bool,
    properties: RefCell<ProcessProperties>,
    exit_code: Cell<Option<i32>>,
    close_timeout: RefCell<Option<TimerId>>,
    maximum_shutdown_timeout: RefCell<Option<TimerId>>,
    pty_process: RefCell<Option<Rc<RawPty>>>,
    current_title: RefCell<String>,
    process_startup_complete: RefCell<Option<Promise<()>>>,
    child_process_monitor: RefCell<Option<Rc<ChildProcessMonitor>>>,
    windows_shell_helper: RefCell<Option<Rc<WindowsShellHelper>>>,
    delayed_resizer: RefCell<Option<Rc<DelayedResizer>>>,
    title_interval: RefCell<Option<IntervalHandle>>,
    is_pty_paused: Cell<bool>,
    unacknowledged_char_count: Cell<usize>,
    /// Os bytes de uma sequência UTF-8 cortada entre duas leituras do pty.
    utf8_pending: RefCell<Vec<u8>>,
    throttle_override: RefCell<Option<Rc<dyn Fn() -> Promise<()>>>>,
    on_process_data: Rc<Emitter<ProcessData>>,
    on_process_ready: Rc<Emitter<IProcessReadyEvent>>,
    on_did_change_property: Rc<Emitter<IProcessProperty>>,
    on_process_exit: Rc<Emitter<Option<i32>>>,
}

impl TerminalProcess {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        shell_launch_config: IShellLaunchConfig,
        cwd: &str,
        cols: i32,
        rows: i32,
        env: ProcessEnvironment,
        executable_env: ProcessEnvironment,
        options: ITerminalProcessOptions,
        log_service: LogServiceRef,
        product_service: Rc<dyn IProductService>,
        file_access: Rc<FileAccess>,
    ) -> Rc<Self> {
        let is_windows = Platform::detect().is_windows;
        let host = HostEnvironment;
        let pty_name = if is_windows {
            PathModule::native(&host).basename(shell_launch_config.executable.as_deref().unwrap_or(""), None)
        } else {
            // Using 'xterm-256color' here helps ensure that the majority of Linux distributions will use a
            // color prompt as defined in the default ~/.bashrc file.
            "xterm-256color".to_owned()
        };
        let use_conpty = is_windows && get_windows_build_number_sync() >= 18309;
        let use_conpty_dll = use_conpty && options.windows_use_conpty_dll;
        // Delay resizes to avoid conpty not respecting very early resize calls.
        let needs_delayed_resizer = is_windows && use_conpty && cols == 0 && rows == 0 && shell_launch_config.executable.as_deref().is_some_and(|executable| executable.ends_with("Git\\bin\\bash.exe"));

        let base = Disposable::new();
        let on_process_data: Rc<Emitter<ProcessData>> = Emitter::new();
        let on_process_ready: Rc<Emitter<IProcessReadyEvent>> = Emitter::new();
        let on_did_change_property: Rc<Emitter<IProcessProperty>> = Emitter::new();
        let on_process_exit: Rc<Emitter<Option<i32>>> = Emitter::new();
        base.register(on_process_data.clone());
        base.register(on_process_ready.clone());
        base.register(on_did_change_property.clone());
        base.register(on_process_exit.clone());

        let this = Rc::new_cyclic(|me| Self {
            me: me.clone(),
            base,
            shell_launch_config: RefCell::new(shell_launch_config),
            executable_env,
            options,
            log_service,
            product_service,
            file_access,
            initial_cwd: cwd.to_owned(),
            pty_env: RefCell::new(env),
            pty_name,
            cols,
            rows,
            use_conpty,
            use_conpty_dll,
            properties: RefCell::new(ProcessProperties {
                cwd: cwd.to_owned(),
                initial_cwd: cwd.to_owned(),
                fixed_dimensions: IFixedTerminalDimensions { cols: None, rows: None },
            }),
            exit_code: Cell::new(None),
            close_timeout: RefCell::new(None),
            maximum_shutdown_timeout: RefCell::new(None),
            pty_process: RefCell::new(None),
            current_title: RefCell::new(String::new()),
            process_startup_complete: RefCell::new(None),
            child_process_monitor: RefCell::new(None),
            windows_shell_helper: RefCell::new(None),
            delayed_resizer: RefCell::new(None),
            title_interval: RefCell::new(None),
            is_pty_paused: Cell::new(false),
            unacknowledged_char_count: Cell::new(0),
            utf8_pending: RefCell::new(Vec::new()),
            throttle_override: RefCell::new(None),
            on_process_data,
            on_process_ready,
            on_did_change_property,
            on_process_exit,
        });

        let weak = Rc::downgrade(&this);
        if needs_delayed_resizer {
            let resizer = DelayedResizer::new();
            this.base.register(resizer.clone());
            this.base.register(resizer.on_trigger().listen({
                let weak = weak.clone();
                move |dimensions| {
                    let Some(this) = weak.upgrade() else {
                        return;
                    };
                    if let Some(resizer) = this.delayed_resizer.borrow_mut().take() {
                        resizer.dispose();
                    }
                    if let (Some(cols), Some(rows)) = (dimensions.cols, dimensions.rows) {
                        if cols != 0 && rows != 0 {
                            let _ = this.resize(cols, rows, None, None);
                        }
                    }
                }
            }));
            *this.delayed_resizer.borrow_mut() = Some(resizer);
        }
        if is_windows {
            // WindowsShellHelper is used to fetch the process title and shell type.
            this.base.register(this.on_process_ready.event().listen({
                let weak = weak.clone();
                move |event: &IProcessReadyEvent| {
                    let Some(this) = weak.upgrade() else {
                        return;
                    };
                    let Ok(helper) = WindowsShellHelper::new(event.pid.max(0) as u32, this.file_access.clone()) else {
                        return;
                    };
                    this.base.register(helper.clone());
                    this.base.register(helper.on_shell_type_changed().listen({
                        let weak = weak.clone();
                        move |value| {
                            if let Some(this) = weak.upgrade() {
                                this.fire_property(IProcessProperty::ShellType(*value));
                            }
                        }
                    }));
                    this.base.register(helper.on_shell_name_changed().listen({
                        let weak = weak.clone();
                        move |value: &String| {
                            if let Some(this) = weak.upgrade() {
                                this.fire_property(IProcessProperty::Title(value.clone()));
                            }
                        }
                    }));
                    *this.windows_shell_helper.borrow_mut() = Some(helper);
                }
            }));
        }
        this.base.register(to_disposable({
            let weak = weak.clone();
            move || {
                if let Some(this) = weak.upgrade() {
                    if let Some(interval) = this.title_interval.borrow_mut().take() {
                        clear_interval(&interval);
                    }
                }
            }
        }));
        this.base.register(to_disposable(move || {
            if let Some(this) = weak.upgrade() {
                *this.pty_process.borrow_mut() = None;
                *this.process_startup_complete.borrow_mut() = None;
                for timer in [&this.close_timeout, &this.maximum_shutdown_timeout] {
                    if let Some(id) = timer.borrow_mut().take() {
                        runtime::clear_timeout(id);
                    }
                }
            }
        }));
        this
    }

    fn this(&self) -> Rc<Self> {
        self.me.upgrade().expect("o processo está vivo enquanto um método dele roda")
    }

    /// `shellLaunchConfig`: a configuração, com o `executable` já resolvido depois do `start`.
    pub fn shell_launch_config(&self) -> IShellLaunchConfig {
        self.shell_launch_config.borrow().clone()
    }

    /// `get currentTitle()`: `this._windowsShellHelper?.shellTitle || this._currentTitle`.
    pub fn current_title(&self) -> String {
        let shell_title = self.windows_shell_helper.borrow().as_ref().map(|helper| helper.shell_title()).filter(|title| !title.is_empty());
        shell_title.unwrap_or_else(|| self.current_title.borrow().clone())
    }

    /// `get shellType()`.
    pub fn shell_type(&self) -> TerminalShellType {
        if Platform::detect().is_windows {
            return self.windows_shell_helper.borrow().as_ref().and_then(|helper| helper.shell_type());
        }
        known_shell_type(&self.current_title.borrow())
    }

    /// `get hasChildProcesses()`.
    pub fn has_child_processes(&self) -> bool {
        self.child_process_monitor.borrow().as_ref().is_some_and(|monitor| monitor.has_child_processes())
    }

    /// O `Reflect.get(terminal, '_ptyProcess')` e o `'_titleInterval'` dos testes.
    pub fn observe(&self) -> TerminalProcessObservation {
        TerminalProcessObservation { native_pty_present: self.pty_process.borrow().is_some(), title_polling: self.title_interval.borrow().is_some() }
    }

    /// O `protected override _throttleKillSpawn()` de uma subclasse: troca o gargalo entre `kill` e `spawn`.
    pub fn override_throttle_kill_spawn(&self, hook: Rc<dyn Fn() -> Promise<()>>) {
        *self.throttle_override.borrow_mut() = Some(hook);
    }

    fn fire_property(&self, property: IProcessProperty) {
        self.on_did_change_property.fire(property);
    }

    // region: start

    fn validate_cwd(&self) -> Option<ITerminalLaunchError> {
        match std::fs::metadata(&self.initial_cwd) {
            Ok(metadata) if !metadata.is_dir() => {
                return Some(ITerminalLaunchError {
                    message: localize("launchFail.cwdNotDirectory", "Starting directory (cwd) \"{0}\" is not a directory", &[&self.initial_cwd]),
                    code: None,
                });
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                return Some(ITerminalLaunchError {
                    message: localize("launchFail.cwdDoesNotExist", "Starting directory (cwd) \"{0}\" does not exist", &[&self.initial_cwd]),
                    code: None,
                });
            }
            _ => {}
        }
        self.fire_property(IProcessProperty::InitialCwd(self.initial_cwd.clone()));
        None
    }

    async fn validate_executable(&self) -> Result<Option<ITerminalLaunchError>, Error> {
        let slc = self.shell_launch_config.borrow().clone();
        let Some(executable) = slc.executable.clone().filter(|executable| !executable.is_empty()) else {
            return Err(Error::new("IShellLaunchConfig.executable not set"));
        };

        let cwd = slc.cwd.as_ref().map(|cwd| match cwd {
            ShellCwd::Uri(uri) => uri.path().to_string_lossy(),
            ShellCwd::Path(path) => path.clone(),
        });
        let host = HostEnvironment;
        let delimiter = PathModule::native(&host).delimiter();
        let env_paths: Option<Vec<String>> = slc
            .env
            .as_ref()
            .and_then(|env| env.iter().find(|(key, _)| key == "PATH"))
            .and_then(|(_, value)| match value {
                TerminalEnvironmentValue::Value(path) if !path.is_empty() => Some(path.split(delimiter).map(str::to_owned).collect()),
                _ => None,
            });
        let found = find_executable(&executable, cwd.as_deref(), env_paths.as_deref(), Some(&self.executable_env), None).await;
        let Some(found) = found else {
            return Ok(Some(ITerminalLaunchError {
                message: localize("launchFail.executableDoesNotExist", "Path to shell executable \"{0}\" does not exist", &[&executable]),
                code: None,
            }));
        };

        match std::fs::metadata(&found) {
            Ok(metadata) => {
                if !metadata.is_file() && !metadata.file_type().is_symlink() {
                    return Ok(Some(ITerminalLaunchError {
                        message: localize("launchFail.executableIsNotFileOrSymlink", "Path to shell executable \"{0}\" is not a file or a symlink", &[&executable]),
                        code: None,
                    }));
                }
                // Set the executable explicitly here so that node-pty doesn't need to search the $PATH too.
                self.shell_launch_config.borrow_mut().executable = Some(found);
            }
            // EACCES: swallow.
            Err(error) if error.kind() == std::io::ErrorKind::PermissionDenied => {}
            Err(error) => return Err(Error::new(error.to_string())),
        }
        Ok(None)
    }

    async fn run_start(self: Rc<Self>) -> Result<Option<ITerminalLaunchOutcome>, Error> {
        let cwd_error = self.validate_cwd();
        let executable_error = self.validate_executable().await?;
        if let Some(error) = cwd_error.or(executable_error) {
            return Ok(Some(ITerminalLaunchOutcome::Error(error)));
        }

        let slc = self.shell_launch_config.borrow().clone();
        let env_for_injection: Vec<(String, TerminalEnvironmentValue)> =
            self.pty_env.borrow().iter().map(|(key, value)| (key.clone(), TerminalEnvironmentValue::Value(value.clone()))).collect();
        let injection = get_shell_integration_injection(
            &slc,
            &self.options,
            Some(&env_for_injection),
            &*self.log_service,
            &*self.product_service,
            &self.file_access,
            false,
        )
        .await?;

        let mut injection_config = None;
        match injection {
            ShellIntegrationInjection::Injection(injection) => {
                self.fire_property(IProcessProperty::UsedShellIntegrationInjection(Some(true)));
                if let Some(env_mixin) = &injection.env_mixin {
                    let mut env = self.pty_env.borrow_mut();
                    for (key, value) in env_mixin {
                        env.insert(key.clone(), value.clone());
                    }
                }
                if let Some(files_to_copy) = &injection.files_to_copy {
                    for file in files_to_copy {
                        // Swallow error, this should only happen when multiple users are on the same machine. Since the
                        // shell integration scripts rarely change, plus the other user should be using the same version
                        // of the server in this case, assume the script is fine if copy fails and swallow the error.
                        if let Some(parent) = std::path::Path::new(&file.dest).parent() {
                            let _ = std::fs::create_dir_all(parent);
                        }
                        let _ = std::fs::copy(&file.source, &file.dest);
                    }
                }
                injection_config = Some(injection);
            }
            ShellIntegrationInjection::Failure(failure) => {
                self.fire_property(IProcessProperty::FailedShellIntegrationActivation(Some(true)));
                self.fire_property(IProcessProperty::ShellIntegrationInjectionFailureReason(Some(failure.reason)));
                // Even if shell integration injection failed, still set the nonce if one was provided. This allows
                // extensions to use shell integration with custom shells.
                if !self.options.shell_integration.nonce.is_empty() {
                    self.pty_env.borrow_mut().insert("VSCODE_NONCE".to_owned(), self.options.shell_integration.nonce.clone());
                }
            }
        }

        let new_args = injection_config.as_ref().and_then(|injection| injection.new_args.clone());
        match self.setup_pty_process(new_args.clone()).await {
            Ok(()) => Ok(new_args.map(|injected_args| ITerminalLaunchOutcome::Result(ITerminalLaunchResult { injected_args }))),
            Err(error) => {
                self.log_service.trace_args("node-pty.node-pty.IPty#spawn native exception", &[LogArg::from(&error)]);
                let error_message = error.to_string();
                if error_message.contains("Cannot launch conpty") {
                    return Ok(Some(ITerminalLaunchOutcome::Error(ITerminalLaunchError {
                        message: localize(
                            "conptyLaunchFailed",
                            "A native exception occurred during launch (Cannot launch conpty). Winpty has been removed, see {0} for more details. You can also try enabling the `{1}` setting.",
                            &["https://code.visualstudio.com/updates/v1_109#_removal-of-winpty-support", "terminal.integrated.windowsUseConptyDll"],
                        ),
                        code: None,
                    })));
                }
                Ok(Some(ITerminalLaunchOutcome::Error(ITerminalLaunchError { message: format!("A native exception occurred during launch ({error_message})"), code: None })))
            }
        }
    }

    async fn setup_pty_process(self: &Rc<Self>, injection_new_args: Option<Vec<String>>) -> Result<(), Error> {
        let slc = self.shell_launch_config.borrow().clone();
        let args: Vec<String> = injection_new_args.unwrap_or_else(|| match &slc.args {
            Some(je_base::arrays::OneOrMany::Many(args)) => args.clone(),
            Some(je_base::arrays::OneOrMany::One(args)) => vec![args.clone()],
            None => Vec::new(),
        });
        self.throttle_kill_spawn().await?;
        // Validation or spawn throttling can finish after the terminal was disposed.
        if self.base.store().is_disposed() {
            return Ok(());
        }
        let executable = slc.executable.clone().unwrap_or_default();
        let mut env = self.pty_env.borrow().clone();
        let sanitized_env = {
            let optional: OptionalProcessEnvironment = env.iter().map(|(key, value)| (key.clone(), Some(value.clone()))).collect();
            sanitize_env_for_logging(Some(&optional))
        };
        self.log_service.trace_args(
            "node-pty.IPty#spawn",
            &[
                LogArg::from(executable.as_str()),
                LogArg::from(args.join(" ").as_str()),
                LogArg::from(format!("{{ name: {}, cwd: {}, env: {:?} }}", self.pty_name, self.initial_cwd, sanitized_env).as_str()),
            ],
        );
        env.insert("TERM".to_owned(), self.pty_name.clone());
        let spec = PtySpawn {
            program: executable,
            args,
            cwd: Some(PathBuf::from(&self.initial_cwd)),
            env: env.into_iter().collect::<HashMap<_, _>>(),
            cols: self.cols.clamp(0, i32::from(u16::MAX)) as u16,
            rows: self.rows.clamp(0, i32::from(u16::MAX)) as u16,
        };
        let pty = Rc::new(RawPty::spawn(&spec).map_err(|error| Error::new(error.to_string()))?);
        *self.pty_process.borrow_mut() = Some(pty.clone());

        let monitor = ChildProcessMonitor::with_file_access(pty.pid(), self.log_service.clone(), self.file_access.clone());
        self.base.register(monitor.clone());
        *self.child_process_monitor.borrow_mut() = Some(monitor.clone());
        let weak = self.me.clone();
        self.base.register(monitor.on_did_change_has_child_processes().listen(move |value: &bool| {
            if let Some(this) = weak.upgrade() {
                this.fire_property(IProcessProperty::HasChildProcesses(*value));
            }
        }));

        let startup: Rc<DeferredPromise<()>> = Rc::new(DeferredPromise::new());
        *self.process_startup_complete.borrow_mut() = Some(startup.promise.clone());
        self.base.register(self.on_process_ready.event().listen(move |_| drop(startup.complete(()))));

        self.schedule_poll(pty.clone());
        self.send_process_id(pty.pid());
        self.setup_title_polling(&pty);
        Ok(())
    }

    // endregion

    // region: o laço do pty (o `onData` e o `onExit` do node-pty)

    fn schedule_poll(&self, pty: Rc<RawPty>) {
        let weak = self.me.clone();
        runtime::set_timeout(move || Self::poll(&weak, &pty), PTY_POLL_INTERVAL_MS);
    }

    fn poll(weak: &Weak<Self>, pty: &Rc<RawPty>) {
        let Some(this) = weak.upgrade() else {
            return;
        };
        // Um pty que deixou de ser o atual (descarte) não é mais consultado.
        if !this.pty_process.borrow().as_ref().is_some_and(|current| Rc::ptr_eq(current, pty)) {
            return;
        }
        let mut exited = false;
        for event in pty.drain_events() {
            match event {
                PtyEvent::Data(bytes) => this.handle_data(pty, &bytes),
                PtyEvent::Exit(code) => {
                    exited = true;
                    this.exit_code.set(Some(code.unwrap_or(0)));
                    this.queue_process_exit();
                }
            }
        }
        if !exited {
            this.schedule_poll(pty.clone());
        }
    }

    fn handle_data(&self, pty: &RawPty, bytes: &[u8]) {
        let data = decode_utf8_chunk(&mut self.utf8_pending.borrow_mut(), bytes);
        if data.is_empty() {
            return;
        }
        // Handle flow control.
        let unacknowledged = self.unacknowledged_char_count.get() + data.encode_utf16().count();
        self.unacknowledged_char_count.set(unacknowledged);
        if !self.is_pty_paused.get() && unacknowledged > FlowControlConstants::HIGH_WATERMARK_CHARS {
            self.log_service.trace(&format!("Flow control: Pause ({unacknowledged} > {})", FlowControlConstants::HIGH_WATERMARK_CHARS));
            self.is_pty_paused.set(true);
            pty.pause();
        }

        // Refire the data event.
        self.log_service.trace_args("node-pty.IPty#onData", &[LogArg::from(data.as_str())]);
        self.on_process_data.fire(ProcessData::Text(data));
        if self.close_timeout.borrow().is_some() {
            self.queue_process_exit();
        }
        let helper = self.windows_shell_helper.borrow().clone();
        if let Some(helper) = helper {
            helper.check_shell();
        }
        let monitor = self.child_process_monitor.borrow().clone();
        if let Some(monitor) = monitor {
            monitor.handle_output();
        }
    }

    fn setup_title_polling(&self, pty: &Rc<RawPty>) {
        // Send initial timeout async to give event listeners a chance to init.
        let weak = self.me.clone();
        runtime::set_timeout(
            {
                let (weak, pty) = (weak.clone(), pty.clone());
                move || {
                    if let Some(this) = weak.upgrade() {
                        this.send_process_title(&pty);
                    }
                }
            },
            0,
        );
        // Setup polling for non-Windows, for Windows `process` doesn't change.
        if !Platform::detect().is_windows {
            let pty = pty.clone();
            *self.title_interval.borrow_mut() = Some(set_interval(
                move || {
                    if let Some(this) = weak.upgrade() {
                        let changed = pty.process().as_deref() != Some(this.current_title.borrow().as_str());
                        if changed {
                            this.send_process_title(&pty);
                        }
                    }
                },
                TITLE_POLL_INTERVAL_MS,
            ));
        }
    }

    fn send_process_id(&self, pid: u32) {
        self.on_process_ready.fire(IProcessReadyEvent { pid: pid as i32, cwd: self.initial_cwd.clone(), windows_pty: Self::windows_pty() });
    }

    fn send_process_title(&self, pty: &RawPty) {
        if self.base.store().is_disposed() {
            return;
        }
        // HACK: The node-pty API can return undefined somehow https://github.com/microsoft/vscode/issues/222323
        let title = pty.process().unwrap_or_default();
        *self.current_title.borrow_mut() = title.clone();
        self.fire_property(IProcessProperty::Title(title.clone()));
        // If fig is installed it may change the title of the process.
        let mut sanitized_title = self.current_title().strip_suffix(" (figterm)").map(str::to_owned).unwrap_or_else(|| self.current_title());
        // Ensure any prefixed path is removed so that the executable name since we use this to detect the shell type.
        if !Platform::detect().is_windows {
            let host = HostEnvironment;
            sanitized_title = PathModule::native(&host).basename(&sanitized_title, None);
        }

        let lower = sanitized_title.to_lowercase();
        let shell_type = if lower.starts_with("python") {
            Some(ShellType::from(GeneralShellType::Python))
        } else if lower.starts_with("julia") {
            Some(ShellType::from(GeneralShellType::Julia))
        } else {
            known_shell_type(&sanitized_title)
        };
        self.fire_property(IProcessProperty::ShellType(shell_type));
    }

    // endregion

    // region: encerramento

    /// Allow any trailing data events to be sent before the exit event is sent.
    /// See https://github.com/microsoft/node-pty/issues/72
    fn queue_process_exit(&self) {
        if self.log_service.get_level() == LogLevel::Trace {
            self.log_service.trace("TerminalProcess#_queueProcessExit");
        }
        if let Some(previous) = self.close_timeout.borrow_mut().take() {
            runtime::clear_timeout(previous);
        }
        let weak = self.me.clone();
        let id = runtime::set_timeout(
            move || {
                if let Some(this) = weak.upgrade() {
                    *this.close_timeout.borrow_mut() = None;
                    drop(this.kill());
                }
            },
            DATA_FLUSH_TIMEOUT,
        );
        *self.close_timeout.borrow_mut() = Some(id);
    }

    fn kill(&self) -> Promise<()> {
        let this = self.this();
        async_fn(async move {
            // Wait to kill to process until the start up code has run. This prevents us from firing a process exit before a
            // process start.
            let startup = this.process_startup_complete.borrow().clone();
            if let Some(startup) = startup {
                startup.await?;
            }
            if this.base.store().is_disposed() {
                return Ok(());
            }
            // Attempt to kill the pty, it may have already been killed at this point but we want to make sure.
            let pty = this.pty_process.borrow().clone();
            if let Some(pty) = pty {
                // Swallow, the pty has already been killed.
                if this.throttle_kill_spawn().await.is_ok() {
                    this.log_service.trace("node-pty.IPty#kill");
                    let _ = pty.kill(None);
                }
            }
            this.on_process_exit.fire(Some(this.exit_code.get().unwrap_or(0)));
            this.dispose();
            Ok(())
        })
    }

    /// `protected async _throttleKillSpawn()`.
    fn throttle_kill_spawn(&self) -> Promise<()> {
        let hook = self.throttle_override.borrow().clone();
        if let Some(hook) = hook {
            return hook();
        }
        // Only throttle on Windows/conpty. Don't throttle when using conpty.dll as it seems to have been fixed in later
        // versions.
        if !Platform::detect().is_windows || !self.use_conpty || self.use_conpty_dll {
            return Promise::resolved(());
        }
        let log_service = self.log_service.clone();
        async_fn(async move {
            // Use a loop to ensure multiple calls in a single interval space out.
            loop {
                let elapsed = runtime::date_now().saturating_sub(LAST_KILL_OR_START.with(Cell::get));
                if elapsed >= KILL_SPAWN_THROTTLE_INTERVAL {
                    break;
                }
                log_service.trace("Throttling kill/spawn call");
                timeout(KILL_SPAWN_THROTTLE_INTERVAL - elapsed + KILL_SPAWN_SPACING_DURATION).await?;
            }
            LAST_KILL_OR_START.with(|last| last.set(runtime::date_now()));
            Ok(())
        })
    }

    // endregion

    fn windows_pty() -> Option<IProcessReadyWindowsPty> {
        Platform::detect().is_windows.then(|| IProcessReadyWindowsPty { backend: WindowsPtyBackend::Conpty, build_number: get_windows_build_number_sync() as i32 })
    }

    fn write_to_pty(&self, bytes: &[u8]) {
        let pty = self.pty_process.borrow().clone();
        if let Some(pty) = pty {
            // O `node-pty` entrega o erro de escrita no evento `error`, que o `TerminalProcess` não trata.
            let _ = pty.write(bytes);
        }
        let monitor = self.child_process_monitor.borrow().clone();
        if let Some(monitor) = monitor {
            monitor.handle_input();
        }
    }

    /// `getCwd()`: no Linux o destino de `/proc/<pid>/cwd`; no macOS a coluna `cwd` do `lsof`.
    fn read_cwd(&self) -> Option<String> {
        let pid = self.pty_process.borrow().as_ref()?.pid();
        self.log_service.trace("node-pty.IPty#pid");
        if cfg!(target_os = "macos") {
            // From Big Sur (darwin v20) there is a spawn blocking thread issue on Electron, this is fixed in VS Code's
            // internal Electron. https://github.com/Microsoft/vscode/issues/105446
            let output = std::process::Command::new("lsof").args(["-OPln", "-p", &pid.to_string()]).env("LANG", "en_US.UTF-8").output();
            return match output {
                Ok(output) if output.status.success() => {
                    let stdout = String::from_utf8_lossy(&output.stdout);
                    let line = stdout.lines().find(|line| line.contains("cwd"))?;
                    line.find('/').map(|start| line[start..].to_owned())
                }
                other => {
                    self.log_service.error(&format!("lsof did not run successfully, it may not be on the $PATH? {:?}", other.err()));
                    None
                }
            };
        }
        if cfg!(target_os = "linux") {
            return std::fs::read_link(format!("/proc/{pid}/cwd")).ok().map(|path| path.to_string_lossy().into_owned());
        }
        None
    }
}

/// Acrescenta `bytes` aos bytes pendentes e devolve o texto UTF-8 que já está completo; o início de uma sequência cortada
/// fica em `pending` para a próxima leitura, e os bytes inválidos viram U+FFFD (o `StringDecoder` do Node).
fn decode_utf8_chunk(pending: &mut Vec<u8>, bytes: &[u8]) -> String {
    pending.extend_from_slice(bytes);
    let mut text = String::new();
    let mut rest: &[u8] = pending.as_slice();
    loop {
        match std::str::from_utf8(rest) {
            Ok(valid) => {
                text.push_str(valid);
                rest = &[];
                break;
            }
            Err(error) => {
                let (valid, after) = rest.split_at(error.valid_up_to());
                text.push_str(&String::from_utf8_lossy(valid));
                match error.error_len() {
                    Some(invalid) => {
                        text.push('\u{FFFD}');
                        rest = &after[invalid..];
                    }
                    None => {
                        rest = after;
                        break;
                    }
                }
            }
        }
    }
    let leftover = rest.to_vec();
    *pending = leftover;
    text
}

impl IDisposable for TerminalProcess {
    fn dispose(&self) {
        self.base.dispose();
    }
}

impl ITerminalChildProcess for TerminalProcess {
    fn id(&self) -> u32 {
        0
    }

    fn should_persist(&self) -> bool {
        false
    }

    fn on_process_data(&self) -> Event<ProcessData> {
        self.on_process_data.event()
    }

    fn on_process_ready(&self) -> Event<IProcessReadyEvent> {
        self.on_process_ready.event()
    }

    fn on_did_change_property(&self) -> Event<IProcessProperty> {
        self.on_did_change_property.event()
    }

    fn on_process_exit(&self) -> Event<Option<i32>> {
        self.on_process_exit.event()
    }

    fn start(&self) -> Promise<Option<ITerminalLaunchOutcome>> {
        let this = self.this();
        async_fn(async move { this.run_start().await })
    }

    fn shutdown(&self, immediate: bool) {
        if self.log_service.get_level() == LogLevel::Trace {
            self.log_service.trace("TerminalProcess#shutdown");
        }
        // Don't force immediate disposal of the terminal processes on Windows as an additional mitigation for
        // https://github.com/microsoft/vscode/issues/71966 which causes the pty host to become unresponsive, disconnecting
        // all terminals across all windows.
        if immediate && !Platform::detect().is_windows {
            drop(self.kill());
        } else if self.close_timeout.borrow().is_none() && !self.base.store().is_disposed() {
            self.queue_process_exit();
            // Allow a maximum amount of time for the process to exit, otherwise force kill it.
            let weak = self.me.clone();
            let id = runtime::set_timeout(
                move || {
                    if let Some(this) = weak.upgrade() {
                        *this.maximum_shutdown_timeout.borrow_mut() = None;
                        if this.close_timeout.borrow().is_some() && !this.base.store().is_disposed() {
                            *this.close_timeout.borrow_mut() = None;
                            drop(this.kill());
                        }
                    }
                },
                MAXIMUM_SHUTDOWN_TIME,
            );
            *self.maximum_shutdown_timeout.borrow_mut() = Some(id);
        }
    }

    fn input(&self, data: &str) {
        self.log_service.trace_args("node-pty.IPty#write", &[LogArg::from(data), LogArg::Value(je_base::dynamic::Dynamic::Bool(false))]);
        self.write_to_pty(data.as_bytes());
    }

    fn send_signal(&self, signal: &str) {
        if self.base.store().is_disposed() {
            return;
        }
        let pty = self.pty_process.borrow().clone();
        if let Some(pty) = pty {
            let _ = pty.kill(Some(signal));
        }
    }

    fn process_binary(&self, data: &str) -> Promise<()> {
        self.log_service.trace_args("node-pty.IPty#write", &[LogArg::from(data), LogArg::Value(je_base::dynamic::Dynamic::Bool(true))]);
        // `Buffer.from(data, 'binary')`: cada unidade de código (até 0xFF) é um byte.
        let bytes: Vec<u8> = data.chars().map(|character| character as u32 as u8).collect();
        self.write_to_pty(&bytes);
        Promise::resolved(())
    }

    fn resize(&self, cols: i32, rows: i32, pixel_width: Option<i32>, pixel_height: Option<i32>) -> Result<(), Error> {
        if self.base.store().is_disposed() {
            return Ok(());
        }
        // Ensure that cols and rows are always >= 1, this prevents a native exception in winpty.
        let pty = self.pty_process.borrow().clone();
        let Some(pty) = pty else {
            return Ok(());
        };
        // Delay resize if needed.
        let resizer = self.delayed_resizer.borrow().clone();
        if let Some(resizer) = resizer {
            resizer.set_dimensions(cols.max(1), rows.max(1));
            return Ok(());
        }
        let cols = cols.clamp(1, i32::from(u16::MAX)) as u16;
        let rows = rows.clamp(1, i32::from(u16::MAX)) as u16;
        self.log_service.trace(&format!("node-pty.IPty#resize {cols} {rows}"));
        let pixel_size = match (pixel_width, pixel_height) {
            (Some(width), Some(height)) => Some((width.clamp(0, i32::from(u16::MAX)) as u16, height.clamp(0, i32::from(u16::MAX)) as u16)),
            _ => None,
        };
        if let Err(error) = pty.resize(cols, rows, pixel_size) {
            // Swallow error if the pty has already exited.
            let message = error.to_string();
            self.log_service.trace(&format!("node-pty.IPty#resize exception {message}"));
            if self.exit_code.get().is_some() && message != "ioctl(2) failed, EBADF" && message != "Cannot resize a pty that has already exited" {
                return Err(Error::new(message));
            }
        }
        Ok(())
    }

    fn clear_buffer(&self) -> Promise<()> {
        // `ptyProcess?.clear()` só tem efeito no ConPTY.
        Promise::resolved(())
    }

    fn acknowledge_data_event(&self, char_count: usize) {
        // Prevent lower than 0 to heal from errors.
        let unacknowledged = self.unacknowledged_char_count.get().saturating_sub(char_count);
        self.unacknowledged_char_count.set(unacknowledged);
        self.log_service.trace(&format!("Flow control: Ack {char_count} chars (unacknowledged: {unacknowledged})"));
        if self.is_pty_paused.get() && unacknowledged < FlowControlConstants::LOW_WATERMARK_CHARS {
            self.log_service.trace(&format!("Flow control: Resume ({unacknowledged} < {})", FlowControlConstants::LOW_WATERMARK_CHARS));
            let pty = self.pty_process.borrow().clone();
            if let Some(pty) = pty {
                pty.resume();
            }
            self.is_pty_paused.set(false);
        }
    }

    fn set_unicode_version(&self, _version: UnicodeVersion) -> Promise<()> {
        // No-op.
        Promise::resolved(())
    }

    fn get_initial_cwd(&self) -> Promise<String> {
        Promise::resolved(self.initial_cwd.clone())
    }

    fn get_cwd(&self) -> Promise<String> {
        Promise::resolved(self.read_cwd().unwrap_or_else(|| self.initial_cwd.clone()))
    }

    fn refresh_property(&self, property: ProcessPropertyType) -> Promise<IProcessProperty> {
        let value = match property {
            ProcessPropertyType::Cwd => {
                let new_cwd = self.read_cwd().unwrap_or_else(|| self.initial_cwd.clone());
                let changed = new_cwd != self.properties.borrow().cwd;
                if changed {
                    self.properties.borrow_mut().cwd = new_cwd.clone();
                    self.fire_property(IProcessProperty::Cwd(new_cwd.clone()));
                }
                IProcessProperty::Cwd(new_cwd)
            }
            ProcessPropertyType::InitialCwd => {
                let initial_cwd = self.initial_cwd.clone();
                let changed = initial_cwd != self.properties.borrow().initial_cwd;
                if changed {
                    self.properties.borrow_mut().initial_cwd = initial_cwd.clone();
                    self.fire_property(IProcessProperty::InitialCwd(initial_cwd.clone()));
                }
                IProcessProperty::InitialCwd(initial_cwd)
            }
            ProcessPropertyType::Title => IProcessProperty::Title(self.current_title()),
            _ => IProcessProperty::ShellType(self.shell_type()),
        };
        Promise::resolved(value)
    }

    fn update_property(&self, property: IProcessProperty) -> Promise<()> {
        if let IProcessProperty::FixedDimensions(dimensions) = property {
            self.properties.borrow_mut().fixed_dimensions = dimensions;
        }
        Promise::resolved(())
    }
}
