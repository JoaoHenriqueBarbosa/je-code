//! Porte do `ErrorResult` e do `getReadonlyEmptyArray` de
//! `src/vs/editor/contrib/inlineCompletions/browser/utils.ts` (o restante do arquivo é do editor observável).
//!
//! Notas de tradução:
//! - `ErrorResult<T = void>` guarda `error: T` e `message?`; `ErrorResult.is(obj)` é a escolha de variante de um
//!   `Result<_, ErrorResult<T>>`, então não há função `is`;
//! - `getReadonlyEmptyArray` devolve uma fatia vazia compartilhada, que em Rust é `&[]`.

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
