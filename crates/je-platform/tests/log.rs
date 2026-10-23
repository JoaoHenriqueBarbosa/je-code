//! Porte de `src/vs/platform/log/test/common/log.test.ts` (suíte `Log`), caso a caso e na mesma ordem.
//!
//! Divergências: o `Error` do JavaScript carrega `stack` sozinho; aqui cada erro nasce com uma pilha escrita
//! à mão (`Error::with_stack`), e os casos que dependem de comparar pilhas diferentes (`compares the
//! complete formatted stack...`) usam pilhas distintas. Um valor `unknown` que chega ao tratador é um
//! [`UnexpectedError`] (`Native` para o `Error`, `Value` para o resto). A subclasse `TestLogger` de
//! `AbstractMessageLogger` é um `MessageLogger` com o `error` sobrescrito por [`delegate_logger!`].

mod common;

use std::cell::RefCell;
use std::rc::Rc;

use common::files::async_test;
use common::ensure_no_disposables_are_leaked_in_test_suite;
use je_base::async_::timeout;
use je_base::dynamic::Dynamic;
use je_base::error_message::{error_to_dynamic, to_error_message, to_error_message_of};
use je_base::errors::{self, Error};
use je_platform::delegate_logger;
use je_platform::log::{
    AdapterLogger, LogArg, LogLevel, LogMessage, Logger, LoggerRef, MessageLogger, MessageSink, UnexpectedError,
    create_unexpected_error_handler,
};

type Messages = Rc<RefCell<Vec<(LogLevel, String)>>>;

struct TestSink {
    messages: Messages,
}

impl MessageSink for TestSink {
    fn log(&self, level: LogLevel, message: &str) {
        self.messages.borrow_mut().push((level, message.to_owned()));
    }
}

/// `class TestLogger extends AbstractMessageLogger`.
struct TestLogger {
    inner: Rc<MessageLogger<TestSink>>,
    errors: RefCell<Vec<LogMessage>>,
    messages: Messages,
    /// O `override error(): void { throw failure; }` do último caso.
    failure: Option<Error>,
}

impl TestLogger {
    fn new(failure: Option<Error>) -> Rc<Self> {
        let messages: Messages = Rc::default();
        Rc::new(Self { inner: MessageLogger::new(TestSink { messages: messages.clone() }), errors: RefCell::default(), messages, failure })
    }
}

delegate_logger!(TestLogger, inner, error_args: |this, message, args| {
    if let Some(failure) = &this.failure {
        errors::throw(failure.clone());
    }
    this.errors.borrow_mut().push(message.clone());
    this.inner.error_args(message, args)
});

fn native(message: &str) -> Error {
    Error::new(message).with_stack(format!("Error: {message}\n    at test (log.test.ts:1:1)"))
}

type Entries = Rc<RefCell<Vec<(LogLevel, Vec<LogArg>)>>>;

fn adapter_logger(entries: &Entries) -> Rc<AdapterLogger> {
    let entries = entries.clone();
    AdapterLogger::new(move |level: LogLevel, args: &[LogArg]| entries.borrow_mut().push((level, args.to_vec())), LogLevel::Info)
}

#[test]
fn retains_unexpected_error_stacks_when_forwarding_to_another_process() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let entries: Entries = Rc::default();
    let logger = guard.add_rc(adapter_logger(&entries));
    let error = native("Unexpected error");

    create_unexpected_error_handler(logger.clone())(&UnexpectedError::Native(error.clone()));

    assert_eq!(*entries.borrow(), vec![(LogLevel::Error, vec![LogArg::from(to_error_message_of(&error, true).as_str())])]);
}

#[test]
fn preserves_string_arguments_warning_formatting_and_log_level_filtering() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let entries: Entries = Rc::default();
    let logger = guard.add_rc(adapter_logger(&entries));
    let warning = native("Warning");

    logger.error_args(&LogMessage::from("Error text"), &[LogArg::from(42)]);
    logger.warn_args(&LogMessage::from(&warning), &[]);
    logger.set_level(LogLevel::Off);
    logger.error_args(&LogMessage::from(native("Suppressed error")), &[]);

    assert_eq!(
        *entries.borrow(),
        vec![
            (LogLevel::Error, vec![LogArg::from("Error text"), LogArg::from(42)]),
            (LogLevel::Warning, vec![LogArg::from(warning.message.as_str())]),
        ]
    );
}

#[test]
fn preserves_native_errors_for_debugger_source_mapping() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let logger = guard.add_rc(TestLogger::new(None));
    let error = native("Unexpected error").with_prop("cause", "Original cause");

    create_unexpected_error_handler(logger.clone() as LoggerRef)(&UnexpectedError::Native(error.clone()));

    assert_eq!(logger.errors.borrow()[0], LogMessage::Error(error));
}

#[test]
fn retains_the_original_stack_in_message_loggers() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let logger = guard.add_rc(TestLogger::new(None));
    let error = native("Unexpected error");
    let stack = error.stack.clone();

    create_unexpected_error_handler(logger.clone() as LoggerRef)(&UnexpectedError::Native(error.clone()));

    assert_eq!((logger.messages.borrow().clone(), error.stack), (vec![(LogLevel::Error, stack.clone().unwrap())], stack));
}

#[test]
fn preserves_formatting_for_stackless_errors_and_non_native_values() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let logger = guard.add_rc(TestLogger::new(None));
    let stackless = Error::new("Missing stack");
    let empty_stack = Error::new("Empty stack").with_stack("");
    let serialized = Dynamic::object([
        ("message", Dynamic::string("Serialized error")),
        ("stack", Dynamic::string("Error: Serialized error\n    at generated.js:1:2")),
    ]);
    let nested = Dynamic::object([(
        "detail",
        Dynamic::object([(
            "exception",
            Dynamic::object([
                ("message", Dynamic::string("Nested error")),
                ("stack", Dynamic::string("Error: Nested error\n    at generated.js:3:4")),
            ]),
        )]),
    )]);
    let list = Dynamic::array([error_to_dynamic(&native("First error")), error_to_dynamic(&native("Second error"))]);
    let errors = [
        UnexpectedError::Native(stackless),
        UnexpectedError::Native(empty_stack),
        UnexpectedError::Value(Dynamic::string("Error text")),
        UnexpectedError::Value(serialized),
        UnexpectedError::Value(nested),
        UnexpectedError::Value(list),
        UnexpectedError::Value(Dynamic::Undefined),
    ];
    let handle_error = create_unexpected_error_handler(logger.clone() as LoggerRef);

    errors.iter().for_each(&handle_error);

    let expected: Vec<LogMessage> = errors
        .iter()
        .map(|error| {
            LogMessage::Text(match error {
                UnexpectedError::Native(error) => to_error_message_of(error, true),
                UnexpectedError::Value(value) => to_error_message(value, true),
            })
        })
        .collect();
    assert_eq!(*logger.errors.borrow(), expected);
}

#[test]
fn suppresses_duplicates_through_1000ms_without_extending_the_interval() {
    async_test(|d| async move {
        let logger = d.add_rc(TestLogger::new(None));
        let error = UnexpectedError::Native(native("Repeated error"));
        let handle_error = create_unexpected_error_handler(logger.clone() as LoggerRef);

        handle_error(&error);
        timeout(1000).await.unwrap();
        handle_error(&error);
        timeout(1).await.unwrap();
        handle_error(&error);

        let UnexpectedError::Native(native_error) = error else { unreachable!() };
        assert_eq!(*logger.errors.borrow(), vec![LogMessage::Error(native_error.clone()), LogMessage::Error(native_error)]);
    });
}

#[test]
fn compares_the_complete_formatted_stack_when_suppressing_duplicates() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let logger = guard.add_rc(TestLogger::new(None));
    let first_error = Error::new("Same message").with_stack("Error: Same message\n    at first (log.test.ts:1:1)");
    let second_error = Error::new("Same message").with_stack("Error: Same message\n    at second (log.test.ts:2:2)");
    let handle_error = create_unexpected_error_handler(logger.clone() as LoggerRef);

    handle_error(&UnexpectedError::Native(first_error.clone()));
    handle_error(&UnexpectedError::Native(second_error.clone()));

    assert_eq!(*logger.errors.borrow(), vec![LogMessage::Error(first_error), LogMessage::Error(second_error)]);
}

#[test]
fn keeps_duplicate_suppression_local_to_each_handler() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let logger = guard.add_rc(TestLogger::new(None));
    let error = native("Unexpected error");

    create_unexpected_error_handler(logger.clone() as LoggerRef)(&UnexpectedError::Native(error.clone()));
    create_unexpected_error_handler(logger.clone() as LoggerRef)(&UnexpectedError::Native(error.clone()));

    assert_eq!(*logger.errors.borrow(), vec![LogMessage::Error(error.clone()), LogMessage::Error(error)]);
}

#[test]
fn retains_log_level_filtering() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let logger = guard.add_rc(TestLogger::new(None));
    let handle_error = create_unexpected_error_handler(logger.clone() as LoggerRef);
    logger.set_level(LogLevel::Off);
    handle_error(&UnexpectedError::Native(native("Suppressed error")));
    logger.set_level(LogLevel::Error);
    let error = native("Logged error");
    handle_error(&UnexpectedError::Native(error.clone()));

    assert_eq!(*logger.messages.borrow(), vec![(LogLevel::Error, error.stack.unwrap())]);
}

#[test]
fn does_not_swallow_logging_failures() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let failure = Error::new("Logging failed");
    let logger = guard.add_rc(TestLogger::new(Some(failure)));
    let handle_error = create_unexpected_error_handler(logger.clone() as LoggerRef);

    let result = errors::catch(|| handle_error(&UnexpectedError::Native(native("Unexpected error"))));

    assert_eq!(result.unwrap_err().to_string(), "Error: Logging failed");
}
