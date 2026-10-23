//! Porte de `observableInternal/transaction.ts`.

use std::cell::RefCell;
use std::future::Future;
use std::rc::Rc;

use super::base::{ITransaction, ObservableRef, ObserverRef, handle_bug_indicating_error_recovery};
use crate::errors;

thread_local! {
    static GLOBAL_TRANSACTION: RefCell<Option<Rc<TransactionImpl>>> = const { RefCell::new(None) };
}

/// Roda `f` e depois `finish()`, mesmo que `f` lance (o `try/finally` do original).
fn run_then_finish(tx: &Rc<TransactionImpl>, f: impl FnOnce(&dyn ITransaction)) {
    let result = errors::catch(|| f(&**tx));
    tx.finish();
    if let Err(e) = result {
        errors::throw(e);
    }
}

/// `transaction(fn, getDebugName)`: os observadores reagem quando `f` termina.
pub fn transaction(f: impl FnOnce(&dyn ITransaction)) {
    let tx = Rc::new(TransactionImpl::new());
    run_then_finish(&tx, f);
}

/// `globalTransaction(fn)`.
pub fn global_transaction(f: impl FnOnce(&dyn ITransaction)) {
    let existing = GLOBAL_TRANSACTION.with(|g| g.borrow().clone());
    if let Some(tx) = existing {
        f(&*tx);
        return;
    }
    let tx = Rc::new(TransactionImpl::new());
    GLOBAL_TRANSACTION.with(|g| *g.borrow_mut() = Some(tx.clone()));
    // Durante o `finish` podem entrar mais ações, por isso só limpa depois.
    run_then_finish(&tx, f);
    GLOBAL_TRANSACTION.with(|g| *g.borrow_mut() = None);
}

/// `asyncTransaction(fn)` (obsoleto no original): `f` devolve o futuro que usa a transação.
pub async fn async_transaction<F: Future<Output = ()>>(f: impl FnOnce(Rc<TransactionImpl>) -> F) {
    let tx = Rc::new(TransactionImpl::new());
    f(tx.clone()).await;
    tx.finish();
}

/// `subtransaction(tx, fn)`: encadeia transações.
pub fn subtransaction(tx: Option<&dyn ITransaction>, f: impl FnOnce(&dyn ITransaction)) {
    match tx {
        None => transaction(f),
        Some(tx) => f(tx),
    }
}

/// `TransactionImpl`.
pub struct TransactionImpl {
    updating_observers: RefCell<Option<Vec<(ObserverRef, ObservableRef)>>>,
}

impl Default for TransactionImpl {
    fn default() -> Self {
        Self::new()
    }
}

impl TransactionImpl {
    pub fn new() -> Self {
        Self { updating_observers: RefCell::new(Some(Vec::new())) }
    }

    /// `finish()`: chama `end_update` de todos; quem entrar durante o `finish` também é considerado.
    pub fn finish(&self) {
        if self.updating_observers.borrow().is_none() {
            handle_bug_indicating_error_recovery("transaction.finish() has already been called!");
            return;
        }
        let mut i = 0;
        loop {
            let item = {
                let b = self.updating_observers.borrow();
                match b.as_ref().and_then(|v| v.get(i)) {
                    Some((o, obs)) => (o.clone(), obs.clone()),
                    None => break,
                }
            };
            item.0.end_update(&*item.1);
            i += 1;
        }
        // Ninguém mais pode atualizar observadores.
        *self.updating_observers.borrow_mut() = None;
    }

    /// `debugGetUpdatingObservers()`.
    pub fn debug_get_updating_observers(&self) -> usize {
        self.updating_observers.borrow().as_ref().map_or(0, Vec::len)
    }
}

impl ITransaction for TransactionImpl {
    fn update_observer(&self, observer: ObserverRef, observable: ObservableRef) {
        if self.updating_observers.borrow().is_none() {
            // Acontece quando uma transação é usada num callback ou função assíncrona.
            handle_bug_indicating_error_recovery("Transaction already finished!");
            transaction(|tx| tx.update_observer(observer, observable));
            return;
        }
        self.updating_observers.borrow_mut().as_mut().expect("conferido acima").push((observer.clone(), observable.clone()));
        observer.begin_update(&*observable);
    }
}
