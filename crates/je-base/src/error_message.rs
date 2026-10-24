//! Porte de `src/vs/base/common/errorMessage.ts`: `toErrorMessage` e os erros com ações
//! (`IErrorWithActions`, `isErrorWithActions`, `createErrorWithActions`).
//!
//! Decisões de tradução:
//! - **`error: any`** é [`Dynamic`]: o original inspeciona a forma do valor (`detail.error`,
//!   `detail.exception`, `stack`, `stacktrace`, `message`, `code`, `errno`, `syscall`), e `Dynamic` lê
//!   propriedades por nome com a semântica do JavaScript. Um [`Error`] do porte entra por
//!   [`error_to_dynamic`] (ou direto por [`to_error_message_of`]), que expõe `name`, `message`, `stack` e
//!   as propriedades extras (`code`, `syscall` e `errno`, este como número quando é um).
//! - **`error = null` e `verbose = false`**: o parâmetro padrão é `Dynamic::Null`, o chamador passa
//!   `&Dynamic::Null` e `false`.
//! - **Valores que não são texto** onde o original interpola ou devolve `exception.message`, ou
//!   `exception.stack`, viram o `String(value)` do JavaScript, de forma simplificada (`undefined`, `null`,
//!   números, booleanos, arrays unidos por vírgula e `[object Object]` para o resto). Os motores JS só
//!   produzem texto nesses campos.
//! - **`IErrorWithActions`**: o original pendura `actions` no próprio objeto `Error`. O [`Error`] do porte
//!   guarda propriedades como texto, então a lista de ações (`Rc<dyn IAction>`) não cabe nele. O erro com
//!   ações é a struct [`ErrorWithActions`], com o [`Error`] e as `actions`, e `isErrorWithActions` é
//!   `error.downcast_ref` sobre o `&dyn Any` que [`as_error_with_actions`] recebe.
//! - Mensagens localizadas passam por [`nls::localize`], que devolve a mensagem padrão em inglês.

use std::any::Any;

use crate::actions::ActionRef;
use crate::arrays;
use crate::dynamic::{Dynamic, ObjectRef, Truthy, number_to_js_string};
use crate::errors::Error;
use crate::js_string::JsString;
use crate::nls;

const DEFAULT_MESSAGE: &str = "An unknown error occurred. Please consult the log for more details.";

fn default_message() -> String {
    nls::localize("error.defaultMessage", DEFAULT_MESSAGE, &[])
}

/// `String(value)`, simplificado (ver o topo do módulo).
fn js_to_string(value: &Dynamic) -> String {
    match value {
        Dynamic::Undefined => "undefined".to_owned(),
        Dynamic::Null => "null".to_owned(),
        Dynamic::Bool(b) => b.to_string(),
        Dynamic::Number(n) => number_to_js_string(*n),
        Dynamic::BigInt(n) => n.to_string(),
        Dynamic::String(s) => s.to_string_lossy(),
        Dynamic::Array(items) => items.to_vec().iter().map(|item| if item.is_nullish() { String::new() } else { js_to_string(item) }).collect::<Vec<_>>().join(","),
        _ => "[object Object]".to_owned(),
    }
}

/// `stackToString(stack)`: um array de linhas vira o texto unido por `\n`, o resto passa como está.
fn stack_to_string(stack: Dynamic) -> Dynamic {
    match &stack {
        Dynamic::Array(lines) => {
            Dynamic::string(&lines.to_vec().iter().map(|line| if line.is_nullish() { String::new() } else { js_to_string(line) }).collect::<Vec<_>>().join("\n"))
        }
        _ => stack,
    }
}

fn exception_to_error_message(exception: &Dynamic, verbose: bool) -> String {
    let stack = exception.get_str("stack");
    let stacktrace = exception.get_str("stacktrace");
    if verbose && (stack.is_truthy() || stacktrace.is_truthy()) {
        let first = stack_to_string(stack);
        let joined = if first.is_truthy() { first } else { stack_to_string(stacktrace) };
        return nls::localize("stackTrace.format", "{0}: {1}", &[&detect_system_error_message(exception), &js_to_string(&joined)]);
    }

    detect_system_error_message(exception)
}

fn detect_system_error_message(exception: &Dynamic) -> String {
    let code = exception.get_str("code");
    let message = exception.get_str("message");

    // Custom node.js error from us
    if code == Dynamic::string("ERR_UNC_HOST_NOT_ALLOWED") {
        return format!("{}. Please update the 'security.allowedUNCHosts' setting if you want to allow this host.", js_to_string(&message));
    }

    // See https://nodejs.org/api/errors.html#errors_class_system_error
    if matches!(code, Dynamic::String(_)) && matches!(exception.get_str("errno"), Dynamic::Number(_)) && matches!(exception.get_str("syscall"), Dynamic::String(_)) {
        return nls::localize("nodeExceptionMessage", "A system error occurred ({0})", &[&js_to_string(&message)]);
    }

    if message.is_truthy() { js_to_string(&message) } else { default_message() }
}

/// Tries to generate a human readable error message out of the error. If the verbose parameter
/// is set to true, the error message will include stacktrace details if provided.
///
/// Returns a string containing the error message.
pub fn to_error_message(error: &Dynamic, verbose: bool) -> String {
    if !error.is_truthy() {
        return default_message();
    }

    if let Dynamic::Array(items) = error {
        let present: Vec<Option<Dynamic>> = items.to_vec().into_iter().map(Some).collect();
        let errors: Vec<Dynamic> = arrays::coalesce(&present);
        let msg = to_error_message(errors.first().unwrap_or(&Dynamic::Undefined), verbose);

        if errors.len() > 1 {
            return nls::localize("error.moreErrors", "{0} ({1} errors in total)", &[&msg, &errors.len().to_string()]);
        }

        return msg;
    }

    if let Dynamic::String(text) = error {
        return text.to_string_lossy();
    }

    let detail = error.get_str("detail");
    if detail.is_truthy() {
        let inner = detail.get_str("error");
        if inner.is_truthy() {
            return exception_to_error_message(&inner, verbose);
        }

        let exception = detail.get_str("exception");
        if exception.is_truthy() {
            return exception_to_error_message(&exception, verbose);
        }
    }

    if error.get_str("stack").is_truthy() {
        return exception_to_error_message(error, verbose);
    }

    let message = error.get_str("message");
    if message.is_truthy() {
        return js_to_string(&message);
    }

    default_message()
}

/// A forma de um [`Error`] do porte como objeto JavaScript (ver o topo do módulo).
pub fn error_to_dynamic(error: &Error) -> Dynamic {
    let object = ObjectRef::new();
    object.set(&JsString::from("name"), Dynamic::string(&error.name));
    object.set(&JsString::from("message"), Dynamic::string(&error.message));
    if let Some(stack) = &error.stack {
        object.set(&JsString::from("stack"), Dynamic::string(stack));
    }
    for (name, value) in &error.props {
        let value = match (name.as_str(), value.parse::<f64>()) {
            ("errno", Ok(number)) => Dynamic::Number(number),
            _ => Dynamic::string(value),
        };
        object.set(&JsString::from(name.as_str()), value);
    }
    Dynamic::Object(object)
}

/// [`to_error_message`] de um [`Error`] do porte.
pub fn to_error_message_of(error: &Error, verbose: bool) -> String {
    to_error_message(&error_to_dynamic(error), verbose)
}

/// `IErrorWithActions`: um erro com as ações que o usuário pode tomar para resolvê-lo.
#[derive(Clone)]
pub struct ErrorWithActions {
    pub error: Error,
    pub actions: Vec<ActionRef>,
}

/// As ações são `Rc<dyn IAction>`, sem `Debug`: o formato mostra só o erro e quantas ações ele carrega.
impl std::fmt::Debug for ErrorWithActions {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ErrorWithActions").field("error", &self.error).field("actions", &self.actions.len()).finish()
    }
}

/// `isErrorWithActions(obj)`: o `obj` é um [`ErrorWithActions`]?
pub fn is_error_with_actions(obj: &dyn Any) -> bool {
    as_error_with_actions(obj).is_some()
}

pub fn as_error_with_actions(obj: &dyn Any) -> Option<&ErrorWithActions> {
    obj.downcast_ref::<ErrorWithActions>()
}

/// `createErrorWithActions(messageOrError, actions)`: com um texto cria um `Error` novo, com um [`Error`]
/// reaproveita o objeto.
pub fn create_error_with_actions(message_or_error: MessageOrError, actions: Vec<ActionRef>) -> ErrorWithActions {
    let error = match message_or_error {
        MessageOrError::Message(message) => Error::new(message),
        MessageOrError::Error(error) => error,
    };

    ErrorWithActions { error, actions }
}

/// `string | Error`.
pub enum MessageOrError {
    Message(String),
    Error(Error),
}

impl From<&str> for MessageOrError {
    fn from(message: &str) -> Self {
        MessageOrError::Message(message.to_owned())
    }
}

impl From<Error> for MessageOrError {
    fn from(error: Error) -> Self {
        MessageOrError::Error(error)
    }
}
