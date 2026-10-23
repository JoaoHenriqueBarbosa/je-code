//! Porte de `src/vs/base/test/common/uuid.test.ts` (suíte `UUID`), caso a caso e na mesma ordem.

use std::time::{Duration, Instant};

use je_base::uuid::{generate_uuid, is_uuid, prefixed_uuid};

#[test]
fn generation() {
    let as_hex = generate_uuid();
    assert_eq!(as_hex.len(), 36);
    assert_eq!(as_hex.as_bytes()[14], b'4');
    assert!(matches!(as_hex.as_bytes()[19], b'8' | b'9' | b'a' | b'b'));
}

#[test]
fn self_check() {
    let t1 = Instant::now();
    while t1.elapsed() < Duration::from_millis(50) {
        let value = generate_uuid();
        assert!(is_uuid(&value));
    }
}

#[test]
fn prefixed_uuid_case() {
    let namespace = "abc";
    let result = prefixed_uuid(namespace);
    assert!(result.starts_with(&format!("{namespace}-")), "Expected \"{result}\" to start with \"{namespace}-\"");

    let expected_length = namespace.len() + 1 + 36;
    assert_eq!(result.len(), expected_length);

    let uuid_part = &result[namespace.len() + 1..];
    assert!(is_uuid(uuid_part), "Expected \"{uuid_part}\" to be a valid UUID");
}
