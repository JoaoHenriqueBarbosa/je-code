//! Porte de `src/vs/platform/terminal/common/capabilities/cwdDetectionCapability.ts`: a capacidade que guarda o
//! diretório de trabalho atual do terminal, informado pela integração com o shell.
//!
//! Diferenças do original:
//! - `_cwds` (um `Map` que mantém a ordem de inserção) é um [`IndexMap`]: o `delete` seguido de `set` do
//!   original, que põe a chave no fim, é o `shift_remove` seguido de `insert`.
//! - `updateCwd(cwd, isTrusted = true)`: o parâmetro com valor padrão é `Option<bool>`, e `None` vale `true`.
//! - O `Disposable` com o `Emitter` registrado: `dispose` descarta o emissor.

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use indexmap::IndexMap;
use je_base::event::{Emitter, Event};
use je_base::lifecycle::{Disposable, IDisposable};

use super::capabilities::{ICwdDetectionCapability, TerminalCapability};

/// `CwdDetectionCapability`.
pub struct CwdDetectionCapability {
    base: Rc<Disposable>,
    cwd: RefCell<String>,
    is_trusted: Cell<bool>,
    /// `Map<cwd, frequency>`.
    cwds: RefCell<IndexMap<String, f64>>,
    on_did_change_cwd: Rc<Emitter<String>>,
}

impl CwdDetectionCapability {
    pub fn new() -> Rc<Self> {
        let base = Disposable::new();
        let on_did_change_cwd: Rc<Emitter<String>> = Emitter::new();
        base.register(on_did_change_cwd.clone());
        Rc::new(Self { base, cwd: RefCell::new(String::new()), is_trusted: Cell::new(true), cwds: RefCell::new(IndexMap::new()), on_did_change_cwd })
    }
}

impl IDisposable for CwdDetectionCapability {
    fn dispose(&self) {
        self.base.dispose();
    }
}

impl ICwdDetectionCapability for CwdDetectionCapability {
    fn capability_type(&self) -> TerminalCapability {
        TerminalCapability::CwdDetection
    }

    fn on_did_change_cwd(&self) -> Event<String> {
        self.on_did_change_cwd.event()
    }

    /// Gets the list of cwds seen in this session in order of last accessed.
    fn cwds(&self) -> Vec<String> {
        self.cwds.borrow().keys().cloned().collect()
    }

    fn is_trusted(&self) -> bool {
        self.is_trusted.get()
    }

    fn get_cwd(&self) -> String {
        self.cwd.borrow().clone()
    }

    fn update_cwd(&self, cwd: &str, is_trusted: Option<bool>) {
        let is_trusted = is_trusted.unwrap_or(true);
        let did_change = *self.cwd.borrow() != cwd || self.is_trusted.get() != is_trusted;
        *self.cwd.borrow_mut() = cwd.to_owned();
        self.is_trusted.set(is_trusted);
        {
            let mut cwds = self.cwds.borrow_mut();
            let count = cwds.get(cwd).copied().unwrap_or(0.0);
            cwds.shift_remove(cwd); // Delete to put it at the bottom of the iterable
            cwds.insert(cwd.to_owned(), count + 1.0);
        }
        if did_change {
            self.on_did_change_cwd.fire(cwd.to_owned());
        }
    }
}
