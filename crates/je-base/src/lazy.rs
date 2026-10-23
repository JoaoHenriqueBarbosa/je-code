//! Porte de `src/vs/base/common/lazy.ts`.
//!
//! O valor é calculado uma vez, na primeira leitura. Se o executor lança (pânico com [`Error`]), o
//! erro fica guardado e toda leitura seguinte o lança de novo. Ler o valor de dentro do próprio
//! executor lança `Cannot read the value of a lazy that is being initialized`.
//!
//! `value()` devolve `T` por clone (o valor fica no `Lazy`, que o JS devolveria por referência);
//! use `Rc<_>` em `T` para valores caros de clonar. `rawValue` é [`Lazy::raw_value`].

use std::cell::{Cell, RefCell};

use crate::errors::{self, Error};

#[derive(Clone, Copy, PartialEq, Eq)]
enum State {
    Uninitialized,
    Running,
    Completed,
}

pub struct Lazy<T> {
    state: Cell<State>,
    value: RefCell<Option<T>>,
    error: RefCell<Option<Error>>,
    executor: RefCell<Option<Box<dyn FnOnce() -> T>>>,
}

impl<T: Clone> Lazy<T> {
    pub fn new(executor: impl FnOnce() -> T + 'static) -> Self {
        Self {
            state: Cell::new(State::Uninitialized),
            value: RefCell::new(None),
            error: RefCell::new(None),
            executor: RefCell::new(Some(Box::new(executor))),
        }
    }

    /// O valor já foi resolvido (com sucesso ou com erro)?
    pub fn has_value(&self) -> bool {
        self.state.get() == State::Completed
    }

    /// O valor embrulhado; força o cálculo se ainda não houve. Relança o erro do executor.
    pub fn value(&self) -> T {
        match self.state.get() {
            State::Uninitialized => {
                self.state.set(State::Running);
                let executor = self.executor.borrow_mut().take().expect("executor presente antes da primeira leitura");
                match errors::catch(executor) {
                    Ok(value) => *self.value.borrow_mut() = Some(value),
                    Err(error) => *self.error.borrow_mut() = Some(error),
                }
                self.state.set(State::Completed);
            }
            State::Running => errors::throw(Error::new("Cannot read the value of a lazy that is being initialized")),
            State::Completed => {}
        }
        if let Some(error) = self.error.borrow().clone() {
            errors::throw(error);
        }
        self.value.borrow().clone().expect("valor resolvido")
    }

    /// O valor sem forçar o cálculo.
    pub fn raw_value(&self) -> Option<T> {
        self.value.borrow().clone()
    }
}
