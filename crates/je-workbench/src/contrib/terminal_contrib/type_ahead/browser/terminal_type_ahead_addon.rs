//! Porte de `src/vs/workbench/contrib/terminalContrib/typeAhead/browser/terminalTypeAheadAddon.ts`: o eco local (type ahead)
//! do terminal. O addon prevê o que o PTY vai devolver para as teclas do usuário, escreve a previsão no terminal antes da
//! resposta e a confirma ou desfaz quando os dados do processo chegam.
//!
//! Decisões de tradução:
//! - **O `Terminal` do xterm.** O original lê `terminal.cols/rows/write/onData/onTitleChange/onResize`,
//!   `terminal.buffer.active` (`type`, `baseY`, `cursorX`, `cursorY`, `getLine`, `getCell`), `terminal.parser.registerCsiHandler` e o
//!   interno `_core._inputHandler._curAttrData`. Os traits [`ITypeAheadTerminal`], [`ITypeAheadBuffer`], [`ITypeAheadLine`] e
//!   [`ITypeAheadCell`] são o recorte exato disso (o contrato do emulador do porte, `je_platform::terminal::xterm_headless`, ainda
//!   não tem o cursor, a escrita nem os atributos de estilo), e [`IXtermAttributes`] é o `XtermAttributes` de `xterm-private.ts`.
//!   Os métodos `isX(): number` que o original usa como booleano são `bool`.
//! - **`ITerminalProcessManager`.** O addon só usa `onBeforeProcessData`; o construtor recebe esse evento.
//! - **`Date.now()` e `setTimeout`.** São [`runtime::date_now`] e [`runtime::set_timeout`] (o relógio virtual vale nos testes).
//! - **`@debounce`.** É o [`Debounce`] de `je_base::decorators`, guardado como campo.
//! - **Índices.** Os textos que o leitor ([`StringReader`]) percorre são UTF-16, como o `string` do original; os tipos de saída são
//!   `String`.
//! - **`NewlinePrediction` e `LinewrapPrediction`.** A segunda estende a primeira só trocando `apply` e `matches`; no porte é uma
//!   struct com o campo `linewrap`.
//! - **`instanceof`.** Os predicados que o original testa com `instanceof` (`CharacterPrediction`, `TentativeBoundary`) usam
//!   [`IPrediction::as_any`].
//! - **`StringReader.eatStr`** devolve se casou com um texto não vazio, que é o uso do original (só em condições), e preserva o
//!   `slice(index, substr.length)` do original, que só compara certo com `index == 0`.
//! - **Membros `protected` que o teste do original expõe pela subclasse `TestTypeAheadAddon`** são públicos aqui
//!   ([`TypeAheadAddon::set_last_row`], [`TypeAheadAddon::reevaluate_predictor_state_now`], [`TypeAheadAddon::timeline`]).
//! - O timer do envio das estatísticas é cancelado no `dispose` (o `setTimeout` solto do original dispararia sobre o addon já
//!   descartado).

use std::any::Any;
use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::rc::{Rc, Weak};

use je_base::async_::disposable_timeout;
use je_base::decorators::Debounce;
use je_base::dynamic::Dynamic;
use je_base::event::{Emitter, Event};
use je_base::js_regexp::JsRegExp;
use je_base::js_string::JsString;
use je_base::lifecycle::{Disposable, DisposableRef, DisposableStore, IDisposable, id_of, plain_disposable};
use je_base::runtime::{self, TimerId};
use je_base::strings::escape_reg_exp_characters;
use je_platform::configuration::service::IConfigurationService;
use je_platform::telemetry::telemetry::{ITelemetryData, ITelemetryService};
use je_theme::color::Color;

use crate::contrib::terminal::common::terminal::{IBeforeProcessDataEvent, TERMINAL_CONFIG_SECTION};
use crate::contrib::terminal_contrib::type_ahead::common::terminal_type_ahead_configuration::{DEFAULT_LOCAL_ECHO_EXCLUDE, ITerminalTypeAheadConfiguration};

// ---------------------------------------------------------------------------------------------------------------------
// O recorte do xterm que o addon lê
// ---------------------------------------------------------------------------------------------------------------------

/// `XtermAttributes` (`xterm-private.ts`): os atributos de estilo de uma célula ou do cursor.
pub trait IXtermAttributes {
    fn is_attribute_default(&self) -> bool;
    fn is_bold(&self) -> bool;
    fn is_dim(&self) -> bool;
    fn is_italic(&self) -> bool;
    fn is_underline(&self) -> bool;
    fn is_blink(&self) -> bool;
    fn is_inverse(&self) -> bool;
    fn is_invisible(&self) -> bool;
    fn is_fg_rgb(&self) -> bool;
    fn is_fg_palette(&self) -> bool;
    fn is_fg_default(&self) -> bool;
    fn get_fg_color(&self) -> u32;
    fn is_bg_rgb(&self) -> bool;
    fn is_bg_palette(&self) -> bool;
    fn is_bg_default(&self) -> bool;
    fn get_bg_color(&self) -> u32;
}

/// `IBufferCell` do xterm, com os atributos.
pub trait ITypeAheadCell: IXtermAttributes {
    fn get_chars(&self) -> String;
    fn get_code(&self) -> u32;
}

/// `IBufferLine` do xterm.
pub trait ITypeAheadLine {
    /// `translateToString(trimRight?, startColumn?, endColumn?)`.
    fn translate_to_string(&self, trim_right: Option<bool>, start_column: Option<i32>, end_column: Option<i32>) -> String;
    /// `getCell(x)`.
    fn get_cell(&self, x: i32) -> Option<Rc<dyn ITypeAheadCell>>;
}

/// `IBuffer` do xterm (`terminal.buffer.active`).
pub trait ITypeAheadBuffer {
    /// `type === 'normal'`.
    fn is_normal(&self) -> bool;
    fn base_y(&self) -> i32;
    fn cursor_x(&self) -> i32;
    fn cursor_y(&self) -> i32;
    fn get_line(&self, y: i32) -> Option<Rc<dyn ITypeAheadLine>>;
}

/// Um parâmetro de CSI: `SingleOrMany<number>`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SgrParam {
    Single(i64),
    Many(Vec<i64>),
}

impl SgrParam {
    /// O texto que `Array#join` escreve: um vetor aninhado vira `a,b`.
    fn to_js_string(&self) -> String {
        match self {
            Self::Single(number) => number.to_string(),
            Self::Many(numbers) => numbers.iter().map(i64::to_string).collect::<Vec<_>>().join(","),
        }
    }

    /// `isNumber(v) ? v : v[0]`.
    fn first(&self) -> i64 {
        match self {
            Self::Single(number) => *number,
            Self::Many(numbers) => numbers.first().copied().unwrap_or(0),
        }
    }
}

/// O `Terminal` do xterm, só com o que o addon lê.
pub trait ITypeAheadTerminal {
    fn cols(&self) -> i32;
    fn rows(&self) -> i32;
    fn buffer_active(&self) -> Rc<dyn ITypeAheadBuffer>;
    fn write(&self, data: &str);
    fn on_data(&self) -> Event<String>;
    fn on_title_change(&self) -> Event<String>;
    fn on_resize(&self) -> Event<()>;
    /// `terminal.parser.registerCsiHandler({ final: 'm' }, handler)`: o tratador devolve `true` quando consumiu a sequência.
    fn register_csi_handler_sgr(&self, handler: Rc<dyn Fn(&[SgrParam]) -> bool>) -> DisposableRef;
    /// `terminal._core._inputHandler._curAttrData`.
    fn cur_attr_data(&self) -> Rc<dyn IXtermAttributes>;
}

// ---------------------------------------------------------------------------------------------------------------------
// Constantes
// ---------------------------------------------------------------------------------------------------------------------

// `const enum VT`
const VT_ESC: &str = "\x1b";
const VT_CSI: &str = "\x1b[";
const VT_SHOW_CURSOR: &str = "\x1b[?25h";
const VT_HIDE_CURSOR: &str = "\x1b[?25l";
const VT_DELETE_CHAR: &str = "\x1b[X";
const VT_DELETE_REST_OF_LINE: &str = "\x1b[K";

macro_rules! regex_const {
    ($name:ident, $pattern:expr) => {
        fn $name() -> Rc<JsRegExp> {
            thread_local! {
                static REGEX: Rc<JsRegExp> = Rc::new(JsRegExp::from_str($pattern, "").expect("padrão do type ahead válido"));
            }
            REGEX.with(Rc::clone)
        }
    };
}

regex_const!(csi_style_re, r"^\x1b\[[0-9;]*m");
regex_const!(csi_move_re, r"^\x1b\[?([0-9]*)(;[35])?O?([DC])");

/// Codes that should be omitted from sending to the prediction engine and instead omitted directly:
/// - Hide cursor (DECTCEM): We wrap the local echo sequence in hide and show
///   CSI ? 2 5 l
/// - Show cursor (DECTCEM): We wrap the local echo sequence in hide and show
///   CSI ? 2 5 h
/// - Device Status Report (DSR): These sequence fire report events from xterm which could cause
///   double reporting and potentially a stack overflow (#119472)
///   CSI Ps n
///   CSI ? Ps n
fn prediction_omit_re() -> Rc<JsRegExp> {
    thread_local! {
        static REGEX: Rc<JsRegExp> = Rc::new(JsRegExp::from_str(r"^(\x1b\[(\??25[hl]|\??[0-9;]+n))+", "").expect("padrão do type ahead válido"));
    }
    REGEX.with(Rc::clone)
}

// `const enum StatsConstants`
const STATS_BUFFER_SIZE: usize = 24;
const STATS_SEND_TELEMETRY_EVERY: u64 = 1000 * 60 * 5; // how often to collect stats
const STATS_MIN_SAMPLES_TO_TURN_ON: f64 = 5.0;
const STATS_MIN_ACCURACY_TO_TURN_ON: f64 = 0.3;
const STATS_TOGGLE_OFF_THRESHOLD: f64 = 0.5; // if latency is less than `threshold * this`, turn off

fn units(text: &str) -> Vec<u16> {
    text.encode_utf16().collect()
}

fn text_of(units: &[u16]) -> String {
    String::from_utf16_lossy(units)
}

/// `const enum CursorMoveDirection`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum CursorMoveDirection {
    Back,
    Forwards,
}

impl CursorMoveDirection {
    fn letter(self) -> &'static str {
        match self {
            Self::Back => "D",
            Self::Forwards => "C",
        }
    }
}

// ---------------------------------------------------------------------------------------------------------------------
// Cursor
// ---------------------------------------------------------------------------------------------------------------------

/// `ICoordinate`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ICoordinate {
    pub x: i32,
    pub y: i32,
    pub base_y: i32,
}

/// `Cursor`.
pub struct Cursor {
    x: Cell<i32>,
    y: Cell<i32>,
    base_y: Cell<i32>,
    pub rows: i32,
    pub cols: i32,
    buffer: Rc<dyn ITypeAheadBuffer>,
}

impl Cursor {
    pub fn new(rows: i32, cols: i32, buffer: Rc<dyn ITypeAheadBuffer>) -> Self {
        Self { x: Cell::new(buffer.cursor_x()), y: Cell::new(buffer.cursor_y()), base_y: Cell::new(buffer.base_y()), rows, cols, buffer }
    }

    pub fn x(&self) -> i32 {
        self.x.get()
    }

    pub fn y(&self) -> i32 {
        self.y.get()
    }

    pub fn base_y(&self) -> i32 {
        self.base_y.get()
    }

    pub fn coordinate(&self) -> ICoordinate {
        ICoordinate { x: self.x.get(), y: self.y.get(), base_y: self.base_y.get() }
    }

    pub fn get_line(&self) -> Option<Rc<dyn ITypeAheadLine>> {
        self.buffer.get_line(self.y.get() + self.base_y.get())
    }

    pub fn get_cell(&self) -> Option<Rc<dyn ITypeAheadCell>> {
        self.get_line()?.get_cell(self.x.get())
    }

    pub fn move_to(&self, coordinate: ICoordinate) -> String {
        self.x.set(coordinate.x);
        self.y.set((coordinate.y + coordinate.base_y) - self.base_y.get());
        self.move_instruction()
    }

    /// `clone()`.
    pub fn clone_cursor(&self) -> Cursor {
        let c = Cursor::new(self.rows, self.cols, self.buffer.clone());
        c.move_to(self.coordinate());
        c
    }

    pub fn move_(&self, x: i32, y: i32) -> String {
        self.x.set(x);
        self.y.set(y);
        self.move_instruction()
    }

    pub fn shift(&self, x: i32, y: i32) -> String {
        self.x.set(self.x.get() + x);
        self.y.set(self.y.get() + y);
        self.move_instruction()
    }

    pub fn move_instruction(&self) -> String {
        if self.y.get() >= self.rows {
            self.base_y.set(self.base_y.get() + self.y.get() - (self.rows - 1));
            self.y.set(self.rows - 1);
        } else if self.y.get() < 0 {
            self.base_y.set(self.base_y.get() - self.y.get());
            self.y.set(0);
        }

        format!("{VT_CSI}{};{}H", self.y.get() + 1, self.x.get() + 1)
    }
}

fn move_to_word_boundary(cursor: &Cursor, direction: i32) {
    let mut ate_leading_whitespace = false;
    if direction < 0 {
        cursor.shift(-1, 0);
    }

    while cursor.x() >= 0 {
        let cell = cursor.get_cell();
        let Some(cell) = cell.filter(|cell| cell.get_code() != 0) else {
            return;
        };

        let chars = JsString::from(cell.get_chars().as_str());
        let not_word = not_word_re_test(&chars);
        if not_word {
            if ate_leading_whitespace {
                break;
            }
        } else {
            ate_leading_whitespace = true;
        }

        cursor.shift(direction, 0);
    }

    if direction < 0 {
        cursor.shift(1, 0); // we want to place the cursor after the whitespace starting the word
    }
}

/// `NOT_WORD_RE.test(chars)` com `NOT_WORD_RE = /[^a-z0-9]/i`.
fn not_word_re_test(chars: &JsString) -> bool {
    thread_local! {
        static REGEX: Rc<JsRegExp> = Rc::new(JsRegExp::from_str(r"[^a-z0-9]", "i").expect("padrão do type ahead válido"));
    }
    let regex = REGEX.with(Rc::clone);
    regex.set_last_index(0);
    regex.exec(chars.units()).is_some()
}

// ---------------------------------------------------------------------------------------------------------------------
// Predições
// ---------------------------------------------------------------------------------------------------------------------

/// `const enum MatchResult`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MatchResult {
    /// matched successfully
    Success,
    /// failed to match
    Failure,
    /// buffer data, it might match in the future one more data comes in
    Buffer,
}

/// `IPrediction`.
pub trait IPrediction {
    /// Whether applying this prediction can modify the style attributes of the terminal. If so it means we need to reset
    /// the cursor style if it's rolled back.
    fn affects_style(&self) -> bool {
        false
    }

    /// If set to false, the prediction will not be cleared if no input is received from the server.
    fn clear_after_timeout(&self) -> bool {
        true
    }

    /// Returns a sequence to apply the prediction. `cursor`: position to write the data; should advance the cursor.
    fn apply(&self, buffer: &Rc<dyn ITypeAheadBuffer>, cursor: &Cursor) -> String;

    /// Returns a sequence to roll back a previous `apply()` call. If `rollForwards` is not given, then this is also called
    /// if a prediction is correct before show the user's data.
    fn rollback(&self, cursor: &Cursor) -> String;

    /// If available, this will be called when the prediction is correct.
    fn roll_forwards(&self, cursor: &Cursor, with_input: &str) -> String;

    /// Returns whether the given input is one expected by this prediction. `look_behind`: the last successfully-made
    /// prediction, if any.
    fn matches(&self, input: &mut StringReader, look_behind: Option<&Rc<dyn IPrediction>>) -> MatchResult;

    /// Para o `instanceof` do original.
    fn as_any(&self) -> &dyn Any;
}

/// `StringReader`.
pub struct StringReader {
    pub index: usize,
    input: Vec<u16>,
}

impl StringReader {
    pub fn new(input: &[u16]) -> Self {
        Self { index: 0, input: input.to_vec() }
    }

    pub fn remaining(&self) -> usize {
        self.input.len() - self.index
    }

    pub fn eof(&self) -> bool {
        self.index == self.input.len()
    }

    pub fn rest(&self) -> Vec<u16> {
        self.input[self.index.min(self.input.len())..].to_vec()
    }

    /// Advances the reader and returns whether the character matched.
    pub fn eat_char(&mut self, char_: u16) -> bool {
        if self.input.get(self.index) != Some(&char_) {
            return false;
        }

        self.index += 1;
        true
    }

    /// Advances the reader and returns whether the (non-empty) string matched. O `slice(index, substr.length)` do original só
    /// compara certo com `index == 0`.
    pub fn eat_str(&mut self, substr: &[u16]) -> bool {
        let end = substr.len().min(self.input.len());
        let start = self.index.min(self.input.len());
        let slice: &[u16] = if start < end { &self.input[start..end] } else { &[] };
        if slice != substr {
            return false;
        }

        self.index += substr.len();
        !substr.is_empty()
    }

    /// Matches and eats the substring character-by-character. If EOF is reached before the substring is consumed, it will
    /// buffer. Index is not moved if it's not a match.
    pub fn eat_gradually(&mut self, substr: &[u16]) -> MatchResult {
        let prev_index = self.index;
        for (i, unit) in substr.iter().enumerate() {
            if i > 0 && self.eof() {
                return MatchResult::Buffer;
            }

            if !self.eat_char(*unit) {
                self.index = prev_index;
                return MatchResult::Failure;
            }
        }

        MatchResult::Success
    }

    /// Advances the reader and returns the captures of the regex if it matches (`match[n]`).
    pub fn eat_re(&mut self, re: &JsRegExp) -> Option<Vec<Option<Vec<u16>>>> {
        let rest = self.rest();
        re.set_last_index(0);
        let found = re.exec(&rest)?;
        let captures: Vec<Option<Vec<u16>>> = found.captures.iter().map(|range| range.as_ref().map(|range| rest[range.clone()].to_vec())).collect();
        self.index += captures[0].as_ref().map_or(0, Vec::len);
        Some(captures)
    }

    /// Advances the reader and returns the code if it is in `[min, max)`.
    pub fn eat_char_code(&mut self, min: u16, max: u16) -> Option<u16> {
        let code = *self.input.get(self.index)?;
        if code < min || code >= max {
            return None;
        }

        self.index += 1;
        Some(code)
    }
}

/// Preidction which never tests true. Will always discard predictions made after it.
struct HardBoundary;

impl IPrediction for HardBoundary {
    fn clear_after_timeout(&self) -> bool {
        false
    }

    fn apply(&self, _buffer: &Rc<dyn ITypeAheadBuffer>, _cursor: &Cursor) -> String {
        String::new()
    }

    fn rollback(&self, _cursor: &Cursor) -> String {
        String::new()
    }

    fn roll_forwards(&self, _cursor: &Cursor, _with_input: &str) -> String {
        String::new()
    }

    fn matches(&self, _input: &mut StringReader, _look_behind: Option<&Rc<dyn IPrediction>>) -> MatchResult {
        MatchResult::Failure
    }

    fn as_any(&self) -> &dyn Any {
        self
    }
}

/// Wraps another prediction. Does not apply the prediction, but will pass through its `matches` request.
pub struct TentativeBoundary {
    applied_cursor: RefCell<Option<Cursor>>,
    pub inner: Rc<dyn IPrediction>,
}

impl TentativeBoundary {
    pub fn new(inner: Rc<dyn IPrediction>) -> Self {
        Self { applied_cursor: RefCell::new(None), inner }
    }
}

impl IPrediction for TentativeBoundary {
    fn apply(&self, buffer: &Rc<dyn ITypeAheadBuffer>, cursor: &Cursor) -> String {
        let applied = cursor.clone_cursor();
        self.inner.apply(buffer, &applied);
        *self.applied_cursor.borrow_mut() = Some(applied);
        String::new()
    }

    fn rollback(&self, cursor: &Cursor) -> String {
        self.inner.rollback(&cursor.clone_cursor());
        String::new()
    }

    fn roll_forwards(&self, cursor: &Cursor, with_input: &str) -> String {
        if let Some(applied) = self.applied_cursor.borrow().as_ref() {
            cursor.move_to(applied.coordinate());
        }

        with_input.to_owned()
    }

    fn matches(&self, input: &mut StringReader, _look_behind: Option<&Rc<dyn IPrediction>>) -> MatchResult {
        self.inner.matches(input, None)
    }

    fn as_any(&self) -> &dyn Any {
        self
    }
}

/// `isTenativeCharacterPrediction(p)`: a posição onde a predição de caractere dentro de um `TentativeBoundary` foi aplicada.
fn tentative_character_applied_pos(prediction: &Rc<dyn IPrediction>) -> Option<ICoordinate> {
    let tentative = prediction.as_any().downcast_ref::<TentativeBoundary>()?;
    let character = tentative.inner.as_any().downcast_ref::<CharacterPrediction>()?;
    let applied_at = character.applied_at.borrow().as_ref().map(|applied_at| applied_at.pos);
    applied_at
}

/// O que `CharacterPrediction.appliedAt` guarda.
struct CharacterApplied {
    pos: ICoordinate,
    old_attributes: String,
    old_char: String,
}

/// Prediction for a single alphanumeric character.
pub struct CharacterPrediction {
    applied_at: RefCell<Option<CharacterApplied>>,
    style: Rc<TypeAheadStyle>,
    char_: u16,
}

impl CharacterPrediction {
    pub fn new(style: Rc<TypeAheadStyle>, char_: u16) -> Self {
        Self { applied_at: RefCell::new(None), style, char_ }
    }

    fn char_text(&self) -> String {
        text_of(&[self.char_])
    }
}

impl IPrediction for CharacterPrediction {
    fn affects_style(&self) -> bool {
        true
    }

    fn apply(&self, _buffer: &Rc<dyn ITypeAheadBuffer>, cursor: &Cursor) -> String {
        let cell = cursor.get_cell();
        *self.applied_at.borrow_mut() = Some(match cell {
            Some(cell) => CharacterApplied { pos: cursor.coordinate(), old_attributes: attributes_to_seq(cell.as_ref()), old_char: cell.get_chars() },
            None => CharacterApplied { pos: cursor.coordinate(), old_attributes: String::new(), old_char: String::new() },
        });

        cursor.shift(1, 0);

        format!("{}{}{}", self.style.apply(), self.char_text(), self.style.undo())
    }

    fn rollback(&self, cursor: &Cursor) -> String {
        let applied_at = self.applied_at.borrow();
        let Some(applied_at) = applied_at.as_ref() else {
            return String::new(); // not applied
        };

        let CharacterApplied { old_attributes, old_char, pos } = applied_at;
        let mut r = cursor.move_to(*pos);
        if old_char.is_empty() {
            r.push_str(VT_DELETE_CHAR);
        } else {
            r.push_str(old_attributes);
            r.push_str(old_char);
            r.push_str(&cursor.move_to(*pos));
        }
        r
    }

    fn roll_forwards(&self, cursor: &Cursor, input: &str) -> String {
        let applied_at = self.applied_at.borrow();
        let Some(applied_at) = applied_at.as_ref() else {
            return String::new(); // not applied
        };

        format!("{}{input}", cursor.clone_cursor().move_to(applied_at.pos))
    }

    fn matches(&self, input: &mut StringReader, look_behind: Option<&Rc<dyn IPrediction>>) -> MatchResult {
        let start_index = input.index;

        // remove any styling CSI before checking the char
        while input.eat_re(&csi_style_re()).is_some() {}

        if input.eof() {
            return MatchResult::Buffer;
        }

        if input.eat_char(self.char_) {
            return MatchResult::Success;
        }

        if let Some(look_behind) = look_behind.and_then(|look_behind| look_behind.as_any().downcast_ref::<CharacterPrediction>()) {
            // see #112842
            let silly_zsh_outcome = input.eat_gradually(&units(&format!("\u{8}{}{}", look_behind.char_text(), self.char_text())));
            if silly_zsh_outcome != MatchResult::Failure {
                return silly_zsh_outcome;
            }
        }

        input.index = start_index;
        MatchResult::Failure
    }

    fn as_any(&self) -> &dyn Any {
        self
    }
}

/// O que `BackspacePrediction._appliedAt` guarda.
struct BackspaceApplied {
    pos: ICoordinate,
    old_attributes: String,
    old_char: String,
    is_last_char: bool,
}

pub struct BackspacePrediction {
    applied_at: RefCell<Option<BackspaceApplied>>,
    terminal: Rc<dyn ITypeAheadTerminal>,
}

impl BackspacePrediction {
    pub fn new(terminal: Rc<dyn ITypeAheadTerminal>) -> Self {
        Self { applied_at: RefCell::new(None), terminal }
    }
}

impl IPrediction for BackspacePrediction {
    fn apply(&self, _buffer: &Rc<dyn ITypeAheadBuffer>, cursor: &Cursor) -> String {
        // at eol if everything to the right is whitespace (zsh will emit a "clear line" code in this case)
        // todo: can be optimized if `getTrimmedLength` is exposed from xterm
        let is_last_char = cursor.get_line().is_none_or(|line| js_trim(&line.translate_to_string(None, Some(cursor.x()), None)).is_empty());
        let pos = cursor.coordinate();
        let move_ = cursor.shift(-1, 0);
        let cell = cursor.get_cell();
        *self.applied_at.borrow_mut() = Some(match cell {
            Some(cell) => BackspaceApplied { is_last_char, pos, old_attributes: attributes_to_seq(cell.as_ref()), old_char: cell.get_chars() },
            None => BackspaceApplied { is_last_char, pos, old_attributes: String::new(), old_char: String::new() },
        });

        format!("{move_}{VT_DELETE_CHAR}")
    }

    fn rollback(&self, cursor: &Cursor) -> String {
        let applied_at = self.applied_at.borrow();
        let Some(applied_at) = applied_at.as_ref() else {
            return String::new(); // not applied
        };

        let BackspaceApplied { old_attributes, old_char, pos, .. } = applied_at;
        if old_char.is_empty() {
            return format!("{}{VT_DELETE_CHAR}", cursor.move_to(*pos));
        }

        format!("{old_attributes}{old_char}{}{}", cursor.move_to(*pos), attributes_to_seq(self.terminal.cur_attr_data().as_ref()))
    }

    fn roll_forwards(&self, _cursor: &Cursor, _with_input: &str) -> String {
        String::new()
    }

    fn matches(&self, input: &mut StringReader, _look_behind: Option<&Rc<dyn IPrediction>>) -> MatchResult {
        if self.applied_at.borrow().as_ref().is_some_and(|applied_at| applied_at.is_last_char) {
            let r1 = input.eat_gradually(&units(&format!("\u{8}{VT_CSI}K")));
            if r1 != MatchResult::Failure {
                return r1;
            }

            let r2 = input.eat_gradually(&units("\u{8} \u{8}"));
            if r2 != MatchResult::Failure {
                return r2;
            }
        }

        MatchResult::Failure
    }

    fn as_any(&self) -> &dyn Any {
        self
    }
}

/// `NewlinePrediction` e, com `linewrap`, a `LinewrapPrediction`: prediction when the cursor reaches the end of the line.
/// Similar to newline prediction, but shells handle it slightly differently.
pub struct NewlinePrediction {
    prev_position: RefCell<Option<ICoordinate>>,
    linewrap: bool,
}

impl NewlinePrediction {
    pub fn new(linewrap: bool) -> Self {
        Self { prev_position: RefCell::new(None), linewrap }
    }
}

impl IPrediction for NewlinePrediction {
    fn apply(&self, _buffer: &Rc<dyn ITypeAheadBuffer>, cursor: &Cursor) -> String {
        *self.prev_position.borrow_mut() = Some(cursor.coordinate());
        cursor.move_(0, cursor.y() + 1);
        if self.linewrap { " \r" } else { "\r\n" }.to_owned()
    }

    fn rollback(&self, cursor: &Cursor) -> String {
        match *self.prev_position.borrow() {
            Some(prev_position) => cursor.move_to(prev_position),
            None => String::new(),
        }
    }

    fn roll_forwards(&self, _cursor: &Cursor, _with_input: &str) -> String {
        String::new() // does not need to rewrite
    }

    fn matches(&self, input: &mut StringReader, _look_behind: Option<&Rc<dyn IPrediction>>) -> MatchResult {
        if !self.linewrap {
            return input.eat_gradually(&units("\r\n"));
        }

        // bash and zshell add a space which wraps in the terminal, then a CR
        let r = input.eat_gradually(&units(" \r"));
        if r != MatchResult::Failure {
            // zshell additionally adds a clear line after wrapping to be safe -- eat it
            let r2 = input.eat_gradually(&units(VT_DELETE_REST_OF_LINE));
            return if r2 == MatchResult::Buffer { MatchResult::Buffer } else { r };
        }

        input.eat_gradually(&units("\r\n"))
    }

    fn as_any(&self) -> &dyn Any {
        self
    }
}

/// O que `CursorMovePrediction._applied` guarda.
struct CursorMoveApplied {
    roll_forward: String,
    prev_position: i32,
    prev_attrs: String,
    amount: i32,
}

struct CursorMovePrediction {
    applied: RefCell<Option<CursorMoveApplied>>,
    direction: CursorMoveDirection,
    move_by_words: bool,
    amount: i32,
}

impl CursorMovePrediction {
    fn new(direction: CursorMoveDirection, move_by_words: bool, amount: i32) -> Self {
        Self { applied: RefCell::new(None), direction, move_by_words, amount }
    }
}

impl IPrediction for CursorMovePrediction {
    fn apply(&self, _buffer: &Rc<dyn ITypeAheadBuffer>, cursor: &Cursor) -> String {
        let prev_position = cursor.x();
        let current_cell = cursor.get_cell();
        let prev_attrs = current_cell.map_or_else(String::new, |cell| attributes_to_seq(cell.as_ref()));

        let delta = if self.direction == CursorMoveDirection::Back { -1 } else { 1 };

        let target = cursor.clone_cursor();
        if self.move_by_words {
            for _ in 0..self.amount {
                move_to_word_boundary(&target, delta);
            }
        } else {
            target.shift(delta * self.amount, 0);
        }

        let roll_forward = cursor.move_to(target.coordinate());
        *self.applied.borrow_mut() = Some(CursorMoveApplied { amount: (cursor.x() - target.x()).abs(), prev_position, prev_attrs, roll_forward: roll_forward.clone() });

        roll_forward
    }

    fn rollback(&self, cursor: &Cursor) -> String {
        let applied = self.applied.borrow();
        let Some(applied) = applied.as_ref() else {
            return String::new();
        };

        format!("{}{}", cursor.move_(applied.prev_position, cursor.y()), applied.prev_attrs)
    }

    fn roll_forwards(&self, _cursor: &Cursor, _with_input: &str) -> String {
        String::new() // does not need to rewrite
    }

    fn matches(&self, input: &mut StringReader, _look_behind: Option<&Rc<dyn IPrediction>>) -> MatchResult {
        let applied = self.applied.borrow();
        let Some(CursorMoveApplied { amount, roll_forward, .. }) = applied.as_ref() else {
            return MatchResult::Failure;
        };

        let direction = self.direction;
        let amount_times = usize::try_from(*amount).unwrap_or(0);

        // arg can be omitted to move one character. We don't eatGradually() here
        // or below moves that don't go as far as the cursor would be buffered
        // indefinitely
        if input.eat_str(&units(&format!("{VT_CSI}{}", direction.letter()).repeat(amount_times))) {
            return MatchResult::Success;
        }

        // \b is the equivalent to moving one character back
        if direction == CursorMoveDirection::Back && input.eat_str(&units(&"\u{8}".repeat(amount_times))) {
            return MatchResult::Success;
        }

        // check if the cursor position is set absolutely
        if !roll_forward.is_empty() {
            let r = input.eat_gradually(&units(roll_forward));
            if r != MatchResult::Failure {
                return r;
            }
        }

        // check for a relative move in the direction
        input.eat_gradually(&units(&format!("{VT_CSI}{amount}{}", direction.letter())))
    }

    fn as_any(&self) -> &dyn Any {
        self
    }
}

/// `text.trim()` do JavaScript.
fn js_trim(text: &str) -> String {
    text_of(je_base::js_string::trim(&units(text)))
}

// ---------------------------------------------------------------------------------------------------------------------
// PredictionStats
// ---------------------------------------------------------------------------------------------------------------------

/// O que `PredictionStats.latency` devolve.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct LatencyStats {
    pub count: usize,
    pub min: Option<f64>,
    pub median: Option<f64>,
    pub max: Option<f64>,
}

/// `PredictionStats`. Em vez do `PredictionTimeline` inteiro, o construtor recebe os três eventos que o original lê dele.
pub struct PredictionStats {
    base: Rc<Disposable>,
    /// `_stats`: `[latency, correct]`.
    stats: RefCell<Vec<(f64, bool)>>,
    index: Cell<usize>,
    /// `_addedAtTime` (um `WeakMap` pela identidade da predição).
    added_at_time: RefCell<HashMap<usize, u64>>,
    change_emitter: Rc<Emitter<()>>,
}

impl PredictionStats {
    pub fn new(on_prediction_added: Event<Rc<dyn IPrediction>>, on_prediction_succeeded: Event<Rc<dyn IPrediction>>, on_prediction_failed: Event<Rc<dyn IPrediction>>) -> Rc<Self> {
        let base = Disposable::new();
        let change_emitter = Emitter::new();
        base.register(change_emitter.clone());
        let this = Rc::new(Self { base, stats: RefCell::new(Vec::new()), index: Cell::new(0), added_at_time: RefCell::new(HashMap::new()), change_emitter });
        let weak = Rc::downgrade(&this);
        this.base.register(on_prediction_added.listen({
            let weak = weak.clone();
            move |p| {
                if let Some(this) = weak.upgrade() {
                    this.added_at_time.borrow_mut().insert(id_of(p), runtime::date_now());
                }
            }
        }));
        this.base.register(on_prediction_succeeded.listen({
            let weak = weak.clone();
            move |p| {
                if let Some(this) = weak.upgrade() {
                    this.push_stat(true, p);
                }
            }
        }));
        this.base.register(on_prediction_failed.listen(move |p| {
            if let Some(this) = weak.upgrade() {
                this.push_stat(false, p);
            }
        }));
        this
    }

    pub fn on_change(&self) -> Event<()> {
        self.change_emitter.event()
    }

    /// Gets the percent (0-1) of predictions that were accurate.
    pub fn accuracy(&self) -> f64 {
        let stats = self.stats.borrow();
        let correct_count = stats.iter().filter(|(_, correct)| *correct).count();
        correct_count as f64 / (if stats.is_empty() { 1 } else { stats.len() }) as f64
    }

    /// Gets the number of recorded stats.
    pub fn sample_size(&self) -> usize {
        self.stats.borrow().len()
    }

    /// Gets latency stats of successful predictions.
    pub fn latency(&self) -> LatencyStats {
        let mut latencies: Vec<f64> = self.stats.borrow().iter().filter(|(_, correct)| *correct).map(|(latency, _)| *latency).collect();
        latencies.sort_by(f64::total_cmp);

        LatencyStats { count: latencies.len(), min: latencies.first().copied(), median: latencies.get(latencies.len() / 2).copied(), max: latencies.last().copied() }
    }

    /// Gets the maximum observed latency.
    pub fn max_latency(&self) -> f64 {
        let mut max = f64::NEG_INFINITY;
        for (latency, correct) in self.stats.borrow().iter() {
            if *correct {
                max = latency.max(max);
            }
        }

        max
    }

    fn push_stat(&self, correct: bool, prediction: &Rc<dyn IPrediction>) {
        let started = self.added_at_time.borrow().get(&id_of(prediction)).copied().unwrap_or(0);
        let entry = (runtime::date_now().saturating_sub(started) as f64, correct);
        {
            let mut stats = self.stats.borrow_mut();
            if self.index.get() < stats.len() {
                stats[self.index.get()] = entry;
            } else {
                stats.push(entry);
            }
        }
        self.index.set((self.index.get() + 1) % STATS_BUFFER_SIZE);
        self.change_emitter.fire(());
    }
}

impl IDisposable for PredictionStats {
    fn dispose(&self) {
        self.base.dispose();
    }
}

// ---------------------------------------------------------------------------------------------------------------------
// PredictionTimeline
// ---------------------------------------------------------------------------------------------------------------------

struct Expected {
    generation: u32,
    p: Rc<dyn IPrediction>,
}

/// `PredictionTimeline`.
pub struct PredictionTimeline {
    base: Rc<Disposable>,
    pub terminal: Rc<dyn ITypeAheadTerminal>,
    style: Rc<TypeAheadStyle>,

    /// Expected queue of events. Only predictions for the lowest are written into the terminal.
    expected: RefCell<Vec<Expected>>,

    /// Current prediction generation.
    current_gen: Cell<u32>,

    /// Current cursor position -- kept outside the buffer since it can be ahead if typing swiftly. The position of the cursor
    /// that the user is currently looking at on their screen (or will be looking at after all pending writes are flushed.)
    physical_cursor: RefCell<Option<Rc<Cursor>>>,

    /// Cursor position taking into account all (possibly not-yet-applied) predictions. A new prediction inserted, if applied,
    /// will be applied at the position of the tentative cursor.
    tentative_cursor: RefCell<Option<Rc<Cursor>>>,

    /// Previously sent data that was buffered and should be prepended to the next input.
    input_buffer: RefCell<Option<Vec<u16>>>,

    /// Whether predictions are echoed to the terminal. If false, predictions will still be computed internally for latency
    /// metrics, but input will never be adjusted.
    show_predictions: Cell<bool>,

    /// The last successfully-made prediction.
    look_behind: RefCell<Option<Rc<dyn IPrediction>>>,

    added_emitter: Rc<Emitter<Rc<dyn IPrediction>>>,
    failed_emitter: Rc<Emitter<Rc<dyn IPrediction>>>,
    succeeded_emitter: Rc<Emitter<Rc<dyn IPrediction>>>,
}

impl PredictionTimeline {
    pub fn new(terminal: Rc<dyn ITypeAheadTerminal>, style: Rc<TypeAheadStyle>) -> Rc<Self> {
        let base = Disposable::new();
        let added_emitter = Emitter::new();
        base.register(added_emitter.clone());
        let failed_emitter = Emitter::new();
        base.register(failed_emitter.clone());
        let succeeded_emitter = Emitter::new();
        base.register(succeeded_emitter.clone());
        Rc::new(Self {
            base,
            terminal,
            style,
            expected: RefCell::new(Vec::new()),
            current_gen: Cell::new(0),
            physical_cursor: RefCell::new(None),
            tentative_cursor: RefCell::new(None),
            input_buffer: RefCell::new(None),
            show_predictions: Cell::new(false),
            look_behind: RefCell::new(None),
            added_emitter,
            failed_emitter,
            succeeded_emitter,
        })
    }

    pub fn on_prediction_added(&self) -> Event<Rc<dyn IPrediction>> {
        self.added_emitter.event()
    }

    pub fn on_prediction_failed(&self) -> Event<Rc<dyn IPrediction>> {
        self.failed_emitter.event()
    }

    pub fn on_prediction_succeeded(&self) -> Event<Rc<dyn IPrediction>> {
        self.succeeded_emitter.event()
    }

    fn current_generation_predictions(&self) -> Vec<Rc<dyn IPrediction>> {
        let expected = self.expected.borrow();
        expected.iter().filter(|e| e.generation == expected[0].generation).map(|e| e.p.clone()).collect()
    }

    pub fn is_showing_predictions(&self) -> bool {
        self.show_predictions.get()
    }

    pub fn length(&self) -> usize {
        self.expected.borrow().len()
    }

    pub fn set_show_predictions(&self, show: bool) {
        if show == self.show_predictions.get() {
            return;
        }

        self.show_predictions.set(show);

        let Some(buffer) = self.get_active_buffer() else {
            return;
        };

        let mut to_apply = self.current_generation_predictions();
        if show {
            self.clear_cursor();
            self.style.expect_incoming_style(to_apply.iter().filter(|p| p.affects_style()).count() as i64);
            let text: String = to_apply.iter().map(|p| p.apply(&buffer, &self.physical_cursor(&buffer))).collect();
            self.terminal.write(&text);
        } else {
            to_apply.reverse();
            let text: String = to_apply.iter().map(|p| p.rollback(&self.physical_cursor(&buffer))).collect();
            self.terminal.write(&text);
        }
    }

    /// Undoes any predictions written and resets expectations.
    pub fn undo_all_predictions(&self) {
        let buffer = self.get_active_buffer();
        if let (true, Some(buffer)) = (self.show_predictions.get(), buffer) {
            let mut predictions = self.current_generation_predictions();
            predictions.reverse();
            let text: String = predictions.iter().map(|p| p.rollback(&self.physical_cursor(&buffer))).collect();
            self.terminal.write(&text);
        }

        self.expected.borrow_mut().clear();
    }

    /// Should be called when input is incoming to the temrinal.
    pub fn before_server_input(&self, input: &str) -> String {
        let original_input = input;
        let mut input = units(input);
        if let Some(buffered) = self.input_buffer.borrow_mut().take().filter(|buffered| !buffered.is_empty()) {
            let mut joined = buffered;
            joined.extend_from_slice(&input);
            input = joined;
        }

        if self.expected.borrow().is_empty() {
            self.clear_prediction_state();
            return text_of(&input);
        }

        let Some(buffer) = self.get_active_buffer() else {
            self.clear_prediction_state();
            return text_of(&input);
        };

        let mut output = String::new();

        let mut reader = StringReader::new(&input);
        let starting_gen = self.expected.borrow()[0].generation;
        let emit_prediction_omitted = |reader: &mut StringReader, output: &mut String| {
            if let Some(omit) = reader.eat_re(&prediction_omit_re()) {
                output.push_str(&text_of(omit[0].as_deref().unwrap_or_default()));
            }
        };

        'read_loop: while !self.expected.borrow().is_empty() && reader.remaining() > 0 {
            emit_prediction_omitted(&mut reader, &mut output);

            let (prediction, generation) = {
                let expected = self.expected.borrow();
                (expected[0].p.clone(), expected[0].generation)
            };
            let cursor = self.physical_cursor(&buffer);
            let before_test_reader_index = reader.index;
            let look_behind = self.look_behind.borrow().clone();
            match prediction.matches(&mut reader, look_behind.as_ref()) {
                MatchResult::Success => {
                    // if the input character matches what the next prediction expected, undo
                    // the prediction and write the real character out.
                    let eaten = text_of(&input[before_test_reader_index..reader.index]);
                    if generation == starting_gen {
                        output.push_str(&prediction.roll_forwards(&cursor, &eaten));
                    } else {
                        prediction.apply(&buffer, &self.physical_cursor(&buffer)); // move cursor for additional apply
                        output.push_str(&eaten);
                    }

                    self.succeeded_emitter.fire(prediction.clone());
                    *self.look_behind.borrow_mut() = Some(prediction);
                    self.expected.borrow_mut().remove(0);
                }
                MatchResult::Buffer => {
                    // on a buffer, store the remaining data and completely read data
                    // to be output as normal.
                    *self.input_buffer.borrow_mut() = Some(input[before_test_reader_index..].to_vec());
                    reader.index = input.len();
                    break 'read_loop;
                }
                MatchResult::Failure => {
                    // on a failure, roll back all remaining items in this generation
                    // and clear predictions, since they are no longer valid
                    let rollback: Vec<Rc<dyn IPrediction>> = self.expected.borrow().iter().filter(|e| e.generation == starting_gen).map(|e| e.p.clone()).rev().collect();
                    for p in &rollback {
                        output.push_str(&p.rollback(&self.physical_cursor(&buffer)));
                    }
                    if rollback.iter().any(|p| p.affects_style()) {
                        // reading the current style should generally be safe, since predictions
                        // always restore the style if they modify it.
                        output.push_str(&attributes_to_seq(self.terminal.cur_attr_data().as_ref()));
                    }
                    self.clear_prediction_state();
                    self.failed_emitter.fire(prediction);
                    break 'read_loop;
                }
            }
        }

        emit_prediction_omitted(&mut reader, &mut output);

        // Extra data (like the result of running a command) should cause us to
        // reset the cursor
        if !reader.eof() {
            output.push_str(&text_of(&reader.rest()));
            self.clear_prediction_state();
        }

        // If we passed a generation boundary, apply the current generation's predictions
        let snapshot: Vec<(u32, Rc<dyn IPrediction>)> = self.expected.borrow().iter().map(|e| (e.generation, e.p.clone())).collect();
        if !snapshot.is_empty() && starting_gen != snapshot[0].0 {
            for (generation, p) in &snapshot {
                if *generation != snapshot[0].0 {
                    break;
                }
                if p.affects_style() {
                    self.style.expect_incoming_style(1);
                }

                output.push_str(&p.apply(&buffer, &self.physical_cursor(&buffer)));
            }
        }

        if !self.show_predictions.get() {
            return original_input.to_owned();
        }

        if output.is_empty() || output == text_of(&input) {
            return output;
        }

        if let Some(physical_cursor) = self.physical_cursor.borrow().as_ref() {
            output.push_str(&physical_cursor.move_instruction());
        }

        // prevent cursor flickering while typing
        output = format!("{VT_HIDE_CURSOR}{output}{VT_SHOW_CURSOR}");

        output
    }

    /// Clears any expected predictions and stored state. Should be called when the pty gives us something we don't recognize.
    fn clear_prediction_state(&self) {
        self.expected.borrow_mut().clear();
        self.clear_cursor();
        *self.look_behind.borrow_mut() = None;
    }

    /// Appends a typeahead prediction.
    pub fn add_prediction(&self, buffer: &Rc<dyn ITypeAheadBuffer>, prediction: Rc<dyn IPrediction>) -> bool {
        self.expected.borrow_mut().push(Expected { generation: self.current_gen.get(), p: prediction.clone() });
        self.added_emitter.fire(prediction.clone());

        if self.current_gen.get() != self.expected.borrow()[0].generation {
            prediction.apply(buffer, &self.tentative_cursor(buffer));
            return false;
        }

        let text = prediction.apply(buffer, &self.physical_cursor(buffer));
        *self.tentative_cursor.borrow_mut() = None; // next read will get or clone the physical cursor

        if self.show_predictions.get() && !text.is_empty() {
            if prediction.affects_style() {
                self.style.expect_incoming_style(1);
            }
            self.terminal.write(&text);
        }

        true
    }

    /// Appends a prediction followed by a boundary (`addBoundary(buffer, prediction)`), or just the boundary
    /// (`addBoundary()`, com `None`). The predictions applied after this one will only be displayed after the give prediction
    /// matches pty output.
    pub fn add_boundary(&self, with: Option<(&Rc<dyn ITypeAheadBuffer>, Rc<dyn IPrediction>)>) -> bool {
        let mut applied = false;
        if let Some((buffer, prediction)) = with {
            // We apply the prediction so that it's matched against, but wrapped
            // in a tentativeboundary so that it doesn't affect the physical cursor.
            // Then we apply it specifically to the tentative cursor.
            applied = self.add_prediction(buffer, Rc::new(TentativeBoundary::new(prediction.clone())));
            prediction.apply(buffer, &self.tentative_cursor(buffer));
        }
        self.current_gen.set(self.current_gen.get() + 1);
        applied
    }

    /// Peeks the last prediction written.
    pub fn peek_end(&self) -> Option<Rc<dyn IPrediction>> {
        self.expected.borrow().last().map(|e| e.p.clone())
    }

    /// Peeks the first pending prediction.
    pub fn peek_start(&self) -> Option<Rc<dyn IPrediction>> {
        self.expected.borrow().first().map(|e| e.p.clone())
    }

    /// Current position of the cursor in the terminal.
    pub fn physical_cursor(&self, buffer: &Rc<dyn ITypeAheadBuffer>) -> Rc<Cursor> {
        let mut physical_cursor = self.physical_cursor.borrow_mut();
        physical_cursor.get_or_insert_with(|| Rc::new(Cursor::new(self.terminal.rows(), self.terminal.cols(), buffer.clone()))).clone()
    }

    /// Cursor position if all predictions and boundaries that have been inserted so far turn out to be successfully predicted.
    pub fn tentative_cursor(&self, buffer: &Rc<dyn ITypeAheadBuffer>) -> Rc<Cursor> {
        let existing = self.tentative_cursor.borrow().clone();
        if let Some(existing) = existing {
            return existing;
        }
        let cloned = Rc::new(self.physical_cursor(buffer).clone_cursor());
        *self.tentative_cursor.borrow_mut() = Some(cloned.clone());
        cloned
    }

    pub fn clear_cursor(&self) {
        *self.physical_cursor.borrow_mut() = None;
        *self.tentative_cursor.borrow_mut() = None;
    }

    fn get_active_buffer(&self) -> Option<Rc<dyn ITypeAheadBuffer>> {
        let buffer = self.terminal.buffer_active();
        buffer.is_normal().then_some(buffer)
    }
}

impl IDisposable for PredictionTimeline {
    fn dispose(&self) {
        self.base.dispose();
    }
}

// ---------------------------------------------------------------------------------------------------------------------
// Atributos e estilo
// ---------------------------------------------------------------------------------------------------------------------

/// Gets the escape sequence args to restore state/appearance in the cell.
fn attributes_to_args(cell: &dyn IXtermAttributes) -> Vec<i64> {
    if cell.is_attribute_default() {
        return vec![0];
    }

    let mut args = Vec::new();
    if cell.is_bold() {
        args.push(1);
    }
    if cell.is_dim() {
        args.push(2);
    }
    if cell.is_italic() {
        args.push(3);
    }
    if cell.is_underline() {
        args.push(4);
    }
    if cell.is_blink() {
        args.push(5);
    }
    if cell.is_inverse() {
        args.push(7);
    }
    if cell.is_invisible() {
        args.push(8);
    }

    if cell.is_fg_rgb() {
        let color = cell.get_fg_color();
        args.extend([38, 2, i64::from(color >> 24), i64::from((color >> 16) & 0xFF), i64::from(color & 0xFF)]);
    }
    if cell.is_fg_palette() {
        args.extend([38, 5, i64::from(cell.get_fg_color())]);
    }
    if cell.is_fg_default() {
        args.push(39);
    }

    if cell.is_bg_rgb() {
        let color = cell.get_bg_color();
        args.extend([48, 2, i64::from(color >> 24), i64::from((color >> 16) & 0xFF), i64::from(color & 0xFF)]);
    }
    if cell.is_bg_palette() {
        args.extend([48, 5, i64::from(cell.get_bg_color())]);
    }
    if cell.is_bg_default() {
        args.push(49);
    }

    args
}

/// Gets the escape sequence to restore state/appearance in the cell.
fn attributes_to_seq(cell: &dyn IXtermAttributes) -> String {
    format!("{VT_CSI}{}m", attributes_to_args(cell).iter().map(i64::to_string).collect::<Vec<_>>().join(";"))
}

/// `arrayHasPrefixAt(a, ai, b)`. O original indexa `b` com `ai` (e não com `bi`) e compara com `!==`: um parâmetro aninhado nunca é
/// igual a um número, e dois `undefined` são iguais.
fn array_has_prefix_at(a: &[SgrParam], mut ai: usize, b: &[SgrParam]) -> bool {
    if a.len() as i64 - ai as i64 > b.len() as i64 {
        return false;
    }

    for _ in 0..b.len() {
        let equal = match (b.get(ai), a.get(ai)) {
            (None, None) => true,
            (Some(SgrParam::Single(x)), Some(SgrParam::Single(y))) => x == y,
            _ => false,
        };
        if !equal {
            return false;
        }
        ai += 1;
    }

    true
}

/// `getColorWidth(params, pos)`.
/// @see https://github.com/xtermjs/xterm.js/blob/065eb13a9d3145bea687239680ec9696d9112b8e/src/common/InputHandler.ts#L2127
fn get_color_width(params: &[SgrParam], pos: usize) -> usize {
    let mut accu: Vec<i64> = vec![0, 0, -1, 0, 0, 0];
    let set = |accu: &mut Vec<i64>, index: usize, value: i64| {
        if index >= accu.len() {
            accu.resize(index + 1, 0);
        }
        accu[index] = value;
    };
    let mut c_space = 0usize;
    let mut advance = 0usize;

    loop {
        let v = &params[pos + advance];
        set(&mut accu, advance + c_space, v.first());
        if let SgrParam::Many(sub_params) = v {
            let mut i = 0usize;
            loop {
                if accu[1] == 5 {
                    c_space = 1;
                }
                set(&mut accu, advance + i + 1 + c_space, sub_params.get(i).copied().unwrap_or(0));
                i += 1;
                if !(i < sub_params.len() && i + advance + 1 + c_space < accu.len()) {
                    break;
                }
            }
            break;
        }
        // exit early if can decide color mode with semicolons
        if (accu[1] == 5 && advance + c_space >= 2) || (accu[1] == 2 && advance + c_space >= 5) {
            break;
        }
        // offset colorSpace slot for semicolon mode
        if accu[1] != 0 {
            c_space = 1;
        }
        advance += 1;
        if !(advance + pos < params.len() && advance + c_space < accu.len()) {
            break;
        }
    }

    advance
}

/// `TypeAheadStyle`.
pub struct TypeAheadStyle {
    terminal: Rc<dyn ITypeAheadTerminal>,

    /// Number of typeahead style arguments we expect to read. If this is 0 and we see a style coming in, we know that the PTY
    /// actually wanted to update.
    expected_incoming_styles: Cell<i64>,
    apply_args: RefCell<Vec<i64>>,
    original_undo_args: RefCell<Vec<i64>>,
    undo_args: RefCell<Vec<SgrParam>>,

    apply: RefCell<String>,
    undo: RefCell<String>,
    csi_handler: RefCell<Option<DisposableRef>>,
    stop_tracking_debounce: RefCell<Option<Debounce<(), ()>>>,
}

impl TypeAheadStyle {
    fn compile_args(args: &[SgrParam]) -> String {
        format!("{VT_CSI}{}m", args.iter().map(SgrParam::to_js_string).collect::<Vec<_>>().join(";"))
    }

    fn singles(args: &[i64]) -> Vec<SgrParam> {
        args.iter().map(|arg| SgrParam::Single(*arg)).collect()
    }

    pub fn new(value: &str, terminal: Rc<dyn ITypeAheadTerminal>) -> Rc<Self> {
        let this = Rc::new(Self {
            terminal,
            expected_incoming_styles: Cell::new(0),
            apply_args: RefCell::new(Vec::new()),
            original_undo_args: RefCell::new(Vec::new()),
            undo_args: RefCell::new(Vec::new()),
            apply: RefCell::new(String::new()),
            undo: RefCell::new(String::new()),
            csi_handler: RefCell::new(None),
            stop_tracking_debounce: RefCell::new(None),
        });
        // `@debounce(2000) debounceStopTracking()`
        let weak = Rc::downgrade(&this);
        *this.stop_tracking_debounce.borrow_mut() = Some(Debounce::without_reducer(2000, move |()| {
            if let Some(this) = weak.upgrade() {
                this.stop_tracking();
            }
        }));
        this.on_update(value);
        this
    }

    /// `apply`: a sequência que liga o estilo.
    pub fn apply(&self) -> String {
        self.apply.borrow().clone()
    }

    /// `undo`: a sequência que desliga o estilo.
    pub fn undo(&self) -> String {
        self.undo.borrow().clone()
    }

    /// Signals that a style was written to the terminal and we should watch for it coming in.
    pub fn expect_incoming_style(&self, n: i64) {
        self.expected_incoming_styles.set(self.expected_incoming_styles.get() + n * 2);
    }

    /// Starts tracking for CSI changes in the terminal.
    pub fn start_tracking(self: &Rc<Self>) {
        self.expected_incoming_styles.set(0);
        self.on_did_write_sgr(&Self::singles(&attributes_to_args(self.terminal.cur_attr_data().as_ref())));
        let weak = Rc::downgrade(self);
        *self.csi_handler.borrow_mut() = Some(self.terminal.register_csi_handler_sgr(Rc::new(move |args| {
            if let Some(this) = weak.upgrade() {
                this.on_did_write_sgr(args);
            }
            false
        })));
    }

    /// Stops tracking terminal CSI changes, after 2 seconds without calls (`@debounce(2000)`).
    pub fn debounce_stop_tracking(&self) {
        if let Some(debounce) = self.stop_tracking_debounce.borrow().as_ref() {
            debounce.call(());
        }
    }

    fn stop_tracking(&self) {
        let handler = self.csi_handler.borrow_mut().take();
        if let Some(handler) = handler {
            handler.dispose();
        }
    }

    fn on_did_write_sgr(&self, args: &[SgrParam]) {
        let mut undo_reassigned = false;
        let apply_args = self.apply_args.borrow().clone();
        let mut i = 0usize;
        while i < args.len() {
            let p = args[i].first();

            if self.expected_incoming_styles.get() != 0 {
                let undo_args = self.undo_args.borrow().clone();
                if array_has_prefix_at(args, i, &undo_args) {
                    self.expected_incoming_styles.set(self.expected_incoming_styles.get() - 1);
                    i += undo_args.len();
                    continue;
                }
                let apply_params = Self::singles(&apply_args);
                if array_has_prefix_at(args, i, &apply_params) {
                    self.expected_incoming_styles.set(self.expected_incoming_styles.get() - 1);
                    i += apply_params.len();
                    continue;
                }
            }

            let width = if p == 38 || p == 48 || p == 58 { get_color_width(args, i) } else { 1 };
            let mut new_undo: Option<Vec<SgrParam>> = None;
            match apply_args.first().copied() {
                Some(1) => {
                    if p == 2 {
                        new_undo = Some(Self::singles(&[22, 2]));
                    } else if p == 22 || p == 0 {
                        new_undo = Some(Self::singles(&[22]));
                    }
                }
                Some(2) => {
                    if p == 1 {
                        new_undo = Some(Self::singles(&[22, 1]));
                    } else if p == 22 || p == 0 {
                        new_undo = Some(Self::singles(&[22]));
                    }
                }
                Some(38) => {
                    if p == 0 || p == 39 || p == 100 {
                        new_undo = Some(Self::singles(&[39]));
                    } else if (30..=38).contains(&p) || (90..=97).contains(&p) {
                        new_undo = Some(args[i..(i + width).min(args.len())].to_vec());
                    }
                }
                first => {
                    if first == Some(p) {
                        new_undo = Some(Self::singles(&apply_args));
                    } else if p == 0 {
                        new_undo = Some(Self::singles(&self.original_undo_args.borrow()));
                    }
                    // no-op
                }
            }
            if let Some(new_undo) = new_undo {
                *self.undo_args.borrow_mut() = new_undo;
                undo_reassigned = true;
            }

            i += width;
        }

        if undo_reassigned {
            *self.undo.borrow_mut() = Self::compile_args(&self.undo_args.borrow());
        }
    }

    /// Updates the current typeahead style.
    pub fn on_update(&self, style: &str) {
        let (apply_args, undo_args) = Self::get_args(style);
        *self.apply_args.borrow_mut() = apply_args.clone();
        *self.original_undo_args.borrow_mut() = undo_args.clone();
        *self.undo_args.borrow_mut() = Self::singles(&undo_args);
        *self.apply.borrow_mut() = Self::compile_args(&Self::singles(&apply_args));
        *self.undo.borrow_mut() = Self::compile_args(&Self::singles(&undo_args));
    }

    fn get_args(style: &str) -> (Vec<i64>, Vec<i64>) {
        match style {
            "bold" => (vec![1], vec![22]),
            "dim" => (vec![2], vec![22]),
            "italic" => (vec![3], vec![23]),
            "underlined" => (vec![4], vec![24]),
            "inverted" => (vec![7], vec![27]),
            _ => {
                let rgba = Color::from_hex(style).rgba();
                (vec![38, 2, i64::from(rgba.r), i64::from(rgba.g), i64::from(rgba.b)], vec![39])
            }
        }
    }
}

impl IDisposable for TypeAheadStyle {
    fn dispose(&self) {
        self.stop_tracking();
    }
}

// ---------------------------------------------------------------------------------------------------------------------
// TypeAheadAddon
// ---------------------------------------------------------------------------------------------------------------------

/// `CharPredictState`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CharPredictState {
    /// No characters typed on this line yet
    Unknown,
    /// Has a pending character prediction
    HasPendingChar,
    /// Character validated on this line
    Validated,
}

/// `_lastRow`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct LastRow {
    pub y: i32,
    pub starting_x: i32,
    pub ending_x: i32,
    pub char_state: CharPredictState,
}

/// `compileExcludeRegexp(programs = DEFAULT_LOCAL_ECHO_EXCLUDE)`.
fn compile_exclude_regexp(programs: Option<&[String]>) -> JsRegExp {
    let default: Vec<String> = DEFAULT_LOCAL_ECHO_EXCLUDE.iter().map(|program| (*program).to_owned()).collect();
    let programs = programs.unwrap_or(&default);
    let escaped: Vec<String> = programs.iter().map(|program| escape_reg_exp_characters(&units(program)).to_string_lossy()).collect();
    JsRegExp::from_str(&format!("\\b({})\\b", escaped.join("|")), "i").expect("a lista de programas escapada é um padrão válido")
}

struct AddonState {
    weak_self: Weak<AddonState>,
    store: Rc<DisposableStore>,
    configuration_service: Rc<dyn IConfigurationService>,
    telemetry_service: Rc<dyn ITelemetryService>,
    on_before_process_data: Event<Rc<IBeforeProcessDataEvent>>,

    typeahead_style: RefCell<Option<Rc<TypeAheadStyle>>>,
    typeahead_threshold: Cell<f64>,
    exclude_program_re: RefCell<JsRegExp>,
    last_row: RefCell<Option<LastRow>>,
    timeline: RefCell<Option<Rc<PredictionTimeline>>>,
    terminal_title: RefCell<String>,
    stats: RefCell<Option<Rc<PredictionStats>>>,

    /// Debounce that clears predictions after a timeout if the PTY doesn't apply them.
    clear_prediction_debounce: RefCell<Option<DisposableRef>>,
    /// `nextStatsSend`.
    next_stats_send: Cell<Option<TimerId>>,
    /// `@debounce(100) _reevaluatePredictorState`.
    reevaluate_debounce: Debounce<(), ()>,
}

/// `TypeAheadAddon`.
pub struct TypeAheadAddon {
    store: Rc<DisposableStore>,
    state: Rc<AddonState>,
}

impl TypeAheadAddon {
    pub fn new(on_before_process_data: Event<Rc<IBeforeProcessDataEvent>>, configuration_service: Rc<dyn IConfigurationService>, telemetry_service: Rc<dyn ITelemetryService>) -> Self {
        let store = DisposableStore::new();
        let configuration = Self::read_configuration(configuration_service.as_ref());
        let state = Rc::new_cyclic(|weak: &Weak<AddonState>| {
            let debounce_weak = weak.clone();
            AddonState {
                weak_self: weak.clone(),
                store: store.clone(),
                configuration_service,
                telemetry_service,
                on_before_process_data,
                typeahead_style: RefCell::new(None),
                typeahead_threshold: Cell::new(configuration.local_echo_latency_threshold.unwrap_or(f64::NAN)),
                exclude_program_re: RefCell::new(compile_exclude_regexp(configuration.local_echo_exclude_programs.as_deref())),
                last_row: RefCell::new(None),
                timeline: RefCell::new(None),
                terminal_title: RefCell::new(String::new()),
                stats: RefCell::new(None),
                clear_prediction_debounce: RefCell::new(None),
                next_stats_send: Cell::new(None),
                reevaluate_debounce: Debounce::without_reducer(100, move |()| {
                    if let Some(state) = debounce_weak.upgrade() {
                        let (stats, timeline) = (state.stats.borrow().clone(), state.timeline.borrow().clone());
                        if let (Some(stats), Some(timeline)) = (stats, timeline) {
                            state.reevaluate_predictor_state_now(&stats, &timeline);
                        }
                    }
                }),
            }
        });
        {
            let weak = Rc::downgrade(&state);
            store.add(plain_disposable(move || {
                if let Some(state) = weak.upgrade() {
                    let debounce = state.clear_prediction_debounce.borrow_mut().take();
                    if let Some(debounce) = debounce {
                        debounce.dispose();
                    }
                    if let Some(timer) = state.next_stats_send.take() {
                        runtime::clear_timeout(timer);
                    }
                }
            }));
        }
        Self { store, state }
    }

    fn read_configuration(configuration_service: &dyn IConfigurationService) -> ITerminalTypeAheadConfiguration {
        ITerminalTypeAheadConfiguration::from_dynamic(&configuration_service.get_value(Some(TERMINAL_CONFIG_SECTION), None))
    }

    /// `activate(terminal)` do `ITerminalAddon`.
    pub fn activate(&self, terminal: Rc<dyn ITypeAheadTerminal>) {
        let state = &self.state;
        let configuration = Self::read_configuration(state.configuration_service.as_ref());
        let style = state.store.add_rc(TypeAheadStyle::new(configuration.local_echo_style.as_deref().unwrap_or(""), terminal.clone()));
        *state.typeahead_style.borrow_mut() = Some(style.clone());
        let timeline = state.store.add_rc(PredictionTimeline::new(terminal.clone(), style.clone()));
        *state.timeline.borrow_mut() = Some(timeline.clone());
        let stats = state.store.add_rc(PredictionStats::new(timeline.on_prediction_added(), timeline.on_prediction_succeeded(), timeline.on_prediction_failed()));
        *state.stats.borrow_mut() = Some(stats.clone());

        timeline.set_show_predictions(state.typeahead_threshold.get() == 0.0);
        let weak = state.weak_self.clone();
        state.store.add(terminal.on_data().listen({
            let weak = weak.clone();
            move |e| {
                if let Some(state) = weak.upgrade() {
                    state.on_user_data(e);
                }
            }
        }));
        state.store.add(terminal.on_title_change().listen({
            let weak = weak.clone();
            move |title| {
                if let Some(state) = weak.upgrade() {
                    *state.terminal_title.borrow_mut() = title.clone();
                    state.reevaluate_debounce.call(());
                }
            }
        }));
        state.store.add(terminal.on_resize().listen({
            let weak = weak.clone();
            let timeline = timeline.clone();
            move |_| {
                timeline.set_show_predictions(false);
                timeline.clear_cursor();
                if let Some(state) = weak.upgrade() {
                    state.reevaluate_debounce.call(());
                }
            }
        }));
        state.store.add(state.configuration_service.on_did_change_configuration().listen({
            let weak = weak.clone();
            let style = style.clone();
            move |e| {
                if e.affects_configuration(TERMINAL_CONFIG_SECTION, None) {
                    let Some(state) = weak.upgrade() else {
                        return;
                    };
                    let configuration = Self::read_configuration(state.configuration_service.as_ref());
                    style.on_update(configuration.local_echo_style.as_deref().unwrap_or(""));
                    state.typeahead_threshold.set(configuration.local_echo_latency_threshold.unwrap_or(f64::NAN));
                    *state.exclude_program_re.borrow_mut() = compile_exclude_regexp(configuration.local_echo_exclude_programs.as_deref());
                    state.reevaluate_debounce.call(());
                }
            }
        }));
        state.store.add(timeline.on_prediction_succeeded().listen({
            let weak = weak.clone();
            move |p| {
                let Some(state) = weak.upgrade() else {
                    return;
                };
                let mut last_row = state.last_row.borrow_mut();
                if let Some(last_row) = last_row.as_mut()
                    && last_row.char_state == CharPredictState::HasPendingChar
                    && let Some(pos) = tentative_character_applied_pos(p)
                    && pos.y + pos.base_y == last_row.y
                {
                    last_row.char_state = CharPredictState::Validated;
                }
            }
        }));
        state.store.add(state.on_before_process_data.listen({
            let weak = weak.clone();
            move |e| {
                if let Some(state) = weak.upgrade() {
                    state.on_before_process_data(e);
                }
            }
        }));

        state.store.add(stats.on_change().listen({
            let weak = weak.clone();
            let stats = stats.clone();
            let timeline = timeline.clone();
            move |_| {
                let Some(state) = weak.upgrade() else {
                    return;
                };
                if state.next_stats_send.get().is_none() {
                    let timer_weak = weak.clone();
                    let timer_stats = stats.clone();
                    state.next_stats_send.set(Some(runtime::set_timeout(
                        move || {
                            if let Some(state) = timer_weak.upgrade() {
                                state.send_latency_stats(&timer_stats);
                                state.next_stats_send.set(None);
                            }
                        },
                        STATS_SEND_TELEMETRY_EVERY,
                    )));
                }

                if timeline.length() == 0 {
                    style.debounce_stop_tracking();
                }

                state.reevaluate_debounce.call(());
            }
        }));
    }

    pub fn reset(&self) {
        *self.state.last_row.borrow_mut() = None;
    }

    /// `_lastRow = ...`: o `TestTypeAheadAddon` do original atribui o campo `protected`.
    pub fn set_last_row(&self, last_row: Option<LastRow>) {
        *self.state.last_row.borrow_mut() = last_row;
    }

    /// `this._reevaluatePredictorStateNow(this.stats!, this._timeline!)`.
    pub fn reevaluate_predictor_state_now(&self) {
        let (stats, timeline) = (self.state.stats.borrow().clone(), self.state.timeline.borrow().clone());
        if let (Some(stats), Some(timeline)) = (stats, timeline) {
            self.state.reevaluate_predictor_state_now(&stats, &timeline);
        }
    }

    /// `_timeline`.
    pub fn timeline(&self) -> Option<Rc<PredictionTimeline>> {
        self.state.timeline.borrow().clone()
    }

    /// `stats`.
    pub fn stats(&self) -> Option<Rc<PredictionStats>> {
        self.state.stats.borrow().clone()
    }
}

impl IDisposable for TypeAheadAddon {
    fn dispose(&self) {
        self.store.dispose();
    }
}

impl AddonState {
    fn defer_clearing_predictions(&self) {
        let (Some(stats), Some(timeline)) = (self.stats.borrow().clone(), self.timeline.borrow().clone()) else {
            return;
        };

        let previous = self.clear_prediction_debounce.borrow_mut().take();
        if let Some(previous) = previous {
            previous.dispose();
        }
        if timeline.length() == 0 || timeline.peek_start().is_some_and(|p| !p.clear_after_timeout()) {
            return;
        }

        let weak = self.weak_self.clone();
        let timeout = 500f64.max(stats.max_latency() * 3.0 / 2.0) as u64;
        *self.clear_prediction_debounce.borrow_mut() = Some(disposable_timeout(
            move || {
                let Some(state) = weak.upgrade() else {
                    return;
                };
                let timeline = state.timeline.borrow().clone();
                if let Some(timeline) = timeline {
                    timeline.undo_all_predictions();
                }
                let mut last_row = state.last_row.borrow_mut();
                if let Some(last_row) = last_row.as_mut()
                    && last_row.char_state == CharPredictState::HasPendingChar
                {
                    last_row.char_state = CharPredictState::Unknown;
                }
            },
            timeout,
            Some(self.store.clone()),
        ));
    }

    /// Note on debounce: we want to toggle the state only when the user has a pause in their typing. Otherwise, we could turn
    /// this on when the PTY sent data but the terminal cursor is not updated, causes issues. (O `@debounce(100)` é o campo
    /// `reevaluate_debounce`.)
    fn reevaluate_predictor_state_now(&self, stats: &PredictionStats, timeline: &PredictionTimeline) {
        let threshold = self.typeahead_threshold.get();
        let title = JsString::from(self.terminal_title.borrow().as_str());
        let excluded = {
            let regex = self.exclude_program_re.borrow();
            regex.set_last_index(0);
            regex.exec(title.units()).is_some()
        };
        if excluded {
            timeline.set_show_predictions(false);
        } else if threshold < 0.0 {
            timeline.set_show_predictions(false);
        } else if threshold == 0.0 {
            timeline.set_show_predictions(true);
        } else if stats.sample_size() as f64 > STATS_MIN_SAMPLES_TO_TURN_ON && stats.accuracy() > STATS_MIN_ACCURACY_TO_TURN_ON {
            let latency = stats.latency().median.unwrap_or(f64::NAN);
            if latency >= threshold {
                timeline.set_show_predictions(true);
            } else if latency < threshold / STATS_TOGGLE_OFF_THRESHOLD {
                timeline.set_show_predictions(false);
            }
        }
    }

    fn send_latency_stats(&self, stats: &PredictionStats) {
        let latency = stats.latency();
        let number = |value: Option<f64>| value.map_or(Dynamic::Undefined, Dynamic::Number);
        self.telemetry_service.public_log(
            "terminalLatencyStats",
            Some(ITelemetryData {
                from: None,
                target: None,
                extra: vec![
                    ("count".to_owned(), Dynamic::Number(latency.count as f64)),
                    ("min".to_owned(), number(latency.min)),
                    ("median".to_owned(), number(latency.median)),
                    ("max".to_owned(), number(latency.max)),
                    ("predictionAccuracy".to_owned(), Dynamic::Number(stats.accuracy())),
                ],
            }),
        );
    }

    fn on_user_data(&self, data: &str) {
        let Some(timeline) = self.timeline.borrow().clone() else {
            return;
        };
        let terminal = timeline.terminal.clone();
        let buffer = terminal.buffer_active();
        if !buffer.is_normal() {
            return;
        }

        // Detect programs like git log/less that use the normal buffer but don't
        // take input by deafult (fixes #109541)
        if buffer.cursor_x() == 1 && buffer.cursor_y() == terminal.rows() - 1 && buffer.get_line(buffer.cursor_y() + buffer.base_y()).and_then(|line| line.get_cell(0)).is_some_and(|cell| cell.get_chars() == ":") {
            return;
        }

        // the following code guards the terminal prompt to avoid being able to
        // arrow or backspace-into the prompt. Record the lowest X value at which
        // the user gave input, and mark all additions before that as tentative.
        let actual_y = buffer.base_y() + buffer.cursor_y();
        let current_row = *self.last_row.borrow();
        match current_row {
            Some(mut row) if row.y == actual_y => {
                row.starting_x = row.starting_x.min(buffer.cursor_x());
                row.ending_x = row.ending_x.max(timeline.physical_cursor(&buffer).x());
                *self.last_row.borrow_mut() = Some(row);
            }
            _ => {
                *self.last_row.borrow_mut() = Some(LastRow { y: actual_y, starting_x: buffer.cursor_x(), ending_x: buffer.cursor_x(), char_state: CharPredictState::Unknown });
            }
        }

        let last_row = || (*self.last_row.borrow()).expect("a última linha foi definida acima");
        let add_left_navigating = |p: Rc<dyn IPrediction>| {
            if timeline.tentative_cursor(&buffer).x() <= last_row().starting_x { timeline.add_boundary(Some((&buffer, p))) } else { timeline.add_prediction(&buffer, p) }
        };
        let add_right_navigating = |p: Rc<dyn IPrediction>| {
            if timeline.tentative_cursor(&buffer).x() >= last_row().ending_x - 1 { timeline.add_boundary(Some((&buffer, p))) } else { timeline.add_prediction(&buffer, p) }
        };

        /* @see https://github.com/xtermjs/xterm.js/blob/1913e9512c048e3cf56bb5f5df51bfff6899c184/src/common/input/Keyboard.ts */
        let data_units = units(data);
        let mut reader = StringReader::new(&data_units);
        while reader.remaining() > 0 {
            if reader.eat_char_code(127, 128).is_some() {
                // backspace
                let previous = timeline.peek_end();
                if previous.is_some_and(|previous| previous.as_any().is::<CharacterPrediction>()) {
                    timeline.add_boundary(None);
                }

                // backspace must be able to read the previously-written character in
                // the event that it needs to undo it (o `flushOutput` do original não faz nada)

                if timeline.tentative_cursor(&buffer).x() <= last_row().starting_x {
                    timeline.add_boundary(Some((&buffer, Rc::new(BackspacePrediction::new(timeline.terminal.clone())))));
                } else {
                    // Backspace decrements our ability to go right.
                    if let Some(row) = self.last_row.borrow_mut().as_mut() {
                        row.ending_x -= 1;
                    }
                    timeline.add_prediction(&buffer, Rc::new(BackspacePrediction::new(timeline.terminal.clone())));
                }

                continue;
            }

            if reader.eat_char_code(32, 126).is_some() {
                // alphanum
                let char_ = data_units[reader.index - 1];
                let style = self.typeahead_style.borrow().clone().expect("o estilo existe depois do activate");
                let prediction: Rc<dyn IPrediction> = Rc::new(CharacterPrediction::new(style, char_));
                if last_row().char_state == CharPredictState::Unknown {
                    timeline.add_boundary(Some((&buffer, prediction)));
                    if let Some(row) = self.last_row.borrow_mut().as_mut() {
                        row.char_state = CharPredictState::HasPendingChar;
                    }
                } else {
                    timeline.add_prediction(&buffer, prediction);
                }

                if timeline.tentative_cursor(&buffer).x() >= terminal.cols() {
                    timeline.add_boundary(Some((&buffer, Rc::new(NewlinePrediction::new(true)))));
                }
                continue;
            }

            if let Some(cursor_mv) = reader.eat_re(&csi_move_re()) {
                let direction = if cursor_mv[3].as_deref() == Some(&units("D")[..]) { CursorMoveDirection::Back } else { CursorMoveDirection::Forwards };
                let amount = text_of(cursor_mv[1].as_deref().unwrap_or_default()).parse::<i32>().ok().filter(|amount| *amount != 0).unwrap_or(1);
                let p: Rc<dyn IPrediction> = Rc::new(CursorMovePrediction::new(direction, cursor_mv[2].is_some(), amount));
                if direction == CursorMoveDirection::Back {
                    add_left_navigating(p);
                } else {
                    add_right_navigating(p);
                }
                continue;
            }

            if reader.eat_str(&units(&format!("{VT_ESC}f"))) {
                add_right_navigating(Rc::new(CursorMovePrediction::new(CursorMoveDirection::Forwards, true, 1)));
                continue;
            }

            if reader.eat_str(&units(&format!("{VT_ESC}b"))) {
                add_left_navigating(Rc::new(CursorMovePrediction::new(CursorMoveDirection::Back, true, 1)));
                continue;
            }

            if reader.eat_char(u16::from(b'\r')) && buffer.cursor_y() < terminal.rows() - 1 {
                timeline.add_prediction(&buffer, Rc::new(NewlinePrediction::new(false)));
                continue;
            }

            // something else
            timeline.add_boundary(Some((&buffer, Rc::new(HardBoundary))));
            break;
        }

        if timeline.length() == 1 {
            self.defer_clearing_predictions();
            if let Some(style) = self.typeahead_style.borrow().clone() {
                style.start_tracking();
            }
        }
    }

    fn on_before_process_data(&self, event: &Rc<IBeforeProcessDataEvent>) {
        let Some(timeline) = self.timeline.borrow().clone() else {
            return;
        };

        let data = event.data.borrow().clone();
        *event.data.borrow_mut() = timeline.before_server_input(&data);

        self.defer_clearing_predictions();
    }
}
