//! Porte de `src/vs/base/test/browser/hash.test.ts` (suíte `Hash`), caso a caso e na mesma ordem.
//!
//! Divergências:
//! - o `hash.test.ts` de `common` (as suítes `isStringInSample` e `ConstantStringHash`) está em `hash.rs`;
//! - `Object.create(null)` e `new Array()` são `Dynamic::object([])` e `Dynamic::array([])`;
//! - `checkSHA1` roda o `StringSHA1` e o `hashAsync`, que aqui devolve uma `Promise` já resolvida (ver
//!   `hash::hash_async`), então o `await` é ler o resultado dela;
//! - a mensagem de `all different` usa o `Debug` do [`Dynamic`] no lugar do `JSON.stringify`.

mod common;

use common::ensure_no_disposables_are_leaked_in_test_suite;
use je_base::dynamic::Dynamic;
use je_base::hash::{HashAsyncInput, StringSha1, hash, hash_async};

fn s(text: &str) -> Dynamic {
    Dynamic::string(text)
}

fn n(value: f64) -> Dynamic {
    Dynamic::Number(value)
}

fn arr<const N: usize>(items: [Dynamic; N]) -> Dynamic {
    Dynamic::array(items)
}

fn obj<const N: usize>(entries: [(&str, Dynamic); N]) -> Dynamic {
    Dynamic::object(entries)
}

#[test]
fn string() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();

    assert_eq!(hash(&s("hello")), hash(&s("hello")));
    assert_ne!(hash(&s("hello")), hash(&s("world")));
    assert_ne!(hash(&s("hello")), hash(&s("olleh")));
    assert_ne!(hash(&s("hello")), hash(&s("Hello")));
    assert_ne!(hash(&s("hello")), hash(&s("Hello ")));
    assert_ne!(hash(&s("h")), hash(&s("H")));
    assert_ne!(hash(&s("-")), hash(&s("_")));
}

#[test]
fn number() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();

    assert_eq!(hash(&n(1.0)), hash(&n(1.0)));
    assert_ne!(hash(&n(0.0)), hash(&n(1.0)));
    assert_ne!(hash(&n(1.0)), hash(&n(-1.0)));
    assert_ne!(hash(&n(305_419_896.0)), hash(&n(4_886_718_345.0))); // 0x12345678 e 0x123456789
}

#[test]
fn boolean() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();

    assert_eq!(hash(&Dynamic::Bool(true)), hash(&Dynamic::Bool(true)));
    assert_ne!(hash(&Dynamic::Bool(true)), hash(&Dynamic::Bool(false)));
}

#[test]
fn array() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();

    assert_eq!(hash(&arr([n(1.0), n(2.0), n(3.0)])), hash(&arr([n(1.0), n(2.0), n(3.0)])));
    assert_eq!(hash(&arr([s("foo"), s("bar")])), hash(&arr([s("foo"), s("bar")])));
    assert_eq!(hash(&arr([])), hash(&arr([])));
    assert_eq!(hash(&arr([])), hash(&Dynamic::array(Vec::new())));
    assert_ne!(hash(&arr([s("foo"), s("bar")])), hash(&arr([s("bar"), s("foo")])));
    assert_ne!(hash(&arr([s("foo"), s("bar")])), hash(&arr([s("bar"), s("foo"), Dynamic::Null])));
    assert_ne!(hash(&arr([s("foo"), s("bar"), Dynamic::Null])), hash(&arr([s("bar"), s("foo"), Dynamic::Null])));
    assert_ne!(hash(&arr([s("foo"), s("bar")])), hash(&arr([s("bar"), s("foo"), Dynamic::Undefined])));
    assert_ne!(hash(&arr([s("foo"), s("bar"), Dynamic::Undefined])), hash(&arr([s("bar"), s("foo"), Dynamic::Undefined])));
    assert_ne!(hash(&arr([s("foo"), s("bar"), Dynamic::Null])), hash(&arr([s("foo"), s("bar"), Dynamic::Undefined])));
}

#[test]
fn object() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();

    assert_eq!(hash(&obj([])), hash(&obj([])));
    assert_eq!(hash(&obj([])), hash(&obj([]))); // Object.create(null)
    assert_eq!(hash(&obj([("foo", s("bar"))])), hash(&obj([("foo", s("bar"))])));
    assert_eq!(
        hash(&obj([("foo", s("bar")), ("foo2", Dynamic::Undefined)])),
        hash(&obj([("foo2", Dynamic::Undefined), ("foo", s("bar"))]))
    );
    assert_ne!(hash(&obj([("foo", s("bar"))])), hash(&obj([("foo", s("bar2"))])));
    assert_ne!(hash(&obj([])), hash(&arr([])));
}

#[test]
fn array_unexpected_collision() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();

    let a = hash(&arr([Dynamic::Undefined, Dynamic::Undefined, Dynamic::Undefined, Dynamic::Undefined, Dynamic::Undefined]));
    let position = || obj([("line", n(0.0)), ("character", n(0.0))]);
    let b = hash(&arr([Dynamic::Undefined, Dynamic::Undefined, s("HHHHHH"), arr([position(), position()]), Dynamic::Undefined]));
    assert_ne!(a, b);
}

#[test]
fn all_different() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();

    let candidates = [
        Dynamic::Null,
        Dynamic::Undefined,
        obj([]),
        arr([]),
        n(0.0),
        Dynamic::Bool(false),
        Dynamic::Bool(true),
        s(""),
        s(" "),
        arr([Dynamic::Null]),
        arr([Dynamic::Undefined]),
        arr([Dynamic::Undefined, Dynamic::Undefined]),
        obj([("", Dynamic::Undefined)]),
        obj([(" ", Dynamic::Undefined)]),
        s("ab"),
        s("ba"),
        arr([s("ab")]),
    ];
    let hashes: Vec<i32> = candidates.iter().map(hash).collect();
    for i in 0..hashes.len() {
        assert_eq!(hashes[i], hash(&candidates[i])); // verify that repeated invocation returns the same hash
        for k in (i + 1)..hashes.len() {
            assert_ne!(hashes[i], hashes[k], "Same hash {} for {:?} and {:?}", hashes[i], candidates[i], candidates[k]);
        }
    }
}

fn check_sha1(units: &[u16], expected: &str) {
    // Test with StringSHA1
    let mut sha = StringSha1::new();
    sha.update(units);
    assert_eq!(sha.digest(), expected);

    // Test with crypto.subtle
    let actual = hash_async(HashAsyncInput::Text(units)).result().expect("a promessa está resolvida").expect("sem erro");
    assert_eq!(actual, expected);
}

#[test]
fn sha1_1() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();

    check_sha1(&[0xDD56], "9bdb77276c1852e1fb067820472812fcf6084024");
}

#[test]
fn sha1_2() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();

    check_sha1(&[0xDB52], "9bdb77276c1852e1fb067820472812fcf6084024");
}

#[test]
fn sha1_3() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();

    check_sha1(&[0xDA02, 0xA44D], "9b483a471f22fe7e09d83f221871a987244bbd3f");
}

#[test]
fn sha1_4() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();

    check_sha1(&"hello".encode_utf16().collect::<Vec<u16>>(), "aaf4c61ddcc5e8a2dabede0f3b482cd9aea9434d");
}
