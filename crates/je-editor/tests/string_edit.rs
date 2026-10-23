//! Porte de `src/vs/editor/test/common/core/stringEdit.test.ts` (suíte `Edit`), caso a caso e na mesma ordem.
//!
//! Os laços `for (let i = 0; i < N; i++) { test('case' + i, ...) }` viram um `#[test]` por suíte que percorre
//! as mesmas sementes em ordem (a semente vai na mensagem de falha). Os `test.skip('fuzz')` viram `#[ignore]`.
//! `ArrayEdit` usa `u16` (unidades UTF-16) onde o original usa os caracteres do `[...texto]`; o texto do
//! teste é ASCII, então são os mesmos elementos.

#[path = "../../je-base/tests/common/mod.rs"]
mod common;
#[path = "support/random.rs"]
mod random;

use common::ensure_no_disposables_are_leaked_in_test_suite;
use je_base::JsString;
use je_editor::edits::array_edit::{ArrayEdit, ArrayReplacement};
use je_editor::edits::edit::{BaseEdit, BaseReplacement};
use je_editor::edits::string_edit::{BaseStringEdit, StringEdit, StringReplacement};
use je_editor::offset_range::OffsetRange;
use random::Random;

fn js(text: &str) -> JsString {
    JsString::from(text)
}

fn get_random_edit(str: &[u16], count: i64, rng: &mut Random) -> StringEdit {
    let mut edits: Vec<StringReplacement> = Vec::new();
    let mut i: i64 = 0;
    for j in 0..count as usize {
        if i >= str.len() as i64 {
            break;
        }
        edits.push(get_random_single_edit(str, i, rng));
        i = i64::from(edits[j].replace_range.end_exclusive) + 1;
    }
    StringEdit::create(edits)
}

fn get_random_single_edit(str: &[u16], range_offset_start: i64, rng: &mut Random) -> StringReplacement {
    let len = str.len() as i64;
    let offset_start = rng.next_int_range(range_offset_start, len);
    let offset_end = rng.next_int_range(offset_start, len);

    let text_start = rng.next_int_range(0, len);
    let text_len = rng.next_int_range(0, 7i64.min(len - text_start));

    StringReplacement::new(
        OffsetRange::new(offset_start as i32, offset_end as i32),
        JsString::from(&str[text_start as usize..(text_start + text_len) as usize]),
    )
}

const S0: &str = "abcde\nfghij\nklmno\npqrst\n";

// suite('StringEdit')

#[test]
fn string_edit_basic() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    let arr = js("0123456789");
    let edit = StringEdit::replace(OffsetRange::new(4, 6), "xyz");
    let result = edit.apply(arr.units());
    assert_eq!(result, js("0123xyz6789"));
}

// suite('inverse')

fn string_edit_inverse_run_test(seed: u32) {
    let mut rng = Random::create(seed);

    let s0 = js(S0);

    let count = rng.next_int_range(1, 4);
    let e = get_random_edit(s0.units(), count, &mut rng);
    let e_inv = e.inverse(s0.units());

    assert_eq!(e_inv.apply(e.apply(s0.units()).units()), s0, "seed {seed}");
}

#[test]
fn string_edit_inverse_cases() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    for i in 0..1000 {
        string_edit_inverse_run_test(i);
    }
}

#[test]
#[ignore = "test.skip no original"]
fn string_edit_inverse_fuzz() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    for i in 0..1_000_000 {
        string_edit_inverse_run_test(i);
    }
}

// suite('compose')

fn string_edit_compose_run_test(seed: u32) {
    let mut rng = Random::create(seed);

    let s0 = js(S0);

    let count = rng.next_int_range(1, 4);
    let edits1 = get_random_edit(s0.units(), count, &mut rng);
    let s1 = edits1.apply(s0.units());

    let count = rng.next_int_range(1, 4);
    let edits2 = get_random_edit(s1.units(), count, &mut rng);
    let s2 = edits2.apply(s1.units());

    let combined_edits = edits1.compose(&edits2);
    let s2_c = combined_edits.apply(s0.units());

    assert_eq!(s2_c, s2, "seed {seed}");
}

#[test]
fn string_edit_compose_cases() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    for i in 0..1000 {
        string_edit_compose_run_test(i);
    }
}

#[test]
#[ignore = "test.skip no original"]
fn string_edit_compose_fuzz() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    for i in 0..1_000_000 {
        string_edit_compose_run_test(i);
    }
}

#[test]
fn string_edit_equals() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    let edit1 = StringEdit::replace(OffsetRange::new(4, 6), "xyz");
    let edit2 = StringEdit::replace(OffsetRange::new(4, 6), "xyz");
    let edit3 = StringEdit::replace(OffsetRange::new(5, 6), "xyz");
    let edit4 = StringEdit::replace(OffsetRange::new(4, 6), "xy");

    assert!(edit1.equals(&edit1));
    assert!(edit1.equals(&edit2));
    assert!(edit2.equals(&edit1));

    assert!(!edit1.equals(&edit3));
    assert!(!edit1.equals(&edit4));
}

#[test]
fn string_edit_get_new_ranges() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    let edit = StringEdit::create(vec![
        StringReplacement::new(OffsetRange::new(4, 6), "abcde"),
        StringReplacement::new(OffsetRange::new(7, 9), "a"),
    ]);
    let ranges = edit.get_new_ranges();
    assert_eq!(ranges, vec![OffsetRange::new(4, 9), OffsetRange::new(10, 11)]);
}

#[test]
fn string_edit_get_joined_replace_range() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    let edit = StringEdit::create(vec![
        StringReplacement::new(OffsetRange::new(4, 6), "abcde"),
        StringReplacement::new(OffsetRange::new(7, 9), "a"),
    ]);
    let range = edit.get_joined_replace_range();
    assert_eq!(range, Some(OffsetRange::new(4, 9)));
}

#[test]
fn string_edit_get_length_delta() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    let edit = StringEdit::create(vec![
        StringReplacement::new(OffsetRange::new(4, 6), "abcde"),
        StringReplacement::new(OffsetRange::new(7, 9), "a"),
    ]);
    let delta = edit.get_length_delta();
    assert_eq!(delta, 2);
    assert_eq!(edit.replacements()[0].get_length_delta(), 3);
    assert_eq!(edit.replacements()[1].get_length_delta(), -1);
}

#[test]
fn string_edit_adjacent_edit_and_insert_should_rebase_successfully() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    // A replacement ending at X followed by an insert at X should not conflict
    let first_edit = StringEdit::create(vec![StringReplacement::replace(
        OffsetRange::new(1826, 1838),
        "function fib(n: number): number {",
    )]);
    let followup_edit = StringEdit::create(vec![StringReplacement::replace(
        OffsetRange::new(1838, 1838),
        "\n\tif (n <= 1) {\n\t\treturn n;\n\t}\n\treturn fib(n - 1) + fib(n - 2);\n}",
    )]);
    let rebased_edit = followup_edit.try_rebase(&first_edit);

    // Since firstEdit replaces [1826, 1838) with text of length 33,
    // the insert at 1838 should be rebased to 1826 + 33 = 1859
    let rebased_edit = rebased_edit.expect("a rebase deveria resolver");
    assert_eq!(rebased_edit.replacements()[0].replace_range.start, 1859);
    assert_eq!(rebased_edit.replacements()[0].replace_range.end_exclusive, 1859);
}

#[test]
fn string_edit_concurrent_inserts_at_same_position_should_conflict() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    // Two inserts at the exact same position conflict because order matters
    let first_edit = StringEdit::create(vec![StringReplacement::replace(OffsetRange::new(1838, 1838), "1")]);
    let followup_edit = StringEdit::create(vec![StringReplacement::replace(OffsetRange::new(1838, 1838), "2")]);
    let rebased_edit = followup_edit.try_rebase(&first_edit);

    // This should return undefined because both are inserts at the same position
    assert!(rebased_edit.is_none());
}

#[test]
fn string_edit_try_rebase_should_return_undefined_when_rebasing_would_produce_non_disjoint_edits_negative_offset_case() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    // ourEdit1: [100, 110) -> "A"
    // ourEdit2: [120, 120) -> "B"
    // baseEdit: [110, 125) -> "" (delete 15 chars, offset = -15)
    // After transformation, ourEdit2 at [105, 105) < ourEdit1 end (110)

    let our_edit = StringEdit::create(vec![
        StringReplacement::new(OffsetRange::new(100, 110), "A"),
        StringReplacement::new(OffsetRange::empty_at(120), "B"),
    ]);

    let base_edit = StringEdit::create(vec![StringReplacement::new(OffsetRange::new(110, 125), "")]);

    let result = our_edit.try_rebase(&base_edit);
    assert!(result.is_none());
}

#[test]
fn string_edit_try_rebase_should_succeed_when_edits_remain_disjoint_after_rebasing() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    // ourEdit1: [100, 110) -> "A"
    // ourEdit2: [200, 210) -> "B"
    // baseEdit: [50, 60) -> "" (delete 10 chars, offset = -10)
    // After: ourEdit1 at [90, 100), ourEdit2 at [190, 200) - still disjoint

    let our_edit = StringEdit::create(vec![
        StringReplacement::new(OffsetRange::new(100, 110), "A"),
        StringReplacement::new(OffsetRange::new(200, 210), "B"),
    ]);

    let base_edit = StringEdit::create(vec![StringReplacement::new(OffsetRange::new(50, 60), "")]);

    let result = our_edit.try_rebase(&base_edit).expect("a rebase deveria resolver");
    assert_eq!(result.replacements()[0].replace_range.start, 90);
    assert_eq!(result.replacements()[1].replace_range.start, 190);
}

#[test]
fn string_edit_rebase_skip_conflicting_should_skip_edits_that_would_produce_non_disjoint_results() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    let our_edit = StringEdit::create(vec![
        StringReplacement::new(OffsetRange::new(100, 110), "A"),
        StringReplacement::new(OffsetRange::empty_at(120), "B"),
    ]);

    let base_edit = StringEdit::create(vec![StringReplacement::new(OffsetRange::new(110, 125), "")]);

    // Should not throw, and should skip the conflicting edit
    let result = our_edit.rebase_skip_conflicting(&base_edit);
    assert_eq!(result.replacements().len(), 1);
    assert_eq!(result.replacements()[0].replace_range.start, 100);
}

// suite('ArrayEdit')

#[test]
fn array_edit_basic() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    let arr: Vec<char> = "0123456789".chars().collect();
    let edit = ArrayEdit::replace(OffsetRange::new(4, 6), vec!['x', 'y', 'z']);
    let result = edit.apply(&arr);
    assert_eq!(result, "0123xyz6789".chars().collect::<Vec<char>>());
}

// suite('compose')

fn array_edit_compose_run_test(seed: u32) {
    let mut rng = Random::create(seed);

    let s0 = js(S0);

    let count = rng.next_int_range(1, 4);
    let e1 = get_random_edit(s0.units(), count, &mut rng);
    let s1 = e1.apply(s0.units());

    let count = rng.next_int_range(1, 4);
    let e2 = get_random_edit(s1.units(), count, &mut rng);

    let ae1 = ArrayEdit::create(
        e1.replacements()
            .iter()
            .map(|r| ArrayReplacement::new(r.replace_range, r.new_text.units().to_vec()))
            .collect(),
    );
    let ae2 = ArrayEdit::create(
        e2.replacements()
            .iter()
            .map(|r| ArrayReplacement::new(r.replace_range, r.new_text.units().to_vec()))
            .collect(),
    );
    let as0 = s0.units().to_vec();
    let as1 = ae1.apply(&as0);
    let as2 = ae2.apply(&as1);
    let a_combined_edits = ae1.compose(&ae2);

    let as2_c = a_combined_edits.apply(&as0);
    assert_eq!(as2, as2_c, "seed {seed}");
}

#[test]
fn array_edit_compose_cases() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    for i in 0..100 {
        array_edit_compose_run_test(i);
    }
}

// suite('tryRebase invariants')

fn try_rebase_invariants_run_test(seed: u32) {
    let mut rng = Random::create(seed);
    let s0 = js(S0);

    let count = rng.next_int_range(1, 4);
    let e1 = get_random_edit(s0.units(), count, &mut rng);
    let count = rng.next_int_range(1, 4);
    let e2 = get_random_edit(s0.units(), count, &mut rng);

    let e1_rebased_on_e2 = e1.try_rebase(&e2);
    let e2_rebased_on_e1 = e2.try_rebase(&e1);

    // Invariant 1: e1.rebase(e2) != undefined <=> e2.rebase(e1) != undefined
    assert_eq!(
        e1_rebased_on_e2.is_some(),
        e2_rebased_on_e1.is_some(),
        "Symmetry violated: e1.rebase(e2)={}, e2.rebase(e1)={}",
        e1_rebased_on_e2.is_some(),
        e2_rebased_on_e1.is_some()
    );

    // Invariant 2: e1.rebase(e2) != undefined => e1.compose(e2.rebase(e1)) = e2.compose(e1.rebase(e2))
    if let (Some(e1_rebased_on_e2), Some(e2_rebased_on_e1)) = (&e1_rebased_on_e2, &e2_rebased_on_e1) {
        let path1 = e1.compose(e2_rebased_on_e1);
        let path2 = e2.compose(e1_rebased_on_e2);

        // Both paths should produce the same result when applied to s0
        let result1 = path1.apply(s0.units());
        let result2 = path2.apply(s0.units());
        assert_eq!(result1, result2, "Diamond property violated (seed {seed})");
    }

    // Invariant 3: empty.rebase(e) = empty
    let empty_rebased_on_e1 = StringEdit::empty().try_rebase(&e1);
    let empty_rebased_on_e1 = empty_rebased_on_e1.expect("empty.rebase(e) deveria resolver");
    assert!(empty_rebased_on_e1.is_empty());

    // Invariant 4: e.rebase(empty) = e
    let e1_rebased_on_empty = e1.try_rebase(&StringEdit::empty()).expect("e.rebase(empty) deveria resolver");
    assert!(e1.equals(&e1_rebased_on_empty), "e.rebase(empty) should equal e");

    // Invariant 5 (TP2): T(T(e3, e1), T(e2, e1)) = T(T(e3, e2), T(e1, e2))
    // For 3+ concurrent operations, transformation order shouldn't matter
    let count = rng.next_int_range(1, 4);
    let e3 = get_random_edit(s0.units(), count, &mut rng);

    let e2_on_e1 = e2.try_rebase(&e1);
    let e1_on_e2 = e1.try_rebase(&e2);
    let e3_on_e1 = e3.try_rebase(&e1);
    let e3_on_e2 = e3.try_rebase(&e2);

    if let (Some(e2_on_e1), Some(e1_on_e2), Some(e3_on_e1), Some(e3_on_e2)) = (e2_on_e1, e1_on_e2, e3_on_e1, e3_on_e2) {
        let path1 = e3_on_e1.try_rebase(&e2_on_e1); // T(T(e3, e1), T(e2, e1))
        let path2 = e3_on_e2.try_rebase(&e1_on_e2); // T(T(e3, e2), T(e1, e2))

        if let (Some(path1), Some(path2)) = (path1, path2) {
            assert!(path1.equals(&path2), "TP2 violated: transformation order matters (seed {seed})");
        }
    }
}

#[test]
fn try_rebase_invariants_cases() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    for i in 0..1000 {
        try_rebase_invariants_run_test(i);
    }
}
