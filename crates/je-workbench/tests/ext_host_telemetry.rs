//! Porte de `src/vs/workbench/api/test/browser/extHostTelemetry.test.ts` (suíte `ExtHostTelemetry`), caso a caso e na
//! mesma ordem.
//!
//! Notas de tradução:
//! - `ensureNoDisposablesAreLeakedInTestSuite()` vira o guarda criado no começo de cada caso, e o `store` é o
//!   `DisposableStore` dele.
//! - `new class extends mock<IExtHostInitDataService>() { environment; telemetryInfo; remote }` é o
//!   [`ExtHostTelemetryInitData`] de [`mock_init_data`]. O `IExtensionDescription` do teste é o `ExtensionDescription`
//!   da `je-extensions` ([`mock_extension`]).
//! - `TestTelemetryLoggerService` (de `telemetryLogAppender.test.ts`, que o original importa) e o logger de teste dele
//!   são [`TestTelemetryLoggerService`] e [`TestTelemetryLogger`], escritos como no teste de `je-platform` (aquele
//!   arquivo é um teste, não um módulo reutilizável).
//! - O `appender` do teste (`TelemetrySender` com `sendEventData`, `sendErrorData` e `flush`) é [`SpySender`], que grava
//!   no [`TelemetryLoggerSpy`]; o `flush` que lança é o `flush_throws`. `logger.logError('evento', data)` e
//!   `logger.logError(new Error(...), data)` são `log_error_event` e `log_error_exception`.
//! - `Validate sender instances` passa [`Dynamic`] (o `<any>null`, `<any>1` etc. do original).
//! - `Reflect.get(extensionTelemetry, '_telemetryLoggers')` é [`ExtHostTelemetry::telemetry_logger_count`].
//! - `assert.throws(() => logger.dispose(), /flush failed/)` é a captura do pânico e a conferência da mensagem.
//! - `Event.on...(() => ...)` com `store.add(...)` é `listen` guardado no `store`.

#[path = "../../je-base/tests/common/mod.rs"]
mod common;

use std::cell::RefCell;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::rc::Rc;

use common::ensure_no_disposables_are_leaked_in_test_suite;
use je_base::dynamic::{Dynamic, FunctionRef};
use je_base::errors::{self, Error};
use je_base::event::{Emitter, Event};
use je_base::json_formatter::json_stringify;
use je_base::lifecycle::{DisposableStore, IDisposable};
use je_base::promise::Promise;
use je_base::uri::URI;
use je_extensions::manifest::{Engines, ExtensionIdentifier, ExtensionManifest, TargetPlatform};
use je_extensions::scanner::ExtensionDescription;
use je_platform::js::to_json;
use je_platform::log::{
    AbstractLogger, DEFAULT_LOG_LEVEL, DidChangeLoggersEvent, LogArg, LogLevel, LogMessage, Logger, LoggerOptions, LoggerRef, LoggerResource,
    LoggerService, LoggerServiceLogLevelChange, ResourceOrId,
};
use je_platform::telemetry::TelemetryLevel;
use je_workbench::api::common::ext_host_protocol::{ExtHostTelemetryShape, TelemetryProductConfig};
use je_workbench::api::common::ext_host_telemetry::{
    ExtHostTelemetry, ExtHostTelemetryEnvironment, ExtHostTelemetryInfo, ExtHostTelemetryInitData, ExtHostTelemetryLogger, ExtHostTelemetryRemote,
    TelemetryLoggerOptions, TelemetrySender,
};

struct TelemetryLoggerSpy {
    data_arr: Vec<(String, Dynamic)>,
    exception_arr: Vec<(Error, Dynamic)>,
    flush_called: bool,
}

type SpyRef = Rc<RefCell<TelemetryLoggerSpy>>;

fn new_spy() -> SpyRef {
    Rc::new(RefCell::new(TelemetryLoggerSpy { data_arr: Vec::new(), exception_arr: Vec::new(), flush_called: false }))
}

/// O `appender` que a extensão contribuiria.
struct SpySender {
    spy: SpyRef,
    flush_throws: bool,
}

impl TelemetrySender for SpySender {
    fn send_event_data(&self, event_name: &str, data: &Dynamic) {
        self.spy.borrow_mut().data_arr.push((event_name.to_owned(), data.clone()));
    }

    fn send_error_data(&self, exception: &Error, data: &Dynamic) {
        self.spy.borrow_mut().exception_arr.push((exception.clone(), data.clone()));
    }

    fn has_flush(&self) -> bool {
        true
    }

    fn flush(&self) -> Result<Promise<()>, Error> {
        if self.flush_throws {
            return Err(Error::new("flush failed"));
        }
        self.spy.borrow_mut().flush_called = true;
        Ok(Promise::resolved(()))
    }
}

// #region TestTelemetryLoggerService (telemetryLogAppender.test.ts)

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
}

impl TestTelemetryLoggerService {
    fn new(log_level: LogLevel) -> Self {
        Self { log_level, logger: RefCell::new(None) }
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
    fn create_logger(&self, _id_or_resource: ResourceOrId, _options: Option<LoggerOptions>) -> LoggerRef {
        self.created_logger()
    }
    fn get_logger(&self, _resource_or_id: &ResourceOrId) -> Option<LoggerRef> {
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
        let never: Rc<Emitter<DidChangeLoggersEvent>> = Emitter::new();
        never.event()
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

// #endregion

fn mock_init_data() -> ExtHostTelemetryInitData {
    ExtHostTelemetryInitData {
        environment: ExtHostTelemetryEnvironment { is_extension_telemetry_logging_only: false, app_host: "test".to_owned(), is_sessions_window: false },
        telemetry_info: ExtHostTelemetryInfo {
            first_session_date: "2020-01-01T00:00:00.000Z".to_owned(),
            session_id: "test".to_owned(),
            machine_id: "test".to_owned(),
            sqm_id: Some("test".to_owned()),
            dev_device_id: Some("test".to_owned()),
        },
        remote: ExtHostTelemetryRemote { authority: Some("test".to_owned()), is_remote: false },
        ..ExtHostTelemetryInitData::default()
    }
}

fn mock_extension() -> ExtensionDescription {
    ExtensionDescription {
        id: "test-extension".to_owned(),
        identifier: ExtensionIdentifier::new("test-extension"),
        is_builtin: true,
        is_user_builtin: true,
        is_under_development: true,
        extension_location: "fake".to_owned(),
        uuid: None,
        target_platform: TargetPlatform::Universal,
        publisher_display_name: None,
        pre_release: false,
        manifest: ExtensionManifest {
            name: "test-extension".to_owned(),
            publisher: "vscode".to_owned(),
            version: "1.0.0".to_owned(),
            engines: Engines { vscode: "*".to_owned() },
            ..ExtensionManifest::default()
        },
    }
}

fn create_ext_host_telemetry(store: &DisposableStore) -> Rc<ExtHostTelemetry> {
    let extension_telemetry = ExtHostTelemetry::new(false, mock_init_data(), &TestTelemetryLoggerService::new(DEFAULT_LOG_LEVEL));
    store.add_rc(extension_telemetry.clone());
    extension_telemetry.rpc_initialize_telemetry_level(TelemetryLevel::Usage, true, Some(TelemetryProductConfig { usage: true, error: true }));
    extension_telemetry
}

fn create_logger(
    store: &DisposableStore,
    function_spy: &SpyRef,
    ext_host_telemetry: Option<&Rc<ExtHostTelemetry>>,
    options: Option<TelemetryLoggerOptions>,
) -> Rc<ExtHostTelemetryLogger> {
    let extension_telemetry = ext_host_telemetry.cloned().unwrap_or_else(|| create_ext_host_telemetry(store));
    // This is the appender which the extension would contribute
    let appender = Rc::new(SpySender { spy: function_spy.clone(), flush_throws: false });

    if let Some(ext_host_telemetry) = ext_host_telemetry {
        store.add_rc(ext_host_telemetry.clone());
    }

    let logger = extension_telemetry.instantiate_logger(&mock_extension(), appender, options);
    store.add_rc(logger.clone());
    logger
}

fn string_prop(data: &Dynamic, key: &str) -> Dynamic {
    data.get_str(key)
}

fn js_object(entries: Vec<(&str, Dynamic)>) -> Dynamic {
    Dynamic::object(entries)
}

fn function() -> Dynamic {
    Dynamic::Function(FunctionRef::native("f", 0, |_| Dynamic::Undefined))
}

fn strict_equals_str(value: &Dynamic, expected: &str) -> bool {
    value.strict_equals(&Dynamic::string(expected))
}

#[test]
fn validate_sender_instances() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    assert!(ExtHostTelemetryLogger::validate_sender(&Dynamic::Null).is_err());
    assert!(ExtHostTelemetryLogger::validate_sender(&Dynamic::Number(1.0)).is_err());
    assert!(ExtHostTelemetryLogger::validate_sender(&js_object(vec![])).is_err());
    assert!(
        ExtHostTelemetryLogger::validate_sender(&js_object(vec![("sendErrorData", function()), ("sendEventData", Dynamic::Bool(true))])).is_err()
    );
    assert!(
        ExtHostTelemetryLogger::validate_sender(&js_object(vec![("sendErrorData", Dynamic::Number(123.0)), ("sendEventData", function())])).is_err()
    );
    assert!(
        ExtHostTelemetryLogger::validate_sender(&js_object(vec![
            ("sendErrorData", function()),
            ("sendEventData", function()),
            ("flush", Dynamic::Bool(true)),
        ]))
        .is_err()
    );
}

#[test]
fn ensure_logger_gets_proper_telemetry_level_during_initialization() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let store = guard.store();
    let extension_telemetry = create_ext_host_telemetry(&store);
    let mut config = extension_telemetry.get_telemetry_details();
    assert!(config.is_crash_enabled);
    assert!(config.is_usage_enabled);
    assert!(config.is_errors_enabled);

    // Initialize would never be called twice, but this is just for testing
    extension_telemetry.rpc_initialize_telemetry_level(TelemetryLevel::Error, true, Some(TelemetryProductConfig { usage: true, error: true }));
    config = extension_telemetry.get_telemetry_details();
    assert!(config.is_crash_enabled);
    assert!(!config.is_usage_enabled);
    assert!(config.is_errors_enabled);

    extension_telemetry.rpc_initialize_telemetry_level(TelemetryLevel::Crash, true, Some(TelemetryProductConfig { usage: true, error: true }));
    config = extension_telemetry.get_telemetry_details();
    assert!(config.is_crash_enabled);
    assert!(!config.is_usage_enabled);
    assert!(!config.is_errors_enabled);

    extension_telemetry.rpc_initialize_telemetry_level(TelemetryLevel::Usage, true, Some(TelemetryProductConfig { usage: false, error: true }));
    config = extension_telemetry.get_telemetry_details();
    assert!(config.is_crash_enabled);
    assert!(!config.is_usage_enabled);
    assert!(config.is_errors_enabled);
    extension_telemetry.dispose();
}

#[test]
fn simple_log_event_to_telemetry_logger() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let store = guard.store();
    let function_spy = new_spy();

    let logger = create_logger(&store, &function_spy, None, None);
    let data = || js_object(vec![("test-data", Dynamic::string("test-data"))]);

    logger.log_usage("test-event", Some(&data()));
    assert_eq!(function_spy.borrow().data_arr.len(), 1);
    assert_eq!(function_spy.borrow().data_arr[0].0, format!("{}/test-event", mock_extension().manifest.name));
    assert!(strict_equals_str(&string_prop(&function_spy.borrow().data_arr[0].1, "test-data"), "test-data"));

    logger.log_usage("test-event", Some(&data()));
    assert_eq!(function_spy.borrow().data_arr.len(), 2);

    logger.log_error_event("test-event", Some(&data()));
    assert_eq!(function_spy.borrow().data_arr.len(), 3);

    logger.log_error_exception(&Error::new("test-error"), Some(&data()));
    assert_eq!(function_spy.borrow().data_arr.len(), 3);
    assert_eq!(function_spy.borrow().exception_arr.len(), 1);

    // Assert not flushed
    assert!(!function_spy.borrow().flush_called);

    // Call flush and assert that flush occurs
    logger.dispose();
    assert!(function_spy.borrow().flush_called);
}

#[test]
fn disposed_telemetry_loggers_are_removed_from_extension_bookkeeping() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let store = guard.store();
    let extension_telemetry = create_ext_host_telemetry(&store);
    let function_spy = new_spy();
    let first_logger = create_logger(&store, &function_spy, Some(&extension_telemetry), None);
    let second_logger = create_logger(&store, &function_spy, Some(&extension_telemetry), None);
    let extension_id = mock_extension().identifier.value;

    assert_eq!(extension_telemetry.telemetry_logger_count(&extension_id), Some(2));
    first_logger.dispose();
    assert_eq!(extension_telemetry.telemetry_logger_count(&extension_id), Some(1));
    second_logger.dispose();
    assert_eq!(extension_telemetry.telemetry_logger_count(&extension_id), None);
}

#[test]
fn telemetry_loggers_are_removed_from_extension_bookkeeping_when_flush_throws() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let store = guard.store();
    let extension_telemetry = create_ext_host_telemetry(&store);
    let sender = Rc::new(SpySender { spy: new_spy(), flush_throws: true });
    let logger = extension_telemetry.instantiate_logger(&mock_extension(), sender, None);
    store.add_rc(logger.clone());
    let extension_id = mock_extension().identifier.value;

    assert_eq!(extension_telemetry.telemetry_logger_count(&extension_id), Some(1));
    let outcome = catch_unwind(AssertUnwindSafe(|| logger.dispose()));
    let error = errors::from_panic(outcome.expect_err("o dispose deveria lançar"));
    assert!(error.message.contains("flush failed"), "a mensagem '{}' deveria conter 'flush failed'", error.message);
    assert_eq!(extension_telemetry.telemetry_logger_count(&extension_id), None);
}

#[test]
fn replacing_the_last_logger_during_enablement_notification_does_not_notify_the_replacement() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let store = guard.store();
    let extension_telemetry = create_ext_host_telemetry(&store);
    let function_spy = new_spy();
    let notifications = Rc::new(RefCell::new(0));
    let current_logger = Rc::new(RefCell::new(create_logger(&store, &function_spy, Some(&extension_telemetry), None)));

    struct Subscriber {
        store: Rc<DisposableStore>,
        extension_telemetry: Rc<ExtHostTelemetry>,
        function_spy: SpyRef,
        notifications: Rc<RefCell<i32>>,
        current_logger: Rc<RefCell<Rc<ExtHostTelemetryLogger>>>,
    }

    fn subscribe(subscriber: &Rc<Subscriber>, logger: &Rc<ExtHostTelemetryLogger>) {
        let state = subscriber.clone();
        subscriber.store.add(logger.on_did_change_enable_states().listen(move |logger| {
            *state.notifications.borrow_mut() += 1;
            // Bound the reproducer so a live-map iteration regression cannot hang the test.
            if *state.notifications.borrow() >= 5 {
                return;
            }
            logger.dispose();
            let replacement = create_logger(&state.store, &state.function_spy, Some(&state.extension_telemetry), None);
            *state.current_logger.borrow_mut() = replacement.clone();
            subscribe(&state, &replacement);
        }));
    }

    let subscriber = Rc::new(Subscriber {
        store: store.clone(),
        extension_telemetry: extension_telemetry.clone(),
        function_spy: function_spy.clone(),
        notifications: notifications.clone(),
        current_logger: current_logger.clone(),
    });
    let first = current_logger.borrow().clone();
    subscribe(&subscriber, &first);

    extension_telemetry.rpc_on_did_change_telemetry_level(TelemetryLevel::None);
    {
        let logger = current_logger.borrow();
        assert_eq!((*notifications.borrow(), logger.is_usage_enabled(), logger.is_errors_enabled()), (1, false, false));
    }

    extension_telemetry.rpc_on_did_change_telemetry_level(TelemetryLevel::Usage);
    {
        let logger = current_logger.borrow();
        assert_eq!((*notifications.borrow(), logger.is_usage_enabled(), logger.is_errors_enabled()), (2, true, true));
    }
}

#[test]
fn simple_log_event_to_telemetry_logger_with_options() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let store = guard.store();
    let function_spy = new_spy();

    let logger = create_logger(
        &store,
        &function_spy,
        None,
        Some(TelemetryLoggerOptions {
            additional_common_properties: Some(js_object(vec![("common.foo", Dynamic::string("bar"))])),
            ..TelemetryLoggerOptions::default()
        }),
    );
    let data = || js_object(vec![("test-data", Dynamic::string("test-data"))]);

    logger.log_usage("test-event", Some(&data()));
    assert_eq!(function_spy.borrow().data_arr.len(), 1);
    assert_eq!(function_spy.borrow().data_arr[0].0, format!("{}/test-event", mock_extension().manifest.name));
    assert!(strict_equals_str(&string_prop(&function_spy.borrow().data_arr[0].1, "test-data"), "test-data"));
    assert!(strict_equals_str(&string_prop(&function_spy.borrow().data_arr[0].1, "common.foo"), "bar"));

    logger.log_usage("test-event", Some(&data()));
    assert_eq!(function_spy.borrow().data_arr.len(), 2);

    logger.log_error_event("test-event", Some(&data()));
    assert_eq!(function_spy.borrow().data_arr.len(), 3);

    logger.log_error_exception(&Error::new("test-error"), Some(&data()));
    assert_eq!(function_spy.borrow().data_arr.len(), 3);
    assert_eq!(function_spy.borrow().exception_arr.len(), 1);

    // Assert not flushed
    assert!(!function_spy.borrow().flush_called);

    // Call flush and assert that flush occurs
    logger.dispose();
    assert!(function_spy.borrow().flush_called);
}

#[test]
fn log_error_should_get_common_properties_193205() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let store = guard.store();
    let function_spy = new_spy();

    let logger = create_logger(
        &store,
        &function_spy,
        None,
        Some(TelemetryLoggerOptions {
            additional_common_properties: Some(js_object(vec![("common.foo", Dynamic::string("bar"))])),
            ..TelemetryLoggerOptions::default()
        }),
    );
    logger.log_error_exception(&Error::new("Test error"), None);
    assert_eq!(function_spy.borrow().exception_arr.len(), 1);
    assert!(strict_equals_str(&string_prop(&function_spy.borrow().exception_arr[0].1, "common.foo"), "bar"));
    assert!(strict_equals_str(&string_prop(&function_spy.borrow().exception_arr[0].1, "common.product"), "test"));

    logger.log_error_event("test-error-event", None);
    assert_eq!(function_spy.borrow().data_arr.len(), 1);
    assert!(strict_equals_str(&string_prop(&function_spy.borrow().data_arr[0].1, "common.foo"), "bar"));
    assert!(strict_equals_str(&string_prop(&function_spy.borrow().data_arr[0].1, "common.product"), "test"));

    logger.log_error_event("test-error-event", Some(&js_object(vec![("test-data", Dynamic::string("test-data"))])));
    assert_eq!(function_spy.borrow().data_arr.len(), 2);
    assert!(strict_equals_str(&string_prop(&function_spy.borrow().data_arr[1].1, "common.foo"), "bar"));
    assert!(strict_equals_str(&string_prop(&function_spy.borrow().data_arr[1].1, "common.product"), "test"));

    logger.log_error_event(
        "test-error-event",
        Some(&js_object(vec![("properties", js_object(vec![("test-data", Dynamic::string("test-data"))]))])),
    );
    assert_eq!(function_spy.borrow().data_arr.len(), 3);
    let properties = string_prop(&function_spy.borrow().data_arr[2].1, "properties");
    assert!(strict_equals_str(&string_prop(&properties, "common.foo"), "bar"));
    assert!(strict_equals_str(&string_prop(&properties, "common.product"), "test"));

    logger.dispose();
    assert!(function_spy.borrow().flush_called);
}

#[test]
fn ensure_logger_properly_cleans_pii() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let store = guard.store();
    let function_spy = new_spy();

    let logger = create_logger(&store, &function_spy, None, None);

    // Log an event with a bunch of PII, this should all get cleaned out
    logger.log_usage(
        "test-event",
        Some(&js_object(vec![
            ("fake-password", Dynamic::string("pwd=123")),
            ("fake-email", Dynamic::string("no-reply@example.com")),
            ("fake-token", Dynamic::string("token=123")),
            ("fake-slack-token", Dynamic::string("xoxp-123")),
            ("fake-path", Dynamic::string("/Users/username/.vscode/extensions")),
        ])),
    );

    assert_eq!(function_spy.borrow().data_arr.len(), 1);
    assert_eq!(function_spy.borrow().data_arr[0].0, format!("{}/test-event", mock_extension().manifest.name));
    let data = function_spy.borrow().data_arr[0].1.clone();
    assert!(strict_equals_str(&string_prop(&data, "fake-password"), "<REDACTED: Generic Secret>"));
    assert!(strict_equals_str(&string_prop(&data, "fake-email"), "<REDACTED: Email>"));
    assert!(strict_equals_str(&string_prop(&data, "fake-token"), "<REDACTED: Generic Secret>"));
    assert!(strict_equals_str(&string_prop(&data, "fake-slack-token"), "<REDACTED: Slack Token>"));
    assert!(strict_equals_str(&string_prop(&data, "fake-path"), "<REDACTED: user-file-path>"));
}

#[test]
fn ensure_output_channel_is_logged_to() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let store = guard.store();

    // Have to re-duplicate code here because I the logger service isn't exposed in the simple setup functions
    let logger_service = TestTelemetryLoggerService::new(LogLevel::Trace);
    let extension_telemetry = ExtHostTelemetry::new(false, mock_init_data(), &logger_service);
    extension_telemetry.rpc_initialize_telemetry_level(TelemetryLevel::Usage, true, Some(TelemetryProductConfig { usage: true, error: true }));

    let function_spy = new_spy();

    let logger = create_logger(&store, &function_spy, Some(&extension_telemetry), None);

    // Ensure headers are logged on instantiation
    assert_eq!(logger_service.created_logger().logs.borrow().len(), 0);

    logger.log_usage("test-event", Some(&js_object(vec![("test-data", Dynamic::string("test-data"))])));
    // Initial header is logged then the event
    assert_eq!(logger_service.created_logger().logs.borrow().len(), 1);
    assert!(logger_service.created_logger().logs.borrow()[0].starts_with("test-extension/test-event"));
}
