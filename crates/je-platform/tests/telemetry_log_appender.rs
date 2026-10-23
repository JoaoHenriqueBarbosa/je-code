//! Porte de `src/vs/platform/telemetry/test/common/telemetryLogAppender.test.ts` (suíte `TelemetryLogAdapter`), caso a caso e na mesma ordem.
//!
//! Divergência: o original usa `TestInstantiationService.stub(IEnvironmentService, {})` e
//! `stub(IProductService, {})`. Aqui são o `NativeEnvironmentService` e o `ProductService` padrão (os stubs
//! de um trait inteiro não existem em Rust); o resultado só muda o sufixo ` (Not Sent)` do nome do logger,
//! que o teste não lê.

mod common;

use std::cell::RefCell;
use std::rc::Rc;

use common::*;
use je_base::event::{Emitter, Event};
use je_base::json_formatter::json_stringify;
use je_base::lifecycle::IDisposable;
use je_base::uri::URI;
use je_platform::environment::native::NativeEnvironmentService;
use je_platform::environment::native_parsed_args::NativeParsedArgs;
use je_platform::js::to_json;
use je_platform::log::{
    AbstractLogger, DEFAULT_LOG_LEVEL, DidChangeLoggersEvent, LogArg, LogLevel, LogMessage, Logger, LoggerOptions, LoggerRef, LoggerResource,
    LoggerService, LoggerServiceLogLevelChange, ResourceOrId,
};
use je_platform::product::ProductService;
use je_platform::telemetry::{ITelemetryAppender, TelemetryLogAppender, TelemetryLogAppenderOptions};

struct TestTelemetryLogger {
    base: AbstractLogger,
    logs: RefCell<Vec<String>>,
}

impl TestTelemetryLogger {
    fn new(log_level: LogLevel) -> Rc<Self> {
        let logger = Rc::new(Self { base: AbstractLogger::new(), logs: RefCell::new(Vec::new()) });
        logger.base.set_level(log_level);
        logger
    }
}

impl IDisposable for TestTelemetryLogger {
    fn dispose(&self) {
        self.base.dispose();
    }
}

impl Logger for TestTelemetryLogger {
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
        if self.base.can_log(LogLevel::Trace) {
            let args: Vec<_> = args.iter().map(|a| to_json(&a.to_dynamic()).unwrap_or(je_base::json::Value::Null)).collect();
            self.logs.borrow_mut().push(format!("{message}{}", json_stringify(&je_base::json::Value::Array(args), None)));
        }
    }
    fn debug_args(&self, message: &str, _args: &[LogArg]) {
        if self.base.can_log(LogLevel::Debug) {
            self.logs.borrow_mut().push(message.to_owned());
        }
    }
    fn info_args(&self, message: &str, _args: &[LogArg]) {
        if self.base.can_log(LogLevel::Info) {
            self.logs.borrow_mut().push(message.to_owned());
        }
    }
    fn warn_args(&self, message: &LogMessage, _args: &[LogArg]) {
        if self.base.can_log(LogLevel::Warning) {
            self.logs.borrow_mut().push(format!("{message:?}"));
        }
    }
    fn error_args(&self, message: &LogMessage, _args: &[LogArg]) {
        if self.base.can_log(LogLevel::Error) {
            self.logs.borrow_mut().push(format!("{message:?}"));
        }
    }
    fn flush(&self) {}
}

struct TestTelemetryLoggerService {
    log_level: LogLevel,
    logger: RefCell<Option<Rc<TestTelemetryLogger>>>,
    logger_id: RefCell<Option<String>>,
    never: Rc<Emitter<()>>,
}

impl TestTelemetryLoggerService {
    fn new(log_level: LogLevel) -> Self {
        Self { log_level, logger: RefCell::new(None), logger_id: RefCell::new(None), never: Emitter::new() }
    }

    fn record(&self, id: &ResourceOrId) {
        if let ResourceOrId::Id(id) = id {
            *self.logger_id.borrow_mut() = Some(id.clone());
        }
    }

    /// `createLogger()` sem argumentos, como o teste usa para ler o logger.
    fn created_logger(&self) -> Rc<TestTelemetryLogger> {
        self.logger.borrow_mut().get_or_insert_with(|| TestTelemetryLogger::new(self.log_level)).clone()
    }
}

impl IDisposable for TestTelemetryLoggerService {
    fn dispose(&self) {}
}

impl LoggerService for TestTelemetryLoggerService {
    fn create_logger(&self, id_or_resource: ResourceOrId, _options: Option<LoggerOptions>) -> LoggerRef {
        self.record(&id_or_resource);
        self.created_logger()
    }
    fn get_logger(&self, resource_or_id: &ResourceOrId) -> Option<LoggerRef> {
        self.record(resource_or_id);
        self.logger.borrow().clone().map(|l| l as LoggerRef)
    }
    fn on_did_change_log_level(&self) -> Event<LoggerServiceLogLevelChange> {
        Event::none()
    }
    fn set_default_log_level(&self, _level: LogLevel) {}
    fn set_log_level(&self, _resource: &URI, _level: LogLevel) {}
    fn get_log_level(&self, _resource: Option<&URI>) -> LogLevel {
        LogLevel::Info
    }
    fn on_did_change_visibility(&self) -> Event<(URI, bool)> {
        Event::none()
    }
    fn set_visibility(&self, _resource_or_id: &ResourceOrId, _visible: bool) {}
    fn on_did_change_loggers(&self) -> Event<DidChangeLoggersEvent> {
        let _ = &self.never;
        Event::none()
    }
    fn register_logger(&self, _resource: LoggerResource) {}
    fn deregister_logger(&self, _id_or_resource: &ResourceOrId) {}
    fn get_registered_loggers(&self) -> Vec<LoggerResource> {
        Vec::new()
    }
    fn get_registered_logger(&self, _resource: &URI) -> Option<LoggerResource> {
        None
    }
}

fn environment() -> NativeEnvironmentService {
    NativeEnvironmentService::native(NativeParsedArgs::default(), ProductService::default(), "/app").unwrap()
}

#[test]
fn do_not_log_telemetry_if_log_level_is_not_trace() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    let test_logger_service = TestTelemetryLoggerService::new(DEFAULT_LOG_LEVEL);
    let test_object = TelemetryLogAppender::new("".into(), false, &test_logger_service, &environment(), &ProductService::default());
    test_object.log("testEvent", &j("{'hello': 'world', 'isTrue': true, 'numberBetween1And3': 2}"));
    assert_eq!(test_logger_service.created_logger().logs.borrow().len(), 0);
    test_object.dispose();
}

#[test]
fn log_telemetry_if_log_level_is_trace() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    let test_logger_service = TestTelemetryLoggerService::new(LogLevel::Trace);
    let test_object = TelemetryLogAppender::new("".into(), false, &test_logger_service, &environment(), &ProductService::default());
    test_object.log("testEvent", &j("{'hello': 'world', 'isTrue': true, 'numberBetween1And3': 2}"));
    assert_eq!(
        test_logger_service.created_logger().logs.borrow()[0],
        r#"telemetry/testEvent[{"properties":{"hello":"world"},"measurements":{"isTrue":1,"numberBetween1And3":2}}]"#
    );
    test_object.dispose();
}

#[test]
fn use_custom_logger_id() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    let test_logger_service = TestTelemetryLoggerService::new(LogLevel::Trace);
    let test_object = TelemetryLogAppender::new(
        TelemetryLogAppenderOptions::Custom { prefix: String::new(), logger_id: "agentHostTelemetry".to_owned() },
        false,
        &test_logger_service,
        &environment(),
        &ProductService::default(),
    );
    assert_eq!(test_logger_service.logger_id.borrow().as_deref(), Some("agentHostTelemetry"));
    test_object.dispose();
}
