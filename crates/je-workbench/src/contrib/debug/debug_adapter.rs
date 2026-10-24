//! Porte de `src/vs/workbench/contrib/debug/node/debugAdapter.ts`: os adaptadores de depuração que falam por fluxos de
//! bytes (`StreamDebugAdapter`, `SocketDebugAdapter`, `NamedPipeDebugAdapter`, `ExecutableDebugAdapter`) e os
//! auxiliares (`prepareWindowsBatchCommand`, `platformAdapterExecutable`).
//!
//! Notas de tradução:
//! - **E/S.** O `net.Socket` e o `child_process` do Node não existem no porte: o fluxo é o [`INodeStream`] (o mesmo
//!   subconjunto de `net.Socket` do `NodeSocket`), e quem abre sockets e processos de verdade (a camada de plataforma, ligada
//!   ao laço de eventos) implementa [`INetworkConnector`] e [`IProcessLauncher`]. A lógica do protocolo (cabeçalho
//!   `Content-Length`, quadros parciais e múltiplos, erro de JSON) é a do original, em [`StreamCore`].
//! - **Herança.** `StreamDebugAdapter` abstrata é o [`StreamCore`] (estado e protocolo) que cada adaptador concreto guarda;
//!   `NetworkDebugAdapter` é o [`NetworkDebugAdapter`] genérico sobre o conector (`SocketDebugAdapter` e
//!   `NamedPipeDebugAdapter` são os construtores [`NetworkDebugAdapter::socket`] e [`NetworkDebugAdapter::named_pipe`]).
//! - `Buffer` é `Vec<u8>`; `JSON.stringify(message)` e `JSON.parse` são o `to_json`/`from_json` de `ProtocolMessage`.
//! - `killTree` (TASKKILL no Windows) fica a cargo do [`IAdapterProcess::kill_tree`].
//! - `platformAdapterExecutable` lê as contribuições `debuggers` do manifesto como objetos JSON (o `IDebuggerContribution`
//!   é um objeto aberto) e o `fsPath` da pasta da extensão.

use std::cell::RefCell;
use std::rc::{Rc, Weak};

use je_base::errors::Error;
use je_base::event::Event;
use je_base::lifecycle::{DisposableStore, IDisposable};
use je_base::nls::localize;
use je_base::path::{PathModule, ProcessEnvironment};
use je_base::platform::Platform;
use je_base::promise::{Promise, Resolver};
use je_base::buffer::VSBuffer;
use je_base::async_::async_fn;
use je_extensions::scanner::ExtensionDescription;
use je_base::uri::URI;
use serde_json::Value;

use super::abstract_debug_adapter::{AbstractDebugAdapter, DebugAdapterState};
use super::debug::{IDebugAdapterExecutable, IDebugAdapterNamedPipeServer, IDebugAdapterServer};
use super::debug_protocol::ProtocolMessage;
use je_base::parts::ipc::node_net::INodeStream;

const WINDOWS_BATCH_UNQUOTED_CHARACTERS: &str = "#$*+-./:?@\\_";

fn windows_batch_invalid(text: &str) -> bool {
    text.chars().any(|c| c == '\0' || c == '\r' || c == '\n')
}

fn windows_batch_argument_needs_quotes(argument: &str) -> bool {
    if argument.is_empty() || argument.ends_with('\\') {
        return true;
    }

    for character in argument.chars() {
        let code_point = character as u32;
        let is_ascii_alpha_numeric = (0x30..=0x39).contains(&code_point) || (0x41..=0x5A).contains(&code_point) || (0x61..=0x7A).contains(&code_point);
        if (code_point <= 0x7F && !is_ascii_alpha_numeric && !WINDOWS_BATCH_UNQUOTED_CHARACTERS.contains(character)) || character.is_control() {
            return true;
        }
    }

    false
}

fn escape_windows_batch_argument(argument: &str, force_quotes: bool) -> String {
    let quote = force_quotes || windows_batch_argument_needs_quotes(argument);
    let mut result = String::new();
    if quote {
        result.push('"');
    }
    let mut backslashes = 0;

    for character in argument.chars() {
        if character == '\\' {
            backslashes += 1;
        } else {
            if character == '"' {
                result.push_str(&"\\".repeat(backslashes));
                result.push('"');
            } else if character == '%' {
                result.push_str("%%cd:~,");
            }
            backslashes = 0;
        }
        result.push(character);
    }

    if quote {
        result.push_str(&"\\".repeat(backslashes));
        result.push('"');
    }

    result
}

/// `prepareWindowsBatchCommand(command, args)`: builds an injection-safe cmd.exe invocation for a Windows batch file.
pub fn prepare_windows_batch_command(command: &str, args: &[String]) -> Result<Vec<String>, Error> {
    if command.contains('"') || windows_batch_invalid(command) || args.iter().any(|argument| windows_batch_invalid(argument)) {
        return Err(Error::new(localize("invalidWindowsBatchCommand", "Debug adapter commands and arguments contain invalid characters.", &[])));
    }

    let mut parts = vec![escape_windows_batch_argument(command, true)];
    parts.extend(args.iter().map(|argument| escape_windows_batch_argument(argument, false)));
    let shell_command = parts.join(" ");

    Ok(vec!["/e:ON".to_owned(), "/v:OFF".to_owned(), "/d".to_owned(), "/c".to_owned(), format!("\"{shell_command}\"")])
}

// ---- StreamDebugAdapter

const TWO_CRLF: &[u8] = b"\r\n\r\n";

/// O estado e o protocolo de `StreamDebugAdapter`: `connect`, `sendMessage` e `handleData`.
pub struct StreamCore {
    output_stream: RefCell<Option<Rc<dyn INodeStream>>>,
    raw_data: RefCell<Vec<u8>>,
    content_length: RefCell<i64>,
    listeners: Rc<DisposableStore>,
}

impl StreamCore {
    pub fn new() -> Self {
        Self { output_stream: RefCell::new(None), raw_data: RefCell::new(Vec::new()), content_length: RefCell::new(-1), listeners: DisposableStore::new() }
    }

    /// `connect(readable, writable)`: `deliver` é o `acceptMessage` do adaptador e `on_error` o `_onError.fire`.
    pub fn connect(self: &Rc<Self>, readable: &Rc<dyn INodeStream>, writable: Rc<dyn INodeStream>, deliver: Rc<dyn Fn(ProtocolMessage)>, on_error: Rc<dyn Fn(Error)>) {
        *self.output_stream.borrow_mut() = Some(writable);
        *self.raw_data.borrow_mut() = Vec::new();
        *self.content_length.borrow_mut() = -1;

        let this = self.clone();
        self.listeners.add(readable.on_data().listen(move |data| this.handle_data(&data.to_vec(), &deliver, &on_error)));
    }

    /// `sendMessage(message)`.
    pub fn send_message(&self, message: &ProtocolMessage) {
        let stream = self.output_stream.borrow().clone();
        if let Some(stream) = stream {
            let json = message.to_json().to_string();
            let payload = format!("Content-Length: {}{}{}", json.len(), String::from_utf8_lossy(TWO_CRLF), json);
            // O erro de escrita chega pelo evento de erro do fluxo.
            let _ = stream.write(VSBuffer::wrap(payload.into_bytes()));
        }
    }

    fn handle_data(&self, data: &[u8], deliver: &Rc<dyn Fn(ProtocolMessage)>, on_error: &Rc<dyn Fn(Error)>) {
        self.raw_data.borrow_mut().extend_from_slice(data);

        loop {
            let content_length = *self.content_length.borrow();
            if content_length >= 0 {
                let content_length = content_length as usize;
                if self.raw_data.borrow().len() >= content_length {
                    let message_bytes: Vec<u8> = self.raw_data.borrow()[..content_length].to_vec();
                    self.raw_data.borrow_mut().drain(..content_length);
                    *self.content_length.borrow_mut() = -1;
                    let message = String::from_utf8_lossy(&message_bytes).into_owned();
                    if !message.is_empty() {
                        match serde_json::from_str::<Value>(&message) {
                            Ok(value) => deliver(ProtocolMessage::from_json(&value)),
                            Err(error) => on_error(Error::new(format!("{error}\n{message}"))),
                        }
                    }
                    continue; // there may be more complete messages to process
                }
            } else {
                let idx = self.raw_data.borrow().windows(TWO_CRLF.len()).position(|window| window == TWO_CRLF);
                if let Some(idx) = idx {
                    let header = String::from_utf8_lossy(&self.raw_data.borrow()[..idx]).into_owned();
                    // allow for non-RFC 2822 conforming line separators
                    for line in header.split('\n').map(|line| line.strip_suffix('\r').unwrap_or(line)) {
                        // `h.split(/: */)`
                        let (key, value) = match line.find(':') {
                            Some(colon) => (&line[..colon], Some(line[colon + 1..].trim_start_matches(' '))),
                            None => (line, None),
                        };
                        if key == "Content-Length" {
                            *self.content_length.borrow_mut() = value.and_then(|v| v.trim().parse::<i64>().ok()).unwrap_or(0);
                        }
                    }
                    self.raw_data.borrow_mut().drain(..idx + TWO_CRLF.len());
                    continue;
                }
            }
            break;
        }
    }
}

impl Default for StreamCore {
    fn default() -> Self {
        Self::new()
    }
}

impl IDisposable for StreamCore {
    fn dispose(&self) {
        self.listeners.dispose();
    }
}

// ---- NetworkDebugAdapter

/// `createConnection(connectionListener)`: abre o socket e chama `on_connected` quando a conexão estabelece.
pub trait INetworkConnector {
    fn create_connection(&self, on_connected: Box<dyn FnOnce()>) -> Rc<dyn INodeStream>;
}

/// `NetworkDebugAdapter` (e, pelos construtores, `SocketDebugAdapter` e `NamedPipeDebugAdapter`).
pub struct NetworkDebugAdapter {
    state: Rc<DebugAdapterState>,
    core: Rc<StreamCore>,
    connector: Rc<dyn INetworkConnector>,
    socket: RefCell<Option<Rc<dyn INodeStream>>>,
    me: Weak<NetworkDebugAdapter>,
}

impl NetworkDebugAdapter {
    pub fn new(connector: Rc<dyn INetworkConnector>) -> Rc<Self> {
        Rc::new_cyclic(|me| Self { state: DebugAdapterState::new(), core: Rc::new(StreamCore::new()), connector, socket: RefCell::new(None), me: me.clone() })
    }

    /// `new SocketDebugAdapter(adapterServer)`: o conector recebe a porta e o host (`127.0.0.1` por padrão).
    pub fn socket(adapter_server: IDebugAdapterServer, open: Rc<dyn Fn(&str, u16, Box<dyn FnOnce()>) -> Rc<dyn INodeStream>>) -> Rc<Self> {
        struct Connector {
            server: IDebugAdapterServer,
            open: Rc<dyn Fn(&str, u16, Box<dyn FnOnce()>) -> Rc<dyn INodeStream>>,
        }
        impl INetworkConnector for Connector {
            fn create_connection(&self, on_connected: Box<dyn FnOnce()>) -> Rc<dyn INodeStream> {
                let host = self.server.host.clone().filter(|h| !h.is_empty()).unwrap_or_else(|| "127.0.0.1".to_owned());
                (self.open)(&host, self.server.port as u16, on_connected)
            }
        }
        Self::new(Rc::new(Connector { server: adapter_server, open }))
    }

    /// `new NamedPipeDebugAdapter(adapterServer)`.
    pub fn named_pipe(adapter_server: IDebugAdapterNamedPipeServer, open: Rc<dyn Fn(&str, Box<dyn FnOnce()>) -> Rc<dyn INodeStream>>) -> Rc<Self> {
        struct Connector {
            server: IDebugAdapterNamedPipeServer,
            open: Rc<dyn Fn(&str, Box<dyn FnOnce()>) -> Rc<dyn INodeStream>>,
        }
        impl INetworkConnector for Connector {
            fn create_connection(&self, on_connected: Box<dyn FnOnce()>) -> Rc<dyn INodeStream> {
                (self.open)(&self.server.path, on_connected)
            }
        }
        Self::new(Rc::new(Connector { server: adapter_server, open }))
    }

    fn this(&self) -> Rc<NetworkDebugAdapter> {
        self.me.upgrade().expect("o adaptador está vivo")
    }
}

/// O `deliver` e o `on_error` do `connect` para um adaptador.
fn deliver_and_error<T: AbstractDebugAdapter>(adapter: &Rc<T>) -> (Rc<dyn Fn(ProtocolMessage)>, Rc<dyn Fn(Error)>) {
    let weak = Rc::downgrade(adapter);
    let state = adapter.state().clone();
    (
        Rc::new(move |message| {
            if let Some(adapter) = weak.upgrade() {
                adapter.accept_message(message);
            }
        }),
        Rc::new(move |error| state.on_error_emitter.fire(error)),
    )
}

impl IDisposable for NetworkDebugAdapter {
    fn dispose(&self) {
        self.core.dispose();
        self.state.dispose();
    }
}

impl AbstractDebugAdapter for NetworkDebugAdapter {
    fn state(&self) -> &Rc<DebugAdapterState> {
        &self.state
    }

    fn start_session(&self) -> Promise<()> {
        let this = self.this();
        let (promise, resolver): (Promise<()>, Resolver<()>) = Promise::new();
        let connected = Rc::new(std::cell::Cell::new(false));

        let on_connected = {
            let this = this.clone();
            let resolver = resolver.clone();
            let connected = connected.clone();
            Box::new(move || {
                let socket = this.socket.borrow().clone().expect("o socket");
                let (deliver, on_error) = deliver_and_error(&this);
                this.core.connect(&socket, socket.clone(), deliver, on_error);
                resolver.resolve(());
                connected.set(true);
            })
        };
        let socket = self.connector.create_connection(on_connected);
        *self.socket.borrow_mut() = Some(socket.clone());

        let close_this = this.clone();
        let close_resolver = resolver.clone();
        let close_connected = connected.clone();
        self.core_listeners().add(socket.on_close().listen(move |_| {
            if close_connected.get() {
                close_this.state.on_error_emitter.fire(Error::new("connection closed"));
            } else {
                close_resolver.reject(Error::new("connection closed"));
            }
        }));

        let error_this = this.clone();
        self.core_listeners().add(socket.on_error().listen(move |error| {
            if connected.get() {
                error_this.state.on_error_emitter.fire(error.clone());
            } else {
                resolver.reject(error.clone());
            }
        }));
        promise
    }

    fn stop_session(&self) -> Promise<()> {
        let this = self.this();
        async_fn(async move {
            this.cancel_pending_requests().await?;
            let socket = this.socket.borrow_mut().take();
            if let Some(socket) = socket {
                socket.end();
            }
            Ok(())
        })
    }

    fn send_message(&self, message: &ProtocolMessage) {
        self.core.send_message(message);
    }
}

impl NetworkDebugAdapter {
    fn core_listeners(&self) -> Rc<DisposableStore> {
        self.core.listeners.clone()
    }
}

// ---- ExecutableDebugAdapter

/// O processo do adaptador (`cp.ChildProcess`).
pub trait IAdapterProcess {
    /// `child.pid` (zero quando o processo não subiu).
    fn pid(&self) -> u32;
    fn stdout(&self) -> Rc<dyn INodeStream>;
    fn stdin(&self) -> Rc<dyn INodeStream>;
    /// `'error'`.
    fn on_error(&self) -> Event<Error>;
    /// `'exit'` (o código).
    fn on_exit(&self) -> Event<Option<i32>>;
    /// `kill('SIGTERM')` (ou `kill()`).
    fn kill(&self);
    /// `killTree(pid, true)`.
    fn kill_tree(&self) -> Promise<()>;
}

/// `cp.fork` (comando `node`) e `cp.spawn`.
pub trait IProcessLauncher {
    /// `cp.fork(args[0], args.slice(1), { env, cwd, silent: true })`.
    fn fork(&self, module: &str, args: &[String], env: &[(String, String)], cwd: Option<&str>) -> Result<Rc<dyn IAdapterProcess>, Error>;
    /// `cp.spawn(command, args, { env, cwd, windowsVerbatimArguments })`.
    fn spawn(&self, command: &str, args: &[String], env: &[(String, String)], cwd: Option<&str>, windows_verbatim_arguments: bool) -> Result<Rc<dyn IAdapterProcess>, Error>;
    /// `Promises.exists(path)`.
    fn exists(&self, path: &str) -> Promise<bool>;
    /// O ambiente do processo atual (`process.env`).
    fn environment(&self) -> Vec<(String, String)>;
}

/// `ExecutableDebugAdapter`.
pub struct ExecutableDebugAdapter {
    state: Rc<DebugAdapterState>,
    core: Rc<StreamCore>,
    adapter_executable: IDebugAdapterExecutable,
    debug_type: String,
    launcher: Rc<dyn IProcessLauncher>,
    server_process: RefCell<Option<Rc<dyn IAdapterProcess>>>,
    me: Weak<ExecutableDebugAdapter>,
}

impl ExecutableDebugAdapter {
    pub fn new(adapter_executable: IDebugAdapterExecutable, debug_type: &str, launcher: Rc<dyn IProcessLauncher>) -> Rc<Self> {
        Rc::new_cyclic(|me| Self {
            state: DebugAdapterState::new(),
            core: Rc::new(StreamCore::new()),
            adapter_executable,
            debug_type: debug_type.to_owned(),
            launcher,
            server_process: RefCell::new(None),
            me: me.clone(),
        })
    }

    fn this(&self) -> Rc<ExecutableDebugAdapter> {
        self.me.upgrade().expect("o adaptador está vivo")
    }

    fn start(&self) -> Result<(), Error> {
        let command = self.adapter_executable.command.clone();
        let args = self.adapter_executable.args.clone();
        let options = self.adapter_executable.options.clone().unwrap_or_default();

        let mut env = self.launcher.environment();
        if let Some(extra) = options.env.as_ref().filter(|e| !e.is_empty()) {
            for (key, value) in extra {
                match env.iter_mut().find(|(k, _)| k == key) {
                    Some(entry) => entry.1 = value.clone(),
                    None => env.push((key.clone(), value.clone())),
                }
            }
        }

        let process: Rc<dyn IAdapterProcess> = if command == "node" {
            if args.is_empty() {
                return Err(Error::new(localize("unableToLaunchDebugAdapterNoArgs", "Unable to launch debug adapter.", &[])));
            }
            let child = self.launcher.fork(&args[0], &args[1..], &env, options.cwd.as_deref())?;
            if child.pid() == 0 {
                return Err(Error::new(localize("unableToLaunchDebugAdapter", "Unable to launch debug adapter from '{0}'.", &[&args[0]])));
            }
            child
        } else {
            let mut spawn_command = command.clone();
            let mut spawn_args = args.clone();
            let mut verbatim = false;
            let lower = command.to_ascii_lowercase();
            if Platform::detect().is_windows && (lower.ends_with(".bat") || lower.ends_with(".cmd")) {
                // https://github.com/microsoft/vscode/issues/224184
                verbatim = true;
                spawn_command = env.iter().find(|(k, _)| k == "ComSpec").map(|(_, v)| v.clone()).unwrap_or_else(|| "cmd.exe".to_owned());
                spawn_args = prepare_windows_batch_command(&command, &args)?;
            }
            self.launcher.spawn(&spawn_command, &spawn_args, &env, options.cwd.as_deref(), verbatim)?
        };
        *self.server_process.borrow_mut() = Some(process.clone());

        let this = self.this();
        let listeners = self.core.listeners.clone();
        let error_state = self.state.clone();
        listeners.add(process.on_error().listen(move |err| error_state.on_error_emitter.fire(err.clone())));
        let exit_state = self.state.clone();
        listeners.add(process.on_exit().listen(move |code| exit_state.on_exit_emitter.fire(*code)));
        let close_state = self.state.clone();
        listeners.add(process.stdout().on_close().listen(move |_| close_state.on_error_emitter.fire(Error::new("read error"))));
        let stdout_error_state = self.state.clone();
        listeners.add(process.stdout().on_error().listen(move |error| stdout_error_state.on_error_emitter.fire(error.clone())));
        let stdin_error_state = self.state.clone();
        listeners.add(process.stdin().on_error().listen(move |error| stdin_error_state.on_error_emitter.fire(error.clone())));

        // finally connect to the DA
        let (deliver, on_error) = deliver_and_error(&this);
        self.core.connect(&process.stdout(), process.stdin(), deliver, on_error);
        Ok(())
    }

    // ---- platformAdapterExecutable

    fn extract(platform_contribution: &Value, extension_folder_path: &str) -> Option<Value> {
        if platform_contribution.is_null() || platform_contribution.as_bool() == Some(false) {
            return None;
        }

        let posix_join = |a: &str, b: &str| PathModule::native(&ProcessEnvironment).join(&[a, b]);
        let mut result = serde_json::Map::new();
        if let Some(runtime) = platform_contribution.get("runtime").and_then(Value::as_str).filter(|r| !r.is_empty()) {
            if runtime.starts_with("./") {
                result.insert("runtime".into(), Value::from(posix_join(extension_folder_path, runtime)));
            } else {
                result.insert("runtime".into(), Value::from(runtime));
            }
        }
        if let Some(runtime_args) = platform_contribution.get("runtimeArgs").filter(|a| !a.is_null()) {
            result.insert("runtimeArgs".into(), runtime_args.clone());
        }
        if let Some(program) = platform_contribution.get("program").and_then(Value::as_str).filter(|p| !p.is_empty()) {
            if !PathModule::native(&ProcessEnvironment).is_absolute(program) {
                result.insert("program".into(), Value::from(posix_join(extension_folder_path, program)));
            } else {
                result.insert("program".into(), Value::from(program));
            }
        }
        if let Some(args) = platform_contribution.get("args").filter(|a| !a.is_null()) {
            result.insert("args".into(), args.clone());
        }

        for key in ["win", "winx86", "windows", "osx", "linux"] {
            if let Some(section) = platform_contribution.get(key).filter(|s| s.is_object())
                && let Some(extracted) = Self::extract(section, extension_folder_path)
            {
                result.insert(key.into(), extracted);
            }
        }
        Some(Value::Object(result))
    }

    /// `ExecutableDebugAdapter.platformAdapterExecutable(extensionDescriptions, debugType)`.
    pub fn platform_adapter_executable(extension_descriptions: &[ExtensionDescription], debug_type: &str) -> Option<IDebugAdapterExecutable> {
        let mut result = Value::Object(serde_json::Map::new());
        let debug_type = debug_type.to_lowercase();

        // merge all contributions into one
        for ed in extension_descriptions {
            let Some(contributes) = &ed.manifest.contributes else {
                continue;
            };
            for dbg in contributes.debuggers.clone().unwrap_or_default() {
                let dbg = serde_json::to_value(&dbg).unwrap_or(Value::Null);
                let matches = dbg.get("type").and_then(Value::as_str).is_some_and(|t| t.to_lowercase() == debug_type);
                if matches {
                    // extract relevant attributes and make them absolute where needed
                    let folder = URI::parse(&ed.extension_location, false).fs_path(je_platform::resources::host_platform()).to_string_lossy();
                    let extracted = Self::extract(&dbg, &folder);

                    // merge
                    if let Some(extracted) = extracted {
                        mixin(&mut result, &extracted, ed.is_builtin);
                    }
                }
            }
        }

        // select the right platform
        let platform = Platform::detect();
        let section = |key: &str| result.get(key).filter(|s| s.is_object()).cloned();
        let platform_info: Option<Value> = if platform.is_windows && std::env::var_os("PROCESSOR_ARCHITEW6432").is_none() {
            section("winx86").or_else(|| section("win")).or_else(|| section("windows"))
        } else if platform.is_windows {
            section("win").or_else(|| section("windows"))
        } else if platform.is_macintosh {
            section("osx")
        } else if platform.is_linux {
            section("linux")
        } else {
            None
        };
        let platform_info = platform_info.unwrap_or_else(|| result.clone());

        // these are the relevant attributes
        let pick = |key: &str| platform_info.get(key).filter(|v| truthy(v)).or_else(|| result.get(key).filter(|v| truthy(v))).cloned();
        let program = pick("program");
        let args: Option<Vec<String>> = pick("args").and_then(|a| a.as_array().map(|a| a.iter().filter_map(|v| v.as_str().map(str::to_owned)).collect()));
        let runtime = pick("runtime").and_then(|r| r.as_str().map(str::to_owned));
        let runtime_args: Option<Vec<String>> = pick("runtimeArgs").and_then(|a| a.as_array().map(|a| a.iter().filter_map(|v| v.as_str().map(str::to_owned)).collect()));

        if let Some(runtime) = runtime {
            let mut command_args = runtime_args.unwrap_or_default();
            if let Some(program) = program.as_ref().and_then(Value::as_str) {
                command_args.push(program.to_owned());
            }
            command_args.extend(args.unwrap_or_default());
            return Some(IDebugAdapterExecutable { command: runtime, args: command_args, options: None });
        } else if let Some(program) = program.and_then(|p| p.as_str().map(str::to_owned)) {
            return Some(IDebugAdapterExecutable { command: program, args: args.unwrap_or_default(), options: None });
        }

        // nothing found
        None
    }
}

fn truthy(value: &Value) -> bool {
    !matches!(value, Value::Null | Value::Bool(false)) && value.as_str().is_none_or(|s| !s.is_empty())
}

/// `objects.mixin(destination, source, overwrite)`.
fn mixin(destination: &mut Value, source: &Value, overwrite: bool) {
    let (Some(destination), Some(source)) = (destination.as_object_mut(), source.as_object()) else {
        return;
    };
    for (key, value) in source {
        if key == "__proto__" {
            continue;
        }
        match destination.get_mut(key) {
            Some(existing) if existing.is_object() && value.is_object() => mixin(existing, value, overwrite),
            Some(existing) => {
                if overwrite {
                    *existing = value.clone();
                }
            }
            None => {
                destination.insert(key.clone(), value.clone());
            }
        }
    }
}

impl IDisposable for ExecutableDebugAdapter {
    fn dispose(&self) {
        self.core.dispose();
        self.state.dispose();
    }
}

impl AbstractDebugAdapter for ExecutableDebugAdapter {
    fn state(&self) -> &Rc<DebugAdapterState> {
        &self.state
    }

    fn start_session(&self) -> Promise<()> {
        let this = self.this();
        async_fn(async move {
            let command = this.adapter_executable.command.clone();
            let precheck: Result<(), Error> = if command.is_empty() {
                Err(Error::new(localize(
                    "debugAdapterCannotDetermineExecutable",
                    "Cannot determine executable for debug adapter '{0}'.",
                    &[&this.debug_type],
                )))
            } else if PathModule::native(&ProcessEnvironment).is_absolute(&command) {
                // verify executables asynchronously
                match this.launcher.exists(&command).await {
                    Ok(true) => Ok(()),
                    Ok(false) => Err(Error::new(localize("debugAdapterBinNotFound", "Debug adapter executable '{0}' does not exist.", &[&command]))),
                    Err(error) => Err(error),
                }
            } else {
                Ok(())
            };
            if let Err(error) = precheck.and_then(|()| this.start()) {
                this.state.on_error_emitter.fire(error);
            }
            Ok(())
        })
    }

    fn stop_session(&self) -> Promise<()> {
        let this = self.this();
        async_fn(async move {
            let process = this.server_process.borrow().clone();
            let Some(process) = process else {
                return Ok(());
            };

            // when killing a process in windows its child processes are *not* killed but become root processes. Therefore we use TASKKILL.EXE
            this.cancel_pending_requests().await?;
            if Platform::detect().is_windows {
                if process.kill_tree().await.is_err() {
                    process.kill();
                }
            } else {
                process.kill();
            }
            Ok(())
        })
    }

    fn send_message(&self, message: &ProtocolMessage) {
        self.core.send_message(message);
    }
}
