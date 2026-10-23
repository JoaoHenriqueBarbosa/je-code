//! Porte de `src/vs/base/test/common/hash.test.ts` (suíte `isStringInSample`), caso a caso e na mesma ordem.
//!
//! O arquivo original tem ainda a suíte `ConstantStringHash`, portada abaixo na mesma ordem.
//!
//! Os casos `extra_*` NÃO são do original (que não testa `hash`, `StringSHA1` nem `stringHash` com
//! valores fixos): os números esperados vêm do `hash.ts` rodado no Node e o SHA-1 do `crypto` do Node.

use je_base::JsString;
use je_base::dynamic::{Dynamic, FunctionRef};
use je_base::errors;
use je_base::hash::{ConstantStringHash, StringSha1, do_hash, hash, is_string_in_sample, string_hash};

fn u(text: &str) -> Vec<u16> {
    text.encode_utf16().collect()
}

// suite('isStringInSample')

#[test]
fn returns_stable_sample_membership() {
    let actual: Vec<bool> = (0..9).map(|index| is_string_in_sample(&u(&format!("session-{}", index + 60)), 5.0)).collect();
    assert_eq!(actual, [false, false, true, true, true, true, true, false, false]);
}

#[test]
fn supports_sample_boundaries() {
    assert_eq!([is_string_in_sample(&u("session"), 0.0), is_string_in_sample(&u("session"), 100.0)], [false, true]);
}

#[test]
fn rejects_invalid_percentages() {
    assert!(errors::catch(|| is_string_in_sample(&u("session"), -1.0)).is_err());
    assert!(errors::catch(|| is_string_in_sample(&u("session"), 1.5)).is_err());
    assert!(errors::catch(|| is_string_in_sample(&u("session"), 101.0)).is_err());
}

// suite('ConstantStringHash')

#[test]
fn matches_string_hash_for_int32_accumulators() {
    let long = "x".repeat(200);
    let values: Vec<Vec<u16>> = [
        "", "a", "label", "description", "values", "value", "expectContiguousMatch", "allowNonContiguousMatches", "ünïcödé", "\u{0}\u{1}", "\u{1F680}",
        &long,
    ]
    .iter()
    .map(|s| u(s))
    .chain([vec![0xD800], vec![0xDFFF]])
    .collect();
    let mut hash_vals: Vec<i32> = vec![0, 1, -1, 149_417, 2_147_483_647, -2_147_483_648, 123_456_789];
    // `Math.imul(index + 1, 2654435761)`: o multiplicando é convertido para int32.
    hash_vals.extend((0..128).map(|index: i32| (index + 1).wrapping_mul(2_654_435_761_u32 as i32)));

    for value in &values {
        let constant = ConstantStringHash::new(value);
        for &hash_val in &hash_vals {
            assert_eq!(constant.apply(hash_val), string_hash(value, hash_val), "valor {value:?}, acumulador {hash_val}");
        }
    }
}

#[test]
fn can_be_chained_like_string_hash() {
    let chained = ConstantStringHash::new(&u("b")).apply(ConstantStringHash::new(&u("a")).apply(0));
    assert_eq!(chained, string_hash(&u("b"), string_hash(&u("a"), 0)));
}

#[test]
fn extra_string_hash_matches_node() {
    assert_eq!(string_hash(&u("hello"), 0), -6_409_527);
    assert_eq!(string_hash(&u("ünï\u{1F680}"), 7), 2_049_993_630);
}

#[test]
fn extra_hash_of_values_matches_node() {
    let value = Dynamic::object([
        (
            "b",
            Dynamic::array([
                Dynamic::Number(1.0),
                Dynamic::string("x"),
                Dynamic::Null,
                Dynamic::Bool(true),
                Dynamic::Undefined,
                Dynamic::Number(1.5),
                Dynamic::Number(-3.0),
                Dynamic::Number(1e21),
                Dynamic::Number(5_000_000_000.0),
            ]),
        ),
        ("a", Dynamic::object([("z", Dynamic::Number(2.0))])),
        ("10", Dynamic::Number(1.0)),
        ("9", Dynamic::Number(2.0)),
        ("f", Dynamic::Function(FunctionRef::native("f", 0, |_| Dynamic::Undefined))),
    ]);
    assert_eq!(hash(&value), 1_880_481_324);
    assert_eq!(do_hash(&Dynamic::array([Dynamic::Number(1e10), Dynamic::Number(2.5), Dynamic::Number(1e21)]), 12345), -1_065_222_144);
    // `NaN | 0` zera o acumulador.
    assert_eq!(do_hash(&Dynamic::Number(f64::NAN), 5), 0);
    // `Date` e `RegExp` são objetos sem chaves próprias.
    assert_eq!(do_hash(&Dynamic::date(0.0), 3), 181_480);
    assert_eq!(do_hash(&Dynamic::regexp("a", ""), 3), 181_480);
}

fn sha1_of(units: Vec<u16>) -> String {
    let mut sha = StringSha1::new();
    sha.update(JsString::from_units(units).units());
    sha.digest()
}

#[test]
fn extra_string_sha1_matches_node() {
    assert_eq!(sha1_of(u("")), "da39a3ee5e6b4b0d3255bfef95601890afd80709");
    assert_eq!(sha1_of(u("abc")), "a9993e364706816aba3e25717850c26c9cd0d89d");
    assert_eq!(sha1_of(u(&"x".repeat(200))), "94218caae9904e93a3d7bf578bf4791926fc5e82");
    let mut mixed = u("a\u{1F680}b");
    mixed.push(0xD800);
    assert_eq!(sha1_of(mixed), "10a9fde45a0e852bcadaf4a1bfa45a78d140a397");
    assert_eq!(sha1_of(u(&"héllo wörld".repeat(13))), "9496fb7b782ad1d41a585128d659390cd0bc4c2a");
    assert_eq!(sha1_of(vec![0xDC00]), "9bdb77276c1852e1fb067820472812fcf6084024");

    // Em várias chamadas, inclusive com o par de surrogates partido entre elas.
    let mut sha = StringSha1::new();
    sha.update(&u("a"));
    sha.update(&[0xD83D]);
    sha.update(&[0xDE80]);
    sha.update(&u("b"));
    sha.update(&[0xD800]);
    assert_eq!(sha.digest(), "10a9fde45a0e852bcadaf4a1bfa45a78d140a397");
    assert_eq!(sha.digest(), "10a9fde45a0e852bcadaf4a1bfa45a78d140a397");
}
