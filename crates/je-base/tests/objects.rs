//! Porte de `src/vs/base/test/common/objects.test.ts` (suíte `Objects`), caso a caso e na mesma ordem.
//!
//! Os valores do TypeScript (`any`) são [`Dynamic`]; ver `src/dynamic.rs`. `assert.deepStrictEqual`
//! é o `Dynamic::deep_strict_equals`. O caso `mapValues` fica fora da suíte `Objects` no original (é
//! um `test` solto no fim do arquivo) e vem por último aqui também. O caso `extra_*` NÃO é do
//! original: cobre os formatos de `JSON.stringify` que `safeStringify` precisa reproduzir
//! (números, `Date`, `RegExp`, arrays tipados, valores omitidos), com a saída conferida no Node.

use je_base::JsString;
use je_base::dynamic::{Dynamic, ObjectRef};
use je_base::objects;

fn s(text: &str) -> Dynamic {
    Dynamic::string(text)
}

fn n(value: f64) -> Dynamic {
    Dynamic::Number(value)
}

fn arr(items: Vec<Dynamic>) -> Dynamic {
    Dynamic::array(items)
}

fn obj(entries: Vec<(&str, Dynamic)>) -> Dynamic {
    Dynamic::object(entries)
}

fn nums(values: &[f64]) -> Dynamic {
    arr(values.iter().map(|v| n(*v)).collect())
}

fn key(text: &str) -> JsString {
    JsString::from(text)
}

fn check(one: &Dynamic, other: &Dynamic, msg: &str) {
    assert!(objects::equals(one, other), "{msg}");
    assert!(objects::equals(other, one), "[reverse] {msg}");
}

fn check_not(one: &Dynamic, other: &Dynamic, msg: &str) {
    assert!(!objects::equals(one, other), "{msg}");
    assert!(!objects::equals(other, one), "[reverse] {msg}");
}

#[test]
fn equals() {
    let (null, undefined) = (Dynamic::Null, Dynamic::Undefined);
    check(&null, &null, "null");
    check(&undefined, &undefined, "undefined");
    check(&n(1234.0), &n(1234.0), "numbers");
    check(&s(""), &s(""), "empty strings");
    check(&s("1234"), &s("1234"), "strings");
    check(&arr(vec![]), &arr(vec![]), "empty arrays");
    // check(['', 123], ['', 123], 'arrays');
    check(&arr(vec![nums(&[1.0, 2.0, 3.0]), nums(&[4.0, 5.0, 6.0])]), &arr(vec![nums(&[1.0, 2.0, 3.0]), nums(&[4.0, 5.0, 6.0])]), "nested arrays");
    check(&obj(vec![]), &obj(vec![]), "empty objects");
    check(&obj(vec![("a", n(1.0)), ("b", s("123"))]), &obj(vec![("a", n(1.0)), ("b", s("123"))]), "objects");
    check(&obj(vec![("a", n(1.0)), ("b", s("123"))]), &obj(vec![("b", s("123")), ("a", n(1.0))]), "objects (key order)");
    check(
        &obj(vec![("a", obj(vec![("b", n(1.0)), ("c", n(2.0))])), ("b", n(3.0))]),
        &obj(vec![("a", obj(vec![("b", n(1.0)), ("c", n(2.0))])), ("b", n(3.0))]),
        "nested objects",
    );

    check_not(&null, &undefined, "null != undefined");
    check_not(&null, &s(""), "null != empty string");
    check_not(&null, &arr(vec![]), "null != empty array");
    check_not(&null, &obj(vec![]), "null != empty object");
    check_not(&null, &n(0.0), "null != zero");
    check_not(&undefined, &s(""), "undefined != empty string");
    check_not(&undefined, &arr(vec![]), "undefined != empty array");
    check_not(&undefined, &obj(vec![]), "undefined != empty object");
    check_not(&undefined, &n(0.0), "undefined != zero");
    check_not(&s(""), &arr(vec![]), "empty string != empty array");
    check_not(&s(""), &obj(vec![]), "empty string != empty object");
    check_not(&s(""), &n(0.0), "empty string != zero");
    check_not(&arr(vec![]), &obj(vec![]), "empty array != empty object");
    check_not(&arr(vec![]), &n(0.0), "empty array != zero");
    check_not(&n(0.0), &arr(vec![]), "zero != empty array");

    check_not(&s("1234"), &n(1234.0), "string !== number");

    check_not(
        &arr(vec![nums(&[1.0, 2.0, 3.0]), nums(&[4.0, 5.0, 6.0])]),
        &arr(vec![nums(&[1.0, 2.0, 3.0]), nums(&[4.0, 5.0, 6000.0])]),
        "arrays",
    );
    check_not(
        &obj(vec![("a", obj(vec![("b", n(1.0)), ("c", n(2.0))])), ("b", n(3.0))]),
        &obj(vec![("b", n(3.0)), ("a", obj(vec![("b", n(9.0)), ("c", n(2.0))]))]),
        "objects",
    );
}

#[test]
fn mixin_array() {
    let foo = obj(vec![]);
    objects::mixin(&foo, &obj(vec![("bar", nums(&[1.0, 2.0, 3.0]))]), true);

    let bar = foo.get_str("bar");
    assert!(!bar.is_nullish());
    assert!(bar.is_array());
    assert_eq!(bar.get_str("length"), n(3.0));
    assert_eq!(bar.get_str("0"), n(1.0));
    assert_eq!(bar.get_str("1"), n(2.0));
    assert_eq!(bar.get_str("2"), n(3.0));
}

#[test]
fn mixin_no_overwrite() {
    let foo = obj(vec![("bar", s("123"))]);
    let bar = obj(vec![("bar", s("456"))]);

    objects::mixin(&foo, &bar, false);

    assert_eq!(foo.get_str("bar"), s("123"));
}

#[test]
fn clone_and_change() {
    let o1 = obj(vec![("something", s("hello"))]);
    let o = obj(vec![("o1", o1.clone()), ("o2", o1)]);

    let cloned = objects::clone_and_change(&o, &|_| Dynamic::Undefined);
    assert!(cloned.deep_strict_equals(&o));
    assert!(!cloned.strict_equals(&o), "é uma cópia, não o mesmo objeto");
}

#[test]
fn safe_stringify() {
    let obj1 = ObjectRef::new();
    obj1.set(&key("friend"), Dynamic::Null);
    let obj2 = ObjectRef::new();
    obj2.set(&key("friend"), Dynamic::Null);
    obj1.set(&key("friend"), Dynamic::Object(obj2.clone()));
    obj2.set(&key("friend"), Dynamic::Object(obj1.clone()));

    let array = je_base::dynamic::ArrayRef::new(vec![n(1.0)]);
    array.push(Dynamic::Array(array.clone()));

    let circular = ObjectRef::new();
    circular.set(&key("a"), n(42.0));
    circular.set(&key("b"), Dynamic::Null);
    circular.set(&key("c"), arr(vec![Dynamic::Object(obj1), Dynamic::Object(obj2)]));
    circular.set(&key("d"), Dynamic::Null);
    circular.set(&key("e"), Dynamic::BigInt(42));
    array.push(Dynamic::Object(circular.clone()));
    circular.set(&key("b"), Dynamic::Object(circular.clone()));
    circular.set(&key("d"), Dynamic::Array(array));

    let result = objects::safe_stringify(&Dynamic::Object(circular)).expect("tem texto");
    assert_eq!(
        result,
        r#"{"a":42,"b":"[Circular]","c":[{"friend":{"friend":"[Circular]"}},{"friend":{"friend":"[Circular]"}}],"d":[1,"[Circular]","[Circular]"],"e":"[BigInt 42]"}"#
    );
}

#[test]
fn safe_stringify_does_not_treat_shared_references_as_circular() {
    let shared = obj(vec![("a", n(1.0))]);
    let value = arr(vec![shared.clone(), obj(vec![("x", shared.clone()), ("y", arr(vec![shared]))])]);
    assert_eq!(objects::safe_stringify(&value).as_deref(), Some(r#"[{"a":1},{"x":{"a":1},"y":[{"a":1}]}]"#));
}

#[test]
fn stable_stringify() {
    // Stable key order regardless of insertion order
    let a = obj(vec![("b", n(1.0)), ("a", n(2.0)), ("c", obj(vec![("y", n(1.0)), ("x", n(2.0))]))]);
    let b = obj(vec![("c", obj(vec![("x", n(2.0)), ("y", n(1.0))])), ("a", n(2.0)), ("b", n(1.0))]);
    assert_eq!(objects::stable_stringify(&a), objects::stable_stringify(&b));
    assert_eq!(objects::stable_stringify(&a), r#"{"a":2,"b":1,"c":{"x":2,"y":1}}"#);

    // Nested keys must be preserved (regression: would be dropped if a
    // global replacer-array was used).
    assert_eq!(objects::stable_stringify(&obj(vec![("outer", obj(vec![("inner", n(1.0))]))])), r#"{"outer":{"inner":1}}"#);

    // Arrays preserve order; their object elements get sorted keys.
    assert_eq!(
        objects::stable_stringify(&arr(vec![obj(vec![("b", n(1.0)), ("a", n(2.0))]), s("x"), Dynamic::Null, n(3.0)])),
        r#"[{"a":2,"b":1},"x",null,3]"#
    );

    // Primitives
    assert_eq!(objects::stable_stringify(&Dynamic::Undefined), "undefined");
    assert_eq!(objects::stable_stringify(&Dynamic::Null), "null");
    assert_eq!(objects::stable_stringify(&n(0.0)), "0");
    assert_eq!(objects::stable_stringify(&s("")), r#""""#);
    assert_eq!(objects::stable_stringify(&Dynamic::Bool(true)), "true");

    // Properties whose value is undefined are omitted (matches JSON.stringify)
    assert_eq!(objects::stable_stringify(&obj(vec![("a", n(1.0)), ("b", Dynamic::Undefined), ("c", n(3.0))])), r#"{"a":1,"c":3}"#);

    // Circular references do not throw and produce stable output
    let circular = ObjectRef::new();
    circular.set(&key("a"), n(1.0));
    circular.set(&key("self"), Dynamic::Object(circular.clone()));
    assert_eq!(objects::stable_stringify(&Dynamic::Object(circular)), r#"{"a":1,"self":"[Circular]"}"#);

    let shared = obj(vec![("a", n(1.0))]);
    assert_eq!(objects::stable_stringify(&obj(vec![("x", shared.clone()), ("y", shared.clone())])), r#"{"x":{"a":1},"y":{"a":1}}"#);
    assert_eq!(objects::stable_stringify(&arr(vec![shared.clone(), shared])), r#"[{"a":1},{"a":1}]"#);
}

fn key_count(value: &Dynamic) -> usize {
    value.own_keys().len()
}

#[test]
fn distinct() {
    let base = obj(vec![("one", s("one")), ("two", n(2.0)), ("three", obj(vec![("3", Dynamic::Bool(true))])), ("four", Dynamic::Bool(false))]);

    let mut diff = objects::distinct(&base, &base);
    assert_eq!(key_count(&diff), 0);

    let mut target = obj(vec![]);
    diff = objects::distinct(&base, &target);
    assert_eq!(key_count(&diff), 0);

    target = obj(vec![("one", s("one")), ("two", n(2.0))]);
    diff = objects::distinct(&base, &target);
    assert_eq!(key_count(&diff), 0);

    target = obj(vec![("three", obj(vec![("3", Dynamic::Bool(true))])), ("four", Dynamic::Bool(false))]);
    diff = objects::distinct(&base, &target);
    assert_eq!(key_count(&diff), 0);

    target = obj(vec![("one", s("two")), ("two", n(2.0)), ("three", obj(vec![("3", Dynamic::Bool(true))])), ("four", Dynamic::Bool(true))]);
    diff = objects::distinct(&base, &target);
    assert_eq!(key_count(&diff), 2);
    assert_eq!(diff.get_str("one"), s("two"));
    assert_eq!(diff.get_str("four"), Dynamic::Bool(true));

    target = obj(vec![("one", Dynamic::Null), ("two", n(2.0)), ("three", obj(vec![("3", Dynamic::Bool(true))])), ("four", Dynamic::Undefined)]);
    diff = objects::distinct(&base, &target);
    assert_eq!(key_count(&diff), 2);
    assert_eq!(diff.get_str("one"), Dynamic::Null);
    assert_eq!(diff.get_str("four"), Dynamic::Undefined);

    target = obj(vec![("one", s("two")), ("two", n(3.0)), ("three", obj(vec![("3", Dynamic::Bool(false))])), ("four", Dynamic::Bool(true))]);
    diff = objects::distinct(&base, &target);
    assert_eq!(key_count(&diff), 4);
    assert_eq!(diff.get_str("one"), s("two"));
    assert_eq!(diff.get_str("two"), n(3.0));
    assert_eq!(diff.get_str("three").get_str("3"), Dynamic::Bool(false));
    assert_eq!(diff.get_str("four"), Dynamic::Bool(true));
}

#[test]
fn get_case_insensitive() {
    let obj1 = obj(vec![("lowercase", n(123.0)), ("mIxEdCaSe", n(456.0))]);

    assert_eq!(obj1.get_str("lowercase"), objects::get_case_insensitive(&obj1, &key("lowercase")));
    assert_eq!(obj1.get_str("lowercase"), objects::get_case_insensitive(&obj1, &key("lOwErCaSe")));

    assert_eq!(obj1.get_str("mIxEdCaSe"), objects::get_case_insensitive(&obj1, &key("MIXEDCASE")));
    assert_eq!(obj1.get_str("mIxEdCaSe"), objects::get_case_insensitive(&obj1, &key("mixedcase")));
}

#[test]
fn map_values() {
    let value = obj(vec![("a", n(1.0)), ("b", n(2.0)), ("c", n(3.0))]);

    let result = objects::map_values(&value, |value, key| {
        let Dynamic::Number(v) = value else { panic!("número esperado") };
        s(&format!("{key}: {}", v * 2.0))
    });

    assert!(result.deep_strict_equals(&obj(vec![("a", s("a: 2")), ("b", s("b: 4")), ("c", s("c: 6"))])));
}

#[test]
fn extra_safe_stringify_matches_json_stringify() {
    let value = obj(vec![
        (
            "n",
            arr(vec![
                n(1.5),
                n(1e21),
                n(1e-7),
                n(f64::NAN),
                Dynamic::Undefined,
                Dynamic::Function(je_base::dynamic::FunctionRef::native("f", 0, |_| Dynamic::Undefined)),
                Dynamic::date(0.0),
                Dynamic::regexp("a", ""),
                Dynamic::String(JsString::from_units(vec![0x61, 0, 0xD800, 0x22, 0x0A])),
            ]),
        ),
        ("u", Dynamic::Undefined),
        ("d", Dynamic::date(f64::NAN)),
        ("t", Dynamic::typed_array(vec![1, 2])),
    ]);
    assert_eq!(
        objects::safe_stringify(&value).as_deref(),
        Some(r#"{"n":[1.5,1e+21,1e-7,null,null,null,"1970-01-01T00:00:00.000Z",{},"a\u0000\ud800\"\n"],"d":null,"t":{"0":1,"1":2}}"#)
    );
    assert_eq!(objects::safe_stringify(&Dynamic::Undefined), None);
}
