//! Testes de `src/vs/base/common/errorMessage.ts`, que não tem `.test.ts` próprio no VS Code. Os valores
//! esperados são os que o `toErrorMessage` do original devolve (conferidos lendo o código, caso a caso).

use je_base::dynamic::Dynamic;
use je_base::error_message::{ErrorWithActions, MessageOrError, as_error_with_actions, create_error_with_actions, is_error_with_actions, to_error_message, to_error_message_of};
use je_base::errors::Error;

const DEFAULT: &str = "An unknown error occurred. Please consult the log for more details.";

fn obj(entries: Vec<(&str, Dynamic)>) -> Dynamic {
    Dynamic::object(entries)
}

fn s(text: &str) -> Dynamic {
    Dynamic::string(text)
}

#[test]
fn falsy_values_give_the_default_message() {
    for value in [Dynamic::Null, Dynamic::Undefined, Dynamic::Bool(false), Dynamic::Number(0.0), s("")] {
        assert_eq!(to_error_message(&value, false), DEFAULT);
    }
}

#[test]
fn strings_are_returned_as_they_are() {
    assert_eq!(to_error_message(&s("falhou"), true), "falhou");
}

#[test]
fn error_with_message_but_no_stack_gives_the_message() {
    assert_eq!(to_error_message_of(&Error::new("boom"), false), "boom");
    assert_eq!(to_error_message(&obj(vec![("message", s("plain"))]), false), "plain");
}

#[test]
fn error_without_message_gives_the_default_message() {
    assert_eq!(to_error_message_of(&Error::new(""), false), DEFAULT);
    assert_eq!(to_error_message(&obj(vec![("other", Dynamic::from(1))]), false), DEFAULT);
}

#[test]
fn verbose_appends_the_stack() {
    let error = Error::new("boom").with_stack("Error: boom\n    at x");

    assert_eq!(to_error_message_of(&error, true), "boom: Error: boom\n    at x");
    assert_eq!(to_error_message_of(&error, false), "boom");
}

#[test]
fn verbose_joins_a_stack_given_as_an_array_and_falls_back_to_stacktrace() {
    let from_array = obj(vec![("message", s("m")), ("stack", Dynamic::array([s("l1"), s("l2")]))]);
    // o `stacktrace` só é lido dentro de `exceptionToErrorMessage`, alcançada por `detail.error`
    let exception = obj(vec![("message", s("m")), ("stack", s("")), ("stacktrace", Dynamic::array([s("a"), s("b")]))]);
    let from_stacktrace = obj(vec![("detail", obj(vec![("error", exception.clone())]))]);

    assert_eq!(to_error_message(&from_array, true), "m: l1\nl2");
    assert_eq!(to_error_message(&from_stacktrace, true), "m: a\nb");
    // sem `stack` no objeto de fora, a mensagem passa direto
    assert_eq!(to_error_message(&exception, true), "m");
}

#[test]
fn arrays_use_the_first_truthy_error_and_count_the_rest() {
    let errors = Dynamic::array([Dynamic::Null, obj(vec![("message", s("first"))]), s("second"), Dynamic::Undefined]);

    assert_eq!(to_error_message(&errors, false), "first (2 errors in total)");
    assert_eq!(to_error_message(&Dynamic::array([s("only")]), false), "only");
    assert_eq!(to_error_message(&Dynamic::array([]), false), DEFAULT);
}

#[test]
fn detail_error_and_detail_exception_win_over_the_outer_message() {
    let with_error = obj(vec![("message", s("outer")), ("detail", obj(vec![("error", obj(vec![("message", s("inner")), ("stack", s("S"))]))]))]);
    let with_exception = obj(vec![("detail", obj(vec![("exception", obj(vec![("message", s("exc"))]))]))]);

    assert_eq!(to_error_message(&with_error, false), "inner");
    assert_eq!(to_error_message(&with_error, true), "inner: S");
    assert_eq!(to_error_message(&with_exception, false), "exc");
}

#[test]
fn node_system_errors_get_the_localized_wrapper() {
    let error = Error::new("ENOENT: no such file").with_prop("code", "ENOENT").with_prop("errno", -2).with_prop("syscall", "open").with_stack("stack");

    assert_eq!(to_error_message_of(&error, false), "A system error occurred (ENOENT: no such file)");
    assert_eq!(to_error_message_of(&error, true), "A system error occurred (ENOENT: no such file): stack");
}

#[test]
fn an_errno_that_is_not_a_number_is_not_a_system_error() {
    let error = Error::new("m").with_prop("code", "X").with_prop("errno", "abc").with_prop("syscall", "open");

    assert_eq!(to_error_message_of(&error, false), "m");
}

#[test]
fn unc_host_not_allowed_points_to_the_setting() {
    let error = Error::new("Host x is not allowed").with_prop("code", "ERR_UNC_HOST_NOT_ALLOWED").with_stack("stack");

    assert_eq!(
        to_error_message_of(&error, false),
        "Host x is not allowed. Please update the 'security.allowedUNCHosts' setting if you want to allow this host."
    );
}

#[test]
fn error_with_actions_keeps_the_error_and_the_actions() {
    let created = create_error_with_actions("msg".into(), vec![]);
    let reused = create_error_with_actions(MessageOrError::Error(Error::named("Custom", "m")), vec![]);

    assert_eq!((created.error.name.as_str(), created.error.message.as_str()), ("Error", "msg"));
    assert_eq!(reused.error.name, "Custom");
    assert!(is_error_with_actions(&created));
    assert!(!is_error_with_actions(&Error::new("plain")));
    let as_any: &dyn std::any::Any = &created;
    assert!(as_error_with_actions(as_any).is_some_and(|e: &ErrorWithActions| e.actions.is_empty()));
}
