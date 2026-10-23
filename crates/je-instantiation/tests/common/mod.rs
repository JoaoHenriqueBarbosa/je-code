//! Auxiliares dos testes portados: o `ensureNoDisposablesAreLeakedInTestSuite` do `utils.ts` do VS Code
//! (copiado do `common` do `je-base`, que não é uma dependência de testes).
#![allow(dead_code)]

use std::cell::RefCell;
use std::rc::Rc;

use je_base::errors::{self, Error};
use je_base::lifecycle::{
    DisposableRef, DisposableStore, DisposableTracker, IDisposable, set_disposable_tracker,
};

thread_local! {
    static UNEXPECTED: RefCell<Vec<Error>> = const { RefCell::new(Vec::new()) };
}

/// Em Node o tratador padrão de erro inesperado relança num `setTimeout`, e o mocha reprova o teste.
/// Aqui o tratador guarda o erro, e o teste reprova ao terminar (ver [`fail_on_unexpected_errors`]).
pub fn record_unexpected_errors() {
    errors::set_unexpected_error_handler(|e| UNEXPECTED.with(|u| u.borrow_mut().push(e.clone())));
}

/// Reprova o teste se algum erro inesperado foi reportado desde a última conferência.
pub fn fail_on_unexpected_errors() {
    let reported = UNEXPECTED.with(|u| std::mem::take(&mut *u.borrow_mut()));
    assert!(
        reported.is_empty(),
        "erros inesperados reportados: {}",
        reported
            .iter()
            .map(ToString::to_string)
            .collect::<Vec<_>>()
            .join("; ")
    );
}

/// `ensureNoDisposablesAreLeakedInTestSuite()`: o `setup` roda na criação e o `teardown` no `Drop`.
/// Devolve o store que o teste usa para registrar o que precisa ser descartado ao fim.
pub struct LeakGuard {
    store: Rc<DisposableStore>,
    tracker: Rc<DisposableTracker>,
}

pub fn ensure_no_disposables_are_leaked_in_test_suite() -> LeakGuard {
    record_unexpected_errors();
    let store = DisposableStore::new();
    let tracker = Rc::new(DisposableTracker::new());
    set_disposable_tracker(Some(tracker.clone()));
    LeakGuard { store, tracker }
}

impl LeakGuard {
    /// `add`: registra para descartar ao fim do teste.
    pub fn add(&self, o: DisposableRef) -> DisposableRef {
        self.store.add(o)
    }

    pub fn add_rc<T: IDisposable + 'static>(&self, o: Rc<T>) -> Rc<T> {
        self.store.add_rc(o)
    }

    /// O store, para quem precisa dele dentro de um fecho `'static`.
    pub fn store(&self) -> Rc<DisposableStore> {
        self.store.clone()
    }
}

impl Drop for LeakGuard {
    fn drop(&mut self) {
        self.store.dispose();
        set_disposable_tracker(None);
        if std::thread::panicking() {
            return;
        }
        fail_on_unexpected_errors();
        compute_leaking_disposables(&self.tracker, true);
    }
}

fn compute_leaking_disposables(tracker: &DisposableTracker, log_to_console: bool) {
    if let Some(result) = tracker.compute_leaking_disposables() {
        if log_to_console {
            eprintln!("{}", result.details);
        }
        panic!(
            "There are {} undisposed disposables!{}",
            result.leaks.len(),
            result.details
        );
    }
}
