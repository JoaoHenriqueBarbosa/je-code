//! Porte de `src/vs/base/test/common/iterator.test.ts` (suíte `Iterable`), caso a caso e na mesma ordem.

use je_base::arrays::OneOrMany;
use je_base::iterator;

/// O `customIterable` do original: gera um iterador novo a cada uso.
struct CustomIterable;

impl IntoIterator for &CustomIterable {
    type Item = &'static str;
    type IntoIter = std::vec::IntoIter<&'static str>;

    fn into_iter(self) -> Self::IntoIter {
        vec!["one", "two", "three"].into_iter()
    }
}

#[test]
fn first() {
    assert_eq!(iterator::first(Vec::<i32>::new()), None);
    assert_eq!(iterator::first([1]), Some(1));
    assert_eq!(iterator::first(&CustomIterable), Some("one"));
    assert_eq!(iterator::first(&CustomIterable), Some("one")); // fresh
}

#[test]
fn wrap() {
    assert_eq!(iterator::wrap(OneOrMany::One(1)).collect::<Vec<_>>(), [1]);
    assert_eq!(iterator::wrap(OneOrMany::Many(vec![1, 2, 3])).collect::<Vec<_>>(), [1, 2, 3]);
}

#[test]
fn every() {
    // Empty iterable should return true (vacuous truth)
    assert!(iterator::every(Vec::<i32>::new(), |_, _| false));

    // All elements match predicate
    assert!(iterator::every([2, 4, 6, 8], |x, _| x % 2 == 0));
    assert!(iterator::every([1, 2, 3], |x, _| x > 0));

    // Not all elements match predicate
    assert!(!iterator::every([1, 2, 3, 4], |x, _| x % 2 == 0));
    assert!(!iterator::every([1, 2, 3], |x, _| x > 2));

    // Single element - matches
    assert!(iterator::every([5], |x, _| x == 5));

    // Single element - doesn't match
    assert!(!iterator::every([5], |x, _| x == 6));

    // Test index parameter in predicate
    let numbers = [10, 11, 12, 13];
    assert!(iterator::every(numbers, |x, i| x == 10 + i as i32));
    assert!(!iterator::every(numbers, |_, i| i < 2));

    // Test early termination - predicate should not be called for all elements
    let mut call_count = 0;
    let result = iterator::every([1, 2, 3, 4, 5], |x, _| {
        call_count += 1;
        x < 3
    });
    assert!(!result);
    assert_eq!(call_count, 3); // Should stop at the third element

    // Test with truthy/falsy values
    assert!(iterator::every([1, 2, 3], |x, _| x));
    assert!(!iterator::every([1, 0, 3], |x, _| x));
    assert!(iterator::every(["a", "b", "c"], |x, _| x));
    assert!(!iterator::every(["a", "", "c"], |x, _| x));
}
