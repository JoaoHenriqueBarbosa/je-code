//! Porte do `ErrorResult` e do `getReadonlyEmptyArray` de
//! `src/vs/editor/contrib/inlineCompletions/browser/utils.ts` (o restante do arquivo é do editor observável).
//!
//! Notas de tradução:
//! - `ErrorResult<T = void>` guarda `error: T` e `message?`; `ErrorResult.is(obj)` é a escolha de variante de um
//!   `Result<_, ErrorResult<T>>`, então não há função `is`;
//! - `getReadonlyEmptyArray` devolve uma fatia vazia compartilhada, que em Rust é `&[]`;
//! - `wait(ms, token?)` é uma [`Promise`] que resolve no tempo ou no cancelamento do token (nunca rejeita).

use std::cell::RefCell;
use std::rc::Rc;

use je_base::cancellation::CancellationToken;
use je_base::lifecycle::{DisposableRef, IDisposable};
use je_base::promise::Promise;
use je_base::runtime;

/// `ErrorResult<T>`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ErrorResult<T = ()> {
    pub error: T,
    pub message: Option<String>,
}

impl ErrorResult<()> {
    /// `ErrorResult.message(message)`.
    pub fn message(message: impl Into<String>) -> Self {
        Self { error: (), message: Some(message.into()) }
    }
}

impl<T> ErrorResult<T> {
    pub fn new(error: T, message: Option<String>) -> Self {
        Self { error, message }
    }
}

impl<T: std::fmt::Debug> ErrorResult<T> {
    /// `logError()`.
    pub fn log_error(&self) {
        match self.message.as_deref().filter(|message| !message.is_empty()) {
            Some(message) => eprintln!("ErrorResult: {message} {:?}", self.error),
            None => eprintln!(
                "ErrorResult: An unexpected error-case occurred, usually caused by invalid input. {:?}",
                self.error
            ),
        }
    }
}

/// `getReadonlyEmptyArray()`.
pub fn get_readonly_empty_array<T>() -> &'static [T] {
    &[]
}

/// `wait(ms, cancellationToken?)`: resolve depois de `ms` milissegundos, ou assim que o token cancelar (sem rejeitar,
/// diferente de `timeout` de `async`).
pub fn wait(ms: u64, cancellation_token: Option<&CancellationToken>) -> Promise<()> {
    let token = cancellation_token.cloned();
    Promise::with_executor(move |resolver| {
        let disposable: Rc<RefCell<Option<DisposableRef>>> = Rc::default();
        let handle = runtime::set_timeout(
            {
                let (disposable, resolver) = (Rc::clone(&disposable), resolver.clone());
                move || {
                    dispose_slot(&disposable);
                    resolver.resolve(());
                }
            },
            ms,
        );
        if let Some(token) = token {
            let listener = token.on_cancellation_requested().listen({
                let disposable = Rc::clone(&disposable);
                move |_| {
                    runtime::clear_timeout(handle);
                    dispose_slot(&disposable);
                    resolver.resolve(());
                }
            });
            *disposable.borrow_mut() = Some(listener);
        }
    })
}

/// `if (d) { d.dispose(); }`.
fn dispose_slot(slot: &Rc<RefCell<Option<DisposableRef>>>) {
    let current = slot.borrow_mut().take();
    if let Some(current) = current {
        current.dispose();
    }
}
