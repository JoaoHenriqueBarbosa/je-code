//! O pty cru: o papel do `node-pty` sob o `TerminalProcess` do VS Code (`spawn`, `onData`, `onExit`, `write`, `resize`,
//! `kill`, `pause`, `resume` e `process`). Diferente do [`Terminal`](crate::Terminal), que entrega os bytes direto ao
//! emulador do Alacritty, aqui os bytes saem crus, para o chamador repassá-los como texto (o `onProcessData`).
//!
//! O pty é o `tty::Pty` do Alacritty. Uma thread dedicada lê o filho e enfileira os eventos; quem consome chama
//! [`RawPty::drain_events`] no próprio laço de eventos, de modo que nenhuma chamada de retorno atravessa threads.
//!
//! No Unix a thread lê o lado mestre em modo bloqueante. No Windows (ConPTY) o `Pty` do Alacritty entrega a saída por um
//! pipe interno que só tem leitura sem bloqueio (`try_read`), então a thread consulta o pipe a cada 2 ms e
//! detecta o fim do filho pelo `ChildExitWatcher` (o ConPTY não fecha a saída quando o filho sai).
//!
//! Divergências do `node-pty`:
//! - O ambiente de [`PtySpawn::env`] soma-se ao do processo pai (o `tty::Options` do Alacritty não limpa o ambiente);
//!   o `node-pty` usa só o `env` dado. Quem passa o `process.env` inteiro (o caso do `TerminalProcess`) não vê diferença.
//! - No Unix, `process` lê o nome do grupo de processos em primeiro plano em `/proc/<pgrp>/comm`, só no Linux. No Windows
//!   `process` é o nome do executável (o `name` do `node-pty`, que o `TerminalProcess` calcula do mesmo `file`); o título
//!   de verdade vem do `WindowsShellHelper`.
//! - No Windows `kill` com sinal devolve erro ("Signals not supported on windows."), como o `node-pty`; sem sinal encerra o
//!   processo filho (`TerminateProcess`). `cols` ou `rows` zero viram 80 e 24 (`DEFAULT_COLS` e `DEFAULT_ROWS`).
//! - O Alacritty não expõe o `pixelSize` do ConPTY: no Windows `resize` ignora os pixels.

use std::collections::{HashMap, VecDeque};
use std::path::PathBuf;
use std::sync::{Arc, Condvar, Mutex, PoisonError};

/// O que o `spawn` do `node-pty` recebe.
#[derive(Clone, Debug)]
pub struct PtySpawn {
    /// O programa (`file`).
    pub program: String,
    /// Os argumentos (`args`).
    pub args: Vec<String>,
    /// O diretório de trabalho (`cwd`).
    pub cwd: Option<PathBuf>,
    /// O ambiente (`env`).
    pub env: HashMap<String, String>,
    /// As colunas (`cols`).
    pub cols: u16,
    /// As linhas (`rows`).
    pub rows: u16,
}

/// O que a thread de leitura entrega ao consumidor, na ordem.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PtyEvent {
    /// `onData`, em bytes (o chamador decodifica o UTF-8).
    Data(Vec<u8>),
    /// `onExit`, depois de todos os `Data`. O `None` é a morte por sinal.
    Exit(Option<i32>),
}

#[cfg(unix)]
pub use unix::RawPty;
#[cfg(windows)]
pub use windows::RawPty;
#[cfg(not(any(unix, windows)))]
pub use unsupported::RawPty;

/// `paused` e `closed` da thread de leitura.
#[derive(Default)]
struct Gate {
    paused: bool,
    closed: bool,
}

fn lock<T>(mutex: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}

/// A fila de eventos e o portão de pausa que a thread de leitura e o `RawPty` compartilham (igual em Unix e Windows).
#[derive(Clone, Default)]
struct Channel {
    events: Arc<Mutex<VecDeque<PtyEvent>>>,
    gate: Arc<(Mutex<Gate>, Condvar)>,
}

impl Channel {
    fn push(&self, event: PtyEvent) {
        lock(&self.events).push_back(event);
    }

    fn drain_events(&self) -> Vec<PtyEvent> {
        lock(&self.events).drain(..).collect()
    }

    /// `pause()`: a thread de leitura para.
    fn pause(&self) {
        lock(&self.gate.0).paused = true;
    }

    /// `resume()`.
    fn resume(&self) {
        lock(&self.gate.0).paused = false;
        self.gate.1.notify_all();
    }

    /// Descarte do pty: a thread de leitura sai.
    fn close(&self) {
        lock(&self.gate.0).closed = true;
        self.gate.1.notify_all();
    }

    /// Bloqueia enquanto estiver em pausa; devolve `true` se o pty foi descartado (a thread deve sair).
    fn wait_while_paused(&self) -> bool {
        let (state, wake) = &*self.gate;
        let mut state = lock(state);
        while state.paused && !state.closed {
            state = wake.wait(state).unwrap_or_else(PoisonError::into_inner);
        }
        state.closed
    }
}

/// O `tty::Options` do Alacritty para um [`PtySpawn`].
#[cfg(any(unix, windows))]
fn options(spec: &PtySpawn) -> alacritty_terminal::tty::Options {
    use alacritty_terminal::tty::{Options, Shell};

    // O ConPTY monta uma linha de comando única: o `node-pty` põe o `file` entre aspas se tiver espaço.
    #[cfg(windows)]
    let program = if spec.program.contains([' ', '\t']) && !spec.program.starts_with('"') { format!("\"{}\"", spec.program) } else { spec.program.clone() };
    #[cfg(not(windows))]
    let program = spec.program.clone();

    // No Windows o `Options` tem o campo extra `escape_args`: o `..Default` é necessário lá.
    #[allow(clippy::needless_update)]
    let options = Options {
        shell: Some(Shell::new(program, spec.args.clone())),
        working_directory: spec.cwd.clone(),
        drain_on_exit: false,
        env: spec.env.clone(),
        ..Options::default()
    };
    // O `node-pty` escapa cada argumento do array (`argsToCommandLine`).
    #[cfg(windows)]
    let options = Options { escape_args: true, ..options };
    options
}

#[cfg(unix)]
mod unix {
    use std::fs::File;
    use std::io::{Read, Write};
    use std::sync::{Arc, Mutex};
    use std::time::{Duration, Instant};

    use alacritty_terminal::event::{OnResize, WindowSize};
    use alacritty_terminal::tty::{self, Pty};

    use super::{Channel, PtyEvent, PtySpawn, lock, options};

    /// Quanto esperar o filho sair depois de o pty fechar, antes de dar o código por desconhecido.
    const EXIT_WAIT: Duration = Duration::from_secs(2);
    const EXIT_POLL: Duration = Duration::from_millis(5);

    /// O `IPty` do `node-pty`.
    pub struct RawPty {
        pid: u32,
        pty: Arc<Mutex<Pty>>,
        writer: Mutex<File>,
        channel: Channel,
    }

    impl RawPty {
        /// `spawn(file, args, options)`.
        pub fn spawn(spec: &PtySpawn) -> std::io::Result<Self> {
            let size = WindowSize { num_lines: spec.rows, num_cols: spec.cols, cell_width: 0, cell_height: 0 };
            let pty = tty::new(&options(spec), size, 0)?;
            pty.set_blocking()?;
            let mut reader = pty.file().try_clone()?;
            let writer = pty.file().try_clone()?;
            let pid = pty.child().id();
            let pty = Arc::new(Mutex::new(pty));
            let channel = Channel::default();

            let thread_channel = channel.clone();
            let thread_pty = pty.clone();
            std::thread::spawn(move || {
                let mut chunk = [0u8; 16384];
                loop {
                    if thread_channel.wait_while_paused() {
                        break;
                    }
                    // O fim do pty chega como `Ok(0)` ou como `EIO` (Linux): os dois encerram a leitura.
                    match reader.read(&mut chunk) {
                        Ok(0) | Err(_) => break,
                        Ok(read) => thread_channel.push(PtyEvent::Data(chunk[..read].to_vec())),
                    }
                }
                let deadline = Instant::now() + EXIT_WAIT;
                let code = loop {
                    let waited = lock(&thread_pty).child_mut().try_wait();
                    match waited {
                        Ok(Some(status)) => break status.code(),
                        Ok(None) if Instant::now() < deadline => std::thread::sleep(EXIT_POLL),
                        _ => break None,
                    }
                };
                thread_channel.push(PtyEvent::Exit(code));
            });

            Ok(Self { pid, pty, writer: Mutex::new(writer), channel })
        }

        /// `pid`.
        pub fn pid(&self) -> u32 {
            self.pid
        }

        /// Os eventos que a thread de leitura já enfileirou, na ordem.
        pub fn drain_events(&self) -> Vec<PtyEvent> {
            self.channel.drain_events()
        }

        /// `write(data)`.
        pub fn write(&self, data: &[u8]) -> std::io::Result<()> {
            let mut writer = lock(&self.writer);
            writer.write_all(data)?;
            writer.flush()
        }

        /// `resize(cols, rows, pixelSize?)`: os pixels são a largura e a altura totais, repartidas entre as células.
        pub fn resize(&self, cols: u16, rows: u16, pixel_size: Option<(u16, u16)>) -> std::io::Result<()> {
            let (cell_width, cell_height) = match pixel_size {
                Some((width, height)) if cols > 0 && rows > 0 => (width / cols, height / rows),
                _ => (0, 0),
            };
            lock(&self.pty).on_resize(WindowSize { num_lines: rows, num_cols: cols, cell_width, cell_height });
            Ok(())
        }

        /// `kill(signal?)`: sem sinal vale `SIGHUP`, como no `node-pty`.
        pub fn kill(&self, signal: Option<&str>) -> std::io::Result<()> {
            let number = match signal.map(|name| name.trim_start_matches("SIG")) {
                None | Some("HUP") => libc_signal::HUP,
                Some("INT") => libc_signal::INT,
                Some("QUIT") => libc_signal::QUIT,
                Some("KILL") => libc_signal::KILL,
                Some("USR1") => libc_signal::USR1,
                Some("USR2") => libc_signal::USR2,
                Some("TERM") => libc_signal::TERM,
                Some(other) => return Err(std::io::Error::new(std::io::ErrorKind::InvalidInput, format!("Unknown signal: {other}"))),
            };
            lock(&self.pty).send_signal(number)
        }

        /// `pause()`: a thread de leitura para, e o núcleo segura a saída do filho.
        pub fn pause(&self) {
            self.channel.pause();
        }

        /// `resume()`.
        pub fn resume(&self) {
            self.channel.resume();
        }

        /// `process`: o nome do processo em primeiro plano do terminal.
        pub fn process(&self) -> Option<String> {
            let group = lock(&self.pty).foreground_process_group()?;
            let name = std::fs::read_to_string(format!("/proc/{group}/comm")).ok()?;
            let name = name.trim();
            (!name.is_empty()).then(|| name.to_owned())
        }
    }

    impl Drop for RawPty {
        fn drop(&mut self) {
            self.channel.close();
        }
    }

    /// Os números dos sinais POSIX que o `kill(signal)` do VS Code usa (iguais em Linux e macOS).
    mod libc_signal {
        pub const HUP: i32 = 1;
        pub const INT: i32 = 2;
        pub const QUIT: i32 = 3;
        pub const KILL: i32 = 9;
        pub const USR1: i32 = if cfg!(target_os = "macos") { 30 } else { 10 };
        pub const USR2: i32 = if cfg!(target_os = "macos") { 31 } else { 12 };
        pub const TERM: i32 = 15;
    }
}

#[cfg(windows)]
mod windows {
    use std::sync::{Arc, Mutex};
    use std::time::{Duration, Instant};

    use alacritty_terminal::event::WindowSize;
    use alacritty_terminal::tty::{self, ChildEvent, EventedPty, Pty};

    use super::{Channel, PtyEvent, PtySpawn, lock, options};

    /// `DEFAULT_COLS` e `DEFAULT_ROWS` do `windowsTerminal.ts` do `node-pty`.
    const DEFAULT_COLS: u16 = 80;
    const DEFAULT_ROWS: u16 = 24;
    /// O intervalo entre duas consultas ao pipe do ConPTY (a leitura do Alacritty não bloqueia).
    const WINDOWS_POLL: Duration = Duration::from_millis(2);
    /// Quantas consultas seguidas sem dado encerram a drenagem do que o ConPTY ainda despeja depois da saída do filho.
    const DRAIN_IDLE_POLLS: u32 = 10;
    /// Quanto esperar para o pipe aceitar a escrita antes de desistir.
    const WRITE_TIMEOUT: Duration = Duration::from_secs(5);
    /// Quanto o descarte gasta drenando a saída para o `ClosePseudoConsole` não travar.
    const DROP_DRAIN: Duration = Duration::from_millis(250);
    /// O código de saída de um filho encerrado por `kill` (o `TerminateProcess` precisa de um).
    const KILLED_EXIT_CODE: u32 = 1;

    /// O `IPty` do `node-pty` sobre o ConPTY.
    pub struct RawPty {
        pid: u32,
        name: String,
        pty: Arc<Mutex<Pty>>,
        channel: Channel,
    }

    /// Lê o que o pipe tem agora e enfileira como `Data`; devolve se havia algo.
    fn read_available(pty: &Mutex<Pty>, channel: &Channel, chunk: &mut [u8]) -> bool {
        let read = lock(pty).try_read(chunk);
        if read > 0 {
            channel.push(PtyEvent::Data(chunk[..read].to_vec()));
        }
        read > 0
    }

    /// Lê até o pipe ficar quieto por `DRAIN_IDLE_POLLS` consultas seguidas.
    fn drain_until_idle(pty: &Mutex<Pty>, channel: &Channel, chunk: &mut [u8]) {
        let mut idle = 0;
        while idle < DRAIN_IDLE_POLLS {
            if read_available(pty, channel, chunk) {
                idle = 0;
            } else {
                idle += 1;
                std::thread::sleep(WINDOWS_POLL);
            }
        }
    }

    /// O nome do executável, sem diretório nem aspas (o `name` que o `TerminalProcess` passa ao `node-pty`).
    fn executable_name(program: &str) -> String {
        let program = program.trim_matches('"');
        program.rsplit(['\\', '/']).next().unwrap_or(program).to_owned()
    }

    impl RawPty {
        /// `spawn(file, args, options)`.
        pub fn spawn(spec: &PtySpawn) -> std::io::Result<Self> {
            let size = WindowSize {
                num_lines: if spec.rows == 0 { DEFAULT_ROWS } else { spec.rows },
                num_cols: if spec.cols == 0 { DEFAULT_COLS } else { spec.cols },
                cell_width: 0,
                cell_height: 0,
            };
            let pty = tty::new(&options(spec), size, 0)?;
            let pid = pty.child_pid().ok_or_else(|| std::io::Error::other("Cannot launch conpty: sem pid do processo filho"))?;
            let pty = Arc::new(Mutex::new(pty));
            let channel = Channel::default();

            let thread_channel = channel.clone();
            let thread_pty = pty.clone();
            std::thread::spawn(move || {
                let mut chunk = [0u8; 16384];
                let code = loop {
                    if thread_channel.wait_while_paused() {
                        return;
                    }
                    if read_available(&thread_pty, &thread_channel, &mut chunk) {
                        continue;
                    }
                    // Sem dado agora: o filho saiu? (o ConPTY não fecha a saída sozinho)
                    let event = lock(&thread_pty).next_child_event();
                    if let Some(ChildEvent::Exited(code)) = event {
                        break code;
                    }
                    std::thread::sleep(WINDOWS_POLL);
                };
                drain_until_idle(&thread_pty, &thread_channel, &mut chunk);
                thread_channel.push(PtyEvent::Exit(code));
            });

            Ok(Self { pid, name: executable_name(&spec.program), pty, channel })
        }

        /// `pid`.
        pub fn pid(&self) -> u32 {
            self.pid
        }

        /// Os eventos que a thread de leitura já enfileirou, na ordem.
        pub fn drain_events(&self) -> Vec<PtyEvent> {
            self.channel.drain_events()
        }

        /// `write(data)`: o pipe do ConPTY aceita só o que couber, então repete até entrar tudo.
        pub fn write(&self, data: &[u8]) -> std::io::Result<()> {
            let deadline = Instant::now() + WRITE_TIMEOUT;
            let mut sent = 0;
            while sent < data.len() {
                sent += lock(&self.pty).try_write(&data[sent..]);
                if sent < data.len() {
                    if Instant::now() >= deadline {
                        return Err(std::io::Error::new(std::io::ErrorKind::TimedOut, "o ConPTY não aceitou a escrita"));
                    }
                    std::thread::sleep(WINDOWS_POLL);
                }
            }
            Ok(())
        }

        /// `resize(cols, rows, pixelSize?)`: o ConPTY não usa os pixels.
        pub fn resize(&self, cols: u16, rows: u16, _pixel_size: Option<(u16, u16)>) -> std::io::Result<()> {
            lock(&self.pty).try_resize(WindowSize { num_lines: rows, num_cols: cols, cell_width: 0, cell_height: 0 })
        }

        /// `kill(signal?)`: o `node-pty` não aceita sinal no Windows; sem sinal encerra o processo filho.
        pub fn kill(&self, signal: Option<&str>) -> std::io::Result<()> {
            if signal.is_some() {
                return Err(std::io::Error::new(std::io::ErrorKind::Unsupported, "Signals not supported on windows."));
            }
            lock(&self.pty).terminate_child(KILLED_EXIT_CODE)
        }

        /// `pause()`.
        pub fn pause(&self) {
            self.channel.pause();
        }

        /// `resume()`.
        pub fn resume(&self) {
            self.channel.resume();
        }

        /// `process`: o nome do executável (ver a nota de divergência no topo do módulo).
        pub fn process(&self) -> Option<String> {
            Some(self.name.clone())
        }
    }

    impl Drop for RawPty {
        fn drop(&mut self) {
            self.channel.close();
            // O `ClosePseudoConsole` do `Pty` bloqueia até a saída ser drenada: encerra o filho e drena antes de soltar.
            let _ = lock(&self.pty).terminate_child(KILLED_EXIT_CODE);
            let mut chunk = [0u8; 16384];
            let deadline = Instant::now() + DROP_DRAIN;
            let scratch = Channel::default();
            while Instant::now() < deadline {
                if !read_available(&self.pty, &scratch, &mut chunk) {
                    std::thread::sleep(WINDOWS_POLL);
                }
            }
        }
    }
}

#[cfg(not(any(unix, windows)))]
mod unsupported {
    use super::PtySpawn;

    /// O pty cru não existe fora do Unix e do Windows.
    pub struct RawPty {
        never: std::convert::Infallible,
    }

    impl RawPty {
        pub fn spawn(_spec: &PtySpawn) -> std::io::Result<Self> {
            Err(std::io::Error::new(std::io::ErrorKind::Unsupported, "o pty cru não existe nesta plataforma"))
        }

        pub fn pid(&self) -> u32 {
            match self.never {}
        }

        pub fn drain_events(&self) -> Vec<super::PtyEvent> {
            match self.never {}
        }

        pub fn write(&self, _data: &[u8]) -> std::io::Result<()> {
            match self.never {}
        }

        pub fn resize(&self, _cols: u16, _rows: u16, _pixel_size: Option<(u16, u16)>) -> std::io::Result<()> {
            match self.never {}
        }

        pub fn kill(&self, _signal: Option<&str>) -> std::io::Result<()> {
            match self.never {}
        }

        pub fn pause(&self) {
            match self.never {}
        }

        pub fn resume(&self) {
            match self.never {}
        }

        pub fn process(&self) -> Option<String> {
            match self.never {}
        }
    }
}
