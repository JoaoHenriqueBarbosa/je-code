//! O `cp.execFile` real do [`LocalGitService`](super::node_local_git_service::LocalGitService): inicia o programa com
//! `std::process::Command` e entrega o `callback(err, stdout, stderr)` no laço de eventos do `je-base`.
//!
//! Decisões de tradução (o `execFile` do Node, com `encoding: 'utf8'`):
//! - **Saída.** Duas threads leem o `stdout` e o `stderr` para buffers (um filho que escreve mais que o pipe aguenta
//!   travaria se ninguém lesse). Um temporizador do laço confere o `try_wait` do filho e, depois que ele saiu e as duas
//!   leituras terminaram (o `close` do Node), chama o `callback` uma única vez. O texto é UTF-8 com substituição.
//! - **`maxBuffer`.** O padrão do Node é 1 MiB por fluxo: ao passar disso o filho é morto e o erro tem
//!   `code = ERR_CHILD_PROCESS_STDIO_MAXBUFFER` e a saída cortada no limite.
//! - **Erro.** Saída diferente de zero: mensagem `Command failed: <comando>\n<stderr>` e `code` com o código de saída
//!   (sem `code` quando o filho morreu por sinal, como o `code: null` do Node). Falha ao iniciar: `spawn <programa> ENOENT`
//!   com `code = ENOENT` (ou `EACCES`), entregue por microtarefa, depois de `exec_file` voltar.
//! - **`stdin`** vai para o nada (o Node deixa um pipe aberto e sem uso): o `git` nunca espera entrada interativa.

use std::cell::{Cell, RefCell};
use std::io::Read;
use std::process::{Child, Command, ExitStatus, Stdio};
use std::rc::Rc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

use je_base::errors::Error;
use je_base::runtime;

use super::node_local_git_service::{ChildProcess, ExecFile, ExecFileCallback, ExecFileOptions};

/// De quanto em quanto tempo o laço confere se o filho terminou.
const POLL_INTERVAL_MS: u64 = 10;

/// O `maxBuffer` padrão do `execFile`: `1024 * 1024`.
const MAX_BUFFER: usize = 1024 * 1024;

/// O erro que o Node entrega quando o programa não pôde ser iniciado: `spawn <programa> ENOENT` com `code = ENOENT` (ou
/// `EACCES`), e `spawn <programa>: <causa>` sem `code` nos demais casos. Compartilhado com o `spawn` do túnel remoto.
pub(crate) fn spawn_failure(command: &str, error: &std::io::Error) -> Error {
    let code = match error.kind() {
        std::io::ErrorKind::NotFound => Some("ENOENT"),
        std::io::ErrorKind::PermissionDenied => Some("EACCES"),
        _ => None,
    };
    match code {
        Some(code) => Error::new(format!("spawn {command} {code}")).with_prop("code", code),
        None => Error::new(format!("spawn {command}: {error}")),
    }
}

/// O `cp.execFile` real.
#[derive(Clone, Copy, Debug, Default)]
pub struct ProcessExecFile;

/// Um filho que nunca rodou (o programa não pôde ser iniciado): `kill` não faz nada.
struct NotStarted;

impl ChildProcess for NotStarted {
    fn kill(&self) {}
}

impl ExecFile for ProcessExecFile {
    fn exec_file(&self, command: &str, args: &[String], options: &ExecFileOptions, callback: ExecFileCallback) -> Rc<dyn ChildProcess> {
        let mut process = Command::new(command);
        process.args(args);
        if let Some(cwd) = options.cwd.as_deref().filter(|cwd| !cwd.is_empty()) {
            process.current_dir(cwd);
        }
        if let Some(env) = &options.env {
            process.env_clear().envs(env);
        }
        process.stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::piped());
        match process.spawn() {
            Ok(child) => RunningProcess::start(child, format!("{command} {}", args.join(" ")), callback),
            Err(error) => {
                let spawn_error = spawn_failure(command, &error);
                // O Node entrega o erro depois de quem chamou registrar o filho.
                runtime::queue_microtask(move || callback(Some(spawn_error), String::new(), String::new()));
                Rc::new(NotStarted)
            }
        }
    }
}

/// O filho em andamento: guarda os buffers e o `callback` até o fim.
struct RunningProcess {
    child: RefCell<Child>,
    command_line: String,
    stdout: Arc<Mutex<Vec<u8>>>,
    stderr: Arc<Mutex<Vec<u8>>>,
    readers_alive: Arc<AtomicUsize>,
    exit: RefCell<Option<ExitStatus>>,
    killed: Cell<bool>,
    /// O fluxo que passou do `maxBuffer`.
    exceeded: Cell<Option<&'static str>>,
    callback: RefCell<Option<ExecFileCallback>>,
}

/// Lê o fluxo até o fim para o buffer e avisa que acabou.
fn read_stream(mut stream: impl Read + Send + 'static, buffer: Arc<Mutex<Vec<u8>>>, readers_alive: Arc<AtomicUsize>) {
    std::thread::spawn(move || {
        let mut chunk = [0u8; 8192];
        while let Ok(read) = stream.read(&mut chunk) {
            if read == 0 {
                break;
            }
            if let Ok(mut buffer) = buffer.lock() {
                buffer.extend_from_slice(&chunk[..read]);
            }
        }
        readers_alive.fetch_sub(1, Ordering::SeqCst);
    });
}

/// O conteúdo do buffer como texto.
fn buffer_text(buffer: &Mutex<Vec<u8>>, limit: Option<usize>) -> String {
    buffer
        .lock()
        .map(|bytes| String::from_utf8_lossy(&bytes[..limit.map_or(bytes.len(), |limit| limit.min(bytes.len()))]).into_owned())
        .unwrap_or_default()
}

impl RunningProcess {
    fn start(mut child: Child, command_line: String, callback: ExecFileCallback) -> Rc<dyn ChildProcess> {
        let stdout = Arc::new(Mutex::new(Vec::new()));
        let stderr = Arc::new(Mutex::new(Vec::new()));
        let readers_alive = Arc::new(AtomicUsize::new(0));
        if let Some(stream) = child.stdout.take() {
            readers_alive.fetch_add(1, Ordering::SeqCst);
            read_stream(stream, stdout.clone(), readers_alive.clone());
        }
        if let Some(stream) = child.stderr.take() {
            readers_alive.fetch_add(1, Ordering::SeqCst);
            read_stream(stream, stderr.clone(), readers_alive.clone());
        }
        let this = Rc::new(Self {
            child: RefCell::new(child),
            command_line,
            stdout,
            stderr,
            readers_alive,
            exit: RefCell::new(None),
            killed: Cell::new(false),
            exceeded: Cell::new(None),
            callback: RefCell::new(Some(callback)),
        });
        Self::schedule_poll(&this);
        this
    }

    fn schedule_poll(this: &Rc<Self>) {
        let this = this.clone();
        runtime::set_timeout(move || Self::poll(&this), POLL_INTERVAL_MS);
    }

    fn poll(this: &Rc<Self>) {
        if this.callback.borrow().is_none() {
            return;
        }
        this.enforce_max_buffer();
        if this.exit.borrow().is_none() {
            let status = this.child.borrow_mut().try_wait();
            match status {
                Ok(Some(status)) => *this.exit.borrow_mut() = Some(status),
                Ok(None) => {}
                Err(error) => return this.finish(Some(Error::new(error.to_string()))),
            }
        }
        if this.exit.borrow().is_some() && this.readers_alive.load(Ordering::SeqCst) == 0 {
            this.finish(None);
        } else {
            Self::schedule_poll(this);
        }
    }

    /// Mata o filho que escreveu mais que o `maxBuffer` num fluxo.
    fn enforce_max_buffer(&self) {
        if self.exceeded.get().is_some() {
            return;
        }
        for (name, buffer) in [("stdout", &self.stdout), ("stderr", &self.stderr)] {
            if buffer.lock().is_ok_and(|bytes| bytes.len() > MAX_BUFFER) {
                self.exceeded.set(Some(name));
                self.kill();
                return;
            }
        }
    }

    /// Entrega o resultado. `failure` é o erro de espera do próprio processo (`try_wait`).
    fn finish(&self, failure: Option<Error>) {
        let Some(callback) = self.callback.borrow_mut().take() else {
            return;
        };
        let limit = self.exceeded.get().map(|_| MAX_BUFFER);
        let (stdout, stderr) = (buffer_text(&self.stdout, limit), buffer_text(&self.stderr, limit));
        let error = failure.or_else(|| {
            if let Some(name) = self.exceeded.get() {
                return Some(Error::new(format!("{name} maxBuffer length exceeded")).with_prop("code", "ERR_CHILD_PROCESS_STDIO_MAXBUFFER"));
            }
            let status = (*self.exit.borrow())?;
            if status.success() {
                return None;
            }
            let error = Error::new(format!("Command failed: {}\n{stderr}", self.command_line)).with_prop("killed", self.killed.get());
            Some(match status.code() {
                Some(code) => error.with_prop("code", code),
                None => error,
            })
        });
        callback(error, stdout, stderr);
    }
}

impl ChildProcess for RunningProcess {
    fn kill(&self) {
        self.killed.set(true);
        // O filho pode já ter saído; `kill` então falha e não há o que fazer.
        drop(self.child.borrow_mut().kill());
    }
}
