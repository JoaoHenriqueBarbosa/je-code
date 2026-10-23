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

use crate::dynamic::{Dynamic, number_to_js_string};
use crate::js_string::JsString;

/// O nome do `CancellationError` (`canceledName` do original).
pub const CANCELED_NAME: &str = "Canceled";

/// Um erro com a forma do `Error` do JavaScript. `errors` é preenchido no `AggregateError`; `stack` é o
/// `error.stack`; `props` são as propriedades extras que o JS pendura num erro (o `ListenerLeakError`
/// leva `kind`, `listenerCount` e `details` aqui).
#[derive(Clone, Debug, PartialEq)]
pub struct Error {
    pub name: String,
    pub message: String,
    pub errors: Vec<Error>,
    pub stack: Option<String>,
    pub props: Vec<(String, String)>,
    /// `error.cause` (`new Error(message, { cause })`).
    pub cause: Option<Box<Error>>,
}

impl Error {
    pub fn new(message: impl Into<String>) -> Self {
        Self::named("Error", message)
    }

    pub fn named(name: impl Into<String>, message: impl Into<String>) -> Self {
        Self { name: name.into(), message: message.into(), errors: Vec::new(), stack: None, props: Vec::new(), cause: None }
    }

    /// `new Error(message, { cause })`.
    pub fn with_cause(mut self, cause: Error) -> Self {
        self.cause = Some(Box::new(cause));
        self
    }

    /// `error.stack = stack`.
    pub fn with_stack(mut self, stack: impl Into<String>) -> Self {
        self.stack = Some(stack.into());
        self
    }

    /// `error.<name> = value`.
    pub fn with_prop(mut self, name: impl Into<String>, value: impl ToString) -> Self {
        self.props.push((name.into(), value.to_string()));
        self
    }

    /// `error.<name>`.
    pub fn prop(&self, name: &str) -> Option<&str> {
        self.props.iter().find(|(n, _)| n == name).map(|(_, v)| v.as_str())
    }

    /// `new AggregateError(errors, message)`.
    pub fn aggregate(errors: Vec<Error>, message: impl Into<String>) -> Self {
        Self { errors, ..Self::named("AggregateError", message) }
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

/// `getErrorCode`: o `code` próprio de um objeto, quando é texto ou número (o número vira o seu texto).
/// `Object.hasOwn` olha só as propriedades próprias; o [`Dynamic`] não tem cadeia de protótipos, então um
/// `code` herdado nem chega a existir e dá `None` do mesmo jeito.
pub fn get_error_code(error: &Dynamic) -> Option<String> {
    if error.type_of() != "object" || error.is_null() || !error.has_property(&JsString::from("code")) {
        return None;
    }
    match error.get_str("code") {
        Dynamic::String(code) => Some(code.to_string_lossy()),
        Dynamic::Number(code) => Some(number_to_js_string(code)),
        _ => None,
    }
}

/// `SerializedError`. O `$isError: true` do original é constante e não ocupa campo aqui (o canal do
/// worker o escreve em `worker::web_worker::serialized_error`). `stack` é opcional porque o `Error` do
/// porte pode não ter pilha; no JavaScript ela é sempre uma `string`.
#[derive(Clone, Debug, PartialEq)]
pub struct SerializedError {
    pub name: String,
    pub message: String,
    pub stack: Option<String>,
    pub no_telemetry: bool,
    pub code: Option<String>,
    pub cause: Option<Box<SerializedError>>,
}

/// `transformErrorForSerialization`: `stacktrace` (quando existe e não é vazio) vale mais que `stack`, e o
/// `code` é a propriedade `code` do erro.
pub fn transform_error_for_serialization(error: &Error) -> SerializedError {
    SerializedError {
        name: error.name.clone(),
        message: error.message.clone(),
        stack: error.prop("stacktrace").filter(|s| !s.is_empty()).map(str::to_owned).or_else(|| error.stack.clone()),
        no_telemetry: error.is_error_no_telemetry(),
        code: error.prop("code").map(str::to_owned),
        cause: error.cause.as_deref().map(|cause| Box::new(transform_error_for_serialization(cause))),
    }
}

/// `transformErrorFromSerialization`: com `noTelemetry` o erro é um `ErrorNoTelemetry`, senão um `Error`
/// com o `name` dado; o `code` só é posto quando não é vazio.
pub fn transform_error_from_serialization(data: &SerializedError) -> Error {
    let mut error = if data.no_telemetry { Error::no_telemetry("") } else { Error::named(data.name.clone(), "") };
    error.message = data.message.clone();
    error.stack = data.stack.clone();
    if let Some(code) = data.code.as_deref().filter(|code| !code.is_empty()) {
        error.props.push(("code".to_owned(), code.to_owned()));
    }
    error.cause = data.cause.as_deref().map(|cause| Box::new(transform_error_from_serialization(cause)));
    error
}

/// O nome do `ErrorNoTelemetry` (`'CodeExpectedError'`).
pub const NO_TELEMETRY_NAME: &str = "CodeExpectedError";

impl Error {
    /// `new ErrorNoTelemetry(msg)`: erro que não vai para a telemetria como erro não tratado.
    pub fn no_telemetry(message: impl Into<String>) -> Self {
        Self::named(NO_TELEMETRY_NAME, message)
    }

    /// `ErrorNoTelemetry.fromError(err)`: o próprio erro se já é um `ErrorNoTelemetry`, senão um novo com a
    /// mensagem e a pilha dele.
    pub fn no_telemetry_from_error(err: &Error) -> Self {
        if err.is_error_no_telemetry() {
            return err.clone();
        }
        let mut result = Self::no_telemetry("");
        result.message = err.message.clone();
        result.stack = err.stack.clone();
        result
    }

    /// `ErrorNoTelemetry.isErrorNoTelemetry(err)`.
    pub fn is_error_no_telemetry(&self) -> bool {
        self.name == NO_TELEMETRY_NAME
    }
}

impl Error {
    /// `new TypeError(message)`.
    pub fn type_error(message: impl Into<String>) -> Self {
        Self::named("TypeError", message)
    }

    /// `new BugIndicatingError(message)`. O original não muda `name` (continua `Error`) e a
    /// distinção é só por classe (`instanceof`); aqui a mensagem padrão é a mesma.
    pub fn bug_indicating(message: Option<&str>) -> Self {
        match message {
            Some(m) if !m.is_empty() => Self::new(m),
            _ => Self::new("An unexpected bug occurred."),
        }
    }

    /// `new ReadonlyError(name)`, um `TypeError`.
    pub fn readonly(name: Option<&str>) -> Self {
        match name {
            Some(n) if !n.is_empty() => Self::type_error(format!("{n} is read-only and cannot be changed")),
            _ => Self::type_error("Cannot change read-only property"),
        }
    }
}
