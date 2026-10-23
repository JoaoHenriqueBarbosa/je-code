//! Porte de `src/vs/editor/test/common/model/bracketPairColorizer/smallImmutableSet.test.ts` (suíte `Bracket Pair Colorizer - ImmutableSet`), caso a caso e na mesma ordem.

#[path = "../../je-base/tests/common/mod.rs"]
mod common;

use common::ensure_no_disposables_are_leaked_in_test_suite;
use je_editor::bracket_pairs_text_model_part::bracket_pairs_tree::small_immutable_set::{DenseKeyProvider, SmallImmutableSet};

#[test]
fn basic() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();

    let key_provider = DenseKeyProvider::<String>::new();
    let s = |v: &str| v.to_owned();

    let empty = SmallImmutableSet::get_empty();
    let items1 = empty.add(&s("item1"), &key_provider);
    let items12 = items1.add(&s("item2"), &key_provider);
    let items2 = empty.add(&s("item2"), &key_provider);
    let items21 = items2.add(&s("item1"), &key_provider);

    let items3 = empty.add(&s("item3"), &key_provider);

    assert!(items12.intersects(&items1));
    assert!(items12.has(&s("item1"), &key_provider));

    assert!(!items12.intersects(&items3));
    assert!(!items12.has(&s("item3"), &key_provider));

    assert!(items21.equals(&items12));
    assert!(!items21.equals(&items2));
}

#[test]
fn many_elements() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();

    let key_provider = DenseKeyProvider::<String>::new();

    let mut set = SmallImmutableSet::get_empty();

    for i in 0..100 {
        key_provider.get_key(&format!("item{i}"));
        if i % 2 == 0 {
            set = set.add(&format!("item{i}"), &key_provider);
        }
    }

    for i in 0..100 {
        assert_eq!(set.has(&format!("item{i}"), &key_provider), i % 2 == 0);
    }
}
