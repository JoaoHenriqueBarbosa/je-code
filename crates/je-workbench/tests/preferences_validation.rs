//! Porte de `src/vs/workbench/services/preferences/test/common/preferencesValidation.test.ts` (suíte
//! `Preferences Validation`), caso a caso e na mesma ordem.
//!
//! Notas de tradução:
//! - Os esquemas e os valores do teste são objetos literais do JavaScript; viram [`Dynamic`] pelas funções `s`, `n`,
//!   `obj` e `arr`. `undefined` é `Dynamic::Undefined`.
//! - `ArrayTester` é idêntico ao `Tester` (só a anotação de tipo de `accepts` muda), então os casos de array usam o
//!   mesmo `Tester`.
//! - A mensagem de asserção de `getInvalidTypeError` usa `JSON.stringify` do valor, no lugar da interpolação de
//!   `${value}` (que só alimenta o texto de falha).
//! - O caso `getInvalidTypeError` se chama `get_invalid_type_error_cases` para não colidir com a função importada.
//! - `ensureNoDisposablesAreLeakedInTestSuite()` não tem o que conferir (nada aqui é descartável) e fica de fora.
#![allow(dead_code)]

use je_base::dynamic::{Dynamic, json_stringify};
use je_workbench::services::preferences::preferences_validation::{ValidatorFn, create_validator, get_invalid_type_error};

fn s(text: &str) -> Dynamic {
    Dynamic::string(text)
}

fn n(value: f64) -> Dynamic {
    Dynamic::Number(value)
}

fn b(value: bool) -> Dynamic {
    Dynamic::Bool(value)
}

fn obj(entries: Vec<(&str, Dynamic)>) -> Dynamic {
    Dynamic::object(entries)
}

fn arr(items: Vec<Dynamic>) -> Dynamic {
    Dynamic::array(items)
}

fn dump(value: &Dynamic) -> String {
    json_stringify(value).unwrap_or_else(|| "undefined".to_owned())
}

struct Tester {
    settings: Dynamic,
    validator: ValidatorFn,
}

/// O objeto `{ withMessage }` que `rejects` devolve.
struct Rejection<'a> {
    tester: &'a Tester,
    input: Dynamic,
}

impl Rejection<'_> {
    fn with_message(&self, message: &str) {
        let actual = (self.tester.validator)(&self.input);
        assert!(!actual.is_empty());
        assert!(
            actual.contains(message),
            "Expected error of {} on `{}` to contain {message}. Got {actual}.",
            dump(&self.tester.settings),
            dump(&self.input)
        );
    }
}

impl Tester {
    fn new(settings: Dynamic) -> Self {
        let validator = create_validator(&settings);
        Self { settings, validator }
    }

    fn accepts(&self, input: Dynamic) {
        let actual = (self.validator)(&input);
        assert_eq!(actual, "", "Expected {} to accept `{}`. Got {actual}.", dump(&self.settings), dump(&input));
    }

    fn rejects(&self, input: Dynamic) -> Rejection<'_> {
        assert_ne!((self.validator)(&input), "", "Expected {} to reject `{}`.", dump(&self.settings), dump(&input));
        Rejection { tester: self, input }
    }

    fn validates_numeric(&self) {
        self.accepts(s("3"));
        self.accepts(s("3."));
        self.accepts(s(".0"));
        self.accepts(s("3.0"));
        self.accepts(s(" 3.0"));
        self.accepts(s(" 3.0  "));
        self.rejects(s("3f"));
        self.accepts(n(3.0));
        self.rejects(s("test"));
    }

    fn validates_nullable_numeric(&self) {
        self.validates_numeric();
        self.accepts(n(0.0));
        self.accepts(s(""));
        self.accepts(Dynamic::Null);
        self.accepts(Dynamic::Undefined);
    }

    fn validates_non_nullable_numeric(&self) {
        self.validates_numeric();
        self.accepts(n(0.0));
        self.rejects(s(""));
        self.rejects(Dynamic::Null);
        self.rejects(Dynamic::Undefined);
    }

    fn validates_string(&self) {
        self.accepts(s("3"));
        self.accepts(s("3."));
        self.accepts(s(".0"));
        self.accepts(s("3.0"));
        self.accepts(s(" 3.0"));
        self.accepts(s(" 3.0  "));
        self.accepts(s(""));
        self.accepts(s("3f"));
        self.accepts(s("hello"));
        self.rejects(n(6.0));
    }
}

#[test]
fn exclusive_max_and_max_work_together_properly() {
    {
        let just_max = Tester::new(obj(vec![("maximum", n(5.0)), ("type", s("number"))]));
        just_max.validates_non_nullable_numeric();
        just_max.rejects(s("5.1"));
        just_max.accepts(s("5.0"));
    }
    {
        let just_e_max = Tester::new(obj(vec![("exclusiveMaximum", n(5.0)), ("type", s("number"))]));
        just_e_max.validates_non_nullable_numeric();
        just_e_max.rejects(s("5.1"));
        just_e_max.rejects(s("5.0"));
        just_e_max.accepts(s("4.999"));
    }
    {
        let both_numeric = Tester::new(obj(vec![("exclusiveMaximum", n(5.0)), ("maximum", n(4.0)), ("type", s("number"))]));
        both_numeric.validates_non_nullable_numeric();
        both_numeric.rejects(s("5.1"));
        both_numeric.rejects(s("5.0"));
        both_numeric.rejects(s("4.999"));
        both_numeric.accepts(s("4"));
    }
    {
        let both_numeric = Tester::new(obj(vec![("exclusiveMaximum", n(5.0)), ("maximum", n(6.0)), ("type", s("number"))]));
        both_numeric.validates_non_nullable_numeric();
        both_numeric.rejects(s("5.1"));
        both_numeric.rejects(s("5.0"));
        both_numeric.accepts(s("4.999"));
    }
}

#[test]
fn exclusive_min_and_min_work_together_properly() {
    {
        let just_min = Tester::new(obj(vec![("minimum", n(-5.0)), ("type", s("number"))]));
        just_min.validates_non_nullable_numeric();
        just_min.rejects(s("-5.1"));
        just_min.accepts(s("-5.0"));
    }
    {
        let just_e_min = Tester::new(obj(vec![("exclusiveMinimum", n(-5.0)), ("type", s("number"))]));
        just_e_min.validates_non_nullable_numeric();
        just_e_min.rejects(s("-5.1"));
        just_e_min.rejects(s("-5.0"));
        just_e_min.accepts(s("-4.999"));
    }
    {
        let both_numeric = Tester::new(obj(vec![("exclusiveMinimum", n(-5.0)), ("minimum", n(-4.0)), ("type", s("number"))]));
        both_numeric.validates_non_nullable_numeric();
        both_numeric.rejects(s("-5.1"));
        both_numeric.rejects(s("-5.0"));
        both_numeric.rejects(s("-4.999"));
        both_numeric.accepts(s("-4"));
    }
    {
        let both_numeric = Tester::new(obj(vec![("exclusiveMinimum", n(-5.0)), ("minimum", n(-6.0)), ("type", s("number"))]));
        both_numeric.validates_non_nullable_numeric();
        both_numeric.rejects(s("-5.1"));
        both_numeric.rejects(s("-5.0"));
        both_numeric.accepts(s("-4.999"));
    }
}

#[test]
fn multiple_of_works_for_both_integers_and_fractions() {
    {
        let only_evens = Tester::new(obj(vec![("multipleOf", n(2.0)), ("type", s("number"))]));
        only_evens.accepts(s("2.0"));
        only_evens.accepts(s("2"));
        only_evens.accepts(s("-4"));
        only_evens.accepts(s("0"));
        only_evens.accepts(s("100"));
        only_evens.rejects(s("100.1"));
        only_evens.rejects(s(""));
        only_evens.rejects(s("we"));
    }
    {
        let hacky_integers = Tester::new(obj(vec![("multipleOf", n(1.0)), ("type", s("number"))]));
        hacky_integers.accepts(s("2.0"));
        hacky_integers.rejects(s(".5"));
    }
    {
        let half_integers = Tester::new(obj(vec![("multipleOf", n(0.5)), ("type", s("number"))]));
        half_integers.accepts(s("0.5"));
        half_integers.accepts(s("1.5"));
        half_integers.rejects(s("1.51"));
    }
}

#[test]
fn integer_type_correctly_adds_a_validation() {
    {
        let integers = Tester::new(obj(vec![("multipleOf", n(1.0)), ("type", s("integer"))]));
        integers.accepts(s("02"));
        integers.accepts(s("2"));
        integers.accepts(s("20"));
        integers.rejects(s(".5"));
        integers.rejects(s("2j"));
        integers.rejects(s(""));
    }
}

#[test]
fn null_is_allowed_only_when_expected() {
    {
        let nullable_integers = Tester::new(obj(vec![("type", arr(vec![s("integer"), s("null")]))]));
        nullable_integers.accepts(s("2"));
        nullable_integers.rejects(s(".5"));
        nullable_integers.accepts(s("2.0"));
        nullable_integers.rejects(s("2j"));
        nullable_integers.accepts(s(""));
    }
    {
        let nonnullable_integers = Tester::new(obj(vec![("type", arr(vec![s("integer")]))]));
        nonnullable_integers.accepts(s("2"));
        nonnullable_integers.rejects(s(".5"));
        nonnullable_integers.accepts(s("2.0"));
        nonnullable_integers.rejects(s("2j"));
        nonnullable_integers.rejects(s(""));
    }
    {
        let nullable_numbers = Tester::new(obj(vec![("type", arr(vec![s("number"), s("null")]))]));
        nullable_numbers.accepts(s("2"));
        nullable_numbers.accepts(s(".5"));
        nullable_numbers.accepts(s("2.0"));
        nullable_numbers.rejects(s("2j"));
        nullable_numbers.accepts(s(""));
    }
    {
        let nonnullable_numbers = Tester::new(obj(vec![("type", arr(vec![s("number")]))]));
        nonnullable_numbers.accepts(s("2"));
        nonnullable_numbers.accepts(s(".5"));
        nonnullable_numbers.accepts(s("2.0"));
        nonnullable_numbers.rejects(s("2j"));
        nonnullable_numbers.rejects(s(""));
    }
}

#[test]
fn string_max_min_length_work() {
    {
        let min = Tester::new(obj(vec![("minLength", n(4.0)), ("type", s("string"))]));
        min.rejects(s("123"));
        min.accepts(s("1234"));
        min.accepts(s("12345"));
    }
    {
        let max = Tester::new(obj(vec![("maxLength", n(6.0)), ("type", s("string"))]));
        max.accepts(s("12345"));
        max.accepts(s("123456"));
        max.rejects(s("1234567"));
    }
    {
        let min_max = Tester::new(obj(vec![("minLength", n(4.0)), ("maxLength", n(6.0)), ("type", s("string"))]));
        min_max.rejects(s("123"));
        min_max.accepts(s("1234"));
        min_max.accepts(s("12345"));
        min_max.accepts(s("123456"));
        min_max.rejects(s("1234567"));
    }
}

#[test]
fn objects_work() {
    {
        let o = Tester::new(obj(vec![
            ("type", s("object")),
            ("properties", obj(vec![("a", obj(vec![("type", s("string")), ("maxLength", n(2.0))]))])),
            ("additionalProperties", b(false)),
        ]));
        o.rejects(obj(vec![("a", s("string"))]));
        o.accepts(obj(vec![("a", s("st"))]));
        o.rejects(obj(vec![("a", Dynamic::Null)]));
        o.rejects(obj(vec![("a", n(7.0))]));
        o.accepts(obj(vec![]));
        o.rejects(s("test"));
        o.rejects(n(7.0));
        o.rejects(arr(vec![n(1.0), n(2.0), n(3.0)]));
    }
    {
        let pattern = Tester::new(obj(vec![
            ("type", s("object")),
            ("patternProperties", obj(vec![("^a[a-z]$", obj(vec![("type", s("string")), ("minLength", n(2.0))]))])),
            ("additionalProperties", b(false)),
        ]));
        pattern.accepts(obj(vec![("ab", s("string"))]));
        pattern.accepts(obj(vec![("ab", s("string")), ("ac", s("hmm"))]));
        pattern.rejects(obj(vec![("ab", s("string")), ("ac", s("h"))]));
        pattern.rejects(obj(vec![("ab", s("string")), ("ac", n(99999.0))]));
        pattern.rejects(obj(vec![("abc", s("string"))]));
        pattern.rejects(obj(vec![("a0", s("string"))]));
        pattern.rejects(obj(vec![("ab", s("string")), ("bc", s("hmm"))]));
        pattern.rejects(obj(vec![("be", s("string"))]));
        pattern.rejects(obj(vec![("be", s("a"))]));
        pattern.accepts(obj(vec![]));
    }
    {
        let pattern = Tester::new(obj(vec![
            ("type", s("object")),
            ("patternProperties", obj(vec![("^#", obj(vec![("type", s("string")), ("minLength", n(3.0))]))])),
            ("additionalProperties", obj(vec![("type", s("string")), ("maxLength", n(3.0))])),
        ]));
        pattern.accepts(obj(vec![("#ab", s("string"))]));
        pattern.accepts(obj(vec![("ab", s("str"))]));
        pattern.rejects(obj(vec![("#ab", s("s"))]));
        pattern.rejects(obj(vec![("ab", n(99999.0))]));
        pattern.rejects(obj(vec![("#ab", n(99999.0))]));
        pattern.accepts(obj(vec![]));
    }
    {
        let pattern = Tester::new(obj(vec![
            ("type", s("object")),
            ("properties", obj(vec![("hello", obj(vec![("type", s("string"))]))])),
            ("additionalProperties", obj(vec![("type", s("boolean"))])),
        ]));
        pattern.accepts(obj(vec![("hello", s("world"))]));
        pattern.accepts(obj(vec![("hello", s("world")), ("bye", b(false))]));
        pattern.rejects(obj(vec![("hello", s("world")), ("bye", s("false"))]));
        pattern.rejects(obj(vec![("hello", s("world")), ("bye", n(1.0))]));
        pattern.rejects(obj(vec![("hello", s("world")), ("bye", s("world"))]));
        pattern.accepts(obj(vec![("hello", s("test"))]));
        pattern.accepts(obj(vec![]));
    }
}

#[test]
fn numerical_objects_work() {
    {
        let o = Tester::new(obj(vec![("type", s("object")), ("properties", obj(vec![("b", obj(vec![("type", s("number"))]))]))]));
        o.accepts(obj(vec![("b", n(2.5))]));
        o.accepts(obj(vec![("b", n(-2.5))]));
        o.accepts(obj(vec![("b", n(0.0))]));
        o.accepts(obj(vec![("b", s("0.12"))]));
        o.rejects(obj(vec![("b", s("abc"))]));
        o.rejects(obj(vec![("b", arr(vec![]))]));
        o.rejects(obj(vec![("b", b(false))]));
        o.rejects(obj(vec![("b", Dynamic::Null)]));
        o.rejects(obj(vec![("b", Dynamic::Undefined)]));
    }
    {
        let o = Tester::new(obj(vec![
            ("type", s("object")),
            ("properties", obj(vec![("b", obj(vec![("type", s("integer")), ("minimum", n(2.0)), ("maximum", n(5.5))]))])),
        ]));
        o.accepts(obj(vec![("b", n(2.0))]));
        o.accepts(obj(vec![("b", n(3.0))]));
        o.accepts(obj(vec![("b", s("3.0"))]));
        o.accepts(obj(vec![("b", n(5.0))]));
        o.rejects(obj(vec![("b", n(1.0))]));
        o.rejects(obj(vec![("b", n(6.0))]));
        o.rejects(obj(vec![("b", n(5.5))]));
    }
}

#[test]
fn patterns_work() {
    {
        let urls = Tester::new(obj(vec![("pattern", s("^(hello)*$")), ("type", s("string"))]));
        urls.accepts(s(""));
        urls.rejects(s("hel"));
        urls.accepts(s("hello"));
        urls.rejects(s("hellohel"));
        urls.accepts(s("hellohello"));
    }
    {
        let urls = Tester::new(obj(vec![
            ("pattern", s("^(hello)*$")),
            ("type", s("string")),
            ("patternErrorMessage", s("err: must be friendly")),
        ]));
        urls.accepts(s(""));
        urls.rejects(s("hel")).with_message("err: must be friendly");
        urls.accepts(s("hello"));
        urls.rejects(s("hellohel")).with_message("err: must be friendly");
        urls.accepts(s("hellohello"));
    }
    {
        let unicode_pattern = Tester::new(obj(vec![("type", s("string")), ("pattern", s("^[\\p{L}\\d_. -]*$")), ("minLength", n(3.0))]));
        unicode_pattern.accepts(s("_autoload"));
        unicode_pattern.rejects(s("#hash"));
        unicode_pattern.rejects(s(""));
    }
}

#[test]
fn custom_error_messages_are_shown() {
    let with_message = Tester::new(obj(vec![
        ("minLength", n(1.0)),
        ("maxLength", n(0.0)),
        ("type", s("string")),
        ("errorMessage", s("always error!")),
    ]));
    with_message.rejects(s("")).with_message("always error!");
    with_message.rejects(s(" ")).with_message("always error!");
    with_message.rejects(s("1")).with_message("always error!");
}

#[test]
fn simple_array() {
    {
        let a = Tester::new(obj(vec![("type", s("array")), ("items", obj(vec![("type", s("string"))]))]));
        a.accepts(arr(vec![]));
        a.accepts(arr(vec![s("foo")]));
        a.accepts(arr(vec![s("foo"), s("bar")]));
        a.rejects(n(76.0));
        a.rejects(arr(vec![n(6.0), s("3"), n(7.0)]));
    }
}

#[test]
fn min_max_items_array() {
    {
        let a = Tester::new(obj(vec![
            ("type", s("array")),
            ("items", obj(vec![("type", s("string"))])),
            ("minItems", n(1.0)),
            ("maxItems", n(2.0)),
        ]));
        a.rejects(arr(vec![])).with_message("Array must have at least 1 items");
        a.accepts(arr(vec![s("a")]));
        a.accepts(arr(vec![s("a"), s("a")]));
        a.rejects(arr(vec![s("a"), s("a"), s("a")])).with_message("Array must have at most 2 items");
    }
}

#[test]
fn array_of_enums() {
    {
        let a = Tester::new(obj(vec![
            ("type", s("array")),
            ("items", obj(vec![("type", s("string")), ("enum", arr(vec![s("a"), s("b")]))])),
        ]));
        a.accepts(arr(vec![s("a")]));
        a.accepts(arr(vec![s("a"), s("b")]));

        a.rejects(arr(vec![s("c")])).with_message("Value 'c' is not one of");
        a.rejects(arr(vec![s("a"), s("c")])).with_message("Value 'c' is not one of");

        a.rejects(arr(vec![s("c"), s("d")])).with_message("Value 'c' is not one of");
        a.rejects(arr(vec![s("c"), s("d")])).with_message("Value 'd' is not one of");
    }
}

#[test]
fn array_of_numbers() {
    // We accept parseable strings since the view handles strings
    {
        let a = Tester::new(obj(vec![("type", s("array")), ("items", obj(vec![("type", s("number"))]))]));
        a.accepts(arr(vec![]));
        a.accepts(arr(vec![n(2.0)]));
        a.accepts(arr(vec![n(2.0), n(3.0)]));
        a.accepts(arr(vec![s("2"), s("3")]));
        a.accepts(arr(vec![n(6.6), s("3"), n(7.0)]));
        a.rejects(n(76.0));
        a.rejects(n(7.6));
        a.rejects(arr(vec![n(6.0), s("a"), n(7.0)]));
    }
    {
        let a = Tester::new(obj(vec![
            ("type", s("array")),
            ("items", obj(vec![("type", s("integer")), ("minimum", n(-2.0)), ("maximum", n(3.0))])),
            ("maxItems", n(4.0)),
        ]));
        a.accepts(arr(vec![]));
        a.accepts(arr(vec![n(-2.0), n(3.0)]));
        a.accepts(arr(vec![n(2.0), n(3.0)]));
        a.accepts(arr(vec![s("2"), s("3")]));
        a.accepts(arr(vec![s("-2"), s("0"), s("3")]));
        a.accepts(arr(vec![s("-2"), n(0.0), s("3")]));
        a.rejects(n(2.0));
        a.rejects(n(76.0));
        a.rejects(arr(vec![n(6.0), s("3"), n(7.0)]));
        a.rejects(arr(vec![n(2.0), s("a"), n(3.0)]));
        a.rejects(arr(vec![n(-2.0), n(4.0)]));
        a.rejects(arr(vec![n(-1.2), n(2.1)]));
        a.rejects(arr(vec![n(-3.0), n(3.0)]));
        a.rejects(arr(vec![n(-3.0), n(4.0)]));
        a.rejects(arr(vec![n(2.0), n(2.0), n(2.0), n(2.0), n(2.0)]));
    }
}

#[test]
fn min_max_and_enum() {
    let a = Tester::new(obj(vec![
        ("type", s("array")),
        ("items", obj(vec![("type", s("string")), ("enum", arr(vec![s("a"), s("b")]))])),
        ("minItems", n(1.0)),
        ("maxItems", n(2.0)),
    ]));

    a.rejects(arr(vec![s("a"), s("b"), s("c")])).with_message("Array must have at most 2 items");
    a.rejects(arr(vec![s("a"), s("b"), s("c")])).with_message("Value 'c' is not one of");
}

#[test]
fn pattern() {
    let a = Tester::new(obj(vec![
        ("type", s("array")),
        ("items", obj(vec![("type", s("string")), ("pattern", s("^(hello)*$"))])),
    ]));

    a.accepts(arr(vec![s("hello")]));
    a.rejects(arr(vec![s("a")])).with_message("Value 'a' must match regex");
}

#[test]
fn unicode_pattern() {
    let a = Tester::new(obj(vec![
        ("type", s("array")),
        ("items", obj(vec![("type", s("string")), ("pattern", s("^[\\p{L}\\d_. -]*$"))])),
    ]));

    a.accepts(arr(vec![s("hello"), s("world")]));
    a.rejects(arr(vec![s("hello"), s("#world")])).with_message("Value '#world' must match regex");
}

#[test]
fn pattern_with_error_message() {
    let a = Tester::new(obj(vec![
        ("type", s("array")),
        (
            "items",
            obj(vec![("type", s("string")), ("pattern", s("^(hello)*$")), ("patternErrorMessage", s("err: must be friendly"))]),
        ),
    ]));

    a.rejects(arr(vec![s("a")])).with_message("err: must be friendly");
}

#[test]
fn unique_items() {
    let a = Tester::new(obj(vec![
        ("type", s("array")),
        ("items", obj(vec![("type", s("string"))])),
        ("uniqueItems", b(true)),
    ]));

    a.rejects(arr(vec![s("a"), s("a")])).with_message("Array has duplicate items");
}

#[test]
fn get_invalid_type_error_cases() {
    fn test_invalid_type_error(value: Dynamic, type_: Dynamic, should_validate: bool) {
        let message = format!(
            "value: {}, type: {}, expected: {}",
            dump(&value),
            dump(&type_),
            if should_validate { "valid" } else { "invalid" }
        );
        let error = get_invalid_type_error(&value, &type_);
        if should_validate {
            assert!(error.is_none(), "{message}");
        } else {
            assert!(error.is_some_and(|text| !text.is_empty()), "{message}");
        }
    }

    test_invalid_type_error(n(1.0), s("number"), true);
    test_invalid_type_error(n(1.5), s("number"), true);
    test_invalid_type_error(arr(vec![n(1.0)]), s("number"), false);
    test_invalid_type_error(s("1"), s("number"), false);
    test_invalid_type_error(obj(vec![("a", n(1.0))]), s("number"), false);
    test_invalid_type_error(Dynamic::Null, s("number"), false);

    test_invalid_type_error(s("a"), s("string"), true);
    test_invalid_type_error(s("1"), s("string"), true);
    test_invalid_type_error(arr(vec![]), s("string"), false);
    test_invalid_type_error(obj(vec![]), s("string"), false);

    test_invalid_type_error(arr(vec![n(1.0)]), s("array"), true);
    test_invalid_type_error(arr(vec![]), s("array"), true);
    test_invalid_type_error(arr(vec![obj(vec![]), arr(vec![arr(vec![])])]), s("array"), true);
    test_invalid_type_error(obj(vec![("a", arr(vec![s("a")]))]), s("array"), false);
    test_invalid_type_error(s("hello"), s("array"), false);

    test_invalid_type_error(b(true), s("boolean"), true);
    test_invalid_type_error(s("hello"), s("boolean"), false);
    test_invalid_type_error(Dynamic::Null, s("boolean"), false);
    test_invalid_type_error(arr(vec![b(true)]), s("boolean"), false);

    test_invalid_type_error(Dynamic::Null, s("null"), true);
    test_invalid_type_error(b(false), s("null"), false);
    test_invalid_type_error(arr(vec![Dynamic::Null]), s("null"), false);
    test_invalid_type_error(s("null"), s("null"), false);
}

#[test]
fn uri_checks_work() {
    let tester = Tester::new(obj(vec![("type", s("string")), ("format", s("uri"))]));
    tester.rejects(s("example.com"));
    tester.rejects(s("example.com/example"));
    tester.rejects(s("example/example.html"));
    tester.rejects(s("www.example.com"));
    tester.rejects(s(""));
    tester.rejects(s(" "));
    tester.rejects(s("example"));

    tester.accepts(s("https:"));
    tester.accepts(s("https://"));
    tester.accepts(s("https://example.com"));
    tester.accepts(s("https://www.example.com"));
}
