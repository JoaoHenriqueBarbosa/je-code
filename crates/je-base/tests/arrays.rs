//! Porte de `src/vs/base/test/common/arrays.test.ts` (suíte `Arrays`), caso a caso e na mesma ordem.
//!
//! Pendente: `topAsync`, que testa `arrays.topAsync` (precisa de `async.ts`, de `setTimeout` do
//! `runtime` integrado a `async` e de `cancellation.rs`). Os dois `testTopAsync` do original só
//! repetem os casos de `top` com lotes de 1 e 2 elementos.

use std::cell::RefCell;
use std::rc::Rc;

use je_base::arrays::{self, ArrayQueue, Splice, compare_by, number_comparator};
use je_base::arrays_find::{self, find_first_idx_monotonous_or_arr_len};

fn compare(a: &i32, b: &i32) -> f64 {
    f64::from(a - b)
}

#[test]
fn remove_fast_without_keeping_order() {
    let mut array = vec![1, 4, 5, 7, 55, 59, 60, 61, 64, 69];
    arrays::remove_fast_without_keeping_order(&mut array, 1);
    assert_eq!(array, [1, 69, 5, 7, 55, 59, 60, 61, 64]);

    arrays::remove_fast_without_keeping_order(&mut array, 0);
    assert_eq!(array, [64, 69, 5, 7, 55, 59, 60, 61]);

    arrays::remove_fast_without_keeping_order(&mut array, 7);
    assert_eq!(array, [64, 69, 5, 7, 55, 59, 60]);
}

#[test]
fn find_first() {
    let array = [1, 4, 5, 7, 55, 59, 60, 61, 64, 69];

    let mut idx = find_first_idx_monotonous_or_arr_len(&array, |e| *e >= 0, 0, array.len());
    assert_eq!(array[idx], 1);

    idx = find_first_idx_monotonous_or_arr_len(&array, |e| *e > 1, 0, array.len());
    assert_eq!(array[idx], 4);

    idx = find_first_idx_monotonous_or_arr_len(&array, |e| *e >= 8, 0, array.len());
    assert_eq!(array[idx], 55);

    idx = find_first_idx_monotonous_or_arr_len(&array, |e| *e >= 61, 0, array.len());
    assert_eq!(array[idx], 61);

    idx = find_first_idx_monotonous_or_arr_len(&array, |e| *e >= 69, 0, array.len());
    assert_eq!(array[idx], 69);

    idx = find_first_idx_monotonous_or_arr_len(&array, |e| *e >= 70, 0, array.len());
    assert_eq!(idx, array.len());

    idx = find_first_idx_monotonous_or_arr_len(&[], |e: &i32| *e >= 0, 0, 0);
    assert_eq!(array[idx], 1);
}

#[test]
fn quick_select() {
    fn assert_median(expected: i32, data: &[i32], nth: Option<usize>) {
        let nth = nth.unwrap_or(data.len() / 2);
        let actual1 = arrays::quick_select(nth, data, compare);
        assert_eq!(actual1, expected);

        let mut sorted = data.to_vec();
        sorted.sort_by(|a, b| arrays::to_ordering(compare(a, b)));
        assert_eq!(sorted[nth], expected);
    }

    assert_median(5, &[9, 1, 0, 2, 3, 4, 6, 8, 7, 10, 5], None);
    assert_median(8, &[9, 1, 0, 2, 3, 4, 6, 8, 7, 10, 5], Some(8));
    assert_median(8, &[13, 4, 8], None);
    assert_median(4, &[13, 4, 8, 4, 4], None);
    assert_median(13, &[13, 4, 8], Some(2));
}

fn mk_splice(start: usize, delete_count: usize, to_insert: &[i32]) -> Splice<i32> {
    Splice { start, delete_count, to_insert: to_insert.to_vec() }
}

#[test]
fn sorted_diff() {
    let mut d = arrays::sorted_diff(&[1, 2, 4], &[], compare);
    assert_eq!(d, [mk_splice(0, 3, &[])]);

    d = arrays::sorted_diff(&[], &[1, 2, 4], compare);
    assert_eq!(d, [mk_splice(0, 0, &[1, 2, 4])]);

    d = arrays::sorted_diff(&[1, 2, 4], &[1, 2, 4], compare);
    assert_eq!(d, []);

    d = arrays::sorted_diff(&[1, 2, 4], &[2, 3, 4, 5], compare);
    assert_eq!(d, [mk_splice(0, 1, &[]), mk_splice(2, 0, &[3]), mk_splice(3, 0, &[5])]);

    d = arrays::sorted_diff(&[2, 3, 4, 5], &[1, 2, 4], compare);
    assert_eq!(d, [mk_splice(0, 0, &[1]), mk_splice(1, 1, &[]), mk_splice(3, 1, &[])]);

    d = arrays::sorted_diff(&[1, 3, 5, 7], &[5, 9, 11], compare);
    assert_eq!(d, [mk_splice(0, 2, &[]), mk_splice(3, 1, &[9, 11])]);

    d = arrays::sorted_diff(&[1, 3, 7], &[5, 9, 11], compare);
    assert_eq!(d, [mk_splice(0, 3, &[5, 9, 11])]);
}

#[test]
fn delta_sorted_arrays() {
    let mut d = arrays::delta(&[1, 2, 4], &[], compare);
    assert_eq!(d.removed, [1, 2, 4]);
    assert_eq!(d.added, Vec::<i32>::new());

    d = arrays::delta(&[], &[1, 2, 4], compare);
    assert_eq!(d.removed, Vec::<i32>::new());
    assert_eq!(d.added, [1, 2, 4]);

    d = arrays::delta(&[1, 2, 4], &[1, 2, 4], compare);
    assert_eq!(d.removed, Vec::<i32>::new());
    assert_eq!(d.added, Vec::<i32>::new());

    d = arrays::delta(&[1, 2, 4], &[2, 3, 4, 5], compare);
    assert_eq!(d.removed, [1]);
    assert_eq!(d.added, [3, 5]);

    d = arrays::delta(&[2, 3, 4, 5], &[1, 2, 4], compare);
    assert_eq!(d.removed, [3, 5]);
    assert_eq!(d.added, [1]);

    d = arrays::delta(&[1, 3, 5, 7], &[5, 9, 11], compare);
    assert_eq!(d.removed, [1, 3, 7]);
    assert_eq!(d.added, [9, 11]);

    d = arrays::delta(&[1, 3, 7], &[5, 9, 11], compare);
    assert_eq!(d.removed, [1, 3, 7]);
    assert_eq!(d.added, [5, 9, 11]);
}

#[test]
fn binary_search() {
    let array = [1, 4, 5, 7, 55, 59, 60, 61, 64, 69];

    assert_eq!(arrays::binary_search(&array, &1, compare), 0);
    assert_eq!(arrays::binary_search(&array, &5, compare), 2);

    // insertion point
    assert_eq!(arrays::binary_search(&array, &0, compare), !0);
    assert_eq!(arrays::binary_search(&array, &6, compare), !3);
    assert_eq!(arrays::binary_search(&array, &70, compare), !10);
}

#[test]
fn binary_search2() {
    let array = [1, 4, 5, 7, 55, 59, 60, 61, 64, 69];
    let compare_to = |key: i32| move |index: usize| f64::from(array[index] - key);

    assert_eq!(arrays::binary_search2(10, compare_to(1)), 0);
    assert_eq!(arrays::binary_search2(10, compare_to(5)), 2);

    // insertion point
    assert_eq!(arrays::binary_search2(10, compare_to(0)), !0);
    assert_eq!(arrays::binary_search2(10, compare_to(6)), !3);
    assert_eq!(arrays::binary_search2(10, compare_to(70)), !10);
    assert_eq!(arrays::binary_search2(2, compare_to(5)), !2);
}

#[test]
fn distinct() {
    let by_self = |a: &&str| a.to_string();

    assert_eq!(arrays::distinct(&["32", "4", "5"], by_self), ["32", "4", "5"]);
    assert_eq!(arrays::distinct(&["32", "4", "5", "4"], by_self), ["32", "4", "5"]);
    assert_eq!(arrays::distinct(&["32", "constructor", "5", "1"], by_self), ["32", "constructor", "5", "1"]);
    assert_eq!(arrays::distinct(&["32", "constructor", "proto", "proto", "constructor"], by_self), ["32", "constructor", "proto"]);
    assert_eq!(arrays::distinct(&["32", "4", "5", "32", "4", "5", "32", "4", "5", "5"], by_self), ["32", "4", "5"]);
}

#[test]
fn top() {
    assert_eq!(arrays::top(&[], compare, 1), Vec::<i32>::new());
    assert_eq!(arrays::top(&[1], compare, 0), Vec::<i32>::new());
    assert_eq!(arrays::top(&[1, 2], compare, 1), [1]);
    assert_eq!(arrays::top(&[2, 1], compare, 1), [1]);
    assert_eq!(arrays::top(&[1, 3, 2], compare, 2), [1, 2]);
    assert_eq!(arrays::top(&[3, 2, 1], compare, 3), [1, 2, 3]);
    assert_eq!(arrays::top(&[4, 6, 2, 7, 8, 3, 5, 1], compare, 3), [1, 2, 3]);
}

// Pendente: topAsync (ver o topo do arquivo).

/// `b[10] = 1; b[20] = 2; ...`: um array esparso, com `None` nos buracos.
fn sparse(entries: &[(usize, i32)]) -> Vec<Option<i32>> {
    let len = entries.iter().map(|(i, _)| i + 1).max().unwrap_or(0);
    let mut array = vec![None; len];
    for &(index, value) in entries {
        array[index] = Some(value);
    }
    array
}

#[test]
fn coalesce() {
    let a = arrays::coalesce(&[None, Some(1), None, Some(2), Some(3)]);
    assert_eq!(a.len(), 3);
    assert_eq!(a[0], 1);
    assert_eq!(a[1], 2);
    assert_eq!(a[2], 3);

    arrays::coalesce(&[None, Some(1), None, None, None, Some(2), Some(3)]);
    assert_eq!(a.len(), 3);
    assert_eq!(a[0], 1);
    assert_eq!(a[1], 2);
    assert_eq!(a[2], 3);

    let b = arrays::coalesce(&sparse(&[(10, 1), (20, 2), (30, 3)]));
    assert_eq!(b.len(), 3);
    assert_eq!(b[0], 1);
    assert_eq!(b[1], 2);
    assert_eq!(b[2], 3);

    let array = sparse(&[(0, 1), (1, 1), (17, 1), (1000, 1), (1001, 1)]);
    assert_eq!(array.len(), 1002);
    let array = arrays::coalesce(&array);
    assert_eq!(array.len(), 5);
}

#[test]
fn coalesce_inplace() {
    let mut a = vec![None, Some(1), None, Some(2), Some(3)];
    arrays::coalesce_in_place(&mut a);
    assert_eq!(a.len(), 3);
    assert_eq!(a[0], Some(1));
    assert_eq!(a[1], Some(2));
    assert_eq!(a[2], Some(3));

    a = vec![None, Some(1), None, None, None, Some(2), Some(3)];
    arrays::coalesce_in_place(&mut a);
    assert_eq!(a.len(), 3);
    assert_eq!(a[0], Some(1));
    assert_eq!(a[1], Some(2));
    assert_eq!(a[2], Some(3));

    let mut b = sparse(&[(10, 1), (20, 2), (30, 3)]);
    arrays::coalesce_in_place(&mut b);
    assert_eq!(b.len(), 3);
    assert_eq!(b[0], Some(1));
    assert_eq!(b[1], Some(2));
    assert_eq!(b[2], Some(3));

    let mut array = sparse(&[(0, 1), (1, 1), (17, 1), (1000, 1), (1001, 1)]);
    assert_eq!(array.len(), 1002);
    arrays::coalesce_in_place(&mut array);
    assert_eq!(array.len(), 5);
}

#[test]
fn insert_remove() {
    let array: Rc<RefCell<Vec<String>>> = Rc::new(RefCell::new(Vec::new()));
    let remove = arrays::insert(&array, "foo".to_string());
    assert_eq!(array.borrow()[0], "foo");

    remove();
    assert_eq!(array.borrow().len(), 0);
}

#[test]
fn splice() {
    // negative start index, absolute value greater than the length
    let mut array = vec![1, 2, 3, 4, 5];
    arrays::splice(&mut array, -6, 3, &[6, 7]);
    assert_eq!(array.len(), 4);
    assert_eq!(array[0], 6);
    assert_eq!(array[1], 7);
    assert_eq!(array[2], 4);
    assert_eq!(array[3], 5);

    // negative start index, absolute value less than the length
    array = vec![1, 2, 3, 4, 5];
    arrays::splice(&mut array, -3, 3, &[6, 7]);
    assert_eq!(array.len(), 4);
    assert_eq!(array[0], 1);
    assert_eq!(array[1], 2);
    assert_eq!(array[2], 6);
    assert_eq!(array[3], 7);

    // Start index less than the length
    array = vec![1, 2, 3, 4, 5];
    arrays::splice(&mut array, 3, 3, &[6, 7]);
    assert_eq!(array.len(), 5);
    assert_eq!(array[0], 1);
    assert_eq!(array[1], 2);
    assert_eq!(array[2], 3);
    assert_eq!(array[3], 6);
    assert_eq!(array[4], 7);

    // Start index greater than the length
    array = vec![1, 2, 3, 4, 5];
    arrays::splice(&mut array, 6, 3, &[6, 7]);
    assert_eq!(array.len(), 7);
    assert_eq!(array[0], 1);
    assert_eq!(array[1], 2);
    assert_eq!(array[2], 3);
    assert_eq!(array[3], 4);
    assert_eq!(array[4], 5);
    assert_eq!(array[5], 6);
    assert_eq!(array[6], 7);
}

struct V {
    v: i32,
}

fn max_by_array() -> Vec<V> {
    [3, 5, 2, 2, 2, 5].map(|v| V { v }).into()
}

fn index_of(array: &[V], found: Option<&V>) -> Option<usize> {
    found.and_then(|f| array.iter().position(|x| std::ptr::eq(x, f)))
}

fn by_v() -> impl Fn(&V, &V) -> f64 {
    compare_by(|v: &V| f64::from(v.v), number_comparator)
}

#[test]
fn find_max_by() {
    let array = max_by_array();
    assert_eq!(index_of(&array, arrays_find::find_first_max(&array, by_v())), Some(1));
}

#[test]
fn find_last_max_by() {
    let array = max_by_array();
    assert_eq!(index_of(&array, arrays_find::find_last_max(&array, by_v())), Some(5));
}

#[test]
fn find_min_by() {
    let array = max_by_array();
    assert_eq!(index_of(&array, arrays_find::find_first_min(&array, by_v())), Some(2));
}

// suite('ArrayQueue') / suite('takeWhile/takeFromEndWhile')

#[test]
fn array_queue_take_while_1() {
    let items = [9, 8, 1, 7, 6];
    let mut queue1 = ArrayQueue::new(&items);
    assert_eq!(queue1.take_while(|x| *x > 5), Some(vec![9, 8]));
    assert_eq!(queue1.take_while(|x| *x < 7), Some(vec![1]));
    assert_eq!(queue1.take_while(|_| true), Some(vec![7, 6]));
}

#[test]
fn array_queue_take_from_end_while_1() {
    let items = [9, 8, 1, 7, 6];
    let mut queue1 = ArrayQueue::new(&items);
    assert_eq!(queue1.take_from_end_while(|x| *x > 5), Some(vec![7, 6]));
    assert_eq!(queue1.take_from_end_while(|x| *x < 2), Some(vec![1]));
    assert_eq!(queue1.take_from_end_while(|_| true), Some(vec![9, 8]));
}

#[test]
fn array_queue_take_while_and_take_from_end_while_mixed() {
    let items = [1, 2, 3, 4, 5];
    let mut queue1 = ArrayQueue::new(&items);
    assert_eq!(queue1.take_from_end_while(|x| *x > 3), Some(vec![4, 5]));
    assert_eq!(queue1.take_while(|x| *x > 0), Some(vec![1, 2, 3]));

    let mut queue2 = ArrayQueue::new(&items);
    assert_eq!(queue2.take_while(|x| *x < 3), Some(vec![1, 2]));
    assert_eq!(queue2.take_from_end_while(|x| *x > 0), Some(vec![3, 4, 5]));
}

// suite('takeWhile/takeFromEndWhile monotonous')

fn test_monotonous(array: &[i32], predicate: impl Fn(i32) -> bool) {
    fn normalize(arr: Vec<i32>) -> Option<Vec<i32>> {
        if arr.is_empty() { None } else { Some(arr) }
    }

    let negated_predicate = |a: i32| !predicate(a);
    let filtered: Vec<i32> = array.iter().copied().filter(|a| predicate(*a)).collect();
    let filtered_negated: Vec<i32> = array.iter().copied().filter(|a| negated_predicate(*a)).collect();

    {
        let mut queue1 = ArrayQueue::new(array);
        assert_eq!(queue1.take_while(|a| predicate(*a)), normalize(filtered.clone()));
        assert_eq!(queue1.length(), (array.len() - filtered.len()) as i64);
        assert_eq!(queue1.take_while(|_| true), normalize(filtered_negated.clone()));
    }

    {
        let mut queue3 = ArrayQueue::new(array);
        assert_eq!(queue3.take_from_end_while(|a| negated_predicate(*a)), normalize(filtered_negated.clone()));
        assert_eq!(queue3.length(), (array.len() - filtered_negated.len()) as i64);
        assert_eq!(queue3.take_from_end_while(|_| true), normalize(filtered.clone()));
    }
}

const MONOTONOUS_ARRAY: [i32; 9] = [1, 1, 1, 2, 5, 5, 7, 8, 8];
const MONOTONOUS_ARRAY2: [i32; 15] = [1, 1, 1, 2, 5, 5, 7, 8, 8, 9, 9, 9, 9, 10, 10];

#[test]
fn array_queue_monotonous_take_while_1() {
    test_monotonous(&MONOTONOUS_ARRAY, |value| value <= 1);
}

#[test]
fn array_queue_monotonous_take_while_2() {
    test_monotonous(&MONOTONOUS_ARRAY, |value| value < 5);
}

#[test]
fn array_queue_monotonous_take_while_3() {
    test_monotonous(&MONOTONOUS_ARRAY, |value| value <= 5);
}

#[test]
fn array_queue_monotonous_take_while_4() {
    test_monotonous(&MONOTONOUS_ARRAY, |_| true);
}

#[test]
fn array_queue_monotonous_take_while_5() {
    test_monotonous(&MONOTONOUS_ARRAY, |_| false);
}

#[test]
fn array_queue_monotonous_take_while_6() {
    test_monotonous(&MONOTONOUS_ARRAY2, |value| value < 10);
}

#[test]
fn array_queue_monotonous_take_while_7() {
    test_monotonous(&MONOTONOUS_ARRAY2, |value| value < 7);
}

#[test]
fn array_queue_monotonous_take_while_8() {
    test_monotonous(&MONOTONOUS_ARRAY2, |value| value < 5);
}

#[test]
fn array_queue_monotonous_take_while_empty() {
    test_monotonous(&[], |value| value <= 5);
}
