//! Porte de `src/vs/base/test/common/types.test.ts` (suíte `Types`), caso a caso e na mesma ordem.
//!
//! Os valores do TypeScript (`unknown`/`any`) são [`Dynamic`]; ver `src/dynamic.rs`. O `assert`
//! do Node, usado no original como exemplo de função, é a função nativa de `assert_fn()`. Casos
//! que só diferem por tipos do TypeScript (`typeCheck`, subtipos de `isOneOf`, os `@ts-expect-error`
//! de `hasKey`) rodam como no original em tempo de execução, com `typeCheck` sem efeito.
//! Os nomes dos casos aninhados levam o caminho das suítes (`is_one_of_success_string_type`)
//! porque `type`, `subtype`, `empty` e `item` se repetem.

use je_base::assert::assert as common_assert;
use je_base::dynamic::{Dynamic, FunctionRef};
use je_base::errors::{self, Error};
use je_base::types::{self, TypeConstraint, assert_defined, is_one_of, type_check};

fn undef() -> Dynamic {
    Dynamic::Undefined
}

fn null() -> Dynamic {
    Dynamic::Null
}

fn st(text: &str) -> Dynamic {
    Dynamic::string(text)
}

fn num(value: f64) -> Dynamic {
    Dynamic::Number(value)
}

fn tr() -> Dynamic {
    Dynamic::Bool(true)
}

fn fl() -> Dynamic {
    Dynamic::Bool(false)
}

fn empty_array() -> Dynamic {
    Dynamic::array([])
}

/// `[1, 2, '3']`.
fn array_1_2_3() -> Dynamic {
    Dynamic::array([num(1.0), num(2.0), st("3")])
}

fn empty_object() -> Dynamic {
    Dynamic::object::<&str>([])
}

/// `{ foo: 'bar' }`.
fn foo_bar() -> Dynamic {
    Dynamic::object([("foo", st("bar"))])
}

fn regexp_literal() -> Dynamic {
    Dynamic::regexp("test", "")
}

fn regexp_new() -> Dynamic {
    Dynamic::regexp("", "")
}

fn date() -> Dynamic {
    Dynamic::date(0.0)
}

/// O `assert` do Node, usado como exemplo de função.
fn assert_fn() -> Dynamic {
    Dynamic::Function(FunctionRef::native("assert", 2, |_| Dynamic::Undefined))
}

/// `function foo() { /**/ }`.
fn foo_fn() -> Dynamic {
    Dynamic::Function(FunctionRef::native("foo", 0, |_| Dynamic::Undefined))
}

fn throws(f: impl FnOnce()) -> Option<Error> {
    errors::catch(f).err()
}

fn assert_throws(f: impl FnOnce()) {
    assert!(throws(f).is_some(), "deveria lançar");
}

fn assert_does_not_throw(f: impl FnOnce()) {
    if let Some(error) = throws(f) {
        panic!("não deveria lançar: {error}");
    }
}

#[test]
fn is_function() {
    assert!(!types::is_function(&undef()));
    assert!(!types::is_function(&null()));
    assert!(!types::is_function(&st("foo")));
    assert!(!types::is_function(&num(5.0)));
    assert!(!types::is_function(&tr()));
    assert!(!types::is_function(&empty_array()));
    assert!(!types::is_function(&array_1_2_3()));
    assert!(!types::is_function(&empty_object()));
    assert!(!types::is_function(&foo_bar()));
    assert!(!types::is_function(&regexp_literal()));
    assert!(!types::is_function(&regexp_new()));
    assert!(!types::is_function(&date()));

    assert!(types::is_function(&assert_fn()));
    assert!(types::is_function(&foo_fn()));
}

#[test]
fn are_functions() {
    assert!(!types::are_functions(&[]));
    assert!(!types::are_functions(&[null()]));
    assert!(!types::are_functions(&[st("foo")]));
    assert!(!types::are_functions(&[num(5.0)]));
    assert!(!types::are_functions(&[tr()]));
    assert!(!types::are_functions(&[empty_array()]));
    assert!(!types::are_functions(&[array_1_2_3()]));
    assert!(!types::are_functions(&[empty_object()]));
    assert!(!types::are_functions(&[foo_bar()]));
    assert!(!types::are_functions(&[regexp_literal()]));
    assert!(!types::are_functions(&[regexp_new()]));
    assert!(!types::are_functions(&[date()]));
    assert!(!types::are_functions(&[assert_fn(), st("")]));

    assert!(types::are_functions(&[assert_fn()]));
    assert!(types::are_functions(&[assert_fn(), assert_fn()]));
    assert!(types::are_functions(&[foo_fn()]));
}

#[test]
fn is_object() {
    assert!(!types::is_object(&undef()));
    assert!(!types::is_object(&null()));
    assert!(!types::is_object(&st("foo")));
    assert!(!types::is_object(&num(5.0)));
    assert!(!types::is_object(&tr()));
    assert!(!types::is_object(&empty_array()));
    assert!(!types::is_object(&array_1_2_3()));
    assert!(!types::is_object(&regexp_literal()));
    assert!(!types::is_object(&regexp_new()));
    assert!(!types::is_function(&date()));

    assert!(!types::is_object(&assert_fn()));
    assert!(!types::is_object(&foo_fn()));

    assert!(types::is_object(&empty_object()));
    assert!(types::is_object(&foo_bar()));
}

#[test]
fn is_empty_object() {
    assert!(!types::is_empty_object(&undef()));
    assert!(!types::is_empty_object(&null()));
    assert!(!types::is_empty_object(&st("foo")));
    assert!(!types::is_empty_object(&num(5.0)));
    assert!(!types::is_empty_object(&tr()));
    assert!(!types::is_empty_object(&empty_array()));
    assert!(!types::is_empty_object(&array_1_2_3()));
    assert!(!types::is_empty_object(&regexp_literal()));
    assert!(!types::is_empty_object(&regexp_new()));
    assert!(!types::is_empty_object(&date()));

    assert!(!types::is_empty_object(&assert_fn()));
    assert!(!types::is_empty_object(&foo_fn()));

    assert!(!types::is_empty_object(&foo_bar()));

    assert!(types::is_empty_object(&empty_object()));
}

#[test]
fn is_string() {
    assert!(!types::is_string(&undef()));
    assert!(!types::is_string(&null()));
    assert!(!types::is_string(&num(5.0)));
    assert!(!types::is_string(&empty_array()));
    assert!(!types::is_string(&array_1_2_3()));
    assert!(!types::is_string(&tr()));
    assert!(!types::is_string(&empty_object()));
    assert!(!types::is_string(&regexp_literal()));
    assert!(!types::is_string(&regexp_new()));
    assert!(!types::is_string(&date()));
    assert!(!types::is_string(&assert_fn()));
    assert!(!types::is_string(&foo_fn()));
    assert!(!types::is_string(&foo_bar()));

    assert!(types::is_string(&st("foo")));
}

#[test]
fn is_string_array() {
    assert!(!types::is_string_array(&undef()));
    assert!(!types::is_string_array(&null()));
    assert!(!types::is_string_array(&num(5.0)));
    assert!(!types::is_string_array(&st("foo")));
    assert!(!types::is_string_array(&tr()));
    assert!(!types::is_string_array(&empty_object()));
    assert!(!types::is_string_array(&regexp_literal()));
    assert!(!types::is_string_array(&regexp_new()));
    assert!(!types::is_string_array(&date()));
    assert!(!types::is_string_array(&assert_fn()));
    assert!(!types::is_string_array(&foo_fn()));
    assert!(!types::is_string_array(&foo_bar()));

    assert!(!types::is_string_array(&Dynamic::array([num(1.0), num(2.0), num(3.0)])));
    assert!(!types::is_string_array(&array_1_2_3()));
    assert!(!types::is_string_array(&Dynamic::array([st("foo"), st("bar"), num(5.0)])));
    assert!(!types::is_string_array(&Dynamic::array([st("foo"), null(), st("bar")])));
    assert!(!types::is_string_array(&Dynamic::array([st("foo"), undef(), st("bar")])));

    assert!(types::is_string_array(&empty_array()));
    assert!(types::is_string_array(&Dynamic::array([st("foo")])));
    assert!(types::is_string_array(&Dynamic::array([st("foo"), st("bar")])));
    assert!(types::is_string_array(&Dynamic::array([st("foo"), st("bar"), st("baz")])));
}

#[test]
fn is_array_of() {
    // Basic non-array values
    assert!(!types::is_array_of(&undef(), types::is_string));
    assert!(!types::is_array_of(&null(), types::is_string));
    assert!(!types::is_array_of(&num(5.0), types::is_string));
    assert!(!types::is_array_of(&st("foo"), types::is_string));
    assert!(!types::is_array_of(&tr(), types::is_string));
    assert!(!types::is_array_of(&empty_object(), types::is_string));
    assert!(!types::is_array_of(&regexp_literal(), types::is_string));
    assert!(!types::is_array_of(&regexp_new(), types::is_string));
    assert!(!types::is_array_of(&date(), types::is_string));
    assert!(!types::is_array_of(&assert_fn(), types::is_string));
    assert!(!types::is_array_of(&foo_fn(), types::is_string));
    assert!(!types::is_array_of(&foo_bar(), types::is_string));

    // Arrays with wrong types
    assert!(!types::is_array_of(&Dynamic::array([num(1.0), num(2.0), num(3.0)]), types::is_string));
    assert!(!types::is_array_of(&array_1_2_3(), types::is_string));
    assert!(!types::is_array_of(&Dynamic::array([st("foo"), st("bar"), num(5.0)]), types::is_string));
    assert!(!types::is_array_of(&Dynamic::array([st("foo"), null(), st("bar")]), types::is_string));
    assert!(!types::is_array_of(&Dynamic::array([st("foo"), undef(), st("bar")]), types::is_string));

    // Valid string arrays
    assert!(types::is_array_of(&empty_array(), types::is_string));
    assert!(types::is_array_of(&Dynamic::array([st("foo")]), types::is_string));
    assert!(types::is_array_of(&Dynamic::array([st("foo"), st("bar")]), types::is_string));
    assert!(types::is_array_of(&Dynamic::array([st("foo"), st("bar"), st("baz")]), types::is_string));

    // Valid number arrays
    assert!(types::is_array_of(&empty_array(), types::is_number));
    assert!(types::is_array_of(&Dynamic::array([num(1.0)]), types::is_number));
    assert!(types::is_array_of(&Dynamic::array([num(1.0), num(2.0), num(3.0)]), types::is_number));
    assert!(!types::is_array_of(&array_1_2_3(), types::is_number));

    // Valid boolean arrays
    assert!(types::is_array_of(&empty_array(), types::is_boolean));
    assert!(types::is_array_of(&Dynamic::array([tr()]), types::is_boolean));
    assert!(types::is_array_of(&Dynamic::array([tr(), fl(), tr()]), types::is_boolean));
    assert!(!types::is_array_of(&Dynamic::array([tr(), num(1.0), fl()]), types::is_boolean));

    // Valid function arrays
    assert!(types::is_array_of(&empty_array(), types::is_function));
    assert!(types::is_array_of(&Dynamic::array([assert_fn()]), types::is_function));
    assert!(types::is_array_of(&Dynamic::array([assert_fn(), foo_fn()]), types::is_function));
    assert!(!types::is_array_of(&Dynamic::array([assert_fn(), st("foo")]), types::is_function));

    // Custom type guard
    let is_even = |n: &Dynamic| types::is_number(n) && matches!(n, Dynamic::Number(v) if v % 2.0 == 0.0);
    assert!(types::is_array_of(&empty_array(), is_even));
    assert!(types::is_array_of(&Dynamic::array([num(2.0), num(4.0), num(6.0)]), is_even));
    assert!(!types::is_array_of(&Dynamic::array([num(2.0), num(3.0), num(4.0)]), is_even));
    assert!(!types::is_array_of(&Dynamic::array([num(1.0), num(3.0), num(5.0)]), is_even));
}

#[test]
fn is_number() {
    assert!(!types::is_number(&undef()));
    assert!(!types::is_number(&null()));
    assert!(!types::is_number(&st("foo")));
    assert!(!types::is_number(&empty_array()));
    assert!(!types::is_number(&array_1_2_3()));
    assert!(!types::is_number(&tr()));
    assert!(!types::is_number(&empty_object()));
    assert!(!types::is_number(&regexp_literal()));
    assert!(!types::is_number(&regexp_new()));
    assert!(!types::is_number(&date()));
    assert!(!types::is_number(&assert_fn()));
    assert!(!types::is_number(&foo_fn()));
    assert!(!types::is_number(&foo_bar()));
    assert!(!types::is_number(&num(f64::NAN))); // parseInt('A', 10)

    assert!(types::is_number(&num(5.0)));
}

#[test]
fn is_undefined() {
    assert!(!types::is_undefined(&null()));
    assert!(!types::is_undefined(&st("foo")));
    assert!(!types::is_undefined(&empty_array()));
    assert!(!types::is_undefined(&array_1_2_3()));
    assert!(!types::is_undefined(&tr()));
    assert!(!types::is_undefined(&empty_object()));
    assert!(!types::is_undefined(&regexp_literal()));
    assert!(!types::is_undefined(&regexp_new()));
    assert!(!types::is_undefined(&date()));
    assert!(!types::is_undefined(&assert_fn()));
    assert!(!types::is_undefined(&foo_fn()));
    assert!(!types::is_undefined(&foo_bar()));

    assert!(types::is_undefined(&undef()));
}

#[test]
fn is_undefined_or_null() {
    assert!(!types::is_undefined_or_null(&st("foo")));
    assert!(!types::is_undefined_or_null(&empty_array()));
    assert!(!types::is_undefined_or_null(&array_1_2_3()));
    assert!(!types::is_undefined_or_null(&tr()));
    assert!(!types::is_undefined_or_null(&empty_object()));
    assert!(!types::is_undefined_or_null(&regexp_literal()));
    assert!(!types::is_undefined_or_null(&regexp_new()));
    assert!(!types::is_undefined_or_null(&date()));
    assert!(!types::is_undefined_or_null(&assert_fn()));
    assert!(!types::is_undefined_or_null(&foo_fn()));
    assert!(!types::is_undefined_or_null(&foo_bar()));

    assert!(types::is_undefined_or_null(&undef()));
    assert!(types::is_undefined_or_null(&null()));
}

#[test]
fn assert_is_defined_assert_are_defined() {
    assert_throws(|| {
        types::assert_returns_defined(undef());
    });
    assert_throws(|| {
        types::assert_returns_defined(null());
    });
    assert_throws(|| {
        types::assert_returns_all_defined(&[null(), undef()]);
    });
    assert_throws(|| {
        types::assert_returns_all_defined(&[tr(), undef()]);
    });
    assert_throws(|| {
        types::assert_returns_all_defined(&[undef(), fl()]);
    });

    assert_eq!(types::assert_returns_defined(tr()), tr());
    assert_eq!(types::assert_returns_defined(fl()), fl());
    assert_eq!(types::assert_returns_defined(st("Hello")), st("Hello"));
    assert_eq!(types::assert_returns_defined(st("")), st(""));

    let res = types::assert_returns_all_defined(&[num(1.0), tr(), st("Hello")]);
    assert_eq!(res[0], num(1.0));
    assert_eq!(res[1], tr());
    assert_eq!(res[2], st("Hello"));
}

// suite('assertDefined')

#[test]
fn assert_defined_should_not_throw_if_value_is_defined_bool() {
    assert_does_not_throw(|| {
        assert_defined(tr(), "Oops something happened.");
    });
}

#[test]
fn assert_defined_should_not_throw_if_value_is_defined_number() {
    assert_does_not_throw(|| {
        assert_defined(num(5.0), "Oops something happened.");
    });
}

#[test]
fn assert_defined_should_not_throw_if_value_is_defined_zero() {
    assert_does_not_throw(|| {
        assert_defined(num(0.0), "Oops something happened.");
    });
}

#[test]
fn assert_defined_should_not_throw_if_value_is_defined_string() {
    assert_does_not_throw(|| {
        assert_defined(st("some string"), "Oops something happened.");
    });
}

#[test]
fn assert_defined_should_not_throw_if_value_is_defined_empty_string() {
    assert_does_not_throw(|| {
        assert_defined(st(""), "Oops something happened.");
    });
}

/// O `assertThrows` local do original: lança com a mensagem esperada.
fn assert_throws_with_message(test_function: impl FnOnce(), error_message: &str) {
    let thrown_error = throws(test_function);
    let thrown_error = assert_defined(thrown_error, "Must throw an error.");
    assert_eq!(thrown_error.message, error_message, "Error must have correct message.");
}

#[test]
fn assert_defined_should_throw_if_value_is_null() {
    let error_message = "Uggh ohh!";

    assert_throws_with_message(
        || {
            assert_defined(null(), error_message);
        },
        error_message,
    );
}

#[test]
fn assert_defined_should_throw_if_value_is_undefined() {
    let error_message = "Oh no!";

    assert_throws_with_message(
        || {
            assert_defined(undef(), Error::new(error_message));
        },
        error_message,
    );
}

#[test]
fn assert_defined_should_throw_assertion_error_by_default() {
    let error_message = "Uggh ohh!";

    let thrown_error = throws(|| {
        assert_defined(null(), error_message);
    });
    let thrown_error = assert_defined(thrown_error, "Must throw an error.");
    assert_eq!(thrown_error.name, "Error", "Error must be an instance of `Error`.");
    assert_eq!(thrown_error.message, error_message, "Error must have correct message.");
}

#[test]
fn assert_defined_should_throw_provided_error_instance() {
    let error_message = "Oops something hapenned.";
    let error = Error::named("TestError", error_message);

    let thrown_error = throws(|| {
        assert_defined(null(), error.clone());
    });
    let thrown_error = assert_defined(thrown_error, "Must throw an error.");
    assert_eq!(thrown_error.name, "TestError", "Error must be an instance of `TestError`.");
    assert_eq!(thrown_error.message, error_message, "Error must have correct message.");
}

// suite('isOneOf') / suite('success')

#[test]
fn is_one_of_success_string_type() {
    assert_does_not_throw(|| {
        common_assert(is_one_of(&"foo", &["foo", "bar"]), "Foo must be one of: foo, bar");
    });
}

#[test]
fn is_one_of_success_string_subtype() {
    assert_does_not_throw(|| {
        let item = "hi";
        let list = ["hi", "ciao"];
        common_assert(is_one_of(&item, &list), "Hi must be one of: hi, ciao");

        type_check::<&str>(item);
    });
}

#[test]
fn is_one_of_success_number_type() {
    assert_does_not_throw(|| {
        common_assert(is_one_of(&10, &[10, 100]), "10 must be one of: 10, 100");
    });
}

#[test]
fn is_one_of_success_number_subtype() {
    assert_does_not_throw(|| {
        let item = 20;
        let list = [20, 2000];
        common_assert(is_one_of(&item, &list), "20 must be one of: 20, 2000");

        type_check::<i32>(item);
    });
}

#[test]
fn is_one_of_success_boolean_type() {
    assert_does_not_throw(|| {
        common_assert(is_one_of(&true, &[true, false]), "true must be one of: true, false");
    });

    assert_does_not_throw(|| {
        common_assert(is_one_of(&false, &[true, false]), "false must be one of: true, false");
    });
}

#[test]
fn is_one_of_success_boolean_subtype_true() {
    assert_does_not_throw(|| {
        let item = true;
        let list = [true, true];
        common_assert(is_one_of(&item, &list), "true must be one of: true, true");

        type_check::<bool>(item);
    });
}

#[test]
fn is_one_of_success_boolean_subtype_false() {
    assert_does_not_throw(|| {
        let item = false;
        let list = [false, true];
        common_assert(is_one_of(&item, &list), "false must be one of: false, true");

        type_check::<bool>(item);
    });
}

#[test]
fn is_one_of_success_undefined_type() {
    assert_does_not_throw(|| {
        common_assert(is_one_of(&undef(), &[undef()]), "undefined must be one of: undefined");
    });

    assert_does_not_throw(|| {
        common_assert(is_one_of(&undef(), &[undef()]), "undefined must be one of: void 0");
    });
}

#[test]
fn is_one_of_success_undefined_subtype() {
    assert_does_not_throw(|| {
        let item = undef();
        let list = [undef()];
        common_assert(is_one_of(&item, &list), "undefined | null must be one of: undefined");

        type_check::<Dynamic>(item);
    });
}

#[test]
fn is_one_of_success_null_type() {
    assert_does_not_throw(|| {
        common_assert(is_one_of(&null(), &[null()]), "null must be one of: null");
    });
}

#[test]
fn is_one_of_success_null_subtype() {
    assert_does_not_throw(|| {
        let item = null();
        let list = [null()];
        common_assert(is_one_of(&item, &list), "null must be one of: null");

        type_check::<Dynamic>(item);
    });
}

#[test]
fn is_one_of_success_any_item() {
    assert_does_not_throw(|| {
        let item = st("1");
        let list = [st("2"), st("1")];
        common_assert(is_one_of(&item, &list), "1 must be one of: 2, 1");

        type_check::<Dynamic>(item);
    });
}

#[test]
fn is_one_of_success_any_list() {
    assert_does_not_throw(|| {
        let item = st("5");
        let list = [st("3"), st("5"), st("2.5")];
        common_assert(is_one_of(&item, &list), "5 must be one of: 3, 5, 2.5");

        type_check::<Dynamic>(item);
    });
}

#[test]
fn is_one_of_success_any_both() {
    assert_does_not_throw(|| {
        let item = st("12");
        let list = [st("14.25"), st("7"), st("12")];
        common_assert(is_one_of(&item, &list), "12 must be one of: 14.25, 7, 12");

        type_check::<Dynamic>(item);
    });
}

#[test]
fn is_one_of_success_unknown_item() {
    assert_does_not_throw(|| {
        let item = st("1");
        let list = [st("2"), st("1")];
        common_assert(is_one_of(&item, &list), "1 must be one of: 2, 1");

        type_check::<Dynamic>(item);
    });
}

#[test]
fn is_one_of_success_unknown_both() {
    assert_does_not_throw(|| {
        let item = st("12");
        let list = [st("14.25"), st("7"), st("12")];
        common_assert(is_one_of(&item, &list), "12 must be one of: 14.25, 7, 12");

        type_check::<Dynamic>(item);
    });
}

// suite('isOneOf') / suite('failure')

#[test]
fn is_one_of_failure_string_type() {
    assert_throws(|| {
        let item = "baz";
        common_assert(is_one_of(&item, &["foo", "bar"]), "Baz must not be one of: foo, bar");
    });
}

#[test]
fn is_one_of_failure_string_subtype() {
    assert_throws(|| {
        let item = "vitannia";
        let list = ["hi", "ciao"];
        common_assert(is_one_of(&item, &list), "vitannia must be one of: hi, ciao");
    });
}

#[test]
fn is_one_of_failure_string_empty() {
    assert_throws(|| {
        let item = "vitannia";
        let list: [&str; 0] = [];
        common_assert(is_one_of(&item, &list), "vitannia must be one of: empty");
    });
}

#[test]
fn is_one_of_failure_number_type() {
    assert_throws(|| {
        common_assert(is_one_of(&19, &[10, 100]), "19 must not be one of: 10, 100");
    });
}

#[test]
fn is_one_of_failure_number_subtype() {
    assert_throws(|| {
        let item = 24;
        let list = [20, 2000];
        common_assert(is_one_of(&item, &list), "24 must not be one of: 20, 2000");
    });
}

#[test]
fn is_one_of_failure_number_empty() {
    assert_throws(|| {
        let item = 20;
        let list: [i32; 0] = [];
        common_assert(is_one_of(&item, &list), "20 must not be one of: empty");
    });
}

#[test]
fn is_one_of_failure_boolean_type() {
    assert_throws(|| {
        common_assert(is_one_of(&true, &[false]), "true must not be one of: false");
    });

    assert_throws(|| {
        common_assert(is_one_of(&false, &[true]), "false must not be one of: true");
    });
}

#[test]
fn is_one_of_failure_boolean_subtype_true() {
    assert_throws(|| {
        let item = true;
        let list = [false];
        common_assert(is_one_of(&item, &list), "true must not be one of: false");
    });
}

#[test]
fn is_one_of_failure_boolean_subtype_false() {
    assert_throws(|| {
        let item = false;
        let list = [true, true, true];
        common_assert(is_one_of(&item, &list), "false must be one of: true, true, true");
    });
}

#[test]
fn is_one_of_failure_boolean_empty() {
    assert_throws(|| {
        let item = true;
        let list: [bool; 0] = [];
        common_assert(is_one_of(&item, &list), "true must be one of: empty");
    });
}

#[test]
fn is_one_of_failure_undefined_type() {
    assert_throws(|| {
        common_assert(is_one_of(&undef(), &[]), "undefined must not be one of: empty");
    });

    assert_throws(|| {
        common_assert(is_one_of(&undef(), &[]), "void 0 must not be one of: empty");
    });
}

#[test]
fn is_one_of_failure_undefined_subtype() {
    assert_throws(|| {
        let item = undef();
        let list = [null()];
        common_assert(is_one_of(&item, &list), "undefined must be one of: null");
    });
}

#[test]
fn is_one_of_failure_undefined_empty() {
    assert_throws(|| {
        let item = undef();
        let list: [Dynamic; 0] = [];
        common_assert(is_one_of(&item, &list), "undefined must be one of: empty");
    });
}

#[test]
fn is_one_of_failure_null_type() {
    assert_throws(|| {
        common_assert(is_one_of(&null(), &[]), "null must be one of: empty");
    });
}

#[test]
fn is_one_of_failure_null_subtype() {
    assert_throws(|| {
        let item = null();
        let list: [Dynamic; 0] = [];
        common_assert(is_one_of(&item, &list), "null must be one of: empty");
    });
}

#[test]
fn is_one_of_failure_any_item() {
    assert_throws(|| {
        let item = st("1");
        let list = [st("3"), st("4")];
        common_assert(is_one_of(&item, &list), "1 must not be one of: 3, 4");
    });
}

#[test]
fn is_one_of_failure_any_list() {
    assert_throws(|| {
        let item = st("5");
        let list = [st("3"), st("6"), st("2.5")];
        common_assert(is_one_of(&item, &list), "5 must not be one of: 3, 6, 2.5");
    });
}

#[test]
fn is_one_of_failure_any_both() {
    assert_throws(|| {
        let item = st("12");
        let list = [st("14.25"), st("7"), st("15")];
        common_assert(is_one_of(&item, &list), "12 must not be one of: 14.25, 7, 15");
    });
}

#[test]
fn is_one_of_failure_any_empty() {
    assert_throws(|| {
        let item = st("25");
        let list: [Dynamic; 0] = [];
        common_assert(is_one_of(&item, &list), "25 must not be one of: empty");
    });
}

#[test]
fn is_one_of_failure_unknown_item() {
    assert_throws(|| {
        let item = st("100");
        let list = [st("12"), st("11")];
        common_assert(is_one_of(&item, &list), "100 must not be one of: 12, 11");
    });
}

#[test]
fn is_one_of_failure_unknown_both() {
    assert_throws(|| {
        let item = st("21");
        let list = [st("14.25"), st("7"), st("12")];
        common_assert(is_one_of(&item, &list), "21 must not be one of: 14.25, 7, 12");
    });
}

fn constraint(function: FunctionRef) -> Option<TypeConstraint> {
    Some(TypeConstraint::Function(function))
}

fn type_of_constraint(name: &str) -> Option<TypeConstraint> {
    Some(TypeConstraint::from(name))
}

/// `types.isUndefined`, `types.isNumber`, `types.isString` como constraint: função de um parâmetro.
fn predicate(name: &str, check: fn(&Dynamic) -> bool) -> Option<TypeConstraint> {
    constraint(FunctionRef::native(name, 1, move |args| Dynamic::Bool(check(&args[0]))))
}

#[test]
fn validate_constraints() {
    use je_base::dynamic::Builtin::{Boolean, Function, Number, String};
    let ctor = |kind| constraint(FunctionRef::builtin(kind));

    types::validate_constraints(&[num(1.0), st("test"), tr()], &[ctor(Number), ctor(String), ctor(Boolean)]);
    types::validate_constraints(&[num(1.0), st("test"), tr()], &[type_of_constraint("number"), type_of_constraint("string"), type_of_constraint("boolean")]);
    // console.log
    types::validate_constraints(&[Dynamic::Function(FunctionRef::native("log", 0, |_| Dynamic::Undefined))], &[ctor(Function)]);
    types::validate_constraints(&[undef()], &[predicate("isUndefined", types::is_undefined)]);
    types::validate_constraints(&[num(1.0)], &[predicate("isNumber", types::is_number)]);

    let foo = FunctionRef::class("Foo");
    types::validate_constraints(&[foo.instantiate()], &[constraint(foo.clone())]);

    // function isFoo(f: any) { }
    let is_foo = FunctionRef::native("isFoo", 1, |_| Dynamic::Undefined);
    assert_throws(|| types::validate_constraints(&[foo.instantiate()], &[constraint(is_foo)]));

    // function isFoo2(f: any) { return true; }
    let is_foo2 = FunctionRef::native("isFoo2", 1, |_| Dynamic::Bool(true));
    types::validate_constraints(&[foo.instantiate()], &[constraint(is_foo2)]);

    assert_throws(|| {
        types::validate_constraints(&[num(1.0), tr()], &[predicate("isNumber", types::is_number), predicate("isString", types::is_string)]);
    });
    assert_throws(|| types::validate_constraints(&[st("2")], &[predicate("isNumber", types::is_number)]));
    assert_throws(|| types::validate_constraints(&[num(1.0), st("test"), tr()], &[ctor(Number), ctor(String), ctor(Number)]));
}

// suite('hasKey')

#[test]
fn has_key_should_return_true_when_object_has_specified_key() {
    let obj = Dynamic::object([("a", st("test"))]);

    assert!(types::has_key(&obj, &["a"]));

    // After this check, TypeScript knows obj is type A
    assert_eq!(obj.get_str("a"), st("test"));
}

#[test]
fn has_key_should_return_false_when_object_does_not_have_specified_key() {
    let obj = Dynamic::object([("b", num(42.0))]);

    assert!(!types::has_key(&obj, &["a"]));
}

#[test]
fn has_key_should_work_with_multiple_keys() {
    let obj = Dynamic::object([("a", st("test")), ("b", num(42.0))]);

    assert!(types::has_key(&obj, &["a", "b"]));

    assert_eq!(obj.get_str("a"), st("test"));
    assert_eq!(obj.get_str("b"), num(42.0));
}

#[test]
fn has_key_should_return_false_if_any_key_is_missing() {
    let obj = Dynamic::object([("a", st("test"))]);

    assert!(!types::has_key(&obj, &["a", "b"]));
}

#[test]
fn has_key_should_work_with_empty_key_object() {
    let obj = Dynamic::object([("a", st("test"))]);

    // Empty key object should return true (all zero keys exist)
    assert!(types::has_key(&obj, &[]));
}

#[test]
fn has_key_should_work_with_complex_union_types() {
    let obj_a = Dynamic::object([("kind", st("a")), ("value", st("hello"))]);
    let obj_b = Dynamic::object([("kind", st("b")), ("count", num(5.0))]);

    assert!(types::has_key(&obj_a, &["value"]));
    assert!(!types::has_key(&obj_a, &["count"]));
    assert!(!types::has_key(&obj_a, &["items"]));

    assert!(!types::has_key(&obj_b, &["value"]));
    assert!(types::has_key(&obj_b, &["count"]));
    assert!(!types::has_key(&obj_b, &["items"]));
}

#[test]
fn has_key_should_handle_objects_with_optional_properties() {
    let obj1 = Dynamic::object([("a", st("test")), ("b", num(42.0))]);
    let obj2 = Dynamic::object([("a", st("test"))]);

    assert!(types::has_key(&obj1, &["a"]));
    assert!(types::has_key(&obj1, &["b"]));

    assert!(types::has_key(&obj2, &["a"]));
    assert!(!types::has_key(&obj2, &["b"]));
}

#[test]
fn has_key_should_work_with_nested_objects() {
    let obj = Dynamic::object([("data", Dynamic::object([("nested", st("test"))]))]);

    assert!(types::has_key(&obj, &["data"]));
    assert!(!types::has_key(&obj, &["value"]));
}
