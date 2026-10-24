//! Porte de `src/vs/platform/telemetry/common/errorTelemetry.ts` e `browser/errorTelemetry.ts`: o coletor que
//! escuta os erros inesperados (e os não capturados do processo) e os publica em lote como o evento
//! `UnhandledError`, agrupando os repetidos pela pilha.
//!
//! Divergências:
//! - `mainWindow.onerror` é o [`WindowErrorHandler`]: o ponto de entrada global que o hospedeiro chama quando uma
//!   exceção escapa de tudo ([`WindowErrorHandler::report`]) e que o `ErrorTelemetry` encadeia com o tratador
//!   anterior, como o original faz com `window.onerror`.
//! - O erro é sempre um [`Error`]: não há valor `any`. Por isso `packErrorForTelemetry` não tem o ramo de valor
//!   que não é objeto, `err.stack` não é um array (a gambiarra de `workerServer.ts` não existe) e `err.detail.stack`
//!   (o erro embrulhado do carregador AMD) não existe. `err.code` é a propriedade `code` do erro.
//! - `FileOperationError` e `PendingMigrationError` (identificado pelo nome, como o `is` do original) vêm de
//!   [`FileOperationError::from_error`] e do nome do erro.
//! - `getRecentDisposableResizeObserverContextForLoopError` (de `base/browser/dom`) lê os `ResizeObserver`
//!   embrulhados do DOM, que não existem aqui: é o gancho [`ResizeObserverContext`] opcional.
//! - O `ErrorTelemetry` do navegador estende o `BaseErrorTelemetry`; aqui o contém, e o tratador de
//!   `window.onerror` guarda uma referência fraca ao coletor (o original o guarda por fechamento, que o coletor de
//!   lixo resolve; um `Rc` forte formaria um ciclo com a janela).

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use je_base::arrays::{CompareResult, binary_search};
use je_base::dynamic::Dynamic;
use je_base::errors::{self, Error};
use je_base::event::ListenerLeakError;
use je_base::lifecycle::{DisposableStore, IDisposable, to_disposable};
use je_base::objects::safe_stringify;
use je_base::runtime::{self, TimerId};

use super::telemetry::ITelemetryService;
use crate::files::files::FileOperationError;

/// `ErrorEvent` (com os campos extras de `UnhandledErrorEvent`: `kind` e `listenerCount`).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ErrorEvent {
    pub callstack: String,
    pub msg: Option<String>,
    pub file: Option<String>,
    pub line: Option<u32>,
    pub column: Option<u32>,
    pub uncaught_error_name: Option<String>,
    pub uncaught_error_msg: Option<String>,
    pub count: Option<u32>,
    pub kind: Option<String>,
    pub listener_count: Option<usize>,
}

impl ErrorEvent {
    /// `ErrorEvent.compare`: ordena pela pilha, como o `<` do JavaScript (unidades UTF-16).
    pub fn compare(a: &ErrorEvent, b: &ErrorEvent) -> CompareResult {
        match a.callstack.encode_utf16().cmp(b.callstack.encode_utf16()) {
            std::cmp::Ordering::Less => -1.0,
            std::cmp::Ordering::Greater => 1.0,
            std::cmp::Ordering::Equal => 0.0,
        }
    }

    /// O objeto que o original entrega ao `publicLogError2`.
    pub fn to_dynamic(&self) -> Dynamic {
        let mut entries: Vec<(&str, Dynamic)> = vec![("callstack", Dynamic::string(&self.callstack))];
        let mut text = |name: &'static str, value: &Option<String>| {
            if let Some(value) = value {
                entries.push((name, Dynamic::string(value)));
            }
        };
        text("msg", &self.msg);
        text("file", &self.file);
        text("uncaught_error_name", &self.uncaught_error_name);
        text("uncaught_error_msg", &self.uncaught_error_msg);
        text("kind", &self.kind);
        let numbers = [("line", self.line.map(f64::from)), ("column", self.column.map(f64::from)), ("count", self.count.map(f64::from)), ("listenerCount", self.listener_count.map(|count| count as f64))];
        entries.extend(numbers.into_iter().filter_map(|(name, value)| value.map(|value| (name, Dynamic::Number(value)))));
        Dynamic::object(entries)
    }
}

/// O que `packErrorForTelemetry` devolve.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PackedError {
    pub callstack: Option<String>,
    pub msg: String,
}

/// `packErrorForTelemetry`: extrai a pilha e a mensagem de um erro para a telemetria. Sem mensagem, a mensagem é
/// o `safeStringify` das propriedades do erro.
pub fn pack_error_for_telemetry(err: &Error) -> PackedError {
    let msg = if err.message.is_empty() {
        let properties = Dynamic::object(err.props.iter().map(|(name, value)| (name.as_str(), Dynamic::string(value))));
        safe_stringify(&properties).unwrap_or_default()
    } else {
        err.message.clone()
    };
    PackedError { callstack: err.stack.clone(), msg }
}

/// Um coletor de erros: `BaseErrorTelemetry`.
pub struct BaseErrorTelemetry {
    telemetry_service: Rc<dyn ITelemetryService>,
    flush_delay: u64,
    flush_handle: Cell<Option<TimerId>>,
    buffer: RefCell<Vec<ErrorEvent>>,
    disposables: Rc<DisposableStore>,
}

impl BaseErrorTelemetry {
    pub const ERROR_FLUSH_TIMEOUT: u64 = 5 * 1000;

    pub fn new(telemetry_service: Rc<dyn ITelemetryService>, flush_delay: Option<u64>) -> Rc<Self> {
        let this = Rc::new(Self {
            telemetry_service,
            flush_delay: flush_delay.unwrap_or(Self::ERROR_FLUSH_TIMEOUT),
            flush_handle: Cell::new(None),
            buffer: RefCell::new(Vec::new()),
            disposables: DisposableStore::new(),
        });

        // (1) check for unexpected but handled errors
        let weak = Rc::downgrade(&this);
        let unbind = errors::add_error_listener(move |err| {
            if let Some(this) = weak.upgrade() {
                this.on_error_event(err);
            }
        });
        this.disposables.add(to_disposable(unbind));
        this
    }

    /// `_disposables`: o que as subclasses registram em `installErrorListeners`.
    pub fn disposables(&self) -> &Rc<DisposableStore> {
        &self.disposables
    }

    fn on_error_event(self: &Rc<Self>, err: &Error) {
        if err.prop("code").is_some_and(|code| !code.is_empty()) {
            return;
        }

        // If it's the no telemetry error it doesn't get logged
        // TOOD @lramos15 hacking in FileOperation error because it's too messy to adopt ErrorNoTelemetry. A better solution should be found
        //
        // Explicitly filter out PendingMigrationError for https://github.com/microsoft/vscode/issues/250648#issuecomment-3394040431
        // We don't inherit from ErrorNoTelemetry to preserve the name used in reporting for exthostdeprecatedapiusage event.
        // TODO(deepak1556): remove when PendingMigrationError is no longer needed.
        if err.is_error_no_telemetry() || FileOperationError::from_error(err).is_some() || err.name == "PendingMigrationError" || err.message.contains("Unable to read file") {
            return;
        }

        // errors without a stack are not useful telemetry
        let PackedError { callstack, msg } = pack_error_for_telemetry(err);
        let Some(callstack) = callstack.filter(|callstack| !callstack.is_empty()) else {
            return;
        };

        let mut error_event = ErrorEvent { msg: Some(msg), callstack, ..ErrorEvent::default() };

        // enrich with listener leak diagnostic fields
        if let Some(leak) = ListenerLeakError::from_error(err) {
            error_event.kind = Some(leak.kind);
            error_event.listener_count = Some(leak.listener_count);
        }

        self.enqueue(error_event);
    }

    /// `_enqueue`: agrupa por pilha, contando as repetições, e agenda o envio do lote.
    pub fn enqueue(self: &Rc<Self>, mut event: ErrorEvent) {
        {
            let mut buffer = self.buffer.borrow_mut();
            let index = binary_search(buffer.as_slice(), &event, ErrorEvent::compare);
            if index < 0 {
                event.count = Some(1);
                buffer.insert(!index as usize, event);
            } else {
                let existing = &mut buffer[index as usize];
                existing.count = Some(existing.count.unwrap_or(0) + 1);
            }
        }

        if self.flush_handle.get().is_none() {
            let weak = Rc::downgrade(self);
            self.flush_handle.set(Some(runtime::set_timeout(
                move || {
                    if let Some(this) = weak.upgrade() {
                        this.flush_buffer();
                        this.flush_handle.set(None);
                    }
                },
                self.flush_delay,
            )));
        }
    }

    fn flush_buffer(&self) {
        let events = std::mem::take(&mut *self.buffer.borrow_mut());
        for error in events {
            self.telemetry_service.public_log_error2("UnhandledError", Some(error.to_dynamic()));
        }
    }
}

impl IDisposable for BaseErrorTelemetry {
    fn dispose(&self) {
        if let Some(handle) = self.flush_handle.get() {
            runtime::clear_timeout(handle);
        }
        self.flush_buffer();
        self.disposables.dispose();
    }
}

/// O tratador de `window.onerror`: `(message, filename, line, column, error)`.
pub type OnErrorHandler = Rc<dyn Fn(&str, Option<&str>, Option<u32>, Option<u32>, Option<&Error>)>;

/// O gancho de `getRecentDisposableResizeObserverContextForLoopError(msg)`: o contexto dos observadores que rodaram
/// há pouco, quando a mensagem é o aviso de laço do `ResizeObserver`.
pub type ResizeObserverContext = Rc<dyn Fn(&str) -> Option<String>>;

/// `mainWindow.onerror`: o tratador global de exceções não capturadas. O hospedeiro o aciona com
/// [`WindowErrorHandler::report`] quando uma exceção escapa de tudo.
#[derive(Default)]
pub struct WindowErrorHandler {
    handler: RefCell<Option<OnErrorHandler>>,
}

impl WindowErrorHandler {
    pub fn new() -> Rc<Self> {
        Rc::new(Self::default())
    }

    /// Lê `mainWindow.onerror`.
    pub fn on_error(&self) -> Option<OnErrorHandler> {
        self.handler.borrow().clone()
    }

    /// Escreve `mainWindow.onerror`.
    pub fn set_on_error(&self, handler: Option<OnErrorHandler>) {
        *self.handler.borrow_mut() = handler;
    }

    /// Chama `mainWindow.onerror(message, filename, line, column, error)`, se houver tratador.
    pub fn report(&self, message: &str, file: Option<&str>, line: Option<u32>, column: Option<u32>, error: Option<&Error>) {
        if let Some(handler) = self.on_error() {
            handler(message, file, line, column, error);
        }
    }
}

/// O que o `ErrorTelemetry` aceita além do serviço de telemetria e da janela.
#[derive(Default)]
pub struct ErrorTelemetryOptions {
    /// `flushDelay` (padrão: [`BaseErrorTelemetry::ERROR_FLUSH_TIMEOUT`]).
    pub flush_delay: Option<u64>,
    pub resize_observer_context: Option<ResizeObserverContext>,
}

/// `_onUncaughtError`: o evento de um erro não capturado, ou `None` quando o erro não vai para a telemetria.
fn uncaught_error_event(
    msg: &str,
    file: Option<&str>,
    line: Option<u32>,
    column: Option<u32>,
    err: Option<&Error>,
    resize_observer_context: Option<&dyn Fn(&str) -> Option<String>>,
) -> Option<ErrorEvent> {
    let mut data = ErrorEvent { callstack: msg.to_owned(), msg: Some(msg.to_owned()), file: file.map(str::to_owned), line, column, ..ErrorEvent::default() };

    if let Some(err) = err {
        // If it's the no telemetry error it doesn't get logged
        if err.is_error_no_telemetry() {
            return None;
        }

        data.uncaught_error_name = Some(err.name.clone());
        if !err.message.is_empty() {
            data.uncaught_error_msg = Some(err.message.clone());
        }
        if let Some(stack) = err.stack.as_deref().filter(|stack| !stack.is_empty()) {
            data.callstack = stack.to_owned();
        }
    }

    // The browser does not expose which observer or skipped target caused a
    // ResizeObserver loop warning. Include the wrapped observers that ran
    // recently as delivery context instead of falsely attributing the loop
    // to whichever callback happened to run last.
    if let Some(context) = resize_observer_context.and_then(|hook| hook(msg)) {
        data.msg = Some(context.clone());
        data.callstack = context;
    }

    Some(data)
}

/// `ErrorTelemetry` do navegador.
pub struct ErrorTelemetry {
    base: Rc<BaseErrorTelemetry>,
}

impl ErrorTelemetry {
    pub const ERROR_FLUSH_TIMEOUT: u64 = BaseErrorTelemetry::ERROR_FLUSH_TIMEOUT;

    pub fn new(telemetry_service: Rc<dyn ITelemetryService>, window: &Rc<WindowErrorHandler>) -> Self {
        Self::with_options(telemetry_service, window, ErrorTelemetryOptions::default())
    }

    pub fn with_options(telemetry_service: Rc<dyn ITelemetryService>, window: &Rc<WindowErrorHandler>, options: ErrorTelemetryOptions) -> Self {
        let base = BaseErrorTelemetry::new(telemetry_service, options.flush_delay);

        // `installErrorListeners`
        let old_on_error = window.on_error();
        let weak = Rc::downgrade(&base);
        let chained = old_on_error.clone();
        let resize_observer_context = options.resize_observer_context;
        let handler: OnErrorHandler = Rc::new(move |message: &str, file: Option<&str>, line: Option<u32>, column: Option<u32>, error: Option<&Error>| {
            if let Some(event) = uncaught_error_event(message, file, line, column, error, resize_observer_context.as_deref())
                && let Some(base) = weak.upgrade()
            {
                base.enqueue(event);
            }
            if let Some(old_on_error) = &chained {
                old_on_error(message, file, line, column, error);
            }
        });
        window.set_on_error(Some(handler));
        let window = window.clone();
        base.disposables().add(to_disposable(move || {
            if let Some(old_on_error) = old_on_error {
                window.set_on_error(Some(old_on_error));
            }
        }));

        Self { base }
    }
}

impl IDisposable for ErrorTelemetry {
    fn dispose(&self) {
        self.base.dispose();
    }
}
