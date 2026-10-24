//! Porte de `src/vs/platform/terminal/common/capabilities/commandDetection/promptInputModel.ts`: o modelo do
//! que o usuário digitou no prompt, deduzido da integração de shell e do buffer do terminal. Pode não ser 100%
//! exato, mas dá o melhor palpite.
//!
//! Decisões de tradução:
//! - O `Terminal` do `@xterm/headless` é o trait [`IHeadlessTerminal`] (só os membros que o modelo lê), e o
//!   `IBuffer`, o `IBufferLine` e o `IBufferCell` são os de [`crate::terminal::xterm_headless`].
//! - O texto é UTF-16 ([`JsString`]) e os índices e comprimentos contam unidades UTF-16, como o JavaScript
//!   (o cursor de um texto com caracteres não ASCII depende disso). Os números do xterm e os índices são `i32`
//!   (`-1` é o `-1` do original, em `cursorIndex` e `ghostTextIndex`).
//! - A classe que estende `Disposable` e implementa `IPromptInputModel` é a struct [`PromptInputModel`] com um
//!   [`Disposable`] por composição. Os `@ILogService` são o parâmetro `log_service`. As propriedades de
//!   `IPromptInputModelState` (`value`, `prefix`, `suffix`, `cursorIndex`, `ghostTextIndex`) são métodos do trait
//!   [`IPromptInputModel`], e o objeto congelado que `_createStateObject` devolve é [`IPromptInputModelState`].
//! - `_logService.trace(message, this.getCombinedString())` é `trace_args` com o texto como argumento.
//! - `_handleCommandStart(e as { marker: IMarker })`: o `marker` é um `Option`. O original lança um `TypeError`
//!   (capturado pelo `Emitter`) no ramo do `lastPromptLine` quando falta o marcador; aqui esse ramo é pulado.
//! - `' '.repeat(n)` com `n` negativo lança `RangeError` no original, e o `_sync` captura e registra o erro; aqui o
//!   `_doSync` devolve um `Err` e o `_sync` registra o mesmo.
//! - Em `_scanForGhostTextAdvanced` o original confere `!checkCell?.getChars.length`, que lê o `length` da
//!   *função* `getChars` (a aridade, sempre 0), então a condição é sempre verdadeira e o laço inteiro faz
//!   `continue`. O porte não tem esse laço, porque ele nunca altera o resultado.
//! - O contrato `IPromptInputModel` e o `ISerializedPromptInputModel` que
//!   [`crate::terminal::capabilities::capabilities`] declara sem membros (porque este arquivo ainda não existia) são
//!   substituídos pelos daqui quando o `CommandDetectionCapability` for portado.

use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::{Rc, Weak};

use je_base::errors::Error;
use je_base::event::{Emitter, Event};
use je_base::js_string::{self, JsString};
use je_base::lifecycle::{Disposable, IDisposable};

use crate::log::{LogArg, LogLevel, LogMessage, LogServiceRef};
use crate::terminal::capabilities::capabilities::{IMarker, ITerminalCommand};
use crate::terminal::terminal::{PosixShellType, ShellType, TerminalShellType};
use crate::terminal::xterm_headless::{IBuffer, IBufferCell, IBufferLine, IHeadlessTerminal};

/// `PromptInputState`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PromptInputState {
    Unknown = 0,
    Input = 1,
    Execute = 2,
}

/// `IPromptInputModelState`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct IPromptInputModelState {
    /// The full prompt input include ghost text.
    pub value: JsString,
    /// The prompt input up to the cursor index, this will always exclude the ghost text.
    pub prefix: JsString,
    /// The prompt input from the cursor to the end, this _does not_ include ghost text.
    pub suffix: JsString,
    /// The index of the cursor in `value`.
    pub cursor_index: i32,
    /// The index of the start of ghost text in `value`. This is -1 when there is no ghost text.
    pub ghost_text_index: i32,
}

/// `ISerializedPromptInputModel`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ISerializedPromptInputModel {
    pub model_state: IPromptInputModelState,
    pub command_start_x: i32,
    pub last_prompt_line: Option<JsString>,
    pub continuation_prompt: Option<JsString>,
    pub last_user_input: JsString,
}

/// A model of the prompt input state using shell integration and analyzing the terminal buffer. This
/// may not be 100% accurate but provides a best guess.
pub trait IPromptInputModel {
    fn state(&self) -> PromptInputState;

    /// The full prompt input include ghost text.
    fn value(&self) -> JsString;
    /// The prompt input up to the cursor index, this will always exclude the ghost text.
    fn prefix(&self) -> JsString;
    /// The prompt input from the cursor to the end, this _does not_ include ghost text.
    fn suffix(&self) -> JsString;
    /// The index of the cursor in `value`.
    fn cursor_index(&self) -> i32;
    /// The index of the start of ghost text in `value`. This is -1 when there is no ghost text.
    fn ghost_text_index(&self) -> i32;

    fn on_did_start_input(&self) -> Event<IPromptInputModelState>;
    fn on_did_change_input(&self) -> Event<IPromptInputModelState>;
    fn on_did_finish_input(&self) -> Event<IPromptInputModelState>;
    /// Fires immediately before `on_did_finish_input` when a SIGINT/Ctrl+C/^C is detected.
    fn on_did_interrupt(&self) -> Event<IPromptInputModelState>;

    /// Gets the prompt input as a user-friendly string where `|` is the cursor position and `[` and
    /// `]` wrap any ghost text.
    ///
    /// `empty_string_when_empty`: if true, an empty string is returned when the prompt input is
    /// empty (as opposed to '|').
    fn get_combined_string(&self, empty_string_when_empty: Option<bool>) -> JsString;

    fn set_shell_type(&self, shell_type: TerminalShellType);
}

/// O estado mutável da classe (os campos `_state`, `_commandStartMarker` e companhia).
struct State {
    state: PromptInputState,
    command_start_marker: Option<Rc<dyn IMarker>>,
    command_start_x: i32,
    last_prompt_line: Option<JsString>,
    continuation_prompt: Option<JsString>,
    shell_type: TerminalShellType,
    last_user_input: JsString,
    value: JsString,
    cursor_index: i32,
    ghost_text_index: i32,
}

/// O comprimento em unidades UTF-16 como o `i32` do JavaScript.
fn length(units: &[u16]) -> i32 {
    i32::try_from(units.len()).unwrap_or(i32::MAX)
}

/// `str.substring(start)`.
fn substring_from(units: &[u16], start: i32) -> &[u16] {
    js_string::substring(units, i64::from(start), i64::MAX)
}

/// `str.substring(start, end)`.
fn substring(units: &[u16], start: i32, end: i32) -> &[u16] {
    js_string::substring(units, i64::from(start), i64::from(end))
}

fn unit(c: char) -> u16 {
    u16::try_from(u32::from(c)).expect("o caractere é da BMP")
}

/// `/^ {6,}/.test(text)`.
fn starts_with_six_spaces(text: &[u16]) -> bool {
    text.len() >= 6 && text[..6].iter().all(|&u| u == unit(' '))
}

/// `this._continuationPrompt` ser verdadeiro (existir e não ser vazio).
fn truthy(text: Option<&JsString>) -> Option<&JsString> {
    text.filter(|text| !text.is_empty())
}

/// `!!(this._continuationPrompt && lineText.startsWith(this._continuationPrompt.trimEnd()))`.
fn line_contains_continuation_prompt(continuation_prompt: Option<&JsString>, line_text: &[u16]) -> bool {
    truthy(continuation_prompt).is_some_and(|prompt| js_string::starts_with_at(line_text, js_string::trim_end(prompt.units()), 0))
}

/// `_trimContinuationPrompt`.
fn trim_continuation_prompt(continuation_prompt: Option<&JsString>, line_text: &[u16]) -> Vec<u16> {
    match continuation_prompt {
        Some(prompt) if line_contains_continuation_prompt(Some(prompt), line_text) => substring_from(line_text, length(prompt.units())).to_vec(),
        _ => line_text.to_vec(),
    }
}

/// `_getContinuationPromptCellWidth`.
fn continuation_prompt_cell_width(continuation_prompt: Option<&JsString>, line: &dyn IBufferLine, line_text: &[u16]) -> i32 {
    let Some(prompt) = truthy(continuation_prompt) else {
        return 0;
    };
    if !js_string::starts_with_at(line_text, js_string::trim_end(prompt.units()), 0) {
        return 0;
    }
    let mut buffer: Vec<u16> = Vec::new();
    let mut x = 0;
    while buffer != prompt.units() {
        let cell = line.get_cell(x);
        x += 1;
        let Some(cell) = cell else {
            break;
        };
        buffer.extend_from_slice(cell.get_chars().units());
    }
    x
}

/// `_getRelativeCursorIndex`.
fn relative_cursor_index(start_cell_x: i32, buffer: &dyn IBuffer, line: &dyn IBufferLine) -> i32 {
    length(line.translate_to_string(Some(false), Some(start_cell_x), Some(buffer.cursor_x())).units())
}

/// `_isCellStyledLikeGhostText`.
fn is_cell_styled_like_ghost_text(cell: &dyn IBufferCell) -> bool {
    cell.is_italic() != 0 || cell.is_dim() != 0
}

/// `_getCellStyleAsString`.
fn cell_style_as_string(cell: &dyn IBufferCell) -> String {
    format!(
        "{}{}{}{}{}{}{}{}{}{}{}{}{}",
        cell.get_fg_color(),
        cell.get_bg_color(),
        cell.is_bold(),
        cell.is_italic(),
        cell.is_dim(),
        cell.is_underline(),
        cell.is_blink(),
        cell.is_inverse(),
        cell.is_invisible(),
        cell.is_strikethrough(),
        cell.is_overline(),
        cell.get_fg_color_mode(),
        cell.get_bg_color_mode()
    )
}

/// `_cellStylesMatch`.
fn cell_styles_match(a: Option<&dyn IBufferCell>, b: Option<&dyn IBufferCell>) -> bool {
    let (Some(a), Some(b)) = (a, b) else {
        return false;
    };
    a.get_fg_color() == b.get_fg_color()
        && a.get_bg_color() == b.get_bg_color()
        && a.is_bold() == b.is_bold()
        && a.is_italic() == b.is_italic()
        && a.is_dim() == b.is_dim()
        && a.is_underline() == b.is_underline()
        && a.is_blink() == b.is_blink()
        && a.is_inverse() == b.is_inverse()
        && a.is_invisible() == b.is_invisible()
        && a.is_strikethrough() == b.is_strikethrough()
        && a.is_overline() == b.is_overline()
        && a.get_bg_color_mode() == b.get_bg_color_mode()
        && a.get_fg_color_mode() == b.get_fg_color_mode()
}

/// 5+ spaces preceding the position, following the command start,
/// indicates that we're likely in a right prompt at the current position.
fn is_position_right_prompt(line: &dyn IBufferLine, position: i32, command_start_x: i32) -> bool {
    let mut count = 0;
    let mut i = position - 1;
    while i >= command_start_x {
        let cell = line.get_cell(i);
        // treat missing cell or whitespace-only cell as empty; reset count on first non-empty
        if cell.is_none_or(|cell| js_string::trim(cell.get_chars().units()).is_empty()) {
            count += 1;
            // If we've already found 5 consecutive empties we can early-return
            if count >= 5 {
                return true;
            }
        } else {
            // consecutive sequence broken
            count = 0;
        }
        i -= 1;
    }
    false
}

pub struct PromptInputModel {
    base: Rc<Disposable>,
    this: Weak<Self>,
    xterm: Rc<dyn IHeadlessTerminal>,
    log_service: LogServiceRef,
    state: RefCell<State>,
    on_did_start_input: Rc<Emitter<IPromptInputModelState>>,
    on_did_change_input: Rc<Emitter<IPromptInputModelState>>,
    on_did_finish_input: Rc<Emitter<IPromptInputModelState>>,
    on_did_interrupt: Rc<Emitter<IPromptInputModelState>>,
}

impl PromptInputModel {
    pub fn new(
        xterm: Rc<dyn IHeadlessTerminal>,
        on_command_start: Event<Rc<dyn ITerminalCommand>>,
        on_command_start_changed: Event<()>,
        on_command_executed: Event<Rc<dyn ITerminalCommand>>,
        on_command_finished: Event<Rc<dyn ITerminalCommand>>,
        log_service: LogServiceRef,
    ) -> Rc<Self> {
        let base = Disposable::new();
        let register_emitter = || -> Rc<Emitter<IPromptInputModelState>> {
            let emitter = Emitter::new();
            base.register(emitter.clone());
            emitter
        };
        let on_did_start_input = register_emitter();
        let on_did_change_input = register_emitter();
        let on_did_finish_input = register_emitter();
        let on_did_interrupt = register_emitter();
        let model = Rc::new_cyclic(|this| Self {
            base: base.clone(),
            this: this.clone(),
            xterm,
            log_service,
            state: RefCell::new(State {
                state: PromptInputState::Unknown,
                command_start_marker: None,
                command_start_x: 0,
                last_prompt_line: None,
                continuation_prompt: None,
                shell_type: None,
                last_user_input: JsString::new(),
                value: JsString::new(),
                cursor_index: 0,
                ghost_text_index: -1,
            }),
            on_did_start_input,
            on_did_change_input,
            on_did_finish_input,
            on_did_interrupt,
        });

        // `Event.any(onCursorMove, onData, onWriteParsed)(() => this._sync())`
        model.listen_to(&model.xterm.on_cursor_move(), |this, _| this.sync());
        model.listen_to(&model.xterm.on_data(), |this, _| this.sync());
        model.listen_to(&model.xterm.on_write_parsed(), |this, _| this.sync());
        model.listen_to(&model.xterm.on_data(), |this, data| this.handle_user_input(data));

        model.listen_to(&on_command_start, |this, command| this.handle_command_start(&**command));
        model.listen_to(&on_command_start_changed, |this, _| this.handle_command_start_changed());
        model.listen_to(&on_command_executed, |this, _| this.handle_command_executed());
        model.listen_to(&on_command_finished, |this, _| this.handle_command_finished());

        model.listen_to(&model.on_did_start_input(), |this, _| this.log_combined_string_if_trace("PromptInputModel#onDidStartInput"));
        model.listen_to(&model.on_did_change_input(), |this, _| this.log_combined_string_if_trace("PromptInputModel#onDidChangeInput"));
        model.listen_to(&model.on_did_finish_input(), |this, _| this.log_combined_string_if_trace("PromptInputModel#onDidFinishInput"));
        model.listen_to(&model.on_did_interrupt(), |this, _| this.log_combined_string_if_trace("PromptInputModel#onDidInterrupt"));
        model
    }

    /// `this._register(event(e => handler(e)))`: o ouvinte guarda só uma referência fraca ao modelo.
    fn listen_to<T: 'static>(&self, event: &Event<T>, handler: impl Fn(&Self, &T) + 'static) {
        let this = self.this.clone();
        self.base.register(event.listen(move |value| {
            if let Some(this) = this.upgrade() {
                handler(&*this, value);
            }
        }));
    }

    fn log_combined_string_if_trace(&self, message: &str) {
        // Only generate the combined string if trace
        if self.log_service.get_level() == LogLevel::Trace {
            let combined = self.get_combined_string(None).to_string_lossy();
            self.log_service.trace_args(message, &[LogArg::from(combined.as_str())]);
        }
    }

    pub fn set_continuation_prompt(&self, value: &str) {
        self.state.borrow_mut().continuation_prompt = Some(JsString::from(value));
        self.sync();
    }

    pub fn set_last_prompt_line(&self, value: &str) {
        self.state.borrow_mut().last_prompt_line = Some(JsString::from(value));
        self.sync();
    }

    pub fn set_confident_command_line(&self, value: &str) {
        let value = JsString::from(value);
        {
            let mut state = self.state.borrow_mut();
            if state.value == value {
                return;
            }
            state.value = value;
            state.cursor_index = -1;
            state.ghost_text_index = -1;
        }
        self.on_did_change_input.fire(self.create_state_object());
    }

    pub fn serialize(&self) -> ISerializedPromptInputModel {
        let state = self.state.borrow();
        ISerializedPromptInputModel {
            model_state: Self::state_object(&state),
            command_start_x: state.command_start_x,
            last_prompt_line: state.last_prompt_line.clone(),
            continuation_prompt: state.continuation_prompt.clone(),
            last_user_input: state.last_user_input.clone(),
        }
    }

    pub fn deserialize(&self, serialized: &ISerializedPromptInputModel) {
        let mut state = self.state.borrow_mut();
        state.value = serialized.model_state.value.clone();
        state.cursor_index = serialized.model_state.cursor_index;
        state.ghost_text_index = serialized.model_state.ghost_text_index;
        state.command_start_x = serialized.command_start_x;
        state.last_prompt_line = serialized.last_prompt_line.clone();
        state.continuation_prompt = serialized.continuation_prompt.clone();
        state.last_user_input = serialized.last_user_input.clone();
    }

    fn handle_command_start(&self, command: &dyn ITerminalCommand) {
        {
            let mut state = self.state.borrow_mut();
            if state.state == PromptInputState::Input {
                return;
            }

            state.state = PromptInputState::Input;
            state.command_start_marker = command.marker();
            state.command_start_x = self.xterm.buffer_active().cursor_x();
            state.value = JsString::new();
            state.cursor_index = 0;
        }
        self.on_did_start_input.fire(self.create_state_object());
        self.on_did_change_input.fire(self.create_state_object());

        // Trigger a sync if prompt terminator is set as that could adjust the command start X
        let (last_prompt_line, command_start_x, command_start_marker) = {
            let state = self.state.borrow();
            (truthy(state.last_prompt_line.as_ref()).cloned(), state.command_start_x, state.command_start_marker.clone())
        };
        let (Some(last_prompt_line), Some(command_start_marker)) = (last_prompt_line, command_start_marker) else {
            return;
        };
        let last_prompt_line_length = length(last_prompt_line.units());
        if command_start_x != last_prompt_line_length {
            let line = self.xterm.buffer_active().get_line(marker_line(command_start_marker.as_ref()));
            if line.is_some_and(|line| js_string::starts_with_at(line.translate_to_string(Some(true), None, None).units(), last_prompt_line.units(), 0)) {
                self.state.borrow_mut().command_start_x = last_prompt_line_length;
                self.sync();
            }
        }
    }

    fn handle_command_start_changed(&self) {
        {
            let mut state = self.state.borrow_mut();
            if state.state != PromptInputState::Input {
                return;
            }
            state.command_start_x = self.xterm.buffer_active().cursor_x();
        }
        self.on_did_change_input.fire(self.create_state_object());
        self.sync();
    }

    fn handle_command_executed(&self) {
        let (event, interrupted) = {
            let mut state = self.state.borrow_mut();
            if state.state == PromptInputState::Execute {
                return;
            }

            state.cursor_index = -1;

            // Remove any ghost text from the input if it exists on execute
            if state.ghost_text_index != -1 {
                state.value = JsString::from(substring(state.value.units(), 0, state.ghost_text_index));
                state.ghost_text_index = -1;
            }

            let event = Self::state_object(&state);
            let interrupted = state.last_user_input == "\u{3}";
            if interrupted {
                state.last_user_input = JsString::new();
            }
            (event, interrupted)
        };
        if interrupted {
            self.on_did_interrupt.fire(event.clone());
        }

        self.state.borrow_mut().state = PromptInputState::Execute;
        self.on_did_finish_input.fire(event.clone());
        self.on_did_change_input.fire(event);
    }

    fn handle_command_finished(&self) {
        // Clear the prompt input value when command finishes to prepare for the next command
        // This prevents runCommand from detecting leftover text and sending ^C unnecessarily
        self.state.borrow_mut().value = JsString::new();
        self.on_did_change_input.fire(self.create_state_object());
    }

    fn sync(&self) {
        if let Err(error) = self.do_sync() {
            self.log_service.error_args(&LogMessage::from("Error while syncing prompt input model"), &[LogArg::from(&error)]);
        }
    }

    fn do_sync(&self) -> Result<(), Error> {
        let (start_marker_line, command_start_x, shell_type, continuation_prompt) = {
            let state = self.state.borrow();
            if state.state != PromptInputState::Input {
                return Ok(());
            }
            (
                state.command_start_marker.as_ref().map(|marker| marker_line(marker.as_ref())),
                state.command_start_x,
                state.shell_type,
                state.continuation_prompt.clone(),
            )
        };
        let continuation_prompt = continuation_prompt.as_ref();

        let Some(mut command_start_y) = start_marker_line else {
            return Ok(());
        };

        let buffer = self.xterm.buffer_active();
        let buffer = buffer.as_ref();
        let mut line = buffer.get_line(command_start_y);
        let absolute_cursor_y = buffer.base_y() + buffer.cursor_y();
        let mut cursor_index: Option<i32> = None;

        let mut command_line: Option<Vec<u16>> = line.as_ref().map(|line| line.translate_to_string(Some(true), Some(command_start_x), None).units().to_vec());
        let is_fish = shell_type == Some(ShellType::Posix(PosixShellType::Fish));
        if is_fish && (line.is_none() || command_line.as_ref().is_none_or(Vec::is_empty)) {
            command_start_y += 1;
            line = buffer.get_line(command_start_y);
            if let Some(line) = &line {
                let text = line.translate_to_string(Some(true), None, None).units().to_vec();
                cursor_index = Some(if absolute_cursor_y == command_start_y { buffer.cursor_x() } else { length(js_string::trim_end(&text)) });
                command_line = Some(text);
            }
        }
        let (Some(line), Some(command_line)) = (line, command_line) else {
            self.log_service.trace("PromptInputModel#_sync: no line");
            return Ok(());
        };
        let line = line.as_ref();

        let mut value: Vec<u16> = command_line.clone();
        let mut cursor_index = match cursor_index {
            Some(cursor_index) => cursor_index,
            None if absolute_cursor_y == command_start_y => relative_cursor_index(command_start_x, buffer, line).min(length(&command_line)),
            None => length(js_string::trim_end(&command_line)),
        };

        // From command start line to cursor line
        for y in (command_start_y + 1)..=absolute_cursor_y {
            let next_line = buffer.get_line(y);
            let line_text = next_line.as_ref().map(|next_line| next_line.translate_to_string(Some(true), None, None).units().to_vec());
            let (Some(line_text), Some(next_line)) = (line_text.filter(|text| !text.is_empty()), next_line) else {
                continue;
            };
            let next_line = next_line.as_ref();
            // Check if the line wrapped without a new line (continuation) or
            // we're on the last line and the continuation prompt is not present, so we need to add the value
            if next_line.is_wrapped()
                || (absolute_cursor_y == y && truthy(continuation_prompt).is_some() && !line_contains_continuation_prompt(continuation_prompt, &line_text))
            {
                value.extend_from_slice(&line_text);
                let relative = relative_cursor_index(0, buffer, next_line);
                if absolute_cursor_y == y {
                    cursor_index += relative;
                } else {
                    cursor_index += length(&line_text);
                }
            } else if is_fish {
                if value.last() == Some(&unit('\\')) {
                    // Trim off the trailing backslash
                    value.pop();
                    let trimmed = js_string::trim(&line_text);
                    value.extend_from_slice(trimmed);
                    cursor_index += length(trimmed) - 1;
                } else if starts_with_six_spaces(&line_text) {
                    // Was likely a new line
                    let trimmed = js_string::trim(&line_text);
                    value.push(unit('\n'));
                    value.extend_from_slice(trimmed);
                    cursor_index += length(trimmed) + 1;
                } else {
                    value.extend_from_slice(&line_text);
                    cursor_index += length(&line_text);
                }
            }
            // Verify continuation prompt if we have it, if this line doesn't have it then the
            // user likely just pressed enter.
            else if continuation_prompt.is_none() || line_contains_continuation_prompt(continuation_prompt, &line_text) {
                let trimmed_line_text = trim_continuation_prompt(continuation_prompt, &line_text);
                value.push(unit('\n'));
                value.extend_from_slice(&trimmed_line_text);
                if absolute_cursor_y == y {
                    let continuation_cell_width = continuation_prompt_cell_width(continuation_prompt, next_line, &line_text);
                    let relative = relative_cursor_index(continuation_cell_width, buffer, next_line);
                    cursor_index += relative + 1;
                } else {
                    cursor_index += length(&trimmed_line_text) + 1;
                }
            }
        }

        // Below cursor line
        for y in (absolute_cursor_y + 1)..(buffer.base_y() + self.xterm.rows()) {
            let below_cursor_line = buffer.get_line(y);
            let line_text = below_cursor_line.map(|line| line.translate_to_string(Some(true), None, None).units().to_vec());
            let Some(line_text) = line_text.filter(|text| !text.is_empty()) else {
                break;
            };
            if is_fish {
                value.extend_from_slice(&line_text);
            } else if continuation_prompt.is_none() || line_contains_continuation_prompt(continuation_prompt, &line_text) {
                value.push(unit('\n'));
                value.extend_from_slice(&trim_continuation_prompt(continuation_prompt, &line_text));
            } else {
                value.extend_from_slice(&line_text);
            }
        }

        if self.log_service.get_level() == LogLevel::Trace {
            self.log_service.trace(&format!("PromptInputModel#_sync: {}", self.get_combined_string(None)));
        }

        // Adjust trailing whitespace
        let (previous_value, previous_cursor_index, mut last_user_input) = {
            let state = self.state.borrow();
            (state.value.units().to_vec(), state.cursor_index, state.last_user_input.clone())
        };
        let value = {
            let mut trailing_whitespace = length(&previous_value) - length(js_string::trim_end(&previous_value));

            // Handle backspace key
            if last_user_input == "\u{7F}" {
                last_user_input = JsString::new();
                if cursor_index == previous_cursor_index - 1 {
                    let previous_trimmed = length(js_string::trim_end(&previous_value));
                    let value_trimmed = length(js_string::trim_end(&value));
                    // If trailing whitespace is being increased by removing a non-whitespace character
                    if previous_trimmed > value_trimmed && value_trimmed <= cursor_index {
                        trailing_whitespace = ((length(&previous_value) - 1) - value_trimmed).max(0);
                    }
                    // Standard case; subtract from trailing whitespace
                    else {
                        trailing_whitespace = (trailing_whitespace - 1).max(0);
                    }
                }
            }

            // Handle delete key
            if last_user_input == "\u{1b}[3~" {
                last_user_input = JsString::new();
                if cursor_index == previous_cursor_index {
                    trailing_whitespace = (trailing_whitespace - 1).max(0);
                }
            }

            let mut value_lines: Vec<Vec<u16>> = value.split(|&u| u == unit('\n')).map(<[u16]>::to_vec).collect();
            let is_multi_line = value_lines.len() > 1;
            let value_end_trimmed = length(js_string::trim_end(&value));
            if !is_multi_line {
                // Adjust trimmed whitespace value based on cursor position
                if value_end_trimmed < length(&value) {
                    // Handle space key
                    if last_user_input == " " {
                        last_user_input = JsString::new();
                        if cursor_index > value_end_trimmed && cursor_index > previous_cursor_index {
                            trailing_whitespace += 1;
                        }
                    }
                    trailing_whitespace = (cursor_index - value_end_trimmed).max(trailing_whitespace).max(0);
                }

                // Handle case where a non-space character is inserted in the middle of trailing whitespace
                let char_before_cursor = if cursor_index == 0 { None } else { usize::try_from(cursor_index - 1).ok().and_then(|index| value.get(index)).copied() };
                if trailing_whitespace > 0
                    && cursor_index == previous_cursor_index + 1
                    && !last_user_input.is_empty()
                    && char_before_cursor != Some(unit(' '))
                {
                    trailing_whitespace = length(&previous_value) - previous_cursor_index;
                }
            }

            if is_multi_line {
                if let Some(last) = value_lines.last_mut() {
                    *last = js_string::trim_end(last).to_vec();
                }
                let continuation_offset = (length_of_lines(&value_lines) - 1) * continuation_prompt.map_or(0, |prompt| length(prompt.units()));
                trailing_whitespace = (cursor_index - length(&value) - continuation_offset).max(0);
            }

            self.state.borrow_mut().last_user_input = last_user_input;

            let Ok(repeat) = usize::try_from(trailing_whitespace) else {
                return Err(Error::new(format!("Invalid count value: {trailing_whitespace}")));
            };
            let mut joined: Vec<u16> = Vec::new();
            for (index, value_line) in value_lines.iter().enumerate() {
                if index > 0 {
                    joined.push(unit('\n'));
                }
                joined.extend_from_slice(js_string::trim_end(value_line));
            }
            joined.extend(std::iter::repeat_n(unit(' '), repeat));
            joined
        };

        let ghost_text_index = self.scan_for_ghost_text(buffer, line, cursor_index, command_start_x, start_marker_line);

        let changed = {
            let state = self.state.borrow();
            state.value.units() != value.as_slice() || state.cursor_index != cursor_index || state.ghost_text_index != ghost_text_index
        };
        if changed {
            {
                let mut state = self.state.borrow_mut();
                state.value = JsString::from_units(value);
                state.cursor_index = cursor_index;
                state.ghost_text_index = ghost_text_index;
            }
            self.on_did_change_input.fire(self.create_state_object());
        }
        Ok(())
    }

    fn handle_user_input(&self, data: &JsString) {
        self.state.borrow_mut().last_user_input = data.clone();
    }

    /// Detect ghost text by looking for italic or dim text in or after the cursor and
    /// non-italic/dim text in the first non-whitespace cell following command start and before the cursor.
    fn scan_for_ghost_text(&self, buffer: &dyn IBuffer, line: &dyn IBufferLine, cursor_index: i32, command_start_x: i32, marker_line: Option<i32>) -> i32 {
        if js_string::trim(self.state.borrow().value.units()).is_empty() {
            return -1;
        }
        // Check last non-whitespace character has non-ghost text styles
        let mut ghost_text_index = -1;
        let mut proceed_with_ghost_text_check = false;
        let mut x = buffer.cursor_x();
        while x > 0 {
            x -= 1;
            let Some(cell) = line.get_cell(x) else {
                break;
            };
            if !js_string::trim(cell.get_chars().units()).is_empty() {
                proceed_with_ghost_text_check = !is_cell_styled_like_ghost_text(cell.as_ref());
                break;
            }
        }

        // Check to the end of the line for possible ghost text. For example pwsh's ghost text
        // can look like this `Get-|Ch[ildItem]`
        if proceed_with_ghost_text_check {
            let mut potential_ghost_index_offset = 0;
            let mut x = buffer.cursor_x();

            while x < line.length() {
                let cell = line.get_cell(x);
                x += 1;
                let Some(cell) = cell else {
                    break;
                };
                if cell.get_code() == 0 {
                    break;
                }
                if is_cell_styled_like_ghost_text(cell.as_ref()) {
                    ghost_text_index = cursor_index + potential_ghost_index_offset;
                    break;
                }

                potential_ghost_index_offset += length(cell.get_chars().units());
            }
        }

        // Ghost text may not be italic or dimmed, but will have a different style than the
        // rest of the line that precedes it.
        if ghost_text_index == -1 {
            ghost_text_index = self.scan_for_ghost_text_advanced(buffer, line, cursor_index, command_start_x, marker_line);
        }

        let mut state = self.state.borrow_mut();
        if ghost_text_index > -1 && substring_from(state.value.units(), ghost_text_index).last() == Some(&unit(' ')) {
            state.value = JsString::from(js_string::trim(state.value.units()));
            if substring_from(state.value.units(), ghost_text_index).is_empty() {
                ghost_text_index = -1;
            }
        }
        ghost_text_index
    }

    fn scan_for_ghost_text_advanced(&self, buffer: &dyn IBuffer, line: &dyn IBufferLine, cursor_index: i32, command_start_x: i32, marker_line: Option<i32>) -> i32 {
        let mut ghost_text_index = -1;
        let mut current_pos = buffer.cursor_x(); // Start scanning from the cursor position

        // Map to store styles and their corresponding positions
        let mut style_map: HashMap<String, Vec<i32>> = HashMap::new();

        // Identify the last non-whitespace character in the line
        let mut last_non_whitespace_cell = line.get_cell(current_pos);
        let mut next_cell = last_non_whitespace_cell.clone();

        // Scan from the cursor position to the end of the line
        while let Some(cell) = next_cell.filter(|_| current_pos < line.length()) {
            // Track all occurrences of each unique style in the line
            style_map.entry(cell_style_as_string(cell.as_ref())).or_default().push(current_pos);

            // Move to the next cell
            current_pos += 1;
            next_cell = line.get_cell(current_pos);

            // Update `lastNonWhitespaceCell` only if the new cell contains visible characters
            if let Some(next) = &next_cell
                && !js_string::trim(next.get_chars().units()).is_empty()
            {
                last_non_whitespace_cell = Some(next.clone());
            }
        }

        // If there's no valid last non-whitespace cell OR the first and last styles match (indicating no ghost text)
        let Some(last_non_whitespace_cell) = last_non_whitespace_cell.filter(|cell| !js_string::trim(cell.get_chars().units()).is_empty()) else {
            return -1;
        };
        if cell_styles_match(line.get_cell(command_start_x).as_deref(), Some(last_non_whitespace_cell.as_ref())) {
            return -1;
        }

        // Retrieve the positions of all cells with the same style as `lastNonWhitespaceCell`
        if let Some(positions_with_ghost_style) = style_map.get(&cell_style_as_string(last_non_whitespace_cell.as_ref())) {
            // Ghost text must start at the cursor or one char after (e.g. a space)
            // To account for cursor movement, we also ensure there are not 5+ spaces preceding the ghost text position
            if positions_with_ghost_style[0] > buffer.cursor_x() + 1 && is_position_right_prompt(line, positions_with_ghost_style[0], command_start_x) {
                return -1;
            }
            // Ensure these positions are contiguous
            for pair in positions_with_ghost_style.windows(2) {
                if pair[1] != pair[0] + 1 {
                    // Discontinuous styles, so may be syntax highlighting vs ghost text
                    return -1;
                }
            }
            // Calculate the ghost text start index
            if Some(buffer.base_y() + buffer.cursor_y()) == marker_line {
                ghost_text_index = positions_with_ghost_style[0] - command_start_x;
            } else {
                ghost_text_index = positions_with_ghost_style[0];
            }
        }

        // O laço do original que garante que nenhuma célula anterior tem o estilo de `lastNonWhitespaceCell` confere
        // `!checkCell?.getChars.length` (a aridade da função `getChars`, sempre 0): ele sempre faz `continue` e não
        // altera o resultado, por isso não existe aqui (ver as notas de tradução do módulo).

        if ghost_text_index >= cursor_index { ghost_text_index } else { -1 }
    }

    /// `_createStateObject`.
    fn create_state_object(&self) -> IPromptInputModelState {
        Self::state_object(&self.state.borrow())
    }

    fn state_object(state: &State) -> IPromptInputModelState {
        let value = state.value.units();
        IPromptInputModelState {
            value: state.value.clone(),
            prefix: JsString::from(substring(value, 0, state.cursor_index)),
            suffix: JsString::from(substring(value, state.cursor_index, if state.ghost_text_index == -1 { i32::MAX } else { state.ghost_text_index })),
            cursor_index: state.cursor_index,
            ghost_text_index: state.ghost_text_index,
        }
    }
}

/// `valueLines.length` como o `i32` do JavaScript.
fn length_of_lines(lines: &[Vec<u16>]) -> i32 {
    i32::try_from(lines.len()).unwrap_or(i32::MAX)
}

/// `marker.line`.
fn marker_line(marker: &dyn IMarker) -> i32 {
    // A linha de um marcador é um inteiro, e `-1` quando ele foi descartado.
    marker.line() as i32
}

impl IPromptInputModel for PromptInputModel {
    fn state(&self) -> PromptInputState {
        self.state.borrow().state
    }

    fn value(&self) -> JsString {
        self.state.borrow().value.clone()
    }

    fn prefix(&self) -> JsString {
        self.create_state_object().prefix
    }

    fn suffix(&self) -> JsString {
        self.create_state_object().suffix
    }

    fn cursor_index(&self) -> i32 {
        self.state.borrow().cursor_index
    }

    fn ghost_text_index(&self) -> i32 {
        self.state.borrow().ghost_text_index
    }

    fn on_did_start_input(&self) -> Event<IPromptInputModelState> {
        self.on_did_start_input.event()
    }

    fn on_did_change_input(&self) -> Event<IPromptInputModelState> {
        self.on_did_change_input.event()
    }

    fn on_did_finish_input(&self) -> Event<IPromptInputModelState> {
        self.on_did_finish_input.event()
    }

    fn on_did_interrupt(&self) -> Event<IPromptInputModelState> {
        self.on_did_interrupt.event()
    }

    fn get_combined_string(&self, empty_string_when_empty: Option<bool>) -> JsString {
        let (value, cursor_index, ghost_text_index) = {
            let state = self.state.borrow();
            let value: Vec<u16> = state.value.units().iter().map(|&u| if u == unit('\n') { 0x23CE } else { u }).collect();
            (value, state.cursor_index, state.ghost_text_index)
        };
        if cursor_index == -1 {
            return JsString::from_units(value);
        }
        let mut result: Vec<u16> = substring(&value, 0, cursor_index).to_vec();
        result.push(unit('|'));
        if ghost_text_index != -1 {
            result.extend_from_slice(substring(&value, cursor_index, ghost_text_index));
            result.push(unit('['));
            result.extend_from_slice(substring_from(&value, ghost_text_index));
            result.push(unit(']'));
        } else {
            result.extend_from_slice(substring_from(&value, cursor_index));
        }
        if result == [unit('|')] && empty_string_when_empty.unwrap_or(false) {
            return JsString::new();
        }
        JsString::from_units(result)
    }

    fn set_shell_type(&self, shell_type: TerminalShellType) {
        self.state.borrow_mut().shell_type = shell_type;
    }
}

impl IDisposable for PromptInputModel {
    fn dispose(&self) {
        self.base.dispose();
    }
}
