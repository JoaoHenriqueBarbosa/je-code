//! Porte de `src/vs/platform/terminal/common/capabilities/commandDetectionCapability.ts`: a capacidade que
//! identifica com confiança prompts, comandos e saídas de comando no buffer, a partir das sequências da
//! integração com o shell, e as heurísticas por plataforma (`UnixPtyHeuristics` e `WindowsPtyHeuristics`).
//!
//! Decisões de tradução:
//! - O `Terminal` do `@xterm/headless` é o trait [`IHeadlessTerminal`]. As linhas dos marcadores são `f64` (`-1`
//!   quando descartados) e as colunas do cursor são `i32`; as colunas guardadas nos comandos são `f64` (o
//!   `number | undefined` do original). `_commands: TerminalCommand[]` guarda `Rc<TerminalCommand>`.
//! - A classe que estende `Disposable` é a struct [`CommandDetectionCapability`] com um [`Disposable`] por
//!   composição; os `Emitter` são registrados nele. Os ouvintes guardam só uma referência fraca à capacidade.
//! - `@debounce(500)` do `_handleCursorMove` é o [`Debounce`] de `je_base::decorators`.
//! - `_ptyHeuristics` (um `MandatoryMutableDisposable<IPtyHeuristics>`) é o enum [`PtyHeuristics`]; trocar de
//!   variante descarta a anterior. Os `hooks` que o original passa às heurísticas (`ICommandDetectionHeuristicsHooks`)
//!   são os métodos `pub(super)` da capacidade que as heurísticas chamam por uma referência fraca.
//! - O `onCommandStarted` dispara o literal `{ marker, markProperties } as ITerminalCommand` ([`StartedCommand`]) e o
//!   `onCommandExecuted` dispara o próprio `PartialTerminalCommand` (o `HACK` do original); os dois são
//!   `Rc<dyn ITerminalCommand>`.
//! - `executingCommandConfidence` lê `isFullTerminalCommand(this._currentCommand)`, que é sempre falso para o
//!   `PartialTerminalCommand` (ele não tem `hasOutput`): o resultado é sempre `None`.
//! - Nos comandos que o original lê com `marker!` (um `TypeError` se faltar), a linha que falta vira `NaN`, de
//!   modo que as comparações dão falso e nada lança.
//! - Em `_getWindowsPrompt`, `lineText.match(/^(?<prompt>>>> )/g)?.groups?.prompt` com a flag `g` devolve a lista
//!   dos casamentos, que não tem `groups`: o resultado é sempre `undefined`, então o ramo do prompt do Python
//!   nunca devolve nada e o porte não o tem.
//! - `__isCommandStorageDisabled` e as demais variáveis privadas ficam em [`State`].
//! - `_waitForCursorMove` (uma `Promise` com `setInterval`) é o [`Promise`] de `je_base::promise` com o
//!   [`set_interval`] de `je_base::async_`.

use std::cell::{Cell, RefCell};
use std::rc::{Rc, Weak};

use je_base::async_::{IntervalHandle, RunOnceScheduler, clear_interval, set_interval};
use je_base::decorators::Debounce;
use je_base::event::{Emitter, Event};
use je_base::js_string::{self, JsString};
use je_base::lifecycle::{Disposable, DisposableRef, IDisposable};
use je_base::promise::Promise;
use zyarr::RegExp;

use super::capabilities::{
    CommandInvalidationReason, ICommandDetectionCapability, ICommandInvalidationRequest, ICurrentPartialCommand, IHandleCommandOptions, IMarker,
    IPromptInputModel, ISerializedCommandDetectionCapability, ISerializedTerminalCommand, ITerminalCommand, TerminalCapability, TerminalCommandConfidence,
    TerminalCommandForLine,
};
use super::command_detection::prompt_input_model::PromptInputModel;
use super::command_detection::terminal_command::{PartialTerminalCommand, StartedCommand, TerminalCommand, get_xterm_line_content};
use crate::log::{LogArg, LogServiceRef};
use crate::terminal::terminal::{ITerminalOutputMatcher, OutputMatcherAnchor};
use crate::terminal::xterm_headless::{CsiParam, IBuffer, IHeadlessTerminal, TerminalResize};

/// `ITerminalDimensions`.
type ITerminalDimensions = TerminalResize;

/// O estado mutável da classe.
struct State {
    commands: Vec<Rc<TerminalCommand>>,
    cwd: Option<String>,
    prompt_terminator: Option<String>,
    current_command: Rc<PartialTerminalCommand>,
    command_markers: Vec<Rc<dyn IMarker>>,
    dimensions: ITerminalDimensions,
    is_command_storage_disabled: bool,
    handle_command_start_options: Option<IHandleCommandOptions>,
    has_rich_command_detection: bool,
    next_command_id: Option<(String, Option<String>)>,
    is_current_command_interrupted: bool,
}

/// `_ptyHeuristics`: o comportamento por plataforma.
#[derive(Clone)]
enum PtyHeuristics {
    Unix(Rc<UnixPtyHeuristics>),
    Windows(Rc<WindowsPtyHeuristics>),
}

impl PtyHeuristics {
    fn dispose(&self) {
        match self {
            Self::Unix(heuristics) => heuristics.dispose(),
            Self::Windows(heuristics) => heuristics.dispose(),
        }
    }
}

/// `new LogArg(...)` de um texto.
fn text_args(args: &[String]) -> Vec<LogArg> {
    args.iter().map(|arg| LogArg::from(arg.as_str())).collect()
}

/// `cloneMarker(xterm, marker, offset = 0)`.
fn clone_marker(xterm: &dyn IHeadlessTerminal, marker: &dyn IMarker, offset: i32) -> Option<Rc<dyn IMarker>> {
    let buffer = xterm.buffer_active();
    xterm.register_marker(marker.line() as i32 - (buffer.base_y() + buffer.cursor_y()) + offset)
}

/// `(command.promptStartMarker ?? command.marker!).line`.
fn command_prompt_line(command: &TerminalCommand) -> f64 {
    ITerminalCommand::prompt_start_marker(command).or_else(|| ITerminalCommand::marker(command)).map_or(f64::NAN, |marker| marker.line())
}

/// `CommandDetectionCapability`.
pub struct CommandDetectionCapability {
    base: Rc<Disposable>,
    this: Weak<Self>,
    terminal: Rc<dyn IHeadlessTerminal>,
    log_service: LogServiceRef,
    prompt_input_model: Rc<PromptInputModel>,
    state: RefCell<State>,
    pty_heuristics: RefCell<PtyHeuristics>,
    is_disposed: Cell<bool>,
    handle_cursor_move_debounce: Debounce<(), ()>,

    on_command_started: Rc<Emitter<Rc<dyn ITerminalCommand>>>,
    on_command_start_changed: Rc<Emitter<()>>,
    on_before_command_finished: Rc<Emitter<Rc<dyn ITerminalCommand>>>,
    on_command_finished: Rc<Emitter<Rc<dyn ITerminalCommand>>>,
    on_command_executed: Rc<Emitter<Rc<dyn ITerminalCommand>>>,
    on_command_invalidated: Rc<Emitter<Vec<Rc<dyn ITerminalCommand>>>>,
    on_current_command_invalidated: Rc<Emitter<ICommandInvalidationRequest>>,
    on_set_rich_command_detection: Rc<Emitter<bool>>,
}

impl CommandDetectionCapability {
    pub fn new(terminal: Rc<dyn IHeadlessTerminal>, log_service: LogServiceRef) -> Rc<Self> {
        let base = Disposable::new();
        fn register_emitter<T: 'static>(base: &Disposable) -> Rc<Emitter<T>> {
            let emitter = Emitter::new();
            base.register(emitter.clone());
            emitter
        }
        let on_command_started: Rc<Emitter<Rc<dyn ITerminalCommand>>> = register_emitter(&base);
        let on_command_start_changed: Rc<Emitter<()>> = register_emitter(&base);
        let on_before_command_finished: Rc<Emitter<Rc<dyn ITerminalCommand>>> = register_emitter(&base);
        let on_command_finished: Rc<Emitter<Rc<dyn ITerminalCommand>>> = register_emitter(&base);
        let on_command_executed: Rc<Emitter<Rc<dyn ITerminalCommand>>> = register_emitter(&base);
        let on_command_invalidated: Rc<Emitter<Vec<Rc<dyn ITerminalCommand>>>> = register_emitter(&base);
        let on_current_command_invalidated: Rc<Emitter<ICommandInvalidationRequest>> = register_emitter(&base);
        let on_set_rich_command_detection: Rc<Emitter<bool>> = register_emitter(&base);

        let current_command = PartialTerminalCommand::new(terminal.clone(), None);
        let prompt_input_model = PromptInputModel::new(
            terminal.clone(),
            on_command_started.event(),
            on_command_start_changed.event(),
            on_command_executed.event(),
            on_command_finished.event(),
            log_service.clone(),
        );
        base.register(prompt_input_model.clone());

        let dimensions = ITerminalDimensions { cols: terminal.cols(), rows: terminal.rows() };
        let capability = Rc::new_cyclic(|this: &Weak<Self>| {
            let weak = this.clone();
            let unix: Rc<UnixPtyHeuristics> = Rc::new(UnixPtyHeuristics { terminal: terminal.clone(), capability: weak.clone(), log_service: log_service.clone() });
            Self {
                base: base.clone(),
                this: this.clone(),
                terminal: terminal.clone(),
                log_service: log_service.clone(),
                prompt_input_model,
                state: RefCell::new(State {
                    commands: Vec::new(),
                    cwd: None,
                    prompt_terminator: None,
                    current_command,
                    command_markers: Vec::new(),
                    dimensions,
                    is_command_storage_disabled: false,
                    handle_command_start_options: None,
                    has_rich_command_detection: false,
                    next_command_id: None,
                    is_current_command_interrupted: false,
                }),
                pty_heuristics: RefCell::new(PtyHeuristics::Unix(unix)),
                is_disposed: Cell::new(false),
                handle_cursor_move_debounce: Debounce::without_reducer(500, move |()| {
                    if let Some(capability) = weak.upgrade() {
                        capability.handle_cursor_move();
                    }
                }),
                on_command_started,
                on_command_start_changed,
                on_before_command_finished,
                on_command_finished,
                on_command_executed,
                on_command_invalidated,
                on_current_command_invalidated,
                on_set_rich_command_detection,
            }
        });

        capability.listen_to(&capability.prompt_input_model.on_did_interrupt(), |this, _| this.state.borrow_mut().is_current_command_interrupted = true);

        // Pull command line from the buffer if it was not set explicitly
        capability.listen_to(&capability.on_command_executed.event(), |this, command| this.pull_command_line_from_buffer(command));

        let weak = Rc::downgrade(&capability);
        capability.base.register(capability.terminal.parser().register_csi_handler(
            'J',
            Rc::new(move |params: &[CsiParam]| {
                if let Some(this) = weak.upgrade()
                    && !params.is_empty()
                    && params[0].is(2)
                {
                    if !this.terminal.scroll_on_erase_in_display() {
                        this.clear_commands_in_viewport();
                    }
                    this.current_command().borrow_mut().was_cleared = Some(true);
                }
                // We don't want to override xterm.js' default behavior, just augment it
                false
            }),
        ));

        capability.listen_to(&capability.terminal.on_resize(), |this, e| this.handle_resize(*e));
        capability.listen_to(&capability.terminal.on_cursor_move(), |this, _| this.handle_cursor_move_debounce.call(()));
        capability
    }

    /// `this._register(event(e => handler(e)))`: o ouvinte guarda só uma referência fraca à capacidade.
    fn listen_to<T: 'static>(&self, event: &Event<T>, handler: impl Fn(&Self, &T) + 'static) {
        let this = self.this.clone();
        self.base.register(event.listen(move |value| {
            if let Some(this) = this.upgrade() {
                handler(&*this, value);
            }
        }));
    }

    fn debug(&self, message: &str, args: &[String]) {
        self.log_service.debug_args(message, &text_args(args));
    }

    /// O `_currentCommand`.
    pub fn current_command(&self) -> Rc<PartialTerminalCommand> {
        self.state.borrow().current_command.clone()
    }

    /// `get commands()`.
    pub fn command_list(&self) -> Vec<Rc<TerminalCommand>> {
        self.state.borrow().commands.clone()
    }

    /// O `_commands` sobrescrito pela subclasse `TestCommandDetectionCapability.setCommands` dos testes (o campo é
    /// `protected` no original).
    pub fn set_commands(&self, commands: Vec<Rc<TerminalCommand>>) {
        self.state.borrow_mut().commands = commands;
    }

    /// `get promptTerminator()`.
    pub fn prompt_terminator(&self) -> Option<String> {
        self.state.borrow().prompt_terminator.clone()
    }

    fn heuristics(&self) -> PtyHeuristics {
        self.pty_heuristics.borrow().clone()
    }

    fn set_heuristics(&self, heuristics: PtyHeuristics) {
        let previous = self.pty_heuristics.replace(heuristics);
        previous.dispose();
    }

    /// `this._ptyHeuristics.value instanceof WindowsPtyHeuristics`.
    fn is_windows_heuristics(&self) -> bool {
        matches!(*self.pty_heuristics.borrow(), PtyHeuristics::Windows(_))
    }

    /// O listener de `onCommandExecuted` que puxa a linha de comando do buffer quando ela não foi definida de forma
    /// explícita.
    fn pull_command_line_from_buffer(&self, command: &Rc<dyn ITerminalCommand>) {
        if command.command_line_confidence() == TerminalCommandConfidence::High {
            return;
        }
        // HACK: onCommandExecuted actually fired with PartialTerminalCommand
        command.set_command(&command.extract_command_line());
        command.set_command_line_confidence(TerminalCommandConfidence::Low);

        // ITerminalCommand
        if command.is_full_terminal_command() {
            if
            // Markers exist
            command.prompt_start_marker().is_some() && command.marker().is_some() && command.executed_marker().is_some() &&
                // Single line command
                !command.command().contains('\n') &&
                // Start marker is not on the left-most column
                command.start_x().is_some_and(|start_x| start_x > 0.0)
            {
                command.set_command_line_confidence(TerminalCommandConfidence::Medium);
            }
        }
        // PartialTerminalCommand
        else if let Some(partial) = command.as_partial_terminal_command() {
            let is_medium = {
                let state = partial.borrow();
                state.prompt_start_marker.is_some()
                    && state.command_start_marker.is_some()
                    && state.command_executed_marker.is_some()
                    && !command.command().contains('\n')
                    && state.command_start_x.is_some_and(|start_x| start_x > 0.0)
            };
            if is_medium {
                command.set_command_line_confidence(TerminalCommandConfidence::Medium);
            }
        }
    }

    fn handle_resize(&self, e: TerminalResize) {
        if let PtyHeuristics::Windows(heuristics) = self.heuristics() {
            heuristics.pre_handle_resize(e);
        }
        let mut state = self.state.borrow_mut();
        state.dimensions.cols = e.cols;
        state.dimensions.rows = e.rows;
    }

    fn handle_cursor_move(&self) {
        if self.is_disposed.get() {
            return;
        }
        // Early versions of conpty do not have real support for an alt buffer, in addition certain
        // commands such as tsc watch will write to the top of the normal buffer. The following
        // checks when the cursor has moved while the normal buffer is empty and if it is above the
        // current command, all decorations within the viewport will be invalidated.
        //
        // This function is debounced so that the cursor is only checked when it is stable so
        // conpty's screen reprinting will not trigger decoration clearing.
        //
        // This is mostly a workaround for Windows but applies to all OS' because of the tsc watch
        // case.
        let current = self.current_command();
        let command_start_marker = current.borrow().command_start_marker.clone();
        if self.terminal.is_active_buffer_normal()
            && let Some(command_start_marker) = command_start_marker
        {
            let buffer = self.terminal.buffer_active();
            if f64::from(buffer.base_y() + buffer.cursor_y()) < command_start_marker.line() {
                self.clear_commands_in_viewport();
                current.borrow_mut().is_invalid = Some(true);
                self.on_current_command_invalidated.fire(ICommandInvalidationRequest { reason: CommandInvalidationReason::Windows });
            }
        }
    }

    /// `_clearCommandsInViewport` (um dos `hooks` das heurísticas).
    pub(super) fn clear_commands_in_viewport(&self) {
        // Find the number of commands on the tail end of the array that are within the viewport
        let base_y = f64::from(self.terminal.buffer_active().base_y());
        let removed: Vec<Rc<TerminalCommand>> = {
            let mut state = self.state.borrow_mut();
            let mut count = 0;
            for command in state.commands.iter().rev() {
                let line = ITerminalCommand::marker(command.as_ref()).map(|marker| marker.line());
                if line.is_some_and(|line| line != 0.0 && line < base_y) {
                    break;
                }
                count += 1;
            }
            // Remove them
            let split_at = state.commands.len() - count;
            state.commands.split_off(split_at)
        };
        if !removed.is_empty() {
            self.on_command_invalidated.fire(removed.into_iter().map(|command| command as Rc<dyn ITerminalCommand>).collect());
        }
    }

    // As partes que as heurísticas usam (os `hooks` do original).

    pub(super) fn fire_command_started(&self, command: Rc<dyn ITerminalCommand>) {
        self.on_command_started.fire(command);
    }

    pub(super) fn fire_command_executed(&self, command: Rc<dyn ITerminalCommand>) {
        self.on_command_executed.fire(command);
    }

    pub(super) fn fire_current_command_invalidated(&self, request: ICommandInvalidationRequest) {
        self.on_current_command_invalidated.fire(request);
    }

    pub(super) fn dimensions(&self) -> ITerminalDimensions {
        self.state.borrow().dimensions
    }

    pub(super) fn command_markers(&self) -> Vec<Rc<dyn IMarker>> {
        self.state.borrow().command_markers.clone()
    }

    pub(super) fn set_command_markers(&self, markers: Vec<Rc<dyn IMarker>>) {
        self.state.borrow_mut().command_markers = markers;
    }

    pub(super) fn on_before_command_finished(&self) -> Event<Rc<dyn ITerminalCommand>> {
        self.on_before_command_finished.event()
    }

    fn ensure_current_command_id(&self) {
        let mut state = self.state.borrow_mut();
        let Some((_, Some(command_id))) = state.next_command_id.clone() else {
            return;
        };
        // Assign the pre-set command ID to the current command. The timing of setNextCommandId
        // (called right before runCommand) and _ensureCurrentCommandId (called on command
        // executed) ensures we're matching the right command without needing string comparison.
        let current = state.current_command.clone();
        if current.borrow().id.as_deref() != Some(command_id.as_str()) {
            current.borrow_mut().id = Some(command_id);
        }
        state.next_command_id = None;
    }

    /// `this._terminal.buffer.active.baseY + this._terminal.buffer.active.cursorY`.
    fn absolute_cursor_y(&self) -> i32 {
        let buffer = self.terminal.buffer_active();
        buffer.base_y() + buffer.cursor_y()
    }
}

impl IDisposable for CommandDetectionCapability {
    fn dispose(&self) {
        self.is_disposed.set(true);
        self.heuristics().dispose();
        self.base.dispose();
    }
}

impl ICommandDetectionCapability for CommandDetectionCapability {
    fn capability_type(&self) -> TerminalCapability {
        TerminalCapability::CommandDetection
    }

    fn prompt_input_model(&self) -> Rc<dyn IPromptInputModel> {
        self.prompt_input_model.clone()
    }

    fn commands(&self) -> Vec<Rc<dyn ITerminalCommand>> {
        self.state.borrow().commands.iter().map(|command| command.clone() as Rc<dyn ITerminalCommand>).collect()
    }

    fn executing_command(&self) -> Option<String> {
        self.current_command().borrow().command.clone()
    }

    fn executing_command_object(&self) -> Option<Rc<dyn ITerminalCommand>> {
        let current = self.current_command();
        if current.borrow().command_start_marker.is_some() {
            // HACK: This does a lot more than the consumer of the API needs. It's also a little
            //       misleading since it's not promoting the current command yet.
            let (cwd, ignore_command_line) = {
                let state = self.state.borrow();
                (state.cwd.clone(), state.handle_command_start_options.as_ref().and_then(|options| options.ignore_command_line).unwrap_or(false))
            };
            return current.promote_to_full_command(cwd.as_deref(), None, ignore_command_line, None).map(|command| command as Rc<dyn ITerminalCommand>);
        }
        None
    }

    fn executing_command_confidence(&self) -> Option<TerminalCommandConfidence> {
        // `isFullTerminalCommand(this._currentCommand)` é sempre falso: o `PartialTerminalCommand` não tem `hasOutput`.
        None
    }

    fn cwd(&self) -> Option<String> {
        self.state.borrow().cwd.clone()
    }

    fn has_rich_command_detection(&self) -> bool {
        self.state.borrow().has_rich_command_detection
    }

    fn current_command(&self) -> Option<Rc<dyn ICurrentPartialCommand>> {
        Some(self.state.borrow().current_command.clone() as Rc<dyn ICurrentPartialCommand>)
    }

    fn on_command_started(&self) -> Event<Rc<dyn ITerminalCommand>> {
        self.on_command_started.event()
    }

    fn on_command_finished(&self) -> Event<Rc<dyn ITerminalCommand>> {
        self.on_command_finished.event()
    }

    fn on_command_executed(&self) -> Event<Rc<dyn ITerminalCommand>> {
        self.on_command_executed.event()
    }

    fn on_command_invalidated(&self) -> Event<Vec<Rc<dyn ITerminalCommand>>> {
        self.on_command_invalidated.event()
    }

    fn on_current_command_invalidated(&self) -> Event<ICommandInvalidationRequest> {
        self.on_current_command_invalidated.event()
    }

    fn on_set_rich_command_detection(&self) -> Event<bool> {
        self.on_set_rich_command_detection.event()
    }

    fn set_continuation_prompt(&self, value: &str) {
        self.prompt_input_model.set_continuation_prompt(value);
    }

    // TODO: Simplify this, can everything work off the last line?
    fn set_prompt_terminator(&self, prompt_terminator: &str, last_prompt_line: &str) {
        self.debug("CommandDetectionCapability#setPromptTerminator", &[prompt_terminator.to_owned()]);
        self.state.borrow_mut().prompt_terminator = Some(prompt_terminator.to_owned());
        self.prompt_input_model.set_last_prompt_line(last_prompt_line);
    }

    fn set_cwd(&self, value: &str) {
        self.state.borrow_mut().cwd = Some(value.to_owned());
    }

    fn set_is_windows_pty(&self, value: bool) {
        if value && !self.is_windows_heuristics() {
            self.set_heuristics(PtyHeuristics::Windows(WindowsPtyHeuristics::new(self.terminal.clone(), self.this.clone(), self.log_service.clone())));
        } else if !value && self.is_windows_heuristics() {
            self.set_heuristics(PtyHeuristics::Unix(Rc::new(UnixPtyHeuristics {
                terminal: self.terminal.clone(),
                capability: self.this.clone(),
                log_service: self.log_service.clone(),
            })));
        }
    }

    fn set_has_rich_command_detection(&self, value: bool) {
        self.state.borrow_mut().has_rich_command_detection = value;
        self.on_set_rich_command_detection.fire(value);
    }

    fn set_is_command_storage_disabled(&self) {
        self.state.borrow_mut().is_command_storage_disabled = true;
    }

    fn get_cwd_for_line(&self, line: f64) -> Option<String> {
        // Handle the current partial command first, anything below it's prompt is considered part
        // of the current command
        let current = self.current_command();
        let prompt_start_marker = current.borrow().prompt_start_marker.clone();
        if prompt_start_marker.is_some_and(|marker| line >= marker.line()) {
            return self.state.borrow().cwd.clone();
        }

        match self.get_command_for_line(line) {
            Some(TerminalCommandForLine::Command(command)) if command.is_full_terminal_command() => command.cwd(),
            _ => None,
        }
    }

    fn get_command_for_line(&self, line: f64) -> Option<TerminalCommandForLine> {
        // Handle the current partial command first, anything below it's prompt is considered part
        // of the current command
        let current = self.current_command();
        let prompt_start_marker = current.borrow().prompt_start_marker.clone();
        if prompt_start_marker.is_some_and(|marker| line >= marker.line()) {
            return Some(TerminalCommandForLine::Partial(current as Rc<dyn ICurrentPartialCommand>));
        }

        let commands = self.command_list();
        // No commands
        let first = commands.first()?;

        // Line is before any registered commands
        if command_prompt_line(first) > line {
            return None;
        }

        // Iterate backwards through commands to find the right one
        commands.iter().rev().find(|command| command_prompt_line(command) <= line).map(|command| TerminalCommandForLine::Command(command.clone() as Rc<dyn ITerminalCommand>))
    }

    fn clear_commands(&self) {
        let removed: Vec<Rc<TerminalCommand>> = std::mem::take(&mut self.state.borrow_mut().commands);
        if !removed.is_empty() {
            self.on_command_invalidated.fire(removed.into_iter().map(|command| command as Rc<dyn ITerminalCommand>).collect());
        }
    }

    fn handle_prompt_start(&self, options: Option<&IHandleCommandOptions>) {
        self.state.borrow_mut().is_current_command_interrupted = false;
        // Adjust the last command's finished marker when needed. The standard position for the
        // finished marker `D` to appear is at the same position as the following prompt started
        // `A`. Only do this when it would not extend past the current cursor position.
        let last_command = self.state.borrow().commands.last().cloned();
        if let Some(last_command) = &last_command
            && let (Some(end_marker), Some(executed_marker)) = (ITerminalCommand::end_marker(last_command.as_ref()), ITerminalCommand::executed_marker(last_command.as_ref()))
            && end_marker.line() == executed_marker.line()
            && executed_marker.line() < f64::from(self.absolute_cursor_y())
        {
            self.debug(
                "CommandDetectionCapability#handlePromptStart adjusted commandFinished",
                &[format!("{} -> {}", end_marker.line(), executed_marker.line() + 1.0)],
            );
            last_command.set_end_marker(clone_marker(self.terminal.as_ref(), executed_marker.as_ref(), 1));
        }

        let current = self.current_command();
        let prompt_start_marker = options.and_then(|options| options.marker.clone()).or_else(|| {
            // Generally the prompt start should happen at the exact place the endmarker happened.
            // However, after ctrl+l is used to clear the display, we want to ensure the actual
            // prompt start marker position is used. This is mostly a workaround for Windows but we
            // apply it generally.
            let was_cleared = current.borrow().was_cleared.unwrap_or(false);
            let last_end_marker = last_command.as_ref().and_then(|last_command| ITerminalCommand::end_marker(last_command.as_ref()));
            match last_end_marker {
                Some(end_marker) if !was_cleared => clone_marker(self.terminal.as_ref(), end_marker.as_ref(), 0),
                _ => self.terminal.register_marker(0),
            }
        });
        let mut current = current.borrow_mut();
        current.prompt_start_marker = prompt_start_marker;
        current.was_cleared = Some(false);
    }

    fn handle_continuation_start(&self) {
        let marker = self.terminal.register_marker(0);
        let line = marker.as_ref().map(|marker| marker.line());
        self.current_command().borrow_mut().current_continuation_marker = marker;
        self.debug("CommandDetectionCapability#handleContinuationStart", &[format!("{line:?}")]);
    }

    fn handle_continuation_end(&self) {
        let current = self.current_command();
        let Some(marker) = current.borrow().current_continuation_marker.clone() else {
            self.log_service.warn("CommandDetectionCapability#handleContinuationEnd Received continuation end without start");
            return;
        };
        let end = f64::from(self.terminal.buffer_active().cursor_x());
        let line = marker.line();
        {
            let mut state = current.borrow_mut();
            state.continuations.get_or_insert_with(Vec::new).push(super::command_detection::terminal_command::Continuation { marker, end });
            state.current_continuation_marker = None;
        }
        self.debug("CommandDetectionCapability#handleContinuationEnd", &[format!("{{ marker: {line}, end: {end} }}")]);
    }

    fn handle_right_prompt_start(&self) {
        let x = f64::from(self.terminal.buffer_active().cursor_x());
        self.current_command().borrow_mut().command_right_prompt_start_x = Some(x);
        self.debug("CommandDetectionCapability#handleRightPromptStart", &[x.to_string()]);
    }

    fn handle_right_prompt_end(&self) {
        let x = f64::from(self.terminal.buffer_active().cursor_x());
        self.current_command().borrow_mut().command_right_prompt_end_x = Some(x);
        self.debug("CommandDetectionCapability#handleRightPromptEnd", &[x.to_string()]);
    }

    fn handle_command_start(&self, options: Option<&IHandleCommandOptions>) {
        let current = self.current_command();
        {
            let mut state = self.state.borrow_mut();
            state.handle_command_start_options = options.cloned();
            current.borrow_mut().cwd = state.cwd.clone();
        }
        // Only update the column if the line has already been set
        let start_marker = {
            let mut current_state = current.borrow_mut();
            if let Some(marker) = options.and_then(|options| options.marker.clone()) {
                current_state.command_start_marker = Some(marker);
            }
            current_state.command_start_marker.clone()
        };
        let buffer = self.terminal.buffer_active();
        if let Some(start_marker) = &start_marker
            && start_marker.line() == f64::from(buffer.cursor_y())
        {
            let x = f64::from(buffer.cursor_x());
            current.borrow_mut().command_start_x = Some(x);
            self.on_command_start_changed.fire(());
            self.debug("CommandDetectionCapability#handleCommandStart", &[x.to_string(), start_marker.line().to_string()]);
            return;
        }
        match self.heuristics() {
            PtyHeuristics::Unix(heuristics) => heuristics.handle_command_start(options),
            PtyHeuristics::Windows(heuristics) => heuristics.handle_command_start(),
        }
    }

    /// Sets the command ID to use for the next command that starts. This is useful when you want to pre-assign an ID
    /// before the shell sends the command start sequence.
    fn set_next_command_id(&self, command: &str, command_id: &str) {
        self.state.borrow_mut().next_command_id = Some((command.to_owned(), Some(command_id.to_owned())));
    }

    fn handle_command_executed(&self, options: Option<&IHandleCommandOptions>) {
        self.ensure_current_command_id();
        match self.heuristics() {
            PtyHeuristics::Unix(heuristics) => heuristics.handle_command_executed(options),
            PtyHeuristics::Windows(heuristics) => heuristics.handle_command_executed(),
        }
        self.current_command().mark_executed_time();
    }

    fn handle_command_finished(&self, exit_code: Option<f64>, options: Option<&IHandleCommandOptions>) {
        let mut exit_code = exit_code;
        // Command executed may not have happened yet, if not handle it now so the expected events
        // properly propagate. This may cause the output to show up in the computed command line,
        // but the command line confidence will be low in the extension host for example and
        // therefore cannot be trusted anyway.
        let current = self.current_command();
        if current.borrow().command_executed_marker.is_none() {
            self.handle_command_executed(None);
        }
        current.mark_finished_time();
        if let PtyHeuristics::Windows(heuristics) = self.heuristics() {
            heuristics.pre_handle_command_finished();
        }

        self.debug(
            "CommandDetectionCapability#handleCommandFinished",
            &[
                self.terminal.buffer_active().cursor_x().to_string(),
                format!("{:?}", options.and_then(|options| options.marker.as_ref()).map(|marker| marker.line())),
                format!("{:?}", current.borrow().command),
            ],
        );

        // HACK: Handle a special case on some versions of bash where identical commands get merged
        // in the output of `history`, this detects that case and sets the exit code to the last
        // command's exit code. This covered the majority of cases but will fail if the same command
        // runs with a different exit code, that will need a more robust fix where we send the
        // command ID and exit code over to the capability to adjust there.
        // A canceled command's exit code should remain undefined.
        if exit_code.is_none() && !self.state.borrow().is_current_command_interrupted {
            let last_command = self.state.borrow().commands.last().cloned();
            let current_command_line = current.borrow().command.clone();
            if let Some(command_line) = current_command_line.filter(|command_line| !command_line.is_empty())
                && let Some(last_command) = last_command
                && IBaseTerminalCommandAlias::command(last_command.as_ref()) == command_line
            {
                exit_code = IBaseTerminalCommandAlias::exit_code(last_command.as_ref());
            }
        }

        if current.borrow().command_start_marker.is_none() {
            return;
        }

        let finished_marker = options.and_then(|options| options.marker.clone()).or_else(|| self.terminal.register_marker(0));
        current.borrow_mut().command_finished_marker = finished_marker;

        if let PtyHeuristics::Windows(heuristics) = self.heuristics() {
            heuristics.post_handle_command_finished();
        }

        let (cwd, ignore_command_line) = {
            let state = self.state.borrow();
            (state.cwd.clone(), state.handle_command_start_options.as_ref().and_then(|options| options.ignore_command_line).unwrap_or(false))
        };
        let new_command = current.promote_to_full_command(cwd.as_deref(), exit_code, ignore_command_line, options.and_then(|options| options.mark_properties.clone()));

        if let Some(new_command) = new_command {
            self.state.borrow_mut().commands.push(new_command.clone());
            self.on_before_command_finished.fire(new_command.clone() as Rc<dyn ITerminalCommand>);
            // NOTE: onCommandFinished used to not fire if the command was invalid, but this causes
            // problems especially with the associated execution event never firing in the extension
            // API. See https://github.com/microsoft/vscode/issues/252489
            self.debug("CommandDetectionCapability#onCommandFinished", &[IBaseTerminalCommandAlias::command(new_command.as_ref())]);
            self.on_command_finished.fire(new_command as Rc<dyn ITerminalCommand>);
        }
        // Create new command for next execution
        let mut state = self.state.borrow_mut();
        state.current_command = PartialTerminalCommand::new(self.terminal.clone(), None);
        state.handle_command_start_options = None;
    }

    fn set_command_line(&self, command_line: &str, is_trusted: bool) {
        self.debug("CommandDetectionCapability#setCommandLine", &[command_line.to_owned(), is_trusted.to_string()]);
        {
            let current = self.current_command();
            let mut current = current.borrow_mut();
            current.command = Some(command_line.to_owned());
            current.command_line_confidence = Some(TerminalCommandConfidence::High);
            current.is_trusted = Some(is_trusted);
        }

        if is_trusted {
            self.prompt_input_model.set_confident_command_line(command_line);
        }
    }

    fn serialize(&self) -> ISerializedCommandDetectionCapability {
        let (is_storage_disabled, cwd, commands) = {
            let state = self.state.borrow();
            (state.is_command_storage_disabled, state.cwd.clone(), state.commands.clone())
        };
        let mut serialized: Vec<ISerializedTerminalCommand> = commands.iter().map(|command| command.serialize(is_storage_disabled)).collect();
        if let Some(partial_command) = self.current_command().serialize(cwd.as_deref()) {
            serialized.push(partial_command);
        }
        ISerializedCommandDetectionCapability {
            is_windows_pty: self.is_windows_heuristics(),
            has_rich_command_detection: self.state.borrow().has_rich_command_detection,
            commands: serialized,
            prompt_input_model: Some(self.prompt_input_model.serialize()),
        }
    }

    fn deserialize(&self, serialized: &ISerializedCommandDetectionCapability) {
        if serialized.is_windows_pty {
            self.set_is_windows_pty(serialized.is_windows_pty);
        }
        if serialized.has_rich_command_detection {
            self.set_has_rich_command_detection(serialized.has_rich_command_detection);
        }
        let buffer = self.terminal.buffer_normal();
        let register = |line: Option<f64>| -> Option<Rc<dyn IMarker>> {
            line.and_then(|line| self.terminal.register_marker(line as i32 - (buffer.base_y() + buffer.cursor_y())))
        };
        for e in &serialized.commands {
            // Partial command
            if e.end_line.is_none_or(|end_line| end_line == 0.0) {
                // Check for invalid command
                let Some(marker) = register(e.start_line) else {
                    continue;
                };
                let current = self.current_command();
                {
                    let mut current = current.borrow_mut();
                    current.command_start_marker = register(e.start_line);
                    current.command_start_x = e.start_x;
                    current.prompt_start_marker = register(e.prompt_start_line);
                }
                self.state.borrow_mut().cwd = e.cwd.clone();
                self.on_command_started.fire(Rc::new(StartedCommand { marker: Some(marker), mark_properties: None }));
                continue;
            }

            // Full command
            let is_storage_disabled = self.state.borrow().is_command_storage_disabled;
            let Some(new_command) = TerminalCommand::deserialize(self.terminal.clone(), e, is_storage_disabled) else {
                continue;
            };

            self.state.borrow_mut().commands.push(new_command.clone());
            self.debug("CommandDetectionCapability#onCommandFinished", &[IBaseTerminalCommandAlias::command(new_command.as_ref())]);
            self.on_command_finished.fire(new_command as Rc<dyn ITerminalCommand>);
        }
        if let Some(prompt_input_model) = &serialized.prompt_input_model {
            self.prompt_input_model.deserialize(prompt_input_model);
        }
    }
}

/// O `IBaseTerminalCommand` sob um nome que não colide com o `ICurrentPartialCommand` nos usos acima.
use super::capabilities::IBaseTerminalCommand as IBaseTerminalCommandAlias;

/// Non-Windows-specific behavior (`UnixPtyHeuristics`).
struct UnixPtyHeuristics {
    terminal: Rc<dyn IHeadlessTerminal>,
    capability: Weak<CommandDetectionCapability>,
    log_service: LogServiceRef,
}

impl UnixPtyHeuristics {
    fn dispose(&self) {}

    fn handle_command_start(&self, options: Option<&IHandleCommandOptions>) {
        let Some(capability) = self.capability.upgrade() else {
            return;
        };
        let current_command = capability.current_command();
        let start_marker = options.and_then(|options| options.marker.clone()).or_else(|| self.terminal.register_marker(0));
        let start_x = f64::from(self.terminal.buffer_active().cursor_x());
        let executed_marker = {
            let mut state = current_command.borrow_mut();
            state.command_start_x = Some(start_x);
            state.command_start_marker = start_marker;

            // Clear executed as it must happen after command start
            let executed_marker = state.command_executed_marker.take();
            state.command_executed_x = None;
            executed_marker
        };
        if let Some(executed_marker) = executed_marker {
            executed_marker.dispose();
        }
        let command_markers = capability.command_markers();
        for marker in &command_markers {
            marker.dispose();
        }
        capability.set_command_markers(Vec::new());

        let command_start_marker = current_command.borrow().command_start_marker.clone();
        capability.fire_command_started(Rc::new(StartedCommand {
            marker: options.and_then(|options| options.marker.clone()).or_else(|| command_start_marker.clone()),
            mark_properties: options.and_then(|options| options.mark_properties.clone()),
        }));
        self.log_service.debug_args(
            "CommandDetectionCapability#handleCommandStart",
            &text_args(&[start_x.to_string(), command_start_marker.map_or("undefined".to_owned(), |marker| marker.line().to_string())]),
        );
    }

    fn handle_command_executed(&self, options: Option<&IHandleCommandOptions>) {
        let Some(capability) = self.capability.upgrade() else {
            return;
        };
        let current_command = capability.current_command();
        let executed_marker = options.and_then(|options| options.marker.clone()).or_else(|| self.terminal.register_marker(0));
        let executed_x = f64::from(self.terminal.buffer_active().cursor_x());
        {
            let mut state = current_command.borrow_mut();
            state.command_executed_marker = executed_marker.clone();
            state.command_executed_x = Some(executed_x);
        }
        self.log_service.debug_args(
            "CommandDetectionCapability#handleCommandExecuted",
            &text_args(&[executed_x.to_string(), executed_marker.map_or("undefined".to_owned(), |marker| marker.line().to_string())]),
        );

        // Sanity check optional props
        {
            let state = current_command.borrow();
            if state.command_start_marker.is_none() || state.command_executed_marker.is_none() || state.command_start_x.is_none() {
                return;
            }
        }

        let prompt_input_model = capability.prompt_input_model.clone();
        let ghost_text_index = prompt_input_model.ghost_text_index();
        let value = prompt_input_model.value();
        let command = if ghost_text_index > -1 { js_string::substring(value.units(), 0, i64::from(ghost_text_index)).to_vec() } else { value.units().to_vec() };
        current_command.borrow_mut().command = Some(JsString::from_units(command).to_string_lossy());
        capability.fire_command_executed(current_command as Rc<dyn ITerminalCommand>);
    }
}

/// `AdjustCommandStartMarkerConstants`.
const MAX_CHECK_LINE_COUNT: i32 = 10;
const INTERVAL: u64 = 20;
const MAXIMUM_POLL_COUNT: i32 = 10;

/// As expressões regulares de `_getWindowsPrompt`.
struct WindowsPromptRegexes {
    /// `/(?<prompt>(\(.+\)\s)?(?:PS.+>\s?))/`
    powershell: RegExp,
    /// `/.*❯(?=[^❯]*$)/g`
    custom: RegExp,
    /// `/^(?<prompt>\$)/`
    bash: RegExp,
    /// `/^(?<prompt>(\(.+\)\s)?(?:[A-Z]:\\.*>))/`
    command_prompt: RegExp,
}

impl WindowsPromptRegexes {
    fn new() -> Self {
        let compile = |pattern: &str| RegExp::from_str(pattern, "").expect("a expressão regular do prompt do Windows é válida");
        Self {
            powershell: compile(r"(?<prompt>(\(.+\)\s)?(?:PS.+>\s?))"),
            custom: compile(".*\u{276f}(?=[^\u{276f}]*$)"),
            bash: compile(r"^(?<prompt>\$)"),
            command_prompt: compile(r"^(?<prompt>(\(.+\)\s)?(?:[A-Z]:\\.*>))"),
        }
    }
}

/// `string | { prompt: string; likelySingleLine: true }`: o prompt e se ele provavelmente ocupa uma linha só.
struct WindowsPrompt {
    prompt: JsString,
    likely_single_line: bool,
}

/// An object that integrated with and decorates the command detection capability to add heuristics
/// that adjust various markers to work better with Windows and ConPTY. This isn't depended upon the
/// frontend OS, or even the backend OS, but the `IsWindows` property which technically a non-Windows
/// client can emit (for example in tests).
struct WindowsPtyHeuristics {
    base: Rc<Disposable>,
    this: Weak<Self>,
    terminal: Rc<dyn IHeadlessTerminal>,
    capability: Weak<CommandDetectionCapability>,
    log_service: LogServiceRef,
    regexes: WindowsPromptRegexes,
    is_disposed: Cell<bool>,
    on_cursor_move_listener: RefCell<Option<DisposableRef>>,
    try_adjust_command_start_marker_scheduler: RefCell<Option<Rc<RunOnceScheduler>>>,
    try_adjust_command_start_marker_scanned_line_count: Cell<i32>,
    try_adjust_command_start_marker_poll_count: Cell<i32>,
}

impl WindowsPtyHeuristics {
    fn new(terminal: Rc<dyn IHeadlessTerminal>, capability: Weak<CommandDetectionCapability>, log_service: LogServiceRef) -> Rc<Self> {
        let base = Disposable::new();
        let heuristics = Rc::new_cyclic(|this| Self {
            base,
            this: this.clone(),
            terminal,
            capability: capability.clone(),
            log_service,
            regexes: WindowsPromptRegexes::new(),
            is_disposed: Cell::new(false),
            on_cursor_move_listener: RefCell::new(None),
            try_adjust_command_start_marker_scheduler: RefCell::new(None),
            try_adjust_command_start_marker_scanned_line_count: Cell::new(0),
            try_adjust_command_start_marker_poll_count: Cell::new(0),
        });

        if let Some(capability) = capability.upgrade() {
            let this = heuristics.this.clone();
            heuristics.base.register(capability.on_before_command_finished().listen(move |command| {
                let Some(this) = this.upgrade() else {
                    return;
                };
                // For older Windows backends we cannot listen to CSI J, instead we assume running clear
                // or cls will clear all commands in the viewport. This is not perfect but it's right
                // most of the time.
                let command_text = JsString::from(command.command().as_str());
                let trimmed = JsString::from(js_string::trim(command_text.units())).to_lower_case();
                if trimmed == "clear" || trimmed == "cls" {
                    let scheduler = this.try_adjust_command_start_marker_scheduler.borrow_mut().take();
                    if let Some(scheduler) = scheduler {
                        scheduler.cancel();
                    }
                    if let Some(capability) = this.capability.upgrade() {
                        capability.clear_commands_in_viewport();
                        capability.current_command().borrow_mut().is_invalid = Some(true);
                        capability.fire_current_command_invalidated(ICommandInvalidationRequest { reason: CommandInvalidationReason::Windows });
                    }
                }
            }));
        }
        heuristics
    }

    fn dispose(&self) {
        self.is_disposed.set(true);
        let listener = self.on_cursor_move_listener.borrow_mut().take();
        if let Some(listener) = listener {
            listener.dispose();
        }
        let scheduler = self.try_adjust_command_start_marker_scheduler.borrow_mut().take();
        if let Some(scheduler) = scheduler {
            scheduler.dispose();
        }
        self.base.dispose();
    }

    fn pre_handle_resize(&self, e: TerminalResize) {
        let Some(capability) = self.capability.upgrade() else {
            return;
        };
        // Resize behavior is different under conpty; instead of bringing parts of the scrollback
        // back into the viewport, new lines are inserted at the bottom (ie. the same behavior as if
        // there was no scrollback).
        //
        // On resize this workaround will wait for a conpty reprint to occur by waiting for the
        // cursor to move, it will then calculate the number of lines that the commands within the
        // viewport _may have_ shifted. After verifying the content of the current line is
        // incorrect, the line after shifting is checked and if that matches delete events are fired
        // on the xterm.js buffer to move the markers.
        //
        // While a bit hacky, this approach is quite safe and seems to work great at least for pwsh.
        let base_y = self.terminal.buffer_active().base_y();
        let rows_difference = e.rows - capability.dimensions().rows;
        // Only do when rows increase, do in the next frame as this needs to happen after
        // conpty reprints the screen
        if rows_difference > 0 {
            let this = self.this.clone();
            self.wait_for_cursor_move().then(move |()| {
                if let Some(this) = this.upgrade() {
                    this.shift_commands_after_resize(base_y, rows_difference);
                }
            });
        }
    }

    /// O corpo do `then` de `preHandleResize`.
    fn shift_commands_after_resize(&self, base_y: i32, rows_difference: i32) {
        let Some(capability) = self.capability.upgrade() else {
            return;
        };
        // Calculate the number of lines the content may have shifted, this will max out at
        // scrollback count since the standard behavior will be used then
        let potential_shifted_line_count = rows_difference.min(base_y);
        let commands = capability.command_list();
        let buffer = self.terminal.buffer_active();
        // For each command within the viewport, assume commands are in the correct order
        for command in commands.iter().rev() {
            let marker = ITerminalCommand::marker(command.as_ref());
            let content = IBaseTerminalCommandAlias::command_start_line_content(command.as_ref());
            let (Some(marker), Some(content)) = (marker, content) else {
                break;
            };
            if marker.line() < f64::from(base_y) {
                break;
            }
            let content = JsString::from(content.as_str());
            let marker_line = marker.line() as i32;
            let Some(line) = buffer.get_line(marker_line) else {
                continue;
            };
            if line.translate_to_string(Some(true), None, None) == content {
                continue;
            }
            let shifted_y = marker_line - potential_shifted_line_count;
            let shifted_line = buffer.get_line(shifted_y);
            if shifted_line.is_none_or(|shifted_line| shifted_line.translate_to_string(Some(true), None, None) != content) {
                continue;
            }
            // HACK: xterm.js doesn't expose this by design as it's an internal core
            // function an embedder could easily do damage with. Additionally, this
            // can't really be upstreamed since the event relies on shell integration to
            // verify the shifting is necessary.
            self.terminal.fire_lines_deleted(buffer.base_y(), potential_shifted_line_count);
        }
    }

    fn handle_command_start(&self) {
        let Some(capability) = self.capability.upgrade() else {
            return;
        };
        let current_command = capability.current_command();
        current_command.borrow_mut().command_start_x = Some(f64::from(self.terminal.buffer_active().cursor_x()));

        // On Windows track all cursor movements after the command start sequence
        capability.set_command_markers(Vec::new());

        let prompt_start_marker = current_command.borrow().prompt_start_marker.clone();
        let initial_command_start_marker = match prompt_start_marker {
            Some(prompt_start_marker) => clone_marker(self.terminal.as_ref(), prompt_start_marker.as_ref(), 0),
            None => self.terminal.register_marker(0),
        };
        {
            let mut state = current_command.borrow_mut();
            state.command_start_marker = initial_command_start_marker.clone();
            state.command_start_x = Some(0.0);
        }

        // The command started sequence may be printed before the actual prompt is, for example a
        // multi-line prompt will typically look like this where D, A and B signify the command
        // finished, prompt started and command started sequences respectively:
        //
        //     D/my/cwdB
        //     > C
        //
        // Due to this, it's likely that this will be called before the line has been parsed.
        // Unfortunately, it is also the case that the actual command start data may not be parsed
        // by the end of the task either, so a microtask cannot be used.
        //
        // The strategy used is to begin polling and scanning downwards for up to the next 5 lines.
        // If it looks like a prompt is found, the command started location is adjusted. If the
        // command executed sequences comes in before polling is done, polling is canceled and the
        // final polling task is executed synchronously.
        self.try_adjust_command_start_marker_scanned_line_count.set(0);
        self.try_adjust_command_start_marker_poll_count.set(0);
        let this = self.this.clone();
        let scheduler = RunOnceScheduler::new(
            move || {
                if let Some(this) = this.upgrade() {
                    this.try_adjust_command_start_marker(initial_command_start_marker.as_ref());
                }
            },
            INTERVAL,
        );
        scheduler.schedule(None);
        *self.try_adjust_command_start_marker_scheduler.borrow_mut() = Some(scheduler);

        // TODO: Cache details about polling for the future - eg. if it always fails, stop bothering
    }

    fn try_adjust_command_start_marker(&self, start: Option<&Rc<dyn IMarker>>) {
        if self.is_disposed.get() {
            return;
        }
        let Some(capability) = self.capability.upgrade() else {
            return;
        };
        let Some(start) = start else {
            // O original lê `start.line` e lança `TypeError` sem o marcador inicial (`registerMarker` que devolveu `undefined`).
            return;
        };
        let current_command = capability.current_command();
        let buffer = self.terminal.buffer_active();
        let mut scanned_line_count = self.try_adjust_command_start_marker_scanned_line_count.get();
        while scanned_line_count < MAX_CHECK_LINE_COUNT && start.line() + f64::from(scanned_line_count) < f64::from(buffer.base_y() + self.terminal.rows()) {
            if self.cursor_on_next_line(&capability) {
                let prompt = self.get_windows_prompt(Some(start.line() as i32 + scanned_line_count));
                if let Some(prompt) = prompt {
                    let adjusted_prompt = prompt.prompt.clone();
                    let command_start_marker = self.terminal.register_marker(0);
                    current_command.borrow_mut().command_start_marker = command_start_marker.clone();
                    if prompt.likely_single_line {
                        if let Some(command_start_marker) = &command_start_marker {
                            let prompt_start_line = current_command.borrow().prompt_start_marker.as_ref().map(|marker| marker.line());
                            self.log_service.debug_args(
                                "CommandDetectionCapability#_tryAdjustCommandStartMarker adjusted promptStart",
                                &text_args(&[format!("{:?} -> {}", prompt_start_line, command_start_marker.line())]),
                            );
                            let previous_prompt_start_marker = current_command.borrow_mut().prompt_start_marker.take();
                            if let Some(previous_prompt_start_marker) = previous_prompt_start_marker {
                                previous_prompt_start_marker.dispose();
                            }
                            current_command.borrow_mut().prompt_start_marker = clone_marker(self.terminal.as_ref(), command_start_marker.as_ref(), 0);
                            // Adjust the last command if it's not in the same position as the following
                            // prompt start marker
                            let last_command = capability.command_list().last().cloned();
                            if let Some(last_command) = last_command {
                                let last_end_line = ITerminalCommand::end_marker(last_command.as_ref()).map(|marker| marker.line());
                                if Some(command_start_marker.line()) != last_end_line {
                                    if let Some(end_marker) = ITerminalCommand::end_marker(last_command.as_ref()) {
                                        end_marker.dispose();
                                    }
                                    last_command.set_end_marker(clone_marker(self.terminal.as_ref(), command_start_marker.as_ref(), 0));
                                }
                            }
                        }
                    }
                    // use the regex to set the position as it's possible input has occurred
                    current_command.borrow_mut().command_start_x = Some(adjusted_prompt.len() as f64);
                    self.log_service.debug_args(
                        "CommandDetectionCapability#_tryAdjustCommandStartMarker adjusted commandStart",
                        &text_args(&[format!(
                            "{} -> {}:{}",
                            start.line(),
                            command_start_marker.as_ref().map_or(f64::NAN, |marker| marker.line()),
                            adjusted_prompt.len()
                        )]),
                    );
                    self.flush_pending_handle_command_start_task(&capability);
                    return;
                }
            }
            scanned_line_count += 1;
        }
        if scanned_line_count < MAX_CHECK_LINE_COUNT {
            self.try_adjust_command_start_marker_scanned_line_count.set(scanned_line_count);
            let poll_count = self.try_adjust_command_start_marker_poll_count.get() + 1;
            self.try_adjust_command_start_marker_poll_count.set(poll_count);
            if poll_count < MAXIMUM_POLL_COUNT {
                let scheduler = self.try_adjust_command_start_marker_scheduler.borrow().clone();
                if let Some(scheduler) = scheduler {
                    scheduler.schedule(None);
                }
            } else {
                self.flush_pending_handle_command_start_task(&capability);
            }
        } else {
            self.flush_pending_handle_command_start_task(&capability);
        }
    }

    fn flush_pending_handle_command_start_task(&self, capability: &Rc<CommandDetectionCapability>) {
        // Perform final try adjust if necessary
        let scheduler = self.try_adjust_command_start_marker_scheduler.borrow().clone();
        if let Some(scheduler) = scheduler {
            // Max out poll count to ensure it's the last run
            self.try_adjust_command_start_marker_poll_count.set(MAXIMUM_POLL_COUNT);
            scheduler.flush();
            *self.try_adjust_command_start_marker_scheduler.borrow_mut() = None;
        }

        let current_command = capability.current_command();
        if current_command.borrow().command_executed_marker.is_none() {
            let this = self.this.clone();
            let listener = self.terminal.on_cursor_move().listen(move |_| {
                let Some(this) = this.upgrade() else {
                    return;
                };
                let Some(capability) = this.capability.upgrade() else {
                    return;
                };
                let command_markers = capability.command_markers();
                if command_markers.last().is_none_or(|last| last.line() != f64::from(this.terminal.buffer_active().cursor_y())) {
                    let marker = this.terminal.register_marker(0);
                    if let Some(marker) = marker {
                        let mut command_markers = command_markers;
                        command_markers.push(marker);
                        capability.set_command_markers(command_markers);
                    }
                }
            });
            let previous = self.on_cursor_move_listener.borrow_mut().replace(listener);
            if let Some(previous) = previous {
                previous.dispose();
            }
        }

        let command_start_marker = current_command.borrow().command_start_marker.clone();
        if let Some(command_start_marker) = &command_start_marker
            && let Some(line) = self.terminal.buffer_active().get_line(command_start_marker.line() as i32)
        {
            current_command.borrow_mut().command_start_line_content = Some(line.translate_to_string(Some(true), None, None).to_string_lossy());
        }
        capability.fire_command_started(Rc::new(StartedCommand { marker: command_start_marker.clone(), mark_properties: None }));
        let command_start_x = current_command.borrow().command_start_x;
        self.log_service.debug_args(
            "CommandDetectionCapability#_handleCommandStartWindows",
            &text_args(&[format!("{command_start_x:?}"), format!("{:?}", command_start_marker.map(|marker| marker.line()))]),
        );
    }

    fn handle_command_executed(&self) {
        let Some(capability) = self.capability.upgrade() else {
            return;
        };
        let has_scheduler = self.try_adjust_command_start_marker_scheduler.borrow().is_some();
        if has_scheduler {
            self.flush_pending_handle_command_start_task(&capability);
        }
        // Use the gathered cursor move markers to correct the command start and executed markers
        let listener = self.on_cursor_move_listener.borrow_mut().take();
        if let Some(listener) = listener {
            listener.dispose();
        }
        self.evaluate_command_markers(&capability);
        let current_command = capability.current_command();
        let executed_x = f64::from(self.terminal.buffer_active().cursor_x());
        current_command.borrow_mut().command_executed_x = Some(executed_x);
        capability.fire_command_executed(current_command.clone() as Rc<dyn ITerminalCommand>);
        let executed_marker_line = current_command.borrow().command_executed_marker.as_ref().map(|marker| marker.line());
        self.log_service.debug_args(
            "CommandDetectionCapability#handleCommandExecuted",
            &text_args(&[executed_x.to_string(), format!("{executed_marker_line:?}")]),
        );
    }

    fn pre_handle_command_finished(&self) {
        let Some(capability) = self.capability.upgrade() else {
            return;
        };
        let current_command = capability.current_command();
        if current_command.borrow().command_executed_marker.is_some() {
            return;
        }
        // This is done on command finished just in case command executed never happens (for example
        // PSReadLine tab completion)
        if capability.command_markers().is_empty() {
            // If the command start timeout doesn't happen before command finished, just use the
            // current marker.
            if current_command.borrow().command_start_marker.is_none() {
                current_command.borrow_mut().command_start_marker = self.terminal.register_marker(0);
            }
            let command_start_marker = current_command.borrow().command_start_marker.clone();
            if let Some(command_start_marker) = command_start_marker {
                let mut command_markers = capability.command_markers();
                command_markers.push(command_start_marker);
                capability.set_command_markers(command_markers);
            }
        }
        self.evaluate_command_markers(&capability);
    }

    fn post_handle_command_finished(&self) {
        let Some(capability) = self.capability.upgrade() else {
            return;
        };
        let current_command = capability.current_command();
        let (command_text, command_line, executed_line) = {
            let state = current_command.borrow();
            (
                state.command.clone(),
                state.command_start_marker.as_ref().map(|marker| marker.line()),
                state.command_executed_marker.as_ref().map(|marker| marker.line()),
            )
        };
        let (Some(command_text), Some(command_line), Some(executed_line)) = (command_text, command_line, executed_line) else {
            return;
        };
        if command_text.is_empty() || command_line == -1.0 || executed_line == -1.0 {
            return;
        }
        let command_text = JsString::from(command_text.as_str());
        let command_units = command_text.units();

        // Scan downwards from the command start line and search for every character in the actual
        // command line. This may end up matching the wrong characters, but it shouldn't matter at
        // least in the typical case as the entire command will still get matched.
        let mut current = 0usize;
        let mut found = false;
        let buffer = self.terminal.buffer_active();
        let cols = self.terminal.cols();
        let mut i = command_line as i32;
        while f64::from(i) <= executed_line {
            let Some(line) = buffer.get_line(i) else {
                break;
            };
            let text = line.translate_to_string(Some(true), None, None);
            let text_units = text.units();
            for (j, unit) in text_units.iter().enumerate() {
                // Skip whitespace in case it was not actually rendered or could be trimmed from the
                // end of the line
                while command_units.len() < current && command_units.get(current) == Some(&u16::from(b' ')) {
                    current += 1;
                }

                // Character match
                if command_units.get(current) == Some(unit) {
                    current += 1;
                }

                // Full command match
                if current == command_units.len() {
                    // It's ambiguous whether the command executed marker should ideally appear at
                    // the end of the line or at the beginning of the next line. Since it's more
                    // useful for extracting the command at the end of the current line we go with
                    // that.
                    let wraps_to_next_line = j as i32 >= cols - 1;
                    let marker = self.terminal.register_marker(i - (buffer.base_y() + buffer.cursor_y()) + i32::from(wraps_to_next_line));
                    {
                        let mut state = current_command.borrow_mut();
                        state.command_executed_marker = marker;
                        state.command_executed_x = Some(if wraps_to_next_line { 0.0 } else { (j + 1) as f64 });
                    }
                    found = true;
                    break;
                }
            }
            if found {
                break;
            }
            i += 1;
        }
    }

    fn evaluate_command_markers(&self, capability: &Rc<CommandDetectionCapability>) {
        // On Windows, use the gathered cursor move markers to correct the command start and
        // executed markers.
        let mut command_markers = capability.command_markers();
        if command_markers.is_empty() {
            return;
        }
        command_markers.sort_by(|a, b| a.line().total_cmp(&b.line()));
        capability.set_command_markers(command_markers.clone());
        let current_command = capability.current_command();
        let command_start_marker = command_markers[0].clone();
        current_command.borrow_mut().command_start_marker = Some(command_start_marker.clone());
        if let Some(line) = self.terminal.buffer_active().get_line(command_start_marker.line() as i32) {
            current_command.borrow_mut().command_start_line_content = Some(line.translate_to_string(Some(true), None, None).to_string_lossy());
        }
        current_command.borrow_mut().command_executed_marker = command_markers.last().cloned();
        // Fire this now to prevent issues like #197409
        capability.fire_command_executed(current_command as Rc<dyn ITerminalCommand>);
    }

    fn cursor_on_next_line(&self, capability: &Rc<CommandDetectionCapability>) -> bool {
        let last_command = capability.command_list().last().cloned();

        // There is only a single command, so this check is unnecessary
        let Some(last_command) = last_command else {
            return true;
        };

        let cursor_y_absolute = f64::from(self.terminal.buffer_active().base_y() + self.terminal.buffer_active().cursor_y());
        // If the cursor position is within the last command, we should poll.
        let last_command_y_absolute = match ITerminalCommand::end_marker(last_command.as_ref()) {
            Some(end_marker) => end_marker.line(),
            None => ITerminalCommand::marker(last_command.as_ref()).map_or(-1.0, |marker| marker.line()),
        };
        cursor_y_absolute > last_command_y_absolute
    }

    fn wait_for_cursor_move(&self) -> Promise<()> {
        let buffer = self.terminal.buffer_active();
        let cursor_x = buffer.cursor_x();
        let cursor_y = buffer.cursor_y();
        let total_delay = Rc::new(Cell::new(0));
        let (promise, resolver) = Promise::new();
        let handle: Rc<RefCell<Option<IntervalHandle>>> = Rc::default();
        let terminal = self.terminal.clone();
        let interval = {
            let handle = handle.clone();
            set_interval(
                move || {
                    let clear = || {
                        if let Some(handle) = handle.borrow().as_ref() {
                            clear_interval(handle);
                        }
                    };
                    let buffer = terminal.buffer_active();
                    if cursor_x != buffer.cursor_x() || cursor_y != buffer.cursor_y() {
                        resolver.resolve(());
                        clear();
                        return;
                    }
                    total_delay.set(total_delay.get() + 10);
                    if total_delay.get() > 1000 {
                        clear();
                        resolver.resolve(());
                    }
                },
                10,
            )
        };
        *handle.borrow_mut() = Some(interval);
        promise
    }

    fn get_windows_prompt(&self, y: Option<i32>) -> Option<WindowsPrompt> {
        let buffer = self.terminal.buffer_active();
        let y = y.unwrap_or_else(|| buffer.base_y() + buffer.cursor_y());
        let line = buffer.get_line(y)?;
        let line_text = line.translate_to_string(Some(true), None, None);
        if line_text.is_empty() {
            return None;
        }
        let units = line_text.units();
        let group = |regex: &RegExp, name: &str| -> Option<JsString> {
            let found = regex.exec(units, 0)?;
            found.group(name).map(|range| JsString::from(&units[range]))
        };

        // PowerShell
        if let Some(pwsh_prompt) = group(&self.regexes.powershell, "prompt").filter(|prompt| !prompt.is_empty())
            && let Some(adjusted_prompt) = self.adjust_prompt(Some(pwsh_prompt), &line_text, ">")
        {
            return Some(WindowsPrompt { prompt: adjusted_prompt, likely_single_line: true });
        }

        // Custom prompts like starship end in the common ❯ character
        if let Some(found) = self.regexes.custom.exec(units, 0)
            && let Some(custom_prompt) = Some(JsString::from(&units[found.start()..found.end()])).filter(|prompt| !prompt.is_empty())
            && let Some(adjusted_prompt) = self.adjust_prompt(Some(custom_prompt), &line_text, "\u{276f}")
        {
            return Some(WindowsPrompt { prompt: adjusted_prompt, likely_single_line: false });
        }

        // Bash Prompt
        if let Some(bash_prompt) = group(&self.regexes.bash, "prompt").filter(|prompt| !prompt.is_empty())
            && let Some(adjusted_prompt) = self.adjust_prompt(Some(bash_prompt), &line_text, "$")
        {
            return Some(WindowsPrompt { prompt: adjusted_prompt, likely_single_line: false });
        }

        // Python Prompt: o `match` com a flag `g` do original não devolve `groups`, então este ramo nunca devolve nada.

        // Dynamic prompt detection
        let prompt_terminator = self.capability.upgrade().and_then(|capability| capability.prompt_terminator()).filter(|terminator| !terminator.is_empty());
        if let Some(prompt_terminator) = prompt_terminator {
            let terminator = JsString::from(prompt_terminator.as_str());
            if line_text == terminator || js_string::ends_with_at(js_string::trim(units), terminator.units(), js_string::trim(units).len()) {
                if let Some(adjusted_prompt) = self.adjust_prompt(Some(line_text.clone()), &line_text, &prompt_terminator) {
                    return Some(WindowsPrompt { prompt: adjusted_prompt, likely_single_line: false });
                }
            }
        }

        // Command Prompt
        group(&self.regexes.command_prompt, "prompt").filter(|prompt| !prompt.is_empty()).map(|prompt| WindowsPrompt { prompt, likely_single_line: true })
    }

    fn adjust_prompt(&self, prompt: Option<JsString>, line_text: &JsString, char_: &str) -> Option<JsString> {
        let mut prompt = prompt.filter(|prompt| !prompt.is_empty())?;
        let char_ = JsString::from(char_);
        // Conpty may not 'render' the space at the end of the prompt
        if *line_text == prompt && js_string::ends_with_at(prompt.units(), char_.units(), prompt.len()) {
            prompt.push_unit(u16::from(b' '));
        }
        Some(prompt)
    }
}

/// `getLinesForCommand(buffer, command, cols, outputMatcher?)`.
pub fn get_lines_for_command(buffer: &dyn IBuffer, command: &dyn ITerminalCommand, cols: i32, output_matcher: Option<&ITerminalOutputMatcher>) -> Option<Vec<String>> {
    let output_matcher = output_matcher?;
    let executed_marker = command.executed_marker()?;
    let end_marker = command.end_marker()?;
    let start_line = executed_marker.line() as i32;
    let end_line = end_marker.line() as i32;

    let lines_to_check = output_matcher.length;
    let mut lines: Vec<String> = Vec::new();
    let offset = output_matcher.offset as i32;
    if output_matcher.anchor == OutputMatcherAnchor::Bottom {
        let mut i = end_line - offset;
        while i >= start_line {
            let mut wrapped_line_start = i;
            let wrapped_line_end = i;
            while wrapped_line_start >= start_line && buffer.get_line(wrapped_line_start).is_some_and(|line| line.is_wrapped()) {
                wrapped_line_start -= 1;
            }
            i = wrapped_line_start;
            lines.insert(0, get_xterm_line_content(buffer, wrapped_line_start, f64::from(wrapped_line_end), cols).to_string_lossy());
            if lines.len() as f64 > lines_to_check {
                lines.pop();
            }
            i -= 1;
        }
    } else {
        let mut i = start_line + offset;
        while i < end_line {
            let wrapped_line_start = i;
            let mut wrapped_line_end = i;
            while wrapped_line_end + 1 < end_line && buffer.get_line(wrapped_line_end + 1).is_some_and(|line| line.is_wrapped()) {
                wrapped_line_end += 1;
            }
            i = wrapped_line_end;
            lines.push(get_xterm_line_content(buffer, wrapped_line_start, f64::from(wrapped_line_end), cols).to_string_lossy());
            if lines.len() as f64 == lines_to_check {
                lines.remove(0);
            }
            i += 1;
        }
    }
    Some(lines)
}
