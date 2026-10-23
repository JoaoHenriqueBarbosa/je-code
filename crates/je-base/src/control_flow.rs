//! Porte de `src/vs/base/common/controlFlow.ts`: ajudantes de fluxo de controle.

use std::cell::Cell;

use crate::errors::{self, Error};

/// Devolve a barreira ao estado livre ao sair, inclusive quando o `runner` lança (o `finally`).
struct Release<'a>(&'a Cell<bool>);

impl Drop for Release<'_> {
    fn drop(&mut self) {
        self.0.set(false);
    }
}

/// `ReentrancyBarrier`: impede que um trecho de código reentre em si mesmo.
#[derive(Default)]
pub struct ReentrancyBarrier {
    is_occupied: Cell<bool>,
}

impl ReentrancyBarrier {
    pub fn new() -> Self {
        Self::default()
    }

    /// `runExclusivelyOrSkip`: chama `runner` se a barreira estiver livre; durante a chamada ela fica ocupada.
    pub fn run_exclusively_or_skip(&self, runner: impl FnOnce()) {
        if self.is_occupied.get() {
            return;
        }
        self.is_occupied.set(true);
        let _release = Release(&self.is_occupied);
        runner();
    }

    /// `runExclusivelyOrThrow`: chama `runner`; se a barreira estiver ocupada, lança um `BugIndicatingError`.
    pub fn run_exclusively_or_throw(&self, runner: impl FnOnce()) {
        if self.is_occupied.get() {
            errors::throw(Error::bug_indicating(Some("ReentrancyBarrier: reentrant call detected!")));
        }
        self.is_occupied.set(true);
        let _release = Release(&self.is_occupied);
        runner();
    }

    /// `isOccupied`.
    pub fn is_occupied(&self) -> bool {
        self.is_occupied.get()
    }

    /// `makeExclusiveOrSkip(fn)`: a função devolvida não faz nada se a barreira estiver ocupada.
    pub fn make_exclusive_or_skip<'a, A>(&'a self, f: impl Fn(A) + 'a) -> impl Fn(A) + 'a {
        move |args| {
            if self.is_occupied.get() {
                return;
            }
            self.is_occupied.set(true);
            let _release = Release(&self.is_occupied);
            f(args);
        }
    }
}
