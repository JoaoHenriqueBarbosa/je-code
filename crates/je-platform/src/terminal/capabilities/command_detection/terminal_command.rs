//! Porte de `src/vs/platform/terminal/common/capabilities/commandDetection/terminalCommand.ts`: o
//! [`TerminalCommand`] (um comando terminado), o [`PartialTerminalCommand`] (o comando em andamento) e as
//! funções que leem o buffer do emulador para extrair a linha de comando e a saída.
//!
//! Decisões de tradução:
//! - O `Terminal` do `@xterm/headless` é o trait [`IHeadlessTerminal`] e os `IBuffer`/`IBufferLine`/`IMarker` são
//!   os contratos de [`crate::terminal::xterm_headless`] e de [`crate::terminal::capabilities::capabilities`].
//! - Os objetos mutáveis e compartilhados do original (`_properties`, os campos do `PartialTerminalCommand`) são
//!   `RefCell` dentro de um `Rc`. [`PartialTerminalCommand::borrow`] e [`PartialTerminalCommand::borrow_mut`]
//!   dão o acesso aos campos; quem os usa não segura o empréstimo enquanto dispara um evento.
//! - `number | undefined` é `Option<f64>` (as colunas e os tempos), e a linha de um marcador é `f64` (`-1` depois
//!   de descartado). `Date.now()` é [`runtime::date_now`].
//! - O texto de uma linha do buffer é UTF-16 ([`JsString`]); a linha de comando, o cwd e a saída são `String`.
//! - O `PartialTerminalCommand` implementa [`ITerminalCommand`] porque o `onCommandExecuted` do original dispara o
//!   próprio `PartialTerminalCommand` como se fosse um `ITerminalCommand` (um truque que o código marca como
//!   `HACK`). As propriedades que o `PartialTerminalCommand` não tem (`marker`, `executedMarker`, `exitCode`...)
//!   valem `None`, como o `undefined` de lá, e `is_full_terminal_command` vale `false`.
//! - [`StartedCommand`] é o literal `{ marker, markProperties } as ITerminalCommand` que o `onCommandStarted`
//!   dispara.
//! - `countNewLines(regex)` lê `regex.multiline` e `regex.source` do [`JsRegExp`], e `lines[0].match(matcher)` é o
//!   [`match_line`] (sem a flag `g`; com ela o `String#match` do original devolveria todos os casamentos como
//!   texto, o que o porte não representa).

use std::cell::{Ref, RefCell, RefMut};
use std::rc::Rc;

use je_base::js_regexp::JsRegExp;
use je_base::js_string::{self, JsString};
use je_base::runtime;
use je_base::uuid::generate_uuid;

use crate::terminal::capabilities::capabilities::{
    IBaseTerminalCommand, IMarkProperties, IMarker, ISerializedTerminalCommand, ITerminalCommand, TerminalCommandConfidence,
};
use crate::terminal::terminal::{ITerminalOutputMatch, ITerminalOutputMatcher, LineMatcher, OutputMatcherAnchor, RegExpMatchArray};
use crate::terminal::xterm_headless::{IBuffer, IHeadlessTerminal};

/// `ITerminalCommandProperties`.
#[derive(Clone)]
pub struct ITerminalCommandProperties {
    pub command: String,
    pub command_line_confidence: TerminalCommandConfidence,
    pub is_trusted: bool,
    pub timestamp: f64,
    pub duration: f64,
    pub id: Option<String>,
    pub marker: Option<Rc<dyn IMarker>>,
    pub cwd: Option<String>,
    pub exit_code: Option<f64>,
    pub command_start_line_content: Option<String>,
    pub mark_properties: Option<IMarkProperties>,
    pub executed_x: Option<f64>,
    pub start_x: Option<f64>,

    pub prompt_start_marker: Option<Rc<dyn IMarker>>,
    pub end_marker: Option<Rc<dyn IMarker>>,
    pub executed_marker: Option<Rc<dyn IMarker>>,
    pub aliases: Option<Vec<Vec<String>>>,
    pub was_replayed: Option<bool>,
}

/// `TerminalCommand`.
pub struct TerminalCommand {
    xterm: Rc<dyn IHeadlessTerminal>,
    properties: RefCell<ITerminalCommandProperties>,
}

/// A linha de um marcador como o índice de linha do buffer.
fn line_index(marker: &dyn IMarker) -> i32 {
    marker.line() as i32
}

impl TerminalCommand {
    pub fn new(xterm: Rc<dyn IHeadlessTerminal>, properties: ITerminalCommandProperties) -> Rc<Self> {
        Rc::new(Self { xterm, properties: RefCell::new(properties) })
    }

    /// `static deserialize(xterm, serialized, isCommandStorageDisabled)`: `serialized.endLine` é obrigatório no
    /// original (`Required<Pick<ISerializedTerminalCommand, 'endLine'>>`); o chamador só passa comandos que o têm.
    pub fn deserialize(xterm: Rc<dyn IHeadlessTerminal>, serialized: &ISerializedTerminalCommand, is_command_storage_disabled: bool) -> Option<Rc<Self>> {
        let buffer = xterm.buffer_normal();
        let anchor = buffer.base_y() + buffer.cursor_y();
        let register = |line: Option<f64>| -> Option<Rc<dyn IMarker>> { line.and_then(|line| xterm.register_marker(line as i32 - anchor)) };
        let marker = register(serialized.start_line);

        // Check for invalid command
        let marker = marker?;
        let prompt_start_marker = register(serialized.prompt_start_line);

        // Valid full command
        let end_marker = register(serialized.end_line);
        let executed_marker = register(serialized.executed_line);
        let new_command = TerminalCommand::new(
            xterm.clone(),
            ITerminalCommandProperties {
                command: if is_command_storage_disabled { String::new() } else { serialized.command.clone() },
                command_line_confidence: serialized.command_line_confidence,
                is_trusted: serialized.is_trusted,
                id: serialized.id.clone(),
                prompt_start_marker,
                marker: Some(marker),
                start_x: serialized.start_x,
                end_marker,
                executed_marker,
                executed_x: serialized.executed_x,
                timestamp: serialized.timestamp,
                duration: serialized.duration,
                cwd: serialized.cwd.clone(),
                command_start_line_content: serialized.command_start_line_content.clone(),
                exit_code: serialized.exit_code,
                mark_properties: serialized.mark_properties.clone(),
                aliases: None,
                was_replayed: Some(true),
            },
        );
        Some(new_command)
    }

    pub fn serialize(&self, is_command_storage_disabled: bool) -> ISerializedTerminalCommand {
        let properties = self.properties.borrow();
        ISerializedTerminalCommand {
            prompt_start_line: properties.prompt_start_marker.as_ref().map(|marker| marker.line()),
            start_line: properties.marker.as_ref().map(|marker| marker.line()),
            start_x: None,
            end_line: properties.end_marker.as_ref().map(|marker| marker.line()),
            executed_line: properties.executed_marker.as_ref().map(|marker| marker.line()),
            executed_x: properties.executed_x,
            command: if is_command_storage_disabled { String::new() } else { properties.command.clone() },
            command_line_confidence: if is_command_storage_disabled { TerminalCommandConfidence::Low } else { properties.command_line_confidence },
            is_trusted: properties.is_trusted,
            cwd: properties.cwd.clone(),
            exit_code: properties.exit_code,
            command_start_line_content: properties.command_start_line_content.clone(),
            timestamp: properties.timestamp,
            duration: properties.duration,
            mark_properties: properties.mark_properties.clone(),
            id: properties.id.clone(),
        }
    }
}

impl IBaseTerminalCommand for TerminalCommand {
    fn command(&self) -> String {
        self.properties.borrow().command.clone()
    }

    fn set_command(&self, command: &str) {
        self.properties.borrow_mut().command = command.to_owned();
    }

    fn command_line_confidence(&self) -> TerminalCommandConfidence {
        self.properties.borrow().command_line_confidence
    }

    fn set_command_line_confidence(&self, confidence: TerminalCommandConfidence) {
        self.properties.borrow_mut().command_line_confidence = confidence;
    }

    fn is_trusted(&self) -> bool {
        self.properties.borrow().is_trusted
    }

    fn timestamp(&self) -> f64 {
        self.properties.borrow().timestamp
    }

    fn duration(&self) -> f64 {
        self.properties.borrow().duration
    }

    fn id(&self) -> Option<String> {
        self.properties.borrow().id.clone()
    }

    fn cwd(&self) -> Option<String> {
        self.properties.borrow().cwd.clone()
    }

    fn exit_code(&self) -> Option<f64> {
        self.properties.borrow().exit_code
    }

    fn command_start_line_content(&self) -> Option<String> {
        self.properties.borrow().command_start_line_content.clone()
    }

    fn mark_properties(&self) -> Option<IMarkProperties> {
        self.properties.borrow().mark_properties.clone()
    }

    fn executed_x(&self) -> Option<f64> {
        self.properties.borrow().executed_x
    }

    fn start_x(&self) -> Option<f64> {
        self.properties.borrow().start_x
    }
}

impl ITerminalCommand for TerminalCommand {
    fn prompt_start_marker(&self) -> Option<Rc<dyn IMarker>> {
        self.properties.borrow().prompt_start_marker.clone()
    }

    fn marker(&self) -> Option<Rc<dyn IMarker>> {
        self.properties.borrow().marker.clone()
    }

    fn end_marker(&self) -> Option<Rc<dyn IMarker>> {
        self.properties.borrow().end_marker.clone()
    }

    fn set_end_marker(&self, marker: Option<Rc<dyn IMarker>>) {
        self.properties.borrow_mut().end_marker = marker;
    }

    fn executed_marker(&self) -> Option<Rc<dyn IMarker>> {
        self.properties.borrow().executed_marker.clone()
    }

    fn aliases(&self) -> Option<Vec<Vec<String>>> {
        self.properties.borrow().aliases.clone()
    }

    fn was_replayed(&self) -> Option<bool> {
        self.properties.borrow().was_replayed
    }

    fn extract_command_line(&self) -> String {
        let properties = self.properties.borrow();
        extract_command_line(
            self.xterm.buffer_active().as_ref(),
            self.xterm.cols(),
            properties.marker.as_deref(),
            properties.start_x,
            properties.executed_marker.as_deref(),
            properties.executed_x,
        )
    }

    fn get_output(&self) -> Option<String> {
        let (executed_marker, end_marker) = {
            let properties = self.properties.borrow();
            (properties.executed_marker.clone()?, properties.end_marker.clone()?)
        };
        let start_line = line_index(executed_marker.as_ref());
        let end_line = line_index(end_marker.as_ref());

        if start_line == end_line {
            return None;
        }
        let mut output = JsString::new();
        let mut current_line = JsString::new();
        let buffer = self.xterm.buffer_active();
        for i in start_line..end_line {
            let Some(line) = buffer.get_line(i) else {
                continue;
            };
            // NOTE: xterm stores wrapping state on the *next* line, not the current one.
            // Use next line's `isWrapped` to determine whether this line should be joined.
            let is_wrapped = if i + 1 < end_line { buffer.get_line(i + 1).is_some_and(|next| next.is_wrapped()) } else { false };
            current_line.push_units(line.translate_to_string(Some(!is_wrapped), None, None).units());
            if !is_wrapped {
                output.push_units(current_line.units());
                output.push_unit(u16::from(b'\n'));
                current_line = JsString::new();
            }
        }
        if !current_line.is_empty() {
            output.push_units(current_line.units());
        }
        if output.is_empty() { None } else { Some(output.to_string_lossy()) }
    }

    fn get_output_match(&self, output_matcher: &ITerminalOutputMatcher) -> Option<ITerminalOutputMatch> {
        // TODO: Add back this check? this._ptyHeuristics.value instanceof WindowsPtyHeuristics && (executedMarker?.line === endMarker?.line) ? this._currentCommand.commandStartMarker : executedMarker
        let (executed_marker, end_marker) = {
            let properties = self.properties.borrow();
            (properties.executed_marker.clone()?, properties.end_marker.clone()?)
        };
        let end_line = line_index(end_marker.as_ref());
        if end_line == -1 {
            return None;
        }
        let buffer = self.xterm.buffer_active();
        let cols = self.xterm.cols();
        let start_line = line_index(executed_marker.as_ref()).max(0);
        let matcher = &output_matcher.line_matcher;
        let lines_to_check = match matcher {
            LineMatcher::Text(_) => 1.0,
            // `outputMatcher.length || countNewLines(matcher)`
            LineMatcher::RegExp(regex) => {
                if output_matcher.length != 0.0 {
                    output_matcher.length
                } else {
                    f64::from(count_new_lines(regex))
                }
            }
        };
        let mut lines: Vec<JsString> = Vec::new();
        let mut found: Option<RegExpMatchArray> = None;
        if output_matcher.anchor == OutputMatcherAnchor::Bottom {
            let mut i = end_line - output_matcher.offset as i32;
            while i >= start_line {
                let mut wrapped_line_start = i;
                let wrapped_line_end = i;
                while wrapped_line_start >= start_line && buffer.get_line(wrapped_line_start).is_some_and(|line| line.is_wrapped()) {
                    wrapped_line_start -= 1;
                }
                i = wrapped_line_start;
                lines.insert(0, get_xterm_line_content(buffer.as_ref(), wrapped_line_start, f64::from(wrapped_line_end), cols));
                if found.is_none() {
                    found = match_line(matcher, &lines[0]);
                }
                if lines.len() as f64 >= lines_to_check {
                    break;
                }
                i -= 1;
            }
        } else {
            let mut i = start_line + output_matcher.offset as i32;
            while i < end_line {
                let wrapped_line_start = i;
                let mut wrapped_line_end = i;
                while wrapped_line_end + 1 < end_line && buffer.get_line(wrapped_line_end + 1).is_some_and(|line| line.is_wrapped()) {
                    wrapped_line_end += 1;
                }
                i = wrapped_line_end;
                lines.push(get_xterm_line_content(buffer.as_ref(), wrapped_line_start, f64::from(wrapped_line_end), cols));
                if found.is_none() {
                    found = match_line(matcher, &lines[lines.len() - 1]);
                }
                if lines.len() as f64 >= lines_to_check {
                    break;
                }
                i += 1;
            }
        }
        found.map(|regex_match| ITerminalOutputMatch { regex_match, output_lines: lines.iter().map(JsString::to_string_lossy).collect() })
    }

    fn has_output(&self) -> bool {
        let properties = self.properties.borrow();
        let executed = properties.executed_marker.as_ref();
        let end = properties.end_marker.as_ref();
        !executed.is_some_and(|marker| marker.is_disposed())
            && !end.is_some_and(|marker| marker.is_disposed())
            && matches!((executed, end), (Some(executed), Some(end)) if executed.line() < end.line())
    }

    fn get_prompt_row_count(&self) -> f64 {
        let properties = self.properties.borrow();
        prompt_row_count(properties.marker.as_deref(), properties.prompt_start_marker.as_deref(), self.xterm.buffer_active().as_ref())
    }

    fn get_command_row_count(&self) -> f64 {
        let properties = self.properties.borrow();
        command_row_count(properties.marker.as_deref(), properties.executed_marker.as_deref(), properties.executed_x)
    }
}

/// `ICurrentPartialCommand`: o comando em andamento, lido pelos consumidores. Os campos são os do original
/// (todos opcionais).
pub trait ICurrentPartialCommand {
    fn prompt_start_marker(&self) -> Option<Rc<dyn IMarker>>;

    fn command_start_marker(&self) -> Option<Rc<dyn IMarker>>;
    fn command_start_x(&self) -> Option<f64>;
    fn command_start_line_content(&self) -> Option<String>;

    fn command_right_prompt_start_x(&self) -> Option<f64>;
    fn command_right_prompt_end_x(&self) -> Option<f64>;

    fn command_lines(&self) -> Option<Rc<dyn IMarker>>;

    fn command_executed_marker(&self) -> Option<Rc<dyn IMarker>>;
    fn command_executed_x(&self) -> Option<f64>;

    fn command_finished_marker(&self) -> Option<Rc<dyn IMarker>>;

    fn current_continuation_marker(&self) -> Option<Rc<dyn IMarker>>;
    fn continuations(&self) -> Option<Vec<Continuation>>;

    fn command(&self) -> Option<String>;

    /// Whether the command line is trusted via a nonce.
    fn is_trusted(&self) -> Option<bool>;

    /// Something invalidated the command before it finished, this will prevent the onCommandFinished
    /// event from firing.
    fn is_invalid(&self) -> Option<bool>;

    fn get_prompt_row_count(&self) -> f64;
    fn get_command_row_count(&self) -> f64;
}

/// `{ marker: IMarker; end: number }` de `continuations`.
#[derive(Clone)]
pub struct Continuation {
    pub marker: Rc<dyn IMarker>,
    pub end: f64,
}

/// Os campos do [`PartialTerminalCommand`].
#[derive(Clone, Default)]
pub struct PartialCommandState {
    pub prompt_start_marker: Option<Rc<dyn IMarker>>,

    pub command_start_marker: Option<Rc<dyn IMarker>>,
    pub command_start_x: Option<f64>,
    pub command_start_line_content: Option<String>,

    pub command_right_prompt_start_x: Option<f64>,
    pub command_right_prompt_end_x: Option<f64>,

    pub command_lines: Option<Rc<dyn IMarker>>,

    pub command_executed_marker: Option<Rc<dyn IMarker>>,
    pub command_executed_x: Option<f64>,

    command_executed_timestamp: Option<f64>,
    command_duration: Option<f64>,

    pub command_finished_marker: Option<Rc<dyn IMarker>>,

    pub current_continuation_marker: Option<Rc<dyn IMarker>>,
    pub continuations: Option<Vec<Continuation>>,

    pub cwd: Option<String>,
    pub command: Option<String>,
    pub command_line_confidence: Option<TerminalCommandConfidence>,
    pub id: Option<String>,

    pub is_trusted: Option<bool>,
    pub is_invalid: Option<bool>,
    /// Track temporarily if the command was recently cleared, this can be used for marker
    /// adjustments
    pub was_cleared: Option<bool>,
}

/// `PartialTerminalCommand`.
pub struct PartialTerminalCommand {
    xterm: Rc<dyn IHeadlessTerminal>,
    state: RefCell<PartialCommandState>,
}

impl PartialTerminalCommand {
    pub fn new(xterm: Rc<dyn IHeadlessTerminal>, id: Option<String>) -> Rc<Self> {
        Rc::new(Self {
            xterm,
            state: RefCell::new(PartialCommandState { id: Some(id.unwrap_or_else(generate_uuid)), ..PartialCommandState::default() }),
        })
    }

    /// Lê os campos. Não segure o empréstimo enquanto dispara um evento.
    pub fn borrow(&self) -> Ref<'_, PartialCommandState> {
        self.state.borrow()
    }

    /// Escreve os campos. Não segure o empréstimo enquanto dispara um evento.
    pub fn borrow_mut(&self) -> RefMut<'_, PartialCommandState> {
        self.state.borrow_mut()
    }

    pub fn serialize(&self, cwd: Option<&str>) -> Option<ISerializedTerminalCommand> {
        let state = self.state.borrow();
        let command_start_marker = state.command_start_marker.as_ref()?;

        Some(ISerializedTerminalCommand {
            prompt_start_line: state.prompt_start_marker.as_ref().map(|marker| marker.line()),
            start_line: Some(command_start_marker.line()),
            start_x: state.command_start_x,
            end_line: None,
            executed_line: None,
            executed_x: None,
            command: String::new(),
            command_line_confidence: TerminalCommandConfidence::Low,
            is_trusted: true,
            cwd: cwd.map(str::to_owned),
            exit_code: None,
            command_start_line_content: None,
            timestamp: 0.0,
            duration: 0.0,
            mark_properties: None,
            id: state.id.clone(),
        })
    }

    pub fn promote_to_full_command(
        &self,
        cwd: Option<&str>,
        exit_code: Option<f64>,
        ignore_command_line: bool,
        mark_properties: Option<IMarkProperties>,
    ) -> Option<Rc<TerminalCommand>> {
        // When the command finishes and executed never fires the placeholder selector should be used.
        {
            let mut state = self.state.borrow_mut();
            if exit_code.is_none() && state.command.is_none() {
                state.command = Some(String::new());
            }
        }

        let state = self.state.borrow();
        if state.command.as_ref().is_some_and(|command| !command.starts_with('\\')) || ignore_command_line {
            let command = state.command.clone().filter(|command| !command.is_empty()).unwrap_or_default();
            return Some(TerminalCommand::new(
                self.xterm.clone(),
                ITerminalCommandProperties {
                    command: if ignore_command_line { String::new() } else { command },
                    command_line_confidence: if ignore_command_line { TerminalCommandConfidence::Low } else { state.command_line_confidence.unwrap_or(TerminalCommandConfidence::Low) },
                    is_trusted: state.is_trusted.unwrap_or(false),
                    id: state.id.clone(),
                    prompt_start_marker: state.prompt_start_marker.clone(),
                    marker: state.command_start_marker.clone(),
                    start_x: state.command_start_x,
                    end_marker: state.command_finished_marker.clone(),
                    executed_marker: state.command_executed_marker.clone(),
                    executed_x: state.command_executed_x,
                    timestamp: runtime::date_now() as f64,
                    duration: state.command_duration.filter(|duration| *duration != 0.0).unwrap_or(0.0),
                    cwd: cwd.map(str::to_owned),
                    exit_code,
                    command_start_line_content: state.command_start_line_content.clone(),
                    mark_properties,
                    aliases: None,
                    was_replayed: None,
                },
            ));
        }

        None
    }

    pub fn mark_executed_time(&self) {
        let mut state = self.state.borrow_mut();
        if state.command_executed_timestamp.is_none() {
            state.command_executed_timestamp = Some(runtime::date_now() as f64);
        }
    }

    pub fn mark_finished_time(&self) {
        let mut state = self.state.borrow_mut();
        if state.command_duration.is_none()
            && let Some(executed) = state.command_executed_timestamp
        {
            state.command_duration = Some(runtime::date_now() as f64 - executed);
        }
    }

    pub fn extract_command_line(&self) -> String {
        let state = self.state.borrow();
        extract_command_line(
            self.xterm.buffer_active().as_ref(),
            self.xterm.cols(),
            state.command_start_marker.as_deref(),
            state.command_start_x,
            state.command_executed_marker.as_deref(),
            state.command_executed_x,
        )
    }

    pub fn get_prompt_row_count(&self) -> f64 {
        let state = self.state.borrow();
        prompt_row_count(state.command_start_marker.as_deref(), state.prompt_start_marker.as_deref(), self.xterm.buffer_active().as_ref())
    }

    pub fn get_command_row_count(&self) -> f64 {
        let state = self.state.borrow();
        command_row_count(state.command_start_marker.as_deref(), state.command_executed_marker.as_deref(), state.command_executed_x)
    }
}

impl ICurrentPartialCommand for PartialTerminalCommand {
    fn prompt_start_marker(&self) -> Option<Rc<dyn IMarker>> {
        self.state.borrow().prompt_start_marker.clone()
    }

    fn command_start_marker(&self) -> Option<Rc<dyn IMarker>> {
        self.state.borrow().command_start_marker.clone()
    }

    fn command_start_x(&self) -> Option<f64> {
        self.state.borrow().command_start_x
    }

    fn command_start_line_content(&self) -> Option<String> {
        self.state.borrow().command_start_line_content.clone()
    }

    fn command_right_prompt_start_x(&self) -> Option<f64> {
        self.state.borrow().command_right_prompt_start_x
    }

    fn command_right_prompt_end_x(&self) -> Option<f64> {
        self.state.borrow().command_right_prompt_end_x
    }

    fn command_lines(&self) -> Option<Rc<dyn IMarker>> {
        self.state.borrow().command_lines.clone()
    }

    fn command_executed_marker(&self) -> Option<Rc<dyn IMarker>> {
        self.state.borrow().command_executed_marker.clone()
    }

    fn command_executed_x(&self) -> Option<f64> {
        self.state.borrow().command_executed_x
    }

    fn command_finished_marker(&self) -> Option<Rc<dyn IMarker>> {
        self.state.borrow().command_finished_marker.clone()
    }

    fn current_continuation_marker(&self) -> Option<Rc<dyn IMarker>> {
        self.state.borrow().current_continuation_marker.clone()
    }

    fn continuations(&self) -> Option<Vec<Continuation>> {
        self.state.borrow().continuations.clone()
    }

    fn command(&self) -> Option<String> {
        self.state.borrow().command.clone()
    }

    fn is_trusted(&self) -> Option<bool> {
        self.state.borrow().is_trusted
    }

    fn is_invalid(&self) -> Option<bool> {
        self.state.borrow().is_invalid
    }

    fn get_prompt_row_count(&self) -> f64 {
        PartialTerminalCommand::get_prompt_row_count(self)
    }

    fn get_command_row_count(&self) -> f64 {
        PartialTerminalCommand::get_command_row_count(self)
    }
}

/// O `PartialTerminalCommand` visto como `ITerminalCommand`, como o `onCommandExecuted` do original o dispara.
impl IBaseTerminalCommand for PartialTerminalCommand {
    fn command(&self) -> String {
        self.state.borrow().command.clone().unwrap_or_default()
    }

    fn set_command(&self, command: &str) {
        self.state.borrow_mut().command = Some(command.to_owned());
    }

    fn command_line_confidence(&self) -> TerminalCommandConfidence {
        self.state.borrow().command_line_confidence.unwrap_or(TerminalCommandConfidence::Low)
    }

    fn set_command_line_confidence(&self, confidence: TerminalCommandConfidence) {
        self.state.borrow_mut().command_line_confidence = Some(confidence);
    }

    fn is_trusted(&self) -> bool {
        self.state.borrow().is_trusted.unwrap_or(false)
    }

    fn timestamp(&self) -> f64 {
        0.0
    }

    fn duration(&self) -> f64 {
        0.0
    }

    fn id(&self) -> Option<String> {
        self.state.borrow().id.clone()
    }

    fn cwd(&self) -> Option<String> {
        self.state.borrow().cwd.clone()
    }

    fn exit_code(&self) -> Option<f64> {
        None
    }

    fn command_start_line_content(&self) -> Option<String> {
        self.state.borrow().command_start_line_content.clone()
    }

    fn mark_properties(&self) -> Option<IMarkProperties> {
        None
    }

    fn executed_x(&self) -> Option<f64> {
        None
    }

    fn start_x(&self) -> Option<f64> {
        None
    }
}

impl ITerminalCommand for PartialTerminalCommand {
    fn prompt_start_marker(&self) -> Option<Rc<dyn IMarker>> {
        self.state.borrow().prompt_start_marker.clone()
    }

    fn marker(&self) -> Option<Rc<dyn IMarker>> {
        None
    }

    fn end_marker(&self) -> Option<Rc<dyn IMarker>> {
        None
    }

    fn set_end_marker(&self, _marker: Option<Rc<dyn IMarker>>) {}

    fn executed_marker(&self) -> Option<Rc<dyn IMarker>> {
        None
    }

    fn aliases(&self) -> Option<Vec<Vec<String>>> {
        None
    }

    fn was_replayed(&self) -> Option<bool> {
        None
    }

    fn is_full_terminal_command(&self) -> bool {
        false
    }

    fn as_partial_terminal_command(&self) -> Option<&PartialTerminalCommand> {
        Some(self)
    }

    fn extract_command_line(&self) -> String {
        PartialTerminalCommand::extract_command_line(self)
    }

    fn get_output(&self) -> Option<String> {
        None
    }

    fn get_output_match(&self, _output_matcher: &ITerminalOutputMatcher) -> Option<ITerminalOutputMatch> {
        None
    }

    fn has_output(&self) -> bool {
        false
    }

    fn get_prompt_row_count(&self) -> f64 {
        PartialTerminalCommand::get_prompt_row_count(self)
    }

    fn get_command_row_count(&self) -> f64 {
        PartialTerminalCommand::get_command_row_count(self)
    }
}

/// O literal `{ marker, markProperties } as ITerminalCommand` que o `onCommandStarted` dispara: só o
/// marcador e as propriedades da marca existem.
pub struct StartedCommand {
    pub marker: Option<Rc<dyn IMarker>>,
    pub mark_properties: Option<IMarkProperties>,
}

impl IBaseTerminalCommand for StartedCommand {
    fn command(&self) -> String {
        String::new()
    }

    fn set_command(&self, _command: &str) {}

    fn command_line_confidence(&self) -> TerminalCommandConfidence {
        TerminalCommandConfidence::Low
    }

    fn set_command_line_confidence(&self, _confidence: TerminalCommandConfidence) {}

    fn is_trusted(&self) -> bool {
        false
    }

    fn timestamp(&self) -> f64 {
        0.0
    }

    fn duration(&self) -> f64 {
        0.0
    }

    fn id(&self) -> Option<String> {
        None
    }

    fn cwd(&self) -> Option<String> {
        None
    }

    fn exit_code(&self) -> Option<f64> {
        None
    }

    fn command_start_line_content(&self) -> Option<String> {
        None
    }

    fn mark_properties(&self) -> Option<IMarkProperties> {
        self.mark_properties.clone()
    }

    fn executed_x(&self) -> Option<f64> {
        None
    }

    fn start_x(&self) -> Option<f64> {
        None
    }
}

impl ITerminalCommand for StartedCommand {
    fn prompt_start_marker(&self) -> Option<Rc<dyn IMarker>> {
        None
    }

    fn marker(&self) -> Option<Rc<dyn IMarker>> {
        self.marker.clone()
    }

    fn end_marker(&self) -> Option<Rc<dyn IMarker>> {
        None
    }

    fn set_end_marker(&self, _marker: Option<Rc<dyn IMarker>>) {}

    fn executed_marker(&self) -> Option<Rc<dyn IMarker>> {
        None
    }

    fn aliases(&self) -> Option<Vec<Vec<String>>> {
        None
    }

    fn was_replayed(&self) -> Option<bool> {
        None
    }

    fn is_full_terminal_command(&self) -> bool {
        false
    }

    fn extract_command_line(&self) -> String {
        String::new()
    }

    fn get_output(&self) -> Option<String> {
        None
    }

    fn get_output_match(&self, _output_matcher: &ITerminalOutputMatcher) -> Option<ITerminalOutputMatch> {
        None
    }

    fn has_output(&self) -> bool {
        false
    }

    fn get_prompt_row_count(&self) -> f64 {
        1.0
    }

    fn get_command_row_count(&self) -> f64 {
        1.0
    }
}

fn extract_command_line(
    buffer: &dyn IBuffer,
    cols: i32,
    command_start_marker: Option<&dyn IMarker>,
    command_start_x: Option<f64>,
    command_executed_marker: Option<&dyn IMarker>,
    command_executed_x: Option<f64>,
) -> String {
    let (Some(command_start_marker), Some(command_executed_marker), Some(command_start_x), Some(command_executed_x)) =
        (command_start_marker, command_executed_marker, command_start_x, command_executed_x)
    else {
        return String::new();
    };
    let mut content = JsString::new();
    let start_line = line_index(command_start_marker);
    let executed_line = line_index(command_executed_marker);
    for i in start_line..=executed_line {
        if let Some(line) = buffer.get_line(i) {
            let start_column = if i == start_line { command_start_x as i32 } else { 0 };
            let end_column = if i == executed_line { command_executed_x as i32 } else { cols };
            content.push_units(line.translate_to_string(Some(true), Some(start_column), Some(end_column)).units());
        }
    }
    content.to_string_lossy()
}

/// `getXtermLineContent`. `lineEnd` é `number` no original: o teto `Math.max(2048 / cols * 2)` não é inteiro.
pub fn get_xterm_line_content(buffer: &dyn IBuffer, line_start: i32, line_end: f64, cols: i32) -> JsString {
    // Cap the maximum number of lines generated to prevent potential performance problems. This is
    // more of a sanity check as the wrapped line should already be trimmed down at this point.
    let max_line_length = 2048.0 / f64::from(cols) * 2.0;
    let line_end = line_end.min(f64::from(line_start) + max_line_length);
    let mut content = JsString::new();
    let mut i = line_start;
    while f64::from(i) <= line_end {
        // Make sure only 0 to cols are considered as resizing when windows mode is enabled will
        // retain buffer data outside of the terminal width as reflow is disabled.
        if let Some(line) = buffer.get_line(i) {
            content.push_units(line.translate_to_string(Some(true), Some(0), Some(cols)).units());
        }
        i += 1;
    }
    content
}

/// `countNewLines`.
fn count_new_lines(regex: &JsRegExp) -> i32 {
    if !regex.flags().multiline {
        return 1;
    }
    let source = regex.source();
    let needle: Vec<u16> = "\\n".encode_utf16().collect();
    let mut count = 1;
    let mut i = js_string::index_of(source, &needle, 0);
    while let Some(index) = i {
        count += 1;
        i = js_string::index_of(source, &needle, index + 1);
    }
    count
}

/// `line.match(matcher)`: com um texto, o `String#match` cria um `RegExp` com ele.
fn match_line(matcher: &LineMatcher, line: &JsString) -> Option<RegExpMatchArray> {
    let found = match matcher {
        LineMatcher::Text(text) => {
            let regex = JsRegExp::from_str(text, "").ok()?;
            regex.exec(line.units())
        }
        LineMatcher::RegExp(regex) => {
            regex.set_last_index(0);
            regex.exec(line.units())
        }
    };
    found.map(|matched| RegExpMatchArray { input: line.clone(), matched })
}

/// `getPromptRowCount(command, buffer)`, com o marcador do comando (`marker` ou `commandStartMarker`) e o
/// `promptStartMarker`.
fn prompt_row_count(marker: Option<&dyn IMarker>, prompt_start_marker: Option<&dyn IMarker>, buffer: &dyn IBuffer) -> f64 {
    let (Some(marker), Some(prompt_start_marker)) = (marker, prompt_start_marker) else {
        return 1.0;
    };
    let mut prompt_start_line = line_index(prompt_start_marker);
    let marker_line = line_index(marker);
    // Trim any leading whitespace-only lines to retain vertical space
    while prompt_start_line < marker_line && buffer.get_line(prompt_start_line).map_or(0, |line| line.translate_to_string(Some(true), None, None).len()) == 0 {
        prompt_start_line += 1;
    }
    f64::from(marker_line - prompt_start_line + 1)
}

/// `getCommandRowCount(command)`, com o marcador do comando, o marcador de execução e o `executedX`.
fn command_row_count(marker: Option<&dyn IMarker>, executed_marker: Option<&dyn IMarker>, executed_x: Option<f64>) -> f64 {
    let (Some(marker), Some(executed_marker)) = (marker, executed_marker) else {
        return 1.0;
    };
    let command_executed_line = executed_marker.line().max(marker.line());
    let mut command_row_count = command_executed_line - marker.line() + 1.0;
    // Trim the last line if the cursor X is in the left-most cell
    if executed_x == Some(0.0) {
        command_row_count -= 1.0;
    }
    command_row_count
}

/// `isFullTerminalCommand(command)`: o comando é um `ITerminalCommand` completo (e não o
/// `PartialTerminalCommand`).
pub fn is_full_terminal_command(command: &dyn ITerminalCommand) -> bool {
    command.is_full_terminal_command()
}
