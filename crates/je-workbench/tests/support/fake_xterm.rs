//! Dublê do emulador de terminal (`Terminal` do `@xterm/xterm` e do `@xterm/headless`) dos testes de terminal: um
//! emulador mínimo, mas fiel no que os testes leem. Os testes do original criam um `Terminal` real e escrevem texto nele
//! (`xterm.write`, `writeP`); este dublê faz o mesmo contra o contrato do porte.
//!
//! O que ele faz:
//! - Escreve texto célula a célula, quebra a linha ao passar das colunas (com o `isWrapped` na linha seguinte, como o
//!   xterm), trata `\r` e `\n`, e deixa o cursor em `cols` depois da última coluna (a quebra pendente do xterm).
//! - Lê as sequências OSC (`ESC ] Ps ; Pt BEL` ou `ESC \`) e CSI (`ESC [ params final`) do texto escrito e as entrega aos
//!   tratadores do `parser`, do último registrado para o primeiro, até um devolver `true`. Sem tratador que consuma, o
//!   `CSI 2 J` e o `CSI 3 J` limpam a área visível, o padrão do xterm.
//! - Registra marcadores ([`FakeMarker`]) na linha do cursor mais o deslocamento.
//!
//! O que ele não faz: caracteres largos (toda célula tem largura 1), atributos de cor e estilo (toda célula é sem estilo),
//! e reflow no redimensionamento. O limite de scrollback (`rows + options.scrollback` linhas, as mais antigas saem e os
//! marcadores descem junto) só vale para o terminal criado com [`FakeXterm::from_options`], e é lido uma vez, na criação.
//!
//! Implementa os contratos do porte: o `IHeadlessTerminal` de `je_platform::terminal::xterm_headless`, o `ITerminal` de
//! `je_workbench::contrib::terminal::xterm_buffer` e o `IXtermRaw` do `XtermTerminal`, com os buffers, as linhas e as células
//! de cada um.

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use je_base::event::{Emitter, Event};
use je_base::js_string::JsString;
use je_base::lifecycle::{DisposableRef, IDisposable, plain_disposable};
use je_platform::terminal::capabilities::capabilities::IMarker;
use je_platform::terminal::xterm_headless as platform;
use je_workbench::contrib::terminal::browser::xterm::xterm_terminal::{IXtermRaw, XtermOptions};
use je_workbench::contrib::terminal::xterm_buffer as workbench;

/// O marcador de uma linha do buffer (`IMarker` do xterm).
pub struct FakeMarker {
    id: f64,
    line: Cell<f64>,
    disposed: Cell<bool>,
    on_dispose: Rc<Emitter<()>>,
}

impl FakeMarker {
    pub fn new(id: f64, line: f64) -> Rc<Self> {
        Rc::new(Self { id, line: Cell::new(line), disposed: Cell::new(false), on_dispose: Emitter::new() })
    }

    /// As `amount` linhas mais antigas saíram do scrollback: o marcador desce o mesmo tanto, e sai do buffer (é descartado)
    /// quando a linha dele foi a que saiu.
    fn scroll_out(&self, amount: i32) {
        if self.disposed.get() {
            return;
        }
        let line = self.line.get() - f64::from(amount);
        if line < 0.0 {
            self.dispose();
        } else {
            self.line.set(line);
        }
    }
}

impl IDisposable for FakeMarker {
    fn dispose(&self) {
        if !self.disposed.replace(true) {
            self.line.set(-1.0);
            self.on_dispose.fire(());
            // O `Marker` do xterm descarta o emissor junto com o marcador: as inscrições de `onDispose` saem.
            self.on_dispose.dispose();
        }
    }
}

impl IMarker for FakeMarker {
    fn id(&self) -> f64 {
        self.id
    }

    fn is_disposed(&self) -> bool {
        self.disposed.get()
    }

    fn line(&self) -> f64 {
        self.line.get()
    }

    fn on_dispose(&self) -> Event<()> {
        // O `onDispose` do xterm é um `EventEmitter` do próprio xterm, fora do rastreador de descartáveis do VS Code: a inscrição
        // não conta como vazamento.
        let event = self.on_dispose.event();
        Event::new(move |listener| je_base::lifecycle::mark_as_singleton(event.listen_rc(listener)))
    }
}

/// Uma célula sem estilo de largura 1.
pub struct FakeCell {
    chars: String,
}

impl platform::IBufferCell for FakeCell {
    fn get_chars(&self) -> JsString {
        JsString::from(self.chars.as_str())
    }

    fn get_code(&self) -> i32 {
        self.chars.chars().next().map_or(0, |c| c as i32)
    }

    fn get_fg_color(&self) -> i32 {
        -1
    }

    fn get_bg_color(&self) -> i32 {
        -1
    }

    fn get_fg_color_mode(&self) -> i32 {
        0
    }

    fn get_bg_color_mode(&self) -> i32 {
        0
    }

    fn is_bold(&self) -> u32 {
        0
    }

    fn is_italic(&self) -> u32 {
        0
    }

    fn is_dim(&self) -> u32 {
        0
    }

    fn is_underline(&self) -> u32 {
        0
    }

    fn is_blink(&self) -> u32 {
        0
    }

    fn is_inverse(&self) -> u32 {
        0
    }

    fn is_invisible(&self) -> u32 {
        0
    }

    fn is_strikethrough(&self) -> u32 {
        0
    }

    fn is_overline(&self) -> u32 {
        0
    }
}

impl workbench::IBufferCell for FakeCell {
    fn get_width(&self) -> i32 {
        1
    }

    fn get_chars(&self) -> JsString {
        JsString::from(self.chars.as_str())
    }

    fn is_bold(&self) -> u32 {
        0
    }

    fn is_inverse(&self) -> u32 {
        0
    }

    fn is_strikethrough(&self) -> u32 {
        0
    }

    fn is_underline(&self) -> u32 {
        0
    }

    fn is_dim(&self) -> u32 {
        0
    }

    fn is_italic(&self) -> u32 {
        0
    }
}

/// O conteúdo de uma linha: `None` é a célula nula (nunca escrita).
#[derive(Clone)]
struct LineData {
    cells: Vec<Option<char>>,
    is_wrapped: bool,
}

impl LineData {
    fn new(cols: i32) -> Self {
        Self { cells: vec![None; usize::try_from(cols).unwrap_or(0)], is_wrapped: false }
    }
}

/// Uma linha do buffer (um retrato do conteúdo na hora da leitura).
pub struct FakeLine {
    data: LineData,
}

impl FakeLine {
    fn length(&self) -> i32 {
        i32::try_from(self.data.cells.len()).unwrap_or(i32::MAX)
    }

    fn get_cell(&self, x: i32) -> Option<Rc<FakeCell>> {
        let cell = self.data.cells.get(usize::try_from(x).ok()?)?;
        Some(Rc::new(FakeCell { chars: cell.map(String::from).unwrap_or_default() }))
    }

    /// `translateToString(trimRight, startColumn, endColumn)`: a célula nula sai como espaço, e `trimRight` corta
    /// depois da última célula com conteúdo.
    fn translate_to_string(&self, trim_right: Option<bool>, start_column: Option<i32>, end_column: Option<i32>) -> JsString {
        let length = self.data.cells.len();
        let mut end = end_column.map_or(length, |end| usize::try_from(end).unwrap_or(0).min(length));
        if trim_right == Some(true) {
            let trimmed_length = self.data.cells.iter().rposition(Option::is_some).map_or(0, |index| index + 1);
            end = end.min(trimmed_length);
        }
        let start = start_column.map_or(0, |start| usize::try_from(start).unwrap_or(0));
        let text: String = if start >= end { String::new() } else { self.data.cells[start..end].iter().map(|cell| cell.unwrap_or(' ')).collect() };
        JsString::from(text.as_str())
    }

    fn is_wrapped(&self) -> bool {
        self.data.is_wrapped
    }
}

impl platform::IBufferLine for FakeLine {
    fn is_wrapped(&self) -> bool {
        FakeLine::is_wrapped(self)
    }

    fn length(&self) -> i32 {
        FakeLine::length(self)
    }

    fn get_cell(&self, x: i32) -> Option<Rc<dyn platform::IBufferCell>> {
        FakeLine::get_cell(self, x).map(|cell| cell as Rc<dyn platform::IBufferCell>)
    }

    fn translate_to_string(&self, trim_right: Option<bool>, start_column: Option<i32>, end_column: Option<i32>) -> JsString {
        FakeLine::translate_to_string(self, trim_right, start_column, end_column)
    }
}

impl workbench::IBufferLine for FakeLine {
    fn is_wrapped(&self) -> bool {
        FakeLine::is_wrapped(self)
    }

    fn length(&self) -> i32 {
        FakeLine::length(self)
    }

    fn get_cell(&self, x: i32) -> Option<Rc<dyn workbench::IBufferCell>> {
        FakeLine::get_cell(self, x).map(|cell| cell as Rc<dyn workbench::IBufferCell>)
    }

    fn translate_to_string(&self, trim_right: Option<bool>, start_column: Option<i32>, end_column: Option<i32>) -> JsString {
        FakeLine::translate_to_string(self, trim_right, start_column, end_column)
    }
}

/// O estado da tela: todas as linhas (o scrollback e a área visível) e o cursor.
struct Screen {
    cols: i32,
    rows: i32,
    lines: RefCell<Vec<LineData>>,
    cursor_x: Cell<i32>,
    cursor_y: Cell<i32>,
    base_y: Cell<i32>,
    /// O limite de linhas do scrollback (`options.scrollback`); `None` não limita.
    scrollback: Option<i32>,
    /// As linhas que saíram do scrollback desde a última leitura (os marcadores descem esse tanto).
    trimmed: Cell<i32>,
}

impl Screen {
    fn ensure_lines(&self, count: i32) {
        let mut lines = self.lines.borrow_mut();
        while i32::try_from(lines.len()).unwrap_or(i32::MAX) < count {
            lines.push(LineData::new(self.cols));
        }
    }

    /// Descarta as linhas mais antigas além de `rows + scrollback`, como o buffer do xterm.
    fn trim_scrollback(&self) {
        let Some(scrollback) = self.scrollback else { return };
        let limit = usize::try_from(self.rows + scrollback).unwrap_or(0);
        let mut lines = self.lines.borrow_mut();
        while lines.len() > limit {
            lines.remove(0);
            self.base_y.set(self.base_y.get() - 1);
            self.trimmed.set(self.trimmed.get() + 1);
        }
    }

    fn reset(&self) {
        self.lines.borrow_mut().clear();
        self.cursor_x.set(0);
        self.cursor_y.set(0);
        self.base_y.set(0);
        self.ensure_lines(self.rows);
    }

    /// O avanço de linha (`\n` e a quebra automática): rola a área visível quando o cursor está na última linha.
    fn line_feed(&self, wrapped: bool) {
        if self.cursor_y.get() >= self.rows - 1 {
            self.base_y.set(self.base_y.get() + 1);
        } else {
            self.cursor_y.set(self.cursor_y.get() + 1);
        }
        self.ensure_lines(self.base_y.get() + self.cursor_y.get() + 1);
        self.trim_scrollback();
        let absolute = self.base_y.get() + self.cursor_y.get();
        if wrapped && let Some(line) = self.lines.borrow_mut().get_mut(usize::try_from(absolute).unwrap_or(0)) {
            line.is_wrapped = true;
        }
    }

    fn put_char(&self, c: char) {
        if self.cursor_x.get() >= self.cols {
            self.cursor_x.set(0);
            self.line_feed(true);
        }
        let absolute = usize::try_from(self.base_y.get() + self.cursor_y.get()).unwrap_or(0);
        if let Some(line) = self.lines.borrow_mut().get_mut(absolute)
            && let Some(cell) = line.cells.get_mut(usize::try_from(self.cursor_x.get()).unwrap_or(0))
        {
            *cell = Some(c);
        }
        self.cursor_x.set(self.cursor_x.get() + 1);
    }

    fn clear_viewport(&self) {
        let base = usize::try_from(self.base_y.get()).unwrap_or(0);
        for line in self.lines.borrow_mut().iter_mut().skip(base) {
            *line = LineData::new(self.cols);
        }
    }

    fn line(&self, y: i32) -> Option<FakeLine> {
        let index = usize::try_from(y).ok()?;
        self.lines.borrow().get(index).map(|data| FakeLine { data: data.clone() })
    }
}

/// `terminal.buffer.active`.
pub struct FakeBuffer {
    screen: Rc<Screen>,
}

impl FakeBuffer {
    /// `buffer.cursorY`.
    pub fn cursor_y(&self) -> i32 {
        self.screen.cursor_y.get()
    }
}

impl platform::IBuffer for FakeBuffer {
    fn cursor_x(&self) -> i32 {
        self.screen.cursor_x.get()
    }

    fn cursor_y(&self) -> i32 {
        self.screen.cursor_y.get()
    }

    fn base_y(&self) -> i32 {
        self.screen.base_y.get()
    }

    fn get_line(&self, y: i32) -> Option<Rc<dyn platform::IBufferLine>> {
        self.screen.line(y).map(|line| Rc::new(line) as Rc<dyn platform::IBufferLine>)
    }
}

impl workbench::IBuffer for FakeBuffer {
    fn get_line(&self, y: i32) -> Option<Rc<dyn workbench::IBufferLine>> {
        self.screen.line(y).map(|line| Rc::new(line) as Rc<dyn workbench::IBufferLine>)
    }
}

/// Os tratadores registrados no `parser`.
#[derive(Default)]
struct ParserState {
    next_id: Cell<u64>,
    osc_handlers: RefCell<Vec<(u64, i32, platform::OscHandler)>>,
    csi_handlers: RefCell<Vec<(u64, char, platform::CsiHandler)>>,
}

impl ParserState {
    fn next_id(&self) -> u64 {
        self.next_id.set(self.next_id.get() + 1);
        self.next_id.get()
    }
}

/// `terminal.parser`.
pub struct FakeParser {
    state: Rc<ParserState>,
}

impl platform::IParser for FakeParser {
    fn register_osc_handler(&self, ident: i32, callback: platform::OscHandler) -> DisposableRef {
        let id = self.state.next_id();
        self.state.osc_handlers.borrow_mut().push((id, ident, callback));
        let state = self.state.clone();
        plain_disposable(move || state.osc_handlers.borrow_mut().retain(|(handler_id, ..)| *handler_id != id))
    }

    fn register_csi_handler(&self, final_byte: char, callback: platform::CsiHandler) -> DisposableRef {
        let id = self.state.next_id();
        self.state.csi_handlers.borrow_mut().push((id, final_byte, callback));
        let state = self.state.clone();
        plain_disposable(move || state.csi_handlers.borrow_mut().retain(|(handler_id, ..)| *handler_id != id))
    }
}

/// O emulador.
pub struct FakeXterm {
    screen: Rc<Screen>,
    parser: Rc<FakeParser>,
    on_data: Rc<Emitter<JsString>>,
    on_cursor_move: Rc<Emitter<()>>,
    on_write_parsed: Rc<Emitter<()>>,
    on_resize: Rc<Emitter<platform::TerminalResize>>,
    next_marker_id: Cell<u64>,
    deleted_lines: RefCell<Vec<(i32, i32)>>,
    /// Os marcadores registrados, descartados com o terminal (`Terminal.dispose()` descarta o buffer e os marcadores dele).
    markers: RefCell<Vec<Rc<FakeMarker>>>,
    /// `terminal.options`.
    options: RefCell<XtermOptions>,
}

impl IDisposable for FakeXterm {
    /// `terminal.dispose()`: descarta os marcadores registrados.
    fn dispose(&self) {
        for marker in self.markers.take() {
            marker.dispose();
        }
    }
}

impl FakeXterm {
    /// Um terminal sem limite de scrollback e com as opções padrão do xterm para o tamanho dado.
    pub fn new(cols: i32, rows: i32) -> Rc<Self> {
        Self::with_options(XtermOptions::defaults(cols, rows), None)
    }

    /// `new Terminal(options)`: o tamanho e o limite de scrollback vêm das opções.
    pub fn from_options(options: &XtermOptions) -> Rc<Self> {
        Self::with_options(options.clone(), Some(options.scrollback as i32))
    }

    fn with_options(options: XtermOptions, scrollback: Option<i32>) -> Rc<Self> {
        let (cols, rows) = (options.cols, options.rows);
        let screen = Rc::new(Screen {
            cols,
            rows,
            lines: RefCell::new(Vec::new()),
            cursor_x: Cell::new(0),
            cursor_y: Cell::new(0),
            base_y: Cell::new(0),
            scrollback,
            trimmed: Cell::new(0),
        });
        screen.ensure_lines(rows);
        Rc::new(Self {
            screen,
            parser: Rc::new(FakeParser { state: Rc::default() }),
            on_data: Emitter::new(),
            on_cursor_move: Emitter::new(),
            on_write_parsed: Emitter::new(),
            on_resize: Emitter::new(),
            next_marker_id: Cell::new(0),
            markers: RefCell::new(Vec::new()),
            deleted_lines: RefCell::new(Vec::new()),
            options: RefCell::new(options),
        })
    }

    /// `terminal.reset()`.
    pub fn reset(&self) {
        self.screen.reset();
    }

    /// `terminal.buffer.active` com os membros próprios do dublê.
    pub fn buffer(&self) -> FakeBuffer {
        FakeBuffer { screen: self.screen.clone() }
    }

    /// `terminal.buffer.active.cursorY`.
    pub fn cursor_y(&self) -> i32 {
        self.screen.cursor_y.get()
    }

    /// `terminal.write(data)`: o texto escrito é interpretado de forma síncrona.
    pub fn write(&self, data: &str) {
        let before = (self.screen.cursor_x.get(), self.screen.cursor_y.get());
        let chars: Vec<char> = data.chars().collect();
        let mut index = 0;
        while index < chars.len() {
            match chars[index] {
                '\u{1b}' if chars.get(index + 1) == Some(&']') => index = self.handle_osc(&chars, index),
                '\u{1b}' if chars.get(index + 1) == Some(&'[') => index = self.handle_csi(&chars, index),
                '\r' => self.screen.cursor_x.set(0),
                '\n' => self.screen.line_feed(false),
                c => self.screen.put_char(c),
            }
            index += 1;
        }
        let trimmed = self.screen.trimmed.take();
        if trimmed > 0 {
            let markers = self.markers.borrow().clone();
            for marker in &markers {
                marker.scroll_out(trimmed);
            }
        }
        if before != (self.screen.cursor_x.get(), self.screen.cursor_y.get()) {
            self.on_cursor_move.fire(());
        }
    }

    /// `writeP(xterm, data)`: escreve e espera o `parser` terminar (o evento `onWriteParsed` dispara depois).
    pub fn write_p(&self, data: &str) {
        self.write(data);
        self.on_write_parsed.fire(());
    }

    /// O que o usuário digita (o `onData` do xterm).
    pub fn fire_data(&self, data: &str) {
        self.on_data.fire(JsString::from(data));
    }

    /// Os pares `(index, amount)` que a heurística do Windows pediu para apagar.
    pub fn deleted_lines(&self) -> Vec<(i32, i32)> {
        self.deleted_lines.borrow().clone()
    }

    /// Trata a sequência OSC que começa em `start` (o `ESC`) e devolve o índice do último caractere consumido.
    fn handle_osc(&self, chars: &[char], start: usize) -> usize {
        let mut end = start + 2;
        while end < chars.len() {
            if chars[end] == '\u{7}' || (chars[end] == '\u{1b}' && chars.get(end + 1) == Some(&'\\')) {
                break;
            }
            end += 1;
        }
        if end >= chars.len() {
            return chars.len();
        }
        let payload: String = chars[start + 2..end].iter().collect();
        let (ident, data) = payload.split_once(';').unwrap_or((payload.as_str(), ""));
        if let Ok(ident) = ident.parse::<i32>() {
            let handlers: Vec<platform::OscHandler> =
                self.parser.state.osc_handlers.borrow().iter().filter(|(_, handler_ident, _)| *handler_ident == ident).map(|(_, _, handler)| handler.clone()).collect();
            for handler in handlers.iter().rev() {
                if handler(data) {
                    break;
                }
            }
        }
        if chars[end] == '\u{7}' { end } else { end + 1 }
    }

    /// Trata a sequência CSI que começa em `start` (o `ESC`) e devolve o índice do último caractere consumido.
    fn handle_csi(&self, chars: &[char], start: usize) -> usize {
        let mut end = start + 2;
        while end < chars.len() && !('@'..='~').contains(&chars[end]) {
            end += 1;
        }
        if end >= chars.len() {
            return chars.len();
        }
        let final_byte = chars[end];
        let params_text: String = chars[start + 2..end].iter().collect();
        let params: Vec<platform::CsiParam> = if params_text.is_empty() {
            Vec::new()
        } else {
            params_text.split(';').map(|param| platform::CsiParam::Number(param.parse().unwrap_or(0))).collect()
        };
        let handlers: Vec<platform::CsiHandler> =
            self.parser.state.csi_handlers.borrow().iter().filter(|(_, handler_final, _)| *handler_final == final_byte).map(|(_, _, handler)| handler.clone()).collect();
        let handled = handlers.iter().rev().any(|handler| handler(&params));
        if !handled && final_byte == 'J' && params.first().is_some_and(|param| param.is(2) || param.is(3)) {
            self.screen.clear_viewport();
        }
        end
    }
}

impl platform::IHeadlessTerminal for FakeXterm {
    fn buffer_active(&self) -> Rc<dyn platform::IBuffer> {
        Rc::new(self.buffer())
    }

    fn buffer_normal(&self) -> Rc<dyn platform::IBuffer> {
        Rc::new(self.buffer())
    }

    fn is_active_buffer_normal(&self) -> bool {
        true
    }

    fn cols(&self) -> i32 {
        self.screen.cols
    }

    fn parser(&self) -> Rc<dyn platform::IParser> {
        self.parser.clone()
    }

    fn register_marker(&self, cursor_y_offset: i32) -> Option<Rc<dyn IMarker>> {
        let line = self.screen.base_y.get() + self.screen.cursor_y.get() + cursor_y_offset;
        if line < 0 {
            return None;
        }
        self.next_marker_id.set(self.next_marker_id.get() + 1);
        let marker = FakeMarker::new(self.next_marker_id.get() as f64, f64::from(line));
        self.markers.borrow_mut().push(marker.clone());
        Some(marker)
    }

    fn scroll_on_erase_in_display(&self) -> bool {
        false
    }

    fn on_resize(&self) -> Event<platform::TerminalResize> {
        self.on_resize.event()
    }

    fn fire_lines_deleted(&self, index: i32, amount: i32) {
        self.deleted_lines.borrow_mut().push((index, amount));
    }

    fn rows(&self) -> i32 {
        self.screen.rows
    }

    fn on_cursor_move(&self) -> Event<()> {
        self.on_cursor_move.event()
    }

    fn on_data(&self) -> Event<JsString> {
        self.on_data.event()
    }

    fn on_write_parsed(&self) -> Event<()> {
        self.on_write_parsed.event()
    }
}

impl IXtermRaw for FakeXterm {
    fn options(&self) -> &RefCell<XtermOptions> {
        &self.options
    }

    fn buffer_length(&self) -> i32 {
        i32::try_from(self.screen.lines.borrow().len()).unwrap_or(i32::MAX)
    }

    fn write(&self, data: &str, callback: Option<Box<dyn FnOnce()>>) {
        self.write_p(data);
        if let Some(callback) = callback {
            callback();
        }
    }
}

impl workbench::ITerminal for FakeXterm {
    fn cols(&self) -> i32 {
        self.screen.cols
    }

    fn buffer_active(&self) -> Rc<dyn workbench::IBuffer> {
        Rc::new(self.buffer())
    }
}
