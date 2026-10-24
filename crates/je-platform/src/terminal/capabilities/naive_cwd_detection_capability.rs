//! Porte de `src/vs/platform/terminal/common/capabilities/naiveCwdDetectionCapability.ts`: a capacidade que pergunta o
//! diretório de trabalho ao processo filho do terminal (a que o backend, e não a integração com o shell, oferece).
//!
//! Diferenças do original:
//! - O `if (!this._process)` do `getCwd` não existe: o processo é obrigatório no construtor, e um `Rc` não é nulo.
//! - O estado (o processo, o último cwd e o emissor) fica num `Rc` interno, porque a continuação da promessa o mantém vivo
//!   depois de o `getCwd` retornar. O emissor não é descartado, como no original (o `Emitter` não é registrado em nenhum
//!   `Disposable`).

use std::cell::RefCell;
use std::rc::Rc;

use je_base::event::{Emitter, Event};
use je_base::promise::Promise;

use super::capabilities::{INaiveCwdDetectionCapability, TerminalCapability};
use crate::terminal::terminal::ITerminalChildProcess;

/// O estado compartilhado entre a capacidade e a continuação do `getCwd`.
struct NaiveCwdState {
    process: Rc<dyn ITerminalChildProcess>,
    cwd: RefCell<String>,
    on_did_change_cwd: Rc<Emitter<String>>,
}

/// `NaiveCwdDetectionCapability`.
pub struct NaiveCwdDetectionCapability {
    state: Rc<NaiveCwdState>,
}

impl NaiveCwdDetectionCapability {
    pub fn new(process: Rc<dyn ITerminalChildProcess>) -> Rc<Self> {
        Rc::new(Self { state: Rc::new(NaiveCwdState { process, cwd: RefCell::new(String::new()), on_did_change_cwd: Emitter::new() }) })
    }
}

impl INaiveCwdDetectionCapability for NaiveCwdDetectionCapability {
    fn capability_type(&self) -> TerminalCapability {
        TerminalCapability::NaiveCwdDetection
    }

    fn on_did_change_cwd(&self) -> Event<String> {
        self.state.on_did_change_cwd.event()
    }

    fn get_cwd(&self) -> Promise<String> {
        let state = self.state.clone();
        self.state.process.get_cwd().then(move |new_cwd| {
            let changed = new_cwd != *state.cwd.borrow();
            if changed {
                state.on_did_change_cwd.fire(new_cwd.clone());
            }
            *state.cwd.borrow_mut() = new_cwd.clone();
            new_cwd
        })
    }
}
