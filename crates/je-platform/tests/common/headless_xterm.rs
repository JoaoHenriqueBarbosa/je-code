//! Emulador de terminal sem tela (`Terminal` do `@xterm/headless`) dos testes de plataforma que escrevem sequências
//! reais no terminal e leem o buffer: [`HeadlessXterm`] implementa o contrato [`IHeadlessTerminal`] com a semântica
//! do xterm.js nos pontos que o `promptInputModel.test.ts` exercita.
//!
//! O que ele faz:
//! - Escreve texto célula a célula com a largura do `UnicodeV6` do xterm (`wcwidth`: Hangul e CJK largos, marcas
//!   combinantes de largura zero anexadas à célula anterior, emoji astral estreito), com a célula espaçadora de
//!   largura zero depois do caractere largo, quebra automática com `isWrapped` na linha seguinte, quebra pendente
//!   (o cursor fica em `cols` depois da última coluna) e rolagem da área visível (`baseY` cresce).
//! - Trata `CR`, `LF` (sem `CR`, e recuando o cursor pendente), `BS` e ignora os demais controles.
//! - Trata as sequências CSI `@` (ICH), `A`, `B`, `C`, `D` (movimentos com o cursor contido na tela), `G`, `H` e `f`
//!   (posicionamento), `J` (ED, de 0 a 2), `K` (EL), `X` (ECH) e `m` (SGR: negrito, esmaecido, itálico, sublinhado,
//!   piscante, inverso, invisível, tachado, linha em cima, e as cores da paleta de 16, de 256 e RGB), ignora as
//!   sequências com prefixo (`?25l`, `?25h`...) e entrega as OSC e as CSI aos tratadores do `parser`, do último
//!   registrado para o primeiro, até um devolver `true`. Um `ESC` dentro de uma OSC a encerra, como no xterm.
//! - Dispara `onData` ([`HeadlessXterm::input`]), `onCursorMove` (a posição do cursor mudou na escrita), `onWriteParsed`
//!   (depois de toda escrita) e `onResize`.
//!
//! O que ele não faz: o `ED 3` (limpar o scrollback), tabulação, modo de inserção, regiões de rolagem, charsets, o
//! limite de scrollback, o reflow no redimensionamento (as linhas são só cortadas ou completadas) e o buffer
//! alternativo.

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use je_base::event::{Emitter, Event};
use je_base::js_string::JsString;
use je_base::lifecycle::{DisposableRef, IDisposable, none, plain_disposable};
use je_platform::terminal::capabilities::capabilities::IMarker;
use je_platform::terminal::xterm_headless::{
    CsiHandler, CsiParam, IBuffer, IBufferCell, IBufferLine, IHeadlessTerminal, IParser, OscHandler, TerminalResize,
};

/// O `CM_P16` do xterm (`Attributes.CM_P16`).
const MODE_P16: i32 = 0x0100_0000;
/// O `CM_P256` do xterm.
const MODE_P256: i32 = 0x0200_0000;
/// O `CM_RGB` do xterm.
const MODE_RGB: i32 = 0x0300_0000;

fn idx(value: i32) -> usize {
    usize::try_from(value).unwrap_or(0)
}

/// `UnicodeV6.wcwidth` do xterm, nas faixas que o terminal de teste precisa.
fn char_width(c: char) -> u8 {
    let code = c as u32;
    if code < 32 {
        return 0;
    }
    if code < 127 {
        return 1;
    }
    if code < 160 {
        return 0;
    }
    const COMBINING: &[(u32, u32)] = &[(0x0300, 0x036F), (0x200B, 0x200F), (0x20D0, 0x20FF), (0xFE00, 0xFE0F), (0xFE20, 0xFE2F)];
    const WIDE: &[(u32, u32)] = &[
        (0x1100, 0x115F),
        (0x2329, 0x232A),
        (0x2E80, 0x303E),
        (0x3041, 0x33FF),
        (0x3400, 0x4DBF),
        (0x4E00, 0x9FFF),
        (0xA000, 0xA4CF),
        (0xAC00, 0xD7A3),
        (0xF900, 0xFAFF),
        (0xFE30, 0xFE6F),
        (0xFF00, 0xFF60),
        (0xFFE0, 0xFFE6),
        (0x20000, 0x2FFFD),
        (0x30000, 0x3FFFD),
    ];
    let within = |ranges: &[(u32, u32)]| ranges.iter().any(|&(start, end)| (start..=end).contains(&code));
    if within(COMBINING) {
        0
    } else if within(WIDE) {
        2
    } else {
        1
    }
}

/// O marcador de uma linha do buffer (`IMarker` do xterm).
pub struct HeadlessMarker {
    id: f64,
    line: Cell<f64>,
    disposed: Cell<bool>,
}

impl IDisposable for HeadlessMarker {
    fn dispose(&self) {
        if !self.disposed.replace(true) {
            self.line.set(-1.0);
        }
    }
}

impl IMarker for HeadlessMarker {
    fn id(&self) -> f64 {
        self.id
    }

    fn is_disposed(&self) -> bool {
        self.disposed.get()
    }

    fn line(&self) -> f64 {
        self.line.get()
    }

    /// O `onDispose` do xterm é um emissor do próprio xterm, fora do rastreador de descartáveis do VS Code: nenhum teste
    /// que usa este emulador espera o descarte de um marcador, então a inscrição não registra nada.
    fn on_dispose(&self) -> Event<()> {
        Event::new(|_listener| none())
    }
}

/// Os atributos de estilo de uma célula (`IAttributeData`).
#[derive(Clone, Copy, Default, PartialEq, Eq)]
struct Attr {
    fg_mode: i32,
    fg: i32,
    bg_mode: i32,
    bg: i32,
    bold: bool,
    dim: bool,
    italic: bool,
    underline: bool,
    blink: bool,
    inverse: bool,
    invisible: bool,
    strikethrough: bool,
    overline: bool,
}

impl Attr {
    /// `_eraseAttrData`: a célula apagada guarda só a cor de fundo do estilo atual.
    fn erase(self) -> Self {
        Self { bg_mode: self.bg_mode, bg: self.bg, ..Self::default() }
    }

    /// `charAttributes`: aplica os parâmetros de um SGR.
    fn apply_sgr(&mut self, params: &[i32]) {
        let params = if params.is_empty() { &[0][..] } else { params };
        let mut index = 0;
        while index < params.len() {
            let param = params[index];
            match param {
                0 => *self = Self::default(),
                1 => self.bold = true,
                2 => self.dim = true,
                3 => self.italic = true,
                4 => self.underline = true,
                5 => self.blink = true,
                7 => self.inverse = true,
                8 => self.invisible = true,
                9 => self.strikethrough = true,
                22 => {
                    self.bold = false;
                    self.dim = false;
                }
                23 => self.italic = false,
                24 => self.underline = false,
                25 => self.blink = false,
                27 => self.inverse = false,
                28 => self.invisible = false,
                29 => self.strikethrough = false,
                30..=37 => (self.fg_mode, self.fg) = (MODE_P16, param - 30),
                90..=97 => (self.fg_mode, self.fg) = (MODE_P16, param - 90 + 8),
                39 => (self.fg_mode, self.fg) = (0, 0),
                40..=47 => (self.bg_mode, self.bg) = (MODE_P16, param - 40),
                100..=107 => (self.bg_mode, self.bg) = (MODE_P16, param - 100 + 8),
                49 => (self.bg_mode, self.bg) = (0, 0),
                53 => self.overline = true,
                55 => self.overline = false,
                38 | 48 => {
                    let color = match (params.get(index + 1), params.get(index + 2), params.get(index + 3), params.get(index + 4)) {
                        (Some(5), Some(&palette), ..) => {
                            index += 2;
                            Some((MODE_P256, palette & 0xFF))
                        }
                        (Some(2), Some(&r), Some(&g), Some(&b)) => {
                            index += 4;
                            Some((MODE_RGB, ((r & 0xFF) << 16) | ((g & 0xFF) << 8) | (b & 0xFF)))
                        }
                        _ => None,
                    };
                    if let Some((mode, value)) = color {
                        if param == 38 {
                            (self.fg_mode, self.fg) = (mode, value);
                        } else {
                            (self.bg_mode, self.bg) = (mode, value);
                        }
                    }
                }
                _ => {}
            }
            index += 1;
        }
    }
}

/// Uma célula do buffer. A célula nula (nunca escrita ou apagada) e a espaçadora do caractere largo têm `chars` vazio; a
/// espaçadora tem largura 0.
#[derive(Clone)]
struct TermCell {
    chars: String,
    width: u8,
    attr: Attr,
}

impl TermCell {
    fn blank(attr: Attr) -> Self {
        Self { chars: String::new(), width: 1, attr }
    }
}

impl IBufferCell for TermCell {
    fn get_chars(&self) -> JsString {
        JsString::from(self.chars.as_str())
    }

    /// Sem conteúdo é 0; com marcas combinantes é a última unidade UTF-16 (`charCodeAt(length - 1)` da cadeia
    /// combinada); senão, o ponto de código.
    fn get_code(&self) -> i32 {
        let mut chars = self.chars.chars();
        match (chars.next(), chars.next()) {
            (None, _) => 0,
            (Some(first), None) => first as i32,
            _ => self.chars.encode_utf16().last().map_or(0, i32::from),
        }
    }

    fn get_fg_color(&self) -> i32 {
        if self.attr.fg_mode == 0 { -1 } else { self.attr.fg }
    }

    fn get_bg_color(&self) -> i32 {
        if self.attr.bg_mode == 0 { -1 } else { self.attr.bg }
    }

    fn get_fg_color_mode(&self) -> i32 {
        self.attr.fg_mode
    }

    fn get_bg_color_mode(&self) -> i32 {
        self.attr.bg_mode
    }

    fn is_bold(&self) -> u32 {
        u32::from(self.attr.bold)
    }

    fn is_italic(&self) -> u32 {
        u32::from(self.attr.italic)
    }

    fn is_dim(&self) -> u32 {
        u32::from(self.attr.dim)
    }

    fn is_underline(&self) -> u32 {
        u32::from(self.attr.underline)
    }

    fn is_blink(&self) -> u32 {
        u32::from(self.attr.blink)
    }

    fn is_inverse(&self) -> u32 {
        u32::from(self.attr.inverse)
    }

    fn is_invisible(&self) -> u32 {
        u32::from(self.attr.invisible)
    }

    fn is_strikethrough(&self) -> u32 {
        u32::from(self.attr.strikethrough)
    }

    fn is_overline(&self) -> u32 {
        u32::from(self.attr.overline)
    }
}

/// Uma linha do buffer (`BufferLine`).
#[derive(Clone)]
struct Line {
    cells: Vec<TermCell>,
    wrapped: bool,
}

impl Line {
    fn blank(cols: i32, attr: Attr) -> Self {
        Self { cells: vec![TermCell::blank(attr); idx(cols)], wrapped: false }
    }

    fn width_at(&self, index: i32) -> u8 {
        usize::try_from(index).ok().and_then(|index| self.cells.get(index)).map_or(1, |cell| cell.width)
    }

    /// `getTrimmedLength`: depois da última célula com conteúdo.
    fn trimmed_length(&self) -> i32 {
        self.cells
            .iter()
            .rposition(|cell| !cell.chars.is_empty())
            .map_or(0, |index| i32::try_from(index).unwrap_or(i32::MAX) + i32::from(self.cells[index].width))
    }

    /// `replaceCells(start, end, fill)`.
    fn replace_cells(&mut self, start: i32, end: i32, fill: &TermCell) {
        let length = i32::try_from(self.cells.len()).unwrap_or(i32::MAX);
        if start > 0 && self.width_at(start - 1) == 2 {
            self.cells[idx(start - 1)] = fill.clone();
        }
        if end > 0 && end < length && self.width_at(end - 1) == 2 {
            self.cells[idx(end)] = fill.clone();
        }
        let mut position = start;
        while position < end && position < length {
            self.cells[idx(position)] = fill.clone();
            position += 1;
        }
    }

    /// `insertCells(pos, n, fill)`.
    fn insert_cells(&mut self, position: i32, count: i32, fill: &TermCell) {
        let length = i32::try_from(self.cells.len()).unwrap_or(i32::MAX);
        let position = position % length;
        if position > 0 && self.width_at(position - 1) == 2 {
            self.cells[idx(position - 1)] = fill.clone();
        }
        if count < length - position {
            for index in (position + count..length).rev() {
                self.cells[idx(index)] = self.cells[idx(index - count)].clone();
            }
            for index in position..position + count {
                self.cells[idx(index)] = fill.clone();
            }
        } else {
            for index in position..length {
                self.cells[idx(index)] = fill.clone();
            }
        }
        if self.width_at(length - 1) == 2 {
            self.cells[idx(length - 1)] = fill.clone();
        }
    }

    /// `translateToString(trimRight, startColumn, endColumn)`: a célula sem conteúdo sai como espaço e o caractere
    /// largo avança duas colunas.
    fn translate_to_string(&self, trim_right: bool, start_column: Option<i32>, end_column: Option<i32>) -> String {
        let length = i32::try_from(self.cells.len()).unwrap_or(i32::MAX);
        let mut column = start_column.unwrap_or(0);
        let mut end = end_column.unwrap_or(length);
        if trim_right {
            end = end.min(self.trimmed_length());
        }
        let mut text = String::new();
        while column < end {
            let cell = usize::try_from(column).ok().and_then(|index| self.cells.get(index));
            match cell {
                Some(cell) if !cell.chars.is_empty() => text.push_str(&cell.chars),
                _ => text.push(' '),
            }
            column += cell.map_or(0, |cell| i32::from(cell.width)).max(1);
        }
        text
    }
}

/// Uma linha do buffer vista pelo contrato (um retrato do conteúdo na hora da leitura).
struct HeadlessLine(Line);

impl IBufferLine for HeadlessLine {
    fn is_wrapped(&self) -> bool {
        self.0.wrapped
    }

    fn length(&self) -> i32 {
        i32::try_from(self.0.cells.len()).unwrap_or(i32::MAX)
    }

    fn get_cell(&self, x: i32) -> Option<Rc<dyn IBufferCell>> {
        let cell = self.0.cells.get(usize::try_from(x).ok()?)?;
        Some(Rc::new(cell.clone()))
    }

    fn translate_to_string(&self, trim_right: Option<bool>, start_column: Option<i32>, end_column: Option<i32>) -> JsString {
        JsString::from(self.0.translate_to_string(trim_right.unwrap_or(false), start_column, end_column).as_str())
    }
}

/// O estado da tela: todas as linhas (o scrollback e a área visível), o cursor e o estilo atual.
struct State {
    cols: i32,
    rows: i32,
    lines: Vec<Line>,
    x: i32,
    y: i32,
    base_y: i32,
    attr: Attr,
    markers: Vec<Rc<HeadlessMarker>>,
    next_marker_id: f64,
}

impl State {
    fn absolute_y(&self) -> usize {
        idx(self.base_y + self.y)
    }

    /// `_restrictCursor`.
    fn restrict_cursor(&mut self) {
        self.x = self.x.clamp(0, self.cols - 1);
        self.y = self.y.clamp(0, self.rows - 1);
    }

    /// `_moveCursor` (CUU, CUD, CUF e CUB).
    fn move_cursor(&mut self, dx: i32, dy: i32) {
        self.restrict_cursor();
        self.x += dx;
        self.y += dy;
        self.restrict_cursor();
    }

    /// `bufferService.scroll(eraseAttr)`: uma linha nova embaixo, e a área visível desce uma linha no buffer.
    fn scroll(&mut self) {
        self.lines.push(Line::blank(self.cols, self.attr.erase()));
        self.base_y += 1;
    }

    /// `lineFeed`: sem retorno de carro; recua o cursor se ele estava na quebra pendente.
    fn line_feed(&mut self) {
        if self.y == self.rows - 1 {
            self.scroll();
        } else {
            self.y += 1;
        }
        if self.x >= self.cols {
            self.x -= 1;
        }
    }

    /// `_eraseInBufferLine(y, start, end, clearWrap)`.
    fn erase_in_line(&mut self, y: i32, start: i32, end: i32, clear_wrap: bool) {
        let fill = TermCell::blank(self.attr.erase());
        let line = &mut self.lines[idx(self.base_y + y)];
        line.replace_cells(start, end, &fill);
        if clear_wrap {
            line.wrapped = false;
        }
    }

    /// `_resetBufferLine(y)`.
    fn reset_line(&mut self, y: i32) {
        let blank = Line::blank(self.cols, self.attr.erase());
        self.lines[idx(self.base_y + y)] = blank;
    }

    /// `print` para um caractere: a quebra automática, a escrita da célula e a anexação das marcas combinantes.
    fn print(&mut self, c: char) {
        let width = char_width(c);
        if width == 0 {
            if self.x > 0 {
                let absolute = self.absolute_y();
                let line = &mut self.lines[absolute];
                let offset = if line.width_at(self.x - 1) == 0 { 2 } else { 1 };
                let cell = &mut line.cells[idx(self.x - offset)];
                if cell.chars.is_empty() {
                    cell.width = 1;
                }
                cell.chars.push(c);
            }
            return;
        }
        let attr = self.attr;
        // Escrever sobre a segunda metade de um caractere largo desfaz a primeira.
        if self.x > 0 {
            let absolute = self.absolute_y();
            if self.lines[absolute].width_at(self.x - 1) == 2 {
                self.lines[absolute].cells[idx(self.x - 1)] = TermCell::blank(attr);
            }
        }
        // A quebra automática (DECAWM) quando o caractere não cabe na linha.
        if self.x + i32::from(width) - 1 >= self.cols {
            let absolute = self.absolute_y();
            while self.x < self.cols {
                self.lines[absolute].cells[idx(self.x)] = TermCell::blank(attr);
                self.x += 1;
            }
            self.x = 0;
            self.y += 1;
            if self.y == self.rows {
                self.y -= 1;
                self.scroll();
            }
            let absolute = self.absolute_y();
            self.lines[absolute].wrapped = true;
        }
        let absolute = self.absolute_y();
        let x = idx(self.x);
        let line = &mut self.lines[absolute];
        line.cells[x] = TermCell { chars: c.to_string(), width, attr };
        if width == 2 {
            line.cells[x + 1] = TermCell { chars: String::new(), width: 0, attr };
        }
        self.x += i32::from(width);
    }
}

/// O buffer ativo (`terminal.buffer.active`), lendo o estado vivo do terminal.
pub struct HeadlessBuffer {
    state: Rc<RefCell<State>>,
}

impl IBuffer for HeadlessBuffer {
    fn cursor_x(&self) -> i32 {
        self.state.borrow().x
    }

    fn cursor_y(&self) -> i32 {
        self.state.borrow().y
    }

    fn base_y(&self) -> i32 {
        self.state.borrow().base_y
    }

    fn get_line(&self, y: i32) -> Option<Rc<dyn IBufferLine>> {
        let index = usize::try_from(y).ok()?;
        let line = self.state.borrow().lines.get(index)?.clone();
        Some(Rc::new(HeadlessLine(line)))
    }
}

/// Os tratadores registrados no `parser`.
#[derive(Default)]
struct ParserState {
    next_id: Cell<u64>,
    osc_handlers: RefCell<Vec<(u64, i32, OscHandler)>>,
    csi_handlers: RefCell<Vec<(u64, char, CsiHandler)>>,
}

impl ParserState {
    fn next_id(&self) -> u64 {
        self.next_id.set(self.next_id.get() + 1);
        self.next_id.get()
    }
}

/// `terminal.parser`.
pub struct HeadlessParser {
    state: Rc<ParserState>,
}

impl IParser for HeadlessParser {
    fn register_osc_handler(&self, ident: i32, callback: OscHandler) -> DisposableRef {
        let id = self.state.next_id();
        self.state.osc_handlers.borrow_mut().push((id, ident, callback));
        let state = self.state.clone();
        plain_disposable(move || state.osc_handlers.borrow_mut().retain(|(handler_id, ..)| *handler_id != id))
    }

    fn register_csi_handler(&self, final_byte: char, callback: CsiHandler) -> DisposableRef {
        let id = self.state.next_id();
        self.state.csi_handlers.borrow_mut().push((id, final_byte, callback));
        let state = self.state.clone();
        plain_disposable(move || state.csi_handlers.borrow_mut().retain(|(handler_id, ..)| *handler_id != id))
    }
}

/// O emulador (`new Terminal({ allowProposedApi: true })` do `@xterm/headless`): 80 colunas por 24 linhas.
pub struct HeadlessXterm {
    state: Rc<RefCell<State>>,
    parser_state: Rc<ParserState>,
    parser: Rc<HeadlessParser>,
    on_data: Rc<Emitter<JsString>>,
    on_cursor_move: Rc<Emitter<()>>,
    on_write_parsed: Rc<Emitter<()>>,
    on_resize: Rc<Emitter<TerminalResize>>,
}

impl HeadlessXterm {
    pub fn new() -> Rc<Self> {
        let (cols, rows) = (80, 24);
        let state = State {
            cols,
            rows,
            lines: vec![Line::blank(cols, Attr::default()); idx(rows)],
            x: 0,
            y: 0,
            base_y: 0,
            attr: Attr::default(),
            markers: Vec::new(),
            next_marker_id: 0.0,
        };
        let parser_state = Rc::new(ParserState::default());
        Rc::new(Self {
            state: Rc::new(RefCell::new(state)),
            parser: Rc::new(HeadlessParser { state: parser_state.clone() }),
            parser_state,
            on_data: Emitter::new(),
            on_cursor_move: Emitter::new(),
            on_write_parsed: Emitter::new(),
            on_resize: Emitter::new(),
        })
    }

    /// `terminal.buffer.active`.
    pub fn buffer(&self) -> HeadlessBuffer {
        HeadlessBuffer { state: self.state.clone() }
    }

    /// `terminal.resize(columns, rows)`: corta ou completa as linhas, sem reflow.
    pub fn resize(&self, cols: i32, rows: i32) {
        {
            let mut state = self.state.borrow_mut();
            let blank = Line::blank(cols, Attr::default());
            for line in &mut state.lines {
                line.cells.resize(idx(cols), TermCell::blank(Attr::default()));
            }
            let total = idx(state.base_y + rows);
            state.lines.resize(total, blank);
            state.cols = cols;
            state.rows = rows;
            state.restrict_cursor();
        }
        self.on_resize.fire(TerminalResize { cols, rows });
    }

    /// `terminal.input(data, true)`: o que o usuário digita (o `onData` do xterm).
    pub fn input(&self, data: &str) {
        self.on_data.fire(JsString::from(data));
    }

    /// `terminal.write(data, callback)`: interpreta o texto e depois avisa o cursor e o `onWriteParsed`.
    pub fn write(&self, data: &str) {
        let before = {
            let state = self.state.borrow();
            (state.x, state.y)
        };
        let chars: Vec<char> = data.chars().collect();
        let mut index = 0;
        while index < chars.len() {
            match chars[index] {
                '\u{1b}' => index = self.escape(&chars, index),
                '\r' => self.state.borrow_mut().x = 0,
                '\n' => self.state.borrow_mut().line_feed(),
                '\u{8}' => {
                    let mut state = self.state.borrow_mut();
                    state.restrict_cursor();
                    if state.x > 0 {
                        state.x -= 1;
                    }
                }
                control if (control as u32) < 0x20 || control == '\u{7f}' => {}
                c => self.state.borrow_mut().print(c),
            }
            index += 1;
        }
        let after = {
            let state = self.state.borrow();
            (state.x, state.y)
        };
        if before != after {
            self.on_cursor_move.fire(());
        }
        self.on_write_parsed.fire(());
    }

    /// Trata a sequência que começa no `ESC` de `start` e devolve o índice do último caractere consumido.
    fn escape(&self, chars: &[char], start: usize) -> usize {
        match chars.get(start + 1) {
            Some(']') => self.osc(chars, start),
            Some('[') => self.csi(chars, start),
            Some(_) => start + 1,
            None => start,
        }
    }

    /// Uma OSC termina no `BEL`, no `ESC \` ou num `ESC` qualquer (que o xterm trata como o fim da OSC e o começo de
    /// outra sequência).
    fn osc(&self, chars: &[char], start: usize) -> usize {
        let mut end = start + 2;
        while end < chars.len() && chars[end] != '\u{7}' && chars[end] != '\u{1b}' {
            end += 1;
        }
        let payload: String = chars[start + 2..end].iter().collect();
        let (ident, data) = payload.split_once(';').unwrap_or((payload.as_str(), ""));
        if let Ok(ident) = ident.parse::<i32>() {
            let handlers: Vec<OscHandler> =
                self.parser_state.osc_handlers.borrow().iter().filter(|(_, handler_ident, _)| *handler_ident == ident).map(|(_, _, handler)| handler.clone()).collect();
            let _handled = handlers.iter().rev().any(|handler| handler(data));
        }
        match chars.get(end) {
            None => chars.len() - 1,
            Some('\u{7}') => end,
            Some(_) if chars.get(end + 1) == Some(&'\\') => end + 1,
            Some(_) => end - 1,
        }
    }

    fn csi(&self, chars: &[char], start: usize) -> usize {
        let mut index = start + 2;
        let prefix = chars.get(index).copied().filter(|c| matches!(c, '<' | '=' | '>' | '?'));
        if prefix.is_some() {
            index += 1;
        }
        let params_start = index;
        while index < chars.len() && (chars[index].is_ascii_digit() || chars[index] == ';' || chars[index] == ':') {
            index += 1;
        }
        let params_text: String = chars[params_start..index].iter().collect();
        let intermediates_start = index;
        while index < chars.len() && ('\u{20}'..='\u{2f}').contains(&chars[index]) {
            index += 1;
        }
        let Some(&final_byte) = chars.get(index) else {
            return chars.len() - 1;
        };
        if !('@'..='~').contains(&final_byte) {
            return start + 1;
        }
        if prefix.is_some() || index > intermediates_start {
            return index;
        }
        let number = |text: &str| text.parse::<i32>().unwrap_or(0);
        let params: Vec<CsiParam> = if params_text.is_empty() {
            Vec::new()
        } else {
            params_text
                .split(';')
                .map(|part| if part.contains(':') { CsiParam::SubParams(part.split(':').map(number).collect()) } else { CsiParam::Number(number(part)) })
                .collect()
        };
        let handlers: Vec<CsiHandler> =
            self.parser_state.csi_handlers.borrow().iter().filter(|(_, handler_final, _)| *handler_final == final_byte).map(|(_, _, handler)| handler.clone()).collect();
        if !handlers.iter().rev().any(|handler| handler(&params)) {
            let numbers: Vec<i32> = params
                .iter()
                .map(|param| match param {
                    CsiParam::Number(value) => *value,
                    CsiParam::SubParams(values) => values.first().copied().unwrap_or(0),
                })
                .collect();
            self.csi_builtin(final_byte, &numbers);
        }
        index
    }

    /// O tratamento padrão das sequências CSI que o terminal de teste entende.
    fn csi_builtin(&self, final_byte: char, numbers: &[i32]) {
        let count = numbers.first().copied().filter(|value| *value > 0).unwrap_or(1);
        let param_or = |position: usize, default: i32| numbers.get(position).copied().filter(|value| *value > 0).unwrap_or(default);
        let mode = numbers.first().copied().unwrap_or(0);
        let mut state = self.state.borrow_mut();
        match final_byte {
            '@' => {
                state.restrict_cursor();
                let fill = TermCell::blank(state.attr.erase());
                let (x, absolute) = (state.x, state.absolute_y());
                state.lines[absolute].insert_cells(x, count, &fill);
            }
            'A' => state.move_cursor(0, -count),
            'B' => state.move_cursor(0, count),
            'C' => state.move_cursor(count, 0),
            'D' => state.move_cursor(-count, 0),
            'G' => {
                state.x = count - 1;
                state.restrict_cursor();
            }
            'H' | 'f' => {
                state.y = param_or(0, 1) - 1;
                state.x = param_or(1, 1) - 1;
                state.restrict_cursor();
            }
            'J' => {
                let (x, y, cols, rows) = (state.x, state.y, state.cols, state.rows);
                match mode {
                    0 => {
                        state.erase_in_line(y, x, cols, x == 0);
                        for row in y + 1..rows {
                            state.reset_line(row);
                        }
                    }
                    1 => {
                        state.erase_in_line(y, 0, x + 1, true);
                        if x + 1 >= cols && y + 1 < rows {
                            let next = idx(state.base_y + y + 1);
                            state.lines[next].wrapped = false;
                        }
                        for row in 0..y {
                            state.reset_line(row);
                        }
                    }
                    2 => {
                        for row in 0..rows {
                            state.reset_line(row);
                        }
                    }
                    _ => {}
                }
            }
            'K' => {
                let (x, y, cols) = (state.x, state.y, state.cols);
                match mode {
                    0 => state.erase_in_line(y, x, cols, x == 0),
                    1 => state.erase_in_line(y, 0, x + 1, false),
                    2 => state.erase_in_line(y, 0, cols, true),
                    _ => {}
                }
            }
            'X' => {
                state.restrict_cursor();
                let fill = TermCell::blank(state.attr.erase());
                let (x, absolute) = (state.x, state.absolute_y());
                state.lines[absolute].replace_cells(x, x + count, &fill);
            }
            'm' => state.attr.apply_sgr(numbers),
            _ => {}
        }
    }
}

impl IDisposable for HeadlessXterm {
    /// `terminal.dispose()`: descarta os marcadores e os emissores.
    fn dispose(&self) {
        let markers = std::mem::take(&mut self.state.borrow_mut().markers);
        for marker in markers {
            marker.dispose();
        }
        self.on_data.dispose();
        self.on_cursor_move.dispose();
        self.on_write_parsed.dispose();
        self.on_resize.dispose();
    }
}

impl IHeadlessTerminal for HeadlessXterm {
    fn buffer_active(&self) -> Rc<dyn IBuffer> {
        Rc::new(self.buffer())
    }

    fn buffer_normal(&self) -> Rc<dyn IBuffer> {
        Rc::new(self.buffer())
    }

    fn is_active_buffer_normal(&self) -> bool {
        true
    }

    fn cols(&self) -> i32 {
        self.state.borrow().cols
    }

    fn parser(&self) -> Rc<dyn IParser> {
        self.parser.clone()
    }

    fn register_marker(&self, cursor_y_offset: i32) -> Option<Rc<dyn IMarker>> {
        let mut state = self.state.borrow_mut();
        let line = state.base_y + state.y + cursor_y_offset;
        if line < 0 {
            return None;
        }
        state.next_marker_id += 1.0;
        let marker = Rc::new(HeadlessMarker { id: state.next_marker_id, line: Cell::new(f64::from(line)), disposed: Cell::new(false) });
        state.markers.push(marker.clone());
        Some(marker)
    }

    fn scroll_on_erase_in_display(&self) -> bool {
        false
    }

    fn on_resize(&self) -> Event<TerminalResize> {
        self.on_resize.event()
    }

    /// O `lines.onDeleteEmitter.fire({ index, amount })`: os marcadores dentro da faixa são descartados e os de depois
    /// sobem `amount` linhas.
    fn fire_lines_deleted(&self, index: i32, amount: i32) {
        let markers = self.state.borrow().markers.clone();
        for marker in markers {
            let line = marker.line.get();
            if line >= f64::from(index) && line < f64::from(index + amount) {
                marker.dispose();
            }
            let line = marker.line.get();
            if line > f64::from(index) {
                marker.line.set(line - f64::from(amount));
            }
        }
    }

    fn rows(&self) -> i32 {
        self.state.borrow().rows
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
