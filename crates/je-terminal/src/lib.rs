//! O terminal integrado: o emulador de terminal do Alacritty (`alacritty_terminal`, Rust puro) e uma ponte com o
//! processo, no papel do xterm.js e do `node-pty` do VS Code (`vs/workbench/contrib/terminal`).
//!
//! O [`Terminal`] separa as duas metades. O emulador (a grade, o parser `vte`, os modos, o `snapshot`, o `resize` e
//! a rolagem) é o mesmo em todas as plataformas. A ponte é o [`Bridge`]: fora do wasm o [`Terminal::new`] abre um
//! pty local (o ConPTY no Windows) numa thread própria e instala a ponte dele; no wasm não há processo local, e
//! quem tem a conexão com o terminal remoto (o pty mora no servidor, como no VS Code web) cria o terminal com
//! [`Terminal::detached`], entrega os bytes do processo remoto em [`Terminal::feed`] e recebe o que o usuário digita
//! pelo callback de escrita do [`Bridge`] passado a [`Terminal::connect`].
//!
//! Quem pinta pede um [`Snapshot`] da grade com as cores já resolvidas pela [`Palette`] (o tema `terminal.ansi*`),
//! escreve as teclas com [`Terminal::write`] e avisa o tamanho com [`Terminal::resize`].

use std::borrow::Cow;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

pub use alacritty_terminal::event::WindowSize;
use alacritty_terminal::event::{Event, EventListener};
use alacritty_terminal::grid::{Dimensions, Scroll};
use alacritty_terminal::sync::FairMutex;
use alacritty_terminal::term::cell::Flags;
use alacritty_terminal::term::{Config, Term, TermMode};
use alacritty_terminal::vte::ansi::{Color, CursorShape, NamedColor, Processor, Timeout};

#[cfg(not(target_arch = "wasm32"))]
mod local;
#[cfg(not(target_arch = "wasm32"))]
pub use local::{PtyNotifier, default_shell};

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

/// O que o emulador avisa de volta.
#[derive(Clone)]
struct Listener {
    dirty: Arc<AtomicBool>,
    exited: Arc<AtomicBool>,
    title: Arc<Mutex<String>>,
    output: Arc<Mutex<Option<Output>>>,
    cell: Arc<Mutex<(u16, u16)>>,
    /// Acorda o laço de eventos da janela quando a grade muda.
    wake: Wake,
}

/// O aviso à janela de que há quadro novo a pintar.
pub type Wake = Arc<dyn Fn() + Send + Sync>;

/// Para onde vai o que o usuário digita e as respostas do emulador às consultas do programa: o pty local ou a
/// conexão com o terminal remoto.
pub type Output = Arc<dyn Fn(Cow<'static, [u8]>) + Send + Sync>;

/// Avisa o processo de que o tamanho da janela mudou (`SIGWINCH` no pty, a mensagem de resize no terminal remoto).
pub type ResizeHook = Box<dyn Fn(WindowSize) + Send + Sync>;

/// A ponte entre o emulador e um processo, local ou remoto.
pub struct Bridge {
    /// Recebe as teclas, o texto colado e as respostas do emulador.
    pub write: Output,
    /// Recebe o novo tamanho da grade.
    pub resize: ResizeHook,
    /// Roda uma vez, quando o [`Terminal`] é descartado (encerra o processo ou fecha a conexão).
    pub shutdown: Option<Box<dyn FnOnce() + Send + Sync>>,
}

impl Bridge {
    /// Uma ponte sem nada a encerrar no descarte.
    pub fn new(write: Output, resize: ResizeHook) -> Self {
        Self { write, resize, shutdown: None }
    }
}

/// A escrita na ponte instalada; sem ponte, os bytes se perdem (o terminal ainda não tem backend).
fn send_output(slot: &Mutex<Option<Output>>, bytes: Cow<'static, [u8]>) {
    // O callback roda fora do `Mutex`, para poder escrever de novo sem travar.
    let output = slot.lock().expect("envio").clone();
    if let Some(output) = output {
        output(bytes);
    }
}

/// Sem prazo de atualização sincronizada (`CSI ? 2026 h`): o `Instant` do `std` não existe no wasm, e num fluxo
/// remoto o programa já entrega o quadro inteiro de uma vez, então o emulador aplica os bytes na hora.
#[derive(Default)]
struct NoSync;

impl Timeout for NoSync {
    fn set_timeout(&mut self, _: Duration) {}

    fn clear_timeout(&mut self) {}

    fn pending_timeout(&self) -> bool {
        false
    }
}

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
        send_output(&self.output, text.into_bytes().into());
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

pub struct Terminal {
    term: Arc<FairMutex<Term<Listener>>>,
    /// O parser `vte` do caminho de bytes ([`Terminal::feed`]); o pty local tem o parser dele no laço de eventos.
    parser: Mutex<Processor<NoSync>>,
    output: Arc<Mutex<Option<Output>>>,
    resize_hook: Option<ResizeHook>,
    shutdown: Option<Box<dyn FnOnce() + Send + Sync>>,
    dirty: Arc<AtomicBool>,
    exited: Arc<AtomicBool>,
    title: Arc<Mutex<String>>,
    cell: Arc<Mutex<(u16, u16)>>,
    wake: Wake,
    shell_name: String,
    columns: usize,
    lines: usize,
}

impl Terminal {
    /// O emulador sem processo: a grade de `columns` x `lines` células (de `cell_width` x `cell_height` px) à espera
    /// de bytes por [`Terminal::feed`] e de uma ponte por [`Terminal::connect`]. É o terminal do VS Code web sem
    /// servidor: a grade vazia, e o que for digitado se perde até haver conexão. `label` é o rótulo no painel.
    pub fn detached(label: impl Into<String>, columns: usize, lines: usize, cell_width: u16, cell_height: u16, wake: Wake) -> Self {
        Self::build(label.into(), columns, lines, cell_width, cell_height, wake).0
    }

    /// Monta o emulador e devolve também o ouvinte, para o laço de eventos do pty local.
    fn build(shell_name: String, columns: usize, lines: usize, cell_width: u16, cell_height: u16, wake: Wake) -> (Self, Listener) {
        let dirty = Arc::new(AtomicBool::new(true));
        let exited = Arc::new(AtomicBool::new(false));
        let title = Arc::new(Mutex::new(String::new()));
        let output = Arc::new(Mutex::new(None));
        let cell = Arc::new(Mutex::new((cell_width, cell_height)));
        let listener = Listener { dirty: dirty.clone(), exited: exited.clone(), title: title.clone(), output: output.clone(), cell: cell.clone(), wake: wake.clone() };
        let term = Arc::new(FairMutex::new(Term::new(Config::default(), &Size { columns, lines }, listener.clone())));
        let terminal = Self { term, parser: Mutex::new(Processor::default()), output, resize_hook: None, shutdown: None, dirty, exited, title, cell, wake, shell_name, columns, lines };
        (terminal, listener)
    }

    /// Instala a ponte, sem avisar o processo do tamanho (o pty local já nasce no tamanho certo).
    fn install(&mut self, bridge: Bridge) {
        *self.output.lock().expect("envio") = Some(bridge.write);
        self.resize_hook = Some(bridge.resize);
        self.shutdown = bridge.shutdown;
    }

    /// Liga o terminal a um processo (por exemplo o pty remoto de uma conexão) e avisa o tamanho atual da grade.
    /// Troca a ponte anterior, se havia, depois de encerrá-la.
    pub fn connect(&mut self, bridge: Bridge) {
        if let Some(shutdown) = self.shutdown.take() {
            shutdown();
        }
        self.install(bridge);
        self.exited.store(false, Ordering::Release);
        self.notify_size(self.columns, self.lines);
    }

    /// Entrega ao emulador os bytes que o processo escreveu (a saída do pty remoto): passam pelo parser `vte`
    /// e mudam a grade.
    pub fn feed(&self, bytes: &[u8]) {
        if bytes.is_empty() {
            return;
        }
        {
            let mut term = self.term.lock();
            self.parser.lock().expect("parser").advance(&mut *term, bytes);
        }
        self.dirty.store(true, Ordering::Release);
        (self.wake)();
    }

    /// O processo acabou (o shell saiu ou a conexão caiu).
    pub fn close(&self) {
        self.exited.store(true, Ordering::Release);
        self.dirty.store(true, Ordering::Release);
        (self.wake)();
    }

    /// O nome do shell (`zsh`, `bash`, `powershell`), o rótulo do terminal no painel.
    pub fn shell_name(&self) -> &str {
        &self.shell_name
    }

    /// Escreve bytes no processo (teclas, texto colado).
    pub fn write(&self, bytes: impl Into<Cow<'static, [u8]>>) {
        send_output(&self.output, bytes.into());
    }

    fn notify_size(&self, columns: usize, lines: usize) {
        if let Some(hook) = &self.resize_hook {
            let (cell_width, cell_height) = *self.cell.lock().expect("célula");
            hook(WindowSize { num_lines: lines as u16, num_cols: columns as u16, cell_width, cell_height });
        }
    }

    pub fn resize(&mut self, columns: usize, lines: usize, cell_width: u16, cell_height: u16) {
        let (columns, lines) = (columns.max(2), lines.max(1));
        *self.cell.lock().expect("célula") = (cell_width, cell_height);
        if (columns, lines) == (self.columns, self.lines) {
            return;
        }
        self.columns = columns;
        self.lines = lines;
        self.notify_size(columns, lines);
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
        if let Some(shutdown) = self.shutdown.take() {
            shutdown();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    pub(crate) fn palette() -> Palette {
        let mut ansi = [(0, 0, 0); 16];
        ansi[1] = (205, 49, 49);
        Palette { foreground: (204, 204, 204), background: (24, 24, 24), cursor: (255, 255, 255), ansi }
    }

    pub(crate) fn text(snapshot: &Snapshot) -> String {
        snapshot.rows.iter().map(|r| r.iter().map(|c| c.ch).collect::<String>().trim_end().to_owned()).collect::<Vec<_>>().join("\n")
    }

    #[test]
    fn fed_bytes_reach_the_grid_without_a_process() {
        let terminal = Terminal::detached("bash", 40, 5, 8, 16, Arc::new(|| {}));
        assert_eq!(text(&terminal.snapshot(&palette())), "\n\n\n\n", "sem backend a grade fica vazia");
        terminal.feed(b"ola\x1b[31m verm");
        terminal.feed(b"elho\x1b[0m");
        let snapshot = terminal.snapshot(&palette());
        assert!(text(&snapshot).starts_with("ola vermelho"), "{:?}", text(&snapshot));
        assert_eq!(snapshot.rows[0][4].fg, (205, 49, 49), "o SGR 31 pinta de vermelho ANSI");
        assert!(terminal.take_dirty());
    }

    #[test]
    fn synchronized_update_is_applied_at_once() {
        let terminal = Terminal::detached("bash", 40, 5, 8, 16, Arc::new(|| {}));
        terminal.feed(b"\x1b[?2026hquadro\x1b[?2026l");
        assert!(text(&terminal.snapshot(&palette())).starts_with("quadro"));
    }

    #[test]
    fn replies_and_typing_go_through_the_bridge_and_resize_is_forwarded() {
        let written = Arc::new(Mutex::new(Vec::<u8>::new()));
        let sizes = Arc::new(Mutex::new(Vec::<(u16, u16)>::new()));
        let closed = Arc::new(AtomicBool::new(false));
        let mut terminal = Terminal::detached("bash", 40, 5, 8, 16, Arc::new(|| {}));
        terminal.write(b"perdido".as_slice());
        let (w, s, c) = (written.clone(), sizes.clone(), closed.clone());
        terminal.connect(Bridge {
            write: Arc::new(move |bytes| w.lock().unwrap().extend_from_slice(&bytes)),
            resize: Box::new(move |size| s.lock().unwrap().push((size.num_cols, size.num_lines))),
            shutdown: Some(Box::new(move || c.store(true, Ordering::Release))),
        });
        assert_eq!(*sizes.lock().unwrap(), vec![(40, 5)], "a conexão avisa o tamanho atual");
        terminal.write(b"ls\n".as_slice());
        terminal.feed(b"\x1b[6n");
        assert_eq!(&*written.lock().unwrap(), b"ls\n\x1b[1;1R", "a digitação e a resposta do emulador à consulta de cursor");
        terminal.resize(60, 8, 8, 16);
        assert_eq!(sizes.lock().unwrap().last(), Some(&(60, 8)));
        drop(terminal);
        assert!(closed.load(Ordering::Acquire), "o descarte encerra a ponte");
    }
}
