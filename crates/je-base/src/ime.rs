//! Porte de `src/vs/base/common/ime.ts`: liga e desliga o IME, e avisa quem escuta.

use std::cell::Cell;
use std::rc::Rc;

use crate::event::{Emitter, Event};

/// `IMEImpl`.
pub struct IMEImpl {
    on_did_change: Rc<Emitter<()>>,
    enabled: Cell<bool>,
}

impl IMEImpl {
    pub fn new() -> Self {
        Self { on_did_change: Emitter::new(), enabled: Cell::new(true) }
    }

    /// `onDidChange`.
    pub fn on_did_change(&self) -> Event<()> {
        self.on_did_change.event()
    }

    /// `enabled`.
    pub fn enabled(&self) -> bool {
        self.enabled.get()
    }

    /// Enable IME.
    pub fn enable(&self) {
        self.enabled.set(true);
        self.on_did_change.fire(());
    }

    /// Disable IME.
    pub fn disable(&self) {
        self.enabled.set(false);
        self.on_did_change.fire(());
    }
}

impl Default for IMEImpl {
    fn default() -> Self {
        Self::new()
    }
}

thread_local! {
    /// `IME`: a instância única do original, uma por thread (a UI roda numa só).
    static IME: Rc<IMEImpl> = Rc::new(IMEImpl::new());
}

/// `IME`.
pub fn ime() -> Rc<IMEImpl> {
    IME.with(Rc::clone)
}
