//! O pty cru: o papel do `node-pty` sob o `TerminalProcess` do VS Code (`spawn`, `onData`, `onExit`, `write`, `resize`,
//! `kill`, `pause`, `resume` e `process`). Diferente do [`Terminal`](crate::Terminal), que entrega os bytes direto ao
//! emulador do Alacritty, aqui os bytes saem crus, para o chamador repassá-los como texto (o `onProcessData`).
//!
//! O pty é o `tty::Pty` do Alacritty. Uma thread dedicada lê o lado mestre (em modo bloqueante) e enfileira os eventos;
//! quem consome chama [`RawPty::drain_events`] no próprio laço de eventos, de modo que nenhuma chamada de retorno
//! atravessa threads.
//!
//! Divergências do `node-pty`:
//! - O ambiente de [`PtySpawn::env`] soma-se ao do processo pai (o `tty::Options` do Alacritty não limpa o ambiente);
//!   o `node-pty` usa só o `env` dado. Quem passa o `process.env` inteiro (o caso do `TerminalProcess`) não vê diferença.
//! - Só existe no Unix. No Windows (ConPTY) [`RawPty::spawn`] devolve `ErrorKind::Unsupported`.
//! - `process` lê o nome do grupo de processos em primeiro plano em `/proc/<pgrp>/comm`, só no Linux.

use std::collections::HashMap;
use std::path::PathBuf;

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
#[cfg(not(unix))]
pub use unsupported::RawPty;

#[cfg(unix)]
mod unix {
    use std::collections::VecDeque;
    use std::fs::File;
    use std::io::{Read, Write};
    use std::sync::{Arc, Condvar, Mutex, PoisonError};
    use std::time::{Duration, Instant};

    use alacritty_terminal::event::{OnResize, WindowSize};
    use alacritty_terminal::tty::{self, Options, Pty, Shell};

    use super::{PtyEvent, PtySpawn};

    /// Quanto esperar o filho sair depois de o pty fechar, antes de dar o código por desconhecido.
    const EXIT_WAIT: Duration = Duration::from_secs(2);
    const EXIT_POLL: Duration = Duration::from_millis(5);

    /// `paused` e `closed` da thread de leitura.
    #[derive(Default)]
    struct Gate {
        paused: bool,
        closed: bool,
    }

    type Events = Arc<Mutex<VecDeque<PtyEvent>>>;

    /// O `IPty` do `node-pty`.
    pub struct RawPty {
        pid: u32,
        pty: Arc<Mutex<Pty>>,
        writer: Mutex<File>,
        events: Events,
        gate: Arc<(Mutex<Gate>, Condvar)>,
    }

    fn lock<T>(mutex: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
        mutex.lock().unwrap_or_else(PoisonError::into_inner)
    }

    impl RawPty {
        /// `spawn(file, args, options)`.
        pub fn spawn(spec: &PtySpawn) -> std::io::Result<Self> {
            // No Windows o `Options` tem o campo extra `escape_args`: o `..Default` é necessário lá.
            #[allow(clippy::needless_update)]
            let options = Options {
                shell: Some(Shell::new(spec.program.clone(), spec.args.clone())),
                working_directory: spec.cwd.clone(),
                drain_on_exit: false,
                env: spec.env.clone(),
                ..Options::default()
            };
            let size = WindowSize { num_lines: spec.rows, num_cols: spec.cols, cell_width: 0, cell_height: 0 };
            let pty = tty::new(&options, size, 0)?;
            pty.set_blocking()?;
            let mut reader = pty.file().try_clone()?;
            let writer = pty.file().try_clone()?;
            let pid = pty.child().id();
            let pty = Arc::new(Mutex::new(pty));
            let events: Events = Arc::default();
            let gate: Arc<(Mutex<Gate>, Condvar)> = Arc::default();

            let thread_events = events.clone();
            let thread_gate = gate.clone();
            let thread_pty = pty.clone();
            std::thread::spawn(move || {
                let mut chunk = [0u8; 16384];
                loop {
                    {
                        let (state, wake) = &*thread_gate;
                        let mut state = lock(state);
                        while state.paused && !state.closed {
                            state = wake.wait(state).unwrap_or_else(PoisonError::into_inner);
                        }
                        if state.closed {
                            break;
                        }
                    }
                    // O fim do pty chega como `Ok(0)` ou como `EIO` (Linux): os dois encerram a leitura.
                    match reader.read(&mut chunk) {
                        Ok(0) | Err(_) => break,
                        Ok(read) => lock(&thread_events).push_back(PtyEvent::Data(chunk[..read].to_vec())),
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
                lock(&thread_events).push_back(PtyEvent::Exit(code));
            });

            Ok(Self { pid, pty, writer: Mutex::new(writer), events, gate })
        }

        /// `pid`.
        pub fn pid(&self) -> u32 {
            self.pid
        }

        /// Os eventos que a thread de leitura já enfileirou, na ordem.
        pub fn drain_events(&self) -> Vec<PtyEvent> {
            lock(&self.events).drain(..).collect()
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
            lock(&self.gate.0).paused = true;
        }

        /// `resume()`.
        pub fn resume(&self) {
            lock(&self.gate.0).paused = false;
            self.gate.1.notify_all();
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
            lock(&self.gate.0).closed = true;
            self.gate.1.notify_all();
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

#[cfg(not(unix))]
mod unsupported {
    use super::PtySpawn;

    /// O pty cru não existe fora do Unix (o ConPTY ainda não tem a ponte crua).
    pub struct RawPty {
        never: std::convert::Infallible,
    }

    impl RawPty {
        pub fn spawn(_spec: &PtySpawn) -> std::io::Result<Self> {
            Err(std::io::Error::new(std::io::ErrorKind::Unsupported, "o pty cru ainda não existe no Windows (ConPTY)"))
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
