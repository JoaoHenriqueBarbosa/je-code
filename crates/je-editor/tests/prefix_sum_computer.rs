//! Porte de `src/vs/editor/test/common/viewModel/prefixSumComputer.test.ts` (suíte `Editor ViewModel - PrefixSumComputer`), caso a caso e na mesma ordem.
//!
//! Como no original, cada caso roda nas duas implementações (`forBoth`) atrás da mesma interface
//! (`IPrefixSumComputer`), em que `get_prefix_sum(count)` é a soma dos primeiros `count` valores. Os
//! números da interface são `i64`, o `number` do original.

#[path = "../../je-base/tests/common/mod.rs"]
mod common;

use common::ensure_no_disposables_are_leaked_in_test_suite;
use je_editor::prefix_sum_computer::{ConstantTimePrefixSumComputer, PrefixSumComputer, PrefixSumIndexOfResult};

trait IPrefixSumComputer {
    fn get_total_sum(&self) -> i64;
    /// A soma dos primeiros `count` valores: `SUM(0 <= j < count, values[j])`.
    fn get_prefix_sum(&self, count: i64) -> i64;
    fn get_index_of(&self, sum: i64) -> PrefixSumIndexOfResult;
    fn set_value(&mut self, index: i64, value: u32);
    fn insert_values(&mut self, insert_index: i64, insert_arr: &[u32]);
    fn remove_values(&mut self, start: i64, delete_count: i64);
}

impl IPrefixSumComputer for PrefixSumComputer {
    fn get_total_sum(&self) -> i64 {
        i64::from(PrefixSumComputer::get_total_sum(self))
    }
    fn get_prefix_sum(&self, count: i64) -> i64 {
        if count == 0 { 0 } else { i64::from(PrefixSumComputer::get_prefix_sum(self, count - 1)) }
    }
    fn get_index_of(&self, sum: i64) -> PrefixSumIndexOfResult {
        PrefixSumComputer::get_index_of(self, sum)
    }
    fn set_value(&mut self, index: i64, value: u32) {
        PrefixSumComputer::set_value(self, index as usize, value);
    }
    fn insert_values(&mut self, insert_index: i64, insert_arr: &[u32]) {
        PrefixSumComputer::insert_values(self, insert_index as usize, insert_arr);
    }
    fn remove_values(&mut self, start: i64, delete_count: i64) {
        PrefixSumComputer::remove_values(self, start as usize, delete_count as usize);
    }
}

impl IPrefixSumComputer for ConstantTimePrefixSumComputer {
    fn get_total_sum(&self) -> i64 {
        ConstantTimePrefixSumComputer::get_total_sum(self) as i64
    }
    fn get_prefix_sum(&self, count: i64) -> i64 {
        ConstantTimePrefixSumComputer::get_prefix_sum(self, count as usize) as i64
    }
    fn get_index_of(&self, sum: i64) -> PrefixSumIndexOfResult {
        ConstantTimePrefixSumComputer::get_index_of(self, sum)
    }
    fn set_value(&mut self, index: i64, value: u32) {
        ConstantTimePrefixSumComputer::set_value(self, index as usize, value);
    }
    fn insert_values(&mut self, insert_index: i64, insert_arr: &[u32]) {
        ConstantTimePrefixSumComputer::insert_values(self, insert_index as usize, insert_arr);
    }
    fn remove_values(&mut self, start: i64, delete_count: i64) {
        ConstantTimePrefixSumComputer::remove_values(self, start as usize, delete_count as usize);
    }
}

fn create_both(values: &[u32]) -> [Box<dyn IPrefixSumComputer>; 2] {
    [
        Box::new(PrefixSumComputer::new(values.to_vec())),
        Box::new(ConstantTimePrefixSumComputer::new(values.to_vec())),
    ]
}

fn for_both(values: &[u32], callback: impl Fn(&mut dyn IPrefixSumComputer)) {
    for mut psc in create_both(values) {
        callback(psc.as_mut());
    }
}

fn r(index: i64, remainder: i64) -> PrefixSumIndexOfResult {
    PrefixSumIndexOfResult::new(index as usize, remainder)
}

#[test]
fn comprehensive_set_value_and_get_index_of() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    for_both(&[1, 1, 2, 1, 3], |psc| {
        assert_eq!(psc.get_total_sum(), 8);
        assert_eq!(psc.get_prefix_sum(0), 0);
        assert_eq!(psc.get_prefix_sum(1), 1);
        assert_eq!(psc.get_prefix_sum(2), 2);
        assert_eq!(psc.get_prefix_sum(3), 4);
        assert_eq!(psc.get_prefix_sum(4), 5);
        assert_eq!(psc.get_prefix_sum(5), 8);
        assert_eq!(psc.get_index_of(0), r(0, 0));
        assert_eq!(psc.get_index_of(1), r(1, 0));
        assert_eq!(psc.get_index_of(2), r(2, 0));
        assert_eq!(psc.get_index_of(3), r(2, 1));
        assert_eq!(psc.get_index_of(4), r(3, 0));
        assert_eq!(psc.get_index_of(5), r(4, 0));
        assert_eq!(psc.get_index_of(6), r(4, 1));
        assert_eq!(psc.get_index_of(7), r(4, 2));
        assert_eq!(psc.get_index_of(8), r(4, 3));

        // [1, 2, 2, 1, 3]
        psc.set_value(1, 2);
        assert_eq!(psc.get_total_sum(), 9);
        assert_eq!(psc.get_prefix_sum(1), 1);
        assert_eq!(psc.get_prefix_sum(2), 3);
        assert_eq!(psc.get_prefix_sum(3), 5);
        assert_eq!(psc.get_prefix_sum(4), 6);
        assert_eq!(psc.get_prefix_sum(5), 9);

        // [1, 0, 2, 1, 3]
        psc.set_value(1, 0);
        assert_eq!(psc.get_total_sum(), 7);
        assert_eq!(psc.get_prefix_sum(1), 1);
        assert_eq!(psc.get_prefix_sum(2), 1);
        assert_eq!(psc.get_prefix_sum(3), 3);
        assert_eq!(psc.get_prefix_sum(4), 4);
        assert_eq!(psc.get_prefix_sum(5), 7);
        assert_eq!(psc.get_index_of(0), r(0, 0));
        assert_eq!(psc.get_index_of(1), r(2, 0));
        assert_eq!(psc.get_index_of(2), r(2, 1));
        assert_eq!(psc.get_index_of(3), r(3, 0));
        assert_eq!(psc.get_index_of(4), r(4, 0));
        assert_eq!(psc.get_index_of(5), r(4, 1));
        assert_eq!(psc.get_index_of(6), r(4, 2));

        // [1, 0, 0, 1, 3]
        psc.set_value(2, 0);
        assert_eq!(psc.get_total_sum(), 5);
        assert_eq!(psc.get_prefix_sum(1), 1);
        assert_eq!(psc.get_prefix_sum(2), 1);
        assert_eq!(psc.get_prefix_sum(3), 1);
        assert_eq!(psc.get_prefix_sum(4), 2);
        assert_eq!(psc.get_prefix_sum(5), 5);
        assert_eq!(psc.get_index_of(0), r(0, 0));
        assert_eq!(psc.get_index_of(1), r(3, 0));
        assert_eq!(psc.get_index_of(2), r(4, 0));
        assert_eq!(psc.get_index_of(3), r(4, 1));
        assert_eq!(psc.get_index_of(4), r(4, 2));

        // [1, 0, 0, 0, 3]
        psc.set_value(3, 0);
        assert_eq!(psc.get_total_sum(), 4);
        assert_eq!(psc.get_prefix_sum(1), 1);
        assert_eq!(psc.get_prefix_sum(2), 1);
        assert_eq!(psc.get_prefix_sum(3), 1);
        assert_eq!(psc.get_prefix_sum(4), 1);
        assert_eq!(psc.get_prefix_sum(5), 4);
        assert_eq!(psc.get_index_of(0), r(0, 0));
        assert_eq!(psc.get_index_of(1), r(4, 0));
        assert_eq!(psc.get_index_of(2), r(4, 1));
        assert_eq!(psc.get_index_of(3), r(4, 2));

        // [1, 1, 0, 1, 1]
        psc.set_value(1, 1);
        psc.set_value(3, 1);
        psc.set_value(4, 1);
        assert_eq!(psc.get_total_sum(), 4);
        assert_eq!(psc.get_prefix_sum(1), 1);
        assert_eq!(psc.get_prefix_sum(2), 2);
        assert_eq!(psc.get_prefix_sum(3), 2);
        assert_eq!(psc.get_prefix_sum(4), 3);
        assert_eq!(psc.get_prefix_sum(5), 4);
        assert_eq!(psc.get_index_of(0), r(0, 0));
        assert_eq!(psc.get_index_of(1), r(1, 0));
        assert_eq!(psc.get_index_of(2), r(3, 0));
        assert_eq!(psc.get_index_of(3), r(4, 0));
    });
}

#[test]
fn get_total_sum_with_typical_values() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    for_both(&[1, 1, 2, 1, 3], |psc| assert_eq!(psc.get_total_sum(), 8));
    for_both(&[10], |psc| assert_eq!(psc.get_total_sum(), 10));
    for_both(&[5, 5, 5], |psc| assert_eq!(psc.get_total_sum(), 15));
}

#[test]
fn get_total_sum_with_all_zeroes() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    for_both(&[0, 0, 0], |psc| assert_eq!(psc.get_total_sum(), 0));
    for_both(&[0], |psc| assert_eq!(psc.get_total_sum(), 0));
}

#[test]
fn get_total_sum_with_empty_array() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    for_both(&[], |psc| assert_eq!(psc.get_total_sum(), 0));
}

#[test]
fn get_total_sum_with_single_element() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    for_both(&[0], |psc| assert_eq!(psc.get_total_sum(), 0));
    for_both(&[1], |psc| assert_eq!(psc.get_total_sum(), 1));
    for_both(&[100], |psc| assert_eq!(psc.get_total_sum(), 100));
}

#[test]
fn get_prefix_sum_with_typical_values() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    for_both(&[1, 1, 2, 1, 3], |psc| {
        assert_eq!(psc.get_prefix_sum(0), 0);
        assert_eq!(psc.get_prefix_sum(1), 1);
        assert_eq!(psc.get_prefix_sum(2), 2);
        assert_eq!(psc.get_prefix_sum(3), 4);
        assert_eq!(psc.get_prefix_sum(4), 5);
        assert_eq!(psc.get_prefix_sum(5), 8);
    });
}

#[test]
fn get_prefix_sum_with_all_zeroes() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    for_both(&[0, 0, 0], |psc| {
        assert_eq!(psc.get_prefix_sum(0), 0);
        assert_eq!(psc.get_prefix_sum(1), 0);
        assert_eq!(psc.get_prefix_sum(2), 0);
        assert_eq!(psc.get_prefix_sum(3), 0);
    });
}

#[test]
fn get_prefix_sum_with_single_element() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    for_both(&[7], |psc| {
        assert_eq!(psc.get_prefix_sum(0), 0);
        assert_eq!(psc.get_prefix_sum(1), 7);
    });
}

#[test]
fn get_prefix_sum_with_empty_array() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    for_both(&[], |psc| {
        assert_eq!(psc.get_prefix_sum(0), 0);
    });
}

#[test]
fn get_prefix_sum_with_leading_trailing_zeroes() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    for_both(&[0, 0, 3, 0, 0], |psc| {
        assert_eq!(psc.get_prefix_sum(0), 0);
        assert_eq!(psc.get_prefix_sum(1), 0);
        assert_eq!(psc.get_prefix_sum(2), 0);
        assert_eq!(psc.get_prefix_sum(3), 3);
        assert_eq!(psc.get_prefix_sum(4), 3);
        assert_eq!(psc.get_prefix_sum(5), 3);
    });
}

#[test]
fn get_index_of_with_typical_values() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    for_both(&[1, 1, 2, 1, 3], |psc| {
        assert_eq!(psc.get_index_of(0), r(0, 0));
        assert_eq!(psc.get_index_of(1), r(1, 0));
        assert_eq!(psc.get_index_of(2), r(2, 0));
        assert_eq!(psc.get_index_of(3), r(2, 1));
        assert_eq!(psc.get_index_of(4), r(3, 0));
        assert_eq!(psc.get_index_of(5), r(4, 0));
        assert_eq!(psc.get_index_of(6), r(4, 1));
        assert_eq!(psc.get_index_of(7), r(4, 2));
        assert_eq!(psc.get_index_of(8), r(4, 3));
    });
}

#[test]
fn get_index_of_with_all_zeroes() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    for_both(&[0, 0, 0], |psc| {
        assert_eq!(psc.get_index_of(0), r(2, 0));
    });
}

#[test]
fn get_index_of_with_single_zero() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    for_both(&[0], |psc| {
        assert_eq!(psc.get_index_of(0), r(0, 0));
    });
}

#[test]
fn get_index_of_with_single_element() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    for_both(&[5], |psc| {
        assert_eq!(psc.get_index_of(0), r(0, 0));
        assert_eq!(psc.get_index_of(1), r(0, 1));
        assert_eq!(psc.get_index_of(4), r(0, 4));
    });
}

#[test]
fn get_index_of_with_leading_zeroes() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    for_both(&[0, 0, 3], |psc| {
        assert_eq!(psc.get_index_of(0), r(2, 0));
        assert_eq!(psc.get_index_of(1), r(2, 1));
        assert_eq!(psc.get_index_of(2), r(2, 2));
    });
}

#[test]
fn get_index_of_with_trailing_zeroes() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    for_both(&[3, 0, 0], |psc| {
        assert_eq!(psc.get_index_of(0), r(0, 0));
        assert_eq!(psc.get_index_of(1), r(0, 1));
        assert_eq!(psc.get_index_of(2), r(0, 2));
    });
}

#[test]
fn get_index_of_with_interleaved_zeroes() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    for_both(&[0, 1, 0, 2, 0], |psc| {
        assert_eq!(psc.get_index_of(0), r(1, 0));
        assert_eq!(psc.get_index_of(1), r(3, 0));
        assert_eq!(psc.get_index_of(2), r(3, 1));
    });
}

#[test]
fn get_index_of_with_all_ones() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    for_both(&[1, 1, 1, 1, 1], |psc| {
        for i in 0..5 {
            assert_eq!(psc.get_index_of(i), r(i, 0));
        }
    });
}

#[test]
fn get_index_of_with_large_value_in_single_element() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    for_both(&[1000], |psc| {
        assert_eq!(psc.get_index_of(0), r(0, 0));
        assert_eq!(psc.get_index_of(500), r(0, 500));
        assert_eq!(psc.get_index_of(999), r(0, 999));
    });
}

#[test]
fn set_value_no_op_when_value_unchanged() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    for_both(&[1, 2, 3], |psc| {
        assert_eq!(psc.get_total_sum(), 6);
        psc.set_value(1, 2);
        assert_eq!(psc.get_total_sum(), 6);
    });
}

#[test]
fn set_value_increase() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    for_both(&[1, 2, 3], |psc| {
        psc.set_value(1, 5);
        assert_eq!(psc.get_total_sum(), 9);
        assert_eq!(psc.get_prefix_sum(2), 6);
        assert_eq!(psc.get_prefix_sum(3), 9);
        assert_eq!(psc.get_index_of(0), r(0, 0));
        assert_eq!(psc.get_index_of(1), r(1, 0));
        assert_eq!(psc.get_index_of(5), r(1, 4));
        assert_eq!(psc.get_index_of(6), r(2, 0));
    });
}

#[test]
fn set_value_decrease() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    for_both(&[1, 5, 3], |psc| {
        psc.set_value(1, 2);
        assert_eq!(psc.get_total_sum(), 6);
        assert_eq!(psc.get_index_of(1), r(1, 0));
        assert_eq!(psc.get_index_of(2), r(1, 1));
        assert_eq!(psc.get_index_of(3), r(2, 0));
    });
}

#[test]
fn set_value_to_zero() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    for_both(&[1, 2, 3], |psc| {
        psc.set_value(1, 0);
        assert_eq!(psc.get_total_sum(), 4);
        assert_eq!(psc.get_prefix_sum(2), 1);
        assert_eq!(psc.get_index_of(0), r(0, 0));
        assert_eq!(psc.get_index_of(1), r(2, 0));
    });
}

#[test]
fn set_value_from_zero() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    for_both(&[0, 0, 0], |psc| {
        psc.set_value(1, 3);
        assert_eq!(psc.get_total_sum(), 3);
        assert_eq!(psc.get_index_of(0), r(1, 0));
        assert_eq!(psc.get_index_of(2), r(1, 2));
    });
}

#[test]
fn set_value_on_first_element() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    for_both(&[1, 2, 3], |psc| {
        psc.set_value(0, 10);
        assert_eq!(psc.get_total_sum(), 15);
        assert_eq!(psc.get_prefix_sum(1), 10);
        assert_eq!(psc.get_index_of(0), r(0, 0));
        assert_eq!(psc.get_index_of(9), r(0, 9));
        assert_eq!(psc.get_index_of(10), r(1, 0));
    });
}

#[test]
fn set_value_on_last_element() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    for_both(&[1, 2, 3], |psc| {
        psc.set_value(2, 10);
        assert_eq!(psc.get_total_sum(), 13);
        assert_eq!(psc.get_index_of(3), r(2, 0));
        assert_eq!(psc.get_index_of(12), r(2, 9));
    });
}

#[test]
fn set_all_values_to_zero_then_restore() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    for_both(&[1, 2, 3], |psc| {
        psc.set_value(0, 0);
        psc.set_value(1, 0);
        psc.set_value(2, 0);
        assert_eq!(psc.get_total_sum(), 0);
        assert_eq!(psc.get_index_of(0), r(2, 0));

        psc.set_value(0, 4);
        assert_eq!(psc.get_total_sum(), 4);
        assert_eq!(psc.get_index_of(0), r(0, 0));
        assert_eq!(psc.get_index_of(3), r(0, 3));
    });
}

#[test]
fn set_value_multiple_times_on_same_index() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    for_both(&[1, 1, 1], |psc| {
        psc.set_value(1, 5);
        psc.set_value(1, 2);
        psc.set_value(1, 10);
        assert_eq!(psc.get_total_sum(), 12);
        assert_eq!(psc.get_index_of(0), r(0, 0));
        assert_eq!(psc.get_index_of(1), r(1, 0));
        assert_eq!(psc.get_index_of(10), r(1, 9));
        assert_eq!(psc.get_index_of(11), r(2, 0));
    });
}

#[test]
fn insert_values_at_beginning() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    for_both(&[3, 4], |psc| {
        psc.insert_values(0, &[1, 2]);
        assert_eq!(psc.get_total_sum(), 10);
        assert_eq!(psc.get_prefix_sum(1), 1);
        assert_eq!(psc.get_prefix_sum(2), 3);
        assert_eq!(psc.get_prefix_sum(3), 6);
        assert_eq!(psc.get_prefix_sum(4), 10);
        assert_eq!(psc.get_index_of(0), r(0, 0));
        assert_eq!(psc.get_index_of(1), r(1, 0));
        assert_eq!(psc.get_index_of(3), r(2, 0));
    });
}

#[test]
fn insert_values_at_end() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    for_both(&[1, 2], |psc| {
        psc.insert_values(2, &[3, 4]);
        assert_eq!(psc.get_total_sum(), 10);
        assert_eq!(psc.get_prefix_sum(3), 6);
        assert_eq!(psc.get_prefix_sum(4), 10);
    });
}

#[test]
fn insert_values_in_the_middle() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    for_both(&[1, 4], |psc| {
        psc.insert_values(1, &[2, 3]);
        assert_eq!(psc.get_total_sum(), 10);
        assert_eq!(psc.get_prefix_sum(1), 1);
        assert_eq!(psc.get_prefix_sum(2), 3);
        assert_eq!(psc.get_prefix_sum(3), 6);
        assert_eq!(psc.get_prefix_sum(4), 10);
    });
}

#[test]
fn insert_values_with_zeroes() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    for_both(&[1, 2], |psc| {
        psc.insert_values(1, &[0, 0]);
        assert_eq!(psc.get_total_sum(), 3);
        assert_eq!(psc.get_prefix_sum(1), 1);
        assert_eq!(psc.get_prefix_sum(2), 1);
        assert_eq!(psc.get_prefix_sum(3), 1);
        assert_eq!(psc.get_prefix_sum(4), 3);
    });
}

#[test]
fn insert_values_into_all_zeroes() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    for_both(&[0, 0, 0], |psc| {
        psc.insert_values(1, &[2, 3]);
        assert_eq!(psc.get_total_sum(), 5);
        assert_eq!(psc.get_prefix_sum(1), 0);
        assert_eq!(psc.get_prefix_sum(2), 2);
        assert_eq!(psc.get_prefix_sum(3), 5);
        assert_eq!(psc.get_index_of(0), r(1, 0));
        assert_eq!(psc.get_index_of(2), r(2, 0));
        assert_eq!(psc.get_index_of(4), r(2, 2));
    });
}

#[test]
fn insert_values_into_empty_computer() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    for_both(&[], |psc| {
        psc.insert_values(0, &[5, 3]);
        assert_eq!(psc.get_total_sum(), 8);
        assert_eq!(psc.get_index_of(0), r(0, 0));
        assert_eq!(psc.get_index_of(4), r(0, 4));
        assert_eq!(psc.get_index_of(5), r(1, 0));
    });
}

#[test]
fn remove_values_from_beginning() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    for_both(&[1, 2, 3, 4], |psc| {
        psc.remove_values(0, 2);
        assert_eq!(psc.get_total_sum(), 7);
        assert_eq!(psc.get_prefix_sum(1), 3);
        assert_eq!(psc.get_prefix_sum(2), 7);
        assert_eq!(psc.get_index_of(0), r(0, 0));
        assert_eq!(psc.get_index_of(3), r(1, 0));
    });
}

#[test]
fn remove_values_from_end() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    for_both(&[1, 2, 3, 4], |psc| {
        psc.remove_values(2, 2);
        assert_eq!(psc.get_total_sum(), 3);
        assert_eq!(psc.get_prefix_sum(1), 1);
        assert_eq!(psc.get_prefix_sum(2), 3);
    });
}

#[test]
fn remove_values_from_the_middle() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    for_both(&[1, 2, 3, 4], |psc| {
        psc.remove_values(1, 2);
        assert_eq!(psc.get_total_sum(), 5);
        assert_eq!(psc.get_prefix_sum(1), 1);
        assert_eq!(psc.get_prefix_sum(2), 5);
        assert_eq!(psc.get_index_of(0), r(0, 0));
        assert_eq!(psc.get_index_of(1), r(1, 0));
        assert_eq!(psc.get_index_of(4), r(1, 3));
    });
}

#[test]
fn remove_values_all() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    for_both(&[1, 2, 3], |psc| {
        psc.remove_values(0, 3);
        assert_eq!(psc.get_total_sum(), 0);
    });
}

#[test]
fn remove_values_single_element() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    for_both(&[5, 10, 15], |psc| {
        psc.remove_values(1, 1);
        assert_eq!(psc.get_total_sum(), 20);
        assert_eq!(psc.get_index_of(0), r(0, 0));
        assert_eq!(psc.get_index_of(5), r(1, 0));
        assert_eq!(psc.get_index_of(19), r(1, 14));
    });
}

#[test]
fn remove_values_zero_valued_elements() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    for_both(&[0, 0, 5, 0, 0], |psc| {
        psc.remove_values(0, 2);
        assert_eq!(psc.get_total_sum(), 5);
        assert_eq!(psc.get_index_of(0), r(0, 0));
        assert_eq!(psc.get_index_of(4), r(0, 4));
    });
}

#[test]
fn insert_then_remove() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    for_both(&[1, 2, 3], |psc| {
        psc.insert_values(1, &[10, 20]);
        assert_eq!(psc.get_total_sum(), 36);
        psc.remove_values(1, 2);
        assert_eq!(psc.get_total_sum(), 6);
        assert_eq!(psc.get_index_of(0), r(0, 0));
        assert_eq!(psc.get_index_of(1), r(1, 0));
        assert_eq!(psc.get_index_of(3), r(2, 0));
    });
}

#[test]
fn remove_then_insert_at_same_position() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    for_both(&[1, 2, 3], |psc| {
        psc.remove_values(1, 1);
        psc.insert_values(1, &[5]);
        assert_eq!(psc.get_total_sum(), 9);
        assert_eq!(psc.get_index_of(1), r(1, 0));
        assert_eq!(psc.get_index_of(5), r(1, 4));
        assert_eq!(psc.get_index_of(6), r(2, 0));
    });
}

#[test]
fn set_value_then_insert_then_remove() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    for_both(&[1, 1, 1], |psc| {
        psc.set_value(0, 5);
        psc.insert_values(1, &[10]);
        psc.remove_values(3, 1);
        // [5, 10, 1]
        assert_eq!(psc.get_total_sum(), 16);
        assert_eq!(psc.get_index_of(0), r(0, 0));
        assert_eq!(psc.get_index_of(4), r(0, 4));
        assert_eq!(psc.get_index_of(5), r(1, 0));
        assert_eq!(psc.get_index_of(14), r(1, 9));
        assert_eq!(psc.get_index_of(15), r(2, 0));
    });
}

#[test]
fn multiple_queries_between_mutations_are_consistent() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    for_both(&[2, 3, 5], |psc| {
        assert_eq!(psc.get_total_sum(), 10);
        assert_eq!(psc.get_index_of(0), r(0, 0));

        psc.set_value(1, 0);
        assert_eq!(psc.get_total_sum(), 7);
        assert_eq!(psc.get_index_of(2), r(2, 0));

        psc.set_value(1, 3);
        assert_eq!(psc.get_total_sum(), 10);
        assert_eq!(psc.get_index_of(2), r(1, 0));
    });
}

#[test]
fn large_values() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    for_both(&[100, 200, 300], |psc| {
        assert_eq!(psc.get_total_sum(), 600);
        assert_eq!(psc.get_prefix_sum(1), 100);
        assert_eq!(psc.get_prefix_sum(2), 300);
        assert_eq!(psc.get_prefix_sum(3), 600);
        assert_eq!(psc.get_index_of(0), r(0, 0));
        assert_eq!(psc.get_index_of(99), r(0, 99));
        assert_eq!(psc.get_index_of(100), r(1, 0));
        assert_eq!(psc.get_index_of(299), r(1, 199));
        assert_eq!(psc.get_index_of(300), r(2, 0));
        assert_eq!(psc.get_index_of(599), r(2, 299));
    });
}

#[test]
fn many_elements() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    for_both(&[1; 100], |psc| {
        assert_eq!(psc.get_total_sum(), 100);
        assert_eq!(psc.get_prefix_sum(50), 50);

        for i in 0..100 {
            assert_eq!(psc.get_index_of(i), r(i, 0));
        }
    });
}

#[test]
fn many_elements_all_zeroes() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    for_both(&[0; 100], |psc| {
        assert_eq!(psc.get_total_sum(), 0);
        for i in 0..=100 {
            assert_eq!(psc.get_prefix_sum(i), 0);
        }
        assert_eq!(psc.get_index_of(0), r(99, 0));
    });
}

#[test]
fn set_value_between_queries_re_validates_correctly() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    for_both(&[1, 1, 1, 1, 1], |psc| {
        assert_eq!(psc.get_total_sum(), 5);

        psc.set_value(2, 10);
        assert_eq!(psc.get_total_sum(), 14);
        assert_eq!(psc.get_prefix_sum(3), 12);
        assert_eq!(psc.get_index_of(2), r(2, 0));
        assert_eq!(psc.get_index_of(11), r(2, 9));
        assert_eq!(psc.get_index_of(12), r(3, 0));
        assert_eq!(psc.get_index_of(13), r(4, 0));

        psc.set_value(0, 0);
        assert_eq!(psc.get_total_sum(), 13);
        assert_eq!(psc.get_index_of(0), r(1, 0));
    });
}
