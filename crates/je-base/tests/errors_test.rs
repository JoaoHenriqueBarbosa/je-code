//! Porte de `src/vs/base/test/common/errors.test.ts` (suíte `Errors`), caso a caso e na mesma ordem.
//!
//! Divergências:
//! - o `Error` do JavaScript sempre tem `stack`; o do porte só tem quando o teste a põe com `with_stack`, então
//!   os casos que leem a pilha (`verbose`, serialização) criam o erro com uma pilha de exemplo, e a que o
//!   `throw new Error()` do original carrega vem do erro lançado dentro de [`errors::catch`];
//! - `error.detail.exception.message` do primeiro caso é um objeto [`Dynamic`], porque o `Error` do porte não
//!   aceita propriedade arbitrária com objeto dentro (ver `error_message.rs`);
//! - `Object.create({ code: 'inherited' })` é `Dynamic::object([])`: o [`Dynamic`] não tem cadeia de
//!   protótipos, então a propriedade herdada não existe e o resultado esperado (`undefined`) é o mesmo;
//! - `assertType(deserializedError.cause instanceof Error)` não tem equivalente: `cause` já é um `Error`
//!   pelo tipo.

mod common;

use common::ensure_no_disposables_are_leaked_in_test_suite;
use je_base::dynamic::Dynamic;
use je_base::error_message::{to_error_message, to_error_message_of};
use je_base::errors::{
    self, Error, get_error_code, transform_error_for_serialization, transform_error_from_serialization,
};

const UNKNOWN: &str = "An unknown error occurred. Please consult the log for more details.";

#[test]
fn get_error_message() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();

    assert_eq!(to_error_message(&Dynamic::string("Foo Bar"), false), "Foo Bar");
    assert_eq!(to_error_message_of(&Error::new("Foo Bar"), false), "Foo Bar");

    let error = Dynamic::object([(
        "detail",
        Dynamic::object([("exception", Dynamic::object([("message", Dynamic::string("Foo Bar"))]))]),
    )]);
    assert_eq!(to_error_message(&error, false), "Foo Bar");
    assert_eq!(to_error_message(&error, true), "Foo Bar");

    assert!(!to_error_message(&Dynamic::Undefined, false).is_empty());
    assert!(!to_error_message(&Dynamic::Null, false).is_empty());
    assert!(!to_error_message(&Dynamic::object::<&str>([]), false).is_empty());

    let thrown = errors::catch(|| errors::throw(Error::new("").with_stack("Error\n    at errors_test::get_error_message")))
        .expect_err("o corpo lança");
    assert_eq!(to_error_message_of(&thrown, false), UNKNOWN);
    assert!(to_error_message_of(&thrown, true).len() > UNKNOWN.len());
}

#[test]
fn get_error_code_cases() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();

    let inherited = Dynamic::object::<&str>([]);
    assert_eq!(
        [
            get_error_code(&Dynamic::object([("code", Dynamic::string("ENOENT"))])),
            get_error_code(&Dynamic::object([("code", Dynamic::Number(-32603.0))])),
            get_error_code(&Dynamic::object([("code", Dynamic::Bool(true))])),
            get_error_code(&inherited),
            get_error_code(&Dynamic::Undefined),
        ],
        [Some("ENOENT".to_owned()), Some("-32603".to_owned()), None, None, None]
    );
}

#[test]
fn transform_error_for_serialization_case() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();

    let error = Error::new("Test error").with_stack("Error: Test error\n    at test");
    let serialized_error = transform_error_for_serialization(&error);
    assert_eq!(serialized_error.name, "Error");
    assert_eq!(serialized_error.message, "Test error");
    assert_eq!(serialized_error.stack, error.stack);
    assert!(!serialized_error.no_telemetry);
    assert_eq!(serialized_error.cause, None);
}

#[test]
fn transform_error_with_cause_for_serialization() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();

    let cause = Error::new("Cause error").with_stack("Error: Cause error\n    at cause");
    let error = Error::new("Test error").with_stack("Error: Test error\n    at test").with_cause(cause.clone());
    let serialized_error = transform_error_for_serialization(&error);
    assert_eq!(serialized_error.name, "Error");
    assert_eq!(serialized_error.message, "Test error");
    assert_eq!(serialized_error.stack, error.stack);
    assert!(!serialized_error.no_telemetry);
    let serialized_cause = serialized_error.cause.expect("a causa é serializada");
    assert_eq!(serialized_cause.name, "Error");
    assert_eq!(serialized_cause.message, "Cause error");
    assert_eq!(serialized_cause.stack, cause.stack);
}

#[test]
fn transform_error_from_serialization_case() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();

    let serialized_error = transform_error_for_serialization(&Error::new("Test error").with_stack("Error: Test error\n    at test"));
    let error = transform_error_from_serialization(&serialized_error);
    assert_eq!(error.name, "Error");
    assert_eq!(error.message, "Test error");
    assert_eq!(error.stack, serialized_error.stack);
    assert!(error.cause.is_none());
}

#[test]
fn transform_error_with_cause_from_serialization() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();

    let cause = Error::new("Cause error").with_stack("Error: Cause error\n    at cause");
    let serialized_cause = transform_error_for_serialization(&cause);
    let error = Error::new("Test error").with_stack("Error: Test error\n    at test").with_cause(cause);
    let serialized_error = transform_error_for_serialization(&error);
    let deserialized_error = transform_error_from_serialization(&serialized_error);
    assert_eq!(deserialized_error.name, "Error");
    assert_eq!(deserialized_error.message, "Test error");
    assert_eq!(deserialized_error.stack, serialized_error.stack);
    let deserialized_cause = deserialized_error.cause.expect("a causa é reconstruída");
    assert_eq!(deserialized_cause.name, "Error");
    assert_eq!(deserialized_cause.message, "Cause error");
    assert_eq!(deserialized_cause.stack, serialized_cause.stack);
}
