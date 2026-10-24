//! Porte de `src/vs/platform/externalTerminal/node/externalTerminalService.ts`: abrir um terminal externo (o do
//! sistema, ou o configurado) no Windows, no macOS e no Linux, e rodar um comando nele.
//!
//! Decisões de tradução:
//! - **O `cp` (`child_process`) é injetado.** O original passa o módulo `cp` do Node para `spawnTerminal(spawner,
//!   ...)`, e o teste dele passa um `cp` falso (`{ spawn: (command, args, opts) => ({ on: ... }) }`). Aqui o `cp` é o
//!   trait [`Spawner`] (só o `spawn`), e o filho que ele devolve é o trait [`ChildProcess`] (só os eventos que o
//!   serviço escuta: `error`, `exit` e `stderr.on('data')`). O [`ChildProcessSpawner`] é o `cp` real, sobre
//!   `std::process::Command`: o `openTerminal` e o `runInTerminal` o usam, como o original usa o `cp` do Node. As opções
//!   são [`SpawnOptions`] (`cwd`, `env`, `detached`, `windowsVerbatimArguments`); `options` ausente é `None`.
//! - **Eventos do filho.** O `ChildProcessSpawner` entrega `error` e `exit` no laço de eventos do `je-base` (um
//!   temporizador confere o `try_wait` do processo, e uma thread lê o `stderr`, que chega de uma vez, antes do `exit`).
//!   Falha ao iniciar o programa entrega só o `error`, com `errno = "ENOENT"` e `path` quando o programa não existe, que
//!   é o que `improveError` procura (o Node moderno põe o número em `errno`; aqui a propriedade é o texto, que é a
//!   intenção do original).
//! - **Classes.** `ExternalTerminalService` (abstrata, só com `getDefaultTerminalForPlatforms`) é a macro
//!   `abstract_external_terminal_service!`, que as três implementações de [`IExternalTerminalService`] usam para herdar o
//!   método. Os membros `static` (`getDefaultTerminalWindows`, `getDefaultTerminalLinuxReady`, `getWtExePath` com
//!   `@memoize`) são funções associadas; os caches viram `OnceLock` (texto) e `thread_local` (promessas, que não cruzam
//!   threads).
//! - **`process`.** `process.env` é [`SafeProcess::native`]; `getSanitizedEnvironment` é `sanitized_environment`. O
//!   ambiente de um filho é uma lista de pares na ordem do objeto: um valor `null` do `ITerminalEnvironment` que não foi
//!   apagado chega ao Node como o texto `null`, e `undefined` é omitido (ver `to_spawn_environment`); as duas
//!   particularidades do original foram mantidas, inclusive o `-e CHAVE=undefined` do macOS.
//! - **`FileAccess.asFileUri(...)`** (o `.scpt` do macOS) é um [`FileAccess`] injetado em
//!   [`MacExternalTerminalService::with_file_access`]; o `new()` usa um sem raiz de arquivos, que lança `Cannot determine
//!   URI for module id!` ao rodar em Terminal.app ou iTerm, como o `FileAccess` global sem `_VSCODE_FILE_ROOT`.
//! - **`getWindowsShell()`** (de `base/node/processes.ts`, não portado) é a função privada `get_windows_shell`
//!   (`env['comspec'] || 'cmd.exe'`). `findExecutable` e `Promises.exists` são os do `sandbox::node_support` e do
//!   `files::pfs`, síncronos; o `await` do original vira uma rodada de microtarefa onde ele cede.
//! - **`nls.localize`** é [`je_base::nls::localize`] (inglês, o idioma padrão).

use std::cell::RefCell;
use std::io::Read;
use std::process::{Child, Command, Stdio};
use std::rc::Rc;
use std::sync::{Arc, Mutex, OnceLock};

use je_base::async_::{async_fn, async_fn_maybe};
use je_base::errors::{self, Error};
use je_base::network::{FileAccess, FileAccessContext, RemoteAuthorities};
use je_base::nls::localize;
use je_base::path::{Environment, PathModule, ProcessEnvironment as HostEnvironment};
use je_base::platform::Platform;
use je_base::process::{ProcessEnvironment, SafeProcess};
use je_base::processes::sanitize_process_environment;
use je_base::promise::{MaybePromise, Promise, Resolver};
use je_base::runtime;

use super::external_terminal::{DEFAULT_TERMINAL_OSX, IExternalTerminalService, IExternalTerminalSettings, ITerminalForPlatform};
use crate::files::pfs;
use crate::sandbox::node_support::find_executable;
use crate::terminal::terminal::{ITerminalEnvironment, TerminalEnvironmentValue};

/// `SpawnOptions` do `child_process`, só com o que o serviço passa.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct SpawnOptions {
    pub cwd: Option<String>,
    /// O ambiente do filho, na ordem do objeto.
    pub env: Option<Vec<(String, String)>>,
    pub detached: Option<bool>,
    pub windows_verbatim_arguments: Option<bool>,
}

/// O `ChildProcess` do Node, só com os eventos que o serviço escuta. O `on('error')` e o `on('exit')` disparam no máximo
/// uma vez; o `stderr?.on('data')` é um `on_stderr_data` que quem não tem `stderr` simplesmente ignora.
pub trait ChildProcess {
    /// `child.on('error', handler)`.
    fn on_error(&self, handler: Box<dyn FnOnce(Error)>);
    /// `child.on('exit', handler)`: o código de saída, ou `None` se o processo terminou por sinal.
    fn on_exit(&self, handler: Box<dyn FnOnce(Option<i32>)>);
    /// `child.stderr?.on('data', handler)`.
    fn on_stderr_data(&self, handler: Box<dyn FnMut(&str)>);
}

/// O `cp` (`typeof cp`) que o serviço recebe: só o `spawn`.
pub trait Spawner {
    /// `cp.spawn(command, args, options)`.
    fn spawn(&self, command: &str, args: Option<&[String]>, options: Option<&SpawnOptions>) -> Rc<dyn ChildProcess>;
}

/// O `cp` real: inicia o programa com `std::process::Command`. O `stdin` e o `stdout` do filho vão para o nada; o
/// `stderr` é lido por uma thread.
#[derive(Clone, Copy, Debug, Default)]
pub struct ChildProcessSpawner;

/// De quanto em quanto tempo o laço confere se o filho terminou.
const EXIT_POLL_INTERVAL_MS: u64 = 20;

#[cfg(windows)]
fn add_arguments(command: &mut Command, args: &[String], verbatim: bool) {
    use std::os::windows::process::CommandExt;
    if verbatim {
        for arg in args {
            command.raw_arg(arg);
        }
    } else {
        command.args(args);
    }
}

#[cfg(not(windows))]
fn add_arguments(command: &mut Command, args: &[String], _verbatim: bool) {
    command.args(args);
}

#[cfg(windows)]
fn detach(command: &mut Command, detached: bool) {
    use std::os::windows::process::CommandExt;
    if detached {
        command.creation_flags(0x0000_0008); // DETACHED_PROCESS
    }
}

#[cfg(unix)]
fn detach(command: &mut Command, detached: bool) {
    use std::os::unix::process::CommandExt;
    if detached {
        command.process_group(0);
    }
}

#[cfg(not(any(windows, unix)))]
fn detach(_command: &mut Command, _detached: bool) {}

/// O erro de um `spawn` que falhou, com as propriedades que o `improveError` lê.
fn spawn_error(command: &str, error: &std::io::Error) -> Error {
    if error.kind() == std::io::ErrorKind::NotFound {
        Error::new(format!("spawn {command} ENOENT")).with_prop("errno", "ENOENT").with_prop("code", "ENOENT").with_prop("path", command)
    } else {
        Error::new(format!("spawn {command}: {error}")).with_prop("path", command)
    }
}

impl Spawner for ChildProcessSpawner {
    fn spawn(&self, command: &str, args: Option<&[String]>, options: Option<&SpawnOptions>) -> Rc<dyn ChildProcess> {
        let options = options.cloned().unwrap_or_default();
        let mut process = Command::new(command);
        add_arguments(&mut process, args.unwrap_or_default(), options.windows_verbatim_arguments == Some(true));
        if let Some(cwd) = non_empty(options.cwd.as_deref()) {
            process.current_dir(cwd);
        }
        if let Some(env) = &options.env {
            process.env_clear().envs(env.iter().map(|(key, value)| (key.as_str(), value.as_str())));
        }
        detach(&mut process, options.detached == Some(true));
        process.stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::piped());
        StdChild::start(process.spawn().map_err(|error| spawn_error(command, &error)))
    }
}

/// O filho de um [`ChildProcessSpawner`]: guarda os tratadores e os dispara no laço de eventos.
struct StdChild {
    process: RefCell<Option<Child>>,
    stderr_buffer: Arc<Mutex<Vec<u8>>>,
    error_handlers: RefCell<Vec<Box<dyn FnOnce(Error)>>>,
    exit_handlers: RefCell<Vec<Box<dyn FnOnce(Option<i32>)>>>,
    stderr_handlers: RefCell<Vec<Box<dyn FnMut(&str)>>>,
}

impl StdChild {
    fn start(spawned: Result<Child, Error>) -> Rc<Self> {
        let this = Rc::new(Self {
            process: RefCell::new(None),
            stderr_buffer: Arc::new(Mutex::new(Vec::new())),
            error_handlers: RefCell::new(Vec::new()),
            exit_handlers: RefCell::new(Vec::new()),
            stderr_handlers: RefCell::new(Vec::new()),
        });
        match spawned {
            Ok(mut child) => {
                if let Some(mut stderr) = child.stderr.take() {
                    let buffer = this.stderr_buffer.clone();
                    std::thread::spawn(move || {
                        let mut chunk = [0u8; 4096];
                        while let Ok(read) = stderr.read(&mut chunk) {
                            if read == 0 {
                                break;
                            }
                            if let Ok(mut buffer) = buffer.lock() {
                                buffer.extend_from_slice(&chunk[..read]);
                            }
                        }
                    });
                }
                *this.process.borrow_mut() = Some(child);
                Self::schedule_poll(&this);
            }
            Err(error) => {
                // O Node entrega o `error` no tick seguinte, depois de quem chamou registrar os tratadores.
                let this = this.clone();
                runtime::queue_microtask(move || this.emit_error(&error));
            }
        }
        this
    }

    fn schedule_poll(this: &Rc<Self>) {
        let this = this.clone();
        runtime::set_timeout(move || Self::poll(&this), EXIT_POLL_INTERVAL_MS);
    }

    fn poll(this: &Rc<Self>) {
        let status = this.process.borrow_mut().as_mut().map(Child::try_wait);
        match status {
            Some(Ok(Some(status))) => this.emit_exit(status.code()),
            Some(Ok(None)) => Self::schedule_poll(this),
            Some(Err(error)) => this.emit_error(&Error::new(error.to_string())),
            None => {}
        }
    }

    fn emit_error(&self, error: &Error) {
        let handlers = std::mem::take(&mut *self.error_handlers.borrow_mut());
        for handler in handlers {
            handler(error.clone());
        }
    }

    fn emit_exit(&self, code: Option<i32>) {
        let stderr = self.stderr_buffer.lock().map(|buffer| String::from_utf8_lossy(&buffer).into_owned()).unwrap_or_default();
        if !stderr.is_empty() {
            for handler in self.stderr_handlers.borrow_mut().iter_mut() {
                handler(&stderr);
            }
        }
        let handlers = std::mem::take(&mut *self.exit_handlers.borrow_mut());
        for handler in handlers {
            handler(code);
        }
    }
}

impl ChildProcess for StdChild {
    fn on_error(&self, handler: Box<dyn FnOnce(Error)>) {
        self.error_handlers.borrow_mut().push(handler);
    }

    fn on_exit(&self, handler: Box<dyn FnOnce(Option<i32>)>) {
        self.exit_handlers.borrow_mut().push(handler);
    }

    fn on_stderr_data(&self, handler: Box<dyn FnMut(&str)>) {
        self.stderr_handlers.borrow_mut().push(handler);
    }
}

/// `const TERMINAL_TITLE = nls.localize('console.title', "VS Code Console")`.
fn terminal_title() -> String {
    localize("console.title", "VS Code Console", &[])
}

/// A string só conta se não for vazia (a verdade de `str` e de `str || padrão` no JavaScript).
fn non_empty(value: Option<&str>) -> Option<&str> {
    value.filter(|value| !value.is_empty())
}

/// O módulo `path` do sistema (`path.basename`).
fn native_path() -> PathModule<'static> {
    static HOST: HostEnvironment = HostEnvironment;
    PathModule::native(&HOST)
}

/// `process.env`.
fn process_environment() -> ProcessEnvironment {
    SafeProcess::native().env
}

/// `process.env[name]`: no Windows o nome não distingue maiúsculas de minúsculas.
fn env_entry<'a>(env: &'a ProcessEnvironment, name: &str) -> Option<&'a str> {
    env.get(name).map(String::as_str).or_else(|| if cfg!(windows) { env.iter().find(|(key, _)| key.eq_ignore_ascii_case(name)).map(|(_, value)| value.as_str()) } else { None })
}

/// `getWindowsShell()` de `base/node/processes.ts`: `env['comspec'] || 'cmd.exe'`.
fn get_windows_shell() -> String {
    non_empty(env_entry(&process_environment(), "comspec")).unwrap_or("cmd.exe").to_owned()
}

/// `getSanitizedEnvironment(process)`: o ambiente do processo sem o que é do Electron, do VS Code, do Snap e do GDK-Pixbuf.
fn sanitized_environment() -> ProcessEnvironment {
    let mut env = process_environment();
    sanitize_process_environment(&mut env, &[]);
    env
}

/// O `env: getSanitizedEnvironment(process)` que o `spawnTerminal` das três implementações passa ao `spawn`.
fn sanitized_spawn_environment() -> Vec<(String, String)> {
    sanitized_environment().into_iter().collect()
}

/// `Object.assign({}, getSanitizedEnvironment(process), envVars)`: as chaves do ambiente na ordem dele, depois as novas de
/// `envVars`; o valor de `envVars` vence.
fn merge_environment(env_vars: &ITerminalEnvironment) -> ITerminalEnvironment {
    let mut merged: ITerminalEnvironment = sanitized_environment().into_iter().map(|(key, value)| (key, TerminalEnvironmentValue::Value(value))).collect();
    for (key, value) in env_vars {
        match merged.iter_mut().find(|(existing, _)| existing == key) {
            Some(entry) => entry.1 = value.clone(),
            None => merged.push((key.clone(), value.clone())),
        }
    }
    merged
}

/// O que o `spawn` do Node faz com o objeto `env`: o texto fica, `undefined` é omitido e `null` vira o texto `null`.
fn to_spawn_environment(env: &ITerminalEnvironment) -> Vec<(String, String)> {
    env.iter()
        .filter_map(|(key, value)| match value {
            TerminalEnvironmentValue::Value(value) => Some((key.clone(), value.clone())),
            TerminalEnvironmentValue::Null => Some((key.clone(), "null".to_owned())),
            TerminalEnvironmentValue::Undefined => None,
        })
        .collect()
}

/// O ambiente do filho do Windows e do Linux: o do processo com `envVars` por cima, sem as variáveis de valor `null`
/// (`Object.keys(env).filter(v => env[v] === null).forEach(key => delete env[key])`).
fn child_environment(env_vars: &ITerminalEnvironment) -> Vec<(String, String)> {
    let mut merged = merge_environment(env_vars);
    merged.retain(|(_, value)| *value != TerminalEnvironmentValue::Null);
    to_spawn_environment(&merged)
}

/// `child.on('error', e); child.on('exit', () => c())`.
fn settle_on_exit(child: &dyn ChildProcess, resolver: &Resolver<()>) {
    let on_error = resolver.clone();
    let on_exit = resolver.clone();
    child.on_error(Box::new(move |error| on_error.reject(error)));
    child.on_exit(Box::new(move |_| on_exit.resolve(())));
}

/// `improveError`: tenta transformar erros do sistema em mensagens mais úteis.
fn improve_error(error: Error) -> Error {
    if error.prop("errno") == Some("ENOENT")
        && let Some(path) = non_empty(error.prop("path"))
    {
        return Error::new(localize("ext.term.app.not.found", "can't find terminal application '{0}'", &[path]));
    }
    error
}

/// `setupSpawnErrorHandling`: liga o filho à promessa de `runInTerminal`. Saída 0 resolve; senão rejeita com a primeira
/// linha do `stderr`, ou com o código de saída se o `stderr` ficou vazio.
fn setup_spawn_error_handling(cmd: &dyn ChildProcess, resolver: &Resolver<Option<i32>>, terminal_app: &str) {
    let stderr = Rc::new(RefCell::new(String::new()));
    let on_error = resolver.clone();
    cmd.on_error(Box::new(move |error| on_error.reject(improve_error(error))));
    let sink = stderr.clone();
    cmd.on_stderr_data(Box::new(move |data| sink.borrow_mut().push_str(data)));
    let on_exit = resolver.clone();
    let terminal_app = terminal_app.to_owned();
    cmd.on_exit(Box::new(move |code| {
        if code == Some(0) {
            on_exit.resolve(None);
            return;
        }
        let stderr = stderr.borrow();
        if stderr.is_empty() {
            let code = code.map_or_else(|| "null".to_owned(), |code| code.to_string());
            on_exit.reject(Error::new(localize("terminal.launch.failed", "Launching '{0}' failed with exit code {1}", &[&terminal_app, &code])));
        } else {
            on_exit.reject(Error::new(stderr.split('\n').next().unwrap_or_default()));
        }
    }));
}

/// Cita os argumentos que têm espaço e junta tudo numa string separada por espaços.
fn quote(args: &[String]) -> String {
    let mut result = String::new();
    for arg in args {
        if arg.contains(' ') {
            result.push('"');
            result.push_str(arg);
            result.push('"');
        } else {
            result.push_str(arg);
        }
        result.push(' ');
    }
    result
}

/// O método `getDefaultTerminalForPlatforms` da classe abstrata `ExternalTerminalService`, herdado pelas três
/// implementações.
macro_rules! abstract_external_terminal_service {
    () => {
        fn get_default_terminal_for_platforms(&self) -> Promise<ITerminalForPlatform> {
            async_fn(async move {
                let windows = WindowsExternalTerminalService::get_default_terminal_windows();
                let linux = LinuxExternalTerminalService::get_default_terminal_linux_ready().await?;
                Ok(ITerminalForPlatform { windows, linux, osx: DEFAULT_TERMINAL_OSX.to_owned() })
            })
        }
    };
}

thread_local! {
    /// `@memoize getWtExePath()`.
    static WT_EXE_PATH: RefCell<Option<Promise<Option<String>>>> = const { RefCell::new(None) };
    /// `LinuxExternalTerminalService._DEFAULT_TERMINAL_LINUX_READY`.
    static DEFAULT_TERMINAL_LINUX_READY: RefCell<Option<Promise<String>>> = const { RefCell::new(None) };
}

/// `WindowsExternalTerminalService`.
#[derive(Clone, Copy, Debug, Default)]
pub struct WindowsExternalTerminalService;

impl WindowsExternalTerminalService {
    const CMD: &'static str = "cmd.exe";

    pub fn new() -> Self {
        Self
    }

    /// `spawnTerminal(spawner, configuration, command, cwd)`.
    pub fn spawn_terminal(&self, spawner: Rc<dyn Spawner>, configuration: &IExternalTerminalSettings, command: &str, cwd: Option<&str>) -> Promise<()> {
        let exec = non_empty(configuration.windows_exec.as_deref()).map_or_else(Self::get_default_terminal_windows, str::to_owned);

        // Make the drive letter uppercase on Windows (see #9448)
        let cwd = cwd.map(|cwd| {
            let mut chars = cwd.chars();
            match (chars.next(), chars.next()) {
                (Some(drive), Some(':')) if drive.len_utf16() == 1 => drive.to_uppercase().chain(cwd[drive.len_utf8()..].chars()).collect(),
                _ => cwd.to_owned(),
            }
        });

        // cmder ignores the environment cwd and instead opts to always open in %USERPROFILE%
        // unless otherwise specified
        let basename = native_path().basename(&exec, Some(".exe")).to_lowercase();
        if basename == "cmder" {
            let args = non_empty(cwd.as_deref()).map(|cwd| vec![cwd.to_owned()]);
            spawner.spawn(&exec, args.as_deref(), None);
            return Promise::resolved(());
        }

        let mut cmd_args = vec!["/c".to_owned(), "start".to_owned(), "/wait".to_owned()];
        if exec.contains(' ') {
            // The "" argument is the window title. Without this, exec doesn't work when the path
            // contains spaces. #6590
            // Title is Execution Path. #220129
            cmd_args.push(exec.clone());
        }
        cmd_args.push(exec);
        // Add starting directory parameter for Windows Terminal (see #90734)
        if basename == "wt" {
            cmd_args.push("-d .".to_owned());
        }

        Promise::with_executor(move |resolver| {
            let options = SpawnOptions { cwd, env: Some(sanitized_spawn_environment()), detached: Some(true), windows_verbatim_arguments: None };
            let child = spawner.spawn(command, Some(&cmd_args), Some(&options));
            settle_on_exit(&*child, &resolver);
        })
    }

    /// `getDefaultTerminalWindows()`: o `cmd.exe` do sistema (o `Sysnative` num processo de 32 bits no Windows de 64).
    pub fn get_default_terminal_windows() -> String {
        static DEFAULT_TERMINAL_WINDOWS: OnceLock<String> = OnceLock::new();
        DEFAULT_TERMINAL_WINDOWS
            .get_or_init(|| {
                let env = process_environment();
                let is_wow64 = env_entry(&env, "PROCESSOR_ARCHITEW6432").is_some();
                let windir = non_empty(env_entry(&env, "windir")).unwrap_or("C:\\Windows");
                format!("{windir}\\{}\\cmd.exe", if is_wow64 { "Sysnative" } else { "System32" })
            })
            .clone()
    }

    /// `getWtExePath()` (`@memoize`): o caminho do Windows Terminal, se achado no `PATH`.
    fn get_wt_exe_path() -> Promise<Option<String>> {
        WT_EXE_PATH.with(|memo| {
            memo.borrow_mut()
                .get_or_insert_with(|| async_fn(async { Ok(errors::catch(|| find_executable("wt", None, None, None, None)).unwrap_or(None)) }))
                .clone()
        })
    }
}

impl IExternalTerminalService for WindowsExternalTerminalService {
    fn open_terminal(&self, configuration: &IExternalTerminalSettings, cwd: Option<&str>) -> Promise<()> {
        self.spawn_terminal(Rc::new(ChildProcessSpawner), configuration, &get_windows_shell(), cwd)
    }

    fn run_in_terminal(&self, _title: &str, dir: &str, args: &[String], env_vars: &ITerminalEnvironment, settings: &IExternalTerminalSettings) -> Promise<Option<i32>> {
        let exec = non_empty(settings.windows_exec.as_deref()).map_or_else(Self::get_default_terminal_windows, str::to_owned);
        let (dir, args, env_vars) = (dir.to_owned(), args.to_vec(), env_vars.clone());

        async_fn_maybe(async move {
            let wt = Self::get_wt_exe_path().await?;

            Ok(MaybePromise::Promise(Promise::with_executor(move |resolver| {
                let title = format!("\"{dir} - {}\"", terminal_title());
                let command = format!("\"{}\" & pause", args.join("\" \"")); // use '|' to only pause on non-zero exit code

                // merge environment variables into a copy of the process.env, and delete the ones with a null value
                let options = SpawnOptions { cwd: Some(dir), env: Some(child_environment(&env_vars)), detached: None, windows_verbatim_arguments: Some(true) };

                let (spawn_exec, cmd_args) = if native_path().basename(&exec, Some(".exe")) == "wt" {
                    // Handle Windows Terminal specially; -d to set the cwd and run a cmd.exe instance
                    // inside it
                    (exec, vec!["-d".to_owned(), ".".to_owned(), Self::CMD.to_owned(), "/c".to_owned(), command])
                } else if let Some(wt) = wt {
                    // prefer to use the window terminal to spawn if it's available instead
                    // of start, since that allows ctrl+c handling (#81322)
                    (wt, vec!["-d".to_owned(), ".".to_owned(), exec, "/c".to_owned(), command])
                } else {
                    (Self::CMD.to_owned(), vec!["/c".to_owned(), "start".to_owned(), title, "/wait".to_owned(), exec, "/c".to_owned(), format!("\"{command}\"")])
                };

                let cmd = ChildProcessSpawner.spawn(&spawn_exec, Some(&cmd_args), Some(&options));

                let on_error = resolver.clone();
                cmd.on_error(Box::new(move |error| on_error.reject(improve_error(error))));

                resolver.resolve(None);
            })))
        })
    }

    abstract_external_terminal_service!();
}

/// `MacExternalTerminalService`.
pub struct MacExternalTerminalService {
    file_access: Rc<FileAccess>,
}

impl Default for MacExternalTerminalService {
    fn default() -> Self {
        Self::new()
    }
}

impl MacExternalTerminalService {
    /// osascript is the AppleScript interpreter on OS X
    const OSASCRIPT: &'static str = "/usr/bin/osascript";

    /// Um serviço com um [`FileAccess`] sem raiz de arquivos: o `.scpt` do Terminal.app e do iTerm não é encontrado (ver
    /// [`Self::with_file_access`]).
    pub fn new() -> Self {
        Self::with_file_access(Rc::new(FileAccess::new(Rc::new(HostEnvironment), FileAccessContext::default(), RemoteAuthorities::new(false))))
    }

    /// O serviço com o `FileAccess` do aplicativo, de onde sai o caminho dos `.scpt` (`FileAccess.asFileUri`).
    pub fn with_file_access(file_access: Rc<FileAccess>) -> Self {
        Self { file_access }
    }

    /// `spawnTerminal(spawner, configuration, cwd)`.
    pub fn spawn_terminal(&self, spawner: Rc<dyn Spawner>, configuration: &IExternalTerminalSettings, cwd: Option<&str>) -> Promise<()> {
        let terminal_app = non_empty(configuration.osx_exec.as_deref()).unwrap_or(DEFAULT_TERMINAL_OSX).to_owned();
        let cwd = cwd.map(str::to_owned);

        Promise::with_executor(move |resolver| {
            let mut args = vec!["-a".to_owned(), terminal_app];
            if let Some(cwd) = non_empty(cwd.as_deref()) {
                args.push(cwd.to_owned());
            }
            let options = SpawnOptions { cwd, env: Some(sanitized_spawn_environment()), ..SpawnOptions::default() };
            let child = spawner.spawn("/usr/bin/open", Some(&args), Some(&options));
            settle_on_exit(&*child, &resolver);
        })
    }
}

impl IExternalTerminalService for MacExternalTerminalService {
    fn open_terminal(&self, configuration: &IExternalTerminalSettings, cwd: Option<&str>) -> Promise<()> {
        self.spawn_terminal(Rc::new(ChildProcessSpawner), configuration, cwd)
    }

    fn run_in_terminal(&self, title: &str, dir: &str, args: &[String], env_vars: &ITerminalEnvironment, settings: &IExternalTerminalSettings) -> Promise<Option<i32>> {
        let terminal_app = non_empty(settings.osx_exec.as_deref()).unwrap_or(DEFAULT_TERMINAL_OSX).to_owned();

        Promise::with_executor(|resolver| {
            if terminal_app == DEFAULT_TERMINAL_OSX || terminal_app == "iTerm.app" {
                // On OS X we launch an AppleScript that creates (or reuses) a Terminal window
                // and then launches the program inside that window.

                let script = if terminal_app == DEFAULT_TERMINAL_OSX { "TerminalHelper" } else { "iTermHelper" };
                let script_uri = self.file_access.as_file_uri(&format!("vs/workbench/contrib/externalTerminal/node/{script}.scpt"));
                let script_path = script_uri.fs_path(HostEnvironment.platform()).to_string_lossy();

                let mut osa_args = vec![script_path, "-t".to_owned(), non_empty(Some(title)).map_or_else(terminal_title, str::to_owned), "-w".to_owned(), dir.to_owned()];

                for arg in args {
                    osa_args.push("-a".to_owned());
                    osa_args.push(arg.clone());
                }

                // merge environment variables into a copy of the process.env
                for (key, value) in merge_environment(env_vars) {
                    match value {
                        TerminalEnvironmentValue::Null => {
                            osa_args.push("-u".to_owned());
                            osa_args.push(key);
                        }
                        TerminalEnvironmentValue::Value(value) => {
                            osa_args.push("-e".to_owned());
                            osa_args.push(format!("{key}={value}"));
                        }
                        TerminalEnvironmentValue::Undefined => {
                            osa_args.push("-e".to_owned());
                            osa_args.push(format!("{key}=undefined"));
                        }
                    }
                }

                let osa = ChildProcessSpawner.spawn(Self::OSASCRIPT, Some(&osa_args), None);
                setup_spawn_error_handling(&*osa, &resolver, &terminal_app);
            } else if terminal_app == "Ghostty.app" {
                // Ghostty uses CLI flags directly instead of AppleScript like Mac Terminal and iTerm
                // Note: -na is required (not just -a) because we need to spawn a new instance that
                // receives our --args. With just -a, if Ghostty is already running, open will
                // activate the existing instance and ignore --args entirely.
                let env = to_spawn_environment(&merge_environment(env_vars));
                let mut open_args = vec!["-na".to_owned(), "Ghostty.app".to_owned(), "--args".to_owned()];
                open_args.push(format!("--working-directory={dir}"));
                open_args.push("--wait-after-command=true".to_owned());
                open_args.push("-e".to_owned());
                open_args.extend(args.iter().cloned());

                let cmd = ChildProcessSpawner.spawn("/usr/bin/open", Some(&open_args), Some(&SpawnOptions { env: Some(env), ..SpawnOptions::default() }));
                setup_spawn_error_handling(&*cmd, &resolver, &terminal_app);
            } else {
                resolver.reject(Error::new(localize("mac.terminal.type.not.supported", "'{0}' not supported", &[&terminal_app])));
            }
        })
    }

    abstract_external_terminal_service!();
}

/// `LinuxExternalTerminalService`.
#[derive(Clone, Copy, Debug, Default)]
pub struct LinuxExternalTerminalService;

impl LinuxExternalTerminalService {
    pub fn new() -> Self {
        Self
    }

    /// `WAIT_MESSAGE`.
    fn wait_message() -> String {
        localize("press.any.key", "Press any key to continue...", &[])
    }

    /// O `settings.linuxExec ? Promise.resolve(settings.linuxExec) : getDefaultTerminalLinuxReady()` de `runInTerminal` e de
    /// `spawnTerminal`.
    fn exec_promise(linux_exec: Option<&str>) -> Promise<String> {
        match non_empty(linux_exec) {
            Some(exec) => Promise::resolved(exec.to_owned()),
            None => Self::get_default_terminal_linux_ready(),
        }
    }

    fn spawn_terminal_with_env(exec: &str, args: &[String], dir: &str, env_vars: &ITerminalEnvironment, resolver: &Resolver<Option<i32>>) {
        let options = SpawnOptions { cwd: Some(dir.to_owned()), env: Some(child_environment(env_vars)), detached: None, windows_verbatim_arguments: None };
        let cmd = ChildProcessSpawner.spawn(exec, Some(args), Some(&options));
        setup_spawn_error_handling(&*cmd, resolver, exec);
    }

    /// O terminal padrão do Linux, pela ordem: o `x-terminal-emulator` do Debian, o `gnome-terminal` e o `konsole` pela
    /// sessão, o `COLORTERM`, o `TERM` e por fim o `xterm`.
    fn default_terminal_linux(is_debian: bool, env: &ProcessEnvironment) -> String {
        let desktop_session = env_entry(env, "DESKTOP_SESSION");
        if is_debian {
            "x-terminal-emulator".to_owned()
        } else if desktop_session == Some("gnome") || desktop_session == Some("gnome-classic") {
            "gnome-terminal".to_owned()
        } else if desktop_session == Some("kde-plasma") {
            "konsole".to_owned()
        } else if let Some(colorterm) = non_empty(env_entry(env, "COLORTERM")) {
            colorterm.to_owned()
        } else if let Some(term) = non_empty(env_entry(env, "TERM")) {
            term.to_owned()
        } else {
            "xterm".to_owned()
        }
    }

    /// `getDefaultTerminalLinuxReady()`: o terminal padrão, calculado uma vez. Fora do Linux é `xterm`.
    pub fn get_default_terminal_linux_ready() -> Promise<String> {
        async_fn_maybe(async move {
            if DEFAULT_TERMINAL_LINUX_READY.with(|ready| ready.borrow().is_none()) {
                let ready = if !Platform::detect().is_linux {
                    Promise::resolved("xterm".to_owned())
                } else {
                    let is_debian = Promise::resolved(pfs::exists("/etc/debian_version")).await?;
                    Promise::with_executor(move |resolver| resolver.resolve(Self::default_terminal_linux(is_debian, &process_environment())))
                };
                DEFAULT_TERMINAL_LINUX_READY.with(|slot| *slot.borrow_mut() = Some(ready));
            }
            Ok(MaybePromise::Promise(DEFAULT_TERMINAL_LINUX_READY.with(|ready| ready.borrow().clone().expect("definido acima"))))
        })
    }

    /// `spawnTerminal(spawner, configuration, cwd)`.
    pub fn spawn_terminal(&self, spawner: Rc<dyn Spawner>, configuration: &IExternalTerminalSettings, cwd: Option<&str>) -> Promise<()> {
        let exec_promise = Self::exec_promise(configuration.linux_exec.as_deref());
        let cwd = cwd.map(str::to_owned);

        Promise::with_executor(move |resolver| {
            exec_promise.then(move |exec| {
                let basename = native_path().basename(&exec, None).to_lowercase();
                let args = match non_empty(cwd.as_deref()) {
                    Some(cwd) if basename == "ghostty" => vec![format!("--working-directory={cwd}")],
                    _ => Vec::new(),
                };
                let options = SpawnOptions { cwd, env: Some(sanitized_spawn_environment()), ..SpawnOptions::default() };
                let child = spawner.spawn(&exec, Some(&args), Some(&options));
                settle_on_exit(&*child, &resolver);
            });
        })
    }
}

impl IExternalTerminalService for LinuxExternalTerminalService {
    fn open_terminal(&self, configuration: &IExternalTerminalSettings, cwd: Option<&str>) -> Promise<()> {
        self.spawn_terminal(Rc::new(ChildProcessSpawner), configuration, cwd)
    }

    fn run_in_terminal(&self, _title: &str, dir: &str, args: &[String], env_vars: &ITerminalEnvironment, settings: &IExternalTerminalSettings) -> Promise<Option<i32>> {
        let exec_promise = Self::exec_promise(settings.linux_exec.as_deref());
        let (dir, args, env_vars) = (dir.to_owned(), args.to_vec(), env_vars.clone());

        Promise::with_executor(move |resolver| {
            exec_promise.then(move |exec| {
                let basename = native_path().basename(&exec, None).to_lowercase();
                if basename == "ghostty" {
                    let mut ghostty_args = Vec::new();
                    if !dir.is_empty() {
                        ghostty_args.push(format!("--working-directory={dir}"));
                    }
                    ghostty_args.push("--wait-after-command=true".to_owned());
                    if !args.is_empty() {
                        ghostty_args.push("-e".to_owned());
                        ghostty_args.extend(args);
                    }
                    Self::spawn_terminal_with_env(&exec, &ghostty_args, &dir, &env_vars, &resolver);
                    return;
                }

                let mut term_args = Vec::new();
                //termArgs.push('--title');
                //termArgs.push(`"${TERMINAL_TITLE}"`);
                term_args.push(if exec.contains("gnome-terminal") { "-x" } else { "-e" }.to_owned());
                term_args.push("bash".to_owned());
                term_args.push("-c".to_owned());

                let bash_command = format!("{}; echo; read -p \"{}\" -n1;", quote(&args), Self::wait_message());
                term_args.push(format!("''{bash_command}''")); // wrapping argument in two sets of ' because node is so "friendly" that it removes one set...

                Self::spawn_terminal_with_env(&exec, &term_args, &dir, &env_vars, &resolver);
            });
        })
    }

    abstract_external_terminal_service!();
}
