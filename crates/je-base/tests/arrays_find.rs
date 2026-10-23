//! Porte de `src/vs/base/test/common/arraysFind.test.ts` (suíte `Arrays`), caso a caso e na mesma ordem.

use je_base::arrays_find::{MonotonousArray, find_first_monotonous, find_last_monotonous};

#[test]
fn find_last_monotonous_cases() {
    let array = [1, 4, 5, 7, 55, 59, 60, 61, 64, 69];

    let result = find_last_monotonous(&array, |n| *n <= 60);
    assert_eq!(result, Some(&60));

    let result2 = find_last_monotonous(&array, |n| *n <= 62);
    assert_eq!(result2, Some(&61));

    let result3 = find_last_monotonous(&array, |n| *n <= 1);
    assert_eq!(result3, Some(&1));

    let result4 = find_last_monotonous(&array, |n| *n <= 70);
    assert_eq!(result4, Some(&69));

    let result5 = find_last_monotonous(&array, |n| *n <= 0);
    assert_eq!(result5, None);
}

#[test]
fn find_first_monotonous_cases() {
    let array = [1, 4, 5, 7, 55, 59, 60, 61, 64, 69];

    let result = find_first_monotonous(&array, |n| *n >= 60);
    assert_eq!(result, Some(&60));

    let result2 = find_first_monotonous(&array, |n| *n >= 62);
    assert_eq!(result2, Some(&64));

    let result3 = find_first_monotonous(&array, |n| *n >= 1);
    assert_eq!(result3, Some(&1));

    let result4 = find_first_monotonous(&array, |n| *n >= 70);
    assert_eq!(result4, None);

    let result5 = find_first_monotonous(&array, |n| *n >= 0);
    assert_eq!(result5, Some(&1));
}

#[test]
fn monotonous_array() {
    let items = [1, 4, 5, 7, 55, 59, 60, 61, 64, 69];
    let mut arr = MonotonousArray::new(&items);
    assert_eq!(arr.find_last_monotonous(|n| *n <= 0), None);
    assert_eq!(arr.find_last_monotonous(|n| *n <= 0), None);
    assert_eq!(arr.find_last_monotonous(|n| *n <= 5), Some(&5));
    assert_eq!(arr.find_last_monotonous(|n| *n <= 6), Some(&5));
    assert_eq!(arr.find_last_monotonous(|n| *n <= 55), Some(&55));
    assert_eq!(arr.find_last_monotonous(|n| *n <= 60), Some(&60));
    assert_eq!(arr.find_last_monotonous(|n| *n <= 80), Some(&69));
}
