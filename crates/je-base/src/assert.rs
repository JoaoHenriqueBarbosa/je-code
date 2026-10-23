//! Porte de `src/vs/base/common/assert.ts`: asserções que lançam (pânico com [`Error`], ver
//! `docs/design.md`) ou reportam como erro inesperado.
//!
//! O TypeScript recebe `string | Error` em `assert`; aqui isso é [`ErrorOrMessage`], e `()` pede a
//! mensagem padrão (`'unexpected state'`). `assertNever(value: never)` não tem o argumento de valor
//! (o tipo `never` não existe como valor em Rust), só a mensagem.

use crate::dynamic::Truthy;
use crate::errors::{self, Error};

/// `string | Error`: o que `assert` e `assertDefined` aceitam como motivo da falha.
#[derive(Clone, Debug)]
pub enum ErrorOrMessage {
    Message(String),
    Error(Error),
}

impl From<&str> for ErrorOrMessage {
    fn from(message: &str) -> Self {
        Self::Message(message.to_string())
    }
}

impl From<String> for ErrorOrMessage {
    fn from(message: String) -> Self {
        Self::Message(message)
    }
}

impl From<Error> for ErrorOrMessage {
    fn from(error: Error) -> Self {
        Self::Error(error)
    }
}

/// O argumento omitido: vale o padrão de cada função.
impl From<()> for ErrorOrMessage {
    fn from(_: ()) -> Self {
        Self::Message(String::new())
    }
}

/// `ok(value, message)`: lança se `value` não for verdadeiro no sentido do JavaScript.
pub fn ok(value: &impl Truthy, message: Option<&str>) {
    if !value.is_truthy() {
        errors::throw(match message {
            Some(m) if !m.is_empty() => Error::new(format!("Assertion failed ({m})")),
            _ => Error::new("Assertion Failed"),
        });
    }
}

/// `assertNever(value, message = 'Unreachable')`.
pub fn assert_never(message: Option<&str>) -> ! {
    errors::throw(Error::new(message.unwrap_or("Unreachable")))
}

/// `softAssertNever`: não faz nada.
pub fn soft_assert_never() {}

/// `assert(condition, messageOrError = 'unexpected state')`: lança `message_or_error` se
/// `condition` for falso. Uma mensagem vira um `BugIndicatingError` com o prefixo
/// `Assertion Failed: `; um `Error` é lançado como veio.
pub fn assert(condition: bool, message_or_error: impl Into<ErrorOrMessage>) {
    if !condition {
        let reason = match message_or_error.into() {
            ErrorOrMessage::Message(m) if m.is_empty() => "unexpected state".to_string(),
            ErrorOrMessage::Message(m) => m,
            ErrorOrMessage::Error(e) => errors::throw(e),
        };
        errors::throw(Error::bug_indicating(Some(&format!("Assertion Failed: {reason}"))));
    }
}

/// `softAssert(condition, message = 'Soft Assertion Failed')`: como `assert`, mas não lança,
/// reporta como erro inesperado.
pub fn soft_assert(condition: bool, message: Option<&str>) {
    if !condition {
        errors::on_unexpected_error(&Error::bug_indicating(Some(message.unwrap_or("Soft Assertion Failed"))));
    }
}

/// `assertFn(condition)`: `condition` precisa ser livre de efeitos colaterais.
pub fn assert_fn(condition: impl Fn() -> bool) {
    if !condition() {
        // Reavalia para facilitar a depuração, como o original.
        condition();
        errors::on_unexpected_error(&Error::bug_indicating(Some("Assertion Failed")));
    }
}

/// `checkAdjacentItems`.
pub fn check_adjacent_items<T>(items: &[T], predicate: impl Fn(&T, &T) -> bool) -> bool {
    items.windows(2).all(|pair| predicate(&pair[0], &pair[1]))
}
