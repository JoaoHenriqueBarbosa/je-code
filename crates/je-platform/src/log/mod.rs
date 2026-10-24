//! Porte de `src/vs/platform/log/common/log.ts` e `logService.ts`: níveis, `ILogger`, `ILoggerService`,
//! os loggers de console, de mensagem, de adaptador e multiplexador, o serviço de loggers, o
//! `LogService` e as funções auxiliares (`parseLogLevel`, `LogLevelToString`, `getLogLevel`...).
//!
//! Decisões de tradução:
//! - **`ILogger` é o trait [`Logger`].** Os argumentos `...args: unknown[]` são `&[LogArg]`: um valor
//!   dinâmico ([`Dynamic`]) ou um [`Error`] (o original deixa o `Error` passar por `unknown` e o `format`
//!   o reconhece com `instanceof Error`; um [`Dynamic`] não carrega o `Error` do porte). `message: string
//!   | Error` é [`LogMessage`]. Os métodos de conveniência de uma mensagem só (`logger.info("texto")`)
//!   são os métodos comuns do trait, e as variantes com argumentos terminam em `_args`.
//! - **`ILogService` é o trait [`LogService`]**, um [`Logger`] qualquer (impl em bloco): `LogServiceRef`
//!   é o handle compartilhado que o resto do `je-platform` recebe.
//! - **Classes abstratas** (`AbstractLogger`, `AbstractMessageLogger`, `AbstractLoggerService`) viram
//!   composição: [`AbstractLogger`] é o estado de nível e evento que os loggers embutem, o
//!   [`MessageLogger`] recebe o método abstrato `log` como um [`MessageSink`], e o
//!   [`AbstractLoggerService`] recebe o `doCreateLogger` como uma função. A macro [`delegate_logger!`]
//!   serve ao tipo que "herda" de um logger e quer sobrescrever só um método.
//! - **`console`**: o [`ConsoleLogger`] escreve no stdout/stderr; os formatos `%c` (CSS) do navegador
//!   não existem num terminal e o texto sai sem eles. O `registerDevConsoleLogForwarder` (troca os métodos
//!   do `console` global) não tem equivalente e não foi portado; `isDevConsoleLogForwardingEnabled` é
//!   `false` no original e aqui.
//! - **`CONTEXT_LOG_LEVEL`** (`RawContextKey`) e `LogLevelToLocalizedString` (`ILocalizedString`) dependem
//!   de `contextkey` e `action`, que não estão portados; ficam de fora.

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use je_base::date::to_local_iso_string;
use je_base::date::local_time_zone;
use je_base::dynamic::{Dynamic, date_to_iso_string, number_to_js_string};
use je_base::error_message::{to_error_message, to_error_message_of};
use je_base::errors::Error;
use je_base::event::{Emitter, Event};
use je_base::hash;
use je_base::lifecycle::{Disposable, IDisposable};
use je_base::map::ResourceMap;
use je_base::objects::safe_stringify;
use je_base::runtime;
use je_base::uri::URI;
use je_instantiation::service_id;

pub mod file_log;
pub mod node;

/// `LogLevel` de `log.ts` (os valores numéricos são os do original).
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum LogLevel {
    Off,
    Trace,
    Debug,
    Info,
    Warning,
    Error,
}

pub const DEFAULT_LOG_LEVEL: LogLevel = LogLevel::Info;

/// `isDevConsoleLogForwardingEnabled`.
pub const IS_DEV_CONSOLE_LOG_FORWARDING_ENABLED: bool = false;

/// `canLog(loggerLevel, messageLevel)`.
pub fn can_log(logger_level: LogLevel, message_level: LogLevel) -> bool {
    logger_level != LogLevel::Off && logger_level <= message_level
}

/// O `message: string | Error` de `warn` e `error`.
#[derive(Clone, Debug, PartialEq)]
pub enum LogMessage {
    Text(String),
    Error(Error),
}

impl From<&str> for LogMessage {
    fn from(text: &str) -> Self {
        Self::Text(text.to_owned())
    }
}

impl From<String> for LogMessage {
    fn from(text: String) -> Self {
        Self::Text(text)
    }
}

impl From<&String> for LogMessage {
    fn from(text: &String) -> Self {
        Self::Text(text.clone())
    }
}

impl From<Error> for LogMessage {
    fn from(error: Error) -> Self {
        Self::Error(error)
    }
}

impl From<&Error> for LogMessage {
    fn from(error: &Error) -> Self {
        Self::Error(error.clone())
    }
}

/// Um elemento de `...args: unknown[]`.
#[derive(Clone, Debug)]
pub enum LogArg {
    Value(Dynamic),
    Error(Error),
}

impl From<Dynamic> for LogArg {
    fn from(value: Dynamic) -> Self {
        Self::Value(value)
    }
}

impl From<&Dynamic> for LogArg {
    fn from(value: &Dynamic) -> Self {
        Self::Value(value.clone())
    }
}

impl From<Error> for LogArg {
    fn from(error: Error) -> Self {
        Self::Error(error)
    }
}

impl From<&Error> for LogArg {
    fn from(error: &Error) -> Self {
        Self::Error(error.clone())
    }
}

impl From<&str> for LogArg {
    fn from(text: &str) -> Self {
        Self::Value(Dynamic::string(text))
    }
}

impl From<f64> for LogArg {
    fn from(value: f64) -> Self {
        Self::Value(Dynamic::Number(value))
    }
}

impl From<i32> for LogArg {
    fn from(value: i32) -> Self {
        Self::Value(Dynamic::Number(f64::from(value)))
    }
}

impl LogArg {
    /// O argumento como valor dinâmico (um `Error` vira o objeto com `name`, `message` e `stack`).
    pub fn to_dynamic(&self) -> Dynamic {
        match self {
            Self::Value(value) => value.clone(),
            Self::Error(error) => je_base::error_message::error_to_dynamic(error),
        }
    }
}

impl PartialEq for LogArg {
    fn eq(&self, other: &Self) -> bool {
        match (self, other) {
            (Self::Value(a), Self::Value(b)) => a == b,
            (Self::Error(a), Self::Error(b)) => a == b,
            _ => false,
        }
    }
}

/// `ILogger`.
pub trait Logger: IDisposable {
    fn on_did_change_log_level(&self) -> Event<LogLevel>;
    fn get_level(&self) -> LogLevel;
    fn set_level(&self, level: LogLevel);

    fn trace_args(&self, message: &str, args: &[LogArg]);
    fn debug_args(&self, message: &str, args: &[LogArg]);
    fn info_args(&self, message: &str, args: &[LogArg]);
    fn warn_args(&self, message: &LogMessage, args: &[LogArg]);
    fn error_args(&self, message: &LogMessage, args: &[LogArg]);

    /// An operation to flush the contents. Can be synchronous.
    fn flush(&self);

    fn trace(&self, message: &str) {
        self.trace_args(message, &[]);
    }

    fn debug(&self, message: &str) {
        self.debug_args(message, &[]);
    }

    fn info(&self, message: &str) {
        self.info_args(message, &[]);
    }

    fn warn(&self, message: &str) {
        self.warn_args(&LogMessage::from(message), &[]);
    }

    fn error(&self, message: &str) {
        self.error_args(&LogMessage::from(message), &[]);
    }
}

/// `ILogService`: um [`Logger`] qualquer.
pub trait LogService: Logger {}

impl<T: Logger + ?Sized> LogService for T {}

service_id!(pub I_LOG_SERVICE: dyn LogService = "logService");

/// O handle compartilhado de um [`Logger`].
pub type LoggerRef = Rc<dyn Logger>;

/// O handle compartilhado do serviço de log.
pub type LogServiceRef = Rc<dyn LogService>;

/// `log(logger, level, message)`.
pub fn log(logger: &dyn Logger, level: LogLevel, message: &str) {
    match level {
        LogLevel::Trace => logger.trace(message),
        LogLevel::Debug => logger.debug(message),
        LogLevel::Info => logger.info(message),
        LogLevel::Warning => logger.warn(message),
        LogLevel::Error => logger.error(message),
        LogLevel::Off => {}
    }
}

/// Um valor `unknown` que chega ao tratador de erros: um `Error` nativo (o do porte) ou qualquer outro
/// valor (texto, objeto serializado, lista de erros).
#[derive(Clone, Debug)]
pub enum UnexpectedError {
    Native(Error),
    Value(Dynamic),
}

impl UnexpectedError {
    fn message(&self) -> String {
        match self {
            Self::Native(error) => to_error_message_of(error, true),
            Self::Value(value) => to_error_message(value, true),
        }
    }
}

/// `createUnexpectedErrorHandler(logger)`: suprime erros inesperados idênticos por um segundo.
pub fn create_unexpected_error_handler(logger: LoggerRef) -> impl Fn(&UnexpectedError) {
    let previous_message: RefCell<Option<String>> = RefCell::new(None);
    let previous_time = Cell::new(0u64);

    move |error| {
        let message = error.message();
        if message.is_empty() {
            return;
        }

        let now = runtime::date_now();
        if previous_message.borrow().as_deref() == Some(message.as_str()) && now - previous_time.get() <= 1000 {
            return;
        }

        previous_time.set(now);
        *previous_message.borrow_mut() = Some(message.clone());

        // Keep native errors for debugger source mapping, but retain the message fallback for stackless errors.
        match error {
            UnexpectedError::Native(native) if native.stack.as_deref().is_some_and(|stack| !stack.is_empty()) => {
                logger.error_args(&LogMessage::Error(native.clone()), &[]);
            }
            _ => logger.error_args(&LogMessage::Text(message), &[]),
        }
    }
}

/// `String(value)` de um valor dinâmico dentro de `format`.
fn stringify_arg(value: &Dynamic) -> String {
    match value {
        Dynamic::Undefined => "undefined".to_owned(),
        Dynamic::Null => "null".to_owned(),
        Dynamic::Bool(b) => b.to_string(),
        Dynamic::Number(n) => number_to_js_string(*n),
        Dynamic::BigInt(n) => n.to_string(),
        Dynamic::String(s) => s.to_string_lossy(),
        Dynamic::Array(_) | Dynamic::Object(_) | Dynamic::Date(_) | Dynamic::RegExp(_) | Dynamic::TypedArray(_) => {
            // `typeof a === 'object'` e `JSON.stringify(a)`; se falhar, `String(a)`.
            if let Dynamic::Date(date) = value {
                return date_to_iso_string(date.time_value()).map_or_else(|| "null".to_owned(), |iso| format!("\"{iso}\""));
            }
            safe_stringify(value).unwrap_or_else(|| "[object Object]".to_owned())
        }
        Dynamic::Function(function) => format!("function {}", function.name()),
    }
}

/// `format(args, verbose)`: junta os argumentos com um espaço; erros viram a mensagem (com a pilha se
/// `verbose`) e objetos viram JSON.
pub fn format(args: &[LogArg], verbose: bool) -> String {
    let mut result = String::new();

    for (i, arg) in args.iter().enumerate() {
        if i > 0 {
            result.push(' ');
        }
        match arg {
            LogArg::Error(error) => result.push_str(&to_error_message_of(error, verbose)),
            LogArg::Value(value) => result.push_str(&stringify_arg(value)),
        }
    }

    result
}

/// O `[message, ...args]` que o `format` recebe.
fn with_message(message: &str, args: &[LogArg]) -> Vec<LogArg> {
    std::iter::once(LogArg::from(message)).chain(args.iter().cloned()).collect()
}

/// `now()` do `log.ts`: `new Date().toISOString()`.
fn now() -> String {
    date_to_iso_string(runtime::date_now() as f64).unwrap_or_default()
}

/// O estado de nível e evento de `AbstractLogger`: quem "herda" guarda um e delega.
pub struct AbstractLogger {
    level: Rc<Cell<LogLevel>>,
    on_did_change_log_level: Rc<Emitter<LogLevel>>,
    disposed: Cell<bool>,
}

impl AbstractLogger {
    pub fn new() -> Self {
        Self { level: Rc::new(Cell::new(DEFAULT_LOG_LEVEL)), on_did_change_log_level: Emitter::new(), disposed: Cell::new(false) }
    }

    pub fn on_did_change_log_level(&self) -> Event<LogLevel> {
        self.on_did_change_log_level.event()
    }

    pub fn set_level(&self, level: LogLevel) {
        if self.level.get() != level {
            self.level.set(level);
            self.on_did_change_log_level.fire(level);
        }
    }

    pub fn get_level(&self) -> LogLevel {
        self.level.get()
    }

    /// O nível compartilhado, para um [`MessageSink`] que precisa lê-lo (`this.getLevel()` da subclasse).
    pub fn level_handle(&self) -> Rc<Cell<LogLevel>> {
        self.level.clone()
    }

    /// `checkLogLevel`.
    pub fn check_log_level(&self, level: LogLevel) -> bool {
        can_log(self.level.get(), level)
    }

    /// `canLog`: um logger descartado não escreve.
    pub fn can_log(&self, level: LogLevel) -> bool {
        if self.disposed.get() {
            return false;
        }
        self.check_log_level(level)
    }

    pub fn is_disposed(&self) -> bool {
        self.disposed.get()
    }
}

impl Default for AbstractLogger {
    fn default() -> Self {
        Self::new()
    }
}

impl IDisposable for AbstractLogger {
    fn dispose(&self) {
        if !self.disposed.replace(true) {
            self.on_did_change_log_level.dispose();
        }
    }
}

/// O método abstrato `log(level, message)` de `AbstractMessageLogger`.
pub trait MessageSink {
    fn log(&self, level: LogLevel, message: &str);

    fn flush(&self) {}

    /// O que o `dispose` da subclasse faz antes de chamar o `super.dispose()`.
    fn dispose(&self) {}
}

/// `AbstractMessageLogger`: formata a mensagem e entrega ao [`MessageSink`].
pub struct MessageLogger<S: MessageSink> {
    base: AbstractLogger,
    log_always: bool,
    pub sink: S,
}

impl<S: MessageSink> MessageLogger<S> {
    pub fn new(sink: S) -> Rc<Self> {
        Self::with_log_always(sink, false)
    }

    pub fn with_log_always(sink: S, log_always: bool) -> Rc<Self> {
        Rc::new(Self { base: AbstractLogger::new(), log_always, sink })
    }

    /// Como [`MessageLogger::new`], para o sink que precisa do nível do logger.
    pub fn build(make_sink: impl FnOnce(Rc<Cell<LogLevel>>) -> S) -> Rc<Self> {
        let base = AbstractLogger::new();
        let sink = make_sink(base.level_handle());
        Rc::new(Self { base, log_always: false, sink })
    }

    fn can_log(&self, level: LogLevel) -> bool {
        if self.base.is_disposed() {
            return false;
        }
        self.log_always || self.base.check_log_level(level)
    }
}

impl<S: MessageSink> Logger for MessageLogger<S> {
    fn on_did_change_log_level(&self) -> Event<LogLevel> {
        self.base.on_did_change_log_level()
    }

    fn get_level(&self) -> LogLevel {
        self.base.get_level()
    }

    fn set_level(&self, level: LogLevel) {
        self.base.set_level(level);
    }

    fn trace_args(&self, message: &str, args: &[LogArg]) {
        if self.can_log(LogLevel::Trace) {
            self.sink.log(LogLevel::Trace, &format(&with_message(message, args), true));
        }
    }

    fn debug_args(&self, message: &str, args: &[LogArg]) {
        if self.can_log(LogLevel::Debug) {
            self.sink.log(LogLevel::Debug, &format(&with_message(message, args), false));
        }
    }

    fn info_args(&self, message: &str, args: &[LogArg]) {
        if self.can_log(LogLevel::Info) {
            self.sink.log(LogLevel::Info, &format(&with_message(message, args), false));
        }
    }

    fn warn_args(&self, message: &LogMessage, args: &[LogArg]) {
        if self.can_log(LogLevel::Warning) {
            let text = match message {
                LogMessage::Text(text) => text.clone(),
                LogMessage::Error(error) => error.message.clone(),
            };
            self.sink.log(LogLevel::Warning, &format(&with_message(&text, args), false));
        }
    }

    fn error_args(&self, message: &LogMessage, args: &[LogArg]) {
        if self.can_log(LogLevel::Error) {
            match message {
                // `array[0] = message.stack`: o `undefined` de um erro sem pilha sai como texto.
                LogMessage::Error(error) => {
                    let stack = error.stack.clone().unwrap_or_else(|| "undefined".to_owned());
                    self.sink.log(LogLevel::Error, &format(&with_message(&stack, args), false));
                }
                LogMessage::Text(text) => self.sink.log(LogLevel::Error, &format(&with_message(text, args), false)),
            }
        }
    }

    fn flush(&self) {
        self.sink.flush();
    }
}

impl<S: MessageSink> IDisposable for MessageLogger<S> {
    fn dispose(&self) {
        self.sink.dispose();
        self.base.dispose();
    }
}

/// Implementa [`Logger`] e `IDisposable` para um tipo que guarda outro logger num campo e quer sobrescrever
/// só o `error`: o `class X extends AbstractMessageLogger { override error(...) }` do original. Sem o
/// terceiro argumento, o `error` também delega; com ele, `error_args: |this, message, args| corpo` o
/// substitui (o `super.error(...)` é `this.<campo>.error_args(message, args)`).
#[macro_export]
macro_rules! delegate_logger {
    ($ty:ty, $field:ident) => {
        $crate::delegate_logger!($ty, $field, error_args: |this, message, args| this.$field.error_args(message, args));
    };
    ($ty:ty, $field:ident, error_args: |$this:ident, $message:ident, $args:ident| $body:expr) => {
        impl $crate::log::Logger for $ty {
            fn on_did_change_log_level(&self) -> $crate::__je_event::Event<$crate::log::LogLevel> {
                self.$field.on_did_change_log_level()
            }
            fn get_level(&self) -> $crate::log::LogLevel {
                self.$field.get_level()
            }
            fn set_level(&self, level: $crate::log::LogLevel) {
                self.$field.set_level(level);
            }
            fn trace_args(&self, message: &str, args: &[$crate::log::LogArg]) {
                self.$field.trace_args(message, args);
            }
            fn debug_args(&self, message: &str, args: &[$crate::log::LogArg]) {
                self.$field.debug_args(message, args);
            }
            fn info_args(&self, message: &str, args: &[$crate::log::LogArg]) {
                self.$field.info_args(message, args);
            }
            fn warn_args(&self, message: &$crate::log::LogMessage, args: &[$crate::log::LogArg]) {
                self.$field.warn_args(message, args);
            }
            fn error_args(&self, $message: &$crate::log::LogMessage, $args: &[$crate::log::LogArg]) {
                let $this = self;
                $body
            }
            fn flush(&self) {
                self.$field.flush();
            }
        }

        impl $crate::__je_lifecycle::IDisposable for $ty {
            fn dispose(&self) {
                $crate::__je_lifecycle::IDisposable::dispose(&self.$field);
            }
        }
    };
}

/// `console.log`/`warn`/`error` das classes de console.
fn log_to_console(method: ConsoleMethod, parts: &[String]) {
    let line = parts.join(" ");
    match method {
        ConsoleMethod::Log => println!("{line}"),
        ConsoleMethod::Warn | ConsoleMethod::Error => eprintln!("{line}"),
    }
}

#[derive(Clone, Copy)]
enum ConsoleMethod {
    Log,
    Warn,
    Error,
}

fn console_parts(prefix: Option<String>, message: &LogMessage, args: &[LogArg]) -> Vec<String> {
    let mut parts: Vec<String> = prefix.into_iter().collect();
    parts.push(match message {
        LogMessage::Text(text) => text.clone(),
        LogMessage::Error(error) => error.stack.clone().unwrap_or_else(|| error.to_string()),
    });
    parts.extend(args.iter().map(|arg| format(std::slice::from_ref(arg), false)));
    parts
}

/// `ConsoleMainLogger`.
pub struct ConsoleMainLogger {
    base: AbstractLogger,
    use_colors: bool,
}

impl ConsoleMainLogger {
    pub fn new(log_level: LogLevel) -> Rc<Self> {
        let logger = Self { base: AbstractLogger::new(), use_colors: !cfg!(windows) };
        logger.base.set_level(log_level);
        Rc::new(logger)
    }

    fn emit(&self, level: LogLevel, color: &str, method: ConsoleMethod, message: &LogMessage, args: &[LogArg]) {
        if self.base.can_log(level) {
            let prefix = if self.use_colors { format!("\x1b[{color}m[main {}]\x1b[0m", now()) } else { format!("[main {}]", now()) };
            log_to_console(method, &console_parts(Some(prefix), message, args));
        }
    }
}

macro_rules! simple_logger_impl {
    ($ty:ty, $emit:expr) => {
        impl Logger for $ty {
            fn on_did_change_log_level(&self) -> Event<LogLevel> {
                self.base.on_did_change_log_level()
            }
            fn get_level(&self) -> LogLevel {
                self.base.get_level()
            }
            fn set_level(&self, level: LogLevel) {
                self.base.set_level(level);
            }
            fn trace_args(&self, message: &str, args: &[LogArg]) {
                $emit(self, LogLevel::Trace, &LogMessage::from(message), args);
            }
            fn debug_args(&self, message: &str, args: &[LogArg]) {
                $emit(self, LogLevel::Debug, &LogMessage::from(message), args);
            }
            fn info_args(&self, message: &str, args: &[LogArg]) {
                $emit(self, LogLevel::Info, &LogMessage::from(message), args);
            }
            fn warn_args(&self, message: &LogMessage, args: &[LogArg]) {
                $emit(self, LogLevel::Warning, message, args);
            }
            fn error_args(&self, message: &LogMessage, args: &[LogArg]) {
                $emit(self, LogLevel::Error, message, args);
            }
            fn flush(&self) {
                // noop
            }
        }

        impl IDisposable for $ty {
            fn dispose(&self) {
                self.base.dispose();
            }
        }
    };
}

simple_logger_impl!(ConsoleMainLogger, |this: &ConsoleMainLogger, level, message: &LogMessage, args: &[LogArg]| {
    match level {
        // 90 é cinza, 93 amarelo e 91 vermelho.
        LogLevel::Warning => this.emit(level, "93", ConsoleMethod::Warn, message, args),
        LogLevel::Error => this.emit(level, "91", ConsoleMethod::Error, message, args),
        _ => this.emit(level, "90", ConsoleMethod::Log, message, args),
    }
});

/// `ConsoleLogger`.
pub struct ConsoleLogger {
    base: AbstractLogger,
    use_colors: bool,
}

impl ConsoleLogger {
    pub fn new(log_level: LogLevel, use_colors: bool) -> Rc<Self> {
        let logger = Self { base: AbstractLogger::new(), use_colors };
        logger.base.set_level(log_level);
        Rc::new(logger)
    }

    fn emit(&self, level: LogLevel, label: &str, method: ConsoleMethod, message: &LogMessage, args: &[LogArg]) {
        if self.base.can_log(level) {
            let prefix = self.use_colors.then(|| label.to_owned());
            log_to_console(method, &console_parts(prefix, message, args));
        }
    }
}

simple_logger_impl!(ConsoleLogger, |this: &ConsoleLogger, level, message: &LogMessage, args: &[LogArg]| {
    match level {
        LogLevel::Trace => this.emit(level, "TRACE", ConsoleMethod::Log, message, args),
        LogLevel::Debug => this.emit(level, "DEBUG", ConsoleMethod::Log, message, args),
        LogLevel::Info => this.emit(level, " INFO", ConsoleMethod::Log, message, args),
        // Sem cores o `warn` cai no `console.log`, como no original.
        LogLevel::Warning if this.use_colors => this.emit(level, " WARN", ConsoleMethod::Warn, message, args),
        LogLevel::Warning => this.emit(level, "", ConsoleMethod::Log, message, args),
        _ => this.emit(level, "  ERR", ConsoleMethod::Error, message, args),
    }
});

/// O `adapter` de `AdapterLogger`: `{ log: (logLevel, args) => void }`.
pub trait LogAdapter {
    fn log(&self, level: LogLevel, args: &[LogArg]);
}

impl<F: Fn(LogLevel, &[LogArg])> LogAdapter for F {
    fn log(&self, level: LogLevel, args: &[LogArg]) {
        self(level, args);
    }
}

/// `AdapterLogger`.
pub struct AdapterLogger {
    base: AbstractLogger,
    adapter: Box<dyn LogAdapter>,
}

impl AdapterLogger {
    pub fn new(adapter: impl LogAdapter + 'static, log_level: LogLevel) -> Rc<Self> {
        let logger = Self { base: AbstractLogger::new(), adapter: Box::new(adapter) };
        logger.base.set_level(log_level);
        Rc::new(logger)
    }

    fn extract_message(&self, message: &LogMessage) -> String {
        match message {
            LogMessage::Text(text) => text.clone(),
            LogMessage::Error(error) => to_error_message_of(error, self.base.can_log(LogLevel::Trace)),
        }
    }

    fn emit(&self, level: LogLevel, message: String, args: &[LogArg]) {
        if self.base.can_log(level) {
            let all: Vec<LogArg> = std::iter::once(LogArg::from(message.as_str())).chain(args.iter().cloned()).collect();
            self.adapter.log(level, &all);
        }
    }
}

impl Logger for AdapterLogger {
    fn on_did_change_log_level(&self) -> Event<LogLevel> {
        self.base.on_did_change_log_level()
    }

    fn get_level(&self) -> LogLevel {
        self.base.get_level()
    }

    fn set_level(&self, level: LogLevel) {
        self.base.set_level(level);
    }

    fn trace_args(&self, message: &str, args: &[LogArg]) {
        self.emit(LogLevel::Trace, message.to_owned(), args);
    }

    fn debug_args(&self, message: &str, args: &[LogArg]) {
        self.emit(LogLevel::Debug, message.to_owned(), args);
    }

    fn info_args(&self, message: &str, args: &[LogArg]) {
        self.emit(LogLevel::Info, message.to_owned(), args);
    }

    fn warn_args(&self, message: &LogMessage, args: &[LogArg]) {
        if self.base.can_log(LogLevel::Warning) {
            let text = self.extract_message(message);
            self.emit(LogLevel::Warning, text, args);
        }
    }

    fn error_args(&self, message: &LogMessage, args: &[LogArg]) {
        if self.base.can_log(LogLevel::Error) {
            let text = match message {
                LogMessage::Text(text) => text.clone(),
                LogMessage::Error(error) => to_error_message_of(error, true),
            };
            self.emit(LogLevel::Error, text, args);
        }
    }

    fn flush(&self) {
        // noop
    }
}

impl IDisposable for AdapterLogger {
    fn dispose(&self) {
        self.base.dispose();
    }
}

/// `MultiplexLogger`.
pub struct MultiplexLogger {
    base: AbstractLogger,
    loggers: Vec<LoggerRef>,
}

impl MultiplexLogger {
    pub fn new(loggers: Vec<LoggerRef>) -> Rc<Self> {
        let logger = Self { base: AbstractLogger::new(), loggers };
        if let Some(first) = logger.loggers.first() {
            logger.base.set_level(first.get_level());
        }
        Rc::new(logger)
    }
}

impl Logger for MultiplexLogger {
    fn on_did_change_log_level(&self) -> Event<LogLevel> {
        self.base.on_did_change_log_level()
    }

    fn get_level(&self) -> LogLevel {
        self.base.get_level()
    }

    fn set_level(&self, level: LogLevel) {
        for logger in &self.loggers {
            logger.set_level(level);
        }
        self.base.set_level(level);
    }

    fn trace_args(&self, message: &str, args: &[LogArg]) {
        for logger in &self.loggers {
            logger.trace_args(message, args);
        }
    }

    fn debug_args(&self, message: &str, args: &[LogArg]) {
        for logger in &self.loggers {
            logger.debug_args(message, args);
        }
    }

    fn info_args(&self, message: &str, args: &[LogArg]) {
        for logger in &self.loggers {
            logger.info_args(message, args);
        }
    }

    fn warn_args(&self, message: &LogMessage, args: &[LogArg]) {
        for logger in &self.loggers {
            logger.warn_args(message, args);
        }
    }

    fn error_args(&self, message: &LogMessage, args: &[LogArg]) {
        for logger in &self.loggers {
            logger.error_args(message, args);
        }
    }

    fn flush(&self) {
        for logger in &self.loggers {
            logger.flush();
        }
    }
}

impl IDisposable for MultiplexLogger {
    fn dispose(&self) {
        for logger in &self.loggers {
            logger.dispose();
        }
        self.base.dispose();
    }
}

/// `NullLogger`.
pub struct NullLogger {
    on_did_change_log_level: Rc<Emitter<LogLevel>>,
}

impl NullLogger {
    pub fn new() -> Self {
        Self { on_did_change_log_level: Emitter::new() }
    }
}

impl Default for NullLogger {
    fn default() -> Self {
        Self::new()
    }
}

impl Logger for NullLogger {
    fn on_did_change_log_level(&self) -> Event<LogLevel> {
        self.on_did_change_log_level.event()
    }

    fn get_level(&self) -> LogLevel {
        LogLevel::Info
    }

    fn set_level(&self, _level: LogLevel) {}
    fn trace_args(&self, _message: &str, _args: &[LogArg]) {}
    fn debug_args(&self, _message: &str, _args: &[LogArg]) {}
    fn info_args(&self, _message: &str, _args: &[LogArg]) {}
    fn warn_args(&self, _message: &LogMessage, _args: &[LogArg]) {}
    fn error_args(&self, _message: &LogMessage, _args: &[LogArg]) {}
    fn flush(&self) {}
}

impl IDisposable for NullLogger {
    fn dispose(&self) {
        self.on_did_change_log_level.dispose();
    }
}

/// `NullLogService`: descarta tudo.
#[derive(Clone, Copy, Debug, Default)]
pub struct NullLogService;

impl NullLogService {
    pub fn shared() -> LogServiceRef {
        Rc::new(Self)
    }
}

impl Logger for NullLogService {
    fn on_did_change_log_level(&self) -> Event<LogLevel> {
        Event::none()
    }

    fn get_level(&self) -> LogLevel {
        LogLevel::Info
    }

    fn set_level(&self, _level: LogLevel) {}
    fn trace_args(&self, _message: &str, _args: &[LogArg]) {}
    fn debug_args(&self, _message: &str, _args: &[LogArg]) {}
    fn info_args(&self, _message: &str, _args: &[LogArg]) {}
    fn warn_args(&self, _message: &LogMessage, _args: &[LogArg]) {}
    fn error_args(&self, _message: &LogMessage, _args: &[LogArg]) {}
    fn flush(&self) {}
}

impl IDisposable for NullLogService {
    fn dispose(&self) {}
}

/// `LogService`: o logger primário mais os demais, atrás de um multiplexador.
pub struct DefaultLogService {
    base: Rc<Disposable>,
    logger: Rc<MultiplexLogger>,
}

impl DefaultLogService {
    pub fn new(primary_logger: LoggerRef, other_loggers: Vec<LoggerRef>) -> Rc<Self> {
        let mut loggers = vec![primary_logger.clone()];
        loggers.extend(other_loggers);
        let logger = MultiplexLogger::new(loggers);
        let base = Disposable::new();
        let service = Rc::new(Self { base, logger });
        let weak = Rc::downgrade(&service);
        service.base.register(primary_logger.on_did_change_log_level().listen(move |level| {
            if let Some(service) = weak.upgrade() {
                service.set_level(*level);
            }
        }));
        service
    }
}

impl Logger for DefaultLogService {
    fn on_did_change_log_level(&self) -> Event<LogLevel> {
        self.logger.on_did_change_log_level()
    }

    fn get_level(&self) -> LogLevel {
        self.logger.get_level()
    }

    fn set_level(&self, level: LogLevel) {
        self.logger.set_level(level);
    }

    fn trace_args(&self, message: &str, args: &[LogArg]) {
        self.logger.trace_args(message, args);
    }

    fn debug_args(&self, message: &str, args: &[LogArg]) {
        self.logger.debug_args(message, args);
    }

    fn info_args(&self, message: &str, args: &[LogArg]) {
        self.logger.info_args(message, args);
    }

    fn warn_args(&self, message: &LogMessage, args: &[LogArg]) {
        self.logger.warn_args(message, args);
    }

    fn error_args(&self, message: &LogMessage, args: &[LogArg]) {
        self.logger.error_args(message, args);
    }

    fn flush(&self) {
        self.logger.flush();
    }
}

impl IDisposable for DefaultLogService {
    fn dispose(&self) {
        self.base.dispose();
    }
}

// region: ILoggerService

/// `LoggerGroup`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LoggerGroup {
    pub id: String,
    pub name: String,
}

/// `logLevel?: 'always' | LogLevel` de `ILoggerOptions`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LoggerLogLevel {
    Always,
    Level(LogLevel),
}

/// `ILoggerOptions`.
#[derive(Clone, Debug, Default)]
pub struct LoggerOptions {
    /// Id of the logger.
    pub id: Option<String>,
    /// Name of the logger.
    pub name: Option<String>,
    /// Do not create rotating files if max size exceeds.
    pub donot_rotate: bool,
    /// Do not use formatters.
    pub donot_use_formatters: bool,
    /// When to log. Set to `always` to log always.
    pub log_level: Option<LoggerLogLevel>,
    /// Whether the log should be hidden from the user.
    pub hidden: Option<bool>,
    /// Condition which must be true to show this logger.
    pub when: Option<String>,
    /// Id of the extension that created this logger.
    pub extension_id: Option<String>,
    /// Group of the logger.
    pub group: Option<LoggerGroup>,
}

/// `ILoggerResource`.
#[derive(Clone, Debug, PartialEq)]
pub struct LoggerResource {
    pub resource: URI,
    pub id: String,
    pub name: Option<String>,
    pub log_level: Option<LogLevel>,
    pub hidden: Option<bool>,
    pub when: Option<String>,
    pub extension_id: Option<String>,
    pub group: Option<LoggerGroup>,
}

impl LoggerResource {
    pub fn new(resource: URI, id: impl Into<String>) -> Self {
        Self { resource, id: id.into(), name: None, log_level: None, hidden: None, when: None, extension_id: None, group: None }
    }
}

/// `DidChangeLoggersEvent`.
#[derive(Clone, Debug)]
pub struct DidChangeLoggersEvent {
    pub added: Vec<LoggerResource>,
    pub removed: Vec<LoggerResource>,
}

/// O `URI | string` que o `ILoggerService` aceita para identificar um logger.
#[derive(Clone, Debug)]
pub enum ResourceOrId {
    Resource(URI),
    Id(String),
}

impl From<&str> for ResourceOrId {
    fn from(id: &str) -> Self {
        Self::Id(id.to_owned())
    }
}

impl From<URI> for ResourceOrId {
    fn from(resource: URI) -> Self {
        Self::Resource(resource)
    }
}

impl From<&URI> for ResourceOrId {
    fn from(resource: &URI) -> Self {
        Self::Resource(resource.clone())
    }
}

/// O `LogLevel | [URI, LogLevel]` de `ILoggerService.onDidChangeLogLevel`.
#[derive(Clone, Debug, PartialEq)]
pub enum LoggerServiceLogLevelChange {
    Default(LogLevel),
    Logger(URI, LogLevel),
}

/// `ILoggerService`.
pub trait LoggerService: IDisposable {
    /// `createLogger`: cria o logger do recurso (ou o `id` na pasta de logs), ou devolve o que já existe,
    /// e o registra no serviço.
    fn create_logger(&self, id_or_resource: ResourceOrId, options: Option<LoggerOptions>) -> LoggerRef;
    /// `getLogger`: um logger existente, se houver.
    fn get_logger(&self, resource_or_id: &ResourceOrId) -> Option<LoggerRef>;
    /// `onDidChangeLogLevel`.
    fn on_did_change_log_level(&self) -> Event<LoggerServiceLogLevelChange>;
    /// `setLogLevel(level)`: o nível padrão.
    fn set_default_log_level(&self, level: LogLevel);
    /// `setLogLevel(resource, level)`: o nível de um logger.
    fn set_log_level(&self, resource: &URI, level: LogLevel);
    /// `getLogLevel(resource?)`.
    fn get_log_level(&self, resource: Option<&URI>) -> LogLevel;
    /// `onDidChangeVisibility`.
    fn on_did_change_visibility(&self) -> Event<(URI, bool)>;
    /// `setVisibility`.
    fn set_visibility(&self, resource_or_id: &ResourceOrId, visible: bool);
    /// `onDidChangeLoggers`.
    fn on_did_change_loggers(&self) -> Event<DidChangeLoggersEvent>;
    /// `registerLogger`.
    fn register_logger(&self, resource: LoggerResource);
    /// `deregisterLogger`.
    fn deregister_logger(&self, id_or_resource: &ResourceOrId);
    /// `getRegisteredLoggers`.
    fn get_registered_loggers(&self) -> Vec<LoggerResource>;
    /// `getRegisteredLogger`.
    fn get_registered_logger(&self, resource: &URI) -> Option<LoggerResource>;
}

#[derive(Clone)]
struct LoggerEntry {
    logger: Option<LoggerRef>,
    info: LoggerResource,
}

/// O método abstrato `doCreateLogger` de `AbstractLoggerService`.
pub type CreateLoggerFn = Rc<dyn Fn(&URI, LogLevel, &LoggerOptions) -> LoggerRef>;

/// `JSON.stringify`-livre: o `hash(...).toString(16)` do original.
fn hash_hex(text: &str) -> String {
    let value = hash::hash(&Dynamic::string(text));
    if value < 0 { format!("-{:x}", i64::from(value).unsigned_abs()) } else { format!("{value:x}") }
}

/// `AbstractLoggerService`.
pub struct AbstractLoggerService {
    base: Rc<Disposable>,
    log_level: Cell<LogLevel>,
    logs_home: URI,
    loggers: RefCell<ResourceMap<LoggerEntry>>,
    do_create_logger: CreateLoggerFn,
    on_did_change_loggers: Rc<Emitter<DidChangeLoggersEvent>>,
    on_did_change_log_level: Rc<Emitter<LoggerServiceLogLevelChange>>,
    on_did_change_visibility: Rc<Emitter<(URI, bool)>>,
}

impl AbstractLoggerService {
    pub fn new(log_level: LogLevel, logs_home: URI, logger_resources: Vec<LoggerResource>, do_create_logger: CreateLoggerFn) -> Rc<Self> {
        let base = Disposable::new();
        let on_did_change_loggers = Emitter::new();
        let on_did_change_log_level = Emitter::new();
        let on_did_change_visibility = Emitter::new();
        base.register(on_did_change_loggers.clone());
        base.register(on_did_change_log_level.clone());
        base.register(on_did_change_visibility.clone());
        let mut loggers = ResourceMap::new();
        for logger_resource in logger_resources {
            loggers.set(logger_resource.resource.clone(), LoggerEntry { logger: None, info: logger_resource });
        }
        Rc::new(Self {
            base,
            log_level: Cell::new(log_level),
            logs_home,
            loggers: RefCell::new(loggers),
            do_create_logger,
            on_did_change_loggers,
            on_did_change_log_level,
            on_did_change_visibility,
        })
    }

    fn get_logger_entry(&self, resource_or_id: &ResourceOrId) -> Option<LoggerEntry> {
        match resource_or_id {
            ResourceOrId::Id(id) => self.loggers.borrow().values().find(|entry| &entry.info.id == id).cloned(),
            ResourceOrId::Resource(resource) => self.loggers.borrow().get(resource).cloned(),
        }
    }

    /// `toResource`.
    pub fn to_resource(&self, id_or_resource: &ResourceOrId) -> URI {
        match id_or_resource {
            ResourceOrId::Resource(resource) => resource.clone(),
            ResourceOrId::Id(id) => {
                let sanitized: String = id.chars().filter(|c| !matches!(c, '\\' | '/' | ':' | '*' | '?' | '"' | '<' | '>' | '|')).collect();
                crate::resources::join_path(&self.logs_home, &[&format!("{sanitized}.log")])
            }
        }
    }
}

impl LoggerService for AbstractLoggerService {
    fn create_logger(&self, id_or_resource: ResourceOrId, options: Option<LoggerOptions>) -> LoggerRef {
        let options = options.unwrap_or_default();
        let resource = self.to_resource(&id_or_resource);
        let id = match &id_or_resource {
            ResourceOrId::Id(id) => id.clone(),
            ResourceOrId::Resource(_) => options.id.clone().unwrap_or_else(|| hash_hex(&resource.to_string(false).to_string_lossy())),
        };
        let mut logger = self.loggers.borrow().get(&resource).and_then(|entry| entry.logger.clone());
        let log_level = match options.log_level {
            Some(LoggerLogLevel::Always) => Some(LogLevel::Trace),
            Some(LoggerLogLevel::Level(level)) => Some(level),
            None => None,
        };
        let logger = match logger.take() {
            Some(logger) => logger,
            None => {
                let level = log_level.unwrap_or_else(|| self.get_log_level(Some(&resource)));
                (self.do_create_logger)(&resource, level, &LoggerOptions { id: Some(id.clone()), ..options.clone() })
            }
        };
        let info = LoggerResource {
            resource: resource.clone(),
            id,
            log_level,
            name: options.name.clone(),
            hidden: options.hidden,
            group: options.group.clone(),
            extension_id: options.extension_id.clone(),
            when: options.when.clone(),
        };
        self.register_logger(info.clone());
        // TODO: @sandy081 Remove this once registerLogger can take ILogger
        self.loggers.borrow_mut().set(resource, LoggerEntry { logger: Some(logger.clone()), info });
        logger
    }

    fn get_logger(&self, resource_or_id: &ResourceOrId) -> Option<LoggerRef> {
        self.get_logger_entry(resource_or_id).and_then(|entry| entry.logger)
    }

    fn on_did_change_log_level(&self) -> Event<LoggerServiceLogLevelChange> {
        self.on_did_change_log_level.event()
    }

    fn set_default_log_level(&self, level: LogLevel) {
        self.log_level.set(level);
        let entries: Vec<LoggerEntry> = self.loggers.borrow().values().cloned().collect();
        for entry in entries {
            if entry.info.log_level.is_none()
                && let Some(logger) = &entry.logger
            {
                logger.set_level(level);
            }
        }
        self.on_did_change_log_level.fire(LoggerServiceLogLevelChange::Default(level));
    }

    fn set_log_level(&self, resource: &URI, level: LogLevel) {
        let entry = self.loggers.borrow().get(resource).cloned();
        if let Some(mut entry) = entry
            && Some(level) != entry.info.log_level
        {
            entry.info.log_level = if level == self.log_level.get() { None } else { Some(level) };
            if let Some(logger) = &entry.logger {
                logger.set_level(level);
            }
            self.loggers.borrow_mut().set(entry.info.resource.clone(), entry);
            self.on_did_change_log_level.fire(LoggerServiceLogLevelChange::Logger(resource.clone(), level));
        }
    }

    fn get_log_level(&self, resource: Option<&URI>) -> LogLevel {
        resource
            .and_then(|resource| self.loggers.borrow().get(resource).and_then(|entry| entry.info.log_level))
            .unwrap_or_else(|| self.log_level.get())
    }

    fn on_did_change_visibility(&self) -> Event<(URI, bool)> {
        self.on_did_change_visibility.event()
    }

    fn set_visibility(&self, resource_or_id: &ResourceOrId, visibility: bool) {
        if let Some(mut entry) = self.get_logger_entry(resource_or_id)
            && visibility != !entry.info.hidden.unwrap_or(false)
        {
            entry.info.hidden = Some(!visibility);
            let resource = entry.info.resource.clone();
            self.loggers.borrow_mut().set(resource.clone(), entry);
            self.on_did_change_visibility.fire((resource, visibility));
        }
    }

    fn on_did_change_loggers(&self) -> Event<DidChangeLoggersEvent> {
        self.on_did_change_loggers.event()
    }

    fn register_logger(&self, resource: LoggerResource) {
        let existing = self.loggers.borrow().get(&resource.resource).cloned();
        match existing {
            Some(existing) => {
                if existing.info.hidden != resource.hidden {
                    self.set_visibility(&ResourceOrId::Resource(resource.resource.clone()), !resource.hidden.unwrap_or(false));
                }
            }
            None => {
                self.loggers.borrow_mut().set(resource.resource.clone(), LoggerEntry { info: resource.clone(), logger: None });
                self.on_did_change_loggers.fire(DidChangeLoggersEvent { added: vec![resource], removed: Vec::new() });
            }
        }
    }

    fn deregister_logger(&self, id_or_resource: &ResourceOrId) {
        let resource = self.to_resource(id_or_resource);
        let existing = self.loggers.borrow().get(&resource).cloned();
        if let Some(existing) = existing {
            if let Some(logger) = &existing.logger {
                logger.dispose();
            }
            self.loggers.borrow_mut().delete(&resource);
            self.on_did_change_loggers.fire(DidChangeLoggersEvent { added: Vec::new(), removed: vec![existing.info] });
        }
    }

    fn get_registered_loggers(&self) -> Vec<LoggerResource> {
        self.loggers.borrow().values().map(|entry| entry.info.clone()).collect()
    }

    fn get_registered_logger(&self, resource: &URI) -> Option<LoggerResource> {
        self.loggers.borrow().get(resource).map(|entry| entry.info.clone())
    }
}

impl IDisposable for AbstractLoggerService {
    fn dispose(&self) {
        let entries: Vec<LoggerEntry> = self.loggers.borrow().values().cloned().collect();
        for entry in entries {
            if let Some(logger) = &entry.logger {
                logger.dispose();
            }
        }
        self.loggers.borrow_mut().clear();
        self.base.dispose();
    }
}

/// `NullLoggerService`.
pub fn null_logger_service() -> Rc<AbstractLoggerService> {
    AbstractLoggerService::new(
        LogLevel::Off,
        URI::parse("log:///log", false),
        Vec::new(),
        Rc::new(|_resource, _level, _options| -> LoggerRef { Rc::new(NullLogger::new()) }),
    )
}

// endregion

/// A parte de `IEnvironmentService` de que o `getLogLevel` precisa: o `--verbose` e o `--log` sem extensão.
pub trait LogLevelSource {
    fn verbose(&self) -> bool;
    fn log_level(&self) -> Option<String>;
}

/// `getLogLevel(environmentService)`.
pub fn get_log_level(environment_service: &dyn LogLevelSource) -> LogLevel {
    if environment_service.verbose() {
        return LogLevel::Trace;
    }
    if let Some(log_level) = environment_service.log_level()
        && let Some(log_level) = parse_log_level(&log_level.to_lowercase())
    {
        return log_level;
    }
    DEFAULT_LOG_LEVEL
}

/// `LogLevelToString`.
pub fn log_level_to_string(log_level: LogLevel) -> &'static str {
    match log_level {
        LogLevel::Trace => "trace",
        LogLevel::Debug => "debug",
        LogLevel::Info => "info",
        LogLevel::Warning => "warn",
        LogLevel::Error => "error",
        LogLevel::Off => "off",
    }
}

/// `parseLogLevel`.
pub fn parse_log_level(log_level: &str) -> Option<LogLevel> {
    match log_level {
        "trace" => Some(LogLevel::Trace),
        "debug" => Some(LogLevel::Debug),
        "info" => Some(LogLevel::Info),
        "warn" => Some(LogLevel::Warning),
        "error" | "critical" => Some(LogLevel::Error),
        "off" => Some(LogLevel::Off),
        _ => None,
    }
}

/// `new Date()` formatado no fuso local, `YYYY-MM-DD HH:mm:ss.SSS`, o carimbo dos arquivos de log.
pub(crate) fn current_timestamp() -> String {
    let local = to_local_iso_string(runtime::date_now() as f64, &local_time_zone());
    local.trim_end_matches('Z').replacen('T', " ", 1)
}
