//! Porte de `src/vs/base/parts/ipc/node/ipc.net.ts` (a parte sem E/S): o `NodeSocket` sobre um fluxo de bytes
//! com a forma de um `net.Socket`, e os nomes dos sockets de IPC (`createRandomIPCHandle`,
//! `createStaticIPCHandle`).
//!
//! O `net.Socket` do Node é um `EventEmitter`; [`INodeStream`] é o subconjunto que o `NodeSocket` usa, com um
//! evento por nome (`data`, `close`, `end`, `error`, `timeout`, `drain`). Quem fala com o sistema (um
//! `TcpStream` ou `UnixStream` ligado ao laço de eventos) implementa [`INodeStream`]; isso fica na camada de
//! plataforma, e o `Server`, o `serve` e o `connect` do original, que abrem sockets de verdade, ficam lá também.
//! O `upgradeToISocket` (HTTP) e o `WebSocketNodeSocket` (que depende do `zlib` do Node para a compressão
//! `permessage-deflate`) não foram portados.

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use sha2::{Digest, Sha256};

use crate::buffer::VSBuffer;
use crate::errors::{self, Error};
use crate::event::Event;
use crate::lifecycle::{DisposableRef, IDisposable};
use crate::parts::ipc::net::{ISocket, SocketCloseEvent};
use crate::path::{PathModule, ProcessEnvironment};
use crate::platform::{OperatingSystem, Platform};
use crate::promise::Promise;
use crate::runtime::{self, TimerId};
use crate::uuid::generate_uuid;

/// Maximum time to wait for a 'close' event to fire after the socket stream ends. For unix domain sockets, the
/// close event may not fire consistently due to what appears to be a Node.js bug.
///
/// See <https://github.com/microsoft/vscode/issues/211462#issuecomment-2155471996>.
pub const SOCKET_END_TIMEOUT_MS: u64 = 30_000;

/// O subconjunto de `net.Socket` que o [`NodeSocket`] usa.
pub trait INodeStream {
    /// `'data'`.
    fn on_data(&self) -> Event<VSBuffer>;
    /// `'close'`, com `hadError`.
    fn on_close(&self) -> Event<bool>;
    /// `'end'`.
    fn on_end(&self) -> Event<()>;
    /// `'error'`; o `code` do erro (`EPIPE`) vai na propriedade `code`.
    fn on_error(&self) -> Event<Error>;
    /// `'timeout'`.
    fn on_timeout(&self) -> Event<()>;
    /// `'drain'`.
    fn on_drain(&self) -> Event<()>;
    /// `write(buffer, callback)`: o erro de escrita, síncrono ou entregue ao callback, volta aqui.
    fn write(&self, buffer: VSBuffer) -> Result<(), Error>;
    fn end(&self);
    fn destroy(&self);
    /// `destroyed`.
    fn destroyed(&self) -> bool;
    /// `bufferSize`: os bytes ainda não escritos.
    fn buffer_size(&self) -> usize;
}

/// `NodeSocket`.
pub struct NodeSocket {
    pub debug_label: String,
    socket: Rc<dyn INodeStream>,
    listeners: RefCell<Vec<DisposableRef>>,
    end_timeout_handle: Rc<Cell<Option<TimerId>>>,
    can_write: Rc<Cell<bool>>,
}

/// `err.code === 'EPIPE'`: o `EPIPE` num momento errado pode derrubar o processo, e o socket avisa o `close`
/// logo depois de qualquer jeito.
fn is_epipe(err: &Error) -> bool {
    err.prop("code") == Some("EPIPE")
}

impl NodeSocket {
    pub fn new(socket: Rc<dyn INodeStream>, debug_label: &str) -> Rc<Self> {
        let can_write = Rc::new(Cell::new(true));
        let end_timeout_handle: Rc<Cell<Option<TimerId>>> = Rc::default();

        let error_listener = socket.on_error().listen(|err| {
            if is_epipe(err) {
                return;
            }
            errors::on_unexpected_error(err);
        });

        let close_listener = socket.on_close().listen({
            let (can_write, end_timeout_handle) = (can_write.clone(), end_timeout_handle.clone());
            move |_| {
                can_write.set(false);
                if let Some(handle) = end_timeout_handle.take() {
                    runtime::clear_timeout(handle);
                }
            }
        });

        let end_listener = socket.on_end().listen({
            let (can_write, end_timeout_handle, socket) = (can_write.clone(), end_timeout_handle.clone(), socket.clone());
            move |()| {
                can_write.set(false);
                let socket = socket.clone();
                end_timeout_handle.set(Some(runtime::set_timeout(move || socket.destroy(), SOCKET_END_TIMEOUT_MS)));
            }
        });

        Rc::new(Self { debug_label: debug_label.to_owned(), socket, listeners: RefCell::new(vec![error_listener, close_listener, end_listener]), end_timeout_handle, can_write })
    }

    /// `dispose(destroySocket)`.
    pub fn dispose_with(&self, destroy_socket: bool) {
        if let Some(handle) = self.end_timeout_handle.take() {
            runtime::clear_timeout(handle);
        }
        for listener in self.listeners.borrow_mut().drain(..) {
            listener.dispose();
        }
        if destroy_socket {
            self.socket.destroy();
        }
    }

    /// `socket`.
    pub fn socket(&self) -> Rc<dyn INodeStream> {
        self.socket.clone()
    }
}

impl IDisposable for NodeSocket {
    fn dispose(&self) {
        self.dispose_with(true);
    }
}

impl ISocket for NodeSocket {
    fn on_data(&self) -> Event<VSBuffer> {
        self.socket.on_data()
    }

    fn on_close(&self) -> Event<Option<SocketCloseEvent>> {
        self.socket.on_close().map(|had_error| Some(SocketCloseEvent::Node { had_error: *had_error, error: None }), None)
    }

    fn on_end(&self) -> Event<()> {
        self.socket.on_end()
    }

    fn write(&self, buffer: VSBuffer) {
        // return early if socket has been destroyed in the meantime
        if self.socket.destroyed() || !self.can_write.get() {
            return;
        }

        // we ignore the returned value from `write` because we would have to cached the data anyways and nodejs is
        // already doing that for us.
        if let Err(err) = self.socket.write(buffer) {
            if is_epipe(&err) {
                return;
            }
            errors::on_unexpected_error(&err);
        }
    }

    fn end(&self) {
        self.socket.end();
    }

    fn drain(&self) -> Promise<()> {
        Promise::with_executor(|resolver| {
            if self.socket.buffer_size() == 0 {
                resolver.resolve(());
                return;
            }
            // `finished`: qualquer um dos eventos termina a espera, e os ouvintes saem juntos.
            let listeners: Rc<RefCell<Vec<DisposableRef>>> = Rc::default();
            let finished = {
                let (listeners, resolver) = (listeners.clone(), resolver.clone());
                Rc::new(move || {
                    for listener in listeners.borrow_mut().drain(..) {
                        listener.dispose();
                    }
                    resolver.resolve(());
                })
            };
            let mut added = Vec::new();
            added.push(self.socket.on_close().listen({
                let finished = finished.clone();
                move |_| finished()
            }));
            added.push(self.socket.on_end().listen({
                let finished = finished.clone();
                move |()| finished()
            }));
            added.push(self.socket.on_error().listen({
                let finished = finished.clone();
                move |_| finished()
            }));
            added.push(self.socket.on_timeout().listen({
                let finished = finished.clone();
                move |()| finished()
            }));
            added.push(self.socket.on_drain().listen(move |()| finished()));
            *listeners.borrow_mut() = added;
        })
    }
}

/// O que `createRandomIPCHandle` e `createStaticIPCHandle` leem do processo: o sistema, `XDG_RUNTIME_DIR`
/// (lido cedo no original, antes de alguém poder mudá-lo), `os.tmpdir()` e `VSCODE_PORTABLE`.
#[derive(Clone, Debug)]
pub struct IpcHandleEnvironment {
    pub platform: Platform,
    pub xdg_runtime_dir: Option<String>,
    pub tmpdir: String,
    pub vscode_portable: Option<String>,
}

impl IpcHandleEnvironment {
    pub fn from_process() -> Self {
        let env = |name: &str| std::env::var(name).ok();
        Self { platform: Platform::detect(), xdg_runtime_dir: env("XDG_RUNTIME_DIR"), tmpdir: std::env::temp_dir().to_string_lossy().into_owned(), vscode_portable: env("VSCODE_PORTABLE") }
    }

    fn is_windows(&self) -> bool {
        self.platform.os == OperatingSystem::Windows
    }

    fn is_darwin(&self) -> bool {
        self.platform.os == OperatingSystem::Macintosh
    }

    /// `safeIpcPathLengths[platform]`.
    fn safe_path_length(&self) -> Option<usize> {
        match self.platform.os {
            OperatingSystem::Linux => Some(107),
            OperatingSystem::Macintosh => Some(103),
            OperatingSystem::Windows => None,
        }
    }

    fn path(&self) -> PathModule<'static> {
        static PROCESS: ProcessEnvironment = ProcessEnvironment;
        if self.is_windows() { PathModule::win32(&PROCESS) } else { PathModule::posix(&PROCESS) }
    }
}

/// `createRandomIPCHandle`.
pub fn create_random_ipc_handle(env: &IpcHandleEnvironment) -> String {
    let random_suffix = generate_uuid();

    // Windows: use named pipe
    if env.is_windows() {
        return format!("\\\\.\\pipe\\vscode-ipc-{random_suffix}-sock");
    }

    // Mac & Unix: Use socket file. Unix: Prefer XDG_RUNTIME_DIR over user data path
    let base_path = match &env.xdg_runtime_dir {
        Some(dir) if !env.is_darwin() && !dir.is_empty() => dir.clone(),
        _ => env.tmpdir.clone(),
    };

    // As of Node.js 24, socket paths that exceed the platform limit cause an `EINVAL` error at bind time instead of
    // being silently truncated. The suffix only needs to be unique, so trim it (while keeping enough entropy) to
    // make the path fit within the limit.
    let path = env.path();
    let mut suffix = random_suffix;
    if let Some(limit) = env.safe_path_length() {
        let used = path.join(&[&base_path, "vscode-ipc-.sock"]).encode_utf16().count();
        let available = (limit - 1).saturating_sub(used);
        if available < suffix.len() {
            suffix.truncate(available);
        }
    }

    path.join(&[&base_path, &format!("vscode-ipc-{suffix}.sock")])
}

/// `createStaticIPCHandle`. Avisa (stderr) se o caminho passa do limite da plataforma, como o original.
pub fn create_static_ipc_handle(env: &IpcHandleEnvironment, directory_path: &str, type_: &str, version: &str) -> String {
    let scope = hex(&Sha256::digest(directory_path.as_bytes()));
    let scope_for_socket = &scope[..8];

    // Windows: use named pipe
    if env.is_windows() {
        return format!("\\\\.\\pipe\\{scope_for_socket}-{version}-{type_}-sock");
    }

    // Mac & Unix: Use socket file. Unix: Prefer XDG_RUNTIME_DIR over user data path, unless portable. Trim the
    // version and type values for the socket to prevent too large file names causing issues:
    // https://unix.stackexchange.com/q/367008
    let version_for_socket: String = version.chars().take(4).collect();
    let type_for_socket: String = type_.chars().take(6).collect();

    let path = env.path();
    let portable = env.vscode_portable.as_deref().is_some_and(|p| !p.is_empty());
    let result = match &env.xdg_runtime_dir {
        Some(dir) if !env.is_darwin() && !dir.is_empty() && !portable => path.join(&[dir, &format!("vscode-{scope_for_socket}-{version_for_socket}-{type_for_socket}.sock")]),
        _ => path.join(&[directory_path, &format!("{version_for_socket}-{type_for_socket}.sock")]),
    };

    // Validate length. Unlike `createRandomIPCHandle`, the path here must be derived deterministically from
    // `directoryPath` so that the server and its clients agree on the same socket. There is no random component to
    // trim, so an over-long `--user-data-dir` can still produce a path that exceeds the platform limit.
    validate_ipc_handle_length(env, &result);

    result
}

fn validate_ipc_handle_length(env: &IpcHandleEnvironment, handle: &str) {
    if let Some(limit) = env.safe_path_length()
        && handle.encode_utf16().count() >= limit
    {
        // https://nodejs.org/api/net.html#net_identifying_paths_for_ipc_connections
        eprintln!("WARNING: IPC handle \"{handle}\" is longer than {limit} chars, try a shorter --user-data-dir");
    }
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}
