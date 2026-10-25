//! Porte de `src/vs/platform/remoteTunnel/node/codeTunnelCliProcess.ts`: executa a CLI do túnel (`code tunnel`) de
//! uma instalação do VS Code, linha a linha, com a vida do processo de verdade.
//!
//! Divergências:
//! - O `child_process.spawn` injetável (`CodeTunnelSpawn`) devolve um `ChildProcess` com `EventEmitter` e `Readable`s.
//!   Aqui o `spawn` recebe os ouvintes já prontos ([`TunnelChildHandlers`]: dados e fim de `stdout` e de `stderr`,
//!   `exit` e `error`) e devolve o processo ([`TunnelChildProcess`]: `pid` e `kill`). O `spawn` de verdade
//!   ([`spawn_process`]) lê os dois pipes em threads e entrega tudo no laço de eventos do `je-base`, por um
//!   temporizador de 10 ms (o mesmo desenho do `ProcessExecFile`), e o `exit` só chega depois de os dados já lidos terem
//!   sido entregues (o Node pode entregar o `exit` antes do fim dos pipes e perder a cauda da saída). Falha ao iniciar o
//!   programa chega como evento `error` por microtarefa, como no Node (`spawn <programa> ENOENT`).
//! - `NodeJS.Platform` é [`TunnelPlatform`]; `tunnelApplicationName` ausente aparece como `undefined` no nome do
//!   binário, como a interpolação do original.
//! - O sinal do `kill` é [`KillSignal`] (`SIGTERM` padrão, `SIGKILL`). Sem `unsafe` não há `kill(2)`: no Unix o `SIGTERM`
//!   sai pelo utilitário `kill -TERM <pid>`; fora do Unix, `kill` encerra o processo (o Node faz o mesmo no Windows).
//! - O `StreamSplitter` é o do `je-base`, e a saída do `stdout` e do `stderr` passa por um cada, como o `pipe` do original.
//! - `process.env` é o ambiente do processo no momento da chamada; o `env` extra o sobrescreve chave a chave.

use std::cell::{Cell, RefCell};
use std::collections::{BTreeMap, VecDeque};
use std::io::Read;
use std::process::{Child, Command, Stdio};
use std::rc::Rc;
use std::sync::{Arc, Mutex};

use je_base::async_::create_cancelable_promise;
use je_base::errors::Error;
use je_base::lifecycle::{DisposableRef, IDisposable};
use je_base::node::node_streams::StreamSplitter;
use je_base::path::{PathModule, ProcessEnvironment};
use je_base::promise::{CancelablePromise, Promise, Resolver};
use je_base::runtime;

use crate::environment::user_data_path::home_dir;
use crate::git::process_exec_file::spawn_failure;

/// Callback for a single line of CLI output: a mensagem e se veio do `stderr`.
pub type CodeTunnelCliOutput = Rc<dyn Fn(&str, bool)>;

/// Receives the service's own diagnostics, separate from CLI output.
pub type CodeTunnelLog = Rc<dyn Fn(&str)>;

/// A running tunnel CLI invocation and its actual-process lifetime.
#[derive(Clone)]
pub struct CodeTunnelCliRun {
    /// Resolves with the process exit code; rejects if the process could not be spawned.
    pub result: CancelablePromise<i32>,
    stop: Rc<dyn Fn() -> Promise<()>>,
}

impl CodeTunnelCliRun {
    /// Kills the process if needed and resolves after its `exit` or `error` event.
    pub fn stop(&self) -> Promise<()> {
        (self.stop)()
    }
}

/// How long a stopped CLI process gets to exit before it is force-killed.
const STOP_GRACE_PERIOD_MS: u64 = 5_000;

/// De quanto em quanto tempo o laço entrega a saída e confere se o filho terminou.
const POLL_INTERVAL_MS: u64 = 10;

/// Quantas conferências o `exit` espera pelo fim dos pipes (um neto pode segurá-los abertos para sempre).
const EXIT_STREAM_GRACE_POLLS: u8 = 5;

/// O sinal do `kill`: `SIGTERM` (o padrão do Node) ou `SIGKILL`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum KillSignal {
    Terminate,
    Kill,
}

/// As opções do `spawn` que a CLI usa: `cwd` e `env` (o `stdio` é sempre `['ignore', 'pipe', 'pipe']`).
#[derive(Clone, Debug, Default)]
pub struct SpawnOptions {
    pub cwd: Option<String>,
    pub env: BTreeMap<String, String>,
}

/// O que a CLI usa de um `ChildProcess`.
pub trait TunnelChildProcess {
    /// `pid`; ausente quando o processo nunca iniciou.
    fn pid(&self) -> Option<u32>;
    /// `kill(signal)`: se o sinal foi entregue.
    fn kill(&self, signal: KillSignal) -> bool;
}

/// Os ouvintes que a CLI registra no processo: os eventos `data`/`end` de `stdout` e `stderr`, e `exit` e `error`.
pub struct TunnelChildHandlers {
    pub on_stdout: Box<dyn Fn(&[u8])>,
    pub on_stdout_end: Box<dyn Fn()>,
    pub on_stderr: Box<dyn Fn(&[u8])>,
    pub on_stderr_end: Box<dyn Fn()>,
    /// `exit`: o código de saída, ausente (`null`) quando o processo morreu por sinal.
    pub on_exit: Box<dyn Fn(Option<i32>)>,
    pub on_error: Box<dyn Fn(Error)>,
}

/// Injectable process-spawning implementation for the tunnel CLI. Falha de síncrona do `spawn` é o `Err`.
pub type CodeTunnelSpawn = Rc<dyn Fn(&str, &[String], &SpawnOptions, TunnelChildHandlers) -> Result<Rc<dyn TunnelChildProcess>, Error>>;

/// O `NodeJS.Platform` que a instalação usa.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TunnelPlatform {
    Darwin,
    Win32,
    Linux,
}

impl TunnelPlatform {
    /// A plataforma deste processo (`isWindows ? 'win32' : isMacintosh ? 'darwin' : 'linux'`).
    pub fn current() -> Self {
        if cfg!(windows) {
            TunnelPlatform::Win32
        } else if cfg!(target_os = "macos") {
            TunnelPlatform::Darwin
        } else {
            TunnelPlatform::Linux
        }
    }
}

/// Resolves the absolute path of a tunnel CLI binary for an installation.
pub fn resolve_tunnel_command_location(app_root: &str, platform: TunnelPlatform, tunnel_application_name: Option<&str>, win32_versioned_update: bool) -> String {
    let path = PathModule::native(&ProcessEnvironment);
    let bin_parent_location = match platform {
        TunnelPlatform::Darwin => app_root.to_owned(),
        TunnelPlatform::Win32 if win32_versioned_update => path.dirname(&path.dirname(&path.dirname(app_root))),
        TunnelPlatform::Win32 | TunnelPlatform::Linux => path.dirname(&path.dirname(app_root)),
    };
    let file_name = format!("{}{}", tunnel_application_name.unwrap_or("undefined"), if platform == TunnelPlatform::Win32 { ".exe" } else { "" });
    path.join(&[bin_parent_location.as_str(), "bin", file_name.as_str()])
}

/// How to reach and run a VS Code installation's bundled CLI.
pub struct CodeTunnelCliOptions {
    pub app_root: String,
    pub is_built: bool,
    pub tunnel_application_name: Option<String>,
    pub win32_versioned_update: bool,
    /// Injectable for tests; defaults to the real process spawn ([`spawn_process`]).
    pub spawn: Option<CodeTunnelSpawn>,
    /// Receives the service's own diagnostics, separate from CLI output.
    pub on_log: Option<CodeTunnelLog>,
}

/// Runs the code-tunnel CLI for a VS Code installation.
pub struct CodeTunnelCli {
    app_root: String,
    is_built: bool,
    tunnel_application_name: Option<String>,
    win32_versioned_update: bool,
    command_location: RefCell<Option<String>>,
    spawn: CodeTunnelSpawn,
    on_log: CodeTunnelLog,
}

impl CodeTunnelCli {
    pub fn new(options: CodeTunnelCliOptions) -> Self {
        Self {
            app_root: options.app_root,
            is_built: options.is_built,
            tunnel_application_name: options.tunnel_application_name,
            win32_versioned_update: options.win32_versioned_update,
            command_location: RefCell::new(None),
            spawn: options.spawn.unwrap_or_else(|| Rc::new(spawn_process) as CodeTunnelSpawn),
            on_log: options.on_log.unwrap_or_else(|| Rc::new(|_: &str| {}) as CodeTunnelLog),
        }
    }

    /// Absolute path of the `code-tunnel` binary for this installation.
    pub fn command_location(&self) -> String {
        self.command_location
            .borrow_mut()
            .get_or_insert_with(|| {
                resolve_tunnel_command_location(&self.app_root, TunnelPlatform::current(), self.tunnel_application_name.as_deref(), self.win32_versioned_update)
            })
            .clone()
    }

    /// Runs the code CLI with the specified complete command arguments.
    pub fn run(&self, log_label: &str, args: &[String], on_output: CodeTunnelCliOutput, env: Option<BTreeMap<String, String>>) -> CodeTunnelCliRun {
        let tunnel_process: Rc<RefCell<Option<Rc<dyn TunnelChildProcess>>>> = Rc::default();
        let did_stop = Rc::new(Cell::new(false));
        let (exited, resolve_exit) = Promise::<()>::new();
        let log_label = log_label.to_owned();

        let stop: Rc<dyn Fn() -> Promise<()>> = {
            let tunnel_process = tunnel_process.clone();
            let on_log = self.on_log.clone();
            let log_label = log_label.clone();
            Rc::new(move || {
                let child = tunnel_process.borrow().clone();
                if let Some(child) = child
                    && !did_stop.get()
                {
                    did_stop.set(true);
                    on_log(&format!("{log_label} terminating({})", describe_pid(child.as_ref())));
                    child.kill(KillSignal::Terminate);
                    // Callers serialize on this promise, so a CLI that ignores the
                    // termination signal would wedge them forever. Escalate instead
                    // of waiting indefinitely: SIGKILL cannot be caught, so `exit`
                    // always follows and `exited` always settles.
                    let force_kill = runtime::set_timeout(
                        {
                            let on_log = on_log.clone();
                            let log_label = log_label.clone();
                            move || {
                                on_log(&format!("{log_label} did not exit within {STOP_GRACE_PERIOD_MS}ms, force killing({})", describe_pid(child.as_ref())));
                                child.kill(KillSignal::Kill);
                            }
                        },
                        STOP_GRACE_PERIOD_MS,
                    );
                    exited.finally(move || runtime::clear_timeout(force_kill));
                }
                exited.clone()
            })
        };

        let result = create_cancelable_promise(|token| {
            Promise::with_executor(|resolver: Resolver<i32>| {
                if token.is_cancellation_requested() {
                    resolver.resolve(-1);
                }

                let cancellation_listener: DisposableRef = token.on_cancellation_requested().listen({
                    let stop = stop.clone();
                    move |_| {
                        drop(stop());
                    }
                });

                let stdout_splitter = Rc::new(StreamSplitter::new("\n"));
                let stderr_splitter = Rc::new(StreamSplitter::new("\n"));
                for (splitter, is_error) in [(&stdout_splitter, false), (&stderr_splitter, true)] {
                    let on_output = on_output.clone();
                    let tunnel_process = tunnel_process.clone();
                    splitter.pipe(move |data: &[u8]| {
                        let active = tunnel_process.borrow().is_some();
                        if active {
                            on_output(&String::from_utf8_lossy(data), is_error);
                        }
                    });
                }

                let handlers = TunnelChildHandlers {
                    on_stdout: Box::new({
                        let splitter = stdout_splitter.clone();
                        move |data: &[u8]| splitter.write(data)
                    }),
                    on_stdout_end: Box::new(move || stdout_splitter.end(|| {})),
                    on_stderr: Box::new({
                        let splitter = stderr_splitter.clone();
                        move |data: &[u8]| splitter.write(data)
                    }),
                    on_stderr_end: Box::new(move || stderr_splitter.end(|| {})),
                    on_exit: Box::new({
                        let (on_output, tunnel_process, resolve_exit, resolver) = (on_output.clone(), tunnel_process.clone(), resolve_exit.clone(), resolver.clone());
                        let (cancellation_listener, log_label) = (cancellation_listener.clone(), log_label.clone());
                        move |code: Option<i32>| {
                            let child = tunnel_process.borrow().clone();
                            if let Some(child) = child {
                                cancellation_listener.dispose();
                                let code_text = code.map_or_else(|| "null".to_owned(), |code| code.to_string());
                                on_output(&format!("{log_label} exit({}): + {code_text} ", describe_pid(child.as_ref())), false);
                                *tunnel_process.borrow_mut() = None;
                                resolve_exit.resolve(());
                                resolver.resolve(code.unwrap_or(0));
                            }
                        }
                    }),
                    on_error: Box::new({
                        let (on_output, tunnel_process, resolve_exit, resolver) = (on_output.clone(), tunnel_process.clone(), resolve_exit.clone(), resolver.clone());
                        let (cancellation_listener, log_label) = (cancellation_listener.clone(), log_label.clone());
                        move |error: Error| {
                            let child = tunnel_process.borrow().clone();
                            if let Some(child) = child {
                                cancellation_listener.dispose();
                                on_output(&format!("{log_label} error({}): + {error} ", describe_pid(child.as_ref())), true);
                                *tunnel_process.borrow_mut() = None;
                                resolve_exit.resolve(());
                                resolver.reject(error);
                            }
                        }
                    }),
                };

                let spawned = if self.is_built {
                    on_output("Running tunnel CLI\n", false);
                    let tunnel_command = self.command_location();
                    on_output(&format!("{log_label} Spawning: {tunnel_command} {}\n", args.join(" ")), false);
                    let mut spawn_env = process_environment();
                    spawn_env.extend(env.unwrap_or_default());
                    (self.spawn)(&tunnel_command, args, &SpawnOptions { cwd: Some(home_dir()), env: spawn_env }, handlers)
                } else {
                    on_output("Building tunnel CLI from sources and run\n", false);
                    on_output(&format!("{log_label} Spawning: cargo run -- {}\n", args.join(" ")), false);
                    let mut cargo_args = vec!["run".to_owned(), "--".to_owned()];
                    cargo_args.extend(args.iter().cloned());
                    let mut spawn_env = process_environment();
                    spawn_env.insert("RUST_BACKTRACE".to_owned(), "1".to_owned());
                    spawn_env.extend(env.unwrap_or_default());
                    let cwd = PathModule::native(&ProcessEnvironment).join(&[self.app_root.as_str(), "cli"]);
                    (self.spawn)("cargo", &cargo_args, &SpawnOptions { cwd: Some(cwd), env: spawn_env }, handlers)
                };

                match spawned {
                    Ok(child) => *tunnel_process.borrow_mut() = Some(child),
                    Err(error) => {
                        cancellation_listener.dispose();
                        resolve_exit.resolve(());
                        resolver.reject(error);
                    }
                }
            })
        });

        CodeTunnelCliRun { result, stop }
    }
}

/// `child.pid` na interpolação do original: `undefined` quando o processo nunca iniciou.
fn describe_pid(child: &dyn TunnelChildProcess) -> String {
    child.pid().map_or_else(|| "undefined".to_owned(), |pid| pid.to_string())
}

/// O `process.env`.
fn process_environment() -> BTreeMap<String, String> {
    std::env::vars_os().map(|(key, value)| (key.to_string_lossy().into_owned(), value.to_string_lossy().into_owned())).collect()
}

// region: o spawn de verdade

/// O que uma thread de leitura entrega ao laço.
enum StreamEvent {
    Stdout(Vec<u8>),
    Stderr(Vec<u8>),
    StdoutEnd,
    StderrEnd,
}

type EventQueue = Arc<Mutex<VecDeque<StreamEvent>>>;

/// Lê o fluxo até o fim para a fila e avisa que acabou.
fn read_stream(mut stream: impl Read + Send + 'static, events: EventQueue, data: fn(Vec<u8>) -> StreamEvent, end: fn() -> StreamEvent) {
    std::thread::spawn(move || {
        let mut chunk = [0u8; 8192];
        while let Ok(read) = stream.read(&mut chunk) {
            if read == 0 {
                break;
            }
            if let Ok(mut queue) = events.lock() {
                queue.push_back(data(chunk[..read].to_vec()));
            }
        }
        if let Ok(mut queue) = events.lock() {
            queue.push_back(end());
        }
    });
}

/// Um filho que nunca rodou (o programa não pôde ser iniciado): `kill` não faz nada.
struct NotStarted;

impl TunnelChildProcess for NotStarted {
    fn pid(&self) -> Option<u32> {
        None
    }

    fn kill(&self, _signal: KillSignal) -> bool {
        false
    }
}

/// O filho em andamento.
struct RealChild {
    pid: u32,
    child: RefCell<Child>,
    events: EventQueue,
    open_streams: Cell<u8>,
    /// `Some(código)` depois de o filho terminar (`Some(None)` quando foi morto por sinal).
    exit: Cell<Option<Option<i32>>>,
    grace_polls: Cell<u8>,
    done: Cell<bool>,
    handlers: TunnelChildHandlers,
}

impl RealChild {
    fn start(mut child: Child, handlers: TunnelChildHandlers) -> Rc<Self> {
        let events: EventQueue = Arc::default();
        let mut open_streams = 0;
        if let Some(stdout) = child.stdout.take() {
            open_streams += 1;
            read_stream(stdout, events.clone(), StreamEvent::Stdout, || StreamEvent::StdoutEnd);
        }
        if let Some(stderr) = child.stderr.take() {
            open_streams += 1;
            read_stream(stderr, events.clone(), StreamEvent::Stderr, || StreamEvent::StderrEnd);
        }
        let this = Rc::new(Self {
            pid: child.id(),
            child: RefCell::new(child),
            events,
            open_streams: Cell::new(open_streams),
            exit: Cell::new(None),
            grace_polls: Cell::new(0),
            done: Cell::new(false),
            handlers,
        });
        Self::schedule_poll(&this);
        this
    }

    fn schedule_poll(this: &Rc<Self>) {
        let this = this.clone();
        runtime::set_timeout(move || Self::poll(&this), POLL_INTERVAL_MS);
    }

    fn poll(this: &Rc<Self>) {
        if this.done.get() {
            return;
        }
        this.deliver_events();
        if this.exit.get().is_none() {
            let waited = this.child.borrow_mut().try_wait();
            match waited {
                Ok(Some(status)) => this.exit.set(Some(status.code())),
                Ok(None) => {}
                Err(error) => {
                    this.done.set(true);
                    (this.handlers.on_error)(Error::new(error.to_string()));
                    return;
                }
            }
        }
        if let Some(code) = this.exit.get() {
            // O `exit` espera a cauda da saída: os dados já lidos saem antes dele.
            this.deliver_events();
            if this.open_streams.get() > 0 && this.grace_polls.get() < EXIT_STREAM_GRACE_POLLS {
                this.grace_polls.set(this.grace_polls.get() + 1);
            } else {
                this.done.set(true);
                (this.handlers.on_exit)(code);
                return;
            }
        }
        Self::schedule_poll(this);
    }

    /// Entrega os dados e os fins de fluxo que as threads já enfileiraram, na ordem.
    fn deliver_events(&self) {
        let events: Vec<StreamEvent> = self.events.lock().map(|mut queue| queue.drain(..).collect()).unwrap_or_default();
        for event in events {
            match event {
                StreamEvent::Stdout(data) => (self.handlers.on_stdout)(data.as_slice()),
                StreamEvent::Stderr(data) => (self.handlers.on_stderr)(data.as_slice()),
                StreamEvent::StdoutEnd => {
                    self.open_streams.set(self.open_streams.get().saturating_sub(1));
                    (self.handlers.on_stdout_end)();
                }
                StreamEvent::StderrEnd => {
                    self.open_streams.set(self.open_streams.get().saturating_sub(1));
                    (self.handlers.on_stderr_end)();
                }
            }
        }
    }
}

impl TunnelChildProcess for RealChild {
    fn pid(&self) -> Option<u32> {
        Some(self.pid)
    }

    fn kill(&self, signal: KillSignal) -> bool {
        if self.exit.get().is_some() {
            return false;
        }
        match signal {
            KillSignal::Kill => self.child.borrow_mut().kill().is_ok(),
            KillSignal::Terminate if cfg!(unix) => send_terminate(self.pid),
            KillSignal::Terminate => self.child.borrow_mut().kill().is_ok(),
        }
    }
}

/// `SIGTERM` pelo utilitário `kill`: sem `unsafe` não há a chamada de sistema.
fn send_terminate(pid: u32) -> bool {
    Command::new("kill")
        .arg("-TERM")
        .arg(pid.to_string())
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .is_ok_and(|status| status.success())
}

/// O `child_process.spawn` real, com `stdio: ['ignore', 'pipe', 'pipe']`.
pub fn spawn_process(command: &str, args: &[String], options: &SpawnOptions, handlers: TunnelChildHandlers) -> Result<Rc<dyn TunnelChildProcess>, Error> {
    let mut process = Command::new(command);
    process.args(args).env_clear().envs(&options.env).stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::piped());
    if let Some(cwd) = options.cwd.as_deref().filter(|cwd| !cwd.is_empty()) {
        process.current_dir(cwd);
    }
    match process.spawn() {
        Ok(child) => Ok(RealChild::start(child, handlers)),
        Err(error) => {
            // O Node entrega a falha como evento `error`, depois de quem chamou registrar o filho.
            let failure = spawn_failure(command, &error);
            let on_error = handlers.on_error;
            runtime::queue_microtask(move || on_error(failure));
            Ok(Rc::new(NotStarted))
        }
    }
}

// endregion
