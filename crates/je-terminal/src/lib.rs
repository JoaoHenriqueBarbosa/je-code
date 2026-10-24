//! O terminal integrado: um pty com o shell do usuário e o emulador de terminal do Alacritty
//! (`alacritty_terminal`, Rust puro, com o ConPTY no Windows), no papel do xterm.js e do `node-pty` do VS Code
//! (`vs/workbench/contrib/terminal`).
//!
//! O [`Terminal`] roda o pty numa thread própria (o laço de eventos do Alacritty). Quem pinta pede um
//! [`Snapshot`] da grade com as cores já resolvidas pela [`Palette`] (o tema `terminal.ansi*`), escreve as
//! teclas com [`Terminal::write`] e avisa o tamanho com [`Terminal::resize`].

use std::io;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use alacritty_terminal::event::{Event, EventListener, WindowSize};
use alacritty_terminal::event_loop::{EventLoop, EventLoopSender, Msg, Notifier};
use alacritty_terminal::grid::{Dimensions, Scroll};
use alacritty_terminal::sync::FairMutex;
use alacritty_terminal::term::cell::Flags;
use alacritty_terminal::term::{Config, Term, TermMode};
use alacritty_terminal::tty::{self, Options, Shell};
use alacritty_terminal::vte::ansi::{Color, CursorShape, NamedColor};

/// Uma cor opaca RGB.
pub type Rgb = (u8, u8, u8);

/// As cores do terminal no tema: o fundo e o primeiro plano padrão, o cursor e as 16 cores ANSI.
#[derive(Clone, Debug)]
pub struct Palette {
    pub foreground: Rgb,
    pub background: Rgb,
    pub cursor: Rgb,
    pub ansi: [Rgb; 16],
}

/// Uma célula da grade já resolvida.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Cell {
    pub ch: char,
    pub fg: Rgb,
    /// `None` é o fundo padrão do terminal.
    pub bg: Option<Rgb>,
    pub bold: bool,
    pub italic: bool,
    pub underline: bool,
    pub strikeout: bool,
    /// Primeira metade de um caractere de duas colunas.
    pub wide: bool,
}

/// A forma do cursor.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CursorKind {
    Block,
    Underline,
    Beam,
    Hollow,
}

/// O que o painel precisa para pintar um quadro.
#[derive(Clone, Debug)]
pub struct Snapshot {
    pub columns: usize,
    pub rows: Vec<Vec<Cell>>,
    /// Coluna, linha e forma do cursor, se visível.
    pub cursor: Option<(usize, usize, CursorKind)>,
    pub title: String,
}

/// O que o laço do pty avisa de volta.
#[derive(Clone)]
struct Listener {
    dirty: Arc<AtomicBool>,
    exited: Arc<AtomicBool>,
    title: Arc<Mutex<String>>,
    sender: Arc<Mutex<Option<EventLoopSender>>>,
    cell: Arc<Mutex<(u16, u16)>>,
    /// Acorda o laço de eventos da janela quando a grade muda.
    wake: Wake,
}

/// O aviso à janela de que há quadro novo a pintar.
pub type Wake = Arc<dyn Fn() + Send + Sync>;

impl EventListener for Listener {
    fn send_event(&self, event: Event) {
        if matches!(event, Event::Wakeup | Event::Title(_) | Event::ResetTitle | Event::Bell | Event::CursorBlinkingChange | Event::Exit | Event::ChildExit(_)) {
            (self.wake)();
        }
        match event {
            Event::Wakeup | Event::CursorBlinkingChange | Event::Bell => self.dirty.store(true, Ordering::Release),
            Event::Title(title) => {
                *self.title.lock().expect("título") = title;
                self.dirty.store(true, Ordering::Release);
            }
            Event::ResetTitle => {
                self.title.lock().expect("título").clear();
                self.dirty.store(true, Ordering::Release);
            }
            Event::PtyWrite(text) => self.reply(text),
            Event::TextAreaSizeRequest(format) => {
                let (w, h) = *self.cell.lock().expect("célula");
                let size = WindowSize { num_lines: 0, num_cols: 0, cell_width: w, cell_height: h };
                self.reply(format(size));
            }
            Event::Exit | Event::ChildExit(_) => {
                self.exited.store(true, Ordering::Release);
                self.dirty.store(true, Ordering::Release);
            }
            _ => {}
        }
    }
}

impl Listener {
    /// A resposta do emulador a uma consulta do programa (posição do cursor, tamanho da janela...).
    fn reply(&self, text: String) {
        if let Some(sender) = self.sender.lock().expect("envio").as_ref() {
            let _ = sender.send(Msg::Input(text.into_bytes().into()));
        }
    }
}

struct Size {
    columns: usize,
    lines: usize,
}

impl Dimensions for Size {
    fn total_lines(&self) -> usize {
        self.lines
    }

    fn screen_lines(&self) -> usize {
        self.lines
    }

    fn columns(&self) -> usize {
        self.columns
    }
}

/// O shell padrão do sistema (`$SHELL`, ou o PowerShell no Windows).
pub fn default_shell() -> (String, Vec<String>) {
    if cfg!(windows) {
        // O perfil padrão do VS Code no Windows é o PowerShell.
        return ("powershell.exe".to_owned(), vec!["-NoLogo".to_owned()]);
    }
    (std::env::var("SHELL").unwrap_or_else(|_| "/bin/sh".to_owned()), Vec::new())
}

pub struct Terminal {
    term: Arc<FairMutex<Term<Listener>>>,
    sender: EventLoopSender,
    dirty: Arc<AtomicBool>,
    exited: Arc<AtomicBool>,
    title: Arc<Mutex<String>>,
    cell: Arc<Mutex<(u16, u16)>>,
    shell_name: String,
    columns: usize,
    lines: usize,
}

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
        let dirty = Arc::new(AtomicBool::new(true));
        let exited = Arc::new(AtomicBool::new(false));
        let title = Arc::new(Mutex::new(String::new()));
        let sender_slot = Arc::new(Mutex::new(None));
        let cell = Arc::new(Mutex::new((cell_width, cell_height)));
        let listener = Listener { dirty: dirty.clone(), exited: exited.clone(), title: title.clone(), sender: sender_slot.clone(), cell: cell.clone(), wake };
        let term = Arc::new(FairMutex::new(Term::new(Config::default(), &Size { columns, lines }, listener.clone())));
        let event_loop = EventLoop::new(term.clone(), listener, pty, false, false)?;
        let sender = event_loop.channel();
        *sender_slot.lock().expect("envio") = Some(sender.clone());
        event_loop.spawn();
        Ok(Self { term, sender, dirty, exited, title, cell, shell_name, columns, lines })
    }

    /// O nome do shell (`zsh`, `bash`, `powershell`), o rótulo do terminal no painel.
    pub fn shell_name(&self) -> &str {
        &self.shell_name
    }

    /// Escreve bytes no pty (teclas, texto colado).
    pub fn write(&self, bytes: impl Into<std::borrow::Cow<'static, [u8]>>) {
        let _ = self.sender.send(Msg::Input(bytes.into()));
    }

    pub fn resize(&mut self, columns: usize, lines: usize, cell_width: u16, cell_height: u16) {
        let (columns, lines) = (columns.max(2), lines.max(1));
        *self.cell.lock().expect("célula") = (cell_width, cell_height);
        if (columns, lines) == (self.columns, self.lines) {
            return;
        }
        self.columns = columns;
        self.lines = lines;
        let size = WindowSize { num_lines: lines as u16, num_cols: columns as u16, cell_width, cell_height };
        let _ = self.sender.send(Msg::Resize(size));
        self.term.lock().resize(Size { columns, lines });
        self.dirty.store(true, Ordering::Release);
    }

    /// Rola o histórico (positivo sobe).
    pub fn scroll(&self, lines: i32) {
        self.term.lock().scroll_display(Scroll::Delta(lines));
        self.dirty.store(true, Ordering::Release);
    }

    pub fn scroll_to_bottom(&self) {
        self.term.lock().scroll_display(Scroll::Bottom);
    }

    /// O programa pediu as setas em modo de aplicação (`DECCKM`).
    pub fn app_cursor(&self) -> bool {
        self.term.lock().mode().contains(TermMode::APP_CURSOR)
    }

    /// O programa pediu colagem entre colchetes (`bracketed paste`).
    pub fn bracketed_paste(&self) -> bool {
        self.term.lock().mode().contains(TermMode::BRACKETED_PASTE)
    }

    /// Mudou algo desde a última chamada?
    pub fn take_dirty(&self) -> bool {
        self.dirty.swap(false, Ordering::AcqRel)
    }

    /// O shell terminou.
    pub fn has_exited(&self) -> bool {
        self.exited.load(Ordering::Acquire)
    }

    pub fn columns(&self) -> usize {
        self.columns
    }

    pub fn lines(&self) -> usize {
        self.lines
    }

    /// Resolve a cor de uma célula pela paleta (as 256 do xterm e as de 24 bits incluídas).
    fn resolve(color: Color, palette: &Palette, bold: bool) -> Rgb {
        match color {
            Color::Spec(rgb) => (rgb.r, rgb.g, rgb.b),
            Color::Named(named) => match named {
                NamedColor::Foreground | NamedColor::BrightForeground | NamedColor::DimForeground => palette.foreground,
                NamedColor::Background => palette.background,
                NamedColor::Cursor => palette.cursor,
                other => {
                    let index = other as usize;
                    // As cores escurecidas (`Dim*`) caem nas normais.
                    let index = if index >= 261 { index - 261 } else { index };
                    // Negrito com cor ANSI clara: a variante brilhante, como o xterm.js com `drawBoldTextInBrightColors`.
                    let index = if bold && index < 8 { index + 8 } else { index };
                    palette.ansi.get(index).copied().unwrap_or(palette.foreground)
                }
            },
            Color::Indexed(i) => {
                let i = usize::from(i);
                match i {
                    0..=15 => palette.ansi[i],
                    16..=231 => {
                        let n = i - 16;
                        let level = |v: usize| if v == 0 { 0 } else { (55 + v * 40) as u8 };
                        (level(n / 36), level(n / 6 % 6), level(n % 6))
                    }
                    _ => {
                        let v = (8 + (i - 232) * 10) as u8;
                        (v, v, v)
                    }
                }
            }
        }
    }

    /// A grade visível como células resolvidas.
    pub fn snapshot(&self, palette: &Palette) -> Snapshot {
        let term = self.term.lock();
        let content = term.renderable_content();
        let offset = content.display_offset;
        let (columns, lines) = (term.columns(), term.screen_lines());
        let blank = Cell { ch: ' ', fg: palette.foreground, bg: None, bold: false, italic: false, underline: false, strikeout: false, wide: false };
        let mut rows = vec![vec![blank; columns]; lines];
        for indexed in content.display_iter {
            let (row, col) = (indexed.point.line.0 + offset as i32, indexed.point.column.0);
            if row < 0 || row as usize >= lines || col >= columns {
                continue;
            }
            let cell = indexed.cell;
            if cell.flags.contains(Flags::WIDE_CHAR_SPACER) || cell.flags.contains(Flags::LEADING_WIDE_CHAR_SPACER) {
                continue;
            }
            let bold = cell.flags.contains(Flags::BOLD);
            let (mut fg, mut bg) = (Self::resolve(cell.fg, palette, bold), Self::resolve(cell.bg, palette, false));
            if cell.flags.contains(Flags::INVERSE) {
                std::mem::swap(&mut fg, &mut bg);
            }
            if cell.flags.contains(Flags::DIM) {
                fg = ((u16::from(fg.0) * 2 / 3) as u8, (u16::from(fg.1) * 2 / 3) as u8, (u16::from(fg.2) * 2 / 3) as u8);
            }
            let has_bg = bg != palette.background || cell.flags.contains(Flags::INVERSE);
            rows[row as usize][col] = Cell {
                ch: if cell.flags.contains(Flags::HIDDEN) { ' ' } else { cell.c },
                fg,
                bg: has_bg.then_some(bg),
                bold,
                italic: cell.flags.contains(Flags::ITALIC),
                underline: cell.flags.intersects(Flags::ALL_UNDERLINES),
                strikeout: cell.flags.contains(Flags::STRIKEOUT),
                wide: cell.flags.contains(Flags::WIDE_CHAR),
            };
        }
        let cursor = (offset == 0 && content.cursor.shape != CursorShape::Hidden).then(|| {
            let kind = match content.cursor.shape {
                CursorShape::Underline => CursorKind::Underline,
                CursorShape::Beam => CursorKind::Beam,
                CursorShape::HollowBlock => CursorKind::Hollow,
                _ => CursorKind::Block,
            };
            (content.cursor.point.column.0, content.cursor.point.line.0.max(0) as usize, kind)
        });
        Snapshot { columns, rows, cursor, title: self.title.lock().expect("título").clone() }
    }
}

impl Drop for Terminal {
    fn drop(&mut self) {
        let _ = self.sender.send(Msg::Shutdown);
    }
}

/// O `Notifier` do Alacritty fica exportado só para quem precisar de um canal próprio; o [`Terminal`] usa
/// o `EventLoopSender` direto.
pub type PtyNotifier = Notifier;

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use std::time::{Duration, Instant};

    fn palette() -> Palette {
        let mut ansi = [(0, 0, 0); 16];
        ansi[1] = (205, 49, 49);
        Palette { foreground: (204, 204, 204), background: (24, 24, 24), cursor: (255, 255, 255), ansi }
    }

    fn text(snapshot: &Snapshot) -> String {
        snapshot.rows.iter().map(|r| r.iter().map(|c| c.ch).collect::<String>().trim_end().to_owned()).collect::<Vec<_>>().join("\n")
    }

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
