//! Porte de `src/vs/platform/terminal/common/capabilities/partialCommandDetectionCapability.ts`: a capacidade que
//! adivinha onde estão os comandos a partir de onde o cursor estava quando o Enter foi apertado. Acerta e erra,
//! mas costuma acertar e é melhor que nada.
//!
//! Diferenças do original:
//! - A classe estende `DisposableStore`; aqui é um [`Disposable`] por composição (descartar descarta os
//!   registros dela: a escuta de `onData`, o tratador de CSI `J` e a escuta de `onDidExecuteText`).
//! - `this.add(event(() => ...))` com a classe viva é uma escuta que guarda só a referência fraca.
//! - O tratador de CSI recebe `(number | number[])[]`: `params[0] === 2` só vale para um número.

use std::cell::RefCell;
use std::rc::{Rc, Weak};

use je_base::event::{Emitter, Event};
use je_base::js_string::JsString;
use je_base::lifecycle::{Disposable, IDisposable};

use super::capabilities::{IMarker, IPartialCommandDetectionCapability, TerminalCapability};
use crate::terminal::xterm_headless::{CsiParam, IHeadlessTerminal};

/// `Constants.MinimumPromptLength`: the minimum size of the prompt in which to assume the line is a command.
const MINIMUM_PROMPT_LENGTH: i32 = 2;

/// `PartialCommandDetectionCapability`.
pub struct PartialCommandDetectionCapability {
    base: Rc<Disposable>,
    terminal: Rc<dyn IHeadlessTerminal>,
    commands: RefCell<Vec<Rc<dyn IMarker>>>,
    on_command_finished: Rc<Emitter<Rc<dyn IMarker>>>,
}

impl PartialCommandDetectionCapability {
    pub fn new(terminal: Rc<dyn IHeadlessTerminal>, on_did_execute_text: Option<Event<()>>) -> Rc<Self> {
        let base = Disposable::new();
        let on_command_finished: Rc<Emitter<Rc<dyn IMarker>>> = Emitter::new();
        base.register(on_command_finished.clone());
        let capability = Rc::new(Self { base, terminal, commands: RefCell::new(Vec::new()), on_command_finished });

        let this: Weak<Self> = Rc::downgrade(&capability);
        capability.base.register(capability.terminal.on_data().listen({
            let this = this.clone();
            move |data: &JsString| {
                if let Some(this) = this.upgrade() {
                    this.handle_data(data);
                }
            }
        }));
        capability.base.register(capability.terminal.parser().register_csi_handler('J', {
            let this = this.clone();
            Rc::new(move |params: &[CsiParam]| {
                if let Some(this) = this.upgrade()
                    && !params.is_empty()
                    && (params[0].is(2) || params[0].is(3))
                {
                    this.clear_commands_in_viewport();
                }
                // We don't want to override xterm.js' default behavior, just augment it
                false
            })
        }));
        if let Some(on_did_execute_text) = on_did_execute_text {
            capability.base.register(on_did_execute_text.listen(move |_| {
                if let Some(this) = this.upgrade() {
                    this.on_enter();
                }
            }));
        }
        capability
    }

    fn handle_data(&self, data: &JsString) {
        if *data == "\r" {
            self.on_enter();
        }
    }

    fn on_enter(&self) {
        if self.terminal.buffer_active().cursor_x() >= MINIMUM_PROMPT_LENGTH
            && let Some(marker) = self.terminal.register_marker(0)
        {
            self.commands.borrow_mut().push(marker.clone());
            self.on_command_finished.fire(marker);
        }
    }

    fn clear_commands_in_viewport(&self) {
        // Find the number of commands on the tail end of the array that are within the viewport
        let base_y = f64::from(self.terminal.buffer_active().base_y());
        let mut commands = self.commands.borrow_mut();
        let mut count = 0;
        for command in commands.iter().rev() {
            if command.line() < base_y {
                break;
            }
            count += 1;
        }
        // Remove them
        let keep = commands.len() - count;
        commands.truncate(keep);
    }
}

impl IDisposable for PartialCommandDetectionCapability {
    fn dispose(&self) {
        self.base.dispose();
    }
}

impl IPartialCommandDetectionCapability for PartialCommandDetectionCapability {
    fn capability_type(&self) -> TerminalCapability {
        TerminalCapability::PartialCommandDetection
    }

    fn commands(&self) -> Vec<Rc<dyn IMarker>> {
        self.commands.borrow().clone()
    }

    fn on_command_finished(&self) -> Event<Rc<dyn IMarker>> {
        self.on_command_finished.event()
    }

    fn clear_commands(&self) {
        self.commands.borrow_mut().clear();
    }
}
