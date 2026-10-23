//! Porte de `src/vs/base/test/common/assert.test.ts` (suíte `Assert`), caso a caso e na mesma ordem.

use je_base::assert::{assert as common_assert, ok};
use je_base::dynamic::Dynamic;
use je_base::errors::{self, Error};

/// `assert.throws(fn)`: o erro lançado, ou falha o teste se não lançou.
fn throws(f: impl FnOnce()) -> Error {
    errors::catch(f).expect_err("deveria lançar")
}

#[test]
fn ok_case() {
    throws(|| ok(&false, None));
    throws(|| ok(&Dynamic::Null, None));
    throws(|| ok(&Dynamic::Undefined, None));

    let error = throws(|| ok(&Dynamic::Null, Some("Foo Bar")));
    assert!(error.message.contains("Foo Bar"));

    ok(&true, None);
    ok(&"foo", None);
    ok(&Dynamic::object::<&str>([]), None);
    ok(&5, None);
}

#[test]
fn throws_a_provided_error_object_generic_error() {
    let original_error = Error::new("Oh no!");

    let thrown_error = throws(|| common_assert(false, original_error.clone()));
    assert_eq!(thrown_error, original_error, "Must throw the provided error instance.");
    assert_eq!(thrown_error.message, "Oh no!", "Must throw the provided error instance.");
}

#[test]
fn throws_a_provided_error_object_cancellation_error() {
    let original_error = Error::cancellation();

    let thrown_error = throws(|| common_assert(false, original_error.clone()));
    assert_eq!(thrown_error, original_error, "Must throw the provided error instance.");
}

#[test]
fn throws_a_provided_error_object_readonly_error() {
    let original_error = Error::readonly(Some("World"));

    let thrown_error = throws(|| common_assert(false, original_error.clone()));
    assert_eq!(thrown_error, original_error, "Must throw the provided error instance.");
    assert_eq!(thrown_error.message, "World is read-only and cannot be changed", "Must throw the provided error instance.");
}
