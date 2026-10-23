//! Porte de `src/vs/base/common/errors.ts`: o tratador de erros inesperados e os erros nomeados.
//!
//! Exceção do TypeScript vira pânico carregando um [`Error`] (ver `docs/design.md`): [`throw`] levanta,
//! [`catch`] captura e devolve o erro, e os pontos do original que fazem `try { ... } catch (e) {
//! onUnexpectedError(e) }` usam [`catch`] e [`on_unexpected_error`].

use std::any::Any;
use std::cell::RefCell;
use std::fmt;
use std::panic::{self, AssertUnwindSafe};
use std::rc::Rc;

/// O nome do `CancellationError` (`canceledName` do original).
pub const CANCELED_NAME: &str = "Canceled";

/// Um erro com a forma do `Error` do JavaScript. `errors` é preenchido no `AggregateError`.
#[derive(Clone, Debug, PartialEq)]
pub struct Error {
    pub name: String,
    pub message: String,
    pub errors: Vec<Error>,
}

impl Error {
    pub fn new(message: impl Into<String>) -> Self {
        Self { name: "Error".into(), message: message.into(), errors: Vec::new() }
    }

    pub fn named(name: impl Into<String>, message: impl Into<String>) -> Self {
        Self { name: name.into(), message: message.into(), errors: Vec::new() }
    }

    /// `new AggregateError(errors, message)`.
    pub fn aggregate(errors: Vec<Error>, message: impl Into<String>) -> Self {
        Self { name: "AggregateError".into(), message: message.into(), errors }
    }

    /// `new CancellationError()`.
    pub fn cancellation() -> Self {
        Self::named(CANCELED_NAME, CANCELED_NAME)
    }

    pub fn is_aggregate(&self) -> bool {
        self.name == "AggregateError"
    }
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.message.is_empty() {
            f.write_str(&self.name)
        } else {
            write!(f, "{}: {}", self.name, self.message)
        }
    }
}

impl std::error::Error for Error {}

/// `throw error`: levanta o erro como pânico.
pub fn throw(error: Error) -> ! {
    panic::panic_any(error)
}

/// Converte o conteúdo de um pânico no [`Error`] que ele carrega; pânico comum do Rust (`panic!`,
/// `unwrap`) vira um `Error` com a mensagem dele, como uma exceção nativa do motor JS.
pub fn from_panic(payload: Box<dyn Any + Send>) -> Error {
    match payload.downcast::<Error>() {
        Ok(error) => *error,
        Err(payload) => match payload.downcast::<String>() {
            Ok(message) => Error::new(*message),
            Err(payload) => match payload.downcast::<&'static str>() {
                Ok(message) => Error::new(*message),
                Err(_) => Error::new("unknown error"),
            },
        },
    }
}

/// `try { f() } catch (e) { ... }`: roda `f` e devolve o erro lançado, se houver.
pub fn catch<T>(f: impl FnOnce() -> T) -> Result<T, Error> {
    with_quiet_panics(|| panic::catch_unwind(AssertUnwindSafe(f))).map_err(from_panic)
}

thread_local! {
    static QUIET: RefCell<u32> = const { RefCell::new(0) };
}

/// O gancho padrão de pânico imprime em stderr a cada `throw` capturado; dentro de um `catch` isso é
/// ruído (no JS a exceção capturada não aparece em lugar nenhum). O gancho é instalado uma vez e
/// consulta a profundidade de `catch` do thread.
fn with_quiet_panics<T>(f: impl FnOnce() -> T) -> T {
    static INSTALL: std::sync::Once = std::sync::Once::new();
    INSTALL.call_once(|| {
        let default = panic::take_hook();
        panic::set_hook(Box::new(move |info| {
            if QUIET.with(|q| *q.borrow()) == 0 {
                default(info);
            }
        }));
    });
    QUIET.with(|q| *q.borrow_mut() += 1);
    let result = f();
    QUIET.with(|q| *q.borrow_mut() -= 1);
    result
}

type Handler = Rc<dyn Fn(&Error)>;

struct ErrorHandler {
    unexpected: Option<Handler>,
    listeners: Vec<(u64, Handler)>,
    next_id: u64,
}

thread_local! {
    static HANDLER: RefCell<ErrorHandler> = RefCell::new(ErrorHandler { unexpected: None, listeners: Vec::new(), next_id: 0 });
}

/// `setUnexpectedErrorHandler`. Sem tratador definido, o padrão do original relança o erro num
/// `setTimeout`; aqui o padrão escreve em stderr, e o executável instala o tratador de log.
pub fn set_unexpected_error_handler(handler: impl Fn(&Error) + 'static) {
    HANDLER.with(|h| h.borrow_mut().unexpected = Some(Rc::new(handler)));
}

/// `errorHandler.addListener`: devolve a função que remove o ouvinte.
pub fn add_error_listener(listener: impl Fn(&Error) + 'static) -> impl FnOnce() {
    let id = HANDLER.with(|h| {
        let mut h = h.borrow_mut();
        h.next_id += 1;
        let id = h.next_id;
        h.listeners.push((id, Rc::new(listener)));
        id
    });
    move || HANDLER.with(|h| h.borrow_mut().listeners.retain(|(i, _)| *i != id))
}

fn dispatch(error: &Error, notify_listeners: bool) {
    let (handler, listeners) = HANDLER.with(|h| {
        let h = h.borrow();
        let listeners: Vec<Handler> = if notify_listeners { h.listeners.iter().map(|(_, l)| l.clone()).collect() } else { Vec::new() };
        (h.unexpected.clone(), listeners)
    });
    match handler {
        Some(handler) => handler(error),
        None => eprintln!("{error}"),
    }
    for listener in listeners {
        listener(error);
    }
}

/// `isCancellationError`.
pub fn is_cancellation_error(error: &Error) -> bool {
    error.name == CANCELED_NAME && error.message == CANCELED_NAME
}

/// `onUnexpectedError`: erro de cancelamento é ignorado.
pub fn on_unexpected_error(error: &Error) {
    if !is_cancellation_error(error) {
        dispatch(error, true);
    }
}

/// `onUnexpectedExternalError`: não avisa os ouvintes.
pub fn on_unexpected_external_error(error: &Error) {
    if !is_cancellation_error(error) {
        dispatch(error, false);
    }
}

/// `onBugIndicatingError`.
pub fn on_bug_indicating_error(error: &Error) {
    dispatch(error, true);
}
