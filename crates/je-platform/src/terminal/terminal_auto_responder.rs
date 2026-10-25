//! Porte de `src/vs/platform/terminal/node/terminalContrib/autoReplies/terminalAutoResponder.ts`: acompanha o fluxo de
//! dados de um processo de terminal e responde na hora quando chega um texto que casa. É feito com pouca sobrecarga e
//! o ideal é rodar no mesmo processo que cuida do processo do terminal, para minimizar a latência.
//!
//! Representação: `extends Disposable` é um [`Disposable`] por composição. O estado (`_pointer`, `_paused`,
//! `_throttled`) fica em células compartilhadas com o ouvinte de `onProcessData`. `matchWord[i]` e `data[i]` são
//! unidades de código UTF-16, então a comparação é feita sobre `encode_utf16`.

use std::cell::Cell;
use std::rc::Rc;

use je_base::async_::timeout;
use je_base::lifecycle::{Disposable, IDisposable};
use je_base::platform::Platform;

use crate::log::LogServiceRef;
use crate::terminal::terminal::ITerminalChildProcess;

/// Tracks a terminal process's data stream and responds immediately when a matching string is
/// received. This is done in a low overhead way and is ideally run on the same process as the
/// where the process is handled to minimize latency.
pub struct TerminalAutoResponder {
    base: Rc<Disposable>,
    paused: Rc<Cell<bool>>,
    is_windows: bool,
}

impl TerminalAutoResponder {
    pub fn new(proc: Rc<dyn ITerminalChildProcess>, match_word: &str, response: &str, log_service: LogServiceRef) -> Rc<Self> {
        let base = Disposable::new();
        let pointer = Rc::new(Cell::new(0usize));
        let paused = Rc::new(Cell::new(false));
        // Each reply is throttled by a second to avoid resource starvation and responding to screen
        // reprints on Winodws.
        let throttled = Rc::new(Cell::new(false));

        let match_units: Vec<u16> = match_word.encode_utf16().collect();
        let match_word = match_word.to_owned();
        let response = response.to_owned();
        let listener = {
            let (pointer, paused, process) = (pointer.clone(), paused.clone(), proc.clone());
            proc.on_process_data().listen(move |event| {
                if paused.get() || throttled.get() {
                    return;
                }
                let data = event.data();
                for unit in data.encode_utf16() {
                    if match_units.get(pointer.get()) == Some(&unit) {
                        pointer.set(pointer.get() + 1);
                    } else {
                        pointer.set(0);
                    }
                    // Auto reply and reset
                    if pointer.get() == match_units.len() {
                        log_service.debug(&format!("Auto reply match: \"{match_word}\", response: \"{response}\""));
                        process.input(&response);
                        throttled.set(true);
                        let throttled = throttled.clone();
                        timeout(1000).then(move |()| throttled.set(false));
                        pointer.set(0);
                    }
                }
            })
        };
        base.register(listener);
        Rc::new(Self { base, paused, is_windows: Platform::detect().is_windows })
    }

    /// No auto response will happen after a resize on Windows in case the resize is a result of
    /// reprinting the screen.
    pub fn handle_resize(&self) {
        if self.is_windows {
            self.paused.set(true);
        }
    }

    pub fn handle_input(&self) {
        self.paused.set(false);
    }
}

impl IDisposable for TerminalAutoResponder {
    fn dispose(&self) {
        self.base.dispose();
    }
}
