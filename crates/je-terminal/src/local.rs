//! A ponte com o processo local: o pty (com o ConPTY no Windows) e o laço de eventos do Alacritty.
//!
//! Só existe fora do wasm: no navegador não há processo local e os bytes chegam pelo [`Bridge`] do terminal
//! remoto, como no VS Code web (o pty mora no servidor).

use std::io;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use alacritty_terminal::event::WindowSize;
use alacritty_terminal::event_loop::{EventLoop, Msg, Notifier};
use alacritty_terminal::tty::{self, Options, Shell};

use crate::{Bridge, Terminal, Wake};

/// O shell padrão do sistema (`$SHELL`, ou o PowerShell no Windows).
pub fn default_shell() -> (String, Vec<String>) {
    if cfg!(windows) {
        // O perfil padrão do VS Code no Windows é o PowerShell.
        return ("powershell.exe".to_owned(), vec!["-NoLogo".to_owned()]);
    }
    (std::env::var("SHELL").unwrap_or_else(|_| "/bin/sh".to_owned()), Vec::new())
}

/// O `Notifier` do Alacritty fica exportado só para quem precisar de um canal próprio; o [`Terminal`] usa
/// o `EventLoopSender` direto.
pub type PtyNotifier = Notifier;

impl Terminal {
    /// Abre um terminal de `columns` x `lines` células (de `cell_width` x `cell_height` px) com o shell dado
    /// (ou o padrão) em `working_directory`.
    pub fn new(working_directory: Option<PathBuf>, shell: Option<(String, Vec<String>)>, columns: usize, lines: usize, cell_width: u16, cell_height: u16, wake: Wake) -> io::Result<Self> {
        let (program, args) = shell.unwrap_or_else(default_shell);
        let shell_name = std::path::Path::new(&program).file_stem().map_or_else(|| program.clone(), |n| n.to_string_lossy().into_owned());
        let mut env = std::collections::HashMap::new();
        env.insert("TERM".to_owned(), "xterm-256color".to_owned());
        env.insert("COLORTERM".to_owned(), "truecolor".to_owned());
        env.insert("TERM_PROGRAM".to_owned(), "vscode".to_owned());
        // No Windows o `Options` tem o campo extra `escape_args`: o `..Default` é necessário lá.
        #[allow(clippy::needless_update)]
        let options = Options { shell: Some(Shell::new(program, args)), working_directory, drain_on_exit: false, env, ..Options::default() };
        let size = WindowSize { num_lines: lines as u16, num_cols: columns as u16, cell_width, cell_height };
        let pty = tty::new(&options, size, 0)?;
        let (mut terminal, listener) = Self::build(shell_name, columns, lines, cell_width, cell_height, wake);
        let event_loop = EventLoop::new(terminal.term.clone(), listener, pty, false, false)?;
        // O `EventLoopSender` fica atrás de um `Mutex` para o callback ser `Sync` sem depender do tipo do canal.
        let (writer, resizer, closer) = (Mutex::new(event_loop.channel()), Mutex::new(event_loop.channel()), Mutex::new(event_loop.channel()));
        terminal.install(Bridge {
            write: Arc::new(move |bytes| {
                let _ = writer.lock().expect("envio").send(Msg::Input(bytes));
            }),
            resize: Box::new(move |size| {
                let _ = resizer.lock().expect("envio").send(Msg::Resize(size));
            }),
            shutdown: Some(Box::new(move || {
                let _ = closer.lock().expect("envio").send(Msg::Shutdown);
            })),
        });
        event_loop.spawn();
        Ok(terminal)
    }
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use crate::tests::{palette, text};
    use std::time::{Duration, Instant};

    #[test]
    fn shell_output_reaches_the_grid() {
        let mut terminal = Terminal::new(None, Some(("/bin/sh".to_owned(), vec!["-c".to_owned(), "printf 'ola\\033[31m vermelho\\033[0m'; sleep 5".to_owned()])), 40, 5, 8, 16, Arc::new(|| {})).expect("abre");
        let deadline = Instant::now() + Duration::from_secs(10);
        let snapshot = loop {
            let snapshot = terminal.snapshot(&palette());
            if text(&snapshot).contains("vermelho") || Instant::now() > deadline {
                break snapshot;
            }
            std::thread::sleep(Duration::from_millis(20));
        };
        assert!(text(&snapshot).starts_with("ola vermelho"), "a saída do programa está na grade: {:?}", text(&snapshot));
        assert_eq!(snapshot.rows[0][4].fg, (205, 49, 49), "o SGR 31 pinta de vermelho ANSI");
        assert_eq!(snapshot.rows[0][0].fg, (204, 204, 204));
        terminal.resize(60, 8, 8, 16);
        assert_eq!((terminal.columns(), terminal.lines()), (60, 8));
        assert_eq!(terminal.snapshot(&palette()).rows.len(), 8);
    }

    #[test]
    fn typed_bytes_are_echoed_by_the_shell() {
        let terminal = Terminal::new(None, Some(("/bin/sh".to_owned(), vec!["-c".to_owned(), "cat".to_owned()])), 40, 5, 8, 16, Arc::new(|| {})).expect("abre");
        terminal.write(b"hello\n".as_slice());
        let deadline = Instant::now() + Duration::from_secs(10);
        loop {
            let t = text(&terminal.snapshot(&palette()));
            if t.matches("hello").count() >= 2 || Instant::now() > deadline {
                assert!(t.matches("hello").count() >= 2, "o eco do pty e a saída do cat: {t:?}");
                break;
            }
            std::thread::sleep(Duration::from_millis(20));
        }
    }
}
