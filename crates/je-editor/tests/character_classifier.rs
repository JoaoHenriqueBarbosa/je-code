//! Porte de `src/vs/editor/test/common/core/characterClassifier.test.ts` (suíte `CharacterClassifier`), caso a caso e na mesma ordem.

#[path = "../../je-base/tests/common/mod.rs"]
mod common;

use common::ensure_no_disposables_are_leaked_in_test_suite;
use je_base::char_code;
use je_editor::character_classifier::CharacterClassifier;

#[test]
fn works() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    let mut classifier = CharacterClassifier::new(0);
    let (a, b, z) = (i32::from(char_code::a), i32::from(char_code::b), i32::from(char_code::z));

    assert_eq!(classifier.get(-1), 0);
    assert_eq!(classifier.get(0), 0);
    assert_eq!(classifier.get(a), 0);
    assert_eq!(classifier.get(b), 0);
    assert_eq!(classifier.get(z), 0);
    assert_eq!(classifier.get(255), 0);
    assert_eq!(classifier.get(1000), 0);
    assert_eq!(classifier.get(2000), 0);

    classifier.set(a, 1);
    classifier.set(z, 2);
    classifier.set(1000, 3);

    assert_eq!(classifier.get(-1), 0);
    assert_eq!(classifier.get(0), 0);
    assert_eq!(classifier.get(a), 1);
    assert_eq!(classifier.get(b), 0);
    assert_eq!(classifier.get(z), 2);
    assert_eq!(classifier.get(255), 0);
    assert_eq!(classifier.get(1000), 3);
    assert_eq!(classifier.get(2000), 0);
}
